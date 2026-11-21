//! The Mark Collections panel, and mark import and export: `docs/spec/marks-layers-analysis.md`, sections A to C. The logic is
//! `vixen_session::marks`; this only converts.
//!
//! One `MarksPanel` per showing of the panel: its header toggles are worked out when it's made, as upstream rebuilds its view model each
//! time the panel opens (F114). Edits go straight to the session, which the timeline follows through its edit counter.

pub mod image_maps;
pub mod node_select;

use crate::core::FfiError;
use crate::sequence::SequencerSession;
use parking_lot::Mutex;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use uuid::Uuid;
use vixen_core::marks::{MarkCollection, MarkCollectionType};
use vixen_core::Color;
use vixen_io::marks_files::{PangolinMode, PangolinRecord};
use vixen_session::marks as logic;
use vixen_session::sequence::{RenderCounters, SequenceSession};

#[derive(uniffi::Enum, Copy, Clone, PartialEq, Eq, Debug)]
pub enum MarkKind {
    Generic,
    Phrase,
    Word,
    Phoneme,
    State,
}

impl From<MarkCollectionType> for MarkKind {
    fn from(t: MarkCollectionType) -> Self {
        match t {
            MarkCollectionType::Generic => MarkKind::Generic,
            MarkCollectionType::Phrase => MarkKind::Phrase,
            MarkCollectionType::Word => MarkKind::Word,
            MarkCollectionType::Phoneme => MarkKind::Phoneme,
            MarkCollectionType::State => MarkKind::State,
        }
    }
}

impl From<MarkKind> for MarkCollectionType {
    fn from(k: MarkKind) -> Self {
        match k {
            MarkKind::Generic => MarkCollectionType::Generic,
            MarkKind::Phrase => MarkCollectionType::Phrase,
            MarkKind::Word => MarkCollectionType::Word,
            MarkKind::Phoneme => MarkCollectionType::Phoneme,
            MarkKind::State => MarkCollectionType::State,
        }
    }
}

/// The clickable column headers (F8).
#[derive(uniffi::Enum, Copy, Clone, PartialEq, Eq, Debug)]
pub enum MarkHeaderKind {
    Lines,
    TailLines,
    Bar,
    Lock,
}

impl From<MarkHeaderKind> for logic::MarkHeader {
    fn from(h: MarkHeaderKind) -> Self {
        match h {
            MarkHeaderKind::Lines => logic::MarkHeader::Lines,
            MarkHeaderKind::TailLines => logic::MarkHeader::TailLines,
            MarkHeaderKind::Bar => logic::MarkHeader::Bar,
            MarkHeaderKind::Lock => logic::MarkHeader::Lock,
        }
    }
}

// STUDIO-BEGIN marks-header-label: a header toggle's name on the shared Undo (R40)
impl MarkHeaderKind {
    fn studio_label(self) -> &'static str {
        use vixen_session::studio_sequence::marks_panel::flag_label;
        flag_label(match self {
            MarkHeaderKind::Lines => logic::MarkFlag::Lines,
            MarkHeaderKind::TailLines => logic::MarkFlag::TailLines,
            MarkHeaderKind::Bar => logic::MarkFlag::Bar,
            MarkHeaderKind::Lock => logic::MarkFlag::Lock,
        })
    }
}
// STUDIO-END marks-header-label

/// A row's checkboxes and check menu items.
#[derive(uniffi::Enum, Copy, Clone, PartialEq, Eq, Debug)]
pub enum MarkFlagKind {
    Lines,
    TailLines,
    Bar,
    Lock,
    SolidLine,
    Bold,
}

impl From<MarkFlagKind> for logic::MarkFlag {
    fn from(f: MarkFlagKind) -> Self {
        match f {
            MarkFlagKind::Lines => logic::MarkFlag::Lines,
            MarkFlagKind::TailLines => logic::MarkFlag::TailLines,
            MarkFlagKind::Bar => logic::MarkFlag::Bar,
            MarkFlagKind::Lock => logic::MarkFlag::Lock,
            MarkFlagKind::SolidLine => logic::MarkFlag::SolidLine,
            MarkFlagKind::Bold => logic::MarkFlag::Bold,
        }
    }
}

/// One row of the panel.
#[derive(uniffi::Record, Clone, PartialEq, Debug)]
pub struct MarkRowInfo {
    pub id: String,
    pub name: String,
    pub argb: u32,
    pub is_default: bool,
    pub show_lines: bool,
    pub tail_enabled: bool,
    pub show_tail: bool,
    pub show_bar: bool,
    pub locked: bool,
    pub level: i32,
    pub kind: MarkKind,
    pub linked_to: Option<String>,
    pub solid: bool,
    pub bold: bool,
    pub mark_count: u32,
}

/// An item of the Linked To submenu. `id` is nil for `None`.
#[derive(uniffi::Record, Clone, PartialEq, Debug)]
pub struct MarkLinkChoice {
    pub id: Option<String>,
    pub name: String,
    pub checked: bool,
}

/// How a rename ended (F15).
#[derive(uniffi::Enum, Copy, Clone, PartialEq, Eq, Debug)]
pub enum MarkRenameEnd {
    Renamed,
    Unchanged,
    Restored,
    /// A duplicate: keep the field open with its message.
    StillInvalid,
}

/// The Import Type Selection's radios (F25), top to bottom.
#[derive(uniffi::Enum, Copy, Clone, PartialEq, Eq, Debug)]
pub enum MarkImportKind {
    VampBars,
    VampBeats,
    AudacityBeats,
    Vixen3,
    XTiming,
    Papagayo,
    SingingFaces,
    PangolinBeyond,
}

impl From<MarkImportKind> for logic::ImportKind {
    fn from(k: MarkImportKind) -> Self {
        match k {
            MarkImportKind::VampBars => logic::ImportKind::VampBars,
            MarkImportKind::VampBeats => logic::ImportKind::VampBeats,
            MarkImportKind::AudacityBeats => logic::ImportKind::AudacityBeats,
            MarkImportKind::Vixen3 => logic::ImportKind::Vixen3,
            MarkImportKind::XTiming => logic::ImportKind::XTiming,
            MarkImportKind::Papagayo => logic::ImportKind::Papagayo,
            MarkImportKind::SingingFaces => logic::ImportKind::SingingFaces,
            MarkImportKind::PangolinBeyond => logic::ImportKind::PangolinBeyond,
        }
    }
}

/// A picker's file type. Every picker also offers All Files.
#[derive(uniffi::Record, Clone, PartialEq, Debug)]
pub struct MarkFileFilter {
    pub description: String,
    pub extensions: Vec<String>,
}

impl From<logic::FileFilter> for MarkFileFilter {
    fn from(f: logic::FileFilter) -> Self {
        MarkFileFilter { description: f.description.to_string(), extensions: f.extensions.iter().map(|e| e.to_string()).collect() }
    }
}

/// A row of the Select Imported Mark Collections dialog (F35).
#[derive(uniffi::Record, Clone, PartialEq, Debug)]
pub struct MarkImportOption {
    pub name: String,
    pub has_duplicate_name: bool,
}

/// What comes after choosing an import file.
#[derive(uniffi::Enum, Clone, PartialEq, Debug)]
pub enum MarkImportStep {
    /// F35's rows. None: the flow ends with no message (F26).
    Choose { options: Vec<MarkImportOption> },
    /// F34 with two or more colours: ask whether to make one collection per colour.
    PangolinChoice,
    /// F32 shows this first, then F35.
    ChooseAfterMessage { title: String, message: String, options: Vec<MarkImportOption> },
    /// The importer's error alert.
    Failed { title: String, message: String },
    /// A part of the port not built yet.
    NotBuilt { message: String },
}

#[derive(uniffi::Enum, Copy, Clone, PartialEq, Eq, Debug)]
pub enum MarkExportFormat {
    Vixen,
    Audacity,
    PangolinBeyond,
}

impl From<MarkExportFormat> for logic::ExportFormat {
    fn from(f: MarkExportFormat) -> Self {
        match f {
            MarkExportFormat::Vixen => logic::ExportFormat::Vixen,
            MarkExportFormat::Audacity => logic::ExportFormat::Audacity,
            MarkExportFormat::PangolinBeyond => logic::ExportFormat::PangolinBeyond,
        }
    }
}

/// A row of the Export Mark Collections dialog (F37).
#[derive(uniffi::Record, Clone, PartialEq, Debug)]
pub struct MarkExportRow {
    pub id: String,
    pub name: String,
    pub argb: u32,
    pub include_text: bool,
}

/// F25's radio label.
#[uniffi::export]
pub fn mark_import_label(kind: MarkImportKind) -> String {
    logic::ImportKind::from(kind).label().to_string()
}

/// The picker filter for an import, or nil for the Lyric Browser.
#[uniffi::export]
pub fn mark_import_filter(kind: MarkImportKind) -> Option<MarkFileFilter> {
    logic::ImportKind::from(kind).filter().map(Into::into)
}

/// An import not built yet in this port, with what to say, so the flow stops before its picker.
#[uniffi::export]
pub fn mark_import_not_built(kind: MarkImportKind) -> Option<String> {
    logic::not_built(kind.into()).map(str::to_string)
}

#[uniffi::export]
pub fn mark_export_filter(format: MarkExportFormat) -> MarkFileFilter {
    logic::ExportFormat::from(format).filter().into()
}

/// F27: only the Vixen export panel starts in, and updates, the last folder.
#[uniffi::export]
pub fn mark_export_uses_last_folder(format: MarkExportFormat) -> bool {
    logic::ExportFormat::from(format).uses_last_folder()
}

/// F18's prompt: title, prompt, default, minimum, maximum.
#[derive(uniffi::Record, Clone, PartialEq, Debug)]
pub struct MarkOffsetPrompt {
    pub title: String,
    pub prompt: String,
    pub default_ms: i32,
    pub minimum_ms: i32,
    pub maximum_ms: i32,
}

#[uniffi::export]
pub fn mark_offset_prompt() -> MarkOffsetPrompt {
    MarkOffsetPrompt {
        title: logic::OFFSET_TITLE.to_string(),
        prompt: logic::OFFSET_PROMPT.to_string(),
        default_ms: logic::OFFSET_DEFAULT_MS,
        minimum_ms: logic::OFFSET_RANGE_MS.0,
        maximum_ms: logic::OFFSET_RANGE_MS.1,
    }
}

/// F35's caution tooltip.
#[uniffi::export]
pub fn mark_duplicate_name_tip() -> String {
    logic::DUPLICATE_NAME_TIP.to_string()
}

enum Pending {
    None,
    Candidates(Vec<MarkCollection>),
    Pangolin(Vec<PangolinRecord>),
}

/// The Mark Collections panel's model for one showing of the panel.
#[derive(uniffi::Object)]
pub struct MarksPanel {
    session: Arc<Mutex<SequenceSession>>,
    counters: Arc<RenderCounters>,
    data_root: PathBuf,
    header: Mutex<logic::HeaderFlags>,
    pending: Mutex<Pending>,
    studio_offset: Mutex<Option<Vec<MarkCollection>>>, // STUDIO: the live offset field's starting marks (R40)
}

#[uniffi::export]
impl SequencerSession {
    /// A fresh panel model. Make one each time the panel is shown, so the header toggles start from the rows (F114).
    pub fn marks_panel(&self) -> Arc<MarksPanel> {
        let session = self.session.lock();
        let header = logic::HeaderFlags::new(session.marks());
        let counters = Arc::clone(&session.counters);
        drop(session);
        Arc::new(MarksPanel {
            session: Arc::clone(&self.session),
            counters,
            data_root: self.core.paths.root().to_path_buf(),
            header: Mutex::new(header),
            pending: Mutex::new(Pending::None),
            studio_offset: Mutex::new(None), // STUDIO: the live offset field's starting marks (R40)
        })
    }
}

fn id_of(text: &str) -> Option<Uuid> {
    Uuid::parse_str(text).ok()
}

fn row_info(r: logic::MarkRow) -> MarkRowInfo {
    MarkRowInfo {
        id: r.id.to_string(),
        name: r.name,
        argb: r.color.to_argb_u32(),
        is_default: r.is_default,
        show_lines: r.show_lines,
        tail_enabled: r.tail_enabled,
        show_tail: r.show_tail,
        show_bar: r.show_bar,
        locked: r.locked,
        level: r.level,
        kind: r.kind.into(),
        linked_to: r.linked_to.map(|id| id.to_string()),
        solid: r.solid,
        bold: r.bold,
        mark_count: r.mark_count as u32,
    }
}

impl MarksPanel {
    // STUDIO-BEGIN marks-panel-undo-ffi: every panel edit is one step on the sequence's Undo, named by `label` (R40)
    /// Runs one edit on a copy of the list, hands the result to the session as the change requires, and records it on the shared Undo.
    fn edit(&self, label: &str, f: impl FnOnce(&mut Vec<MarkCollection>) -> logic::MarksChange) -> logic::MarksChange {
        let mut session = self.session.lock();
        vixen_session::studio_sequence::marks_panel::apply(&mut session, label, false, f)
    }

    fn edit_row(&self, label: &str, id: &str, f: impl FnOnce(&mut Vec<MarkCollection>, Uuid) -> logic::MarksChange) {
        if let Some(id) = id_of(id) {
            self.edit(label, |marks| f(marks, id));
        }
    }
    // STUDIO-END marks-panel-undo-ffi

    fn options(&self, candidates: &[MarkCollection]) -> Vec<MarkImportOption> {
        let session = self.session.lock();
        logic::import_options(session.marks(), candidates).into_iter().map(|o| MarkImportOption { name: o.name, has_duplicate_name: o.has_duplicate_name }).collect()
    }

    fn step_for(&self, read: logic::ImportRead) -> MarkImportStep {
        match read {
            logic::ImportRead::Candidates(c) => {
                let options = self.options(&c);
                *self.pending.lock() = if c.is_empty() { Pending::None } else { Pending::Candidates(c) };
                MarkImportStep::Choose { options }
            }
            logic::ImportRead::CandidatesAfterMessage { title, message, candidates } => {
                let options = self.options(&candidates);
                *self.pending.lock() = if candidates.is_empty() { Pending::None } else { Pending::Candidates(candidates) };
                MarkImportStep::ChooseAfterMessage { title: title.to_string(), message, options }
            }
            logic::ImportRead::PangolinChoice(records) => {
                *self.pending.lock() = Pending::Pangolin(records);
                MarkImportStep::PangolinChoice
            }
            logic::ImportRead::Failed { message, title, detail } => {
                vixen_session::logging::error("MarkImportExportService", message, Some(&format!("{title}: {detail}")));
                *self.pending.lock() = Pending::None;
                MarkImportStep::Failed { title: title.to_string(), message: message.to_string() }
            }
            logic::ImportRead::NotBuilt(message) => {
                *self.pending.lock() = Pending::None;
                MarkImportStep::NotBuilt { message }
            }
        }
    }
}

#[uniffi::export]
impl MarksPanel {
    /// Bumps whenever the sequence's effects or marks change from anywhere: a lock-free read, cheap to check a few times a second.
    pub fn revision(&self) -> u64 {
        self.counters.edits.load(Ordering::Relaxed)
    }

    /// Every row, in list order.
    pub fn rows(&self) -> Vec<MarkRowInfo> {
        let session = self.session.lock();
        logic::rows(session.marks()).into_iter().map(row_info).collect()
    }

    /// F4. Returns the new collection's id.
    pub fn add_collection(&self) -> String {
        let mut id = Uuid::nil();
        self.edit("Add Mark Collection", |marks| { // STUDIO: undo label
            id = logic::add_collection(marks);
            logic::MarksChange::Collections
        });
        id.to_string()
    }

    /// F17.
    pub fn delete_collection(&self, id: String) {
        self.edit_row("Delete Mark Collection", &id, |marks, id| logic::delete_collection(marks, id)); // STUDIO: undo label
    }

    /// F9.
    pub fn set_active(&self, id: String) {
        self.edit_row("Set Active Mark Collection", &id, |marks, id| logic::set_active(marks, id)); // STUDIO: undo label
    }

    /// F10 to F13, F19, F20.
    pub fn set_flag(&self, id: String, flag: MarkFlagKind, on: bool) {
        self.edit_row(vixen_session::studio_sequence::marks_panel::flag_label(flag.into()), &id, |marks, id| logic::set_flag(marks, id, flag.into(), on)); // STUDIO: undo label
    }

    /// F8. Returns whether any row changed.
    pub fn click_header(&self, header: MarkHeaderKind) -> bool {
        let mut flags = self.header.lock();
        self.edit(header.studio_label(), |marks| flags.click(header.into(), marks)) != logic::MarksChange::None // STUDIO: undo label
    }

    /// The value a header's next click sets, for tests.
    pub fn header_next_value(&self, header: MarkHeaderKind) -> bool {
        self.header.lock().next_value(header.into())
    }

    /// F15's message while typing, or nil.
    pub fn rename_error(&self, id: String, text: String) -> Option<String> {
        let id = id_of(&id)?;
        logic::rename_error(self.session.lock().marks(), id, &text)
    }

    /// F15, on Return or loss of focus.
    pub fn end_rename(&self, id: String, text: String) -> MarkRenameEnd {
        let Some(id) = id_of(&id) else { return MarkRenameEnd::Restored };
        let mut end = logic::RenameEnd::Restored;
        self.edit("Rename Mark Collection", |marks| { // STUDIO: undo label
            end = logic::end_rename(marks, id, &text);
            if end == logic::RenameEnd::Renamed {
                logic::MarksChange::Properties
            } else {
                logic::MarksChange::None
            }
        });
        match end {
            logic::RenameEnd::Renamed => MarkRenameEnd::Renamed,
            logic::RenameEnd::Unchanged => MarkRenameEnd::Unchanged,
            logic::RenameEnd::Restored => MarkRenameEnd::Restored,
            logic::RenameEnd::StillInvalid => MarkRenameEnd::StillInvalid,
        }
    }

    /// F18.
    pub fn offset_time(&self, id: String, ms: i32) {
        self.edit_row(vixen_session::studio_sequence::marks_panel::OFFSET_LABEL, &id, |marks, id| logic::offset_time(marks, id, ms)); // STUDIO: undo label
    }

    // STUDIO-BEGIN marks-live-offset: the Offset Time popover applies its total as it changes, one Undo step per showing (R40)
    /// The live Offset Time field: `total_ms` from the marks as the field found them. `continuing` is false for its first change; later
    /// changes extend the same "Offset Marks" step while it's still the newest.
    pub fn studio_offset_live(&self, id: String, total_ms: i32, continuing: bool) {
        let Some(id) = id_of(&id) else { return };
        let mut base = self.studio_offset.lock();
        let mut session = self.session.lock();
        vixen_session::studio_sequence::marks_panel::offset_live(&mut session, id, total_ms, continuing, &mut base);
    }
    // STUDIO-END marks-live-offset

    /// F21.
    pub fn set_color(&self, id: String, argb: u32) {
        self.edit_row("Mark Collection Color", &id, |marks, id| logic::set_color(marks, id, Color::from_argb_u32(argb))); // STUDIO: undo label
    }

    /// F22.
    pub fn set_level(&self, id: String, level: i32) {
        self.edit_row("Mark Collection Weight", &id, |marks, id| logic::set_level(marks, id, level)); // STUDIO: undo label
    }

    /// F23.
    pub fn set_kind(&self, id: String, kind: MarkKind) {
        self.edit_row("Mark Collection Type", &id, |marks, id| logic::set_kind(marks, id, kind.into())); // STUDIO: undo label
    }

    /// F24's submenu, built as it opens. Nil hides it.
    pub fn link_choices(&self, id: String) -> Option<Vec<MarkLinkChoice>> {
        let id = id_of(&id)?;
        let choices = logic::link_choices(self.session.lock().marks(), id)?;
        Some(choices.into_iter().map(|c| MarkLinkChoice { id: c.id.map(|i| i.to_string()), name: c.name, checked: c.checked }).collect())
    }

    /// F24's choice; nil is `None`.
    pub fn set_link(&self, id: String, parent: Option<String>) {
        let parent = parent.as_deref().and_then(id_of);
        self.edit_row("Link Mark Collection", &id, |marks, id| logic::set_link(marks, id, parent)); // STUDIO: undo label
    }

    /// A collection's mark start times in milliseconds, in order: for checks, not for drawing.
    pub fn mark_times(&self, id: String) -> Vec<f64> {
        let Some(id) = id_of(&id) else { return Vec::new() };
        let session = self.session.lock();
        session.marks().iter().find(|c| c.id == id).map_or_else(Vec::new, |c| c.marks.iter().map(|m| m.start_time.total_ms()).collect())
    }

    /// F6.
    pub fn can_export(&self) -> bool {
        logic::can_export(self.session.lock().marks())
    }

    /// F27: where an import picker, or a Vixen export panel, starts.
    pub fn last_folder(&self) -> String {
        logic::last_folder(&self.data_root).to_string_lossy().into_owned()
    }

    /// F28 to F34: reads the chosen file and remembers its folder. Nothing changes until `commit_import`.
    pub fn import_file(&self, kind: MarkImportKind, path: String) -> MarkImportStep {
        let kind: logic::ImportKind = kind.into();
        let path = Path::new(&path);
        logic::remember_folder(path);
        let read = match std::fs::read(path) {
            Ok(bytes) => logic::read_candidates(kind, &bytes, &path.file_stem().unwrap_or_default().to_string_lossy()),
            Err(e) => logic::read_failed(kind, e.to_string()),
        };
        self.step_for(read)
    }

    /// F33 uses the same xTiming materializer without changing the last file-picker folder.
    pub fn import_song_timing(&self, xml: String) -> MarkImportStep {
        self.step_for(logic::read_candidates(logic::ImportKind::SingingFaces, xml.as_bytes(), ""))
    }

    /// F34's answer: one collection per colour, or a single one in `argb`.
    pub fn import_pangolin(&self, group_by_color: bool, argb: u32) -> MarkImportStep {
        let records = match std::mem::replace(&mut *self.pending.lock(), Pending::None) {
            Pending::Pangolin(r) => r,
            _ => Vec::new(),
        };
        let mode = if group_by_color { PangolinMode::GroupByColor } else { PangolinMode::Single(Color::from_argb_u32(argb)) };
        self.step_for(logic::ImportRead::Candidates(logic::pangolin_candidates(&records, mode)))
    }

    /// F35's OK and F36: adds the checked rows, by index in the order offered. Returns how many were added.
    pub fn commit_import(&self, selected: Vec<u32>) -> u32 {
        let Pending::Candidates(candidates) = std::mem::replace(&mut *self.pending.lock(), Pending::None) else { return 0 };
        let chosen: Vec<MarkCollection> =
            candidates.into_iter().enumerate().filter(|(i, _)| selected.contains(&(*i as u32))).map(|(_, c)| c).collect();
        let count = chosen.len() as u32;
        self.edit("Import Marks", |marks| if logic::commit_import(marks, chosen) { logic::MarksChange::Collections } else { logic::MarksChange::None }); // STUDIO: undo label
        count
    }

    /// Cancel anywhere in the import flow.
    pub fn cancel_import(&self) {
        *self.pending.lock() = Pending::None;
    }

    /// F37's rows.
    pub fn export_rows(&self) -> Vec<MarkExportRow> {
        logic::export_rows(self.session.lock().marks())
            .into_iter()
            .map(|r| MarkExportRow { id: r.id.to_string(), name: r.name, argb: r.color.to_argb_u32(), include_text: r.include_text })
            .collect()
    }

    /// F38 to F40: writes the file. FIX for F38 and GUARD for F39, F40: a failure comes back as an error to show, where upstream logs it or
    /// lets it go uncaught.
    pub fn export_marks(&self, format: MarkExportFormat, included: Vec<String>, text_included: Vec<String>, path: String) -> Result<(), FfiError> {
        let format: logic::ExportFormat = format.into();
        let included: Vec<Uuid> = included.iter().filter_map(|s| id_of(s)).collect();
        let text_included: Vec<Uuid> = text_included.iter().filter_map(|s| id_of(s)).collect();
        let bytes = {
            let session = self.session.lock();
            logic::build_export(format, session.marks(), &included, &text_included).map_err(FfiError::new)?
        };
        let path = Path::new(&path);
        if format.uses_last_folder() {
            logic::remember_folder(path);
        }
        std::fs::write(path, bytes).map_err(|e| FfiError::new(e.to_string()))
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Beat and bar detection, F67 to F75
// ---------------------------------------------------------------------------------------------------------------------

use vixen_session::marks::beats::{self, BeatDetection, BeatJob};

/// Where a detection has got to.
#[derive(uniffi::Enum, Clone, PartialEq, Debug)]
pub enum BeatJobState {
    /// Reading the audio and analysing its first ten seconds.
    Preparing,
    /// The Beats and Bars dialog can show.
    Ready,
    /// The progress sheet. Past 100%, `finalizing` is the tracker's long last step.
    Running { percent: i32, finalizing: bool },
    /// Done: the collections are in the sequence.
    Finished,
    /// An alert titled "Beats and Bars" with this text; nothing changed.
    Failed { message: String },
    Cancelled,
}

impl From<BeatJob> for BeatJobState {
    fn from(j: BeatJob) -> Self {
        match j {
            BeatJob::Preparing => BeatJobState::Preparing,
            BeatJob::Ready => BeatJobState::Ready,
            BeatJob::Running { percent, finalizing } => BeatJobState::Running { percent, finalizing },
            BeatJob::Finished => BeatJobState::Finished,
            BeatJob::Failed(message) => BeatJobState::Failed { message },
            BeatJob::Cancelled => BeatJobState::Cancelled,
        }
    }
}

/// The four outputs of F72, left to right.
#[derive(uniffi::Enum, Copy, Clone, PartialEq, Eq, Debug)]
pub enum BeatOutput {
    Bars,
    BeatCounts,
    BeatSplits,
    AllFeatures,
}

/// Everything the Beats and Bars dialog shows (F69 to F72).
#[derive(uniffi::Record, Clone, PartialEq, Debug)]
pub struct BeatDialogState {
    pub bars_checked: bool,
    pub beat_counts_checked: bool,
    pub beat_splits_checked: bool,
    pub all_features_checked: bool,
    pub bars_color_enabled: bool,
    pub beat_counts_color_enabled: bool,
    pub beat_splits_color_enabled: bool,
    pub all_color_enabled: bool,
    pub beat_splits_checkbox_enabled: bool,
    pub base_name: String,
    pub bars_argb: u32,
    pub beat_counts_argb: u32,
    pub beat_splits_argb: u32,
    pub all_argb: u32,
    pub beats_per_bar: i32,
    pub note_size: i32,
    pub split_beats: bool,
    /// `60000 / beatPeriod` as `F1`, `∞` for a zero period.
    pub bpm_text: String,
    /// `F2`.
    pub bar_period_text: String,
    /// `F0`.
    pub div_time_text: String,
    pub generate_enabled: bool,
}

/// F73: the waveform as interleaved (min, max) per column over `period_ms`, and the yellow lines.
#[derive(uniffi::Record, Clone, PartialEq, Debug)]
pub struct BeatPreview {
    pub lines_ms: Vec<f64>,
    pub period_ms: f64,
    pub peaks: Vec<f32>,
}

fn fixed(value: f64, places: usize) -> String {
    if value.is_infinite() {
        if value > 0.0 { "∞".to_string() } else { "-∞".to_string() }
    } else if value.is_nan() {
        "NaN".to_string()
    } else {
        format!("{value:.places$}")
    }
}

/// One run of Tools > Audio > Beat/Bar Detection.
#[derive(uniffi::Object)]
pub struct BeatDetector {
    inner: BeatDetection,
}

#[uniffi::export]
impl SequencerSession {
    /// F67: on when the sequence has any media, even if the file is missing.
    pub fn beat_detection_available(&self) -> bool {
        beats::detection_available(&self.session.lock())
    }

    /// Starts F68: the first ten seconds analyse on a worker; poll `job` no faster than the display.
    pub fn start_beat_detection(&self) -> Arc<BeatDetector> {
        Arc::new(BeatDetector { inner: BeatDetection::start(Arc::clone(&self.session), &self.core.paths.media_dir()) })
    }
}

#[uniffi::export]
impl BeatDetector {
    pub fn job(&self) -> BeatJobState {
        self.inner.job().into()
    }

    pub fn dialog(&self) -> BeatDialogState {
        let m = self.inner.model();
        BeatDialogState {
            bars_checked: m.bars_checked,
            beat_counts_checked: m.beat_counts_checked,
            beat_splits_checked: m.beat_splits_checked,
            all_features_checked: m.all_features_checked,
            bars_color_enabled: m.bars_color_enabled,
            beat_counts_color_enabled: m.beat_counts_color_enabled,
            beat_splits_color_enabled: m.beat_splits_color_enabled,
            all_color_enabled: m.all_color_enabled,
            beat_splits_checkbox_enabled: m.beat_splits_checkbox_enabled,
            base_name: m.collection_base_name.clone(),
            bars_argb: m.bars_color.to_argb_u32(),
            beat_counts_argb: m.beat_counts_color.to_argb_u32(),
            beat_splits_argb: m.beat_splits_color.to_argb_u32(),
            all_argb: m.all_features_color.to_argb_u32(),
            beats_per_bar: m.beats_per_bar,
            note_size: m.note_size(),
            split_beats: m.split_beats,
            bpm_text: fixed(m.bpm(), 1),
            bar_period_text: fixed(m.bar_period_seconds(), 2),
            div_time_text: fixed(m.div_time_ms(), 0),
            generate_enabled: m.generate_enabled(),
        }
    }

    /// An output's checkbox (F72), through the dialog's own enable rules.
    pub fn set_output(&self, output: BeatOutput, on: bool) {
        self.inner.edit_model(|m| match output {
            BeatOutput::Bars => m.set_bars_checked(on),
            BeatOutput::BeatCounts => m.set_beat_counts_checked(on),
            BeatOutput::BeatSplits => m.set_beat_splits_checked(on),
            BeatOutput::AllFeatures => m.set_all_features_checked(on),
        });
    }

    pub fn set_output_color(&self, output: BeatOutput, argb: u32) {
        let color = Color::from_argb_u32(argb);
        self.inner.edit_model(|m| match output {
            BeatOutput::Bars => m.bars_color = color,
            BeatOutput::BeatCounts => m.beat_counts_color = color,
            BeatOutput::BeatSplits => m.beat_splits_color = color,
            BeatOutput::AllFeatures => m.all_features_color = color,
        });
    }

    /// F70's Split Beats, which F71 ties Beat Splits to.
    pub fn set_split_beats(&self, on: bool) {
        self.inner.edit_model(|m| m.set_split_beats(on));
    }

    /// F70's time signature menu: beats per bar 2 to 16.
    pub fn set_beats_per_bar(&self, beats_per_bar: i32) {
        self.inner.edit_model(|m| m.set_beats_per_bar(beats_per_bar));
    }

    /// F72's Collection Base Name, read when Generate is pressed. A blank name is accepted (KEEP).
    pub fn set_base_name(&self, name: String) {
        self.inner.edit_model(|m| m.collection_base_name = name);
    }

    pub fn preview(&self) -> BeatPreview {
        let (lines, period) = self.inner.preview_lines();
        BeatPreview {
            lines_ms: lines.iter().map(|t| t.total_ms()).collect(),
            period_ms: period.total_ms(),
            peaks: self.inner.preview_peaks().into_iter().flat_map(|(lo, hi)| [lo, hi]).collect(),
        }
    }

    /// F68 step 5, on a worker.
    pub fn generate(&self) {
        self.inner.generate();
    }

    /// Cancel on the dialog or the progress sheet (GUARD F74: upstream has none).
    pub fn cancel(&self) {
        self.inner.cancel();
    }
}

/// The time signatures F70's menu offers, as (beats per bar, note size).
#[uniffi::export]
pub fn beat_time_signatures() -> Vec<Vec<i32>> {
    (2..=16).map(|bpb| vec![bpb, vixen_analysis_note_size(bpb)]).collect()
}

fn vixen_analysis_note_size(bpb: i32) -> i32 {
    vixen_session::marks::beats::note_size(bpb)
}

// ---------------------------------------------------------------------------------------------------------------------
// The Lyric Converter and the phoneme dictionary: section J

use vixen_session::marks::lipsync;
use vixen_session::marks::lyric_converter as lyric;

#[derive(uniffi::Enum, Copy, Clone, PartialEq, Eq, Debug)]
pub enum LyricPlacement {
    Cursor,
    Clipboard,
    Mark,
}

impl From<lyric::Placement> for LyricPlacement {
    fn from(p: lyric::Placement) -> Self {
        match p {
            lyric::Placement::Cursor => LyricPlacement::Cursor,
            lyric::Placement::Clipboard => LyricPlacement::Clipboard,
            lyric::Placement::Mark => LyricPlacement::Mark,
        }
    }
}

impl From<LyricPlacement> for lyric::Placement {
    fn from(p: LyricPlacement) -> Self {
        match p {
            LyricPlacement::Cursor => lyric::Placement::Cursor,
            LyricPlacement::Clipboard => lyric::Placement::Clipboard,
            LyricPlacement::Mark => lyric::Placement::Mark,
        }
    }
}

#[derive(uniffi::Enum, Copy, Clone, PartialEq, Eq, Debug)]
pub enum LyricAlignment {
    Phoneme,
    Word,
    Phrase,
}

/// What the window shows. The lists are empty and unselected while the Marks group is off, as upstream's are.
#[derive(uniffi::Record)]
pub struct LyricConverterState {
    pub placement: LyricPlacement,
    /// F77: Per Mark Collection is greyed while no collection has marks.
    pub mark_placement_available: bool,
    pub marks_enabled: bool,
    pub collections: Vec<String>,
    pub collection: Option<u32>,
    /// .NET's `TimeSpan` text for each mark's start.
    pub start_offsets: Vec<String>,
    pub start_offset: Option<u32>,
    /// Nothing until the Marks group has been on once.
    pub alignment: Option<LyricAlignment>,
}

#[derive(uniffi::Enum)]
pub enum LyricConvertStep {
    /// F82: show the mapping dialog for `word`, call `add_mapping` if it's accepted, then `resume` either way.
    NeedsMapping { word: String, title: String, message: String },
    /// Put `clipboard_json` on the pasteboard under `lyric_pasteboard_types()`, unless it's nil, where nothing more happens. Add the cursor
    /// to `first_mark` for Cursor, paste at it in default mode for anything but Clipboard, and report with `delivered`.
    Delivered { clipboard_json: Option<String>, count: u32, placement: LyricPlacement, first_mark: i64, clear_text: bool },
    /// Nothing happens.
    Abandoned,
}

/// A title and a message for an alert.
#[derive(uniffi::Record)]
pub struct MarkMessage {
    pub title: String,
    pub message: String,
}

/// The timeline clipboard's pasteboard types: the JSON, and its count of effects as text.
#[derive(uniffi::Record)]
pub struct LyricPasteboardTypes {
    pub data: String,
    pub count: String,
}

#[uniffi::export]
pub fn lyric_pasteboard_types() -> LyricPasteboardTypes {
    use vixen_session::sequencer::clipboard::{PASTEBOARD_COUNT_TYPE, PASTEBOARD_TYPE};
    LyricPasteboardTypes { data: PASTEBOARD_TYPE.to_string(), count: PASTEBOARD_COUNT_TYPE.to_string() }
}

/// One of F82's ten buttons: `text` is what it appends, `image` the picture's name in `Resources/marks/phonemes`.
#[derive(uniffi::Record)]
pub struct LyricPhonemeButton {
    pub caption: String,
    pub text: String,
    pub image: String,
}

/// F82's two rows of five.
#[uniffi::export]
pub fn lyric_failure_buttons() -> Vec<Vec<LyricPhonemeButton>> {
    lipsync::FAILURE_BUTTONS
        .iter()
        .map(|row| {
            row.iter()
                .map(|p| LyricPhonemeButton { caption: p.name().to_string(), text: format!("{} ", p.name()), image: p.name().to_string() })
                .collect()
        })
        .collect()
}

/// F82's title and label for a word, for the marks bar's breakdown, which shows the same dialog.
#[uniffi::export]
pub fn lyric_failure_prompt(word: String) -> MarkMessage {
    MarkMessage { title: lyric::FAILURE_TITLE.to_string(), message: lyric::failure_text(&word) }
}

/// A word and its phonemes' names, empty when it has no mapping.
#[derive(uniffi::Record)]
pub struct MarkWordPhonemes {
    pub word: String,
    pub phonemes: Vec<String>,
}

fn user_dictionary(core: &vixen_session::app::AppCore) -> PathBuf {
    core.paths.root().join("user_dictionary")
}

/// `AddUserMaping`. GUARD F85: a write failure comes back as an alert, with the mapping live for this run.
fn add_mapping(path: &Path, word: &str, mapped: &str) -> Option<MarkMessage> {
    lyric::with_dictionary(path, |d| d.add_user_mapping(word, mapped, path)).err().map(|e| {
        vixen_session::logging::error("LipSyncTextConvert", "The user dictionary couldn't be written.", Some(&e.to_string()));
        MarkMessage {
            title: "Error".to_string(),
            message: format!("The mapping couldn't be saved to the user dictionary, so it lasts only until Vixen closes.\n\n{e}"),
        }
    })
}

#[uniffi::export]
impl SequencerSession {
    /// F86: the phonemes for each of the marks bar's selected words, for Breakdown Word. A word that comes back empty needs F82 first:
    /// `lyric_failure_prompt`, `add_phoneme_mapping`, and then this again.
    pub fn word_phonemes(&self, words: Vec<String>) -> Vec<MarkWordPhonemes> {
        let path = user_dictionary(&self.core);
        lyric::with_dictionary(&path, |d| words.into_iter().map(|w| MarkWordPhonemes { phonemes: lyric::word_phonemes(d, &w), word: w }).collect())
    }

    /// F82's OK, from anywhere: `word` as it was asked about, `mapped` as the buttons wrote it.
    pub fn add_phoneme_mapping(&self, word: String, mapped: String) -> Option<MarkMessage> {
        add_mapping(&user_dictionary(&self.core), &word, &mapped)
    }

    /// A Lyric Converter window's model. One for each opening: nothing persists (F76).
    pub fn lyric_converter(&self) -> Arc<LyricConverter> {
        lipsync::warm_up();
        let session = self.session.lock();
        let counters = Arc::clone(&session.counters);
        let mut model = lyric::LyricConverter::new();
        model.refresh(session.marks());
        drop(session);
        Arc::new(LyricConverter { session: Arc::clone(&self.session), counters, user_dictionary: user_dictionary(&self.core), model: Mutex::new(model) })
    }
}

#[derive(uniffi::Object)]
pub struct LyricConverter {
    session: Arc<Mutex<SequenceSession>>,
    counters: Arc<RenderCounters>,
    user_dictionary: PathBuf,
    model: Mutex<lyric::LyricConverter>,
}

impl LyricConverter {
    fn step(&self, outcome: lyric::ConvertOutcome) -> LyricConvertStep {
        match outcome {
            lyric::ConvertOutcome::NeedsMapping(word) => {
                LyricConvertStep::NeedsMapping { title: lyric::FAILURE_TITLE.to_string(), message: lyric::failure_text(&word), word }
            }
            lyric::ConvertOutcome::Delivered(d) => LyricConvertStep::Delivered {
                count: d.clipboard.as_ref().map_or(0, |c| c.effects.len() as u32),
                clipboard_json: d.clipboard.map(|c| c.to_json()),
                placement: d.placement.into(),
                first_mark: d.first_mark.ticks(),
                clear_text: d.clear_text,
            },
            lyric::ConvertOutcome::Abandoned => LyricConvertStep::Abandoned,
        }
    }
}

#[uniffi::export]
impl LyricConverter {
    /// The sequence's edit counter, to notice marks changing under the window.
    pub fn revision(&self) -> u64 {
        self.counters.edits.load(Ordering::Relaxed)
    }

    pub fn state(&self) -> LyricConverterState {
        let m = self.model.lock();
        LyricConverterState {
            placement: m.placement().into(),
            mark_placement_available: m.mark_placement_available(),
            marks_enabled: m.marks_enabled(),
            collections: if m.marks_enabled() { m.collection_names() } else { Vec::new() },
            collection: m.collection_index().map(|i| i as u32),
            start_offsets: m.start_offsets().iter().map(|t| lyric::start_offset_text(*t)).collect(),
            start_offset: m.start_offset_index().map(|i| i as u32),
            alignment: m.alignment().map(|a| match a {
                lipsync::Alignment::Phoneme => LyricAlignment::Phoneme,
                lipsync::Alignment::Word => LyricAlignment::Word,
                lipsync::Alignment::Phrase => LyricAlignment::Phrase,
            }),
        }
    }

    /// The window came forward, or the sequence's marks changed.
    pub fn refresh(&self) {
        let session = self.session.lock();
        self.model.lock().refresh(session.marks());
    }

    pub fn set_placement(&self, placement: LyricPlacement) {
        let session = self.session.lock();
        self.model.lock().set_placement(placement.into(), session.marks());
    }

    pub fn set_collection(&self, index: u32) {
        self.model.lock().set_collection(index as usize);
    }

    pub fn set_start_offset(&self, index: u32) {
        self.model.lock().set_start_offset(index as usize);
    }

    pub fn set_alignment(&self, alignment: LyricAlignment) {
        self.model.lock().set_alignment(match alignment {
            LyricAlignment::Phoneme => lipsync::Alignment::Phoneme,
            LyricAlignment::Word => lipsync::Alignment::Word,
            LyricAlignment::Phrase => lipsync::Alignment::Phrase,
        });
    }

    /// Convert, with the selected text if any is selected and the whole box otherwise.
    pub fn convert(&self, text: String, clear_after: bool) -> LyricConvertStep {
        self.refresh();
        let outcome = lyric::with_dictionary(&self.user_dictionary, |d| self.model.lock().convert(&text, clear_after, d));
        self.step(outcome)
    }

    /// F82's OK.
    pub fn add_mapping(&self, word: String, mapped: String) -> Option<MarkMessage> {
        add_mapping(&self.user_dictionary, &word, &mapped)
    }

    /// After F82 closed, by OK or Cancel.
    pub fn resume(&self) -> LyricConvertStep {
        let outcome = lyric::with_dictionary(&self.user_dictionary, |d| self.model.lock().resume(d));
        self.step(outcome)
    }

    /// After a delivery with a clipboard: the sequence is modified whatever was pasted, and with nothing pasted there's a message (F83).
    pub fn delivered(&self, pasted: u32) -> Option<MarkMessage> {
        self.session.lock().set_modified();
        (pasted == 0).then(|| MarkMessage { title: lyric::COPIED_TITLE.to_string(), message: lyric::COPIED_MESSAGE.to_string() })
    }
}
