import SwiftUI

/// The preview designer inside the show window's Previews pane. The show window (Administration area) can't import the
/// preview area, so the designer registers itself here and the pane hosts it.
@MainActor
public enum StudioPreviewDesignerPane {
    /// Builds the designer for a preview. Nil in a build without the preview designer, where Edit Layout opens its window.
    public static var make: ((_ previewID: String) -> AnyView)?
    /// Leaves the designer the pane shows: the prompts closing its window asked (invalid links, OpenGL), then the write-back,
    /// one step on the shared undo history. False when the user kept editing.
    public static var leave: (() async -> Bool)?
    /// Writes the layout back without asking. The app awaits it before shutting down.
    public static var flush: (() async -> Void)?
}
