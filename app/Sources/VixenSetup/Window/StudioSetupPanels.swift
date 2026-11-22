import SwiftUI
import VixenBridge
import VixenKit

/// Studio's Display Setup side columns. Same model calls as Vixen's `ElementsPanel` and `ControllersPanel`, with
/// labelled actions, an Add menu instead of a type picker plus button, and explanations where controls are disabled.

private struct StudioColumnHeader<Trailing: View>: View {
    let title: String
    @ViewBuilder let trailing: Trailing

    var body: some View {
        HStack {
            Text(title)
                .font(.headline)
            Spacer(minLength: StudioMetrics.spacingS)
            trailing
        }
    }
}

private struct StudioActionButton: View {
    let title: String
    let symbol: String
    var enabled = true
    var help: String? = nil
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Label(title, systemImage: symbol)
                .labelStyle(.titleAndIcon)
        }
        .disabled(!enabled)
        .help(help ?? title)
    }
}

/// A row of labelled buttons that wraps when the column is narrow.
private struct StudioActionRow<Content: View>: View {
    @ViewBuilder let content: Content

    var body: some View {
        WrappingHStack(spacing: StudioMetrics.spacingXS, lineSpacing: StudioMetrics.spacingXS) {
            content
        }
        .buttonStyle(.bordered)
        .controlSize(.small)
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

/// Lays children out left to right, wrapping to new lines.
struct WrappingHStack: Layout {
    var spacing: CGFloat = 4
    var lineSpacing: CGFloat = 4

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        arrange(width: proposal.width ?? .infinity, subviews: subviews).size
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let result = arrange(width: bounds.width, subviews: subviews)
        for (index, point) in result.points.enumerated() {
            subviews[index].place(at: CGPoint(x: bounds.minX + point.x, y: bounds.minY + point.y), proposal: .unspecified)
        }
    }

    private func arrange(width: CGFloat, subviews: Subviews) -> (size: CGSize, points: [CGPoint]) {
        var points: [CGPoint] = []
        var x: CGFloat = 0, y: CGFloat = 0, lineHeight: CGFloat = 0, widest: CGFloat = 0
        for view in subviews {
            let size = view.sizeThatFits(.unspecified)
            if x > 0, x + size.width > width {
                y += lineHeight + lineSpacing
                x = 0
                lineHeight = 0
            }
            points.append(CGPoint(x: x, y: y))
            x += size.width + spacing
            lineHeight = max(lineHeight, size.height)
            widest = max(widest, x - spacing)
        }
        return (CGSize(width: widest, height: y + lineHeight), points)
    }
}

// MARK: Elements

struct StudioElementsPanel: View {
    @Bindable var model: SetupModel

    var body: some View {
        VStack(alignment: .leading, spacing: StudioMetrics.spacingS) {
            StudioColumnHeader(title: "Elements") {
                Menu {
                    ForEach(model.templates, id: \.name) { template in
                        Button(template.name) {
                            model.templateName = template.name
                            Task { await model.addFromTemplate() }
                        }
                    }
                } label: {
                    Label("Add", systemImage: "plus")
                        .labelStyle(.titleAndIcon)
                }
                .fixedSize()
                .help("Add a single item, a group, or a prop from a template such as a mega tree or arch")
                .disabled(model.studioEditingPaused)
            }
            ElementOutline(model: model)
                .frame(minHeight: 160)
                .background(.background.secondary, in: RoundedRectangle(cornerRadius: StudioMetrics.smallCornerRadius))
                .clipShape(RoundedRectangle(cornerRadius: StudioMetrics.smallCornerRadius))
            selection
                .disabled(model.studioEditingPaused)
        }
        .padding(StudioMetrics.spacingM)
    }

    @ViewBuilder
    private var selection: some View {
        let any = model.elementCount > 0
        StudioActionRow {
            StudioActionButton(title: "Rename…", symbol: "pencil", enabled: any) { Task { await model.renameElements() } }
            StudioActionButton(title: "Delete", symbol: "trash", enabled: any) { Task { await model.deleteElements() } }
            StudioActionButton(title: "Find Outputs", symbol: "text.magnifyingglass", enabled: any,
                               help: "Select the controller outputs these elements are patched to") { Task { await model.findOutputs() } }
        }
        if !any {
            Text("Select elements to rename, delete, configure or patch them.")
                .font(.caption)
                .foregroundStyle(.secondary)
        } else {
            VStack(alignment: .leading, spacing: StudioMetrics.spacingS) {
                Text("Selected Elements")
                    .font(.subheadline.weight(.semibold))
                HStack(spacing: StudioMetrics.spacingS) {
                    Picker("Setup", selection: $model.helperName) {
                        ForEach(model.helperNames, id: \.self) { Text($0).tag($0) }
                    }
                    .help("Setup helpers configure many elements at once, such as color handling or dimming curves")
                    Button("Run…") { Task { await model.runHelper() } }
                }
                .controlSize(.small)
                Text("Properties")
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.secondary)
                if model.properties.isEmpty {
                    Text("No properties. Add one to set color handling, dimming or orientation.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                } else {
                    List(model.properties, id: \.typeId, selection: $model.propertySelection) { row in
                        Text(row.typeName).lineLimit(1)
                    }
                    .listStyle(.plain)
                    .frame(minHeight: 60, maxHeight: 110)
                    .clipShape(RoundedRectangle(cornerRadius: StudioMetrics.smallCornerRadius))
                    .contextMenu(forSelectionType: String.self) { _ in
                    } primaryAction: { _ in
                        Task { await model.configureProperty() }
                    }
                }
                StudioActionRow {
                    StudioActionButton(title: "Add…", symbol: "plus") { Task { await model.addProperty() } }
                    StudioActionButton(title: "Remove", symbol: "minus", enabled: !model.propertySelection.isEmpty) { Task { await model.deleteProperties() } }
                    StudioActionButton(title: "Configure…", symbol: "gearshape", enabled: model.propertySelection.count == 1) { Task { await model.configureProperty() } }
                }
            }
            .studioCard(padding: StudioMetrics.spacingS)
        }
    }
}

// MARK: Controllers

struct StudioControllersPanel: View {
    @Bindable var model: SetupModel
    var inline = false
    /// In the Controllers section, which lists the controllers itself: only the selected controller's inspector and actions.
    var section = false
    /// The single selected controller is E1.31 or DDP, so the network inspector edits it.
    @State private var networkController = false
    /// The single selected controller belongs to a plugin, so the plugin inspector edits its settings.
    @State private var pluginController = false

    var body: some View {
        let panel = model.controllerPanel
        VStack(alignment: .leading, spacing: StudioMetrics.spacingS) {
            if !section {
                StudioColumnHeader(title: "Controllers") {
                    Menu {
                        StudioAddControllerMenuItems(model: model)
                    } label: {
                        Label("Add", systemImage: "plus")
                            .labelStyle(.titleAndIcon)
                    }
                    .fixedSize()
                    .help("Add a controller such as E1.31 (sACN), DDP, a serial device or a plugin's type")
                    .disabled(model.studioEditingPaused)
                }
                ControllerOutline(model: model)
                    .frame(minHeight: 160)
                    .background(.background.secondary, in: RoundedRectangle(cornerRadius: StudioMetrics.smallCornerRadius))
                    .clipShape(RoundedRectangle(cornerRadius: StudioMetrics.smallCornerRadius))
                HStack(spacing: StudioMetrics.spacingM) {
                    legend(.green, "Running")
                    legend(.red, "Stopped")
                }
                .font(.caption)
                .foregroundStyle(.secondary)
            }
            if panel.anyController {
                VStack(alignment: .leading, spacing: StudioMetrics.spacingS) {
                    if panel.oneController && networkController {
                        StudioControllerNetworkInspector(model: model)
                        Divider()
                    }
                    if panel.oneController && pluginController {
                        StudioPluginControllerInspector(model: model)
                        Divider()
                    }
                    // The network inspector already shows the protocol and outputs.
                    if !(panel.oneController && networkController) {
                        Grid(alignment: .leading, horizontalSpacing: StudioMetrics.spacingS, verticalSpacing: 2) {
                            GridRow {
                                Text("Type").foregroundStyle(.secondary)
                                Text(panel.typeLabel).lineLimit(1).truncationMode(.tail)
                            }
                            GridRow {
                                Text("Outputs").foregroundStyle(.secondary)
                                Text(panel.outputsLabel).monospacedDigit()
                            }
                        }
                        .font(.callout)
                    }
                    StudioActionRow {
                        if panel.canStart {
                            StudioActionButton(title: "Start", symbol: "play.fill", help: "Start sending data to this controller") { Task { await model.startControllers() } }
                        }
                        if panel.canStop {
                            StudioActionButton(title: "Stop", symbol: "stop.fill", help: "Stop sending data to this controller") { Task { await model.stopControllers() } }
                        }
                        if networkController {
                            StudioActionButton(title: "Advanced…", symbol: "gearshape", enabled: panel.oneController,
                                               help: "Multicast, a universe table set by hand, priority and other settings") { Task { await model.configureController() } }
                        } else {
                            if !pluginController {
                                StudioActionButton(title: "Configure…", symbol: "gearshape", enabled: panel.oneController,
                                                   help: "Network address, universes and other settings") { Task { await model.configureController() } }
                            }
                            StudioActionButton(title: "Outputs…", symbol: "number", enabled: panel.oneController,
                                               help: "Change how many outputs (channels) this controller has") { Task { await model.channelCount() } }
                        }
                        StudioActionButton(title: "Rename…", symbol: "pencil", enabled: panel.oneController) { Task { await model.renameController() } }
                        StudioActionButton(title: "Find Elements", symbol: "text.magnifyingglass", enabled: panel.canFindElements,
                                           help: "Select the elements patched to the selected outputs") {
                            Task {
                                await model.findElements()
                                // The elements are in Display.
                                if section { StudioDisplayPane.showDisplay?() }
                            }
                        }
                        Menu {
                            Button("Move Up") { Task { await model.moveControllers(up: true) } }
                            Button("Move Down") { Task { await model.moveControllers(up: false) } }
                        } label: {
                            Label("Order", systemImage: "arrow.up.arrow.down")
                        }
                        .fixedSize()
                        StudioActionButton(title: "Delete", symbol: "trash") { Task { await model.deleteControllers() } }
                    }
                    if !panel.oneController {
                        Text("Select a single controller to configure, rename or change its outputs.")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
                .studioCard(padding: StudioMetrics.spacingS)
                .disabled(model.studioEditingPaused)
            } else {
                Text(section
                    ? "Select a controller to see its settings and start, stop, test or configure it."
                    : "Select a controller to start, stop or configure it. Double-click to open its settings.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer(minLength: 0)
            // In the show window the pane's own bar applies or discards, across the whole width.
            if !inline {
                HStack(spacing: StudioMetrics.spacingS) {
                    Button("Help") { model.help() }
                    Spacer(minLength: 0)
                    Button("Cancel") { model.cancel() }
                        .keyboardShortcut(.cancelAction)
                    Button("Save") { model.ok() }
                        .keyboardShortcut(.defaultAction)
                        .help("Save the display and controller changes and close Display Setup")
                }
                .controlSize(.regular)
            }
        }
        .padding(StudioMetrics.spacingM)
        .task(id: model.controllerPanel) {
            networkController = await model.studioSelectedNetworkController()
            pluginController = await model.studioSelectedPluginController()
        }
    }

    private func legend(_ color: Color, _ text: String) -> some View {
        HStack(spacing: 4) {
            Circle().fill(color).frame(width: 7, height: 7)
            Text(text)
        }
    }
}
