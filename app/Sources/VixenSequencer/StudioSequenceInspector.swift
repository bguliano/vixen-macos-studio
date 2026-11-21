import SwiftUI
import VixenBridge
import VixenKit

// The inspector when no effect is selected: the sequence itself (`sequence_overview`). Every edit is a Studio sequence
// command on the window's own document (`studioCommand`), so it is one step on the window's Undo, as the MCP tools are.

/// What `sequence_overview` returns.
struct StudioSequenceOverview: Equatable {
    struct Audio: Equatable {
        var song: String
        var title: String
        var missing: Bool
        var durationMs: Double?
    }
    struct Marks: Equatable, Identifiable {
        var id: String
        var name: String
        var count: Int
        var visible: Bool
    }
    struct Layer: Equatable, Identifiable {
        var id: String
        var name: String
        var filter: String
        var isDefault: Bool
        var effects: Int
    }
    var lengthMs: Double = 0
    var audio: Audio?
    var marks: [Marks] = []
    var layers: [Layer] = []
    var effects = 0
    var rendered = 0
    var renderPercent = 100
    var renderHelp = ""

    init() {}

    init?(json: String) {
        guard let o = (try? JSONSerialization.jsonObject(with: Data(json.utf8))) as? [String: Any] else { return nil }
        lengthMs = o["lengthMs"] as? Double ?? 0
        if let a = o["audio"] as? [String: Any], let song = a["song"] as? String {
            audio = Audio(song: song, title: a["title"] as? String ?? song, missing: a["missing"] as? Bool ?? false,
                          durationMs: a["durationMs"] as? Double)
        }
        marks = (o["marks"] as? [[String: Any]] ?? []).compactMap { c in
            guard let id = c["id"] as? String else { return nil }
            return Marks(id: id, name: c["name"] as? String ?? "", count: c["marks"] as? Int ?? 0, visible: c["visible"] as? Bool ?? false)
        }
        layers = (o["layers"] as? [[String: Any]] ?? []).compactMap { l in
            guard let id = l["id"] as? String else { return nil }
            return Layer(id: id, name: l["name"] as? String ?? "", filter: l["filter"] as? String ?? "",
                         isDefault: l["isDefault"] as? Bool ?? false, effects: l["effects"] as? Int ?? 0)
        }
        let render = o["render"] as? [String: Any] ?? [:]
        effects = render["effects"] as? Int ?? 0
        rendered = render["rendered"] as? Int ?? 0
        renderPercent = render["percent"] as? Int ?? 100
        renderHelp = render["help"] as? String ?? ""
    }
}

struct StudioSequenceInspector: View {
    let model: SequencerModel
    @State private var overview = StudioSequenceOverview()
    @State private var lengthText = ""
    @State private var problem: String?

    var body: some View {
        Form {
            Section("Sequence") {
                LabeledContent("Length") {
                    TextField("Length", text: $lengthText, prompt: Text("m:ss.fff"))
                        .labelsHidden()
                        .multilineTextAlignment(.trailing)
                        .frame(maxWidth: 110)
                        .onSubmit { setLength(text: lengthText) }
                        .help("The sequence's length, as m:ss.fff, m:ss or seconds. It can't be shorter than its last effect.")
                }
                if let problem {
                    Text(problem)
                        .font(.caption)
                        .foregroundStyle(.red)
                }
            }
            audioSection
            marksSection
            layersSection
            renderSection
        }
        .formStyle(.grouped)
        .controlSize(.small)
        .task(id: refreshKey) { reload() }
        .onAppear { registerHook() }
    }

    // MARK: Sections

    @ViewBuilder private var audioSection: some View {
        Section("Audio") {
            if let audio = overview.audio {
                LabeledContent("Song") {
                    Text(audio.title)
                        .lineLimit(2)
                        .multilineTextAlignment(.trailing)
                        .textSelection(.enabled)
                }
                if audio.missing {
                    Label("The Media folder doesn't have \(audio.song), so the sequence plays silent.", systemImage: "exclamationmark.triangle.fill")
                        .font(.caption)
                        .foregroundStyle(.orange)
                } else if let duration = audio.durationMs {
                    LabeledContent("Song Length", value: formatTime(ms: duration))
                    if abs(duration - overview.lengthMs) >= 1 {
                        Button("Fit Length to Song") { setLength(ms: duration) }
                            .help("Sets the sequence's length to the song's, \(formatTime(ms: duration)).")
                    }
                }
            } else {
                Text("No audio. Add a song to see its waveform and play along with it.")
                    .foregroundStyle(.secondary)
            }
            HStack {
                Button(overview.audio == nil ? "Add Audio…" : "Change Audio…") { model.studioChangeAudio() }
                    .help("Chooses the sequence's song. Undo puts the previous one back.")
                if overview.audio != nil {
                    Button("Remove Audio") { model.studioRemoveAudio() }
                        .help("Removes the song from the sequence (the file stays in Media). Undo brings it back.")
                }
            }
        }
    }

    @ViewBuilder private var marksSection: some View {
        Section {
            if overview.marks.isEmpty {
                Text("No mark collections. Add Onset Marks or Detect Beats and Bars from the Audio menu.")
                    .foregroundStyle(.secondary)
            }
            ForEach(overview.marks) { collection in
                Toggle(isOn: Binding(get: { collection.visible }, set: { setVisible(collection, $0) })) {
                    HStack {
                        Text(collection.name)
                        Spacer()
                        Text("\(collection.count)")
                            .monospacedDigit()
                            .foregroundStyle(.secondary)
                    }
                }
                .help(collection.visible ? "Hide \(collection.name)'s marks on the timeline" : "Show \(collection.name)'s marks on the timeline")
            }
        } header: {
            Text("Marks")
        } footer: {
            if !overview.marks.isEmpty {
                Text("The switch shows a collection's lines and bar on the timeline.")
            }
        }
    }

    @ViewBuilder private var layersSection: some View {
        Section("Layers") {
            ForEach(overview.layers) { layer in
                StudioLayerRow(layer: layer) { rename(layer, to: $0) }
            }
            Button("Add Layer") { addLayer() }
                .help("Adds a layer above the others, mixing by highest value.")
        }
    }

    @ViewBuilder private var renderSection: some View {
        Section("Rendering") {
            if overview.effects == 0 {
                Text(overview.renderHelp)
                    .foregroundStyle(.secondary)
            } else {
                ProgressView(value: Double(overview.renderPercent), total: 100) {
                    Text(overview.rendered < overview.effects
                         ? "\(overview.rendered) of \(overview.effects) effects rendered"
                         : "All \(overview.effects) effects rendered")
                }
                .help(overview.renderHelp)
            }
        }
    }

    // MARK: Reading and commands

    /// Reread after any edit (ours, the timeline's or an agent's) and as renders finish.
    private var refreshKey: [UInt64] {
        [model.editRevision, UInt64(model.studioChrome?.rendered ?? 0), UInt64(model.studioChrome?.effects ?? 0)]
    }

    private func reload() {
        guard let text = try? model.session?.studioCommand(command: "sequence_overview", argumentsJson: "{}"),
              let read = StudioSequenceOverview(json: text) else { return }
        if read.lengthMs != overview.lengthMs || lengthText.isEmpty {
            lengthText = formatTime(ms: read.lengthMs)
        }
        if read != overview {
            overview = read
        }
    }

    private func run(_ command: String, _ arguments: [String: Any]) {
        guard let session = model.session,
              let data = try? JSONSerialization.data(withJSONObject: arguments),
              let json = String(data: data, encoding: .utf8) else { return }
        do {
            _ = try session.studioCommand(command: command, argumentsJson: json)
            problem = nil
        } catch {
            problem = placeOnMarksMessage(error)
        }
        model.refresh()
        reload()
    }

    private func setLength(text: String) {
        run("sequence_length", ["length": text])
        lengthText = formatTime(ms: overview.lengthMs)
    }

    private func setLength(ms: Double) {
        run("sequence_length", ["lengthMs": ms])
    }

    private func setVisible(_ collection: StudioSequenceOverview.Marks, _ visible: Bool) {
        run("marks_visibility", ["collectionId": collection.id, "visible": visible])
    }

    private func rename(_ layer: StudioSequenceOverview.Layer, to name: String) {
        let trimmed = name.trimmingCharacters(in: .whitespaces)
        guard !trimmed.isEmpty, trimmed != layer.name else { return }
        run("layer_rename", ["layerId": layer.id, "name": trimmed])
    }

    private func addLayer() {
        let names = Set(overview.layers.map(\.name))
        let name = (1...).lazy.map { $0 == 1 ? "Layer" : "Layer \($0)" }.first { !names.contains($0) } ?? "Layer"
        run("layer_create", ["name": name])
    }

    /// `sequencer.inspector`: what the empty inspector shows, and `sequencer.inspector length|visible|audio …` to edit
    /// through the same paths as its controls.
    private func registerHook() {
        guard UITestHarness.isActive else { return }
        UITestHarness.hooks["sequencer.inspector"] = { [model] args in
            guard let session = model.session else { return "FAIL no sequencer" }
            switch args.first {
            case "audio" where args == ["audio", "remove"]:
                model.studioRemoveAudio()
            case "audio" where args == ["audio", "choose"]:
                // Change Audio… itself: under test its open panel answers with VIXEN_TEST_FILE.
                model.studioChangeAudio()
            case "audio" where args.count == 2:
                // Change Audio… with the file the open panel would have chosen, through the same path.
                await model.studioSetAudio(URL(fileURLWithPath: args[1]))
            case "length" where args.count == 2:
                _ = try? session.studioCommand(command: "sequence_length", argumentsJson: "{\"length\":\"\(args[1])\"}")
            case "visible" where args.count == 3:
                let text = (try? session.studioCommand(command: "sequence_overview", argumentsJson: "{}")) ?? ""
                guard let id = StudioSequenceOverview(json: text)?.marks.first(where: { $0.name == args[1] })?.id else { return "FAIL no collection \(args[1])" }
                _ = try? session.studioCommand(command: "marks_visibility", argumentsJson: "{\"collectionId\":\"\(id)\",\"visible\":\(args[2] == "on")}")
            case nil:
                break
            default:
                return "FAIL unknown arguments"
            }
            model.refresh()
            let state = (try? session.studioCommand(command: "sequence_overview", argumentsJson: "{}")) ?? "FAIL overview"
            return "{\"editorSelection\":\(model.editorSelection.count),\"overview\":\(state)}"
        }
    }
}

/// A layer's name, renamed in place when Return is pressed or the field loses focus.
private struct StudioLayerRow: View {
    let layer: StudioSequenceOverview.Layer
    let rename: (String) -> Void
    @State private var name = ""
    @FocusState private var focused: Bool

    var body: some View {
        LabeledContent {
            Text(layer.effects == 1 ? "1 effect" : "\(layer.effects) effects")
                .monospacedDigit()
                .foregroundStyle(.secondary)
        } label: {
            if layer.isDefault {
                Text(layer.name)
                    .help("The default layer. Effects go here unless assigned to another layer.")
            } else {
                TextField("Layer name", text: $name)
                    .labelsHidden()
                    .focused($focused)
                    .onSubmit { rename(name) }
                    .onChange(of: focused) { _, now in if !now { rename(name) } }
                    .help("\(layer.filter). Rename it here.")
            }
        }
        .onAppear { name = layer.name }
        .onChange(of: layer.name) { _, new in name = new }
    }
}
