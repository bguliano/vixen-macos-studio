import AppKit
import CoreText
import QuartzCore
import VixenBridge
import VixenKit

// MARK: - Grid

/// F10's top layers, over the tiles. Selection borders and lifted effects are layers of their own, so selecting, rubber banding and dragging
/// repaint no tile.
@MainActor
final class GridCanvasView: TimelineSurface {
    let plane: TilePlane
    /// Narrow fixed viewport strips use the same painter, including marks and effects above the row outline.
    /// Keeping the outline out of scrolling tiles prevents an old edge appearing inside the view during async repaint.
    private let edgePlanes: [TilePlane]
    let planes: [TilePlane]
    private let pictures: PictureStore
    /// Scrolls with the tiles.
    private let overlayHost = plainLayer()
    private var occurrences: [OccurrenceKey: OccurrenceLayer] = [:]
    private var pool: [OccurrenceLayer] = []
    private let rubberBand = plainLayer()
    private let drawBox = plainLayer()
    private let indicators = CAShapeLayer()
    private let guideLines = CAShapeLayer()
    private let movingMarks = plainLayer()
    private let cursorLine = plainLayer()
    private let playbackLine = plainLayer()
    private let hoverBox = plainLayer()
    private let hoverText = CATextLayer()
    private var cover = CGRect.zero
    private var coverMargin: CGFloat = 512
    private var scroll = CGPoint.zero
    private var lastBase = CGPoint.zero
    private var lastState: TimelineView?
    /// Guides in canvas x, for the waveform.
    private(set) var lastGuides: [Double] = []
    private var gridGuides: [Double] = []
    private var indicatorX: [Double] = []
    private var hoverRect: TimelineRect?
    private var hoverSize = CGSize.zero
    private var cursorX: CGFloat = 0
    private var playbackX: CGFloat?

    override var wheelSurface: TimelineSurfaceKind { .grid }

    init(controller: TimelineController, pictures: PictureStore) {
        self.pictures = pictures
        let painter: TilePainter = { request, ctx in
            paintGridTile(request, ctx, controller: controller, pictures: pictures)
        }
        let main = TilePlane(tileWidth: 512, tileHeight: 256, timePerPixel: controller.view().timePerPixel, opaque: true, vertical: true, horizontal: true, painter: painter)
        plane = main
        let edges = (0..<2).map { _ in
            let edge = TilePlane(tileWidth: 1, tileHeight: 256, timePerPixel: controller.view().timePerPixel, opaque: true, vertical: true, horizontal: true, painter: painter)
            edge.paintsViewportEdges = true
            return edge
        }
        edgePlanes = edges
        planes = [main] + edges
        super.init(controller: controller)
        layer?.backgroundColor = TimelineColors.effectsBackground
        let root = layer!
        // A lifted effect goes no further than the grid, over the scroll bar or the row list.
        root.masksToBounds = true
        plane.root.frame = bounds
        root.addSublayer(plane.root)
        for edge in edgePlanes { root.addSublayer(edge.root) }
        overlayHost.anchorPoint = .zero
        root.addSublayer(overlayHost)
        for shape in [indicators, guideLines] {
            shape.actions = noActions
            shape.anchorPoint = .zero
            shape.fillColor = nil
            root.addSublayer(shape)
        }
        guideLines.strokeColor = TimelineColors.yellow
        guideLines.lineWidth = 1
        guideLines.lineDashPattern = [2, 2]
        indicators.lineWidth = 1
        root.addSublayer(movingMarks)
        rubberBand.isHidden = true
        rubberBand.borderWidth = 1
        rubberBand.backgroundColor = TimelineColors.selectionFill
        overlayHost.addSublayer(rubberBand)
        drawBox.isHidden = true
        drawBox.borderWidth = 2
        drawBox.borderColor = TimelineColors.black
        drawBox.backgroundColor = TimelineColors.drawFill
        overlayHost.addSublayer(drawBox)
        cursorLine.backgroundColor = TimelineColors.cursor
        root.addSublayer(cursorLine)
        playbackLine.backgroundColor = TimelineColors.green
        playbackLine.isHidden = true
        root.addSublayer(playbackLine)
        hoverBox.backgroundColor = TimelineColors.infoBackground
        hoverBox.isHidden = true
        hoverText.actions = noActions
        hoverText.anchorPoint = .zero
        hoverText.font = NSFont.systemFont(ofSize: gdiPoints(7))
        hoverText.fontSize = gdiPoints(7)
        hoverText.foregroundColor = TimelineColors.white
        hoverBox.addSublayer(hoverText)
        root.addSublayer(hoverBox)
        setAccessibilityIdentifier("timeline-grid")
        setAccessibilityRole(.group)
        setAccessibilityElement(true)
        setAccessibilityLabel("Timeline")
    }

    override func layout() {
        super.layout()
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        plane.root.position = .zero
        plane.root.bounds = bounds
        placeEdgePlanes()
        CATransaction.commit()
    }

    func setView(scrollX: CGFloat, scrollY: CGFloat, state: TimelineView) {
        scroll = CGPoint(x: scrollX, y: scrollY)
        lastState = state
        plane.setView(scrollX: scrollX, scrollY: scrollY, size: bounds.size, timePerPixel: state.timePerPixel)
        placeEdgePlanes()
        for (index, edge) in edgePlanes.enumerated() {
            let x = index == 0 ? 0 : max(0, bounds.width - 1)
            edge.setView(scrollX: scrollX + x, scrollY: scrollY, size: CGSize(width: min(1, bounds.width), height: bounds.height), timePerPixel: state.timePerPixel)
        }
        overlayHost.position = plane.hostPosition(viewportHeight: bounds.height)
        cursorX = CGFloat(state.cursorX)
        placeColumn(cursorLine, x: cursorX, width: 2.5)
        if let x = playbackX {
            placeColumn(playbackLine, x: x, width: 1)
        }
        if !gridGuides.isEmpty || !indicatorX.isEmpty {
            redrawColumnPaths()
        }
        if plane.currentBase != lastBase {
            lastBase = plane.currentBase
            cover = .zero
        }
    }

    private func placeEdgePlanes() {
        for (index, edge) in edgePlanes.enumerated() {
            edge.root.frame = CGRect(x: index == 0 ? 0 : max(0, bounds.width - 1), y: 0, width: min(1, bounds.width), height: bounds.height)
        }
    }

    private func placeColumn(_ layer: CALayer, x: CGFloat, width: CGFloat) {
        layer.frame = CGRect(x: x - scroll.x - width / 2, y: 0, width: width, height: bounds.height)
    }

    /// Whether the view has moved past what the overlay was built for.
    func overlayCoverIsStale() -> Bool {
        let view = CGRect(x: scroll.x, y: scroll.y, width: bounds.width, height: bounds.height)
        return !cover.contains(view)
    }

    @discardableResult
    func refreshOverlay() -> TimelineOverlay {
        let c = CGRect(x: scroll.x - coverMargin, y: scroll.y - coverMargin, width: bounds.width + coverMargin * 2, height: bounds.height + coverMargin * 2).integral
        cover = c
        let o = controller.overlay(x: Int32(c.minX), y: Int32(c.minY), w: Int32(c.width), h: Int32(c.height))
        applyOccurrences(o.occurrences)
        place(rubberBand, o.rubberBand)
        rubberBand.borderColor = o.selectionGreen ? TimelineColors.selectionGreen : TimelineColors.selectionBlue
        place(drawBox, o.drawBox)
        lastGuides = o.guides
        gridGuides = o.gridGuides
        indicatorX = o.indicatorLines
        indicators.strokeColor = TimelineColors.argb(o.indicatorColor)
        redrawColumnPaths()
        applyMovingMarks(o.movingMarkLines)
        applyHover(o.hover)
        cursorX = CGFloat(o.cursorX)
        placeColumn(cursorLine, x: cursorX, width: 2.5)
        return o
    }

    private func place(_ layer: CALayer, _ r: TimelineRect?) {
        guard let r else {
            layer.isHidden = true
            return
        }
        layer.isHidden = false
        layer.frame = plane.localFrame(canvas: CGRect(x: CGFloat(r.x), y: CGFloat(r.y), width: CGFloat(r.w), height: CGFloat(r.h)))
    }

    private func redrawColumnPaths() {
        let height = bounds.height
        func path(_ xs: [Double]) -> CGPath? {
            guard !xs.isEmpty else { return nil }
            let p = CGMutablePath()
            for x in xs {
                let v = (CGFloat(x) - scroll.x).rounded(.down) + 0.5
                p.move(to: CGPoint(x: v, y: 0))
                p.addLine(to: CGPoint(x: v, y: height))
            }
            return p
        }
        guideLines.frame = bounds
        indicators.frame = bounds
        guideLines.path = path(gridGuides)
        indicators.path = path(indicatorX)
    }

    private func applyMovingMarks(_ lines: [TimelineLine]) {
        guard !lines.isEmpty || movingMarks.sublayers?.isEmpty == false else { return }
        movingMarks.sublayers = lines.map { line in
            let shape = CAShapeLayer()
            shape.actions = noActions
            shape.anchorPoint = .zero
            shape.frame = bounds
            let p = CGMutablePath()
            let x = (CGFloat(line.x) - scroll.x).rounded(.down) + 0.5
            p.move(to: CGPoint(x: x, y: 0))
            p.addLine(to: CGPoint(x: x, y: bounds.height))
            shape.path = p
            shape.strokeColor = TimelineColors.argb(line.color)
            shape.lineWidth = CGFloat(line.width)
            if line.dash > 0 {
                shape.lineDashPattern = [NSNumber(value: line.dash), NSNumber(value: line.dash)]
            }
            return shape
        }
    }

    private func applyHover(_ hover: TimelineHover?) {
        guard let hover else {
            hoverBox.isHidden = true
            hoverRect = nil
            return
        }
        let text = hover.text.replacingOccurrences(of: "\r\n", with: "\n")
        let attributes: [NSAttributedString.Key: Any] = [.font: NSFont.systemFont(ofSize: gdiPoints(7))]
        let size = (text as NSString).size(withAttributes: attributes)
        let origin = controller.hoverBoxOrigin(rect: hover.rect, textHeight: Int32(size.height.rounded(.up)), clipX: Int32(scroll.x), clipY: Int32(scroll.y))
        let width = size.width.rounded(.up) + 2
        let height = size.height.rounded(.up) + 2
        hoverBox.isHidden = false
        // The dark fill is the box offset by (1, 1) and 1 larger each way, and the text is in the box itself.
        hoverBox.frame = CGRect(x: CGFloat(origin[0]) - scroll.x + 1, y: bounds.height - (CGFloat(origin[1]) - scroll.y) - height - 2, width: width + 1, height: height + 1)
        hoverText.frame = CGRect(x: -1, y: 1, width: width, height: height)
        hoverText.contentsScale = plane.scale
        hoverText.string = text
    }

    // MARK: Selected and lifted effects

    private func applyOccurrences(_ quads: [TimelineElementQuad]) {
        var seen = Set<OccurrenceKey>(minimumCapacity: quads.count)
        for q in quads {
            let key = OccurrenceKey(id: q.id, row: q.row)
            seen.insert(key)
            let layer: OccurrenceLayer
            if let existing = occurrences[key] {
                layer = existing
            } else {
                layer = pool.popLast() ?? OccurrenceLayer()
                layer.layer.contentsScale = plane.scale
                occurrences[key] = layer
                overlayHost.insertSublayer(layer.layer, at: 0)
            }
            let full = CGRect(x: CGFloat(q.x), y: CGFloat(q.y), width: CGFloat(q.w), height: CGFloat(q.h))
            layer.show(q, full: full, cover: cover, in: plane, image: q.moving ? pictures.bestImage(id: q.id) : nil)
        }
        if occurrences.count > seen.count {
            for (key, layer) in occurrences where !seen.contains(key) {
                layer.layer.removeFromSuperlayer()
                occurrences[key] = nil
                if pool.count < 256 {
                    layer.clear()
                    pool.append(layer)
                }
            }
        }
    }

    /// A lifted effect's picture turned up while it was in the air.
    func picturesArrived(_ ids: Set<UInt64>) {
        for (key, layer) in occurrences where layer.isLifted && ids.contains(key.id) {
            layer.setImage(pictures.bestImage(id: key.id))
        }
    }

    func setPlayback(x: CGFloat, state: TimelineView) {
        playbackX = x
        playbackLine.isHidden = false
        placeColumn(playbackLine, x: x, width: 1)
    }

    func clearPlayback() {
        playbackX = nil
        playbackLine.isHidden = true
    }

    /// For the test scene's dump.
    var playbackLineX: CGFloat? { playbackLine.isHidden ? nil : playbackX }

    // MARK: Drops

    private func dropTarget(_ info: NSDraggingInfo) -> (Int64, UInt32?, UInt64?) {
        let p = convert(info.draggingLocation, from: nil)
        let x = Int32(p.x.rounded(.down))
        let y = Int32((bounds.height - p.y).rounded(.down))
        return (controller.timeAtClient(x: x), controller.rowAtClient(y: y), controller.elementAtClient(x: x, y: y))
    }

    override func draggingEntered(_ sender: NSDraggingInfo) -> NSDragOperation {
        draggingUpdated(sender)
    }

    override func draggingUpdated(_ sender: NSDraggingInfo) -> NSDragOperation {
        let (time, row, element) = dropTarget(sender)
        return canvas?.delegate?.timelineValidateDrop(sender, time: time, row: row, element: element) ?? []
    }

    override func performDragOperation(_ sender: NSDraggingInfo) -> Bool {
        let (time, row, element) = dropTarget(sender)
        return canvas?.delegate?.timelinePerformDrop(sender, time: time, row: row, element: element) ?? false
    }

    // MARK: Input

    override func press(_ pointer: TimelinePointer) {
        guard let canvas else { return }
        canvas.noteAutoscrollOwner(.grid)
        let update = controller.gridMouseDown(ev: pointer)
        setCursorKind(update.cursorKind)
        canvas.apply(update)
        if pointer.clicks == 2 {
            canvas.apply(controller.gridDoubleClick(ev: pointer))
        }
    }

    override func move(_ pointer: TimelinePointer) {
        guard let canvas else { return }
        let update = controller.gridMouseMove(ev: pointer)
        setCursorKind(update.cursorKind)
        canvas.apply(update)
    }

    override func release(_ pointer: TimelinePointer) {
        guard let canvas else { return }
        let update = controller.gridMouseUp(ev: pointer)
        setCursorKind(update.cursorKind)
        canvas.apply(update)
    }

    override func keyDown(with event: NSEvent) {
        guard !timelineKeyDown(event) else { return }
        super.keyDown(with: event)
    }

    /// F149, for what the window around it leaves. The sequencer's own table sees keys first.
    private func timelineKeyDown(_ event: NSEvent) -> Bool {
        guard let canvas else { return false }
        let mods = timelineMods(event.modifierFlags)
        let plain = !mods.shift && !mods.cmd && !mods.opt
        let characters = event.charactersIgnoringModifiers ?? ""
        let pointerClient = pointerInClient()
        func alignUnderPointer(_ kind: TimelineAlign, hold: Bool) -> Bool {
            guard !controller.isDragInProgress() else { return true }
            let selected = controller.selectedElements()
            guard selected.count > 1, let under = controller.elementAtClient(x: pointerClient.0, y: pointerClient.1), selected.contains(under) else { return true }
            canvas.apply(controller.align(kind: kind, reference: under, hold: hold))
            return true
        }
        switch event.keyCode {
        case 53: // Esc
            canvas.apply(controller.gridCancelDrag())
            return true
        case 126 where plain:
            if controller.moveActiveRow(direction: -1) { canvas.apply(controller.refresh()); rowsChanged(canvas) }
            return true
        case 125 where plain:
            if controller.moveActiveRow(direction: 1) { canvas.apply(controller.refresh()); rowsChanged(canvas) }
            return true
        case 123 where mods.cmd:
            canvas.apply(controller.nudgeSelected(direction: -1))
            return true
        case 124 where mods.cmd:
            canvas.apply(controller.nudgeSelected(direction: 1))
            return true
        case 115: // Home
            canvas.apply(mods.cmd ? controller.scrollToStart() : controller.scrollToTop())
            return true
        case 119: // End
            canvas.apply(mods.cmd ? controller.scrollToEnd() : controller.scrollToBottom())
            return true
        case 116: // PageUp
            canvas.apply(mods.cmd ? controller.pageTime(direction: -1) : controller.pageVertical(direction: -1))
            return true
        case 121: // PageDown
            canvas.apply(mods.cmd ? controller.pageTime(direction: 1) : controller.pageVertical(direction: 1))
            return true
        case 51, 117: // ⌫, ⌦. Marks only go from the bar, with the bar focused (F134, F154).
            if !controller.selectedElements().isEmpty {
                canvas.apply(controller.deleteSelectedEffects())
            }
            return true
        default:
            break
        }
        let exactShift = mods.shift && !mods.cmd && !mods.opt
        switch characters.lowercased() {
        case "a" where mods.cmd:
            canvas.apply(controller.selectAll())
        case "s" where !mods.cmd:
            return alignUnderPointer(.start, hold: mods.shift)
        case "e" where !mods.cmd:
            return alignUnderPointer(.end, hold: mods.shift)
        case "b" where !mods.cmd:
            return alignUnderPointer(.both, hold: false)
        case "c" where !mods.cmd:
            return alignUnderPointer(.centers, hold: false)
        case "u" where !mods.cmd:
            return alignUnderPointer(.durations, hold: exactShift)
        case "r" where !mods.cmd:
            return alignUnderPointer(.startToEnd, hold: exactShift)
        case "n" where !mods.cmd:
            return alignUnderPointer(.endToStart, hold: exactShift)
        case "t":
            canvas.apply(controller.toggleSelectedRows(includeChildren: mods.cmd))
        case "d" where mods.cmd:
            canvas.apply(controller.splitSelectedAt(time: controller.timeAtClient(x: pointerClient.0)))
        case "=" where mods.cmd, "+" where mods.cmd:
            canvas.apply(mods.shift ? controller.zoomRows(scale: 1.25) : controller.zoomTime(scale: 0.8))
        case "-" where mods.cmd, "_" where mods.cmd:
            canvas.apply(mods.shift ? controller.zoomRows(scale: 0.8) : controller.zoomTime(scale: 1.25))
        default:
            return false
        }
        return true
    }

    private func rowsChanged(_ canvas: TimelineCanvas) {
        canvas.rows.plane.invalidateAll()
    }

    /// The pointer now, in client coordinates: hotkeys read it at key time.
    private func pointerInClient() -> (Int32, Int32) {
        let p = convert(window?.mouseLocationOutsideOfEventStream ?? .zero, from: nil)
        return (Int32(p.x.rounded(.down)), Int32((bounds.height - p.y).rounded(.down)))
    }
}

struct OccurrenceKey: Hashable {
    let id: UInt64
    let row: UInt32
}

/// A selected or lifted effect in one row. The dashes are its own shape, so a border that hasn't changed size never rasterizes again as it moves.
@MainActor
final class OccurrenceLayer {
    let layer = plainLayer()
    private let dash = CAShapeLayer()
    private var dashSize = CGSize.zero
    private(set) var isLifted = false

    init() {
        layer.masksToBounds = true
        layer.contentsGravity = .resize
        dash.actions = noActions
        dash.anchorPoint = .zero
        dash.fillColor = nil
        dash.strokeColor = TimelineColors.foreground
        dash.lineWidth = 1
        dash.lineDashPattern = [1, 2]
        layer.addSublayer(dash)
    }

    func clear() {
        layer.contents = nil
        isLifted = false
        layer.backgroundColor = nil
        dashSize = .zero
        dash.path = nil
    }

    func setImage(_ image: CGImage?) {
        layer.contents = image
    }

    func show(_ q: TimelineElementQuad, full: CGRect, cover: CGRect, in plane: TilePlane, image: CGImage?) {
        // A very wide effect is cut down to what could show. Its picture follows.
        let shown = full.intersection(cover)
        guard !shown.isNull, shown.width > 0 else {
            layer.isHidden = true
            return
        }
        layer.isHidden = false
        layer.frame = plane.localFrame(canvas: shown)
        isLifted = q.moving
        if q.moving {
            if q.rendered {
                layer.contents = image
                layer.contentsRect = CGRect(x: (shown.minX - full.minX) / full.width, y: 0, width: shown.width / full.width, height: 1)
                layer.backgroundColor = image == nil ? TimelineColors.effectsBackground : nil
            } else {
                layer.contents = nil
                layer.backgroundColor = TimelineColors.placeholder
            }
        } else {
            layer.contents = nil
            layer.backgroundColor = nil
        }
        layer.borderColor = q.redBorder ? TimelineColors.red : TimelineColors.black
        // `AddSelectionOverlayToCanvas`: selected is a 3 point line half clipped by the bounds, with [1, 2] in ForeColor along it.
        layer.borderWidth = q.selected ? 2 : 1
        if q.selected {
            if shown.size != dashSize {
                dashSize = shown.size
                dash.frame = CGRect(origin: .zero, size: shown.size)
                dash.path = CGPath(rect: CGRect(origin: .zero, size: shown.size).insetBy(dx: 0.5, dy: 0.5), transform: nil)
            }
            dash.isHidden = false
        } else {
            dash.isHidden = true
        }
    }
}

// MARK: - Scrollers

/// Always shown, as upstream's are: you drag these, so an overlay that fades out wouldn't do. Drawn here, because the system's own legacy style only draws through
/// private layers.
nonisolated final class TimelineScroller: NSScroller {
    override class var isCompatibleWithOverlayScrollers: Bool { false }

    override func drawKnobSlot(in slotRect: NSRect, highlight flag: Bool) {
        NSColor(srgbRed: 40 / 255, green: 40 / 255, blue: 40 / 255, alpha: 1).setFill()
        slotRect.fill()
    }

    override func drawKnob() {
        let knob = rect(for: .knob)
        guard !knob.isEmpty else { return }
        NSColor(srgbRed: 136 / 255, green: 136 / 255, blue: 136 / 255, alpha: 1).setFill()
        NSBezierPath(roundedRect: knob.insetBy(dx: 2, dy: 2), xRadius: 4, yRadius: 4).fill()
    }
}

/// The grid and its two scroll bars. No `NSScrollView`: at the zoom floor its document view would be far bigger than views and layers can handle.
@MainActor
final class GridViewportView: NSView {
    let grid: GridCanvasView
    weak var canvas: TimelineCanvas?
    private let horizontal = TimelineScroller()
    private let vertical = TimelineScroller()
    private var state: TimelineView?

    init(canvas grid: GridCanvasView) {
        self.grid = grid
        super.init(frame: .zero)
        addSubview(grid)
        for scroller in [horizontal, vertical] {
            scroller.scrollerStyle = .legacy
            scroller.controlSize = .regular
            scroller.target = self
            addSubview(scroller)
        }
        horizontal.action = #selector(horizontalScrolled)
        vertical.action = #selector(verticalScrolled)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("not from a nib")
    }

    override var isFlipped: Bool { true }

    override func layout() {
        super.layout()
        let bar = NSScroller.scrollerWidth(for: .regular, scrollerStyle: .legacy)
        grid.frame = CGRect(x: 0, y: 0, width: max(0, bounds.width - bar), height: max(0, bounds.height - bar))
        horizontal.frame = CGRect(x: 0, y: bounds.height - bar, width: max(0, bounds.width - bar), height: bar)
        vertical.frame = CGRect(x: bounds.width - bar, y: 0, width: bar, height: max(0, bounds.height - bar))
        grid.layoutSubtreeIfNeeded()
        canvas?.viewportChanged()
    }

    func updateScrollers(state v: TimelineView, gridSize: CGSize) {
        state = v
        let width = Double(max(v.canvasWidth, 1))
        let viewWidth = Double(gridSize.width)
        horizontal.isEnabled = width > viewWidth
        horizontal.knobProportion = CGFloat(min(1, viewWidth / width))
        horizontal.doubleValue = width > viewWidth ? v.scrollX / (width - viewWidth) : 0
        let height = Double(max(v.canvasHeight, 1))
        let viewHeight = Double(gridSize.height)
        vertical.isEnabled = height > viewHeight
        vertical.knobProportion = CGFloat(min(1, viewHeight / height))
        vertical.doubleValue = height > viewHeight ? Double(v.verticalOffset) / (height - viewHeight) : 0
    }

    @objc private func horizontalScrolled() {
        guard let canvas, let v = state else { return }
        let viewWidth = Double(grid.bounds.width)
        switch horizontal.hitPart {
        case .decrementPage:
            canvas.apply(canvas.controller.setScrollX(x: Int32(max(0, v.scrollX - viewWidth))))
        case .incrementPage:
            canvas.apply(canvas.controller.setScrollX(x: Int32(v.scrollX + viewWidth)))
        default:
            // F7: `VisibleTimeStart = time(scrollX)`, so the bar snaps time to whole pixels.
            canvas.apply(canvas.controller.setScrollX(x: Int32(horizontal.doubleValue * max(0, Double(v.canvasWidth) - viewWidth))))
        }
    }

    @objc private func verticalScrolled() {
        guard let canvas, let v = state else { return }
        let viewHeight = Double(grid.bounds.height)
        switch vertical.hitPart {
        case .decrementPage:
            canvas.apply(canvas.controller.setVerticalOffset(y: Int32(Double(v.verticalOffset) - viewHeight)))
        case .incrementPage:
            canvas.apply(canvas.controller.setVerticalOffset(y: Int32(Double(v.verticalOffset) + viewHeight)))
        default:
            canvas.apply(canvas.controller.setVerticalOffset(y: Int32(vertical.doubleValue * max(0, Double(v.canvasHeight) - viewHeight))))
        }
    }
}

// MARK: - Ruler

/// F97 to F116.
@MainActor
final class RulerView: TimelineSurface {
    let plane: TilePlane
    private let cursorLine = plainLayer()
    private let arrow = CAShapeLayer()
    private let dragLine = CAShapeLayer()
    private let dragText = CATextLayer()
    private var scrollX: CGFloat = 0
    private var cursorX: CGFloat = 0
    private var state: TimelineView?
    private var dragX: Double?

    override var wheelSurface: TimelineSurfaceKind { .ruler }

    /// F154: upstream's ruler has no focus call and doesn't call the base, so a press there leaves focus alone. That decides what Esc and Delete do next.
    override var acceptsFirstResponder: Bool { false }

    override init(controller: TimelineController) {
        plane = TilePlane(tileWidth: 512, tileHeight: 50, timePerPixel: controller.view().timePerPixel, opaque: true, vertical: false, horizontal: true) { request, ctx in
            paintRulerTile(request, ctx, controller: controller)
        }
        super.init(controller: controller)
        let root = layer!
        root.backgroundColor = TimelineColors.gray
        root.addSublayer(plane.root)
        cursorLine.backgroundColor = TimelineColors.blue
        root.addSublayer(cursorLine)
        arrow.actions = noActions
        arrow.anchorPoint = .zero
        arrow.fillColor = TimelineColors.green
        arrow.isHidden = true
        root.addSublayer(arrow)
        dragLine.actions = noActions
        dragLine.anchorPoint = .zero
        dragLine.strokeColor = TimelineColors.yellow
        dragLine.lineWidth = 1
        dragLine.lineDashPattern = [2, 2]
        dragLine.fillColor = nil
        dragLine.isHidden = true
        root.addSublayer(dragLine)
        dragText.actions = noActions
        dragText.anchorPoint = .zero
        dragText.font = NSFont.boldSystemFont(ofSize: gdiPoints(8))
        dragText.fontSize = gdiPoints(8)
        dragText.foregroundColor = TimelineColors.white
        dragText.isHidden = true
        root.addSublayer(dragText)
        setAccessibilityIdentifier("timeline-ruler")
        setAccessibilityRole(.group)
        setAccessibilityElement(true)
        setAccessibilityLabel("Ruler")
    }

    override func layout() {
        super.layout()
        plane.root.position = .zero
        plane.root.bounds = bounds
    }

    func setView(scrollX: CGFloat, state v: TimelineView) {
        self.scrollX = scrollX
        state = v
        plane.setTileSize(width: 512, height: bounds.height)
        plane.setView(scrollX: scrollX, scrollY: 0, size: bounds.size, timePerPixel: v.timePerPixel)
        cursorX = CGFloat(v.cursorX)
        cursorLine.frame = CGRect(x: (cursorX - scrollX).rounded(.down), y: 0, width: 1, height: bounds.height)
        if let dragX {
            placeDrag(CGFloat(dragX))
        }
    }

    func overlayChanged(_ o: TimelineOverlay) {
        dragX = o.rulerDragX
        if let x = o.rulerDragX, let text = o.rulerDragText {
            dragLine.isHidden = false
            dragText.isHidden = false
            dragText.string = text
            dragText.contentsScale = plane.scale
            placeDrag(CGFloat(x))
        } else {
            dragLine.isHidden = true
            dragText.isHidden = true
        }
    }

    private func placeDrag(_ x: CGFloat) {
        let v = (x - scrollX).rounded(.down) + 0.5
        dragLine.frame = bounds
        let p = CGMutablePath()
        p.move(to: CGPoint(x: v, y: 0))
        p.addLine(to: CGPoint(x: v, y: bounds.height))
        dragLine.path = p
        dragText.frame = CGRect(x: v, y: bounds.height - 16, width: 90, height: 16)
    }

    /// The triangle at the top: `(x, 10)`, `(x - 8, 0)`, `(x + 8, 0)`. F94.
    func setPlayback(x: CGFloat, state v: TimelineView) {
        arrow.isHidden = false
        arrow.frame = bounds
        let px = (x - scrollX).rounded(.down)
        let top = bounds.height
        let p = CGMutablePath()
        p.move(to: CGPoint(x: px, y: top - 10))
        p.addLine(to: CGPoint(x: px - 8, y: top))
        p.addLine(to: CGPoint(x: px + 8, y: top))
        p.closeSubpath()
        arrow.path = p
    }

    func clearPlayback() {
        arrow.isHidden = true
    }

    override func press(_ pointer: TimelinePointer) {
        guard let canvas else { return }
        canvas.apply(controller.rulerMouseDown(ev: pointer))
    }

    override func move(_ pointer: TimelinePointer) {
        guard let canvas else { return }
        let update = controller.rulerMouseMove(ev: pointer)
        setCursorKind(update.cursorKind)
        canvas.apply(update)
    }

    override func release(_ pointer: TimelinePointer) {
        guard let canvas else { return }
        canvas.apply(controller.rulerMouseUp(ev: pointer))
        if pointer.clicks == 2 {
            canvas.apply(controller.rulerDoubleClick())
        }
    }

    override func mouseExited(with event: NSEvent) {
        guard let canvas else { return }
        setCursorKind(.arrow)
        canvas.apply(controller.rulerMouseExited())
    }
}

// MARK: - Marks bar

/// F117 to F139.
@MainActor
final class MarksBarView: TimelineSurface {
    let plane: TilePlane

    override var wheelSurface: TimelineSurfaceKind { .marksBar }

    override init(controller: TimelineController) {
        plane = TilePlane(tileWidth: 512, tileHeight: 20, timePerPixel: controller.view().timePerPixel, opaque: true, vertical: false, horizontal: true) { request, ctx in
            paintMarksTile(request, ctx, controller: controller)
        }
        super.init(controller: controller)
        layer?.backgroundColor = TimelineColors.gray
        layer?.addSublayer(plane.root)
        setAccessibilityIdentifier("timeline-marks")
        setAccessibilityRole(.group)
        setAccessibilityElement(true)
        setAccessibilityLabel("Marks")
    }

    override func layout() {
        super.layout()
        plane.root.position = .zero
        plane.root.bounds = bounds
    }

    func setView(scrollX: CGFloat, state v: TimelineView) {
        plane.setTileSize(width: 512, height: max(bounds.height, 1))
        plane.setView(scrollX: scrollX, scrollY: 0, size: bounds.size, timePerPixel: v.timePerPixel)
    }

    override func press(_ pointer: TimelinePointer) {
        guard let canvas else { return }
        canvas.noteAutoscrollOwner(.marks)
        canvas.apply(controller.marksMouseDown(ev: pointer))
    }

    override func move(_ pointer: TimelinePointer) {
        guard let canvas else { return }
        let update = controller.marksMouseMove(ev: pointer)
        setCursorKind(update.cursorKind)
        canvas.apply(update)
    }

    override func release(_ pointer: TimelinePointer) {
        guard let canvas else { return }
        canvas.apply(controller.marksMouseUp(ev: pointer))
    }

    override func keyDown(with event: NSEvent) {
        // F134: the editor's Delete shortcut for effects runs first when any effect is selected.
        if event.keyCode == 51 || event.keyCode == 117, let canvas {
            canvas.apply(controller.selectedElements().isEmpty ? controller.deleteMarksKey() : controller.deleteSelectedEffects())
            return
        }
        super.keyDown(with: event)
    }
}

// MARK: - Waveform

/// F140 to F148.
@MainActor
final class WaveformView: TimelineSurface {
    let plane: TilePlane
    private let cursorLine = plainLayer()
    private let guides = CAShapeLayer()
    private let building = CATextLayer()
    private var scrollX: CGFloat = 0
    private var guideX: [Double] = []

    override var wheelSurface: TimelineSurfaceKind { .waveform }

    override init(controller: TimelineController) {
        plane = TilePlane(tileWidth: 512, tileHeight: 50, timePerPixel: controller.view().timePerPixel, opaque: true, vertical: false, horizontal: true) { request, ctx in
            paintWaveformTile(request, ctx, controller: controller)
        }
        super.init(controller: controller)
        let root = layer!
        root.backgroundColor = TimelineColors.waveformBackground
        root.addSublayer(plane.root)
        guides.actions = noActions
        guides.anchorPoint = .zero
        guides.strokeColor = TimelineColors.yellow
        guides.lineWidth = 1
        guides.lineDashPattern = [2, 2]
        guides.fillColor = nil
        root.addSublayer(guides)
        cursorLine.backgroundColor = TimelineColors.blue
        root.addSublayer(cursorLine)
        building.actions = noActions
        building.anchorPoint = .zero
        building.string = "Building waveform....."
        building.font = NSFont.systemFont(ofSize: gdiPoints(10))
        building.fontSize = gdiPoints(10)
        building.foregroundColor = TimelineColors.black
        building.isHidden = true
        root.addSublayer(building)
        setAccessibilityIdentifier("timeline-waveform")
        setAccessibilityRole(.group)
        setAccessibilityElement(true)
        setAccessibilityLabel("Waveform")
    }

    override func layout() {
        super.layout()
        plane.root.position = .zero
        plane.root.bounds = bounds
        // F142, FIX Q32: at the viewport's left plus 15.
        building.frame = CGRect(x: 15, y: (bounds.height - 20) / 2, width: 300, height: 20)
    }

    func setView(scrollX: CGFloat, state v: TimelineView) {
        self.scrollX = scrollX
        plane.setTileSize(width: 512, height: max(bounds.height, 1))
        plane.setView(scrollX: scrollX, scrollY: 0, size: bounds.size, timePerPixel: v.timePerPixel)
        cursorLine.frame = CGRect(x: (CGFloat(v.cursorX) - scrollX).rounded(.down), y: 0, width: 1, height: bounds.height)
        if !guideX.isEmpty {
            redrawGuides()
        }
        building.isHidden = !v.waveformBuilding
        building.contentsScale = plane.scale
        cursorLine.isHidden = v.waveformBlank
        guides.isHidden = v.waveformBlank
    }

    func overlayChanged(guides x: [Double], state v: TimelineView) {
        guard x != guideX else { return }
        guideX = x
        redrawGuides()
    }

    private func redrawGuides() {
        guides.frame = bounds
        guard !guideX.isEmpty else {
            guides.path = nil
            return
        }
        let p = CGMutablePath()
        for x in guideX {
            let v = (CGFloat(x) - scrollX).rounded(.down) + 0.5
            p.move(to: CGPoint(x: v, y: 0))
            p.addLine(to: CGPoint(x: v, y: bounds.height))
        }
        guides.path = p
    }

    override func press(_ pointer: TimelinePointer) {
        guard let canvas else { return }
        if pointer.clicks == 2 {
            canvas.apply(controller.waveformDoubleClick())
        }
    }

    override func move(_ pointer: TimelinePointer) {
        guard let canvas else { return }
        let kind = controller.waveformMouseMove(y: pointer.y, leftDown: pointer.button == .left)
        setCursorKind(kind)
        canvas.apply(controller.refresh())
    }

    override func mouseExited(with event: NSEvent) {
        setCursorKind(.arrow)
    }
}

// MARK: - Row list

/// One view, drawing visible rows only. Not the shared outline view: its rows have to stay locked to the grid's scroll to the point, take any
/// height by dragging, and come from the same layout as the grid.
@MainActor
final class RowHeaderView: TimelineSurface {
    let plane: TilePlane

    override var wheelSurface: TimelineSurfaceKind { .rowList }

    override init(controller: TimelineController) {
        plane = TilePlane(tileWidth: 200, tileHeight: 256, timePerPixel: 1, opaque: true, vertical: true, horizontal: false) { request, ctx in
            paintRowsTile(request, ctx, controller: controller)
        }
        super.init(controller: controller)
        layer?.backgroundColor = TimelineColors.panelBackground
        layer?.addSublayer(plane.root)
        setAccessibilityIdentifier("timeline-rows")
        setAccessibilityRole(.outline)
        setAccessibilityElement(true)
        setAccessibilityLabel("Rows")
    }

    override func layout() {
        super.layout()
        plane.root.position = .zero
        plane.root.bounds = bounds
        plane.setTileSize(width: max(bounds.width, 1), height: 256)
    }

    func setView(scrollY: CGFloat) {
        plane.setView(scrollX: 0, scrollY: scrollY, size: bounds.size, timePerPixel: 1)
    }

    override func press(_ pointer: TimelinePointer) {
        guard let canvas else { return }
        canvas.apply(controller.rowMouseDown(ev: pointer, width: Int32(bounds.width)))
    }

    override func move(_ pointer: TimelinePointer) {
        guard let canvas else { return }
        let update = controller.rowMouseMove(ev: pointer, width: Int32(bounds.width))
        setCursorKind(update.cursorKind)
        canvas.apply(update)
    }

    override func release(_ pointer: TimelinePointer) {
        guard let canvas else { return }
        canvas.apply(controller.rowMouseUp(ev: pointer, width: Int32(bounds.width)))
    }

    /// One element per visible row, with a disclosure action.
    override func accessibilityChildren() -> [Any]? {
        guard let canvas else { return [] }
        let v = canvas.state
        let frame = controller.rowFrame(top: v.verticalOffset, bottom: v.verticalOffset + Int32(bounds.height))
        return frame.rows.map { row in
            let top = CGFloat(row.top - v.verticalOffset)
            return RowAccessibilityElement(
                row: row, host: self, frame: NSRect(x: 0, y: bounds.height - top - CGFloat(row.height), width: bounds.width, height: CGFloat(row.height)),
                client: NSPoint(x: CGFloat(row.depth) * 24 + 5, y: top + 5))
        }
    }
}

nonisolated final class RowAccessibilityElement: NSAccessibilityElement {
    private let row: TimelineRowDisplay
    private let host: RowHeaderView
    private let client: NSPoint

    init(row: TimelineRowDisplay, host: RowHeaderView, frame: NSRect, client: NSPoint) {
        self.row = row
        self.host = host
        self.client = client
        super.init()
        setAccessibilityRole(.row)
        setAccessibilityLabel(row.name)
        setAccessibilityParent(host)
        setAccessibilityFrameInParentSpace(frame)
        if row.expandable {
            setAccessibilityDisclosed(row.open)
        }
    }

    override func accessibilityPerformPress() -> Bool {
        guard row.expandable else { return false }
        let press = TimelinePointer(
            x: Int32(client.x), y: Int32(client.y), button: .left, clicks: 1, mods: TimelineMods(shift: false, cmd: false, opt: false))
        let host = self.host
        MainActor.assumeIsolated {
            host.press(press)
            host.release(press)
        }
        return true
    }
}

/// The blank above the row list, level with the strips, then the list.
@MainActor
final class RowPaneView: NSView {
    let rows: RowHeaderView
    weak var canvas: TimelineCanvas?
    private let backdrop = NSView()

    init(rows: RowHeaderView) {
        self.rows = rows
        super.init(frame: .zero)
        wantsLayer = true
        layer?.backgroundColor = TimelineColors.panelBackground
        addSubview(backdrop)
        addSubview(rows)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("not from a nib")
    }

    override var isFlipped: Bool { true }

    override func layout() {
        super.layout()
        guard let canvas else { return }
        let v = canvas.state
        let top = CGFloat(v.waveformHeight + v.rulerHeight + v.marksBarHeight)
        rows.frame = CGRect(x: 0, y: top, width: bounds.width, height: canvas.grid.bounds.height)
        rows.layoutSubtreeIfNeeded()
        rows.setView(scrollY: CGFloat(v.verticalOffset))
    }
}

/// The stack on the right: waveform, ruler, marks bar, then the grid filling the rest. By hand, no Auto Layout.
@MainActor
final class TimePanelView: NSView {
    let waveform: WaveformView
    let ruler: RulerView
    let marks: MarksBarView
    let grid: GridViewportView
    weak var canvas: TimelineCanvas?

    init(waveform: WaveformView, ruler: RulerView, marks: MarksBarView, grid: GridViewportView) {
        self.waveform = waveform
        self.ruler = ruler
        self.marks = marks
        self.grid = grid
        super.init(frame: .zero)
        addSubview(waveform)
        addSubview(ruler)
        addSubview(marks)
        addSubview(grid)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("not from a nib")
    }

    override var isFlipped: Bool { true }

    override func layout() {
        super.layout()
        guard let canvas else { return }
        let v = canvas.state
        let w = bounds.width
        var y: CGFloat = 0
        waveform.frame = CGRect(x: 0, y: y, width: w, height: CGFloat(v.waveformHeight))
        waveform.isHidden = v.waveformHeight == 0
        y += CGFloat(v.waveformHeight)
        ruler.frame = CGRect(x: 0, y: y, width: w, height: CGFloat(v.rulerHeight))
        y += CGFloat(v.rulerHeight)
        marks.frame = CGRect(x: 0, y: y, width: w, height: CGFloat(v.marksBarHeight))
        marks.isHidden = v.marksBarHeight == 0
        y += CGFloat(v.marksBarHeight)
        grid.frame = CGRect(x: 0, y: y, width: w, height: max(0, bounds.height - y))
        grid.layoutSubtreeIfNeeded()
        for strip in [waveform as NSView, ruler, marks] {
            strip.layoutSubtreeIfNeeded()
        }
        canvas.rowPane.needsLayout = true
    }
}

/// The left pane and the right, with a divider. The left keeps its width when the window resizes. Minimum 100.
@MainActor
public final class TimelineContainerView: NSView, NSSplitViewDelegate {
    private let split = NSSplitView()
    private let rowPane: RowPaneView
    private let timePanel: TimePanelView
    weak var canvas: TimelineCanvas?
    nonisolated(unsafe) private var splitObserver: NSObjectProtocol?
    private var didSetDivider = false
    /// F2's restore rule: this if it's above 0 and the control is wider, otherwise the default if the control is wider than that.
    var splitterDistance: CGFloat = 200

    var dividerThickness: CGFloat { split.dividerThickness }

    init(rowPane: RowPaneView, timePanel: TimePanelView) {
        self.rowPane = rowPane
        self.timePanel = timePanel
        super.init(frame: .zero)
        wantsLayer = true
        split.isVertical = true
        split.dividerStyle = .thin
        split.delegate = self
        split.addArrangedSubview(rowPane)
        split.addArrangedSubview(timePanel)
        split.setHoldingPriority(.defaultHigh, forSubviewAt: 0)
        split.setHoldingPriority(.defaultLow, forSubviewAt: 1)
        addSubview(split)
        setAccessibilityIdentifier("timeline")
        // Through `self` only, and removed in deinit: a block that captured the row pane kept the whole closed timeline (and its sequence) alive.
        splitObserver = NotificationCenter.default.addObserver(forName: NSSplitView.didResizeSubviewsNotification, object: split, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self, self.didSetDivider else { return }
                self.canvas?.controllerSplitterMoved(width: self.rowPane.bounds.width)
            }
        }
    }

    deinit {
        if let splitObserver {
            NotificationCenter.default.removeObserver(splitObserver)
        }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("not from a nib")
    }

    public override func layout() {
        super.layout()
        split.frame = bounds
        if !didSetDivider, bounds.width > 0 {
            didSetDivider = true
            let wanted = splitterDistance > 0 && bounds.width > splitterDistance ? splitterDistance : 200
            split.setPosition(min(wanted, bounds.width - 100), ofDividerAt: 0)
        }
        split.layoutSubtreeIfNeeded()
        timePanel.needsLayout = true
    }

    /// The scale and the colour space both count as backing properties, and either can change when the window moves to another screen.
    public override func viewDidChangeBackingProperties() {
        super.viewDidChangeBackingProperties()
        canvas?.setDisplay(scale: window?.backingScaleFactor ?? 2, colorSpace: window?.colorSpace?.cgColorSpace ?? sRGBSpace)
    }

    public override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        canvas?.setDisplay(scale: window?.backingScaleFactor ?? 2, colorSpace: window?.colorSpace?.cgColorSpace ?? sRGBSpace)
    }

    public func splitView(_ splitView: NSSplitView, constrainMinCoordinate proposedMinimumPosition: CGFloat, ofSubviewAt dividerIndex: Int) -> CGFloat {
        max(proposedMinimumPosition, 100)
    }

    public func splitView(_ splitView: NSSplitView, canCollapseSubview subview: NSView) -> Bool {
        false
    }
}

extension TimelineCanvas {
    func setDisplay(scale: CGFloat, colorSpace: CGColorSpace) {
        if scale > 0 {
            self.scale = scale
        }
        for plane in grid.planes + [ruler.plane, marksBar.plane, waveform.plane, rows.plane] {
            if scale > 0 {
                plane.scale = scale
            }
            plane.colorSpace = colorSpace
        }
    }

    func controllerSplitterMoved(width: CGFloat) {
        rowPane.needsLayout = true
        var s = controller.settings()
        if s.splitterDistance != Int32(width) {
            s.splitterDistance = Int32(width)
            _ = controller.applySettings(s: s)
        }
    }
}
