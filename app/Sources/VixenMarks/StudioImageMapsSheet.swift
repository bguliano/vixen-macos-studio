import AppKit
import SwiftUI
import UniformTypeIdentifiers
import VixenBridge
import VixenKit

// Rest of R40: Tools ▸ LipSync ▸ Edit Image Maps without OK/Cancel windows. Every change (New, Rename, Clone, Delete, Make Default,
// a picture, the notes) applies at once as one step on the profile's shared undo history (`studio_lipsync.rs`), the same commands
// agents run (`image_map_*`). The sheets mirror that history into their window's Edit ▸ Undo, as the show window's panes do, and
// close with Done. Vanilla's `ImageMapsSheet` stays for vanilla's own checks (`studio/replaced-views.txt`).

/// The library as `studio_image_map_list` gives it.
struct StudioImageMapsSnapshot: Decodable, Equatable, Sendable {
    struct Map: Decodable, Equatable, Identifiable, Sendable {
        let name: String
        let notes: String
        let isDefault: Bool
        let editable: Bool
        let usingDefaults: Bool
        let pictures: [String]
        var id: String { name }
    }

    let revision: UInt64
    let phonemes: [String]
    let maps: [Map]
}

private struct StudioImageMapsHistory: Decodable, Sendable {
    let undo: [String]
    let redo: [String]
}

private struct StudioImageMapsFailure: Error, Sendable {
    let text: String
    nonisolated init(_ error: Error) {
        if case FfiError.Failed(let message) = error { text = message } else { text = "\(error)" }
    }
}

/// The map open in the editor sheet.
struct StudioImageMapEditing: Identifiable, Equatable {
    var name: String
    var id: String { "editor" }
}

/// The Image Mapper's files and choices, while it shows.
@Observable
final class StudioImageMapperModel: Identifiable {
    let phonemes: [String]
    let files: [String]
    var choices: [ImageMapperChoice]

    init(phonemes: [String], files: [String], choices: [ImageMapperChoice]) {
        self.phonemes = phonemes
        self.files = files
        self.choices = choices
    }

    var fileNames: [String] { files.map { URL(fileURLWithPath: $0).lastPathComponent } }

    /// The pictures Map Pictures sets: a file for a phoneme, null to clear it; `<Mapped>` leaves a phoneme alone.
    var pictures: [String: Any] {
        var out: [String: Any] = [:]
        for (phoneme, choice) in zip(phonemes, choices) {
            switch choice {
            case .unmapped: out[phoneme] = NSNull()
            case .mapped: break
            case let .file(index): out[phoneme] = files[Int(index)]
            }
        }
        return out
    }
}

/// The sheets' state and work: one per showing of Edit Image Maps.
@MainActor @Observable
final class StudioImageMapsModel {
    let core: VixenCore
    private(set) var snapshot: StudioImageMapsSnapshot?
    var selection: Set<String> = []
    var renaming: String?
    var renameText = ""
    var editing: StudioImageMapEditing?
    var confirmingDelete = false
    private(set) var notice: String?
    private(set) var undoLabel: String?
    private(set) var redoLabel: String?
    private(set) var working = false
    /// Bumped when pictures change, so the editor reloads the one it shows.
    private(set) var pictureVersion = 0
    @ObservationIgnored var onChange: () -> Void = {}
    @ObservationIgnored private var renamingEditor = false
    /// The undo managers of the sheets showing, the front one last; the shared history is mirrored into the front one.
    @ObservationIgnored private var managers: [WeakManager] = []
    @ObservationIgnored private var manager: UndoManager? { managers.last?.manager }
    @ObservationIgnored private var history = ""
    @ObservationIgnored private let undoTarget = UndoTarget()

    private final class UndoTarget: NSObject {}

    private struct WeakManager {
        weak var manager: UndoManager?
        weak var window: NSWindow?
    }

    /// The sheet in front, whose undo manager holds the history.
    var frontWindow: NSWindow? { managers.last?.window }

    init(core: VixenCore) {
        self.core = core
    }

    var maps: [StudioImageMapsSnapshot.Map] { snapshot?.maps ?? [] }
    var phonemes: [String] { snapshot?.phonemes ?? [] }

    func map(_ name: String) -> StudioImageMapsSnapshot.Map? {
        maps.first { $0.name == name }
    }

    var selectedMap: StudioImageMapsSnapshot.Map? {
        selection.count == 1 ? selection.first.flatMap(map) : nil
    }

    var canEdit: Bool { !working && selectedMap?.editable == true }
    var canDelete: Bool { !working && !selection.isEmpty && selection.count < maps.count }
    var canClone: Bool { !working && !selection.isEmpty }
    var canMakeDefault: Bool { !working && selectedMap.map { !$0.isDefault } == true }
    var canUndo: Bool { !working && undoLabel != nil }
    var canRedo: Bool { !working && redoLabel != nil }

    // MARK: Loading

    func reload() async {
        let core = core
        let (text, historyText) = await Task.detached { (core.studioImageMapList(), core.studioDisplayHistory()) }.value
        if let next = try? JSONDecoder().decode(StudioImageMapsSnapshot.self, from: Data(text.utf8)), next != snapshot {
            snapshot = next
            selection = selection.intersection(next.maps.map(\.name))
            // The editor's map went (an agent, Undo of New); not while the editor itself renames it.
            if !renamingEditor, let open = editing, next.maps.first(where: { $0.name == open.name }) == nil {
                editing = nil
            }
        }
        pictureVersion += 1
        applyHistory(historyText, mode: .rebuild)
    }

    // MARK: Edits

    /// One `image_map_*` command, the same an agent runs. Returns its result, or nil when it was refused.
    @discardableResult
    func run(_ command: String, _ arguments: [String: Any] = [:], history mode: HistoryMode = .register) async -> [String: Any]? {
        guard let data = try? JSONSerialization.data(withJSONObject: arguments), let json = String(data: data, encoding: .utf8) else { return nil }
        // One command at a time, in the order they were asked for (the notes and the name can both commit as the editor closes).
        while working {
            try? await Task.sleep(for: .milliseconds(20))
        }
        working = true
        let core = core
        let outcome: Result<String, StudioImageMapsFailure> = await Task.detached {
            do { return .success(try core.studioDisplayCommand(command: command, argumentsJson: json)) } catch { return .failure(StudioImageMapsFailure(error)) }
        }.value
        working = false
        switch outcome {
        case .success(let reply):
            notice = nil
            let object = (try? JSONSerialization.jsonObject(with: Data(reply.utf8))) as? [String: Any] ?? [:]
            if object["changed"] as? Bool == true {
                let text = await Task.detached { core.studioDisplayHistory() }.value
                applyHistory(text, mode: mode)
                onChange()
            }
            await reload()
            return object["result"] as? [String: Any] ?? [:]
        case .failure(let failure):
            notice = failure.text
            await reload()
            return nil
        }
    }

    func new() async {
        guard let result = await run("image_map_create"), let name = result["name"] as? String else { return }
        selection = [name]
        editing = StudioImageMapEditing(name: name)
    }

    func edit(_ name: String) {
        guard map(name)?.editable == true else { return }
        renaming = nil
        selection = [name]
        editing = StudioImageMapEditing(name: name)
    }

    func beginRename(_ name: String) {
        selection = [name]
        renameText = name
        renaming = name
    }

    func commitRename() async {
        guard let old = renaming else { return }
        renaming = nil
        let new = renameText.trimmingCharacters(in: .whitespaces)
        guard !new.isEmpty, new != old else { return }
        if let result = await run("image_map_rename", ["name": old, "newName": new]), let name = result["name"] as? String {
            selection = [name]
        }
    }

    /// Renames the map the editor shows, and keeps the editor on it.
    func renameEditing(to new: String) async {
        guard let open = editing else { return }
        let new = new.trimmingCharacters(in: .whitespaces)
        guard !new.isEmpty, new != open.name else { return }
        renamingEditor = true
        defer { renamingEditor = false }
        if let result = await run("image_map_rename", ["name": open.name, "newName": new]), let name = result["name"] as? String {
            editing?.name = name
            selection = [name]
        }
        await reload()
    }

    func clone() async {
        var made: String?
        for name in maps.map(\.name) where selection.contains(name) {
            made = await run("image_map_clone", ["name": name])?["name"] as? String ?? made
        }
        if let made {
            selection = [made]
        }
    }

    func delete() async {
        let names = maps.map(\.name).filter(selection.contains)
        if await run("image_map_delete", ["names": names, "confirm": true]) != nil {
            selection = []
        }
    }

    func makeDefault() async {
        if let map = selectedMap {
            await run("image_map_set_default", ["name": map.name])
        }
    }

    func setNotes(_ notes: String, of name: String) async {
        guard let map = map(name), map.notes != notes else { return }
        await run("image_map_update", ["name": name, "notes": notes])
    }

    func setPictures(_ pictures: [String: Any], of name: String) async {
        guard !pictures.isEmpty else { return }
        await run("image_map_update", ["name": name, "pictures": pictures])
    }

    func picture(_ name: String, phoneme: String) -> NSImage? {
        guard map(name)?.pictures.contains(phoneme) == true, let data = core.studioImageMapPicture(name: name, phoneme: phoneme) else { return nil }
        return NSImage(data: data)
    }

    // MARK: Undo

    func undo() async {
        if let manager, manager.canUndo, manager.undoActionName == undoLabel {
            manager.undo()
        } else {
            await step(undo: true, fromMenu: false)
        }
    }

    func redo() async {
        if let manager, manager.canRedo, manager.redoActionName == redoLabel {
            manager.redo()
        } else {
            await step(undo: false, fromMenu: false)
        }
    }

    private func step(undo: Bool, fromMenu: Bool) async {
        guard undo ? canUndo : canRedo else {
            if fromMenu { forget() }
            return
        }
        if await run(undo ? "display_undo" : "display_redo", history: fromMenu ? .note : .rebuild) == nil {
            forget()
        }
    }

    private func forget() {
        manager?.removeAllActions(withTarget: undoTarget)
        history = ""
    }

    /// The sheet in front hands over its window's undo manager; the shared history is mirrored into it.
    func attach(_ next: UndoManager?, window: NSWindow?) {
        guard let next, next !== manager else { return }
        manager?.removeAllActions(withTarget: undoTarget)
        managers.removeAll { $0.manager == nil || $0.manager === next }
        managers.append(WeakManager(manager: next, window: window))
        history = ""
        Task { await reload() }
    }

    /// A sheet closed: the one behind it, if any, takes the history back.
    func detach(_ old: UndoManager?) {
        guard let old else { return }
        old.removeAllActions(withTarget: undoTarget)
        let wasFront = old === manager
        managers.removeAll { $0.manager == nil || $0.manager === old }
        if wasFront {
            history = ""
            if manager != nil {
                Task { await reload() }
            }
        }
    }

    enum HistoryMode {
        /// The sheet's own edit: add its one step.
        case register
        /// The undo manager just performed a step and already holds its opposite.
        case note
        /// Someone else changed the history: rebuild every step.
        case rebuild
    }

    private func applyHistory(_ text: String, mode: HistoryMode) {
        guard let decoded = try? JSONDecoder().decode(StudioImageMapsHistory.self, from: Data(text.utf8)) else { return }
        undoLabel = decoded.undo.first
        redoLabel = decoded.redo.first
        guard text != history, let manager else { return }
        history = text
        guard manager.groupingLevel == 0, !manager.isUndoing, !manager.isRedoing else {
            history = ""
            return
        }
        switch mode {
        case .note:
            return
        case .register:
            if let label = decoded.undo.first {
                ownGroup(manager) { registerStep(manager, label: label, undoing: true) }
            }
            return
        case .rebuild:
            break
        }
        manager.removeAllActions(withTarget: undoTarget)
        for label in decoded.undo.reversed() {
            ownGroup(manager) { registerStep(manager, label: label, undoing: true) }
        }
    }

    private func ownGroup(_ manager: UndoManager, _ register: () -> Void) {
        let automatic = manager.groupsByEvent
        manager.groupsByEvent = false
        manager.beginUndoGrouping()
        register()
        manager.endUndoGrouping()
        manager.groupsByEvent = automatic
    }

    private func registerStep(_ manager: UndoManager, label: String, undoing: Bool) {
        manager.registerUndo(withTarget: undoTarget) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self else { return }
                self.registerStep(manager, label: label, undoing: !undoing)
                Task { await self.step(undo: undoing, fromMenu: true) }
            }
        }
        manager.setActionName(label)
    }
}

/// Undo and Redo, named for the shared history's next step.
private struct StudioImageMapsUndoButtons: View {
    let model: StudioImageMapsModel

    var body: some View {
        HStack(spacing: 4) {
            Button { Task { await model.undo() } } label: { Label("Undo", systemImage: "arrow.uturn.backward") }
                .help(model.undoLabel.map { "Undo \($0)" } ?? "Nothing to undo")
                .disabled(!model.canUndo)
            Button { Task { await model.redo() } } label: { Label("Redo", systemImage: "arrow.uturn.forward") }
                .help(model.redoLabel.map { "Redo \($0)" } ?? "Nothing to redo")
                .disabled(!model.canRedo)
        }
        .labelStyle(.iconOnly)
    }
}

/// Mirrors the shared history into the sheet window's undo manager while the sheet shows, so Edit ▸ Undo reaches it.
private struct StudioUndoAttachment: ViewModifier {
    let model: StudioImageMapsModel

    func body(content: Content) -> some View {
        content.background(StudioUndoWindowReader(model: model))
    }
}

private struct StudioUndoWindowReader: NSViewRepresentable {
    let model: StudioImageMapsModel

    final class Marker: NSView {
        var onWindow: (NSWindow?) -> Void = { _ in }
        private weak var attached: UndoManager?

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            onWindow(window)
        }

        func attach(_ manager: UndoManager?, to model: StudioImageMapsModel) {
            if manager !== attached {
                model.detach(attached)
                attached = manager
                model.attach(manager, window: window)
            }
        }
    }

    func makeNSView(context: Context) -> Marker {
        let marker = Marker()
        marker.onWindow = { [weak marker, model] window in
            MainActor.assumeIsolated { marker?.attach(window?.undoManager, to: model) }
        }
        return marker
    }

    func updateNSView(_ view: Marker, context: Context) {}

    static func dismantleNSView(_ view: Marker, coordinator: ()) {
        view.onWindow(nil)
    }
}

// MARK: - The library

/// Tools ▸ LipSync ▸ Edit Image Maps. `onChange` runs after each change, for the sequencer to refresh; `onClose` when Done closes it.
public struct StudioImageMapsSheet: View {
    @State private var model: StudioImageMapsModel
    private let onClose: () -> Void

    public init(core: VixenCore, onChange: @escaping () -> Void, onClose: @escaping () -> Void) {
        let model = StudioImageMapsModel(core: core)
        model.onChange = onChange
        _model = State(initialValue: model)
        self.onClose = onClose
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                SheetTitle("Image Maps")
                Spacer()
                StudioImageMapsUndoButtons(model: model)
            }
            HStack(alignment: .top, spacing: 10) {
                list
                VStack(spacing: 8) {
                    Button("New") { Task { await model.new() } }
                        .disabled(model.working)
                    Button("Edit…") { if let map = model.selectedMap { model.edit(map.name) } }
                        .disabled(!model.canEdit)
                        .help(model.selectedMap.map { $0.editable ? "Change \($0.name)’s pictures and notes" : "\($0.name) is a string map from Windows Vixen and can’t be edited here" } ?? "Select one map")
                    Button("Clone") { Task { await model.clone() } }
                        .disabled(!model.canClone)
                    Button("Make Default") { Task { await model.makeDefault() } }
                        .disabled(!model.canMakeDefault)
                        .help("LipSync effects without a map of their own use the default")
                    Button("Delete", role: .destructive) { model.confirmingDelete = true }
                        .disabled(!model.canDelete)
                        .help(model.selection.count >= model.maps.count && !model.maps.isEmpty ? "The library keeps at least one map" : "Delete the selected maps. Undo brings them back.")
                }
                .buttonStyle(StudioEqualWidthButtonStyle())
                .frame(width: 104)
            }
            HStack(alignment: .firstTextBaseline) {
                Text(model.notice ?? "Changes apply as you make them. Edit ▸ Undo takes them back.")
                    .font(.callout)
                    .foregroundStyle(model.notice == nil ? AnyShapeStyle(.secondary) : AnyShapeStyle(.red))
                    .fixedSize(horizontal: false, vertical: true)
                Spacer()
                Button("Done") { onClose() }
                    .keyboardShortcut(.defaultAction)
                    .disabled(model.renaming != nil)
            }
        }
        .padding(16)
        .frame(minWidth: 460, idealWidth: 520, maxWidth: 720, minHeight: 320, idealHeight: 380, maxHeight: 560)
        .onExitCommand {
            if model.renaming != nil { model.renaming = nil } else { onClose() }
        }
        .modifier(StudioUndoAttachment(model: model))
        .sheet(item: $model.editing) { _ in
            StudioImageMapEditorSheet(model: model)
        }
        .confirmationDialog(model.selection.count == 1 ? "Delete “\(model.selection.first ?? "")”?" : "Delete \(model.selection.count) maps?", isPresented: $model.confirmingDelete) {
            Button("Delete", role: .destructive) { Task { await model.delete() } }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("LipSync effects that use it go back to the default map. Edit ▸ Undo brings it back with its pictures.")
        }
        .task { await model.reload() }
        .onAppear { StudioImageMapsTestHooks.install(model) }
    }

    private var list: some View {
        Table(model.maps, selection: $model.selection) {
            TableColumn("Name") { map in
                if model.renaming == map.name {
                    StudioImageMapRenameField(model: model)
                } else {
                    HStack(spacing: 6) {
                        Text(map.name).fontWeight(map.isDefault ? .bold : .regular)
                        if map.isDefault {
                            Text("Default").font(.caption).foregroundStyle(.secondary)
                        }
                    }
                }
            }
            TableColumn("Notes") { map in
                Text(map.usingDefaults ? "Default pictures" : map.notes)
                    .foregroundStyle(map.usingDefaults ? .secondary : .primary)
                    .lineLimit(1)
            }
        }
        .contextMenu(forSelectionType: String.self) { names in
            if names.count == 1, let name = names.first {
                Button("Edit…") { model.edit(name) }
                    .disabled(model.map(name)?.editable != true)
                Button("Rename") { model.beginRename(name) }
                Button("Make Default") {
                    model.selection = [name]
                    Task { await model.makeDefault() }
                }
                .disabled(model.map(name)?.isDefault == true)
            }
            if !names.isEmpty {
                Button("Clone") {
                    model.selection = names
                    Task { await model.clone() }
                }
                Button("Delete", role: .destructive) {
                    model.selection = names
                    model.confirmingDelete = true
                }
                .disabled(names.count >= model.maps.count)
            }
        } primaryAction: { names in
            if names.count == 1, let name = names.first {
                model.edit(name)
            }
        }
        .onDeleteCommand { if model.canDelete { model.confirmingDelete = true } }
    }
}

struct StudioEqualWidthButtonStyle: PrimitiveButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        Button(role: configuration.role, action: configuration.trigger) {
            configuration.label.frame(maxWidth: .infinity)
        }
    }
}

/// The name being edited in place. Return renames (one Undo step), Escape leaves it.
private struct StudioImageMapRenameField: View {
    @Bindable var model: StudioImageMapsModel
    @FocusState private var focused: Bool

    var body: some View {
        TextField("Name", text: $model.renameText)
            .textFieldStyle(.roundedBorder)
            .focused($focused)
            .onSubmit { Task { await model.commitRename() } }
            .onExitCommand { model.renaming = nil }
            .onAppear { focused = true }
    }
}

// MARK: - One map

/// One map's pictures and notes, each change applied as it is made. Done closes it.
struct StudioImageMapEditorSheet: View {
    @Bindable var model: StudioImageMapsModel
    @State private var name = ""
    @State private var notes = ""
    @State private var phonemeIndex = 0
    @State private var image: NSImage?
    @State private var mapper: StudioImageMapperModel?
    /// The picture open in another app (F103): its file, phoneme and app.
    @State private var external: (file: URL, phoneme: String, app: String)?
    @FocusState private var focus: Field?

    private enum Field { case name, notes }

    private var mapName: String { model.editing?.name ?? "" }
    private var phoneme: String { model.phonemes.indices.contains(phonemeIndex) ? model.phonemes[phonemeIndex] : "" }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                SheetTitle("Image Map")
                Spacer()
                StudioImageMapsUndoButtons(model: model)
            }
            HStack {
                Text("Name")
                TextField("Name", text: $name)
                    .labelsHidden()
                    .textFieldStyle(.roundedBorder)
                    .focused($focus, equals: .name)
                    .onSubmit { commitName() }
            }
            HStack(spacing: 12) {
                Spacer()
                Button { step(-1) } label: { Image(systemName: "chevron.left") }
                    .help("Previous phoneme")
                    .accessibilityLabel("Previous phoneme")
                VStack(spacing: 2) {
                    Text(phoneme).font(.callout.bold())
                    StudioPhonemeIcon(name: phoneme)
                }
                .frame(width: 60)
                Button { step(1) } label: { Image(systemName: "chevron.right") }
                    .help("Next phoneme")
                    .accessibilityLabel("Next phoneme")
                Spacer()
            }
            .disabled(external != nil)
            HStack(alignment: .top, spacing: 10) {
                StudioMappedImageView(image: image)
                    .onTapGesture(count: 2) { image == nil ? choose() : beginExternalEdit() }
                    .accessibilityLabel(image == nil ? "No image for \(phoneme)" : "Picture for \(phoneme)")
                if let external {
                    VStack(alignment: .leading, spacing: 8) {
                        Text("Editing in \(external.app)")
                            .font(.callout)
                            .fixedSize(horizontal: false, vertical: true)
                        Button("Done") { finishExternalEdit() }
                            .help("Use the picture as saved in \(external.app)")
                        Button("Discard") { discardExternalEdit() }
                    }
                    .buttonStyle(StudioEqualWidthButtonStyle())
                    .frame(width: 80)
                } else {
                    VStack(spacing: 8) {
                        Button("File…") { choose() }
                            .help("Choose a picture for \(phoneme), or several to map at once")
                        Button("Edit") { beginExternalEdit() }
                            .help("Open the picture in another app")
                        Button("Clear") { Task { await model.setPictures([phoneme: NSNull()], of: mapName) } }
                            .disabled(image == nil)
                    }
                    .buttonStyle(StudioEqualWidthButtonStyle())
                    .frame(width: 70)
                }
            }
            Text("Notes:")
            TextEditor(text: $notes)
                .font(.body)
                .frame(height: 64)
                .overlay(RoundedRectangle(cornerRadius: 4).stroke(.separator))
                .focused($focus, equals: .notes)
                .accessibilityLabel("Notes")
            HStack(alignment: .firstTextBaseline) {
                if let notice = model.notice {
                    Text(notice).font(.callout).foregroundStyle(.red).fixedSize(horizontal: false, vertical: true)
                }
                Spacer()
                Button("Done") { close() }
                    .keyboardShortcut(.defaultAction)
                    .disabled(external != nil)
            }
        }
        .padding(16)
        .frame(width: 300)
        .modifier(StudioUndoAttachment(model: model))
        .dropDestination(for: URL.self) { urls, _ in _ = drop(urls) }
        .sheet(item: $mapper) { mapper in
            StudioImageMapperSheet(model: mapper) { accepted in
                self.mapper = nil
                if accepted {
                    Task { await model.setPictures(mapper.pictures, of: mapName) }
                }
            }
        }
        .onAppear {
            load()
            StudioImageMapsTestHooks.editor = Hooks(name: { name = $0; commitName() }, notes: { notes = $0; commitNotes() }, step: step, phoneme: { phoneme }, image: { image })
        }
        .onChange(of: model.editing?.name) { load() }
        .onChange(of: model.pictureVersion) { refreshImage(); syncNotes() }
        .onChange(of: focus) { old, _ in
            if old == .notes { commitNotes() }
            if old == .name { commitName() }
        }
        .onDisappear { discardExternalEdit() }
    }

    /// What test scripts can do in the editor.
    struct Hooks {
        let name: (String) -> Void
        let notes: (String) -> Void
        let step: (Int) -> Void
        let phoneme: () -> String
        let image: () -> NSImage?
    }

    private func load() {
        name = mapName
        notes = model.map(mapName)?.notes ?? ""
        refreshImage()
    }

    /// Someone else (an agent, Undo) changed the notes while the field isn't being typed in.
    private func syncNotes() {
        if focus != .notes, let current = model.map(mapName)?.notes, current != notes {
            notes = current
        }
        if focus != .name, name != mapName {
            name = mapName
        }
    }

    private func refreshImage() {
        image = model.picture(mapName, phoneme: phoneme)
    }

    private func step(_ by: Int) {
        let count = model.phonemes.count
        guard count > 0 else { return }
        phonemeIndex = (phonemeIndex + by + count) % count
        refreshImage()
    }

    private func commitName() {
        let wanted = name
        Task {
            await model.renameEditing(to: wanted)
            name = mapName
        }
    }

    private func commitNotes() {
        let text = notes
        let map = mapName
        Task { await model.setNotes(text, of: map) }
    }

    private func close() {
        let (text, wanted, map) = (notes, name, mapName)
        Task {
            await model.setNotes(text, of: map)
            await model.renameEditing(to: wanted)
            model.editing = nil
        }
    }

    private func choose() {
        Task {
            let files = await ImageMapFiles.choose()
            load(files)
        }
    }

    private func drop(_ urls: [URL]) -> Bool {
        let files = urls.filter(\.isFileURL).map(\.path)
        guard !files.isEmpty, external == nil, studioImageMapAcceptsDrop(files: files) else { return false }
        load(files)
        return true
    }

    /// F101: one file is the current phoneme's picture; several open the Image Mapper.
    private func load(_ files: [String]) {
        switch files.count {
        case 0:
            return
        case 1:
            Task { await model.setPictures([phoneme: files[0]], of: mapName) }
        default:
            let mapped = model.map(mapName)?.pictures ?? []
            mapper = StudioImageMapperModel(phonemes: model.phonemes, files: files, choices: studioImageMapMapperChoices(current: phoneme, fileCount: UInt32(files.count), mapped: mapped))
        }
    }

    /// F103: the picture, or a blank one, as a PNG in a temporary folder, opened in the app that edits PNGs.
    private func beginExternalEdit() {
        guard external == nil else { return }
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("vixen-image-map-\(UUID().uuidString)")
        let file = dir.appendingPathComponent("\(phoneme).png")
        let rep = image.flatMap { $0.tiffRepresentation }.flatMap(NSBitmapImageRep.init(data:))
            ?? NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 128, pixelsHigh: 128, bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)
        guard let png = rep?.representation(using: .png, properties: [:]),
              (try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)) != nil,
              (try? png.write(to: file)) != nil
        else {
            return
        }
        let app = NSWorkspace.shared.urlForApplication(toOpen: file)
        external = (file, phoneme, app.map { FileManager.default.displayName(atPath: $0.path).replacingOccurrences(of: ".app", with: "") } ?? "another app")
        if UITestHarness.isActive {
            UITestHarness.note("open \(file.path)")
        } else {
            NSWorkspace.shared.open(file)
        }
    }

    private func finishExternalEdit() {
        guard let open = external else { return }
        external = nil
        let map = mapName
        Task {
            await model.setPictures([open.phoneme: open.file.path], of: map)
            try? FileManager.default.removeItem(at: open.file.deletingLastPathComponent())
        }
    }

    private func discardExternalEdit() {
        guard let open = external else { return }
        external = nil
        try? FileManager.default.removeItem(at: open.file.deletingLastPathComponent())
    }
}

/// The phoneme's reference mouth, at upstream's 48 points.
private struct StudioPhonemeIcon: View {
    let name: String

    var body: some View {
        if let image = VixenResources.image("marks/phonemes/\(name).jpg") {
            Image(nsImage: image).resizable().interpolation(.high).scaledToFit().frame(width: 48, height: 48)
        } else {
            Color.clear.frame(width: 48, height: 48)
        }
    }
}

/// The picture a phoneme maps to, aspect kept in a 128 point square, or upstream's tiled "no image" pattern.
private struct StudioMappedImageView: View {
    let image: NSImage?

    var body: some View {
        ZStack {
            if let image {
                Color.black.opacity(0.25)
                Image(nsImage: image).resizable().interpolation(.high).scaledToFit()
            } else if let tile = VixenResources.image("marks/NoImage.png") {
                Image(nsImage: tile).resizable(resizingMode: .tile)
            } else {
                Color.gray.opacity(0.2)
            }
        }
        .frame(width: 128, height: 128)
        .clipShape(Rectangle())
        .border(.separator)
        .contentShape(Rectangle())
    }
}

// MARK: - Image Mapper

/// F102: several pictures at once, a phoneme each. Map Pictures sets them all as one Undo step.
struct StudioImageMapperSheet: View {
    @Bindable var model: StudioImageMapperModel
    let onFinish: (Bool) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            SheetTitle("Image Mapper")
            Text("Choose the picture for each phoneme.")
                .fixedSize(horizontal: false, vertical: true)
            Grid(alignment: .leading, horizontalSpacing: 10, verticalSpacing: 6) {
                ForEach(Array(model.phonemes.enumerated()), id: \.offset) { index, phoneme in
                    GridRow {
                        Text(phoneme)
                        Picker(phoneme, selection: $model.choices[index]) {
                            Text("No picture").tag(ImageMapperChoice.unmapped)
                            Text("Keep current").tag(ImageMapperChoice.mapped)
                            ForEach(Array(model.fileNames.enumerated()), id: \.offset) { fileIndex, file in
                                Text(file).tag(ImageMapperChoice.file(index: UInt32(fileIndex)))
                            }
                        }
                        .labelsHidden()
                        .frame(maxWidth: .infinity)
                    }
                }
            }
            HStack {
                Spacer()
                Button("Cancel") { onFinish(false) }
                    .keyboardShortcut(.cancelAction)
                Button("Map Pictures") { onFinish(true) }
                    .keyboardShortcut(.defaultAction)
            }
        }
        .padding(16)
        .frame(width: 300)
        .onAppear { StudioImageMapsTestHooks.mapper = (model, onFinish) }
    }
}

// MARK: - Test hooks

/// What test scripts can ask of the live Image Maps sheets: `{ "action": "hook", "name": "studioImageMaps", "path": ["state"] }`.
@MainActor
enum StudioImageMapsTestHooks {
    static weak var model: StudioImageMapsModel?
    static var editor: StudioImageMapEditorSheet.Hooks?
    static var mapper: (StudioImageMapperModel, (Bool) -> Void)?
    static var lastClone: String?
    /// The sheet in front: Edit ▸ Undo's target while it shows.
    static var window: NSWindow? { model?.frontWindow }

    static func install(_ current: StudioImageMapsModel) {
        guard UITestHarness.isActive else { return }
        model = current
        UITestHarness.hooks["studioImageMaps"] = { args in
            guard let model, let verb = args.first else { return "FAIL needs the sheet and a verb" }
            let rest = Array(args.dropFirst())
            switch verb {
            case "state":
                let maps = model.maps.map { "\($0.name)\($0.isDefault ? "*" : "")[\($0.pictures.count)]" }.joined(separator: ", ")
                return "maps [\(maps)] selected [\(model.selection.sorted().joined(separator: ", "))] editing \(model.editing?.name ?? "-") undo \(model.undoLabel ?? "-") redo \(model.redoLabel ?? "-") notice \(model.notice ?? "-")"
            case "map":
                guard let name = rest.first, let map = model.map(name) else { return "FAIL no such map" }
                return "notes \(map.notes) default \(map.isDefault) usingDefaults \(map.usingDefaults) pictures [\(map.pictures.joined(separator: ","))]"
            case "select":
                model.selection = Set(rest)
                return "selected \(rest.count)"
            case "new":
                await model.new()
                return "new \(model.editing?.name ?? "-")"
            case "rename":
                guard rest.count == 2 else { return "FAIL needs an old and a new name" }
                model.beginRename(rest[0])
                model.renameText = rest[1]
                await model.commitRename()
                return model.notice.map { "FAIL \($0)" } ?? "renamed"
            case "clone":
                model.selection = Set(rest)
                await model.clone()
                lastClone = model.selection.first
                return "cloned \(lastClone ?? "-")"
            case "delete":
                // `@clone` is the map the last clone made, whose number comes from the run's counter.
                model.selection = Set(rest.map { $0 == "@clone" ? lastClone ?? $0 : $0 })
                await model.delete()
                return model.notice.map { "FAIL \($0)" } ?? "deleted"
            case "default":
                model.selection = Set(rest)
                await model.makeDefault()
                return "default"
            case "edit":
                guard let name = rest.first else { return "FAIL needs a name" }
                model.edit(name)
                return "editing \(model.editing?.name ?? "-")"
            case "closeEditor":
                model.editing = nil
                return "closed"
            case "editorName":
                guard let editor, let name = rest.first else { return "FAIL needs the editor and a name" }
                editor.name(name)
                return "named"
            case "editorNotes":
                guard let editor, let notes = rest.first else { return "FAIL needs the editor and notes" }
                editor.notes(notes)
                return "noted"
            case "editorStep":
                guard let editor else { return "FAIL no editor" }
                editor.step(Int(rest.first ?? "1") ?? 1)
                return editor.phoneme()
            case "editorImage":
                guard let editor else { return "FAIL no editor" }
                return "phoneme \(editor.phoneme()) image \(editor.image().map { "\(Int($0.size.width))x\(Int($0.size.height))" } ?? "none")"
            case "picture":
                guard let name = model.editing?.name, rest.count == 2 else { return "FAIL needs the editor, a phoneme and a file (or -)" }
                await model.setPictures([rest[0]: rest[1] == "-" ? NSNull() : rest[1]], of: name)
                return model.notice.map { "FAIL \($0)" } ?? "set"
            case "mapper":
                guard let mapper = mapper?.0 else { return "FAIL no mapper" }
                return zip(mapper.phonemes, mapper.choices).map { phoneme, choice in
                    switch choice {
                    case .unmapped: "\(phoneme)=-"
                    case .mapped: "\(phoneme)=mapped"
                    case let .file(index): "\(phoneme)=\(mapper.fileNames[Int(index)])"
                    }
                }.joined(separator: " ")
            case "mapperAccept":
                guard let finish = mapper?.1 else { return "FAIL no mapper" }
                finish(true)
                self.mapper = nil
                return "accepted"
            case "undoMenu":
                // The sequencer's Edit ▸ Undo sends undo: to the window with the keyboard: this sheet, whose undo manager names the step.
                let manager = window?.undoManager
                return "key \(window == nil ? "-" : "sheet") undo \(manager?.canUndo == true ? manager!.undoMenuItemTitle : "-") redo \(manager?.canRedo == true ? manager!.redoMenuItemTitle : "-")"
            case "sendUndo", "sendRedo":
                // What Edit ▸ Undo and Redo do with the sheet in front. Scripts run the app in the background, where no window has the
                // keyboard, so the action goes to the sheet itself.
                guard let window else { return "FAIL no sheet" }
                // Out of the text fields first: with one focused, ⌘Z undoes its typing before the history's steps.
                window.makeFirstResponder(nil)
                let sent = window.tryToPerform(Selector(verb == "sendUndo" ? "undo:" : "redo:"), with: nil)
                try? await Task.sleep(for: .milliseconds(600))
                return sent ? "sent" : "FAIL the sheet didn't take it"
            case "undo":
                await model.undo()
                return "undo"
            case "redo":
                await model.redo()
                return "redo"
            default:
                return "FAIL unknown verb \(verb)"
            }
        }
    }
}
