//! Studio: View ▸ Row Height, Compact or Regular (UI roadmap R36). Compact rows are 20 points, Regular are vanilla's 32,
//! so a compact grid fits 1.6 times as many rows. Choosing one scales every row by the same factor, so rows made taller
//! or shorter by hand keep their proportions. The choice is the sequence's `DefaultRowHeight` with its `RowSettings`,
//! which the file already carries, so it is remembered per sequence and Windows Vixen and vanilla open it as saved.
//!
//! `sequence_row_height` is the one command, for the menu and for agents. It writes the document; an open window's
//! timeline sees the new default on its next sync and scales its own rows, which a save then writes back. Like zoom it
//! is the window's view rather than an edit, so it isn't on Undo: choosing the other height reverses it.
use crate::sequence::SequenceSession;
use crate::timeline::rows::{RowTree, DEFAULT_ROW_HEIGHT, MIN_ROW_HEIGHT};
use crate::timeline::Timeline;
use serde_json::{json, Value};
use std::sync::atomic::Ordering;

pub const COMPACT_ROW_HEIGHT: i32 = 20;
pub const REGULAR_ROW_HEIGHT: i32 = DEFAULT_ROW_HEIGHT;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowHeight {
    Compact,
    Regular,
}

impl RowHeight {
    /// Which one a default row height is closer to. 0 is a file's way of saying 32.
    pub fn of(default_height: i32) -> RowHeight {
        let h = if default_height != 0 { default_height } else { DEFAULT_ROW_HEIGHT };
        if h <= (COMPACT_ROW_HEIGHT + REGULAR_ROW_HEIGHT) / 2 { RowHeight::Compact } else { RowHeight::Regular }
    }

    pub fn height(self) -> i32 {
        match self {
            RowHeight::Compact => COMPACT_ROW_HEIGHT,
            RowHeight::Regular => REGULAR_ROW_HEIGHT,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            RowHeight::Compact => "compact",
            RowHeight::Regular => "regular",
        }
    }

    pub fn parse(text: &str) -> Option<RowHeight> {
        match text {
            "compact" => Some(RowHeight::Compact),
            "regular" => Some(RowHeight::Regular),
            _ => None,
        }
    }
}

/// One row's height when the default goes from `from` to `to`: scaled, rounded, at least the timeline's floor.
fn scaled(height: i32, from: i32, to: i32) -> i32 {
    let from = if from > 0 { from } else { DEFAULT_ROW_HEIGHT };
    ((height as f64 * to as f64 / from as f64).round() as i32).max(MIN_ROW_HEIGHT)
}

impl RowTree {
    /// Every row, and the default new rows take, scaled so the default becomes `to`. False when it already is.
    pub fn studio_set_default_height(&mut self, to: i32) -> bool {
        let from = self.row_height;
        if from == to {
            return false;
        }
        for row in &mut self.rows {
            row.height = scaled(row.height, from, to);
        }
        self.row_height = to;
        self.relayout();
        true
    }
}

impl Timeline {
    /// The window's own row height, which the View menu checks.
    pub fn studio_row_height(&self) -> RowHeight {
        RowHeight::of(self.rows.row_height)
    }

    /// Rows the grid shows at least part of, for the harness.
    pub fn studio_rows_in_view(&self) -> usize {
        self.rows.visible_rows(self.vertical_offset, self.vertical_offset + self.grid_height).len()
    }

    /// Scales the rows to `height`, as F31's row zoom does: a view change that saves silently at close.
    pub fn studio_set_row_height(&mut self, height: RowHeight) -> bool {
        if !self.rows.studio_set_default_height(height.height()) {
            return false;
        }
        self.rows_changed();
        self.mark_editor_state_modified();
        true
    }

    /// From `sync_from_session`: a default row height the document gained from outside (`sequence_row_height`) is applied
    /// to this window's rows. A save writes the window's own default to the document first, which changes nothing here.
    pub(crate) fn studio_follow_row_height(&mut self, session: &SequenceSession) {
        let theirs = session.document().default_row_height;
        if theirs == self.studio_seen_row_height {
            return;
        }
        self.studio_seen_row_height = theirs;
        let to = if theirs != 0 { theirs } else { DEFAULT_ROW_HEIGHT };
        if to != self.rows.row_height && self.rows.studio_set_default_height(to) {
            self.rows_changed();
            self.mark_editor_state_modified();
        }
    }
}

pub fn tools() -> Vec<(&'static str, &'static str, Value, Vec<&'static str>, bool)> {
    let string = json!({"type":"string","minLength":1,"maxLength":256});
    vec![(
        "sequence_row_height",
        "Read or set the timeline's row height, as View ▸ Row Height does: compact (20 pt rows, 1.6 times as many in view) or regular (32 pt). Every row scales by the same factor, so hand-sized rows keep their proportions. Remembered in the sequence (its DefaultRowHeight and row settings) when it is saved, and an open window follows at once. A view setting, like zoom, so not on Undo. Omit rowHeight to read. Returns rowHeight, heightPt and changed.",
        json!({"sequenceId":string,"rowHeight":{"type":"string","enum":["compact","regular"]}}),
        vec!["sequenceId"],
        false,
    )]
}

/// `sequence_row_height`.
pub fn execute(s: &mut SequenceSession, a: &Value) -> Result<Value, String> {
    let doc = s.document_mut();
    let from = if doc.default_row_height != 0 { doc.default_row_height } else { DEFAULT_ROW_HEIGHT };
    let mut changed = false;
    if let Some(text) = a.get("rowHeight") {
        let to = RowHeight::parse(text.as_str().unwrap_or_default()).ok_or("rowHeight must be compact or regular")?.height();
        if to != from {
            for (_, setting) in &mut doc.row_settings {
                setting.row_height = scaled(setting.row_height, from, to);
            }
            doc.default_row_height = to;
            changed = true;
        }
    }
    let now = if doc.default_row_height != 0 { doc.default_row_height } else { DEFAULT_ROW_HEIGHT };
    if changed {
        // An open window syncs on the next edit count.
        s.counters.edits.fetch_add(1, Ordering::Relaxed);
    }
    Ok(json!({"rowHeight": RowHeight::of(now).name(), "heightPt": now, "changed": changed}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sequence::SessionEnvironment;
    use crate::timeline::RowId;
    use std::sync::Arc;
    use uuid::Uuid;
    use vixen_core::ElementTree;
    use vixen_io::ModuleStore;

    fn session(elements: usize) -> SequenceSession {
        let mut tree = ElementTree::new();
        for i in 0..elements {
            tree.add_node(Uuid::new_v4(), &format!("Prop {i}"), None);
        }
        let env = SessionEnvironment {
            video_cache: None,
            tree: Arc::new(tree),
            store: Arc::new(ModuleStore::new()),
            module_data_root: None,
            frame_ms: 25,
            media_dir: std::env::temp_dir(),
        };
        SequenceSession::new_untitled(&env)
    }

    fn rows_in_view(t: &Timeline, height: i32) -> usize {
        t.rows.visible_rows(0, height).len()
    }

    #[test]
    fn compact_fits_at_least_half_again_as_many_rows() {
        let mut s = session(200);
        let mut t = Timeline::new();
        t.load_from_session(&s, Vec::new());
        t.set_viewport(1000, 800, 1000);
        assert_eq!(t.studio_row_height(), RowHeight::Regular);
        let regular = rows_in_view(&t, 800);
        assert!(t.studio_set_row_height(RowHeight::Compact));
        assert!(!t.studio_set_row_height(RowHeight::Compact), "already compact");
        let compact = rows_in_view(&t, 800);
        assert!(compact * 2 >= regular * 3, "{compact} compact rows vs {regular} regular");
        assert!(t.is_editor_state_modified(), "saves silently at close");

        // Remembered by the sequence: a save stores it, and a fresh window opens compact.
        t.store_view_state(&mut s);
        assert_eq!(s.document().default_row_height, COMPACT_ROW_HEIGHT);
        let mut reopened = Timeline::new();
        reopened.load_from_session(&s, Vec::new());
        assert_eq!(reopened.studio_row_height(), RowHeight::Compact);
        assert!(reopened.rows.rows.iter().all(|r| r.height == COMPACT_ROW_HEIGHT));
    }

    #[test]
    fn rows_sized_by_hand_keep_their_proportions() {
        let s = session(3);
        let mut t = Timeline::new();
        t.load_from_session(&s, Vec::new());
        t.rows.set_row_height(RowId(1), 64);
        t.studio_set_row_height(RowHeight::Compact);
        assert_eq!((t.rows.row(RowId(0)).height, t.rows.row(RowId(1)).height), (20, 40));
        t.studio_set_row_height(RowHeight::Regular);
        assert_eq!((t.rows.row(RowId(0)).height, t.rows.row(RowId(1)).height), (32, 64));
        assert_eq!(scaled(10, 32, 20), MIN_ROW_HEIGHT, "never under the floor");
    }

    #[test]
    fn the_command_reaches_an_open_window_on_its_next_sync() {
        let mut s = session(5);
        let mut t = Timeline::new();
        t.load_from_session(&s, Vec::new());
        t.set_viewport(1000, 400, 1000);
        let edits = s.counters.edits.load(Ordering::Relaxed);
        let out = execute(&mut s, &json!({"rowHeight": "compact"})).unwrap();
        assert_eq!(out, json!({"rowHeight": "compact", "heightPt": 20, "changed": true}));
        assert!(s.counters.edits.load(Ordering::Relaxed) > edits, "the window's refresh syncs");
        assert!(t.sync_from_session(&s));
        assert_eq!(t.studio_row_height(), RowHeight::Compact);
        assert!(t.rows.rows.iter().all(|r| r.height == COMPACT_ROW_HEIGHT));
        assert_eq!(execute(&mut s, &json!({})).unwrap()["changed"], false, "a read changes nothing");
        assert_eq!(execute(&mut s, &json!({"rowHeight": "compact"})).unwrap()["changed"], false);

        // A save writing the window's own default doesn't move the rows again.
        t.store_view_state(&mut s);
        t.sync_from_session(&s);
        assert_eq!(t.rows.row_height, COMPACT_ROW_HEIGHT);

        execute(&mut s, &json!({"rowHeight": "regular"})).unwrap();
        t.sync_from_session(&s);
        assert_eq!(t.studio_row_height(), RowHeight::Regular);
        assert!(execute(&mut s, &json!({"rowHeight": "tiny"})).is_err());
    }

    #[test]
    fn saved_row_settings_scale_without_a_window() {
        let mut s = session(1);
        s.document_mut().row_settings =
            vec![("KEY".into(), vixen_io::RowSetting { row_height: 64, expanded: true, visible: true })];
        execute(&mut s, &json!({"rowHeight": "compact"})).unwrap();
        assert_eq!(s.document().row_settings[0].1.row_height, 40);
        assert_eq!(RowHeight::of(0), RowHeight::Regular, "0 in a file means 32");
        assert_eq!(RowHeight::of(25), RowHeight::Compact);
    }
}
