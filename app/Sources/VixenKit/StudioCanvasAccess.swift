import AppKit

/// VoiceOver and the keyboard on the canvases that draw their own contents: the timeline grid, the patching graph and
/// the preview designer (UI roadmap R43). Each canvas is a layout area holding one layout item per visible effect,
/// shape or prop, with words from Rust (`studio_access`); a press selects the item and, where the canvas has one, the
/// item's menu opens with VoiceOver's VO-⇧-M. The keys are the same on all three: Tab and ⇧Tab select the next and
/// previous item inside the canvas, ⌃Tab and ⌃⇧Tab move on to the next control, and a focus ring shows which canvas has
/// the keyboard.
@MainActor
public protocol StudioAccessCanvas: NSView {
    /// The canvas's name for VoiceOver and for the harness (`studio.access`).
    var studioAccessName: String { get }
    /// The items as the canvas's accessibility children last built them.
    func studioAccessItems() -> [StudioCanvasItem]
    /// The menu the keyboard opens for the selected item, or nil when the canvas has none for it.
    func studioAccessMenu() -> NSMenu?
}

public extension StudioAccessCanvas {
    func studioAccessMenu() -> NSMenu? { nil }
}

/// One effect, shape or prop on a canvas.
public nonisolated final class StudioCanvasItem: NSAccessibilityElement {
    private let press: @MainActor () -> Void
    private let menu: (@MainActor () -> Void)?

    /// `frame` is in the canvas's coordinates measured from its top left, as the canvases' Rust frames are.
    @MainActor
    public static func make(label: String, selected: Bool, frame: NSRect, in canvas: NSView, press: @escaping @MainActor () -> Void, menu: (@MainActor () -> Void)? = nil) -> StudioCanvasItem {
        // Parent space is the view's own coordinate system, which runs bottom up unless the view is flipped.
        let rect = canvas.isFlipped ? frame : NSRect(x: frame.minX, y: canvas.bounds.height - frame.maxY, width: frame.width, height: frame.height)
        return StudioCanvasItem(label: label, selected: selected, frameInParent: rect, parent: canvas, press: press, menu: menu)
    }

    init(label: String, selected: Bool, frameInParent: NSRect, parent: NSView, press: @escaping @MainActor () -> Void, menu: (@MainActor () -> Void)?) {
        self.press = press
        self.menu = menu
        super.init()
        setAccessibilityRole(.layoutItem)
        setAccessibilityLabel(label)
        setAccessibilitySelected(selected)
        setAccessibilityParent(parent)
        setAccessibilityFrameInParentSpace(frameInParent)
    }

    public override func accessibilityPerformPress() -> Bool {
        let press = self.press
        MainActor.assumeIsolated { press() }
        return true
    }

    public override func accessibilityPerformShowMenu() -> Bool {
        guard let menu else { return false }
        MainActor.assumeIsolated { menu() }
        return true
    }
}

@MainActor
public enum StudioCanvasAccess {
    /// What a key does on a canvas: step the selection (1 or -1), or move the keyboard out of the canvas.
    public enum TabMove {
        case step(Int)
        case leave(forward: Bool)
    }

    /// Tab, ⇧Tab, ⌃Tab and ⌃⇧Tab, or nil for any other key.
    public static func tabMove(_ event: NSEvent) -> TabMove? {
        guard event.keyCode == 48 else { return nil }
        let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        let back = flags.contains(.shift)
        if flags.contains(.control) { return .leave(forward: !back) }
        guard !flags.contains(.command), !flags.contains(.option) else { return nil }
        return .step(back ? -1 : 1)
    }

    /// ⌃Tab: the next control in the window's key view loop.
    public static func leave(_ view: NSView, forward: Bool) {
        if forward { view.window?.selectKeyView(following: view) } else { view.window?.selectKeyView(preceding: view) }
    }

    /// Says where the keyboard landed, and tells VoiceOver the canvas's selection changed.
    public static func announce(_ text: String?, from view: NSView) {
        NSAccessibility.post(element: view, notification: .selectedChildrenChanged)
        lastAnnouncement = text ?? lastAnnouncement
        guard let text, !text.isEmpty else { return }
        NSAccessibility.post(element: NSApp as Any, notification: .announcementRequested, userInfo: [.announcement: text, .priority: NSAccessibilityPriorityLevel.high.rawValue])
    }

    /// The last thing announced, for the harness.
    public private(set) static var lastAnnouncement = ""

    /// The canvas's own help, read after its name.
    public static let keysHelp = "Tab and Shift-Tab select the next and previous item. Control-Tab moves to the next control."

    // MARK: Focus ring

    /// Canvases a click gave the keyboard to. Their focus ring stays hidden, as a button's does after a click, until the
    /// keyboard (Tab, ⌃Tab, the harness's `focus`) brings focus back.
    private static let clickFocused = NSHashTable<NSView>.weakObjects()

    /// From the canvas's `becomeFirstResponder`: whether a mouse press gave it the keyboard decides its ring.
    public static func noteFocus(_ view: NSView) {
        switch NSApp.currentEvent?.type {
        case .leftMouseDown, .rightMouseDown, .otherMouseDown: clickFocused.add(view)
        default: clickFocused.remove(view)
        }
        view.noteFocusRingMaskChanged()
    }

    /// From the canvas's mouse presses, for a canvas that already has the keyboard.
    public static func noteClick(_ view: NSView) {
        guard !clickFocused.contains(view) else { return }
        clickFocused.add(view)
        view.noteFocusRingMaskChanged()
    }

    /// Whether the canvas draws its focus ring while it has the keyboard.
    public static func showsFocusRing(_ view: NSView) -> Bool { !clickFocused.contains(view) }

    // MARK: Harness

    /// `{ "action": "hook", "name": "studio.access", "path": [...] }`, with the canvas named as `studioAccessName`
    /// says:
    /// - `focus <canvas>`: gives that canvas in the key window the keyboard, as clicking it or tabbing to it would.
    /// - `report <canvas>`: answers `focused=<0|1> role=<role> label=<label> items=<n> frames=<selected items' x,y,w,h>
    ///   selected=<labels> said=<last announcement> ring=<0|1>` (ring: focused and drawing its focus ring) and writes the canvas's items, one per line (`*` before selected ones, then `@ x,y,w,h` in
    ///   screen points), to `access-<canvas>.txt`.
    /// - `menu <canvas>`: the titles of the menu Return opens for the selected item, `|` between them, `>` before a
    ///   submenu's.
    /// - `choose <canvas> <title> [<submenu title>]`: runs that item of the selected item's menu, as choosing it would.
    /// - `click <canvas>`: posts a left click on an empty corner of the canvas, where no item is, for the focus ring checks.
    ///   A script's window is never key, so the canvas must take a first click (`acceptsFirstMouse`), at least while the harness runs.
    public static func installHarnessHook() {
        guard UITestHarness.isActive, UITestHarness.hooks["studio.access"] == nil else { return }
        UITestHarness.hooks["studio.access"] = { args in
            guard args.count >= 2 else { return "FAIL focus|report <canvas>" }
            let name = args[0] == "choose" ? args[1] : args[1...].joined(separator: " ")
            guard let canvas = canvas(named: name) else { return "FAIL no canvas \(name)" }
            switch args[0] {
            case "focus":
                guard canvas.window?.makeFirstResponder(canvas) == true else { return "FAIL \(name) refused focus" }
                return "focused"
            case "report":
                let items = canvas.studioAccessItems()
                let lines = items.map { item in
                    let f = item.accessibilityFrame()
                    return (item.isAccessibilitySelected() ? "* " : "  ") + (item.accessibilityLabel() ?? "") + " @ \(Int(f.minX)),\(Int(f.minY)),\(Int(f.width)),\(Int(f.height))"
                }
                if let out = ProcessInfo.processInfo.environment["VIXEN_UI_TEST_OUT"] {
                    let file = URL(fileURLWithPath: out).appending(path: "access-\(name.replacingOccurrences(of: " ", with: "-")).txt")
                    try? lines.joined(separator: "\n").write(to: file, atomically: true, encoding: .utf8)
                }
                let chosen = items.filter { $0.isAccessibilitySelected() }
                let selected = chosen.compactMap { $0.accessibilityLabel() }
                let frames = chosen.map { let f = $0.accessibilityFrame(); return "\(Int(f.minX)),\(Int(f.minY)),\(Int(f.width)),\(Int(f.height))" }
                let focused = canvas.window?.firstResponder === canvas
                return "focused=\(focused ? 1 : 0) role=\(canvas.accessibilityRole()?.rawValue ?? "") label=\(canvas.accessibilityLabel() ?? "") items=\(items.count) frames=\(frames.joined(separator: ";")) selected=\(selected.joined(separator: " | ")) said=\(lastAnnouncement) ring=\(focused && showsFocusRing(canvas) ? 1 : 0)"
            case "click":
                guard let window = canvas.window else { return "FAIL \(name) has no window" }
                let frames = canvas.studioAccessItems().map { $0.accessibilityFrame() }
                let r = canvas.visibleRect.insetBy(dx: 8, dy: 8)
                let corners = [NSPoint(x: r.maxX, y: r.maxY), NSPoint(x: r.minX, y: r.maxY), NSPoint(x: r.maxX, y: r.minY), NSPoint(x: r.minX, y: r.minY)]
                guard let point = corners.map({ canvas.convert($0, to: nil) }).first(where: { p in
                    canvas.superview.map { canvas.hitTest($0.convert(p, from: nil)) === canvas } == true
                        && !frames.contains { $0.contains(window.convertPoint(toScreen: p)) }
                }) else { return "FAIL no empty corner on \(name)" }
                window.makeKey()
                for type in [NSEvent.EventType.leftMouseDown, .leftMouseUp] {
                    if let e = NSEvent.mouseEvent(
                        with: type, location: point, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
                        windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: type == .leftMouseDown ? 1 : 0
                    ) {
                        NSApp.postEvent(e, atStart: false)
                    }
                }
                return "clicked"
            case "menu":
                guard let menu = canvas.studioAccessMenu() else { return "FAIL no menu" }
                return menu.items.filter { !$0.isSeparatorItem }.map { item in
                    item.title + (item.submenu.map { " > " + $0.items.map(\.title).joined(separator: ", ") } ?? "") + (item.isEnabled ? "" : " (off)")
                }.joined(separator: " | ")
            case "choose":
                // The canvas name is one word here; the rest are menu titles.
                guard let menu = canvas.studioAccessMenu() else { return "FAIL no menu" }
                var current = menu
                for (n, title) in args.dropFirst(2).enumerated() {
                    guard let index = current.items.firstIndex(where: { $0.title == title }) else { return "FAIL no item \(title)" }
                    if n == args.count - 3 {
                        guard current.items[index].isEnabled else { return "FAIL \(title) is off" }
                        current.performActionForItem(at: index)
                        return "chose \(title)"
                    }
                    guard let sub = current.items[index].submenu else { return "FAIL \(title) has no submenu" }
                    current = sub
                }
                return "FAIL choose <canvas> <title>..."
            default:
                return "FAIL unknown access step \(args[0])"
            }
        }
    }

    private static func canvas(named name: String) -> (any StudioAccessCanvas)? {
        var windows = NSApp.orderedWindows.filter(\.isVisible)
        if let key = NSApp.keyWindow { windows.insert(key, at: 0) }
        for window in windows {
            guard let root = window.contentView else { continue }
            if let found = find(name, in: root) { return found }
        }
        return nil
    }

    private static func find(_ name: String, in view: NSView) -> (any StudioAccessCanvas)? {
        if let canvas = view as? any StudioAccessCanvas, canvas.studioAccessName == name, !canvas.isHiddenOrHasHiddenAncestor { return canvas }
        for sub in view.subviews {
            if let found = find(name, in: sub) { return found }
        }
        return nil
    }
}
