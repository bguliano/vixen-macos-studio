import AppKit
import VixenKit

// Native window tabs for sequences (UI roadmap R37). Vanilla keeps one window per sequence with tabbing off and draws a
// "Timeline - song.mp3" tab above the grid; Studio drops that strip and lets sequencer windows tab with each other, so
// several open sequences sit in one window under the system's tab bar, each tab titled with its sequence. Tabs are the
// windows' arrangement, not the document, so there is no command or Undo for them: dragging a tab out, Window ▸ Merge
// All Windows and Move Tab to New Window are the system's own.

enum StudioSequencerTabs {
    /// Only sequencer windows join each other's tabs; the show window, previews and panels stay on their own.
    static let identifier = "io.github.bguliano.vixen.sequencer"

    /// Called once the window exists. SwiftUI has already shown it by then, so it joins the frontmost other sequencer
    /// window's tabs itself rather than relying on AppKit's automatic tabbing at ordering in.
    @MainActor
    static func adopt(_ window: NSWindow) {
        window.tabbingMode = .preferred
        window.tabbingIdentifier = identifier
        // After the saved frame is restored (also queued), so the restore can't resize the group it joins.
        DispatchQueue.main.async { [weak window] in
            guard let window, window.isVisible, (window.tabbedWindows?.count ?? 1) <= 1,
                  let host = host(for: window) else { return }
            host.addTabbedWindow(window, ordered: .above)
            window.tabGroup?.selectedWindow = window
            // Test windows never take the keyboard, so a script can't steal focus from whoever is at the machine.
            if NSApp.isActive, !UITestHarness.isActive {
                window.makeKeyAndOrderFront(nil)
            }
        }
        if UITestHarness.isActive {
            registerHook()
        }
    }

    /// The front-most other visible sequencer window: the active one when the user opened the sequence from it.
    @MainActor
    static func host(for window: NSWindow) -> NSWindow? {
        NSApp.orderedWindows.first {
            $0 !== window && $0.tabbingIdentifier == identifier && $0.isVisible && !$0.isMiniaturized
        } ?? NSApp.windows.first {
            $0 !== window && $0.tabbingIdentifier == identifier && $0.isVisible && !$0.isMiniaturized
        }
    }

    /// `sequencer.tabs`: every open sequencer window's tab group, titles in tab order with the selected one, so a script
    /// can check that sequences share one window. Not tied to a window, so it still answers after tabs close.
    @MainActor
    private static func registerHook() {
        UITestHarness.hooks["sequencer.tabs"] = { _ in
            let windows = NSApp.windows.filter { $0.tabbingIdentifier == identifier && $0.isVisible }
            var groups: [[String: Any]] = []
            var seen: Set<ObjectIdentifier> = []
            for window in windows where !seen.contains(ObjectIdentifier(window)) {
                let members = window.tabbedWindows ?? [window]
                members.forEach { seen.insert(ObjectIdentifier($0)) }
                groups.append(["tabs": members.map(\.title), "selected": window.tabGroup?.selectedWindow?.title ?? window.title,
                               "tabBarVisible": window.tabGroup?.isTabBarVisible ?? false,
                               "preferred": members.allSatisfy { $0.tabbingMode == .preferred }])
            }
            // AppKit adds Show Tab Bar, Merge All Windows and the rest to the Window menu for the key window when this is
            // on; test windows are never key, so the menu itself can't show them under the harness.
            let state: [String: Any] = ["sequencerWindows": windows.count, "groups": groups,
                                        "automaticTabbing": NSWindow.allowsAutomaticWindowTabbing]
            guard let data = try? JSONSerialization.data(withJSONObject: state, options: [.sortedKeys]) else { return "FAIL tabs serialization" }
            return String(decoding: data, as: UTF8.self)
        }
    }
}
