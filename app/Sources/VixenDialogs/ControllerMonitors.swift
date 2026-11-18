import AppKit
import QuartzCore
import SwiftUI
import VixenBridge
import VixenKit

/// The Dummy Lighting and Debugging Values output windows (F46 to F50, F134), and messages from running controllers (F90 to F92). Include this
/// scene once, in whichever scene owns the setup windows. Windows open when a controller starts, and close when it stops. Closing one by hand
/// leaves the controller running, and its next start opens it again.
public struct ControllerMonitorScene: Scene {
    private let model: AppModel
    @State private var center = ControllerMonitorCenter()

    public init(model: AppModel) {
        self.model = model
    }

    public var body: some Scene {
        WindowGroup("Controller Output", id: ControllerMonitorCenter.windowID, for: String.self) { $controller in
            if let controller, let core = model.core {
                ControllerMonitorWindow(context: core.dialogContext(), controller: controller)
                    .studioAppearance() // STUDIO: follow the chosen appearance
            }
        }
        .defaultSize(width: 331, height: 302)
        .restorationBehavior(.disabled)
        .defaultLaunchBehavior(.suppressed)
        .commands {
            ControllerMonitorStarter(center: center, model: model)
        }
    }
}

/// Commands stay alive as long as the app does and can open windows, so this is what watches for controllers starting.
private struct ControllerMonitorStarter: Commands {
    let center: ControllerMonitorCenter
    let model: AppModel
    @Environment(\.openWindow) private var openWindow

    var body: some Commands {
        let _ = center.start(model: model, openWindow: openWindow)
        CommandGroup(after: .windowArrangement) {}
    }
}

@MainActor
@Observable
final class ControllerMonitorCenter {
    static let windowID = "controller-monitor"

    private var timer: Timer?
    private var model: AppModel?
    private var openWindow: OpenWindowAction?
    private var opened = Set<String>()
    private var messagePanels: [NSPanel] = []

    /// Twice a second is plenty: this only notices controllers starting and stopping.
    func start(model: AppModel, openWindow: OpenWindowAction) {
        self.model = model
        self.openWindow = openWindow
        guard timer == nil else { return }
        timer = Timer.scheduledTimer(withTimeInterval: 0.5, repeats: true) { [weak self] _ in
            // A Foundation run-loop callback is not a Swift actor task. Hop explicitly rather than asking the
            // runtime to infer executor identity (which crashed in the native timer on this host).
            Task { @MainActor [weak self] in self?.tick() }
        }
    }

    private func tick() {
        guard let core = model?.core else { return }
        let context = core.dialogContext()
        let running = Set(context.runningMonitors().map(\.controller))
        for controller in running.subtracting(opened) {
            openWindow?(id: Self.windowID, value: controller)
        }
        opened = running
        for message in context.takeControllerMessages() {
            show(message)
        }
    }

    private func show(_ message: DialogControllerMessage) {
        let panel = NSPanel(contentRect: NSRect(x: 0, y: 0, width: 420, height: 200), styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
        panel.title = message.title
        panel.isReleasedWhenClosed = false
        let close = { [weak self, weak panel] in
            panel?.close()
            self?.messagePanels.removeAll { $0 === panel }
        }
        let view = ScrollingMessageSheet(heading: message.heading ?? "", message: message.text, onFinish: close)
            .studioAppearance() // STUDIO: follow the chosen appearance
        panel.contentViewController = NSHostingController(rootView: view)
        panel.center()
        panel.makeKeyAndOrderFront(nil)
        messagePanels.append(panel)
    }
}

/// One controller's output window. It closes itself when the controller stops.
public struct ControllerMonitorWindow: View {
    let context: DialogContext
    let controller: String
    @Environment(\.dismissWindow) private var dismissWindow
    @State private var monitor: DialogMonitor?

    public init(context: DialogContext, controller: String) {
        self.context = context
        self.controller = controller
    }

    public var body: some View {
        Group {
            switch monitor?.kind {
            case .dummy:
                if let view = try? context.dummyMonitor(controller: controller) {
                    DummyMonitorView(monitor: view)
                }
            case .debug:
                if let view = try? context.debugMonitor(controller: controller) {
                    DebugMonitorView(monitor: view)
                }
            case nil:
                Color.black
            }
        }
        .navigationTitle(monitor?.title ?? "Controller Output")
        .task {
            monitor = context.runningMonitors().first { $0.controller == controller }
            while !Task.isCancelled {
                try? await Task.sleep(for: .milliseconds(500))
                guard context.runningMonitors().contains(where: { $0.controller == controller }) else {
                    dismissWindow()
                    return
                }
            }
        }
    }
}

/// F47 to F49: the boxes are one bitmap with a pixel per box, scaled up with nothing smoothed, and the grid lines an overlay. Upstream fills a
/// rectangle per output, and 3351 of them at 100 Hz would bring back the slowness this port exists to remove.
struct DummyMonitorView: NSViewRepresentable {
    let monitor: DummyMonitor

    func makeNSView(context: Context) -> DummyMonitorNSView {
        DummyMonitorNSView(monitor: monitor)
    }

    func updateNSView(_ view: DummyMonitorNSView, context: Context) {}
}

final class DummyMonitorNSView: NSView {
    private let monitor: DummyMonitor
    private let boxes = CALayer()
    private let grid = CAShapeLayer()
    private let fps = CATextLayer()
    private var displayLink: CADisplayLink?
    private var across: UInt32 = 0
    private var down: UInt32 = 0

    init(monitor: DummyMonitor) {
        self.monitor = monitor
        super.init(frame: .zero)
        wantsLayer = true
        layer?.backgroundColor = NSColor.black.cgColor
        boxes.magnificationFilter = .nearest
        boxes.contentsGravity = .resize
        boxes.actions = ["contents": NSNull(), "bounds": NSNull(), "position": NSNull()]
        layer?.addSublayer(boxes)
        grid.fillColor = nil
        grid.strokeColor = NSColor.black.cgColor
        grid.lineWidth = 1
        layer?.addSublayer(grid)
        fps.foregroundColor = NSColor.red.cgColor
        fps.fontSize = 12
        fps.font = NSFont.monospacedDigitSystemFont(ofSize: 12, weight: .regular)
        fps.contentsScale = 2
        fps.actions = ["contents": NSNull(), "bounds": NSNull(), "position": NSNull()]
        layer?.addSublayer(fps)
    }

    required init?(coder: NSCoder) {
        fatalError("not from a file")
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        displayLink?.invalidate()
        displayLink = nil
        if window != nil {
            let link = displayLink(target: self, selector: #selector(refresh))
            link.add(to: .main, forMode: .common)
            displayLink = link
        }
    }

    override func layout() {
        super.layout()
        let height = 18.0
        fps.frame = CGRect(x: 2, y: bounds.height - height - 2, width: bounds.width - 4, height: height)
        layoutGrid()
    }

    /// F47: reflows on resize.
    private func layoutGrid() {
        guard across > 0, down > 0 else {
            boxes.frame = bounds
            grid.path = nil
            return
        }
        let cellWidth = bounds.width / CGFloat(across)
        let cellHeight = bounds.height / CGFloat(down)
        boxes.frame = bounds
        let path = CGMutablePath()
        for x in 1..<max(across, 1) {
            path.move(to: CGPoint(x: CGFloat(x) * cellWidth, y: 0))
            path.addLine(to: CGPoint(x: CGFloat(x) * cellWidth, y: bounds.height))
        }
        for y in 1..<max(down, 1) {
            path.move(to: CGPoint(x: 0, y: CGFloat(y) * cellHeight))
            path.addLine(to: CGPoint(x: bounds.width, y: CGFloat(y) * cellHeight))
        }
        grid.frame = bounds
        grid.path = path
    }

    @objc private func refresh() {
        let frame = monitor.frame()
        if frame.across != across || frame.down != down {
            across = frame.across
            down = frame.down
            layoutGrid()
        }
        fps.string = frame.fps
        guard across > 0, down > 0, frame.rgba.count == Int(across * down) * 4 else {
            boxes.contents = nil
            return
        }
        let provider = CGDataProvider(data: frame.rgba as CFData)
        boxes.contents = provider.flatMap {
            CGImage(
                width: Int(across), height: Int(down), bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: Int(across) * 4,
                space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue),
                provider: $0, decode: nil, shouldInterpolate: false, intent: .defaultIntent
            )
        }
    }
}

/// F134: read-only text filling the window, with Verbose at the bottom right.
struct DebugMonitorView: View {
    let monitor: DebugMonitor
    @State private var verbose = false

    var body: some View {
        VStack(spacing: 6) {
            DebugTextView(monitor: monitor, verbose: verbose)
            HStack {
                Spacer()
                Toggle("Verbose", isOn: $verbose).toggleStyle(.checkbox)
            }
        }
        .padding(8)
        .frame(minWidth: 538, minHeight: 405)
    }
}

/// F134: read-only, and its storage replaced at most once per display refresh.
struct DebugTextView: NSViewRepresentable {
    let monitor: DebugMonitor
    let verbose: Bool

    func makeNSView(context: Context) -> DebugTextScrollView {
        DebugTextScrollView(monitor: monitor)
    }

    func updateNSView(_ view: DebugTextScrollView, context: Context) {
        view.verbose = verbose
    }
}

final class DebugTextScrollView: NSScrollView {
    private let monitor: DebugMonitor
    private let text = NSTextView()
    private var displayLink: CADisplayLink?
    var verbose = false

    init(monitor: DebugMonitor) {
        self.monitor = monitor
        super.init(frame: .zero)
        hasVerticalScroller = true
        borderType = .bezelBorder
        text.isEditable = false
        text.isSelectable = true
        text.isRichText = false
        text.font = NSFont.monospacedSystemFont(ofSize: NSFont.smallSystemFontSize, weight: .regular)
        text.isVerticallyResizable = true
        text.autoresizingMask = .width
        text.textContainer?.widthTracksTextView = true
        documentView = text
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("not used")
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        displayLink?.invalidate()
        displayLink = nil
        if window != nil {
            let link = displayLink(target: self, selector: #selector(refresh))
            link.add(to: .main, forMode: .common)
            displayLink = link
        }
    }

    @objc private func refresh() {
        if let latest = monitor.text(verbose: verbose) {
            text.string = latest
        }
    }
}
