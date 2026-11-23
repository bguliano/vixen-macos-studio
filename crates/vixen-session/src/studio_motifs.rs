//! Motifs (B75): a template of 1..10 effects placed on every mark of a collection, varied per event (target set, colour
//! from a palette, alternating parameters, a stepped band or offset, an intensity ramp), in one undo step. Built-in motifs
//! match the show-building agent's library; agents save their own recipes by name in the profile.
use super::{
    apply_add, mark_effects, parameter_values, plan_add, plan_band, record, string, target_ids_schema,
    target_slice_schema, NewEffect, BATCH_LIMIT,
};
use crate::{
    app::AppCore,
    sequence::SequenceSession,
    undo::{AddRemoveKind, UndoAction},
};
use serde_json::{json, Map, Value};
use uuid::Uuid;
use vixen_effects::framework::{EffectDescriptor, ParamKind};

/// Saved recipes, at the profile's top level so they travel with the show.
const STORE_FILE: &str = "Studio Motifs.json";
/// A step's parameter value replaced by the source mark collection's id (e.g. Bars MarkCollectionId).
const MARKS: &str = "$marks";
const STEP_LIMIT: usize = 10;
const SET_LIMIT: usize = 8;
const DRY_RUN_LISTED: usize = 200;

/// The built-in motifs: name, recipe.
fn builtins() -> Vec<(&'static str, Value)> {
    let count_wipe = |direction: &str, pulse: f64| json!({"Direction":direction,"WipeMovement":"Count","PassCount":1,"PulsePercent":pulse});
    vec![
        ("march", json!({
            "description": "Candy stripes stepping one notch per mark: one Bars effect across the run with Step on Marks on the motif's mark collection (4 marks per stripe cycle). Every mark in the run counts, including any minStrength or snap left out. Palette: the stripe colours.",
            "palette": ["#ff0000", "#ffffff"],
            "steps": [{"effect":"Bars","scope":"span","set":"all","params":{"BarType":"Flat","Direction":"Up","Repeat":3,"StepToMarks":true,"MarkCollectionId":MARKS,"StepsPerCycle":4}}]
        })),
        ("train", json!({
            "description": "A new diagonal wipe launched on every mark, each running for two holds so they follow one another like train cars. Alternates target sets per mark; colour cycles through the palette.",
            "palette": ["#00b4ff", "#ffffff"],
            "steps": [{"effect":"Wipe","length":2,"params":count_wipe("DiagonalUp", 35.0)}]
        })),
        ("swoosh_glow", json!({
            "description": "A fast horizontal swoosh (the first 35% of the hold), alternating direction per mark, then the same targets glow and fade for the rest of the hold.",
            "palette": ["#ff2000", "#ffb000"],
            "steps": [
                {"effect":"Wipe","length":0.35,"params":count_wipe("Horizontal", 50.0),"alternate":[{"ReverseDirection":false},{"ReverseDirection":true}]},
                {"effect":"Pulse","start":0.35,"length":0.65,"params":{"LevelCurve":[[0,100],[100,30]]}}
            ]
        })),
        ("slam", json!({
            "description": "Every target set slams to full and decays, with a ring bursting outward in the next palette colour over the first 60% of the hold.",
            "palette": ["#ffffff", "#ff0000"],
            "steps": [
                {"effect":"Pulse","set":"all","params":{"LevelCurve":[[0,100],[15,100],[100,0]]}},
                {"effect":"Wipe","set":"all","length":0.6,"shift":1,"params":count_wipe("Circle", 20.0)}
            ]
        })),
        ("hop", json!({
            "description": "A band that hops low, middle, high on successive marks (a still band Wipe, as effect_band), alternating target sets. Targets need Vixen's Location property.",
            "palette": ["#00ff40", "#ffff00"],
            "steps": [{"band":{"fromPct":0,"toPct":30},"stepPct":35,"stepWrap":3,"length":0.7}]
        })),
        ("pingpong", json!({
            "description": "A horizontal wipe bouncing between target sets: the first set left to right, the next right to left, and so on.",
            "palette": ["#ff00ff", "#00ffff"],
            "steps": [{"effect":"Wipe","params":count_wipe("Horizontal", 40.0),"alternate":[{"ReverseDirection":false},{"ReverseDirection":true}]}]
        })),
        ("stab_run", json!({
            "description": "Staccato stabs walking through the target sets: a full stab for 45% of the hold, then a softer echo on the next set.",
            "palette": ["#ffffff"],
            "steps": [
                {"effect":"Set Level","length":0.45,"params":{"IntensityLevel":1.0}},
                {"effect":"Set Level","start":0.5,"length":0.3,"shift":1,"level":60,"params":{"IntensityLevel":1.0}}
            ]
        })),
        ("climb", json!({
            "description": "A rung climbing the targets' height one fifth per mark, with the rung below it lit dimly behind, then starting again at the bottom. Targets need Vixen's Location property.",
            "palette": ["#ffd000", "#ff6000"],
            "steps": [
                {"band":{"fromPct":0,"toPct":20},"stepPct":20,"stepWrap":5},
                {"band":{"fromPct":0,"toPct":20},"stepPct":20,"stepWrap":5,"shift":-1,"level":35}
            ]
        })),
    ]
}

pub(super) const PLACE_DESCRIPTION: &str = "Place a motif, a small template of 1..10 effects, on every mark of a mark collection, varied per mark, in one undo step with one render pass. Marks: markCollection (id or exact name), range, minStrength, snap and gridCollection as effects_on_marks. motif names a built-in (march, train, swoosh_glow, slam, hop, pingpong, stab_run, climb; motif_list describes them) or one saved with motif_save; or pass recipe inline (the motif_save format). targetSets lists 1..8 target groups, each {targetId|targetIds|targetSlice} as effect_add: a step on set \"event\" (the default) uses set i % n on the i-th mark, so two sets alternate left/right, more sets walk. palette [#RRGGBB...] overrides the motif's own colours, cycled per mark. ramp [fromPct,toPct] raises (or lowers) every step's level linearly from the first mark to the last (default [100,100]); levelFromStrength:true also scales it by each mark's strength. Each mark's hold lasts until the next mark times gapFraction (default 1), capped at maxMs (default 2000); the last mark holds the median gap. Steps are timed as fractions of the hold, so a motif follows the tempo. layerId puts every effect on a layer. dryRun:true returns the effects it would place (step, mark, effect, target, times, parameters; the first 200) without adding anything. Returns effectIds, events used, skipped marks, and unscaledSteps (steps whose effect has no level parameter, so ramp did not apply).";

pub(super) const LIST_DESCRIPTION: &str = "List motifs for motif_place: the built-ins and those saved in this profile with motif_save, each with its description and full recipe.";

pub(super) const SAVE_DESCRIPTION: &str = "Save a motif recipe under a name in this profile (the file Studio Motifs.json at the profile's top level), for motif_place. Not part of sequence undo; motif_delete removes it. A name is 1..64 letters, digits, spaces, _ or -, and cannot be a built-in's. replace:true overwrites a saved motif of that name. recipe: {description?, palette?:[#RRGGBB...], steps:[1..10 steps]}. A step is {effect:<name or typeId>, params?} or {band:{fromPct,toPct}} (a still band Wipe, as effect_band), plus: set (\"event\": set i % n per mark, the default; \"all\"; or a set index); scope (\"event\", one effect per mark, the default; or \"span\", one effect from the first mark to the end of the last hold); start and length (fractions of the hold; length up to 4 overlaps later marks; default start 0, length the rest of the hold) or lengthMs; alternate [{params}...] merged over params on mark i % n; color (\"palette\", the default: the palette colour of this mark, or the whole palette on a span step; \"none\"; or #RRGGBB); shift (act as mark i+shift for set, palette, alternate and step; a step shifted before the first mark is skipped); level (percent, default 100) and ramp (false ignores the motif ramp); stepPct (bands: move the band this much per mark) or stepParams {param: increment} (numbers in params, added per mark); stepWrap (marks before the step starts over; bands default to as many as fit). A string parameter value \"$marks\" becomes the motif's mark collection id (e.g. Bars MarkCollectionId with StepToMarks). Example: {\"palette\":[\"#ff0000\",\"#00ff00\"],\"steps\":[{\"effect\":\"Wipe\",\"length\":0.4,\"params\":{\"Direction\":\"Horizontal\",\"WipeMovement\":\"Count\",\"PassCount\":1},\"alternate\":[{\"ReverseDirection\":false},{\"ReverseDirection\":true}]},{\"effect\":\"Pulse\",\"start\":0.4,\"length\":0.6,\"set\":\"all\",\"params\":{\"LevelCurve\":[[0,100],[100,0]]}}]}.";

pub(super) const DELETE_DESCRIPTION: &str = "Delete a motif saved with motif_save from this profile. Built-ins cannot be deleted. Requires confirm:true; not part of sequence undo.";

fn color_schema() -> Value {
    json!({"type":"string","minLength":7,"maxLength":7})
}
fn palette_schema() -> Value {
    json!({"type":"array","items":color_schema(),"minItems":1,"maxItems":64})
}
fn step_schema() -> Value {
    let percent = json!({"type":"number","minimum":0,"maximum":100});
    json!({"type":"object","properties":{
        "effect":{"type":"string","minLength":1,"maxLength":256},
        "band":{"type":"object","properties":{"fromPct":percent,"toPct":percent},"required":["fromPct","toPct"],"additionalProperties":false},
        "set":{"description":"\"event\", \"all\" or a set index"},
        "scope":{"type":"string","enum":["event","span"]},
        "start":{"type":"number","minimum":0,"maximum":1},
        "length":{"type":"number","minimum":0.01,"maximum":4},
        "lengthMs":{"type":"number","minimum":1,"maximum":60000},
        "params":{"type":"object"},
        "alternate":{"type":"array","items":{"type":"object"},"minItems":1,"maxItems":16},
        "color":{"type":"string","minLength":4,"maxLength":7},
        "shift":{"type":"integer","minimum":-64,"maximum":64},
        "level":percent,
        "ramp":{"type":"boolean"},
        "stepPct":{"type":"number","minimum":-100,"maximum":100},
        "stepParams":{"type":"object"},
        "stepWrap":{"type":"integer","minimum":1,"maximum":256}
    },"additionalProperties":false})
}
fn recipe_schema() -> Value {
    json!({"type":"object","properties":{
        "description":{"type":"string","maxLength":1024},
        "palette":palette_schema(),
        "steps":{"type":"array","items":step_schema(),"minItems":1,"maxItems":STEP_LIMIT}
    },"required":["steps"],"additionalProperties":false,
    "examples":[{"palette":["#ff0000","#00ff00"],"steps":[{"effect":"Wipe","length":0.4,"params":{"Direction":"Horizontal","WipeMovement":"Count","PassCount":1},"alternate":[{"ReverseDirection":false},{"ReverseDirection":true}]},{"effect":"Pulse","start":0.4,"length":0.6,"set":"all","params":{"LevelCurve":[[0,100],[100,0]]}}]}]})
}
fn target_set_schema() -> Value {
    json!({"type":"object","properties":{"targetId":{"type":"string","minLength":1,"maxLength":256},"targetIds":target_ids_schema(),"targetSlice":target_slice_schema()},"additionalProperties":false})
}

pub(super) fn place_schema() -> Value {
    let string = json!({"type":"string","minLength":1,"maxLength":256});
    let number = json!({"type":"number","minimum":0,"maximum":3600000});
    let percent = json!({"type":"number","minimum":0,"maximum":100});
    json!({
        "sequenceId": string,
        "markCollection": string,
        "range": {"type":"array","items":number,"minItems":2,"maxItems":2},
        "minStrength": {"type":"number","minimum":0,"maximum":1},
        "snap": {"type":"string","enum":["beat","8th","16th"]},
        "gridCollection": string,
        "motif": {"type":"string","minLength":1,"maxLength":64,"examples":["march","train","swoosh_glow","slam","hop","pingpong","stab_run","climb"]},
        "recipe": recipe_schema(),
        "targetSets": {"type":"array","items":target_set_schema(),"minItems":1,"maxItems":SET_LIMIT,
            "examples":[[{"targetId":"<left tree UUID>"},{"targetId":"<right tree UUID>"}],[{"targetSlice":{"groupId":"<megatree UUID>","every":2}},{"targetSlice":{"groupId":"<megatree UUID>","every":2,"offset":1}}]]},
        "palette": palette_schema(),
        "ramp": {"type":"array","items":percent,"minItems":2,"maxItems":2,"examples":[[40,100]]},
        "levelFromStrength": {"type":"boolean"},
        "gapFraction": {"type":"number","minimum":0.05,"maximum":1},
        "maxMs": {"type":"number","minimum":1,"maximum":60000},
        "layerId": string,
        "dryRun": {"type":"boolean"},
    })
}
pub(super) fn save_schema() -> Value {
    json!({"name":{"type":"string","minLength":1,"maxLength":64,"examples":["big_hit"]},"recipe":recipe_schema(),"replace":{"type":"boolean"}})
}
pub(super) fn delete_schema() -> Value {
    json!({"name":{"type":"string","minLength":1,"maxLength":64},"confirm":{"type":"boolean"}})
}

enum SetRef {
    Event,
    All,
    Index(usize),
}
enum Look {
    Effect(&'static EffectDescriptor),
    Band(f64, f64),
}
enum Paint {
    Palette,
    None,
    Fixed(String),
}
/// One validated step of a recipe.
struct Step {
    look: Look,
    set: SetRef,
    span: bool,
    start: f64,
    length: Option<f64>,
    length_ms: Option<f64>,
    params: Map<String, Value>,
    alternate: Vec<Map<String, Value>>,
    paint: Paint,
    shift: i64,
    level: f64,
    ramp: bool,
    step_pct: f64,
    step_params: Vec<(String, f64)>,
    wrap: Option<u64>,
}
struct Recipe {
    palette: Vec<String>,
    steps: Vec<Step>,
}

fn hex(c: &str) -> bool {
    c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|c| c.is_ascii_hexdigit())
}

/// An effect by typeId or by name, ignoring case ("set level", "Wipe").
fn effect_by_name(name: &str) -> Result<&'static EffectDescriptor, String> {
    if let Ok(id) = Uuid::parse_str(name) {
        return vixen_effect_registry::by_type_id(&id).ok_or(format!("Effect type {name} not found"));
    }
    vixen_effect_registry::all()
        .iter()
        .copied()
        .find(|d| d.name.eq_ignore_ascii_case(name))
        .ok_or(format!("Unknown effect \"{name}\"; effect_catalog lists the effect names"))
}

/// `params` with every "$marks" string replaced by `marks`.
fn with_marks(params: &Map<String, Value>, marks: &str) -> Map<String, Value> {
    params.iter().map(|(k, v)| (k.clone(), if v == MARKS { json!(marks) } else { v.clone() })).collect()
}

/// Checks a recipe's shape, effects and parameters; nothing needs a sequence.
fn parse_recipe(recipe: &Value) -> Result<Recipe, String> {
    crate::studio_schema::arguments(&recipe_schema(), recipe).map_err(|e| e.replacen("arguments", "recipe", 1))?;
    let palette: Vec<String> = match recipe.get("palette") {
        Some(list) => list.as_array().unwrap().iter().map(|c| c.as_str().unwrap().to_ascii_lowercase()).collect(),
        None => vec!["#ffffff".into()],
    };
    if let Some(bad) = palette.iter().position(|c| !hex(c)) {
        return Err(format!("recipe.palette[{bad}] must be #RRGGBB"));
    }
    let steps = recipe["steps"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(k, step)| parse_step(step).map_err(|e| format!("recipe.steps[{k}]: {e}")))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Recipe { palette, steps })
}

fn parse_step(step: &Value) -> Result<Step, String> {
    let object = |k: &str| step.get(k).and_then(Value::as_object).cloned().unwrap_or_default();
    let look = match (step.get("effect"), step.get("band")) {
        (Some(name), None) => Look::Effect(effect_by_name(name.as_str().unwrap())?),
        (None, Some(band)) => {
            let (from, to) = (band["fromPct"].as_f64().unwrap(), band["toPct"].as_f64().unwrap());
            if to - from < 1.0 {
                return Err("band toPct must be at least 1 above fromPct".into());
            }
            Look::Band(from, to)
        }
        _ => return Err("give exactly one of effect or band".into()),
    };
    let set = match step.get("set") {
        None => SetRef::Event,
        Some(Value::String(s)) if s == "event" => SetRef::Event,
        Some(Value::String(s)) if s == "all" => SetRef::All,
        Some(v) => match v.as_u64() {
            Some(n) if (n as usize) < SET_LIMIT => SetRef::Index(n as usize),
            _ => return Err(format!("set must be \"event\", \"all\" or a set index below {SET_LIMIT}")),
        },
    };
    if step.get("length").is_some() && step.get("lengthMs").is_some() {
        return Err("give length or lengthMs, not both".into());
    }
    let paint = match step.get("color").and_then(Value::as_str) {
        None | Some("palette") => Paint::Palette,
        Some("none") => Paint::None,
        Some(c) if hex(c) => Paint::Fixed(c.to_ascii_lowercase()),
        Some(_) => return Err("color must be \"palette\", \"none\" or #RRGGBB".into()),
    };
    let params = object("params");
    let alternate: Vec<Map<String, Value>> =
        step.get("alternate").and_then(Value::as_array).map(|l| l.iter().map(|v| v.as_object().cloned().unwrap()).collect()).unwrap_or_default();
    let step_params: Vec<(String, f64)> = object("stepParams")
        .into_iter()
        .map(|(name, inc)| {
            let inc = inc.as_f64().ok_or(format!("stepParams.{name} must be a number"))?;
            if !params.get(&name).is_some_and(Value::is_number) {
                return Err(format!("stepParams.{name} needs a number for {name} in params to step from"));
            }
            Ok((name, inc))
        })
        .collect::<Result<_, String>>()?;
    let step_pct = step.get("stepPct").and_then(Value::as_f64).unwrap_or(0.0);
    match look {
        Look::Effect(descriptor) => {
            if step.get("stepPct").is_some() {
                return Err("stepPct moves bands; use stepParams for an effect".into());
            }
            // Decode everything once now, so a bad parameter fails before any mark is planned.
            let nil = Uuid::nil().to_string();
            for (i, alt) in std::iter::once(&Map::new()).chain(alternate.iter()).enumerate() {
                let mut merged = with_marks(&params, &nil);
                merged.extend(with_marks(alt, &nil));
                parameter_values(descriptor, &Value::Object(merged))
                    .map_err(|e| if i == 0 { e } else { format!("alternate[{}]: {e}", i - 1) })?;
            }
        }
        Look::Band(..) => {
            if !params.is_empty() || !alternate.is_empty() || !step_params.is_empty() {
                return Err("a band step takes no params, alternate or stepParams".into());
            }
        }
    }
    Ok(Step {
        look,
        set,
        span: step.get("scope").and_then(Value::as_str) == Some("span"),
        start: step.get("start").and_then(Value::as_f64).unwrap_or(0.0),
        length: step.get("length").and_then(Value::as_f64),
        length_ms: step.get("lengthMs").and_then(Value::as_f64),
        params,
        alternate,
        paint,
        shift: step.get("shift").and_then(Value::as_i64).unwrap_or(0),
        level: step.get("level").and_then(Value::as_f64).unwrap_or(100.0),
        ramp: step.get("ramp") != Some(&Value::Bool(false)),
        step_pct,
        step_params,
        wrap: step.get("stepWrap").and_then(Value::as_u64),
    })
}

/// A band step's position on the j-th mark: the band moved stepPct per mark, starting over after stepWrap marks (by
/// default, as many as fit inside 0..100).
fn band_at(from: f64, to: f64, step: f64, wrap: Option<u64>, j: u64) -> (f64, f64) {
    if step == 0.0 {
        return (from, to);
    }
    let room = if step > 0.0 { 100.0 - to } else { from };
    let fits = (room / step.abs() + 1e-9).floor() as u64 + 1;
    let k = (j % wrap.unwrap_or(fits).max(1)) as f64;
    let shifted = (from + step * k, to + step * k);
    (shifted.0.clamp(0.0, 99.0), shifted.1.clamp(1.0, 100.0))
}

// ---- Saved recipes ----

/// The store file. Only the file itself is checked: a profile's Media or other folders may well be symlinks.
fn store_path(core: &AppCore) -> Result<std::path::PathBuf, String> {
    let path = core.paths.root().join(STORE_FILE);
    match std::fs::symlink_metadata(&path) {
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_file() => Err(format!("{STORE_FILE} must be an ordinary file, not a symlink")),
        _ => Ok(path),
    }
}
fn load(core: &AppCore) -> Result<Map<String, Value>, String> {
    let path = store_path(core)?;
    match std::fs::read(&path) {
        Ok(bytes) => {
            let value: Value = serde_json::from_slice(&bytes).map_err(|e| format!("{STORE_FILE} is not valid JSON: {e}"))?;
            Ok(value.get("motifs").and_then(Value::as_object).cloned().unwrap_or_default())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
        Err(e) => Err(e.to_string()),
    }
}
fn store(core: &AppCore, motifs: Map<String, Value>) -> Result<(), String> {
    let path = store_path(core)?;
    let text = serde_json::to_string_pretty(&json!({"motifs":motifs})).unwrap();
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&temp, &path).map_err(|e| e.to_string())
}
fn builtin(name: &str) -> Option<(&'static str, Value)> {
    builtins().into_iter().find(|(n, _)| n.eq_ignore_ascii_case(name))
}
fn valid_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty() || name.len() > 64 || !name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '-')) {
        return Err("name must be 1..64 letters, digits, spaces, _ or -".into());
    }
    Ok(name)
}

pub(super) fn list(core: &AppCore) -> Result<Value, String> {
    let mut motifs: Vec<Value> = builtins()
        .into_iter()
        .map(|(name, recipe)| json!({"name":name,"builtIn":true,"description":recipe["description"],"recipe":recipe}))
        .collect();
    motifs.extend(load(core)?.into_iter().map(|(name, recipe)| json!({"name":name,"builtIn":false,"description":recipe.get("description").cloned().unwrap_or(json!("")),"recipe":recipe})));
    Ok(json!({"motifs":motifs}))
}

pub(super) fn save(core: &AppCore, a: &Value) -> Result<Value, String> {
    let name = valid_name(string(a, "name")?)?;
    if builtin(name).is_some() {
        return Err(format!("\"{name}\" is a built-in motif; pick another name"));
    }
    parse_recipe(&a["recipe"])?;
    let mut motifs = load(core)?;
    let existing = motifs.keys().find(|k| k.eq_ignore_ascii_case(name)).cloned();
    if existing.is_some() && a["replace"] != true {
        return Err(format!("A motif named \"{name}\" is already saved; pass replace:true to overwrite it"));
    }
    if let Some(old) = &existing {
        motifs.remove(old);
    }
    motifs.insert(name.to_string(), a["recipe"].clone());
    store(core, motifs)?;
    Ok(json!({"name":name,"replaced":existing.is_some()}))
}

pub(super) fn delete(core: &AppCore, a: &Value) -> Result<Value, String> {
    super::confirm(a)?;
    let name = string(a, "name")?;
    if builtin(name).is_some() {
        return Err(format!("\"{name}\" is a built-in motif and cannot be deleted"));
    }
    let mut motifs = load(core)?;
    let key = motifs.keys().find(|k| k.eq_ignore_ascii_case(name.trim())).cloned().ok_or(format!("No saved motif named \"{name}\""))?;
    motifs.remove(&key);
    store(core, motifs)?;
    Ok(json!({"deleted":key}))
}

// ---- Placement ----

/// One planned effect, with the step and mark it came from and the parameters it was given (for dry runs).
struct Planned {
    new: NewEffect,
    step: usize,
    event: Option<usize>,
    params: Value,
}

fn median_gap(times: &[f64]) -> Option<f64> {
    let mut gaps: Vec<f64> = times.windows(2).map(|w| w[1] - w[0]).collect();
    gaps.sort_by(f64::total_cmp);
    gaps.get(gaps.len() / 2).copied()
}

pub(super) fn place(core: &AppCore, s: &mut SequenceSession, a: &Value, agent: bool) -> Result<Value, String> {
    let (name, recipe_value) = match (a.get("motif"), a.get("recipe")) {
        (Some(_), None) => {
            let wanted = string(a, "motif")?;
            match builtin(wanted) {
                Some((name, recipe)) => (name.to_string(), recipe),
                None => {
                    let saved = load(core)?;
                    let (name, recipe) = saved.into_iter().find(|(k, _)| k.eq_ignore_ascii_case(wanted.trim())).ok_or_else(|| {
                        format!("Unknown motif \"{wanted}\"; built-ins are {}, and motif_list shows saved ones", builtins().iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", "))
                    })?;
                    (name, recipe)
                }
            }
        }
        (None, Some(recipe)) => ("recipe".to_string(), recipe.clone()),
        _ => return Err("Give exactly one of motif or recipe".into()),
    };
    let recipe = parse_recipe(&recipe_value)?;
    let palette: Vec<String> = match a.get("palette") {
        Some(list) => {
            let list: Vec<String> = list.as_array().unwrap().iter().map(|c| c.as_str().unwrap().to_ascii_lowercase()).collect();
            if let Some(bad) = list.iter().position(|c| !hex(c)) {
                return Err(format!("palette[{bad}] must be #RRGGBB"));
            }
            list
        }
        None => recipe.palette.clone(),
    };
    let sets: Vec<Map<String, Value>> = a["targetSets"].as_array().unwrap().iter().map(|v| v.as_object().cloned().unwrap()).collect();
    for (k, set) in sets.iter().enumerate() {
        super::targets(core, &Value::Object(set.clone())).map_err(|e| format!("targetSets[{k}]: {e}"))?;
    }
    for (k, step) in recipe.steps.iter().enumerate() {
        if let SetRef::Index(i) = step.set {
            if i >= sets.len() {
                return Err(format!("recipe.steps[{k}] uses set {i}, but targetSets has {}", sets.len()));
            }
        }
    }
    let marks_id = mark_effects::collection(s, string(a, "markCollection")?, "markCollection")?.id.to_string();
    let selection = mark_effects::select_hits(s, a)?;
    let length = s.length().total_ms();
    let times: Vec<f64> = selection.hits.iter().map(|h| h.time).collect();
    let n = times.len();
    let gap = a.get("gapFraction").and_then(Value::as_f64).unwrap_or(1.0);
    let max = a.get("maxMs").and_then(Value::as_f64).unwrap_or(2000.0);
    let median = median_gap(&times);
    let holds: Vec<f64> = (0..n)
        .map(|i| {
            let room = match times.get(i + 1) {
                Some(next) => next - times[i],
                None => median.map_or(selection.end - times[i], |m| m.min(selection.end - times[i])),
            };
            (room * gap).min(max)
        })
        .collect();
    let (ramp_from, ramp_to) = match a.get("ramp") {
        Some(r) => (r[0].as_f64().unwrap(), r[1].as_f64().unwrap()),
        None => (100.0, 100.0),
    };
    let strength = a["levelFromStrength"] == true;
    let layer = a.get("layerId").cloned();

    let mut planned: Vec<Planned> = Vec::new();
    let mut unscaled = std::collections::BTreeSet::new();
    let mut short = 0usize;
    for (k, step) in recipe.steps.iter().enumerate() {
        let level_param = match step.look {
            Look::Effect(d) => mark_effects::LEVEL_PARAMS.iter().find_map(|n| d.params.iter().find(|p| p.name == *n)).map(|p| p.name),
            Look::Band(..) => None,
        };
        let color_param = match step.look {
            Look::Effect(d) => mark_effects::color_param(d),
            Look::Band(..) => None,
        };
        // A span step is placed once, as if on the first mark, across the whole run.
        let events: Vec<usize> = if step.span { vec![0] } else { (0..n).collect() };
        for i in events {
            let Some(j) = u64::try_from(i as i64 + step.shift).ok() else { continue };
            let (start, duration) = if step.span {
                let start = times[0] + holds[0] * step.start;
                (start, times[n - 1] + holds[n - 1] - start)
            } else {
                let start = times[i] + holds[i] * step.start;
                (start, step.length_ms.unwrap_or(holds[i] * step.length.unwrap_or(1.0 - step.start)))
            };
            let duration = duration.min(length - start);
            if start >= length || duration < 1.0 {
                short += 1;
                continue;
            }
            let ramp = if step.ramp && !step.span {
                let t = if n > 1 { i as f64 / (n - 1) as f64 } else { 0.0 };
                (ramp_from + (ramp_to - ramp_from) * t) / 100.0
            } else {
                1.0
            };
            let factor = step.level / 100.0 * ramp * if strength && !step.span { selection.hits[i].strength } else { 1.0 };
            let color = |kind: Option<ParamKind>| -> Option<Value> {
                let pick = match &step.paint {
                    Paint::None => return None,
                    Paint::Fixed(c) => c.clone(),
                    Paint::Palette if step.span && kind.is_some_and(|k| !matches!(k, ParamKind::Color | ParamKind::Rgb)) => {
                        return Some(match kind.unwrap() {
                            ParamKind::Gradient | ParamKind::ColorList => json!(palette),
                            ParamKind::GradientList => json!(palette.iter().map(|c| [c]).collect::<Vec<_>>()),
                            _ => json!(palette.iter().map(|c| json!({"gradient":[c]})).collect::<Vec<_>>()),
                        });
                    }
                    Paint::Palette => palette[j as usize % palette.len()].clone(),
                };
                Some(match kind {
                    Some(kind) => mark_effects::solid_color(kind, &json!(pick)),
                    None => json!(pick),
                })
            };
            let chosen: Vec<&Map<String, Value>> = match step.set {
                SetRef::Event => vec![&sets[j as usize % sets.len()]],
                SetRef::All => sets.iter().collect(),
                SetRef::Index(x) => vec![&sets[x]],
            };
            let mut add = Map::new();
            add.insert("startMs".into(), json!(start));
            add.insert("durationMs".into(), json!(duration));
            if let Some(layer) = &layer {
                add.insert("layerId".into(), layer.clone());
            }
            let params: Value = match step.look {
                Look::Band(from, to) => {
                    let (from, to) = band_at(from, to, step.step_pct, step.wrap, j);
                    add.insert("fromPct".into(), json!(from));
                    add.insert("toPct".into(), json!(to));
                    add.insert("color".into(), color(None).unwrap_or(json!("#ffffff")));
                    add.insert("level".into(), json!((factor * 100.0).clamp(0.0, 100.0)));
                    json!({"fromPct":from,"toPct":to,"color":add["color"],"level":add["level"]})
                }
                Look::Effect(d) => {
                    let mut params = with_marks(&step.params, &marks_id);
                    if !step.alternate.is_empty() {
                        params.extend(with_marks(&step.alternate[j as usize % step.alternate.len()], &marks_id));
                    }
                    let k_wrapped = step.wrap.map_or(j, |w| j % w) as f64;
                    for (param, inc) in &step.step_params {
                        let base = params[param].as_f64().unwrap();
                        params.insert(param.clone(), json!(base + inc * k_wrapped));
                    }
                    if let Some(p) = color_param {
                        if !params.contains_key(p.name) {
                            if let Some(value) = color(Some(p.kind)) {
                                params.insert(p.name.into(), value);
                            }
                        }
                    }
                    add.insert("typeId".into(), json!(d.type_id.to_string()));
                    add.insert("params".into(), Value::Object(params.clone()));
                    Value::Object(params)
                }
            };
            for set in &chosen {
                let mut one = add.clone();
                one.extend(set.iter().map(|(k, v)| (k.clone(), v.clone())));
                let one = Value::Object(one);
                let news = match step.look {
                    Look::Band(..) => plan_band(core, s, &one),
                    Look::Effect(_) => plan_add(core, s, &one),
                }
                .map_err(|e| format!("recipe.steps[{k}]: {e}"))?;
                for mut new in news {
                    if (factor - 1.0).abs() > 1e-9 {
                        match (&step.look, level_param) {
                            (Look::Effect(_), Some(param)) => new.level = Some((param, factor)),
                            (Look::Effect(_), None) => {
                                unscaled.insert(k);
                            }
                            _ => {}
                        }
                    }
                    planned.push(Planned { new, step: k, event: (!step.span).then_some(i), params: params.clone() });
                }
            }
            if planned.len() > BATCH_LIMIT {
                return Err(format!("motif_place adds at most {BATCH_LIMIT} effects"));
            }
        }
    }
    if planned.is_empty() {
        return Err("Every effect would be shorter than 1 ms; raise gapFraction or maxMs".into());
    }
    let skipped = json!({"belowMinStrength":selection.weak,"merged":selection.merged,"tooShort":short});
    let unscaled: Vec<usize> = unscaled.into_iter().collect();
    if a["dryRun"] == true {
        let listed: Vec<Value> = planned
            .iter()
            .take(DRY_RUN_LISTED)
            .map(|p| {
                let name = vixen_effect_registry::by_type_id(&p.new.type_id).map_or("Effect", |d| d.name);
                json!({"step":p.step,"mark":p.event,"effect":name,"target":p.new.target_name,"targetId":p.new.target.to_string(),"startMs":p.new.start.total_ms(),"durationMs":p.new.duration.total_ms(),"params":p.params,"levelFactor":p.new.level.map(|(_, f)| f)})
            })
            .collect();
        return Ok(json!({"dryRun":true,"motif":name,"events":n,"total":planned.len(),"effects":listed,"truncated":planned.len() > DRY_RUN_LISTED,"skipped":skipped,"unscaledSteps":unscaled}));
    }
    let ids: Vec<_> = planned.into_iter().map(|p| apply_add(s, p.new)).collect();
    let all: Vec<u64> = ids.iter().map(|id| id.0).collect();
    record(s, &format!("Place motif {name}"), UndoAction::EffectsAdded { kind: AddRemoveKind::AddedOrRemoved, ids }, agent);
    s.render_all_dirty();
    Ok(json!({"motif":name,"effectIds":all,"events":n,"skipped":skipped,"unscaledSteps":unscaled,"effects":s.effect_count()}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_recipe_parses() {
        for (name, recipe) in builtins() {
            let parsed = parse_recipe(&recipe).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!((1..=STEP_LIMIT).contains(&parsed.steps.len()), "{name}");
            assert!(recipe["description"].as_str().is_some_and(|d| !d.is_empty()), "{name}");
        }
        let names: Vec<_> = builtins().into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["march", "train", "swoosh_glow", "slam", "hop", "pingpong", "stab_run", "climb"]);
    }

    #[test]
    fn bands_step_and_wrap_inside_the_height() {
        assert_eq!(band_at(0.0, 30.0, 35.0, Some(3), 0), (0.0, 30.0));
        assert_eq!(band_at(0.0, 30.0, 35.0, Some(3), 2), (70.0, 100.0));
        assert_eq!(band_at(0.0, 30.0, 35.0, Some(3), 3), (0.0, 30.0));
        // Without stepWrap, as many as fit: 0-20, 20-40 ... 80-100, then over.
        assert_eq!(band_at(0.0, 20.0, 20.0, None, 4), (80.0, 100.0));
        assert_eq!(band_at(0.0, 20.0, 20.0, None, 5), (0.0, 20.0));
        assert_eq!(band_at(80.0, 100.0, -20.0, None, 1), (60.0, 80.0));
    }

    #[test]
    fn recipes_are_checked_before_placing() {
        let bad = |recipe: Value| parse_recipe(&recipe).err().unwrap();
        assert!(bad(json!({"steps":[]})).contains("recipe.steps"));
        assert!(bad(json!({"steps":[{"effect":"Nope"}]})).contains("Unknown effect"));
        assert!(bad(json!({"steps":[{"effect":"Wipe","band":{"fromPct":0,"toPct":10}}]})).contains("exactly one"));
        assert!(bad(json!({"steps":[{"effect":"Wipe","params":{"Nope":1}}]})).contains("Unknown parameter Nope"));
        assert!(bad(json!({"steps":[{"effect":"Wipe","alternate":[{"Direction":"Sideways"}]}]})).starts_with("recipe.steps[0]: alternate[0]"));
        assert!(bad(json!({"steps":[{"effect":"Wipe","stepParams":{"XOffset":5}}]})).contains("needs a number"));
        assert!(bad(json!({"steps":[{"band":{"fromPct":0,"toPct":10},"params":{"x":1}}]})).contains("band step"));
        assert!(bad(json!({"steps":[{"effect":"Pulse","color":"red"}]})).contains("color"));
        assert!(bad(json!({"palette":["#12345g"],"steps":[{"effect":"Pulse"}]})).contains("palette[0]"));
        assert!(parse_recipe(&json!({"steps":[{"effect":"set level","stepParams":{"IntensityLevel":0.1},"params":{"IntensityLevel":0.5},"stepWrap":4}]})).is_ok());
    }
}
