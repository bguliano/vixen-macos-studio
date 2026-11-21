//! Filling a timeline from a session, and following what others do to it. `LoadSequence` and the row and effect parts of
//! `TimedSequenceEditorForm`.

use super::host::describe_effect;
use super::model::{ElementState, Timeline};
use super::rows::Occ;
use super::row_input::TagDisplay;
use super::view::visible_span;
use super::{DragState, ElementId, ElementTimes, IRect, RowId};
use crate::sequence::SequenceSession;
use std::collections::{HashMap, HashSet};
use vixen_core::{MarkCollection, TimeSpan};

/// More rectangles than this and the tiles might as well all repaint.
const DIRTY_RECT_LIMIT: usize = 400;

impl Timeline {
    /// Replaces the marks: on load, and when someone else changed them.
    pub fn load_marks(&mut self, collections: Vec<MarkCollection>) {
        self.marks.set_collections(collections);
        self.snap_index.tpp = self.tpp();
        self.snap_index.rebuild(&self.marks.collections, self.opts.snap_strength);
        self.refresh_marks_layout();
        self.dirty.grid_all = true;
        self.dirty.marks_bar = true;
        self.dirty.ruler = true;
        self.dirty.view = true;
        self.dirty.waveform = true; // STUDIO: the waveform draws mark ticks
    }

    /// `LoadSystemNodesToRows` and the view state the sequence holds. Applied only when above zero; a new sequence holds `TimeSpan.MinValue`.
    pub fn load_from_session(&mut self, session: &SequenceSession, tags: Vec<TagDisplay>) {
        let doc = session.document();
        self.tags = tags;
        self.rows.load_rows(session.tree(), &doc.row_settings, doc.default_row_height);
        self.studio_seen_row_height = doc.default_row_height; // STUDIO: R36, follow later changes to it
        self.info.total = session.length();
        if doc.time_per_pixel > TimeSpan::ZERO {
            self.info.time_per_pixel = doc.time_per_pixel;
        }
        // F100: applied when the end isn't exactly zero, and null counts as not zero.
        if doc.default_playback_end_time != Some(TimeSpan::ZERO) {
            self.info.playback_start = doc.default_playback_start_time;
            self.info.playback_end = doc.default_playback_end_time;
            self.marks.ruler.remembered = (doc.default_playback_start_time, doc.default_playback_end_time);
        }
        self.load_marks(doc.marks.clone());

        // `AddBulkElements`: sorting is delayed until everything is in.
        let mut placed: Vec<(RowId, Occ)> = Vec::with_capacity(session.effect_count());
        for entry in session.effects() {
            let target = entry.targets.first().copied();
            let mut state = ElementState {
                id: entry.id,
                start: entry.node.start,
                duration: entry.node.time_span,
                last_row: None,
                rows: Vec::new(),
                selected: false,
                desc: describe_effect(session, entry.id).unwrap_or_default(),
                target,
                rendered: entry.intents.is_some() && entry.is_rendered(),
                picture: entry.rendered_generation,
                rendered_duration: entry.node.time_span,
            };
            // F41: one effect, in every row of every target it has.
            for node in &entry.targets {
                for r in self.rows.rows_of_node(*node) {
                    if state.rows.contains(r) {
                        continue;
                    }
                    state.rows.push(*r);
                    state.last_row = Some(*r);
                    placed.push((
                        *r,
                        Occ { id: entry.id, start: state.start.ticks(), end: state.end().ticks(), index: 0, count: 0, moving: false },
                    ));
                }
            }
            state.rows.sort_unstable();
            self.elements.insert(entry.id, state);
        }
        let mut rows_touched: Vec<RowId> = Vec::new();
        for (r, occ) in placed {
            let row = self.rows.row_mut(r);
            if row.occurrences.is_empty() {
                rows_touched.push(r);
            }
            row.occurrences.push(occ);
        }
        for r in rows_touched {
            let count = {
                let row = self.rows.row_mut(r);
                row.occurrences.sort_by_key(|o| (o.start, o.end));
                row.rebuild_derived();
                row.occurrences.len() as i64
            };
            let mut at = self.rows.row(r).parent;
            while let Some(p) = at {
                let parent = self.rows.row_mut(p);
                parent.subtree_occurrences = (parent.subtree_occurrences as i64 + count) as u32;
                at = parent.parent;
            }
        }
        self.rows_changed();
        self.mark_everything_dirty();
        self.apply_saved_view(doc.visible_time_start);
        self.document_view = (self.info.time_per_pixel, self.saved_visible_start.unwrap_or(self.info.visible_start));
        self.rows_restored_seen = session.rows_restored().len();
    }

    /// The saved scroll needs the zoom and, once known, the viewport to clamp against.
    fn apply_saved_view(&mut self, visible_start: TimeSpan) {
        self.saved_visible_start = (visible_start > TimeSpan::ZERO).then_some(visible_start);
    }

    /// Applies the saved scroll, once the grid has a size.
    pub(crate) fn apply_pending_view(&mut self) {
        if self.grid_width > 0 {
            if let Some(start) = self.saved_visible_start.take() {
                self.set_visible_start(start);
            }
        }
    }

    /// `SaveGridRowSettings`, and the zoom and scroll upstream copies on every change. Call just before saving.
    pub fn store_view_state(&self, session: &mut SequenceSession) {
        let row_settings = self.rows.save_row_settings();
        let doc = session.document_mut();
        doc.default_row_height = self.rows.row_height;
        doc.default_playback_start_time = self.info.playback_start;
        doc.default_playback_end_time = self.info.playback_end;
        doc.row_settings = row_settings;
        doc.time_per_pixel = self.info.time_per_pixel;
        doc.visible_time_start = self.info.visible_start;
    }

    /// Follows edits made through the session rather than through this timeline: undo and redo, and the other panels. Not during a drag,
    /// when the working times differ on purpose; a later call catches up.
    pub fn sync_from_session(&mut self, session: &SequenceSession) -> bool {
        if self.g.drag_state != DragState::Normal || self.marks.drag_state != DragState::Normal {
            return false;
        }
        let mut seen: HashSet<ElementId> = HashSet::with_capacity(session.effect_count());
        for entry in session.effects() {
            seen.insert(entry.id);
            let target = entry.targets.first().copied();
            let times = ElementTimes::new(entry.node.start, entry.node.time_span);
            match self.elements.get(&entry.id) {
                None => {
                    let desc = describe_effect(session, entry.id).unwrap_or_default();
                    self.add_element_to_targets(entry.id, times, &entry.targets, desc);
                }
                Some(e) => {
                    let (time_changed, target_changed) = (e.times() != times, e.target != target);
                    if time_changed {
                        self.set_element_times(entry.id, times.start, times.duration, true);
                    }
                    if target_changed {
                        self.retarget_element(entry.id, target);
                    }
                    if let (Some(desc), Some(e)) = (describe_effect(session, entry.id), self.elements.get_mut(&entry.id)) {
                        e.desc = desc;
                    }
                }
            }
        }
        let gone: Vec<ElementId> = self.elements.keys().filter(|id| !seen.contains(id)).copied().collect();
        let had_gone = !gone.is_empty();
        for id in gone {
            self.remove_element(id);
        }
        if had_gone {
            self.raise_selection_changed();
        }
        // Effects a clipping move took off their last rows, which an undo has put back. The session knows nothing of rows.
        let restored = session.rows_restored();
        for id in restored.iter().skip(self.rows_restored_seen) {
            if let Some(row) = self.elements.get(id).and_then(|e| e.last_row) {
                self.row_add_element(row, *id);
            }
        }
        self.rows_restored_seen = restored.len();
        self.flush_rows();
        self.refresh_render_state(session);
        self.sync_marks(session);
        self.studio_follow_row_height(session); // STUDIO: R36, sequence_row_height from the menu or an agent
        if self.info.total != session.length() {
            self.set_total(session.length());
        }
        true
    }

    fn sync_marks(&mut self, session: &SequenceSession) {
        let mut theirs: Vec<MarkCollection> = session.marks().to_vec();
        for c in &mut theirs {
            super::marks::ensure_order(c);
        }
        if theirs != self.marks.collections {
            self.load_marks(theirs);
        }
    }

    /// Which effects have a picture to show. F43: an effect whose duration has changed since its render draws a placeholder; a plain move
    /// doesn't dirty it.
    pub fn refresh_render_state(&mut self, session: &SequenceSession) {
        let mut changed: Vec<ElementId> = Vec::new();
        for entry in session.effects() {
            let Some(e) = self.elements.get_mut(&entry.id) else { continue };
            if entry.is_rendered() {
                e.rendered_duration = entry.node.time_span;
            }
            let rendered = entry.intents.is_some() && e.duration == e.rendered_duration;
            let picture = entry.rendered_generation;
            if rendered != e.rendered || picture != e.picture {
                e.rendered = rendered;
                e.picture = picture;
                changed.push(entry.id);
            }
        }
        if changed.len() > DIRTY_RECT_LIMIT {
            self.dirty.grid_all = true;
        } else {
            for id in changed {
                self.dirty_element(id);
            }
        }
        self.dirty.overlay = true;
    }

    fn dirty_element(&mut self, id: ElementId) {
        let Some(e) = self.elements.get(&id) else { return };
        let (start, end) = (e.start.ticks(), e.end().ticks());
        for r in e.rows.clone() {
            self.dirty_time_range_in_row(r, start, end);
        }
    }

    /// Moves an effect to the rows of another node, or to none.
    fn retarget_element(&mut self, id: ElementId, target: Option<vixen_core::NodeId>) {
        let Some(e) = self.elements.get(&id) else { return };
        for r in e.rows.clone() {
            self.row_remove_element(r, id);
        }
        if let Some(e) = self.elements.get_mut(&id) {
            e.target = target;
        }
        if let Some(node) = target {
            for r in self.rows.rows_of_node(node).to_vec() {
                self.row_add_element(r, id);
            }
        }
    }

    /// `RemoveEffectNodeAndElement`'s timeline half.
    pub(crate) fn remove_element(&mut self, id: ElementId) {
        self.set_selected(id, false);
        // GUARD Q33: the hover info can otherwise name an effect that has since been deleted.
        if let Some(at) = self.g.captured.iter().position(|c| *c == id) {
            self.g.captured.remove(at);
            self.dirty.overlay = true;
        }
        let Some(e) = self.elements.get(&id) else { return };
        for r in e.rows.clone() {
            self.row_remove_element(r, id);
        }
        self.elements.remove(&id);
    }

    /// `elements` with their last rows' node ids, for code that wants a document dump.
    pub fn element_rows(&self, id: ElementId) -> Vec<RowId> {
        self.elements.get(&id).map_or_else(Vec::new, |e| e.rows.clone())
    }

    /// F6's visible span, for a width that isn't the grid's.
    pub fn span_for_width(&self, width: i32) -> TimeSpan {
        visible_span(width, self.tpp())
    }

    /// The grid rectangle covering a row, for a caller that wants to scroll to it.
    pub fn row_rect(&self, r: RowId) -> Option<IRect> {
        let top = self.rows.row_top(r)?;
        Some(IRect::new(0, top, self.canvas_width(), self.rows.row(r).height))
    }

    /// What `describe` knows, by effect, for the overlap menu and effect editor.
    pub fn descriptions(&self, ids: &[ElementId]) -> HashMap<ElementId, String> {
        ids.iter().filter_map(|id| self.elements.get(id).map(|e| (*id, e.desc.type_name.to_string()))).collect()
    }
}
