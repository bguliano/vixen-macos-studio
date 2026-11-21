import AppKit
import SwiftUI
import VixenBridge
import VixenKit

/// Color Breakdown Filter Setup (F135 to F147).
struct ColorBreakdownSheet: View {
    let context: DialogContext
    let filter: String
    let onFinish: (Bool) -> Void
    @State private var draft: DialogColorBreakdown
    @State private var template: String
    @State private var alert: DialogAlert?
    @State private var studio = StudioLiveSheet(step: "Configure Color Breakdown") // STUDIO: live sheet, Undo while it's up (B9)

    init(context: DialogContext, filter: String, draft: DialogColorBreakdown, onFinish: @escaping (Bool) -> Void) {
        self.context = context
        self.filter = filter
        self.onFinish = onFinish
        _draft = State(initialValue: draft)
        _template = State(initialValue: context.colorBreakdownTemplates().first ?? "RGB")
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            DialogTitle("Color Breakdown Filter Setup")
            Text("Select a pre-configured template from the drop-down box, or build a\ncustom color breakdown filter below.")
            HStack {
                Text("Template:")
                Picker("Template:", selection: $template) {
                    ForEach(context.colorBreakdownTemplates(), id: \.self) { Text($0).tag($0) }
                }
                .labelsHidden()
                .fixedSize()
                Button("Apply Template", action: applyTemplate)
            }
            ScrollView {
                VStack(spacing: 6) {
                    ForEach(Array(draft.rows.enumerated()), id: \.offset) { index, _ in
                        row(index)
                    }
                }
                .padding(6)
            }
            .frame(minHeight: 200, maxHeight: .infinity)
            .background(.quaternary.opacity(0.3), in: RoundedRectangle(cornerRadius: 6))
            VStack(alignment: .leading, spacing: 4) {
                Toggle("These colors should be mixed to produce the input color", isOn: $draft.mix).toggleStyle(.checkbox)
                if let note = context.colorBreakdownMixNote(rows: draft.rows, mix: draft.mix) {
                    Label(note, systemImage: "exclamationmark.triangle").font(.caption).foregroundStyle(.secondary)
                }
                Toggle("16 Bit Output (For Use with Course Fine Break Down)", isOn: $draft.sixteenBit).toggleStyle(.checkbox)
            }
            HStack {
                Button("Add Color") { draft.rows.append(context.colorBreakdownNewRow()) }
                StudioDoneButton(studio) { onFinish($0) } // STUDIO: live, closes with Done (B9)
            }
        }
        .padding(20)
        .frame(minWidth: 464, idealWidth: 464, minHeight: 548, idealHeight: 548)
        .studioLive(studio, draft: $draft) { try context.applyColorBreakdown(filter: filter, draft: $0) } // STUDIO: live sheet, Undo while it's up (B9)
        .dialogAlert($alert)
    }

    private func row(_ index: Int) -> some View {
        HStack(spacing: 10) {
            ArgbColorPicker(argb: Binding(get: { draft.rows[index].argb }, set: { draft.rows[index].argb = $0 }))
                .frame(width: 60, height: 32)
            Text("Name:")
            TextField("", text: Binding(get: { draft.rows[index].name }, set: { draft.rows[index].name = $0 })).labelsHidden()
            Button("Delete") { draft.rows.remove(at: index) }
        }
    }

    private func applyTemplate() {
        do {
            draft.rows = try context.colorBreakdownTemplate(template: template)
        } catch {
            alert = .notice("Error", error.userMessage)
        }
    }

    private func ok() {
        onFinish((try? context.applyColorBreakdown(filter: filter, draft: draft)) != nil)
    }
}

/// Coarse Fine Breakdown Setup (F148 to F150).
struct CoarseFineSheet: View {
    let context: DialogContext
    let filter: String
    let onFinish: (Bool) -> Void
    private let original: DialogCoarseFine
    @State private var draft: DialogCoarseFine
    @State private var live = false
    @State private var lastLive: DialogCoarseFine?
    @State private var studio = StudioLiveSheet(step: "Configure Coarse Fine Breakdown") // STUDIO: live sheet, Undo while it's up (B9)

    init(context: DialogContext, filter: String, draft: DialogCoarseFine, onFinish: @escaping (Bool) -> Void) {
        self.context = context
        self.filter = filter
        self.onFinish = onFinish
        original = draft
        _draft = State(initialValue: draft)
    }

    var body: some View {
        let issues = context.coarseFineIssues(draft: draft)
        VStack(alignment: .leading, spacing: 12) {
            DialogTitle("Coarse Fine Breakdown Setup")
            Toggle("Enable default value mapping", isOn: $draft.enableDefaultValueMapping).toggleStyle(.checkbox)
            // STUDIO-BEGIN coarse-fine-always-live: every change applies, so upstream's Live mode switch goes (B9)
            // STUDIO-END coarse-fine-always-live
            slider("Resting coarse value (0-255)", value: $draft.restingCoarse)
            slider("Resting fine value (0-255)", value: $draft.restingFine)
            ForEach(issues, id: \.self) { Text($0).font(.caption).foregroundStyle(.red) }
            StudioDoneButton(studio) { onFinish($0) } // STUDIO: live, closes with Done (B9)
        }
        .padding(20)
        .frame(width: 420)
        .studioLive(studio, draft: $draft) { d in guard context.coarseFineIssues(draft: d).isEmpty else { throw StudioLiveHold() }; try context.applyCoarseFine(filter: filter, draft: d) } // STUDIO: live sheet, Undo while it's up (B9)
        .onChange(of: draft) { applyLive() }
        .onChange(of: live) { applyLive() }
    }

    private func slider(_ title: String, value: Binding<Int32>) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(title)
            IntSlider(value: value, range: 0...255)
        }
    }

    private func applyLive() {
        guard live, context.coarseFineIssues(draft: draft).isEmpty else { return }
        if (try? context.applyCoarseFine(filter: filter, draft: draft)) != nil {
            lastLive = draft
        }
    }

    private func ok() {
        if lastLive != draft {
            _ = try? context.applyCoarseFine(filter: filter, draft: draft)
        }
        onFinish(true)
    }

    /// F150: restores the original, if anything was applied live.
    private func cancel() {
        if lastLive != nil {
            _ = try? context.applyCoarseFine(filter: filter, draft: original)
        }
        onFinish(false)
    }
}

/// The tag family's sheet (F230 to F236).
struct TagFilterSheet: View {
    let context: DialogContext
    let filter: String
    let labels: DialogTagFilterLabels
    let onFinish: (Bool) -> Void
    @State private var draft: DialogTagFilter
    @State private var alert: DialogAlert?
    @State private var studio = StudioLiveSheet(step: "Configure Tag Filter") // STUDIO: live sheet, Undo while it's up (B9)

    init(context: DialogContext, filter: String, labels: DialogTagFilterLabels, draft: DialogTagFilter, onFinish: @escaping (Bool) -> Void) {
        self.context = context
        self.filter = filter
        self.labels = labels
        self.onFinish = onFinish
        _draft = State(initialValue: draft)
    }

    var body: some View {
        let canApply = (try? context.tagFilterCanApply(filter: filter, draft: draft)) ?? false
        VStack(alignment: .leading, spacing: 12) {
            DialogTitle(labels.title)
            Grid(alignment: .leading, verticalSpacing: 8) {
                GridRow {
                    Text("Tag:")
                    TextField("", text: $draft.tag).labelsHidden()
                }
                if !labels.automatic.isEmpty {
                    GridRow {
                        Color.clear.gridCellUnsizedAxes([.horizontal, .vertical])
                        Toggle(labels.automatic, isOn: $draft.automatic).toggleStyle(.checkbox).fixedSize()
                    }
                }
                if !labels.open.isEmpty {
                    valueRow(labels.open, text: $draft.openText)
                    valueRow(labels.close, text: $draft.closeText)
                }
                if !labels.function.isEmpty {
                    GridRow {
                        Text(labels.function).fixedSize()
                        TextField("", text: $draft.functionName).labelsHidden()
                    }
                }
            }
            if !canApply { Text("Changes apply once the tag and values are valid.").font(.caption).foregroundStyle(.secondary) } // STUDIO: why a change waits (B9)
            StudioDoneButton(studio) { onFinish($0) } // STUDIO: live, closes with Done (B9)
        }
        .padding(20)
        .frame(width: labels.function.isEmpty ? 480 : 560)
        .studioLive(studio, draft: $draft) { d in guard (try? context.tagFilterCanApply(filter: filter, draft: d)) == true else { throw StudioLiveHold() }; try context.applyTagFilter(filter: filter, draft: d) } // STUDIO: live sheet, Undo while it's up (B9)
        .dialogAlert($alert)
    }

    private func valueRow(_ title: String, text: Binding<String>) -> some View {
        GridRow {
            Text(title).fixedSize().foregroundStyle(draft.automatic ? .primary : .secondary)
            TextField("", text: text)
                .labelsHidden()
                .monospacedDigit()
                .disabled(!draft.automatic)
                .help("0-255")
        }
    }

    private func ok() {
        do {
            try context.applyTagFilter(filter: filter, draft: draft)
            onFinish(true)
        } catch {
            alert = .notice(labels.title, error.userMessage)
        }
    }
}

extension DialogCurve {
    var value: CurveValue {
        CurveValue(points: points.map { KitPoint(x: $0.x, y: $0.y) }, libraryName: libraryReferenceName, isCurrentLibrary: isCurrentLibraryCurve)
    }

    init(_ value: CurveValue) {
        self.init(points: value.points.map { DialogCurvePoint(x: $0.x, y: $0.y) }, libraryReferenceName: value.libraryName, isCurrentLibraryCurve: value.isCurrentLibrary)
    }
}

/// F237: the Dimming Curve filter's Configure goes straight to the curve editor.
struct DimmingCurveFilterSheet: View {
    let context: DialogContext
    let filter: String
    let curve: DialogCurve
    let onFinish: (Bool) -> Void

    var body: some View {
        // STUDIO-BEGIN curve-editor-live-filter: each change applies to the filter at once; Done closes (B24)
        CurveEditorView(
            core: context.vixenCore(),
            curve: curve.value,
            onChange: { edited in _ = try? context.applyDimmingCurve(filter: filter, curve: DialogCurve(edited)) },
            onDone: onFinish
        )
        // STUDIO-END curve-editor-live-filter
    }
}
