import AppKit
import SwiftUI
import VixenBridge
import VixenKit

/// Lower and Upper brightness rows, shared by Chroma Key and Luma Key (F216, F219). A slider and a number box each, kept in step by Rust.
struct BrightnessRows: View {
    let context: DialogContext
    @Binding var limits: DialogLimits
    /// F219: Luma Key's have none.
    var tooltips = true

    var body: some View {
        Grid(alignment: .leading, verticalSpacing: 6) {
            row("Lower", value: limits.lower, range: 0...99, end: .lower)
            row("Upper", value: limits.upper, range: 1...100, end: .upper)
        }
    }

    private func row(_ title: String, value: Int32, range: ClosedRange<Int32>, end: DialogLimitEnd) -> some View {
        GridRow {
            Text(title)
            IntSlider(value: Binding(get: { value }, set: { set($0, end) }), range: range, tickEvery: 5, showsValueInTooltip: tooltips)
                .frame(minWidth: 180)
            TextField("", value: Binding(get: { value }, set: { set($0, end) }), format: .number.grouping(.never))
                .labelsHidden()
                .multilineTextAlignment(.trailing)
                .monospacedDigit()
                .frame(width: 46)
        }
    }

    private func set(_ value: Int32, _ end: DialogLimitEnd) {
        var moved = limits
        switch end {
        case .lower: moved.lower = value
        case .upper: moved.upper = value
        }
        limits = context.clampLimits(limits: moved, changed: end)
    }
}

/// Chroma Key Configuration (F212 to F218). `onFinish` gets the result, or nil for Cancel.
public struct ChromaKeySheet: View {
    let context: DialogContext
    let onFinish: (DialogChromaKey?) -> Void
    @State private var draft: DialogChromaKey
    // STUDIO-BEGIN layer-chroma-live: the Layer Editor applies each change to the layer as it's made, with Undo while the sheet is up (B9)
    private var studioApply: (DialogChromaKey) throws -> Void = { _ in }
    @State private var studio = StudioLiveSheet(step: "Configure Layer")

    /// `apply` writes a change to the layer; Done hands back the last value when anything changed, nil otherwise.
    public init(context: DialogContext, draft: DialogChromaKey, apply: @escaping (DialogChromaKey) throws -> Void, onFinish: @escaping (DialogChromaKey?) -> Void) {
        self.init(context: context, draft: draft, onFinish: onFinish)
        studioApply = apply
    }
    // STUDIO-END layer-chroma-live

    public init(context: DialogContext, draft: DialogChromaKey, onFinish: @escaping (DialogChromaKey?) -> Void) {
        self.context = context
        self.onFinish = onFinish
        _draft = State(initialValue: draft)
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            DialogTitle("Chroma Key Configuration")
            HStack(alignment: .top, spacing: 24) {
                VStack(alignment: .leading, spacing: 6) {
                    Text("Color")
                    ArgbColorPicker(argb: $draft.key)
                        .controlSize(.large)
                        .help("Click to Select the Key Color")
                }
                VStack(alignment: .leading, spacing: 6) {
                    Text("Tolerance").fontWeight(.medium)
                    tolerance("Hue", value: $draft.hueTolerance, edited: $draft.hueToleranceEdited, range: 0...180, ticks: 20)
                    tolerance("Saturation", value: $draft.saturationPercent, edited: $draft.saturationToleranceEdited, range: 0...99, ticks: 10)
                }
            }
            VStack(alignment: .leading, spacing: 6) {
                Text("Brightness Range").fontWeight(.medium)
                BrightnessRows(context: context, limits: $draft.limits)
            }
            Toggle("Treat Zero Brightness as Transparent.", isOn: $draft.transparent)
                .toggleStyle(.checkbox)
                .help("Determines if the layer should pass the underlying color when the Chroma Key layer's effect(s) have a zero brightness color.")
            StudioDoneButton(studio) { onFinish($0 ? draft : nil) } // STUDIO: live, closes with Done (B9)
        }
        .padding(20)
        .frame(width: 460)
        .studioLive(studio, draft: $draft, apply: studioApply) // STUDIO: live (B9)
    }

    private func tolerance(_ title: String, value: Binding<Int32>, edited: Binding<Bool>, range: ClosedRange<Int32>, ticks: Int32) -> some View {
        HStack {
            Text(title).frame(width: 70, alignment: .leading)
            IntSlider(value: Binding(get: { value.wrappedValue }, set: {
                value.wrappedValue = $0
                edited.wrappedValue = true
            }), range: range, tickEvery: ticks)
        }
    }
}

/// Luma Key Configuration (F219). `onFinish` gets the result, or nil for Cancel.
public struct LumaKeySheet: View {
    let context: DialogContext
    let onFinish: (DialogLimits?) -> Void
    @State private var limits: DialogLimits
    // STUDIO-BEGIN layer-luma-live: the Layer Editor applies each change to the layer as it's made, with Undo while the sheet is up (B9)
    private var studioApply: (DialogLimits) throws -> Void = { _ in }
    @State private var studio = StudioLiveSheet(step: "Configure Layer")

    /// `apply` writes a change to the layer; Done hands back the last value when anything changed, nil otherwise.
    public init(context: DialogContext, limits: DialogLimits, apply: @escaping (DialogLimits) throws -> Void, onFinish: @escaping (DialogLimits?) -> Void) {
        self.init(context: context, limits: limits, onFinish: onFinish)
        studioApply = apply
    }
    // STUDIO-END layer-luma-live

    public init(context: DialogContext, limits: DialogLimits, onFinish: @escaping (DialogLimits?) -> Void) {
        self.context = context
        self.onFinish = onFinish
        _limits = State(initialValue: limits)
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            DialogTitle("Luma Key Configuration")
            Text("Brightness Limits").fontWeight(.medium)
            BrightnessRows(context: context, limits: $limits, tooltips: false)
            StudioDoneButton(studio) { onFinish($0 ? limits : nil) } // STUDIO: live, closes with Done (B9)
        }
        .padding(20)
        .frame(width: 400)
        .studioLive(studio, draft: $limits, apply: studioApply) // STUDIO: live (B9)
    }
}

/// Mask And Fill Configuration (F220 to F223). `onFinish` gets the result, or nil for Cancel.
public struct MaskFillSheet: View {
    let onFinish: (DialogMaskFill?) -> Void
    @State private var draft: DialogMaskFill
    // STUDIO-BEGIN layer-mask-live: the Layer Editor applies each change to the layer as it's made, with Undo while the sheet is up (B9)
    private var studioApply: (DialogMaskFill) throws -> Void = { _ in }
    @State private var studio = StudioLiveSheet(step: "Configure Layer")

    /// `apply` writes a change to the layer; Done hands back the last value when anything changed, nil otherwise.
    public init(draft: DialogMaskFill, apply: @escaping (DialogMaskFill) throws -> Void, onFinish: @escaping (DialogMaskFill?) -> Void) {
        self.init(draft: draft, onFinish: onFinish)
        studioApply = apply
    }
    // STUDIO-END layer-mask-live

    public init(draft: DialogMaskFill, onFinish: @escaping (DialogMaskFill?) -> Void) {
        self.onFinish = onFinish
        _draft = State(initialValue: draft)
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            DialogTitle("Mask And Fill Configuration")
            Toggle("Exclude zero values", isOn: $draft.excludeZeroValues).toggleStyle(.checkbox)
            Toggle("Require Mixing Partner", isOn: $draft.requiresMixingPartner).toggleStyle(.checkbox)
            StudioDoneButton(studio) { onFinish($0 ? draft : nil) } // STUDIO: live, closes with Done (B9)
        }
        .padding(20)
        .frame(width: 320)
        .studioLive(studio, draft: $draft, apply: studioApply) // STUDIO: live (B9)
    }
}
