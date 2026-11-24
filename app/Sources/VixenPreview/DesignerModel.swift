import AppKit
import Observation
import SwiftUI
import UniformTypeIdentifiers
import VixenBridge
import VixenKit

/// Which designer the menu bar acts on: the one whose window is key.
@Observable
final class DesignerRegistry {
    static let shared = DesignerRegistry()

    private(set) var active: PreviewDesignerModel?
    @ObservationIgnored private var open: [PreviewDesignerModel] = []

    private init() {
        NotificationCenter.default.addObserver(forName: NSWindow.didBecomeKeyNotification, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.keyWindowChanged() }
        }
        NotificationCenter.default.addObserver(forName: NSWindow.didResignKeyNotification, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.keyWindowChanged() }
        }
        NotificationCenter.default.addObserver(forName: NSWindow.didBecomeMainNotification, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.keyWindowChanged() }
        }
    }

    func opened(_ model: PreviewDesignerModel) {
        open.append(model)
        keyWindowChanged()
    }

    func windowsChanged() {
        keyWindowChanged()
    }

    func closed(_ model: PreviewDesignerModel) {
        open.removeAll { $0 === model }
        if active === model {
            active = nil
        }
        keyWindowChanged()
    }

    private func keyWindowChanged() {
        let key = NSApp.keyWindow ?? NSApp.mainWindow
        if UITestHarness.isActive, key == nil {
            // Older offscreen fixtures may have no key window; CG-backed key tests use real ownership.
            active = open.last
        } else {
            // The main window when nothing is key, which is what the menu bar acts on with the app in the background.
            let found = open.first { $0.owns(key) }
            if active !== found {
                active = found
            }
        }
        // The Settings menu belongs to this window: it's empty, so hidden, with another one in front, and fills after this.
        WindowMenus.setNeedsArrange()
    }
}

enum DesignerMenuBar {
    /// The Edit menu names the action. The session's stack is the source of truth, so these aren't registered with an `UndoManager`, whose stack clears redo on a new action (F101).
    static func nameUndo(undo: String?, redo: String?) {
        guard let edit = NSApp.mainMenu?.items.first(where: { $0.title == "Edit" })?.submenu else { return }
        for item in edit.items {
            if item.action == Selector(("undo:")) || item.title.hasPrefix("Undo") {
                item.title = undo.map { "Undo \($0.trimmingCharacters(in: .whitespaces))" } ?? "Undo"
            } else if item.action == Selector(("redo:")) || item.title.hasPrefix("Redo") {
                item.title = redo.map { "Redo \($0.trimmingCharacters(in: .whitespaces))" } ?? "Redo"
            }
        }
    }
}

/// What each sheet needs up front.
enum DesignerSheet: Identifiable {
    case resizeBackground
    case locationOffset
    case singleBulb
    case templates
    case createTemplate(shape: UInt32)
    case link(linker: PreviewLinker, shape: UInt32)
    case fixtureLink(shape: UInt32, current: String?)

    var id: String {
        switch self {
        case .resizeBackground: "resize"
        case .locationOffset: "offset"
        case .singleBulb: "single"
        case .templates: "templates"
        case .createTemplate: "create"
        case .link: "link"
        case .fixtureLink: "fixture"
        }
    }
}

/// One Preview Configuration window. The FFI object does the work and the renderer gets everything directly, so this only holds what SwiftUI shows, and each
/// field is set only when its value differs. A drag that returns an empty change invalidates nothing.
@Observable
final class PreviewDesignerModel {
    let previewID: String
    let app: AppModel
    @ObservationIgnored private(set) var designer: PreviewDesigner?
    @ObservationIgnored weak var window: NSWindow?
    @ObservationIgnored weak var canvas: DesignerCanvasView?
    @ObservationIgnored let treeHandle = OutlineHandle()
    /// F82: the tree's menu, keys and drags, from Display Setup. Nil in a build without it, where the tree only selects.
    @ObservationIgnored private(set) var treeEditor: (any ElementTreeEditing)?
    @ObservationIgnored private var windowDelegate: DesignerWindowDelegate?
    @ObservationIgnored private var responder: DesignerResponder?

    private(set) var toolbar = DesignerToolbarState(bulb: false, lock: false, unlock: false, unlockAll: false, align: false, hideLockedEnabled: false, hideLockedActive: false, canUndo: false, canRedo: false, tool: .select)
    private(set) var undoLabels: [String] = []
    private(set) var redoLabels: [String] = []
    private(set) var zoom: Double = 1
    /// Prop Information: session only, off to begin with (F9).
    var showInfo = false {
        didSet { apply(designer?.setShowInfo(show: showInfo)) }
    }
    private(set) var settings: DesignerSettings?
    var backgroundAlpha: Double = 255
    private(set) var problem: String?
    var sheet: DesignerSheet?
    var showElements = true {
        didSet { split?.setElementsVisible(showElements) }
    }
    var showProperties = true {
        didSet { split?.setPropertiesVisible(showProperties) }
    }
    /// F80: a panel taken out of the window into one of its own. Session only, as upstream's dock layout isn't saved.
    private(set) var elementsFloating = false
    private(set) var propertiesFloating = false
    @ObservationIgnored weak var split: DesignerSplitController?
    /// Edit > Undo and Redo reach this from the window and from its floating panels.
    @ObservationIgnored let undoManager = DesignerUndoManager()
    @ObservationIgnored private var panelResponders: [ObjectIdentifier: DesignerResponder] = [:]

    let properties = DesignerPropertiesModel()

    /// What the tool band's ⌘-clicked Single Light is for.
    var pendingToolAfterSheet: DesignerTool?

    /// F51: the pending zoom, the percent the slider says. Applied 250 ms after the last move or at once on mouse up.
    var zoomSliderPercent: Double = 100
    @ObservationIgnored private var zoomDebounce: Task<Void, Never>?
    /// ClipboardPopulated is static upstream, so a new window still offers Paste.
    @ObservationIgnored private static var clipboardPopulated = false
    @ObservationIgnored private var pendingPanel: PanelChange?
    @ObservationIgnored private var panelFlushScheduled = false
    @ObservationIgnored private var propertiesDirty = false
    @ObservationIgnored var mouseIsDown = false
    @ObservationIgnored private var closing = false
    @ObservationIgnored private var allowedToClose = false
    @ObservationIgnored private var ignoringTreeSelection = 0
    @ObservationIgnored private let templateMemory = ElementTemplateMemory()
    private var runningTemplate = false
    /// Shown as a small spinner in the toolbar while a prop file is read and built.
    private(set) var importingProp = false

    private enum PanelChange {
        case show(UInt32)
        case clear
    }

    /// From the window's root appearing until it had a session, a renderer and its first change. For measuring.
    @ObservationIgnored private(set) var openMilliseconds = 0.0
    @ObservationIgnored private let createdAt = DispatchTime.now().uptimeNanoseconds

    init(previewID: String, app: AppModel) {
        self.previewID = previewID
        self.app = app
        undoManager.model = self
    }

    /// The window, a sheet on it, or one of its floating panels.
    func owns(_ window: NSWindow?) -> Bool {
        guard let window else { return false }
        return window === self.window || window.sheetParent === self.window || split?.floating.values.contains { $0 === window } == true
    }

    // MARK: Floating panels, F80

    func setFloating(_ kind: DesignerPanelKind, _ on: Bool) {
        split?.setFloating(kind, on)
    }

    func floatingChanged() {
        let elements = split?.floating[.elements] != nil
        let properties = split?.floating[.properties] != nil
        if elementsFloating != elements {
            elementsFloating = elements
        }
        if propertiesFloating != properties {
            propertiesFloating = properties
        }
    }

    /// The Edit actions and the menu bar follow the designer while one of its panels has the keyboard.
    func panelOpened(_ panel: NSPanel) {
        let responder = DesignerResponder(model: self)
        responder.nextResponder = panel.nextResponder
        panel.nextResponder = responder
        panelResponders[ObjectIdentifier(panel)] = responder
        DesignerRegistry.shared.windowsChanged()
    }

    func panelClosed(_ panel: NSPanel) {
        if let responder = panelResponders.removeValue(forKey: ObjectIdentifier(panel)) {
            panel.nextResponder = responder.nextResponder
        }
        DesignerRegistry.shared.windowsChanged()
    }

    // MARK: Opening

    /// F2. The canvas is in its window by now, so its layer is final.
    func open(in canvas: DesignerCanvasView, window: NSWindow) {
        guard let core = app.core else {
            problem = "The show hasn't finished loading."
            return
        }
        self.canvas = canvas
        attach(window: window)
        let size = canvas.metal.pixelSize
        do {
            let opened = try core.openPreviewDesigner(
                id: previewID,
                metalLayer: canvas.metal.layerAddress,
                widthPx: size.width,
                heightPx: size.height,
                scale: canvas.metal.scale,
                widthPt: Int32(canvas.bounds.width),
                heightPt: Int32(canvas.bounds.height)
            )
            designer = opened
            treeEditor = ElementTreeEditor.make?(ElementTreeHost(
                app: app,
                window: { [weak self] in self?.window },
                collapseAll: { [weak self] in self?.treeHandle.collapseAll() },
                changed: { [weak self] in await self?.elementsEdited($0) }
            ))
            opened.setClipboardPopulated(on: Self.clipboardPopulated)
            canvas.model = self
            apply(opened.takeInitialChange())
            refreshSettings()
            backgroundAlpha = Double(settings?.backgroundAlpha ?? 255)
            if studioInPane { window.makeFirstResponder(canvas) } else { restoreBounds(window) } // STUDIO: designer-in-pane keeps the show window's bounds and gives the canvas the keyboard
            DesignerRegistry.shared.opened(self)
            if let message = opened.takeVerifyMessage() {
                alert("Invalid Prop Locations", message)
            }
            openMilliseconds = Double(DispatchTime.now().uptimeNanoseconds - createdAt) / 1e6
            PreviewHarness.designerOpened(self)
            Task { await treeHandle.loadRoots() }
            Task.detached { [opened] in
                let failure = opened.loadBackground()
                await MainActor.run { [weak self] in
                    self?.backgroundLoaded(failure)
                }
            }
        } catch {
            problem = error.userMessage
        }
    }

    private func attach(window: NSWindow) {
        self.window = window
        window.tabbingMode = .disallowed
        let proxy = DesignerWindowDelegate(model: self, original: window.delegate)
        windowDelegate = proxy
        window.delegate = proxy
        let responder = DesignerResponder(model: self)
        responder.nextResponder = window.nextResponder
        window.nextResponder = responder
        self.responder = responder
    }

    /// F5: only if the top left or top right corner is on a screen, which the owner's Windows values usually aren't. Under test the harness
    /// keeps windows where it puts them.
    private func restoreBounds(_ window: NSWindow) {
        guard !UITestHarness.isActive, let saved = designer?.restorableSetupBounds(screens: PreviewCoordinates.workingAreas()) else { return }
        let origin = PreviewCoordinates.origin(x: saved.x, y: saved.y, height: CGFloat(saved.height))
        window.setFrame(NSRect(x: origin.x, y: origin.y, width: CGFloat(saved.width), height: CGFloat(saved.height)), display: true)
    }

    private func backgroundLoaded(_ failure: String?) {
        if let failure {
            alert("Error", failure)
        }
        refreshSettings()
        apply(designer?.viewResized(widthPt: Int32(canvas?.bounds.width ?? 0), heightPt: Int32(canvas?.bounds.height ?? 0)))
    }

    func refreshSettings() {
        guard let settings = designer?.settings() else { return }
        self.settings = settings
    }

    // MARK: Changes

    /// Everything comes through here. A drag's empty change returns almost at once.
    func apply(_ change: DesignerChange?) {
        guard let c = change else { return }
        if let cursor = c.cursor {
            canvas?.setCursor(cursor)
        }
        if let size = c.contentSize {
            canvas?.setDocumentSize(width: size.x, height: size.y)
        }
        if let scroll = c.scroll {
            canvas?.scrollTo(x: scroll.x, y: scroll.y)
        }
        if let labels = c.labels {
            canvas?.showLabels(labels)
        }
        if let zoom = c.zoom, zoom != self.zoom {
            zoomChanged(zoom)
        }
        if let toolbar = c.toolbar, toolbar != self.toolbar {
            self.toolbar = toolbar
        }
        if let tool = c.tool, tool != toolbar.tool {
            toolbar.tool = tool
        }
        if c.undoChanged {
            refreshUndoLabels()
        }
        if let text = c.clipboard {
            Self.clipboardPopulated = true
            designer?.setClipboardPopulated(on: true)
            let board = Self.pasteboard
            board.clearContents()
            board.setString(text, forType: .string)
        }
        if c.elementsDeselected {
            // The session has already cleared its own, and the tree telling it so would clear the panel it's about to show.
            ignoringTreeSelection += 1
            Task {
                await treeHandle.select([])
                ignoringTreeSelection -= 1
            }
        }
        // F90, hazard 22: the panel never holds up a drag. A short drag doesn't build and tear down a form.
        if let shape = c.showProperties {
            pendingPanel = .show(shape)
            schedulePanelFlush()
        }
        if c.clearProperties {
            pendingPanel = .clear
            schedulePanelFlush()
        }
        if c.refreshProperties {
            propertiesDirty = true
            if !mouseIsDown {
                schedulePanelFlush()
            }
        }
        if let message = c.message {
            alert(alertTitle(for: message), message)
        }
        if c.lockHint {
            alert("How to Unlock a Prop", "Press the shift key in combination with the mouse to reselect a locked prop.")
        }
        if let request = c.elementTemplate {
            elementTemplateRequested(shape: request.shape, name: request.templateName)
        }
    }

    private func alertTitle(for message: String) -> String {
        if message.hasPrefix("You cannot create a group") || message.hasPrefix("Grouping or creating") {
            return "Grouping Error"
        }
        if message.hasPrefix("You can only match") {
            return "Match Properties"
        }
        if message.hasPrefix("The '") {
            return "Data Entry Error"
        }
        return "Preview"
    }

    static var pasteboard: NSPasteboard {
        UITestHarness.isActive ? NSPasteboard(name: NSPasteboard.Name("VixenPreviewTest")) : .general
    }

    private func schedulePanelFlush() {
        guard !panelFlushScheduled else { return }
        panelFlushScheduled = true
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            panelFlushScheduled = false
            flushPanel()
        }
    }

    private func flushPanel() {
        guard let designer else { return }
        switch pendingPanel {
        case .show(let shape):
            properties.show(shape: shape, designer: designer, model: self)
        case .clear:
            properties.clear()
        case nil:
            if propertiesDirty, !mouseIsDown {
                properties.reload()
            }
        }
        pendingPanel = nil
        propertiesDirty = false
    }

    func mouseWentUp() {
        mouseIsDown = false
        if propertiesDirty {
            schedulePanelFlush()
        }
    }

    private func refreshUndoLabels() {
        guard let designer else { return }
        let undo = designer.undoLabels()
        let redo = designer.redoLabels()
        if undo != undoLabels {
            undoLabels = undo
        }
        if redo != redoLabels {
            redoLabels = redo
        }
        DesignerMenuBar.nameUndo(undo: undo.first, redo: redo.first)
    }

    // MARK: Zoom, F51

    private func zoomChanged(_ value: Double) {
        zoom = value
        zoomDebounce?.cancel()
        zoomSliderPercent = (value * 100).rounded()
        canvas?.layoutLabels()
    }

    /// The slider moved: the label follows at once and the zoom 250 ms after it stops.
    func zoomSliderMoved(_ percent: Double) {
        zoomSliderPercent = percent
        zoomDebounce?.cancel()
        zoomDebounce = Task { [weak self] in
            try? await Task.sleep(for: .milliseconds(250))
            guard !Task.isCancelled else { return }
            self?.applyPendingZoom()
        }
    }

    /// `ApplyPendingZoom`: moves under 10 percentage points do nothing, and the label then disagrees with the zoom. Upstream's quirk, kept until the owner decides (Q5). It also
    /// stops the slider's echo of a wheel zoom from rounding that zoom to a whole percent.
    func applyPendingZoom() {
        zoomDebounce?.cancel()
        let pending = zoomSliderPercent / 100
        guard abs(zoom - pending) >= 0.1 else { return }
        zoom = pending
        apply(designer?.setZoomLevel(zoom: pending))
        canvas?.layoutLabels()
    }

    // MARK: Tool band

    func choose(_ tool: DesignerTool) {
        apply(designer?.setTool(tool: tool))
    }

    /// F24: ⌘-click on Single Light opens its dialog first, and Cancel still activates the tool, with default naming.
    func chooseSingleLight(commandHeld: Bool) {
        if commandHeld {
            sheet = .singleBulb
        } else {
            choose(.single)
        }
    }

    func singleBulbFinished(prefix: String?, index: Int32, size: Int32) {
        choose(.single)
        // Upstream only sets these with a prefix, so OK with an emptied one also throws away the Light Size.
        if let prefix, !prefix.isEmpty {
            designer?.setSingleItemDefaults(name: prefix, startIndex: index, bulbSize: size)
        }
    }

    func align(_ kind: DesignerAlign) {
        apply(designer?.align(kind: kind))
    }

    func undo(steps: UInt32 = 1) {
        apply(designer?.studioUndoMany(steps: steps)) // STUDIO: designer-live-dialogs undo may load the background again
    }

    func redo(steps: UInt32 = 1) {
        apply(designer?.studioRedoMany(steps: steps)) // STUDIO: designer-live-dialogs redo may load the background again
    }

    func toggleHideLocked() {
        apply(designer?.setHideLocked(hide: !toolbar.hideLockedActive))
    }

    // MARK: Clipboard, F31 to F34

    func copy() {
        apply(designer?.copy())
    }

    func cut() {
        apply(designer?.cut())
    }

    /// F32: from the pasteboard, wherever the command came from, with the pointer's real position first, because the pasted shapes stick to it.
    func paste() {
        guard let designer else { return }
        if let point = canvas?.pointerInCanvas() {
            apply(designer.mouseMove(x: point.x, y: point.y, mods: canvas?.currentMods() ?? DesignerMods(command: false, shift: false, option: false)))
        }
        let text = Self.pasteboard.string(forType: .string) ?? ""
        apply(designer.pasteText(text: text))
    }

    func deleteSelection() {
        apply(designer?.deleteSelection())
    }

    func selectAll() {
        apply(designer?.selectAll())
    }

    // MARK: Background, F23

    func chooseBackground() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.jpeg, .png, .gif, .bmp, .image, .data]
        panel.allowsMultipleSelection = false
        guard let window else { return }
        panel.beginSheetModal(for: window) { [weak self] response in
            guard response == .OK, let url = panel.url else { return }
            MainActor.assumeIsolated { self?.setBackground(path: url.path) }
        }
    }

    func setBackground(path: String) {
        guard let designer else { return }
        Task {
            do {
                let change = try await Task.detached { try designer.chooseBackground(path: path) }.value
                apply(change)
                backgroundAlpha = 255
                refreshSettings()
            } catch {
                alert("Error", error.userMessage)
            }
        }
    }

    func backgroundAlphaChanged(_ value: Double) {
        backgroundAlpha = value
        designer?.setBackgroundAlpha(alpha: Int32(value))
    }

    /// F14, on OK.
    func resizeBackground(width: Int32, height: Int32, scaleShapes: Bool) {
        guard let designer else { return }
        guard height > 10, width > 10 else {
            alert("Invalid Size", "An invalid image size was specified!")
            return
        }
        Task {
            do {
                let change = try await Task.detached { try designer.studioResizeBackground(width: width, height: height, scaleShapes: scaleShapes) }.value // STUDIO: undoable resize
                apply(change)
                refreshSettings()
            } catch {
                alert("Error", error.userMessage)
            }
        }
    }

    // MARK: Elements, F83

    /// `primary` is the row last acted on, which is what a new shape links to, so it goes first.
    func elementsSelected(_ selection: OutlineSelection) {
        guard ignoringTreeSelection == 0 else { return }
        var ordered = selection.ids
        if let primary = selection.primary, let at = ordered.firstIndex(of: primary) {
            ordered.remove(at: at)
            ordered.insert(primary, at: 0)
        }
        apply(designer?.setSelectedElements(nodes: ordered))
    }

    /// F29: the tool's wizard, once the shape is drawn. Cancelled, or in a build without Display Setup, the shape stays unlinked.
    private func elementTemplateRequested(shape: UInt32, name: String) {
        if UITestHarness.isActive {
            UITestHarness.note("element template asked for: \(name)")
        }
        // Upstream makes a new template object each time here, so nothing is remembered between shapes.
        Task { await runElementTemplate(name: name, shape: shape, memory: nil) }
    }

    /// F81's Add button, whose templates live as long as the window and remember what was typed.
    func addElementTemplate(name: String) async {
        await runElementTemplate(name: name, shape: nil, memory: templateMemory)
    }

    var canRunElementTemplates: Bool { ElementTemplateRunner.run != nil && !runningTemplate }

    /// The wizard works on its own copy of the display and applies it without saving. Then the tree shows what was made, a drawn shape links to it
    /// (F30a: one copy of the shape for each further fixture), and the tree selects it.
    private func runElementTemplate(name: String, shape: UInt32?, memory: ElementTemplateMemory?) async {
        guard let designer, let window, let run = ElementTemplateRunner.run, !runningTemplate else { return }
        runningTemplate = true
        defer { runningTemplate = false }
        guard let made = await run(name, window, app, memory) else { return }
        let change = designer.elementTemplateFinished(shape: shape, nodes: made.nodes, selection: made.selection)
        let rows = designer.elementPaths(nodes: made.selection)
        ignoringTreeSelection += 1
        await treeHandle.reload(roots: true, parents: { _ in true })
        await treeHandle.select(rows, expanding: rows.compactMap(Self.parentRow))
        ignoringTreeSelection -= 1
        apply(change)
        if UITestHarness.isActive {
            UITestHarness.note("element template made \(made.nodes.count) node(s), selected \(rows.count)")
        }
    }

    private static func parentRow(_ row: String) -> String? {
        row.lastIndex(of: "/").map { String(row[..<$0]) }
    }

    /// F82 and F85: the tree was edited and the display has it. Links to elements that went are dropped, and the tree shows what the edit says.
    private func elementsEdited(_ edit: ElementTreeChange) async {
        guard let designer else { return }
        let change = await Task.detached { designer.reloadElements() }.value
        ignoringTreeSelection += 1
        await treeHandle.reload(roots: true, parents: { _ in true })
        if let select = edit.select {
            await treeHandle.select(select, expanding: edit.expand)
        } else if !edit.expand.isEmpty {
            await treeHandle.expand(edit.expand)
        }
        ignoringTreeSelection -= 1
        apply(change)
        if edit.select != nil {
            elementsSelected(treeNodes(treeHandle.selection))
        }
        if UITestHarness.isActive {
            UITestHarness.note("elements edited: selected \(treeHandle.selection.ids.count), linked lights \(designer.linkedLightCountForTest())")
        }
    }

    /// The tree's rows are paths. The session wants the nodes at their ends.
    func treeNodes(_ selection: OutlineSelection) -> OutlineSelection {
        func node(_ row: String) -> String { row.split(separator: "/").last.map(String.init) ?? row }
        return OutlineSelection(ids: selection.ids.map(node), primary: selection.primary.map(node))
    }

    func elementMenu() -> NSMenu? {
        let selection = treeHandle.selection
        return treeEditor?.menu(rows: treeHandle.selectionInDisplayOrder, primary: selection.primary)
    }

    func canPerformOnElements(_ command: OutlineCommand) -> Bool {
        treeEditor?.canPerform(command, rows: treeHandle.selectionInDisplayOrder) ?? false
    }

    func performOnElements(_ command: OutlineCommand) {
        treeEditor?.perform(command, rows: treeHandle.selectionInDisplayOrder, primary: treeHandle.selection.primary)
    }

    // MARK: Custom props, F7, F58

    /// F58: there's no real library behind this, upstream or here.
    func openCustomPropLibrary() {
        let documents = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
        if UITestHarness.isActive {
            UITestHarness.note("library: \(documents.path)")
        } else {
            NSWorkspace.shared.open(documents)
        }
    }

    /// F7: a prop file from the Custom Prop Editor, placed at (20, 20).
    func importProp() {
        guard let window, !importingProp else { return }
        let panel = NSOpenPanel()
        panel.message = "Import Custom Prop"
        panel.allowedContentTypes = [UTType(filenameExtension: "prp") ?? .data]
        panel.allowsMultipleSelection = false
        if let root = app.dataRoot {
            panel.directoryURL = URL(fileURLWithPath: root)
        }
        panel.beginSheetModal(for: window) { [weak self] response in
            guard response == .OK, let url = panel.url else { return }
            MainActor.assumeIsolated {
                guard let self else { return }
                Task { await self.importPropFiles([url.path], x: 20, y: 20, atDrop: false) }
            }
        }
    }

    /// F40, F41, F57. Each file's elements join the running display unsaved, as a template's do, and its shape lands at `x`, `y`. `answer` stands in
    /// for the naming prompt under test. Upstream adds no undo step for an import, so there's none here.
    func importPropFiles(_ paths: [String], x: Int32, y: Int32, atDrop: Bool, answer: ((String) -> String)? = nil) async {
        guard let designer, !importingProp else { return }
        importingProp = true
        defer { importingProp = false }
        for path in paths {
            let file: PreviewPropFile
            do {
                file = try await Task.detached { try designer.openPropFile(path: path) }.value
            } catch {
                if UITestHarness.isActive {
                    UITestHarness.note("prop import failed: \(error.userMessage)")
                } else {
                    _ = await ask("Prop Load Error!", "An error occurred loading the prop.", ["OK"])
                }
                continue
            }
            var answers: [String: String] = [:]
            for token in file.tokens() {
                if let answer {
                    answers[token] = answer(token)
                } else {
                    answers[token] = await askText("Prop Naming", "Enter token replacement value for \(token).", initial: "1")
                }
            }
            let started = ContinuousClock.now
            let given = answers
            let made = await Task.detached { designer.importProp(file: file, answers: given, x: x, y: y, atDrop: atDrop) }.value
            treeEditor?.invalidate()
            let rows = designer.elementPaths(nodes: [made.root])
            ignoringTreeSelection += 1
            await treeHandle.reload(roots: true, parents: { _ in true })
            await treeHandle.select(rows, expanding: [])
            ignoringTreeSelection -= 1
            apply(made.change)
            if answer == nil, let window, let setup = ElementPropSetup.run {
                let color: ImportedPropColor = switch file.colorMode() {
                case .fullColor: .fullColor
                case .multiple: .multiple
                case .single: .single
                case .other: .other
                }
                // F44: the helpers work on a copy of the display and apply it, so links and discrete colours are read again afterwards.
                if await setup(made.lightNodes, color, window, app) {
                    treeEditor?.invalidate()
                    let change = await Task.detached { designer.refreshImportedPropColors(root: made.root) }.value
                    ignoringTreeSelection += 1
                    await treeHandle.reload(roots: true, parents: { _ in true })
                    await treeHandle.select(rows, expanding: [])
                    ignoringTreeSelection -= 1
                    apply(change)
                    if UITestHarness.isActive {
                        UITestHarness.note("prop setup helpers ran, linked lights \(designer.linkedLightCountForTest())")
                    }
                }
            }
            if UITestHarness.isActive {
                let took = ContinuousClock.now - started
                let ms = Double(took.components.seconds) * 1000 + Double(took.components.attoseconds) / 1e15
                UITestHarness.note("prop imported: \(made.lightNodes.count) light element(s), tokens \(answers.keys.sorted()), selected \(rows.count), \(String(format: "%.1f", ms)) ms")
            }
        }
    }

    // MARK: Context menu actions, F37

    func perform(_ item: DesignerMenuItem) {
        switch item {
        case .ungroup: apply(designer?.ungroup())
        case .group: apply(designer?.group())
        case .createTemplate: createTemplate()
        case .cut: cut()
        case .copy: copy()
        case .paste: paste()
        case .delete: deleteSelection()
        case .undo: undo()
        case .redo: redo()
        case .lock: apply(designer?.lock())
        case .unlock: apply(designer?.unlock())
        case .setZ: break
        }
    }

    /// F39: the group exists while the name dialog is up, and a refused selection never sees the dialog.
    private func createTemplate() {
        guard let designer else { return }
        let change = designer.group()
        apply(change)
        guard change.message == nil, let shape = designer.selectedShape() else { return }
        sheet = .createTemplate(shape: shape)
    }

    func openLinker(shape: UInt32) {
        guard let linker = designer?.openLinker(shape: shape) else { return }
        sheet = .link(linker: linker, shape: shape)
    }

    func help(_ address: String) {
        if UITestHarness.isActive {
            UITestHarness.note("help: \(address)")
        } else if let url = URL(string: address) {
            NSWorkspace.shared.open(url)
        }
    }

    // MARK: Saving and closing

    /// F6.
    func save() {
        guard let designer else { return }
        Task {
            do {
                try await Task.detached { try designer.save() }.value
                // The save wrote this preview into the display the editor's copy was made from.
                treeEditor?.invalidate()
            } catch {
                alert("Error", error.userMessage)
            }
        }
    }

    /// F4 and F20, in that order, as the close button and ⌘W get them.
    func shouldClose() -> Bool {
        if allowedToClose {
            return true
        }
        guard let designer, !closing else { return designer == nil }
        closing = true
        Task { await confirmClose(designer) }
        return false
    }

    // STUDIO-BEGIN designer-in-pane: the prompts and write-back are shared with leaving the designer in the show window's Previews pane
    private func confirmClose(_ designer: PreviewDesigner) async {
        guard await confirmAndCommit(designer) else {
            closing = false
            return
        }
        allowedToClose = true
        window?.performClose(nil)
    }

    /// Set when the show window's Previews pane hosts this designer instead of a window of its own.
    @ObservationIgnored var studioInPane = false

    /// The pane's back button: the close prompts and the write-back, then the designer goes and the show window gets its
    /// own delegate and responders back. False: the user kept editing.
    func studioLeave() async -> Bool {
        guard let designer else {
            app.previewDesignerClosed(id: previewID)
            return true
        }
        guard !closing else { return false }
        closing = true
        guard await confirmAndCommit(designer) else {
            closing = false
            return false
        }
        studioDetach()
        windowWillClose()
        return true
    }

    /// Quitting with the designer in the pane: the write-back without the prompts.
    func studioFlush() async {
        guard let designer, !closing else { return }
        closing = true
        await Task.detached { designer.commit() }.value
        studioDetach()
        windowWillClose()
    }

    private func studioDetach() {
        guard let window else { return }
        if let windowDelegate, window.delegate === windowDelegate {
            window.delegate = windowDelegate.studioOriginal
        }
        if let responder, window.nextResponder === responder {
            window.nextResponder = responder.nextResponder
        }
        windowDelegate = nil
        responder = nil
        DesignerMenuBar.nameUndo(undo: nil, redo: nil)
    }

    /// F4's prompts and F21's write-back. False when the user chose to keep editing.
    private func confirmAndCommit(_ designer: PreviewDesigner) async -> Bool {
        let invalid = designer.invalidLinkShapes()
        if !invalid.isEmpty {
            apply(designer.selectShapes(shapes: invalid))
            let answer = await ask(
                "Invalid Element Links",
                "This preview contains display item(s) that have invalid Linked Elements. They have been selected in the preview.\n\nPlease update the Linked Elements in the Properties pane to ensure they are correct, or those items will not display in the preview.",
                ["Close Anyway", "Keep Editing"]
            )
            if answer != "Close Anyway" {
                return false
            }
        }
        if designer.fixturesWithoutOpenGl() {
            let answer = await ask(
                "Change Preview Viewer",
                "This preview contains Intelligent Fixtures and the 'Use OpenGL Preview' option is not selected. Intelligent Fixtures are not supported on the flat preview.",
                ["Use OpenGL Preview", "Keep Flat Preview"]
            )
            if answer == "Use OpenGL Preview" {
                designer.setUseOpenGl(on: true)
            }
        }
        // F21: off the main thread, since a big show has many pixels. The window is on its way out by then, so nothing here waits.
        await Task.detached { designer.commit() }.value
        return true
    }
    // STUDIO-END designer-in-pane

    /// The window is going. SwiftUI's own delegate still runs after this.
    func windowWillClose() {
        split?.closeFloatingPanels()
        guard let designer else { return }
        DesignerRegistry.shared.closed(self)
        self.designer = nil
        // The renderer's thread can take a moment to wind down, and this is the main thread.
        Task.detached { designer.close() }
        let id = previewID
        app.previewDesignerClosed(id: id)
        LivePreviewCoordinator.shared?.designerClosed(id: id)
    }

    // MARK: Alerts

    func alert(_ title: String, _ text: String) {
        Task { _ = await ask(title, text, ["OK"]) }
    }

    /// Upstream's `TextDialog`: Cancel gives an empty answer.
    func askText(_ title: String, _ text: String, initial: String) async -> String {
        guard let window else { return initial }
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = text
        let field = NSTextField(string: initial)
        field.frame = NSRect(x: 0, y: 0, width: 240, height: 22)
        alert.accessoryView = field
        alert.addButton(withTitle: "Continue") // STUDIO: a verb on the confirmation button
        alert.addButton(withTitle: "Cancel")
        alert.window.initialFirstResponder = field
        return await withCheckedContinuation { finished in
            alert.beginSheetModal(for: window) { response in
                finished.resume(returning: response == .alertFirstButtonReturn ? field.stringValue : "")
            }
        }
    }

    func ask(_ title: String, _ text: String, _ buttons: [String]) async -> String {
        guard let window else { return buttons.first ?? "" }
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = text
        for button in buttons {
            alert.addButton(withTitle: button)
        }
        return await withCheckedContinuation { finished in
            alert.beginSheetModal(for: window) { response in
                let index = response.rawValue - NSApplication.ModalResponse.alertFirstButtonReturn.rawValue
                finished.resume(returning: buttons.indices.contains(index) ? buttons[index] : "")
            }
        }
    }
}

/// `windowShouldClose` for any way of closing, with SwiftUI's own delegate behind it.
final class DesignerWindowDelegate: NSObject, NSWindowDelegate {
    private weak var model: PreviewDesignerModel?
    /// Set once, before any window message can arrive.
    nonisolated(unsafe) private weak var original: NSWindowDelegate?
    var studioOriginal: NSWindowDelegate? { original } // STUDIO: designer-in-pane hands the show window its delegate back
    private var boundsSave: Task<Void, Never>?

    init(model: PreviewDesignerModel, original: NSWindowDelegate?) {
        self.model = model
        self.original = original
    }

    func windowShouldClose(_ sender: NSWindow) -> Bool {
        guard model?.shouldClose() ?? true else { return false }
        return original?.windowShouldClose?(sender) ?? true
    }

    /// NSWindow's own Undo and Redo go to this, which goes to the session unless a text field is editing.
    func windowWillReturnUndoManager(_ window: NSWindow) -> UndoManager? {
        model?.undoManager
    }

    func windowWillClose(_ notification: Notification) {
        model?.windowWillClose()
        original?.windowWillClose?(notification)
    }

    /// F5: the same four fields upstream writes, top left and y down, so the file stays compatible.
    private func reportBounds(_ window: NSWindow) {
        guard !UITestHarness.isActive, model?.studioInPane != true else { return } // STUDIO: designer-in-pane doesn't save the show window's frame as the preview's
        let frame = window.frame
        let top = PreviewCoordinates.topLeft(ofFrame: frame)
        model?.designer?.setSetupBounds(left: top.x, top: top.y, width: Int32(frame.width), height: Int32(frame.height))
    }

    func windowDidMove(_ notification: Notification) {
        if let window = notification.object as? NSWindow {
            reportBounds(window)
        }
        original?.windowDidMove?(notification)
    }

    func windowDidEndLiveResize(_ notification: Notification) {
        if let window = notification.object as? NSWindow {
            reportBounds(window)
        }
        model?.canvas?.liveResizeEnded()
        original?.windowDidEndLiveResize?(notification)
    }

    nonisolated override func responds(to selector: Selector!) -> Bool {
        super.responds(to: selector) || (original?.responds(to: selector) ?? false)
    }

    nonisolated override func forwardingTarget(for selector: Selector!) -> Any? {
        (original?.responds(to: selector) ?? false) ? original : super.forwardingTarget(for: selector)
    }
}

/// The window's undo manager. Edit > Undo and Redo reach NSWindow wherever the keyboard is, as upstream's menu items act whatever has
/// the focus, and this sends them to the session. A text field that's editing keeps its own undo. The session's stack is the only one, so
/// nothing is ever registered here except by text fields (F101).
final class DesignerUndoManager: UndoManager {
    nonisolated(unsafe) weak var model: PreviewDesignerModel?

    private nonisolated var routes: Bool {
        MainActor.assumeIsolated {
            guard let model else { return false }
            let focused = NSApp.keyWindow.flatMap { model.owns($0) ? $0 : nil } ?? model.window
            return !(focused?.firstResponder is NSText)
        }
    }

    nonisolated override var canUndo: Bool {
        routes ? MainActor.assumeIsolated { model?.toolbar.canUndo ?? false } : super.canUndo
    }

    nonisolated override var canRedo: Bool {
        routes ? MainActor.assumeIsolated { model?.toolbar.canRedo ?? false } : super.canRedo
    }

    nonisolated override func undo() {
        if routes {
            MainActor.assumeIsolated { model?.undo() }
        } else {
            super.undo()
        }
    }

    nonisolated override func redo() {
        if routes {
            MainActor.assumeIsolated { model?.redo() }
        } else {
            super.redo()
        }
    }

    nonisolated override var undoActionName: String {
        routes ? MainActor.assumeIsolated { model?.undoLabels.first?.trimmingCharacters(in: .whitespaces) ?? "" } : super.undoActionName
    }

    nonisolated override var redoActionName: String {
        routes ? MainActor.assumeIsolated { model?.redoLabels.first?.trimmingCharacters(in: .whitespaces) ?? "" } : super.redoActionName
    }
}

/// The standard Edit actions, for whichever view has the keyboard and doesn't take them itself. A text field keeps its own, because it's earlier in the chain.
final class DesignerResponder: NSResponder, NSMenuItemValidation {
    private weak var model: PreviewDesignerModel?

    init(model: PreviewDesignerModel) {
        self.model = model
        super.init()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

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

    /// F11: only undo, redo and the lock family are ever off, so the rest stay on.
    func validateMenuItem(_ item: NSMenuItem) -> Bool {
        guard let model else { return false }
        switch item.action {
        case Selector(("undo:")):
            item.title = model.undoLabels.first.map { "Undo \($0.trimmingCharacters(in: .whitespaces))" } ?? "Undo"
            return model.toolbar.canUndo
        case Selector(("redo:")):
            item.title = model.redoLabels.first.map { "Redo \($0.trimmingCharacters(in: .whitespaces))" } ?? "Redo"
            return model.toolbar.canRedo
        default:
            return true
        }
    }
}
