import AppKit
import SwiftUI
import VixenBridge
import VixenKit

/// Tools > LipSync > Lyric Converter (`docs/spec/marks-layers-analysis.md` F76 to F83): the content of the "LipSync Text Convert" window.
///
/// The conversion and the clipboard it makes are Rust's. The paste belongs to the sequencer window, which has the timeline's rows, so it
/// comes in as two closures: `cursorPosition` gives the timeline cursor in ticks, and `pasteAtTime` pastes the timeline clipboard at a
/// time in default mode and returns how many effects went in. Without them nothing pastes and the "Convert Text" message shows, as it
/// does upstream whenever a paste puts nothing down.
public struct LyricConverterView: View {
    private let session: SequencerSession
    private let cursorPosition: (() -> Int64)?
    private let pasteAtTime: ((Int64) -> UInt32)?
    @State private var model: LyricConverterModel?

    public init(session: SequencerSession, cursorPosition: (() -> Int64)? = nil, pasteAtTime: ((Int64) -> UInt32)? = nil) {
        self.session = session
        self.cursorPosition = cursorPosition
        self.pasteAtTime = pasteAtTime
    }

    public var body: some View {
        Group {
            if let model {
                LyricConverterContent(model: model)
            } else {
                Color.clear
            }
        }
        .frame(minWidth: 320, idealWidth: 320, minHeight: 440, idealHeight: 440)
        .onAppear {
            if model == nil {
                model = LyricConverterModel(session: session, cursorPosition: cursorPosition, pasteAtTime: pasteAtTime)
            }
        }
    }
}

/// The timeline clipboard, as the sequencer window reads it. Tests use a private pasteboard so a script never touches the owner's clipboard.
enum LyricPasteboard {
    private static let types = lyricPasteboardTypes()
    private static let dataType = NSPasteboard.PasteboardType(types.data)
    private static let countType = NSPasteboard.PasteboardType(types.count)

    static var board: NSPasteboard {
        UITestHarness.isActive ? NSPasteboard(name: NSPasteboard.Name("org.vixenlights.test")) : .general
    }

    static func write(json: String, count: Int) {
        let board = board
        board.clearContents()
        board.declareTypes([dataType, countType], owner: nil)
        board.setString(json, forType: dataType)
        board.setString(String(count), forType: countType)
    }

    static func read() -> (json: String?, count: Int) {
        (board.string(forType: dataType), Int(board.string(forType: countType) ?? "") ?? 0)
    }
}

/// The word F82 is asking about.
struct LyricFailure: Identifiable {
    let id = UUID()
    let word: String
    let title: String
    let message: String
}

@Observable
final class LyricConverterModel {
    let converter: LyricConverter
    private(set) var state: LyricConverterState
    var text = ""
    var clearAfter = false
    var failure: LyricFailure?
    var alert: MarksAlert?
    /// What the last Convert did, for test scripts.
    private(set) var lastOutcome = ""
    @ObservationIgnored var selectedText = ""
    @ObservationIgnored weak var textView: NSTextView?
    @ObservationIgnored private var afterAlert: (() -> Void)?
    @ObservationIgnored private var revision: UInt64
    @ObservationIgnored private let cursorPosition: (() -> Int64)?
    @ObservationIgnored private let pasteAtTime: ((Int64) -> UInt32)?
    @ObservationIgnored private let session: SequencerSession // STUDIO: Map Word goes on the profile's shared Undo through it

    init(session: SequencerSession, cursorPosition: (() -> Int64)?, pasteAtTime: ((Int64) -> UInt32)?) {
        self.session = session // STUDIO: Map Word goes on the profile's shared Undo through it
        converter = session.lyricConverter()
        state = converter.state()
        revision = converter.revision()
        self.cursorPosition = cursorPosition
        self.pasteAtTime = pasteAtTime
        LyricTestHooks.current = self
    }

    var canConvert: Bool {
        !text.allSatisfy(\.isWhitespace)
    }

    /// Upstream rebuilds the lists each time the window comes forward. Following the sequence's edit counter covers that and more.
    func follow() async {
        while !Task.isCancelled {
            let now = converter.revision()
            if now != revision {
                revision = now
                refresh()
            }
            try? await Task.sleep(for: .milliseconds(250))
        }
    }

    func refresh() {
        converter.refresh()
        state = converter.state()
    }

    func setPlacement(_ placement: LyricPlacement) {
        converter.setPlacement(placement: placement)
        state = converter.state()
    }

    func setCollection(_ index: UInt32) {
        converter.setCollection(index: index)
        state = converter.state()
    }

    func setStartOffset(_ index: UInt32) {
        converter.setStartOffset(index: index)
        state = converter.state()
    }

    func setAlignment(_ alignment: LyricAlignment) {
        converter.setAlignment(alignment: alignment)
        state = converter.state()
    }

    func convert() {
        guard canConvert, failure == nil else { return }
        let source = selectedText.isEmpty ? text : selectedText
        handle(converter.convert(text: source, clearAfter: clearAfter))
    }

    /// F82 closed: `mapped` is the field's text for OK, nil for Cancel.
    func finishMapping(_ mapped: String?) {
        guard let asked = failure else { return }
        failure = nil
        // STUDIO-BEGIN lyric-map-word: Map Word is one step on the profile's shared Undo, mirrored into this window's Edit ▸ Undo (R40)
        let outcome = mapped.map { session.studioMapWord(word: asked.word, mapped: $0) }
        if let label = outcome?.label {
            StudioWordMappingUndo.register(label, session: session, on: textView?.window?.undoManager)
        }
        if let problem = outcome?.problem {
        // STUDIO-END lyric-map-word
            afterAlert = { [weak self] in
                guard let self else { return }
                handle(converter.resume())
            }
            alert = MarksAlert(title: problem.title, message: problem.message)
            return
        }
        handle(converter.resume())
    }

    func alertDismissed() {
        let next = afterAlert
        afterAlert = nil
        next?()
    }

    private func handle(_ step: LyricConvertStep) {
        switch step {
        case let .needsMapping(word, title, message):
            lastOutcome = "needsMapping \(word)"
            failure = LyricFailure(word: word, title: title, message: message)
        case let .delivered(clipboardJson, count, placement, firstMark, clearText):
            if clearText {
                text = ""
                selectedText = ""
            }
            state = converter.state()
            guard let clipboardJson else {
                lastOutcome = "delivered nothing"
                return
            }
            LyricPasteboard.write(json: clipboardJson, count: Int(count))
            var pasted: UInt32 = 0
            if placement != .clipboard, let pasteAtTime {
                let at = placement == .cursor ? firstMark + (cursorPosition?() ?? 0) : firstMark
                pasted = pasteAtTime(at)
            }
            lastOutcome = "delivered \(count) \(placement) at \(firstMark) pasted \(pasted)"
            if let message = converter.delivered(pasted: pasted) {
                alert = MarksAlert(title: message.title, message: message.message, isError: false)
            }
        case .abandoned:
            lastOutcome = "abandoned"
            state = converter.state()
        }
    }
}

private struct LyricConverterContent: View {
    @Bindable var model: LyricConverterModel

    var body: some View {
        let state = model.state
        VStack(alignment: .leading, spacing: 10) {
            GroupBox("Target / Spacing") {
                Picker("Target / Spacing", selection: Binding(get: { state.placement }, set: { model.setPlacement($0) })) {
                    Text("Cursor / Even Spacing").tag(LyricPlacement.cursor)
                    Text("Clipboard / Even Spacing").tag(LyricPlacement.clipboard)
                    Text("Per Mark Collection").tag(LyricPlacement.mark)
                        .selectionDisabled(!state.markPlacementAvailable)
                }
                .pickerStyle(.radioGroup)
                .labelsHidden()
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(4)
            }
            GroupBox("Marks") {
                Grid(alignment: .leading, horizontalSpacing: 8, verticalSpacing: 8) {
                    GridRow {
                        Text("Collection")
                        Picker("Collection", selection: Binding(get: { state.collection }, set: { if let i = $0 { model.setCollection(i) } })) {
                            ForEach(Array(state.collections.enumerated()), id: \.offset) { index, name in
                                Text(name).tag(Optional(UInt32(index)))
                            }
                        }
                    }
                    GridRow {
                        Text("Start Offset")
                        Picker("Start Offset", selection: Binding(get: { state.startOffset }, set: { if let i = $0 { model.setStartOffset(i) } })) {
                            ForEach(Array(state.startOffsets.enumerated()), id: \.offset) { index, time in
                                Text(time).monospacedDigit().tag(Optional(UInt32(index)))
                            }
                        }
                    }
                    GridRow {
                        Text("Alignment")
                        Picker("Alignment", selection: Binding(get: { state.alignment }, set: { if let a = $0 { model.setAlignment(a) } })) {
                            // Empty until the group has been on once, as upstream's list is.
                            if state.alignment != nil {
                                Text("Phoneme").tag(Optional(LyricAlignment.phoneme))
                                Text("Word").tag(Optional(LyricAlignment.word))
                                Text("Phrase").tag(Optional(LyricAlignment.phrase))
                            }
                        }
                    }
                }
                .labelsHidden()
                .foregroundStyle(state.marksEnabled ? .primary : .tertiary)
                .disabled(!state.marksEnabled)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(4)
            }
            HStack {
                Text("Text to Convert")
                Spacer()
                Toggle("Clear after conversion", isOn: $model.clearAfter)
            }
            LyricTextBox(model: model)
                .frame(minHeight: 60, maxHeight: .infinity)
            HStack {
                Spacer()
                Button("Convert") { model.convert() }
                    .keyboardShortcut(.defaultAction)
                    .disabled(!model.canConvert)
            }
        }
        .padding(12)
        .task { await model.follow() }
        .sheet(item: Binding(get: { model.failure }, set: { if $0 == nil { model.finishMapping(nil) } })) { asked in
            LyricFailureSheet(failure: asked) { model.finishMapping($0) }
        }
        .alert(model.alert?.title ?? "", isPresented: Binding(get: { model.alert != nil }, set: { if !$0 { model.alert = nil; model.alertDismissed() } })) {
            Button("OK") {}
        } message: {
            Text(model.alert?.message ?? "")
        }
    }
}

/// The text box. AppKit's, for the selection (F79: a selection converts alone) and so Return converts as upstream's accept button does;
/// Option-Return makes a line break.
private struct LyricTextBox: NSViewRepresentable {
    let model: LyricConverterModel

    func makeCoordinator() -> Coordinator {
        Coordinator(model: model)
    }

    func makeNSView(context: Context) -> NSScrollView {
        let scroll = NSTextView.scrollableTextView()
        scroll.borderType = .bezelBorder
        scroll.hasVerticalScroller = true
        scroll.autohidesScrollers = true
        guard let view = scroll.documentView as? NSTextView else { return scroll }
        view.isRichText = false
        view.allowsUndo = true
        view.font = .systemFont(ofSize: NSFont.systemFontSize)
        // The dictionary knows plain apostrophes only.
        view.isAutomaticQuoteSubstitutionEnabled = false
        view.isAutomaticDashSubstitutionEnabled = false
        view.isAutomaticTextReplacementEnabled = false
        view.isAutomaticSpellingCorrectionEnabled = false
        view.delegate = context.coordinator
        view.setAccessibilityLabel("Text to Convert")
        model.textView = view
        return scroll
    }

    func updateNSView(_ scroll: NSScrollView, context: Context) {
        guard let view = scroll.documentView as? NSTextView else { return }
        if view.string != model.text {
            view.string = model.text
        }
    }

    final class Coordinator: NSObject, NSTextViewDelegate {
        let model: LyricConverterModel

        init(model: LyricConverterModel) {
            self.model = model
        }

        func textDidChange(_ notification: Notification) {
            guard let view = notification.object as? NSTextView else { return }
            model.text = view.string
        }

        func textViewDidChangeSelection(_ notification: Notification) {
            guard let view = notification.object as? NSTextView else { return }
            let range = view.selectedRange()
            model.selectedText = range.length > 0 ? (view.string as NSString).substring(with: range) : ""
        }

        func textView(_ textView: NSTextView, doCommandBy selector: Selector) -> Bool {
            if selector == #selector(NSResponder.insertNewline(_:)) {
                model.convert()
                return true
            }
            return false
        }
    }
}

/// F82, "Text Conversion Failure": the user spells an unknown word out of the ten mouth shapes. `onFinish` gets the field's text for OK
/// and nil for Cancel. Public so the marks bar's Breakdown Word can show the same dialog.
public struct LyricFailureSheet: View {
    private let title: String
    private let message: String
    private let onFinish: (String?) -> Void
    @State private var mapped = ""
    private let rows = lyricFailureButtons()

    init(failure: LyricFailure, onFinish: @escaping (String?) -> Void) {
        title = failure.title
        message = failure.message
        self.onFinish = onFinish
    }

    /// For a word from anywhere: the title and label come from `lyricFailurePrompt(word:)`.
    public init(word: String, onFinish: @escaping (String?) -> Void) {
        let prompt = lyricFailurePrompt(word: word)
        title = prompt.title
        message = prompt.message
        self.onFinish = onFinish
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(title).font(.headline)
            Text(message)
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityIdentifier("lyric-failure-message")
            HStack {
                TextField("Mapping", text: .constant(mapped))
                    .textFieldStyle(.roundedBorder)
                    .labelsHidden()
                    .disabled(true)
                    .accessibilityLabel("Mapping")
                Button("Clear") { mapped = "" }
            }
            Grid(horizontalSpacing: 10, verticalSpacing: 10) {
                ForEach(Array(rows.enumerated()), id: \.offset) { _, row in
                    GridRow {
                        ForEach(row, id: \.caption) { button in
                            Button {
                                mapped += button.text
                            } label: {
                                VStack(spacing: 2) {
                                    PhonemeImage(name: button.image)
                                    Text(button.caption).font(.caption)
                                }
                                .frame(width: 56)
                            }
                            .accessibilityLabel(button.caption)
                        }
                    }
                }
            }
            .frame(maxWidth: .infinity)
            HStack {
                Spacer()
                Button("Skip Word", role: .cancel) { onFinish(nil) } // STUDIO: a verb, not Cancel (R40)
                    .keyboardShortcut(.cancelAction)
                Button("Map Word") { onFinish(mapped) } // STUDIO: a verb, not OK; the mapping is one Undo step (R40)
                    .keyboardShortcut(.defaultAction)
            }
        }
        .padding(16)
        .frame(width: 446)
    }
}

/// A mouth picture from `Resources/marks/phonemes`, at upstream's 45 points.
private struct PhonemeImage: View {
    let name: String

    @MainActor private static var cache: [String: NSImage] = [:]

    var body: some View {
        if let image = Self.load(name) {
            Image(nsImage: image)
                .resizable()
                .interpolation(.high)
                .scaledToFit()
                .frame(width: 45, height: 45)
        } else {
            Color.clear.frame(width: 45, height: 45)
        }
    }

    private static func load(_ name: String) -> NSImage? {
        if let hit = cache[name] {
            return hit
        }
        let image = VixenResources.image("marks/phonemes/\(name).jpg")
        cache[name] = image
        return image
    }
}

/// What test scripts can ask of the converter: `{ "action": "hook", "name": "lyricState", "args": ["out-name"] }` and so on.
enum LyricTestHooks {
    static weak var current: LyricConverterModel?

    static func install() {
        StudioLyricTestHooks.install() // STUDIO: the word mapping's Undo for test scripts (R40)
        UITestHarness.hooks["lyricText"] = { args in
            guard let model = current else { return "FAIL no converter" }
            model.text = args.first ?? ""
            model.selectedText = ""
            return "text \(model.text.count) characters"
        }
        // Selects `length` UTF-16 units from `start` in the text box, as a drag would.
        UITestHarness.hooks["lyricSelect"] = { args in
            guard let model = current, let view = model.textView, args.count >= 2, let start = Int(args[0]), let length = Int(args[1]) else {
                return "FAIL needs the converter, a start and a length"
            }
            view.string = model.text
            view.setSelectedRange(NSRange(location: start, length: length))
            return "selected \(model.selectedText)"
        }
        UITestHarness.hooks["lyricState"] = { args in
            guard let model = current else { return "FAIL no converter" }
            model.refresh()
            let s = model.state
            let board = LyricPasteboard.read()
            let state: [String: Any] = [
                "placement": "\(s.placement)", "markPlacementAvailable": s.markPlacementAvailable, "marksEnabled": s.marksEnabled,
                "collections": s.collections, "collection": s.collection.map { Int($0) } ?? -1,
                "startOffsets": s.startOffsets, "startOffset": s.startOffset.map { Int($0) } ?? -1,
                "alignment": s.alignment.map { "\($0)" } ?? "", "text": model.text, "clearAfter": model.clearAfter,
                "failure": model.failure?.message ?? "", "alertTitle": model.alert?.title ?? "", "alert": model.alert?.message ?? "",
                "lastOutcome": model.lastOutcome, "pasteboardCount": board.count,
                "pasteboard": (board.json?.data(using: .utf8)).flatMap { try? JSONSerialization.jsonObject(with: $0) } ?? NSNull(),
            ]
            let name = args.first ?? "lyric-state"
            let out = URL(fileURLWithPath: ProcessInfo.processInfo.environment["VIXEN_UI_TEST_OUT"] ?? NSTemporaryDirectory()).appending(path: "\(name).json")
            if let data = try? JSONSerialization.data(withJSONObject: state, options: [.prettyPrinted, .sortedKeys]) {
                try? data.write(to: out)
            }
            return "\(s.placement) \(model.lastOutcome)"
        }
    }
}
