import AppKit
import Observation
import SwiftUI
import VixenBridge
import VixenKit

/// Display Setup inside the show window's Display section, editing live. Each edit is made by Display Setup's own algorithms in
/// a working copy, then published within half a second as one step on the profile's shared history, the one agents' edits join.
/// Undo and Redo walk that history, and the profile saves itself shortly after each change. The rest of the app stays usable.
/// While Live Output is on, editing pauses: publishing rebuilds the running graph, which can restart controllers mid-show.
struct StudioDisplaySetupPane: View {
    let model: SetupModel
    @Environment(AppModel.self) private var app
    private var live: StudioDisplayLive { .shared }

    var body: some View {
        VStack(spacing: 0) {
            if model.session == nil {
                ProgressView("Opening Display Setup")
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                if let blocked = live.blocked {
                    StudioLiveOutputNotice(reason: blocked)
                    Divider()
                }
                if let test = live.commission, let core = app.core {
                    StudioCommissionBanner(status: test) { live.stopTest(core: core) }
                    Divider()
                }
                // The trees stay browsable while editing is paused, for selecting and the Test menu; each panel disables its editing controls.
                SetupRootView(model: model, inline: true)
                Divider()
                statusBar
            }
        }
        .background(StudioPaneWindowReader())
        .onAppear {
            live.paneVisible = true
            // The Previews pane had the window's undo stack; rebuild this pane's steps on the next tick.
            live.forgetHistory()
            openIfReady()
        }
        .onDisappear {
            live.paneVisible = false
            // The show window closed. Leaving the section has already flushed through `confirmLeave`.
            guard model.session != nil else { return }
            Task {
                await live.flush(model: model, app: app)
                model.studioCloseLive()
            }
        }
        .onChange(of: app.core == nil) { openIfReady() }
        // Publishes edits, notices other writers (agents, Undo) and keeps the Live Output pause current.
        .task {
            while !Task.isCancelled {
                await live.tick(model: model, app: app)
                try? await Task.sleep(for: .milliseconds(500))
            }
        }
    }

    private var statusBar: some View {
        HStack(spacing: StudioMetrics.spacingS) {
            if let notice = live.notice {
                Label(notice, systemImage: "exclamationmark.triangle.fill")
                    .symbolRenderingMode(.multicolor)
                    .lineLimit(2)
            } else {
                Text(live.saveText)
                    .foregroundStyle(.secondary)
            }
            Spacer()
            Button { live.undo(model: model, app: app) } label: {
                Label("Undo", systemImage: "arrow.uturn.backward")
            }
            .help(live.undoLabel.map { "Undo \($0)" } ?? "Nothing to undo")
            .disabled(live.undoLabel == nil || live.blocked != nil || live.working)
            Button { live.redo(model: model, app: app) } label: {
                Label("Redo", systemImage: "arrow.uturn.forward")
            }
            .help(live.redoLabel.map { "Redo \($0)" } ?? "Nothing to redo")
            .disabled(live.redoLabel == nil || live.blocked != nil || live.working)
            Button("Help") { model.help() }
        }
        .labelStyle(.titleAndIcon)
        .font(.callout)
        .padding(.horizontal, StudioMetrics.spacingM)
        .padding(.vertical, StudioMetrics.spacingS)
    }

    private func openIfReady() {
        guard live.paneVisible, model.session == nil, !model.isClosing, app.core != nil else { return }
        live.reset()
        model.studioOpenLive()
    }
}

/// Step 3: why editing has paused, in place, with the way out.
private struct StudioLiveOutputNotice: View {
    let reason: String

    var body: some View {
        HStack(spacing: StudioMetrics.spacingS) {
            Image(systemName: "dot.radiowaves.left.and.right")
                .foregroundStyle(.green)
                .imageScale(.large)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text("\(reason), so display editing is paused")
                    .font(.headline)
                Text("Changing the display rebuilds what the controllers are sent, which can restart them mid-show. You can still look around.")
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
            Spacer()
            if let stop = StudioDisplayPane.stopLiveOutput {
                Button("Turn Off Live Output") { stop() }
            }
        }
        .padding(.horizontal, StudioMetrics.spacingM)
        .padding(.vertical, StudioMetrics.spacingS)
        .background(.green.opacity(0.08))
    }
}

/// The live pane's state and the work behind it. One per app, like the pane's model.
@MainActor @Observable
final class StudioDisplayLive {
    static let shared = StudioDisplayLive()

    private(set) var blocked: String?
    private(set) var notice: String?
    private(set) var undoLabel: String?
    private(set) var redoLabel: String?
    private(set) var working = false
    private(set) var saveText = "Edits apply as you make them"
    /// The commissioning test running now (`StudioTestMenu.swift`), read every tick.
    private(set) var commission: StudioCommissionStatus?
    @ObservationIgnored var paneVisible = false
    /// Scripts run muted, where Live Output never truly comes on; `displayTestBlock` stands in for it to show the pause.
    @ObservationIgnored var testBlock: String?
    /// Scripts can't run a test while muted; `displayTestCommission` stands in for one to show the banner.
    @ObservationIgnored var testCommission: StudioCommissionStatus?
    @ObservationIgnored private var history = ""
    @ObservationIgnored private var saveTask: Task<Void, Never>?
    @ObservationIgnored private var unsaved = false
    @ObservationIgnored private let undoTarget = UndoTarget()

    private final class UndoTarget: NSObject {}

    private enum Outcome: Sendable {
        case idle, stale, published(revision: UInt64, label: String), failed(String)
    }

    func reset() {
        notice = nil
        blocked = nil
    }

    func forgetHistory() {
        history = ""
    }

    /// Every half second while the pane shows.
    func tick(model: SetupModel, app: AppModel) async {
        guard paneVisible, let core = app.core else { return }
        let (real, test) = await offMain { (core.studioDisplayEditBlock(), core.studioCommissionStatus()) }
        blocked = testBlock ?? real
        commission = testCommission ?? test.flatMap { try? JSONDecoder().decode(StudioCommissionStatus.self, from: Data($0.utf8)) }
        // A template's elements, colour handling and preview shape publish together, as one undo step.
        guard !working, !model.addingFromTemplate else { return }
        await publish(model: model, core: core, cleanup: false)
        await refreshHistory(core: core, rebuildIfChanged: true)
    }

    /// Before the pane goes: publish what's left, with the cleanup Display Setup's OK did, and save.
    func flush(model: SetupModel, app: AppModel) async {
        guard let core = app.core else { return }
        while working { try? await Task.sleep(for: .milliseconds(20)) }
        await publish(model: model, core: core, cleanup: true)
        saveTask?.cancel()
        await save(core: core)
    }

    private func publish(model: SetupModel, core: VixenCore, cleanup: Bool) async {
        guard let session = model.session, blocked == nil || cleanup else { return }
        working = true
        defer { working = false }
        let base = model.studioBaseRevision
        let outcome: Outcome = await offMain {
            if core.displayRevision() != base { return .stale }
            guard cleanup || session.isDirty() else { return .idle }
            do {
                let reply = try core.studioPublishDisplaySetup(session: session, expectedRevision: base, cleanup: cleanup)
                guard let object = try? JSONSerialization.jsonObject(with: Data(reply.utf8)) as? [String: Any],
                      object["changed"] as? Bool == true,
                      let revision = (object["revision"] as? NSNumber)?.uint64Value else { return .idle }
                return .published(revision: revision, label: object["label"] as? String ?? "Edit Display")
            } catch {
                return .failed(studioErrorText(error))
            }
        }
        switch outcome {
        case .idle:
            break
        case .published(let revision, let label):
            model.studioBaseRevision = revision
            notice = nil
            registerUndo(label: label)
            scheduleSave(core: core)
        case .stale, .failed:
            if case .failed(let message) = outcome, !message.contains("Display changed"), !message.contains(StudioDisplayLive.liveOutput) {
                notice = message
                return
            }
            // Another writer (an agent, Undo, a legacy window) moved the display, or Live Output came on: start again from the display as it is.
            var liveOutput = blocked != nil
            if case .failed(let message) = outcome, message.contains(StudioDisplayLive.liveOutput) { liveOutput = true }
            let lost = await offMain { session.isDirty() }
            if lost {
                notice = liveOutput
                    ? "Live Output came on before your last edit applied, so it was undone."
                    : "The display changed elsewhere before your last edit applied, so it was undone."
            }
            reload(model: model)
        }
    }

    static let liveOutput = "Live Output is on"

    private func reload(model: SetupModel) {
        // A prompt or a long edit still needs the working copy it started with; try again on the next tick.
        guard model.sheet == nil, model.alert == nil, !model.isBusy else { return }
        model.studioCloseLive()
        model.studioOpenLive()
    }

    // MARK: Undo

    private struct History: Decodable {
        let undo: [String]
        let redo: [String]
    }

    /// Mirrors the shared history into the window's undo manager, so Edit ▸ Undo reaches it, unless the change was this pane's own.
    private func refreshHistory(core: VixenCore, rebuildIfChanged: Bool) async {
        let text = await offMain { core.studioDisplayHistory() }
        guard let decoded = try? JSONDecoder().decode(History.self, from: Data(text.utf8)) else { return }
        undoLabel = decoded.undo.first.map(Self.pretty)
        redoLabel = decoded.redo.first.map(Self.pretty)
        guard text != history else { return }
        let ours = !rebuildIfChanged
        history = text
        if !ours, let manager = StudioDisplayPane.hostWindow?.undoManager {
            // Mid-event, a group is already open and every step would land in it; the next tick rebuilds instead.
            guard manager.groupingLevel == 0, !manager.isUndoing, !manager.isRedoing else {
                history = ""
                return
            }
            manager.removeAllActions(withTarget: undoTarget)
            for label in decoded.undo.reversed() {
                ownGroup(manager) { registerStep(manager, label: label, undoing: true) }
            }
        }
    }

    private func registerUndo(label: String) {
        guard let manager = StudioDisplayPane.hostWindow?.undoManager else { return }
        if manager.groupingLevel == 0 {
            ownGroup(manager) { registerStep(manager, label: label, undoing: true) }
        } else {
            registerStep(manager, label: label, undoing: true)
        }
    }

    /// One top-level undo group per display step, so Undo takes back exactly one. Automatic grouping would put every step
    /// registered in this pass into a single group.
    private func ownGroup(_ manager: UndoManager, _ register: () -> Void) {
        let automatic = manager.groupsByEvent
        manager.groupsByEvent = false
        manager.beginUndoGrouping()
        register()
        manager.endUndoGrouping()
        manager.groupsByEvent = automatic
    }

    /// One step on the window's stack. Performing it registers its opposite first, which is how `UndoManager` builds Redo.
    private func registerStep(_ manager: UndoManager, label: String, undoing: Bool) {
        manager.registerUndo(withTarget: undoTarget) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self, let model = StudioDisplayLive.model, let app = StudioDisplayLive.app else { return }
                self.registerStep(manager, label: label, undoing: !undoing)
                self.step(undo: undoing, model: model, app: app)
            }
        }
        manager.setActionName(Self.pretty(label))
    }

    /// The pane's own buttons go through the window's stack, so the menu and the buttons stay in step.
    func undo(model: SetupModel, app: AppModel) {
        if let manager = StudioDisplayPane.hostWindow?.undoManager, manager.canUndo {
            manager.undo()
        } else {
            step(undo: true, model: model, app: app)
        }
    }

    func redo(model: SetupModel, app: AppModel) {
        if let manager = StudioDisplayPane.hostWindow?.undoManager, manager.canRedo {
            manager.redo()
        } else {
            step(undo: false, model: model, app: app)
        }
    }

    private func step(undo: Bool, model: SetupModel, app: AppModel) {
        guard let core = app.core, !working, blocked == nil else { return }
        Task {
            // An edit made in the last half second goes first, so Undo takes back what's on screen.
            await publish(model: model, core: core, cleanup: false)
            working = true
            let command = undo ? "display_undo" : "display_redo"
            let failure: String? = await offMain {
                do {
                    _ = try core.studioDisplayCommand(command: command, argumentsJson: "{}")
                    return nil
                } catch {
                    return studioErrorText(error)
                }
            }
            working = false
            if let failure {
                notice = failure
                StudioDisplayPane.hostWindow?.undoManager?.removeAllActions(withTarget: undoTarget)
                history = ""
            } else {
                notice = nil
                scheduleSave(core: core)
            }
            await refreshHistory(core: core, rebuildIfChanged: false)
            reload(model: model)
        }
    }

    /// A display command from the pane's own menus (Patch ▸ Reverse and the like), the same command the MCP tool runs. An
    /// edit made in the last half second publishes first; the command is then one step on the shared history, and the
    /// working copy reloads to show it.
    func perform(command: String, arguments: [String: Any], model: SetupModel, app: AppModel) {
        guard let core = app.core, !working, blocked == nil,
              let data = try? JSONSerialization.data(withJSONObject: arguments),
              let json = String(data: data, encoding: .utf8) else { return }
        Task {
            await publish(model: model, core: core, cleanup: false)
            working = true
            let failure: String? = await offMain {
                do {
                    _ = try core.studioDisplayCommand(command: command, argumentsJson: json)
                    return nil
                } catch {
                    return studioErrorText(error)
                }
            }
            working = false
            notice = failure
            await refreshHistory(core: core, rebuildIfChanged: false)
            if failure == nil {
                registerUndo(label: command)
                scheduleSave(core: core)
            }
            reload(model: model)
        }
    }

    /// Starts a commissioning test from the Test submenu. Tests change nothing in the show, so there's no undo step; a
    /// refusal (Live Output off, nothing patched, the controller stopped) shows in the status bar.
    func startTest(arguments: [String: Any], core: VixenCore) {
        guard let data = try? JSONSerialization.data(withJSONObject: arguments),
              let json = String(data: data, encoding: .utf8) else { return }
        Task {
            let outcome: Result<String, StudioTestFailure> = await offMain {
                do { return .success(try core.studioCommissionStart(argumentsJson: json)) } catch { return .failure(StudioTestFailure(text: studioErrorText(error))) }
            }
            switch outcome {
            case .success(let reply):
                notice = nil
                commission = try? JSONDecoder().decode(StudioCommissionStatus.self, from: Data(reply.utf8))
            case .failure(let failure):
                notice = "Couldn't start the test: \(failure.text)"
            }
        }
    }

    func stopTest(core: VixenCore) {
        testCommission = nil
        commission = nil
        Task { _ = await offMain { core.studioCommissionStop() } }
    }

    /// Which pane the window's undo steps act on. Set when the pane is registered.
    static weak var model: SetupModel?
    static weak var app: AppModel?

    /// `element_create (Agent)` reads as "Element Create (Agent)"; the pane's own labels are already words.
    static func pretty(_ label: String) -> String {
        guard label.contains("_") else { return label }
        return label.split(separator: " ", maxSplits: 1).enumerated().map { index, part in
            index == 0 ? part.split(separator: "_").map { $0.prefix(1).uppercased() + $0.dropFirst() }.joined(separator: " ") : String(part)
        }.joined(separator: " ")
    }

    // MARK: Saving

    private func scheduleSave(core: VixenCore) {
        saveTask?.cancel()
        unsaved = true
        saveText = "Unsaved changes"
        saveTask = Task {
            try? await Task.sleep(for: .milliseconds(1500))
            guard !Task.isCancelled else { return }
            await save(core: core)
        }
    }

    /// The whole profile's system and module configuration, as Display Setup's OK wrote it, including anything an agent changed.
    private func save(core: VixenCore) async {
        guard unsaved else { return }
        unsaved = false
        saveText = "Saving…"
        for attempt in 0..<5 {
            let failure: String? = await offMain {
                do {
                    _ = try core.studioDisplayCommand(command: "profile_save", argumentsJson: #"{"confirm":true}"#)
                    return nil
                } catch {
                    return studioErrorText(error)
                }
            }
            guard let failure else {
                saveText = "All changes saved"
                return
            }
            if failure.contains("in progress"), attempt < 4 {
                try? await Task.sleep(for: .seconds(1))
                continue
            }
            unsaved = true
            saveText = "Couldn't save: \(failure)"
            return
        }
    }
}

extension SetupModel {
    /// Live Output has paused editing in the live pane: the trees still select, expand and offer their read-only menu (where an
    /// element is patched, and Test), and every control that edits is disabled. Never true for the legacy Display Setup window.
    var studioEditingPaused: Bool {
        StudioDisplayLive.model === self && StudioDisplayLive.shared.blocked != nil
    }

    /// The element menu while editing is paused: only what doesn't edit.
    func studioPausedElementMenu() -> NSMenu {
        studioPausedMenu([studioPatchMenuItem(), studioElementTestMenuItem()])
    }

    /// The controller menu while editing is paused: only Test.
    func studioPausedControllerMenu() -> NSMenu {
        studioPausedMenu([studioControllerTestMenuItem()])
    }

    private func studioPausedMenu(_ items: [NSMenuItem?]) -> NSMenu {
        let menu = NSMenu()
        menu.autoenablesItems = false
        menu.addItem(SetupMenuItem("Editing is paused while Live Output is on", enabled: false))
        let available = items.compactMap { $0 }
        if !available.isEmpty { menu.addItem(.separator()) }
        available.forEach(menu.addItem)
        return menu
    }

    private static var studioInstance: SetupModel?

    /// The one model. SwiftUI may build `SetupScene` again, and a second model would open a second working copy.
    static func studioShared(app: AppModel) -> SetupModel {
        if let studioInstance { return studioInstance }
        let made = SetupModel(app: app)
        studioInstance = made
        return made
    }

    /// Hooks the pane into the show window. Called from `SetupScene`.
    static func registerStudioPane(model: SetupModel, app: AppModel) {
        StudioDisplayLive.model = model
        StudioDisplayLive.app = app
        StudioDisplayPane.make = { AnyView(StudioDisplaySetupPane(model: model).environment(app)) }
        StudioDisplayPane.confirmLeave = { [weak model] in
            guard let model else { return true }
            await StudioDisplayLive.shared.flush(model: model, app: app)
            model.studioCloseLive()
            return true
        }
        StudioDisplayPane.flush = { [weak model] in
            guard let model, model.session != nil else { return }
            await StudioDisplayLive.shared.flush(model: model, app: app)
        }
        if UITestHarness.isActive {
            SetupTestHooks.install(model: model)
            UITestHarness.customActions["displayPaneState"] = { _ in
                let live = StudioDisplayLive.shared
                return (true, "session=\(model.session != nil) busy=\(app.isBusy) blocked=\(live.blocked ?? "no") undo=\(live.undoLabel ?? "-") redo=\(live.redoLabel ?? "-") save=\(live.saveText) notice=\(live.notice ?? "-") base=\(model.studioBaseRevision)")
            }
            UITestHarness.customActions["displayTestBlock"] = { step in
                StudioDisplayLive.shared.testBlock = step.text
                return (true, nil)
            }
            UITestHarness.customActions["displayTestCommission"] = { step in
                StudioDisplayLive.shared.testCommission = step.text.flatMap { try? JSONDecoder().decode(StudioCommissionStatus.self, from: Data($0.utf8)) }
                return (true, nil)
            }
            // What Edit ▸ Undo does with the show window key: `undo:` up the responder chain to the window, which asks its undo manager.
            UITestHarness.customActions["displayMenuUndoRedo"] = { step in
                guard let window = StudioDisplayPane.hostWindow else { return (false, "no host window") }
                let action = Selector(step.text == "redo" ? "redo:" : "undo:")
                guard window.responds(to: action) else { return (false, "window doesn't respond to \(action)") }
                let title = step.text == "redo" ? window.undoManager?.redoMenuItemTitle : window.undoManager?.undoMenuItemTitle
                window.perform(action, with: nil)
                return (true, title)
            }
            UITestHarness.customActions["displayUndo"] = { _ in
                StudioDisplayLive.shared.undo(model: model, app: app)
                return (true, nil)
            }
            UITestHarness.customActions["displayRedo"] = { _ in
                StudioDisplayLive.shared.redo(model: model, app: app)
                return (true, nil)
            }
        }
    }
}

private struct StudioTestFailure: Error, Sendable {
    let text: String
}

/// An FFI error's message off the main thread, where `userMessage` isn't available.
private nonisolated func studioErrorText(_ error: Error) -> String {
    if case FfiError.Failed(let message) = error { return message }
    return "\(error)"
}

/// Tells Display Setup which window hosts the pane, for sheets and the undo manager.
private struct StudioPaneWindowReader: NSViewRepresentable {
    final class Marker: NSView {
        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            if let window { StudioDisplayPane.hostWindow = window }
        }
    }

    func makeNSView(context: Context) -> Marker { Marker() }
    func updateNSView(_ marker: Marker, context: Context) {}
}
