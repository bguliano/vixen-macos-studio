import AppKit
import VixenBridge
import VixenKit

/// The timeline grid for VoiceOver and the keyboard (UI roadmap R43): one layout item per effect in view, read as
/// "Pulse on Mega Tree, 0:12.500 to 0:13.000"; ← and → (and Tab, ⇧Tab) step through the active row's effects, ⇧ with an
/// arrow extends the selection, ↑ and ↓ change rows, ⌥→ and ⌥← open and close the active row, and ⌃Tab moves on, to the
/// inspector that edits the selected effect. Words and moves come from Rust (`studio_access`).
extension GridCanvasView: StudioAccessCanvas {
    public var studioAccessName: String { "timeline" }

    public func studioAccessItems() -> [StudioCanvasItem] {
        controller.studioAccessEffects().map { effect in
            StudioCanvasItem.make(
                label: effect.label, selected: effect.selected,
                frame: NSRect(x: CGFloat(effect.x), y: CGFloat(effect.y), width: CGFloat(effect.w), height: CGFloat(effect.h)), in: self,
                press: { [weak self] in self?.studioAccessApply(self?.controller.studioAccessSelect(id: effect.id, row: effect.row)) })
        }
    }

    // MARK: VoiceOver

    override func isAccessibilityElement() -> Bool { true }
    override func accessibilityRole() -> NSAccessibility.Role? { .layoutArea }
    override func accessibilityLabel() -> String? { "Timeline" }
    override func accessibilityValue() -> Any? { controller.studioAccessSummary() }
    override func accessibilityHelp() -> String? {
        "Left and Right Arrow select effects in the active row, Up and Down Arrow change rows, Option-Right and Option-Left open and close a row. "
            + StudioCanvasAccess.keysHelp
    }
    override func accessibilityChildren() -> [Any]? { studioAccessItems() }
    override func accessibilitySelectedChildren() -> [Any]? { studioAccessItems().filter { $0.isAccessibilitySelected() } }

    // MARK: Focus ring

    /// Only when the keyboard brought focus here (#60): a click already shows where it landed.
    override var focusRingMaskBounds: NSRect { StudioCanvasAccess.showsFocusRing(self) ? bounds : .zero }
    override func drawFocusRingMask() {
        if StudioCanvasAccess.showsFocusRing(self) { bounds.insetBy(dx: 1, dy: 1).fill() }
    }

    override func becomeFirstResponder() -> Bool {
        guard super.becomeFirstResponder() else { return false }
        StudioCanvasAccess.noteFocus(self)
        return true
    }

    // A click on a grid that already has the keyboard hides the ring too.
    override func mouseDown(with event: NSEvent) {
        StudioCanvasAccess.noteClick(self)
        super.mouseDown(with: event)
    }

    override func rightMouseDown(with event: NSEvent) {
        StudioCanvasAccess.noteClick(self)
        super.rightMouseDown(with: event)
    }

    // MARK: Keys

    /// Runs before the grid's own keys. True when the key was the timeline's to handle here.
    func studioAccessKeyDown(_ event: NSEvent) -> Bool {
        if let move = StudioCanvasAccess.tabMove(event) {
            switch move {
            case .step(let direction): studioAccessApply(controller.studioAccessStep(direction: Int32(direction), extend: false))
            case .leave(let forward): StudioCanvasAccess.leave(self, forward: forward)
            }
            return true
        }
        let flags = event.modifierFlags.intersection([.shift, .command, .option, .control])
        let code = event.keyCode
        if code == 123 || code == 124 { // ← →
            if flags.isEmpty || flags == .shift {
                studioAccessApply(controller.studioAccessStep(direction: code == 124 ? 1 : -1, extend: flags == .shift))
                return true
            }
            if flags == .option {
                studioAccessApply(controller.studioAccessOpenRow(open: code == 124), rows: true)
                return true
            }
        }
        if (code == 125 || code == 126) && flags.isEmpty { // ↓ ↑, which also give the grid a row when it has none
            studioAccessApply(controller.studioAccessMoveRow(direction: code == 125 ? 1 : -1), rows: true)
            return true
        }
        return false
    }

    /// Reads out the active row after the sequencer's own ↑ or ↓ moved it, while the grid has the keyboard.
    public func studioAccessAnnounceRow() {
        guard window?.firstResponder === self else { return }
        StudioCanvasAccess.announce(controller.studioAccessRowLabel(), from: self)
    }

    private func studioAccessApply(_ move: TimelineStudioAccessMove?, rows: Bool = false) {
        guard let move, let canvas else { return }
        canvas.apply(move.update)
        if rows { canvas.rows.plane.invalidateAll() }
        StudioCanvasAccess.announce(move.announcement, from: self)
    }
}

extension TimelineCanvas {
    /// VoiceOver reads the active row, when the grid has the keyboard: for the sequencer's own ↑ and ↓.
    public func studioAccessAnnounceRow() {
        grid.studioAccessAnnounceRow()
    }
}
