//! Studio: the patching graph's VoiceOver labels, Tab order and keyboard "Connect To" (UI roadmap R43,
//! `vixen_session::studio_access`). Each call builds the scene the canvas shows from the same selection, so ids match
//! it.
use crate::setup::DisplaySetupSession;

/// The visible shapes in Tab order: `ids[i]` reads `labels[i]`.
#[derive(uniffi::Record, Clone, Default)]
pub struct SetupGraphStudioAccess {
    pub ids: Vec<u64>,
    pub labels: Vec<String>,
}

#[uniffi::export]
impl DisplaySetupSession {
    /// What VoiceOver reads for each visible shape, in reading order (columns left to right, top down).
    pub fn studio_graph_access(&self, selection: Vec<String>, rows: Vec<String>, width: i32) -> SetupGraphStudioAccess {
        let s = self.inner.read();
        let outputs = s.output_selection(&rows);
        let access = vixen_session::studio_access::graph_access(&s.graph_scene(&selection, &outputs, width));
        SetupGraphStudioAccess { ids: access.ids, labels: access.labels }
    }

    /// The shapes a connection from the shape `source` (an id) can go to, in reading order: the Connect To menu.
    pub fn studio_graph_connect_targets(&self, selection: Vec<String>, rows: Vec<String>, width: i32, source: u64) -> Vec<u64> {
        let s = self.inner.read();
        let outputs = s.output_selection(&rows);
        let scene = s.graph_scene(&selection, &outputs, width);
        vixen_session::studio_access::graph_connect_targets(&scene, source, |from, to| s.graph_connect_candidate(from, to))
    }
}
