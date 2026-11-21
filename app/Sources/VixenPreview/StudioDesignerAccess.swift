import AppKit
import VixenBridge
import VixenKit

/// The preview designer's canvas for VoiceOver and the keyboard (UI roadmap R43): one layout item per prop, read as
/// "Poly Line, 161 lights, linked to White Bottom"; a press selects the prop, VO-⇧-M opens its context menu, Tab and
/// ⇧Tab select the next and previous unlocked prop, and the arrows already nudge it (⇧ for bigger steps). Words and
/// moves come from Rust (`studio_access` in vixen-preview).
extension DesignerCanvasView: StudioAccessCanvas {
    public var studioAccessName: String { "designer" }

    public func studioAccessItems() -> [StudioCanvasItem] {
        guard let designer = model?.designer else { return [] }
        return designer.studioAccessItems().map { item in
            let frame = NSRect(x: CGFloat(item.x), y: CGFloat(item.y), width: CGFloat(item.w), height: CGFloat(item.h))
            return StudioCanvasItem.make(
                label: item.label, selected: item.selected, frame: frame, in: self,
                press: { [weak self] in self?.studioAccessApply(self?.model?.designer?.studioAccessSelect(shape: item.shape)) },
                menu: { [weak self] in self?.studioAccessShowMenu(at: NSPoint(x: frame.midX, y: frame.midY)) })
        }
    }

    // MARK: VoiceOver

    override func isAccessibilityElement() -> Bool { true }
    override func accessibilityRole() -> NSAccessibility.Role? { .layoutArea }
    override func accessibilityLabel() -> String? { "Preview layout" }
    override func accessibilityHelp() -> String? { "Arrow keys move the selected prop, Delete removes it. " + StudioCanvasAccess.keysHelp }
    override func accessibilityChildren() -> [Any]? { studioAccessItems() + (super.accessibilityChildren() ?? []) }
    override func accessibilitySelectedChildren() -> [Any]? { studioAccessItems().filter { $0.isAccessibilitySelected() } }

    // MARK: Focus ring

    override var focusRingMaskBounds: NSRect { bounds }
    override func drawFocusRingMask() { bounds.insetBy(dx: 1, dy: 1).fill() }

    // MARK: Keys

    /// Runs before the designer's own keys. True when the key was Tab, ⇧Tab, ⌃Tab or ⌃⇧Tab.
    func studioAccessKeyDown(_ event: NSEvent) -> Bool {
        guard let move = StudioCanvasAccess.tabMove(event) else { return false }
        switch move {
        case .step(let direction): studioAccessApply(model?.designer?.studioAccessStep(direction: Int32(direction)))
        case .leave(let forward): StudioCanvasAccess.leave(self, forward: forward)
        }
        return true
    }

    private func studioAccessApply(_ move: DesignerStudioAccessMove?) {
        guard let move, let model else { return }
        model.apply(move.change)
        StudioCanvasAccess.announce(move.announcement, from: self)
    }

    /// VoiceOver's show-menu on a prop: the right press there, which selects it, and its menu.
    private func studioAccessShowMenu(at point: NSPoint) {
        guard let menu = contextMenu(atX: Int32(point.x.rounded(.down)), y: Int32(point.y.rounded(.down)), mods: DesignerMods(command: false, shift: false, option: false)) else { return }
        menu.popUp(positioning: nil, at: point, in: self)
    }
}
