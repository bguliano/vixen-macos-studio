import AppKit
import QuartzCore
import VixenBridge

// Studio: agent edits flash on the timeline (UI roadmap R39). While the show window's toolbar shows an agent working,
// the effects it adds or changes in an open sequence light up once on the grid and fade, so you can see where it worked.

/// How long a flash takes to fade, and how much of that it holds at full strength first.
private let flashDuration: CFTimeInterval = 1.4
private let flashHold: NSNumber = 0.3
/// One shape for a flash, so a batch of thousands of effects adds one layer. Past this many tiles the rest go unlit.
private let flashLimit = 4000

extension TimelineCanvas {
    /// Flashes what agents added or changed after `since` (nil: the window's first look, which flashes nothing) and
    /// returns where to ask from next time.
    @discardableResult
    public func studioFlashAgentEdits(since: UInt64?) -> TimelineStudioAgentFlash {
        let flash = controller.studioAgentFlash(since: since)
        if !flash.rects.isEmpty {
            grid.studioFlash(flash.rects)
        }
        return flash
    }
}

extension GridCanvasView {
    /// The tiles, outlined and filled in the agent's colour, fading out. Drawn over the tiles in the layer that scrolls
    /// with them, under the cursor and hover box.
    func studioFlash(_ rects: [TimelineRect]) {
        let path = CGMutablePath()
        for r in rects.prefix(flashLimit) {
            let frame = plane.localFrame(canvas: CGRect(x: CGFloat(r.x), y: CGFloat(r.y), width: CGFloat(r.w), height: CGFloat(r.h)))
            path.addRoundedRect(in: frame.insetBy(dx: -1, dy: -1), cornerWidth: 2, cornerHeight: 2)
        }
        let shape = CAShapeLayer()
        shape.actions = noActions
        shape.anchorPoint = .zero
        shape.path = path
        let tint = NSColor.systemPurple
        shape.fillColor = tint.withAlphaComponent(0.45).cgColor
        shape.strokeColor = tint.cgColor
        shape.lineWidth = 2
        shape.opacity = 0
        shape.name = "studio-agent-flash"
        overlayHost.addSublayer(shape)

        let fade = CAKeyframeAnimation(keyPath: "opacity")
        fade.values = [1, 1, 0]
        fade.keyTimes = [0, flashHold, 1]
        fade.duration = flashDuration
        fade.timingFunctions = [CAMediaTimingFunction(name: .linear), CAMediaTimingFunction(name: .easeOut)]
        CATransaction.begin()
        CATransaction.setCompletionBlock { [weak shape] in shape?.removeFromSuperlayer() }
        shape.add(fade, forKey: "flash")
        CATransaction.commit()
    }
}
