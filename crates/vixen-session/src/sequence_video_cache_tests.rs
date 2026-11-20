use super::*;
use crate::app::AppCore;
use vixen_video::cache::{CacheOptions, VideoCache};

struct Fixture {
    core: AppCore,
    root: PathBuf,
    target: Uuid,
}
impl Fixture {
    fn new() -> Self {
        vixen_video::require_media_tools_for_tests();
        let root = std::env::temp_dir().join(format!("vixen-sequence-video-{}", Uuid::new_v4()));
        let mut core = AppCore::open(root.clone(), true).unwrap();
        core.video_cache = VideoCache::new(root.join("cache"), CacheOptions::default());
        let mut tree = ElementTree::new();
        let grid = vixen_effects::testing::grid(&mut tree, "Video grid", 16, 8);
        let target = tree.node(grid).id;
        core.system.write().tree = tree;
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../vixen-video/tests/assets/colors.mov");
        std::fs::copy(source, core.paths.module_data_dir().join("Video/clip.mov")).unwrap();
        Self { core, root, target }
    }
    fn options(&self, clear: bool, format: &str) {
        let mut options = self.core.options();
        options.clear_effect_cache_on_exit = clear;
        options.video_effect_cache_file_type = format.into();
        self.core.set_options(&options).unwrap();
    }
    fn sequence(&self) -> (SequenceSession, EffectId) {
        let mut sequence = SequenceSession::new_untitled(&self.core.session_environment());
        let descriptor = vixen_effect_registry::all().iter().find(|d| d.name == "Video").unwrap();
        let id = sequence
            .add_new_effect(descriptor.type_id, self.target, "Video grid", TimeSpan::ZERO, TimeSpan::from_ms_i(2000))
            .unwrap();
        let mut params = sequence.effect(id).unwrap().node.params.clone();
        params.set("FileName", ParamValue::Str("clip.mov".into()));
        sequence.set_params(id, params);
        sequence.wait_for_renders();
        assert!(sequence.effect(id).unwrap().intents.as_ref().is_some_and(|intents| !intents.is_empty()));
        (sequence, id)
    }
    fn folders(&self) -> Vec<PathBuf> {
        std::fs::read_dir(self.core.video_cache.root())
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| entry.path().join("manifest.json").is_file())
            .map(|entry| entry.path())
            .collect()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.core.video_cache.shutdown();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn options_reach_real_video_cache_and_cache_size_label_then_clear_at_close() {
    let f = Fixture::new();
    f.options(false, "png");
    let mut options = f.core.options();
    options.update_interval_ms = 20;
    f.core.set_options(&options).unwrap();
    let (mut sequence, id) = f.sequence();
    let folder = f.folders().pop().unwrap();
    assert!(folder.join("00000000.png").is_file());
    let bytes: u64 = std::fs::read_dir(&folder)
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|e| e == "png"))
        .map(|e| e.metadata().unwrap().len())
        .sum();
    assert_eq!(
        sequence.effect(id).unwrap().node.params.string("CacheSize"),
        format!("{:.2} MB", bytes as f64 / 1_048_576.0)
    );
    let panel = crate::effect_editor::panel::panel_model(&sequence, &[id], 100);
    let row = panel.categories.iter().flat_map(|category| &category.rows).find(|row| row.label == "Cache Size").unwrap();
    assert!(row.read_only);
    assert_eq!(
        row.control,
        crate::effect_editor::panel::RowControl::Label {
            value: format!("{:.2} MB", bytes as f64 / 1_048_576.0)
        }
    );
    assert_ne!(sequence.effect(id).unwrap().node.params.string("CacheSize"), "0.00 MB");
    let path = f.core.paths.sequence_dir().join("cache.tim");
    sequence.save(Some(&path)).unwrap();
    drop(sequence);
    assert!(folder.exists());
    let mut reopened = SequenceSession::open(&path, &f.core.session_environment()).unwrap();
    reopened.wait_for_renders();
    assert_eq!(f.folders(), [folder.clone()]);
    // A changed option is visible to an already-open lease at close.
    f.options(true, "bmp");
    reopened.close_video_cache();
    assert!(!folder.exists());
}

#[test]
fn actual_delete_undo_and_redo_keep_cache_until_editor_close() {
    let f = Fixture::new();
    f.options(false, "bmp");
    let (mut sequence, id) = f.sequence();
    let folder = f.folders().pop().unwrap();
    let removed = sequence.remove_effects(&[id]);
    sequence.record_undo(UndoAction::EffectsRemoved {
        kind: crate::undo::AddRemoveKind::AddedOrRemoved,
        effects: removed,
    });
    assert!(folder.exists());
    sequence.undo(1);
    assert!(sequence.effect(id).is_some());
    assert!(folder.exists());
    sequence.redo(1);
    assert!(sequence.effect(id).is_none());
    assert!(folder.exists());
    sequence.close_video_cache();
    assert!(!folder.exists());
}

#[test]
fn transferred_non_editor_lease_outlives_temporary_sequence_and_shutdown_closes_it() {
    let f = Fixture::new();
    let (mut sequence, _) = f.sequence();
    let folder = f.folders().pop().unwrap();
    let resource = sequence.take_video_cache_lease().unwrap();
    drop(sequence);
    assert!(folder.exists());
    drop(resource);
    assert!(!folder.exists());
    let (mut sequence, _) = f.sequence();
    let folder = f.folders().pop().unwrap();
    let resource = sequence.take_video_cache_lease().unwrap();
    drop(sequence);
    assert!(folder.exists());
    f.core.shutdown();
    assert!(!folder.exists());
    drop(resource);
}

#[test]
fn legacy_avi_software_frames_reach_disk_cache_intents_and_cache_size() {
    let f = Fixture::new();
    f.options(false, "bmp");
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../vixen-video/tests/assets/colors.avi");
    std::fs::copy(source, f.core.paths.module_data_dir().join("Video/legacy.avi")).unwrap();
    let (mut sequence, id) = f.sequence();
    let mut params = sequence.effect(id).unwrap().node.params.clone();
    params.set("FileName", ParamValue::Str("legacy.avi".into()));
    sequence.set_params(id, params);
    sequence.wait_for_renders();
    let entry = sequence.effect(id).unwrap();
    assert!(!entry.intents.as_ref().unwrap().is_empty());
    let folder = f
        .folders()
        .into_iter()
        .find(|folder| std::fs::read_to_string(folder.join("manifest.json")).unwrap().contains("legacy.avi"))
        .unwrap();
    assert!(folder.join("00000000.bmp").is_file());
    let bytes: u64 = std::fs::read_dir(&folder)
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|e| e == "bmp"))
        .map(|e| e.metadata().unwrap().len())
        .sum();
    let panel = crate::effect_editor::panel::panel_model(&sequence, &[id], 100);
    let row = panel.categories.iter().flat_map(|category| &category.rows).find(|row| row.label == "Cache Size").unwrap();
    assert_eq!(
        row.control,
        crate::effect_editor::panel::RowControl::Label {
            value: format!("{:.2} MB", bytes as f64 / 1_048_576.0)
        }
    );
    assert!(bytes > 0);
}

#[test]
fn completed_cache_metadata_refreshes_the_editor_without_dirtying_or_undo() {
    let f = Fixture::new();
    f.options(false, "bmp");
    let (mut sequence, id) = f.sequence();
    let mut params = sequence.effect(id).unwrap().node.params.clone();
    params.set("CacheSize", ParamValue::Str("stale metadata".into()));
    sequence.set_params(id, params);
    let path = f.core.paths.sequence_dir().join("cache-label.tim");
    sequence.save(Some(&path)).unwrap();
    drop(sequence);

    let mut reopened = SequenceSession::open(&path, &f.core.session_environment()).unwrap();
    let id = reopened.effects().next().unwrap().id;
    assert_eq!(reopened.effect(id).unwrap().node.params.string("CacheSize"), "stale metadata");
    let revision = reopened.counters.edits.load(Ordering::Relaxed);
    reopened.wait_for_renders();
    assert_eq!(reopened.counters.edits.load(Ordering::Relaxed), revision + 1);
    assert_ne!(reopened.effect(id).unwrap().node.params.string("CacheSize"), "stale metadata");
    assert!(!reopened.is_modified());
    assert!(!reopened.undo_stack().can_undo());
    assert!(!reopened.undo_stack().can_redo());

    reopened.rerender_effects_where(|_| true);
    let revision = reopened.counters.edits.load(Ordering::Relaxed);
    reopened.wait_for_renders();
    assert_eq!(reopened.counters.edits.load(Ordering::Relaxed), revision, "unchanged metadata must not refresh again");
    assert!(!reopened.is_modified());
    assert!(!reopened.undo_stack().can_undo());
}
