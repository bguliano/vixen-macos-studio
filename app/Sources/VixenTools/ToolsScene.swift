import AppKit
import SwiftUI
import VixenBridge
import VixenKit

public struct ToolsScene: Scene {
    private let app: AppModel
    @State private var registry = PropEditorRegistry()
    public init(model: AppModel) { app = model; PropEditorHarness.install(); PolygonPresenter.install(); StudioPropPreferencesHarness.install() } // STUDIO: live Preferences' harness hook
    public var body: some Scene {
        WindowGroup("Custom Prop Editor", id: WindowID.customPropEditor, for: UUID.self) { _ in
            PropEditorRoot(app: app, registry: registry)
        }
        .defaultSize(width: 900, height: 700)
        .windowResizability(.contentMinSize)
        .defaultLaunchBehavior(.suppressed)
        .restorationBehavior(.disabled)
        .commands { PropEditorCommands(registry: registry) }
    }
}

@Observable private final class PropEditorRegistry { var active: PropEditorModel? }

private struct PropEditorFocus: FocusedValueKey { typealias Value = PropEditorModel }
extension FocusedValues {
    var propEditor: PropEditorModel? {
        get { self[PropEditorFocus.self] }
        set { self[PropEditorFocus.self] = newValue }
    }
}

private struct PropEditorCommands: Commands {
    let registry: PropEditorRegistry
    @FocusedValue(\.propEditor) private var focused
    private var model: PropEditorModel? { focused ?? (UITestHarness.isActive ? registry.active : nil) }
    var body: some Commands {
        CommandGroup(after: .newItem) {
            if let model {
                Button("New Prop…") { model.newDocument() }.keyboardShortcut("n")
                Button("Open Prop…") { model.openDocument() }.keyboardShortcut("o")
                Button("Import xModel…") { model.importXModelDocument() }.disabled(model.busy)
                Button("Save Prop") { Task { _ = await model.save() } }.keyboardShortcut("s")
                Button("Save Prop As…") { Task { _ = await model.save(asNew: true) } }.keyboardShortcut("s", modifiers: [.command, .shift])
            }
        }
        CommandGroup(after: .pasteboard) {
            if let model {
                Button("Paste Prop Elements as New…") { model.paste(asNew: true) }.keyboardShortcut("v", modifiers: [.command, .shift]).disabled(!model.canPasteAsNew)
            }
        }
        CommandMenu("Prop") {
            if let model {
                Button("Vendor Browser") { model.openVendorBrowser() }.disabled(model.busy)
                Button("Assign Background…") { model.assignBackground() }
                Button("Scale Background Image…") { model.showScale = true }
                Menu("Background Opacity") {
                    ForEach(1...10, id: \.self) { step in
                        let value = Double(step) / 10
                        Toggle("\(step * 10)%", isOn: Binding(get: { abs(model.state.opacity - value) < 0.00001 }, set: { _ in model.mutate { _ = $0.setOpacity(opacity: value) } }))
                    }
                }
                Button("Preferences…") { model.showPreferences = true }
                Menu("Export Wire Diagram") {
                    Button("Front Wire Diagram") { model.wireRequest = PropWireRequest(flip: false) }
                    Button("Back Wire Diagram") { model.wireRequest = PropWireRequest(flip: true) }
                }.disabled(model.canvas.lights.isEmpty)
                Divider()
                Button("View Help") { model.help() }
                if UITestHarness.isActive {
                    Button("Test Undo") { model.documentUndoManager.undo() }
                    Button("Test Redo") { model.documentUndoManager.redo() }
                }
            }
        }
    }
}

private struct PropEditorRoot: View {
    let app: AppModel
    let registry: PropEditorRegistry
    @State private var model = PropEditorModel()
    var body: some View {
        VStack(spacing: 0) {
            HSplitView {
                VSplitView {
                    PropElementPanel(model: model).frame(minHeight: 200)
                    PropInfoPanel(model: model).frame(minHeight: 190)
                }.frame(minWidth: 300, idealWidth: 300).background(PropSplitStart())
                VStack(spacing: 0) { PropToolbar(model: model); Divider(); PropCanvas(model: model) }.frame(minWidth: 360)
            }
            Divider()
            HStack {
                Text(model.state.pointerText).monospacedDigit()
                Spacer()
                Text(model.loadingStatus ?? model.state.status).lineLimit(1)
            }.font(.caption).padding(.horizontal, 8).frame(height: 24)
        }
        .frame(minWidth: 740, minHeight: 540)
        .studioAppearance() // STUDIO: follow the chosen appearance
        .focusedSceneValue(\.propEditor, model)
        .background(PropWindowAttachment(model: model, app: app))
        .onAppear { PropEditorHarness.current = model; registry.active = model }
        .onDisappear { if registry.active === model { registry.active = nil } }
        .sheet(isPresented: $model.showScale) { PropScaleSheet(model: model) }
        .sheet(isPresented: $model.showPreferences) { StudioPropPreferencesSheet(model: model) } // STUDIO: live, with Undo and Done (B8)
        .sheet(item: $model.renameRequest) { request in
            if request.findReplace {
                SubstitutionRenamerSheet(oldNames: request.names, verb: "Rename") { names in // STUDIO: a verb on the confirmation
                    if let names { model.mutate { _ = $0.renameMany(idsText: request.ids, names: names) } }; model.renameRequest = nil
                }
            } else {
                NameGeneratorSheet(seed: "NewName", oldNames: request.names, verb: "Rename") { names in // STUDIO: a verb on the confirmation
                    if let names { model.mutate { _ = $0.renameMany(idsText: request.ids, names: names) } }; model.renameRequest = nil
                }
            }
        }
        .sheet(item: $model.vendorRequest) { vendor in PropVendorSheet(model: vendor) }
        .sheet(item: $model.xmodelRequest) { request in
            PropXModelSheet(request: request) { index in
                model.xmodelRequest = nil
                if let index { Task { await model.importXModel(source: request.source, index: index) } }
            }
        }
        .sheet(item: $model.wireRequest) { request in
            WireDiagramSheet(verb: "Export") { options in // STUDIO: a verb on the confirmation
                model.wireRequest = nil
                if let options { model.exportWireDiagram(flip: request.flip, options: options) }
            }
        }
        .onReceive(NotificationCenter.default.publisher(for: .propPreferencesChanged)) { _ in model.reloadPreferences() }
        .overlay { if model.busy { ProgressView().padding(20).background(.regularMaterial, in: RoundedRectangle(cornerRadius: 8)) } }
        .disabled(model.busy)
    }
}

struct PropSplitStart: NSViewRepresentable {
    final class Marker: NSView {
        var placed = false
        override func viewDidMoveToWindow() { super.viewDidMoveToWindow(); place(0) }
        private func place(_ attempt: Int) {
            guard !placed, window != nil, attempt < 50 else { return }
            var parent = superview
            while let p = parent, !(p is NSSplitView && (p as? NSSplitView)?.isVertical == true) { parent = p.superview }
            if let split = parent as? NSSplitView, split.bounds.width >= 700 { placed = true; split.setPosition(split.bounds.width / 3, ofDividerAt: 0) }
            else { DispatchQueue.main.asyncAfter(deadline: .now() + 0.02) { [weak self] in self?.place(attempt + 1) } }
        }
    }
    func makeNSView(context: Context) -> Marker { Marker() }
    func updateNSView(_ view: Marker, context: Context) {}
}

private struct PropToolbar: View {
    let model: PropEditorModel
    // STUDIO-BEGIN prop-toolbar-fits: at the window's minimum the arranging tools fold into one menu, not out of sight
    var body: some View {
        ViewThatFits(in: .horizontal) {
            row(compact: false)
            row(compact: true)
        }
        .frame(maxWidth: .infinity, alignment: .leading).fixedSize(horizontal: false, vertical: true).background(.bar)
    }
    private func row(compact: Bool) -> some View {
        HStack(spacing: 9) {
            tool("New", "doc.badge.plus") { model.newDocument() }
            tool("Open", "folder") { model.openDocument() }
            tool("Save", "square.and.arrow.down") { Task { _ = await model.save() } }
            Divider().frame(height: 24)
            Picker("Mode", selection: Binding(get: { model.state.mode }, set: { mode in model.mutate { $0.setMode(mode: mode) } })) {
                Text("Draw").tag(PropEdMode.draw)
                Text("Select").tag(PropEdMode.select)
            }.pickerStyle(.segmented).frame(width: 125)
            tool("Assign Background", "photo") { model.assignBackground() }
            if compact {
                Menu {
                    Menu("Align") { alignItems }.disabled(!model.state.canIncreaseLightSize)
                    Menu("Distribute") { distributeItems }.disabled(!model.state.canIncreaseLightSize)
                    Menu("Flip") { flipItems }.disabled(!model.state.canIncreaseLightSize)
                    Divider()
                    Button("Increase Light Size") { model.mutate { _ = $0.changeLightSize(change: .increase) } }.disabled(!model.state.canIncreaseLightSize)
                    Button("Decrease Light Size") { model.mutate { _ = $0.changeLightSize(change: .decrease) } }.disabled(!model.state.canDecreaseLightSize)
                    Button("Match Light Size") { model.mutate { _ = $0.changeLightSize(change: .match) } }.disabled(!model.state.canMatchLightSize)
                } label: { Image(systemName: "square.on.square.dashed") }.help("Arrange selected lights").accessibilityLabel("Arrange selected lights")
            } else {
                Menu { alignItems } label: { Image(systemName: "align.horizontal.left") }.help("Align selected lights").accessibilityLabel("Align selected lights").disabled(!model.state.canIncreaseLightSize)
                Menu { distributeItems } label: { Image(systemName: "distribute.horizontal.center") }.help("Distribute selected lights").accessibilityLabel("Distribute selected lights").disabled(!model.state.canIncreaseLightSize)
                Menu { flipItems } label: { Image(systemName: "arrow.left.and.right.righttriangle.left.righttriangle.right") }.help("Flip selected lights").accessibilityLabel("Flip selected lights").disabled(!model.state.canIncreaseLightSize)
                tool("Increase Light Size", "plus.magnifyingglass") { model.mutate { _ = $0.changeLightSize(change: .increase) } }.disabled(!model.state.canIncreaseLightSize)
                tool("Decrease Light Size", "minus.magnifyingglass") { model.mutate { _ = $0.changeLightSize(change: .decrease) } }.disabled(!model.state.canDecreaseLightSize)
                tool("Match Light Size", "equal.square") { model.mutate { _ = $0.changeLightSize(change: .match) } }.disabled(!model.state.canMatchLightSize)
            }
            Divider().frame(height: 24)
            tool("Undo", "arrow.uturn.backward") { model.documentUndoManager.undo() }.disabled(model.state.undoName == nil)
            tool("Redo", "arrow.uturn.forward") { model.documentUndoManager.redo() }.disabled(model.state.redoName == nil)
        }.padding(8).buttonStyle(.borderless)
    }
    @ViewBuilder private var alignItems: some View {
        Button("Left") { model.align(.left) }; Button("Right") { model.align(.right) }; Button("Top") { model.align(.top) }; Button("Bottom") { model.align(.bottom) }
    }
    @ViewBuilder private var distributeItems: some View {
        Button("Horizontal") { model.mutate { _ = $0.distribute(axis: .horizontal) } }; Button("Vertical") { model.mutate { _ = $0.distribute(axis: .vertical) } }
    }
    @ViewBuilder private var flipItems: some View {
        Button("Horizontal") { model.mutate { _ = $0.flip(axis: .horizontal) } }; Button("Vertical") { model.mutate { _ = $0.flip(axis: .vertical) } }
    }
    // STUDIO-END prop-toolbar-fits
    private func tool(_ title: String, _ image: String, action: @escaping () -> Void) -> some View {
        Button(action: action) { Image(systemName: image).frame(width: 22, height: 22) }.help(title).accessibilityLabel(title)
    }
}
