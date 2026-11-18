import AppKit
import SwiftUI
import VixenBridge
import VixenKit

/// Used when `VixenSetup` isn't in the build: lists every dialog, so each can be opened and photographed with real data.
public struct DialogsTestScene: Scene {
    private let model: AppModel

    public init(model: AppModel) {
        self.model = model
    }

    public var body: some Scene {
        Window("Dialogs Test", id: "dialogs-test") {
            DialogsTestView()
                .environment(model)
                .studioAppearance() // STUDIO: follow the chosen appearance
                .task { await model.start() }
        }
        .defaultSize(width: 980, height: 760)
        .defaultLaunchBehavior(.suppressed)
        .restorationBehavior(.disabled)
        ControllerMonitorScene(model: model)
    }
}

private struct DialogsTestView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        if let core = model.core {
            DialogsTestList(context: core.dialogContext())
        } else {
            ProgressView("Opening the profile")
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }
}

/// What's open.
private enum TestSheet: Identifiable {
    case controller(String)
    case filter(String)
    case property(String, String)
    case helper(DialogSetupHelper)
    case chroma, luma, maskFill

    var id: String {
        switch self {
        case let .controller(c): "c\(c)"
        case let .filter(f): "f\(f)"
        case let .property(n, p): "p\(n)\(p)"
        case let .helper(h): "h\(h)"
        case .chroma: "chroma"
        case .luma: "luma"
        case .maskFill: "mask"
        }
    }
}

private struct DialogsTestList: View {
    let context: DialogContext
    @State private var controllers: [DialogController] = []
    @State private var filters: [(String, String)] = []
    @State private var nodes: [DialogTestNode] = []
    @State private var nodeID = ""
    @State private var sheet: TestSheet?
    @State private var result = "Nothing yet"
    @State private var alert: DialogAlert?

    private static let propertyTypes: [(String, String)] = [
        ("Color Property", "bff34727-6b88-4f87-82b7-68424498c725"),
        ("Location Property", "3fb53423-dd2a-4719-b3e3-19aa6f062f62"),
        ("Orientation Property", "db0fc9fc-cdd5-4f88-bd66-5e4cf08feae4"),
        ("Order Property", "47d4a43b-5407-44bd-8be1-4974b148504f"),
        ("Grid Property", "de3b4ec9-4e7f-4690-b0fe-3780b17a85ec"),
        ("Face Property", "eda73851-8350-4d3f-9df2-0d3b34e70bb6"),
        ("State Property", "35b893c3-d10c-4359-82bc-929c0762e809"),
    ]

    private static let helpers: [(String, DialogSetupHelper)] = [
        ("Color Handling", .colorHandling),
        ("Dimming Curve", .dimmingCurve),
        ("Face Mapping", .faceMapping),
        ("Orientation", .orientation),
        ("Patching Order", .patchingOrder),
        ("State Mapping", .stateMapping),
    ]

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                HStack {
                    Text("Last result: \(result)").monospacedDigit()
                    Spacer()
                    Button("Add Test Controllers", action: addControllers)
                    Button("Add Test Filters", action: addFilters)
                    Button("Save Profile", action: save)
                }
                section("Controllers") {
                    ForEach(controllers, id: \.id) { c in
                        Button("\(c.name) [\(c.typeName)]") { sheet = .controller(c.id) }
                            .help(c.status ?? "")
                    }
                }
                section("Monitors") {
                    Button("Start Monitors") { monitor(start: true) }
                    Button("Stop Monitors") { monitor(start: false) }
                }
                section("Filters") {
                    ForEach(filters, id: \.0) { filter in
                        Button(filter.1) { sheet = .filter(filter.0) }
                    }
                }
                HStack {
                    Text("Element:")
                    Picker("Element:", selection: $nodeID) {
                        ForEach(nodes, id: \.id) { Text($0.name).tag($0.id) }
                    }
                    .labelsHidden()
                    .fixedSize()
                }
                section("Properties") {
                    ForEach(Self.propertyTypes, id: \.0) { title, type in
                        Button(title) { openProperty(type) }
                    }
                }
                section("Helpers") {
                    ForEach(Self.helpers, id: \.0) { title, helper in
                        Button(title) { sheet = .helper(helper) }
                    }
                }
                section("Layers") {
                    Button("Chroma Key") { sheet = .chroma }
                    Button("Luma Key") { sheet = .luma }
                    Button("Mask And Fill") { sheet = .maskFill }
                }
            }
            .padding(20)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .onAppear(perform: refresh)
        .sheet(item: $sheet) { open in
            content(open)
        }
        .dialogAlert($alert)
    }

    private func section(_ title: String, @ViewBuilder content: () -> some View) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(title).font(.headline)
            FlowRow { content() }
        }
    }

    @ViewBuilder
    private func content(_ open: TestSheet) -> some View {
        let finish = { (changed: Bool) in
            result = "\(open.id) \(changed ? "OK" : "Cancel")"
            sheet = nil
            refresh()
        }
        switch open {
        case let .controller(id):
            ControllerSetupSheet(controller: id, context: context, onFinish: finish)
        case let .filter(id):
            FilterSetupSheet(filter: id, context: context, onFinish: finish)
        case let .property(node, type):
            PropertySetupSheet(element: node, selection: [node], property: type, context: context, onFinish: finish)
        case let .helper(helper):
            SetupHelperSheet(helper: helper, nodes: [nodeID], context: context, onFinish: finish)
        case .chroma:
            ChromaKeySheet(context: context, draft: context.defaultChromaKey()) { finish($0 != nil) }
        case .luma:
            LumaKeySheet(context: context, limits: context.defaultLumaKey()) { finish($0 != nil) }
        case .maskFill:
            MaskFillSheet(draft: context.defaultMaskFill()) { finish($0 != nil) }
        }
    }

    private func refresh() {
        controllers = context.controllers()
        filters = context.filterTypeIds().compactMap { type in
            guard let first = (try? context.filtersOfType(typeId: type))?.first, let info = try? context.filter(filter: first) else { return nil }
            return (first, info.name.isEmpty ? String(describing: info.kind) : info.name)
        }
        nodes = context.testNodes()
        if nodeID.isEmpty || !nodes.contains(where: { $0.id == nodeID }) {
            nodeID = nodes.first?.id ?? ""
        }
    }

    private func addControllers() {
        for entry in context.testControllerTypes() {
            let parts = entry.split(separator: "|", maxSplits: 1).map(String.init)
            guard parts.count == 2, !controllers.contains(where: { $0.typeName == parts[1] }) else { continue }
            _ = try? context.testAddController(typeId: parts[0], name: "Test \(parts[1])", outputs: 512)
        }
        refresh()
    }

    private func addFilters() {
        for type in context.filterTypeIds() where ((try? context.filtersOfType(typeId: type)) ?? []).isEmpty {
            _ = try? context.testAddFilter(typeId: type)
        }
        refresh()
    }

    private func openProperty(_ type: String) {
        sheet = .property(nodeID, type)
    }

    private func save() {
        do {
            try context.testSaveProfile()
            result = "Saved"
        } catch {
            alert = .notice("Save Profile", error.userMessage)
        }
    }

    private func monitor(start: Bool) {
        for controller in controllers where controller.kind == .dummyLighting || controller.typeName == "Debugging Values" {
            if start {
                do {
                    try context.testStartMonitor(controller: controller.id, value: 0)
                    result = "Started \(controller.name)"
                } catch {
                    alert = .notice("Monitor", error.userMessage)
                }
            } else {
                try? context.testStopMonitor(controller: controller.id)
                result = "Stopped \(controller.name)"
            }
        }
    }
}

/// Buttons that wrap.
private struct FlowRow: Layout {
    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width ?? 800
        var (x, y, rowHeight) = (CGFloat.zero, CGFloat.zero, CGFloat.zero)
        for view in subviews {
            let size = view.sizeThatFits(.unspecified)
            if x + size.width > width {
                x = 0
                y += rowHeight + 6
                rowHeight = 0
            }
            x += size.width + 6
            rowHeight = max(rowHeight, size.height)
        }
        return CGSize(width: width, height: y + rowHeight)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var (x, y, rowHeight) = (bounds.minX, bounds.minY, CGFloat.zero)
        for view in subviews {
            let size = view.sizeThatFits(.unspecified)
            if x + size.width > bounds.maxX {
                x = bounds.minX
                y += rowHeight + 6
                rowHeight = 0
            }
            view.place(at: CGPoint(x: x, y: y), proposal: .unspecified)
            x += size.width + 6
            rowHeight = max(rowHeight, size.height)
        }
    }
}
