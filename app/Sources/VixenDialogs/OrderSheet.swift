import AppKit
import SwiftUI
import VixenBridge
import VixenKit

/// Patching Order Preference (F170 to F177).
struct OrderHelperSheet: View {
    let session: OrderSession
    let onFinish: (Bool) -> Void
    @State private var names: [String]
    @State private var selection = IndexSet()
    @State private var revision = 0
    @State private var zigzagging = false
    @State private var alert: DialogAlert?
    @State private var studio = StudioLiveSheet(step: "Patching Order") // STUDIO: live sheet, Undo while it's up (B9)

    init(session: OrderSession, onFinish: @escaping (Bool) -> Void) {
        self.session = session
        self.onFinish = onFinish
        _names = State(initialValue: session.names())
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            DialogTitle("Patching Order Preference")
            OrderTable(
                names: names,
                revision: revision,
                selection: $selection,
                move: { selected, to in
                    session.moveRows(selected: selected.map { UInt32($0) }, to: UInt32(to))
                    changed()
                },
                menu: { (reverse: Bool) in
                    if reverse {
                        session.reverse(selected: selection.map { UInt32($0) })
                        changed()
                    } else {
                        zigzagging = true
                    }
                }
            )
            .frame(minHeight: 300)
            StudioDoneButton(studio) { onFinish($0) } // STUDIO: live, closes with Done (B9)
        }
        .padding(20)
        .frame(minWidth: 462, idealWidth: 462, minHeight: 519, idealHeight: 519)
        .onChange(of: revision) { session.apply(); studio.appliedNow() } // STUDIO: each move applies at once; the visit is one step on Display Setup's Undo (B9)
        .sheet(isPresented: $zigzagging) {
            let bounds = session.zigzagBounds(selectedCount: UInt32(selection.count))
            NumberPromptSheet(
                title: "ZigZag Length",
                prompt: "How many pixels to ZigZag?",
                value: bounds.defaultValue,
                range: bounds.minimum...max(bounds.minimum, bounds.maximum),
                verb: "ZigZag" // STUDIO: a verb (B9)
            ) { length in
                zigzagging = false
                guard let length else { return }
                if let error = session.zigzag(selected: selection.map { UInt32($0) }, every: length) {
                    alert = .notice("Zigzag Error", error)
                } else {
                    changed()
                }
            }
        }
        .dialogAlert($alert)
    }

    private func changed() {
        names = session.names()
        revision += 1
    }
}

/// F171 to F174: `NSTableView`, with thousands of rows the normal case. Dragging several selected rows together is a harmless extra.
struct OrderTable: NSViewRepresentable {
    let names: [String]
    let revision: Int
    @Binding var selection: IndexSet
    let move: ([Int], Int) -> Void
    let menu: (Bool) -> Void

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeNSView(context: Context) -> NSScrollView {
        let table = MenuTable()
        table.allowsMultipleSelection = true
        table.usesAlternatingRowBackgroundColors = true
        table.style = .fullWidth
        table.rowSizeStyle = .small
        for (id, title, width) in [("order", "Order", 60.0), ("name", "Name", 340.0)] {
            let column = NSTableColumn(identifier: .init(id))
            column.title = title
            column.width = width
            table.addTableColumn(column)
        }
        table.columnAutoresizingStyle = .lastColumnOnlyAutoresizingStyle
        table.dataSource = context.coordinator
        table.delegate = context.coordinator
        table.registerForDraggedTypes([Coordinator.pasteboardType])
        table.draggingDestinationFeedbackStyle = .gap
        table.menuProvider = { [weak coordinator = context.coordinator] in coordinator?.contextMenu() }
        let scroll = NSScrollView()
        scroll.documentView = table
        scroll.hasVerticalScroller = true
        scroll.autohidesScrollers = true
        scroll.borderType = .bezelBorder
        context.coordinator.table = table
        return scroll
    }

    func updateNSView(_ scroll: NSScrollView, context: Context) {
        let c = context.coordinator
        c.parent = self
        guard let table = c.table else { return }
        if c.lastRevision != revision {
            c.lastRevision = revision
            table.reloadData()
        }
        if table.selectedRowIndexes != selection {
            table.selectRowIndexes(selection, byExtendingSelection: false)
        }
    }

    @MainActor
    final class Coordinator: NSObject, NSTableViewDataSource, NSTableViewDelegate {
        static let pasteboardType = NSPasteboard.PasteboardType("com.vixenlights.order-row")

        var parent: OrderTable
        weak var table: MenuTable?
        var lastRevision = 0

        init(_ parent: OrderTable) {
            self.parent = parent
        }

        func numberOfRows(in tableView: NSTableView) -> Int {
            parent.names.count
        }

        func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
            let id = tableColumn?.identifier.rawValue
            let reuse = NSUserInterfaceItemIdentifier(id ?? "cell")
            let field = (tableView.makeView(withIdentifier: reuse, owner: nil) as? NSTextField) ?? {
                let f = NSTextField(labelWithString: "")
                f.identifier = reuse
                f.lineBreakMode = .byTruncatingTail
                return f
            }()
            field.stringValue = id == "order" ? String(row + 1) : parent.names[row]
            field.font = id == "order" ? .monospacedDigitSystemFont(ofSize: NSFont.smallSystemFontSize + 1, weight: .regular) : .systemFont(ofSize: NSFont.smallSystemFontSize + 1)
            return field
        }

        func tableViewSelectionDidChange(_ notification: Notification) {
            guard let table else { return }
            if parent.selection != table.selectedRowIndexes {
                parent.selection = table.selectedRowIndexes
            }
        }

        func tableView(_ tableView: NSTableView, pasteboardWriterForRow row: Int) -> NSPasteboardWriting? {
            let item = NSPasteboardItem()
            item.setString(String(row), forType: Self.pasteboardType)
            return item
        }

        func tableView(_ tableView: NSTableView, validateDrop info: NSDraggingInfo, proposedRow row: Int, proposedDropOperation dropOperation: NSTableView.DropOperation) -> NSDragOperation {
            if dropOperation == .on {
                tableView.setDropRow(row, dropOperation: .above)
            }
            return .move
        }

        func tableView(_ tableView: NSTableView, acceptDrop info: NSDraggingInfo, row: Int, dropOperation: NSTableView.DropOperation) -> Bool {
            let dragged = (info.draggingPasteboard.pasteboardItems ?? []).compactMap { $0.string(forType: Self.pasteboardType).flatMap(Int.init) }
            guard !dragged.isEmpty else { return false }
            parent.move(dragged, row)
            return true
        }

        /// F174: with one row selected, upstream shows an empty menu. Here there's no menu.
        func contextMenu() -> NSMenu? {
            guard parent.selection.count >= 2 else { return nil }
            let menu = NSMenu()
            let reverse = NSMenuItem(title: "Reverse", action: #selector(reverse(_:)), keyEquivalent: "")
            reverse.target = self
            menu.addItem(reverse)
            let zigzag = NSMenuItem(title: "Zig Zag...", action: #selector(zigzag(_:)), keyEquivalent: "")
            zigzag.target = self
            menu.addItem(zigzag)
            return menu
        }

        @objc func reverse(_ sender: Any?) {
            parent.menu(true)
        }

        @objc func zigzag(_ sender: Any?) {
            parent.menu(false)
        }
    }
}

final class MenuTable: NSTableView {
    var menuProvider: (() -> NSMenu?)?

    override func menu(for event: NSEvent) -> NSMenu? {
        menuProvider?()
    }
}
