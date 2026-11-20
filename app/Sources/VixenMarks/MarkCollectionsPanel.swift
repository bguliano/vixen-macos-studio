import AppKit
import SwiftUI
import VixenBridge
import VixenKit

/// The Mark Collections panel (`docs/spec/marks-layers-analysis.md`, section A): a toolbar, a header row of column toggles, and one row
/// per collection. The sequencer window embeds it in its right-hand panels. Each time it's shown it makes a fresh model, so the header
/// toggles start from the rows, as upstream rebuilds its panel (F114).
public struct MarkCollectionsPanel: View {
    private let session: SequencerSession
    /// Made when the panel appears, not in `init`, which SwiftUI runs on every redraw of the parent.
    @State private var model: MarksPanelModel?

    public init(session: SequencerSession) {
        self.session = session
    }

    public var body: some View {
        Group {
            if let model {
                MarksPanelContent(model: model)
                    .task { await model.follow() }
            } else {
                Color.black
            }
        }
        .onAppear {
            if model == nil {
                model = MarksPanelModel(session: session)
            }
        }
    }
}

/// The column widths shared by the header and the rows.
enum MarksColumns {
    static let toggle: CGFloat = 22
    static let rowHeight: CGFloat = 22
}

/// Upstream's duplicate-name red, `#CC3B2E`.
let marksErrorColor = Color(red: 0xCC / 255, green: 0x3B / 255, blue: 0x2E / 255)

/// What the panel has open over it.
enum MarksSheet: Identifiable {
    case offset(MarkRowInfo)
    case importType
    case importSelection([MarkImportOption])
    case pangolinColor
    case export([MarkExportRow])

    var id: String {
        switch self {
        case let .offset(row): "offset-\(row.id)"
        case .importType: "import-type"
        case .importSelection: "import-selection"
        case .pangolinColor: "pangolin-color"
        case .export: "export"
        }
    }
}

/// An alert with its title, as upstream's message boxes have.
struct MarksAlert: Identifiable {
    let id = UUID()
    let title: String
    let message: String
    var isError = true
}

@Observable
final class MarksPanelModel {
    let panel: MarksPanel
    private(set) var rows: [MarkRowInfo] = []
    /// The row whose name is being edited (F15), its text and its error.
    var editingID: String?
    var editText = ""
    var renameError: String?
    var sheet: MarksSheet?
    var alert: MarksAlert?
    var askPangolin = false
    /// Colours shown while the system colour panel is open, before they're committed.
    var liveColors: [String: UInt32] = [:]
    @ObservationIgnored private var seenRevision: UInt64 = .max
    @ObservationIgnored private var oldName = ""
    @ObservationIgnored private var importKind: MarkImportKind = .vampBars
    @ObservationIgnored weak var window: NSWindow?

    init(session: SequencerSession) {
        panel = session.marksPanel()
        reload()
        MarksTestHooks.current = self
        if UITestHarness.isActive { MarksTestHooks.session = session }
        MarksTestHooks.install()
    }

    func reload() {
        seenRevision = panel.revision()
        let fresh = panel.rows()
        if fresh != rows {
            rows = fresh
        }
    }

    /// Follows changes made elsewhere: the timeline, undo, other windows. Reads one atomic a few times a second.
    func follow() async {
        while !Task.isCancelled {
            try? await Task.sleep(for: .milliseconds(250))
            if panel.revision() != seenRevision {
                reload()
            }
        }
    }

    // MARK: Toolbar and header

    func addCollection() {
        _ = panel.addCollection()
        reload()
    }

    func clickHeader(_ header: MarkHeaderKind) {
        _ = panel.clickHeader(header: header)
        reload()
    }

    // MARK: Rows

    func setActive(_ row: MarkRowInfo) {
        panel.setActive(id: row.id)
        reload()
    }

    func setFlag(_ row: MarkRowInfo, _ flag: MarkFlagKind, _ on: Bool) {
        panel.setFlag(id: row.id, flag: flag, on: on)
        reload()
    }

    func delete(_ row: MarkRowInfo) {
        if editingID == row.id {
            editingID = nil
        }
        panel.deleteCollection(id: row.id)
        reload()
    }

    func offset(_ row: MarkRowInfo, by ms: Int32) {
        panel.offsetTime(id: row.id, ms: ms)
        reload()
    }

    func setLevel(_ row: MarkRowInfo, _ level: Int32) {
        panel.setLevel(id: row.id, level: level)
        reload()
    }

    func setKind(_ row: MarkRowInfo, _ kind: MarkKind) {
        panel.setKind(id: row.id, kind: kind)
        reload()
    }

    func setLink(_ row: MarkRowInfo, _ parent: String?) {
        panel.setLink(id: row.id, parent: parent)
        reload()
    }

    func linkChoices(_ row: MarkRowInfo) -> [MarkLinkChoice]? {
        panel.linkChoices(id: row.id)
    }

    /// F21: the system colour panel, committing when it closes.
    func chooseColor(_ row: MarkRowInfo) {
        let id = row.id
        ColorPanelSession.shared.begin(title: "Color", argb: row.argb, onLive: { [weak self] argb in
            self?.liveColors[id] = argb
        }, onEnd: { [weak self] in
            self?.liveColors[id] = nil
        }, onFinish: { [weak self] argb in
            guard let self else { return }
            panel.setColor(id: id, argb: argb)
            reload()
        })
    }

    func color(of row: MarkRowInfo) -> Color {
        Color(argb: liveColors[row.id] ?? row.argb)
    }

    // MARK: Rename, F15

    func beginRename(_ row: MarkRowInfo) {
        if let editingID, editingID != row.id {
            endRename()
            if self.editingID != nil {
                return
            }
        }
        oldName = row.name
        editText = row.name
        renameError = nil
        editingID = row.id
    }

    func validateRename() {
        guard let editingID else { return }
        renameError = panel.renameError(id: editingID, text: editText)
    }

    /// Return or loss of focus. A duplicate stays open with its message.
    func endRename() {
        guard let editingID else { return }
        switch panel.endRename(id: editingID, text: editText) {
        case .stillInvalid:
            renameError = panel.renameError(id: editingID, text: editText)
        case .renamed, .unchanged, .restored:
            renameError = nil
            self.editingID = nil
            reload()
        }
    }

    func cancelRename() {
        editText = oldName
        renameError = nil
        editingID = nil
    }

    // MARK: Import, section B

    func beginImport() {
        sheet = .importType
    }

    /// F25's OK.
    func importTypeChosen(_ kind: MarkImportKind) async {
        sheet = nil
        importKind = kind
        if kind == .singingFaces {
            guard let present = SongInventory.present else {
                alert = MarksAlert(title: "Import", message: "The Song Inventory window requires the Scheduler area in this build.")
                return
            }
            try? await Task.sleep(for: .milliseconds(100))
            let result: (String?, String?) = await withCheckedContinuation { continuation in
                present(window) { xml, error in continuation.resume(returning: (xml, error)) }
            }
            if let xml = result.0 { handle(panel.importSongTiming(xml: xml)) }
            else if result.1 != nil { alert = MarksAlert(title: "Singing Faces Import Error", message: "There was an error importing the Singing Faces timing marks.") }
            else { panel.cancelImport() }
            return
        }
        if let message = markImportNotBuilt(kind: kind) {
            alert = MarksAlert(title: "Import", message: message, isError: false)
            return
        }
        guard let filter = markImportFilter(kind: kind) else { return }
        try? await Task.sleep(for: .milliseconds(50))
        guard let path = await MarkFiles.chooseOpen(filter: filter, directory: panel.lastFolder()) else {
            panel.cancelImport()
            return
        }
        handle(panel.importFile(kind: kind, path: path))
    }

    func handle(_ step: MarkImportStep) {
        switch step {
        case let .choose(options):
            if !options.isEmpty {
                sheet = .importSelection(options)
            }
        case .pangolinChoice:
            askPangolin = true
        case let .chooseAfterMessage(title, message, options):
            alert = MarksAlert(title: title, message: message, isError: false)
            pendingOptions = options
        case let .failed(title, message):
            alert = MarksAlert(title: title, message: message)
        case let .notBuilt(message):
            alert = MarksAlert(title: "Import", message: message, isError: false)
        }
    }

    /// Options waiting behind an information alert (F32 shows its summary before the selection).
    @ObservationIgnored var pendingOptions: [MarkImportOption]?

    func alertDismissed() {
        if let options = pendingOptions {
            pendingOptions = nil
            if !options.isEmpty {
                sheet = .importSelection(options)
            }
        }
    }

    /// F34's question.
    func pangolinAnswer(groupByColor: Bool?) {
        askPangolin = false
        switch groupByColor {
        case .some(true):
            handle(panel.importPangolin(groupByColor: true, argb: 0))
        case .some(false):
            sheet = .pangolinColor
        case .none:
            panel.cancelImport()
        }
    }

    func pangolinColorChosen(_ argb: UInt32?) {
        sheet = nil
        guard let argb else {
            panel.cancelImport()
            return
        }
        handle(panel.importPangolin(groupByColor: false, argb: argb))
    }

    /// F35's OK or Cancel.
    func importSelectionFinished(_ selected: [UInt32]?) {
        sheet = nil
        guard let selected else {
            panel.cancelImport()
            return
        }
        _ = panel.commitImport(selected: selected)
        reload()
    }

    // MARK: Export, section C

    func beginExport() {
        guard panel.canExport() else { return }
        sheet = .export(panel.exportRows())
    }

    func exportChosen(format: MarkExportFormat, included: [String], textIncluded: [String]) async {
        sheet = nil
        try? await Task.sleep(for: .milliseconds(50))
        let directory = markExportUsesLastFolder(format: format) ? panel.lastFolder() : nil
        guard let path = await MarkFiles.chooseSave(filter: markExportFilter(format: format), directory: directory) else { return }
        do {
            try panel.exportMarks(format: format, included: included, textIncluded: textIncluded, path: path)
        } catch {
            alert = MarksAlert(title: "Export Mark Collections", message: "The mark collections could not be exported: \(error.userMessage)")
        }
    }
}

// MARK: - Views

struct MarksPanelContent: View {
    @Bindable var model: MarksPanelModel

    var body: some View {
        VStack(spacing: 0) {
            MarksToolbar(model: model)
            Divider()
            MarksHeaderRow(model: model)
            Divider()
            ScrollView(.vertical) {
                LazyVStack(spacing: 0) {
                    ForEach(model.rows, id: \.id) { row in
                        MarkRowView(model: model, row: row)
                    }
                }
            }
            .background(Color.black)
        }
        .frame(minWidth: 180, minHeight: 120)
        .background(WindowFinder { model.window = $0 })
        .background(PanelFocusCapture(testHook: "marksFocus"))
        .sheet(item: $model.sheet) { sheet in
            MarksSheetContent(model: model, sheet: sheet)
        }
        .alert(model.alert?.title ?? "", isPresented: Binding(get: { model.alert != nil }, set: { if !$0 { model.alert = nil; model.alertDismissed() } })) {
            Button("OK") {}
        } message: {
            Text(model.alert?.message ?? "")
        }
        .confirmationDialog("Pangolin Beyond Import", isPresented: Binding(get: { model.askPangolin }, set: { if !$0 && model.askPangolin { model.pangolinAnswer(groupByColor: nil) } })) {
            Button("One for Each Color") { model.pangolinAnswer(groupByColor: true) }
            Button("Single Collection") { model.pangolinAnswer(groupByColor: false) }
            Button("Cancel", role: .cancel) { model.pangolinAnswer(groupByColor: nil) }
        } message: {
            Text("Create a Mark Collection for each Beyond color?")
        }
    }
}

/// F4 to F6.
struct MarksToolbar: View {
    let model: MarksPanelModel

    var body: some View {
        HStack(spacing: 2) {
            toolButton("plus", label: "New") { model.addCollection() }
            toolButton("square.and.arrow.down", label: "Import") { model.beginImport() }
            toolButton("square.and.arrow.up", label: "Export") { model.beginExport() }
                .disabled(model.rows.isEmpty)
            Spacer()
        }
        .padding(.horizontal, 4)
        .padding(.vertical, 3)
    }

    private func toolButton(_ symbol: String, label: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: symbol)
                .frame(width: 22, height: 20)
        }
        .buttonStyle(.borderless)
        .help(label)
        .accessibilityLabel(label)
    }
}

/// F7: the header icons, four of them buttons for F8.
struct MarksHeaderRow: View {
    let model: MarksPanelModel

    var body: some View {
        HStack(spacing: 0) {
            Image(systemName: "pencil")
                .frame(width: MarksColumns.toggle)
                .help("Active Collection")
                .accessibilityLabel("Active Collection")
            headerButton(image: "MarkLineVisible", label: "Show Mark Lines", header: .lines)
            headerButton(image: "MarkTailLineVisible", label: "Show Tail Mark Lines", header: .tailLines)
            headerButton(image: "MarkBarVisible", label: "Show Mark Bar", header: .bar)
            Button { model.clickHeader(.lock) } label: {
                Image(systemName: "lock").frame(width: MarksColumns.toggle, height: 18)
            }
            .buttonStyle(.plain)
            .help("Lock Access")
            .accessibilityLabel("Lock Access")
            Text("Collection")
                .padding(.leading, 4)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        .font(.callout)
        .frame(height: MarksColumns.rowHeight)
    }

    private func headerButton(image: String, label: String, header: MarkHeaderKind) -> some View {
        Button { model.clickHeader(header) } label: {
            MarksImage(name: image, size: 16)
                .frame(width: MarksColumns.toggle, height: 18)
        }
        .buttonStyle(.plain)
        .help(label)
        .accessibilityLabel(label)
    }
}

/// One collection: F9 to F16.
struct MarkRowView: View {
    @Bindable var model: MarksPanelModel
    let row: MarkRowInfo

    var body: some View {
        HStack(spacing: 0) {
            Button { model.setActive(row) } label: {
                Image(systemName: row.isDefault ? "largecircle.fill.circle" : "circle")
                    .foregroundStyle(row.isDefault ? Color.accentColor : Color.secondary)
                    .frame(width: MarksColumns.toggle, height: MarksColumns.rowHeight)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("\(row.name) Active")
            .accessibilityValue(row.isDefault ? "on" : "off")
            check(row.showLines, flag: .lines, label: "Lines")
            check(row.showTail, flag: .tailLines, label: "Tail Lines")
                .disabled(!row.tailEnabled)
            check(row.showBar, flag: .bar, label: "Bar")
            check(row.locked, flag: .lock, label: "Lock")
            nameCell
                .padding(.leading, 4)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        .frame(height: MarksColumns.rowHeight)
    }

    private func check(_ value: Bool, flag: MarkFlagKind, label: String) -> some View {
        Toggle(label, isOn: Binding(get: { value }, set: { model.setFlag(row, flag, $0) }))
            .toggleStyle(.checkbox)
            .labelsHidden()
            .controlSize(.small)
            .frame(width: MarksColumns.toggle)
            .accessibilityLabel("\(row.name) \(label)")
    }

    @ViewBuilder private var nameCell: some View {
        if model.editingID == row.id {
            MarkRenameField(model: model)
        } else {
            Text(row.name)
                .foregroundStyle(model.color(of: row))
                .lineLimit(1)
                .truncationMode(.tail)
                .frame(minWidth: 50, maxWidth: .infinity, minHeight: MarksColumns.rowHeight, alignment: .leading)
                .contentShape(Rectangle())
                .onTapGesture(count: 2) { model.beginRename(row) }
                .contextMenu { MarkRowMenu(model: model, row: row) }
                .accessibilityLabel(row.name)
                .accessibilityAddTraits(.isButton)
        }
    }
}

/// F15's field: Return or leaving it ends the edit, Escape puts the old name back, a duplicate keeps it open in red.
struct MarkRenameField: View {
    @Bindable var model: MarksPanelModel
    @FocusState private var focused: Bool

    var body: some View {
        HStack(spacing: 4) {
            TextField("Name", text: $model.editText)
                .textFieldStyle(.plain)
                .labelsHidden()
                .focused($focused)
                .padding(.horizontal, 2)
                .background(Color(nsColor: .textBackgroundColor))
                .overlay(RoundedRectangle(cornerRadius: 2).stroke(model.renameError == nil ? Color.clear : marksErrorColor, lineWidth: 2))
                .help(model.renameError ?? "")
                .onSubmit { model.endRename() }
                .onExitCommand { model.cancelRename() }
                .onChange(of: model.editText) { model.validateRename() }
                .accessibilityLabel("Collection Name")
            if model.renameError != nil {
                Text("Duplicate name")
                    .font(.caption)
                    .foregroundStyle(marksErrorColor)
                    .fixedSize()
            }
        }
        .onAppear {
            focused = true
            DispatchQueue.main.async {
                NSApp.sendAction(#selector(NSText.selectAll(_:)), to: nil, from: nil)
            }
        }
        .onChange(of: focused) { _, isFocused in
            if !isFocused {
                model.endRename()
            }
        }
    }
}

/// F16: Delete, Offset Time, Solid Line, Bold, Color, Weight, Type, Linked To.
struct MarkRowMenu: View {
    let model: MarksPanelModel
    let row: MarkRowInfo

    var body: some View {
        Button("Delete", systemImage: "trash", role: .destructive) { model.delete(row) }
        Button("Offset Time…", systemImage: "clock.arrow.circlepath") { model.sheet = .offset(row) }
        Toggle("Solid Line", isOn: Binding(get: { row.solid }, set: { model.setFlag(row, .solidLine, $0) }))
        Toggle("Bold", isOn: Binding(get: { row.bold }, set: { model.setFlag(row, .bold, $0) }))
        Button("Color…", systemImage: "paintpalette") { model.chooseColor(row) }
        Picker("Weight", selection: Binding(get: { row.level }, set: { model.setLevel(row, $0) })) {
            ForEach(Int32(1)...Int32(6), id: \.self) { level in
                Text("\(level)").tag(level)
            }
        }
        .pickerStyle(.menu)
        Picker("Type", selection: Binding(get: { row.kind }, set: { model.setKind(row, $0) })) {
            ForEach(MarkKind.allInOrder, id: \.self) { kind in
                Text(kind.title).tag(kind)
            }
        }
        .pickerStyle(.menu)
        if let choices = model.linkChoices(row) {
            Picker("Linked To", selection: Binding(get: { row.linkedTo }, set: { model.setLink(row, $0) })) {
                ForEach(choices, id: \.id) { choice in
                    Text(choice.name).tag(choice.id)
                }
            }
            .pickerStyle(.menu)
        }
    }
}

extension MarkKind {
    /// Enum order, as F23's submenu lists them.
    static let allInOrder: [MarkKind] = [.generic, .phrase, .word, .phoneme, .state]

    var title: String {
        switch self {
        case .generic: "Generic"
        case .phrase: "Phrase"
        case .word: "Word"
        case .phoneme: "Phoneme"
        case .state: "State"
        }
    }
}

/// One of Vixen's own icons from the bundle's `marks` folder.
struct MarksImage: View {
    let name: String
    let size: CGFloat

    @MainActor private static var cache: [String: NSImage] = [:]

    var body: some View {
        if let image = Self.load(name) {
            Image(nsImage: image)
                .resizable()
                .interpolation(.high)
                .scaledToFit()
                .frame(width: size, height: size)
        } else {
            Color.clear.frame(width: size, height: size)
        }
    }

    static func load(_ name: String) -> NSImage? {
        if let hit = cache[name] {
            return hit
        }
        let image = VixenResources.image("marks/\(name).png")
        cache[name] = image
        return image
    }
}
