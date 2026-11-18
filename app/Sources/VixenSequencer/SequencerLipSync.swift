import AppKit
import SwiftUI
import VixenBridge
import VixenKit

#if canImport(VixenMarks)
import VixenMarks
#endif
#if canImport(VixenTimeline)
import VixenTimeline
#endif

// MARK: Tools > LipSync, F87–F90, F299, F301

extension SequencerModel {
    /// F87: the submenu's items.
    func reloadLipSyncMaps() {
        let maps = shell?.lipsyncMaps() ?? []
        if maps.map(\.name) != lipSyncMapList.map(\.name) || maps.map(\.isDefault) != lipSyncMapList.map(\.isDefault) {
            lipSyncMapList = maps
        }
    }

    func setDefaultLipSyncMap(_ name: String) {
        guard let shell, shell.setDefaultLipsyncMap(name: name) else { return }
        reloadLipSyncMaps()
        refresh()
    }

    /// `ClipboardPaste(pasteTime)` for the converter, which has just put its effects on the clipboard. The count is what it reports on.
    func pasteAtTime(ticks: Int64) -> UInt32 {
        guard let shell else { return 0 }
        let result = shell.pasteAtTime(json: SequencerPasteboard.readJSON(), ticks: ticks)
        afterCommand(result.outcome)
        return result.pasted
    }

    /// Whether the marks side's Image Maps sheet is built in.
    static var hasImageMaps: Bool {
        #if canImport(VixenMarks)
        true
        #else
        false
        #endif
    }

    /// F88 and F146: `LipSyncMapSelector`. When something changed, the sheet's own close has already marked the open sequences modified,
    /// rendered their LipSync effects again and saved the configuration.
    func openImageMaps() {
        #if canImport(VixenMarks)
        guard let window, let core = app.core else { return }
        KitSheet.present(on: window) { dismiss in
            ImageMapsSheet(core: core) { [weak self] changed in
                dismiss()
                self?.reloadLipSyncMaps()
                if changed {
                    self?.refresh()
                }
            }
        }
        #endif
    }

    /// F89: the marks area owns selection, Face Setup, saving and rerendering the affected LipSync effects.
    func openFaceMapping() {
        #if canImport(VixenMarks)
        guard let window, let core = app.core else { return }
        KitSheet.present(on: window) { dismiss in
            FaceMappingNodeSelectSheet(core: core, model: app) { [weak self] saved in
                dismiss()
                if saved {
                    self?.refresh()
                }
            }
        }
        #endif
    }

    #if canImport(VixenMarks)
    /// F90. Upstream's `Active` flag is static, so there's one converter however many sequencers are open, and it belongs to the one that opened
    /// it. Choosing the item again brings it forward, where upstream does nothing.
    func openLyricConverter() {
        if let open = LyricConverterWindow.current {
            open.makeKeyAndOrderFront(nil)
            return
        }
        guard let session, let window else { return }
        let panel = LyricConverterWindow(model: self, session: session)
        LyricConverterWindow.current = panel
        if let frame = window.screen?.visibleFrame, panel.frame.origin == .zero {
            panel.setFrameOrigin(NSPoint(x: window.frame.midX - panel.frame.width / 2, y: min(window.frame.midY - panel.frame.height / 2, frame.maxY - panel.frame.height)))
        }
        // `Show(this)`: it stays above its sequencer.
        window.addChildWindow(panel, ordered: .above)
        panel.makeKeyAndOrderFront(nil)
    }

    func closeLyricConverter() {
        guard let open = LyricConverterWindow.current, open.owner === self else { return }
        open.close()
    }

    /// F299's Word half: each selected mark's phonemes, asking for a mapping where the dictionary has none. Cancel skips the word; so does
    /// OK with nothing mapped, after writing the empty mapping as upstream does. The answers go back keyed by text, so of two marks with the same unknown word the last answer serves both, where upstream
    /// would skip the first and break down the second.
    func phonemes(for words: [String]) -> [MarkWordPhonemes] {
        guard let session else { return [] }
        var found = session.wordPhonemes(words: words)
        for index in found.indices where found[index].phonemes.isEmpty {
            let word = found[index].word
            // An earlier answer in this run may have covered it.
            if let again = session.wordPhonemes(words: [word]).first, !again.phonemes.isEmpty {
                found[index] = again
                continue
            }
            let clean = shell?.breakdownCleanWord(text: word) ?? word
            guard let mapped = askForMapping(word: clean) else { continue }
            if let problem = session.addPhonemeMapping(word: clean, mapped: mapped) {
                let alert = NSAlert()
                alert.alertStyle = .warning
                alert.messageText = problem.title
                alert.informativeText = problem.message
                if !UITestHarness.isActive {
                    alert.runModal()
                }
            }
            if !mapped.isEmpty, let again = session.wordPhonemes(words: [word]).first {
                found[index] = again
            }
        }
        return found
    }

    /// `GetUserMappingForFailedWord`: the Text Conversion Failure dialog, modal as upstream's `ShowDialog`, because the breakdown waits on it.
    /// Nil for Cancel.
    private func askForMapping(word: String) -> String? {
        guard let window else { return nil }
        var answer: String?
        let sheet = NSPanel(contentRect: .zero, styleMask: [.titled], backing: .buffered, defer: false)
        sheet.title = "Text Conversion Failure"
        sheet.isReleasedWhenClosed = false
        let content = NSHostingView(rootView: LyricFailureSheet(word: word) { mapped in
            answer = mapped
            NSApp.stopModal()
        }.studioAppearance()) // STUDIO: follow the chosen appearance
        sheet.contentView = content
        sheet.setContentSize(content.fittingSize)
        window.beginSheet(sheet)
        NSApp.runModal(for: sheet)
        window.endSheet(sheet)
        sheet.orderOut(nil)
        return answer
    }
    #endif

    func registerLipSyncHooks() {
        // `sequencer.lipSyncMaps`: the Default Image Map items, the default starred. With path [name]: chooses one first.
        UITestHarness.hooks["sequencer.lipSyncMaps"] = { [weak self] args in
            guard let self else { return "FAIL no sequencer" }
            if let name = args.first {
                self.setDefaultLipSyncMap(name)
            }
            self.reloadLipSyncMaps()
            return self.lipSyncMapList.map { $0.isDefault ? "*\($0.name)" : $0.name }.joined(separator: "|")
        }
        // `sequencer.state`: what a script checks an edit by. With path [ticks]: moves the cursor there first.
        UITestHarness.hooks["sequencer.state"] = { [weak self] args in
            guard let self, let shell = self.shell else { return "FAIL no sequencer" }
            #if canImport(VixenTimeline)
            if let ticks = args.first.flatMap(Int64.init), let canvas = self.canvas {
                canvas.apply(canvas.controller.setCursor(time: ticks))
            }
            #endif
            self.refresh()
            let state = shell.uiState()
            return "effects=\(state.effectCount) selected=\(state.selectionCount) modified=\(state.modified) undo=\(state.undoName ?? "-") cursor=\(shell.cursorTicks()) clipboard=\(SequencerPasteboard.count())"
        }
        // `sequencer.find` with path [effect name]: the Find panel's rows for it, as "row@start".
        UITestHarness.hooks["sequencer.find"] = { [weak self] args in
            guard let shell = self?.shell, let name = args.first else { return "FAIL needs an effect name" }
            return shell.findRows(layersMode: false, search: name).map { "\($0.rowName)@\($0.startText)" }.joined(separator: "|")
        }
        #if canImport(VixenMarks)
        // `sequencer.phonemes` with path [words]: starts Breakdown Word's lookup and returns at once, since an unknown word puts up a modal
        // sheet for the script to answer. `sequencer.phonemesResult` reads what came of it.
        UITestHarness.hooks["sequencer.phonemes"] = { [weak self] args in
            guard let self else { return "FAIL no sequencer" }
            self.lastPhonemes = nil
            RunLoop.main.perform {
                MainActor.assumeIsolated {
                    self.lastPhonemes = self.phonemes(for: args).map { "\($0.word)=\($0.phonemes.joined(separator: " "))" }.joined(separator: "|")
                }
            }
            return "started"
        }
        UITestHarness.hooks["sequencer.phonemesResult"] = { [weak self] _ in
            self?.lastPhonemes ?? "FAIL still waiting"
        }
        #endif
    }
}

#if canImport(VixenMarks)
/// F90's window: "LipSync Text Convert", not modal, above the sequencer that opened it and closed with it.
final class LyricConverterWindow: NSPanel, NSWindowDelegate {
    static weak var current: LyricConverterWindow?
    private(set) weak var owner: SequencerModel?

    init(model: SequencerModel, session: SequencerSession) {
        owner = model
        super.init(
            contentRect: NSRect(x: 0, y: 0, width: 340, height: 480),
            styleMask: [.titled, .closable, .resizable, .utilityWindow],
            backing: .buffered,
            defer: false
        )
        title = "LipSync Text Convert"
        isFloatingPanel = false
        hidesOnDeactivate = false
        isReleasedWhenClosed = false
        delegate = self
        let view = LyricConverterView(
            session: session,
            cursorPosition: { [weak model] in model?.shell?.cursorTicks() ?? 0 },
            pasteAtTime: { [weak model] ticks in model?.pasteAtTime(ticks: ticks) ?? 0 }
        )
        contentView = NSHostingView(rootView: view.studioAppearance()) // STUDIO: follow the chosen appearance
        minSize = NSSize(width: 320, height: 440)
    }

    func windowWillClose(_ notification: Notification) {
        parent?.removeChildWindow(self)
        if LyricConverterWindow.current === self {
            LyricConverterWindow.current = nil
        }
    }
}
#endif
