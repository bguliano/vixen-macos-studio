import AppKit
import SwiftUI
import VixenBridge
import VixenKit

#if canImport(VixenMarks)
import VixenMarks
#endif

/// The window's modal dialogs, as sheets on it.
enum SequencerSheet: Identifiable {
    case distribute(DistributeDialogModel)
    case editTime(clicked: UInt64, editor: ShellTimeEditor)
    case bulkMove(cursorMs: Double, lengthMs: Double)
    case sequenceLength(lengthMs: Double)
    case evenMarks(ShellTimeRange, divisions: Int)
    case addMultiple(dialog: ShellAddMultiple, typeId: String, row: UInt32)
    case missingAudio(path: String, finished: (MissingAudioAnswer) -> Void)
    case colorCollections(ShellCollectionEditor)
    case effectDefaults([ShellEffectDefault])
    case text(TextPrompt)
    case beatsAndBars
    case placeOnMarks // STUDIO: Edit ▸ Place Effect on Marks… (StudioPlaceOnMarksSheet.swift)

    var id: String {
        switch self {
        case .distribute: "distribute"
        case .editTime: "edit-time"
        case .bulkMove: "bulk-move"
        case .sequenceLength: "sequence-length"
        case .evenMarks: "even-marks"
        case .addMultiple: "add-multiple"
        case .missingAudio: "missing-audio"
        case .colorCollections: "color-collections"
        case .effectDefaults: "effect-defaults"
        case .text(let prompt): "text-\(prompt.prompt)"
        case .beatsAndBars: "beats-and-bars"
        case .placeOnMarks: "place-on-marks" // STUDIO: Place Effect on Marks sheet
        }
    }
}

/// `InvalidAudioPathDialog`. Closing it any other way leaves the default, Locate (F383).
enum MissingAudioAnswer {
    case locate
    case remove
    case keep
}

/// `TextDialog`: F395.
struct TextPrompt {
    let prompt: String
    var title: String?
    var initial = ""
    let finished: (String?) -> Void
}

struct SequencerSheetView: View {
    let model: SequencerModel
    let sheet: SequencerSheet

    var body: some View {
        switch sheet {
        case .distribute(let dialog):
            DistributeSheet(model: model, dialog: dialog)
        case .editTime(let clicked, let editor):
            EditTimeSheet(model: model, clicked: clicked, editor: editor)
        case .bulkMove(let cursorMs, let lengthMs):
            BulkMoveSheet(model: model, cursorMs: cursorMs, lengthMs: lengthMs)
        case .sequenceLength(let lengthMs):
            SequenceLengthSheet(model: model, lengthMs: lengthMs)
        case .evenMarks(let range, let divisions):
            EvenMarksSheet(model: model, range: range, initialDivisions: divisions)
        case .addMultiple(let dialog, let typeId, let row):
            AddMultipleSheet(model: model, dialog: dialog, typeId: typeId, row: row)
        case .missingAudio(let path, let finished):
            MissingAudioSheet(model: model, path: path, finished: finished)
        case .colorCollections(let editor):
            ColorCollectionsSheet(model: model, editor: editor)
        case .effectDefaults(let summaries):
            EffectDefaultsExportSheet(model: model, summaries: summaries)
        case .text(let prompt):
            TextPromptSheet(model: model, prompt: prompt)
        case .beatsAndBars:
            #if canImport(VixenMarks)
            if let session = model.session {
                BeatsAndBarsSheet(session: session) {
                    model.sheet = nil
                    model.refresh()
                }
            }
            #else
            EmptyView()
            #endif
        case .placeOnMarks: // STUDIO: Place Effect on Marks sheet
            PlaceOnMarksSheet(model: model) // STUDIO: Place Effect on Marks sheet
        }
    }
}

// MARK: Sheet furniture

private struct SheetButtons: View {
    var okLabel = "OK"
    var okEnabled = true
    let cancel: () -> Void
    let ok: () -> Void

    var body: some View {
        HStack {
            Spacer()
            Button("Cancel", action: cancel)
                .keyboardShortcut(.cancelAction)
            Button(okLabel, action: ok)
                .keyboardShortcut(.defaultAction)
                .disabled(!okEnabled)
        }
    }
}

private struct SheetTitle: View {
    let text: String

    init(_ text: String) {
        self.text = text
    }

    var body: some View {
        Text(text)
            .font(.headline)
            .frame(maxWidth: .infinity, alignment: .leading)
    }
}

// MARK: The time box, `TimeControl`

/// `mm:ss.fff` with one digit highlighted: type over it, ↑↓ rotate it (⌘ by nine, ⌥ fills the digits after it), ←→ move along. As `NSDatePicker`'s field works, with its stepper beside.
struct TimeBox: NSViewRepresentable {
    var ms: Double
    var minMs: Double = 0
    var maxMs: Double
    var isEnabled = true
    var label: String
    let onChange: (Double) -> Void

    func makeNSView(context: Context) -> TimeBoxView {
        TimeBoxView(frame: NSRect(x: 0, y: 0, width: 110, height: 24))
    }

    func updateNSView(_ view: TimeBoxView, context: Context) {
        view.onChange = onChange
        view.configure(ms: ms, minMs: minMs, maxMs: maxMs, isEnabled: isEnabled, label: label)
    }

    func sizeThatFits(_ proposal: ProposedViewSize, nsView: TimeBoxView, context: Context) -> CGSize? {
        CGSize(width: 110, height: 24)
    }
}

final class TimeBoxView: NSView {
    var onChange: ((Double) -> Void)?
    private var text = "00:00.000"
    private var position = 0
    private var minMs = 0.0
    private var maxMs = Double.greatestFiniteMagnitude
    private var isEnabled = true
    private let cell = NSTextFieldCell(textCell: "")
    private let stepper = NSStepper()
    private var stepperValue = 0.0

    override init(frame: NSRect) {
        super.init(frame: frame)
        cell.isBezeled = true
        cell.bezelStyle = .squareBezel
        cell.isEditable = false
        cell.drawsBackground = true
        cell.backgroundColor = .textBackgroundColor
        stepper.minValue = -1_000_000
        stepper.maxValue = 1_000_000
        stepper.increment = 1
        stepper.valueWraps = false
        stepper.controlSize = .small
        stepper.target = self
        stepper.action = #selector(stepped)
        stepper.doubleValue = 0
        addSubview(stepper)
        setAccessibilityElement(true)
        setAccessibilityRole(.textField)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("not used")
    }

    func configure(ms: Double, minMs: Double, maxMs: Double, isEnabled: Bool, label: String) {
        let changedLimits = self.minMs != minMs || self.maxMs != maxMs
        self.minMs = minMs
        self.maxMs = maxMs
        self.isEnabled = isEnabled
        stepper.isEnabled = isEnabled
        setAccessibilityLabel(label)
        if UITestHarness.isActive {
            UITestHarness.hooks["timeField.\(label)"] = { [weak self] args in
                guard let self else { return "FAIL no time field" }
                if args.first == "focus" { self.window?.makeFirstResponder(self) }
                return "text=\(self.text) digit=\(self.position) focused=\(self.window?.firstResponder === self)"
            }
        }
        let shown = timeBoxText(ms: ms, minMs: minMs, maxMs: maxMs)
        if shown != text || changedLimits {
            let wasEmpty = text == "00:00.000" && window == nil
            text = shown
            if wasEmpty {
                position = Int(timeBoxFirstPosition(text: text))
            }
            setAccessibilityValue(text)
            needsDisplay = true
        }
    }

    private func commit(_ new: String) {
        guard new != text else {
            needsDisplay = true
            return
        }
        text = new
        setAccessibilityValue(text)
        needsDisplay = true
        onChange?(timeBoxMs(text: new))
    }

    override var acceptsFirstResponder: Bool { isEnabled }
    override var canBecomeKeyView: Bool { isEnabled }

    override func becomeFirstResponder() -> Bool {
        needsDisplay = true
        return super.becomeFirstResponder()
    }

    override func resignFirstResponder() -> Bool {
        needsDisplay = true
        return super.resignFirstResponder()
    }

    override func layout() {
        super.layout()
        stepper.sizeToFit()
        stepper.frame.origin = NSPoint(x: bounds.width - stepper.frame.width, y: (bounds.height - stepper.frame.height) / 2)
    }

    private var fieldRect: NSRect {
        NSRect(x: 0, y: 0, width: bounds.width - stepper.frame.width - 2, height: bounds.height)
    }

    private static let font = NSFont.monospacedDigitSystemFont(ofSize: NSFont.systemFontSize, weight: .regular)

    private func textOrigin() -> NSPoint {
        let size = (text as NSString).size(withAttributes: [.font: Self.font])
        return NSPoint(x: 6, y: (bounds.height - size.height) / 2)
    }

    override func draw(_ dirtyRect: NSRect) {
        cell.isEnabled = isEnabled
        cell.stringValue = ""
        cell.draw(withFrame: fieldRect, in: self)
        let color: NSColor = isEnabled ? .textColor : .disabledControlTextColor
        let attributes: [NSAttributedString.Key: Any] = [.font: Self.font, .foregroundColor: color]
        let origin = textOrigin()
        let focused = window?.firstResponder === self
        if focused, position < text.count {
            let before = String(text.prefix(position))
            let digit = String(text[text.index(text.startIndex, offsetBy: position)])
            let x = origin.x + (before as NSString).size(withAttributes: attributes).width
            let w = (digit as NSString).size(withAttributes: attributes).width
            NSColor.selectedTextBackgroundColor.setFill()
            NSRect(x: x, y: origin.y, width: w, height: Self.font.boundingRectForFont.height).fill()
        }
        (text as NSString).draw(at: origin, withAttributes: attributes)
    }

    override func drawFocusRingMask() {
        NSBezierPath(rect: fieldRect).fill()
    }

    override var focusRingMaskBounds: NSRect { fieldRect }

    override func mouseDown(with event: NSEvent) {
        guard isEnabled else { return }
        window?.makeFirstResponder(self)
        let x = convert(event.locationInWindow, from: nil).x - textOrigin().x
        var at = text.count
        for i in 0..<text.count {
            let width = (String(text.prefix(i + 1)) as NSString).size(withAttributes: [.font: Self.font]).width
            if x < width {
                at = i
                break
            }
        }
        position = Int(timeBoxSnap(position: UInt32(at)))
        needsDisplay = true
    }

    private func delta(_ direction: Int32, _ flags: NSEvent.ModifierFlags) -> Int32 {
        if flags.contains(.command) { return direction * 9 }
        if flags.contains(.option) { return direction * 99 }
        return direction
    }

    override func keyDown(with event: NSEvent) {
        guard isEnabled else { return }
        let flags = event.modifierFlags
        switch Int(event.keyCode) {
        case 123:
            position = Int(timeBoxMove(position: UInt32(position), direction: -1))
            needsDisplay = true
        case 124:
            position = Int(timeBoxMove(position: UInt32(position), direction: 1))
            needsDisplay = true
        case 126:
            commit(timeBoxRotate(text: text, position: UInt32(position), delta: delta(1, flags), minMs: minMs, maxMs: maxMs))
        case 125:
            commit(timeBoxRotate(text: text, position: UInt32(position), delta: delta(-1, flags), minMs: minMs, maxMs: maxMs))
        case 115:
            position = 0
            needsDisplay = true
        case 119:
            position = 8
            needsDisplay = true
        case 48, 36, 76, 53:
            // Tab, Return and Esc go on to the sheet's buttons and the next field.
            super.keyDown(with: event)
        default:
            if let digit = event.characters, digit.count == 1, digit.first?.isNumber == true, !flags.contains(.command) {
                let edit = timeBoxType(text: text, position: UInt32(position), digit: digit, minMs: minMs, maxMs: maxMs)
                position = Int(edit.position)
                commit(edit.text)
            }
            // Anything else is swallowed, as `SuppressKeyPress` does.
        }
    }

    @objc private func stepped() {
        let direction: Int32 = stepper.doubleValue > stepperValue ? 1 : -1
        stepperValue = stepper.doubleValue
        window?.makeFirstResponder(self)
        commit(timeBoxRotate(text: text, position: UInt32(position), delta: delta(direction, NSApp.currentEvent?.modifierFlags ?? []), minMs: minMs, maxMs: maxMs))
    }
}

private struct TimeRow: View {
    let label: String
    var ms: Double
    var minMs: Double = 0
    var maxMs: Double
    var isEnabled = true
    let onChange: (Double) -> Void

    var body: some View {
        GridRow {
            Text(label)
                .gridColumnAlignment(.trailing)
                .foregroundStyle(isEnabled ? .primary : .secondary)
            TimeBox(ms: ms, minMs: minMs, maxMs: maxMs, isEnabled: isEnabled, label: label, onChange: onChange)
                .frame(width: 110, height: 24)
                .gridColumnAlignment(.leading)
        }
    }
}

// MARK: F379

private struct EditTimeSheet: View {
    let model: SequencerModel
    let clicked: UInt64
    let editor: ShellTimeEditor
    @State private var values: ShellTimeEditorValues?

    var body: some View {
        let v = values ?? editor.values()
        VStack(spacing: 14) {
            SheetTitle("Effect Time Editor")
            Grid(alignment: .leading, horizontalSpacing: 10, verticalSpacing: 8) {
                TimeRow(label: "Start", ms: v.startMs, maxMs: v.startMaxMs) { set { editor.setStart(ms: $0) }($0) }
                TimeRow(label: "Duration", ms: v.durationMs, minMs: v.durationMinMs, maxMs: v.durationMaxMs) { set { editor.setDuration(ms: $0) }($0) }
                TimeRow(label: "End", ms: v.endMs, minMs: v.endMinMs, maxMs: v.endMaxMs) { set { editor.setEnd(ms: $0) }($0) }
            }
            HStack {
                Button("Set") {
                    editor.matchSequence()
                    values = editor.values()
                }
                Text("Match sequence start/end")
                    .foregroundStyle(.secondary)
                Spacer()
            }
            SheetButtons(cancel: { model.sheet = nil }, ok: {
                let v = editor.values()
                model.sheet = nil
                model.applyEditTime(clicked: clicked, startMs: v.startMs, durationMs: v.durationMs)
            })
        }
        .padding(20)
        .frame(width: 320)
    }

    private func set(_ apply: @escaping (Double) -> Void) -> (Double) -> Void {
        { value in
            apply(value)
            values = editor.values()
        }
    }
}

// MARK: F380

private struct BulkMoveSheet: View {
    let model: SequencerModel
    let cursorMs: Double
    let lengthMs: Double
    @State private var startMs: Double?
    @State private var endMs: Double?
    @State private var offsetMs = 0.0
    @State private var forward = true
    @State private var visibleOnly = false
    @State private var clip = false
    @State private var marks = false

    var body: some View {
        VStack(spacing: 14) {
            SheetTitle("Bulk Effect Move")
            Grid(alignment: .leading, horizontalSpacing: 10, verticalSpacing: 8) {
                TimeRow(label: "Start Time", ms: startMs ?? cursorMs, maxMs: lengthMs) { startMs = $0 }
                TimeRow(label: "End Time", ms: endMs ?? cursorMs, maxMs: lengthMs) { endMs = $0 }
                TimeRow(label: "Offset", ms: offsetMs, maxMs: lengthMs) { offsetMs = $0 }
            }
            Picker("Direction", selection: $forward) {
                Text("Forward").tag(true)
                Text("Backward").tag(false)
            }
            .pickerStyle(.radioGroup)
            .horizontalRadioGroupLayout()
            VStack(alignment: .leading, spacing: 4) {
                Toggle("Process visible rows only", isOn: $visibleOnly)
                Toggle("Clip Effects", isOn: $clip)
                Toggle("Include Marks", isOn: $marks)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            SheetButtons(cancel: { model.sheet = nil }, ok: {
                model.sheet = nil
                model.shell?.bulkMove(
                    startMs: startMs ?? cursorMs,
                    endMs: endMs ?? cursorMs,
                    offsetMs: forward ? offsetMs : -offsetMs,
                    visibleOnly: visibleOnly,
                    clip: clip,
                    processMarks: marks
                )
                model.refresh()
            })
        }
        .padding(20)
        .frame(width: 340)
    }
}

// MARK: F381, F294

/// What F294's dialog opens with.
struct DistributeDialogModel {
    let count: Int
    let startMs: Double
    let endMs: Double
    let lengthMs: Double
}

private struct DistributeSheet: View {
    enum Duration: Hashable { case equal, keep, specified }
    enum Placement: Hashable { case stair, overlap, spaced }

    let model: SequencerModel
    let dialog: DistributeDialogModel
    @State private var startMs: Double?
    @State private var endMs: Double?
    @State private var duration = Duration.equal
    @State private var specifiedMs = 0.0
    @State private var placement = Placement.stair
    @State private var overlapMs = 0.0
    @State private var spacedMs = 0.0
    @State private var first = true

    var body: some View {
        VStack(spacing: 12) {
            SheetTitle("Effect Distribution")
            Text("Effects selected: \(dialog.count)")
                .frame(maxWidth: .infinity, alignment: .leading)
            GroupBox("Time Control") {
                Grid(alignment: .leading, horizontalSpacing: 10, verticalSpacing: 8) {
                    TimeRow(label: "Start Time", ms: startMs ?? dialog.startMs, maxMs: dialog.lengthMs) { startMs = $0 }
                    TimeRow(label: "End Time", ms: endMs ?? dialog.endMs, maxMs: dialog.lengthMs) { endMs = $0 }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(4)
            }
            GroupBox("Duration Control") {
                VStack(alignment: .leading, spacing: 6) {
                    radio("Equal Duration", .equal, $duration)
                    radio("Do not change duration", .keep, $duration)
                    HStack {
                        radio("Specified duration", .specified, $duration)
                        TimeBox(ms: specifiedMs, maxMs: dialog.lengthMs, isEnabled: duration == .specified, label: "Specified duration") { specifiedMs = $0 }
                            .frame(width: 110, height: 24)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(4)
            }
            GroupBox("Placement Control") {
                VStack(alignment: .leading, spacing: 6) {
                    radio("Stair stepped", .stair, $placement)
                    HStack {
                        radio("Overlapping", .overlap, $placement)
                        TimeBox(ms: overlapMs, maxMs: dialog.lengthMs, isEnabled: placement == .overlap, label: "Overlapping") { overlapMs = $0 }
                            .frame(width: 110, height: 24)
                    }
                    HStack {
                        radio("Spaced duration", .spaced, $placement)
                        TimeBox(ms: spacedMs, maxMs: dialog.lengthMs, isEnabled: placement == .spaced, label: "Spaced duration") { spacedMs = $0 }
                            .frame(width: 110, height: 24)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(4)
            }
            GroupBox("Starting point") {
                Picker("", selection: $first) {
                    Text("First Effect").tag(true)
                    Text("Last Effect").tag(false)
                }
                .labelsHidden()
                .pickerStyle(.radioGroup)
                .horizontalRadioGroupLayout()
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(4)
            }
            SheetButtons(cancel: { model.sheet = nil }, ok: apply)
        }
        .padding(20)
        .frame(width: 400)
    }

    private func radio<T: Hashable>(_ title: String, _ value: T, _ selection: Binding<T>) -> some View {
        Button {
            selection.wrappedValue = value
        } label: {
            HStack(spacing: 5) {
                Image(systemName: selection.wrappedValue == value ? "largecircle.fill.circle" : "circle")
                    .foregroundStyle(selection.wrappedValue == value ? Color.accentColor : Color.secondary)
                Text(title)
            }
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(selection.wrappedValue == value ? [.isButton, .isSelected] : .isButton)
    }

    private func apply() {
        model.sheet = nil
        let durationChoice: ShellDistributeDuration
        switch duration {
        case .equal: durationChoice = .equal
        case .keep: durationChoice = .doNotChange
        case .specified: durationChoice = .specified(ms: specifiedMs)
        }
        let placementChoice: ShellDistributePlacement
        switch placement {
        case .stair: placementChoice = .stairStep
        case .overlap: placementChoice = .overlap(ms: overlapMs)
        case .spaced: placementChoice = .spaced(ms: spacedMs)
        }
        model.shell?.applyDistribute(startMs: startMs ?? dialog.startMs, endMs: endMs ?? dialog.endMs, duration: durationChoice, placement: placementChoice, startWithFirst: first)
        model.refresh()
    }
}

// MARK: F382

private struct SequenceLengthSheet: View {
    let model: SequencerModel
    let lengthMs: Double
    @State private var value: Double?

    var body: some View {
        VStack(spacing: 14) {
            SheetTitle("Sequence Length")
            HStack {
                Text("Enter new sequence length:")
                TimeBox(ms: value ?? lengthMs, maxMs: Double(59 * 60_000 + 59_999), label: "Sequence length") { value = $0 }
                    .frame(width: 110, height: 24)
                Spacer()
            }
            SheetButtons(cancel: { model.sheet = nil }, ok: {
                model.sheet = nil
                if let value {
                    model.shell?.setSequenceLength(lengthMs: value)
                }
                model.refresh()
            })
        }
        .padding(20)
        .frame(width: 340)
    }
}

// MARK: F394, F82

private struct EvenMarksSheet: View {
    let model: SequencerModel
    let range: ShellTimeRange
    let initialDivisions: Int
    @State private var startMs: Double?
    @State private var endMs: Double?
    @State private var divisions: Int?

    var body: some View {
        VStack(spacing: 14) {
            SheetTitle("Create Evenly Divided Marks")
            Grid(alignment: .leading, horizontalSpacing: 10, verticalSpacing: 8) {
                TimeRow(label: "Start Time", ms: startMs ?? range.startMs, maxMs: range.lengthMs) { startMs = $0 }
                TimeRow(label: "End Time", ms: endMs ?? range.endMs, maxMs: range.lengthMs) { endMs = $0 }
                GridRow {
                    Text("Number of Divisions")
                        .gridColumnAlignment(.trailing)
                    Stepper(value: Binding(get: { divisions ?? initialDivisions }, set: { divisions = $0 }), in: 1...10_000) {
                        TextField("", value: Binding(get: { divisions ?? initialDivisions }, set: { divisions = min(10_000, max(1, $0)) }), format: .number.grouping(.never))
                            .labelsHidden()
                            .frame(width: 70)
                    }
                }
            }
            SheetButtons(cancel: { model.sheet = nil }, ok: {
                let (start, end, count) = (startMs ?? range.startMs, endMs ?? range.endMs, divisions ?? initialDivisions)
                model.sheet = nil
                model.createEvenMarks(startMs: start, endMs: end, divisions: count, range: range)
            })
        }
        .padding(20)
        .frame(width: 340)
    }
}

// MARK: F383

private struct MissingAudioSheet: View {
    let model: SequencerModel
    let path: String
    let finished: (MissingAudioAnswer) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            SheetTitle("Missing Audio File")
            Text("The audio file \(URL(fileURLWithPath: path).lastPathComponent) could not be found at the following location:")
                .fixedSize(horizontal: false, vertical: true)
            Text(path)
                .font(.callout)
                .foregroundStyle(.secondary)
                .textSelection(.enabled)
                .fixedSize(horizontal: false, vertical: true)
            Text("Selected the action you wish to take:")
            HStack {
                Spacer()
                Button("Proceed As Is") { answer(.keep) }
                Button("Forget This Audio") { answer(.remove) }
                Button("Select New Audio") { answer(.locate) }
                    .keyboardShortcut(.defaultAction)
            }
        }
        .padding(20)
        .frame(width: 460)
    }

    private func answer(_ answer: MissingAudioAnswer) {
        model.sheet = nil
        finished(answer)
    }
}

// MARK: F384 to F393, F422

private struct AddMultipleSheet: View {
    let model: SequencerModel
    let dialog: ShellAddMultiple
    let typeId: String
    let row: UInt32
    @State private var values: ShellAddMultipleValues?

    var body: some View {
        let v = values ?? dialog.values()
        VStack(alignment: .leading, spacing: 12) {
            SheetTitle(v.title)
            Text(v.possibleLabel)
                .foregroundStyle(.secondary)
                .onTapGesture(count: 2) {
                    dialog.useAllPossible()
                    reload()
                }
                .help("Double-click to use them all")
            Grid(alignment: .leading, horizontalSpacing: 10, verticalSpacing: 8) {
                GridRow {
                    Text("Number of effects to add")
                        .gridColumnAlignment(.trailing)
                    TextField("", text: Binding(get: { String(v.count) }, set: {
                        dialog.setCountText(text: $0)
                        reload()
                    }))
                    .labelsHidden()
                    .frame(width: 70)
                }
                TimeRow(label: "Starting time", ms: v.startMs, maxMs: v.lengthMs) { change { dialog.setStart(ms: $0) }($0) }
                TimeRow(label: "Ending time", ms: v.endMs, maxMs: v.lengthMs) { change { dialog.setEnd(ms: $0) }($0) }
                TimeRow(label: "Duration", ms: v.durationMs, minMs: 10, maxMs: v.lengthMs, isEnabled: v.durationEnabled) { change { dialog.setDuration(ms: $0) }($0) }
                TimeRow(label: "Duration between", ms: v.betweenMs, maxMs: v.lengthMs, isEnabled: v.betweenEnabled) { change { dialog.setBetween(ms: $0) }($0) }
            }
            DisclosureGroup(isExpanded: Binding(get: { v.panelOpen }, set: {
                dialog.setPanelOpen(on: $0)
                reload()
            })) {
                VStack(alignment: .leading, spacing: 6) {
                    Toggle("Align to beat marks", isOn: Binding(get: { v.alignToMarks }, set: { on in
                        let warning = dialog.setAlign(on: on)
                        reload()
                        if let warning, warning.count == 2 {
                            Task { await SequencerAlerts.tell(title: warning[1], message: warning[0], style: .warning, window: model.window?.attachedSheet ?? model.window) }
                        }
                    }))
                    Toggle("Skip every other beat", isOn: Binding(get: { v.skipEveryOther }, set: {
                        dialog.setSkipEveryOther(on: $0)
                        reload()
                    }))
                    .disabled(!v.skipCanToggle)
                    Toggle("Fill duration between marks", isOn: Binding(get: { v.fill }, set: {
                        dialog.setFill(on: $0)
                        reload()
                    }))
                    .disabled(!v.fillCanToggle)
                    Toggle("Align to mark start / end", isOn: Binding(get: { v.markStartEnd }, set: {
                        dialog.setMarkStartEnd(on: $0)
                        reload()
                    }))
                    .disabled(!v.markStartEndCanToggle)
                    if v.alignToMarks {
                        List(uniqueNames(v.markNames), id: \.self) { name in
                            Toggle(name, isOn: Binding(get: { v.checkedMarks.contains(name) }, set: {
                                dialog.setMarkChecked(name: name, on: $0)
                                reload()
                            }))
                        }
                        .frame(height: 110)
                        .listStyle(.bordered)
                    }
                }
                .padding(.top, 6)
            } label: {
                Text("Show beat mark alignment options")
            }
            Toggle("Select / Edit effects", isOn: Binding(get: { v.selectEffects }, set: {
                dialog.setSelectEffects(on: $0)
                reload()
            }))
            SheetButtons(okEnabled: v.okEnabled, cancel: { model.sheet = nil }, ok: ok)
        }
        .padding(20)
        .frame(width: 380)
    }

    private func uniqueNames(_ names: [String]) -> [String] {
        var seen = Set<String>()
        return names.filter { seen.insert($0).inserted }
    }

    private func reload() {
        values = dialog.values()
    }

    private func change(_ apply: @escaping (Double) -> Void) -> (Double) -> Void {
        { value in
            apply(value)
            reload()
        }
    }

    /// F392: the dialog stays open on a zero count, and on answering No.
    private func ok() {
        let host = model.window?.attachedSheet ?? model.window
        switch dialog.checkOk() {
        case .proceed:
            finish()
        case .zeroCount(let message, let title):
            Task { await SequencerAlerts.tell(title: title, message: message, style: .warning, window: host) }
        case .overrun(let message, let title):
            Task {
                if await SequencerAlerts.confirm(title: title, message: message, style: .warning, confirm: "Proceed", cancel: "Cancel", window: host) {
                    finish()
                }
            }
        }
    }

    private func finish() {
        model.sheet = nil
        _ = model.shell?.addMultipleApply(dialog: dialog, typeId: typeId, row: row)
        model.refresh()
    }
}

// MARK: F403

private struct EffectDefaultsExportSheet: View {
    let model: SequencerModel
    let summaries: [ShellEffectDefault]
    @State private var unchecked: Set<String> = []

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            SheetTitle("Export Effect Defaults")
            Text("Select saved effect defaults to export:")
            List(summaries, id: \.typeId) { item in
                Toggle(item.effectName, isOn: Binding(get: { !unchecked.contains(item.typeId) }, set: { on in
                    if on {
                        unchecked.remove(item.typeId)
                    } else {
                        unchecked.insert(item.typeId)
                    }
                }))
            }
            .listStyle(.bordered)
            .frame(height: 220)
            SheetButtons(cancel: { model.sheet = nil }, ok: {
                model.sheet = nil
                model.finishExportingEffectDefaults(summaries.map(\.typeId).filter { !unchecked.contains($0) })
            })
        }
        .padding(20)
        .frame(width: 340)
    }
}

// MARK: F395

private struct TextPromptSheet: View {
    let model: SequencerModel
    let prompt: TextPrompt
    @State private var text: String?
    @FocusState private var focused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            if let title = prompt.title {
                SheetTitle(title)
            }
            Text(prompt.prompt)
            TextField("", text: Binding(get: { text ?? prompt.initial }, set: { text = $0 }))
                .labelsHidden()
                .frame(width: 280)
                .focused($focused)
            SheetButtons(cancel: {
                model.sheet = nil
                prompt.finished(nil)
            }, ok: {
                model.sheet = nil
                prompt.finished(text ?? prompt.initial)
            })
        }
        .padding(20)
        .onAppear { focused = true }
    }
}

// MARK: F364 to F374

private struct ColorCollectionsSheet: View {
    let model: SequencerModel
    let editor: ShellCollectionEditor
    @State private var values: ShellCollectionEditorValues?
    @State private var selectedIndexes: Set<Int> = []
    @State private var colorSession: ColorPanelSession?

    var body: some View {
        let v = values ?? editor.values()
        VStack(spacing: 12) {
            SheetTitle("Color Collection Editor")
            HStack {
                Text("Color Collection")
                Picker("Color Collection", selection: Binding(get: { v.selected?.name ?? "" }, set: { name in
                    ColorPanelSession.shared.finish()
                    editor.select(name: name)
                    selectedIndexes = []
                    reload()
                })) {
                    if v.selected == nil {
                        Text("").tag("")
                    }
                    ForEach(v.names, id: \.self) { Text($0).tag($0) }
                }
                .labelsHidden()
                .frame(maxWidth: .infinity)
                Button {
                    newCollection()
                } label: {
                    Image(systemName: "plus")
                }
                .help("New collection")
                Button {
                    deleteCollection(v.selected?.name)
                } label: {
                    Image(systemName: "minus")
                }
                .help("Delete collection")
                .disabled(v.selected == nil)
            }
            HStack {
                Text("Description")
                TextField("", text: Binding(get: { v.selected?.description ?? "" }, set: {
                    editor.setDescription(text: $0)
                    reload()
                }))
                .labelsHidden()
                .disabled(v.selected == nil)
            }
            ScrollView {
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 52), spacing: 8)], spacing: 8) {
                    ForEach(Array((v.selected?.colors ?? []).enumerated()), id: \.offset) { index, argb in
                        CollectionSwatch(argb: argb, selected: selectedIndexes.contains(index))
                            .onTapGesture {
                                if selectedIndexes.contains(index) {
                                    selectedIndexes.remove(index)
                                } else {
                                    selectedIndexes.insert(index)
                                }
                            }
                            .draggable(String(index))
                            .dropDestination(for: String.self) { items, _ in
                                guard let from = items.first.flatMap({ UInt32($0) }) else { return false }
                                editor.moveColor(from: from, to: UInt32(index))
                                selectedIndexes = []
                                reload()
                                return true
                            }
                    }
                }
                .padding(8)
            }
            .frame(height: 200)
            .background(Color(nsColor: .textBackgroundColor))
            .overlay(RoundedRectangle(cornerRadius: 4).stroke(Color(nsColor: .separatorColor)))
            HStack {
                Button("Import Collection") { importCollections() }
                Button("Export Collection") { exportCollections() }
                Spacer()
                Button("Add Color") { addColor() }
                    .disabled(v.selected == nil)
                Button("Remove Color") {
                    let colors = selectedIndexes.sorted().compactMap { v.selected?.colors[safe: $0] }
                    editor.removeColors(colors: colors)
                    selectedIndexes = []
                    reload()
                }
                .disabled(selectedIndexes.isEmpty)
            }
            SheetButtons(cancel: cancel, ok: commit)
        }
        .padding(20)
        .frame(width: 520)
    }

    private func reload() {
        values = editor.values()
    }

    private var host: NSWindow? {
        model.window?.attachedSheet ?? model.window
    }

    private func commit() {
        ColorPanelSession.shared.finish()
        do {
            try editor.commit()
        } catch {
            Task { await SequencerAlerts.tell(title: "Color Collections", message: error.userMessage, style: .error, window: model.window) }
        }
        model.sheet = nil
    }

    /// F373. The Yes button keeps the edits, as OK does.
    private func cancel() {
        ColorPanelSession.shared.finish()
        guard editor.values().dirty else {
            model.sheet = nil
            return
        }
        Task {
            let keep = await SequencerAlerts.ask(
                title: "Warning",
                message: "You will loose any changes, do you wish to save them now ?",
                first: "Save",
                second: "Don't Save",
                third: "Cancel",
                window: host
            )
            switch keep {
            case .first: commit()
            case .second: model.sheet = nil
            case .third: break
            }
        }
    }

    /// F366: blank asks again; a duplicate asks about overwriting, and either answer ends.
    private func newCollection() {
        guard let host else { return }
        ColorPanelSession.shared.finish()
        KitTextPrompt.ask(on: host, prompt: "Collecton Name?") { name in
            guard let name else { return }
            switch editor.newCollection(name: name, overwrite: false) {
            case .blank:
                Task {
                    await SequencerAlerts.tell(title: "Color Collection Name", message: "Please enter a name.", style: .warning, window: host)
                    newCollection()
                }
            case .duplicate(let existing):
                Task {
                    let yes = await SequencerAlerts.confirm(
                        title: "Overwrite collection?",
                        message: "A collection with the name \(existing) already exists. Do you want to overwrite it?",
                        style: .warning,
                        confirm: "Overwrite",
                        cancel: "Cancel",
                        window: host
                    )
                    if yes {
                        _ = editor.newCollection(name: name, overwrite: true)
                        reload()
                    }
                }
            case .added:
                selectedIndexes = []
                reload()
            }
        }
    }

    private func deleteCollection(_ name: String?) {
        guard let name else { return }
        ColorPanelSession.shared.finish()
        Task {
            let yes = await SequencerAlerts.confirm(
                title: "Delete collection?",
                message: "Are you sure you want to delete the collection: \(name) ?",
                style: .warning,
                confirm: "Delete",
                cancel: "Cancel",
                window: host,
                destructive: true
            )
            if yes {
                editor.deleteSelected()
                selectedIndexes = []
                reload()
            }
        }
    }

    private func addColor() {
        ColorPanelSession.shared.finish()
        ColorPanelSession.shared.begin(title: "Add Color", argb: editor.values().lastColor, notifyUnchanged: true, onFinish: { argb in
            editor.addColor(argb: argb)
            reload()
        })
    }

    private func exportCollections() {
        ColorPanelSession.shared.finish()
        guard let url = LibraryFilePanels.save(title: "Export Color Collections", name: "Color Collections", ext: "vcc", typeName: "Vixen 3 Color Collections") else { return }
        do {
            try editor.export(path: url.path)
        } catch {
            Task { await SequencerAlerts.tell(title: "Unable to export", message: error.userMessage, style: .error, window: host) }
        }
    }

    /// F372: asks per duplicate, then says how many came in.
    private func importCollections() {
        ColorPanelSession.shared.finish()
        guard let url = LibraryFilePanels.open(title: "Import Color Collections", ext: "vcc", typeName: "Vixen 3 Color Collections") else { return }
        let host = self.host
        Task {
            do {
                let incoming = try editor.readImport(path: url.path)
                var overwrite: [String] = []
                for name in editor.importClashes(incoming: incoming) {
                    let yes = await SequencerAlerts.confirm(
                        title: "Overwrite collection?",
                        message: "A collection with the name \(name) already exists. Do you want to overwrite it?",
                        style: .warning,
                        confirm: "Overwrite",
                        cancel: "Keep Existing",
                        window: host
                    )
                    if yes {
                        overwrite.append(name)
                    }
                }
                let count = editor.import(incoming: incoming, overwrite: overwrite)
                reload()
                await SequencerAlerts.tell(title: "Color Collections Import", message: "Imported \(count) Color Collections.", style: .information, window: host)
            } catch {
                await SequencerAlerts.tell(title: "Invalid file", message: error.userMessage, style: .error, window: host)
            }
        }
    }
}

private struct CollectionSwatch: View {
    let argb: UInt32
    let selected: Bool

    var body: some View {
        RoundedRectangle(cornerRadius: 3)
            .fill(Color(nsColor: NSColor(argb: argb)))
            .frame(width: 48, height: 48)
            .overlay(RoundedRectangle(cornerRadius: 3).stroke(selected ? Color.accentColor : Color(nsColor: .separatorColor), lineWidth: selected ? 3 : 1))
            .help(rgbTooltip(argb))
    }
}

/// F341's tooltip.
func rgbTooltip(_ argb: UInt32) -> String {
    "R: \((argb >> 16) & 0xFF) G: \((argb >> 8) & 0xFF) B: \(argb & 0xFF)"
}

extension Array {
    subscript(safe index: Int) -> Element? {
        indices.contains(index) ? self[index] : nil
    }
}

// MARK: Shared panels

/// `TextDialog` where the sheet is already a sheet.
enum KitTextPrompt {
    static func ask(on window: NSWindow, prompt: String, initial: String = "", finished: @escaping (String?) -> Void) {
        KitSheet.present(on: window) { dismiss in
            TextPromptContent(prompt: prompt, initial: initial) { result in
                dismiss()
                finished(result)
            }
        }
    }
}

private struct TextPromptContent: View {
    let prompt: String
    let initial: String
    let finished: (String?) -> Void
    @State private var text: String?
    @FocusState private var focused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(prompt)
            TextField("", text: Binding(get: { text ?? initial }, set: { text = $0 }))
                .labelsHidden()
                .frame(width: 280)
                .focused($focused)
            SheetButtons(cancel: { finished(nil) }, ok: { finished(text ?? initial) })
        }
        .padding(20)
        .onAppear { focused = true }
    }
}

/// Open and save panels for the library files. Under the harness a script names the file, since panels can't be answered there.
enum LibraryFilePanels {
    private static var scripted: String? {
        UITestHarness.isActive ? ProcessInfo.processInfo.environment["VIXEN_TEST_FILE"] : nil
    }

    static func open(title: String, ext: String, typeName: String) -> URL? {
        if UITestHarness.isActive {
            return scripted.map { URL(fileURLWithPath: $0) }
        }
        let panel = NSOpenPanel()
        panel.title = title
        panel.message = "\(typeName) (*.\(ext))"
        panel.allowsMultipleSelection = false
        panel.canChooseDirectories = false
        return panel.runModal() == .OK ? panel.url : nil
    }

    static func save(title: String, name: String, ext: String, typeName: String) -> URL? {
        if UITestHarness.isActive {
            return scripted.map { URL(fileURLWithPath: $0) }
        }
        let panel = NSSavePanel()
        panel.title = title
        panel.message = "\(typeName) (*.\(ext))"
        panel.nameFieldStringValue = "\(name).\(ext)"
        panel.allowsOtherFileTypes = true
        return panel.runModal() == .OK ? panel.url : nil
    }
}
