import SwiftUI
import VixenBridge
import VixenKit

// What Display Setup, the sequencer and the Layer Editor call. Each view is a sheet's whole content, and calls `onFinish` once it's done, with
// true when something changed.
//
// Pass a `DialogContext` to say where a dialog reads and writes. Display Setup passes one on its working copy, so its Cancel drops what the
// dialogs did. A context from `core.dialogContext()` works on the open profile.

extension AppModel {
    /// A context on the open profile.
    var dialogContextOnProfile: DialogContext? {
        core?.dialogContext()
    }
}

/// F1: Configure on a controller. A controller with no setup finishes at once with false, so check `controllerKind` first to not show a sheet.
public struct ControllerSetupSheet: View {
    let controller: String
    let context: DialogContext?
    let onFinish: (Bool) -> Void

    public init(controller: String, context: DialogContext, onFinish: @escaping (Bool) -> Void) {
        self.controller = controller
        self.context = context
        self.onFinish = onFinish
    }

    public init(controller: String, model: AppModel, onFinish: @escaping (Bool) -> Void) {
        self.controller = controller
        context = model.dialogContextOnProfile
        self.onFinish = onFinish
    }

    public var body: some View {
        if let context {
            content(context)
        } else {
            NothingToConfigure(onFinish: onFinish)
        }
    }

    @ViewBuilder
    private func content(_ context: DialogContext) -> some View {
        switch try? context.controllerKind(controller: controller) {
        case .genericSerial:
            if let draft = try? context.genericSerial(controller: controller) {
                GenericSerialSheet(context: context, controller: controller, draft: draft, onFinish: onFinish)
            }
        case .dummyLighting:
            if let draft = try? context.dummyLighting(controller: controller) {
                DummyLightingSheet(context: context, controller: controller, draft: draft, onFinish: onFinish)
            }
        case .e131:
            E131Host(context: context, controller: controller, onFinish: onFinish)
        case .ddp:
            if let draft = try? context.ddp(controller: controller) {
                DdpSheet(context: context, controller: controller, draft: draft, onFinish: onFinish)
            }
        case .serialPortOnly:
            SerialOnlySheet(
                context: context,
                controller: controller,
                current: (try? context.serialOnlySettings(controller: controller)) ?? nil,
                onFinish: onFinish
            )
        case .openDmx:
            if let draft = try? context.openDmx(controller: controller) {
                OpenDmxSheet(context: context, controller: controller, draft: draft, onFinish: onFinish)
            }
        case .elexol:
            if let draft = try? context.elexol(controller: controller) {
                ElexolSheet(context: context, controller: controller, draft: draft, onFinish: onFinish)
            }
        case .rds:
            if let draft = try? context.rds(controller: controller) {
                RdsSheet(context: context, controller: controller, draft: draft, onFinish: onFinish)
            }
        case .launcher:
            if let hide = try? context.launcherHidesWindows(controller: controller) {
                LauncherSheet(context: context, controller: controller, hide: hide, onFinish: onFinish)
            }
        case .some(.none), nil:
            NothingToConfigure(onFinish: onFinish)
        }
    }
}

/// Opens the draft once the sheet is really there, since opening pauses the controller.
private struct E131Host: View {
    let context: DialogContext
    let controller: String
    let onFinish: (Bool) -> Void
    @State private var session: E131Session?

    var body: some View {
        Group {
            if let session {
                E131Sheet(context: context, session: session, onFinish: onFinish)
            } else {
                ProgressView().padding(40)
            }
        }
        .onAppear {
            if session == nil {
                session = try? context.openE131(controller: controller)
                if session == nil {
                    onFinish(false)
                }
            }
        }
    }
}

/// A module with no setup does nothing.
private struct NothingToConfigure: View {
    let onFinish: (Bool) -> Void

    var body: some View {
        Color.clear.frame(width: 1, height: 1).onAppear { onFinish(false) }
    }
}

/// F2: Configure on a filter. On true, redraw what shows its outputs, since their count may have changed.
public struct FilterSetupSheet: View {
    let filter: String
    let context: DialogContext?
    let onFinish: (Bool) -> Void

    public init(filter: String, context: DialogContext, onFinish: @escaping (Bool) -> Void) {
        self.filter = filter
        self.context = context
        self.onFinish = onFinish
    }

    public init(filter: String, model: AppModel, onFinish: @escaping (Bool) -> Void) {
        self.filter = filter
        context = model.dialogContextOnProfile
        self.onFinish = onFinish
    }

    public var body: some View {
        if let context {
            content(context)
        } else {
            NothingToConfigure(onFinish: onFinish)
        }
    }

    @ViewBuilder
    private func content(_ context: DialogContext) -> some View {
        switch (try? context.filter(filter: filter))?.kind {
        case .colorBreakdown:
            if let draft = try? context.colorBreakdown(filter: filter) {
                ColorBreakdownSheet(context: context, filter: filter, draft: draft, onFinish: onFinish)
            }
        case .coarseFine:
            if let draft = try? context.coarseFine(filter: filter) {
                CoarseFineSheet(context: context, filter: filter, draft: draft, onFinish: onFinish)
            }
        case .dimmingCurve:
            if let curve = try? context.dimmingCurve(filter: filter) {
                DimmingCurveFilterSheet(context: context, filter: filter, curve: curve, onFinish: onFinish)
            }
        case .tagged, .dimming, .colorWheel, .shutter, .prism:
            if let labels = try? context.tagFilterLabels(filter: filter), let draft = try? context.tagFilter(filter: filter) {
                TagFilterSheet(context: context, filter: filter, labels: labels, draft: draft, onFinish: onFinish)
            }
        case .some(.none), nil:
            NothingToConfigure(onFinish: onFinish)
        }
    }
}

/// F3: Configure on a property. `element` is the tree's primary node, whose property the row shows, and `selection` every selected element.
public struct PropertySetupSheet: View {
    let element: String
    let selection: [String]
    let property: String
    let context: DialogContext?
    let onFinish: (Bool) -> Void

    public init(element: String, selection: [String], property: String, context: DialogContext, onFinish: @escaping (Bool) -> Void) {
        self.element = element
        self.selection = selection
        self.property = property
        self.context = context
        self.onFinish = onFinish
    }

    public init(element: String, selection: [String], property: String, model: AppModel, onFinish: @escaping (Bool) -> Void) {
        self.element = element
        self.selection = selection
        self.property = property
        context = model.dialogContextOnProfile
        self.onFinish = onFinish
    }

    public var body: some View {
        if let context {
            content(context)
        } else {
            NothingToConfigure(onFinish: onFinish)
        }
    }

    @ViewBuilder
    private func content(_ context: DialogContext) -> some View {
        switch try? context.propertyKind(typeId: property) {
        case .color:
            ColorHelperSheet(context: context, nodes: selection, preset: element, onFinish: onFinish)
        case .location:
            if let location = try? context.location(node: element) {
                LocationSheet(context: context, element: element, selection: selection, location: location, onFinish: onFinish)
            }
        case .orientation:
            OrientationSheet(context: context, initial: (try? context.orientation(node: element)) ?? "Vertical") { value in
                guard let value else {
                    onFinish(false)
                    return
                }
                onFinish((try? context.applyOrientation(node: element, selection: selection, value: value)) != nil)
            }
        case .order:
            if let value = try? context.orderValue(node: element) {
                OrderValueSheet(context: context, element: element, selection: selection, value: value, onFinish: onFinish)
            }
        case .grid:
            if let grid = try? context.grid(node: element) {
                GridSheet(context: context, element: element, selection: selection, grid: grid, onFinish: onFinish)
            }
        case .face:
            FaceSheet(context: context, nodes: selection, onFinish: onFinish)
        case .state:
            // `SetupElements` quietly does nothing unless exactly one node is selected.
            if selection.count == 1 {
                StateMapperHost(context: context, node: selection[0], onFinish: onFinish)
            } else {
                NothingToConfigure(onFinish: onFinish)
            }
        case .intelligentFixture:
            FixturePropertySheet(context: context, node: element, selection: selection, onFinish: onFinish)
        case .some(.none), nil:
            NothingToConfigure(onFinish: onFinish)
        }
    }
}

private struct StateMapperHost: View {
    let context: DialogContext
    let node: String
    let onFinish: (Bool) -> Void
    @State private var session: StateSession?

    var body: some View {
        Group {
            if let session {
                StateMapperSheet(context: context, session: session, onFinish: onFinish)
            } else {
                ProgressView().padding(40)
            }
        }
        .onAppear {
            if session == nil {
                session = try? context.openStateMapper(node: node)
                if session == nil {
                    onFinish(false)
                }
            }
        }
    }
}

private struct OrderHelperHost: View {
    let context: DialogContext
    let nodes: [String]
    let onFinish: (Bool) -> Void
    @State private var session: OrderSession?

    var body: some View {
        Group {
            if let session {
                OrderHelperSheet(session: session, onFinish: onFinish)
            } else {
                ProgressView().padding(40)
            }
        }
        .onAppear {
            if session == nil {
                session = context.openOrderHelper(nodes: nodes)
            }
        }
    }
}

/// F193: a message in place of the mapper, when it's given anything but one node.
private struct RefusalNotice: View {
    let prompt: DialogStatePrompt
    let onFinish: (Bool) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            DialogTitle(prompt.title)
            Text(prompt.text)
            HStack {
                Spacer()
                Button("OK") { onFinish(false) }.keyboardShortcut(.defaultAction)
            }
        }
        .padding(20)
        .frame(width: 360)
    }
}

/// F4: the Configure combo's helpers. `nodes` is every selected node. After running one, refresh the tree and properties list whatever the result.
/// `simple` is the Dimming Curve helper's simple mode, and `colorType` starts the Color helper on a choice, as after an element template (F6).
public struct SetupHelperSheet: View {
    let helper: DialogSetupHelper
    let nodes: [String]
    let simple: Bool
    let colorType: DialogColorType?
    let silent: Bool
    let context: DialogContext?
    let onFinish: (Bool) -> Void

    public init(
        helper: DialogSetupHelper,
        nodes: [String],
        simple: Bool = false,
        colorType: DialogColorType? = nil,
        silent: Bool = false,
        context: DialogContext,
        onFinish: @escaping (Bool) -> Void
    ) {
        self.helper = helper
        self.nodes = nodes
        self.simple = simple
        self.colorType = colorType
        self.silent = silent
        self.context = context
        self.onFinish = onFinish
    }

    public init(helper: DialogSetupHelper, nodes: [String], model: AppModel, onFinish: @escaping (Bool) -> Void) {
        self.helper = helper
        self.nodes = nodes
        simple = false
        colorType = nil
        silent = false
        context = model.dialogContextOnProfile
        self.onFinish = onFinish
    }

    public var body: some View {
        if let context {
            content(context)
        } else {
            NothingToConfigure(onFinish: onFinish)
        }
    }

    @ViewBuilder
    private func content(_ context: DialogContext) -> some View {
        switch helper {
        case .colorHandling:
            ColorHelperSheet(context: context, nodes: nodes, preset: nil, initialType: colorType, silent: silent, onFinish: onFinish)
        case .dimmingCurve:
            DimmingHelperSheet(context: context, nodes: nodes, simple: simple, onFinish: onFinish)
        case .faceMapping:
            FaceSheet(context: context, nodes: nodes, onFinish: onFinish)
        case .orientation:
            OrientationSheet(context: context, initial: "Vertical") { value in
                guard let value else {
                    onFinish(false)
                    return
                }
                context.applyOrientationHelper(nodes: nodes, value: value)
                onFinish(true)
            }
        case .patchingOrder:
            OrderHelperHost(context: context, nodes: nodes, onFinish: onFinish)
        case .stateMapping:
            if let refusal = context.stateMappingRefusal(nodes: nodes) {
                RefusalNotice(prompt: refusal, onFinish: onFinish)
            } else {
                StateMapperHost(context: context, node: nodes[0], onFinish: onFinish)
            }
        }
    }
}

/// F5: Configuration on a layer in the Layer Editor. Show the button only when `session.layerFilterKind(layer:)` isn't `.none`.
public struct LayerFilterSetupSheet: View {
    let layer: String
    let session: SequencerSession
    let context: DialogContext
    let onFinish: (Bool) -> Void

    public init(layer: String, session: SequencerSession, context: DialogContext, onFinish: @escaping (Bool) -> Void) {
        self.layer = layer
        self.session = session
        self.context = context
        self.onFinish = onFinish
    }

    public var body: some View {
        switch try? session.layerFilterKind(layer: layer) {
        case .chromaKey:
            if let draft = try? session.chromaKey(layer: layer) {
                ChromaKeySheet(context: context, draft: draft, apply: { try session.applyChromaKey(layer: layer, draft: $0) }) { edited in // STUDIO: live (B9)
                    guard let edited else {
                        onFinish(false)
                        return
                    }
                    onFinish((try? session.applyChromaKey(layer: layer, draft: edited)) != nil)
                }
            }
        case .lumaKey:
            if let limits = try? session.lumaKey(layer: layer) {
                LumaKeySheet(context: context, limits: limits, apply: { try session.applyLumaKey(layer: layer, limits: $0) }) { edited in // STUDIO: live (B9)
                    guard let edited else {
                        onFinish(false)
                        return
                    }
                    onFinish((try? session.applyLumaKey(layer: layer, limits: edited)) != nil)
                }
            }
        case .maskFill:
            if let draft = try? session.maskFill(layer: layer) {
                MaskFillSheet(draft: draft, apply: { try session.applyMaskFill(layer: layer, draft: $0) }) { edited in // STUDIO: live (B9)
                    guard let edited else {
                        onFinish(false)
                        return
                    }
                    onFinish((try? session.applyMaskFill(layer: layer, draft: edited)) != nil)
                }
            }
        case .some(.none), nil:
            NothingToConfigure(onFinish: onFinish)
        }
    }
}
