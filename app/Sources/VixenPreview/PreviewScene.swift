import AppKit
import SwiftUI
import VixenBridge
import VixenKit

/// The Preview Configuration windows. Live windows are AppKit and follow the running previews, through `LivePreviewCoordinator`, which needs no scene.
public struct PreviewScene: Scene {
    private let model: AppModel

    public init(model: AppModel) {
        self.model = model
        LivePreviewCoordinator.start(model: model)
        PreviewHarness.install(model: model)
    }

    public var body: some Scene {
        WindowGroup("Preview Configuration", id: WindowID.previewSetup, for: String.self) { $previewID in
            DesignerWindowRoot(previewID: previewID)
                .environment(model)
                .studioAppearance() // STUDIO: follow the chosen appearance
        }
        // F1's client size, less the 24 px menu strip that moves to the menu bar.
        .defaultSize(width: 1194, height: 601)
        .windowResizability(.contentMinSize)
        .defaultLaunchBehavior(.suppressed)
        // Windows open from the preview's own bounds, not from the system's restoration.
        .restorationBehavior(.disabled)
        .commands {
            PreviewDesignerCommands(registry: DesignerRegistry.shared)
        }
    }
}
