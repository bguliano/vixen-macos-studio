import SwiftUI
import VixenBridge // STUDIO: the plugin host's shutdown
import VixenKit

#if canImport(VixenAdmin)
import VixenAdmin
#endif
#if canImport(VixenSetup)
import VixenSetup
#elseif canImport(VixenDialogs)
import VixenDialogs
#endif
#if canImport(VixenPreview)
import VixenPreview
#endif
#if canImport(VixenSequencer)
import VixenSequencer
#elseif canImport(VixenTimeline)
import VixenTimeline
#endif
#if canImport(VixenEffectEditor) && !canImport(VixenSequencer)
import VixenEffectEditor
#endif
#if canImport(VixenMarks) && !canImport(VixenSequencer)
import VixenMarks
#endif
#if canImport(VixenScheduler)
import VixenScheduler
#endif
#if canImport(VixenTools)
import VixenTools
#endif

final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationWillFinishLaunching(_ notification: Notification) {
        NSApp.appearance = NSAppearance(named: .darkAqua)
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        UITestHarness.startIfRequested()
        WindowMenus.install()
    }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        AppHooks.shouldTerminate?() ?? .terminateNow
    }

    func application(_ application: NSApplication, open urls: [URL]) {
        AppHooks.openFiles?(urls)
    }

    // STUDIO-BEGIN plugins-shutdown: stop every plugin process when Studio quits
    func applicationWillTerminate(_ notification: Notification) {
        studioPluginsShutdown()
    }
    // STUDIO-END plugins-shutdown
}

/// Puts the app together from whichever area modules this build has. Each area's module exports a scene named after it.
@main
struct VixenMacApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate
    @State private var model = AppModel()

    init() {
        // The original never reopens windows. With saved state that lists none, macOS would also skip the main window at launch. And the command
        // line's words, such as `no_controllers`, aren't files to open.
        UserDefaults.standard.register(defaults: [
            "ApplePersistenceIgnoreState": true,
            "NSQuitAlwaysKeepsWindows": false,
            "NSTreatUnknownArgumentsAsOpen": "NO",
        ])
    }

    var body: some Scene {
        #if canImport(VixenAdmin)
        // STUDIO-BEGIN command-palette: View ▸ Command Palette… (⌘K) over every menu command, in every window
        AdministrationScene(model: model)
            .commands { StudioCommandPaletteCommands() }
        // STUDIO-END command-palette
        #endif
        #if canImport(VixenSetup)
        SetupScene(model: model)
        #elseif canImport(VixenDialogs)
        DialogsTestScene(model: model)
        #endif
        #if canImport(VixenPreview)
        PreviewScene(model: model)
        #endif
        #if canImport(VixenSequencer)
        SequencerScene(model: model)
        #else
        // A worker building one part of the sequencer alone gets that part's own test window.
        #if canImport(VixenTimeline)
        TimelineTestScene(model: model)
        #endif
        #if canImport(VixenEffectEditor)
        EffectEditorTestScene(model: model)
        #endif
        #if canImport(VixenMarks)
        MarksTestScene(model: model)
        #endif
        #endif
        #if canImport(VixenScheduler)
        SchedulerScene(model: model)
        #endif
        #if canImport(VixenTools)
        ToolsScene(model: model)
        #endif
    }
}
