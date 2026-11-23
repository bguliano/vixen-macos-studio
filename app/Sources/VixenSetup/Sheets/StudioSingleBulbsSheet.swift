import SwiftUI
import VixenBridge
import VixenKit

/// Add ▸ Single Bulbs… in the Display pane (B60): smart bulbs, lamps and ceiling lights that are one light each, made in one
/// step. Each name becomes a Single Item with the chosen colour handling; a controller patches them in order (an RGB bulb takes
/// three outputs), and a preview shows each as a Single Light linked to it. Rust does all of it (`studio_single_bulbs.rs`), the
/// MCP `element_single_bulbs` tool, as one step on the shared display history. A controller that lists its lights (a plugin
/// controller such as Govee Output, through `controller.outputsFor`'s `lights`, or outputs named per light) offers Use the
/// Controller's Lights (B62): one bulb per unpatched light, named after it, each patched to its own outputs.
struct StudioSingleBulbsSheet: View {
    let model: SetupModel
    let onFinish: () -> Void

    /// The last choices, for the app's life, as the templates remember theirs.
    @MainActor private static var remembered = (names: "", color: "rgb", controller: "", start: 1, preview: "")
    /// Under test: a controller and preview to choose by name when the sheet opens (`studioSingleBulbsFill`).
    @MainActor private static var testChoices: (controller: String, preview: String)?
    /// Under test: press Use the Controller's Lights once its lights are known (`studioSingleBulbsFill` with `index` 1).
    @MainActor private static var testUseLights = false
    /// Opens the sheet from the Elements panel (`studioSingleBulbsOpen` under test).
    @MainActor static var open: (() -> Void)?

    /// `studioSingleBulbsFill` with `text` "names separated by |", `name` a controller and `label` a preview (by name): the next
    /// sheet opens with them filled in, so a script can press Add Bulbs; `index` 1 also uses the controller's lights.
    /// `studioSingleBulbsOpen` opens the sheet.
    static func installTestHook() {
        guard UITestHarness.isActive, UITestHarness.customActions["studioSingleBulbsFill"] == nil else { return }
        UITestHarness.customActions["studioSingleBulbsFill"] = { step in
            remembered.names = (step.text ?? "").split(separator: "|").joined(separator: "\n")
            testChoices = (step.name ?? "", step.label ?? "")
            testUseLights = step.index == 1
            return (true, nil)
        }
        UITestHarness.customActions["studioSingleBulbsOpen"] = { _ in
            guard let open else { return (false, "the Elements panel isn't showing") }
            open()
            return (true, nil)
        }
    }

    /// `controller_lights` for the chosen controller.
    struct Lights: Decodable {
        struct Light: Decodable {
            let name: String
            let firstOutput: Int
            let lastOutput: Int
            let outputCount: Int
            let patched: Bool
        }
        let lights: [Light]
        var unpatched: [Light] { lights.filter { !$0.patched } }
    }

    @State private var names = StudioSingleBulbsSheet.remembered.names
    @State private var color = StudioSingleBulbsSheet.remembered.color
    @State private var controller = StudioSingleBulbsSheet.remembered.controller
    @State private var start = StudioSingleBulbsSheet.remembered.start
    @State private var preview = StudioSingleBulbsSheet.remembered.preview
    @State private var controllers: [StudioControllerOverview.Row] = []
    @State private var previews: [StudioTemplatePreview.Choice] = []
    /// The chosen controller's lights, when it lists them.
    @State private var lights: Lights?
    /// The names came from Use the Controller's Lights: bulbs take their own lights' outputs while there's one name a light.
    @State private var fromLights = false

    private var parsed: [String] { studioSingleBulbNames(text: names) }
    private var perBulb: Int { StudioTemplateColor.choices.first { $0.mode == color }?.channelsPerPixel ?? 3 }
    private var chosenController: StudioControllerOverview.Row? { controllers.first { $0.id == controller } }
    /// Using the controller's lights: chosen from them, with one name per unpatched light still.
    private var usingLights: Bool { fromLights && chosenController != nil && lights?.unpatched.count == parsed.count }

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
            if usingLights {
                Text("Each bulb's color handling follows its outputs: RGB for 3, RGBW for 4, a single color for 1.")
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            } else {
                StudioTemplateColorPicker(mode: $color)
            }
            patchSection
            previewSection
            SheetButtons(okTitle: parsed.count == 1 ? "Add Bulb" : "Add Bulbs", okDisabled: parsed.isEmpty, ok: add, cancel: onFinish)
        }
        .padding(16)
        .frame(width: 460)
        .sheetKeys(okEnabled: !parsed.isEmpty, ok: add, cancel: onFinish)
        .task { await load() }
        .task(id: controller) { await loadLights() }
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
            if let chosen = chosenController, !usingLights {
                HStack(spacing: 6) {
                    SheetFieldLabel("From output:")
                    SheetNumberField("From output:", value: $start, range: 1...max(1, chosen.outputs))
                }
            }
            if let chosen = chosenController, let free = lights?.unpatched, !free.isEmpty, !usingLights {
                Button("Use \(chosen.name)'s Lights (\(free.count))") { useLights() }
                    .help("Make a bulb for each light \(chosen.name) lists that isn't patched yet, named after it and patched to its own outputs")
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
        if usingLights, let free = lights?.unpatched {
            let each = zip(parsed, free).prefix(2).map { "\($0) outputs \($1.firstOutput)–\($1.lastOutput)" }
            return "Each bulb takes its own light's outputs on \(chosen.name): " + each.joined(separator: ", ") + (free.count > 2 ? ", and so on." : ".")
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

    private func loadLights() async {
        fromLights = false
        guard let session = model.session, !controller.isEmpty else {
            lights = nil
            return
        }
        let id = controller
        let json = await offMain { try? session.studioControllerLights(id: id) }
        guard id == controller else { return }
        lights = json.flatMap { try? JSONDecoder().decode(Lights.self, from: Data($0.utf8)) }
        if Self.testUseLights {
            Self.testUseLights = false
            useLights()
        }
    }

    /// Names from the controller's unpatched lights, each bulb on its own light's outputs.
    private func useLights() {
        guard let free = lights?.unpatched, !free.isEmpty else { return }
        names = free.map(\.name).joined(separator: "\n")
        fromLights = true
    }

    private func add() {
        let list = parsed
        guard !list.isEmpty else { return }
        Self.remembered = (names, color, controller, start, preview)
        var arguments: [String: Any] = ["names": list, "color": color]
        if usingLights, let chosen = chosenController {
            arguments = ["names": list, "fromController": true, "controllerId": chosen.id]
        } else if let chosen = chosenController {
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
