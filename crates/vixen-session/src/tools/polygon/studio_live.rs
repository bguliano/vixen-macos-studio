//! Studio (B8): the Polygon Editor is live. Each change to the draft is written to the edited Morph effects at once, so
//! a preview shows it while the window is open, and closing the window (Done) records everything done in it as one
//! "Polygons" step on the sequence's undo history. Nothing is recorded when the shapes end where they started.
use super::morph::apply_shapes;
use super::*;
use crate::{
    sequence::{EffectId, SequenceSession},
    undo::UndoAction,
};

/// The effects one Polygon Editor window writes to, with their parameters from before it opened.
pub struct LiveTarget {
    original: Vec<(EffectId, Params)>,
}

impl LiveTarget {
    /// Takes the Morph effects among `effects` (each once) and remembers their parameters.
    pub fn new(s: &SequenceSession, effects: &[EffectId]) -> Self {
        let mut original: Vec<(EffectId, Params)> = Vec::new();
        for id in effects {
            if original.iter().any(|(seen, _)| seen == id) {
                continue;
            }
            let Some(e) = s.effect(*id) else { continue };
            if vixen_effect_registry::by_type_id(&e.node.type_id).is_none_or(|d| d.data_type != "MorphData") {
                continue;
            }
            original.push((*id, e.node.params.clone()));
        }
        Self { original }
    }

    /// Writes the draft's shapes to every effect still there whose parameters differ, without an undo record, and
    /// queues their renders. True when something changed.
    pub fn apply(&self, s: &mut SequenceSession, ed: &Editor) -> bool {
        let shapes = ed.finish();
        let mut changed = false;
        for (id, _) in &self.original {
            let Some(e) = s.effect(*id) else { continue };
            let old = &e.node.params;
            let new = apply_shapes(old, &shapes);
            if &new != old {
                s.set_params(*id, new);
                changed = true;
            }
        }
        if changed {
            s.render_all_dirty();
        }
        changed
    }

    /// The window closed: one undo step putting back the parameters of every effect that ended up different, or
    /// none. Returns the step's description.
    pub fn finish(&self, s: &mut SequenceSession) -> Option<String> {
        let before: Vec<(EffectId, Params)> = self
            .original
            .iter()
            .filter(|(id, old)| s.effect(*id).is_some_and(|e| &e.node.params != old))
            .cloned()
            .collect();
        if before.is_empty() {
            return None;
        }
        let action = UndoAction::ParamsChanged { label: "Polygons".into(), params: before };
        let label = action.description();
        s.record_undo(action);
        Some(label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sequence::SessionEnvironment;
    use std::sync::Arc;
    use vixen_core::{ElementTree, TimeSpan};
    use vixen_io::ModuleStore;

    fn session() -> (SequenceSession, EffectId) {
        let mut tree = ElementTree::new();
        let target = tree.add_string("Test", 10);
        let target = tree.node(target).id;
        let env = SessionEnvironment {
            video_cache: None,
            tree: Arc::new(tree),
            store: Arc::new(ModuleStore::new()),
            module_data_root: None,
            frame_ms: 50,
            media_dir: std::env::temp_dir(),
        };
        let mut session = SequenceSession::new_untitled(&env);
        let d = vixen_effect_registry::by_data_type("MorphData").unwrap();
        let id = session.add_new_effect(d.type_id, target, "Test", TimeSpan::ZERO, TimeSpan::from_ms(1000.)).unwrap();
        (session, id)
    }

    #[test]
    fn changes_reach_the_effect_at_once_and_closing_is_one_undo_step() {
        let (mut s, id) = session();
        let original = s.effect(id).unwrap().node.params.clone();
        let undo_before = s.undo_stack().undo_descriptions().len();
        let mut draft = open_from_sequence(&s, &[id], (1000., 1000.)).unwrap();
        let live = LiveTarget::new(&s, &[id, id]);
        assert!(!live.apply(&mut s, &draft), "an untouched draft changes nothing");

        draft.set_label("First".into());
        assert!(live.apply(&mut s, &draft));
        let first = s.effect(id).unwrap().node.params.clone();
        assert_ne!(first, original, "the effect has the change while the window is open");
        draft.set_label("Second".into());
        assert!(live.apply(&mut s, &draft));
        assert!(!live.apply(&mut s, &draft), "the same draft again is no change");
        assert_eq!(s.undo_stack().undo_descriptions().len(), undo_before, "nothing is recorded until the window closes");

        let label = live.finish(&mut s).unwrap();
        assert!(label.starts_with("1 Effect") && label.contains("Polygons"), "{label}");
        assert_eq!(s.undo_stack().undo_descriptions().len(), undo_before + 1, "two changes, one step");
        let last = s.effect(id).unwrap().node.params.clone();
        s.undo(1);
        assert_eq!(s.effect(id).unwrap().node.params, original, "Undo puts the polygons from before the window back");
        s.redo(1);
        assert_eq!(s.effect(id).unwrap().node.params, last, "Redo applies the window's result again");
    }

    #[test]
    fn ending_where_it_started_records_nothing() {
        let (mut s, id) = session();
        let undo_before = s.undo_stack().undo_descriptions().len();
        let mut draft = open_from_sequence(&s, &[id], (1000., 1000.)).unwrap();
        let (_, x, _, _) = draft.point_rows()[0];
        let live = LiveTarget::new(&s, &[id]);
        draft.set_point(0, "X", &(x + 1.).to_string()).unwrap();
        assert!(live.apply(&mut s, &draft));
        draft.set_point(0, "X", &x.to_string()).unwrap();
        assert!(live.apply(&mut s, &draft), "the point moved back");
        assert!(live.finish(&mut s).is_none());
        assert_eq!(s.undo_stack().undo_descriptions().len(), undo_before);
    }

    #[test]
    fn a_removed_effect_is_skipped() {
        let (mut s, id) = session();
        let mut draft = open_from_sequence(&s, &[id], (1000., 1000.)).unwrap();
        let live = LiveTarget::new(&s, &[id]);
        s.remove_effects(&[id]);
        draft.set_label("Changed".into());
        assert!(!live.apply(&mut s, &draft));
        assert!(live.finish(&mut s).is_none());
    }
}
