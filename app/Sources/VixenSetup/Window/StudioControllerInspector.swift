import SwiftUI
import VixenBridge
import VixenKit

/// The controller inspector's network settings (Display ▸ Controllers) for one E1.31 or DDP controller: protocol, a unicast
/// address up front, the output count, and universes generated from it, with WLED presets and a status line. Every edit is
/// `controller_configure` in Rust, the same command agents call, made on the working copy, so it publishes with Undo.
struct StudioControllerNetworkInspector: View {
    let model: SetupModel

    @State private var info: StudioControllerNetwork?
    @State private var status: StudioControllerLiveStatus?
    @State private var check: StudioProbeResult?
    @State private var checking = false
    @State private var error: String?
    @State private var address = ""
    @State private var outputs = ""
    @State private var firstUniverse = ""
    @FocusState private var focus: Field?

    private enum Field { case address, outputs, firstUniverse }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if let info, let proto = info.protocol {
                content(info, proto)
            } else {
                // Something to hang the task on while the first read is on its way.
                Color.clear.frame(height: 1)
            }
        }
        // Follows the selection, undo and agent edits, and keeps the status line current.
        .task(id: model.controllerPanel) {
            while !Task.isCancelled {
                await refresh()
                try? await Task.sleep(for: .seconds(1))
            }
        }
    }

    @ViewBuilder
    private func content(_ info: StudioControllerNetwork, _ proto: String) -> some View {
        VStack(alignment: .leading, spacing: StudioMetrics.spacingS) {
            VStack(alignment: .leading, spacing: StudioMetrics.spacingS) {
                row("Protocol") {
                    Picker("Protocol", selection: Binding(get: { proto }, set: { apply(["protocol": $0]) })) {
                        Text("E1.31").tag("e131")
                        Text("DDP").tag("ddp")
                    }
                    .pickerStyle(.segmented)
                    .labelsHidden()
                    .fixedSize()
                    .help("E1.31 (sACN) sends universes; DDP sends one stream. Changing it keeps the name, outputs and patches.")
                }
                row("Address") {
                    HStack(spacing: StudioMetrics.spacingXS) {
                        TextField("192.168.1.50", text: $address)
                            .textFieldStyle(.roundedBorder)
                            .focused($focus, equals: .address)
                            .accessibilityLabel("Controller address")
                            .onSubmit { commit(.address) }
                            .help("The controller's IP address. Data is sent straight to it (unicast).")
                        Button(checking ? "Checking…" : "Check") { runCheck() }
                            .controlSize(.small)
                            .disabled(checking || info.address == nil)
                            .help("See whether the address answers. Sends no lighting data.")
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                if let check {
                    row(nil) {
                        Label(check.text, systemImage: check.reachable == true ? "checkmark.circle.fill" : "exclamationmark.triangle.fill")
                            .symbolRenderingMode(.multicolor)
                            .font(.caption)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
                row("Outputs") {
                    TextField("Outputs", text: $outputs)
                        .textFieldStyle(.roundedBorder)
                        .monospacedDigit()
                        .frame(maxWidth: 90)
                        .focused($focus, equals: .outputs)
                        .accessibilityLabel("Controller outputs")
                        .onSubmit { commit(.outputs) }
                        .help("Channels on this controller: 3 per RGB pixel, 4 per RGBW pixel")
                }
                row(nil) {
                    Text("\(info.pixels.rgbw.formatted()) RGBW or \(info.pixels.rgb.formatted()) RGB pixels")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                if proto == "e131" {
                    row("Universe") {
                        TextField("1", text: $firstUniverse)
                            .textFieldStyle(.roundedBorder)
                            .monospacedDigit()
                            .frame(maxWidth: 90)
                            .focused($focus, equals: .firstUniverse)
                            .accessibilityLabel("First universe")
                            .onSubmit { commit(.firstUniverse) }
                            .help("The first universe number; the rest follow on, one per 510 or 512 channels")
                    }
                    row("Channels") {
                        Picker("Channels per universe", selection: Binding(get: { info.universeSize ?? 510 }, set: { apply(["universeSize": $0]) })) {
                            Text("510").tag(510)
                            Text("512").tag(512)
                        }
                        .pickerStyle(.segmented)
                        .labelsHidden()
                        .fixedSize()
                        .help("Channels per universe: 510 fits 170 RGB pixels, 512 fits 128 RGBW pixels")
                    }
                    row("Preset") {
                        Menu("WLED…") {
                            Button("Multiple RGB (510 per universe)") { apply(["preset": "wledRgb"]) }
                            Button("Multiple RGBW (512 per universe)") { apply(["preset": "wledRgbw"]) }
                        }
                        .fixedSize()
                        .help("Match WLED's E1.31 DMX mode")
                    }
                }
                row(nil) {
                    Text(info.layout)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                row("Status") {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(status?.text ?? "")
                            .fixedSize(horizontal: false, vertical: true)
                        if let problem = status?.problem {
                            StudioControllerProblemLabel(problem: problem)
                        }
                    }
                }
            }
            .font(.callout)
            .frame(maxWidth: .infinity, alignment: .leading)
            if let error {
                Label(error, systemImage: "exclamationmark.triangle.fill")
                    .symbolRenderingMode(.multicolor)
                    .font(.caption)
            }
        }
        .onChange(of: focus) { old, _ in
            if let old { commit(old) }
        }
    }

    /// One setting: its label in a narrow leading column, the control taking the rest of the width.
    private func row<Content: View>(_ label: String?, @ViewBuilder content: () -> Content) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: StudioMetrics.spacingS) {
            Text(label ?? "")
                .foregroundStyle(.secondary)
                .frame(width: 62, alignment: .leading)
            content()
                .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    // MARK: Editing

    private func commit(_ field: Field) {
        guard let info else { return }
        switch field {
        case .address:
            let text = address.trimmingCharacters(in: .whitespaces)
            if text != (info.address ?? "") { apply(["address": text]) }
        case .outputs:
            guard let value = Int(outputs.filter(\.isNumber)), value != info.outputs else { outputs = "\(info.outputs)"; return }
            apply(["outputs": value])
        case .firstUniverse:
            guard let value = Int(firstUniverse.filter(\.isNumber)), value != info.firstUniverse else { firstUniverse = info.firstUniverse.map(String.init) ?? ""; return }
            apply(["firstUniverse": value])
        }
    }

    private func apply(_ change: [String: Any]) {
        guard let info, let session = model.session else { return }
        var arguments = change
        arguments["id"] = info.id
        // A controller with no address yet takes the one being typed, so choosing a preset first still works.
        if arguments["address"] == nil, info.address == nil {
            let typed = address.trimmingCharacters(in: .whitespaces)
            if !typed.isEmpty { arguments["address"] = typed }
        }
        guard let data = try? JSONSerialization.data(withJSONObject: arguments), let json = String(data: data, encoding: .utf8) else { return }
        Task {
            let result = await offMain { Result { try session.studioConfigureControllerNetwork(argumentsJson: json) } }
            switch result {
            case .success(let text):
                error = nil
                if let slot = (try? JSONDecoder().decode(StudioConfigureResult.self, from: Data(text.utf8)))?.newSlot {
                    // A protocol change replaces the controller; the tree shows the new one in the old one's place.
                    await model.reloadControllers()
                    await model.controllerTree.select(["c\(slot)"])
                }
                model.controllersChanged()
                await refresh(resetFields: true)
            case .failure(let failure):
                error = failure.userMessage
                await refresh(resetFields: true)
            }
        }
    }

    private func runCheck() {
        guard let address = info?.address, let core = model.app.core else { return }
        checking = true
        Task {
            let text = await offMain { core.studioProbeController(address: address) }
            check = try? JSONDecoder().decode(StudioProbeResult.self, from: Data(text.utf8))
            checking = false
        }
    }

    // MARK: Reading

    private func refresh(resetFields: Bool = false) async {
        guard let session = model.session else { info = nil; return }
        let rows = model.controllerSelection
        let core = model.app.core
        let (json, statusJSON) = await offMain { () -> (String?, String?) in
            guard let id = session.firstControllerInfo(rows: rows)?.id else { return (nil, nil) }
            return (session.studioControllerNetwork(id: id), core?.studioControllerStatus(id: id))
        }
        guard let json, let next = try? JSONDecoder().decode(StudioControllerNetwork.self, from: Data(json.utf8)) else {
            info = nil
            return
        }
        if next.id != info?.id {
            check = nil
            error = nil
        }
        let changed = next != info
        info = next
        if let statusJSON, let decoded = try? JSONDecoder().decode(StudioControllerLiveStatus.self, from: Data(statusJSON.utf8)) {
            status = decoded
        }
        // Fields being typed in keep their text; the others follow the controller.
        guard changed || resetFields else { return }
        if focus != .address { address = next.address ?? "" }
        if focus != .outputs { outputs = "\(next.outputs)" }
        if focus != .firstUniverse { firstUniverse = next.firstUniverse.map(String.init) ?? "" }
    }
}

/// `studio_controller_network::info`.
nonisolated struct StudioControllerNetwork: Decodable, Equatable, Sendable {
    struct Pixels: Decodable, Equatable, Sendable { let rgb: Int; let rgbw: Int }
    let id: String
    let `protocol`: String?
    let address: String?
    let outputs: Int
    let pixels: Pixels
    let firstUniverse: Int?
    let universeSize: Int?
    let layout: String
}

/// `studio_controller_network::status`: the status line, and what's wrong while something is (send errors with how long
/// ago the last one was; a plugin's own `unavailable`/`error` state while it's current).
struct StudioControllerLiveStatus: Decodable, Equatable {
    struct Problem: Decodable, Equatable {
        let level: String
        let text: String
    }

    let text: String
    let problem: Problem?
    let moduleState: String?
    let moduleProblem: String?

    /// A plugin controller's state comes first: it's what the plugin says right now.
    var current: Problem? {
        if let moduleProblem { return Problem(level: moduleState == "unavailable" ? "warning" : "error", text: moduleProblem) }
        return problem
    }
}

/// A problem in full, wrapping rather than truncating: the list's Output column only has room for its start.
struct StudioControllerProblemLabel: View {
    let problem: StudioControllerLiveStatus.Problem

    var body: some View {
        Label(problem.text, systemImage: problem.level == "error" ? "xmark.octagon.fill" : "exclamationmark.triangle.fill")
            .symbolRenderingMode(.multicolor)
            .font(.caption)
            .fixedSize(horizontal: false, vertical: true)
            .textSelection(.enabled)
    }
}

private struct StudioProbeResult: Decodable { let reachable: Bool?; let text: String }
private struct StudioConfigureResult: Decodable { let newSlot: Int? }

extension SetupModel {
    /// Whether the selected controller is one the network inspector edits.
    func studioSelectedNetworkController() async -> Bool {
        guard let session else { return false }
        let rows = controllerSelection
        return await offMain {
            guard let id = session.firstControllerInfo(rows: rows)?.id, let json = session.studioControllerNetwork(id: id) else { return false }
            return (try? JSONDecoder().decode(StudioControllerNetwork.self, from: Data(json.utf8)))?.protocol != nil
        }
    }
}
