import AppKit
import VixenBridge

// Studio (B27): the Color Gradient Editor is live instead of committing through OK/Cancel, as the curve editor is (B24).
// Each finished change (a stop added, dropped or deleted, a colour picked, a location typed, Load or Unlink, an edited
// library gradient) goes to the editor's owner at once: the effect editor's gradient row or a library entry. Each is a
// step on the sheet's own Edit ▸ Undo while it's up, and Done (or Esc) closes and keeps everything. The effect editor
// records the whole visit as one step on the sequence's Undo.

/// One gradient editor visit: hands each change to the owner and keeps the sheet's undo steps.
@MainActor
final class StudioGradientLive: NSObject {
    private let onChange: (GradientValue) -> Void
    private let onDone: (Bool) -> Void
    private var published: GradientValue
    private(set) var changed = false
    weak var undo: UndoManager?
    /// The editor on screen, for the harness.
    static weak var current: StudioGradientLive?
    weak var model: GradientEditorViewModel?

    init(gradient: GradientValue, onChange: @escaping (GradientValue) -> Void, onDone: @escaping (Bool) -> Void) {
        published = gradient
        self.onChange = onChange
        self.onDone = onDone
    }

    /// The session's gradient may have changed. A mouse still down waits for its release, so a drag is one change.
    func sessionChanged() {
        guard let model, !model.pointerDown else { return }
        let gradient = model.session.result()
        guard gradient != published else { return }
        let old = published
        published = gradient
        changed = true
        register(restoring: old)
        onChange(gradient)
    }

    private func register(restoring old: GradientValue) {
        guard let undo else { return }
        let group = !undo.groupsByEvent && !undo.isUndoing && !undo.isRedoing
        if group { undo.beginUndoGrouping() }
        undo.registerUndo(withTarget: self) { $0.restore(old) }
        undo.setActionName("Edit Gradient")
        if group { undo.endUndoGrouping() }
    }

    /// Undo and Redo: the editor shows `old` again and the owner gets it, registering what it replaces as the opposite step.
    private func restore(_ old: GradientValue) {
        guard let model else { return }
        model.finishColorEdit()
        model.pointerDown = false
        model.state = model.session.restore(gradient: old)
    }

    /// Done: the sheet's steps go (the owner's Undo holds the visit now) and the owner hears whether anything changed.
    func finish() {
        model?.finishColorEdit()
        undo?.removeAllActions(withTarget: self)
        if StudioGradientLive.current === self { StudioGradientLive.current = nil }
        onDone(changed)
    }

    /// Text for the harness.
    var summary: String {
        StudioGradientLive.describe(published)
            + " library=\(published.libraryName) changed=\(changed) undo=\(undo?.canUndo == true ? undo?.undoActionName ?? "-" : "-") redo=\(undo?.canRedo == true ? undo?.redoActionName ?? "-" : "-")"
    }

    /// `stops=` each stop's ARGB in hex and position in percent, sorted by position.
    static func describe(_ gradient: GradientValue) -> String {
        let stops = gradient.stops.sorted { $0.position < $1.position }
            .map { String(format: "%08X@%d", $0.argb, Int(($0.position * 100).rounded())) }
            .joined(separator: " ")
        return "stops=\(stops)"
    }
}

/// `studio.gradientEditor` with path ["state"], ["select", index] (selects the stop that's `index` by position),
/// ["color", ARGB hex] (sets the selected stop's colour), ["location", percent], ["delete"], ["undo"], ["redo"],
/// ["responder-undo"], ["responder-redo"] (Edit ▸ Undo through the key window's responder chain), ["escape"] (a real Esc),
/// ["done"].
@MainActor
enum StudioGradientEditorHarness {
    static func install() {
        guard UITestHarness.isActive else { return }
        UITestHarness.hooks["studio.gradientEditor"] = { args in
            guard let live = StudioGradientLive.current, let model = live.model else { return "FAIL no gradient editor" }
            switch args.first {
            case "select" where args.count == 2:
                guard let index = Int(args[1]), index < model.state.faders.count else { return "FAIL no fader \(args[1])" }
                let fader = model.state.faders[index]
                let width = model.barSize.width, height = model.barSize.height
                // A click on the fader's body, left of where its diamond shows, as GradientBarNSView's mouseDown sends it.
                model.pointerDown = true
                model.state = model.session.press(x: fader.x + 3, y: height - 6, width: width, height: height)
                model.pointerDown = false
                model.state = model.session.release(x: fader.x + 3, y: height - 6, width: width, height: height)
                guard model.state.hasSelection else { return "FAIL nothing selected" }
            case "color" where args.count == 2:
                guard let argb = UInt32(args[1], radix: 16) else { return "FAIL bad colour" }
                model.state = model.session.setSelectedColor(argb: argb)
            case "location" where args.count == 2:
                guard let value = Int32(args[1]) else { return "FAIL bad location" }
                model.state = model.session.setLocation(value: value)
            case "delete": model.state = model.session.deleteSelected()
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
