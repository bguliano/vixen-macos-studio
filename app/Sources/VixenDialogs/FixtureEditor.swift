import AppKit
import Observation
import SwiftUI
import UniformTypeIdentifiers
import VixenBridge
import VixenKit

extension FixtureFunctionRow: Identifiable {}
extension FixtureChannelRow: Identifiable {}
extension FixtureIndexRow: Identifiable {}

@Observable @MainActor
public final class FixtureEditorModel {
    public let editor: FixtureEditor
    public private(set) var state: FixtureEditorState
    public var changed: (() -> Void)?
    var images = [" "]
    var error: String?
    var functions: FixtureEditorModel?
    var profileNames: [String]?
    var profileSelection: String?
    var busy = false

    public init(editor: FixtureEditor) { self.editor = editor; state = editor.state(); FixtureEditorHarness.use(self) }
    public func apply(_ change: (FixtureEditor) -> FixtureEditorState) { state = change(editor); changed?() }
    func text(_ value: String, _ write: @escaping (String) -> Void) -> Binding<String> { Binding(get: { value }, set: write) }
    func flag(_ value: Bool, _ write: @escaping (Bool) -> Void) -> Binding<Bool> { Binding(get: { value }, set: write) }
    func function(_ row: UInt32, _ field: String, _ value: String) {
        apply { $0.selectFunction(row: row) }
        if state.selectedFunction == row { apply { $0.setFunctionField(row: row, field: field, value: value) } }
    }
    func index(_ row: UInt32, _ field: String, _ value: String) { apply { $0.setIndexField(row: row, field: field, value: value) } }
    func openFunctions(_ initial: String) { functions = FixtureEditorModel(editor: editor.editFunctions(initial: initial)) }
    func finishFunctions(_ save: Bool) {
        if save, let functions {
            do { state = try editor.acceptFunctions(editor: functions.editor); changed?() } catch { self.error = error.userMessage; return }
        }
        functions = nil
        FixtureEditorHarness.use(self)
    }
    func loadProfiles() {
        busy = true
        Task { let names = await Task.detached { self.editor.profileNames() }.value; profileNames = names; profileSelection = names.first; busy = false }
    }
    func loadSelected() {
        guard let name = profileSelection else { return }
        busy = true
        Task {
            let result = await Task.detached { Result { try self.editor.loadProfile(name: name) } }.value
            switch result { case .success(let value): state = value; changed?(); profileNames = nil
            case .failure(let problem): error = problem.userMessage }
            busy = false
        }
    }
    // STUDIO-BEGIN fixture-functions-live: Edit Functions writes each valid change back to the profile being edited (B9)
    /// The functions sheet's editor changed: take its functions while they're valid. The last valid ones stay otherwise.
    func studioAcceptFunctions() {
        guard let functions, functions.state.functionErrors.isEmpty else { return }
        if let accepted = try? editor.acceptFunctions(editor: functions.editor), accepted != state {
            state = accepted
            changed?()
        }
    }
    // STUDIO-END fixture-functions-live
    func readImages() { Task { images = await Task.detached { self.editor.goboImages() }.value } }
    func importImages() {
        let panel = NSOpenPanel(); panel.allowedContentTypes = [.png]; panel.allowsMultipleSelection = true
        panel.begin { response in
            guard response == .OK else { return }
            let paths = panel.urls.map(\.path)
            Task { self.images = await Task.detached { self.editor.importGoboImages(paths: paths) }.value }
        }
    }
}

/// Shared by the fixture property sheet and the wizard's Edit Channels page.
public struct FixtureChannelsView: View {
    let model: FixtureEditorModel
    let showProfile: Bool
    public init(model: FixtureEditorModel, showProfile: Bool = true) { self.model = model; self.showProfile = showProfile }
    public var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            if showProfile { profileFields }
            Table(model.state.channels, selection: Binding(get: { model.state.selectedChannel }, set: { row in model.apply { $0.selectChannel(row: row) } })) {
                TableColumn("Channel #") { row in Text("\(row.number)").monospacedDigit() }.width(70)
                TableColumn("Name") { row in TextField("Name", text: model.text(row.name) { value in model.apply { $0.setChannelField(row: row.id, field: "name", value: value) } }).help("Unique name for the channel.") }
                TableColumn("Function") { row in
                    Picker("Function", selection: model.text(row.function) { value in model.apply { $0.setChannelField(row: row.id, field: "function", value: value) } }) {
                        ForEach(model.state.functionNames, id: \.self) { Text($0).tag($0) }
                    }.labelsHidden().help("Fixture function associated with the channel. Note functions may span more than one channel.")
                }
                TableColumn("") { row in
                    if showProfile { Button { model.openFunctions(row.function) } label: { Image(systemName: row.incomplete ? "exclamationmark.triangle.fill" : "gearshape").foregroundStyle(row.incomplete ? .red : .primary) }.buttonStyle(.borderless).help("Select to Edit the function associated with the channel.") }
                }.width(showProfile ? 32 : 1)
            }
            HStack {
                Button { model.apply { $0.addChannel() } } label: { Image(systemName: "plus") }.help("Select to Add a new channel.").accessibilityLabel("Add Channel")
                Button { if let row = model.state.selectedChannel { model.apply { $0.removeChannel(row: row) } } } label: { Image(systemName: "minus") }.disabled(model.state.selectedChannel == nil).help("Select to Remove the selected channel.").accessibilityLabel("Remove Channel")
                Spacer()
            }
            FixtureErrors(errors: model.state.channelErrors, notice: model.state.notice)
        }
        .sheet(isPresented: Binding(get: { model.functions != nil }, set: { if !$0 { model.finishFunctions(false) } })) {
            if let functions = model.functions { FixtureFunctionsSheet(model: functions, finish: model.finishFunctions).onChange(of: functions.state) { model.studioAcceptFunctions() } } // STUDIO: live (B9)
        }
        .sheet(isPresented: Binding(get: { model.profileNames != nil }, set: { if !$0 { model.profileNames = nil } })) { profilePicker }
        .alert("Fixture Profile", isPresented: Binding(get: { model.error != nil }, set: { if !$0 { model.error = nil } })) { Button("OK") { model.error = nil } } message: { Text(model.error ?? "") }
    }
    private var profileFields: some View {
        Grid(alignment: .leading, horizontalSpacing: 12, verticalSpacing: 8) {
            GridRow { Text("Profile:"); field("Profile", "name", model.state.name).help("Enter a unique name for the fixture profile."); Button("Load Fixture Profile") { model.loadProfiles() }.disabled(model.busy).help("Select to load an existing fixture profile.") }
            GridRow { Text("Manufacturer:"); field("Manufacturer", "manufacturer", model.state.manufacturer); HStack { Text("Revision:"); field("Revision", "revision", model.state.revision) } }
            GridRow { Text("Created By:"); field("Created By", "createdBy", model.state.createdBy); Color.clear.frame(height: 1) }
        }
    }
    private func field(_ label: String, _ key: String, _ value: String) -> some View { TextField(label, text: model.text(value) { value in model.apply { $0.setProfileField(field: key, value: value) } }) }
    private var profilePicker: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Load Fixture Specification").font(.headline)
            List(model.profileNames ?? [], id: \.self, selection: Binding(get: { model.profileSelection }, set: { model.profileSelection = $0 })) { Text($0).tag($0) }
            HStack { Spacer(); Button("Cancel", role: .cancel) { model.profileNames = nil }; Button("Load") { model.loadSelected() }.disabled(model.profileSelection == nil || model.busy).keyboardShortcut(.defaultAction) } // STUDIO: a verb, not OK (B9)
        }.padding(20).frame(width: 350, height: 400)
    }
}

public struct FixtureFunctionsView: View {
    let model: FixtureEditorModel
    public init(model: FixtureEditorModel) { self.model = model }
    public var body: some View {
        HSplitView {
            GroupBox("Functions") {
                VStack {
                    FixtureFunctionsTable(model: model)
                    HStack {
                        Button { model.apply { $0.addFunction() } } label: { Image(systemName: "plus") }.help("Select to Add a new function.").accessibilityLabel("Add Function")
                        Button { if let row = model.state.selectedFunction { model.apply { $0.removeFunction(row: row) } } } label: { Image(systemName: "minus") }.disabled(model.state.selectedFunction == nil).help("Select to Remove the selected function.").accessibilityLabel("Remove Function")
                        Spacer()
                    }
                }.padding(4)
            }.frame(minWidth: 450, idealWidth: 500, maxWidth: 520)
            GroupBox(model.state.detailTitle.isEmpty ? "<Function> Details" : model.state.detailTitle) {
                VStack(alignment: .leading, spacing: 10) {
                    FixtureFunctionDetails(model: model)
                    FixtureErrors(errors: model.state.functionErrors, notice: model.state.notice)
                }.padding(4).frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            }.frame(minWidth: 700)
        }.task { model.readImages() }
    }
}
private struct FixtureFunctionsTable: View {
    let model: FixtureEditorModel
    @FocusState private var nameFocus: UInt32?
    var body: some View {
        Table(model.state.functions, selection: Binding(get: { model.state.selectedFunction }, set: { row in model.apply { $0.selectFunction(row: row) } })) {
            TableColumn("Name") { row in TextField("Function Name", text: model.text(row.name) { model.function(row.id, "name", $0) }).focused($nameFocus, equals: row.id).help("Enter a unique name for the function. This name appears in effects.") }
            TableColumn("Type") { row in
                Picker("Type", selection: model.text(row.functionType) { model.function(row.id, "type", $0) }) { ForEach(model.state.functionTypes, id: \.self) { Text(fixtureLabel($0)).tag($0) } }.labelsHidden().help("Select the type of function.")
            }.width(110)
            TableColumn("Preview Legend") { row in TextField("", text: model.text(row.label) { model.function(row.id, "label", $0) }).accessibilityLabel("Preview Legend").help("Character to display to represent the function in a legend below the fixture graphic on the preview.") }.width(65)
            TableColumn("Tag") { row in
                Picker("Tag", selection: model.text(row.tag) { model.function(row.id, "tag", $0) }) { Text(" ").tag(row.tags.contains(row.tag) ? "" : row.tag); ForEach(row.tags, id: \.self) { Text($0).tag($0) } }.labelsHidden().help("Select a Tag for functions when Vixen needs to be aware of the function for the Preview or automation. Click the Help button for additional information.")
            }.width(125)
        }.onChange(of: model.state.functions.count) { old, new in if new > old { nameFocus = model.state.selectedFunction } }
    }
}
private struct FixtureFunctionDetails: View {
    let model: FixtureEditorModel
    var body: some View {
        switch model.state.panel {
        case "indexed", "wheel":
            FixtureIndexTable(model: model)
            HStack {
                Button { model.apply { $0.addIndex() } } label: { Image(systemName: "plus") }.help("Select to Add a new index item.").accessibilityLabel("Add Index")
                Button { if let row = model.state.selectedIndex { model.apply { $0.removeIndex(row: row) } } } label: { Image(systemName: "minus") }.disabled(model.state.selectedIndex == nil).help("Select to Remove the selected index item.").accessibilityLabel("Remove Index")
                if model.state.showImages { Button("Import Gobo Images") { model.importImages() }.help("Select to Import png images for the Gobo items.") }
                Spacer()
            }
            if model.state.showAssociated {
                HStack { Text("Associated Prism Function:"); Picker("Associated Prism Function", selection: binding("associated", model.state.associated)) { Text(" ").tag(""); ForEach(model.state.prismFunctions, id: \.self) { Text($0).tag($0) } }.labelsHidden() }.help("Select the Prism function that is opened and closed by this function.")
            }
        case "rotation":
            Grid(alignment: .leading) {
                GridRow { Text("Start Position (Degrees):"); TextField("Start Position", text: binding("start", model.state.start)).help("Defines the resting position of the fixture.") }
                GridRow { Text("Stop Position (Degrees):"); TextField("Stop Position", text: binding("stop", model.state.stop)).help("Defines the maximum range of movement.") }
            }
            MovingHeadView(color: 0xff00ffff, animation: model.state.rotationTag).frame(width: 300, height: 300).frame(maxWidth: .infinity)
        case "zoom":
            Picker("Zoom", selection: binding("zoom", model.state.zoom)) {
                Text("Narrow To Wide").tag("NarrowToWide").help("Fixture zooms from a narrow beam to a wide beam.")
                Text("Wide To Narrow").tag("WideToNarrow").help("Fixture zooms from a wide beam to a narrow beam.")
            }.pickerStyle(.radioGroup)
        default: EmptyView()
        }
    }
    private func binding(_ field: String, _ value: String) -> Binding<String> { model.text(value) { if let row = model.state.selectedFunction { model.function(row, field, $0) } } }
}
private struct FixtureIndexTable: View {
    let model: FixtureEditorModel
    @FocusState private var nameFocus: UInt32?
    var body: some View {
        Table(model.state.indices, selection: Binding(get: { model.state.selectedIndex }, set: { row in model.apply { $0.selectIndex(row: row) } })) {
            TableColumn("Name") { row in text(row, "name", row.name).focused($nameFocus, equals: row.id) }.width(min: 120, ideal: 150)
            TableColumn("Use Curve") { row in flag(row, "curve", row.useCurve) }.width(58)
            TableColumn("Start Value") { row in text(row, "start", row.start) }.width(63)
            TableColumn("End Value") { row in text(row, "end", row.end) }.width(63)
            TableColumn(model.state.panel == "wheel" ? "Half Stop" : model.state.showImages ? "Image" : "Tag") { row in
                if model.state.panel == "wheel" { flag(row, "half", row.half) }
                else if model.state.showImages { Picker("Image", selection: model.text(row.image) { model.index(row.id, "image", $0) }) { ForEach(model.images, id: \.self) { Text($0).tag($0) } }.labelsHidden() }
                else { Picker("Tag", selection: model.text(row.tag) { model.index(row.id, "tag", $0) }) { ForEach(model.state.indexTags, id: \.self) { Text($0).tag($0) } }.labelsHidden() }
            }.width(model.state.panel == "wheel" ? 60 : model.state.showImages ? 150 : 120)
            TableColumn(model.state.panel == "wheel" ? "Color" : "") { row in
                if model.state.panel == "wheel" { HStack(spacing: 2) { color(row, "color1", row.color1); if row.half { color(row, "color2", row.color2) } }.disabled(row.half) }
            }.width(model.state.panel == "wheel" ? 70 : 1)
            TableColumn(model.state.panel == "wheel" ? "Exclude Color Property" : "") { row in if model.state.panel == "wheel" { flag(row, "exclude", row.exclude) } }.width(model.state.panel == "wheel" ? 70 : 1)
        }.onChange(of: model.state.indices.count) { old, new in if new > old { nameFocus = model.state.selectedIndex } }
    }
    private func text(_ row: FixtureIndexRow, _ field: String, _ value: String) -> some View { TextField("", text: model.text(value) { model.index(row.id, field, $0) }).accessibilityLabel(field) }
    private func flag(_ row: FixtureIndexRow, _ field: String, _ value: Bool) -> some View { Toggle(field, isOn: model.flag(value) { model.index(row.id, field, $0 ? "true" : "false") }).labelsHidden() }
    private func color(_ row: FixtureIndexRow, _ field: String, _ argb: UInt32) -> some View {
        ColorPicker("Color", selection: Binding(get: { Color(nsColor: NSColor(srgbRed: CGFloat((argb >> 16) & 255) / 255, green: CGFloat((argb >> 8) & 255) / 255, blue: CGFloat(argb & 255) / 255, alpha: CGFloat(argb >> 24) / 255)) }, set: { value in
            guard let c = NSColor(value).usingColorSpace(.sRGB) else { return }
            model.index(row.id, field, String(format: "#%02X%02X%02X", Int((c.redComponent * 255).rounded()), Int((c.greenComponent * 255).rounded()), Int((c.blueComponent * 255).rounded())))
        }), supportsOpacity: false).labelsHidden()
    }
}
private struct FixtureErrors: View {
    let errors: [String]
    let notice: String?
    var body: some View { if let text = notice ?? errors.first { Label(text, systemImage: "exclamationmark.triangle.fill").foregroundStyle(.yellow).font(.callout).fixedSize(horizontal: false, vertical: true).help(errors.joined(separator: "\n")) } }
}
private func fixtureLabel(_ value: String) -> String { ["ColorWheel": "Color Wheel", "RGBColor": "RGB Color", "RGBWColor": "RGBW Color"][value] ?? value }

struct FixtureFunctionsSheet: View {
    let model: FixtureEditorModel
    let finish: (Bool) -> Void
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Edit Functions").font(.headline)
            FixtureFunctionsView(model: model)
            // STUDIO-BEGIN fixture-functions-done: changes went back as they were made, so Done closes (B9)
            HStack {
                FixtureHelp(title: "Learn More About Fixture Functions")
                Spacer()
                if let problem = model.state.functionErrors.first {
                    Text("Not applied until fixed: \(problem)").font(.caption).foregroundStyle(.secondary).lineLimit(2)
                }
                Button("Done") { finish(false) }.keyboardShortcut(.defaultAction)
            }
            // STUDIO-END fixture-functions-done
        }.padding(20).frame(minWidth: 1320, minHeight: 550)
    }
}
private struct FixtureHelp: View {
    let title: String
    var body: some View { Button(title) { if UITestHarness.isActive { UITestHarness.note("help: http://www.vixenlights.com") } else { NSWorkspace.shared.open(URL(string: "http://www.vixenlights.com")!) } }.buttonStyle(.link) }
}
struct FixturePropertySheet: View {
    let context: DialogContext
    let node: String
    let selection: [String]
    let onFinish: (Bool) -> Void
    @State private var model: FixtureEditorModel?
    @State private var error: String?
    @State private var saving = false
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Fixture Property Editor").font(.headline)
            if let model { FixtureChannelsView(model: model) } else { ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity) }
            // STUDIO-BEGIN fixture-property-done: each valid change saves as it's made, so Done closes (B9)
            HStack {
                FixtureHelp(title: "Learn More About Fixture Profiles")
                Spacer()
                if let problem = model?.state.errors.first {
                    Text("Not saved until fixed: \(problem)").font(.caption).foregroundStyle(.secondary).lineLimit(2)
                }
                Button("Done") { studioDone() }.disabled(saving).keyboardShortcut(.defaultAction)
            }
            // STUDIO-END fixture-property-done
        }.padding(20).frame(minWidth: 650, minHeight: 500)
        .onChange(of: model?.state) { old, _ in if old != nil { studioSaveSoon() } } // STUDIO: live, not on opening (B9)
        .task { do { model = FixtureEditorModel(editor: try context.fixtureEditor(node: node)) } catch { self.error = error.userMessage } }
        .alert("Fixture Profile", isPresented: Binding(get: { error != nil }, set: { if !$0 { error = nil } })) { Button("OK") { error = nil } } message: { Text(error ?? "") }
    }
    // STUDIO-BEGIN fixture-property-live: saves the profile and applies it to the elements a moment after each valid change (B9)
    @State private var studioPending: Task<Void, Never>?
    @State private var studioSaved = false

    private func studioSaveSoon() {
        guard let model, model.state.errors.isEmpty else { return }
        studioPending?.cancel()
        studioPending = Task {
            try? await Task.sleep(for: .milliseconds(600))
            guard !Task.isCancelled else { return }
            await studioSave()
        }
    }

    private func studioSave() async {
        guard let model, model.state.errors.isEmpty else { return }
        studioPending = nil
        let result = await Task.detached { Result { try model.editor.saveLibrary(); try context.applyFixtureEditor(node: node, selection: selection, editor: model.editor) } }.value
        switch result { case .success: studioSaved = true; case .failure(let problem): error = problem.userMessage }
    }

    private func studioDone() {
        guard let pending = studioPending else { return onFinish(studioSaved) }
        pending.cancel()
        saving = true
        Task { await studioSave(); saving = false; onFinish(studioSaved) }
    }
    // STUDIO-END fixture-property-live

    private func save() {
        guard let model else { return }; saving = true
        Task {
            let result = await Task.detached { Result { try model.editor.saveLibrary(); try context.applyFixtureEditor(node: node, selection: selection, editor: model.editor) } }.value
            saving = false
            switch result { case .success: onFinish(true); case .failure(let problem): error = problem.userMessage }
        }
    }
}
