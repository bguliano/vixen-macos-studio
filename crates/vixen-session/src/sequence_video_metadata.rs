//! Video.FileName's deferred GetVideoInformation, kept on the render worker.
//! Stored ModuleData does not request a refresh unless its legacy quality is 0.

use super::*;
use vixen_video::VideoInfo;

pub(super) fn read(params: &Params, root: Option<&Path>, cancel: &AtomicBool) -> Result<VideoInfo, String> {
    let name = params.string("FileName");
    let path = root.map(|root| root.join("Video").join(name));
    path.and_then(|path| vixen_video::video_source_info_cancelable(&path, cancel))
        .ok_or_else(|| format!("Couldn't get video information for {name}."))
}

/// GetVideoInformation resets quality only after a successful probe. These are
/// computed values, not a second user edit; render and persistence use them too.
pub(super) fn apply(params: &mut Params, info: VideoInfo) -> bool {
    let mut changed = false;
    for (name, value) in [("VideoLength", ParamValue::Double(info.duration_seconds)), ("VideoQuality", ParamValue::Int(50))] {
        if params.get(name) != &value {
            params.set(name, value);
            changed = true;
        }
    }
    if let ParamValue::Record(size) = params.get("VideoSize") {
        if size.int("width") != info.width || size.int("height") != info.height {
            let mut size = size.clone();
            size.set("width", ParamValue::Int(info.width));
            size.set("height", ParamValue::Int(info.height));
            params.set("VideoSize", ParamValue::Record(size));
            changed = true;
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AppCore;
    use crate::effect_editor::panel::{self, CommitOptions, CommitOutcome, PanelEdits, RowControl, RowKey, ShownValue};
    use vixen_video::cache::{CacheOptions, VideoCache};

    struct Fixture { core: AppCore, root: PathBuf, target: Uuid }
    impl Fixture {
        fn new() -> Self {
            vixen_video::require_media_tools_for_tests();
            let root = std::env::temp_dir().join(format!("vixen-video-metadata-{}", Uuid::new_v4()));
            let mut core = AppCore::open(root.clone(), true).unwrap();
            core.video_cache = VideoCache::new(root.join("cache"), CacheOptions::default());
            let mut tree = ElementTree::new();
            let grid = vixen_effects::testing::grid(&mut tree, "Video grid", 16, 8);
            let target = tree.node(grid).id;
            core.system.write().tree = tree;
            std::fs::create_dir_all(root.join("inputs")).unwrap();
            let f = Self { core, root, target };
            f.copy("colors.mov", "a.mov");
            f.copy("quarter_turn.mov", "b.mov");
            f.copy("colors.avi", "c.avi");
            f
        }
        fn copy(&self, asset: &str, name: &str) {
            let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../vixen-video/tests/assets").join(asset);
            std::fs::copy(source, self.root.join("inputs").join(name)).unwrap();
        }
        fn sequence(&self, span_ms: i64) -> (SequenceSession, EffectId) {
            let mut sequence = SequenceSession::new_untitled(&self.core.session_environment());
            let descriptor = vixen_effect_registry::all().iter().find(|d| d.name == "Video").unwrap();
            let id = sequence.add_new_effect(descriptor.type_id, self.target, "Video grid", TimeSpan::ZERO, TimeSpan::from_ms_i(span_ms)).unwrap();
            sequence.wait_for_renders();
            (sequence, id)
        }
        fn choose(&self, sequence: &mut SequenceSession, id: EffectId, name: &str) -> CommitOutcome {
            panel::commit(sequence, &mut PanelEdits::new(Some(self.core.paths.module_data_dir())), &[id], &RowKey::top("FileName"),
                ShownValue::Path(self.root.join("inputs").join(name).to_string_lossy().into_owned()), CommitOptions::default())
        }
        fn save(&self, sequence: &mut SequenceSession) -> PathBuf {
            let path = self.core.paths.sequence_dir().join("metadata.tim");
            sequence.save(Some(&path)).unwrap();
            path
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            self.core.video_cache.shutdown();
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    fn metadata(sequence: &SequenceSession, id: EffectId) -> (f64, i32, i32, i32) {
        let p = &sequence.effect(id).unwrap().node.params;
        let ParamValue::Record(size) = p.get("VideoSize") else { panic!("missing size") };
        (p.double("VideoLength"), size.int("width"), size.int("height"), p.int("VideoQuality"))
    }
    fn assert_lit(sequence: &SequenceSession, id: EffectId) {
        assert!(!sequence.effect(id).unwrap().intents.as_ref().unwrap().is_empty());
    }
    fn workers_finished(sequence: &SequenceSession) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while sequence.counters.finished.load(Ordering::Acquire) < sequence.counters.queued.load(Ordering::Acquire) {
            assert!(std::time::Instant::now() < deadline, "worker completion timed out");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn file_selection_updates_real_duration_size_quality_and_preserves_saved_preferences() {
        let f = Fixture::new();
        let (mut sequence, id) = f.sequence(2000);
        sequence.set_param(id, "VideoQuality", ParamValue::Int(80));
        sequence.set_param(id, "StartTimeSeconds", ParamValue::Double(0.1));
        sequence.set_param(id, "PlayBackSpeed", ParamValue::Int(25));
        assert!(matches!(f.choose(&mut sequence, id, "a.mov"), CommitOutcome::Committed { new_step: true, .. }));
        assert_eq!(sequence.undo_stack().undo_descriptions().len(), 1);
        f.save(&mut sequence); // Computed completion must not dirty a saved document.
        let revision = sequence.counters.edits.load(Ordering::Relaxed);
        sequence.wait_for_renders();
        assert_eq!(metadata(&sequence, id), (3.0, 160, 90, 50));
        assert_lit(&sequence, id);
        assert!(!sequence.is_modified());
        assert_eq!(sequence.undo_stack().undo_descriptions().len(), 1);
        assert_eq!(sequence.counters.edits.load(Ordering::Relaxed), revision + 1);
        let p = &sequence.effect(id).unwrap().node.params;
        assert_eq!(p.double("StartTimeSeconds"), 0.1);
        assert_eq!(p.int("PlayBackSpeed"), 25);
        let panel = panel::panel_model(&sequence, &[id], 100);
        let row = panel.categories.iter().flat_map(|c| &c.rows).find(|r| r.label == "Video Length (sec)").unwrap();
        assert!(row.read_only);
        assert_eq!(row.control, RowControl::Label { value: "3".into() });
        assert!(!panel.categories.iter().flat_map(|c| &c.rows).any(|r| r.label == "Quality"));
        sequence.set_param(id, "VideoQuality", ParamValue::Int(73));
        sequence.wait_for_renders();
        let path = f.save(&mut sequence);
        drop(sequence);
        let mut reopened = SequenceSession::open(&path, &f.core.session_environment()).unwrap();
        let id = reopened.effects().next().unwrap().id;
        reopened.wait_for_renders();
        assert_eq!(metadata(&reopened, id), (3.0, 160, 90, 73));
        assert_lit(&reopened, id);
        assert!(!reopened.is_modified());
        assert!(!reopened.undo_stack().can_undo());
        reopened.set_param(id, "TargetPositioning", ParamValue::Enum("Locations".into()));
        let panel = panel::panel_model(&reopened, &[id], 100);
        let quality = panel.categories.iter().flat_map(|c| &c.rows).find(|r| r.label == "Quality").unwrap();
        assert!(matches!(quality.control, RowControl::Slider { value: 73.0, min: 10.0, max: 100.0, .. }));
    }

    #[test]
    fn failed_media_keeps_previous_metadata_and_same_basename_reselection_retries_without_extra_undo() {
        let f = Fixture::new();
        let (mut sequence, id) = f.sequence(2000);
        f.choose(&mut sequence, id, "a.mov");
        sequence.wait_for_renders();
        sequence.set_param(id, "VideoQuality", ParamValue::Int(77));
        std::fs::write(f.root.join("inputs/broken.mov"), "not a movie").unwrap();
        f.choose(&mut sequence, id, "broken.mov");
        sequence.wait_for_renders();
        assert_eq!(metadata(&sequence, id), (3.0, 160, 90, 77));
        assert!(sequence.effect(id).unwrap().intents.as_ref().unwrap().is_empty());
        assert!(sequence.take_problems().iter().any(|p| p.contains("video information for broken.mov")));
        f.save(&mut sequence);
        let undo = sequence.undo_stack().undo_descriptions();
        f.copy("colors.mov", "broken.mov");
        assert_eq!(f.choose(&mut sequence, id, "broken.mov"), CommitOutcome::NoChange);
        sequence.wait_for_renders();
        assert_eq!(metadata(&sequence, id), (3.0, 160, 90, 50));
        assert_lit(&sequence, id);
        assert!(!sequence.is_modified());
        assert_eq!(sequence.undo_stack().undo_descriptions(), undo);
        // A successful same-file reselection also resets a later quality edit.
        sequence.set_param(id, "VideoQuality", ParamValue::Int(88));
        sequence.wait_for_renders();
        f.save(&mut sequence);
        f.choose(&mut sequence, id, "broken.mov");
        sequence.wait_for_renders();
        assert_eq!(metadata(&sequence, id), (3.0, 160, 90, 50));
        assert!(!sequence.is_modified());
        assert_eq!(sequence.undo_stack().undo_descriptions(), undo);
    }

    #[test]
    fn rapid_file_changes_and_undo_reject_late_metadata_and_retry_unresolved_redo_files() {
        let f = Fixture::new();
        let (mut sequence, id) = f.sequence(500);
        f.choose(&mut sequence, id, "a.mov");
        sequence.wait_for_renders();
        sequence.set_param(id, "VideoQuality", ParamValue::Int(77));
        sequence.wait_for_renders();
        f.choose(&mut sequence, id, "b.mov");
        f.choose(&mut sequence, id, "c.avi");
        workers_finished(&sequence); // Real old results are queued, deliberately not yet applied.
        assert_eq!(sequence.undo(1), 1); // C -> still-unresolved B.
        sequence.wait_for_renders();
        assert_eq!(sequence.effect(id).unwrap().node.params.string("FileName"), "b.mov");
        assert_eq!(metadata(&sequence, id), (1.0, 160, 90, 50));
        assert_lit(&sequence, id); // Display rotation is still decoded/rendered normally.
        assert_eq!(sequence.undo(1), 1); // B -> A must restore A's custom quality.
        sequence.wait_for_renders();
        assert_eq!(metadata(&sequence, id), (3.0, 160, 90, 77));
        assert_eq!(sequence.redo(2), 2);
        sequence.wait_for_renders();
        assert_eq!(sequence.effect(id).unwrap().node.params.string("FileName"), "c.avi");
        assert_eq!(metadata(&sequence, id), (3.0, 160, 90, 50));
        assert_lit(&sequence, id); // Bundled software fallback survives metadata refresh.
    }

    #[test]
    fn choosing_first_existing_file_updates_other_selected_videos_in_one_undo_step() {
        let f = Fixture::new();
        let (mut sequence, a) = f.sequence(500);
        let type_id = sequence.effect(a).unwrap().node.type_id;
        let b = sequence.add_new_effect(type_id, f.target, "Video grid", TimeSpan::from_ms_i(1000), TimeSpan::from_ms_i(500)).unwrap();
        f.choose(&mut sequence, a, "a.mov");
        f.choose(&mut sequence, b, "b.mov");
        sequence.wait_for_renders();
        sequence.set_param(a, "VideoQuality", ParamValue::Int(77));
        sequence.set_param(b, "VideoQuality", ParamValue::Int(88));
        sequence.wait_for_renders();
        let before = sequence.undo_stack().undo_descriptions().len();
        let result = panel::commit(&mut sequence, &mut PanelEdits::new(Some(f.core.paths.module_data_dir())), &[a, b],
            &RowKey::top("FileName"), ShownValue::Path(f.root.join("inputs/a.mov").to_string_lossy().into_owned()), CommitOptions::default());
        assert!(matches!(result, CommitOutcome::Committed { new_step: true, .. }));
        sequence.wait_for_renders();
        assert_eq!(sequence.undo_stack().undo_descriptions().len(), before + 1);
        for id in [a, b] {
            assert_eq!(sequence.effect(id).unwrap().node.params.string("FileName"), "a.mov");
            assert_eq!(metadata(&sequence, id), (3.0, 160, 90, 50));
            assert_lit(&sequence, id);
        }
        assert_eq!(sequence.undo(1), 1);
        sequence.wait_for_renders();
        assert_eq!(sequence.effect(b).unwrap().node.params.string("FileName"), "b.mov");
        assert_eq!(metadata(&sequence, a), (3.0, 160, 90, 77));
        assert_eq!(metadata(&sequence, b), (1.0, 160, 90, 88));
    }

    #[test]
    fn undo_delete_of_pending_video_rebuilds_current_file_and_metadata() {
        let f = Fixture::new();
        let (mut sequence, id) = f.sequence(500);
        f.choose(&mut sequence, id, "a.mov");
        sequence.wait_for_renders();
        let old_intents = sequence.effect(id).unwrap().intents.clone().unwrap();
        f.choose(&mut sequence, id, "b.mov");
        workers_finished(&sequence); // B completed, but its metadata/intents have not been applied.
        let generation = sequence.effect(id).unwrap().generation;
        let effects = sequence.remove_effects(&[id]);
        sequence.record_undo(UndoAction::EffectsRemoved { kind: crate::undo::AddRemoveKind::AddedOrRemoved, effects });
        assert_eq!(sequence.undo(1), 1);
        sequence.wait_for_renders();
        let entry = sequence.effect(id).unwrap();
        assert!(entry.generation > generation, "restoration must reject the older queued result");
        assert!(entry.is_rendered());
        assert_eq!(entry.node.params.string("FileName"), "b.mov");
        assert_eq!(metadata(&sequence, id), (1.0, 160, 90, 50));
        assert!(!Arc::ptr_eq(&old_intents, entry.intents.as_ref().unwrap()));
        assert_lit(&sequence, id);
    }

    #[test]
    fn loaded_metadata_zero_is_preserved_but_legacy_quality_zero_requests_refresh() {
        let f = Fixture::new();
        let (mut sequence, id) = f.sequence(2000);
        std::fs::copy(f.root.join("inputs/a.mov"), f.core.paths.module_data_dir().join("Video/a.mov")).unwrap();
        let mut params = sequence.effect(id).unwrap().node.params.clone();
        params.set("FileName", ParamValue::Str("a.mov".into()));
        sequence.set_params(id, params); // ModuleData assignment, not the FileName setter.
        let path = f.save(&mut sequence);
        drop(sequence);
        let mut sequence = SequenceSession::open(&path, &f.core.session_environment()).unwrap();
        let id = sequence.effects().next().unwrap().id;
        sequence.wait_for_renders();
        assert_eq!(metadata(&sequence, id), (0.0, 600, 400, 50));
        assert_lit(&sequence, id); // Existing macOS repair of stale loaded render metadata stays intact.
        let mut params = sequence.effect(id).unwrap().node.params.clone();
        params.set("VideoQuality", ParamValue::Int(0));
        sequence.set_params(id, params);
        f.save(&mut sequence);
        drop(sequence);
        let mut reopened = SequenceSession::open(&path, &f.core.session_environment()).unwrap();
        let id = reopened.effects().next().unwrap().id;
        reopened.wait_for_renders();
        assert_eq!(metadata(&reopened, id), (3.0, 160, 90, 50));
        assert_lit(&reopened, id);
        assert!(!reopened.is_modified());
        assert!(!reopened.undo_stack().can_undo());
    }
}
