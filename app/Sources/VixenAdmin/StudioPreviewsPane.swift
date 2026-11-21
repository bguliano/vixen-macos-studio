import AppKit
import Observation
import SwiftUI
import VixenBridge
import VixenKit

/// The show window's Previews section: every preview with a thumbnail of its layout, an On switch, inline rename, duplicate
/// and delete, all live edits on the profile's shared undo history (the one Display edits and agents use), and Edit Layout
/// to swap the pane to the designer, with a back button to the list. Replaces Setup Previews' OK/Cancel sheet. The edits are the `preview_*` display commands
/// (`studio_previews.rs`), so an agent's changes show here at once and the profile saves after each one.
struct StudioPreviewsPane: View {
    /// The shared history's event revision, which moves whenever anyone (an agent, the designer, Undo) changes the display.
    let revision: UInt64
    @Environment(AdminModel.self) private var admin
    @Environment(\.openWindow) private var openWindow
    private var model: StudioPreviewsModel { .shared }

    var body: some View {
        VStack(spacing: 0) {
            if let id = model.designing, let make = StudioPreviewDesignerPane.make {
                designerBar(id)
                Divider()
                make(id)
                    .id(id)
            } else {
                content
                Divider()
                statusBar
            }
        }
        .background(StudioPreviewsWindowReader())
        .toolbar { toolbar }
        .task(id: revision) { await reload() }
        .onChange(of: admin.executionStatus.stopEnabled) { Task { await reload() } }
        .onAppear {
            model.paneVisible = true
            model.takeOverUndo()
            if UITestHarness.isActive { installTestActions() }
        }
        .onDisappear {
            model.paneVisible = false
            model.releaseUndo()
        }
    }

    private func reload() async {
        guard let core = admin.app.core else { return }
        await model.reload(core: core)
    }

    @ViewBuilder
    private var content: some View {
        if !model.loaded {
            ProgressView()
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else if model.rows.isEmpty {
            ContentUnavailableView {
                Label("No Previews", systemImage: "eye")
            } description: {
                Text("A preview draws your display on screen, so you can watch a sequence without the real lights. Add one, then lay out your props in it.")
            } actions: {
                Button("Add Preview") { perform { await model.add(core: $0) } }
            }
        } else {
            ScrollView {
                VStack(alignment: .leading, spacing: StudioMetrics.spacingM) {
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 220, maximum: 360), spacing: StudioMetrics.spacingL)],
                              alignment: .leading, spacing: StudioMetrics.spacingL) {
                        ForEach(model.rows) { row in
                            StudioPreviewCard(row: row, editLayout: { editLayout(row) }, perform: perform)
                        }
                    }
                    Text(model.liveOutput
                        ? "Previews that are on show in their own windows now."
                        : "Previews that are on show in their own windows while Live Output is on.")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                }
                .padding(StudioMetrics.spacingL)
            }
            .contentShape(Rectangle())
            .onTapGesture { model.selection = nil }
            .onDeleteCommand {
                if let id = model.selection { perform { await model.delete(id: id, core: $0) } }
            }
        }
    }

    private var statusBar: some View {
        HStack(spacing: StudioMetrics.spacingS) {
            if let notice = model.notice {
                Label(notice, systemImage: "exclamationmark.triangle.fill")
                    .symbolRenderingMode(.multicolor)
                    .lineLimit(2)
            } else {
                Text(model.status)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
            Spacer()
            Button { perform { await model.undo(core: $0) } } label: {
                Label("Undo", systemImage: "arrow.uturn.backward")
            }
            .help(model.undoHelp)
            .disabled(!model.canUndo)
            Button { perform { await model.redo(core: $0) } } label: {
                Label("Redo", systemImage: "arrow.uturn.forward")
            }
            .help(model.redoHelp)
            .disabled(!model.canRedo)
        }
        .labelStyle(.titleAndIcon)
        .font(.callout)
        .padding(.horizontal, StudioMetrics.spacingM)
        .padding(.vertical, StudioMetrics.spacingS)
    }

    /// Over the designer: the way back to the list, which writes the layout back as one undo step.
    private func designerBar(_ id: String) -> some View {
        HStack(spacing: StudioMetrics.spacingM) {
            Button { Task { _ = await model.leaveDesigner() } } label: {
                Label("Previews", systemImage: "chevron.backward")
            }
            .help("Back to the previews. The layout becomes one step on Edit ▸ Undo.")
            .disabled(model.leaving)
            Text(model.rows.first { $0.id == id }?.title ?? "Preview")
                .font(.headline)
                .lineLimit(1)
            Spacer()
            if model.leaving {
                ProgressView().controlSize(.small)
            }
        }
        .labelStyle(.titleAndIcon)
        .padding(.horizontal, StudioMetrics.spacingM)
        .padding(.vertical, StudioMetrics.spacingS)
        .background(.bar)
    }

    @ToolbarContentBuilder
    private var toolbar: some ToolbarContent {
        ToolbarItemGroup(placement: .primaryAction) {
            if model.designing == nil { listButtons }
        }
    }

    @ViewBuilder
    private var listButtons: some View {
        let selected = model.selectedRow
        Button { if let selected { editLayout(selected) } } label: {
            Label("Edit Layout…", systemImage: "square.and.pencil")
        }
        .help(selected.map { "Lay out the props in \($0.title)" } ?? "Select a preview to lay out its props")
        .disabled(selected == nil || model.working)
        Button { if let selected { perform { await model.duplicate(id: selected.id, core: $0) } } } label: {
            Label("Duplicate", systemImage: "plus.square.on.square")
        }
        .help("Duplicate the selected preview with its layout")
        .disabled(selected == nil || model.working)
        Button { if let selected { perform { await model.delete(id: selected.id, core: $0) } } } label: {
            Label("Delete", systemImage: "trash")
        }
        .help(selected.map { model.isDesigning($0.id) ? "Close its layout designer first" : "Delete the selected preview. Undo brings it back." } ?? "Select a preview to delete")
        .disabled(selected == nil || model.working || selected.map { model.isDesigning($0.id) } == true)
        Button { perform { await model.add(core: $0) } } label: {
            Label("Add Preview", systemImage: "plus")
        }
        .help("Add an empty preview")
        .disabled(model.working || !model.loaded)
    }

    private func perform(_ action: @escaping (VixenCore) async -> Void) {
        guard let core = admin.app.core else { return }
        Task { await action(core) }
    }

    private func editLayout(_ row: StudioPreviewRow) {
        guard let core = admin.app.core else { return }
        if !model.beginDesign(row: row, core: core) {
            openWindow(id: WindowID.previewSetup, value: row.id)
        }
    }

    /// Cards aren't reachable by a script's click: `studioPreviewSelect` picks one by name, `studioPreviewRename` starts
    /// renaming the selected one as a double-click does, and `studioPreviewEditLayout` opens its designer.
    private func installTestActions() {
        func chain(_ action: Selector) -> (Bool, String?) {
            let done = model.window?.firstResponder?.tryToPerform(action, with: nil) ?? false
            return (done, done ? "\(action) taken" : "nothing took \(action)")
        }
        UITestHarness.customActions["studioPreviewSelect"] = { step in
            guard let row = model.rows.first(where: { $0.name == step.name }) else { return (false, "no preview \(step.name ?? "")") }
            model.selection = row.id
            return (true, nil)
        }
        UITestHarness.customActions["studioPreviewRename"] = { _ in
            guard let id = model.selection else { return (false, "nothing selected") }
            model.renaming = id
            return (true, nil)
        }
        UITestHarness.customActions["studioPreviewEditLayout"] = { _ in
            guard let row = model.selectedRow else { return (false, "nothing selected") }
            editLayout(row)
            return (true, nil)
        }
        UITestHarness.customActions["studioPreviewLeaveDesigner"] = { _ in
            let left = await model.leaveDesigner()
            return (left, left ? nil : "still designing")
        }
        // Edit ▸ Undo and Redo send undo: and redo: to the key window, which hands them to its undo manager. A test window is
        // never key, so these call that undo manager as the menu items would. With the designer showing, they go down the
        // window's responder chain from the canvas, as the menu sends them, to the designer.
        UITestHarness.customActions["studioPreviewMenuUndo"] = { _ in
            if model.designing != nil { return chain(Selector(("undo:"))) }
            guard let manager = model.window?.undoManager, manager.canUndo else { return (false, "Edit ▸ Undo is off") }
            let title = manager.undoMenuItemTitle
            manager.undo()
            return (true, title)
        }
        UITestHarness.customActions["studioPreviewMenuRedo"] = { _ in
            if model.designing != nil { return chain(Selector(("redo:"))) }
            guard let manager = model.window?.undoManager, manager.canRedo else { return (false, "Edit ▸ Redo is off") }
            let title = manager.redoMenuItemTitle
            manager.redo()
            return (true, title)
        }
        UITestHarness.customActions["studioPreviewState"] = { _ in
            (true, model.rows.map { "\($0.name)=\($0.enabled ? "on" : "off")\($0.running ? ",showing" : "")" }.joined(separator: "; ")
                + " | selected=\(model.selectedRow?.name ?? "-") renaming=\(model.renaming == nil ? "no" : "yes") designing=\(model.designing.flatMap { id in model.rows.first { $0.id == id }?.name } ?? "-") undo=\(model.undoLabel ?? "-") redo=\(model.redoLabel ?? "-") menu=\(model.window?.undoManager?.undoMenuItemTitle ?? "-")/\(model.window?.undoManager?.redoMenuItemTitle ?? "-") status=\(model.status) notice=\(model.notice ?? "-")")
        }
    }
}

/// One preview: its layout thumbnail, name (double-click to rename) and On switch.
private struct StudioPreviewCard: View {
    let row: StudioPreviewRow
    let editLayout: () -> Void
    let perform: (@escaping (VixenCore) async -> Void) -> Void
    private var model: StudioPreviewsModel { .shared }
    @State private var draft = ""
    @FocusState private var nameFocused: Bool

    private var selected: Bool { model.selection == row.id }

    var body: some View {
        VStack(alignment: .leading, spacing: StudioMetrics.spacingS) {
            thumbnail
            HStack(alignment: .firstTextBaseline, spacing: StudioMetrics.spacingS) {
                name
                Spacer(minLength: 0)
                Toggle("Show \(row.title)", isOn: Binding(get: { row.enabled }, set: { on in
                    perform { await model.setEnabled(id: row.id, on: on, core: $0) }
                }))
                .labelsHidden()
                .toggleStyle(.switch)
                .controlSize(.small)
                .help(row.enabled ? "On: shows in its own window while Live Output is on. Turn off to hide it." : "Off: never shows. Turn on to show it while Live Output is on.")
            }
            Text(details)
                .font(.caption)
                .foregroundStyle(.secondary)
                .lineLimit(1)
        }
        .padding(StudioMetrics.spacingS)
        .background(selected ? AnyShapeStyle(.selection.opacity(0.25)) : AnyShapeStyle(.clear),
                    in: RoundedRectangle(cornerRadius: StudioMetrics.cornerRadius, style: .continuous))
        .contentShape(RoundedRectangle(cornerRadius: StudioMetrics.cornerRadius, style: .continuous))
        .onTapGesture { model.selection = row.id }
        .contextMenu { menu }
        // On appearing too: a new preview's card appears already renaming.
        .task(id: model.renaming == row.id) {
            guard model.renaming == row.id else { return }
            draft = row.name
            nameFocused = true
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel(row.title)
    }

    private var thumbnail: some View {
        ZStack(alignment: .topLeading) {
            Rectangle().fill(.black)
            if let image = model.thumbnails[row.id] {
                Image(nsImage: image)
                    .resizable()
                    .interpolation(.high)
                    .aspectRatio(contentMode: .fit)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
            if row.props == 0 {
                Text("Empty: Edit Layout to add props")
                    .font(.caption)
                    .foregroundStyle(.white.opacity(0.7))
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
            if row.running {
                StudioStatusBadge("Showing", tone: .active)
                    .padding(StudioMetrics.spacingXS)
                    .background(.black.opacity(0.55), in: Capsule())
                    .padding(StudioMetrics.spacingS)
                    .environment(\.colorScheme, .dark)
            }
        }
        .aspectRatio(16 / 10, contentMode: .fit)
        .clipShape(RoundedRectangle(cornerRadius: StudioMetrics.smallCornerRadius, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: StudioMetrics.smallCornerRadius, style: .continuous)
                .strokeBorder(selected ? AnyShapeStyle(.tint) : AnyShapeStyle(.separator), lineWidth: selected ? 2 : 0.5)
        )
        .opacity(row.enabled ? 1 : 0.55)
        .onTapGesture(count: 2) { editLayout() }
        .simultaneousGesture(TapGesture().onEnded { model.selection = row.id })
        .help("Double-click to lay out \(row.title)")
    }

    @ViewBuilder
    private var name: some View {
        if model.renaming == row.id {
            TextField("Name", text: $draft)
                .textFieldStyle(.roundedBorder)
                .focused($nameFocused)
                .onSubmit(commitRename)
                .onExitCommand { model.renaming = nil }
                .onChange(of: nameFocused) { _, focused in
                    if !focused, model.renaming == row.id { commitRename() }
                }
        } else {
            Text(row.name)
                .font(.headline)
                .lineLimit(1)
                .truncationMode(.middle)
                .onTapGesture(count: 2) {
                    model.selection = row.id
                    model.renaming = row.id
                }
                .help(row.name == row.title ? "Double-click to rename" : "Its window is titled “\(row.title)”. Double-click to rename.")
        }
    }

    private var details: String {
        let props = row.props == 1 ? "1 prop" : "\(row.props) props"
        let lights = row.lights == 1 ? "1 light" : "\(row.lights.formatted()) lights"
        let unlinked = row.lights - row.linkedLights
        return unlinked > 0 ? "\(props) · \(lights), \(unlinked.formatted()) not linked" : "\(props) · \(lights)"
    }

    @ViewBuilder
    private var menu: some View {
        Button("Edit Layout…", action: editLayout)
        Button("Rename") {
            model.selection = row.id
            model.renaming = row.id
        }
        Button("Duplicate") { perform { await model.duplicate(id: row.id, core: $0) } }
        Divider()
        Button(row.enabled ? "Turn Off" : "Turn On") { perform { await model.setEnabled(id: row.id, on: !row.enabled, core: $0) } }
        Divider()
        Button("Delete", role: .destructive) { perform { await model.delete(id: row.id, core: $0) } }
            .disabled(model.isDesigning(row.id))
    }

    private func commitRename() {
        let name = draft.trimmingCharacters(in: .whitespacesAndNewlines)
        model.renaming = nil
        guard !name.isEmpty, name != row.name else { return }
        perform { await model.rename(id: row.id, name: name, core: $0) }
    }
}

/// One row of `studioPreviewList`.
struct StudioPreviewRow: Decodable, Identifiable, Equatable, Sendable {
    let id: String
    let name: String
    let title: String
    let enabled: Bool
    let running: Bool
    let props: Int
    let lights: Int
    let linkedLights: Int
    let camera: Bool
}

private struct StudioPreviewList: Decodable, Sendable {
    let revision: UInt64
    let liveOutput: Bool
    let items: [StudioPreviewRow]
}

private struct StudioPreviewHistory: Decodable, Sendable {
    let undo: [String]
    let redo: [String]
    let undoPreviewOnly: Bool
    let redoPreviewOnly: Bool
}

/// The pane's state and work. One per app, so the designer closing finds it with the pane hidden.
@MainActor @Observable
final class StudioPreviewsModel {
    static let shared = StudioPreviewsModel()

    private(set) var rows: [StudioPreviewRow] = []
    private(set) var loaded = false
    private(set) var liveOutput = false
    private(set) var thumbnails: [String: NSImage] = [:]
    var selection: String?
    var renaming: String?
    private(set) var notice: String?
    private(set) var status = "Edits apply as you make them"
    private(set) var undoLabel: String?
    private(set) var redoLabel: String?
    private(set) var working = false
    /// The preview whose designer the pane shows in place of the list.
    private(set) var designing: String?
    private(set) var leaving = false
    private var undoPreviewOnly = false
    private var redoPreviewOnly = false
    /// Previews whose designer is open. Undo waits for them, since the designer writes its copy back when it closes.
    private var designers = Set<String>()
    @ObservationIgnored var paneVisible = false
    @ObservationIgnored weak var window: NSWindow?
    @ObservationIgnored private var displayRevision: UInt64?
    @ObservationIgnored private var history = ""
    @ObservationIgnored private let undoTarget = UndoTarget()
    /// Previews stopped while their designer is open, as Setup Previews did, to start again when it closes.
    @ObservationIgnored private var stoppedForDesign = Set<String>()
    @ObservationIgnored private var saveTask: Task<Void, Never>?
    @ObservationIgnored private var saveAgain = false
    @ObservationIgnored private var closeTask: Task<Void, Never>?

    private final class UndoTarget: NSObject {}

    var selectedRow: StudioPreviewRow? { rows.first { $0.id == selection } }
    func isDesigning(_ id: String) -> Bool { designers.contains(id) }

    /// A display step undoes only while Live Output is off, since it rebuilds what controllers are sent; a preview step always.
    var canUndo: Bool { undoLabel != nil && !working && designers.isEmpty && (undoPreviewOnly || !liveOutput) }
    var canRedo: Bool { redoLabel != nil && !working && designers.isEmpty && (redoPreviewOnly || !liveOutput) }
    var undoHelp: String { help(undoLabel, previewOnly: undoPreviewOnly, verb: "Undo") }
    var redoHelp: String { help(redoLabel, previewOnly: redoPreviewOnly, verb: "Redo") }

    private func help(_ label: String?, previewOnly: Bool, verb: String) -> String {
        guard let label else { return "Nothing to \(verb.lowercased())" }
        if !designers.isEmpty { return "Close the layout designer to \(verb.lowercased()) \(label)" }
        if !previewOnly && liveOutput { return "Turn off Live Output to \(verb.lowercased()) \(label), a display change" }
        return "\(verb) \(label)"
    }

    // MARK: Loading

    func reload(core: VixenCore) async {
        let (text, historyText) = await Task.detached { (core.studioPreviewList(), core.studioDisplayHistory()) }.value
        guard let list = try? JSONDecoder().decode(StudioPreviewList.self, from: Data(text.utf8)) else { return }
        if list.items != rows { rows = list.items }
        if liveOutput != list.liveOutput { liveOutput = list.liveOutput }
        if let selection, !rows.contains(where: { $0.id == selection }) { self.selection = nil }
        loaded = true
        applyHistory(historyText, mode: .rebuild)
        if displayRevision != list.revision || rows.contains(where: { thumbnails[$0.id] == nil }) {
            displayRevision = list.revision
            await loadThumbnails(core: core, ids: rows.map(\.id))
        }
    }

    /// Off the main thread, one at a time; each is a few milliseconds.
    private func loadThumbnails(core: VixenCore, ids: [String]) async {
        let pictures: [(String, Data)] = await Task.detached {
            ids.compactMap { id in (try? core.studioPreviewThumbnail(id: id, maxWidth: 720, maxHeight: 450)).map { (id, $0) } }
        }.value
        var next: [String: NSImage] = [:]
        for (id, data) in pictures {
            next[id] = NSImage(data: data)
        }
        thumbnails = next
    }

    // MARK: Edits

    /// One display command, the same the MCP tool runs. Returns its result, or nil when it was refused.
    @discardableResult
    private func run(_ command: String, _ arguments: [String: Any], core: VixenCore, history mode: HistoryMode = .register) async -> [String: Any]? {
        guard !working, let data = try? JSONSerialization.data(withJSONObject: arguments),
              let json = String(data: data, encoding: .utf8) else { return nil }
        working = true
        let outcome: Result<String, StudioPreviewFailure> = await Task.detached {
            do { return .success(try core.studioDisplayCommand(command: command, argumentsJson: json)) } catch { return .failure(StudioPreviewFailure(error)) }
        }.value
        working = false
        switch outcome {
        case .success(let reply):
            notice = nil
            let object = (try? JSONSerialization.jsonObject(with: Data(reply.utf8))) as? [String: Any] ?? [:]
            if object["changed"] as? Bool == true {
                await refreshHistory(core: core, mode: mode)
                scheduleSave(core: core)
            }
            await reload(core: core)
            return object["result"] as? [String: Any] ?? [:]
        case .failure(let failure):
            notice = failure.text
            await reload(core: core)
            return nil
        }
    }

    func add(core: VixenCore) async {
        guard let result = await run("preview_create", [:], core: core), let id = result["previewId"] as? String else { return }
        selection = id
        renaming = id
        status = "Added “\(result["name"] as? String ?? "New Preview")”. Name it, then use Edit Layout to add props."
    }

    func rename(id: String, name: String, core: VixenCore) async {
        await run("preview_rename", ["previewId": id, "name": name], core: core)
    }

    func duplicate(id: String, core: VixenCore) async {
        guard let result = await run("preview_duplicate", ["previewId": id], core: core), let copy = result["previewId"] as? String else { return }
        selection = copy
    }

    func delete(id: String, core: VixenCore) async {
        guard !designers.contains(id), let index = rows.firstIndex(where: { $0.id == id }) else { return }
        let name = rows[index].name
        guard await run("preview_delete", ["previewId": id, "confirm": true], core: core) != nil else { return }
        selection = rows.indices.contains(index) ? rows[index].id : rows.last?.id
        status = "Deleted “\(name)”. ⌘Z brings it back."
    }

    func setEnabled(id: String, on: Bool, core: VixenCore) async {
        await run("preview_enable", ["previewId": id, "enabled": on], core: core)
    }

    // MARK: The designer

    /// True when the pane shows the designer itself; false in a build without it, where the caller opens its window.
    func beginDesign(row: StudioPreviewRow, core: VixenCore) -> Bool {
        guard !designers.contains(row.id) else { return designing == row.id }
        selection = row.id
        designers.insert(row.id)
        if row.running, core.setupSetPreviewRunning(id: row.id, running: false) == false {
            stoppedForDesign.insert(row.id)
        }
        guard StudioPreviewDesignerPane.make != nil else { return false }
        // The designer's own history drives Edit ▸ Undo while it shows.
        releaseUndo()
        renaming = nil
        designing = row.id
        return true
    }

    /// The back button, and the show window leaving the section: the designer's prompts, then its write-back. False when the
    /// user kept editing.
    func leaveDesigner() async -> Bool {
        guard designing != nil else { return true }
        guard !leaving else { return false }
        leaving = true
        defer { leaving = false }
        guard await StudioPreviewDesignerPane.leave?() ?? true else { return false }
        // `designerClosed` normally clears it; a designer that never opened doesn't call it.
        designing = nil
        return true
    }

    /// The designer wrote its layout back as one undo step ("Edit Preview Layout"); start its preview again and save.
    func designerClosed(id: String, core: VixenCore) {
        designers.remove(id)
        if designing == id {
            designing = nil
            takeOverUndo()
        }
        if stoppedForDesign.remove(id) != nil, rows.first(where: { $0.id == id })?.enabled == true {
            _ = core.setupSetPreviewRunning(id: id, running: true)
        }
        closeTask = Task {
            await refreshHistory(core: core, mode: .rebuild)
            scheduleSave(core: core)
            await reload(core: core)
        }
    }

    // MARK: Undo

    func undo(core: VixenCore) async {
        if let manager = window?.undoManager, manager.canUndo, manager.undoActionName == undoLabel.map(Self.pretty) {
            manager.undo()
        } else {
            await step(undo: true, core: core, fromMenu: false)
        }
    }

    func redo(core: VixenCore) async {
        if let manager = window?.undoManager, manager.canRedo, manager.redoActionName == redoLabel.map(Self.pretty) {
            manager.redo()
        } else {
            await step(undo: false, core: core, fromMenu: false)
        }
    }

    /// `fromMenu`: the window's undo manager is performing the step and registers its opposite itself; otherwise its
    /// stack is rebuilt from the history afterwards.
    private func step(undo: Bool, core: VixenCore, fromMenu: Bool) async {
        guard undo ? canUndo : canRedo else {
            notice = undo ? undoHelp : redoHelp
            if fromMenu {
                window?.undoManager?.removeAllActions(withTarget: undoTarget)
                history = ""
            }
            return
        }
        let label = undo ? undoLabel : redoLabel
        if await run(undo ? "display_undo" : "display_redo", [:], core: core, history: fromMenu ? .note : .rebuild) != nil, let label {
            status = "\(undo ? "Undid" : "Redid") \(Self.pretty(label))."
        } else {
            window?.undoManager?.removeAllActions(withTarget: undoTarget)
            history = ""
        }
    }

    /// The window's Edit ▸ Undo walks the shared history while this pane shows. The Display pane rebuilds its own steps
    /// when it comes back.
    func takeOverUndo() {
        guard designing == nil else { return }
        window?.undoManager?.removeAllActions()
        history = ""
        if let core = StudioPreviewsCore.current {
            Task { await refreshHistory(core: core, mode: .rebuild) }
        }
    }

    func releaseUndo() {
        guard designing == nil else { return }
        window?.undoManager?.removeAllActions(withTarget: undoTarget)
        history = ""
    }

    enum HistoryMode {
        /// This pane's own edit: add its one step.
        case register
        /// The window's undo manager just performed a step and already holds its opposite.
        case note
        /// Someone else changed the history: rebuild every step.
        case rebuild
    }

    private func refreshHistory(core: VixenCore, mode: HistoryMode) async {
        let text = await Task.detached { core.studioDisplayHistory() }.value
        applyHistory(text, mode: mode)
    }

    /// Mirrors the shared history into the window's undo manager, so Edit ▸ Undo reaches it.
    private func applyHistory(_ text: String, mode: HistoryMode) {
        guard let decoded = try? JSONDecoder().decode(StudioPreviewHistory.self, from: Data(text.utf8)) else { return }
        undoLabel = decoded.undo.first
        redoLabel = decoded.redo.first
        undoPreviewOnly = decoded.undoPreviewOnly
        redoPreviewOnly = decoded.redoPreviewOnly
        // While the designer shows, the window's undo manager is the designer's.
        guard paneVisible, designing == nil, text != history, let manager = window?.undoManager else { return }
        history = text
        guard manager.groupingLevel == 0, !manager.isUndoing, !manager.isRedoing else {
            history = ""
            return
        }
        switch mode {
        case .note:
            return
        case .register:
            if let label = decoded.undo.first {
                ownGroup(manager) { registerStep(manager, label: label, undoing: true) }
            }
            return
        case .rebuild:
            break
        }
        manager.removeAllActions(withTarget: undoTarget)
        for label in decoded.undo.reversed() {
            ownGroup(manager) { registerStep(manager, label: label, undoing: true) }
        }
    }

    private func ownGroup(_ manager: UndoManager, _ register: () -> Void) {
        let automatic = manager.groupsByEvent
        manager.groupsByEvent = false
        manager.beginUndoGrouping()
        register()
        manager.endUndoGrouping()
        manager.groupsByEvent = automatic
    }

    private func registerStep(_ manager: UndoManager, label: String, undoing: Bool) {
        manager.registerUndo(withTarget: undoTarget) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self, let core = StudioPreviewsCore.current else { return }
                self.registerStep(manager, label: label, undoing: !undoing)
                Task { await self.step(undo: undoing, core: core, fromMenu: true) }
            }
        }
        manager.setActionName(Self.pretty(label))
    }

    /// `element_create (Agent)` reads as "Element Create (Agent)"; preview labels are already words.
    static func pretty(_ label: String) -> String {
        guard label.contains("_") else { return label }
        return label.split(separator: " ", maxSplits: 1).enumerated().map { index, part in
            index == 0 ? part.split(separator: "_").map { $0.prefix(1).uppercased() + $0.dropFirst() }.joined(separator: " ") : String(part)
        }.joined(separator: " ")
    }

    // MARK: Saving

    /// The profile saves right after each change, one save at a time, so nothing waits for the pane to be left.
    private func scheduleSave(core: VixenCore) {
        guard saveTask == nil else {
            saveAgain = true
            return
        }
        status = "Saving…"
        saveTask = Task {
            repeat {
                saveAgain = false
                let failure: String? = await Task.detached {
                    do {
                        _ = try core.studioDisplayCommand(command: "profile_save", argumentsJson: #"{"confirm":true}"#)
                        return nil
                    } catch {
                        return StudioPreviewFailure(error).text
                    }
                }.value
                if let failure {
                    notice = "Couldn't save: \(failure)"
                }
            } while saveAgain
            saveTask = nil
            if notice == nil || notice?.hasPrefix("Couldn't save") == false {
                status = status == "Saving…" ? "All changes saved" : status
            }
        }
    }

    /// Waits for a save in flight. The app awaits it before shutting down.
    func flush() async {
        await StudioPreviewDesignerPane.flush?()
        await closeTask?.value
        await saveTask?.value
    }
}

/// The running core, for undo steps the window's menu performs and for the designer closing with the pane hidden, and the
/// way to the section for System ▸ Setup Previews. Set by the show window.
@MainActor
enum StudioPreviewsCore {
    static weak var admin: AdminModel?
    static var current: VixenCore? { admin?.app.core }
    static var reveal: (() -> Void)?
}

private struct StudioPreviewFailure: Error, Sendable {
    let text: String
    nonisolated init(_ error: Error) {
        if case FfiError.Failed(let message) = error { text = message } else { text = "\(error)" }
    }
}

/// Tells the model which window hosts the pane, for its undo manager.
private struct StudioPreviewsWindowReader: NSViewRepresentable {
    final class Marker: NSView {
        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            if let window {
                MainActor.assumeIsolated {
                    StudioPreviewsModel.shared.window = window
                    StudioPreviewsModel.shared.takeOverUndo()
                }
            }
        }
    }

    func makeNSView(context: Context) -> Marker { Marker() }
    func updateNSView(_ marker: Marker, context: Context) {}
}
