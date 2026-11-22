import AppKit
import VixenBridge
import VixenKit

// The toolbar's Inspector button and View ▸ Hide Inspector (#69): they collapse and bring back the whole right-hand
// drawer, as a Mac inspector does. Toggling only the Effect Editor tab left the libraries, marks and layers holding the
// drawer open. Collapsing hides the pane without closing its tabs, so they come back as they were, at their width; new
// windows open the way the last one was left.

extension SequencerModel {
    static let studioInspectorCollapsedKey = "studio.sequencerInspectorCollapsed"

    /// The right-hand drawer is on screen: not collapsed, and with at least one tab docked there.
    var studioInspectorShown: Bool {
        !studioInspectorCollapsed && !(panels?.rightTabs.isEmpty ?? true)
    }

    func toggleStudioInspector() {
        if studioInspectorShown {
            setStudioInspectorCollapsed(true)
            return
        }
        setStudioInspectorCollapsed(false)
        // Every right-hand tab was closed, floated or moved: the Effect Editor comes back docked there.
        guard let panels, panels.rightTabs.isEmpty else { return }
        if panels.floating.contains(where: { $0.panel == .effectEditor }) {
            redockPanel(.effectEditor)
        } else if panels.visible.contains(.effectEditor) {
            togglePanel(.effectEditor)
            togglePanel(.effectEditor)
        } else {
            togglePanel(.effectEditor)
        }
    }

    /// A panel shown or docked on the right opens a collapsed drawer, so it never goes into a hidden pane.
    func studioRevealInspector(for panel: ShellPanel) {
        guard studioInspectorCollapsed, panels?.rightTabs.contains(where: { $0.panel == panel }) ?? false else { return }
        setStudioInspectorCollapsed(false)
    }

    private func setStudioInspectorCollapsed(_ collapsed: Bool) {
        guard studioInspectorCollapsed != collapsed else { return }
        studioInspectorCollapsed = collapsed
        UserDefaults.standard.set(collapsed, forKey: Self.studioInspectorCollapsedKey)
    }

    /// `sequencer.inspectorDrawer`: whether the drawer shows, and whether the window's right pane is really hidden.
    /// `path` ["toggle"] presses the toolbar's Inspector button first.
    func registerStudioInspectorHook(_ window: NSWindow) {
        UITestHarness.hooks["sequencer.inspectorDrawer"] = { [weak self, weak window] path in
            guard let self, let window else { return "FAIL no sequencer" }
            if path.first == "toggle" {
                self.toggleStudioInspector()
                window.contentView?.layoutSubtreeIfNeeded()
            }
            let pane = window.contentView.flatMap(Self.studioDockSplit)?.arrangedSubviews.last
            let paneText = pane.map { "paneHidden=\($0.isHidden) paneWidth=\(Int($0.frame.width))" } ?? "pane=none"
            return "shown=\(self.studioInspectorShown) collapsed=\(self.studioInspectorCollapsed) rightTabs=\(self.panels?.rightTabs.count ?? 0) \(paneText)"
        }
    }

    private static func studioDockSplit(_ view: NSView) -> NSSplitView? {
        if let split = view as? NSSplitView, split.isVertical, split.arrangedSubviews.count == 3 { return split }
        for child in view.subviews {
            if let found = studioDockSplit(child) { return found }
        }
        return nil
    }
}
