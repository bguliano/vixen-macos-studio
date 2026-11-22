import SwiftUI
import VixenBridge
import VixenKit

/// The controller inspector for one plugin controller (Display ▸ Controllers): the plugin's `configSchema` as a native
/// form (`studio_plugins::ui::form` in Rust decides the fields), checked as it's typed, with the plugin's own refusals
/// shown under the field they name. Each finished edit is `controller_configure {id, config}`, the command agents call,
/// on the working copy, so it publishes as one step on the display's Undo.
struct StudioPluginControllerInspector: View {
    let model: SetupModel

    @State private var info: PluginInfo?
    @State private var form: [FormField] = []
    @State private var formSchema = ""
    /// The configuration being edited: the controller's, plus edits not applied yet because they don't check out.
    @State private var draft: JSONValue = .object([:])
    @State private var draftErrors: [String: String] = [:]
    @State private var failure: String?
    @Environment(\.openSettings) private var openSettings

    var body: some View {
        VStack(alignment: .leading, spacing: StudioMetrics.spacingS) {
            if let info {
                content(info)
            } else {
                Color.clear.frame(height: 1)
            }
        }
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
            Spacer()
            Text([plugin.installedVersion ?? plugin.pluginVersion, plugin.frameEncoding].compactMap { $0 }.joined(separator: " · "))
                .font(.caption)
                .foregroundStyle(.secondary)
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

    /// An array of objects: one row per item, a column per property, Add Row and a remove button per row.
    @ViewBuilder
    private func tableView(_ field: FormField, pointer: String) -> some View {
        let rows = draft[pointer]?.array ?? []
        let columns = field.columns ?? []
        VStack(alignment: .leading, spacing: StudioMetrics.spacingXS) {
            Text(field.label)
                .foregroundStyle(.secondary)
            Grid(alignment: .leading, horizontalSpacing: StudioMetrics.spacingXS, verticalSpacing: StudioMetrics.spacingXS) {
                GridRow {
                    ForEach(columns) { Text($0.label).font(.caption).foregroundStyle(.secondary) }
                    Color.clear.frame(width: 1, height: 1)
                }
                ForEach(rows.indices, id: \.self) { index in
                    GridRow {
                        ForEach(columns) { column in
                            cell(column, pointer: "\(pointer)/\(index)\(column.pointer)")
                        }
                        Button {
                            var items = rows
                            items.remove(at: index)
                            set(pointer, .array(items))
                        } label: {
                            Image(systemName: "minus.circle")
                        }
                        .buttonStyle(.borderless)
                        .help("Remove this row")
                        .disabled(rows.count <= (field.minItems ?? 0))
                    }
                    ForEach(columns) { column in
                        ForEach(errors(at: "\(pointer)/\(index)\(column.pointer)"), id: \.self) { text in
                            Text("Row \(index + 1) \(column.label): \(text)")
                                .font(.caption)
                                .foregroundStyle(.red)
                                .gridCellColumns(columns.count + 1)
                        }
                    }
                }
            }
            Button("Add Row") { set(pointer, .array(rows + [field.newRow ?? .object([:])])) }
                .controlSize(.small)
                .disabled(field.maxItems.map { rows.count >= $0 } ?? false)
        }
    }

    @ViewBuilder
    private func cell(_ column: FormField, pointer: String) -> some View {
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
            .fixedSize()
        default:
            StudioCommitField(text: draft[pointer].map(text(of:)) ?? "", prompt: column.prompt) { typed in
                commitText(typed, field: column, pointer: pointer)
            }
            .frame(minWidth: 70)
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

    private func apply(_ config: String, to id: String) {
        guard let session = model.session else { return }
        let arguments = "{\"id\":\(JSONValue.string(id).jsonText),\"config\":\(config)}"
        Task {
            let result = await offMain { Result { try session.studioConfigureControllerNetwork(argumentsJson: arguments) } }
            switch result {
            case .success:
                failure = nil
                model.controllersChanged()
            case .failure(let error):
                failure = error.userMessage
            }
            await refresh(reset: true)
        }
    }

    // MARK: Reading

    private func refresh(reset: Bool = false) async {
        guard let session = model.session else { info = nil; return }
        let rows = model.controllerSelection
        let json = await offMain { () -> String? in
            guard let id = session.firstControllerInfo(rows: rows)?.id else { return nil }
            return session.studioControllerNetwork(id: id)
        }
        guard let json, let next = try? JSONDecoder().decode(PluginInfo.self, from: Data(json.utf8)) else {
            info = nil
            return
        }
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
        if switched || reset || (configChanged && draftErrors.isEmpty) {
            draft = next.plugin.config ?? .object([:])
            if switched || configChanged { draftErrors = [:] }
            if switched { failure = nil }
        }
    }

    // MARK: Harness

    /// `{ "action": "hook", "name": "studio.pluginForm", "path": [...] }`: `commit <pointer> <text>` hands a field's text
    /// over as Return would, `toggle <pointer>` flips a switch, `state` answers the configuration shown and its problems.
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
            guard info != nil else { return nil }
            switch args.first ?? "" {
            case "commit":
                guard args.count >= 2, let field = find(args[1], in: form) else { return "FAIL no field \(args.dropFirst().first ?? "")" }
                commitText(args.dropFirst(2).joined(separator: " "), field: field, pointer: field.pointer)
            case "toggle":
                guard args.count >= 2 else { return "FAIL toggle <pointer>" }
                set(args[1], .bool(!(draft[args[1]]?.bool ?? false)))
            case "state": break
            default: return "FAIL unknown \(args.first ?? "")"
            }
            try? await Task.sleep(for: .milliseconds(1_500))
            let problems = draftErrors.sorted { $0.key < $1.key }.map { "\($0.key) \($0.value)" }.joined(separator: "; ")
            return "config=\(info?.plugin.config?.jsonText ?? "nil") draft=\(draft.jsonText) errors=[\(problems)] failure=\(failure ?? "")"
        }
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

    var bool: Bool? { if case .bool(let b) = self { b } else { nil } }
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

/// The `studio.pluginForm` hook. Display and Controllers each keep a plugin inspector, so it acts in the one that came
/// first and showing a controller (the one on screen), as a person editing one of them would.
enum StudioPluginFormHarness {
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
}
