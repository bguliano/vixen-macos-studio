import AppKit
import SwiftUI

/// Display Setup's pane inside the show window. The show window (Administration area) can't import Display Setup, so Display Setup
/// registers the pane here and the show window hosts it.
@MainActor
public enum StudioDisplayPane {
    /// Builds the pane: Display, or with `true` the Controllers section, which shares its working copy. Nil in a build without
    /// Display Setup, where the show window keeps its summary and the separate window.
    public static var make: ((_ controllers: Bool) -> AnyView)?
    /// Asked before the show window leaves the Display and Controllers sections: false keeps it there. The live pane publishes and saves what
    /// it holds, then lets its working copy go.
    public static var confirmLeave: (() async -> Bool)?
    /// Brings the Display section forward, for Setup Display in the menus and a failed apply that needs its edits back on screen.
    public static var reveal: (() -> Void)?
    /// Moves the show window from Controllers to Display, for Find Elements, whose result is selected there.
    public static var showDisplay: (() -> Void)?
    /// Publishes and saves anything the pane still holds. The app awaits it before shutting down.
    public static var flush: (() async -> Void)?
    /// Turns Live Output off, for the pane's notice while editing is paused.
    public static var stopLiveOutput: (() -> Void)?
    /// The window hosting the pane, for the few dialogs Display Setup attaches as sheets.
    public static weak var hostWindow: NSWindow?
}
