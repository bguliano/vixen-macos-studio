import AppKit
import SwiftUI
import VixenBridge
import VixenKit

/// A window with just the Effect Editor, for building and testing it alone. The app includes this when `VixenSequencer` isn't in the build. It lists the profile's sequences and a sequence's effects, and the
/// menu picks effects by type, so a test script can open every kind. Under the test harness, `VIXEN_EE_SEQUENCE` names a file in the profile's Sequence folder to open.
public struct EffectEditorTestScene: Scene {
    private let model: AppModel

    public init(model: AppModel) {
        self.model = model
    }

    public var body: some Scene {
        Window("Effect Editor Test", id: "effect-editor-test") {
            EffectEditorTestView()
                .environment(model)
                .studioAppearance() // STUDIO: follow the chosen appearance
        }
        .defaultSize(width: 980, height: 980)
        .commands {
            CommandMenu("Effect Editor Test") {
                Menu("Select") {
                    ForEach(EffectEditorTestState.effectNames, id: \.self) { name in
                        Button(name) { EffectEditorTestState.shared.select(type: name) }
                    }
                }
                Menu("Add To Selection") {
                    ForEach(EffectEditorTestState.effectNames, id: \.self) { name in
                        Button(name) { EffectEditorTestState.shared.add(type: name) }
                    }
                }
                Button("Add Next Of Same Type") { EffectEditorTestState.shared.addNextOfSameType() }
                Button("Clear Selection") { EffectEditorTestState.shared.clear() }
                Button("Select 76") { EffectEditorTestState.shared.selectMany(76) }
                Divider()
                Button("Dump Panel") { EffectEditorTestState.shared.dump() }
                Button("Cycle Choice 1") { EffectEditorTestState.shared.cycleChoice(0) }
                Button("Cycle Choice 2") { EffectEditorTestState.shared.cycleChoice(1) }
                Button("Cycle Choice 3") { EffectEditorTestState.shared.cycleChoice(2) }
                Button("Cycle Choice 4") { EffectEditorTestState.shared.cycleChoice(3) }
                Button("Toggle Preview") { EffectEditorTestState.shared.togglePreview() }
                Button("Play Sequence") { EffectEditorTestState.shared.playSequence() }
                Button("Stop Sequence") { EffectEditorTestState.shared.stopSequence() }
                Button("Note Preview") { EffectEditorTestState.shared.notePreview() }
                Button("Save As Default") { EffectEditorTestState.shared.editor?.saveAsDefault() }
                Button("Reset Default") { EffectEditorTestState.shared.editor?.resetDefault() }
                Button("Undo") { EffectEditorTestState.shared.undo() }
                Button("Redo") { EffectEditorTestState.shared.redo() }
                Divider()
                Button("Tall Window") { EffectEditorTestState.shared.setHeight(1700) }
                Button("Normal Window") { EffectEditorTestState.shared.setHeight(980) }
                Divider()
                Button("Colour Panel: Choose Red") {
                    ColorPanelSession.shared.testSetColor(0xFFFF_0000)
                    ColorPanelSession.shared.finish()
                }
                Button("Font Panel: Choose Helvetica Bold 14") {
                    FontPanelSession.shared.testSetFont(KitFont(name: "Helvetica", size: 14, bold: true, italic: false))
                    FontPanelSession.shared.finish()
                }
            }
        }
    }
}

/// What the test menu acts on.
@MainActor
@Observable
final class EffectEditorTestState {
    static let shared = EffectEditorTestState()
    /// Every effect module, from the build's table.
    static let effectNames: [String] = allEffectNames()

    var session: SequencerSession?
    var editor: EffectEditorModel?
    var effects: [EeEffectItem] = []
    var selection: [UInt64] = []
    var window: NSWindow?
    var api: EffectEditorApi?

    func open(_ path: String, core: VixenCore) {
        let session: SequencerSession
        do {
            session = try core.openSequence(path: path)
        } catch {
            UITestHarness.note("Couldn't open \(path): \(error)")
            return
        }
        self.session = session
        let editor = EffectEditorModel(session: session)
        self.editor = editor
        // The panel's own: each `effectEditor()` is a separate panel, with its own preview.
        let api = editor.api
        self.api = api
        effects = api.effectList()
        selection = []
    }

    func select(type: String) {
        guard let api, let id = api.testEffectOfType(name: type) else { return }
        effects = api.effectList()
        selection = [id]
        editor?.setSelectionNow(selection)
    }

    func add(type: String) {
        guard let api, let id = api.testEffectOfType(name: type) else { return }
        effects = api.effectList()
        selection.append(id)
        editor?.setSelectionNow(selection)
    }

    func addNextOfSameType() {
        guard let api, let first = selection.first, let id = api.testNextOfSameType(effect: first) else { return }
        selection.append(id)
        editor?.setSelectionNow(selection)
    }

    func clear() {
        selection = []
        editor?.setSelectionNow([])
    }

    func selectMany(_ count: Int) {
        selection = Array(effects.prefix(count).map(\.id))
        editor?.setSelectionNow(selection)
    }

    func undo() {
        _ = session?.undo(steps: 1)
        editor?.reload()
    }

    func redo() {
        _ = session?.redo(steps: 1)
        editor?.reload()
    }

    func setHeight(_ height: CGFloat) {
        guard let window else { return }
        var frame = window.frame
        frame.size.height = height
        window.setFrame(frame, display: true)
    }

    /// Moves the nth drop-down row in the panel to its next item: a system menu can't be driven from a script.
    func cycleChoice(_ n: Int) {
        guard let api, let editor else { return }
        let panel = api.panel(effects: selection)
        var found: [(EeRowKey, Int, Int)] = []
        for row in panel.categories.flatMap(\.rows) {
            if case let .choice(labels, selected) = row.control, !labels.isEmpty {
                found.append((row.key, Int(selected ?? 0), labels.count))
            }
        }
        guard n < found.count else { return }
        let (key, current, count) = found[n]
        editor.commit(key, .choice(index: UInt32((current + 1) % count)))
    }

    func togglePreview() {
        guard let editor else { return }
        editor.previewOn.toggle()
        notePreview()
    }

    /// Real playback, as the sequencer's Play does: it takes the engine from the preview.
    func playSequence() {
        guard let session else { return }
        session.play(startMs: 0, endMs: session.lengthMs(), looping: false)
        notePreview()
    }

    func stopSequence() {
        session?.stop()
        notePreview()
    }

    func notePreview() {
        guard let editor, let api else { return }
        UITestHarness.note("preview on \(editor.previewOn), running \(api.previewIsRunning()), transport \(String(describing: session?.transport()))")
    }

    /// Writes the panel's rows and each selected effect's render to `panel.txt` in the output folder, so a test can read back Rust's state.
    func dump() {
        guard let api else { return }
        let panel = api.panel(effects: selection)
        var lines = ["effect \(panel.effectName) count \(panel.effectCount)", "information \(panel.information)"]
        func describe(_ rows: [EeRow], indent: String) {
            for row in rows {
                switch row.control {
                case let .recordList(style, items):
                    lines.append("\(indent)\(row.label) [\(row.key.param)] list items=\(items.count) min=\(style.minItems)")
                    for item in items {
                        lines.append("\(indent)  -- \(item.header)")
                        describe(item.rows, indent: indent + "    ")
                    }
                default:
                    lines.append("\(indent)\(row.label) [\(row.key.param)] \(summary(row.control))")
                }
            }
        }
        for category in panel.categories {
            lines.append("# \(category.name)")
            describe(category.rows, indent: "  ")
        }
        for id in selection {
            lines.append("render \(id): \(api.renderSummary(effect: id))")
        }
        lines.append("preview running \(api.previewIsRunning()) position \(session?.positionMs() ?? 0)")
        lines.append("undo: \(session?.undoDescriptions().first ?? "")")
        let out = ProcessInfo.processInfo.environment["VIXEN_UI_TEST_OUT"] ?? NSTemporaryDirectory()
        let url = URL(fileURLWithPath: out).appending(path: "panel.txt")
        let previous = (try? String(contentsOf: url, encoding: .utf8)) ?? ""
        try? (previous + lines.joined(separator: "\n") + "\n\n").write(to: url, atomically: true, encoding: .utf8)
    }

    private func summary(_ control: EeRowControl) -> String {
        switch control {
        case let .check(value): "check \(value)"
        case let .choice(labels, selected): "choice \(selected.map { labels[Int($0)] } ?? "(blank)") of \(labels.count)"
        case let .number(text, limits): "number \(text) \(limits.min)...\(limits.max) step \(limits.step)"
        case let .slider(value, min, max, tick, snap, tip): "slider \(value) \(min)...\(max) tick \(tick) snap \(snap) tip \(tip)"
        case let .text(value): "text \(value)"
        case let .label(value): "label \(value)"
        case let .path(value, _, filter, _): "path \(value) filter \(filter)"
        case let .font(text, _, _, _, _): "font \(text)"
        case let .color(_, text, valid): "color \(text) valid \(valid.count)"
        case let .curve(value): "curve \(value.points.map { "(\($0.x),\($0.y))" }.joined()) link '\(value.libraryName)'"
        case let .gradient(value, discrete, _): "gradient stops \(value.stops.map { String(format: "%.2f:%08X", $0.position, $0.argb) }) discrete \(discrete) link '\(value.libraryName)'"
        case let .gradientList(items, discrete, _, min):
            "gradient list \(items.count) discrete \(discrete) min \(min) colors \(items.map { $0.stops.map { String(format: "%08X", $0.argb) }.joined(separator: "+") })"
        case let .colorList(items, _, min): "color list \(items.count) min \(min)"
        case let .gradientLevels(items, discrete, _, min): "levels \(items.count) discrete \(discrete) min \(min)"
        case let .stringList(items, min): "strings \(items) min \(min)"
        case .recordList: "records"
        case .polygons: "polygons"
        }
    }
}

private struct EffectEditorTestView: View {
    @Environment(AppModel.self) private var app
    private let state = EffectEditorTestState.shared
    @State private var sequences: [String] = []
    @State private var tableSelection = Set<UInt64>()
    @State private var hostWindow: NSWindow?

    var body: some View {
        HSplitView {
            VStack(spacing: 6) {
                Table(sequences, selection: Binding<String?>(get: { nil }, set: { name in
                    if let name, let core = app.core, let root = app.dataRoot {
                        state.open(root + "/Sequence/" + name, core: core)
                    }
                })) {
                    TableColumn("Sequence") { Text($0) }
                }
                .frame(height: 120)
                Table(state.effects, selection: $tableSelection) {
                    TableColumn("Effect", value: \.name)
                    TableColumn("Target", value: \.target)
                    TableColumn("Start") { Text(formatTime(ms: $0.startMs)).monospacedDigit() }
                }
                .onChange(of: tableSelection) { _, chosen in
                    let ordered = state.effects.filter { chosen.contains($0.id) }.map(\.id)
                    state.selection = ordered
                    state.editor?.selectionChanged(ordered)
                }
                .frame(maxHeight: .infinity)
            }
            .frame(minWidth: 320, idealWidth: 420, maxHeight: .infinity)
            Group {
                if let editor = state.editor {
                    EffectEditorPane(model: editor, selectedEffects: state.selection)
                        .id(ObjectIdentifier(editor))
                } else {
                    ContentUnavailableView("Open a sequence", systemImage: "slider.horizontal.3")
                }
            }
            .frame(minWidth: 380, idealWidth: 520, maxHeight: .infinity)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(WindowFinder(window: Binding(get: { state.window }, set: { state.window = $0 })))
        .task(id: app.core != nil) {
            if !app.startsElsewhere {
                await app.start()
            }
            guard let root = app.dataRoot, app.core != nil else { return }
            let folder = root + "/Sequence"
            let names = (try? FileManager.default.contentsOfDirectory(atPath: folder)) ?? []
            sequences = names.filter { $0.hasSuffix(".tim") }.sorted()
            if state.session == nil, let wanted = ProcessInfo.processInfo.environment["VIXEN_EE_SEQUENCE"], let core = app.core {
                state.open(folder + "/" + wanted, core: core)
            }
        }
    }
}

extension String: @retroactive Identifiable {
    public var id: String { self }
}

extension EeEffectItem: Identifiable {}
