import AppKit
import VixenBridge
import VixenKit

/// The Display pane's graphical patching for VoiceOver and the keyboard (UI roadmap R43): one layout item per shape,
/// read as "White Bottom, element, to Output 1"; Tab and ⇧Tab select shapes column by column, Return (or
/// VoiceOver's VO-⇧-M) opens the selected shape's menu, which adds Connect To (what a drag from its output port does),
/// Expand or Collapse and Configure Filter… to the canvas's own items, and the arrows scroll as before. Words, order
/// and connect targets come from Rust (`studio_access`); connecting goes through the same command as a drag.
extension GraphCanvasView: StudioAccessCanvas {
    public var studioAccessName: String { "graph" }

    public func studioAccessItems() -> [StudioCanvasItem] {
        let access = studioAccess()
        let index = Dictionary(scene.ids.enumerated().map { ($1, $0) }, uniquingKeysWith: { first, _ in first })
        return zip(access.ids, access.labels).compactMap { id, label in
            guard let i = index[id] else { return nil }
            return StudioCanvasItem.make(
                label: label, selected: studioSelectedShapeIDs.contains(id), frame: scene.rects[i], in: self,
                press: { [weak self] in self?.studioAccessSelect(id) },
                menu: { [weak self] in
                    self?.studioAccessSelect(id)
                    self?.studioAccessShowMenu()
                })
        }
    }

    // MARK: VoiceOver

    override func isAccessibilityElement() -> Bool { true }
    override func accessibilityRole() -> NSAccessibility.Role? { .layoutArea }
    override func accessibilityLabel() -> String? { "Patching graph" }
    override func accessibilityHelp() -> String? {
        "Return opens the selected shape's menu, with Connect To. Arrow keys scroll, Delete removes the selection. " + StudioCanvasAccess.keysHelp
    }
    override func accessibilityChildren() -> [Any]? { studioAccessItems() }
    override func accessibilitySelectedChildren() -> [Any]? { studioAccessItems().filter { $0.isAccessibilitySelected() } }

    // MARK: Focus ring

    /// Only when the keyboard brought focus here (B38, as the timeline's #60): a click already shows where it landed.
    override var focusRingMaskBounds: NSRect { StudioCanvasAccess.showsFocusRing(self) ? visibleRect : .zero }
    override func drawFocusRingMask() {
        if StudioCanvasAccess.showsFocusRing(self) { visibleRect.insetBy(dx: 1, dy: 1).fill() }
    }

    override func becomeFirstResponder() -> Bool {
        guard super.becomeFirstResponder() else { return false }
        StudioCanvasAccess.noteFocus(self)
        return true
    }

    /// From the canvas's mouse presses: a click on a canvas that already has the keyboard hides the ring too.
    func studioAccessNoteClick() { StudioCanvasAccess.noteClick(self) }

    // A script's window is never key, so the harness's clicks reach the canvas only as a first click.
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { UITestHarness.isActive || super.acceptsFirstMouse(for: event) }

    // MARK: Keys

    /// Runs before the canvas's own keys: Tab, ⇧Tab, ⌃Tab, ⌃⇧Tab, and Return on a selected shape.
    func studioAccessKeyDown(_ event: NSEvent) -> Bool {
        if let move = StudioCanvasAccess.tabMove(event) {
            switch move {
            case .step(let direction): studioAccessStep(direction)
            case .leave(let forward): StudioCanvasAccess.leave(self, forward: forward)
            }
            return true
        }
        let flags = event.modifierFlags.intersection([.shift, .command, .option, .control])
        if (event.keyCode == 36 || event.keyCode == 76) && flags.isEmpty, studioSelectedIndex() != nil {
            studioAccessShowMenu()
            return true
        }
        return false
    }

    // MARK: Selection and menu

    private func studioAccessStep(_ direction: Int) {
        let order = studioAccess().ids
        guard !order.isEmpty else { return }
        let current = studioSelectedShapeIDs.count == 1 ? order.firstIndex(of: studioSelectedShapeIDs.first!) : nil
        let n = order.count
        let next = switch current {
        case .some(let at): (at + (direction < 0 ? n - 1 : 1)) % n
        case .none: direction < 0 ? n - 1 : 0
        }
        studioAccessSelect(order[next])
    }

    private func studioAccessSelect(_ id: UInt64) {
        guard let i = scene.ids.firstIndex(of: id) else { return }
        studioSelectShape(i)
        let access = studioAccess()
        StudioCanvasAccess.announce(access.ids.firstIndex(of: id).map { access.labels[$0] }, from: self)
    }

    private func studioSelectedIndex() -> Int? {
        guard studioSelectedShapeIDs.count == 1, let id = studioSelectedShapeIDs.first else { return nil }
        return scene.ids.firstIndex(of: id)
    }

    private func studioAccessShowMenu() {
        guard let i = studioSelectedIndex(), let menu = studioAccessMenu() else { return }
        menu.popUp(positioning: nil, at: CGPoint(x: scene.rects[i].midX, y: scene.rects[i].midY), in: self)
    }

    /// The selected shape's menu: Connect To, Expand or Collapse, Configure Filter…, then the canvas's own items,
    /// pasting at the shape's middle.
    public func studioAccessMenu() -> NSMenu? {
        guard let i = studioSelectedIndex() else { return nil }
        studioSetMenuPoint(CGPoint(x: scene.rects[i].midX, y: scene.rects[i].midY))
        let menu = contextMenu()
        var extra: [NSMenuItem] = []
        if scene.outputs[i] > 0 {
            let connect = NSMenuItem(title: "Connect To", action: nil, keyEquivalent: "")
            let sub = NSMenu(title: "Connect To")
            sub.autoenablesItems = false
            let targets = studioConnectTargets(from: scene.ids[i])
            for target in targets {
                guard let t = scene.ids.firstIndex(of: target), scene.components[t] >= 0, scene.components[i] >= 0 else { continue }
                let output = UInt32(studioFirstFreeOutput(of: i))
                let (source, destination) = (UInt32(scene.components[i]), UInt32(scene.components[t]))
                sub.addItem(SetupMenuItem(scene.titles[t]) { [weak self] in
                    self?.controller?.connect(target: destination, source: source, output: output)
                })
            }
            connect.submenu = sub
            connect.isEnabled = !sub.items.isEmpty
            if sub.items.isEmpty { connect.toolTip = "Nothing here can take this shape's output" }
            extra.append(connect)
        }
        if scene.isExpandable(i) {
            let collapsed = scene.flags[i] & 4 != 0
            let id = scene.ids[i]
            extra.append(SetupMenuItem(collapsed ? "Expand" : "Collapse") { [weak self] in self?.controller?.toggleCollapsed(shape: id) })
        }
        if scene.isFilter(i), scene.components[i] >= 0 {
            let component = UInt32(scene.components[i])
            extra.append(SetupMenuItem("Configure Filter…") { [weak self] in self?.controller?.configureFilter(component: component) })
        }
        if !extra.isEmpty {
            for (at, item) in extra.enumerated() { menu.insertItem(item, at: at) }
            menu.insertItem(.separator(), at: extra.count)
        }
        return menu
    }

    // MARK: Rust

    /// The shown scene's words and Tab order, built from the same selection and width as the scene.
    private func studioAccess() -> SetupGraphStudioAccess {
        guard let controller, let model = controller.model, let session = model.session else { return SetupGraphStudioAccess(ids: [], labels: []) }
        return session.studioGraphAccess(selection: model.elementSelection, rows: model.controllerSelection, width: Int32(controller.studioSceneWidth))
    }

    private func studioConnectTargets(from id: UInt64) -> [UInt64] {
        guard let controller, let model = controller.model, let session = model.session else { return [] }
        return session.studioGraphConnectTargets(selection: model.elementSelection, rows: model.controllerSelection, width: Int32(controller.studioSceneWidth), source: id)
    }
}
