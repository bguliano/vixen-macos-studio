//! Beat and bar detection as the sequencer runs it: `docs/spec/marks-layers-analysis.md` F67 to F75. The analysis itself is
//! `vixen_analysis`, pinned against the C++; this runs it off the main thread, with progress and a working Cancel, and applies the result to
//! the sequence as one change.

use super::{publish, MarksChange};
use crate::sequence::SequenceSession;
use parking_lot::Mutex;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use vixen_analysis::{AnalysisError, AnalysisProgress, BeatBarPreviewData, BeatBarSettingsData, BeatsAndBars, BeatsAndBarsDialogModel, RawSampleFormat};
use vixen_audio::{AudioClip, Peaks};
use vixen_core::{MarkCollection, TimeSpan};

/// F75's alert. Upstream has none: it throws.
pub const NO_BEATS_TITLE: &str = "Beats and Bars";
pub const NO_BEATS_MESSAGE: &str = "The audio did not produce any beats or bars.";

/// How many columns of waveform the preview gets; the view scales them to its width.
pub const PREVIEW_COLUMNS: usize = 1400;

/// Where a detection has got to.
#[derive(Clone, Debug, PartialEq)]
pub enum BeatJob {
    /// F68 steps 1 to 3: the audio read and its first ten seconds analysed, with no progress to show.
    Preparing,
    /// The dialog can show (F69).
    Ready,
    /// F74. `percent` stops at 100 while the tracker's long final step runs, which is `finalizing`.
    Running { percent: i32, finalizing: bool },
    /// The collections are in the sequence, or there was no audio to analyse and only F68 step 6 ran.
    Finished,
    /// F75, or the tracker failing to start: nothing changed.
    Failed(String),
    /// Cancelled at the dialog or during the analysis. Only F68 step 6 can have changed anything.
    Cancelled,
}

struct State {
    job: BeatJob,
    analysis: Option<Arc<BeatsAndBars>>,
    preview: Option<BeatBarPreviewData>,
    model: BeatsAndBarsDialogModel,
    peaks: Vec<(f32, f32)>,
}

/// One run of Tools > Audio > Beat/Bar Detection.
pub struct BeatDetection {
    session: Arc<Mutex<SequenceSession>>,
    state: Arc<Mutex<State>>,
    cancel: Arc<AtomicBool>,
}

/// `CalcNoteSize`: the time signature's lower number for a number of beats per bar (F70).
pub fn note_size(beats_per_bar: i32) -> i32 {
    vixen_analysis::calc_note_size(beats_per_bar)
}

/// F67: the menu item is on when the sequence has any media, even if its file is missing.
pub fn detection_available(session: &SequenceSession) -> bool {
    !session.document().media.is_empty()
}

/// The first media file, which `Menu.cs` takes as the audio, in the profile's Media folder.
pub fn audio_path(session: &SequenceSession, media_dir: &Path) -> Option<PathBuf> {
    session.document().media.first().map(|m| media_dir.join(&m.file_name))
}

impl BeatDetection {
    /// Starts F68 steps 1 to 3 on a worker. With no decoded audio (channels 0 upstream), only step 6 runs and the job is finished at once.
    pub fn start(session: Arc<Mutex<SequenceSession>>, media_dir: &Path) -> BeatDetection {
        let (clip, path) = {
            let s = session.lock();
            (s.audio().cloned(), audio_path(&s, media_dir))
        };
        let detection = BeatDetection {
            session,
            state: Arc::new(Mutex::new(State {
                job: BeatJob::Preparing,
                analysis: None,
                preview: None,
                model: BeatsAndBarsDialogModel::default(),
                peaks: Vec::new(),
            })),
            cancel: Arc::new(AtomicBool::new(false)),
        };
        let (Some(clip), Some(path)) = (clip, path) else {
            detection.finish_without_analysis(BeatJob::Finished);
            return detection;
        };
        let state = Arc::clone(&detection.state);
        let cancel = Arc::clone(&detection.cancel);
        let _ = std::thread::Builder::new().name("vixen-beats-preview".to_string()).spawn(move || {
            let job = prepare(&clip, &path, &state, &cancel);
            state.lock().job = job;
        });
        detection
    }

    pub fn job(&self) -> BeatJob {
        self.state.lock().job.clone()
    }

    /// The dialog's state, for binding.
    pub fn model(&self) -> BeatsAndBarsDialogModel {
        self.state.lock().model.clone()
    }

    /// Changes the dialog's state through its own rules (F71's Split Beats link and so on).
    pub fn edit_model(&self, edit: impl FnOnce(&mut BeatsAndBarsDialogModel)) {
        edit(&mut self.state.lock().model);
    }

    /// F73: the line times and the span the preview covers. With no split marks the span stays at its first value, 1 second.
    pub fn preview_lines(&self) -> (Vec<TimeSpan>, TimeSpan) {
        let state = self.state.lock();
        let Some(preview) = &state.preview else { return (Vec::new(), TimeSpan::from_ms_i(1000)) };
        (state.model.preview_marks(preview), BeatsAndBarsDialogModel::preview_period(preview).unwrap_or(TimeSpan::from_ms_i(1000)))
    }

    /// F73's waveform over the preview span, as (min, max) per column.
    pub fn preview_peaks(&self) -> Vec<(f32, f32)> {
        self.state.lock().peaks.clone()
    }

    /// F68 step 5: the whole file, behind the progress sheet. Cancel works at any point; the sequence changes only once the features exist.
    pub fn generate(&self) {
        let (analysis, settings) = {
            let mut state = self.state.lock();
            let Some(analysis) = state.analysis.clone() else { return };
            if state.job != BeatJob::Ready || !state.model.generate_enabled() {
                return;
            }
            state.job = BeatJob::Running { percent: 0, finalizing: false };
            (analysis, state.model.settings())
        };
        let (session, state, cancel) = (Arc::clone(&self.session), Arc::clone(&self.state), Arc::clone(&self.cancel));
        let _ = std::thread::Builder::new().name("vixen-beats".to_string()).spawn(move || {
            let mut new = Vec::new();
            let progress_state = Arc::clone(&state);
            let mut progress = move |p: AnalysisProgress| {
                let mut s = progress_state.lock();
                s.job = BeatJob::Running { percent: p.value, finalizing: p.is_finalizing };
            };
            let job = match analysis.do_beat_bar_detection(&mut new, &settings, &mut progress, &cancel) {
                Ok(()) => {
                    let mut s = session.lock();
                    // STUDIO-BEGIN beats-undo: the generated collections are one step on the sequence's Undo (R40)
                    crate::studio_sequence::marks_panel::apply(&mut s, "Beats and Bars", false, |marks| apply_beat_collections(marks, new, &settings));
                    // STUDIO-END beats-undo
                    BeatJob::Finished
                }
                Err(AnalysisError::Cancelled) => {
                    default_rule(&session);
                    BeatJob::Cancelled
                }
                Err(AnalysisError::MissingOutput(_)) => BeatJob::Failed(NO_BEATS_MESSAGE.to_string()),
                Err(e) => BeatJob::Failed(e.to_string()),
            };
            state.lock().job = job;
        });
    }

    /// Cancel on the dialog or on the progress sheet.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
        let mut state = self.state.lock();
        if matches!(state.job, BeatJob::Preparing | BeatJob::Ready) {
            state.job = BeatJob::Cancelled;
            drop(state);
            default_rule(&self.session);
        }
    }

    fn finish_without_analysis(&self, job: BeatJob) {
        default_rule(&self.session);
        self.state.lock().job = job;
    }
}

/// F68 steps 1 to 3.
fn prepare(clip: &Arc<AudioClip>, path: &Path, state: &Arc<Mutex<State>>, cancel: &AtomicBool) -> BeatJob {
    // The clip came from this file, so its header reads; MP3 and others are 16-bit PCM either way.
    let format = RawSampleFormat::for_path(path).unwrap_or(RawSampleFormat::Pcm16);
    let analysis = Arc::new(BeatsAndBars::from_interleaved(clip.samples(), format, clip.sample_rate() as f32, clip.channels() as i32));
    match analysis.generate_preview_data(cancel) {
        Ok(preview) => {
            let period = BeatsAndBarsDialogModel::preview_period(&preview).unwrap_or(TimeSpan::from_ms_i(1000));
            let peaks = Peaks::compute(Arc::clone(clip)).range(TimeSpan::ZERO, period, PREVIEW_COLUMNS);
            let mut s = state.lock();
            s.model.beat_period = preview.beat_period;
            s.preview = Some(preview);
            s.analysis = Some(analysis);
            s.peaks = peaks;
            BeatJob::Ready
        }
        Err(AnalysisError::Cancelled) => BeatJob::Cancelled,
        Err(AnalysisError::MissingOutput(_)) => BeatJob::Failed(NO_BEATS_MESSAGE.to_string()),
        Err(e) => BeatJob::Failed(e.to_string()),
    }
}

/// F68 step 6 on the session.
fn default_rule(session: &Arc<Mutex<SequenceSession>>) {
    let mut s = session.lock();
    let mut marks = s.marks().to_vec();
    if super::ensure_default_collection(&mut marks) {
        publish(&mut s, marks, MarksChange::Properties);
    }
}

/// `BuildMarkCollections` and step 6 on the sequence's list, given what the analysis made from an empty one: each enabled kind's existing
/// collections of exactly those names go (KEEP: no warning, and new ids), the new ones are appended in name order, and with no default the
/// first collection becomes it. FIX (F67, for veto): reports no change when nothing changed, where upstream marks the sequence modified
/// even on Cancel.
pub fn apply_beat_collections(target: &mut Vec<MarkCollection>, mut new: Vec<MarkCollection>, settings: &BeatBarSettingsData) -> MarksChange {
    let mut doomed: Vec<String> = Vec::new();
    if settings.bars_enabled {
        doomed.push(settings.bars_collection_name());
    }
    if settings.beat_collections_enabled {
        doomed.extend(settings.beat_collection_names(false));
    }
    if settings.beat_splits_enabled {
        doomed.extend(settings.beat_collection_names(true));
    }
    if settings.all_features_enabled {
        doomed.push(settings.all_collection_name());
    }
    let before = target.len();
    target.retain(|c| !doomed.contains(&c.name));
    let removed = before != target.len();
    let added = !new.is_empty();
    for c in &mut new {
        c.is_default = false;
    }
    target.append(&mut new);
    let defaulted = super::ensure_default_collection(target);
    if removed || added {
        MarksChange::Collections
    } else if defaulted {
        MarksChange::Properties
    } else {
        MarksChange::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vixen_core::Mark;

    fn collection(name: &str, starts: &[i64]) -> MarkCollection {
        let mut c = MarkCollection { name: name.to_string(), ..MarkCollection::default() };
        c.marks = starts.iter().map(|s| Mark::new(TimeSpan::from_ms_i(*s))).collect();
        c
    }

    #[test]
    fn applying_replaces_same_named_collections_and_keeps_the_rest() {
        let settings = BeatsAndBarsDialogModel::default().settings();
        let mut target = vec![collection("Mine", &[5]), collection("Beats - All", &[1, 2]), collection("beats - all", &[3])];
        target[1].is_default = true;
        let old_id = target[1].id;
        let mut fresh = collection("Beats - All", &[10, 20]);
        fresh.is_default = true;
        let change = apply_beat_collections(&mut target, vec![fresh], &settings);
        assert_eq!(change, MarksChange::Collections);
        let names: Vec<&str> = target.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Mine", "beats - all", "Beats - All"], "case-sensitive removal, new ones appended");
        assert_ne!(target[2].id, old_id);
        // The removed default leaves none, so the first collection takes it.
        assert!(target[0].is_default && !target[2].is_default);
    }

    #[test]
    fn nothing_new_and_a_default_already_is_no_change() {
        let settings = BeatsAndBarsDialogModel::default().settings();
        let mut target = vec![collection("Mine", &[5])];
        target[0].is_default = true;
        assert_eq!(apply_beat_collections(&mut target, Vec::new(), &settings), MarksChange::None);
        let mut target = vec![collection("Mine", &[5])];
        assert_eq!(apply_beat_collections(&mut target, Vec::new(), &settings), MarksChange::Properties);
        assert!(target[0].is_default);
    }

    #[test]
    fn only_enabled_kinds_remove_their_names() {
        let mut model = BeatsAndBarsDialogModel::default();
        model.set_bars_checked(false);
        let settings = model.settings();
        let bars = settings.bars_collection_name();
        let mut target = vec![collection(&bars, &[1])];
        apply_beat_collections(&mut target, Vec::new(), &settings);
        assert_eq!(target[0].name, bars);
    }
}
