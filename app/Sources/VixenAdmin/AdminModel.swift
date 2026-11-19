import AppKit
import SwiftUI
import VixenBridge
import VixenKit

// Objective-C invokes this callback on the thread that raised the exception.
// A closure created in AdminModel.start inherits MainActor isolation and traps
// before logging when a background thread throws.
nonisolated private func logUncaughtException(_ exception: NSException) {
    fatalExit(message: "\(exception.name.rawValue): \(exception.reason ?? "")\n\(exception.callStackSymbols.joined(separator: "\n"))")
}

enum AdminSheet: Identifiable, Equatable {
    case selectProfile
    case dataProfile
    case setupPreviews
    case checkForUpdates
    case webServer

    var id: Self { self }
}

/// What the Administration window shows and does: `VixenApplication` without the form. The rules are in Rust, so this mostly sequences calls off
/// the main actor and presents what comes back.
@Observable
final class AdminModel {
    let app: AppModel
    let alerts = AlertPresenter()
    let cpu = CpuReadout()
    let version: VersionInfoRecord

    private(set) var title = "Vixen Administration"
    private(set) var profileName: String? // STUDIO: the show window titles itself with the profile
    private(set) var progressValue = 0.0
    private(set) var progressText = ""
    private(set) var progressVisible = true
    private(set) var executionStatus = ExecutionStatusRecord(
        text: "Execution: Closed", lightRed: 178, lightGreen: 34, lightBlue: 34, startEnabled: true, stopEnabled: false
    )
    private(set) var updateNotice = ""
    private(set) var recent: [RecentSequenceRow] = []
    private(set) var logFiles: [String] = []
    private(set) var startupFinished = false
    private(set) var webServer: WebServerSession?
    /// A save or reload is running.
    private(set) var working = false

    var sheet: AdminSheet? {
        didSet { updateBusy() }
    }

    /// The sequence types New Sequence... can make.
    let creatableTypes = sequenceTypes().filter(\.canCreateNew)

    @ObservationIgnored var openWindow: OpenWindowAction?
    @ObservationIgnored var window: NSWindow?
    @ObservationIgnored private var closeDelegate: CloseQuitsDelegate?
    @ObservationIgnored private var started = false
    @ObservationIgnored private var terminating = false
    /// Shutdown has started, so state changes no longer repaint the status bar.
    @ObservationIgnored private var stopping = false
    @ObservationIgnored private var lockPath: String?
    @ObservationIgnored private var pendingFiles: [URL] = []
    /// The latest recent-list save, which shutdown waits for.
    @ObservationIgnored private var recentSave: Task<Void, Never>?
    @ObservationIgnored private var lastOpenFolder: URL?
    @ObservationIgnored private var selectProfileContinuation: CheckedContinuation<ProfileRow, Never>?
    @ObservationIgnored private let bundleBuild: String
    @ObservationIgnored private lazy var updateChecker: UpdateChecker = makeUpdateChecker()

    init(app: AppModel) {
        self.app = app
        let build = Bundle.main.object(forInfoDictionaryKey: "CFBundleVersion") as? String ?? "0"
        bundleBuild = build
        version = versionInfo(bundleBuild: build)
    }

    var isBusy: Bool { app.isBusy }

    /// Help's items appear only once startup has finished, upstream.
    var isHelpAvailable: Bool { startupFinished }

    private func updateBusy() {
        app.administrationBusy = !startupFinished || sheet != nil || working
    }

    private func setProgress(_ percent: Int32, _ text: String) {
        progressValue = Double(percent) / 100
        progressText = text
        app.reportStartup(progress: progressValue, message: text)
    }

    // MARK: Launch

    func attach(window: NSWindow) {
        guard self.window !== window else { return }
        self.window = window
        let delegate = CloseQuitsDelegate(original: window.delegate)
        closeDelegate = delegate
        window.delegate = delegate
    }

    /// F1: the whole launch, in the original's order.
    func start() async {
        guard !started else { return }
        started = true
        updateBusy()
        AppHooks.shouldTerminate = { [weak self] in self?.shouldTerminate() ?? .terminateNow }
        app.sequenceClosed = { [weak self] path in self?.sequenceClosed(path: path) }
        app.systemConfigSessionEnded = { [weak self] outcome in
            Task { await self?.finishDisplaySetup(outcome) }
        }
        NSSetUncaughtExceptionHandler(logUncaughtException)

        startLogging()
        progressText = "Starting up"
        let options = parseLaunchOptions(args: CommandLine.arguments)
        var profile: (folder: String, name: String)?
        if options.dataDir == nil {
            logInfo(logger: "VixenApplication.VixenApplication", message: "Processing Profiles")
            profile = await resolveProfile()
            logInfo(logger: "VixenApplication.VixenApplication", message: "Finished Processing Profiles")
        }
        if let profile {
            title = "Vixen Administration - \(profile.name) Profile"
            profileName = profile.name // STUDIO: the show window titles itself with the profile
            logInfo(logger: "VixenApplication.VixenApplication", message: "Profile root : \(profile.folder)")
        }

        let folder = resolveLaunchFolder(options: options, chosen: profile?.folder)
        let lock = await takeLock(folder: folder)
        lockPath = lock

        logInfo(
            logger: "VixenApplication.VixenApplication",
            message: version.isTestBuild ? version.versionName : "\(version.versionName) - \(version.buildName)"
        )
        // Upstream shows this on `Shown`, which comes after the lock check, so a locked profile exits without it.
        if version.isDevBuild {
            await alerts.tell(
                "Development/Test Software",
                "Please be aware that this is a development version. Some parts of the software may not work, and data loss is possible! Please backup your data before using this version of the software."
            )
        }
        setProgress(10, "Starting up")
        let name = profile?.name
        let listener = ProgressForwarder(model: self)
        do {
            let core = try await Task.detached {
                try VixenCore.start(root: folder, profileName: name, options: options, lockPath: lock, progress: listener)
            }.value
            lockPath = nil
            await finishStartup(core: core)
        } catch {
            // Anything that stops the load reaches here. The original can lose these and carry on half-loaded.
            app.startupFailed(error.userMessage)
            await alerts.tell("Error", "An error occurred starting the system and the application will be halted.", critical: true)
            if let lockPath { _ = removeLockFile(path: lockPath) }
            lockPath = nil
            exit(0)
        }
    }

    fileprivate func reportProgress(_ percent: Int32, _ text: String) {
        setProgress(percent, text)
    }

    private func finishStartup(core: VixenCore) async {
        app.adopt(core: core)
        if UITestHarness.isActive { UITestHarness.beforeExit = { core.studioStopMcp() } } // STUDIO: a script's quit leaves no MCP socket behind
        let wwwroot = Bundle.main.resourceURL!.appendingPathComponent("wwwroot").path
        webServer = await Task.detached { core.webServer(wwwroot: wwwroot) }.value
        core.subscribeExecution(listener: ExecutionForwarder(model: self))
        refreshExecutionStatus()
        setProgress(80, "Initializing editors")
        cpu.start()
        setProgress(90, "Finalizing")
        logFiles = listLogFiles()
        setProgress(100, "Ready")
        progressVisible = false
        await refreshRecents()
        startupFinished = true
        if UITestHarness.isActive {
            UITestHarness.hooks["admin.openSequence"] = { [weak self] args in
                guard let self, let path = args.first, FileManager.default.fileExists(atPath: path) else { return "FAIL needs an existing sequence path" }
                self.openFiles([URL(fileURLWithPath: path)])
                return "requested \(path)"
            }
            UITestHarness.hooks["admin.fatal"] = { [weak self] args in
                guard args.first == "raise",
                      ProcessInfo.processInfo.environment["VIXEN_TEST_FATAL"] == "uncaught-exception",
                      let self, let core = self.app.core,
                      let root = ProcessInfo.processInfo.environment["VIXEN_DATA"], root == core.dataRoot(),
                      FileManager.default.fileExists(atPath: URL(fileURLWithPath: root).appendingPathComponent(".lock").path)
                else { return "FAIL fatal fixture requires explicit test mode and a locked private profile" }
                logInfo(logger: "VixenFatalPathFixture", message: "Profile lock present before deliberate native exception")
                // A dedicated thread leaves the exception uncaught by AppKit's event dispatch.
                // This reaches the production NSSetUncaughtExceptionHandler installed in start().
                Thread.detachNewThread {
                    Thread.sleep(forTimeInterval: 0.2)
                    NSException(name: NSExceptionName("VixenFatalPathFixture"),
                                reason: "Deliberate exception in disposable UI-test process", userInfo: nil).raise()
                }
                return "scheduled disposable native exception"
            }
            UITestHarness.hooks["admin.webServer"] = { [weak self] args in
                guard let self, let server = self.webServer else { return "unavailable" }
                if args.first == "open" { self.sheet = .webServer }
                let settings = server.settings()
                return "port=\(settings.port) enabled=\(settings.enabled) running=\(settings.running) error=\(settings.error ?? "")"
            }
        }
        updateBusy()
        if !UITestHarness.isActive {
            NSApp.activate()
        }
        let queued = pendingFiles
        pendingFiles = []
        openFiles(queued)
    }

    /// F3. Returns the profile to load, or nil to fall through to the default folder.
    private func resolveProfile() async -> (folder: String, name: String)? {
        switch await Task.detached(operation: { decideStartupProfile() }).value {
        case .noProfiles:
            return nil
        case .load(let folder, let name):
            return (folder, name)
        case .ask(let error):
            if let error, await showLoadError(error) {
                return (error.folder, error.name)
            }
            return await askForProfile()
        }
    }

    /// F3's loop: keep asking until a profile passes its checks.
    private func askForProfile() async -> (folder: String, name: String) {
        while true {
            let row = await withCheckedContinuation { continuation in
                selectProfileContinuation = continuation
                sheet = .selectProfile
            }
            // A sheet has to be gone before an alert can show.
            try? await Task.sleep(for: .milliseconds(350))
            let (folder, name) = (row.dataFolder, row.name)
            guard let problem = await Task.detached(operation: { checkProfile(folder: folder, name: name) }).value else {
                return (folder, name)
            }
            if await showLoadError(problem) {
                return (folder, name)
            }
        }
    }

    /// Select a Profile's Load.
    func profileChosen(_ row: ProfileRow) {
        do {
            try markProfileLoaded(number: row.number)
        } catch {
            logError(logger: "VixenApplication.SelectProfile", message: "Couldn't record when the profile was loaded.", error: error.userMessage)
        }
        sheet = nil
        selectProfileContinuation?.resume(returning: row)
        selectProfileContinuation = nil
    }

    /// F4. True if the user opened it anyway, which only a lock from another machine offers, since that machine will never clear it.
    private func showLoadError(_ problem: ProfileLoadProblem) async -> Bool {
        guard let machine = problem.lockedByMachine else {
            await alerts.tell("Error", problem.message, critical: true)
            return false
        }
        let openAnyway = "Open Anyway"
        let answer = await alerts.ask(
            "Error",
            "\(problem.message)\n\nThe lock names the machine \(machine). If Vixen isn't running there, you can open this profile anyway.",
            [.init("OK"), .init(openAnyway)],
            critical: true
        )
        guard answer == openAnyway else { return false }
        return clearForeignLock(folder: problem.folder)
    }

    /// F6: a profile in use, or a lock that can't be made, ends the launch.
    private func takeLock(folder: String) async -> String {
        let attempt: @Sendable () -> String? = {
            if isProfileLocked(folder: folder) { return nil }
            return try? createLockFile(folder: folder)
        }
        if let lock = await Task.detached(operation: attempt).value {
            return lock
        }
        let message = "Profile is already in use or unable to the lock the profile."
        if let machine = foreignLockMachine(folder: folder) {
            let openAnyway = "Open Anyway"
            let answer = await alerts.ask(
                "Error",
                "\(message)\n\nThe lock names the machine \(machine). If Vixen isn't running there, you can open this profile anyway.",
                [.init("Quit Vixen", role: .cancel), .init(openAnyway)],
                critical: true
            )
            if answer == openAnyway, clearForeignLock(folder: folder), let lock = await Task.detached(operation: attempt).value {
                return lock
            }
        } else {
            await alerts.tell("Error", message, critical: true)
        }
        exit(0)
    }

    // MARK: Shutdown

    /// F12. Later: NO really cancels, which upstream's close can't do once its handler has yielded.
    private func shouldTerminate() -> NSApplication.TerminateReply {
        guard !terminating else { return .terminateLater }
        terminating = true
        Task {
            let proceed = await runShutdown()
            terminating = proceed
            NSApp.reply(toApplicationShouldTerminate: proceed)
        }
        return .terminateLater
    }

    private func runShutdown() async -> Bool {
        guard await app.prepareDocumentsForShutdown() else { return false }
        if app.openEditors.contains(where: { $0.isModified }), await !confirmClosingEditors() {
            return false
        }
        for editor in app.openEditors {
            await editor.closeForShutdown()
        }
        // Each close adds its file to the recent list, which upstream saves before it stops anything.
        await recentSave?.value
        guard let core = app.core else {
            if let lockPath { _ = removeLockFile(path: lockPath) }
            lockPath = nil
            return true
        }
        // Quitting closes a Settings window that's still open, and its changes save as closing it would.
        await savePendingOptions()
        await StudioDisplayPane.flush?() // STUDIO: the live Display pane publishes and saves its last edit
        while core.isSaving() {
            logWarn(logger: "VixenApplication.VixenApplication", message: "Waiting for save to finish before closing.")
            try? await Task.sleep(for: .milliseconds(250))
        }
        stopping = true
        cpu.stop()
        await Task.detached { core.shutdown() }.value
        return true
    }

    /// An `NSAlert` on whatever is frontmost in the window, because this can arrive with a sheet up, over which a SwiftUI alert wouldn't show.
    private func confirmClosingEditors() async -> Bool {
        let alert = NSAlert()
        alert.alertStyle = .warning
        alert.messageText = "Close Vixen?"
        alert.informativeText =
            "You have open editor(s) with unsaved changes, are you sure you want to close Vixen?\n\n If you choose yes, you will be prompted to save those editors as they are closed."
        alert.addButton(withTitle: "Close Vixen")
        alert.addButton(withTitle: "Keep Open")
        guard var host = window else {
            return alert.runModal() == .alertFirstButtonReturn
        }
        while let sheet = host.attachedSheet {
            host = sheet
        }
        return await alert.beginSheetModal(for: host) == .alertFirstButtonReturn
    }

    // MARK: Execution

    fileprivate func refreshExecutionStatus() {
        guard !stopping, let core = app.core else { return }
        executionStatus = core.executionStatus()
    }

    func startExecution() {
        guard let core = app.core else { return }
        Task.detached { core.executionOpen() }
    }

    func stopExecution() {
        guard let core = app.core else { return }
        Task.detached { core.executionClose() }
    }

    // MARK: Sequences

    func refreshRecents() async {
        guard let core = app.core else { return }
        recent = await Task.detached { core.recentSequences() }.value
    }

    /// F19: one creatable type opens straight away, and more would pop up a menu.
    func newSequence() {
        if UITestHarness.isActive {
            UITestHarness.note("new sequence")
        }
        openWindow?(id: WindowID.sequencer, value: SequenceDocumentRef.new())
    }

    /// F20.
    func chooseSequencesToOpen() {
        guard let core = app.core else { return }
        // The first time it starts in the profile's Sequence folder, and after that where the last open left off.
        let start = lastOpenFolder ?? URL(fileURLWithPath: core.dataRoot()).appending(path: "Sequence")
        let panel = OpenSequencePanel()
        Task {
            let urls = await panel.run(directory: start, on: window)
            if let first = urls.first {
                lastOpenFolder = first.deletingLastPathComponent()
            }
            openFiles(urls)
        }
    }

    /// Drops and Finder's Open With take the same path. Before startup has finished, they wait for it.
    func openFiles(_ urls: [URL]) {
        guard startupFinished else {
            pendingFiles.append(contentsOf: urls)
            return
        }
        for url in urls {
            open(path: url.path)
        }
    }

    private func open(path: String) {
        let known = sequenceTypes().map(\.fileExtension)
        let name = URL(fileURLWithPath: path).lastPathComponent
        guard known.contains(where: { name.lowercased().hasSuffix($0) }) else {
            logError(logger: "VixenApplication.VixenApplication", message: "Can't find an appropriate editor to open file \(path)", error: nil)
            Task { await alerts.tell("Error opening file", "Can't find an editor to open this file type. (\"\(name)\")", critical: true) }
            return
        }
        if UITestHarness.isActive {
            UITestHarness.note("open sequence \(path)")
        }
        openWindow?(id: WindowID.sequencer, value: SequenceDocumentRef.existing(path: path))
    }

    /// F22. The check runs on the resolved path, not the stored string.
    func openRecent(path: String) {
        guard FileManager.default.fileExists(atPath: path) else {
            Task { await alerts.tell("Error", "Can't find selected sequence.", critical: true) }
            return
        }
        open(path: path)
    }

    /// F23: no confirmation, and it reaches the disk at the next save.
    func clearRecents() {
        app.core?.clearRecentSequences()
        recent = []
    }

    /// F32: a sequence joins the list when it closes, never when it opens. Each waits for the one before, so the last closed stays first.
    private func sequenceClosed(path: String) {
        guard let core = app.core else { return }
        let previous = recentSave
        recentSave = Task {
            await previous?.value
            do {
                try await Task.detached { try core.addRecentSequence(path: path) }.value
            } catch {
                logError(logger: "VixenApplication.VixenApplicationData", message: "Couldn't save the recent sequences.", error: error.userMessage)
            }
            await refreshRecents()
        }
    }

    // MARK: System configuration

    private func beginWork(_ percent: Int32, _ text: String) {
        working = true
        progressVisible = true
        progressValue = Double(percent) / 100
        progressText = text
        updateBusy()
    }

    private func step(_ percent: Int32, _ text: String) {
        progressValue = Double(percent) / 100
        progressText = text
    }

    private func endWork() {
        working = false
        progressVisible = false
        updateBusy()
    }

    /// F117: a failed save is an alert, with the changes still in memory. The original goes down with no message.
    private func reportSaveFailure(_ error: Error) async {
        await alerts.tell("Error", "Could not save the configuration.\n\n\(error.userMessage)", critical: true)
    }

    private func saveSystemConfig(core: VixenCore) async {
        do {
            _ = try await Task.detached { try core.saveSystemConfig() }.value
        } catch {
            await reportSaveFailure(error)
        }
    }

    private func saveModuleConfig(core: VixenCore) async {
        do {
            _ = try await Task.detached { try core.saveModuleConfig() }.value
        } catch {
            await reportSaveFailure(error)
        }
    }

    /// F102: what OK does in the original, for when the Settings window closes.
    func saveOptionsIfChanged() {
        Task { await savePendingOptions() }
    }

    /// Changes still waiting for their Settings window to close. Upstream's Options is modal, so its OK has saved them before anything else can
    /// happen: quitting, or a dialog whose Cancel reloads the config from disk, which would otherwise keep that dialog's edits or lose these.
    private func savePendingOptions() async {
        guard let core = app.core else { return }
        do {
            _ = try await Task.detached { try core.saveOptionsIfChanged() }.value
        } catch {
            await reportSaveFailure(error)
        }
    }

    private func reloadConfig(core: VixenCore) async {
        do {
            try await Task.detached { try core.reloadSystemConfig() }.value
        } catch {
            await alerts.tell("Error", error.userMessage, critical: true)
        }
    }

    /// F25, after the Display Setup window has closed.
    private func finishDisplaySetup(_ outcome: SystemConfigOutcome) async {
        guard let core = app.core else { return }
        switch outcome {
        case .saved(let apply):
            beginWork(0, "Preparing Display Setup Changes")
            if let apply {
                await Task.detached(operation: apply).value
            }
            step(50, "Saving System Configuration")
            await saveSystemConfig(core: core)
            step(100, "Saving Module Configuration")
            await saveModuleConfig(core: core)
        case .performed(let work):
            beginWork(0, "Preparing Display Setup Changes")
            let report: @Sendable (Int32, String) -> Void = { percent, text in
                Task { @MainActor [weak self] in self?.step(percent, text) }
            }
            await Task.detached { work(report) }.value
        case .discarded(changed: true):
            beginWork(0, "Reloading System Configuration")
            await reloadConfig(core: core)
        case .discarded(changed: false):
            app.finishSystemConfigSession()
            return
        }
        endWork()
        app.finishSystemConfigSession()
    }

    /// F26: under the same session as Display Setup, since Cancel can replace the system data under an open sequencer. The session starts at once,
    /// so a Settings window left open stops taking changes, and what it already has saves before the sheet can change anything.
    func showSetupPreviews() {
        app.beginSystemConfigSession()
        Task {
            await savePendingOptions()
            sheet = .setupPreviews
        }
    }

    /// F25: the Display Setup window is the setup area's, and starts its own session when it appears.
    func showDisplaySetup() {
        Task {
            await savePendingOptions()
            if let reveal = StudioDisplayPane.reveal { reveal() } else { openWindow?(id: WindowID.displaySetup) } // STUDIO: Display Setup is the show window's Display section
        }
    }

    /// F26, after Setup Previews has closed: OK saves, and Cancel reloads if anything changed, ticks included, since that's what puts the running
    /// states back. With nothing changed Cancel does nothing, because a reload stops and restarts every controller and preview.
    func finishSetupPreviews(saved: Bool) {
        guard let core = app.core else { return }
        let needsReload = core.setupPreviewNeedsReload()
        core.setupResetPreviewChanges()
        sheet = nil
        Task {
            if saved {
                beginWork(0, "Saving System Configuration")
                await saveSystemConfig(core: core)
                step(50, "Saving Module Configuration")
                await saveModuleConfig(core: core)
                endWork()
            } else if needsReload {
                beginWork(0, "Reloading Configuration")
                await reloadConfig(core: core)
                endWork()
            }
            app.finishSystemConfigSession()
        }
    }

    /// F41: the running session doesn't change.
    func profilesEdited(saved: Bool) {
        sheet = nil
        guard saved else { return }
        Task {
            try? await Task.sleep(for: .milliseconds(350))
            await alerts.tell("Profiles Changed", "You must re-start Vixen for the changes to take effect.")
        }
    }

    // MARK: Updates and links

    private func makeUpdateChecker() -> UpdateChecker {
        // A UI test never reaches the network. A test can name its own local server, to exercise the rest of the path.
        let environment = ProcessInfo.processInfo.environment
        if UITestHarness.isActive {
            if let base = environment["VIXEN_UPDATE_TEST_BASE"] {
                return UpdateChecker.github(client: URLSessionUpdateClient(base: base), bundleBuild: bundleBuild)
            }
            return UpdateChecker.unavailable(bundleBuild: bundleBuild)
        }
        return UpdateChecker.github(client: URLSessionUpdateClient(base: updateHttpSettings().base), bundleBuild: bundleBuild)
    }

    /// Only ever from Check for Updates, which the user opened. Nothing checks by itself.
    func checkForUpdates() async -> UpdateDialogRecord {
        let checker = updateChecker
        let result = await Task.detached { checker.checkForUpdates() }.value
        if let notice = result.notice {
            updateNotice = notice
        }
        return result
    }

    func releaseNotes() async -> String {
        let checker = updateChecker
        let folder = Bundle.main.resourcePath ?? ""
        return await Task.detached { checker.releaseNotesText(bundleDir: folder) }.value
    }

    /// Links and log files open in another app. Under test they're only written down.
    func openExternally(_ url: URL) {
        guard UITestHarness.isActive else {
            NSWorkspace.shared.open(url)
            return
        }
        UITestHarness.note("opened \(url.absoluteString)")
    }

    func openLog(named name: String) {
        openExternally(URL(fileURLWithPath: logsFolder()).appending(path: name))
    }
}

/// `IProgress`, hopping to the main actor.
private nonisolated final class ProgressForwarder: StartupProgressListener, @unchecked Sendable {
    private let model: AdminModel

    init(model: AdminModel) {
        self.model = model
    }

    func report(percent: Int32, text: String) {
        let model = model
        Task { @MainActor in model.reportProgress(percent, text) }
    }
}

/// `executionStateChangedHandler`: an async hop where the original blocks a pool thread. It reads the state again rather than trusting the event.
private nonisolated final class ExecutionForwarder: ExecutionStateListener, @unchecked Sendable {
    private let model: AdminModel

    init(model: AdminModel) {
        self.model = model
    }

    func stateChanged(status: ExecutionStatusRecord) {
        let model = model
        Task { @MainActor in model.refreshExecutionStatus() }
    }
}
