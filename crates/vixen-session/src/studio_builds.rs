//! Section rebuilds (B80). Effects an agent places carry a build id: the build manifest remembers each one as it was placed,
//! by its file instance id, so a later rebuild of a time range can tell untouched agent effects from hand edits made in the
//! GUI (moved, resized, retargeted, re-layered or re-parametered effects, new ones, deleted ones), and `build_edits` reports
//! them. Labelled saves keep named copies of a sequence that restore in place, in one undo step.
//!
//! The manifest is a sidecar next to the `.tim` (`<name>.studio-builds.json`), so the sequence file stays exactly what
//! Windows Vixen reads. It lives on the session, travels with undo (`UndoAction::StudioBuilds`) and is written on every save.
use super::{apply_add, confirm, ms, op_schema, ops_file, plan_add, plan_band, string, BATCH_LIMIT};
use crate::{
    app::AppCore,
    sequence::{EffectEntry, EffectId, SequenceSession},
    undo::{AddRemoveKind, MoveKind, UndoAction},
};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use uuid::Uuid;
use vixen_core::TimeSpan;

const SIDECAR_SUFFIX: &str = ".studio-builds.json";
/// Labelled saves of `Foo.tim` live in `.versions/Foo/` beside it, hidden from the sequence list.
const VERSIONS_FOLDER: &str = ".versions";
const VERSIONS_INDEX: &str = "versions.json";
/// Effects listed per category in a report or a refusal; counts are always complete.
const LISTED: usize = 200;
const REFUSAL_LISTED: usize = 12;

/// An effect as its build placed it (or as `build_tag` last accepted it).
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub build: String,
    pub type_id: Uuid,
    /// Ticks, as the file holds them.
    pub start: i64,
    pub span: i64,
    pub targets: Vec<Uuid>,
    pub layer: Option<Uuid>,
    /// FNV-1a of the effect's data model as the file writes it.
    pub params: u64,
}

/// One build: a section_rebuild, a tagged effects_batch or a build_tag call.
#[derive(Clone, Debug, PartialEq)]
pub struct BuildRecord {
    pub id: String,
    pub at: String,
    pub range: Option<(f64, f64)>,
    pub placed: usize,
    /// Untagged effects that were already there when builds began to be tracked and still are: never counted as added.
    pub baseline: Vec<Uuid>,
    /// Every untagged effect present right after this build: not counted as added since it (build_edits sinceBuild).
    pub untagged: Vec<Uuid>,
}

/// A sequence's build manifest.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Builds {
    pub records: Vec<BuildRecord>,
    pub placed: BTreeMap<Uuid, Placed>,
    /// The sequence's palette (`sequence_palette`, B81) as `#rrggbb`, and how far a colour may stray from it; it shares
    /// the sidecar so it travels with undo, labelled saves and the file.
    pub palette: Vec<String>,
    pub palette_distance: Option<f64>,
}

fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3))
}

/// Where `sequence`'s manifest lives: `Foo.tim` → `Foo.studio-builds.json`.
pub fn sidecar(sequence: &Path) -> PathBuf {
    let name = sequence.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let stem = name.strip_suffix(".tim").or_else(|| name.strip_suffix(".TIM")).unwrap_or(&name);
    sequence.with_file_name(format!("{stem}{SIDECAR_SUFFIX}"))
}

impl Builds {
    pub fn is_empty(&self) -> bool {
        self.records.is_empty() && self.placed.is_empty() && self.palette.is_empty()
    }

    /// The manifest beside `sequence`, or an empty one when there is none or it can't be read.
    pub fn load(sequence: &Path) -> Builds {
        let Ok(text) = std::fs::read(sidecar(sequence)) else { return Builds::default() };
        serde_json::from_slice::<Value>(&text).ok().and_then(|v| Builds::from_json(&v)).unwrap_or_default()
    }

    /// Writes the manifest beside `sequence`; an empty manifest removes the sidecar.
    pub fn store(&self, sequence: &Path) -> Result<(), String> {
        let path = sidecar(sequence);
        if self.is_empty() {
            return match std::fs::remove_file(&path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
                _ => Ok(()),
            };
        }
        let text = serde_json::to_string(&self.to_json()).map_err(|e| e.to_string())?;
        vixen_io::file::write_atomic(&path, text.as_bytes()).map_err(|e| e.to_string())
    }

    fn to_json(&self) -> Value {
        let builds: Vec<Value> = self.records.iter().map(|r| json!({"id":r.id,"at":r.at,"range":r.range.map(|(a,b)|json!([a,b])),"placed":r.placed,"baseline":r.baseline.iter().map(Uuid::to_string).collect::<Vec<_>>(),"untagged":r.untagged.iter().map(Uuid::to_string).collect::<Vec<_>>()})).collect();
        let effects: Map<String, Value> = self.placed.iter().map(|(id, p)| (id.to_string(), json!({"build":p.build,"typeId":p.type_id.to_string(),"start":p.start,"span":p.span,"targets":p.targets.iter().map(Uuid::to_string).collect::<Vec<_>>(),"layer":p.layer.map(|l|l.to_string()),"params":format!("{:016x}",p.params)}))).collect();
        let mut out = json!({"version":1,"builds":builds,"effects":effects});
        if !self.palette.is_empty() {
            out["palette"] = json!({"colors":self.palette,"distance":self.palette_distance});
        }
        out
    }

    fn from_json(v: &Value) -> Option<Builds> {
        let uuids = |v: &Value| -> Option<Vec<Uuid>> { v.as_array()?.iter().map(|u| u.as_str().and_then(|s| s.parse().ok())).collect() };
        let mut builds = Builds::default();
        for r in v["builds"].as_array()? {
            let range = r["range"].as_array().and_then(|a| Some((a.first()?.as_f64()?, a.get(1)?.as_f64()?)));
            builds.records.push(BuildRecord { id: r["id"].as_str()?.into(), at: r["at"].as_str().unwrap_or_default().into(), range, placed: r["placed"].as_u64().unwrap_or(0) as usize, baseline: uuids(&r["baseline"])?, untagged: uuids(&r["untagged"]).unwrap_or_default() });
        }
        for (id, p) in v["effects"].as_object()? {
            builds.placed.insert(id.parse().ok()?, Placed {
                build: p["build"].as_str()?.into(),
                type_id: p["typeId"].as_str()?.parse().ok()?,
                start: p["start"].as_i64()?,
                span: p["span"].as_i64()?,
                targets: uuids(&p["targets"])?,
                layer: p["layer"].as_str().and_then(|l| l.parse().ok()),
                params: u64::from_str_radix(p["params"].as_str()?, 16).ok()?,
            });
        }
        builds.palette = v["palette"]["colors"].as_array().map(|c| c.iter().filter_map(|c| c.as_str().map(str::to_string)).collect()).unwrap_or_default();
        builds.palette_distance = v["palette"]["distance"].as_f64();
        Some(builds)
    }

    /// The last record of `build`, or of the newest build when `build` is None.
    fn record(&self, build: Option<&str>) -> Option<&BuildRecord> {
        match build {
            Some(id) => self.records.iter().rev().find(|r| r.id == id),
            None => self.records.last(),
        }
    }
}

/// What `entry` is now, in the terms the manifest records.
fn fingerprint(s: &SequenceSession, e: &EffectEntry, build: &str) -> Placed {
    let n = &e.node;
    let model = vixen_io::params::effect_model_to_xml(&n.type_id, &n.instance_id, &n.params)
        .map(|x| x.to_fragment())
        .unwrap_or_else(|_| format!("{:?}{:?}", n.params.values, n.params.unknown_members));
    Placed { build: build.into(), type_id: n.type_id, start: n.start.0, span: n.time_span.0, targets: n.targets.iter().map(|(t, _)| *t).collect(), layer: s.layer_of(e.id), params: fnv(&model) }
}

/// What changed since the effect was placed: empty when untouched.
fn changes(placed: &Placed, now: &Placed) -> Vec<&'static str> {
    let mut out = Vec::new();
    if placed.type_id != now.type_id {
        out.push("type");
    }
    if (placed.start, placed.span) != (now.start, now.span) {
        out.push("timing");
    }
    if placed.targets != now.targets {
        out.push("targets");
    }
    if placed.layer != now.layer {
        out.push("layer");
    }
    if placed.params != now.params {
        out.push("params");
    }
    out
}

/// How an effect stands against the manifest.
enum Standing {
    /// Placed by a build and unchanged since.
    Untouched,
    /// Placed by a build, changed since.
    Edited(String, Vec<&'static str>),
    /// Not from any build.
    Untagged,
}

fn standing(s: &SequenceSession, e: &EffectEntry) -> Standing {
    match s.studio_builds.placed.get(&e.node.instance_id) {
        None => Standing::Untagged,
        Some(p) => {
            let changed = changes(p, &fingerprint(s, e, &p.build));
            if changed.is_empty() {
                Standing::Untouched
            } else {
                Standing::Edited(p.build.clone(), changed)
            }
        }
    }
}

fn effect_json(e: &EffectEntry) -> Value {
    json!({"effectId":e.id.0,"instanceId":e.node.instance_id.to_string(),"typeId":e.node.type_id.to_string(),"effect":e.descriptor.map(|d|d.name),"startMs":e.node.start.total_ms(),"durationMs":e.node.time_span.total_ms(),"targets":e.node.targets.iter().map(|(id,name)|json!({"id":id.to_string(),"name":name})).collect::<Vec<_>>()})
}

fn removed_json(instance: &Uuid, p: &Placed) -> Value {
    json!({"instanceId":instance.to_string(),"buildId":p.build,"typeId":p.type_id.to_string(),"effect":vixen_effect_registry::by_type_id(&p.type_id).map(|d|d.name),"startMs":TimeSpan(p.start).total_ms(),"durationMs":TimeSpan(p.span).total_ms(),"targets":p.targets.iter().map(Uuid::to_string).collect::<Vec<_>>()})
}

fn build_id(a: &Value) -> Result<String, String> {
    let id = a["buildId"].as_str().map(str::trim).unwrap_or_default();
    if id.is_empty() || id.len() > 64 || !id.chars().all(|c| c.is_ascii_alphanumeric() || " _-.:#".contains(c)) {
        return Err("buildId must be 1..64 letters, digits, spaces or _-.:#".into());
    }
    Ok(id.into())
}

fn range(s: &SequenceSession, a: &Value, required: bool) -> Result<Option<(f64, f64)>, String> {
    let Some(r) = a.get("range") else { return if required { Err("range [startMs,endMs] is required".into()) } else { Ok(None) } };
    let (lo, hi) = (r[0].as_f64().unwrap_or(-1.0), r[1].as_f64().unwrap_or(-1.0));
    if !(lo >= 0.0 && hi > lo && hi <= s.length().total_ms() + 0.5) {
        return Err("range must be [startMs,endMs] with startMs < endMs inside the sequence".into());
    }
    Ok(Some((lo, hi)))
}

/// targetIds and every element under them, or None for every target.
fn scope(s: &SequenceSession, a: &Value) -> Result<Option<HashSet<Uuid>>, String> {
    let Some(ids) = a.get("targetIds").and_then(Value::as_array) else { return Ok(None) };
    let tree = s.tree();
    let mut out = HashSet::new();
    for id in ids {
        let id: Uuid = id.as_str().and_then(|t| t.parse().ok()).ok_or("targetIds must be element or group UUIDs")?;
        let node = tree.find(&id).ok_or_else(|| format!("Element {id} not found"))?;
        out.extend(tree.descendants(node).into_iter().map(|n| tree.node(n).id));
    }
    Ok(Some(out))
}

fn in_scope(scope: &Option<HashSet<Uuid>>, targets: impl IntoIterator<Item = Uuid>) -> bool {
    scope.as_ref().is_none_or(|set| targets.into_iter().any(|t| set.contains(&t)))
}

fn overlaps(start: f64, end: f64, range: Option<(f64, f64)>) -> bool {
    range.is_none_or(|(lo, hi)| start < hi && end > lo)
}

fn now_text() -> String {
    let t = vixen_io::LocalDateTime::now();
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", t.year, t.month, t.day, t.hour, t.minute, t.second)
}

/// Tags `ids` as placed by `build` in their current state and adds a build record. Returns the old manifest's undo action.
fn tag(s: &mut SequenceSession, build: &str, ids: &[EffectId], range: Option<(f64, f64)>, label: &str) -> UndoAction {
    let old = s.studio_builds.clone();
    let placed: Vec<(Uuid, Placed)> = ids.iter().filter_map(|id| s.effect(*id)).map(|e| (e.node.instance_id, fingerprint(s, e, build))).collect();
    let count = placed.len();
    s.studio_builds.placed.extend(placed);
    finish_record(s, build, range, count);
    UndoAction::StudioBuilds { label: label.into(), builds: old }
}

fn finish_record(s: &mut SequenceSession, build: &str, range: Option<(f64, f64)>, placed: usize) {
    let untagged: Vec<Uuid> = s.effects().map(|e| e.node.instance_id).filter(|i| !s.studio_builds.placed.contains_key(i)).collect();
    // Effects there before the first build stay out of "added"; anything untagged that appears later is a hand edit until
    // build_tag accepts it, even when a rebuild kept it.
    let baseline = match s.studio_builds.records.last() {
        Some(previous) => {
            let before: HashSet<&Uuid> = previous.baseline.iter().collect();
            untagged.iter().filter(|i| before.contains(i)).copied().collect()
        }
        None => untagged.clone(),
    };
    s.studio_builds.records.push(BuildRecord { id: build.into(), at: now_text(), range, placed, baseline, untagged });
}

/// effects_batch with a buildId: tags the batch's added effects and returns the manifest's undo action.
pub(super) fn tag_batch(s: &mut SequenceSession, a: &Value, added: &[EffectId]) -> Result<Option<UndoAction>, String> {
    if a.get("buildId").is_none() {
        return Ok(None);
    }
    let build = build_id(a)?;
    Ok(Some(tag(s, &build, added, None, "Build tags")))
}

/// The buildId argument of effects_batch and the placers (effects_on_marks, effect_step, motif_place).
pub(super) fn batch_schema() -> Value {
    json!({"type":"string","minLength":1,"maxLength":64,"examples":["intro v2"]})
}

/// Checks a batch's buildId before anything changes.
pub(super) fn check_batch(a: &Value) -> Result<(), String> {
    if a.get("buildId").is_some() {
        build_id(a)?;
    }
    Ok(())
}

pub(super) fn tools() -> Vec<(&'static str, &'static str, Value, Vec<&'static str>, bool)> {
    let string = json!({"type":"string","minLength":1,"maxLength":256});
    let number = json!({"type":"number","minimum":0,"maximum":3600000});
    let range = json!({"type":"array","items":number,"minItems":2,"maxItems":2,"examples":[[64000,96000]]});
    let build = json!({"type":"string","minLength":1,"maxLength":64,"examples":["chorus-2 v3","wiw-2026-11-23T15:40"]});
    let targets = json!({"type":"array","items":string,"minItems":1,"maxItems":4096,"examples":[["<megatree UUID>","<arches group UUID>"]]});
    let label = json!({"type":"string","minLength":1,"maxLength":64,"examples":["WiW v6"]});
    vec![
        ("section_rebuild",
         "Rebuild one section: replace the effects in range [startMs,endMs] (optionally only on targetIds and the elements under them) with ops, tagging every new effect with buildId, in one undo step with one render pass; the rest of the sequence is untouched. ops/opsFile take effects_batch's add and band ops (empty ops just clears the section); every new effect must lie inside the range. Effects that cross the range's edges are trimmed to the outside part (one spanning the whole range is split in two) unless edges:\"keep\". Before changing anything it checks for hand edits in the section: effects a person edited (moved, resized, retargeted, re-layered or changed) since a build placed them, effects not from any build (added in the GUI, or placed before builds were tracked), and build effects since deleted. onHandEdits decides: \"refuse\" (default) changes nothing and lists them (build_edits has the detail); \"keep\" leaves them as they are and replaces only untouched build effects; \"replace\" replaces them too. dryRun:true reports the plan without changing anything. A sequence built before builds were tracked has no tagged effects: tag them once with build_tag, or rebuild with onHandEdits:\"replace\". Example: {sequenceId, range:[64000,96000], buildId:\"chorus-2 v3\", ops:[{op:\"add\",typeId:\"<Pulse>\",targetId:\"<tree>\",startMs:64000,durationMs:500}]}.",
         json!({"sequenceId":string,"range":range,"buildId":build,"ops":{"type":"array","items":op_schema(),"maxItems":BATCH_LIMIT},"opsFile":{"type":"string","minLength":1,"maxLength":1024,"examples":["_show_tools/chorus2.json"]},"targetIds":targets,"onHandEdits":{"type":"string","enum":["refuse","keep","replace"]},"edges":{"type":"string","enum":["trim","keep"]},"dryRun":{"type":"boolean"}}),
         vec!["sequenceId","range","buildId"], false),
        ("build_edits",
         "Report what people changed in the GUI since a build, so an agent's rebuild never silently wipes hand edits. edited: effects a build placed that were since moved, resized, retargeted, re-layered or changed (changed names which: timing, targets, layer, params, type); added: effects from no build that appeared after builds began to be tracked and weren't accepted with build_tag (with sinceBuild, only those that weren't there right after that build); removed: effects a build placed that are gone. untracked counts effects from before tracking began, whose edits can't be seen. range [startMs,endMs] and targetIds (with the elements under them) narrow it. builds lists every build: id, when, range and effects placed. Changes nothing. Edits made with other tools count too: accept them with build_tag. Example: {sequenceId, sinceBuild:\"chorus-2 v3\", range:[64000,96000]}.",
         json!({"sequenceId":string,"sinceBuild":build,"range":range,"targetIds":targets}),
         vec!["sequenceId"], true),
        ("build_tag",
         "Tag effects as placed by buildId in their current state, in one undo step: adopt effects placed before builds were tracked, accept hand edits so the next section_rebuild may replace them, or tag what effects_on_marks, motif_place or effect_step placed without their buildId. Pick effects with effectIds, or with range [startMs,endMs] and/or targetIds (effects overlapping it, on those elements or under them), or all:true. Already tagged effects take the new build id. Example: {sequenceId, buildId:\"baseline\", all:true}.",
         json!({"sequenceId":string,"buildId":build,"effectIds":{"type":"array","items":{"type":"integer","minimum":0},"minItems":1,"maxItems":BATCH_LIMIT},"range":range,"targetIds":targets,"all":{"type":"boolean"}}),
         vec!["sequenceId","buildId"], false),
        ("sequence_versions",
         "List a saved sequence's labelled saves (sequence_save with label), oldest first: label, savedAt, effects and lengthMs. sequence_version_restore puts one back. Changes nothing.",
         json!({"sequenceId":string}),
         vec!["sequenceId"], true),
        ("sequence_version_restore",
         "Put a labelled save back into the open sequence in place, in one undo step: its effects, marks, layers, length and build tags replace the current ones (the song stays). Save afterwards to keep it. Requires confirm:true. Example: {sequenceId, label:\"WiW v6\", confirm:true}.",
         json!({"sequenceId":string,"label":label,"confirm":{"type":"boolean"}}),
         vec!["sequenceId","label","confirm"], false),
    ]
}

/// The sequence_save label schema, for the sequence_save tool entry.
pub(super) fn label_schema() -> Value {
    json!({"type":"string","minLength":1,"maxLength":64,"examples":["WiW v6"]})
}

pub(super) fn execute(core: &AppCore, s: &mut SequenceSession, name: &str, a: &Value, agent: bool) -> Result<Value, String> {
    match name {
        "section_rebuild" => rebuild(core, s, a, agent),
        "build_edits" => report(s, a),
        "build_tag" => tag_tool(s, a, agent),
        "sequence_versions" => versions(s),
        _ => restore(core, s, a, agent),
    }
}

fn report(s: &SequenceSession, a: &Value) -> Result<Value, String> {
    let range = range(s, a, false)?;
    let scope = scope(s, a)?;
    let since = match a.get("sinceBuild") {
        Some(_) => Some(s.studio_builds.record(Some(&build_id(&json!({"buildId":a["sinceBuild"]}))?)).ok_or("sinceBuild is not a build of this sequence; build_edits without it lists them")?),
        None => s.studio_builds.record(None),
    };
    // Not added: effects from before builds were tracked, and with sinceBuild, untagged effects already there after it.
    let mut baseline: HashSet<Uuid> = s.studio_builds.records.last().map(|r| r.baseline.iter().copied().collect()).unwrap_or_default();
    if a.get("sinceBuild").is_some() {
        baseline.extend(since.iter().flat_map(|r| r.untagged.iter().copied()));
    }
    let (mut edited, mut added, mut removed) = (Vec::new(), Vec::new(), Vec::new());
    let (mut counts, mut untracked) = ([0usize; 3], 0usize);
    let mut present = HashSet::new();
    for e in s.effects() {
        present.insert(e.node.instance_id);
        if !overlaps(e.node.start.total_ms(), e.end().total_ms(), range) || !in_scope(&scope, e.node.targets.iter().map(|t| t.0)) {
            continue;
        }
        match standing(s, e) {
            Standing::Untouched => {}
            Standing::Edited(build, changed) => {
                counts[0] += 1;
                if edited.len() < LISTED {
                    let mut v = effect_json(e);
                    v["buildId"] = json!(build);
                    v["changed"] = json!(changed);
                    edited.push(v);
                }
            }
            Standing::Untagged if since.is_some() && !baseline.contains(&e.node.instance_id) => {
                counts[1] += 1;
                if added.len() < LISTED {
                    added.push(effect_json(e));
                }
            }
            Standing::Untagged => untracked += 1,
        }
    }
    for (instance, p) in &s.studio_builds.placed {
        let (start, end) = (TimeSpan(p.start).total_ms(), TimeSpan(p.start + p.span).total_ms());
        if present.contains(instance) || !overlaps(start, end, range) || !in_scope(&scope, p.targets.iter().copied()) {
            continue;
        }
        counts[2] += 1;
        if removed.len() < LISTED {
            removed.push(removed_json(instance, p));
        }
    }
    let builds: Vec<Value> = s.studio_builds.records.iter().map(|r| json!({"buildId":r.id,"at":r.at,"range":r.range.map(|(a,b)|json!([a,b])),"placed":r.placed})).collect();
    let mut out = json!({"sinceBuild":since.map(|r|r.id.clone()),"builds":builds,"edited":edited,"added":added,"removed":removed,
        "counts":{"edited":counts[0],"added":counts[1],"removed":counts[2],"untracked":untracked},"clean":counts.iter().all(|c|*c==0)});
    if since.is_none() {
        out["note"] = json!("No builds recorded for this sequence yet: place effects with section_rebuild, or the buildId of effects_batch, effects_on_marks, effect_step or motif_place, or tag the current ones with build_tag");
    }
    if counts.iter().any(|c| *c > LISTED) {
        out["truncated"] = json!(format!("Lists stop at {LISTED} effects each; narrow with range or targetIds"));
    }
    Ok(out)
}

fn tag_tool(s: &mut SequenceSession, a: &Value, agent: bool) -> Result<Value, String> {
    let build = build_id(a)?;
    let ids: Vec<EffectId> = if let Some(list) = a.get("effectIds").and_then(Value::as_array) {
        if a.get("range").is_some() || a.get("targetIds").is_some() || a.get("all").is_some() {
            return Err("Give effectIds, or range and/or targetIds, or all:true".into());
        }
        list.iter().map(|v| {
            let id = EffectId(v.as_u64().unwrap_or(u64::MAX));
            s.effect(id).map(|_| id).ok_or_else(|| format!("Effect {} not found", v))
        }).collect::<Result<_, _>>()?
    } else {
        let (r, sc) = (range(s, a, false)?, scope(s, a)?);
        if r.is_none() && sc.is_none() && a["all"] != true {
            return Err("Give effectIds, or range and/or targetIds, or all:true".into());
        }
        s.effects().filter(|e| overlaps(e.node.start.total_ms(), e.end().total_ms(), r) && in_scope(&sc, e.node.targets.iter().map(|t| t.0))).map(|e| e.id).collect()
    };
    let range = range(s, a, false)?;
    let undo = tag(s, &build, &ids, range, "Build tags");
    s.set_modified();
    let label = format!("Tag build {build}{}", if agent { " (Agent)" } else { "" });
    s.record_undo(UndoAction::Group { label, actions: vec![undo] });
    Ok(json!({"buildId":build,"tagged":ids.len(),"tracked":s.studio_builds.placed.len()}))
}

/// What a section rebuild would do to one existing effect.
enum Plan {
    Remove(EffectId),
    /// Trim to one side: new start and span.
    Trim(EffectId, TimeSpan, TimeSpan),
    /// Spans the whole range: keep [start, lo), add a copy for [hi, end).
    Split(EffectId),
}

fn rebuild(core: &AppCore, s: &mut SequenceSession, a: &Value, agent: bool) -> Result<Value, String> {
    let (lo, hi) = range(s, a, true)?.unwrap();
    let build = build_id(a)?;
    let mode = a["onHandEdits"].as_str().unwrap_or("refuse");
    let trim = a["edges"].as_str() != Some("keep");
    let scope = scope(s, a)?;
    let file;
    let ops: &[Value] = match (a.get("ops"), a.get("opsFile")) {
        (Some(ops), None) => ops.as_array().unwrap(),
        (None, Some(path)) => {
            file = ops_file(core, path.as_str().unwrap_or_default())?;
            &file
        }
        _ => return Err("Give exactly one of ops or opsFile (an empty ops list clears the section)".into()),
    };
    if ops.len() > BATCH_LIMIT {
        return Err(format!("A rebuild takes at most {BATCH_LIMIT} ops"));
    }
    let schema = op_schema();
    let mut planned = Vec::new();
    for (index, op) in ops.iter().enumerate() {
        (|| -> Result<(), String> {
            crate::studio_schema::arguments(&schema, op)?;
            let kind = op["op"].as_str().unwrap();
            let allowed: &[&str] = match kind {
                "add" => &["op", "typeId", "targetId", "targetIds", "targetSlice", "targetRegion", "startMs", "durationMs", "params", "layerId"],
                "band" => &["op", "targetId", "targetIds", "targetSlice", "targetRegion", "startMs", "durationMs", "layerId", "fromPct", "toPct", "color", "level", "featherPct", "featherLights"],
                _ => return Err(format!("a rebuild only adds effects (add and band ops), not {kind}")),
            };
            if let Some(extra) = op.as_object().unwrap().keys().find(|k| !allowed.contains(&k.as_str())) {
                return Err(format!("{extra} does not apply to a {kind} op"));
            }
            let (start, span) = (ms(op, "startMs")?, ms(op, "durationMs")?);
            if start < lo - 0.5 || start + span > hi + 0.5 {
                return Err(format!("the effect ({start}-{} ms) must lie inside the range {lo}-{hi} ms", start + span));
            }
            planned.extend(if kind == "add" { plan_add(core, s, op)? } else { plan_band(core, s, op)? });
            if planned.len() > BATCH_LIMIT {
                return Err(format!("a rebuild adds at most {BATCH_LIMIT} effects, counting one per target"));
            }
            Ok(())
        })()
        .map_err(|e| format!("ops[{index}]: {e}"))?;
    }

    // Sort the section's effects: untouched build effects go; hand edits wait on onHandEdits.
    let (lo_t, hi_t) = (TimeSpan::from_ms(lo), TimeSpan::from_ms(hi));
    let mut plans = Vec::new();
    let (mut hand, mut hand_count, mut kept) = (Vec::new(), 0usize, 0usize);
    let mut present = HashSet::new();
    for e in s.effects() {
        present.insert(e.node.instance_id);
        let (start, end) = (e.node.start, e.end());
        if !(start < hi_t && end > lo_t) || !in_scope(&scope, e.node.targets.iter().map(|t| t.0)) {
            continue;
        }
        let inside = start >= lo_t && end <= hi_t;
        let hand_edit = match standing(s, e) {
            Standing::Untouched => None,
            Standing::Edited(b, changed) => Some(json!({"kind":"edited","buildId":b,"changed":changed})),
            Standing::Untagged => Some(json!({"kind":"added"})),
        };
        if let Some(kind) = hand_edit {
            hand_count += 1;
            if hand.len() < LISTED {
                let mut v = effect_json(e);
                v.as_object_mut().unwrap().extend(kind.as_object().unwrap().clone());
                hand.push(v);
            }
            if mode != "replace" {
                kept += 1;
                continue;
            }
        }
        if inside {
            plans.push(Plan::Remove(e.id));
        } else if trim && start < lo_t && end > hi_t {
            plans.push(Plan::Split(e.id));
        } else if trim && start < lo_t {
            plans.push(Plan::Trim(e.id, start, lo_t - start));
        } else if trim {
            plans.push(Plan::Trim(e.id, hi_t, end - hi_t));
        }
    }
    let gone: Vec<Uuid> = s.studio_builds.placed.iter()
        .filter(|(i, p)| !present.contains(*i) && p.start < hi_t.0 && p.start + p.span > lo_t.0 && in_scope(&scope, p.targets.iter().copied()))
        .map(|(i, _)| *i).collect();
    for instance in &gone {
        hand_count += 1;
        if hand.len() < LISTED {
            let mut v = removed_json(instance, &s.studio_builds.placed[instance]);
            v["kind"] = json!("removed");
            hand.push(v);
        }
    }
    let count = |f: fn(&Plan) -> bool| plans.iter().filter(|p| f(p)).count();
    let (removing, trimming, splitting) = (count(|p| matches!(p, Plan::Remove(_))), count(|p| matches!(p, Plan::Trim(..))), count(|p| matches!(p, Plan::Split(_))));
    if a["dryRun"] == true {
        return Ok(json!({"dryRun":true,"buildId":build,"range":[lo,hi],"wouldRefuse":mode=="refuse"&&hand_count>0,"add":planned.len(),"remove":removing,"trim":trimming,"split":splitting,"handEdits":hand,"handEditCount":hand_count}));
    }
    if mode == "refuse" && hand_count > 0 {
        let listed: Vec<String> = hand.iter().take(REFUSAL_LISTED).map(|h| {
            let what = match h["kind"].as_str().unwrap() {
                "edited" => format!("edited ({})", h["changed"].as_array().unwrap().iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")),
                "added" => "not from a build".into(),
                _ => "deleted since its build".into(),
            };
            let target = h["targets"].get(0).and_then(|t| t["name"].as_str().or(t.as_str())).unwrap_or("?").to_string();
            format!("{} on {target} at {}-{} ms: {what}", h["effect"].as_str().unwrap_or("effect"), h["startMs"], h["startMs"].as_f64().unwrap_or(0.0) + h["durationMs"].as_f64().unwrap_or(0.0))
        }).collect();
        return Err(format!(
            "Refused: {hand_count} hand edit{} in {lo}-{hi} ms would be lost. Nothing changed. {}{}. Pass onHandEdits:\"keep\" to keep them and replace only untouched build effects, \"replace\" to replace them too, or accept them first with build_tag; build_edits has the detail.",
            if hand_count == 1 { "" } else { "s" },
            listed.join("; "),
            if hand_count > REFUSAL_LISTED { format!("; and {} more", hand_count - REFUSAL_LISTED) } else { String::new() }
        ));
    }

    let old_builds = s.studio_builds.clone();
    let mut actions = vec![UndoAction::StudioBuilds { label: "Build tags".into(), builds: old_builds }];
    // Trims and splits first, then removals, then the new effects.
    let mut times = Vec::new();
    let mut copies = Vec::new();
    let mut retag = Vec::new();
    let mut removing_ids = Vec::new();
    for plan in &plans {
        match plan {
            Plan::Remove(id) => removing_ids.push(*id),
            Plan::Trim(id, start, span) => {
                let e = s.effect(*id).unwrap();
                times.push((*id, e.node.start, e.node.time_span));
                s.set_times(&[(*id, *start, *span)]);
                retag.push(*id);
            }
            Plan::Split(id) => {
                let e = s.effect(*id).unwrap();
                let (node, start, end, layer) = (e.node.clone(), e.node.start, e.end(), s.layer_of(*id));
                times.push((*id, start, e.node.time_span));
                s.set_times(&[(*id, start, lo_t - start)]);
                let (target, target_name) = node.targets.first().cloned().unwrap_or_default();
                if let Some(copy) = s.add_copy(node, target, &target_name, hi_t, end - hi_t) {
                    if let Some(layer) = layer {
                        s.set_layer(&[copy], layer);
                    }
                    copies.push((*id, copy));
                }
                retag.push(*id);
            }
        }
    }
    if !times.is_empty() {
        actions.push(UndoAction::TimesChanged { kind: MoveKind::Moved, times });
    }
    if !copies.is_empty() {
        actions.push(UndoAction::EffectsAdded { kind: AddRemoveKind::AddedOrRemoved, ids: copies.iter().map(|c| c.1).collect() });
    }
    let removed_instances: Vec<Uuid> = removing_ids.iter().filter_map(|id| s.effect(*id)).map(|e| e.node.instance_id).collect();
    if !removing_ids.is_empty() {
        let effects = s.remove_effects(&removing_ids);
        actions.push(UndoAction::EffectsRemoved { kind: AddRemoveKind::AddedOrRemoved, effects });
    }
    let new_ids: Vec<EffectId> = planned.into_iter().map(|new| apply_add(s, new)).collect();
    if !new_ids.is_empty() {
        actions.push(UndoAction::EffectsAdded { kind: AddRemoveKind::AddedOrRemoved, ids: new_ids.clone() });
    }

    // The manifest: removed and acknowledged-deleted effects drop out; trimmed and split build effects keep their build;
    // the new effects are this build's.
    for instance in removed_instances.iter().chain(&gone) {
        s.studio_builds.placed.remove(instance);
    }
    let mut placed = Vec::new();
    for id in &retag {
        let e = s.effect(*id).unwrap();
        if let Some(old) = s.studio_builds.placed.get(&e.node.instance_id) {
            placed.push((e.node.instance_id, fingerprint(s, e, &old.build.clone())));
            if let Some((_, copy)) = copies.iter().find(|c| c.0 == *id) {
                let c = s.effect(*copy).unwrap();
                placed.push((c.node.instance_id, fingerprint(s, c, &old.build)));
            }
        }
    }
    for id in &new_ids {
        let e = s.effect(*id).unwrap();
        placed.push((e.node.instance_id, fingerprint(s, e, &build)));
    }
    s.studio_builds.placed.extend(placed);
    finish_record(s, &build, Some((lo, hi)), new_ids.len());

    let group = UndoAction::Group { label: format!("Rebuild section {build}{}", if agent { " (Agent)" } else { "" }), actions };
    if agent {
        s.studio_agent_flash.note(&group);
    }
    s.record_undo(group);
    s.render_all_dirty();
    let added: Vec<u64> = new_ids.iter().map(|id| id.0).collect();
    Ok(json!({"buildId":build,"range":[lo,hi],"added":added.len(),"effectIds":added,"removed":removing,"trimmed":trimming,"split":splitting,
        "handEditsKept":kept,"handEditsReplaced":if mode=="replace" {hand_count} else {0},"handEdits":hand,"effects":s.effect_count()}))
}

// ---------------------------------------------------------------------------------------------------------------------
// Labelled saves
// ---------------------------------------------------------------------------------------------------------------------

fn versions_dir(sequence: &Path) -> PathBuf {
    let name = sequence.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let stem = name.strip_suffix(".tim").or_else(|| name.strip_suffix(".TIM")).unwrap_or(&name).to_string();
    sequence.with_file_name(VERSIONS_FOLDER).join(stem)
}

fn read_index(dir: &Path) -> Vec<Value> {
    std::fs::read(dir.join(VERSIONS_INDEX)).ok().and_then(|t| serde_json::from_slice::<Value>(&t).ok()).and_then(|v| v["versions"].as_array().cloned()).unwrap_or_default()
}

fn check_label_text(label: &str) -> Result<&str, String> {
    let label = label.trim();
    if label.is_empty() || label.len() > 64 || label.chars().any(|c| c.is_control() || "/\\".contains(c)) {
        return Err("label must be 1..64 characters, without / or \\".into());
    }
    Ok(label)
}

/// sequence_save with a label: refuses a label this sequence file already has, before anything is written.
pub(super) fn check_label(path: &Path, label: &str) -> Result<(), String> {
    let label = check_label_text(label)?;
    if read_index(&versions_dir(path)).iter().any(|v| v["label"].as_str().is_some_and(|l| l.eq_ignore_ascii_case(label))) {
        return Err(format!("This sequence already has a save labelled \"{label}\" (sequence_versions lists them); pick another label"));
    }
    Ok(())
}

/// After a labelled save wrote `path`: keeps a copy of the file and its manifest under the label.
pub(super) fn keep_version(path: &Path, label: &str, effects: usize, length_ms: f64) -> Result<Value, String> {
    let label = check_label_text(label)?;
    let dir = versions_dir(path);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    crate::studio_paths::reject_links(&dir, 0)?;
    let mut index = read_index(&dir);
    let number = index.iter().filter_map(|v| v["file"].as_str()?.strip_suffix(".tim")?.parse::<u32>().ok()).max().unwrap_or(0) + 1;
    let file = format!("{number:03}.tim");
    let copy = dir.join(&file);
    std::fs::copy(path, &copy).map_err(|e| format!("Saved, but the labelled copy failed: {e}"))?;
    if sidecar(path).exists() {
        std::fs::copy(sidecar(path), sidecar(&copy)).map_err(|e| format!("Saved, but the labelled copy's build tags failed: {e}"))?;
    }
    let entry = json!({"label":label,"file":file,"savedAt":now_text(),"effects":effects,"lengthMs":length_ms});
    index.push(entry.clone());
    let text = serde_json::to_string_pretty(&json!({"versions":index})).map_err(|e| e.to_string())?;
    vixen_io::file::write_atomic(&dir.join(VERSIONS_INDEX), text.as_bytes()).map_err(|e| e.to_string())?;
    Ok(entry)
}

fn saved_path(s: &SequenceSession) -> Result<PathBuf, String> {
    s.path().map(Path::to_path_buf).ok_or_else(|| "This sequence has no file yet; sequence_save it first".to_string())
}

fn versions(s: &SequenceSession) -> Result<Value, String> {
    let path = saved_path(s)?;
    let list: Vec<Value> = read_index(&versions_dir(&path)).into_iter().map(|mut v| {
        v.as_object_mut().map(|o| o.remove("file"));
        v
    }).collect();
    Ok(json!({"file":path.file_name().map(|n|n.to_string_lossy().into_owned()),"versions":list}))
}

fn restore(core: &AppCore, s: &mut SequenceSession, a: &Value, agent: bool) -> Result<Value, String> {
    confirm(a)?;
    let path = saved_path(s)?;
    let wanted = check_label_text(string(a, "label")?)?;
    let dir = versions_dir(&path);
    let entry = read_index(&dir).into_iter().find(|v| v["label"].as_str().is_some_and(|l| l.eq_ignore_ascii_case(wanted)))
        .ok_or_else(|| format!("No save labelled \"{wanted}\"; sequence_versions lists this sequence's labels"))?;
    let file = entry["file"].as_str().filter(|f| !f.contains(['/', '\\']) && f.ends_with(".tim")).ok_or("The labelled save's index entry is damaged")?;
    let copy = dir.join(file);
    let document = SequenceSession::studio_load_document(&copy, &core.session_environment()).map_err(|e| e.to_string())?;
    let builds = Builds::load(&copy);

    let mut actions = Vec::new();
    let ids: Vec<EffectId> = s.effects().map(|e| e.id).collect();
    let removed = ids.len();
    if !ids.is_empty() {
        let effects = s.remove_effects(&ids);
        actions.push(UndoAction::EffectsRemoved { kind: AddRemoveKind::AddedOrRemoved, effects });
    }
    actions.push(UndoAction::StudioSettings { label: "Restore labelled save".into(), length: s.length(), layers: s.document().layers.clone() });
    s.set_length(document.length);
    s.document_mut().layers = document.layers.clone();
    s.layers_changed();
    actions.push(UndoAction::StudioMarkCollections { label: "Restore labelled save".into(), marks: s.marks().to_vec(), params: Vec::new() });
    s.set_marks(document.marks.clone());
    let restored = s.studio_insert_nodes(document.effects);
    if !restored.is_empty() {
        actions.push(UndoAction::EffectsAdded { kind: AddRemoveKind::AddedOrRemoved, ids: restored.clone() });
    }
    actions.push(UndoAction::StudioBuilds { label: "Build tags".into(), builds: std::mem::replace(&mut s.studio_builds, builds) });
    let group = UndoAction::Group { label: format!("Restore \"{}\"{}", entry["label"].as_str().unwrap_or(wanted), if agent { " (Agent)" } else { "" }), actions };
    if agent {
        s.studio_agent_flash.note(&group);
    }
    s.record_undo(group);
    s.render_all_dirty();
    Ok(json!({"label":entry["label"],"savedAt":entry["savedAt"],"removed":removed,"restored":restored.len(),"lengthMs":s.length().total_ms(),"marks":s.marks().len()}))
}
