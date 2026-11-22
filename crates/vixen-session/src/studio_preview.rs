//! Preview frames from the real preview geometry, links and rendered schedule: deterministic CPU inspection frames, or
//! the app's own GPU preview renderer drawing offscreen (background artwork, antialiased lights at any scale).
//! Fixture beams are reported as omitted, not silently synthesized.
use crate::{app::AppCore, sequence::SequenceSession};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
};
use uuid::Uuid;
use vixen_core::{CombinedState, TimeSpan};
use vixen_engine::{EvalScratch, Schedule};

#[path = "studio_video.rs"]
mod video;
pub use video::Codec;

#[path = "studio_geometry.rs"]
pub mod geometry;

#[path = "studio_activity.rs"]
pub mod activity;

const OMITTED: [&str; 3] = ["background artwork", "fixture beams", "GPU antialiasing"];
const GPU_OMITTED: [&str; 1] = ["fixture beams"];

/// One linked light, in scene units, resolved once so each frame only evaluates the schedule and paints.
struct Light {
    x: f32,
    y: f32,
    size: f32,
    index: usize,
    element: Uuid,
    discrete: Option<[u8; 3]>,
}

/// The app's preview renderer on an offscreen texture, and the colours last given to it.
struct Gpu {
    renderer: vixen_render::Renderer,
    colors: Vec<u32>,
}

/// A preview's lights bound to one rendered schedule. Holds no session lock, so long renders leave the editor free.
/// The frame shows scene units from `origin`, `scale` pixels per unit.
pub struct Renderer {
    lights: Vec<Light>,
    total: usize,
    /// The whole preview canvas, in scene units.
    canvas: (f32, f32),
    origin: (f32, f32),
    width: u32,
    height: u32,
    scale: f32,
    glow: bool,
    smooth: bool,
    background: Option<PathBuf>,
    background_alpha: u8,
    gpu: Option<Gpu>,
    schedule: Arc<Schedule>,
    scratch: EvalScratch,
    states: Vec<CombinedState>,
}

impl Renderer {
    /// Renders every pending effect first, then keeps the resulting schedule. `scale` shrinks geometry, keeping each
    /// light at least a pixel wide so a thumbnail never loses a light.
    pub fn new(
        core: &AppCore,
        session: &mut SequenceSession,
        preview_id: Uuid,
        scale: f32,
        glow: bool,
    ) -> Result<Self, String> {
        let system = core.system.read().clone();
        let entry = crate::preview::preview_entries(&system)
            .into_iter()
            .find(|p| p.id == preview_id)
            .ok_or("Preview not found")?;
        let data = crate::preview::load_preview_data(&system.module_store, &entry)
            .map_err(|e| e.to_string())?;
        let scene = vixen_preview::Scene::build(&data, Some(&system.tree));
        let right = scene
            .lights
            .iter()
            .map(|l| (l.x + l.size).ceil() as i32)
            .max()
            .unwrap_or(640);
        let bottom = scene
            .lights
            .iter()
            .map(|l| (l.y + l.size).ceil() as i32)
            .max()
            .unwrap_or(480);
        let width = data.width.max(data.setup_width).max(right).clamp(64, 2048) as u32;
        let height = data
            .height
            .max(data.setup_height)
            .max(bottom)
            .clamp(64, 2048) as u32;
        session.render_all_dirty();
        session.wait_for_renders();
        let schedule = session.schedule();
        let resolved = scene.resolve_nodes(&system.tree);
        let mut lights = Vec::new();
        for (light, link) in scene.lights.iter().zip(&scene.links) {
            let Some(Some(node)) = resolved.get(link.node as usize).copied() else {
                continue;
            };
            let Some((element, index)) = system
                .tree
                .node(node)
                .element_id
                .and_then(|e| Some((e, schedule.elements().index_of(&e)?)))
            else {
                continue;
            };
            let discrete = if link.color_count > 1 || system.tree.is_discrete(node) {
                let colors = system.tree.valid_colors(node, false);
                Some(
                    colors
                        .get(link.color_slot as usize)
                        .map(|c| [c.r, c.g, c.b])
                        .unwrap_or([0, 0, 0]),
                )
            } else {
                None
            };
            lights.push(Light {
                x: light.x,
                y: light.y,
                size: light.size,
                index: index as usize,
                element,
                discrete,
            });
        }
        let background = data
            .background_file_name
            .as_deref()
            .filter(|n| !n.is_empty())
            .map(|n| crate::preview::background_path(&core.paths, n))
            .filter(|p| p.is_file());
        Ok(Self {
            lights,
            total: scene.lights.len(),
            canvas: (width as f32, height as f32),
            origin: (0.0, 0.0),
            width: ((width as f32 * scale).round() as u32).max(16),
            height: ((height as f32 * scale).round() as u32).max(16),
            scale,
            glow,
            smooth: false,
            background,
            background_alpha: data.background_alpha.clamp(0, 255) as u8,
            gpu: None,
            schedule,
            scratch: Default::default(),
            states: Vec::new(),
        })
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// How many linked lights it paints.
    pub fn lights(&self) -> usize {
        self.total
    }

    /// The whole preview canvas in scene units: what a frame at scale 1 shows.
    pub fn canvas(&self) -> (f32, f32) {
        self.canvas
    }

    /// A light's radius in scene units, as the CPU frames draw it.
    fn unit_radius(light: &Light) -> f32 {
        (light.size / 2.0).clamp(0.5, 32.0)
    }

    /// How far a light reaches past its centre, glow included, in scene units.
    fn reach(&self, light: &Light) -> f32 {
        Self::unit_radius(light) * if self.glow { 3.0 } else { 1.0 } + 2.0
    }

    /// The scene rectangle `[x, y, width, height]` around the lights of `elements`, glow included, or `None` when the
    /// preview shows none of them.
    pub fn bounds_of(&self, elements: &HashSet<Uuid>) -> Option<[f32; 4]> {
        let mut found = None::<[f32; 4]>;
        for light in self.lights.iter().filter(|l| elements.contains(&l.element)) {
            let reach = self.reach(light);
            let [l, t, r, b] = found.unwrap_or([f32::MAX, f32::MAX, f32::MIN, f32::MIN]);
            found = Some([
                l.min(light.x - reach),
                t.min(light.y - reach),
                r.max(light.x + reach),
                b.max(light.y + reach),
            ]);
        }
        found.map(|[l, t, r, b]| [l, t, r - l, b - t])
    }

    /// Frames scene rectangle `[x, y, width, height]` at `scale` pixels per scene unit. Every light still draws; those
    /// outside the rectangle fall off the frame.
    pub fn frame(&mut self, rect: [f32; 4], scale: f32) {
        self.origin = (rect[0], rect[1]);
        self.scale = scale;
        self.width = ((rect[2] * scale).round() as u32).max(16);
        self.height = ((rect[3] * scale).round() as u32).max(16);
    }

    /// Draws lights as antialiased discs: each edge pixel takes the share of it the disc covers, rather than all or
    /// nothing. Thumbnails keep hard dots, which stay legible at a pixel or two across.
    pub fn smooth(&mut self, on: bool) {
        self.smooth = on;
    }

    /// Keeps only the lights of `elements` and crops to them, scaled to fit `max_width` by `max_height` (at most four
    /// times their preview size), so one prop fills a thumbnail. Call on a renderer built at scale 1. Returns how many
    /// lights are left; with none the size is unchanged.
    pub fn focus(&mut self, elements: &HashSet<Uuid>, max_width: u32, max_height: u32) -> usize {
        let Some(rect) = self.bounds_of(elements) else {
            self.lights.clear();
            self.total = 0;
            return 0;
        };
        self.lights.retain(|l| elements.contains(&l.element));
        self.total = self.lights.len();
        let scale = (max_width as f32 / rect[2]).min(max_height as f32 / rect[3]).min(4.0);
        self.frame(rect, scale);
        self.total
    }

    /// Draws with the app's preview renderer instead of the CPU: the same antialiased lights and background artwork as
    /// the preview window, at this frame's size and scale. Call after framing. Returns the GPU's name.
    pub fn use_gpu(&mut self) -> Result<String, String> {
        let mut renderer = vixen_render::Renderer::headless(self.width, self.height, self.scale)
            .map_err(|e| format!("GPU renderer unavailable: {e}"))?;
        let geometry: Vec<vixen_render::LightGeom> = self
            .lights
            .iter()
            .map(|l| vixen_render::LightGeom { x: l.x, y: l.y, size: l.size })
            .collect();
        renderer.set_lights(&geometry);
        // A scene point lands at (point + pan + 0.5) * scale pixels, so this pan puts the origin at the corner.
        renderer.set_view(1.0, -self.origin.0, -self.origin.1);
        if let Some((rgba, w, h)) = self.background.as_deref().and_then(crate::preview::load_background) {
            renderer.set_background(&rgba, w, h);
            renderer.set_background_alpha(self.background_alpha);
        }
        renderer.render().map_err(|e| format!("GPU renderer failed: {e}"))?;
        if renderer.read_pixels().len() != self.width as usize * self.height as usize * 4 {
            return Err(format!("GPU renderer cannot draw {}x{}", self.width, self.height));
        }
        let name = renderer.adapter_name().to_string();
        self.gpu = Some(Gpu { colors: vec![0; self.lights.len()], renderer });
        Ok(name)
    }

    /// Whether frames come from the app's preview renderer.
    pub fn is_gpu(&self) -> bool {
        self.gpu.is_some()
    }

    /// Each linked light's level at `time_ms`, 0 to 1: its brightest channel (or a discrete colour's intensity), as the
    /// preview draws it.
    pub fn levels(&mut self, time_ms: f64, out: &mut Vec<f32>) {
        self.schedule.evaluate(
            TimeSpan::from_ms(time_ms),
            &mut self.scratch,
            &mut self.states,
        );
        out.clear();
        out.extend(self.lights.iter().map(|l| {
            self.states.get(l.index).map_or(0.0, |state| {
                f32::from(crate::preview::light_color(state, l.discrete).to_le_bytes()[3]) / 255.0
            })
        }));
    }

    /// Paints the lights at `time_ms` onto black (or the background artwork, on the GPU), returning how many were lit.
    pub fn paint(&mut self, time_ms: f64, image: &mut image::RgbaImage) -> usize {
        self.schedule.evaluate(
            TimeSpan::from_ms(time_ms),
            &mut self.scratch,
            &mut self.states,
        );
        let (width, height) = image.dimensions();
        let colors: Vec<u32> = self
            .lights
            .iter()
            .map(|l| {
                self.states
                    .get(l.index)
                    .map_or(0, |state| crate::preview::light_color(state, l.discrete))
            })
            .collect();
        let illuminated = colors.iter().filter(|c| c.to_le_bytes()[3] != 0).count();
        let mut drawn = false;
        if let Some(gpu) = &mut self.gpu {
            gpu.colors.copy_from_slice(&colors);
            gpu.renderer.set_light_colors(&gpu.colors);
            if gpu.renderer.render().is_ok() {
                let pixels = gpu.renderer.read_pixels();
                let row = self.width as usize * 4;
                let out = width as usize * 4;
                // The target is opaque, so its rows go straight in; an even-size frame's extra column stays black.
                for (y, line) in pixels.chunks_exact(row).enumerate().take(height as usize) {
                    let n = row.min(out);
                    image.as_mut()[y * out..y * out + n].copy_from_slice(&line[..n]);
                }
                drawn = pixels.len() == row * self.height as usize;
            }
        }
        if !drawn {
            for pixel in image.pixels_mut() {
                *pixel = image::Rgba([0, 0, 0, 255]);
            }
        }
        for (light, color) in self.lights.iter().zip(&colors) {
            let [r, g, b, a] = color.to_le_bytes();
            if a == 0 {
                continue;
            }
            let (x, y) = (
                (light.x - self.origin.0) * self.scale,
                (light.y - self.origin.1) * self.scale,
            );
            let radius = (Self::unit_radius(light) * self.scale).max(0.75);
            if self.glow {
                // A soft additive halo, so dense strings read as light rather than dots.
                let reach = (radius * 3.0).max(2.0 * self.scale.max(0.5));
                let (hx, hy) = if self.gpu.is_some() { (x + 0.5 * self.scale, y + 0.5 * self.scale) } else { (x, y) };
                paint_disc(image, width, height, hx, hy, reach, |pixel, d| {
                    let falloff = (1.0 - d / reach).powi(2) * 0.35 * f32::from(a) / 255.0;
                    for (i, c) in [r, g, b].into_iter().enumerate() {
                        pixel[i] = (f32::from(pixel[i]) + f32::from(c) * falloff).min(255.0) as u8;
                    }
                });
            }
            if self.gpu.is_some() {
                continue;
            }
            let over = |pixel: &mut image::Rgba<u8>, alpha: u32| {
                for (i, c) in [r, g, b].into_iter().enumerate() {
                    pixel[i] = ((u32::from(c) * alpha + u32::from(pixel[i]) * (255 - alpha)) / 255) as u8;
                }
            };
            if self.smooth {
                paint_disc(image, width, height, x, y, radius + 0.5, |pixel, d| {
                    let coverage = (radius + 0.5 - d).clamp(0.0, 1.0);
                    over(pixel, (f32::from(a) * coverage).round() as u32);
                });
            } else {
                paint_disc(image, width, height, x, y, radius, |pixel, _| over(pixel, u32::from(a)));
            }
        }
        illuminated
    }
}

pub(crate) fn paint_disc(
    image: &mut image::RgbaImage,
    width: u32,
    height: u32,
    cx: f32,
    cy: f32,
    radius: f32,
    mut blend: impl FnMut(&mut image::Rgba<u8>, f32),
) {
    let left = (cx - radius).floor().max(0.0) as u32;
    let right = (cx + radius).ceil().min(width as f32) as u32;
    let top = (cy - radius).floor().max(0.0) as u32;
    let bottom = (cy + radius).ceil().min(height as f32) as u32;
    for y in top..bottom {
        for x in left..right {
            let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
            if d <= radius {
                blend(image.get_pixel_mut(x, y), d);
            }
        }
    }
}

/// Every preview that shows any of `elements`' lights, with how many, most first: where a prop's own geometry is.
pub fn previews_linking(core: &AppCore, elements: &HashSet<Uuid>) -> Vec<(Uuid, String, usize)> {
    let system = core.system.read();
    let mut found = Vec::new();
    for entry in crate::preview::preview_entries(&system) {
        let Ok(data) = crate::preview::load_preview_data(&system.module_store, &entry) else {
            continue;
        };
        let scene = vixen_preview::Scene::build(&data, Some(&system.tree));
        let resolved = scene.resolve_nodes(&system.tree);
        let lit = scene
            .links
            .iter()
            .filter(|link| {
                resolved
                    .get(link.node as usize)
                    .copied()
                    .flatten()
                    .and_then(|node| system.tree.node(node).element_id)
                    .is_some_and(|e| elements.contains(&e))
            })
            .count();
        if lit > 0 {
            found.push((entry.id, entry.name.clone(), lit));
        }
    }
    found.sort_by(|a, b| b.2.cmp(&a.2));
    found
}

pub fn frame(
    core: &AppCore,
    session: &mut SequenceSession,
    preview_id: Uuid,
    time_ms: f64,
    path: &Path,
) -> Result<Value, String> {
    let mut renderer = Renderer::new(core, session, preview_id, 1.0, false)?;
    let (width, height) = renderer.size();
    let mut image = image::RgbaImage::new(width, height);
    let illuminated = renderer.paint(time_ms, &mut image);
    image.save(path).map_err(|e| e.to_string())?;
    Ok(
        json!({"path":path,"width":width,"height":height,"lights":renderer.total,"illuminated":illuminated,"timeMs":time_ms,"renderer":"CPU inspection frame using real scene/schedule","omitted":OMITTED}),
    )
}

/// What a frame range covers, resolved and validated by the command layer.
pub struct Range {
    pub preview_id: Uuid,
    pub start_ms: f64,
    pub end_ms: f64,
    pub fps: f64,
    pub glow: bool,
}

impl Range {
    fn times(&self) -> impl Iterator<Item = f64> + '_ {
        let count = ((self.end_ms - self.start_ms) * self.fps / 1000.0)
            .ceil()
            .max(1.0) as usize;
        (0..count).map(move |i| self.start_ms + i as f64 * 1000.0 / self.fps)
    }
}

/// What part of the preview a video shows.
pub enum Framing {
    /// The whole preview canvas.
    Canvas,
    /// The lights of these elements, with `padding` scene units around them.
    Fit { elements: HashSet<Uuid>, padding: f32 },
    /// A scene rectangle `[x, y, width, height]`.
    Crop([f32; 4]),
}

/// How big a video's frames are.
pub enum Size {
    /// Pixels per scene unit.
    Scale(f32),
    /// The largest scale that fits these pixel bounds (either may be open), at most 8.
    Fit(Option<u32>, Option<u32>),
    /// Twice the preview's size, as a Retina preview window draws it, or less if that is more than the encoder takes.
    Default,
}

/// How a video is compressed.
#[derive(Clone, Copy)]
pub enum Encoding {
    /// H.264 at this many bits per pixel per frame, between 2 and 120 Mb/s.
    H264Density(f64),
    /// H.264 at this average bit rate, bits per second.
    H264Bitrate(u32),
    ProRes4444,
}

impl Encoding {
    /// The default: generous enough that lights on black come through without visible artefacts.
    pub const HIGH: Encoding = Encoding::H264Density(0.3);
    pub const STANDARD: Encoding = Encoding::H264Density(0.1);
    pub const MAX: Encoding = Encoding::H264Density(0.8);

    pub fn codec(self, width: u32, height: u32, fps: f64) -> Codec {
        match self {
            Encoding::H264Density(bpp) => Codec::H264 {
                bitrate: (f64::from(width) * f64::from(height) * fps * bpp).clamp(2.0e6, 120.0e6) as u32,
            },
            Encoding::H264Bitrate(bitrate) => Codec::H264 { bitrate },
            Encoding::ProRes4444 => Codec::ProRes4444,
        }
    }
}

/// Everything about a video beyond its span, resolved by the command layer.
pub struct VideoOptions {
    pub framing: Framing,
    pub size: Size,
    /// Draw with the app's preview renderer (falling back to CPU frames if no GPU is available).
    pub gpu: bool,
    pub encoding: Encoding,
}

/// The largest frame the encoders take: H.264 tops out at 4096x2304 worth of pixels.
const MAX_SIDE: u32 = 4096;
const MAX_H264_PIXELS: u64 = 4096 * 2304;

/// Renders the range to an MP4 (H.264) or MOV, with the sequence audio for the same span as AAC when `audio` is given.
/// The session lock is held only to render effects and take the schedule.
pub fn video(
    core: &AppCore,
    shared: &Arc<Mutex<SequenceSession>>,
    range: &Range,
    options: &VideoOptions,
    audio: Option<&Path>,
    path: &Path,
) -> Result<Value, String> {
    let mut renderer = Renderer::new(
        core,
        &mut shared.lock(),
        range.preview_id,
        1.0,
        range.glow,
    )?;
    let (canvas_w, canvas_h) = renderer.canvas();
    let rect = match &options.framing {
        Framing::Canvas => [0.0, 0.0, canvas_w, canvas_h],
        Framing::Crop(rect) => *rect,
        Framing::Fit { elements, padding } => {
            let [x, y, w, h] = renderer
                .bounds_of(elements)
                .ok_or("This preview shows none of the fit target's lights")?;
            [x - padding, y - padding, w + 2.0 * padding, h + 2.0 * padding]
        }
    };
    let scale = match options.size {
        Size::Scale(scale) => scale,
        Size::Fit(width, height) => {
            let by = |limit: Option<u32>, extent: f32| limit.map_or(f32::MAX, |l| l as f32 / extent);
            by(width, rect[2]).min(by(height, rect[3])).clamp(0.1, 8.0)
        }
        Size::Default => {
            let side = (MAX_SIDE - 1) as f32 / rect[2].max(rect[3]);
            let area = ((MAX_H264_PIXELS as f64 * 0.98) / f64::from(rect[2] * rect[3])).sqrt() as f32;
            2.0f32.min(side).min(area).max(0.1)
        }
    };
    renderer.frame(rect, scale);
    renderer.smooth(true);
    let (width, height) = renderer.size();
    let pixels = u64::from(width + width % 2) * u64::from(height + height % 2);
    if width > MAX_SIDE || height > MAX_SIDE || (!matches!(options.encoding, Encoding::ProRes4444) && pixels > MAX_H264_PIXELS) {
        return Err(format!(
            "Frames would be {width}x{height}; keep within {MAX_SIDE} px a side and 4096x2304 pixels for H.264 (lower scale or width/height, or fit/crop to the props)"
        ));
    }
    let mut omitted: Vec<&str> = OMITTED.to_vec();
    let mut renderer_note = "CPU inspection frames using real scene/schedule, antialiased".to_string();
    if options.gpu {
        match renderer.use_gpu() {
            Ok(adapter) => {
                omitted = GPU_OMITTED.to_vec();
                renderer_note = format!("app preview renderer (GPU, {adapter}) using real scene/schedule");
            }
            Err(reason) => renderer_note = format!("CPU inspection frames using real scene/schedule, antialiased ({reason})"),
        }
    }
    let times: Vec<f64> = range.times().collect();
    let started = std::time::Instant::now();
    let codec = options.encoding.codec(width + width % 2, height + height % 2, range.fps);
    let (frames, audio, width, height, lit) = encode(&mut renderer, &times, codec, range.fps, range.start_ms, range.end_ms, audio, path, &mut |_, _| {})?;
    // Dead air across the whole preview at the default thresholds, from the same schedule: cheap next to painting.
    let thresholds = activity::Thresholds::DEFAULT;
    let mut whole = [activity::Group::whole(&renderer)];
    activity::measure(&mut renderer, &times, thresholds.level, &mut whole);
    let (dark, dark_ms) = activity::dark_json(&times, range.end_ms, &whole[0].series, thresholds);
    Ok(json!({
        "path": path,
        "width": width,
        "height": height,
        "scale": (scale * 1000.0).round() / 1000.0,
        "crop": rect.map(|v| (v * 10.0).round() / 10.0),
        "fps": range.fps,
        "frames": frames,
        "startMs": range.start_ms,
        "endMs": range.end_ms,
        "audio": audio,
        "lights": renderer.total,
        "maxIlluminated": lit,
        "dark": dark,
        "darkMs": dark_ms,
        "darkThresholds": thresholds.to_json(),
        "seconds": (started.elapsed().as_secs_f64() * 100.0).round() / 100.0,
        "codec": format!("{}, AAC audio, {}", codec.describe(), if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("mov")) { "MOV" } else { "MP4" }),
        "renderer": renderer_note,
        "omitted": omitted,
    }))
}

/// Paints `times` from `renderer` into a video in `codec` (with `audio` for the span as AAC when given). Returns what was
/// written, the even frame size and the most lights lit in one frame. `overlay` draws on each frame after the lights.
pub fn encode(
    renderer: &mut Renderer,
    times: &[f64],
    codec: Codec,
    fps: f64,
    start_ms: f64,
    end_ms: f64,
    audio: Option<&Path>,
    path: &Path,
    overlay: &mut dyn FnMut(f64, &mut image::RgbaImage),
) -> Result<(usize, Value, u32, u32, usize), String> {
    let (width, height) = renderer.size();
    // H.264 needs even dimensions; the extra row or column stays black.
    let (width, height) = (width + width % 2, height + height % 2);
    let mut image = image::RgbaImage::new(width, height);
    let mut lit = 0usize;
    let mut next = 0;
    let written = video::write(
        path,
        codec,
        width,
        height,
        fps,
        start_ms,
        end_ms,
        audio,
        &mut |bgra: &mut [u8], row: usize| {
            let time = *times.get(next)?;
            next += 1;
            lit = lit.max(renderer.paint(time, &mut image));
            overlay(time, &mut image);
            for (y, line) in image.rows().enumerate() {
                for (out, px) in bgra[y * row..y * row + width as usize * 4]
                    .chunks_exact_mut(4)
                    .zip(line)
                {
                    out.copy_from_slice(&[px[2], px[1], px[0], 255]);
                }
            }
            Some(())
        },
    )?;
    Ok((written.frames, written.audio, width, height, lit))
}

/// sequence_activity: the lit share of the preview (and of each of `elements`, with their names) at every frame of the
/// range, and the dark runs, without painting anything. The session lock is held only to render effects.
pub fn activity(
    core: &AppCore,
    shared: &Arc<Mutex<SequenceSession>>,
    range: &Range,
    thresholds: activity::Thresholds,
    elements: &[(Uuid, String, HashSet<Uuid>)],
    series: bool,
) -> Result<Value, String> {
    let mut renderer = Renderer::new(core, &mut shared.lock(), range.preview_id, 1.0, false)?;
    let mut groups = vec![activity::Group::whole(&renderer)];
    groups.extend(elements.iter().map(|(id, name, under)| activity::Group::of(&renderer, *id, name.clone(), under)));
    let times: Vec<f64> = range.times().collect();
    activity::measure(&mut renderer, &times, thresholds.level, &mut groups);
    let mut out = activity::group_json(&groups[0], &times, range.end_ms, thresholds, series);
    out["previewId"] = json!(range.preview_id.to_string());
    out["startMs"] = json!(range.start_ms);
    out["endMs"] = json!(range.end_ms);
    out["fps"] = json!(range.fps);
    out["frames"] = json!(times.len());
    out["thresholds"] = thresholds.to_json();
    if !elements.is_empty() {
        out["elements"] = json!(groups[1..].iter().map(|g| activity::group_json(g, &times, range.end_ms, thresholds, series)).collect::<Vec<_>>());
    }
    out["source"] = json!("rendered schedule intensities of the preview's linked lights (brightest channel), not a decoded video");
    Ok(out)
}

/// Tiles the range's frames into one PNG, each captioned with its sequence time, for reviewing a passage as one image.
pub fn sheet(
    core: &AppCore,
    shared: &Arc<Mutex<SequenceSession>>,
    range: &Range,
    columns: u32,
    tile_width: u32,
    path: &Path,
) -> Result<Value, String> {
    let times: Vec<f64> = range.times().collect();
    if times.len() > 400 {
        return Err(format!(
            "{} tiles requested; a sheet holds at most 400 (shorten the range or lower fps)",
            times.len()
        ));
    }
    let full = {
        let mut s = shared.lock();
        Renderer::new(core, &mut s, range.preview_id, 1.0, false)?.size()
    };
    let scale = (tile_width as f32 / full.0 as f32).min(1.0);
    let mut renderer = Renderer::new(
        core,
        &mut shared.lock(),
        range.preview_id,
        scale,
        range.glow,
    )?;
    let (tile_w, tile_h) = renderer.size();
    const CAPTION: u32 = 11;
    const GAP: u32 = 2;
    let columns = columns.min(times.len() as u32).max(1);
    let rows = (times.len() as u32).div_ceil(columns);
    let cell_w = tile_w + GAP;
    let cell_h = tile_h + CAPTION + GAP;
    let (width, height) = (columns * cell_w + GAP, rows * cell_h + GAP);
    if width > 8192 || height > 8192 {
        return Err(format!("Sheet would be {width}x{height}; keep it within 8192 (fewer columns, smaller tileWidth or a shorter range)"));
    }
    let mut sheet = image::RgbaImage::from_pixel(width, height, image::Rgba([40, 40, 40, 255]));
    let mut tile = image::RgbaImage::new(tile_w, tile_h);
    let mut lit = 0usize;
    for (i, time) in times.iter().enumerate() {
        lit = lit.max(renderer.paint(*time, &mut tile));
        let (x, y) = (
            GAP + (i as u32 % columns) * cell_w,
            GAP + (i as u32 / columns) * cell_h,
        );
        image::imageops::replace(&mut sheet, &tile, i64::from(x), i64::from(y));
        caption(&mut sheet, x + 1, y + tile_h + 2, &timestamp(*time));
    }
    sheet.save(path).map_err(|e| e.to_string())?;
    Ok(json!({
        "path": path,
        "width": width,
        "height": height,
        "tiles": times.len(),
        "columns": columns,
        "rows": rows,
        "tileWidth": tile_w,
        "tileHeight": tile_h,
        "timesMs": times,
        "maxIlluminated": lit,
        "lights": renderer.total,
        "renderer": "CPU inspection frames using real scene/schedule, read left to right, top to bottom",
        "omitted": OMITTED,
    }))
}

/// `m:ss.cc`, the sequence time under each tile.
fn timestamp(ms: f64) -> String {
    let centis = (ms / 10.0).round() as u64;
    format!(
        "{}:{:02}.{:02}",
        centis / 6000,
        centis / 100 % 60,
        centis % 100
    )
}

/// A 3x5 glyph, one row per byte with the leftmost column in bit 2: digits, letters (either case), space and
/// `: . - # / ( ) % + , ' _`. Anything else is a dot.
fn glyph(ch: char) -> [u8; 5] {
    match ch.to_ascii_uppercase() {
        '0' => [7, 5, 5, 5, 7],
        '1' => [2, 6, 2, 2, 7],
        '2' => [7, 1, 7, 4, 7],
        '3' => [7, 1, 7, 1, 7],
        '4' => [5, 5, 7, 1, 1],
        '5' => [7, 4, 7, 1, 7],
        '6' => [7, 4, 7, 5, 7],
        '7' => [7, 1, 1, 1, 1],
        '8' => [7, 5, 7, 5, 7],
        '9' => [7, 5, 7, 1, 7],
        'A' => [2, 5, 7, 5, 5],
        'B' => [6, 5, 6, 5, 6],
        'C' => [3, 4, 4, 4, 3],
        'D' => [6, 5, 5, 5, 6],
        'E' => [7, 4, 6, 4, 7],
        'F' => [7, 4, 6, 4, 4],
        'G' => [3, 4, 5, 5, 3],
        'H' => [5, 5, 7, 5, 5],
        'I' => [7, 2, 2, 2, 7],
        'J' => [1, 1, 1, 5, 2],
        'K' => [5, 5, 6, 5, 5],
        'L' => [4, 4, 4, 4, 7],
        'M' => [5, 7, 7, 5, 5],
        'N' => [6, 5, 5, 5, 5],
        'O' => [2, 5, 5, 5, 2],
        'P' => [6, 5, 6, 4, 4],
        'Q' => [2, 5, 5, 6, 3],
        'R' => [6, 5, 6, 5, 5],
        'S' => [3, 4, 2, 1, 6],
        'T' => [7, 2, 2, 2, 2],
        'U' => [5, 5, 5, 5, 7],
        'V' => [5, 5, 5, 5, 2],
        'W' => [5, 5, 7, 7, 5],
        'X' => [5, 5, 2, 5, 5],
        'Y' => [5, 5, 2, 2, 2],
        'Z' => [7, 1, 2, 4, 7],
        ' ' => [0, 0, 0, 0, 0],
        ':' => [0, 2, 0, 2, 0],
        '-' => [0, 0, 7, 0, 0],
        '#' => [5, 7, 5, 7, 5],
        '/' => [1, 1, 2, 4, 4],
        '(' => [1, 2, 2, 2, 1],
        ')' => [4, 2, 2, 2, 4],
        '%' => [5, 1, 2, 4, 5],
        '+' => [0, 2, 7, 2, 0],
        ',' => [0, 0, 0, 2, 4],
        '\'' => [2, 2, 0, 0, 0],
        '_' => [0, 0, 0, 0, 7],
        _ => [0, 0, 0, 0, 2],
    }
}

/// Draws `text` in the 3x5 font at double size: 8 px per character, 10 px tall.
pub(crate) fn caption(image: &mut image::RgbaImage, x: u32, y: u32, text: &str) {
    let (width, height) = image.dimensions();
    let mut left = x;
    for ch in text.chars() {
        for (row, bits) in glyph(ch).iter().enumerate() {
            for col in 0..3 {
                if bits & (4 >> col) != 0 {
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let (px, py) = (left + col * 2 + dx, y + row as u32 * 2 + dy);
                        if px < width && py < height {
                            image.put_pixel(px, py, image::Rgba([220, 220, 220, 255]));
                        }
                    }
                }
            }
        }
        left += 8;
    }
}

#[cfg(test)]
mod tests {
    use super::{Codec, Encoding};

    #[test]
    fn video_bit_rate_follows_frame_size_within_bounds() {
        assert_eq!(Encoding::HIGH.codec(1144, 928, 25.0), Codec::H264 { bitrate: 7_962_240 });
        assert_eq!(Encoding::HIGH.codec(160, 120, 25.0), Codec::H264 { bitrate: 2_000_000 });
        assert_eq!(Encoding::MAX.codec(4096, 2304, 60.0), Codec::H264 { bitrate: 120_000_000 });
        assert_eq!(Encoding::H264Bitrate(9_000_000).codec(64, 64, 1.0), Codec::H264 { bitrate: 9_000_000 });
        assert_eq!(Encoding::ProRes4444.codec(64, 64, 1.0), Codec::ProRes4444);
    }

    #[test]
    fn timestamps_read_minutes_seconds_and_hundredths() {
        assert_eq!(super::timestamp(0.0), "0:00.00");
        assert_eq!(super::timestamp(83_400.0), "1:23.40");
        assert_eq!(super::timestamp(5_999.0), "0:06.00");
    }
}
