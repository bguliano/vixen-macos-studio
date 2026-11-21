//! Regression guard for preview frame pacing (roadmap R44): a sequence on the sample show's display plays muted into a
//! headless live preview of its largest preview, and the frames that reach the renderer must keep the engine's update
//! interval. The sequence is back-to-back two-second Pulse ramps on every top-level group, so the lights change on nearly
//! every update and a gap between frames is the pipeline's, not the music's (the show's own songs change every beat or so).
//! Runs on `target/show-copy` with a GPU, and is skipped without either.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use vixen_core::TimeSpan;
use vixen_session::app::AppCore;
use vixen_session::preview::{load_preview_data, preview_entries, LiveCounters, LiveTarget};
use vixen_session::render_support::{LiveRenderer, Scene};
use vixen_session::sequence::SequenceSession;

/// A Pulse with its default level curve, a ramp up: 255 levels over two seconds, a new one every 10 ms update.
const PULSE: uuid::Uuid = uuid::uuid!("cbd76d3b-c924-40ff-bad6-d1437b3dbdc0");
const RAMP_MS: i64 = 2000;
const LENGTH_MS: i64 = 10_000;
const WARM_UP: Duration = Duration::from_secs(1);
const MEASURE: Duration = Duration::from_secs(4);

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let to = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &to)
        } else if entry.file_type().unwrap().is_file() {
            std::fs::copy(entry.path(), to).unwrap();
        }
    }
}

struct Cleanup(PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

#[test]
fn playback_reaches_the_preview_at_the_update_interval() {
    let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
    let show = target.join("show-copy");
    if !show.join("SystemData").is_dir() {
        eprintln!("skipped: no target/show-copy");
        return;
    }
    let Ok(renderer) = LiveRenderer::headless(640, 360, 1.0) else {
        eprintln!("skipped: no GPU");
        return;
    };
    let renderer = Arc::new(renderer);
    let root = target.join(format!("studio-preview-pacing-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let _cleanup = Cleanup(root.clone());
    copy_dir(&show.join("SystemData"), &root.join("SystemData"));
    copy_dir(&show.join("Module Data Files"), &root.join("Module Data Files"));

    let core = AppCore::open(root.clone(), true).unwrap();
    let (scene, tree) = {
        let system = core.system.read();
        let scenes = preview_entries(&system).into_iter().map(|entry| Scene::build(&load_preview_data(&system.module_store, &entry).unwrap(), Some(&system.tree)));
        (scenes.max_by_key(|scene| scene.lights.len()).expect("the sample show has a preview"), system.tree.clone())
    };
    let counters = Arc::new(LiveCounters::default());
    let preview = core.playback.previews().add(LiveTarget::new(renderer.clone(), &scene, &tree, core.playback.elements()).with_counters(counters.clone()));

    let env = core.session_environment();
    let frame_ms = env.frame_ms as f64;
    let mut session = SequenceSession::new_untitled(&env);
    session.set_length(TimeSpan::from_ms_i(LENGTH_MS));
    for &group in &tree.roots {
        let node = tree.node(group);
        for start in (0..LENGTH_MS).step_by(RAMP_MS as usize) {
            session.add_new_effect(PULSE, node.id, &node.name, TimeSpan::from_ms_i(start), TimeSpan::from_ms_i(RAMP_MS)).unwrap();
        }
    }
    session.render_all_dirty();
    session.wait_for_renders();
    core.playback.play(&mut session, TimeSpan::ZERO, TimeSpan::from_ms_i(LENGTH_MS), false);

    std::thread::sleep(WARM_UP);
    let (engine_start, rendered_start) = (counters.engine_frames.load(Ordering::Relaxed), renderer.stats().frames_rendered);
    std::thread::sleep(MEASURE);
    let (engine_end, rendered_end) = (counters.engine_frames.load(Ordering::Relaxed), renderer.stats().frames_rendered);
    let changed = counters.changed_frames.load(Ordering::Relaxed);
    core.playback.stop();
    core.playback.previews().remove(preview);

    let seconds = MEASURE.as_secs_f64();
    let engine_fps = (engine_end - engine_start) as f64 / seconds;
    let rendered = (rendered_end - rendered_start) as usize;
    let gaps = renderer.recent_frame_gaps();
    let mut gaps: Vec<f64> = gaps[gaps.len().saturating_sub(rendered.saturating_sub(1))..].iter().map(|d| d.as_secs_f64() * 1000.0).collect();
    gaps.sort_by(f64::total_cmp);
    let (p50, p95, max) = (percentile(&gaps, 0.5), percentile(&gaps, 0.95), *gaps.last().unwrap());
    eprintln!(
        "{} lights, {frame_ms} ms updates: engine {engine_fps:.1} fps, {rendered} frames drawn ({changed} changed in all), gaps p50 {p50:.1} p95 {p95:.1} max {max:.1} ms",
        scene.lights.len()
    );

    let expected_fps = 1000.0 / frame_ms;
    assert!(engine_fps > expected_fps * 0.85, "the engine ran {engine_fps:.1} updates a second, expected {expected_fps:.1}");
    assert!(rendered as f64 > expected_fps * seconds * 0.8, "only {rendered} preview frames in {seconds} s at {expected_fps:.1} expected a second");
    // A healthy run on the Mac mini: p50 10.0, p95 13.5, max 19 ms against 10 ms updates.
    assert!(p50 < frame_ms * 1.25, "median frame gap {p50:.1} ms against {frame_ms} ms updates");
    assert!(p95 < frame_ms * 2.0, "95th percentile frame gap {p95:.1} ms against {frame_ms} ms updates");
    assert!(max < frame_ms * 5.0, "a {max:.1} ms stall against {frame_ms} ms updates");
}
