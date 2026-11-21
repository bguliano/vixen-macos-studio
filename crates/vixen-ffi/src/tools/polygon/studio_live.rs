//! Studio (B8): the Polygon Editor writes its draft to the effects as it changes, and records the whole window as one
//! undo step when it closes (`vixen_session::tools::polygon::studio_live`). Replaces the OK/Cancel `commit`.
use super::PolygonEditor;

#[uniffi::export]
impl PolygonEditor {
    /// Writes the draft to the edited effects now, without an undo record. True when an effect changed. The sample
    /// editor (no sequence) writes nothing.
    pub fn studio_apply_live(&self) -> bool {
        if *self.finished.lock() {
            return false;
        }
        let (Some((session, _)), Some(live)) = (&self.target, &self.live) else {
            return false;
        };
        let draft = self.draft.lock();
        live.apply(&mut session.lock(), &draft)
    }

    /// The window closed: writes the draft a last time and records everything since it opened as one undo step.
    /// Returns the step's description, or nothing when the effects ended where they started. Later calls do nothing.
    pub fn studio_finish_live(&self) -> Option<String> {
        let mut finished = self.finished.lock();
        if *finished {
            return None;
        }
        *finished = true;
        let (Some((session, _)), Some(live)) = (&self.target, &self.live) else {
            return None;
        };
        let draft = self.draft.lock();
        let mut session = session.lock();
        live.apply(&mut session, &draft);
        live.finish(&mut session)
    }
}
