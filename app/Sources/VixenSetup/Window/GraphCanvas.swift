import AppKit
import Observation
import QuartzCore
import SwiftUI
import VixenBridge
import VixenKit

/// A scene from Rust in the form the canvas draws: flat arrays, built off the main thread. Shapes aren't views or objects, so a thousand of them cost what the visible ones cost.
struct GraphSceneData: Sendable {
    var ids: [UInt64] = []
    var kinds: [UInt8] = []
    var titles: [String] = []
    var rects: [CGRect] = []
    var flags: [UInt8] = []
    var inputs: [UInt32] = []
    var outputs: [UInt32] = []
    var components: [Int64] = []
    var parents: [Int32] = []
    /// Shape indexes, bottom to top.
    var drawOrder: [Int32] = []
    var lineIDs: [UInt64] = []
    var lineKinds: [UInt8] = []
    var lineStarts: [CGPoint] = []
    var lineEnds: [CGPoint] = []
    var lineFrom: [Int32] = []
    var lineFromPorts: [UInt32] = []
    var lineTo: [Int32] = []
    var lineDestinations: [Int64] = []
    var contentSize = CGSize.zero
    /// The lines touching each shape, for a filter drag.
    var linesOfShape: [Int: [Int]] = [:]

    nonisolated init() {}

    nonisolated init(_ s: SetupGraphScene) {
        ids = s.ids
        kinds = [UInt8](s.kinds)
        titles = s.titles
        flags = [UInt8](s.flags)
        inputs = s.inputs
        outputs = s.outputs
        components = s.components
        parents = s.parents
        rects.reserveCapacity(ids.count)
        for i in 0..<ids.count {
            let at = i * 4
            rects.append(CGRect(x: CGFloat(s.rects[at]), y: CGFloat(s.rects[at + 1]), width: CGFloat(s.rects[at + 2]), height: CGFloat(s.rects[at + 3])))
        }
        let z = [UInt8](s.z)
        // A stable sort, so that within one level the order things were added is the order they stack.
        drawOrder = Array(0..<Int32(ids.count))
        drawOrder.sort { z[Int($0)] != z[Int($1)] ? z[Int($0)] < z[Int($1)] : $0 < $1 }
        lineIDs = s.lineIds
        lineKinds = [UInt8](s.lineKinds)
        lineFrom = s.lineFrom
        lineFromPorts = s.lineFromPorts
        lineTo = s.lineTo
        lineDestinations = s.lineDestinations
        lineStarts.reserveCapacity(lineIDs.count)
        lineEnds.reserveCapacity(lineIDs.count)
        for i in 0..<lineIDs.count {
            let at = i * 4
            lineStarts.append(CGPoint(x: CGFloat(s.linePoints[at]), y: CGFloat(s.linePoints[at + 1])))
            lineEnds.append(CGPoint(x: CGFloat(s.linePoints[at + 2]), y: CGFloat(s.linePoints[at + 3])))
            if lineFrom[i] >= 0 { linesOfShape[Int(lineFrom[i]), default: []].append(i) }
            if lineTo[i] >= 0 { linesOfShape[Int(lineTo[i]), default: []].append(i) }
        }
        contentSize = CGSize(width: CGFloat(s.contentWidth), height: CGFloat(s.contentHeight))
    }

    nonisolated func isHidden(_ i: Int) -> Bool { flags[i] & 1 != 0 }
    nonisolated func isExpandable(_ i: Int) -> Bool { flags[i] & 2 != 0 }
    nonisolated func isFilter(_ i: Int) -> Bool { kinds[i] == 2 }

    nonisolated func samePicture(as other: GraphSceneData) -> Bool {
        ids == other.ids && rects == other.rects && titles == other.titles && flags == other.flags && outputs == other.outputs
            && lineIDs == other.lineIDs && lineStarts == other.lineStarts && lineEnds == other.lineEnds
    }
}

/// F113's shape colours.
private enum ShapeStyle {
    static let gradients: [CGGradient] = {
        let space = CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB()
        let make = { (top: (CGFloat, CGFloat, CGFloat), bottom: (CGFloat, CGFloat, CGFloat)) -> CGGradient in
            let colors = [
                CGColor(srgbRed: top.0 / 255, green: top.1 / 255, blue: top.2 / 255, alpha: 1),
                CGColor(srgbRed: bottom.0 / 255, green: bottom.1 / 255, blue: bottom.2 / 255, alpha: 1),
            ] as CFArray
            return CGGradient(colorsSpace: space, colors: colors, locations: [0, 1]) ?? CGGradient(colorsSpace: space, colors: [CGColor(gray: 0.5, alpha: 1)] as CFArray, locations: [0])!
        }
        return [
            make((120, 160, 240), (90, 120, 180)),
            make((200, 220, 255), (140, 160, 200)),
            make((255, 220, 150), (230, 200, 100)),
            make((100, 200, 100), (50, 200, 50)),
            make((180, 230, 180), (120, 210, 120)),
        ]
    }()

    /// For very small sizes, where a gradient can't be seen and a flat fill is cheaper.
    static let flat: [CGColor] = [
        CGColor(srgbRed: 105 / 255, green: 140 / 255, blue: 210 / 255, alpha: 1),
        CGColor(srgbRed: 170 / 255, green: 190 / 255, blue: 228 / 255, alpha: 1),
        CGColor(srgbRed: 243 / 255, green: 210 / 255, blue: 125 / 255, alpha: 1),
        CGColor(srgbRed: 75 / 255, green: 200 / 255, blue: 75 / 255, alpha: 1),
        CGColor(srgbRed: 150 / 255, green: 220 / 255, blue: 150 / 255, alpha: 1),
    ]
}

/// The state SwiftUI shows beside the canvas.
@Observable
final class GraphToolbarState {
    /// F120: updated only on selection events, so it starts enabled.
    var canDeleteFilter = true
    var zoomPercent = 100
}

/// Everything about the graphical view but its drawing: when to ask for a scene and what gestures mean.
final class GraphCanvasController {
    weak var model: SetupModel?
    let toolbar = GraphToolbarState()
    let canvas = GraphCanvasView()
    let scrollView = GraphScrollView()
    private var active = false
    private var token = 0
    private var resizeTask: Task<Void, Never>?
    private var lastWidth = 0
    var studioSceneWidth: Int { lastWidth } // STUDIO: the width the shown scene was built for, so R43's labels match it (StudioGraphAccess.swift)
    /// F122's clipboard, of filter components. A cut filter can still be pasted, because its entry stays.
    private var clipboard: [UInt32] = []
    /// F117: what the next scene should leave selected, because it was just inserted.
    private var inserted: GraphCanvasView.Inserted?

    init() {
        scrollView.hasVerticalScroller = true
        scrollView.hasHorizontalScroller = true
        scrollView.autohidesScrollers = true
        scrollView.allowsMagnification = true
        scrollView.minMagnification = 0.01
        scrollView.maxMagnification = 40
        scrollView.drawsBackground = true
        scrollView.backgroundColor = GraphCanvasView.background
        scrollView.documentView = canvas
        scrollView.contentView.postsBoundsChangedNotifications = false
        scrollView.onResize = { [weak self] in self?.viewResized() }
        canvas.controller = self
    }

    func reset() {
        token += 1
        active = false
        clipboard = []
        canvas.setScene(GraphSceneData(), keepSelection: false)
        toolbar.canDeleteFilter = true
    }

    func activeChanged(_ on: Bool) {
        active = on
        if on {
            sceneNeeded(fit: true)
        }
    }

    /// F115: asked for off the main thread, and a newer request drops an older answer. An inactive view hears nothing (F3).
    func sceneNeeded(fit: Bool) {
        guard active, let model, let session = model.session else { return }
        token += 1
        let current = token
        let selection = model.elementSelection
        let rows = model.controllerSelection
        let width = viewWidth
        lastWidth = width
        Task {
            await Task.yield()
            guard current == token else { return }
            let data = await offMain { GraphSceneData(session.graphScene(selection: selection, rows: rows, width: Int32(width))) }
            guard current == token else { return }
            canvas.setScene(data, keepSelection: true)
            if let inserted {
                self.inserted = nil
                canvas.select(inserted: inserted)
            }
            if fit {
                zoomToFit()
            }
        }
    }

    /// `diagramDisplay.Width`: the control's, whatever the zoom.
    private var viewWidth: Int {
        max(Int(scrollView.frame.width), 400)
    }

    /// F115: everything again, debounced.
    private func viewResized() {
        guard active, viewWidth != lastWidth else { return }
        resizeTask?.cancel()
        resizeTask = Task { [weak self] in
            try? await Task.sleep(for: .milliseconds(250))
            if !Task.isCancelled {
                self?.sceneNeeded(fit: true)
            }
        }
    }

    // MARK: Zoom

    /// F125.
    func zoomToFit() {
        let size = canvas.scene.contentSize
        let clip = scrollView.frame.size
        let zoom = setupZoomToFit(viewWidth: Float(clip.width), viewHeight: Float(clip.height), contentWidth: Float(size.width), contentHeight: Float(size.height))
        setZoom(CGFloat(zoom))
        scrollView.contentView.scroll(to: .zero)
        scrollView.reflectScrolledClipView(scrollView.contentView)
    }

    func zoom(in zoomIn: Bool) {
        let level = Int32((scrollView.magnification * 100).rounded())
        setZoom(CGFloat(setupZoomStep(level: level, zoomIn: zoomIn)) / 100)
    }

    private func setZoom(_ value: CGFloat) {
        scrollView.magnification = value
        toolbar.zoomPercent = Int((value * 100).rounded())
        canvas.fitFrame()
    }

    // MARK: Gestures

    func selectionChanged(anyFilter: Bool) {
        toolbar.canDeleteFilter = anyFilter
    }

    /// F120, F126's batching: one update for the gesture.
    func delete(lines: [UInt32], filters: [UInt32]) {
        guard let model, !lines.isEmpty || !filters.isEmpty else { return }
        Task {
            guard let session = model.session else { return }
            await offMain { session.graphDelete(destinations: lines, filters: filters) }
            model.patchingUpdated()
        }
    }

    /// F119, on release.
    func connect(target: UInt32, source: UInt32, output: UInt32) {
        guard let model, let session = model.session else { return }
        Task {
            let done = await offMain { session.graphConnect(target: target, source: source, output: output) }
            if done {
                inserted = .line(destination: target)
                model.patchingUpdated()
            }
        }
    }

    func isConnectable(source: UInt32, target: UInt32) -> Bool {
        model?.session?.graphConnectCandidate(source: source, target: target) ?? false
    }

    /// F118's mouse up, with its undo.
    func placeFilters(_ placements: [SetupFilterPlacement], previous: [SetupFilterPlacement]) {
        guard let model, let session = model.session else { return }
        session.graphPlaceFilters(placed: placements)
        if let manager = canvas.undoManager {
            manager.registerUndo(withTarget: self) { controller in
                MainActor.assumeIsolated {
                    controller.placeFilters(previous, previous: placements)
                }
            }
            manager.setActionName("Move Filter")
        }
        sceneNeeded(fit: false)
    }

    /// F124.
    func toggleCollapsed(shape: UInt64) {
        guard let model, let session = model.session else { return }
        session.graphToggleCollapsed(shape: shape)
        sceneNeeded(fit: false)
    }

    /// F124: a filter's setup. Connections are rebuilt and patching refreshed after it, which upstream doesn't do.
    func configureFilter(component: UInt32) {
        guard let model, let session = model.session, let instance = session.filterInstanceId(component: component) else { return }
        let typeName = session.filterTypeName(component: component) ?? "Filter"
        Task {
            if await model.runDialog(.filter(instanceID: instance, typeName: typeName)) {
                model.patchingUpdated()
            }
        }
    }

    /// F121.
    func addFilter() {
        guard let model, let session = model.session else { return }
        Task {
            guard let choice = await model.promptList(title: "Add Filter", items: setupFilterTypes()) else { return }
            let visible = canvas.visibleRect
            let (x, y) = (Float(visible.midX - 80), Float(visible.midY - 10))
            if let added = await offMain({ try? session.graphAddFilter(typeId: choice.id, x: x, y: y) }) {
                inserted = .filter(component: added)
            }
            sceneNeeded(fit: false)
        }
    }

    /// The Delete Filter button: its handler deletes selected lines too.
    func deleteSelected() {
        canvas.deleteSelection()
    }

    /// F122.
    func copy(filters: [UInt32]) {
        if !filters.isEmpty {
            clipboard = filters
            canvas.clipboardHasFilters = true
        }
    }

    /// F122, F123.
    func paste(copies: UInt32, at point: CGPoint) {
        guard let model, let session = model.session, !clipboard.isEmpty else { return }
        let sources = clipboard
        Task {
            // NShape places a shape by its centre.
            let pasted = await offMain { session.graphPasteFilters(sources: sources, copies: copies, x: Float(point.x - 80), y: Float(point.y - 10)) }
            if let last = pasted.last {
                inserted = .filter(component: last)
            }
            sceneNeeded(fit: false)
        }
    }

    /// F123's "Paste Filter (Multiple)".
    func pasteMultiple(at point: CGPoint) {
        guard let model else { return }
        Task {
            guard let count = await model.promptNumber(title: "Number of Copies", prompt: "How many copies of the given filter(s)?", value: 1, min: 1, max: 1000) else { return }
            paste(copies: UInt32(count), at: point)
        }
    }
}

/// The scroll view, which says when its size changes.
final class GraphScrollView: NSScrollView {
    var onResize: (() -> Void)?

    override func setFrameSize(_ newSize: NSSize) {
        let changed = newSize != frame.size
        super.setFrameSize(newSize)
        if changed {
            onResize?()
        }
    }
}

/// One view for the whole diagram. Draws only what meets the dirty rect, keeps titles' layout between draws, and keeps everything a gesture moves on layers above it, so a drag never
/// redraws the graph.
final class GraphCanvasView: NSView {
    static let background = NSColor(srgbRed: 40 / 255, green: 40 / 255, blue: 40 / 255, alpha: 1)
    private static let gridColor = NSColor(white: 1, alpha: 0.05).cgColor
    private static let lineColor = NSColor(white: 0.88, alpha: 1).cgColor
    private static let stubColor = NSColor(srgbRed: 0.45, green: 0.65, blue: 1, alpha: 1).cgColor
    private static let portColor = NSColor(white: 0.1, alpha: 0.9).cgColor
    private static let shapeEdge = NSColor(white: 0, alpha: 0.45).cgColor
    private static let titleFont = NSFont.systemFont(ofSize: 11)

    weak var controller: GraphCanvasController?
    private(set) var scene = GraphSceneData()
    private var selectedShapes = Set<UInt64>()
    private var selectedLines = Set<UInt64>()
    private var titleCache: [UInt64: (title: String, width: CGFloat, line: CTLine)] = [:]
    var clipboardHasFilters = false

    private enum Gesture {
        case none
        case pressed(start: CGPoint, port: (shape: Int, output: Int)?, hit: Int?)
        case frame(start: CGPoint, additive: Bool)
        case moving(start: CGPoint, shapes: [Int], lines: [Int])
        case connecting(source: Int, output: Int, from: CGPoint, target: Int?)
        /// Right button or Control: connects on a drag, shows the menu on a release without one.
        case secondary(start: CGPoint, hit: Int?, port: Int?)
    }

    private var gesture = Gesture.none
    private var hiddenByGesture = Set<Int>()
    private var hiddenLinesByGesture = Set<Int>()
    private var moveOffset = CGSize.zero
    private var lastPoint: CGPoint?
    private var connectAnswers: [Int: Bool] = [:]

    private let overlayLines = CAShapeLayer()
    private let overlayShapes = CAShapeLayer()
    private let overlayPort = CAShapeLayer()
    private let overlayFrame = CAShapeLayer()

    /// Frame time of the last few draws, for measuring.
    private(set) var lastDrawDuration: Double = 0
    private(set) var drawCount = 0

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        wantsLayer = true
        layerContentsRedrawPolicy = .onSetNeedsDisplay
        let accent = NSColor.controlAccentColor.cgColor
        overlayLines.strokeColor = Self.lineColor
        overlayLines.fillColor = nil
        overlayLines.lineWidth = 1
        overlayShapes.fillColor = NSColor(srgbRed: 243 / 255, green: 210 / 255, blue: 125 / 255, alpha: 0.9).cgColor
        overlayShapes.strokeColor = accent
        overlayShapes.lineWidth = 2
        overlayPort.fillColor = NSColor.systemGreen.cgColor
        overlayFrame.fillColor = accent.copy(alpha: 0.15)
        overlayFrame.strokeColor = accent
        overlayFrame.lineWidth = 1
        for layer in [overlayLines, overlayShapes, overlayPort, overlayFrame] {
            layer.actions = ["path": NSNull(), "position": NSNull(), "bounds": NSNull(), "transform": NSNull(), "hidden": NSNull()]
            layer.zPosition = 10
            layer.isHidden = true
            self.layer?.addSublayer(layer)
        }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("not used")
    }

    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }
    override var isOpaque: Bool { true }
    override var wantsDefaultClipping: Bool { true }

    override func layout() {
        super.layout()
        for layer in [overlayLines, overlayShapes, overlayPort, overlayFrame] {
            layer.frame = bounds
        }
    }

    // MARK: The scene

    func setScene(_ new: GraphSceneData, keepSelection: Bool) {
        // Nothing visible changed, as when a selection only added to what's already shown: nothing redraws.
        let same = new.samePicture(as: scene)
        scene = new
        connectAnswers = [:]
        let hadSelection = selectedShapes.count + selectedLines.count
        if keepSelection {
            // F117: selection stays on shapes still present.
            let ids = Set(new.ids)
            selectedShapes.formIntersection(ids)
            selectedLines.formIntersection(Set(new.lineIDs))
        } else {
            selectedShapes = []
            selectedLines = []
            titleCache = [:]
        }
        fitFrame()
        // F120: the button only hears about selection events, so it stays as it was when nothing was deselected here.
        if selectedShapes.count + selectedLines.count != hadSelection {
            reportSelection()
        }
        if !same {
            needsDisplay = true
        }
    }

    /// The document is as big as the content, and at least as big as the view at this zoom, so the background and a frame-drag reach everywhere.
    func fitFrame() {
        guard let scroll = enclosingScrollView else { return }
        let zoom = max(scroll.magnification, 0.01)
        let size = NSSize(width: max(scene.contentSize.width, scroll.frame.width / zoom), height: max(scene.contentSize.height, scroll.frame.height / zoom))
        if frame.size != size {
            setFrameSize(size)
        }
    }

    private var zoom: CGFloat { enclosingScrollView?.magnification ?? 1 }

    // MARK: Drawing

    override func draw(_ dirtyRect: NSRect) {
        guard let ctx = NSGraphicsContext.current?.cgContext else { return }
        let started = CACurrentMediaTime()
        let zoom = self.zoom
        ctx.setFillColor(Self.background.cgColor)
        ctx.fill(dirtyRect)
        drawGrid(ctx, dirtyRect, zoom: zoom)

        let showDetail = zoom * 11 >= 5
        let showPorts = zoom >= 0.35
        let flat = zoom * 20 < 7
        let lifted = !hiddenByGesture.isEmpty
        let portPath = CGMutablePath()
        let radius: CGFloat = 2.5

        for order in scene.drawOrder where order >= 0 {
            let i = Int(order)
            if scene.isHidden(i) || (lifted && hiddenByGesture.contains(i)) { continue }
            let rect = scene.rects[i]
            guard rect.intersects(dirtyRect) else { continue }
            let kind = Int(scene.kinds[i])
            if flat {
                ctx.setFillColor(ShapeStyle.flat[kind])
                ctx.fill(rect)
            } else {
                let path = CGPath(roundedRect: rect, cornerWidth: 3, cornerHeight: 3, transform: nil)
                ctx.saveGState()
                ctx.addPath(path)
                ctx.clip()
                ctx.drawLinearGradient(ShapeStyle.gradients[kind], start: CGPoint(x: rect.minX, y: rect.minY), end: CGPoint(x: rect.minX, y: rect.maxY), options: [])
                ctx.restoreGState()
                ctx.addPath(path)
                ctx.setStrokeColor(Self.shapeEdge)
                ctx.setLineWidth(1)
                ctx.strokePath()
            }
            if selectedShapes.contains(scene.ids[i]) {
                ctx.addRect(rect.insetBy(dx: -1, dy: -1))
                ctx.setStrokeColor(NSColor.controlAccentColor.cgColor)
                ctx.setLineWidth(2)
                ctx.strokePath()
            }
            if showDetail {
                drawTitle(ctx, i, rect)
            }
            if showPorts {
                let inputs = scene.inputs[i]
                let outputs = scene.outputs[i]
                if inputs > 0 {
                    let y = rect.minY + rect.height * CGFloat(setupPortFraction(index: 0, count: 1))
                    portPath.addEllipse(in: CGRect(x: rect.minX - radius, y: y - radius, width: radius * 2, height: radius * 2))
                }
                if outputs > 0 {
                    for o in 0..<outputs {
                        let y = rect.minY + rect.height * CGFloat(Self.fraction(o, outputs))
                        portPath.addEllipse(in: CGRect(x: rect.maxX - radius, y: y - radius, width: radius * 2, height: radius * 2))
                    }
                }
            }
        }
        ctx.addPath(portPath)
        ctx.setFillColor(Self.portColor)
        ctx.fillPath()
        drawLines(ctx, dirtyRect)
        lastDrawDuration = CACurrentMediaTime() - started
        drawCount += 1
    }

    /// F114. The same sum as Rust's, kept here because a redraw can't cross for it per port.
    private static func fraction(_ index: UInt32, _ count: UInt32) -> Double {
        count <= 1 ? 0.5 : 0.1 + 0.8 * Double(index) / Double(count - 1)
    }

    private func drawGrid(_ ctx: CGContext, _ dirty: CGRect, zoom: CGFloat) {
        guard zoom >= 0.5 else { return }
        let spacing: CGFloat = 19
        let path = CGMutablePath()
        var x = (dirty.minX / spacing).rounded(.down) * spacing
        while x <= dirty.maxX {
            path.move(to: CGPoint(x: x, y: dirty.minY))
            path.addLine(to: CGPoint(x: x, y: dirty.maxY))
            x += spacing
        }
        var y = (dirty.minY / spacing).rounded(.down) * spacing
        while y <= dirty.maxY {
            path.move(to: CGPoint(x: dirty.minX, y: y))
            path.addLine(to: CGPoint(x: dirty.maxX, y: y))
            y += spacing
        }
        ctx.addPath(path)
        ctx.setStrokeColor(Self.gridColor)
        ctx.setLineWidth(1)
        ctx.strokePath()
    }

    /// F113: the font shrinks by steps of 0.9 until the title fits 90% of the width, or it reaches 6.
    private func titleLine(_ i: Int, width: CGFloat) -> CTLine {
        let id = scene.ids[i]
        let title = scene.titles[i]
        if let cached = titleCache[id], cached.width == width, cached.title == title {
            return cached.line
        }
        let measure = CTLineCreateWithAttributedString(NSAttributedString(string: title, attributes: [.font: Self.titleFont, .foregroundColor: NSColor.black]))
        let natural = CGFloat(CTLineGetTypographicBounds(measure, nil, nil, nil))
        var size: CGFloat = 11
        while natural * size / 11 > width * 0.9, size > 6 {
            size = max(size * 0.9, 6)
            if size == 6 { break }
        }
        let line = size == 11 ? measure : CTLineCreateWithAttributedString(NSAttributedString(string: title, attributes: [.font: NSFont.systemFont(ofSize: size), .foregroundColor: NSColor.black]))
        titleCache[id] = (title, width, line)
        return line
    }

    private func drawTitle(_ ctx: CGContext, _ i: Int, _ rect: CGRect) {
        let line = titleLine(i, width: rect.width)
        var ascent: CGFloat = 0
        var descent: CGFloat = 0
        let width = CGFloat(CTLineGetTypographicBounds(line, &ascent, &descent, nil))
        // A group's title sits centred in its 20pt header.
        let band = scene.isExpandable(i) && rect.height > 20 ? CGRect(x: rect.minX, y: rect.minY, width: rect.width, height: 20) : rect
        let x = rect.minX + (rect.width - width) / 2
        let y = band.midY + (ascent - descent) / 2
        ctx.saveGState()
        ctx.textMatrix = CGAffineTransform(scaleX: 1, y: -1)
        ctx.textPosition = CGPoint(x: x, y: y)
        CTLineDraw(line, ctx)
        ctx.restoreGState()
    }

    private func drawLines(_ ctx: CGContext, _ dirty: CGRect) {
        let real = CGMutablePath()
        let stubs = CGMutablePath()
        let heads = CGMutablePath()
        let selected = CGMutablePath()
        let lifted = !hiddenLinesByGesture.isEmpty
        for i in 0..<scene.lineIDs.count {
            if lifted && hiddenLinesByGesture.contains(i) { continue }
            let (a, b) = (scene.lineStarts[i], scene.lineEnds[i])
            let box = CGRect(x: min(a.x, b.x) - 6, y: min(a.y, b.y) - 6, width: abs(a.x - b.x) + 12, height: abs(a.y - b.y) + 12)
            guard box.intersects(dirty) else { continue }
            let kind = scene.lineKinds[i]
            let path = selectedLines.contains(scene.lineIDs[i]) ? selected : (kind == 0 ? real : stubs)
            path.move(to: a)
            path.addLine(to: b)
            if kind == 0 {
                Self.addArrowHead(to: heads, from: a, to: b)
            }
        }
        ctx.setLineWidth(1)
        ctx.addPath(real)
        ctx.setStrokeColor(Self.lineColor)
        ctx.strokePath()
        ctx.addPath(heads)
        ctx.setFillColor(Self.lineColor)
        ctx.fillPath()
        if !stubs.isEmpty {
            ctx.saveGState()
            ctx.setLineDash(phase: 0, lengths: [4, 3])
            ctx.addPath(stubs)
            ctx.setStrokeColor(Self.stubColor)
            ctx.strokePath()
            ctx.restoreGState()
        }
        if !selected.isEmpty {
            ctx.addPath(selected)
            ctx.setStrokeColor(NSColor.controlAccentColor.cgColor)
            ctx.setLineWidth(2)
            ctx.strokePath()
        }
    }

    private static func addArrowHead(to path: CGMutablePath, from a: CGPoint, to b: CGPoint) {
        let angle = atan2(b.y - a.y, b.x - a.x)
        let length: CGFloat = 7
        let spread: CGFloat = 0.4
        path.move(to: b)
        path.addLine(to: CGPoint(x: b.x - length * cos(angle - spread), y: b.y - length * sin(angle - spread)))
        path.addLine(to: CGPoint(x: b.x - length * cos(angle + spread), y: b.y - length * sin(angle + spread)))
        path.closeSubpath()
    }

    // MARK: Hit testing

    private func portPoint(shape i: Int, output o: Int) -> CGPoint {
        let rect = scene.rects[i]
        return CGPoint(x: rect.maxX, y: rect.minY + rect.height * CGFloat(Self.fraction(UInt32(o), scene.outputs[i])))
    }

    private func inputPoint(shape i: Int) -> CGPoint {
        let rect = scene.rects[i]
        return CGPoint(x: rect.minX, y: rect.midY)
    }

    /// The top-most visible shape at a point, within `slop`.
    private func topShape(at p: CGPoint, slop: CGFloat = 0) -> Int? {
        for order in scene.drawOrder.reversed() {
            let i = Int(order)
            if !scene.isHidden(i), scene.rects[i].insetBy(dx: -slop, dy: -slop).contains(p) {
                return i
            }
        }
        return nil
    }

    private func outputPort(at p: CGPoint) -> (shape: Int, output: Int)? {
        let reach = max(6 / zoom, 6)
        for order in scene.drawOrder.reversed() {
            let i = Int(order)
            let count = scene.outputs[i]
            guard count > 0, !scene.isHidden(i) else { continue }
            let rect = scene.rects[i]
            guard abs(p.x - rect.maxX) <= reach, p.y >= rect.minY - reach, p.y <= rect.maxY + reach else { continue }
            for o in 0..<Int(count) {
                let point = portPoint(shape: i, output: o)
                if hypot(point.x - p.x, point.y - p.y) <= reach {
                    return (i, o)
                }
            }
        }
        return nil
    }

    private func line(at p: CGPoint) -> Int? {
        let reach = max(3 / zoom, 3)
        for i in stride(from: scene.lineIDs.count - 1, through: 0, by: -1) {
            let (a, b) = (scene.lineStarts[i], scene.lineEnds[i])
            if p.x < min(a.x, b.x) - reach || p.x > max(a.x, b.x) + reach || p.y < min(a.y, b.y) - reach || p.y > max(a.y, b.y) + reach { continue }
            let (dx, dy) = (b.x - a.x, b.y - a.y)
            let length = dx * dx + dy * dy
            let t = length == 0 ? 0 : max(0, min(1, ((p.x - a.x) * dx + (p.y - a.y) * dy) / length))
            if hypot(a.x + t * dx - p.x, a.y + t * dy - p.y) <= reach {
                return i
            }
        }
        return nil
    }

    // MARK: Selection

    private func reportSelection() {
        var anyFilter = false
        if !selectedShapes.isEmpty {
            for i in 0..<scene.ids.count where scene.isFilter(i) && selectedShapes.contains(scene.ids[i]) {
                anyFilter = true
                break
            }
        }
        controller?.selectionChanged(anyFilter: anyFilter)
    }

    /// F117: inserting something selects it, replacing the selection.
    enum Inserted {
        case filter(component: UInt32)
        case line(destination: UInt32)
    }

    func select(inserted: Inserted) {
        switch inserted {
        case .filter(let component):
            guard let i = scene.ids.indices.first(where: { scene.isFilter($0) && scene.components[$0] == Int32(component) }) else { return }
            selectedShapes = [scene.ids[i]]
            selectedLines = []
        case .line(let destination):
            guard let i = scene.lineIDs.indices.first(where: { scene.lineDestinations[$0] == Int32(destination) }) else { return }
            selectedShapes = []
            selectedLines = [scene.lineIDs[i]]
        }
        needsDisplay = true
        reportSelection()
    }

    private func redraw(shape i: Int) {
        setNeedsDisplay(scene.rects[i].insetBy(dx: -4, dy: -4))
    }

    private func clearSelection() {
        if selectedShapes.isEmpty && selectedLines.isEmpty { return }
        selectedShapes = []
        selectedLines = []
        needsDisplay = true
        reportSelection()
    }

    /// F117.
    private func select(shape: Int?, line lineIndex: Int?, modifiers: NSEvent.ModifierFlags) {
        let adds = modifiers.contains(.command) || modifiers.contains(.shift)
        if let lineIndex {
            let id = scene.lineIDs[lineIndex]
            if adds {
                if selectedLines.remove(id) == nil { selectedLines.insert(id) }
            } else if !selectedLines.contains(id) {
                selectedShapes = []
                selectedLines = [id]
            }
            needsDisplay = true
        } else if let shape {
            let id = scene.ids[shape]
            if adds {
                if selectedShapes.remove(id) == nil { selectedShapes.insert(id) }
            } else if !selectedShapes.contains(id) {
                selectedShapes = [id]
                selectedLines = []
            }
            needsDisplay = true
        } else if !adds {
            clearSelection()
        }
        reportSelection()
    }

    /// NShape's ⌘A and frame select take the diagram's own shapes, which are the root shapes and the lines: a nested shape is part of its group.
    override func selectAll(_ sender: Any?) {
        selectedShapes = Set(scene.ids.indices.filter { !scene.isHidden($0) && scene.parents[$0] < 0 }.map { scene.ids[$0] })
        selectedLines = Set(scene.lineIDs)
        needsDisplay = true
        reportSelection()
    }

    private var selectedFilterComponents: [UInt32] {
        scene.ids.indices.compactMap { i in
            guard scene.isFilter(i), selectedShapes.contains(scene.ids[i]), scene.components[i] >= 0 else { return nil }
            return UInt32(scene.components[i])
        }
    }

    /// F120: selected lines and filters. Elements, outputs and controllers are ignored.
    func deleteSelection() {
        var rightStub = false
        let lines = scene.lineIDs.indices.compactMap { i -> UInt32? in
            guard selectedLines.contains(scene.lineIDs[i]) else { return nil }
            guard scene.lineDestinations[i] >= 0 else {
                // A right stub doesn't say which hidden destination it means. Upstream throws here.
                rightStub = true
                return nil
            }
            return UInt32(scene.lineDestinations[i])
        }
        let filters = selectedFilterComponents
        if rightStub, lines.isEmpty, filters.isEmpty {
            NSSound.beep()
        }
        controller?.delete(lines: lines, filters: filters)
    }

    // MARK: Cursor

    private var trackingArea: NSTrackingArea?
    private(set) var cursorName = "arrow"

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let trackingArea {
            removeTrackingArea(trackingArea)
        }
        let area = NSTrackingArea(rect: .zero, options: [.mouseMoved, .mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect], owner: self, userInfo: nil)
        addTrackingArea(area)
        trackingArea = area
    }

    override func mouseMoved(with event: NSEvent) {
        updateCursor(at: convert(event.locationInWindow, from: nil))
    }

    override func mouseExited(with event: NSEvent) {
        setCursor(.arrow, named: "arrow")
    }

    /// F151.
    private func updateCursor(at p: CGPoint) {
        switch gesture {
        case .moving:
            setCursor(.closedHand, named: "closedHand")
        case .connecting:
            setCursor(.pointingHand, named: "pointingHand")
        default:
            if outputPort(at: p) != nil {
                setCursor(.crosshair, named: "crosshair")
            } else if let i = topShape(at: p), scene.isFilter(i) {
                setCursor(.openHand, named: "openHand")
            } else {
                setCursor(.arrow, named: "arrow")
            }
        }
    }

    private func setCursor(_ cursor: NSCursor, named name: String) {
        cursorName = name
        cursor.set()
    }

    // MARK: Mouse

    override func mouseDown(with event: NSEvent) {
        studioAccessNoteClick() // STUDIO: no focus ring after a click (StudioGraphAccess.swift)
        window?.makeFirstResponder(self)
        let p = convert(event.locationInWindow, from: nil)
        lastPoint = p
        if event.modifierFlags.contains(.control) {
            beginSecondary(at: p)
            return
        }
        if event.clickCount == 2 {
            doubleClick(at: p)
            return
        }
        let port = outputPort(at: p)
        let lineHit = port == nil ? line(at: p) : nil
        let shape = lineHit == nil ? topShape(at: p) : nil
        if port == nil {
            select(shape: shape, line: lineHit, modifiers: event.modifierFlags)
        }
        gesture = .pressed(start: p, port: port, hit: shape)
    }

    override func mouseDragged(with event: NSEvent) {
        let p = convert(event.locationInWindow, from: nil)
        lastPoint = p
        autoscroll(with: event)
        switch gesture {
        case .pressed(let start, let port, let hit):
            guard hypot(p.x - start.x, p.y - start.y) * zoom >= 3 else { return }
            if let port {
                // F119 on the Mac: a left drag from an output port connects.
                beginConnect(source: port.shape, output: port.output)
                updateDrag(to: p)
                return
            }
            let shapes = selectedShapeIndexes
            if let hit, selectedShapes.contains(scene.ids[hit]), !shapes.isEmpty, shapes.allSatisfy({ scene.isFilter($0) }), selectedLines.isEmpty {
                beginMove(start: start, shapes: shapes)
            } else {
                // F118: anything unmovable in the selection makes the drag a frame select.
                gesture = .frame(start: start, additive: event.modifierFlags.contains(.command) || event.modifierFlags.contains(.shift))
            }
            updateDrag(to: p)
        case .none:
            break
        default:
            updateDrag(to: p)
        }
    }

    override func mouseUp(with event: NSEvent) {
        if case .secondary = gesture {
            rightMouseUp(with: event)
            return
        }
        let p = convert(event.locationInWindow, from: nil)
        finishGesture(at: p, event: event)
    }

    override func rightMouseDown(with event: NSEvent) {
        studioAccessNoteClick() // STUDIO: no focus ring after a right click (StudioGraphAccess.swift)
        window?.makeFirstResponder(self)
        beginSecondary(at: convert(event.locationInWindow, from: nil))
    }

    private func beginSecondary(at p: CGPoint) {
        lastPoint = p
        let port = outputPort(at: p)
        gesture = .secondary(start: p, hit: port?.shape ?? topShape(at: p), port: port?.output)
    }

    override func rightMouseDragged(with event: NSEvent) {
        let p = convert(event.locationInWindow, from: nil)
        lastPoint = p
        autoscroll(with: event)
        if case .secondary(let start, let hit, let port) = gesture {
            guard hypot(p.x - start.x, p.y - start.y) * zoom >= 3 else { return }
            guard let hit, scene.outputs[hit] > 0 else {
                gesture = .none
                return
            }
            // F119: the port under the pointer, else the first with no line on it, else the first.
            beginConnect(source: hit, output: port ?? firstFreeOutput(of: hit))
        }
        updateDrag(to: p)
    }

    override func rightMouseUp(with event: NSEvent) {
        let p = convert(event.locationInWindow, from: nil)
        if case .secondary = gesture {
            gesture = .none
            showMenu(with: event)
            return
        }
        finishGesture(at: p, event: event)
    }

    private func firstFreeOutput(of shape: Int) -> Int {
        let used = Set((scene.linesOfShape[shape] ?? []).filter { scene.lineFrom[$0] == Int32(shape) }.map { Int(scene.lineFromPorts[$0]) })
        return (0..<Int(scene.outputs[shape])).first { !used.contains($0) } ?? 0
    }

    private var selectedShapeIndexes: [Int] {
        scene.ids.indices.filter { selectedShapes.contains(scene.ids[$0]) }
    }

    private func doubleClick(at p: CGPoint) {
        gesture = .none
        // F124: on a selected shape only.
        guard let i = topShape(at: p), selectedShapes.contains(scene.ids[i]) else { return }
        switch scene.kinds[i] {
        case 0, 3:
            controller?.toggleCollapsed(shape: scene.ids[i])
        case 2:
            if scene.components[i] >= 0 {
                controller?.configureFilter(component: UInt32(scene.components[i]))
            }
        default:
            break
        }
    }

    private func beginConnect(source: Int, output: Int) {
        connectAnswers = [:]
        gesture = .connecting(source: source, output: output, from: portPoint(shape: source, output: output), target: nil)
        overlayLines.strokeColor = Self.lineColor
        overlayLines.lineWidth = 1
        overlayLines.lineDashPattern = nil
        overlayLines.isHidden = false
        setCursor(.pointingHand, named: "pointingHand")
    }

    private func beginMove(start: CGPoint, shapes: [Int]) {
        var lines = Set<Int>()
        for s in shapes {
            lines.formUnion(scene.linesOfShape[s] ?? [])
        }
        hiddenByGesture = Set(shapes)
        hiddenLinesByGesture = lines
        moveOffset = .zero
        let path = CGMutablePath()
        for s in shapes {
            path.addRoundedRect(in: scene.rects[s], cornerWidth: 3, cornerHeight: 3)
        }
        overlayShapes.path = path
        overlayShapes.isHidden = false
        overlayLines.strokeColor = Self.lineColor
        overlayLines.lineWidth = 1
        overlayLines.lineDashPattern = nil
        overlayLines.isHidden = false
        gesture = .moving(start: start, shapes: shapes, lines: Array(lines))
        // The lifted shapes and lines leave the picture once. From here the move only changes the layers.
        needsDisplay = true
        setCursor(.closedHand, named: "closedHand")
    }

    private func updateDrag(to p: CGPoint) {
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        defer { CATransaction.commit() }
        switch gesture {
        case .frame(let start, _):
            overlayFrame.path = CGPath(rect: CGRect(x: min(start.x, p.x), y: min(start.y, p.y), width: abs(p.x - start.x), height: abs(p.y - start.y)), transform: nil)
            overlayFrame.isHidden = false
        case .moving(let start, _, let lines):
            moveOffset = CGSize(width: p.x - start.x, height: p.y - start.y)
            overlayShapes.setAffineTransform(CGAffineTransform(translationX: moveOffset.width, y: moveOffset.height))
            let path = CGMutablePath()
            for l in lines {
                let moved = { (shape: Int32) in shape >= 0 && self.hiddenByGesture.contains(Int(shape)) }
                let a = scene.lineStarts[l]
                let b = scene.lineEnds[l]
                path.move(to: moved(scene.lineFrom[l]) ? CGPoint(x: a.x + moveOffset.width, y: a.y + moveOffset.height) : a)
                path.addLine(to: moved(scene.lineTo[l]) ? CGPoint(x: b.x + moveOffset.width, y: b.y + moveOffset.height) : b)
            }
            overlayLines.path = path
        case .connecting(let source, let output, let from, _):
            let target = connectTarget(at: p, source: source)
            gesture = .connecting(source: source, output: output, from: from, target: target)
            let path = CGMutablePath()
            path.move(to: from)
            path.addLine(to: p)
            overlayLines.path = path
            if let target {
                let port = inputPoint(shape: target)
                overlayPort.path = CGPath(ellipseIn: CGRect(x: port.x - 5, y: port.y - 5, width: 10, height: 10), transform: nil)
                overlayPort.isHidden = false
            } else {
                overlayPort.isHidden = true
            }
        default:
            break
        }
    }

    /// F119: the top-most shape within 5 units with an input, not the source and not forming a loop. Rust's answer is kept per shape for the drag, so hovering costs nothing more.
    private func connectTarget(at p: CGPoint, source: Int) -> Int? {
        guard let hit = topShape(at: p, slop: 5), hit != source, scene.inputs[hit] > 0, scene.components[hit] >= 0, scene.components[source] >= 0 else { return nil }
        if let known = connectAnswers[hit] {
            return known ? hit : nil
        }
        let allowed = controller?.isConnectable(source: UInt32(scene.components[source]), target: UInt32(scene.components[hit])) ?? false
        connectAnswers[hit] = allowed
        return allowed ? hit : nil
    }

    private func finishGesture(at p: CGPoint, event: NSEvent) {
        let finished = gesture
        gesture = .none
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        defer {
            CATransaction.commit()
            updateCursor(at: p)
        }
        overlayFrame.isHidden = true
        overlayPort.isHidden = true
        switch finished {
        case .pressed(_, let port, _):
            if let port {
                select(shape: port.shape, line: nil, modifiers: event.modifierFlags)
            }
        case .frame(let start, let additive):
            let box = CGRect(x: min(start.x, p.x), y: min(start.y, p.y), width: abs(p.x - start.x), height: abs(p.y - start.y))
            if !additive {
                selectedShapes = []
                selectedLines = []
            }
            for i in scene.ids.indices where !scene.isHidden(i) && scene.parents[i] < 0 && box.contains(scene.rects[i]) {
                selectedShapes.insert(scene.ids[i])
            }
            for i in scene.lineIDs.indices where box.contains(scene.lineStarts[i]) && box.contains(scene.lineEnds[i]) {
                selectedLines.insert(scene.lineIDs[i])
            }
            needsDisplay = true
            reportSelection()
        case .moving(_, let shapes, _):
            commitMove(shapes: shapes)
        case .connecting(let source, let output, _, let target):
            overlayLines.isHidden = true
            if let target {
                controller?.connect(target: UInt32(scene.components[target]), source: UInt32(scene.components[source]), output: UInt32(output))
            }
        case .secondary, .none:
            break
        }
    }

    /// F118's commit: the filters take their new places, and a rebuild from Rust settles the lines.
    private func commitMove(shapes: [Int]) {
        var placed: [SetupFilterPlacement] = []
        var previous: [SetupFilterPlacement] = []
        for s in shapes where scene.components[s] >= 0 {
            let rect = scene.rects[s]
            let component = UInt32(scene.components[s])
            previous.append(SetupFilterPlacement(component: component, x: Float(rect.minX), y: Float(rect.minY), w: Float(rect.width), h: Float(rect.height)))
            let moved = rect.offsetBy(dx: moveOffset.width, dy: moveOffset.height)
            placed.append(SetupFilterPlacement(component: component, x: Float(moved.minX), y: Float(moved.minY), w: Float(moved.width), h: Float(moved.height)))
        }
        // Until the new scene arrives, draw the shapes where they landed.
        for s in shapes {
            scene.rects[s] = scene.rects[s].offsetBy(dx: moveOffset.width, dy: moveOffset.height)
        }
        for (l, _) in scene.lineIDs.enumerated() where hiddenLinesByGesture.contains(l) {
            if scene.lineFrom[l] >= 0, hiddenByGesture.contains(Int(scene.lineFrom[l])) {
                scene.lineStarts[l] = CGPoint(x: scene.lineStarts[l].x + moveOffset.width, y: scene.lineStarts[l].y + moveOffset.height)
            }
            if scene.lineTo[l] >= 0, hiddenByGesture.contains(Int(scene.lineTo[l])) {
                scene.lineEnds[l] = CGPoint(x: scene.lineEnds[l].x + moveOffset.width, y: scene.lineEnds[l].y + moveOffset.height)
            }
        }
        hiddenByGesture = []
        hiddenLinesByGesture = []
        overlayShapes.isHidden = true
        overlayShapes.setAffineTransform(.identity)
        overlayLines.isHidden = true
        needsDisplay = true
        if moveOffset != .zero {
            controller?.placeFilters(placed, previous: previous)
        }
    }

    private func showMenu(with event: NSEvent) {
        NSMenu.popUpContextMenu(contextMenu(), with: event, for: self)
    }

    /// F123: attached here, where upstream's strip is defined but unreachable.
    func contextMenu() -> NSMenu {
        let point = lastPoint ?? .zero
        let anyFilter = !selectedFilterComponents.isEmpty
        let menu = NSMenu()
        menu.autoenablesItems = false
        menu.addItem(SetupMenuItem("Copy Filter", enabled: anyFilter) { [weak self] in
            guard let self else { return }
            self.controller?.copy(filters: self.selectedFilterComponents)
        })
        menu.addItem(SetupMenuItem("Paste Filter", enabled: clipboardHasFilters) { [weak self] in
            self?.controller?.paste(copies: 1, at: point)
        })
        menu.addItem(SetupMenuItem("Paste Filter (Multiple)", enabled: clipboardHasFilters) { [weak self] in
            self?.controller?.pasteMultiple(at: point)
        })
        return menu
    }

    // STUDIO-BEGIN graph-access-hooks: what R43's keyboard and VoiceOver reach inside the canvas (StudioGraphAccess.swift)
    var studioSelectedShapeIDs: Set<UInt64> { selectedShapes }

    /// Selects one shape alone, as a click on it does, and scrolls it into view.
    func studioSelectShape(_ i: Int) {
        selectedShapes = [scene.ids[i]]
        selectedLines = []
        needsDisplay = true
        reportSelection()
        scrollToVisible(scene.rects[i].insetBy(dx: -20, dy: -20))
    }

    func studioFirstFreeOutput(of i: Int) -> Int { firstFreeOutput(of: i) }

    /// Where a menu shown from the keyboard pastes.
    func studioSetMenuPoint(_ p: CGPoint) { lastPoint = p }
    // STUDIO-END graph-access-hooks

    // MARK: Keyboard and Edit menu

    override func keyDown(with event: NSEvent) {
        if studioAccessKeyDown(event) { return } // STUDIO: R43's Tab between shapes and Return for a shape's menu (StudioGraphAccess.swift)
        let key = event.charactersIgnoringModifiers?.unicodeScalars.first.map { Int($0.value) } ?? 0
        if key == 0x1B, cancelGesture() {
            return
        }
        if key == 0x7F || key == NSDeleteFunctionKey || key == NSBackspaceCharacter {
            deleteSelection()
            return
        }
        // Arrow keys scroll one small step.
        let step: CGFloat = 20 / max(zoom, 0.01)
        var delta: CGPoint?
        switch key {
        case NSUpArrowFunctionKey: delta = CGPoint(x: 0, y: -step)
        case NSDownArrowFunctionKey: delta = CGPoint(x: 0, y: step)
        case NSLeftArrowFunctionKey: delta = CGPoint(x: -step, y: 0)
        case NSRightArrowFunctionKey: delta = CGPoint(x: step, y: 0)
        default: break
        }
        if let delta, let clip = enclosingScrollView?.contentView {
            clip.scroll(to: NSPoint(x: clip.bounds.minX + delta.x, y: clip.bounds.minY + delta.y))
            enclosingScrollView?.reflectScrolledClipView(clip)
            return
        }
        super.keyDown(with: event)
    }

    override func cancelOperation(_ sender: Any?) {
        if !cancelGesture() {
            super.cancelOperation(sender)
        }
    }

    /// F150: Esc in the middle of a drag cancels that drag and nothing else. The window's Cancel button would otherwise take the key first.
    private func cancelGesture() -> Bool {
        switch gesture {
        case .none:
            return false
        case .moving:
            hiddenByGesture = []
            hiddenLinesByGesture = []
            moveOffset = .zero
            needsDisplay = true
        default:
            break
        }
        gesture = .none
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        overlayFrame.isHidden = true
        overlayPort.isHidden = true
        overlayShapes.isHidden = true
        overlayShapes.setAffineTransform(.identity)
        overlayLines.isHidden = true
        CATransaction.commit()
        if let lastPoint {
            updateCursor(at: lastPoint)
        }
        return true
    }

    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        if event.charactersIgnoringModifiers == "\u{1B}", cancelGesture() {
            return true
        }
        guard window?.firstResponder === self, event.modifierFlags.intersection(.deviceIndependentFlagsMask) == .command else {
            return super.performKeyEquivalent(with: event)
        }
        switch event.charactersIgnoringModifiers {
        case "=", "+":
            controller?.zoom(in: true)
            return true
        case "-":
            controller?.zoom(in: false)
            return true
        default:
            return super.performKeyEquivalent(with: event)
        }
    }

    @objc func copy(_ sender: Any?) {
        controller?.copy(filters: selectedFilterComponents)
    }

    @objc func cut(_ sender: Any?) {
        controller?.copy(filters: selectedFilterComponents)
        deleteSelection()
    }

    @objc func paste(_ sender: Any?) {
        // F122: at the mouse position, or at the last click if the pointer has left the view.
        var point = lastPoint ?? CGPoint(x: visibleRect.midX, y: visibleRect.midY)
        if let window {
            let pointer = convert(window.mouseLocationOutsideOfEventStream, from: nil)
            if visibleRect.contains(pointer) {
                point = pointer
            }
        }
        controller?.paste(copies: 1, at: point)
    }

    @objc func delete(_ sender: Any?) {
        deleteSelection()
    }

    // MARK: For tests and measuring

    /// A filter drag from code, the same path the mouse takes: `steps` moves, then the release. Returns the time of each move and of the commit.
    func measureDrag(steps: Int) -> (moves: [Double], redraw: Double)? {
        let filters = scene.ids.indices.filter { scene.isFilter($0) && !scene.isHidden($0) }
        guard let first = filters.first else { return nil }
        selectedShapes = [scene.ids[first]]
        selectedLines = []
        let start = CGPoint(x: scene.rects[first].midX, y: scene.rects[first].midY)
        beginMove(start: start, shapes: [first])
        displayIfNeeded()
        var times: [Double] = []
        for i in 1...steps {
            let t0 = CACurrentMediaTime()
            updateDrag(to: CGPoint(x: start.x + CGFloat(i), y: start.y + CGFloat(i) / 2))
            displayIfNeeded()
            CATransaction.flush()
            times.append(CACurrentMediaTime() - t0)
        }
        let t0 = CACurrentMediaTime()
        finishGesture(at: CGPoint(x: start.x + CGFloat(steps), y: start.y), event: NSEvent())
        displayIfNeeded()
        return (times, CACurrentMediaTime() - t0)
    }

    /// A connection drag from code, measuring each hover, from a shape's output to wherever `targets` are.
    func measureConnectHover(steps: Int) -> [Double]? {
        guard let source = scene.ids.indices.first(where: { scene.outputs[$0] > 0 && !scene.isHidden($0) }) else { return nil }
        beginConnect(source: source, output: 0)
        var times: [Double] = []
        let content = scene.contentSize
        for i in 0..<steps {
            let t0 = CACurrentMediaTime()
            updateDrag(to: CGPoint(x: content.width * CGFloat(i) / CGFloat(steps), y: content.height * CGFloat(i) / CGFloat(steps)))
            displayIfNeeded()
            CATransaction.flush()
            times.append(CACurrentMediaTime() - t0)
        }
        gesture = .none
        overlayLines.isHidden = true
        overlayPort.isHidden = true
        return times
    }

    /// A visible shape by its title and which of that title it is, for scripts.
    func shapeIndex(titled title: String, occurrence: Int = 0) -> Int? {
        scene.ids.indices.filter { scene.titles[$0] == title && !scene.isHidden($0) }.dropFirst(occurrence).first
    }

    func shapeCentre(_ i: Int) -> CGPoint { CGPoint(x: scene.rects[i].midX, y: scene.rects[i].midY) }

    func shapeRect(_ i: Int) -> CGRect { scene.rects[i] }

    func topShapeIndex(at p: CGPoint) -> Int? { topShape(at: p) }

    var lineCount: Int { scene.lineIDs.count }

    func lineMidpoint(_ i: Int) -> CGPoint {
        CGPoint(x: (scene.lineStarts[i].x + scene.lineEnds[i].x) / 2, y: (scene.lineStarts[i].y + scene.lineEnds[i].y) / 2)
    }

    func outputPoint(shape i: Int, output: Int) -> CGPoint { portPoint(shape: i, output: output) }

    func inputPointOf(shape i: Int) -> CGPoint { inputPoint(shape: i) }

    /// A mouse event with the real path behind it, from canvas coordinates.
    func sendMouse(_ type: NSEvent.EventType, at p: CGPoint, flags: NSEvent.ModifierFlags = [], clicks: Int = 1) {
        guard let window, let event = NSEvent.mouseEvent(
            with: type, location: convert(p, to: nil), modifierFlags: flags, timestamp: ProcessInfo.processInfo.systemUptime,
            windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: clicks, pressure: 1
        ) else { return }
        switch type {
        case .leftMouseDown: mouseDown(with: event)
        case .leftMouseDragged: mouseDragged(with: event)
        case .leftMouseUp: mouseUp(with: event)
        case .rightMouseDown: rightMouseDown(with: event)
        case .rightMouseDragged: rightMouseDragged(with: event)
        case .mouseMoved: mouseMoved(with: event)
        default: break
        }
    }

    /// The right button's release without its menu, which would hold a script up. A drag finishes as usual.
    func releaseSecondary(at p: CGPoint) -> Bool {
        if case .secondary = gesture {
            gesture = .none
            return true
        }
        finishGesture(at: p, event: NSEvent())
        return false
    }

    /// What's selected and what's going on, for scripts.
    var stateText: String {
        let shapes = scene.ids.indices.filter { selectedShapes.contains(scene.ids[$0]) }.map { scene.titles[$0] }
        let lines = scene.lineIDs.indices.filter { selectedLines.contains(scene.lineIDs[$0]) }.map { i -> String in
            let from = scene.lineFrom[i] >= 0 ? scene.titles[Int(scene.lineFrom[i])] : "stub"
            let to = scene.lineTo[i] >= 0 ? scene.titles[Int(scene.lineTo[i])] : "stub"
            return "\(from)>\(to)"
        }
        let name: String
        switch gesture {
        case .none: name = "idle"
        case .pressed: name = "pressed"
        case .frame: name = "frame"
        case .moving: name = "moving"
        case .connecting(_, _, _, let target): name = target.map { "connecting to \(scene.titles[$0])" } ?? "connecting"
        case .secondary: name = "secondary"
        }
        return "shapes [\(shapes.joined(separator: "|"))] lines [\(lines.joined(separator: "|"))] gesture \(name) cursor \(cursorName) overlays \(overlayLines.isHidden ? "-" : "L")\(overlayShapes.isHidden ? "-" : "S")\(overlayFrame.isHidden ? "-" : "F")\(overlayPort.isHidden ? "-" : "P")"
    }

    /// Everything on the canvas, one per line, for scripts.
    var sceneText: String {
        let shapes = scene.ids.indices.map { i -> String in
            let r = scene.rects[i]
            return String(format: "%d %@ %@ %.0f,%.0f %.0fx%.0f in%d out%d%@", scene.kinds[i], scene.titles[i], scene.isFilter(i) ? "filter" : "-", r.minX, r.minY, r.width, r.height, scene.inputs[i], scene.outputs[i], scene.isHidden(i) ? " hidden" : "")
        }
        let lines = scene.lineIDs.indices.map { i -> String in
            let from = scene.lineFrom[i] >= 0 ? scene.titles[Int(scene.lineFrom[i])] : "stub"
            let to = scene.lineTo[i] >= 0 ? scene.titles[Int(scene.lineTo[i])] : "stub"
            return "\(from)[\(scene.lineFromPorts[i])]>\(to) kind\(scene.lineKinds[i])"
        }
        return (shapes + lines).joined(separator: "\n")
    }

    /// The whole visible area drawn again, for measuring.
    /// The visible area drawn into a bitmap, start to finish on this thread, which is what a full redraw costs here. At low zoom the bitmap is bigger than the screen area,
    /// as it's in canvas units, so this is the upper bound.
    func measureVisibleDraw() -> Double {
        let rect = visibleRect
        guard rect.width > 0, rect.height > 0, let rep = bitmapImageRepForCachingDisplay(in: rect) else { return -1 }
        let t0 = CACurrentMediaTime()
        cacheDisplay(in: rect, to: rep)
        return CACurrentMediaTime() - t0
    }

    /// The visible area drawn again now, and how long the drawing took. -1 if nothing drew.
    func measureFullRedraw() -> Double {
        let before = drawCount
        setNeedsDisplay(visibleRect)
        layer?.setNeedsDisplay()
        layer?.displayIfNeeded()
        displayIfNeeded()
        return drawCount > before ? lastDrawDuration : -1
    }

    /// Shapes and lines in the scene, for scripts.
    var counts: (shapes: Int, lines: Int) { (scene.ids.count, scene.lineIDs.count) }

    func rectOfShape(titled title: String) -> CGRect? {
        scene.titles.firstIndex(of: title).map { scene.rects[$0] }
    }
}

/// Hosts the canvas's scroll view, which lives as long as the window so the view keeps its state while hidden (F3).
private struct GraphCanvasHost: NSViewRepresentable {
    let controller: GraphCanvasController

    func makeNSView(context: Context) -> GraphScrollView {
        controller.scrollView
    }

    func updateNSView(_ view: GraphScrollView, context: Context) {}
}

/// F111: the canvas with its button row beneath.
struct GraphicalPatchingView: View {
    let controller: GraphCanvasController

    var body: some View {
        VStack(spacing: 6) {
            GraphCanvasHost(controller: controller)
                .clipShape(RoundedRectangle(cornerRadius: 4))
            HStack(spacing: 6) {
                SetupIconButton(symbol: "plus", label: "Add Filter") {
                    controller.addFilter()
                }
                SetupIconButton(symbol: "trash", label: "Delete Filter", enabled: controller.toolbar.canDeleteFilter) {
                    controller.deleteSelected()
                }
                Spacer().frame(width: 10)
                SetupIconButton(symbol: "plus.magnifyingglass", label: "Zoom In") {
                    controller.zoom(in: true)
                }
                SetupIconButton(symbol: "minus.magnifyingglass", label: "Zoom Out") {
                    controller.zoom(in: false)
                }
                SetupIconButton(symbol: "arrow.up.left.and.down.right.magnifyingglass", label: "Zoom to Fit") {
                    controller.zoomToFit()
                }
                Spacer(minLength: 0)
                Text("\(controller.toolbar.zoomPercent)%")
                    .font(.callout)
                    .monospacedDigit()
                    .foregroundStyle(.secondary)
            }
        }
        .padding(8)
    }
}
