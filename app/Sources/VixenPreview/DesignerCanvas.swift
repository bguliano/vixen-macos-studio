import AppKit
import QuartzCore
import SwiftUI
import VixenBridge
import VixenKit

/// Cursors the system doesn't have. Upstream's two come from their `.ico`s, and the four-way arrow is drawn.
@MainActor
enum DesignerCursors {
    private static func image(_ name: String) -> NSImage? {
        guard let url = Bundle.main.url(forResource: name, withExtension: "png", subdirectory: "preview") else { return nil }
        return NSImage(contentsOf: url)
    }

    static let rotate: NSCursor = image("cursorRotate").map { NSCursor(image: $0, hotSpot: NSPoint(x: 15, y: 15)) } ?? .crosshair
    static let insert: NSCursor = image("cursorInsert").map { NSCursor(image: $0, hotSpot: NSPoint(x: 15, y: 15)) } ?? .crosshair
    static let sizeAll: NSCursor = {
        let image = NSImage(size: NSSize(width: 20, height: 20), flipped: false) { rect in
            let path = NSBezierPath()
            let (c, a, h): (CGFloat, CGFloat, CGFloat) = (10, 9, 3)
            path.move(to: NSPoint(x: c, y: c - a))
            path.line(to: NSPoint(x: c, y: c + a))
            path.move(to: NSPoint(x: c - a, y: c))
            path.line(to: NSPoint(x: c + a, y: c))
            for (x, y, dx, dy) in [(c, c + a, 1.0, 0.0), (c, c - a, 1.0, 0.0), (c + a, c, 0.0, 1.0), (c - a, c, 0.0, 1.0)] {
                let sx: CGFloat = y > c || x > c ? -1 : 1
                path.move(to: NSPoint(x: x, y: y))
                path.line(to: NSPoint(x: x + (dx == 1 ? -h : sx * h), y: y + (dy == 1 ? -h : sx * h)))
                path.move(to: NSPoint(x: x, y: y))
                path.line(to: NSPoint(x: x + (dx == 1 ? h : sx * h), y: y + (dy == 1 ? h : sx * h)))
            }
            NSColor.black.setStroke()
            path.lineWidth = 3.5
            path.stroke()
            NSColor.white.setStroke()
            path.lineWidth = 1.5
            path.stroke()
            return true
        }
        return NSCursor(image: image, hotSpot: NSPoint(x: 10, y: 10))
    }()

    static func cursor(_ c: DesignerCursor) -> NSCursor {
        switch c {
        case .default: .arrow
        case .cross: .crosshair
        case .sizeAll: sizeAll
        case .rotate: rotate
        case .insertMode: insert
        }
    }
}

/// What a scroll view scrolls: nothing but its own size. The Metal view is a sibling behind it, sized to the view, so the layer never scrolls or re-rasterizes.
private final class CanvasDocument: NSView {
    override var isFlipped: Bool { true }
    override func draw(_ dirtyRect: NSRect) {}
}

/// Clicks go through to the canvas, except on a scroller.
private final class CanvasScrollView: NSScrollView {
    override func hitTest(_ point: NSPoint) -> NSView? {
        let hit = super.hitTest(point)
        return hit is NSScroller ? hit : superview
    }
}

/// Text for Prop Information, drawn above the renderer's layer. Reused by index, never one view per label.
private final class LabelsView: NSView {
    override var isFlipped: Bool { true }
    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    private var boxes: [CATextLayer] = []

    func show(_ labels: [DesignerLabel], zoom: CGFloat, scroll: NSPoint, scale: CGFloat) {
        guard let root = layer else { return }
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        while boxes.count < labels.count {
            let box = CATextLayer()
            box.fontSize = 7
            box.font = NSFont.systemFont(ofSize: 7)
            box.foregroundColor = NSColor.black.cgColor
            box.backgroundColor = NSColor.white.cgColor
            box.alignmentMode = .left
            root.addSublayer(box)
            boxes.append(box)
        }
        for (i, box) in boxes.enumerated() {
            guard i < labels.count else {
                box.isHidden = true
                continue
            }
            let label = labels[i]
            box.isHidden = false
            box.string = label.text
            box.contentsScale = scale
            // DrawInfo draws at the unzoomed Left and Top, so it drifts from the shape when zoomed. The label's numbers are in scene units with that zoom taken off, which brings it back to where upstream has it.
            box.frame = NSRect(x: CGFloat(label.x) * zoom - scroll.x, y: CGFloat(label.y) * zoom - scroll.y, width: 8 * CGFloat(label.text.count) * 0.55 + 4, height: 10)
        }
        CATransaction.commit()
    }
}

/// The design surface. Every event is one FFI call, which updates the session and tells the renderer what moved. All Swift does with light data is nothing.
final class DesignerCanvasView: NSView, NSMenuItemValidation {
    let metal = MetalLayerNSView(frame: .zero)
    weak var model: PreviewDesignerModel?
    private let labelsView = LabelsView()
    private let scroll = CanvasScrollView()
    private let document = CanvasDocument()
    private var applyingScroll = false
    private var tracking: NSTrackingArea?
    private var currentCursor: DesignerCursor = .default
    private var labels: [DesignerLabel] = []
    private var lastPoint = (x: Int32(0), y: Int32(0))

    /// Called once the view has a window and so a final layer.
    var onReady: ((DesignerCanvasView, NSWindow) -> Void)?

    override init(frame: NSRect) {
        super.init(frame: frame)
        metal.autoresizingMask = [.width, .height]
        metal.frame = bounds
        addSubview(metal)

        labelsView.wantsLayer = true
        labelsView.autoresizingMask = [.width, .height]
        labelsView.frame = bounds
        addSubview(labelsView)

        scroll.frame = bounds
        scroll.autoresizingMask = [.width, .height]
        scroll.hasVerticalScroller = true
        scroll.hasHorizontalScroller = true
        scroll.autohidesScrollers = true
        scroll.scrollerStyle = .overlay
        scroll.drawsBackground = false
        scroll.contentView.drawsBackground = false
        scroll.verticalScrollElasticity = .none
        scroll.horizontalScrollElasticity = .none
        scroll.documentView = document
        scroll.contentView.postsBoundsChangedNotifications = true
        addSubview(scroll)
        NotificationCenter.default.addObserver(self, selector: #selector(clipBoundsChanged), name: NSView.boundsDidChangeNotification, object: scroll.contentView)

        metal.onResize = { [weak self] width, height, scale in
            self?.drawableResized(width: width, height: height, scale: scale)
        }
        registerForDraggedTypes([.fileURL])
    }

    // MARK: Prop files dropped from the Finder, F57

    /// Upstream compares the ending with case; the Mac's file names don't care about it.
    private func propFiles(_ sender: any NSDraggingInfo) -> [String] {
        let urls = sender.draggingPasteboard.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL] ?? []
        return urls.filter { $0.pathExtension.lowercased() == "prp" }.map(\.path)
    }

    override func draggingEntered(_ sender: any NSDraggingInfo) -> NSDragOperation {
        propFiles(sender).isEmpty ? [] : .copy
    }

    override func performDragOperation(_ sender: any NSDraggingInfo) -> Bool {
        let files = propFiles(sender)
        guard !files.isEmpty, let model else { return false }
        let p = convert(sender.draggingLocation, from: nil)
        let x = Int32(p.x.rounded(.down))
        let y = Int32(p.y.rounded(.down))
        Task { await model.importPropFiles(files, x: x, y: y, atDrop: true) }
        return true
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }
    override var mouseDownCanMoveWindow: Bool { false }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        guard let window, let ready = onReady else { return }
        onReady = nil
        metal.metalLayer?.contentsScale = window.backingScaleFactor
        // After SwiftUI has laid the window out, so the first drawable has its real size.
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            ready(self, window)
        }
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let tracking {
            removeTrackingArea(tracking)
        }
        // F29, F32: moves with no button down carry a Multi-Point String between clicks, and pasted shapes on the pointer.
        let area = NSTrackingArea(rect: .zero, options: [.mouseMoved, .cursorUpdate, .inVisibleRect, .activeInKeyWindow], owner: self, userInfo: nil)
        addTrackingArea(area)
        tracking = area
    }

    // MARK: Coordinates

    private func point(_ event: NSEvent) -> (x: Int32, y: Int32) {
        let p = convert(event.locationInWindow, from: nil)
        let point = (x: Int32(p.x.rounded(.down)), y: Int32(p.y.rounded(.down)))
        lastPoint = point
        return point
    }

    private func mods(_ flags: NSEvent.ModifierFlags) -> DesignerMods {
        DesignerMods(command: flags.contains(.command), shift: flags.contains(.shift), option: flags.contains(.option))
    }

    func currentMods() -> DesignerMods {
        mods(NSEvent.modifierFlags)
    }

    /// Where the pointer is now, wherever it is: off the canvas it goes negative, as upstream's does.
    func pointerInCanvas() -> (x: Int32, y: Int32)? {
        guard let window else { return nil }
        let p = convert(window.mouseLocationOutsideOfEventStream, from: nil)
        return (Int32(p.x.rounded(.down)), Int32(p.y.rounded(.down)))
    }

    // MARK: Mouse

    override func mouseDown(with event: NSEvent) {
        studioAccessNoteClick() // STUDIO: no focus ring after a click (StudioDesignerAccess.swift)
        window?.makeFirstResponder(self)
        guard let model, let designer = model.designer else { return }
        // The Control key is the Mac's secondary click, and ⌘ stands in for upstream's Ctrl.
        if event.modifierFlags.contains(.control) {
            rightMouseDown(with: event)
            return
        }
        model.mouseIsDown = true
        let p = point(event)
        model.apply(designer.mouseDown(button: .left, x: p.x, y: p.y, mods: mods(event.modifierFlags)))
    }

    override func mouseDragged(with event: NSEvent) {
        guard let model, let designer = model.designer else { return }
        let p = point(event)
        model.apply(designer.mouseMove(x: p.x, y: p.y, mods: mods(event.modifierFlags)))
    }

    override func mouseUp(with event: NSEvent) {
        guard let model, let designer = model.designer else { return }
        let p = point(event)
        model.apply(designer.mouseUp(button: .left, x: p.x, y: p.y, mods: mods(event.modifierFlags)))
        model.mouseWentUp()
    }

    override func mouseMoved(with event: NSEvent) {
        guard let model, let designer = model.designer else { return }
        let p = point(event)
        model.apply(designer.mouseMove(x: p.x, y: p.y, mods: mods(event.modifierFlags)))
    }

    override func otherMouseDown(with event: NSEvent) {
        studioAccessNoteClick() // STUDIO: no focus ring after a middle click (StudioDesignerAccess.swift)
        guard let model, let designer = model.designer else { return }
        window?.makeFirstResponder(self)
        let p = point(event)
        model.apply(designer.mouseDown(button: .middle, x: p.x, y: p.y, mods: mods(event.modifierFlags)))
    }

    override func otherMouseDragged(with event: NSEvent) {
        guard let model, let designer = model.designer else { return }
        let p = point(event)
        model.apply(designer.mouseMove(x: p.x, y: p.y, mods: mods(event.modifierFlags)))
    }

    override func otherMouseUp(with event: NSEvent) {
        guard let model, let designer = model.designer else { return }
        let p = point(event)
        model.apply(designer.mouseUp(button: .middle, x: p.x, y: p.y, mods: mods(event.modifierFlags)))
    }

    /// F37: built after the click has selected. `popUpContextMenu` runs its own loop and swallows the right mouse up, so the session never sees one, and a shape mid-creation or
    /// pasted shapes on the pointer stay as they were.
    override func rightMouseDown(with event: NSEvent) {
        studioAccessNoteClick() // STUDIO: no focus ring after a right click (StudioDesignerAccess.swift)
        window?.makeFirstResponder(self)
        let p = point(event)
        guard let menu = contextMenu(atX: p.x, y: p.y, mods: mods(event.modifierFlags)) else { return }
        NSMenu.popUpContextMenu(menu, with: event, for: self)
    }

    /// The right press itself, which selects, and the menu it builds. Nil when the menu would be empty, which upstream doesn't show.
    func contextMenu(atX x: Int32, y: Int32, mods: DesignerMods) -> NSMenu? {
        guard let model, let designer = model.designer else { return nil }
        let change = designer.mouseDown(button: .right, x: x, y: y, mods: mods)
        model.apply(change)
        guard let items = change.contextMenu, !items.isEmpty else { return nil }
        return contextMenu(items)
    }

    /// F37's items with its separators, but none leading or trailing.
    func contextMenu(_ items: [DesignerMenuItem]) -> NSMenu {
        let menu = NSMenu()
        menu.autoenablesItems = false
        var previous: DesignerMenuItem?
        for item in items {
            let separated: Bool
            switch item {
            case .createTemplate: separated = true
            case .paste: separated = previous == .createTemplate || previous == .group
            case .undo: separated = true
            case .redo: separated = previous != .undo
            case .lock, .unlock: separated = true
            case .setZ: separated = true
            default: separated = false
            }
            if separated, !menu.items.isEmpty {
                menu.addItem(.separator())
            }
            previous = item
            if item == .setZ {
                let parent = NSMenuItem(title: "Set Z Location to", action: nil, keyEquivalent: "")
                let sub = NSMenu()
                let names = ["0 Front", "1", "2", "3", "4 Middle", "5", "6", "7", "8", "9 Back"]
                for (z, name) in names.enumerated() {
                    let entry = NSMenuItem(title: name, action: #selector(setZ(_:)), keyEquivalent: "")
                    entry.target = self
                    entry.tag = z
                    sub.addItem(entry)
                }
                parent.submenu = sub
                menu.addItem(parent)
                continue
            }
            let entry = NSMenuItem(title: Self.title(of: item), action: #selector(menuChosen(_:)), keyEquivalent: "")
            entry.target = self
            entry.representedObject = MenuChoice(item)
            entry.image = Self.symbol(of: item).flatMap { NSImage(systemSymbolName: $0, accessibilityDescription: nil) }
            menu.addItem(entry)
        }
        return menu
    }

    private final class MenuChoice {
        let item: DesignerMenuItem
        init(_ item: DesignerMenuItem) { self.item = item }
    }

    private static func title(of item: DesignerMenuItem) -> String {
        switch item {
        case .ungroup: "Ungroup"
        case .group: "Group"
        case .createTemplate: "Create Template..."
        case .cut: "Cut"
        case .copy: "Copy"
        case .paste: "Paste"
        case .delete: "Delete"
        case .undo: "Undo"
        case .redo: "Redo"
        case .lock: "Lock"
        case .unlock: "Unlock"
        case .setZ: "Set Z Location to"
        }
    }

    private static func symbol(of item: DesignerMenuItem) -> String? {
        switch item {
        case .cut: "scissors"
        case .copy: "doc.on.doc"
        case .paste: "doc.on.clipboard"
        case .delete: "trash"
        case .undo: "arrow.uturn.backward"
        case .redo: "arrow.uturn.forward"
        case .lock: "lock"
        case .unlock: "lock.open"
        default: nil
        }
    }

    @objc private func menuChosen(_ sender: NSMenuItem) {
        guard let choice = sender.representedObject as? MenuChoice else { return }
        model?.perform(choice.item)
    }

    /// F38: the pixels change, and the location write that would overwrite each element's Y isn't passed on until the owner decides (Q1).
    @objc private func setZ(_ sender: NSMenuItem) {
        model?.apply(model?.designer?.setZ(position: Int32(sender.tag)))
    }

    // MARK: Wheel and gestures, F52

    override func scrollWheel(with event: NSEvent) {
        guard let model, let designer = model.designer else { return }
        // A trackpad pans through the scroll view, and a real wheel zooms as upstream.
        if event.hasPreciseScrollingDeltas {
            scroll.scrollWheel(with: event)
            return
        }
        let sign: CGFloat = event.isDirectionInvertedFromDevice ? -1 : 1
        let delta = Int32((event.deltaY * sign * 120).rounded())
        guard delta != 0 else { return }
        let p = point(event)
        model.apply(designer.scrollWheel(delta: delta, x: p.x, y: p.y))
    }

    override func magnify(with event: NSEvent) {
        guard let model, let designer = model.designer else { return }
        let p = point(event)
        model.apply(designer.magnify(magnification: event.magnification, x: p.x, y: p.y))
    }

    // MARK: Keyboard, F27

    override func keyDown(with event: NSEvent) {
        if studioAccessKeyDown(event) { return } // STUDIO: R43's Tab between props (StudioDesignerAccess.swift)
        guard let model, let designer = model.designer else { return }
        let m = mods(event.modifierFlags)
        let key: DesignerKey?
        switch event.keyCode {
        case 51, 117: key = .delete
        case 53: key = .escape
        case 123: key = .left
        case 124: key = .right
        case 125: key = .down
        case 126: key = .up
        default: key = nil
        }
        if let key {
            model.apply(designer.keyDown(key: key, mods: m))
            return
        }
        if performZoomKey(event) {
            return
        }
        super.keyDown(with: event)
    }

    /// The session reads Control alone for these, and ⌘ stands in for it.
    private func performZoomKey(_ event: NSEvent) -> Bool {
        guard let model, let designer = model.designer, event.modifierFlags.contains(.command) else { return false }
        let only = DesignerMods(command: true, shift: false, option: false)
        switch event.charactersIgnoringModifiers {
        case "=", "+":
            model.apply(designer.keyDown(key: .plus, mods: only))
            return true
        case "-":
            model.apply(designer.keyDown(key: .minus, mods: only))
            return true
        default:
            return false
        }
    }

    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        guard window?.firstResponder === self, event.modifierFlags.contains(.command) else { return super.performKeyEquivalent(with: event) }
        // ⌘Y, upstream's Redo, alongside the Mac's ⇧⌘Z.
        if event.charactersIgnoringModifiers?.lowercased() == "y", !event.modifierFlags.contains(.shift) {
            model?.redo()
            return true
        }
        return performZoomKey(event) || super.performKeyEquivalent(with: event)
    }

    /// F104: the session reads modifiers from each event, so a change shows at once.
    override func flagsChanged(with event: NSEvent) {
        guard let model, let designer = model.designer, model.mouseIsDown else { return }
        model.apply(designer.mouseMove(x: lastPoint.x, y: lastPoint.y, mods: mods(event.modifierFlags)))
    }

    // MARK: Edit menu, F11, F12

    /// With the canvas focused the Edit menu acts on it, ahead of anything SwiftUI's views might claim.
    @objc func undo(_ sender: Any?) {
        model?.undo()
    }

    @objc func redo(_ sender: Any?) {
        model?.redo()
    }

    @objc func cut(_ sender: Any?) {
        model?.cut()
    }

    @objc func copy(_ sender: Any?) {
        model?.copy()
    }

    @objc func paste(_ sender: Any?) {
        model?.paste()
    }

    @objc func delete(_ sender: Any?) {
        model?.deleteSelection()
    }

    @objc override func selectAll(_ sender: Any?) {
        model?.selectAll()
    }

    // MARK: Cursor, F30

    func setCursor(_ cursor: DesignerCursor) {
        currentCursor = cursor
        DesignerCursors.cursor(cursor).set()
    }

    override func cursorUpdate(with event: NSEvent) {
        DesignerCursors.cursor(currentCursor).set()
    }

    // MARK: Scrolling, F53

    func setDocumentSize(width: Int32, height: Int32) {
        applyingScroll = true
        document.setFrameSize(NSSize(width: CGFloat(max(width, 1)), height: CGFloat(max(height, 1))))
        applyingScroll = false
    }

    /// From the session: zoom about the pointer, or middle-drag panning.
    func scrollTo(x: Int32, y: Int32) {
        applyingScroll = true
        scroll.contentView.scroll(to: NSPoint(x: CGFloat(x), y: CGFloat(y)))
        scroll.reflectScrolledClipView(scroll.contentView)
        applyingScroll = false
        layoutLabels()
    }

    @objc private func clipBoundsChanged() {
        guard !applyingScroll else { return }
        let origin = scroll.contentView.bounds.origin
        model?.designer?.setScroll(h: Int32(origin.x), v: Int32(origin.y))
        layoutLabels()
    }

    // MARK: Size

    private var sentSize = (0, 0)

    private func drawableResized(width: UInt32, height: UInt32, scale: Float) {
        guard let model, let designer = model.designer else { return }
        designer.viewResizing(widthPx: width, heightPx: height, scale: scale, widthPt: Int32(bounds.width), heightPt: Int32(bounds.height))
        // Hazard 20: during a live drag only the numbers. A resize that isn't one, such as a split divider or the zoom button, finishes now.
        if !inLiveResize {
            liveResizeEnded()
        }
    }

    func liveResizeEnded() {
        guard let model, let designer = model.designer else { return }
        let size = (Int(bounds.width), Int(bounds.height))
        guard size != sentSize else { return }
        sentSize = size
        model.apply(designer.viewResized(widthPt: Int32(bounds.width), heightPt: Int32(bounds.height)))
    }

    // MARK: Labels, F107

    func showLabels(_ labels: [DesignerLabel]) {
        self.labels = labels
        layoutLabels()
    }

    func layoutLabels() {
        guard !labels.isEmpty || !labelsView.isHidden else { return }
        labelsView.isHidden = labels.isEmpty
        labelsView.show(labels, zoom: CGFloat(model?.zoom ?? 1), scroll: scroll.contentView.bounds.origin, scale: window?.backingScaleFactor ?? 2)
    }

    /// F11: only undo, redo and the lock family are ever off.
    func validateMenuItem(_ item: NSMenuItem) -> Bool {
        guard let model else { return false }
        switch item.action {
        case #selector(undo(_:)):
            item.title = model.undoLabels.first.map { "Undo \($0.trimmingCharacters(in: .whitespaces))" } ?? "Undo"
            return model.toolbar.canUndo
        case #selector(redo(_:)):
            item.title = model.redoLabels.first.map { "Redo \($0.trimmingCharacters(in: .whitespaces))" } ?? "Redo"
            return model.toolbar.canRedo
        default:
            return true
        }
    }

    deinit {
        MainActor.assumeIsolated {
            NotificationCenter.default.removeObserver(self)
        }
    }
}

/// `DesignerCanvasView` for the split view's right side.
final class DesignerCanvasController: NSViewController {
    let canvas = DesignerCanvasView(frame: NSRect(x: 0, y: 0, width: 800, height: 500))

    override func loadView() {
        view = canvas
    }
}
