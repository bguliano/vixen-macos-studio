import AppKit
import SwiftUI
import VixenBridge
import VixenKit

/// Streaming ACN (E1.31) Configuration (F51 to F88).
struct E131Sheet: View {
    let context: DialogContext
    let session: E131Session
    let onFinish: (Bool) -> Void
    @State private var rows: [DialogUniverseRow]
    @State private var options: DialogE131Options
    @State private var destinations: DialogDestinationList
    @State private var selectedRows = IndexSet()
    @State private var reload = 0
    @State private var scrollToEnd = 0
    @State private var alert: DialogAlert?
    @State private var addingUnicast = false
    @State private var addingMany = false
    @State private var pending: [DialogE131Step] = []
    @State private var tab = 0
    @State private var grid = UniverseGridHandle()
    @FocusState private var focus: AdvancedField?

    private enum AdvancedField: Hashable {
        case repeatCount, suppressCount
    }

    init(context: DialogContext, session: E131Session, onFinish: @escaping (Bool) -> Void) {
        self.context = context
        self.session = session
        self.onFinish = onFinish
        _rows = State(initialValue: session.rows())
        _options = State(initialValue: session.options())
        _destinations = State(initialValue: session.destinations())
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            DialogTitle(session.title())
            SheetTabs(selection: $tab, tabs: [("Universes", 0), ("Advanced Options", 1)])
            Group {
                if tab == 0 {
                    universes
                } else {
                    advanced
                }
            }
            .frame(minHeight: 420, alignment: .top)
            StudioDoneButton(live, ready: studioReady) { finish($0) } // STUDIO: live, closes with Done (B9)
        }
        .padding(20)
        .frame(width: 560)
        .sheet(isPresented: $addingUnicast) {
            UnicastSheet(context: context) { text in
                addingUnicast = false
                guard let text else { return }
                if let error = session.addUnicast(text: text) {
                    alert = .notice("IP Address Validation", error)
                }
                destinations = session.destinations()
            }
        }
        .sheet(isPresented: $addingMany) {
            NumberPromptSheet(title: "Add Universes", prompt: "Number of universes to add.", value: 1, range: 1...max(1, session.maxUniversesToAdd()), verb: "Add") { count in // STUDIO: a verb (B9)
                addingMany = false
                if let count {
                    session.addUniverses(count: count)
                    rowsChanged(selectLast: true)
                }
            }
        }
        .dialogAlert($alert)
        .onChange(of: options) { session.setOptions(options: options); rows = session.rows(); reload += 1 }
        // STUDIO-BEGIN e131-live-hooks: every edit reaches the grid's reload or the destination list; each applies (B9)
        .onChange(of: reload) { studioApply() }
        .onChange(of: destinations) { studioApply() }
        // STUDIO-END e131-live-hooks
        .onAppear {
            if let missing = destinations.missingInterfaceAlert {
                alert = .notice("", missing)
            }
        }
    }

    private var universes: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .top, spacing: 6) {
                UniverseTable(
                    handle: grid,
                    rows: rows,
                    manual: options.manual,
                    revision: reload,
                    scrollToEnd: scrollToEnd,
                    selection: $selectedRows,
                    setCell: { row, column, text in session.setCell(row: row, column: column, text: text) },
                    setActive: { row, on in session.setActive(row: row, active: on) },
                    showError: { message in alert = .notice("Cell Validation", message) },
                    edited: { rows = session.rows(); reload += 1 },
                    sort: { column, ascending in
                        if let warning = session.sort(column: column, ascending: ascending) {
                            alert = .notice("", warning)
                        }
                        rows = session.rows()
                        reload += 1
                    },
                    deleteKey: deleteSelected
                )
                VStack(spacing: 6) {
                    stripButton("Add Universe", "plus") {
                        if let error = session.addUniverse() {
                            alert = .notice("Add Universes", error)
                        } else {
                            rowsChanged(selectLast: true)
                        }
                    }
                    stripButton("Add Multiple Universes", "plus.rectangle.on.rectangle", action: addMany)
                    stripButton("Delete Universe", "minus", action: deleteSelected)
                        .disabled(rows.count <= 1 || selectedRows.isEmpty)
                    stripButton("Delete All Universes", "trash", action: deleteAll)
                }
            }
            HStack {
                Text("Destination").font(.title3)
                Spacer()
                Button("Add Unicast Destination", systemImage: "plus") {
                    if grid.endEditing() {
                        addingUnicast = true
                    }
                }
                    .labelStyle(.iconOnly)
                    .help("Add Unicast Destination")
                Button("Remove Unicast Destination", systemImage: "minus", action: removeUnicast)
                    .labelStyle(.iconOnly)
                    .help("Remove Unicast Destination")
                    .disabled(selectedDestinationIsMulticast)
            }
            Picker("Destination", selection: Binding(get: { destinations.selected }, set: { index in
                destinations.selected = index
                session.selectDestination(index: index)
            })) {
                if destinations.selected == nil {
                    Text("").tag(UInt32?.none)
                }
                ForEach(Array(destinations.entries.enumerated()), id: \.offset) { index, entry in
                    Text(entry.text).tag(UInt32?.some(UInt32(index)))
                }
            }
            .labelsHidden()
        }
    }

    private var selectedDestinationIsMulticast: Bool {
        guard let index = destinations.selected, Int(index) < destinations.entries.count else { return true }
        return destinations.entries[Int(index)].isMulticast
    }

    private func stripButton(_ title: String, _ symbol: String, action: @escaping () -> Void) -> some View {
        Button(title, systemImage: symbol, action: action)
            .labelStyle(.iconOnly)
            .help(title)
            .accessibilityLabel(title)
    }

    private var advanced: some View {
        VStack(alignment: .leading, spacing: 12) {
            Toggle("Manually manage start values.", isOn: $options.manual).toggleStyle(.checkbox)
            Toggle("Display ALL Warnings/Errors and wait For OK", isOn: $options.warnings).toggleStyle(.checkbox)
            Toggle("Gather statistics and display at end of session", isOn: $options.statistics).toggleStyle(.checkbox)
            Toggle("Transmit blind data.", isOn: $options.blind).toggleStyle(.checkbox)
            HStack {
                NumberField(value: $options.priority, range: 0...200, width: 50)
                Text("sACN priority.")
            }
            countRow(
                $options.repeatText,
                field: .repeatCount,
                isValid: { context.e131RepeatIsValid(text: $0) },
                label: "Max Repeat Count\n\nSet to 0 to send all events (even 0s)\nto each universe, Set to >0 to stop\nrepeating frames after N duplicates\nare sent."
            )
            countRow(
                $options.suppressText,
                field: .suppressCount,
                isValid: { context.e131SuppressIsValid(text: $0) },
                label: "Max Suppress Count\n\nOnly used if Max Repeat Count\nis not  0. Set to >0 to allow every\nNth duplicate frame in a universe\nto go out."
            )
            Spacer()
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    /// F79, F80: two characters, and a bad value beeps and keeps the focus, with no message.
    private func countRow(_ text: Binding<String>, field: AdvancedField, isValid: @escaping (String) -> Bool, label: String) -> some View {
        HStack(alignment: .top) {
            TextField("", text: Binding(get: { text.wrappedValue }, set: { text.wrappedValue = String($0.prefix(2)) }))
                .labelsHidden()
                .multilineTextAlignment(.center)
                .monospacedDigit()
                .frame(width: 34)
                .focused($focus, equals: field)
                .onChange(of: focus) { old, new in
                    if old == field, new != field, !isValid(text.wrappedValue) {
                        NSSound.beep()
                        focus = field
                    }
                }
            Text(label).font(.callout).fixedSize(horizontal: false, vertical: true)
        }
    }

    private func rowsChanged(selectLast: Bool) {
        rows = session.rows()
        reload += 1
        if selectLast, !rows.isEmpty {
            selectedRows = IndexSet(integer: rows.count - 1)
            scrollToEnd += 1
        }
    }

    private func addMany() {
        if session.maxUniversesToAdd() < 1 {
            alert = .notice("Add Universes", "No additional universes can be added. Universe numbers must be between 1 and 64000 inclusive.")
        } else {
            addingMany = true
        }
    }

    private func deleteSelected() {
        guard !selectedRows.isEmpty else { return }
        session.deleteRows(rows: selectedRows.map { UInt32($0) })
        selectedRows = []
        rowsChanged(selectLast: false)
    }

    private func deleteAll() {
        alert = DialogAlert(
            title: "Delete All Universes?",
            message: "Are you sure you want to delete all universes?",
            choices: [
                .init("Delete All", role: .destructive) {
                    session.deleteAllRows()
                    selectedRows = []
                    rowsChanged(selectLast: false)
                },
                .init("Cancel", role: .cancel),
            ]
        )
    }

    private func removeUnicast() {
        if let error = session.removeSelectedUnicast() {
            alert = .notice("Streaming ACN (E1.31)", error)
        }
        destinations = session.destinations()
    }

    /// F87: one alert at a time, each able to stop the rest. F66: an open editor commits first, and a bad value stops here with its own alert. F79: a
    /// bad count beeps and goes back to its box.
    private func ok() {
        guard grid.endEditing() else { return }
        for (text, field, isValid) in [
            (options.repeatText, AdvancedField.repeatCount, context.e131RepeatIsValid(text:)),
            (options.suppressText, AdvancedField.suppressCount, context.e131SuppressIsValid(text:)),
        ] where !isValid(text) {
            NSSound.beep()
            tab = 1
            focus = field
            return
        }
        pending = session.okSteps()
        nextStep()
    }

    private func nextStep() {
        guard !pending.isEmpty else {
            session.apply()
            finish(true)
            return
        }
        switch pending.removeFirst() {
        case let .warn(title, text):
            alert = DialogAlert(title: title, message: text, choices: [
                .init("Save Anyway") { Task { nextStep() } },
                .init("Go Back", role: .cancel),
            ])
        case let .reviewOrContinue(text):
            alert = DialogAlert(title: "Warning", message: text, choices: [
                .init("Review Settings", role: .cancel),
                .init("Continue") { Task { nextStep() } },
            ])
        case let .block(title, text):
            alert = .notice(title, text)
        }
    }

    private func finish(_ saved: Bool) {
        session.close()
        onFinish(saved)
    }

    // STUDIO-BEGIN e131-live: applies each change as it's made instead of at OK (B9)
    // What upstream's OK checks still runs on every change: a step that blocks shows above Done and leaves the controller
    // as last applied; a warning applies and shows there too. The visit is one step on Display Setup's Undo; the sheet's
    // own working state has no per-edit Undo.
    @State private var live = StudioLiveSheet(step: "Configure Controller")

    private func studioApply() {
        guard context.e131RepeatIsValid(text: options.repeatText), context.e131SuppressIsValid(text: options.suppressText) else {
            live.error = "Fix the count on Advanced Options; the last valid settings stay applied."
            return
        }
        let steps = session.okSteps()
        for step in steps {
            if case let .block(_, text) = step {
                live.error = text
                return
            }
        }
        session.apply()
        live.appliedNow()
        for step in steps {
            switch step {
            case let .warn(_, text), let .reviewOrContinue(text): live.error = text
            case .block: break
            }
        }
    }

    /// Done ends a cell's edit first; an invalid value keeps the sheet up with its alert, as OK did.
    private func studioReady() -> Bool {
        guard grid.endEditing() else { return false }
        studioApply()
        return true
    }
    // STUDIO-END e131-live
}

/// New Unicast Destination (F56 to F59).
struct UnicastSheet: View {
    let context: DialogContext
    let onFinish: (String?) -> Void
    @State private var usesIP = true
    @State private var ip = "127.0.0.1"
    @State private var name = "myComputer"

    var body: some View {
        let text = usesIP ? ip : name
        VStack(alignment: .leading, spacing: 12) {
            DialogTitle("New Unicast Destination")
            if usesIP {
                TextField("", text: $ip).labelsHidden().monospacedDigit()
            } else {
                TextField("", text: $name).labelsHidden()
            }
            Picker("", selection: $usesIP) {
                Text("IP Address").tag(true)
                Text("Network Name").tag(false)
            }
            .pickerStyle(.radioGroup)
            .labelsHidden()
            StudioVerbButtons(verb: "Add", disabled: !context.unicastTextIsValid(text: text, isIp: usesIP), onConfirm: { onFinish(text) }, onCancel: { onFinish(nil) }) // STUDIO: a verb (B9)
        }
        .padding(20)
        .frame(width: 300)
    }
}

/// Lets the sheet end a cell's edit before acting on the grid's data.
@MainActor
final class UniverseGridHandle {
    weak var table: NSTableView?

    /// False if the cell's value is invalid. Its alert is up by then, and the edit stays open.
    func endEditing() -> Bool {
        guard let table, let window = table.window else { return true }
        return window.makeFirstResponder(nil)
    }
}

/// F61: the universe grid. AppKit, because Add Multiple can make 63,999 rows. Rows are selected, where upstream selects cells.
struct UniverseTable: NSViewRepresentable {
    let handle: UniverseGridHandle
    let rows: [DialogUniverseRow]
    let manual: Bool
    let revision: Int
    let scrollToEnd: Int
    @Binding var selection: IndexSet
    let setCell: (UInt32, DialogUniverseColumn, String) -> String?
    let setActive: (UInt32, Bool) -> Void
    let showError: (String) -> Void
    let edited: () -> Void
    let sort: (DialogUniverseColumn, Bool) -> Void
    let deleteKey: () -> Void

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeNSView(context: Context) -> NSScrollView {
        let table = KeyTable()
        table.usesAlternatingRowBackgroundColors = true
        table.allowsMultipleSelection = true
        table.allowsColumnResizing = false
        table.allowsColumnReordering = false
        table.rowSizeStyle = .small
        table.style = .fullWidth
        table.gridStyleMask = [.solidVerticalGridLineMask, .solidHorizontalGridLineMask]
        let columns: [(String, String, CGFloat)] = [("#", "", 44), ("start", "Start", 80), ("on", "On", 36), ("universe", "Universe", 90), ("size", "Size", 70)]
        for (id, title, width) in columns {
            let column = NSTableColumn(identifier: .init(id))
            column.title = title
            column.width = width
            column.headerCell.alignment = id == "on" ? .center : .right
            table.addTableColumn(column)
        }
        let header = KeyTableHeader()
        header.onRightClick = { [weak coordinator = context.coordinator] column in coordinator?.headerClicked(column, button: .right) }
        table.headerView = header
        table.dataSource = context.coordinator
        table.delegate = context.coordinator
        table.onDelete = { [weak coordinator = context.coordinator] in coordinator?.parent.deleteKey() }
        let scroll = NSScrollView()
        scroll.documentView = table
        scroll.hasVerticalScroller = true
        scroll.autohidesScrollers = true
        scroll.borderType = .bezelBorder
        context.coordinator.table = table
        handle.table = table
        return scroll
    }

    func updateNSView(_ scroll: NSScrollView, context: Context) {
        let c = context.coordinator
        c.parent = self
        guard let table = c.table else { return }
        for column in table.tableColumns {
            column.headerToolTip = manual ? "Sort (LeftClick = Ascending, RightClick = Descending)" : ""
        }
        if c.lastRevision != revision {
            c.lastRevision = revision
            table.reloadData()
        }
        if table.selectedRowIndexes != selection {
            table.selectRowIndexes(selection, byExtendingSelection: false)
        }
        if c.lastScroll != scrollToEnd {
            c.lastScroll = scrollToEnd
            if !rows.isEmpty {
                table.scrollRowToVisible(rows.count - 1)
            }
        }
    }

    @MainActor
    final class Coordinator: NSObject, NSTableViewDataSource, NSTableViewDelegate, NSTextFieldDelegate {
        var parent: UniverseTable
        weak var table: KeyTable?
        var lastRevision = 0
        var lastScroll = 0
        private var sortColumn: String?
        private var ascending = true

        init(_ parent: UniverseTable) {
            self.parent = parent
        }

        private func kind(_ id: String) -> DialogUniverseColumn? {
            switch id {
            case "start": .start
            case "on": .active
            case "universe": .universe
            case "size": .size
            default: nil
            }
        }

        func numberOfRows(in tableView: NSTableView) -> Int {
            parent.rows.count
        }

        func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
            guard let id = tableColumn?.identifier.rawValue, row < parent.rows.count else { return nil }
            let data = parent.rows[row]
            if id == "on" {
                let button = NSButton(checkboxWithTitle: "", target: self, action: #selector(toggled(_:)))
                button.state = data.active ? .on : .off
                button.tag = row
                return button
            }
            let field = NSTextField(labelWithString: "")
            field.alignment = .right
            field.font = .monospacedDigitSystemFont(ofSize: NSFont.smallSystemFontSize + 1, weight: .regular)
            switch id {
            case "#":
                field.stringValue = String(row + 1)
                field.textColor = .secondaryLabelColor
            case "start":
                field.stringValue = String(data.start)
                field.isEditable = parent.manual
                if !parent.manual {
                    // F62: read-only starts are shaded and italic.
                    field.font = NSFontManager.shared.convert(field.font!, toHaveTrait: .italicFontMask)
                    field.textColor = .secondaryLabelColor
                }
            case "universe":
                field.stringValue = String(data.universe)
                field.isEditable = true
            default:
                field.stringValue = String(data.size)
                field.isEditable = true
            }
            if field.isEditable {
                field.delegate = self
                field.isBezeled = false
                field.drawsBackground = false
                field.formatter = DigitsFormatter(maxLength: id == "size" ? 3 : 5)
                field.identifier = NSUserInterfaceItemIdentifier(id)
                field.tag = row
            }
            return field
        }

        @objc func toggled(_ sender: NSButton) {
            parent.setActive(UInt32(sender.tag), sender.state == .on)
            parent.edited()
        }

        /// F66: a cell can't be left while it's invalid.
        func control(_ control: NSControl, textShouldEndEditing fieldEditor: NSText) -> Bool {
            guard let field = control as? NSTextField,
                  let id = field.identifier?.rawValue,
                  let column = kind(id) else { return true }
            if let error = parent.setCell(UInt32(field.tag), column, fieldEditor.string) {
                parent.showError(error)
                return false
            }
            return true
        }

        func controlTextDidEndEditing(_ notification: Notification) {
            DispatchQueue.main.async { [weak self] in self?.parent.edited() }
        }

        func tableViewSelectionDidChange(_ notification: Notification) {
            guard let table else { return }
            if parent.selection != table.selectedRowIndexes {
                parent.selection = table.selectedRowIndexes
            }
        }

        func tableView(_ tableView: NSTableView, didClick tableColumn: NSTableColumn) {
            headerClicked(tableColumn, button: .left)
        }

        enum ClickButton { case left, right }

        func headerClicked(_ column: NSTableColumn, button: ClickButton) {
            guard let kind = kind(column.identifier.rawValue) else { return }
            // Left is ascending, and clicking the same header again toggles. Right is always descending.
            switch button {
            case .left:
                ascending = sortColumn == column.identifier.rawValue ? !ascending : true
            case .right:
                ascending = false
            }
            sortColumn = column.identifier.rawValue
            parent.sort(kind, ascending)
        }
    }
}

/// `NSTableView` that takes the Delete key.
final class KeyTable: NSTableView {
    var onDelete: (() -> Void)?

    override func keyDown(with event: NSEvent) {
        if event.keyCode == 51 || event.keyCode == 117 {
            onDelete?()
        } else {
            super.keyDown(with: event)
        }
    }
}

/// A header that also takes a right click.
final class KeyTableHeader: NSTableHeaderView {
    var onRightClick: ((NSTableColumn) -> Void)?

    override func rightMouseDown(with event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        let index = column(at: point)
        guard index >= 0, let table = tableView else { return }
        onRightClick?(table.tableColumns[index])
    }
}

/// F66: digits only, and anything else beeps.
nonisolated final class DigitsFormatter: Formatter {
    private let maxLength: Int

    init(maxLength: Int) {
        self.maxLength = maxLength
        super.init()
    }

    required init?(coder: NSCoder) {
        maxLength = 5
        super.init(coder: coder)
    }

    override func string(for obj: Any?) -> String? {
        (obj as? String) ?? (obj as? NSNumber)?.stringValue
    }

    override func getObjectValue(_ obj: AutoreleasingUnsafeMutablePointer<AnyObject?>?, for string: String, errorDescription error: AutoreleasingUnsafeMutablePointer<NSString?>?) -> Bool {
        obj?.pointee = string as NSString
        return true
    }

    override func isPartialStringValid(_ partial: String, newEditingString: AutoreleasingUnsafeMutablePointer<NSString?>?, errorDescription error: AutoreleasingUnsafeMutablePointer<NSString?>?) -> Bool {
        if partial.count <= maxLength, partial.allSatisfy(\.isNumber) {
            return true
        }
        NSSound.beep()
        return false
    }
}
