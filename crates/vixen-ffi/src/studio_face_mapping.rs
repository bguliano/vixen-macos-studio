//! Studio: Tools ▸ LipSync ▸ Edit Element Face Mapping's Face Setup visit as one step on the profile's shared undo history
//! (`studio_face_mapping`, the history `face_mapping_set` uses too), with the node select sheet's Undo and Redo of it.
use crate::core::VixenCore;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;
use vixen_session::app::AppCore;
use vixen_session::studio_display::FaceSetupStart;

/// One Face Setup visit, opened from the node select sheet on its chosen elements.
#[derive(uniffi::Object)]
pub struct StudioFaceSetupVisit {
    core: Arc<AppCore>,
    nodes: Vec<Uuid>,
    start: Mutex<Option<FaceSetupStart>>,
}

/// What a visit did: `changed` whether the Face properties changed, `rendered` the LipSync effects drawn again, `label` its undo
/// step (none when nothing changed, or when the display moved during the visit so Undo would take that back too).
#[derive(uniffi::Record)]
pub struct StudioFaceSetupOutcome {
    pub changed: bool,
    pub rendered: u32,
    pub label: Option<String>,
}

#[uniffi::export]
impl StudioFaceSetupVisit {
    /// Face Setup closed with Done: everything it changed is one step on the shared history, and LipSync effects render again.
    pub fn finish(&self) -> StudioFaceSetupOutcome {
        let Some(start) = self.start.lock().take() else {
            return StudioFaceSetupOutcome { changed: false, rendered: 0, label: None };
        };
        let done = self.core.studio_display.face_setup_finish(&self.core, start, &self.nodes);
        StudioFaceSetupOutcome { changed: done.changed, rendered: done.rendered as u32, label: done.label }
    }
}

#[uniffi::export]
impl VixenCore {
    /// Face Setup is about to open on `nodes` (the node select sheet's chosen elements), which it changes as it's used.
    pub fn studio_begin_face_setup(&self, nodes: Vec<String>) -> Arc<StudioFaceSetupVisit> {
        let core = Arc::clone(&self.inner);
        let nodes = nodes.iter().filter_map(|n| Uuid::parse_str(n).ok()).collect();
        let start = core.studio_display.face_setup_begin(&core);
        Arc::new(StudioFaceSetupVisit { core, nodes, start: Mutex::new(Some(start)) })
    }

    /// Undoes the shared history's latest step if it is still the one labelled `label` (another window or an agent may have moved it).
    pub fn studio_undo_face_mapping(&self, label: String) -> bool {
        step(&self.inner, "undo", &label)
    }

    pub fn studio_redo_face_mapping(&self, label: String) -> bool {
        step(&self.inner, "redo", &label)
    }

    /// Elements with a Face property as JSON: `{revision, phonemes, components, elements: [{id, name, phonemes, components, color}]}`.
    pub fn studio_face_mapping_list(&self) -> String {
        let revision = self.inner.display.revision();
        vixen_session::studio_face_mapping::describe(&self.inner.system.read(), revision).to_string()
    }
}

fn step(core: &AppCore, side: &str, label: &str) -> bool {
    let history = core.studio_display.history();
    if history[side].get(0).and_then(Value::as_str) != Some(label) {
        return false;
    }
    core.studio_display.execute(core, &format!("display_{side}"), &json!({}), false).is_ok()
}
