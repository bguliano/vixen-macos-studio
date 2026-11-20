import AppKit
import VixenKit

/// Where the sequence editor window opens. Upstream saves the editor's size and location when it closes and puts it back next time,
/// falling back to a default when the saved bounds aren't on any screen (`TimedSequenceEditorForm`, `IsVisibleOnAnyScreen`).
/// `setFrameAutosaveName` can't do this for a SwiftUI window group: SwiftUI keeps its own per-window autosave name, so the editor
/// opened wherever the system cascaded it, sometimes on another display or off screen.
@MainActor
enum SequencerWindowFrame {
    static let defaultsKey = "TimedSequenceEditorForm.Frame"
    private static let defaultSize = NSSize(width: 1440, height: 900)
    /// Windows whose frame is still being set up; their resize and move messages aren't the user's.
    private static var restoring: Set<ObjectIdentifier> = []

    static func restore(_ window: NSWindow) {
        guard !UITestHarness.isActive else { return }
        let id = ObjectIdentifier(window)
        restoring.insert(id)
        // Read it now: SwiftUI's own sizing of the new window would otherwise be saved over it first.
        let saved = UserDefaults.standard.string(forKey: defaultsKey).map(NSRectFromString)
        // After SwiftUI has placed the window, so this placement wins.
        DispatchQueue.main.async {
            window.setFrame(frame(saved: saved, minSize: window.minSize, screens: NSScreen.screens, main: NSScreen.main), display: true)
            restoring.remove(id)
        }
    }

    static func save(_ window: NSWindow?) {
        guard let window, !UITestHarness.isActive, !restoring.contains(ObjectIdentifier(window)), window.isVisible,
              !window.isMiniaturized, !window.styleMask.contains(.fullScreen) else { return }
        UserDefaults.standard.set(NSStringFromRect(window.frame), forKey: defaultsKey)
    }

    /// The saved frame if its title bar is reachable on some screen, or a default-sized window centred on the main screen.
    static func frame(saved: NSRect?, minSize: NSSize, screens: [NSScreen], main: NSScreen?) -> NSRect {
        let visibleFrames = screens.map(\.visibleFrame)
        if let saved, saved.width >= minSize.width, saved.height >= minSize.height,
           isReachable(saved, on: visibleFrames) {
            return saved
        }
        guard let area = (main ?? screens.first)?.visibleFrame else {
            return NSRect(origin: .zero, size: defaultSize)
        }
        return centered(in: area)
    }

    static func isReachable(_ frame: NSRect, on visibleFrames: [NSRect]) -> Bool {
        let titleBar = NSRect(x: frame.minX, y: frame.maxY - 28, width: frame.width, height: 28)
        return visibleFrames.contains { area in
            let overlap = area.intersection(titleBar)
            return overlap.width >= 120 && overlap.height >= 20
        }
    }

    static func centered(in area: NSRect) -> NSRect {
        let size = NSSize(width: min(defaultSize.width, area.width), height: min(defaultSize.height, area.height))
        return NSRect(x: (area.midX - size.width / 2).rounded(), y: (area.midY - size.height / 2).rounded(),
                      width: size.width, height: size.height)
    }
}
