import Observation
import SwiftUI
import VixenBridge

/// Find / Replace Name (F137). OK hands back a new name for each old one.
public struct SubstitutionRenamerSheet: View {
    private let onFinish: ([String]?) -> Void
    @State private var model: RenamerModel
    @FocusState private var findFocused: Bool
    // STUDIO-BEGIN renamer-sheet-verb: a caller can put its verb ("Rename") on the confirmation button
    private let verb: String

    public init(oldNames: [String], verb: String = "OK", onFinish: @escaping ([String]?) -> Void) {
        self.onFinish = onFinish
        self.verb = verb
        _model = State(initialValue: RenamerModel(oldNames: oldNames))
    }
    // STUDIO-END renamer-sheet-verb

    public var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Find / Replace Name").font(.headline)
            HStack(alignment: .top, spacing: 12) {
                patterns
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
                    .disabled(model.finishing)
                NamePreviewTable(preview: model.preview)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
            SheetButtons(okTitle: verb, okDisabled: !model.okEnabled || model.finishing, ok: ok, cancel: cancel) // STUDIO: the caller's verb
        }
        .padding(16)
        .sheetResizable(width: 708, height: 508)
        .sheetKeys(okEnabled: model.okEnabled, ok: ok, cancel: cancel)
        .onAppear { findFocused = true }
    }

    private var patterns: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 4) {
                Text("Patterns")
                Spacer().frame(width: 20)
                SheetIconButton(systemName: "plus", label: "Add Pattern") {
                    model.addPattern()
                    findFocused = true
                }
                SheetIconButton(systemName: "minus", label: "Delete Pattern", action: model.deletePattern)
                Spacer(minLength: 0)
            }
            HStack(alignment: .top, spacing: 4) {
                List(selection: $model.selected) {
                    ForEach(model.rows) { row in
                        Text(row.text)
                    }
                }
                .listStyle(.bordered)
                VStack(spacing: 4) {
                    SheetIconButton(systemName: "arrow.up", label: "Move Pattern Up") { model.movePattern(by: -1) }
                    SheetIconButton(systemName: "arrow.down", label: "Move Pattern Down") { model.movePattern(by: 1) }
                }
                .disabled(model.selected == nil)
            }
            .frame(maxHeight: .infinity)
            Grid(alignment: .leading, horizontalSpacing: 6, verticalSpacing: 6) {
                GridRow {
                    SheetFieldLabel("Find What:", enabled: model.selected != nil)
                    SheetTextField("Find What:", text: $model.find)
                        .focused($findFocused)
                }
                GridRow {
                    SheetFieldLabel("Replace With:", enabled: model.selected != nil)
                    SheetTextField("Replace With:", text: $model.replace)
                }
            }
            .disabled(model.selected == nil)
        }
    }

    /// Names still being made when OK comes are waited for, with the controls off.
    private func ok() {
        guard !model.finishing else { return }
        model.finishing = true
        model.preview.onceCurrent(finish)
    }

    private func cancel() {
        finish(nil)
    }

    private func finish(_ names: [String]?) {
        guard !model.finished else { return }
        model.finished = true
        model.close()
        onFinish(names)
    }
}

@Observable
private final class RenamerModel {
    struct Row: Identifiable {
        let id: UUID
        let text: String
    }

    private struct Pattern {
        let id = UUID()
        var find = ""
        var replace = ""
    }

    let preview: NamePreview
    var finishing = false
    @ObservationIgnored var finished = false

    /// The list's text for each pattern, kept so that drawing it never crosses to Rust.
    private(set) var rows: [Row] = []
    /// SubstitutionRenamer.cs:28, :177-183: looked at only when a Find is edited, so deleting the one pattern with a Find leaves it on.
    private(set) var okEnabled = false

    var selected: UUID? {
        didSet { loadFields() }
    }

    var find = "" {
        didSet {
            guard find != oldValue, !loadingFields else { return }
            editSelected { $0.find = find }
            okEnabled = patterns.contains { !$0.find.isEmpty }
        }
    }

    var replace = "" {
        didSet {
            guard replace != oldValue, !loadingFields else { return }
            editSelected { $0.replace = replace }
        }
    }

    @ObservationIgnored private var oldNames: [String]
    @ObservationIgnored private var patterns = [Pattern()]
    @ObservationIgnored private var loadingFields = false

    init(oldNames: [String]) {
        self.oldNames = oldNames
        // With nothing to find the new names are the old ones, so the first answer is already known.
        preview = NamePreview(oldNames: oldNames, names: oldNames)
        selected = patterns[0].id
        refreshRows()
    }

    /// The preview frees its copy off the main thread, which only helps if this one has already gone.
    func close() {
        oldNames = []
        preview.close()
    }

    func addPattern() {
        patterns.append(Pattern())
        refreshRows()
        selected = patterns.last?.id
    }

    /// Does nothing when one is left (:202). Deleting the first leaves nothing selected (:208-212).
    func deletePattern() {
        guard patterns.count > 1, let index = patterns.firstIndex(where: { $0.id == selected }) else { return }
        patterns.remove(at: index)
        refreshRows()
        selected = index > 0 ? patterns[index - 1].id : nil
        regenerate()
    }

    func movePattern(by offset: Int) {
        guard let index = patterns.firstIndex(where: { $0.id == selected }), patterns.indices.contains(index + offset) else { return }
        patterns.swapAt(index, index + offset)
        refreshRows()
        regenerate()
    }

    private func editSelected(_ change: (inout Pattern) -> Void) {
        guard let index = patterns.firstIndex(where: { $0.id == selected }) else { return }
        change(&patterns[index])
        refreshRows()
        regenerate()
    }

    /// With nothing selected the boxes keep their text and are only disabled, as upstream's are.
    private func loadFields() {
        guard let pattern = patterns.first(where: { $0.id == selected }) else { return }
        loadingFields = true
        find = pattern.find
        replace = pattern.replace
        loadingFields = false
    }

    private func refreshRows() {
        rows = patterns.enumerated().map { index, pattern in
            Row(id: pattern.id, text: setupPatternRowText(index: UInt32(index), find: pattern.find, replace: pattern.replace))
        }
    }

    private func regenerate() {
        let old = oldNames
        let pairs = patterns.map { SetupFindReplace(find: $0.find, replace: $0.replace) }
        preview.request {
            setupReplacePatterns(old: old, patterns: pairs)
        }
    }
}
