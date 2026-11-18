import AppKit
import SwiftUI
import UniformTypeIdentifiers
import VixenBridge
import VixenKit

enum ElementMapperPresenter {
    static func present(core: VixenCore, name: String, sources: [ElementMappingSource], over parent: NSWindow?, completion: @escaping ([ElementMappingResult]?) -> Void) {
        Task {
            do {
                let inputs = sources.map { MapperSource(id: $0.id, name: $0.name) }
                let dialog = try await Task.detached { try core.elementMapper(name: name, sources: inputs) }.value
                show(dialog: dialog, over: parent) { result, _ in completion(result) }
            } catch {
                await SchedulerAlerts.tell("Element Mapper", error.userMessage, style: .error, window: parent)
                completion(nil)
            }
        }
    }

    static func show(dialog: ElementMapperDialog, over parent: NSWindow?, completion: @escaping ([ElementMappingResult]?, String?) -> Void) {
                let model = ElementMapperModel(dialog: dialog)
                ElementMapperModel.current = model
                let host = NSHostingController(rootView: ElementMapperSheet(model: model).studioAppearance()) // STUDIO: follow the chosen appearance
                host.sizingOptions = []
                let sheet = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 800, height: 600), styleMask: [.titled, .resizable], backing: .buffered, defer: false)
                // Swift/ARC owns this window. AppKit's legacy release-on-close
                // would release it again when a standalone mapper finishes.
                sheet.isReleasedWhenClosed = false
                sheet.contentViewController = host
                // Attaching an NSHostingController replaces the initial content rect with
                // its fitting size. Restore the intended size after that assignment.
                sheet.contentMinSize = NSSize(width: 640, height: 400)
                sheet.setContentSize(NSSize(width: 800, height: 600))
                model.window = sheet
                model.refresh()
                model.complete = { [weak sheet] result in
                    guard let sheet else { return }
                    ElementMapperModel.lastResult = result?.map { "\($0.sourceId)=\($0.targetId)" }.joined(separator: "|") ?? "cancelled"
                    ElementMapperModel.current = nil
                    if let parent = sheet.sheetParent { parent.endSheet(sheet) } else { sheet.close() }
                    completion(result, result == nil ? nil : dialog.view().path)
                }
                if let parent { parent.beginSheet(sheet, completionHandler: nil) } else { sheet.center(); sheet.makeKeyAndOrderFront(nil) }
    }
}

@Observable
final class ElementMapperModel {
    @ObservationIgnored static weak var current: ElementMapperModel?
    @ObservationIgnored static var lastResult = ""
    let dialog: ElementMapperDialog
    var view: MapperView
    var revision = 0
    var selectedSources: [String] = []
    var selectedDestination: String?
    var busy = false
    @ObservationIgnored weak var window: NSWindow?
    @ObservationIgnored var complete: ([ElementMappingResult]?) -> Void = { _ in }
    init(dialog: ElementMapperDialog) { self.dialog = dialog; view = dialog.view() }
    func refresh() {
        view = dialog.view(); revision += 1
        window?.title = "Element Mapper - \(view.name)\(view.modified ? " *" : "")"
    }
    func assign(sources: [String], target: String?) {
        do { for source in sources { try dialog.assign(source: source, target: target) }; refresh() }
        catch { Task { await SchedulerAlerts.tell("Element Mapper", error.userMessage, style: .error, window: window) } }
    }
    func discardChanges() async -> Bool {
        if !view.modified { return true }
        return await SchedulerAlerts.confirm("Discard Changes?", "The current element map has unsaved changes.", style: .warning, confirm: "Discard Changes", cancel: "Cancel", destructive: true, window: window)
    }
    func newMap() {
        Task {
            guard await discardChanges() else { return }
            let alert = NSAlert(); alert.messageText = "New Map - Name"; alert.informativeText = "Enter new map name."
            let field = NSTextField(string: "New Map"); field.frame = NSRect(x: 0, y: 0, width: 260, height: 24); alert.accessoryView = field
            alert.addButton(withTitle: "OK"); alert.addButton(withTitle: "Cancel")
            let result: NSApplication.ModalResponse
            if let window { result = await withCheckedContinuation { continuation in alert.beginSheetModal(for: window) { continuation.resume(returning: $0) } } }
            else { result = alert.runModal() }
            if result == .alertFirstButtonReturn { dialog.reset(name: field.stringValue); selectedSources = []; refresh() }
        }
    }
    func openMap(tree: Bool = false) {
        Task {
            if !tree, !(await discardChanges()) { return }
            let panel = NSOpenPanel(); panel.title = tree ? "Load Element Tree" : "Open Element Mapping"
            panel.allowedContentTypes = [UTType(filenameExtension: tree ? "v3e" : "vmap") ?? .json]
            panel.canChooseDirectories = false; panel.allowsMultipleSelection = false
            let response: NSApplication.ModalResponse
            if let window { response = await panel.beginSheetModal(for: window) } else { response = await panel.begin() }
            guard response == .OK, let url = panel.url else { return }
            busy = true
            do {
                let dialog = self.dialog
                try await Task.detached { if tree { try dialog.loadTree(path: url.path) } else { try dialog.openMap(path: url.path) } }.value
                selectedSources = []; refresh()
                if !tree, !view.hasSourceTree, view.missingSources > 0,
                   await SchedulerAlerts.confirm("Add Missing Sources.", "Add missing source elements to map?", style: .information, confirm: "Add", cancel: "Skip", window: window) {
                    dialog.addMissingSources(); refresh()
                }
            } catch { await SchedulerAlerts.tell(tree ? "Error Loading Element Tree" : "Error Loading Map", error.userMessage, style: .error, window: window) }
            busy = false
        }
    }
    /// Upstream treats cancelling Save's file panel as success, including when OK invokes it.
    func save(asNew: Bool = false) async -> Bool {
        var path = asNew ? nil : view.path
        if path == nil || !FileManager.default.fileExists(atPath: path!) {
            let panel = NSSavePanel(); panel.title = "Save Element Map"
            panel.allowedContentTypes = [UTType(filenameExtension: "vmap") ?? .json]; panel.nameFieldStringValue = view.name + ".vmap"
            let response: NSApplication.ModalResponse
            if let window { response = await panel.beginSheetModal(for: window) } else { response = await panel.begin() }
            guard response == .OK, let url = panel.url else { return true }
            path = url.path
        }
        guard let path else { return true }
        busy = true
        defer { busy = false }
        do { let dialog = self.dialog; try await Task.detached { try dialog.saveMap(path: path) }.value; refresh(); return true }
        catch { await SchedulerAlerts.tell("Error Saving Map", "An error occurred saving the map.\n\(error.userMessage)", style: .error, window: window); return false }
    }
    func accept() {
        Task {
            if view.modified, !(await save()) { return }
            complete(dialog.acceptedEntries().compactMap { entry in entry.targetId.map { ElementMappingResult(sourceId: entry.sourceId, targetId: $0) } })
        }
    }
}

private struct ElementMapperSheet: View {
    @Bindable var model: ElementMapperModel
    var body: some View {
        VStack(spacing: 10) {
            HStack {
                Menu("File") {
                    Button("New Map") { model.newMap() }
                    Button("Open Map…") { model.openMap() }
                    Button("Save Map") { Task { _ = await model.save() } }.disabled(!model.view.modified)
                    Button("Save Map As…") { Task { _ = await model.save(asNew: true) } }
                    Divider()
                    Button("Load Element Tree…") { model.openMap(tree: true) }
                }.fixedSize()
                Spacer()
                Button("View Help") {
                    let link = "https://www.vixenlights.com/vixen-3-documentation/sequencer/sequence-import/"
                    if UITestHarness.isActive { UITestHarness.note("help \(link)") }
                    else if let url = URL(string: link) { NSWorkspace.shared.open(url) }
                }
            }
            HSplitView {
                VStack(alignment: .leading) {
                    Text("Source").font(.headline)
                    if model.view.hasSourceTree { MapperOutline(model: model, source: true) }
                    else { MapperTable(model: model) }
                }.frame(minWidth: 270)
                VStack(alignment: .leading) { Text("Destination").font(.headline); MapperOutline(model: model, source: false) }.frame(minWidth: 260)
            }
            HStack {
                Button("Map Selected") { model.assign(sources: model.selectedSources, target: model.selectedDestination) }
                    .disabled(model.selectedSources.isEmpty || model.selectedDestination == nil)
                Button("Clear Mapping") { model.assign(sources: model.selectedSources, target: nil) }.disabled(model.selectedSources.isEmpty)
                Spacer()
                if model.busy { ProgressView().controlSize(.small) }
                Button("Cancel") { model.complete(nil) }.keyboardShortcut(.cancelAction)
                Button("OK") { model.accept() }.keyboardShortcut(.defaultAction)
            }
        }.padding(14)
            .frame(minWidth: 640, idealWidth: 800, minHeight: 400, idealHeight: 600)
            .disabled(model.busy)
    }
}

private let mapperPasteboard = NSPasteboard.PasteboardType("com.vixenlights.element-map-destination")

private final class MapperOutlineView: NSOutlineView {
    var clear: (() -> Void)?
    override func keyDown(with event: NSEvent) {
        if event.keyCode == 51 || event.keyCode == 117, let clear { clear(); return }
        super.keyDown(with: event)
    }
}

private struct MapperOutline: NSViewRepresentable {
    let model: ElementMapperModel
    let source: Bool
    func makeCoordinator() -> Coordinator { Coordinator(model: model, source: source) }
    func makeNSView(context: Context) -> NSScrollView {
        let tree = MapperOutlineView()
        let column = NSTableColumn(identifier: .init("element")); tree.addTableColumn(column); tree.outlineTableColumn = column
        tree.headerView = nil; tree.rowHeight = 23; tree.allowsMultipleSelection = source
        tree.dataSource = context.coordinator; tree.delegate = context.coordinator
        tree.target = context.coordinator; tree.doubleAction = #selector(Coordinator.doubleClicked)
        if source {
            tree.registerForDraggedTypes([mapperPasteboard]); tree.clear = { model.assign(sources: model.selectedSources, target: nil) }
        } else { tree.setDraggingSourceOperationMask(.copy, forLocal: true) }
        let scroll = NSScrollView(); scroll.documentView = tree; scroll.hasVerticalScroller = true; scroll.autohidesScrollers = true
        context.coordinator.tree = tree
        tree.setAccessibilityLabel(source ? "Source elements" : "Destination elements")
        return scroll
    }
    func updateNSView(_ scroll: NSScrollView, context: Context) {
        let c = context.coordinator
        guard c.revision != model.revision else { return }
        c.revision = model.revision
        guard let tree = c.tree else { return }
        let selected = Set((source ? model.selectedSources : model.selectedDestination.map { [$0] } ?? []))
        let expanded = c.items.values.filter { tree.isItemExpanded($0) }.sorted { $0.row.path.split(separator: "/").count < $1.row.path.split(separator: "/").count }
        c.updating = true
        c.children.removeAll(); tree.reloadData()
        for item in expanded where tree.row(forItem: item) >= 0 { tree.expandItem(item) }
        let selection = IndexSet((0..<tree.numberOfRows).filter { row in
            (tree.item(atRow: row) as? Coordinator.Item).map { selected.contains($0.row.id) } ?? false
        })
        tree.selectRowIndexes(selection, byExtendingSelection: false)
        c.updating = false
    }
    final class Coordinator: NSObject, NSOutlineViewDataSource, NSOutlineViewDelegate {
        final class Item: NSObject { var row: MapperTreeRow; init(_ row: MapperTreeRow) { self.row = row } }
        let model: ElementMapperModel
        let source: Bool
        var children: [String: [Item]] = [:]
        var items: [String: Item] = [:]
        var revision = -1
        var updating = false
        weak var tree: MapperOutlineView?
        init(model: ElementMapperModel, source: Bool) { self.model = model; self.source = source }
        func rows(_ item: Any?) -> [Item] {
            let path = (item as? Item)?.row.path
            let key = path ?? ""
            if let cached = children[key] { return cached }
            let rows = model.dialog.treeRows(source: source, parent: path).map { row in
                if let item = items[row.path] { item.row = row; return item }
                let item = Item(row); items[row.path] = item; return item
            }
            children[key] = rows; return rows
        }
        func outlineView(_ outlineView: NSOutlineView, numberOfChildrenOfItem item: Any?) -> Int { rows(item).count }
        func outlineView(_ outlineView: NSOutlineView, child index: Int, ofItem item: Any?) -> Any { rows(item)[index] }
        func outlineView(_ outlineView: NSOutlineView, isItemExpandable item: Any) -> Bool { (item as? Item)?.row.childCount ?? 0 > 0 }
        func outlineView(_ outlineView: NSOutlineView, viewFor tableColumn: NSTableColumn?, item: Any) -> NSView? {
            guard let row = (item as? Item)?.row else { return nil }
            let field = NSTextField(labelWithString: row.name + (row.targetName.isEmpty ? "" : " → " + row.targetName))
            field.lineBreakMode = .byTruncatingTail; field.textColor = row.active ? .systemYellow : .labelColor
            return field
        }
        func outlineViewSelectionDidChange(_ notification: Notification) {
            guard !updating, let tree else { return }
            let ids = tree.selectedRowIndexes.compactMap { (tree.item(atRow: $0) as? Item)?.row.id }
            if source { model.selectedSources = ids } else { model.selectedDestination = ids.first }
        }
        @objc func doubleClicked() { if !source, let target = model.selectedDestination { model.assign(sources: model.selectedSources, target: target) } }
        func outlineView(_ outlineView: NSOutlineView, pasteboardWriterForItem item: Any) -> NSPasteboardWriting? {
            guard !source, let item = item as? Item else { return nil }
            let p = NSPasteboardItem(); p.setString(item.row.id, forType: mapperPasteboard); return p
        }
        func outlineView(_ outlineView: NSOutlineView, validateDrop info: NSDraggingInfo, proposedItem item: Any?, proposedChildIndex index: Int) -> NSDragOperation {
            let row = outlineView.row(at: outlineView.convert(info.draggingLocation, from: nil))
            guard source, row >= 0, let hit = outlineView.item(atRow: row) as? Item,
                  info.draggingPasteboard.string(forType: mapperPasteboard) != nil else { return [] }
            outlineView.setDropItem(hit, dropChildIndex: NSOutlineViewDropOnItemIndex); return .copy
        }
        func outlineView(_ outlineView: NSOutlineView, acceptDrop info: NSDraggingInfo, item: Any?, childIndex index: Int) -> Bool {
            let row = outlineView.row(at: outlineView.convert(info.draggingLocation, from: nil))
            guard source, row >= 0, let hit = outlineView.item(atRow: row) as? Item,
                  let target = info.draggingPasteboard.string(forType: mapperPasteboard) else { return false }
            model.assign(sources: [hit.row.id], target: target); return true
        }
    }
}

private final class MapperTableView: NSTableView {
    var clear: (() -> Void)?
    override func keyDown(with event: NSEvent) {
        if event.keyCode == 51 || event.keyCode == 117 { clear?(); return }
        super.keyDown(with: event)
    }
}

private struct MapperTable: NSViewRepresentable {
    let model: ElementMapperModel
    func makeCoordinator() -> Coordinator { Coordinator(model: model) }
    func makeNSView(context: Context) -> NSScrollView {
        let table = MapperTableView()
        for (id,title) in [("source","Source Name"),("target","Destination Name")] {
            let column = NSTableColumn(identifier: .init(id)); column.title = title; column.width = 170; table.addTableColumn(column)
        }
        table.columnAutoresizingStyle = .uniformColumnAutoresizingStyle
        table.rowHeight = 23; table.allowsMultipleSelection = true; table.dataSource = context.coordinator; table.delegate = context.coordinator
        table.registerForDraggedTypes([mapperPasteboard]); table.clear = { model.assign(sources: model.selectedSources, target: nil) }
        table.setAccessibilityLabel("Element mappings")
        let scroll = NSScrollView(); scroll.documentView = table; scroll.hasVerticalScroller = true; scroll.autohidesScrollers = true
        context.coordinator.table = table; return scroll
    }
    func updateNSView(_ scroll: NSScrollView, context: Context) {
        let c = context.coordinator
        if c.revision != model.revision {
            c.revision = model.revision
            let selected = Set(model.selectedSources)
            c.updating = true
            c.table?.reloadData()
            c.table?.selectRowIndexes(IndexSet(model.view.rows.indices.filter { selected.contains(model.view.rows[$0].sourceId) }), byExtendingSelection: false)
            c.updating = false
        }
    }
    final class Coordinator: NSObject, NSTableViewDataSource, NSTableViewDelegate {
        let model: ElementMapperModel
        var revision = -1
        var updating = false
        weak var table: MapperTableView?
        init(model: ElementMapperModel) { self.model = model }
        func numberOfRows(in tableView: NSTableView) -> Int { model.view.rows.count }
        func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
            let entry = model.view.rows[row]
            let field = NSTextField(labelWithString: tableColumn?.identifier.rawValue == "source" ? entry.sourceName : entry.targetName)
            field.lineBreakMode = .byTruncatingTail; return field
        }
        func tableViewSelectionDidChange(_ notification: Notification) {
            guard !updating else { return }
            model.selectedSources = table?.selectedRowIndexes.compactMap { row in model.view.rows.indices.contains(row) ? model.view.rows[row].sourceId : nil } ?? []
        }
        func tableView(_ tableView: NSTableView, validateDrop info: NSDraggingInfo, proposedRow row: Int, proposedDropOperation dropOperation: NSTableView.DropOperation) -> NSDragOperation {
            let hitRow = tableView.row(at: tableView.convert(info.draggingLocation, from: nil))
            guard model.view.rows.indices.contains(hitRow), info.draggingPasteboard.string(forType: mapperPasteboard) != nil else { return [] }
            tableView.setDropRow(hitRow, dropOperation: .on); return .copy
        }
        func tableView(_ tableView: NSTableView, acceptDrop info: NSDraggingInfo, row: Int, dropOperation: NSTableView.DropOperation) -> Bool {
            let hitRow = tableView.row(at: tableView.convert(info.draggingLocation, from: nil))
            guard model.view.rows.indices.contains(hitRow), let target = info.draggingPasteboard.string(forType: mapperPasteboard) else { return false }
            model.assign(sources: [model.view.rows[hitRow].sourceId], target: target); return true
        }
    }
}
