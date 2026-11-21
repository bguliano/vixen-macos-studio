import AppKit
import SwiftUI
import VixenBridge
import VixenKit

/// State Mapper (F194 to F210).
struct StateMapperSheet: View {
    let context: DialogContext
    let session: StateSession
    let onFinish: (Bool) -> Void
    @State private var names: [String] = []
    @State private var selectedDefinition: UInt32 = 0
    @State private var description = ""
    @State private var items: [DialogStateItem] = []
    @State private var selection = Set<Int>()
    @State private var issues = DialogStateIssues(errors: [], warnings: [], blocking: false)
    @State private var preview = false
    @State private var treeRevision = 0
    @State private var treeSelection = Set<UInt32>()
    @State private var sortOrder: [KeyPathComparator<Row>] = []
    @State private var prompt: Prompt?
    @State private var choosingColor: ColorChoice?
    @State private var alert: DialogAlert?
    @State private var frameProbe: DialogFrameProbe?

    struct Prompt: Identifiable {
        enum Kind { case add, rename, copy }
        let id = UUID()
        let kind: Kind
        let title: String
        let text: String
        let current: String?
    }

    struct ColorChoice: Identifiable {
        let id = UUID()
        let row: Int
        let current: UInt32
        let allowed: [UInt32]
    }

    /// A row of the items grid.
    struct Row: Identifiable {
        let id: Int
        let name: String
        let hex: String
        let count: UInt32
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            DialogTitle("State Mapper")
            ForEach(issues.errors, id: \.self) { Label($0, systemImage: "xmark.octagon").foregroundStyle(.red).font(.caption) }
            ForEach(issues.warnings, id: \.self) { Label($0, systemImage: "exclamationmark.triangle").foregroundStyle(.orange).font(.caption) }
            definitionRow
            HStack(alignment: .top) {
                Text("Description")
                TextEditor(text: $description)
                    .frame(minHeight: 48, maxHeight: 60)
                    .border(.separator)
                    .onChange(of: description) { session.setDescription(text: description) }
            }
            HSplitView {
                itemsPane.frame(minWidth: 320).padding(.trailing, 6)
                assignmentsPane.frame(minWidth: 280).padding(.leading, 6)
            }
            StudioDoneButton(studio) { changed in session.close(); onFinish(changed) } // STUDIO: live, closes with Done (B9)
        }
        .padding(16)
        .frame(minWidth: 720, idealWidth: 920, minHeight: 420, idealHeight: 560)
        // STUDIO-BEGIN state-mapper-live-hooks: every edit refreshes the items, names or description; each applies (B9)
        .onChange(of: items) { studioApply() }
        .onChange(of: names) { studioApply() }
        .onChange(of: description) { studioApply() }
        // STUDIO-END state-mapper-live-hooks
        .onAppear {
            refreshAll()
            if UITestHarness.isActive {
                frameProbe = context.testWatchFrames()
                UITestHarness.hooks["stateMapper"] = { _ in
                    let frame = frameProbe.flatMap { $0.snapshot().data(using: .utf8) }
                        .flatMap { try? JSONSerialization.jsonObject(with: $0) } ?? NSNull()
                    let info: [String: Any] = ["items": items.map { ["name": $0.name, "count": $0.count, "color": $0.color] },
                        "selection": selection.sorted(), "preview": preview, "frame": frame]
                    guard let data = try? JSONSerialization.data(withJSONObject: info, options: [.sortedKeys]),
                          let text = String(data: data, encoding: .utf8) else { return "FAIL serialization" }
                    return text
                }
            }
        }
        .sheet(item: $prompt) { p in
            TextPromptSheet(
                title: p.title,
                prompt: "Name",
                text: p.text,
                verb: p.kind == .add ? "Add" : p.kind == .rename ? "Rename" : "Copy", // STUDIO: a verb (B9)
                validate: { context.stateNameError(name: $0, current: p.current, existing: names) },
                warn: { context.stateNameWarning(name: $0, current: p.current, existing: names) }
            ) { response in
                prompt = nil
                if let response {
                    finishPrompt(p.kind, response)
                }
            }
        }
        .sheet(item: $choosingColor) { choice in
            if choice.allowed.isEmpty {
                FreeColorSheet(current: choice.current) { finishColor(choice, $0) }
            } else {
                DiscreteColorSheet(colors: choice.allowed, current: choice.current) { finishColor(choice, $0) }
            }
        }
        .dialogAlert($alert)
    }

    private var definitionRow: some View {
        HStack {
            Text("Name")
            Picker("Name", selection: Binding(get: { selectedDefinition }, set: { chooseDefinition($0) })) {
                ForEach(Array(names.enumerated()), id: \.offset) { index, name in
                    Text(name).tag(UInt32(index))
                }
            }
            .labelsHidden()
            .fixedSize()
            .frame(minWidth: 180, alignment: .leading)
            Button("Add") {
                prompt = Prompt(kind: .add, title: "Add State Definition", text: session.nextDefinitionName(), current: nil)
            }
            .disabled(issues.blocking)
            Button("Delete", action: deleteDefinition).disabled(names.count <= 1)
            Menu("...") {
                Button("Rename") {
                    let name = names[Int(selectedDefinition)]
                    prompt = Prompt(kind: .rename, title: "Rename State Definition", text: name, current: name)
                }
                .disabled(issues.blocking)
                Button("Copy") {
                    prompt = Prompt(kind: .copy, title: "Copy State Definition", text: names[Int(selectedDefinition)] + " Copy", current: nil)
                }
                .disabled(issues.blocking)
            }
            .fixedSize()
        }
    }

    private var rows: [Row] {
        items.enumerated().map { Row(id: $0.offset, name: $0.element.name, hex: $0.element.hex, count: $0.element.count) }
    }

    private var itemsPane: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text("Preview")
                Toggle(preview ? "On" : "Off", isOn: Binding(get: { preview }, set: { on in
                    preview = on
                    session.setPreview(on: on, selected: selection.map { UInt32($0) })
                }))
                .toggleStyle(.button)
            }
            Text("State Items").fontWeight(.bold).frame(maxWidth: .infinity)
            Table(rows, selection: $selection, sortOrder: $sortOrder) {
                TableColumn("Name", value: \.name) { row in nameCell(row) }
                TableColumn("Color", value: \.hex) { row in colorCell(row) }.width(120)
                TableColumn("Count", value: \.count) { row in Text(String(row.count)).monospacedDigit() }.width(72)
            }
            .onChange(of: sortOrder) { persistSort() }
            .onChange(of: selection) { selectionChanged() }
            HStack {
                Spacer()
                Button("Add") {
                    select(Int(session.addItem()))
                }
                Button("Copy") {
                    if let only = selection.first, let row = session.copyItem(index: UInt32(only)) {
                        select(Int(row))
                    }
                }
                .disabled(selection.count != 1)
                Button("Remove", action: removeItems).disabled(selection.isEmpty)
                Button("Move selected State Item up", systemImage: "arrow.up") { move(-1) }
                    .labelStyle(.iconOnly)
                    .help("Move selected State Item up")
                    .disabled(selection.count != 1 || selection.first == 0)
                Button("Move selected State Item down", systemImage: "arrow.down") { move(1) }
                    .labelStyle(.iconOnly)
                    .help("Move selected State Item down")
                    .disabled(selection.count != 1 || (selection.first ?? 0) >= items.count - 1)
            }
        }
    }

    private func nameCell(_ row: Row) -> some View {
        NameCell(name: row.name) { name in
            session.setItemName(index: UInt32(row.id), name: name)
            refreshItems()
        }
    }

    private func colorCell(_ row: Row) -> some View {
        let item = items[row.id]
        return Text(item.hex)
            .font(.caption.monospaced())
            .foregroundStyle(item.textIsBlack ? Color.black : Color.white)
            .frame(maxWidth: .infinity)
            .padding(.vertical, 2)
            .background(Color(argb: item.color))
            .contentShape(Rectangle())
            .onTapGesture(count: 2) {
                choosingColor = ColorChoice(row: row.id, current: item.color, allowed: session.itemColorChoices(index: UInt32(row.id)))
            }
    }

    private var assignmentsPane: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Spacer()
                Text("Assigned Elements").fontWeight(.medium)
                Spacer()
                Button("Toggle Selected") {
                    if let item = selection.first {
                        session.toggleAssignments(item: UInt32(item), nodes: Array(treeSelection))
                        assignmentsChanged()
                    }
                }
                .help("Toggle selected assigned elements")
                .disabled(treeSelection.isEmpty)
            }
            if selection.count == 1, let item = selection.first, items.indices.contains(item) {
                AssignmentOutline(
                    session: session,
                    item: UInt32(item),
                    revision: treeRevision,
                    selection: $treeSelection,
                    toggled: { node, on in
                        session.setAssignment(item: UInt32(item), node: node, on: on)
                        assignmentsChanged()
                    }
                )
                .id(items[item].id)
            } else {
                Color.clear.frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
    }

    private func refreshAll() {
        names = session.definitionNames()
        selectedDefinition = session.selectedDefinition()
        description = session.description()
        refreshItems()
    }

    private func refreshItems() {
        items = session.items()
        issues = session.issues()
    }

    private func assignmentsChanged() {
        refreshItems()
        treeRevision += 1
        session.refreshPreview(selected: selection.map { UInt32($0) })
    }

    private func chooseDefinition(_ index: UInt32) {
        let actual = session.selectDefinition(index: index)
        selectedDefinition = actual
        if actual == index {
            selection = []
            treeSelection = []
            description = session.description()
            refreshItems()
            session.refreshPreview(selected: [])
        }
    }

    private func finishPrompt(_ kind: Prompt.Kind, _ name: String) {
        let error: String?
        switch kind {
        case .add: error = session.addDefinition(name: name)
        case .rename: error = session.renameDefinition(name: name)
        case .copy: error = session.copyDefinition(name: name)
        }
        if let error {
            alert = .notice("State Mapper", error)
            return
        }
        if kind != .rename {
            selection = []
        }
        refreshAll()
    }

    private func deleteDefinition() {
        let name = names[Int(selectedDefinition)]
        alert = DialogAlert(
            title: "Delete State Definition",
            message: "Delete State definition '\(name)'?",
            choices: [
                .init("Delete", role: .destructive) {
                    session.deleteDefinition()
                    selection = []
                    refreshAll()
                },
                .init("Cancel", role: .cancel),
            ]
        )
    }

    private func select(_ row: Int) {
        refreshItems()
        selection = [row]
    }

    private func removeItems() {
        let rows = selection.sorted().map { UInt32($0) }
        let prompt = session.removePrompt(rows: rows)
        alert = DialogAlert(
            title: prompt.title,
            message: prompt.text,
            choices: [
                .init("Delete", role: .destructive) {
                    let next = session.removeItems(rows: rows)
                    refreshItems()
                    selection = next.map { [Int($0)] } ?? []
                    // Removing a row can select a different item at the same index. A selection onChange alone
                    // would leave the old item's preview showing in that case.
                    selectionChanged()
                },
                .init("Cancel", role: .cancel),
            ]
        )
    }

    private func move(_ offset: Int32) {
        guard let row = selection.first, let moved = session.moveItem(index: UInt32(row), offset: offset) else { return }
        refreshItems()
        selection = [Int(moved)]
    }

    /// F199: a header sort becomes the stored order, then the sort clears.
    private func persistSort() {
        guard let comparator = sortOrder.first else { return }
        let path = comparator.keyPath as AnyKeyPath
        let column: DialogStateColumn = path == \Row.name ? .name : (path == \Row.hex ? .color : .count)
        session.sortItems(column: column, ascending: comparator.order == .forward)
        sortOrder = []
        selection = []
        refreshItems()
    }

    private func selectionChanged() {
        treeSelection = []
        session.refreshPreview(selected: selection.map { UInt32($0) })
    }

    private func finishColor(_ choice: ColorChoice, _ color: UInt32?) {
        choosingColor = nil
        guard let color else { return }
        session.setItemColor(index: UInt32(choice.row), color: color)
        refreshItems()
        session.refreshPreview(selected: selection.map { UInt32($0) })
    }

    private func ok() {
        if let error = session.apply() {
            alert = .notice("State Mapper", error)
            return
        }
        session.close()
        onFinish(true)
    }

    private func cancel() {
        session.close()
        onFinish(false)
    }

    // STUDIO-BEGIN state-mapper-live: applies each change as it's made instead of at OK (B9)
    // A blocking problem (a missing or repeated name) leaves the element's states as last applied; the issues above say what
    // to fix. The visit is one step on Display Setup's Undo.
    @State private var studio = StudioLiveSheet(step: "State Mapping")

    private func studioApply() {
        guard !issues.blocking else { return }
        if let error = session.apply() {
            studio.error = error
        } else {
            studio.appliedNow()
        }
    }
    // STUDIO-END state-mapper-live
}

/// A name that commits on focus loss, as upstream's grid does.
private struct NameCell: View {
    let name: String
    let commit: (String) -> Void
    @State private var text = ""
    @FocusState private var focused: Bool

    var body: some View {
        TextField("", text: $text)
            .labelsHidden()
            .textFieldStyle(.plain)
            .focused($focused)
            .onAppear { text = name }
            .onChange(of: name) { text = name }
            .onChange(of: focused) { if !focused { commit(text) } }
            .onSubmit { commit(text) }
    }
}

/// F205: the node's tree, with a checkbox per row. Rows come from Rust as they're needed, and rows under a checked group are disabled.
struct AssignmentOutline: NSViewRepresentable {
    let session: StateSession
    let item: UInt32
    let revision: Int
    @Binding var selection: Set<UInt32>
    let toggled: (UInt32, Bool) -> Void

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeNSView(context: Context) -> NSScrollView {
        let outline = NSOutlineView()
        outline.headerView = nil
        outline.style = .fullWidth
        outline.rowSizeStyle = .small
        outline.allowsMultipleSelection = true
        outline.usesAutomaticRowHeights = false
        outline.rowHeight = 20
        outline.indentationPerLevel = 14
        let column = NSTableColumn(identifier: .init("title"))
        column.resizingMask = .autoresizingMask
        outline.addTableColumn(column)
        outline.outlineTableColumn = column
        outline.columnAutoresizingStyle = .uniformColumnAutoresizingStyle
        outline.dataSource = context.coordinator
        outline.delegate = context.coordinator
        let scroll = NSScrollView()
        scroll.documentView = outline
        scroll.hasVerticalScroller = true
        scroll.autohidesScrollers = true
        scroll.borderType = .bezelBorder
        context.coordinator.outline = outline
        context.coordinator.lastRevision = revision
        outline.reloadData()
        context.coordinator.expandToChecked()
        return scroll
    }

    func updateNSView(_ scroll: NSScrollView, context: Context) {
        let c = context.coordinator
        c.parent = self
        if c.lastRevision != revision {
            c.lastRevision = revision
            c.refresh()
        }
    }

    @MainActor
    final class Coordinator: NSObject, NSOutlineViewDataSource, NSOutlineViewDelegate {
        final class Box: NSObject {
            var row: DialogAssignmentRow
            var children: [Box]?
            init(_ row: DialogAssignmentRow) {
                self.row = row
            }
        }

        var parent: AssignmentOutline
        weak var outline: NSOutlineView?
        var lastRevision = 0
        private var roots: [Box]?
        private var restoringSelection = false

        init(_ parent: AssignmentOutline) {
            self.parent = parent
        }

        private func children(of box: Box?) -> [Box] {
            if let box {
                if box.children == nil {
                    box.children = parent.session.assignmentChildren(item: parent.item, parent: box.row.index).map(Box.init)
                }
                return box.children ?? []
            }
            if roots == nil {
                roots = parent.session.assignmentChildren(item: parent.item, parent: nil).map(Box.init)
            }
            return roots ?? []
        }

        /// Selecting an item expands to its checked nodes.
        func expandToChecked() {
            guard let outline else { return }
            let wanted = Set(parent.session.assignmentExpanded(item: parent.item))
            func expand(_ boxes: [Box]) {
                for b in boxes where wanted.contains(b.row.index) {
                    outline.expandItem(b)
                    expand(children(of: b))
                }
            }
            expand(children(of: nil))
        }

        /// New rows with the same expansion.
        func refresh() {
            guard let outline else { return }
            let selected = parent.selection
            restoringSelection = true
            defer { restoringSelection = false }
            var expanded = Set<UInt32>()
            func collect(_ boxes: [Box]) {
                for b in boxes where outline.isItemExpanded(b) {
                    expanded.insert(b.row.index)
                    collect(b.children ?? [])
                }
            }
            collect(roots ?? [])
            roots = nil
            outline.reloadData()
            func restore(_ boxes: [Box]) {
                for b in boxes where expanded.contains(b.row.index) {
                    outline.expandItem(b)
                    restore(children(of: b))
                }
            }
            restore(children(of: nil))
            // Upstream keeps selected enabled assignments after Toggle Selected; only descendants disabled
            // by a newly checked group lose their selection. Rebuilding the rows must preserve that state.
            var rows = IndexSet()
            var retained = Set<UInt32>()
            for row in 0..<outline.numberOfRows {
                if let box = outline.item(atRow: row) as? Box, box.row.enabled, selected.contains(box.row.index) {
                    rows.insert(row)
                    retained.insert(box.row.index)
                }
            }
            outline.selectRowIndexes(rows, byExtendingSelection: false)
            if parent.selection != retained {
                parent.selection = retained
            }
        }

        func outlineView(_ o: NSOutlineView, numberOfChildrenOfItem item: Any?) -> Int {
            children(of: item as? Box).count
        }

        func outlineView(_ o: NSOutlineView, child index: Int, ofItem item: Any?) -> Any {
            children(of: item as? Box)[index]
        }

        func outlineView(_ o: NSOutlineView, isItemExpandable item: Any) -> Bool {
            (item as? Box)?.row.hasChildren ?? false
        }

        func outlineView(_ o: NSOutlineView, viewFor column: NSTableColumn?, item: Any) -> NSView? {
            guard let box = item as? Box else { return nil }
            let button = NSButton(checkboxWithTitle: box.row.name, target: self, action: #selector(checked(_:)))
            button.state = box.row.checked ? .on : .off
            button.isEnabled = box.row.enabled
            button.tag = Int(box.row.index)
            button.font = .systemFont(ofSize: NSFont.smallSystemFontSize + 1)
            button.lineBreakMode = .byTruncatingTail
            return button
        }

        func outlineView(_ o: NSOutlineView, shouldSelectItem item: Any) -> Bool {
            (item as? Box)?.row.enabled ?? false
        }

        @objc func checked(_ sender: NSButton) {
            parent.toggled(UInt32(sender.tag), sender.state == .on)
        }

        func outlineViewSelectionDidChange(_ notification: Notification) {
            guard !restoringSelection, let outline else { return }
            parent.selection = Set(outline.selectedRowIndexes.compactMap { (outline.item(atRow: $0) as? Box)?.row.index })
        }
    }
}
