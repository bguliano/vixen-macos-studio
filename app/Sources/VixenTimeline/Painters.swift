import AppKit
import CoreText
import VixenBridge

/// Colours from upstream's `ThemeColorTable` and the control's constants. Painting threads use these.
nonisolated enum TimelineColors {
    static func rgb(_ r: CGFloat, _ g: CGFloat, _ b: CGFloat, _ a: CGFloat = 255) -> CGColor {
        CGColor(colorSpace: sRGBSpace, components: [r / 255, g / 255, b / 255, a / 255])!
    }

    static func argb(_ v: UInt32) -> CGColor {
        rgb(CGFloat((v >> 16) & 0xFF), CGFloat((v >> 8) & 0xFF), CGFloat(v & 0xFF), CGFloat((v >> 24) & 0xFF))
    }

    // STUDIO-BEGIN timeline-palette: a deeper, quieter dark palette so effects stand out (Vixen's greys in comments)
    /// `TimeLineEffectsBackColor` (Vixen: 68).
    static let effectsBackground = rgb(30, 30, 33)
    static let labelBackground = rgb(40, 40, 44)   // Vixen: 68
    static let panelBackground = rgb(24, 24, 27)   // Vixen: 40
    static let black = rgb(0, 0, 0)
    static let white = rgb(255, 255, 255)
    static let foreground = rgb(232, 232, 236)
    static let placeholder = rgb(96, 96, 102)      // Vixen: 122
    static let red = rgb(255, 69, 58)
    /// `SelectionColor`.
    static let selectionFill = rgb(10, 132, 255, 70)
    static let selectionBlue = rgb(10, 132, 255)
    static let selectionGreen = rgb(48, 209, 88)
    static let drawFill = rgb(255, 255, 255, 100)
    /// The ruler's background (Vixen: `Color.Gray`, 128).
    static let gray = rgb(44, 44, 48)
    /// `Color.DarkGray`.
    static let darkGray = rgb(169, 169, 169)
    static let waveformBackground = rgb(36, 36, 40)  // Vixen: 120
    static let waveformTop = rgb(150, 190, 255)     // Vixen: 60 on a light background
    static let waveformMid = rgb(70, 110, 210)      // Vixen: 20
    static let blue = rgb(0, 0, 255)
    static let green = rgb(0, 128, 0)
    static let yellow = rgb(255, 255, 0)
    static let activeYellow = rgb(255, 214, 10)
    static let infoBackground = rgb(0, 0, 0, 128)
    static let expanderBorder = rgb(120, 120, 128)
    static let expanderFill = rgb(58, 58, 64)
    static let cursor = rgb(50, 50, 50, 150)
    /// Studio: one-second gridlines and row separators, faint against the dark grid.
    static let gridLine = rgb(255, 255, 255, 22)
    static let rowSeparator = rgb(0, 0, 0, 160)
    /// Studio: ruler ticks, light against the dark ruler.
    static let rulerTick = rgb(150, 150, 158)
    // STUDIO-END timeline-palette
}

/// One point of GDI's type is a third larger than one of ours at the same pixel size: 96 against 72.
nonisolated func gdiPoints(_ points: CGFloat) -> CGFloat {
    points * 96 / 72
}

nonisolated func systemFont(size: CGFloat, bold: Bool = false, monospacedDigits: Bool = false) -> CTFont {
    let weight: NSFont.Weight = bold ? .bold : .regular
    let font = monospacedDigits ? NSFont.monospacedDigitSystemFont(ofSize: size, weight: weight) : NSFont.systemFont(ofSize: size, weight: weight)
    return font as CTFont
}

nonisolated func textLine(_ text: String, font: CTFont, color: CGColor) -> CTLine {
    let attributes: [CFString: Any] = [kCTFontAttributeName: font, kCTForegroundColorAttributeName: color]
    return CTLineCreateWithAttributedString(CFAttributedStringCreate(nil, text as CFString, attributes as CFDictionary))
}

/// Draws a line of text with its baseline at `y`, in a context whose y points down.
nonisolated func draw(_ line: CTLine, at point: CGPoint, in ctx: CGContext) {
    ctx.saveGState()
    ctx.setShouldAntialias(true)
    ctx.setShouldSmoothFonts(false)
    ctx.textMatrix = CGAffineTransform(scaleX: 1, y: -1)
    ctx.textPosition = point
    CTLineDraw(line, ctx)
    ctx.restoreGState()
}

/// `(width, ascent, descent)`.
nonisolated func measure(_ line: CTLine) -> (CGFloat, CGFloat, CGFloat) {
    var ascent: CGFloat = 0
    var descent: CGFloat = 0
    var leading: CGFloat = 0
    let width = CTLineGetTypographicBounds(line, &ascent, &descent, &leading)
    return (CGFloat(width), ascent, descent)
}

/// A vertical line one point wide at a canvas x, on the pixel grid.
nonisolated func fillColumn(_ ctx: CGContext, x: CGFloat, top: CGFloat, height: CGFloat, width: CGFloat = 1) {
    ctx.fill(CGRect(x: (x - (width - 1) / 2).rounded(.down), y: top, width: width, height: height))
}

/// A dashed vertical line, as fills so thousands stay cheap. The pattern is pinned to the canvas, so it runs on across tiles.
nonisolated func fillDashedColumn(_ ctx: CGContext, x: CGFloat, top: CGFloat, height: CGFloat, on: CGFloat, off: CGFloat, width: CGFloat = 1) {
    let period = on + off
    guard period > 0 else { return }
    let left = (x - (width - 1) / 2).rounded(.down)
    var y = (top / period).rounded(.down) * period
    let bottom = top + height
    while y < bottom {
        ctx.fill(CGRect(x: left, y: y, width: width, height: on))
        y += period
    }
}

// MARK: - Grid

/// F10: gridlines, row fills and separators, mark lines, effects. Selection borders and lifted effects are layers, above.
nonisolated func paintGridTile(_ request: TileRequest, _ ctx: CGContext, controller: TimelineController, pictures: PictureStore) -> [UInt64]? {
    let r = request.rect
    let frame = controller.gridFrame(x: Int32(r.minX), y: Int32(r.minY), w: Int32(r.width), h: Int32(r.height), timePerPixel: request.timePerPixel)
    guard !frame.failed else { return nil }
    ctx.setFillColor(TimelineColors.effectsBackground)
    ctx.fill(r)

    // 1 second dashes: `DashStyle.Dash` is 3 on and 1 off.
    ctx.setFillColor(TimelineColors.gridLine) // STUDIO: faint gridlines
    for x in frame.gridlines {
        fillDashedColumn(ctx, x: CGFloat(x), top: r.minY, height: r.height, on: 3, off: 1)
    }

    for row in frame.rows {
        let band = CGRect(x: r.minX, y: CGFloat(row.top), width: r.width, height: CGFloat(row.height))
        if row.selected {
            ctx.setFillColor(TimelineColors.selectionFill)
            ctx.fill(band)
            ctx.setFillColor(frame.selectionGreen ? TimelineColors.selectionGreen : TimelineColors.selectionBlue)
            ctx.fill(CGRect(x: band.minX, y: band.minY, width: band.width, height: 1))
            ctx.fill(CGRect(x: band.minX, y: band.maxY - 1, width: band.width, height: 1))
            if let edges = request.viewportEdges {
                ctx.fill(CGRect(x: edges.left, y: band.minY, width: 1, height: band.height))
                ctx.fill(CGRect(x: edges.right, y: band.minY, width: 1, height: band.height))
            }
        }
        if row.active {
            ctx.setFillColor(TimelineColors.selectionFill)
            ctx.fill(band)
        }
        ctx.setFillColor(TimelineColors.rowSeparator) // STUDIO: softer row separators
        ctx.fill(CGRect(x: band.minX, y: band.maxY - 1, width: band.width, height: 1))
    }

    for line in frame.markLines {
        paintMarkLine(line, ctx, top: r.minY, height: r.height)
    }

    var waiting: [UInt64] = []
    for q in frame.elements {
        let rect = CGRect(x: CGFloat(q.x), y: CGFloat(q.y), width: CGFloat(q.w), height: CGFloat(q.h))
        if q.rendered {
            if !pictures.draw(q, in: ctx, clip: r, scale: request.scale) {
                waiting.append(q.id)
            }
        } else {
            ctx.setFillColor(TimelineColors.placeholder)
            ctx.fill(rect)
        }
        paintBorder(rect, color: q.redBorder ? TimelineColors.red : TimelineColors.black, ctx)
    }
    return waiting
}

/// `AddSelectionOverlayToCanvas` for an unselected effect: one point inside the rectangle `(0, 0, w-1, h-1)`, so the right and bottom lines sit on the
/// last column and row.
nonisolated func paintBorder(_ rect: CGRect, color: CGColor, _ ctx: CGContext) {
    ctx.setFillColor(color)
    ctx.fill(CGRect(x: rect.minX, y: rect.minY, width: rect.width, height: 1))
    ctx.fill(CGRect(x: rect.minX, y: rect.maxY - 1, width: rect.width, height: 1))
    ctx.fill(CGRect(x: rect.minX, y: rect.minY, width: 1, height: rect.height))
    ctx.fill(CGRect(x: rect.maxX - 1, y: rect.minY, width: 1, height: rect.height))
}

nonisolated func paintMarkLine(_ line: TimelineLine, _ ctx: CGContext, top: CGFloat, height: CGFloat) {
    ctx.setFillColor(TimelineColors.argb(line.color))
    if line.dash > 0 {
        fillDashedColumn(ctx, x: CGFloat(line.x), top: top, height: height, on: CGFloat(line.dash), off: CGFloat(line.dash), width: CGFloat(line.width))
    } else {
        fillColumn(ctx, x: CGFloat(line.x), top: top, height: height, width: CGFloat(line.width))
    }
}

// MARK: - Row list

/// F21, F22 for a span of the list.
nonisolated func paintRowsTile(_ request: TileRequest, _ ctx: CGContext, controller: TimelineController) -> [UInt64]? {
    let r = request.rect
    let frame = controller.rowFrame(top: Int32(r.minY), bottom: Int32(r.maxY))
    guard !frame.failed else { return nil }
    ctx.setFillColor(TimelineColors.panelBackground)
    ctx.fill(r)

    // The dotted connectors are on the list's background, behind the labels.
    ctx.setFillColor(TimelineColors.foreground)
    for c in frame.connectors {
        if c.y0 == c.y1 {
            var x = CGFloat(c.x0)
            while x < CGFloat(c.x1) {
                ctx.fill(CGRect(x: x, y: CGFloat(c.y0), width: 1, height: 1))
                x += 2
            }
        } else {
            var y = (CGFloat(min(c.y0, c.y1)) / 2).rounded(.down) * 2
            while y < CGFloat(max(c.y0, c.y1)) {
                ctx.fill(CGRect(x: CGFloat(c.x0), y: y, width: 1, height: 1))
                y += 2
            }
        }
    }

    let inset = CGFloat(24)
    for row in frame.rows {
        let left = CGFloat(row.depth) * inset
        let band = CGRect(x: left, y: CGFloat(row.top), width: r.maxX - left, height: CGFloat(row.height))
        ctx.setFillColor(TimelineColors.labelBackground)
        ctx.fill(band)
        ctx.setFillColor(TimelineColors.black)
        // An inset border on the rectangle (0, -1, width - 1, height): right and bottom, and the top is the row above's.
        ctx.fill(CGRect(x: band.minX, y: band.maxY - 1, width: band.width, height: 1))
        ctx.fill(CGRect(x: band.minX, y: band.minY, width: 1, height: band.height))
        ctx.fill(CGRect(x: band.maxX - 1, y: band.minY, width: 1, height: band.height))

        let iconWidth = (row.expandable || row.depth == 0) ? inset : inset / 2
        if row.expandable {
            paintExpander(in: CGRect(x: left, y: band.minY, width: iconWidth, height: band.height), open: row.open, active: row.childHighlighted, ctx)
        }

        // Size min(12, height * 0.4), in GDI's points.
        let fontSize = max(1, gdiPoints(CGFloat(min(12, Int(CGFloat(row.height) * 0.4)))))
        let font = systemFont(size: fontSize)
        let color = row.highlighted ? TimelineColors.activeYellow : TimelineColors.foreground
        let box = CGRect(x: left + iconWidth, y: band.minY, width: band.width - iconWidth - 6, height: band.height)
        var line = textLine(row.name, font: font, color: color)
        var (w, ascent, descent) = measure(line)
        if w > box.width, box.width > 0 {
            let dots = textLine("…", font: font, color: color)
            if let cut = CTLineCreateTruncatedLine(line, Double(box.width), .end, dots) {
                line = cut
                (w, ascent, descent) = measure(line)
            }
        }
        ctx.saveGState()
        ctx.clip(to: box)
        draw(line, at: CGPoint(x: box.minX, y: box.midY + (ascent - descent) / 2), in: ctx)
        ctx.restoreGState()

        // Tag dots: 8 points, 3 apart, starting 6 after the text.
        ctx.setShouldAntialias(true)
        var x = box.minX + w + 6
        for color in row.tagColors {
            if x + 8 > band.maxX {
                break
            }
            ctx.setFillColor(TimelineColors.argb(color))
            ctx.fillEllipse(in: CGRect(x: x, y: band.midY - 4, width: 8, height: 8))
            x += 11
        }
        ctx.setShouldAntialias(false)
    }
    return []
}

/// Plus or minus in a box, centred in the icon area, on yellow when something below has effects (F23).
nonisolated func paintExpander(in area: CGRect, open: Bool, active: Bool, _ ctx: CGContext) {
    let size: CGFloat = 11
    let box = CGRect(x: (area.midX - size / 2).rounded(), y: (area.midY - size / 2).rounded(), width: size, height: size)
    ctx.setFillColor(active ? TimelineColors.activeYellow : TimelineColors.expanderFill)
    ctx.fill(box)
    paintBorder(box, color: TimelineColors.expanderBorder, ctx)
    ctx.setFillColor(active ? TimelineColors.black : TimelineColors.foreground)
    ctx.fill(CGRect(x: box.minX + 2, y: box.minY + 5, width: size - 4, height: 1))
    if !open {
        ctx.fill(CGRect(x: box.minX + 5, y: box.minY + 2, width: 1, height: size - 4))
    }
}

// MARK: - Ruler

/// F97 to F99, F104, F106.
nonisolated func paintRulerTile(_ request: TileRequest, _ ctx: CGContext, controller: TimelineController) -> [UInt64]? {
    let r = request.rect
    let f = controller.rulerFrame(x: Int32(r.minX), width: Int32(r.width), timePerPixel: request.timePerPixel)
    guard !f.failed else { return nil }
    let height = CGFloat(f.height)
    ctx.setFillColor(TimelineColors.gray)
    ctx.fill(r)

    ctx.setFillColor(TimelineColors.rulerTick) // STUDIO: light ticks on the dark ruler
    let majorTop = height * (1 - 0.4)
    for x in f.majorTicks {
        ctx.fill(CGRect(x: CGFloat(x).rounded(.down), y: majorTop.rounded(), width: 1, height: height - majorTop.rounded())) // STUDIO: hairline major ticks
    }
    let minorTop = height * (1 - 0.20)
    for x in f.minorTicks {
        ctx.fill(CGRect(x: CGFloat(x).rounded(.down), y: minorTop.rounded(), width: 1, height: height - minorTop.rounded()))
    }

    let font = rulerFont(height: f.height)
    for label in f.labels {
        let line = textLine(label.text, font: font, color: TimelineColors.white)
        let (w, ascent, descent) = measure(line)
        let textHeight = ascent + descent
        // `(height / 4 - textHeight / 2) + 9`, with height / 4 an integer division.
        let top = CGFloat(f.height / 4) - textHeight / 2 + 9
        draw(line, at: CGPoint(x: CGFloat(label.centre) - w / 2, y: top + ascent), in: ctx)
    }

    // The bottom line: 2 points at y = height - 1, so half of it falls off the edge.
    ctx.setFillColor(TimelineColors.black)
    let lineRight = min(r.maxX, CGFloat(f.totalPx))
    if lineRight > r.minX {
        ctx.fill(CGRect(x: r.minX, y: height - 1, width: lineRight - r.minX, height: 1))
    }

    // Range indicators, shifted up by base / 2 with base 16 and len 10.
    paintRange(f, height: height, ctx)

    for line in f.markLines {
        paintStudioRulerTick(line, ctx, height: height) // STUDIO: marks are coloured ticks, not full-height dashed lines
    }
    return []
}

nonisolated func paintRange(_ f: TimelineRulerFrame, height h: CGFloat, _ ctx: CGContext) {
    let base: CGFloat = 16
    let length: CGFloat = 10
    let shift = -base / 2
    ctx.setShouldAntialias(true)
    ctx.setFillColor(TimelineColors.darkGray)
    func arrow(_ x: CGFloat, _ direction: CGFloat) {
        ctx.beginPath()
        ctx.move(to: CGPoint(x: x, y: h - base / 2 + shift))
        ctx.addLine(to: CGPoint(x: x + direction * length, y: h - base + shift))
        ctx.addLine(to: CGPoint(x: x + direction * length, y: h + shift))
        ctx.closePath()
        ctx.fillPath()
        ctx.fill(CGRect(x: x, y: h - base + shift, width: 1, height: base))
    }
    if let start = f.rangeStart {
        arrow(CGFloat(start), 1)
    }
    if let end = f.rangeEnd {
        arrow(CGFloat(end), -1)
    }
    if let start = f.rangeStart, let end = f.rangeEnd {
        let x1 = CGFloat(start) + length
        let x2 = CGFloat(end) - length
        if x2 > x1 {
            ctx.fill(CGRect(x: x1, y: h - base / 2 + shift - 2, width: x2 - x1, height: 4))
        }
    }
    ctx.setShouldAntialias(false)
}

/// Font height `height / 3` in pixels, with digits of one width.
nonisolated func rulerFont(height: Int32) -> CTFont {
    systemFont(size: CGFloat(max(1, height / 3)), monospacedDigits: true)
}

// MARK: - Marks bar

/// F117, F119.
nonisolated func paintMarksTile(_ request: TileRequest, _ ctx: CGContext, controller: TimelineController) -> [UInt64]? {
    let r = request.rect
    let f = controller.marksFrame(x: Int32(r.minX), width: Int32(r.width), timePerPixel: request.timePerPixel)
    guard !f.failed else { return nil }
    ctx.setFillColor(TimelineColors.gray)
    ctx.fill(r)
    ctx.setFillColor(TimelineColors.black)
    for bottom in f.rowBottoms {
        ctx.fill(CGRect(x: r.minX, y: CGFloat(bottom) - 1, width: r.width, height: 1))
    }
    let font = systemFont(size: 12)
    for m in f.marks {
        let rect = CGRect(x: CGFloat(m.x), y: CGFloat(m.y), width: CGFloat(m.w), height: CGFloat(m.h))
        guard rect.intersects(r) else { continue }
        ctx.setFillColor(TimelineColors.argb(m.color))
        ctx.fill(rect)
        if let hatch = m.hatch {
            paintHatch(rect, color: TimelineColors.argb(hatch), ctx)
        }
        if m.selected {
            ctx.setFillColor(TimelineColors.black)
            let inner = rect.insetBy(dx: 1, dy: 1)
            ctx.fill(CGRect(x: rect.minX, y: rect.minY, width: rect.width, height: 2))
            ctx.fill(CGRect(x: rect.minX, y: rect.maxY - 1, width: rect.width, height: 2))
            ctx.fill(CGRect(x: rect.minX, y: rect.minY, width: 2, height: rect.height))
            ctx.fill(CGRect(x: rect.maxX - 1, y: rect.minY, width: 2, height: rect.height))
            paintDashedRect(inner, ctx)
        } else {
            ctx.setFillColor(TimelineColors.black)
            ctx.fill(CGRect(x: rect.minX, y: rect.minY, width: rect.width + 1, height: 1))
            ctx.fill(CGRect(x: rect.minX, y: rect.maxY, width: rect.width + 1, height: 1))
            ctx.fill(CGRect(x: rect.minX, y: rect.minY, width: 1, height: rect.height))
            ctx.fill(CGRect(x: rect.maxX, y: rect.minY, width: 1, height: rect.height))
        }
        if !m.text.isEmpty {
            paintWrapped(m.text, in: rect, font: font, color: TimelineColors.argb(m.textColor), ctx)
        }
    }
    return []
}

/// `HatchStyle.DiagonalCross`.
nonisolated func paintHatch(_ rect: CGRect, color: CGColor, _ ctx: CGContext) {
    ctx.saveGState()
    ctx.clip(to: rect)
    ctx.setFillColor(color)
    let step: CGFloat = 8
    var offset = (rect.minX / step).rounded(.down) * step - rect.height
    let end = rect.maxX + rect.height
    while offset < end {
        var d: CGFloat = 0
        while d < rect.height {
            let y = rect.minY + d
            ctx.fill(CGRect(x: offset + d, y: y, width: 1, height: 1))
            ctx.fill(CGRect(x: offset + step - d, y: y, width: 1, height: 1))
            d += 1
        }
        offset += step
    }
    ctx.restoreGState()
}

/// `[1, 2]` in `ForeColor` around a rectangle.
nonisolated func paintDashedRect(_ rect: CGRect, _ ctx: CGContext) {
    ctx.setFillColor(TimelineColors.foreground)
    var x = rect.minX
    while x < rect.maxX {
        ctx.fill(CGRect(x: x, y: rect.minY, width: 1, height: 1))
        ctx.fill(CGRect(x: x, y: rect.maxY - 1, width: 1, height: 1))
        x += 3
    }
    var y = rect.minY
    while y < rect.maxY {
        ctx.fill(CGRect(x: rect.minX, y: y, width: 1, height: 1))
        ctx.fill(CGRect(x: rect.maxX - 1, y: y, width: 1, height: 1))
        y += 3
    }
}

/// Wrapped in the rectangle and clipped.
nonisolated func paintWrapped(_ text: String, in rect: CGRect, font: CTFont, color: CGColor, _ ctx: CGContext) {
    let attributes: [CFString: Any] = [kCTFontAttributeName: font, kCTForegroundColorAttributeName: color]
    let attributed = CFAttributedStringCreate(nil, text as CFString, attributes as CFDictionary)!
    let setter = CTFramesetterCreateWithAttributedString(attributed)
    // A frame's coordinates go up, so lay it out as if the rectangle were at the origin and flip into place.
    let path = CGPath(rect: CGRect(origin: .zero, size: rect.size), transform: nil)
    let frame = CTFramesetterCreateFrame(setter, CFRange(location: 0, length: 0), path, nil)
    ctx.saveGState()
    ctx.clip(to: rect)
    ctx.setShouldAntialias(true)
    ctx.setShouldSmoothFonts(false)
    ctx.translateBy(x: rect.minX, y: rect.maxY)
    ctx.scaleBy(x: 1, y: -1)
    ctx.textMatrix = .identity
    CTFrameDraw(frame, ctx)
    ctx.restoreGState()
}

// MARK: - Waveform

/// F141, F142.
nonisolated func paintWaveformTile(_ request: TileRequest, _ ctx: CGContext, controller: TimelineController) -> [UInt64]? {
    let r = request.rect
    let f = controller.waveformFrame(firstColumn: Int32(r.minX), columns: Int32(r.width), timePerPixel: request.timePerPixel)
    guard !f.failed else { return nil }
    ctx.setFillColor(TimelineColors.waveformBackground)
    ctx.fill(r)
    defer { paintStudioWaveformTicks(request, ctx, controller: controller, height: CGFloat(f.height)) } // STUDIO: mark ticks over the wave
    guard !f.building, !f.highs.isEmpty else { return [] }
    let baseline = CGFloat(f.baseline)
    let top = CGFloat(f.top)
    // One column per point, in a gradient from (60,60,60) at the far end to (20,20,20) at the baseline.
    let gradient = waveformGradient
    ctx.saveGState()
    let path = CGMutablePath()
    for (i, high) in f.highs.enumerated() {
        let x = CGFloat(f.firstColumn) + CGFloat(i)
        let h = CGFloat(high)
        if h > 0 {
            path.addRect(CGRect(x: x, y: baseline - h, width: 1, height: h))
        }
    }
    ctx.addPath(path)
    ctx.clip()
    ctx.drawLinearGradient(gradient, start: CGPoint(x: 0, y: baseline - top), end: CGPoint(x: 0, y: baseline), options: [])
    ctx.restoreGState()
    if f.full {
        ctx.saveGState()
        let below = CGMutablePath()
        for (i, low) in f.lows.enumerated() {
            let x = CGFloat(f.firstColumn) + CGFloat(i)
            let h = CGFloat(low)
            if h > 0 {
                below.addRect(CGRect(x: x, y: baseline, width: 1, height: h))
            }
        }
        ctx.addPath(below)
        ctx.clip()
        ctx.drawLinearGradient(gradient, start: CGPoint(x: 0, y: baseline + top), end: CGPoint(x: 0, y: baseline), options: [])
        ctx.restoreGState()
    }
    return []
}

nonisolated(unsafe) let waveformGradient = CGGradient(
    colorsSpace: sRGBSpace,
    colors: [TimelineColors.waveformTop, TimelineColors.waveformMid] as CFArray,
    locations: [0, 1]
)!
