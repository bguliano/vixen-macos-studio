import AppKit
import SwiftUI
import UniformTypeIdentifiers
import VixenBridge
import VixenKit

@MainActor @Observable
final class PropEditorModel {
    var editor: CustomPropEditor
    var state: PropEdState
    var states: PropEdStates
    var rows: [PropEdTreeRow] = []
    var order: [PropEdOrderRow] = []
    var element: PropEdElementInfo?
    var canvas: PropEdCanvas
    var image: NSImage?
    var revision = 0
    var treeSelection = Set<String>()
    var orderSelection = Set<String>()
    var busy = false
    var showScale = false
    var showPreferences = false
    var renameRequest: PropRenameRequest?
    var wireRequest: PropWireRequest?
    var xmodelRequest: PropXModelRequest?
    var vendorRequest: PropVendorModel?
    var loadingStatus: String?
    @ObservationIgnored weak var window: NSWindow?
    @ObservationIgnored weak var app: AppModel?
    @ObservationIgnored var delegate: PropWindowDelegate?
    @ObservationIgnored let documentUndoManager = UndoManager()
    @ObservationIgnored var responder: PropDocumentResponder?
    @ObservationIgnored lazy var treeProvider = PropTreeProvider(model: self)
    @ObservationIgnored let treeHandle = OutlineHandle()
    @ObservationIgnored private var treeRefresh: Task<Void, Never>?
    @ObservationIgnored var canvasRefresh: (() -> Void)?
    @ObservationIgnored private var imageToken: UInt64?
    @ObservationIgnored private var adjustingSelection = false
    @ObservationIgnored private let closeGuardID = UUID()
    static let clipboardType = NSPasteboard.PasteboardType("com.vixenlights.custom-prop-elements")

    init() {
        let document = CustomPropEditor(name: "New Prop {1}")
        editor = document; state = document.state(); states = document.states(); canvas = document.canvas()
        documentUndoManager.groupsByEvent = false
        refresh()
    }
    func attach(window: NSWindow, app: AppModel) {
        guard self.window !== window else { return }
        self.window = window; self.app = app
        app.registerDocumentCloseGuard(id: closeGuardID) { [weak self] in
            guard let self else { return true }
            return await self.confirmReplacement()
        }
        let d = PropWindowDelegate(model: self, original: window.delegate)
        delegate = d; window.delegate = d
        let responder = PropDocumentResponder(model: self)
        responder.nextResponder = window.nextResponder; window.nextResponder = responder; self.responder = responder
        if let p = app.core?.propPreferences() { editor.applyPreferences(preferences: p) }
        refresh()
    }
    func didClose() { app?.removeDocumentCloseGuard(id: closeGuardID) }
    func refresh(full: Bool = true) {
        state = editor.state(); canvas = editor.canvas()
        if imageToken != state.imageToken {
            imageToken = state.imageToken
            image = NSImage(data: Data(editor.backgroundImage()))
        }
        if full {
            states = editor.states()
            rows = editor.treeRows(); order = editor.orderRows(); element = editor.elementInfo()
            adjustingSelection = true
            treeSelection = Set(editor.treeSelection()); orderSelection = Set(editor.orderSelection())
            adjustingSelection = false
            revision += 1
            if state.tab == .layout { refreshTree() }
        } else {
            let selected = Set(editor.treeSelection())
            if selected != treeSelection {
                treeSelection = selected; element = editor.elementInfo()
                if state.tab == .layout { refreshTree(reload: false) }
            }
        }
        window?.isDocumentEdited = state.dirty
        canvasRefresh?()
    }
    func refreshTree(initial: Bool = false, reload: Bool = true) {
        treeRefresh?.cancel()
        let wanted = Array(treeSelection)
        let requests = editor.takeExpandRequests() + (initial ? rows.filter(\.isRoot).map(\.path) : [])
        var expanded = Set<String>()
        let expand = requests.flatMap { path -> [String] in
            let parts = path.split(separator: "/"); return (1...max(parts.count,1)).map { parts.prefix($0).joined(separator: "/") }
        }.filter { !$0.isEmpty && expanded.insert($0).inserted }.sorted { $0.split(separator: "/").count < $1.split(separator: "/").count }
        treeRefresh = Task {
            if initial { await treeHandle.loadRoots() } else if reload { await treeHandle.reload(roots: true, parents: { _ in true }) }
            guard !Task.isCancelled else { return }
            adjustingSelection = true
            await treeHandle.select(wanted, expanding: expand)
            adjustingSelection = false
        }
    }
    func mutate(_ action: (CustomPropEditor) -> Void) {
        let before = state.revision
        action(editor); refresh(); registerChange(after: before)
    }
    func registerChange(after before: UInt64) {
        guard state.revision != before, let name = state.undoName else { return }
        documentUndoManager.beginUndoGrouping()
        documentUndoManager.registerUndo(withTarget: self) { $0.undo() }
        documentUndoManager.setActionName(name)
        documentUndoManager.endUndoGrouping()
    }
    func undo() {
        let name = state.undoName ?? "Edit Prop"
        guard editor.undo() else { return }
        documentUndoManager.registerUndo(withTarget: self) { $0.redo() }
        documentUndoManager.setActionName(name); refresh()
    }
    func redo() {
        let name = state.redoName ?? "Edit Prop"
        guard editor.redo() else { return }
        documentUndoManager.registerUndo(withTarget: self) { $0.undo() }
        documentUndoManager.setActionName(name); refresh()
    }
    func selectTree(_ value: Set<String>) {
        guard !adjustingSelection else { return }
        editor.setTreeSelection(pathsText: rows.filter { value.contains($0.path) }.map(\.path)); refresh(full: false); element = editor.elementInfo()
    }
    func selectOrder(_ value: Set<String>) {
        guard !adjustingSelection else { return }
        editor.setOrderSelection(idsText: order.filter { value.contains($0.id) }.map(\.id)); refresh(full: false); element = editor.elementInfo()
    }
    func align(_ edge: PropEdEdge) { mutate { _ = $0.align(edge: edge) } }
    func deleteElements() { mutate { _ = $0.deleteSelectedElements() } }
    func deleteLights() { mutate { _ = $0.deleteSelectedLights() } }
    var clip: String? { NSPasteboard.general.string(forType: Self.clipboardType) }
    var canPaste: Bool { clip.map { editor.canPaste(clipJson: $0) } ?? false }
    var canPasteAsNew: Bool { clip.map { editor.canPasteAsNew(clipJson: $0) } ?? false }
    func copy(cut: Bool = false) {
        let before = state.revision
        guard let text = cut ? editor.cut() : editor.copy() else { return }
        NSPasteboard.general.clearContents(); NSPasteboard.general.setString(text, forType: Self.clipboardType)
        refresh(); registerChange(after: before)
    }
    func paste(asNew: Bool = false) {
        guard let clip else { return }
        if asNew {
            Task { if let name = await prompt("Please enter the new name.", title: "Paste As New", initial: editor.suggestedPasteName()) { mutate { _ = $0.pasteAsNew(clipJson: clip, name: name) } } }
        } else { mutate { _ = $0.paste(clipJson: clip) } }
    }
    func group(_ mode: Int) {
        Task {
            guard let name = await prompt("Please enter the group name.", title: "Create Group", initial: mode == 0 ? editor.suggestedEmptyGroupName() : "") else { return }
            mutate { if mode == 0 { _ = $0.createEmptyGroup(name: name) } else if mode == 1 { _ = $0.copyToNewGroup(name: name) } else { _ = $0.moveToNewGroup(name: name) } }
        }
    }
    func rename(findReplace: Bool = false) {
        let selected = state.tab == .order ? order.filter { orderSelection.contains($0.id) }.map { ($0.id, $0.name) } : rows.filter { treeSelection.contains($0.path) }.map { ($0.id, $0.name) }
        guard let first = selected.first else { return }
        if selected.count > 1 {
            renameRequest = PropRenameRequest(ids: selected.map(\.0), names: selected.map(\.1), findReplace: findReplace)
            return
        }
        Task {
            guard let name = await prompt("Please enter the new name.", title: "Rename", initial: first.1) else { return }
            mutate { _ = try? $0.rename(idText: first.0, name: name) }
        }
    }
    func newDocument() {
        Task {
            guard await confirmReplacement(), let name = await prompt("Please enter the model name.", title: "Create Model", initial: "New Prop") else { return }
            replace(CustomPropEditor(name: propEdNewName(typed: name)))
        }
    }
    func replace(_ document: CustomPropEditor) {
        editor = document; imageToken = nil
        if let p = app?.core?.propPreferences() { editor.applyPreferences(preferences: p) }
        documentUndoManager.removeAllActions(withTarget: self)
        refresh()
    }
    func openDocument() {
        Task {
            guard await confirmReplacement() else { return }
            let panel = NSOpenPanel(); panel.allowedContentTypes = [UTType(filenameExtension: "prp") ?? .data]; panel.allowsMultipleSelection = false
            panel.directoryURL = app?.core.map { URL(fileURLWithPath: $0.dataRoot()) }
            guard await panel.begin() == .OK, let url = panel.url else { return }
            await open(path: url.path)
        }
    }
    func open(path: String) async {
        busy = true; defer { busy = false }
        do { let document = try await Task.detached { try CustomPropEditor.open(path: path) }.value; replace(document) }
        catch { await tell("Prop Load Error", "The prop could not be loaded.\n\(error.userMessage)") }
    }
    func save(asNew: Bool = false, path requested: String? = nil) async -> Bool {
        let errors = editor.states().validation
        if !errors.isEmpty { await tell("Invalid State Definitions", errors.joined(separator: "\n")); return false }
        var path = requested ?? (asNew ? nil : state.filePath)
        if path == nil {
            let panel = NSSavePanel(); panel.allowedContentTypes = [UTType(filenameExtension: "prp") ?? .data]
            panel.nameFieldStringValue = state.fileName + ".prp"
            panel.directoryURL = app?.core.map { URL(fileURLWithPath: $0.dataRoot()) }
            guard await panel.begin() == .OK, let url = panel.url else { return false }; path = url.path
        }
        guard let path else { return false }
        busy = true; defer { busy = false }
        do { let editor = editor; _ = try await Task.detached { try editor.saveAs(path: path) }.value; refresh(); return true }
        catch { await tell("Prop Save Error", "The prop could not be saved.\n\(error.userMessage)"); return false }
    }
    func confirmReplacement() async -> Bool {
        guard !busy else { return false }
        guard state.dirty else { return true }
        let alert = NSAlert(); alert.messageText = "Save Prop \"\(state.fileName)\"?"; alert.addButton(withTitle: "Save"); alert.addButton(withTitle: "Don't Save"); alert.addButton(withTitle: "Cancel")
        switch await show(alert) {
        case .alertFirstButtonReturn: return await save()
        case .alertSecondButtonReturn: return true
        default: return false
        }
    }
    func assignBackground() {
        Task {
            let panel = NSOpenPanel(); panel.allowedContentTypes = [.image]; panel.allowsMultipleSelection = false
            panel.directoryURL = FileManager.default.urls(for: .picturesDirectory, in: .userDomainMask).first
            guard await panel.begin() == .OK, let url = panel.url else { return }
            await assignBackground(path: url.path)
        }
    }
    func assignBackground(path: String) async {
        guard !busy else { return }
        let before = state.revision; busy = true
        do { let editor = editor; try await Task.detached { try editor.assignBackground(path: path) }.value; refresh(); registerChange(after: before) }
        catch { await tell("Background Image", error.userMessage) }
        busy = false
    }
    func help() {
        let url = "https://www.vixenlights.com/vixen-3-documentation/preview/custom-prop-editor/"
        if UITestHarness.isActive { UITestHarness.note("help \(url)") } else if let url = URL(string: url) { NSWorkspace.shared.open(url) }
    }
    func exportWireDiagram(flip: Bool, options: SetupWireOptions) {
        busy = true
        Task {
            defer { busy = false }
            do {
                let editor = editor
                let path = try await Task.detached { try editor.exportWireDiagram(flip: flip, options: options) }.value
                if let path {
                    if UITestHarness.isActive { UITestHarness.note("wire diagram \(path)") }
                    else { NSWorkspace.shared.open(URL(fileURLWithPath: path)) }
                }
            } catch { await tell("Export Wire Diagram", error.userMessage) }
        }
    }
    func show(_ alert: NSAlert) async -> NSApplication.ModalResponse {
        if let window { return await withCheckedContinuation { continuation in alert.beginSheetModal(for: window) { continuation.resume(returning: $0) } } }
        return alert.runModal()
    }
    func tell(_ title: String, _ text: String) async {
        let alert = NSAlert(); alert.messageText = title; alert.informativeText = text; alert.addButton(withTitle: "OK"); _ = await show(alert)
    }
    func prompt(_ text: String, title: String, initial: String, validator: ((String) -> Bool)? = nil) async -> String? {
        let alert = NSAlert(); alert.messageText = title; alert.informativeText = text
        let input = NSTextField(string: initial); input.frame = NSRect(x: 0, y: 0, width: 280, height: 24)
        let verb = String(title.split(separator: " ").first ?? "Continue") // STUDIO: every prompt's title starts with its verb (Create, Rename, Paste, Add, Copy); never OK
        alert.accessoryView = input; alert.addButton(withTitle: verb); alert.addButton(withTitle: "Cancel")
        let validation = PropPromptValidation(button: alert.buttons[0], validator: validator)
        input.delegate = validation; validation.validate(input.stringValue)
        alert.window.initialFirstResponder = input
        let result = await show(alert)
        withExtendedLifetime(validation) {}
        return result == .alertFirstButtonReturn && alert.buttons[0].isEnabled ? input.stringValue : nil
    }
}

struct PropRenameRequest: Identifiable {
    let id = UUID()
    let ids: [String]
    let names: [String]
    let findReplace: Bool
}

struct PropWireRequest: Identifiable { let id = UUID(); let flip: Bool }

private final class PropPromptValidation: NSObject, NSTextFieldDelegate {
    let button: NSButton
    let validator: ((String) -> Bool)?
    init(button: NSButton, validator: ((String) -> Bool)?) { self.button = button; self.validator = validator }
    func validate(_ value: String) { button.isEnabled = !value.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && (validator?(value) ?? true) }
    func controlTextDidChange(_ notification: Notification) { if let input = notification.object as? NSTextField { validate(input.stringValue) } }
}

final class PropWindowDelegate: NSObject, NSWindowDelegate {
    weak var model: PropEditorModel?
    nonisolated(unsafe) weak var original: NSWindowDelegate?
    private var allowClose = false
    private var asking = false
    init(model: PropEditorModel, original: NSWindowDelegate?) { self.model = model; self.original = original }
    func windowShouldClose(_ sender: NSWindow) -> Bool {
        guard !allowClose, let model, model.state.dirty else { return true }
        if !asking {
            asking = true
            Task { if await model.confirmReplacement() { allowClose = true; sender.performClose(nil) }; asking = false }
        }
        return false
    }
    func windowDidBecomeKey(_ notification: Notification) { PropEditorHarness.current = model }
    func windowWillReturnUndoManager(_ window: NSWindow) -> UndoManager? { model?.documentUndoManager }
    func windowWillClose(_ notification: Notification) { model?.didClose(); original?.windowWillClose?(notification) }
    nonisolated override func responds(to selector: Selector!) -> Bool { super.responds(to: selector) || original?.responds(to: selector) == true }
    nonisolated override func forwardingTarget(for selector: Selector!) -> Any? { original }
}

final class PropDocumentResponder: NSResponder, NSUserInterfaceValidations {
    weak var model: PropEditorModel?
    init(model: PropEditorModel) { self.model = model; super.init() }
    required init?(coder: NSCoder) { nil }
    override var undoManager: UndoManager? { model?.documentUndoManager }
    @objc func undo(_ sender: Any?) { model?.documentUndoManager.undo() }
    @objc func redo(_ sender: Any?) { model?.documentUndoManager.redo() }
    @objc func cut(_ sender: Any?) { model?.copy(cut: true) }
    @objc func copy(_ sender: Any?) { model?.copy() }
    @objc func paste(_ sender: Any?) { model?.paste() }
    @objc func delete(_ sender: Any?) { model?.deleteElements() }
    func validateUserInterfaceItem(_ item: any NSValidatedUserInterfaceItem) -> Bool {
        guard let model else { return false }
        switch item.action {
        case #selector(undo(_:)): return model.documentUndoManager.canUndo
        case #selector(redo(_:)): return model.documentUndoManager.canRedo
        case #selector(cut(_:)), #selector(copy(_:)): return model.state.canCopy
        case #selector(paste(_:)): return model.canPaste
        case #selector(delete(_:)): return model.state.canDeleteElements
        default: return true
        }
    }
}

enum PropEditorHarness {
    static weak var current: PropEditorModel?
    static var installed = false
    static var shutdownResult = "pending"
    private static func view<T: NSView>(_ type: T.Type, in root: NSView?, matching: (T) -> Bool = { _ in true }) -> T? {
        guard let root else { return nil }
        if let value = root as? T, matching(value) { return value }
        for child in root.subviews { if let value = view(type, in: child, matching: matching) { return value } }
        return nil
    }
    private static func textControls(in window: NSWindow) -> [NSView] {
        func visit(_ view: NSView) -> [NSView] {
            guard !view.isHidden else { return [] }
            if let field = view as? NSTextField, field.isEditable, field.isEnabled { return [field] }
            if let field = view as? NSTextView, field.isEditable, !field.isFieldEditor { return [field] }
            return view.subviews.flatMap(visit)
        }
        guard let root = window.contentView else { return [] }
        return visit(root).sorted { a, b in
            let x = a.convert(a.bounds, to: nil), y = b.convert(b.bounds, to: nil)
            return abs(x.midY - y.midY) > 4 ? x.midY > y.midY : x.midX < y.midX
        }
    }
    static func install() {
        guard UITestHarness.isActive, !installed else { return }; installed = true
        UITestHarness.hooks["props"] = { args in
            guard let model = current, let verb = args.first else { return "FAIL no prop editor" }
            switch verb {
            case "tree-validate", "tree-drop":
                guard args.count >= 4, let parent = model.rows.first(where: { $0.name == args[1] }) else { return "FAIL tree-drop parent index names" }
                let index = args[2] == "on" ? nil : Int(args[2]); let names = Set(args.dropFirst(3)); let paths = model.rows.filter { names.contains($0.name) }.map(\.path)
                let allowed = model.treeProvider.outlineCanDrop(items: paths, parent: parent.path, index: index, copy: names.contains("copy"))
                if !allowed { return "refused" }
                if verb == "tree-drop" { model.treeProvider.outlineDrop(items: paths, parent: parent.path, index: index, copy: false) }
                return verb == "tree-drop" ? "native provider drop accepted" : "allowed"
            case "native-f2":
                guard let window = model.window, let down = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [.function], timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber, context: nil, characters: "\u{F705}", charactersIgnoringModifiers: "\u{F705}", isARepeat: false, keyCode: 120) else { return "FAIL F2 event" }
                window.sendEvent(down); return "sent native F2 keyCode120"
            case "canvas":
                let data: [String: Any] = ["lights": model.canvas.lights.map { [Double($0.x), Double($0.y), Double($0.size), $0.selected ? 1.0 : 0.0] }, "tree": model.rows.filter { model.treeSelection.contains($0.path) }.map(\.name), "order": model.order.filter { model.orderSelection.contains($0.id) }.map(\.name), "handles": model.canvas.adorner?.handles.map { [String(describing: $0.handle), String($0.x), String($0.y)] } ?? [], "angle": model.canvas.adorner?.angle ?? 0, "band": model.canvas.band != nil]
                guard let encoded = try? JSONSerialization.data(withJSONObject: data, options: [.sortedKeys]) else { return "FAIL canvas encoding" }
                return String(data: encoded, encoding: .utf8)!
            case "native-canvas":
                guard args.count >= 4, let x = Double(args[2]), let y = Double(args[3]), let canvas = view(PropCanvasView.self, in: model.window?.contentView), let window = canvas.window else { return "FAIL native-canvas down/drag/up x y [cmd/shift]" }
                let type: NSEvent.EventType = args[1] == "down" ? .leftMouseDown : args[1] == "drag" ? .leftMouseDragged : .leftMouseUp
                var flags: NSEvent.ModifierFlags = []; if args.contains("cmd") { flags.insert(.command) }; if args.contains("shift") { flags.insert(.shift) }
                guard let event = NSEvent.mouseEvent(with: type, location: canvas.convert(NSPoint(x: x, y: y), to: nil), modifierFlags: flags, timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: type == .leftMouseUp ? 0 : 1) else { return "FAIL native canvas event" }
                NSApp.postEvent(event, atStart: false); try? await Task.sleep(for: .milliseconds(100)); return "posted canvas \(args[1])"
            case "canvas-perf":
                guard let canvas = view(PropCanvasView.self, in: model.window?.contentView), let bitmap = canvas.bitmapImageRepForCachingDisplay(in: canvas.visibleRect) else { return "FAIL no canvas" }
                var times: [Double] = []
                for _ in 0..<20 { let start = ProcessInfo.processInfo.systemUptime; canvas.cacheDisplay(in: canvas.visibleRect, to: bitmap); times.append((ProcessInfo.processInfo.systemUptime - start) * 1000) }
                times.sort(); return String(format: "lights=%d cached-draw-ms median=%.3f max=%.3f visible=%.0fx%.0f", model.canvas.lights.count, times[10], times.last!, canvas.visibleRect.width, canvas.visibleRect.height)
            case "order": return model.order.map { "\($0.order):\($0.name)" }.joined(separator: "|")
            case "order-select", "order-context", "focus-order", "order-drop":
                guard let table = view(NSTableView.self, in: model.window?.contentView, matching: { $0.accessibilityLabel() == "Light order" }) else { return "FAIL no order table" }
                if verb == "order-drop" {
                    guard args.count >= 3, let index = Int(args[1]) else { return "FAIL order-drop index names" }
                    let names = Set(args.dropFirst(2)); let rows = model.order.indices.filter { names.contains(model.order[$0].name) }
                    let items = rows.compactMap { table.dataSource?.tableView?(table, pasteboardWriterForRow: $0) }
                    let board = NSPasteboard(name: NSPasteboard.Name("com.vixen.prop-test.\(UUID().uuidString)")); defer { board.releaseGlobally() }
                    board.writeObjects(items)
                    let info = PropTestDraggingInfo(pasteboard: board, table: table, foreign: names.contains("foreign"))
                    guard table.dataSource?.tableView?(table, validateDrop: info, proposedRow: index, proposedDropOperation: .above) == .move else { return "refused" }
                    return table.dataSource?.tableView?(table, acceptDrop: info, row: index, dropOperation: .above) == true ? "native table drop accepted" : "FAIL accepted but not taken"
                }
                if verb == "order-select" { let names = Set(args.dropFirst()); table.selectRowIndexes(IndexSet(model.order.indices.filter { names.contains(model.order[$0].name) }), byExtendingSelection: false) }
                if verb == "order-context" {
                    guard args.count == 2, let menu = table.menu else { return "FAIL order-context title" }; menu.delegate?.menuWillOpen?(menu)
                    let index = menu.indexOfItem(withTitle: args[1]); guard index >= 0, menu.items[index].isEnabled else { return "disabled" }; menu.performActionForItem(at: index)
                }
                model.window?.makeFirstResponder(table); return "native order selected=\(table.selectedRowIndexes.count)"
            case "editing": return "editing=\(model.window?.firstResponder is NSText)"
            case "element-info":
                guard let info = model.element else { return "none" }
                return "name=\(info.name) face=\(info.faceComponent) color=\(info.faceColor) model=\(info.modelType) children=\(info.children) lights=\(info.lights) size=\(info.lightSize) error=\(info.error ?? "-")"
            case "responder":
                guard args.count == 2, ["cut", "copy", "paste", "undo", "redo"].contains(args[1]), let responder = model.window?.firstResponder else { return "FAIL responder action" }
                return responder.tryToPerform(NSSelectorFromString(args[1] + ":"), with: nil) ? "performed window responder \(args[1])" : "FAIL no responder action"
            case "expand": await model.treeHandle.expand(model.rows.filter { $0.childCount > 0 }.map(\.path)); return "expanded native tree"
            case "label-window": guard args.count == 2 else { return "FAIL label-window title" }; model.window?.title = args[1]; return "labelled window"
            case "focus-outline":
                @MainActor func find(_ view: NSView) -> NSOutlineView? {
                    if let outline = view as? NSOutlineView { return outline }
                    for child in view.subviews { if let outline = find(child) { return outline } }; return nil
                }
                guard let window = model.window, let root = window.contentView, let outline = find(root) else { return "FAIL no outline" }
                window.makeFirstResponder(outline); return "focused native outline rows=\(outline.numberOfRows) selected=\(outline.selectedRowIndexes)"
            case "context":
                guard args.count == 2 else { return "FAIL context title" }; let menu = model.elementMenu(); let index = menu.indexOfItem(withTitle: args[1])
                guard index >= 0, menu.items[index].isEnabled else { return "FAIL disabled/missing context action" }
                menu.performActionForItem(at: index); return "performed native context action"
            case "state-color":
                guard args.count == 2, let index = Int(args[1]), model.states.items.indices.contains(index), let root = model.window?.contentView else { return "FAIL state-color row" }
                let id = model.states.items[index].id
                @MainActor func find(_ view: NSView) -> NSColorWell? {
                    if let well = view as? NSColorWell, well.identifier?.rawValue == id { return well }
                    for child in view.subviews { if let well = find(child) { return well } }; return nil
                }
                guard let well = find(root) else { return "FAIL no native state color well" }
                well.activate(true); return "opened native color well"
            case "color-panel":
                let panel = NSColorPanel.shared
                guard panel.isVisible else { return "FAIL color panel closed" }
                panel.mode = .RGB; return "native RGB color panel"
            case "color-fields": return textControls(in: NSColorPanel.shared).map { ($0 as? NSTextField)?.stringValue ?? "?" }.joined(separator: "|")
            case "focus-color-text":
                guard args.count == 2, let index = Int(args[1]) else { return "FAIL focus-color-text index" }
                let panel = NSColorPanel.shared; panel.makeFirstResponder(nil); let fields = textControls(in: panel)
                guard fields.indices.contains(index), let field = fields[index] as? NSTextField else { return "FAIL color field index" }
                panel.makeFirstResponder(field); field.selectText(nil); return "focused native color text"
            case "close-color": NSColorPanel.shared.orderOut(nil); return "closed color panel"
            case "state-header":
                guard args.count == 2, let index = Int(args[1]), let window = model.window else { return "FAIL state-header index" }
                @MainActor func find(_ view: NSView) -> NSTableView? {
                    if let table = view as? NSTableView, table.accessibilityLabel() == "State Items" { return table }
                    for child in view.subviews { if let table = find(child) { return table } }; return nil
                }
                guard let root = window.contentView, let table = find(root), table.headerView != nil, table.tableColumns.indices.contains(index) else { return "FAIL no state table/header" }
                // Inactive offscreen windows ignore table-header presses. Set the native header's
                // descriptor, which invokes the same NSTableView data-source callback.
                guard let prototype = table.tableColumns[index].sortDescriptorPrototype, let key = prototype.key else { return "FAIL no sortable column" }
                let ascending = table.sortDescriptors.first?.key == key ? !(table.sortDescriptors.first?.ascending ?? false) : prototype.ascending
                table.sortDescriptors = [NSSortDescriptor(key: key, ascending: ascending)]
                try? await Task.sleep(for: .milliseconds(100)); return "native header \(key) ascending=\(ascending)"
            case "native-hit":
                guard args.count == 3, let x = Double(args[1]), let y = Double(args[2]), let window = model.window, let content = window.contentView else { return "FAIL native-hit x y" }
                let point = NSPoint(x: x, y: content.bounds.height - y)
                var view = content.hitTest(content.superview?.convert(point, from: nil) ?? point); var names: [String] = []
                while let current = view { names.append(String(describing: type(of: current))); view = current.superview }
                return "height=\(content.bounds.height) hit=\(names.joined(separator: ">"))"
            case "native-click":
                guard args.count >= 3, let x = Double(args[1]), let y = Double(args[2]), let window = model.window, let content = window.contentView else { return "FAIL native-click x y [count]" }
                let count = args.count > 3 ? Int(args[3]) ?? 1 : 1
                window.makeKey()
                for kind in [NSEvent.EventType.leftMouseDown, .leftMouseUp] {
                    if let event = NSEvent.mouseEvent(with: kind, location: NSPoint(x: x, y: content.bounds.height - y), modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: count, pressure: kind == .leftMouseDown ? 1 : 0) { NSApp.postEvent(event, atStart: false) }
                }
                try? await Task.sleep(for: .milliseconds(200)); return "posted native click"
            case "focus-text":
                guard args.count == 2, let index = Int(args[1]), let window = model.window else { return "FAIL focus-text index" }
                window.makeFirstResponder(nil)
                let fields = textControls(in: window)
                guard fields.indices.contains(index) else { return "FAIL field index in \(fields.count)" }
                let field = fields[index]
                window.makeFirstResponder(field)
                if let text = field as? NSTextField { text.selectText(nil) }
                else if let text = field as? NSTextView { text.selectAll(nil) }
                return "focused native text \(index)"
            case "text-fields":
                guard let window = model.window else { return "FAIL no window" }
                return textControls(in: window).map { field in
                    let value = (field as? NSTextField)?.stringValue ?? (field as? NSTextView)?.string ?? ""
                    return "\(type(of: field)):\(value)"
                }.joined(separator: "|")
            case "host": return "host=\(model.window?.title ?? "missing")"
            case "state": return "name=\(model.state.propName) dirty=\(model.state.dirty) rows=\(model.rows.count) lights=\(model.canvas.lights.count) size=\(model.state.canvasWidth)x\(model.state.canvasHeight) undo=\(model.state.undoName ?? "-") mode=\(model.state.mode) path=\(model.state.filePath ?? "-")"
            case "background": guard args.count == 2 else { return "FAIL background path" }; Task { await model.assignBackground(path: args[1]) }; return "requested"
            case "draw":
                guard args.count == 3, let x = Double(args[1]), let y = Double(args[2]) else { return "FAIL draw x y" }
                model.mutate { $0.setMode(mode: .draw); $0.pointerDown(x: x, y: y, command: false, shift: false); $0.pointerUp() }; return "drawn"
            case "save": guard args.count == 2 else { return "FAIL save path" }; return await model.save(path: args[1]) ? "saved" : "FAIL save"
            case "open": guard args.count == 2 else { return "FAIL open path" }; await model.open(path: args[1]); return "opened"
            case "open-async": guard args.count == 2 else { return "FAIL open-async path" }; Task { await model.open(path: args[1]) }; return "requested open"
            case "xmodel": guard args.count == 2 else { return "FAIL xmodel path" }; Task { await model.importXModel(path: args[1], confirm: true) }; return "requested"
            case "vendor-state":
                guard let vendor = model.vendorRequest else { return "closed" }
                return "vendors=\(vendor.view.vendors.joined(separator: "|")) current=\(vendor.view.name) product=\(vendor.view.product?.name ?? "-") models=\(vendor.view.product?.models.count ?? 0)"
            case "vendor-expand": model.vendorRequest?.testExpand?(); return "expanded native outline"
            case "vendor-select":
                guard let vendor = model.vendorRequest, args.count == 2 else { return "FAIL no vendor/path" }; vendor.selectProduct(args[1]); return "selected"
            case "vendor-change":
                guard let vendor = model.vendorRequest, args.count == 2, let index = UInt32(args[1]) else { return "FAIL no vendor/index" }; vendor.selectVendor(index); return "changed"
            case "vendor-download": guard let vendor = model.vendorRequest, args.count == 2, let index = UInt32(args[1]) else { return "FAIL no vendor/index" }; vendor.selectModel(index); return "requested download"
            case "vendor-tab": model.vendorRequest?.tab = 1; return "model options"
            case "xmodel-choices": return model.xmodelRequest?.choices.map { "\($0.index):\($0.displayName):\($0.modelType):\($0.supported)" }.joined(separator: "|") ?? "none"
            case "undo": model.documentUndoManager.undo(); return "undone"
            case "redo": model.documentUndoManager.redo(); return "redone"
            case "select":
                let rows = model.rows.filter { args.dropFirst().contains($0.name) }; model.editor.setTreeSelection(pathsText: rows.map(\.path)); model.refresh(); return "selected \(rows.count)"
            case "rows": return model.rows.map { "\($0.name):\($0.lightCount)" }.joined(separator: "|")
            case "lights": return model.canvas.lights.map { "\($0.x),\($0.y),\($0.size)" }.joined(separator: "|")
            case "preferences": return "size=\(model.app?.core?.propPreferences().lightSize ?? 0)"
            case "rename": model.rename(findReplace: args.count > 1 && args[1] == "replace"); return "opened rename"
            case "tab":
                guard args.count == 2 else { return "FAIL tab name" }
                model.editor.setTab(tab: args[1] == "state" ? .state : args[1] == "order" ? .order : .layout); model.refresh(); return "tab \(args[1])"
            case "states": return "definitions=\(model.states.definitions.count) items=\(model.states.items.map { "\($0.name):\($0.count):\($0.colorHex)" }.joined(separator: "|")) errors=\(model.states.validation.count)"
            case "add-state":
                do { try model.editor.addStateDefinition(name: args.dropFirst().joined(separator: " ")); model.refresh(); return "added" } catch { return "FAIL \(error)" }
            case "assign":
                guard args.count == 3, let x = Double(args[1]), let y = Double(args[2]) else { return "FAIL assign x y" }
                model.mutate { $0.setMode(mode: .select); $0.pointerDown(x: x, y: y, command: false, shift: false); $0.pointerUp() }; return "assigned"
            case "shutdown":
                shutdownResult = "pending"; Task { shutdownResult = await model.app?.prepareDocumentsForShutdown() == true ? "allowed" : "cancelled" }; return "requested"
            case "shutdown-status": return shutdownResult
            default: return "FAIL unknown prop verb"
            }
        }
    }
}
