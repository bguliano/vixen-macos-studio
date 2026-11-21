//! One sequencer window's state: what `TimedSequenceEditorForm` holds in its fields and controls, plus the commands on it.

use crate::app::AppCore;
use crate::execution::ExecutionState;
use crate::playback::Transport;
use crate::sequence::{EffectId, SequenceSession};
use crate::sequencer::clipboard::{self, ClipboardData, CopyCandidate, CopyRow, PasteRow, PastingMode};
use crate::sequencer::effect_ops::{self, DistributeError, MarkAlign, MarkIndex};
use crate::sequencer::grid::{GridPort, RowSummary, ShellGrid};
use crate::sequencer::keys::{command_for_key, HotkeyCommand, Key, Mods};
use crate::sequencer::lifecycle::{self, AutoSaveClock, CloseAction};
use crate::sequencer::panels::{self, DockSide, PanelId, PanelLayout};
use crate::sequencer::prefs::EditorPrefs;
use crate::sequencer::toolbars::{self, StripId, ToolStripStates, ToolbarLayout};
use crate::sequencer::transport::{EngineCommand, PlaybackRange, SpeedControls, TransportController, TransportUi};
use crate::undo::{AddRemoveKind, MoveKind, UndoAction};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;
use vixen_core::{MarkCollection, TimeSpan};
use vixen_effects::framework::{EffectDescriptor, EffectGroup};
use vixen_io::{settings_dir, IoError, ProfileSettings, SelectedTimingProvider, TimedSequence};

/// `DefaultSequenceTime`: 1 minute.
pub const DEFAULT_SEQUENCE_LENGTH: TimeSpan = TimeSpan(60 * 10_000_000);
/// `_sequence.FileExtension`.
pub const SEQUENCE_EXTENSION: &str = ".tim";

/// `TimeSpan.ToString("m\\:ss\\.fff")`: the minutes component only, so past 59:59.999 the hours are lost (KEEP F257), and milliseconds truncate.
pub fn format_position(t: TimeSpan) -> String {
    let ticks = t.0.unsigned_abs();
    let ms = ticks / 10_000;
    let minutes = (ms / 60_000) % 60;
    let seconds = (ms / 1000) % 60;
    let sign = if t.0 < 0 { "-" } else { "" };
    format!("{sign}{minutes}:{seconds:02}.{:03}", ms % 1000)
}

/// `TimeSpan.ToString()`: `hh:mm:ss`, with `.fffffff` only when there's a fraction. F6's elapsed time and F325's start column.
pub fn format_timespan_raw(t: TimeSpan) -> String {
    let ticks = t.0.unsigned_abs();
    let fraction = ticks % 10_000_000;
    let total_seconds = ticks / 10_000_000;
    let days = total_seconds / 86_400;
    let hours = (total_seconds / 3600) % 24;
    let minutes = (total_seconds / 60) % 60;
    let seconds = total_seconds % 60;
    let mut text = String::new();
    if t.0 < 0 {
        text.push('-');
    }
    if days > 0 {
        text.push_str(&format!("{days}."));
    }
    text.push_str(&format!("{hours:02}:{minutes:02}:{seconds:02}"));
    if fraction > 0 {
        text.push_str(&format!(".{fraction:07}"));
    }
    text
}

/// What opening a sequence found. F8, F10, F11.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LoadReport {
    pub dropped_past_end: u32,
    pub shortened: u32,
    pub dropped_without_row: u32,
    /// Mark collections renamed for being duplicates: `(old, new)`.
    pub renames: Vec<(String, String)>,
    /// Elements tagged Deprecated with how many effects each has.
    pub deprecated: Vec<(String, u32)>,
}

/// A prompt to show. Text is as upstream words it; the window puts verbs on the buttons.
#[derive(Clone, Debug, PartialEq)]
pub struct Alert {
    pub title: String,
    pub message: String,
    pub style: AlertStyle,
    /// The default button's verb and what it does. `None` is an acknowledgement.
    pub confirm: Option<AlertConfirm>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AlertStyle {
    Information,
    Warning,
    Error,
}

/// A prompt's default button.
#[derive(Clone, Debug, PartialEq)]
pub struct AlertConfirm {
    pub label: String,
    pub cancel_label: String,
    pub action: AlertAction,
}

/// What a prompt's default button does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AlertAction {
    /// F295's no-selection warning.
    AlignToMarksEverywhere(MarkAlign),
    /// F303.
    RemoveAudio,
    /// F302: go on to the open panel. The window does that part.
    AssociateAudio,
    /// F296's no-selection warning.
    AddMarksEverywhere,
    /// F297.
    CreateDefaultMarks,
    /// F297.
    EnableDefaultMarks,
    /// F305's ⌘-drop.
    ReplaceSelected(Uuid),
    /// F244's warning.
    CloneToSelected(EffectId),
    /// F307's warning: the window carries on with the drop.
    ContinueDrop,
    /// F139's no-selection warning.
    CloseGapsEverywhere,
}

/// A window's request that Swift raises something it owns.
#[derive(Clone, Debug, PartialEq)]
pub enum UiRequest {
    /// F294.
    DistributeDialog { count: u32, start: TimeSpan, end: TimeSpan, length: TimeSpan },
    /// F204: the Effects panel clears its highlight.
    ClearEffectsSelection,
}

/// The result of a command.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CommandOutcome {
    pub alert: Option<Alert>,
    pub request: Option<UiRequest>,
    /// A cut or copy made this: the window puts it on the pasteboard.
    pub clipboard: Option<ClipboardData>,
    /// The key was used. An unused key falls through to the focused control.
    pub handled: bool,
}

impl CommandOutcome {
    pub(crate) fn handled() -> CommandOutcome {
        CommandOutcome { handled: true, ..CommandOutcome::default() }
    }

    fn alert(alert: Alert) -> CommandOutcome {
        CommandOutcome { alert: Some(alert), handled: true, ..CommandOutcome::default() }
    }
}

/// `UpdateToolStrip4`. A new message with a timeout restarts the timer rather than stacking, and an earlier one with no timeout isn't protected.
struct StatusLine {
    text: String,
    clear_at: Option<Instant>,
}

/// What the Audio items show. F9, F302, F303.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct AudioUi {
    pub remove_enabled: bool,
    pub beat_bar_enabled: bool,
    pub speed_tempo_enabled: bool,
    /// `toolStripButton_AssociateAudio.ToolTipText`.
    pub button_tooltip: String,
    /// `TimelineControl.Parent.Text`.
    pub timeline_title: String,
}

/// Everything menus, strips and the status bar read, in one call whenever something changes.
#[derive(Clone, Debug, PartialEq)]
pub struct UiState {
    pub title: String,
    pub sequence_name: String,
    pub profile_name: String,
    pub file_path: Option<String>,
    pub modified: bool,

    pub transport: TransportUi,
    pub speed: SpeedControls,

    pub has_selection: bool,
    pub selection_count: u32,
    pub paste: clipboard::PasteMenuState,
    pub undo_name: Option<String>,
    pub redo_name: Option<String>,
    pub audio: AudioUi,
    pub effect_count: u32,

    pub prefs: EditorPrefs,
    pub draw_mode: bool,
    pub selection_mode: bool,
    pub show_hidden_rows: bool,
    pub drag_box_filter: bool,
    pub drag_box_filter_types: Vec<Uuid>,
    /// `grid.SelectedEffect`: what Draw Mode draws.
    pub selected_effect: Option<Uuid>,

    pub position_text: String,
    pub length_text: String,
    pub status_message: String,
    pub rendering_visible: bool,
    pub rendering_percent: i32,
}

/// What the polling tick found.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct PollResult {
    /// Something a window shows changed: read `ui_state` again.
    pub changed: bool,
    /// `AutoSaveEventProcessor`: it's time to save. A never-saved sequence raises the save panel (KEEP F15).
    pub auto_save_due: bool,
}

pub use vixen_effects::framework::EffectGroup as EffectGroupPublic;

/// One entry of the effect catalogue: the Effects panel, the Effects strip, Add Effect and the drag box filter all read this one list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectCatalogEntry {
    pub type_id: Uuid,
    pub name: &'static str,
    pub group: EffectGroup,
    /// File name in `Resources/effects`, without the extension.
    pub icon: String,
}

/// `EffectDescriptor.icon` is `Vixen.Modules/Effect/<Dir>/...`. Resources name the file after the module in snake case.
fn icon_name(descriptor: &EffectDescriptor) -> String {
    let dir = descriptor.icon.split('/').nth(2).unwrap_or(descriptor.name);
    let chars: Vec<char> = dir.chars().collect();
    let mut name = String::with_capacity(dir.len() + 4);
    for (i, c) in chars.iter().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            let previous_lower = chars[i - 1].is_ascii_lowercase() || chars[i - 1].is_ascii_digit();
            let next_lower = chars.get(i + 1).is_some_and(char::is_ascii_lowercase);
            if previous_lower || (chars[i - 1].is_ascii_uppercase() && next_lower) {
                name.push('_');
            }
        }
        name.push(c.to_ascii_lowercase());
    }
    name
}

/// Every effect, in descriptor order. F150, F284, F320.
pub fn effect_catalog() -> Vec<EffectCatalogEntry> {
    vixen_effect_registry::all()
        .iter()
        .map(|d| EffectCatalogEntry { type_id: d.type_id, name: d.name, group: d.group, icon: icon_name(d) })
        .collect()
}

/// A snapshot to write away from the locks. Hazard 24: a save never freezes the window or stutters playback.
pub struct PreparedSave {
    document: TimedSequence,
    target: PathBuf,
}

impl PreparedSave {
    pub fn target(&self) -> &Path {
        &self.target
    }

    pub fn write(&self) -> Result<(), IoError> {
        vixen_io::save_sequence(&self.document, &self.target)
    }
}

/// How the Save items go. F13.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaveRoute {
    /// A file already: save in place.
    InPlace(PathBuf),
    /// Show the save panel here.
    Panel { directory: PathBuf, suggested_name: String },
}

/// One sequencer window.
pub struct SequencerState {
    playback_id: Uuid,
    pub core: Arc<AppCore>,
    pub session: Arc<Mutex<SequenceSession>>,
    pub grid: Box<dyn GridPort>,
    pub transport: TransportController,
    pub prefs: EditorPrefs,
    pub(crate) settings: Arc<ProfileSettings>,
    pub(crate) settings_dir: PathBuf,
    pub toolbars: ToolbarLayout,
    pub strip_states: ToolStripStates,
    pub panels: PanelLayout,
    auto_save: AutoSaveClock,
    last_modified: bool,
    /// `IsEditorStateModified`: set by row height changes, zoom and scroll, and never cleared. F16.
    editor_state_modified: bool,
    show_hidden_rows: bool,
    drag_box_filter: bool,
    drag_box_filter_types: Vec<Uuid>,
    pub(crate) selected_effect: Option<Uuid>,
    pub(crate) position_text: String,
    status: StatusLine,
    report: LoadReport,
    load_finished: bool,
    /// `_properties` for Add Multiple is process-wide; this is the clipboard count last seen on the pasteboard.
    pub(crate) clipboard_count: usize,
    render_baseline: usize,
    /// What the status bar last showed for F261, so `poll` notices the renders catching up.
    last_render_report: (bool, i32),
    last_execution: ExecutionState,
    was_active: bool,
    /// F12 answered with a mapping, so the sequence stays modified.
    mapping_applied: bool,
    drag_box_invert_checked: bool,
    /// `_colorCollections`: F253, F364.
    pub color_collections: Vec<crate::sequencer::libraries::ColorCollection>,
    /// F302: the length a "Resize Sequence" answer takes.
    pub(crate) pending_audio_length: Option<TimeSpan>,
}

impl SequencerState {
    /// The window's state over a session that's already open: `Form_Load`. Settings come from the shared store.
    pub fn attach(core: Arc<AppCore>, session: Arc<Mutex<SequenceSession>>) -> SequencerState {
        let settings = ProfileSettings::shared();
        let dir = settings_dir();
        SequencerState::with_settings(core, session, settings, dir)
    }

    pub fn with_settings(core: Arc<AppCore>, session: Arc<Mutex<SequenceSession>>, settings: Arc<ProfileSettings>, dir: PathBuf) -> SequencerState {
        let prefs = EditorPrefs::read(&settings);
        let toolbars = ToolbarLayout::restore(&settings);
        let profile = profile_name(&core);
        let strip_states = toolbars::load_tool_strip_states(&dir, &profile);
        let panels = panels::load_layout(&dir);
        let auto_save = AutoSaveClock::new(prefs.auto_save_interval_ms);
        let tree = Arc::clone(session.lock().tree());
        let grid: Box<dyn GridPort> = Box::new(ShellGrid::new(&tree));
        // F285: effects this window adds start from the saved defaults of their type.
        session.lock().set_effect_defaults(core.effect_defaults());
        let color_collections = crate::sequencer::libraries::load_color_collections(&dir);
        // Playback restores the saved audio device at profile construction, so
        // opening another editor cannot reset a current selection.
        let last_modified = session.lock().is_modified();
        let last_execution = core.playback.execution_state();
        let draw_mode = prefs.draw_mode_selected;
        let playback_id = session.lock().playback_id();
        let mut state = SequencerState {
            playback_id,
            core,
            session,
            grid,
            transport: TransportController::new(),
            prefs,
            settings,
            settings_dir: dir,
            toolbars,
            strip_states,
            panels,
            auto_save,
            last_modified,
            editor_state_modified: false,
            show_hidden_rows: false,
            drag_box_filter: false,
            drag_box_filter_types: Vec::new(),
            selected_effect: None,
            position_text: "0:00.000".to_string(),
            status: StatusLine { text: String::new(), clear_at: None },
            report: LoadReport::default(),
            load_finished: false,
            clipboard_count: 0,
            render_baseline: 0,
            last_render_report: (false, 100),
            last_execution,
            was_active: false,
            mapping_applied: false,
            drag_box_invert_checked: false,
            color_collections,
            pending_audio_length: None,
        };
        state.prefs.draw_mode_selected = draw_mode;
        state
    }

    /// Stands in for clicking a row's label, for tests and the shell-only build, where there's no row list to click.
    pub fn set_selected_row_for_tests(&mut self, row: Option<u32>) {
        self.grid.select_row_for_tests(row);
    }

    /// Replaces the plain grid with the real timeline, which has loaded rows, zoom, scroll and range from the session itself.
    pub fn attach_grid(&mut self, mut grid: Box<dyn GridPort>) {
        grid.set_show_hidden(self.show_hidden_rows);
        grid.apply_prefs(&self.prefs);
        grid.set_selected_effect(self.selected_effect);
        grid.set_drag_box_filter(self.drag_box_filter, &self.drag_box_filter_types);
        self.grid = grid;
    }

    /// The menus' and strips' toggles changed `prefs`.
    pub fn prefs_changed(&mut self) {
        self.grid.apply_prefs(&self.prefs);
    }

    // ------------------------------------------------------------------------------------------------------------------
    // Opening: F6 to F12
    // ------------------------------------------------------------------------------------------------------------------

    /// Effects whose first target isn't known to this profile: `(node id, node name)`, each once. F12: asked about before the window loads.
    pub fn unmapped_targets(&self) -> Vec<(Uuid, String)> {
        let session = self.session.lock();
        let mut seen = Vec::new();
        for effect in session.effects() {
            if effect.targets.is_empty() && effect.descriptor.is_some() {
                if let Some((id, name)) = effect.node.targets.first() {
                    if !seen.iter().any(|(s, _): &(Uuid, String)| s == id) {
                        seen.push((*id, name.clone()));
                    }
                }
            }
        }
        seen
    }

    /// F12: `foreignSequence`, a sequence from outside the profile's sequence folder.
    pub fn is_foreign_location(&self) -> bool {
        let session = self.session.lock();
        let Some(path) = session.path() else { return true };
        path.parent() != Some(self.core.paths.sequence_dir().as_path())
    }

    /// F12's answers. `None`: "No", with F8 then dropping the effects at load. `Some(map)`: source to target; anything the map doesn't reach is
    /// removed. An empty map is a cancelled mapper: all are removed. A foreign sequence loses its path so Save asks for a name.
    pub fn apply_element_map(&mut self, map: Option<&[(Uuid, Uuid)]>) {
        let foreign = self.is_foreign_location();
        let mut session = self.session.lock();
        if let Some(map) = map {
            let tree = Arc::clone(session.tree());
            let unmapped: Vec<(EffectId, Option<Uuid>)> = session
                .effects()
                .filter(|e| e.targets.is_empty() && e.descriptor.is_some())
                .map(|e| (e.id, e.node.targets.first().map(|(id, _)| *id)))
                .collect();
            let mut remove = Vec::new();
            for (id, source) in unmapped {
                let target = source
                    .and_then(|s| map.iter().find(|(from, _)| *from == s).map(|(_, to)| *to))
                    .and_then(|to| tree.find(&to).map(|node| (to, tree.node(node).name.clone())));
                match target {
                    Some((to, name)) => session.set_target(id, to, &name),
                    None => remove.push(id),
                }
            }
            if !remove.is_empty() {
                session.remove_effects(&remove);
            }
            // Upstream marks it modified here and then clears that at load for a sequence in the normal folder, so the mapping can be lost
            // with no prompt. It stays modified here (F12's FIX). `finish_load` respects this.
            session.set_modified();
            drop(session);
            self.mapping_applied = true;
        } else {
            drop(session);
        }
        if foreign {
            self.session.lock().clear_path();
        }
    }

    /// The rest of `LoadSequence`, on the window's own thread of work so the window never waits: the load rules, row settings, view state,
    /// the rename summary and the deprecated list. Call once after any F12 answer.
    pub fn finish_load(&mut self) {
        if self.load_finished {
            return;
        }
        self.load_finished = true;
        let mapping_applied = self.mapping_applied;
        let mut session = self.session.lock();
        let length = session.length();

        // `AddElementsForEffectNodes`, F8. These silently change data, as upstream's do.
        let mut remove = Vec::new();
        let mut shorten = Vec::new();
        let mut deprecated: HashMap<String, u32> = HashMap::new();
        let mut deprecated_order: Vec<String> = Vec::new();
        for effect in session.effects() {
            if effect.node.start > length {
                self.report.dropped_past_end += 1;
                remove.push(effect.id);
                continue;
            }
            if effect.end() > length {
                self.report.shortened += 1;
                shorten.push((effect.id, effect.node.start, length - effect.node.start));
            }
            if effect.targets.is_empty() && effect.descriptor.is_some() {
                self.report.dropped_without_row += 1;
                remove.push(effect.id);
            }
        }
        // F10: `CheckDeprecatedEffects` looks at what stays.
        for effect in session.effects() {
            if remove.contains(&effect.id) {
                continue;
            }
            let tree = session.tree();
            if let Some(node) = effect.targets.first() {
                let node = tree.node(*node);
                if node.tags.contains(&crate::sequencer::grid::DEPRECATED_TAG) {
                    let count = deprecated.entry(node.name.clone()).or_insert_with(|| {
                        deprecated_order.push(node.name.clone());
                        0
                    });
                    *count += 1;
                }
            }
        }
        self.report.deprecated = deprecated_order.into_iter().map(|n| (n.clone(), deprecated[&n])).collect();
        if !shorten.is_empty() {
            session.set_times(&shorten);
        }
        if !remove.is_empty() {
            session.remove_effects(&remove);
        }

        // `MarkCollectionNameService.RenameDuplicates`, F11.
        let mut marks = session.marks().to_vec();
        let renames = lifecycle::rename_duplicate_mark_collections(&mut marks);
        if !renames.is_empty() {
            session.set_marks(marks);
        }
        self.report.renames = renames;

        session.render_all_dirty();

        // F6: a new sequence starts modified. One with a file doesn't, unless a rename or a mapping changed it.
        if session.path().is_none() || !self.report.renames.is_empty() || mapping_applied {
            session.set_modified();
        } else {
            session.set_unmodified();
        }
        self.last_modified = session.is_modified();
        let enabled = self.prefs.auto_save_enabled;
        self.auto_save.on_modified_changed(self.last_modified, enabled, Instant::now());

        let document = session.document();
        let tpp = document.time_per_pixel;
        let visible_start = document.visible_time_start;
        let range = document.default_playback_end_time;
        let start = document.default_playback_start_time;
        let length = document.length;
        drop(session);

        self.grid.set_total(length);
        if tpp > TimeSpan::ZERO {
            self.grid.set_time_per_pixel(tpp);
        }
        if visible_start > TimeSpan::ZERO {
            self.grid.set_visible_start(visible_start);
        }
        // F1: restored unless the end is exactly zero. A nil end counts as set.
        if range != Some(TimeSpan::ZERO) {
            let restored = PlaybackRange { start, end: range };
            self.grid.set_playback_range(restored);
            self.transport.set_remembered(restored);
        }
        self.apply_timing_enabled_state();
    }

    pub fn load_report(&self) -> &LoadReport {
        &self.report
    }

    /// F9: the missing file, when the first media has gone.
    pub fn missing_audio(&self) -> Option<PathBuf> {
        let session = self.session.lock();
        let media = session.document().media.first()?;
        let path = self.core.paths.media_dir().join(&media.file_name);
        (!path.exists()).then_some(path)
    }

    /// `PopulateWaveformAudio` and what F302 and F303 change. With Proceed As Is on a missing file, Remove Audio and Beat/Bar stay enabled while
    /// the rest stay at their defaults (F9).
    pub fn audio_ui(&self) -> AudioUi {
        let session = self.session.lock();
        audio_ui_for(session.document(), &self.core.paths.media_dir())
    }

    /// `_SetTimingToolStripEnabledState`.
    pub(crate) fn apply_timing_enabled_state(&mut self) {
        let enabled = {
            let session = self.session.lock();
            timing_source_exists(session.document(), &self.core.paths.media_dir())
        };
        self.transport.set_speed_controls_enabled(enabled);
    }

    // ------------------------------------------------------------------------------------------------------------------
    // Saving: F13 to F16
    // ------------------------------------------------------------------------------------------------------------------

    /// Save with no path and no prompt: in place, or the panel for a never-saved sequence.
    pub fn save_route(&self, force_panel: bool) -> SaveRoute {
        let session = self.session.lock();
        match session.path() {
            Some(path) if !force_panel => SaveRoute::InPlace(path.to_path_buf()),
            _ => SaveRoute::Panel {
                directory: self.core.paths.sequence_dir(),
                suggested_name: format!("{}{SEQUENCE_EXTENSION}", session.name()),
            },
        }
    }

    /// `SaveGridRowSettings`, F14: what a save stores about the window. Without the real timeline the rows' settings stay as they loaded.
    fn store_row_settings(&mut self, session: &mut SequenceSession) {
        let range = self.grid.playback_range();
        let document = session.document_mut();
        document.default_playback_start_time = range.start;
        document.default_playback_end_time = range.end;
    }

    /// The first half of `SaveSequence`: the panel's choice goes through the extension rule, and a snapshot comes back to write off the main thread.
    pub fn prepare_save(&mut self, chosen: Option<&Path>) -> Result<PreparedSave, crate::SessionError> {
        let grid_stored = self.grid.store_view_state();
        let session = Arc::clone(&self.session);
        let mut session = session.lock();
        let target = match chosen {
            Some(path) => lifecycle::save_path_with_extension(path, SEQUENCE_EXTENSION),
            None => session.path().map(Path::to_path_buf).ok_or_else(|| crate::SessionError::Other("This sequence has no file yet.".to_string()))?,
        };
        if !grid_stored {
            self.store_row_settings(&mut session);
        }
        let document = session.begin_save();
        Ok(PreparedSave { document, target })
    }

    /// The second half: the window's title and auto save follow.
    pub fn finish_save(&mut self, target: PathBuf, succeeded: bool) {
        self.session.lock().finish_save(Some(target).filter(|_| succeeded), succeeded);
        self.note_modified_transition(Instant::now());
    }

    /// `_CloseEditor`'s choice. F16.
    pub fn close_action(&self, app_closing: bool) -> CloseAction {
        lifecycle::close_action(self.session.lock().is_modified(), self.editor_state_modified, app_closing)
    }

    /// `EditorClosing`: F17, and playback stopping with the window. The window is going, so a settings failure comes back as text for the log. The
    /// recent list is the app's, through `AppModel.editorClosed`.
    pub fn finish_close(&mut self) -> Option<String> {
        self.core.playback.stop_session(self.playback_id);
        self.session.lock().set_schedule_listener(None);
        self.session.lock().close_video_cache();
        self.save_window_settings().err().map(|e| e.to_string())
    }

    fn save_window_settings(&mut self) -> Result<(), IoError> {
        self.grid.collect_prefs(&mut self.prefs);
        self.prefs.write(&self.settings)?;
        self.toolbars.save(&self.settings)?;
        let profile = profile_name(&self.core);
        toolbars::save_tool_strip_states(&self.settings_dir, &profile, &self.strip_states)?;
        panels::save_layout(&self.settings_dir, &self.panels)
    }

    // ------------------------------------------------------------------------------------------------------------------
    // Keeping up: F15, F19, F260, F262, F267
    // ------------------------------------------------------------------------------------------------------------------

    fn note_modified_transition(&mut self, now: Instant) -> bool {
        let modified = self.session.lock().is_modified();
        if modified == self.last_modified {
            return false;
        }
        self.last_modified = modified;
        self.auto_save.on_modified_changed(modified, self.prefs.auto_save_enabled, now);
        true
    }

    /// The low-rate tick: a few times a second, and only reads what's cheap.
    pub fn poll(&mut self, now: Instant) -> PollResult {
        let mut changed = self.note_modified_transition(now);
        let auto_save_due = self.auto_save.due(now) && self.session.lock().is_modified();

        if let Some(at) = self.status.clear_at {
            if now >= at {
                self.status = StatusLine { text: String::new(), clear_at: None };
                changed = true;
            }
        }

        // `OnExecutionStateChanged`.
        let execution = self.core.playback.execution_state();
        if execution != self.last_execution {
            match execution {
                ExecutionState::Closing => self.transport.set_has_context(false),
                ExecutionState::Open | ExecutionState::OpenForTesting => self.transport.set_has_context(true),
                _ => {}
            }
            self.last_execution = execution;
            changed = true;
        }

        if self.transport_tick(now) {
            changed = true;
        }

        // F261: renders finish on their own, with nothing else to say the bar should move or go.
        let session = Arc::clone(&self.session);
        let report = self.render_progress(&session.lock());
        if report != self.last_render_report {
            self.last_render_report = report;
            changed = true;
        }
        PollResult { changed, auto_save_due }
    }

    /// Keep ticking through the final pending end transition as well as playback/countdown.
    pub fn transport_active(&self) -> bool {
        self.transport.needs_tick(self.engine_state())
    }

    /// `timerPlaying_Tick`, the delay timers and the context's events. Returns whether anything changed besides the position.
    pub fn transport_tick(&mut self, now: Instant) -> bool {
        let engine = self.engine_state();
        let live = self.grid.playback_range();
        let events = self.transport.tick(engine, live, now);
        let mut changed = self.was_active != self.transport_active();
        self.was_active = self.transport_active();
        if let Some(range) = events.restore_range {
            self.context_ended(range);
            changed = true;
        }
        if events.started {
            changed = true;
        }
        if events.command != EngineCommand::None {
            self.apply_engine(events.command);
            changed = true;
        }
        if self.engine_state() != Transport::Stopped {
            let position = self.core.playback.session_position(self.playback_id);
            self.position_text = format_position(position);
            self.grid.set_playback_current(Some(position));
        }
        changed
    }

    /// `context_ContextEnded`: the live range goes back, and the playback time clears.
    fn context_ended(&mut self, range: PlaybackRange) {
        self.grid.playback_ended(range);
        self.position_text = String::new();
        self.core.playback.take_session_finished(self.playback_id);
        // A retired window may receive its end tick after another editor starts.
        if self.owns_playback() {
            self.core.notify_player_active(false);
        }
    }

    fn owns_playback(&self) -> bool {
        self.core.playback.owns_session(self.playback_id)
    }

    /// This window's view of the engine: another window's playback isn't this window's.
    pub fn engine_state(&self) -> Transport {
        self.core.playback.session_transport(self.playback_id)
    }

    fn apply_engine(&mut self, command: EngineCommand) {
        let playback = Arc::clone(&self.core.playback);
        // `_PlaySequence` calls `PlayerStarted` on every preview before it resumes or plays. F269.
        if matches!(command, EngineCommand::Play { .. } | EngineCommand::Resume) {
            self.core.notify_player_active(true);
        }
        match command {
            EngineCommand::None => return,
            EngineCommand::Play { start, end, looping } => {
                playback.set_rate_mode(self.prefs.use_tempo_for_speed);
                {
                    let mut session = self.session.lock();
                    playback.play(&mut session, start, end, looping);
                }
                // The `TimingSource` setter. The default timing source takes no speed, so speed works only with audio. F272.
                let audio = {
                    let session = self.session.lock();
                    timing_source_exists(session.document(), &self.core.paths.media_dir()) && session.audio().is_some()
                };
                if let Some(speed) = self.transport.timing_source_started(audio) {
                    playback.set_session_speed(self.playback_id, speed);
                }
            }
            EngineCommand::Resume => playback.resume_session(self.playback_id),
            EngineCommand::Pause => playback.pause_session(self.playback_id),
            EngineCommand::Stop => playback.stop_session(self.playback_id),
        }
        let engine = self.engine_state();
        if let Some(range) = self.transport.note_engine(engine) {
            self.context_ended(range);
        }
    }

    // ------------------------------------------------------------------------------------------------------------------
    // Transport commands: F27 to F36, F105 to F109, F263 to F274
    // ------------------------------------------------------------------------------------------------------------------

    pub fn play_pause(&mut self, now: Instant) {
        let command = self.transport.play_pause_toggle(self.engine_state(), self.grid.playback_range(), now);
        self.apply_engine(command);
    }

    pub fn stop(&mut self) {
        let command = self.transport.stop(self.engine_state());
        self.apply_engine(command);
    }

    /// F105: range start and remembered start go to 0, and the view to the start. Not marked modified.
    pub fn goto_start(&mut self) {
        if self.grid.goto_start() {
            return;
        }
        let mut range = self.grid.playback_range();
        range.start = Some(TimeSpan::ZERO);
        self.grid.set_playback_range(range);
        let mut remembered = self.transport.remembered();
        remembered.start = Some(TimeSpan::ZERO);
        self.transport.set_remembered(remembered);
        self.grid.set_visible_start(TimeSpan::ZERO);
    }

    /// F108.
    pub fn goto_end(&mut self) {
        if self.grid.goto_end() {
            return;
        }
        let length = self.session.lock().length();
        let mut range = self.grid.playback_range();
        range.end = Some(length);
        self.grid.set_playback_range(range);
        let mut remembered = self.transport.remembered();
        remembered.end = Some(length);
        self.transport.set_remembered(remembered);
        self.grid.set_visible_start(length - self.grid.visible_span());
    }

    /// F265: the marks bar's play range. Paused, it stops first.
    pub fn play_between(&mut self, start: TimeSpan, end: TimeSpan) {
        if self.engine_state() == Transport::Paused {
            self.stop();
        }
        let total = self.session.lock().length();
        let command = self.transport.play_between(start, end, total, self.grid.playback_range());
        self.apply_engine(command);
    }

    /// F273's left button.
    pub fn ruler_clicked(&mut self, time: TimeSpan, cmd_held: bool) {
        let click = self.transport.ruler_clicked(time, cmd_held, self.grid.playback_range());
        if !click.modified {
            return;
        }
        self.grid.set_playback_range(click.live);
        self.session.lock().set_modified();
        if click.move_cursor {
            self.set_cursor(time);
        }
        self.apply_engine(click.command);
    }

    /// F274.
    pub fn ruler_range_begin(&mut self) {
        let live = self.grid.playback_range();
        self.transport.ruler_range_begin(live);
    }

    /// F274. The grid has the dragged range by now.
    pub fn ruler_range_end(&mut self, cmd_held: bool) {
        let command = self.transport.ruler_range_end(cmd_held, self.grid.playback_range());
        self.apply_engine(command);
    }

    /// `CursorMovedHandler`: the status bar follows the cursor. F257.
    pub fn set_cursor(&mut self, t: TimeSpan) {
        self.grid.set_cursor(t);
        self.position_text = format_position(t);
    }

    /// The timeline's cursor moved by its own hand: only the label follows.
    pub fn cursor_moved(&mut self, t: TimeSpan) {
        self.position_text = format_position(t);
    }

    /// Forward a speed the controller allowed to whatever's playing. `_SetTimingSpeed` does it only when there's a timing source.
    pub fn forward_speed(&mut self, speed: Option<f32>) {
        if let Some(speed) = speed {
            if self.engine_state() != Transport::Stopped {
                self.core.playback.set_session_speed(self.playback_id, speed);
            }
        }
    }

    /// F81: another editor's preference must not change the active sequence's audio mode.
    pub fn set_use_tempo(&mut self, on: bool) {
        self.prefs.use_tempo_for_speed = on;
        self.core.playback.set_session_rate_mode(self.playback_id, on);
    }

    // ------------------------------------------------------------------------------------------------------------------
    // Sequence: F4, F26, F83
    // ------------------------------------------------------------------------------------------------------------------

    /// Tools > Sequence Length: no undo, and effects past a shorter length stay until the next load (F83).
    pub fn set_sequence_length(&mut self, length: TimeSpan) {
        self.session.lock().set_length(length);
        self.grid.set_total(length);
    }

    pub fn set_auto_save(&mut self, on: bool) {
        self.prefs.auto_save_enabled = on;
        let modified = self.session.lock().is_modified();
        self.auto_save.set_enabled(on, modified, Instant::now());
    }

    /// `UpdateToolStrip4`.
    pub fn set_status_message(&mut self, text: &str, timeout_seconds: u32, now: Instant) {
        self.status.text = text.to_string();
        if timeout_seconds > 0 {
            self.status.clear_at = Some(now + Duration::from_secs(timeout_seconds as u64));
        }
    }

    // ------------------------------------------------------------------------------------------------------------------
    // View: zoom and scroll, F54 to F58, F181 to F188
    // ------------------------------------------------------------------------------------------------------------------

    fn view_changed(&mut self) {
        // `TimelineControl_TimePerPixelChanged` and `VisibleTimeStartChanged` keep the sequence's copy and set the state flag: it saves silently at
        // close (KEEP F16).
        let tpp = self.grid.time_per_pixel();
        let start = self.grid.visible_start();
        let mut session = self.session.lock();
        let document = session.document_mut();
        document.time_per_pixel = tpp;
        document.visible_time_start = start;
        self.editor_state_modified = true;
    }

    pub fn zoom_time(&mut self, scale: f64) {
        self.grid.zoom_time(scale);
        self.view_changed();
    }

    pub fn zoom_rows(&mut self, scale: f64) {
        self.grid.zoom_rows(scale);
        self.editor_state_modified = true;
    }

    /// The real timeline tells the window about zoom, scroll and row height changes through this.
    pub fn note_editor_state_modified(&mut self) {
        self.view_changed();
    }

    pub fn set_show_hidden_rows(&mut self, show: bool) {
        self.show_hidden_rows = show;
        self.grid.set_show_hidden(show);
    }

    // ------------------------------------------------------------------------------------------------------------------
    // Hotkeys: section D
    // ------------------------------------------------------------------------------------------------------------------

    pub fn run_key(&mut self, key: Key, mods: Mods, now: Instant) -> CommandOutcome {
        match command_for_key(key, mods) {
            Some(command) => self.run_hotkey(command, now),
            None => CommandOutcome::default(),
        }
    }

    /// `OnKeyDown`'s bodies. Alignment that needs the timeline's own commands comes through `GridPort`.
    pub fn run_hotkey(&mut self, command: HotkeyCommand, now: Instant) -> CommandOutcome {
        use HotkeyCommand as C;
        // The timeline's own commands go straight to it. What it doesn't take comes down here.
        let outcome = self.grid.run_hotkey(command);
        if outcome.handled {
            return outcome;
        }
        match command {
            C::ScrollRowsToTop => self.grid.set_vertical_offset(0),
            C::ScrollToSequenceStart => {
                self.grid.set_visible_start(TimeSpan::ZERO);
                self.view_changed();
            }
            // A bit iffy, as upstream says, but the grid caps it to what's visible.
            C::ScrollRowsToBottom => self.grid.set_vertical_offset(i32::MAX),
            C::ScrollToSequenceEnd => {
                let length = self.session.lock().length();
                self.grid.set_visible_start(length - self.grid.visible_span());
                self.view_changed();
            }
            C::PageRowsUp => {
                let y = self.grid.vertical_offset() - self.grid.visible_height() / 2;
                self.grid.set_vertical_offset(y);
            }
            C::PageRowsDown => {
                let y = self.grid.vertical_offset().saturating_add(self.grid.visible_height() / 2);
                self.grid.set_vertical_offset(y);
            }
            C::PageTimeBack => {
                let half = TimeSpan((self.grid.visible_span().0 as f64 * 0.5) as i64);
                self.grid.set_visible_start(self.grid.visible_start() - half);
                self.view_changed();
            }
            C::PageTimeForward => {
                let half = TimeSpan((self.grid.visible_span().0 as f64 * 0.5) as i64);
                self.grid.set_visible_start(self.grid.visible_start() + half);
                self.view_changed();
            }
            C::ActiveRowUp => {
                return CommandOutcome { handled: self.grid.move_active_row(-1), ..CommandOutcome::default() };
            }
            C::ActiveRowDown => {
                return CommandOutcome { handled: self.grid.move_active_row(1), ..CommandOutcome::default() };
            }
            C::SpacePlayStop | C::SpacePlayPause => {
                let pause = command == C::SpacePlayPause;
                let command = self.transport.spacebar(pause, self.engine_state(), self.grid.playback_range(), now);
                self.apply_engine(command);
            }
            C::EscapeDrawMode => {
                // If drawing, Esc does nothing.
                if self.grid.is_drawing() {
                    return CommandOutcome::default();
                }
                self.selected_effect = None;
                self.grid.set_selected_effect(None);
                self.prefs.draw_mode_selected = false;
                self.prefs.selection_mode_selected = true;
                self.prefs_changed();
                // During a drag the key goes on to the grid too, which cancels it (timeline F64): the form sees keys first, and the grid still gets them.
                let handled = !self.grid.is_dragging();
                return CommandOutcome { request: Some(UiRequest::ClearEffectsSelection), handled, ..CommandOutcome::default() };
            }
            C::ZoomTimeOut => self.zoom_time(1.25),
            C::ZoomRowsOut => self.zoom_rows(0.8),
            C::ZoomTimeIn => self.zoom_time(0.8),
            C::ZoomRowsIn => self.zoom_rows(1.25),
            C::ToggleSelectedRows { include_children } => self.grid.toggle_selected_rows(include_children),
            C::PlayPause => self.play_pause(now),
            C::Stop => self.stop(),
            C::ToggleLoop => {
                let on = !self.transport.looping();
                self.transport.set_loop(on);
            }
            C::DeleteSelected => {
                self.delete_selected();
            }
            C::DistributeEqually => return self.distribute_equally_selected(),
            C::AlignStartToNearestMark => return self.align_to_marks(MarkAlign::Start, false),
            C::AlignEndToNearestMark => return self.align_to_marks(MarkAlign::End, false),
            C::AlignBothToNearestMark => return self.align_to_marks(MarkAlign::Both, false),
            C::DistributeDialog => {
                if self.grid.is_drag_in_progress() {
                    return CommandOutcome::default();
                }
                let selected = self.grid.selected_elements();
                match self.grid.element_under_pointer() {
                    Some(under) if selected.len() > 1 && selected.contains(&under) => return self.distribute_dialog_request(),
                    _ => {}
                }
            }
            // A grid with no timeline has none of these to do.
            // `if (TimingSource != null)`: only while something's playing.
            C::AddMarkAtPlayPosition => {
                if self.engine_state() != Transport::Stopped {
                    self.grid.add_mark_at(self.core.playback.position());
                }
            }
            C::NudgeSelectionBack
            | C::NudgeSelectionForward
            | C::AlignStart(_)
            | C::AlignEnd(_)
            | C::AlignBoth
            | C::SplitAtPointer
            | C::AlignCenters
            | C::AlignDurations(_)
            | C::AlignStartToEnd(_)
            | C::AlignEndToStart(_) => return CommandOutcome::default(),
        }
        CommandOutcome::handled()
    }

    // ------------------------------------------------------------------------------------------------------------------
    // Effects: F284 to F295
    // ------------------------------------------------------------------------------------------------------------------

    fn row_for_new_effect(&self) -> Option<u32> {
        self.grid.active_row().or_else(|| self.grid.selected_row())
    }

    /// Edit > Add Effect: F284. Nothing happens with no row chosen.
    pub fn add_effect_from_menu(&mut self, type_id: Uuid) -> Option<EffectId> {
        let row = self.row_for_new_effect()?;
        let start = self.grid.cursor();
        let visible_end = self.grid.visible_start() + self.grid.visible_span();
        let duration = effect_ops::default_effect_duration(visible_end, start);
        self.add_effect(type_id, row, start, duration, true)
    }

    /// `AddEffectInstance`: F285.
    pub fn add_effect(&mut self, type_id: Uuid, row: u32, start: TimeSpan, duration: TimeSpan, select: bool) -> Option<EffectId> {
        let summary = self.grid.row_summary(row)?;
        let mut session = self.session.lock();
        let (start, duration) = effect_ops::fit_new_effect(start, duration, session.length());
        let id = session.add_new_effect(type_id, summary.node, &summary.name, start, duration)?;
        session.record_undo(UndoAction::EffectsAdded { kind: AddRemoveKind::AddedOrRemoved, ids: vec![id] });
        drop(session);
        crate::studio_effects_library::used(type_id); // STUDIO: the Effects library's Recently Used (R38)
        self.grid.effects_added(&[id]);
        if select {
            self.grid.set_selection(&[id]);
        }
        Some(id)
    }

    /// `RemoveSelectedElements`: F290. One undo step, and the effects stay in memory for it.
    pub fn delete_selected(&mut self) {
        let selected = self.grid.selected_elements();
        if selected.is_empty() {
            return;
        }
        let snapshots = {
            let mut session = self.session.lock();
            let snapshots = session.remove_effects(&selected);
            session.record_undo(UndoAction::EffectsRemoved { kind: AddRemoveKind::AddedOrRemoved, effects: snapshots.clone() });
            snapshots
        };
        let _ = snapshots;
        self.grid.effects_removed(&selected);
        self.grid.remove_from_selection(&selected);
    }

    /// `selectAllElementsToolStripMenuItem_Click`.
    pub fn select_all(&mut self) {
        let all: Vec<EffectId> = self.session.lock().effects().map(|e| e.id).collect();
        self.grid.set_selection(&all);
    }

    /// `OkToUseAlignmentHelper`: at most 32 selected per row. The grid decides; a plain grid holds no stacks, so it says yes.
    pub fn ok_to_use_alignment(&self) -> bool {
        self.grid.ok_to_use_alignment(&self.grid.selected_elements())
    }

    fn selected_times(&self) -> Vec<(EffectId, TimeSpan, TimeSpan)> {
        let session = self.session.lock();
        self.grid
            .selected_elements()
            .into_iter()
            .filter_map(|id| session.effect(id).map(|e| (id, e.node.start, e.node.time_span)))
            .collect()
    }

    /// Applies `(effect, start, end)` as one undo step: `MoveResizeElements`. It skips any item ending past the total or under 1 ms, yet all stay in the
    /// undo, as upstream's do (`Grid.cs:1350-1375`).
    pub(crate) fn move_resize(&mut self, moves: &[(EffectId, TimeSpan, TimeSpan)], kind: MoveKind) {
        let mut session = self.session.lock();
        let total = session.length();
        let one_ms = TimeSpan::from_ms_i(1);
        let mut before = Vec::with_capacity(moves.len());
        let mut changes = Vec::with_capacity(moves.len());
        for (id, start, end) in moves {
            let Some(effect) = session.effect(*id) else { continue };
            before.push((*id, effect.node.start, effect.node.time_span));
            if *end > total || *end - *start < one_ms {
                continue;
            }
            changes.push((*id, *start, *end - *start));
        }
        session.set_times(&changes);
        session.record_undo(UndoAction::TimesChanged { kind, times: before });
        session.render_all_dirty();
        drop(session);
        self.grid.effects_moved(moves);
    }

    /// `DistributeSelectedEffectsEqually`: F293.
    pub fn distribute_equally_selected(&mut self) -> CommandOutcome {
        if !self.ok_to_use_alignment() {
            return CommandOutcome::alert(Alert {
                title: String::new(),
                message: ALIGNMENT_HELPER_WARNING.to_string(),
                style: AlertStyle::Warning,
                confirm: None,
            });
        }
        let selected = self.selected_times();
        if selected.is_empty() {
            return CommandOutcome::handled();
        }
        let times: Vec<(TimeSpan, TimeSpan)> = selected.iter().map(|(_, s, d)| (*s, *d)).collect();
        match effect_ops::distribute_equally(&times) {
            Err(DistributeError::TooShort { message, title }) => {
                CommandOutcome::alert(Alert { title: title.to_string(), message, style: AlertStyle::Warning, confirm: None })
            }
            Ok(placed) => {
                let moves: Vec<(EffectId, TimeSpan, TimeSpan)> =
                    selected.iter().zip(&placed).map(|((id, ..), (start, end))| (*id, *start, *end)).collect();
                self.move_resize(&moves, MoveKind::Distributed);
                CommandOutcome::handled()
            }
        }
    }

    /// The checks before F294's dialog.
    pub fn distribute_dialog_request(&mut self) -> CommandOutcome {
        if !self.ok_to_use_alignment() {
            return CommandOutcome::alert(Alert {
                title: String::new(),
                message: ALIGNMENT_HELPER_WARNING.to_string(),
                style: AlertStyle::Warning,
                confirm: None,
            });
        }
        let selected = self.selected_times();
        if selected.len() < 2 {
            return CommandOutcome::alert(Alert {
                title: "Select more effects".to_string(),
                message: "Select at least two effects to distribute.".to_string(),
                style: AlertStyle::Information,
                confirm: None,
            });
        }
        let times: Vec<(TimeSpan, TimeSpan)> = selected.iter().map(|(_, s, d)| (*s, *d)).collect();
        let (start, end) = effect_ops::distribute_dialog_span(&times).unwrap_or((TimeSpan::ZERO, TimeSpan::ZERO));
        let length = self.session.lock().length();
        CommandOutcome {
            request: Some(UiRequest::DistributeDialog { count: selected.len() as u32, start, end, length }),
            handled: true,
            ..CommandOutcome::default()
        }
    }

    /// F294's OK. `MoveResizeElements(..., ElementMoveType.Distribute)`.
    pub fn apply_distribute(&mut self, params: &effect_ops::DistributeParams) {
        let selected = self.selected_times();
        let times: Vec<(TimeSpan, TimeSpan)> = selected.iter().map(|(_, s, d)| (*s, *d)).collect();
        let placed = effect_ops::distribute(&times, params);
        let moves: Vec<(EffectId, TimeSpan, TimeSpan)> =
            selected.iter().zip(&placed).map(|((id, ..), (start, end))| (*id, *start, *end)).collect();
        if !moves.is_empty() {
            self.move_resize(&moves, MoveKind::Distributed);
        }
    }

    /// `CloseGapsBetweenElements`: F139. With nothing selected it asks first, and then applies to every effect. The threshold parses with a fixed `.`.
    pub fn close_gaps(&mut self, confirmed: bool) -> CommandOutcome {
        if !confirmed && self.grid.selected_elements().is_empty() {
            return CommandOutcome {
                alert: Some(Alert {
                    title: "Close element gaps".to_string(),
                    message: NO_SELECTION_WARNING.to_string(),
                    style: AlertStyle::Warning,
                    confirm: Some(AlertConfirm { label: "Close Gaps".to_string(), cancel_label: "Cancel".to_string(), action: AlertAction::CloseGapsEverywhere }),
                }),
                handled: true,
                ..CommandOutcome::default()
            };
        }
        let threshold = effect_ops::parse_threshold(&self.prefs.close_gap_threshold).map_or(0.1, |t| t.total_secs());
        self.grid.close_gaps(threshold, confirmed);
        CommandOutcome::handled()
    }

    /// `AlignEffectsToNearestMarks`: F295. With nothing selected it asks first, and then applies to every effect.
    pub fn align_to_marks(&mut self, kind: MarkAlign, confirmed_everywhere: bool) -> CommandOutcome {
        let selected = self.selected_times();
        if selected.is_empty() && !confirmed_everywhere {
            return CommandOutcome::alert(Alert {
                title: "Align effects to marks".to_string(),
                message: NO_SELECTION_WARNING.to_string(),
                style: AlertStyle::Warning,
                confirm: Some(AlertConfirm {
                    label: "Apply to Entire Sequence".to_string(),
                    cancel_label: "Cancel".to_string(),
                    action: AlertAction::AlignToMarksEverywhere(kind),
                }),
            });
        }
        let effects: Vec<(EffectId, TimeSpan, TimeSpan)> = if selected.is_empty() {
            self.session.lock().effects().map(|e| (e.id, e.node.start, e.node.time_span)).collect()
        } else {
            selected
        };
        let Some(threshold) = effect_ops::parse_threshold(&self.prefs.align_to_threshold) else {
            return CommandOutcome::handled();
        };
        let index = MarkIndex::new(self.session.lock().marks());
        let times: Vec<(TimeSpan, TimeSpan)> = effects.iter().map(|(_, s, d)| (*s, *d)).collect();
        let results = effect_ops::align_to_marks(kind, &times, threshold, &index);
        let moves: Vec<(EffectId, TimeSpan, TimeSpan)> = effects
            .iter()
            .zip(&results)
            .filter_map(|((id, ..), result)| result.map(|(start, end)| (*id, start, end)))
            .collect();
        if !moves.is_empty() {
            // Default move type, so the undo reads `Moved {n} effect{s}`.
            self.move_resize(&moves, MoveKind::Moved);
        }
        CommandOutcome::handled()
    }

    /// Runs what a prompt's default button stands for.
    pub fn run_alert_action(&mut self, action: &AlertAction) -> CommandOutcome {
        match action {
            AlertAction::AlignToMarksEverywhere(kind) => self.align_to_marks(*kind, true),
            AlertAction::RemoveAudio => {
                self.remove_audio();
                self.grid.sync_from_document();
                CommandOutcome::handled()
            }
            AlertAction::AddMarksEverywhere => self.add_marks_to_effects(true),
            AlertAction::CreateDefaultMarks => {
                self.create_default_marks();
                self.grid.sync_from_document();
                CommandOutcome::handled()
            }
            AlertAction::EnableDefaultMarks => {
                self.enable_default_marks();
                self.grid.sync_from_document();
                CommandOutcome::handled()
            }
            AlertAction::ReplaceSelected(type_id) => {
                self.replace_selected(*type_id);
                CommandOutcome::handled()
            }
            AlertAction::CloneToSelected(source) => {
                self.clone_to_selected(*source);
                CommandOutcome::handled()
            }
            AlertAction::CloseGapsEverywhere => self.close_gaps(true),
            AlertAction::AssociateAudio | AlertAction::ContinueDrop => CommandOutcome::handled(),
        }
    }

    // ------------------------------------------------------------------------------------------------------------------
    // Clipboard: F315 to F318
    // ------------------------------------------------------------------------------------------------------------------

    /// The pasteboard's count, cached by the window on `changeCount`. F316.
    pub fn set_clipboard_count(&mut self, count: usize) {
        self.clipboard_count = count;
    }

    /// `ClipboardAddData`. Cut removes each effect and adds one undo step; copy marks nothing. Nothing selected returns nothing.
    pub fn copy_or_cut(&mut self, cut: bool) -> Option<ClipboardData> {
        let selected = self.grid.selected_elements();
        if selected.is_empty() {
            return None;
        }
        let rows = self.grid.visible_rows();
        let session = self.session.lock();
        // Which selected effects each row shows, without a scan per row. Hazard 6.
        let mut by_node: HashMap<Uuid, Vec<CopyCandidate<EffectId>>> = HashMap::new();
        for id in &selected {
            if let Some(effect) = session.effect(*id) {
                if let Some((node, _)) = effect.node.targets.first() {
                    by_node.entry(*node).or_default().push(CopyCandidate { id: *id, start: effect.node.start, duration: effect.node.time_span });
                }
            }
        }
        let copy_rows: Vec<CopyRow<EffectId>> = rows
            .iter()
            .map(|r| CopyRow { name: r.name.clone(), selected: by_node.get(&r.node).cloned().unwrap_or_default() })
            .collect();
        let plan = clipboard::plan_copy(&copy_rows);
        let layers = &session.document().layers;
        let mut effects = Vec::with_capacity(plan.picks.len());
        for (pick, row_offset) in &plan.picks {
            let Some(entry) = session.effect(pick.id) else { continue };
            let Some(data) = clipboard::effect_data_text(&entry.node.type_id, &entry.node.instance_id, &entry.node.params) else { continue };
            let layer = layers.layers.get(layers.layer_index_for_effect(&entry.node.instance_id));
            effects.push(clipboard::ClipboardEffect {
                type_id: entry.node.type_id,
                start: pick.start,
                duration: pick.duration,
                layer_id: layer.map_or_else(Uuid::nil, |l| l.id),
                layer_name: layer.map_or_else(String::new, |l| l.name.clone()),
                layer_type_id: layer.and_then(|l| l.filter.as_ref()).map_or_else(Uuid::nil, |f| f.type_id()),
                data,
                row_offset: *row_offset,
            });
        }
        let data = ClipboardData { effects, first_visible_row: plan.first_visible_row, earliest_start: plan.earliest_start };
        drop(session);
        if cut {
            let ids: Vec<EffectId> = plan.picks.iter().map(|(p, _)| p.id).collect();
            let mut session = self.session.lock();
            let snapshots = session.remove_effects(&ids);
            session.record_undo(UndoAction::EffectsRemoved { kind: AddRemoveKind::Cut, effects: snapshots });
            drop(session);
            self.grid.effects_removed(&ids);
            self.grid.remove_from_selection(&ids);
        }
        Some(data)
    }

    /// `PasteEffects`: the anchor row, and time zero when that row is selected. No rows at all: nothing (GUARD).
    pub fn paste(&mut self, data: &ClipboardData, mode: PastingMode, now: Instant) -> CommandOutcome {
        let anchor_id = self.grid.selected_row().or_else(|| self.grid.active_row()).or_else(|| self.grid.top_visible_row());
        let Some(anchor_id) = anchor_id else { return CommandOutcome::handled() };
        let rows = self.grid.visible_rows();
        let anchor = rows.iter().position(|r| r.id == anchor_id).map_or(-1, |i| i as i32);
        let selected = self.grid.row_summary(anchor_id).is_some_and(|r| r.selected);
        let paste_time = if selected { TimeSpan::ZERO } else { self.grid.cursor() };
        self.paste_at(data, mode, paste_time, anchor, &rows, now)
    }

    /// `ClipboardPaste(pasteTime)` as the Lyric Converter calls it (F301): default mode, at the given time whether or not the anchor row is
    /// selected, and the number pasted comes back. A time of zero keeps the clipboard's own times, as any paste at zero does.
    pub fn paste_at_time(&mut self, data: &ClipboardData, paste_time: TimeSpan, now: Instant) -> (CommandOutcome, usize) {
        let rows = self.grid.visible_rows();
        let anchor_id = self.grid.selected_row().or_else(|| self.grid.active_row()).or_else(|| self.grid.top_visible_row());
        // With no row at all upstream's index is -1, and an offset can still land on a row.
        let anchor = anchor_id.and_then(|id| rows.iter().position(|r| r.id == id)).map_or(-1, |i| i as i32);
        self.paste_counted(data, PastingMode::Default, paste_time, anchor, &rows, now)
    }

    /// `ClipboardPaste`.
    pub fn paste_at(&mut self, data: &ClipboardData, mode: PastingMode, paste_time: TimeSpan, anchor: i32, rows: &[RowSummary], now: Instant) -> CommandOutcome {
        self.paste_counted(data, mode, paste_time, anchor, rows, now).0
    }

    fn paste_counted(&mut self, data: &ClipboardData, mode: PastingMode, paste_time: TimeSpan, anchor: i32, rows: &[RowSummary], now: Instant) -> (CommandOutcome, usize) {
        let paste_rows: Vec<PasteRow> = rows.iter().map(|r| PasteRow { name: r.name.clone(), deprecated: r.deprecated }).collect();
        let mut session = self.session.lock();
        let plan = clipboard::plan_paste(data, mode, paste_time, anchor, &paste_rows, session.length(), session.marks());
        let mut added = Vec::with_capacity(plan.items.len());
        let mut incomplete = false;
        for item in &plan.items {
            let record = &data.effects[item.record];
            let Some(params) = clipboard::effect_data_from_text(&record.type_id, &record.data) else {
                incomplete = true;
                continue;
            };
            let row = &rows[item.row];
            let node = vixen_io::EffectNode {
                start: item.time,
                time_span: item.duration,
                type_id: record.type_id,
                instance_id: Uuid::new_v4(),
                targets: vec![(row.node, row.name.clone())],
                params,
            };
            let Some(id) = session.add_copy(node, row.node, &row.name, item.time, item.duration) else { continue };
            // Same id if this sequence has it, else the same name and type, else the default.
            let layers = &session.document().layers;
            let layer = if layers.layer(&record.layer_id).is_some() {
                Some(record.layer_id)
            } else {
                layers
                    .layers
                    .iter()
                    .find(|l| l.name == record.layer_name && l.filter.as_ref().map_or_else(Uuid::nil, |f| f.type_id()) == record.layer_type_id)
                    .map(|l| l.id)
            };
            if let Some(layer) = layer {
                session.set_layer(&[id], layer);
            }
            added.push(id);
        }
        // One undo step, even when nothing was pasted (KEEP F318).
        session.record_undo(UndoAction::EffectsAdded { kind: AddRemoveKind::Pasted, ids: added.clone() });
        session.set_modified();
        session.render_all_dirty();
        drop(session);
        self.grid.effects_added(&added);
        self.grid.set_selection(&added);
        if !plan.skipped_deprecated.is_empty() {
            let text = format!("Skipped pasting to Deprecated row(s): {}.", plan.skipped_deprecated.join(", "));
            self.set_status_message(&text, 10, now);
        }
        let count = added.len();
        if incomplete {
            let alert = Alert {
                title: "Error".to_string(),
                message: "Effect(s) on clipboard are incomplete.  Please close Vixen and restart it.".to_string(),
                style: AlertStyle::Error,
                confirm: None,
            };
            return (CommandOutcome::alert(alert), count);
        }
        (CommandOutcome::handled(), count)
    }

    // ------------------------------------------------------------------------------------------------------------------
    // Undo: section M
    // ------------------------------------------------------------------------------------------------------------------

    pub fn undo(&mut self, steps: usize) -> usize {
        let done = self.session.lock().undo(steps);
        self.after_undo_or_redo();
        done
    }

    pub fn redo(&mut self, steps: usize) -> usize {
        let done = self.session.lock().redo(steps);
        self.after_undo_or_redo();
        done
    }

    /// Effects an undo removed can't stay selected.
    fn after_undo_or_redo(&mut self) {
        let session = self.session.lock();
        let gone: Vec<EffectId> = self.grid.selected_elements().into_iter().filter(|id| session.effect(*id).is_none()).collect();
        drop(session);
        if !gone.is_empty() {
            self.grid.remove_from_selection(&gone);
        }
        self.grid.effects_reloaded();
    }

    // ------------------------------------------------------------------------------------------------------------------
    // Modes: F118 to F123, F150, F320
    // ------------------------------------------------------------------------------------------------------------------

    pub fn set_draw_mode(&mut self) {
        self.prefs.draw_mode_selected = true;
        self.prefs.selection_mode_selected = false;
        self.prefs_changed();
    }

    pub fn set_selection_mode(&mut self) {
        self.prefs.draw_mode_selected = false;
        self.prefs.selection_mode_selected = true;
        self.selected_effect = None;
        self.grid.set_selected_effect(None);
        self.prefs_changed();
    }

    /// The Effects panel's leaf click: F320. Already chosen clears it. It doesn't switch Draw Mode on.
    pub fn toggle_selected_effect(&mut self, type_id: Uuid) -> Option<Uuid> {
        self.selected_effect = if self.selected_effect == Some(type_id) { None } else { Some(type_id) };
        self.grid.set_selected_effect(self.selected_effect);
        self.selected_effect
    }

    pub fn clear_selected_effect(&mut self) {
        self.selected_effect = None;
        self.grid.set_selected_effect(None);
    }

    pub fn set_drag_box_filter(&mut self, on: bool) {
        self.drag_box_filter = on;
        self.grid.set_drag_box_filter(on, &self.drag_box_filter_types);
    }

    /// F123: each check adds or removes the type and turns the filter on.
    pub fn toggle_drag_box_type(&mut self, type_id: Uuid) {
        if let Some(at) = self.drag_box_filter_types.iter().position(|t| *t == type_id) {
            self.drag_box_filter_types.remove(at);
        } else {
            self.drag_box_filter_types.push(type_id);
        }
        self.drag_box_filter = true;
        self.grid.set_drag_box_filter(true, &self.drag_box_filter_types);
    }

    /// F123's Invert Selection: flips every effect's check (KEEP: and its own, which the window shows from this flag).
    pub fn invert_drag_box_types(&mut self) {
        let all: Vec<Uuid> = vixen_effect_registry::all().iter().map(|d| d.type_id).collect();
        self.drag_box_filter_types = all.into_iter().filter(|t| !self.drag_box_filter_types.contains(t)).collect();
        self.drag_box_invert_checked = !self.drag_box_invert_checked;
        self.drag_box_filter = true;
        self.grid.set_drag_box_filter(true, &self.drag_box_filter_types);
    }

    pub fn drag_box_invert_checked(&self) -> bool {
        self.drag_box_invert_checked
    }

    // ------------------------------------------------------------------------------------------------------------------
    // Toolbars and panels
    // ------------------------------------------------------------------------------------------------------------------

    /// `resetToolStripMenuItem_Click`: every item in the strip shown. F104.
    pub fn reset_strip(&mut self, strip: StripId) {
        self.strip_states.reset_strip(strip.name());
    }

    pub fn toggle_panel(&mut self, panel: PanelId) {
        self.panels.toggle(panel);
    }

    pub fn select_panel(&mut self, side: DockSide, panel: PanelId) {
        self.panels.set_selected(side, Some(panel));
    }

    // ------------------------------------------------------------------------------------------------------------------
    // What the window shows
    // ------------------------------------------------------------------------------------------------------------------

    /// F261: percent of what's queued. `render_baseline` resets each time everything's caught up.
    fn render_progress(&mut self, session: &SequenceSession) -> (bool, i32) {
        let queued = session.counters.queued.load(Ordering::Relaxed);
        let finished = session.counters.finished.load(Ordering::Relaxed);
        if finished >= queued {
            self.render_baseline = queued;
            return (false, 100);
        }
        let total = queued - self.render_baseline;
        let done = finished.saturating_sub(self.render_baseline);
        let percent = if total == 0 { 100 } else { (done * 100 / total) as i32 };
        (percent < 100, percent)
    }

    pub fn ui_state(&mut self) -> UiState {
        let session = Arc::clone(&self.session);
        let session = session.lock();
        let (rendering_visible, rendering_percent) = self.render_progress(&session);
        self.last_render_report = (rendering_visible, rendering_percent);
        let profile = profile_name(&self.core);
        let name = session.path().map(|_| session.name());
        let modified = session.is_modified();
        let engine = self.engine_state();
        let selected = self.grid.selected_elements();
        let marks_present = clipboard::marks_present_after(self.grid.cursor(), session.marks());
        let stack = session.undo_stack();
        UiState {
            title: lifecycle::window_title(name.as_deref(), modified, &profile),
            sequence_name: session.name(),
            profile_name: profile,
            file_path: session.path().map(|p| p.to_string_lossy().into_owned()),
            modified,
            transport: self.transport.transport_ui(engine),
            speed: self.transport.speed_controls(),
            has_selection: !selected.is_empty(),
            selection_count: selected.len() as u32,
            paste: clipboard::paste_menu_state(self.clipboard_count, marks_present),
            undo_name: stack.undo_descriptions().into_iter().next(),
            redo_name: stack.redo_descriptions().into_iter().next(),
            audio: audio_ui_for(session.document(), &self.core.paths.media_dir()),
            effect_count: session.effect_count() as u32,
            prefs: self.prefs.clone(),
            draw_mode: self.prefs.draw_mode_selected,
            selection_mode: self.prefs.selection_mode_selected,
            show_hidden_rows: self.show_hidden_rows,
            drag_box_filter: self.drag_box_filter,
            drag_box_filter_types: self.drag_box_filter_types.clone(),
            selected_effect: self.selected_effect,
            position_text: self.position_text.clone(),
            length_text: format_position(session.length()),
            status_message: self.status.text.clone(),
            rendering_visible,
            rendering_percent,
        }
    }

    pub fn position_text(&self) -> &str {
        &self.position_text
    }

    /// Everything a test script checks, as JSON, so a harness run confirms what Rust holds and not just what's on screen.
    pub fn debug_state_json(&mut self) -> String {
        let ui = self.ui_state();
        let range = self.grid.playback_range();
        let selection: Vec<u64> = self.grid.selected_elements().iter().map(|e| e.0).collect();
        let (undo, redo) = {
            let session = self.session.lock();
            (session.undo_stack().undo_descriptions(), session.undo_stack().redo_descriptions())
        };
        let mut map = serde_json::Map::new();
        let mut put = |key: &str, value: serde_json::Value| {
            map.insert(key.to_string(), value);
        };
        put("title", serde_json::json!(ui.title));
        put("modified", serde_json::json!(ui.modified));
        put("filePath", serde_json::json!(ui.file_path));
        put("effectCount", serde_json::json!(ui.effect_count));
        put("selection", serde_json::json!(selection));
        put("undo", serde_json::json!(undo));
        put("redo", serde_json::json!(redo));
        put("positionText", serde_json::json!(ui.position_text));
        put("lengthText", serde_json::json!(ui.length_text));
        put("statusMessage", serde_json::json!(ui.status_message));
        put("running", serde_json::json!(ui.transport.running));
        put("paused", serde_json::json!(ui.transport.paused));
        put("looping", serde_json::json!(ui.transport.looping));
        put("playIcon", serde_json::json!(format!("{:?}", ui.transport.icon)));
        put("playTooltip", serde_json::json!(ui.transport.play_tooltip));
        put("stopEnabled", serde_json::json!(ui.transport.stop_enabled));
        put("delay", serde_json::json!(format!("{:?}", ui.transport.delay)));
        put("delayVisible", serde_json::json!(ui.transport.delay_visible));
        put("delayText", serde_json::json!(ui.transport.delay_text));
        put("speed", serde_json::json!(ui.speed.speed));
        put("speedDisplay", serde_json::json!(ui.speed.display_speed));
        put("speedGaugeEnabled", serde_json::json!(ui.speed.gauge_enabled));
        put("drawMode", serde_json::json!(ui.draw_mode));
        put("selectionMode", serde_json::json!(ui.selection_mode));
        put("snapTo", serde_json::json!(ui.prefs.snap_to_selected));
        put("snapStrength", serde_json::json!(ui.prefs.snap_strength));
        put("closeGapThreshold", serde_json::json!(ui.prefs.close_gap_threshold));
        put("alignToThreshold", serde_json::json!(ui.prefs.align_to_threshold));
        put("autoSave", serde_json::json!(ui.prefs.auto_save_enabled));
        put("showEffectInfo", serde_json::json!(ui.prefs.show_effect_info));
        put("showHiddenRows", serde_json::json!(ui.show_hidden_rows));
        put("cursorMs", serde_json::json!(self.grid.cursor().total_ms()));
        put("timePerPixelTicks", serde_json::json!(self.grid.time_per_pixel().0));
        put("visibleStartMs", serde_json::json!(self.grid.visible_start().total_ms()));
        put("rangeStartMs", serde_json::json!(range.start.map(TimeSpan::total_ms)));
        put("rangeEndMs", serde_json::json!(range.end.map(TimeSpan::total_ms)));
        put("rowsVisible", serde_json::json!(self.grid.visible_rows().len()));
        put("panelsLeft", serde_json::json!(self.panels.tabs(DockSide::Left).iter().map(|p| p.title()).collect::<Vec<_>>()));
        put("panelsRight", serde_json::json!(self.panels.tabs(DockSide::Right).iter().map(|p| p.title()).collect::<Vec<_>>()));
        put("stripRows", serde_json::json!(self.toolbars.rows.iter().map(|r| r.iter().map(|s| s.title()).collect::<Vec<_>>()).collect::<Vec<_>>()));
        put("stripsVisible", serde_json::json!(StripId::ALL.iter().filter(|s| self.toolbars.is_visible(**s)).map(|s| s.title()).collect::<Vec<_>>()));
        put("removeAudioEnabled", serde_json::json!(ui.audio.remove_enabled));
        put("timelineTitle", serde_json::json!(ui.audio.timeline_title));
        put("audioTooltip", serde_json::json!(ui.audio.button_tooltip));
        {
            let session = self.session.lock();
            put("lengthMs", serde_json::json!(session.length().total_ms()));
            put("media", serde_json::json!(session.document().media.iter().map(|m| m.file_name.clone()).collect::<Vec<_>>()));
            put("layers", serde_json::json!(session.document().layers.layers.iter().map(|l| l.name.clone()).collect::<Vec<_>>()));
            put("markCollections", serde_json::json!(session.marks().iter().map(|c| format!("{} ({})", c.name, c.marks.len())).collect::<Vec<_>>()));
            let starts: Vec<f64> = self.grid.selected_elements().iter().filter_map(|id| session.effect(*id)).map(|e| e.node.start.total_ms()).collect();
            let durations: Vec<f64> = self.grid.selected_elements().iter().filter_map(|id| session.effect(*id)).map(|e| e.node.time_span.total_ms()).collect();
            put("firstMarksMs", serde_json::json!(session.marks().iter().map(|c| c.marks.iter().map(|m| m.start_time.total_ms()).fold(f64::NAN, f64::min)).map(|v| if v.is_nan() { None } else { Some(v) }).collect::<Vec<_>>()));
            put("firstEffectStartMs", serde_json::json!(session.effects().map(|e| e.node.start).min().map(TimeSpan::total_ms)));
            put("selectedStartsMs", serde_json::json!(starts));
            put("selectedDurationsMs", serde_json::json!(durations));
        }
        put("colorCollections", serde_json::json!(self.color_collections.iter().map(|c| c.name.clone()).collect::<Vec<_>>()));
        serde_json::Value::Object(map).to_string()
    }

    pub fn marks(&self) -> Vec<MarkCollection> {
        self.session.lock().marks().to_vec()
    }
}

/// `grid.alignmentHelperWarning`. Upstream's text says 4, and the real limit is 32.
pub const ALIGNMENT_HELPER_WARNING: &str =
    "Too many effects selected on the same row for this action.\nMax selected effects per row for this action is 4";

/// F295's and F296's warning.
pub const NO_SELECTION_WARNING: &str =
    "No effects have been selected and action will be applied to your entire sequence. This can take a considerable length of time, are you sure ?";

/// `VixenSystem.ProfileName`, or the folder's name when the profile isn't in `Settings.xml`.
pub fn profile_name(core: &AppCore) -> String {
    core.profile_name.clone().unwrap_or_else(|| {
        core.paths.root().file_name().map_or_else(|| "Vixen".to_string(), |n| n.to_string_lossy().into_owned())
    })
}

/// `GetTiming` with the selected provider: F272, F281. The default timing source takes no speed, so only a Media source counts.
pub fn timing_source_exists(document: &TimedSequence, media_dir: &Path) -> bool {
    let SelectedTimingProvider { provider_type, source_name } = &document.selected_timing_provider;
    let (Some(provider), Some(source)) = (provider_type, source_name) else { return false };
    if provider != "Media" || source.trim().is_empty() {
        return false;
    }
    document.media.iter().any(|m| &m.file_name == source && media_dir.join(&m.file_name).exists())
}

fn audio_ui_for(document: &TimedSequence, media_dir: &Path) -> AudioUi {
    let mut ui = AudioUi { button_tooltip: "Associate Audio".to_string(), timeline_title: "Timeline".to_string(), ..AudioUi::default() };
    let Some(media) = document.media.first() else { return ui };
    ui.remove_enabled = true;
    ui.beat_bar_enabled = true;
    if media_dir.join(&media.file_name).exists() {
        ui.speed_tempo_enabled = true;
        ui.button_tooltip = format!("Associated Audio: {}", media.file_name);
        ui.timeline_title = format!("Timeline - {}", media.file_name);
    }
    ui
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sequence::SessionEnvironment;
    use crate::sequencer::transport::DelayOption;
    use crate::undo::UndoAction;
    use vixen_core::{ElementTree, Mark};
    use vixen_io::ModuleStore;

    const SET_LEVEL: Uuid = uuid::uuid!("32cff8e0-5b10-4466-a093-0d232c55aac0");

    struct Fixture {
        state: SequencerState,
        nodes: Vec<(Uuid, String)>,
        _dir: PathBuf,
    }

    fn fixture(name: &str, path: Option<&str>) -> Fixture {
        let root = std::env::temp_dir().join(format!("vixen-seq-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let core = Arc::new(AppCore::open(root.clone(), true).unwrap());
        let mut tree = ElementTree::new();
        let mut nodes = Vec::new();
        for n in ["A", "B", "C"] {
            let id = tree.add_element(n);
            nodes.push((tree.node(id).id, n.to_string()));
        }
        let env = SessionEnvironment { video_cache: None,
            tree: Arc::new(tree),
            store: Arc::new(ModuleStore::new()),
            module_data_root: None,
            frame_ms: 25,
            media_dir: core.paths.media_dir(),
        };
        let session = match path {
            None => SequenceSession::new_untitled(&env),
            Some(file) => {
                let target = core.paths.sequence_dir().join(file);
                let mut s = SequenceSession::new_untitled(&env);
                s.save(Some(&target)).unwrap();
                SequenceSession::open(&target, &env).unwrap()
            }
        };
        let session = Arc::new(Mutex::new(session));
        let settings = Arc::new(ProfileSettings::open(root.join("Settings.xml")));
        let mut state = SequencerState::with_settings(core, session, settings, root.clone());
        state.finish_load();
        Fixture { state, nodes, _dir: root }
    }

    fn secs(s: i64) -> TimeSpan {
        TimeSpan::from_ms_i(s * 1000)
    }

    #[test]
    fn positions_format_like_the_status_bar() {
        assert_eq!(format_position(TimeSpan::ZERO), "0:00.000");
        assert_eq!(format_position(TimeSpan::from_ms_i(223_440)), "3:43.440");
        // No hours: past an hour only the minutes component shows (KEEP).
        assert_eq!(format_position(TimeSpan::from_ms_i(3_661_005)), "1:01.005");
    }

    #[test]
    fn raw_time_text_has_no_fraction_at_zero() {
        assert_eq!(format_timespan_raw(TimeSpan::ZERO), "00:00:00");
        assert_eq!(format_timespan_raw(TimeSpan::from_ms_i(1500)), "00:00:01.5000000");
        assert_eq!(format_timespan_raw(secs(3725)), "01:02:05");
    }

    #[test]
    fn new_sequence_starts_modified_and_saved_one_does_not() {
        let mut f = fixture("modified", None);
        assert!(f.state.ui_state().modified);
        assert_eq!(f.state.ui_state().title, format!("Untitled * - {}", profile_name(&f.state.core)));
        let g = fixture("clean", Some("Song.tim"));
        assert!(!g.state.session.lock().is_modified());
    }

    #[test]
    fn length_zero_becomes_one_minute() {
        let mut f = fixture("length", None);
        assert_eq!(f.state.ui_state().length_text, "1:00.000");
    }

    #[test]
    fn save_then_edit_then_close_prompts() {
        let mut f = fixture("close", None);
        assert_eq!(f.state.close_action(false), CloseAction::Prompt { can_cancel: true });
        assert_eq!(f.state.close_action(true), CloseAction::Prompt { can_cancel: false });
        let target = f.state.core.paths.sequence_dir().join("Named.txt");
        let prepared = f.state.prepare_save(Some(&target)).unwrap();
        assert_eq!(prepared.target().file_name().unwrap(), "Named.txt.tim");
        prepared.write().unwrap();
        f.state.finish_save(prepared.target().to_path_buf(), true);
        assert!(!f.state.session.lock().is_modified());
        assert_eq!(f.state.close_action(false), CloseAction::Close);
        // Zoom is saved silently at close.
        f.state.zoom_time(0.8);
        assert_eq!(f.state.close_action(false), CloseAction::SaveSilently);
        assert!(!f.state.session.lock().is_modified());
    }

    #[test]
    fn poll_follows_renders_catching_up() {
        let mut f = fixture("render-poll", None);
        let now = Instant::now();
        f.state.poll(now);
        assert!(!f.state.poll(now).changed);
        let counters = Arc::clone(&f.state.session.lock().counters);
        counters.queued.fetch_add(4, Ordering::Relaxed);
        assert!(f.state.poll(now).changed);
        assert!(f.state.ui_state().rendering_visible);
        counters.finished.fetch_add(2, Ordering::Relaxed);
        assert!(f.state.poll(now).changed);
        assert_eq!(f.state.ui_state().rendering_percent, 50);
        assert!(!f.state.poll(now).changed);
        counters.finished.fetch_add(2, Ordering::Relaxed);
        assert!(f.state.poll(now).changed);
        assert!(!f.state.ui_state().rendering_visible);
    }

    #[test]
    fn a_failed_write_stays_modified() {
        let mut f = fixture("failed", None);
        let target = f.state.core.paths.sequence_dir().join("X.tim");
        let prepared = f.state.prepare_save(Some(&target)).unwrap();
        assert!(!f.state.session.lock().is_modified());
        f.state.finish_save(prepared.target().to_path_buf(), false);
        assert!(f.state.session.lock().is_modified());
    }

    #[test]
    fn save_route_uses_the_panel_for_a_new_sequence() {
        let f = fixture("route", None);
        match f.state.save_route(false) {
            SaveRoute::Panel { directory, suggested_name } => {
                assert_eq!(directory, f.state.core.paths.sequence_dir());
                assert_eq!(suggested_name, "Untitled.tim");
            }
            other => panic!("{other:?}"),
        }
        let g = fixture("route2", Some("Song.tim"));
        assert!(matches!(g.state.save_route(false), SaveRoute::InPlace(_)));
        assert!(matches!(g.state.save_route(true), SaveRoute::Panel { .. }));
    }

    #[test]
    fn auto_save_comes_due_once_after_an_edit() {
        let mut f = fixture("autosave", Some("Song.tim"));
        let now = Instant::now();
        assert!(!f.state.poll(now).auto_save_due);
        f.state.session.lock().set_modified();
        // The transition arms it from the real clock inside `poll`.
        assert!(f.state.poll(now).changed);
        assert!(!f.state.poll(now + Duration::from_secs(10)).auto_save_due);
        assert!(f.state.poll(now + Duration::from_secs(301)).auto_save_due);
        assert!(!f.state.poll(now + Duration::from_secs(302)).auto_save_due);
    }

    #[test]
    fn status_message_clears_after_its_timeout() {
        let mut f = fixture("status", None);
        let now = Instant::now();
        f.state.set_status_message("hello", 8, now);
        assert_eq!(f.state.ui_state().status_message, "hello");
        assert!(!f.state.poll(now + Duration::from_secs(7)).changed || f.state.ui_state().status_message == "hello");
        assert!(f.state.poll(now + Duration::from_secs(9)).changed);
        assert_eq!(f.state.ui_state().status_message, "");
    }

    #[test]
    fn stale_and_load_rules_drop_and_shorten_effects() {
        let root = std::env::temp_dir().join(format!("vixen-seq-rules-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let core = Arc::new(AppCore::open(root.clone(), true).unwrap());
        let mut tree = ElementTree::new();
        let a = tree.add_element("A");
        let a = (tree.node(a).id, "A".to_string());
        let env = SessionEnvironment { video_cache: None,
            tree: Arc::new(tree),
            store: Arc::new(ModuleStore::new()),
            module_data_root: None,
            frame_ms: 25,
            media_dir: core.paths.media_dir(),
        };
        let mut session = SequenceSession::new_untitled(&env);
        session.set_length(secs(10));
        let keep = session.add_new_effect(SET_LEVEL, a.0, &a.1, secs(1), secs(1)).unwrap();
        let past = session.add_new_effect(SET_LEVEL, a.0, &a.1, secs(20), secs(1)).unwrap();
        let over = session.add_new_effect(SET_LEVEL, a.0, &a.1, secs(9), secs(5)).unwrap();
        let session = Arc::new(Mutex::new(session));
        let settings = Arc::new(ProfileSettings::open(root.join("Settings.xml")));
        let mut state = SequencerState::with_settings(core, Arc::clone(&session), settings, root);
        state.finish_load();
        let s = session.lock();
        assert!(s.effect(keep).is_some());
        assert!(s.effect(past).is_none());
        assert_eq!(s.effect(over).unwrap().node.time_span, secs(1));
        assert_eq!(state.load_report().dropped_past_end, 1);
        assert_eq!(state.load_report().shortened, 1);
    }

    #[test]
    fn duplicate_mark_collections_rename_and_mark_modified() {
        let f = fixture("rename", Some("Song.tim"));
        let mut state = f.state;
        let mut marks = vec![MarkCollection::default(), MarkCollection::default()];
        marks[0].name = "Beats".into();
        marks[1].name = "Beats".into();
        {
            let mut s = state.session.lock();
            s.set_marks(marks);
            s.set_unmodified();
        }
        state.load_finished = false;
        state.finish_load();
        assert_eq!(state.load_report().renames, vec![("Beats".to_string(), "Beats - 2".to_string())]);
        assert!(state.session.lock().is_modified());
    }

    #[test]
    fn add_effect_uses_the_active_row_and_undoes() {
        let mut f = fixture("add", None);
        // No row chosen: nothing.
        assert!(f.state.add_effect_from_menu(SET_LEVEL).is_none());
        f.state.grid.move_active_row(1);
        f.state.set_cursor(secs(5));
        let id = f.state.add_effect_from_menu(SET_LEVEL).unwrap();
        {
            let s = f.state.session.lock();
            let e = s.effect(id).unwrap();
            assert_eq!(e.node.start, secs(5));
            assert_eq!(e.node.time_span, secs(2));
            assert_eq!(e.node.targets[0].0, f.nodes[0].0);
        }
        assert_eq!(f.state.grid.selected_elements(), vec![id]);
        let ui = f.state.ui_state();
        assert_eq!(ui.undo_name.as_deref(), Some("Added 1 effect"));
        assert!(ui.has_selection);
        assert_eq!(f.state.undo(1), 1);
        assert!(f.state.session.lock().effect(id).is_none());
        assert!(f.state.grid.selected_elements().is_empty());
        assert_eq!(f.state.ui_state().redo_name.as_deref(), Some("Added 1 effect"));
        assert_eq!(f.state.redo(1), 1);
        assert!(f.state.session.lock().effect(id).is_some());
    }

    #[test]
    fn delete_selected_records_one_undo() {
        let mut f = fixture("delete", None);
        f.state.grid.move_active_row(1);
        let a = f.state.add_effect_from_menu(SET_LEVEL).unwrap();
        f.state.set_cursor(secs(9));
        let b = f.state.add_effect_from_menu(SET_LEVEL).unwrap();
        f.state.grid.set_selection(&[a, b]);
        f.state.delete_selected();
        let ui = f.state.ui_state();
        assert_eq!(ui.effect_count, 0);
        assert_eq!(ui.undo_name.as_deref(), Some("Removed 2 effects"));
        assert!(!ui.has_selection);
        f.state.undo(1);
        assert_eq!(f.state.ui_state().effect_count, 2);
    }

    #[test]
    fn cut_copy_and_paste_round_trip_through_json() {
        let mut f = fixture("clip", None);
        f.state.grid.move_active_row(1);
        f.state.set_cursor(secs(10));
        let a = f.state.add_effect_from_menu(SET_LEVEL).unwrap();
        f.state.grid.move_active_row(1);
        f.state.set_cursor(secs(12));
        let b = f.state.add_effect_from_menu(SET_LEVEL).unwrap();
        f.state.grid.set_selection(&[a, b]);

        // Copy marks nothing and leaves the effects.
        let before = f.state.ui_state().undo_name;
        let data = f.state.copy_or_cut(false).unwrap();
        assert_eq!(data.effects.len(), 2);
        assert_eq!(data.earliest_start, secs(10));
        assert_eq!(data.effects[1].row_offset, 1);
        assert_eq!(f.state.ui_state().undo_name, before);
        let json = data.to_json();

        // Cut removes them with one step.
        let cut = f.state.copy_or_cut(true).unwrap();
        assert_eq!(cut.effects.len(), 2);
        let ui = f.state.ui_state();
        assert_eq!(ui.effect_count, 0);
        assert_eq!(ui.undo_name.as_deref(), Some("Cut 2 effects"));

        // Paste at the cursor on the active row, from the JSON.
        let parsed = ClipboardData::from_json(&json).unwrap();
        f.state.grid.set_selection(&[]);
        f.state.set_cursor(secs(30));
        let outcome = f.state.paste(&parsed, PastingMode::Default, Instant::now());
        assert!(outcome.alert.is_none());
        let ui = f.state.ui_state();
        assert_eq!(ui.effect_count, 2);
        assert_eq!(ui.undo_name.as_deref(), Some("Paste 2 effects"));
        let s = f.state.session.lock();
        let mut starts: Vec<TimeSpan> = s.effects().map(|e| e.node.start).collect();
        starts.sort();
        assert_eq!(starts, [secs(30), secs(32)]);
    }

    #[test]
    fn paste_onto_a_selected_row_keeps_original_times() {
        let mut f = fixture("cliprow", None);
        f.state.grid.move_active_row(1);
        f.state.set_cursor(secs(10));
        let a = f.state.add_effect_from_menu(SET_LEVEL).unwrap();
        f.state.grid.set_selection(&[a]);
        let data = f.state.copy_or_cut(false).unwrap();
        f.state.set_selected_row_for_tests(Some(1));
        f.state.set_cursor(secs(40));
        f.state.paste(&data, PastingMode::Default, Instant::now());
        let s = f.state.session.lock();
        let on_b: Vec<&_> = s.effects().filter(|e| e.node.targets[0].0 == f.nodes[1].0).collect();
        assert_eq!(on_b.len(), 1);
        assert_eq!(on_b[0].node.start, secs(10));
    }

    #[test]
    fn paste_at_a_time_ignores_the_selected_row_and_counts() {
        let mut f = fixture("clipat", None);
        f.state.grid.move_active_row(1);
        f.state.set_cursor(secs(10));
        let a = f.state.add_effect_from_menu(SET_LEVEL).unwrap();
        f.state.set_cursor(secs(12));
        let b = f.state.add_effect_from_menu(SET_LEVEL).unwrap();
        f.state.grid.set_selection(&[a, b]);
        let data = f.state.copy_or_cut(false).unwrap();
        // A selected row sends an ordinary paste to the clipboard's own times; the converter's paste goes where it says.
        f.state.set_selected_row_for_tests(Some(1));
        f.state.set_cursor(secs(40));
        let (outcome, pasted) = f.state.paste_at_time(&data, secs(20), Instant::now());
        assert!(outcome.alert.is_none());
        assert_eq!(pasted, 2);
        assert_eq!(f.state.ui_state().undo_name.as_deref(), Some("Paste 2 effects"));
        {
            let s = f.state.session.lock();
            let mut starts: Vec<TimeSpan> = s.effects().map(|e| e.node.start).collect();
            starts.sort();
            assert_eq!(starts, [secs(10), secs(12), secs(20), secs(22)]);
        }
        // Time zero keeps the times that were copied, which is where upstream's `offset` goes to zero too.
        let (_, pasted) = f.state.paste_at_time(&data, TimeSpan::ZERO, Instant::now());
        assert_eq!(pasted, 2);
        let s = f.state.session.lock();
        assert_eq!(s.effects().filter(|e| e.node.start == secs(10)).count(), 2);
    }

    #[test]
    fn paste_at_a_time_past_the_end_pastes_nothing() {
        let mut f = fixture("clipatend", None);
        f.state.grid.move_active_row(1);
        f.state.set_cursor(secs(10));
        let a = f.state.add_effect_from_menu(SET_LEVEL).unwrap();
        f.state.grid.set_selection(&[a]);
        let data = f.state.copy_or_cut(false).unwrap();
        let length = f.state.session.lock().length();
        let (_, pasted) = f.state.paste_at_time(&data, length + secs(5), Instant::now());
        assert_eq!(pasted, 0);
    }

    #[test]
    fn nothing_selected_copies_nothing() {
        let mut f = fixture("clipnone", None);
        assert!(f.state.copy_or_cut(false).is_none());
    }

    #[test]
    fn paste_with_no_rows_does_nothing() {
        let root = std::env::temp_dir().join(format!("vixen-seq-norows-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let core = Arc::new(AppCore::open(root.clone(), true).unwrap());
        let env = SessionEnvironment { video_cache: None,
            tree: Arc::new(ElementTree::new()),
            store: Arc::new(ModuleStore::new()),
            module_data_root: None,
            frame_ms: 25,
            media_dir: core.paths.media_dir(),
        };
        let session = Arc::new(Mutex::new(SequenceSession::new_untitled(&env)));
        let settings = Arc::new(ProfileSettings::open(root.join("Settings.xml")));
        let mut state = SequencerState::with_settings(core, session, settings, root);
        state.finish_load();
        let data = ClipboardData { effects: Vec::new(), first_visible_row: -1, earliest_start: TimeSpan::MAX };
        assert!(state.paste(&data, PastingMode::Default, Instant::now()).alert.is_none());
        assert_eq!(state.ui_state().undo_name, None);
    }

    #[test]
    fn align_to_marks_asks_first_with_nothing_selected() {
        let mut f = fixture("marks", None);
        f.state.grid.move_active_row(1);
        f.state.set_cursor(secs(10));
        let id = f.state.add_effect_from_menu(SET_LEVEL).unwrap();
        f.state.grid.set_selection(&[]);
        let mut collection = MarkCollection::default();
        collection.show_grid_lines = true;
        collection.marks = vec![Mark::new(TimeSpan::from_ms_i(10_200))];
        f.state.session.lock().set_marks(vec![collection]);

        let outcome = f.state.align_to_marks(MarkAlign::Start, false);
        let alert = outcome.alert.unwrap();
        assert_eq!(alert.title, "Align effects to marks");
        let action = alert.confirm.unwrap().action;
        // Answered yes.
        f.state.run_alert_action(&action);
        {
            let s = f.state.session.lock();
            assert_eq!(s.effect(id).unwrap().node.start, TimeSpan::from_ms_i(10_200));
        }
        assert_eq!(f.state.ui_state().undo_name.as_deref(), Some("Moved 1 effect"));
    }

    #[test]
    fn distribute_equally_moves_the_selection_and_says_so_in_undo() {
        let mut f = fixture("dist", None);
        f.state.grid.move_active_row(1);
        f.state.set_cursor(secs(0));
        let a = f.state.add_effect_from_menu(SET_LEVEL).unwrap();
        f.state.set_cursor(secs(6));
        let b = f.state.add_effect_from_menu(SET_LEVEL).unwrap();
        f.state.grid.set_selection(&[a, b]);
        f.state.run_hotkey(HotkeyCommand::DistributeEqually, Instant::now());
        let s = f.state.session.lock();
        assert_eq!((s.effect(a).unwrap().node.start, s.effect(a).unwrap().node.time_span), (secs(0), secs(4)));
        assert_eq!((s.effect(b).unwrap().node.start, s.effect(b).unwrap().node.time_span), (secs(4), secs(4)));
        drop(s);
        assert_eq!(f.state.ui_state().undo_name.as_deref(), Some("Distributed 2 effects"));
        // One selected is allowed, stays put, and still pushes an entry.
        f.state.grid.set_selection(&[a]);
        f.state.run_hotkey(HotkeyCommand::DistributeEqually, Instant::now());
        assert_eq!(f.state.ui_state().undo_name.as_deref(), Some("Distributed 1 effect"));
    }

    #[test]
    fn distribute_dialog_needs_two() {
        let mut f = fixture("dist2", None);
        f.state.grid.move_active_row(1);
        let a = f.state.add_effect_from_menu(SET_LEVEL).unwrap();
        f.state.grid.set_selection(&[a]);
        let outcome = f.state.distribute_dialog_request();
        assert_eq!(outcome.alert.unwrap().message, "Select at least two effects to distribute.");
        f.state.set_cursor(secs(6));
        let b = f.state.add_effect_from_menu(SET_LEVEL).unwrap();
        f.state.grid.set_selection(&[a, b]);
        match f.state.distribute_dialog_request().request {
            Some(UiRequest::DistributeDialog { count, start, end, .. }) => {
                assert_eq!((count, start, end), (2, secs(0), secs(8)));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn transport_runs_the_real_engine_muted_and_restores_the_range() {
        let mut f = fixture("transport", None);
        let start = Instant::now();
        f.state.grid.set_playback_range(PlaybackRange { start: Some(TimeSpan::ZERO), end: Some(TimeSpan::from_ms_i(40)) });
        f.state.transport.set_remembered(PlaybackRange { start: Some(TimeSpan::ZERO), end: Some(TimeSpan::from_ms_i(40)) });
        f.state.play_pause(start);
        assert_eq!(f.state.engine_state(), Transport::Playing);
        assert!(f.state.transport_active());
        let ui = f.state.ui_state();
        assert_eq!(ui.transport.play_tooltip, "Pause F5");
        assert!(ui.transport.stop_enabled);

        // Let the frame worker end between UI ticks. The pending transition must
        // keep the transport active until one final tick restores its UI state.
        for _ in 0..300 {
            if f.state.engine_state() == Transport::Stopped {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(f.state.engine_state(), Transport::Stopped);
        assert!(f.state.transport_active());
        assert!(f.state.transport_tick(Instant::now()));
        assert!(!f.state.transport_active());
        assert_eq!(f.state.ui_state().position_text, "");
        assert_eq!(f.state.grid.playback_range().end, Some(TimeSpan::from_ms_i(40)));
        // The cursor brings the position back.
        f.state.set_cursor(secs(1));
        assert_eq!(f.state.ui_state().position_text, "0:01.000");
    }

    #[test]
    fn another_windows_playback_is_not_this_ones() {
        let mut a = fixture("own-a", None);
        let b = fixture("own-b", None);
        a.state.grid.set_playback_range(PlaybackRange { start: Some(TimeSpan::ZERO), end: Some(secs(30)) });
        a.state.play_pause(Instant::now());
        assert_eq!(a.state.engine_state(), Transport::Playing);
        assert_eq!(b.state.engine_state(), Transport::Stopped);
        a.state.stop();
        assert_eq!(a.state.engine_state(), Transport::Stopped);
    }

    #[test]
    fn closing_a_replaced_editor_keeps_the_current_editor_and_preview_active() {
        let mut a = fixture("close-replaced", None);
        let core = a.state.core.clone();
        let second = Arc::new(Mutex::new(SequenceSession::new_untitled(&core.session_environment())));
        let mut b = SequencerState::with_settings(core.clone(), second, a.state.settings.clone(), a._dir.clone());
        b.finish_load();
        let notices = Arc::new(Mutex::new(Vec::new()));
        let recorded = notices.clone();
        core.subscribe_player_active(Arc::new(move |active| recorded.lock().push(active)));
        a.state.play_pause(Instant::now());
        // Timeline/FFI Play also enters through ShowPlayback, without the shell's
        // former process-global window id. Both layers must observe that owner.
        core.playback.play(&mut b.session.lock(), secs(2), secs(30), false);
        assert_eq!(a.state.engine_state(), Transport::Stopped);
        assert_eq!(b.engine_state(), Transport::Playing);
        a.state.transport_tick(Instant::now());
        assert_eq!(*notices.lock(), vec![true], "retired completion must not lower the active preview");
        assert_eq!(a.state.finish_close(), None);
        assert_eq!(b.engine_state(), Transport::Playing);
        assert_eq!(b.finish_close(), None);
        assert_eq!(b.engine_state(), Transport::Stopped);
    }

    #[test]
    fn separate_profiles_do_not_share_editor_playback_ownership() {
        let mut a = fixture("independent-a", None);
        let mut b = fixture("independent-b", None);
        a.state.play_pause(Instant::now());
        b.state.play_pause(Instant::now());
        assert_eq!(a.state.engine_state(), Transport::Playing);
        assert_eq!(b.state.engine_state(), Transport::Playing);
        assert_eq!(a.state.finish_close(), None);
        assert_eq!(a.state.engine_state(), Transport::Stopped);
        assert_eq!(b.state.engine_state(), Transport::Playing);
        b.state.stop();
    }

    #[test]
    fn delay_flows_through_the_state() {
        let mut f = fixture("delay", None);
        f.state.transport.set_delay(DelayOption::Seconds5);
        let t = Instant::now();
        f.state.play_pause(t);
        assert_eq!(f.state.engine_state(), Transport::Stopped);
        assert!(f.state.transport_active());
        f.state.transport_tick(t + Duration::from_secs(5));
        assert_eq!(f.state.engine_state(), Transport::Playing);
        f.state.stop();
    }

    #[test]
    fn escape_leaves_draw_mode() {
        let mut f = fixture("escape", None);
        f.state.set_draw_mode();
        f.state.toggle_selected_effect(SET_LEVEL);
        assert_eq!(f.state.ui_state().selected_effect, Some(SET_LEVEL));
        let outcome = f.state.run_key(Key::Escape, Mods::NONE, Instant::now());
        assert_eq!(outcome.request, Some(UiRequest::ClearEffectsSelection));
        let ui = f.state.ui_state();
        assert!(!ui.draw_mode && ui.selection_mode && ui.selected_effect.is_none());
    }

    #[test]
    fn selected_effect_toggles() {
        let mut f = fixture("effectpick", None);
        assert_eq!(f.state.toggle_selected_effect(SET_LEVEL), Some(SET_LEVEL));
        assert_eq!(f.state.toggle_selected_effect(SET_LEVEL), None);
    }

    #[test]
    fn drag_box_filter_turns_on_and_invert_flips_every_item() {
        let mut f = fixture("dragbox", None);
        f.state.toggle_drag_box_type(SET_LEVEL);
        let ui = f.state.ui_state();
        assert!(ui.drag_box_filter && ui.drag_box_filter_types == vec![SET_LEVEL]);
        f.state.invert_drag_box_types();
        let ui = f.state.ui_state();
        assert!(!ui.drag_box_filter_types.contains(&SET_LEVEL));
        assert_eq!(ui.drag_box_filter_types.len(), vixen_effect_registry::all().len() - 1);
        assert!(f.state.drag_box_invert_checked());
    }

    #[test]
    fn view_changes_set_the_silent_save_flag_and_go_in_the_document() {
        let mut f = fixture("view", Some("Song.tim"));
        assert_eq!(f.state.close_action(false), CloseAction::Close);
        f.state.zoom_time(0.8);
        let tpp = f.state.session.lock().document().time_per_pixel;
        assert_eq!(tpp, TimeSpan(80_000));
        assert_eq!(f.state.close_action(false), CloseAction::SaveSilently);
    }

    #[test]
    fn start_and_end_set_the_range_without_marking_modified() {
        let mut f = fixture("startend", Some("Song.tim"));
        f.state.goto_end();
        let range = f.state.grid.playback_range();
        assert_eq!(range.end, Some(f.state.session.lock().length()));
        f.state.goto_start();
        assert_eq!(f.state.grid.playback_range().start, Some(TimeSpan::ZERO));
        assert!(!f.state.session.lock().is_modified());
    }

    #[test]
    fn range_is_saved_and_restored() {
        let mut f = fixture("rangesave", None);
        let range = PlaybackRange { start: Some(secs(2)), end: None };
        f.state.grid.set_playback_range(range);
        let target = f.state.core.paths.sequence_dir().join("R.tim");
        let prepared = f.state.prepare_save(Some(&target)).unwrap();
        prepared.write().unwrap();
        f.state.finish_save(prepared.target().to_path_buf(), true);
        let loaded = vixen_io::load_sequence(prepared.target()).unwrap();
        assert_eq!(loaded.default_playback_start_time, Some(secs(2)));
        assert_eq!(loaded.default_playback_end_time, None);
    }

    #[test]
    fn window_settings_reach_the_files() {
        let mut f = fixture("settings", Some("Song.tim"));
        f.state.prefs.snap_strength = 4;
        f.state.toolbars.set_visible(StripId::Effects, true);
        f.state.panels.toggle(PanelId::Layers);
        assert_eq!(f.state.finish_close(), None);
        let settings = ProfileSettings::open(f._dir.join("Settings.xml"));
        assert_eq!(EditorPrefs::read(&settings).snap_strength, 4);
        assert!(ToolbarLayout::restore(&settings).is_visible(StripId::Effects));
        assert!(!panels::load_layout(&f._dir).is_visible(PanelId::Layers));
    }

    #[test]
    fn catalog_icons_exist_in_the_resources() {
        let resources = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../app/Resources/effects");
        for entry in effect_catalog() {
            assert!(resources.join(format!("{}.png", entry.icon)).exists(), "{} has no icon {}", entry.name, entry.icon);
        }
    }

    #[test]
    fn unmapped_effects_map_or_go() {
        let mut f = fixture("unmapped", None);
        let ghost = Uuid::new_v4();
        let target = f.nodes[1].clone();
        let (keep, gone) = {
            let mut s = f.state.session.lock();
            let keep = s.add_new_effect(SET_LEVEL, f.nodes[0].0, "A", secs(1), secs(1)).unwrap();
            let gone = s.add_new_effect(SET_LEVEL, f.nodes[0].0, "A", secs(2), secs(1)).unwrap();
            s.set_target(keep, ghost, "Ghost");
            s.set_target(gone, Uuid::new_v4(), "Other Ghost");
            (keep, gone)
        };
        let unmapped = f.state.unmapped_targets();
        assert_eq!(unmapped.len(), 2);
        assert!(unmapped.iter().any(|(id, name)| *id == ghost && name == "Ghost"));
        f.state.apply_element_map(Some(&[(ghost, target.0)]));
        let s = f.state.session.lock();
        assert_eq!(s.effect(keep).unwrap().node.targets[0].0, target.0);
        assert!(s.effect(gone).is_none());
        assert!(s.is_modified());
    }

    #[test]
    fn answering_no_to_unmapped_drops_them_at_load() {
        let mut f = fixture("unmapped-no", None);
        let gone = {
            let mut s = f.state.session.lock();
            let gone = s.add_new_effect(SET_LEVEL, f.nodes[0].0, "A", secs(2), secs(1)).unwrap();
            s.set_target(gone, Uuid::new_v4(), "Ghost");
            gone
        };
        f.state.apply_element_map(None);
        f.state.load_finished = false;
        f.state.finish_load();
        assert!(f.state.session.lock().effect(gone).is_none());
        assert_eq!(f.state.load_report().dropped_without_row, 1);
    }

    #[test]
    fn deprecated_counts_show_in_the_report() {
        let mut f = fixture("deprecated", None);
        let tree = {
            let mut t = (**f.state.session.lock().tree()).clone();
            let id = t.find(&f.nodes[2].0).unwrap();
            t.node_mut(id).tags.push(crate::sequencer::grid::DEPRECATED_TAG);
            t
        };
        let env = SessionEnvironment { video_cache: None,
            tree: Arc::new(tree),
            store: Arc::new(ModuleStore::new()),
            module_data_root: None,
            frame_ms: 25,
            media_dir: f.state.core.paths.media_dir(),
        };
        let mut session = SequenceSession::new_untitled(&env);
        session.add_new_effect(SET_LEVEL, f.nodes[2].0, "C", secs(1), secs(1)).unwrap();
        session.add_new_effect(SET_LEVEL, f.nodes[2].0, "C", secs(3), secs(1)).unwrap();
        let session = Arc::new(Mutex::new(session));
        let settings = Arc::new(ProfileSettings::open(f._dir.join("Settings.xml")));
        f.state = SequencerState::with_settings(Arc::clone(&f.state.core), session, settings, f._dir.clone());
        f.state.finish_load();
        assert_eq!(f.state.load_report().deprecated, vec![("C".to_string(), 2)]);
    }

    #[test]
    fn audio_ui_follows_the_media() {
        let f = fixture("audio", None);
        let dir = f.state.core.paths.media_dir();
        let ui = f.state.audio_ui();
        assert!(!ui.remove_enabled && !ui.speed_tempo_enabled);
        assert_eq!(ui.button_tooltip, "Associate Audio");
        assert_eq!(ui.timeline_title, "Timeline");

        let mut session = f.state.session.lock();
        session.document_mut().media.push(vixen_io::MediaEntry {
            type_id: Uuid::nil(),
            instance_id: Uuid::new_v4(),
            file_name: "gone.mp3".to_string(),
            data: None,
        });
        drop(session);
        // Missing: the two stay enabled and the rest stay at their defaults.
        let ui = f.state.audio_ui();
        assert!(ui.remove_enabled && ui.beat_bar_enabled && !ui.speed_tempo_enabled);
        assert_eq!(ui.timeline_title, "Timeline");
        assert_eq!(f.state.missing_audio(), Some(dir.join("gone.mp3")));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("gone.mp3"), b"x").unwrap();
        let ui = f.state.audio_ui();
        assert!(ui.speed_tempo_enabled);
        assert_eq!(ui.button_tooltip, "Associated Audio: gone.mp3");
        assert_eq!(ui.timeline_title, "Timeline - gone.mp3");
        assert_eq!(f.state.missing_audio(), None);

        let session = f.state.session.lock();
        assert!(!timing_source_exists(session.document(), &dir));
        drop(session);
        f.state.session.lock().document_mut().selected_timing_provider =
            SelectedTimingProvider { provider_type: Some("Media".to_string()), source_name: Some("gone.mp3".to_string()) };
        let session = f.state.session.lock();
        assert!(timing_source_exists(session.document(), &dir));
    }

    #[test]
    fn undo_records_use_the_stack() {
        let mut f = fixture("stack", None);
        f.state.session.lock().record_undo(UndoAction::LayersChanged { layers: vec![] });
        assert_eq!(f.state.ui_state().undo_name.as_deref(), Some("Modified 0 effect layers"));
    }
}
