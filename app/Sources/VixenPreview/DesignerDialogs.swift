import AppKit
import SwiftUI
import VixenBridge
import VixenKit

/// Upstream's dead or odd behaviour, kept until the owner decides. Each is a recommendation in the spec's Quirks and decisions, and changing one is this one line.
enum PreviewQuirks {
    /// Q2: nothing calls `PopulateTemplateList`, so Templates (Legacy) is always empty.
    static let populateLegacyTemplates = false
}

struct DesignerSheetView: View {
    let sheet: DesignerSheet
    let model: PreviewDesignerModel

    var body: some View {
        switch sheet {
        case .resizeBackground: ResizeBackgroundSheet(model: model)
        case .locationOffset: LocationOffsetSheet(model: model)
        case .singleBulb: SingleBulbSheet(model: model)
        case .templates: LegacyTemplatesSheet(model: model)
        case .createTemplate(let shape): CreateTemplateSheet(model: model, shape: shape)
        case .link(let linker, let shape): LinkElementsSheet(model: model, linker: linker, shape: shape)
        case .fixtureLink(let shape, let current): FixtureLinkSheet(model: model, shape: shape, current: current)
        }
    }
}

/// A number box that holds what's typed and reports the number. `NumericUpDown`'s range, clamped rather than throwing.
private struct NumberField: View {
    let title: String
    @Binding var value: Int
    let range: ClosedRange<Int>

    var body: some View {
        TextField(title, value: Binding(get: { value }, set: { value = min(max($0, range.lowerBound), range.upperBound) }), format: .number.grouping(.never))
            .labelsHidden()
            .textFieldStyle(.roundedBorder)
            .monospacedDigit()
            .frame(width: 90)
    }
}

// MARK: F45, F14

private struct ResizeBackgroundSheet: View {
    let model: PreviewDesignerModel
    @Environment(\.dismiss) private var dismiss
    @State private var width = 0
    @State private var height = 0
    @State private var originalWidth = 0
    @State private var originalHeight = 0
    @State private var locked = true
    @State private var scaleShapes = true

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Resize Preview").font(.headline)
            GroupBox("Existing Image Size") {
                Grid(alignment: .leading, horizontalSpacing: 8, verticalSpacing: 6) {
                    GridRow {
                        Text("Width:")
                        Text(String(originalWidth)).monospacedDigit()
                    }
                    GridRow {
                        Text("Height:")
                        Text(String(originalHeight)).monospacedDigit()
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(4)
            }
            GroupBox("New Image Size") {
                HStack {
                    Grid(alignment: .leading, horizontalSpacing: 8, verticalSpacing: 6) {
                        GridRow {
                            Text("Width:")
                            NumberField(title: "Width:", value: $width, range: 0...10000)
                        }
                        GridRow {
                            Text("Height:")
                            NumberField(title: "Height:", value: $height, range: 0...10000)
                                .disabled(locked)
                        }
                    }
                    Toggle(isOn: $locked) {
                        Image(systemName: locked ? "link" : "link")
                            .symbolVariant(locked ? .circle.fill : .none)
                    }
                    .toggleStyle(.button)
                    .help("Keep the aspect ratio")
                    .accessibilityLabel("Keep the aspect ratio")
                    Spacer()
                }
                .padding(4)
            }
            Toggle("Scale Shapes", isOn: $scaleShapes)
            HStack {
                Button("Help") { model.help("https://www.vixenlights.com/docs/usage/preview/preview-main-screen/#background") }
                Spacer()
                Button("Resize") { // STUDIO: a verb on the confirmation button; Undo takes the resize back
                    model.resizeBackground(width: Int32(width), height: Int32(height), scaleShapes: scaleShapes)
                    dismiss()
                }
                .keyboardShortcut(.defaultAction)
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
            }
        }
        .padding(16)
        .frame(width: 320)
        .onAppear {
            let s = model.designer?.settings()
            originalWidth = Int(s?.backgroundWidth ?? 0)
            originalHeight = Int(s?.backgroundHeight ?? 0)
            width = min(originalWidth, 10000)
            height = min(originalHeight, 10000)
        }
        .onChange(of: width) {
            // `height < 10` then the aspect, truncating: `(int)(origH * (newW / origW))`.
            if height < 10 {
                height = 10
            }
            if locked, originalWidth > 0 {
                height = min(Int(Double(originalHeight) * (Double(width) / Double(originalWidth))), 10000)
            }
        }
    }
}

// MARK: F48

private struct LocationOffsetSheet: View {
    let model: PreviewDesignerModel
    @Environment(\.dismiss) private var dismiss
    @State private var horizontal = ""
    @State private var vertical = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Location Offset").font(.headline)
            // STUDIO-BEGIN designer-live-offset: each field applies as it's committed, one step on the designer's Undo; Done closes
            Grid(alignment: .leading, horizontalSpacing: 8, verticalSpacing: 8) {
                GridRow {
                    Text("Horizontal")
                    TextField("Horizontal", text: $horizontal).labelsHidden().frame(width: 100)
                        .onSubmit(commit)
                }
                GridRow {
                    Text("Vertical")
                    TextField("Vertical", text: $vertical).labelsHidden().frame(width: 100)
                        .onSubmit(commit)
                }
            }
            Text("Added to every linked light's position when Save Locations writes it.")
                .font(.caption)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            HStack {
                Spacer()
                Button("Done") {
                    commit()
                    dismiss()
                }
                .keyboardShortcut(.defaultAction)
            }
        }
        .onDisappear(perform: commit)
        // STUDIO-END designer-live-offset
        .padding(16)
        .frame(width: 260)
        .onAppear {
            let s = model.designer?.settings()
            horizontal = String(Int(s?.locationOffsetX ?? 0))
            vertical = String(Int(s?.locationOffsetY ?? 0))
        }
    }

    // STUDIO-BEGIN designer-live-offset-commit: the offset as typed, live; the same values add no undo step
    private func commit() {
        // An empty box is 0, and `IntValue` truncates toward zero.
        let x = Double(horizontal).map { $0.rounded(.towardZero) } ?? 0
        let y = Double(vertical).map { $0.rounded(.towardZero) } ?? 0
        model.apply(model.designer?.studioSetLocationOffset(x: x, y: y))
        model.refreshSettings()
    }
    // STUDIO-END designer-live-offset-commit
}

// MARK: F49

private struct SingleBulbSheet: View {
    let model: PreviewDesignerModel
    @Environment(\.dismiss) private var dismiss
    @State private var prefix = "Pixel"
    @State private var index = 1
    @State private var size = 3

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Single Bulb Configuration").font(.headline)
            Grid(alignment: .leading, horizontalSpacing: 8, verticalSpacing: 8) {
                GridRow {
                    Text("Prefix Name")
                    TextField("Prefix Name", text: $prefix).labelsHidden().frame(width: 120)
                }
                GridRow {
                    Text("Suffix Starting Index")
                    NumberField(title: "Suffix Starting Index", value: $index, range: 0...Int(Int32.max))
                }
                GridRow {
                    Text("Light Size")
                    NumberField(title: "Light Size", value: $size, range: 1...100)
                }
            }
            HStack {
                Spacer()
                Button("Draw Bulbs") { // STUDIO: a verb on the confirmation button
                    model.singleBulbFinished(prefix: prefix, index: Int32(index), size: Int32(size))
                    dismiss()
                }
                .keyboardShortcut(.defaultAction)
                Button("Cancel", role: .cancel) {
                    model.singleBulbFinished(prefix: nil, index: 1, size: 3)
                    dismiss()
                }
                .keyboardShortcut(.cancelAction)
            }
        }
        .padding(16)
        .frame(width: 320)
    }
}

// MARK: F47

private struct LegacyTemplatesSheet: View {
    let model: PreviewDesignerModel
    @Environment(\.dismiss) private var dismiss
    @State private var rows: [DesignerTemplateRow] = []
    @State private var file = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("TemplateDialog").font(.headline)
            Picker("Template", selection: $file) {
                ForEach(rows, id: \.file) { Text($0.name).tag($0.file) }
            }
            .labelsHidden()
            HStack {
                Spacer()
                Button("Add to Preview") {
                    if !file.isEmpty {
                        do {
                            model.apply(try model.designer?.addTemplateFile(file: file))
                        } catch {
                            model.alert("Error Loading Template", error.userMessage)
                        }
                    }
                    dismiss()
                }
                .keyboardShortcut(.defaultAction)
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
            }
        }
        .padding(16)
        .frame(width: 300)
        .task {
            // Q2: upstream never fills this, and the switch is one line away. Off the main thread when it does: the store parses every template to name it.
            guard PreviewQuirks.populateLegacyTemplates, let designer = model.designer else { return }
            let found = await Task.detached { designer.templates() }.value
            rows = found.rows
            file = found.rows.first?.file ?? ""
            for error in found.errors {
                model.alert("Error Loading Template", error)
            }
        }
    }
}

// MARK: F47a

private struct CreateTemplateSheet: View {
    let model: PreviewDesignerModel
    let shape: UInt32
    @Environment(\.dismiss) private var dismiss
    @State private var name = ""
    @State private var invalid = false

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Create a Template").font(.headline)
            HStack {
                Text("Template Name")
                TextField("Template Name", text: $name).labelsHidden()
            }
            HStack {
                Spacer()
                Button("Save Template") { finish() } // STUDIO: a verb on the confirmation button
                    .keyboardShortcut(.defaultAction)
                // The group stays, as upstream.
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
            }
        }
        .padding(16)
        .frame(width: 340)
        .alert("Invalid Template Name", isPresented: $invalid) {
            Button("OK", role: .cancel) {}
        } message: {
            Text("The template name must be a valid file name. Please ensure there are no invalid characters in the template name.")
        }
    }

    /// Q12: the sheet stays open on the alert, where upstream closes it and goes on to throw.
    private func finish() {
        guard let designer = model.designer else {
            dismiss()
            return
        }
        guard designer.templateNameIsValid(name: name) else {
            invalid = true
            return
        }
        do {
            model.apply(try designer.saveGroupAsTemplate(shape: shape, name: name))
        } catch {
            model.alert("Template", error.userMessage)
        }
        dismiss()
    }
}

// MARK: F94

private struct FixtureLinkSheet: View {
    let model: PreviewDesignerModel
    let shape: UInt32
    let current: String?
    @Environment(\.dismiss) private var dismiss
    @State private var nodes: [DesignerFixtureNode] = []
    @State private var selection: String?
    @State private var linked: String? // STUDIO: designer-live-fixture-link, the link the shape has now

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Link Element").font(.headline)
            // STUDIO-BEGIN designer-live-fixture-link: choosing a row links it at once, one step on the designer's Undo; Done closes
            List(nodes, id: \.id, selection: $selection) { node in
                Text(node.name).tag(node.id)
            }
            .onChange(of: selection) { _, node in
                guard node != linked else { return }
                linked = node
                do {
                    model.apply(try model.designer?.studioSetFixtureNode(shape: shape, node: node))
                } catch {
                    model.alert("Link Element", error.userMessage)
                }
                model.properties.reload()
            }
            HStack {
                Button("Unlink") { selection = nil }
                    .disabled(selection == nil)
                    .help("Link this fixture to no element")
                Spacer()
                Button("Done") { dismiss() }
                    .keyboardShortcut(.defaultAction)
            }
        }
        // STUDIO-END designer-live-fixture-link
        .padding(16)
        .frame(width: 350, height: 400)
        .onAppear {
            nodes = model.designer?.fixtureNodes() ?? []
            linked = current // STUDIO: designer-live-fixture-link
            selection = current
        }
    }
}

// MARK: F93

private final class LinkTreeProvider: OutlineDataProvider {
    private weak var model: PreviewDesignerModel?

    init(model: PreviewDesignerModel) {
        self.model = model
    }

    func outlineChildren(of parent: String?) -> [OutlineRow] {
        guard let designer = model?.designer else { return [] }
        let node = parent.flatMap { $0.split(separator: "/").last }.map(String.init)
        return designer.elementRows(parent: node).map { row in
            OutlineRow(id: parent.map { "\($0)/\(row.id)" } ?? row.id, title: row.name, hasChildren: row.hasChildren)
        }
    }
}

/// The Linked Elements dialog. Pixels are clones, and nothing reaches the shape until OK.
private struct LinkElementsSheet: View {
    let model: PreviewDesignerModel
    let linker: PreviewLinker
    let shape: UInt32
    @Environment(\.dismiss) private var dismiss
    @State private var provider: LinkTreeProvider?
    @State private var strings: [String] = []
    @State private var string = 0
    @State private var rows: [DesignerLinkRow] = []
    @State private var lightCount = 1
    @State private var treeHandle = OutlineHandle()
    @State private var tableSelection = IndexSet()
    @State private var droppedOffRow = false

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Link Elements").font(.headline)
            HStack {
                Text("String to Link:")
                Picker("String to Link:", selection: $string) {
                    ForEach(Array(strings.enumerated()), id: \.offset) { index, name in
                        Text(name).tag(index)
                    }
                }
                .labelsHidden()
                .frame(width: 160)
                Spacer()
                if linker.lightCountEditable() {
                    Text("Light Count for String:")
                    NumberField(title: "Light Count for String:", value: $lightCount, range: 1...1000)
                    Button("Set") {
                        linker.setLightCount(string: UInt32(string), count: UInt32(lightCount))
                        changed() // STUDIO: designer-live-linking
                    }
                }
            }
            HSplitView {
                VStack(alignment: .leading, spacing: 4) {
                    Text("Available Elements").font(.caption)
                    if let provider {
                        OutlineView(
                            provider: provider,
                            handle: treeHandle,
                            allowsDragging: true,
                            accessibilityLabel: "Available Elements",
                            onSelectionChange: { _ in },
                            onDoubleClick: { row in doubleClicked(row) }
                        )
                    }
                }
                .frame(minWidth: 200)
                VStack(alignment: .leading, spacing: 4) {
                    Text("#        Linked Element").font(.caption)
                    LinkedRowsTable(
                        rows: rows,
                        selection: $tableSelection,
                        onDrop: { row, ids in
                            // The outline puts at most 16 rows on the pasteboard so a big drag starts at once. When what came is part of the tree's selection,
                            // the whole selection was dragged, in display order, which is upstream's top-to-bottom order too.
                            let selection = treeHandle.selectionInDisplayOrder
                            let dragged = Set(ids).isSubset(of: Set(selection)) ? selection : ids
                            let nodes = dragged.compactMap { $0.split(separator: "/").last.map(String.init) }
                            linker.dropNodes(string: UInt32(string), row: UInt32(row), nodes: nodes)
                            changed() // STUDIO: designer-live-linking
                        },
                        onCopyToString: { row in
                            linker.copyToThisString(string: UInt32(string), row: UInt32(row))
                            changed() // STUDIO: designer-live-linking
                        },
                        onCopyToAll: { row in
                            linker.copyToAllStrings(string: UInt32(string), row: UInt32(row))
                            changed() // STUDIO: designer-live-linking
                        },
                        onReverse: {
                            linker.reverse(string: UInt32(string))
                            changed() // STUDIO: designer-live-linking
                        }
                    )
                }
                .frame(minWidth: 240)
            }
            VStack(alignment: .leading, spacing: 1) {
                Text("- Drag elements from left to right to link them.")
                Text("- Drag a group with children to link them all.")
                Text("- ⌘-Click or ⇧-Click to select multiple elements to drag.")
            }
            .font(.caption)
            .foregroundStyle(.secondary)
            HStack {
                Button("Help") { model.help("https://www.vixenlights.com/docs/usage/preview/preview-linking-elements/") }
                Spacer()
                // STUDIO-BEGIN designer-live-linking-done: every change is already on the shape, and the sheet's changes are one step on the designer's Undo
                Button("Done") { dismiss() }
                    .keyboardShortcut(.defaultAction)
                // STUDIO-END designer-live-linking-done
            }
        }
        .padding(14)
        .frame(minWidth: 620, idealWidth: 700, minHeight: 480, idealHeight: 560)
        .onAppear {
            provider = LinkTreeProvider(model: model)
            strings = linker.strings()
            fill()
            Task { await treeHandle.loadRoots() }
            if UITestHarness.isActive {
                UITestHarness.hooks["previewLinker"] = { args in
                    guard args.first == "doubleClick", let row = treeHandle.selectionInDisplayOrder.first else {
                        return "FAIL needs a selected tree row"
                    }
                    doubleClicked(row)
                    return "assigned selected tree row to \(tableSelection.count) linked rows"
                }
            }
        }
        .onDisappear { if UITestHarness.isActive { UITestHarness.hooks["previewLinker"] = nil } }
        .onChange(of: string) { fill() }
    }

    // STUDIO-BEGIN designer-live-linking: the shape takes the sheet's links after each change
    private func changed() {
        model.apply(linker.studioApplyLive())
        model.properties.reload()
        fill()
    }
    // STUDIO-END designer-live-linking

    private func fill() {
        rows = linker.rows(string: UInt32(string))
        lightCount = Int(linker.lightCount(string: UInt32(string)))
    }

    /// Needs a selected tree node and a selected row: the node goes to each selected row in turn, so later rows overwrite earlier ones' spill.
    private func doubleClicked(_ row: String) {
        guard !tableSelection.isEmpty, let node = row.split(separator: "/").last.map(String.init) else { return }
        linker.setRows(string: UInt32(string), rows: tableSelection.map { UInt32($0) }, node: node)
        changed() // STUDIO: designer-live-linking
    }
}

/// `NSTableView` with a data source: a string can have thousands of pixels, and only rows on screen are ever made.
private struct LinkedRowsTable: NSViewRepresentable {
    let rows: [DesignerLinkRow]
    @Binding var selection: IndexSet
    let onDrop: (Int, [String]) -> Void
    let onCopyToString: (Int) -> Void
    let onCopyToAll: (Int) -> Void
    let onReverse: () -> Void

    /// `OutlineView`'s drag type.
    static let dragType = NSPasteboard.PasteboardType("com.vixenlights.outline-items")

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeNSView(context: Context) -> NSScrollView {
        let table = ContextTable()
        table.headerView = nil
        table.rowSizeStyle = .small
        table.allowsMultipleSelection = true
        table.style = .plain
        table.usesAlternatingRowBackgroundColors = true
        let number = NSTableColumn(identifier: .init("number"))
        number.width = 48
        number.minWidth = 40
        let name = NSTableColumn(identifier: .init("name"))
        name.resizingMask = .autoresizingMask
        table.addTableColumn(number)
        table.addTableColumn(name)
        table.columnAutoresizingStyle = .lastColumnOnlyAutoresizingStyle
        table.dataSource = context.coordinator
        table.delegate = context.coordinator
        table.registerForDraggedTypes([Self.dragType])
        table.setDraggingSourceOperationMask(.every, forLocal: true)
        table.menuProvider = { [weak coordinator = context.coordinator] row in
            coordinator?.menu(row: row)
        }
        let scroll = NSScrollView()
        scroll.documentView = table
        scroll.hasVerticalScroller = true
        scroll.autohidesScrollers = true
        context.coordinator.table = table
        return scroll
    }

    func updateNSView(_ scroll: NSScrollView, context: Context) {
        let c = context.coordinator
        let same = c.parent.rows.map(\.node) == rows.map(\.node)
        c.parent = self
        if !same {
            c.table?.reloadData()
        }
    }

    private final class ContextTable: NSTableView {
        var menuProvider: ((Int) -> NSMenu?)?

        override func menu(for event: NSEvent) -> NSMenu? {
            let row = row(at: convert(event.locationInWindow, from: nil))
            if row >= 0, !selectedRowIndexes.contains(row) {
                selectRowIndexes(IndexSet(integer: row), byExtendingSelection: false)
            }
            // clickedRow is the last left-clicked row and can be stale for a context menu.
            return menuProvider?(row)
        }
    }

    @MainActor
    final class Coordinator: NSObject, NSTableViewDataSource, NSTableViewDelegate {
        var parent: LinkedRowsTable
        weak var table: NSTableView?
        private var menuRow = -1

        init(_ parent: LinkedRowsTable) {
            self.parent = parent
        }

        func numberOfRows(in tableView: NSTableView) -> Int {
            parent.rows.count
        }

        func tableView(_ tableView: NSTableView, viewFor column: NSTableColumn?, row: Int) -> NSView? {
            let id = column?.identifier ?? NSUserInterfaceItemIdentifier("cell")
            let cell = (tableView.makeView(withIdentifier: id, owner: nil) as? NSTextField) ?? {
                let field = NSTextField(labelWithString: "")
                field.identifier = id
                field.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
                field.lineBreakMode = .byTruncatingTail
                return field
            }()
            let data = parent.rows[row]
            cell.stringValue = id.rawValue == "number" ? "\(data.number)" : data.elementName
            return cell
        }

        func tableViewSelectionDidChange(_ notification: Notification) {
            parent.selection = table?.selectedRowIndexes ?? IndexSet()
        }

        func tableView(_ tableView: NSTableView, validateDrop info: NSDraggingInfo, proposedRow row: Int, proposedDropOperation operation: NSTableView.DropOperation) -> NSDragOperation {
            // Upstream targets the row under the pointer, never an insertion proposal.
            let hitRow = tableView.row(at: tableView.convert(info.draggingLocation, from: nil))
            guard parent.rows.indices.contains(hitRow),
                  info.draggingPasteboard.pasteboardItems?.contains(where: { $0.string(forType: LinkedRowsTable.dragType) != nil }) == true else { return [] }
            tableView.setDropRow(hitRow, dropOperation: .on)
            return .copy
        }

        func tableView(_ tableView: NSTableView, acceptDrop info: NSDraggingInfo, row: Int, dropOperation: NSTableView.DropOperation) -> Bool {
            let hitRow = tableView.row(at: tableView.convert(info.draggingLocation, from: nil))
            guard parent.rows.indices.contains(hitRow) else { return false }
            let ids = (info.draggingPasteboard.pasteboardItems ?? []).compactMap { $0.string(forType: LinkedRowsTable.dragType) }
            guard !ids.isEmpty else { return false }
            parent.onDrop(hitRow, ids)
            return true
        }

        func menu(row: Int) -> NSMenu {
            menuRow = row
            let menu = NSMenu()
            for (title, action) in [
                ("Copy to All Elements in This String", #selector(copyToString)),
                ("Copy to All Elements/All Strings", #selector(copyToAll)),
                ("Reverse Element Linking in This String", #selector(reverse)),
            ] {
                let item = NSMenuItem(title: title, action: action, keyEquivalent: "")
                item.target = self
                menu.addItem(item)
            }
            return menu
        }

        @objc private func copyToString() {
            if menuRow >= 0 {
                parent.onCopyToString(menuRow)
            }
        }

        @objc private func copyToAll() {
            if menuRow >= 0 {
                parent.onCopyToAll(menuRow)
            }
        }

        @objc private func reverse() {
            parent.onReverse()
        }
    }
}
