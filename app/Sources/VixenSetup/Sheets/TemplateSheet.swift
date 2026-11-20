import SwiftUI
import VixenBridge
import VixenKit

/// A template's dialog (F102 to F108), chosen by `draft.kind`. OK hands back the edited draft, which the caller keeps as that template's memory for the window's
/// life (F99). Upstream's memory also takes what was typed before a Cancel, except on Single Item: pass `rememberOnCancel` to get that.
struct TemplateSheet: View {
    private let onFinish: (SetupTemplateDraft?) -> Void
    private let rememberOnCancel: ((SetupTemplateDraft) -> Void)?
    @State private var draft: SetupTemplateDraft
    @State private var problem: SetupTemplateProblem?
    @State private var finished = false
    @State private var colorMode: String // STUDIO: the template's Colour choice (StudioTemplateColor)
    @State private var hiddenPixels: Int // STUDIO: hidden pixels per string (StudioTemplateWiring)

    init(draft: SetupTemplateDraft, rememberOnCancel: ((SetupTemplateDraft) -> Void)? = nil, onFinish: @escaping (SetupTemplateDraft?) -> Void) {
        self.onFinish = onFinish
        self.rememberOnCancel = rememberOnCancel
        _draft = State(initialValue: draft)
        _colorMode = State(initialValue: StudioTemplateColor.mode(for: draft.kind)) // STUDIO: the template's Colour choice
        _hiddenPixels = State(initialValue: Int(StudioTemplateWiring.hidden(for: draft.kind))) // STUDIO: hidden pixels per string
    }

    var body: some View {
        content
            .alert(
                problem?.title ?? "",
                isPresented: Binding(get: { problem != nil }, set: { if !$0 { problem = nil } }),
                presenting: problem
            ) { _ in
                Button("OK", role: .cancel) {}
            } message: { problem in
                Text(problem.message)
            }
    }

    @ViewBuilder private var content: some View {
        switch draft.kind {
        // STUDIO-BEGIN template-color-single: Single Item is a form rather than a name prompt, so it can carry the Colour choice
        case .singleItem:
            form("Single Item", width: 420) {
                Grid(alignment: .leading, horizontalSpacing: 8, verticalSpacing: 8) {
                    GridRow {
                        SheetFieldLabel("Element Name:").gridColumnAlignment(.trailing)
                        SheetTextField("Element Name:", text: $draft.name)
                    }
                }
            }
        // STUDIO-END template-color-single
        case .numberedGroup, .arch, .candyCane, .star:
            form("Group Setup", width: 344) { GroupSetupForm(draft: $draft) }
        case .starburst:
            form("Starburst Setup", width: 520) { StarburstForm(draft: $draft) }
        case .icicles:
            form("Icicles Setup", width: 556) { IciclesForm(draft: $draft) }
        case .lipSync:
            form("LipSync Setup", width: 344) { LipSyncForm(draft: $draft) }
        case .megatree:
            form("Megatree Setup", width: 540) { MegatreeForm(draft: $draft) }
        case .pixelGrid:
            form("Pixel Grid / Matrix Setup", width: 575) { PixelGridForm(draft: $draft) }
        case .intelligentFixture:
            // It has a wizard rather than a form (F110), so nothing should bring it here.
            Color.clear.onAppear { onFinish(nil) }
        }
    }

    private func form(_ title: String, width: CGFloat, @ViewBuilder fields: () -> some View) -> some View {
        VStack(alignment: .leading, spacing: 14) {
            Text(title).font(.headline)
            fields()
            // STUDIO-BEGIN template-wiring-section: start corner, reverse every other string, hidden pixels and the live diagram
            if StudioTemplateWiring.supports(draft.kind) {
                StudioTemplateWiringSection(draft: $draft, hidden: $hiddenPixels)
                    .disabled(draft.kind == .megatree && !draft.flag)
            }
            // STUDIO-END template-wiring-section
            // STUDIO-BEGIN template-color-picker: RGB / RGBW / Single colour, with the channel count it gives
            if StudioTemplateColor.supports(draft.kind) {
                StudioTemplateColorPicker(mode: $colorMode)
            }
            // STUDIO-END template-color-picker
            SheetButtons(ok: ok, cancel: cancel)
        }
        .padding(16)
        .frame(width: width)
        .sheetKeys(ok: ok, cancel: cancel)
    }

    private func ok() {
        // STUDIO-BEGIN template-color-ok: remember the Colour choice; Single Item takes an empty name as the default, as its prompt did
        if draft.kind == .singleItem, draft.name.isEmpty {
            draft.name = setupTemplateDraft(kind: .singleItem)?.name ?? draft.name
        }
        StudioTemplateColor.chosen[draft.kind] = colorMode
        // STUDIO-END template-color-ok
        StudioTemplateWiring.chosenHidden[draft.kind] = hiddenPixels // STUDIO: remember the hidden pixels per string
        if let found = setupTemplateProblem(draft: draft) {
            problem = found
        } else {
            finish(draft)
        }
    }

    private func cancel() {
        rememberOnCancel?(draft)
        finish(nil)
    }

    private func finish(_ result: SetupTemplateDraft?) {
        guard !finished else { return }
        finished = true
        onFinish(result)
    }
}

private let countRange = 1...1000

/// Generic Numbered Group, Arch, Candy Cane and Star (F103).
private struct GroupSetupForm: View {
    @Binding var draft: SetupTemplateDraft

    var body: some View {
        Grid(alignment: .leading, horizontalSpacing: 8, verticalSpacing: 8) {
            GridRow {
                SheetFieldLabel("Group Name:").gridColumnAlignment(.trailing)
                SheetTextField("Group Name:", text: $draft.name)
            }
            GridRow {
                SheetFieldLabel("Item Prefix:")
                SheetTextField("Item Prefix:", text: $draft.prefix)
            }
            GridRow {
                SheetFieldLabel("Item Count:")
                SheetNumberField("Item Count:", value: $draft.count.sheetInt, range: countRange)
            }
        }
    }
}

/// F104.
private struct StarburstForm: View {
    @Binding var draft: SetupTemplateDraft

    var body: some View {
        Grid(alignment: .leading, horizontalSpacing: 8, verticalSpacing: 10) {
            GridRow {
                SheetFieldLabel("Starburst Name:").gridColumnAlignment(.trailing)
                SheetTextField("Starburst Name:", text: $draft.name).gridCellColumns(3)
            }
            GridRow {
                SheetFieldLabel("Spoke Count:")
                SheetNumberField("Spoke Count:", value: $draft.count.sheetInt, range: countRange)
                SheetFieldLabel("Spoke Prefix:").gridColumnAlignment(.trailing)
                SheetTextField("Spoke Prefix:", text: $draft.prefix)
            }
            GridRow {
                SheetFieldLabel("Pixel Starburst:")
                Toggle("Pixel Starburst:", isOn: $draft.flag)
                    .toggleStyle(.checkbox)
                    .labelsHidden()
                    .gridCellColumns(3)
            }
            GridRow {
                SheetFieldLabel("Pixels / Spoke:", enabled: draft.flag)
                SheetNumberField("Pixels / Spoke:", value: $draft.secondCount.sheetInt, range: countRange)
                    .disabled(!draft.flag)
                SheetFieldLabel("Pixel Prefix:", enabled: draft.flag)
                SheetTextField("Pixel Prefix:", text: $draft.secondPrefix)
                    .disabled(!draft.flag)
            }
        }
    }
}

/// F105.
private struct IciclesForm: View {
    @Binding var draft: SetupTemplateDraft

    var body: some View {
        Grid(alignment: .leading, horizontalSpacing: 8, verticalSpacing: 10) {
            GridRow {
                SheetFieldLabel("Icicle Name:").gridColumnAlignment(.trailing)
                SheetTextField("Icicle Name:", text: $draft.name).gridCellColumns(3)
            }
            GridRow {
                SheetFieldLabel("String Count:")
                SheetNumberField("String Count:", value: $draft.count.sheetInt, range: countRange)
                SheetFieldLabel("String Prefix:").gridColumnAlignment(.trailing)
                SheetTextField("String Prefix:", text: $draft.prefix)
            }
            GridRow {
                SheetFieldLabel("Pixels / String Pattern:")
                SheetTextField("Pixels / String Pattern:", text: $draft.pattern)
                    .frame(minWidth: 150)
                    .onChange(of: draft.pattern) {
                        // Paste as well as typing, unlike upstream's key filter (F105).
                        let cleaned = draft.pattern.filter { ($0.isASCII && $0.isNumber) || $0 == "," }
                        if cleaned != draft.pattern { draft.pattern = cleaned }
                    }
                SheetFieldLabel("Pixel Prefix:")
                SheetTextField("Pixel Prefix:", text: $draft.secondPrefix)
            }
            GridRow {
                Color.clear.frame(width: 0, height: 0)
                Text("Example: 7,9,5")
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .gridCellColumns(3)
            }
        }
    }
}

/// F106.
private struct LipSyncForm: View {
    @Binding var draft: SetupTemplateDraft

    var body: some View {
        Grid(alignment: .leading, horizontalSpacing: 8, verticalSpacing: 8) {
            GridRow {
                SheetFieldLabel("Element Name:")
                SheetTextField("Element Name:", text: $draft.name)
            }
        }
    }
}

/// F107. Pixel Tree off disables the pixel fields and both groups, but what they hold stays and still applies.
private struct MegatreeForm: View {
    @Binding var draft: SetupTemplateDraft

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Grid(alignment: .leading, horizontalSpacing: 8, verticalSpacing: 10) {
                GridRow {
                    SheetFieldLabel("Tree Name:").gridColumnAlignment(.trailing)
                    SheetTextField("Tree Name:", text: $draft.name).gridCellColumns(3)
                }
                GridRow {
                    SheetFieldLabel("String Count:")
                    SheetNumberField("String Count:", value: $draft.count.sheetInt, range: countRange)
                    SheetFieldLabel("String Prefix:").gridColumnAlignment(.trailing)
                    SheetTextField("String Prefix:", text: $draft.prefix)
                }
                GridRow {
                    SheetFieldLabel("Pixel Tree:")
                    Toggle("Pixel Tree:", isOn: $draft.flag)
                        .toggleStyle(.checkbox)
                        .labelsHidden()
                        .gridCellColumns(3)
                }
                GridRow {
                    SheetFieldLabel("Pixels / String:", enabled: draft.flag)
                    SheetNumberField("Pixels / String:", value: $draft.secondCount.sheetInt, range: countRange)
                        .disabled(!draft.flag)
                    SheetFieldLabel("Pixel Prefix:", enabled: draft.flag)
                    SheetTextField("Pixel Prefix:", text: $draft.secondPrefix)
                        .disabled(!draft.flag)
                }
            }
            // STUDIO-BEGIN megatree-wiring-moved: the wiring section under the fields replaces Wire Start Location and Zig Zag (StudioTemplateWiring)
            // STUDIO-END megatree-wiring-moved
        }
    }
}

/// F108.
private struct PixelGridForm: View {
    @Binding var draft: SetupTemplateDraft

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                SheetFieldLabel("Name")
                SheetTextField("Name", text: $draft.name)
            }
            HStack(alignment: .top, spacing: 12) {
                GroupBox("String Orientation") {
                    Picker("String Orientation", selection: orientation) {
                        Text("Horizontal").tag(true)
                        Text("Vertical").tag(false)
                    }
                    .pickerStyle(.radioGroup)
                    .horizontalRadioGroupLayout()
                    .labelsHidden()
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(4)
                }
                GroupBox("Dimensions") {
                    HStack {
                        SheetFieldLabel("Height")
                        SheetNumberField("Height", value: $draft.count.sheetInt, range: countRange)
                        Spacer(minLength: 8)
                        SheetFieldLabel("Width")
                        SheetNumberField("Width", value: $draft.secondCount.sheetInt, range: countRange)
                    }
                    .padding(4)
                }
            }
            HStack(spacing: 12) {
                HStack {
                    SheetFieldLabel("String Prefix:")
                    SheetTextField("String Prefix:", text: $draft.prefix)
                }
                HStack {
                    SheetFieldLabel("Pixel Prefix:")
                    SheetTextField("Pixel Prefix:", text: $draft.secondPrefix)
                }
            }
            // STUDIO-BEGIN grid-wiring-moved: the wiring section under the fields replaces Wire Start Location and Zig Zag (StudioTemplateWiring)
            // STUDIO-END grid-wiring-moved
        }
    }

    /// Changing it swaps what the two prefix boxes hold (PixelGrid.cs:242-248), so Vertical starts with "C" and "R".
    private var orientation: Binding<Bool> {
        Binding(
            get: { draft.flag },
            set: { horizontal in
                guard horizontal != draft.flag else { return }
                var changed = draft
                changed.flag = horizontal
                swap(&changed.prefix, &changed.secondPrefix)
                draft = changed
            }
        )
    }

    private func corner(_ title: String, _ location: SetupStartLocation) -> some View {
        SheetRadioButton(title: title, isOn: draft.start == location) { draft.start = location }
    }
}

/// "Patching " on Megatree and Pixel Grid: the checkbox and the read-only number beside it, which follows the fields it depends on.
private struct ZigZagGroup: View {
    @Binding var draft: SetupTemplateDraft
    @State private var every: UInt32 = 0
    @Environment(\.isEnabled) private var isEnabled

    var body: some View {
        GroupBox {
            HStack(spacing: 6) {
                Toggle("Zig Zag Every", isOn: $draft.zigZag)
                    .toggleStyle(.checkbox)
                SheetFieldLabel(String(every), enabled: isEnabled)
                    .monospacedDigit()
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(4)
        } label: {
            SheetFieldLabel("Patching ", enabled: isEnabled)
        }
        .onChange(of: draft, initial: true) {
            every = setupZigZagEvery(draft: draft)
        }
    }
}
