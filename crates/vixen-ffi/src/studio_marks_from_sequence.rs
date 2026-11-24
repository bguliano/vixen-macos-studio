//! The Mark Collections panel's Import ▸ Timing from Sequence… (#61): a `.tim` from any profile, read in place, offered in the Select
//! Imported Mark Collections dialog as mark collections built from its effects (`vixen-session` `studio_marks_from_sequence.rs`). The
//! commit is the panel's import commit, one step on the sequence's Undo.
use super::{logic, MarkImportStep, MarksPanel, Pending};
use std::path::Path;
use vixen_session::studio_sequence::marks_from_sequence as timing;

const ERROR_TITLE: &str = "Import Timing Error";

#[uniffi::export]
impl MarksPanel {
    /// Reads the chosen sequence and remembers its folder. `merged` makes one collection for every effect instead of one per element;
    /// `offset_ms` moves every mark; `snap_to` is a collection id in this sequence whose nearest mark each start moves to. The file's own
    /// mark collections are offered too. Nothing changes until `commit_import`.
    pub fn studio_import_sequence_timing(&self, path: String, merged: bool, offset_ms: f64, snap_to: Option<String>) -> MarkImportStep {
        let path = Path::new(&path);
        logic::remember_folder(path);
        let failed = |message: String| {
            *self.pending.lock() = Pending::None;
            vixen_session::logging::error("MarkImportExportService", "There was an error importing the sequence's timing.", Some(&message));
            MarkImportStep::Failed { title: ERROR_TITLE.into(), message }
        };
        let snap = match snap_to {
            Some(id) => match timing::snap_points(self.session.lock().marks(), &id) {
                Ok(points) => points,
                Err(e) => return failed(e),
            },
            None => Vec::new(),
        };
        let source = match timing::read(path) {
            Ok(source) => source,
            Err(e) => return failed(e),
        };
        let options = timing::TimingOptions { merged, offset_ms, snap_to: snap, include_marks: true, ..Default::default() };
        let candidates = timing::candidates(&source, &options);
        if candidates.is_empty() {
            return failed(format!("“{}” has no effects or marks to import.", source.name));
        }
        self.step_for(logic::ImportRead::Candidates(candidates))
    }
}
