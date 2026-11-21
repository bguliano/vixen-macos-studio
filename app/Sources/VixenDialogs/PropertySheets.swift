import AppKit
import SwiftUI
import VixenBridge
import VixenKit

/// Location Setup (F181, F182).
struct LocationSheet: View {
    let context: DialogContext
    let element: String
    let selection: [String]
    let onFinish: (Bool) -> Void
    @State private var location: DialogLocation
    @State private var studio = StudioLiveSheet(step: "Location Setup") // STUDIO: live sheet, Undo while it's up (B9)

    init(context: DialogContext, element: String, selection: [String], location: DialogLocation, onFinish: @escaping (Bool) -> Void) {
        self.context = context
        self.element = element
        self.selection = selection
        self.onFinish = onFinish
        _location = State(initialValue: location)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            DialogTitle("Location Setup")
            Grid(alignment: .leading, verticalSpacing: 8) {
                row("X Position", $location.x)
                row("Y Position", $location.y)
                row("Z Position", $location.z)
            }
            StudioDoneButton(studio) { onFinish($0) } // STUDIO: live, closes with Done (B9)
        }
        .padding(20)
        .frame(width: 300)
        .studioLive(studio, draft: $location) { try context.applyLocation(node: element, selection: selection, location: $0) } // STUDIO: live sheet, Undo while it's up (B9)
    }

    private func row(_ title: String, _ value: Binding<Int32>) -> some View {
        GridRow {
            Text(title)
            NumberField(value: value, range: 0...Int32.max, width: 110)
        }
    }
}

/// Orientation Setup (F179). Used by the property's own Configure, and by the helper, which starts on Vertical (F180).
struct OrientationSheet: View {
    let context: DialogContext
    let initial: String
    let onChoose: (String?) -> Void
    @State private var value: String

    init(context: DialogContext, initial: String, onChoose: @escaping (String?) -> Void) {
        self.context = context
        self.initial = initial
        self.onChoose = onChoose
        _value = State(initialValue: initial)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            DialogTitle("Orientation Setup")
            HStack {
                Text("Orientation")
                Picker("Orientation", selection: $value) {
                    ForEach(context.orientations(), id: \.self) { Text($0).tag($0) }
                }
                .labelsHidden()
            }
            StudioVerbButtons(verb: "Set", onConfirm: { onChoose(value) }, onCancel: { onChoose(nil) }) // STUDIO: a verb, not OK (B9)
        }
        .padding(20)
        .frame(width: 300)
    }
}

/// Order Setup (F178).
struct OrderValueSheet: View {
    let context: DialogContext
    let element: String
    let selection: [String]
    let onFinish: (Bool) -> Void
    @State private var value: Int32
    @State private var studio = StudioLiveSheet(step: "Order Setup") // STUDIO: live sheet, Undo while it's up (B9)

    init(context: DialogContext, element: String, selection: [String], value: Int32, onFinish: @escaping (Bool) -> Void) {
        self.context = context
        self.element = element
        self.selection = selection
        self.onFinish = onFinish
        _value = State(initialValue: value)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            DialogTitle("Order Setup")
            HStack {
                Text("Order")
                NumberField(value: $value, range: 0...Int32.max, width: 110)
            }
            StudioDoneButton(studio) { onFinish($0) } // STUDIO: live, closes with Done (B9)
        }
        .padding(20)
        .frame(width: 300)
        .studioLive(studio, draft: $value) { try context.applyOrderValue(node: element, selection: selection, value: $0) } // STUDIO: live sheet, Undo while it's up (B9)
    }
}

/// Grid Setup (F183). The check runs at open, where upstream's starts wrong.
struct GridSheet: View {
    let context: DialogContext
    let element: String
    let selection: [String]
    let required: Int32
    let onFinish: (Bool) -> Void
    @State private var width: Int32
    @State private var height: Int32
    @State private var studio = StudioLiveSheet(step: "Grid Setup") // STUDIO: live sheet, Undo while it's up (B9)

    init(context: DialogContext, element: String, selection: [String], grid: DialogGrid, onFinish: @escaping (Bool) -> Void) {
        self.context = context
        self.element = element
        self.selection = selection
        self.onFinish = onFinish
        required = grid.required
        _width = State(initialValue: grid.width.clamped(to: 1...10000))
        _height = State(initialValue: grid.height.clamped(to: 1...10000))
    }

    var body: some View {
        let problem = context.gridCheck(width: width, height: height, required: required)
        VStack(alignment: .leading, spacing: 12) {
            DialogTitle("Grid Setup")
            Grid(alignment: .leading, verticalSpacing: 8) {
                GridRow {
                    Text("Width")
                    NumberField(value: $width, range: 1...10000)
                }
                GridRow {
                    Text("Height")
                    NumberField(value: $height, range: 1...10000)
                }
            }
            if let problem {
                Text(problem).foregroundStyle(.red)
            }
            StudioDoneButton(studio) { onFinish($0) } // STUDIO: live, closes with Done (B9)
        }
        .padding(20)
        .frame(width: 320)
        // STUDIO-BEGIN grid-live: width and height apply together once they pass the check (B9)
        .studioLive(studio, draft: Binding(get: { SIMD2(width, height) }, set: { width = $0.x; height = $0.y })) { size in
            guard context.gridCheck(width: size.x, height: size.y, required: required) == nil else { throw StudioLiveHold() }
            try context.applyGrid(node: element, selection: selection, width: size.x, height: size.y)
        }
        // STUDIO-END grid-live
    }
}

/// Color Configuration (F151 to F161).
struct ColorHelperSheet: View {
    let context: DialogContext
    let nodes: [String]
    /// F161: no dialog and no report, as the custom prop importer uses it. The two overwrite questions still appear.
    let silent: Bool
    let onFinish: (Bool) -> Void
    private let colorOrders: [String]
    @State private var colorType: DialogColorType?
    @State private var singleColor: UInt32
    @State private var setNames: [String]
    @State private var setName: String?
    @State private var colorOrder: String
    @State private var editingSets = false
    @State private var working = false
    @State private var alert: DialogAlert?

    init(context: DialogContext, nodes: [String], preset: String?, initialType: DialogColorType? = nil, silent: Bool = false, onFinish: @escaping (Bool) -> Void) {
        self.context = context
        self.nodes = nodes
        self.silent = silent
        self.onFinish = onFinish
        let model = context.colorHelper(preset: preset)
        colorOrders = model.colorOrders
        _colorType = State(initialValue: initialType ?? model.presetType)
        _singleColor = State(initialValue: model.singleColor)
        _setNames = State(initialValue: model.setNames)
        _setName = State(initialValue: model.setName)
        _colorOrder = State(initialValue: model.colorOrder)
    }

    var body: some View {
        if silent {
            Color.clear
                .frame(width: 1, height: 1)
                .onAppear(perform: ok)
                .dialogAlert($alert)
        } else {
            form
        }
    }

    private var form: some View {
        VStack(alignment: .leading, spacing: 14) {
            DialogTitle("Color Configuration")
            Text("How do these items handle color?").fontWeight(.bold)
            block(.singleColor, "They are a single color, and do not change color at all. The color is:") {
                ArgbColorPicker(argb: $singleColor)
                    .disabled(colorType != .singleColor)
            }
            block(.multipleDiscreteColors, "They can be multiple colors, but the colors are independent: they do not mix.") {
                VStack(alignment: .leading, spacing: 6) {
                    Text("(eg. multiple strings of different colors wrapped around each item.)")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    HStack {
                        Text("Colors:")
                        Picker("Colors:", selection: $setName) {
                            if setName == nil {
                                Text("").tag(String?.none)
                            }
                            ForEach(setNames, id: \.self) { Text($0).tag(String?.some($0)) }
                        }
                        .labelsHidden()
                        Button("Edit Colors") { editingSets = true }
                    }
                    .disabled(colorType != .multipleDiscreteColors)
                }
            }
            block(.fullColor, "They can be any color: they are full RGB and mix to make any color.") {
                HStack {
                    Text("Color Order:")
                    Picker("Color Order:", selection: $colorOrder) {
                        ForEach(colorOrders, id: \.self) { Text($0).tag($0) }
                    }
                    .labelsHidden()
                    .fixedSize()
                    Text(studioColorOrderNote(order: colorOrder)).font(.callout).foregroundStyle(.secondary) // STUDIO: channel count per pixel
                }
                .disabled(colorType != .fullColor)
            }
            HStack {
                if working {
                    ProgressView().controlSize(.small)
                }
                StudioVerbButtons(verb: "Set Up Colors", disabled: !canApply || working, onConfirm: ok, onCancel: { onFinish(false) }) // STUDIO: a verb, not OK (B9)
            }
        }
        .padding(20)
        .frame(width: 520)
        .sheet(isPresented: $editingSets) {
            ColorSetsSheet(context: context) {
                editingSets = false
                setNames = context.colorSets().map(\.name)
                if setName == nil || !setNames.contains(setName ?? "") {
                    setName = setNames.first
                }
            }
        }
        .dialogAlert($alert)
    }

    private func block(_ type: DialogColorType, _ title: String, @ViewBuilder content: () -> some View) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Button {
                colorType = type
            } label: {
                HStack(alignment: .firstTextBaseline, spacing: 6) {
                    Image(systemName: colorType == type ? "largecircle.fill.circle" : "circle")
                        .foregroundStyle(colorType == type ? Color.accentColor : .secondary)
                    Text(title).multilineTextAlignment(.leading)
                }
            }
            .buttonStyle(.plain)
            .accessibilityLabel(title)
            .accessibilityAddTraits(colorType == type ? .isSelected : [])
            content().padding(.leading, 24)
        }
    }

    private var request: DialogColorRequest? {
        guard let colorType else { return nil }
        return DialogColorRequest(colorType: colorType, singleColor: singleColor, setName: setName, colorOrder: colorOrder)
    }

    /// F152, F155.
    private var canApply: Bool {
        guard colorType != nil else { return false }
        return colorType != .multipleDiscreteColors || setName != nil
    }

    /// F159 and F160's questions, each at most once, then the work off the main thread, then F161's report.
    private func ok() {
        guard let request else { return }
        let preflight = context.colorHelperPreflight(nodes: nodes)
        askProperties(preflight, request)
    }

    private func askProperties(_ preflight: DialogColorPreflight, _ request: DialogColorRequest) {
        guard preflight.hasExistingProperties else {
            askFilters(preflight, request, properties: false)
            return
        }
        alert = DialogAlert(
            title: "Color Setup",
            message: "Some elements already have color properties set up. Should these be overwritten?",
            choices: [
                .init("Overwrite") { later { askFilters(preflight, request, properties: true) } },
                .init("Skip", role: .cancel) { later { askFilters(preflight, request, properties: false) } },
            ]
        )
    }

    private func askFilters(_ preflight: DialogColorPreflight, _ request: DialogColorRequest, properties: Bool) {
        guard preflight.hasExistingFilters else {
            apply(request, properties: properties, filters: false)
            return
        }
        alert = DialogAlert(
            title: "Color Setup",
            message: "Some elements are already patched to color filters. Should these be overwritten?",
            choices: [
                .init("Overwrite") { later { apply(request, properties: properties, filters: true) } },
                .init("Skip", role: .cancel) { later { apply(request, properties: properties, filters: false) } },
            ]
        )
    }

    /// An alert's button runs before the alert has gone, and the next one can't show until it has.
    private func later(_ work: @escaping () -> Void) {
        Task { work() }
    }

    private func apply(_ request: DialogColorRequest, properties: Bool, filters: Bool) {
        working = true
        let context = context
        let nodes = nodes
        Task {
            let report = await offMain {
                context.applyColorHelper(nodes: nodes, request: request, overwriteProperties: properties, overwriteFilters: filters)
            }
            working = false
            if silent {
                onFinish(true)
            } else {
                alert = .notice("Color Setup", report) { onFinish(true) }
            }
        }
    }
}

/// Color Sets (F162 to F169). The one dialog that writes as it goes: its changes hold even if the caller cancels.
struct ColorSetsSheet: View {
    let context: DialogContext
    let onFinish: () -> Void
    @State private var sets: [DialogColorSet]
    @State private var selected: String?
    /// What the right side shows. It may differ from `selected` once a name has been typed.
    @State private var showing = false
    @State private var name = ""
    @State private var colors: [UInt32] = []
    @State private var buttonTitle = "Make New Color Set"
    @State private var addingSet = false
    @State private var alert: DialogAlert?

    init(context: DialogContext, onFinish: @escaping () -> Void) {
        self.context = context
        self.onFinish = onFinish
        _sets = State(initialValue: context.colorSets())
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            DialogTitle("Color Sets")
            HStack(alignment: .top, spacing: 14) {
                VStack(alignment: .leading, spacing: 4) {
                    Text("Defined Color Sets:")
                    List(sets, id: \.name, selection: $selected) { Text($0.name).tag($0.name) }
                        .frame(width: 180, height: 220)
                        .listStyle(.bordered)
                    HStack {
                        Button("Add Color Set", systemImage: "plus") { addingSet = true }
                            .labelStyle(.iconOnly)
                        Button("Remove Color Set", systemImage: "minus", action: removeSet)
                            .labelStyle(.iconOnly)
                            .disabled(selected == nil)
                    }
                }
                GroupBox("Selected Color Set") { details.padding(6).disabled(!showing) }
            }
            HStack {
                Spacer()
                Button("Done", action: close).keyboardShortcut(.defaultAction) // STUDIO: it writes as it goes, so Done (B9)
            }
        }
        .padding(20)
        .frame(width: 600)
        .onChange(of: selected) { show(selected) }
        .sheet(isPresented: $addingSet) {
            TextPromptSheet(title: "New Color Set", prompt: "New Color Set name?", verb: "Add", validate: { $0.isEmpty ? "" : nil }) { response in // STUDIO: a verb (B9)
                addingSet = false
                if let response {
                    addSet(response)
                }
            }
        }
        .dialogAlert($alert)
    }

    private var details: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text("Name:")
                TextField("", text: $name).labelsHidden()
            }
            Text("Colors:")
            ScrollView {
                LazyVGrid(columns: Array(repeating: GridItem(.fixed(44)), count: 5), alignment: .leading, spacing: 6) {
                    ForEach(Array(colors.indices), id: \.self) { index in
                        ArgbColorPicker(argb: Binding(get: { colors[index] }, set: { colors[index] = $0 }))
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .frame(height: 110)
            HStack {
                Spacer()
                Button("Add Color", systemImage: "plus") { colors.append(0xFFFF_FFFF) }
                    .labelStyle(.iconOnly)
                    .help("Add Color")
            }
            HStack {
                Spacer()
                Button(buttonTitle, action: saveDisplayed)
            }
        }
        .onChange(of: name) { buttonTitle = context.colorSetButtonTitle(name: name, current: buttonTitle) }
    }

    /// F163: switching silently drops edits to the previous set, as upstream does.
    private func show(_ set: String?) {
        guard let set, let found = sets.first(where: { $0.name == set }) else {
            showing = false
            name = ""
            colors = []
            return
        }
        showing = true
        name = found.name
        colors = found.colors
        buttonTitle = context.colorSetButtonTitle(name: name, current: buttonTitle)
    }

    private func addSet(_ newName: String) {
        if let error = context.addColorSet(name: newName) {
            alert = .notice("Error", error)
            return
        }
        sets = context.colorSets()
        selected = newName
        show(newName)
    }

    private func removeSet() {
        guard let selected else { return }
        if let error = context.removeColorSet(name: selected) {
            alert = .notice("Error", error)
        }
        sets = context.colorSets()
        self.selected = nil
    }

    private func saveDisplayed() {
        if let error = context.saveColorSet(name: name, colors: colors) {
            alert = .notice("Name Requred", error)
            return
        }
        sets = context.colorSets()
    }

    /// F169.
    private func close() {
        guard showing, context.colorSetDiffers(name: name, colors: colors) else {
            onFinish()
            return
        }
        alert = DialogAlert(
            title: "Save Changes?",
            message: "Do you want to save changes to the displayed color set?",
            choices: [
                .init("Save") {
                    if let error = context.saveColorSet(name: name, colors: colors) {
                        Task { alert = .notice("Name Requred", error) }
                    } else {
                        onFinish()
                    }
                },
                .init("Don't Save", role: .destructive) { onFinish() },
                .init("Cancel", role: .cancel),
            ]
        )
    }
}

/// Dimming Curve Configuration (F238 to F245).
struct DimmingHelperSheet: View {
    let context: DialogContext
    let nodes: [String]
    /// Simple mode offers only Insert, as after an element template (F242).
    let simple: Bool
    let onFinish: (Bool) -> Void
    @State private var curve: DialogCurve?
    @State private var existing: DialogExistingCurve
    @State private var editing = false
    @State private var alert: DialogAlert?

    init(context: DialogContext, nodes: [String], simple: Bool, onFinish: @escaping (Bool) -> Void) {
        self.context = context
        self.nodes = nodes
        self.simple = simple
        self.onFinish = onFinish
        _existing = State(initialValue: simple ? .addInsertAfterElement : .doNothing)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            DialogTitle("Dimming Curve Configuration")
            Text("This will set up a dimming curve for each selected element.")
            Text("Use the button below to configure the dimming curve for the selected elements.")
            VStack(spacing: 6) {
                Text("Hint: Use a library curve to make editing easier.").foregroundStyle(.secondary)
                Button("Setup Dimming Curve") { editing = true }
            }
            .frame(maxWidth: .infinity)
            Text("If some elements are already patched to a dimming curve, what do you want to do?")
            Picker("", selection: $existing) {
                Text("Leave the existing dimming curve alone.").tag(DialogExistingCurve.doNothing).disabled(simple)
                Text("Update the existing dimming curve with the new data.").tag(DialogExistingCurve.updateExisting).disabled(simple)
                Text("Insert a new dimming curve after the element.").tag(DialogExistingCurve.addInsertAfterElement)
                Text("Add a new dimming curve anyway (not recommended).").tag(DialogExistingCurve.addNew).disabled(simple)
            }
            .pickerStyle(.radioGroup)
            .labelsHidden()
            HStack {
                Button("Help", systemImage: "questionmark.circle") {
                    if let url = URL(string: context.patchingHelpUrl()) {
                        NSWorkspace.shared.open(url)
                    }
                }
                StudioVerbButtons(verb: "Set Up Curves", disabled: curve == nil, onConfirm: ok, onCancel: { onFinish(false) }) // STUDIO: a verb, not OK (B9)
            }
        }
        .padding(20)
        .frame(width: 520)
        .sheet(isPresented: $editing) {
            CurveEditorView(core: context.vixenCore(), curve: (curve ?? context.defaultCurve()).value) { edited in
                editing = false
                if let edited {
                    curve = DialogCurve(edited)
                }
            }
        }
        .dialogAlert($alert)
    }

    private func ok() {
        guard let curve else { return }
        let report = context.applyDimmingHelper(nodes: nodes, curve: curve, existing: existing)
        alert = .notice("", report) { onFinish(true) }
    }
}
