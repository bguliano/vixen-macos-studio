//! One open sequence: its effects, background rendering, schedule and undo stack. What `TimedSequenceEditorForm` keeps in
//! `_effectNodeToElement` and friends, without the window.
//!
//! All edits go through a [`SequenceSession`], which the UI holds behind a lock. Rendering happens on worker threads with no
//! lock held: an edit marks an effect dirty, a worker renders a copy of its parameters, and the result is applied only if the
//! effect hasn't changed again since. The frame thread never waits for any of this, because it only reads the latest
//! `Arc<Schedule>`.

use crate::library_links::{apply_library_links, refresh_links, Libraries};
use crate::render::{apply_load_rules, resolve_targets};
use crate::undo::{EffectPlacement, EffectSnapshot, MarkPlacement, UndoAction, UndoEntry, UndoStack};
use crate::SessionError;
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use uuid::Uuid;
use vixen_audio::AudioClip;
use vixen_core::{EffectIntents, ElementTree, MarkCollection, NodeId, TimeSpan};
use vixen_effects::framework::{AudioSource, EffectDescriptor, ParamValue, Params, TargetInfo};
use vixen_engine::render::{render_effect, EffectJob, RenderEnv};
use vixen_engine::schedule::{ElementMap, Schedule, ScheduledEffect};
use vixen_io::{load_sequence_with_store, new_sequence, save_sequence, EffectNode, ModuleStore, TimedSequence};

#[path = "sequence_video_metadata.rs"]
mod video_metadata;

/// `Mark.Duration`'s floor, which undoing a mark's times goes through.
const MARK_MIN_DURATION: TimeSpan = TimeSpan::from_ms_i(25);

/// Identifies an effect for the life of the window, across undo and redo. Unrelated to the instance id in the file.
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct EffectId(pub u64);

/// The sequence length of a file that has none (`LoadSequence`).
const DEFAULT_LENGTH: TimeSpan = TimeSpan(60 * 10_000_000);

/// An effect in an open sequence.
pub struct EffectEntry {
    pub id: EffectId,
    pub node: EffectNode,
    /// The element nodes `node.targets` resolves to here: empty on a deprecated row, whose element has gone.
    pub targets: Vec<NodeId>,
    /// `None` for a module this version doesn't have. It renders nothing and is saved back unchanged.
    pub descriptor: Option<&'static EffectDescriptor>,
    /// The last render, even if newer changes are waiting for one.
    pub intents: Option<Arc<EffectIntents>>,
    /// Bumps with every change that needs a new render.
    pub generation: u64,
    /// The generation `intents` was rendered from.
    pub rendered_generation: u64,
}

impl EffectEntry {
    pub fn is_rendered(&self) -> bool {
        self.rendered_generation == self.generation
    }

    pub fn end(&self) -> TimeSpan {
        self.node.start + self.node.time_span
    }
}

/// What every render of this sequence reads. Replaced as a whole when any of it changes.
#[derive(Clone)]
struct RenderShared {
    video_cache: Option<Arc<vixen_video::cache::CacheSession>>,
    tree: Arc<ElementTree>,
    marks: Arc<Vec<MarkCollection>>,
    audio: Option<Arc<AudioClip>>,
    module_data_root: Option<Arc<Path>>,
    frame_ms: i32,
}

struct RenderTask {
    instance_id: Uuid,
    id: EffectId,
    generation: u64,
    descriptor: &'static EffectDescriptor,
    params: Params,
    targets: Vec<NodeId>,
    start: TimeSpan,
    span: TimeSpan,
    refresh_video_metadata: bool,
}

struct RenderBatch {
    shared: RenderShared,
    tasks: Vec<RenderTask>,
}

/// How far rendering has got, readable with no lock for the status bar.
#[derive(Default)]
pub struct RenderCounters {
    pub queued: AtomicUsize,
    pub finished: AtomicUsize,
    pub failed: AtomicUsize,
    /// Bumps each time finished renders are applied, so the UI knows when to redraw.
    pub revision: AtomicU64,
    /// Bumps when effect/mark properties change, including derived display metadata.
    /// Dirty state is tracked separately by `SequenceSession::modified`.
    pub edits: AtomicU64,
}

/// Called with each schedule the session produces. Playback hands this to the engine.
pub type ScheduleListener = Box<dyn Fn(Arc<Schedule>) + Send + Sync>;

/// An open sequence.
pub struct SequenceSession {
    video_cache: Option<vixen_video::cache::CacheLease>,
    /// FileName setter requests survive an intervening Undo until that file's
    /// metadata has actually been applied. ModuleData loads create no request.
    video_metadata_requests: HashMap<EffectId, HashSet<String>>,
    /// Runtime identity, independent of file names and stable for this open document.
    playback_id: Uuid,
    /// The document, with `effects` moved out into the table and put back to save.
    document: TimedSequence,
    path: Option<PathBuf>,
    modified: bool,

    effects: HashMap<EffectId, EffectEntry>,
    /// File order, so a save doesn't shuffle the document.
    order: Vec<EffectId>,
    next_id: u64,

    shared: RenderShared,
    elements: Arc<ElementMap>,
    schedule: Arc<Schedule>,
    listener: Option<ScheduleListener>,

    undo: UndoStack,
    /// What linked curves and gradients read from.
    libraries: Arc<Libraries>,

    /// The render worker's queue. Closed when the session drops, which ends the worker.
    render_queue: Option<mpsc::Sender<RenderBatch>>,
    /// Gets a message after each finished batch, until someone takes it.
    finished_batches: Option<mpsc::Receiver<()>>,
    results: Arc<Mutex<Vec<RenderResult>>>,
    // STUDIO-BEGIN render-queue-dedupe: never queue a render twice for one generation; rapid edits re-queued every pending effect each time
    /// The generation each effect was last queued at, until its result is applied. `render_all_dirty` skips one already on its way.
    queued_generations: std::collections::HashMap<EffectId, u64>,
    // STUDIO-END render-queue-dedupe
    pub counters: Arc<RenderCounters>,
    effect_defaults: Option<Arc<Mutex<vixen_io::effect_defaults::EffectDefaultsStore>>>,
    problems: Arc<Mutex<Vec<String>>>,
    /// Set to abandon renders in progress when the session closes.
    cancel: Arc<AtomicBool>,
    /// See [`UndoAction::RowsRestored`].
    rows_restored: Vec<EffectId>,
    pub(crate) studio_agent_flash: crate::studio_agent_flash::AgentFlash, // STUDIO: R39, the effects agent edits touched
}

struct RenderResult {
    id: EffectId,
    generation: u64,
    intents: Arc<EffectIntents>,
    video_metadata: Option<vixen_video::VideoInfo>,
}

/// What opening or creating a session needs from the profile.
#[derive(Clone)]
pub struct SessionEnvironment {
    pub video_cache: Option<Arc<vixen_video::cache::VideoCache>>,
    pub tree: Arc<ElementTree>,
    pub store: Arc<ModuleStore>,
    pub module_data_root: Option<PathBuf>,
    pub frame_ms: i32,
    /// Folder the sequence's media files are in.
    pub media_dir: PathBuf,
}

impl SequenceSession {
    fn update_cache_live_effects(&self) {
        if let Some(cache) = &self.video_cache { cache.set_live_effects(self.effects.values().map(|entry| entry.node.instance_id)); }
    }

    /// Editor close, after Save/Discard has been resolved. Removed effects remain
    /// cache owners for Undo until here, and late workers cannot republish afterward.
    pub fn close_video_cache(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.render_queue = None;
        self.update_cache_live_effects();
        self.video_cache.take();
    }

    /// A prerendered non-editor sequence owns resources until its playback/run ends.
    pub fn take_video_cache_lease(&mut self) -> Option<vixen_video::cache::CacheLease> {
        self.update_cache_live_effects();
        self.video_cache.take()
    }

    /// A new, unsaved sequence. It starts out modified, as in the original.
    pub fn new_untitled(env: &SessionEnvironment) -> SequenceSession {
        let mut session = SequenceSession::from_document(new_sequence(), None, env);
        session.modified = true;
        session
    }

    /// A converted legacy sequence opens unsaved, with the normal render/audio lifecycle.
    pub fn imported(document: TimedSequence, env: &SessionEnvironment) -> SequenceSession {
        let mut session = Self::from_document(document, None, env);
        session.modified = true;
        session.load_audio(env);
        session.render_all_dirty();
        session
    }

    /// Opens a sequence file. Decoding its audio and rendering its effects continue in the background.
    pub fn open(path: &Path, env: &SessionEnvironment) -> Result<SequenceSession, SessionError> {
        Self::open_cancellable(path, env, &AtomicBool::new(false))
    }

    /// Existing background operations can stop media decoding while opening.
    /// XML decoding and legacy file copying remain synchronous between checks.
    pub fn open_cancellable(path: &Path, env: &SessionEnvironment, cancel: &AtomicBool) -> Result<SequenceSession, SessionError> {
        Self::check_load_cancel(cancel)?;
        let mut document = load_sequence_with_store(path, &env.store)?;
        Self::check_load_cancel(cancel)?;
        document.restore_migrated_media(&env.media_dir)?;
        Self::check_load_cancel(cancel)?;
        apply_library_links(&mut document, &env.store);
        apply_load_rules(&mut document, &env.tree);
        let mut session = SequenceSession::from_document(document, Some(path.to_path_buf()), env);
        session.load_audio_cancellable(env, cancel)?;
        session.render_all_dirty();
        Ok(session)
    }

    fn check_load_cancel(cancel: &AtomicBool) -> Result<(), SessionError> {
        if cancel.load(Ordering::Acquire) {
            Err(SessionError::Other("Sequence loading was cancelled.".into()))
        } else { Ok(()) }
    }

    fn from_document(mut document: TimedSequence, path: Option<PathBuf>, env: &SessionEnvironment) -> SequenceSession {
        if document.length <= TimeSpan::ZERO {
            document.length = DEFAULT_LENGTH;
        }
        let video_cache = env.video_cache.as_ref().map(|cache| cache.session());
        if let Some(cache) = &video_cache { cache.set_live_effects(document.effects.iter().map(|node| node.instance_id)); }
        let shared = RenderShared {
            video_cache: video_cache.as_ref().map(|lease| lease.session().clone()),
            tree: Arc::clone(&env.tree),
            marks: Arc::new(document.marks.clone()),
            audio: None,
            module_data_root: env.module_data_root.as_deref().map(Arc::from),
            frame_ms: env.frame_ms,
        };
        let elements = Arc::new(ElementMap::from_tree(&env.tree));
        let schedule = Arc::new(Schedule::empty(Arc::clone(&elements), document.layers.layers.clone()));

        let mut effects = HashMap::with_capacity(document.effects.len());
        let mut order = Vec::with_capacity(document.effects.len());
        let mut next_id = 1;
        for node in std::mem::take(&mut document.effects) {
            let id = EffectId(next_id);
            next_id += 1;
            let targets = resolve_targets(&env.tree, &node);
            let descriptor = vixen_effect_registry::by_type_id(&node.type_id);
            effects.insert(id, EffectEntry { id, node, targets, descriptor, intents: None, generation: 1, rendered_generation: 0 });
            order.push(id);
        }

        let counters = Arc::new(RenderCounters::default());
        let results = Arc::new(Mutex::new(Vec::new()));
        let problems = Arc::new(Mutex::new(Vec::new()));
        let cancel = Arc::new(AtomicBool::new(false));
        let (finished_sender, finished_batches) = mpsc::channel();
        let render_queue =
            spawn_render_worker(Arc::clone(&results), Arc::clone(&counters), Arc::clone(&problems), Arc::clone(&cancel), finished_sender);

        SequenceSession {
            video_cache,
            video_metadata_requests: HashMap::new(),
            playback_id: Uuid::new_v4(),
            document,
            path,
            modified: false,
            effects,
            order,
            next_id,
            shared,
            elements,
            schedule,
            listener: None,
            undo: UndoStack::default(),
            libraries: Arc::new(Libraries::from_store(&env.store)),
            render_queue: Some(render_queue),
            finished_batches: Some(finished_batches),
            results,
            queued_generations: Default::default(), // STUDIO: render-queue dedupe
            counters,
            effect_defaults: None,
            problems,
            cancel,
            rows_restored: Vec::new(),
            studio_agent_flash: Default::default(), // STUDIO: R39
        }
    }

    fn load_audio(&mut self, env: &SessionEnvironment) {
        let _ = self.load_audio_cancellable(env, &AtomicBool::new(false));
    }

    fn load_audio_cancellable(&mut self, env: &SessionEnvironment, cancel: &AtomicBool) -> Result<(), SessionError> {
        // `Media` names files in the profile's Media folder. The first that decodes is the sequence's audio.
        for media in &self.document.media {
            Self::check_load_cancel(cancel)?;
            let path = env.media_dir.join(&media.file_name);
            match AudioClip::load_cancellable(&path, cancel) {
                Ok(clip) => {
                    self.shared.audio = Some(Arc::new(clip));
                    break;
                }
                Err(vixen_audio::AudioError::Cancelled) => return Err(SessionError::Other("Sequence loading was cancelled.".into())),
                Err(error) => self.problems.lock().push(format!("Couldn't load {}: {error}", media.file_name)),
            }
        }
        Self::check_load_cancel(cancel)
    }

    // ------------------------------------------------------------------------------------------------------------------
    // The document
    // ------------------------------------------------------------------------------------------------------------------

    pub fn document(&self) -> &TimedSequence {
        &self.document
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// The file name without its extension, or "Untitled".
    pub fn name(&self) -> String {
        self.path.as_deref().and_then(Path::file_stem).map_or_else(|| "Untitled".to_string(), |n| n.to_string_lossy().into_owned())
    }

    pub fn is_modified(&self) -> bool {
        self.modified
    }

    pub fn playback_id(&self) -> Uuid {
        self.playback_id
    }

    /// Every edit comes through here: the document is modified, and windows watching `counters.edits` can tell.
    fn touch(&mut self) {
        self.modified = true;
        self.counters.edits.fetch_add(1, Ordering::Relaxed);
    }

    pub fn set_modified(&mut self) {
        self.touch();
    }

    /// `SequenceNotModified`: a sequence with a file opens unmodified, whatever loading had to correct.
    pub fn set_unmodified(&mut self) {
        self.modified = false;
    }

    pub fn length(&self) -> TimeSpan {
        self.document.length
    }

    /// `Tools > Sequence Length`: no undo, as in the original.
    pub fn set_length(&mut self, length: TimeSpan) {
        self.document.length = length;
        self.touch();
    }

    pub fn audio(&self) -> Option<&Arc<AudioClip>> {
        self.shared.audio.as_ref()
    }

    /// Writes the sequence, to `path` or to where it came from. Row settings and view state are set on the document before
    /// this is called.
    pub fn save(&mut self, path: Option<&Path>) -> Result<(), SessionError> {
        let target = path.map(Path::to_path_buf).or_else(|| self.path.clone());
        let Some(target) = target else {
            return Err(SessionError::Other("This sequence has no file yet.".to_string()));
        };
        self.document.effects = self.order.iter().filter_map(|id| self.effects.get(id).map(|e| e.node.clone())).collect();
        let result = save_sequence(&self.document, &target);
        self.document.effects = Vec::new();
        result?;
        self.path = Some(target);
        self.modified = false;
        Ok(())
    }

    /// The document as a save would write it, to write away from the lock. Edits made meanwhile set `modified` again, because this clears it.
    pub fn begin_save(&mut self) -> TimedSequence {
        let mut document = self.document.clone();
        document.effects = self.order.iter().filter_map(|id| self.effects.get(id).map(|e| e.node.clone())).collect();
        self.modified = false;
        document
    }

    /// After writing what `begin_save` gave: the sequence now lives at `path`, or it's still modified if the write failed.
    pub fn finish_save(&mut self, path: Option<PathBuf>, succeeded: bool) {
        if succeeded {
            if let Some(path) = path {
                self.path = Some(path);
            }
        } else {
            self.modified = true;
        }
    }

    /// `FilePath = String.Empty`: a foreign sequence asks for a name at its next save. F12.
    pub fn clear_path(&mut self) {
        self.path = None;
    }

    /// The audio the sequence plays and effects read, replaced when the association changes.
    pub fn set_audio_clip(&mut self, clip: Option<Arc<AudioClip>>) {
        self.shared.audio = clip;
    }

    /// Direct access for the view state the timeline stores in the document (row settings, zoom, playback range).
    pub fn document_mut(&mut self) -> &mut TimedSequence {
        &mut self.document
    }

    // ------------------------------------------------------------------------------------------------------------------
    // Effects
    // ------------------------------------------------------------------------------------------------------------------

    pub fn effect(&self, id: EffectId) -> Option<&EffectEntry> {
        self.effects.get(&id)
    }

    /// Effects in document order.
    pub fn effects(&self) -> impl Iterator<Item = &EffectEntry> {
        self.order.iter().filter_map(|id| self.effects.get(id))
    }

    pub fn effect_count(&self) -> usize {
        self.effects.len()
    }

    pub fn target_info(&self, id: EffectId) -> Option<TargetInfo<'_>> {
        let e = self.effects.get(&id)?;
        Some(self.info_for(&e.targets, e.node.start, e.node.time_span))
    }

    fn info_for<'a>(&'a self, targets: &'a [NodeId], start: TimeSpan, span: TimeSpan) -> TargetInfo<'a> {
        TargetInfo {
            tree: &self.shared.tree,
            targets,
            marks: &self.document.marks,
            start_time: start,
            time_span: span,
            audio_loaded: self.shared.audio.is_some(),
        }
    }

    /// Where new effects read saved defaults from: the open profile's store. Without one they start from the built-in values.
    pub fn set_effect_defaults(&mut self, store: Arc<Mutex<vixen_io::effect_defaults::EffectDefaultsStore>>) {
        self.effect_defaults = Some(store);
    }

    /// Creates an effect and adds it with no undo record: `CreateEffectInstanceWithDefaults` followed by `AddEffectNode`.
    /// The caller records one undo action for however many it adds.
    pub fn add_new_effect(&mut self, type_id: Uuid, target: Uuid, target_name: &str, start: TimeSpan, span: TimeSpan) -> Option<EffectId> {
        let descriptor = vixen_effect_registry::by_type_id(&type_id)?;
        let mut params = self.effect_defaults.as_ref().and_then(|store| store.lock().create_params(descriptor)).unwrap_or_else(|| descriptor.new_params());
        let targets: Vec<NodeId> = self.shared.tree.find(&target).into_iter().collect();
        let info = self.info_for(&targets, start, span);
        descriptor.set_targets(&mut params, &info);
        // `AddEffectInstance` assigns the mark collections after the targets.
        descriptor.set_marks(&mut params, &info);
        let node = EffectNode {
            start,
            time_span: span,
            type_id,
            instance_id: Uuid::new_v4(),
            targets: vec![(target, target_name.to_string())],
            params,
        };
        Some(self.insert_node(node, targets, descriptor))
    }

    /// Adds a copy of an existing effect's data under a new instance id, at a new place: paste and clone.
    pub fn add_copy(&mut self, mut node: EffectNode, target: Uuid, target_name: &str, start: TimeSpan, span: TimeSpan) -> Option<EffectId> {
        let descriptor = vixen_effect_registry::by_type_id(&node.type_id)?;
        let targets: Vec<NodeId> = self.shared.tree.find(&target).into_iter().collect();
        node.instance_id = Uuid::new_v4();
        node.start = start;
        node.time_span = span;
        node.targets = vec![(target, target_name.to_string())];
        let mut params = node.params.clone();
        let info = self.info_for(&targets, start, span);
        descriptor.set_targets(&mut params, &info);
        descriptor.set_marks(&mut params, &info);
        node.params = params;
        Some(self.insert_node(node, targets, descriptor))
    }

    fn insert_node(&mut self, node: EffectNode, targets: Vec<NodeId>, descriptor: &'static EffectDescriptor) -> EffectId {
        let id = EffectId(self.next_id);
        self.next_id += 1;
        self.effects.insert(
            id,
            EffectEntry { id, node, targets, descriptor: Some(descriptor), intents: None, generation: 1, rendered_generation: 0 },
        );
        self.order.push(id);
        self.update_cache_live_effects();
        self.touch();
        self.render_all_dirty();
        id
    }

    /// `RemoveEffectNodeAndElement`, with no undo record.
    pub fn remove_effects(&mut self, ids: &[EffectId]) -> Vec<EffectSnapshot> {
        let mut snapshots = Vec::with_capacity(ids.len());
        let mut removed_instances = Vec::with_capacity(ids.len());
        for id in ids {
            let Some(entry) = self.effects.remove(id) else { continue };
            self.queued_generations.remove(id); // STUDIO: render-queue dedupe
            let layer = self.document.layers.effect_layer_map.remove(&entry.node.instance_id);
            removed_instances.push(entry.node.instance_id);
            snapshots.push(EffectSnapshot {
                id: *id,
                position: self.order.iter().position(|o| o == id),
                node: entry.node,
                layer,
                intents: entry.intents,
                generation: entry.generation,
                rendered_generation: entry.rendered_generation,
            });
        }
        let gone: HashSet<EffectId> = ids.iter().copied().collect();
        self.order.retain(|id| !gone.contains(id));
        self.update_cache_live_effects();
        if !removed_instances.is_empty() {
            self.touch();
            self.replace_schedule(self.schedule.with_changes(&removed_instances, &[]));
        }
        snapshots
    }

    fn restore_effects(&mut self, snapshots: Vec<EffectSnapshot>) {
        let mut added = Vec::new();
        for s in snapshots {
            let targets = resolve_targets(&self.shared.tree, &s.node);
            let descriptor = vixen_effect_registry::by_type_id(&s.node.type_id);
            if let Some(layer) = s.layer {
                self.document.layers.effect_layer_map.insert(s.node.instance_id, layer);
            }
            // Every pre-delete job belongs to a retired incarnation, including
            // generation 1. Cached intents are complete only if they covered the
            // snapshot's current parameters when removed.
            let generation = s.generation + 1;
            let cached_complete = s.intents.is_some() && s.rendered_generation == s.generation;
            let entry = EffectEntry {
                id: s.id,
                targets,
                descriptor,
                rendered_generation: if cached_complete { generation } else { s.rendered_generation },
                generation,
                intents: s.intents,
                node: s.node,
            };
            if entry.is_rendered() {
                added.push(self.scheduled(&entry));
            }
            let at = s.position.unwrap_or(self.order.len()).min(self.order.len());
            self.order.insert(at, s.id);
            self.effects.insert(s.id, entry);
        }
        self.update_cache_live_effects();
        self.touch();
        if !added.is_empty() {
            self.replace_schedule(self.schedule.with_changes(&[], &added));
        }
        self.render_all_dirty();
    }

    /// Moves or resizes effects with no undo record. A drag calls this as it goes, then records one action at the end. The
    /// render stays until the drag finishes, when `render_all_dirty` starts the new one.
    pub fn set_times(&mut self, changes: &[(EffectId, TimeSpan, TimeSpan)]) {
        let mut replaced = Vec::with_capacity(changes.len());
        let mut added = Vec::with_capacity(changes.len());
        for (id, start, span) in changes {
            let Some(entry) = self.effects.get_mut(id) else { continue };
            if entry.node.start == *start && entry.node.time_span == *span {
                continue;
            }
            entry.node.start = *start;
            entry.node.time_span = *span;
            // `EffectModuleInstanceBase.StartTime` and `TimeSpan` mark the effect dirty, whatever it does with them.
            entry.generation += 1;
            replaced.push(entry.node.instance_id);
        }
        for (id, ..) in changes {
            if let Some(entry) = self.effects.get(id) {
                if entry.intents.is_some() && replaced.contains(&entry.node.instance_id) {
                    added.push(self.scheduled(entry));
                }
            }
        }
        if !replaced.is_empty() {
            self.touch();
            self.replace_schedule(self.schedule.with_changes(&replaced, &added));
        }
    }

    /// Moves effects to other elements: dragging between rows.
    pub fn set_target(&mut self, id: EffectId, target: Uuid, name: &str) {
        let tree = Arc::clone(&self.shared.tree);
        let marks = Arc::clone(&self.shared.marks);
        let audio_loaded = self.shared.audio.is_some();
        let Some(entry) = self.effects.get_mut(&id) else { return };
        entry.node.targets = vec![(target, name.to_string())];
        entry.targets = tree.find(&target).into_iter().collect();
        if let Some(descriptor) = entry.descriptor {
            let info = TargetInfo {
                tree: &tree,
                targets: &entry.targets,
                marks: &marks,
                start_time: entry.node.start,
                time_span: entry.node.time_span,
                audio_loaded,
            };
            descriptor.set_targets(&mut entry.node.params, &info);
        }
        entry.generation += 1;
        self.touch();
    }

    /// Sets one effect parameter as its C# property setter does, with no undo record.
    pub fn set_param(&mut self, id: EffectId, name: &str, value: ParamValue) {
        let tree = Arc::clone(&self.shared.tree);
        let marks = Arc::clone(&self.shared.marks);
        let audio_loaded = self.shared.audio.is_some();
        let Some(entry) = self.effects.get_mut(&id) else { return };
        let Some(descriptor) = entry.descriptor else { return };
        let info = TargetInfo {
            tree: &tree,
            targets: &entry.targets,
            marks: &marks,
            start_time: entry.node.start,
            time_span: entry.node.time_span,
            audio_loaded,
        };
        descriptor.set_param(&mut entry.node.params, name, value, &info);
        if descriptor.name == "Video" && name == "FileName" {
            self.video_metadata_requests.entry(id).or_default().insert(entry.node.params.string("FileName").to_string());
        }
        entry.generation += 1;
        self.touch();
    }

    /// A file chooser can replace the file but adopt the same basename. Refresh
    /// its computed information without inventing another property/Undo change.
    pub(crate) fn refresh_video_metadata(&mut self, id: EffectId) {
        let Some(entry) = self.effects.get_mut(&id) else { return };
        if !entry.descriptor.is_some_and(|descriptor| descriptor.name == "Video") { return; }
        self.video_metadata_requests.entry(id).or_default().insert(entry.node.params.string("FileName").to_string());
        entry.generation += 1;
    }

    /// Replaces an effect's whole parameter set: paste-parameters and undo.
    pub fn set_params(&mut self, id: EffectId, params: Params) {
        let Some(entry) = self.effects.get_mut(&id) else { return };
        entry.node.params = params;
        entry.generation += 1;
        self.touch();
    }

    /// `AssignEffect`: moves effects to a layer, with no undo record.
    pub fn set_layer(&mut self, ids: &[EffectId], layer: Uuid) {
        let mut replaced = Vec::with_capacity(ids.len());
        for id in ids {
            let Some(entry) = self.effects.get(id) else { continue };
            self.document.layers.assign_effect(entry.node.instance_id, layer);
            replaced.push(entry.node.instance_id);
        }
        let added: Vec<ScheduledEffect> = ids
            .iter()
            .filter_map(|id| self.effects.get(id))
            .filter(|e| e.intents.is_some())
            .map(|e| self.scheduled(e))
            .collect();
        if !replaced.is_empty() {
            self.touch();
            self.replace_schedule(self.schedule.with_changes(&replaced, &added));
        }
    }

    pub fn layer_of(&self, id: EffectId) -> Option<Uuid> {
        let entry = self.effects.get(&id)?;
        let index = self.document.layers.layer_index_for_effect(&entry.node.instance_id);
        self.document.layers.layers.get(index).map(|l| l.id)
    }

    // ------------------------------------------------------------------------------------------------------------------
    // Marks and layers
    // ------------------------------------------------------------------------------------------------------------------

    pub fn marks(&self) -> &[MarkCollection] {
        &self.document.marks
    }

    /// Replaces the marks, for code that has changed them. Effects that read marks render again: `MarkCollectionsChanged`.
    pub fn set_marks(&mut self, marks: Vec<MarkCollection>) {
        self.document.marks = marks;
        self.shared.marks = Arc::new(self.document.marks.clone());
        self.touch();
        self.mark_effects_using_marks_dirty();
        self.render_all_dirty();
    }

    /// Puts back the marks a `MarksChanged` holds, collection by collection and only their marks: a collection added since (the Mark
    /// Collections panel has no undo) stays, and so do names, flags and links. Collections are matched by id, the k-th saved one with an id to
    /// the k-th with that id now, since an imported `.v3m` can repeat one. Ids gone since are skipped. Returns the same for what it replaced,
    /// which reverses it.
    pub fn apply_marks_snapshot(&mut self, snapshot: &[MarkCollection]) -> Vec<MarkCollection> {
        let mut marks = self.document.marks.clone();
        let mut reverse = Vec::with_capacity(snapshot.len());
        let mut seen: HashMap<Uuid, usize> = HashMap::new();
        for saved in snapshot {
            let nth = seen.entry(saved.id).or_insert(0);
            let found = marks.iter_mut().filter(|c| c.id == saved.id).nth(*nth);
            *nth += 1;
            if let Some(c) = found {
                let old = std::mem::take(&mut c.marks);
                let mut entry = c.clone();
                entry.marks = old;
                c.marks = saved.marks.clone();
                reverse.push(entry);
            }
        }
        self.set_marks(marks);
        reverse
    }

    /// Takes marks as they are with nothing else following: not modified, nothing rendered again. For the one path upstream leaves a mark changed
    /// without saying so (the timeline's Q43).
    pub fn set_marks_quietly(&mut self, marks: Vec<MarkCollection>) {
        self.document.marks = marks;
        self.shared.marks = Arc::new(self.document.marks.clone());
    }

    /// After an undo changed marks in place: back in `EnsureOrder`'s order, and effects that read marks render again.
    fn marks_restored(&mut self) {
        for c in &mut self.document.marks {
            c.marks.sort_by_key(|m| (m.start_time, m.end_time()));
        }
        self.shared.marks = Arc::new(self.document.marks.clone());
        self.mark_effects_using_marks_dirty();
    }

    /// Every effect an undo has put back on its row, in order, for a window to follow with its own place in the list.
    pub fn rows_restored(&self) -> &[EffectId] {
        &self.rows_restored
    }

    /// `MarkCollectionsChanged`: each effect sees the new list, and those that read marks render again.
    // STUDIO-BEGIN marks-panel-params: what a Mark Collections panel undo step keeps of the effects that read marks
    /// The parameters of every effect that reads mark collections: deleting a collection clears their references to it, and Undo puts them back.
    pub fn studio_mark_reader_params(&self) -> Vec<(EffectId, Params)> {
        self.effects
            .iter()
            .filter(|(_, e)| e.descriptor.is_some_and(|d| reads_marks(d.params)))
            .map(|(id, e)| (*id, e.node.params.clone()))
            .collect()
    }
    // STUDIO-END marks-panel-params

    fn mark_effects_using_marks_dirty(&mut self) {
        let tree = Arc::clone(&self.shared.tree);
        let marks = Arc::clone(&self.shared.marks);
        let audio_loaded = self.shared.audio.is_some();
        for entry in self.effects.values_mut() {
            let Some(descriptor) = entry.descriptor else { continue };
            let info = TargetInfo {
                tree: &tree,
                targets: &entry.targets,
                marks: &marks,
                start_time: entry.node.start,
                time_span: entry.node.time_span,
                audio_loaded,
            };
            descriptor.set_marks(&mut entry.node.params, &info);
            if reads_marks(descriptor.params) {
                entry.generation += 1;
            }
        }
    }

    /// Call after changing `document_mut().layers`: mixing is part of the schedule.
    pub fn layers_changed(&mut self) {
        self.touch();
        self.replace_schedule(self.schedule.with_layers(self.document.layers.layers.clone()));
    }

    // ------------------------------------------------------------------------------------------------------------------
    // Rendering and the schedule
    // ------------------------------------------------------------------------------------------------------------------

    pub fn schedule(&self) -> Arc<Schedule> {
        Arc::clone(&self.schedule)
    }

    /// Playback registers here to get each new schedule.
    pub fn set_schedule_listener(&mut self, listener: Option<ScheduleListener>) {
        if let Some(listener) = &listener {
            listener(Arc::clone(&self.schedule));
        }
        self.listener = listener;
    }

    fn replace_schedule(&mut self, schedule: Schedule) {
        self.schedule = Arc::new(schedule);
        if let Some(listener) = &self.listener {
            listener(Arc::clone(&self.schedule));
        }
    }

    pub(crate) fn scheduled(&self, entry: &EffectEntry) -> ScheduledEffect {
        let layer = self.document.layers.layer_index_for_effect(&entry.node.instance_id) as u16;
        ScheduledEffect {
            id: entry.node.instance_id,
            start: entry.node.start,
            span: entry.node.time_span,
            layer,
            intents: entry.intents.clone().unwrap_or_default(),
        }
    }

    /// Display Setup's OK changed the display: every effect resolves its target again, a deleted element's effect goes dark, and everything renders against the new tree.
    /// The sequence itself hasn't changed, so this doesn't mark it modified.
    pub fn display_changed(&mut self, env: &SessionEnvironment) {
        self.shared.tree = Arc::clone(&env.tree);
        self.shared.frame_ms = env.frame_ms;
        self.elements = Arc::new(ElementMap::from_tree(&env.tree));
        for entry in self.effects.values_mut() {
            entry.targets = resolve_targets(&env.tree, &entry.node);
            entry.intents = None;
            entry.generation += 1;
        }
        self.replace_schedule(Schedule::empty(Arc::clone(&self.elements), self.document.layers.layers.clone()));
        self.render_all_dirty();
    }

    /// Element properties changed on the profile with the nodes themselves as they were, such as Face Setup from the Tools menu: later
    /// renders read them. Nothing renders again here; the caller knows which effects read what changed.
    pub fn element_properties_changed(&mut self, tree: Arc<ElementTree>) {
        self.shared.tree = tree;
    }

    /// Sends every effect that has changed to the render workers.
    pub fn render_all_dirty(&mut self) {
        let mut tasks = Vec::new();
        let libraries = Arc::clone(&self.libraries);
        for entry in self.effects.values_mut() {
            if entry.is_rendered() {
                continue;
            }
            let Some(descriptor) = entry.descriptor else { continue };
            // STUDIO-BEGIN render-queue-skip: already queued at this generation, its result is on its way
            if self.queued_generations.insert(entry.id, entry.generation) == Some(entry.generation) {
                continue;
            }
            // STUDIO-END render-queue-skip
            // A paste or an undo can bring back a copy older than the library. The original reads the library as it renders.
            refresh_links(&mut entry.node.params, &libraries);
            tasks.push(RenderTask {
                instance_id: entry.node.instance_id,
                id: entry.id,
                generation: entry.generation,
                descriptor,
                params: entry.node.params.clone(),
                targets: entry.targets.clone(),
                start: entry.node.start,
                span: entry.node.time_span,
                refresh_video_metadata: descriptor.name == "Video" &&
                    (entry.node.params.int("VideoQuality") == 0 || self.video_metadata_requests.get(&entry.id)
                        .is_some_and(|files| files.contains(entry.node.params.string("FileName")))),
            });
        }
        if tasks.is_empty() {
            return;
        }
        self.counters.queued.fetch_add(tasks.len(), Ordering::Relaxed);
        if let Some(queue) = &self.render_queue {
            let _ = queue.send(RenderBatch { shared: self.shared.clone(), tasks });
        }
    }

    /// `CurvesChanged` and `GradientsChanged`: effects linked to an entry that changed render again. As in the original, this
    /// doesn't count as editing the sequence.
    pub fn libraries_changed(&mut self, libraries: Libraries) {
        self.libraries = Arc::new(libraries);
        let libraries = Arc::clone(&self.libraries);
        let mut any = false;
        for entry in self.effects.values_mut() {
            if refresh_links(&mut entry.node.params, &libraries) {
                entry.generation += 1;
                any = true;
            }
        }
        if any {
            self.counters.edits.fetch_add(1, Ordering::Relaxed);
            self.render_all_dirty();
        }
    }

    /// Something outside the sequence that these effects draw from has changed, such as a LipSync image map: each one is marked dirty and
    /// rendered again. Returns how many. The caller decides whether the sequence counts as modified.
    pub fn rerender_effects_where(&mut self, mut matches: impl FnMut(&mut EffectEntry) -> bool) -> usize {
        let mut count = 0;
        for entry in self.effects.values_mut() {
            if matches(entry) {
                entry.generation += 1;
                count += 1;
            }
        }
        if count > 0 {
            self.counters.edits.fetch_add(1, Ordering::Relaxed);
            self.render_all_dirty();
        }
        count
    }

    /// Effects still waiting for a render.
    pub fn unrendered_count(&self) -> usize {
        self.effects.values().filter(|e| !e.is_rendered() && e.descriptor.is_some()).count()
    }

    /// Applies finished renders, returning how many took effect. Called from the UI's refresh and by playback.
    pub fn apply_finished_renders(&mut self) -> usize {
        let finished = std::mem::take(&mut *self.results.lock());
        if finished.is_empty() {
            return 0;
        }
        let mut replaced = Vec::with_capacity(finished.len());
        let mut changed = Vec::with_capacity(finished.len());
        let mut metadata_changed = false;
        for result in finished {
            let Some(entry) = self.effects.get_mut(&result.id) else { continue };
            // Changed again since this render started: another is on its way.
            if entry.generation != result.generation {
                continue;
            }
            // STUDIO-BEGIN render-result-dedupe: a second result for a generation already applied would duplicate its schedule entries
            if entry.rendered_generation == result.generation {
                continue;
            }
            if self.queued_generations.get(&result.id) == Some(&result.generation) {
                self.queued_generations.remove(&result.id);
            }
            // STUDIO-END render-result-dedupe
            entry.intents = Some(result.intents);
            entry.rendered_generation = result.generation;
            if entry.descriptor.is_some_and(|descriptor| descriptor.name == "Video") {
                if let Some(info) = result.video_metadata {
                    metadata_changed |= video_metadata::apply(&mut entry.node.params, info);
                    if let Some(files) = self.video_metadata_requests.get_mut(&result.id) {
                        files.remove(entry.node.params.string("FileName"));
                    }
                }
                if let Some(cache) = &self.shared.video_cache {
                    let size = cache.cache_size(entry.node.instance_id);
                    let size = format!("{:.2} MB", size as f64 / 1_048_576.0);
                    if entry.node.params.string("CacheSize") != size {
                        entry.node.params.set("CacheSize", ParamValue::Str(size));
                        metadata_changed = true;
                    }
                }
            }
            replaced.push(entry.node.instance_id);
            changed.push(result.id);
        }
        if changed.is_empty() {
            return 0;
        }
        let added: Vec<ScheduledEffect> = changed.iter().filter_map(|id| self.effects.get(id)).map(|e| self.scheduled(e)).collect();
        self.replace_schedule(self.schedule.with_changes(&replaced, &added));
        // The editor polls this counter while its selection remains unchanged.
        // Refresh its read-only label without making a user edit or rerendering.
        if metadata_changed {
            self.counters.edits.fetch_add(1, Ordering::Relaxed);
        }
        self.counters.revision.fetch_add(1, Ordering::Relaxed);
        changed.len()
    }

    /// A channel with a message after each finished batch, for whoever holds the session's lock to call
    /// `apply_finished_renders`. It closes when the session drops. There's only one, so this returns it once.
    pub fn take_finished_batches(&mut self) -> Option<mpsc::Receiver<()>> {
        self.finished_batches.take()
    }

    /// Waits for renders in progress and applies them. For tests and export, which need the finished result.
    pub fn wait_for_renders(&mut self) {
        loop {
            self.apply_finished_renders();
            if self.unrendered_count() == 0 {
                return;
            }
            let queued = self.counters.queued.load(Ordering::Acquire);
            let finished = self.counters.finished.load(Ordering::Acquire);
            if finished >= queued {
                self.apply_finished_renders();
                if self.unrendered_count() == 0 {
                    return;
                }
                // Everything sent has come back and something is still dirty: it changed since. Send it.
                self.render_all_dirty();
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    /// Messages about effects that failed to render, and media that failed to load.
    pub fn take_problems(&self) -> Vec<String> {
        std::mem::take(&mut *self.problems.lock())
    }

    pub fn elements(&self) -> &Arc<ElementMap> {
        &self.elements
    }

    pub fn tree(&self) -> &Arc<ElementTree> {
        &self.shared.tree
    }

    // ------------------------------------------------------------------------------------------------------------------
    // Undo
    // ------------------------------------------------------------------------------------------------------------------

    /// Records an action already applied. The UI's `UndoManager` mirrors the stack.
    pub fn record_undo(&mut self, action: UndoAction) {
        self.undo.push(action);
    }

    pub fn undo_stack(&self) -> &UndoStack {
        &self.undo
    }

    /// `UndoManager.Undo(n)`: returns how many steps ran.
    pub fn undo(&mut self, steps: usize) -> usize {
        let mut done = 0;
        for _ in 0..steps {
            let Some(entry) = self.undo.pop_undo() else { break };
            let action = self.apply_inverse(entry.action);
            self.undo.push_redo(UndoEntry { action, description: entry.description });
            done += 1;
        }
        if done > 0 {
            self.render_all_dirty();
        }
        done
    }

    /// `UndoManager.Redo(n)`.
    pub fn redo(&mut self, steps: usize) -> usize {
        let mut done = 0;
        for _ in 0..steps {
            let Some(entry) = self.undo.pop_redo() else { break };
            let action = self.apply_inverse(entry.action);
            self.undo.push_undo_keeping_redo(UndoEntry { action, description: entry.description });
            done += 1;
        }
        if done > 0 {
            self.render_all_dirty();
        }
        done
    }

    /// Applies an action's stored state, returning the action that reverses it.
    fn apply_inverse(&mut self, action: UndoAction) -> UndoAction {
        if matches!(action, UndoAction::MarkTimesChanged { .. }) {
            self.counters.edits.fetch_add(1, Ordering::Relaxed);
        } else {
            self.touch();
        }
        match action {
            // STUDIO-BEGIN sequence-settings-inverse: restore document settings without a parallel undo stack
            UndoAction::StudioSettings {label,length,layers} => {
                let inverse=UndoAction::StudioSettings {label,length:self.length(),layers:self.document().layers.clone()};
                self.set_length(length);self.document_mut().layers=layers;self.layers_changed();inverse
            }
            // STUDIO-END sequence-settings-inverse
            // STUDIO-BEGIN sequence-inspector-inverse: swap each collection's lines and bar back, leaving its marks alone
            UndoAction::StudioMarkVisibility { label, flags } => {
                let mut current = Vec::with_capacity(flags.len());
                for (id, lines, bar) in flags {
                    if let Some(c) = self.document.marks.iter_mut().find(|c| c.id == id) {
                        current.push((id, c.show_grid_lines, c.show_marks));
                        c.show_grid_lines = lines;
                        c.show_marks = bar;
                    }
                }
                self.shared.marks = Arc::new(self.document.marks.clone());
                UndoAction::StudioMarkVisibility { label, flags: current }
            }
            // STUDIO-END sequence-inspector-inverse
            // STUDIO-BEGIN sequence-audio-inverse: swap the song back whole, and the length only when its change set it
            UndoAction::StudioAudio { label, media, timing, clip, length } => {
                let inverse = UndoAction::StudioAudio {
                    label,
                    media: std::mem::replace(&mut self.document.media, media),
                    timing: std::mem::replace(&mut self.document.selected_timing_provider, timing),
                    clip: std::mem::replace(&mut self.shared.audio, clip),
                    length: length.map(|_| self.document.length),
                };
                if let Some(length) = length {
                    self.document.length = length;
                }
                inverse
            }
            // STUDIO-END sequence-audio-inverse
            // STUDIO-BEGIN marks-panel-inverse: swap the whole collection list back, then the mark-reading effects' parameters a delete normalised
            UndoAction::StudioMarkCollections { label, marks, params } => {
                let current_params = self.studio_mark_reader_params();
                let current = std::mem::take(&mut self.document.marks);
                let same_marks = current.len() == marks.len()
                    && current.iter().zip(&marks).all(|(a, b)| a.id == b.id && a.marks == b.marks);
                if same_marks {
                    // Only names, flags, colours or links: nothing that effects read changes.
                    self.set_marks_quietly(marks);
                    self.touch();
                } else {
                    self.set_marks(marks);
                }
                for (id, p) in params {
                    self.set_params(id, p);
                }
                UndoAction::StudioMarkCollections { label, marks: current, params: current_params }
            }
            // STUDIO-END marks-panel-inverse
            UndoAction::EffectsAdded { kind, ids } => {
                let snapshots = self.remove_effects(&ids);
                UndoAction::EffectsRemoved { kind, effects: snapshots }
            }
            UndoAction::EffectsRemoved { kind, effects } => {
                let ids = effects.iter().map(|s| s.id).collect();
                self.restore_effects(effects);
                UndoAction::EffectsAdded { kind, ids }
            }
            UndoAction::TimesChanged { kind, times } => {
                let current: Vec<(EffectId, TimeSpan, TimeSpan)> = times
                    .iter()
                    .filter_map(|(id, ..)| self.effects.get(id).map(|e| (*id, e.node.start, e.node.time_span)))
                    .collect();
                self.set_times(&times);
                UndoAction::TimesChanged { kind, times: current }
            }
            UndoAction::PlacementChanged { kind, places } => {
                let current: Vec<EffectPlacement> = places
                    .iter()
                    .filter_map(|p| {
                        let e = self.effects.get(&p.id)?;
                        Some(EffectPlacement { id: p.id, start: e.node.start, span: e.node.time_span, target: e.node.targets.first().cloned() })
                    })
                    .collect();
                let times: Vec<(EffectId, TimeSpan, TimeSpan)> = places.iter().map(|p| (p.id, p.start, p.span)).collect();
                self.set_times(&times);
                for p in &places {
                    if let Some((target, name)) = &p.target {
                        let changed = self.effects.get(&p.id).is_some_and(|e| e.node.targets.first().map(|(t, _)| t) != Some(target));
                        if changed {
                            self.set_target(p.id, *target, name);
                        }
                    }
                }
                UndoAction::PlacementChanged { kind, places: current }
            }
            UndoAction::Group { label, actions } => {
                let mut inverses: Vec<UndoAction> = actions.into_iter().rev().map(|a| self.apply_inverse(a)).collect();
                inverses.shrink_to_fit();
                UndoAction::Group { label, actions: inverses }
            }
            UndoAction::LayersChanged { layers } => {
                let current: Vec<(EffectId, Uuid)> =
                    layers.iter().filter_map(|(id, _)| self.layer_of(*id).map(|l| (*id, l))).collect();
                for (id, layer) in &layers {
                    self.set_layer(&[*id], *layer);
                }
                UndoAction::LayersChanged { layers: current }
            }
            UndoAction::ParamsChanged { label, params } => {
                let current: Vec<(EffectId, Params)> =
                    params.iter().filter_map(|(id, _)| self.effects.get(id).map(|e| (*id, e.node.params.clone()))).collect();
                for (id, p) in params {
                    self.set_params(id, p);
                }
                UndoAction::ParamsChanged { label, params: current }
            }
            UndoAction::MarksChanged { label, marks } => {
                let current = self.apply_marks_snapshot(&marks);
                UndoAction::MarksChanged { label, marks: current }
            }
            UndoAction::MarkTimesChanged { label, marks } => {
                let mut current = Vec::with_capacity(marks.len());
                for saved in &marks {
                    if let Some(m) = self.document.marks.iter_mut().find_map(|c| c.marks.iter_mut().find(|m| m.id == saved.mark)) {
                        current.push(MarkPlacement { mark: saved.mark, start: m.start_time, duration: m.duration });
                        // `MarkTimeInfo.SwapPlaces`: start first, so the setters' 25 ms rule applies.
                        if m.start_time != saved.start {
                            m.start_time = saved.start;
                            m.duration = m.duration.max(MARK_MIN_DURATION);
                        }
                        m.duration = saved.duration.max(MARK_MIN_DURATION);
                    }
                }
                self.marks_restored();
                UndoAction::MarkTimesChanged { label, marks: current }
            }
            UndoAction::MarksAdded { label, marks } => {
                let mut removed = Vec::with_capacity(marks.len());
                for (collection, mark) in marks {
                    let Some(c) = self.document.marks.iter_mut().find(|c| c.id == collection) else { continue };
                    // What it holds now, which may have had its text edited: that has no undo of its own.
                    match c.marks.iter().position(|m| m.id == mark.id) {
                        Some(at) => removed.push((collection, c.marks.remove(at))),
                        None => removed.push((collection, mark)),
                    }
                }
                self.marks_restored();
                UndoAction::MarksRemoved { label, marks: removed }
            }
            UndoAction::MarksRemoved { label, marks } => {
                for (collection, mark) in &marks {
                    let Some(c) = self.document.marks.iter_mut().find(|c| c.id == *collection) else { continue };
                    if !c.marks.iter().any(|m| m.id == mark.id) {
                        c.marks.push(mark.clone());
                    }
                }
                self.marks_restored();
                UndoAction::MarksAdded { label, marks }
            }
            UndoAction::RowsRestored { ids } => {
                self.rows_restored.extend_from_slice(&ids);
                UndoAction::RowsRestored { ids }
            }
        }
    }
}

impl Drop for SequenceSession {
    fn drop(&mut self) {
        self.close_video_cache();
    }
}

/// Whether an effect has a mark collection selector, at the top level or in a row of a list.
fn reads_marks(params: &[vixen_effects::framework::ParamDesc]) -> bool {
    use vixen_effects::framework::ParamKind;
    params.iter().any(|p| match p.kind {
        ParamKind::Record(r) | ParamKind::RecordList(r) => reads_marks(r.params),
        _ => matches!(p.name, "MarkCollectionId" | "MarkCollectionIds" | "MarkCollections"),
    })
}

/// One thread takes batches and renders each across the shared pool, so a big batch can't starve the next edit for long and
/// the session is never locked while it runs.
fn spawn_render_worker(
    results: Arc<Mutex<Vec<RenderResult>>>,
    counters: Arc<RenderCounters>,
    problems: Arc<Mutex<Vec<String>>>,
    cancel: Arc<AtomicBool>,
    batch_done: mpsc::Sender<()>,
) -> mpsc::Sender<RenderBatch> {
    let (sender, receiver) = mpsc::channel::<RenderBatch>();
    std::thread::Builder::new()
        .name("vixen-render-effects".to_string())
        .spawn(move || {
            use rayon::prelude::*;
            while let Ok(batch) = receiver.recv() {
                let RenderBatch { shared, tasks } = batch;
                let count = tasks.len();
                let audio = shared.audio.as_deref().map(|a| a as &dyn AudioSource);
                let env = RenderEnv { video_cache: shared.video_cache.as_deref(),
                    tree: &shared.tree,
                    frame_ms: shared.frame_ms,
                    marks: &shared.marks,
                    audio,
                    module_data_root: shared.module_data_root.as_deref(),
                };
                let finished: Vec<RenderResult> = tasks
                    .into_par_iter()
                    .filter_map(|mut task| {
                        if cancel.load(Ordering::Relaxed) {
                            return None;
                        }
                        let mut video_metadata = None;
                        if task.refresh_video_metadata && !task.params.string("FileName").is_empty() {
                            match video_metadata::read(&task.params, shared.module_data_root.as_deref(), &cancel) {
                                Ok(info) => {
                                    video_metadata::apply(&mut task.params, info);
                                    video_metadata = Some(info);
                                }
                                Err(message) => {
                                    if cancel.load(Ordering::Relaxed) { return None; }
                                    counters.failed.fetch_add(1, Ordering::Relaxed);
                                    problems.lock().push(message);
                                    return Some(RenderResult { id: task.id, generation: task.generation,
                                        intents: Arc::new(EffectIntents::new()), video_metadata: None });
                                }
                            }
                        }
                        if cancel.load(Ordering::Relaxed) { return None; }
                        let job = EffectJob {
                            id: task.instance_id,
                            descriptor: task.descriptor,
                            params: &task.params,
                            targets: &task.targets,
                            start: task.start,
                            span: task.span,
                        };
                        let intents = render_effect(&env, &job, &cancel).unwrap_or_else(|message| {
                            counters.failed.fetch_add(1, Ordering::Relaxed);
                            problems.lock().push(message);
                            EffectIntents::new()
                        });
                        Some(RenderResult { id: task.id, generation: task.generation, intents: Arc::new(intents), video_metadata })
                    })
                    .collect();
                results.lock().extend(finished);
                counters.finished.fetch_add(count, Ordering::Release);
                counters.revision.fetch_add(1, Ordering::Relaxed);
                let _ = batch_done.send(());
            }
        })
        .expect("the render thread starts");
    sender
}

#[cfg(test)]
#[path = "sequence_video_cache_tests.rs"]
mod video_cache_tests;

#[cfg(test)]
#[path = "sequence_restore_tests.rs"]
mod restore_tests;
