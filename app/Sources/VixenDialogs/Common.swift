import AppKit
import SwiftUI
import VixenBridge
import VixenKit

/// A sheet has no title bar, so upstream's window title goes at the top.
struct DialogTitle: View {
    let text: String

    init(_ text: String) {
        self.text = text
    }

    var body: some View {
        Text(text).font(.headline)
    }
}

/// OK then Cancel at the bottom right, as upstream has them. Return is OK and Esc is Cancel on every sheet.
struct SheetButtons: View {
    var okTitle = "OK"
    var okDisabled = false
    let onOK: () -> Void
    let onCancel: () -> Void

    var body: some View {
        HStack {
            Spacer()
            Button(okTitle, action: onOK)
                .keyboardShortcut(.defaultAction)
                .disabled(okDisabled)
            Button("Cancel", action: onCancel)
                .keyboardShortcut(.cancelAction)
        }
    }
}

/// An alert with upstream's title and text, and verbs on the buttons.
struct DialogAlert: Identifiable {
    struct Choice {
        var title: String
        var role: ButtonRole?
        var action: () -> Void

        init(_ title: String, role: ButtonRole? = nil, action: @escaping () -> Void = {}) {
            self.title = title
            self.role = role
            self.action = action
        }
    }

    let id = UUID()
    var title: String
    var message: String
    var choices: [Choice]

    /// A message with only an OK button.
    static func notice(_ title: String, _ message: String, then: @escaping () -> Void = {}) -> DialogAlert {
        DialogAlert(title: title, message: message, choices: [Choice("OK", action: then)])
    }
}

extension View {
    func dialogAlert(_ alert: Binding<DialogAlert?>) -> some View {
        let isPresented = Binding(get: { alert.wrappedValue != nil }, set: { if !$0 { alert.wrappedValue = nil } })
        return self.alert(alert.wrappedValue?.title ?? "", isPresented: isPresented, presenting: alert.wrappedValue) { shown in
            ForEach(Array(shown.choices.enumerated()), id: \.offset) { _, choice in
                Button(choice.title, role: choice.role, action: choice.action)
            }
        } message: { shown in
            Text(shown.message)
        }
    }
}

/// The system colour panel on an ARGB value.
struct ArgbColorPicker: View {
    @Binding var argb: UInt32
    var label = ""

    var body: some View {
        ColorPicker(
            label,
            selection: Binding(get: { Color(argb: argb) }, set: { argb = NSColor($0).argb }),
            supportsOpacity: false
        )
        .labelsHidden()
    }
}

/// An `Int32` in a text field with a stepper, for upstream's number up/downs.
struct NumberField: View {
    @Binding var value: Int32
    let range: ClosedRange<Int32>
    var width: CGFloat = 90

    var body: some View {
        HStack(spacing: 4) {
            TextField("", value: Binding(get: { value }, set: { value = $0.clamped(to: range) }), format: .number.grouping(.never))
                .labelsHidden()
                .multilineTextAlignment(.trailing)
                .monospacedDigit()
                .frame(width: width)
            Stepper("", value: $value, in: range)
                .labelsHidden()
        }
    }
}

/// A slider on whole numbers, with tick marks where upstream's `TrackBar` has them. `NSSlider`, since SwiftUI's with a step of 1 draws a tick per value.
struct IntSlider: NSViewRepresentable {
    @Binding var value: Int32
    let range: ClosedRange<Int32>
    /// How far apart the ticks are. Nil has none.
    var tickEvery: Int32?
    var showsValueInTooltip = true

    func makeNSView(context: Context) -> NSSlider {
        let slider = NSSlider(value: Double(value), minValue: Double(range.lowerBound), maxValue: Double(range.upperBound), target: context.coordinator, action: #selector(Coordinator.changed(_:)))
        slider.isContinuous = true
        slider.controlSize = .small
        if let tickEvery {
            slider.numberOfTickMarks = Int((range.upperBound - range.lowerBound) / tickEvery) + 1
            slider.allowsTickMarkValuesOnly = false
        }
        return slider
    }

    func updateNSView(_ slider: NSSlider, context: Context) {
        context.coordinator.parent = self
        slider.isEnabled = context.environment.isEnabled
        if Int32(slider.doubleValue.rounded()) != value {
            slider.doubleValue = Double(value)
        }
        slider.toolTip = showsValueInTooltip ? String(value) : nil
    }

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    @MainActor
    final class Coordinator: NSObject {
        var parent: IntSlider

        init(_ parent: IntSlider) {
            self.parent = parent
        }

        @objc func changed(_ slider: NSSlider) {
            parent.value = Int32(slider.doubleValue.rounded())
        }
    }
}

/// Upstream's tabs as a segmented control, as the Mac draws them: `TabView` in a sheet can't be relied on to size its bar.
struct SheetTabs<Tag: Hashable>: View {
    @Binding var selection: Tag
    let tabs: [(String, Tag)]

    var body: some View {
        Picker("", selection: $selection) {
            ForEach(tabs, id: \.1) { Text($0.0).tag($0.1) }
        }
        .pickerStyle(.segmented)
        .labelsHidden()
        .fixedSize()
        .frame(maxWidth: .infinity)
    }
}

/// `NumberDialog`: Return is OK, and Esc is Cancel.
public struct NumberPromptSheet: View {
    let title: String
    let prompt: String
    let range: ClosedRange<Int32>
    let onFinish: (Int32?) -> Void
    @State private var value: Int32
    // STUDIO-BEGIN dialog-prompt-verb: the caller's verb on the confirmation ("Add"), never OK (B9)
    private var verb = "OK"

    public init(title: String, prompt: String, value: Int32, range: ClosedRange<Int32>, verb: String, onFinish: @escaping (Int32?) -> Void) {
        self.init(title: title, prompt: prompt, value: value, range: range, onFinish: onFinish)
        self.verb = verb
    }
    // STUDIO-END dialog-prompt-verb

    public init(title: String, prompt: String, value: Int32, range: ClosedRange<Int32>, onFinish: @escaping (Int32?) -> Void) {
        self.title = title
        self.prompt = prompt
        self.range = range
        self.onFinish = onFinish
        _value = State(initialValue: value)
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            DialogTitle(title)
            Text(prompt)
            NumberField(value: $value, range: range)
            StudioVerbButtons(verb: verb, onConfirm: { onFinish(value) }, onCancel: { onFinish(nil) }) // STUDIO: the caller's verb (B9)
        }
        .padding(20)
        .frame(minWidth: 280)
    }
}

/// `TextDialog`. `validate` returns what's wrong with the text, which keeps OK off, and `warn` what's worth saying without stopping it.
public struct TextPromptSheet: View {
    let title: String
    let prompt: String
    let validate: (String) -> String?
    let warn: (String) -> String?
    let onFinish: (String?) -> Void
    @State private var text: String
    @FocusState private var focused: Bool
    // STUDIO-BEGIN dialog-text-verb: the caller's verb on the confirmation ("Create"), never OK (B9)
    private var verb = "OK"

    public init(
        title: String,
        prompt: String,
        text: String = "",
        verb: String,
        validate: @escaping (String) -> String? = { _ in nil },
        warn: @escaping (String) -> String? = { _ in nil },
        onFinish: @escaping (String?) -> Void
    ) {
        self.init(title: title, prompt: prompt, text: text, validate: validate, warn: warn, onFinish: onFinish)
        self.verb = verb
    }
    // STUDIO-END dialog-text-verb

    public init(
        title: String,
        prompt: String,
        text: String = "",
        validate: @escaping (String) -> String? = { _ in nil },
        warn: @escaping (String) -> String? = { _ in nil },
        onFinish: @escaping (String?) -> Void
    ) {
        self.title = title
        self.prompt = prompt
        self.validate = validate
        self.warn = warn
        self.onFinish = onFinish
        _text = State(initialValue: text)
    }

    public var body: some View {
        let problem = validate(text)
        VStack(alignment: .leading, spacing: 10) {
            DialogTitle(title)
            Text(prompt)
            TextField("", text: $text)
                .labelsHidden()
                .focused($focused)
            if let problem {
                Text(problem).font(.caption).foregroundStyle(.red)
            } else if let warning = warn(text) {
                Text(warning).font(.caption).foregroundStyle(.secondary)
            }
            StudioVerbButtons(verb: verb, disabled: problem != nil, onConfirm: { onFinish(text) }, onCancel: { onFinish(nil) }) // STUDIO: the caller's verb (B9)
        }
        .padding(20)
        .frame(width: 360)
        .onAppear { focused = true }
    }
}

/// `J1MsgBox`: a heading, scrolling text that can be selected, and OK (F93).
public struct ScrollingMessageSheet: View {
    let heading: String
    let message: String
    let onFinish: () -> Void

    public init(heading: String, message: String, onFinish: @escaping () -> Void) {
        self.heading = heading
        self.message = message
        self.onFinish = onFinish
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(heading)
            ScrollView {
                Text(message)
                    .textSelection(.enabled)
                    .monospacedDigit()
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(6)
            }
            .frame(maxWidth: 600, maxHeight: 400)
            .background(.quaternary.opacity(0.4))
            HStack {
                Spacer()
                Button("OK", action: onFinish).keyboardShortcut(.defaultAction)
            }
        }
        .padding(20)
        .frame(minWidth: 360)
    }
}

/// `SingleDiscreteColorPickerView`: only the colours an element can show, each 50 x 50.
public struct DiscreteColorSheet: View {
    let colors: [UInt32]
    let onFinish: (UInt32?) -> Void
    @State private var selected: UInt32?

    public init(colors: [UInt32], current: UInt32, onFinish: @escaping (UInt32?) -> Void) {
        self.colors = colors
        self.onFinish = onFinish
        _selected = State(initialValue: colors.contains(current) ? current : colors.first)
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            DialogTitle("Select Color")
            LazyVGrid(columns: Array(repeating: GridItem(.fixed(54)), count: min(max(colors.count, 1), 6)), spacing: 6) {
                ForEach(colors, id: \.self) { color in
                    Button {
                        selected = color
                    } label: {
                        RoundedRectangle(cornerRadius: 4)
                            .fill(Color(argb: color))
                            .frame(width: 50, height: 50)
                            .overlay(
                                RoundedRectangle(cornerRadius: 4)
                                    .stroke(selected == color ? Color.accentColor : Color.secondary.opacity(0.5), lineWidth: selected == color ? 3 : 1)
                            )
                    }
                    .buttonStyle(.plain)
                }
            }
            StudioVerbButtons(verb: "Choose", disabled: selected == nil, onConfirm: { onFinish(selected) }, onCancel: { onFinish(nil) }) // STUDIO: a verb, not Ok (B9)
        }
        .padding(20)
    }
}

/// The full colour choice as a sheet, for a double-click on a colour cell: the system panel, with OK and Cancel.
struct FreeColorSheet: View {
    let onFinish: (UInt32?) -> Void
    @State private var color: UInt32

    init(current: UInt32, onFinish: @escaping (UInt32?) -> Void) {
        self.onFinish = onFinish
        _color = State(initialValue: current)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            DialogTitle("Select Color")
            ArgbColorPicker(argb: $color)
                .controlSize(.large)
            StudioVerbButtons(verb: "Choose", onConfirm: { onFinish(color) }, onCancel: { onFinish(nil) }) // STUDIO: a verb, not OK (B9)
        }
        .padding(20)
        .frame(minWidth: 240)
    }
}

/// Runs slow Rust off the main thread.
func offMain<T: Sendable>(_ work: @escaping @Sendable () -> T) async -> T {
    await Task.detached(priority: .userInitiated, operation: work).value
}
