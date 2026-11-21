//! The sequencer window's bindings: everything the shell needs from `vixen_session::sequencer`, as bulk reads and single commands. The timeline, effect
//! editor and marks have their own modules.

use crate::core::{FfiError, VixenCore};
use crate::sequence::SequencerSession;
use parking_lot::Mutex;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;
use vixen_core::TimeSpan;
use vixen_session::sequencer as seq;
use crate::kit::{CurveValue, GradientValue};
use vixen_session::sequence::EffectId;
use vixen_session::sequencer::clipboard::{ClipboardData, PastingMode, PASTEBOARD_COUNT_TYPE, PASTEBOARD_TYPE};
use vixen_session::sequencer::dialogs::{possible_label, AddMultipleOk, AddMultipleState, EffectTimeEditorState};
use vixen_session::sequencer::drops::{plan_file_drop, DropPlan, DropRequest, DropValue, FileDropItem, PickerChoice, PickerImage};
use vixen_session::sequencer::effect_ops::{DistributeParams, DurationMode, MarkAlign, PlacementMode};
use vixen_session::sequencer::find::FindMode;
use vixen_session::sequencer::libraries::{self as libs, CollectionEditor, ColorCollection, NewCollection};
use vixen_session::sequencer::{layers, time_box, AddAudioOutcome};
use vixen_session::sequencer::keys::{Key, Mods};
use vixen_session::sequencer::lifecycle::CloseAction;
use vixen_session::sequencer::panels::{DockSide, PanelId};
use vixen_session::sequencer::toolbars::StripId;
use vixen_session::sequencer::transport::{speed_gauge, DelayOption, PlayIcon};

fn parse_uuid(text: &str) -> Option<Uuid> {
    Uuid::parse_str(text).ok()
}

fn time_from_ms(ms: f64) -> TimeSpan {
    TimeSpan::from_ms(ms)
}

// ------------------------------------------------------------------------------------------------------------------
// Records
// ------------------------------------------------------------------------------------------------------------------

/// `Keys` as the window hears them.
#[derive(uniffi::Enum, Clone, PartialEq, Eq)]
pub enum ShellKey {
    Home,
    End,
    PageUp,
    PageDown,
    Up,
    Down,
    Left,
    Right,
    Space,
    Escape,
    Minus,
    Equals,
    Delete,
    F5,
    F8,
    F9,
    Letter { value: String },
}

impl ShellKey {
    fn key(&self) -> Option<Key> {
        Some(match self {
            ShellKey::Home => Key::Home,
            ShellKey::End => Key::End,
            ShellKey::PageUp => Key::PageUp,
            ShellKey::PageDown => Key::PageDown,
            ShellKey::Up => Key::Up,
            ShellKey::Down => Key::Down,
            ShellKey::Left => Key::Left,
            ShellKey::Right => Key::Right,
            ShellKey::Space => Key::Space,
            ShellKey::Escape => Key::Escape,
            ShellKey::Minus => Key::Minus,
            ShellKey::Equals => Key::Equals,
            ShellKey::Delete => Key::Delete,
            ShellKey::F5 => Key::F5,
            ShellKey::F8 => Key::F8,
            ShellKey::F9 => Key::F9,
            ShellKey::Letter { value } => Key::Letter(value.chars().next()?.to_ascii_lowercase()),
        })
    }
}

#[derive(uniffi::Enum, Copy, Clone, PartialEq, Eq)]
pub enum ShellDelay {
    Off,
    Seconds5,
    Seconds10,
    Seconds20,
    Seconds30,
    Seconds60,
}

impl From<ShellDelay> for DelayOption {
    fn from(d: ShellDelay) -> DelayOption {
        match d {
            ShellDelay::Off => DelayOption::Off,
            ShellDelay::Seconds5 => DelayOption::Seconds5,
            ShellDelay::Seconds10 => DelayOption::Seconds10,
            ShellDelay::Seconds20 => DelayOption::Seconds20,
            ShellDelay::Seconds30 => DelayOption::Seconds30,
            ShellDelay::Seconds60 => DelayOption::Seconds60,
        }
    }
}

impl From<DelayOption> for ShellDelay {
    fn from(d: DelayOption) -> ShellDelay {
        match d {
            DelayOption::Off => ShellDelay::Off,
            DelayOption::Seconds5 => ShellDelay::Seconds5,
            DelayOption::Seconds10 => ShellDelay::Seconds10,
            DelayOption::Seconds20 => ShellDelay::Seconds20,
            DelayOption::Seconds30 => ShellDelay::Seconds30,
            DelayOption::Seconds60 => ShellDelay::Seconds60,
        }
    }
}

#[derive(uniffi::Enum, Copy, Clone, PartialEq, Eq)]
pub enum ShellPlayIcon {
    Play,
    Pause,
    Hourglass,
}

/// `PlayBack` strip, Playback menu and the status bar's delay label. F268.
#[derive(uniffi::Record)]
pub struct ShellTransport {
    pub play_enabled: bool,
    pub play_menu_enabled: bool,
    pub play_has_shortcut: bool,
    pub stop_enabled: bool,
    pub pause_menu_enabled: bool,
    pub icon: ShellPlayIcon,
    pub play_title: String,
    pub play_tooltip: String,
    pub delay_visible: bool,
    pub delay_text: String,
    pub delay: ShellDelay,
    pub looping: bool,
    pub running: bool,
    pub paused: bool,
}

/// `audioToolStripLabel_TimingSpeed` and the +/- buttons. F129 to F133.
#[derive(uniffi::Record)]
pub struct ShellSpeed {
    pub speed: f32,
    pub decrease_enabled: bool,
    pub increase_enabled: bool,
    pub gauge_enabled: bool,
    pub gauge_text: String,
    pub gauge_text_x: i32,
    pub gauge_red: u8,
    pub gauge_green: u8,
    pub gauge_blue: u8,
    pub needle_end_x: i32,
    pub needle_end_y: i32,
}

#[derive(uniffi::Record)]
pub struct ShellPrefs {
    pub auto_save: bool,
    pub snap_to: bool,
    pub snap_strength: i32,
    pub close_gap_threshold: String,
    pub align_to_threshold: String,
    pub resize_indicator: bool,
    pub resize_indicator_color: String,
    pub cad_style: bool,
    pub legacy_cursor: bool,
    pub zoom_under_mouse: bool,
    pub highlight_rows: bool,
    pub show_effect_info: bool,
    pub full_waveform: bool,
    pub lock_ruler_height: bool,
    pub lock_waveform_height: bool,
    pub use_tempo_for_speed: bool,
    pub effect_label_index: String,
    pub basic_effects: bool,
    pub pixel_effects: bool,
    pub device_effects: bool,
    pub fixture_effects: bool,
    pub curve_link_curves: bool,
    pub gradient_link_gradients: bool,
}

/// Everything menus, strips and the status bar read, in one call.
#[derive(uniffi::Record)]
pub struct ShellUiState {
    pub title: String,
    pub sequence_name: String,
    pub profile_name: String,
    pub file_path: Option<String>,
    pub modified: bool,

    pub transport: ShellTransport,
    pub speed: ShellSpeed,

    pub has_selection: bool,
    pub selection_count: u32,
    pub paste_enabled: bool,
    pub paste_marks_enabled: bool,
    pub paste_invert_enabled: bool,
    pub paste_special_enabled: bool,
    pub undo_name: Option<String>,
    pub redo_name: Option<String>,
    pub remove_audio_enabled: bool,
    pub beat_bar_enabled: bool,
    pub speed_tempo_enabled: bool,
    pub audio_tooltip: String,
    pub timeline_title: String,
    pub effect_count: u32,

    pub prefs: ShellPrefs,
    pub draw_mode: bool,
    pub selection_mode: bool,
    pub show_hidden_rows: bool,
    pub drag_box_filter: bool,
    pub drag_box_invert_checked: bool,
    pub drag_box_filter_types: Vec<String>,
    pub selected_effect: Option<String>,

    pub position_text: String,
    pub length_text: String,
    pub status_message: String,
    pub rendering_visible: bool,
    pub rendering_percent: i32,
}

/// The toggles in the menus and strips.
#[derive(uniffi::Enum, Copy, Clone, PartialEq, Eq)]
pub enum ShellOption {
    AutoSave,
    SnapTo,
    ResizeIndicator,
    CadStyle,
    LegacyCursor,
    ZoomUnderMouse,
    HighlightRows,
    ShowEffectInfo,
    ShowHiddenRows,
    FullWaveform,
    LockRulerHeight,
    LockWaveformHeight,
    SpeedTempo,
    DragBoxFilter,
    CurveLinkCurves,
    GradientLinkGradients,
    BasicEffects,
    PixelEffects,
    DeviceEffects,
    FixtureEffects,
}

/// Menu items and buttons with nothing to say but "do it".
#[derive(uniffi::Enum, Copy, Clone, PartialEq, Eq)]
pub enum ShellCommand {
    PlayPause,
    Stop,
    ToggleLoop,
    GotoStart,
    GotoEnd,
    ZoomTimeIn,
    ZoomTimeOut,
    ZoomRowsIn,
    ZoomRowsOut,
    CollapseAll,
    ResetRowHeight,
    SelectAll,
    DeleteSelected,
    DrawMode,
    SelectionMode,
    SpeedDecrease,
    SpeedIncrease,
    SpeedReset,
    SpeedMax,
    SpeedWheelUp,
    SpeedWheelDown,
    InvertDragBoxFilter,
    AlignStartToMarks,
    AlignEndToMarks,
    AlignBothToMarks,
    DistributeEqually,
    DistributeDialog,
    CloseGaps,
}

#[derive(uniffi::Enum, Copy, Clone, PartialEq, Eq)]
pub enum ShellMarkAlign {
    Start,
    End,
    Both,
}

impl From<ShellMarkAlign> for MarkAlign {
    fn from(a: ShellMarkAlign) -> MarkAlign {
        match a {
            ShellMarkAlign::Start => MarkAlign::Start,
            ShellMarkAlign::End => MarkAlign::End,
            ShellMarkAlign::Both => MarkAlign::Both,
        }
    }
}

impl From<MarkAlign> for ShellMarkAlign {
    fn from(a: MarkAlign) -> ShellMarkAlign {
        match a {
            MarkAlign::Start => ShellMarkAlign::Start,
            MarkAlign::End => ShellMarkAlign::End,
            MarkAlign::Both => ShellMarkAlign::Both,
        }
    }
}

/// What a prompt's default button does.
#[derive(uniffi::Enum, Clone, PartialEq, Eq)]
pub enum ShellAlertAction {
    AlignToMarksEverywhere { kind: ShellMarkAlign },
    RemoveAudio,
    AssociateAudio,
    AddMarksEverywhere,
    CreateDefaultMarks,
    EnableDefaultMarks,
    ReplaceSelected { type_id: String },
    CloneToSelected { source: u64 },
    ContinueDrop,
    CloseGapsEverywhere,
}

#[derive(uniffi::Enum, Copy, Clone, PartialEq, Eq)]
pub enum ShellAlertStyle {
    Information,
    Warning,
    Error,
}

#[derive(uniffi::Record, Clone)]
pub struct ShellAlert {
    pub title: String,
    pub message: String,
    pub style: ShellAlertStyle,
    /// The default button's verb, when it does something. With none, it's an acknowledgement.
    pub confirm_label: Option<String>,
    pub cancel_label: Option<String>,
    pub action: Option<ShellAlertAction>,
}

impl From<seq::Alert> for ShellAlert {
    fn from(a: seq::Alert) -> ShellAlert {
        let style = match a.style {
            seq::AlertStyle::Information => ShellAlertStyle::Information,
            seq::AlertStyle::Warning => ShellAlertStyle::Warning,
            seq::AlertStyle::Error => ShellAlertStyle::Error,
        };
        let (confirm_label, cancel_label, action) = match a.confirm {
            Some(c) => {
                let action = match c.action {
                    seq::AlertAction::AlignToMarksEverywhere(kind) => ShellAlertAction::AlignToMarksEverywhere { kind: kind.into() },
                    seq::AlertAction::RemoveAudio => ShellAlertAction::RemoveAudio,
                    seq::AlertAction::AssociateAudio => ShellAlertAction::AssociateAudio,
                    seq::AlertAction::AddMarksEverywhere => ShellAlertAction::AddMarksEverywhere,
                    seq::AlertAction::CreateDefaultMarks => ShellAlertAction::CreateDefaultMarks,
                    seq::AlertAction::EnableDefaultMarks => ShellAlertAction::EnableDefaultMarks,
                    seq::AlertAction::ReplaceSelected(type_id) => ShellAlertAction::ReplaceSelected { type_id: type_id.to_string() },
                    seq::AlertAction::CloneToSelected(source) => ShellAlertAction::CloneToSelected { source: source.0 },
                    seq::AlertAction::ContinueDrop => ShellAlertAction::ContinueDrop,
                    seq::AlertAction::CloseGapsEverywhere => ShellAlertAction::CloseGapsEverywhere,
                };
                (Some(c.label), Some(c.cancel_label), Some(action))
            }
            None => (None, None, None),
        };
        ShellAlert { title: a.title, message: a.message, style, confirm_label, cancel_label, action }
    }
}

/// Something Swift should raise.
#[derive(uniffi::Enum, Clone)]
pub enum ShellRequest {
    DistributeDialog { count: u32, start_ms: f64, end_ms: f64, length_ms: f64 },
    ClearEffectsSelection,
}

/// What came of a command.
#[derive(uniffi::Record, Clone)]
pub struct ShellOutcome {
    pub alert: Option<ShellAlert>,
    pub request: Option<ShellRequest>,
    /// A cut or copy made this, for the pasteboard.
    pub clipboard_json: Option<String>,
    pub clipboard_count: u32,
    pub handled: bool,
}

impl From<seq::CommandOutcome> for ShellOutcome {
    fn from(o: seq::CommandOutcome) -> ShellOutcome {
        let clipboard_count = o.clipboard.as_ref().map_or(0, |c| c.effects.len() as u32);
        ShellOutcome {
            alert: o.alert.map(Into::into),
            request: o.request.map(|r| match r {
                seq::UiRequest::DistributeDialog { count, start, end, length } => ShellRequest::DistributeDialog {
                    count,
                    start_ms: start.total_ms(),
                    end_ms: end.total_ms(),
                    length_ms: length.total_ms(),
                },
                seq::UiRequest::ClearEffectsSelection => ShellRequest::ClearEffectsSelection,
            }),
            clipboard_json: o.clipboard.map(|c| c.to_json()),
            clipboard_count,
            handled: o.handled,
        }
    }
}

fn empty_outcome() -> ShellOutcome {
    ShellOutcome { alert: None, request: None, clipboard_json: None, clipboard_count: 0, handled: true }
}

/// `PastingMode`.
#[derive(uniffi::Enum, Copy, Clone, PartialEq, Eq)]
pub enum ShellPasteMode {
    Default,
    Invert,
    VisibleMarks,
}

impl From<ShellPasteMode> for PastingMode {
    fn from(m: ShellPasteMode) -> PastingMode {
        match m {
            ShellPasteMode::Default => PastingMode::Default,
            ShellPasteMode::Invert => PastingMode::Invert,
            ShellPasteMode::VisibleMarks => PastingMode::VisibleMarks,
        }
    }
}

/// What the Save items should do.
#[derive(uniffi::Enum, Clone)]
pub enum ShellSaveRoute {
    InPlace { path: String },
    Panel { directory: String, suggested_name: String },
}

/// `_CloseEditor`.
#[derive(uniffi::Enum, Copy, Clone, PartialEq, Eq)]
pub enum ShellCloseAction {
    Prompt { can_cancel: bool },
    SaveSilently,
    Close,
}

/// `ToolStrip` by id.
#[derive(uniffi::Enum, Copy, Clone, PartialEq, Eq, Hash)]
pub enum ShellStrip {
    Alignment,
    Audio,
    Edit,
    Effects,
    File,
    LibraryColor,
    LibraryCurve,
    LibraryGradient,
    Mode,
    PlayBack,
    Tools,
    View,
}

impl From<ShellStrip> for StripId {
    fn from(s: ShellStrip) -> StripId {
        match s {
            ShellStrip::Alignment => StripId::Alignment,
            ShellStrip::Audio => StripId::Audio,
            ShellStrip::Edit => StripId::Edit,
            ShellStrip::Effects => StripId::Effects,
            ShellStrip::File => StripId::File,
            ShellStrip::LibraryColor => StripId::LibraryColor,
            ShellStrip::LibraryCurve => StripId::LibraryCurve,
            ShellStrip::LibraryGradient => StripId::LibraryGradient,
            ShellStrip::Mode => StripId::Mode,
            ShellStrip::PlayBack => StripId::PlayBack,
            ShellStrip::Tools => StripId::Tools,
            ShellStrip::View => StripId::View,
        }
    }
}

impl From<StripId> for ShellStrip {
    fn from(s: StripId) -> ShellStrip {
        match s {
            StripId::Alignment => ShellStrip::Alignment,
            StripId::Audio => ShellStrip::Audio,
            StripId::Edit => ShellStrip::Edit,
            StripId::Effects => ShellStrip::Effects,
            StripId::File => ShellStrip::File,
            StripId::LibraryColor => ShellStrip::LibraryColor,
            StripId::LibraryCurve => ShellStrip::LibraryCurve,
            StripId::LibraryGradient => ShellStrip::LibraryGradient,
            StripId::Mode => ShellStrip::Mode,
            StripId::PlayBack => ShellStrip::PlayBack,
            StripId::Tools => ShellStrip::Tools,
            StripId::View => ShellStrip::View,
        }
    }
}

#[derive(uniffi::Record, Clone)]
pub struct ShellStripInfo {
    pub strip: ShellStrip,
    pub title: String,
    pub name: String,
    pub visible: bool,
    pub large: bool,
}

/// `TopToolStripPanel.Rows`, with the Toolbars list. F98, F99.
#[derive(uniffi::Record, Clone)]
pub struct ShellToolbars {
    pub rows: Vec<Vec<ShellStripInfo>>,
    /// Every strip, alphabetically, for the Toolbars menus.
    pub list: Vec<ShellStripInfo>,
    /// Hidden items as `strip name/item name`.
    pub hidden_items: Vec<String>,
}

#[derive(uniffi::Record, Clone)]
pub struct ShellStripItem {
    pub name: String,
    pub tooltip: String,
    pub visible: bool,
}

#[derive(uniffi::Enum, Copy, Clone, PartialEq, Eq, Hash)]
pub enum ShellPanel {
    Effects,
    EffectEditor,
    ColorLibrary,
    CurveLibrary,
    GradientLibrary,
    Marks,
    Layers,
    FindEffects,
}

impl From<ShellPanel> for PanelId {
    fn from(p: ShellPanel) -> PanelId {
        match p {
            ShellPanel::Effects => PanelId::Effects,
            ShellPanel::EffectEditor => PanelId::EffectEditor,
            ShellPanel::ColorLibrary => PanelId::ColorLibrary,
            ShellPanel::CurveLibrary => PanelId::CurveLibrary,
            ShellPanel::GradientLibrary => PanelId::GradientLibrary,
            ShellPanel::Marks => PanelId::Marks,
            ShellPanel::Layers => PanelId::Layers,
            ShellPanel::FindEffects => PanelId::FindEffects,
        }
    }
}

impl From<PanelId> for ShellPanel {
    fn from(p: PanelId) -> ShellPanel {
        match p {
            PanelId::Effects => ShellPanel::Effects,
            PanelId::EffectEditor => ShellPanel::EffectEditor,
            PanelId::ColorLibrary => ShellPanel::ColorLibrary,
            PanelId::CurveLibrary => ShellPanel::CurveLibrary,
            PanelId::GradientLibrary => ShellPanel::GradientLibrary,
            PanelId::Marks => ShellPanel::Marks,
            PanelId::Layers => ShellPanel::Layers,
            PanelId::FindEffects => ShellPanel::FindEffects,
        }
    }
}

#[derive(uniffi::Enum, Copy, Clone, PartialEq, Eq)]
pub enum ShellSide {
    Left,
    Right,
}

impl From<ShellSide> for DockSide {
    fn from(s: ShellSide) -> DockSide {
        match s {
            ShellSide::Left => DockSide::Left,
            ShellSide::Right => DockSide::Right,
        }
    }
}

#[derive(uniffi::Record, Clone)]
pub struct ShellPanelTab {
    pub panel: ShellPanel,
    pub title: String,
}

/// The dock layout. F2, F68 to F77.
#[derive(uniffi::Record, Clone)]
pub struct ShellPanels {
    pub left_tabs: Vec<ShellPanelTab>,
    pub right_tabs: Vec<ShellPanelTab>,
    pub floating: Vec<ShellPanelTab>,
    pub selected_left: Option<ShellPanel>,
    pub selected_right: Option<ShellPanel>,
    pub left_width: f64,
    pub right_width: f64,
    /// The View menu's checks: which panels are showing, in the dock or floating.
    pub visible: Vec<ShellPanel>,
}

#[derive(uniffi::Enum, Copy, Clone, PartialEq, Eq)]
pub enum ShellEffectGroup {
    Basic,
    Pixel,
    Device,
    IntelligentFixture,
}

/// One effect, for the Effects panel, the Effects strip, Add Effect and the drag box filter.
#[derive(uniffi::Record, Clone)]
pub struct ShellEffect {
    pub type_id: String,
    pub name: String,
    pub group: ShellEffectGroup,
    /// File name in `Resources/effects`, without the extension.
    pub icon: String,
}

/// F8, F10, F11.
#[derive(uniffi::Record, Clone)]
pub struct ShellLoadReport {
    pub rename_summary: Option<String>,
    pub deprecated_message: Option<String>,
}

#[derive(uniffi::Record, Clone)]
pub struct ShellUnmappedTarget {
    pub id: String,
    pub name: String,
}

#[derive(uniffi::Record, Clone)]
pub struct ShellElementMapEntry {
    pub source: String,
    pub target: String,
}

/// A snapshot to write off the main thread. Hazard 24.
#[derive(uniffi::Object)]
pub struct ShellPreparedSave {
    inner: seq::PreparedSave,
}

#[uniffi::export]
impl ShellPreparedSave {
    pub fn target_path(&self) -> String {
        self.inner.target().to_string_lossy().into_owned()
    }

    pub fn write(&self) -> Result<(), FfiError> {
        Ok(self.inner.write()?)
    }
}

/// The custom pasteboard types. F315.
#[derive(uniffi::Record)]
pub struct ShellPasteboardTypes {
    pub data: String,
    pub count: String,
}

#[uniffi::export]
pub fn sequencer_pasteboard_types() -> ShellPasteboardTypes {
    ShellPasteboardTypes { data: PASTEBOARD_TYPE.to_string(), count: PASTEBOARD_COUNT_TYPE.to_string() }
}

/// Every effect, in descriptor order.
#[uniffi::export]
pub fn sequencer_effect_catalog() -> Vec<ShellEffect> {
    use vixen_session::sequencer::EffectGroupPublic as G;
    seq::effect_catalog()
        .into_iter()
        .map(|e| ShellEffect {
            type_id: e.type_id.to_string(),
            name: e.name.to_string(),
            group: match e.group {
                G::Basic => ShellEffectGroup::Basic,
                G::Pixel => ShellEffectGroup::Pixel,
                G::Device => ShellEffectGroup::Device,
                G::IntelligentFixture => ShellEffectGroup::IntelligentFixture,
            },
            icon: e.icon,
        })
        .collect()
}

// ------------------------------------------------------------------------------------------------------------------
// The window's object
// ------------------------------------------------------------------------------------------------------------------

/// One sequencer window's state. Holds the session it edits, and `SequencerSession` is what the other parts of the window share.
#[derive(uniffi::Object)]
pub struct SequencerShell {
    pub session: Arc<SequencerSession>,
    state: Mutex<seq::SequencerState>,
    /// The import plan the Layers panel last showed, and the file it came from: the import takes this one, not a second read.
    layer_plan: Mutex<Option<(String, layers::LayerImportPlan)>>,
    #[cfg(feature = "timeline")]
    timeline: Mutex<Option<with_timeline::TimelineLink>>,
}

#[uniffi::export]
impl SequencerSession {
    /// The shell's state over this session. One per window.
    pub fn make_shell(self: Arc<Self>) -> Arc<SequencerShell> {
        let state = seq::SequencerState::attach(Arc::clone(&self.core), Arc::clone(&self.session));
        Arc::new(SequencerShell {
            session: self,
            state: Mutex::new(state),
            layer_plan: Mutex::new(None),
            #[cfg(feature = "timeline")]
            timeline: Mutex::new(None),
        })
    }
}

#[uniffi::export]
impl VixenCore {
    /// The profile's sequence folder, where a save panel starts.
    pub fn sequence_directory(&self) -> String {
        self.inner.paths.sequence_dir().to_string_lossy().into_owned()
    }
}

#[uniffi::export]
impl SequencerShell {
    // Opening

    pub fn unmapped_targets(&self) -> Vec<ShellUnmappedTarget> {
        self.state.lock().unmapped_targets().into_iter().map(|(id, name)| ShellUnmappedTarget { id: id.to_string(), name }).collect()
    }

    /// F12's answers: `None` is "No". A map from the mapper, with an empty one meaning the mapper was cancelled.
    pub fn apply_element_map(&self, map: Option<Vec<ShellElementMapEntry>>) {
        let map: Option<Vec<(Uuid, Uuid)>> =
            map.map(|m| m.iter().filter_map(|e| Some((parse_uuid(&e.source)?, parse_uuid(&e.target)?))).collect());
        self.state.lock().apply_element_map(map.as_deref());
    }

    pub fn finish_load(&self) {
        self.state.lock().finish_load();
    }

    pub fn load_report(&self) -> ShellLoadReport {
        let state = self.state.lock();
        let report = state.load_report();
        ShellLoadReport {
            rename_summary: (!report.renames.is_empty()).then(|| seq::lifecycle::mark_rename_summary(&report.renames)),
            deprecated_message: (!report.deprecated.is_empty()).then(|| seq::lifecycle::deprecated_elements_message(&report.deprecated)),
        }
    }

    pub fn missing_audio(&self) -> Option<String> {
        self.state.lock().missing_audio().map(|p| p.to_string_lossy().into_owned())
    }

    // What the window shows

    pub fn ui_state(&self) -> ShellUiState {
        let mut state = self.state.lock();
        let invert = state.drag_box_invert_checked();
        ui_state_record(state.ui_state(), invert)
    }

    /// The low-rate tick. Returns whether to read `ui_state` again, and whether auto save has come due.
    pub fn poll(&self) -> ShellPoll {
        let result = self.state.lock().poll(Instant::now());
        ShellPoll { changed: result.changed, auto_save_due: result.auto_save_due, transport_active: self.state.lock().transport_active() }
    }

    /// The display-rate tick, only while playing or counting down. Returns whether the rest of `ui_state` changed.
    pub fn transport_tick(&self) -> bool {
        self.state.lock().transport_tick(Instant::now())
    }

    pub fn position_text(&self) -> String {
        self.state.lock().position_text().to_string()
    }

    // Commands

    pub fn run(&self, command: ShellCommand) -> ShellOutcome {
        let mut state = self.state.lock();
        let now = Instant::now();
        match command {
            ShellCommand::PlayPause => state.play_pause(now),
            ShellCommand::Stop => state.stop(),
            ShellCommand::ToggleLoop => {
                let on = !state.transport.looping();
                state.transport.set_loop(on);
            }
            ShellCommand::GotoStart => state.goto_start(),
            ShellCommand::GotoEnd => state.goto_end(),
            ShellCommand::ZoomTimeIn => state.zoom_time(0.8),
            ShellCommand::ZoomTimeOut => state.zoom_time(1.25),
            ShellCommand::ZoomRowsIn => state.zoom_rows(1.25),
            ShellCommand::ZoomRowsOut => state.zoom_rows(0.8),
            ShellCommand::CollapseAll => state.grid.collapse_all(),
            ShellCommand::ResetRowHeight => state.grid.reset_row_heights(),
            ShellCommand::SelectAll => state.select_all(),
            ShellCommand::DeleteSelected => state.delete_selected(),
            ShellCommand::DrawMode => state.set_draw_mode(),
            ShellCommand::SelectionMode => state.set_selection_mode(),
            ShellCommand::SpeedDecrease => {
                let forward = state.transport.speed_decrease();
                state.forward_speed(forward);
            }
            ShellCommand::SpeedIncrease => {
                let forward = state.transport.speed_increase();
                state.forward_speed(forward);
            }
            ShellCommand::SpeedReset => {
                let forward = state.transport.speed_reset();
                state.forward_speed(forward);
            }
            ShellCommand::SpeedMax => {
                let forward = state.transport.speed_max();
                state.forward_speed(forward);
            }
            ShellCommand::SpeedWheelUp => {
                let forward = state.transport.speed_wheel(true);
                state.forward_speed(forward);
            }
            ShellCommand::SpeedWheelDown => {
                let forward = state.transport.speed_wheel(false);
                state.forward_speed(forward);
            }
            ShellCommand::InvertDragBoxFilter => state.invert_drag_box_types(),
            ShellCommand::AlignStartToMarks => return state.align_to_marks(MarkAlign::Start, false).into(),
            ShellCommand::AlignEndToMarks => return state.align_to_marks(MarkAlign::End, false).into(),
            ShellCommand::AlignBothToMarks => return state.align_to_marks(MarkAlign::Both, false).into(),
            ShellCommand::DistributeEqually => return state.distribute_equally_selected().into(),
            ShellCommand::DistributeDialog => return state.distribute_dialog_request().into(),
            ShellCommand::CloseGaps => return state.close_gaps(false).into(),
        }
        empty_outcome()
    }

    pub fn set_option(&self, option: ShellOption, on: bool) {
        let mut state = self.state.lock();
        match option {
            ShellOption::AutoSave => state.set_auto_save(on),
            ShellOption::SnapTo => state.prefs.snap_to_selected = on,
            ShellOption::ResizeIndicator => state.prefs.resize_indicator_enabled = on,
            ShellOption::CadStyle => state.prefs.cad_style_selection_box = on,
            ShellOption::LegacyCursor => state.prefs.legacy_cursor_active_row = on,
            ShellOption::ZoomUnderMouse => state.prefs.zoom_under_mouse_position = on,
            ShellOption::HighlightRows => state.prefs.highlight_active_elements = on,
            ShellOption::ShowEffectInfo => state.prefs.show_effect_info = on,
            ShellOption::ShowHiddenRows => state.set_show_hidden_rows(on),
            ShellOption::FullWaveform => state.prefs.full_waveform = on,
            ShellOption::LockRulerHeight => state.prefs.lock_ruler_height = on,
            ShellOption::LockWaveformHeight => state.prefs.lock_waveform_height = on,
            ShellOption::SpeedTempo => state.set_use_tempo(on),
            ShellOption::DragBoxFilter => state.set_drag_box_filter(on),
            ShellOption::CurveLinkCurves => state.prefs.curve_link_curves = on,
            ShellOption::GradientLinkGradients => state.prefs.gradient_link_gradients = on,
            ShellOption::BasicEffects => state.prefs.basic_effect_strip = on,
            ShellOption::PixelEffects => state.prefs.pixel_effect_strip = on,
            ShellOption::DeviceEffects => state.prefs.device_effect_strip = on,
            ShellOption::FixtureEffects => state.prefs.fixture_effect_strip = on,
        }
        state.prefs_changed();
    }

    pub fn set_delay(&self, delay: ShellDelay) {
        self.state.lock().transport.set_delay(delay.into());
    }

    /// `modeToolStripMenuItem_SnapStrength_*`: exclusive, and clicking the checked one does nothing.
    pub fn set_snap_strength(&self, strength: i32) {
        let mut state = self.state.lock();
        state.prefs.snap_strength = strength;
        state.prefs_changed();
    }

    pub fn set_close_gap_threshold(&self, text: String) {
        let mut state = self.state.lock();
        state.prefs.close_gap_threshold = text;
        state.prefs_changed();
    }

    pub fn set_align_to_threshold(&self, text: String) {
        self.state.lock().prefs.align_to_threshold = text;
    }

    /// F51: choosing a colour checks only that one and turns the indicator on.
    pub fn set_resize_indicator_color(&self, name: String) {
        let mut state = self.state.lock();
        state.prefs.resize_indicator_color = name;
        state.prefs.resize_indicator_enabled = true;
        state.prefs_changed();
    }

    /// F157.
    pub fn set_effect_label_index(&self, index: String) {
        self.state.lock().prefs.effect_label_index = index;
    }

    pub fn toggle_drag_box_type(&self, type_id: String) {
        if let Some(id) = parse_uuid(&type_id) {
            self.state.lock().toggle_drag_box_type(id);
        }
    }

    /// Edit > Add Effect. F284.
    pub fn add_effect(&self, type_id: String) -> bool {
        parse_uuid(&type_id).is_some_and(|id| self.state.lock().add_effect_from_menu(id).is_some())
    }

    /// The Effects panel's leaf click, and the Effects strip's in Draw Mode. F320, F150. Returns what's now chosen.
    pub fn toggle_selected_effect(&self, type_id: String) -> Option<String> {
        let id = parse_uuid(&type_id)?;
        self.state.lock().toggle_selected_effect(id).map(|u| u.to_string())
    }

    /// Hotkeys. Menu shortcuts have gone first.
    pub fn key_down(&self, key: ShellKey, shift: bool, cmd: bool, opt: bool) -> ShellOutcome {
        let Some(key) = key.key() else {
            return ShellOutcome { handled: false, ..empty_outcome() };
        };
        self.state.lock().run_key(key, Mods { shift, cmd, opt }, Instant::now()).into()
    }

    /// A prompt's default button.
    pub fn run_alert_action(&self, action: ShellAlertAction) -> ShellOutcome {
        let action = match action {
            ShellAlertAction::AlignToMarksEverywhere { kind } => seq::AlertAction::AlignToMarksEverywhere(kind.into()),
            ShellAlertAction::RemoveAudio => seq::AlertAction::RemoveAudio,
            ShellAlertAction::AssociateAudio => seq::AlertAction::AssociateAudio,
            ShellAlertAction::AddMarksEverywhere => seq::AlertAction::AddMarksEverywhere,
            ShellAlertAction::CreateDefaultMarks => seq::AlertAction::CreateDefaultMarks,
            ShellAlertAction::EnableDefaultMarks => seq::AlertAction::EnableDefaultMarks,
            ShellAlertAction::ReplaceSelected { type_id } => seq::AlertAction::ReplaceSelected(parse_uuid(&type_id).unwrap_or_else(Uuid::nil)),
            ShellAlertAction::CloneToSelected { source } => seq::AlertAction::CloneToSelected(EffectId(source)),
            ShellAlertAction::ContinueDrop => seq::AlertAction::ContinueDrop,
            ShellAlertAction::CloseGapsEverywhere => seq::AlertAction::CloseGapsEverywhere,
        };
        self.state.lock().run_alert_action(&action).into()
    }

    /// F83.
    pub fn set_sequence_length(&self, length_ms: f64) {
        self.state.lock().set_sequence_length(time_from_ms(length_ms));
    }

    pub fn set_cursor(&self, ms: f64) {
        self.state.lock().set_cursor(time_from_ms(ms));
    }

    /// The timeline moved its cursor: the status bar follows.
    pub fn cursor_moved(&self, ticks: i64) {
        self.state.lock().cursor_moved(TimeSpan(ticks));
    }

    /// F265: the marks bar and the ruler ask to play a range.
    pub fn play_between(&self, start_ticks: i64, end_ticks: Option<i64>) {
        let end = end_ticks.map_or(TimeSpan::MAX, TimeSpan);
        self.state.lock().play_between(TimeSpan(start_ticks), end);
    }

    /// Zoom, scroll or a row height changed: F16's silent save at close.
    pub fn note_editor_state_modified(&self) {
        self.state.lock().note_editor_state_modified();
    }

    /// F260.
    pub fn set_status_message(&self, text: String, timeout_seconds: u32) {
        self.state.lock().set_status_message(&text, timeout_seconds, Instant::now());
    }

    // Clipboard

    /// The pasteboard's count, whenever `changeCount` moved. F316.
    pub fn set_clipboard_count(&self, count: u32) {
        self.state.lock().set_clipboard_count(count as usize);
    }

    /// Cut or Copy: the JSON for the pasteboard, or nothing with nothing selected.
    pub fn copy_or_cut(&self, cut: bool) -> ShellOutcome {
        match self.state.lock().copy_or_cut(cut) {
            Some(data) => {
                let count = data.effects.len() as u32;
                ShellOutcome { clipboard_json: Some(data.to_json()), clipboard_count: count, ..empty_outcome() }
            }
            None => ShellOutcome { handled: false, ..empty_outcome() },
        }
    }

    pub fn paste(&self, json: String, mode: ShellPasteMode) -> ShellOutcome {
        let Some(data) = ClipboardData::from_json(&json) else {
            return ShellOutcome { handled: false, ..empty_outcome() };
        };
        self.state.lock().paste(&data, mode.into(), Instant::now()).into()
    }

    /// The Lyric Converter's paste (F301): default mode at `ticks`, and how many went in. Nothing on the pasteboard pastes nothing and
    /// records nothing, as `ClipboardPaste` returns before its undo step there.
    pub fn paste_at_time(&self, json: Option<String>, ticks: i64) -> ShellPasteCount {
        let Some(data) = json.as_deref().and_then(ClipboardData::from_json) else {
            return ShellPasteCount { outcome: ShellOutcome { handled: false, ..empty_outcome() }, pasted: 0 };
        };
        let (outcome, pasted) = self.state.lock().paste_at_time(&data, TimeSpan::from_ticks(ticks), Instant::now());
        ShellPasteCount { outcome: outcome.into(), pasted: pasted as u32 }
    }

    // Saving and closing

    pub fn save_route(&self, force_panel: bool) -> ShellSaveRoute {
        match self.state.lock().save_route(force_panel) {
            seq::SaveRoute::InPlace(path) => ShellSaveRoute::InPlace { path: path.to_string_lossy().into_owned() },
            seq::SaveRoute::Panel { directory, suggested_name } => {
                ShellSaveRoute::Panel { directory: directory.to_string_lossy().into_owned(), suggested_name }
            }
        }
    }

    /// `path` is the panel's choice, or empty to save in place. Write the result off the main thread, then call `finish_save`.
    pub fn prepare_save(&self, path: Option<String>) -> Result<Arc<ShellPreparedSave>, FfiError> {
        let inner = self.state.lock().prepare_save(path.as_deref().map(Path::new))?;
        Ok(Arc::new(ShellPreparedSave { inner }))
    }

    pub fn finish_save(&self, path: String, succeeded: bool) {
        self.state.lock().finish_save(PathBuf::from(path), succeeded);
    }

    pub fn close_action(&self, app_closing: bool) -> ShellCloseAction {
        match self.state.lock().close_action(app_closing) {
            CloseAction::Prompt { can_cancel } => ShellCloseAction::Prompt { can_cancel },
            CloseAction::SaveSilently => ShellCloseAction::SaveSilently,
            CloseAction::Close => ShellCloseAction::Close,
        }
    }

    /// `EditorClosing`. Returns a problem for the log.
    pub fn finish_close(&self) -> Option<String> {
        let problem = self.state.lock().finish_close();
        self.session.core.unregister_sequence_session(&self.session.session);
        problem
    }

    // Undo

    pub fn undo(&self, steps: u32) -> u32 {
        self.state.lock().undo(steps as usize) as u32
    }

    pub fn redo(&self, steps: u32) -> u32 {
        self.state.lock().redo(steps as usize) as u32
    }

    // Toolbars

    pub fn toolbars(&self) -> ShellToolbars {
        let state = self.state.lock();
        let info = |s: StripId| ShellStripInfo {
            strip: s.into(),
            title: s.title().to_string(),
            name: s.name().to_string(),
            visible: state.toolbars.is_visible(s),
            large: state.toolbars.is_large(s),
        };
        let mut hidden = Vec::new();
        for (strip, items) in &state.strip_states.states {
            for (item, visible) in items {
                if !visible {
                    hidden.push(format!("{strip}/{item}"));
                }
            }
        }
        ShellToolbars {
            rows: state.toolbars.rows.iter().map(|row| row.iter().map(|s| info(*s)).collect()).collect(),
            list: StripId::ALL.into_iter().map(info).collect(),
            hidden_items: hidden,
        }
    }

    pub fn set_strip_visible(&self, strip: ShellStrip, visible: bool) {
        self.state.lock().toolbars.set_visible(strip.into(), visible);
    }

    pub fn move_strip(&self, strip: ShellStrip, row: u32, index: u32) {
        self.state.lock().toolbars.move_strip(strip.into(), row as usize, index as usize);
    }

    pub fn toggle_strip_size(&self, strip: ShellStrip) {
        self.state.lock().toolbars.toggle_size(strip.into());
    }

    /// Show/Hide Items, for a strip with fixed items. F101.
    pub fn strip_items(&self, strip: ShellStrip) -> Vec<ShellStripItem> {
        let strip: StripId = strip.into();
        let state = self.state.lock();
        strip
            .items()
            .iter()
            .map(|(name, tooltip)| ShellStripItem {
                name: name.to_string(),
                tooltip: tooltip.to_string(),
                visible: state.strip_states.is_item_visible(strip.name(), name),
            })
            .collect()
    }

    pub fn set_strip_item_visible(&self, strip: ShellStrip, item: String, visible: bool) {
        let strip: StripId = strip.into();
        self.state.lock().strip_states.set_item_visible(strip.name(), &item, visible);
    }

    /// Reset Toolbar. F104.
    pub fn reset_strip(&self, strip: ShellStrip) {
        self.state.lock().reset_strip(strip.into());
    }

    // Panels

    pub fn panels(&self) -> ShellPanels {
        let state = self.state.lock();
        let tab = |id: PanelId| ShellPanelTab { panel: id.into(), title: id.title().to_string() };
        let layout = &state.panels;
        ShellPanels {
            left_tabs: layout.tabs(DockSide::Left).into_iter().map(tab).collect(),
            right_tabs: layout.tabs(DockSide::Right).into_iter().map(tab).collect(),
            floating: layout.panels.iter().filter(|p| p.visible && p.floating).map(|p| tab(p.id)).collect(),
            selected_left: layout.selected_left.map(Into::into),
            selected_right: layout.selected_right.map(Into::into),
            left_width: layout.left_width,
            right_width: layout.right_width,
            visible: layout.panels.iter().filter(|p| p.visible).map(|p| p.id.into()).collect(),
        }
    }

    pub fn toggle_panel(&self, panel: ShellPanel) {
        self.state.lock().toggle_panel(panel.into());
    }

    pub fn select_panel(&self, side: ShellSide, panel: ShellPanel) {
        self.state.lock().select_panel(side.into(), panel.into());
    }

    pub fn set_panel_floating(&self, panel: ShellPanel, floating: bool) {
        self.state.lock().panels.set_floating(panel.into(), floating);
    }

    pub fn set_panel_widths(&self, left: f64, right: f64) {
        let mut state = self.state.lock();
        state.panels.left_width = left;
        state.panels.right_width = right;
    }

    /// Everything a test script checks, as JSON. Read through a menu item the harness alone sees.
    pub fn debug_state_json(&self) -> String {
        self.state.lock().debug_state_json()
    }
}

/// `poll`'s answer.
#[derive(uniffi::Record)]
pub struct ShellPoll {
    pub changed: bool,
    pub auto_save_due: bool,
    pub transport_active: bool,
}

fn ui_state_record(ui: seq::UiState, drag_box_invert_checked: bool) -> ShellUiState {
    let gauge = speed_gauge(ui.speed.display_speed);
    let p = &ui.prefs;
    ShellUiState {
        title: ui.title.clone(),
        sequence_name: ui.sequence_name.clone(),
        profile_name: ui.profile_name.clone(),
        file_path: ui.file_path.clone(),
        modified: ui.modified,
        transport: ShellTransport {
            play_enabled: ui.transport.play_enabled,
            play_menu_enabled: ui.transport.play_menu_enabled,
            play_has_shortcut: ui.transport.play_has_shortcut,
            stop_enabled: ui.transport.stop_enabled,
            pause_menu_enabled: ui.transport.pause_menu_enabled,
            icon: match ui.transport.icon {
                PlayIcon::Play => ShellPlayIcon::Play,
                PlayIcon::Pause => ShellPlayIcon::Pause,
                PlayIcon::Hourglass => ShellPlayIcon::Hourglass,
            },
            play_title: ui.transport.play_title.to_string(),
            play_tooltip: ui.transport.play_tooltip.to_string(),
            delay_visible: ui.transport.delay_visible,
            delay_text: ui.transport.delay_text.clone(),
            delay: ui.transport.delay.into(),
            looping: ui.transport.looping,
            running: ui.transport.running,
            paused: ui.transport.paused,
        },
        speed: ShellSpeed {
            speed: ui.speed.speed,
            decrease_enabled: ui.speed.decrease_enabled,
            increase_enabled: ui.speed.increase_enabled,
            gauge_enabled: ui.speed.gauge_enabled,
            gauge_text: gauge.text,
            gauge_text_x: gauge.text_x,
            gauge_red: gauge.rgb.0,
            gauge_green: gauge.rgb.1,
            gauge_blue: gauge.rgb.2,
            needle_end_x: gauge.needle_end.0,
            needle_end_y: gauge.needle_end.1,
        },
        has_selection: ui.has_selection,
        selection_count: ui.selection_count,
        paste_enabled: ui.paste.paste,
        paste_marks_enabled: ui.paste.paste_marks,
        paste_invert_enabled: ui.paste.paste_invert,
        paste_special_enabled: ui.paste.paste_special,
        undo_name: ui.undo_name.clone(),
        redo_name: ui.redo_name.clone(),
        remove_audio_enabled: ui.audio.remove_enabled,
        beat_bar_enabled: ui.audio.beat_bar_enabled,
        speed_tempo_enabled: ui.audio.speed_tempo_enabled,
        audio_tooltip: ui.audio.button_tooltip.clone(),
        timeline_title: ui.audio.timeline_title.clone(),
        effect_count: ui.effect_count,
        prefs: ShellPrefs {
            auto_save: p.auto_save_enabled,
            snap_to: p.snap_to_selected,
            snap_strength: p.snap_strength,
            close_gap_threshold: p.close_gap_threshold.clone(),
            align_to_threshold: p.align_to_threshold.clone(),
            resize_indicator: p.resize_indicator_enabled,
            resize_indicator_color: p.resize_indicator_color.clone(),
            cad_style: p.cad_style_selection_box,
            legacy_cursor: p.legacy_cursor_active_row,
            zoom_under_mouse: p.zoom_under_mouse_position,
            highlight_rows: p.highlight_active_elements,
            show_effect_info: p.show_effect_info,
            full_waveform: p.full_waveform,
            lock_ruler_height: p.lock_ruler_height,
            lock_waveform_height: p.lock_waveform_height,
            use_tempo_for_speed: p.use_tempo_for_speed,
            effect_label_index: p.effect_label_index.clone(),
            basic_effects: p.basic_effect_strip,
            pixel_effects: p.pixel_effect_strip,
            device_effects: p.device_effect_strip,
            fixture_effects: p.fixture_effect_strip,
            curve_link_curves: p.curve_link_curves,
            gradient_link_gradients: p.gradient_link_gradients,
        },
        draw_mode: ui.draw_mode,
        selection_mode: ui.selection_mode,
        show_hidden_rows: ui.show_hidden_rows,
        drag_box_filter: ui.drag_box_filter,
        drag_box_invert_checked,
        drag_box_filter_types: ui.drag_box_filter_types.iter().map(Uuid::to_string).collect(),
        selected_effect: ui.selected_effect.map(|u| u.to_string()),
        position_text: ui.position_text.clone(),
        length_text: ui.length_text.clone(),
        status_message: ui.status_message.clone(),
        rendering_visible: ui.rendering_visible,
        rendering_percent: ui.rendering_percent,
    }
}

// ------------------------------------------------------------------------------------------------------------------
// The timeline, when it's in the build
// ------------------------------------------------------------------------------------------------------------------

// ------------------------------------------------------------------------------------------------------------------
// The effect context menu: F226 to F254
// ------------------------------------------------------------------------------------------------------------------

#[derive(uniffi::Record, Clone)]
pub struct ShellLayerItem {
    pub id: String,
    pub name: String,
    pub filter_name: String,
    pub in_use: bool,
}

#[derive(uniffi::Record, Clone)]
pub struct ShellLayerMenu {
    pub items: Vec<ShellLayerItem>,
    pub single_in_use: bool,
}

#[derive(uniffi::Record, Clone)]
pub struct ShellCollectionItem {
    pub name: String,
    pub description: String,
}

#[derive(uniffi::Record, Clone)]
pub struct ShellContextMenu {
    pub row_deprecated: bool,
    pub add_effect_groups: Vec<Vec<ShellEffect>>,
    pub add_effect_enabled: bool,
    pub add_effect_tooltip: String,
    pub layers: Option<ShellLayerMenu>,
    pub alignment_enabled: bool,
    pub alignment_tooltip: String,
    pub reference_enabled: bool,
    pub distribute_enabled: bool,
    pub marks_enabled: bool,
    pub show_manipulation: bool,
    pub clone_to_selected_enabled: bool,
    pub show_edit_time: bool,
    pub edit_time_enabled: bool,
    pub edit_time_tooltip: String,
    pub cut_copy_enabled: bool,
    pub paste_enabled: bool,
    pub paste_children_enabled: bool,
    pub paste_special_submenu: bool,
    pub paste_marks_alone: bool,
    pub paste_invert_alone: bool,
    pub show_delete: bool,
    pub collections: Vec<ShellCollectionItem>,
    pub element: Option<u64>,
}

fn shell_effect(e: seq::EffectCatalogEntry) -> ShellEffect {
    use vixen_session::sequencer::EffectGroupPublic as G;
    ShellEffect {
        type_id: e.type_id.to_string(),
        name: e.name.to_string(),
        group: match e.group {
            G::Basic => ShellEffectGroup::Basic,
            G::Pixel => ShellEffectGroup::Pixel,
            G::Device => ShellEffectGroup::Device,
            G::IntelligentFixture => ShellEffectGroup::IntelligentFixture,
        },
        icon: e.icon,
    }
}

// ------------------------------------------------------------------------------------------------------------------
// Dialogs that hold state between fields: F379, F384 to F393
// ------------------------------------------------------------------------------------------------------------------

/// What a time box shows and allows.
#[derive(uniffi::Record, Clone)]
pub struct ShellTimeEditorValues {
    pub start_ms: f64,
    pub duration_ms: f64,
    pub end_ms: f64,
    pub start_max_ms: f64,
    pub duration_min_ms: f64,
    pub duration_max_ms: f64,
    pub end_min_ms: f64,
    pub end_max_ms: f64,
}

/// `EffectTimeEditor`.
#[derive(uniffi::Object)]
pub struct ShellTimeEditor {
    state: Mutex<EffectTimeEditorState>,
}

#[uniffi::export]
impl ShellTimeEditor {
    pub fn values(&self) -> ShellTimeEditorValues {
        let s = self.state.lock();
        ShellTimeEditorValues {
            start_ms: s.start.total_ms(),
            duration_ms: s.duration.total_ms(),
            end_ms: s.end.total_ms(),
            start_max_ms: s.start_max.total_ms(),
            duration_min_ms: s.duration_min.total_ms(),
            duration_max_ms: s.duration_max.total_ms(),
            end_min_ms: s.end_min.total_ms(),
            end_max_ms: s.end_max.total_ms(),
        }
    }

    pub fn set_start(&self, ms: f64) {
        self.state.lock().set_start(time_from_ms(ms));
    }

    pub fn set_duration(&self, ms: f64) {
        self.state.lock().set_duration(time_from_ms(ms));
    }

    pub fn set_end(&self, ms: f64) {
        self.state.lock().set_end(time_from_ms(ms));
    }

    pub fn match_sequence(&self) {
        self.state.lock().match_sequence();
    }
}

/// `Form_AddMultipleEffects`' fields.
#[derive(uniffi::Record, Clone)]
pub struct ShellAddMultipleValues {
    pub title: String,
    pub possible_label: String,
    pub length_ms: f64,
    pub start_ms: f64,
    pub end_ms: f64,
    pub duration_ms: f64,
    pub between_ms: f64,
    pub count: i32,
    pub count_max: i32,
    pub ok_enabled: bool,
    pub align_to_marks: bool,
    pub skip_every_other: bool,
    pub fill: bool,
    pub mark_start_end: bool,
    pub select_effects: bool,
    pub panel_open: bool,
    pub checked_marks: Vec<String>,
    pub duration_enabled: bool,
    pub between_enabled: bool,
    pub fill_can_toggle: bool,
    pub skip_can_toggle: bool,
    pub mark_start_end_can_toggle: bool,
    pub mark_names: Vec<String>,
}

/// F392.
#[derive(uniffi::Enum, Clone)]
pub enum ShellAddMultipleOk {
    Proceed,
    ZeroCount { message: String, title: String },
    Overrun { message: String, title: String },
}

#[derive(uniffi::Object)]
pub struct ShellAddMultiple {
    state: Mutex<AddMultipleState>,
    marks: Vec<vixen_core::MarkCollection>,
}

#[uniffi::export]
impl ShellAddMultiple {
    pub fn values(&self) -> ShellAddMultipleValues {
        let s = self.state.lock();
        ShellAddMultipleValues {
            title: format!("Add {} effects", s.effect_name),
            possible_label: possible_label(s.count_max),
            length_ms: s.length.total_ms(),
            start_ms: s.start.total_ms(),
            end_ms: s.end.total_ms(),
            duration_ms: s.duration.total_ms(),
            between_ms: s.between.total_ms(),
            count: s.count,
            count_max: s.count_max,
            ok_enabled: s.ok_enabled,
            align_to_marks: s.align_to_marks,
            skip_every_other: s.skip_every_other,
            fill: s.fill,
            mark_start_end: s.mark_start_end,
            select_effects: s.select_effects,
            panel_open: s.panel_open,
            checked_marks: s.checked_marks.clone(),
            duration_enabled: s.duration_enabled,
            between_enabled: s.between_enabled,
            fill_can_toggle: s.fill_can_toggle,
            skip_can_toggle: s.skip_can_toggle,
            mark_start_end_can_toggle: s.mark_start_end_can_toggle,
            mark_names: self.marks.iter().map(|m| m.name.clone()).collect(),
        }
    }

    pub fn set_start(&self, ms: f64) {
        self.state.lock().set_start(time_from_ms(ms), &self.marks);
    }

    pub fn set_end(&self, ms: f64) {
        self.state.lock().set_end(time_from_ms(ms), &self.marks);
    }

    pub fn set_duration(&self, ms: f64) {
        self.state.lock().set_duration(time_from_ms(ms), &self.marks);
    }

    pub fn set_between(&self, ms: f64) {
        self.state.lock().set_between(time_from_ms(ms), &self.marks);
    }

    /// F390's warning, when collections share a name: `(message, title)`.
    pub fn set_align(&self, on: bool) -> Option<Vec<String>> {
        self.state.lock().set_align(on, &self.marks).map(|(message, title)| vec![message, title])
    }

    pub fn set_skip_every_other(&self, on: bool) {
        self.state.lock().set_skip_every_other(on, &self.marks);
    }

    pub fn set_fill(&self, on: bool) {
        self.state.lock().set_fill(on, &self.marks);
    }

    pub fn set_mark_start_end(&self, on: bool) {
        self.state.lock().set_mark_start_end(on, &self.marks);
    }

    pub fn set_mark_checked(&self, name: String, on: bool) {
        self.state.lock().set_mark_checked(&name, on, &self.marks);
    }

    pub fn set_count_text(&self, text: String) {
        self.state.lock().set_count_text(&text);
    }

    pub fn use_all_possible(&self) {
        self.state.lock().use_all_possible();
    }

    pub fn set_select_effects(&self, on: bool) {
        self.state.lock().select_effects = on;
    }

    pub fn set_panel_open(&self, on: bool) {
        self.state.lock().panel_open = on;
    }

    pub fn check_ok(&self) -> ShellAddMultipleOk {
        match self.state.lock().check_ok() {
            AddMultipleOk::Proceed => ShellAddMultipleOk::Proceed,
            AddMultipleOk::ZeroCount { message, title } => ShellAddMultipleOk::ZeroCount { message, title },
            AddMultipleOk::Overrun { message, title } => ShellAddMultipleOk::Overrun { message, title },
        }
    }
}

/// F394's start.
#[derive(uniffi::Record, Clone)]
pub struct ShellTimeRange {
    pub start_ms: f64,
    pub end_ms: f64,
    pub length_ms: f64,
}

/// F381's choices.
#[derive(uniffi::Enum, Clone, Copy)]
pub enum ShellDistributeDuration {
    Equal,
    DoNotChange,
    Specified { ms: f64 },
}

#[derive(uniffi::Enum, Clone, Copy)]
pub enum ShellDistributePlacement {
    StairStep,
    Overlap { ms: f64 },
    Spaced { ms: f64 },
}

/// F302.
#[derive(uniffi::Enum, Clone, Copy)]
pub enum ShellAddAudio {
    Unsupported,
    Unprocessable,
    Added { resize_prompt: bool },
}

// ------------------------------------------------------------------------------------------------------------------
// Drops: F305 to F312
// ------------------------------------------------------------------------------------------------------------------

/// A value dragged over an effect.
#[derive(uniffi::Enum, Clone)]
pub enum ShellDropValue {
    Color { argb: u32 },
    Gradient { value: GradientValue },
    Curve { value: CurveValue },
}

impl From<&ShellDropValue> for DropValue {
    fn from(v: &ShellDropValue) -> DropValue {
        match v {
            ShellDropValue::Color { argb } => DropValue::Color(vixen_core::Color::from_argb_u32(*argb)),
            ShellDropValue::Gradient { value } => DropValue::Gradient(value.into()),
            ShellDropValue::Curve { value } => DropValue::Curve(value.into()),
        }
    }
}

/// What a picker item shows: F310.
#[derive(uniffi::Enum, Clone)]
pub enum ShellPickerImage {
    Color { argb: u32 },
    Gradient { value: GradientValue },
    Curve { points: CurveValue },
}

#[derive(uniffi::Record, Clone)]
pub struct ShellPickerChoice {
    pub label: String,
    pub group: Option<String>,
    pub owner: Option<String>,
    pub image: ShellPickerImage,
}

/// A drop waiting for its answers.
#[derive(uniffi::Object)]
pub struct ShellDropPlan {
    shell: Arc<SequencerShell>,
    request: DropRequest,
    choices: Vec<PickerChoice>,
    direct: bool,
}

#[uniffi::export]
impl ShellDropPlan {
    /// F307's warning about other types. Cancel gives the drop up.
    pub fn warning(&self) -> Option<ShellAlert> {
        self.request.warning.clone().map(Into::into)
    }

    /// True when `apply(0)` goes straight on. Otherwise the picker shows `choices()`.
    pub fn is_direct(&self) -> bool {
        self.direct
    }

    pub fn choices(&self) -> Vec<ShellPickerChoice> {
        self.choices
            .iter()
            .map(|c| ShellPickerChoice {
                label: c.label.clone(),
                group: c.group.clone(),
                owner: c.prop.owner.map(|(_, index)| format!("{} {}", c.prop.owner_label.unwrap_or(""), index + 1)),
                image: match &c.image {
                    PickerImage::Color(color) => ShellPickerImage::Color { argb: color.to_argb_u32() },
                    PickerImage::Gradient(g) => ShellPickerImage::Gradient { value: g.into() },
                    PickerImage::Curve(curve) => ShellPickerImage::Curve { points: curve.into() },
                    PickerImage::Effect(_) => ShellPickerImage::Color { argb: 0 },
                },
            })
            .collect()
    }

    pub fn apply(&self, choice: u32) {
        if let Some(choice) = self.choices.get(choice as usize) {
            self.shell.state.lock().apply_drop(choice, &self.request.value, &self.request.elements, Instant::now());
        }
    }
}

// ------------------------------------------------------------------------------------------------------------------
// The Layers panel: F330 to F340
// ------------------------------------------------------------------------------------------------------------------

#[derive(uniffi::Record, Clone)]
pub struct ShellLayer {
    pub id: String,
    pub name: String,
    pub filter_name: String,
    pub is_default: bool,
    pub has_setup: bool,
}

#[derive(uniffi::Record)]
pub struct ShellLipSyncMap {
    pub name: String,
    pub is_default: bool,
}

#[derive(uniffi::Record)]
pub struct ShellPasteCount {
    pub outcome: ShellOutcome,
    pub pasted: u32,
}

#[derive(uniffi::Record, Clone)]
pub struct ShellLayerPlan {
    pub failure: Option<String>,
    pub importable: u32,
    /// F338's question, when some can't come in.
    pub skipped_message: Option<String>,
}

// ------------------------------------------------------------------------------------------------------------------
// Colours
// ------------------------------------------------------------------------------------------------------------------

/// One row of the Audio Devices menu.
#[derive(uniffi::Record, Clone)]
pub struct ShellAudioDevice {
    pub id: String,
    pub name: String,
    pub checked: bool,
}

/// F403's list.
#[derive(uniffi::Record, Clone)]
pub struct ShellEffectDefault {
    pub type_id: String,
    pub effect_name: String,
}

#[derive(uniffi::Record, Clone)]
pub struct ShellColorCollection {
    pub name: String,
    pub description: String,
    pub colors: Vec<u32>,
}

fn collection_record(c: &ColorCollection) -> ShellColorCollection {
    ShellColorCollection { name: c.name.clone(), description: c.description.clone(), colors: c.colors.iter().map(|x| x.to_argb_u32()).collect() }
}

fn collection_value(c: &ShellColorCollection) -> ColorCollection {
    ColorCollection { name: c.name.clone(), description: c.description.clone(), colors: colors_from(&c.colors) }
}

fn colors_from(list: &[u32]) -> Vec<vixen_core::Color> {
    list.iter().map(|c| vixen_core::Color::from_argb_u32(*c)).collect()
}

/// F366.
#[derive(uniffi::Enum, Clone)]
pub enum ShellNewCollection {
    Blank,
    Duplicate { name: String },
    Added,
}

#[derive(uniffi::Record, Clone)]
pub struct ShellCollectionEditorValues {
    pub names: Vec<String>,
    pub selected: Option<ShellColorCollection>,
    pub last_color: u32,
    pub dirty: bool,
}

/// `ColorCollectionLibrary_Form`'s working copy.
#[derive(uniffi::Object)]
pub struct ShellCollectionEditor {
    shell: Arc<SequencerShell>,
    editor: Mutex<CollectionEditor>,
}

#[uniffi::export]
impl ShellCollectionEditor {
    pub fn values(&self) -> ShellCollectionEditorValues {
        let e = self.editor.lock();
        ShellCollectionEditorValues {
            names: e.list.iter().map(|c| c.name.clone()).collect(),
            selected: e.selected.and_then(|at| e.list.get(at)).map(collection_record),
            last_color: e.last_color.to_argb_u32(),
            dirty: e.dirty,
        }
    }

    pub fn select(&self, name: String) {
        self.editor.lock().select(&name);
    }

    pub fn new_collection(&self, name: String, overwrite: bool) -> ShellNewCollection {
        match self.editor.lock().new_collection(&name, overwrite) {
            NewCollection::Blank => ShellNewCollection::Blank,
            NewCollection::Duplicate(name) => ShellNewCollection::Duplicate { name },
            NewCollection::Added => ShellNewCollection::Added,
        }
    }

    pub fn delete_selected(&self) {
        self.editor.lock().delete_selected();
    }

    pub fn add_color(&self, argb: u32) {
        self.editor.lock().add_color(vixen_core::Color::from_argb_u32(argb));
    }

    pub fn remove_colors(&self, colors: Vec<u32>) {
        self.editor.lock().remove_colors(&colors_from(&colors));
    }

    pub fn move_color(&self, from: u32, to: u32) {
        self.editor.lock().move_color(from as usize, to as usize);
    }

    pub fn set_description(&self, text: String) {
        self.editor.lock().set_description(&text);
    }

    /// F372: what the file holds. `Err` is unreadable.
    pub fn read_import(&self, path: String) -> Result<Vec<ShellColorCollection>, FfiError> {
        Ok(libs::read_vcc(Path::new(&path))?.iter().map(collection_record).collect())
    }

    pub fn import_clashes(&self, incoming: Vec<ShellColorCollection>) -> Vec<String> {
        let incoming: Vec<ColorCollection> = incoming.iter().map(collection_value).collect();
        self.editor.lock().import_clashes(&incoming)
    }

    pub fn import(&self, incoming: Vec<ShellColorCollection>, overwrite: Vec<String>) -> u32 {
        self.editor.lock().import(incoming.iter().map(collection_value).collect(), &overwrite) as u32
    }

    pub fn export(&self, path: String) -> Result<(), FfiError> {
        Ok(libs::write_vcc(Path::new(&path), &self.editor.lock().list)?)
    }

    /// OK: F373's Yes takes the same path.
    pub fn commit(&self) -> Result<(), FfiError> {
        let list = self.editor.lock().list.clone();
        self.editor.lock().dirty = false;
        Ok(self.shell.state.lock().set_color_collections(list)?)
    }
}

// ------------------------------------------------------------------------------------------------------------------
// Find Effects/Layers: F321 to F327
// ------------------------------------------------------------------------------------------------------------------

#[derive(uniffi::Record, Clone)]
pub struct ShellFindRow {
    pub element: u64,
    pub row_name: String,
    pub start_text: String,
    pub other: String,
}

#[uniffi::export]
pub fn sequencer_audio_extensions() -> Vec<String> {
    seq::SequencerState::supported_audio_extensions().iter().map(|s| s.to_string()).collect()
}

/// The time box. `Rust` owns what each key does.
#[uniffi::export]
pub fn time_box_text(ms: f64, min_ms: f64, max_ms: f64) -> String {
    time_box::validate(&time_box::format_time(time_from_ms(ms)), time_from_ms(min_ms), time_from_ms(max_ms))
}

#[uniffi::export]
pub fn time_box_ms(text: String) -> f64 {
    time_box::parse_time(&text).total_ms()
}

#[uniffi::export]
pub fn time_box_first_position(text: String) -> u32 {
    time_box::first_position(&text) as u32
}

#[derive(uniffi::Record, Clone)]
pub struct ShellTimeBoxEdit {
    pub text: String,
    pub position: u32,
}

#[uniffi::export]
pub fn time_box_type(text: String, position: u32, digit: String, min_ms: f64, max_ms: f64) -> ShellTimeBoxEdit {
    let digit = digit.chars().next().unwrap_or(' ');
    let (text, position) = time_box::replace_digit(&text, position as usize, digit, time_from_ms(min_ms), time_from_ms(max_ms));
    ShellTimeBoxEdit { text, position: position as u32 }
}

#[uniffi::export]
pub fn time_box_rotate(text: String, position: u32, delta: i32, min_ms: f64, max_ms: f64) -> String {
    time_box::rotate(&text, position as usize, delta, time_from_ms(min_ms), time_from_ms(max_ms))
}

#[uniffi::export]
pub fn time_box_move(position: u32, direction: i32) -> u32 {
    time_box::move_position(position as usize, direction) as u32
}

#[uniffi::export]
pub fn time_box_snap(position: u32) -> u32 {
    time_box::snap_position(position as usize) as u32
}

#[uniffi::export]
pub fn layer_filter_names() -> Vec<String> {
    layers::filter_choices().iter().map(layers::filter_type_name).collect()
}

fn layer_uuid(id: &str) -> Uuid {
    parse_uuid(id).unwrap_or_else(Uuid::nil)
}

#[uniffi::export]
impl SequencerShell {
    // The context menu

    pub fn context_menu(&self, row: Option<u32>, under_pointer: Vec<u64>) -> ShellContextMenu {
        let under: Vec<EffectId> = under_pointer.into_iter().map(EffectId).collect();
        let m = self.state.lock().context_menu(row, &under);
        ShellContextMenu {
            row_deprecated: m.row_deprecated,
            add_effect_groups: m.add_effect_groups.into_iter().map(|g| g.into_iter().map(shell_effect).collect()).collect(),
            add_effect_enabled: m.add_effect_enabled,
            add_effect_tooltip: m.add_effect_tooltip,
            layers: m.layers.map(|l| ShellLayerMenu {
                items: l.items.into_iter().map(|i| ShellLayerItem { id: i.id.to_string(), name: i.name, filter_name: i.filter_name, in_use: i.in_use }).collect(),
                single_in_use: l.single_in_use,
            }),
            alignment_enabled: m.alignment_enabled,
            alignment_tooltip: m.alignment_tooltip,
            reference_enabled: m.reference_enabled,
            distribute_enabled: m.distribute_enabled,
            marks_enabled: m.marks_enabled,
            show_manipulation: m.show_manipulation,
            clone_to_selected_enabled: m.clone_to_selected_enabled,
            show_edit_time: m.show_edit_time,
            edit_time_enabled: m.edit_time_enabled,
            edit_time_tooltip: m.edit_time_tooltip,
            cut_copy_enabled: m.cut_copy_enabled,
            paste_enabled: m.paste.paste_enabled,
            paste_children_enabled: m.paste.children_enabled,
            paste_special_submenu: m.paste.submenu,
            paste_marks_alone: m.paste.marks_alone,
            paste_invert_alone: m.paste.invert_alone,
            show_delete: m.show_delete,
            collections: m.collections.into_iter().map(|c| ShellCollectionItem { name: c.name, description: c.description }).collect(),
            element: m.element.map(|e| e.0),
        }
    }

    pub fn assign_layer(&self, under_pointer: Vec<u64>, layer: String) {
        let under: Vec<EffectId> = under_pointer.into_iter().map(EffectId).collect();
        self.state.lock().assign_layer(&under, layer_uuid(&layer));
    }

    pub fn add_effect_at(&self, type_id: String, row: u32, time_ms: f64) -> Option<u64> {
        self.state.lock().add_effect_at(parse_uuid(&type_id)?, row, time_from_ms(time_ms)).map(|e| e.0)
    }

    pub fn row_is_deprecated(&self, row: u32) -> bool {
        self.state.lock().row_is_deprecated(row)
    }

    pub fn clone_elements(&self, clicked: u64) -> Vec<u64> {
        self.state.lock().clone_elements(EffectId(clicked)).into_iter().map(|e| e.0).collect()
    }

    pub fn clone_to_selected_prompt(&self, source: u64) -> Option<ShellAlert> {
        self.state.lock().clone_to_selected_prompt(EffectId(source)).map(Into::into)
    }

    pub fn clone_to_selected(&self, source: u64) {
        self.state.lock().clone_to_selected(EffectId(source));
    }

    pub fn replace_prompt(&self, type_id: String) -> Option<ShellAlert> {
        Some(self.state.lock().replace_prompt(parse_uuid(&type_id)?).into())
    }

    pub fn replace_selected(&self, type_id: String) -> Vec<u64> {
        let Some(type_id) = parse_uuid(&type_id) else { return Vec::new() };
        self.state.lock().replace_selected(type_id).into_iter().map(|e| e.0).collect()
    }

    pub fn edit_time(&self, clicked: u64) -> Option<Arc<ShellTimeEditor>> {
        self.state.lock().edit_time_values(EffectId(clicked)).map(|state| Arc::new(ShellTimeEditor { state: Mutex::new(state) }))
    }

    pub fn apply_edit_time(&self, clicked: u64, start_ms: f64, duration_ms: f64) {
        self.state.lock().apply_edit_time(EffectId(clicked), time_from_ms(start_ms), time_from_ms(duration_ms));
    }

    pub fn add_marks_to_effects(&self) -> ShellOutcome {
        self.state.lock().add_marks_to_effects(false).into()
    }

    pub fn apply_color_collection(&self, name: String, random: bool) -> ShellOutcome {
        self.state.lock().apply_color_collection(&name, random).into()
    }

    pub fn add_multiple(&self, effect_name: String, time_ms: f64) -> Arc<ShellAddMultiple> {
        let state = self.state.lock();
        let dialog = state.add_multiple_open(&effect_name, time_from_ms(time_ms));
        let marks = state.session.lock().marks().to_vec();
        Arc::new(ShellAddMultiple { state: Mutex::new(dialog), marks })
    }

    pub fn add_multiple_apply(&self, dialog: Arc<ShellAddMultiple>, type_id: String, row: u32) -> u32 {
        let Some(type_id) = parse_uuid(&type_id) else { return 0 };
        let dialog = dialog.state.lock().clone();
        self.state.lock().add_multiple_apply(type_id, row, &dialog) as u32
    }

    pub fn apply_distribute(
        &self,
        start_ms: f64,
        end_ms: f64,
        duration: ShellDistributeDuration,
        placement: ShellDistributePlacement,
        start_with_first: bool,
    ) {
        let params = DistributeParams {
            start: time_from_ms(start_ms),
            end: time_from_ms(end_ms),
            duration: match duration {
                ShellDistributeDuration::Equal => DurationMode::Equal,
                ShellDistributeDuration::DoNotChange => DurationMode::DoNotChange,
                ShellDistributeDuration::Specified { ms } => DurationMode::Specified(time_from_ms(ms)),
            },
            placement: match placement {
                ShellDistributePlacement::StairStep => PlacementMode::StairStep,
                ShellDistributePlacement::Overlap { ms } => PlacementMode::Overlap(time_from_ms(ms)),
                ShellDistributePlacement::Spaced { ms } => PlacementMode::Spaced(time_from_ms(ms)),
            },
            start_with_first,
        };
        self.state.lock().apply_distribute(&params);
    }

    pub fn even_marks_values(&self) -> ShellTimeRange {
        let (start, end, length) = self.state.lock().even_marks_values();
        ShellTimeRange { start_ms: start.total_ms(), end_ms: end.total_ms(), length_ms: length.total_ms() }
    }

    /// The error is the message, and the dialog opens again.
    pub fn create_even_marks(&self, start_ms: f64, end_ms: f64, divisions: i32) -> Option<String> {
        self.state.lock().create_even_marks(time_from_ms(start_ms), time_from_ms(end_ms), divisions).err().map(str::to_string)
    }

    pub fn bulk_move(&self, start_ms: f64, end_ms: f64, offset_ms: f64, visible_only: bool, clip: bool, process_marks: bool) {
        self.state.lock().bulk_move(time_from_ms(start_ms), time_from_ms(end_ms), time_from_ms(offset_ms), visible_only, clip, process_marks);
    }

    pub fn cursor_ms(&self) -> f64 {
        self.state.lock().grid.cursor().total_ms()
    }

    /// The cursor to the tick, for a paste measured from it (F301).
    pub fn cursor_ticks(&self) -> i64 {
        self.state.lock().grid.cursor().ticks()
    }

    /// A mark's text as Breakdown Word looks it up and asks about it (F299): trimmed, punctuation removed.
    pub fn breakdown_clean_word(&self, text: String) -> String {
        vixen_session::marks::lipsync::remove_punctuation(text.trim())
    }

    pub fn length_ms(&self) -> f64 {
        self.state.lock().session.lock().length().total_ms()
    }

    // Audio

    pub fn existing_audio_warning(&self) -> Option<ShellAlert> {
        self.state.lock().existing_audio_warning().map(Into::into)
    }

    pub fn remove_audio_prompt(&self) -> Option<ShellAlert> {
        self.state.lock().remove_audio_prompt().map(Into::into)
    }

    pub fn audio_import_conflict(&self, path: String) -> bool {
        self.state.lock().audio_import_conflict(Path::new(&path))
    }

    pub fn add_audio(&self, path: String, replace_existing_copy: bool) -> ShellAddAudio {
        match self.state.lock().add_audio(Path::new(&path), replace_existing_copy) {
            AddAudioOutcome::Unsupported => ShellAddAudio::Unsupported,
            AddAudioOutcome::Unprocessable => ShellAddAudio::Unprocessable,
            AddAudioOutcome::Added { resize_prompt } => ShellAddAudio::Added { resize_prompt },
        }
    }

    pub fn resize_to_audio(&self) {
        self.state.lock().resize_to_audio();
    }

    // Drops

    pub fn drop_accepted(&self, element: u64, value: ShellDropValue) -> bool {
        self.state.lock().drop_accepted(EffectId(element), &(&value).into())
    }

    /// F307 to F309's first half. `None` when there's nothing to do.
    pub fn plan_drop(self: Arc<Self>, element: u64, value: ShellDropValue) -> Option<Arc<ShellDropPlan>> {
        let request = self.state.lock().plan_drop(EffectId(element), &(&value).into())?;
        let (choices, direct) = match &request.plan {
            DropPlan::Nothing => return None,
            DropPlan::Direct(choice) => (vec![choice.clone()], true),
            DropPlan::Picker(list) => (list.clone(), false),
        };
        Some(Arc::new(ShellDropPlan { shell: Arc::clone(&self), request, choices, direct }))
    }

    /// F311: effects for what the extensions name, selected, each with its own undo step. Returns what was added.
    pub fn drop_files(&self, paths: Vec<String>, time_ms: f64, row: u32, shift: bool, cmd: bool, opt: bool) -> Vec<u64> {
        let files: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
        let (items, horizontal) = plan_file_drop(&files, Mods { shift, cmd, opt }, &[]);
        let mut state = self.state.lock();
        let mut added = Vec::new();
        for item in items {
            let (path, type_id, index) = match item {
                FileDropItem::Skip => continue,
                FileDropItem::Effect { path, type_id, index_in_sequence, .. } => (path, type_id, index_in_sequence),
                // No two effects share an extension today, so this doesn't come up: the first takes it.
                FileDropItem::Ask { path, choices, index_in_sequence } => match choices.first() {
                    Some(type_id) => (path, *type_id, index_in_sequence),
                    None => continue,
                },
            };
            if let Some(id) = state.add_file_effect(type_id, &path, horizontal, row, time_from_ms(time_ms), index) {
                added.push(id);
            }
        }
        state.grid.effects_added(&added);
        state.grid.set_selection(&added);
        added.iter().map(|e| e.0).collect()
    }

    // The Layers panel

    pub fn layers(&self) -> Vec<ShellLayer> {
        let snapshot = self.state.lock().layers_snapshot();
        snapshot
            .layers
            .iter()
            .map(|l| ShellLayer {
                id: l.id.to_string(),
                name: l.name.clone(),
                filter_name: l.filter.as_ref().map_or_else(String::new, layers::filter_type_name),
                is_default: layers::is_default(l),
                has_setup: l.filter.as_ref().is_some_and(layers::has_setup),
            })
            .collect()
    }

    pub fn layers_revision(&self) -> Vec<String> {
        self.layers().into_iter().map(|l| format!("{}|{}|{}", l.id, l.name, l.filter_name)).collect()
    }

    pub fn add_layer(&self) -> String {
        let filter = layers::filter_choices().remove(0);
        self.state.lock().edit_layers(|l| (layers::add_layer(l, filter), true)).to_string()
    }

    pub fn remove_layer(&self, id: String) -> bool {
        let id = layer_uuid(&id);
        self.state.lock().edit_layers(|l| {
            let done = layers::remove_layer(l, id);
            (done, done)
        })
    }

    pub fn move_layer(&self, id: String, index: i32) -> bool {
        let id = layer_uuid(&id);
        self.state.lock().edit_layers(|l| {
            let done = layers::move_layer(l, id, index);
            (done, done)
        })
    }

    pub fn quick_rename_layer(&self, id: String) -> bool {
        let id = layer_uuid(&id);
        self.state.lock().edit_layers(|l| {
            let done = layers::quick_rename(l, id);
            (done, done)
        })
    }

    pub fn rename_layer(&self, id: String, name: String) -> bool {
        let id = layer_uuid(&id);
        self.state.lock().edit_layers(|l| {
            let done = layers::rename_layer(l, id, &name);
            (done, done)
        })
    }

    pub fn set_layer_filter(&self, id: String, filter_name: String) -> bool {
        let id = layer_uuid(&id);
        let Some(filter) = layers::filter_choices().into_iter().find(|f| layers::filter_type_name(f) == filter_name) else { return false };
        self.state.lock().edit_layers(|l| {
            let done = layers::set_layer_filter(l, id, filter);
            (done, done)
        })
    }

    pub fn note_layer_configured(&self) {
        self.state.lock().note_layer_configured();
    }

    pub fn layers_exportable(&self) -> bool {
        layers::has_exportable(&self.state.lock().layers_snapshot())
    }

    pub fn export_layers(&self, path: String, exported_utc: String) -> Result<(), FfiError> {
        Ok(layers::export_v3l(&self.state.lock().layers_snapshot(), Path::new(&path), &exported_utc)?)
    }

    pub fn layer_import_plan(&self, path: String) -> ShellLayerPlan {
        let plan = layers::read_v3l_plan(Path::new(&path));
        let shown = ShellLayerPlan {
            failure: plan.failure.clone(),
            importable: plan.importable.len() as u32,
            skipped_message: (!plan.skipped.is_empty()).then(|| layers::skipped_message(&plan.skipped)),
        };
        *self.layer_plan.lock() = Some((path, plan));
        shown
    }

    /// Imports the plan `layer_import_plan` made for this file, so what the user agreed to is what comes in even if the file has changed
    /// since. Without one, the file is read now.
    pub fn import_layers(&self, path: String) -> u32 {
        let held = self.layer_plan.lock().take();
        let plan = match held {
            Some((held_path, plan)) if held_path == path => plan,
            _ => layers::read_v3l_plan(Path::new(&path)),
        };
        self.state.lock().edit_layers(|l| {
            let count = layers::import_plan(l, &plan);
            (count as u32, count > 0)
        })
    }

    // Find Effects/Layers

    pub fn find_available(&self, layers_mode: bool) -> Vec<String> {
        self.state.lock().find_available(if layers_mode { FindMode::Layers } else { FindMode::Effects })
    }

    pub fn find_rows(&self, layers_mode: bool, search: String) -> Vec<ShellFindRow> {
        self.state
            .lock()
            .find_rows(if layers_mode { FindMode::Layers } else { FindMode::Effects }, &search)
            .into_iter()
            .map(|r| ShellFindRow { element: r.element.0, row_name: r.row_name, start_text: r.start_text, other: r.other })
            .collect()
    }

    pub fn find_display(&self, elements: Vec<u64>, collapse_groups: bool) {
        let list: Vec<EffectId> = elements.into_iter().map(EffectId).collect();
        self.state.lock().find_display(&list, collapse_groups);
    }

    /// How many edits there have been: a panel that follows the sequence looks here.
    pub fn edit_count(&self) -> u64 {
        self.state.lock().session.lock().counters.edits.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn papagayo_import(&self, path: String) -> ShellOutcome {
        self.state.lock().papagayo_import(Path::new(&path)).into()
    }

    /// Tools > LipSync > Default Image Map (F87), read each time the menu opens.
    pub fn lipsync_maps(&self) -> Vec<ShellLipSyncMap> {
        self.state.lock().lipsync_maps().into_iter().map(|(name, is_default)| ShellLipSyncMap { name, is_default }).collect()
    }

    /// True when the default changed, which also marked the sequence modified.
    pub fn set_default_lipsync_map(&self, name: String) -> bool {
        self.state.lock().set_default_lipsync_map(&name)
    }

    // Audio Devices

    pub fn audio_devices(&self) -> Vec<ShellAudioDevice> {
        self.state.lock().audio_devices().into_iter().map(|d| ShellAudioDevice { id: d.id, name: d.name, checked: d.checked }).collect()
    }

    pub fn set_audio_device(&self, id: String) {
        self.state.lock().set_audio_device(&id);
    }

    // Effect Defaults

    pub fn effect_default_summaries(&self) -> Vec<ShellEffectDefault> {
        self.state.lock().effect_default_summaries().into_iter().map(|(id, name)| ShellEffectDefault { type_id: id.to_string(), effect_name: name }).collect()
    }

    pub fn export_effect_defaults(&self, path: String, type_ids: Vec<String>) -> Result<(), FfiError> {
        let ids: Vec<Uuid> = type_ids.iter().filter_map(|t| parse_uuid(t)).collect();
        Ok(self.state.lock().export_effect_defaults(Path::new(&path), &ids)?)
    }

    /// "Imported {n} new and {m} updated effect default(s)."
    pub fn import_effect_defaults(&self, path: String) -> Result<String, FfiError> {
        let (new, updated) = self.state.lock().import_effect_defaults(Path::new(&path))?;
        Ok(format!("Imported {new} new and {updated} updated effect default(s)."))
    }

    pub fn dump_effect_defaults(&self, path: String) -> Result<(), FfiError> {
        Ok(self.state.lock().dump_effect_defaults(Path::new(&path))?)
    }

    // Colours

    pub fn palette(&self) -> Vec<u32> {
        libs::load_palette(self.state.lock().library_dir()).iter().map(|c| c.to_argb_u32()).collect()
    }

    pub fn set_palette(&self, colors: Vec<u32>) {
        let _ = libs::save_palette(self.state.lock().library_dir(), &colors_from(&colors));
    }

    /// F345: the first match per value, since duplicates are allowed.
    pub fn palette_without(&self, colors: Vec<u32>, removing: Vec<u32>) -> Vec<u32> {
        let mut list = colors_from(&colors);
        libs::remove_colors(&mut list, &colors_from(&removing));
        list.iter().map(|c| c.to_argb_u32()).collect()
    }

    /// F347: the selected colours, by position, dragged onto the one at `target`, or past the last.
    pub fn palette_moved(&self, colors: Vec<u32>, selected: Vec<u32>, target: Option<u32>) -> Vec<u32> {
        let selected: Vec<usize> = selected.iter().map(|i| *i as usize).collect();
        libs::moved_by_position(&colors, &selected, target.map(|t| t as usize))
    }

    pub fn import_palette(&self, path: String) -> Result<Vec<u32>, FfiError> {
        Ok(libs::read_vfc(Path::new(&path))?.iter().map(|c| c.to_argb_u32()).collect())
    }

    pub fn export_palette(&self, path: String, colors: Vec<u32>) -> Result<(), FfiError> {
        Ok(libs::write_vfc(Path::new(&path), &colors_from(&colors))?)
    }

    pub fn color_collection_editor(self: Arc<Self>) -> Arc<ShellCollectionEditor> {
        let list = self.state.lock().color_collections.clone();
        Arc::new(ShellCollectionEditor { shell: Arc::clone(&self), editor: Mutex::new(CollectionEditor::new(list)) })
    }

    pub fn panel_scale(&self, key: String) -> f64 {
        self.state.lock().panel_scale(&key)
    }

    pub fn set_panel_scale(&self, key: String, value: f64) {
        self.state.lock().set_panel_scale(&key, value);
    }

    pub fn import_curve_library(&self, path: String) -> Result<u32, FfiError> {
        Ok(self.state.lock().import_curve_library(Path::new(&path))? as u32)
    }

    pub fn export_curve_library(&self, path: String) -> Result<(), FfiError> {
        Ok(self.state.lock().export_curve_library(Path::new(&path))?)
    }

    pub fn import_gradient_library(&self, path: String) -> Result<u32, FfiError> {
        Ok(self.state.lock().import_gradient_library(Path::new(&path))? as u32)
    }

    pub fn export_gradient_library(&self, path: String) -> Result<(), FfiError> {
        Ok(self.state.lock().export_gradient_library(Path::new(&path))?)
    }
}

#[cfg(feature = "timeline")]
mod with_timeline {
    use super::*;
    use crate::timeline::{TimelineAlign, TimelineController, TimelineSettings, TimelineUpdate};
    use vixen_session::sequence::EffectId;
    use vixen_session::sequencer::grid::{GridPort, RowSummary, DEPRECATED_TAG};
    use vixen_session::sequencer::keys::HotkeyCommand;
    use vixen_session::sequencer::prefs::EditorPrefs;
    use vixen_session::sequencer::transport::PlaybackRange;
    use vixen_session::sequencer::CommandOutcome;

    /// What a row is. `RowTree` numbers rows depth first from the roots, one per occurrence, so walking the tree the same way gives each row's node.
    struct RowFacts {
        node: Uuid,
        name: String,
        deprecated: bool,
    }

    fn row_facts(tree: &vixen_core::ElementTree) -> Vec<RowFacts> {
        fn walk(tree: &vixen_core::ElementTree, node: vixen_core::NodeId, out: &mut Vec<RowFacts>) {
            let n = tree.node(node);
            out.push(RowFacts { node: n.id, name: n.name.clone(), deprecated: n.tags.contains(&DEPRECATED_TAG) });
            for child in &n.children {
                walk(tree, *child, out);
            }
        }
        let mut out = Vec::with_capacity(tree.len());
        for root in &tree.roots {
            walk(tree, *root, &mut out);
        }
        out
    }

    /// `GridPort` over the real timeline. What a command returns for the surfaces waits in `pending` for Swift to apply.
    pub(super) struct TimelineGrid {
        pub(super) controller: Arc<TimelineController>,
        rows: Vec<RowFacts>,
        pending: Arc<Mutex<Vec<TimelineUpdate>>>,
        /// The pointer in the grid's client coordinates, as of the key press. `MousePosition`.
        pointer: Arc<Mutex<Option<(i32, i32)>>>,
        /// The grid's client size, which the timeline's bindings don't give back yet.
        size: Arc<Mutex<(i32, i32)>>,
    }

    impl TimelineGrid {
        fn push(&self, update: TimelineUpdate) {
            self.pending.lock().push(update);
        }

        fn view(&self) -> crate::timeline::TimelineView {
            self.controller.view()
        }

        fn display_rows(&self) -> Vec<crate::timeline::TimelineRowDisplay> {
            self.controller.row_frame(0, i32::MAX).rows
        }

        /// The pointed-at effect, when it's among more than one selected, and no drag is on: the condition every pointer key has.
        fn pointed_reference(&self) -> Option<u64> {
            if self.controller.is_drag_in_progress() {
                return None;
            }
            let (x, y) = (*self.pointer.lock())?;
            let under = self.controller.element_at_client(x, y)?;
            let selected = self.controller.selected_elements();
            (selected.len() > 1 && selected.contains(&under)).then_some(under)
        }

        fn align(&self, kind: TimelineAlign, hold: bool) -> CommandOutcome {
            if let Some(reference) = self.pointed_reference() {
                self.push(self.controller.align(kind, Some(reference), hold));
            }
            handled()
        }
    }

    fn handled() -> CommandOutcome {
        CommandOutcome { handled: true, ..CommandOutcome::default() }
    }

    fn ids(list: &[EffectId]) -> Vec<u64> {
        list.iter().map(|e| e.0).collect()
    }

    impl GridPort for TimelineGrid {
        fn visible_rows(&self) -> Vec<RowSummary> {
            self.display_rows()
                .into_iter()
                .filter_map(|r| {
                    let facts = self.rows.get(r.id as usize)?;
                    Some(RowSummary { id: r.id, node: facts.node, name: facts.name.clone(), deprecated: facts.deprecated, selected: r.selected })
                })
                .collect()
        }

        fn row_summary(&self, row: u32) -> Option<RowSummary> {
            let facts = self.rows.get(row as usize)?;
            let selected = self.display_rows().iter().any(|r| r.id == row && r.selected);
            Some(RowSummary { id: row, node: facts.node, name: facts.name.clone(), deprecated: facts.deprecated, selected })
        }

        fn selected_row(&self) -> Option<u32> {
            self.display_rows().iter().find(|r| r.selected).map(|r| r.id)
        }

        fn active_row(&self) -> Option<u32> {
            self.controller.active_row()
        }

        fn top_visible_row(&self) -> Option<u32> {
            let offset = self.view().vertical_offset;
            self.display_rows().iter().find(|r| r.top + r.height > offset).map(|r| r.id)
        }

        fn move_active_row(&mut self, direction: i32) -> bool {
            let moved = self.controller.move_active_row(direction);
            self.push(self.controller.refresh());
            moved
        }

        fn rows_of_node(&self, node: &Uuid) -> Vec<u32> {
            self.rows.iter().enumerate().filter(|(_, r)| &r.node == node).map(|(i, _)| i as u32).collect()
        }

        fn collapse_all(&mut self) {
            self.push(self.controller.collapse_all_rows());
        }

        fn reset_row_heights(&mut self) {
            self.push(self.controller.reset_row_heights());
        }

        fn toggle_selected_rows(&mut self, include_children: bool) {
            self.push(self.controller.toggle_selected_rows(include_children));
        }

        fn set_show_hidden(&mut self, show: bool) {
            self.push(self.controller.set_show_hidden_rows(show));
        }

        fn selected_elements(&self) -> Vec<EffectId> {
            self.controller.selected_elements().into_iter().map(EffectId).collect()
        }

        fn set_selection(&mut self, list: &[EffectId]) {
            self.push(self.controller.clear_selection());
            self.push(self.controller.select_elements(ids(list)));
        }

        fn remove_from_selection(&mut self, list: &[EffectId]) {
            for id in list {
                self.push(self.controller.deselect_element(id.0));
            }
        }

        fn cursor(&self) -> TimeSpan {
            TimeSpan(self.view().cursor)
        }

        fn set_cursor(&mut self, t: TimeSpan) {
            self.push(self.controller.set_cursor(t.0));
        }

        fn set_total(&mut self, _total: TimeSpan) {
            // `sync_from_session` follows the sequence's length.
            self.push(self.controller.refresh());
        }

        fn time_per_pixel(&self) -> TimeSpan {
            TimeSpan(self.view().time_per_pixel)
        }

        fn set_time_per_pixel(&mut self, tpp: TimeSpan) {
            self.push(self.controller.set_time_per_pixel_for_bench(tpp.0));
        }

        fn visible_start(&self) -> TimeSpan {
            TimeSpan(self.view().visible_start)
        }

        fn set_visible_start(&mut self, t: TimeSpan) {
            self.push(self.controller.set_visible_start(t.0));
        }

        fn visible_span(&self) -> TimeSpan {
            TimeSpan(self.view().time_per_pixel.saturating_mul(self.size.lock().0 as i64))
        }

        fn vertical_offset(&self) -> i32 {
            self.view().vertical_offset
        }

        fn set_vertical_offset(&mut self, y: i32) {
            self.push(self.controller.set_vertical_offset(y));
        }

        fn visible_height(&self) -> i32 {
            self.size.lock().1
        }

        fn zoom_time(&mut self, scale: f64) {
            self.push(self.controller.zoom_time(scale));
        }

        fn zoom_rows(&mut self, scale: f64) {
            self.push(self.controller.zoom_rows(scale));
        }

        fn playback_range(&self) -> PlaybackRange {
            let range = self.controller.playback_range();
            PlaybackRange { start: range.first().copied().flatten().map(TimeSpan), end: range.get(1).copied().flatten().map(TimeSpan) }
        }

        fn set_playback_range(&mut self, range: PlaybackRange) {
            self.push(self.controller.set_playback_range(range.start.map(|t| t.0), range.end.map(|t| t.0)));
        }

        // The canvas follows playback itself, from its own display link.
        fn set_playback_current(&mut self, _t: Option<TimeSpan>) {}

        fn is_drag_in_progress(&self) -> bool {
            self.controller.is_drag_in_progress()
        }

        fn element_under_pointer(&self) -> Option<EffectId> {
            let (x, y) = (*self.pointer.lock())?;
            self.controller.element_at_client(x, y).map(EffectId)
        }

        fn is_drawing(&self) -> bool {
            self.controller.drag_state_name() == "Drawing"
        }

        fn is_dragging(&self) -> bool {
            self.controller.is_drag_in_progress()
        }

        fn effects_added(&mut self, _ids: &[EffectId]) {
            self.push(self.controller.refresh());
        }

        fn effects_removed(&mut self, _ids: &[EffectId]) {
            self.push(self.controller.refresh());
        }

        fn effects_moved(&mut self, _moves: &[(EffectId, TimeSpan, TimeSpan)]) {
            self.push(self.controller.refresh());
        }

        fn effects_reloaded(&mut self) {
            self.push(self.controller.refresh());
        }

        fn move_in_range_by_time(&mut self, start: TimeSpan, end: TimeSpan, offset: TimeSpan, visible_only: bool, clip: bool) -> TimeSpan {
            let (update, moved) = self.controller.move_in_range_reporting(start.0, end.0, offset.0, visible_only, clip);
            self.push(update);
            TimeSpan(moved)
        }

        fn add_mark_at(&mut self, time: TimeSpan) {
            self.push(self.controller.add_mark_at(time.0, false, None));
        }

        fn close_gaps(&mut self, threshold_seconds: f64, confirmed: bool) {
            self.push(self.controller.close_gaps(threshold_seconds, confirmed));
        }

        fn display_selected(&mut self, elements: &[EffectId]) {
            self.push(self.controller.reveal_elements(ids(elements)));
        }

        fn sync_from_document(&mut self) {
            self.push(self.controller.refresh());
        }

        fn ok_to_use_alignment(&self, ids: &[EffectId]) -> bool {
            self.controller.ok_to_use_alignment(ids)
        }

        fn run_hotkey(&mut self, command: HotkeyCommand) -> CommandOutcome {
            use HotkeyCommand as C;
            match command {
                C::ScrollRowsToTop => self.push(self.controller.scroll_to_top()),
                C::ScrollRowsToBottom => self.push(self.controller.scroll_to_bottom()),
                C::ScrollToSequenceStart => self.push(self.controller.scroll_to_start()),
                C::ScrollToSequenceEnd => self.push(self.controller.scroll_to_end()),
                C::PageRowsUp => self.push(self.controller.page_vertical(-1)),
                C::PageRowsDown => self.push(self.controller.page_vertical(1)),
                C::PageTimeBack => self.push(self.controller.page_time(-1)),
                C::PageTimeForward => self.push(self.controller.page_time(1)),
                C::NudgeSelectionBack => self.push(self.controller.nudge_selected(-1)),
                C::NudgeSelectionForward => self.push(self.controller.nudge_selected(1)),
                C::AlignStart(hold) => return self.align(TimelineAlign::Start, hold),
                C::AlignEnd(hold) => return self.align(TimelineAlign::End, hold),
                C::AlignBoth => return self.align(TimelineAlign::Both, false),
                C::AlignCenters => return self.align(TimelineAlign::Centers, false),
                C::AlignDurations(hold) => return self.align(TimelineAlign::Durations, hold),
                C::AlignStartToEnd(hold) => return self.align(TimelineAlign::StartToEnd, hold),
                C::AlignEndToStart(hold) => return self.align(TimelineAlign::EndToStart, hold),
                C::SplitAtPointer => {
                    if let Some((x, _)) = *self.pointer.lock() {
                        let time = self.controller.time_at_client(x);
                        self.push(self.controller.split_selected_at(time));
                    }
                }
                _ => return CommandOutcome::default(),
            }
            handled()
        }

        fn apply_prefs(&mut self, p: &EditorPrefs) {
            let current = self.controller.settings();
            self.push(self.controller.apply_settings(TimelineSettings {
                snap_enabled: p.snap_to_selected,
                snap_strength: p.snap_strength,
                draw_mode: p.draw_mode_selected,
                cad_style: p.cad_style_selection_box,
                legacy_cursor: p.legacy_cursor_active_row,
                zoom_under_mouse: p.zoom_under_mouse_position,
                highlight_rows: p.highlight_active_elements,
                show_effect_info: p.show_effect_info,
                resize_indicator: p.resize_indicator_enabled,
                resize_indicator_color: p.resize_indicator_color.clone(),
                waveform_height: current.waveform_height,
                ruler_height: current.ruler_height,
                lock_ruler_height: p.lock_ruler_height,
                lock_waveform_height: p.lock_waveform_height,
                full_waveform: p.full_waveform,
                splitter_distance: current.splitter_distance,
                close_gap_threshold: p.close_gap_threshold.clone(),
            }));
        }

        fn collect_prefs(&self, p: &mut EditorPrefs) {
            let s = self.controller.settings();
            p.wave_form_height = s.waveform_height;
            p.ruler_height = s.ruler_height;
            p.splitter_distance = s.splitter_distance;
        }

        fn store_view_state(&mut self) -> bool {
            self.controller.store_view_state();
            true
        }

        fn playback_ended(&mut self, _remembered: PlaybackRange) {
            // The timeline keeps its own remembered range, and restores it.
            self.push(self.controller.playback_ended());
        }

        fn goto_start(&mut self) -> bool {
            self.push(self.controller.playback_range_to_start());
            true
        }

        fn goto_end(&mut self) -> bool {
            self.push(self.controller.playback_range_to_end());
            true
        }

        fn remembered_range(&self) -> Option<PlaybackRange> {
            let (start, end) = self.controller.remembered_playback_range();
            Some(PlaybackRange { start, end })
        }

        fn set_drag_box_filter(&mut self, enabled: bool, types: &[Uuid]) {
            self.push(self.controller.set_drag_box_filter(enabled, types.iter().map(Uuid::to_string).collect()));
        }

        fn set_selected_effect(&mut self, type_id: Option<Uuid>) {
            self.push(self.controller.set_selected_effect_type(type_id.map(|t| t.to_string())));
        }
    }

    /// The window's own handles on the timeline grid.
    pub(super) struct TimelineLink {
        pub(super) pending: Arc<Mutex<Vec<TimelineUpdate>>>,
        pub(super) pointer: Arc<Mutex<Option<(i32, i32)>>>,
        pub(super) size: Arc<Mutex<(i32, i32)>>,
    }

    pub(super) fn make_grid(controller: Arc<TimelineController>, tree: &vixen_core::ElementTree) -> (TimelineGrid, TimelineLink) {
        let pending = Arc::new(Mutex::new(Vec::new()));
        let pointer = Arc::new(Mutex::new(None));
        let size = Arc::new(Mutex::new((1000, 600)));
        let grid = TimelineGrid { controller, rows: row_facts(tree), pending: Arc::clone(&pending), pointer: Arc::clone(&pointer), size: Arc::clone(&size) };
        (grid, TimelineLink { pending, pointer, size })
    }
}

#[cfg(feature = "timeline")]
#[uniffi::export]
impl SequencerShell {
    /// Commands from here on go to the real timeline.
    pub fn attach_timeline(&self, controller: Arc<crate::timeline::TimelineController>) {
        let tree = Arc::clone(self.session.session.lock().tree());
        let (grid, link) = with_timeline::make_grid(controller, &tree);
        self.state.lock().attach_grid(Box::new(grid));
        *self.timeline.lock() = Some(link);
    }

    /// What the last commands changed on the timeline's surfaces, for `TimelineCanvas.apply`.
    pub fn take_timeline_updates(&self) -> Vec<crate::timeline::TimelineUpdate> {
        self.timeline.lock().as_ref().map_or_else(Vec::new, |l| std::mem::take(&mut *l.pending.lock()))
    }

    /// `MousePosition`, in the grid's client coordinates, as of a key press.
    pub fn set_grid_pointer(&self, x: i32, y: i32) {
        if let Some(link) = self.timeline.lock().as_ref() {
            *link.pointer.lock() = Some((x, y));
        }
    }

    pub fn set_grid_size(&self, width: i32, height: i32) {
        if let Some(link) = self.timeline.lock().as_ref() {
            *link.size.lock() = (width, height);
        }
    }
}
