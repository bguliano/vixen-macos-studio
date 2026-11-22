import AppKit
import SwiftUI
import VixenBridge
import VixenKit

/// Used when `VixenSequencer` isn't in the build: opens a real sequence beside the Mark Collections panel, so the panel can be driven and
/// photographed alone. `VIXEN_MARKS_SEQUENCE` names a sequence in the profile's Sequence folder to open at once.
public struct MarksTestScene: Scene {
    private let model: AppModel
    @State private var shared = MarksTestShared()

    public init(model: AppModel) {
        self.model = model
    }

    public var body: some Scene {
        Window("Marks Test", id: "marks-test") {
            MarksTestView(shared: shared)
                .environment(model)
                .studioAppearance() // STUDIO: follow the chosen appearance
                .task { await model.start() }
        }
        .defaultSize(width: 780, height: 560)
        .defaultLaunchBehavior(.suppressed)
        .restorationBehavior(.disabled)

        // The sequencer window's Tools > LipSync > Lyric Converter, for this window. Nothing pastes here: there's no timeline.
        Window("LipSync Text Convert", id: MarksTestScene.lyricConverterID) {
            MarksTestLyricConverter(shared: shared)
                .studioAppearance() // STUDIO: follow the chosen appearance
        }
        .windowResizability(.contentMinSize)
        .defaultSize(width: 320, height: 440)
        .defaultLaunchBehavior(.suppressed)
        .restorationBehavior(.disabled)
    }

    static let lyricConverterID = "marks-test-lyric-converter"
}

/// The sequence the Marks Test window has open, for its other windows.
@Observable
final class MarksTestShared {
    var session: SequencerSession?
}

private struct MarksTestLyricConverter: View {
    let shared: MarksTestShared

    var body: some View {
        if let session = shared.session {
            LyricConverterView(session: session)
                .id(ObjectIdentifier(session))
        } else {
            ContentUnavailableView("No sequence open", systemImage: "text.bubble", description: Text("Open a sequence in the Marks Test window first."))
                .frame(minWidth: 320, minHeight: 440)
        }
    }
}

private struct MarksTestView: View {
    let shared: MarksTestShared
    @Environment(AppModel.self) private var model
    @Environment(\.openWindow) private var openWindow
    @State private var sequences: [String] = []
    @State private var chosen: String?
    @State private var session: SequencerSession?
    @State private var problem: String?
    @State private var opening = false
    @State private var showingBeats = false
    @State private var showingImageMaps = false
    @State private var showingFaceMapping = false

    var body: some View {
        // Fixed widths, so scripts can click the panel's icons at known points: the panel starts at x = 301.
        HStack(spacing: 0) {
            VStack(alignment: .leading, spacing: 8) {
                Text("Sequences").font(.headline)
                List(sequences, id: \.self, selection: $chosen) { name in
                    Text(name)
                }
                HStack {
                    Button("Open") {
                        if let chosen {
                            Task { _ = await open(chosen) }
                        }
                    }
                    .disabled(chosen == nil || opening)
                    if opening {
                        ProgressView().controlSize(.small)
                    }
                }
                if let session {
                    MarksTestStatus(session: session)
                    // The sequencer window's Tools > Audio > Beat/Bar Detection, for this window.
                    Button("Beat/Bar Detection…") { showingBeats = true }
                        .disabled(!session.beatDetectionAvailable())
                    Button("Lyric Converter…") { openWindow(id: MarksTestScene.lyricConverterID) }
                }
                if let core = model.core {
                    // The sequencer window's Tools > LipSync > Edit Image Maps and Default Image Map, for this window.
                    Button("Image Maps…") { showingImageMaps = true }
                    Menu("Default Image Map") {
                        DefaultImageMapMenuItems(core: core)
                    }
                    // Tools > LipSync > Edit Element Face Mapping.
                    Button("Edit Element Face Mapping…") { showingFaceMapping = true }
                }
                if let problem {
                    Text(problem).foregroundStyle(.red)
                }
            }
            .padding(10)
            .frame(width: 300)
            .frame(maxHeight: .infinity)
            Divider()
            Group {
                if let session {
                    MarkCollectionsPanel(session: session)
                        .id(ObjectIdentifier(session))
                } else {
                    ContentUnavailableView("No sequence open", systemImage: "metronome", description: Text("Open a sequence to see its mark collections."))
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .sheet(isPresented: $showingBeats) {
            if let session {
                BeatsAndBarsSheet(session: session) { showingBeats = false }
            }
        }
        .sheet(isPresented: $showingImageMaps) {
            if let core = model.core {
                ImageMapsSheet(core: core) { _ in showingImageMaps = false }
            }
        }
        .sheet(isPresented: $showingFaceMapping) {
            if let core = model.core {
                FaceMappingNodeSelectSheet(core: core, model: model) { _ in showingFaceMapping = false }
            }
        }
        .task(id: model.core != nil) {
            guard let core = model.core else { return }
            ImageMapsTestHooks.install { core }
            FaceNodeSelectTestHooks.install()
            let folder = URL(fileURLWithPath: core.dataRoot()).appending(path: "Sequence")
            let names = (try? FileManager.default.contentsOfDirectory(atPath: folder.path)) ?? []
            sequences = names.filter { $0.hasSuffix(".tim") }.sorted { $0.localizedStandardCompare($1) == .orderedAscending }
            MarksTestHooks.install()
            MarksTestHooks.openSequence = { name in await open(name) }
            if let name = ProcessInfo.processInfo.environment["VIXEN_MARKS_SEQUENCE"] {
                chosen = name
                _ = await open(name)
            }
        }
    }

    private func open(_ name: String) async -> String {
        guard let core = model.core else { return "FAIL no profile" }
        opening = true
        defer { opening = false }
        let path = name.hasPrefix("/") ? name : core.dataRoot() + "/Sequence/" + name
        do {
            let opened: SequencerSession = try await Task.detached { try core.openSequence(path: path) }.value
            session = opened
            shared.session = opened
            MarksTestHooks.session = opened
            problem = nil
            return "opened \(opened.name())"
        } catch {
            problem = error.userMessage
            return "FAIL \(error.userMessage)"
        }
    }
}

/// The sequence's name and whether it's modified, refreshed as the panel changes it.
private struct MarksTestStatus: View {
    let session: SequencerSession
    @State private var modified = false

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(session.name()).bold()
            Text(modified ? "Modified" : "Not modified").foregroundStyle(.secondary)
        }
        .task {
            while !Task.isCancelled {
                modified = session.isModified()
                try? await Task.sleep(for: .milliseconds(250))
            }
        }
    }
}

/// What test scripts can ask of the panel beyond clicks: `{ "action": "hook", "name": "marksRows" }` and so on.
enum MarksTestHooks {
    static weak var current: MarksPanelModel?
    static var session: SequencerSession?
    static var openSequence: ((String) async -> String)?
    private static var installed = false

    static func install() {
        guard UITestHarness.isActive, !installed else { return }
        installed = true
        LyricTestHooks.install()
        UITestHarness.hooks["marksEdit"] = { args in
            guard let model = current else { return "FAIL no panel" }
            switch args.first ?? "" {
            case "new": model.addCollection()
            case "rename":
                guard args.count > 1, let index = Int(args[1]), model.rows.indices.contains(index) else { return "FAIL unknown row" }
                model.beginRename(model.rows[index])
            // STUDIO-BEGIN marks-panel-test-edits: open the live Offset popover, drive it as its field does, and delete a row (R40)
            case "offset", "offsetLive", "delete":
                guard args.count > 1, let index = Int(args[1]), model.rows.indices.contains(index) else { return "FAIL unknown row" }
                let row = model.rows[index]
                if args[0] == "offset" {
                    model.offsetting = row.id
                    // Popovers have no title; name it so Studio Capture can find it.
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.6) {
                        NSApp.windows.first { String(describing: type(of: $0)).contains("Popover") && $0.isVisible }?.title = "Offset Time"
                    }
                }
                else if args[0] == "delete" { model.delete(row) }
                else {
                    guard args.count > 3, let ms = Int32(args[2]) else { return "FAIL offsetLive needs a row, milliseconds and continuing" }
                    model.offsetLive(row, total: ms, continuing: args[3] == "true")
                }
                return "rows=\(model.rows.count) offsetting=\(model.offsetting ?? "") undo=\(MarksTestHooks.session?.undoDescriptions().first ?? "")"
            // STUDIO-END marks-panel-test-edits
            default: return "FAIL unknown edit"
            }
            return "rows=\(model.rows.count) editing=\(model.editingID ?? "")"
        }
        UITestHarness.hooks["marksOpen"] = { args in
            guard let name = args.first, let open = openSequence else { return "FAIL needs a sequence name and the Marks Test window" }
            return await open(name)
        }
        StudioTimingPicker.installTestHooks() // STUDIO: queue Import ▸ Timing from Sequence…'s answers (#61)
        UITestHarness.hooks["marksNextFile"] = { args in
            MarkFiles.queued.append(contentsOf: args)
            return "queued \(MarkFiles.queued.count)"
        }
        UITestHarness.hooks["marksRows"] = { args in
            guard let model = current else { return "FAIL no panel" }
            model.reload()
            let rows = model.rows.map { r -> [String: Any] in
                [
                    "name": r.name, "id": r.id, "argb": String(format: "%08X", r.argb), "default": r.isDefault, "lines": r.showLines,
                    "tailEnabled": r.tailEnabled, "tail": r.showTail, "bar": r.showBar, "locked": r.locked, "level": r.level,
                    "kind": r.kind.title, "linkedTo": r.linkedTo ?? "", "solid": r.solid, "bold": r.bold, "marks": r.markCount,
                ]
            }
            let state: [String: Any] = [
                "rows": rows,
                "modified": session?.isModified() ?? false,
                "editing": model.editingID ?? "",
                "renameError": model.renameError ?? "",
                "headerNext": [
                    "lines": model.panel.headerNextValue(header: .lines), "tail": model.panel.headerNextValue(header: .tailLines),
                    "bar": model.panel.headerNextValue(header: .bar), "lock": model.panel.headerNextValue(header: .lock),
                ],
            ]
            let name = args.first ?? "marks-rows"
            let out = URL(fileURLWithPath: ProcessInfo.processInfo.environment["VIXEN_UI_TEST_OUT"] ?? NSTemporaryDirectory()).appending(path: "\(name).json")
            if let data = try? JSONSerialization.data(withJSONObject: state, options: [.prettyPrinted, .sortedKeys]) {
                try? data.write(to: out)
            }
            return model.rows.map { "\($0.name)\($0.isDefault ? "*" : "") [\($0.markCount)]" }.joined(separator: ", ")
        }
        UITestHarness.hooks["marksColor"] = { args in
            guard let hex = args.first, let argb = UInt32(hex, radix: 16) else { return "FAIL needs an ARGB hex" }
            ColorPanelSession.shared.testSetColor(argb)
            ColorPanelSession.shared.finish()
            return "color \(hex)"
        }
        UITestHarness.hooks["marksSave"] = { args in
            guard let session, let path = args.first else { return "FAIL needs an open sequence and a path" }
            do {
                try session.save(path: path)
                return "saved \(path)"
            } catch {
                return "FAIL \(error.userMessage)"
            }
        }
        UITestHarness.hooks["marksTimes"] = { args in
            guard let model = current, args.count >= 2 else { return "FAIL needs a collection name and an output name" }
            let wanted = args[0]
            guard let row = model.rows.first(where: { $0.name == wanted }) else { return "FAIL no collection \(wanted)" }
            let times = model.panel.markTimes(id: row.id)
            let out = URL(fileURLWithPath: ProcessInfo.processInfo.environment["VIXEN_UI_TEST_OUT"] ?? NSTemporaryDirectory()).appending(path: "\(args[1]).json")
            if let data = try? JSONSerialization.data(withJSONObject: ["name": wanted, "times": times]) {
                try? data.write(to: out)
            }
            return "\(times.count) marks"
        }
        UITestHarness.hooks["marksBeats"] = { _ in
            guard let beats = BeatsTestHooks.current else { return "FAIL no beats sheet" }
            let d = beats.detector.dialog()
            return "\(beats.detector.job()) bpm \(d.bpmText) bar \(d.barPeriodText) div \(d.divTimeText) \(d.beatsPerBar)/\(d.noteSize)"
        }
        UITestHarness.hooks["marksUndo"] = { _ in
            guard let session else { return "FAIL no sequence" }
            return "undid \(session.undo(steps: 1))"
        }
    }
}
