import AppKit
import VixenBridge
import VixenKit

// Studio: agent edits flash on the timeline (UI roadmap R39). The window's poll asks the document what agents added or
// changed since it last looked (`studio_agent_flash`, noted where agent edits go on the Undo stack) and flashes those
// effects once. The user's own edits and Undo never flash. It's the window's view of the document, so there is no
// command or Undo for it; an agent sees its edits in `sequence_status` and `sequence_effects` as before.
extension SequencerModel {
    func refreshStudioAgentFlash() {
        #if canImport(VixenTimeline)
        guard let canvas else { return }
        let first = studioAgentFlashRevision == nil
        let flash = canvas.studioFlashAgentEdits(since: studioAgentFlashRevision)
        studioAgentFlashRevision = flash.revision
        if UITestHarness.isActive {
            if first { registerAgentFlashHook() }
            if flash.effects > 0 { studioAgentFlashes.append(flash) }
        }
        #endif
    }

    #if canImport(VixenTimeline)
    /// `sequencer.agentFlash`: the flashes since the window opened, for the harness: how many, the effects and tiles of
    /// the last, and the revision the window has seen.
    private func registerAgentFlashHook() {
        UITestHarness.hooks["sequencer.agentFlash"] = { [weak self] _ in
            guard let self else { return "FAIL no sequencer" }
            let last = self.studioAgentFlashes.last
            let state: [String: Any] = ["flashes": self.studioAgentFlashes.count, "revision": self.studioAgentFlashRevision ?? 0,
                                        "lastEffects": last?.effects ?? 0, "lastTiles": last?.rects.count ?? 0]
            guard let data = try? JSONSerialization.data(withJSONObject: state, options: [.sortedKeys]) else { return "FAIL flash" }
            return String(decoding: data, as: UTF8.self)
        }
    }
    #endif
}
