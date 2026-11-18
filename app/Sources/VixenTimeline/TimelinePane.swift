import AppKit
import SwiftUI
import VixenBridge
import VixenKit

/// The timeline as a SwiftUI view, for the sequencer window's main area. Everything inside is AppKit.
///
/// Make one `TimelineCanvas` per window and keep it, so its state lives as long as the window: `TimelinePane(canvas: canvas)`. Commands go straight to
/// `canvas.controller`, and what they return goes through `canvas.apply(_:)`.
public struct TimelinePane: NSViewRepresentable {
    private let canvas: TimelineCanvas

    public init(canvas: TimelineCanvas) {
        self.canvas = canvas
    }

    public func makeNSView(context: Context) -> TimelineContainerView {
        // SwiftUI can take a view down and make it again while the window stays.
        canvas.resume()
        canvas.view.studioUseCanvasAppearance() // STUDIO: the timeline stays dark in light mode
        return canvas.view
    }

    public func updateNSView(_ nsView: TimelineContainerView, context: Context) {}

    public static func dismantleNSView(_ nsView: TimelineContainerView, coordinator: ()) {
        nsView.canvas?.stop()
    }
}
