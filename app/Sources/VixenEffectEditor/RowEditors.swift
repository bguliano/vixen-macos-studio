import AppKit
import SwiftUI
import UniformTypeIdentifiers
import VixenBridge
import VixenKit

/// F21's result: the editor a row gets, by what Rust worked out.
struct RowEditor: View {
    let rowKey: EeRowKey
    let label: String
    let control: EeRowControl
    let readOnly: Bool
    let model: EffectEditorModel

    var body: some View {
        switch control {
        case let .check(value):
            Toggle(
                label,
                isOn: Binding(get: { value }, set: { model.commit(rowKey, .bool(value: $0)) })
            )
            .toggleStyle(.checkbox)
            .labelsHidden()
            .disabled(readOnly)
        case let .choice(labels, selected):
            ChoicePicker(labels: labels, selected: selected) { model.commit(rowKey, .choice(index: $0)) }
                .disabled(readOnly)
        case let .number(text, limits):
            DragNumberField(text: text, limits: limits, isEnabled: !readOnly) { typed, gesture in
                model.commit(rowKey, .text(value: typed), gesture: gesture)
            }
        case let .slider(value, min, max, tick, snap, tooltip):
            ValueSlider(value: value, range: min...Swift.max(min, max), tick: tick, snap: snap, tooltip: tooltip, isEnabled: !readOnly) { shown, gesture in
                model.commit(rowKey, .slider(value: shown), gesture: gesture)
            }
        case let .text(value):
            CommitTextField(text: value, isEnabled: !readOnly) { model.commit(rowKey, .text(value: $0)) }
        case let .label(value):
            Text(value).textSelection(.enabled)
        case let .path(value, titleIsRowName, filterName, extensions):
            PathField(value: value, title: titleIsRowName ? label : nil, filterName: filterName, extensions: extensions, readOnly: readOnly) {
                model.commit(rowKey, .path(value: $0))
            }
        case let .font(text, name, size, bold, italic):
            FontField(text: text, font: KitFont(name: name, size: size, bold: bold, italic: italic), readOnly: readOnly) { chosen in
                model.commit(rowKey, .font(name: chosen.name, size: chosen.size, bold: chosen.bold, italic: chosen.italic))
            }
        case let .color(argb, text, valid):
            ColorField(rowKey: rowKey, title: label, argb: argb, text: text, valid: valid, readOnly: readOnly, model: model)
        case let .curve(value):
            CurveRowStrip(rowKey: rowKey, title: label, value: value, readOnly: readOnly, model: model)
        case let .gradient(value, discrete, valid):
            GradientRowStrip(rowKey: rowKey, title: label, value: value, discrete: discrete, valid: valid, readOnly: readOnly, model: model)
        case .polygons:
            if !readOnly {
                Button("Edit Polygons") { editPolygons() }
            }
        case .gradientList, .colorList, .gradientLevels, .stringList, .recordList:
            EmptyView()
        }
    }

    /// F37: the polygon editor is another worker's, and plugs in through `EffectEditorHooks.editPolygons`.
    private func editPolygons() {
        guard let edit = EffectEditorHooks.editPolygons else {
            polygonEditorMissing()
            return
        }
        PendingCommits.flushAll()
        let request = PolygonEditRequest(session: model.session, effects: model.selection, param: rowKey.param, window: model.hostWindow) { label in
            model.externalCommitFinished(undoLabel: label)
        }
        edit(request)
    }

    private func polygonEditorMissing() {
        guard let window = model.hostWindow else { return }
        Task { @MainActor in
            _ = await askInSheet(on: window, message: "The polygon editor isn't part of this build yet.", title: "Polygon Error", buttons: ["OK"], style: .informational)
        }
    }
}

/// What other areas plug in here.
@MainActor
public enum EffectEditorHooks {
    /// F37. The polygon editor is another worker's: it sets this.
    public static var editPolygons: ((PolygonEditRequest) -> Void)?
}

/// F37's Edit Polygons, for the polygon editor. Upstream clones First's container, opens the modal editor on it, and on OK writes a new container with the edited shapes and their times,
/// keeping width and height; any failure says "An error occurred loading the polygon editor.", titled "Polygon Error". The shapes are `MorphPolygonData`'s rows, and like every list
/// edit (F62) OK gives each selected effect a copy of First's.
@MainActor
public struct PolygonEditRequest {
    public let session: SequencerSession
    /// The selection, First first.
    public let effects: [UInt64]
    /// The row's property: `PolygonContainer`.
    public let param: String
    /// For the editor's sheet.
    public let window: NSWindow?
    /// Call once the editor closes. After OK, with the label of the one undo step it put on the session's stack; after Cancel, or OK with nothing changed, with `nil`.
    public let finished: (String?) -> Void
}

/// F27 and F29: a menu, blank when nothing matches. Choosing commits.
struct ChoicePicker: View {
    let labels: [String]
    let selected: UInt32?
    let onChoose: (UInt32) -> Void

    var body: some View {
        Picker(
            "",
            selection: Binding<Int>(
                get: { selected.map(Int.init) ?? -1 },
                set: { if $0 >= 0 { onChoose(UInt32($0)) } }
            )
        ) {
            if selected == nil {
                Text("").tag(-1)
            }
            ForEach(Array(labels.enumerated()), id: \.offset) { index, label in
                Text(label).tag(index)
            }
        }
        .pickerStyle(.menu)
        .labelsHidden()
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

/// F31 and F54: commits on Return and on leaving the field; Escape reverts; nothing while typing.
struct CommitTextField: NSViewRepresentable {
    let text: String
    var isEnabled = true
    /// The field took the focus: F54's FIX has a click in a box select its row.
    var onFocus: (() -> Void)?
    let onCommit: (String) -> Void

    func makeCoordinator() -> Coordinator {
        Coordinator()
    }

    func makeNSView(context: Context) -> NSTextField {
        let field = FocusReportingTextField()
        field.delegate = context.coordinator
        field.controlSize = .small
        field.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        field.lineBreakMode = .byClipping
        field.cell?.isScrollable = true
        return field
    }

    func updateNSView(_ field: NSTextField, context: Context) {
        context.coordinator.valueText = text
        context.coordinator.onCommit = onCommit
        (field as? FocusReportingTextField)?.onFocus = onFocus
        field.isEnabled = isEnabled
        if field.currentEditor() == nil, field.stringValue != text {
            field.stringValue = text
        }
    }

    func sizeThatFits(_ proposal: ProposedViewSize, nsView: NSTextField, context: Context) -> CGSize? {
        CGSize(width: proposal.width ?? 120, height: nsView.intrinsicContentSize.height)
    }

    final class Coordinator: NSObject, NSTextFieldDelegate {
        var valueText = ""
        var onCommit: ((String) -> Void)?

        func control(_ control: NSControl, textView: NSTextView, doCommandBy commandSelector: Selector) -> Bool {
            guard let field = control as? NSTextField else { return false }
            if commandSelector == #selector(NSResponder.insertNewline(_:)) {
                commit(field)
                return true
            }
            if commandSelector == #selector(NSResponder.cancelOperation(_:)) {
                field.stringValue = valueText
                field.window?.makeFirstResponder(nil)
                return true
            }
            return false
        }

        func controlTextDidEndEditing(_ note: Notification) {
            if let field = note.object as? NSTextField {
                commit(field)
            }
        }

        private func commit(_ field: NSTextField) {
            if field.stringValue != valueText {
                onCommit?(field.stringValue)
            }
        }
    }
}

private final class FocusReportingTextField: NSTextField {
    var onFocus: (() -> Void)?

    override func becomeFirstResponder() -> Bool {
        let became = super.becomeFirstResponder()
        if became {
            onFocus?()
        }
        return became
    }
}

/// F34: a read-only box with the path, tooltip the same text, and a **...** button that hides when read-only.
private struct PathField: View {
    let value: String
    let title: String?
    let filterName: String
    let extensions: [String]
    let readOnly: Bool
    let onChoose: (String) -> Void
    @State private var hostReference = WeakWindow()
    private var hostWindow: NSWindow? { hostReference.window }

    var body: some View {
        HStack(spacing: 4) {
            Text(value)
                .lineLimit(1)
                .truncationMode(.middle)
                .padding(.horizontal, 4)
                .frame(maxWidth: .infinity, minHeight: 18, alignment: .leading)
                .background(Color.primary.opacity(0.06), in: RoundedRectangle(cornerRadius: 3))
                .help(value)
            if !readOnly {
                Button("...") { choose() }
                    .frame(width: 26)
            }
        }
        .background(WindowFinder(into: hostReference))
    }

    private func choose() {
        let panel = NSOpenPanel()
        panel.allowsMultipleSelection = false
        panel.canChooseDirectories = false
        if let title {
            panel.message = title
        }
        let types = extensions.compactMap { UTType(filenameExtension: $0) }
        if !types.isEmpty {
            panel.allowedContentTypes = types
        }
        if let hostWindow {
            panel.beginSheetModal(for: hostWindow) { response in
                if response == .OK, let url = panel.url {
                    onChoose(url.path)
                }
            }
        }
    }
}

/// F35: a read-only box with the font's text, and a **...** button that opens the font panel.
private struct FontField: View {
    let text: String
    let font: KitFont
    let readOnly: Bool
    let onChoose: (KitFont) -> Void

    var body: some View {
        HStack(spacing: 4) {
            Text(text)
                .lineLimit(1)
                .padding(.horizontal, 4)
                .frame(maxWidth: .infinity, minHeight: 18, alignment: .leading)
                .background(Color.primary.opacity(0.06), in: RoundedRectangle(cornerRadius: 3))
                .help(text)
            if !readOnly {
                Button("...") { FontPanelSession.shared.begin(font: font, onFinish: onChoose) }
                    .frame(width: 26)
            }
        }
    }
}

/// F36: a swatch you double-click, then the value's text.
private struct ColorField: View {
    let rowKey: EeRowKey
    let title: String
    let argb: UInt32
    let text: String
    let valid: [UInt32]
    let readOnly: Bool
    let model: EffectEditorModel
    @State private var live: UInt32?

    var body: some View {
        HStack(spacing: 6) {
            ColorSwatch(
                argb: live ?? argb,
                isEnabled: !readOnly,
                onOpen: { openColorEditor(title: title, argb: argb, valid: valid, model: model, live: { live = $0 }) { model.commit(rowKey, .color(argb: $0)) } },
                acceptsDrop: { model.canDrop(rowKey, $0) },
                performDrop: { model.drop(rowKey, $0) }
            )
            .frame(width: 25, height: 25)
            Text(text).lineLimit(1).monospacedDigit()
        }
    }
}

/// F36's Open: a discrete picker for discrete targets, otherwise the colour panel, one commit when it's done.
func openColorEditor(title: String, argb: UInt32, valid: [UInt32], model: EffectEditorModel, live: ((UInt32?) -> Void)? = nil, onDone: @escaping (UInt32) -> Void) {
    if !valid.isEmpty {
        guard let window = model.hostWindow else { return }
        DiscreteColorPicker.pickOne(on: window, colors: valid, current: argb, onChosen: onDone)
        return
    }
    let start: UInt32 = (argb >> 24) == 0 ? 0xFFFF_FFFF : argb
    ColorPanelSession.shared.begin(title: title, argb: start, onLive: { live?($0) }, onEnd: { live?(nil) }, onFinish: { chosen in
        onDone(chosen)
    })
}

/// F38 to F43 for a row, with the dialog behind a double-click.
struct CurveRowStrip: View {
    let rowKey: EeRowKey
    let title: String
    let value: CurveValue
    let readOnly: Bool
    let model: EffectEditorModel
    var onPressed: (() -> Void)?

    var body: some View {
        CurveStrip(
            value: value,
            isEnabled: !readOnly,
            onCommit: { model.commit(rowKey, .curve(value: $0)) },
            onOpenEditor: openEditor,
            acceptsDrop: { model.canDrop(rowKey, $0) },
            performDrop: { model.drop(rowKey, $0) },
            onPressed: onPressed
        )
    }

    private func openEditor() {
        guard let window = model.hostWindow else { return }
        PendingCommits.flushAll()
        CurveEditorDialog.present(on: window, core: model.core, curve: value, title: title) { edited in
            model.commit(rowKey, .curve(value: edited))
        }
    }
}

/// F44 to F50 for a row.
struct GradientRowStrip: View {
    let rowKey: EeRowKey
    let title: String
    let value: GradientValue
    let discrete: Bool
    let valid: [UInt32]
    let readOnly: Bool
    let model: EffectEditorModel
    var onPressed: (() -> Void)?
    var opensOnReturn = true

    var body: some View {
        GradientStrip(
            value: value,
            discrete: discrete,
            validColors: valid,
            isEnabled: !readOnly,
            opensOnReturn: opensOnReturn,
            onCommit: { model.commit(rowKey, .gradient(value: $0)) },
            onOpenEditor: openEditor,
            onEditStop: { handle in
                GradientStopColorEdit.edit(handle: handle, value: value, discrete: discrete, validColors: valid, window: model.hostWindow) {
                    model.commit(rowKey, .gradient(value: $0))
                }
            },
            acceptsDrop: { model.canDrop(rowKey, $0) },
            performDrop: { model.drop(rowKey, $0) },
            onPressed: onPressed
        )
    }

    private func openEditor() {
        guard let window = model.hostWindow else { return }
        PendingCommits.flushAll()
        GradientEditorDialog.present(on: window, core: model.core, gradient: value, discrete: discrete, validColors: valid, title: title) { edited in
            model.commit(rowKey, .gradient(value: edited))
        }
    }
}
