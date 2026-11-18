import AppKit
import SwiftUI
import VixenBridge
import VixenKit

#if canImport(VixenTimeline)
import VixenTimeline

/// What the grid takes: section I. Effect types from the panel and strip, values from the library panels, strips and the effect editor, and files from Finder.
extension SequencerModel {
    static var dropTypes: [NSPasteboard.PasteboardType] {
        [EffectDrag.pasteboardType, .fileURL] + VixenDragValue.pasteboardTypes
    }

    private func draggedEffect(_ board: NSPasteboard) -> String? {
        board.data(forType: EffectDrag.pasteboardType).flatMap { String(data: $0, encoding: .utf8) }
    }

    private func dragValue(_ board: NSPasteboard) -> ShellDropValue? {
        switch VixenDragValue.read(from: board) {
        case .color(let argb): .color(argb: argb)
        case .gradient(let value): .gradient(value: value)
        case .curve(let value): .curve(value: value)
        case nil: nil
        }
    }

    /// F306.
    func validateDrop(_ board: NSPasteboard, row: UInt32?, element: UInt64?) -> NSDragOperation {
        guard let shell else { return [] }
        if draggedEffect(board) != nil {
            guard let row, !shell.rowIsDeprecated(row: row) else { return [] }
            return .copy
        }
        if board.canReadObject(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) {
            return .copy
        }
        if let value = dragValue(board) {
            guard let element else { return [] }
            if element == dropCacheElement, let cached = dropCacheAnswer {
                return cached
            }
            let answer: NSDragOperation = shell.dropAccepted(element: element, value: value) ? .copy : []
            dropCacheElement = element
            dropCacheAnswer = answer
            return answer
        }
        return []
    }

    /// F305, F307 to F309, F311.
    func performDrop(_ board: NSPasteboard, at point: NSPoint, flags: NSEvent.ModifierFlags, time: Int64, row: UInt32?, element: UInt64?) -> Bool {
        dropCacheElement = nil
        guard let shell else { return false }
        let timeMs = Double(time) / 10_000
        if let typeId = draggedEffect(board) {
            guard let row, !shell.rowIsDeprecated(row: row) else { return false }
            if flags.contains(.command), ui?.hasSelection == true, let prompt = shell.replacePrompt(typeId: typeId) {
                showAlert(prompt)
                return true
            }
            _ = shell.addEffectAt(typeId: typeId, row: row, timeMs: timeMs)
            refresh()
            return true
        }
        if let urls = board.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL], !urls.isEmpty {
            guard let row else { return false }
            _ = shell.dropFiles(paths: urls.map(\.path), timeMs: timeMs, row: row, shift: flags.contains(.shift), cmd: flags.contains(.command), opt: flags.contains(.option))
            refresh()
            return true
        }
        guard let value = dragValue(board), let element, let plan = shell.planDrop(element: element, value: value) else { return false }
        Task { await carryOutDrop(plan, at: point) }
        return true
    }

    /// An effect type, or a file, dropped on a row at 5 s, for scripts: a drag can't be started from one. The real path from the pasteboard on.
    func testDropOnRow(typeId: String?, file: String?, row: UInt32) {
        let board = NSPasteboard(name: NSPasteboard.Name("org.vixenlights.test-drag"))
        board.clearContents()
        if let typeId {
            board.writeObjects([EffectDrag.pasteboardItem(typeId: typeId)])
        }
        if let file {
            board.writeObjects([URL(fileURLWithPath: file) as NSURL])
        }
        let answer = validateDrop(board, row: row, element: nil)
        shell?.setStatusMessage(text: "Test drop: \(answer == .copy ? "copy" : "refused")", timeoutSeconds: 8)
        _ = performDrop(board, at: NSPoint(x: 300, y: 300), flags: [], time: 50_000_000, row: row, element: nil)
    }

    /// A value dropped on the first selected effect, for scripts: a drag can't be started from one.
    func testDrop(_ value: ShellDropValue) {
        guard let shell, let element = canvas?.controller.selectedElements().first else { return }
        guard let plan = shell.planDrop(element: element, value: value) else {
            shell.setStatusMessage(text: "Test drop: nothing takes it", timeoutSeconds: 8)
            refresh()
            return
        }
        Task { await carryOutDrop(plan, at: NSPoint(x: 300, y: 300)) }
    }

    /// F307's warning, then one property or the picker.
    private func carryOutDrop(_ plan: ShellDropPlan, at point: NSPoint) async {
        guard let shell else { return }
        if let warning = plan.warning() {
            let go = await SequencerAlerts.confirm(
                title: warning.title,
                message: warning.message,
                style: warning.style,
                confirm: warning.confirmLabel ?? "OK",
                cancel: warning.cancelLabel ?? "Cancel",
                window: window
            )
            if !go {
                return
            }
        }
        if plan.isDirect() {
            plan.apply(choice: 0)
            refresh()
            return
        }
        shell.setStatusMessage(text: "Choose the property to set, press Escape to cancel.", timeoutSeconds: 8)
        refresh()
        if UITestHarness.isActive, !ParameterPicker.showsUnderHarness {
            // A script that hasn't asked for the popover (`sequencer.picker`) gets the first choice.
            plan.apply(choice: 0)
            refresh()
            return
        }
        ParameterPicker.show(choices: plan.choices(), at: point, window: window) { [weak self] index in
            plan.apply(choice: UInt32(index))
            self?.refresh()
        }
    }
}

/// `FormParameterPicker`: a popover at the pointer that goes away by itself after 8 s. F310, F312.
@MainActor
enum ParameterPicker {
    private static var current: NSPopover?
    private static var timeout: Task<Void, Never>?
    private static var answer: ((Int) -> Void)?
    private static var labels: [String] = []
    /// Set by a script through `sequencer.picker`, so the popover shows there too.
    static var showsUnderHarness = false

    /// `sequencer.picker` with path ["show"], ["state"], ["snapshot", name] or ["choose", index]: the popover for scripts, which can't see
    /// or click one. The snapshot goes to the harness's folder.
    static func harness(_ args: [String]) -> String {
        switch args.first {
        case "show":
            showsUnderHarness = true
            return "shown from now on"
        case "state":
            return current?.isShown == true ? "open: " + labels.joined(separator: "|") : "closed"
        case "snapshot":
            guard args.count == 2, let view = current?.contentViewController?.view, let rep = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { return "FAIL no picker" }
            // The popover's own dark backing isn't part of the content view, and the labels are white.
            if let context = NSGraphicsContext(bitmapImageRep: rep) {
                NSGraphicsContext.saveGraphicsState()
                NSGraphicsContext.current = context
                NSColor(white: 0.17, alpha: 1).setFill()
                view.bounds.fill()
                NSGraphicsContext.restoreGraphicsState()
            }
            view.cacheDisplay(in: view.bounds, to: rep)
            let folder = URL(fileURLWithPath: ProcessInfo.processInfo.environment["VIXEN_UI_TEST_OUT"] ?? NSTemporaryDirectory())
            guard let data = rep.representation(using: .png, properties: [:]), (try? data.write(to: folder.appending(path: args[1] + ".png"))) != nil else { return "FAIL not written" }
            return "\(Int(view.bounds.width))x\(Int(view.bounds.height))"
        case "choose":
            guard args.count == 2, let index = Int(args[1]), let answer, labels.indices.contains(index) else { return "FAIL no picker or choice" }
            answer(index)
            return "chose \(labels[index])"
        default:
            return "FAIL needs show, state, snapshot or choose"
        }
    }

    static func show(choices: [ShellPickerChoice], at point: NSPoint, window: NSWindow?, chosen: @escaping (Int) -> Void) {
        guard let window, let content = window.contentView else { return }
        close()
        let popover = NSPopover()
        popover.behavior = .transient
        let pick: (Int) -> Void = { index in
            close()
            chosen(index)
        }
        answer = pick
        labels = choices.map(\.label)
        let view = PickerContent(choices: choices, chosen: pick)
        popover.contentViewController = NSHostingController(rootView: view.studioAppearance()) // STUDIO: follow the chosen appearance
        let inContent = content.convert(point, from: nil)
        popover.show(relativeTo: NSRect(origin: inContent, size: NSSize(width: 1, height: 1)), of: content, preferredEdge: .maxX)
        current = popover
        timeout = Task {
            try? await Task.sleep(for: .seconds(8))
            if !Task.isCancelled {
                close()
            }
        }
    }

    static func close() {
        timeout?.cancel()
        timeout = nil
        current?.close()
        current = nil
        answer = nil
    }
}

private struct PickerContent: View {
    let choices: [ShellPickerChoice]
    let chosen: (Int) -> Void

    /// Items spanning more than one owner: one group per owner, top down. Otherwise left to right, ungrouped first, then a group per `GroupName`.
    private var groups: [(title: String?, items: [(Int, ShellPickerChoice)])] {
        let items = Array(choices.enumerated()).map { ($0.offset, $0.element) }
        let owners = Set(choices.map(\.owner))
        var result: [(title: String?, items: [(Int, ShellPickerChoice)])] = []
        let key: (ShellPickerChoice) -> String? = owners.count > 1 ? { $0.owner.map { String($0.prefix(10)) } } : { $0.group }
        for item in items {
            let title = key(item.1)
            if let at = result.firstIndex(where: { $0.title == title }) {
                result[at].items.append(item)
            } else {
                result.append((title, [item]))
            }
        }
        return result
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            ForEach(Array(groups.enumerated()), id: \.offset) { _, group in
                if let title = group.title {
                    Text(title)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                HStack(alignment: .top, spacing: 10) {
                    ForEach(group.items, id: \.0) { index, choice in
                        Button {
                            chosen(index)
                        } label: {
                            VStack(spacing: 4) {
                                thumbnail(choice.image)
                                    .border(Color(nsColor: NSColor(srgbRed: 136 / 255, green: 136 / 255, blue: 136 / 255, alpha: 1)))
                                Text(choice.label)
                                    .font(.caption)
                            }
                        }
                        .buttonStyle(.plain)
                    }
                }
            }
        }
        .padding(10)
        .onExitCommand { ParameterPicker.close() }
    }

    @ViewBuilder
    private func thumbnail(_ image: ShellPickerImage) -> some View {
        switch image {
        case .color(let argb):
            Color(nsColor: NSColor(argb: argb)).frame(width: 48, height: 48)
        case .gradient(let value):
            Image(nsImage: GradientThumbnail.image(value: value, size: CGSize(width: 64, height: 64))).frame(width: 64, height: 64)
        case .curve(let curve):
            Image(nsImage: CurveThumbnail.image(points: curve.points, size: CGSize(width: 64, height: 64))).frame(width: 64, height: 64)
        }
    }
}
#endif
