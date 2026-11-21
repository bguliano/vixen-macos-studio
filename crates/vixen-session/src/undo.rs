//! The sequencer's undo stack: `Vixen.Common/Controls/Undo/UndoManager.cs` and the actions in `TimedSequenceEditor/Undo`. Each
//! action holds the state to restore, and applying one gives back the action that reverses it.

use crate::sequence::EffectId;
use std::sync::Arc;
use uuid::Uuid;
use vixen_core::marks::Mark;
use vixen_core::{EffectIntents, MarkCollection, TimeSpan};
use vixen_effects::framework::Params;
use vixen_io::EffectNode;

/// `UndoManager`'s limit. The oldest action falls off.
const MAX_UNDO: usize = 1000;

/// An effect removed from the table, enough to put it back as it was.
#[derive(Clone)]
pub struct EffectSnapshot {
    pub id: EffectId,
    /// Where it was in document order.
    pub position: Option<usize>,
    pub node: EffectNode,
    /// The original captures this, then loses it on the way back in; here an undone delete returns to its layer.
    pub layer: Option<Uuid>,
    pub intents: Option<Arc<EffectIntents>>,
    /// Render ownership at removal. Restoring advances beyond this generation
    /// so results still arriving from the removed incarnation cannot apply.
    pub generation: u64,
    /// Whether cached intents actually cover the snapshot's current parameters.
    pub rendered_generation: u64,
}

/// `EffectsAddedUndoAction` and its siblings, which share code and differ in their wording.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum AddRemoveKind {
    /// "Added n effects" / "Removed n effects", a matching pair in the original.
    AddedOrRemoved,
    /// "Paste n effects".
    Pasted,
    /// "Cut n effects".
    Cut,
}

/// `ElementMoveType`, as `ElementsTimeChangedUndoAction` words it.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum MoveKind {
    Moved,
    Resize,
    StartAlign,
    EndAlign,
    Align,
    DurationAlign,
    StartToEndAlign,
    EndToStartAlign,
    CenterAlign,
    Distributed,
}

impl MoveKind {
    fn word(self) -> &'static str {
        match self {
            MoveKind::Moved => "Moved",
            MoveKind::Resize => "Resize",
            MoveKind::StartAlign => "Start Align",
            MoveKind::EndAlign => "End Align",
            MoveKind::Align => "Align",
            MoveKind::DurationAlign => "Duration Align",
            MoveKind::StartToEndAlign => "Start to End Align",
            MoveKind::EndToStartAlign => "End to Start Align",
            MoveKind::CenterAlign => "Center Align",
            MoveKind::Distributed => "Distributed",
        }
    }
}

/// Where an effect goes back to after a drag: `ElementTimeInfo` holds the row as well as the times.
#[derive(Clone, Debug)]
pub struct EffectPlacement {
    pub id: EffectId,
    pub start: TimeSpan,
    pub span: TimeSpan,
    /// (node id, node name). `None` for an effect with no element here.
    pub target: Option<(Uuid, String)>,
}

/// A mark's times, as `MarkTimeInfo` saves them.
#[derive(Clone, Debug, PartialEq)]
pub struct MarkPlacement {
    pub mark: Uuid,
    pub start: TimeSpan,
    pub duration: TimeSpan,
}

/// One step on the stack.
#[derive(Clone)]
pub enum UndoAction {
    // STUDIO-BEGIN sequence-settings-undo: shared reversible document length and layer configuration
    StudioSettings { label: String, length: TimeSpan, layers: vixen_core::layer::SequenceLayers },
    // STUDIO-END sequence-settings-undo
    // STUDIO-BEGIN sequence-inspector-undo: the sequence inspector's mark collection visibility, as (collection, lines, bar) to go back to
    StudioMarkVisibility { label: String, flags: Vec<(Uuid, bool, bool)> },
    // STUDIO-END sequence-inspector-undo
    /// Undoing removes these.
    EffectsAdded { kind: AddRemoveKind, ids: Vec<EffectId> },
    /// Undoing puts these back.
    EffectsRemoved { kind: AddRemoveKind, effects: Vec<EffectSnapshot> },
    /// `(effect, start, span)` to go back to.
    TimesChanged { kind: MoveKind, times: Vec<(EffectId, TimeSpan, TimeSpan)> },
    /// A timeline drag, which can also have moved effects between rows.
    PlacementChanged { kind: MoveKind, places: Vec<EffectPlacement> },
    /// Several actions that undo as one, with one description.
    Group { label: String, actions: Vec<UndoAction> },
    LayersChanged { layers: Vec<(EffectId, Uuid)> },
    /// `EffectsModifiedUndoAction` and `EffectsPropertyModifiedUndoAction`. `label` is what changed: "properties", or a
    /// property's display name.
    ParamsChanged { label: String, params: Vec<(EffectId, Params)> },
    /// The marks actions. `label` is the whole description, which depends on the edit and on the original's own wording.
    MarksChanged { label: String, marks: Vec<MarkCollection> },
    /// `MarksTimeChangedUndoAction`: swaps each mark's times, so a text edit made since isn't lost. Doesn't mark the sequence modified, as upstream
    /// doesn't, and a mark no longer in a collection drops out.
    MarkTimesChanged { label: String, marks: Vec<MarkPlacement> },
    /// `MarksAddedUndoAction`, as `(collection, mark)`. Undoing removes these.
    MarksAdded { label: String, marks: Vec<(Uuid, Mark)> },
    /// `MarksRemovedUndoAction`, as `(collection, mark)`. Undoing puts these back.
    MarksRemoved { label: String, marks: Vec<(Uuid, Mark)> },
    /// Effects a clipping move took off their last rows, which the document never heard of. Undo and redo both put them back, as
    /// `SwapElementPlacement` does with its null entries. A window reads `rows_restored` to follow.
    RowsRestored { ids: Vec<EffectId> },
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

impl UndoAction {
    /// The Edit menu name and drop-down text, as the original words each action.
    pub fn description(&self) -> String {
        match self {
            UndoAction::StudioSettings {label,..} => label.clone(), // STUDIO: document settings share the same undo menu
            UndoAction::StudioMarkVisibility { label, .. } => label.clone(), // STUDIO: the sequence inspector's mark visibility
            UndoAction::EffectsAdded { kind, ids } => added_removed_text(*kind, ids.len(), true),
            UndoAction::EffectsRemoved { kind, effects } => added_removed_text(*kind, effects.len(), false),
            UndoAction::TimesChanged { kind, times } => {
                format!("{} {} effect{}", kind.word(), times.len(), plural(times.len()))
            }
            UndoAction::PlacementChanged { kind, places } => {
                format!("{} {} effect{}", kind.word(), places.len(), plural(places.len()))
            }
            UndoAction::Group { label, .. } => label.clone(),
            UndoAction::LayersChanged { layers } => format!("Modified {} effect layer{}", layers.len(), plural(layers.len())),
            UndoAction::ParamsChanged { label, params } => format!("{} Effect(s) {label} modified", params.len()),
            UndoAction::MarksChanged { label, .. }
            | UndoAction::MarkTimesChanged { label, .. }
            | UndoAction::MarksAdded { label, .. }
            | UndoAction::MarksRemoved { label, .. } => label.clone(),
            UndoAction::RowsRestored { ids } => format!("Moved {} effect{}", ids.len(), plural(ids.len())),
        }
    }
}

fn added_removed_text(kind: AddRemoveKind, count: usize, is_add_side: bool) -> String {
    match (kind, is_add_side) {
        (AddRemoveKind::AddedOrRemoved, true) => format!("Added {count} effect{}", plural(count)),
        (AddRemoveKind::AddedOrRemoved, false) => format!("Removed {count} effect{}", plural(count)),
        (AddRemoveKind::Pasted, _) => format!("Paste {count} effect{}", plural(count)),
        (AddRemoveKind::Cut, _) => format!("Cut {count} effect{}", plural(count)),
    }
}

/// `UndoManager`. A new action clears the redo list, as `NSUndoManager` does. The original leaves it, and Redo can then act on
/// effects that no longer exist.
#[derive(Default)]
pub struct UndoStack {
    undo: Vec<UndoEntry>,
    redo: Vec<UndoEntry>,
}

/// An action and its wording. The wording stays with the entry on both lists: in the original the same action object moves between them, so
/// after undoing `Added 2 effects` the Redo list says `Added 2 effects`, though what's stored here to redo it is the reverse.
pub(crate) struct UndoEntry {
    pub(crate) action: UndoAction,
    pub(crate) description: String,
}

impl UndoStack {
    /// `AddUndoAction`.
    pub fn push(&mut self, action: UndoAction) {
        self.redo.clear();
        let description = action.description();
        self.push_undo_keeping_redo(UndoEntry { action, description });
    }

    pub(crate) fn push_undo_keeping_redo(&mut self, entry: UndoEntry) {
        self.undo.push(entry);
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
    }

    pub(crate) fn push_redo(&mut self, entry: UndoEntry) {
        self.redo.push(entry);
    }

    pub(crate) fn pop_undo(&mut self) -> Option<UndoEntry> {
        self.undo.pop()
    }

    pub(crate) fn pop_redo(&mut self) -> Option<UndoEntry> {
        self.redo.pop()
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Descriptions, newest first, for the Undo button's list.
    pub fn undo_descriptions(&self) -> Vec<String> {
        self.undo.iter().rev().map(|e| e.description.clone()).collect()
    }

    /// Descriptions, newest first, for the Redo button's list.
    pub fn redo_descriptions(&self) -> Vec<String> {
        self.redo.iter().rev().map(|e| e.description.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moved(n: usize) -> UndoAction {
        let times = (0..n).map(|i| (EffectId(i as u64), TimeSpan::ZERO, TimeSpan::ZERO)).collect();
        UndoAction::TimesChanged { kind: MoveKind::Moved, times }
    }

    #[test]
    fn descriptions_use_the_originals_wording() {
        assert_eq!(moved(1).description(), "Moved 1 effect");
        assert_eq!(moved(3).description(), "Moved 3 effects");
        let added = UndoAction::EffectsAdded { kind: AddRemoveKind::AddedOrRemoved, ids: vec![EffectId(1), EffectId(2)] };
        assert_eq!(added.description(), "Added 2 effects");
        let removed = UndoAction::EffectsRemoved { kind: AddRemoveKind::AddedOrRemoved, effects: Vec::new() };
        assert_eq!(removed.description(), "Removed 0 effects");
        let cut = UndoAction::EffectsRemoved { kind: AddRemoveKind::Cut, effects: Vec::new() };
        assert_eq!(cut.description(), "Cut 0 effects");
        let layers = UndoAction::LayersChanged { layers: vec![(EffectId(1), Uuid::nil())] };
        assert_eq!(layers.description(), "Modified 1 effect layer");
        let params = UndoAction::ParamsChanged { label: "Color".into(), params: Vec::new() };
        assert_eq!(params.description(), "0 Effect(s) Color modified");
    }

    #[test]
    fn new_action_clears_redo_and_list_is_newest_first() {
        let mut s = UndoStack::default();
        s.push(moved(1));
        s.push(moved(2));
        assert_eq!(s.undo_descriptions(), ["Moved 2 effects", "Moved 1 effect"]);
        let a = s.pop_undo().unwrap();
        s.push_redo(a);
        assert!(s.can_redo());
        s.push(moved(3));
        assert!(!s.can_redo());
    }

    #[test]
    fn oldest_action_falls_off_at_the_limit() {
        let mut s = UndoStack::default();
        for i in 0..1005 {
            s.push(moved(i));
        }
        assert_eq!(s.undo.len(), 1000);
        assert_eq!(s.undo_descriptions().last().unwrap(), "Moved 5 effects");
    }
}
