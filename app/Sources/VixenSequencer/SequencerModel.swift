import AppKit
import Observation
import SwiftUI
import UniformTypeIdentifiers
import VixenBridge
import VixenKit

#if canImport(VixenTimeline)
import VixenTimeline
#endif

/// One sequencer window. Holds nothing but what Rust says, cached between changes so a menu opening or a view redrawing doesn't cross the boundary.
@Observable
final class SequencerModel: OpenEditor {
    let app: AppModel
    let ref: SequenceDocumentRef
    private let registry: SequencerRegistry

    private(set) var session: SequencerSession?
    private(set) var shell: SequencerShell?
    #if canImport(VixenTimeline)
    /// One per window, kept so its state lives as long as the window.
    private(set) var canvas: TimelineCanvas?
    #endif
    /// Everything menus, strips and the status bar read. `nil` until the sequence has loaded.
    private(set) var ui: ShellUiState?
    private(set) var toolbars: ShellToolbars?
    private(set) var panels: ShellPanels?
    /// Updated at display rate only while playing, on its own, so only the label redraws.
    private(set) var positionText = "0:00.000"
    /// F6's text while loading.
    private(set) var loadingText: String?
    var sheet: SequencerSheet?

    let effects: [ShellEffect] = sequencerEffectCatalog()

    /// The three libraries: strips and panels read these, and `librariesChanged()` reloads them from Rust.
    private(set) var palette: [UInt32] = []
    private(set) var curves: [LibraryCurveItem] = []
    private(set) var gradients: [LibraryGradientItem] = []
    /// How many edits the sequence has had. Panels that follow it watch this.
    private(set) var editRevision: UInt64 = 0
    /// What the Effect Editor shows: the timeline's selection, 200 ms behind it (F375).
    private(set) var editorSelection: [UInt64] = []
    @ObservationIgnored private var selectionDebounce: Task<Void, Never>?

    @ObservationIgnored weak var window: NSWindow?
    @ObservationIgnored private var delegateProxy: SequencerWindowDelegate?
    @ObservationIgnored private var pollTask: Task<Void, Never>?
    @ObservationIgnored private var frameTask: Task<Void, Never>?
    @ObservationIgnored private var keyMonitor: Any?
    @ObservationIgnored private var closing = false
    /// The close prompt or silent save in progress, so a second request to close doesn't stack another on the same window.
    @ObservationIgnored private var closeAnswer: Task<Void, Never>?
    @ObservationIgnored private var saving = false
    @ObservationIgnored private var clipboardChangeCount = -1
    @ObservationIgnored private var floating: [ShellPanel: FloatingPanelWindow] = [:]
    /// F87's submenu. Read when the window comes forward and after a choice, since the maps are edited in another window.
    var lipSyncMapList: [ShellLipSyncMap] = []
    @ObservationIgnored var lastPhonemes: String?
    /// The `Effects` panel's selection clears through this: F204.
    @ObservationIgnored var effectsSelectionCleared: (() -> Void)?
    /// F306: the answer is cached per hovered effect while hovering.
    @ObservationIgnored var dropCacheElement: UInt64?
    @ObservationIgnored var dropCacheAnswer: NSDragOperation?
    /// The last context menu, so a script can run an item of it.
    @ObservationIgnored var testContextMenu: NSMenu?
    /// Its items by path, for the test menu.
    var testContextPaths: [String] = []
    /// What a script can't hold down, or point at.
    @ObservationIgnored var testShift = false
    @ObservationIgnored var testPointer: NSPoint?
    @ObservationIgnored private var testKeyRoute = ""

    init(app: AppModel, ref: SequenceDocumentRef, registry: SequencerRegistry) {
        self.app = app
        self.ref = ref
        self.registry = registry
    }

    private var isReady: Bool { shell != nil }

    // MARK: OpenEditor

    var isModified: Bool {
        ui?.modified ?? false
    }

    func closeForShutdown() async {
        if let closeAnswer {
            await closeAnswer.value
            if closing {
                return
            }
        }
        guard let shell else {
            finishClosing()
            window?.close()
            return
        }
        refresh()
        switch shell.closeAction(appClosing: true) {
        case .prompt:
            let answer = await SequencerAlerts.saveChanges(name: session?.name() ?? "Untitled", canCancel: false, window: window)
            if answer == .save {
                _ = await save(forcePanel: false)
            }
        case .saveSilently:
            _ = await save(forcePanel: false)
        case .close:
            break
        }
        finishClosing()
        window?.close()
    }

    // MARK: Opening

    func attach(window: NSWindow) {
        guard self.window !== window else { return }
        self.window = window
        window.tabbingMode = .disallowed
        window.minSize = NSSize(width: 640, height: 480)
        SequencerWindowFrame.restore(window)
        let proxy = SequencerWindowDelegate(model: self, original: window.delegate)
        delegateProxy = proxy
        window.delegate = proxy
        if window.isKeyWindow || UITestHarness.isActive {
            registry.activate(self)
        }
        if UITestHarness.isActive {
            registerHarnessHooks(window: window)
            // `sequencer.activate` with path [part of a window title]: what clicking another sequencer's window does. Harness windows never
            // become key, so without it every menu and hook acts on the sequencer opened last.
            let earlier = UITestHarness.hooks["sequencer.activate"]
            UITestHarness.hooks["sequencer.activate"] = { [weak self, weak window] args in
                if let self, let window, let part = args.first, window.title.contains(part) {
                    self.registerHarnessHooks(window: window)
                    self.windowBecameKey()
                    return "active \(window.title)"
                }
                if let earlier { return await earlier(args) }
                return "FAIL no such sequencer"
            }
        }
        reloadLipSyncMaps()
        updateWindowChrome()
    }

    private func registerHarnessHooks(window: NSWindow) {
        UITestHarness.hooks["sequencer.focusState"] = { [weak self, weak window] _ in
            guard let self else { return "FAIL no sequencer" }
            @MainActor func describe(_ value: NSWindow?) -> [String: Any] {
                guard let value else { return ["title": "nil"] }
                return ["title": value.title, "identity": String(describing: ObjectIdentifier(value)),
                        "key": value.isKeyWindow, "main": value.isMainWindow]
            }
            let state: [String: Any] = ["inFront": self.registry.inFront,
                "captured": describe(window), "registered": describe(self.window),
                "active": describe(self.registry.active?.window), "key": describe(NSApp.keyWindow),
                "main": describe(NSApp.mainWindow), "keyIsRegistered": NSApp.keyWindow === self.window,
                "mainIsRegistered": NSApp.mainWindow === self.window, "keyRoute": self.testKeyRoute]
            guard let data = try? JSONSerialization.data(withJSONObject: state, options: [.sortedKeys]) else { return "FAIL focus serialization" }
            return String(decoding: data, as: UTF8.self)
        }
        UITestHarness.hooks["sequencer.pointer"] = { [weak self, weak window] args in
            guard let self, let window, args.count == 2, let x = Double(args[0]), let y = Double(args[1]) else { return "FAIL needs pointer x/y" }
            // An offscreen test window cannot put the physical mouse over an effect. Use the existing Test Pointer fixture
            // for the upstream hotkeys whose precondition is a selected effect under the pointer.
            self.testPointer = NSPoint(x: x, y: window.frame.height - y)
            return "pointer \(x),\(y)"
        }
        UITestHarness.hooks["sequencer.keyboardState"] = { [weak self] _ in
            guard let ui = self?.ui else { return "FAIL no sequencer" }
            return "running=\(ui.transport.running) paused=\(ui.transport.paused) looping=\(ui.transport.looping)"
        }
        #if canImport(VixenTimeline)
        UITestHarness.hooks["sequencer.waveformState"] = { [weak self] _ in
            guard let canvas = self?.canvas else { return "FAIL no timeline" }
            let view = canvas.controller.view()
            let frame = canvas.controller.waveformFrame(firstColumn: 0, columns: 64, timePerPixel: 100_000)
            let peak = (frame.highs + frame.lows).map { abs($0) }.max() ?? 0
            let state: [String: Any] = ["kind": "waveform", "building": frame.building,
                "failed": frame.failed, "height": view.waveformHeight, "blank": view.waveformBlank,
                "columns": frame.highs.count, "peak": peak]
            guard let data = try? JSONSerialization.data(withJSONObject: state, options: [.sortedKeys]) else { return "FAIL waveform state" }
            return String(decoding: data, as: UTF8.self)
        }
        #endif
        UITestHarness.hooks["sequencer.keyEvent"] = { [weak self, weak window] args in
            guard let window else { return "FAIL no sequencer window" }
            let keys: [String: UInt16] = ["space": 49, "return": 36, "escape": 53, "delete": 51,
                "f5": 96, "f8": 100, "f9": 101, "d": 2, "i": 34, "u": 32, "a": 0, "c": 8, "v": 9, "z": 6, "y": 16, "s": 1]
            guard let key = args.first, let code = keys[key] else { return "FAIL unknown key" }
            var flags: NSEvent.ModifierFlags = []
            for modifier in args.dropFirst() {
                switch modifier {
                case "shift": flags.insert(.shift)
                case "cmd": flags.insert(.command)
                case "ctrl", "control": flags.insert(.control)
                case "opt": flags.insert(.option)
                case "function": flags.insert(.function)
                default: break
                }
            }
            // SwiftUI key equivalents also inspect the underlying CGEvent. Hand-made NSEvent strings can fail
            // Cmd-Shift-Z even when their characters match a real event. The harness's focused window receives these.
            self?.testKeyRoute = "not dispatched"
            for down in [true, false] {
                guard let keyEvent = CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: down) else { return "FAIL cannot create key event" }
                keyEvent.flags = CGEventFlags(rawValue: UInt64(flags.rawValue))
                guard let event = NSEvent(cgEvent: keyEvent) else { return "FAIL cannot create AppKit key event" }
                if args.contains("forwarded") {
                    if down { NotificationCenter.default.post(name: .previewForwardedKey, object: event) }
                } else {
                    NSApp.postEvent(event, atStart: false)
                }
            }
            try? await Task.sleep(for: .milliseconds(150))
            return "posted \(key) responder=\(String(describing: window.firstResponder)) route=\(self?.testKeyRoute ?? "")"
        }
        UITestHarness.hooks["sequencer.tags"] = { [weak self] _ in
            guard let self else { return "FAIL no sequencer" }
            self.timelineManageTags()
            return "opened tag manager"
        }
        // Exercise the same transport commands as the gauge, and expose observable UI
        // state for assertions when offscreen SwiftUI controls omit their AX children.
        UITestHarness.hooks["sequencer.transport"] = { [weak self] args in
            guard let self else { return "FAIL no sequencer" }
            switch args.first ?? "state" {
            case "max": self.run(.speedMax)
            case "reset": self.run(.speedReset)
            case "increase": self.run(.speedIncrease)
            case "decrease": self.run(.speedDecrease)
            case "playPause": self.run(.playPause)
            case "stop": self.run(.stop)
            case "loop": self.run(.toggleLoop)
            case "state": break
            default: return "FAIL unknown transport action"
            }
            guard let ui = self.ui else { return "FAIL sequencer not loaded" }
            return "speed=\(ui.speed.gaugeText) running=\(ui.transport.running) tempo=\(ui.prefs.useTempoForSpeed)"
        }
        // `sequencer.resize` with path [width, height]: what a script can't do with the pointer.
        UITestHarness.hooks["sequencer.resize"] = { [weak window] args in
            guard let window, args.count == 2, let width = Double(args[0]), let height = Double(args[1]) else { return "FAIL needs width and height" }
            var frame = window.frame
            frame.size = NSSize(width: width, height: height)
            window.setFrame(frame, display: true)
            return "\(Int(window.frame.width))x\(Int(window.frame.height))"
        }
        // `sequencer.wheel` with path [x, y from the top, dy, modifiers joined by "+"]: a wheel event through the app's own queue, so
        // event monitors hear it. `sequencer.panelScale` with path [settings key]: the saved scale.
        UITestHarness.hooks["sequencer.wheel"] = { [weak window] args in
            guard let window, let content = window.contentView, args.count == 4, let x = Double(args[0]), let y = Double(args[1]), let dy = Int32(args[2]) else {
                return "FAIL needs x, y, dy, modifiers"
            }
            guard let cg = CGEvent(scrollWheelEvent2Source: nil, units: .line, wheelCount: 1, wheel1: dy, wheel2: 0, wheel3: 0) else { return "FAIL no event" }
            var flags: CGEventFlags = []
            for name in args[3].split(separator: "+") {
                switch name {
                case "cmd": flags.insert(.maskCommand)
                case "shift": flags.insert(.maskShift)
                default: break
                }
            }
            cg.flags = flags
            let screen = window.convertPoint(toScreen: NSPoint(x: x, y: content.bounds.height - y))
            cg.location = CGPoint(x: screen.x, y: (NSScreen.screens.first?.frame.maxY ?? 0) - screen.y)
            cg.setIntegerValueField(.mouseEventWindowUnderMousePointer, value: Int64(window.windowNumber))
            cg.setIntegerValueField(.mouseEventWindowUnderMousePointerThatCanHandleThisEvent, value: Int64(window.windowNumber))
            guard let event = NSEvent(cgEvent: cg) else { return "FAIL no event" }
            NSApp.postEvent(event, atStart: false)
            return "posted"
        }
        UITestHarness.hooks["sequencer.panelScale"] = { [weak self] args in
            guard let shell = self?.shell, let key = args.first else { return "FAIL needs a key" }
            return String(format: "%.4f", shell.panelScale(key: key))
        }
        // `sequencer.panel` with path [tab title]: chooses that tab, which a click on a window that's never key can't.
        UITestHarness.hooks["sequencer.panel"] = { [weak self] args in
            guard let self, let title = args.first, let panels = self.panels else { return "FAIL needs a tab title" }
            for (side, tabs) in [(ShellSide.left, panels.leftTabs), (ShellSide.right, panels.rightTabs)] {
                if let tab = tabs.first(where: { $0.title == title }) {
                    self.selectPanel(tab.panel, side: side)
                    return "selected \(title)"
                }
            }
            return "FAIL no tab \(title)"
        }
        // `sequencer.libraryMove` with path [color|curve|gradient, selected ids joined by "|", target id or ""]: a panel's reorder by
        // drop, which a script can't drag. Answers with the new order.
        UITestHarness.hooks["sequencer.libraryMove"] = { [weak self] args in
            guard let self, args.count == 3 else { return "FAIL needs kind, selected, target" }
            let kind: LibraryKind
            switch args[0] {
            case "color": kind = .color
            case "curve": kind = .curve
            case "gradient": kind = .gradient
            default: return "FAIL no kind \(args[0])"
            }
            let target = self.entries(kind).first { $0.id == args[2] }
            self.moveSelection(Set(args[1].split(separator: "|").map(String.init)), to: target, kind: kind)
            return self.entries(kind).map(\.name).joined(separator: "|")
        }
        #if canImport(VixenTimeline)
        UITestHarness.hooks["sequencer.picker"] = { ParameterPicker.harness($0) }
        // `sequencer.dropEffect` with path [effect name, row]: as Test Drop Pulse On Row, for any effect.
        UITestHarness.hooks["sequencer.dropEffect"] = { [weak self] args in
            guard let self, args.count == 2, let row = UInt32(args[1]), let type = self.effects.first(where: { $0.name == args[0] }) else { return "FAIL needs an effect name and a row" }
            self.testDropOnRow(typeId: type.typeId, file: nil, row: row)
            return "dropped \(type.name)"
        }
        #endif
        registerLipSyncHooks()
        // `sequencer.bulkMove` with path [start ms, end ms, offset ms, visible only, clip, marks]: F380's OK without its sheet.
        UITestHarness.hooks["sequencer.bulkMove"] = { [weak self] args in
            let numbers = args.prefix(3).compactMap(Double.init)
            guard let self, let shell = self.shell, args.count == 6, numbers.count == 3 else { return "FAIL needs start, end, offset, visible, clip, marks" }
            shell.bulkMove(startMs: numbers[0], endMs: numbers[1], offsetMs: numbers[2], visibleOnly: args[3] == "true", clip: args[4] == "true", processMarks: args[5] == "true")
            self.refresh()
            return "moved"
        }
    }

    /// F1, F5, F6: the window is already up, showing this text, while the file loads on a worker.
    func open() async {
        let started = Date()
        loadingText = Self.loadingMessage(0)
        let ticker = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .milliseconds(250))
                guard let self, !Task.isCancelled else { return }
                self.loadingText = Self.loadingMessage(Date().timeIntervalSince(started))
            }
        }
        defer { ticker.cancel() }

        await app.waitForCore()
        guard let core = app.core else {
            loadingText = nil
            return
        }
        let opened: SequencerSession
        do {
            switch ref {
            case .untitled:
                opened = core.newSequence()
            case .existing(let path):
                if URL(fileURLWithPath: path).pathExtension.lowercased() == "vix", let importer = Vixen2Importer.present {
                    let imported: SequencerSession? = await withCheckedContinuation { continuation in
                        importer(core, path, window) { continuation.resume(returning: $0) }
                    }
                    guard let imported else { loadingText = nil; window?.close(); return }
                    opened = imported
                } else {
                    opened = try await Task.detached { try core.openSequence(path: path) }.value
                }
            }
        } catch {
            loadingText = nil
            await SequencerAlerts.tell(title: "Vixen", message: error.userMessage, style: .warning, window: window)
            window?.close()
            return
        }
        session = opened
        let made = opened.makeShell()
        await resolveUnmappedEffects(made)
        await Task.detached { made.finishLoad() }.value
        #if canImport(VixenTimeline)
        let controller = await Task.detached { TimelineController(session: opened) }.value
        made.attachTimeline(controller: controller)
        shell = made
        canvas = TimelineCanvas(session: opened, controller: controller, delegate: self, loadProfileSettings: true)
        canvas?.registerDropTypes(Self.dropTypes)
        #else
        shell = made
        #endif
        app.editorOpened(self)
        loadingText = nil
        reloadLayout()
        librariesChanged()
        updateClipboardCount()
        startPolling()
        startKeyMonitor()
        await showLoadAlerts(made)
    }

    /// `Please Wait. Loading: <elapsed>`, with `TimeSpan`'s own text.
    private static func loadingMessage(_ seconds: TimeInterval) -> String {
        let total = Int(seconds)
        let fraction = Int((seconds - Double(total)) * 10_000_000)
        return String(format: "Please Wait. Loading: %02d:%02d:%02d.%07d", total / 3600, (total / 60) % 60, total % 60, fraction)
    }

    /// F12: resolve unknown targets before loading the timeline, while their original IDs and names are still available.
    private func resolveUnmappedEffects(_ shell: SequencerShell) async {
        let sources = shell.unmappedTargets()
        guard !sources.isEmpty else { return }
        let wantsMapping = await SequencerAlerts.confirm(
            title: "Unmapped Effects",
            message: "The sequence has Effects that belong to Elements not known to this profile.\nWould you like to map them to existing elements?",
            style: .warning,
            confirm: "Map Elements",
            cancel: "Leave Unmapped",
            window: window
        )
        guard wantsMapping else {
            shell.applyElementMap(map: nil)
            return
        }
        guard let core = app.core, let present = ElementMapper.present else {
            await SequencerAlerts.tell(
                title: "Element Mapper Unavailable",
                message: "This build does not include the Element Mapper. Open the sequence in a build with the scheduler and export tools to map its elements.",
                style: .warning,
                window: window
            )
            shell.applyElementMap(map: nil)
            return
        }
        let result: [ElementMappingResult]? = await withCheckedContinuation { continuation in
            present(core, session?.name() ?? "Untitled", sources.map { ElementMappingSource(id: $0.id, name: $0.name) }, window) { mapped in
                continuation.resume(returning: mapped)
            }
        }
        // Upstream drops the remaining unknown effects even when the mapper is cancelled. An empty map preserves that rule;
        // the session marks the change modified so it cannot silently disappear on close.
        shell.applyElementMap(map: (result ?? []).map { ShellElementMapEntry(source: $0.sourceId, target: $0.targetId) })
    }

    /// F9, F11, F10, in that order.
    private func showLoadAlerts(_ shell: SequencerShell) async {
        if let path = shell.missingAudio() {
            await handleMissingAudio(path: path)
        }
        let report = shell.loadReport()
        if let summary = report.renameSummary {
            await SequencerAlerts.tell(title: "Mark Collections Renamed", message: summary, style: .information, window: window)
        }
        if let message = report.deprecatedMessage {
            await SequencerAlerts.tell(title: "Deprecated Elements", message: message, style: .warning, window: window)
        }
    }

    /// `ShowInvalidAudioDialog`. F9, F383.
    private func handleMissingAudio(path: String) async {
        await askAboutMissingAudio(path: path)
    }

    // MARK: Keeping up

    private func startPolling() {
        pollTask = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .milliseconds(250))
                guard let self, let shell = self.shell, !self.closing else { return }
                let result = shell.poll()
                if result.changed {
                    self.refresh()
                }
                if result.transportActive {
                    self.startFrameTaskIfNeeded()
                }
                if result.autoSaveDue {
                    // `AutoSaveEventProcessor`: a never-saved sequence raises the save panel (KEEP F15).
                    _ = await self.save(forcePanel: false)
                }
            }
        }
    }

    /// The display-rate tick, only while playing or counting down. The timeline's playback layer has its own display link.
    private func startFrameTaskIfNeeded() {
        guard frameTask == nil else { return }
        frameTask = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .milliseconds(33))
                guard let self, let shell = self.shell, !self.closing else { return }
                let changed = shell.transportTick()
                let text = shell.positionText()
                if text != self.positionText {
                    self.positionText = text
                }
                if changed {
                    self.refresh()
                }
                if !(self.ui?.transport.running ?? false), !shell.poll().transportActive {
                    self.frameTask = nil
                    return
                }
            }
        }
    }

    fileprivate func setPositionText(_ text: String) {
        positionText = text
    }

    /// Reads everything the window shows.
    func refresh() {
        guard let shell else { return }
        #if canImport(VixenTimeline)
        if let canvas {
            for update in shell.takeTimelineUpdates() {
                canvas.apply(update)
            }
        }
        #endif
        let state = shell.uiState()
        ui = state
        positionText = state.positionText
        let edits = shell.editCount()
        if edits != editRevision {
            editRevision = edits
        }
        updateWindowChrome()
    }

    /// F164, F350, F361: any library changed. Curves and gradients may have been changed by another window, so this reads them all.
    func librariesChanged() {
        guard let shell else { return }
        let newPalette = shell.palette()
        if newPalette != palette {
            palette = newPalette
        }
        if let core = app.core {
            let newCurves = core.libraryCurves()
            if newCurves != curves {
                curves = newCurves
            }
            let newGradients = core.libraryGradients()
            if newGradients != gradients {
                gradients = newGradients
            }
        }
        refresh()
    }

    private func reloadLayout() {
        toolbars = shell?.toolbars()
        panels = shell?.panels()
        syncFloatingPanels()
    }

    /// F4: the name in the title, the edited dot for ` *`, and the profile as the subtitle.
    private func updateWindowChrome() {
        guard let window else { return }
        let title = ui?.sequenceName ?? {
            if case .existing(let path) = ref { return URL(fileURLWithPath: path).deletingPathExtension().lastPathComponent }
            return "Untitled"
        }()
        if window.title != title {
            window.title = title
        }
        if let profile = ui?.profileName, window.subtitle != profile {
            window.subtitle = profile
        }
        let edited = ui?.modified ?? false
        if window.isDocumentEdited != edited {
            window.isDocumentEdited = edited
        }
        let url = ui?.filePath.map { URL(fileURLWithPath: $0) }
        if window.representedURL != url {
            window.representedURL = url
        }
    }

    // MARK: Commands

    /// Runs a command, then shows whatever it asked for.
    func run(_ command: ShellCommand) {
        guard let shell else { return }
        let outcome = shell.run(command: command)
        afterCommand(outcome)
    }

    func set(_ option: ShellOption, _ on: Bool) {
        shell?.setOption(option: option, on: on)
        refresh()
    }

    func afterCommand(_ outcome: ShellOutcome) {
        refresh()
        if let frameNeeded = ui?.transport.running, frameNeeded || (ui?.transport.icon == .hourglass) {
            startFrameTaskIfNeeded()
        }
        handle(outcome)
    }

    private func handle(_ outcome: ShellOutcome) {
        if let json = outcome.clipboardJson {
            SequencerPasteboard.write(json: json, count: Int(outcome.clipboardCount))
            updateClipboardCount()
        }
        if let alert = outcome.alert {
            Task { await present(alert) }
        }
        switch outcome.request {
        case .clearEffectsSelection:
            effectsSelectionCleared?()
        case .distributeDialog(let count, let startMs, let endMs, let lengthMs):
            sheet = .distribute(DistributeDialogModel(count: Int(count), startMs: startMs, endMs: endMs, lengthMs: lengthMs))
        case nil:
            break
        }
    }

    private func present(_ alert: ShellAlert) async {
        guard let shell else { return }
        if let label = alert.confirmLabel, let action = alert.action {
            let yes = await SequencerAlerts.confirm(
                title: alert.title,
                message: alert.message,
                style: alert.style,
                confirm: label,
                cancel: alert.cancelLabel ?? "Cancel",
                window: window
            )
            if yes {
                afterCommand(shell.runAlertAction(action: action))
            }
        } else {
            await SequencerAlerts.tell(title: alert.title, message: alert.message, style: alert.style, window: window)
        }
    }

    // MARK: Saving, F13 to F16

    /// `SaveSequence`. Returns false when it didn't happen: the panel was cancelled, or the write failed.
    @discardableResult
    func save(forcePanel: Bool) async -> Bool {
        guard let shell, !saving else { return false }
        saving = true
        defer { saving = false }
        var chosen: String?
        switch shell.saveRoute(forcePanel: forcePanel) {
        case .inPlace:
            chosen = nil
        case .panel(let directory, let suggestedName):
            guard let url = await SaveSequencePanel.run(directory: directory, suggestedName: suggestedName, window: window) else {
                // Cancelling returns without clearing modified.
                return false
            }
            chosen = url.path
        }
        do {
            let prepared = try shell.prepareSave(path: chosen)
            let target = prepared.targetPath()
            refresh()
            let failure: (any Error)? = await Task.detached {
                do {
                    try prepared.write()
                    return nil
                } catch {
                    return error
                }
            }.value
            let problem = failure?.userMessage
            shell.finishSave(path: target, succeeded: problem == nil)
            refresh()
            if let problem {
                await SequencerAlerts.tell(title: "Unable to Save", message: problem, style: .error, window: window)
                return false
            }
            return true
        } catch {
            refresh()
            await SequencerAlerts.tell(title: "Unable to Save", message: error.userMessage, style: .error, window: window)
            return false
        }
    }

    /// The window's delegate asks. Anything but an immediate yes finishes on its own, closing the window itself.
    func shouldClose() -> Bool {
        if closing {
            return true
        }
        if closeAnswer != nil {
            return false
        }
        guard let shell else {
            // Still loading: closing gives up on it.
            finishClosing()
            return true
        }
        refresh()
        switch shell.closeAction(appClosing: false) {
        case .close:
            finishClosing()
            return true
        case .saveSilently:
            closeAnswer = Task {
                defer { closeAnswer = nil }
                // KEEP F16: zoom, scroll or a row height change saves at close with no prompt.
                _ = await save(forcePanel: false)
                closeNow()
            }
            return false
        case .prompt(let canCancel):
            closeAnswer = Task {
                defer { closeAnswer = nil }
                let answer = await SequencerAlerts.saveChanges(name: session?.name() ?? "Untitled", canCancel: canCancel, window: window)
                switch answer {
                case .cancel:
                    return
                case .dontSave:
                    closeNow()
                case .save:
                    // F16's FIX: cancelling the panel a never-saved sequence raises keeps the window open.
                    if await save(forcePanel: false) {
                        closeNow()
                    }
                }
            }
            return false
        }
    }

    private func closeNow() {
        finishClosing()
        window?.close()
    }

    /// `EditorClosing`.
    private func finishClosing() {
        guard !closing else { return }
        closing = true
        pollTask?.cancel()
        frameTask?.cancel()
        if let keyMonitor {
            NSEvent.removeMonitor(keyMonitor)
        }
        for panel in floating.values {
            panel.closeProgrammatically()
        }
        floating.removeAll()
        #if canImport(VixenMarks)
        closeLyricConverter()
        #endif
        #if canImport(VixenTimeline)
        canvas?.stop()
        #endif
        if let shell {
            if let problem = shell.finishClose() {
                FileHandle.standardError.write(Data("sequencer settings: \(problem)\n".utf8))
            }
            app.editorClosed(self, filePath: session?.filePath())
        }
        registry.closed(self)
    }

    func windowBecameKey() {
        registry.activate(self)
        librariesChanged()
        reloadLipSyncMaps()
        updateClipboardCount()
    }

    // MARK: Clipboard, F315, F316

    /// `NSPasteboard` has no change notification. This runs on window key, on the Edit menu opening and on the pointer entering the Edit strip, and reads
    /// only when `changeCount` moved.
    func updateClipboardCount() {
        guard let shell else { return }
        let board = SequencerPasteboard.board
        guard board.changeCount != clipboardChangeCount else {
            refresh()
            return
        }
        clipboardChangeCount = board.changeCount
        shell.setClipboardCount(count: UInt32(SequencerPasteboard.count()))
        refresh()
    }

    func cut() {
        guard let shell else { return }
        afterCommand(shell.copyOrCut(cut: true))
    }

    func copy() {
        guard let shell else { return }
        afterCommand(shell.copyOrCut(cut: false))
    }

    func paste(_ mode: ShellPasteMode) {
        guard let shell, let json = SequencerPasteboard.readJSON() else { return }
        afterCommand(shell.paste(json: json, mode: mode))
    }

    // MARK: Export, F37, F137

    /// The Export Sequence dialog is the scheduler and export area's: a sheet on this window over this session, unsaved edits included, which holds
    /// the Effect Preview while it's up as upstream's `PreviewStop` and `ResumePreview` do. A build without that area has nothing to open.
    var canExport: Bool {
        session != nil && SequenceExport.present != nil
    }

    func export() {
        guard let session, let present = SequenceExport.present else { return }
        present(session, window)
    }

    // MARK: Undo

    func undo(_ steps: UInt32 = 1) {
        _ = shell?.undo(steps: steps)
        refresh()
    }

    func redo(_ steps: UInt32 = 1) {
        _ = shell?.redo(steps: steps)
        refresh()
    }

    // MARK: Layout

    func setStrip(_ strip: ShellStrip, visible: Bool) {
        shell?.setStripVisible(strip: strip, visible: visible)
        toolbars = shell?.toolbars()
    }

    func moveStrip(_ strip: ShellStrip, row: Int, index: Int) {
        shell?.moveStrip(strip: strip, row: UInt32(row), index: UInt32(index))
        toolbars = shell?.toolbars()
    }

    func toggleStripSize(_ strip: ShellStrip) {
        shell?.toggleStripSize(strip: strip)
        toolbars = shell?.toolbars()
    }

    func setItem(_ strip: ShellStrip, item: String, visible: Bool) {
        shell?.setStripItemVisible(strip: strip, item: item, visible: visible)
        toolbars = shell?.toolbars()
    }

    func resetStrip(_ strip: ShellStrip) {
        shell?.resetStrip(strip: strip)
        toolbars = shell?.toolbars()
    }

    func isItemVisible(_ stripName: String, _ item: String) -> Bool {
        !(toolbars?.hiddenItems.contains("\(stripName)/\(item)") ?? false)
    }

    func togglePanel(_ panel: ShellPanel) {
        shell?.togglePanel(panel: panel)
        panels = shell?.panels()
        syncFloatingPanels()
    }

    func selectPanel(_ panel: ShellPanel, side: ShellSide) {
        shell?.selectPanel(side: side, panel: panel)
        panels = shell?.panels()
    }

    func setPanelWidths(left: Double?, right: Double?) {
        guard let shell, let current = panels else { return }
        let l = left ?? current.leftWidth
        let r = right ?? current.rightWidth
        guard l != current.leftWidth || r != current.rightWidth else { return }
        shell.setPanelWidths(left: l, right: r)
        panels = shell.panels()
    }

    func detachPanel(_ panel: ShellPanel) {
        shell?.setPanelFloating(panel: panel, floating: true)
        panels = shell?.panels()
        syncFloatingPanels()
    }

    func redockPanel(_ panel: ShellPanel) {
        shell?.setPanelFloating(panel: panel, floating: false)
        panels = shell?.panels()
        syncFloatingPanels()
    }

    /// Floating panels are `NSPanel`s, so they stay beside the window and the key monitor still hears them.
    private func syncFloatingPanels() {
        let wanted = Set((panels?.floating ?? []).map(\.panel))
        for (panel, window) in floating where !wanted.contains(panel) {
            window.closeProgrammatically()
            floating[panel] = nil
        }
        for tab in panels?.floating ?? [] where floating[tab.panel] == nil {
            let window = FloatingPanelWindow(title: tab.title, model: self, panel: tab.panel)
            floating[tab.panel] = window
            window.orderFront(nil)
        }
    }

    // MARK: Keys, section D

    /// A sequencer-wide monitor, inactive in text fields. Menu shortcuts fire first, as upstream's do.
    private func startKeyMonitor() {
        keyMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
            guard let self else { return event }
            return self.handleKey(event) ? nil : event
        }
    }

    /// Returns true when the key was used.
    @discardableResult
    /// `forwarded`: a key the live preview window passes on while it has the keyboard, which only the active editor hears.
    func handleKey(_ event: NSEvent, forwarded: Bool = false) -> Bool {
        if UITestHarness.isActive { testKeyRoute = "entered \(event.charactersIgnoringModifiers ?? "")" }
        guard let shell, !closing else { return false }
        if window?.attachedSheet != nil {
            return false
        }
        if !forwarded {
            // Only this window, or a floating panel while this is the active sequencer.
            let keyWindow = event.window ?? NSApp.keyWindow
            let isOurs = keyWindow === window || (keyWindow is FloatingPanelWindow && registry.active === self)
            guard isOurs else { return false }
            if keyWindow?.firstResponder is NSText {
                return false
            }
            // F66 of the effect editor spec: a focused editor's own keys, such as Delete on a gradient handle, aren't the timeline's.
            if EditorKeys.focusedEditorTakes(event) {
                return false
            }
            // Dispatch document shortcuts to the same scoped methods as the menu. The hidden Redo item does not
            // register Cmd-Y, so this path also supplies that equivalent. Cmd-Shift-S
            // remains the upstream align-to-mark shortcut; Save As is Cmd-Option-S.
            let menuFlags = event.modifierFlags.intersection([.command, .control, .option, .shift])
            let letter = event.charactersIgnoringModifiers?.lowercased()
            if ui != nil, !app.systemConfigSessionActive {
                if letter == "s", menuFlags == .command || menuFlags == [.command, .option] {
                    if UITestHarness.isActive { testKeyRoute = "window save" }
                    Task { _ = await self.save(forcePanel: menuFlags.contains(.option)) }
                    return true
                }
                if letter == "z", menuFlags == .command || menuFlags == [.command, .shift] {
                    if menuFlags.contains(.shift) { redo() } else { undo() }
                    if UITestHarness.isActive { testKeyRoute = "window undo/redo" }
                    return true
                }
                if letter == "y", menuFlags == .command {
                    redo()
                    if UITestHarness.isActive { testKeyRoute = "window redo" }
                    return true
                }
            }
            // Plain window hotkeys and transport function keys go directly to the table. Modified menu
            // shortcuts retain priority (e.g. Cmd-S saves instead of aligning). Focus/sheet guards apply above.
            let transportFunctionKey = [UInt16(96), 100, 101].contains(event.keyCode)
            let hasMenuModifier = !event.modifierFlags.intersection([.command, .control, .option]).isEmpty
            if hasMenuModifier, !transportFunctionKey, NSApp.mainMenu?.performKeyEquivalent(with: event) == true {
                if UITestHarness.isActive { testKeyRoute = "menu" }
                return true
            }
        }
        // Control is a distinct macOS modifier, not the Command mapping of upstream Ctrl.
        // Real menu equivalents had priority above; an unclaimed Control combination
        // must not lose its modifier and become a plain timeline command. This also
        // applies to keys forwarded by the live preview.
        if event.modifierFlags.contains(.control) {
            if UITestHarness.isActive { testKeyRoute = "control passed through" }
            return false
        }
        guard let key = Self.shellKey(for: event) else { return false }
        notePointerForKeys()
        let flags = event.modifierFlags
        let outcome = shell.keyDown(key: key, shift: flags.contains(.shift), cmd: flags.contains(.command), opt: flags.contains(.option))
        if UITestHarness.isActive { testKeyRoute = "table \(key) handled=\(outcome.handled)" }
        afterCommand(outcome)
        return outcome.handled
    }

    private static func shellKey(for event: NSEvent) -> ShellKey? {
        switch Int(event.keyCode) {
        case 115: return .home
        case 119: return .end
        case 116: return .pageUp
        case 121: return .pageDown
        case 126: return .up
        case 125: return .down
        case 123: return .left
        case 124: return .right
        case 49: return .space
        case 53: return .escape
        case 27: return .minus
        case 24: return .equals
        case 51, 117: return .delete
        case 96: return .f5
        case 100: return .f8
        case 101: return .f9
        default:
            guard let letter = event.charactersIgnoringModifiers?.lowercased(), letter.count == 1, letter.first?.isLetter == true else { return nil }
            return .letter(value: letter)
        }
    }

    /// `MousePosition` for the keys that act on the effect under the pointer, and the grid's size. Worked out from what the timeline makes public.
    func notePointerForKeys() {
        #if canImport(VixenTimeline)
        guard let shell, let canvas, let window else { return }
        let container = canvas.view
        let point = testPointer.map { container.convert($0, from: nil) } ?? container.convert(window.mouseLocationOutsideOfEventStream, from: nil)
        let view = canvas.controller.view()
        let settings = canvas.controller.settings()
        let bars = view.waveformHeight + view.rulerHeight + view.marksBarHeight
        let fromTop = container.isFlipped ? point.y : container.bounds.height - point.y
        let left = CGFloat(settings.splitterDistance) + 1
        shell.setGridPointer(x: Int32(point.x - left), y: Int32(fromTop) - bars)
        shell.setGridSize(width: Int32(container.bounds.width - left), height: Int32(container.bounds.height) - bars)
        #endif
    }

    // MARK: Test support

    /// What a test script checks. A file per call, in order, beside the snapshots.
    @ObservationIgnored private var dumpCount = 0

    func writeTestState() {
        guard let shell, let out = ProcessInfo.processInfo.environment["VIXEN_UI_TEST_OUT"] else { return }
        dumpCount += 1
        let folder = URL(fileURLWithPath: out)
        SequencerMenuBar.arrange(inFront: true)
        try? shell.debugStateJson().write(to: folder.appending(path: "state-\(dumpCount).json"), atomically: true, encoding: .utf8)
        try? Self.menuText().write(to: folder.appending(path: "menus-\(dumpCount).txt"), atomically: true, encoding: .utf8)
    }

    /// Every menu item with its state, so a script can check the menu bar against `menu_state` with no screen control.
    private static func menuText() -> String {
        NSApp.mainMenu.map(text(of:)) ?? ""
    }

    static func text(of root: NSMenu) -> String {
        func walk(_ menu: NSMenu, _ depth: Int, _ lines: inout [String]) {
            menu.delegate?.menuNeedsUpdate?(menu)
            menu.update()
            for item in menu.items {
                if item.isSeparatorItem {
                    lines.append(String(repeating: "  ", count: depth) + "---")
                    continue
                }
                var line = String(repeating: "  ", count: depth) + item.title
                if item.state == .on { line += " [on]" }
                if !item.isEnabled { line += " (disabled)" }
                if item.isHidden { line += " (hidden)" }
                if !item.keyEquivalent.isEmpty {
                    let mods = item.keyEquivalentModifierMask
                    var keys = ""
                    if mods.contains(.control) { keys += "⌃" }
                    if mods.contains(.option) { keys += "⌥" }
                    if mods.contains(.shift) { keys += "⇧" }
                    if mods.contains(.command) { keys += "⌘" }
                    line += " <\(keys)\(item.keyEquivalent)>"
                }
                lines.append(line)
                if let submenu = item.submenu {
                    walk(submenu, depth + 1, &lines)
                }
            }
        }
        var lines: [String] = []
        walk(root, 0, &lines)
        return lines.joined(separator: "\n")
    }
}

/// `windowShouldClose` for any way of closing, with SwiftUI's own delegate behind it.
final class SequencerWindowDelegate: NSObject, NSWindowDelegate {
    private weak var model: SequencerModel?
    /// Set once, before any window message can arrive.
    nonisolated(unsafe) private weak var original: NSWindowDelegate?

    init(model: SequencerModel, original: NSWindowDelegate?) {
        self.model = model
        self.original = original
    }

    func windowShouldClose(_ sender: NSWindow) -> Bool {
        guard model?.shouldClose() ?? true else { return false }
        return original?.windowShouldClose?(sender) ?? true
    }

    func windowDidBecomeKey(_ notification: Notification) {
        model?.windowBecameKey()
        original?.windowDidBecomeKey?(notification)
    }

    func windowDidMove(_ notification: Notification) {
        SequencerWindowFrame.save(notification.object as? NSWindow)
        original?.windowDidMove?(notification)
    }

    func windowDidResize(_ notification: Notification) {
        SequencerWindowFrame.save(notification.object as? NSWindow)
        original?.windowDidResize?(notification)
    }

    func windowWillClose(_ notification: Notification) {
        SequencerWindowFrame.save(notification.object as? NSWindow)
        original?.windowWillClose?(notification)
    }

    nonisolated override func responds(to selector: Selector!) -> Bool {
        super.responds(to: selector) || (original?.responds(to: selector) ?? false)
    }

    nonisolated override func forwardingTarget(for selector: Selector!) -> Any? {
        (original?.responds(to: selector) ?? false) ? original : super.forwardingTarget(for: selector)
    }
}

extension AppModel {
    /// A window can open before the profile has.
    func waitForCore() async {
        while core == nil {
            if case .failed = phase {
                return
            }
            try? await Task.sleep(for: .milliseconds(50))
        }
    }
}

#if canImport(VixenTimeline)
extension SequencerModel: TimelineDelegate {
    func timelineSelectionChanged() {
        refresh()
        selectionDebounce?.cancel()
        selectionDebounce = Task { [weak self] in
            try? await Task.sleep(for: .milliseconds(200))
            guard let self, !Task.isCancelled, let canvas = self.canvas else { return }
            let selected = canvas.controller.selectedElements()
            if selected != self.editorSelection {
                self.editorSelection = selected
            }
        }
    }

    func timelineCursorMoved(ticks: Int64) {
        shell?.cursorMoved(ticks: ticks)
        if let text = shell?.positionText(), text != positionText {
            setPositionText(text)
        }
    }

    /// The Edit menu reads the name from Rust, so there's nothing else to tell.
    func timelineUndoRecorded() -> Bool {
        refresh()
        return true
    }

    func timelinePlay(start: Int64, end: Int64?) -> Bool {
        shell?.playBetween(startTicks: start, endTicks: end)
        refresh()
        return true
    }

    func timelineDocumentChanged() {
        refresh()
    }

    func timelineEditorStateModified() {
        shell?.noteEditorStateModified()
    }

    /// F378: Display Setup's tag manager, which saves the system configuration when it closes; then the labels and the hidden row filter again.
    /// Tags aren't part of the sequence, so nothing is marked modified.
    func timelineManageTags() {
        guard let present = ElementTagManager.present else {
            Task {
                await SequencerAlerts.tell(
                    title: "Manage Tags",
                    message: "The tag manager comes from Display Setup, which isn't in this build.",
                    style: .information,
                    window: window
                )
            }
            return
        }
        present(window) { [weak self] saved in
            guard saved, let canvas = self?.canvas else { return }
            canvas.apply(canvas.controller.tagsChanged())
        }
    }

    #if canImport(VixenMarks)
    /// The dictionary is compiled in, so there's always an answer and upstream's "Unable to find Standard Phoneme Dictionary" can't come up.
    func timelinePhonemes(for words: [String]) -> [TimelineWordPhonemes]? {
        phonemes(for: words).map { TimelineWordPhonemes(word: $0.word, phonemes: $0.phonemes) }
    }
    #endif

    func timelineShowEffectMenu(elements: [UInt64], time: Int64, row: UInt32, at point: NSPoint, in view: NSView) {
        showEffectMenu(elements: elements, time: time, row: row, at: point, in: view)
    }

    func timelineValidateDrop(_ info: NSDraggingInfo, time: Int64, row: UInt32?, element: UInt64?) -> NSDragOperation {
        validateDrop(info.draggingPasteboard, row: row, element: element)
    }

    func timelinePerformDrop(_ info: NSDraggingInfo, time: Int64, row: UInt32?, element: UInt64?) -> Bool {
        performDrop(info.draggingPasteboard, at: info.draggingLocation, flags: NSEvent.modifierFlags, time: time, row: row, element: element)
    }
}
#endif
