import AppKit
import VixenBridge
import VixenKit

// View ▸ Row Height (UI roadmap R36): Compact or Regular, remembered by the sequence. The menu runs the
// sequence_row_height command through the window's own document, exactly as an agent's call does, and the timeline
// follows the document's new default row height at once.

extension SequencerModel {
    func setStudioRowHeight(_ mode: String) {
        #if canImport(VixenTimeline)
        guard let session, let canvas else { return }
        // The document's default becomes the window's own first, so the choice scales from the rows as shown.
        canvas.controller.storeViewState()
        _ = try? session.studioCommand(command: "sequence_row_height", argumentsJson: "{\"rowHeight\":\"\(mode)\"}")
        canvas.apply(canvas.controller.refresh())
        refreshStudioRowHeight()
        #endif
    }

    /// Keeps the menu's checkmark on the window's row height, which agents and row zoom also change.
    func refreshStudioRowHeight() {
        #if canImport(VixenTimeline)
        let mode = canvas?.controller.studioRowHeight().mode
        guard studioRowHeight != mode else { return }
        // The first reading after the window loads hands the harness this window.
        if studioRowHeight == nil, UITestHarness.isActive {
            registerRowHeightHook()
        }
        studioRowHeight = mode
        #endif
    }

    #if canImport(VixenTimeline)
    /// `sequencer.rows`: the row height and how many rows the grid shows, for the harness. `path` ["compact"] or
    /// ["regular"] chooses one first, as the menu does.
    private func registerRowHeightHook() {
        UITestHarness.hooks["sequencer.rows"] = { [weak self] path in
            guard let self, let canvas = self.canvas else { return "FAIL no sequencer" }
            if let mode = path.first {
                self.setStudioRowHeight(mode)
            }
            let state = canvas.controller.studioRowHeight()
            let fields: [String: Any] = ["mode": state.mode, "heightPt": state.heightPt, "rowsInView": state.rowsInView]
            guard let data = try? JSONSerialization.data(withJSONObject: fields, options: [.sortedKeys]) else { return "FAIL rows" }
            return String(decoding: data, as: UTF8.self)
        }
    }
    #endif
}
