import AppKit
import SwiftUI
import VixenBridge
import VixenKit

/// Vixen's own icons, for the shapes and alignment.
@MainActor
enum ToolIcon {
    private static var cache: [String: NSImage] = [:]

    static func image(_ name: String) -> NSImage {
        if let found = cache[name] {
            return found
        }
        let url = Bundle.main.url(forResource: name, withExtension: "png", subdirectory: "preview")
        let image = url.flatMap { NSImage(contentsOf: $0) } ?? NSImage()
        cache[name] = image
        return image
    }
}

/// The root of a Preview Configuration window.
struct DesignerWindowRoot: View {
    let previewID: String?
    @Environment(AppModel.self) private var app
    @State private var model: PreviewDesignerModel?

    var body: some View {
        Group {
            if let model {
                DesignerContent(model: model)
            } else {
                Color.black
            }
        }
        // F1: upstream's 1038x656 is a frame size with its title bar, borders and menu strip in it. Don't apply 656 as a content height.
        .frame(minWidth: 1020, minHeight: 590)
        .onAppear {
            guard model == nil else { return }
            let id = previewID ?? PreviewHarness.targetPreviewID(app: app)
            if let id {
                model = PreviewDesignerModel(previewID: id, app: app)
            }
        }
    }
}

struct DesignerContent: View { // STUDIO: designer-in-pane shows this in the show window's Previews pane
    @Bindable var model: PreviewDesignerModel

    var body: some View {
        VStack(spacing: 0) {
            DesignerToolBand(model: model)
            Divider()
            ZStack {
                DesignerSplit(model: model)
                if let problem = model.problem {
                    ContentUnavailableView(problem, systemImage: "exclamationmark.triangle")
                }
            }
            Divider()
            DesignerStatusBand(model: model)
        }
        .sheet(item: $model.sheet) { sheet in
            DesignerSheetView(sheet: sheet, model: model)
        }
    }
}

// MARK: The tool band, F23 to F26, F59, F100, F110

/// Nine captioned groups, in upstream's order, reading only what they show. A drag changes none of it, so none of this runs during one.
private struct DesignerToolBand: View {
    let model: PreviewDesignerModel

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(alignment: .top, spacing: 10) {
                BackgroundGroup(model: model)
                bandDivider
                ToolGroups(model: model)
                bandDivider
                AlignGroup(model: model)
                bandDivider
                BulbGroup(model: model)
                bandDivider
                CustomPropGroup(model: model)
                bandDivider
                UndoGroup(model: model)
                bandDivider
                LockGroup(model: model)
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 5)
        }
        .fixedSize(horizontal: false, vertical: true)
        .background(.bar)
    }

    private var bandDivider: some View {
        Divider().frame(height: 62)
    }
}

private struct ToolCaption: ViewModifier {
    let title: String

    func body(content: Content) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            Text(title)
                .font(.caption2)
                .foregroundStyle(.secondary)
            content
        }
    }
}

private extension View {
    func toolGroup(_ title: String) -> some View {
        modifier(ToolCaption(title: title))
    }
}

/// An icon button, with a highlight while its tool is active. Square and tight, because thirty of these share one row of the window.
private struct IconButton: View {
    let icon: String
    let help: String
    var active = false
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Image(nsImage: ToolIcon.image(icon))
                .resizable()
                .interpolation(.high)
                .frame(width: 18, height: 18)
        }
        .buttonStyle(ToolButtonStyle(active: active))
        .help(help)
        .accessibilityLabel(help)
    }
}

private struct ToolButtonStyle: ButtonStyle {
    let active: Bool
    @Environment(\.isEnabled) private var isEnabled

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .frame(width: 26, height: 26)
            .background(
                RoundedRectangle(cornerRadius: 5)
                    .fill(active ? Color.accentColor : Color.primary.opacity(configuration.isPressed ? 0.25 : 0.1))
            )
            .opacity(isEnabled ? 1 : 0.35)
    }
}

private struct BackgroundGroup: View {
    @Bindable var model: PreviewDesignerModel

    var body: some View {
        HStack(spacing: 6) {
            IconButton(icon: "buttonSetBackground", help: "Set Background") { model.chooseBackground() }
            Slider(value: Binding(get: { model.backgroundAlpha }, set: { model.backgroundAlphaChanged($0.rounded()) }), in: 0...255)
                .controlSize(.mini)
                .frame(width: 96)
                .help("Background Brightness")
                .accessibilityLabel("Background Brightness")
        }
        .toolGroup("Background")
    }
}

/// F24's table, in its visual order.
private struct ToolGroups: View {
    let model: PreviewDesignerModel

    var body: some View {
        let tool = model.toolbar.tool
        IconButton(icon: "buttonSelect", help: "Select", active: tool == .select) { model.choose(.select) }
            .toolGroup("Select")
        VStack(spacing: 3) {
            HStack(spacing: 3) {
                IconButton(icon: "buttonDrawPixel", help: "Single Light", active: tool == .single) {
                    // `Keyboard.Modifiers == Control`: that key alone, so ⌘⇧ doesn't count. Read from the click itself.
                    let flags = (NSApp.currentEvent?.modifierFlags ?? NSEvent.modifierFlags).intersection([.command, .shift, .option, .control])
                    model.chooseSingleLight(commandHeld: flags == .command)
                }
                button("buttonLine", "Light String", .light, tool)
                button("buttonRectangle", "Rectangle", .rectangle, tool)
            }
            HStack(spacing: 3) {
                button("buttonEllipse", "Ellipse", .ellipse, tool)
                button("buttonTriangle", "Triangle", .triangle, tool)
                button("buttonPolyLine", "Multi-Point String", .polyLine, tool)
            }
        }
        .toolGroup("Basic Drawing")
        VStack(spacing: 3) {
            HStack(spacing: 3) {
                button("buttonSemiCircle", "Arch", .arch, tool)
                button("buttonNet", "Net", .net, tool)
                button("buttonMegaTree", "Mega Tree", .megaTree, tool)
                button("buttonStarBurst", "Star Burst", .starBurst, tool)
                button("buttonMultiString", "Multi-String", .multiString, tool)
            }
            HStack(spacing: 3) {
                button("buttonCane", "Candy Cane", .cane, tool)
                button("buttonMovingHead", "Intelligent Fixture", .movingHead, tool)
                button("buttonStar", "Star", .star, tool)
                button("buttonPixelGrid", "Pixel Grid", .pixelGrid, tool)
                button("buttonIcicle", "Icicles", .icicle, tool)
            }
        }
        .toolGroup("Smart Objects")
    }

    private func button(_ icon: String, _ help: String, _ value: DesignerTool, _ current: DesignerTool) -> some View {
        IconButton(icon: icon, help: help, active: current == value) { model.choose(value) }
    }
}

private struct AlignGroup: View {
    let model: PreviewDesignerModel

    var body: some View {
        let on = model.toolbar.align
        VStack(alignment: .leading, spacing: 3) {
            HStack(spacing: 3) {
                button("buttonAlignLeft", "Align Left", .left)
                button("buttonAlignRight", "Align Right", .right)
                button("buttonAlignTop", "Align Top", .top)
                button("buttonDistributeHorizontal", "Distribute Horizontally", .distributeHorizontally)
                button("buttonMatchProperties", "Match Properties", .matchProperties)
            }
            HStack(spacing: 3) {
                button("buttonAlignHorizMid", "Align Horizontally", .horizontally)
                button("buttonAlignVertMid", "Align Vertically", .vertically)
                button("buttonAlignBottom", "Align Bottom", .bottom)
                button("buttonDistributeVertical", "Distribute Vertically", .distributeVertically)
            }
        }
        .disabled(!on)
        .toolGroup("Align")
    }

    private func button(_ icon: String, _ help: String, _ kind: DesignerAlign) -> some View {
        IconButton(icon: icon, help: help) { model.align(kind) }
    }
}

private struct BulbGroup: View {
    let model: PreviewDesignerModel

    var body: some View {
        HStack(spacing: 3) {
            IconButton(icon: "buttonBulbSmaller", help: "Decrease Bulb Size") { model.apply(model.designer?.decreaseBulb()) }
            IconButton(icon: "buttonBulbBigger", help: "Increase Bulb Size") { model.apply(model.designer?.increaseBulb()) }
        }
        .disabled(!model.toolbar.bulb)
        .toolGroup("Bulb Size")
    }
}

private struct CustomPropGroup: View {
    let model: PreviewDesignerModel
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        HStack(spacing: 3) {
            IconButton(icon: "Prop_Edit", help: "Custom Prop Editor") { openWindow(id: WindowID.customPropEditor, value: UUID()) }
            if model.importingProp {
                ProgressView().controlSize(.small).frame(width: 22, height: 22)
            } else {
                IconButton(icon: "Prop_Add", help: "Add Custom Prop") { model.importProp() }
            }
            IconButton(icon: "folder_explore", help: "Custom Prop Library") { model.openCustomPropLibrary() }
        }
        .toolGroup("Custom Prop")
    }
}

/// F100: one click is one step, and the list undoes down to the row chosen.
private struct UndoGroup: View {
    let model: PreviewDesignerModel

    var body: some View {
        HStack(spacing: 3) {
            Menu {
                ForEach(Array(model.undoLabels.enumerated()), id: \.offset) { index, label in
                    Button(index == 0 ? label : "\(label)  (Undo \(index + 1) Actions)") { model.undo(steps: UInt32(index + 1)) }
                }
            } label: {
                Image(systemName: "arrow.uturn.backward")
            } primaryAction: {
                model.undo()
            }
            .disabled(!model.toolbar.canUndo)
            .help("Undo")
            .accessibilityLabel("Undo")
            Menu {
                ForEach(Array(model.redoLabels.enumerated()), id: \.offset) { index, label in
                    Button(index == 0 ? label : "\(label)  (Redo \(index + 1) Actions)") { model.redo(steps: UInt32(index + 1)) }
                }
            } label: {
                Image(systemName: "arrow.uturn.forward")
            } primaryAction: {
                model.redo()
            }
            .disabled(!model.toolbar.canRedo)
            .help("Redo")
            .accessibilityLabel("Redo")
        }
        .controlSize(.small)
        .frame(width: 84)
        .toolGroup("Undo/Redo")
    }
}

private struct LockGroup: View {
    let model: PreviewDesignerModel

    var body: some View {
        let t = model.toolbar
        HStack(spacing: 3) {
            IconButton(icon: "locked", help: "Lock") { model.apply(model.designer?.lock()) }
                .disabled(!t.lock)
            IconButton(icon: "unlocked", help: "Unlock") { model.apply(model.designer?.unlock()) }
                .disabled(!t.unlock)
            IconButton(icon: "unlockedAll", help: "Unlock All") { model.apply(model.designer?.unlockAll()) }
                .disabled(!t.unlockAll)
            IconButton(icon: t.hideLockedActive ? "ShowLocked" : "HideLocked", help: t.hideLockedActive ? "Show Locked" : "Hide Locked") { model.toggleHideLocked() }
                .disabled(!t.hideLockedEnabled)
        }
        .toolGroup("Lock/Unlock")
    }
}

// MARK: The status band, F50, F51

private struct DesignerStatusBand: View {
    @Bindable var model: PreviewDesignerModel

    var body: some View {
        HStack(spacing: 12) {
            HStack(spacing: 10) {
                legend("Linked", Theme.Preview.linked)
                legend("Selected", Theme.Preview.selected)
                legend("Un-Linked", Theme.Preview.unlinked)
                legend("Element Selected", Theme.Preview.elementSelected)
                legend("Locked", Theme.Preview.locked)
                legend("Pixel #1", Theme.Preview.pixel1)
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 2)
            .background(Color.black)
            Spacer()
            Slider(
                value: Binding(get: { model.zoomSliderPercent }, set: { model.zoomSliderMoved($0) }),
                in: 25...400,
                onEditingChanged: { editing in
                    if !editing {
                        model.applyPendingZoom()
                    }
                }
            )
            .controlSize(.mini)
            .frame(width: 180)
            .help("Zoom")
            .accessibilityLabel("Zoom")
            Text("\(Int(model.zoomSliderPercent))%")
                .monospacedDigit()
                .font(.caption)
                .frame(width: 40, alignment: .trailing)
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 3)
    }

    private func legend(_ title: String, _ color: Color) -> some View {
        Text(title)
            .font(.caption)
            .foregroundStyle(color)
    }
}

// MARK: The split, F80

enum DesignerPanelKind {
    case elements, properties

    var title: String {
        switch self {
        case .elements: "Elements"
        case .properties: "Properties"
        }
    }
}

/// A panel taken out of the window. Its close button puts it back, since upstream's panels can't be closed, only docked again.
final class DesignerFloatingPanel: NSPanel {
    let kind: DesignerPanelKind

    init(kind: DesignerPanelKind, contentRect: NSRect) {
        self.kind = kind
        super.init(contentRect: contentRect, styleMask: [.titled, .closable, .resizable, .utilityWindow], backing: .buffered, defer: false)
        title = kind.title
        isFloatingPanel = true
        hidesOnDeactivate = true
        becomesKeyOnlyIfNeeded = true
        isReleasedWhenClosed = false
        tabbingMode = .disallowed
        minSize = NSSize(width: 200, height: 160)
    }
}

/// The canvas on the right. Elements above Properties on the left, because the brief is that the buttons are where they typically are.
final class DesignerSplitController: NSSplitViewController, NSWindowDelegate {
    let canvasController = DesignerCanvasController()
    private let left = NSSplitViewController()
    private let elementsController: NSViewController
    private let propertiesController: NSViewController
    private var elementsItem: NSSplitViewItem?
    private var propertiesItem: NSSplitViewItem?
    private var leftItem: NSSplitViewItem?
    private weak var model: PreviewDesignerModel?
    private(set) var floating: [DesignerPanelKind: DesignerFloatingPanel] = [:]

    init(model: PreviewDesignerModel) {
        self.model = model
        let elements = NSHostingController(rootView: ElementsPanel(model: model))
        let properties = NSHostingController(rootView: PropertiesPanel(model: model, panel: model.properties))
        // AppKit sizes these. Otherwise SwiftUI's ideal sizes fight the dividers on every update.
        elements.sizingOptions = []
        properties.sizingOptions = []
        elementsController = elements
        propertiesController = properties
        super.init(nibName: nil, bundle: nil)
        left.splitView.isVertical = false
        left.splitView.dividerStyle = .thin
        let elementsItem = Self.item(for: elements, kind: .elements)
        let propertiesItem = Self.item(for: properties, kind: .properties)
        left.addSplitViewItem(elementsItem)
        left.addSplitViewItem(propertiesItem)
        self.elementsItem = elementsItem
        self.propertiesItem = propertiesItem

        splitView.isVertical = true
        splitView.dividerStyle = .thin
        let leftItem = NSSplitViewItem(viewController: left)
        leftItem.minimumThickness = 200
        leftItem.maximumThickness = 420
        leftItem.canCollapse = true
        leftItem.holdingPriority = .init(260)
        let canvasItem = NSSplitViewItem(viewController: canvasController)
        canvasItem.minimumThickness = 300
        addSplitViewItem(leftItem)
        addSplitViewItem(canvasItem)
        self.leftItem = leftItem
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    private var placedDividers = false

    override func viewDidLayout() {
        super.viewDidLayout()
        // Default 240 wide and split in half, and left to the user from then on.
        guard !placedDividers, view.bounds.width > 400, view.bounds.height > 200 else { return }
        placedDividers = true
        splitView.setPosition(240, ofDividerAt: 0)
        left.splitView.setPosition(max(view.bounds.height / 2, 120), ofDividerAt: 0)
    }

    private static func item(for controller: NSViewController, kind: DesignerPanelKind) -> NSSplitViewItem {
        let item = NSSplitViewItem(viewController: controller)
        item.minimumThickness = kind == .elements ? 120 : 160
        item.canCollapse = true
        return item
    }

    func setElementsVisible(_ on: Bool) {
        setVisible(.elements, on)
    }

    func setPropertiesVisible(_ on: Bool) {
        setVisible(.properties, on)
    }

    private func setVisible(_ kind: DesignerPanelKind, _ on: Bool) {
        if let panel = floating[kind] {
            if on {
                panel.orderFront(nil)
            } else {
                panel.orderOut(nil)
            }
            return
        }
        (kind == .elements ? elementsItem : propertiesItem)?.isCollapsed = !on
        collapseLeftIfEmpty()
    }

    private func collapseLeftIfEmpty() {
        let elementsGone = elementsItem?.isCollapsed ?? true
        let propertiesGone = propertiesItem?.isCollapsed ?? true
        leftItem?.isCollapsed = elementsGone && propertiesGone
    }

    // MARK: Floating, F80

    func setFloating(_ kind: DesignerPanelKind, _ on: Bool) {
        if on {
            detach(kind)
        } else {
            dock(kind)
        }
    }

    /// Out of the left column into a panel of its own, over where it was, at the size it had.
    private func detach(_ kind: DesignerPanelKind) {
        guard floating[kind] == nil, let item = kind == .elements ? elementsItem : propertiesItem, let window = view.window else { return }
        let controller = item.viewController
        let docked = controller.view.convert(controller.view.bounds, to: nil)
        // Out of the column it has room for every row of a big shape's grid.
        let size = NSSize(width: max(docked.width, 280), height: max(docked.height, kind == .properties ? 420 : 360))
        left.removeSplitViewItem(item)
        if kind == .elements {
            elementsItem = nil
        } else {
            propertiesItem = nil
        }
        collapseLeftIfEmpty()
        let panel = DesignerFloatingPanel(kind: kind, contentRect: NSRect(origin: .zero, size: size))
        panel.contentViewController = controller
        panel.setContentSize(size)
        let onScreen = window.convertToScreen(docked)
        panel.setFrameTopLeftPoint(NSPoint(x: onScreen.minX + 16, y: onScreen.maxY - 16))
        panel.delegate = self
        model?.panelOpened(panel)
        floating[kind] = panel
        let visible = kind == .elements ? model?.showElements ?? true : model?.showProperties ?? true
        if visible {
            panel.orderFront(nil)
        }
        model?.floatingChanged()
    }

    /// Back into the left column where it was: Elements above, Properties below.
    private func dock(_ kind: DesignerPanelKind) {
        guard let panel = floating.removeValue(forKey: kind) else { return }
        panel.delegate = nil
        panel.orderOut(nil)
        model?.panelClosed(panel)
        let controller = kind == .elements ? elementsController : propertiesController
        panel.contentViewController = nil
        let item = Self.item(for: controller, kind: kind)
        if kind == .elements {
            left.insertSplitViewItem(item, at: 0)
            elementsItem = item
            item.isCollapsed = !(model?.showElements ?? true)
        } else {
            left.addSplitViewItem(item)
            propertiesItem = item
            item.isCollapsed = !(model?.showProperties ?? true)
        }
        collapseLeftIfEmpty()
        if elementsItem != nil, propertiesItem != nil, left.view.bounds.height > 240 {
            left.splitView.setPosition(left.view.bounds.height / 2, ofDividerAt: 0)
        }
        model?.floatingChanged()
    }

    func windowShouldClose(_ sender: NSWindow) -> Bool {
        if let panel = sender as? DesignerFloatingPanel {
            dock(panel.kind)
        }
        return false
    }

    func windowWillReturnUndoManager(_ window: NSWindow) -> UndoManager? {
        model?.undoManager
    }

    func windowDidBecomeKey(_ notification: Notification) {
        DesignerRegistry.shared.windowsChanged()
    }

    /// The window is going, and its panels with it.
    func closeFloatingPanels() {
        for (_, panel) in floating {
            panel.delegate = nil
            panel.orderOut(nil)
            model?.panelClosed(panel)
            panel.contentViewController = nil
        }
        floating = [:]
    }
}

private struct DesignerSplit: NSViewControllerRepresentable {
    let model: PreviewDesignerModel

    func makeNSViewController(context: Context) -> DesignerSplitController {
        let controller = DesignerSplitController(model: model)
        model.split = controller
        controller.canvasController.canvas.onReady = { [weak model] canvas, window in
            model?.open(in: canvas, window: window)
        }
        return controller
    }

    func updateNSViewController(_ controller: DesignerSplitController, context: Context) {}
}
