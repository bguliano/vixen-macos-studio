import AppKit
import SwiftUI
import VixenBridge

extension KitMods {
    /// The modifiers of an event. ⌘ is upstream's Ctrl, ⌥ its Alt.
    public init(_ flags: NSEvent.ModifierFlags) {
        self.init(shift: flags.contains(.shift), command: flags.contains(.command), option: flags.contains(.option))
    }
}

extension NSCursor {
    /// What a Rust gesture asks for.
    public static func kit(_ cursor: KitCursor) -> NSCursor {
        switch cursor {
        case .arrow: .arrow
        case .cross: .crosshair
        case .resizeHorizontal: .resizeLeftRight
        case .resizeVertical: .resizeUpDown
        case .hand: .pointingHand
        }
    }
}

extension NSPasteboard.PasteboardType {
    public static let vixenCurve = NSPasteboard.PasteboardType("com.vixen.curve")
    public static let vixenGradient = NSPasteboard.PasteboardType("com.vixen.gradient")
    public static let vixenColor = NSPasteboard.PasteboardType("com.vixen.color")
}

/// What a value drag carries: from a strip, a swatch, a library list or a toolbar.
public enum VixenDragValue: Equatable, Sendable {
    case curve(CurveValue)
    case gradient(GradientValue)
    case color(UInt32)

    /// The text for the pasteboard. Rust owns the format.
    public func pasteboardItem() -> NSPasteboardItem {
        let item = NSPasteboardItem()
        switch self {
        case let .curve(value):
            item.setString(curveDragText(value: value), forType: .vixenCurve)
        case let .gradient(value):
            item.setString(gradientDragText(value: value), forType: .vixenGradient)
        case let .color(argb):
            item.setString(colorDragText(argb: argb), forType: .vixenColor)
            if let data = try? NSKeyedArchiver.archivedData(withRootObject: NSColor(argb: argb), requiringSecureCoding: true) {
                item.setData(data, forType: .color)
            }
        }
        return item
    }

    public static func read(from pasteboard: NSPasteboard) -> VixenDragValue? {
        if let text = pasteboard.string(forType: .vixenCurve), let value = curveFromDragText(text: text) {
            return .curve(value)
        }
        if let text = pasteboard.string(forType: .vixenGradient), let value = gradientFromDragText(text: text) {
            return .gradient(value)
        }
        if let text = pasteboard.string(forType: .vixenColor), let value = colorFromDragText(text: text) {
            return .color(value)
        }
        return nil
    }

    public static let pasteboardTypes: [NSPasteboard.PasteboardType] = [.vixenCurve, .vixenGradient, .vixenColor]
}

/// Numbers one continuous gesture, so all its commits become one undo step in Rust.
public enum KitGesture {
    private static var last: UInt64 = 0

    public static func next() -> UInt64 {
        last += 1
        return last
    }
}

/// F32's flush rule: a slider's or number strip's delayed commit goes to the selection it was made on, before the selection changes, the panel closes, a dialog opens or an undo runs.
@MainActor
public enum PendingCommits {
    private static var pending: [ObjectIdentifier: () -> Void] = [:]

    /// `flush` commits now.
    public static func set(_ owner: AnyObject, flush: @escaping () -> Void) {
        pending[ObjectIdentifier(owner)] = flush
    }

    public static func clear(_ owner: AnyObject) {
        pending[ObjectIdentifier(owner)] = nil
    }

    public static func flushAll() {
        let flushes = pending
        pending = [:]
        for flush in flushes.values {
            flush()
        }
    }
}

/// The base of the strips and swatches: a value drag out and drops in (F67, F68). A subclass starts a drag; the host decides what a drop means.
open class ValueDragView: NSView, NSDraggingSource {
    /// `nil` refuses everything.
    public var acceptsDrop: ((VixenDragValue) -> Bool)?
    public var performDrop: ((VixenDragValue) -> Void)?
    /// Any press, whatever it goes on to do: a list uses this to select the row (F58's FIX).
    public var onPressed: (() -> Void)?

    public override init(frame: NSRect) {
        super.init(frame: frame)
        registerForDraggedTypes(VixenDragValue.pasteboardTypes)
    }

    public required init?(coder: NSCoder) {
        fatalError("not used")
    }

    /// SwiftUI retains inactive dock tabs at zero opacity. Their native views
    /// must also leave AppKit's drag destination hit testing until shown again.
    public func setPanelVisible(_ visible: Bool) {
        isHidden = !visible
        if visible {
            if registeredDraggedTypes.isEmpty { registerForDraggedTypes(VixenDragValue.pasteboardTypes) }
        } else if !registeredDraggedTypes.isEmpty {
            unregisterDraggedTypes()
        }
    }

    /// Under the harness a window never becomes key, and a window that isn't key ignores a first click. Everywhere else it's the system's rule.
    open override func acceptsFirstMouse(for event: NSEvent?) -> Bool {
        UITestHarness.isActive
    }

    /// What a value drag from here carries, or `nil`. The test harness reads it, since it can't run a drag session.
    open var dragValue: VixenDragValue? { nil }

    /// `nil` when nothing should be dragged.
    open func dragImage() -> NSImage? {
        guard let rep = bitmapImageRepForCachingDisplay(in: bounds) else { return nil }
        cacheDisplay(in: bounds, to: rep)
        let image = NSImage(size: bounds.size)
        image.addRepresentation(rep)
        return image
    }

    public func startValueDrag(_ value: VixenDragValue, event: NSEvent) {
        let item = NSDraggingItem(pasteboardWriter: value.pasteboardItem())
        item.setDraggingFrame(bounds, contents: dragImage())
        beginDraggingSession(with: [item], event: event, source: self)
    }

    public func draggingSession(_ session: NSDraggingSession, sourceOperationMaskFor context: NSDraggingContext) -> NSDragOperation {
        .copy
    }

    open override func draggingEntered(_ sender: any NSDraggingInfo) -> NSDragOperation {
        operation(for: sender)
    }

    open override func draggingUpdated(_ sender: any NSDraggingInfo) -> NSDragOperation {
        operation(for: sender)
    }

    private func operation(for sender: any NSDraggingInfo) -> NSDragOperation {
        guard !isHiddenOrHasHiddenAncestor else { return [] }
        guard let value = VixenDragValue.read(from: sender.draggingPasteboard), acceptsDrop?(value) == true else { return [] }
        return .copy
    }

    open override func performDragOperation(_ sender: any NSDraggingInfo) -> Bool {
        guard !isHiddenOrHasHiddenAncestor else { return false }
        guard let value = VixenDragValue.read(from: sender.draggingPasteboard), acceptsDrop?(value) == true else { return false }
        performDrop?(value)
        return true
    }
}

private struct PanelIsShowingKey: EnvironmentKey {
    static let defaultValue = true
}

public extension EnvironmentValues {
    /// False inside a retained dock tab that is not currently showing.
    var panelIsShowing: Bool {
        get { self[PanelIsShowingKey.self] }
        set { self[PanelIsShowingKey.self] = newValue }
    }
}

/// The value tooltip that follows a drag (upstream's `ToolTip` with `Placement.Bottom`, and a slider's automatic one).
@MainActor
public final class ValueTip {
    public static let shared = ValueTip()
    private var panel: NSPanel?
    private let label = NSTextField(labelWithString: "")

    /// Shows `text` under `point` in `view`.
    public func show(_ text: String, near view: NSView, at point: NSPoint) {
        guard !UITestHarness.isActive, let window = view.window else { return }
        let panel = self.panel ?? makePanel()
        label.stringValue = text
        label.sizeToFit()
        let size = NSSize(width: label.frame.width + 12, height: label.frame.height + 6)
        label.setFrameOrigin(NSPoint(x: 6, y: 3))
        let screen = window.convertPoint(toScreen: view.convert(point, to: nil))
        panel.setFrame(NSRect(origin: NSPoint(x: screen.x + 8, y: screen.y - size.height - 8), size: size), display: true)
        panel.orderFront(nil)
    }

    public func hide() {
        panel?.orderOut(nil)
    }

    private func makePanel() -> NSPanel {
        let panel = NSPanel(contentRect: .zero, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: true)
        panel.isFloatingPanel = true
        panel.level = .floating
        panel.ignoresMouseEvents = true
        panel.hasShadow = true
        panel.isOpaque = false
        panel.backgroundColor = .windowBackgroundColor
        panel.hidesOnDeactivate = true
        label.font = .monospacedDigitSystemFont(ofSize: NSFont.smallSystemFontSize, weight: .regular)
        panel.contentView?.addSubview(label)
        self.panel = panel
        return panel
    }
}

/// Presents SwiftUI content as a sheet on a window. Rows are AppKit views with no SwiftUI state to hang a `.sheet` on, and a sheet can present another.
@MainActor
public enum KitSheet {
    /// `content` gets the function that closes the sheet.
    public static func present<Content: View>(on parent: NSWindow, @ViewBuilder content: (_ dismiss: @escaping () -> Void) -> Content) {
        var close: () -> Void = {}
        let host = NSHostingController(rootView: content({ close() }).studioAppearance()) // STUDIO: follow the chosen appearance
        let sheet = NSWindow(contentViewController: host)
        sheet.styleMask = [.titled]
        sheet.isReleasedWhenClosed = false
        close = { [weak parent, weak sheet] in
            guard let parent, let sheet else { return }
            parent.endSheet(sheet)
            sheet.orderOut(nil)
        }
        parent.beginSheet(sheet)
    }
}

/// Text input with verbs, for F78's and F93's name prompts: `TextDialog`.
struct NamePromptSheet: View {
    let prompt: String
    let onFinish: (String?) -> Void
    @State private var text: String

    /// F107: the loops reuse one dialog, so what was typed is still there after "Choose Another Name" or the empty-name error.
    init(prompt: String, initialText: String = "", onFinish: @escaping (String?) -> Void) {
        self.prompt = prompt
        self.onFinish = onFinish
        _text = State(initialValue: initialText)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(prompt)
            TextField("", text: $text)
                .frame(minWidth: 280)
                .onSubmit { onFinish(text) }
            HStack {
                Spacer()
                Button("Cancel") { onFinish(nil) }
                    .keyboardShortcut(.cancelAction)
                Button("OK") { onFinish(text) }
                    .keyboardShortcut(.defaultAction)
            }
        }
        .padding(20)
    }
}

/// An `NSAlert` as a sheet, with a verb per button. Returns the index of the button pressed.
@MainActor
public func askInSheet(on window: NSWindow, message: String, title: String, buttons: [String], destructiveIndex: Int? = nil, style: NSAlert.Style = .warning) async -> Int {
    let alert = NSAlert()
    alert.messageText = title
    alert.informativeText = message
    alert.alertStyle = style
    for (i, name) in buttons.enumerated() {
        let button = alert.addButton(withTitle: name)
        if i == destructiveIndex {
            button.hasDestructiveAction = true
        }
    }
    let response = await alert.beginSheetModal(for: window)
    return response.rawValue - NSApplication.ModalResponse.alertFirstButtonReturn.rawValue
}
