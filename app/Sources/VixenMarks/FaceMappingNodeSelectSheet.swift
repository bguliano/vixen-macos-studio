import AppKit
import SwiftUI
import VixenBridge
import VixenDialogs
import VixenKit

/// Tools > LipSync > Edit Element Face Mapping: choose elements, then Face Setup on them. Present it as a sheet; `onClose` runs when it
/// is done either way, and says whether Face Setup saved.
public struct FaceMappingNodeSelectSheet: View {
    @State private var model: FaceNodeSelectModel
    private let app: AppModel
    private let onClose: (Bool) -> Void
    @State private var studio: StudioFaceMappingVisits // STUDIO: each Face Setup visit is one step on the shared Undo (R40)
    @Environment(\.undoManager) private var undoManager // STUDIO: the sheet's Edit ▸ Undo mirrors the visits' steps (R40)

    public init(core: VixenCore, model app: AppModel, onClose: @escaping (Bool) -> Void) {
        _model = State(initialValue: FaceNodeSelectModel(select: core.faceNodeSelect()))
        _studio = State(initialValue: StudioFaceMappingVisits(core: core)) // STUDIO: each Face Setup visit is one step on the shared Undo (R40)
        self.app = app
        self.onClose = onClose
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            SheetTitle("LipSync Mapping Node Select")
            HStack(alignment: .top, spacing: 10) {
                OutlineView(
                    provider: model.tree,
                    handle: model.treeHandle,
                    accessibilityLabel: "Elements",
                    onSelectionChange: { model.treeSelection = $0.ids }
                )
                .border(.separator)
                VStack(spacing: 8) {
                    Button("->") { model.add() }
                        .help("Add the selected elements")
                        .accessibilityLabel("Add")
                    Button("<-") { model.remove() }
                        .help("Remove the selected chosen elements")
                        .accessibilityLabel("Remove")
                    Button("Reset") { model.reset() }
                    VStack(alignment: .leading, spacing: 8) {
                        Toggle("Allow Groups", isOn: Binding(get: { model.allowGroups }, set: { model.setAllowGroups($0) }))
                        Toggle("Add Recursively", isOn: Binding(get: { model.recurse }, set: { model.setRecurse($0) }))
                    }
                    .padding(.top, 12)
                }
                .buttonStyle(NodeSelectButtonStyle())
                .frame(width: 130, alignment: .leading)
                .padding(.top, 40)
                OutlineView(
                    provider: model.chosen,
                    handle: model.chosenHandle,
                    accessibilityLabel: "Chosen elements",
                    onSelectionChange: { model.chosenSelection = $0.ids }
                )
                .border(.separator)
            }
            // STUDIO-BEGIN face-mapping-live: choosing changes nothing; Set Up Faces… opens Face Setup, and Done closes (R40)
            HStack {
                Spacer()
                Button("Done") { finish(studio.changed) }
                    .keyboardShortcut(.cancelAction)
                Button("Set Up Faces…") { ok() }
                    .keyboardShortcut(.defaultAction)
                    .disabled(model.studioChosenCount == 0)
            }
            // STUDIO-END face-mapping-live
        }
        .padding(16)
        .frame(minWidth: 560, idealWidth: 675, maxWidth: 1000, minHeight: 320, idealHeight: 433, maxHeight: 800)
        .sheet(isPresented: $model.showingFaceSetup) {
            SetupHelperSheet(helper: .faceMapping, nodes: model.faceNodes, model: app) { saved in
                model.showingFaceSetup = false
                // STUDIO-BEGIN face-mapping-visit: the visit is one step on the shared Undo; the sheet stays up for more (R40)
                _ = saved
                if let outcome = studio.finish() {
                    model.rendered = Int(outcome.rendered)
                }
                // STUDIO-END face-mapping-visit
            }
        }
        .task { await model.load() }
        .onAppear {
            // The sequencer presents this sheet too, without ever opening MarksTestScene.
            FaceNodeSelectTestHooks.install()
            FaceNodeSelectTestHooks.current = model
            StudioFaceMappingTestHooks.install() // STUDIO: the visits' Undo for test scripts (R40)
        }
        .onChange(of: undoManager, initial: true) { studio.attach(undoManager) } // STUDIO: the sheet's Edit ▸ Undo (R40)
    }

    private func ok() {
        // OK with nothing chosen closes and does nothing more.
        model.faceNodes = model.select.chosenNodes()
        if model.faceNodes.isEmpty {
            finish(false)
        } else {
            studio.begin(nodes: model.faceNodes) // STUDIO: the visit becomes one step on the shared Undo (R40)
            model.showingFaceSetup = true
        }
    }

    private func finish(_ saved: Bool) {
        FaceNodeSelectTestHooks.lastClose = saved
        onClose(saved)
    }
}

private struct NodeSelectButtonStyle: PrimitiveButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        Button(role: configuration.role, action: configuration.trigger) {
            configuration.label.frame(width: 60)
        }
    }
}

@MainActor
@Observable
final class FaceNodeSelectModel {
    let select: FaceNodeSelect
    @ObservationIgnored let tree: TreeProvider
    @ObservationIgnored let chosen: ChosenProvider
    @ObservationIgnored let treeHandle = OutlineHandle()
    @ObservationIgnored let chosenHandle = OutlineHandle()
    @ObservationIgnored var treeSelection: [String] = []
    @ObservationIgnored var chosenSelection: [String] = []
    var allowGroups: Bool
    var recurse: Bool
    var showingFaceSetup = false
    var faceNodes: [String] = []
    var rendered: Int?
    var studioChosenCount = 0 // STUDIO: Set Up Faces… needs a chosen element (R40)

    init(select: FaceNodeSelect) {
        self.select = select
        tree = TreeProvider(select: select)
        chosen = ChosenProvider(select: select)
        allowGroups = select.allowGroups()
        recurse = select.recurse()
    }

    func load() async {
        await treeHandle.loadRoots()
        await chosenHandle.loadRoots()
    }

    func add() {
        select.add(rows: treeSelection)
        reloadChosen()
    }

    func remove() {
        select.remove(nodes: chosenSelection)
        reloadChosen()
    }

    func reset() {
        select.reset()
        reloadChosen()
    }

    func setAllowGroups(_ on: Bool) {
        select.setAllowGroups(on: on)
        allowGroups = select.allowGroups()
        reloadChosen()
    }

    func setRecurse(_ on: Bool) {
        select.setRecurse(on: on)
        recurse = select.recurse()
    }

    private func reloadChosen() {
        Task {
            await chosenHandle.reload(roots: true) { _ in false }
            chosenSelection = chosenHandle.selection.ids
            studioChosenCount = chosenHandle.rowCount // STUDIO: Set Up Faces… needs a chosen element (R40)
        }
    }

    @MainActor
    final class TreeProvider: OutlineDataProvider {
        let select: FaceNodeSelect

        init(select: FaceNodeSelect) {
            self.select = select
        }

        func outlineChildren(of parent: String?) -> [OutlineRow] {
            select.children(parent: parent).map { OutlineRow(id: $0.id, title: $0.name, hasChildren: $0.hasChildren) }
        }

        func outlineChildrenAsync(of parent: String?) async -> [OutlineRow] {
            let select = select
            let rows = await Task.detached { select.children(parent: parent) }.value
            return rows.map { OutlineRow(id: $0.id, title: $0.name, hasChildren: $0.hasChildren) }
        }
    }

    @MainActor
    final class ChosenProvider: OutlineDataProvider {
        let select: FaceNodeSelect

        init(select: FaceNodeSelect) {
            self.select = select
        }

        func outlineChildren(of parent: String?) -> [OutlineRow] {
            parent == nil ? select.chosen().map { OutlineRow(id: $0.id, title: $0.name, hasChildren: false) } : []
        }

        func outlineChildrenAsync(of parent: String?) async -> [OutlineRow] {
            guard parent == nil else { return [] }
            let select = select
            let rows = await Task.detached { select.chosen() }.value
            return rows.map { OutlineRow(id: $0.id, title: $0.name, hasChildren: false) }
        }
    }
}

/// `{ "action": "hook", "name": "faceNodes", "path": ["state"] }` and so on, for test scripts: a test window's tree takes no clicks.
enum FaceNodeSelectTestHooks {
    static weak var current: FaceNodeSelectModel?
    static var lastClose: Bool?
    private static var installed = false

    static func install() {
        guard UITestHarness.isActive, !installed else { return }
        installed = true
        UITestHarness.hooks["faceNodes"] = { args in
            guard let verb = args.first else { return "FAIL needs a verb" }
            let rest = Array(args.dropFirst())
            guard let model = current else {
                return verb == "state" ? "(closed) lastClose \(lastClose.map(String.init) ?? "-")" : "FAIL no Node Select sheet"
            }
            switch verb {
            case "state":
                let chosen = model.select.chosen().map(\.name)
                let shown = chosen.count > 12 ? chosen.prefix(12).joined(separator: ", ") + ", …" : chosen.joined(separator: ", ")
                return "chosen \(chosen.count) [\(shown)] groups=\(model.allowGroups) recurse=\(model.recurse) treeRows=\(model.treeHandle.rowCount) chosenRows=\(model.chosenHandle.rowCount) treeSel=\(model.treeSelection.count) chosenSel=\(model.chosenSelection.count) faceSetup=\(model.showingFaceSetup) rendered=\(model.rendered.map(String.init) ?? "-")"
            case "add":
                // The arrow buttons' own actions: the harness finds text by reading the picture, and "->" is not reliably read.
                model.add()
                return "added"
            case "remove":
                model.remove()
                return "removed"
            case "roots":
                return model.select.children(parent: nil).map(\.name).joined(separator: ", ")
            case "selectTree":
                // By name, among the roots and the children of expanded rows named before a slash: "Group/Child".
                var ids: [String] = []
                for path in rest {
                    var parent: String?
                    var found: String?
                    for part in path.split(separator: "/").map(String.init) {
                        guard let row = model.select.children(parent: parent).first(where: { $0.name == part }) else {
                            return "FAIL no row \(path)"
                        }
                        if let parent { await model.treeHandle.expand([parent]) }
                        parent = row.id
                        found = row.id
                    }
                    if let found { ids.append(found) }
                }
                let expanding = Set(ids.compactMap { id -> String? in
                    guard let slash = id.lastIndex(of: "/") else { return nil }
                    return String(id[..<slash])
                })
                await model.treeHandle.select(ids, expanding: Array(expanding))
                return "selected \(model.treeSelection.count)"
            case "selectChosen":
                let rows = model.select.chosen().filter { rest.contains($0.name) }.map(\.id)
                await model.chosenHandle.select(rows)
                return "selected \(model.chosenSelection.count)"
            default:
                return "FAIL unknown verb \(verb)"
            }
        }
    }
}
