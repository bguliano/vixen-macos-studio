import AppKit
import SwiftUI
import VixenBridge

/// View ▸ Command Palette… (⌘K): every command in the menu bar of the front window, searchable, with its shortcut, why it
/// is disabled and which MCP tool does the same thing. The list is read from the live menu bar each time it opens, so a menu
/// an area adds (or a menu that is hidden and shown as windows change) can never leave a command unreachable. Ranking, the
/// disabled reasons and the agent equivalents come from Rust (`studio_palette.rs`), shared with the MCP `command_palette` tool.
public struct StudioCommandPaletteCommands: Commands {
    public init() {
        StudioCommandPalette.installHarnessHook()
    }

    public var body: some Commands {
        CommandGroup(before: .toolbar) {
            Button(StudioCommandPalette.menuTitle) { StudioCommandPalette.present() }
                .keyboardShortcut("k", modifiers: .command)
            Divider()
        }
    }
}

/// One command found in the menu bar: where it is (titles and indexes from the menu bar down), its shortcut and state.
struct PaletteCommand {
    var path: [String]
    var indexes: [Int]
    var shortcut: String
    var enabled: Bool
    var checked: Bool
}

/// One ranked result from Rust.
struct PaletteMatch: Identifiable {
    var id: Int { index }
    var index: Int
    var title: String
    var location: String
    var shortcut: String
    var enabled: Bool
    var checked: Bool
    var reason: String?
    var agentTool: String?
}

public enum StudioCommandPalette {
    static let menuTitle = "Command Palette…"
    private static var panel: NSWindow?
    private static var model: PaletteModel?

    /// "sequencer" when a sequencer owns the menu bar (its Sequence menu is showing), otherwise "show".
    static func windowKind() -> String {
        let sequence = NSApp.mainMenu?.items.first { $0.title == "Sequence" }
        return sequence.map { $0.isHidden ? "show" : "sequencer" } ?? "show"
    }

    /// Every visible command in the menu bar, brought up to date as if each menu were opened. Separators, submenu titles,
    /// hidden items, the system Services menu and the palette itself are left out.
    static func commands() -> [PaletteCommand] {
        guard let main = NSApp.mainMenu else { return [] }
        var found: [PaletteCommand] = []
        func walk(_ menu: NSMenu, _ path: [String], _ indexes: [Int]) {
            // SwiftUI brings a menu's items up to date when it's about to open.
            menu.delegate?.menuNeedsUpdate?(menu)
            menu.update()
            for (i, item) in menu.items.enumerated() where !item.isSeparatorItem && !item.isHidden && !item.title.isEmpty {
                if let sub = item.submenu {
                    if path.isEmpty || item.title != "Services" {
                        walk(sub, path + [item.title], indexes + [i])
                    }
                } else if item.title != menuTitle {
                    found.append(PaletteCommand(path: path + [item.title], indexes: indexes + [i], shortcut: shortcut(item),
                                                enabled: item.isEnabled, checked: item.state == .on))
                }
            }
        }
        walk(main, [], [])
        return found
    }

    /// A menu item's shortcut as the menu shows it: ⌃⌥⇧⌘ then the key.
    static func shortcut(_ item: NSMenuItem) -> String {
        let key = item.keyEquivalent
        guard !key.isEmpty else { return "" }
        var mods = item.keyEquivalentModifierMask
        if key.count == 1, let c = key.first, c.isUppercase { mods.insert(.shift) }
        var text = ""
        if mods.contains(.control) { text += "⌃" }
        if mods.contains(.option) { text += "⌥" }
        if mods.contains(.shift) { text += "⇧" }
        if mods.contains(.command) { text += "⌘" }
        let scalar = key.unicodeScalars.first.map { Int($0.value) } ?? 0
        switch scalar {
        case NSF1FunctionKey...NSF35FunctionKey: text += "F\(scalar - NSF1FunctionKey + 1)"
        case 0x08, 0x7f, NSDeleteFunctionKey: text += "⌫"
        case 0x0d, 0x03: text += "↩"
        case 0x1b: text += "⎋"
        case 0x09: text += "⇥"
        case 0x20: text += "Space"
        case NSUpArrowFunctionKey: text += "↑"
        case NSDownArrowFunctionKey: text += "↓"
        case NSLeftArrowFunctionKey: text += "←"
        case NSRightArrowFunctionKey: text += "→"
        case NSHomeFunctionKey: text += "↖"
        case NSEndFunctionKey: text += "↘"
        case NSPageUpFunctionKey: text += "⇞"
        case NSPageDownFunctionKey: text += "⇟"
        default: text += key.uppercased()
        }
        return text
    }

    /// Reads the menu bar and hands it to Rust for searching (and the MCP tool). Returns the commands and the window kind.
    @discardableResult
    static func publish() -> (kind: String, commands: [PaletteCommand]) {
        let kind = windowKind()
        let found = commands()
        let json = found.map { ["path": $0.path, "shortcut": $0.shortcut, "enabled": $0.enabled, "checked": $0.checked] as [String: Any] }
        if let data = try? JSONSerialization.data(withJSONObject: json), let text = String(data: data, encoding: .utf8) {
            _ = try? studioPalettePublish(window: kind, entriesJson: text)
        }
        return (kind, found)
    }

    /// Ranked matches for `query` among the commands last published for `kind`.
    static func search(_ query: String, kind: String, limit: UInt32 = 500) -> [PaletteMatch] {
        guard let text = try? studioPaletteSearch(window: kind, query: query, limit: limit),
              let root = try? JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: Any],
              let matches = root["matches"] as? [[String: Any]] else { return [] }
        return matches.map { m in
            let path = m["path"] as? [String] ?? []
            return PaletteMatch(index: m["index"] as? Int ?? 0, title: m["title"] as? String ?? "",
                                location: path.dropLast().joined(separator: " ▸ "), shortcut: m["shortcut"] as? String ?? "",
                                enabled: m["enabled"] as? Bool ?? false, checked: m["checked"] as? Bool ?? false,
                                reason: m["reason"] as? String, agentTool: m["agentTool"] as? String)
        }
    }

    /// Runs a command as if it were chosen from its menu: found again by its indexes (menus can rebuild while the palette is
    /// open), or by its titles if they moved, and only when it is still enabled.
    @discardableResult
    static func perform(_ command: PaletteCommand) -> String? {
        guard var menu = NSApp.mainMenu else { return "no menu bar" }
        for (depth, title) in command.path.enumerated() {
            menu.delegate?.menuNeedsUpdate?(menu)
            menu.update()
            let at = command.indexes[depth]
            let item = at < menu.items.count && menu.items[at].title == title && !menu.items[at].isHidden
                ? menu.items[at] : menu.items.first { $0.title == title && !$0.isHidden }
            guard let item else { return "\(title) is no longer in the menu" }
            if depth == command.path.count - 1 {
                guard item.isEnabled else { return "\(title) is disabled" }
                menu.performActionForItem(at: menu.index(of: item))
                return nil
            }
            guard let sub = item.submenu else { return "\(title) has no submenu" }
            menu = sub
        }
        return "empty path"
    }

    /// The window the palette attaches to: the front document or show window, never a floating panel or another sheet.
    static func hostWindow(titled title: String? = nil) -> NSWindow? {
        if let title { return NSApp.windows.first { $0.title == title && $0.isVisible } }
        var window = NSApp.keyWindow ?? NSApp.mainWindow
        while let w = window, w.sheetParent != nil || w.parent != nil { window = w.sheetParent ?? w.parent }
        return window ?? NSApp.orderedWindows.first { $0.isVisible && $0.canBecomeMain }
    }

    /// Opens the palette as a sheet on the front window. ⌘K again, Escape or running a command closes it.
    public static func present(over host: NSWindow? = nil) {
        if panel != nil { close(); return }
        guard let host = host ?? hostWindow(), host.attachedSheet == nil else { NSSound.beep(); return }
        let (kind, found) = publish()
        let model = PaletteModel(kind: kind, commands: found)
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 640, height: 440), styleMask: [.titled, .fullSizeContentView],
                              backing: .buffered, defer: false)
        window.title = menuTitle
        window.contentViewController = NSHostingController(rootView: PaletteView(model: model).studioAppearance())
        window.setContentSize(NSSize(width: 640, height: 440))
        self.model = model
        self.panel = window
        host.beginSheet(window)
    }

    static func close() {
        guard let panel else { return }
        panel.sheetParent?.endSheet(panel)
        panel.orderOut(nil)
        self.panel = nil
        model = nil
    }

    /// Closes the palette, then runs the command once its window is in front again, so the command sees that window's state.
    static func run(_ match: PaletteMatch, in model: PaletteModel) {
        guard match.enabled, model.commands.indices.contains(match.index) else { NSSound.beep(); return }
        let command = model.commands[match.index]
        close()
        DispatchQueue.main.async {
            if let problem = perform(command) {
                NSSound.beep()
                if UITestHarness.isActive { UITestHarness.note("palette: \(problem)") }
            }
        }
    }

    /// `{ "action": "hook", "name": "studio.palette", "path": [...] }`: `open [window title]`, `query <text>`, `run <text>`
    /// (the best enabled match, as Return does), `check` (every visible menu command is listed and found by its own title,
    /// every disabled one says why; writes palette-<window>.txt), `close`.
    static func installHarnessHook() {
        guard UITestHarness.isActive, UITestHarness.hooks["studio.palette"] == nil else { return }
        UITestHarness.hooks["studio.palette"] = { args in
            switch args.first ?? "" {
            case "open":
                close()
                guard let host = hostWindow(titled: args.count > 1 ? args[1] : nil) else { return "FAIL no window" }
                present(over: host)
                guard let model else { return "FAIL palette did not open" }
                return "\(model.kind) \(model.commands.count) commands, \(model.commands.filter { !$0.enabled }.count) disabled"
            case "query":
                guard let model else { return "FAIL palette not open" }
                model.query = args.dropFirst().joined(separator: " ")
                return model.results.prefix(5).map { "\($0.location) ▸ \($0.title)" }.joined(separator: " | ")
            case "run":
                guard let model else { return "FAIL palette not open" }
                model.query = args.dropFirst().joined(separator: " ")
                guard let best = model.results.first(where: \.enabled) else { return "FAIL nothing enabled matches" }
                run(best, in: model)
                try? await Task.sleep(for: .milliseconds(300))
                return "ran \(best.location) ▸ \(best.title)"
            case "check":
                return check()
            case "close":
                close()
                return "closed"
            default:
                return "FAIL unknown palette step"
            }
        }
    }

    /// Walks the menu bar again and proves every visible command is reachable from the palette: searching its title lists it
    /// among the commands with that title (plus a little slack), and a disabled one carries a reason.
    static func check() -> String {
        let (kind, found) = publish()
        guard !found.isEmpty else { return "FAIL no commands" }
        var lines: [String] = []
        var problems: [String] = []
        for (i, command) in found.enumerated() {
            let title = command.path.last ?? ""
            let sameTitle = found.filter { $0.path.last == title }.count
            let results = search(title, kind: kind)
            guard let rank = results.firstIndex(where: { $0.index == i }) else {
                problems.append("not found: \(command.path.joined(separator: " ▸ "))")
                continue
            }
            if rank >= sameTitle + 3 { problems.append("ranked \(rank + 1): \(command.path.joined(separator: " ▸ "))") }
            let match = results[rank]
            if !command.enabled && (match.reason ?? "").isEmpty { problems.append("no reason: \(command.path.joined(separator: " ▸ "))") }
            lines.append([command.path.joined(separator: " ▸ "), command.shortcut, command.enabled ? "on" : "off",
                          match.reason ?? "", match.agentTool ?? ""].joined(separator: " | "))
        }
        let out = ProcessInfo.processInfo.environment["VIXEN_UI_TEST_OUT"] ?? NSTemporaryDirectory()
        try? lines.joined(separator: "\n").write(to: URL(fileURLWithPath: out).appending(path: "palette-\(kind).txt"), atomically: true, encoding: .utf8)
        let disabled = found.filter { !$0.enabled }.count
        let summary = "\(kind): \(found.count) commands, \(disabled) disabled, \(found.count - problems.count) reachable"
        return problems.isEmpty ? "PASS \(summary)" : "FAIL \(summary); " + problems.prefix(10).joined(separator: "; ")
    }
}

@Observable
final class PaletteModel {
    let kind: String
    let commands: [PaletteCommand]
    var query = "" { didSet { refresh() } }
    private(set) var results: [PaletteMatch] = []
    var selection = 0

    init(kind: String, commands: [PaletteCommand]) {
        self.kind = kind
        self.commands = commands
        refresh()
    }

    private func refresh() {
        results = StudioCommandPalette.search(query, kind: kind)
        // Start on the best command that can run.
        selection = results.firstIndex(where: \.enabled) ?? 0
    }

    func move(_ by: Int) {
        guard !results.isEmpty else { return }
        selection = min(max(selection + by, 0), results.count - 1)
    }
}

struct PaletteView: View {
    @Bindable var model: PaletteModel
    @FocusState private var searching: Bool

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: StudioMetrics.spacingS) {
                Image(systemName: "magnifyingglass").foregroundStyle(.secondary)
                TextField("Search commands", text: $model.query)
                    .textFieldStyle(.plain)
                    .font(.title3)
                    .focused($searching)
                    .onKeyPress(.upArrow) { model.move(-1); return .handled }
                    .onKeyPress(.downArrow) { model.move(1); return .handled }
                    .onSubmit(runSelection)
                    .accessibilityLabel("Search commands")
            }
            .padding(StudioMetrics.spacingM)
            Divider()
            if model.results.isEmpty {
                ContentUnavailableView.search(text: model.query)
                    .frame(maxHeight: .infinity)
            } else {
                ScrollViewReader { proxy in
                    ScrollView {
                        LazyVStack(spacing: 2) {
                            ForEach(Array(model.results.enumerated()), id: \.element.id) { offset, match in
                                PaletteRow(match: match, selected: offset == model.selection)
                                    .id(match.id)
                                    .contentShape(Rectangle())
                                    .onTapGesture {
                                        model.selection = offset
                                        runSelection()
                                    }
                            }
                        }
                        .padding(StudioMetrics.spacingXS)
                    }
                    .onChange(of: model.selection) { _, new in
                        if model.results.indices.contains(new) { proxy.scrollTo(model.results[new].id) }
                    }
                }
            }
            Divider()
            HStack {
                Text("\(model.results.count) of \(model.commands.count) commands")
                Spacer()
                Text("↑↓ Choose   ↩ Run   ⎋ Close")
            }
            .font(.caption)
            .foregroundStyle(.secondary)
            .padding(.horizontal, StudioMetrics.spacingM)
            .padding(.vertical, StudioMetrics.spacingS)
        }
        .frame(minWidth: 520, idealWidth: 640, minHeight: 320, idealHeight: 440)
        .background {
            Button("Close", action: StudioCommandPalette.close).keyboardShortcut(.cancelAction).hidden()
        }
        .onAppear { searching = true }
    }

    private func runSelection() {
        guard model.results.indices.contains(model.selection) else { NSSound.beep(); return }
        StudioCommandPalette.run(model.results[model.selection], in: model)
    }
}

struct PaletteRow: View {
    let match: PaletteMatch
    let selected: Bool

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: StudioMetrics.spacingS) {
            Image(systemName: match.checked ? "checkmark" : "command")
                .foregroundStyle(.secondary)
                .frame(width: 16)
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: StudioMetrics.spacingXS) {
                    Text(match.title).foregroundStyle(match.enabled ? .primary : .secondary)
                    if !match.location.isEmpty {
                        Text(match.location).font(.caption).foregroundStyle(.secondary)
                    }
                }
                if let reason = match.reason {
                    Label(reason, systemImage: "info.circle").font(.caption).foregroundStyle(.secondary)
                }
            }
            Spacer(minLength: StudioMetrics.spacingS)
            if let tool = match.agentTool {
                Text(tool)
                    .font(.caption.monospaced())
                    .foregroundStyle(.secondary)
                    .padding(.horizontal, 5)
                    .padding(.vertical, 1)
                    .background(.quaternary, in: Capsule())
                    .help("Agents can do this with the MCP tool \(tool)")
            }
            if !match.shortcut.isEmpty {
                Text(match.shortcut).font(.body.monospaced()).foregroundStyle(.secondary)
            }
        }
        .padding(.horizontal, StudioMetrics.spacingS)
        .padding(.vertical, 5)
        .background(selected ? AnyShapeStyle(.selection) : AnyShapeStyle(.clear), in: RoundedRectangle(cornerRadius: 6))
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(.isButton)
        .help(match.enabled ? "Run \(match.title)" : (match.reason ?? "Not available right now"))
    }
}
