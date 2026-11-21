//! The per-window timeline state: time, rows, effects, selection and the frames painted from them.
//!
//! Effects keep their working times here. A drag changes these on every pointer event and the session only on release, so a drag
//! never waits on a render or the session's lock (`Element.cs:53-60`: the effect node isn't updated during a drag either).

use super::grid_input::GridInput;
use super::marks::MarksState;
use super::rows::{Occ, RowTree};
use super::snap::{mark_lines, MarkLine, MarkSnapIndex};
use super::stack::{lane_geometry, occurrence_rect, walk_stacks, OccurrenceRect, StackScratch};
use super::view::{clamp_visible_start, px, time_at, visible_end, visible_span, zoom, zoom_at, TimeInfo};
use super::waveform::WaveformState;
use super::{CursorKind, ElementId, ElementTimes, IRect, RowId};
use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};
use std::sync::Arc;
use uuid::Uuid;
use vixen_core::{Color, TimeSpan};

/// Ids are small sequential integers, so a multiply is all the hashing they need.
#[derive(Default)]
pub struct IdHasher(u64);

impl Hasher for IdHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }

    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0 ^ *b as u64).wrapping_mul(0x100000001b3);
        }
    }

    #[inline]
    fn write_u64(&mut self, v: u64) {
        self.0 = v.wrapping_mul(0x9E3779B97F4A7C15);
    }
}

pub type IdMap<V> = HashMap<ElementId, V, BuildHasherDefault<IdHasher>>;

/// The effect-side facts a row or tooltip shows.
#[derive(Clone, Debug, PartialEq)]
pub struct ElementDescription {
    pub type_id: Uuid,
    /// `Descriptor.TypeName`, the same as `EffectName` here.
    pub type_name: Arc<str>,
    pub layer_level: i32,
    pub layer_name: Arc<str>,
}

impl Default for ElementDescription {
    fn default() -> Self {
        ElementDescription { type_id: Uuid::nil(), type_name: Arc::from(""), layer_level: 0, layer_name: Arc::from("") }
    }
}

/// `Element`: one per effect node, the same in every row (F41).
#[derive(Clone, Debug)]
pub struct ElementState {
    pub id: ElementId,
    pub start: TimeSpan,
    pub duration: TimeSpan,
    /// `Element.Row`: the row it was most recently added to. Not to be trusted, as upstream says, and yet the paste and clone rules use it.
    pub last_row: Option<RowId>,
    /// Every row holding it, ascending, so the first is `RowContainingElement`.
    pub rows: Vec<RowId>,
    pub selected: bool,
    pub desc: ElementDescription,
    /// The display element it targets, which only reaches the effect at commit (`Element.cs:69-72`).
    pub target: Option<vixen_core::NodeId>,
    /// There is a picture to show: rendered, and the duration hasn't changed since. A plain move doesn't dirty it (F43).
    pub rendered: bool,
    /// Changes when the picture's content does. Swift's thumbnail cache is keyed by it.
    pub picture: u64,
    /// The duration of the last render.
    pub rendered_duration: TimeSpan,
}

impl ElementState {
    #[inline]
    pub fn times(&self) -> ElementTimes {
        ElementTimes::new(self.start, self.duration)
    }

    #[inline]
    pub fn end(&self) -> TimeSpan {
        self.start + self.duration
    }
}

/// `Color.FromName` for the Resize / Draw Indicator's five choices. Note `Color.Green` is (0,128,0).
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum IndicatorColor {
    Blue,
    Yellow,
    Green,
    White,
    #[default]
    Red,
}

impl IndicatorColor {
    pub fn color(self) -> Color {
        match self {
            IndicatorColor::Blue => Color::rgb(0, 0, 255),
            IndicatorColor::Yellow => Color::rgb(255, 255, 0),
            IndicatorColor::Green => Color::rgb(0, 128, 0),
            IndicatorColor::White => Color::rgb(255, 255, 255),
            IndicatorColor::Red => Color::rgb(255, 0, 0),
        }
    }

    /// As the profile stores it.
    pub fn from_name(name: &str) -> IndicatorColor {
        match name {
            "Blue" => IndicatorColor::Blue,
            "Yellow" => IndicatorColor::Yellow,
            "Green" => IndicatorColor::Green,
            "White" => IndicatorColor::White,
            _ => IndicatorColor::Red,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            IndicatorColor::Blue => "Blue",
            IndicatorColor::Yellow => "Yellow",
            IndicatorColor::Green => "Green",
            IndicatorColor::White => "White",
            IndicatorColor::Red => "Red",
        }
    }
}

/// F53.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DragBoxFilter {
    pub enabled: bool,
    pub types: HashSet<Uuid>,
}

/// Toggles the menus and toolbars set. Defaults are the profile's, and the owner of the window persists them.
#[derive(Clone, Debug)]
pub struct TimelineOptions {
    pub snap_enabled: bool,
    pub snap_strength: i32,
    pub draw_mode: bool,
    /// `Grid.SelectedEffect`: the type the Effects panel has chosen for drawing.
    pub selected_effect: Option<Uuid>,
    pub cad_style: bool,
    pub legacy_cursor: bool,
    pub zoom_under_mouse: bool,
    pub highlight_rows: bool,
    pub show_effect_info: bool,
    pub resize_indicator: bool,
    pub resize_indicator_color: IndicatorColor,
    pub drag_box_filter: DragBoxFilter,
}

impl Default for TimelineOptions {
    fn default() -> Self {
        TimelineOptions {
            snap_enabled: true,
            snap_strength: 2,
            draw_mode: false,
            selected_effect: None,
            cad_style: false,
            legacy_cursor: false,
            zoom_under_mouse: false,
            highlight_rows: false,
            show_effect_info: true,
            resize_indicator: false,
            resize_indicator_color: IndicatorColor::Red,
            drag_box_filter: DragBoxFilter::default(),
        }
    }
}

/// What changed since Swift last asked, by surface.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Dirty {
    /// Every grid tile.
    pub grid_all: bool,
    /// Canvas rectangles of grid tiles.
    pub grid_rects: Vec<IRect>,
    pub row_header: bool,
    pub ruler: bool,
    pub marks_bar: bool,
    pub waveform: bool,
    /// Selection borders, lifted effects, rubber band, hover, guides, cursor.
    pub overlay: bool,
    /// Canvas size, offsets or a bar's height changed.
    pub view: bool,
}

/// What a surface wants from the window around it. `HostRequest` in the spec.
#[derive(Clone, Debug, PartialEq)]
pub enum HostRequest {
    /// F50: more than one effect under the press. Answer with `select_element` and `grid_menu_closed`.
    ChooseOverlapping { elements: Vec<ElementId>, at: (i32, i32) },
    /// F54.
    ContextMenu { elements: Vec<ElementId>, time: TimeSpan, row: RowId },
    /// F33.
    RowContextMenu { row: RowId },
    Warning { text: String, title: String },
    /// An undo entry went on the session's stack.
    UndoRecorded,
    /// F55. Nothing in the editor listens, but the event exists.
    ElementDoubleClicked(ElementId),
    /// F101, F103, F124: play this range.
    PlayRange { start: TimeSpan, end: Option<TimeSpan> },
    /// F112.
    MarkMenu,
    /// F130 to F137. Show the menu `marks_context_menu` describes.
    MarksContextMenu { press: (i32, i32), release: (i32, i32) },
    /// F111: a mark needs a collection first. Answer with `add_mark_at` again.
    NeedsNewCollection { time: TimeSpan, fill_gap: bool, label: Option<String> },
    NeedsEnableDefault { time: TimeSpan, fill_gap: bool, label: Option<String> },
    /// F111: ask for the label, then call `add_mark_at`.
    PromptMarkLabel { time: TimeSpan, fill_gap: bool },
    /// F111: call `add_mark_at`, answering any question it comes back with.
    AddMark { time: TimeSpan, fill_gap: bool, label: Option<String> },
}

/// The result of an input call. `InputOutcome` in the spec.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InputOutcome {
    pub cursor: CursorKind,
    pub dirty: Dirty,
    pub selection_changed: bool,
    pub cursor_moved: bool,
    /// Run the 50 ms tick.
    pub autoscroll: bool,
    pub requests: Vec<HostRequest>,
    /// F153's editor state flag was set just now.
    pub editor_state_modified: bool,
}

/// A row as a tile paints it.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct RowQuad {
    pub id: RowId,
    pub top: i32,
    pub height: i32,
    pub selected: bool,
    pub active: bool,
}

/// An occurrence as a tile or a lifted layer paints it, in canvas coordinates. `x` and `w` cover the whole effect; the viewport
/// clips, which draws the same as upstream's clipped rectangle.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct ElementQuad {
    pub id: ElementId,
    pub row: RowId,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub selected: bool,
    pub rendered: bool,
    pub red_border: bool,
    pub moving: bool,
    pub picture: u64,
}

/// F10, for one rectangle of the canvas.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GridFrame {
    pub rows: Vec<RowQuad>,
    pub elements: Vec<ElementQuad>,
    pub mark_lines: Vec<MarkLine>,
    /// Canvas x of each 1 second dashed line.
    pub gridlines: Vec<f64>,
    pub canvas_width: i32,
    pub canvas_height: i32,
}

/// F45.
#[derive(Clone, Debug, PartialEq)]
pub struct HoverInfo {
    pub text: String,
    /// The rectangle upstream hands `DrawInfo`, in canvas coordinates: the effect's x and width in the row under the pointer.
    pub rect: IRect,
}

/// What moves without touching a tile.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OverlayFrame {
    /// Selected or lifted occurrences near the viewport.
    pub occurrences: Vec<ElementQuad>,
    pub rubber_band: Option<IRect>,
    /// `SelectionBorder`: green for a CAD enclosed rubber band.
    pub selection_green: bool,
    pub draw_box: Option<IRect>,
    /// F79: full-height lines at these canvas x.
    pub indicator_lines: Vec<f64>,
    pub indicator_color: Color,
    /// F76: yellow dashed guides in the grid, only when it isn't dragging.
    pub grid_guides: Vec<f64>,
    /// Guides for the waveform (F143), which draws them through a drag.
    pub guides: Vec<f64>,
    /// Lines of marks being dragged, lifted out of the tiles.
    pub moving_mark_lines: Vec<MarkLine>,
    pub hover: Option<HoverInfo>,
    pub cursor_x: f64,
}

/// F4 to F8: the per-document object upstream keeps in global dictionaries, and everything else one sequencer window needs.
pub struct Timeline {
    pub info: TimeInfo,
    /// `CursorPosition`.
    pub cursor: TimeSpan,
    /// The grid's client size. `VisibleTimeSpan` and every command use its width.
    pub grid_width: i32,
    pub grid_height: i32,
    /// `splitContainer.Panel2.Width`, for zoom under the pointer. Includes any scrollbar.
    pub panel_width: i32,
    pub vertical_offset: i32,
    /// Sub-point trackpad scrolling that hasn't added up to a point yet.
    pub(crate) vertical_remainder: f64,

    pub rows: RowTree,
    pub elements: IdMap<ElementState>,
    pub opts: TimelineOptions,
    pub snap_index: MarkSnapIndex,
    pub marks: MarksState,
    pub waveform: WaveformState,

    pub(crate) first_selected_row: Option<RowId>,
    selected_cache: Option<Vec<ElementId>>,
    pub(crate) selection_green: bool,
    /// `MarkAlignmentPoints`.
    pub(crate) guides: Vec<TimeSpan>,
    /// Tag definitions, for the row labels' dots and the Tags menu.
    pub tags: Vec<super::row_input::TagDisplay>,
    /// The row label under a press, and a height drag's row and last y within it.
    pub(crate) row_press: Option<RowId>,
    pub(crate) row_resizing: Option<(RowId, i32)>,
    /// A trackpad's ⌘⌥ or ⌘⇧ scroll, adding up to a row height step.
    pub(crate) wheel_accumulator: i32,
    /// A ⇧ pinch, adding up to a row height step.
    pub(crate) pinch_rows: f64,
    /// Mark selection changed since the last time someone asked.
    pub(crate) marks_selection_notify: bool,
    pub(crate) studio_lit_collections: Vec<Uuid>, // STUDIO: collections with selected marks, whose lines cross the grid (R35)
    pub(crate) studio_seen_row_height: i32, // STUDIO: the document's DefaultRowHeight at the last sync, to follow sequence_row_height (R36)

    pub(crate) g: GridInput,
    pub(crate) cursor_kind: CursorKind,

    pub(crate) dirty: Dirty,
    pub(crate) selection_changed: bool,
    pub(crate) cursor_moved: bool,
    pub(crate) requests: Vec<HostRequest>,
    /// Rows whose occurrence times need refreshing and sorting.
    pub(crate) rows_to_sort: Vec<RowId>,
    scratch: StackScratch,
    /// Canvas x covered by the last view walk, so a view change that exposes nothing new does no work.
    pub(crate) stack_view: Option<(i64, i64)>,
    /// A sequence's saved scroll, until the grid has a size to clamp it against.
    pub(crate) saved_visible_start: Option<TimeSpan>,
    /// `_editorStateModified`: never reset once set (F153).
    pub(crate) editor_state_modified: bool,
    /// Set since the last outcome.
    pub(crate) editor_state_note: bool,
    /// The sequence's copy of `TimePerPixel` and `VisibleTimeStart`, which a change has to differ from to count.
    pub(crate) document_view: (TimeSpan, TimeSpan),
    /// How far into the session's `rows_restored` this has followed.
    pub(crate) rows_restored_seen: usize,
}

impl Default for Timeline {
    fn default() -> Self {
        Timeline::new()
    }
}

impl Timeline {
    pub fn new() -> Timeline {
        Timeline {
            info: TimeInfo::default(),
            cursor: TimeSpan::ZERO,
            grid_width: 0,
            grid_height: 0,
            panel_width: 0,
            vertical_offset: 0,
            vertical_remainder: 0.0,
            rows: RowTree::default(),
            elements: IdMap::default(),
            opts: TimelineOptions::default(),
            snap_index: MarkSnapIndex::default(),
            marks: MarksState::default(),
            waveform: WaveformState::default(),
            first_selected_row: None,
            selected_cache: Some(Vec::new()),
            selection_green: false,
            guides: Vec::new(),
            tags: Vec::new(),
            row_press: None,
            row_resizing: None,
            wheel_accumulator: 0,
            pinch_rows: 1.0,
            marks_selection_notify: false,
            studio_lit_collections: Vec::new(), // STUDIO: R35
            studio_seen_row_height: 0, // STUDIO: R36
            g: GridInput::default(),
            cursor_kind: CursorKind::Arrow,
            dirty: Dirty { grid_all: true, row_header: true, ruler: true, marks_bar: true, waveform: true, overlay: true, view: true, ..Dirty::default() },
            selection_changed: false,
            cursor_moved: false,
            requests: Vec::new(),
            rows_to_sort: Vec::new(),
            scratch: StackScratch::default(),
            stack_view: None,
            saved_visible_start: None,
            editor_state_modified: false,
            editor_state_note: false,
            document_view: (TimeSpan::ZERO, TimeSpan::ZERO),
            rows_restored_seen: 0,
        }
    }

    // ------------------------------------------------------------------------------------------------------------------
    // View
    // ------------------------------------------------------------------------------------------------------------------

    #[inline]
    pub fn tpp(&self) -> TimeSpan {
        self.info.time_per_pixel
    }

    #[inline]
    pub fn span(&self) -> TimeSpan {
        visible_span(self.grid_width, self.tpp())
    }

    #[inline]
    pub fn vis_end(&self) -> TimeSpan {
        visible_end(&self.info, self.grid_width)
    }

    /// `-AutoScrollPosition.X`: the integer the grid translates by.
    #[inline]
    pub fn scroll_x(&self) -> i32 {
        px(self.info.visible_start, self.tpp()) as i32
    }

    /// Canvas width: `(int)px(TotalTime)`.
    #[inline]
    pub fn canvas_width(&self) -> i32 {
        px(self.info.total, self.tpp()) as i32
    }

    /// `TranslateLocation`.
    #[inline]
    pub fn to_canvas(&self, client: (i32, i32)) -> (i32, i32) {
        (client.0 + self.scroll_x(), client.1 + self.vertical_offset)
    }

    /// The `VisibleTimeStart` setter.
    pub fn set_visible_start(&mut self, value: TimeSpan) {
        let clamped = clamp_visible_start(value, self.info.total, self.span());
        if clamped != self.info.visible_start {
            self.info.visible_start = clamped;
            self.dirty.view = true;
        }
    }

    /// F8: the `VerticalOffset` setter.
    pub fn set_vertical_offset(&mut self, value: i32) {
        let max = self.rows.canvas_height() - self.grid_height;
        // Below 0, then above the maximum in that order, as upstream does. A short canvas gives a maximum below 0, which WinForms
        // pins to 0.
        let clamped = value.max(0).min(max).max(0);
        if clamped != self.vertical_offset {
            self.vertical_offset = clamped;
            self.dirty.view = true;
        }
    }

    /// Applies a new `TimePerPixel`. Mark windows are recalculated in place.
    pub fn set_time_per_pixel(&mut self, tpp: TimeSpan) {
        if tpp != self.info.time_per_pixel {
            self.info.time_per_pixel = tpp;
            self.snap_index.tpp = tpp;
            self.mark_everything_dirty();
            self.stack_view = None;
            // FIX Q46: upstream's test for this can never be true, because the end is capped at the total.
            let start = self.info.visible_start;
            self.set_visible_start(start);
        }
    }

    /// `TotalTime`: the sequence length.
    pub fn set_total(&mut self, total: TimeSpan) {
        if total != self.info.total {
            self.info.total = total;
            let start = self.info.visible_start;
            self.set_visible_start(start);
            self.mark_everything_dirty();
        }
    }

    /// The grid's client size and the panel holding it.
    pub fn set_viewport(&mut self, width: i32, height: i32, panel_width: i32) {
        if (width, height, panel_width) == (self.grid_width, self.grid_height, self.panel_width) {
            return;
        }
        let height_changed = height != self.grid_height;
        self.grid_width = width;
        self.grid_height = height;
        self.panel_width = panel_width;
        // OnResize. `Grid.cs:788-789`.
        if self.info.visible_start + self.span() > self.info.total {
            let start = self.info.total - self.span();
            self.set_visible_start(start);
        }
        if height_changed {
            let offset = self.vertical_offset;
            self.set_vertical_offset(offset);
        }
        self.dirty.view = true;
        self.dirty.overlay = true;
        self.stack_view = None;
        self.apply_pending_view();
    }

    pub(crate) fn mark_everything_dirty(&mut self) {
        self.dirty.grid_all = true;
        self.dirty.ruler = true;
        self.dirty.marks_bar = true;
        self.dirty.waveform = true;
        self.dirty.overlay = true;
        self.dirty.view = true;
    }

    /// After anything that moved rows: re-clamp the offset and repaint (F8).
    pub(crate) fn rows_changed(&mut self) {
        let offset = self.vertical_offset;
        self.set_vertical_offset(offset);
        self.dirty.grid_all = true;
        self.dirty.row_header = true;
        self.dirty.overlay = true;
        self.dirty.view = true;
        self.stack_view = None;
    }

    /// F15.
    pub fn zoom_time(&mut self, scale: f64) -> bool {
        match zoom(&self.info, self.grid_width, scale) {
            Some((tpp, start)) => {
                self.apply_zoom(tpp, start);
                true
            }
            None => false,
        }
    }

    /// F16. `pointer_offset_px` is the pointer's distance from the splitter.
    pub fn zoom_time_at(&mut self, pointer_offset_px: i32, scale: f64) -> bool {
        match zoom_at(&self.info, self.grid_width, self.panel_width, pointer_offset_px, scale) {
            Some((tpp, start)) => {
                self.apply_zoom(tpp, start);
                true
            }
            None => false,
        }
    }

    fn apply_zoom(&mut self, tpp: TimeSpan, start: TimeSpan) {
        self.set_time_per_pixel(tpp);
        self.set_visible_start(start);
    }

    /// F31.
    pub fn zoom_rows(&mut self, scale: f64) {
        self.rows.zoom_rows(scale, self.grid_height);
        self.rows_changed();
        self.mark_editor_state_modified();
    }

    /// F153: saves silently at close. Set by zoom, scroll and row height, and never cleared.
    pub fn mark_editor_state_modified(&mut self) {
        if !self.editor_state_modified {
            self.editor_state_note = true;
        }
        self.editor_state_modified = true;
    }

    pub fn is_editor_state_modified(&self) -> bool {
        self.editor_state_modified
    }

    // ------------------------------------------------------------------------------------------------------------------
    // Elements in rows
    // ------------------------------------------------------------------------------------------------------------------

    /// `AddElementForEffectNode`: one effect, in every row of its target node.
    pub fn add_element(&mut self, id: ElementId, times: ElementTimes, target: Option<vixen_core::NodeId>, desc: ElementDescription) {
        let targets: Vec<vixen_core::NodeId> = target.into_iter().collect();
        self.add_element_to_targets(id, times, &targets, desc);
    }

    /// F41: one effect, in every row of every target it has.
    pub fn add_element_to_targets(&mut self, id: ElementId, times: ElementTimes, targets: &[vixen_core::NodeId], desc: ElementDescription) {
        let state = ElementState {
            id,
            start: times.start,
            duration: times.duration,
            last_row: None,
            rows: Vec::new(),
            selected: false,
            desc,
            target: targets.first().copied(),
            rendered: false,
            picture: 0,
            rendered_duration: TimeSpan::ZERO,
        };
        self.elements.insert(id, state);
        for node in targets {
            for r in self.rows.rows_of_node(*node).to_vec() {
                self.row_add_element(r, id);
            }
        }
    }

    /// `Row.AddElement`.
    pub(crate) fn row_add_element(&mut self, r: RowId, id: ElementId) {
        let Some(e) = self.elements.get_mut(&id) else { return };
        match e.rows.binary_search(&r) {
            Ok(_) => return,
            Err(at) => e.rows.insert(at, r),
        }
        e.last_row = Some(r);
        let (start, end, selected) = (e.start.ticks(), e.end().ticks(), e.selected);
        let moving = self.g.is_moving(id);
        let row = self.rows.row_mut(r);
        let at = row.occurrences.partition_point(|o| (o.start, o.end) <= (start, end));
        row.occurrences.insert(at, Occ { id, start, end, index: 0, count: 0, moving });
        if selected {
            row.selected_elements.push(id);
        }
        row.rebuild_derived();
        self.add_to_ancestor_counts(r, 1);
        self.after_row_membership_change(r, start, end);
    }

    /// `Row.RemoveElement`.
    pub(crate) fn row_remove_element(&mut self, r: RowId, id: ElementId) {
        let Some(e) = self.elements.get_mut(&id) else { return };
        match e.rows.binary_search(&r) {
            Ok(at) => {
                e.rows.remove(at);
            }
            Err(_) => return,
        }
        let row = self.rows.row_mut(r);
        let Some(at) = row.occurrences.iter().position(|o| o.id == id) else { return };
        let occ = row.occurrences.remove(at);
        row.selected_elements.retain(|s| *s != id);
        row.rebuild_derived();
        self.add_to_ancestor_counts(r, -1);
        self.after_row_membership_change(r, occ.start, occ.end);
    }

    fn add_to_ancestor_counts(&mut self, r: RowId, delta: i32) {
        let mut at = self.rows.row(r).parent;
        while let Some(p) = at {
            let row = self.rows.row_mut(p);
            row.subtree_occurrences = (row.subtree_occurrences as i64 + delta as i64).max(0) as u32;
            at = row.parent;
        }
    }

    fn after_row_membership_change(&mut self, r: RowId, start: i64, end: i64) {
        self.selected_cache = None;
        self.dirty.overlay = true;
        self.dirty_time_range_in_row(r, start, end);
        if self.opts.highlight_rows {
            self.dirty.row_header = true;
        }
    }

    /// Everything in a row's band between two times needs a repaint, a little wider for lanes that shift.
    pub(crate) fn dirty_time_range_in_row(&mut self, r: RowId, start: i64, end: i64) {
        let Some(top) = self.rows.row_top(r) else { return };
        let tpp = self.tpp().ticks().max(1);
        let x = (start / tpp) as i32 - 2;
        let right = (end / tpp) as i32 + 3;
        let height = self.rows.row(r).height;
        self.dirty.grid_rects.push(IRect::new(x, top, right - x, height));
    }

    /// `ElementMovedHandler`: copies times into queued rows' occurrences, sorts them once and updates what is derived.
    pub(crate) fn flush_rows(&mut self) {
        let mut queue = std::mem::take(&mut self.rows_to_sort);
        for r in queue.drain(..) {
            let row = self.rows.row_mut(r);
            let all = row.queued_sort_all;
            row.queued_sort = false;
            row.queued_sort_all = false;
            for occ in row.occurrences.iter_mut() {
                if all || occ.moving {
                    if let Some(e) = self.elements.get(&occ.id) {
                        occ.start = e.start.ticks();
                        occ.end = e.end().ticks();
                    }
                }
            }
            row.occurrences.sort_by_key(|o| (o.start, o.end));
            row.rebuild_derived();
            if !row.has_overlap {
                // Every slot is index 0 of 1, however it got here. A lane change repaints that occurrence.
                let mut changed = Vec::new();
                for occ in row.occurrences.iter_mut() {
                    if occ.count != 1 || occ.index != 0 {
                        if occ.count > 1 && !occ.moving {
                            changed.push((occ.start, occ.end));
                        }
                        occ.count = 1;
                        occ.index = 0;
                    }
                }
                for (start, end) in changed {
                    self.dirty_time_range_in_row(r, start, end);
                }
            }
        }
        self.rows_to_sort = queue;
    }

    /// Sets an effect's working times, with the same guard as the `StartTime` setter: below zero becomes zero. `all_in_row` is for a change that
    /// isn't to a lifted effect, which the tiles have to hear about.
    pub(crate) fn set_element_times(&mut self, id: ElementId, start: TimeSpan, duration: TimeSpan, all_in_row: bool) -> bool {
        let Timeline { elements, rows, rows_to_sort, .. } = self;
        let Some(e) = elements.get_mut(&id) else { return false };
        let start = start.max(TimeSpan::ZERO);
        if e.start == start && e.duration == duration {
            return false;
        }
        let was = (e.start.ticks(), e.end().ticks());
        e.start = start;
        e.duration = duration;
        for r in &e.rows {
            let row = rows.row_mut(*r);
            if !row.queued_sort {
                row.queued_sort = true;
                rows_to_sort.push(*r);
            }
            row.queued_sort_all |= all_in_row;
        }
        if all_in_row {
            let affected = e.rows.clone();
            for r in affected {
                self.dirty_time_range_in_row(r, was.0, was.1);
                self.dirty_time_range_in_row(r, start.ticks(), (start + duration).ticks());
            }
        }
        true
    }

    // ------------------------------------------------------------------------------------------------------------------
    // Stacks
    // ------------------------------------------------------------------------------------------------------------------

    /// `SetStackIndexes` for the rows in view, ahead of paint and hit tests rather than during paint (FIX Q3). Only a row whose walk
    /// would differ does any work. A changed lane repaints that occurrence.
    pub(crate) fn refresh_view_stacks(&mut self) {
        self.flush_rows();
        let (start, end) = (self.info.visible_start.ticks(), self.vis_end().ticks());
        let first = self.rows.row_tops()[1..].partition_point(|&b| b <= self.vertical_offset);
        let last_y = self.vertical_offset + self.grid_height;
        let mut scratch = std::mem::take(&mut self.scratch);
        let mut repaint: Vec<(RowId, i64, i64)> = Vec::new();
        for i in first..self.rows.visible().len() {
            if self.rows.row_tops()[i] >= last_y {
                break;
            }
            let r = self.rows.visible()[i];
            let row = self.rows.row_mut(r);
            if !row.has_overlap {
                for occ in row.occurrences.iter_mut().filter(|o| o.count == 0) {
                    occ.count = 1;
                }
                continue;
            }
            let m0 = row.prefix_max_end.partition_point(|&m| m < start);
            let stop = row.occurrences.partition_point(|o| o.start <= end);
            if row.walk_key == Some((m0, stop)) {
                continue;
            }
            row.walk_key = Some((m0, stop));
            let mut assignments = Vec::new();
            let occurrences = &row.occurrences;
            walk_stacks(
                occurrences.len(),
                |k| occurrences[k].start,
                |k| occurrences[k].end,
                Some(&row.prefix_max_end),
                start,
                end,
                &mut scratch,
                |k, slot| assignments.push((k, slot)),
            );
            for (k, slot) in assignments {
                let occ = &mut row.occurrences[k];
                if occ.index != slot.index || occ.count != slot.count {
                    if !occ.moving {
                        repaint.push((r, occ.start, occ.end));
                    }
                    occ.index = slot.index;
                    occ.count = slot.count;
                    self.dirty.overlay = true;
                }
            }
        }
        self.scratch = scratch;
        for (r, s, e) in repaint {
            self.dirty_time_range_in_row(r, s, e);
        }
    }

    /// Gives occurrences that have never been in view lanes for a span, without disturbing any that have. They only matter off screen,
    /// and the view walk corrects them once they are on it.
    pub(crate) fn ensure_slots(&mut self, r: RowId, from: i64, to: i64) {
        let row = self.rows.row_mut(r);
        let lo = row.prefix_max_end.partition_point(|&m| m < from);
        let hi = row.occurrences.partition_point(|o| o.start <= to);
        if lo >= hi || !row.occurrences[lo..hi].iter().any(|o| o.count == 0) {
            return;
        }
        if !row.has_overlap {
            for occ in row.occurrences[lo..hi].iter_mut() {
                occ.count = 1;
                occ.index = 0;
            }
            return;
        }
        let mut scratch = std::mem::take(&mut self.scratch);
        let mut assignments = Vec::new();
        let occurrences = &row.occurrences;
        walk_stacks(
            occurrences.len(),
            |k| occurrences[k].start,
            |k| occurrences[k].end,
            Some(&row.prefix_max_end),
            from,
            to,
            &mut scratch,
            |k, slot| assignments.push((k, slot)),
        );
        for (k, slot) in assignments {
            let occ = &mut row.occurrences[k];
            if occ.count == 0 {
                occ.index = slot.index;
                occ.count = slot.count;
            }
        }
        self.scratch = scratch;
    }

    // ------------------------------------------------------------------------------------------------------------------
    // Hit testing
    // ------------------------------------------------------------------------------------------------------------------

    /// The test the hit functions share: `px(start) <= x <= px(start) + px(duration)` and the lane holds y. `tpp` is passed so
    /// paint and hit test use one row's values (Q3).
    #[inline]
    fn occ_hit(occ: &Occ, tpp: TimeSpan, row_top: i32, row_height: i32, p: (i32, i32)) -> bool {
        let x = px(TimeSpan(occ.start), tpp);
        let w = px(TimeSpan(occ.end - occ.start), tpp);
        if (p.0 as f64) < x || (p.0 as f64) > x + w {
            return false;
        }
        let (h, offset, _) = lane_geometry(occ.slot(), row_height);
        let top = row_top + offset;
        p.1 >= top && p.1 < top + h
    }

    /// First occurrence that could reach a canvas x: anything earlier ended at least a pixel before it.
    #[inline]
    fn first_reaching(&self, r: RowId, x: i32) -> usize {
        let row = self.rows.row(r);
        let t = (x as i64 - 1) * self.tpp().ticks();
        row.prefix_max_end.partition_point(|&m| m < t)
    }

    /// `elementsAt`.
    pub fn elements_at(&self, p: (i32, i32)) -> Vec<ElementId> {
        let mut out = Vec::new();
        let Some(r) = self.rows.row_at(p.1) else { return out };
        let Some(top) = self.rows.row_top(r) else { return out };
        let row = self.rows.row(r);
        let tpp = self.tpp();
        for occ in &row.occurrences[self.first_reaching(r, p.0)..] {
            if px(TimeSpan(occ.start), tpp) > p.0 as f64 {
                break;
            }
            if Self::occ_hit(occ, tpp, top, row.height, p) {
                out.push(occ.id);
            }
        }
        out
    }

    /// `elementAt`: the first match.
    pub fn element_at(&self, p: (i32, i32)) -> Option<ElementId> {
        let r = self.rows.row_at(p.1)?;
        let top = self.rows.row_top(r)?;
        let row = self.rows.row(r);
        let tpp = self.tpp();
        for occ in &row.occurrences[self.first_reaching(r, p.0)..] {
            if px(TimeSpan(occ.start), tpp) > p.0 as f64 {
                break;
            }
            if Self::occ_hit(occ, tpp, top, row.height, p) {
                return Some(occ.id);
            }
        }
        None
    }

    /// `ElementsAtTime`: visible rows, `start <= t < end`, not distinct.
    pub fn elements_at_time(&self, t: TimeSpan) -> Vec<ElementId> {
        let mut out = Vec::new();
        for r in self.rows.visible() {
            for occ in &self.rows.row(*r).occurrences {
                if occ.start > t.ticks() {
                    break;
                }
                if t.ticks() < occ.end {
                    out.push(occ.id);
                }
            }
        }
        out
    }

    /// `ElementsWithinRange`: start in the inclusive window, distinct, in row order.
    pub fn elements_within(&self, start: TimeSpan, end: TimeSpan, visible_only: bool) -> Vec<ElementId> {
        let mut seen: HashSet<ElementId, BuildHasherDefault<IdHasher>> = HashSet::default();
        let mut out = Vec::new();
        let mut scan = |row: &super::Row| {
            let from = row.occurrences.partition_point(|o| o.start < start.ticks());
            for occ in &row.occurrences[from..] {
                if occ.start > end.ticks() {
                    break;
                }
                if seen.insert(occ.id) {
                    out.push(occ.id);
                }
            }
        };
        if visible_only {
            for r in self.rows.visible() {
                scan(self.rows.row(*r));
            }
        } else {
            for row in &self.rows.rows {
                scan(row);
            }
        }
        out
    }

    // ------------------------------------------------------------------------------------------------------------------
    // Selection
    // ------------------------------------------------------------------------------------------------------------------

    /// `Element.Selected`.
    pub(crate) fn set_selected(&mut self, id: ElementId, selected: bool) {
        let Timeline { elements, rows, selected_cache, dirty, .. } = self;
        let Some(e) = elements.get_mut(&id) else { return };
        if e.selected == selected {
            return;
        }
        e.selected = selected;
        for r in &e.rows {
            let list = &mut rows.row_mut(*r).selected_elements;
            if selected {
                list.push(id);
            } else if let Some(at) = list.iter().position(|s| *s == id) {
                list.remove(at);
            }
        }
        *selected_cache = None;
        dirty.overlay = true;
    }

    /// F57: rows top to bottom, within a row in the order selected, distinct. Always authoritative (FIX Q6).
    pub fn selected_elements(&mut self) -> &[ElementId] {
        if self.selected_cache.is_none() {
            let mut seen: HashSet<ElementId, BuildHasherDefault<IdHasher>> = HashSet::default();
            let mut list = Vec::new();
            for row in &self.rows.rows {
                for id in &row.selected_elements {
                    if seen.insert(*id) {
                        list.push(*id);
                    }
                }
            }
            self.selected_cache = Some(list);
        }
        self.selected_cache.as_deref().unwrap_or(&[])
    }

    /// A copy of the selected list, for code that goes on to change things.
    pub fn selected_list(&mut self) -> Vec<ElementId> {
        self.selected_elements().to_vec()
    }

    /// `_SelectionChanged`.
    pub(crate) fn raise_selection_changed(&mut self) {
        self.selection_changed = true;
        self.dirty.overlay = true;
    }

    /// `ClearSelectedElements`, without the event, which the caller raises or suppresses.
    pub(crate) fn clear_selected_quietly(&mut self) {
        for row in &mut self.rows.rows {
            row.selected_elements.clear();
        }
        let list = self.selected_cache.take();
        match list {
            Some(ids) => {
                for id in ids {
                    if let Some(e) = self.elements.get_mut(&id) {
                        e.selected = false;
                    }
                }
            }
            None => {
                for e in self.elements.values_mut() {
                    e.selected = false;
                }
            }
        }
        self.selected_cache = Some(Vec::new());
        self.dirty.overlay = true;
    }

    /// `ClearSelectedElements`.
    pub fn clear_selection(&mut self) {
        self.clear_selected_quietly();
        self.raise_selection_changed();
    }

    /// `ClearSelectedRows`.
    pub(crate) fn clear_selected_rows(&mut self, leave: Option<RowId>) {
        let mut changed = Vec::new();
        for row in &mut self.rows.rows {
            if Some(row.id) != leave && row.selected {
                row.selected = false;
                changed.push(row.id);
            }
        }
        for r in changed {
            self.dirty_row(r);
        }
    }

    /// `ClearActiveRows`.
    pub(crate) fn clear_active_rows(&mut self, leave: Option<RowId>) {
        let mut changed = Vec::new();
        for row in &mut self.rows.rows {
            if Some(row.id) != leave && row.active {
                row.active = false;
                changed.push(row.id);
            }
        }
        for r in changed {
            self.dirty_row(r);
        }
    }

    pub(crate) fn set_row_active(&mut self, r: RowId, active: bool) {
        if self.rows.row(r).active != active {
            self.rows.row_mut(r).active = active;
            self.dirty_row(r);
        }
    }

    pub(crate) fn set_row_selected(&mut self, r: RowId, selected: bool) {
        if self.rows.row(r).selected != selected {
            self.rows.row_mut(r).selected = selected;
            self.dirty_row(r);
        }
    }

    /// A whole row band. The highlight spans the viewport wherever it scrolls to, so all tiles across it are affected.
    pub(crate) fn dirty_row(&mut self, r: RowId) {
        self.dirty.row_header = true;
        if let Some(top) = self.rows.row_top(r) {
            let height = self.rows.row(r).height;
            self.dirty.grid_rects.push(IRect::new(i32::MIN / 2, top, i32::MAX, height));
        }
    }

    /// `ActiveRow`: the first with the flag.
    pub fn active_row(&self) -> Option<RowId> {
        self.rows.rows.iter().find(|r| r.active).map(|r| r.id)
    }

    /// `TopVisibleRow`.
    pub fn top_visible_row(&self) -> Option<RowId> {
        self.rows.row_at(self.vertical_offset)
    }

    /// `SelectedRow`: the first selected in flat order.
    pub fn selected_row(&self) -> Option<RowId> {
        self.rows.rows.iter().find(|r| r.selected).map(|r| r.id)
    }

    /// F27, paste: `SelectedRow ?? ActiveRow ?? TopVisibleRow`. The paste time is 0 if that row is selected, otherwise the edit cursor.
    pub fn paste_target_row(&self) -> Option<RowId> {
        self.selected_row().or_else(|| self.active_row()).or_else(|| self.top_visible_row())
    }

    /// F27, Edit > Add Effect: `ActiveRow ?? SelectedRow`, the other way round, and nothing happens with neither.
    pub fn add_effect_target_row(&self) -> Option<RowId> {
        self.active_row().or_else(|| self.selected_row())
    }

    /// `MoveCursorForSelectedEffects` with the last action element as target.
    pub(crate) fn move_cursor_to_last_of(&mut self, action: &[ElementId]) {
        if self.opts.legacy_cursor {
            return;
        }
        let Some(start) = action.last().and_then(|id| self.elements.get(id)).map(|e| e.start) else { return };
        self.set_cursor(start);
    }

    /// `MoveCursorForSelectedEffects` with the earliest selected as target. Nothing moves if there was no action on anything.
    pub(crate) fn move_cursor_to_earliest(&mut self, action: &[ElementId]) {
        if self.opts.legacy_cursor || action.is_empty() {
            return;
        }
        let earliest = self.selected_list().iter().filter_map(|id| self.elements.get(id)).map(|e| e.start).min();
        if let Some(earliest) = earliest {
            self.set_cursor(earliest);
        }
    }

    /// F91. Moves only its own layer. Setting it always raises the moved event, even to the same value.
    pub fn set_cursor(&mut self, t: TimeSpan) {
        self.cursor_moved = true;
        if self.cursor != t {
            self.cursor = t;
            self.dirty.overlay = true;
        }
    }

    /// `SelectElement`.
    pub fn select_element(&mut self, id: ElementId) {
        self.set_selected(id, true);
        self.raise_selection_changed();
        self.move_cursor_to_last_of(&[id]);
    }

    /// `SelectElements`: the cursor goes to the earliest selected. An empty list moves nothing.
    pub fn select_elements(&mut self, ids: &[ElementId]) {
        for id in ids {
            self.set_selected(*id, true);
        }
        self.raise_selection_changed();
        self.move_cursor_to_earliest(ids);
    }

    /// `DeselectElement`.
    pub fn deselect_element(&mut self, id: ElementId) {
        self.set_selected(id, false);
        self.raise_selection_changed();
    }

    /// `ToggleElementSelection`.
    pub fn toggle_element(&mut self, id: ElementId) {
        let selected = self.elements.get(&id).is_some_and(|e| e.selected);
        self.set_selected(id, !selected);
        self.raise_selection_changed();
    }

    /// F56: every row selects all of its effects. FIX Q6: this raises selection changed, so commands don't act on a stale list.
    pub fn select_all(&mut self) {
        let mut seen = Vec::new();
        for row in &self.rows.rows {
            for occ in &row.occurrences {
                seen.push(occ.id);
            }
        }
        for id in seen {
            self.set_selected(id, true);
        }
        self.raise_selection_changed();
    }

    /// `Grid.SelectedEffect`: what the Effects panel has chosen for drawing. F150: deselecting one puts the pointer back to an arrow.
    pub fn set_selected_effect(&mut self, effect: Option<Uuid>) {
        self.opts.selected_effect = effect;
        if effect.is_none() {
            self.cursor_kind = CursorKind::Arrow;
        }
    }

    /// F155: what moves, resizes, edit text, phonemes, cut, copy and delete act on, in the order selected.
    pub fn mark_selection(&self) -> Vec<Uuid> {
        self.marks.selected.clone()
    }

    /// F53.
    pub fn set_drag_box_filter(&mut self, filter: DragBoxFilter) {
        self.opts.drag_box_filter = filter;
    }

    // ------------------------------------------------------------------------------------------------------------------
    // Frames
    // ------------------------------------------------------------------------------------------------------------------

    /// F10 and F39 for a rectangle of the canvas: a tile or the viewport. Rows and occurrences outside it aren't touched.
    pub fn grid_frame(&mut self, rect: IRect) -> GridFrame {
        self.grid_frame_at(rect, self.tpp())
    }

    /// The same at a given zoom: a set of tiles is painted for one, which a zoom that's still running will have moved on from by the time they're done.
    pub fn grid_frame_at(&mut self, rect: IRect, tpp: TimeSpan) -> GridFrame {
        self.flush_rows();
        let tpp_ticks = tpp.ticks().max(1);
        let canvas_height = self.rows.canvas_height();
        let t0 = rect.x as i64 * tpp_ticks;
        let t1 = rect.right() as i64 * tpp_ticks;
        let mut frame = GridFrame { canvas_width: px(self.info.total, tpp) as i32, canvas_height, ..GridFrame::default() };

        let first = self.rows.row_tops()[1..].partition_point(|&b| b <= rect.y);
        for i in first..self.rows.visible().len() {
            let top = self.rows.row_tops()[i];
            if top >= rect.bottom() {
                break;
            }
            let r = self.rows.visible()[i];
            self.ensure_slots(r, t0 - tpp_ticks, t1 + tpp_ticks);
            let row = self.rows.row(r);
            frame.rows.push(RowQuad { id: r, top, height: row.height, selected: row.selected, active: row.active });
            let from = row.prefix_max_end.partition_point(|&m| m < t0 - tpp_ticks);
            for occ in &row.occurrences[from..] {
                let x = px(TimeSpan(occ.start), tpp).floor() as i32;
                if x >= rect.right() {
                    break;
                }
                if occ.moving {
                    continue;
                }
                let w = px(TimeSpan(occ.end - occ.start), tpp) as i32;
                if w <= 0 || x + w < rect.x {
                    continue;
                }
                let Some(e) = self.elements.get(&occ.id) else { continue };
                let (h, offset, red) = lane_geometry(occ.slot(), row.height);
                frame.elements.push(ElementQuad {
                    id: occ.id,
                    row: r,
                    x,
                    y: top + offset,
                    w,
                    h,
                    selected: e.selected,
                    rendered: e.rendered,
                    red_border: red,
                    moving: false,
                    picture: e.picture,
                });
            }
        }

        // F9. KEEP-ish: upstream loops in sub-pixel steps when zoomed far out. Under 2 points apart it would be a solid wash, so skip it.
        let interval = 10_000_000i64;
        let step = interval as f64 / tpp_ticks as f64;
        if step >= 2.0 {
            let mut k = (t0 / interval).max(1);
            loop {
                let x = k as f64 * step;
                if x >= rect.right() as f64 {
                    break;
                }
                if x >= rect.x as f64 - 1.0 && k as f64 * interval as f64 <= self.info.total.ticks() as f64 {
                    frame.gridlines.push(x);
                }
                k += 1;
            }
        }

        // Wider than the rectangle by the width of a bold line. Lines of marks being dragged are lifted, like effects.
        let lines = mark_lines(&self.snap_index, TimeSpan(t0 - 3 * tpp_ticks), TimeSpan(t1 + 3 * tpp_ticks), &self.guides, tpp);
        frame.mark_lines = if self.marks.moving_ids_empty() { lines } else { self.without_moving_marks(lines) };
        frame.mark_lines = self.studio_tile_mark_lines(std::mem::take(&mut frame.mark_lines)); // STUDIO: only selected marks' collections cross the tiles
        frame
    }

    /// Selected or lifted occurrences near the viewport, for the layers that draw them.
    pub fn overlay_occurrences(&mut self, cover: IRect) -> Vec<ElementQuad> {
        self.flush_rows();
        let tpp = self.tpp();
        let mut out = Vec::new();
        let first = self.rows.row_tops()[1..].partition_point(|&b| b <= cover.y);
        let adjoining = self.g.adjoining_ids().to_vec();
        let (t0, t1) = (cover.x as i64 * tpp.ticks(), cover.right() as i64 * tpp.ticks());
        for i in first..self.rows.visible().len() {
            let top = self.rows.row_tops()[i];
            if top >= cover.bottom() {
                break;
            }
            let r = self.rows.visible()[i];
            if self.rows.row(r).selected_elements.is_empty() && adjoining.is_empty() {
                continue;
            }
            self.ensure_slots(r, t0, t1);
            let row = self.rows.row(r);
            let push = |id: &ElementId, out: &mut Vec<ElementQuad>| {
                let Some(e) = self.elements.get(id) else { return };
                let Some(at) = row.find_occ(*id, e.start.ticks(), e.end().ticks()) else { return };
                let occ = &row.occurrences[at];
                let x = px(TimeSpan(occ.start), tpp).floor() as i32;
                let w = px(TimeSpan(occ.end - occ.start), tpp) as i32;
                if w <= 0 || x >= cover.right() || x + w < cover.x {
                    return;
                }
                let (h, offset, red) = lane_geometry(occ.slot(), row.height);
                out.push(ElementQuad {
                    id: *id,
                    row: r,
                    x,
                    y: top + offset,
                    w,
                    h,
                    selected: e.selected,
                    rendered: e.rendered,
                    red_border: red,
                    moving: occ.moving,
                    picture: e.picture,
                });
            };
            for id in &row.selected_elements {
                push(id, &mut out);
            }
            for id in &adjoining {
                let selected = self.elements.get(id).is_some_and(|e| e.selected);
                if !selected && self.elements.get(id).is_some_and(|e| e.rows.binary_search(&r).is_ok()) {
                    push(id, &mut out);
                }
            }
        }
        out
    }

    /// Everything that moves without touching a tile. `cover` is the canvas rectangle the layers extend over.
    pub fn overlay_frame(&mut self, cover: IRect) -> OverlayFrame {
        self.refresh_view_stacks();
        let tpp = self.tpp();
        let guides: Vec<f64> = self.guides.iter().map(|t| px(*t, tpp)).collect();
        let dragging = self.g.drag_state != super::DragState::Normal;
        OverlayFrame {
            occurrences: self.overlay_occurrences(cover),
            rubber_band: (!self.g.selection_area.is_empty()).then_some(self.g.selection_area),
            selection_green: self.selection_green,
            draw_box: (!self.g.drawing_area.is_empty()).then_some(self.g.drawing_area),
            indicator_lines: self.resize_indicator_lines(),
            indicator_color: self.opts.resize_indicator_color.color(),
            grid_guides: if dragging { Vec::new() } else { guides.clone() },
            guides,
            moving_mark_lines: self.moving_mark_lines(),
            hover: self.hover_info(),
            cursor_x: px(self.cursor, tpp),
        }
    }

    /// `DrawInfo`'s text and rectangle for the first effect under the pointer. F45.
    pub fn hover_info(&self) -> Option<HoverInfo> {
        if !self.opts.show_effect_info {
            return None;
        }
        let id = *self.g.captured.first()?;
        let e = self.elements.get(&id)?;
        let r = self.rows.row_at(self.g.last_grid_location.1)?;
        let top = self.rows.row_top(r)?;
        let row = self.rows.row(r);
        let rect = occurrence_rect(e.times(), super::StackSlot { index: 0, count: 1 }, top, row.height, &self.info, self.vis_end());
        let text = hover_info_text(&e.desc.type_name, &e.desc.layer_name, e.times());
        Some(HoverInfo { text, rect: IRect::new(rect.x, top, rect.w, row.height) })
    }

    /// Takes everything that changed, as one outcome.
    pub(crate) fn finish(&mut self) -> InputOutcome {
        self.refresh_view_stacks();
        self.refresh_marks_layout();
        // A new drawn interval moves every label.
        let interval = self.current_label_interval();
        if interval != self.marks.ruler.painted_interval {
            self.marks.ruler.painted_interval = interval;
            self.dirty.ruler = true;
        }
        // `TimelineControl_TimePerPixelChanged` and `VisibleTimeStartChanged`, which only count a value that differs from the sequence's copy.
        let view = (self.info.time_per_pixel, self.info.visible_start);
        if view != self.document_view {
            self.document_view = view;
            self.mark_editor_state_modified();
        }
        let dirty = std::mem::take(&mut self.dirty);
        InputOutcome {
            cursor: self.cursor_kind,
            dirty,
            selection_changed: std::mem::take(&mut self.selection_changed),
            cursor_moved: std::mem::take(&mut self.cursor_moved),
            autoscroll: self.g.autoscroll || self.marks.autoscroll,
            requests: std::mem::take(&mut self.requests),
            editor_state_modified: std::mem::take(&mut self.editor_state_note),
        }
    }

    /// The same, for code outside an input call.
    pub fn take_outcome(&mut self) -> InputOutcome {
        self.finish()
    }

    /// A rectangle for one occurrence with the view's clipping, for code that wants upstream's numbers.
    pub fn occurrence_rect_in_view(&self, id: ElementId, r: RowId) -> Option<OccurrenceRect> {
        let e = self.elements.get(&id)?;
        let top = self.rows.row_top(r)?;
        let row = self.rows.row(r);
        let at = row.find_occ(id, e.start.ticks(), e.end().ticks())?;
        Some(occurrence_rect(e.times(), row.occurrences[at].slot(), top, row.height, &self.info, self.vis_end()))
    }

    /// `SelectionBorder`: green after a CAD enclosed rubber band, until the next rubber band update. KEEP. It also outlines selected rows (F26).
    pub fn overlay_selection_green(&self) -> bool {
        self.selection_green
    }

    /// `SnapStrength`. Rebuilds mark snap points (F68).
    pub fn set_snap_strength(&mut self, strength: i32) {
        if self.opts.snap_strength != strength {
            self.opts.snap_strength = strength;
            self.snap_index.rebuild(&self.marks.collections, strength);
        }
    }

    /// F23.
    pub fn set_highlight_rows(&mut self, on: bool) {
        if self.opts.highlight_rows != on {
            self.opts.highlight_rows = on;
            self.dirty.row_header = true;
        }
    }

    /// F113's restore.
    pub fn set_ruler_height(&mut self, height: i32) {
        if self.marks.ruler.height != height {
            self.marks.ruler.height = height;
            self.dirty.view = true;
            self.dirty.ruler = true;
        }
    }

    /// F145's restore.
    pub fn set_waveform_height(&mut self, height: i32) {
        if self.waveform.height != height {
            self.waveform.height = height;
            self.dirty.view = true;
            self.dirty.waveform = true;
        }
    }

    /// The ruler's range, from the sequencer's Play and its remembered values. F105.
    pub fn set_playback_range(&mut self, start: Option<TimeSpan>, end: Option<TimeSpan>) {
        if (self.info.playback_start, self.info.playback_end) != (start, end) {
            self.info.playback_start = start;
            self.info.playback_end = end;
            self.dirty.ruler = true;
        }
    }

    /// A problem for whoever shows them. Upstream writes these to its log.
    pub fn report_problem(&mut self, message: String) {
        self.requests.push(HostRequest::Warning { text: message, title: "Error".to_string() });
    }

    /// `time(px)` at the current zoom.
    #[inline]
    pub fn time(&self, px: i32) -> TimeSpan {
        time_at(px, self.tpp())
    }
}

/// F45's exact form, in which the lines after the first start with a space.
pub fn hover_info_text(name: &str, layer: &str, times: ElementTimes) -> String {
    format!(
        "{name} \r\n Layer: {layer} \r\n Start: {} \r\n End: {} \r\n Length: {}",
        format_m_ss_fff(times.start),
        format_m_ss_fff(times.end()),
        format_m_ss_fff(times.duration)
    )
}

/// F151, FIX Q45: whether a frame failed, so the one alert shows once, and not again until a frame has come out right.
#[derive(Debug, Default)]
pub struct DrawFailures {
    state: std::sync::atomic::AtomicU8,
}

impl DrawFailures {
    const NONE: u8 = 0;
    const WAITING: u8 = 1;
    const SHOWN: u8 = 2;

    /// What every failure shows, whichever surface it was on.
    pub const TEXT: &'static str = "An unexpected error occurred while drawing the grid. Please notify the Vixen team and provide the error logs.";

    pub fn frame_succeeded(&self) {
        use std::sync::atomic::Ordering::Relaxed;
        if self.state.load(Relaxed) != Self::NONE {
            self.state.store(Self::NONE, Relaxed);
        }
    }

    pub fn frame_failed(&self) {
        use std::sync::atomic::Ordering::Relaxed;
        let _ = self.state.compare_exchange(Self::NONE, Self::WAITING, Relaxed, Relaxed);
    }

    /// The text, once per run of failures.
    pub fn take(&self) -> Option<String> {
        use std::sync::atomic::Ordering::Relaxed;
        self.state.compare_exchange(Self::WAITING, Self::SHOWN, Relaxed, Relaxed).ok().map(|_| Self::TEXT.to_string())
    }
}

/// `TimeSpan.ToString(@"m\:ss\.fff")`: the minutes component, not the total, as upstream does.
pub fn format_m_ss_fff(t: TimeSpan) -> String {
    let negative = t.ticks() < 0;
    let total_ms = t.ticks().abs() / 10_000;
    let minutes = (total_ms / 60_000) % 60;
    let seconds = (total_ms / 1000) % 60;
    let millis = total_ms % 1000;
    format!("{}{minutes}:{seconds:02}.{millis:03}", if negative { "-" } else { "" })
}

/// F45's box position, from the rectangle, the text's measured size and the visible clip's origin. Swift measures; this decides.
pub fn hover_box_origin(rect: IRect, text_h: i32, clip_x: i32, clip_y: i32) -> (i32, i32) {
    const MARGIN: i32 = 2;
    let box_h = text_h + MARGIN;
    let mut x = rect.x;
    let mut y = rect.y;
    if rect.y - box_h < clip_y {
        y += rect.h + MARGIN - 8;
    } else {
        y -= text_h + MARGIN - 4;
    }
    if clip_x > x {
        x = clip_x + 5;
    }
    (x, y)
}
