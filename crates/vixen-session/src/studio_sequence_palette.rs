//! A palette per sequence (B81): the colours a show is built from. `sequence_palette` sets, clears or reads it; effect adds
//! and parameter edits that bring in a colour outside it say so (`paletteWarnings`), and `render_analyze` checks the
//! rendered sequence against it when no palette is passed. It lives in the build manifest beside the `.tim`
//! (`Builds::palette`), so it is one undo step, kept by labelled saves and invisible to Windows Vixen.
use super::{record, string};
use crate::sequence::{EffectEntry, EffectId, SequenceSession};
use crate::studio_preview::taste;
use crate::undo::UndoAction;
use serde_json::{json, Value};

/// How far (full-brightness RGB, 0 to 1 per channel) a colour may be from a palette colour, as render_analyze's default.
pub const DEFAULT_DISTANCE: f64 = 0.35;
const MAX_COLORS: usize = 32;
/// Effects listed in a report or a warning; counts are always complete.
const LISTED: usize = 50;

pub(super) fn tools() -> Vec<(&'static str, &'static str, Value, Vec<&'static str>, bool)> {
    let string = json!({"type":"string","minLength":1,"maxLength":256});
    vec![(
        "sequence_palette",
        "The sequence's palette: the colours it is built from, saved with the sequence (beside the .tim, so Windows Vixen ignores it). colors sets it (#RRGGBB, 1..32; black is never off-palette), distance how far a colour may stray from the nearest palette colour (full-brightness RGB, 0 to 1 per channel, default 0.35; colours are compared at full brightness, so a dim red is still red), clear:true removes it; with none of them it only reads. Setting or clearing is one undo step. Returns palette, distance and offPalette: the effects whose colour parameters fall outside it ({effectId, name, startMs, colors}, at most 50; offPaletteCount counts all). While a palette is set, effect_add, effect_band, effects_batch, section_rebuild and effect_set_parameter add paletteWarnings to their result for effects they bring in with colours outside it, and render_analyze checks against it when called without palette. Example: {sequenceId, colors:[\"#ff0000\",\"#ffffff\",\"#00ff00\"]}.",
        json!({"sequenceId":string,"colors":{"type":"array","items":{"type":"string","pattern":"^#[0-9A-Fa-f]{6}$"},"minItems":1,"maxItems":MAX_COLORS,"examples":[["#ff0000","#ffffff","#00ff00"]]},"distance":{"type":"number","minimum":0.01,"maximum":2,"examples":[0.35]},"clear":{"type":"boolean","examples":[true]}}),
        vec!["sequenceId"],
        false,
    )]
}

/// The palette render_analyze falls back to: colours and distance (None when the sequence has none).
pub(super) fn current(s: &SequenceSession) -> Option<(Vec<String>, f64)> {
    (!s.studio_builds.palette.is_empty()).then(|| (s.studio_builds.palette.clone(), s.studio_builds.palette_distance.unwrap_or(DEFAULT_DISTANCE)))
}

pub(super) fn execute(s: &mut SequenceSession, a: &Value, agent: bool) -> Result<Value, String> {
    let _ = string(a, "sequenceId")?;
    if let Some(extra) = a.as_object().and_then(|o| o.keys().find(|k| !["sequenceId", "colors", "distance", "clear"].contains(&k.as_str())).cloned()) {
        return Err(format!("Unknown argument {extra}"));
    }
    let clear = match a.get("clear") {
        None => false,
        Some(v) => v.as_bool().ok_or("clear must be true or false")?,
    };
    if clear && (a.get("colors").is_some() || a.get("distance").is_some()) {
        return Err("clear:true removes the palette; give it alone".into());
    }
    let distance = a.get("distance").map(|v| v.as_f64().filter(|d| (0.01..=2.0).contains(d)).ok_or("distance must be a number in 0.01..2")).transpose()?;
    let colors = match a.get("colors") {
        None => None,
        Some(list) => {
            let list = list.as_array().filter(|l| (1..=MAX_COLORS).contains(&l.len())).ok_or(format!("colors must list 1..{MAX_COLORS} #RRGGBB colours"))?;
            let mut out: Vec<String> = Vec::new();
            for c in list {
                let text = c.as_str().filter(|t| t.len() == 7 && taste::parse_hex(t).is_some()).ok_or("colors must be #RRGGBB colours, not black (black is never off-palette)")?.to_ascii_lowercase();
                if !out.contains(&text) {
                    out.push(text);
                }
            }
            Some(out)
        }
    };
    if distance.is_some() && colors.is_none() && s.studio_builds.palette.is_empty() {
        return Err("distance needs a palette: give colors too".into());
    }
    let mut changed = false;
    if clear || colors.is_some() || distance.is_some() {
        let mut next = s.studio_builds.clone();
        if clear {
            next.palette.clear();
            next.palette_distance = None;
        }
        if let Some(c) = colors {
            next.palette = c;
        }
        if distance.is_some() {
            next.palette_distance = distance;
        }
        if next != s.studio_builds {
            let label = if clear { "Clear palette" } else { "Set palette" };
            let old = std::mem::replace(&mut s.studio_builds, next);
            record(s, label, UndoAction::StudioBuilds { label: label.into(), builds: old }, agent);
            changed = true;
        }
    }
    let ids: Vec<EffectId> = s.effects().map(|e| e.id).collect();
    let (listed, count) = off_palette(s, &ids);
    Ok(json!({
        "palette": s.studio_builds.palette,
        "distance": s.studio_builds.palette_distance.unwrap_or(DEFAULT_DISTANCE),
        "changed": changed,
        "offPalette": listed,
        "offPaletteCount": count,
    }))
}

/// Adds `paletteWarnings` (and `paletteWarningCount`) to an editing tool's result for the effects it names (effectId and
/// effectIds, plus `extra`) that use colours outside the sequence's palette. Without a palette the result is unchanged.
pub(super) fn warn(s: &SequenceSession, mut result: Value, extra: &[u64]) -> Value {
    if s.studio_builds.palette.is_empty() {
        return result;
    }
    let mut ids: Vec<u64> = result["effectIds"].as_array().map(|l| l.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
    ids.extend(result["effectId"].as_u64());
    ids.extend(extra);
    ids.sort_unstable();
    ids.dedup();
    let ids: Vec<EffectId> = ids.into_iter().map(EffectId).collect();
    let (listed, count) = off_palette(s, &ids);
    if count > 0 {
        result["paletteWarnings"] = json!(listed);
        result["paletteWarningCount"] = json!(count);
        result["palette"] = json!(s.studio_builds.palette);
    }
    result
}

/// The effects among `ids` with a colour parameter outside the palette: up to [`LISTED`] of them, and how many in all.
fn off_palette(s: &SequenceSession, ids: &[EffectId]) -> (Vec<Value>, usize) {
    let palette: Vec<[f32; 3]> = s.studio_builds.palette.iter().filter_map(|h| taste::parse_hex(h)).collect();
    if palette.is_empty() {
        return (Vec::new(), 0);
    }
    let limit = s.studio_builds.palette_distance.unwrap_or(DEFAULT_DISTANCE) as f32;
    let mut listed = Vec::new();
    let mut count = 0;
    for entry in ids.iter().filter_map(|id| s.effect(*id)) {
        let outside: Vec<String> = colors(s, entry).into_iter().filter(|c| taste::parse_hex(c).is_some_and(|rgb| palette.iter().all(|p| taste::distance(*p, rgb) > limit))).collect();
        if outside.is_empty() {
            continue;
        }
        count += 1;
        if listed.len() < LISTED {
            let name = entry.descriptor.map_or("Effect", |d| d.name);
            listed.push(json!({"effectId":entry.id.0,"name":name,"startMs":entry.node.start.total_ms(),"colors":outside}));
        }
    }
    (listed, count)
}

/// Every distinct `#rrggbb` the effect's shown parameters hold (colours, colour lists, gradients, nested records): a
/// parameter its editor hides, such as the gradient of a Chase set to a static colour, doesn't colour anything.
pub fn colors(s: &SequenceSession, entry: &EffectEntry) -> Vec<String> {
    fn walk(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::String(t) if t.len() == 7 && t.starts_with('#') && t[1..].chars().all(|c| c.is_ascii_hexdigit()) => {
                let t = t.to_ascii_lowercase();
                if !out.contains(&t) {
                    out.push(t);
                }
            }
            Value::Array(items) => items.iter().for_each(|i| walk(i, out)),
            Value::Object(fields) => fields.values().for_each(|i| walk(i, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    if let Some(desc) = entry.descriptor {
        let mut shown = vixen_effects::framework::Visibility::default();
        if let (Some(f), Some(info)) = (desc.visibility, s.target_info(entry.id)) {
            f(&entry.node.params, &info, &mut shown);
        }
        for p in desc.params.iter().filter(|p| shown.is_visible(p)) {
            walk(&crate::studio_params::encode(entry.node.params.get(p.name)), &mut out);
        }
    }
    out
}
