import AppKit
import SwiftUI
import VixenBridge
import VixenKit

/// The sequencer's windows, one per open sequence. A `.tim` needs the profile's element tree, so it opens from the Administration window rather
/// than through `DocumentGroup`, and upstream's explicit Auto Save and Save As stay as they were.
public struct SequencerScene: Scene {
    private let model: AppModel
    @State private var registry = SequencerRegistry()

    public init(model: AppModel) {
        self.model = model
    }

    public var body: some Scene {
        WindowGroup("Sequencer", id: WindowID.sequencer, for: SequenceDocumentRef.self) { $ref in
            SequencerWindow(ref: ref)
                .environment(model)
                .environment(registry)
                .studioAppearance() // STUDIO: follow the chosen appearance
        }
        .defaultSize(width: 1440, height: 900)
        // Sequences reopen from the recent list, not from the system's window restoration.
        .restorationBehavior(.disabled)
        .commands {
            SequencerCommands(registry: registry)
        }
    }
}

/// The open sequencer windows, and which one the menus and floating panels act on: F19, F225. A floating panel doesn't make another sequencer active.
@Observable
final class SequencerRegistry {
    private(set) var active: SequencerModel?
    /// A sequencer window or one of its panels has the keyboard, so the menu bar is the sequencer's, as each upstream window has its own menu strip.
    private(set) var inFront = false
    @ObservationIgnored private var observers: [NSObjectProtocol] = []

    init() {
        let center = NotificationCenter.default
        for name in [NSWindow.didBecomeKeyNotification, NSWindow.didResignKeyNotification] {
            observers.append(center.addObserver(forName: name, object: nil, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.keyWindowChanged() }
            })
        }
        // The live preview's "KeydownSWF" broadcast (preview F72): its keys go to the active editor, and nowhere with none. F225.
        observers.append(center.addObserver(forName: .previewForwardedKey, object: nil, queue: .main) { [weak self] note in
            nonisolated(unsafe) let event = note.object as? NSEvent
            MainActor.assumeIsolated {
                if let event {
                    _ = self?.active?.handleKey(event, forwarded: true)
                }
            }
        })
        // Key-window notifications can arrive before NSApp publishes the new key window.
        // Reconcile the settled ownership before arranging SwiftUI's rebuilt menu items.
        observers.append(center.addObserver(forName: NSApplication.didUpdateNotification, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated {
                self?.keyWindowChanged()
                SequencerMenuBar.arrange(inFront: self?.inFront ?? false)
            }
        })
    }

    func activate(_ model: SequencerModel) {
        active = model
        model.app.editorActivated(model)
        keyWindowChanged()
    }

    func closed(_ model: SequencerModel) {
        if active === model {
            active = nil
        }
        keyWindowChanged()
    }

    /// Menu actions can arrive before SwiftUI has rebuilt their key equivalents.
    /// Recheck ownership at dispatch so an old shortcut cannot save another window.
    func focusedModel() -> SequencerModel? {
        keyWindowChanged()
        return inFront ? active : nil
    }

    private func keyWindowChanged() {
        guard let active else {
            if inFront { inFront = false }
            return
        }
        // The main window owns the menu while the app is inactive. Only old offscreen
        // fixtures with neither window need the harness fallback.
        let key = NSApp.keyWindow ?? NSApp.mainWindow
        let ownsFocus: Bool
        if let key {
            ownsFocus = key === active.window || key is FloatingPanelWindow || key.sheetParent === active.window || key.parent === active.window
        } else {
            ownsFocus = UITestHarness.isActive
        }
        if inFront != ownsFocus { inFront = ownsFocus }
    }
}

/// `Sequence, Edit, View, Tools, Help`: the menu bar with a sequencer in front. "Sequence" goes where a document app has File, and the Administration window's menus step aside.
/// With anything else in front, the sequencer's step aside instead.
enum SequencerMenuBar {
    private static let administrationOnly: Set<String> = ["System", "Scheduler"]

    static func arrange(inFront: Bool) {
        guard let main = NSApp.mainMenu else { return }
        let items = main.items
        guard let sequence = items.first(where: { $0.title == "Sequence" }) else { return }
        let ours = items.first { $0.title == "Tools" && $0.submenu?.items.first?.title == "Audio" }
        for item in items {
            let hide: Bool
            if item === sequence || item === ours {
                hide = !inFront
            } else if administrationOnly.contains(item.title) || (item.title == "Tools" && ours != nil) { // STUDIO: never hide the only Tools menu (SwiftUI can merge both into one)
                hide = inFront
            } else {
                continue
            }
            if item.isHidden != hide {
                item.isHidden = hide
            }
        }
        if inFront {
            if main.index(of: sequence) != 1 {
                main.removeItem(sequence)
                main.insertItem(sequence, at: 1)
            }
            if let ours, let view = items.first(where: { $0.title == "View" }), main.index(of: ours) != main.index(of: view) + 1 {
                main.removeItem(ours)
                main.insertItem(ours, at: main.index(of: view) + 1)
            }
        }
    }
}

/// The root of one sequencer window. The window appears at once and stays responsive while the sequence loads (F5).
struct SequencerWindow: View {
    let ref: SequenceDocumentRef?
    @Environment(AppModel.self) private var app
    @Environment(SequencerRegistry.self) private var registry
    @State private var model: SequencerModel?

    var body: some View {
        Group {
            if let model {
                SequencerContent(model: model)
                    .background(WindowReader { window in model.attach(window: window) })
            } else {
                Color.clear
            }
        }
        // F3's minimum.
        .frame(minWidth: 640, minHeight: 480)
        .onAppear {
            if model == nil {
                let created = SequencerModel(app: app, ref: Self.resolve(ref), registry: registry)
                model = created
                Task { await created.open() }
            }
        }
    }

    /// `openWindow(id:)` with no value is how a test script opens one: it names the sequence in `VIXEN_TEST_SEQUENCE`, or gets a new one.
    private static func resolve(_ ref: SequenceDocumentRef?) -> SequenceDocumentRef {
        if let ref {
            return ref
        }
        if let path = ProcessInfo.processInfo.environment["VIXEN_TEST_SEQUENCE"] {
            return .existing(path: path)
        }
        return .new()
    }
}

/// Finds the `NSWindow` a view lands in.
struct WindowReader: NSViewRepresentable {
    let found: (NSWindow) -> Void

    func makeNSView(context: Context) -> NSView {
        let view = ReaderView()
        view.found = found
        return view
    }

    func updateNSView(_ nsView: NSView, context: Context) {}

    private final class ReaderView: NSView {
        var found: ((NSWindow) -> Void)?

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            if let window {
                found?(window)
            }
        }
    }
}
