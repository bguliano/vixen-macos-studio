//! Onset marks and effects_on_marks: detected hits become editable mark collections, and one template effect is placed
//! on every mark of a collection in one undo step.
use super::{
    analysis_audio, filename, onsets, parameter_value, plan_add, record, string, uuid, NewEffect, BATCH_LIMIT,
};
use crate::{
    app::AppCore,
    sequence::SequenceSession,
    undo::{AddRemoveKind, UndoAction},
};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::sync::Arc;
use vixen_core::{marks::Mark, Color, MarkCollection, TimeSpan};
use vixen_effects::framework::{ParamKind, ParamValue};

/// The beat collection sequence_music_marks adds; the default snap grid.
const BEATS: &str = "Music - All";
/// Onset marks are drawn this long; Vixen's minimum mark duration.
const ONSET_MARK_MS: f64 = 25.0;
/// Level parameters levelFromStrength scales, in order of preference.
const LEVEL_PARAMS: &[&str] = &["LevelCurve", "Curve", "PulseCurve", "IntensityLevel"];

/// sequence_onset_marks: one mark collection per band, each mark's text its strength.
pub(super) fn onset_marks(
    core: &AppCore,
    shared: &Arc<Mutex<SequenceSession>>,
    a: &Value,
    agent: bool,
) -> Result<Value, String> {
    let media = match a.get("media") {
        Some(_) => filename(a, "media")?,
        None => sequence_audio(&shared.lock()).ok_or("The sequence has no audio; pass media, a profile Media filename")?,
    };
    let audio = analysis_audio(core, &media)?;
    let bands: Vec<usize> = match a.get("bands") {
        Some(list) => {
            let mut bands: Vec<usize> = list
                .as_array()
                .unwrap()
                .iter()
                .map(|b| onsets::BANDS.iter().position(|n| Some(*n) == b.as_str()).unwrap())
                .collect();
            bands.sort_unstable();
            bands.dedup();
            bands
        }
        None => (0..onsets::BANDS.len()).collect(),
    };
    let min_strength = a.get("minStrength").and_then(Value::as_f64).unwrap_or(0.0);
    let prefix = if a.get("prefix").is_some() { string(a, "prefix")? } else { "Onsets" };
    let detected = onsets::detect(audio.mono(), audio.sample_rate());
    let mut s = shared.lock();
    let length = s.length().total_ms();
    let mut old = s.marks().to_vec();
    let mut collections = old.clone();
    let mut created = Vec::new();
    for band in bands {
        let mut collection = MarkCollection::default();
        collection.name = format!("{prefix} - {}", onsets::BANDS[band]);
        collection.decorator.color = [Color::rgb(230, 80, 60), Color::rgb(90, 200, 90), Color::rgb(80, 150, 240)][band];
        collection.marks = detected
            .iter()
            .filter(|o| o.band == band && o.strength >= min_strength && o.time_ms < length)
            .map(|o| {
                let mut mark = Mark::new(TimeSpan::from_ms(o.time_ms.round()));
                mark.duration = TimeSpan::from_ms(ONSET_MARK_MS);
                mark.text = format!("{:.2}", o.strength);
                mark
            })
            .collect();
        collection.sort_marks();
        created.push(json!({"id":collection.id.to_string(),"name":collection.name,"band":onsets::BANDS[band],"marks":collection.marks.len()}));
        // Undo restores marks into collections it knows and never removes a collection, so it gets the new ones empty.
        old.push(MarkCollection { marks: Vec::new(), ..collection.clone() });
        collections.push(collection);
    }
    s.set_marks(collections);
    record(&mut s, "Onset marks", UndoAction::MarksChanged { label: "Onset marks".into(), marks: old }, agent);
    Ok(json!({"media":media,"collections":created}))
}

/// The file name of the sequence's audio, as stored (a Windows show may store a full path).
fn sequence_audio(s: &SequenceSession) -> Option<String> {
    s.document()
        .media
        .iter()
        .filter(|m| m.type_id == crate::export::AUDIO_MEDIA_TYPE)
        .find_map(|m| m.file_name.rsplit(['/', '\\']).next().filter(|n| !n.is_empty()).map(str::to_string))
}

pub(super) const DESCRIPTION: &str = "Place one template effect on every mark of a mark collection, in one undo step with one render pass: the GUI-visible way to put effects on the rhythm (see sequence_onset_marks). markCollection is the collection's id or exact name. range [startMs,endMs] limits which marks are used (default the whole sequence). A mark's strength is its text read as a number 0..1 (as sequence_onset_marks writes it); a mark whose text is not a number counts as 1, so hand-added marks always fire. minStrength skips weaker marks. snap moves each mark to the nearest beat, 8th or 16th of gridCollection, a beat collection (default \"Music - All\" from sequence_music_marks; 8th halves each beat, 16th quarters it); marks that land on the same point merge, keeping the strongest. template is effect_add's own fields: {typeId, targetId|targetIds|targetSlice, params?, layerId?, rotateTargets?}; every target gets an effect on every mark, or with rotateTargets:true one target per mark in turn. duration is \"untilNext\" (default: each effect lasts until the next mark times gapFraction, default 0.8, so staccato passages keep gaps; the last runs to the range end) or a number of milliseconds; maxMs caps it. levelFromStrength:true scales the effect's level (LevelCurve, Curve, PulseCurve or IntensityLevel, whichever it has) by levelRange[0] + (levelRange[1]-levelRange[0]) * strength percent, default [0,100]. cycle {param, values} (or a list of them) sets that parameter to values[i % n] on the i-th mark, e.g. rotating ColorGradient colours per hit. Returns effectIds, marks used and why others were skipped.";

pub(super) fn schema() -> Value {
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
        "template": {"type":"object","properties":{"typeId":string,"targetId":string,"targetIds":super::target_ids_schema(),"targetSlice":super::target_slice_schema(),"params":{"type":"object"},"layerId":string,"rotateTargets":{"type":"boolean"}},"required":["typeId"],"additionalProperties":false},
        "duration": {"description":"\"untilNext\" or milliseconds"},
        "gapFraction": {"type":"number","minimum":0.05,"maximum":1},
        "maxMs": {"type":"number","minimum":1,"maximum":3600000},
        "levelFromStrength": {"type":"boolean"},
        "levelRange": {"type":"array","items":percent,"minItems":2,"maxItems":2},
        "cycle": {"description":"{param, values} or a list of them", "maxItems":16},
    })
}
fn cycle_schema() -> Value {
    json!({"type":"object","properties":{"param":{"type":"string","minLength":1,"maxLength":256},"values":{"type":"array","minItems":1,"maxItems":256}},"required":["param","values"],"additionalProperties":false})
}

/// A mark collection by id or exact name; an ambiguous or unknown name lists what there is.
fn collection<'a>(s: &'a SequenceSession, wanted: &str, what: &str) -> Result<&'a MarkCollection, String> {
    let marks = s.marks();
    if let Some(c) = marks.iter().find(|c| c.id.to_string() == wanted) {
        return Ok(c);
    }
    let named: Vec<_> = marks.iter().filter(|c| c.name == wanted).collect();
    match named.as_slice() {
        [c] => Ok(c),
        [] => Err(format!(
            "{what} \"{wanted}\" not found; the sequence has: {}",
            if marks.is_empty() { "no mark collections".into() } else { marks.iter().map(|c| format!("\"{}\"", c.name)).collect::<Vec<_>>().join(", ") }
        )),
        _ => Err(format!("{what} \"{wanted}\" names {} collections; pass its id from sequence_marks", named.len())),
    }
}

/// A mark's strength: its text as a number, clamped to 0..1, or 1 when the text is not a number.
fn strength(mark: &Mark) -> f64 {
    mark.text.trim().parse::<f64>().ok().filter(|n| n.is_finite()).map_or(1.0, |n| n.clamp(0.0, 1.0))
}

/// Snap points: each beat interval split into `parts`, extended one beat past either end with the edge intervals.
fn grid(beats: &[f64], parts: usize) -> Vec<f64> {
    if beats.len() < 2 {
        return beats.to_vec();
    }
    let n = beats.len();
    let mut edges = Vec::with_capacity(n + 2);
    edges.push(beats[0] - (beats[1] - beats[0]));
    edges.extend_from_slice(beats);
    edges.push(beats[n - 1] + (beats[n - 1] - beats[n - 2]));
    let mut points: Vec<f64> = edges
        .windows(2)
        .flat_map(|w| (0..parts).map(move |i| w[0] + (w[1] - w[0]) * i as f64 / parts as f64))
        .collect();
    points.push(edges[n + 1]);
    points
}

fn nearest(points: &[f64], t: f64) -> f64 {
    let i = points.partition_point(|p| *p < t);
    [i.checked_sub(1), Some(i)]
        .into_iter()
        .flatten()
        .filter_map(|i| points.get(i))
        .copied()
        .min_by(|a, b| (a - t).abs().total_cmp(&(b - t).abs()))
        .unwrap_or(t)
}

/// A level parameter value scaled by `factor`: curve points' levels, or a 0..1 level.
pub(super) fn scale_level(value: &ParamValue, factor: f64) -> ParamValue {
    match value {
        ParamValue::Curve(curve) => {
            let mut curve = curve.clone();
            curve.points.iter_mut().for_each(|p| p.1 = (p.1 * factor).clamp(0.0, 100.0));
            curve.library_reference_name.clear();
            curve.is_current_library_curve = false;
            ParamValue::Curve(curve)
        }
        ParamValue::Double(v) => ParamValue::Double((v * factor).clamp(0.0, 1.0)),
        ParamValue::Percentage(v) => ParamValue::Percentage((v * factor).clamp(0.0, 1.0)),
        other => other.clone(),
    }
}

struct Hit {
    time: f64,
    strength: f64,
}

/// effects_on_marks: validates everything, then adds every effect as one undo step and renders once.
pub(super) fn effects_on_marks(core: &AppCore, s: &mut SequenceSession, a: &Value, agent: bool) -> Result<Value, String> {
    let length = s.length().total_ms();
    let source = collection(s, string(a, "markCollection")?, "markCollection")?;
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
    let min_strength = a.get("minStrength").and_then(Value::as_f64).unwrap_or(0.0);
    let mut weak = 0usize;
    let mut hits: Vec<Hit> = source
        .marks
        .iter()
        .map(|m| Hit { time: m.start_time.total_ms(), strength: strength(m) })
        .filter(|h| (start..end).contains(&h.time))
        .filter(|h| {
            let keep = h.strength >= min_strength;
            weak += usize::from(!keep);
            keep
        })
        .collect();
    let in_range = hits.len() + weak;
    if let Some(snap) = a.get("snap").and_then(Value::as_str) {
        let wanted = if a.get("gridCollection").is_some() { string(a, "gridCollection")? } else { BEATS };
        let beats = collection(s, wanted, "gridCollection").map_err(|e| {
            if a.get("gridCollection").is_none() { format!("{e}. snap needs a beat collection: run sequence_music_marks, or pass gridCollection") } else { e }
        })?;
        let mut times: Vec<f64> = beats.marks.iter().map(|m| m.start_time.total_ms()).collect();
        times.sort_by(f64::total_cmp);
        times.dedup();
        if times.len() < 2 {
            return Err("gridCollection needs at least two beats to snap to".into());
        }
        let points = grid(&times, match snap { "beat" => 1, "8th" => 2, _ => 4 });
        for hit in &mut hits {
            hit.time = nearest(&points, hit.time).max(0.0);
        }
        hits.retain(|h| (start..end).contains(&h.time));
    }
    hits.sort_by(|a, b| a.time.total_cmp(&b.time).then(b.strength.total_cmp(&a.strength)));
    let before = hits.len();
    // Keep the strongest of marks on the same millisecond (sorted strongest first within a time).
    hits.dedup_by(|later, kept| (later.time - kept.time).abs() < 0.5);
    let merged = before - hits.len();
    if hits.is_empty() {
        return Err(format!("No marks to place effects on: {in_range} in range, {weak} below minStrength"));
    }

    let template = a["template"].as_object().unwrap();
    let rotate = template.get("rotateTargets") == Some(&Value::Bool(true));
    let mut add = template.clone();
    add.remove("rotateTargets");
    add.insert("startMs".into(), json!(0));
    add.insert("durationMs".into(), json!(1));
    let targets = plan_add(core, s, &Value::Object(add)).map_err(|e| format!("template: {e}"))?;
    let type_id = uuid(&a["template"], "typeId")?;
    let descriptor = vixen_effect_registry::by_type_id(&type_id).ok_or("Effect type not found")?;

    let cycles: Vec<(&'static str, Vec<ParamValue>)> = match a.get("cycle") {
        None => Vec::new(),
        Some(value) => {
            let list = match value {
                Value::Array(list) => list.clone(),
                single => vec![single.clone()],
            };
            if list.is_empty() || list.len() > 16 {
                return Err("cycle must be {param, values} or a list of 1..16 of them".into());
            }
            let item = cycle_schema();
            list.iter()
                .enumerate()
                .map(|(i, c)| {
                    crate::studio_schema::arguments(&item, c).map_err(|e| format!("cycle[{i}]: {e}"))?;
                    let param = c["param"].as_str().unwrap();
                    let decoded = c["values"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|v| parameter_value(descriptor, param, v))
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|e| format!("cycle[{i}]: {e}"))?;
                    Ok((decoded[0].0, decoded.into_iter().map(|(_, v)| v).collect()))
                })
                .collect::<Result<_, String>>()?
        }
    };

    let level = if a["levelFromStrength"] == true {
        let param = LEVEL_PARAMS
            .iter()
            .find_map(|n| descriptor.params.iter().find(|p| p.name == *n))
            .filter(|p| matches!(p.kind, ParamKind::Curve | ParamKind::Double | ParamKind::Percentage))
            .ok_or_else(|| format!("levelFromStrength: {} has no level parameter ({})", descriptor.name, LEVEL_PARAMS.join(", ")))?;
        let (lo, hi) = match a.get("levelRange") {
            Some(r) => (r[0].as_f64().unwrap(), r[1].as_f64().unwrap()),
            None => (0.0, 100.0),
        };
        Some((param.name, lo, hi))
    } else {
        None
    };

    let until_next = match a.get("duration") {
        None => true,
        Some(Value::String(s)) if s == "untilNext" => true,
        Some(v) => match v.as_f64() {
            Some(n) if n >= 1.0 && n <= 3_600_000.0 => false,
            _ => return Err("duration must be \"untilNext\" or milliseconds in 1..3600000".into()),
        },
    };
    let gap = a.get("gapFraction").and_then(Value::as_f64).unwrap_or(0.8);
    let max = a.get("maxMs").and_then(Value::as_f64).unwrap_or(f64::INFINITY);
    let fixed = a.get("duration").and_then(Value::as_f64).unwrap_or(0.0);

    let mut planned: Vec<NewEffect> = Vec::new();
    let mut short = 0usize;
    for (i, hit) in hits.iter().enumerate() {
        let span = if until_next { (hits.get(i + 1).map_or(end, |n| n.time) - hit.time) * gap } else { fixed };
        let span = span.min(max).min(length - hit.time);
        if span < 1.0 {
            short += 1;
            continue;
        }
        let chosen: Vec<&NewEffect> = if rotate { vec![&targets[i % targets.len()]] } else { targets.iter().collect() };
        for base in chosen {
            let mut new = base.clone();
            new.start = TimeSpan::from_ms(hit.time);
            new.duration = TimeSpan::from_ms(span);
            new.params.extend(cycles.iter().map(|(name, values)| (*name, values[i % values.len()].clone())));
            new.level = level.map(|(name, lo, hi)| (name, (lo + (hi - lo) * hit.strength) / 100.0));
            planned.push(new);
        }
        if planned.len() > BATCH_LIMIT {
            return Err(format!("effects_on_marks adds at most {BATCH_LIMIT} effects, counting one per target per mark"));
        }
    }
    if planned.is_empty() {
        return Err("Every effect would be shorter than 1 ms; raise gapFraction or use a fixed duration".into());
    }
    let ids: Vec<_> = planned.into_iter().map(|new| super::apply_add(s, new)).collect();
    let all: Vec<u64> = ids.iter().map(|id| id.0).collect();
    record(s, "Place effects on marks", UndoAction::EffectsAdded { kind: AddRemoveKind::AddedOrRemoved, ids }, agent);
    s.render_all_dirty();
    Ok(json!({"effectIds":all,"marksUsed":hits.len() - short,"skipped":{"belowMinStrength":weak,"merged":merged,"tooShort":short},"effects":s.effect_count()}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_splits_beats_and_extends_one_beat_each_way() {
        let points = grid(&[1000.0, 1500.0, 2000.0], 4);
        assert_eq!(points.first(), Some(&500.0));
        assert_eq!(points.last(), Some(&2500.0));
        assert!(points.contains(&1125.0) && points.contains(&1875.0));
        assert_eq!(nearest(&points, 1140.0), 1125.0);
        assert_eq!(nearest(&points, 10.0), 500.0);
    }

    #[test]
    fn strength_reads_mark_text() {
        let mut mark = Mark::new(TimeSpan::from_ms(0.0));
        assert_eq!(strength(&mark), 1.0);
        mark.text = "0.42".into();
        assert_eq!(strength(&mark), 0.42);
        mark.text = "7".into();
        assert_eq!(strength(&mark), 1.0);
        mark.text = "chorus".into();
        assert_eq!(strength(&mark), 1.0);
    }
}
