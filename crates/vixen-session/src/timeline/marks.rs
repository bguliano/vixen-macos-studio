//! Marks in the timeline: the marks bar, the selection it shares with the ruler, and everything that edits marks. F117 to F139.

use super::grid_input::{edge_distance, resize_zone, PointerEvent};
use super::host::TimelineHost;
use super::model::{HostRequest, InputOutcome, Timeline};
use super::ruler::RulerState;
use super::snap::MarkLine;
use super::stack::{walk_stacks, StackScratch, StackSlot};
use super::view::{px, time_at};
use super::{Button, CursorKind, DragState, ElementTimes, IRect, Mods, ResizeZone};
use crate::undo::{MarkPlacement, UndoAction};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;
use vixen_core::marks::{Mark, MarkCollectionType};
use vixen_core::{Color, MarkCollection, TimeSpan};

/// `_baseRowHeight`.
const MARK_ROW_HEIGHT: i32 = 20;
/// `MinMarkWidthPx`.
const MIN_MARK_WIDTH_PX: i32 = 12;
/// `DragThreshold`.
const DRAG_THRESHOLD: i32 = 8;
/// `AutoScrollPxScaleFactor`.
const AUTOSCROLL_SCALE: i32 = 4;
/// `Mark.Duration`'s floor.
const MARK_MIN_DURATION: TimeSpan = TimeSpan::from_ms_i(25);
/// F135's order.
pub const PHONEMES: [&str; 10] = ["REST", "AI", "E", "ETC", "FV", "L", "MBP", "O", "U", "WQ"];

/// The `Mark.StartTime` setter: any shorter duration is raised to 25 ms.
pub fn set_mark_start(mark: &mut Mark, t: TimeSpan) {
    if mark.start_time == t {
        return;
    }
    mark.start_time = t;
    if mark.duration < MARK_MIN_DURATION {
        mark.duration = MARK_MIN_DURATION;
    }
}

/// The `Mark.Duration` setter: floor 25 ms.
pub fn set_mark_duration(mark: &mut Mark, d: TimeSpan) {
    mark.duration = d.max(MARK_MIN_DURATION);
}

/// `MarkCollection.EnsureOrder`: start then end. `vixen_core`'s own sort is by start only, which gives different lanes to marks with
/// equal starts.
pub fn ensure_order(collection: &mut MarkCollection) {
    collection.marks.sort_by_key(|m| (m.start_time, m.end_time()));
}

/// `FillGapTimes`. F125.
pub fn fill_gap_times(collection: &mut MarkCollection, id: Uuid) {
    ensure_order(collection);
    let Some(index) = collection.marks.iter().position(|m| m.id == id) else { return };
    let start = collection.marks[index].start_time;
    for i in (0..index).rev() {
        let time = collection.marks[i].end_time();
        if time < start {
            set_mark_start(&mut collection.marks[index], time + TimeSpan::from_ms_i(1));
            break;
        }
    }
    if index != collection.marks.len() - 1 {
        let duration = collection.marks[index + 1].start_time - TimeSpan::from_ms_i(1) - collection.marks[index].start_time;
        set_mark_duration(&mut collection.marks[index], duration);
    }
}

/// `MarkCollectionNameService.GetUniqueName`, for the names it adds to.
pub fn unique_collection_name(desired: &str, collections: &[MarkCollection]) -> String {
    let base = desired.trim().to_string();
    let names: HashSet<String> = collections.iter().map(|c| c.name.trim().to_lowercase()).collect();
    if !names.contains(&base.to_lowercase()) {
        return base;
    }
    let mut n = 2;
    loop {
        let candidate = format!("{base} - {n}");
        if !names.contains(&candidate.to_lowercase()) {
            return candidate;
        }
        n += 1;
    }
}

/// `IdealTextColor`. F119.
pub fn ideal_text_color(bg: Color) -> Color {
    let delta = (bg.r as f64 * 0.299 + bg.g as f64 * 0.587 + bg.b as f64 * 0.114).round_ties_even() as i32;
    if 255 - delta <= 105 {
        Color::rgb(0, 0, 0)
    } else {
        Color::rgb(255, 255, 255)
    }
}

/// The hatch colour on a locked collection's marks. F119.
pub fn locked_hatch_color(bg: Color) -> Color {
    let channel = |v: u8| if v as i32 + 30 <= 255 { v + 30 } else { v - 30 };
    Color { a: bg.a, r: channel(bg.r), g: channel(bg.g), b: channel(bg.b) }
}

/// `MarkCollectionType`'s display name, for F133's warning.
pub fn collection_type_name(t: MarkCollectionType) -> &'static str {
    match t {
        MarkCollectionType::Generic => "Generic",
        MarkCollectionType::Phrase => "Phrase",
        MarkCollectionType::Word => "Word",
        MarkCollectionType::Phoneme => "Phoneme",
        MarkCollectionType::State => "State",
    }
}

/// `ElementMoveType` as marks use it.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum MarkMove {
    Move,
    Resize,
}

/// KEEP Q23: mark undo labels say "effect", as `MarksTimeChangedUndoAction` words them.
pub fn mark_label_undo(kind: MarkMove, count: usize) -> String {
    let s = if count == 1 { "" } else { "s" };
    match kind {
        MarkMove::Move => format!("Moved {count} effect{s}"),
        MarkMove::Resize => format!("Resize {count} effect{s}"),
    }
}

fn marks_added_label(count: usize) -> String {
    format!("Added {count} Mark{}", if count == 1 { "" } else { "s" })
}

fn marks_removed_label(count: usize) -> String {
    format!("Removed {count} Mark{}", if count == 1 { "" } else { "s" })
}

/// `MarkRow`: lanes and height for one collection.
#[derive(Clone, Debug, Default)]
pub(crate) struct MarkRowState {
    /// Parallel to the collection's marks. `GetStackForMark` gives 0 of 1 for any the walk didn't reach.
    slots: Vec<StackSlot>,
    prefix_max_end: Vec<i64>,
    pub stack_count: i32,
    pub height: i32,
    key: Option<(usize, usize)>,
}

/// One clipboard entry. KEEP Q30: the clipboard holds marks by reference, so later edits show up in a paste. `mark` is refreshed from the
/// live one at paste time, and when it's deleted.
#[derive(Clone, Debug)]
pub(crate) struct ClipMark {
    mark: Mark,
    collection: Uuid,
}

/// Marks' times saved when a drag begins: `MarksMoveResizeInfo`.
#[derive(Clone, Debug, Default)]
pub(crate) struct MarkMoveInfo {
    originals: Vec<(Uuid, TimeSpan, TimeSpan)>,
    /// FIX Q29: glued neighbours, in the undo data though not in its label's count.
    neighbours: Vec<(Uuid, TimeSpan, TimeSpan)>,
    earliest: TimeSpan,
    latest: TimeSpan,
    shortest: TimeSpan,
}

/// The marks model and the input of the two surfaces that edit it.
pub struct MarksState {
    pub collections: Vec<MarkCollection>,
    /// In the order selected. `MarksSelectionManager`.
    pub selected: Vec<Uuid>,
    pub ruler: RulerState,
    pub(crate) rows: Vec<MarkRowState>,
    index: HashMap<Uuid, (u32, u32)>,
    /// `Height`: the sum of shown rows. Changes as you scroll into overlapping marks. KEEP Q24.
    pub bar_height: i32,
    pub(crate) drag_state: DragState,
    down_mark: Option<Uuid>,
    down_location: (i32, i32),
    last_single: (i32, i32),
    ignore_area: IRect,
    zone: ResizeZone,
    move_start: (i32, i32),
    pub(crate) move_info: Option<MarkMoveInfo>,
    last_move: Option<PointerEvent>,
    outside_x: i32,
    pub(crate) autoscroll: bool,
    clipboard: Vec<ClipMark>,
    /// Marks whose lines are lifted out of the grid's tiles while they move.
    moving: HashSet<Uuid>,
    scratch: StackScratch,
    menu: MarksMenuModel,
}

impl Default for MarksState {
    fn default() -> Self {
        MarksState {
            collections: Vec::new(),
            selected: Vec::new(),
            ruler: RulerState::default(),
            rows: Vec::new(),
            index: HashMap::new(),
            bar_height: 0,
            drag_state: DragState::Normal,
            down_mark: None,
            down_location: (0, 0),
            last_single: (0, 0),
            ignore_area: IRect::default(),
            zone: ResizeZone::None,
            move_start: (0, 0),
            move_info: None,
            last_move: None,
            outside_x: 0,
            autoscroll: false,
            clipboard: Vec::new(),
            moving: HashSet::new(),
            scratch: StackScratch::default(),
            menu: MarksMenuModel::default(),
        }
    }
}

impl MarksState {
    #[inline]
    pub fn is_selected(&self, id: Uuid) -> bool {
        self.selected.contains(&id)
    }

    pub fn mark(&self, id: Uuid) -> Option<&Mark> {
        let (c, m) = *self.index.get(&id)?;
        self.collections.get(c as usize)?.marks.get(m as usize)
    }

    fn mark_mut(&mut self, id: Uuid) -> Option<&mut Mark> {
        let (c, m) = *self.index.get(&id)?;
        self.collections.get_mut(c as usize)?.marks.get_mut(m as usize)
    }

    #[cfg(test)]
    pub(crate) fn mark_mut_for_tests(&mut self, id: Uuid) -> Option<&mut Mark> {
        self.mark_mut(id)
    }

    pub fn collection_of(&self, id: Uuid) -> Option<&MarkCollection> {
        let (c, _) = *self.index.get(&id)?;
        self.collections.get(c as usize)
    }

    fn collection_index_of(&self, id: Uuid) -> Option<usize> {
        self.index.get(&id).map(|(c, _)| *c as usize)
    }

    pub(crate) fn moving_ids_empty(&self) -> bool {
        self.moving.is_empty()
    }

    pub(crate) fn is_moving(&self, id: Uuid) -> bool { self.moving.contains(&id) } // STUDIO: for R35's quieter mark lines

    /// `MarksMoveResizeInfo`: the selected marks as they are now.
    pub(crate) fn save_selected_times(&mut self) {
        let mut info = MarkMoveInfo { earliest: TimeSpan::MAX, latest: TimeSpan::MIN, shortest: TimeSpan::MAX, ..MarkMoveInfo::default() };
        for id in &self.selected {
            if let Some(m) = self.mark(*id) {
                info.originals.push((*id, m.start_time, m.duration));
                info.earliest = info.earliest.min(m.start_time);
                info.latest = info.latest.max(m.end_time());
                info.shortest = info.shortest.min(m.duration);
            }
        }
        self.move_info = Some(info);
    }

    /// Replaces the marks wholesale: on load, and when someone else changed them. The selection stays as it is: an undone add leaves the mark
    /// selected, and it shows as selected again if a redo brings it back (F152).
    pub(crate) fn set_collections(&mut self, mut collections: Vec<MarkCollection>) {
        for c in &mut collections {
            ensure_order(c);
        }
        self.collections = collections;
        self.rebuild_all();
    }

    fn rebuild_all(&mut self) {
        self.index.clear();
        self.rows.clear();
        self.rows.resize(self.collections.len(), MarkRowState::default());
        for c in 0..self.collections.len() {
            self.rebuild_collection(c);
        }
    }

    /// After a collection's marks moved: its id map, running max end and lanes.
    fn rebuild_collection(&mut self, c: usize) {
        let collection = &self.collections[c];
        let row = &mut self.rows[c];
        row.slots.clear();
        row.slots.resize(collection.marks.len(), StackSlot { index: 0, count: 1 });
        row.prefix_max_end.clear();
        let mut max_end = i64::MIN;
        for (i, m) in collection.marks.iter().enumerate() {
            max_end = max_end.max(m.end_time().ticks());
            row.prefix_max_end.push(max_end);
            self.index.insert(m.id, (c as u32, i as u32));
        }
        row.key = None;
    }

    /// `SetStackIndexes` for every shown row, ahead of paint (FIX Q25). Returns whether anything would draw differently.
    fn refresh_layout(&mut self, start: i64, end: i64) -> bool {
        let mut changed = false;
        let mut bar = 0;
        for c in 0..self.collections.len() {
            let collection = &self.collections[c];
            let row = &mut self.rows[c];
            if !collection.show_marks {
                continue;
            }
            let m0 = row.prefix_max_end.partition_point(|&m| m < start);
            let stop = collection.marks.partition_point(|m| m.start_time.ticks() <= end);
            if row.key != Some((m0, stop)) {
                row.key = Some((m0, stop));
                let marks = &collection.marks;
                let mut new = vec![StackSlot::default(); marks.len()];
                let mut max_stack = 1;
                walk_stacks(
                    marks.len(),
                    |i| marks[i].start_time.ticks(),
                    |i| marks[i].end_time().ticks(),
                    Some(&row.prefix_max_end),
                    start,
                    end,
                    &mut self.scratch,
                    |i, slot| {
                        // KEEP Q26: a mark reached by two clusters keeps its first values, because upstream's update changes a struct copy.
                        if new[i].count == 0 {
                            new[i] = slot;
                        }
                        max_stack = max_stack.max(slot.count);
                    },
                );
                for slot in new.iter_mut().filter(|s| s.count == 0) {
                    *slot = StackSlot { index: 0, count: 1 };
                }
                if new != row.slots || row.stack_count != max_stack {
                    changed = true;
                }
                row.slots = new;
                row.stack_count = max_stack;
                row.height = MARK_ROW_HEIGHT * max_stack;
            }
            bar += row.height;
        }
        if bar != self.bar_height {
            self.bar_height = bar;
            changed = true;
        }
        changed
    }
}

/// A mark as a tile paints it. F119.
#[derive(Clone, Debug, PartialEq)]
pub struct MarkQuad {
    pub id: Uuid,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub color: Color,
    pub hatch: Option<Color>,
    pub selected: bool,
    pub text: String,
    pub text_color: Color,
}

/// F117 to F119 for a span of the bar's canvas.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MarksBarFrame {
    pub total_height: i32,
    /// Where each row's separator goes.
    pub row_bottoms: Vec<i32>,
    pub marks: Vec<MarkQuad>,
}

/// F130 to F137's enabling and visibility.
#[derive(Copy, Clone, Default, PartialEq, Eq, Debug)]
pub struct MarksMenuModel {
    pub edit_text: bool,
    pub cut: bool,
    pub copy: bool,
    pub paste: bool,
    pub delete: bool,
    pub phoneme: bool,
    pub breakdown_phrase: bool,
    pub breakdown_word: bool,
}

/// F133's question.
#[derive(Clone, Debug, PartialEq)]
pub struct MismatchInfo {
    pub text: String,
}

/// F111.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum AddMarkResult {
    Added(Uuid),
    /// A mark already has exactly that start, or no collection could be found.
    Nothing,
    /// Ask "Marks are stored in Mark Collections...", then `create_default_mark_collection`.
    NeedsNewCollection,
    /// Ask "The active mark collection is not visible...", then `enable_default_mark_collection`.
    NeedsEnableDefault,
}

impl Timeline {
    // ------------------------------------------------------------------------------------------------------------------
    // Layout and frames
    // ------------------------------------------------------------------------------------------------------------------

    /// Bars' layout pass, run with the other view work.
    pub(crate) fn refresh_marks_layout(&mut self) {
        let (start, end) = (self.info.visible_start.ticks(), self.vis_end().ticks());
        let before = self.marks.bar_height;
        if self.marks.refresh_layout(start, end) {
            self.dirty.marks_bar = true;
            if self.marks.bar_height != before {
                self.dirty.view = true;
            }
        }
    }

    /// Rows top to bottom with their tops: `(collection index, top)`.
    fn mark_row_tops(&self) -> Vec<(usize, i32)> {
        let mut out = Vec::new();
        let mut top = 0;
        for (c, collection) in self.marks.collections.iter().enumerate() {
            if collection.show_marks {
                out.push((c, top));
                top += self.marks.rows[c].height;
            }
        }
        out
    }

    /// `marks_bar_frame` for a span of columns. Height is worked out in the layout pass, not here.
    pub fn marks_bar_frame(&mut self, x: i32, width: i32) -> MarksBarFrame {
        self.marks_bar_frame_at(x, width, self.tpp())
    }

    /// The same at a given zoom. See `grid_frame_at`.
    pub fn marks_bar_frame_at(&mut self, x: i32, width: i32, tpp: TimeSpan) -> MarksBarFrame {
        self.refresh_marks_layout();
        let (t0, t1) = (time_at(x - 2, tpp).ticks(), time_at(x + width + 2, tpp).ticks());
        let mut frame = MarksBarFrame { total_height: self.marks.bar_height, ..MarksBarFrame::default() };
        for (c, top) in self.mark_row_tops() {
            let collection = &self.marks.collections[c];
            let row = &self.marks.rows[c];
            frame.row_bottoms.push(top + row.height);
            let from = row.prefix_max_end.partition_point(|&m| m < t0);
            for (i, m) in collection.marks.iter().enumerate().skip(from) {
                if m.start_time.ticks() > t1 {
                    break;
                }
                let w = px(m.duration, tpp) as i32;
                if w <= 0 {
                    continue;
                }
                let slot = row.slots.get(i).copied().unwrap_or(StackSlot { index: 0, count: 1 });
                let mut h = if slot.count > 1 { (row.height - 2) / row.stack_count.max(1) } else { row.height - 2 };
                let y = top + h * slot.index;
                if h == 0 {
                    h = row.height;
                }
                let color = collection.decorator.color;
                frame.marks.push(MarkQuad {
                    id: m.id,
                    x: px(m.start_time, tpp).floor() as i32,
                    y,
                    w,
                    h,
                    color,
                    hatch: collection.is_locked.then(|| locked_hatch_color(color)),
                    selected: self.marks.is_selected(m.id),
                    text: m.text.clone(),
                    text_color: ideal_text_color(color),
                });
            }
        }
        frame
    }

    /// F120.
    pub fn mark_at(&self, p: (i32, i32)) -> Option<Uuid> {
        let rows = self.mark_row_tops();
        let (c, top) = rows.iter().copied().find(|(c, top)| p.1 < top + self.marks.rows[*c].height)?;
        let collection = &self.marks.collections[c];
        let row = &self.marks.rows[c];
        let tpp = self.tpp();
        let t = self.time(p.0);
        let from = row.prefix_max_end.partition_point(|&m| m < t.ticks());
        for (i, m) in collection.marks.iter().enumerate().skip(from) {
            if t > m.end_time() {
                continue;
            }
            if t < m.start_time {
                break;
            }
            let x = px(m.start_time, tpp);
            let w = px(m.duration, tpp);
            let slot = row.slots.get(i).copied().unwrap_or(StackSlot { index: 0, count: 1 });
            // KEEP: hit height differs from draw height by the 2.
            let h = if slot.count == 1 { row.height } else { row.height / row.stack_count.max(1) };
            let lane_top = top + h * slot.index;
            if p.0 as f64 >= x && p.0 as f64 <= x + w && p.1 >= lane_top && p.1 < lane_top + h {
                return Some(m.id);
            }
        }
        None
    }

    // ------------------------------------------------------------------------------------------------------------------
    // Selection
    // ------------------------------------------------------------------------------------------------------------------

    pub(crate) fn marks_clear_selected(&mut self) {
        if !self.marks.selected.is_empty() {
            self.marks.selected.clear();
            self.marks_selection_changed();
        }
    }

    pub(crate) fn marks_select(&mut self, id: Uuid) {
        if !self.marks.is_selected(id) {
            self.marks.selected.push(id);
            self.marks_selection_changed();
        }
    }

    pub(crate) fn marks_deselect(&mut self, id: Uuid) {
        self.marks.selected.retain(|s| *s != id);
        self.marks_selection_changed();
    }

    fn marks_selection_changed(&mut self) {
        self.dirty.ruler = true;
        self.dirty.marks_bar = true;
        self.dirty.waveform = true; // STUDIO: the waveform draws mark ticks
        self.studio_refresh_lit_collections(); // STUDIO: a selected mark's collection draws its lines across the grid
        self.marks_selection_notify = true;
    }

    /// `SelectMarksBetween`. F123. KEEP Q27: this can select marks in locked collections.
    fn select_marks_between(&mut self, a: (i32, i32), b: (i32, i32)) {
        self.marks.selected.clear();
        let rect = IRect::new(a.0.min(b.0), a.1.min(b.1), (a.0 - b.0).abs(), (a.1 - b.1).abs());
        let rows = self.mark_row_tops();
        let row_at = |y: i32| rows.iter().position(|(c, top)| y < top + self.marks.rows[*c].height);
        let Some(first) = row_at(rect.y) else {
            self.marks_selection_changed();
            return;
        };
        let last = row_at(rect.bottom());
        let (sel_start, sel_end) = (self.time(rect.x), self.time(rect.right()));
        let mut chosen = Vec::new();
        for (i, (c, _)) in rows.iter().enumerate() {
            if i < first {
                continue;
            }
            for m in &self.marks.collections[*c].marks {
                if m.start_time < sel_end && m.end_time() > sel_start {
                    chosen.push(m.id);
                }
            }
            if Some(i) == last {
                break;
            }
        }
        self.marks.selected = chosen;
        self.marks_selection_changed();
    }

    // ------------------------------------------------------------------------------------------------------------------
    // Input
    // ------------------------------------------------------------------------------------------------------------------

    #[inline]
    fn marks_canvas(&self, ev: PointerEvent) -> (i32, i32) {
        (ev.x + self.scroll_x(), ev.y)
    }

    fn marks_update_zone(&mut self, p: (i32, i32)) {
        let zone = match self.mark_at(p).and_then(|id| self.marks.mark(id)) {
            // F139: hover doesn't check for a lock, so the zone and pointer still appear on locked marks.
            Some(m) => resize_zone(ElementTimes::new(m.start_time, m.duration), self.tpp(), p.0),
            None => ResizeZone::None,
        };
        self.marks.zone = zone;
    }

    /// `MarksBar.OnMouseDown`. F121 to F125.
    pub fn marks_mouse_down(&mut self, ev: PointerEvent, host: &mut dyn TimelineHost) -> InputOutcome {
        self.refresh_marks_layout();
        let p = self.marks_canvas(ev);
        self.marks.down_location = p;
        self.marks.down_mark = self.mark_at(p).filter(|id| !self.marks.collection_of(*id).is_some_and(|c| c.is_locked));
        if self.marks.drag_state == DragState::Normal {
            self.marks_update_zone(p);
        }
        let down = self.marks.down_mark;
        let mods = ev.mods;
        if matches!(ev.button, Some(Button::Left | Button::Right)) {
            if ev.clicks == 2 {
                if let Some(id) = down {
                    if mods.opt {
                        self.marks_fill_gap(id, host);
                    } else if let Some(m) = self.marks.mark(id) {
                        // `PlaySequenceBetween`: what's outside the sequence is replaced by the range, or by the ends.
                        let (start, end) = (m.start_time, m.end_time());
                        let start = if start < TimeSpan::ZERO { self.info.playback_start.unwrap_or(TimeSpan::ZERO) } else { start };
                        let end = if end > self.info.total { self.info.playback_end } else { Some(end) };
                        self.requests.push(HostRequest::PlayRange { start, end });
                    }
                    return self.finish_marks();
                }
            }
            if !mods.cmd && !mods.shift {
                if !down.is_some_and(|id| self.marks.is_selected(id)) {
                    self.marks.last_single = p;
                    self.marks_clear_selected();
                    if let Some(id) = down {
                        self.marks_select(id);
                    }
                }
            } else if mods.shift {
                if self.marks.last_single != (0, 0) {
                    self.select_marks_between(self.marks.last_single, p);
                }
            } else if let Some(id) = down {
                if self.marks.is_selected(id) {
                    self.marks_deselect(id);
                } else {
                    self.marks.last_single = p;
                    self.marks_select(id);
                }
            } else {
                self.marks.last_single = p;
            }
            if ev.button == Some(Button::Left) {
                if self.marks.zone == ResizeZone::None {
                    // Begin waiting for a normal drag.
                    self.marks.drag_state = DragState::Waiting;
                    self.marks.ignore_area = IRect::new(p.0 - DRAG_THRESHOLD, p.1 - DRAG_THRESHOLD, DRAG_THRESHOLD * 2, DRAG_THRESHOLD * 2);
                } else if !mods.cmd && self.marks.drag_state == DragState::Normal {
                    self.marks.drag_state = DragState::HResizing;
                    self.begin_move_resize_marks(p);
                }
            }
        }
        self.finish_marks()
    }

    /// `MarksBar.OnMouseMove`, and `UpdateAutoScrollState`. Right button never drags.
    pub fn marks_mouse_move(&mut self, ev: PointerEvent) -> InputOutcome {
        if ev.button == Some(Button::Right) {
            return self.finish_marks();
        }
        self.refresh_marks_layout();
        self.marks_handle_move(ev);
        if matches!(self.marks.drag_state, DragState::Moving | DragState::HResizing) {
            self.marks.last_move = Some(ev);
            self.marks.outside_x = autoscroll_distance(ev.x, self.panel_width);
            self.marks.autoscroll = self.marks.outside_x != 0;
        } else {
            self.marks_stop_autoscroll();
        }
        self.finish_marks()
    }

    fn marks_stop_autoscroll(&mut self) {
        self.marks.autoscroll = false;
        self.marks.last_move = None;
        self.marks.outside_x = 0;
    }

    fn marks_handle_move(&mut self, ev: PointerEvent) {
        let p = self.marks_canvas(ev);
        match self.marks.drag_state {
            DragState::Normal => self.marks_update_zone(p),
            DragState::Waiting => {
                if !self.marks.ignore_area.contains(p) && self.marks.down_mark.is_some() {
                    self.marks.drag_state = DragState::Moving;
                    self.marks.ignore_area = IRect::default();
                    self.begin_move_resize_marks(p);
                }
            }
            DragState::Moving => self.marks_drag_moving(p),
            DragState::HResizing => self.marks_h_resizing(p, ev.mods),
            DragState::Selecting | DragState::Drawing => {}
        }
    }

    /// `BeginMoveResizeMarks`.
    fn begin_move_resize_marks(&mut self, p: (i32, i32)) {
        self.marks.move_start = p;
        self.marks.save_selected_times();
    }

    /// `MouseMove_DragMoving`. F126: along their rows, no snapping, y ignored.
    fn marks_drag_moving(&mut self, p: (i32, i32)) {
        if self.marks.selected.is_empty() {
            return;
        }
        let Some(down) = self.marks.down_mark else { return };
        let Some(info) = self.marks.move_info.as_ref() else { return };
        let mut dt = self.time(p.0 - self.marks.move_start.0);
        if dt == TimeSpan::ZERO {
            return;
        }
        if info.earliest + dt < TimeSpan::ZERO {
            dt = TimeSpan::ZERO - info.earliest;
        }
        if info.latest + dt > self.info.total {
            dt = self.info.total - info.latest;
        }
        let originals = info.originals.clone();
        for (id, start, _) in &originals {
            if let Some(m) = self.marks.mark_mut(*id) {
                set_mark_start(m, *start + dt);
            }
        }
        let selected = self.marks.selected.clone();
        self.marks_moving(&selected);
        if let Some(m) = self.marks.mark(down) {
            let times = vec![m.start_time, m.end_time()];
            self.set_guides(times);
        }
    }

    /// `MouseMove_HResizing`. F127, F128.
    fn marks_h_resizing(&mut self, p: (i32, i32), mods: Mods) {
        let Some(down) = self.marks.down_mark else { return };
        let Some(info) = self.marks.move_info.clone() else { return };
        if info.originals.is_empty() {
            return;
        }
        let mut dt = self.time(p.0 - self.marks.move_start.0);
        if dt == TimeSpan::ZERO {
            return;
        }
        let tpp = self.tpp();
        let min_width = self.time(MIN_MARK_WIDTH_PX);
        let mut shortest = info.shortest;
        let glued = mods.opt && self.marks.selected.len() == 1;
        let zone = self.marks.zone;
        let mut neighbour = None;
        if glued {
            if let Some((c, m)) = self.marks.index.get(&down).map(|(c, m)| (*c as usize, *m as usize)) {
                let marks = &self.marks.collections[c].marks;
                let other = match zone {
                    ResizeZone::Front if m > 0 => marks.get(m - 1),
                    ResizeZone::Back => marks.get(m + 1),
                    _ => None,
                };
                if let Some(other) = other {
                    neighbour = Some(other.id);
                    shortest = shortest.min(other.duration);
                    let saved = (other.id, other.start_time, other.duration);
                    if let Some(info) = self.marks.move_info.as_mut() {
                        if !info.neighbours.iter().any(|n| n.0 == saved.0) && !info.originals.iter().any(|o| o.0 == saved.0) {
                            info.neighbours.push(saved);
                        }
                    }
                }
            }
        }
        match zone {
            ResizeZone::Front => {
                if info.earliest + dt < TimeSpan::ZERO {
                    dt = TimeSpan::ZERO - info.earliest;
                }
                if px(shortest - dt, tpp) < MIN_MARK_WIDTH_PX as f64 {
                    dt = shortest - min_width;
                }
            }
            ResizeZone::Back => {
                if info.latest + dt > self.info.total {
                    dt = self.info.total - info.latest;
                }
                if px(shortest + dt, tpp) < MIN_MARK_WIDTH_PX as f64 {
                    dt = min_width - shortest;
                }
            }
            ResizeZone::None => {}
        }
        if let Some(m) = self.marks.mark(down) {
            let guide = if zone == ResizeZone::Front { m.start_time } else { m.end_time() };
            self.set_guides(vec![guide]);
        }
        let mut moved: Vec<Uuid> = Vec::with_capacity(info.originals.len() + 1);
        for (id, start, duration) in &info.originals {
            if let Some(m) = self.marks.mark_mut(*id) {
                match zone {
                    ResizeZone::Front => {
                        set_mark_start(m, *start + dt);
                        set_mark_duration(m, *duration - dt);
                    }
                    ResizeZone::Back => set_mark_duration(m, *duration + dt),
                    ResizeZone::None => {}
                }
            }
            moved.push(*id);
        }
        if let Some(other) = neighbour {
            let primary = self.marks.mark(down).map(|m| (m.start_time, m.end_time()));
            if let (Some((start, end)), Some(n)) = (primary, self.marks.mark_mut(other)) {
                // KEEP Q28: the minimum-width check above runs in the wrong direction for the neighbour, so it can collapse to the floor.
                match zone {
                    ResizeZone::Front => {
                        let d = start - n.start_time;
                        set_mark_duration(n, d);
                    }
                    ResizeZone::Back => {
                        let n_end = n.end_time();
                        set_mark_start(n, end);
                        set_mark_duration(n, n_end - n.start_time);
                    }
                    ResizeZone::None => {}
                }
            }
            moved.push(other);
        }
        self.marks_moving(&moved);
    }

    /// `AutoScrollTimer_Tick`. F129.
    pub fn marks_autoscroll_tick(&mut self, mods: Mods) -> InputOutcome {
        let moving = matches!(self.marks.drag_state, DragState::Moving | DragState::HResizing);
        let Some(mut ev) = self.marks.last_move.filter(|_| moving && self.marks.outside_x != 0) else {
            self.marks_stop_autoscroll();
            return self.finish_marks();
        };
        let start = autoscroll_visible_start(self.info.visible_start, self.marks.outside_x, self.tpp(), self.info.total, self.span());
        self.set_visible_start(start);
        self.refresh_marks_layout();
        ev.mods = mods;
        self.marks_handle_move(ev);
        self.finish_marks()
    }

    /// `MarksBar.OnMouseUp`. F121, F126, F127, F130.
    pub fn marks_mouse_up(&mut self, ev: PointerEvent, host: &mut dyn TimelineHost) -> InputOutcome {
        self.refresh_marks_layout();
        let p = self.marks_canvas(ev);
        if ev.button == Some(Button::Right) {
            self.marks.menu = self.marks_menu_model(p);
            self.requests.push(HostRequest::MarksContextMenu { press: self.marks.down_location, release: p });
        }
        match self.marks.drag_state {
            DragState::Moving => {
                self.marks.last_single = p;
                self.set_guides(Vec::new());
                self.marks_moved(MarkMove::Move, host);
                self.marks_end_all_drag();
            }
            DragState::HResizing => {
                self.set_guides(Vec::new());
                self.marks_moved(MarkMove::Resize, host);
                self.marks_end_all_drag();
            }
            _ => {
                // A click on one of several selected marks collapses to it.
                if !ev.mods.cmd && !ev.mods.shift && ev.button != Some(Button::Right) && self.marks.selected.len() > 1 {
                    self.marks.last_single = p;
                    self.marks_clear_selected();
                    if let Some(id) = self.marks.down_mark {
                        self.marks_select(id);
                    }
                }
                self.marks_end_all_drag();
            }
        }
        self.finish_marks()
    }

    /// `EndAllDrag`.
    fn marks_end_all_drag(&mut self) {
        self.marks_stop_autoscroll();
        self.marks.drag_state = DragState::Normal;
        self.marks.move_info = None;
        self.marks.down_mark = None;
    }

    /// F130 to F137's enabling, for the point released at. The press has already gone through `marks_mouse_down`.
    fn marks_menu_model(&self, release: (i32, i32)) -> MarksMenuModel {
        let same_point = self.marks.down_location == release;
        let on_mark = same_point && self.marks.down_mark.is_some();
        let any = !self.marks.selected.is_empty();
        let all = |t: MarkCollectionType| self.marks.selected.iter().all(|id| self.marks.collection_of(*id).is_some_and(|c| c.collection_type == t));
        MarksMenuModel {
            edit_text: on_mark,
            cut: any,
            copy: any,
            paste: same_point && !self.marks.clipboard.is_empty(),
            delete: any,
            phoneme: on_mark && all(MarkCollectionType::Phoneme),
            breakdown_phrase: on_mark && all(MarkCollectionType::Phrase),
            breakdown_word: on_mark && all(MarkCollectionType::Word),
        }
    }

    /// The menu the last right release asked for.
    pub fn marks_context_menu(&self) -> MarksMenuModel {
        self.marks.menu
    }

    fn finish_marks(&mut self) -> InputOutcome {
        let mut outcome = self.finish();
        outcome.cursor = match self.marks.drag_state {
            DragState::Moving => CursorKind::Hand,
            _ if self.marks.zone != ResizeZone::None => CursorKind::ResizeLeftRight,
            _ => CursorKind::Arrow,
        };
        outcome
    }

    // ------------------------------------------------------------------------------------------------------------------
    // Edits
    // ------------------------------------------------------------------------------------------------------------------

    /// `OnMarksMoving`: re-sorts the collections concerned and replaces those marks' snap details by identity (F75). Their grid lines go
    /// to a layer so the drag repaints no tile.
    pub(crate) fn marks_moving(&mut self, ids: &[Uuid]) {
        for id in ids {
            if self.marks.moving.insert(*id) {
                if let Some(start) = self.marks.mark(*id).map(|m| m.start_time) {
                    self.dirty_mark_column(start);
                }
            }
        }
        let mut collections: Vec<usize> = ids.iter().filter_map(|id| self.marks.collection_index_of(*id)).collect();
        collections.sort_unstable();
        collections.dedup();
        for c in collections {
            ensure_order(&mut self.marks.collections[c]);
            self.marks.rebuild_collection(c);
        }
        self.snap_index.update_marks(ids, &self.marks.collections);
        self.refresh_marks_layout();
        self.dirty.ruler = true;
        self.dirty.marks_bar = true;
        self.dirty.overlay = true;
        self.dirty.waveform = true; // STUDIO: the waveform draws mark ticks
    }

    fn dirty_mark_column(&mut self, t: TimeSpan) {
        let x = px(t, self.tpp()) as i32;
        self.dirty.grid_rects.push(IRect::new(x - 3, i32::MIN / 2, 7, i32::MAX));
    }

    /// Marks stop being lifted: their lines go back into the tiles where they now are.
    pub(crate) fn marks_lift_end(&mut self) {
        let moving: Vec<Uuid> = self.marks.moving.drain().collect();
        for id in moving {
            if let Some((start, end)) = self.marks.mark(id).map(|m| (m.start_time, m.end_time())) {
                self.dirty_mark_column(start);
                self.dirty_mark_column(end);
            }
        }
    }

    /// `OnMarkMoved`, as the form answers it: an undo entry, snap points rebuilt, dependents rendered, sequence modified.
    pub(crate) fn marks_moved(&mut self, kind: MarkMove, host: &mut dyn TimelineHost) {
        self.marks_lift_end();
        let info = self.marks.move_info.take().unwrap_or_default();
        self.commit_marks(host);
        let label = mark_label_undo(kind, info.originals.len());
        let marks = info.originals.iter().chain(info.neighbours.iter()).map(|(mark, start, duration)| MarkPlacement { mark: *mark, start: *start, duration: *duration }).collect();
        host.record_undo(UndoAction::MarkTimesChanged { label, marks });
        self.requests.push(HostRequest::UndoRecorded);
    }

    fn commit_marks(&mut self, host: &mut dyn TimelineHost) {
        host.commit_marks(&self.marks.collections);
        self.snap_index.rebuild(&self.marks.collections, self.opts.snap_strength);
        self.refresh_marks_layout();
        self.dirty.grid_all = true;
        self.dirty.ruler = true;
        self.dirty.marks_bar = true;
        self.dirty.overlay = true;
        self.dirty.waveform = true; // STUDIO: the waveform draws mark ticks
    }

    /// Puts every collection in the bar, for tests and the benchmark.
    pub fn set_all_mark_bars_visible(&mut self, visible: bool) {
        for c in &mut self.marks.collections {
            c.show_marks = visible;
        }
        self.marks.rebuild_all();
        self.refresh_marks_layout();
        self.dirty.marks_bar = true;
        self.dirty.ruler = true;
        self.dirty.view = true;
    }

    /// `x.StartTime += diff` for each. No clamp. KEEP Q22.
    pub(crate) fn marks_offset_starts(&mut self, ids: &[Uuid], diff: TimeSpan) {
        for id in ids {
            if let Some(m) = self.marks.mark_mut(*id) {
                let t = m.start_time + diff;
                set_mark_start(m, t);
            }
        }
    }

    /// F125: `FillGapTimes` on the clicked mark, then marks moved with type Resize, for the selected marks.
    fn marks_fill_gap(&mut self, id: Uuid, host: &mut dyn TimelineHost) {
        self.marks.save_selected_times();
        if let Some(c) = self.marks.collection_index_of(id) {
            fill_gap_times(&mut self.marks.collections[c], id);
            self.marks.rebuild_collection(c);
        }
        self.marks_moved(MarkMove::Resize, host);
    }

    /// F130, F135. KEEP: no undo.
    pub fn set_selected_marks_text(&mut self, text: &str, host: &mut dyn TimelineHost) {
        for id in self.marks.selected.clone() {
            if let Some(m) = self.marks.mark_mut(id) {
                m.text = text.to_string();
            }
        }
        host.commit_marks(&self.marks.collections);
        self.dirty.marks_bar = true;
    }

    /// F130's dialog: its title and initial text, selected, only for one mark.
    pub fn edit_text_prompt(&self) -> (String, String) {
        if self.marks.selected.len() == 1 {
            let text = self.marks.mark(self.marks.selected[0]).map_or(String::new(), |m| m.text.clone());
            ("Edit Mark".to_string(), text)
        } else {
            ("Edit Multiple Marks".to_string(), String::new())
        }
    }

    /// F132. KEEP Q30: references, not copies. Private to the bar.
    pub fn copy_marks(&mut self) {
        self.marks.clipboard = self
            .marks
            .selected
            .iter()
            .filter_map(|id| {
                let mark = self.marks.mark(*id)?.clone();
                let collection = self.marks.collection_of(*id)?.id;
                Some(ClipMark { mark, collection })
            })
            .collect();
    }

    /// F131.
    pub fn cut_marks(&mut self, host: &mut dyn TimelineHost) {
        self.copy_marks();
        self.delete_selected_marks(host);
    }

    /// F134's menu item, which is only enabled with a selection.
    pub fn delete_selected_marks(&mut self, host: &mut dyn TimelineHost) {
        if !self.marks.selected.is_empty() {
            self.delete_marks_key(host);
        }
    }

    /// The Delete key on the bar, and the ruler's menu (F112, F134). KEEP Q35: neither is gated on a selection, so with nothing selected this
    /// records "Removed 0 Marks". FIX Q31: upstream's bar leaves the deleted marks selected, so a second Delete records another undo entry holding
    /// the same marks, and undoing both duplicates them. The selection clears, as the ruler's does, so that entry holds nothing.
    pub fn delete_marks_key(&mut self, host: &mut dyn TimelineHost) {
        let doomed: HashSet<Uuid> = self.marks.selected.iter().copied().collect();
        // A cut's marks stay on the clipboard with what they last held.
        let mut removed: Vec<(Uuid, Mark)> = Vec::with_capacity(doomed.len());
        for c in &self.marks.collections {
            for m in c.marks.iter().filter(|m| doomed.contains(&m.id)) {
                removed.push((c.id, m.clone()));
            }
        }
        for clip in &mut self.marks.clipboard {
            if let Some((_, m)) = removed.iter().find(|(_, m)| m.id == clip.mark.id) {
                clip.mark = m.clone();
            }
        }
        for c in &mut self.marks.collections {
            c.marks.retain(|m| !doomed.contains(&m.id));
        }
        self.marks.rebuild_all();
        self.marks.selected.clear();
        self.marks_selection_changed();
        self.commit_marks(host);
        host.record_undo(UndoAction::MarksRemoved { label: marks_removed_label(removed.len()), marks: removed });
        self.requests.push(HostRequest::UndoRecorded);
    }

    /// F133: groups by original collection, ordered by collection index, and offsets rows from the row under the press. `proceed` answers the
    /// type mismatch question; No stops the remaining groups. KEEP: no clamp to the sequence.
    pub fn paste_marks(&mut self, press: (i32, i32), proceed: &mut dyn FnMut(&MismatchInfo) -> bool, host: &mut dyn TimelineHost) {
        // The row under the press, counted among all rows, shown or not.
        let rows = self.mark_row_tops();
        let Some(base_row) = rows.iter().find(|(c, top)| press.1 < top + self.marks.rows[*c].height).map(|(c, _)| *c) else { return };
        let base_time = self.time(press.0);
        let mut groups: Vec<(usize, Vec<Mark>)> = Vec::new();
        for clip in &self.marks.clipboard {
            let Some(c) = self.marks.collections.iter().position(|col| col.id == clip.collection) else { continue };
            // Later edits show up in a paste.
            let mark = self.marks.mark(clip.mark.id).cloned().unwrap_or_else(|| clip.mark.clone());
            match groups.iter_mut().find(|(g, _)| *g == c) {
                Some((_, list)) => list.push(mark),
                None => groups.push((c, vec![mark])),
            }
        }
        groups.sort_by_key(|(c, _)| *c);
        let Some(offset) = groups.first().map(|(c, _)| *c) else { return };
        let earliest = groups.iter().flat_map(|(_, list)| list.iter().map(|m| m.start_time)).min().unwrap_or(TimeSpan::ZERO);
        let mut pasted = Vec::new();
        let mut added: Vec<(Uuid, Mark)> = Vec::new();
        for (source, list) in groups {
            let target = base_row + source - offset;
            if target > self.marks.collections.len() - 1 {
                continue;
            }
            let (source_name, source_type) = {
                let c = &self.marks.collections[source];
                (c.name.clone(), c.collection_type)
            };
            let (target_name, target_type) = {
                let c = &self.marks.collections[target];
                (c.name.clone(), c.collection_type)
            };
            if source_type != target_type {
                let text = format!(
                    "Warning, {source_name} is of type {} and\nthe target {target_name} is of type {}. Proceed?",
                    collection_type_name(source_type),
                    collection_type_name(target_type)
                );
                if !proceed(&MismatchInfo { text }) {
                    break;
                }
            }
            let mut ordered = list;
            ordered.sort_by_key(|m| m.start_time);
            for m in ordered {
                // `Mark.Clone` sets the duration through its setter, so the 25 ms floor applies.
                let mut clone = Mark::new(base_time + (m.start_time - earliest));
                set_mark_duration(&mut clone, m.duration);
                clone.text = m.text.clone();
                pasted.push(clone.id);
                added.push((self.marks.collections[target].id, clone.clone()));
                self.marks.collections[target].add_mark(clone);
            }
            ensure_order(&mut self.marks.collections[target]);
        }
        if !pasted.is_empty() {
            self.marks.rebuild_all();
            self.marks.selected = pasted;
            self.marks_selection_changed();
            self.commit_marks(host);
            host.record_undo(UndoAction::MarksAdded { label: marks_added_label(added.len()), marks: added });
            self.requests.push(HostRequest::UndoRecorded);
        }
    }

    /// F133's questions, in the order they'd come, with no change made. For bindings, which can't call back in the middle of a paste.
    pub fn paste_questions(&mut self, press: (i32, i32)) -> Vec<String> {
        let rows = self.mark_row_tops();
        let Some(base_row) = rows.iter().find(|(c, top)| press.1 < top + self.marks.rows[*c].height).map(|(c, _)| *c) else { return Vec::new() };
        let mut sources: Vec<usize> = self.marks.clipboard.iter().filter_map(|clip| self.marks.collections.iter().position(|c| c.id == clip.collection)).collect();
        sources.sort_unstable();
        sources.dedup();
        let Some(offset) = sources.first().copied() else { return Vec::new() };
        let mut out = Vec::new();
        for source in sources {
            let target = base_row + source - offset;
            let Some(t) = self.marks.collections.get(target) else { continue };
            let s = &self.marks.collections[source];
            if s.collection_type != t.collection_type {
                out.push(format!(
                    "Warning, {} is of type {} and\nthe target {} is of type {}. Proceed?",
                    s.name,
                    collection_type_name(s.collection_type),
                    t.name,
                    collection_type_name(t.collection_type)
                ));
            }
        }
        out
    }

    /// `GetOrCreatePhonemeMarkCollection`.
    fn linked_collection(&mut self, parent: usize, wanted: MarkCollectionType) -> usize {
        let parent_id = self.marks.collections[parent].id;
        if let Some(found) = self.marks.collections.iter().position(|c| c.linked_mark_collection_id == Some(parent_id) && c.collection_type == wanted) {
            return found;
        }
        let parent_c = &self.marks.collections[parent];
        let mut name = format!("{} {}", parent_c.name, collection_type_name(wanted));
        if parent_c.collection_type != MarkCollectionType::Phrase {
            let phrase_parent = self.marks.collections.iter().find(|c| Some(c.id) == parent_c.linked_mark_collection_id);
            if let Some(p) = phrase_parent.filter(|p| p.collection_type == MarkCollectionType::Phrase) {
                name = format!("{} {}", p.name, collection_type_name(wanted));
            }
        }
        let color = parent_c.decorator.color;
        let mut created = self.new_mark_collection(color, &name);
        created.linked_mark_collection_id = Some(parent_id);
        created.collection_type = wanted;
        created.show_marks = true;
        self.marks.collections.push(created);
        self.marks.rows.push(MarkRowState::default());
        self.marks.collections.len() - 1
    }

    /// `CreateNewMarkCollection`: the first in a sequence becomes the default.
    fn new_mark_collection(&self, color: Color, name: &str) -> MarkCollection {
        let mut c = MarkCollection { name: unique_collection_name(name, &self.marks.collections), ..MarkCollection::default() };
        c.decorator.color = color;
        c.is_default = self.marks.collections.is_empty();
        c
    }

    /// F136. Empty entries are kept, as `Split()` keeps them when words have two spaces between them.
    pub fn breakdown_phrase(&mut self, host: &mut dyn TimelineHost) {
        let mut added: Vec<(Uuid, Mark)> = Vec::new();
        let mut by_parent: Vec<(usize, Vec<Mark>)> = Vec::new();
        for id in &self.marks.selected {
            let Some(c) = self.marks.collection_index_of(*id) else { continue };
            let Some(m) = self.marks.mark(*id).cloned() else { continue };
            match by_parent.iter_mut().find(|(g, _)| *g == c) {
                Some((_, list)) => list.push(m),
                None => by_parent.push((c, vec![m])),
            }
        }
        for (parent, marks) in by_parent {
            let target = self.linked_collection(parent, MarkCollectionType::Word);
            for mark in marks {
                let words: Vec<&str> = split_like_csharp(mark.text.trim());
                let duration = TimeSpan(mark.duration.ticks() / words.len().max(1) as i64);
                let mut start = mark.start_time;
                for word in words {
                    let mut w = Mark::new(start);
                    set_mark_duration(&mut w, duration);
                    w.text = word.to_string();
                    start += duration;
                    added.push((self.marks.collections[target].id, w.clone()));
                    self.marks.collections[target].add_mark(w);
                }
            }
            ensure_order(&mut self.marks.collections[target]);
        }
        self.marks_broken_down(added, host);
    }

    /// `AddMarks` for a breakdown: an undo entry if any were added. A new linked collection stays either way, since that has no undo.
    fn marks_broken_down(&mut self, added: Vec<(Uuid, Mark)>, host: &mut dyn TimelineHost) {
        self.marks.rebuild_all();
        self.commit_marks(host);
        if !added.is_empty() {
            host.record_undo(UndoAction::MarksAdded { label: marks_added_label(added.len()), marks: added });
            self.requests.push(HostRequest::UndoRecorded);
        }
    }

    /// F137. `convert` is the lip sync dictionary, which is another area's: it gives the phonemes for a word's cleaned text, or an empty list
    /// for an unknown word, which is skipped here unless the host's mapping dialog gave it an answer first.
    pub fn breakdown_word(&mut self, convert: &mut dyn FnMut(&str) -> Vec<String>, host: &mut dyn TimelineHost) {
        let mut added: Vec<(Uuid, Mark)> = Vec::new();
        let mut by_parent: Vec<(usize, Vec<Mark>)> = Vec::new();
        for id in &self.marks.selected {
            let Some(c) = self.marks.collection_index_of(*id) else { continue };
            let Some(m) = self.marks.mark(*id).cloned() else { continue };
            match by_parent.iter_mut().find(|(g, _)| *g == c) {
                Some((_, list)) => list.push(m),
                None => by_parent.push((c, vec![m])),
            }
        }
        for (parent, marks) in by_parent {
            let target = self.linked_collection(parent, MarkCollectionType::Phoneme);
            for mark in marks {
                let phonemes = convert(mark.text.trim());
                if phonemes.is_empty() {
                    continue;
                }
                let duration = TimeSpan(mark.duration.ticks() / phonemes.len() as i64);
                let mut start = mark.start_time;
                for phoneme in phonemes {
                    let mut p = Mark::new(start);
                    set_mark_duration(&mut p, duration);
                    p.text = phoneme;
                    start += duration;
                    added.push((self.marks.collections[target].id, p.clone()));
                    self.marks.collections[target].add_mark(p);
                }
            }
            ensure_order(&mut self.marks.collections[target]);
        }
        self.marks_broken_down(added, host);
    }

    /// `AddMarkAtTime`. F111. A question comes back as a result; the host answers with the matching call and asks again.
    pub fn add_mark_at(&mut self, t: TimeSpan, fill_gap: bool, label: Option<String>, host: &mut dyn TimelineHost) -> AddMarkResult {
        if self.marks.collections.is_empty() {
            return AddMarkResult::NeedsNewCollection;
        }
        let visible = |c: &MarkCollection| c.show_grid_lines || c.show_marks;
        let Some(c) = self.marks.collections.iter().position(|c| c.is_default && visible(c)) else {
            return if self.marks.collections.iter().any(|c| c.is_default) { AddMarkResult::NeedsEnableDefault } else { AddMarkResult::Nothing };
        };
        if self.marks.collections[c].marks.iter().any(|m| m.start_time == t) {
            return AddMarkResult::Nothing;
        }
        let mut mark = Mark::new(t);
        mark.text = label.unwrap_or_default();
        let id = mark.id;
        self.marks.collections[c].add_mark(mark);
        ensure_order(&mut self.marks.collections[c]);
        if fill_gap {
            fill_gap_times(&mut self.marks.collections[c], id);
        }
        self.marks.rebuild_all();
        self.commit_marks(host);
        let collection = self.marks.collections[c].id;
        let saved = self.marks.collections[c].marks.iter().find(|m| m.id == id).cloned();
        host.record_undo(UndoAction::MarksAdded { label: marks_added_label(1), marks: saved.into_iter().map(|m| (collection, m)).collect() });
        self.requests.push(HostRequest::UndoRecorded);
        AddMarkResult::Added(id)
    }

    /// F111's "Default Marks", in white.
    pub fn create_default_mark_collection(&mut self, host: &mut dyn TimelineHost) {
        let existing = self.marks.collections.iter().any(|c| c.name.trim().eq_ignore_ascii_case("Default Marks"));
        if !existing {
            let created = self.new_mark_collection(Color::rgb(255, 255, 255), "Default Marks");
            self.marks.collections.push(created);
            self.marks.rebuild_all();
            self.commit_marks(host);
        }
    }

    /// F111's "enable it": grid lines and bar on, tail lines off, on the default collection.
    pub fn enable_default_mark_collection(&mut self, host: &mut dyn TimelineHost) {
        if let Some(c) = self.marks.collections.iter_mut().find(|c| c.is_default) {
            c.show_grid_lines = true;
            c.show_marks = true;
            c.show_tail_lines = false;
            self.marks.rebuild_all();
            self.commit_marks(host);
        }
    }

    /// Lines for marks being dragged, at their live positions. Times in the alignment list are skipped, since the guide replaces them.
    pub(crate) fn moving_mark_lines(&self) -> Vec<MarkLine> {
        let mut out = Vec::new();
        let tpp = self.tpp();
        for id in &self.marks.moving {
            let (Some(c), Some(m)) = (self.marks.collection_of(*id), self.marks.mark(*id)) else { continue };
            if !c.show_grid_lines {
                continue;
            }
            let mut push = |time: TimeSpan| {
                if !self.guides.contains(&time) {
                    out.push(MarkLine {
                        time,
                        x: px(time, tpp),
                        color: c.decorator.color,
                        width: if c.decorator.is_bold { 3 } else { 1 },
                        dash: if c.decorator.is_solid_line { 0 } else { c.level.max(1) },
                    });
                }
            };
            push(m.start_time);
            if c.show_tail_lines {
                push(m.end_time());
            }
        }
        out
    }

    /// `mark_lines` without the marks being dragged.
    pub(crate) fn without_moving_marks(&self, lines: Vec<MarkLine>) -> Vec<MarkLine> {
        lines
            .into_iter()
            .filter_map(|line| {
                let details = self.snap_index.details_at(line.time);
                let rest: Vec<_> = details.iter().filter(|d| !self.marks.moving.contains(&d.mark)).collect();
                let mut chosen = *rest.first()?;
                for d in &rest {
                    if d.level > chosen.level && d.color != Color::EMPTY {
                        chosen = d;
                    }
                }
                Some(MarkLine {
                    color: chosen.color,
                    width: if chosen.bold { 3 } else { 1 },
                    dash: if chosen.solid { 0 } else { chosen.level.max(1) },
                    ..line
                })
            })
            .collect()
    }
}

/// `string.Split()` with no arguments: on whitespace, keeping empty entries.
fn split_like_csharp(text: &str) -> Vec<&str> {
    text.split(char::is_whitespace).collect()
}

/// `GetHorizontalAutoScrollDistance`. F129.
pub fn autoscroll_distance(x: i32, width: i32) -> i32 {
    edge_distance(x, width)
}

/// `CalculateAutoScrollVisibleTimeStart`: 0 when the whole sequence fits.
pub fn autoscroll_visible_start(start: TimeSpan, outside: i32, tpp: TimeSpan, total: TimeSpan, span: TimeSpan) -> TimeSpan {
    let candidate = start + TimeSpan(tpp.ticks() * (outside / AUTOSCROLL_SCALE) as i64);
    if candidate < TimeSpan::ZERO || span >= total {
        return TimeSpan::ZERO;
    }
    candidate.min(total - span)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::tests::RecordingHost;

    fn sec(v: f64) -> TimeSpan {
        TimeSpan::from_secs(v)
    }

    #[test]
    fn duration_has_a_floor_and_the_start_setter_raises_it() {
        let mut m = Mark::new(sec(1.0));
        set_mark_duration(&mut m, TimeSpan::from_ms_i(10));
        assert_eq!(m.duration, TimeSpan::from_ms_i(25));
        m.duration = TimeSpan::from_ms_i(5);
        set_mark_start(&mut m, sec(2.0));
        assert_eq!(m.duration, TimeSpan::from_ms_i(25));
    }

    #[test]
    fn fill_gaps_uses_both_neighbours_and_leaves_the_last_alone() {
        let mut c = MarkCollection::default();
        for t in [1.0, 3.0, 5.0] {
            let mut m = Mark::new(sec(t));
            m.duration = TimeSpan::from_ms_i(500);
            c.add_mark(m);
        }
        let (middle, last) = (c.marks[1].id, c.marks[2].id);
        fill_gap_times(&mut c, middle);
        assert_eq!(c.marks[1].start_time, TimeSpan::from_ms_i(1501));
        assert_eq!(c.marks[1].duration, TimeSpan::from_ms_i(3498));
        fill_gap_times(&mut c, last);
        assert_eq!(c.marks[2].duration, TimeSpan::from_ms_i(500));
    }

    #[test]
    fn text_colour_and_hatch() {
        assert_eq!(ideal_text_color(Color::rgb(255, 255, 255)), Color::rgb(0, 0, 0));
        assert_eq!(ideal_text_color(Color::rgb(0, 0, 0)), Color::rgb(255, 255, 255));
        assert_eq!(ideal_text_color(Color::rgb(150, 150, 150)), Color::rgb(0, 0, 0));
        assert_eq!(ideal_text_color(Color::rgb(149, 149, 149)), Color::rgb(255, 255, 255));
        let hatch = locked_hatch_color(Color::rgb(200, 240, 0));
        assert_eq!((hatch.r, hatch.g, hatch.b), (230, 210, 30));
    }

    // MarksBarAutoScrollTests.cs
    #[test]
    fn autoscroll_formulas() {
        assert_eq!(autoscroll_distance(10, 100), -14);
        assert_eq!(autoscroll_distance(90, 100), 14);
        assert_eq!(autoscroll_distance(50, 100), 0);
        let tpp = TimeSpan::from_ms_i(100);
        let span = sec(10.0);
        assert_eq!(autoscroll_visible_start(TimeSpan::from_ms_i(500), -400, tpp, sec(60.0), span), TimeSpan::ZERO);
        assert_eq!(autoscroll_visible_start(sec(49.0), 80, tpp, sec(60.0), span), sec(50.0));
        assert_eq!(autoscroll_visible_start(sec(2.0), 80, tpp, sec(5.0), span), TimeSpan::ZERO);
        assert_eq!(autoscroll_visible_start(sec(10.0), 80, tpp, sec(60.0), span), sec(12.0));
        assert_eq!(autoscroll_visible_start(sec(10.0), -80, tpp, sec(60.0), span), sec(8.0));
    }

    fn mark(t: f64, d: f64, text: &str) -> Mark {
        let mut m = Mark::new(sec(t));
        m.duration = sec(d);
        m.text = text.to_string();
        m
    }

    fn collection(name: &str, marks: Vec<Mark>) -> MarkCollection {
        MarkCollection { name: name.to_string(), show_marks: true, show_grid_lines: true, marks, ..MarkCollection::default() }
    }

    fn timeline(collections: Vec<MarkCollection>) -> Timeline {
        let mut t = Timeline::new();
        t.info.time_per_pixel = TimeSpan::from_ms_i(10);
        t.info.total = sec(600.0);
        t.set_viewport(1000, 500, 1000);
        t.load_marks(collections);
        t.refresh_marks_layout();
        t
    }

    fn at(x: i32, y: i32, button: Button, mods: Mods) -> PointerEvent {
        PointerEvent::new(x, y, Some(button), mods)
    }

    fn click(t: &mut Timeline, x: i32, y: i32, mods: Mods, host: &mut RecordingHost) {
        t.marks_mouse_down(at(x, y, Button::Left, mods), host);
        t.marks_mouse_up(at(x, y, Button::Left, mods), host);
    }

    #[test]
    fn row_height_follows_the_lanes_in_view_and_the_bar_sums_rows() {
        let mut t = timeline(vec![
            collection("A", vec![mark(0.0, 2.0, "a"), mark(1.0, 2.0, "b"), mark(100.0, 1.0, "c")]),
            collection("B", vec![mark(0.0, 1.0, "d")]),
        ]);
        assert_eq!(t.marks.rows[0].height, 40);
        assert_eq!(t.marks.rows[1].height, 20);
        assert_eq!(t.marks.bar_height, 60);
        // KEEP Q24: scrolling away from the overlap shrinks it.
        t.set_visible_start(sec(50.0));
        t.refresh_marks_layout();
        assert_eq!(t.marks.rows[0].height, 20);
        assert_eq!(t.marks.bar_height, 40);
        // Hidden rows don't count.
        t.marks.collections[1].show_marks = false;
        t.marks.rebuild_all();
        t.refresh_marks_layout();
        assert_eq!(t.marks.bar_height, 20);
    }

    #[test]
    fn hit_test_uses_the_lanes() {
        let t = timeline(vec![collection("A", vec![mark(0.0, 2.0, "a"), mark(1.0, 2.0, "b")])]);
        let (a, b) = (t.marks.collections[0].marks[0].id, t.marks.collections[0].marks[1].id);
        assert_eq!(t.mark_at((50, 5)), Some(a));
        assert_eq!(t.mark_at((150, 5)), Some(a));
        assert_eq!(t.mark_at((150, 25)), Some(b));
        assert_eq!(t.mark_at((50, 25)), None);
        assert_eq!(t.mark_at((250, 5)), None);
        assert_eq!(t.mark_at((50, 45)), None);
    }

    #[test]
    fn clicks_select_toggle_and_collapse() {
        let mut host = RecordingHost::default();
        let mut t = timeline(vec![collection("A", vec![mark(0.0, 1.0, "a"), mark(2.0, 1.0, "b"), mark(4.0, 1.0, "c")])]);
        let ids: Vec<Uuid> = t.marks.collections[0].marks.iter().map(|m| m.id).collect();
        click(&mut t, 50, 5, Mods::NONE, &mut host);
        assert_eq!(t.marks.selected, [ids[0]]);
        click(&mut t, 250, 5, Mods::CMD, &mut host);
        assert_eq!(t.marks.selected, [ids[0], ids[1]]);
        click(&mut t, 250, 5, Mods::CMD, &mut host);
        assert_eq!(t.marks.selected, [ids[0]]);
        // The anchor is where the last selecting click was, so this takes the second and third.
        click(&mut t, 450, 5, Mods::SHIFT, &mut host);
        assert_eq!(t.marks.selected, [ids[1], ids[2]]);
        // A plain click on one of several selected collapses on release.
        click(&mut t, 250, 5, Mods::NONE, &mut host);
        assert_eq!(t.marks.selected, [ids[1]]);
        // Blank clears.
        click(&mut t, 350, 5, Mods::NONE, &mut host);
        assert!(t.marks.selected.is_empty());
    }

    #[test]
    fn locked_marks_count_as_nothing_when_pressed() {
        let mut host = RecordingHost::default();
        let mut locked = collection("A", vec![mark(0.0, 1.0, "a")]);
        locked.is_locked = true;
        let mut t = timeline(vec![locked]);
        click(&mut t, 50, 5, Mods::NONE, &mut host);
        assert!(t.marks.selected.is_empty());
    }

    #[test]
    fn a_double_click_plays_or_fills() {
        let mut host = RecordingHost::default();
        let mut t = timeline(vec![collection("A", vec![mark(1.0, 1.0, "a")])]);
        let mut ev = at(150, 5, Button::Left, Mods::NONE);
        ev.clicks = 2;
        let out = t.marks_mouse_down(ev, &mut host);
        assert_eq!(out.requests, [HostRequest::PlayRange { start: sec(1.0), end: Some(sec(2.0)) }]);
        ev.mods = Mods::OPT;
        let out = t.marks_mouse_down(ev, &mut host);
        assert_eq!(host.undo.last().map(|u| u.description()).as_deref(), Some("Resize 0 effects"));
        assert_eq!(out.requests, [HostRequest::UndoRecorded]);
    }

    #[test]
    fn dragging_clamps_keeps_rows_and_records_one_undo() {
        let mut host = RecordingHost::default();
        let mut t = timeline(vec![collection("A", vec![mark(1.0, 1.0, "a")])]);
        let id = t.marks.collections[0].marks[0].id;
        t.marks_mouse_down(at(150, 5, Button::Left, Mods::NONE), &mut host);
        // Inside the threshold nothing moves.
        t.marks_mouse_move(at(155, 5, Button::Left, Mods::NONE));
        assert_eq!(t.marks.mark(id).unwrap().start_time, sec(1.0));
        t.marks_mouse_move(at(170, 5, Button::Left, Mods::NONE));
        t.marks_mouse_move(at(250, 40, Button::Left, Mods::NONE));
        assert_eq!(t.marks.mark(id).unwrap().start_time, sec(1.8));
        assert_eq!(t.guides(), [sec(1.8), sec(2.8)]);
        t.marks_mouse_move(at(-500, 5, Button::Left, Mods::NONE));
        assert_eq!(t.marks.mark(id).unwrap().start_time, TimeSpan::ZERO);
        let out = t.marks_mouse_up(at(250, 5, Button::Left, Mods::NONE), &mut host);
        assert_eq!(host.undo.len(), 1);
        assert_eq!(host.undo[0].description(), "Moved 1 effect");
        assert!(t.guides().is_empty());
        assert!(out.requests.contains(&HostRequest::UndoRecorded));
        assert_eq!(host.marks.len(), 1);
    }

    #[test]
    fn resizing_keeps_twelve_pixels_and_gluing_saves_the_neighbour() {
        let mut host = RecordingHost::default();
        let mut t = timeline(vec![collection("A", vec![mark(1.0, 1.0, "a"), mark(2.0, 1.0, "b")])]);
        let (a, b) = (t.marks.collections[0].marks[0].id, t.marks.collections[0].marks[1].id);
        // Back edge of the first mark, ⌥ glues the next one.
        t.marks_mouse_move(at(198, 5, Button::Left, Mods::NONE));
        let glue = at(198, 5, Button::Left, Mods::OPT);
        t.marks_mouse_down(glue, &mut host);
        t.marks_mouse_move(at(218, 5, Button::Left, Mods::OPT));
        assert_eq!(t.marks.mark(a).unwrap().duration, sec(1.2));
        assert_eq!(t.marks.mark(b).unwrap().start_time, sec(2.2));
        assert_eq!(t.marks.mark(b).unwrap().end_time(), sec(3.0));
        t.marks_mouse_up(at(218, 5, Button::Left, Mods::OPT), &mut host);
        // FIX Q29: the undo data has the neighbour.
        match &host.undo[0] {
            UndoAction::MarkTimesChanged { marks, label } => {
                assert_eq!(label, "Resize 1 effect");
                assert!(marks.contains(&MarkPlacement { mark: b, start: sec(2.0), duration: sec(1.0) }));
                assert!(marks.contains(&MarkPlacement { mark: a, start: sec(1.0), duration: sec(1.0) }));
            }
            _ => panic!("marks undo"),
        }
        // Dragging the back edge far left stops at 12 pixels.
        let mut t = timeline(vec![collection("A", vec![mark(1.0, 1.0, "a")])]);
        let a = t.marks.collections[0].marks[0].id;
        t.marks_mouse_move(at(198, 5, Button::Left, Mods::NONE));
        t.marks_mouse_down(at(198, 5, Button::Left, Mods::NONE), &mut host);
        t.marks_mouse_move(at(0, 5, Button::Left, Mods::NONE));
        assert_eq!(t.marks.mark(a).unwrap().duration, TimeSpan::from_ms_i(120));
    }

    #[test]
    fn delete_twice_records_one_undo_q31() {
        let mut host = RecordingHost::default();
        let mut t = timeline(vec![collection("A", vec![mark(1.0, 1.0, "a")])]);
        click(&mut t, 150, 5, Mods::NONE, &mut host);
        t.delete_selected_marks(&mut host);
        t.delete_selected_marks(&mut host);
        assert_eq!(host.undo.len(), 1);
        assert_eq!(host.undo[0].description(), "Removed 1 Mark");
        assert!(t.marks.collections[0].marks.is_empty());
    }

    fn with_type(mut c: MarkCollection, t: MarkCollectionType) -> MarkCollection {
        c.collection_type = t;
        c
    }

    #[test]
    fn paste_keeps_offsets_and_row_offsets_and_stops_on_a_mismatch() {
        let mut host = RecordingHost::default();
        let a = collection("A", vec![mark(1.0, 1.0, "a"), mark(3.0, 1.0, "c")]);
        let b = collection("B", vec![mark(2.0, 1.0, "b")]);
        let c = with_type(collection("C", vec![]), MarkCollectionType::Phrase);
        let mut t = timeline(vec![a, b, c]);
        t.marks.selected = t.marks.collections.iter().flat_map(|c| c.marks.iter().map(|m| m.id)).collect();
        t.copy_marks();
        // Pasting at row 1: A's group goes to B, B's goes to C, which is another type.
        t.paste_marks((1000, 25), &mut |_| false, &mut host);
        assert_eq!(t.marks.collections[1].marks.iter().map(|m| m.start_time).collect::<Vec<_>>(), [sec(2.0), sec(10.0), sec(12.0)]);
        assert!(t.marks.collections[2].marks.is_empty());
        assert_eq!(host.undo[0].description(), "Added 2 Marks");
        // Yes carries on, with the base time taken from the earliest of the whole clipboard.
        let mut host = RecordingHost::default();
        let mut asked = 0;
        t.paste_marks((2000, 25), &mut |m| { asked += 1; m.text.contains("Warning, B is of type Generic") }, &mut host);
        assert_eq!(asked, 1);
        assert_eq!(t.marks.collections[2].marks[0].start_time, sec(21.0));
        // A group past the last row is skipped.
        let mut host = RecordingHost::default();
        t.paste_marks((3000, 45), &mut |_| true, &mut host);
        assert_eq!(host.undo.len(), 1);
    }

    #[test]
    fn a_pasted_mark_takes_the_duration_floor_as_clone_does() {
        // A file can hold a mark shorter than the floor; upstream's setter would have raised it on load, and `Clone` raises it again.
        let mut host = RecordingHost::default();
        let mut t = timeline(vec![collection("A", vec![mark(1.0, 0.01, "short")])]);
        t.marks.selected = vec![t.marks.collections[0].marks[0].id];
        t.copy_marks();
        t.paste_marks((500, 5), &mut |_| true, &mut host);
        let pasted = t.marks.collections[0].marks.iter().find(|m| m.start_time == sec(5.0)).expect("pasted at the press");
        assert_eq!(pasted.duration, TimeSpan::from_ms_i(25));
    }

    #[test]
    fn a_cut_mark_stays_on_the_clipboard_and_edits_show_in_a_paste() {
        let mut host = RecordingHost::default();
        let mut t = timeline(vec![collection("A", vec![mark(1.0, 1.0, "a")])]);
        let id = t.marks.collections[0].marks[0].id;
        t.marks.selected = vec![id];
        t.copy_marks();
        t.set_selected_marks_text("edited", &mut host);
        t.paste_marks((500, 5), &mut |_| true, &mut host);
        assert!(t.marks.collections[0].marks.iter().filter(|m| m.id != id).all(|m| m.text == "edited"));
        t.marks.selected = vec![id];
        t.cut_marks(&mut host);
        let count = t.marks.collections[0].marks.len();
        t.paste_marks((900, 5), &mut |_| true, &mut host);
        assert_eq!(t.marks.collections[0].marks.len(), count + 1);
    }

    #[test]
    fn breakdown_phrase_makes_equal_shares_in_a_linked_collection() {
        let mut host = RecordingHost::default();
        let c = with_type(collection("Lyrics", vec![mark(0.0, 3.0, "a b c")]), MarkCollectionType::Phrase);
        let mut t = timeline(vec![c]);
        t.marks.selected = vec![t.marks.collections[0].marks[0].id];
        t.breakdown_phrase(&mut host);
        let words = &t.marks.collections[1];
        assert_eq!(words.name, "Lyrics Word");
        assert_eq!(words.collection_type, MarkCollectionType::Word);
        assert_eq!(words.linked_mark_collection_id, Some(t.marks.collections[0].id));
        assert!(words.show_marks);
        let got: Vec<(TimeSpan, TimeSpan, &str)> = words.marks.iter().map(|m| (m.start_time, m.duration, m.text.as_str())).collect();
        assert_eq!(got, [(sec(0.0), sec(1.0), "a"), (sec(1.0), sec(1.0), "b"), (sec(2.0), sec(1.0), "c")]);
        assert_eq!(host.undo[0].description(), "Added 3 Marks");
        // Doing it again finds the same collection.
        t.breakdown_phrase(&mut host);
        assert_eq!(t.marks.collections.len(), 2);
    }

    #[test]
    fn the_menu_model_follows_the_press_and_the_types() {
        let mut host = RecordingHost::default();
        let c = with_type(collection("P", vec![mark(1.0, 1.0, "AI")]), MarkCollectionType::Phoneme);
        let mut t = timeline(vec![c]);
        t.marks_mouse_down(at(150, 5, Button::Right, Mods::NONE), &mut host);
        let out = t.marks_mouse_up(at(150, 5, Button::Right, Mods::NONE), &mut host);
        assert!(matches!(out.requests[0], HostRequest::MarksContextMenu { .. }));
        let m = t.marks_context_menu();
        assert!(m.edit_text && m.cut && m.copy && m.delete && m.phoneme);
        assert!(!m.paste && !m.breakdown_phrase && !m.breakdown_word);
        // Released elsewhere, Edit Text and the type items go.
        t.marks_mouse_down(at(150, 5, Button::Right, Mods::NONE), &mut host);
        t.marks_mouse_up(at(160, 5, Button::Right, Mods::NONE), &mut host);
        let m = t.marks_context_menu();
        assert!(!m.edit_text && !m.phoneme && m.delete);
    }

    #[test]
    fn adding_marks_asks_when_it_has_to() {
        let mut host = RecordingHost::default();
        let mut t = timeline(vec![]);
        assert_eq!(t.add_mark_at(sec(1.0), false, None, &mut host), AddMarkResult::NeedsNewCollection);
        t.create_default_mark_collection(&mut host);
        assert_eq!(t.marks.collections[0].name, "Default Marks");
        assert!(t.marks.collections[0].is_default);
        t.marks.collections[0].show_grid_lines = false;
        t.marks.collections[0].show_marks = false;
        assert_eq!(t.add_mark_at(sec(1.0), false, None, &mut host), AddMarkResult::NeedsEnableDefault);
        t.enable_default_mark_collection(&mut host);
        // Tapping ignores the collection lock, unlike dragging existing marks.
        t.marks.collections[0].is_locked = true;
        assert!(matches!(t.add_mark_at(sec(1.0), false, Some("x".into()), &mut host), AddMarkResult::Added(_)));
        assert_eq!(t.marks.collections[0].marks[0].duration, TimeSpan::from_ms_i(450));
        assert_eq!(t.marks.collections[0].marks[0].text, "x");
        assert_eq!(t.add_mark_at(sec(1.0), false, None, &mut host), AddMarkResult::Nothing);
        assert_eq!(host.undo.last().unwrap().description(), "Added 1 Mark");
    }

    #[test]
    fn unique_names_count_up() {
        let cs = vec![collection("Default Marks", vec![]), collection("default marks - 2", vec![])];
        assert_eq!(unique_collection_name("Default Marks", &cs), "Default Marks - 3");
        assert_eq!(unique_collection_name("Other", &cs), "Other");
    }
}
