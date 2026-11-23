import SwiftUI
import AppKit
import VixenBridge
import VixenKit

/// The controller inspector for one plugin controller (Display ▸ Controllers): the plugin's `configSchema` as a native
/// form (`studio_plugins::ui::form` in Rust decides the fields), checked as it's typed, with the plugin's own refusals
/// shown under the field they name. Each finished edit is `controller_configure {id, config}`, the command agents call,
/// on the working copy, so it publishes as one step on the display's Undo. It never grows past its column: a table too
/// wide for it becomes one card per row. A plugin that sets its own output count (`outputCount: "fromConfig"`) has the
/// controller resized in the same edit; the count shows read-only, and an edit that would drop patched outputs waits
/// for Remove Outputs. The plugin's settings actions (Detect Lights) are buttons under the header.
struct StudioPluginControllerInspector: View {
    let model: SetupModel

    @State private var info: PluginInfo?
    /// The engine's status for the controller: sending, the plugin's own state, send errors.
    @State private var status: StudioControllerLiveStatus?
    @State private var form: [FormField] = []
    @State private var formSchema = ""
    /// The configuration being edited: the controller's, plus edits not applied yet because they don't check out.
    @State private var draft: JSONValue = .object([:])
    @State private var draftErrors: [String: String] = [:]
    @State private var failure: String?
    /// Table rows added and not yet edited or left (`/devices/2`): their missing required values aren't errors yet.
    @State private var freshRows: Set<String> = []
    /// Settings that would drop patched outputs, waiting for Remove Outputs: the configuration and what it removes.
    @State private var pendingRemoval: (config: String, message: String)?
    /// The settings action running now, and what the last one said.
    @State private var runningAction: String?
    @State private var actionMessage: String?
    /// Display's columns stay alive, invisible, under the Controllers section; the harness then edits the section's copy.
    @Environment(\.studioCoveredBySection) private var covered
    @State private var coveredNow = StudioPluginFormHarness.Covered()
    @Environment(\.openSettings) private var openSettings

    var body: some View {
        VStack(alignment: .leading, spacing: StudioMetrics.spacingS) {
            if let info {
                content(info)
            } else {
                Color.clear.frame(height: 1)
            }
        }
        .onChange(of: covered, initial: true) { _, now in coveredNow.value = now }
        .task(id: model.controllerPanel) {
            let token = UUID()
            installHarnessHook(token)
            defer { StudioPluginFormHarness.handlers[token] = nil }
            while !Task.isCancelled {
                await refresh()
                try? await Task.sleep(for: .seconds(1))
            }
        }
    }

    @ViewBuilder
    private func content(_ info: PluginInfo) -> some View {
        let plugin = info.plugin
        HStack(alignment: .firstTextBaseline) {
            Label(plugin.pluginName ?? plugin.pluginId ?? "Plugin", systemImage: "puzzlepiece.extension")
                .font(.callout.weight(.semibold))
                .lineLimit(1)
            Spacer(minLength: StudioMetrics.spacingXS)
            Text([plugin.installedVersion ?? plugin.pluginVersion, plugin.frameEncoding].compactMap { $0 }.joined(separator: " · "))
                .font(.caption)
                .foregroundStyle(.secondary)
                .lineLimit(1)
            if plugin.configSchema != nil {
                Menu {
                    Button("Copy Settings") { copySettings() }
                    Button("Paste Settings") { pasteSettings() }
                    if let actions = plugin.actions, !actions.isEmpty {
                        Divider()
                        ForEach(actions, id: \.id) { action in
                            Button(action.title) { runAction(action) }
                                .disabled(runningAction != nil || plugin.problem != nil)
                        }
                    }
                } label: {
                    Image(systemName: "ellipsis.circle")
                }
                .menuStyle(.borderlessButton)
                .menuIndicator(.hidden)
                .fixedSize()
                .help("Copy this controller's settings as JSON, or paste settings from another show or a plugin's docs")
                .accessibilityLabel("Settings actions")
            }
        }
        if plugin.outputsFromConfig == true {
            Label("\(info.outputs) \(info.outputs == 1 ? "output" : "outputs"), set by \(plugin.pluginName ?? "the plugin") from these settings", systemImage: "number")
                .font(.caption)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
                .help("This plugin works out how many outputs the controller needs from its settings and names them; change the settings to change the outputs")
                .accessibilityIdentifier("studio.pluginController.outputs")
        }
        if let actions = plugin.actions, !actions.isEmpty {
            HStack(spacing: StudioMetrics.spacingXS) {
                ForEach(actions, id: \.id) { action in
                    Button {
                        runAction(action)
                    } label: {
                        Text(action.title).lineLimit(1)
                    }
                    .controlSize(.small)
                    .disabled(runningAction != nil || plugin.problem != nil)
                    .help(action.description ?? action.title)
                    .accessibilityIdentifier("studio.pluginController.action.\(action.id)")
                }
                if runningAction != nil {
                    ProgressView().controlSize(.small)
                }
            }
            if let actionMessage {
                Text(actionMessage)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                    .textSelection(.enabled)
            }
        }
        if let pending = pendingRemoval {
            VStack(alignment: .leading, spacing: StudioMetrics.spacingXS) {
                Label(pending.message, systemImage: "exclamationmark.triangle.fill")
                    .symbolRenderingMode(.multicolor)
                    .font(.caption)
                    .fixedSize(horizontal: false, vertical: true)
                HStack(spacing: StudioMetrics.spacingXS) {
                    Button("Remove Outputs") {
                        pendingRemoval = nil
                        apply(pending.config, to: info.id, removePatched: true)
                    }
                    .accessibilityIdentifier("studio.pluginController.removeOutputs")
                    Button("Keep Current Settings") {
                        pendingRemoval = nil
                        Task { await refresh(reset: true) }
                    }
                }
                .controlSize(.small)
            }
        }
        if plugin.problem == nil, let status {
            Group {
                if let problem = status.current {
                    StudioControllerProblemLabel(problem: problem)
                } else {
                    Text(status.text)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            .accessibilityIdentifier("studio.pluginController.status")
        }
        if let problem = plugin.problem {
            VStack(alignment: .leading, spacing: StudioMetrics.spacingXS) {
                Label(problem, systemImage: "exclamationmark.triangle.fill")
                    .symbolRenderingMode(.multicolor)
                    .font(.callout)
                    .fixedSize(horizontal: false, vertical: true)
                Text("The controller and its patches stay; it sends nothing until the plugin is ready.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                Button("Manage Plugins…") {
                    UserDefaults.standard.set(StudioSettingsTab.plugins, forKey: StudioSettingsTab.storageKey)
                    openSettings()
                }
                .controlSize(.small)
            }
        }
        if plugin.configSchema != nil {
            if form.isEmpty {
                Text("This controller has no settings.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            ForEach(form) { field in
                fieldView(field, pointer: field.pointer)
            }
        }
        if let failure {
            Label(failure, systemImage: "exclamationmark.triangle.fill")
                .symbolRenderingMode(.multicolor)
                .font(.caption)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    // MARK: Fields

    private func errors(at pointer: String) -> [String] {
        // A new row's missing values wait until the row has been edited or left.
        if freshRows.contains(where: { pointer.hasPrefix($0 + "/") }) { return [] }
        var out: [String] = []
        if let text = draftErrors[pointer] { out.append(text) }
        for error in info?.plugin.configErrors ?? [] where error.field == pointer || (pointer.isEmpty && error.field == "/") {
            out.append("\(info?.plugin.pluginName ?? "The plugin") says it \(error.message)")
        }
        return out
    }

    @ViewBuilder
    private func fieldView(_ field: FormField, pointer: String) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            switch field.kind {
            case "boolean":
                Toggle(field.label, isOn: Binding(
                    get: { draft[pointer]?.bool ?? field.defaultValue?.bool ?? false },
                    set: { set(pointer, .bool($0)) }))
                    .help(field.help ?? "")
            case "choice":
                Picker(field.label, selection: Binding(
                    get: { draft[pointer] ?? field.defaultValue ?? .null },
                    set: { set(pointer, $0) })) {
                    if draft[pointer] == nil && field.defaultValue == nil {
                        Text("Choose…").tag(JSONValue.null)
                    }
                    ForEach(field.options ?? [], id: \.value) { option in
                        Text(option.label).tag(option.value)
                    }
                }
                .help(field.help ?? "")
            case "string", "number", "integer":
                LabeledContent(field.label) {
                    StudioCommitField(text: draft[pointer].map(text(of:)) ?? "", prompt: field.prompt) { typed in
                        commitText(typed, field: field, pointer: pointer)
                    }
                    .frame(maxWidth: field.kind == "string" ? .infinity : 110, alignment: .leading)
                    .accessibilityLabel(field.label)
                }
                .help(field.help ?? "")
            case "object":
                DisclosureGroup(field.label) {
                    VStack(alignment: .leading, spacing: StudioMetrics.spacingS) {
                        ForEach(field.fields ?? []) { child in
                            AnyView(fieldView(child, pointer: child.pointer))
                        }
                    }
                    .padding(.leading, StudioMetrics.spacingS)
                }
            case "table":
                tableView(field, pointer: pointer)
            case "list":
                listView(field, pointer: pointer)
            default:
                jsonView(field, pointer: pointer)
            }
            if let help = field.help, field.kind != "boolean", field.kind != "choice" {
                Text(help)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            ForEach(errors(at: pointer), id: \.self) { text in
                Label(text, systemImage: "exclamationmark.circle.fill")
                    .symbolRenderingMode(.multicolor)
                    .font(.caption)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .font(.callout)
    }

    /// An array of objects: one row per item, a column per property, Add Row and a remove button per row. When the
    /// columns don't fit the inspector, each row becomes a card with its labels above their values.
    @ViewBuilder
    private func tableView(_ field: FormField, pointer: String) -> some View {
        let rows = draft[pointer]?.array ?? []
        VStack(alignment: .leading, spacing: StudioMetrics.spacingXS) {
            Text(field.label)
                .foregroundStyle(.secondary)
            if !rows.isEmpty {
                ViewThatFits(in: .horizontal) {
                    tableGrid(field, pointer: pointer, rows: rows)
                    tableCards(field, pointer: pointer, rows: rows)
                }
            }
            if let waiting = rows.indices.first(where: { freshRows.contains("\(pointer)/\($0)") && hasHiddenErrors(in: "\(pointer)/\($0)") }) {
                Text("Row \(waiting + 1) needs its values before your changes apply.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Button("Add Row") { addRow(field, rows: rows) }
                .controlSize(.small)
                .disabled(field.maxItems.map { rows.count >= $0 } ?? false)
        }
    }

    private func addRow(_ field: FormField, rows: [JSONValue]) {
        freshRows.insert("\(field.pointer)/\(rows.count)")
        set(field.pointer, .array(rows + [field.newRow ?? .object([:])]))
    }

    private func tableGrid(_ field: FormField, pointer: String, rows: [JSONValue]) -> some View {
        let columns = field.columns ?? []
        return Grid(alignment: .leading, horizontalSpacing: StudioMetrics.spacingXS, verticalSpacing: StudioMetrics.spacingXS) {
            GridRow {
                ForEach(columns) {
                    Text($0.label).font(.caption).foregroundStyle(.secondary).lineLimit(2).fixedSize(horizontal: false, vertical: true)
                }
                Color.clear.frame(width: 1, height: 1)
            }
            ForEach(rows.indices, id: \.self) { index in
                GridRow {
                    ForEach(columns) { column in
                        cell(column, pointer: "\(pointer)/\(index)\(column.pointer)")
                    }
                    removeRowButton(field, pointer: pointer, rows: rows, index: index)
                }
                ForEach(columns) { column in
                    ForEach(errors(at: "\(pointer)/\(index)\(column.pointer)"), id: \.self) { text in
                        Text("Row \(index + 1) \(column.label): \(text)")
                            .font(.caption)
                            .foregroundStyle(.red)
                            .fixedSize(horizontal: false, vertical: true)
                            .gridCellColumns(columns.count + 1)
                    }
                }
            }
        }
    }

    private func tableCards(_ field: FormField, pointer: String, rows: [JSONValue]) -> some View {
        let columns = field.columns ?? []
        return VStack(alignment: .leading, spacing: StudioMetrics.spacingXS) {
            ForEach(rows.indices, id: \.self) { index in
                VStack(alignment: .leading, spacing: StudioMetrics.spacingXS) {
                    HStack {
                        Text("Row \(index + 1)").font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                        Spacer(minLength: 0)
                        removeRowButton(field, pointer: pointer, rows: rows, index: index)
                    }
                    ForEach(columns) { column in
                        let at = "\(pointer)/\(index)\(column.pointer)"
                        VStack(alignment: .leading, spacing: 2) {
                            if column.kind == "boolean" {
                                Toggle(column.label, isOn: Binding(get: { draft[at]?.bool ?? column.defaultValue?.bool ?? false }, set: { set(at, .bool($0)) }))
                            } else {
                                Text(column.label).font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                                cell(column, pointer: at, compact: true)
                            }
                            ForEach(errors(at: at), id: \.self) { text in
                                Text(text)
                                    .font(.caption)
                                    .foregroundStyle(.red)
                                    .fixedSize(horizontal: false, vertical: true)
                            }
                        }
                    }
                }
                .padding(StudioMetrics.spacingS)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(.background.secondary, in: RoundedRectangle(cornerRadius: StudioMetrics.smallCornerRadius))
            }
        }
    }

    private func removeRowButton(_ field: FormField, pointer: String, rows: [JSONValue], index: Int) -> some View {
        Button {
            var items = rows
            items.remove(at: index)
            // Rows after the removed one move up a place, and keep whether they're new.
            freshRows = Set(freshRows.compactMap { row -> String? in
                guard row.hasPrefix(pointer + "/"), let n = Int(row.dropFirst(pointer.count + 1)) else { return row }
                return n == index ? nil : n > index ? "\(pointer)/\(n - 1)" : row
            })
            set(pointer, .array(items))
        } label: {
            Image(systemName: "minus.circle")
        }
        .buttonStyle(.borderless)
        .help("Remove this row")
        .accessibilityLabel("Remove row \(index + 1)")
        .disabled(rows.count <= (field.minItems ?? 0))
    }

    private func hasHiddenErrors(in row: String) -> Bool {
        draftErrors.keys.contains { $0.hasPrefix(row + "/") }
    }

    /// A new table row stops being new once one of its values is edited or left.
    private func touch(_ pointer: String) {
        freshRows = freshRows.filter { !pointer.hasPrefix($0 + "/") }
    }

    @ViewBuilder
    private func cell(_ column: FormField, pointer: String, compact: Bool = false) -> some View {
        switch column.kind {
        case "boolean":
            Toggle(column.label, isOn: Binding(get: { draft[pointer]?.bool ?? column.defaultValue?.bool ?? false }, set: { set(pointer, .bool($0)) }))
                .labelsHidden()
        case "choice":
            Picker(column.label, selection: Binding(get: { draft[pointer] ?? column.defaultValue ?? .null }, set: { set(pointer, $0) })) {
                if draft[pointer] == nil && column.defaultValue == nil { Text("—").tag(JSONValue.null) }
                ForEach(column.options ?? [], id: \.value) { Text($0.label).tag($0.value) }
            }
            .labelsHidden()
            .fixedSize(horizontal: !compact, vertical: false)
            .frame(maxWidth: compact ? .infinity : nil, alignment: .leading)
        default:
            StudioCommitField(text: draft[pointer].map(text(of:)) ?? "", prompt: column.prompt, leave: { touch(pointer) }) { typed in
                commitText(typed, field: column, pointer: pointer)
            }
            .frame(minWidth: compact ? 0 : 70, idealWidth: compact ? nil : 110, maxWidth: .infinity)
            .accessibilityLabel(column.label)
        }
    }

    /// An array of strings or numbers: one field per item, Add and a remove button per item.
    @ViewBuilder
    private func listView(_ field: FormField, pointer: String) -> some View {
        let items = draft[pointer]?.array ?? []
        let item = field.item ?? FormField.plain
        VStack(alignment: .leading, spacing: StudioMetrics.spacingXS) {
            Text(field.label).foregroundStyle(.secondary)
            ForEach(items.indices, id: \.self) { index in
                HStack(spacing: StudioMetrics.spacingXS) {
                    StudioCommitField(text: text(of: items[index]), prompt: item.prompt) { typed in
                        commitText(typed, field: item, pointer: "\(pointer)/\(index)")
                    }
                    Button {
                        var next = items
                        next.remove(at: index)
                        set(pointer, .array(next))
                    } label: { Image(systemName: "minus.circle") }
                    .buttonStyle(.borderless)
                    .disabled(items.count <= (field.minItems ?? 0))
                }
            }
            Button("Add") { set(pointer, .array(items + [item.kind == "string" ? .string("") : .number(0)])) }
                .controlSize(.small)
                .disabled(field.maxItems.map { items.count >= $0 } ?? false)
        }
    }

    /// Settings the form can't draw, as JSON text applied when it's committed.
    @ViewBuilder
    private func jsonView(_ field: FormField, pointer: String) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(field.label).foregroundStyle(.secondary)
            StudioCommitField(text: (pointer.isEmpty ? draft : draft[pointer])?.jsonText ?? "", prompt: "JSON", axis: .vertical) { typed in
                guard let value = JSONValue(jsonText: typed) else {
                    draftErrors[pointer] = "isn't valid JSON"
                    return
                }
                draftErrors[pointer] = nil
                set(pointer, value)
            }
            .font(.system(.caption, design: .monospaced))
        }
    }

    // MARK: Editing

    private func text(of value: JSONValue) -> String {
        switch value {
        case .string(let s): s
        case .number(let n): n.rounded() == n && abs(n) < 1e15 ? String(Int64(n)) : String(n)
        case .bool(let b): b ? "true" : "false"
        case .null: ""
        default: value.jsonText
        }
    }

    private func commitText(_ typed: String, field: FormField, pointer: String) {
        let trimmed = typed.trimmingCharacters(in: .whitespaces)
        if trimmed.isEmpty && !field.required && field.kind != "string" {
            set(pointer, nil)
            return
        }
        switch field.kind {
        case "number", "integer":
            guard let number = Double(trimmed), field.kind == "number" || number.rounded() == number else {
                draftErrors[pointer] = field.kind == "integer" ? "must be a whole number" : "must be a number"
                return
            }
            set(pointer, .number(number))
        default:
            set(pointer, trimmed.isEmpty && !field.required ? nil : .string(trimmed))
        }
    }

    /// Puts `value` at `pointer` (nil removes it), checks the whole configuration and applies it when it checks out;
    /// otherwise the problems show under their fields and the controller keeps its last good configuration.
    private func set(_ pointer: String, _ value: JSONValue?) {
        touch(pointer)
        var next = draft
        if pointer.isEmpty {
            next = value ?? .object([:])
        } else {
            next[pointer] = value
        }
        draft = next
        guard let info, let schema = info.plugin.configSchema?.jsonText else { return }
        let config = next.jsonText
        let problems = (try? JSONDecoder().decode([FieldError].self, from: Data(studioPluginCheckConfig(schema: schema, config: config).utf8))) ?? []
        draftErrors = Dictionary(problems.map { ($0.field == "/" ? "" : $0.field, $0.message) }, uniquingKeysWith: { a, _ in a })
        // A change that ends where the controller already is makes no Undo step.
        guard problems.isEmpty, next != (info.plugin.config ?? .object([:])) else { return }
        apply(config, to: info.id)
    }

    // MARK: Copy and paste

    /// The controller's settings as JSON, for another show or a plugin's issue tracker.
    private func copySettings() {
        let config = info?.plugin.config ?? .object([:])
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(config.prettyJSONText, forType: .string)
    }

    /// Settings from the clipboard replace the configuration through the same check as typing, so they apply as one
    /// step on Undo, or their problems show under the fields they name.
    private func pasteSettings(_ text: String? = nil) {
        let text = text ?? NSPasteboard.general.string(forType: .string) ?? ""
        guard let value = JSONValue(jsonText: text.trimmingCharacters(in: .whitespacesAndNewlines)), case .object = value else {
            failure = "The clipboard doesn't hold settings: copy a JSON object such as {\"host\": \"192.168.1.50\"}."
            return
        }
        failure = nil
        freshRows = []
        set("", value)
    }

    /// `removePatched` repeats an edit the user agreed may drop patched outputs (Remove Outputs).
    private func apply(_ config: String, to id: String, removePatched: Bool = false) {
        guard let session = model.session else { return }
        let arguments = "{\"id\":\(JSONValue.string(id).jsonText),\"config\":\(config)\(removePatched ? ",\"removePatchedOutputs\":true" : "")}"
        Task {
            let result = await offMain { Result { try session.studioConfigureControllerNetwork(argumentsJson: arguments) } }
            switch result {
            case .success(let json):
                pendingRemoval = nil
                // The plugin couldn't work out the outputs: the settings applied and the count stayed.
                failure = (JSONValue(jsonText: json)?["/warning"]).flatMap { $0.string }
                model.controllersChanged()
                await refresh(reset: true)
            case .failure(let error):
                // `studio_controller_network::REMOVES_PATCHED`: the edit waits for Remove Outputs, still on screen.
                if error.userMessage.hasPrefix("These settings remove ") {
                    // The agents' hint names an argument; the inspector has the button instead.
                    let message = error.userMessage.replacingOccurrences(of: "; pass removePatchedOutputs: true to apply anyway (Undo brings them back).", with: ". Undo brings them back.")
                    pendingRemoval = (config, message)
                    failure = nil
                } else {
                    failure = error.userMessage
                    await refresh(reset: true)
                }
            }
        }
    }

    /// Runs a settings action: the plugin's tool works out new settings (it may scan the network), which then apply like
    /// an edit, one step on Undo.
    private func runAction(_ action: PluginAction) {
        guard let session = model.session, let info, runningAction == nil else { return }
        runningAction = action.id
        actionMessage = nil
        failure = nil
        let id = info.id
        Task {
            let result = await offMain { Result { try session.studioControllerAction(id: id, action: action.id) } }
            runningAction = nil
            switch result {
            case .success(let json):
                let answer = JSONValue(jsonText: json)
                actionMessage = answer?["/message"]?.string
                if let config = answer?["/config"], case .object = config, config != (self.info?.plugin.config ?? .object([:])) {
                    draft = config
                    draftErrors = [:]
                    freshRows = []
                    apply(config.jsonText, to: id)
                } else if actionMessage == nil {
                    actionMessage = "\(action.title): nothing to change."
                }
            case .failure(let error):
                failure = "\(action.title): \(error.userMessage)"
            }
        }
    }

    // MARK: Reading

    private func refresh(reset: Bool = false) async {
        guard let session = model.session else { info = nil; return }
        let rows = model.controllerSelection
        let core = model.app.core
        let (json, statusJSON) = await offMain { () -> (String?, String?) in
            guard let id = session.firstControllerInfo(rows: rows)?.id else { return (nil, nil) }
            return (session.studioControllerNetwork(id: id), core?.studioControllerStatus(id: id))
        }
        guard let json, let next = try? JSONDecoder().decode(PluginInfo.self, from: Data(json.utf8)) else {
            info = nil
            return
        }
        let nextStatus = statusJSON.flatMap { try? JSONDecoder().decode(StudioControllerLiveStatus.self, from: Data($0.utf8)) }
        if nextStatus != status { status = nextStatus }
        let switched = next.id != info?.id
        let configChanged = next.plugin.config != info?.plugin.config
        info = next
        let schema = next.plugin.configSchema?.jsonText ?? ""
        if schema != formSchema {
            formSchema = schema
            let text = studioPluginConfigForm(schema: schema)
            form = (try? JSONDecoder().decode(FormDescription.self, from: Data(text.utf8)))?.fields ?? []
        }
        // Edits that don't check out stay on screen until the controller changes some other way (Undo, an agent).
        if (switched || reset || (configChanged && draftErrors.isEmpty)) && pendingRemoval == nil {
            draft = next.plugin.config ?? .object([:])
            if switched || configChanged { draftErrors = [:] }
            if switched { failure = nil; freshRows = []; actionMessage = nil }
        }
        if switched { pendingRemoval = nil }
    }

    // MARK: Harness

    /// `{ "action": "hook", "name": "studio.pluginForm", "path": [...] }`: `commit <pointer> <text>` hands a field's text
    /// over as Return would (a table cell too: `/devices/0/mac`), `toggle <pointer>` flips a switch, `addRow <pointer>`
    /// presses a table's Add Row, `leave <pointer>` moves focus out of a cell, `copy` and `paste <json>` run Copy and
    /// Paste Settings, `action <id>` presses a settings action, `removeOutputs` / `keepSettings` answer the patched-outputs
    /// warning, `state` answers the configuration shown and its problems (`errors`, every one; `shown`, those on
    /// screen), plus `outputs`, `pending` (the warning) and `message` (the last action's).
    private func installHarnessHook(_ token: UUID) {
        guard UITestHarness.isActive else { return }
        StudioPluginFormHarness.install()
        StudioPluginFormHarness.handlers[token] = { args in
            func find(_ pointer: String, in fields: [FormField]) -> FormField? {
                for field in fields {
                    if field.pointer == pointer { return field }
                    if let inner = find(pointer, in: field.fields ?? []) { return inner }
                }
                return nil
            }
            guard info != nil, !coveredNow.value else { return nil }
            switch args.first ?? "" {
            case "commit":
                guard args.count >= 2, let (field, pointer) = Self.locate(args[1], in: form) else { return "FAIL no field \(args.dropFirst().first ?? "")" }
                commitText(args.dropFirst(2).joined(separator: " "), field: field, pointer: pointer)
            case "toggle":
                guard args.count >= 2 else { return "FAIL toggle <pointer>" }
                set(args[1], .bool(!(draft[args[1]]?.bool ?? false)))
            case "addRow":
                guard args.count >= 2, let field = find(args[1], in: form), field.kind == "table" else { return "FAIL no table \(args.dropFirst().first ?? "")" }
                addRow(field, rows: draft[field.pointer]?.array ?? [])
            case "leave":
                guard args.count >= 2 else { return "FAIL leave <pointer>" }
                touch(args[1])
            case "copy":
                // Puts back what the pasteboard held, so a run's earlier copies stay checkable.
                let before = NSPasteboard.general.string(forType: .string)
                copySettings()
                let copied = NSPasteboard.general.string(forType: .string) ?? ""
                if let before {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(before, forType: .string)
                }
                return "copied=\(copied)"
            case "paste":
                pasteSettings(args.dropFirst().joined(separator: " "))
            case "action":
                guard args.count >= 2, let action = info?.plugin.actions?.first(where: { $0.id == args[1] }) else { return "FAIL no action \(args.dropFirst().first ?? "")" }
                runAction(action)
                for _ in 0..<300 where runningAction != nil { try? await Task.sleep(for: .milliseconds(100)) }
            case "removeOutputs":
                guard let pending = pendingRemoval, let id = info?.id else { return "FAIL nothing pending" }
                pendingRemoval = nil
                apply(pending.config, to: id, removePatched: true)
            case "keepSettings":
                pendingRemoval = nil
                await refresh(reset: true)
            case "state": break
            default: return "FAIL unknown \(args.first ?? "")"
            }
            try? await Task.sleep(for: .milliseconds(1_500))
            let problems = draftErrors.sorted { $0.key < $1.key }.map { "\($0.key) \($0.value)" }.joined(separator: "; ")
            let shown = draftErrors.keys.sorted().filter { !errors(at: $0).isEmpty }.joined(separator: ",")
            return "config=\(info?.plugin.config?.jsonText ?? "nil") draft=\(draft.jsonText) errors=[\(problems)] shown=[\(shown)] failure=\(failure ?? "") outputs=\(info?.outputs ?? -1) pending=\(pendingRemoval?.message ?? "") message=\(actionMessage ?? "")"
        }
    }

    /// A form field by pointer, including a table cell (`/devices/0/mac` is the `devices` table's `mac` column).
    private static func locate(_ pointer: String, in fields: [FormField]) -> (FormField, String)? {
        for field in fields {
            if field.pointer == pointer { return (field, pointer) }
            if let inner = locate(pointer, in: field.fields ?? []) { return inner }
            if field.kind == "table", pointer.hasPrefix(field.pointer + "/") {
                let rest = pointer.dropFirst(field.pointer.count + 1)
                if let slash = rest.firstIndex(of: "/"), Int(rest[..<slash]) != nil,
                   let column = (field.columns ?? []).first(where: { $0.pointer == String(rest[slash...]) }) {
                    return (column, pointer)
                }
            }
        }
        return nil
    }

    // MARK: Rust's JSON

    struct PluginInfo: Decodable, Equatable {
        let id: String
        let outputs: Int
        let plugin: Plugin
    }

    struct Plugin: Decodable, Equatable {
        let pluginId: String?
        let pluginName: String?
        let pluginVersion: String?
        let installedVersion: String?
        let config: JSONValue?
        let configSchema: JSONValue?
        let frameEncoding: String?
        let problem: String?
        let configErrors: [FieldError]?
        let outputsFromConfig: Bool?
        let actions: [PluginAction]?
    }

    struct PluginAction: Decodable, Equatable {
        let id: String
        let title: String
        let description: String?
    }

    struct FieldError: Decodable, Equatable, Hashable {
        let field: String
        let message: String
    }
}

/// `studio_plugins::ui::form`.
struct FormDescription: Decodable {
    let fields: [FormField]
}

struct FormField: Decodable, Identifiable {
    struct Option: Decodable { let value: JSONValue; let label: String }
    let key: String
    let pointer: String
    let label: String
    let help: String?
    let kind: String
    let required: Bool
    let defaultValue: JSONValue?
    let format: String?
    let minimum: Double?
    let maximum: Double?
    let options: [Option]?
    let fields: [FormField]?
    let columns: [FormField]?
    let newRow: JSONValue?
    /// A list's item, boxed in an array since a struct can't hold itself.
    private let itemBox: [FormField]?
    var item: FormField? { itemBox?.first }
    let minItems: Int?
    let maxItems: Int?
    var id: String { pointer.isEmpty ? key : pointer }

    enum CodingKeys: String, CodingKey {
        case key, pointer, label, help, kind, required, format, minimum, maximum, options, fields, columns, newRow, minItems, maxItems
        case defaultValue = "default", itemBox = "item"
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        key = try c.decode(String.self, forKey: .key)
        pointer = try c.decode(String.self, forKey: .pointer)
        label = try c.decode(String.self, forKey: .label)
        help = try c.decodeIfPresent(String.self, forKey: .help)
        kind = try c.decode(String.self, forKey: .kind)
        required = try c.decodeIfPresent(Bool.self, forKey: .required) ?? false
        defaultValue = try c.decodeIfPresent(JSONValue.self, forKey: .defaultValue)
        format = try c.decodeIfPresent(String.self, forKey: .format)
        minimum = try c.decodeIfPresent(Double.self, forKey: .minimum)
        maximum = try c.decodeIfPresent(Double.self, forKey: .maximum)
        options = try c.decodeIfPresent([Option].self, forKey: .options)
        fields = try c.decodeIfPresent([FormField].self, forKey: .fields)
        columns = try c.decodeIfPresent([FormField].self, forKey: .columns)
        newRow = try c.decodeIfPresent(JSONValue.self, forKey: .newRow)
        itemBox = try c.decodeIfPresent(FormField.self, forKey: .itemBox).map { [$0] }
        minItems = try c.decodeIfPresent(Int.self, forKey: .minItems)
        maxItems = try c.decodeIfPresent(Int.self, forKey: .maxItems)
    }

    private init(kind: String) {
        key = ""; pointer = ""; label = ""; help = nil; self.kind = kind; required = true; defaultValue = nil; format = nil
        minimum = nil; maximum = nil; options = nil; fields = nil; columns = nil; newRow = nil; itemBox = nil; minItems = nil; maxItems = nil
    }

    static let plain = FormField(kind: "string")

    /// The placeholder: the default, the format's example, or the range.
    var prompt: String {
        if let defaultValue, defaultValue != .null {
            if case .string(let s) = defaultValue { return s }
            return defaultValue.jsonText
        }
        switch format {
        case "ipv4": return "192.168.1.50"
        case "hostname": return "device.local"
        default: break
        }
        switch (minimum, maximum) {
        case let (low?, high?): return "\(Self.number(low))–\(Self.number(high))"
        case let (low?, nil): return "≥ \(Self.number(low))"
        case let (nil, high?): return "≤ \(Self.number(high))"
        default: return required ? "Required" : ""
        }
    }

    private static func number(_ n: Double) -> String {
        n.rounded() == n ? String(Int64(n)) : String(n)
    }
}

/// A text field that hands its text over when Return is pressed or focus leaves it, and follows `text` otherwise.
struct StudioCommitField: View {
    let text: String
    let prompt: String
    var axis: Axis = .horizontal
    /// Called when focus leaves the field, whether or not its text changed.
    var leave: (() -> Void)? = nil
    let commit: (String) -> Void
    @State private var typed = ""
    @FocusState private var focused: Bool

    var body: some View {
        TextField(prompt, text: $typed, axis: axis)
            .textFieldStyle(.roundedBorder)
            .focused($focused)
            .onSubmit { if typed != text { commit(typed) } }
            .onChange(of: focused) { _, now in
                if !now, typed != text { commit(typed) }
                if !now { leave?() }
            }
            .onChange(of: text, initial: true) { _, new in
                if !focused { typed = new }
            }
    }
}

/// A JSON value the inspector edits, addressed by JSON Pointer.
enum JSONValue: Codable, Hashable, Sendable {
    case null, bool(Bool), number(Double), string(String), array([JSONValue]), object([String: JSONValue])

    init(from decoder: Decoder) throws {
        let c = try decoder.singleValueContainer()
        if c.decodeNil() { self = .null }
        else if let b = try? c.decode(Bool.self) { self = .bool(b) }
        else if let n = try? c.decode(Double.self) { self = .number(n) }
        else if let s = try? c.decode(String.self) { self = .string(s) }
        else if let a = try? c.decode([JSONValue].self) { self = .array(a) }
        else { self = .object(try c.decode([String: JSONValue].self)) }
    }

    func encode(to encoder: Encoder) throws {
        var c = encoder.singleValueContainer()
        switch self {
        case .null: try c.encodeNil()
        case .bool(let b): try c.encode(b)
        case .number(let n): try c.encode(n)
        case .string(let s): try c.encode(s)
        case .array(let a): try c.encode(a)
        case .object(let o): try c.encode(o)
        }
    }

    init?(jsonText: String) {
        guard let value = try? JSONDecoder().decode(JSONValue.self, from: Data(jsonText.utf8)) else { return nil }
        self = value
    }

    var jsonText: String {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
        return (try? encoder.encode(self)).flatMap { String(data: $0, encoding: .utf8) } ?? "null"
    }

    var prettyJSONText: String {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes, .prettyPrinted]
        return (try? encoder.encode(self)).flatMap { String(data: $0, encoding: .utf8) } ?? "null"
    }

    var bool: Bool? { if case .bool(let b) = self { b } else { nil } }
    var string: String? { if case .string(let t) = self { t } else { nil } }
    var array: [JSONValue]? { if case .array(let a) = self { a } else { nil } }

    private static func tokens(_ pointer: String) -> [String] {
        pointer.split(separator: "/", omittingEmptySubsequences: false).dropFirst()
            .map { $0.replacingOccurrences(of: "~1", with: "/").replacingOccurrences(of: "~0", with: "~") }
    }

    subscript(pointer: String) -> JSONValue? {
        get {
            var current: JSONValue? = self
            for token in Self.tokens(pointer) {
                switch current {
                case .object(let o)?: current = o[token]
                case .array(let a)?: current = Int(token).flatMap { a.indices.contains($0) ? a[$0] : nil }
                default: return nil
                }
            }
            return current
        }
        set {
            self = Self.setting(self, Self.tokens(pointer)[...], newValue)
        }
    }

    private static func setting(_ base: JSONValue, _ tokens: ArraySlice<String>, _ value: JSONValue?) -> JSONValue {
        guard let token = tokens.first else { return value ?? .null }
        let rest = tokens.dropFirst()
        switch base {
        case .array(var a):
            guard let index = Int(token), a.indices.contains(index) else { return base }
            if rest.isEmpty, value == nil { a.remove(at: index) } else { a[index] = setting(a[index], rest, value) }
            return .array(a)
        case .object(var o):
            if rest.isEmpty, value == nil { o[token] = nil } else { o[token] = setting(o[token] ?? .object([:]), rest, value) }
            return .object(o)
        default:
            return rest.isEmpty && value == nil ? base : .object([token: setting(.object([:]), rest, value)])
        }
    }
}

/// The `studio.pluginForm` hook. Display and Controllers each keep a plugin inspector, so it acts in the one on screen
/// showing a controller, as a person editing one of them would.
enum StudioPluginFormHarness {
    /// Whether an inspector is under the Controllers section, readable from its hook.
    final class Covered { var value = false }

    static var handlers: [UUID: ([String]) async -> String?] = [:] {
        didSet { order = order.filter { handlers[$0] != nil } + handlers.keys.filter { !order.contains($0) } }
    }
    private static var order: [UUID] = []

    static func install() {
        guard UITestHarness.hooks["studio.pluginForm"] == nil else { return }
        UITestHarness.hooks["studio.pluginForm"] = { args in
            for token in order {
                if let handler = handlers[token], let answer = await handler(args) { return answer }
            }
            return "FAIL no plugin inspector"
        }
    }
}

extension SetupModel {
    /// Whether the selected controller belongs to a plugin, so the plugin inspector edits it.
    func studioSelectedPluginController() async -> Bool {
        guard let session else { return false }
        let rows = controllerSelection
        return await offMain {
            guard let id = session.firstControllerInfo(rows: rows)?.id, let json = session.studioControllerNetwork(id: id),
                  let object = try? JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any] else { return false }
            return object["plugin"] is [String: Any]
        }
    }

    /// Whether the selected plugin controller's plugin sets its output count from the settings (no Outputs… button).
    func studioSelectedPluginOwnsOutputs() async -> Bool {
        guard let session else { return false }
        let rows = controllerSelection
        return await offMain {
            guard let id = session.firstControllerInfo(rows: rows)?.id, let json = session.studioControllerNetwork(id: id),
                  let object = try? JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any],
                  let plugin = object["plugin"] as? [String: Any] else { return false }
            return plugin["outputsFromConfig"] as? Bool == true
        }
    }
}

private struct StudioCoveredBySectionKey: EnvironmentKey {
    static let defaultValue = false
}

extension EnvironmentValues {
    /// Display Setup's columns are hidden under the show window's Controllers section.
    var studioCoveredBySection: Bool {
        get { self[StudioCoveredBySectionKey.self] }
        set { self[StudioCoveredBySectionKey.self] = newValue }
    }
}
