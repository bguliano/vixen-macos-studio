import StudioAgent
import SwiftUI
import VixenBridge
import VixenKit

/// Studio's main window: the show at a glance. A sidebar lists the show's areas; Sequences is a searchable library of
/// every sequence in the profile. Replaces the Windows-style administration window (`AdministrationView`).
enum StudioSection: String, Hashable, CaseIterable, Identifiable {
    case sequences, display, previews, schedule
    var id: Self { self }

    var title: String {
        switch self {
        case .sequences: "Sequences"
        case .display: "Display"
        case .previews: "Previews"
        case .schedule: "Schedule"
        }
    }

    var systemImage: String {
        switch self {
        case .sequences: "music.note.list"
        case .display: "lightbulb.2"
        case .previews: "eye"
        case .schedule: "calendar"
        }
    }
}

struct StudioShowWindow: View {
    @Environment(AdminModel.self) private var admin
    @Environment(\.openWindow) private var openWindow
    @SceneStorage("studio.section") private var selection: StudioSection = .sequences
    @State private var library = StudioLibrary()
    @State private var agents = StudioAgentActivity()

    var body: some View {
        StudioSplitView {
            sidebar
        } detail: {
            detail
        }
        .navigationTitle(admin.profileName ?? "Vixen Studio")
        .navigationSubtitle(admin.version.versionName)
        .toolbar { toolbar }
        .frame(minWidth: 860, minHeight: 520)
        .background(WindowReader { admin.attach(window: $0) })
        .background(StudioWindowOpenerRegistration())
        .alerts(from: admin.alerts)
        .sheet(item: Binding(get: { admin.sheet }, set: { admin.sheet = $0 })) { sheet in
            sheetContent(sheet)
                .interactiveDismissDisabled()
                .environment(admin)
        }
        .environment(library)
        .onAppear { admin.openWindow = openWindow }
        .task { await admin.start() }
        .task(id: admin.startupFinished) {
            await library.reload(admin: admin)
            if admin.startupFinished, let core = admin.app.core { agents.observe(core: core) }
        }
        .onChange(of: agents.revision) { Task { await library.reload(admin: admin) } }
        .onChange(of: admin.recent.map(\.path)) { Task { await library.reload(admin: admin) } }
        .onReceive(NotificationCenter.default.publisher(for: NSWindow.didBecomeKeyNotification)) { _ in
            Task { await library.reload(admin: admin) }
        }
    }

    // MARK: Sidebar

    private var sidebar: some View {
        List(selection: Binding(get: { selection }, set: { if let new = $0 { selection = new } })) {
            Section("Show") {
                ForEach(StudioSection.allCases) { section in
                    Label(section.title, systemImage: section.systemImage)
                        .badge(badge(for: section))
                        .tag(section)
                }
            }
        }
        .listStyle(.sidebar)
        .safeAreaInset(edge: .bottom) { sidebarFooter }
    }

    private func badge(for section: StudioSection) -> Int {
        switch section {
        case .sequences: library.sequences.count
        case .display: Int(library.overview?.elements ?? 0)
        case .previews: Int(library.overview?.previews ?? 0)
        case .schedule: 0
        }
    }

    private var sidebarFooter: some View {
        VStack(alignment: .leading, spacing: StudioMetrics.spacingXS) {
            if admin.progressVisible, !admin.progressText.isEmpty {
                Text(admin.progressText)
                    .lineLimit(1)
                ProgressView(value: admin.progressValue)
                    .progressViewStyle(.linear)
            }
            if !admin.updateNotice.isEmpty {
                Label(admin.updateNotice, systemImage: "arrow.down.circle")
                    .lineLimit(2)
            }
            StudioCpuText(readout: admin.cpu)
        }
        .font(.caption)
        .foregroundStyle(.secondary)
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(StudioMetrics.spacingM)
    }

    // MARK: Detail

    @ViewBuilder
    private var detail: some View {
        switch selection {
        case .sequences:
            StudioSequencesPane()
        case .display:
            StudioAreaPane(
                section: .display,
                summary: "Your props, groups and elements, the controllers that drive them, and the patching between them.",
                stats: displayStats,
                primary: ("Open Display Setup", { admin.showDisplaySetup() })
            )
        case .previews:
            StudioAreaPane(
                section: .previews,
                summary: "On-screen layouts of your display, used to watch sequences without the real lights.",
                stats: [("Previews", library.overview.map { "\($0.previews)" } ?? "–")],
                primary: ("Set Up Previews", { admin.showSetupPreviews() })
            )
        case .schedule:
            StudioAreaPane(
                section: .schedule,
                summary: "Shows bundle sequences, actions and pauses. Schedule them to run on their own at set times and days.",
                stats: [],
                primary: ("Open Scheduler", { openWindow(id: WindowID.schedules) })
            )
        }
    }

    private var displayStats: [(String, String)] {
        guard let o = library.overview else { return [] }
        return [
            ("Top-level groups", "\(o.topLevelItems)"),
            ("Elements", o.elements.formatted()),
            ("Controllers", "\(o.controllers)"),
            ("Outputs", o.outputs.formatted()),
        ]
    }

    // MARK: Toolbar

    @ToolbarContentBuilder
    private var toolbar: some ToolbarContent {
        ToolbarItem(placement: .primaryAction) {
            StudioAgentStatusButton(activity: agents)
        }
        ToolbarItem(placement: .primaryAction) {
            StudioLiveOutputToggle()
        }
    }

    @ViewBuilder
    private func sheetContent(_ sheet: AdminSheet) -> some View {
        switch sheet {
        case .selectProfile:
            SelectProfileSheet()
        case .dataProfile:
            DataProfileSheet { saved in admin.profilesEdited(saved: saved) }
        case .setupPreviews:
            SetupPreviewsSheet()
        case .checkForUpdates:
            CheckForUpdatesSheet()
        case .webServer:
            if let server = admin.webServer { WebServerSheet(server: server) }
        }
    }
}

/// The profile's sequences and show summary, loaded off the main thread.
@Observable
final class StudioLibrary {
    private(set) var sequences: [StudioSequenceSummary] = []
    private(set) var overview: StudioShowOverview?
    private(set) var loaded = false

    func reload(admin: AdminModel) async {
        guard admin.startupFinished, let core = admin.app.core else { return }
        let root = core.dataRoot()
        let (rows, overview) = await Task.detached {
            (studioSequenceLibrary(dataRoot: root), core.studioShowOverview())
        }.value
        if rows != sequences { sequences = rows }
        if overview != self.overview { self.overview = overview }
        loaded = true
    }
}

/// Live output: whether Vixen is driving controllers (Vixen calls this "execution"). One toggle, always in the toolbar.
struct StudioLiveOutputToggle: View {
    @Environment(AdminModel.self) private var admin

    var body: some View {
        let status = admin.executionStatus
        let isOn = status.stopEnabled
        Toggle(isOn: Binding(
            get: { isOn },
            set: { $0 ? admin.startExecution() : admin.stopExecution() }
        )) {
            Label(isOn ? "Live Output On" : "Live Output Off", systemImage: isOn ? "dot.radiowaves.left.and.right" : "wifi.slash")
        }
        .toggleStyle(.button)
        .labelStyle(.titleAndIcon)
        .tint(isOn ? .green : nil)
        .disabled(!status.startEnabled && !status.stopEnabled)
        .help(isOn
            ? "Controllers are receiving lighting data. Turn off to sequence without driving the real lights, which also speeds up editing."
            : "Controllers are idle. Turn on to send lighting data to your controllers.")
    }
}

/// One `Text`, so the reading changing once a second touches nothing else.
private struct StudioCpuText: View {
    let readout: CpuReadout

    var body: some View {
        Text(readout.text)
            .monospacedDigit()
    }
}

/// Registers the SwiftUI window opener with the test harness.
private struct StudioWindowOpenerRegistration: View {
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        Color.clear
            .frame(width: 0, height: 0)
            .onAppear { UITestHarness.openWindow = { openWindow(id: $0) } }
    }
}

// MARK: - Sequences

nonisolated private struct SequenceRow: Identifiable, Hashable, Sendable {
    let id: String
    let name: String
    let modified: Date
    let length: Double
    let audio: String
    let hasAudio: Bool
    let lengthMs: Double?

    init(_ summary: StudioSequenceSummary) {
        id = summary.path
        name = summary.name
        modified = Date(timeIntervalSince1970: summary.modified)
        lengthMs = summary.lengthMs
        length = summary.lengthMs ?? -1
        hasAudio = summary.audioName != nil
        audio = summary.audioName.map { ($0 as NSString).deletingPathExtension } ?? ""
    }
}

struct StudioSequencesPane: View {
    @Environment(AdminModel.self) private var admin
    @Environment(StudioLibrary.self) private var library
    @State private var search = ""
    @State private var selection = Set<String>()
    @State private var sortOrder: [KeyPathComparator<SequenceRow>] = [KeyPathComparator(\.modified, order: .reverse)]

    var body: some View {
        content
            .searchable(text: $search, placement: .toolbar, prompt: "Search sequences or songs")
            .toolbar {
                ToolbarItemGroup(placement: .primaryAction) {
                    Button { admin.chooseSequencesToOpen() } label: {
                        Label("Open…", systemImage: "folder")
                    }
                    .help("Open a sequence file from anywhere")
                    .disabled(admin.isBusy)
                    Button { admin.newSequence() } label: {
                        Label("New Sequence", systemImage: "plus")
                    }
                    .help("Create a sequence")
                    .disabled(admin.isBusy)
                }
            }
            .dropDestination(for: URL.self) { urls, _ in
                let files = urls.filter { ["tim", "vix"].contains($0.pathExtension.lowercased()) }
                admin.openFiles(files)
                return !files.isEmpty
            }
    }

    @ViewBuilder
    private var content: some View {
        if !library.loaded {
            ProgressView("Loading show…")
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else if library.sequences.isEmpty {
            ContentUnavailableView {
                Label("No Sequences Yet", systemImage: "music.note")
            } description: {
                Text("A sequence choreographs your lights, usually to a song. Create one to get started, or drop a .tim file here.")
            } actions: {
                Button("New Sequence") { admin.newSequence() }
                    .buttonStyle(.borderedProminent)
                    .disabled(admin.isBusy)
            }
        } else if rows.isEmpty {
            ContentUnavailableView.search(text: search)
        } else {
            VStack(spacing: 0) {
                if search.isEmpty, !recent.isEmpty {
                    recentStrip
                    Divider()
                }
                table
            }
        }
    }

    private var rows: [SequenceRow] {
        let all = library.sequences.map(SequenceRow.init)
        let filtered = search.isEmpty ? all : all.filter {
            $0.name.localizedCaseInsensitiveContains(search) || $0.audio.localizedCaseInsensitiveContains(search)
        }
        return filtered.sorted(using: sortOrder)
    }

    /// The recently opened sequences that still exist, most recent first.
    private var recent: [StudioSequenceSummary] {
        let byPath = Dictionary(library.sequences.map { ($0.path, $0) }, uniquingKeysWith: { a, _ in a })
        return admin.recent.prefix(6).compactMap { byPath[$0.path] }
    }

    private var recentStrip: some View {
        VStack(alignment: .leading, spacing: StudioMetrics.spacingS) {
            Text("Recently Opened")
                .font(.headline)
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: StudioMetrics.spacingM) {
                    ForEach(recent, id: \.path) { item in
                        Button { admin.openRecent(path: item.path) } label: {
                            StudioSequenceCard(summary: item)
                        }
                        .buttonStyle(.plain)
                        .help("Open \(item.name)")
                    }
                }
                .padding(.vertical, 2)
            }
        }
        .padding(StudioMetrics.spacingL)
    }

    private var table: some View {
        Table(rows, selection: $selection, sortOrder: $sortOrder) {
            TableColumn("Name", value: \.name) { row in
                Label(row.name, systemImage: row.hasAudio ? "music.note" : "sparkles")
                    .lineLimit(1)
            }
            .width(min: 180, ideal: 280)
            TableColumn("Length", value: \.length) { row in
                Text(row.lengthMs.map { studioFormatDuration(ms: $0) } ?? "–")
                    .monospacedDigit()
                    .foregroundStyle(.secondary)
            }
            .width(min: 60, ideal: 70, max: 90)
            TableColumn("Song", value: \.audio) { row in
                Text(row.hasAudio ? row.audio : "No audio")
                    .lineLimit(1)
                    .foregroundStyle(row.hasAudio ? .secondary : .tertiary)
            }
            .width(min: 120, ideal: 220)
            TableColumn("Modified", value: \.modified) { row in
                Text(row.modified, format: .relative(presentation: .named))
                    .foregroundStyle(.secondary)
            }
            .width(min: 90, ideal: 120)
        }
        .contextMenu(forSelectionType: String.self) { paths in
            if !paths.isEmpty {
                Button(paths.count == 1 ? "Open" : "Open \(paths.count) Sequences") { admin.openFiles(paths.map { URL(fileURLWithPath: $0) }) }
                Button("Show in Finder") {
                    NSWorkspace.shared.activateFileViewerSelecting(paths.map { URL(fileURLWithPath: $0) })
                }
            }
        } primaryAction: { paths in
            admin.openFiles(paths.map { URL(fileURLWithPath: $0) })
        }
    }
}

/// A recently opened sequence, as a card: a waveform-like glyph, the name, the song and the length.
private struct StudioSequenceCard: View {
    let summary: StudioSequenceSummary

    var body: some View {
        VStack(alignment: .leading, spacing: StudioMetrics.spacingS) {
            RoundedRectangle(cornerRadius: StudioMetrics.smallCornerRadius, style: .continuous)
                .fill(LinearGradient(colors: gradient, startPoint: .topLeading, endPoint: .bottomTrailing))
                .frame(height: 64)
                .overlay {
                    Image(systemName: summary.audioName == nil ? "sparkles" : "waveform")
                        .font(.system(size: 26, weight: .medium))
                        .foregroundStyle(.white.opacity(0.9))
                }
            Text(summary.name)
                .font(.callout.weight(.semibold))
                .lineLimit(1)
            Text(summary.lengthMs.map { studioFormatDuration(ms: $0) } ?? "No length")
                .font(.caption)
                .monospacedDigit()
                .foregroundStyle(.secondary)
        }
        .frame(width: 168, alignment: .leading)
        .studioCard(padding: StudioMetrics.spacingS)
        .contentShape(Rectangle())
    }

    /// A stable colour pair per sequence, so cards are easy to tell apart at a glance.
    private var gradient: [Color] {
        let hue = Double(summary.name.unicodeScalars.reduce(0) { ($0 &* 31 &+ Int($1.value)) % 360 }) / 360
        return [Color(hue: hue, saturation: 0.65, brightness: 0.75), Color(hue: (hue + 0.12).truncatingRemainder(dividingBy: 1), saturation: 0.7, brightness: 0.5)]
    }
}

// MARK: - Area panes

/// A show area's summary with the action that opens its editor.
struct StudioAreaPane: View {
    let section: StudioSection
    let summary: String
    let stats: [(String, String)]
    let primary: (String, () -> Void)
    @Environment(AdminModel.self) private var admin

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: StudioMetrics.spacingXL) {
                HStack(alignment: .top, spacing: StudioMetrics.spacingL) {
                    Image(systemName: section.systemImage)
                        .font(.system(size: 30, weight: .medium))
                        .foregroundStyle(.tint)
                        .frame(width: 44)
                    VStack(alignment: .leading, spacing: StudioMetrics.spacingXS) {
                        Text(section.title)
                            .font(.largeTitle.weight(.semibold))
                        Text(summary)
                            .foregroundStyle(.secondary)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
                if !stats.isEmpty {
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 140), spacing: StudioMetrics.spacingM)], spacing: StudioMetrics.spacingM) {
                        ForEach(stats, id: \.0) { stat in
                            VStack(alignment: .leading, spacing: 2) {
                                Text(stat.1)
                                    .font(.title.weight(.semibold))
                                    .monospacedDigit()
                                Text(stat.0)
                                    .font(.callout)
                                    .foregroundStyle(.secondary)
                            }
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .studioCard()
                        }
                    }
                }
                Button(action: primary.1) {
                    Text(primary.0)
                        .padding(.horizontal, StudioMetrics.spacingS)
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
                .disabled(admin.isBusy)
            }
            .frame(maxWidth: 720, alignment: .leading)
            .padding(StudioMetrics.spacingXL)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}
