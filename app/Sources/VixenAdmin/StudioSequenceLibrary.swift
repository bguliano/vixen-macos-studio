import AppKit
import SwiftUI
import VixenBridge
import VixenKit

/// The Sequences section's file commands: New Sequence from Audio, Duplicate and Export FSEQ, run through the same
/// `library_*` commands agents use. A created file goes on the window's Undo, which removes it again while it's unchanged.
@MainActor
@Observable
final class StudioLibraryActions {
    /// What's running now, shown under the list ("Exporting Song.fseq…").
    private(set) var activity: String?
    /// The last outcome, shown under the list until the next command.
    private(set) var notice: String?
    /// A file the last command wrote, for the notice's Show in Finder.
    private(set) var noticeFile: URL?
    /// Paths the last Duplicate made, which the list selects (a copy keeps its original's date, as in Finder, so it may
    /// sort far from the top).
    private(set) var created: [String] = []
    @ObservationIgnored private let undoTarget = NSObject()

    var isBusy: Bool { activity != nil }

    func clearCreated() {
        created = []
    }

    func clearNotice() {
        notice = nil
        noticeFile = nil
    }

    /// A drop on the list: sequence files open, anything else is taken as a song and gets a new sequence, which opens.
    func dropped(_ urls: [URL], admin: AdminModel, undo: UndoManager?) -> Bool {
        let sequences = urls.filter { ["tim", "vix"].contains($0.pathExtension.lowercased()) }
        let songs = urls.filter { !sequences.contains($0) && !$0.hasDirectoryPath }
        if !sequences.isEmpty { admin.openFiles(sequences) }
        if !songs.isEmpty { Task { await newSequences(fromAudio: songs.map(\.path), admin: admin, undo: undo) } }
        return !sequences.isEmpty || !songs.isEmpty
    }

    /// One sequence per song, each saved in the Sequence folder with the song as its audio, then opened.
    func newSequences(fromAudio songs: [String], names: [String?]? = nil, admin: AdminModel, undo: UndoManager?) async {
        guard let core = admin.app.core else { return }
        var made: [[String: Any]] = []
        var failures: [String] = []
        for (index, song) in songs.enumerated() {
            activity = "Creating a sequence for \((song as NSString).lastPathComponent)…"
            var arguments: [String: Any] = ["audio": song]
            if let name = names?[index] { arguments["name"] = name }
            switch await run("library_new_from_audio", arguments, core: core) {
            case .success(let result): made.append(result)
            case .failure(let message): failures.append(message)
            }
        }
        activity = nil
        guard !made.isEmpty else {
            show(failures.joined(separator: " "))
            return
        }
        admin.openFiles(made.compactMap { $0["path"] as? String }.map { URL(fileURLWithPath: $0) })
        show(([made.count == 1 ? "Created \(made[0]["file"] as? String ?? "a sequence")." : "Created \(made.count) sequences."] + failures).joined(separator: " "))
        registerUndo(undo, name: "New Sequence from Audio", made: made, admin: admin) { [weak self] admin, undo in
            await self?.newSequences(
                fromAudio: made.compactMap { $0["audio"] as? String },
                names: made.map { $0["name"] as? String },
                admin: admin, undo: undo)
        }
    }

    /// A copy of each sequence, "<name> copy", beside it.
    func duplicate(_ paths: [String], admin: AdminModel, undo: UndoManager?) async {
        guard let core = admin.app.core else { return }
        var made: [[String: Any]] = []
        var failures: [String] = []
        activity = "Duplicating…"
        for path in paths {
            switch await run("library_duplicate", ["sequence": (path as NSString).lastPathComponent], core: core) {
            case .success(let result): made.append(result)
            case .failure(let message): failures.append(message)
            }
        }
        activity = nil
        created = made.compactMap { $0["path"] as? String }
        if let first = created.first { noticeFile = URL(fileURLWithPath: first) }
        show(([made.count == 1 ? "Created \(made[0]["file"] as? String ?? "a copy")." : made.isEmpty ? nil : "Created \(made.count) copies."].compactMap { $0 } + failures).joined(separator: " "))
        guard !made.isEmpty else { return }
        registerUndo(undo, name: made.count == 1 ? "Duplicate Sequence" : "Duplicate Sequences", made: made, admin: admin) { [weak self] admin, undo in
            await self?.duplicate(paths, admin: admin, undo: undo)
        }
    }

    /// Renders each sequence and writes `<name>.fseq` into the profile's Export folder. An open sequence is exported as
    /// it stands in its window.
    func exportFseq(_ paths: [String], admin: AdminModel) async {
        guard let core = admin.app.core else { return }
        var written: [String] = []
        var failures: [String] = []
        var unsaved = false
        for path in paths {
            let name = ((path as NSString).lastPathComponent as NSString).deletingPathExtension
            activity = "Exporting \(name).fseq…"
            switch await run("library_export_fseq", ["sequence": (path as NSString).lastPathComponent, "confirm": true], core: core) {
            case .success(let result):
                if let out = result["path"] as? String { written.append(out) }
                unsaved = unsaved || result["includesUnsavedChanges"] as? Bool == true
            case .failure(let message): failures.append(message)
            }
        }
        activity = nil
        noticeFile = written.first.map { URL(fileURLWithPath: $0) }
        let done = written.count == 1
            ? "Exported \((written[0] as NSString).lastPathComponent) to the Export folder\(unsaved ? ", including unsaved changes" : "")."
            : written.isEmpty ? nil : "Exported \(written.count) FSEQ files to the Export folder."
        show(([done].compactMap { $0 } + failures).joined(separator: " "))
    }

    func reveal(_ urls: [URL]) {
        if UITestHarness.isActive {
            UITestHarness.note("reveal \(urls.map(\.path).joined(separator: ", "))")
        } else {
            NSWorkspace.shared.activateFileViewerSelecting(urls)
        }
    }

    // MARK: Plumbing

    private enum Outcome {
        case success([String: Any])
        case failure(String)
    }

    private func run(_ command: String, _ arguments: [String: Any], core: VixenCore) async -> Outcome {
        guard let data = try? JSONSerialization.data(withJSONObject: arguments), let json = String(data: data, encoding: .utf8) else {
            return .failure("Couldn't send \(command).")
        }
        let reply: Result<String, StudioLibraryFailure> = await Task.detached {
            do { return .success(try core.studioSequenceCommand(command: command, argumentsJson: json)) } catch { return .failure(StudioLibraryFailure(error)) }
        }.value
        switch reply {
        case .success(let text):
            return .success((try? JSONSerialization.jsonObject(with: Data(text.utf8))) as? [String: Any] ?? [:])
        case .failure(let failure):
            return .failure(failure.text)
        }
    }

    private func show(_ text: String) {
        notice = text.isEmpty ? nil : text
    }

    /// Undo removes the created files (library_discard refuses any that changed or are open, and says so); Redo makes
    /// them again. When Undo keeps a file, the step stays on Undo instead, to try again once the file is closed.
    private func registerUndo(_ manager: UndoManager?, name: String, made: [[String: Any]], admin: AdminModel,
                              redo: @escaping @MainActor (AdminModel, UndoManager?) async -> Void) {
        guard let manager else { return }
        manager.registerUndo(withTarget: undoTarget) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self else { return }
                manager.registerUndo(withTarget: self.undoTarget) { _ in
                    MainActor.assumeIsolated { _ = Task { await redo(admin, manager) } }
                }
                manager.setActionName(name)
                Task {
                    let kept = await self.discard(made, admin: admin)
                    guard !kept.isEmpty else { return }
                    // Nothing to redo for what's still there: the Redo just registered would make it a second time.
                    manager.removeAllActions(withTarget: self.undoTarget)
                    self.registerUndo(manager, name: name, made: kept, admin: admin, redo: redo)
                }
            }
        }
        manager.setActionName(name)
    }

    /// Removes the files, returning the ones library_discard kept.
    private func discard(_ made: [[String: Any]], admin: AdminModel) async -> [[String: Any]] {
        guard let core = admin.app.core else { return made }
        var kept: [[String: Any]] = []
        var failures: [String] = []
        for file in made {
            let arguments: [String: Any] = [
                "sequence": file["file"] ?? "", "modified": file["modified"] ?? 0, "sizeBytes": file["sizeBytes"] ?? 0, "confirm": true,
            ]
            if case .failure(let message) = await run("library_discard", arguments, core: core) {
                failures.append("\(file["file"] as? String ?? "The sequence") was kept: \(message.prefix(1).lowercased() + message.dropFirst()).")
                kept.append(file)
            }
        }
        noticeFile = nil
        show(failures.isEmpty ? (made.count == 1 ? "Removed \(made[0]["file"] as? String ?? "the sequence")." : "Removed \(made.count) sequences.") : failures.joined(separator: " "))
        return kept
    }
}

private struct StudioLibraryFailure: Error, Sendable {
    let text: String
    nonisolated init(_ error: Error) {
        if case FfiError.Failed(let message) = error { text = message } else { text = "\(error)" }
    }
}

/// A sequence's state at a glance: unsaved, rendering or rendered (while open), audio missing, FSEQ export up to date.
struct StudioSequenceBadges: View {
    let unsaved: Bool
    let open: Bool
    let effects: UInt32?
    let rendered: UInt32?
    let audioMissing: Bool
    let audioName: String?
    let fseqCurrent: Bool

    var body: some View {
        HStack(spacing: StudioMetrics.spacingXS) {
            if unsaved {
                badge("Unsaved", .orange, "Open with changes that aren't saved yet. Save from its window.")
            }
            if let effects, let rendered, effects > 0 {
                if rendered >= effects {
                    badge("Rendered", .green, "Every effect is rendered, so it plays without waiting.")
                } else {
                    badge("Rendering \(Int((Double(rendered) / Double(effects) * 100).rounded(.down)))%", .secondary,
                          "Rendering \(rendered) of \(effects) effects in the open window.")
                }
            }
            if audioMissing {
                badge("Audio Missing", .red, "The show's Media folder has no \(audioName ?? "audio file"), so this sequence plays silent. Open it and choose the song again from the Audio menu.")
            }
            if fseqCurrent {
                badge("FSEQ", .blue, "The Export folder's FSEQ file is up to date with the saved sequence.")
            }
        }
    }

    private func badge(_ title: String, _ color: Color, _ help: String) -> some View {
        Text(title)
            .font(.caption2.weight(.semibold))
            .monospacedDigit()
            .padding(.horizontal, 6)
            .padding(.vertical, 1)
            .foregroundStyle(color)
            .background(color.opacity(0.15), in: Capsule())
            .help(help)
            .accessibilityLabel(title)
    }
}
