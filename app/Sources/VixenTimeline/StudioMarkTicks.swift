import CoreGraphics
import VixenBridge

// Studio: quieter marks (UI roadmap R35). Marks are coloured ticks in the ruler and the waveform; only the selected
// marks' collections draw lines across the grid, and the rest join them as overlay lines while an effect drag snaps.

/// A mark on the ruler: a solid tick in its collection's colour, rising from the bottom edge, wider when selected.
nonisolated func paintStudioRulerTick(_ line: TimelineLine, _ ctx: CGContext, height: CGFloat) {
    let tall = (height * 0.5).rounded()
    ctx.setFillColor(TimelineColors.argb(line.color))
    fillColumn(ctx, x: CGFloat(line.x), top: height - tall, height: tall, width: line.width > 1 ? 3 : 2)
}

/// The waveform's marks for a tile: short ticks at its top and bottom edges, clear of the wave in the middle.
nonisolated func paintStudioWaveformTicks(_ request: TileRequest, _ ctx: CGContext, controller: TimelineController, height: CGFloat) {
    let r = request.rect
    let ticks = controller.studioMarkTicks(x: Int32(r.minX), width: Int32(r.width), timePerPixel: request.timePerPixel)
    guard !ticks.isEmpty, height > 0 else { return }
    let tall = max(4, (height * 0.18).rounded())
    for line in ticks {
        let width: CGFloat = line.width > 1 ? 3 : 2
        ctx.setFillColor(TimelineColors.argb(line.color))
        fillColumn(ctx, x: CGFloat(line.x), top: r.minY, height: tall, width: width)
        fillColumn(ctx, x: CGFloat(line.x), top: r.minY + height - tall, height: tall, width: width)
    }
}
