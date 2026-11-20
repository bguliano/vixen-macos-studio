import AppKit
import SwiftUI
import VixenBridge
import VixenKit

/// The menu bar: Vixen, File, Edit, System, Tools, Scheduler, Window, Help, with the original's items in its order, and the Mac's own around them.
struct AdministrationCommands: Commands {
    let admin: AdminModel
    /// Everything the menus depend on, read in the scene's body. SwiftUI only rebuilds menu items when its `Commands` value changes.
    let state: MenuState

    var body: some Commands {
        CommandGroup(replacing: .appInfo) {
            AboutMenuItem()
        }
        CommandGroup(replacing: .newItem) {
            FileMenuItems(admin: admin, state: state)
        }
        CommandMenu("System") {
            SystemMenuItems(admin: admin, state: state)
        }
        CommandMenu("Tools") {
            ToolsMenuItems(admin: admin, state: state)
        }
        CommandMenu("Scheduler") {
            SchedulerMenuItems(admin: admin, state: state)
        }
        CommandGroup(replacing: .help) {
            HelpMenuItems(admin: admin, state: state)
        }
    }
}

struct MenuState: Equatable {
    var isBusy: Bool
    var helpAvailable: Bool
    var startEnabled: Bool
    var stopEnabled: Bool
    var recent: [String]
    var logFiles: [String]
    var hasSchedulerToggle: Bool

    init(admin: AdminModel) {
        isBusy = admin.isBusy
        helpAvailable = admin.isHelpAvailable
        startEnabled = admin.executionStatus.startEnabled
        stopEnabled = admin.executionStatus.stopEnabled
        recent = admin.recent.map(\.path)
        logFiles = admin.logFiles
        hasSchedulerToggle = admin.app.schedulerEnabledToggle != nil
    }
}

private struct AboutMenuItem: View {
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        Button("About Vixen Studio") { openWindow(id: WindowID.about) } // STUDIO: Studio's name
    }
}

private struct OptionsMenuItem: View {
    let title: String
    @Environment(\.openSettings) private var openSettings

    var body: some View {
        Button(title) { openSettings() }
    }
}

private struct FileMenuItems: View {
    let admin: AdminModel
    let state: MenuState

    var body: some View {
        Button("New Sequence...") { admin.newSequence() }
            .keyboardShortcut("n", modifiers: .command)
            .disabled(state.isBusy)
        Button("Open Sequence...") { admin.chooseSequencesToOpen() }
            .keyboardShortcut("o", modifiers: .command)
            .disabled(state.isBusy)
        Menu("Open Recent") {
            ForEach(state.recent, id: \.self) { path in
                Button(URL(fileURLWithPath: path).lastPathComponent) { admin.openRecent(path: path) }
            }
            Divider()
            Button("Clear Menu") { admin.clearRecents() }
                .disabled(state.recent.isEmpty)
        }
        .disabled(state.isBusy)
    }
}

private struct SystemMenuItems: View {
    let admin: AdminModel
    let state: MenuState
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        Group {
            Menu("Logs") {
                ForEach(state.logFiles, id: \.self) { name in
                    Button(name) { admin.openLog(named: name) }
                }
            }
            .disabled(state.logFiles.isEmpty)
            Button("View Installed Modules") { openWindow(id: WindowID.installedModules) }
            Button("Profiles...") { admin.sheet = .dataProfile }
            Menu("System Configuration") {
                Button("Setup Display") { admin.showDisplaySetup() }
                Button("Setup Previews") { admin.showSetupPreviews() }
            }
            Menu("Execution Engine") {
                Button("Start") { admin.startExecution() }
                    .disabled(!state.startEnabled)
                Button("Stop") { admin.stopExecution() }
                    .disabled(!state.stopEnabled)
            }
            Divider()
            Button("Shutdown and Exit") { NSApp.terminate(nil) }
        }
        .disabled(state.isBusy)
    }
}

private struct ToolsMenuItems: View {
    let admin: AdminModel
    let state: MenuState
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        Group {
            OptionsMenuItem(title: "Options...")
            Button("Custom Prop Editor") { openWindow(id: WindowID.customPropEditor, value: UUID()) }
            Button("Export Wizard") { openWindow(id: WindowID.exportWizard) }
            Button("Instrumentation...") { openWindow(id: WindowID.instrumentation) }
            Button("Import Sequence Package") { openWindow(id: WindowID.sequencePackageImport) }
            Button("Export Sequence Package") { openWindow(id: WindowID.sequencePackageExport) }
            Button("Web Server") { admin.sheet = .webServer }
        }
        .disabled(state.isBusy)
    }
}

private struct SchedulerMenuItems: View {
    let admin: AdminModel
    let state: MenuState
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        Group {
            // STUDIO-BEGIN schedule-pane: Shows and Schedules are the show window's Schedule section, editing live with Undo
            Button("Shows") { revealSchedule(else: WindowID.shows) }
            Button("Schedules") { revealSchedule(else: WindowID.schedules) }
            // STUDIO-END schedule-pane
            Divider()
            // The toggle and the status window it opens belong to the scheduler area, which fills in `AppModel.schedulerEnabledToggle`.
            if let toggle = admin.app.schedulerEnabledToggle {
                Toggle("Enabled", isOn: Binding(get: { toggle.isOn() }, set: { toggle.setOn($0) }))
            } else {
                Toggle("Enabled", isOn: .constant(false))
                    .disabled(true)
            }
        }
        .disabled(state.isBusy)
    }

    // STUDIO-BEGIN schedule-pane-reveal: the section replaces the Shows and Scheduler windows
    private func revealSchedule(else window: String) {
        if let reveal = StudioScheduleCore.reveal {
            admin.window?.makeKeyAndOrderFront(nil)
            reveal()
        } else {
            openWindow(id: window)
        }
    }
    // STUDIO-END schedule-pane-reveal
}

private struct HelpMenuItems: View {
    let admin: AdminModel
    let state: MenuState
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        Group {
            Button("Online Help") { open("https://www.vixenlights.com/docs/") }
            Button("Vixen YouTube Channel") { open("https://www.youtube.com/user/VixenLightsSoftware") }
            Button("Check for Updates") { admin.sheet = .checkForUpdates }
                .disabled(admin.version.isTestBuild || state.isBusy)
            Button("Release Notes") { openWindow(id: WindowID.releaseNotes) }
            // STUDIO: About lives only in the app menu, its one place on the Mac
        }
        .disabled(!state.helpAvailable)
    }

    private func open(_ address: String) {
        if let url = URL(string: address) {
            admin.openExternally(url)
        }
    }
}
