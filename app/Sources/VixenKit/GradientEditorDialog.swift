import AppKit
import SwiftUI
import VixenBridge

/// The Color Gradient Editor dialog, F87 to F93, as a sheet. It edits its own copy, in Rust; `onOK` gets the result and Cancel does nothing.
@MainActor
public enum GradientEditorDialog {
    public static func present(
        on window: NSWindow,
        core: VixenCore,
        gradient: GradientValue,
        discrete: Bool = false,
        validColors: [UInt32] = [],
        title: String? = nil,
        libraryName: String? = nil,
        onOK: @escaping (GradientValue) -> Void
    ) {
        KitSheet.present(on: window) { dismiss in
            GradientEditorContent(
                session: GradientEditorSession(gradient: gradient, libraryName: libraryName, discrete: discrete, validColors: validColors),
                core: core,
                rowTitle: title,
                onFinish: { result in
                    dismiss()
                    if let result {
                        onOK(result)
                    }
                }
            )
        }
    }

    /// `EditLibraryItemAsync`: with no discrete limit, whatever the row's targets are.
    public static func editLibraryEntry(on window: NSWindow, core: VixenCore, name: String, onDone: (() -> Void)? = nil) {
        guard let entry = core.libraryGradients().first(where: { $0.name == name }) else { return }
        present(on: window, core: core, gradient: entry.gradient, libraryName: name) { edited in
            _ = core.libraryAddGradient(name: name, gradient: edited)
            onDone?()
        }
    }

    /// F93's name loop.
    public static func askForName(on window: NSWindow, core: VixenCore, typed: String = "", completion: @escaping (String?) -> Void) {
        KitSheet.present(on: window) { dismiss in
            NamePromptSheet(prompt: "Gradient name?", initialText: typed) { response in
                dismiss()
                guard let response else {
                    completion(nil)
                    return
                }
                Task { @MainActor in
                    if response.isEmpty {
                        _ = await askInSheet(on: window, message: "Please enter a name.", title: "Color Gradient Name", buttons: ["OK"], style: .critical)
                        askForName(on: window, core: core, typed: response, completion: completion)
                        return
                    }
                    guard core.libraryGradients().contains(where: { $0.name == response }) else {
                        completion(response)
                        return
                    }
                    let answer = await askInSheet(
                        on: window, message: "There is already a gradient with that name. Do you want to overwrite it?", title: "Overwrite gradient?",
                        buttons: ["Overwrite", "Choose Another Name", "Cancel"], destructiveIndex: 0
                    )
                    switch answer {
                    case 0: completion(response)
                    case 1: askForName(on: window, core: core, typed: response, completion: completion)
                    default: completion(nil)
                    }
                }
            }
        }
    }
}

@Observable
final class GradientEditorViewModel {
    let session: GradientEditorSession
    var state: GradientEditorState
    /// The colour panel is showing this over the swatch, before it commits.
    var liveSwatch: UInt32?

    init(session: GradientEditorSession) {
        self.session = session
        state = session.state(width: 421)
    }

    /// The nonmodal colour panel still belongs to the stop selected when it opened.
    /// Commit before changing that selection or replacing/deleting its stops.
    func finishColorEdit() {
        ColorPanelSession.shared.finish()
        liveSwatch = nil
    }
}

private struct GradientEditorContent: View {
    let core: VixenCore
    let rowTitle: String?
    let onFinish: (GradientValue?) -> Void
    @State private var model: GradientEditorViewModel
    @State private var hostReference = WeakWindow()
    private var hostWindow: NSWindow? { hostReference.window }
    @State private var usesRowTitle: Bool

    init(session: GradientEditorSession, core: VixenCore, rowTitle: String?, onFinish: @escaping (GradientValue?) -> Void) {
        self.core = core
        self.rowTitle = rowTitle
        self.onFinish = onFinish
        _model = State(initialValue: GradientEditorViewModel(session: session))
        _usesRowTitle = State(initialValue: rowTitle != nil)
    }

    private var state: GradientEditorState { model.state }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(usesRowTitle ? (rowTitle ?? state.title) : state.title).font(.headline)
            GradientBar(model: model, onDoubleClickFader: editColor)
                .frame(minWidth: 480, minHeight: 90)
            GroupBox("Selected Stop") {
                HStack(spacing: 10) {
                    Text("Color:")
                    // The colour panel's live colour on top, the stop's below, as upstream's label shows a colour being picked over the old one.
                    ColorLabelBox(
                        color: model.liveSwatch ?? state.swatchArgb,
                        previous: state.swatchArgb,
                        isEnabled: state.hasSelection && !state.focusSelected && !state.readOnly,
                        onClick: editColor
                    )
                    .frame(width: 75, height: 29)
                    Text("Location:")
                    LocationUpDown(value: Int(state.location), isEnabled: state.hasSelection && !state.readOnly) { location in
                        model.finishColorEdit()
                        model.state = model.session.setLocation(value: Int32(location))
                    }
                    .frame(width: 96)
                    Button("Delete") {
                        model.finishColorEdit()
                        model.state = model.session.deleteSelected()
                    }
                        .disabled(!state.hasSelection || state.focusSelected || state.readOnly)
                    Spacer()
                }
                .padding(6)
            }
            GroupBox("Library") {
                VStack(alignment: .leading, spacing: 6) {
                    Text(state.label).fixedSize(horizontal: false, vertical: true)
                    HStack {
                        Button("Load Preset") { loadPreset() }
                            .disabled(!state.canLoad)
                        Button("Save Preset") { savePreset() }
                            .disabled(!state.canSave)
                        Button("Unlink Gradient") {
                            model.finishColorEdit()
                            usesRowTitle = false
                            model.state = model.session.unlink()
                        }
                        .disabled(!state.canUnlink)
                        Button("Edit Library Gradient") { editLibraryGradient() }
                            .disabled(!state.canEditLibrary)
                        Spacer(minLength: 0)
                    }
                }
                .padding(6)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            HStack {
                Spacer()
                Button("Cancel") {
                    model.finishColorEdit()
                    onFinish(nil)
                }
                .keyboardShortcut(.cancelAction)
                Button("OK") {
                    model.finishColorEdit()
                    onFinish(model.session.result())
                }
                .keyboardShortcut(.defaultAction)
            }
        }
        .padding(16)
        .frame(minWidth: 560)
        .background(WindowFinder(into: hostReference))
    }

    /// F90.
    private func editColor() {
        model.finishColorEdit()
        guard !state.readOnly, state.hasSelection, !state.focusSelected, let hostWindow else { return }
        if state.discrete {
            DiscreteColorPicker.pickSeveral(on: hostWindow, colors: state.validColors, selected: state.selectedColors) { chosen in
                model.state = model.session.setSelectedColors(colors: chosen)
            }
            return
        }
        Task { @MainActor in
            if state.selectedColors.count > 1 {
                _ = await askInSheet(
                    on: hostWindow, message: "Non-discrete color gradient, >1 selected point. oops! please report it.", title: "Delete library gradient?", buttons: ["OK"], style: .critical
                )
            }
            ColorPanelSession.shared.begin(
                title: "Color Picker",
                argb: state.swatchArgb,
                onLive: { argb in model.liveSwatch = argb },
                onEnd: { model.liveSwatch = nil },
                onFinish: { argb in
                    model.state = model.session.setSelectedColor(argb: argb)
                }
            )
        }
    }

    private func loadPreset() {
        guard let hostWindow else { return }
        model.finishColorEdit()
        GradientLibrarySelector.present(on: hostWindow, core: core) { item in
            usesRowTitle = false
            model.state = model.session.loadFromLibrary(name: item.name, entry: item.gradient)
        }
    }

    private func savePreset() {
        guard let hostWindow else { return }
        model.finishColorEdit()
        GradientEditorDialog.askForName(on: hostWindow, core: core) { name in
            if let name {
                _ = core.libraryAddGradient(name: name, gradient: model.session.result())
            }
        }
    }

    private func editLibraryGradient() {
        guard let hostWindow else { return }
        model.finishColorEdit()
        let name = state.gradient.libraryName
        GradientEditorDialog.editLibraryEntry(on: hostWindow, core: core, name: name) {
            if let entry = core.libraryGradients().first(where: { $0.name == name }) {
                model.state = model.session.refreshFromLibrary(entry: entry.gradient)
            }
        }
    }
}

/// F88's bar: a checkerboard, the gradient, a fader per distinct position below it, and the selected fader's diamond.
private struct GradientBar: NSViewRepresentable {
    let model: GradientEditorViewModel
    let onDoubleClickFader: () -> Void

    func makeNSView(context: Context) -> GradientBarNSView {
        GradientBarNSView(frame: .zero)
    }

    func updateNSView(_ view: GradientBarNSView, context: Context) {
        _ = model.state
        view.model = model
        view.onDoubleClickFader = onDoubleClickFader
        view.needsDisplay = true
    }
}

final class GradientBarNSView: NSView {
    private static let border: CGFloat = 10

    var model: GradientEditorViewModel?
    var onDoubleClickFader: (() -> Void)?
    private var tracking: NSTrackingArea?
    private var drawnWidth: CGFloat = 0

    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }

    override func acceptsFirstMouse(for event: NSEvent?) -> Bool {
        UITestHarness.isActive
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let tracking {
            removeTrackingArea(tracking)
        }
        let area = NSTrackingArea(rect: bounds, options: [.mouseMoved, .mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect], owner: self)
        addTrackingArea(area)
        tracking = area
    }

    /// The faders' positions depend on the width, so a new width asks Rust once, outside the layout pass.
    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        guard drawnWidth != newSize.width, newSize.width > 0 else { return }
        drawnWidth = newSize.width
        DispatchQueue.main.async { [weak self] in
            guard let self, let model = self.model else { return }
            model.state = model.session.state(width: self.drawnWidth)
            self.needsDisplay = true
        }
    }

    override func draw(_ dirtyRect: NSRect) {
        guard let model else { return }
        let state = model.state
        let area = NSRect(x: Self.border, y: Self.border, width: bounds.width - Self.border * 2 - 1, height: bounds.height - Self.border * 3 - 1)
        guard area.width > 0, area.height > 0 else { return }
        drawCheckerboard(in: area)
        GradientDrawing.draw(bands: state.bands, in: area)
        NSColor(srgbRed: 192 / 255, green: 192 / 255, blue: 192 / 255, alpha: 1).setStroke()
        NSBezierPath.defaultLineWidth = 1
        NSBezierPath(rect: area.insetBy(dx: 0.5, dy: 0.5)).stroke()
        for fader in state.faders {
            drawFader(fader)
        }
        if let x = state.diamondX {
            drawDiamond(x: x, selected: state.diamondSelected)
        }
    }

    private func drawCheckerboard(in rect: NSRect) {
        NSColor.white.setFill()
        rect.fill()
        NSColor(srgbRed: 192 / 255, green: 192 / 255, blue: 192 / 255, alpha: 1).setFill()
        let cell: CGFloat = 8
        var y = rect.minY
        var row = 0
        while y < rect.maxY {
            var x = rect.minX + (row % 2 == 0 ? 0 : cell)
            while x < rect.maxX {
                NSRect(x: x, y: y, width: cell, height: cell).intersection(rect).fill()
                x += cell * 2
            }
            y += cell
            row += 1
        }
    }

    private var faderBottom: CGFloat { bounds.height - 1 }

    private func drawFader(_ fader: GradientFaderInfo) {
        let left = CGFloat(fader.x)
        let right = left + 20
        let bottom = faderBottom
        let path = NSBezierPath()
        path.move(to: NSPoint(x: left + 1, y: bottom))
        path.line(to: NSPoint(x: right - 1, y: bottom))
        path.line(to: NSPoint(x: right, y: bottom - 1))
        path.line(to: NSPoint(x: right, y: bottom - 16))
        path.line(to: NSPoint(x: left + 10, y: bottom - 20))
        path.line(to: NSPoint(x: left, y: bottom - 16))
        path.line(to: NSPoint(x: left, y: bottom - 1))
        path.close()
        (fader.selected ? NSColor.selectedContentBackgroundColor : NSColor(white: 0.78, alpha: 1)).setFill()
        path.fill()
        let field = NSRect(x: left + 1, y: bottom - 14.5, width: 18, height: 14)
        if fader.colors.count == 1 {
            NSColor(argb: fader.colors[0]).setFill()
            field.fill()
        } else if let space = CGColorSpace(name: CGColorSpace.sRGB), let context = NSGraphicsContext.current?.cgContext {
            let increment = 1 / CGFloat(fader.colors.count - 1)
            let locations = fader.colors.indices.map { min(CGFloat($0) * increment, 1) }
            if let gradient = CGGradient(colorsSpace: space, colors: fader.colors.map { NSColor(argb: $0).cgColor } as CFArray, locations: locations) {
                context.saveGState()
                context.clip(to: field)
                context.drawLinearGradient(gradient, start: CGPoint(x: field.minX, y: field.midY), end: CGPoint(x: field.maxX, y: field.midY), options: [])
                context.restoreGState()
            }
        }
        NSColor.black.setStroke()
        path.lineWidth = 1
        path.stroke()
    }

    private func drawDiamond(x: Double, selected: Bool) {
        let m = CGFloat(x) + 10
        let bottom = faderBottom
        let path = NSBezierPath()
        path.move(to: NSPoint(x: m - 5, y: bottom - 8))
        path.line(to: NSPoint(x: m, y: bottom - 3))
        path.line(to: NSPoint(x: m + 5, y: bottom - 8))
        path.line(to: NSPoint(x: m, y: bottom - 13))
        path.close()
        (selected ? NSColor.selectedContentBackgroundColor : NSColor(white: 0.78, alpha: 1)).setFill()
        path.fill()
        NSColor.black.setStroke()
        path.lineWidth = 1
        path.stroke()
    }

    private func local(_ event: NSEvent) -> NSPoint {
        convert(event.locationInWindow, from: nil)
    }

    override func mouseMoved(with event: NSEvent) {
        guard let model else { return }
        let p = local(event)
        NSCursor.kit(model.session.hover(x: p.x, y: p.y, width: bounds.width, height: bounds.height)).set()
    }

    override func mouseExited(with event: NSEvent) {
        NSCursor.arrow.set()
    }

    override func mouseDown(with event: NSEvent) {
        guard let model else { return }
        model.finishColorEdit()
        window?.makeFirstResponder(self)
        let p = local(event)
        if event.clickCount == 2 {
            if model.session.faderUnder(x: p.x, y: p.y, width: bounds.width, height: bounds.height) {
                onDoubleClickFader?()
            }
            return
        }
        model.state = model.session.press(x: p.x, y: p.y, width: bounds.width, height: bounds.height)
        needsDisplay = true
    }

    override func mouseDragged(with event: NSEvent) {
        guard let model else { return }
        let p = local(event)
        model.state = model.session.drag(x: p.x, y: p.y, width: bounds.width, height: bounds.height)
        needsDisplay = true
    }

    override func mouseUp(with event: NSEvent) {
        guard let model else { return }
        let p = local(event)
        model.state = model.session.release(x: p.x, y: p.y, width: bounds.width, height: bounds.height)
        needsDisplay = true
    }

    override func keyDown(with event: NSEvent) {
        if event.keyCode == 51 || event.keyCode == 117, let model {
            model.finishColorEdit()
            model.state = model.session.deleteSelected()
            needsDisplay = true
        } else {
            super.keyDown(with: event)
        }
    }
}
