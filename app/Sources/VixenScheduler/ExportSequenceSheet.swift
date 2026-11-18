import AppKit
import SwiftUI
import UniformTypeIdentifiers
import VixenBridge
import VixenKit

/// The Export Sequence dialog, `ExportDialog`: F1 to F13. A sheet on the sequencer window. All the logic is Rust's `ExportDialog`; this shows it.
enum ExportSheetPresenter {
    /// Builds the dialog off the main thread, since reading a network controller's configuration can look up a host name, then attaches it.
    static func present(session: SequencerSession, over parent: NSWindow?) {
        Task {
            let dialog = await Task.detached { session.exportDialog() }.value
            show(dialog: dialog, over: parent)
        }
    }

    static func show(dialog: ExportDialog, over parent: NSWindow?) {
        let model = ExportSheetModel(dialog: dialog)
        let host = NSHostingController(rootView: ExportSequenceView(model: model).studioAppearance()) // STUDIO: follow the chosen appearance
        // A fixed size, as upstream's dialog: letting the window follow the content can loop between the note's wrapping and the window's height.
        host.sizingOptions = []
        let size = host.view.fittingSize
        let sheet = NSWindow(contentRect: NSRect(origin: .zero, size: size), styleMask: [.titled], backing: .buffered, defer: false)
        // Swift/ARC owns the window, including the standalone close path.
        sheet.isReleasedWhenClosed = false
        sheet.contentViewController = host
        sheet.setContentSize(size)
        sheet.title = "Export Sequence"
        model.window = sheet
        model.dismiss = { [weak sheet] in
            guard let sheet else { return }
            if let parent = sheet.sheetParent {
                parent.endSheet(sheet)
            } else {
                sheet.close()
            }
        }
        ExportSheetModel.current = model
        if let parent {
            parent.beginSheet(sheet)
        } else {
            sheet.center()
            sheet.makeKeyAndOrderFront(nil)
        }
    }
}

/// One open dialog: what Rust says, cached for the view, and the run's progress.
@Observable
final class ExportSheetModel {
    let dialog: ExportDialog
    private(set) var view: ExportDialogView
    /// The Timing box. Typing goes to Rust as it happens; it's used for the next export and not saved.
    var timingText: String {
        didSet { dialog.setTimingText(text: timingText) }
    }
    private(set) var working = false
    /// The status strip's label: `Saving: {name}`, then `Export Complete` or `Export Canceled`.
    private(set) var statusText = ""
    private(set) var percent: Int32 = 0
    /// Whether the bar and the percentage show: while working only.
    private(set) var showsProgress = false

    @ObservationIgnored weak var window: NSWindow?
    @ObservationIgnored var dismiss: () -> Void = {}

    /// The dialog showing now, for tests.
    @ObservationIgnored static weak var current: ExportSheetModel?

    init(dialog: ExportDialog) {
        self.dialog = dialog
        let view = dialog.view()
        self.view = view
        timingText = view.timingText
    }

    private func refresh() {
        view = dialog.view()
    }

    func setFormat(_ index: Int) {
        dialog.setFormat(index: UInt32(index))
        refresh()
    }

    func chooseTiming(_ index: Int) {
        dialog.chooseTiming(index: UInt32(index))
        refresh()
        timingText = view.timingText
    }

    func setGenerate(_ on: Bool) {
        dialog.setGenerate(on: on)
        refresh()
    }

    func setFpp2x(_ on: Bool) {
        dialog.setFpp2x(on: on)
        refresh()
    }

    func setCompress(_ on: Bool) {
        dialog.setCompress(on: on)
        refresh()
    }

    func setActive(row: Int, _ on: Bool) {
        dialog.setRowActive(row: UInt32(row), on: on)
        refresh()
    }

    func move(from: IndexSet, to: Int) {
        dialog.moveRows(from: from.map { UInt32($0) }, to: UInt32(to))
        refresh()
    }

    /// F9: the save panel, then the run, then what follows it.
    func start() async {
        guard !working else { return }
        let request: ExportSavePanel?
        do {
            request = try dialog.savePanel()
        } catch {
            await SchedulerAlerts.tell("Export Failed", error.userMessage, style: .error, window: window)
            return
        }
        // A sequence that has never been saved can't be exported, and upstream says nothing.
        guard let request, let url = await chooseFile(request) else { return }
        let outcome: ExportStartOutcome
        do {
            outcome = try dialog.start(outPath: url.path, listener: ExportForwarder(
                progress: { [weak self] percent in self?.percent = percent },
                finished: { [weak self] result in self?.finished(result) }
            ))
        } catch {
            await SchedulerAlerts.tell("Export Failed", error.userMessage, style: .error, window: window)
            return
        }
        switch outcome {
        case .invalidTiming(let message):
            await SchedulerAlerts.tell("Error", message, style: .warning, window: window)
        case .started:
            working = true
            percent = 0
            showsProgress = true
            statusText = "Saving: \(view.sequenceName)"
        }
    }

    private func chooseFile(_ request: ExportSavePanel) async -> URL? {
        let directory = URL(fileURLWithPath: request.directory, isDirectory: true)
        if UITestHarness.isActive {
            // The panel would wait for a person. A test takes the offered name, or the file it asks for.
            if let file = ProcessInfo.processInfo.environment["VIXEN_TEST_FILE"] {
                return URL(fileURLWithPath: file)
            }
            UITestHarness.note("export to \(directory.appending(path: request.fileName).path)")
            return directory.appending(path: request.fileName)
        }
        let panel = NSSavePanel()
        panel.directoryURL = directory
        panel.message = "\(request.formatName) (*.\(request.extension))"
        if let type = UTType(filenameExtension: request.extension) {
            panel.allowedContentTypes = [type]
        }
        // The filter's `All Files(*.*)`.
        panel.allowsOtherFileTypes = true
        panel.isExtensionHidden = false
        // Set the name after the type options so AppKit does not append the extension twice.
        panel.nameFieldStringValue = request.fileName
        if let window {
            return await panel.beginSheetModal(for: window) == .OK ? panel.url : nil
        }
        return panel.runModal() == .OK ? panel.url : nil
    }

    private func finished(_ result: ExportFinish) {
        working = false
        showsProgress = false
        statusText = result.statusText
        Task {
            if let error = result.error {
                await SchedulerAlerts.tell("Export Failed", error, style: .error, window: window)
            } else if let warning = result.falconWarning {
                await SchedulerAlerts.tell("Warning", warning, style: .warning, window: window)
            }
        }
    }

    func stop() {
        dialog.stop()
    }

    /// F13's FIX: a sheet can't outlive its export, so closing during one asks first.
    func close() async {
        if working {
            let stop = await SchedulerAlerts.confirm(
                "An export is in progress. Stop it and close?", "", style: .warning, confirm: "Stop and Close", cancel: "Keep Exporting", window: window
            )
            guard stop else { return }
            let dialog = dialog
            await Task.detached { dialog.close() }.value
        }
        dismiss()
    }
}

extension ExportControllerRow: Identifiable {}

/// The dialog's regions, top to bottom as upstream: Settings, Network, the note, Generate Controller File, Enable Compression, the buttons,
/// the status strip.
struct ExportSequenceView: View {
    @Bindable var model: ExportSheetModel

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            GroupBox("Settings") {
                HStack(spacing: 8) {
                    Picker("Format:", selection: Binding(get: { Int(model.view.formatIndex) }, set: { model.setFormat($0) })) {
                        ForEach(Array(model.view.formatNames.enumerated()), id: \.offset) { index, name in
                            Text(name).tag(index)
                        }
                    }
                    .fixedSize()
                    Spacer(minLength: 12)
                    Text("Timing (ms):")
                    TimingField(model: model)
                }
                .padding(6)
                .disabled(model.working)
            }

            GroupBox("Network") {
                NetworkTable(model: model)
                    .frame(height: 190)
                    .disabled(model.working)
            }

            Text(model.view.note)
                .font(.callout)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)

            HStack(spacing: 14) {
                Toggle("Generate Controller File", isOn: Binding(get: { model.view.generate }, set: { model.setGenerate($0) }))
                Picker("Controller file version", selection: Binding(get: { model.view.fpp2x }, set: { model.setFpp2x($0) })) {
                    Text("FPP 1.x").tag(false)
                    Text("FPP 2.x").tag(true)
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .fixedSize()
                .disabled(!model.view.radiosEnabled)
            }
            .disabled(model.working)

            Toggle("Enable Compression", isOn: Binding(get: { model.view.compress }, set: { model.setCompress($0) }))
                .disabled(!model.view.compressEnabled || model.working)

            HStack {
                Button("Start") { Task { await model.start() } }
                    .keyboardShortcut(.defaultAction)
                    .disabled(model.working)
                Button("Stop") { model.stop() }
                    .disabled(!model.working)
                Spacer()
                Button("Close") { Task { await model.close() } }
                    .keyboardShortcut(.cancelAction)
            }
            .controlSize(.large)

            Divider()
            StatusStrip(model: model)
        }
        .padding(16)
        .frame(width: 540)
    }
}

/// The editable Timing combo: type a value, or pick 25, 50 or 100.
private struct TimingField: View {
    @Bindable var model: ExportSheetModel

    var body: some View {
        HStack(spacing: 2) {
            TextField("Timing (ms)", text: $model.timingText)
                .labelsHidden()
                .multilineTextAlignment(.trailing)
                .monospacedDigit()
                .frame(width: 52)
            Menu {
                ForEach(Array(model.view.timingChoices.enumerated()), id: \.offset) { index, choice in
                    Button(choice) { model.chooseTiming(index) }
                }
            } label: {
                EmptyView()
            }
            .menuStyle(.borderlessButton)
            .fixedSize()
            .accessibilityLabel("Timing choices")
        }
    }
}

/// `networkListView`: ticks and drag to reorder, columns Controller, Channels, Start and End.
private struct NetworkTable: View {
    @Bindable var model: ExportSheetModel

    var body: some View {
        let rows = model.view.rows
        Table(of: ExportControllerRow.self) {
            TableColumn("Controller") { row in
                Toggle(row.name, isOn: Binding(
                    get: { row.active },
                    set: { on in
                        if let index = rows.firstIndex(where: { $0.id == row.id }) {
                            model.setActive(row: index, on)
                        }
                    }
                ))
            }
            TableColumn("Channels") { row in
                Text(String(row.channels)).monospacedDigit()
            }
            .width(70)
            TableColumn("Start") { row in
                Text(row.start.map(String.init) ?? "").monospacedDigit()
            }
            .width(60)
            TableColumn("End") { row in
                Text(row.end.map(String.init) ?? "").monospacedDigit()
            }
            .width(60)
        } rows: {
            ForEach(rows) { row in
                TableRow(row)
                    .draggable(row.id)
            }
            // A drag within the list reorders it, as `AllowRowReorder` does.
            .dropDestination(for: String.self) { (index: Int, ids: [String]) in
                let from = IndexSet(ids.compactMap { id in rows.firstIndex { $0.id == id } })
                model.move(from: from, to: index)
            }
        }
    }
}

/// `progressLabel`, `exportProgressBar` and `currentTimeLabel`, which shows the percentage.
private struct StatusStrip: View {
    let model: ExportSheetModel

    var body: some View {
        HStack(spacing: 10) {
            Text(model.statusText)
                .lineLimit(1)
                .truncationMode(.middle)
            Spacer()
            if model.showsProgress {
                ProgressView(value: Double(model.percent), total: 100)
                    .frame(width: 140)
                Text("\(model.percent)%")
                    .monospacedDigit()
                    .frame(width: 40, alignment: .trailing)
            }
        }
        .font(.callout)
        .frame(height: 18)
    }
}

/// Hops a run's reports to the main actor.
private nonisolated final class ExportForwarder: ExportDialogListener, @unchecked Sendable {
    private let onProgress: @MainActor (Int32) -> Void
    private let onFinished: @MainActor (ExportFinish) -> Void

    init(progress: @escaping @MainActor (Int32) -> Void, finished: @escaping @MainActor (ExportFinish) -> Void) {
        onProgress = progress
        onFinished = finished
    }

    func progress(percent: Int32) {
        let call = onProgress
        Task { @MainActor in call(percent) }
    }

    func finished(result: ExportFinish) {
        let call = onFinished
        Task { @MainActor in call(result) }
    }
}

/// The dialog in a window of its own, for a sequence opened from its file. See `WindowID.exportSequence`.
struct ExportSequenceWindow: View {
    let path: String?
    @Environment(AppModel.self) private var app
    @Environment(\.dismiss) private var dismissWindow
    @State private var model: ExportSheetModel?
    @State private var failure: String?

    var body: some View {
        Group {
            if let model {
                ExportSequenceView(model: model)
            } else if let failure {
                ContentUnavailableView("Can't Export", systemImage: "exclamationmark.triangle", description: Text(failure))
                    .frame(width: 420, height: 220)
            } else {
                ProgressView().frame(width: 420, height: 220)
            }
        }
        .navigationTitle("Export Sequence")
        .task(id: path) { await load() }
        .background(WindowFinder { window in model?.window = window })
    }

    private func load() async {
        await app.start()
        guard let core = app.core, let path else {
            failure = "No sequence was given."
            return
        }
        do {
            let dialog = try await Task.detached { try core.openSequence(path: path).exportDialog() }.value
            let made = ExportSheetModel(dialog: dialog)
            made.dismiss = { dismissWindow() }
            ExportSheetModel.current = made
            model = made
        } catch {
            failure = error.userMessage
        }
    }
}

/// Hands over the window a view is in.
private struct WindowFinder: NSViewRepresentable {
    let found: (NSWindow) -> Void

    func makeNSView(context: Context) -> NSView {
        FinderView(found: found)
    }

    func updateNSView(_ view: NSView, context: Context) {
        if let window = view.window { found(window) }
    }

    private final class FinderView: NSView {
        let found: (NSWindow) -> Void

        init(found: @escaping (NSWindow) -> Void) {
            self.found = found
            super.init(frame: .zero)
        }

        @available(*, unavailable)
        required init?(coder: NSCoder) { fatalError("not used") }

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            if let window { found(window) }
        }
    }
}
