import AppKit
import QuartzCore
import Observation
import SwiftUI
import UniformTypeIdentifiers
import VixenBridge
import VixenKit

/// Runs work off the main thread. Anything that can scale with the display goes through here, so a selection or an edit never holds up a frame.
func offMain<T: Sendable>(_ work: @escaping @Sendable () -> T) async -> T {
    await Task.detached(priority: .userInitiated) { work() }.value
}

/// The window's state and every action in it. Logic lives in Rust: this keeps the selection, runs the prompts in upstream's order, and tells the panels what changed.
@Observable
final class SetupModel {
    let app: AppModel

    /// Nil until the working copy has been made.
    private(set) var session: DisplaySetupSession?
    /// Bumps each time the window opens, so nothing from the last time around survives.
    private(set) var openCount = 0
    private(set) var isClosing = false
    /// A long edit is running. Shows a spinner after a moment.
    private(set) var isBusy = false

    // Prompts.
    var alert: SetupAlert?
    var sheet: SetupSheet?
    private var sheetDismissing = false
    /// The wizard's window shows this.
    var fixtureWizard: FixtureWizardModel?
    @ObservationIgnored var openWizardWindow: (() -> Void)?
    /// For measuring: opens this window again, and when the last open began.
    @ObservationIgnored var openSetupWindow: (() -> Void)?
    @ObservationIgnored private(set) var openedAt: Double = 0
    @ObservationIgnored var dismissWizardWindow: (() -> Void)?
    @ObservationIgnored var window: NSWindow?

    // Trees. Their selections change a lot and can be huge, so views read the counts and the state worked out from them.
    @ObservationIgnored let elementTree = OutlineHandle()
    @ObservationIgnored let controllerTree = OutlineHandle()
    @ObservationIgnored private(set) var elementSelection: [String] = []
    @ObservationIgnored private(set) var controllerSelection: [String] = []
    /// F81: selected outputs a collapse has hidden. They count, but rules about highlighted rows leave them out.
    @ObservationIgnored private(set) var controllerHidden: Set<String> = []
    var controllerHighlighted: [String] { controllerHidden.isEmpty ? controllerSelection : controllerSelection.filter { !controllerHidden.contains($0) } }
    private(set) var elementCount = 0
    private(set) var primaryElement: String?
    private(set) var controllerRowCount = 0

    // Elements panel.
    let templates = setupElementTemplates()
    var templateName = "Single Item"
    let helperNames = setupHelperNames()
    var helperName = setupHelperNames().first ?? ""
    private(set) var properties: [SetupPropertyRow] = []
    var propertySelection: Set<String> = []
    @ObservationIgnored private var drafts: [SetupTemplateKind: SetupTemplateDraft] = [:]
    @ObservationIgnored private var addingFromTemplate = false

    // Simple patching.
    private(set) var elementStats = SetupElementStats.none
    private(set) var controllerStats = SetupControllerStats.none
    var usesAllPoints = false { didSet { updateSummary() } }
    var usesAllOutputs = false { didSet { updateSummary() } }
    var reversesElements = false
    var reversesOutputs = false
    private(set) var summary = SetupPatchSummary.none

    // Controllers panel.
    let controllerTypes = setupControllerTypes()
    var controllerTypeID = setupControllerTypes().first?.id ?? ""
    private(set) var controllerPanel = SetupControllerPanelState.none

    // Patching view.
    var usesGraphicalView = false {
        didSet {
            if usesGraphicalView != oldValue {
                patchingViewChanged()
            }
        }
    }
    @ObservationIgnored let graph = GraphCanvasController()

    // What comes back from a background task for a selection that has since changed is dropped.
    @ObservationIgnored private var statsToken = 0
    @ObservationIgnored private var controllerToken = 0
    @ObservationIgnored private var propertiesToken = 0
    @ObservationIgnored private var menuToken = 0
    @ObservationIgnored private(set) var menuCache: (state: SetupTreeMenuState, tags: [SetupTagMenuItem], selection: [String], revision: Int)?
    @ObservationIgnored private var editRevision = 0

    init(app: AppModel) {
        self.app = app
        graph.model = self
    }

    // MARK: Opening and closing

    /// F1. The session starts here, and with it the app's system configuration session.
    func windowOpened() {
        guard session == nil, !isClosing, let core = app.core else { return }
        app.beginSystemConfigSession()
        openedAt = CACurrentMediaTime()
        openCount += 1
        resetForNewWindow()
        Task {
            let opened = await offMain { core.openDisplaySetup() }
            session = opened
            await loadTrees()
        }
    }

    private func loadTrees() async {
        // Controllers first, then elements, as `DisplaySetup_Load`.
        await controllerTree.loadRoots()
        await elementTree.loadRoots()
        refreshAfterSelection()
        scheduleControllerRefresh()
    }

    private func resetForNewWindow() {
        elementSelection = []
        controllerSelection = []
        elementCount = 0
        primaryElement = nil
        controllerRowCount = 0
        templateName = "Single Item"
        helperName = helperNames.first ?? ""
        properties = []
        propertySelection = []
        drafts = [:]
        elementStats = .none
        controllerStats = .none
        usesAllPoints = false
        usesAllOutputs = false
        reversesElements = false
        reversesOutputs = false
        summary = .none
        controllerPanel = .none
        usesGraphicalView = false
        menuCache = nil
        graph.reset()
        controllerTypeID = controllerTypes.first?.id ?? ""
    }

    /// F6. The window goes away, the Administration window shows the progress, and the working copy reaches the system off the main thread. If the save fails the window comes back with its work.
    func ok() {
        guard let session, !isClosing else { return }
        isClosing = true
        let window = window
        window?.orderOut(nil)
        let failed = FailureBox()
        let finished: @Sendable () -> Void = { [weak self] in
            Task { @MainActor [weak self] in
                self?.commitFinished(failure: failed.message)
            }
        }
        // The closure goes back to the main thread when it's done with, and must not take the last reference to the working copy there: freeing it scales with the display.
        let held = SessionHolder(session)
        app.endSystemConfigSession(.performed { report in
            guard let session = held.take() else { return }
            report(0, "Preparing Display Setup Changes")
            let poller = Task.detached {
                var last = ""
                while !Task.isCancelled {
                    let progress = session.commitProgress()
                    if progress.running, progress.text != last {
                        last = progress.text
                        report(Int32(progress.percent), progress.text)
                    }
                    try? await Task.sleep(for: .milliseconds(16))
                }
            }
            do {
                _ = try session.commit()
            } catch let failure as FfiError {
                if case .Failed(let message) = failure {
                    failed.message = message
                }
            } catch {
                failed.message = "\(error)"
            }
            poller.cancel()
            finished()
        })
    }

    private func commitFinished(failure: String?) {
        if let failure {
            // The save failed, so the system still has the old display and the working copy has the new one. Back to the window.
            isClosing = false
            app.beginSystemConfigSession()
            window?.makeKeyAndOrderFront(nil)
            if window == nil { StudioDisplayPane.reveal?() } // STUDIO: the show window's pane brings the edits back on screen
            Task {
                _ = await ask("Display Setup", "The display couldn't be saved. Your changes are still here.\n\n\(failure)", [.init(title: "OK", value: 0)])
            }
            return
        }
        closeForGood()
    }

    /// F7, for the Cancel button and Esc, which never ask.
    func cancel() {
        guard !isClosing else { return }
        finishCancelling()
        closeForGood()
    }

    private func finishCancelling() {
        isClosing = true
        guard let session else {
            app.endSystemConfigSession(.discarded(changed: false))
            return
        }
        // Only controllers started or stopped here need anything, and stopping one can take a while. Detached, so the working copy isn't freed on the main thread.
        let app = app
        Task.detached {
            session.discard()
            await MainActor.run { app.endSystemConfigSession(.discarded(changed: false)) }
        }
    }

    private func closeForGood() {
        let window = window
        window?.close()
        Self.releaseOffMain(session)
        session = nil
        isClosing = false
        fixtureWizard = nil
        dismissWizardWindow?()
    }

    /// F7, for the close box and ⌘W: the one place that asks, and only with something to lose.
    func windowShouldClose() -> Bool {
        guard !isClosing else { return true }
        guard let session, session.isDirty() else {
            cancel()
            return false
        }
        Task {
            let choice = await ask("Display Setup", "Discard changes to the display?", [
                .init(title: "Discard Changes", role: .destructive, value: 1),
                .init(title: "Keep Editing", role: .cancel, value: 0),
            ])
            if choice == 1 {
                cancel()
            }
        }
        return false
    }

    /// The window went away by a route that didn't come through here, such as the app quitting.
    func windowClosed() {
        guard session != nil, !isClosing else { return }
        finishCancelling()
        Self.releaseOffMain(session)
        session = nil
        isClosing = false
    }

    /// Keeps the working copy a moment longer on a background thread, so that whoever lets go of it last, it's freed there: for the owner's show that's a few
    /// milliseconds, for a display of 50,000 elements far more.
    private nonisolated static func releaseOffMain(_ session: DisplaySetupSession?) {
        guard let session else { return }
        Task.detached(priority: .utility) {
            try? await Task.sleep(for: .seconds(2))
            withExtendedLifetime(session) {}
        }
    }

    /// F8.
    func help() {
        let address = "https://www.vixenlights.com/docs/usage/display-setup/"
        if UITestHarness.isActive {
            UITestHarness.note("help \(address)")
        } else if let url = URL(string: address) {
            NSWorkspace.shared.open(url)
        }
    }

    // MARK: Prompts

    private func settled() async {
        // A sheet closed before SwiftUI got it on screen never reports its dismissal, so that wait gives up after a moment.
        var idle = 0
        while sheet != nil || alert != nil || (sheetDismissing && idle < 60) {
            try? await Task.sleep(for: .milliseconds(15))
            if sheet == nil, alert == nil {
                idle += 1
            }
        }
        sheetDismissing = false
    }

    /// `MessageBoxForm`: waits for an answer.
    func ask(_ title: String, _ message: String, _ choices: [SetupAlert.Choice]) async -> Int {
        await settled()
        return await withCheckedContinuation { continuation in
            alert = SetupAlert(title: title, message: message, choices: choices) { value in
                continuation.resume(returning: value)
            }
        }
    }

    func tell(_ title: String, _ message: String) async {
        _ = await ask(title, message, [.init(title: "OK", value: 0)])
    }

    func alertAnswered(_ alert: SetupAlert, _ value: Int) {
        self.alert = nil
        alert.finish(value)
    }

    func sheetDidDismiss() {
        sheetDismissing = false
    }

    private func closeSheet() {
        sheetDismissing = true
        sheet = nil
    }

    private func present<T: Sendable>(_ make: (@escaping (T) -> Void) -> SetupSheet.Kind) async -> T {
        await settled()
        return await withCheckedContinuation { continuation in
            let kind = make { [weak self] value in
                self?.closeSheet()
                continuation.resume(returning: value)
            }
            sheet = SetupSheet(kind: kind)
        }
    }

    func promptText(title: String, prompt: String, initial: String = "", selectAll: Bool = false) async -> String? {
        await present { .text(title: title, prompt: prompt, initial: initial, selectAll: selectAll, finish: $0) }
    }

    func promptNumber(title: String, prompt: String, value: Int, min: Int = 0, max: Int = Int(Int32.max)) async -> Int? {
        await present { .number(title: title, prompt: prompt, value: value, min: min, max: max, finish: $0) }
    }

    func promptList(title: String, items: [SetupModuleType]) async -> SetupModuleType? {
        await present { .list(title: title, items: items, finish: $0) }
    }

    func runDialog(_ request: SetupDialogRequest) async -> Bool {
        await present { .dialog(request, finish: $0) }
    }

    private func working<T: Sendable>(_ work: @escaping @Sendable (DisplaySetupSession) -> T) async -> T? {
        guard let session else { return nil }
        let spinner = Task { @MainActor [weak self] in
            try? await Task.sleep(for: .milliseconds(200))
            if !Task.isCancelled {
                self?.isBusy = true
            }
        }
        let result = await offMain { work(session) }
        spinner.cancel()
        isBusy = false
        return result
    }

    // MARK: Selection

    func elementSelectionChanged(_ selection: OutlineSelection) {
        elementSelection = selection.ids
        elementCount = selection.ids.count
        primaryElement = selection.primary
        refreshAfterSelection()
    }

    private func refreshAfterSelection() {
        refreshProperties()
        scheduleElementStats()
        scheduleMenuCache()
        graph.sceneNeeded(fit: true)
    }

    func controllerSelectionChanged(_ selection: OutlineSelection) {
        controllerSelection = selection.ids
        controllerHidden = Set(selection.hidden)
        controllerRowCount = selection.ids.count
        scheduleControllerRefresh()
        graph.sceneNeeded(fit: true)
    }

    /// F62. Off the main thread, with the newest selection only. The numbers on screen stay until new ones arrive.
    private func scheduleElementStats() {
        guard let session else { return }
        statsToken += 1
        let token = statsToken
        let selection = elementSelection
        Task {
            // One computation per trip round the run loop, so a held ⇧↓ never queues work.
            await Task.yield()
            guard token == statsToken else { return }
            let stats = await offMain { session.elementStats(selection: selection) }
            guard token == statsToken else { return }
            elementStats = stats
            updateSummary()
        }
    }

    /// F65, F85, F86.
    private func scheduleControllerRefresh() {
        guard let session else { return }
        controllerToken += 1
        let token = controllerToken
        let rows = controllerSelection
        Task {
            await Task.yield()
            guard token == controllerToken else { return }
            let (stats, panel) = await offMain { (session.controllerStats(rows: rows), session.controllerPanelState(rows: rows)) }
            guard token == controllerToken else { return }
            controllerStats = stats
            controllerPanel = panel
            updateSummary()
        }
    }

    private func updateSummary() {
        let options = patchOptions
        summary = setupPatchSummary(elements: elementStats, controllers: controllerStats, options: options)
    }

    private var patchOptions: SetupPatchOptions {
        SetupPatchOptions(allPoints: usesAllPoints, allOutputs: usesAllOutputs, reverseElements: reversesElements, reverseOutputs: reversesOutputs)
    }

    /// F16: the primary row's properties.
    private func refreshProperties() {
        guard let session else { return }
        propertiesToken += 1
        let token = propertiesToken
        guard let primary = primaryElement else {
            properties = []
            propertySelection = []
            return
        }
        Task {
            let rows = await offMain { session.propertiesOf(primary: primary) }
            guard token == propertiesToken else { return }
            properties = rows
            propertySelection = []
        }
    }

    /// F32 and F54 for the selection, ready before the menu is asked for, so a right-click on a huge selection has nothing to wait for.
    private func scheduleMenuCache() {
        guard let session else { return }
        menuToken += 1
        let token = menuToken
        let selection = elementTree.selectionInDisplayOrder
        let primary = primaryElement
        let revision = editRevision
        Task {
            await Task.yield()
            guard token == menuToken else { return }
            let (state, tags) = await offMain { (session.contextMenuState(selection: selection, primary: primary), session.tagMenu(selection: selection)) }
            guard token == menuToken else { return }
            menuCache = (state, tags, selection, revision)
        }
    }

    /// The menu state for right now. Asks on the spot only when the selection has just changed, in which case a right-click has made it one row.
    func elementMenuState() -> (state: SetupTreeMenuState, tags: [SetupTagMenuItem])? {
        let selection = elementTree.selectionInDisplayOrder
        if let cache = menuCache, cache.selection == selection, cache.revision == editRevision {
            return (cache.state, cache.tags)
        }
        guard let session else { return nil }
        return (session.contextMenuState(selection: selection, primary: primaryElement), session.tagMenu(selection: selection))
    }

    // MARK: What changed

    /// F4: elements changed. The patching view hears about it, and Remove and Edit (and, as the register says, every drop) refresh the controllers' patch status too.
    func elementsChanged(_ change: ElementsChange) {
        editRevision += 1
        refreshAfterSelection()
        if change == .remove || change == .edit {
            controllerTree.invalidateIcons()
            scheduleControllerRefresh()
        }
        elementTree.invalidateIcons()
    }

    /// F4: patching updated. Both trees refresh their status.
    func patchingUpdated() {
        editRevision += 1
        elementTree.invalidateIcons()
        controllerTree.invalidateIcons()
        scheduleElementStats()
        scheduleControllerRefresh()
        scheduleMenuCache()
        graph.sceneNeeded(fit: false)
    }

    /// F4: controllers changed.
    func controllersChanged() {
        editRevision += 1
        elementTree.invalidateIcons()
        scheduleElementStats()
        scheduleControllerRefresh()
        graph.sceneNeeded(fit: true)
    }

    private func patchingViewChanged() {
        // F3: the view that's showing gets the current selections at once, controllers first.
        graph.activeChanged(usesGraphicalView)
    }

    /// Applies what an edit reports: reloads exactly what changed and puts the selection where it says.
    func apply(_ edit: SetupEditResult) async {
        let reload = Set(edit.reloadNodes.map(String.init))
        let refresh = Set(edit.refreshNodes.map(String.init))
        if edit.reloadRoots || !reload.isEmpty {
            await elementTree.reload(roots: edit.reloadRoots) { reload.contains(lastComponent(of: $0)) }
        }
        if !refresh.isEmpty {
            await elementTree.refresh { refresh.contains(lastComponent(of: $0)) }
        }
        if edit.changesSelection {
            await elementTree.select(edit.select, expanding: edit.expand, scrollToTop: edit.scrollToTop)
        } else if !edit.expand.isEmpty {
            await elementTree.expand(edit.expand)
        }
    }

    // MARK: Elements panel

    var selectedTemplate: SetupTemplateInfo? {
        templates.first { $0.name == templateName } ?? templates.first
    }

    /// F10, F100.
    func addFromTemplate() async {
        guard let info = selectedTemplate, !addingFromTemplate else { return }
        addingFromTemplate = true
        defer { addingFromTemplate = false }
        if info.kind == .intelligentFixture {
            await addFixture()
            return
        }
        let kind = info.kind
        guard let draft = drafts[kind] ?? setupTemplateDraft(kind: kind) else { return }
        // F99: each template remembers its last values for as long as the window is open, Cancel included.
        let chosen: SetupTemplateDraft? = await present { [weak self] finish in
            .template(draft: draft, remember: { self?.drafts[kind] = $0 }, finish: finish)
        }
        guard let chosen else { return }
        drafts[kind] = chosen
        let outcome = await working { session in Result { try session.generateTemplate(draft: chosen) } }
        guard case .success(let made)? = outcome else {
            await tell("Error", "Could not create elements.\nPlease close all dialogs and try again.\n\nPlease send error logs to Vixen Team.")
            return
        }
        let nodes = (await working { session in session.nodeIdsOf(selection: made.createdPaths) }) ?? []
        // Cancelling either helper doesn't remove what was made.
        if info.askDimming {
            let answer = await ask("Dimming Curve Setup", "Would you like to configure a dimming curve for this Prop?", [
                .init(title: "Configure", value: 1),
                .init(title: "Not Now", role: .cancel, value: 0),
            ])
            if answer == 1 {
                _ = await runDialog(.helper(name: "Dimming Curve", nodes: nodes, simple: true, startFullColor: false))
            }
        }
        if info.runColor {
            _ = await runDialog(.helper(name: "Color Handling", nodes: nodes, simple: false, startFullColor: true))
        }
        await apply(made.edit)
        elementsChanged(.add)
    }

    /// F110: the wizard, in a window of its own.
    private func addFixture() async {
        guard let session else { return }
        let edit: SetupEditResult? = await withCheckedContinuation { continuation in
            fixtureWizard = FixtureWizardModel(session: session) { [weak self] edit in
                self?.fixtureWizard = nil
                self?.dismissWizardWindow?()
                continuation.resume(returning: edit)
            }
            openWizardWindow?()
        }
        if let edit {
            await apply(edit)
            elementsChanged(.add)
        }
    }

    /// F12, F28, F42.
    func deleteElements() async {
        let rows = elementTree.selectionInDisplayOrder
        guard !rows.isEmpty else { return }
        let answer = await ask("Delete Elements?", "Are you sure you want to delete these element(s)?", [
            .init(title: "Delete", role: .destructive, value: 1),
            .init(title: "Cancel", role: .cancel, value: 0),
        ])
        guard answer == 1, let edit = await working({ $0.deleteRows(selection: rows) }) else { return }
        await apply(edit)
        elementsChanged(.remove)
    }

    /// F13, F46, F49.
    func renameElements() async {
        let rows = elementTree.selectionInDisplayOrder
        guard let session, !rows.isEmpty else { return }
        let names = await offMain { session.namesOf(selection: rows) }
        var newNames: [String]?
        if rows.count == 1 {
            guard let text = await promptText(title: "Rename item", prompt: "Item name?", initial: names.first ?? "", selectAll: true),
                  !text.isEmpty, text != names.first else { return }
            newNames = [text]
        } else {
            newNames = await present { .nameGenerator(seed: "NewName", oldNames: names, finish: $0) }
        }
        guard let newNames, let edit = await working({ $0.renameRows(selection: rows, names: newNames) }) else { return }
        await apply(edit)
        elementsChanged(.rename)
    }

    /// F14.
    func findOutputs() async {
        let rows = elementTree.selectionInDisplayOrder
        guard !rows.isEmpty, let reveal = await working({ $0.findOutputs(selection: rows) }) else { return }
        if reveal.select.isEmpty {
            await tell("Not Found", "No controller patch points found.")
        } else {
            await controllerTree.select(reveal.select, expanding: reveal.expand, scrollToTop: true)
        }
    }

    /// F15.
    func runHelper() async {
        let rows = elementTree.selectionInDisplayOrder
        guard let session, !rows.isEmpty else { return }
        let nodes = await offMain { session.nodeIdsOf(selection: rows) }
        _ = await runDialog(.helper(name: helperName, nodes: nodes, simple: false, startFullColor: false))
        elementsChanged(.edit)
    }

    /// F17.
    func addProperty() async {
        let rows = elementTree.selectionInDisplayOrder
        guard !rows.isEmpty, let choice = await promptList(title: "Add Property", items: setupPropertyTypes()) else { return }
        _ = await working { session in try? session.addProperty(selection: rows, typeId: choice.id) }
        elementsChanged(.edit)
    }

    /// F18.
    func deleteProperties() async {
        let rows = elementTree.selectionInDisplayOrder
        let ids = Array(propertySelection)
        guard !rows.isEmpty, !ids.isEmpty else { return }
        let one = ids.count == 1
        let answer = await ask(
            one ? "Remove Property?" : "Remove Properties?",
            one ? "Are you sure you want to remove the selected property from the element?" : "Are you sure you want to remove the selected properties from the element?",
            [.init(title: "Remove", role: .destructive, value: 1), .init(title: "Cancel", role: .cancel, value: 0)]
        )
        guard answer == 1 else { return }
        _ = await working { $0.removeProperties(selection: rows, typeIds: ids) }
        elementsChanged(.edit)
    }

    /// F19. A property with neither a setup nor a helper does nothing.
    func configureProperty() async {
        guard propertySelection.count == 1, let typeID = propertySelection.first, let primary = primaryElement,
              let row = properties.first(where: { $0.typeId == typeID }), row.isColor || row.hasSetup || row.hasHelper,
              let session else { return }
        let rows = elementTree.selectionInDisplayOrder
        let (selection, element) = await offMain { (session.nodeIdsOf(selection: rows), session.nodeIdsOf(selection: [primary]).first) }
        guard let element else { return }
        let changed = await runDialog(.property(element: element, selection: selection, typeID: typeID, typeName: row.typeName))
        if changed || row.isColor {
            elementsChanged(.edit)
        }
    }

    // MARK: Element tree commands

    /// Asks F43 for a destination, true if it's fine to go on. Anything but the verb aborts.
    private func checkLosesPatches(_ destination: String?) async -> Bool {
        guard let session else { return false }
        let loses = await offMain { session.wouldLosePatches(destination: destination) }
        guard loses else { return true }
        let answer = await ask(
            "Convert Element to Group?",
            "Adding items to this element will convert it into a Group, which will remove any patches it may have. Are you sure you want to continue?",
            [.init(title: "Convert to Group", role: .destructive, value: 1), .init(title: "Cancel", role: .cancel, value: 0)]
        )
        return answer == 1
    }

    /// F40.
    func addElement() async {
        let parent = primaryElement
        guard let name = await promptText(title: "", prompt: "Element Name?"), await checkLosesPatches(parent),
              let edit = await working({ $0.addElement(parent: parent, name: name) }) else { return }
        await apply(edit)
        elementsChanged(.add)
    }

    /// F41.
    func addMultiple() async {
        let parent = primaryElement
        var seed = "NewName"
        if let parent, let session {
            seed = (await offMain { session.namesOf(selection: [parent]) }).first ?? seed
        }
        let chosen = seed
        guard let names: [String] = await present({ .nameGenerator(seed: chosen, oldNames: nil, finish: $0) }) else { return }
        guard await checkLosesPatches(parent), let edit = await working({ $0.addElements(parent: parent, names: names) }) else { return }
        guard !edit.select.isEmpty else {
            await tell("", "Could not create elements.  Ensure you use a valid name and try again.")
            return
        }
        await apply(edit)
        if let last = edit.select.last {
            elementTree.reveal(last)
        }
        elementsChanged(.add)
    }

    /// F45.
    func createGroup() async {
        let rows = elementTree.selectionInDisplayOrder
        guard !rows.isEmpty, let name = await promptText(title: "", prompt: "Element Name?"),
              let edit = await working({ $0.createGroup(selection: rows, name: name) }) else { return }
        await apply(edit)
        elementsChanged(.add)
    }

    /// F47.
    func findReplace() async {
        let rows = elementTree.selectionInDisplayOrder
        guard let session, !rows.isEmpty else { return }
        let names = await offMain { session.namesOf(selection: rows) }
        guard let newNames: [String] = await present({ .renamer(oldNames: names, finish: $0) }),
              let edit = await working({ $0.renameRows(selection: rows, names: newNames) }) else { return }
        await apply(edit)
        elementsChanged(.rename)
    }

    /// F34.
    func copyElements() async {
        let rows = elementTree.selectionInDisplayOrder
        _ = await working { $0.copyRows(selection: rows) }
        editRevision += 1
        scheduleMenuCache()
    }

    /// F35, F37.
    func paste(asNew: Bool) async {
        let destination = primaryElement
        guard let session else { return }
        let can = await offMain { session.canPaste(destination: destination) }
        guard (asNew ? can[safe: 1] : can[safe: 0]) == true else {
            NSSound.beep()
            return
        }
        guard await checkLosesPatches(destination),
              let edit = await working({ asNew ? $0.pasteAsNew(destination: destination) : $0.pasteLinks(destination: destination) }) else { return }
        await apply(edit)
        elementsChanged(.add)
    }

    /// F38.
    func copyProperties() async {
        guard let primary = primaryElement else { return }
        _ = await working { $0.copyProperties(path: primary) }
        editRevision += 1
        scheduleMenuCache()
    }

    /// F39.
    func pasteProperties() async {
        let rows = elementTree.selectionInDisplayOrder
        _ = await working { $0.pasteProperties(selection: rows) }
        elementsChanged(.edit)
    }

    /// F50.
    func reverseElements() async {
        let rows = elementTree.selectionInDisplayOrder
        guard let edit = await working({ $0.reverseRows(selection: rows) }) else { return }
        await apply(edit)
        elementsChanged(.edit)
    }

    /// F51.
    func sortElements() async {
        guard let row = elementTree.selectionInDisplayOrder.first, let edit = await working({ $0.sortGroup(path: row) }) else { return }
        await apply(edit)
        elementsChanged(.edit)
    }

    /// F48.
    func collapseAll() {
        elementTree.collapseAll()
    }

    /// F52, F128, F131.
    func exportWireDiagram(flip: Bool) async {
        guard let row = elementTree.selectionInDisplayOrder.first, let session else { return }
        let message = "Every element in the group needs a Location property to export a wire diagram."
        guard await offMain({ session.wireDiagramHasLocations(groupPath: row) }) else {
            await tell("Export Wire Diagram", message)
            return
        }
        guard let options: SetupWireOptions = await present({ .wireDiagram(finish: $0) }) else { return }
        let result = await offMain { Result { try session.exportWireDiagram(groupPath: row, flip: flip, options: options) } }
        switch result {
        case .success(let path?):
            if UITestHarness.isActive {
                UITestHarness.note("wire diagram \(path)")
            } else {
                NSWorkspace.shared.open(URL(fileURLWithPath: path))
            }
        case .success(nil):
            await tell("Export Wire Diagram", message)
        case .failure(let error):
            await tell("Export Wire Diagram", error.userMessage)
        }
    }

    /// F53.
    func exportElementTree() async {
        guard let session else { return }
        var destination: URL?
        if UITestHarness.isActive {
            let folder = ProcessInfo.processInfo.environment["VIXEN_UI_TEST_OUT"] ?? NSTemporaryDirectory()
            destination = URL(fileURLWithPath: folder).appending(path: "tree.v3e")
        } else {
            let panel = NSSavePanel()
            panel.allowedContentTypes = [UTType(filenameExtension: "v3e") ?? .json]
            panel.canCreateDirectories = true
            panel.title = "Export Element Tree"
            panel.message = "Export the element tree for importing into other profiles."
            if let root = app.dataRoot {
                panel.directoryURL = URL(fileURLWithPath: root).appending(path: "Sequence")
            }
            if let window = window ?? StudioDisplayPane.hostWindow { // STUDIO: attach to the show window when Display Setup is its pane
                let answer = await panel.beginSheetModal(for: window)
                destination = answer == .OK ? panel.url : nil
            } else {
                destination = panel.runModal() == .OK ? panel.url : nil
            }
        }
        guard let destination else { return }
        let path = destination.path
        if case .failure(let error) = await offMain({ Result { try session.exportElementTree(path: path) } }) {
            await tell("Export Element Tree", error.userMessage)
        } else if UITestHarness.isActive {
            UITestHarness.note("element tree \(path)")
        }
    }

    /// F55.
    func toggleTag(_ id: String, cascade: Bool) async {
        let rows = elementTree.selectionInDisplayOrder
        guard let touched = await working({ try? $0.toggleTag(selection: rows, tag: id, cascade: cascade) }), let nodes = touched else { return }
        let set = Set(nodes.map(String.init))
        await elementTree.refresh { set.contains(lastComponent(of: $0)) }
        editRevision += 1
        scheduleMenuCache()
    }

    /// F56, F132.
    func manageTags() async {
        guard let session else { return }
        let rows = await offMain { session.tagColors() }
        guard let colors: [SetupTagColor] = await present({ .tagColors(rows: rows, finish: $0) }) else { return }
        _ = await working { $0.setTagColors(colors: colors) }
        await elementTree.refresh { _ in true }
        editRevision += 1
        scheduleMenuCache()
    }

    // MARK: Element tree drag and drop

    func canDrop(items: [String], parent: String?, index: Int?, copy: Bool) -> Bool {
        session?.canDropRows(sources: items, parent: parent, index: index.map { UInt32($0) }, copy: copy) ?? false
    }

    /// F57, F58. The register: every drop refreshes patch status and the patching view.
    func drop(items: [String], parent: String?, index: Int?, copy: Bool) async {
        guard await checkLosesPatches(parent),
              let edit = await working({ $0.dropRows(sources: items, parent: parent, index: index.map { UInt32($0) }, copy: copy) }) else { return }
        await apply(edit)
        elementsChanged(.remove)
    }

    // MARK: Simple patching

    /// F63.
    func unpatchElements() async {
        let selection = elementSelection
        guard let preview = await working({ $0.unpatchElementsPreview(selection: selection) }) else { return }
        if preview.patched > setupUnpatchPromptThreshold() {
            let answer = await ask("Unpatch Elements?", "Are you sure you want to unpatch \(preview.patched) patch points?", [
                .init(title: "Unpatch", role: .destructive, value: 1),
                .init(title: "Cancel", role: .cancel, value: 0),
            ])
            guard answer == 1 else { return }
        }
        var removeFilters = false
        if preview.anyFilters {
            let answer = await ask("Remove Filters?", "Some elements are patched to filters.  Should these filters be removed as well?", [
                .init(title: "Remove Filters", role: .destructive, value: 1),
                .init(title: "Keep Filters", value: 2),
                .init(title: "Cancel", role: .cancel, value: 0),
            ])
            guard answer != 0 else { return }
            removeFilters = answer == 1
        }
        let remove = removeFilters
        _ = await working { $0.unpatchElements(selection: selection, removeFilters: remove) }
        patchingUpdated()
    }

    /// F66, F96, F126: always the live selection, whichever patching view is showing.
    func unpatchControllers() async {
        let rows = controllerSelection
        guard let stats = await working({ $0.controllerStats(rows: rows) }), stats.patched > 0 else { return }
        if stats.patched > setupUnpatchPromptThreshold() {
            let answer = await ask("Unpatch Controllers?", "Are you sure you want to unpatch \(stats.patched) patch points?", [
                .init(title: "Unpatch", role: .destructive, value: 1),
                .init(title: "Cancel", role: .cancel, value: 0),
            ])
            guard answer == 1 else { return }
        }
        _ = await working { $0.unpatchOutputs(rows: rows) }
        patchingUpdated()
    }

    /// F69.
    func patchElements() async {
        let selection = elementSelection
        let rows = controllerSelection
        let options = patchOptions
        guard let check = await working({ $0.orderCheck(selection: selection) }) else { return }
        var useOrder = false
        switch check {
        case .treeOrder:
            break
        case .useOrder:
            useOrder = true
        case .missingSome, .duplicates:
            let lead = check == .missingSome
                ? "Not all elements have a preferred patching order set. Would you like to fix this before proceding?"
                : "Some elements have a duplicate preferred patching order set. Would you like to fix this before proceeding?"
            let answer = await ask("Patching Order Conflict", lead + "\n\nYes) Open the order wizard.\nNo) Proceed using the element tree order.\nCancel) Abort patching.", [
                .init(title: "Open Order Wizard", value: 1),
                .init(title: "Use Tree Order", value: 2),
                .init(title: "Cancel", role: .cancel, value: 0),
            ])
            switch answer {
            case 0:
                return
            case 1:
                guard let session else { return }
                let nodes = await offMain { session.nodeIdsOf(selection: selection) }
                guard await runDialog(.helper(name: "Patching Order", nodes: nodes, simple: false, startFullColor: false)) else { return }
                useOrder = true
            default:
                break
            }
        }
        let sorted = useOrder
        guard let count = await working({ $0.patchElements(selection: selection, rows: rows, options: options, useOrder: sorted) }) else { return }
        patchingUpdated()
        await tell("Patching Complete", "Patched \(count) element patch points to controllers.")
    }

    // MARK: Controllers panel

    /// Reloads the controller tree after an edit.
    func reloadControllers() async {
        await controllerTree.reload(roots: true) { _ in true }
    }

    /// F77.
    func addController() async {
        guard let type = controllerTypes.first(where: { $0.id == controllerTypeID }),
              let name = await promptText(title: "Controller Name", prompt: "New Controller Name?", initial: type.name, selectAll: true) else { return }
        let limit = setupOutputLimit(typeId: type.id)
        guard let count = await promptNumber(title: "Controller Output Count", prompt: "Outputs on this controller?", value: 1, min: 1, max: Int(limit)) else { return }
        let typeID = type.id
        let result = await working { session in Result { try session.addController(typeId: typeID, name: name, outputs: UInt32(count)) } }
        switch result {
        case .success(let row)?:
            await controllerTree.reload(roots: true) { _ in false }
            await controllerTree.select([row], scrollToTop: true)
            controllersChanged()
        case .failure(let error)?:
            await tell("Add Controller", error.userMessage)
        case nil:
            break
        }
    }

    /// F78.
    func moveControllers(up: Bool) async {
        let rows = controllerSelection
        _ = await working { $0.moveControllers(rows: rows, up: up) }
        await reloadControllers()
    }

    /// F87.
    func configureController() async {
        let rows = controllerSelection
        guard let info = await working({ $0.firstControllerInfo(rows: rows) }), let info, info.hasSetup else { return }
        if await runDialog(.controller(id: info.id, name: info.name, typeName: info.typeName)) {
            await reloadControllers()
            controllersChanged()
        }
    }

    /// F88.
    func channelCount() async {
        let rows = controllerSelection
        guard let info = await working({ $0.firstControllerInfo(rows: rows) }), let info,
              let count = await promptNumber(title: "Controller Output Count", prompt: "Outputs on this controller?", value: Int(info.outputCount)) else { return }
        _ = await working { try? $0.setOutputCount(id: info.id, count: UInt32(count)) }
        await reloadControllers()
        controllersChanged()
    }

    /// F89.
    func renameController() async {
        let rows = controllerSelection
        guard let info = await working({ $0.firstControllerInfo(rows: rows) }), let info,
              let name = await promptText(title: "Controller Name", prompt: "Controller Name?", initial: info.name, selectAll: true), !name.isEmpty else { return }
        _ = await working { try? $0.renameController(id: info.id, name: name) }
        await reloadControllers()
        controllersChanged()
    }

    /// F90.
    func deleteControllers() async {
        let rows = controllerSelection
        guard let panel = await working({ $0.controllerMenuState(rows: rows) }), panel.controllerItems else { return }
        let one = await working({ $0.controllerPanelState(rows: rows) })?.oneController ?? false
        let answer = await ask(
            one ? "Delete Controller?" : "Delete Controllers?",
            one ? "Are you sure you want to delete the selected controller?" : "Are you sure you want to delete the selected controllers?",
            [.init(title: "Delete", role: .destructive, value: 1), .init(title: "Cancel", role: .cancel, value: 0)]
        )
        guard answer == 1 else { return }
        _ = await working { $0.deleteControllers(rows: rows) }
        await reloadControllers()
        await controllerTree.select([])
        controllersChanged()
        patchingUpdated()
    }

    /// F91.
    func startControllers() async {
        let rows = controllerSelection
        _ = await working { $0.startControllers(rows: rows) }
        await controllerTree.refresh { $0.hasPrefix("c") }
        scheduleControllerRefresh()
        controllersChanged()
    }

    func stopControllers() async {
        let rows = controllerSelection
        _ = await working { $0.stopControllers(rows: rows) }
        await controllerTree.refresh { $0.hasPrefix("c") }
        scheduleControllerRefresh()
        controllersChanged()
    }

    /// F84, F97.
    func findElements() async {
        let rows = controllerSelection
        guard let edit = await working({ $0.findElements(rows: rows) }) else { return }
        if edit.changesSelection, !edit.select.isEmpty {
            await apply(edit)
        } else {
            await tell("Not Found", "No element patch points found.")
        }
    }

    /// F94.
    func insertChannels() async {
        guard let row = controllerSelection.first,
              let count = await promptNumber(title: "Insert Outputs", prompt: "Number of outputs to insert.", value: 10) else { return }
        _ = await working { $0.insertOutputs(outputRow: row, count: UInt32(count)) }
        await reloadControllers()
        controllersChanged()
    }

    /// F95: the highlighted rows. Only their highlight goes: the same (controller, index) pairs are selected again, so the outputs that move up into them are, and an index
    /// past the new end drops out.
    func removeChannels() async {
        let rows = controllerHighlighted
        let logical = controllerSelection
        _ = await working { $0.removeSelectedOutputs(rows: rows) }
        await reloadControllers()
        await controllerTree.select(logical)
        controllersChanged()
    }

    /// F79.
    func dropControllers(items: [String], index: Int) async {
        _ = await working { $0.dropControllers(rows: items, index: UInt32(index)) }
        await reloadControllers()
    }
}

/// Holds what a background task found. Only read after that task has finished.
nonisolated private final class FailureBox: @unchecked Sendable {
    var message: String?
}

/// Hands the working copy to the commit once, so the closure carrying it holds nothing afterwards.
nonisolated private final class SessionHolder: @unchecked Sendable {
    private let lock = NSLock()
    private var session: DisplaySetupSession?

    init(_ session: DisplaySetupSession) {
        self.session = session
    }

    func take() -> DisplaySetupSession? {
        lock.lock()
        defer { lock.unlock() }
        let taken = session
        session = nil
        return taken
    }
}

func lastComponent(of path: String) -> String {
    guard let slash = path.lastIndex(of: "/") else { return path }
    return String(path[path.index(after: slash)...])
}

extension Array {
    subscript(safe index: Int) -> Element? {
        indices.contains(index) ? self[index] : nil
    }
}
