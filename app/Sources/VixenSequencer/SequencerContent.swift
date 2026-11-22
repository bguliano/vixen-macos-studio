import AppKit
import SwiftUI
import VixenBridge
import VixenKit

#if canImport(VixenEffectEditor)
import VixenEffectEditor
#endif

#if canImport(VixenMarks)
import VixenMarks
#endif

#if canImport(VixenTimeline)
import VixenTimeline
#endif

/// The window's regions, top to bottom, as upstream: tool strips; Effects, Timeline and the tabbed panels; the status bar.
struct SequencerContent: View {
    @Bindable var model: SequencerModel

    var body: some View {
        VStack(spacing: 0) {
            // STUDIO-BEGIN native-toolbar: the window's native toolbar replaces Vixen's tool strips
            DockArea(model: model)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            Divider()
            StatusBarView(model: model)
        }
        .toolbar(id: "studio.sequencer") { StudioSequencerToolbar(model: model) }
        .toolbarRole(.editor)
        .background(WindowReader { StudioToolbarDefaults.applyOnce(to: $0) })
        // STUDIO-END native-toolbar
        .background(KeyCatcher(model: model))
        .sheet(item: $model.sheet) { sheet in
            SequencerSheetView(model: model, sheet: sheet)
        }
    }
}

/// Holds the first responder when nothing else wants it, so a key press with no control focused reaches the window's hotkeys.
private struct KeyCatcher: NSViewRepresentable {
    let model: SequencerModel

    final class CatcherView: NSView {
        weak var model: SequencerModel?

        override var acceptsFirstResponder: Bool { true }

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            if let window, window.firstResponder === window {
                window.makeFirstResponder(self)
            }
        }

        override func keyDown(with event: NSEvent) {
            if model?.handleKey(event) != true {
                super.keyDown(with: event)
            }
        }
    }

    func makeNSView(context: Context) -> CatcherView {
        let view = CatcherView()
        view.model = model
        return view
    }

    func updateNSView(_ nsView: CatcherView, context: Context) {}
}

/// `dockPanel`: Effects on the left, the timeline in the middle, the tabbed panels on the right. An `NSSplitView`, because the side panes hold their width as the window resizes and
/// come back at the width they had. Panels hide rather than dispose, so what's in them survives (F2).
struct DockArea: NSViewRepresentable {
    let model: SequencerModel

    func makeCoordinator() -> Coordinator {
        Coordinator(model: model)
    }

    func makeNSView(context: Context) -> NSSplitView {
        let split = NSSplitView()
        split.isVertical = true
        split.dividerStyle = .thin
        split.delegate = context.coordinator
        let left = hosted(DockPane(model: model, side: .left))
        let centre = hosted(TimelineRegion(model: model))
        let right = hosted(DockPane(model: model, side: .right))
        for pane in [left, centre, right] {
            split.addArrangedSubview(pane)
        }
        split.setHoldingPriority(NSLayoutConstraint.Priority(260), forSubviewAt: 0)
        split.setHoldingPriority(NSLayoutConstraint.Priority(200), forSubviewAt: 1)
        split.setHoldingPriority(NSLayoutConstraint.Priority(260), forSubviewAt: 2)
        StudioDockLayout.constrain([left, centre, right]) // STUDIO: panes keep their minimum widths as the window narrows (R42)
        context.coordinator.split = split
        return split
    }

    private func hosted(_ view: some View) -> NSView {
        let host = NSHostingView(rootView: view)
        // The split view decides sizes, not what's inside.
        host.sizingOptions = []
        return host
    }

    func updateNSView(_ split: NSSplitView, context: Context) {
        context.coordinator.apply(model.panels)
    }

    @MainActor
    final class Coordinator: NSObject, NSSplitViewDelegate {
        let model: SequencerModel
        weak var split: NSSplitView?
        private var placed = false
        private var leftHidden = false
        private var rightHidden = false

        init(model: SequencerModel) {
            self.model = model
        }

        /// Shows or hides the side panes to match the layout, and puts the dividers where they were saved.
        func apply(_ panels: ShellPanels?) {
            guard let split, let panels, split.arrangedSubviews.count == 3 else { return }
            let hideLeft = panels.leftTabs.isEmpty
            let hideRight = panels.rightTabs.isEmpty || model.studioInspectorCollapsed // STUDIO: the Inspector button collapses the whole drawer (#69)
            let changed = !placed || hideLeft != leftHidden || hideRight != rightHidden
            guard changed else { return }
            leftHidden = hideLeft
            rightHidden = hideRight
            split.arrangedSubviews[0].isHidden = hideLeft
            split.arrangedSubviews[2].isHidden = hideRight
            split.adjustSubviews()
            place(panels)
        }

        private func place(_ panels: ShellPanels) {
            guard let split else { return }
            // Sizes aren't known until there's a window to lay out in.
            if split.bounds.width < 1 {
                DispatchQueue.main.async { [weak self] in
                    if let panels = self?.model.panels {
                        self?.place(panels)
                    }
                }
                return
            }
            placed = true
            // STUDIO-BEGIN dock-saved-minimums: saved widths fit the window, never below the panes' minimums (R42)
            StudioDockLayout.apply(to: split, left: leftHidden ? nil : panels.leftWidth, right: rightHidden ? nil : panels.rightWidth)
            // STUDIO-END dock-saved-minimums
        }

        func splitView(_ splitView: NSSplitView, constrainMinCoordinate proposedMinimumPosition: CGFloat, ofSubviewAt dividerIndex: Int) -> CGFloat {
            // STUDIO-BEGIN dock-minimums: dragging a divider stops at the panes' minimum widths (R42)
            dividerIndex == 0 ? max(proposedMinimumPosition, StudioDockLayout.leftMinimum) : max(proposedMinimumPosition, splitView.arrangedSubviews[0].frame.maxX + StudioDockLayout.centreMinimum)
            // STUDIO-END dock-minimums
        }

        func splitView(_ splitView: NSSplitView, constrainMaxCoordinate proposedMaximumPosition: CGFloat, ofSubviewAt dividerIndex: Int) -> CGFloat {
            if dividerIndex == 0 {
                return min(proposedMaximumPosition, splitView.arrangedSubviews[2].frame.minX - StudioDockLayout.centreMinimum) // STUDIO: the timeline's minimum (R42)
            }
            return min(proposedMaximumPosition, splitView.bounds.width - StudioDockLayout.rightMinimum) // STUDIO: the right pane's minimum (R42)
        }

        // STUDIO-BEGIN dock-window-resize: a window resize changes the timeline; the side panes give way only down to their minimums (R42)
        func splitView(_ splitView: NSSplitView, resizeSubviewsWithOldSize oldSize: NSSize) {
            StudioDockLayout.whileResizing {
                splitView.adjustSubviews()
                guard placed, let panels = model.panels else { return }
                StudioDockLayout.apply(to: splitView, left: leftHidden ? nil : panels.leftWidth, right: rightHidden ? nil : panels.rightWidth)
            }
        }
        // STUDIO-END dock-window-resize

        func splitViewDidResizeSubviews(_ notification: Notification) {
            guard placed, let split else { return }
            // STUDIO-BEGIN dock-save-drag: remember only the pane whose divider the user drags, not window resizes or a pane squeezed by a narrow window (R42)
            guard StudioDockLayout.isDividerDrag(in: split), let panels = model.panels else { return }
            let moved = StudioDockLayout.dragged(split, left: leftHidden ? nil : panels.leftWidth, right: rightHidden ? nil : panels.rightWidth)
            model.setPanelWidths(left: moved.left, right: moved.right)
            // STUDIO-END dock-save-drag
        }
    }
}

/// The timeline's tab: `Form_Grid`. Title "Timeline", changed by the audio it holds (F319), and with no close button.
struct TimelineRegion: View {
    let model: SequencerModel

    var body: some View {
        VStack(spacing: 0) {
            // STUDIO-BEGIN native-window-tabs: no "Timeline - song" tab strip; sequences tab natively (StudioSequencerTabs)
            timeline
            // STUDIO-END native-window-tabs
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    @ViewBuilder
    private var timeline: some View {
        #if canImport(VixenTimeline)
        if let canvas = model.canvas {
            TimelinePane(canvas: canvas)
        } else {
            placeholder
        }
        #else
        placeholder
        #endif
    }

    private var placeholder: some View {
        ContentUnavailableView {
            Label("Timeline", systemImage: "rectangle.split.3x1")
        } description: {
            if model.ui != nil {
                Text("The timeline joins this build with the VixenTimeline module.")
            } else {
                Text("Loading")
            }
        }
        .background(Color(nsColor: Theme.Timeline.panelBackground))
    }
}

/// One side's tabs. Upstream's sit along the bottom of the pane (`Editor.png`).
struct DockPane: View {
    let model: SequencerModel
    let side: ShellSide

    private var tabs: [ShellPanelTab] {
        (side == .left ? model.panels?.leftTabs : model.panels?.rightTabs) ?? []
    }

    private var current: ShellPanelTab? {
        let selected = side == .left ? model.panels?.selectedLeft : model.panels?.selectedRight
        return tabs.first { $0.panel == selected } ?? tabs.first
    }

    var body: some View {
        VStack(spacing: 0) {
            // STUDIO-BEGIN panel-tabs: labelled tabs along the top replace the header and the bottom tab row
            StudioPanelTabs(tabs: tabs, selected: current?.panel) {
                model.selectPanel($0, side: side)
            } detach: {
                model.detachPanel($0)
            } close: {
                model.togglePanel($0)
            }
            Divider()
            if let current {
                // Every panel stays alive behind the one showing, so switching tabs keeps its state.
                ZStack {
                    ForEach(tabs, id: \.panel) { tab in
                        PanelContent(model: model, panel: tab.panel)
                            .opacity(tab.panel == current.panel ? 1 : 0)
                            .allowsHitTesting(tab.panel == current.panel)
                            .accessibilityHidden(tab.panel != current.panel)
                            .environment(\.panelIsShowing, tab.panel == current.panel)
                    }
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .background(.background)
        // STUDIO-END panel-tabs
    }
}

/// A panel's title row: `Effect Editor`, with detach and close at the right.
struct PanelHeader: View {
    let title: String
    let detach: () -> Void
    let close: () -> Void

    var body: some View {
        HStack(spacing: 6) {
            Text(title)
                .font(.callout)
                .foregroundStyle(.secondary)
                .lineLimit(1)
            Spacer(minLength: 4)
            Button(action: detach) {
                Image(systemName: "rectangle.portrait.on.rectangle.portrait")
            }
            .help("Float this panel")
            .accessibilityLabel("Float \(title)")
            Button(action: close) {
                Image(systemName: "xmark")
            }
            .help("Close")
            .accessibilityLabel("Close \(title)")
        }
        .buttonStyle(.borderless)
        .controlSize(.small)
        .padding(.horizontal, 8)
        .padding(.vertical, 4)
    }
}

/// The tabs along the bottom. Wraps rather than hides when there are more than fit.
struct TabStrip: View {
    let tabs: [ShellPanelTab]
    let selected: ShellPanel?
    let choose: (ShellPanel) -> Void

    var body: some View {
        WrappingStack(spacing: 2, lineSpacing: 2) {
            ForEach(tabs, id: \.panel) { tab in
                Button {
                    choose(tab.panel)
                } label: {
                    Text(tab.title)
                        .font(.caption)
                        .lineLimit(1)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 3)
                        .foregroundStyle(tab.panel == selected ? Color.accentColor : Color.primary)
                        .background(tab.panel == selected ? Theme.inputBackground : Color.clear, in: RoundedRectangle(cornerRadius: 4))
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityAddTraits(tab.panel == selected ? .isSelected : [])
            }
        }
        .padding(.horizontal, 4)
        .padding(.vertical, 3)
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

/// Lays children left to right, wrapping to another line when they don't fit.
struct WrappingStack: Layout {
    var spacing: CGFloat = 4
    var lineSpacing: CGFloat = 4

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width ?? .infinity
        var x: CGFloat = 0
        var y: CGFloat = 0
        var lineHeight: CGFloat = 0
        var widest: CGFloat = 0
        for view in subviews {
            let size = view.sizeThatFits(.unspecified)
            if x > 0, x + size.width > width {
                y += lineHeight + lineSpacing
                x = 0
                lineHeight = 0
            }
            x += size.width + spacing
            widest = max(widest, x - spacing)
            lineHeight = max(lineHeight, size.height)
        }
        return CGSize(width: width.isFinite ? width : widest, height: y + lineHeight)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var x = bounds.minX
        var y = bounds.minY
        var lineHeight: CGFloat = 0
        for view in subviews {
            let size = view.sizeThatFits(.unspecified)
            if x > bounds.minX, x + size.width > bounds.maxX {
                y += lineHeight + lineSpacing
                x = bounds.minX
                lineHeight = 0
            }
            view.place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(size))
            x += size.width + spacing
            lineHeight = max(lineHeight, size.height)
        }
    }
}

/// What each panel shows.
struct PanelContent: View {
    let model: SequencerModel
    let panel: ShellPanel

    var body: some View {
        switch panel {
        case .effects:
            StudioEffectsLibrary(model: model) // STUDIO: searchable effects library that switches to Draw mode
        case .effectEditor:
            #if canImport(VixenEffectEditor)
            if let session = model.session {
                EffectEditorPane(session: session, selectedEffects: model.editorSelection) { _ in
                    model.refresh()
                    return true
                }
                // STUDIO-BEGIN sequence-inspector: with no effect selected, the inspector shows the sequence; the editor stays underneath so its pending edits still flush
                .accessibilityHidden(model.editorSelection.isEmpty)
                .overlay {
                    if model.editorSelection.isEmpty {
                        StudioSequenceInspector(model: model)
                            .background(.background)
                    }
                }
                // STUDIO-END sequence-inspector
            }
            #else
            PanelPlaceholder(title: "Effect Editor", detail: "The effect's properties show here, from the VixenEffectEditor module.", symbol: "slider.horizontal.3")
            #endif
        case .marks:
            #if canImport(VixenMarks)
            if let session = model.session {
                MarkCollectionsPanel(session: session)
            }
            #else
            PanelPlaceholder(title: "Mark Collections", detail: "Mark collections show here, from the VixenMarks module.", symbol: "metronome")
            #endif
        case .colorLibrary:
            LibraryPanel(model: model, kind: .color)
        case .curveLibrary:
            LibraryPanel(model: model, kind: .curve)
        case .gradientLibrary:
            LibraryPanel(model: model, kind: .gradient)
        case .layers:
            LayersPanel(model: model)
        case .findEffects:
            FindPanel(model: model)
        }
    }
}

/// A panel whose contents come from another module or a later pass.
struct PanelPlaceholder: View {
    let title: String
    let detail: String
    let symbol: String

    var body: some View {
        ContentUnavailableView {
            Label(title, systemImage: symbol)
        } description: {
            Text(detail)
        }
    }
}

/// A panel in its own `NSPanel`, floating beside the window.
final class FloatingPanelWindow: NSPanel, NSWindowDelegate {
    private weak var model: SequencerModel?
    private let panelID: ShellPanel
    private var closingProgrammatically = false

    init(title: String, model: SequencerModel, panel: ShellPanel) {
        self.model = model
        self.panelID = panel
        super.init(
            contentRect: NSRect(x: 0, y: 0, width: 320, height: 480),
            styleMask: [.titled, .closable, .resizable, .utilityWindow],
            backing: .buffered,
            defer: false
        )
        self.title = title
        isFloatingPanel = true
        hidesOnDeactivate = false
        isReleasedWhenClosed = false
        delegate = self
        contentView = NSHostingView(rootView: PanelContent(model: model, panel: panel).studioAppearance()) // STUDIO: follow the chosen appearance
        setFrameAutosaveName("SequencerPanel-\(title)")
    }

    /// Closing a floating panel puts it back in the dock.
    func windowWillClose(_ notification: Notification) {
        guard !closingProgrammatically else { return }
        model?.redockPanel(panelID)
    }

    func closeProgrammatically() {
        closingProgrammatically = true
        close()
    }
}
