import AppKit
import SwiftUI
import VixenBridge
import VixenKit

/// The Effect Editor tab, F1: the Effect Preview bar, the header bar, the scrolling categories, and the information box at the end of the scrolled content.
public struct EffectEditorPane: View {
    @State private var ownModel: EffectEditorModel?
    private let hostModel: EffectEditorModel?
    private let session: SequencerSession?
    private let onUndoRecorded: ((String) -> Bool)?
    private let selectedEffects: [UInt64]

    /// `selectedEffects` are the effects selected on the timeline, in `SelectedElements` order. `onUndoRecorded` is called when a commit puts an entry on the session's undo stack: return true if the
    /// window's `UndoManager` has been told, otherwise the panel tells it.
    public init(session: SequencerSession, selectedEffects: [UInt64], onUndoRecorded: ((String) -> Bool)? = nil) {
        self.session = session
        hostModel = nil
        self.onUndoRecorded = onUndoRecorded
        self.selectedEffects = selectedEffects
    }

    /// With a model the host keeps, to call `reload()` after its own edits, or `suspendPreview()` and `resumePreview()` around playback.
    public init(model: EffectEditorModel, selectedEffects: [UInt64]) {
        session = nil
        hostModel = model
        onUndoRecorded = nil
        self.selectedEffects = selectedEffects
    }

    public var body: some View {
        if let model = hostModel ?? ownModel {
            EffectEditorContent(model: model, selectedEffects: selectedEffects)
        } else {
            Color.clear.onAppear {
                guard let session else { return }
                let model = EffectEditorModel(session: session)
                model.onUndoRecorded = onUndoRecorded
                ownModel = model
            }
        }
    }
}

private struct EffectEditorContent: View {
    @Bindable var model: EffectEditorModel
    let selectedEffects: [UInt64]

    var body: some View {
        VStack(spacing: 0) {
            // STUDIO-BEGIN inspector-header: effect name first, labelled preview switch, an empty state when nothing is selected
            if !model.effectName.isEmpty {
                HeaderBar(model: model)
            }
            previewBar
            Divider()
            if model.effectName.isEmpty {
                ContentUnavailableView {
                    Label("No Effect Selected", systemImage: "slider.horizontal.3")
                } description: {
                    Text("Select an effect on the timeline to edit its colors, timing curves and other settings.")
                }
                .frame(maxHeight: .infinity)
            } else {
            ScrollView(.vertical) {
                VStack(alignment: .leading, spacing: 0) {
                    ForEach(model.sections) { section in
                        SectionView(section: section, model: model)
                    }
                    InformationBox(model: model)
                }
                .padding(.horizontal, StudioMetrics.spacingS)
                .padding(.bottom, StudioMetrics.spacingS)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            }
            // STUDIO-END inspector-header
        }
        .controlSize(.small)
        .background(HostWindowReader(model: model))
        .onAppear {
            model.watchPresses()
            model.selectionChanged(selectedEffects)
            EditorKeys.registerTestHook()
            ValueDragTesting.register()
            if UITestHarness.isActive {
                UITestHarness.hooks["effectEditor.renderProbe"] = { [weak model = model] args in
                    guard args.isEmpty, let model, !model.selection.isEmpty, model.selection.count <= 75 else {
                        return "FAIL needs one to 75 selected effects and no arguments"
                    }
                    // Observe the current selection and cached render only: no
                    // refresh, render request, result application or selection edit.
                    let effects: [[String: Any]] = model.selection.map { id in
                        ["id": id, "summary": model.api.renderSummary(effect: id)]
                    }
                    guard let data = try? JSONSerialization.data(withJSONObject: ["effects": effects], options: [.sortedKeys]),
                          let text = String(data: data, encoding: .utf8) else { return "FAIL render observation" }
                    return text
                }
                // Keep the same selected effect while a normal property commit
                // starts a new render. Observations use the actual AX label.
                UITestHarness.hooks["effectEditor.cacheProbe"] = { [weak model = model] args in
                    guard let model, model.effectName == "Video", model.selection.count == 1 else {
                        return "FAIL needs one selected Video effect"
                    }
                    if args.first == "render" {
                        guard let row = model.sections.flatMap(\.rows).first(where: { $0.key.param == "MaintainAspect" }),
                              case let .check(value) = row.control else {
                            return "FAIL no Maintain Aspect checkbox"
                        }
                        model.commit(row.key, .bool(value: !value))
                    } else if args.first == "file" {
                        guard args.count == 2,
                              let data = ProcessInfo.processInfo.environment["VIXEN_DATA"],
                              let row = model.sections.flatMap(\.rows).first(where: { $0.key.param == "FileName" }) else {
                            return "FAIL needs a private media path"
                        }
                        let root = URL(fileURLWithPath: data).resolvingSymlinksInPath().standardizedFileURL.path + "/"
                        let file = URL(fileURLWithPath: args[1]).resolvingSymlinksInPath().standardizedFileURL
                        guard file.path.hasPrefix(root), ["mov", "avi"].contains(file.pathExtension.lowercased()),
                              (try? file.resourceValues(forKeys: [.isRegularFileKey]).isRegularFile) == true else {
                            return "FAIL file must be regular generated media inside VIXEN_DATA"
                        }
                        // Exactly the PathField chooser's normal commit; no
                        // computed-property assignment or explicit panel reload.
                        model.commit(row.key, .path(value: file.path))
                    } else if args.first == "quality" || args.first == "metadata" {
                        @MainActor func descendants(_ view: NSView) -> [NSView] {
                            [view] + view.subviews.flatMap { descendants($0) }
                        }
                        guard let content = model.hostWindow?.contentView else { return "FAIL no editor window" }
                        let sliders = descendants(content).compactMap { $0 as? NSSlider }
                            .filter { !$0.isHiddenOrHasHiddenAncestor && $0.minValue == 10 && $0.maxValue == 100 }
                        guard sliders.count == 1, let slider = sliders.first else { return "FAIL needs one visible Video Quality slider" }
                        if args.first == "quality" {
                            guard args.count == 2, let value = Double(args[1]), value.isFinite, (10...100).contains(value),
                                  let action = slider.action else { return "FAIL needs quality10...100" }
                            slider.doubleValue = value
                            guard slider.sendAction(action, to: slider.target) else { return "FAIL quality action was not delivered" }
                        } else {
                            let observation: [String: Any] = ["selection": model.selection, "qualityControl": slider.doubleValue,
                                "undoCount": model.session.undoDescriptions().count]
                            guard let bytes = try? JSONSerialization.data(withJSONObject: observation, options: [.sortedKeys]),
                                  let text = String(data: bytes, encoding: .utf8) else { return "FAIL metadata observation" }
                            return text
                        }
                    }
                    return "selection=\(model.selection.map(String.init).joined(separator: ","))"
                }
            }
        }
        .onDisappear { model.panelClosed() }
        .onChange(of: selectedEffects) { _, effects in model.selectionChanged(effects) }
        .task {
            // Undo, the timeline and the libraries can all change what's shown.
            while !Task.isCancelled {
                try? await Task.sleep(for: .milliseconds(250))
                model.checkForOutsideEdits()
            }
        }
    }

    /// F5: always enabled, also with nothing selected.
    // STUDIO-BEGIN preview-switch: one labelled switch for the live effect preview
    private var previewBar: some View {
        Toggle(isOn: $model.previewOn) {
            Text("Preview on lights while editing")
                .foregroundStyle(.secondary)
        }
        .toggleStyle(.switch)
        .controlSize(.mini)
        .help("Shows the selected effect on the preview and real lights as you change it")
        .padding(.horizontal, StudioMetrics.spacingM)
        .padding(.vertical, StudioMetrics.spacingXS)
        .frame(maxWidth: .infinity, alignment: .leading)
    }
    // STUDIO-END preview-switch
}

/// The window the editor's dialogs open on. Read when the pane moves into a window: the dock builds the pane first, so a one-time read when
/// the view was made found no window, and double-clicking a curve or gradient did nothing.
private struct HostWindowReader: View {
    let model: EffectEditorModel

    var body: some View {
        WindowFinder { [weak model] in model?.hostWindow = $0 }
    }
}

/// F7: the effect name, bold, centred; Save as Default; Reset to Built-in; help.
// STUDIO-BEGIN inspector-title: the effect's name as the inspector title, with defaults in one labelled menu
private struct HeaderBar: View {
    let model: EffectEditorModel

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: StudioMetrics.spacingS) {
            VStack(alignment: .leading, spacing: 1) {
                Text(model.effectName)
                    .font(.title3.weight(.semibold))
                    .lineLimit(1)
                if model.selection.count > 1 {
                    Text("\(model.selection.count) effects selected")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            Spacer(minLength: 0)
            Menu("Defaults") {
                Button("Save as Default") { model.saveAsDefault() }
                    .disabled(!model.canSaveDefault)
                Button("Reset to Built-in") { model.resetDefault() }
                    .disabled(!model.canSaveDefault || !model.hasStoredDefault)
            }
            .menuStyle(.button)
            .buttonStyle(.borderless)
            .fixedSize()
            .help("Make these settings the starting point for new \(model.effectName) effects, or go back to Vixen's")
        }
        .padding(.horizontal, StudioMetrics.spacingM)
        .padding(.top, StudioMetrics.spacingS)
    }
}
// STUDIO-END inspector-title

private struct InformationBox: View {
    let model: EffectEditorModel

    var body: some View {
        if !model.information.isEmpty {
            VStack(alignment: .leading, spacing: 4) {
                Text(model.information)
                    .fixedSize(horizontal: false, vertical: true)
                    .frame(maxWidth: .infinity, alignment: .leading)
                if let url = URL(string: model.informationLink) {
                    Link("More Info", destination: url)
                }
            }
            .padding(8)
            .background(Color.primary.opacity(0.05), in: RoundedRectangle(cornerRadius: 5))
            .padding(.top, 8)
        }
    }
}

/// F13.
private struct SectionView: View {
    @Bindable var section: EditorSection
    let model: EffectEditorModel

    var body: some View {
        DisclosureGroup(isExpanded: $section.expanded) {
            VStack(alignment: .leading, spacing: 0) {
                ForEach(section.rows) { row in
                    RowView(row: row, model: model)
                }
            }
            .padding(.top, 2)
        } label: {
            Text(section.name).font(.callout.weight(.semibold)) // STUDIO: readable section titles
        }
        .padding(.vertical, 2)
    }
}

/// F15 and F16.
private struct RowView: View {
    let row: EditorRow
    let model: EffectEditorModel

    var body: some View {
        let control = row.control
        if control.isCollection {
            VStack(alignment: .center, spacing: 2) {
                Text(row.label)
                    .frame(maxWidth: .infinity)
                    .help(row.tooltip)
                CollectionEditor(row: row, model: model)
            }
            .padding(4)
        } else {
            HStack(alignment: .center, spacing: 6) {
                NameLabel(text: row.label, tooltip: row.tooltip, width: model.labelWidth)
                RowEditor(rowKey: row.key, label: row.label, control: control, readOnly: row.readOnly, model: model)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            .padding(4)
        }
    }
}

/// Right aligned, one line, an ellipsis when it's cut. The tooltip is the description (F17).
struct NameLabel: View {
    let text: String
    let tooltip: String
    let width: CGFloat

    var body: some View {
        Text(text)
            .lineLimit(1)
            .truncationMode(.tail)
            .frame(width: width, alignment: .trailing)
            .help(tooltip)
    }
}
