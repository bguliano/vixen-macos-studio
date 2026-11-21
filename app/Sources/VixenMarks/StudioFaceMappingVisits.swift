import AppKit
import VixenBridge
import VixenKit

// Rest of R40: Tools ▸ LipSync ▸ Edit Element Face Mapping no longer commits through Cancel/OK. Choosing elements changes nothing;
// Set Up Faces… opens Face Setup on them, which writes the Face properties as it's used, and the whole visit is one step on the
// profile's shared undo history (`studio_face_mapping.rs`, the history agents' `face_mapping_set` uses). The node select sheet
// mirrors each visit's step into its window's Edit ▸ Undo, stays up for more, and closes with Done; the show window's panes list
// the steps with the rest.

/// The node select sheet's Face Setup visits.
@MainActor @Observable
final class StudioFaceMappingVisits {
    @ObservationIgnored let core: VixenCore
    @ObservationIgnored private var visit: StudioFaceSetupVisit?
    /// The node select sheet window's undo manager, which mirrors the visits' steps.
    @ObservationIgnored weak var manager: UndoManager?
    /// Whether a visit, or its Undo or Redo, changed the Face properties since the sheet opened: the sequencer refreshes on Done.
    private(set) var changed = false
    /// The last visit's step, for the harness.
    private(set) var lastLabel: String?
    static weak var current: StudioFaceMappingVisits?

    init(core: VixenCore) {
        self.core = core
    }

    /// The sheet is up, with its window's undo manager.
    func attach(_ manager: UndoManager?) {
        self.manager = manager
        StudioFaceMappingVisits.current = self
    }

    /// Face Setup is about to open on `nodes`.
    func begin(nodes: [String]) {
        visit = core.studioBeginFaceSetup(nodes: nodes)
    }

    /// Face Setup closed: its changes become one step, registered on the node select sheet's undo manager. The outcome, or none
    /// when no visit was open.
    func finish() -> StudioFaceSetupOutcome? {
        guard let visit else { return nil }
        self.visit = nil
        let outcome = visit.finish()
        changed = changed || outcome.changed
        lastLabel = outcome.label
        if let label = outcome.label {
            register(label)
        }
        return outcome
    }

    private final class Target: NSObject {}
    @ObservationIgnored private let target = Target()

    private func register(_ label: String) {
        guard let manager, manager.groupingLevel == 0 else { return }
        let automatic = manager.groupsByEvent
        manager.groupsByEvent = false
        manager.beginUndoGrouping()
        register(label, manager: manager, undoing: true)
        manager.endUndoGrouping()
        manager.groupsByEvent = automatic
    }

    /// Undo and Redo reach the shared history only while the visit's step is still the latest one there; otherwise they beep.
    private func register(_ label: String, manager: UndoManager, undoing: Bool) {
        manager.registerUndo(withTarget: target) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self else { return }
                let done = undoing ? self.core.studioUndoFaceMapping(label: label) : self.core.studioRedoFaceMapping(label: label)
                if done {
                    self.changed = true
                    self.register(label, manager: manager, undoing: !undoing)
                } else {
                    NSSound.beep()
                }
            }
        }
        manager.setActionName(label)
    }
}

/// What test scripts can ask of the node select sheet's visits: `{ "action": "hook", "name": "studioFaceMapping", "path": ["state"] }`.
enum StudioFaceMappingTestHooks {
    private static var installed = false

    static func install() {
        guard UITestHarness.isActive, !installed else { return }
        installed = true
        UITestHarness.hooks["studioFaceMapping"] = { args in
            guard let verb = args.first else { return "FAIL needs a verb" }
            if verb == "closed" {
                // What the sheet last reported to whoever opened it (true: something changed, so the sequencer refreshes).
                return "lastClose \(FaceNodeSelectTestHooks.lastClose.map(String.init) ?? "-")"
            }
            guard let visits = StudioFaceMappingVisits.current else { return "FAIL no Node Select sheet" }
            let manager = visits.manager
            switch verb {
            case "state":
                return "changed \(visits.changed) label \(visits.lastLabel ?? "-")"
            case "list":
                // Each element with a Face property: its name and how many mouth shapes it draws.
                let data = Data(visits.core.studioFaceMappingList().utf8)
                let elements = (try? JSONSerialization.jsonObject(with: data) as? [String: Any])?["elements"] as? [[String: Any]] ?? []
                return elements.isEmpty ? "(none)" : elements.map { "\($0["name"] ?? "?"):\(($0["phonemes"] as? [Any])?.count ?? 0)" }.joined(separator: ", ")
            case "undoMenu":
                return "undo \(manager?.canUndo == true ? manager!.undoMenuItemTitle : "-") redo \(manager?.canRedo == true ? manager!.redoMenuItemTitle : "-")"
            case "undo":
                // What Edit ▸ Undo does in the sheet: scripts run the app in the background, where no window has the keyboard.
                guard let manager, manager.canUndo else { return "FAIL nothing to undo" }
                manager.undo()
                return "undone"
            case "redo":
                guard let manager, manager.canRedo else { return "FAIL nothing to redo" }
                manager.redo()
                return "redone"
            case "history":
                return visits.core.studioDisplayHistory()
            default:
                return "FAIL unknown verb \(verb)"
            }
        }
    }
}
