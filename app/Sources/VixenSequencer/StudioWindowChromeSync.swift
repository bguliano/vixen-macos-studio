import AppKit
import VixenBridge
import VixenKit

/// Studio: the sequencer window's chrome from Rust (`studio_window_chrome`, the same fields `sequence_status` gives
/// agents): the song as the subtitle, the edited dot and the toolbar's render ring. Read on every refresh and poll,
/// because renders and agent edits finish between UI changes.
extension SequencerModel {
    func refreshStudioChrome() {
        guard let session else { return }
        let chrome = session.studioWindowChrome()
        guard chrome != studioChrome else { return }
        studioChrome = chrome
        guard let window else { return }
        if UITestHarness.isActive {
            registerChromeHook(window: window)
        }
        if window.subtitle != chrome.subtitle {
            window.subtitle = chrome.subtitle
        }
        if window.isDocumentEdited != chrome.unsaved {
            window.isDocumentEdited = chrome.unsaved
        }
    }

    /// `sequencer.chrome`: what the title bar and the toolbar's Save and render ring show, for the harness.
    private func registerChromeHook(window: NSWindow) {
        UITestHarness.hooks["sequencer.chrome"] = { [weak self, weak window] _ in
            guard let self, let window, let chrome = self.studioChrome else { return "FAIL no sequencer" }
            let state: [String: Any] = ["title": window.title, "subtitle": window.subtitle, "edited": window.isDocumentEdited,
                                        "unsaved": chrome.unsaved, "rendering": chrome.rendering,
                                        "renderingPercent": chrome.renderingPercent, "effects": chrome.effects,
                                        "rendered": chrome.rendered, "renderHelp": chrome.renderHelp]
            guard let data = try? JSONSerialization.data(withJSONObject: state, options: [.sortedKeys]) else { return "FAIL chrome" }
            return String(decoding: data, as: UTF8.self)
        }
    }
}
