//! Deterministic CPU inspection frame using the real preview geometry, links and rendered schedule.
//! Fixtures and background artwork are reported as omitted, not silently synthesized.
use crate::{app::AppCore, sequence::SequenceSession};
use serde_json::{json, Value};
use std::path::Path;
use uuid::Uuid;
use vixen_core::TimeSpan;

pub fn frame(
    core: &AppCore,
    session: &mut SequenceSession,
    preview_id: Uuid,
    time_ms: f64,
    path: &Path,
) -> Result<Value, String> {
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
    let mut states = Vec::new();
    let mut scratch = Default::default();
    schedule.evaluate(TimeSpan::from_ms(time_ms), &mut scratch, &mut states);
    let resolved = scene.resolve_nodes(&system.tree);
    let mut image = image::RgbaImage::from_pixel(width, height, image::Rgba([0, 0, 0, 255]));
    let mut illuminated = 0;
    for (light, link) in scene.lights.iter().zip(&scene.links) {
        let Some(Some(node)) = resolved.get(link.node as usize).copied() else {
            continue;
        };
        let Some(index) = system
            .tree
            .node(node)
            .element_id
            .and_then(|e| schedule.elements().index_of(&e))
        else {
            continue;
        };
        let Some(state) = states.get(index as usize) else {
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
        let [r, g, b, a] = crate::preview::light_color(state, discrete).to_le_bytes();
        if a == 0 {
            continue;
        }
        illuminated += 1;
        let radius = (light.size / 2.0).clamp(0.5, 32.0);
        let left = (light.x - radius).floor().max(0.0) as u32;
        let right = (light.x + radius).ceil().min(width as f32) as u32;
        let top = (light.y - radius).floor().max(0.0) as u32;
        let bottom = (light.y + radius).ceil().min(height as f32) as u32;
        for y in top..bottom {
            for x in left..right {
                if (x as f32 - light.x).powi(2) + (y as f32 - light.y).powi(2) <= radius * radius {
                    let pixel = image.get_pixel_mut(x, y);
                    for (i, c) in [r, g, b].into_iter().enumerate() {
                        pixel[i] = ((u32::from(c) * u32::from(a)
                            + u32::from(pixel[i]) * (255 - u32::from(a)))
                            / 255) as u8;
                    }
                }
            }
        }
    }
    image.save(path).map_err(|e| e.to_string())?;
    Ok(
        json!({"path":path,"width":width,"height":height,"lights":scene.lights.len(),"illuminated":illuminated,"timeMs":time_ms,"renderer":"CPU inspection frame using real scene/schedule","omitted":["background artwork","fixture beams","GPU antialiasing"]}),
    )
}
