import AppKit

/// Checks a window's layout from its accessibility tree, for the adaptive-layout pass (UI roadmap R42): every window should
/// lay out at its minimum size, in light and dark, without clipped or overlapping controls.
///
/// `{ "action": "hook", "name": "studio.layout", "path": [...] }`:
/// - `appearance light|dark|system`: sets Settings ▸ Appearance, as the user would; windows follow at once.
/// - `restore`: puts back the appearance the user had before the script changed it. End every script that changes it with this.
/// Window titles may be `@settings` for the Settings window, or `@sheet` for the sheet showing on any window.
/// - `min [window title]`: shrinks the window to the smallest size its content allows (a sheet that sets none keeps the size
///   its content asked for). Answers `WxH`.
/// - `size <width> <height> [window title]`: resizes the window's content to that size.
/// - `panes [window title]`: the widths of the window's widest side-by-side split view's panes, as `180,320,260`.
/// - `snapshot <name> [window title]`: writes `<name>.png` of that window, as the harness's `snapshot` does.
/// - `check [window title]`: writes `layout-<title>.txt` and answers `FAIL ...` when a control or scroll view reaches past
///   the window's edge, when two sibling controls overlap, or when a one-line label is drawn narrower than its text.
///   Content scrolled out of a scroll view, and controls drawn inside each other, are fine.
enum StudioLayoutAudit {
    /// The user's own appearance setting (`.some(nil)` when they never chose one), while a script has changed it.
    nonisolated(unsafe) private static var savedAppearance: String??

    static func installHarnessHook() {
        guard UITestHarness.isActive, UITestHarness.hooks["studio.layout"] == nil else { return }
        UITestHarness.hooks["studio.layout"] = { args in
            switch args.first ?? "" {
            case "appearance":
                let value = args.count > 1 ? args[1] : StudioAppearance.system.rawValue
                guard let appearance = StudioAppearance(rawValue: value) else { return "FAIL unknown appearance \(value)" }
                // The harness shares the user's defaults; `restore` puts their own choice back.
                if savedAppearance == nil {
                    savedAppearance = .some(UserDefaults.standard.string(forKey: StudioAppearance.storageKey))
                }
                UserDefaults.standard.set(appearance.rawValue, forKey: StudioAppearance.storageKey)
                try? await Task.sleep(for: .milliseconds(300))
                return appearance.rawValue
            case "restore":
                guard let saved = savedAppearance else { return "unchanged" }
                UserDefaults.standard.set(saved, forKey: StudioAppearance.storageKey)
                savedAppearance = nil
                return saved ?? "default"
            case "min":
                guard let window = window(titled: args.count > 1 ? args[1...].joined(separator: " ") : nil) else { return "FAIL no window" }
                return await resize(window, to: .zero)
            case "size":
                guard args.count >= 3, let w = Double(args[1]), let h = Double(args[2]) else { return "FAIL size <width> <height>" }
                guard let window = window(titled: args.count > 3 ? args[3...].joined(separator: " ") : nil) else { return "FAIL no window" }
                return await resize(window, to: NSSize(width: w, height: h))
            case "panes":
                guard let window = window(titled: args.count > 1 ? args[1...].joined(separator: " ") : nil) else { return "FAIL no window" }
                return panes(window)
            case "snapshot":
                guard args.count >= 2 else { return "FAIL snapshot <name> [window title]" }
                guard let window = window(titled: args.count > 2 ? args[2...].joined(separator: " ") : nil) else { return "FAIL no window" }
                return snapshot(window, name: args[1])
            case "check":
                guard let window = window(titled: args.count > 1 ? args[1...].joined(separator: " ") : nil) else { return "FAIL no window" }
                return check(window)
            default:
                return "FAIL unknown layout step"
            }
        }
    }

    /// `@settings` names the Settings window, whose title is whichever tab was last open; `@sheet` the frontmost sheet, which
    /// has no title of its own.
    static func window(titled title: String?) -> NSWindow? {
        if title == "@settings" {
            return NSApp.windows.first { $0.isVisible && ($0.identifier?.rawValue.contains("Settings") ?? false) }
        }
        if title == "@sheet" {
            var sheet = NSApp.orderedWindows.lazy.compactMap(\.attachedSheet).first
            while let inner = sheet?.attachedSheet { sheet = inner }
            return sheet
        }
        if let title { return NSApp.windows.first { $0.title == title && $0.isVisible } }
        var window = NSApp.keyWindow ?? NSApp.mainWindow ?? NSApp.orderedWindows.first { $0.isVisible && $0.canBecomeMain }
        while let sheet = window?.attachedSheet { window = sheet }
        return window
    }

    /// Asks for `size` (`.zero` for the smallest); the window clamps it to its content's minimum.
    @MainActor
    static func resize(_ window: NSWindow, to size: NSSize) async -> String {
        // SwiftUI publishes the content's minimum as the window's minimum size once it has laid out.
        window.contentView?.layoutSubtreeIfNeeded()
        var minimum = window.contentMinSize
        // A SwiftUI sheet publishes no minimum: it's the size its content asked for, and a person can't make it smaller.
        if window.sheetParent != nil, minimum.width < 2 || minimum.height < 2 {
            minimum = window.contentRect(forFrameRect: window.frame).size
        }
        let wanted = NSSize(width: max(size.width, minimum.width, 1), height: max(size.height, minimum.height, 1))
        var frame = window.frameRect(forContentRect: NSRect(origin: .zero, size: wanted))
        frame.origin = NSPoint(x: window.frame.minX, y: window.frame.maxY - frame.height)
        window.setFrame(frame, display: true)
        try? await Task.sleep(for: .milliseconds(400))
        window.contentView?.layoutSubtreeIfNeeded()
        let content = window.contentRect(forFrameRect: window.frame).size
        return "\(Int(content.width.rounded()))x\(Int(content.height.rounded()))"
    }

    /// The widths of the widest side-by-side split view's panes (0 for a hidden one), left to right: `180,320,260`.
    @MainActor
    static func panes(_ window: NSWindow) -> String {
        var splits: [NSSplitView] = []
        func walk(_ view: NSView) {
            if let split = view as? NSSplitView, split.isVertical { splits.append(split) }
            view.subviews.forEach(walk)
        }
        if let content = window.contentView { walk(content) }
        guard let split = splits.max(by: { $0.bounds.width < $1.bounds.width }) else { return "FAIL no split view" }
        return split.arrangedSubviews.map { $0.isHidden ? "0" : String(Int($0.frame.width.rounded())) }.joined(separator: ",")
    }

    /// As the harness's own `snapshot`, for a window it can't target by title (Settings).
    @MainActor
    static func snapshot(_ window: NSWindow, name: String) -> String {
        guard let view = window.contentView?.superview ?? window.contentView else { return "FAIL no view" }
        view.layoutSubtreeIfNeeded()
        guard let rep = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { return "FAIL no bitmap" }
        view.cacheDisplay(in: view.bounds, to: rep)
        let out = ProcessInfo.processInfo.environment["VIXEN_UI_TEST_OUT"] ?? NSTemporaryDirectory()
        guard let png = rep.representation(using: .png, properties: [:]),
              (try? png.write(to: URL(fileURLWithPath: out).appending(path: "\(name).png"))) != nil else { return "FAIL not written" }
        return "\(rep.pixelsWide)x\(rep.pixelsHigh) \(window.title)"
    }

    struct Node {
        var role: String
        var title: String
        var frame: NSRect
        var depth: Int
        var path: String
    }

    /// Roles whose frames are the controls a person reads or clicks; groups, rows and cells only arrange them.
    static let controlRoles: Set<String> = [
        "AXButton", "AXCheckBox", "AXRadioButton", "AXPopUpButton", "AXMenuButton", "AXTextField", "AXStaticText",
        "AXSlider", "AXIncrementor", "AXComboBox", "AXSearchField", "AXSegmentedControl", "AXDisclosureTriangle",
        "AXColorWell", "AXImage", "AXLink", "AXDateTimeArea", "AXProgressIndicator", "AXToggle", "AXSwitch",
    ]
    /// Containers that show only part of their content: what lies wholly outside them is scrolled out, not clipped.
    static let scrollingRoles: Set<String> = ["AXScrollArea", "AXTable", "AXOutline", "AXList", "AXBrowser", "AXWebArea"]

    @MainActor
    static func check(_ window: NSWindow) -> String {
        // Screen coordinates, as accessibility reports them; the title bar and toolbar belong to the window too.
        let bounds = window.frame.insetBy(dx: -1, dy: -1)
        var problems: [String] = []
        var controls = 0
        var lines: [String] = ["Window: \(window.title) \(Int(window.frame.width))x\(Int(window.frame.height))"]

        func frame(_ element: Any) -> NSRect? {
            guard let e = element as? NSAccessibilityProtocol else { return nil }
            let f = e.accessibilityFrame()
            return f.width > 0.5 && f.height > 0.5 ? f : nil
        }
        func label(_ n: Node) -> String {
            "\(n.role) \"\(n.title.prefix(40))\" @\(Int(n.frame.minX - window.frame.minX)),\(Int(window.frame.maxY - n.frame.maxY)) \(Int(n.frame.width))x\(Int(n.frame.height))"
        }

        /// `scroll` is the visible part of the innermost scroll view, or nil outside any.
        func walk(_ element: Any, depth: Int, scroll: NSRect?) {
            guard depth < 60 else { return }
            let d = describe(element)
            var innerScroll = scroll
            if depth > 0, let f = frame(element) {
                let node = Node(role: d.role, title: d.title, frame: f, depth: depth, path: "")
                let isControl = controlRoles.contains(d.role), isScroll = scrollingRoles.contains(d.role)
                if isControl || isScroll {
                    if isControl { controls += 1 }
                    lines.append(String(repeating: "  ", count: depth) + label(node))
                    // Inside a scroll view, reaching past its visible edge is how scrolling looks; elsewhere, past the
                    // window's edge means the window is too small for it.
                    let edge = scroll ?? bounds
                    if scroll == nil || !bounds.contains(edge) {
                        if !f.intersects(bounds) {
                            problems.append("outside window: \(label(node))")
                        } else if !bounds.contains(f) {
                            problems.append("clipped: \(label(node))")
                        }
                    }
                    if d.role == "AXStaticText", let missing = truncation(d.title, frame: f),
                       f.intersection(edge).width >= f.width - 2 {
                        problems.append("truncated (\(missing)): \(label(node))")
                    }
                }
                if isScroll { innerScroll = f.intersection(scroll ?? bounds).insetBy(dx: -1, dy: -1) }
            }
            let kids = children(element)
            var siblings: [Node] = []
            for c in kids {
                let cd = describe(c)
                if controlRoles.contains(cd.role), let cf = frame(c), cf.intersects(innerScroll ?? bounds) {
                    siblings.append(Node(role: cd.role, title: cd.title, frame: cf, depth: depth + 1, path: ""))
                }
            }
            for a in siblings.indices {
                for b in siblings.indices where b > a {
                    let x = siblings[a].frame.intersection(siblings[b].frame)
                    guard !x.isNull, x.width > 2, x.height > 2 else { continue }
                    // One drawn inside the other (an icon in its button, a label in its field) isn't an overlap.
                    if siblings[a].frame.insetBy(dx: -1, dy: -1).contains(siblings[b].frame)
                        || siblings[b].frame.insetBy(dx: -1, dy: -1).contains(siblings[a].frame) { continue }
                    problems.append("overlap: \(label(siblings[a])) and \(label(siblings[b]))")
                }
            }
            for c in kids { walk(c, depth: depth + 1, scroll: innerScroll) }
        }
        walk(window, depth: 0, scroll: nil)

        var seen = Set<String>()
        let unique = problems.filter { seen.insert($0).inserted }
        lines.append("")
        lines.append(unique.isEmpty ? "No problems" : unique.joined(separator: "\n"))
        let out = ProcessInfo.processInfo.environment["VIXEN_UI_TEST_OUT"] ?? NSTemporaryDirectory()
        let safe = window.title.replacingOccurrences(of: "/", with: "-")
        try? lines.joined(separator: "\n").write(to: URL(fileURLWithPath: out).appending(path: "layout-\(safe).txt"),
                                                 atomically: true, encoding: .utf8)
        return unique.isEmpty ? "ok \(controls) controls" : "FAIL \(unique.count): " + unique.prefix(6).joined(separator: " | ")
    }

    static func children(_ element: Any) -> [Any] {
        (element as? NSAccessibilityProtocol)?.accessibilityChildren() ?? []
    }

    /// Role and visible text. SwiftUI can hand back an attributed string where AppKit declares a plain one, so the raw
    /// object is read first (as the harness's own `dump` does).
    static func describe(_ element: Any) -> (role: String, title: String) {
        guard let e = element as? NSAccessibilityProtocol else { return ("", "") }
        let role = e.accessibilityRole()?.rawValue ?? ""
        func text(_ name: String) -> String {
            guard let object = element as? NSObject else { return "" }
            let selector = NSSelectorFromString(name)
            guard object.responds(to: selector), let value = object.perform(selector)?.takeUnretainedValue() else { return "" }
            if let attributed = value as? NSAttributedString { return attributed.string }
            return value as? String ?? ""
        }
        var title = text("accessibilityTitle")
        if title.isEmpty { title = text("accessibilityLabel") }
        if title.isEmpty, role == "AXStaticText" { title = text("accessibilityValue") }
        return (role, title)
    }

    /// A one-line label drawn narrower than its text at the size its height implies. Answers how much is missing.
    static func truncation(_ text: String, frame: NSRect) -> String? {
        let text = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard text.count > 1, !text.contains("\n") else { return nil }
        // A line of system text is about 1.2× its point size tall; one-line labels only.
        let pointSize = (frame.height / 1.2).rounded()
        guard (9...20).contains(pointSize) else { return nil }
        let width = (text as NSString).size(withAttributes: [.font: NSFont.systemFont(ofSize: pointSize)]).width
        // Leave room for font differences (bold, monospaced digits) before calling it cut off.
        return width > frame.width * 1.18 + 6 ? "\(Int(width))>\(Int(frame.width))" : nil
    }
}
