//! Studio: the timeline grid's VoiceOver elements and keyboard moves (UI roadmap R43, `vixen_session::studio_access`).
//! A child module of `timeline`, so it reaches the controller's timeline without widening it.
use super::{TimelineController, TimelineUpdate};
use vixen_session::sequence::EffectId;
use vixen_session::timeline::RowId;

/// One visible effect as an accessibility element, in the grid's client coordinates (y down).
#[derive(uniffi::Record, Clone)]
pub struct TimelineStudioAccessEffect {
    pub id: u64,
    pub row: u32,
    pub label: String,
    pub selected: bool,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// A keyboard move: what to redraw, and what VoiceOver should say about where it landed (None when nothing moved).
#[derive(uniffi::Record)]
pub struct TimelineStudioAccessMove {
    pub update: TimelineUpdate,
    pub announcement: Option<String>,
}

#[uniffi::export]
impl TimelineController {
    /// The effects in view, one element each, rows top down.
    pub fn studio_access_effects(&self) -> Vec<TimelineStudioAccessEffect> {
        let t = self.timeline.lock();
        t.studio_access_effects()
            .into_iter()
            .map(|e| TimelineStudioAccessEffect { id: e.id.0, row: e.row.0, label: e.label, selected: e.selected, x: e.x, y: e.y, w: e.w, h: e.h })
            .collect()
    }

    /// The grid's value: active row, selection and cursor.
    pub fn studio_access_summary(&self) -> String {
        self.timeline.lock().studio_access_summary()
    }

    /// ←, →, Tab and ⇧Tab: the next or previous effect in the active row; `extend` adds it to the selection.
    pub fn studio_access_step(&self, direction: i32, extend: bool) -> TimelineStudioAccessMove {
        let mut said = None;
        let update = self.update_only(|t| said = t.studio_access_step(direction, extend));
        TimelineStudioAccessMove { update, announcement: said }
    }

    /// VoiceOver's press on an effect element.
    pub fn studio_access_select(&self, id: u64, row: u32) -> TimelineStudioAccessMove {
        let mut said = None;
        let update = self.update_only(|t| said = t.studio_access_select(EffectId(id), RowId(row)));
        TimelineStudioAccessMove { update, announcement: said }
    }

    /// ↑ and ↓, which also give the grid an active row when it has none.
    pub fn studio_access_move_row(&self, direction: i32) -> TimelineStudioAccessMove {
        let mut said = None;
        let update = self.update_only(|t| {
            if t.studio_access_move_row(direction) {
                said = t.studio_access_row_label();
            }
        });
        TimelineStudioAccessMove { update, announcement: said }
    }

    /// ⌥→ and ⌥←: opens or closes the active row.
    pub fn studio_access_open_row(&self, open: bool) -> TimelineStudioAccessMove {
        let mut said = None;
        let update = self.update_only(|t| {
            if t.studio_access_open_row(open) {
                said = t.studio_access_row_label();
            }
        });
        TimelineStudioAccessMove { update, announcement: said }
    }

    /// The active row's words (name, open or closed, how many effects), for VoiceOver after the sequencer's ↑ or ↓.
    pub fn studio_access_row_label(&self) -> Option<String> {
        self.timeline.lock().studio_access_row_label()
    }
}
