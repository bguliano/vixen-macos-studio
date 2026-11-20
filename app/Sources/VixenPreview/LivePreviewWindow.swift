import AppKit
import QuartzCore
import SwiftUI
import VixenBridge
import VixenKit

/// Window positions in the settings are top left and y down, as Windows has them. This is the one place that converts.
enum PreviewCoordinates {
    private static var top: CGFloat { NSScreen.screens.first?.frame.maxY ?? 0 }

    static func topLeft(ofFrame frame: NSRect) -> (x: Int32, y: Int32) {
        (Int32(frame.minX.rounded()), Int32((top - frame.maxY).rounded()))
    }

    /// An AppKit origin for a frame this tall, with its top left there.
    static func origin(x: Int32, y: Int32, height: CGFloat) -> NSPoint {
        NSPoint(x: CGFloat(x), y: top - CGFloat(y) - height)
    }

    static func workingAreas() -> [PreviewRect] {
        NSScreen.screens.map { screen in
            let f = screen.visibleFrame
            return PreviewRect(x: Int32(f.minX), y: Int32(top - f.maxY), width: Int32(f.width), height: Int32(f.height))
        }
    }
}

/// `CP_NOCLOSE_BUTTON` and a borderless window that still takes keys. Only the coordinator closes one of these, and ⌘W gets the alert.
final class LivePreviewNSWindow: NSWindow {
    var closeRequested: (() -> Void)?

    override var canBecomeKey: Bool { true }
    override var canBecomeMain: Bool { true }

    override func performClose(_ sender: Any?) {
        closeRequested?()
    }
}

/// The canvas: a Metal layer the renderer draws into, with Swift only forwarding events.
final class LiveCanvasView: NSView {
    let metal = MetalLayerNSView(frame: .zero)
    let legends = FixtureLegendView(frame: .zero)
    weak var controller: LivePreviewController?
    private var pressAt: NSPoint?
    private var dragging = false
    private var previous = NSPoint.zero
    private var wheelCarry: CGFloat = 0

    override init(frame: NSRect) {
        super.init(frame: frame)
        addSubview(metal)
        metal.frame = bounds
        metal.autoresizingMask = [.width, .height]
        addSubview(legends)
        legends.frame = bounds
        legends.autoresizingMask = [.width, .height]
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }
    override var mouseDownCanMoveWindow: Bool { false }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    private func screenPoint(_ event: NSEvent) -> NSPoint {
        event.window?.convertPoint(toScreen: event.locationInWindow) ?? event.locationInWindow
    }

    override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        guard let controller else { return }
        if event.modifierFlags.contains(.control) {
            controller.showMenu(event, in: self)
            return
        }
        previous = convert(event.locationInWindow, from: nil)
        pressAt = previous
        dragging = false
        if controller.isCamera {
            NSCursor.pointingHand.set()
        }
    }

    override func mouseDragged(with event: NSEvent) {
        guard let controller else { return }
        let point = convert(event.locationInWindow, from: nil)
        if controller.isCamera {
            // F67: a left drag pans. This form has no Lock Position and doesn't move its window.
            controller.live?.drag(dx: Float(previous.x - point.x), dy: Float(previous.y - point.y))
            previous = point
            return
        }
        // F65: past 2 px the window follows. `performDrag` tracks from here, so the rest of the drag never comes back to this view.
        guard let start = pressAt, !controller.flags.lockPosition else { return }
        if hypot(point.x - start.x, point.y - start.y) > 2 {
            pressAt = nil
            window?.performDrag(with: event)
        }
    }

    override func mouseUp(with event: NSEvent) {
        pressAt = nil
        if controller?.isCamera == true {
            NSCursor.arrow.set()
            controller?.saveSoon()
        }
    }

    override func rightMouseDown(with event: NSEvent) {
        controller?.showMenu(event, in: self)
    }

    override func scrollWheel(with event: NSEvent) {
        guard let controller, let live = controller.live else { return }
        // Away from the user zooms in, whichever way the system scrolls content.
        let sign: CGFloat = event.isDirectionInvertedFromDevice ? -1 : 1
        // A wheel's notch is 120. A trackpad's point is about a tenth of a notch, since this view has nowhere to pan.
        wheelCarry += event.scrollingDeltaY * sign * (event.hasPreciseScrollingDeltas ? 12 : 120)
        if abs(wheelCarry) >= 20 {
            let delta = Int32(wheelCarry)
            wheelCarry -= CGFloat(delta)
            live.wheel(delta: delta, shift: event.modifierFlags.contains(.shift))
            controller.distanceChanged()
            if controller.isCamera {
                controller.saveSoon()
            }
        }
    }

    override func magnify(with event: NSEvent) {
        guard let controller, let live = controller.live else { return }
        wheelCarry += event.magnification * 1000
        if abs(wheelCarry) >= 20 {
            let delta = Int32(wheelCarry)
            wheelCarry -= CGFloat(delta)
            live.wheel(delta: delta, shift: false)
            controller.distanceChanged()
            if controller.isCamera {
                controller.saveSoon()
            }
        }
    }

    override func keyDown(with event: NSEvent) {
        guard let controller, let live = controller.live else { return }
        switch event.charactersIgnoringModifiers?.lowercased() {
        case "i", "o":
            live.zoomKey(zoomIn: event.charactersIgnoringModifiers?.lowercased() == "i", shift: event.modifierFlags.contains(.shift))
            controller.distanceChanged()
            if controller.isCamera {
                controller.saveSoon()
            }
        default:
            // F72: everything else belongs to the active sequencer.
            NotificationCenter.default.post(name: .previewForwardedKey, object: event)
        }
    }
}

/// Text stays native while the fixture volumes remain in the GPU scene. Labels share the mesh's
/// perspective projection and never intercept canvas drags or context menus.
final class FixtureLegendView: NSView {
    private var labels: [LiveFixtureLegend] = []
    override var isFlipped: Bool { true }
    override func hitTest(_ point: NSPoint) -> NSView? { nil }
    func update(_ next: [LiveFixtureLegend]) {
        guard labels.count != next.count || zip(labels, next).contains(where: { pair in
            pair.0.text != pair.1.text || pair.0.x != pair.1.x || pair.0.y != pair.1.y || pair.0.fontSize != pair.1.fontSize
        }) else { return }
        labels = next
        needsDisplay = true
    }
    override func draw(_ dirtyRect: NSRect) {
        for label in labels where label.fontSize > 0 {
            let font = NSFont(name: "Courier", size: CGFloat(label.fontSize)) ?? NSFont.monospacedSystemFont(ofSize: CGFloat(label.fontSize), weight: .regular)
            (label.text as NSString).draw(at: NSPoint(x: CGFloat(label.x), y: CGFloat(label.y)),
                withAttributes: [.font: font, .foregroundColor: NSColor.red])
        }
    }
}

/// The thin bar at the bottom: `Lights`, then `Distance` in the camera view, then `FPS`.
final class LiveStatusBar: NSView {
    private let stack = NSStackView()
    private let lights = NSTextField(labelWithString: "Lights: 0")
    private let distance = NSTextField(labelWithString: "Distance 0.0")
    private let fps = NSTextField(labelWithString: "FPS: 0")
    private let camera: Bool
    // STUDIO-BEGIN preview-frame-rate: the frame rate is a diagnostic, shown only when asked for from the window's menu
    static let frameRateKey = "studio.preview.showFrameRate"
    var showsFrameRate = UserDefaults.standard.bool(forKey: LiveStatusBar.frameRateKey) {
        didSet { fps.isHidden = !showsFrameRate }
    }
    // STUDIO-END preview-frame-rate

    init(camera: Bool) {
        self.camera = camera
        super.init(frame: .zero)
        wantsLayer = true
        layer?.backgroundColor = NSColor.windowBackgroundColor.cgColor
        let font = NSFont.monospacedDigitSystemFont(ofSize: NSFont.smallSystemFontSize, weight: .regular)
        for label in [lights, distance, fps] {
            label.font = font
            label.textColor = .labelColor
        }
        stack.orientation = .horizontal
        stack.spacing = 14
        stack.alignment = .centerY
        stack.addArrangedSubview(lights)
        if camera {
            stack.addArrangedSubview(distance)
        }
        stack.addArrangedSubview(fps)
        fps.isHidden = !showsFrameRate // STUDIO: hidden unless asked for
        stack.translatesAutoresizingMaskIntoConstraints = false
        addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 6),
            stack.centerYAnchor.constraint(equalTo: centerYAnchor),
        ])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    func show(_ status: LivePreviewStatus) {
        let lightsText = "Lights: \(status.lights)"
        if lights.stringValue != lightsText {
            lights.stringValue = lightsText
        }
        if camera {
            showDistance(status.distance)
        }
        let fpsText = camera ? "FPS: \(status.fps)" : "FPS: \(status.fps) fps"
        if fps.stringValue != fpsText {
            fps.stringValue = fpsText
        }
    }

    func showDistance(_ value: Float) {
        let text = "Distance " + String(format: "%.1f", value)
        if distance.stringValue != text {
            distance.stringValue = text
        }
    }
}

/// A live preview window: the union of `GDIPreviewForm` and `OpenGlPreviewForm`, with `UseOpenGL` choosing which view it offers.
final class LivePreviewController: NSObject, NSWindowDelegate {
    let id: String
    let isCamera: Bool
    let window: LivePreviewNSWindow
    let canvas: LiveCanvasView
    let statusBar: LiveStatusBar
    private(set) var live: LivePreview?
    private(set) var flags: LiveWindowFlags
    private let root = NSView()
    private var statusTimer: Timer?
    private var fixtureTimer: Timer?
    private var saveTask: Task<Void, Never>?
    private var loading = true
    private var badge: NSPanel?
    private var badgeMonitorActive = false
    /// F64: while the bar is shown its height is part of the frame in the flat view, and the content rect stays put when borders go.
    private static let barHeight: CGFloat = 22
    /// Set while a test keeps this window where it is.
    var keepOnScreen = false

    private static let borderedMask: NSWindow.StyleMask = [.titled, .resizable, .miniaturizable]
    private static let borderlessMask: NSWindow.StyleMask = [.borderless, .resizable]

    init(id: String, preparation: LivePreparation) {
        self.id = id
        isCamera = preparation.isCamera()
        flags = preparation.flags()
        statusBar = LiveStatusBar(camera: isCamera)
        canvas = LiveCanvasView(frame: NSRect(x: 0, y: 0, width: 400, height: 300))

        let bordersOn = isCamera || flags.showBorders
        let mask = bordersOn ? Self.borderedMask : Self.borderlessMask
        let extra = NSWindow.frameRect(forContentRect: NSRect(x: 0, y: 0, width: 100, height: 100), styleMask: mask)
        let bounds = preparation.bounds(screens: PreviewCoordinates.workingAreas(), frameExtraWidth: Int32(extra.width - 100), frameExtraHeight: Int32(extra.height - 100))
        let contentSize: NSSize
        if isCamera {
            // OpenGL keeps a client size, under the status bar too.
            contentSize = NSSize(width: CGFloat(bounds.width), height: CGFloat(bounds.height))
        } else {
            contentSize = NSSize(width: CGFloat(bounds.width) - (extra.width - 100), height: CGFloat(bounds.height) - (extra.height - 100))
        }
        window = LivePreviewNSWindow(contentRect: NSRect(origin: .zero, size: contentSize), styleMask: mask, backing: .buffered, defer: false)
        super.init()

        window.title = studioPreviewWindowTitle(name: preparation.name()) // STUDIO: no "(OpenGL)" tag in the title
        window.isReleasedWhenClosed = false
        window.tabbingMode = .disallowed
        window.contentMinSize = NSSize(width: 100, height: 60)
        window.collectionBehavior = [.managed, .participatesInCycle]
        window.backgroundColor = .black
        window.delegate = self
        window.closeRequested = { [weak self] in self?.showCannotClose() }
        window.isMovableByWindowBackground = false

        canvas.controller = self
        root.frame = NSRect(origin: .zero, size: contentSize)
        root.autoresizesSubviews = true
        root.addSubview(canvas)
        root.addSubview(statusBar)
        window.contentView = root

        if let location = bounds.location {
            window.setFrameOrigin(PreviewCoordinates.origin(x: location.x, y: location.y, height: window.frame.height))
        } else {
            window.cascadeTopLeft(from: NSPoint(x: 80, y: (NSScreen.main?.visibleFrame.maxY ?? 800) - 40))
        }
        layoutContent()
        NotificationCenter.default.addObserver(self, selector: #selector(contentResized), name: NSView.frameDidChangeNotification, object: root)
        root.postsFrameChangedNotifications = true

        window.orderFront(nil)
        if bounds.maximized {
            window.zoom(nil)
        }
        applyLevel()
        openRenderer(preparation)
        applyTransparencyChrome()
        startStatusTimer()
        loading = false
    }

    private func openRenderer(_ preparation: LivePreparation) {
        canvas.metal.metalLayer?.contentsScale = window.backingScaleFactor
        let size = canvas.metal.pixelSize
        do {
            let opened = try preparation.open(
                metalLayer: canvas.metal.layerAddress,
                widthPx: size.width,
                heightPx: size.height,
                scale: canvas.metal.scale,
                clientWidth: Float(canvas.bounds.width),
                clientHeight: Float(canvas.bounds.height),
                frameHeight: Float(window.frame.height)
            )
            live = opened
            canvas.metal.onResize = { [weak self] width, height, scale in
                self?.drawableResized(width: width, height: height, scale: scale)
            }
            reportFrame()
        } catch {
            showAlert(title: "Preview", text: error.userMessage)
        }
    }

    private func drawableResized(width: UInt32, height: UInt32, scale: Float) {
        live?.resize(widthPx: width, heightPx: height, scale: scale, clientWidth: Float(canvas.bounds.width), clientHeight: Float(canvas.bounds.height), frameHeight: Float(window.frame.height))
    }

    // MARK: Layout

    /// F62: the flat view's canvas stops above the bar. The camera view's fills the content area, and the bar lies over its bottom edge.
    private func layoutContent() {
        let bounds = root.bounds
        let bar = flags.showStatus ? Self.barHeight : 0
        statusBar.isHidden = !flags.showStatus
        statusBar.frame = NSRect(x: 0, y: 0, width: bounds.width, height: Self.barHeight)
        canvas.frame = isCamera ? bounds : NSRect(x: 0, y: bar, width: bounds.width, height: max(bounds.height - bar, 1))
    }

    @objc private func contentResized() {
        layoutContent()
    }

    // MARK: Window delegate

    func windowDidResize(_ notification: Notification) {
        reportFrame()
    }

    func windowDidMove(_ notification: Notification) {
        reportFrame()
        saveSoon()
    }

    func windowDidEndLiveResize(_ notification: Notification) {
        reportFrame()
        save()
    }

    func windowDidMiniaturize(_ notification: Notification) {
        flags.windowState = "Minimized"
        live?.setFlags(flags: flags)
        save()
    }

    func windowDidDeminiaturize(_ notification: Notification) {
        flags.windowState = window.isZoomed ? "Maximized" : "Normal"
        live?.setFlags(flags: flags)
        reportFrame()
        save()
    }

    func windowDidChangeOcclusionState(_ notification: Notification) {
        // The renderer checks for itself when something changes. A window coming back has to ask for a frame.
        statusTick()
    }

    private func reportFrame() {
        guard !loading, !window.isMiniaturized else { return }
        let frame = window.frame
        let top = PreviewCoordinates.topLeft(ofFrame: frame)
        let size = isCamera ? window.contentRect(forFrameRect: frame).size : frame.size
        live?.setWindowFrame(x: top.x, y: top.y, width: Int32(size.width), height: Int32(size.height))
    }

    // MARK: Saving

    /// F71, without upstream's 12 full file writes per event: a burst of moves or wheel steps is one write.
    func saveSoon() {
        saveTask?.cancel()
        saveTask = Task { [weak self] in
            try? await Task.sleep(for: .milliseconds(300))
            guard !Task.isCancelled else { return }
            self?.save()
        }
    }

    func save() {
        guard !loading else { return }
        saveTask?.cancel()
        reportFrame()
        live?.saveState()
    }

    // MARK: Status

    private func startStatusTimer() {
        statusTimer = Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.statusTick() }
        }
        statusTimer?.tolerance = 0.1
        updateFixtureTimer()
    }

    private func updateFixtureTimer() {
        if isCamera && live?.hasFixtures() == true {
            guard fixtureTimer == nil else { return }
            fixtureTimer = Timer.scheduledTimer(withTimeInterval: 0.05, repeats: true) { [weak self] _ in
                MainActor.assumeIsolated {
                    guard let self, let live = self.live else { return }
                    self.canvas.legends.update(live.fixtureLegends())
                }
            }
            fixtureTimer?.tolerance = 0.01
        } else {
            fixtureTimer?.invalidate()
            fixtureTimer = nil
            canvas.legends.update([])
        }
    }

    private func statusTick() {
        guard let live else { return }
        updateFixtureTimer()
        statusBar.show(live.status())
        let name = studioPreviewWindowTitle(name: live.name()) // STUDIO: no "(OpenGL)" tag in the title
        if window.title != name {
            window.title = name
        }
    }

    func distanceChanged() {
        if isCamera, let live {
            statusBar.showDistance(live.status().distance)
        }
    }

    // MARK: Context menu, F63

    func showMenu(_ event: NSEvent, in view: NSView) {
        NSMenu.popUpContextMenu(contextMenu(), with: event, for: view)
    }

    /// F63's items for this view, rebuilt each time as upstream does.
    func contextMenu() -> NSMenu {
        let menu = NSMenu()
        menu.autoenablesItems = false
        func toggle(_ title: String, _ tip: String, _ on: Bool, _ action: Selector) {
            let item = NSMenuItem(title: title, action: action, keyEquivalent: "")
            item.target = self
            item.state = on ? .on : .off
            item.toolTip = tip
            menu.addItem(item)
        }
        func command(_ title: String, _ tip: String, _ action: Selector, enabled: Bool = true) {
            let item = NSMenuItem(title: title, action: action, keyEquivalent: "")
            item.target = self
            item.toolTip = tip
            item.isEnabled = enabled
            menu.addItem(item)
        }
        toggle("Show Status", "Enable/Disable the preview status bar.", flags.showStatus, #selector(toggleStatus))
        toggle("Show Frame Rate", "Show how many frames a second the preview draws, in the status bar.", statusBar.showsFrameRate, #selector(toggleFrameRate)) // STUDIO: frame rate on request
        if !isCamera {
            toggle("Show Borders", "Enable/Disable the preview borders.", flags.showBorders, #selector(toggleBorders))
            toggle("Lock Position", "Enable/Disable the window position lock.", flags.lockPosition, #selector(toggleLockPosition))
        }
        toggle("Always On Top", "Enable/Disable the window always on top.", flags.alwaysOnTop, #selector(toggleAlwaysOnTop))
        toggle("Auto On Top", "Enable/Disable bringing this preview on top and back automatically.", flags.onTopWhenActive, #selector(toggleAutoOnTop))
        toggle("Transparent Background", "Make the background transparent so the desktop is visible through unlit areas. Transparent areas are click-through.", flags.transparent, #selector(toggleTransparent))
        command("Reset Size", "Resets the viewable size to match the background size.", #selector(resetSize), enabled: !isCamera || live?.hasBackground() == true)
        if isCamera {
            command("Center Preview", "Repositions the viewpoint to the center of the preview.", #selector(centerPreview))
            toggle("Enable Light Scaling", "Scales the light size as the camera is zoomed in or out.", flags.enableLightScaling, #selector(toggleLightScaling))
        } else {
            command("Reset Zoom", "Resets the Zoom to 100%.", #selector(resetZoom))
        }
        menu.addItem(.separator())
        let frame = window.frame
        let top = PreviewCoordinates.topLeft(ofFrame: frame)
        let size = window.contentRect(forFrameRect: frame).size
        let location = NSMenuItem(title: "Location: \(top.x),\(top.y)", action: nil, keyEquivalent: "")
        location.isEnabled = false
        menu.addItem(location)
        let sizeItem = NSMenuItem(title: "Size: \(Int(size.width)) X \(Int(size.height))", action: nil, keyEquivalent: "")
        sizeItem.isEnabled = false
        menu.addItem(sizeItem)
        return menu
    }

    private func flagsChanged() {
        live?.setFlags(flags: flags)
        save()
    }

    // STUDIO-BEGIN preview-frame-rate-toggle: one setting for every preview window
    @objc func toggleFrameRate() {
        let show = !statusBar.showsFrameRate
        UserDefaults.standard.set(show, forKey: LiveStatusBar.frameRateKey)
        for controller in LivePreviewCoordinator.shared?.controllers.values.map({ $0 }) ?? [self] {
            controller.statusBar.showsFrameRate = show
        }
        statusBar.showsFrameRate = show
    }
    // STUDIO-END preview-frame-rate-toggle

    @objc func toggleStatus() {
        flags.showStatus.toggle()
        layoutContent()
        flagsChanged()
    }

    @objc func toggleBorders() {
        flags.showBorders.toggle()
        applyBorders()
        flagsChanged()
    }

    /// F64: the content rect stays put, which upstream doesn't manage.
    private func applyBorders() {
        let content = window.convertToScreen(window.contentLayoutRect)
        window.styleMask = flags.showBorders ? Self.borderedMask : Self.borderlessMask
        window.setFrame(window.frameRect(forContentRect: content), display: true)
        window.delegate = self
        window.makeFirstResponder(canvas)
    }

    @objc func toggleLockPosition() {
        flags.lockPosition.toggle()
        flagsChanged()
    }

    /// F68: the OpenGL form's rule, in both views: turning this on clears Auto On Top.
    @objc func toggleAlwaysOnTop() {
        flags.alwaysOnTop.toggle()
        if flags.alwaysOnTop {
            flags.onTopWhenActive = false
        }
        applyLevel()
        flagsChanged()
    }

    /// F69: turning this on clears Always On Top, and turning it off drops a window that's on top.
    @objc func toggleAutoOnTop() {
        flags.onTopWhenActive.toggle()
        if flags.onTopWhenActive {
            flags.alwaysOnTop = false
        } else {
            window.level = .normal
        }
        applyLevel()
        flagsChanged()
    }

    private func applyLevel() {
        window.level = flags.alwaysOnTop ? .floating : .normal
    }

    /// F69, from the sequencer's play path.
    func playerActive(_ active: Bool) {
        guard flags.onTopWhenActive else { return }
        if active {
            window.level = .floating
            window.orderFrontRegardless()
        } else {
            window.level = .normal
            window.orderBack(nil)
        }
    }

    /// F70: turning it on with Auto On Top off forces Always On Top on, and turning it off doesn't undo that.
    @objc func toggleTransparent() {
        setTransparent(!flags.transparent)
    }

    func setTransparent(_ on: Bool) {
        flags.transparent = on
        if on, !flags.onTopWhenActive {
            flags.alwaysOnTop = true
            applyLevel()
        }
        live?.setTransparent(on: on)
        applyTransparencyChrome()
        flagsChanged()
        LivePreviewCoordinator.shared?.transparencyChanged()
    }

    private func applyTransparencyChrome() {
        let on = flags.transparent
        window.isOpaque = !on
        window.backgroundColor = on ? .clear : .black
        window.hasShadow = !on
        if on {
            showBadge()
        } else {
            badge?.orderOut(nil)
            if let badge {
                window.removeChildWindow(badge)
            }
            badge = nil
        }
    }

    /// F70's way back when the rest of the window lets clicks through: a separate small window, so it stays clickable whatever the main one does.
    private func showBadge() {
        guard badge == nil else { return }
        let button = NSButton(title: "Transparent", target: self, action: #selector(badgeClicked))
        button.isBordered = false
        button.wantsLayer = true
        button.layer?.backgroundColor = NSColor(red: 40 / 255, green: 110 / 255, blue: 175 / 255, alpha: 1).cgColor
        button.attributedTitle = NSAttributedString(string: "Transparent", attributes: [.foregroundColor: NSColor.white, .font: NSFont.boldSystemFont(ofSize: NSFont.smallSystemFontSize)])
        button.sizeToFit()
        button.frame.size.width += 8
        let panel = NSPanel(contentRect: button.bounds, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
        panel.isReleasedWhenClosed = false
        panel.contentView = button
        panel.hasShadow = false
        panel.level = window.level
        let content = window.convertToScreen(window.contentLayoutRect)
        panel.setFrameTopLeftPoint(NSPoint(x: content.minX + 2, y: content.maxY - 2))
        window.addChildWindow(panel, ordered: .above)
        badge = panel
    }

    @objc private func badgeClicked() {
        setTransparent(false)
    }

    @objc func resetSize() {
        guard let size = live?.resetSize() else { return }
        window.setContentSize(NSSize(width: size.width, height: size.height))
        reportFrame()
        save()
    }

    @objc func resetZoom() {
        live?.resetZoom()
        save()
    }

    @objc func centerPreview() {
        live?.center()
        save()
    }

    @objc func toggleLightScaling() {
        flags.enableLightScaling.toggle()
        live?.setLightScaling(on: flags.enableLightScaling)
        save()
    }

    // MARK: Closing, F60

    private func showCannotClose() {
        showAlert(title: "Close", text: "To close this preview, turn it off in the Previews section of the show window.") // STUDIO: the Previews pane replaced Setup Previews
    }

    private func showAlert(title: String, text: String) {
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = text
        alert.addButton(withTitle: "OK")
        alert.beginSheetModal(for: window) { _ in }
    }

    /// The device stopped, or the app is going.
    func shutDown() {
        statusTimer?.invalidate()
        statusTimer = nil
        fixtureTimer?.invalidate()
        fixtureTimer = nil
        saveTask?.cancel()
        if !isCamera {
            save()
        }
        NotificationCenter.default.removeObserver(self)
        if let badge {
            window.removeChildWindow(badge)
            badge.orderOut(nil)
        }
        window.delegate = nil
        window.closeRequested = nil
        window.orderOut(nil)
        canvas.metal.onResize = nil
        let closing = live
        live = nil
        // The renderer's thread can take a moment to wind down, and this is the main thread.
        Task.detached { _ = closing }
    }

    func reload() {
        guard let live else { return }
        do {
            try live.reload()
        } catch {
            showAlert(title: "Preview", text: error.userMessage)
        }
    }
}
