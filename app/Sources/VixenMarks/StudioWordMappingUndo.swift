import AppKit
import VixenBridge
import VixenKit

// Rest of R40: the Text Conversion Failure sheet's Map Word is one step on the profile's shared undo history
// (`studio_phoneme_mappings.rs`, the `phoneme_mapping_set` command agents run), since the user dictionary is a profile file, not the
// sequence. The Lyric Converter window mirrors that step into its Edit ▸ Undo; the show window's panes list it with the rest.

/// Registers a word mapping's step on a window's undo manager. Undo and Redo reach the shared history only while that step is still
/// the latest one there; otherwise they beep and leave the history alone.
@MainActor
enum StudioWordMappingUndo {
    private final class Target: NSObject {}
    private static let target = Target()

    static func register(_ label: String, session: SequencerSession, on manager: UndoManager?) {
        guard let manager, manager.groupingLevel == 0 else { return }
        let automatic = manager.groupsByEvent
        manager.groupsByEvent = false
        manager.beginUndoGrouping()
        register(label, session: session, manager: manager, undoing: true)
        manager.endUndoGrouping()
        manager.groupsByEvent = automatic
    }

    private static func register(_ label: String, session: SequencerSession, manager: UndoManager, undoing: Bool) {
        manager.registerUndo(withTarget: target) { _ in
            MainActor.assumeIsolated {
                let done = undoing ? session.studioUndoWordMapping(label: label) : session.studioRedoWordMapping(label: label)
                if done {
                    register(label, session: session, manager: manager, undoing: !undoing)
                } else {
                    NSSound.beep()
                }
            }
        }
        manager.setActionName(label)
    }
}

/// What test scripts can ask of the Lyric Converter's word mapping: `{ "action": "hook", "name": "studioLyric", "path": ["undoMenu"] }`.
enum StudioLyricTestHooks {
    static func install() {
        UITestHarness.hooks["studioLyric"] = { args in
            guard let model = LyricTestHooks.current, let verb = args.first else { return "FAIL needs the converter and a verb" }
            let window = model.textView?.window
            switch verb {
            case "convert":
                // What the Convert button does.
                model.convert()
                return "outcome \(model.lastOutcome) asking \(model.failure?.word ?? "-")"
            case "undoMenu":
                // Edit ▸ Undo sends undo: to the window with the keyboard: the converter, whose undo manager names the step.
                let manager = window?.undoManager
                return "undo \(manager?.canUndo == true ? manager!.undoMenuItemTitle : "-") redo \(manager?.canRedo == true ? manager!.redoMenuItemTitle : "-")"
            case "sendUndo", "sendRedo":
                // Scripts run the app in the background, where no window has the keyboard, so the action goes to the window itself.
                guard let window else { return "FAIL no converter window" }
                window.makeFirstResponder(nil)
                let sent = window.tryToPerform(Selector(verb == "sendUndo" ? "undo:" : "redo:"), with: nil)
                return sent ? verb : "FAIL nothing to \(verb)"
            default:
                return "FAIL unknown verb \(verb)"
            }
        }
    }
}
