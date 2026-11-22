import AppKit
import SwiftUI
import UniformTypeIdentifiers
import VixenBridge
import VixenKit

/// Settings ▸ Plugins: every installed and linked plugin with its state and an On switch, installing from a file or a
/// dropped package (after its permissions are shown), linking a development folder, and per plugin Reload, Show in
/// Finder, Log and Uninstall (Unlink for a development folder), plus the local "Allow agents to manage plugins" switch.
/// Each action is the plugin host call the matching MCP tool makes (`plugin_install`, `plugin_enable`, …).
struct StudioPluginsSettings: View {
    @State private var selection: String?
    @State private var details: StudioPluginDetails?
    @State private var pending: StudioPluginPackage?
    @State private var logPlugin: StudioPlugin?
    @State private var importing: ImportKind?
    @State private var error: String?
    @State private var working = false
    @State private var agentsMayManage = studioPluginsAgentsMayManage()
    @State private var dropTargeted = false
    @State private var confirmRemove: StudioPlugin?

    private var model: StudioPluginsModel { .shared }

    private enum ImportKind: Identifiable {
        case install, link
        var id: Self { self }
    }

    var body: some View {
        Form {
            Section {
                list
                    .frame(height: 170)
                HStack(spacing: StudioMetrics.spacingS) {
                    Button("Install from File…") { importing = .install }
                        .help("Install a .vixenplugin package or a plugin folder. You see what it can do before it's installed.")
                    Button("Link Development Folder…") { importing = .link }
                        .help("Run a plugin you're writing straight from its folder; Studio restarts it whenever a file changes.")
                    Spacer()
                    if working { ProgressView().controlSize(.small) }
                }
                .disabled(working)
                if let error {
                    Label(error, systemImage: "exclamationmark.triangle.fill")
                        .symbolRenderingMode(.multicolor)
                        .font(.callout)
                        .fixedSize(horizontal: false, vertical: true)
                }
            } header: {
                Text("Installed")
            } footer: {
                Text("Plugins add controller types and agent tools. Each runs as its own program, as you, like any app you install: a crash never stops the show.")
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
            if let plugin = selectedPlugin {
                detailSection(plugin)
            }
            Section {
                Toggle("Allow agents to manage plugins", isOn: Binding(
                    get: { agentsMayManage },
                    set: { allowed in
                        agentsMayManage = allowed
                        studioPluginsSetAgentsMayManage(allowed: allowed)
                    }
                ))
            } footer: {
                Text("Off each time Studio starts. While off, agents can list, inspect, write and test plugins, but only you can install, remove, turn on or off, link or reload them.")
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .frame(minWidth: 560, idealWidth: 620)
        .fixedSize(horizontal: false, vertical: true)
        .fileImporter(isPresented: Binding(get: { importing != nil }, set: { if !$0 { importing = nil } }),
                      allowedContentTypes: importing == .link ? [.folder] : [.folder, Self.packageType]) { result in
            let kind = importing
            importing = nil
            guard case .success(let url) = result else { return }
            if kind == .link { link(url) } else { inspect(url) }
        }
        .sheet(item: $pending) { package in
            StudioPluginInstallSheet(package: package) { install(package) }
        }
        .sheet(item: $logPlugin) { plugin in
            StudioPluginLogSheet(plugin: plugin)
        }
        .confirmationDialog(confirmRemove.map { $0.isDev ? "Unlink “\($0.title)”?" : "Uninstall “\($0.title)”?" } ?? "",
                            isPresented: Binding(get: { confirmRemove != nil }, set: { if !$0 { confirmRemove = nil } }),
                            presenting: confirmRemove) { plugin in
            Button(plugin.isDev ? "Unlink" : "Uninstall", role: .destructive) { remove(plugin) }
        } message: { plugin in
            Text(plugin.isDev
                 ? "The plugin stops and its folder stays where it is."
                 : "The plugin stops and its files are removed; its saved settings and log stay. Shows that use its controllers keep them, silent, until it's installed again.")
        }
        .task {
            installHarnessHook()
            while !Task.isCancelled {
                await model.refresh()
                if selection == nil { selection = model.plugins.first?.id }
                await loadDetails()
                try? await Task.sleep(for: .seconds(1))
            }
        }
        .onChange(of: selection) { Task { await loadDetails() } }
    }

    static let packageType = UTType(filenameExtension: "vixenplugin") ?? .zip

    private var selectedPlugin: StudioPlugin? {
        model.plugins.first { $0.id == selection }
    }

    // MARK: List

    @ViewBuilder
    private var list: some View {
        Group {
            if model.plugins.isEmpty {
                ContentUnavailableView {
                    Label("No Plugins", systemImage: "puzzlepiece.extension")
                } description: {
                    Text("Drop a .vixenplugin here, install one from a file, or link a folder you're developing.")
                }
                .frame(maxWidth: .infinity)
            } else {
                List(selection: $selection) {
                    ForEach(model.plugins) { plugin in
                        StudioPluginRow(plugin: plugin, toggle: { on in setEnabled(plugin, on) })
                            .tag(plugin.id)
                            .contextMenu { actions(plugin) }
                    }
                }
                .listStyle(.inset)
                .alternatingRowBackgrounds()
            }
        }
        .overlay {
            if dropTargeted {
                RoundedRectangle(cornerRadius: StudioMetrics.smallCornerRadius)
                    .strokeBorder(Color.accentColor, lineWidth: 2)
            }
        }
        .onDrop(of: [.fileURL], isTargeted: $dropTargeted) { providers in
            guard let provider = providers.first else { return false }
            _ = provider.loadObject(ofClass: URL.self) { url, _ in
                guard let url else { return }
                Task { @MainActor in inspect(url) }
            }
            return true
        }
    }

    @ViewBuilder
    private func actions(_ plugin: StudioPlugin) -> some View {
        Button("Reload") { reload(plugin) }
            .disabled(!plugin.enabled)
        Button("Show in Finder") { NSWorkspace.shared.activateFileViewerSelecting([URL(filePath: plugin.folder)]) }
        Button("Log…") { logPlugin = plugin }
        Divider()
        Button(plugin.isDev ? "Unlink…" : "Uninstall…", role: .destructive) { confirmRemove = plugin }
    }

    // MARK: Details

    @ViewBuilder
    private func detailSection(_ plugin: StudioPlugin) -> some View {
        Section(plugin.title) {
            if let text = plugin.description, !text.isEmpty {
                Text(text)
                    .fixedSize(horizontal: false, vertical: true)
            }
            LabeledContent("Version", value: plugin.version ?? "—")
            LabeledContent("Source", value: plugin.isDev ? "Development folder" : "Installed")
            LabeledContent("State") {
                StudioStatusBadge(plugin.stateText, tone: plugin.stateTone)
            }
            if let problem = plugin.problem ?? (plugin.state == "crashed" ? plugin.lastError : nil) {
                Label(problem, systemImage: "exclamationmark.triangle.fill")
                    .symbolRenderingMode(.multicolor)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if !plugin.controllerTypes.isEmpty {
                LabeledContent("Controllers", value: plugin.controllerTypes.map(\.name).joined(separator: ", "))
            }
            if !plugin.tools.isEmpty {
                LabeledContent("Agent tools", value: plugin.tools.joined(separator: ", "))
            }
            if let details, details.id == plugin.id {
                LabeledContent("It can") {
                    if details.permissionText.isEmpty {
                        Text("Nothing beyond running")
                            .foregroundStyle(.secondary)
                    } else {
                        VStack(alignment: .trailing, spacing: 2) {
                            ForEach(details.permissionText, id: \.self) { Text($0) }
                        }
                    }
                }
            }
            HStack(spacing: StudioMetrics.spacingS) {
                Button("Reload") { reload(plugin) }
                    .disabled(!plugin.enabled || working)
                    .help("Stop the plugin, read its manifest again and start it, clearing a crash")
                Button("Show in Finder") { NSWorkspace.shared.activateFileViewerSelecting([URL(filePath: plugin.folder)]) }
                Button("Log…") { logPlugin = plugin }
                    .help("What the plugin and Studio wrote about it: its messages, starts, exits and errors")
                Spacer()
                Button(plugin.isDev ? "Unlink…" : "Uninstall…", role: .destructive) { confirmRemove = plugin }
                    .disabled(working)
            }
        }
    }

    private func loadDetails() async {
        guard let id = selection else { details = nil; return }
        let text = await Task.detached { try? studioPluginInfo(id: id) }.value
        let next = text.flatMap { try? JSONDecoder().decode(StudioPluginDetails.self, from: Data($0.utf8)) }
        if next != details { details = next }
    }

    // MARK: Actions

    private func run(_ work: @escaping @Sendable () throws -> Void) {
        working = true
        Task {
            error = await model.perform(work)
            working = false
            await loadDetails()
        }
    }

    private func setEnabled(_ plugin: StudioPlugin, _ on: Bool) {
        let id = plugin.id
        run { _ = try studioPluginSetEnabled(id: id, enabled: on) }
    }

    private func reload(_ plugin: StudioPlugin) {
        let id = plugin.id
        run { _ = try studioPluginReload(id: id) }
    }

    private func remove(_ plugin: StudioPlugin) {
        let id = plugin.id, dev = plugin.isDev
        if selection == id { selection = nil }
        run {
            if dev { try studioPluginUnlinkDev(id: id) } else { _ = try studioPluginUninstall(id: id) }
        }
    }

    private func link(_ url: URL) {
        let path = url.standardizedFileURL.path
        working = true
        Task {
            let failure = await model.perform { _ = try studioPluginLinkDev(path: path) }
            error = failure
            working = false
            if failure == nil, let added = model.plugins.first(where: { $0.folder == path }) { selection = added.id }
        }
    }

    /// Reads the package's manifest without installing it, for the permissions sheet.
    private func inspect(_ url: URL) {
        let path = url.standardizedFileURL.path
        working = true
        Task {
            let result = await Task.detached { Result { try studioPluginInspectPackage(path: path) } }.value
            working = false
            switch result {
            case .success(let text):
                if var package = try? JSONDecoder().decode(StudioPluginPackage.self, from: Data(text.utf8)) {
                    package.path = path
                    error = nil
                    pending = package
                }
            case .failure(let failure):
                error = failure.userMessage
            }
        }
    }

    private func install(_ package: StudioPluginPackage) {
        pending = nil
        let path = package.path
        working = true
        Task {
            let failure = await model.perform { _ = try studioPluginInstall(path: path) }
            error = failure
            working = false
            if failure == nil { selection = package.id }
        }
    }

    // MARK: Harness

    /// `{ "action": "hook", "name": "studio.plugins", "path": [...] }`: `inspect <path>` (the install sheet), `confirm`
    /// and `cancel` (its buttons), `install <path>`, `link <path>`, `select <id>`, `log <id>`, `closeLog`, `list` (ids and
    /// states). The file pickers aren't scriptable.
    private func installHarnessHook() {
        guard UITestHarness.isActive else { return }
        UITestHarness.hooks["studio.plugins"] = { args in
            let rest = args.dropFirst().joined(separator: " ")
            switch args.first ?? "" {
            case "inspect": inspect(URL(filePath: rest))
            case "install":
                let path = rest
                let failure = await model.perform { _ = try studioPluginInstall(path: path) }
                if let failure { return "FAIL \(failure)" }
            case "link": link(URL(filePath: rest))
            case "confirm":
                guard let package = pending else { return "FAIL no install sheet" }
                install(package)
                try? await Task.sleep(for: .seconds(2))
            case "cancel": pending = nil
            case "closeLog": logPlugin = nil
            case "select": selection = rest
            case "log": logPlugin = model.plugins.first { $0.id == rest }
            case "list":
                await model.refresh()
                return model.plugins.map { "\($0.id)=\($0.state)" }.joined(separator: ",")
            default: return "FAIL unknown \(args.first ?? "")"
            }
            try? await Task.sleep(for: .milliseconds(400))
            return error.map { "error: \($0)" } ?? "ok"
        }
    }
}

/// One plugin in the list: name, version, where it came from, its state and an On switch.
private struct StudioPluginRow: View {
    let plugin: StudioPlugin
    let toggle: (Bool) -> Void

    var body: some View {
        HStack(spacing: StudioMetrics.spacingS) {
            Image(systemName: plugin.isDev ? "hammer" : "puzzlepiece.extension")
                .foregroundStyle(.secondary)
                .frame(width: 18)
            VStack(alignment: .leading, spacing: 1) {
                Text(plugin.title)
                    .lineLimit(1)
                Text([plugin.version, plugin.isDev ? "Development" : nil].compactMap { $0 }.joined(separator: " · "))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
            Spacer(minLength: StudioMetrics.spacingS)
            StudioStatusBadge(plugin.stateText, tone: plugin.stateTone)
            Toggle("On", isOn: Binding(get: { plugin.enabled }, set: { toggle($0) }))
                .toggleStyle(.switch)
                .controlSize(.small)
                .labelsHidden()
                .disabled(plugin.state == "invalid" || plugin.state == "incompatible")
                .accessibilityLabel("\(plugin.title) on")
        }
        .padding(.vertical, 2)
    }
}

/// What installing a package would install (`studio_plugin_inspect_package`).
struct StudioPluginPackage: Decodable, Identifiable {
    let id: String
    let name: String
    let version: String
    let description: String?
    let author: String?
    let permissions: [String]
    let files: Int
    let bytes: Int
    let installedVersion: String?
    let problem: String?
    let controllerTypes: [StudioPlugin.ControllerType]
    let tools: [String]
    var path = ""

    enum CodingKeys: String, CodingKey {
        case id, name, version, description, author, permissions, files, bytes, installedVersion, problem, controllerTypes, tools
    }
}

/// `plugin_info`'s parts the details show.
struct StudioPluginDetails: Decodable, Equatable {
    let id: String
    let permissionText: [String]

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        permissionText = (try? c.decode([String].self, forKey: .permissionText)) ?? []
    }

    enum CodingKeys: String, CodingKey { case id, permissionText }
}

/// The install confirmation: what the plugin is, what it adds and what it says it can do, before anything is copied.
private struct StudioPluginInstallSheet: View {
    let package: StudioPluginPackage
    let install: () -> Void
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(alignment: .leading, spacing: StudioMetrics.spacingM) {
            HStack(alignment: .top, spacing: StudioMetrics.spacingM) {
                Image(systemName: "puzzlepiece.extension.fill")
                    .font(.largeTitle)
                    .foregroundStyle(.tint)
                VStack(alignment: .leading, spacing: 2) {
                    Text(verbatim: "Install “\(package.name)” \(package.version)?")
                        .font(.headline)
                    Text([package.author.map { "by \($0)" }, package.id].compactMap { $0 }.joined(separator: " · "))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    if let installed = package.installedVersion {
                        Text("Replaces version \(installed).")
                            .font(.caption)
                    }
                }
            }
            if let text = package.description, !text.isEmpty {
                Text(text)
                    .fixedSize(horizontal: false, vertical: true)
            }
            let adds = package.controllerTypes.map { "\($0.name) controllers" } + package.tools.map { "the agent tool \($0)" }
            if !adds.isEmpty {
                Text("Adds " + adds.formatted(.list(type: .and)) + ".")
                    .fixedSize(horizontal: false, vertical: true)
            }
            VStack(alignment: .leading, spacing: StudioMetrics.spacingXS) {
                Text("This plugin can:")
                    .font(.subheadline.weight(.semibold))
                if package.permissions.isEmpty {
                    Text("Nothing beyond running and showing its settings.")
                        .foregroundStyle(.secondary)
                }
                ForEach(package.permissions, id: \.self) { line in
                    Label(line, systemImage: "checkmark.circle")
                }
            }
            Text("It runs as you, like any app you install. Install plugins only from people you trust.")
                .font(.callout)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            if let problem = package.problem {
                Label(problem, systemImage: "exclamationmark.triangle.fill")
                    .symbolRenderingMode(.multicolor)
                    .fixedSize(horizontal: false, vertical: true)
            }
            HStack {
                Text("\(package.files) files, \(ByteCountFormatter.string(fromByteCount: Int64(package.bytes), countStyle: .file))")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Install") { install() }
                    .keyboardShortcut(.defaultAction)
                    .disabled(package.problem != nil)
            }
        }
        .padding(StudioMetrics.spacingXL)
        .frame(width: 440)
    }
}

/// A plugin's log: its messages, stderr and the host's notes, newest at the bottom, following as it grows.
private struct StudioPluginLogSheet: View {
    let plugin: StudioPlugin
    @Environment(\.dismiss) private var dismiss
    @State private var level = "info"
    @State private var entries: [Entry] = []
    @State private var logFile = ""

    struct Entry: Decodable, Identifiable, Equatable {
        let seq: Int
        let timeMs: Int
        let level: String
        let source: String
        let message: String
        var id: Int { seq }
    }

    private struct Answer: Decodable {
        let entries: [Entry]
        let logFile: String
    }

    var body: some View {
        VStack(alignment: .leading, spacing: StudioMetrics.spacingS) {
            HStack {
                Text("\(plugin.title) Log")
                    .font(.headline)
                Spacer()
                Picker("Show", selection: $level) {
                    Text("Everything").tag("debug")
                    Text("Info").tag("info")
                    Text("Warnings").tag("warn")
                    Text("Errors").tag("error")
                }
                .fixedSize()
            }
            ScrollViewReader { proxy in
                List(entries) { entry in
                    HStack(alignment: .firstTextBaseline, spacing: StudioMetrics.spacingS) {
                        Text(Date(timeIntervalSince1970: Double(entry.timeMs) / 1000), format: .dateTime.hour().minute().second())
                            .foregroundStyle(.secondary)
                        Image(systemName: symbol(entry.level))
                            .foregroundStyle(entry.level == "error" ? .red : entry.level == "warn" ? .orange : .secondary)
                        Text(entry.message)
                            .textSelection(.enabled)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                    .font(.system(.caption, design: .monospaced))
                    .id(entry.seq)
                }
                .overlay {
                    if entries.isEmpty {
                        ContentUnavailableView("Nothing Logged", systemImage: "text.alignleft", description: Text("The plugin hasn't written anything at this level yet."))
                    }
                }
                .onChange(of: entries.last?.seq) { _, last in
                    if let last { proxy.scrollTo(last, anchor: .bottom) }
                }
            }
            HStack {
                Button("Show Log File in Finder") { NSWorkspace.shared.activateFileViewerSelecting([URL(filePath: logFile)]) }
                    .disabled(logFile.isEmpty || !FileManager.default.fileExists(atPath: logFile))
                Spacer()
                Button("Done") { dismiss() }
                    .keyboardShortcut(.defaultAction)
            }
        }
        .padding(StudioMetrics.spacingL)
        .frame(minWidth: 560, idealWidth: 680, minHeight: 360, idealHeight: 460)
        .task(id: level) {
            while !Task.isCancelled {
                let id = plugin.id, level = level
                let text = await Task.detached { try? studioPluginLogs(id: id, limit: 500, level: level) }.value
                if let text, let answer = try? JSONDecoder().decode(Answer.self, from: Data(text.utf8)) {
                    if answer.entries != entries { entries = answer.entries }
                    logFile = answer.logFile
                }
                try? await Task.sleep(for: .seconds(1))
            }
        }
    }

    private func symbol(_ level: String) -> String {
        switch level {
        case "error": "xmark.octagon"
        case "warn": "exclamationmark.triangle"
        case "debug": "ant"
        default: "info.circle"
        }
    }
}
