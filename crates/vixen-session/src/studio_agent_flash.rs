//! Studio: agent edits flash on the timeline (UI roadmap R39). Each agent edit that adds, moves or changes effects
//! notes them here, on the document, as it goes on the Undo stack; an open sequencer window asks for what's new since
//! it last looked and flashes those effects once, as the show window's toolbar shows the agent working.
//!
//! The note is taken where every agent edit is recorded (`studio_sequence::record` and `effects_batch`), so a new
//! command can't forget it. Removed effects have nothing left to flash, and the user's own edits never flash. Like the
//! toolbar's activity it is the window's view of the document, not an edit: nothing here is saved or undone.
use crate::sequence::EffectId;
use crate::timeline::{IRect, Timeline};
use crate::undo::UndoAction;
use std::collections::VecDeque;

/// More than this many effects waiting and the oldest stop counting: a window that comes back after a long batch
/// flashes the latest ones, not hundreds of thousands.
const KEPT: usize = 4096;

#[derive(Default)]
pub struct AgentFlash {
    revision: u64,
    /// `(revision, effect)`, oldest first.
    recent: VecDeque<(u64, EffectId)>,
}

impl AgentFlash {
    /// Notes the effects an agent's edit added or changed, as one new revision.
    pub fn note(&mut self, action: &UndoAction) {
        let mut ids = Vec::new();
        touched(action, &mut ids);
        if ids.is_empty() {
            return;
        }
        self.revision += 1;
        for id in ids {
            self.recent.push_back((self.revision, id));
        }
        while self.recent.len() > KEPT {
            self.recent.pop_front();
        }
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The effects agents touched after `revision`, each once, with the revision to ask from next time.
    pub fn since(&self, revision: u64) -> (u64, Vec<EffectId>) {
        let mut ids: Vec<EffectId> = self.recent.iter().filter(|(r, _)| *r > revision).map(|(_, id)| *id).collect();
        ids.sort_unstable();
        ids.dedup();
        (self.revision, ids)
    }
}

/// The effects an action leaves in the sequence having added or changed. Marks, settings and removals touch none.
fn touched(action: &UndoAction, out: &mut Vec<EffectId>) {
    match action {
        UndoAction::Group { actions, .. } => actions.iter().for_each(|a| touched(a, out)),
        UndoAction::EffectsAdded { ids, .. } | UndoAction::RowsRestored { ids } => out.extend(ids),
        UndoAction::TimesChanged { times, .. } => out.extend(times.iter().map(|(id, _, _)| *id)),
        UndoAction::PlacementChanged { places, .. } => out.extend(places.iter().map(|p| p.id)),
        UndoAction::LayersChanged { layers } => out.extend(layers.iter().map(|(id, _)| *id)),
        UndoAction::ParamsChanged { params, .. } => out.extend(params.iter().map(|(id, _)| *id)),
        UndoAction::StudioSettings { .. }
        | UndoAction::StudioMarkVisibility { .. }
        | UndoAction::EffectsRemoved { .. }
        | UndoAction::MarksChanged { .. }
        | UndoAction::MarkTimesChanged { .. }
        | UndoAction::MarksAdded { .. }
        | UndoAction::MarksRemoved { .. } => {}
    }
}

impl crate::sequence::SequenceSession {
    pub fn studio_agent_flash(&self) -> &AgentFlash {
        &self.studio_agent_flash
    }
}

impl Timeline {
    /// Canvas rectangles for these effects in every row they show in, for the grid's flash. Effects in collapsed rows
    /// or outside the drawn span have none.
    pub fn studio_flash_rects(&self, ids: &[EffectId]) -> Vec<IRect> {
        let mut rects = Vec::new();
        for id in ids {
            for row in self.element_rows(*id) {
                if let Some(r) = self.occurrence_rect_in_view(*id, row) {
                    if r.w > 0 && r.h > 0 {
                        rects.push(r.rect());
                    }
                }
            }
        }
        rects
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::undo::{AddRemoveKind, MoveKind};
    use vixen_core::TimeSpan;

    #[test]
    fn notes_added_and_changed_effects_once_per_edit() {
        let mut flash = AgentFlash::default();
        assert_eq!(flash.since(0), (0, vec![]));
        flash.note(&UndoAction::Group {
            label: "Add effects (Agent)".into(),
            actions: vec![UndoAction::EffectsAdded { kind: AddRemoveKind::AddedOrRemoved, ids: vec![EffectId(3), EffectId(1)] }],
        });
        assert_eq!(flash.since(0), (1, vec![EffectId(1), EffectId(3)]));
        flash.note(&UndoAction::TimesChanged { kind: MoveKind::Moved, times: vec![(EffectId(3), TimeSpan::ZERO, TimeSpan::ZERO)] });
        // Only what changed after the window last looked.
        assert_eq!(flash.since(1), (2, vec![EffectId(3)]));
        assert_eq!(flash.since(2), (2, vec![]));
        // An edit that touches no effect makes no revision.
        flash.note(&UndoAction::MarksChanged { label: "Onset marks".into(), marks: vec![] });
        flash.note(&UndoAction::EffectsRemoved { kind: AddRemoveKind::AddedOrRemoved, effects: vec![] });
        assert_eq!(flash.revision(), 2);
    }

    #[test]
    fn keeps_only_the_latest_effects() {
        let mut flash = AgentFlash::default();
        let ids: Vec<EffectId> = (0..(KEPT as u64 + 10)).map(EffectId).collect();
        flash.note(&UndoAction::EffectsAdded { kind: AddRemoveKind::AddedOrRemoved, ids });
        let (_, waiting) = flash.since(0);
        assert_eq!(waiting.len(), KEPT);
        assert_eq!(waiting[0], EffectId(10));
    }
}
