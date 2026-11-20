import AppKit
import SwiftUI
import VixenBridge

/// The Curve Editor dialog, F70 to F78, as a sheet. Everything it does happens on its own copy, in Rust; `onOK` gets the result and Cancel does nothing. `title` is the row's display name when
/// there is one: Load or Unlink then sets F71's title.
@MainActor
public enum CurveEditorDialog {
    public static func present(
        on window: NSWindow,
        core: VixenCore,
        curve: CurveValue,
        title: String? = nil,
        libraryName: String? = nil,
        onOK: @escaping (CurveValue) -> Void
    ) {
        KitSheet.present(on: window) { dismiss in
            CurveEditorContent(
                session: CurveEditorSession(curve: curve, libraryName: libraryName),
                core: core,
                rowTitle: title,
                onFinish: { result in
                    dismiss()
                    if let result {
                        onOK(result)
                    }
                }
            )
        }
    }

    /// `EditLibraryCurveAsync`: opens the editor on a library entry, and OK replaces the entry. `onDone` runs after the library has changed.
    public static func editLibraryEntry(on window: NSWindow, core: VixenCore, name: String, onDone: (() -> Void)? = nil) {
        guard let entry = core.libraryCurves().first(where: { $0.name == name }) else { return }
        present(on: window, core: core, curve: entry.curve, libraryName: name) { edited in
            _ = core.libraryAddCurve(name: name, curve: edited)
            onDone?()
        }
    }

    /// F78's and F81's name loop: empty asks again, an existing name asks to overwrite. `completion` gets the name to use, or `nil` if cancelled.
    public static func askForName(on window: NSWindow, core: VixenCore, typed: String = "", completion: @escaping (String?) -> Void) {
        KitSheet.present(on: window) { dismiss in
            NamePromptSheet(prompt: "Curve name?", initialText: typed) { response in
                dismiss()
                guard let response else {
                    completion(nil)
                    return
                }
                Task { @MainActor in
                    if response.isEmpty {
                        _ = await askInSheet(on: window, message: "Please enter a name.", title: "Curve Name", buttons: ["OK"], style: .critical)
                        askForName(on: window, core: core, typed: response, completion: completion)
                        return
                    }
                    guard core.libraryCurves().contains(where: { $0.name == response }) else {
                        completion(response)
                        return
                    }
                    let answer = await askInSheet(
                        on: window, message: "There is already a curve with that name. Do you want to overwrite it?", title: "Overwrite curve?",
                        buttons: ["Overwrite", "Choose Another Name", "Cancel"], destructiveIndex: 0
                    )
                    switch answer {
                    case 0: completion(response)
                    case 1: askForName(on: window, core: core, typed: response, completion: completion)
                    default: completion(nil)
                    }
                }
            }
        }
    }
}

/// Holds the session's state, so a view can observe each change.
@Observable
final class CurveEditorViewModel {
    let session: CurveEditorSession
    var state: CurveEditorState
    var xText = ""
    var yText = ""

    init(session: CurveEditorSession) {
        self.session = session
        state = session.state()
        xText = state.xText
        yText = state.yText
    }

    func apply(_ new: CurveEditorState) {
        state = new
        xText = new.xText
        yText = new.yText
    }
}

/// The same dialog as SwiftUI content, for a sheet already being presented. `onFinish` gets the result, or `nil` for Cancel.
public struct CurveEditorView: View {
    let core: VixenCore
    let curve: CurveValue
    let onFinish: (CurveValue?) -> Void

    public init(core: VixenCore, curve: CurveValue, onFinish: @escaping (CurveValue?) -> Void) {
        self.core = core
        self.curve = curve
        self.onFinish = onFinish
    }

    public var body: some View {
        CurveEditorContent(session: CurveEditorSession(curve: curve, libraryName: nil), core: core, rowTitle: nil, onFinish: onFinish)
    }
}

private struct CurveEditorContent: View {
    let core: VixenCore
    let rowTitle: String?
    let onFinish: (CurveValue?) -> Void
    @State private var model: CurveEditorViewModel
    @State private var drawInterval: String
    @State private var drawBalloon: BalloonText?
    @State private var hostReference = WeakWindow()
    private var hostWindow: NSWindow? { hostReference.window }
    /// The row's name is shown until Load or Unlink sets F71's title.
    @State private var usesRowTitle: Bool

    init(session: CurveEditorSession, core: VixenCore, rowTitle: String?, onFinish: @escaping (CurveValue?) -> Void) {
        self.core = core
        self.rowTitle = rowTitle
        self.onFinish = onFinish
        _model = State(initialValue: CurveEditorViewModel(session: session))
        _drawInterval = State(initialValue: session.state().drawInterval)
        _usesRowTitle = State(initialValue: rowTitle != nil)
    }

    private var state: CurveEditorState { model.state }

    private var windowTitle: String {
        usesRowTitle ? (rowTitle ?? state.title) : state.title
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(windowTitle).font(.headline)
            CurveGraph(model: model)
                .frame(minWidth: 549, minHeight: 482)
            curveGroup
            libraryGroup
            HStack(alignment: .bottom) {
                if state.instructionsVisible {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Hold down ⌘ to add new points on the grid.")
                        Text("Hold down ⌥ to remove a point.")
                    }
                    .font(.caption)
                    .foregroundStyle(.secondary)
                }
                Spacer()
                Button("Cancel") { onFinish(nil) }
                    .keyboardShortcut(.cancelAction)
                Button("OK") { onFinish(model.session.result()) }
                    .keyboardShortcut(.defaultAction)
            }
        }
        .padding(16)
        .frame(minWidth: 591)
        .background(WindowFinder(into: hostReference))
    }

    private var curveGroup: some View {
        GroupBox("Curve") {
            VStack(alignment: .leading, spacing: 8) {
                HStack(alignment: .top, spacing: 16) {
                    VStack(alignment: .leading, spacing: 6) {
                        Button("Reverse Curve") { model.apply(model.session.reverse()) }
                            .disabled(!state.canInvertReverse)
                        Button("Invert Curve") { model.apply(model.session.invert()) }
                            .disabled(!state.canInvertReverse)
                    }
                    .fixedSize()
                    VStack(alignment: .leading, spacing: 6) {
                        Button("Draw Curve") {
                            model.apply(model.session.drawBegin())
                            showBalloon("Draw curve from left side to right side of grid using the left mouse button", seconds: 4)
                        }
                        .disabled(!state.canDraw)
                        .popover(item: $drawBalloon) { balloon in
                            Text(balloon.text).padding(10).frame(maxWidth: 300).fixedSize(horizontal: false, vertical: true)
                        }
                        HStack(spacing: 6) {
                            Text("Draw Interval")
                                .help("Min distance to create Curve point")
                            TextField("", text: $drawInterval)
                                .multilineTextAlignment(.center)
                                .frame(width: 36)
                                .monospacedDigit()
                                .onChange(of: drawInterval) { _, text in
                                    if model.session.setDrawInterval(text: text) {
                                        showBalloon("Draw Threshold has a maximun value of 20", seconds: 3)
                                    }
                                    let normalized = model.session.state().drawInterval
                                    if normalized != text {
                                        drawInterval = normalized
                                    }
                                }
                        }
                    }
                    .fixedSize()
                    Button("f(x) Curve") { openFunctionGenerator() }
                        .disabled(!state.canDraw)
                        .fixedSize()
                    Spacer()
                }
                HStack(spacing: 6) {
                    Text("Selected Point")
                    Spacer().frame(width: 8)
                    Text("X:")
                    TextField("", text: $model.xText)
                        .frame(width: 80)
                        .monospacedDigit()
                        .disabled(!state.fieldsEnabled)
                        .onChange(of: model.xText) { before, typed in
                            let kept = curveCoordinateText(typed: typed, before: before)
                            if kept != typed {
                                model.xText = kept
                            }
                        }
                    Text("Y:")
                    TextField("", text: $model.yText)
                        .frame(width: 80)
                        .monospacedDigit()
                        .disabled(!state.fieldsEnabled)
                        .onChange(of: model.yText) { before, typed in
                            let kept = curveCoordinateText(typed: typed, before: before)
                            if kept != typed {
                                model.yText = kept
                            }
                        }
                    Button("Update") {
                        model.apply(model.session.updateCoordinates(xText: model.xText, yText: model.yText))
                    }
                    .disabled(!state.updateEnabled)
                    .fixedSize()
                    Spacer()
                }
            }
            .padding(6)
        }
    }

    private func showBalloon(_ text: String, seconds: Int) {
        let balloon = BalloonText(text: text)
        drawBalloon = balloon
        Task { @MainActor in
            try? await Task.sleep(for: .seconds(seconds))
            if drawBalloon?.id == balloon.id {
                drawBalloon = nil
            }
        }
    }

    private var libraryGroup: some View {
        GroupBox("Library") {
            VStack(alignment: .leading, spacing: 6) {
                Text(state.label).fixedSize(horizontal: false, vertical: true)
                HStack {
                    Button("Load Curve") { loadCurve() }
                        .disabled(!state.canLoad)
                    Button("Save Curve") { saveCurve() }
                        .disabled(!state.canSave)
                    Button("Unlink Curve") {
                        usesRowTitle = false
                        model.apply(model.session.unlink())
                    }
                    .disabled(!state.canUnlink)
                    Button("Edit Library Curve") { editLibraryCurve() }
                        .disabled(!state.canEditLibrary)
                    Spacer()
                }
            }
            .padding(6)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private func loadCurve() {
        guard let hostWindow else { return }
        CurveLibrarySelector.present(on: hostWindow, core: core) { item in
            usesRowTitle = false
            model.apply(model.session.loadFromLibrary(name: item.name, entry: item.curve))
        }
    }

    private func saveCurve() {
        guard let hostWindow else { return }
        CurveEditorDialog.askForName(on: hostWindow, core: core) { name in
            if let name {
                _ = core.libraryAddCurve(name: name, curve: model.session.result())
            }
        }
    }

    private func editLibraryCurve() {
        guard let hostWindow else { return }
        let name = state.curve.libraryName
        CurveEditorDialog.editLibraryEntry(on: hostWindow, core: core, name: name) {
            if let entry = core.libraryCurves().first(where: { $0.name == name }) {
                model.apply(model.session.refreshFromLibrary(entry: entry.curve))
            }
        }
    }

    private func openFunctionGenerator() {
        guard let hostWindow else { return }
        KitSheet.present(on: hostWindow) { dismiss in
            FunctionGeneratorSheet(function: state.lastFunction) { function in
                dismiss()
                guard let function else { return }
                let problem = model.session.applyFunction(function: function)
                model.apply(model.session.state())
                if let problem {
                    Task { @MainActor in
                        _ = await askInSheet(on: hostWindow, message: problem.message, title: problem.title, buttons: ["OK"], style: .critical)
                    }
                }
            }
        }
    }
}

/// A balloon's text, for `.popover(item:)`.
struct BalloonText: Identifiable {
    let text: String
    var id: String { text }
}

/// `FunctionGenerator`.
private struct FunctionGeneratorSheet: View {
    let onFinish: (String?) -> Void
    @State private var function: String

    init(function: String, onFinish: @escaping (String?) -> Void) {
        self.onFinish = onFinish
        _function = State(initialValue: function)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Function Curve Generator").font(.headline)
            Text("Enter the mathmatical f(x) that provides the value of y. The function will be run with values of x from 0-100 incremented by the Draw Interval to get the x,y values in the curve.  Example gamma function where 2.2 is the gamma and 80 is the max level: Pow(x/100, 2.2) * 80.")
                .fixedSize(horizontal: false, vertical: true)
            if let help = URL(string: "http://www.vixenlights.com/vixen-3-documentation/basic-concepts-of-vixen-3/curve-editor/") {
                Link("Help", destination: help)
            }
            HStack {
                Text("Function")
                TextField("", text: $function)
                    .frame(minWidth: 320)
            }
            HStack {
                Spacer()
                Button("Cancel") { onFinish(nil) }
                    .keyboardShortcut(.cancelAction)
                Button("Generate") { onFinish(function) }
                    .keyboardShortcut(.defaultAction)
            }
        }
        .padding(20)
        .frame(width: 460)
    }
}

/// A window reference that doesn't keep the window alive. A view inside a window that holds its window strongly (in `@State`, or in a
/// `Binding`, which keeps a copy of its value) makes a cycle through the window's own view graph, so a closed window was never freed.
@MainActor
public final class WeakWindow {
    public weak var window: NSWindow?

    public init() {}
}

/// Finds the window a SwiftUI view is in, and reports it without holding on to it. It reports whenever the view moves to a window, so a
/// view built before it was put in one (a docked pane, for instance) still learns its window.
public struct WindowFinder: NSViewRepresentable {
    private let found: @MainActor (NSWindow?) -> Void

    public init(into reference: WeakWindow) {
        found = { [weak reference] in reference?.window = $0 }
    }

    public init(_ found: @escaping @MainActor (NSWindow?) -> Void) {
        self.found = found
    }

    public func makeNSView(context: Context) -> TrackingView {
        let view = TrackingView()
        view.found = found
        return view
    }

    public func updateNSView(_ view: TrackingView, context: Context) {
        view.found = found
    }

    public final class TrackingView: NSView {
        var found: (@MainActor (NSWindow?) -> Void)?

        public override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            found?(window)
        }
    }
}

/// F72's graph: dashed grid, axis numbers, a line with point symbols. Mouse input goes straight to the session; nothing goes to Rust per draw.
private struct CurveGraph: NSViewRepresentable {
    let model: CurveEditorViewModel

    func makeNSView(context: Context) -> CurveGraphNSView {
        let view = CurveGraphNSView(frame: .zero)
        view.model = model
        return view
    }

    func updateNSView(_ view: CurveGraphNSView, context: Context) {
        _ = model.state
        view.model = model
        view.needsDisplay = true
    }
}

final class CurveGraphNSView: NSView {
    private static let blue = NSColor(srgbRed: 0, green: 128 / 255, blue: 1, alpha: 1)
    private static let grey = NSColor(srgbRed: 169 / 255, green: 169 / 255, blue: 169 / 255, alpha: 1)
    private static let margin = NSEdgeInsets(top: 12, left: 34, bottom: 24, right: 14)

    var model: CurveEditorViewModel?
    private var startupGuardUntil = Date().addingTimeInterval(0.3)
    private var tracking: NSTrackingArea?

    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }

    override func acceptsFirstMouse(for event: NSEvent?) -> Bool {
        UITestHarness.isActive
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        // F70's guard: the release of the double-click that opened the dialog can't add or move a point.
        startupGuardUntil = Date().addingTimeInterval(0.3)
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let tracking {
            removeTrackingArea(tracking)
        }
        let area = NSTrackingArea(rect: bounds, options: [.mouseMoved, .activeInKeyWindow, .inVisibleRect], owner: self)
        addTrackingArea(area)
        tracking = area
    }

    private var chart: NSRect {
        NSRect(
            x: Self.margin.left, y: Self.margin.top,
            width: max(1, bounds.width - Self.margin.left - Self.margin.right),
            height: max(1, bounds.height - Self.margin.top - Self.margin.bottom)
        )
    }

    private func data(from p: NSPoint) -> (x: Double, y: Double) {
        let c = chart
        return ((p.x - c.minX) / c.width * 100, (c.maxY - p.y) / c.height * 100)
    }

    private func view(fromX x: Double, y: Double) -> NSPoint {
        let c = chart
        return NSPoint(x: c.minX + x / 100 * c.width, y: c.maxY - y / 100 * c.height)
    }

    override func draw(_ dirtyRect: NSRect) {
        NSColor(srgbRed: 68 / 255, green: 68 / 255, blue: 68 / 255, alpha: 1).setFill()
        bounds.fill()
        let c = chart
        let border = NSColor(srgbRed: 96 / 255, green: 96 / 255, blue: 96 / 255, alpha: 1)
        border.setStroke()
        let grid = NSBezierPath()
        grid.setLineDash([2, 4], count: 2, phase: 0)
        grid.lineWidth = 1
        let attributes: [NSAttributedString.Key: Any] = [.font: NSFont.monospacedDigitSystemFont(ofSize: 10, weight: .regular), .foregroundColor: NSColor(white: 0.92, alpha: 1)]
        for tick in stride(from: 0, through: 100, by: 10) {
            let v = view(fromX: Double(tick), y: 0)
            let h = view(fromX: 0, y: Double(tick))
            if tick > 0, tick < 100 {
                grid.move(to: NSPoint(x: v.x.rounded() + 0.5, y: c.minY))
                grid.line(to: NSPoint(x: v.x.rounded() + 0.5, y: c.maxY))
                grid.move(to: NSPoint(x: c.minX, y: h.y.rounded() + 0.5))
                grid.line(to: NSPoint(x: c.maxX, y: h.y.rounded() + 0.5))
            }
            let text = NSAttributedString(string: "\(tick)", attributes: attributes)
            let size = text.size()
            text.draw(at: NSPoint(x: v.x - size.width / 2, y: c.maxY + 5))
            text.draw(at: NSPoint(x: c.minX - size.width - 6, y: h.y - size.height / 2))
        }
        grid.stroke()
        NSBezierPath(rect: c.insetBy(dx: 0.5, dy: 0.5)).stroke()

        guard let state = model?.state else { return }
        let color = state.curve.libraryName.isEmpty ? Self.blue : Self.grey
        let points = state.curve.points.filter { !$0.x.isNaN && !$0.y.isNaN }
        NSGraphicsContext.current?.saveGraphicsState()
        NSBezierPath(rect: c.insetBy(dx: -6, dy: -6)).addClip()
        if points.count > 1 {
            let path = NSBezierPath()
            path.lineWidth = 1.5
            path.move(to: view(fromX: points[0].x, y: points[0].y))
            for p in points.dropFirst() {
                path.line(to: view(fromX: p.x, y: p.y))
            }
            color.setStroke()
            path.stroke()
        }
        color.setFill()
        for p in points {
            let at = view(fromX: p.x, y: p.y)
            NSBezierPath(ovalIn: NSRect(x: at.x - 3.5, y: at.y - 3.5, width: 7, height: 7)).fill()
        }
        NSGraphicsContext.current?.restoreGraphicsState()
    }

    private var ignoringInput: Bool { Date() < startupGuardUntil }

    private var unitScale: (x: Double, y: Double) {
        (chart.width / 100, chart.height / 100)
    }

    override func mouseMoved(with event: NSEvent) {
        guard let model, !ignoringInput else { return }
        let p = data(from: convert(event.locationInWindow, from: nil))
        NSCursor.kit(model.session.hover(x: p.x, y: p.y, pointsPerUnitX: unitScale.x, pointsPerUnitY: unitScale.y)).set()
    }

    override func mouseDown(with event: NSEvent) {
        guard let model, !ignoringInput else { return }
        window?.makeFirstResponder(self)
        let location = convert(event.locationInWindow, from: nil)
        guard chart.insetBy(dx: -8, dy: -8).contains(location) else { return }
        let p = data(from: location)
        model.apply(model.session.press(x: p.x, y: p.y, mods: KitMods(event.modifierFlags), pointsPerUnitX: unitScale.x, pointsPerUnitY: unitScale.y))
        needsDisplay = true
    }

    override func mouseDragged(with event: NSEvent) {
        guard let model, !ignoringInput else { return }
        let p = data(from: convert(event.locationInWindow, from: nil))
        model.apply(model.session.drag(x: p.x, y: p.y, mods: KitMods(event.modifierFlags)))
        needsDisplay = true
    }

    override func mouseUp(with event: NSEvent) {
        guard let model, !ignoringInput else { return }
        let p = data(from: convert(event.locationInWindow, from: nil))
        model.apply(model.session.release(y: p.y))
        needsDisplay = true
    }

    override func keyDown(with event: NSEvent) {
        if event.keyCode == 53, let model, model.session.cancelDrag() {
            model.apply(model.session.state())
            needsDisplay = true
        } else {
            super.keyDown(with: event)
        }
    }

    override func cancelOperation(_ sender: Any?) {
        if let model, model.session.cancelDrag() {
            model.apply(model.session.state())
            needsDisplay = true
        } else {
            super.cancelOperation(sender)
        }
    }
}
