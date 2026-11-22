//! Deterministic CPU inspection frames using the real preview geometry, links and rendered schedule.
//! Fixtures and background artwork are reported as omitted, not silently synthesized.
use crate::{app::AppCore, sequence::SequenceSession};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::{collections::HashSet, path::Path, sync::Arc};
use uuid::Uuid;
use vixen_core::{CombinedState, TimeSpan};
use vixen_engine::{EvalScratch, Schedule};

#[path = "studio_video.rs"]
mod video;

#[path = "studio_geometry.rs"]
pub mod geometry;

const OMITTED: [&str; 3] = ["background artwork", "fixture beams", "GPU antialiasing"];

/// One linked light, resolved once so each frame only evaluates the schedule and paints.
struct Light {
    x: f32,
    y: f32,
    radius: f32,
    index: usize,
    element: Uuid,
    discrete: Option<[u8; 3]>,
}

/// A preview's lights bound to one rendered schedule. Holds no session lock, so long renders leave the editor free.
pub struct Renderer {
    lights: Vec<Light>,
    total: usize,
    width: u32,
    height: u32,
    scale: f32,
    glow: bool,
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
                x: light.x * scale,
                y: light.y * scale,
                radius: (light.size / 2.0).clamp(0.5, 32.0) * scale,
                index: index as usize,
                element,
                discrete,
            });
        }
        for light in &mut lights {
            light.radius = light.radius.max(0.75);
        }
        Ok(Self {
            lights,
            total: scene.lights.len(),
            width: ((width as f32 * scale).round() as u32).max(16),
            height: ((height as f32 * scale).round() as u32).max(16),
            scale,
            glow,
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

    /// Keeps only the lights of `elements` and crops to them, scaled to fit `max_width` by `max_height` (at most four
    /// times their preview size), so one prop fills a thumbnail. Call on a renderer built at scale 1. Returns how many
    /// lights are left; with none the size is unchanged.
    pub fn focus(&mut self, elements: &HashSet<Uuid>, max_width: u32, max_height: u32) -> usize {
        self.lights.retain(|l| elements.contains(&l.element));
        self.total = self.lights.len();
        if self.lights.is_empty() {
            return 0;
        }
        let reach = self.lights.iter().map(|l| l.radius).fold(0.0, f32::max) * if self.glow { 3.0 } else { 1.0 } + 2.0;
        let left = self.lights.iter().map(|l| l.x).fold(f32::MAX, f32::min) - reach;
        let top = self.lights.iter().map(|l| l.y).fold(f32::MAX, f32::min) - reach;
        let width = self.lights.iter().map(|l| l.x).fold(f32::MIN, f32::max) + reach - left;
        let height = self.lights.iter().map(|l| l.y).fold(f32::MIN, f32::max) + reach - top;
        let scale = (max_width as f32 / width).min(max_height as f32 / height).min(4.0);
        for light in &mut self.lights {
            light.x = (light.x - left) * scale;
            light.y = (light.y - top) * scale;
            light.radius = (light.radius * scale).max(0.75);
        }
        self.scale = scale;
        self.width = ((width * scale).round() as u32).max(16);
        self.height = ((height * scale).round() as u32).max(16);
        self.total
    }

    /// Paints the lights at `time_ms` onto black, returning how many were lit.
    pub fn paint(&mut self, time_ms: f64, image: &mut image::RgbaImage) -> usize {
        for pixel in image.pixels_mut() {
            *pixel = image::Rgba([0, 0, 0, 255]);
        }
        self.schedule.evaluate(
            TimeSpan::from_ms(time_ms),
            &mut self.scratch,
            &mut self.states,
        );
        let (width, height) = image.dimensions();
        let mut illuminated = 0;
        for light in &self.lights {
            let Some(state) = self.states.get(light.index) else {
                continue;
            };
            let [r, g, b, a] = crate::preview::light_color(state, light.discrete).to_le_bytes();
            if a == 0 {
                continue;
            }
            illuminated += 1;
            if self.glow {
                // A soft additive halo, so dense strings read as light rather than dots.
                let reach = (light.radius * 3.0).max(2.0 * self.scale.max(0.5));
                paint_disc(image, width, height, light.x, light.y, reach, |pixel, d| {
                    let falloff = (1.0 - d / reach).powi(2) * 0.35 * f32::from(a) / 255.0;
                    for (i, c) in [r, g, b].into_iter().enumerate() {
                        pixel[i] = (f32::from(pixel[i]) + f32::from(c) * falloff).min(255.0) as u8;
                    }
                });
            }
            paint_disc(
                image,
                width,
                height,
                light.x,
                light.y,
                light.radius,
                |pixel, _| {
                    for (i, c) in [r, g, b].into_iter().enumerate() {
                        pixel[i] = ((u32::from(c) * u32::from(a)
                            + u32::from(pixel[i]) * (255 - u32::from(a)))
                            / 255) as u8;
                    }
                },
            );
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

/// Renders the range to an H.264 MP4, with the sequence audio for the same span as AAC when `audio` is given.
/// The session lock is held only to render effects and take the schedule.
pub fn video(
    core: &AppCore,
    shared: &Arc<Mutex<SequenceSession>>,
    range: &Range,
    scale: f32,
    audio: Option<&Path>,
    path: &Path,
) -> Result<Value, String> {
    let mut renderer = Renderer::new(
        core,
        &mut shared.lock(),
        range.preview_id,
        scale,
        range.glow,
    )?;
    let times: Vec<f64> = range.times().collect();
    let started = std::time::Instant::now();
    let (frames, audio, width, height, lit) = encode(&mut renderer, &times, range.fps, range.start_ms, range.end_ms, audio, path, &mut |_, _| {})?;
    Ok(json!({
        "path": path,
        "width": width,
        "height": height,
        "fps": range.fps,
        "frames": frames,
        "startMs": range.start_ms,
        "endMs": range.end_ms,
        "audio": audio,
        "lights": renderer.total,
        "maxIlluminated": lit,
        "seconds": (started.elapsed().as_secs_f64() * 100.0).round() / 100.0,
        "codec": "H.264 video, AAC audio, MP4",
        "renderer": "CPU inspection frames using real scene/schedule",
        "omitted": OMITTED,
    }))
}

/// Paints `times` from `renderer` into an H.264 MP4 (with `audio` for the span as AAC when given). Returns what was
/// written, the even frame size and the most lights lit in one frame. `overlay` draws on each frame after the lights.
pub fn encode(
    renderer: &mut Renderer,
    times: &[f64],
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
    #[test]
    fn timestamps_read_minutes_seconds_and_hundredths() {
        assert_eq!(super::timestamp(0.0), "0:00.00");
        assert_eq!(super::timestamp(83_400.0), "1:23.40");
        assert_eq!(super::timestamp(5_999.0), "0:06.00");
    }
}
