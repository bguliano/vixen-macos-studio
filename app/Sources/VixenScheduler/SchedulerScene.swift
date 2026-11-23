import AppKit
import SwiftUI
import VixenBridge
import VixenKit

/// The scheduler and export area: the Export Sequence dialog, the Export Wizard, Shows and the Scheduler. The Scheduler Status window is
/// `SchedulerHub`'s panel, since it comes and goes with the scheduler and not with a menu item. See
/// `docs/spec/shows-scheduler-export.md` and `docs/parity/shows-scheduler-export.md`.
public struct SchedulerScene: Scene {
    private let model: AppModel

    public init(model: AppModel) {
        self.model = model
        if SongInventory.present == nil {
            SongInventory.present = { window, completion in SongInventoryPresenter.present(over: window, completion: completion) }
        }
        if Vixen2Importer.present == nil {
            Vixen2Importer.present = { core, path, window, completion in
                Vixen2Presenter.present(core: core, path: path, over: window, completion: completion)
            }
        }
        if ElementMapper.present == nil {
            ElementMapper.present = { core, name, sources, window, completion in
                ElementMapperPresenter.present(core: core, name: name, sources: sources, over: window, completion: completion)
            }
        }
        // `init` can run again, and as in the Administration area the first model is the one that stays.
        if SequenceExport.present == nil {
            SequenceExport.present = { session, window in
                ExportSheetPresenter.present(session: session, over: window)
            }
            SchedulerTestHooks.install(model: model)
            SchedulerHub.follow(model)
        }
    }

    public var body: some Scene {
        Window("Sequence Package Export", id: WindowID.sequencePackageExport) {
            PackageWizardRoot(importing: false).environment(model).studioAppearance() // STUDIO: follow the chosen appearance
        }
        .defaultSize(width: 680, height: 500)
        .windowResizability(.contentMinSize)
        .defaultLaunchBehavior(.suppressed)
        .restorationBehavior(.disabled)
        .commandsRemoved() // STUDIO: no Window menu opener; Tools ▸ Export Sequence Package opens it (B83)

        Window("Sequence Package Import", id: WindowID.sequencePackageImport) {
            PackageWizardRoot(importing: true).environment(model).studioAppearance() // STUDIO: follow the chosen appearance
        }
        .defaultSize(width: 680, height: 500)
        .windowResizability(.contentMinSize)
        .defaultLaunchBehavior(.suppressed)
        .restorationBehavior(.disabled)
        .commandsRemoved() // STUDIO: no Window menu opener; Tools ▸ Import Sequence Package opens it (B83)

        // F14: one wizard at a time.
        Window("Export Wizard", id: WindowID.exportWizard) {
            ExportWizardRoot()
                .environment(model)
                .studioAppearance() // STUDIO: follow the chosen appearance
        }
        .defaultSize(width: 627, height: 575)
        .windowResizability(.contentMinSize)
        .defaultLaunchBehavior(.suppressed)
        .restorationBehavior(.disabled)
        .commandsRemoved() // STUDIO: no Window menu opener; Tools ▸ Export Wizard opens it (B83)

        Window("Shows", id: WindowID.shows) {
            ShowsRoot()
                .environment(model)
                .studioAppearance() // STUDIO: follow the chosen appearance
        }
        .windowResizability(.contentSize)
        .defaultLaunchBehavior(.suppressed)
        .restorationBehavior(.disabled)
        .commandsRemoved() // STUDIO: no Window menu opener; Tools ▸ Scheduler ▸ Shows is the Schedule pane (B83)

        // F49: upstream's title for Scheduler > Schedules.
        Window("Scheduler", id: WindowID.schedules) {
            SchedulesRoot()
                .environment(model)
                .studioAppearance() // STUDIO: follow the chosen appearance
        }
        .windowResizability(.contentSize)
        .defaultLaunchBehavior(.suppressed)
        .restorationBehavior(.disabled)
        .commandsRemoved() // STUDIO: no Window menu opener; Tools ▸ Scheduler ▸ Schedules is the Schedule pane (B83)

        WindowGroup("Export Sequence", id: WindowID.exportSequence, for: String.self) { $path in
            ExportSequenceWindow(path: path)
                .environment(model)
                .studioAppearance() // STUDIO: follow the chosen appearance
        }
        .windowResizability(.contentSize)
        .defaultLaunchBehavior(.suppressed)
        .restorationBehavior(.disabled)
    }
}
