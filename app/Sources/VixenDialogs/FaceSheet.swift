import AppKit
import SwiftUI
import VixenBridge
import VixenKit

/// Face Setup (F184 to F192).
struct FaceSheet: View {
    let context: DialogContext
    let onFinish: (Bool) -> Void
    @State private var rows: [DialogFaceRow]
    @State private var choosingColor: ColorChoice?
    @State private var tab = 0
    @State private var studio = StudioLiveSheet(step: "Face Setup") // STUDIO: live sheet, Undo while it's up (B9)

    struct ColorChoice: Identifiable {
        let id = UUID()
        let rows: [Int]
        let current: UInt32
        let allowed: [UInt32]
    }

    init(context: DialogContext, nodes: [String], onFinish: @escaping (Bool) -> Void) {
        self.context = context
        self.onFinish = onFinish
        _rows = State(initialValue: context.faceRows(nodes: nodes))
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            DialogTitle("Face Setup")
            SheetTabs(selection: $tab, tabs: [("Mouth", 0), ("Outlines/Eyes", 1)])
            if tab == 0 {
                FaceTable(
                    columns: context.facePhonemes().map { FaceTable.Column(key: $0, title: $0, image: Self.phonemeImage($0)) },
                    values: \.phonemes,
                    rows: $rows,
                    context: context,
                    chooseColor: chooseColor
                )
            } else {
                FaceTable(
                    columns: context.faceComponents().map { FaceTable.Column(key: $0.key, title: $0.title, image: nil) },
                    values: \.components,
                    rows: $rows,
                    context: context,
                    chooseColor: chooseColor
                )
            }
            StudioDoneButton(studio) { onFinish($0) } // STUDIO: live, closes with Done (B9)
        }
        .padding(20)
        .frame(minWidth: 864, idealWidth: 864, minHeight: 394, idealHeight: 420)
        .studioLive(studio, draft: $rows) { context.applyFace(rows: $0) } // STUDIO: live sheet, Undo while it's up (B9)
        .sheet(item: $choosingColor) { choice in
            if choice.allowed.isEmpty {
                FreeColorSheet(current: choice.current) { color in finish(choice, color) }
            } else {
                DiscreteColorSheet(colors: choice.allowed, current: choice.current) { color in finish(choice, color) }
            }
        }
    }

    private func finish(_ choice: ColorChoice, _ color: UInt32?) {
        choosingColor = nil
        guard let color else { return }
        for row in choice.rows {
            rows[row].color = color
        }
    }

    /// F188: the choice goes by what the selected rows' elements allow.
    private func chooseColor(_ selected: [Int]) {
        guard let first = selected.first else { return }
        let allowed = context.faceColorChoices(nodes: selected.map { rows[$0].node })
        choosingColor = ColorChoice(rows: selected, current: rows[first].color, allowed: allowed)
    }

    private static func phonemeImage(_ phoneme: String) -> NSImage? {
        VixenResources.image("dialogs/phonemes/\(phoneme).png")
    }
}

/// F185, F186: `NSTableView` with selection by cell: click, ⌘-click, and ⇧-click for a rectangle. Clicking a selected checkbox sets every selected
/// checkbox to the same value, by F187's rule in Rust.
struct FaceTable: NSViewRepresentable {
    struct Column {
        let key: String
        let title: String
        let image: NSImage?
    }

    let columns: [Column]
    let values: WritableKeyPath<DialogFaceRow, [Bool]>
    @Binding var rows: [DialogFaceRow]
    let context: DialogContext
    let chooseColor: ([Int]) -> Void

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeNSView(context ctx: Context) -> NSScrollView {
        let table = FaceNSTable()
        table.selectionHighlightStyle = .none
        table.allowsColumnReordering = false
        table.allowsColumnSelection = false
        table.rowSizeStyle = .small
        table.style = .fullWidth
        table.gridStyleMask = [.solidVerticalGridLineMask, .solidHorizontalGridLineMask]
        table.columnAutoresizingStyle = .noColumnAutoresizing
        table.intercellSpacing = NSSize(width: 0, height: table.intercellSpacing.height)
        let nameColumn = NSTableColumn(identifier: .init("name"))
        nameColumn.title = " "
        // F185: the name column sizes to its content.
        let font = NSFont.systemFont(ofSize: NSFont.systemFontSize)
        let longest = rows.map { ($0.name as NSString).size(withAttributes: [.font: font]).width }.max() ?? 0
        nameColumn.width = max(120, ceil(longest) + 24)
        table.addTableColumn(nameColumn)
        for (index, column) in columns.enumerated() {
            let c = NSTableColumn(identifier: .init("v\(index)"))
            c.headerCell = FaceHeaderCell(title: column.title, image: column.image)
            c.width = column.image == nil ? 90 : 50
            c.sortDescriptorPrototype = NSSortDescriptor(key: "v\(index)", ascending: true)
            table.addTableColumn(c)
        }
        let colorColumn = NSTableColumn(identifier: .init("color"))
        colorColumn.headerCell = FaceHeaderCell(title: "Color", image: nil)
        colorColumn.width = 90
        table.addTableColumn(colorColumn)
        nameColumn.sortDescriptorPrototype = NSSortDescriptor(key: "name", ascending: true)
        table.headerView = NSTableHeaderView(frame: NSRect(x: 0, y: 0, width: 800, height: 75))
        table.dataSource = ctx.coordinator
        table.delegate = ctx.coordinator
        table.onClick = { [weak coordinator = ctx.coordinator] row, column, flags, clicks in
            coordinator?.clicked(row: row, column: column, flags: flags, clicks: clicks)
        }
        let scroll = NSScrollView()
        scroll.documentView = table
        scroll.hasVerticalScroller = true
        scroll.hasHorizontalScroller = true
        scroll.autohidesScrollers = true
        scroll.borderType = .bezelBorder
        ctx.coordinator.table = table
        return scroll
    }

    func updateNSView(_ scroll: NSScrollView, context ctx: Context) {
        let c = ctx.coordinator
        c.parent = self
        c.table?.reloadData()
    }

    struct Cell: Hashable {
        let row: Int
        let column: Int
    }

    @MainActor
    final class Coordinator: NSObject, NSTableViewDataSource, NSTableViewDelegate {
        var parent: FaceTable
        weak var table: FaceNSTable?
        private var selected = Set<Cell>()
        private var anchor: Cell?

        init(_ parent: FaceTable) {
            self.parent = parent
        }

        /// The column indexes: 0 is the name, then the checkboxes, then the colour.
        private var colorColumn: Int { parent.columns.count + 1 }

        func numberOfRows(in tableView: NSTableView) -> Int {
            parent.rows.count
        }

        func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
            guard let tableColumn, row < parent.rows.count else { return nil }
            let column = tableView.tableColumns.firstIndex(of: tableColumn) ?? 0
            let data = parent.rows[row]
            let cell = FaceCellView()
            cell.isSelected = selected.contains(Cell(row: row, column: column))
            if column == 0 {
                cell.configure(title: data.name)
            } else if column == colorColumn {
                cell.configure(color: data.color)
            } else {
                cell.configure(checked: data[keyPath: parent.values][column - 1])
            }
            return cell
        }

        func clicked(row: Int, column: Int, flags: NSEvent.ModifierFlags, clicks: Int) {
            guard row >= 0, column > 0, row < parent.rows.count else { return }
            let cell = Cell(row: row, column: column)
            if flags.contains(.command) {
                if selected.contains(cell) {
                    selected.remove(cell)
                } else {
                    selected.insert(cell)
                    anchor = cell
                }
            } else if flags.contains(.shift), let anchor {
                selected = Set(rectangle(anchor, cell))
            } else {
                if !selected.contains(cell) {
                    selected = [cell]
                    anchor = cell
                }
                if column == colorColumn {
                    if clicks >= 2 {
                        parent.chooseColor(selected.filter { $0.column == colorColumn }.map(\.row).sorted())
                    }
                } else {
                    toggleSelected()
                }
            }
            table?.reloadData()
        }

        private func rectangle(_ a: Cell, _ b: Cell) -> [Cell] {
            var cells: [Cell] = []
            for row in min(a.row, b.row)...max(a.row, b.row) {
                for column in min(a.column, b.column)...max(a.column, b.column) where column > 0 {
                    cells.append(Cell(row: row, column: column))
                }
            }
            return cells
        }

        /// F187.
        private func toggleSelected() {
            let boxes = selected.filter { $0.column > 0 && $0.column < colorColumn }
            let current = boxes.map { parent.rows[$0.row][keyPath: parent.values][$0.column - 1] }
            let value = parent.context.faceToggle(cells: current)
            var rows = parent.rows
            for box in boxes {
                rows[box.row][keyPath: parent.values][box.column - 1] = value
            }
            parent.rows = rows
        }

        /// F192: any column but Color sorts, as a grid does when its header is clicked.
        func tableView(_ tableView: NSTableView, sortDescriptorsDidChange oldDescriptors: [NSSortDescriptor]) {
            guard let descriptor = tableView.sortDescriptors.first, let key = descriptor.key else { return }
            let ascending = descriptor.ascending
            let values = parent.values
            var rows = parent.rows
            if key == "name" {
                rows.sort { ascending ? $0.name.localizedStandardCompare($1.name) == .orderedAscending : $1.name.localizedStandardCompare($0.name) == .orderedAscending }
            } else if let index = Int(key.dropFirst()) {
                let enumerated = rows.enumerated().sorted { a, b in
                    let (x, y) = (a.element[keyPath: values][index], b.element[keyPath: values][index])
                    return x == y ? a.offset < b.offset : (ascending ? !x : x)
                }
                rows = enumerated.map(\.element)
            }
            selected = []
            anchor = nil
            parent.rows = rows
        }
    }
}

final class FaceNSTable: NSTableView {
    var onClick: ((Int, Int, NSEvent.ModifierFlags, Int) -> Void)?

    override func mouseDown(with event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        onClick?(row(at: point), column(at: point), event.modifierFlags, event.clickCount)
    }
}

/// F185: the phoneme's picture above its name.
final class FaceHeaderCell: NSTableHeaderCell {
    private let picture: NSImage?

    init(title: String, image: NSImage?) {
        picture = image
        super.init(textCell: title)
    }

    required init(coder: NSCoder) {
        picture = nil
        super.init(coder: coder)
    }

    override func draw(withFrame cellFrame: NSRect, in controlView: NSView) {
        let title = stringValue
        stringValue = ""
        super.draw(withFrame: cellFrame, in: controlView)
        stringValue = title
        if let picture {
            let side = min(48, cellFrame.width - 2)
            let rect = NSRect(x: cellFrame.midX - side / 2, y: cellFrame.minY + 3, width: side, height: side)
            picture.draw(in: rect, from: .zero, operation: .sourceOver, fraction: 1, respectFlipped: true, hints: nil)
        }
        let attributes: [NSAttributedString.Key: Any] = [
            .font: NSFont.systemFont(ofSize: NSFont.smallSystemFontSize),
            .foregroundColor: NSColor.headerTextColor,
        ]
        let text = NSAttributedString(string: title, attributes: attributes)
        let size = text.size()
        text.draw(at: NSPoint(x: cellFrame.midX - size.width / 2, y: cellFrame.maxY - size.height - 4))
    }
}

/// A cell that takes no clicks of its own, so the table can apply its rules.
final class FaceCellView: NSView {
    var isSelected = false
    private var title: String?
    private var color: UInt32?
    private var checked: Bool?

    func configure(title: String) { self.title = title }
    func configure(color: UInt32) { self.color = color }
    func configure(checked: Bool) { self.checked = checked }

    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    override func draw(_ dirtyRect: NSRect) {
        if isSelected {
            NSColor.selectedContentBackgroundColor.withAlphaComponent(0.55).setFill()
            bounds.fill()
        }
        if let color {
            NSColor(argb: color).setFill()
            bounds.insetBy(dx: isSelected ? 3 : 1, dy: isSelected ? 3 : 1).fill()
        }
        if let title {
            let attributes: [NSAttributedString.Key: Any] = [.font: NSFont.systemFont(ofSize: NSFont.smallSystemFontSize + 1), .foregroundColor: NSColor.labelColor]
            NSAttributedString(string: title, attributes: attributes).draw(with: bounds.insetBy(dx: 4, dy: 1), options: [.usesLineFragmentOrigin, .truncatesLastVisibleLine])
        }
        if let checked {
            let cell = NSButtonCell()
            cell.setButtonType(.switch)
            cell.title = ""
            cell.state = checked ? .on : .off
            let side: CGFloat = 16
            cell.draw(withFrame: NSRect(x: bounds.midX - side / 2, y: bounds.midY - side / 2, width: side, height: side), in: self)
        }
    }
}
