import AppKit
import SwiftUI
import VixenBridge
import VixenKit

// Edit ▸ Place Effect on Marks… and Add Onset Marks: the GUI over the effects_on_marks and sequence_onset_marks commands,
// run through the window's own document (`studioCommand`) so each is one step on its Undo, exactly as the MCP tools do.

extension SequencerModel {
    func openPlaceOnMarks() {
        sheet = .placeOnMarks
    }

    /// sequence_onset_marks on the sequence's audio: "Onsets - low", "- mid" and "- high", each mark's text its strength.
    func addOnsetMarks() {
        guard let session else { return }
        Task {
            let outcome = await Task.detached { () -> Result<String, Error> in
                Result { try session.studioCommand(command: "sequence_onset_marks", argumentsJson: "{}") }
            }.value
            refresh()
            if case .failure(let error) = outcome {
                await SequencerAlerts.tell(title: "Add Onset Marks", message: placeOnMarksMessage(error), style: .warning, window: window)
            }
        }
    }

    /// The selected rows' elements, or the active row's, as Place Effect on Marks targets them.
    var placeOnMarksTargets: [(id: String, name: String)] {
        #if canImport(VixenTimeline)
        return (canvas?.controller.studioSelectedTargets() ?? []).map { ($0.node, $0.name) }
        #else
        return []
        #endif
    }
}

/// The command's own message, without the FFI wrapper.
func placeOnMarksMessage(_ error: Error) -> String {
    if case FfiError.Failed(let message) = error { return message }
    return error.localizedDescription
}

struct PlaceOnMarksSheet: View {
    let model: SequencerModel

    private struct Collection: Identifiable, Hashable {
        let id: String
        let name: String
        let marks: Int
    }

    private enum Snap: String, CaseIterable, Identifiable {
        case none = "Off", beat = "Beat", eighth = "8th", sixteenth = "16th"
        var id: String { rawValue }
        var argument: String? {
            switch self {
            case .none: nil
            case .beat: "beat"
            case .eighth: "8th"
            case .sixteenth: "16th"
            }
        }
    }

    @State private var collections: [Collection] = []
    @State private var lengthMs: Double = 0
    @State private var targets: [(id: String, name: String)] = []
    @State private var collection = ""
    @State private var limitRange = false
    @State private var startSeconds = 0.0
    @State private var endSeconds = 0.0
    @State private var minStrength = 0.0
    @State private var snap = Snap.none
    @State private var grid = ""
    @State private var typeId = ""
    @State private var rotateTargets = false
    @State private var untilNext = true
    @State private var gapPercent = 80.0
    @State private var fixedMs = 250.0
    @State private var capDuration = false
    @State private var maxMs = 1000.0
    @State private var levelFromStrength = false
    @State private var levelLow = 0.0
    @State private var levelHigh = 100.0
    @State private var cycleColors = false
    @State private var colors: [Color] = [.red, .green, .blue]
    /// The dry run's outcome: what the button will place, or why it can't.
    private enum Summary {
        case places(String)
        case problem(String)
    }

    @State private var summary: Summary?
    @State private var placing = false

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Place Effect on Marks")
                .font(.headline)
            if collections.isEmpty {
                ContentUnavailableView(
                    "No Marks",
                    systemImage: "metronome",
                    description: Text("Add marks first: Audio ▸ Detect Beats and Bars… or Add Onset Marks, or create a mark collection in the Marks panel.")
                )
                .frame(minHeight: 160)
            } else if targets.isEmpty {
                ContentUnavailableView(
                    "No Rows Selected",
                    systemImage: "rectangle.stack",
                    description: Text("Select the rows to put effects on in the timeline, then choose Edit ▸ Place Effect on Marks… again.")
                )
                .frame(minHeight: 160)
            } else {
                form
                summaryLine
            }
            HStack {
                Spacer()
                Button("Cancel") { model.sheet = nil }
                    .keyboardShortcut(.cancelAction)
                Button("Place Effects") { place() }
                    .keyboardShortcut(.defaultAction)
                    .disabled(!canPlace)
            }
        }
        .padding(20)
        .frame(width: 460)
        .task { load() }
        .task(id: arguments(dryRun: true)) { await preview() }
    }

    private var form: some View {
        Form {
            Section {
                Picker("Marks", selection: $collection) {
                    ForEach(collections) { c in
                        Text("\(c.name) (\(c.marks))").tag(c.id)
                    }
                }
                Toggle("Only between", isOn: $limitRange)
                if limitRange {
                    LabeledContent("Seconds") {
                        HStack {
                            TextField("Start", value: $startSeconds, format: .number.precision(.fractionLength(0...3)))
                                .labelsHidden()
                            Text("to")
                            TextField("End", value: $endSeconds, format: .number.precision(.fractionLength(0...3)))
                                .labelsHidden()
                        }
                        .frame(width: 180)
                    }
                }
                LabeledContent("Minimum strength") {
                    HStack {
                        Slider(value: $minStrength, in: 0...1)
                        Text(minStrength, format: .number.precision(.fractionLength(2)))
                            .monospacedDigit()
                            .frame(width: 34, alignment: .trailing)
                    }
                    .frame(width: 200)
                }
                .help("A mark's strength is its text as a number from 0 to 1, as Add Onset Marks writes it. Marks without a number always count.")
                Picker("Snap to", selection: $snap) {
                    ForEach(Snap.allCases) { Text($0.rawValue).tag($0) }
                }
                .help("Moves each mark to the nearest beat, 8th or 16th of a beat collection. Marks that meet keep the strongest.")
                if snap != .none {
                    Picker("Beats", selection: $grid) {
                        ForEach(collections) { c in
                            Text(c.name).tag(c.name)
                        }
                    }
                }
            }
            Section {
                Picker("Effect", selection: $typeId) {
                    ForEach(model.effects, id: \.typeId) { effect in
                        Text(effect.name).tag(effect.typeId)
                    }
                }
                LabeledContent("Rows", value: targets.map(\.name).joined(separator: ", "))
                    .lineLimit(2)
                if targets.count > 1 {
                    Toggle("One row per mark, in turn", isOn: $rotateTargets)
                }
            }
            Section {
                Picker("Length", selection: $untilNext) {
                    Text("Until the next mark").tag(true)
                    Text("Fixed").tag(false)
                }
                if untilNext {
                    LabeledContent("Fill") {
                        HStack {
                            Slider(value: $gapPercent, in: 5...100, step: 5)
                            Text("\(Int(gapPercent))%")
                                .monospacedDigit()
                                .frame(width: 40, alignment: .trailing)
                        }
                        .frame(width: 200)
                    }
                    .help("How much of the time to the next mark each effect lasts, so fast passages keep their gaps.")
                } else {
                    LabeledContent("Milliseconds") {
                        TextField("", value: $fixedMs, format: .number.grouping(.never))
                            .labelsHidden()
                            .frame(width: 80)
                    }
                }
                Toggle("At most", isOn: $capDuration)
                if capDuration {
                    LabeledContent("Milliseconds") {
                        TextField("", value: $maxMs, format: .number.grouping(.never))
                            .labelsHidden()
                            .frame(width: 80)
                    }
                }
            }
            Section {
                Toggle("Level from strength", isOn: $levelFromStrength)
                    .help("Scales the effect's level by each mark's strength, between the two percentages.")
                if levelFromStrength {
                    LabeledContent("Percent") {
                        HStack {
                            TextField("Weakest", value: $levelLow, format: .number.grouping(.never))
                                .labelsHidden()
                            Text("to")
                            TextField("Strongest", value: $levelHigh, format: .number.grouping(.never))
                                .labelsHidden()
                        }
                        .frame(width: 140)
                    }
                }
                Toggle("Cycle colours", isOn: $cycleColors)
                    .help("Each mark's effect is solid in the next colour, in turn.")
                if cycleColors {
                    HStack {
                        ForEach(colors.indices, id: \.self) { i in
                            ColorPicker("Colour \(i + 1)", selection: $colors[i], supportsOpacity: false)
                                .labelsHidden()
                        }
                        Spacer()
                        Button { colors.append(.white) } label: { Image(systemName: "plus") }
                            .disabled(colors.count >= 16)
                            .help("Add a colour")
                        Button { colors.removeLast() } label: { Image(systemName: "minus") }
                            .disabled(colors.count <= 1)
                            .help("Remove the last colour")
                    }
                    .buttonStyle(.borderless)
                }
            }
        }
        .formStyle(.grouped)
        .scrollContentBackground(.hidden)
        .frame(height: 520)
    }

    @ViewBuilder private var summaryLine: some View {
        switch summary {
        case .places(let text):
            Label(text, systemImage: "checkmark.circle")
                .foregroundStyle(.secondary)
        case .problem(let text):
            Label(text, systemImage: "exclamationmark.triangle")
                .foregroundStyle(.orange)
                .fixedSize(horizontal: false, vertical: true)
        case nil:
            Text(" ")
        }
    }

    private var canPlace: Bool {
        if case .places = summary, !placing { return true }
        return false
    }

    private func load() {
        guard let session = model.session else { return }
        targets = model.placeOnMarksTargets
        if let data = try? session.studioCommand(command: "sequence_marks", argumentsJson: "{}").data(using: .utf8),
           let list = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]] {
            collections = list.compactMap { c in
                guard let id = c["id"] as? String, let name = c["name"] as? String else { return nil }
                return Collection(id: id, name: name, marks: (c["marks"] as? [Any])?.count ?? 0)
            }
        }
        if let data = try? session.studioCommand(command: "sequence_status", argumentsJson: "{}").data(using: .utf8),
           let status = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
            lengthMs = status["lengthMs"] as? Double ?? 0
        }
        endSeconds = lengthMs / 1000
        // Onsets first, as they carry strengths; then whatever has marks.
        let preferred = collections.first { $0.name.hasPrefix("Onsets") && $0.marks > 0 } ?? collections.first { $0.marks > 0 } ?? collections.first
        collection = preferred?.id ?? ""
        grid = collections.first { $0.name == "Music - All" }?.name ?? collections.first?.name ?? ""
        typeId = model.effects.first { $0.name == "Pulse" }?.typeId ?? model.effects.first?.typeId ?? ""
    }

    /// effects_on_marks' arguments; nil until there is something to place.
    private func arguments(dryRun: Bool) -> String? {
        guard !collection.isEmpty, !typeId.isEmpty, !targets.isEmpty else { return nil }
        var template: [String: Any] = ["typeId": typeId, "targetIds": targets.map(\.id)]
        if rotateTargets && targets.count > 1 { template["rotateTargets"] = true }
        var args: [String: Any] = ["markCollection": collection, "template": template, "minStrength": minStrength]
        if limitRange { args["range"] = [max(0, startSeconds) * 1000, max(0, endSeconds) * 1000] }
        if let snap = snap.argument {
            args["snap"] = snap
            if !grid.isEmpty { args["gridCollection"] = grid }
        }
        if untilNext {
            args["duration"] = "untilNext"
            args["gapFraction"] = gapPercent / 100
        } else {
            args["duration"] = fixedMs
        }
        if capDuration { args["maxMs"] = maxMs }
        if levelFromStrength {
            args["levelFromStrength"] = true
            args["levelRange"] = [levelLow, levelHigh]
        }
        if cycleColors { args["cycleColors"] = colors.map(hex) }
        if dryRun { args["dryRun"] = true }
        guard let data = try? JSONSerialization.data(withJSONObject: args, options: [.sortedKeys]) else { return nil }
        return String(data: data, encoding: .utf8)
    }

    private func hex(_ color: Color) -> String {
        let c = NSColor(color).usingColorSpace(.sRGB) ?? .white
        let byte = { (v: CGFloat) in Int((min(1, max(0, v)) * 255).rounded()) }
        return String(format: "#%02x%02x%02x", byte(c.redComponent), byte(c.greenComponent), byte(c.blueComponent))
    }

    /// The dry run's counts, as the button will place them.
    private func preview() async {
        guard let session = model.session, let json = arguments(dryRun: true) else {
            summary = nil
            return
        }
        try? await Task.sleep(for: .milliseconds(150))
        guard !Task.isCancelled else { return }
        let outcome = await Task.detached { () -> Result<String, Error> in
            Result { try session.studioCommand(command: "effects_on_marks", argumentsJson: json) }
        }.value
        guard !Task.isCancelled else { return }
        switch outcome {
        case .success(let text):
            summary = .places(Self.describe(text))
        case .failure(let error):
            summary = .problem(placeOnMarksMessage(error))
        }
    }

    private static func describe(_ json: String) -> String {
        guard let data = json.data(using: .utf8), let r = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return json }
        let effects = r["effects"] as? Int ?? 0
        let marks = r["marksUsed"] as? Int ?? 0
        var text = "Places \(effects) \(effects == 1 ? "effect" : "effects") on \(marks) \(marks == 1 ? "mark" : "marks")"
        let skipped = r["skipped"] as? [String: Int] ?? [:]
        let notes = [("belowMinStrength", "below minimum strength"), ("merged", "merged by snapping"), ("tooShort", "too short")]
            .compactMap { key, label in (skipped[key] ?? 0) > 0 ? "\(skipped[key]!) \(label)" : nil }
        if !notes.isEmpty { text += " · skips " + notes.joined(separator: ", ") }
        return text
    }

    private func place() {
        guard let session = model.session, let json = arguments(dryRun: false) else { return }
        placing = true
        Task {
            let outcome = await Task.detached { () -> Result<String, Error> in
                Result { try session.studioCommand(command: "effects_on_marks", argumentsJson: json) }
            }.value
            placing = false
            switch outcome {
            case .success:
                model.sheet = nil
                model.refresh()
            case .failure(let error):
                summary = .problem(placeOnMarksMessage(error))
            }
        }
    }
}
