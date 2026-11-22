import AppKit
import SwiftUI
import UniformTypeIdentifiers
import VixenBridge
import VixenKit

// R40: the Mark Collections panel without OK/Cancel windows. Every edit is live and one step on the sequence's Undo
// (`studio_marks_panel.rs`); these are the panel's Studio-only views.

extension MarksPanelModel {
    /// The live Offset Time field: `total` milliseconds from where the field found the marks.
    func offsetLive(_ row: MarkRowInfo, total: Int32, continuing: Bool) {
        panel.studioOffsetLive(id: row.id, totalMs: total, continuing: continuing)
        reload()
    }
}

/// Offset Time as a popover on the collection's row, in place of vanilla's number dialog: the marks move as the value changes, and one
/// Undo takes the whole adjustment back. Click away or press Escape when done.
struct StudioOffsetPopover: View {
    let model: MarksPanelModel
    let row: MarkRowInfo
    let prompt: MarkOffsetPrompt
    @State private var total: Int32 = 0
    @State private var started = false

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Offset “\(row.name)”").font(.headline)
            HStack {
                TextField("Offset", value: $total, format: .number.grouping(.never))
                    .monospacedDigit()
                    .frame(width: 90)
                    .labelsHidden()
                    .accessibilityLabel("Offset in milliseconds")
                Stepper("Offset", value: $total, in: prompt.minimumMs...prompt.maximumMs, step: 10)
                    .labelsHidden()
                Text("ms").foregroundStyle(.secondary)
            }
            HStack(spacing: 6) {
                ForEach([-100, -10, 10, 100] as [Int32], id: \.self) { step in
                    Button(step > 0 ? "+\(step)" : "\(step)") { total = clamp(total + step) }
                        .controlSize(.small)
                        .help("Move the marks \(abs(step)) ms \(step > 0 ? "later" : "earlier")")
                }
            }
            Text("Every mark moves as you change the value. Edit ▸ Undo puts them back.")
                .font(.callout)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
        }
        .padding(14)
        .frame(width: 260)
        .onChange(of: total) {
            let value = clamp(total)
            if value != total {
                total = value
                return
            }
            model.offsetLive(row, total: value, continuing: started)
            started = true
        }
    }

    private func clamp(_ value: Int32) -> Int32 {
        min(max(value, prompt.minimumMs), prompt.maximumMs)
    }
}

/// The toolbar's Import as a menu of the file types, in place of vanilla's Import Type Selection dialog.
struct StudioImportMenu: View {
    let model: MarksPanelModel

    var body: some View {
        Menu {
            ForEach(ImportTypeSheet.kinds, id: \.self) { kind in
                Button(markImportLabel(kind: kind)) {
                    Task { await model.importTypeChosen(kind) }
                }
            }
            Divider()
            Button("Timing from Sequence…") {
                Task { await model.importSequenceTiming() }
            }
            .help("Turn another sequence's effects into mark collections, from any profile")
        } label: {
            Image(systemName: "square.and.arrow.down")
                .frame(width: 22, height: 20)
        }
        .menuStyle(.borderlessButton)
        .menuIndicator(.hidden)
        .fixedSize()
        .help("Import")
        .accessibilityLabel("Import")
    }
}

// #61: Import ▸ Timing from Sequence…. A .tim from any profile becomes mark collections built from its effects; the open panel's
// options choose how, and the Select Imported Mark Collections dialog picks which to add, one step on the sequence's Undo.

extension MarksPanelModel {
    func importSequenceTiming() async {
        sheet = nil
        try? await Task.sleep(for: .milliseconds(50))
        let collections = rows.map { (id: $0.id, name: $0.name) }
        guard let choice = await StudioTimingPicker.choose(directory: panel.lastFolder(), collections: collections) else {
            panel.cancelImport()
            return
        }
        handle(panel.studioImportSequenceTiming(path: choice.path, merged: choice.merged, offsetMs: choice.offsetMs, snapTo: choice.snapTo))
    }
}

/// What the Timing from Sequence picker chose.
struct StudioTimingChoice {
    var path: String
    var merged = false
    var offsetMs: Double = 0
    var snapTo: String?
}

/// The open panel for a sequence file, with the import's options beside the file list: one collection per element or one for every
/// effect, an offset in milliseconds, and a collection of this sequence to snap the marks to.
enum StudioTimingPicker {
    /// The UI test harness's answers, taken in order, in place of the panel.
    static var queued: [StudioTimingChoice] = []

    /// `marksNextTiming <path> [merged] [offset ms] [snap collection name]` queues the picker's next answer.
    static func installTestHooks() {
        UITestHarness.hooks["marksNextTiming"] = { args in
            guard let path = args.first else { return "FAIL needs a .tim path" }
            var choice = StudioTimingChoice(path: path, merged: args.count > 1 && args[1] == "true", offsetMs: args.count > 2 ? Double(args[2]) ?? 0 : 0)
            if args.count > 3 {
                guard let row = MarksTestHooks.current?.rows.first(where: { $0.name == args[3] }) else { return "FAIL no collection \(args[3])" }
                choice.snapTo = row.id
            }
            queued.append(choice)
            return "queued \(queued.count)"
        }
    }

    static func choose(directory: String?, collections: [(id: String, name: String)]) async -> StudioTimingChoice? {
        if UITestHarness.isActive {
            return queued.isEmpty ? nil : queued.removeFirst()
        }
        let panel = NSOpenPanel()
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = false
        panel.message = "Choose a sequence. Its effects become mark collections in this sequence."
        panel.prompt = "Choose"
        panel.allowedContentTypes = [UTType(filenameExtension: "tim")].compactMap { $0 }
        if let directory {
            panel.directoryURL = URL(fileURLWithPath: directory, isDirectory: true)
        }
        let options = TimingOptionsAccessory(collections: collections)
        panel.accessoryView = options.view
        panel.isAccessoryViewDisclosed = true
        let response: NSApplication.ModalResponse
        if let window = NSApp.keyWindow {
            response = await panel.beginSheetModal(for: window)
        } else {
            response = panel.runModal()
        }
        guard response == .OK, let path = panel.url?.path else { return nil }
        return StudioTimingChoice(path: path, merged: options.merged, offsetMs: options.offsetMs, snapTo: options.snapTo)
    }
}

private final class TimingOptionsAccessory {
    let view: NSView
    private let marks = NSPopUpButton(frame: .zero, pullsDown: false)
    private let offset = NSTextField(string: "0")
    private let snap = NSPopUpButton(frame: .zero, pullsDown: false)
    private let collectionIDs: [String]

    init(collections: [(id: String, name: String)]) {
        collectionIDs = collections.map(\.id)
        marks.addItems(withTitles: ["One collection per element", "One collection for all effects"])
        marks.setAccessibilityLabel("Marks")
        offset.alignment = .right
        offset.formatter = {
            let f = NumberFormatter()
            f.numberStyle = .none
            f.minimum = -600_000
            f.maximum = 600_000
            return f
        }()
        offset.widthAnchor.constraint(equalToConstant: 70).isActive = true
        offset.setAccessibilityLabel("Offset in milliseconds")
        offset.toolTip = "Moves every mark, for example -35 to take out an old rig's latency"
        snap.addItem(withTitle: "Don't snap")
        snap.addItems(withTitles: collections.map(\.name))
        snap.setAccessibilityLabel("Snap to")
        snap.toolTip = "Moves each mark's start to the nearest mark of this collection"
        let row = { (title: String, views: [NSView]) -> NSStackView in
            let stack = NSStackView(views: [NSTextField(labelWithString: title)] + views)
            stack.orientation = .horizontal
            return stack
        }
        let grid = NSStackView(views: [
            row("Marks:", [marks]),
            row("Offset:", [offset, NSTextField(labelWithString: "ms")]),
            row("Snap to:", [snap]),
        ])
        grid.orientation = .vertical
        grid.alignment = .leading
        grid.edgeInsets = NSEdgeInsets(top: 8, left: 12, bottom: 8, right: 12)
        view = grid
    }

    var merged: Bool { marks.indexOfSelectedItem == 1 }
    var offsetMs: Double { Double(offset.integerValue) }
    var snapTo: String? {
        let i = snap.indexOfSelectedItem - 1
        return collectionIDs.indices.contains(i) ? collectionIDs[i] : nil
    }
}
