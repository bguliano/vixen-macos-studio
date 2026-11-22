import AppKit
import Foundation
import Sparkle
import SwiftUI
import VixenBridge
import VixenKit

/// Automatic updates with Sparkle 2 (B49): Vixen Studio ▸ Check for Updates…, Settings ▸ General ▸ Updates, and the
/// state the MCP tools `app_update_status` and `app_update_check` read.
///
/// Builds without an update feed and signing key in their Info.plist (debug builds, and releases made before the key
/// existed) have no updater at all: the menu item and the Settings section stay hidden.
///
/// It never interrupts a show: while a sequence or scheduled show plays or Live Output is on, a scheduled update waits
/// as a gentle reminder and an accepted install waits to relaunch, and both carry on once the show stops.
@MainActor
@Observable
public final class StudioUpdater: NSObject {
    public static let shared = StudioUpdater()

    /// Whether this build updates itself: it carries a feed (`SUFeedURL`) and the key that signs updates (`SUPublicEDKey`).
    public let isEnabled: Bool
    public private(set) var canCheckForUpdates = false
    public private(set) var lastCheck: Date?
    public private(set) var checking = false
    public private(set) var available: SUAppcastItem?
    public private(set) var lastError: String?
    /// A scheduled update found while a show ran, shown once it stops.
    public private(set) var deferred = false
    public private(set) var readyToInstallOnQuit = false

    @ObservationIgnored private var controller: SPUStandardUpdaterController?
    @ObservationIgnored private weak var app: AppModel?
    @ObservationIgnored private var canCheckObservation: NSKeyValueObservation?
    @ObservationIgnored private var pendingRelaunch: SparkleHandler?
    @ObservationIgnored private var showWatch: Timer?

    private override init() {
        let info = Bundle.main.infoDictionary ?? [:]
        let feed = (info["SUFeedURL"] as? String) ?? ""
        let key = (info["SUPublicEDKey"] as? String) ?? ""
        isEnabled = !feed.isEmpty && !key.isEmpty
        super.init()
    }

    /// Starts the updater once, at launch. Later calls do nothing.
    public func start(app: AppModel) {
        self.app = app
        registerTestHook()
        guard isEnabled, controller == nil else {
            publish()
            return
        }
        let controller = SPUStandardUpdaterController(startingUpdater: false, updaterDelegate: self, userDriverDelegate: self)
        self.controller = controller
        // SPUUpdater changes it on the main thread, so KVO reports it there.
        canCheckObservation = controller.updater.observe(\.canCheckForUpdates, options: [.initial, .new]) { [weak self] updater, _ in
            MainActor.assumeIsolated {
                self?.canCheckForUpdates = updater.canCheckForUpdates
            }
        }
        studioUpdatesRegister(checker: StudioUpdateCheckHook())
        publish()
        // Sparkle checks as soon as it starts when a check is due. Start it once the show has loaded, so it can tell
        // whether a sequence plays or Live Output is on (the profile opens Live Output as it loads).
        Task { [weak self] in
            while self?.app?.core == nil {
                try? await Task.sleep(for: .milliseconds(500))
                if self == nil { return }
            }
            guard let self, let controller = self.controller else { return }
            controller.startUpdater()
            lastCheck = controller.updater.lastUpdateCheckDate
            publish()
        }
    }

    // MARK: Settings

    public var automaticallyChecks: Bool {
        get { access(keyPath: \.automaticallyChecks); return controller?.updater.automaticallyChecksForUpdates ?? false }
        set {
            withMutation(keyPath: \.automaticallyChecks) { controller?.updater.automaticallyChecksForUpdates = newValue }
            publish()
        }
    }

    /// Sparkle's `SUAutomaticallyUpdate`: download updates in the background and install them when Vixen Studio quits.
    public var automaticallyDownloads: Bool {
        get { access(keyPath: \.automaticallyDownloads); return controller?.updater.automaticallyDownloadsUpdates ?? false }
        set {
            withMutation(keyPath: \.automaticallyDownloads) { controller?.updater.automaticallyDownloadsUpdates = newValue }
            publish()
        }
    }

    public var currentVersion: String { Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "" }
    public var currentBuild: String { Bundle.main.object(forInfoDictionaryKey: "CFBundleVersion") as? String ?? "" }

    // MARK: Checking

    /// Vixen Studio ▸ Check for Updates…: the user asked, so Sparkle's window shows the answer.
    public func checkForUpdates() {
        controller?.checkForUpdates(nil)
    }

    /// `app_update_check`: fetch the feed and note what it offers, with no window, download or install.
    fileprivate func checkForUpdateInformation() {
        guard let updater = controller?.updater else { return }
        if updater.sessionInProgress {
            lastError = "An update is already showing or installing; it finishes in Vixen Studio's own window."
            publish(finishedCheck: true)
            return
        }
        checking = true
        lastError = nil
        publish()
        updater.checkForUpdateInformation()
    }

    // MARK: Shows

    /// Why a show is running ("A sequence is playing", "Live Output is on"), or nil when none is.
    private var showRunningReason: String? {
        guard let core = app?.core else { return "Vixen Studio is still loading the show" }
        guard let data = core.studioShowActivity().data(using: .utf8),
              let activity = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return nil }
        if activity["playing"] as? Bool == true { return "A sequence is playing" }
        if activity["liveOutput"] as? Bool == true { return "Live Output is on" }
        return nil
    }

    private var showRunning: Bool { showRunningReason != nil }

    /// Someone asked to install and relaunch while a show runs: it waits for the show, unless they choose to restart now.
    private func offerRestartNow(_ reason: String) {
        DispatchQueue.main.async { [weak self] in
            guard let self, pendingRelaunch != nil else { return }
            let alert = NSAlert()
            alert.messageText = "Vixen Studio will restart to finish updating when the show stops."
            alert.informativeText = "\(reason). Restarting now stops it. If you quit Vixen Studio first, the update installs then."
            alert.addButton(withTitle: "Restart When the Show Stops")
            alert.addButton(withTitle: "Restart Now")
            // A sheet, so nothing else in the app stops while it's up; app-modal only when no window is open.
            let finish: (NSApplication.ModalResponse) -> Void = { [weak self] response in
                guard let self, response == .alertSecondButtonReturn, let relaunch = pendingRelaunch else { return }
                pendingRelaunch = nil
                showWatch?.invalidate()
                showWatch = nil
                relaunch.run()
            }
            let showWindow = NSApp.windows.first { $0.title == "Vixen Studio" && $0.canBecomeMain }
            if let window = showWindow ?? NSApp.mainWindow {
                alert.beginSheetModal(for: window, completionHandler: finish)
            } else {
                finish(alert.runModal())
            }
        }
    }

    /// While something waits for the show to stop, looks every 15 seconds; then shows the update or relaunches.
    private func watchShow() {
        guard showWatch == nil else { return }
        showWatch = Timer.scheduledTimer(withTimeInterval: 15, repeats: true) { [weak self] _ in
            Task { @MainActor in self?.showMayHaveStopped() }
        }
    }

    private func showMayHaveStopped() {
        guard !showRunning else { return }
        showWatch?.invalidate()
        showWatch = nil
        if let relaunch = pendingRelaunch {
            pendingRelaunch = nil
            relaunch.run()
        } else if deferred {
            // Brings the waiting update back into focus.
            deferred = false
            controller?.updater.checkForUpdates()
            publish()
        }
    }

    // MARK: Reporting

    /// The state `app_update_status` returns.
    private var status: [String: Any] {
        guard isEnabled else {
            return ["enabled": false, "currentVersion": currentVersion, "currentBuild": currentBuild,
                    "reason": "This build has no update feed, so it doesn't update itself."]
        }
        let updater = controller?.updater
        var update: Any = NSNull()
        if let item = available {
            update = [
                "version": item.displayVersionString,
                "build": item.versionString,
                "title": item.title ?? "",
                "notesUrl": item.fullReleaseNotesURL?.absoluteString ?? item.releaseNotesURL?.absoluteString ?? "",
            ]
        }
        return [
            "enabled": true,
            "currentVersion": currentVersion,
            "currentBuild": currentBuild,
            "automaticallyChecks": updater?.automaticallyChecksForUpdates ?? false,
            "checkIntervalHours": (updater?.updateCheckInterval ?? 86400) / 3600,
            "automaticallyDownloads": updater?.automaticallyDownloadsUpdates ?? false,
            "lastCheck": lastCheck.map { ISO8601DateFormatter().string(from: $0) } ?? NSNull(),
            "checking": checking,
            "available": update,
            "deferred": deferred,
            "readyToInstallOnQuit": readyToInstallOnQuit,
            "showRunning": showRunning,
            "waitingForShowToStop": pendingRelaunch != nil,
            "lastError": lastError ?? NSNull(),
        ]
    }

    private func publish(finishedCheck: Bool = false) {
        guard let data = try? JSONSerialization.data(withJSONObject: status, options: [.sortedKeys]),
              let text = String(data: data, encoding: .utf8) else { return }
        try? studioUpdatesPublish(statusJson: text, finishedCheck: finishedCheck)
    }

    // MARK: Test harness

    /// `{ "action": "hook", "name": "studio.updater", "path": [...] }`: status, check (as the menu item), information
    /// (as `app_update_check`), waitReady <seconds> (until an automatic download is ready to install, or an error).
    private func registerTestHook() {
        guard UITestHarness.isActive else { return }
        UITestHarness.hooks["studio.updater"] = { [weak self] args in
            guard let self else { return "FAIL no updater" }
            switch args.first {
            case "check": checkForUpdates()
            case "information": checkForUpdateInformation()
            case "waitReady":
                let seconds = Double(args.count > 1 ? args[1] : "") ?? 60
                let deadline = Date().addingTimeInterval(seconds)
                while !readyToInstallOnQuit, lastError == nil, Date() < deadline {
                    try? await Task.sleep(for: .milliseconds(250))
                }
            case "status", nil: break
            default: return "FAIL status|check|information|waitReady"
            }
            try? await Task.sleep(for: .milliseconds(300))
            let data = (try? JSONSerialization.data(withJSONObject: status, options: [.sortedKeys])) ?? Data()
            let text = String(data: data, encoding: .utf8) ?? ""
            // Also in notes.txt: an update's relaunch quits the app before the harness writes result.json.
            UITestHarness.note("updater status: \(text)")
            return text
        }
    }
}

extension StudioUpdater: SPUUpdaterDelegate {
    public nonisolated func updater(_ updater: SPUUpdater, didFindValidUpdate item: SUAppcastItem) {
        MainActor.assumeIsolated {
            available = item
            publish()
        }
    }

    public nonisolated func updaterDidNotFindUpdate(_ updater: SPUUpdater, error: any Error) {
        MainActor.assumeIsolated {
            available = nil
            publish()
        }
    }

    public nonisolated func updater(_ updater: SPUUpdater, didFinishUpdateCycleFor updateCheck: SPUUpdateCheck, error: (any Error)?) {
        MainActor.assumeIsolated {
            checking = false
            lastCheck = updater.lastUpdateCheckDate
            // "No update" ends a cycle with an error too; it isn't one.
            if let error = error as NSError?, error.code != Int(SUError.noUpdateError.rawValue) {
                lastError = error.localizedDescription
            } else if error != nil {
                lastError = nil
            }
            publish(finishedCheck: true)
        }
    }

    public nonisolated func updater(_ updater: SPUUpdater, shouldPostponeRelaunchForUpdate item: SUAppcastItem, untilInvokingBlock installHandler: @escaping () -> Void) -> Bool {
        let handler = SparkleHandler(run: installHandler)
        return MainActor.assumeIsolated {
            guard let reason = showRunningReason else { return false }
            pendingRelaunch = handler
            publish()
            watchShow()
            offerRestartNow(reason)
            return true
        }
    }

    public nonisolated func updater(_ updater: SPUUpdater, willInstallUpdateOnQuit item: SUAppcastItem, immediateInstallationBlock immediateInstallHandler: @escaping () -> Void) -> Bool {
        MainActor.assumeIsolated {
            readyToInstallOnQuit = true
            publish()
            // Sparkle installs it when Vixen Studio quits.
            return false
        }
    }

    public nonisolated func updater(_ updater: SPUUpdater, willInstallUpdate item: SUAppcastItem) {
        MainActor.assumeIsolated { UITestHarness.note("updater: installing build \(item.versionString)") }
    }

    public nonisolated func updaterWillRelaunchApplication(_ updater: SPUUpdater) {
        MainActor.assumeIsolated { UITestHarness.note("updater: relaunching") }
    }

    public nonisolated func updater(_ updater: SPUUpdater, didAbortWithError error: any Error) {
        MainActor.assumeIsolated {
            let error = error as NSError
            if error.code != Int(SUError.noUpdateError.rawValue) {
                lastError = error.localizedDescription
                publish()
            }
        }
    }
}

extension StudioUpdater: SPUStandardUserDriverDelegate {
    public nonisolated var supportsGentleScheduledUpdateReminders: Bool { true }

    public nonisolated func standardUserDriverShouldHandleShowingScheduledUpdate(_ update: SUAppcastItem, andInImmediateFocus immediateFocus: Bool) -> Bool {
        MainActor.assumeIsolated { !showRunning }
    }

    public nonisolated func standardUserDriverWillHandleShowingUpdate(_ handleShowingUpdate: Bool, forUpdate update: SUAppcastItem, state: SPUUserUpdateState) {
        MainActor.assumeIsolated {
            guard !handleShowingUpdate else { return }
            deferred = true
            publish()
            watchShow()
        }
    }

    public nonisolated func standardUserDriverDidReceiveUserAttention(forUpdate update: SUAppcastItem) {
        MainActor.assumeIsolated {
            deferred = false
            publish()
        }
    }

    public nonisolated func standardUserDriverWillFinishUpdateSession() {
        MainActor.assumeIsolated {
            deferred = false
            publish()
        }
    }
}

/// A block Sparkle hands over to run later. Sparkle calls its delegates, and these blocks, on the main thread.
private nonisolated struct SparkleHandler: @unchecked Sendable {
    let run: () -> Void
}

/// `app_update_check`'s hook. Rust calls it on an MCP command thread; the check starts on the main actor and reports
/// back through `studioUpdatesPublish`.
private nonisolated final class StudioUpdateCheckHook: StudioUpdateChecker, @unchecked Sendable {
    func check() -> String? {
        Task { @MainActor in StudioUpdater.shared.checkForUpdateInformation() }
        return nil
    }
}

/// Vixen Studio ▸ Check for Updates…, under About. Absent in builds that don't update themselves.
struct StudioCheckForUpdatesMenuItem: View {
    private let updater = StudioUpdater.shared

    var body: some View {
        if updater.isEnabled {
            Button("Check for Updates…") { updater.checkForUpdates() }
                .disabled(!updater.canCheckForUpdates)
        }
    }
}

/// Settings ▸ General ▸ Updates. Live: each switch takes effect as it changes.
struct StudioUpdateSettingsSection: View {
    @Bindable private var updater = StudioUpdater.shared

    var body: some View {
        if updater.isEnabled {
            Section {
                Toggle("Automatically check for updates", isOn: $updater.automaticallyChecks)
                Toggle("Automatically download and install updates", isOn: $updater.automaticallyDownloads)
                    .disabled(!updater.automaticallyChecks)
                LabeledContent("Last checked") {
                    HStack {
                        Text(lastChecked)
                            .foregroundStyle(.secondary)
                        Button("Check Now") { updater.checkForUpdates() }
                            .disabled(!updater.canCheckForUpdates)
                    }
                }
            } header: {
                Text("Updates")
            } footer: {
                Text("Vixen Studio checks once a day. Updates never interrupt a playing sequence or Live Output: they wait until the show stops, and otherwise install when you quit.")
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
        }
    }

    private var lastChecked: String {
        guard let date = updater.lastCheck else { return "Never" }
        return date.formatted(.relative(presentation: .named))
    }
}
