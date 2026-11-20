import AppKit
import SwiftUI
import VixenBridge
import VixenKit
#if canImport(VixenDialogs)
import VixenDialogs
#endif

/// Runs one element template for a window that isn't Display Setup (`ElementTemplateRunner` in VixenKit), on a working copy of its own: the same dialogs
/// in the same order as `SetupModel.addFromTemplate`, as sheets on the asking window, and then the copy becomes the running display. Nothing is saved:
/// the asking window's own save or reload decides, as upstream's preview setup leaves it.
enum TemplateRunner {
    static func register() {
        ElementTemplateRunner.run = { name, window, app, memory in
            await run(name: name, on: window, app: app, memory: memory)
        }
        ElementPropSetup.run = { nodes, color, window, app in
            await importedProp(nodes: nodes, color: color, on: window, app: app)
        }
    }

    /// The questions `PreviewCustomPropBuilder.CreateAsync` asks after it has made a prop's elements.
    private static func importedProp(nodes: [String], color: ImportedPropColor, on window: NSWindow, app: AppModel) async -> Bool {
        guard let core = app.core, !nodes.isEmpty else { return false }
        let session = await offMain { core.openDisplaySetup() }
        var ran = false
        let answer = await alert(on: window, "Dimming Curve Setup", "Would you like to configure a dimming curve for this element?", ["Configure", "Not Now"])
        if answer == 0 {
            let _: Bool = await sheet(on: window) { finish in
                DialogEntryPoint(request: .helper(name: "Dimming Curve", nodes: nodes, simple: true, startFullColor: false), session: session, onFinish: finish)
            }
            ran = true
        }
        if color != .other {
            let _: Bool = await sheet(on: window) { finish in
                ImportedPropColorSheet(nodes: nodes, color: color, session: session, onFinish: finish)
            }
            ran = true
        }
        guard ran else { return false }
        _ = await offMain { session.applyLive() }
        return true
    }

    private static func run(name: String, on window: NSWindow, app: AppModel, memory: ElementTemplateMemory?) async -> ElementTemplateResult? {
        guard let core = app.core, let info = setupElementTemplates().first(where: { $0.name == name }) else { return nil }
        let session = await offMain { core.openDisplaySetup() }
        let result: ElementTemplateResult? =
            if info.kind == .intelligentFixture {
                await fixtures(session: session, on: window)
            } else {
                await template(info, session: session, on: window, memory: memory)
            }
        guard let result else { return nil }
        _ = await offMain { session.applyLive() }
        return result
    }

    private static func template(_ info: SetupTemplateInfo, session: DisplaySetupSession, on window: NSWindow, memory: ElementTemplateMemory?) async -> ElementTemplateResult? {
        guard let draft = memory?.values[info.name] as? SetupTemplateDraft ?? setupTemplateDraft(kind: info.kind) else { return nil }
        let chosen: SetupTemplateDraft? = await sheet(on: window) { finish in
            TemplateSheet(draft: draft, rememberOnCancel: { memory?.values[info.name] = $0 }, onFinish: finish)
        }
        guard let chosen else { return nil }
        memory?.values[info.name] = chosen
        let outcome = await offMain { Result { try session.generateTemplate(draft: chosen) } }
        guard case .success(let made) = outcome else {
            _ = await alert(on: window, "Error", "Could not create elements.\nPlease close all dialogs and try again.\n\nPlease send error logs to Vixen Team.", ["OK"])
            return nil
        }
        let created = await offMain { session.nodeIdsOf(selection: made.createdPaths) }
        // Cancelling either helper doesn't remove what was made.
        if info.askDimming {
            let answer = await alert(on: window, "Dimming Curve Setup", "Would you like to configure a dimming curve for this Prop?", ["Configure", "Not Now"])
            if answer == 0 {
                let _: Bool = await sheet(on: window) { finish in
                    DialogEntryPoint(request: .helper(name: "Dimming Curve", nodes: created, simple: true, startFullColor: false), session: session, onFinish: finish)
                }
            }
        }
        // STUDIO-BEGIN template-runner-color: the template's Colour choice replaces the Color Handling sheet where it's offered
        if StudioTemplateColor.supports(info.kind) {
            let mode = StudioTemplateColor.mode(for: info.kind)
            await offMain { StudioTemplateColor.apply(mode, to: created, in: session) }
        } else if info.runColor {
            let _: Bool = await sheet(on: window) { finish in
                DialogEntryPoint(request: .helper(name: "Color Handling", nodes: created, simple: false, startFullColor: true), session: session, onFinish: finish)
            }
        }
        // STUDIO-END template-runner-color
        let selected = await offMain { session.nodeIdsOf(selection: made.edit.select) }
        return ElementTemplateResult(nodes: selected, selection: selected)
    }

    private static func fixtures(session: DisplaySetupSession, on window: NSWindow) async -> ElementTemplateResult? {
        let before = session.nodeWatermark()
        let edit: SetupEditResult? = await sheet(on: window, resizable: true) { finish in
            FixtureWizardView(model: FixtureWizardModel(session: session, completion: finish))
        }
        guard let edit else { return nil }
        let (fixtures, selected) = await offMain { (session.leavesCreatedSince(watermark: before), session.nodeIdsOf(selection: edit.select)) }
        return ElementTemplateResult(nodes: fixtures, selection: selected)
    }

    /// One sheet on a window that isn't ours to give a `.sheet` modifier to. Returns once it has been taken down, so the next one can go up.
    static func sheet<Value: Sendable, Content: View>(on window: NSWindow, resizable: Bool = false, @ViewBuilder _ content: (@escaping (Value) -> Void) -> Content) async -> Value {
        await withCheckedContinuation { continuation in
            let panel = NSWindow(contentRect: .zero, styleMask: resizable ? [.titled, .resizable] : [.titled], backing: .buffered, defer: true)
            panel.isReleasedWhenClosed = false
            var finished = false
            let view = content { value in
                guard !finished else { return }
                finished = true
                window.endSheet(panel)
                continuation.resume(returning: value)
            }
            let host = NSHostingController(rootView: view.studioAppearance()) // STUDIO: follow the chosen appearance
            // Sized once: a sheet that follows its content's preferred size can ask for constraint passes without end as it animates in, which AppKit answers with an exception.
            host.sizingOptions = []
            let size = host.view.fittingSize
            panel.contentViewController = host
            panel.setContentSize(size)
            if resizable {
                panel.contentMinSize = size
            }
            window.beginSheet(panel)
        }
    }

    /// The index of the button pressed.
    static func alert(on window: NSWindow, _ title: String, _ text: String, _ buttons: [String]) async -> Int {
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = text
        for button in buttons {
            alert.addButton(withTitle: button)
        }
        let response = await alert.beginSheetModal(for: window)
        return response.rawValue - NSApplication.ModalResponse.alertFirstButtonReturn.rawValue
    }
}

/// The colour helper preset to a prop file's colour mode. Full colour is applied without its dialog, as upstream's `SilentMode` does.
private struct ImportedPropColorSheet: View {
    let nodes: [String]
    let color: ImportedPropColor
    let session: DisplaySetupSession
    let onFinish: (Bool) -> Void

    var body: some View {
        #if canImport(VixenDialogs)
        SetupHelperSheet(helper: .colorHandling, nodes: nodes, colorType: type, silent: color == .fullColor, context: session.dialogContext(), onFinish: onFinish)
        #else
        DialogEntryPoint(request: .helper(name: "Color Handling", nodes: nodes, simple: false, startFullColor: color == .fullColor), session: session, onFinish: onFinish)
        #endif
    }

    #if canImport(VixenDialogs)
    private var type: DialogColorType {
        switch color {
        case .fullColor: .fullColor
        case .multiple: .multipleDiscreteColors
        case .single, .other: .singleColor
        }
    }
    #endif
}
