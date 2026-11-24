//! Import timing from another sequence (#61): a `.tim` anywhere on disk, from any profile, read in place. Its effects become mark
//! collections, one per element the effects target or one for all of them, each mark keeping an effect's start and duration with the
//! effect's name as its text; the file's own mark collections come along too. A global offset moves every mark, and snapping moves each
//! mark's start to the nearest mark of a collection already in this sequence. The Mark Collections panel's Import ▸ Timing from
//! Sequence… (`vixen-ffi` `studio_marks_from_sequence.rs`) and the agent tool `marks_import_from_sequence` both build their candidates
//! here and add them through the panel's import commit, one step on the sequence's Undo.
use super::marks_panel::apply;
use super::{filename, mark_effects::sequence_audio, rhythm::grid_points, string};
use crate::{app::AppCore, marks as logic, sequence::SequenceSession};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use uuid::Uuid;
use vixen_core::marks::{Mark, MarkCollection};
use vixen_core::{Color, TimeSpan};
use vixen_io::sequence::EffectNode;

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
    /// The effects themselves, which effects_port_from_sequence copies onto this profile's props (B79).
    pub nodes: Vec<EffectNode>,
}

/// A zone: source elements that move together, e.g. every relay of an old roofline, so their timing is one collection and they
/// are re-targeted to Studio props as one (B79).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Zone {
    pub name: String,
    pub elements: Vec<String>,
}

/// How the candidates are built.
#[derive(Clone, Debug, Default)]
pub struct TimingOptions {
    /// One collection for every effect, instead of one per element.
    pub merged: bool,
    /// Added to every mark's start; marks that land before 0 are dropped.
    pub offset_ms: f64,
    /// Start times to snap to, from a collection in this sequence or the song's music grid.
    pub snap_to: Vec<f64>,
    /// Snapping moves a start at most this far; a start further from the grid stays put and counts as off the grid. None snaps all.
    pub snap_tolerance_ms: Option<f64>,
    /// Only these elements (by name), or all when empty.
    pub elements: Vec<String>,
    /// Also offer the file's own mark collections.
    pub include_marks: bool,
    /// One collection per zone instead of per element: these zones, then (with `auto_zones`) every other element by its name
    /// without a trailing number, so "Mega Tree 12" joins "Mega Tree".
    pub zones: Vec<Zone>,
    pub auto_zones: bool,
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
        let timed = TimedEffect { start_ms: node.start.total_ms(), duration_ms: node.time_span.total_ms(), effect: effect_name(node) };
        for name in target_names(node) {
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
        nodes: sequence.effects,
    })
}

/// The effect's name in the registry, or "Effect" for a type this build doesn't know.
pub fn effect_name(node: &EffectNode) -> String {
    vixen_effect_registry::by_type_id(&node.type_id).map_or("Effect", |d| d.name).to_string()
}

/// The names of the elements an effect targets, as `read` lists them.
pub fn target_names(node: &EffectNode) -> Vec<String> {
    node.targets.iter().map(|(_, name)| if name.trim().is_empty() { "Unnamed element".to_string() } else { name.clone() }).collect()
}

/// An element's name without a trailing number and the separator before it ("Relay 12" → "Relay", "Arch1" → "Arch"): the zone
/// `auto_zones` puts it in. A name without a trailing number is its own zone.
pub fn auto_zone(name: &str) -> String {
    let digits = name.trim_end_matches(|c: char| c.is_ascii_digit());
    if digits.len() == name.len() {
        return name.to_string();
    }
    let base = digits.trim_end_matches([' ', '-', '_', '#', '.']);
    if base.is_empty() { name.to_string() } else { base.to_string() }
}

/// The zone an element belongs to: the given zone listing it, else its automatic zone with `auto_zones`, else the element itself.
pub fn zone_of(element: &str, options: &TimingOptions) -> String {
    match options.zones.iter().find(|z| z.elements.iter().any(|e| e == element)) {
        Some(z) => z.name.clone(),
        None if options.auto_zones => auto_zone(element),
        None => element.to_string(),
    }
}

/// Every zone with its elements and effect count, in the order zones first appear in the file.
pub fn zones(source: &SourceTiming, options: &TimingOptions) -> Vec<(Zone, usize)> {
    let mut out: Vec<(Zone, usize)> = Vec::new();
    for (name, effects) in &source.elements {
        let zone = zone_of(name, options);
        match out.iter_mut().find(|(z, _)| z.name == zone) {
            Some((z, count)) => {
                z.elements.push(name.clone());
                *count += effects.len();
            }
            None => out.push((Zone { name: zone, elements: vec![name.clone()] }, effects.len())),
        }
    }
    out
}

/// Where one start lands: moved by the offset, then to the nearest grid point unless that is further than the tolerance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Landing {
    pub to_ms: f64,
    /// How far snapping moved it after the offset (signed).
    pub snap_ms: f64,
    /// Further from the grid than the tolerance, so left where the offset put it.
    pub off_grid: bool,
}

pub fn land(start_ms: f64, options: &TimingOptions) -> Landing {
    let shifted = start_ms + options.offset_ms;
    if options.snap_to.is_empty() {
        return Landing { to_ms: shifted, snap_ms: 0.0, off_grid: false };
    }
    let snapped = snap(shifted, &options.snap_to);
    if options.snap_tolerance_ms.is_some_and(|t| (snapped - shifted).abs() > t + 1e-6) {
        return Landing { to_ms: shifted, snap_ms: 0.0, off_grid: true };
    }
    Landing { to_ms: snapped, snap_ms: snapped - shifted, off_grid: false }
}

/// The dry-run report of what an import or port moves: how many starts snapping moved, how far, which were off the grid, which
/// were dropped or merged, and the furthest moves to check by ear.
#[derive(Clone, Debug, Default)]
pub struct Moves {
    pub events: usize,
    pub snapped: usize,
    pub off_grid: usize,
    pub dropped: usize,
    pub merged: usize,
    total_snap_ms: f64,
    max_snap_ms: f64,
    /// (|snap|, text, from, to), furthest first.
    furthest: Vec<(f64, String, f64, f64)>,
}

/// The furthest moves a report lists.
const FURTHEST: usize = 10;

impl Moves {
    pub fn note(&mut self, text: &str, from_ms: f64, landing: &Landing) {
        self.events += 1;
        let distance = landing.snap_ms.abs();
        if landing.off_grid {
            self.off_grid += 1;
        } else if distance >= 0.5 {
            self.snapped += 1;
            self.total_snap_ms += distance;
            self.max_snap_ms = self.max_snap_ms.max(distance);
            if self.furthest.len() < FURTHEST || distance > self.furthest[FURTHEST - 1].0 {
                self.furthest.push((distance, text.to_string(), from_ms, landing.to_ms));
                self.furthest.sort_by(|a, b| b.0.total_cmp(&a.0));
                self.furthest.truncate(FURTHEST);
            }
        }
    }

    pub fn absorb(&mut self, other: &Moves) {
        self.events += other.events;
        self.snapped += other.snapped;
        self.off_grid += other.off_grid;
        self.dropped += other.dropped;
        self.merged += other.merged;
        self.total_snap_ms += other.total_snap_ms;
        self.max_snap_ms = self.max_snap_ms.max(other.max_snap_ms);
        self.furthest.extend(other.furthest.iter().cloned());
        self.furthest.sort_by(|a, b| b.0.total_cmp(&a.0));
        self.furthest.truncate(FURTHEST);
    }

    pub fn json(&self, options: &TimingOptions) -> Value {
        let round = |v: f64| (v * 10.0).round() / 10.0;
        let grid = !options.snap_to.is_empty();
        json!({
            "events": self.events,
            "offsetMs": options.offset_ms,
            "snapped": self.snapped,
            "alreadyOnGrid": if grid { self.events - self.snapped - self.off_grid } else { 0 },
            "offGrid": self.off_grid,
            "dropped": self.dropped,
            "merged": self.merged,
            "meanSnapMs": if self.snapped > 0 { round(self.total_snap_ms / self.snapped as f64) } else { 0.0 },
            "maxSnapMs": round(self.max_snap_ms),
            "furthest": self.furthest.iter().map(|(_, text, from, to)| json!({"text": text, "fromMs": round(*from), "toMs": round(*to)})).collect::<Vec<_>>(),
        })
    }
}

/// The collections to offer, in order: the effects' collections, then the file's own mark collections.
pub fn candidates(source: &SourceTiming, options: &TimingOptions) -> Vec<MarkCollection> {
    report(source, options).into_iter().map(|(c, _)| c).collect()
}

/// The collections to offer with what each moves: the effects' collections (one merged, one per zone, or one per element), then the
/// file's own mark collections. Empty collections are left out.
pub fn report(source: &SourceTiming, options: &TimingOptions) -> Vec<(MarkCollection, Moves)> {
    let wanted = |name: &str| options.elements.is_empty() || options.elements.iter().any(|e| e == name);
    let mut out = Vec::new();
    if options.merged {
        let effects: Vec<&TimedEffect> = source.elements.iter().filter(|(n, _)| wanted(n)).flat_map(|(_, e)| e).collect();
        if !effects.is_empty() {
            let mut moves = Moves::default();
            out.push((collection(format!("{} - All effects", source.name), PALETTE[0], effects, options, &mut moves), moves));
        }
    } else {
        let mut groups: Vec<(String, Vec<&TimedEffect>)> = Vec::new();
        for (name, effects) in source.elements.iter().filter(|(n, _)| wanted(n)) {
            let zone = zone_of(name, options);
            match groups.iter_mut().find(|(z, _)| *z == zone) {
                Some((_, list)) => list.extend(effects),
                None => groups.push((zone, effects.iter().collect())),
            }
        }
        for (i, (zone, effects)) in groups.into_iter().enumerate() {
            let mut moves = Moves::default();
            out.push((collection(format!("{} - {zone}", source.name), PALETTE[i % PALETTE.len()], effects, options, &mut moves), moves));
        }
    }
    if options.include_marks {
        for c in &source.marks {
            let mut c = c.clone();
            c.id = Uuid::new_v4();
            c.linked_mark_collection_id = None;
            let effects: Vec<TimedEffect> = c.marks.iter().map(|m| TimedEffect { start_ms: m.start_time.total_ms(), duration_ms: m.duration.total_ms(), effect: m.text.clone() }).collect();
            let mut moves = Moves::default();
            c.marks = place(effects.iter().collect(), options, false, &mut moves);
            out.push((c, moves));
        }
    }
    out.retain(|(c, _)| !c.marks.is_empty());
    out
}

fn collection(name: String, argb: u32, effects: Vec<&TimedEffect>, options: &TimingOptions, moves: &mut Moves) -> MarkCollection {
    let mut c = MarkCollection { name, marks: place(effects, options, true, moves), ..Default::default() };
    c.decorator.color = Color::from_argb_u32(argb);
    c
}

/// Offsets, snaps and sorts the marks. Marks starting on the same millisecond become one (the longest, its text naming every effect)
/// when `merge` is set, as effects on several elements at once give.
fn place(effects: Vec<&TimedEffect>, options: &TimingOptions, merge: bool, moves: &mut Moves) -> Vec<Mark> {
    let mut placed: Vec<TimedEffect> = Vec::with_capacity(effects.len());
    for e in effects {
        let landing = land(e.start_ms, options);
        moves.note(&e.effect, e.start_ms, &landing);
        if landing.to_ms < 0.0 {
            moves.dropped += 1;
            continue;
        }
        placed.push(TimedEffect { start_ms: landing.to_ms, ..e.clone() });
    }
    placed.sort_by(|a, b| a.start_ms.total_cmp(&b.start_ms).then(b.duration_ms.total_cmp(&a.duration_ms)));
    if merge {
        let mut kept: Vec<TimedEffect> = Vec::with_capacity(placed.len());
        for e in placed {
            match kept.last_mut() {
                Some(last) if (e.start_ms - last.start_ms).abs() < 0.5 => {
                    moves.merged += 1;
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

/// The song's music grid when the call asks for one (grid: 16th, 8th, beat or bar of music_rhythm's bars, from media or the
/// sequence's audio). Read before the sequence is locked: the analysis takes a few seconds.
pub(super) fn music_grid(core: &AppCore, shared: &Arc<Mutex<SequenceSession>>, a: &Value) -> Result<Option<Vec<f64>>, String> {
    let Some(grid) = a.get("grid") else { return Ok(None) };
    if a.get("snapTo").is_some() {
        return Err("Give snapTo or grid, not both".into());
    }
    let per_bar = match grid.as_str() {
        Some("16th") => 16,
        Some("8th") => 8,
        Some("beat") => 4,
        Some("bar") => 1,
        _ => return Err("grid must be 16th, 8th, beat or bar".into()),
    };
    let media = match a.get("media") {
        Some(_) => filename(a, "media")?,
        None => sequence_audio(&shared.lock()).ok_or("The sequence has no audio to take a grid from; pass media, a profile Media filename")?,
    };
    Ok(Some(grid_points(core, &media, per_bar)?))
}

/// The absolute `.tim` the call names.
pub(super) fn source_path(a: &Value) -> Result<&Path, String> {
    let path = Path::new(string(a, "path")?);
    if !path.is_absolute() {
        return Err("path must be absolute".into());
    }
    Ok(path)
}

/// The options the import and the port share, checked against the file: elements, merged, includeMarks, offsetMs, snapTo (or the
/// music grid read by `music_grid`), snapToleranceMs, zones and autoZones.
pub(super) fn options(marks: &[MarkCollection], a: &Value, source: &SourceTiming, grid: Option<Vec<f64>>) -> Result<TimingOptions, String> {
    let names = |v: &Value| -> Vec<String> { v.as_array().map(|v| v.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default() };
    let known = |e: &str| source.elements.iter().any(|(n, _)| n == e);
    let offered = || source.elements.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>();
    let elements = names(&a["elements"]);
    if let Some(missing) = elements.iter().find(|e| !known(e)) {
        return Err(format!("No effects in {} target \"{missing}\"; they target {:?}", source.name, offered()));
    }
    let mut zones: Vec<Zone> = Vec::new();
    for z in a.get("zones").and_then(Value::as_array).into_iter().flatten() {
        let zone = Zone { name: string(z, "name")?.to_string(), elements: names(&z["elements"]) };
        if zone.elements.is_empty() {
            return Err(format!("Zone \"{}\" lists no elements", zone.name));
        }
        if zones.iter().any(|other| other.name == zone.name) {
            return Err(format!("Zone \"{}\" is given twice", zone.name));
        }
        for e in &zone.elements {
            if !known(e) {
                return Err(format!("Zone \"{}\" lists \"{e}\", but no effects in {} target it; they target {:?}", zone.name, source.name, offered()));
            }
            if let Some(other) = zones.iter().find(|other| other.elements.contains(e)) {
                return Err(format!("\"{e}\" is in zones \"{}\" and \"{}\"", other.name, zone.name));
            }
        }
        zones.push(zone);
    }
    let snap_to = match (grid, a.get("snapTo")) {
        (Some(points), _) => points,
        (None, Some(_)) => snap_points(marks, string(a, "snapTo")?)?,
        (None, None) => Vec::new(),
    };
    let snap_tolerance_ms = a["snapToleranceMs"].as_f64();
    if snap_tolerance_ms.is_some() && snap_to.is_empty() {
        return Err("snapToleranceMs needs snapTo or grid".into());
    }
    Ok(TimingOptions {
        merged: a["merged"] == true,
        offset_ms: a["offsetMs"].as_f64().unwrap_or(0.0),
        snap_to,
        snap_tolerance_ms,
        elements,
        include_marks: a["includeMarks"] != false,
        zones,
        auto_zones: a["autoZones"] == true,
    })
}

/// Schema fields the import and the port share.
pub(super) fn shared_schema() -> serde_json::Map<String, Value> {
    let string = json!({"type":"string","minLength":1,"maxLength":256});
    let names = json!({"type":"array","items":string,"minItems":1,"maxItems":10000});
    let fields = json!({
        "sequenceId": string,
        "path": {"type":"string","minLength":1,"maxLength":1024,"examples":["/Users/me/Documents/Vixen 3/Sequence/Wizards in Winter 2019.tim"]},
        "offsetMs": {"type":"number","minimum":-600000,"maximum":600000,"examples":[-35]},
        "snapTo": {"type":"string","minLength":1,"maxLength":256,"examples":["Beats"]},
        "grid": {"type":"string","enum":["16th","8th","beat","bar"],"examples":["16th"]},
        "media": {"type":"string","minLength":1,"maxLength":256,"examples":["Wizards in Winter.mp3"]},
        "snapToleranceMs": {"type":"number","minimum":0,"maximum":10000,"examples":[40]},
        "zones": {"type":"array","minItems":1,"maxItems":1000,"items":{"type":"object","properties":{
            "name":{"type":"string","minLength":1,"maxLength":128},
            "elements": names},"required":["name","elements"],"additionalProperties":false},
            "examples":[[{"name":"Roofline","elements":["Relay 1","Relay 2","Relay 3"]},{"name":"Mega Tree","elements":["Tree 1","Tree 2"]}]]},
        "autoZones": {"type":"boolean"},
        "dryRun": {"type":"boolean"},
    });
    fields.as_object().unwrap().clone()
}

pub fn tools() -> Vec<(&'static str, &'static str, Value, Vec<&'static str>, bool)> {
    let string = json!({"type":"string","minLength":1,"maxLength":256});
    let mut schema = shared_schema();
    schema.insert("merged".into(), json!({"type":"boolean"}));
    schema.insert("includeMarks".into(), json!({"type":"boolean"}));
    schema.insert("elements".into(), json!({"type":"array","items":string,"minItems":1,"maxItems":10000,"examples":[["Relay 1","Relay 2"]]}));
    schema.insert("collections".into(), json!({"type":"array","items":string,"minItems":1,"maxItems":10000,"examples":[["Old Song - Roofline"]]}));
    vec![(
        "marks_import_from_sequence",
        "Import another sequence's timing as mark collections, as the Mark Collections panel's Import ▸ Timing from Sequence… does, in one undo step. path is an absolute path to a .tim anywhere on disk (another profile is fine; read only). Each effect becomes a mark with the effect's start and duration and its effect name as text, in one collection per element the effects target (\"<file> - <element>\"), or one collection for all of them with merged:true (\"<file> - All effects\"; effects starting together become one mark). zones [{name, elements}] groups source elements into one collection per zone (\"<file> - <zone>\"), and autoZones:true puts every other element in a zone by its name without a trailing number (\"Relay 12\" joins \"Relay\"), for re-targeting an old rig's channels to Studio props. The file's own mark collections come too unless includeMarks:false. offsetMs (-600000 to 600000) moves every mark, e.g. -35 takes out an old rig's 35 ms lag; marks landing before 0 are dropped. Then each start snaps, after the offset, to the nearest mark of snapTo (a collection id or name in this sequence) or to grid: the 16th, 8th, beat or bar of the song's bars as music_rhythm reads them (from media, a profile Media filename, or the sequence's audio). snapToleranceMs leaves starts further than that from the grid where they are (reported as offGrid). elements limits it to those element names; collections picks which of the offered collections to add by name (default all). dryRun:true adds nothing and reports, per collection and in total, what moves: events, snapped, alreadyOnGrid, offGrid, dropped, merged, meanSnapMs, maxSnapMs and the furthest moves; the import returns the same moves. Names already in the sequence get a number.",
        Value::Object(schema),
        vec!["sequenceId", "path"],
        false,
    )]
}

pub fn execute(s: &mut SequenceSession, a: &Value, agent: bool, grid: Option<Vec<f64>>) -> Result<Value, String> {
    let source = read(source_path(a)?)?;
    let options = options(s.marks(), a, &source, grid)?;
    let names = |key: &str| -> Vec<String> { a.get(key).and_then(Value::as_array).map(|v| v.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default() };
    let candidates = report(&source, &options);
    let wanted = names("collections");
    if let Some(missing) = wanted.iter().find(|n| !candidates.iter().any(|(c, _)| &c.name == *n)) {
        let offered: Vec<&str> = candidates.iter().map(|(c, _)| c.name.as_str()).collect();
        return Err(format!("The import offers no collection named \"{missing}\"; it offers {offered:?}"));
    }
    let chosen: Vec<(MarkCollection, Moves)> = candidates.into_iter().filter(|(c, _)| wanted.is_empty() || wanted.contains(&c.name)).collect();
    let mut total = Moves::default();
    for (_, m) in &chosen {
        total.absorb(m);
    }
    let summary = json!({"file": source.name, "effects": source.effect_count, "elements": source.elements.len(), "sourceMarkCollections": source.marks.len()});
    if a["dryRun"] == true {
        let offered: Vec<Value> = chosen.iter().map(|(c, m)| json!({"name": c.name, "marks": c.marks.len(), "moves": m.json(&options)})).collect();
        return Ok(json!({"dryRun": true, "source": summary, "collections": offered, "moves": total.json(&options)}));
    }
    if chosen.is_empty() {
        return Err(format!("{} has no effects or marks to import with these options", source.name));
    }
    let ids: Vec<Uuid> = chosen.iter().map(|(c, _)| c.id).collect();
    let chosen: Vec<MarkCollection> = chosen.into_iter().map(|(c, _)| c).collect();
    apply(s, "Import Marks", agent, |marks| if logic::commit_import(marks, chosen) { logic::MarksChange::Collections } else { logic::MarksChange::None });
    let added: Vec<Value> = s.marks().iter().filter(|c| ids.contains(&c.id)).map(|c| json!({"id": c.id.to_string(), "name": c.name, "marks": c.marks.len()})).collect();
    Ok(json!({"source": summary, "added": added, "moves": total.json(&options)}))
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::app::AppCore;
    use serde_json::{json, Value};

    /// The source show: its own profile under `base`, two elements ("Relay 1": Pulses at 1 s and 3 s; "Pixel Strip": at 1 s and
    /// 6 s), a "Phrases" mark collection, saved as "Old Song.tim". Returns the file's path.
    pub(crate) fn old_show(base: &std::path::Path) -> String {
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
        file
    }

    #[test]
    fn effects_of_another_sequence_become_marks_per_element_or_merged() {
        let base = std::env::temp_dir().join(format!("studio-marks-from-seq-{}", uuid::Uuid::new_v4()));
        let file = old_show(&base);

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

        assert_eq!(added["moves"]["events"], 6, "{added}");
        run("sequence_undo", json!({"sequenceId": id}));

        // B79: zones make one collection for the old rig's channels together; effects starting together become one mark.
        let zoned = run("marks_import_from_sequence", json!({"sequenceId": id, "path": file, "includeMarks": false, "dryRun": true, "zones": [{"name": "Rig", "elements": ["Relay 1", "Pixel Strip"]}]}));
        let offered = zoned["collections"].as_array().unwrap();
        assert_eq!(offered.len(), 1, "{zoned}");
        assert_eq!((offered[0]["name"].as_str().unwrap(), offered[0]["marks"].as_u64().unwrap()), ("Old Song - Rig", 3), "{zoned}");
        assert_eq!(zoned["moves"]["merged"], 1, "{zoned}");
        let auto = run("marks_import_from_sequence", json!({"sequenceId": id, "path": file, "includeMarks": false, "dryRun": true, "autoZones": true}));
        let names: Vec<&str> = auto["collections"].as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["Old Song - Relay", "Old Song - Pixel Strip"], "{auto}");

        // Lag correction then snapping, reported: 35 ms off the beat is past a 20 ms tolerance and within a 50 ms one.
        let lagged = json!({"sequenceId": id, "path": file, "includeMarks": false, "offsetMs": -35, "snapTo": "Beats", "dryRun": true});
        let mut strict = lagged.clone();
        strict["snapToleranceMs"] = json!(20);
        let strict = run("marks_import_from_sequence", strict)["moves"].clone();
        assert_eq!((strict["snapped"].as_u64(), strict["offGrid"].as_u64()), (Some(0), Some(4)), "{strict}");
        let mut loose = lagged.clone();
        loose["snapToleranceMs"] = json!(50);
        let loose = run("marks_import_from_sequence", loose)["moves"].clone();
        assert_eq!((loose["snapped"].as_u64(), loose["offGrid"].as_u64(), loose["maxSnapMs"].as_f64()), (Some(4), Some(0), Some(35.0)), "{loose}");
        assert_eq!(loose["furthest"].as_array().unwrap().len(), 4, "{loose}");
        assert_eq!(loose["furthest"][0]["toMs"].as_f64().unwrap() - loose["furthest"][0]["fromMs"].as_f64().unwrap(), 0.0, "lag and snap cancel: {loose}");

        assert!(fail("marks_import_from_sequence", json!({"sequenceId": id, "path": file, "zones": [{"name": "Rig", "elements": ["Arch"]}]})).contains("Arch"));
        assert!(fail("marks_import_from_sequence", json!({"sequenceId": id, "path": file, "zones": [{"name": "A", "elements": ["Relay 1"]}, {"name": "B", "elements": ["Relay 1"]}]})).contains("zones"));
        assert!(fail("marks_import_from_sequence", json!({"sequenceId": id, "path": file, "snapToleranceMs": 10})).contains("snapToleranceMs"));
        assert!(fail("marks_import_from_sequence", json!({"sequenceId": id, "path": file, "snapTo": "Beats", "grid": "16th"})).contains("not both"));
        assert!(fail("marks_import_from_sequence", json!({"sequenceId": id, "path": file, "grid": "16th"})).contains("audio"));
        assert!(fail("marks_import_from_sequence", json!({"sequenceId": id, "path": file, "snapTo": "Nope"})).contains("snap"));
        assert!(fail("marks_import_from_sequence", json!({"sequenceId": id, "path": file, "elements": ["Arch"]})).contains("Relay 1"));
        assert!(fail("marks_import_from_sequence", json!({"sequenceId": id, "path": "Old Song.tim"})).contains("absolute"));
        core.studio_display.stop();
        drop(core);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn auto_zones_drop_a_trailing_number() {
        assert_eq!(super::auto_zone("Relay 12"), "Relay");
        assert_eq!(super::auto_zone("Mega Tree-3"), "Mega Tree");
        assert_eq!(super::auto_zone("Arch1"), "Arch");
        assert_eq!(super::auto_zone("Roofline"), "Roofline");
        assert_eq!(super::auto_zone("42"), "42");
    }

    #[test]
    fn snapping_picks_the_nearest_point() {
        assert_eq!(super::snap(1490.0, &[1000.0, 2000.0]), 1000.0);
        assert_eq!(super::snap(1510.0, &[1000.0, 2000.0]), 2000.0);
        assert_eq!(super::snap(5000.0, &[1000.0, 2000.0]), 2000.0);
        assert_eq!(super::snap(5.0, &[]), 5.0);
    }
}
