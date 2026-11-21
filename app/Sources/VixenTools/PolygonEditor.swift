import AppKit
import SwiftUI
import VixenBridge
import VixenKit
#if canImport(VixenEffectEditor)
import VixenEffectEditor
#endif

@Observable final class PolygonModel {
    let editor: PolygonEditor
    var state: PolygonEdState
    var error: String?
    @ObservationIgnored var timeBarWidth = 560.0
    var close: ((Bool) -> Void)?
    init(_ editor: PolygonEditor) { self.editor = editor; state = editor.state() }
    func refresh() { state = editor.state() }
    // STUDIO-BEGIN polygon-live: each change is written to the edited effects at once (pointer moves wait for mouse up)
    func act(_ action: (PolygonEditor) -> Void) { action(editor); _ = editor.studioApplyLive(); refresh() }
    func track(_ action: (PolygonEditor) -> Void) { action(editor); refresh() }
    // STUDIO-END polygon-live
    func editPoint(_ row: Int, axis: String, value: String) {
        do { try editor.setPoint(row: UInt32(row), axis: axis, value: value); error = nil; _ = editor.studioApplyLive(); refresh() } // STUDIO: live
        catch { self.error = error.userMessage }
    }
    static let pasteboardType = NSPasteboard.PasteboardType("com.vixenlights.polygon-shape")
    func copy(cut: Bool = false) {
        guard let value = editor.copy() else { return }
        NSPasteboard.general.clearContents(); NSPasteboard.general.setString(value, forType: Self.pasteboardType)
        if cut { act { $0.delete() } } else { refresh() }
    }
    func paste() { if let value = NSPasteboard.general.string(forType: Self.pasteboardType) { act { _ = $0.paste(value: value) } } }
}

/// Native sheet retained until completion, so the draft and completion callback have the same lifetime.
enum PolygonPresenter {
    private static var windows: [UUID: PolygonWindow] = [:]
    static weak var current: PolygonModel?
    private static var testSource: SequencerSession?
    static func install() {
        #if canImport(VixenEffectEditor)
        EffectEditorHooks.editPolygons = { request in
            if UITestHarness.isActive { testSource = request.session }
            do {
                let frame = request.window?.screen?.visibleFrame ?? NSScreen.main?.visibleFrame ?? NSRect(x: 0, y: 0, width: 1280, height: 800)
                let editor = try request.session.openPolygonEditor(effects: request.effects, param: request.param, screenWidth: frame.width, screenHeight: frame.height)
                present(editor, owner: request.window, finished: request.finished)
            } catch {
                let alert = NSAlert(); alert.messageText = "Polygon Error"; alert.informativeText = "An error occurred loading the polygon editor.\n\(error.userMessage)"
                if let window = request.window { alert.beginSheetModal(for: window) { _ in request.finished(nil) } }
                else { alert.runModal(); request.finished(nil) }
            }
        }
        #endif
        guard UITestHarness.isActive else { return }
        UITestHarness.hooks["polygon"] = { args in
            guard let command = args.first else { return "FAIL polygon command" }
            if command == "open" { present(PolygonEditor.sample(mode: args.count > 1 ? args[1] : "FreeForm"), owner: nil) { _ in }; return "opened" }
            if command == "save", args.count == 2 {
                guard let source = testSource else { return "FAIL no source sequence" }
                do { try source.save(path: args[1]); return "saved" } catch { return "FAIL \(error)" }
            }
            if command == "source" { guard let source = testSource else { return "FAIL no source sequence" }; return "dirty=\(source.isModified()) undo=\(source.undoDescriptions().joined(separator: "|"))" }
            guard let m = current else { return "FAIL no polygon editor" }
            switch command {
            case "state": return "shapes=\(m.state.shapes.count) rows=\(m.state.rows.count) mode=\(m.state.mode) frames=\(m.state.frames.count) label=\(m.state.label ?? "-")"
            case "focus-name":
                guard let field: NSTextField = testView(model: m), field.isEnabled else { return "FAIL no enabled name field" }
                field.window?.makeFirstResponder(field); field.selectText(nil); return "focused native name"
            case "delete": m.act { $0.delete() }; return "deleted"
            case "names": return m.state.shapes.map { "\($0.kind):\($0.label)" }.joined(separator: "|")
            case "leave": m.act { $0.pointerLeave() }; return "left canvas"
            case "label": m.act { $0.setLabel(label: args.dropFirst().joined(separator: " ")) }; return "label \(m.state.label ?? "-")"
            case "native-drag":
                guard args.count >= 5, let x = Double(args[1]), let y = Double(args[2]), let tx = Double(args[3]), let ty = Double(args[4]), let view: PolygonCanvasView = testView(model: m) else { return "FAIL native-drag x y toX toY" }
                view.testDrag(from: CGPoint(x: x, y: y), to: CGPoint(x: tx, y: ty), shift: args.last == "shift")
                try? await Task.sleep(for: .milliseconds(150)); return "native drag"
            case "native-frame":
                guard args.count == 2, let time = Double(args[1]), let view: PolygonTimeView = testView(model: m) else { return "FAIL native-frame time" }
                view.testMoveFrame(to: time); try? await Task.sleep(for: .milliseconds(150)); return "native frame drag"
            case "frames": return m.state.frames.map { "\($0.time):\($0.selected)" }.joined(separator: "|")
            case "handles": return m.state.handles.map { "\($0.x),\($0.y)" }.joined(separator: "|")
            case "selection": return m.state.shapes.filter { $0.selected }.map(\.id).joined(separator: "|")
            case "geometry": return m.state.shapes.map { "\($0.id):\($0.kind):" + $0.points.map { "\($0.x),\($0.y)" }.joined(separator: ";") }.joined(separator: "|")
            case "mode": guard args.count == 2 else { return "FAIL mode" }; m.act { $0.setMode(mode: args[1]) }; return "mode \(m.state.mode)"
            case "click": guard args.count == 3, let x = Double(args[1]), let y = Double(args[2]) else { return "FAIL click" }; m.act { $0.pointerDown(x: x, y: y); $0.pointerUp(x: x, y: y, shift: false) }; return "clicked"
            case "point": guard args.count == 4, let row = Int(args[1]) else { return "FAIL point" }; m.editPoint(row, axis: args[2], value: args[3]); return m.error ?? "edited"
            case "rows": return m.state.rows.map { "\($0.label):\($0.x),\($0.y)" }.joined(separator: "|")
            default: return "FAIL unknown polygon command"
            }
        }
    }
    private static func testView<T: NSView>(model: PolygonModel) -> T? {
        func visit(_ view: NSView) -> T? { if let result = view as? T { return result }; for child in view.subviews { if let found = visit(child) { return found } }; return nil }
        guard let root = windows.values.first(where: { $0.model === model })?.window?.contentView else { return nil }
        return visit(root)
    }
    static func present(_ editor: PolygonEditor, owner: NSWindow?, finished: @escaping (String?) -> Void) {
        let id = UUID(); let model = PolygonModel(editor); let controller = PolygonWindow(model: model, screen: owner?.screen?.visibleFrame.size ?? NSScreen.main?.visibleFrame.size ?? NSSize(width: 1280, height: 800))
        windows[id] = controller; current = model
        var completed = false
        model.close = { ok in
            guard !completed else { return }; completed = true
            let label = editor.studioFinishLive() // STUDIO: the changes are already on the effects; closing records them as one undo step
            if let parent = controller.window?.sheetParent, let sheet = controller.window { parent.endSheet(sheet) }
            controller.window?.orderOut(nil); model.close = nil; windows[id] = nil; finished(label)
        }
        guard let window = controller.window else { return }
        if let owner { owner.beginSheet(window) } else { controller.showWindow(nil) }
    }
}
private final class PolygonWindow: NSWindowController, NSWindowDelegate {
    let model: PolygonModel
    init(model: PolygonModel, screen: NSSize) {
        self.model = model
        let content = NSHostingView(rootView: PolygonSheet(model: model))
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: max(680, max(model.state.width, screen.width * 0.7)), height: max(380, max(model.state.height + (model.state.timeMode ? 190 : 0), screen.height * 0.25))), styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
        window.title = "Polygon Editor"; window.contentView = content; window.isReleasedWhenClosed = false; window.minSize = NSSize(width: 680, height: 380)
        super.init(window: window); window.delegate = self; window.center()
    }
    required init?(coder: NSCoder) { nil }
    func windowShouldClose(_ sender: NSWindow) -> Bool { model.close?(false); return false }
}

private struct PolygonSheet: View {
    let model: PolygonModel
    var body: some View {
        VStack(spacing: 0) {
            PolygonToolbar(model: model).padding(8)
            Divider()
            HStack(spacing: 0) {
                PolygonCanvas(model: model).frame(minWidth: 400, minHeight: 260)
                VStack(alignment: .leading, spacing: 6) {
                    Text("Name:")
                    TextField("Select Shape", text: Binding(get: { model.state.label ?? "" }, set: { value in model.act { $0.setLabel(label: value) } })).disabled(model.state.label == nil)
                    PolygonPointTable(model: model)
                    if let error = model.error { Text(error).font(.caption).foregroundStyle(.red).fixedSize(horizontal: false, vertical: true) }
                }.padding(10).frame(width: 170)
            }
            if model.state.timeMode {
                Divider(); PolygonTimeBar(model: model).frame(height: 40).padding(.horizontal, 8)
                HStack {
                    Button { model.act { $0.selectFrame(index: model.state.selectedFrame - 1) } } label: { Image(systemName: "arrow.left") }.disabled(model.state.selectedFrame == 0).help("Previous Frame")
                    Button { model.act { $0.selectFrame(index: model.state.selectedFrame + 1) } } label: { Image(systemName: "arrow.right") }.disabled(Int(model.state.selectedFrame) + 1 >= model.state.frames.count).help("Next Frame")
                    Button("Add Frame") { model.act { $0.addFrame(width: model.timeBarWidth) } }
                    Button("Delete Frame") { model.act { $0.deleteFrame() } }.disabled(model.state.frames.count < 2)
                    Spacer()
                }.padding(.horizontal, 12).padding(.bottom, 6)
            }
            Divider()
            // STUDIO-BEGIN polygon-live-done: changes are live, so the window closes with Done; Edit > Undo in the sequence takes the whole window back
            HStack {
                Text("Changes apply as you make them. Undo in the sequence takes them all back.").font(.caption).foregroundStyle(.secondary)
                Spacer()
                Button("Done") { model.close?(true) }.keyboardShortcut(.defaultAction)
            }.padding(12)
            .onExitCommand { model.close?(true) }
            // STUDIO-END polygon-live-done
        }.studioAppearance() // STUDIO: follow the chosen appearance
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in model.refresh() }
    }
}
private struct PolygonToolbar: View {
    let model: PolygonModel
    private func tool(_ title: String, _ image: String, _ action: @escaping () -> Void) -> some View {
        Button(action: action) { Image(systemName: image).frame(width: 20) }.help(title).accessibilityLabel(title)
    }
    var body: some View {
        HStack(spacing: 4) {
            if model.state.freeForm {
                mode("Draw Line", "line.diagonal", "Line", enabled: model.state.canAdd)
                mode("Draw Polygon", "pentagon", "Polygon", enabled: model.state.canAdd)
                mode("Draw Ellipse", "oval", "Ellipse", enabled: model.state.canAdd)
            }
            mode("Add Point", "point.3.filled.connected.trianglepath.dotted", "AddPoint", enabled: model.state.shapes.contains { $0.kind == "Polygon" })
            mode("Selection mode", "cursorarrow", "Select", enabled: true)
            Divider().frame(height: 22)
            if model.state.freeForm {
                tool("Paste Polygon/Ellipse/Line", "document.on.clipboard") { model.paste() }.disabled(!model.state.canAdd || NSPasteboard.general.string(forType: PolygonModel.pasteboardType) == nil)
                tool("Cut Polygon/Ellipse/Line", "scissors") { model.copy(cut: true) }.disabled(!model.state.canCopy)
                tool("Copy Polygon/Ellipse/Line", "document.on.document") { model.copy() }.disabled(!model.state.canCopy)
                tool("Delete Polygon/Ellipse/Line", "trash") { model.act { $0.delete() } }.disabled(!model.state.canDelete)
            }
            if !model.state.timeMode {
                tool("Toggle Start Side", "arrow.trianglehead.clockwise") { model.act { $0.toggleStart() } }.disabled(!model.state.shapes.contains { $0.selected && $0.kind != "Line" })
                tool("Toggle Start Point", "arrow.left.arrow.right") { model.act { $0.toggleStart() } }.disabled(!model.state.shapes.contains { $0.selected && $0.kind == "Line" })
            }
            tool("Convert to Line", "line.diagonal.arrow") { model.act { $0.convert(kind: "Line") } }.disabled(!canConvert("Line"))
            tool("Convert to Polygon", "pentagon.fill") { model.act { $0.convert(kind: "Polygon") } }.disabled(!canConvert("Polygon"))
            tool("Convert to Ellipse", "oval.fill") { model.act { $0.convert(kind: "Ellipse") } }.disabled(!canConvert("Ellipse"))
            Toggle(isOn: Binding(get: { model.state.showLabels }, set: { value in model.act { $0.showLabels(show: value) } })) { Image(systemName: "character.textbox") }.toggleStyle(.button).help("Shows or hides shape labels").accessibilityLabel("Show Labels")
            Spacer(minLength: 0)
        }.buttonStyle(.borderless)
    }
    private func canConvert(_ kind: String) -> Bool {
        guard model.state.mode == "Select" else { return false }
        if model.state.freeForm && !model.state.timeMode { return model.state.shapes.contains { $0.selected && $0.kind != kind } }
        return !model.state.shapes.contains { $0.kind == kind }
    }
    private func mode(_ title: String, _ image: String, _ mode: String, enabled: Bool) -> some View {
        Toggle(isOn: Binding(get: { model.state.mode == mode }, set: { value in if value { model.act { $0.setMode(mode: mode) } } })) { Image(systemName: image).frame(width: 20) }.toggleStyle(.button).disabled(!enabled).help(title).accessibilityLabel(title)
    }
}
