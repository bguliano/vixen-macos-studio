import SwiftUI
import VixenBridge
import VixenKit
#if canImport(VixenDialogs)
import VixenDialogs
#endif

/// Display Setup: one window, opened from the main window's Setup Display button and the System menu, plus the Intelligent Fixture wizard's window.
public struct SetupScene: Scene {
    private let model: AppModel
    @State private var setup: SetupModel

    public init(model: AppModel) {
        self.model = model
        _setup = State(initialValue: SetupModel(app: model))
        TemplateRunner.register()
        TreeEditRunner.register()
        ElementTagManagerPresenter.register(app: model)
    }

    public var body: some Scene {
        Window("Display Setup", id: WindowID.displaySetup) {
            SetupRootView(model: setup)
                .environment(model)
                .studioAppearance() // STUDIO: follow the chosen appearance
                .background(SetupWindowOpeners(model: setup))
        }
        .defaultSize(width: 1281, height: 737)
        .defaultLaunchBehavior(.suppressed)
        .restorationBehavior(.disabled)

        Window("Intelligent Fixture Wizard", id: SetupWindows.fixtureWizard) {
            FixtureWizardWindowContent(setup: setup)
                .studioAppearance() // STUDIO: follow the chosen appearance
        }
        .windowResizability(.contentSize)
        .defaultLaunchBehavior(.suppressed)
        .restorationBehavior(.disabled)

        #if DEBUG
        Window("Display Setup Sheets", id: "setup-sheets-test") {
            SetupSheetsTestView()
                .studioAppearance() // STUDIO: follow the chosen appearance
        }
        .defaultLaunchBehavior(.suppressed)
        .restorationBehavior(.disabled)
        #endif

        #if canImport(VixenDialogs)
        ControllerMonitorScene(model: model)
        #endif
    }
}

enum SetupWindows {
    static let fixtureWizard = "setup-fixture-wizard"
}

/// Gives the model what it needs to open and close the wizard's window, and registers the test hooks when a script is running.
private struct SetupWindowOpeners: View {
    let model: SetupModel
    @Environment(\.openWindow) private var openWindow
    @Environment(\.dismissWindow) private var dismissWindow

    var body: some View {
        Color.clear
            .frame(width: 0, height: 0)
            .onAppear {
                model.openWizardWindow = { openWindow(id: SetupWindows.fixtureWizard) }
                model.openSetupWindow = { openWindow(id: WindowID.displaySetup) }
                model.dismissWizardWindow = { dismissWindow(id: SetupWindows.fixtureWizard) }
                if UITestHarness.isActive {
                    SetupTestHooks.install(model: model)
                }
            }
    }
}

/// The wizard while it's running. The close box counts as Cancel.
private struct FixtureWizardWindowContent: View {
    let setup: SetupModel

    var body: some View {
        Group {
            if let wizard = setup.fixtureWizard {
                FixtureWizardView(model: wizard)
                    .onDisappear { wizard.windowClosed() }
            } else {
                Color.clear
                    .frame(width: 1, height: 1)
            }
        }
    }
}
