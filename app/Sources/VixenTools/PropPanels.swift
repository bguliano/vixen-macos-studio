import AppKit
import SwiftUI
import VixenBridge
import VixenKit

final class PropTreeProvider: OutlineDataProvider {
    weak var model: PropEditorModel?
    private var cachedRevision = -1
    private var children: [String: [PropEdTreeRow]] = [:]
    init(model: PropEditorModel) { self.model = model }
    func outlineChildren(of parent: String?) -> [OutlineRow] {
        guard let model else { return [] }
        if cachedRevision != model.revision {
            children = Dictionary(grouping: model.rows) { row in row.path.split(separator: "/").dropLast().joined(separator: "/") }
            cachedRevision = model.revision
        }
        return (children[parent ?? ""] ?? []).map { OutlineRow(id: $0.path, title: $0.name, hasChildren: $0.childCount > 0, icon: $0.isGroup ? .folder : .ball(model.state.lightColor), childCount: Int($0.childCount)) }
    }
    func outlineRename(item: String, to title: String) {
        guard let model, let row = model.rows.first(where: { $0.path == item }) else { return }
        model.mutate { _ = try? $0.rename(idText: row.id, name: title) }
    }
    func outlineCanDrop(items: [String], parent: String?, index: Int?, copy: Bool) -> Bool {
        guard let model, let parent else { return false }
        return !copy && model.editor.canDrop(dragged: items, parent: parent, index: index.map(UInt32.init))
    }
    func outlineDrop(items: [String], parent: String?, index: Int?, copy: Bool) {
        guard !copy, let model, let parent else { return }
        model.mutate { _ = $0.dropRows(dragged: items, parent: parent, index: index.map(UInt32.init), reverse: NSEvent.modifierFlags.contains(.command)) }
    }
}

struct PropElementPanel: View {
    @Bindable var model: PropEditorModel
    var body: some View {
        VStack(spacing: 4) {
            Picker("Element panel", selection: Binding(get: { model.state.tab }, set: { value in model.mutate { $0.setTab(tab: value) } })) {
                Text("Layout").tag(PropEdTab.layout)
                Text("Order").tag(PropEdTab.order)
                Text("State").tag(PropEdTab.state)
            }.pickerStyle(.segmented).labelsHidden().padding(6)
            if model.state.tab == .layout {
                OutlineView(provider: model.treeProvider, handle: model.treeHandle, allowsDragging: true, allowsRename: true,
                    onSelectionChange: { selection in model.treeSelection = Set(selection.ids); model.selectTree(model.treeSelection) },
                    onCommand: { command in
                        switch command { case .copy: model.copy(); case .paste: model.paste(); case .pasteAsNew: model.paste(asNew: true); case .delete: model.deleteLights() }
                    }, canPerform: { command in
                        switch command { case .copy: model.state.canCopy; case .paste: model.canPaste; case .pasteAsNew: model.canPasteAsNew; case .delete: model.canvas.lights.contains(where: \.selected) }
                    }, menu: { model.elementMenu() })
                    .task { model.refreshTree(initial: true) }
            } else if model.state.tab == .order {
                PropOrderTable(model: model)
            } else {
                PropStatePanel(model: model)
            }
        }
    }
}

private final class PropActionTarget: NSObject {
    let perform: () -> Void
    init(_ action: @escaping () -> Void) { perform = action }
    @objc func run() { perform() }
}
private func PropActionItem(_ title: String, enabled: Bool, action: @escaping () -> Void) -> NSMenuItem {
    let target = PropActionTarget(action)
    let item = NSMenuItem(title: title, action: #selector(PropActionTarget.run), keyEquivalent: "")
    item.target = target; item.representedObject = target; item.isEnabled = enabled
    return item
}

extension PropEditorModel {
    func elementMenu() -> NSMenu {
        let menu = NSMenu(); menu.autoenablesItems = false
        menu.addItem(PropActionItem("Create Empty Group", enabled: state.canCreateEmptyGroup) { [weak self] in self?.group(0) })
        menu.addItem(PropActionItem("Copy to New Group", enabled: state.canGroup) { [weak self] in self?.group(1) })
        menu.addItem(PropActionItem("Move to New Group", enabled: state.canGroup) { [weak self] in self?.group(2) })
        menu.addItem(.separator())
        menu.addItem(PropActionItem("Cut", enabled: state.canCopy) { [weak self] in self?.copy(cut: true) })
        menu.addItem(PropActionItem("Copy", enabled: state.canCopy) { [weak self] in self?.copy() })
        menu.addItem(PropActionItem("Paste", enabled: canPaste) { [weak self] in self?.paste() })
        menu.addItem(PropActionItem("Paste as New", enabled: canPasteAsNew) { [weak self] in self?.paste(asNew: true) })
        menu.addItem(.separator())
        menu.addItem(PropActionItem("Delete", enabled: state.canDeleteElements) { [weak self] in self?.deleteElements() })
        menu.addItem(PropActionItem("Rename", enabled: !treeSelection.isEmpty) { [weak self] in self?.rename() })
        menu.addItem(PropActionItem("Find/Replace Rename", enabled: treeSelection.count > 1) { [weak self] in self?.rename(findReplace: true) })
        return menu
    }
}

private struct PropOrderTable: NSViewRepresentable {
    let model: PropEditorModel
    func makeCoordinator() -> Coordinator { Coordinator(model: model) }
    func makeNSView(context: Context) -> NSScrollView {
        let table = OrderView(); table.owner = model
        for (id, title, width) in [("order", "Order", 55.0), ("name", "Name", 210.0)] {
            let column = NSTableColumn(identifier: .init(id)); column.title = title; column.width = width; column.isEditable = id == "name"; table.addTableColumn(column)
        }
        table.allowsMultipleSelection = true; table.dataSource = context.coordinator; table.delegate = context.coordinator
        table.setDraggingSourceOperationMask(.move, forLocal: true); table.registerForDraggedTypes([.init("com.vixenlights.prop-order")])
        let menu = NSMenu(); menu.autoenablesItems = false; menu.delegate = context.coordinator
        menu.addItem(PropActionItem("Reverse", enabled: false) { model.mutate { _ = $0.orderReverse() } })
        menu.addItem(PropActionItem("Rename", enabled: false) { model.rename() })
        menu.addItem(PropActionItem("Find/Replace Rename", enabled: false) { model.rename(findReplace: true) }); table.menu = menu
        let scroll = NSScrollView(); scroll.documentView = table; scroll.hasVerticalScroller = true; table.setAccessibilityLabel("Light order")
        context.coordinator.table = table; return scroll
    }
    func updateNSView(_ view: NSScrollView, context: Context) {
        let c = context.coordinator
        guard c.revision != model.revision else { return }; c.revision = model.revision
        c.syncing = true; c.table?.reloadData()
        c.table?.selectRowIndexes(IndexSet(model.order.indices.filter { model.orderSelection.contains(model.order[$0].id) }), byExtendingSelection: false)
        c.syncing = false
    }
    final class OrderView: NSTableView {
        weak var owner: PropEditorModel?
        var selectionTime = Date.distantPast
        override func keyDown(with event: NSEvent) {
            if event.keyCode == 120, event.modifierFlags.intersection([.command, .option, .control, .shift]).isEmpty {
                if selectedRow >= 0, Date().timeIntervalSince(selectionTime) >= 0.75 { editColumn(1, row: selectedRow, with: nil, select: true) }
            } else if event.keyCode == 51 || event.keyCode == 117 { owner?.deleteLights() } else { super.keyDown(with: event) }
        }
    }
    final class Coordinator: NSObject, NSTableViewDataSource, NSTableViewDelegate, NSMenuDelegate {
        let model: PropEditorModel
        weak var table: NSTableView?
        var revision = -1
        var syncing = false
        init(model: PropEditorModel) { self.model = model }
        func numberOfRows(in tableView: NSTableView) -> Int { model.order.count }
        func tableView(_ tableView: NSTableView, objectValueFor tableColumn: NSTableColumn?, row: Int) -> Any? {
            tableColumn?.identifier.rawValue == "order" ? String(model.order[row].order) : model.order[row].name
        }
        func tableView(_ tableView: NSTableView, shouldEdit tableColumn: NSTableColumn?, row: Int) -> Bool {
            tableColumn?.identifier.rawValue == "name" && tableView.selectedRowIndexes.contains(row) && Date().timeIntervalSince((tableView as? OrderView)?.selectionTime ?? .distantPast) >= 0.75
        }
        func tableView(_ tableView: NSTableView, setObjectValue object: Any?, for tableColumn: NSTableColumn?, row: Int) {
            guard tableColumn?.identifier.rawValue == "name", model.order.indices.contains(row), let name = object as? String else { return }
            let id = model.order[row].id; model.mutate { _ = try? $0.rename(idText: id, name: name) }
        }
        func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
            guard selector == #selector(NSResponder.cancelOperation(_:)), let table, model.order.indices.contains(table.editedRow) else { return false }
            textView.string = model.order[table.editedRow].name
            _ = table.abortEditing(); table.window?.makeFirstResponder(table)
            return true
        }
        func menuWillOpen(_ menu: NSMenu) {
            if let table, table.clickedRow >= 0, !table.selectedRowIndexes.contains(table.clickedRow) { table.selectRowIndexes(IndexSet(integer: table.clickedRow), byExtendingSelection: false) }
            menu.items[0].isEnabled = !model.orderSelection.isEmpty
            menu.items[1].isEnabled = !model.orderSelection.isEmpty
            menu.items[2].isEnabled = model.orderSelection.count > 1
        }
        func tableViewSelectionDidChange(_ notification: Notification) {
            (table as? OrderView)?.selectionTime = Date()
            guard !syncing, let table else { return }
            model.orderSelection = Set(table.selectedRowIndexes.compactMap { model.order.indices.contains($0) ? model.order[$0].id : nil }); model.selectOrder(model.orderSelection)
        }
        func tableView(_ tableView: NSTableView, pasteboardWriterForRow row: Int) -> NSPasteboardWriting? {
            let item = NSPasteboardItem(); item.setString(model.order[row].id, forType: .init("com.vixenlights.prop-order")); return item
        }
        func tableView(_ tableView: NSTableView, validateDrop info: NSDraggingInfo, proposedRow row: Int, proposedDropOperation op: NSTableView.DropOperation) -> NSDragOperation {
            guard info.draggingSource as? NSTableView === tableView else { return [] }
            tableView.setDropRow(row, dropOperation: .above); return .move
        }
        func tableView(_ tableView: NSTableView, acceptDrop info: NSDraggingInfo, row: Int, dropOperation: NSTableView.DropOperation) -> Bool {
            guard info.draggingSource as? NSTableView === tableView else { return false }
            let ids = info.draggingPasteboard.pasteboardItems?.compactMap { $0.string(forType: .init("com.vixenlights.prop-order")) } ?? []
            guard !ids.isEmpty else { return false }; model.mutate { _ = $0.orderDrop(dragged: ids, index: UInt32(max(row, 0))) }; return true
        }
    }
}

struct PropInfoPanel: View {
    let model: PropEditorModel
    @State private var tab = 0
    @State private var metadata = 0
    var body: some View {
        VStack(spacing: 4) {
            Picker("Information", selection: $tab) { Text("Prop Info").tag(0); Text("Element Info").tag(1) }.pickerStyle(.segmented).labelsHidden().padding(6)
            ScrollView {
                if tab == 0 {
                    Picker("Metadata", selection: $metadata) { Text("General").tag(0); Text("Physical").tag(1); Text("Vendor").tag(2); Text("Notes").tag(3) }.labelsHidden().padding(.horizontal, 8)
                    Form {
                        if metadata == 0 { field("Name", .name); field("Type", .type).help("A useful category like Christmas, Halloween, General"); field("Created By", .createdBy); LabeledContent("Created", value: model.state.creationDate); LabeledContent("Modified", value: model.state.modifiedDate) }
                        if metadata == 1 {
                            field("Material", .material).help("The predominate material the props is made of.")
                            field("Height", .physicalHeight).help("The overall height of the prop.")
                            field("Width", .physicalWidth).help("The overall width of the prop.")
                            field("Depth", .physicalDepth).help("The overall depth of the prop.")
                            field("Node Count", .nodeCount)
                            LabeledContent("Bulb Type") { PropBulbType(text: value(.bulbType)).frame(minWidth: 95, maxWidth: .infinity) }.help("The light type used.")
                            Picker("Color Mode", selection: value(.colorMode)) { ForEach(propEdColorModes(), id: \.value) { Text($0.label).tag($0.value) } }.help("Declares color handling for the entire prop.\nIf the prop contains multiple color modes, choose other.\nThis maps to color handling in the Display Setup.")
                        }
                        if metadata == 2 { field("Name", .vendorName); field("Contact", .vendorContact); field("Website", .vendorWebsite); field("Email", .vendorEmail); field("Phone", .vendorPhone) }
                        if metadata == 3 { PropNotes(text: value(.notes)).frame(minHeight: 100) }
                    }.padding(8)
                } else if let info = model.element {
                    Form {
                        TextField("Name", text: Binding(get: { model.element?.name ?? "" }, set: { text in model.mutate { _ = try? $0.setElementName(idText: info.id, name: text) } }))
                        Picker("Face Component", selection: Binding(get: { model.element?.faceComponent ?? "None" }, set: { text in model.mutate { _ = try? $0.setFaceComponent(idText: info.id, component: text) } })) { ForEach(propEdFaceComponents(), id: \.value) { Text($0.label).tag($0.value) } }.help("Face component associated with this element for Lip-Sync.")
                        TextField("Face Color", text: Binding(get: { model.element?.faceColor ?? "" }, set: { text in model.mutate { _ = try? $0.setFaceColor(idText: info.id, hex: text) } })).help("Face color in Hex associated with this element for Lip-Sync.")
                        Picker("Model Type", selection: Binding(get: { model.element?.modelType ?? "None" }, set: { text in model.mutate { _ = try? $0.setModelType(idText: info.id, modelType: text) } })) { ForEach(propEdModelTypes(), id: \.value) { Text($0.label).tag($0.value) } }.help("Custom prop role for this element when State definitions are authored.")
                        LabeledContent("Children", value: String(info.children)).help("Number of child elements associated with this element."); LabeledContent("Lights", value: String(info.lights)).help("Number of light elements under this element."); LabeledContent("Light Size", value: info.lightSize).help("Size of light element(s) under this element.")
                        if let error = info.error { Text(error).foregroundStyle(.red) }
                    }.padding(8)
                } else { Text("Select an element to see its properties.").foregroundStyle(.secondary).padding() }
            }
        }.font(.caption)
    }
    private func value(_ field: PropEdField) -> Binding<String> { Binding(get: { _ = model.revision; return model.editor.propField(field: field) }, set: { text in model.mutate { _ = $0.setPropField(field: field, value: text) } }) }
    private func field(_ title: String, _ field: PropEdField) -> some View { TextField(title, text: value(field)) }
}

struct PropScaleSheet: View {
    let model: PropEditorModel
    @Environment(\.dismiss) private var dismiss
    @State private var dialog: PropEdScale?
    @State private var state: PropEdScaleState?
    @State private var error = ""
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Scale Background Image").font(.headline)
            if let state {
                LabeledContent("Original bitmap:", value: "\(state.sourceWidth) × \(state.sourceHeight) px")
                LabeledContent("Current editor:", value: "\(state.currentWidth) × \(state.currentHeight) px")
                Picker("Units:", selection: Binding(get: { state.percent }, set: { value in change { $0.setPercent(percent: value) } })) { Text("Pixels").tag(false); Text("Percent").tag(true) }
                LabeledContent("Width:") { TextField("Width", text: Binding(get: { state.width }, set: { value in change { $0.setInput(width: true, text: value) } })).labelsHidden() }
                LabeledContent("Height:") { TextField("Height", text: Binding(get: { state.height }, set: { value in change { $0.setInput(width: false, text: value) } })).labelsHidden() }
                Toggle("Lock aspect ratio", isOn: Binding(get: { state.locked }, set: { value in change { $0.setLocked(locked: value) } }))
                Toggle("Scale existing light positions", isOn: Binding(get: { state.scaleLights }, set: { value in change { $0.setScaleLights(scale: value) } })).disabled(!state.hasLights)
                Text("Resulting size: \(state.resultWidth) × \(state.resultHeight) px").bold()
                if !state.errors.isEmpty { Text(state.errors.joined(separator: "\n")).font(.caption).foregroundStyle(.red) }
            }
            if !error.isEmpty { Text(error).foregroundStyle(.red) }
            HStack { Spacer(); Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction); Button("Scale") { apply() }.keyboardShortcut(.defaultAction).disabled(state == nil || state?.errors.isEmpty == false) } // STUDIO: a verb on the confirmation; Edit > Undo takes the scale back
        }.padding(20).frame(width: 400)
            .onAppear { do { dialog = try model.editor.backgroundScale(); state = dialog?.state() } catch { self.error = error.userMessage } }
    }
    private func change(_ edit: (PropEdScale) -> Void) { guard let dialog else { return }; edit(dialog); state = dialog.state() }
    private func apply() {
        guard let state, state.errors.isEmpty else { return }
        let before = model.state.revision
        do { try model.editor.scaleBackground(width: state.resultWidth, height: state.resultHeight, scaleLights: state.scaleLights); model.refresh(); model.registerChange(after: before); dismiss() }
        catch { self.error = error.userMessage }
    }
}
