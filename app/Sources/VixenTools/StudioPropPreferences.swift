import AppKit
import SwiftUI
import VixenBridge
import VixenKit

// Studio (B8): the prop editor's Preferences sheet is live, replacing vanilla's OK/Cancel `PropPreferencesSheet`.
// Each change applies to the open prop at once and is saved to the profile (`savePropPreferences`), and each is one
// step on Edit ▸ Undo ("Change Preferences") while the sheet is up; a color drag or typing a size is one step.
// Defaults is a step too. Done closes.

@MainActor @Observable
final class StudioPropPreferences {
    let model: PropEditorModel
    var light = Color.white
    var selected = Color.pink
    var base = Color.gray
    var size = 3
    var error = ""
    private(set) var applied: PropEdPreferences
    @ObservationIgnored weak var undo: UndoManager?
    @ObservationIgnored private var loading = false
    @ObservationIgnored private var lastChange = Date.distantPast
    @ObservationIgnored private var saving: Task<Void, Never>?
    /// What `show` last put in the controls, so a color the user hasn't touched keeps its exact value (a Color doesn't round-trip).
    @ObservationIgnored private var shown: (p: PropEdPreferences, light: Color, selected: Color, base: Color)?
    static let stepName = "Change Preferences"

    init(model: PropEditorModel) {
        self.model = model
        applied = model.app?.core?.propPreferences() ?? propEdDefaultPreferences()
        show(applied)
    }

    /// A control changed: apply what the controls show. Changes within a second of each other are one Undo step.
    func edited() {
        guard !loading, size > 0 else { return }
        let lightSize = UInt32(size)
        let lightColor = light == shown?.light ? shown!.p.lightColor : argb(light)
        let selectedColor = selected == shown?.selected ? shown!.p.selectedColor : argb(selected)
        let baseColor = base == shown?.base ? shown!.p.stateBaseColor : argb(base)
        set(PropEdPreferences(lightColor: lightColor, selectedColor: selectedColor, stateBaseColor: baseColor, lightSize: lightSize), coalesce: true)
    }

    func defaults() {
        let p = propEdDefaultPreferences()
        show(p)
        set(p, coalesce: false)
    }

    private func set(_ p: PropEdPreferences, coalesce: Bool) {
        guard p != applied else { return }
        if !coalesce || Date().timeIntervalSince(lastChange) > 1 { register(restoring: applied) }
        lastChange = Date()
        apply(p)
    }

    private func register(restoring old: PropEdPreferences) {
        guard let undo else { return }
        let group = !undo.groupsByEvent && !undo.isUndoing && !undo.isRedoing
        if group { undo.beginUndoGrouping() }
        undo.registerUndo(withTarget: self) { $0.restore(old) }
        undo.setActionName(Self.stepName)
        if group { undo.endUndoGrouping() }
    }

    /// Undo and Redo: put `p` back, registering the preferences it replaces as the opposite step.
    private func restore(_ p: PropEdPreferences) {
        register(restoring: applied)
        lastChange = .distantPast
        show(p)
        apply(p)
    }

    private func show(_ p: PropEdPreferences) {
        loading = true
        light = Color(nsColor: NSColor(argb: p.lightColor))
        selected = Color(nsColor: NSColor(argb: p.selectedColor))
        base = Color(nsColor: NSColor(argb: p.stateBaseColor))
        size = Int(p.lightSize)
        shown = (p, light, selected, base)
        loading = false
    }

    /// This prop shows the change at once; the profile is written a moment later, and the other prop windows reload
    /// from it once it's saved.
    private func apply(_ p: PropEdPreferences) {
        applied = p
        model.editor.applyPreferences(preferences: p)
        model.refresh()
        saving?.cancel()
        let core = model.app?.core
        saving = Task {
            try? await Task.sleep(for: .milliseconds(300))
            guard !Task.isCancelled else { return }
            do {
                if let core { try await Task.detached { try core.savePropPreferences(preferences: p) }.value }
                guard !Task.isCancelled else { return }
                error = ""
                NotificationCenter.default.post(name: .propPreferencesChanged, object: nil)
            } catch { self.error = error.userMessage }
        }
    }

    private func argb(_ value: Color) -> UInt32 {
        let c = NSColor(value).usingColorSpace(.deviceRGB) ?? .white
        return 0xff000000 | UInt32((c.redComponent * 255).rounded()) << 16 | UInt32((c.greenComponent * 255).rounded()) << 8 | UInt32((c.blueComponent * 255).rounded())
    }
}

struct StudioPropPreferencesSheet: View {
    @State private var prefs: StudioPropPreferences
    @Environment(\.dismiss) private var dismiss
    @Environment(\.undoManager) private var undoManager
    init(model: PropEditorModel) { _prefs = State(initialValue: StudioPropPreferences(model: model)) }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Preferences").font(.headline)
            Form {
                ColorPicker("Light Color", selection: $prefs.light, supportsOpacity: false)
                ColorPicker("Selected Color", selection: $prefs.selected, supportsOpacity: false)
                ColorPicker("State Preview Base Color", selection: $prefs.base, supportsOpacity: false)
                // A stepper rather than a text field, so Edit ▸ Undo reaches the preferences instead of a field's typing.
                Stepper(value: $prefs.size, in: 1...max(100, prefs.size)) {
                    LabeledContent("Default Light Size", value: String(prefs.size)).monospacedDigit()
                }
            }
            Text("Changes apply as you make them. Edit ▸ Undo takes them back.").font(.caption).foregroundStyle(.secondary)
            if !prefs.error.isEmpty { Text(prefs.error).foregroundStyle(.red) }
            HStack {
                Button("Defaults") { prefs.defaults() }
                Spacer()
                Button("Done") { dismiss() }.keyboardShortcut(.defaultAction)
            }
        }
        .padding(20).frame(width: 350)
        .onChange(of: prefs.light) { prefs.edited() }
        .onChange(of: prefs.selected) { prefs.edited() }
        .onChange(of: prefs.base) { prefs.edited() }
        .onChange(of: prefs.size) { prefs.edited() }
        .onAppear { prefs.undo = undoManager; StudioPropPreferencesHarness.current = prefs }
        .onChange(of: undoManager) { prefs.undo = undoManager }
        .onExitCommand { dismiss() }
    }
}

/// `studio.propPreferences` with path ["state"] (the applied preferences and the sheet's Undo), ["size", n] (sets the
/// light size, as the stepper does), ["undo"] or ["redo"] (the sheet's undo manager), ["responder-undo"] or
/// ["responder-redo"] (Edit ▸ Undo's route, through the sheet window's responder chain), ["defaults"].
@MainActor
enum StudioPropPreferencesHarness {
    static weak var current: StudioPropPreferences?
    static func install() {
        guard UITestHarness.isActive else { return }
        UITestHarness.hooks["studio.propPreferences"] = { args in
            guard let prefs = current else { return "FAIL no preferences sheet" }
            switch args.first {
            case "size": guard args.count == 2, let n = Int(args[1]) else { return "FAIL size n" }; prefs.size = n; prefs.edited()
            case "undo": prefs.undo?.undo()
            case "redo": prefs.undo?.redo()
            case "defaults": prefs.defaults()
            case "responder-undo", "responder-redo":
                // Edit ▸ Undo's route: the sheet window's first responder up its responder chain.
                guard let sheet = prefs.model.window?.attachedSheet, let responder = sheet.firstResponder else { return "FAIL no sheet" }
                if args.count > 1 { return "first=\(type(of: responder)) windowManager=\(sheet.undoManager === prefs.undo) sheetDelegate=\(String(describing: sheet.delegate.map { type(of: $0) }))" }
                guard responder.tryToPerform(NSSelectorFromString(args[0] == "responder-undo" ? "undo:" : "redo:"), with: nil) else { return "FAIL nothing in the sheet's chain handled it" }
            default: break
            }
            let saved = prefs.model.app?.core?.propPreferences().lightSize ?? 0
            return "size=\(prefs.applied.lightSize) field=\(prefs.size) saved=\(saved) light=\(String(prefs.applied.lightColor, radix: 16)) undo=\(prefs.undo?.canUndo == true ? prefs.undo?.undoActionName ?? "-" : "-") redo=\(prefs.undo?.canRedo == true ? prefs.undo?.redoActionName ?? "-" : "-") document=\(prefs.undo === prefs.model.documentUndoManager)"
        }
    }
}
