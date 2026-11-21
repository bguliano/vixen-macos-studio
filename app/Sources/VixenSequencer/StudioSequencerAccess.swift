import VixenBridge
#if canImport(VixenTimeline)
import VixenTimeline
#endif

/// The sequencer's own ↑ and ↓ move the timeline's active row before the grid sees the key, so VoiceOver hears the row
/// from here (UI roadmap R43; the grid's other keys are in `StudioTimelineAccess.swift`).
extension SequencerModel {
    func studioAccessAfterKey(_ key: ShellKey, handled: Bool) {
        #if canImport(VixenTimeline)
        guard handled else { return }
        switch key {
        case .up, .down: canvas?.studioAccessAnnounceRow()
        default: break
        }
        #endif
    }
}
