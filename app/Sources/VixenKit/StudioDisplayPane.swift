import AppKit
import SwiftUI

/// Display Setup's pane inside the show window. The show window (Administration area) can't import Display Setup, so Display Setup
/// registers the pane here and the show window hosts it.
@MainActor
public enum StudioDisplayPane {
    /// Builds the pane. Nil in a build without Display Setup, where the show window keeps its summary and the separate window.
    public static var make: (() -> AnyView)?
    /// Asked before the show window leaves the Display section: false keeps it there. Clean edits close quietly; unapplied
    /// ones ask to apply, discard or keep editing.
    public static var confirmLeave: (() async -> Bool)?
    /// Brings the Display section forward, for Setup Display in the menus and a failed apply that needs its edits back on screen.
    public static var reveal: (() -> Void)?
    /// The window hosting the pane, for the few dialogs Display Setup attaches as sheets.
    public static weak var hostWindow: NSWindow?
}
