//! effect_gallery (#62): several effect variants rendered on one prop and shown side by side, cropped to the prop's own
//! preview geometry, so their looks can be compared before anything is placed. The variants play one after another on a
//! throwaway sequence that is never registered, saved or put on any Undo; the user's sequences are not touched.
use super::{apply_add, confirm, filename, output, plan_add, string, uuid};
use crate::{
    app::AppCore,
    sequence::SequenceSession,
    studio_preview::{self, Renderer},
};
use serde_json::{json, Value};
use std::collections::HashSet;
use uuid::Uuid;
use vixen_core::TimeSpan;

pub const MAX_VARIANTS: usize = 24;
const LABEL_CHARS: usize = 22;

pub const DESCRIPTION: &str = "Render several effect variants on one prop and compare them side by side, as a PNG contact sheet (one row per variant, frames across, labelled) or an MP4 (the variants one after another, each labelled), under profile Export. Each variant is an effect type (typeId, or effect by name as effect_catalog lists it) with optional params (as effect_add takes them) and a label. Frames are cropped to the target's lights in its own preview, so a megatree fills each tile; previewId picks the preview when the target is in several (default: the one showing most of its lights). Nothing is added to any sequence: the variants play on a throwaway sequence. Each variant in the result carries place (typeId, params, durationMs) for effect_add, and warnings name variants that lit nothing (Wipe, for one, only lights elements with Vixen's Location property). Defaults: 2000 ms per variant, 6 frames, 160 px tiles (320 for video), 25 fps, glow off. Up to 24 variants. Requires confirmed output filename (.png for a sheet, .mp4 for a video).";

pub fn schema() -> Value {
    let string = json!({"type":"string","minLength":1,"maxLength":256});
    json!({
        "sequenceId": string,
        "targetId": string,
        "previewId": string,
        "variants": {"type":"array","minItems":1,"maxItems":MAX_VARIANTS,"items":{"type":"object","properties":{"typeId":string,"effect":string,"params":{"type":"object"},"label":string},"additionalProperties":false}},
        "durationMs": {"type":"number","minimum":250,"maximum":10000},
        "frames": {"type":"integer","minimum":1,"maximum":16},
        "tileWidth": {"type":"integer","minimum":48,"maximum":640},
        "format": {"type":"string","enum":["sheet","video"]},
        "fps": {"type":"integer","minimum":1,"maximum":60},
        "glow": {"type":"boolean"},
        "file": string,
        "confirm": {"type":"boolean"},
    })
}

/// One variant as asked for: an effect type, its parameters and the label shown with it.
pub struct Variant {
    pub label: String,
    pub type_id: Uuid,
    pub effect: &'static str,
    pub params: Value,
}

/// A gallery ready to paint: the variants back to back on a rendered throwaway sequence, the renderer focused on the
/// target's lights.
pub struct Gallery {
    pub variants: Vec<Variant>,
    pub duration_ms: f64,
    pub target: (Uuid, String),
    pub preview: (Uuid, String),
    pub renderer: Renderer,
}

impl Gallery {
    /// Where variant `index` plays on the throwaway sequence.
    pub fn span(&self, index: usize) -> (f64, f64) {
        let start = index as f64 * self.duration_ms;
        (start, start + self.duration_ms)
    }

    /// `count` frame times spread evenly through variant `index`, each in the middle of its slice.
    pub fn times(&self, index: usize, count: usize) -> Vec<f64> {
        let (start, _) = self.span(index);
        (0..count).map(|k| start + self.duration_ms * (k as f64 + 0.5) / count as f64).collect()
    }
}

/// Resolves the target, preview and variants, plays the variants on a throwaway sequence and focuses a renderer on the
/// target's lights, fitting `tile_width` (and at most twice that in height).
pub fn prepare(core: &AppCore, a: &Value, tile_width: u32, glow: bool) -> Result<Gallery, String> {
    let target = uuid(a, "targetId")?;
    let (target_name, elements) = {
        let system = core.system.read();
        let node = system.tree.find(&target).ok_or(format!("Target {target} not found"))?;
        let elements: HashSet<Uuid> = system
            .tree
            .leaves(node)
            .into_iter()
            .chain([node])
            .filter_map(|n| system.tree.node(n).element_id)
            .collect();
        (system.tree.node(node).name.clone(), elements)
    };
    let linking = studio_preview::previews_linking(core, &elements);
    let preview = match a.get("previewId") {
        Some(_) => {
            let id = uuid(a, "previewId")?;
            let (id, name, _) = linking
                .iter()
                .find(|p| p.0 == id)
                .ok_or(format!("That preview doesn't show any of {target_name}'s lights"))?;
            (*id, name.clone())
        }
        None => linking
            .first()
            .map(|(id, name, _)| (*id, name.clone()))
            .ok_or(format!("No preview shows {target_name}'s lights; link it in a preview first (preview_prop_create with elementId)"))?,
    };
    let variants = variants(a)?;
    let duration_ms = match a.get("durationMs") {
        Some(v) => v.as_f64().filter(|d| (250.0..=10_000.0).contains(d)).ok_or("durationMs must be 250..10000")?,
        None => 2000.0,
    };
    let mut session = SequenceSession::new_untitled(&core.session_environment());
    session.set_length(TimeSpan::from_ms(duration_ms * variants.len() as f64));
    for (i, variant) in variants.iter().enumerate() {
        let mut add = json!({"targetId":target.to_string(),"typeId":variant.type_id.to_string(),"startMs":i as f64 * duration_ms,"durationMs":duration_ms});
        if !variant.params.is_null() {
            add["params"] = variant.params.clone();
        }
        for new in plan_add(core, &session, &add).map_err(|e| format!("variants[{i}]: {e}"))? {
            apply_add(&mut session, new);
        }
    }
    let mut renderer = Renderer::new(core, &mut session, preview.0, 1.0, glow)?;
    if renderer.focus(&elements, tile_width, tile_width * 2) == 0 {
        return Err(format!("{} doesn't show {target_name}'s lights", preview.1));
    }
    Ok(Gallery { variants, duration_ms, target: (target, target_name), preview, renderer })
}

/// The variants as asked for, each effect looked up by typeId or by name; repeated labels are numbered.
fn variants(a: &Value) -> Result<Vec<Variant>, String> {
    let list = a["variants"]
        .as_array()
        .filter(|l| (1..=MAX_VARIANTS).contains(&l.len()))
        .ok_or(format!("variants must list 1..{MAX_VARIANTS} variants"))?;
    let mut variants: Vec<Variant> = Vec::new();
    for (i, v) in list.iter().enumerate() {
        let fields = v.as_object().ok_or(format!("variants[{i}] must be an object"))?;
        if let Some(extra) = fields.keys().find(|k| !["typeId", "effect", "params", "label"].contains(&k.as_str())) {
            return Err(format!("variants[{i}]: {extra} is not a variant field (typeId, effect, params, label)"));
        }
        let descriptor = match (v.get("typeId"), v.get("effect")) {
            (Some(_), None) => vixen_effect_registry::by_type_id(&uuid(v, "typeId").map_err(|e| format!("variants[{i}]: {e}"))?),
            (None, Some(_)) => {
                let name = string(v, "effect").map_err(|e| format!("variants[{i}]: {e}"))?;
                vixen_effect_registry::all().iter().find(|d| d.name.eq_ignore_ascii_case(name)).copied()
            }
            _ => return Err(format!("variants[{i}]: give typeId or effect")),
        }
        .ok_or(format!("variants[{i}]: effect type not found (effect_catalog lists them)"))?;
        let params = v.get("params").cloned().unwrap_or(Value::Null);
        if !params.is_null() && !params.is_object() {
            return Err(format!("variants[{i}]: params must be an object"));
        }
        let label = match v.get("label") {
            Some(_) => string(v, "label").map_err(|e| format!("variants[{i}]: {e}"))?.to_string(),
            None => descriptor.name.to_string(),
        };
        variants.push(Variant { label, type_id: descriptor.type_id, effect: descriptor.name, params });
    }
    let labels: Vec<String> = variants.iter().map(|v| v.label.clone()).collect();
    for (i, variant) in variants.iter_mut().enumerate() {
        if labels.iter().filter(|l| **l == labels[i]).count() > 1 {
            let nth = labels[..i].iter().filter(|l| **l == labels[i]).count() + 1;
            variant.label = format!("{} #{nth}", labels[i]);
        }
    }
    Ok(variants)
}

/// A label cut to `chars` characters, ending in '.' when cut.
fn fit(label: &str, chars: usize) -> String {
    if label.chars().count() <= chars {
        return label.to_string();
    }
    let mut cut: String = label.chars().take(chars.saturating_sub(1)).collect();
    cut.push('.');
    cut
}

/// The tool: a labelled contact sheet or MP4 under Export.
pub fn execute(core: &AppCore, a: &Value) -> Result<Value, String> {
    confirm(a)?;
    let video = a["format"] == "video";
    let file = filename(a, "file")?;
    let extension = if video { ".mp4" } else { ".png" };
    if !file.to_lowercase().ends_with(extension) {
        return Err(format!("Use {extension} extension for a {}", if video { "video" } else { "sheet" }));
    }
    let path = output(core, "Export", &file)?;
    let whole = |k: &str, default: u32, range: std::ops::RangeInclusive<u32>| {
        a.get(k)
            .map_or(Some(default), |v| v.as_u64().and_then(|n| u32::try_from(n).ok()))
            .filter(|n| range.contains(n))
            .ok_or(format!("{k} must be a whole number in {}..{}", range.start(), range.end()))
    };
    let tile_width = whole("tileWidth", if video { 320 } else { 160 }, 48..=640)?;
    let frames = whole("frames", 6, 1..=16)? as usize;
    let fps = whole("fps", 25, 1..=60)?;
    let mut gallery = prepare(core, a, tile_width, a["glow"] == true)?;
    let (tile_w, tile_h) = gallery.renderer.size();
    let mut lit = vec![0usize; gallery.variants.len()];
    let mut layout = json!({});
    if video {
        let end = gallery.span(gallery.variants.len() - 1).1;
        let times: Vec<f64> = (0..(end * f64::from(fps) / 1000.0).ceil() as usize).map(|i| i as f64 * 1000.0 / f64::from(fps)).collect();
        let labels: Vec<String> = gallery.variants.iter().map(|v| fit(&v.label, (tile_w as usize / 8).max(1))).collect();
        let duration = gallery.duration_ms;
        let (written, _, width, height, _) = studio_preview::encode(
            &mut gallery.renderer,
            &times,
            f64::from(fps),
            0.0,
            end,
            None,
            &path,
            &mut |time, image| {
                let index = ((time / duration) as usize).min(labels.len() - 1);
                studio_preview::caption(image, 2, 2, &labels[index]);
            },
        )?;
        layout = json!({"width":width,"height":height,"fps":fps,"frames":written,"codec":"H.264 video, MP4, no audio"});
        let mut scratch = image::RgbaImage::new(tile_w, tile_h);
        for (i, lit) in lit.iter_mut().enumerate() {
            for time in gallery.times(i, frames) {
                *lit = (*lit).max(gallery.renderer.paint(time, &mut scratch));
            }
        }
    } else {
        const GAP: u32 = 2;
        const LINE: u32 = 12;
        let chars = gallery.variants.iter().map(|v| v.label.chars().count()).max().unwrap_or(1).min(LABEL_CHARS);
        let label_w = chars as u32 * 8 + 6;
        let (cell_w, cell_h) = (tile_w + GAP, tile_h + GAP);
        let rows = gallery.variants.len() as u32;
        let (width, height) = (label_w + frames as u32 * cell_w + GAP, LINE + GAP + rows * cell_h + GAP);
        if width > 8192 || height > 8192 {
            return Err(format!("Sheet would be {width}x{height}; keep it within 8192 (fewer frames or variants, or a smaller tileWidth)"));
        }
        let mut sheet = image::RgbaImage::from_pixel(width, height, image::Rgba([40, 40, 40, 255]));
        let mut tile = image::RgbaImage::new(tile_w, tile_h);
        // Column headers: where in each variant the frame is, in seconds.
        for (k, offset) in gallery.times(0, frames).iter().enumerate() {
            let x = label_w + GAP + k as u32 * cell_w;
            studio_preview::caption(&mut sheet, x + 1, 1, &format!("{:.2}S", offset / 1000.0));
        }
        for i in 0..gallery.variants.len() {
            let y = LINE + GAP + i as u32 * cell_h;
            studio_preview::caption(&mut sheet, 3, y + tile_h.saturating_sub(10) / 2, &fit(&gallery.variants[i].label, chars));
            for (k, time) in gallery.times(i, frames).into_iter().enumerate() {
                lit[i] = lit[i].max(gallery.renderer.paint(time, &mut tile));
                image::imageops::replace(&mut sheet, &tile, i64::from(label_w + GAP + k as u32 * cell_w), i64::from(y));
            }
        }
        sheet.save(&path).map_err(|e| e.to_string())?;
        layout = json!({"width":width,"height":height,"rows":rows,"columns":frames,"frameOffsetsMs":gallery.times(0, frames)});
    }
    let mut warnings = Vec::new();
    let variants: Vec<Value> = gallery
        .variants
        .iter()
        .enumerate()
        .map(|(i, v)| {
            if lit[i] == 0 {
                warnings.push(format!("{} lit none of {}'s lights{}", v.label, gallery.target.1, if v.effect == "Wipe" { " (Wipe only lights elements with Vixen's Location property; element_location gives them one)" } else { "" }));
            }
            let (start, end) = gallery.span(i);
            let mut place = json!({"typeId":v.type_id.to_string(),"durationMs":gallery.duration_ms});
            if !v.params.is_null() {
                place["params"] = v.params.clone();
            }
            let mut row = json!({"index":i,"label":v.label,"effect":v.effect,"typeId":v.type_id.to_string(),"maxIlluminated":lit[i],"place":place});
            if video {
                row["startMs"] = json!(start);
                row["endMs"] = json!(end);
            } else {
                row["row"] = json!(i);
            }
            row
        })
        .collect();
    let mut result = json!({
        "path": path,
        "format": if video { "video" } else { "sheet" },
        "target": {"id": gallery.target.0.to_string(), "name": gallery.target.1},
        "previewId": gallery.preview.0.to_string(),
        "previewName": gallery.preview.1,
        "lights": gallery.renderer.lights(),
        "tileWidth": tile_w,
        "tileHeight": tile_h,
        "durationMs": gallery.duration_ms,
        "variants": variants,
        "warnings": warnings,
        "next": "Place a variant with effect_add: its place fields plus sequenceId, targetId and startMs",
        "renderer": "CPU inspection frames of the target's lights in its preview, from a throwaway sequence",
        "omitted": ["background artwork", "fixture beams", "GPU antialiasing"],
    });
    result.as_object_mut().unwrap().extend(layout.as_object().unwrap().clone());
    Ok(result)
}

/// One variant's frames as PNG images, for the sequencer's Effect Gallery sheet.
pub struct Thumbnails {
    pub label: String,
    pub type_id: Uuid,
    pub frames: Vec<Vec<u8>>,
    pub illuminated: usize,
}

/// The sequencer's Effect Gallery: `frames` PNG frames per variant (in order, evenly through the variant), to loop.
/// Takes effect_gallery's targetId, previewId, variants and durationMs.
pub fn thumbnails(core: &AppCore, a: &Value, frames: usize, tile_width: u32) -> Result<(Vec<Thumbnails>, String), String> {
    let mut gallery = prepare(core, a, tile_width, true)?;
    let (w, h) = gallery.renderer.size();
    let mut image = image::RgbaImage::new(w, h);
    let mut out = Vec::new();
    for i in 0..gallery.variants.len() {
        let mut pngs = Vec::new();
        let mut lit = 0;
        for time in gallery.times(i, frames) {
            lit = lit.max(gallery.renderer.paint(time, &mut image));
            let mut png = std::io::Cursor::new(Vec::new());
            image.write_to(&mut png, image::ImageFormat::Png).map_err(|e| e.to_string())?;
            pngs.push(png.into_inner());
        }
        let v = &gallery.variants[i];
        out.push(Thumbnails { label: v.label.clone(), type_id: v.type_id, frames: pngs, illuminated: lit });
    }
    Ok((out, gallery.preview.1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AppCore;

    fn tree_in_preview() -> (AppCore, std::path::PathBuf, String, String) {
        let root = std::env::temp_dir().join(format!("studio-gallery-{}", Uuid::new_v4()));
        let core = AppCore::open(root.clone(), true).unwrap();
        let run = |name: &str, args: Value| core.studio_display.execute(&core, name, &args, false).unwrap();
        let preview = run("preview_create", json!({"name":"Yard"}))["result"]["previewId"].as_str().unwrap().to_string();
        run("preview_create", json!({"name":"Empty"}));
        let tree = run("element_template", json!({"kind":"megatree","name":"Tree","count":8,"pixels":10,"previewId":preview}))["result"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        (core, root, preview, tree)
    }

    #[test]
    fn gallery_renders_each_variant_on_the_prop_without_touching_sequences() {
        let (core, root, preview, tree) = tree_in_preview();
        let run = |args: Value| super::super::execute(&core, "effect_gallery", &args, true);
        let variants = json!([
            {"effect":"set level","params":{"Color":"#ff0000"}},
            {"effect":"Set Level","params":{"Color":"#0000ff"}},
            {"effect":"Chase","label":"Chase"},
            {"effect":"Wipe"},
        ]);
        let sheet = run(json!({"targetId":tree,"variants":variants,"frames":4,"tileWidth":100,"file":"gallery.png","confirm":true})).unwrap();
        assert_eq!(sheet["previewId"], preview.as_str(), "the preview that shows the tree, not the empty one");
        assert_eq!(sheet["lights"], 80);
        assert_eq!(sheet["rows"], 4);
        assert_eq!(sheet["columns"], 4);
        let labels: Vec<&str> = sheet["variants"].as_array().unwrap().iter().map(|v| v["label"].as_str().unwrap()).collect();
        assert_eq!(labels, ["Set Level #1", "Set Level #2", "Chase", "Wipe"]);
        assert_eq!(sheet["variants"][0]["maxIlluminated"], 80, "{sheet}");
        assert_eq!(sheet["variants"][0]["place"]["params"]["Color"], "#ff0000");
        assert_eq!(sheet["variants"][0]["place"]["durationMs"], 2000.0);
        assert!(sheet["variants"][2]["maxIlluminated"].as_u64().unwrap() > 0, "{sheet}");
        assert_eq!(sheet["variants"][3]["maxIlluminated"], 0, "no Location property, so the Wipe stays dark");
        assert!(sheet["warnings"][0].as_str().unwrap().contains("Location property"), "{sheet}");
        let tile = (sheet["tileWidth"].as_u64().unwrap(), sheet["tileHeight"].as_u64().unwrap());
        assert!(tile.0 <= 100 && tile.1 <= 200 && (tile.0 >= 99 || tile.1 >= 199), "cropped to the tree and fitted: {tile:?}");
        let png = image::open(root.join("Export/gallery.png")).unwrap().to_rgba8();
        assert_eq!((u64::from(png.width()), u64::from(png.height())), (sheet["width"].as_u64().unwrap(), sheet["height"].as_u64().unwrap()));
        // The first row's first frame is solid red on the tree.
        let x0 = png.width() - 4 * (tile.0 as u32 + 2) - 2;
        let red = (0..tile.1 as u32).flat_map(|y| (0..tile.0 as u32).map(move |x| (x, y))).filter(|(x, y)| png.get_pixel(x0 + x, 14 + y)[0] > 200).count();
        assert!(red > 0, "row 1 is lit red");
        assert!(core.open_sequence_sessions().is_empty(), "the throwaway sequence is never registered");

        let video = run(json!({"targetId":tree,"variants":[{"effect":"Chase"},{"effect":"Set Level"}],"format":"video","durationMs":500,"fps":10,"file":"gallery.mp4","confirm":true})).unwrap();
        assert_eq!(video["frames"], 10, "{video}");
        assert_eq!(video["variants"][1]["startMs"], 500.0);
        assert!(root.join("Export/gallery.mp4").metadata().unwrap().len() > 0);

        let thumbs = thumbnails(&core, &json!({"targetId":tree,"variants":[{"effect":"Chase"}]}), 3, 80).unwrap();
        assert_eq!(thumbs.1, "Yard");
        assert_eq!(thumbs.0[0].frames.len(), 3);
        assert_eq!(&thumbs.0[0].frames[0][1..4], b"PNG");

        let fail = |args: Value| run(args).unwrap_err();
        assert!(fail(json!({"targetId":tree,"variants":[{"effect":"Nope"}],"file":"g.png","confirm":true})).contains("variants[0]"));
        assert!(fail(json!({"targetId":tree,"variants":[{"effect":"Chase","params":{"Bogus":1}}],"file":"g.png","confirm":true})).starts_with("variants[0]"));
        assert!(fail(json!({"targetId":tree,"variants":[{"effect":"Chase"}],"format":"video","file":"g.png","confirm":true})).contains(".mp4"));
        assert!(fail(json!({"targetId":tree,"variants":[{"effect":"Chase"}],"file":"g.png"})).contains("confirm"));
        let other = core.studio_display.execute(&core, "element_template", &json!({"kind":"arch","name":"Arch","count":1,"pixels":10}), false).unwrap()["result"]["id"].clone();
        assert!(fail(json!({"targetId":other,"variants":[{"effect":"Chase"}],"file":"g.png","confirm":true})).contains("No preview shows Arch"));
        core.studio_display.stop();
        drop(core);
        let _ = std::fs::remove_dir_all(root);
    }
}
