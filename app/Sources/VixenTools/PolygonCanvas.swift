import AppKit
import SwiftUI
import VixenBridge

struct PolygonCanvas: NSViewRepresentable {
    let model: PolygonModel
    func makeNSView(context: Context) -> PolygonCanvasView { PolygonCanvasView(model: model) }
    func updateNSView(_ nsView: PolygonCanvasView, context: Context) { nsView.state = model.state; nsView.needsDisplay = true }
}
final class PolygonCanvasView: NSView {
    let model: PolygonModel
    var state: PolygonEdState
    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
    init(model: PolygonModel) { self.model = model; state = model.state; super.init(frame: .zero); setAccessibilityLabel("Polygon canvas") }
    required init?(coder: NSCoder) { nil }
    private var scale: CGFloat { max(0.001, min((bounds.width - 60) / state.width, (bounds.height - 60) / state.height)) }
    private var origin: CGPoint { CGPoint(x: (bounds.width - state.width * scale) / 2, y: (bounds.height - state.height * scale) / 2) }
    private func location(_ event: NSEvent) -> CGPoint { let p = convert(event.locationInWindow, from: nil); return CGPoint(x: (p.x - origin.x) / scale, y: (p.y - origin.y) / scale) }
    func testDrag(from: CGPoint, to: CGPoint, shift: Bool) {
        let start = CGPoint(x: origin.x + from.x * scale, y: origin.y + from.y * scale)
        let end = CGPoint(x: origin.x + to.x * scale, y: origin.y + to.y * scale)
        postPolygonDrag(view: self, from: start, to: end, shift: shift)
    }
    override func updateTrackingAreas() { super.updateTrackingAreas(); for area in trackingAreas { removeTrackingArea(area) }; addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseMoved, .mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect], owner: self, userInfo: nil)) }
    override func mouseDown(with event: NSEvent) { window?.makeFirstResponder(self); let p = location(event); model.track { $0.pointerDown(x: p.x, y: p.y) } } // STUDIO: the live editor writes at mouse up
    override func mouseDragged(with event: NSEvent) { let p = location(event); model.track { $0.pointerMove(x: p.x, y: p.y, dragging: true, shift: event.modifierFlags.contains(.shift)) } } // STUDIO: a drag is a ghost until mouse up
    override func mouseUp(with event: NSEvent) { let p = location(event); model.act { $0.pointerUp(x: p.x, y: p.y, shift: event.modifierFlags.contains(.shift)) } }
    override func mouseMoved(with event: NSEvent) { let p = location(event); model.track { $0.pointerMove(x: p.x, y: p.y, dragging: false, shift: false) }; switch model.editor.cursor(x: p.x, y: p.y) { case "cross": NSCursor.crosshair.set(); case "move": NSCursor.openHand.set(); case "add": NSCursor.dragCopy.set(); default: NSCursor.arrow.set() } } // STUDIO: hovering writes nothing
    override func mouseExited(with event: NSEvent) { model.track { $0.pointerLeave() }; NSCursor.arrow.set() } // STUDIO: hovering writes nothing
    override func keyDown(with event: NSEvent) { if event.keyCode == 51 || event.keyCode == 117 { model.act { $0.delete() } } else { super.keyDown(with: event) } }
    @objc func copy(_ sender: Any?) { model.copy() }
    @objc func cut(_ sender: Any?) { model.copy(cut: true) }
    @objc func paste(_ sender: Any?) { model.paste() }
    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        if event.modifierFlags.contains(.command) { switch event.charactersIgnoringModifiers { case "c": model.copy(); return true; case "x": model.copy(cut: true); return true; case "v": model.paste(); return true; default: break } }
        return super.performKeyEquivalent(with: event)
    }
    override func draw(_ dirtyRect: NSRect) {
        NSColor.windowBackgroundColor.setFill(); bounds.fill()
        NSGraphicsContext.saveGraphicsState()
        let transform = NSAffineTransform(); transform.translateX(by: origin.x, yBy: origin.y); transform.scale(by: scale); transform.concat()
        let area = NSRect(x: 0, y: 0, width: state.width, height: state.height)
        NSColor(white: 68 / 255, alpha: 1).setFill(); area.fill()
        let blue = NSColor(red: 30 / 255, green: 144 / 255, blue: 1, alpha: 1)
        let pink = NSColor(srgbRed: 1, green: 105 / 255, blue: 180 / 255, alpha: 1)
        func path(_ points: [PolygonEdPoint], close: Bool = false) -> NSBezierPath {
            let path = NSBezierPath(); if let first = points.first { path.move(to: NSPoint(x: first.x, y: first.y)); for p in points.dropFirst() { path.line(to: NSPoint(x: p.x, y: p.y)) }; if close { path.close() } }; return path
        }
        let grid = NSBezierPath(); grid.lineWidth = 1 / scale; grid.setLineDash([3, 3], count: 2, phase: 0)
        let spacing = max(state.width, state.height) / 10
        for x in stride(from: 0.0, through: state.width, by: spacing) { grid.move(to: NSPoint(x: x, y: 0)); grid.line(to: NSPoint(x: x, y: state.height)) }
        for y in stride(from: 0.0, through: state.height, by: spacing) { grid.move(to: NSPoint(x: 0, y: y)); grid.line(to: NSPoint(x: state.width, y: y)) }
        NSColor(srgbRed: 211 / 255, green: 211 / 255, blue: 211 / 255, alpha: 1).setStroke(); grid.stroke(); let edges = NSBezierPath(rect: area); edges.lineWidth = 1 / scale; edges.stroke()
        let ticks = NSBezierPath(); ticks.lineWidth = 1 / scale
        for x in stride(from: 0.0, through: state.width, by: spacing / 5) { ticks.move(to: NSPoint(x: x, y: 0)); ticks.line(to: NSPoint(x: x, y: 10)); ticks.move(to: NSPoint(x: x, y: state.height)); ticks.line(to: NSPoint(x: x, y: state.height - 10)) }
        for y in stride(from: 0.0, through: state.height, by: spacing / 5) { ticks.move(to: NSPoint(x: 0, y: y)); ticks.line(to: NSPoint(x: 10, y: y)); ticks.move(to: NSPoint(x: state.width, y: y)); ticks.line(to: NSPoint(x: state.width - 10, y: y)) }; ticks.stroke()
        if state.showDisplay { NSColor.systemRed.setStroke(); let p = NSBezierPath(rect: NSRect(x: state.displayX, y: state.displayY, width: state.displayWidth, height: state.displayHeight)); p.lineWidth = 1; p.stroke() }
        for shape in state.shapes {
            let outline: NSBezierPath
            if shape.kind == "Ellipse" {
                outline = NSBezierPath(ovalIn: NSRect(x: shape.center.x - shape.width / 2, y: shape.center.y - shape.height / 2, width: shape.width, height: shape.height))
                let rotate = NSAffineTransform(); rotate.translateX(by: shape.center.x, yBy: shape.center.y); rotate.rotate(byDegrees: shape.angle); rotate.translateX(by: -shape.center.x, yBy: -shape.center.y); outline.transform(using: rotate as AffineTransform)
                let box = path(shape.points, close: true); box.lineWidth = 1; box.setLineDash([1, 1], count: 2, phase: 0); blue.setStroke(); box.stroke()
            } else { outline = path(shape.points, close: shape.kind == "Polygon") }
            if shape.kind != "Line" { blue.withAlphaComponent(0.5).setFill(); outline.fill() }; blue.setStroke(); outline.lineWidth = 3; outline.stroke()
            if !state.timeMode && shape.fill == "Wipe" && shape.points.count >= 2 && (shape.kind != "Polygon" || shape.points.count == 3 || shape.points.count == 4) {
                NSColor.green.setStroke(); let side = path(Array(shape.points.prefix(2))); side.lineWidth = 3; if shape.kind != "Line" { side.stroke() }
            }
            for (index, point) in shape.points.enumerated() {
                var color = shape.selectedPoints.contains(UInt32(index)) ? pink : blue
                if !state.timeMode && shape.kind == "Line" && shape.fill == "Wipe" && index == 0 && !shape.selectedPoints.contains(UInt32(index)) { color = NSColor(srgbRed: 0, green: shape.startArgb == 0xFF00FF00 ? 1 : 128 / 255, blue: 0, alpha: 1) }
                color.setFill(); NSRect(x: point.x - 3.5, y: point.y - 3.5, width: 7, height: 7).fill()
                if state.showLabels { ("\(index + 1)" as NSString).draw(at: NSPoint(x: point.x + 5, y: point.y + 5), withAttributes: [.font: NSFont.systemFont(ofSize: 12), .foregroundColor: NSColor.white]) }
            }
            (shape.selected ? pink : .black).setStroke(); let cross = NSBezierPath(); cross.move(to: NSPoint(x: shape.center.x - 20, y: shape.center.y)); cross.line(to: NSPoint(x: shape.center.x + 20, y: shape.center.y)); cross.move(to: NSPoint(x: shape.center.x, y: shape.center.y - 20)); cross.line(to: NSPoint(x: shape.center.x, y: shape.center.y + 20)); cross.lineWidth = 1; cross.stroke()
            if shape.kind == "Line" { NSBezierPath(ovalIn: NSRect(x: shape.center.x - 20, y: shape.center.y - 20, width: 40, height: 40)).stroke() }
            if state.showLabels && shape.kind != "Line" { (shape.label as NSString).draw(at: NSPoint(x: shape.center.x + 5, y: shape.center.y + 5), withAttributes: [.font: NSFont.systemFont(ofSize: 24), .foregroundColor: NSColor.white]) }
        }
        if !state.partial.isEmpty { blue.setStroke(); let p = path(state.partial); p.lineWidth = 2; p.stroke(); for p in state.partial { blue.setFill(); NSRect(x: p.x - 3, y: p.y - 3, width: 6, height: 6).fill() } }
        if let ghost = state.ghost {
            let line = NSBezierPath(); if let last = state.partial.last { line.move(to: NSPoint(x: last.x, y: last.y)); line.line(to: NSPoint(x: ghost.x, y: ghost.y)) }
            if state.partial.isEmpty, let shape = state.shapes.first(where: { $0.selectedPoints.count == 1 && $0.kind != "Ellipse" }), let index = shape.selectedPoints.first {
                let i = Int(index), n = shape.points.count
                if n > 1 {
                    let neighbors = shape.kind == "Line" ? [i == 0 ? 1 : 0] : [(i + n - 1) % n, (i + 1) % n]
                    for j in neighbors { let q = shape.points[j]; line.move(to: NSPoint(x: q.x, y: q.y)); line.line(to: NSPoint(x: ghost.x, y: ghost.y)) }
                }
            }
            line.lineWidth = 1; line.setLineDash([2, 2], count: 2, phase: 0); blue.setStroke(); line.stroke(); NSColor(srgbRed: 211 / 255, green: 211 / 255, blue: 211 / 255, alpha: 1).setFill(); NSRect(x: ghost.x - 3, y: ghost.y - 3, width: 6, height: 6).fill()
        }
        if state.band.count == 2 { let a = state.band[0], b = state.band[1]; let box = NSRect(x: min(a.x, b.x), y: min(a.y, b.y), width: abs(a.x - b.x), height: abs(a.y - b.y)); NSColor.selectedContentBackgroundColor.withAlphaComponent(0.2).setFill(); box.fill(); NSColor.selectedContentBackgroundColor.setStroke(); NSBezierPath(rect: box).stroke() }
        if state.handles.count == 10 {
            let border = path(Array(state.handles.prefix(8)), close: true); NSColor.white.setStroke(); border.lineWidth = 1; border.stroke()
            for p in state.handles { let box = NSBezierPath(rect: NSRect(x: p.x - 5, y: p.y - 5, width: 10, height: 10)); NSColor.windowBackgroundColor.setFill(); box.fill(); NSColor.white.setStroke(); box.stroke() }
        }
        NSGraphicsContext.restoreGraphicsState()
    }
}

struct PolygonPointTable: NSViewRepresentable {
    let model: PolygonModel
    func makeCoordinator() -> Coordinator { Coordinator(model) }
    func makeNSView(context: Context) -> NSScrollView {
        let table = NSTableView(); table.style = .fullWidth; table.columnAutoresizingStyle = .uniformColumnAutoresizingStyle; table.headerView = NSTableHeaderView(); table.rowHeight = 23; table.usesAlternatingRowBackgroundColors = true; table.allowsMultipleSelection = true
        for (id, width) in [("#", 30.0), ("X", 55.0), ("Y", 55.0)] { let c = NSTableColumn(identifier: NSUserInterfaceItemIdentifier(id)); c.title = id; c.minWidth = id == "#" ? 26 : 45; c.width = width; c.isEditable = id != "#"; table.addTableColumn(c) }
        table.dataSource = context.coordinator; table.delegate = context.coordinator; context.coordinator.table = table
        let scroll = NSScrollView(); scroll.documentView = table; scroll.hasVerticalScroller = true; scroll.borderType = .bezelBorder; return scroll
    }
    func updateNSView(_ nsView: NSScrollView, context: Context) { context.coordinator.rows = model.state.rows; context.coordinator.table?.reloadData() }
    final class Coordinator: NSObject, NSTableViewDataSource, NSTableViewDelegate {
        let model: PolygonModel; var rows: [PolygonEdRow]; weak var table: NSTableView?
        init(_ model: PolygonModel) { self.model = model; rows = model.state.rows }
        func numberOfRows(in tableView: NSTableView) -> Int { rows.count }
        func tableView(_ tableView: NSTableView, objectValueFor tableColumn: NSTableColumn?, row: Int) -> Any? { guard rows.indices.contains(row) else { return nil }; let r = rows[row]; switch tableColumn?.identifier.rawValue { case "#": return String(r.label); case "X": return String(format: "%g", r.x); default: return String(format: "%g", r.y) } }
        func tableView(_ tableView: NSTableView, shouldEdit tableColumn: NSTableColumn?, row: Int) -> Bool { rows.indices.contains(row) && rows[row].editable && tableColumn?.identifier.rawValue != "#" }
        func tableView(_ tableView: NSTableView, setObjectValue object: Any?, for tableColumn: NSTableColumn?, row: Int) { model.editPoint(row, axis: tableColumn?.identifier.rawValue ?? "X", value: String(describing: object ?? "")) }
    }
}

struct PolygonTimeBar: NSViewRepresentable {
    let model: PolygonModel
    func makeNSView(context: Context) -> PolygonTimeView { PolygonTimeView(model: model) }
    func updateNSView(_ nsView: PolygonTimeView, context: Context) { nsView.needsDisplay = true }
}
final class PolygonTimeView: NSView {
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
    let model: PolygonModel; var dragging = false
    override var isFlipped: Bool { true }
    init(model: PolygonModel) { self.model = model; super.init(frame: .zero); setAccessibilityLabel("Shape timeline") }
    required init?(coder: NSCoder) { nil }
    func testMoveFrame(to time: Double) {
        let state = model.state; guard state.frames.indices.contains(Int(state.selectedFrame)) else { return }
        let x = (state.frames[Int(state.selectedFrame)].time * max(1, bounds.width - 10)).rounded(.towardZero) + 4
        postPolygonDrag(view: self, from: CGPoint(x: x, y: 10), to: CGPoint(x: 4 + time * max(1, bounds.width - 10), y: 10), shift: false)
    }
    override func setFrameSize(_ newSize: NSSize) { super.setFrameSize(newSize); model.timeBarWidth = newSize.width }
    override func updateTrackingAreas() { super.updateTrackingAreas(); for a in trackingAreas { removeTrackingArea(a) }; addTrackingArea(NSTrackingArea(rect: .zero, options: [.activeAlways, .inVisibleRect, .mouseMoved, .mouseEnteredAndExited], owner: self, userInfo: nil)) }
    override func mouseMoved(with event: NSEvent) { let x = convert(event.locationInWindow, from: nil).x; if model.state.frames.contains(where: { abs(($0.time * max(1, bounds.width - 10)).rounded(.towardZero) + 4 - x) <= 4 }) { NSCursor.resizeLeftRight.set() } else { NSCursor.arrow.set() } }
    override func mouseExited(with event: NSEvent) { dragging = false; NSCursor.arrow.set() }
    override func draw(_ dirtyRect: NSRect) {
        NSColor.gray.setFill(); bounds.fill(); let ticks = NSBezierPath(); ticks.lineWidth = 1
        for i in 0...100 { let x = 5 + (bounds.width - 10) * Double(i) / 100; ticks.move(to: NSPoint(x: x, y: bounds.height)); ticks.line(to: NSPoint(x: x, y: bounds.height - (i % 10 == 0 ? 15 : 10))) }; NSColor.black.setStroke(); ticks.stroke()
        for f in model.state.frames { let x = (f.time * max(1, bounds.width - 10)).rounded(.towardZero) + 4; let marker = NSBezierPath(); marker.move(to: NSPoint(x: x - 4, y: 0)); marker.line(to: NSPoint(x: x + 4, y: 0)); marker.line(to: NSPoint(x: x + 4, y: 20)); marker.line(to: NSPoint(x: x, y: 25)); marker.line(to: NSPoint(x: x - 4, y: 20)); marker.close(); (f.selected ? NSColor(srgbRed: 1, green: 105 / 255, blue: 180 / 255, alpha: 1) : NSColor(srgbRed: 68 / 255, green: 68 / 255, blue: 68 / 255, alpha: 1)).setFill(); marker.fill() }
    }
    override func mouseDown(with event: NSEvent) { let p = convert(event.locationInWindow, from: nil); if let i = model.state.frames.firstIndex(where: { abs((($0.time * max(1, bounds.width - 10)).rounded(.towardZero) + 4) - p.x) <= 4 }) { model.act { $0.selectFrame(index: UInt32(i)) }; dragging = true } }
    override func mouseDragged(with event: NSEvent) { guard dragging else { return }; let p = convert(event.locationInWindow, from: nil); model.act { $0.moveFrameAt(position: p.x, width: bounds.width) } }
    override func mouseUp(with event: NSEvent) { mouseDragged(with: event); dragging = false }
}

/// Uses normal AppKit events in the offscreen harness; coordinates are converted through the real view.
private func postPolygonDrag(view: NSView, from: CGPoint, to: CGPoint, shift: Bool) {
    guard let window = view.window else { return }
    window.makeKey()
    func send(_ type: NSEvent.EventType, _ point: CGPoint) {
        if let event = NSEvent.mouseEvent(with: type, location: view.convert(point, to: nil), modifierFlags: shift ? [.shift] : [], timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: 1) { NSApp.postEvent(event, atStart: false) }
    }
    send(.leftMouseDown, from)
    if from != to { for i in 1...8 { let t = Double(i) / 8; send(.leftMouseDragged, CGPoint(x: from.x + (to.x - from.x) * t, y: from.y + (to.y - from.y) * t)) } }
    send(.leftMouseUp, to)
}
