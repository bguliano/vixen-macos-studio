import AppKit
import Darwin
import SwiftUI
import Vision

/// Lets the app drive itself from a JSON script, so windows can be exercised and photographed without
/// controlling the screen. See docs/UI_GUIDE.md. Active only when `VIXEN_UI_TEST` is set.
public enum UITestHarness {
    public static var isActive: Bool { ProcessInfo.processInfo.environment["VIXEN_UI_TEST"] != nil }

    /// Scenes register here so scripts can open them by id.
    public static var openWindow: ((String) -> Void)?

    /// What a window offers scripts beyond clicks, by name, for what events can't reach or to measure something: `{ "action": "hook", "name": "...", "path": ["arguments"] }`.
    /// The answer goes into `result.json` as the step's detail. It fails the step if it starts with "FAIL".
    public static var hooks: [String: ([String]) async -> String] = [:]

    /// Steps an area adds for its own tests, by action name. Runs like any other step, and fails the run the same way.
    public static var customActions: [String: (UITestStep) async -> (Bool, String?)] = [:]

    /// Says which windows stay where they are rather than going off screen, for a test that needs one really showing: a Metal layer
    /// draws nothing while its window is hidden.
    public static var staysOnScreen: ((NSWindow) -> Bool)?
    // STUDIO-BEGIN harness-exit-cleanup: `quit` exits at once, so whatever must not outlive the run (the MCP socket) stops here first
    /// Runs just before a script's `quit` exits. Keep it quick and silent: no saves, no controller stops.
    public static var beforeExit: (() -> Void)?
    // STUDIO-END harness-exit-cleanup

    /// Writes a line to `notes.txt` in the output folder, for what a test shouldn't really do, like opening a browser.
    public static func note(_ line: String) {
        let out = ProcessInfo.processInfo.environment["VIXEN_UI_TEST_OUT"] ?? NSTemporaryDirectory()
        let record = URL(fileURLWithPath: out).appending(path: "notes.txt")
        let previous = (try? String(contentsOf: record, encoding: .utf8)) ?? ""
        try? (previous + line + "\n").write(to: record, atomically: true, encoding: .utf8)
    }

    /// Call once from the application delegate.
    public static func startIfRequested() {
        guard let path = ProcessInfo.processInfo.environment["VIXEN_UI_TEST"] else { return }
        // No Dock icon and no focus changes while a script runs.
        NSApp.setActivationPolicy(.accessory)
        enableAccessibilityTree()
        Task { @MainActor in
            await Runner(scriptPath: path).run()
        }
    }

    /// Keeps a test window out of the way. The window still draws, so snapshots work.
    /// SwiftUI builds its accessibility tree only once an assistive client is attached. Asking our own
    /// process for its tree through the Accessibility API counts as one.
    static func enableAccessibilityTree() {
        let app = AXUIElementCreateApplication(getpid())
        AXUIElementSetAttributeValue(app, "AXManualAccessibility" as CFString, kCFBooleanTrue)
        AXUIElementSetAttributeValue(app, "AXEnhancedUserInterface" as CFString, kCFBooleanTrue)
        var value: AnyObject?
        AXUIElementCopyAttributeValue(app, kAXWindowsAttribute as CFString, &value)
    }

    static func moveOffscreen(_ window: NSWindow) {
        guard window.frame.origin.x > -5000 else { return }
        // STUDIO: VIXEN_UI_TEST_ONSCREEN keeps windows visible, so scripts/studio_capture.sh can capture real rendering (Liquid Glass)
        if ProcessInfo.processInfo.environment["VIXEN_UI_TEST_ONSCREEN"] != nil { return }
        // Not remembered: the next real launch shares these defaults, and would open the window off screen.
        window.setFrameAutosaveName("")
        window.setFrameOrigin(NSPoint(x: -12000, y: -12000))
    }
}

/// What a step of an area's own says. See `UITestHarness.customActions`.
public struct UITestStep: Sendable {
    public var id: String?
    public var text: String?
    public var name: String?
    public var label: String?
    public var seconds: Double?
    public var x: Double?
    public var y: Double?
    public var toX: Double?
    public var toY: Double?
    public var index: Int?
}

// Read-only synchronization with the physical driver's exact private markers.
// This action is reachable only in VIXEN_UI_TEST scripts. It never creates a
// marker or performs input, and a failure stops the script before later Save.
enum UITestHelperCompletion {
    enum Sample {
        case missing
        case bytes(Data)
        case invalid
    }

    static func sample(_ url: URL, maximum: Int) -> Sample {
        let descriptor = open(url.path, O_RDONLY | O_CLOEXEC | O_NOFOLLOW | O_NONBLOCK)
        guard descriptor >= 0 else { return errno == ENOENT ? .missing : .invalid }
        defer { close(descriptor) }
        var metadata = stat()
        guard fstat(descriptor, &metadata) == 0,
              metadata.st_mode & S_IFMT == S_IFREG, metadata.st_uid == getuid(),
              metadata.st_size >= 0, metadata.st_size <= maximum else { return .invalid }
        var buffer = [UInt8](repeating: 0, count: maximum + 1)
        let count = read(descriptor, &buffer, buffer.count)
        guard count >= 0, count == metadata.st_size else { return .invalid }
        return .bytes(Data(buffer.prefix(count)))
    }

    static func wait(name: String?, seconds: Double?, directory: URL, script: URL) async -> (Bool, String?) {
        guard let name, ["drop-complete", "cancel-complete"].contains(name),
              let seconds, seconds.isFinite, seconds > 0, seconds <= 18 else {
            return (false, "invalid completion marker or timeout")
        }
        let root = directory.standardizedFileURL
        guard root.lastPathComponent == "ui",
              root.deletingLastPathComponent() == script.standardizedFileURL.deletingLastPathComponent(),
              root.resolvingSymlinksInPath() == root else {
            return (false, "completion marker directory is not the private script directory")
        }
        let parent = root.deletingLastPathComponent()
        let deadline = ProcessInfo.processInfo.systemUptime + seconds
        func stopReason() -> String? {
            if Task.isCancelled { return "completion wait cancelled" }
            switch sample(parent.appendingPathComponent("abort-request.json"), maximum: 4096) {
            case .missing: break
            case .bytes, .invalid: return "completion wait aborted"
            }
            // The helper publishes this only when it has stopped. While this
            // app is waiting, any such report is terminal, even beside a valid
            // marker or when its JSON claims success (success requires exit).
            switch sample(parent.appendingPathComponent("foreground.json"), maximum: 1_000_000) {
            case .missing: break
            case .bytes, .invalid: return "helper stopped before completion marker: " + name
            }
            return nil
        }
        while ProcessInfo.processInfo.systemUptime < deadline {
            if let reason = stopReason() { return (false, reason) }
            switch sample(parent.appendingPathComponent(name), maximum: 8) {
            case .bytes(let data):
                guard data == Data("complete".utf8) else { return (false, "invalid completion marker contents: " + name) }
                if let reason = stopReason() { return (false, reason) }
                guard ProcessInfo.processInfo.systemUptime < deadline else { return (false, "completion marker arrived after timeout: " + name) }
                return (true, "complete " + name)
            case .invalid: return (false, "invalid completion marker file: " + name)
            case .missing: break
            }
            do { try await Task.sleep(for: .milliseconds(50)) }
            catch { return (false, "completion wait cancelled") }
        }
        return (false, "completion marker timeout: " + name)
    }
}

private struct Step: Decodable {
    var action: String
    var id: String?
    var seconds: Double?
    var label: String?
    var role: String?
    /// `click`: target the actual disclosure arrow immediately left of this label.
    var disclosure: Bool?
    var index: Int?
    var path: [String]?
    var name: String?
    var text: String?
    var key: String?
    var x: Double?
    var y: Double?
    var toX: Double?
    var toY: Double?
    var modifiers: [String]?
    var window: String?
    /// For `mouse`: "right", "middle" or "none" (just moves). Left when absent.
    var button: String?
    /// For `mouse`: 2 double-clicks.
    var clicks: Int?
    /// For `mouse`: how many drag events between the ends. 8 when absent.
    var moves: Int?
    /// For `mouse`: don't release, or don't press, so a script can stop in the middle of a drag and photograph it.
    var hold: Bool?
    var resume: Bool?
    /// For `scroll`: deltas, in points when `precise`, otherwise lines.
    var dx: Double?
    var dy: Double?
    var precise: Bool?
}

private struct Script: Decodable {
    var steps: [Step]
}

private struct StepResult: Encodable {
    var step: Int
    var action: String
    var ok: Bool
    var detail: String?
    var startedUptime: Double? = nil
    var finishedUptime: Double? = nil
    var hiddenUptime: Double? = nil
}

@MainActor
private final class Runner {
    let scriptPath: String
    let outDir: URL
    var results: [StepResult] = []
    var targetWindowTitle: String?

    init(scriptPath: String) {
        self.scriptPath = scriptPath
        let out = ProcessInfo.processInfo.environment["VIXEN_UI_TEST_OUT"] ?? NSTemporaryDirectory()
        outDir = URL(fileURLWithPath: out)
        try? FileManager.default.createDirectory(at: outDir, withIntermediateDirectories: true)
    }

    func run() async {
        // Let the first scene appear.
        try? await Task.sleep(for: .milliseconds(400))
        hideWindows()
        do {
            let data = try Data(contentsOf: URL(fileURLWithPath: scriptPath))
            let script = try JSONDecoder().decode(Script.self, from: data)
            for (i, step) in script.steps.enumerated() {
                let trace = ProcessInfo.processInfo.environment["VIXEN_UI_TEST_TRACE"] == "1"
                let started = ProcessInfo.processInfo.systemUptime
                if trace { FileHandle.standardError.write(Data("UI test step \(i) \(step.action) starting uptime=\(started)\n".utf8)) }
                let (ok, detail) = await perform(step)
                let finished = ProcessInfo.processInfo.systemUptime
                if trace { FileHandle.standardError.write(Data("UI test step \(i) \(step.action) finished ok=\(ok) uptime=\(finished) duration=\(finished-started)\n".utf8)) }
                hideWindows()
                let hidden = ProcessInfo.processInfo.systemUptime
                if trace { FileHandle.standardError.write(Data("UI test step \(i) windows hidden uptime=\(hidden) duration=\(hidden-finished)\n".utf8)) }
                results.append(StepResult(step: i, action: step.action, ok: ok, detail: detail, startedUptime: started, finishedUptime: finished, hiddenUptime: hidden))
                if !ok {
                    FileHandle.standardError.write(Data("UI test step \(i) (\(step.action)) failed: \(detail ?? "")\n".utf8))
                    // Never continue to Save/Cancel after a failed physical-driver handshake.
                    if step.action == "waitForFile" { break }
                }
            }
        } catch {
            results.append(StepResult(step: -1, action: "load", ok: false, detail: "\(error)"))
        }
        finish()
    }

    func finish() {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted]
        if let data = try? encoder.encode(results) {
            try? data.write(to: outDir.appending(path: "result.json"))
        }
        let failed = results.filter { !$0.ok }.count
        print("UI test finished: \(results.count) steps, \(failed) failed")
        UITestHarness.beforeExit?() // STUDIO: release the MCP socket before exiting
        exit(failed == 0 ? 0 : 1)
    }

    func hideWindows() {
        for w in NSApp.windows where w.isVisible && w.level != .popUpMenu && !(w is NSPanel) && UITestHarness.staysOnScreen?(w) != true {
            UITestHarness.moveOffscreen(w)
        }
    }

    /// The window a step acts on: a sheet if one is up, otherwise the requested or frontmost window.
    func currentWindow() -> NSWindow? {
        var candidate: NSWindow?
        if let title = targetWindowTitle {
            candidate = NSApp.windows.first { $0.title == title && $0.isVisible }
        }
        candidate = candidate ?? NSApp.keyWindow ?? NSApp.mainWindow
            ?? NSApp.orderedWindows.first { $0.isVisible && $0.contentView != nil }
        // An alert's glass background is a child window of the alert, and frontmost.
        while targetWindowTitle == nil, let parent = candidate?.parent {
            candidate = parent
        }
        while let sheet = candidate?.attachedSheet {
            candidate = sheet
        }
        return candidate
    }

    func perform(_ s: Step) async -> (Bool, String?) {
        switch s.action {
        case "waitForFile":
            return await UITestHelperCompletion.wait(name: s.name, seconds: s.seconds, directory: outDir, script: URL(fileURLWithPath: scriptPath))
        case "wait":
            try? await Task.sleep(for: .seconds(s.seconds ?? 0.25))
            return (true, nil)
        case "window":
            guard let id = s.id else { return (false, "needs id") }
            guard let open = UITestHarness.openWindow else { return (false, "no window opener registered") }
            open(id)
            try? await Task.sleep(for: .milliseconds(400))
            return (true, nil)
        case "focus":
            targetWindowTitle = s.window
            let w = currentWindow()
            w?.makeKeyAndOrderFront(nil)
            return (w != nil, w == nil ? "no such window" : nil)
        case "windows":
            let names = NSApp.windows.map {
                "\($0.title) \(Int($0.frame.width))x\(Int($0.frame.height))" + ($0.isVisible ? "" : " (hidden)")
            }
            return (true, names.joined(separator: " | "))
        case "click":
            return await click(s)
        case "selectRow":
            return selectRow(s)
        case "popup":
            return await choosePopup(s)
        case "editCell":
            return editCell(s)
        case "menu":
            return await performMenu(s.path ?? [])
        case "closeWindow":
            guard let window = currentWindow() else { return (false, "no window") }
            await outsideDispatch { window.performClose(nil) }
            return (true, nil)
        case "snapshot":
            return snapshot(name: s.name ?? "snapshot")
        case "dump":
            return dump(name: s.name ?? "dump")
        case "menus":
            return dumpMenus(name: s.name ?? "menus")
        case "type":
            return typeText(s.text ?? "")
        case "key":
            return pressKey(s.key ?? "", modifiers: s.modifiers ?? [])
        case "mouse":
            return mouse(s)
        case "scroll":
            return scroll(s)
        case "hook":
            guard let name = s.name, let hook = UITestHarness.hooks[name] else { return (false, "no hook \(s.name ?? "")") }
            let answer = await hook(s.path ?? [])
            return (!answer.hasPrefix("FAIL"), answer)
        case "quit":
            finish()
            return (true, nil)
        default:
            if let custom = UITestHarness.customActions[s.action] {
                return await custom(UITestStep(id: s.id, text: s.text, name: s.name, label: s.label, seconds: s.seconds, x: s.x, y: s.y, toX: s.toX, toY: s.toY, index: s.index))
            }
            return (false, "unknown action \(s.action)")
        }
    }

    // MARK: Accessibility traversal

    func children(of element: Any) -> [Any] {
        (element as? NSAccessibilityProtocol)?.accessibilityChildren() ?? []
    }

    func describe(_ element: Any) -> (role: String, title: String) {
        guard let e = element as? NSAccessibilityProtocol else { return ("", "") }
        let role = e.accessibilityRole()?.rawValue ?? ""
        // Some SwiftUI AX objects return NSAttributedString from title/label
        // despite the protocol's NSString declaration. The typed Swift getter
        // then unconditionally bridges it and throws an Objective-C exception,
        // abandoning the harness Task. Read the raw object before converting.
        func text(_ name: String) -> String {
            guard let object = element as? NSObject else { return "" }
            let selector = NSSelectorFromString(name)
            guard object.responds(to: selector), let value = object.perform(selector)?.takeUnretainedValue() else { return "" }
            if let attributed = value as? NSAttributedString { return attributed.string }
            return value as? String ?? ""
        }
        var title = text("accessibilityTitle")
        if title.isEmpty { title = text("accessibilityLabel") }
        if title.isEmpty { title = text("accessibilityValue") }
        return (role, title)
    }

    func find(label: String, role: String?, in root: Any, depth: Int = 0, skip: inout Int) -> Any? {
        guard depth < 60 else { return nil }
        let d = describe(root)
        if d.title == label, role == nil || d.role == role {
            if skip == 0 { return root }
            skip -= 1
        }
        for c in children(of: root) {
            if let hit = find(label: label, role: role, in: c, depth: depth + 1, skip: &skip) { return hit }
        }
        return nil
    }

    func click(_ s: Step) async -> (Bool, String?) {
        guard let label = s.label else { return (false, "needs label") }
        guard let window = currentWindow() else { return (false, "no window") }
        if s.disclosure == true {
            guard s.x == nil, s.y == nil, s.role == nil else {
                return (false, "disclosure click uses a label, without coordinates or AX role")
            }
            guard let point = locateText(label, in: window, occurrence: s.index ?? 0, disclosure: true) else {
                return (false, "no disclosure label \"\(label)\"")
            }
            clickAt(point, in: window, modifiers: [])
            try? await Task.sleep(for: .milliseconds(300))
            return (true, "clicked disclosure at \(Int(point.x)),\(Int(point.y))")
        }
        var skip = s.index ?? 0
        let roots: [Any] = [window]
        for r in roots {
            if let hit = find(label: label, role: s.role, in: r, skip: &skip) {
                let performed = (hit as? NSAccessibilityProtocol)?.accessibilityPerformPress() ?? false
                if !performed, let control = (hit as? NSControl) ?? ((hit as? NSCell)?.controlView as? NSControl) {
                    // An alert's button can report false after it has worked, and then has no window.
                    if control.window?.isVisible == true {
                        control.performClick(nil)
                    }
                    try? await Task.sleep(for: .milliseconds(120))
                    return (true, nil)
                }
                try? await Task.sleep(for: .milliseconds(120))
                return (performed, performed ? nil : "element would not accept a press")
            }
        }
        // SwiftUI content isn't reachable through the in-process accessibility tree, so find the visible
        // text in the window's own picture and click there.
        guard let point = locateText(label, in: window, occurrence: s.index ?? 0) else {
            return (false, "no element labelled \"\(label)\"")
        }
        clickAt(point, in: window, modifiers: [])
        try? await Task.sleep(for: .milliseconds(150))
        return (true, "clicked visible text at \(Int(point.x)),\(Int(point.y))")
    }

    /// Selects the table or list row showing some text. A click can't do it: a window that isn't key ignores a first click on a table, and a test
    /// window never is. `index` picks among rows showing the same text.
    func selectRow(_ s: Step) -> (Bool, String?) {
        guard let label = s.label else { return (false, "needs label") }
        guard let window = currentWindow() else { return (false, "no window") }
        guard let root = window.contentView?.superview ?? window.contentView else { return (false, "no view") }
        guard let point = locateText(label, in: window, occurrence: s.index ?? 0) else {
            return (false, "no text \"\(label)\"")
        }
        var view = root.hitTest(root.convert(point, from: nil))
        while let current = view, !(current is NSTableView) {
            view = current.superview
        }
        guard let table = view as? NSTableView else { return (false, "that text isn't in a table") }
        let row = table.row(at: table.convert(point, from: nil))
        guard row >= 0 else { return (false, "no row there") }
        table.selectRowIndexes(IndexSet(integer: row), byExtendingSelection: s.modifiers?.contains("cmd") == true)
        return (true, "row \(row)")
    }

    /// Starts editing the table cell at window coordinates (origin at the content view's top left), as a click on a selected row would. A window
    /// that isn't key, which a test window never is, ignores that click.
    func editCell(_ s: Step) -> (Bool, String?) {
        guard let window = currentWindow(), let content = window.contentView else { return (false, "no window") }
        let point = NSPoint(x: s.x ?? 0, y: content.bounds.height - (s.y ?? 0))
        let root = content.superview ?? content
        var view = root.hitTest(root.convert(point, from: nil))
        while let current = view, !(current is NSTableView) {
            view = current.superview
        }
        guard let table = view as? NSTableView else { return (false, "no table there") }
        let inTable = table.convert(point, from: nil)
        let (row, column) = (table.row(at: inTable), table.column(at: inTable))
        guard row >= 0, column >= 0 else { return (false, "no cell there") }
        window.makeKey()
        table.selectRowIndexes(IndexSet(integer: row), byExtendingSelection: false)
        table.editColumn(column, row: row, with: nil, select: true)
        return (true, nil)
    }

    /// Clicks a pop-up or menu button at window coordinates (origin at the content view's top left), and chooses by title in the menu it opens.
    /// SwiftUI draws these itself, and a menu's tracking takes no events from here, so the item's action runs directly.
    func choosePopup(_ s: Step) async -> (Bool, String?) {
        guard let label = s.label else { return (false, "needs label") }
        guard let window = currentWindow(), let content = window.contentView else { return (false, "no window") }
        nonisolated final class Opened: @unchecked Sendable {
            var menu: NSMenu?
        }
        let opened = Opened()
        let observer = NotificationCenter.default.addObserver(forName: NSMenu.didBeginTrackingNotification, object: nil, queue: nil) { note in
            opened.menu = note.object as? NSMenu
        }
        defer { NotificationCenter.default.removeObserver(observer) }
        if s.button == "right" {
            _ = mouse(s)
        } else if let name = s.name {
            guard let point = locateText(name, in: window, occurrence: s.index ?? 0) else {
                return (false, "no popup showing \"\(name)\"")
            }
            clickAt(point, in: window, modifiers: [])
        } else {
            clickAt(NSPoint(x: s.x ?? 0, y: content.bounds.height - (s.y ?? 0)), in: window, modifiers: [])
        }
        for _ in 0..<60 where opened.menu == nil {
            try? await Task.sleep(for: .milliseconds(50))
        }
        guard let menu = opened.menu else { return (false, "no menu opened") }
        // `path` walks submenus first: `"path": ["Type"], "label": "Phrase"`.
        var target = menu
        for title in s.path ?? [] {
            guard let submenu = target.item(withTitle: title)?.submenu else {
                menu.cancelTrackingWithoutAnimation()
                return (false, "no submenu \"\(title)\" in \(target.items.map(\.title))")
            }
            target = submenu
        }
        let index = target.indexOfItem(withTitle: label)
        menu.cancelTrackingWithoutAnimation()
        try? await Task.sleep(for: .milliseconds(150))
        guard index >= 0 else { return (false, "no item \"\(label)\" in \(target.items.map(\.title))") }
        target.performActionForItem(at: index)
        return (true, nil)
    }

    /// Finds text in the window with Vision. Returns its centre in the frame view's coordinates (origin bottom left).
    func locateText(_ text: String, in window: NSWindow, occurrence: Int, disclosure: Bool = false) -> NSPoint? {
        guard let view = window.contentView?.superview ?? window.contentView else { return nil }
        guard let rep = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { return nil }
        view.cacheDisplay(in: view.bounds, to: rep)
        guard let image = rep.cgImage else { return nil }
        let request = VNRecognizeTextRequest()
        request.recognitionLevel = .accurate
        request.usesLanguageCorrection = false
        try? VNImageRequestHandler(cgImage: image, options: [:]).perform([request])
        let wanted = normalize(text)
        var hits: [NSPoint] = []
        for obs in request.results ?? [] {
            guard let candidate = obs.topCandidates(1).first else { continue }
            if normalize(candidate.string) == wanted || normalize(candidate.string).contains(wanted) {
                // Vision's boxes are normalised with the origin at the bottom left, like the view.
                if disclosure {
                    // The observation can include the recognized arrow or other
                    // text. Use just this label's glyphs, then its leading arrow.
                    guard let range = candidate.string.range(of: text, options: .caseInsensitive),
                          let glyphs = try? candidate.boundingBox(for: range) else { continue }
                    let b = glyphs.boundingBox
                    let point = NSPoint(x: b.minX * view.bounds.width - 8, y: b.midY * view.bounds.height)
                    if view.bounds.contains(point) { hits.append(point) }
                } else {
                    let b = obs.boundingBox
                    hits.append(NSPoint(x: b.midX * view.bounds.width, y: b.midY * view.bounds.height))
                }
            }
        }
        // Top of the window first, then left to right: reading order.
        hits.sort { abs($0.y - $1.y) > 4 ? $0.y > $1.y : $0.x < $1.x }
        return occurrence < hits.count ? hits[occurrence] : nil
    }

    func normalize(_ s: String) -> String {
        s.lowercased().replacing("…", with: "...").filter { !$0.isWhitespace }
    }

    func clickAt(_ point: NSPoint, in window: NSWindow, modifiers: NSEvent.ModifierFlags) {
        window.makeKey()
        for type in [NSEvent.EventType.leftMouseDown, .leftMouseUp] {
            if let e = NSEvent.mouseEvent(
                with: type, location: point, modifierFlags: modifiers, timestamp: ProcessInfo.processInfo.systemUptime,
                windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: type == .leftMouseDown ? 1 : 0
            ) {
                // Posted, not sent: a control's mouseDown runs its own tracking loop, which needs the
                // mouse-up already waiting in the queue, or it would never return.
                NSApp.postEvent(e, atStart: false)
            }
        }
    }

    /// Runs an action the way a real event does, from the run loop and not inside a dispatched block. A nested run loop, such as quitting's, can't
    /// drain the main queue from inside one of its blocks, and everything else on the main actor would wait forever.
    func outsideDispatch(_ action: @escaping () -> Void) async {
        await withCheckedContinuation { (finished: CheckedContinuation<Void, Never>) in
            RunLoop.main.perform {
                // First: the script carries on inside a nested run loop, which is how it answers a prompt the action puts up.
                finished.resume()
                action()
            }
        }
    }

    func performMenu(_ path: [String]) async -> (Bool, String?) {
        guard !path.isEmpty, var menu = NSApp.mainMenu else { return (false, "no menu") }
        for (i, title) in path.enumerated() {
            // SwiftUI brings a menu's items up to date when it's about to open.
            menu.delegate?.menuNeedsUpdate?(menu)
            guard let item = menu.items.first(where: { $0.title == title || $0.title.replacing("…", with: "...") == title }) else {
                return (false, "no menu item \(title)")
            }
            if i == path.count - 1 {
                menu.update()
                guard item.isEnabled else { return (false, "\(title) is disabled") }
                let (owner, index) = (menu, menu.index(of: item))
                await outsideDispatch { owner.performActionForItem(at: index) }
                return (true, nil)
            }
            guard let sub = item.submenu else { return (false, "\(title) has no submenu") }
            sub.update()
            menu = sub
        }
        return (false, "empty path")
    }

    // MARK: Output

    func snapshot(name: String) -> (Bool, String?) {
        guard let window = currentWindow(), let view = window.contentView else { return (false, "no window") }
        // The frame view includes the title bar and toolbar when it will draw them.
        let target = view.superview ?? view
        target.layoutSubtreeIfNeeded()
        guard let rep = target.bitmapImageRepForCachingDisplay(in: target.bounds) else { return (false, "no bitmap") }
        target.cacheDisplay(in: target.bounds, to: rep)
        guard let png = rep.representation(using: .png, properties: [:]) else { return (false, "no png") }
        do {
            try png.write(to: outDir.appending(path: "\(name).png"))
            return (true, "\(rep.pixelsWide)x\(rep.pixelsHigh) \(window.title)")
        } catch {
            return (false, "\(error)")
        }
    }

    func dump(name: String) -> (Bool, String?) {
        guard let window = currentWindow() else { return (false, "no window") }
        let root: Any = window
        var lines: [String] = ["Window: \(window.title)"]
        func walk(_ e: Any, _ depth: Int) {
            guard depth < 60 else { return }
            let d = describe(e)
            if !d.role.isEmpty && (d.title != "" || d.role != "AXGroup") {
                var line = String(repeating: "  ", count: depth) + d.role
                if !d.title.isEmpty { line += " \"\(d.title)\"" }
                if let a = e as? NSAccessibilityProtocol, !a.isAccessibilityEnabled() { line += " (disabled)" }
                lines.append(line)
            }
            for c in children(of: e) { walk(c, depth + 1) }
        }
        walk(root, 0)
        do {
            try lines.joined(separator: "\n").write(to: outDir.appending(path: "\(name).txt"), atomically: true, encoding: .utf8)
            return (true, "\(lines.count) lines")
        } catch {
            return (false, "\(error)")
        }
    }

    /// The menu bar as text: each item's title, its shortcut, and whether it's off, indented by depth.
    func dumpMenus(name: String) -> (Bool, String?) {
        guard let main = NSApp.mainMenu else { return (false, "no menu") }
        var lines: [String] = []
        func walk(_ menu: NSMenu, _ depth: Int) {
            menu.delegate?.menuNeedsUpdate?(menu)
            menu.update()
            for item in menu.items {
                if item.isSeparatorItem {
                    lines.append(String(repeating: "  ", count: depth) + "---")
                    continue
                }
                var line = String(repeating: "  ", count: depth) + item.title
                if !item.keyEquivalent.isEmpty { line += " [\(item.keyEquivalent)]" }
                if item.state == .on { line += " (checked)" }
                if !item.isEnabled { line += " (disabled)" }
                lines.append(line)
                if let sub = item.submenu { walk(sub, depth + 1) }
            }
        }
        walk(main, 0)
        do {
            try lines.joined(separator: "\n").write(to: outDir.appending(path: "\(name).txt"), atomically: true, encoding: .utf8)
            return (true, "\(lines.count) lines")
        } catch {
            return (false, "\(error)")
        }
    }

    // MARK: Synthesised input

    func typeText(_ text: String) -> (Bool, String?) {
        guard let window = currentWindow() else { return (false, "no window") }
        if let responder = window.firstResponder as? NSText {
            responder.insertText(text)
            return (true, nil)
        }
        if let responder = window.firstResponder as? NSTextInputClient {
            responder.insertText(text, replacementRange: NSRange(location: NSNotFound, length: 0))
            return (true, nil)
        }
        return (false, "first responder takes no text")
    }

    func flags(_ modifiers: [String]) -> NSEvent.ModifierFlags {
        var f: NSEvent.ModifierFlags = []
        for m in modifiers {
            switch m {
            case "cmd", "command": f.insert(.command)
            case "shift": f.insert(.shift)
            case "option", "alt": f.insert(.option)
            case "control", "ctrl": f.insert(.control)
            default: break
            }
        }
        return f
    }

    func pressKey(_ key: String, modifiers: [String]) -> (Bool, String?) {
        guard let window = currentWindow() else { return (false, "no window") }
        let named: [String: (String, UInt16)] = [
            "return": ("\r", 36), "escape": ("\u{1b}", 53), "space": (" ", 49), "tab": ("\t", 48),
            "delete": ("\u{7f}", 51), "left": ("\u{F702}", 123), "right": ("\u{F703}", 124),
            "down": ("\u{F701}", 125), "up": ("\u{F700}", 126),
        ]
        let (baseChars, code) = named[key] ?? (key, 0)
        // AppKit key equivalents distinguish Command-Z from Command-Shift-Z
        // using the shifted characters as well as flags, just like a real key.
        let chars = modifiers.contains("shift") && baseChars.count == 1 && baseChars.allSatisfy({ $0.isASCII && $0.isLetter })
            ? baseChars.uppercased() : baseChars
        for type in [NSEvent.EventType.keyDown, .keyUp] {
            guard let event = NSEvent.keyEvent(
                with: type, location: .zero, modifierFlags: flags(modifiers), timestamp: ProcessInfo.processInfo.systemUptime,
                windowNumber: window.windowNumber, context: nil, characters: chars, charactersIgnoringModifiers: chars,
                isARepeat: false, keyCode: code
            ) else { return (false, "could not build event") }
            if type == .keyDown, !modifiers.isEmpty, NSApp.mainMenu?.performKeyEquivalent(with: event) == true { return (true, nil) }
            window.sendEvent(event)
        }
        return (true, nil)
    }

    /// A click or drag at window coordinates (origin at the content view's top left).
    func mouse(_ s: Step) -> (Bool, String?) {
        guard let window = currentWindow(), let content = window.contentView else { return (false, "no window") }
        let toWindow = { (x: Double, y: Double) in NSPoint(x: x, y: content.bounds.height - y) }
        let start = toWindow(s.x ?? 0, s.y ?? 0)
        let end = toWindow(s.toX ?? (s.x ?? 0), s.toY ?? (s.y ?? 0))
        let mods = flags(s.modifiers ?? [])
        let clicks = s.clicks ?? 1
        func send(_ type: NSEvent.EventType, _ p: NSPoint, button: Int = 0, pressure: Float = 1) {
            if let e = NSEvent.mouseEvent(
                with: type, location: p, modifierFlags: mods, timestamp: ProcessInfo.processInfo.systemUptime,
                windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: type == .mouseMoved ? 0 : clicks, pressure: pressure
            ) {
                NSApp.postEvent(e, atStart: false)
            }
        }
        let kinds: (down: NSEvent.EventType, drag: NSEvent.EventType, up: NSEvent.EventType)
        switch s.button {
        case "none":
            // AppKit only sends moves to a tracking area for the real pointer, so hand them to the view under the point.
            for p in start == end ? [start] : [start, end] {
                if let e = NSEvent.mouseEvent(
                    with: .mouseMoved, location: p, modifierFlags: mods, timestamp: ProcessInfo.processInfo.systemUptime,
                    windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: 0, pressure: 0
                ) {
                    // `hitTest` takes a point in the superview's coordinates.
                    content.hitTest(content.superview?.convert(p, from: nil) ?? p)?.mouseMoved(with: e)
                }
            }
            return (true, nil)
        case "right":
            kinds = (.rightMouseDown, .rightMouseDragged, .rightMouseUp)
        case "middle":
            kinds = (.otherMouseDown, .otherMouseDragged, .otherMouseUp)
        default:
            kinds = (.leftMouseDown, .leftMouseDragged, .leftMouseUp)
        }
        if s.resume != true {
            send(kinds.down, start)
        }
        if start != end {
            let steps = s.moves ?? 8
            for i in 1...max(steps, 1) {
                let t = Double(i) / Double(max(steps, 1))
                send(kinds.drag, NSPoint(x: start.x + (end.x - start.x) * t, y: start.y + (end.y - start.y) * t))
            }
        }
        if s.hold != true {
            send(kinds.up, end)
        }
        return (true, nil)
    }

    /// A scroll wheel or trackpad event at window coordinates, delivered to the view under that point.
    func scroll(_ s: Step) -> (Bool, String?) {
        guard let window = currentWindow(), let content = window.contentView else { return (false, "no window") }
        let p = NSPoint(x: s.x ?? 0, y: content.bounds.height - (s.y ?? 0))
        let units: CGScrollEventUnit = s.precise == true ? .pixel : .line
        // CGEvent's wheels count up for away from the user, like `scrollingDeltaY`.
        guard let cg = CGEvent(
            scrollWheelEvent2Source: nil, units: units, wheelCount: 2, wheel1: Int32(s.dy ?? 0), wheel2: Int32(s.dx ?? 0), wheel3: 0
        ) else { return (false, "could not build event") }
        cg.flags = CGEventFlags(rawValue: UInt64(flags(s.modifiers ?? []).rawValue))
        let screen = window.convertPoint(toScreen: p)
        let top = NSScreen.screens.first?.frame.maxY ?? 0
        cg.location = CGPoint(x: screen.x, y: top - screen.y)
        cg.setIntegerValueField(.mouseEventWindowUnderMousePointer, value: Int64(window.windowNumber))
        cg.setIntegerValueField(.mouseEventWindowUnderMousePointerThatCanHandleThisEvent, value: Int64(window.windowNumber))
        guard let event = NSEvent(cgEvent: cg) else { return (false, "could not build event") }
        guard let view = content.hitTest(content.superview?.convert(p, from: nil) ?? p) else { return (false, "nothing at that point") }
        view.scrollWheel(with: event)
        return (true, event.window == nil ? "delivered with no window" : nil)
    }
}
