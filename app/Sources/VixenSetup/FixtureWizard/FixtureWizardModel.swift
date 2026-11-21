import AppKit
import Observation
import SwiftUI
import VixenBridge
import VixenDialogs
import VixenKit

struct FixtureWizardAlert {
    let title: String
    let message: String
}

/// The Intelligent Fixture wizard (F139 to F147). `SetupFixtureWizard` in Rust holds its state and rules, and this only forwards what the user does.
/// `completion` runs once, on the main thread: with what Finish made, or nil for Cancel or the window closing.
@Observable @MainActor
final class FixtureWizardModel {
    @ObservationIgnored private let session: DisplaySetupSession
    @ObservationIgnored private let completion: (SetupEditResult?) -> Void
    @ObservationIgnored private var wizard: SetupFixtureWizard?
    @ObservationIgnored private var isDone = false

    /// Nil until the profile folder has been read.
    private(set) var state: SetupFixtureWizardState?
    private(set) var isFinishing = false
    var alert: FixtureWizardAlert?
    var showsCurveEditor = false
    var editor: FixtureEditorModel?
    var confirmOverwrite = false

    init(session: DisplaySetupSession, completion: @escaping (SetupEditResult?) -> Void) {
        self.session = session
        self.completion = completion
    }

    func load() async {
        guard wizard == nil else { return }
        let session = session
        let opened = await Task.detached { session.openFixtureWizard() }.value
        wizard = opened
        state = opened.state()
        if UITestHarness.isActive {
            UITestHarness.hooks["fixtureWizard"] = { [weak self] args in
                guard let self, let state = self.state else { return "FAIL no fixture wizard" }
                if args.count == 2 {
                    switch args[0] {
                    case "name": self.apply { $0.setProfileName(text: args[1]) }
                    case "manufacturer": self.apply { $0.setManufacturer(text: args[1]) }
                    default: return "FAIL unknown fixture field"
                    }
                } else if !args.isEmpty { return "FAIL expected field/value or no arguments" }
                let current = self.state ?? state
                let info: [String: Any] = ["name": current.profileName, "manufacturer": current.manufacturer,
                    "page": String(describing: current.page), "existing": current.selectExisting,
                    "canFinish": current.canFinish, "confirmOverwrite": self.confirmOverwrite, "done": self.isDone]
                guard let data = try? JSONSerialization.data(withJSONObject: info, options: [.sortedKeys]),
                      let text = String(data: data, encoding: .utf8) else { return "FAIL serialization" }
                return text
            }
        }
        let warnings = opened.loadWarnings()
        if let first = warnings.first {
            alert = FixtureWizardAlert(title: first.title, message: warnings.map(\.message).joined(separator: "\n\n"))
        }
    }

    func apply(_ change: (SetupFixtureWizard) -> SetupFixtureWizardState) {
        if let wizard {
            state = change(wizard)
            if state?.page == .editFunctions || state?.page == .editChannels {
                if editor == nil {
                    let model = FixtureEditorModel(editor: wizard.profileEditor())
                    model.changed = { [weak self, weak wizard] in if let wizard { self?.state = wizard.state() } }
                    editor = model
                }
                state = wizard.state()
            } else if state?.page == .selectProfile { editor = nil }
        }
    }

    /// A binding that shows `value`, which comes from the latest state, and sends edits to Rust.
    func binding<Value>(_ value: Value, _ write: @escaping (SetupFixtureWizard, Value) -> SetupFixtureWizardState) -> Binding<Value> {
        Binding(get: { value }, set: { new in self.apply { write($0, new) } })
    }

    func openHelp() {
        guard let text = state?.helpUrl, let url = URL(string: text) else { return }
        if UITestHarness.isActive {
            UITestHarness.note("help: \(text)")
        } else {
            NSWorkspace.shared.open(url)
        }
    }

    var curveCore: VixenCore? { wizard?.vixenCore() }
    var dimmingCurve: CurveValue? { wizard?.dimmingCurve() }

    // STUDIO-BEGIN curve-editor-live-wizard: the wizard's curve follows each change in the curve editor (B24)
    func changeCurve(_ curve: CurveValue) {
        apply { $0.setDimmingCurve(curve: curve) }
    }
    // STUDIO-END curve-editor-live-wizard

    func finish(allowOverwrite: Bool = false) {
        guard let wizard, state?.canFinish == true, !isFinishing, !isDone else { return }
        if allowOverwrite { wizard.approveProfileOverwrite() }
        if wizard.requiresProfileOverwrite() { confirmOverwrite = true; return }
        isFinishing = true
        let session = session
        Task {
            let outcome = await Task.detached { Result { try session.createFixtureElements(wizard: wizard) } }.value
            isFinishing = false
            switch outcome {
            case .success(let edit):
                done(edit)
            case .failure(let error):
                alert = FixtureWizardAlert(title: "Error", message: error.userMessage)
            }
        }
    }

    func cancel() {
        done(nil)
    }

    /// The close box. A Finish already running still reports what it made.
    func windowClosed() {
        if !isFinishing {
            done(nil)
        }
    }

    private func done(_ edit: SetupEditResult?) {
        guard !isDone else { return }
        isDone = true
        completion(edit)
    }
}
