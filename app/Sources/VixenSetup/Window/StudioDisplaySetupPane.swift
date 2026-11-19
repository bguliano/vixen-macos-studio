import AppKit
import Observation
import SwiftUI
import VixenBridge
import VixenKit

/// Display Setup inside the show window's Display section. The editor is the same working copy as the window had: nothing
/// reaches the display until Apply Changes, and Discard throws the edits away. Leaving the section with edits asks first.
struct StudioDisplaySetupPane: View {
    let model: SetupModel
    @Environment(AppModel.self) private var app
    private var changes: StudioSetupChanges { .shared }

    var body: some View {
        VStack(spacing: 0) {
            if model.session == nil {
                ProgressView(model.isClosing ? "Applying Display Changes" : "Opening Display Setup")
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                SetupRootView(model: model, inline: true)
                Divider()
                applyBar
            }
        }
        .background(StudioPaneWindowReader())
        .onAppear {
            changes.paneVisible = true
            openIfIdle()
        }
        .onDisappear {
            changes.paneVisible = false
            // The show window closed or the app is quitting; leaving the section comes through `confirmLeave` first.
            model.windowClosed()
        }
        // After an apply or a discard the working copy is gone. A fresh one opens once the save or reload has finished.
        .onChange(of: model.session == nil) { openIfIdle() }
        .onChange(of: app.isBusy) { openIfIdle() }
        .onChange(of: app.core == nil) { openIfIdle() }
        // Every route that edits the working copy (trees, patching, dialogs, the wizard) shows up here within half a second.
        .task {
            while !Task.isCancelled {
                changes.refresh(model.session)
                try? await Task.sleep(for: .milliseconds(500))
            }
        }
    }

    private var applyBar: some View {
        HStack(spacing: StudioMetrics.spacingS) {
            if changes.hasChanges {
                Image(systemName: "circle.fill")
                    .foregroundStyle(.orange)
                    .imageScale(.small)
                    .accessibilityHidden(true)
                Text("Unapplied changes")
            } else {
                Text("No unapplied changes")
                    .foregroundStyle(.secondary)
            }
            Spacer()
            Button("Help") { model.help() }
            Button("Discard Changes") { model.cancel() }
                .disabled(!changes.hasChanges || model.isClosing)
            Button("Apply Changes") { model.applyFromPane() }
                .disabled(!changes.hasChanges || model.isClosing)
                .buttonStyle(.borderedProminent)
        }
        .font(.callout)
        .padding(.horizontal, StudioMetrics.spacingM)
        .padding(.vertical, StudioMetrics.spacingS)
    }

    private func openIfIdle() {
        guard changes.paneVisible, model.session == nil, !model.isClosing, !app.isBusy else { return }
        changes.hasChanges = false
        model.windowOpened()
    }
}

/// Whether the pane's working copy differs from the display. Kept apart from `SetupModel` so the window's model stays as upstream has it.
@MainActor @Observable
final class StudioSetupChanges {
    static let shared = StudioSetupChanges()
    var hasChanges = false
    @ObservationIgnored var paneVisible = false
    @ObservationIgnored private var token = 0

    /// The working copy answers off the main thread, since a long edit may still hold it.
    func refresh(_ session: DisplaySetupSession?) {
        guard paneVisible else { return }
        guard let session else {
            hasChanges = false
            return
        }
        token += 1
        let mine = token
        Task {
            let dirty = await offMain { session.isDirty() }
            if mine == token { hasChanges = dirty }
        }
    }
}

extension SetupModel {
    private static var studioInstance: SetupModel?

    /// The one model. SwiftUI may build `SetupScene` again, and a second model would open a second working copy.
    static func studioShared(app: AppModel) -> SetupModel {
        if let studioInstance { return studioInstance }
        let made = SetupModel(app: app)
        studioInstance = made
        return made
    }

    /// Hooks the pane into the show window. Called from `SetupScene`.
    static func registerStudioPane(model: SetupModel, app: AppModel) {
        StudioDisplayPane.make = { AnyView(StudioDisplaySetupPane(model: model).environment(app)) }
        StudioDisplayPane.confirmLeave = { [weak model] in await model?.confirmLeavingPane() ?? true }
        if UITestHarness.isActive {
            SetupTestHooks.install(model: model)
            UITestHarness.customActions["displayPaneState"] = { _ in
                (true, "session=\(model.session != nil) closing=\(model.isClosing) changes=\(StudioSetupChanges.shared.hasChanges) busy=\(app.isBusy)")
            }
        }
    }

    /// Apply Changes: the same commit as the window's OK, which the show window's progress reports.
    func applyFromPane() {
        guard let session, !isClosing, session.isDirty() else { return }
        ok()
    }

    /// The show window is leaving the Display section.
    func confirmLeavingPane() async -> Bool {
        guard !isClosing else { return false }
        guard let session else { return true }
        guard session.isDirty() else {
            cancel()
            return true
        }
        let choice = await ask("Display Setup", "Apply your changes to the display before leaving Display Setup?", [
            .init(title: "Apply Changes", value: 2),
            .init(title: "Discard Changes", role: .destructive, value: 1),
            .init(title: "Keep Editing", role: .cancel, value: 0),
        ])
        switch choice {
        case 2:
            ok()
            return true
        case 1:
            cancel()
            return true
        default:
            return false
        }
    }
}

/// Tells Display Setup which window hosts the pane, for sheets.
private struct StudioPaneWindowReader: NSViewRepresentable {
    final class Marker: NSView {
        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            if let window { StudioDisplayPane.hostWindow = window }
        }
    }

    func makeNSView(context: Context) -> Marker { Marker() }
    func updateNSView(_ marker: Marker, context: Context) {}
}
