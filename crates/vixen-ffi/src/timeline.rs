//! The timeline's bindings. One `TimelineController` per sequencer window, holding the window's `SequencerSession`. Results come back in
//! bulk, once per redraw, and never a call per row. Everything that changes state returns a `TimelineUpdate` saying what to redraw.
//!
//! Locks: the timeline's first, then the session's, and the session's only when something is committed. A drag holds no session lock, so
//! it can't wait on a render. Frame and picture calls are safe from any thread; the pictures take the session's lock just long enough to
//! copy what they read.

use crate::sequence::SequencerSession;
use parking_lot::{Mutex, MutexGuard};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use uuid::Uuid;
use vixen_core::{Color, NodeId, TimeSpan};
use vixen_io::{ProfileSettings, SettingSection};
use vixen_session::sequence::{EffectId, SequenceSession};
use vixen_session::timeline::{AlignKind, ScrollEvent, Surface};
use vixen_session::timeline::host::TimelineHost;
use vixen_session::timeline::marks::{AddMarkResult, MarksMenuModel, MismatchInfo, PHONEMES};
use vixen_session::timeline::model::{DrawFailures, HostRequest, IndicatorColor, InputOutcome, OverlayFrame, Timeline};
use vixen_session::timeline::row_input::{TagCheck, TagDisplay};
use vixen_session::timeline::ruler::LabelMetrics;
use vixen_session::timeline::thumb::{picture_source, render_picture};
use vixen_session::timeline::waveform::WaveformStyle;
use vixen_session::timeline::{Button, CursorKind, DragState, ElementId, IRect, Mods, PointerEvent, RowId};
use vixen_session::undo::{EffectSnapshot, UndoAction};

// STUDIO-BEGIN timeline-access: VoiceOver elements and keyboard moves for the grid (R43)
#[path = "studio_timeline_access.rs"]
mod studio_timeline_access;
// STUDIO-END timeline-access

/// The profile setting keys are under this. `docs/spec/timeline.md`, Data and persistence.
const FORM: &str = "TimedSequenceEditorForm";

// ----------------------------------------------------------------------------------------------------------------------
// Records
// ----------------------------------------------------------------------------------------------------------------------

// STUDIO-BEGIN row-height-record: what View ▸ Row Height shows (R36)
#[derive(uniffi::Record, Clone)]
pub struct TimelineStudioRowHeight {
    /// "compact" or "regular".
    pub mode: String,
    pub height_pt: i32,
    pub rows_in_view: u32,
}
// STUDIO-END row-height-record

// STUDIO-BEGIN agent-flash-record: agent edits flash on the timeline (R39)
#[derive(uniffi::Record, Clone)]
pub struct TimelineStudioAgentFlash {
    /// Ask from this next time.
    pub revision: u64,
    /// Canvas rectangles of the effects agents added or changed since the revision asked from.
    pub rects: Vec<TimelineRect>,
    pub effects: u32,
}
// STUDIO-END agent-flash-record

#[derive(uniffi::Record, Copy, Clone)]
pub struct TimelineMods {
    pub shift: bool,
    pub cmd: bool,
    pub opt: bool,
}

impl From<TimelineMods> for Mods {
    fn from(m: TimelineMods) -> Mods {
        Mods { shift: m.shift, cmd: m.cmd, opt: m.opt }
    }
}

#[derive(uniffi::Enum, Copy, Clone)]
pub enum TimelineButton {
    Left,
    Right,
    Middle,
}

/// One pointer event, in the surface's client coordinates.
#[derive(uniffi::Record, Copy, Clone)]
pub struct TimelinePointer {
    pub x: i32,
    pub y: i32,
    pub button: Option<TimelineButton>,
    pub clicks: u8,
    pub mods: TimelineMods,
}

impl From<TimelinePointer> for PointerEvent {
    fn from(p: TimelinePointer) -> PointerEvent {
        PointerEvent {
            x: p.x,
            y: p.y,
            button: p.button.map(|b| match b {
                TimelineButton::Left => Button::Left,
                TimelineButton::Right => Button::Right,
                TimelineButton::Middle => Button::Middle,
            }),
            clicks: p.clicks,
            mods: p.mods.into(),
        }
    }
}

#[derive(uniffi::Enum, Copy, Clone, PartialEq, Eq)]
pub enum TimelineCursorKind {
    Arrow,
    ResizeLeftRight,
    ResizeUpDown,
    Crosshair,
    Move,
    Hand,
}

impl From<CursorKind> for TimelineCursorKind {
    fn from(c: CursorKind) -> TimelineCursorKind {
        match c {
            CursorKind::Arrow => TimelineCursorKind::Arrow,
            CursorKind::ResizeLeftRight => TimelineCursorKind::ResizeLeftRight,
            CursorKind::ResizeUpDown => TimelineCursorKind::ResizeUpDown,
            CursorKind::Crosshair => TimelineCursorKind::Crosshair,
            CursorKind::Move => TimelineCursorKind::Move,
            CursorKind::Hand => TimelineCursorKind::Hand,
        }
    }
}

#[derive(uniffi::Record, Copy, Clone)]
pub struct TimelineRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl From<IRect> for TimelineRect {
    fn from(r: IRect) -> TimelineRect {
        TimelineRect { x: r.x, y: r.y, w: r.w, h: r.h }
    }
}

/// Where everything is. Comes with every update, so a surface needs no further call to scroll.
#[derive(uniffi::Record, Clone)]
pub struct TimelineView {
    pub time_per_pixel: i64,
    pub visible_start: i64,
    pub total: i64,
    /// `px(VisibleTimeStart)`.
    pub scroll_x: f64,
    pub vertical_offset: i32,
    pub canvas_width: i32,
    pub canvas_height: i32,
    /// 0 with no audio.
    pub waveform_height: i32,
    pub ruler_height: i32,
    pub marks_bar_height: i32,
    pub cursor_x: f64,
    pub cursor: i64,
    pub dragging: bool,
    /// F142: the audio is there and its peaks aren't yet.
    pub waveform_building: bool,
    /// F141, F142: the strip shows no guides or cursor now.
    pub waveform_blank: bool,
}

/// Something a surface wants from the window around it.
#[derive(uniffi::Enum, Clone)]
pub enum TimelineRequest {
    /// Offer these in a menu at the point, then `finish_chooser` with what was picked, or nothing. F50.
    ChooseOverlapping { items: Vec<TimelineMenuItem>, x: i32, y: i32 },
    /// The effect context menu is the sequencer's. F54.
    ContextMenu { elements: Vec<u64>, time: i64, row: u32 },
    /// F33.
    RowContextMenu { row: u32 },
    Warning { text: String, title: String },
    /// An entry went on the undo stack, so the window's `UndoManager` should hear of it.
    UndoRecorded,
    ElementDoubleClicked { id: u64 },
    /// Play from here, to `end` or to the end of the sequence.
    PlayRange { start: i64, end: Option<i64> },
    /// The ruler's one-item mark menu. F112.
    MarkMenu,
    /// Show `marks_context_menu()` at the pointer. F130.
    MarksContextMenu { press_x: i32, press_y: i32 },
    /// Ask, then `create_default_mark_collection` and `add_mark_at` again. F111.
    NeedsNewCollection { time: i64, fill_gap: bool, label: Option<String> },
    /// Ask, then `enable_default_mark_collection` and `add_mark_at` again. F111.
    NeedsEnableDefault { time: i64, fill_gap: bool, label: Option<String> },
    /// Ask for the text, then `add_mark_at`. F111.
    PromptMarkLabel { time: i64, fill_gap: bool },
}

#[derive(uniffi::Record, Clone)]
pub struct TimelineMenuItem {
    pub id: u64,
    pub label: String,
}

/// What changed. Apply it and draw only that.
#[derive(uniffi::Record, Clone)]
pub struct TimelineUpdate {
    pub view: TimelineView,
    pub cursor_kind: TimelineCursorKind,
    pub autoscroll: bool,
    pub selection_changed: bool,
    pub cursor_moved: bool,
    pub grid_all: bool,
    pub grid_rects: Vec<TimelineRect>,
    pub row_header: bool,
    pub ruler: bool,
    pub marks_bar: bool,
    pub waveform: bool,
    pub overlay: bool,
    pub view_changed: bool,
    pub requests: Vec<TimelineRequest>,
    /// F153: the editor state flag was just set, so the window should remember to save silently at close.
    pub editor_state_modified: bool,
}

#[derive(uniffi::Record, Clone)]
pub struct TimelineRowQuad {
    pub top: i32,
    pub height: i32,
    pub selected: bool,
    pub active: bool,
}

#[derive(uniffi::Record, Clone, Copy)]
pub struct TimelineElementQuad {
    pub id: u64,
    pub row: u32,
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

#[derive(uniffi::Record, Clone, Copy)]
pub struct TimelineLine {
    pub x: f64,
    /// ARGB.
    pub color: u32,
    pub width: i32,
    /// `[dash, dash]`, or 0 for solid.
    pub dash: i32,
}

/// One rectangle of the grid: F10.
#[derive(uniffi::Record, Clone)]
pub struct TimelineGridFrame {
    pub rows: Vec<TimelineRowQuad>,
    pub elements: Vec<TimelineElementQuad>,
    pub mark_lines: Vec<TimelineLine>,
    pub gridlines: Vec<f64>,
    pub selection_green: bool,
    /// F151: leave what's there.
    pub failed: bool,
}

#[derive(uniffi::Record, Clone)]
pub struct TimelineHover {
    pub text: String,
    pub rect: TimelineRect,
}

/// What moves without touching a tile.
#[derive(uniffi::Record, Clone)]
pub struct TimelineOverlay {
    pub occurrences: Vec<TimelineElementQuad>,
    pub rubber_band: Option<TimelineRect>,
    pub selection_green: bool,
    pub draw_box: Option<TimelineRect>,
    pub indicator_lines: Vec<f64>,
    pub indicator_color: u32,
    pub grid_guides: Vec<f64>,
    pub guides: Vec<f64>,
    pub moving_mark_lines: Vec<TimelineLine>,
    pub snapping_mark_lines: Vec<TimelineLine>, // STUDIO: every collection's lines while an effect drag snaps
    pub hover: Option<TimelineHover>,
    pub cursor_x: f64,
    /// The time beside the pointer while a mark drags on the ruler, and where.
    pub ruler_drag_x: Option<f64>,
    pub ruler_drag_text: Option<String>,
    pub playback_x: Option<f64>,
}

#[derive(uniffi::Record, Clone)]
pub struct TimelineRowDisplay {
    pub id: u32,
    pub name: String,
    pub depth: i32,
    pub height: i32,
    pub top: i32,
    pub expandable: bool,
    pub open: bool,
    /// Yellow text, with Highlight Rows With Effects on.
    pub highlighted: bool,
    /// The expander's active look.
    pub child_highlighted: bool,
    pub selected: bool,
    pub active: bool,
    pub tag_colors: Vec<u32>,
}

#[derive(uniffi::Record, Clone, Copy)]
pub struct TimelineConnector {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

/// A span of the row list.
#[derive(uniffi::Record, Clone)]
pub struct TimelineRowFrame {
    pub rows: Vec<TimelineRowDisplay>,
    pub connectors: Vec<TimelineConnector>,
    /// F151: leave what's there.
    pub failed: bool,
}

#[derive(uniffi::Record, Clone)]
pub struct TimelineRulerLabel {
    pub centre: f64,
    pub text: String,
}

/// A span of the ruler.
#[derive(uniffi::Record, Clone)]
pub struct TimelineRulerFrame {
    pub major_ticks: Vec<f64>,
    pub minor_ticks: Vec<f64>,
    pub labels: Vec<TimelineRulerLabel>,
    pub mark_lines: Vec<TimelineLine>,
    pub total_px: f64,
    pub range_start: Option<i32>,
    pub range_end: Option<i32>,
    pub height: i32,
    /// F151: leave what's there.
    pub failed: bool,
}

#[derive(uniffi::Record, Clone)]
pub struct TimelineMarkQuad {
    pub id: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub color: u32,
    pub hatch: Option<u32>,
    pub selected: bool,
    pub text: String,
    pub text_color: u32,
}

/// A span of the marks bar.
#[derive(uniffi::Record, Clone)]
pub struct TimelineMarksFrame {
    pub total_height: i32,
    pub row_bottoms: Vec<i32>,
    pub marks: Vec<TimelineMarkQuad>,
    /// F151: leave what's there.
    pub failed: bool,
}

/// A span of the waveform.
#[derive(uniffi::Record, Clone)]
pub struct TimelineWaveformFrame {
    pub building: bool,
    pub first_column: i32,
    pub top: i32,
    pub baseline: i32,
    pub full: bool,
    pub highs: Vec<f32>,
    pub lows: Vec<f32>,
    pub height: i32,
    /// F151: leave what's there.
    pub failed: bool,
}

/// An effect's picture: premultiplied RGBA, top row first.
#[derive(uniffi::Record, Clone)]
pub struct TimelinePicture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub picture: u64,
}

#[derive(uniffi::Record, Clone, Copy)]
pub struct TimelineMarksMenu {
    pub edit_text: bool,
    pub cut: bool,
    pub copy: bool,
    pub paste: bool,
    pub delete: bool,
    pub phoneme: bool,
    pub breakdown_phrase: bool,
    pub breakdown_word: bool,
}

impl From<MarksMenuModel> for TimelineMarksMenu {
    fn from(m: MarksMenuModel) -> TimelineMarksMenu {
        TimelineMarksMenu {
            edit_text: m.edit_text,
            cut: m.cut,
            copy: m.copy,
            paste: m.paste,
            delete: m.delete,
            phoneme: m.phoneme,
            breakdown_phrase: m.breakdown_phrase,
            breakdown_word: m.breakdown_word,
        }
    }
}

#[derive(uniffi::Record, Clone)]
pub struct TimelineTextPrompt {
    pub title: String,
    pub text: String,
    pub select_all: bool,
}

#[derive(uniffi::Enum, Clone, Copy, PartialEq, Eq)]
pub enum TimelineAlign {
    Start,
    End,
    Both,
    Durations,
    StartToEnd,
    EndToStart,
    Centers,
}

#[derive(uniffi::Enum, Clone, Copy, PartialEq, Eq)]
pub enum TimelineSurface {
    Grid,
    Ruler,
    Waveform,
    RowList,
    MarksBar,
}

#[derive(uniffi::Enum, Clone, Copy, PartialEq, Eq)]
pub enum TimelineTagState {
    Unchecked,
    Checked,
    Mixed,
}

#[derive(uniffi::Record, Clone)]
pub struct TimelineTagItem {
    pub tag: String,
    pub name: String,
    pub color: Option<u32>,
    pub state: TimelineTagState,
}

#[derive(uniffi::Record, Clone)]
pub struct TimelineRowMenu {
    pub reset_selected_enabled: bool,
    pub tags_enabled: bool,
    pub tags: Vec<TimelineTagItem>,
}

/// Every option the menus and toolbars set. `settings()` and `apply_settings()` carry these together.
#[derive(uniffi::Record, Clone)]
pub struct TimelineSettings {
    pub snap_enabled: bool,
    pub snap_strength: i32,
    pub draw_mode: bool,
    pub cad_style: bool,
    pub legacy_cursor: bool,
    pub zoom_under_mouse: bool,
    pub highlight_rows: bool,
    pub show_effect_info: bool,
    pub resize_indicator: bool,
    pub resize_indicator_color: String,
    pub waveform_height: i32,
    pub ruler_height: i32,
    pub lock_ruler_height: bool,
    pub lock_waveform_height: bool,
    pub full_waveform: bool,
    pub splitter_distance: i32,
    pub close_gap_threshold: String,
}

/// An effect's facts, for a dump or the status bar.
#[derive(uniffi::Record, Clone)]
pub struct TimelineElementInfo {
    pub id: u64,
    pub start: i64,
    pub duration: i64,
    pub rows: Vec<u32>,
    pub selected: bool,
    pub rendered: bool,
    pub type_name: String,
}

/// A mark's facts, for a dump.
#[derive(uniffi::Record, Clone)]
pub struct TimelineMarkInfo {
    pub id: String,
    pub collection: String,
    pub start: i64,
    pub duration: i64,
    pub text: String,
    pub selected: bool,
}

// ----------------------------------------------------------------------------------------------------------------------
// Helpers
// ----------------------------------------------------------------------------------------------------------------------

/// The tag definitions as the row labels and the Tags menu show them.
fn tag_displays(system: &vixen_io::system::SystemData) -> Vec<TagDisplay> {
    system
        .tags
        .iter()
        .map(|t| TagDisplay { id: t.id, name: t.name.clone(), sort_order: t.sort_order, color: t.display_color.as_deref().and_then(parse_display_color) })
        .collect()
}

fn argb(c: Color) -> u32 {
    ((c.a as u32) << 24) | ((c.r as u32) << 16) | ((c.g as u32) << 8) | c.b as u32
}

/// `displayColor` as the system configuration holds it, read as `ColorTranslator.FromHtml` does for the hex forms: `#RGB`, `#RRGGBB` or `#AARRGGBB`.
fn parse_display_color(text: &str) -> Option<Color> {
    let hex = text.trim().trim_start_matches('#');
    let value = u32::from_str_radix(hex, 16).ok()?;
    match hex.len() {
        3 => {
            let channel = |v: u32| (v * 17) as u8;
            Some(Color::rgb(channel((value >> 8) & 0xF), channel((value >> 4) & 0xF), channel(value & 0xF)))
        }
        6 => Some(Color::rgb((value >> 16) as u8, (value >> 8) as u8, value as u8)),
        8 => Some(Color { a: (value >> 24) as u8, r: (value >> 16) as u8, g: (value >> 8) as u8, b: value as u8 }),
        _ => None,
    }
}

fn elem_quad(q: &vixen_session::timeline::model::ElementQuad) -> TimelineElementQuad {
    TimelineElementQuad {
        id: q.id.0,
        row: q.row.0,
        x: q.x,
        y: q.y,
        w: q.w,
        h: q.h,
        selected: q.selected,
        rendered: q.rendered,
        red_border: q.red_border,
        moving: q.moving,
        picture: q.picture,
    }
}

/// Takes the session's lock the first time anything needs it, and not at all for a pointer move that commits nothing.
struct LazySession<'a> {
    session: &'a Mutex<SequenceSession>,
    guard: Option<MutexGuard<'a, SequenceSession>>,
}

impl<'a> LazySession<'a> {
    fn new(session: &'a Mutex<SequenceSession>) -> LazySession<'a> {
        LazySession { session, guard: None }
    }

    fn get(&mut self) -> &mut SequenceSession {
        self.guard.get_or_insert_with(|| self.session.lock())
    }
}

impl TimelineHost for LazySession<'_> {
    fn commit_times(&mut self, changes: &[(ElementId, TimeSpan, TimeSpan)]) {
        self.get().commit_times(changes);
    }

    fn commit_target(&mut self, id: ElementId, target: Uuid, name: &str) {
        self.get().commit_target(id, target, name);
    }

    fn clone_effect(&mut self, id: ElementId, target: Uuid, name: &str, start: TimeSpan, span: TimeSpan) -> Option<ElementId> {
        self.get().clone_effect(id, target, name, start, span)
    }

    fn add_effect(&mut self, type_id: Uuid, target: Uuid, name: &str, start: TimeSpan, span: TimeSpan) -> Option<ElementId> {
        self.get().add_effect(type_id, target, name, start, span)
    }

    fn remove_effects(&mut self, ids: &[ElementId]) -> Vec<EffectSnapshot> {
        TimelineHost::remove_effects(self.get(), ids)
    }

    fn record_undo(&mut self, action: UndoAction) {
        TimelineHost::record_undo(self.get(), action);
    }

    fn render_changes(&mut self) {
        self.get().render_changes();
    }

    fn commit_marks(&mut self, marks: &[vixen_core::MarkCollection]) {
        self.get().commit_marks(marks);
    }

    fn describe(&mut self, id: ElementId) -> Option<vixen_session::timeline::ElementDescription> {
        self.get().describe(id)
    }

    fn mark_modified(&mut self) {
        self.get().mark_modified();
    }

    fn commit_marks_quietly(&mut self, marks: &[vixen_core::MarkCollection]) {
        self.get().commit_marks_quietly(marks);
    }
}

/// The timeline for one sequencer window.
#[derive(uniffi::Object)]
pub struct TimelineController {
    session: Arc<SequencerSession>,
    timeline: Mutex<Timeline>,
    /// The session's counters when this last caught up with them.
    seen_edits: AtomicU64,
    seen_renders: AtomicU64,
    /// `effect.TargetNodes.GetElements()` can be slow for a big group, and never changes here.
    elements_of: Mutex<HashMap<NodeId, Arc<Vec<Uuid>>>>,
    tree: Arc<vixen_core::ElementTree>,
    /// Text that isn't a number still shows here, as the toolbar's box holds it.
    close_gap_threshold: Mutex<String>,
    splitter_distance: Mutex<i32>,
    /// The session's counters, readable with no lock.
    counters_cell: Arc<vixen_session::sequence::RenderCounters>,
    draw_failures: DrawFailures,
}

impl TimelineController {
    /// One locked call: runs `f` with a host, then packs up what changed.
    fn with_host(&self, f: impl FnOnce(&mut Timeline, &mut LazySession) -> InputOutcome) -> TimelineUpdate {
        let mut t = self.timeline.lock();
        let mut host = LazySession::new(&self.session.session);
        let outcome = f(&mut t, &mut host);
        let touched = host.guard.is_some();
        let update = self.pack(&mut t, &mut host, outcome);
        if touched || host.guard.is_some() {
            // What was just committed needs no syncing back.
            self.seen_edits.store(self.session_counters_edits(), Ordering::Relaxed);
        }
        update
    }

    fn session_counters_edits(&self) -> u64 {
        self.counters().edits.load(Ordering::Relaxed)
    }

    fn counters(&self) -> Arc<vixen_session::sequence::RenderCounters> {
        Arc::clone(&self.counters_cell)
    }

    fn update_only(&self, f: impl FnOnce(&mut Timeline)) -> TimelineUpdate {
        self.with_host(|t, _| {
            f(t);
            t.take_outcome()
        })
    }

    fn pack(&self, t: &mut Timeline, host: &mut LazySession, outcome: InputOutcome) -> TimelineUpdate {
        let mut requests = Vec::with_capacity(outcome.requests.len());
        for request in &outcome.requests {
            match request {
                HostRequest::ChooseOverlapping { elements, at } => {
                    let items = elements
                        .iter()
                        .filter_map(|id| t.overlap_menu_label(*id).map(|label| TimelineMenuItem { id: id.0, label }))
                        .collect();
                    requests.push(TimelineRequest::ChooseOverlapping { items, x: at.0, y: at.1 });
                }
                HostRequest::ContextMenu { elements, time, row } => requests.push(TimelineRequest::ContextMenu {
                    elements: elements.iter().map(|id| id.0).collect(),
                    time: time.ticks(),
                    row: row.0,
                }),
                HostRequest::RowContextMenu { row } => requests.push(TimelineRequest::RowContextMenu { row: row.0 }),
                HostRequest::Warning { text, title } => requests.push(TimelineRequest::Warning { text: text.clone(), title: title.clone() }),
                HostRequest::UndoRecorded => requests.push(TimelineRequest::UndoRecorded),
                HostRequest::ElementDoubleClicked(id) => requests.push(TimelineRequest::ElementDoubleClicked { id: id.0 }),
                HostRequest::PlayRange { start, end } => {
                    requests.push(TimelineRequest::PlayRange { start: start.ticks(), end: end.map(TimeSpan::ticks) })
                }
                HostRequest::MarkMenu => requests.push(TimelineRequest::MarkMenu),
                HostRequest::MarksContextMenu { press, .. } => requests.push(TimelineRequest::MarksContextMenu { press_x: press.0, press_y: press.1 }),
                HostRequest::NeedsNewCollection { time, fill_gap, label } => {
                    requests.push(TimelineRequest::NeedsNewCollection { time: time.ticks(), fill_gap: *fill_gap, label: label.clone() })
                }
                HostRequest::NeedsEnableDefault { time, fill_gap, label } => {
                    requests.push(TimelineRequest::NeedsEnableDefault { time: time.ticks(), fill_gap: *fill_gap, label: label.clone() })
                }
                HostRequest::PromptMarkLabel { time, fill_gap } => {
                    requests.push(TimelineRequest::PromptMarkLabel { time: time.ticks(), fill_gap: *fill_gap })
                }
                // The ruler's right-click: nothing for the window to do unless a question comes back.
                HostRequest::AddMark { time, fill_gap, label } => {
                    match t.add_mark_at(*time, *fill_gap, label.clone(), host) {
                        AddMarkResult::NeedsNewCollection => {
                            requests.push(TimelineRequest::NeedsNewCollection { time: time.ticks(), fill_gap: *fill_gap, label: label.clone() })
                        }
                        AddMarkResult::NeedsEnableDefault => {
                            requests.push(TimelineRequest::NeedsEnableDefault { time: time.ticks(), fill_gap: *fill_gap, label: label.clone() })
                        }
                        AddMarkResult::Added(_) => requests.push(TimelineRequest::UndoRecorded),
                        AddMarkResult::Nothing => {}
                    }
                }
            }
        }
        // Adding a mark above dirties more, which goes with this update too.
        let extra = t.take_outcome();
        let dirty = outcome.dirty;
        let mut grid_rects: Vec<TimelineRect> = dirty.grid_rects.iter().map(|r| (*r).into()).collect();
        grid_rects.extend(extra.dirty.grid_rects.iter().map(|r| TimelineRect::from(*r)));
        TimelineUpdate {
            view: view_of(t),
            cursor_kind: outcome.cursor.into(),
            autoscroll: outcome.autoscroll,
            selection_changed: outcome.selection_changed || extra.selection_changed,
            cursor_moved: outcome.cursor_moved || extra.cursor_moved,
            grid_all: dirty.grid_all || extra.dirty.grid_all,
            grid_rects,
            row_header: dirty.row_header || extra.dirty.row_header,
            ruler: dirty.ruler || extra.dirty.ruler,
            marks_bar: dirty.marks_bar || extra.dirty.marks_bar,
            waveform: dirty.waveform || extra.dirty.waveform,
            overlay: dirty.overlay || extra.dirty.overlay,
            view_changed: dirty.view || extra.dirty.view,
            requests,
            editor_state_modified: outcome.editor_state_modified || extra.editor_state_modified,
        }
    }

    /// F151, FIX Q45: builds a frame, and if that fails logs it and hands back `failed`, so the last good content stays. Frames are built by Rust, which
    /// shouldn't panic; a panic crossing the bindings would end the app in the middle of someone's work.
    fn frame<R>(&self, surface: &str, failed: impl FnOnce() -> R, build: impl FnOnce() -> R) -> R {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(build)) {
            Ok(frame) => {
                self.draw_failures.frame_succeeded();
                frame
            }
            Err(_) => {
                eprintln!("Drawing the timeline's {surface} failed");
                self.draw_failures.frame_failed();
                failed()
            }
        }
    }

    /// F142: "Building waveform....." shows until this is done.
    fn build_peaks(self: &Arc<Self>, clip: Arc<vixen_session::timeline::waveform::AudioClip>) {
        let weak = Arc::downgrade(self);
        let _ = std::thread::Builder::new().name("vixen-waveform-peaks".to_string()).spawn(move || {
            let peaks = vixen_session::timeline::waveform::build_peaks(clip);
            if let Some(controller) = weak.upgrade() {
                controller.timeline.lock().set_peaks(Some(peaks));
            }
        });
    }

    fn build_row_frame(&self, top: i32, bottom: i32) -> TimelineRowFrame {
        let t = self.timeline.lock();
        let highlight = t.opts.highlight_rows;
        let by_id: HashMap<Uuid, (&TagDisplay, u32)> =
            t.tags.iter().filter_map(|d| d.color.map(|c| (d.id, (d, argb(c))))).collect();
        let rows = t
            .rows
            .visible_rows(top, bottom)
            .into_iter()
            .map(|r| {
                let mut colors: Vec<(i32, u32)> =
                    r.tags.iter().filter_map(|id| by_id.get(id).map(|(d, c)| (d.sort_order, *c))).collect();
                colors.sort_by_key(|(order, _)| *order);
                TimelineRowDisplay {
                    id: r.id.0,
                    name: r.name,
                    depth: r.depth,
                    height: r.height,
                    top: r.top,
                    expandable: r.expandable,
                    open: r.open,
                    highlighted: highlight && r.has_effects,
                    child_highlighted: highlight && r.subtree_has_effects,
                    selected: r.selected,
                    active: r.active,
                    tag_colors: colors.into_iter().map(|(_, c)| c).collect(),
                }
            })
            .collect();
        let connectors = t
            .rows
            .connector_lines(top, bottom, vixen_session::timeline::rows::TOGGLE_BUTTON_WIDTH)
            .into_iter()
            .map(|c| TimelineConnector { x0: c.x0, y0: c.y0, x1: c.x1, y1: c.y1 })
            .collect();
        TimelineRowFrame { rows, connectors, failed: false }
    }

    fn build_ruler_frame(&self, x: i32, width: i32, tpp: TimeSpan) -> TimelineRulerFrame {
        let t = self.timeline.lock();
        let f = t.ruler_frame_at(x, width, tpp);
        TimelineRulerFrame {
            major_ticks: f.major_ticks,
            minor_ticks: f.minor_ticks,
            labels: f.labels.into_iter().map(|(centre, text)| TimelineRulerLabel { centre, text }).collect(),
            mark_lines: f.mark_lines.iter().map(|l| TimelineLine { x: l.x, color: argb(l.color), width: l.width, dash: l.dash }).collect(),
            total_px: f.total_px,
            range_start: f.range_start,
            range_end: f.range_end,
            height: t.ruler_height(),
            failed: false,
        }
    }

    fn build_marks_frame(&self, x: i32, width: i32, tpp: TimeSpan) -> TimelineMarksFrame {
        let mut t = self.timeline.lock();
        let f = t.marks_bar_frame_at(x, width, tpp);
        TimelineMarksFrame {
            total_height: f.total_height,
            row_bottoms: f.row_bottoms,
            marks: f
                .marks
                .into_iter()
                .map(|m| TimelineMarkQuad {
                    id: m.id.to_string(),
                    x: m.x,
                    y: m.y,
                    w: m.w,
                    h: m.h,
                    color: argb(m.color),
                    hatch: m.hatch.map(argb),
                    selected: m.selected,
                    text: m.text,
                    text_color: argb(m.text_color),
                })
                .collect(),
            failed: false,
        }
    }

    fn build_waveform_frame(&self, first_column: i32, columns: i32, tpp: TimeSpan) -> TimelineWaveformFrame {
        let t = self.timeline.lock();
        let f = t.waveform_frame_at(first_column, columns, tpp);
        TimelineWaveformFrame {
            building: f.building,
            first_column: f.first_column,
            top: f.top,
            baseline: f.baseline,
            full: f.full,
            highs: f.highs,
            lows: f.lows,
            height: t.waveform.height,
            failed: false,
        }
    }

    fn elements_for(&self, node: NodeId) -> Arc<Vec<Uuid>> {
        let mut cache = self.elements_of.lock();
        Arc::clone(cache.entry(node).or_insert_with(|| Arc::new(self.tree.elements(node))))
    }
}

fn view_of(t: &Timeline) -> TimelineView {
    TimelineView {
        time_per_pixel: t.tpp().ticks(),
        visible_start: t.info.visible_start.ticks(),
        total: t.info.total.ticks(),
        scroll_x: t.time_to_px(t.info.visible_start),
        vertical_offset: t.vertical_offset,
        canvas_width: t.canvas_width(),
        canvas_height: t.rows.canvas_height(),
        waveform_height: t.waveform_layout_height(),
        ruler_height: t.ruler_height(),
        marks_bar_height: t.marks.bar_height,
        cursor_x: t.time_to_px(t.cursor),
        cursor: t.cursor.ticks(),
        dragging: t.is_drag_in_progress(),
        waveform_building: t.waveform.has_audio && t.waveform.peaks.is_none(),
        waveform_blank: t.waveform_blank(),
    }
}

fn overlay_record(frame: OverlayFrame, t: &Timeline) -> TimelineOverlay {
    let line = |l: &vixen_session::timeline::snap::MarkLine| TimelineLine { x: l.x, color: argb(l.color), width: l.width, dash: l.dash };
    let drag = t.marks.ruler.drag_label.clone();
    TimelineOverlay {
        occurrences: frame.occurrences.iter().map(elem_quad).collect(),
        rubber_band: frame.rubber_band.map(Into::into),
        selection_green: frame.selection_green,
        draw_box: frame.draw_box.map(Into::into),
        indicator_lines: frame.indicator_lines,
        indicator_color: argb(frame.indicator_color),
        grid_guides: frame.grid_guides,
        guides: frame.guides,
        moving_mark_lines: frame.moving_mark_lines.iter().map(line).collect(),
        snapping_mark_lines: Vec::new(), // STUDIO: filled in by `overlay`
        hover: frame.hover.map(|h| TimelineHover { text: h.text, rect: h.rect.into() }),
        cursor_x: frame.cursor_x,
        ruler_drag_x: drag.as_ref().map(|d| d.0),
        ruler_drag_text: drag.map(|d| d.1),
        playback_x: t.info.playback_current.map(|p| t.time_to_px(p)),
    }
}

// ----------------------------------------------------------------------------------------------------------------------
// The object
// ----------------------------------------------------------------------------------------------------------------------

#[uniffi::export]
impl TimelineController {
    /// Rows, effects and marks from the session. Peaks build on a worker if the sequence has audio.
    #[uniffi::constructor]
    pub fn new(session: Arc<SequencerSession>) -> Arc<TimelineController> {
        let tags = tag_displays(&session.core.system.read());
        let mut timeline = Timeline::new();
        let (counters, tree, audio, media) = {
            let s = session.session.lock();
            timeline.load_from_session(&s, tags);
            let audio = s.audio().cloned();
            let media = audio.as_ref().map(|c| TimeSpan::from_secs(c.frames() as f64 / c.sample_rate() as f64));
            (Arc::clone(&s.counters), Arc::clone(s.tree()), audio, media)
        };
        timeline.set_audio(audio.is_some(), media.unwrap_or(TimeSpan::ZERO));
        let edits = counters.edits.load(Ordering::Relaxed);
        let renders = counters.revision.load(Ordering::Relaxed);
        let controller = Arc::new(TimelineController {
            session,
            timeline: Mutex::new(timeline),
            seen_edits: AtomicU64::new(edits),
            seen_renders: AtomicU64::new(renders),
            elements_of: Mutex::new(HashMap::new()),
            tree,
            close_gap_threshold: Mutex::new(".100".to_string()),
            splitter_distance: Mutex::new(200),
            counters_cell: counters,
            draw_failures: DrawFailures::default(),
        });
        if let Some(clip) = audio {
            controller.build_peaks(clip);
        }
        controller
    }

    /// Catches up with whatever changed in the session from outside: undo, redo, other panels, finished renders. Cheap when nothing has:
    /// two atomic reads. Never waits for the session's lock on a display path; if it's busy, a later call catches up.
    pub fn refresh(&self) -> TimelineUpdate {
        let counters = self.counters();
        let edits = counters.edits.load(Ordering::Relaxed);
        let renders = counters.revision.load(Ordering::Relaxed);
        let mut t = self.timeline.lock();
        let edits_changed = edits != self.seen_edits.load(Ordering::Relaxed);
        let renders_changed = renders != self.seen_renders.load(Ordering::Relaxed);
        if edits_changed || renders_changed {
            if let Some(session) = self.session.session.try_lock() {
                if edits_changed {
                    if t.sync_from_session(&session) {
                        self.seen_edits.store(edits, Ordering::Relaxed);
                    }
                } else {
                    t.refresh_render_state(&session);
                }
                if renders_changed {
                    t.refresh_render_state(&session);
                    self.seen_renders.store(renders, Ordering::Relaxed);
                }
            }
        }
        let mut host = LazySession::new(&self.session.session);
        let outcome = t.take_outcome();
        self.pack(&mut t, &mut host, outcome)
    }

    /// What's on the document now. Call before `SequencerSession.save`: `SaveGridRowSettings`, and the zoom and scroll.
    pub fn store_view_state(&self) {
        let t = self.timeline.lock();
        t.store_view_state(&mut self.session.session.lock());
    }

    /// The window's session, for code that has only this.
    pub fn session(&self) -> Arc<SequencerSession> {
        Arc::clone(&self.session)
    }

    // Layout

    /// The grid's client size and the panel holding it. Returns the view with any re-clamping done.
    pub fn set_viewport(&self, width: i32, height: i32, panel_width: i32) -> TimelineUpdate {
        self.update_only(|t| t.set_viewport(width, height, panel_width))
    }

    /// Widths of the ruler font's digits and punctuation, measured once. F99.
    pub fn set_ruler_metrics(&self, digit: f64, colon: f64, dot: f64) -> TimelineUpdate {
        self.update_only(|t| t.marks.ruler.metrics = LabelMetrics { digit, colon, dot })
    }

    pub fn view(&self) -> TimelineView {
        view_of(&self.timeline.lock())
    }

    // Frames. Any thread.

    /// F10 for a rectangle of the canvas, at a zoom: a set of tiles is painted for one, and a zoom that's still running will have moved on before they're done.
    pub fn grid_frame(&self, x: i32, y: i32, w: i32, h: i32, time_per_pixel: i64) -> TimelineGridFrame {
        self.frame(
            "grid",
            || TimelineGridFrame { rows: Vec::new(), elements: Vec::new(), mark_lines: Vec::new(), gridlines: Vec::new(), selection_green: false, failed: true },
            || {
                let mut t = self.timeline.lock();
                let frame = t.grid_frame_at(IRect::new(x, y, w, h), TimeSpan(time_per_pixel));
                TimelineGridFrame {
                    rows: frame.rows.iter().map(|r| TimelineRowQuad { top: r.top, height: r.height, selected: r.selected, active: r.active }).collect(),
                    elements: frame.elements.iter().map(elem_quad).collect(),
                    mark_lines: frame.mark_lines.iter().map(|l| TimelineLine { x: l.x, color: argb(l.color), width: l.width, dash: l.dash }).collect(),
                    gridlines: frame.gridlines,
                    selection_green: t.overlay_selection_green(),
                    failed: false,
                }
            },
        )
    }

    /// F151's alert text, once per run of failures. Show it, with the title "Error".
    pub fn take_draw_failure(&self) -> Option<String> {
        self.draw_failures.take()
    }

    /// Everything that moves without touching a tile. `x`..: the canvas rectangle the layers extend over.
    pub fn overlay(&self, x: i32, y: i32, w: i32, h: i32) -> TimelineOverlay {
        let mut t = self.timeline.lock();
        let frame = t.overlay_frame(IRect::new(x, y, w, h));
        let mut o = overlay_record(frame, &t);
        o.snapping_mark_lines = t.studio_snapping_mark_lines(IRect::new(x, y, w, h)).iter().map(|l| TimelineLine { x: l.x, color: argb(l.color), width: l.width, dash: l.dash }).collect(); // STUDIO: R35
        o
    }

    /// F21, F22 for a span of the canvas.
    pub fn row_frame(&self, top: i32, bottom: i32) -> TimelineRowFrame {
        self.frame("row list", || TimelineRowFrame { rows: Vec::new(), connectors: Vec::new(), failed: true }, || self.build_row_frame(top, bottom))
    }

    /// F98, F99, F104, F106 for a span of the ruler's canvas.
    pub fn ruler_frame(&self, x: i32, width: i32, time_per_pixel: i64) -> TimelineRulerFrame {
        self.frame(
            "ruler",
            || TimelineRulerFrame {
                major_ticks: Vec::new(),
                minor_ticks: Vec::new(),
                labels: Vec::new(),
                mark_lines: Vec::new(),
                total_px: 0.0,
                range_start: None,
                range_end: None,
                height: 0,
                failed: true,
            },
            || self.build_ruler_frame(x, width, TimeSpan(time_per_pixel)),
        )
    }

    /// F117 to F119 for a span of the bar's canvas.
    pub fn marks_frame(&self, x: i32, width: i32, time_per_pixel: i64) -> TimelineMarksFrame {
        self.frame("marks bar", || TimelineMarksFrame { total_height: 0, row_bottoms: Vec::new(), marks: Vec::new(), failed: true }, || self.build_marks_frame(x, width, TimeSpan(time_per_pixel)))
    }

    // STUDIO-BEGIN quiet-mark-ticks: marks as coloured ticks in the waveform (UI roadmap R35)
    /// The waveform's mark ticks over a span of columns: the ruler's marks, colours and selection, solid.
    pub fn studio_mark_ticks(&self, x: i32, width: i32, time_per_pixel: i64) -> Vec<TimelineLine> {
        let t = self.timeline.lock();
        t.studio_mark_ticks(x, width, TimeSpan(time_per_pixel)).iter().map(|l| TimelineLine { x: l.x, color: argb(l.color), width: l.width, dash: l.dash }).collect()
    }
    // STUDIO-END quiet-mark-ticks

    // STUDIO-BEGIN row-height-mode: View ▸ Row Height, compact or regular (UI roadmap R36)
    /// The window's row height, which View ▸ Row Height checks, and how many rows the grid shows at it.
    pub fn studio_row_height(&self) -> TimelineStudioRowHeight {
        let t = self.timeline.lock();
        TimelineStudioRowHeight { mode: t.studio_row_height().name().to_string(), height_pt: t.rows.row_height, rows_in_view: t.studio_rows_in_view() as u32 }
    }
    // STUDIO-END row-height-mode

    // STUDIO-BEGIN agent-flash: agent edits flash on the timeline (R39)
    /// What agents added or changed in this document after `since`, where it shows on the grid
    /// (`studio_agent_flash`). No `since`, a window's first look, answers with nothing, so opening a sequence an agent
    /// edited earlier doesn't flash it. While the session is busy, or this grid hasn't caught up with it yet, the answer
    /// keeps `since`, so the effects flash on a later call once they are on the grid.
    pub fn studio_agent_flash(&self, since: Option<u64>) -> TimelineStudioAgentFlash {
        let t = self.timeline.lock();
        let Some(s) = self.session.session.try_lock() else {
            return TimelineStudioAgentFlash { revision: since.unwrap_or(0), rects: Vec::new(), effects: 0 };
        };
        let flash = s.studio_agent_flash();
        let Some(since) = since else {
            return TimelineStudioAgentFlash { revision: flash.revision(), rects: Vec::new(), effects: 0 };
        };
        if self.seen_edits.load(Ordering::Relaxed) != self.session_counters_edits() {
            return TimelineStudioAgentFlash { revision: since, rects: Vec::new(), effects: 0 };
        }
        let (revision, ids) = flash.since(since);
        drop(s);
        let rects = if ids.is_empty() { Vec::new() } else { t.studio_flash_rects(&ids) };
        TimelineStudioAgentFlash { revision, rects: rects.iter().map(|r| TimelineRect { x: r.x, y: r.y, w: r.w, h: r.h }).collect(), effects: ids.len() as u32 }
    }
    // STUDIO-END agent-flash

    /// F141 for a span of the canvas.
    pub fn waveform_frame(&self, first_column: i32, columns: i32, time_per_pixel: i64) -> TimelineWaveformFrame {
        self.frame(
            "waveform",
            || TimelineWaveformFrame { building: false, first_column: 0, top: 0, baseline: 0, full: false, highs: Vec::new(), lows: Vec::new(), height: 0, failed: true },
            || self.build_waveform_frame(first_column, columns, TimeSpan(time_per_pixel)),
        )
    }

    /// F42: an effect's picture, or a slice of it. Blocks while it rasterizes, so call it off the main thread. `None` if the effect has gone.
    pub fn picture(&self, id: u64, overall_width: f64, slice_x: i32, width: u32, height: u32) -> Option<TimelinePicture> {
        let source = {
            let session = self.session.session.lock();
            picture_source(&session, EffectId(id), &mut |node| self.elements_for(node))?
        };
        let pixels = render_picture(&source, overall_width, slice_x, width, height);
        Some(TimelinePicture { width: pixels.width, height: pixels.height, rgba: pixels.rgba, picture: source.picture })
    }

    // Grid input

    pub fn grid_mouse_down(&self, ev: TimelinePointer) -> TimelineUpdate {
        self.with_host(|t, _| t.grid_mouse_down(ev.into()))
    }

    pub fn grid_mouse_move(&self, ev: TimelinePointer) -> TimelineUpdate {
        self.with_host(|t, host| t.grid_mouse_move(ev.into(), host))
    }

    pub fn grid_mouse_up(&self, ev: TimelinePointer) -> TimelineUpdate {
        self.with_host(|t, host| t.grid_mouse_up(ev.into(), host))
    }

    pub fn grid_double_click(&self, ev: TimelinePointer) -> TimelineUpdate {
        self.update_only(|t| {
            t.grid_double_click(ev.into());
        })
    }

    /// Every 50 ms while `autoscroll` is true.
    pub fn grid_autoscroll_tick(&self, mods: TimelineMods) -> TimelineUpdate {
        self.with_host(|t, host| t.grid_autoscroll_tick(mods.into(), host))
    }

    /// F64: Esc during a drag.
    pub fn grid_cancel_drag(&self) -> TimelineUpdate {
        self.with_host(|t, _| t.grid_cancel_drag())
    }

    /// F50: the overlap menu has closed, with what was picked or nothing. Its tracking loop took the release.
    pub fn finish_chooser(&self, chosen: Option<u64>) -> TimelineUpdate {
        self.with_host(|t, _| t.finish_chooser(chosen.map(EffectId)))
    }

    pub fn is_drag_in_progress(&self) -> bool {
        self.timeline.lock().is_drag_in_progress()
    }

    // Row list input

    pub fn row_mouse_down(&self, ev: TimelinePointer, width: i32) -> TimelineUpdate {
        self.with_host(|t, _| t.row_mouse_down(ev.into(), width))
    }

    pub fn row_mouse_move(&self, ev: TimelinePointer, width: i32) -> TimelineUpdate {
        self.with_host(|t, _| t.row_mouse_move(ev.into(), width))
    }

    pub fn row_mouse_up(&self, ev: TimelinePointer, width: i32) -> TimelineUpdate {
        self.with_host(|t, _| t.row_mouse_up(ev.into(), width))
    }

    // Ruler input

    pub fn ruler_mouse_down(&self, ev: TimelinePointer) -> TimelineUpdate {
        self.with_host(|t, _| t.ruler_mouse_down(ev.into()))
    }

    pub fn ruler_mouse_move(&self, ev: TimelinePointer) -> TimelineUpdate {
        self.with_host(|t, _| t.ruler_mouse_move(ev.into()))
    }

    pub fn ruler_mouse_up(&self, ev: TimelinePointer) -> TimelineUpdate {
        self.with_host(|t, host| t.ruler_mouse_up(ev.into(), host))
    }

    pub fn ruler_double_click(&self) -> TimelineUpdate {
        self.with_host(|t, _| t.ruler_double_click())
    }

    pub fn ruler_mouse_exited(&self) -> TimelineUpdate {
        self.with_host(|t, _| t.ruler_mouse_exited())
    }

    // Marks bar input

    pub fn marks_mouse_down(&self, ev: TimelinePointer) -> TimelineUpdate {
        self.with_host(|t, host| t.marks_mouse_down(ev.into(), host))
    }

    pub fn marks_mouse_move(&self, ev: TimelinePointer) -> TimelineUpdate {
        self.with_host(|t, _| t.marks_mouse_move(ev.into()))
    }

    pub fn marks_mouse_up(&self, ev: TimelinePointer) -> TimelineUpdate {
        self.with_host(|t, host| t.marks_mouse_up(ev.into(), host))
    }

    pub fn marks_autoscroll_tick(&self, mods: TimelineMods) -> TimelineUpdate {
        self.with_host(|t, _| t.marks_autoscroll_tick(mods.into()))
    }

    pub fn marks_context_menu(&self) -> TimelineMarksMenu {
        self.timeline.lock().marks_context_menu().into()
    }

    /// F130's dialog.
    pub fn edit_text_prompt(&self) -> TimelineTextPrompt {
        let t = self.timeline.lock();
        let (title, text) = t.edit_text_prompt();
        TimelineTextPrompt { select_all: t.marks.selected.len() == 1, title, text }
    }

    pub fn set_selected_marks_text(&self, text: String) -> TimelineUpdate {
        self.with_host(|t, host| {
            t.set_selected_marks_text(&text, host);
            t.take_outcome()
        })
    }

    pub fn copy_marks(&self) {
        self.timeline.lock().copy_marks();
    }

    pub fn cut_marks(&self) -> TimelineUpdate {
        self.with_host(|t, host| {
            t.cut_marks(host);
            t.take_outcome()
        })
    }

    /// F133's questions, in order, with no change yet. Ask them until one is answered No, then call `paste_marks` with how many were Yes.
    pub fn paste_mark_questions(&self, press_x: i32, press_y: i32) -> Vec<String> {
        let mut t = self.timeline.lock();
        t.paste_questions((press_x, press_y))
    }

    /// F133. `allowed_mismatches` is how many of its questions were answered Yes before a No, or all of them.
    pub fn paste_marks(&self, press_x: i32, press_y: i32, allowed_mismatches: u32) -> TimelineUpdate {
        self.with_host(|t, host| {
            let mut left = allowed_mismatches;
            t.paste_marks(
                (press_x, press_y),
                &mut |_: &MismatchInfo| {
                    if left == 0 {
                        return false;
                    }
                    left -= 1;
                    true
                },
                host,
            );
            t.take_outcome()
        })
    }

    /// F134's menu item.
    pub fn delete_selected_marks(&self) -> TimelineUpdate {
        self.with_host(|t, host| {
            t.delete_selected_marks(host);
            t.take_outcome()
        })
    }

    /// The Delete key on the bar, and the ruler's menu. F112, F134: neither is gated on a selection.
    pub fn delete_marks_key(&self) -> TimelineUpdate {
        self.with_host(|t, host| {
            t.delete_marks_key(host);
            t.take_outcome()
        })
    }

    /// F155, in the order selected.
    pub fn mark_selection(&self) -> Vec<String> {
        self.timeline.lock().mark_selection().iter().map(Uuid::to_string).collect()
    }

    /// F153: zoom, scroll and row height have changed since load. Never cleared. Closing saves silently on it.
    pub fn is_editor_state_modified(&self) -> bool {
        self.timeline.lock().is_editor_state_modified()
    }

    /// F135's list.
    pub fn phoneme_names(&self) -> Vec<String> {
        PHONEMES.iter().map(|p| p.to_string()).collect()
    }

    pub fn breakdown_phrase(&self) -> TimelineUpdate {
        self.with_host(|t, host| {
            t.breakdown_phrase(host);
            t.take_outcome()
        })
    }

    /// F137 with phonemes from the caller, per selected mark's text, because the lip sync dictionary is another area's. A word mapping to
    /// nothing is skipped.
    pub fn breakdown_word(&self, phonemes: Vec<TimelineWordPhonemes>) -> TimelineUpdate {
        self.with_host(|t, host| {
            let map: HashMap<String, Vec<String>> = phonemes.into_iter().map(|p| (p.word, p.phonemes)).collect();
            t.breakdown_word(&mut |text: &str| map.get(text).cloned().unwrap_or_default(), host);
            t.take_outcome()
        })
    }

    /// F111. A question comes back in `requests`.
    pub fn add_mark_at(&self, time: i64, fill_gap: bool, label: Option<String>) -> TimelineUpdate {
        self.with_host(|t, host| {
            let mut outcome = t.take_outcome();
            outcome.requests.push(HostRequest::AddMark { time: TimeSpan(time), fill_gap, label });
            let _ = host;
            outcome
        })
    }

    pub fn create_default_mark_collection(&self) -> TimelineUpdate {
        self.with_host(|t, host| {
            t.create_default_mark_collection(host);
            t.take_outcome()
        })
    }

    pub fn enable_default_mark_collection(&self) -> TimelineUpdate {
        self.with_host(|t, host| {
            t.enable_default_mark_collection(host);
            t.take_outcome()
        })
    }

    // Wheels. `pointer_offset_x` is the distance from the splitter, `grid_y` the pointer in the grid's client coordinates.

    pub fn scroll_wheel(&self, surface: TimelineSurface, dx: f64, dy: f64, precise: bool, mods: TimelineMods, pointer_offset_x: i32, grid_y: i32) -> TimelineUpdate {
        self.update_only(|t| {
            t.scroll_wheel(ScrollEvent {
                surface: match surface {
                    TimelineSurface::Grid => Surface::Grid,
                    TimelineSurface::Ruler => Surface::Ruler,
                    TimelineSurface::Waveform => Surface::Waveform,
                    TimelineSurface::RowList => Surface::RowList,
                    TimelineSurface::MarksBar => Surface::MarksBar,
                },
                dx,
                dy,
                precise,
                mods: mods.into(),
                pointer_offset_x,
                grid_y,
            })
        })
    }

    pub fn magnify(&self, magnification: f64, pointer_offset_x: i32, mods: TimelineMods) -> TimelineUpdate {
        self.update_only(|t| t.magnify(magnification, pointer_offset_x, mods.into()))
    }

    // View commands

    /// F15: 0.8 zooms in, 1.25 out.
    pub fn zoom_time(&self, scale: f64) -> TimelineUpdate {
        self.update_only(|t| {
            t.zoom_time(scale);
        })
    }

    /// F31: 1.25 and 0.8.
    pub fn zoom_rows(&self, scale: f64) -> TimelineUpdate {
        self.update_only(|t| t.zoom_rows(scale))
    }

    pub fn set_scroll_x(&self, x: i32) -> TimelineUpdate {
        self.update_only(|t| t.set_scroll_x(x))
    }

    pub fn set_vertical_offset(&self, y: i32) -> TimelineUpdate {
        self.update_only(|t| t.set_vertical_offset(y))
    }

    pub fn set_visible_start(&self, ticks: i64) -> TimelineUpdate {
        self.update_only(|t| t.set_visible_start(TimeSpan(ticks)))
    }

    /// Home and ⌘Home, End and ⌘End.
    pub fn scroll_to_top(&self) -> TimelineUpdate {
        self.update_only(|t| t.set_vertical_offset(0))
    }

    pub fn scroll_to_bottom(&self) -> TimelineUpdate {
        self.update_only(|t| t.set_vertical_offset(i32::MAX))
    }

    pub fn scroll_to_start(&self) -> TimelineUpdate {
        self.update_only(|t| t.scroll_to_start())
    }

    pub fn scroll_to_end(&self) -> TimelineUpdate {
        self.update_only(|t| t.scroll_to_end())
    }

    /// PageUp, PageDown: half a view.
    pub fn page_vertical(&self, direction: i32) -> TimelineUpdate {
        self.update_only(|t| t.page_vertical(direction))
    }

    /// ⌘PageUp, ⌘PageDown: half a view of time.
    pub fn page_time(&self, direction: i32) -> TimelineUpdate {
        self.update_only(|t| t.page_time(direction))
    }

    // Rows

    /// F28.
    pub fn move_active_row(&self, direction: i32) -> bool {
        self.timeline.lock().move_active_row(direction)
    }

    pub fn toggle_selected_rows(&self, include_children: bool) -> TimelineUpdate {
        self.update_only(|t| t.toggle_selected_rows(include_children))
    }

    /// Every collection in the marks bar, for tests and the benchmark. Not committed: the marks docker owns these flags.
    pub fn set_all_mark_bars_visible(&self, visible: bool) -> TimelineUpdate {
        self.update_only(|t| t.set_all_mark_bars_visible(visible))
    }

    /// Every row open, for tests and the benchmark.
    pub fn expand_all_rows(&self) -> TimelineUpdate {
        self.update_only(|t| t.expand_all_rows())
    }

    /// A zoom with no rules, for tests and the benchmark.
    pub fn set_time_per_pixel_for_bench(&self, ticks: i64) -> TimelineUpdate {
        self.update_only(|t| t.set_time_per_pixel(TimeSpan(ticks)))
    }

    pub fn collapse_all_rows(&self) -> TimelineUpdate {
        self.update_only(|t| t.collapse_all_rows())
    }

    pub fn reset_row_heights(&self) -> TimelineUpdate {
        self.update_only(|t| t.reset_row_heights())
    }

    pub fn reset_selected_row_heights(&self) -> TimelineUpdate {
        self.update_only(|t| t.reset_selected_row_heights())
    }

    /// F24.
    pub fn set_show_hidden_rows(&self, show: bool) -> TimelineUpdate {
        self.update_only(|t| t.set_show_hidden_rows(show))
    }

    /// F33, F37.
    pub fn row_menu(&self) -> TimelineRowMenu {
        let t = self.timeline.lock();
        let model = t.row_menu_model();
        TimelineRowMenu {
            reset_selected_enabled: model.reset_selected_enabled,
            tags_enabled: model.tags_enabled,
            tags: t
                .tag_menu_state()
                .into_iter()
                .map(|i| TimelineTagItem {
                    tag: i.tag.to_string(),
                    name: i.name,
                    color: i.color.map(argb),
                    state: match i.state {
                        TagCheck::Unchecked => TimelineTagState::Unchecked,
                        TagCheck::Checked => TimelineTagState::Checked,
                        TagCheck::Mixed => TimelineTagState::Mixed,
                    },
                })
                .collect(),
        }
    }

    /// F37: saved to the system configuration at once. `cascade` is ⌘.
    pub fn toggle_tag_on_selected(&self, tag: String, cascade: bool) -> TimelineUpdate {
        let Ok(tag) = Uuid::parse_str(&tag) else { return self.update_only(|_| {}) };
        self.update_only(|t| {
            let changes = t.toggle_tag_on_selected(tag, cascade);
            let core = &self.session.core;
            let mut system = core.system.write();
            for change in &changes {
                if let Some(node) = system.tree.find(&change.node) {
                    let tags = &mut system.tree.node_mut(node).tags;
                    if change.added {
                        if !tags.contains(&change.tag) {
                            tags.push(change.tag);
                        }
                    } else {
                        tags.retain(|t| *t != change.tag);
                    }
                }
            }
            if !changes.is_empty() {
                if let Err(error) = vixen_io::system::save_system_config_file(&system, &core.paths) {
                    t.report_problem(format!("Couldn't save the system configuration: {error}"));
                }
            }
        })
    }

    /// After the tag manager saved: the tags as they are now, then labels and the hidden row filter again. F38.
    pub fn tags_changed(&self) -> TimelineUpdate {
        // Read and let go of the system first: `toggle_tag_on_selected` takes the two locks the other way round.
        let (tags, node_tags) = {
            let system = self.session.core.system.read();
            let node_tags: HashMap<Uuid, Vec<Uuid>> = system.tree.all_nodes().map(|(_, n)| (n.id, n.tags.clone())).collect();
            (tag_displays(&system), node_tags)
        };
        self.update_only(|t| t.reload_tags(tags, &|node| node_tags.get(&node).cloned()))
    }

    // Selection

    /// F57's order.
    pub fn selected_elements(&self) -> Vec<u64> {
        self.timeline.lock().selected_elements().iter().map(|id| id.0).collect()
    }

    pub fn select_element(&self, id: u64) -> TimelineUpdate {
        self.update_only(|t| t.select_element(EffectId(id)))
    }

    pub fn select_elements(&self, ids: Vec<u64>) -> TimelineUpdate {
        let ids: Vec<EffectId> = ids.into_iter().map(EffectId).collect();
        self.update_only(|t| t.select_elements(&ids))
    }

    pub fn deselect_element(&self, id: u64) -> TimelineUpdate {
        self.update_only(|t| t.deselect_element(EffectId(id)))
    }

    pub fn toggle_element(&self, id: u64) -> TimelineUpdate {
        self.update_only(|t| t.toggle_element(EffectId(id)))
    }

    pub fn select_all(&self) -> TimelineUpdate {
        self.update_only(|t| t.select_all())
    }

    pub fn clear_selection(&self) -> TimelineUpdate {
        self.update_only(|t| t.clear_selection())
    }

    /// F90.
    pub fn reveal_elements(&self, ids: Vec<u64>) -> TimelineUpdate {
        let ids: Vec<EffectId> = ids.into_iter().map(EffectId).collect();
        self.update_only(|t| t.reveal_elements(&ids))
    }

    /// F53.
    pub fn set_drag_box_filter(&self, enabled: bool, types: Vec<String>) -> TimelineUpdate {
        let types = types.iter().filter_map(|t| Uuid::parse_str(t).ok()).collect();
        self.update_only(|t| t.set_drag_box_filter(vixen_session::timeline::model::DragBoxFilter { enabled, types }))
    }

    // Hit tests

    /// The first effect at a client point, for keys that act on the effect under the pointer.
    pub fn element_at_client(&self, x: i32, y: i32) -> Option<u64> {
        let t = self.timeline.lock();
        t.element_at(t.to_canvas((x, y))).map(|id| id.0)
    }

    pub fn elements_at_client(&self, x: i32, y: i32) -> Vec<u64> {
        let t = self.timeline.lock();
        t.elements_at(t.to_canvas((x, y))).into_iter().map(|id| id.0).collect()
    }

    /// The row at a client y.
    pub fn row_at_client(&self, y: i32) -> Option<u32> {
        let t = self.timeline.lock();
        t.rows.row_at(y + t.vertical_offset).map(|r| r.0)
    }

    pub fn time_at_client(&self, x: i32) -> i64 {
        let t = self.timeline.lock();
        t.time(t.to_canvas((x, 0)).0).ticks()
    }

    /// F62, F89: a Deprecated row refuses effects.
    pub fn row_is_deprecated(&self, row: u32) -> bool {
        self.timeline.lock().rows.row(RowId(row)).has_deprecated_tag()
    }

    /// The display element a row shows, and its name.
    pub fn row_target(&self, row: u32) -> TimelineRowTarget {
        let t = self.timeline.lock();
        let r = t.rows.row(RowId(row));
        TimelineRowTarget { node: r.node_uuid.to_string(), name: r.name.clone() }
    }

    // STUDIO-BEGIN timeline-selected-targets: the selected rows' elements, the targets of Studio commands such as Place Effect on Marks
    /// The display elements of the selected rows, in row order without repeats, or the active row's when none is selected.
    pub fn studio_selected_targets(&self) -> Vec<TimelineRowTarget> {
        let t = self.timeline.lock();
        let mut rows: Vec<_> = t.rows.rows.iter().filter(|r| r.selected).collect();
        if rows.is_empty() {
            rows.extend(t.active_row().map(|id| t.rows.row(id)));
        }
        let mut seen = std::collections::HashSet::new();
        rows.into_iter()
            .filter(|r| seen.insert(r.node_uuid))
            .map(|r| TimelineRowTarget { node: r.node_uuid.to_string(), name: r.name.clone() })
            .collect()
    }
    // STUDIO-END timeline-selected-targets

    /// `SelectedRow ?? ActiveRow ?? TopVisibleRow`. F27.
    /// F27, paste: `SelectedRow ?? ActiveRow ?? TopVisibleRow`.
    pub fn paste_target_row(&self) -> Option<u32> {
        self.timeline.lock().paste_target_row().map(|r| r.0)
    }

    /// F27, Edit > Add Effect: `ActiveRow ?? SelectedRow`.
    pub fn add_effect_target_row(&self) -> Option<u32> {
        self.timeline.lock().add_effect_target_row().map(|r| r.0)
    }

    pub fn active_row(&self) -> Option<u32> {
        self.timeline.lock().active_row().map(|r| r.0)
    }

    /// `ElementsAtTime`, `ElementsWithinRange`.
    pub fn elements_at_time(&self, time: i64) -> Vec<u64> {
        self.timeline.lock().elements_at_time(TimeSpan(time)).into_iter().map(|id| id.0).collect()
    }

    pub fn elements_within(&self, start: i64, end: i64, visible_only: bool) -> Vec<u64> {
        self.timeline.lock().elements_within(TimeSpan(start), TimeSpan(end), visible_only).into_iter().map(|id| id.0).collect()
    }

    /// F45's box position, from the measured text height.
    pub fn hover_box_origin(&self, rect: TimelineRect, text_height: i32, clip_x: i32, clip_y: i32) -> Vec<i32> {
        let (x, y) = vixen_session::timeline::model::hover_box_origin(IRect::new(rect.x, rect.y, rect.w, rect.h), text_height, clip_x, clip_y);
        vec![x, y]
    }

    /// F43: percent of queued renders done. 100 hides the status bar's progress control.
    pub fn render_progress(&self) -> u8 {
        let counters = self.counters();
        let queued = counters.queued.load(Ordering::Relaxed);
        let finished = counters.finished.load(Ordering::Relaxed);
        if queued == 0 || finished >= queued {
            100
        } else {
            (finished * 100 / queued) as u8
        }
    }

    // Commands on effects. Each is one undo entry unless the spec says otherwise, and `requests` carries `UndoRecorded`.

    /// F80. `reference` is the effect under the press or pointer, and with none nothing happens.
    pub fn align(&self, kind: TimelineAlign, reference: Option<u64>, hold: bool) -> TimelineUpdate {
        self.with_host(|t, host| {
            let selected = t.selected_list();
            let kind = match kind {
                TimelineAlign::Start => AlignKind::Start,
                TimelineAlign::End => AlignKind::End,
                TimelineAlign::Both => AlignKind::Both,
                TimelineAlign::Durations => AlignKind::Durations,
                TimelineAlign::StartToEnd => AlignKind::StartToEnd,
                TimelineAlign::EndToStart => AlignKind::EndToStart,
                TimelineAlign::Centers => AlignKind::Centers,
            };
            t.align(kind, &selected, reference.map(EffectId), hold, host);
            t.take_outcome()
        })
    }

    /// F81, for the sequencer's Distribute and friends: `(id, new start, new end)` as parallel lists.
    pub fn move_resize_batch(&self, ids: Vec<u64>, starts: Vec<i64>, ends: Vec<i64>, kind_word: String) -> TimelineUpdate {
        self.with_host(|t, host| {
            let moves: Vec<(ElementId, TimeSpan, TimeSpan)> =
                ids.iter().zip(starts.iter().zip(ends.iter())).map(|(id, (s, e))| (EffectId(*id), TimeSpan(*s), TimeSpan(*e))).collect();
            t.move_resize_batch(&moves, move_kind_named(&kind_word), host);
            t.take_outcome()
        })
    }

    pub fn move_resize_element(&self, id: u64, start: i64, duration: i64, kind_word: String) -> TimelineUpdate {
        self.with_host(|t, host| {
            t.move_resize_element(EffectId(id), TimeSpan(start), TimeSpan(duration), move_kind_named(&kind_word), host);
            t.take_outcome()
        })
    }

    /// F84, at a time.
    pub fn split_at(&self, ids: Vec<u64>, time: i64) -> TimelineUpdate {
        self.with_host(|t, host| {
            let ids: Vec<EffectId> = ids.into_iter().map(EffectId).collect();
            t.split_at(&ids, TimeSpan(time), host);
            t.take_outcome()
        })
    }

    /// ⌘D: the selection, at the pointer's time.
    pub fn split_selected_at(&self, time: i64) -> TimelineUpdate {
        self.with_host(|t, host| {
            t.split_selected_at(TimeSpan(time), host);
            t.take_outcome()
        })
    }

    /// F85. With nothing selected and `confirmed` false, nothing happens and this is false: ask the question first.
    pub fn close_gaps_needs_confirmation(&self) -> bool {
        self.timeline.lock().selected_list().is_empty()
    }

    pub fn close_gaps(&self, threshold_seconds: f64, confirmed: bool) -> TimelineUpdate {
        self.with_host(|t, host| {
            t.close_gaps(threshold_seconds, confirmed, host);
            t.take_outcome()
        })
    }

    /// F87: ⌘← and ⌘→.
    pub fn nudge_selected(&self, direction: i32) -> TimelineUpdate {
        self.with_host(|t, host| {
            t.nudge_selected(direction, host);
            t.take_outcome()
        })
    }

    /// F86. The offset that was used is `moved_by()` afterwards.
    pub fn move_in_range_by_time(&self, start: i64, end: i64, offset: i64, visible_only: bool, clip: bool) -> TimelineUpdate {
        self.with_host(|t, host| {
            t.move_in_range_by_time(TimeSpan(start), TimeSpan(end), TimeSpan(offset), visible_only, clip, host);
            t.take_outcome()
        })
    }

    /// ⌫ with effects selected. F134's order is the caller's: this first, then the marks.
    pub fn delete_selected_effects(&self) -> TimelineUpdate {
        self.with_host(|t, host| {
            t.delete_selected_effects(host);
            t.take_outcome()
        })
    }

    // Cursor and playback

    pub fn set_cursor(&self, time: i64) -> TimelineUpdate {
        self.update_only(|t| t.set_cursor(TimeSpan(time)))
    }

    /// A display tick while playing: F94, F95. Moves only layers, unless the view flips a page.
    pub fn follow_playback(&self, position: i64) -> TimelineUpdate {
        self.update_only(|t| t.follow_playback(TimeSpan(position)))
    }

    /// Playback ended: the remembered range comes back. F101, F103.
    pub fn playback_ended(&self) -> TimelineUpdate {
        self.update_only(|t| t.playback_ended())
    }

    /// `PlaybackStartTime ?? 0`, `PlaybackEndTime`. F105.
    pub fn playback_range(&self) -> Vec<Option<i64>> {
        let t = self.timeline.lock();
        vec![t.info.playback_start.map(TimeSpan::ticks), t.info.playback_end.map(TimeSpan::ticks)]
    }

    pub fn set_playback_range(&self, start: Option<i64>, end: Option<i64>) -> TimelineUpdate {
        self.update_only(|t| t.set_playback_range(start.map(TimeSpan), end.map(TimeSpan)))
    }

    /// The Start and End toolbar buttons. F105.
    pub fn playback_range_to_start(&self) -> TimelineUpdate {
        self.update_only(|t| t.playback_range_to_start())
    }

    pub fn playback_range_to_end(&self) -> TimelineUpdate {
        self.update_only(|t| t.playback_range_to_end())
    }

    // Waveform

    /// F140: call after the session's audio changes. A new one replaces the old, and peaks build on a worker.
    pub fn audio_changed(self: Arc<Self>) -> TimelineUpdate {
        let (audio, media) = {
            let s = self.session.session.lock();
            let audio = s.audio().cloned();
            let media = audio.as_ref().map(|c| TimeSpan::from_secs(c.frames() as f64 / c.sample_rate() as f64));
            (audio, media)
        };
        self.timeline.lock().set_peaks(None);
        let update = self.update_only(|t| t.set_audio(audio.is_some(), media.unwrap_or(TimeSpan::ZERO)));
        if let Some(clip) = audio {
            self.build_peaks(clip);
        }
        update
    }

    pub fn waveform_mouse_move(&self, y: i32, left_down: bool) -> TimelineCursorKind {
        self.timeline.lock().waveform_mouse_move(y, left_down).into()
    }

    pub fn waveform_double_click(&self) -> TimelineUpdate {
        self.update_only(|t| t.waveform_double_click())
    }

    // Options

    /// Everything the menus and toolbars set.
    pub fn settings(&self) -> TimelineSettings {
        let t = self.timeline.lock();
        TimelineSettings {
            snap_enabled: t.opts.snap_enabled,
            snap_strength: t.opts.snap_strength,
            draw_mode: t.opts.draw_mode,
            cad_style: t.opts.cad_style,
            legacy_cursor: t.opts.legacy_cursor,
            zoom_under_mouse: t.opts.zoom_under_mouse,
            highlight_rows: t.opts.highlight_rows,
            show_effect_info: t.opts.show_effect_info,
            resize_indicator: t.opts.resize_indicator,
            resize_indicator_color: t.opts.resize_indicator_color.name().to_string(),
            waveform_height: t.waveform.height,
            ruler_height: t.marks.ruler.height,
            lock_ruler_height: t.marks.ruler.locked,
            lock_waveform_height: t.waveform.locked,
            full_waveform: t.waveform.style == WaveformStyle::Full,
            splitter_distance: *self.splitter_distance.lock(),
            close_gap_threshold: self.close_gap_threshold.lock().clone(),
        }
    }

    pub fn apply_settings(&self, s: TimelineSettings) -> TimelineUpdate {
        *self.splitter_distance.lock() = s.splitter_distance;
        *self.close_gap_threshold.lock() = s.close_gap_threshold;
        self.update_only(|t| {
            t.opts.snap_enabled = s.snap_enabled;
            t.set_snap_strength(s.snap_strength);
            t.opts.draw_mode = s.draw_mode;
            t.opts.cad_style = s.cad_style;
            t.opts.legacy_cursor = s.legacy_cursor;
            t.opts.zoom_under_mouse = s.zoom_under_mouse;
            t.set_highlight_rows(s.highlight_rows);
            t.opts.show_effect_info = s.show_effect_info;
            t.opts.resize_indicator = s.resize_indicator;
            t.opts.resize_indicator_color = IndicatorColor::from_name(&s.resize_indicator_color);
            t.set_waveform_height(s.waveform_height);
            t.set_ruler_height(s.ruler_height);
            t.set_ruler_locked(s.lock_ruler_height);
            t.set_waveform_locked(s.lock_waveform_height);
            t.set_waveform_style(if s.full_waveform { WaveformStyle::Full } else { WaveformStyle::Half });
        })
    }

    /// `Grid.SelectedEffect`: the effect type a draw makes. F77.
    pub fn set_selected_effect_type(&self, type_id: Option<String>) -> TimelineUpdate {
        let id = type_id.and_then(|t| Uuid::parse_str(&t).ok());
        self.update_only(|t| t.set_selected_effect(id))
    }

    /// The profile's values under `TimedSequenceEditorForm/`, for a window with no settings of its own. The sequencer's own reader has them too.
    pub fn load_profile_settings(&self) -> TimelineUpdate {
        let p = ProfileSettings::shared();
        let d = self.settings();
        let section = SettingSection::AppSettings;
        let key = |name: &str| format!("{FORM}/{name}");
        let b = |name: &str, default: bool| p.get_bool(section, &key(name), default);
        let i = |name: &str, default: i32| p.get_i32(section, &key(name), default);
        self.apply_settings(TimelineSettings {
            snap_enabled: b("SnapToSelected", d.snap_enabled),
            snap_strength: i("SnapStrength", d.snap_strength),
            draw_mode: b("DrawModeSelected", d.draw_mode),
            cad_style: b("CadStyleSelectionBox", d.cad_style),
            legacy_cursor: b("LegacyCursorActiveRow", d.legacy_cursor),
            zoom_under_mouse: b("ZoomUnderMousePosition", d.zoom_under_mouse),
            highlight_rows: b("HighlightActiveElements", d.highlight_rows),
            show_effect_info: b("ShowEffectInfo", d.show_effect_info),
            resize_indicator: b("ResizeIndicatorEnabled", d.resize_indicator),
            resize_indicator_color: p.get_string(section, &key("ResizeIndicatorColor"), &d.resize_indicator_color),
            waveform_height: i("WaveFormHeight", d.waveform_height),
            ruler_height: i("RulerHeight", d.ruler_height),
            lock_ruler_height: b("LockRulerHeight", d.lock_ruler_height),
            lock_waveform_height: b("LockWaveformHeight", d.lock_waveform_height),
            full_waveform: b("FullWaveform", d.full_waveform),
            splitter_distance: i("SplitterDistance", d.splitter_distance),
            close_gap_threshold: p.get_string(section, &key("CloseGapThreshold"), &d.close_gap_threshold),
        })
    }

    /// Once, when the window closes.
    pub fn save_profile_settings(&self) {
        let p = ProfileSettings::shared();
        let s = self.settings();
        let section = SettingSection::AppSettings;
        let key = |name: &str| format!("{FORM}/{name}");
        let text = |v: bool| if v { "True".to_string() } else { "False".to_string() };
        let values: Vec<(String, String)> = vec![
            (key("SnapToSelected"), text(s.snap_enabled)),
            (key("SnapStrength"), s.snap_strength.to_string()),
            (key("CloseGapThreshold"), s.close_gap_threshold),
            (key("ResizeIndicatorEnabled"), text(s.resize_indicator)),
            (key("ResizeIndicatorColor"), s.resize_indicator_color),
            (key("DrawModeSelected"), text(s.draw_mode)),
            (key("SelectionModeSelected"), text(!s.draw_mode)),
            (key("CadStyleSelectionBox"), text(s.cad_style)),
            (key("LegacyCursorActiveRow"), text(s.legacy_cursor)),
            (key("ZoomUnderMousePosition"), text(s.zoom_under_mouse)),
            (key("HighlightActiveElements"), text(s.highlight_rows)),
            (key("WaveFormHeight"), s.waveform_height.to_string()),
            (key("RulerHeight"), s.ruler_height.to_string()),
            (key("LockRulerHeight"), text(s.lock_ruler_height)),
            (key("LockWaveformHeight"), text(s.lock_waveform_height)),
            (key("ShowEffectInfo"), text(s.show_effect_info)),
            (key("FullWaveform"), text(s.full_waveform)),
            (key("SplitterDistance"), s.splitter_distance.to_string()),
        ];
        let pairs: Vec<(&str, String)> = values.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
        let _ = p.put_many(section, &pairs);
    }

    // For tests and the status bar

    pub fn elements_info(&self) -> Vec<TimelineElementInfo> {
        let t = self.timeline.lock();
        let mut out: Vec<TimelineElementInfo> = t
            .elements
            .values()
            .map(|e| TimelineElementInfo {
                id: e.id.0,
                start: e.start.ticks(),
                duration: e.duration.ticks(),
                rows: e.rows.iter().map(|r| r.0).collect(),
                selected: e.selected,
                rendered: e.rendered,
                type_name: e.desc.type_name.to_string(),
            })
            .collect();
        out.sort_by_key(|e| e.id);
        out
    }

    pub fn marks_info(&self) -> Vec<TimelineMarkInfo> {
        let t = self.timeline.lock();
        let mut out = Vec::new();
        for c in &t.marks.collections {
            for m in &c.marks {
                out.push(TimelineMarkInfo {
                    id: m.id.to_string(),
                    collection: c.name.clone(),
                    start: m.start_time.ticks(),
                    duration: m.duration.ticks(),
                    text: m.text.clone(),
                    selected: t.marks.is_selected(m.id),
                });
            }
        }
        out
    }

    pub fn drag_state_name(&self) -> String {
        match self.timeline.lock().drag_state() {
            DragState::Normal => "Normal",
            DragState::Waiting => "Waiting",
            DragState::Moving => "Moving",
            DragState::Selecting => "Selecting",
            DragState::HResizing => "HResizing",
            DragState::Drawing => "Drawing",
        }
        .to_string()
    }
}

/// What a row is for, so a drop or a paste can target it.
#[derive(uniffi::Record, Clone)]
pub struct TimelineRowTarget {
    pub node: String,
    pub name: String,
}

/// F137's input.
#[derive(uniffi::Record, Clone)]
pub struct TimelineWordPhonemes {
    pub word: String,
    pub phonemes: Vec<String>,
}

fn move_kind_named(word: &str) -> vixen_session::undo::MoveKind {
    use vixen_session::undo::MoveKind::*;
    match word {
        "Resize" => Resize,
        "Start Align" => StartAlign,
        "End Align" => EndAlign,
        "Align" => Align,
        "Duration Align" => DurationAlign,
        "Start to End Align" => StartToEndAlign,
        "End to Start Align" => EndToStartAlign,
        "Center Align" => CenterAlign,
        "Distributed" => Distributed,
        _ => Moved,
    }
}

/// For the sequencer window's `GridPort`, which is Rust on both sides and so needs no export.
impl TimelineController {
    /// F86, with the offset that was used after clipping to the sequence.
    pub(crate) fn move_in_range_reporting(&self, start: i64, end: i64, offset: i64, visible_only: bool, clip: bool) -> (TimelineUpdate, i64) {
        let mut moved = offset;
        let update = self.with_host(|t, host| {
            moved = t.move_in_range_by_time(TimeSpan(start), TimeSpan(end), TimeSpan(offset), visible_only, clip, host).ticks();
            t.take_outcome()
        });
        (update, moved)
    }

    pub(crate) fn remembered_playback_range(&self) -> (Option<TimeSpan>, Option<TimeSpan>) {
        self.timeline.lock().remembered_playback_range()
    }

    pub(crate) fn ok_to_use_alignment(&self, elements: &[EffectId]) -> bool {
        self.timeline.lock().ok_to_use_alignment(elements)
    }
}
