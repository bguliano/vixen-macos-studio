import Observation
import SwiftUI
import VixenBridge

/// Create/Modify Multiple Items (F133 to F136). With `oldNames` it renames: two preview columns, the count fixed to theirs. OK hands back a name per generated row,
/// which is fewer than `oldNames` if the rules ran out.
public struct NameGeneratorSheet: View {
    private let onFinish: ([String]?) -> Void
    @State private var model: NameGeneratorModel
    // STUDIO-BEGIN name-sheet-verb: a caller can put its verb ("Rename") on the confirmation button
    private let verb: String

    public init(seed: String, oldNames: [String]?, verb: String = "OK", onFinish: @escaping ([String]?) -> Void) {
        self.onFinish = onFinish
        self.verb = verb
        _model = State(initialValue: NameGeneratorModel(seed: seed, oldNames: oldNames))
    }
    // STUDIO-END name-sheet-verb

    public var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Create/Modify Multiple Items").font(.headline)
            HStack(alignment: .top, spacing: 12) {
                NamingConfiguration(model: model)
                    .frame(width: 300)
                    .frame(maxHeight: .infinity, alignment: .top)
                    .disabled(model.finishing)
                NamePreviewTable(preview: model.preview)
            }
            SheetButtons(okTitle: verb, okDisabled: model.finishing, ok: ok, cancel: cancel) // STUDIO: the caller's verb
        }
        .padding(16)
        .sheetResizable(width: 715, height: 684)
        .sheetKeys(ok: ok, cancel: cancel)
        .onChange(of: model.input, initial: true) {
            model.syncPreview()
        }
    }

    /// Names still being made when OK comes are waited for, with the controls off.
    private func ok() {
        guard !model.finishing else { return }
        model.syncPreview()
        model.finishing = true
        model.preview.onceCurrent(finish)
    }

    private func cancel() {
        finish(nil)
    }

    private func finish(_ names: [String]?) {
        guard !model.finished else { return }
        model.finished = true
        model.preview.close()
        onFinish(names)
    }
}

/// The left side. Split into views that each read only what they show, so that typing in the format box redraws the box and nothing else.
private struct NamingConfiguration: View {
    let model: NameGeneratorModel

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            ItemCountRow(model: model)
            NameFormatRow(model: model)
            Text(verbatim: "Example: \"Tree - <1> - <2> - <3>\"")
                .font(.callout)
                .foregroundStyle(.secondary)
            GroupBox("Naming Rules") {
                NamingRules(model: model).padding(4)
            }
        }
    }
}

private struct ItemCountRow: View {
    @Bindable var model: NameGeneratorModel

    var body: some View {
        HStack {
            SheetFieldLabel("Total Number of Items:")
            Spacer(minLength: 0)
            SheetNumberField("Total Number of Items:", value: $model.count, range: model.countRange, width: 72)
                .disabled(model.countIsFixed)
        }
    }
}

private struct NameFormatRow: View {
    @Bindable var model: NameGeneratorModel

    var body: some View {
        HStack {
            SheetFieldLabel("Name format:")
            SheetTextField("Name format:", text: $model.format)
        }
    }
}

private struct NamingRules: View {
    @Bindable var model: NameGeneratorModel

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 4) {
                Picker("Rule type", selection: $model.newRuleKind) {
                    ForEach(model.ruleKinds, id: \.kind) { choice in
                        Text(choice.name).tag(choice.kind)
                    }
                }
                .labelsHidden()
                SheetIconButton(systemName: "plus", label: "Add Rule", action: model.addRule)
                SheetIconButton(systemName: "minus", label: "Delete Rule", action: model.deleteRule)
                    .disabled(model.selected == nil)
            }
            HStack(alignment: .top, spacing: 4) {
                List(selection: $model.selected) {
                    ForEach(Array(model.rules.enumerated()), id: \.element.id) { index, row in
                        Text(verbatim: "<\(index + 1)> \(row.name)")
                    }
                }
                .listStyle(.bordered)
                VStack(spacing: 4) {
                    SheetIconButton(systemName: "arrow.up", label: "Move Rule Up") { model.moveRule(by: -1) }
                    SheetIconButton(systemName: "arrow.down", label: "Move Rule Down") { model.moveRule(by: 1) }
                }
                .disabled(model.selected == nil)
            }
            .frame(minHeight: 120, maxHeight: .infinity)
            Group {
                if let id = model.selected {
                    RuleEditor(rule: model.ruleBinding(id)).id(id)
                }
            }
            .frame(maxWidth: .infinity, minHeight: 104, alignment: .topLeading)
        }
    }
}

@Observable
private final class NameGeneratorModel {
    struct Row: Identifiable {
        let id = UUID()
        let name: String
        var rule: SetupNamingRule

        init(_ rule: SetupNamingRule) {
            self.rule = rule
            name = setupRuleName(kind: rule.kind)
        }
    }

    /// What decides the names.
    struct Input: Equatable {
        var format: String
        var rules: [SetupNamingRule]
        var wanted: Int
    }

    let preview: NamePreview
    let countRange = 1...1_000_000
    let countIsFixed: Bool
    /// F134's order, the one reflection most likely gave upstream.
    let ruleKinds: [(kind: SetupRuleKind, name: String)] = [SetupRuleKind.sequentialLetters, .listOfLetters, .sequentialNumbers, .listOfWords].map {
        (kind: $0, name: setupRuleName(kind: $0))
    }

    var count: Int
    var format: String
    var rules: [Row]
    var selected: Row.ID?
    var newRuleKind = SetupRuleKind.sequentialLetters
    var finishing = false
    @ObservationIgnored var finished = false

    @ObservationIgnored private let fixedCount: Int?
    @ObservationIgnored private var requested: Input?

    init(seed: String, oldNames: [String]?) {
        preview = NamePreview(oldNames: oldNames)
        // NameGenerator.cs: renaming always seeds "NewName", and an empty list of old names falls back to the box.
        fixedCount = oldNames.flatMap { $0.isEmpty ? nil : $0.count }
        countIsFixed = fixedCount != nil
        count = fixedCount ?? 5
        let template = setupNumberedItemsTemplate(seed: oldNames == nil ? seed : "NewName")
        format = template.format
        rules = template.rules.map(Row.init)
        selected = rules.first?.id
    }

    var input: Input {
        Input(format: format, rules: rules.map(\.rule), wanted: fixedCount ?? count)
    }

    func syncPreview() {
        let input = input
        guard input != requested else { return }
        requested = input
        let (format, rules, wanted) = (input.format, input.rules, UInt32(clamping: input.wanted))
        preview.request {
            setupGenerateNames(format: format, rules: rules, wanted: wanted)
        }
    }

    func addRule() {
        let row = Row(setupNewNamingRule(kind: newRuleKind))
        rules.append(row)
        selected = row.id
        format = setupFormatAfterAdd(format: format, ruleNumber: UInt32(rules.count))
    }

    func deleteRule() {
        guard let index = rules.firstIndex(where: { $0.id == selected }) else { return }
        let oldCount = rules.count
        rules.remove(at: index)
        selected = rules.isEmpty ? nil : rules[max(index - 1, 0)].id
        format = setupFormatAfterDelete(format: format, oldRuleCount: UInt32(oldCount))
    }

    func moveRule(by offset: Int) {
        guard let index = rules.firstIndex(where: { $0.id == selected }), rules.indices.contains(index + offset) else { return }
        rules.swapAt(index, index + offset)
    }

    func ruleBinding(_ id: Row.ID) -> Binding<SetupNamingRule> {
        Binding(
            get: { self.rules.first { $0.id == id }?.rule ?? setupNewNamingRule(kind: .sequentialNumbers) },
            set: { rule in
                if let index = self.rules.firstIndex(where: { $0.id == id }) { self.rules[index].rule = rule }
            }
        )
    }
}

/// `panelRuleConfig`, which shows the selected rule's editor. Each is made anew when the selection changes, as upstream's are.
private struct RuleEditor: View {
    @Binding var rule: SetupNamingRule

    var body: some View {
        switch rule.kind {
        case .sequentialNumbers: NumericCounterEditor(rule: $rule)
        case .sequentialLetters: LetterCounterEditor(rule: $rule)
        case .listOfLetters: LetterIteratorEditor(rule: $rule)
        case .listOfWords: WordIteratorEditor(rule: $rule)
        }
    }
}

/// F136, Sequential Numbers.
private struct NumericCounterEditor: View {
    @Binding var rule: SetupNamingRule
    private let range = -1_000_000...1_000_000

    var body: some View {
        Grid(alignment: .leading, horizontalSpacing: 6, verticalSpacing: 6) {
            GridRow {
                SheetFieldLabel("Start Number:")
                SheetNumberField("Start Number:", value: $rule.start.sheetInt, range: range, width: 72)
            }
            GridRow {
                SheetFieldLabel("End Number:", enabled: !rule.endless)
                SheetNumberField("End Number:", value: $rule.end.sheetInt, range: range, width: 72)
                    .disabled(rule.endless)
                Toggle("Endless", isOn: $rule.endless)
                    .toggleStyle(.checkbox)
            }
            GridRow {
                SheetFieldLabel("Step:")
                SheetNumberField("Step:", value: step, range: range, width: 72)
            }
        }
    }

    private var step: Binding<Int> {
        Binding(
            get: { Int(rule.step) },
            set: { rule.step = setupStepAfterEdit(typed: Int32($0), previous: rule.step) }
        )
    }
}

/// F136, Sequential Letters. Clearing the box leaves the last letter in force.
private struct LetterCounterEditor: View {
    @Binding var rule: SetupNamingRule
    @State private var startLetter: String

    init(rule: Binding<SetupNamingRule>) {
        _rule = rule
        _startLetter = State(initialValue: rule.wrappedValue.startLetter)
    }

    var body: some View {
        Grid(alignment: .leading, horizontalSpacing: 6, verticalSpacing: 6) {
            GridRow {
                SheetFieldLabel("Start Letter:")
                SheetTextField("Start Letter:", text: $startLetter)
                    .frame(width: 56)
                    .onChange(of: startLetter) {
                        let letters = startLetter.filter { $0.isASCII && $0.isLetter }
                        if letters != startLetter {
                            startLetter = letters
                        } else if let first = letters.first {
                            rule.startLetter = String(first)
                        }
                    }
            }
            GridRow {
                SheetFieldLabel("Steps:")
                SheetNumberField("Steps:", value: $rule.count.sheetInt, range: 1...1_000_000, width: 72)
            }
        }
    }
}

/// F136, List of Letters. Clearing the box leaves the last letters in force.
private struct LetterIteratorEditor: View {
    @Binding var rule: SetupNamingRule
    @State private var letters: String

    init(rule: Binding<SetupNamingRule>) {
        _rule = rule
        _letters = State(initialValue: rule.wrappedValue.text)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("Enter a list of Letters to be use")
            SheetTextField("Enter a list of Letters to be use", text: $letters)
                .onChange(of: letters) {
                    if !letters.trimmingCharacters(in: .whitespaces).isEmpty {
                        rule.text = setupCleanLetterList(text: letters)
                    }
                }
            Text("Example: LCR LR NS EW UL (Only one)")
                .font(.callout)
                .foregroundStyle(.secondary)
        }
    }
}

/// F136, List of Words.
private struct WordIteratorEditor: View {
    @Binding var rule: SetupNamingRule

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("Enter a list of Words seperated by commas")
            SheetTextField("Enter a list of Words seperated by commas", text: $rule.text)
            Text("Examples: Left, Right, Upper, Lower")
                .font(.callout)
                .foregroundStyle(.secondary)
        }
    }
}
