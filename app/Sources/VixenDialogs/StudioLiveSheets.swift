import AppKit
import SwiftUI
import VixenBridge
import VixenKit

// Studio (B9): the property, filter and controller setup sheets are live instead of committing through OK/Cancel.
// Each change is written to where the sheet works (Display Setup's working copy, or the profile) as it's made, and is one
// step on the sheet's Edit ▸ Undo while the sheet is up: a field's typing, a slider drag or a run of quick changes is one
// step. Done closes and keeps everything. Display Setup holds publication while one of these sheets is open, so the whole
// visit is one step on the display's shared Undo, named for the sheet ("Configure Controller").
//
// A sheet adds `.studioLive(sheet, draft: $draft) { try context.applyX(..., draft: $0) }` and swaps its `SheetButtons`
// for `StudioDoneButton(sheet) { onFinish($0) }`. Questions that only pick a value for the sheet that opened them keep
// a confirmation, with a verb (Choose, Set, Rename), never OK.

/// One sheet's live state: whether anything applied, the last refusal, and its undo steps.
@MainActor @Observable
public final class StudioLiveSheet {
    /// The last change the sheet couldn't apply, shown above Done until a change applies.
    var error: String?
    private(set) var changed = false
    let step: String
    @ObservationIgnored weak var undo: UndoManager?
    @ObservationIgnored fileprivate var fields: [StudioLiveFieldBase] = []
    /// The sheet on screen, for the harness.
    static weak var current: StudioLiveSheet?

    public init(step: String) {
        self.step = step
    }

    /// Applies what's still waiting out its typing pause, drops the sheet's own undo steps (the display's Undo now holds
    /// the visit as one), and reports whether anything changed.
    func finish(_ done: (Bool) -> Void) {
        for field in fields {
            field.flush()
            undo?.removeAllActions(withTarget: field)
        }
        done(changed)
    }

    fileprivate func applied() {
        changed = true
        error = nil
    }

    /// Something applied outside a draft (a row's own action, a chosen port). Undo for it, when given, puts it back.
    func appliedNow(undoing: (() -> Void)? = nil) {
        StudioLiveSheet.current = self
        applied()
        guard let undoing, let undo else { return }
        let target = StudioLiveAction()
        fields.append(target)
        undo.registerUndo(withTarget: target) { _ in undoing() }
        undo.setActionName(step)
    }
}

/// Thrown by an apply that can't take the draft yet (no serial port chosen), so it waits without an error.
struct StudioLiveHold: Error {}

class StudioLiveFieldBase: NSObject {
    @MainActor func flush() {}
    /// What's applied, for the harness.
    @MainActor var appliedText: String { "" }
}

private final class StudioLiveAction: StudioLiveFieldBase {}

/// One draft of a sheet: applies it as it changes and keeps its undo steps.
@MainActor
final class StudioLiveField<Value: Equatable>: StudioLiveFieldBase {
    private let apply: (Value) throws -> Void
    private let delay: Duration
    private(set) var applied: Value
    private var waiting: Value?
    private var pause: Task<Void, Never>?
    private var lastChange = Date.distantPast
    private var restoring = false
    weak var sheet: StudioLiveSheet?
    var write: (Value) -> Void = { _ in }

    init(start: Value, delay: Duration, apply: @escaping (Value) throws -> Void) {
        applied = start
        self.delay = delay
        self.apply = apply
    }

    /// The draft changed. Applied after `delay` with no further change, so typing applies once it pauses.
    func edited(_ value: Value) {
        guard !restoring, value != applied || waiting != nil else { return }
        waiting = value
        pause?.cancel()
        pause = Task { [weak self] in
            try? await Task.sleep(for: self?.delay ?? .zero)
            guard !Task.isCancelled else { return }
            self?.flush()
        }
    }

    override var appliedText: String { String(describing: applied) }

    override func flush() {
        pause?.cancel()
        guard let value = waiting else { return }
        waiting = nil
        guard value != applied else { return }
        do {
            try apply(value)
        } catch is StudioLiveHold {
            return
        } catch {
            sheet?.error = error.userMessage
            return
        }
        // Changes within a second and a half of each other are one step.
        if Date().timeIntervalSince(lastChange) > 1.5 { register(restoring: applied) }
        lastChange = Date()
        applied = value
        sheet?.applied()
    }

    private func register(restoring old: Value) {
        guard let undo = sheet?.undo else { return }
        let group = !undo.groupsByEvent && !undo.isUndoing && !undo.isRedoing
        if group { undo.beginUndoGrouping() }
        undo.registerUndo(withTarget: self) { $0.restore(old) }
        undo.setActionName(sheet?.step ?? "Change")
        if group { undo.endUndoGrouping() }
    }

    /// Undo and Redo: put `old` back, registering what it replaces as the opposite step.
    private func restore(_ old: Value) {
        pause?.cancel()
        waiting = nil
        do {
            try apply(old)
        } catch {
            sheet?.error = error.userMessage
            return
        }
        register(restoring: applied)
        lastChange = .distantPast
        applied = old
        restoring = true
        write(old)
        restoring = false
        sheet?.applied()
    }
}

private struct StudioLiveModifier<Value: Equatable>: ViewModifier {
    let sheet: StudioLiveSheet
    @Binding var draft: Value
    @State private var field: StudioLiveField<Value>
    @Environment(\.undoManager) private var undoManager

    init(sheet: StudioLiveSheet, draft: Binding<Value>, delay: Duration, apply: @escaping (Value) throws -> Void) {
        self.sheet = sheet
        _draft = draft
        _field = State(initialValue: StudioLiveField(start: draft.wrappedValue, delay: delay, apply: apply))
    }

    func body(content: Content) -> some View {
        content
            .onAppear {
                StudioLiveSheetHarness.install()
                field.sheet = sheet
                let binding = $draft
                field.write = { binding.wrappedValue = $0 }
                if !sheet.fields.contains(where: { $0 === field }) { sheet.fields.append(field) }
                if sheet.undo == nil { sheet.undo = undoManager }
                StudioLiveSheet.current = sheet
            }
            .onChange(of: undoManager) { sheet.undo = undoManager }
            .onChange(of: draft) { _, value in field.edited(value) }
    }
}

extension View {
    /// Applies `draft` with `apply` as it changes, as one of `sheet`'s undo steps. A refusal shows above Done and leaves
    /// the last applied value in place.
    func studioLive<Value: Equatable>(
        _ sheet: StudioLiveSheet,
        draft: Binding<Value>,
        delay: Duration = .milliseconds(350),
        apply: @escaping (Value) throws -> Void
    ) -> some View {
        modifier(StudioLiveModifier(sheet: sheet, draft: draft, delay: delay, apply: apply))
    }
}

/// Done at the bottom right, with what the sheet couldn't apply beside it. Return and Esc are Done.
struct StudioDoneButton: View {
    let sheet: StudioLiveSheet
    var disabled = false
    /// False keeps the sheet up, as when a grid cell's edit is invalid (its alert says why).
    var ready: () -> Bool = { true }
    let onFinish: (Bool) -> Void

    init(_ sheet: StudioLiveSheet, disabled: Bool = false, ready: @escaping () -> Bool = { true }, onFinish: @escaping (Bool) -> Void) {
        self.sheet = sheet
        self.disabled = disabled
        self.ready = ready
        self.onFinish = onFinish
    }

    private func done() {
        guard !disabled, ready() else { return }
        sheet.finish(onFinish)
    }

    var body: some View {
        HStack {
            if let error = sheet.error {
                Label(error, systemImage: "exclamationmark.triangle.fill")
                    .symbolRenderingMode(.multicolor)
                    .font(.caption)
                    .lineLimit(3)
                    .fixedSize(horizontal: false, vertical: true)
            } else {
                Text("Changes apply as you make them.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer()
            Button("Done", action: done)
                .keyboardShortcut(.defaultAction)
                .disabled(disabled)
                // Esc closes too, keeping the changes, as there's nothing to cancel.
                .background {
                    Button("", action: done).keyboardShortcut(.cancelAction).opacity(0).accessibilityHidden(true)
                }
        }
    }
}

/// A confirmation with a verb, for questions that hand a value back to the sheet that asked (Choose, Set, Rename).
struct StudioVerbButtons: View {
    let verb: String
    var disabled = false
    let onConfirm: () -> Void
    let onCancel: () -> Void

    var body: some View {
        HStack {
            Spacer()
            Button("Cancel", action: onCancel)
                .keyboardShortcut(.cancelAction)
            Button(verb, action: onConfirm)
                .keyboardShortcut(.defaultAction)
                .disabled(disabled)
        }
    }
}

/// `studio.liveSheet` with path ["state"] (the sheet's step, whether it changed, its error and Undo), ["undo"], ["redo"],
/// ["responder-undo"] or ["responder-redo"] (Edit ▸ Undo's route through the key window's responder chain), ["done"].
@MainActor
public enum StudioLiveSheetHarness {
    public static func install() {
        guard UITestHarness.isActive else { return }
        UITestHarness.hooks["studio.liveSheet"] = { args in
            guard let sheet = StudioLiveSheet.current else { return "FAIL no live sheet" }
            switch args.first {
            case "undo": sheet.undo?.undo()
            case "redo": sheet.undo?.redo()
            case "responder-undo", "responder-redo":
                // The sheet's window, as Edit ▸ Undo reaches it with the sheet up (the harness app isn't always active).
                let sheetWindow = NSApp.keyWindow ?? NSApp.windows.lazy.compactMap(\.attachedSheet).first
                guard let window = sheetWindow, let responder = window.firstResponder else { return "FAIL no sheet window" }
                guard responder.tryToPerform(NSSelectorFromString(args[0] == "responder-undo" ? "undo:" : "redo:"), with: nil) else {
                    return "FAIL nothing in the chain handled it"
                }
            case "flush": for field in sheet.fields { field.flush() }
            default: break
            }
            let undo = sheet.undo
            let applied = sheet.fields.map(\.appliedText).filter { !$0.isEmpty }.joined(separator: ";")
            return "applied=\(applied) step=\(sheet.step) changed=\(sheet.changed) error=\(sheet.error ?? "-") undo=\(undo?.canUndo == true ? undo?.undoActionName ?? "-" : "-") redo=\(undo?.canRedo == true ? undo?.redoActionName ?? "-" : "-")"
        }
    }
}
