import AppKit
import SwiftUI
import VixenBridge
import VixenKit

/// The window's three columns at 25%, 50% and 25%, with its prompts.
struct SetupRootView: View {
    @Bindable var model: SetupModel
    var inline = false // STUDIO: true inside the show window's Display section, which owns opening and closing
    var studioControllers = false // STUDIO: the show window's Controllers section: the controllers alone, with the same prompts

    var body: some View {
        Group {
            if model.session == nil {
                ProgressView("Display Setup")
                    .frame(minWidth: inline ? 820 : 1100, minHeight: inline ? 480 : 640) // STUDIO: fits the show window
            } else {
                // STUDIO-BEGIN setup-controllers-section: the Controllers section covers the columns, which stay alive for the controller tree's selection
                ZStack {
                    content
                        .opacity(studioControllers ? 0 : 1)
                        .allowsHitTesting(!studioControllers)
                        .accessibilityHidden(studioControllers)
                    if studioControllers {
                        StudioControllersSection(model: model)
                    }
                }
                // STUDIO-END setup-controllers-section
                    .id(model.openCount)
            }
        }
        // STUDIO-BEGIN setup-inline: the show window is not Display Setup's window, so it never closes or delegates it
        .background {
            if !inline {
                SetupWindowAccess(model: model)
            }
        }
        .onAppear { if !inline { model.windowOpened() } }
        // STUDIO-END setup-inline
        .alert(
            model.alert?.title ?? "",
            isPresented: Binding(get: { model.alert != nil }, set: { _ in }),
            presenting: model.alert
        ) { alert in
            ForEach(alert.choices) { choice in
                Button(choice.title, role: choice.role) { model.alertAnswered(alert, choice.value) }
            }
        } message: { alert in
            Text(alert.message)
        }
        .sheet(item: $model.sheet, onDismiss: { model.sheetDidDismiss() }) { sheet in
            SetupSheetContent(model: model, sheet: sheet)
        }
    }

    private var content: some View {
        HSplitView {
            // STUDIO-BEGIN setup-columns: Studio's labelled Elements and Controllers columns
            StudioElementsPanel(model: model)
                .frame(minWidth: inline ? 220 : 280, idealWidth: 320, maxHeight: .infinity)
                .background(SplitViewStart())
            PatchingPanel(model: model)
                .frame(minWidth: inline ? 360 : 480, idealWidth: 640, maxHeight: .infinity)
                .disabled(model.studioEditingPaused) // Live Output pauses editing; the side columns keep their trees browsable
            StudioControllersPanel(model: model, inline: inline)
                .frame(minWidth: inline ? 240 : 300, idealWidth: 340, maxHeight: .infinity)
            // STUDIO-END setup-columns
        }
        .frame(minWidth: inline ? 820 : 1100, minHeight: inline ? 480 : 640) // STUDIO: fits the show window
        .overlay(alignment: .top) {
            if model.isBusy {
                ProgressView()
                    .controlSize(.small)
                    .padding(6)
                    .background(.regularMaterial, in: Capsule())
                    .padding(.top, 8)
            }
        }
    }
}

/// F2, F3: the centre column. Both views stay alive, so each keeps its state while the other is showing.
private struct PatchingPanel: View {
    @Bindable var model: SetupModel

    var body: some View {
        VStack(spacing: 6) {
            // STUDIO-BEGIN patching-header: a title with a segmented view switch, and guidance until something is selected
            HStack {
                Text("Patching")
                    .font(.headline)
                Spacer()
                Picker("Patching view", selection: $model.usesGraphicalView) {
                    Text("Simple").tag(false)
                    Text("Graphical").tag(true)
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .fixedSize()
                .help("Simple patches selected elements to selected outputs in order; Graphical shows every connection")
            }
            .padding(.horizontal, StudioMetrics.spacingM)
            .padding(.top, StudioMetrics.spacingM)
            if !model.usesGraphicalView, model.elementCount == 0, !model.controllerPanel.anyController {
                Label("Select elements on the left and controllers or outputs on the right, then patch them here.", systemImage: "arrow.left.and.right")
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, StudioMetrics.spacingM)
            }
            // STUDIO-END patching-header
            ZStack {
                SimplePatchingView(model: model)
                    .opacity(model.usesGraphicalView ? 0 : 1)
                    .allowsHitTesting(!model.usesGraphicalView)
                    .accessibilityHidden(model.usesGraphicalView)
                GraphicalPatchingView(controller: model.graph)
                    .opacity(model.usesGraphicalView ? 1 : 0)
                    .allowsHitTesting(model.usesGraphicalView)
                    .accessibilityHidden(!model.usesGraphicalView)
            }
        }
    }
}

/// F2: `HSplitView` ignores ideal widths, so the columns start at 25%, 50% and 25% by being told so once, when the window first lays them out.
private struct SplitViewStart: NSViewRepresentable {
    final class Marker: NSView {
        private var done = false

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            place(attempt: 0)
        }

        private func place(attempt: Int) {
            guard !done, window != nil, attempt < 50 else { return }
            var view: NSView? = superview
            while let current = view, !(current is NSSplitView) {
                view = current.superview
            }
            guard let split = view as? NSSplitView, split.arrangedSubviews.count == 3, split.bounds.width > 800 else {
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.02) { [weak self] in
                    self?.place(attempt: attempt + 1)
                }
                return
            }
            done = true
            let width = split.bounds.width
            split.setPosition(width * 0.25, ofDividerAt: 0)
            split.setPosition(width * 0.75, ofDividerAt: 1)
        }
    }

    func makeNSView(context: Context) -> Marker {
        Marker()
    }

    func updateNSView(_ marker: Marker, context: Context) {}
}

/// What the model asked to show.
private struct SetupSheetContent: View {
    let model: SetupModel
    let sheet: SetupSheet

    var body: some View {
        switch sheet.kind {
        case .text(let title, let prompt, let initial, let selectAll, let finish):
            TextPromptSheet(title: title, prompt: prompt, initial: initial, selectAll: selectAll, onFinish: finish)
        case .number(let title, let prompt, let value, let min, let max, let finish):
            NumberPromptSheet(title: title, prompt: prompt, value: value, min: min, max: max, onFinish: finish)
        case .list(let title, let items, let finish):
            ListSelectSheet(title: title, items: items, onFinish: finish)
        case .template(let draft, let remember, let finish):
            TemplateSheet(draft: draft, rememberOnCancel: remember, onFinish: finish)
        case .nameGenerator(let seed, let oldNames, let finish):
            NameGeneratorSheet(seed: seed, oldNames: oldNames, onFinish: finish)
        case .renamer(let oldNames, let finish):
            SubstitutionRenamerSheet(oldNames: oldNames, onFinish: finish)
        case .tagColors(let rows, let finish):
            ManageTagColorsSheet(rows: rows, onFinish: finish)
        case .wireDiagram(let finish):
            WireDiagramSheet(onFinish: finish)
        case .dialog(let request, let finish):
            DialogEntryPoint(request: request, session: model.session, onFinish: finish)
        }
    }
}

/// Finds the window, and sits in its delegate's way for one question: may it close? (F7).
private struct SetupWindowAccess: NSViewRepresentable {
    let model: SetupModel

    final class Marker: NSView {
        var onWindow: ((NSWindow) -> Void)?

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            if let window {
                onWindow?(window)
            }
        }
    }

    final class CloseDelegate: NSObject, NSWindowDelegate {
        nonisolated(unsafe) private weak var original: NSWindowDelegate?
        let model: SetupModel

        init(original: NSWindowDelegate?, model: SetupModel) {
            self.original = original
            self.model = model
        }

        func windowShouldClose(_ sender: NSWindow) -> Bool {
            model.windowShouldClose()
        }

        func windowWillClose(_ notification: Notification) {
            model.windowClosed()
            original?.windowWillClose?(notification)
        }

        nonisolated override func responds(to selector: Selector!) -> Bool {
            super.responds(to: selector) || original?.responds(to: selector) == true
        }

        nonisolated override func forwardingTarget(for selector: Selector!) -> Any? {
            original
        }
    }

    final class Coordinator {
        var delegate: CloseDelegate?
    }

    func makeCoordinator() -> Coordinator {
        Coordinator()
    }

    func makeNSView(context: Context) -> Marker {
        let marker = Marker()
        let coordinator = context.coordinator
        marker.onWindow = { [model] window in
            model.window = window
            if !(window.delegate is CloseDelegate) {
                let delegate = CloseDelegate(original: window.delegate, model: model)
                coordinator.delegate = delegate
                window.delegate = delegate
            }
        }
        return marker
    }

    func updateNSView(_ marker: Marker, context: Context) {}
}
