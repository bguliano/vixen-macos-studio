//! effect_step: discrete, segmented on/off figures (walk, alternate, stack, jump) stepped on marks or listed times, for
//! staccato passages. Each lit segment is a native Set Level (around a group) or a band Wipe (vertical), so the result
//! opens in the effect editor and in Windows Vixen like hand-placed effects, and the whole figure is one undo step.
use super::{
    band_params, mark_effects, parameter_value, parameter_values, record, string, targets, uuid, NewEffect, BATCH_LIMIT,
};
use crate::{
    app::AppCore,
    sequence::SequenceSession,
    undo::{AddRemoveKind, UndoAction},
};
use serde_json::{json, Value};
use uuid::Uuid;
use vixen_core::TimeSpan;

const MODES: &[&str] = &["walk", "alternate", "stack", "jump"];
/// At most this many segments; a vertical band must stay at least 1% tall.
const SEGMENT_LIMIT: usize = 100;
const STEP_LIMIT: usize = 10_000;

pub(super) const DESCRIPTION: &str = "Add a discrete, segmented on/off figure stepped in time with the music, for staccato hits (\"do do do do\" stabs) where smooth chases, spins and sweeps are wrong. One call replaces one effect per segment per hit, in one undo step with one render pass. Step times: markCollection (id or exact name; range [startMs,endMs] and minStrength as effects_on_marks, each mark's text its strength) or stepsMs, an explicit list of times. segments: {axis:\"around\", groupId, count?} splits the group's children (e.g. a megatree's strings) into count contiguous segments (default one per child), each lit as native Set Level effects on its children; or {axis:\"vertical\", targetId|targetIds|targetSlice, count} splits the targets' height into count horizontal bands (2..100), each lit as a still band Wipe as effect_band (elements need Vixen's Location property; element_location sets it). mode: \"walk\" lights width segments (default 1) and steps them one segment per hit, wrapping; \"alternate\" lights the even segments, then the odd ones, per hit (count 2 gives front/back halves); \"stack\" adds width segments (default 1) per hit from the bottom (or first segment), keeping earlier ones lit, and starts again after every segment is lit; \"jump\" lights width segments at seeded random positions (seed, default 1), never reusing a segment lit on the previous hit (needs width at most half of count). reverse:true walks the other way, stacks from the top (or last segment), or starts alternate on the odd segments. colors: #RRGGBB list cycled per hit (default white); a stacked segment keeps its hit's colour. level: brightness percent (default 100). Each hit lasts holdFraction (default 0.8) of the time to the next hit, so staccato gaps stay dark; stacked segments all go dark together, holdFraction of the way from the stack's last hit to the next reset. cutAtMs ends the run: later steps are dropped and the last hit ends there (times holdFraction); without it the last hit lasts as long as the median gap between hits. layerId puts every effect on a layer. Returns effectIds, steps used, segments and skipped steps.";

pub(super) fn schema() -> Value {
    let string = json!({"type":"string","minLength":1,"maxLength":256});
    let number = json!({"type":"number","minimum":0,"maximum":3600000});
    let whole = json!({"type":"integer","minimum":1});
    json!({
        "sequenceId": string,
        "mode": {"type":"string","enum":MODES},
        "markCollection": string,
        "range": {"type":"array","items":number,"minItems":2,"maxItems":2},
        "minStrength": {"type":"number","minimum":0,"maximum":1},
        "stepsMs": {"type":"array","items":number,"minItems":1,"maxItems":STEP_LIMIT},
        "segments": {"type":"object","properties":{"axis":{"type":"string","enum":["around","vertical"]},"groupId":string,"targetId":string,"targetIds":super::target_ids_schema(),"targetSlice":super::target_slice_schema(),"count":{"type":"integer","minimum":2,"maximum":SEGMENT_LIMIT},"width":whole},"required":["axis"],"additionalProperties":false},
        "reverse": {"type":"boolean"},
        "colors": {"type":"array","items":string,"minItems":1,"maxItems":64},
        "level": {"type":"number","minimum":0,"maximum":100},
        "holdFraction": {"type":"number","minimum":0.05,"maximum":1},
        "cutAtMs": number,
        "seed": {"type":"integer","minimum":0},
        "layerId": string,
    })
}

/// splitmix64: a small seeded generator, so a jump figure is the same every time it is rebuilt.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// The segments each step lights, in step order, for every mode but stack.
fn pattern(mode: &str, count: usize, width: usize, reverse: bool, steps: usize, seed: u64) -> Vec<Vec<usize>> {
    let mut rng = Rng(seed);
    let mut previous: Vec<usize> = Vec::new();
    (0..steps)
        .map(|k| {
            let mut lit: Vec<usize> = match mode {
                "walk" => {
                    let head = if reverse { (count - k % count) % count } else { k % count };
                    (0..width).map(|w| (head + w) % count).collect()
                }
                "alternate" => (0..count).filter(|i| i % 2 == (k + usize::from(reverse)) % 2).collect(),
                _ => {
                    let mut free: Vec<usize> = (0..count).filter(|i| !previous.contains(i)).collect();
                    // A partial Fisher-Yates shuffle picks width distinct segments the previous hit left dark.
                    for i in 0..width {
                        let j = i + rng.below(free.len() - i);
                        free.swap(i, j);
                    }
                    free.truncate(width);
                    free
                }
            };
            lit.sort_unstable();
            previous = lit.clone();
            lit
        })
        .collect()
}

/// The median gap between steps, or None for a single step.
fn median_gap(times: &[f64]) -> Option<f64> {
    let mut gaps: Vec<f64> = times.windows(2).map(|w| w[1] - w[0]).collect();
    gaps.sort_by(f64::total_cmp);
    gaps.get(gaps.len() / 2).copied()
}

/// Contiguous runs of sorted segment indexes, as [first, last] pairs.
fn runs(lit: &[usize]) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    for &i in lit {
        match out.last_mut() {
            Some(run) if run.1 + 1 == i => run.1 = i,
            _ => out.push((i, i)),
        }
    }
    out
}

enum Segments {
    /// Each segment's children (elements), with their names.
    Around(Vec<Vec<(Uuid, String)>>),
    /// The targets, and the number of bands their height is split into.
    Vertical(Vec<(Uuid, String)>, usize),
}
impl Segments {
    fn count(&self) -> usize {
        match self {
            Segments::Around(s) => s.len(),
            Segments::Vertical(_, n) => *n,
        }
    }
}

fn segments(core: &AppCore, a: &Value) -> Result<Segments, String> {
    let fields = a.as_object().unwrap();
    let count = fields.get("count").and_then(Value::as_u64).map(|n| n as usize);
    if a["axis"] == "around" {
        if let Some(extra) = fields.keys().find(|k| !["axis", "groupId", "count", "width"].contains(&k.as_str())) {
            return Err(format!("segments.{extra} does not apply to axis \"around\"; give groupId"));
        }
        let system = core.system.read();
        let tree = &system.tree;
        let group = tree.find(&uuid(a, "groupId").map_err(|e| format!("segments.{e}"))?).ok_or("segments group not found")?;
        let children: Vec<(Uuid, String)> = tree.node(group).children.iter().map(|c| (tree.node(*c).id, tree.node(*c).name.clone())).collect();
        let count = count.unwrap_or(children.len());
        if children.len() < 2 || count > children.len() {
            return Err(format!("segments.count must be 2..{} for this group (one segment per child at most)", children.len()));
        }
        let n = children.len();
        Ok(Segments::Around((0..count).map(|i| children[i * n / count..(i + 1) * n / count].to_vec()).collect()))
    } else {
        let count = count.ok_or("segments.count is required for axis \"vertical\" (the number of bands)")?;
        let mut target = a.clone();
        let object = target.as_object_mut().unwrap();
        for k in ["axis", "count", "width"] {
            object.remove(k);
        }
        if object.contains_key("groupId") {
            return Err("segments.groupId does not apply to axis \"vertical\"; give targetId, targetIds or targetSlice".into());
        }
        Ok(Segments::Vertical(targets(core, &target).map_err(|e| format!("segments: {e}"))?, count))
    }
}

/// The step times, sorted and deduplicated, and how many were dropped (below minStrength, past the cut).
fn step_times(s: &SequenceSession, a: &Value, length: f64) -> Result<(Vec<f64>, f64, usize), String> {
    if a.get("markCollection").is_some() == a.get("stepsMs").is_some() {
        return Err("Give exactly one of markCollection or stepsMs".into());
    }
    let (start, end) = match a.get("range") {
        Some(range) => {
            let r: Vec<f64> = range.as_array().unwrap().iter().filter_map(Value::as_f64).collect();
            if r[0] >= r[1] || r[0] >= length {
                return Err(format!("range must be [startMs, endMs] with startMs before endMs, inside the sequence (0..{length})"));
            }
            (r[0], r[1].min(length))
        }
        None => (0.0, length),
    };
    let mut weak = 0usize;
    let mut times: Vec<f64> = match a.get("markCollection") {
        Some(_) => {
            let min = a.get("minStrength").and_then(Value::as_f64).unwrap_or(0.0);
            mark_effects::collection(s, string(a, "markCollection")?, "markCollection")?
                .marks
                .iter()
                .filter(|m| {
                    let keep = mark_effects::strength(m) >= min;
                    weak += usize::from(!keep);
                    keep
                })
                .map(|m| m.start_time.total_ms())
                .collect()
        }
        None => {
            if a.get("minStrength").is_some() {
                return Err("minStrength applies to markCollection, not stepsMs".into());
            }
            a["stepsMs"].as_array().unwrap().iter().filter_map(Value::as_f64).collect()
        }
    };
    let cut = a.get("cutAtMs").and_then(Value::as_f64).map(|c| c.min(end));
    let limit = cut.unwrap_or(end);
    times.retain(|t| (start..limit).contains(t));
    times.sort_by(f64::total_cmp);
    times.dedup_by(|later, kept| (*later - *kept).abs() < 0.5);
    if times.is_empty() {
        return Err(format!("No steps between {start} and {limit} ms{}", if weak > 0 { format!(" ({weak} marks below minStrength)") } else { String::new() }));
    }
    let run_end = match cut {
        Some(cut) => cut,
        None => match median_gap(&times) {
            Some(gap) => (times[times.len() - 1] + gap).min(length),
            None => return Err("A single step needs cutAtMs to say when it ends".into()),
        },
    };
    Ok((times, run_end, weak))
}

fn color(value: &str) -> bool {
    value.len() == 7 && value.starts_with('#') && value[1..].chars().all(|c| c.is_ascii_hexdigit())
}

/// effect_step: validates everything, then adds the whole figure as one undo step and renders once.
pub(super) fn effect_step(core: &AppCore, s: &mut SequenceSession, a: &Value, agent: bool) -> Result<Value, String> {
    let length = s.length().total_ms();
    let mode = string(a, "mode")?;
    let segments = segments(core, &a["segments"])?;
    let count = segments.count();
    let width = match a["segments"].get("width") {
        None => 1,
        Some(_) if mode == "alternate" => return Err("segments.width does not apply to alternate, which lights every other segment".into()),
        Some(w) => w.as_u64().unwrap() as usize,
    };
    let widest = match mode {
        "jump" => count / 2,
        "stack" => count,
        _ => count - 1,
    };
    if width > widest {
        return Err(format!("segments.width must be 1..{widest} for {mode} with {count} segments"));
    }
    let (times, run_end, weak) = step_times(s, a, length)?;
    let reverse = a["reverse"] == true;
    let hold = a.get("holdFraction").and_then(Value::as_f64).unwrap_or(0.8);
    let level = a.get("level").and_then(Value::as_f64).unwrap_or(100.0);
    let colors: Vec<&str> = match a.get("colors") {
        Some(list) => list.as_array().unwrap().iter().map(|c| c.as_str().unwrap()).collect(),
        None => vec!["#ffffff"],
    };
    if let Some(bad) = colors.iter().find(|c| !color(c)) {
        return Err(format!("colors must be #RRGGBB; got {bad}"));
    }
    let layer = match a.get("layerId") {
        Some(_) => {
            let layer = uuid(a, "layerId")?;
            if !s.document().layers.layers.iter().any(|l| l.id == layer) {
                return Err("Layer not found".into());
            }
            Some(layer)
        }
        None => None,
    };

    // Lit spans: (segment indexes, start, end, step), one per hit, or per stacked group of segments.
    let next = |k: usize| times.get(k + 1).copied().unwrap_or(run_end);
    let mut spans: Vec<(Vec<usize>, f64, f64, usize)> = Vec::new();
    if mode == "stack" {
        let per_cycle = count.div_ceil(width);
        for (k, &t) in times.iter().enumerate() {
            let j = k % per_cycle;
            let cycle_last = (k - j + per_cycle - 1).min(times.len() - 1);
            let off = times[cycle_last] + (next(cycle_last) - times[cycle_last]) * hold;
            let lit = (j * width..((j + 1) * width).min(count)).map(|i| if reverse { count - 1 - i } else { i }).collect();
            spans.push((lit, t, off, k));
        }
    } else {
        let lit = pattern(mode, count, width, reverse, times.len(), a.get("seed").and_then(Value::as_u64).unwrap_or(1));
        for (k, (lit, &t)) in lit.into_iter().zip(&times).enumerate() {
            spans.push((lit, t, t + (next(k) - t) * hold, k));
        }
    }

    let mut planned: Vec<NewEffect> = Vec::new();
    let mut short = 0usize;
    match &segments {
        Segments::Around(groups) => {
            let set_level = vixen_effect_registry::all().iter().find(|d| d.name == "Set Level").ok_or("Set Level effect not available")?;
            let intensity = parameter_value(set_level, "IntensityLevel", &json!(level / 100.0))?;
            for (lit, on, off, k) in &spans {
                if off - on < 1.0 {
                    short += 1;
                    continue;
                }
                let paint = parameter_value(set_level, "Color", &json!(colors[k % colors.len()]))?;
                for (target, target_name) in lit.iter().flat_map(|i| &groups[*i]) {
                    planned.push(NewEffect {
                        type_id: set_level.type_id,
                        target: *target,
                        target_name: target_name.clone(),
                        start: TimeSpan::from_ms(*on),
                        duration: TimeSpan::from_ms(off - on),
                        params: vec![paint.clone(), intensity.clone()],
                        layer,
                        level: None,
                    });
                }
            }
        }
        Segments::Vertical(targets, bands) => {
            let wipe = vixen_effect_registry::all().iter().find(|d| d.name == "Wipe").ok_or("Wipe effect not available")?;
            let pct = |i: usize| 100.0 * i as f64 / *bands as f64;
            for (lit, on, off, k) in &spans {
                if off - on < 1.0 {
                    short += 1;
                    continue;
                }
                for (first, last) in runs(lit) {
                    for band in band_params(pct(first), pct(last + 1), colors[k % colors.len()], level) {
                        let params = parameter_values(wipe, &band)?;
                        for (target, target_name) in targets {
                            planned.push(NewEffect {
                                type_id: wipe.type_id,
                                target: *target,
                                target_name: target_name.clone(),
                                start: TimeSpan::from_ms(*on),
                                duration: TimeSpan::from_ms(off - on),
                                params: params.clone(),
                                layer,
                                level: None,
                            });
                        }
                    }
                }
            }
        }
    }
    if planned.len() > BATCH_LIMIT {
        return Err(format!("effect_step adds at most {BATCH_LIMIT} effects, counting one per lit element or band per hit"));
    }
    if planned.is_empty() {
        return Err("Every hit would be shorter than 1 ms; raise holdFraction".into());
    }
    let ids: Vec<_> = planned.into_iter().map(|new| super::apply_add(s, new)).collect();
    let all: Vec<u64> = ids.iter().map(|id| id.0).collect();
    record(s, "Step effect", UndoAction::EffectsAdded { kind: AddRemoveKind::AddedOrRemoved, ids }, agent);
    s.render_all_dirty();
    Ok(json!({"effectIds":all,"steps":times.len(),"segments":count,"endMs":run_end,"skipped":{"belowMinStrength":weak,"tooShort":short},"effects":s.effect_count()}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walk_alternate_and_jump_patterns() {
        assert_eq!(pattern("walk", 4, 1, false, 5, 1), vec![vec![0], vec![1], vec![2], vec![3], vec![0]]);
        assert_eq!(pattern("walk", 4, 1, true, 4, 1), vec![vec![0], vec![3], vec![2], vec![1]]);
        assert_eq!(pattern("walk", 4, 2, false, 4, 1), vec![vec![0, 1], vec![1, 2], vec![2, 3], vec![0, 3]]);
        assert_eq!(pattern("alternate", 4, 1, false, 3, 1), vec![vec![0, 2], vec![1, 3], vec![0, 2]]);
        assert_eq!(pattern("alternate", 2, 1, true, 2, 1), vec![vec![1], vec![0]]);
        let jumps = pattern("jump", 8, 2, false, 200, 7);
        assert_eq!(jumps, pattern("jump", 8, 2, false, 200, 7), "seeded: the same figure every time");
        assert_ne!(jumps, pattern("jump", 8, 2, false, 200, 8));
        for pair in jumps.windows(2) {
            assert_eq!(pair[1].len(), 2);
            assert!(pair[1].iter().all(|i| *i < 8 && !pair[0].contains(i)), "no segment repeats from the previous hit: {pair:?}");
        }
        let mut seen: Vec<usize> = jumps.iter().flatten().copied().collect();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), 8, "every position comes up");
    }

    #[test]
    fn runs_merge_neighbours() {
        assert_eq!(runs(&[0, 1, 2, 5, 7, 8]), vec![(0, 2), (5, 5), (7, 8)]);
        assert_eq!(median_gap(&[0.0, 100.0, 300.0, 400.0]), Some(100.0));
        assert_eq!(median_gap(&[5.0]), None);
    }
}
