import SwiftUI
import VixenBridge

/// Wire Diagram Export Options (F128). `onFinish` gets the controls' values at OK, for `exportWireDiagram`, and nil at Cancel.
public struct WireDiagramSheet: View {
    private let onFinish: (SetupWireOptions?) -> Void
    @State private var options = setupWireDefaultOptions()
    @State private var finished = false
    // STUDIO-BEGIN wire-sheet-verb: a caller can put its verb ("Export") on the confirmation button
    private let verb: String

    public init(verb: String = "OK", onFinish: @escaping (SetupWireOptions?) -> Void) {
        self.onFinish = onFinish
        self.verb = verb
    }
    // STUDIO-END wire-sheet-verb

    private func accept() {
        if setupWireOptionsError(options: options) == nil {
            finish(options)
        }
    }

    /// Return and Esc can each arrive by two routes (`sheetKeys`).
    private func finish(_ result: SetupWireOptions?) {
        guard !finished else { return }
        finished = true
        onFinish(result)
    }

    public var body: some View {
        let problem = setupWireOptionsError(options: options)
        VStack(alignment: .leading, spacing: 14) {
            Text("Wire Diagram Export Options").font(.headline)
            Toggle("Show all node numbers", isOn: $options.allNumbers)
            VStack(alignment: .leading, spacing: 4) {
                HStack {
                    Text("Number Label Interval")
                    Spacer()
                    TextField("Number Label Interval", text: $options.intervalText, prompt: Text(""))
                        .labelsHidden()
                        .multilineTextAlignment(.trailing)
                        .monospacedDigit()
                        .frame(width: 80)
                        .disabled(options.allNumbers)
                }
                // Always there, so the sheet doesn't change height as the message comes and goes.
                Text(problem ?? " ")
                    .font(.callout)
                    .foregroundStyle(.red)
            }
            WireSliderRow(title: "Node Marker Size", value: $options.markerSize, range: 1...30)
            WireSliderRow(title: "Number Label Size", value: $options.labelSize, range: 6...24)
            WireSliderRow(title: "Scaling", value: $options.scalePercent, range: 100...400, suffix: "%")
            Text("The diagram is fitted to a letter-sized page. Scaling scales the entire result, including the page, labels, markers, and lines.")
                .fixedSize(horizontal: false, vertical: true)
            SheetButtons(okTitle: verb, okDisabled: problem != nil, ok: accept, cancel: { finish(nil) }) // STUDIO: the caller's verb
        }
        .padding(16)
        .frame(width: 440)
        .sheetKeys(okEnabled: problem == nil, ok: accept, cancel: { finish(nil) })
    }
}

/// A title with the value at its right, over a slider in whole steps.
private struct WireSliderRow: View {
    let title: String
    @Binding var value: Int32
    let range: ClosedRange<Double>
    var suffix = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text(title)
                Spacer()
                Text("\(value)\(suffix)").monospacedDigit()
            }
            // Rounded here rather than with `step:`, which on a Mac draws a tick per step: 301 of them on Scaling.
            Slider(value: Binding(get: { Double(value) }, set: { value = Int32($0.rounded()) }), in: range)
                .accessibilityLabel(title)
        }
    }
}
