import AppKit
import SwiftUI
import VixenBridge

/// The plugins Studio knows about (Settings ▸ Plugins, the Plugins menu, the missing-plugin banner and the Add Controller
/// menus), read from the plugin host in Rust (`crates/studio-plugins`, `studio/PLUGINS-DESIGN.md`). Every action here is
/// the same host call the matching MCP tool makes.
@MainActor @Observable
public final class StudioPluginsModel {
    public static let shared = StudioPluginsModel()

    public private(set) var plugins: [StudioPlugin] = []
    public private(set) var commands: [StudioPluginCommand] = []
    /// The open show's plugins that aren't ready (not installed, too old, disabled, crashed).
    public private(set) var missing: [StudioPluginNeed] = []
    public private(set) var folder = ""
    public private(set) var loaded = false
    /// Changes whenever the plugin list or its commands do, so views listing controller types read them again.
    public private(set) var revision = 0
    /// A command running from the Plugins menu or ⌘K.
    public private(set) var running: StudioPluginCommand?

    private init() {}

    /// Controller type ids that belong to plugins, for the "Plugins" heading of the Add Controller menus.
    public var pluginTypeIDs: Set<String> {
        Set(plugins.flatMap { $0.controllerTypes.map(\.typeId) })
    }

    /// Reads the plugin list and the contributed commands again.
    public func refresh() async {
        let (list, commandsText) = await Task.detached { (try? studioPluginsList(), try? studioPluginCommands()) }.value
        let decoder = JSONDecoder()
        let next = list.flatMap { try? decoder.decode(StudioPluginList.self, from: Data($0.utf8)) }
        let nextCommands = commandsText.flatMap { try? decoder.decode([StudioPluginCommand].self, from: Data($0.utf8)) } ?? []
        var changed = false
        if let next, next.plugins != plugins {
            plugins = next.plugins
            changed = true
        }
        if let next, next.folder != folder { folder = next.folder }
        if nextCommands != commands {
            commands = nextCommands
            changed = true
        }
        if changed { revision &+= 1 }
        loaded = true
    }

    /// Tells the host about the open show and Live Output (starting the enabled plugins the first time), and reads what
    /// the show needs for the banner.
    public func sync(core: VixenCore) async {
        let text = await Task.detached { try? core.studioPluginsSync() }.value
        if let text, let needs = try? JSONDecoder().decode(StudioPluginNeeds.self, from: Data(text.utf8)) {
            let unmet = needs.plugins.filter { !$0.satisfied }
            if unmet != missing { missing = unmet }
        }
    }

    /// Runs a host call off the main actor, then reads the list again. Returns the error's message when it failed.
    @discardableResult
    public func perform(_ work: @escaping @Sendable () throws -> Void) async -> String? {
        let result = await Task.detached { Result { try work() } }.value
        await refresh()
        if case .failure(let error) = result { return error.userMessage }
        return nil
    }

    /// The MCP tool a Plugins menu item runs, for the ⌘K palette (`Plugins ▸ <plugin> ▸ <title>`).
    public func tool(forMenuPath path: [String]) -> String? {
        guard path.count == 3, path[0] == StudioPluginsCommands.menuTitle else { return nil }
        return commands.first { $0.pluginName == path[1] && $0.title == path[2] }?.tool
    }

    /// Runs a contributed command (its tool, no arguments) and shows what it answered.
    public func run(_ command: StudioPluginCommand) {
        guard running == nil else { return }
        running = command
        Task {
            let pluginID = command.pluginId, commandID = command.id
            let result = await Task.detached { () -> Result<String, Error> in
                Result { try studioPluginRunCommand(pluginId: pluginID, commandId: commandID) }
            }.value
            running = nil
            let (text, failed): (String, Bool)
            switch result {
            case .success(let json):
                let answer = try? JSONDecoder().decode(StudioToolAnswer.self, from: Data(json.utf8))
                text = answer?.text ?? json
                failed = answer?.isError ?? false
            case .failure(let error):
                text = error.userMessage
                failed = true
            }
            Self.show(title: command.title.trimmingCharacters(in: CharacterSet(charactersIn: "…. ")), text: text.isEmpty ? "Done." : text, failed: failed)
            await refresh()
        }
    }

    private static func show(title: String, text: String, failed: Bool) {
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = String(text.prefix(2_000))
        alert.alertStyle = failed ? .warning : .informational
        alert.addButton(withTitle: "Done")
        if let window = NSApp.keyWindow ?? NSApp.mainWindow {
            alert.beginSheetModal(for: window)
        } else {
            alert.runModal()
        }
    }
}

// MARK: Rust's JSON

nonisolated public struct StudioPluginList: Decodable, Sendable {
    public let folder: String
    public let plugins: [StudioPlugin]
}

/// One plugin as `plugin_list` reports it.
nonisolated public struct StudioPlugin: Decodable, Equatable, Identifiable, Sendable {
    public struct ControllerType: Decodable, Equatable, Sendable {
        public let typeId: String
        public let name: String
    }
    public let id: String
    public let name: String?
    public let version: String?
    public let description: String?
    public let source: String
    public let enabled: Bool
    public let state: String
    public let folder: String
    public let restartCount: Int
    public let problem: String?
    public let lastError: String?
    /// Whether the running plugin was started under its macOS sandbox profile; nil while it isn't running.
    public let sandboxed: Bool?
    private let controllerTypesList: [ControllerType]?
    private let toolsList: [String]?
    private let commandsList: [String]?

    public var controllerTypes: [ControllerType] { controllerTypesList ?? [] }
    public var tools: [String] { toolsList ?? [] }
    public var commandIds: [String] { commandsList ?? [] }
    public var title: String { name ?? id }
    public var isDev: Bool { source == "dev" }

    enum CodingKeys: String, CodingKey {
        case id, name, version, description, source, enabled, state, folder, restartCount, problem, lastError, sandboxed
        case controllerTypesList = "controllerTypes", toolsList = "tools", commandsList = "commands"
    }

    /// The state in words, as the Settings list shows it.
    public var stateText: String {
        switch state {
        case "running": "Running"
        case "starting": "Starting"
        case "stopped": "Stopped"
        case "restarting": "Restarting"
        case "crashed": "Crashed"
        case "disabled": "Off"
        case "invalid": "Invalid"
        case "incompatible": "Incompatible"
        default: state.capitalized
        }
    }

    public var stateTone: StudioStatusBadge.Tone {
        switch state {
        case "running": .active
        case "crashed", "invalid", "incompatible": .error
        case "restarting", "starting": .warning
        default: .neutral
        }
    }
}

/// A command a plugin adds to the Plugins menu.
nonisolated public struct StudioPluginCommand: Decodable, Equatable, Identifiable, Sendable {
    public let pluginId: String
    public let pluginName: String
    public let id: String
    public let title: String
    public let tool: String?
    public let enabled: Bool
    public var key: String { "\(pluginId)/\(id)" }
}

nonisolated public struct StudioPluginNeeds: Decodable, Sendable {
    public let plugins: [StudioPluginNeed]
}

/// One plugin the open show needs (`studio_plugins_required`).
nonisolated public struct StudioPluginNeed: Decodable, Equatable, Sendable, Identifiable {
    public let id: String
    public let versionRange: String
    public let installedVersion: String?
    public let state: String?
    public let satisfied: Bool
    public let message: String?
}

/// A tool's MCP-style answer: the text of its text items.
nonisolated struct StudioToolAnswer: Decodable {
    struct Item: Decodable { let type: String; let text: String? }
    let content: [Item]
    let isError: Bool?
    var text: String { content.compactMap(\.text).joined(separator: "\n") }
}

// MARK: The Plugins menu

/// Plugins in the menu bar: each plugin's contributed commands in a submenu of its own, and Manage Plugins… for Settings ▸
/// Plugins. The ⌘K palette finds the commands here, with the plugin tool each one runs.
public struct StudioPluginsCommands: Commands {
    public static let menuTitle = "Plugins"
    public init() {}

    public var body: some Commands {
        CommandMenu(Self.menuTitle) {
            StudioPluginsMenuContent()
        }
    }
}

/// The selected Settings tab, so Manage Plugins… can open Settings on Plugins.
public enum StudioSettingsTab {
    public static let storageKey = "studio.settingsTab"
    public static let plugins = "plugins"
}

private struct StudioPluginsMenuContent: View {
    @Environment(\.openSettings) private var openSettings
    private var model: StudioPluginsModel { .shared }

    var body: some View {
        let groups = Dictionary(grouping: model.commands, by: \.pluginId)
        let order = model.commands.map(\.pluginId).reduce(into: [String]()) { if !$0.contains($1) { $0.append($1) } }
        if order.isEmpty {
            Button("No Plugin Commands") {}
                .disabled(true)
        }
        ForEach(order, id: \.self) { pluginID in
            let commands = groups[pluginID] ?? []
            Menu(commands.first?.pluginName ?? pluginID) {
                ForEach(commands, id: \.key) { command in
                    Button(command.title) { model.run(command) }
                        .disabled(!command.enabled || model.running != nil)
                }
            }
        }
        Divider()
        Button("Manage Plugins…") {
            UserDefaults.standard.set(StudioSettingsTab.plugins, forKey: StudioSettingsTab.storageKey)
            openSettings()
        }
    }
}
