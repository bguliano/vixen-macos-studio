import SwiftUI
import VixenBridge
import VixenKit

/// The app's main window, the menus it owns, and the small windows that open from them.
public struct AdministrationScene: Scene {
    private let model: AppModel
    @State private var admin: AdminModel

    public init(model: AppModel) {
        self.model = model
        model.startsElsewhere = true
        let admin = AdminModel(app: model)
        // Finder can ask to open a file before the window has appeared. The first wins, because `init` can run again and only the first model stays.
        if AppHooks.openFiles == nil {
            AppHooks.openFiles = { [weak admin] urls in admin?.openFiles(urls) }
        }
        _admin = State(initialValue: admin)
    }

    public var body: some Scene {
        // STUDIO-BEGIN show-window: Studio's show window replaces the administration window
        Window("Vixen Studio", id: WindowID.administration) {
            StudioShowWindow()
                .environment(admin)
                .environment(model)
                .studioAppearance() // STUDIO: follow the chosen appearance
        }
        .defaultSize(width: 1080, height: 680)
        .windowResizability(.contentMinSize)
        // STUDIO-END show-window
        // Always, even when the app starts by being asked to open a file, since the rest of the launch happens in this window.
        .defaultLaunchBehavior(.presented)
        .restorationBehavior(.disabled)
        .commands {
            AdministrationCommands(admin: admin, state: MenuState(admin: admin))
        }

        Settings {
            StudioSettingsView() // STUDIO: Studio's tabbed Settings, with Vixen's options as the Engine tab
                .environment(admin)
                .studioAppearance() // STUDIO: follow the chosen appearance
        }

        Window("About Vixen", id: WindowID.about) {
            AboutView()
                .environment(admin)
                .studioAppearance() // STUDIO: follow the chosen appearance
        }
        .defaultLaunchBehavior(.suppressed)
        .restorationBehavior(.disabled)
        .windowResizability(.contentSize)

        Window("Installed Modules", id: WindowID.installedModules) {
            InstalledModulesView()
                .studioAppearance() // STUDIO: follow the chosen appearance
        }
        .defaultLaunchBehavior(.suppressed)
        .restorationBehavior(.disabled)
        .windowResizability(.contentSize)

        Window("Vixen Release Notes", id: WindowID.releaseNotes) {
            ReleaseNotesView()
                .environment(admin)
                .studioAppearance() // STUDIO: follow the chosen appearance
        }
        .defaultSize(width: 774, height: 571)
        .defaultLaunchBehavior(.suppressed)
        .restorationBehavior(.disabled)

        Window("Instrumentation", id: WindowID.instrumentation) {
            InstrumentationView()
                .environment(admin)
                .studioAppearance() // STUDIO: follow the chosen appearance
        }
        .defaultSize(width: 807, height: 437)
        .defaultLaunchBehavior(.suppressed)
        .restorationBehavior(.disabled)
    }
}
