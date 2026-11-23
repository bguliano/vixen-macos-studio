import SwiftUI
import VixenBridge
import VixenKit

/// Add ▸ Single Bulbs… in the Display pane (B60): smart bulbs, lamps and ceiling lights that are one light each, made in one
/// step. Each name becomes a Single Item with the chosen colour handling; a controller patches them in order (an RGB bulb takes
/// three outputs), and a preview shows each as a Single Light linked to it. Rust does all of it (`studio_single_bulbs.rs`), the
/// MCP `element_single_bulbs` tool, as one step on the shared display history.
struct StudioSingleBulbsSheet: View {
    let model: SetupModel
    let onFinish: () -> Void

    /// The last choices, for the app's life, as the templates remember theirs.
    @MainActor private static var remembered = (names: "", color: "rgb", controller: "", start: 1, preview: "")
    /// Under test: a controller and preview to choose by name when the sheet opens (`studioSingleBulbsFill`).
    @MainActor private static var testChoices: (controller: String, preview: String)?

    /// `studioSingleBulbsFill` with `text` "names separated by |", `name` a controller and `label` a preview (by name): the next
    /// sheet opens with them filled in, so a script can press Add Bulbs.
    static func installTestHook() {
        guard UITestHarness.isActive, UITestHarness.customActions["studioSingleBulbsFill"] == nil else { return }
        UITestHarness.customActions["studioSingleBulbsFill"] = { step in
            remembered.names = (step.text ?? "").split(separator: "|").joined(separator: "\n")
            testChoices = (step.name ?? "", step.label ?? "")
            return (true, nil)
        }
    }

    @State private var names = StudioSingleBulbsSheet.remembered.names
    @State private var color = StudioSingleBulbsSheet.remembered.color
    @State private var controller = StudioSingleBulbsSheet.remembered.controller
    @State private var start = StudioSingleBulbsSheet.remembered.start
    @State private var preview = StudioSingleBulbsSheet.remembered.preview
    @State private var controllers: [StudioControllerOverview.Row] = []
    @State private var previews: [StudioTemplatePreview.Choice] = []

    private var parsed: [String] { studioSingleBulbNames(text: names) }
    private var perBulb: Int { StudioTemplateColor.choices.first { $0.mode == color }?.channelsPerPixel ?? 3 }
    private var chosenController: StudioControllerOverview.Row? { controllers.first { $0.id == controller } }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Single Bulbs").font(.headline)
            Text("For lights that are one bulb each, such as smart bulbs, lamps and ceiling lights. Each name becomes its own element.")
                .font(.callout)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            VStack(alignment: .leading, spacing: 4) {
                Text("Names, one per line:")
                TextEditor(text: $names)
                    .font(.body)
                    .frame(minHeight: 110, maxHeight: 160)
                    .overlay(RoundedRectangle(cornerRadius: 4).stroke(.separator))
                    .accessibilityLabel("Names, one per line")
                Text(countText)
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
            StudioTemplateColorPicker(mode: $color)
            patchSection
            previewSection
            SheetButtons(okTitle: parsed.count == 1 ? "Add Bulb" : "Add Bulbs", okDisabled: parsed.isEmpty, ok: add, cancel: onFinish)
        }
        .padding(16)
        .frame(width: 460)
        .sheetKeys(okEnabled: !parsed.isEmpty, ok: add, cancel: onFinish)
        .task { await load() }
    }

    private var countText: String {
        switch parsed.count {
        case 0: "Type or paste a name for each bulb, such as Kitchen 1."
        case 1: "1 bulb"
        default: "\(parsed.count) bulbs"
        }
    }

    private var patchSection: some View {
        VStack(alignment: .leading, spacing: 4) {
            Picker("Patch to:", selection: $controller) {
                Text("Don't patch").tag("")
                if !controllers.isEmpty { Divider() }
                ForEach(controllers) { Text("\($0.name) (\($0.outputs) outputs)").tag($0.id) }
            }
            .fixedSize()
            .disabled(controllers.isEmpty)
            if let chosen = chosenController {
                HStack(spacing: 6) {
                    SheetFieldLabel("From output:")
                    SheetNumberField("From output:", value: $start, range: 1...max(1, chosen.outputs))
                }
            }
            Text(patchNote)
                .font(.callout)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    private var patchNote: String {
        guard let chosen = chosenController else {
            return controllers.isEmpty ? "Add a controller under Controllers to patch the bulbs as they're made." : "Patch them later from Patching, or choose a controller."
        }
        let first = start, size = perBulb
        let needed = parsed.count * size
        var text = parsed.count > 1
            ? "In order, from the first free output: \(parsed[0]) takes outputs \(first)–\(first + size - 1), \(parsed[1]) \(first + size)–\(first + 2 * size - 1), and so on."
            : "Takes \(size == 1 ? "1 output" : "\(size) outputs") from the first free one."
        if first - 1 + needed > chosen.outputs {
            text += " \(chosen.name) has \(chosen.outputs) outputs, too few for \(needed): the last bulbs stay unpatched."
        }
        return text
    }

    private var previewSection: some View {
        VStack(alignment: .leading, spacing: 4) {
            Picker("Show in Preview:", selection: $preview) {
                Text("None").tag("")
                if !previews.isEmpty { Divider() }
                ForEach(previews, id: \.id) { Text($0.name).tag($0.id) }
            }
            .fixedSize()
            .disabled(previews.isEmpty)
            Text(previews.isEmpty
                ? "There are no previews yet. Add one under Previews to show the bulbs in it."
                : preview.isEmpty
                    ? "Choose a preview to add a bulb for each, linked to its element."
                    : "Adds a linked Single Light for each, in a row. Move them in the preview to where the lights hang.")
                .font(.callout)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    private func load() async {
        guard let session = model.session, let core = model.app.core else { return }
        let (overview, choices) = await offMain {
            (core.studioControllerOverview(session: session), StudioTemplatePreview.load(from: session))
        }
        controllers = (try? JSONDecoder().decode(StudioControllerOverview.self, from: Data(overview.utf8)))?.controllers ?? []
        previews = choices
        if let test = Self.testChoices {
            Self.testChoices = nil
            controller = controllers.first { $0.name == test.controller }?.id ?? ""
            preview = previews.first { $0.name == test.preview }?.id ?? ""
        }
        if !controllers.contains(where: { $0.id == controller }) { controller = "" }
        if !previews.contains(where: { $0.id == preview }) { preview = "" }
    }

    private func add() {
        let list = parsed
        guard !list.isEmpty else { return }
        Self.remembered = (names, color, controller, start, preview)
        var arguments: [String: Any] = ["names": list, "color": color]
        if let chosen = chosenController {
            arguments["controllerId"] = chosen.id
            arguments["startOutput"] = max(0, start - 1)
        }
        if !preview.isEmpty {
            arguments["previewId"] = preview
        }
        StudioDisplayLive.shared.perform(command: "element_single_bulbs", arguments: arguments, model: model, app: model.app,
                                         label: list.count == 1 ? "Add Single Bulb" : "Add Single Bulbs")
        onFinish()
    }
}
