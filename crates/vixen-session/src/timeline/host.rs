//! What the timeline asks of the document. Upstream's control raises events and `TimedSequenceEditorForm` answers; here that's a
//! trait, so the logic can be tested with no session and the bindings can take the session's lock only when something is committed.

use super::{ElementDescription, ElementId};
use crate::sequence::SequenceSession;
use crate::undo::{EffectSnapshot, UndoAction};
use std::sync::Arc;
use uuid::Uuid;
use vixen_core::{MarkCollection, TimeSpan};

/// The document side of the timeline.
pub trait TimelineHost {
    /// `EndUpdate`: the effect takes its new times.
    fn commit_times(&mut self, changes: &[(ElementId, TimeSpan, TimeSpan)]);
    /// `EndUpdate`: the effect takes its new target.
    fn commit_target(&mut self, id: ElementId, target: Uuid, name: &str);
    /// `CloneElements` for one effect: same type, cloned data, same layer, at this place.
    fn clone_effect(&mut self, id: ElementId, target: Uuid, name: &str, start: TimeSpan, span: TimeSpan) -> Option<ElementId>;
    /// `CreateEffectInstanceWithDefaults` and `AddEffectNode`.
    fn add_effect(&mut self, type_id: Uuid, target: Uuid, name: &str, start: TimeSpan, span: TimeSpan) -> Option<ElementId>;
    /// `RemoveEffectNodeAndElement`.
    fn remove_effects(&mut self, ids: &[ElementId]) -> Vec<EffectSnapshot>;
    /// `AddUndoAction`, for an action already applied.
    fn record_undo(&mut self, action: UndoAction);
    /// `RenderElement` for whatever has changed.
    fn render_changes(&mut self);
    /// The marks after an edit. Effects that read marks render again.
    fn commit_marks(&mut self, marks: &[MarkCollection]);
    /// Type and layer names for an effect the host just made.
    fn describe(&mut self, id: ElementId) -> Option<ElementDescription>;
    /// `SequenceModified`, for a change that isn't to an effect or a mark: the ruler's range.
    fn mark_modified(&mut self);
    /// Marks changed with nothing following: not modified, nothing rendered again. Q43's one path.
    fn commit_marks_quietly(&mut self, marks: &[MarkCollection]);
}

impl TimelineHost for SequenceSession {
    fn commit_times(&mut self, changes: &[(ElementId, TimeSpan, TimeSpan)]) {
        self.set_times(changes);
    }

    fn commit_target(&mut self, id: ElementId, target: Uuid, name: &str) {
        self.set_target(id, target, name);
    }

    fn clone_effect(&mut self, id: ElementId, target: Uuid, name: &str, start: TimeSpan, span: TimeSpan) -> Option<ElementId> {
        let node = self.effect(id)?.node.clone();
        let layer = self.layer_of(id);
        let new = self.add_copy(node, target, name, start, span)?;
        if let Some(layer) = layer {
            self.set_layer(&[new], layer);
        }
        Some(new)
    }

    fn add_effect(&mut self, type_id: Uuid, target: Uuid, name: &str, start: TimeSpan, span: TimeSpan) -> Option<ElementId> {
        self.add_new_effect(type_id, target, name, start, span).inspect(|_| crate::studio_effects_library::used(type_id)) // STUDIO: Recently Used (R38)
    }

    fn remove_effects(&mut self, ids: &[ElementId]) -> Vec<EffectSnapshot> {
        SequenceSession::remove_effects(self, ids)
    }

    fn record_undo(&mut self, action: UndoAction) {
        SequenceSession::record_undo(self, action);
    }

    fn render_changes(&mut self) {
        self.render_all_dirty();
    }

    fn commit_marks(&mut self, marks: &[MarkCollection]) {
        self.set_marks(marks.to_vec());
    }

    fn describe(&mut self, id: ElementId) -> Option<ElementDescription> {
        describe_effect(self, id)
    }

    fn mark_modified(&mut self) {
        self.set_modified();
    }

    fn commit_marks_quietly(&mut self, marks: &[MarkCollection]) {
        self.set_marks_quietly(marks.to_vec());
    }
}

/// The tooltip's and the overlap menu's names for an effect in a session.
pub fn describe_effect(session: &SequenceSession, id: ElementId) -> Option<ElementDescription> {
    let entry = session.effect(id)?;
    let layers = &session.document().layers;
    let layer = layers.layers.get(layers.layer_index_for_effect(&entry.node.instance_id));
    Some(ElementDescription {
        type_id: entry.node.type_id,
        type_name: Arc::from(entry.descriptor.map_or("Unknown", |d| d.name)),
        layer_level: layer.map_or(0, |l| l.level),
        layer_name: Arc::from(layer.map_or("", |l| l.name.as_str())),
    })
}
