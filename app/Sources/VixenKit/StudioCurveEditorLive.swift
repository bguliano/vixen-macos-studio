import AppKit
import VixenBridge

// Studio (B24): the curve editor is live instead of committing through OK/Cancel. Each finished change (a point dropped,
// Update, Reverse, a drawn or generated curve, Load or Unlink) goes to the editor's owner at once: the effect editor's row,
// the Dimming Curve filter, a library entry, a helper or the fixture wizard. Each is a step on the sheet's own Edit ▸ Undo
// while it's up, and Done (or Esc) closes and keeps everything. The effect editor records the whole visit as one step on
// the sequence's Undo; the Dimming Curve filter's visit is one step on Display Setup's Undo, as its other setup sheets are.

/// One curve editor visit: hands each change to the owner and keeps the sheet's undo steps.
@MainActor
final class StudioCurveLive: NSObject {
    private let onChange: (CurveValue) -> Void
    private let onDone: (Bool) -> Void
    private var published: CurveValue
    private(set) var changed = false
    weak var undo: UndoManager?
    /// The editor on screen, for the harness.
    static weak var current: StudioCurveLive?
    weak var model: CurveEditorViewModel?

    init(curve: CurveValue, onChange: @escaping (CurveValue) -> Void, onDone: @escaping (Bool) -> Void) {
        published = curve
        self.onChange = onChange
        self.onDone = onDone
    }

    /// The session's curve may have changed. A mouse still down waits for its release, so a drag is one change.
    func sessionChanged() {
        guard let model, !model.pointerDown else { return }
        let curve = model.session.result()
        guard curve != published else { return }
        let old = published
        published = curve
        changed = true
        register(restoring: old)
        onChange(curve)
    }

    private func register(restoring old: CurveValue) {
        guard let undo else { return }
        let group = !undo.groupsByEvent && !undo.isUndoing && !undo.isRedoing
        if group { undo.beginUndoGrouping() }
        undo.registerUndo(withTarget: self) { $0.restore(old) }
        undo.setActionName("Edit Curve")
        if group { undo.endUndoGrouping() }
    }

    /// Undo and Redo: the editor shows `old` again and the owner gets it, registering what it replaces as the opposite step.
    private func restore(_ old: CurveValue) {
        guard let model else { return }
        model.pointerDown = false
        model.apply(model.session.restore(curve: old))
    }

    /// Done: the sheet's steps go (the owner's Undo holds the visit now) and the owner hears whether anything changed.
    func finish() {
        undo?.removeAllActions(withTarget: self)
        if StudioCurveLive.current === self { StudioCurveLive.current = nil }
        onDone(changed)
    }

    /// Text for the harness.
    var summary: String {
        let points = published.points.map { "\(Int($0.x.rounded())),\(Int($0.y.rounded()))" }.joined(separator: " ")
        return "points=\(points) library=\(published.libraryName) changed=\(changed) undo=\(undo?.canUndo == true ? undo?.undoActionName ?? "-" : "-") redo=\(undo?.canRedo == true ? undo?.redoActionName ?? "-" : "-")"
    }
}

/// `studio.curveEditor` with path ["state"], ["reverse"], ["invert"], ["function", f(x)], ["undo"], ["redo"], ["responder-undo"],
/// ["responder-redo"] (Edit ▸ Undo through the key window's responder chain), ["escape"] (a real Esc), ["done"].
@MainActor
enum StudioCurveEditorHarness {
    static func install() {
        guard UITestHarness.isActive else { return }
        UITestHarness.hooks["studio.curveEditor"] = { args in
            guard let live = StudioCurveLive.current, let model = live.model else { return "FAIL no curve editor" }
            switch args.first {
            case "reverse": model.apply(model.session.reverse())
            case "invert": model.apply(model.session.invert())
            case "function" where args.count == 2:
                if let problem = model.session.applyFunction(function: args[1]) { return "FAIL \(problem.message)" }
                model.apply(model.session.state())
            case "undo": live.undo?.undo()
            case "redo": live.undo?.redo()
            case "responder-undo", "responder-redo":
                let sheetWindow = NSApp.keyWindow ?? NSApp.windows.lazy.compactMap(\.attachedSheet).first
                guard let window = sheetWindow, let responder = window.firstResponder else { return "FAIL no sheet window" }
                guard responder.tryToPerform(NSSelectorFromString(args[0] == "responder-undo" ? "undo:" : "redo:"), with: nil) else {
                    return "FAIL nothing in the chain handled it"
                }
            case "escape":
                // Through NSApp, as a real key press goes, so the key-equivalent pass runs before a focused field sees it.
                let sheetWindow = NSApp.windows.lazy.compactMap(\.attachedSheet).first
                guard let window = sheetWindow else { return "FAIL no sheet window" }
                for type in [NSEvent.EventType.keyDown, .keyUp] {
                    guard let event = NSEvent.keyEvent(
                        with: type, location: .zero, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
                        windowNumber: window.windowNumber, context: nil, characters: "\u{1b}", charactersIgnoringModifiers: "\u{1b}",
                        isARepeat: false, keyCode: 53
                    ) else { return "FAIL could not build event" }
                    NSApp.sendEvent(event)
                }
                return "sent escape"
            case "done":
                let summary = live.summary
                live.finish()
                return summary
            default: break
            }
            return live.summary
        }
    }
}
