import AppKit
import SwiftUI
import VixenBridge
import VixenKit

/// A real radio button with its own tooltip and enabling, which a radio `Picker`'s rows can't promise, and it can have a combo beside it.
struct FixtureRadioButton: NSViewRepresentable {
    let title: String
    let isOn: Bool
    var help: String?
    let choose: () -> Void

    func makeCoordinator() -> Coordinator {
        Coordinator()
    }

    func makeNSView(context: Context) -> NSButton {
        NSButton(radioButtonWithTitle: title, target: context.coordinator, action: #selector(Coordinator.clicked))
    }

    func sizeThatFits(_ proposal: ProposedViewSize, nsView: NSButton, context: Context) -> CGSize? {
        nsView.intrinsicContentSize
    }

    func updateNSView(_ button: NSButton, context: Context) {
        context.coordinator.choose = choose
        button.title = title
        button.state = isOn ? .on : .off
        button.toolTip = help
        button.isEnabled = context.environment.isEnabled
    }

    final class Coordinator: NSObject {
        var choose: () -> Void = {}

        @objc func clicked() {
            choose()
        }
    }
}

/// A titled group of controls, as wide as the page.
private struct FixtureGroup<Content: View>: View {
    let title: String
    @ViewBuilder let content: Content

    var body: some View {
        GroupBox(title) {
            VStack(alignment: .leading, spacing: 12) {
                content
            }
            .padding(10)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

/// F141.
struct FixtureSelectProfilePage: View {
    let model: FixtureWizardModel
    let state: SetupFixtureWizardState

    /// Blank when nothing in the list matches, which is Create New, or a renamed copy once Edit Channels has been left.
    private var comboSelection: String? {
        state.selectedFixture.flatMap { state.profileNames.contains($0) ? $0 : nil }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            FixtureGroup(title: "Select Profile") {
                Grid(alignment: .leading, horizontalSpacing: 12, verticalSpacing: 12) {
                    GridRow {
                        FixtureRadioButton(title: "Select Existing Profile", isOn: state.selectExisting) {
                            model.apply { $0.chooseExisting() }
                        }
                        .disabled(!state.existingEnabled)
                        Picker("Select Existing Profile", selection: Binding(get: { comboSelection }, set: { name in
                            if let name {
                                model.apply { $0.selectProfile(name: name) }
                            }
                        })) {
                            if comboSelection == nil {
                                Text("").tag(String?.none)
                            }
                            ForEach(state.profileNames, id: \.self) { name in
                                Text(name).tag(String?.some(name))
                            }
                        }
                        .labelsHidden()
                        .frame(width: 300, alignment: .leading)
                        .disabled(!state.selectExisting)
                    }
                    GridRow {
                        FixtureRadioButton(title: "Create New Profile", isOn: !state.selectExisting) {
                            model.apply { $0.chooseCreateNew() }
                        }
                        .gridCellColumns(2)
                    }
                }
            }
            FixtureGroup(title: "Profile Properties") {
                Grid(alignment: .leading, horizontalSpacing: 12, verticalSpacing: 8) {
                    FixtureFieldRow(title: "Profile Name:", help: "Enter a unique name for the fixture profile.", text: model.binding(state.profileName) { $0.setProfileName(text: $1) })
                    FixtureFieldRow(title: "Manufacturer:", help: "Enter the manufacturer of the fixture.", text: model.binding(state.manufacturer) { $0.setManufacturer(text: $1) })
                    FixtureFieldRow(title: "Created By:", help: "User that created the fixture profile.", text: model.binding(state.createdBy) { $0.setCreatedBy(text: $1) })
                    FixtureFieldRow(title: "Revision:", help: "Revision of the fixture profile.", text: model.binding(state.revision) { $0.setRevision(text: $1) })
                }
            }
        }
    }
}

/// A label and a text field, with the same tooltip on both as upstream.
private struct FixtureFieldRow: View {
    let title: String
    let help: String
    let text: Binding<String>
    var enabled = true
    var width: CGFloat?

    var body: some View {
        GridRow {
            Text(title)
                .fixedSize()
                .foregroundStyle(enabled ? Color.primary : Color(nsColor: .disabledControlTextColor))
                .help(help)
            TextField(title, text: text, prompt: Text(""))
                .labelsHidden()
                .frame(width: width)
                .disabled(!enabled)
                .help(help)
        }
    }
}

/// F143.
struct FixtureColorSupportPage: View {
    let model: FixtureWizardModel
    let state: SetupFixtureWizardState

    var body: some View {
        FixtureGroup(title: "Color Support") {
            radio(.noColorSupport, "No Color Support", "Select if you want to manually control color through separate display elements.  This option is not recommended.")
            radio(.colorMixing, "Color Mixing", "Select when the fixture uses multiple color source or multiple light sources of different colors.  Examples are RGB and RGBW")
            radio(.colorWheel, "Color Wheel", "Select when the fixture has a single white light source and is equipped with a color wheel to change colors.")
        }
    }

    private func radio(_ support: SetupFixtureColorSupport, _ title: String, _ help: String) -> some View {
        FixtureRadioButton(title: title, isOn: state.colorSupport == support, help: help) {
            model.apply { $0.setColorSupport(support: support) }
        }
    }
}

/// F144.
struct FixtureAutomationPage: View {
    let model: FixtureWizardModel
    let state: SetupFixtureWizardState

    var body: some View {
        FixtureGroup(title: "Automation Options") {
            box(
                "Automatically Open and Close Shutter",
                "This option will automatically control the shutter channel based on the color channels.  Whenever an effect is generating color, the shutter will open.  Whenever there is no effect, or the effect is dark, the shutter will be closed.",
                state.autoShutter, state.enableShutter
            ) { $0.setAutoShutter(on: $1) }
            box(
                "Automatically Control Color Wheel",
                "This option will automatically control the color wheel by matching colors generated in the effects to colors supported on the color wheel.",
                state.autoColorWheel, state.enableColorWheel
            ) { $0.setAutoColorWheel(on: $1) }
            box(
                "Automatically Control Dimmer",
                "This option is usually selected for fixtures with a color wheel and fixed white light source.  It will automatically control the fixture's dimmer channel based on the intensity of the colors in the effects.",
                state.autoDimmer, state.enableDimmer
            ) { $0.setAutoDimmer(on: $1) }
            box(
                "Automatically Open and Close Prism",
                "This option is usually selected for fixtures that both a channel for enabling the prism and another channel for selecting the position or movement of the prism.",
                state.autoPrism, state.enablePrism
            ) { $0.setAutoPrism(on: $1) }
        }
    }

    private func box(
        _ title: String,
        _ help: String,
        _ isOn: Bool,
        _ enabled: Bool,
        _ write: @escaping (SetupFixtureWizard, Bool) -> SetupFixtureWizardState
    ) -> some View {
        Toggle(title, isOn: model.binding(isOn, write))
            .toggleStyle(.checkbox)
            .disabled(!enabled)
            .help(help)
    }
}

/// F145.
struct FixtureDimmingCurvesPage: View {
    @Bindable var model: FixtureWizardModel
    let state: SetupFixtureWizardState

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            FixtureGroup(title: "Dimming Options") {
                radio(.noDimmingCurve, "Do Not Include a Dimming Curve", "Select when dimming curves are not desired.")
                radio(.oneDimmingCurvePerFixture, "Add Dimming Curve For Fixture", "This option adds a single dimming curve for all color channels.")
                radio(
                    .oneDimmingCurvePerColor,
                    "Add Dimming Curve For Each Color Channel",
                    "This option configures a separate dimming curve for each color channel.  With this option all the dimming curves are initially identical.  The curves can be further adjusted via the Patching Graphical View."
                )
                .disabled(!state.perColorEnabled)
            }
            Button("Setup Dimming Curve") { model.showsCurveEditor = true }
        }
        .sheet(isPresented: $model.showsCurveEditor) {
            if let core = model.curveCore, let curve = model.dimmingCurve {
                CurveEditorView(core: core, curve: curve, onChange: model.changeCurve, onDone: { _ in model.showsCurveEditor = false }) // STUDIO: live, with Done (B24)
            }
        }
    }

    private func radio(_ selection: SetupFixtureDimming, _ title: String, _ help: String) -> some View {
        FixtureRadioButton(title: title, isOn: state.dimming == selection, help: help) {
            model.apply { $0.setDimming(selection: selection) }
        }
    }
}

/// F146.
struct FixtureGroupingPage: View {
    let model: FixtureWizardModel
    let state: SetupFixtureWizardState

    var body: some View {
        HStack(alignment: .top, spacing: 30) {
            Grid(alignment: .leading, horizontalSpacing: 12, verticalSpacing: 12) {
                GridRow {
                    Text("Number of Fixtures to Create:")
                        .fixedSize()
                        .help("Number of fixture display elements to create.")
                    HStack(spacing: 6) {
                        Text(String(state.count))
                            .monospacedDigit()
                            .frame(minWidth: 30)
                            .padding(.vertical, 3)
                            .padding(.horizontal, 4)
                            .background(.quaternary, in: RoundedRectangle(cornerRadius: 5))
                            .accessibilityLabel("Number of Fixtures to Create")
                        Button { model.apply { $0.addFixture() } } label: {
                            Image(systemName: "plus").frame(width: 16, height: 16)
                        }
                        .accessibilityLabel("Add")
                        .help("Select to increment the number of fixtures to create.")
                        Button { model.apply { $0.removeFixture() } } label: {
                            Image(systemName: "minus").frame(width: 16, height: 16)
                        }
                        .accessibilityLabel("Remove")
                        .help("Select to decrement the number of fixtures to create.")
                        .disabled(!state.canRemoveFixture)
                    }
                }
                FixtureFieldRow(
                    title: "Element Prefix:",
                    help: "Name prefix used in the display element tree for the fixtures.",
                    text: model.binding(state.prefix) { $0.setPrefix(text: $1) },
                    width: 200
                )
                GridRow {
                    Toggle("Create Group", isOn: model.binding(state.createGroup) { $0.setCreateGroup(on: $1) })
                        .toggleStyle(.checkbox)
                        .help("Select to nest the fixtures under a group in the display element tree.")
                        .gridCellColumns(2)
                }
                FixtureFieldRow(
                    title: "Group Name:",
                    help: "Name of the fixture group in the display element tree.",
                    text: model.binding(state.groupName) { $0.setGroupName(text: $1) },
                    enabled: state.createGroup,
                    width: 200
                )
            }
            GroupBox("Display Elements Preview") {
                List {
                    if state.createGroup {
                        Text(state.groupName)
                            .listRowSeparator(.hidden)
                    }
                    ForEach(Array(state.previewFixtures.enumerated()), id: \.offset) { _, name in
                        Label {
                            Text(name)
                        } icon: {
                            Circle()
                                .fill(.green)
                                .frame(width: 8, height: 8)
                        }
                        .padding(.leading, state.createGroup ? 16 : 0)
                        .listRowSeparator(.hidden)
                    }
                }
                .listStyle(.plain)
                .scrollContentBackground(.hidden)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
    }
}

/// F147.
struct FixtureSummaryPage: View {
    let state: SetupFixtureWizardState

    var body: some View {
        Grid(alignment: .topLeading, horizontalSpacing: 20, verticalSpacing: 14) {
            ForEach(state.summary, id: \.title) { row in
                GridRow {
                    Text(row.title)
                        .fontWeight(.semibold)
                    Text(row.text)
                        .textSelection(.enabled)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
        }
    }
}
