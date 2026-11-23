import AppKit
import SwiftUI
import VixenBridge
import VixenKit

/// The show window's Controllers section: every controller with its run state, address, universes, patching and output
/// health, and the selected controller's inspector beside the list. It is the Display pane's working copy seen another way:
/// the list's selection is the controller tree's selection, and every action is Display Setup's own, so edits publish with
/// Undo and show in Display at once. The list reads `controller_overview` in Rust, the command the MCP tool runs.
struct StudioControllersSection: View {
    @Bindable var model: SetupModel
    @State private var overview = StudioControllerOverview.empty
    @State private var selection: Set<String> = []

    var body: some View {
        HSplitView {
            VStack(alignment: .leading, spacing: StudioMetrics.spacingS) {
                header
                table
            }
            .padding(StudioMetrics.spacingM)
            .frame(minWidth: 520, idealWidth: 700, maxHeight: .infinity)
            ScrollView {
                StudioControllersPanel(model: model, inline: true, section: true)
            }
            .frame(minWidth: 280, idealWidth: 320, maxWidth: 360, maxHeight: .infinity)
        }
        .background(.background)
        .overlay(alignment: .top) {
            if model.isBusy {
                ProgressView()
                    .controlSize(.small)
                    .padding(6)
                    .background(.regularMaterial, in: Capsule())
                    .padding(.top, 8)
            }
        }
        .onAppear {
            // The Display columns stay alive underneath, hidden; none of them may keep the keyboard.
            StudioDisplayPane.hostWindow?.makeFirstResponder(nil)
            followTree()
            installTestHooks()
        }
        // Follows edits here, in Display, by agents and Undo, and keeps run state and health current.
        .task(id: model.controllerPanel) {
            followTree()
            while !Task.isCancelled {
                await refresh()
                try? await Task.sleep(for: .seconds(1))
            }
        }
        .onChange(of: selection) { _, new in selectInTree(new) }
    }

    // MARK: List

    private var header: some View {
        HStack(alignment: .firstTextBaseline) {
            VStack(alignment: .leading, spacing: 2) {
                Text("Controllers")
                    .font(.headline)
                Text(overview.summary)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer(minLength: StudioMetrics.spacingS)
            Menu {
                StudioAddControllerMenuItems(model: model)
            } label: {
                Label("Add", systemImage: "plus")
                    .labelStyle(.titleAndIcon)
            }
            .fixedSize()
            .help("Add a controller such as E1.31 (sACN), DDP, a serial device or a plugin's type")
            .disabled(model.studioEditingPaused)
        }
    }

    /// A Table starts its columns at their ideal widths and scrolls sideways rather than narrow them, so the ideals add up
    /// to the width the table gets in the show window at its minimum size; wider windows give the columns the rest, and
    /// long names truncate in their cells.
    private var table: some View {
        Table(overview.controllers, selection: $selection) {
            TableColumn("State") { row in
                HStack(spacing: 5) {
                    Circle()
                        .fill(row.running ? Color.green : Color.red)
                        .frame(width: 7, height: 7)
                        .accessibilityHidden(true)
                    Text(row.running ? "Running" : "Stopped")
                }
                .help(row.running ? "Sends while Live Output is on" : "Doesn't send, even with Live Output on")
            }
            .width(min: 60, ideal: 68, max: 100)
            TableColumn("Name") { row in
                VStack(alignment: .leading, spacing: 0) {
                    Text(row.name)
                    Text(row.typeName)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            .width(min: 72, ideal: 88)
            TableColumn("Address") { row in
                Text(row.address ?? "—")
                    .monospacedDigit()
                    .foregroundStyle(row.address == nil ? .secondary : .primary)
            }
            .width(min: 52, ideal: 56)
            TableColumn("Universes") { row in
                Text(row.universes.isEmpty ? "—" : row.universes)
                    .foregroundStyle(row.universes.isEmpty ? .secondary : .primary)
            }
            .width(min: 52, ideal: 58)
            TableColumn("Patched") { row in
                Text("\(row.patchedOutputs.formatted()) of \(row.outputs.formatted())")
                    .monospacedDigit()
                    .help("\(row.patchedOutputs.formatted()) of this controller's \(row.outputs.formatted()) outputs have an element patched to them")
            }
            .width(min: 52, ideal: 60)
            TableColumn("Output") { row in
                Label(row.health.text, systemImage: row.health.symbol)
                    .symbolRenderingMode(.multicolor)
                    .foregroundStyle(row.health.dimmed ? .secondary : .primary)
                    // Two lines, like Name: enough for most reasons; the inspector shows them in full.
                    .lineLimit(2)
                    .help(([row.health.text] + row.problems.dropFirst()).joined(separator: "\n"))
            }
            .width(min: 64, ideal: 72)
        }
        .contextMenu(forSelectionType: String.self) { ids in
            contextMenu(ids)
        } primaryAction: { ids in
            guard ids.count == 1, !model.studioEditingPaused else { return }
            act(on: ids) { await model.configureController() }
        }
        .onDeleteCommand {
            guard !selection.isEmpty, !model.studioEditingPaused else { return }
            act(on: selection) { await model.deleteControllers() }
        }
        .accessibilityLabel("Controllers")
        .overlay {
            if overview.loaded, overview.controllers.isEmpty {
                ContentUnavailableView {
                    Label("No Controllers", systemImage: "cable.connector")
                } description: {
                    Text("Add the controllers your lights are wired to, such as an E1.31 or DDP pixel controller.")
                }
            }
        }
    }

    /// What Display's controller menu offers, for the rows clicked. Each item selects them first, so it acts on what was
    /// clicked even when that wasn't the selection.
    @ViewBuilder
    private func contextMenu(_ ids: Set<String>) -> some View {
        let rows = overview.controllers.filter { ids.contains($0.id) }
        let one = rows.count == 1
        let paused = model.studioEditingPaused
        if !rows.isEmpty {
            Group {
                Button("Start") { act(on: ids) { await model.startControllers() } }
                    .disabled(!rows.contains { !$0.running })
                Button("Stop") { act(on: ids) { await model.stopControllers() } }
                    .disabled(!rows.contains { $0.running })
                Divider()
                Button(rows.first?.protocol != nil ? "Advanced…" : "Configure…") { act(on: ids) { await model.configureController() } }
                    .disabled(!one)
                if rows.first?.protocol == nil {
                    Button("Outputs…") { act(on: ids) { await model.channelCount() } }
                        .disabled(!one)
                }
                Button("Rename…") { act(on: ids) { await model.renameController() } }
                    .disabled(!one)
                Divider()
                Button("Move Up") { act(on: ids) { await model.moveControllers(up: true) } }
                Button("Move Down") { act(on: ids) { await model.moveControllers(up: false) } }
            }
            .disabled(paused)
            if one, let id = rows.first?.id {
                // Display's own Test submenu for this controller, item for item: patterns, Brightness and Stop Test.
                let test = model.studioControllerTestMenuItem(controllerId: id)?.submenu?.items ?? []
                Menu("Test") {
                    StudioMenuContent(items: test) { path in
                        act(on: ids) { model.studioRunControllerTest(path, controllerId: id) }
                    }
                }
                .help("Light what's patched to this controller, to check its wiring (needs Live Output on)")
                .disabled(test.isEmpty)
            }
            Divider()
            Group {
                Button("Find Elements") {
                    act(on: ids) {
                        await model.findElements()
                        StudioDisplayPane.showDisplay?()
                    }
                }
                .disabled(!rows.contains { $0.patchedOutputs > 0 })
                Button("Unpatch") { act(on: ids) { await model.unpatchControllers() } }
                    .disabled(!rows.contains { $0.patchedOutputs > 0 })
                Button("Delete", role: .destructive) { act(on: ids) { await model.deleteControllers() } }
            }
            .disabled(paused)
        }
    }

    // MARK: Selection

    /// The list shows the controller tree's selection: a controller, or the controller of a selected output.
    private func followTree() {
        let slots = Set(model.controllerSelection.compactMap(Self.slot(ofRow:)))
        let ids = Set(overview.controllers.filter { slots.contains($0.slot) }.map(\.id))
        guard overview.loaded, ids != selection else { return }
        selection = ids
    }

    private func selectInTree(_ ids: Set<String>) {
        let slots = overview.controllers.filter { ids.contains($0.id) }.map(\.slot)
        // Already the tree's selection (or the controllers of its selected outputs): leave it, outputs and all.
        guard Set(model.controllerSelection.compactMap(Self.slot(ofRow:))) != Set(slots) else { return }
        Task { await model.controllerTree.select(slots.map { "c\($0)" }) }
    }

    /// Selects `ids` in the tree, then runs Display Setup's action on them.
    private func act(on ids: Set<String>, _ work: @escaping () async -> Void) {
        let slots = overview.controllers.filter { ids.contains($0.id) }.map(\.slot)
        Task {
            if Set(model.controllerSelection.compactMap(Self.slot(ofRow:))) != Set(slots) || model.controllerSelection.contains(where: { !$0.hasPrefix("c") }) {
                await model.controllerTree.select(slots.map { "c\($0)" })
            }
            await work()
        }
    }

    /// `c3` is controller slot 3; `o3:12` and `p3:0` are its outputs and output pages.
    static func slot(ofRow row: String) -> UInt32? {
        guard let kind = row.first, "cop".contains(kind) else { return nil }
        return UInt32(row.dropFirst().split(separator: ":").first ?? "")
    }

    // MARK: Reading

    private func refresh() async {
        guard let session = model.session, let core = model.app.core else { return }
        let text = await offMain { core.studioControllerOverview(session: session) }
        guard let next = try? JSONDecoder().decode(StudioControllerOverview.self, from: Data(text.utf8)) else { return }
        var loaded = next
        loaded.loaded = true
        if loaded != overview {
            overview = loaded
            followTree()
        }
    }

    private func installTestHooks() {
        guard UITestHarness.isActive else { return }
        UITestHarness.customActions["studioControllersState"] = { _ in
            let rows = overview.controllers.map {
                "\($0.name)=\($0.running ? "running" : "stopped"),\($0.address ?? "-"),\($0.universes),\($0.patchedOutputs)/\($0.outputs),\($0.health.level)"
            }
            let selected = overview.controllers.filter { selection.contains($0.id) }.map(\.name)
            let tree = model.controllerSelection.joined(separator: ",")
            return (true, rows.joined(separator: "; ") + " | selected=\(selected.joined(separator: ",")) tree=\(tree) treeRows=\(model.controllerTree.rowCount) open=\(model.openCount) one=\(model.controllerPanel.oneController) summary=\(overview.summary)")
        }
        UITestHarness.customActions["studioControllersSelect"] = { step in
            let names = Set((step.name ?? "").split(separator: ",").map(String.init))
            let ids = Set(overview.controllers.filter { names.contains($0.name) }.map(\.id))
            guard ids.count == names.count else { return (false, "no controller named \(step.name ?? "")") }
            selection = ids
            return (true, nil)
        }
        // Reopens the working copy `name` times, as Undo and other writers' edits do, and fails if the tree under this
        // section comes back empty: the new tree must get its rows even while the one it replaces is still alive.
        UITestHarness.customActions["studioControllersReopen"] = { step in
            var counts: [Int] = []
            for _ in 0..<(Int(step.name ?? "") ?? 10) {
                model.studioCloseLive()
                model.studioOpenLive()
                for _ in 0..<1000 where model.session == nil {
                    try? await Task.sleep(for: .milliseconds(5))
                }
                for _ in 0..<150 where model.controllerTree.rowCount == 0 {
                    try? await Task.sleep(for: .milliseconds(20))
                }
                counts.append(model.controllerTree.rowCount)
            }
            return (!counts.contains(0), "treeRows " + counts.map(String.init).joined(separator: ","))
        }
        // A context menu item, by title, on the selected rows: SwiftUI's table menus can't be opened by a script.
        UITestHarness.customActions["studioControllersMenu"] = { step in
            let ids = selection
            switch step.name {
            case "Start": act(on: ids) { await model.startControllers() }
            case "Stop": act(on: ids) { await model.stopControllers() }
            case "Move Up": act(on: ids) { await model.moveControllers(up: true) }
            case "Move Down": act(on: ids) { await model.moveControllers(up: false) }
            case "Find Elements": act(on: ids) { await model.findElements(); StudioDisplayPane.showDisplay?() }
            default: return (false, "no menu item \(step.name ?? "")")
            }
            return (true, nil)
        }
        // The selected controller's Test submenu as the section draws it: titles, `>` into submenus, `*` on the checked one.
        UITestHarness.customActions["studioControllersTestMenu"] = { _ in
            guard selection.count == 1, let id = selection.first,
                  let items = model.studioControllerTestMenuItem(controllerId: id)?.submenu?.items else { return (false, "no Test menu") }
            func describe(_ items: [NSMenuItem]) -> String {
                items.map { item in
                    if item.isSeparatorItem { return "-" }
                    let title = (item.state == .on ? "*" : "") + item.title + (item.isEnabled ? "" : "(off)")
                    return item.submenu.map { "\(title)>[\(describe($0.items))]" } ?? title
                }.joined(separator: ", ")
            }
            return (true, describe(items))
        }
        // `{ "action": "studioControllersTest", "name": "Brightness/100%" }`: chooses that Test submenu item on the selection.
        UITestHarness.customActions["studioControllersTest"] = { step in
            guard selection.count == 1, let id = selection.first else { return (false, "select one controller") }
            let path = (step.name ?? "").split(separator: "/").map(String.init)
            act(on: selection) { model.studioRunControllerTest(path, controllerId: id) }
            return (true, nil)
        }
    }
}

/// `studio_controller_network::overview`.
nonisolated struct StudioControllerOverview: Decodable, Equatable, Sendable {
    struct Row: Decodable, Equatable, Sendable, Identifiable {
        let id: String
        let slot: UInt32
        let name: String
        let typeName: String
        let `protocol`: String?
        let address: String?
        let universes: String
        let outputs: Int
        let patchedOutputs: Int
        let running: Bool
        let health: Health
        let problems: [String]
    }

    struct Health: Decodable, Equatable, Sendable {
        let level: String
        let text: String

        var symbol: String {
            switch level {
            case "ok": "checkmark.circle.fill"
            case "error": "xmark.octagon.fill"
            case "warning": "exclamationmark.triangle.fill"
            case "stopped": "stop.circle"
            case "pending": "clock"
            default: "moon.zzz"
            }
        }

        var dimmed: Bool { !["ok", "error", "warning"].contains(level) }
    }

    let controllers: [Row]
    let running: Int
    let needAttention: Int
    var loaded = false

    private enum CodingKeys: String, CodingKey { case controllers, running, needAttention }

    static let empty = StudioControllerOverview(controllers: [], running: 0, needAttention: 0)

    init(controllers: [Row], running: Int, needAttention: Int) {
        self.controllers = controllers
        self.running = running
        self.needAttention = needAttention
    }

    var summary: String {
        guard !controllers.isEmpty else { return loaded ? "None yet" : "" }
        var parts = [controllers.count == 1 ? "1 controller" : "\(controllers.count) controllers", "\(running) running"]
        if needAttention > 0 { parts.append(needAttention == 1 ? "1 needs attention" : "\(needAttention) need attention") }
        return parts.joined(separator: ", ")
    }
}
