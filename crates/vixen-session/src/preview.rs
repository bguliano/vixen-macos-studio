//! Live previews: each frame's element states turned into light colours for a GPU renderer. `PreviewPixel.Draw`, without the
//! window: a frame computes colours and hands them over, and drawing happens on the renderer's own thread.

use crate::SessionError;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;
use vixen_core::{CombinedState, ElementTree, TimeSpan};
use vixen_engine::schedule::ElementMap;
use vixen_engine::FrameSink;
use vixen_io::{DataPaths, ModuleStore, SystemData};
use vixen_preview::scene::NO_NODE;
use vixen_preview::{PreviewData, Scene};
use vixen_render::{rgba, LightGeom, LiveRenderer};

/// `VixenPreviewDescriptor.ModulePath`: the folder under Module Data Files that holds background images.
pub const MODULE_FOLDER: &str = "VixenDisplayPreview";

/// A preview in the system configuration.
#[derive(Clone, Debug)]
pub struct PreviewEntry {
    pub id: Uuid,
    pub name: String,
    pub type_id: Uuid,
}

/// The previews a profile has, in system order.
pub fn preview_entries(system: &SystemData) -> Vec<PreviewEntry> {
    system.previews.iter().map(|p| PreviewEntry { id: p.instance_id, name: p.name.clone(), type_id: p.type_id }).collect()
}

/// A preview's stored data.
pub fn load_preview_data(store: &ModuleStore, entry: &PreviewEntry) -> Result<PreviewData, SessionError> {
    let module = store
        .instance(entry.type_id, entry.id)
        .and_then(|m| m.data())
        .ok_or_else(|| SessionError::Other(format!("The preview {} has no data.", entry.name)))?;
    PreviewData::from_xml(module).map_err(|e| SessionError::Other(format!("The preview {} couldn't be read: {e}", entry.name)))
}

/// Where a preview's background image is. Only the file name is stored.
pub fn background_path(paths: &DataPaths, file_name: &str) -> PathBuf {
    paths.module_data_dir().join(MODULE_FOLDER).join(vixen_export::file_name_of(file_name))
}

/// Reads an image as RGBA8, top row first, for `LiveRenderer::set_background`.
pub fn load_background(path: &Path) -> Option<(Vec<u8>, u32, u32)> {
    let image = image::open(path).ok()?.to_rgba8();
    let (w, h) = image.dimensions();
    Some((image.into_raw(), w, h))
}

/// What one light shows.
#[derive(Copy, Clone)]
struct LightSource {
    /// The element's index in each frame's states, or `NO_STATE`.
    state: u32,
    /// For a light showing one colour of a discrete element, that colour. Otherwise the light is full colour.
    discrete: Option<[u8; 3]>,
}

const NO_STATE: u32 = u32::MAX;

/// How many engine frames reached a live window, and in how many its colours changed. For the status bar's numbers and for
/// measuring: a frame whose colours didn't change isn't sent to the renderer at all.
#[derive(Debug, Default)]
pub struct LiveCounters {
    pub engine_frames: std::sync::atomic::AtomicU64,
    pub changed_frames: std::sync::atomic::AtomicU64,
}

/// A renderer and how its lights map onto element states.
pub struct LiveTarget {
    renderer: Arc<LiveRenderer>,
    sources: Vec<LightSource>,
    /// What was last sent to the renderer.
    colors: Vec<u32>,
    /// All dark on the last frame, so another dark frame needs no work.
    was_dark: bool,
    /// `Update time for preview`.
    update_time: Option<Arc<crate::instrumentation::MillisecondsValue>>,
    counters: Option<Arc<LiveCounters>>,
    fixtures: Option<crate::preview_fixtures::FixturePlayer>,
}

impl LiveTarget {
    /// Sets the scene's lights on the renderer and works out where each light's colour comes from.
    pub fn new(renderer: Arc<LiveRenderer>, scene: &Scene, tree: &ElementTree, elements: &ElementMap) -> LiveTarget {
        let lights: Vec<LightGeom> = scene.lights.iter().map(|l| LightGeom { x: l.x, y: l.y, size: l.size }).collect();
        renderer.set_lights(&lights);
        renderer.set_view(scene.zoom() as f32, 0.0, 0.0);

        let resolved = scene.resolve_nodes(tree);
        let mut valid_colors: HashMap<u32, Vec<[u8; 3]>> = HashMap::new();
        let sources = scene
            .links
            .iter()
            .map(|link| {
                if link.node == NO_NODE {
                    return LightSource { state: NO_STATE, discrete: None };
                }
                let Some(Some(node)) = resolved.get(link.node as usize).copied() else {
                    return LightSource { state: NO_STATE, discrete: None };
                };
                let state = tree.node(node).element_id.and_then(|e| elements.index_of(&e)).unwrap_or(NO_STATE);
                let discrete = (link.color_count > 1 || tree.is_discrete(node)).then(|| {
                    let colors = valid_colors
                        .entry(link.node)
                        .or_insert_with(|| tree.valid_colors(node, false).iter().map(|c| [c.r, c.g, c.b]).collect());
                    colors.get(link.color_slot as usize).copied().unwrap_or([0, 0, 0])
                });
                LightSource { state, discrete }
            })
            .collect::<Vec<_>>();
        let colors = vec![0; sources.len()];
        // The renderer starts with every light transparent, which is what all zeros say.
        LiveTarget { renderer, sources, colors, was_dark: true, update_time: None, counters: None, fixtures: None }
    }

    pub fn with_fixtures(mut self, prepared: crate::preview_fixtures::FixturePreparation) -> LiveTarget {
        self.fixtures = crate::preview_fixtures::FixturePlayer::new(prepared, self.renderer.clone());
        self
    }

    pub fn with_counters(mut self, counters: Arc<LiveCounters>) -> LiveTarget {
        self.counters = Some(counters);
        self
    }

    /// Reports each frame's time to this value, for the Instrumentation window.
    pub fn with_update_time(mut self, value: Arc<crate::instrumentation::MillisecondsValue>) -> LiveTarget {
        self.update_time = Some(value);
        self
    }

    fn frame(&mut self, states: &[CombinedState], any_state: bool) {
        let started = self.update_time.is_some().then(std::time::Instant::now);
        self.compute(states, any_state);
        if let (Some(value), Some(started)) = (&self.update_time, started) {
            value.set(started.elapsed().as_secs_f64() * 1000.0);
        }
    }

    fn compute(&mut self, states: &[CombinedState], any_state: bool) {
        if let Some(fixtures) = &self.fixtures { fixtures.frame(if any_state { states } else { &[] }); }
        use std::sync::atomic::Ordering::Relaxed;
        if let Some(c) = &self.counters {
            c.engine_frames.fetch_add(1, Relaxed);
        }
        let changed = if !any_state {
            if self.was_dark {
                return;
            }
            self.was_dark = true;
            let lit = self.colors.iter().any(|c| *c != 0);
            self.colors.fill(0);
            lit
        } else {
            let mut changed = false;
            for (color, source) in self.colors.iter_mut().zip(&self.sources) {
                let new = states.get(source.state as usize).map_or(0, |state| light_color(state, source.discrete));
                if *color != new {
                    *color = new;
                    changed = true;
                }
            }
            self.was_dark = false;
            changed
        };
        // An unchanged frame would draw nothing new. Not sending it saves a copy and a wake-up, and keeps the renderer's dropped count to
        // frames that really were replaced before they could show.
        if changed {
            self.renderer.publish_colors(&self.colors);
            if let Some(c) = &self.counters {
                c.changed_frames.fetch_add(1, Relaxed);
            }
        }
    }
}

/// `PreviewPixel.Draw`: a full colour light shows the element's mixed colour with intensity as alpha, and ignores discrete
/// states. A discrete light shows its colour with that colour's intensity as alpha.
#[inline]
pub(crate) fn light_color(state: &CombinedState, discrete: Option<[u8; 3]>) -> u32 { // STUDIO: share native light colors with offscreen inspection frames
    match discrete {
        None => {
            let color = if let Some(rgb) = state.rgb {
                rgb.full_color_with_alpha()
            } else if let Some(lighting) = state.lighting {
                lighting.full_color_with_alpha()
            } else {
                return 0;
            };
            rgba(color.r, color.g, color.b, color.a)
        }
        Some([r, g, b]) => {
            let intensity = state.discrete.iter().find(|d| (d.color.r, d.color.g, d.color.b) == (r, g, b)).map(|d| d.intensity);
            match intensity {
                Some(i) if i > 0.0 => rgba(r, g, b, (i * 255.0) as u8),
                _ => 0,
            }
        }
    }
}

/// Every open live preview, fed from the engine's frames.
#[derive(Clone, Default)]
pub struct PreviewHub {
    targets: Arc<Mutex<Vec<(u64, LiveTarget)>>>,
    next_id: Arc<std::sync::atomic::AtomicU64>,
}

impl PreviewHub {
    pub fn has_fixtures(&self, id: u64) -> bool {
        self.targets.lock().iter().any(|(i,target)|*i==id && target.fixtures.is_some())
    }
    pub fn fixture_legends(&self, id: u64) -> Vec<crate::preview_fixtures::FixtureLegend> {
        self.targets.lock().iter().find(|(i,_)|*i==id).and_then(|(_,target)|target.fixtures.as_ref()).map_or_else(Vec::new, |f|f.legends())
    }
    /// Adds a target, returning an id to remove it with.
    pub fn add(&self, target: LiveTarget) -> u64 {
        let id = self.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.targets.lock().push((id, target));
        id
    }

    pub fn remove(&self, id: u64) {
        self.targets.lock().retain(|(i, _)| *i != id);
    }

    /// A window's lights changed, as they do after its designer closes: the same window carries on with this target.
    pub fn replace(&self, id: u64, target: LiveTarget) {
        if let Some(slot) = self.targets.lock().iter_mut().find(|(i, _)| *i == id) {
            slot.1 = target;
            // The previous worker is now joined. Publish after it so a paused reload cannot retain its last geometry.
            if let Some(fixtures) = &slot.1.fixtures { fixtures.frame(&[]); }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.targets.lock().is_empty()
    }

    /// The frame source to give the engine, once.
    pub fn sink(&self) -> PreviewSink {
        PreviewSink(self.clone())
    }
}

/// `FrameSink` over a hub.
pub struct PreviewSink(PreviewHub);

impl FrameSink for PreviewSink {
    fn frame(&mut self, _time: TimeSpan, states: &[CombinedState], any_state: bool) {
        for (_, target) in self.0.targets.lock().iter_mut() {
            target.frame(states, any_state);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vixen_core::{Color, DiscreteValue, LightingValue, RgbValue};

    #[test]
    fn full_colour_light_uses_intensity_as_alpha_and_ignores_discrete() {
        let mut state = CombinedState::default();
        assert_eq!(light_color(&state, None), 0);
        state.lighting = Some(LightingValue::new(Color::rgb(255, 128, 0), 0.5));
        assert_eq!(light_color(&state, None), rgba(255, 128, 0, 127));
        state.lighting = None;
        state.rgb = Some(RgbValue { r: 128, g: 0, b: 0 });
        assert_eq!(light_color(&state, None), rgba(255, 0, 0, 128));
        state.rgb = None;
        state.discrete.push(DiscreteValue::new(Color::RED, 1.0));
        assert_eq!(light_color(&state, None), 0);
    }

    #[test]
    fn an_unchanged_frame_is_not_sent() {
        let Ok(renderer) = LiveRenderer::headless(8, 8, 1.0) else { return };
        let renderer = Arc::new(renderer);
        let mut target = LiveTarget {
            renderer: Arc::clone(&renderer),
            sources: vec![LightSource { state: 0, discrete: None }],
            colors: vec![0],
            was_dark: true,
            update_time: None,
            counters: None,
            fixtures: None,
        };
        let counters = Arc::new(LiveCounters::default());
        target = target.with_counters(counters.clone());
        let mut lit = CombinedState::default();
        lit.lighting = Some(LightingValue::new(Color::rgb(255, 0, 0), 1.0));
        let dark = CombinedState::default();
        use std::sync::atomic::Ordering::Relaxed;
        // All dark to begin with, and a dark frame changes nothing.
        target.compute(&[dark.clone()], false);
        assert_eq!(counters.changed_frames.load(Relaxed), 0);
        target.compute(std::slice::from_ref(&lit), true);
        target.compute(std::slice::from_ref(&lit), true);
        assert_eq!((counters.engine_frames.load(Relaxed), counters.changed_frames.load(Relaxed)), (3, 1));
        target.compute(&[dark.clone()], true);
        target.compute(&[dark], false);
        assert_eq!(counters.changed_frames.load(Relaxed), 2, "a state that lights nothing, then no state, is one change");
        assert_eq!(renderer.read_pixels()[..4], [0, 0, 0, 255]);
    }

    #[test]
    fn discrete_light_shows_only_its_own_colour() {
        let mut state = CombinedState::default();
        state.discrete.push(DiscreteValue::new(Color::RED, 0.5));
        assert_eq!(light_color(&state, Some([255, 0, 0])), rgba(255, 0, 0, 127));
        assert_eq!(light_color(&state, Some([0, 255, 0])), 0);
    }
}
