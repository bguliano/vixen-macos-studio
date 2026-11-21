import AppKit
import UniformTypeIdentifiers
import VixenBridge
import VixenKit
#if canImport(VixenTimeline)
import VixenTimeline
#endif

// Changing or removing the song (B3). Both are one step on the window's Undo, which puts back the old song, its timing
// source, its decoded audio and the length the change set (`studio_sequence_audio.rs`), so neither asks first: no
// "Remove existing audio?" and no resize question (the inspector's Fit Length to Song does that, on Undo too). Only
// replacing a different file of the same name in Media, which Undo can't put back, still asks. The waveform and the
// speed controls follow the session's song however it changed: here, Undo, or an agent's `sequence_audio`.

extension SequencerModel {
    /// Change Audio… (Add Audio… with none): choose a song and make it the sequence's, as one undo step.
    func studioChangeAudio() {
        Task {
            guard let url = await studioChooseAudioFile() else { return }
            await studioSetAudio(url)
        }
    }

    /// Remove Audio, as one undo step.
    func studioRemoveAudio() {
        guard let shell, ui?.removeAudioEnabled == true else { return }
        _ = shell.runAlertAction(action: .removeAudio)
        refresh()
    }

    func studioSetAudio(_ url: URL) async {
        guard let shell else { return }
        var replace = false
        if shell.audioImportConflict(path: url.path) {
            replace = await SequencerAlerts.confirm(
                title: "Replace the song in Media?",
                message: "The show's Media folder already has a different file named \(url.lastPathComponent). Replacing it can't be undone, and other sequences using it will play the new one.",
                style: .warning,
                confirm: "Replace",
                cancel: "Use Existing",
                window: window
            )
        }
        switch shell.addAudio(path: url.path, replaceExistingCopy: replace) {
        case .unsupported:
            await SequencerAlerts.tell(title: "Not an Audio File", message: "\(url.lastPathComponent) isn't an audio file Vixen plays.", style: .warning, window: window)
        case .unprocessable:
            await SequencerAlerts.tell(title: "Can't Read the Song", message: "Vixen can't read \(url.lastPathComponent). The previous song was removed; Undo brings it back.", style: .warning, window: window)
        case .added:
            break
        }
        refresh()
    }

    /// The timeline's waveform reloads when the session's song is no longer the one it drew.
    func studioFollowAudio() {
        #if canImport(VixenTimeline)
        guard let canvas, canvas.controller.studioAudioStale() else { return }
        canvas.apply(canvas.controller.audioChanged())
        #endif
    }

    private func studioChooseAudioFile() async -> URL? {
        if UITestHarness.isActive {
            return ProcessInfo.processInfo.environment["VIXEN_TEST_FILE"].map { URL(fileURLWithPath: $0) }
        }
        let panel = NSOpenPanel()
        panel.title = "Choose a Song"
        panel.prompt = "Use Song"
        panel.allowsMultipleSelection = false
        panel.canChooseDirectories = false
        panel.allowedContentTypes = sequencerAudioExtensions().compactMap { UTType(filenameExtension: $0) }
        if let window {
            return await panel.beginSheetModal(for: window) == .OK ? panel.url : nil
        }
        return panel.runModal() == .OK ? panel.url : nil
    }
}
