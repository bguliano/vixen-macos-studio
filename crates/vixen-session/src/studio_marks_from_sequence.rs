//! Import timing from another sequence (#61): a `.tim` anywhere on disk, from any profile, read in place. Its effects become mark
//! collections, one per element the effects target or one for all of them, each mark keeping an effect's start and duration with the
//! effect's name as its text; the file's own mark collections come along too. A global offset moves every mark, and snapping moves each
//! mark's start to the nearest mark of a collection already in this sequence. The Mark Collections panel's Import ▸ Timing from
//! Sequence… (`vixen-ffi` `studio_marks_from_sequence.rs`) and the agent tool `marks_import_from_sequence` both build their candidates
//! here and add them through the panel's import commit, one step on the sequence's Undo.
use super::marks_panel::apply;
use super::string;
use crate::{marks as logic, sequence::SequenceSession};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use uuid::Uuid;
use vixen_core::marks::{Mark, MarkCollection};
use vixen_core::{Color, TimeSpan};

/// The largest `.tim` read, as the panel's other imports.
const MAX_BYTES: u64 = 64 * 1024 * 1024;

/// Per-element collections cycle through these, so neighbouring rows are told apart on the timeline.
const PALETTE: [u32; 8] = [0xFFFF5A5A, 0xFF4FC3F7, 0xFF81C784, 0xFFFFD54F, 0xFFBA68C8, 0xFFFF8A65, 0xFF4DB6AC, 0xFFF06292];

/// One effect as timing: where it starts, how long it runs and what it was.
#[derive(Clone, Debug, PartialEq)]
pub struct TimedEffect {
    pub start_ms: f64,
    pub duration_ms: f64,
    pub effect: String,
}

/// What a `.tim` holds for timing: each targeted element's effects, elements in the order they first appear, and its mark collections.
#[derive(Clone, Debug, Default)]
pub struct SourceTiming {
    /// The file's name without `.tim`, which names the collections.
    pub name: String,
    pub elements: Vec<(String, Vec<TimedEffect>)>,
    pub marks: Vec<MarkCollection>,
    pub effect_count: usize,
}

/// How the candidates are built.
#[derive(Clone, Debug, Default)]
pub struct TimingOptions {
    /// One collection for every effect, instead of one per element.
    pub merged: bool,
    /// Added to every mark's start; marks that land before 0 are dropped.
    pub offset_ms: f64,
    /// Start times to snap to, from a collection in this sequence.
    pub snap_to: Vec<f64>,
    /// Only these elements (by name), or all when empty.
    pub elements: Vec<String>,
    /// Also offer the file's own mark collections.
    pub include_marks: bool,
}

/// Reads a `.tim` in place. Nothing is written, and the file's profile is never opened.
pub fn read(path: &Path) -> Result<SourceTiming, String> {
    let size = std::fs::metadata(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?.len();
    if size > MAX_BYTES {
        return Err("Sequence files are limited to 64 MiB".into());
    }
    let sequence = vixen_io::sequence::load_sequence(path).map_err(|e| format!("{} is not a Vixen 3 sequence: {e}", path.display()))?;
    let mut elements: Vec<(String, Vec<TimedEffect>)> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for node in &sequence.effects {
        let effect = vixen_effect_registry::by_type_id(&node.type_id).map_or("Effect", |d| d.name).to_string();
        let timed = TimedEffect { start_ms: node.start.total_ms(), duration_ms: node.time_span.total_ms(), effect };
        for (_, name) in &node.targets {
            let name = if name.trim().is_empty() { "Unnamed element".to_string() } else { name.clone() };
            let i = *index.entry(name.clone()).or_insert_with(|| {
                elements.push((name, Vec::new()));
                elements.len() - 1
            });
            elements[i].1.push(timed.clone());
        }
    }
    Ok(SourceTiming {
        name: path.file_stem().unwrap_or_default().to_string_lossy().into_owned(),
        elements,
        marks: sequence.marks,
        effect_count: sequence.effects.len(),
    })
}

/// The collections to offer, in order: the effects' collections, then the file's own mark collections.
pub fn candidates(source: &SourceTiming, options: &TimingOptions) -> Vec<MarkCollection> {
    let wanted = |name: &str| options.elements.is_empty() || options.elements.iter().any(|e| e == name);
    let mut out = Vec::new();
    if options.merged {
        let effects: Vec<&TimedEffect> = source.elements.iter().filter(|(n, _)| wanted(n)).flat_map(|(_, e)| e).collect();
        if !effects.is_empty() {
            out.push(collection(format!("{} - All effects", source.name), PALETTE[0], effects, options));
        }
    } else {
        for (i, (name, effects)) in source.elements.iter().filter(|(n, _)| wanted(n)).enumerate() {
            out.push(collection(format!("{} - {name}", source.name), PALETTE[i % PALETTE.len()], effects.iter().collect(), options));
        }
    }
    if options.include_marks {
        for c in &source.marks {
            let mut c = c.clone();
            c.id = Uuid::new_v4();
            c.linked_mark_collection_id = None;
            for m in &mut c.marks {
                m.id = Uuid::new_v4();
            }
            let effects: Vec<TimedEffect> = c.marks.iter().map(|m| TimedEffect { start_ms: m.start_time.total_ms(), duration_ms: m.duration.total_ms(), effect: m.text.clone() }).collect();
            c.marks = place(effects.iter().collect(), options, false);
            out.push(c);
        }
    }
    out.retain(|c| !c.marks.is_empty());
    out
}

fn collection(name: String, argb: u32, effects: Vec<&TimedEffect>, options: &TimingOptions) -> MarkCollection {
    let mut c = MarkCollection { name, marks: place(effects, options, true), ..Default::default() };
    c.decorator.color = Color::from_argb_u32(argb);
    c
}

/// Offsets, snaps and sorts the marks. Marks starting on the same millisecond become one (the longest, its text naming every effect)
/// when `merge` is set, as effects on several elements at once give.
fn place(effects: Vec<&TimedEffect>, options: &TimingOptions, merge: bool) -> Vec<Mark> {
    let mut placed: Vec<TimedEffect> = effects
        .into_iter()
        .map(|e| TimedEffect { start_ms: snap(e.start_ms + options.offset_ms, &options.snap_to), ..e.clone() })
        .filter(|e| e.start_ms >= 0.0)
        .collect();
    placed.sort_by(|a, b| a.start_ms.total_cmp(&b.start_ms).then(b.duration_ms.total_cmp(&a.duration_ms)));
    if merge {
        let mut kept: Vec<TimedEffect> = Vec::with_capacity(placed.len());
        for e in placed {
            match kept.last_mut() {
                Some(last) if (e.start_ms - last.start_ms).abs() < 0.5 => {
                    if !last.effect.split(", ").any(|n| n == e.effect) {
                        last.effect = format!("{}, {}", last.effect, e.effect);
                    }
                }
                _ => kept.push(e),
            }
        }
        placed = kept;
    }
    placed
        .into_iter()
        .map(|e| {
            let mut m = Mark::new(TimeSpan::from_ms(e.start_ms.round()));
            m.duration = TimeSpan::from_ms(e.duration_ms.round().max(0.0));
            m.text = e.effect;
            m.apply_minimum_duration();
            m
        })
        .collect()
}

/// The nearest of `points` (sorted), or `time` when there are none.
fn snap(time: f64, points: &[f64]) -> f64 {
    let i = points.partition_point(|p| *p < time);
    [i.checked_sub(1), Some(i)].into_iter().flatten().filter_map(|i| points.get(i)).copied().min_by(|a, b| (a - time).abs().total_cmp(&(b - time).abs())).unwrap_or(time)
}

/// A collection in this sequence to snap to, by id or name, as sorted, distinct start times.
pub fn snap_points(marks: &[MarkCollection], wanted: &str) -> Result<Vec<f64>, String> {
    let found = Uuid::parse_str(wanted)
        .ok()
        .and_then(|id| marks.iter().find(|c| c.id == id))
        .or_else(|| marks.iter().find(|c| c.name == wanted))
        .ok_or_else(|| format!("This sequence has no mark collection \"{wanted}\" to snap to"))?;
    let mut times: Vec<f64> = found.marks.iter().map(|m| m.start_time.total_ms()).collect();
    times.sort_by(f64::total_cmp);
    times.dedup();
    if times.is_empty() {
        return Err(format!("\"{}\" has no marks to snap to", found.name));
    }
    Ok(times)
}

pub fn tools() -> Vec<(&'static str, &'static str, Value, Vec<&'static str>, bool)> {
    let string = json!({"type":"string","minLength":1,"maxLength":256});
    vec![(
        "marks_import_from_sequence",
        "Import another sequence's timing as mark collections, as the Mark Collections panel's Import ▸ Timing from Sequence… does, in one undo step. path is an absolute path to a .tim anywhere on disk (another profile is fine; read only). Each effect becomes a mark with the effect's start and duration and its effect name as text, in one collection per element the effects target (\"<file> - <element>\"), or one collection for all of them with merged:true (\"<file> - All effects\"; effects starting together become one mark). The file's own mark collections come too unless includeMarks:false. offsetMs (-600000 to 600000) moves every mark, e.g. to take out an old rig's latency; marks landing before 0 are dropped. snapTo (a collection id or name in this sequence) moves each mark's start to that collection's nearest mark, after the offset. elements limits it to those element names; collections picks which of the offered collections to add by name (default all). dryRun:true returns what would be added without changing anything. Names already in the sequence get a number.",
        json!({"sequenceId":string,"path":{"type":"string","minLength":1,"maxLength":1024},"merged":{"type":"boolean"},"includeMarks":{"type":"boolean"},
               "offsetMs":{"type":"number","minimum":-600000,"maximum":600000},"snapTo":string,
               "elements":{"type":"array","items":string,"minItems":1,"maxItems":10000},"collections":{"type":"array","items":string,"minItems":1,"maxItems":10000},
               "dryRun":{"type":"boolean"}}),
        vec!["sequenceId", "path"],
        false,
    )]
}

pub fn execute(s: &mut SequenceSession, a: &Value, agent: bool) -> Result<Value, String> {
    let path = Path::new(string(a, "path")?);
    if !path.is_absolute() {
        return Err("path must be absolute".into());
    }
    let source = read(path)?;
    let names = |key: &str| -> Vec<String> { a.get(key).and_then(Value::as_array).map(|v| v.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default() };
    let elements = names("elements");
    if let Some(missing) = elements.iter().find(|e| !source.elements.iter().any(|(n, _)| n == *e)) {
        let offered: Vec<&str> = source.elements.iter().map(|(n, _)| n.as_str()).collect();
        return Err(format!("No effects in {} target \"{missing}\"; they target {offered:?}", source.name));
    }
    let snap_to = match a.get("snapTo") {
        Some(_) => snap_points(s.marks(), string(a, "snapTo")?)?,
        None => Vec::new(),
    };
    let options = TimingOptions {
        merged: a["merged"] == true,
        offset_ms: a["offsetMs"].as_f64().unwrap_or(0.0),
        snap_to,
        elements,
        include_marks: a["includeMarks"] != false,
    };
    let candidates = candidates(&source, &options);
    let wanted = names("collections");
    if let Some(missing) = wanted.iter().find(|n| !candidates.iter().any(|c| &c.name == *n)) {
        let offered: Vec<&str> = candidates.iter().map(|c| c.name.as_str()).collect();
        return Err(format!("The import offers no collection named \"{missing}\"; it offers {offered:?}"));
    }
    let chosen: Vec<MarkCollection> = candidates.into_iter().filter(|c| wanted.is_empty() || wanted.contains(&c.name)).collect();
    let summary = json!({"file": source.name, "effects": source.effect_count, "elements": source.elements.len(), "sourceMarkCollections": source.marks.len()});
    if a["dryRun"] == true {
        let offered: Vec<Value> = chosen.iter().map(|c| json!({"name": c.name, "marks": c.marks.len()})).collect();
        return Ok(json!({"dryRun": true, "source": summary, "collections": offered}));
    }
    if chosen.is_empty() {
        return Err(format!("{} has no effects or marks to import with these options", source.name));
    }
    let ids: Vec<Uuid> = chosen.iter().map(|c| c.id).collect();
    apply(s, "Import Marks", agent, |marks| if logic::commit_import(marks, chosen) { logic::MarksChange::Collections } else { logic::MarksChange::None });
    let added: Vec<Value> = s.marks().iter().filter(|c| ids.contains(&c.id)).map(|c| json!({"id": c.id.to_string(), "name": c.name, "marks": c.marks.len()})).collect();
    Ok(json!({"source": summary, "added": added}))
}

#[cfg(test)]
mod tests {
    use crate::app::AppCore;
    use serde_json::{json, Value};

    #[test]
    fn effects_of_another_sequence_become_marks_per_element_or_merged() {
        let base = std::env::temp_dir().join(format!("studio-marks-from-seq-{}", uuid::Uuid::new_v4()));
        // The source show: its own profile, two elements, a few effects and a mark collection, saved as a .tim.
        let old = AppCore::open(base.join("old"), true).unwrap();
        let run_old = |name: &str, args: Value| crate::studio_sequence::execute(&old, name, &args, false).unwrap();
        let display = |name: &str| old.studio_display.execute(&old, "element_create", &json!({"name": name}), false).unwrap()["result"]["ids"][0].clone();
        let (relays, pixels) = (display("Relay 1"), display("Pixel Strip"));
        let src = run_old("sequence_create", json!({"lengthMs": 20000}))["sequenceId"].clone();
        let pulse = vixen_effect_registry::all().iter().find(|d| d.name == "Pulse").unwrap().type_id.to_string();
        for (target, start, duration) in [(&relays, 1000, 500), (&relays, 3000, 250), (&pixels, 1000, 800), (&pixels, 6000, 400)] {
            run_old("effect_add", json!({"sequenceId": src, "typeId": pulse, "targetId": target, "startMs": start, "durationMs": duration}));
        }
        run_old("marks_create", json!({"sequenceId": src, "name": "Phrases", "timesMs": [500, 5000]}));
        run_old("sequence_save", json!({"sequenceId": src, "file": "Old Song.tim", "confirm": true}));
        let file = old.paths.sequence_dir().join("Old Song.tim").to_string_lossy().into_owned();
        assert!(std::path::Path::new(&file).exists(), "{file}");
        old.studio_display.stop();
        drop(old);

        // The new show imports it.
        let core = AppCore::open(base.join("new"), true).unwrap();
        let run = |name: &str, args: Value| crate::studio_sequence::execute(&core, name, &args, false).unwrap();
        let fail = |name: &str, args: Value| crate::studio_sequence::execute(&core, name, &args, false).unwrap_err();
        let id = run("sequence_create", json!({"lengthMs": 20000}))["sequenceId"].clone();
        let marks = || run("sequence_marks", json!({"sequenceId": id}));
        let collection = |name: &str| marks().as_array().unwrap().iter().find(|c| c["name"] == name).cloned().unwrap_or_else(|| panic!("{name} in {}", marks()));
        let times = |c: &Value| c["marks"].as_array().unwrap().iter().map(|m| m["timeMs"].as_f64().unwrap()).collect::<Vec<_>>();

        let dry = run("marks_import_from_sequence", json!({"sequenceId": id, "path": file, "dryRun": true}));
        assert_eq!(dry["collections"].as_array().unwrap().len(), 3, "{dry}");
        assert!(marks().as_array().unwrap().is_empty(), "a dry run changes nothing");

        let added = run("marks_import_from_sequence", json!({"sequenceId": id, "path": file, "offsetMs": -35}));
        assert_eq!(added["added"].as_array().unwrap().len(), 3, "{added}");
        assert_eq!(times(&collection("Old Song - Relay 1")), [965.0, 2965.0]);
        assert_eq!(times(&collection("Old Song - Pixel Strip")), [965.0, 5965.0]);
        assert_eq!(times(&collection("Phrases")), [465.0, 4965.0], "the file's own marks come too, offset");
        let undo = run("sequence_status", json!({"sequenceId": id}))["undo"].clone();
        assert_eq!(undo[0], "Import Marks");
        run("sequence_undo", json!({"sequenceId": id}));
        assert!(marks().as_array().unwrap().is_empty(), "one Undo takes the whole import back");

        // Merged and snapped to a beat grid already in the sequence; the two effects at 1 s become one mark.
        run("marks_create", json!({"sequenceId": id, "name": "Beats", "timesMs": [0, 1000, 2000, 3000, 4000, 5000, 6000]}));
        run("marks_import_from_sequence", json!({"sequenceId": id, "path": file, "merged": true, "includeMarks": false, "offsetMs": -35, "snapTo": "Beats"}));
        let all = collection("Old Song - All effects");
        assert_eq!(times(&all), [1000.0, 3000.0, 6000.0], "{all}");
        assert_eq!(all["marks"][0]["text"], "Pulse");

        assert!(fail("marks_import_from_sequence", json!({"sequenceId": id, "path": file, "snapTo": "Nope"})).contains("snap"));
        assert!(fail("marks_import_from_sequence", json!({"sequenceId": id, "path": file, "elements": ["Arch"]})).contains("Relay 1"));
        assert!(fail("marks_import_from_sequence", json!({"sequenceId": id, "path": "Old Song.tim"})).contains("absolute"));
        core.studio_display.stop();
        drop(core);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn snapping_picks_the_nearest_point() {
        assert_eq!(super::snap(1490.0, &[1000.0, 2000.0]), 1000.0);
        assert_eq!(super::snap(1510.0, &[1000.0, 2000.0]), 2000.0);
        assert_eq!(super::snap(5000.0, &[1000.0, 2000.0]), 2000.0);
        assert_eq!(super::snap(5.0, &[]), 5.0);
    }
}
