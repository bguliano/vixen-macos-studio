//! Passages (B73): the show-building steps agents kept doing by hand over the low-level tools. Find effects by time,
//! section name, type, target, layer or build (effects_find), change all of them in one undo step (effects_edit), copy a
//! choreographed passage to other times or to every matching section with optional retargeting (passage_copy), and read
//! a whole sequence section by section in one call with the problems an agent would otherwise miss (sequence_summary).
//! Sections are the marks sequence_section_marks writes ("Sections"): each mark starts a section named by its text.
use super::{builds, confirm, mark_effects, parameter_values, record, string, uuid, BATCH_LIMIT};
use crate::{
    app::AppCore,
    sequence::{EffectId, SequenceSession},
    undo::{AddRemoveKind, MoveKind, UndoAction},
};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
};
use uuid::Uuid;
use vixen_core::TimeSpan;

/// The mark collection sequence_section_marks writes and music_rhythm reads back.
pub(super) const SECTIONS: &str = "Sections";
/// Effects listed by effects_find unless limit says otherwise, and at most.
const LISTED: usize = 200;
const LIST_LIMIT: usize = 2000;
/// A copy whose end passes the sequence by less than this is trimmed to the end instead of refused (float rounding).
const END_SLACK_MS: f64 = 1.0;

/// One section: its name and [start, end) in milliseconds.
#[derive(Clone, Debug)]
pub(super) struct Section {
    pub(super) name: String,
    pub(super) start: f64,
    pub(super) end: f64,
}

/// The sequence's sections in time order: every mark of the collection starts one, named by its text (or "Section n"),
/// and it lasts until the next mark or the end of the sequence.
pub(super) fn sections(s: &SequenceSession, collection: Option<&str>) -> Result<Vec<Section>, String> {
    let wanted = collection.unwrap_or(SECTIONS);
    let source = mark_effects::collection(s, wanted, "sectionsCollection").map_err(|e| {
        if collection.is_none() {
            format!("{e}. Write sections first with sequence_section_marks (music_rhythm estimates them)")
        } else {
            e
        }
    })?;
    let length = s.length().total_ms();
    let mut marks: Vec<(f64, String)> = source
        .marks
        .iter()
        .map(|m| (m.start_time.total_ms(), m.text.trim().to_string()))
        .filter(|(t, _)| *t < length)
        .collect();
    marks.sort_by(|a, b| a.0.total_cmp(&b.0));
    Ok(marks
        .iter()
        .enumerate()
        .map(|(i, (start, text))| Section {
            name: if text.is_empty() { format!("Section {}", i + 1) } else { text.clone() },
            start: *start,
            end: marks.get(i + 1).map_or(length, |next| next.0),
        })
        .collect())
}

/// Case-insensitive name match where `*` stands for any run of characters: "Chorus*" matches "Chorus 2".
pub(super) fn name_matches(pattern: &str, name: &str) -> bool {
    let (pattern, name) = (pattern.trim().to_lowercase(), name.trim().to_lowercase());
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == name;
    }
    let mut rest = name.as_str();
    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            match rest.strip_prefix(part) {
                Some(r) => rest = r,
                None => return false,
            }
        } else if i == parts.len() - 1 {
            return rest.ends_with(part);
        } else if let Some(at) = rest.find(part) {
            rest = &rest[at + part.len()..];
        } else {
            return false;
        }
    }
    true
}

/// The sections `a[key]` names: one name or pattern, or a list of them, matched against the section collection.
pub(super) fn named_sections(s: &SequenceSession, a: &Value, key: &str) -> Result<Vec<Section>, String> {
    let patterns: Vec<&str> = match &a[key] {
        Value::String(one) => vec![one.as_str()],
        Value::Array(list) if !list.is_empty() && list.len() <= 64 => {
            list.iter().map(|v| v.as_str().ok_or(format!("{key} must be a section name or a list of names"))).collect::<Result<_, _>>()?
        }
        _ => return Err(format!("{key} must be a section name (\"Chorus 1\", \"Chorus*\") or a list of 1..64 of them")),
    };
    let collection = a.get("sectionsCollection").map(|_| string(a, "sectionsCollection")).transpose()?;
    let all = sections(s, collection)?;
    let picked: Vec<Section> = all.iter().filter(|sec| patterns.iter().any(|p| name_matches(p, &sec.name))).cloned().collect();
    if picked.is_empty() {
        let names: Vec<&str> = all.iter().map(|sec| sec.name.as_str()).collect();
        return Err(format!("No section matches {}; the sections are: {}", patterns.join(", "), names.join(", ")));
    }
    Ok(picked)
}

/// One section by name (a pattern must match exactly one), for tools that take `section` in place of `range`.
pub(super) fn section_range(s: &SequenceSession, a: &Value) -> Result<(f64, f64), String> {
    let wanted = string(a, "section")?;
    match named_sections(s, a, "section")?.as_slice() {
        [one] => Ok((one.start, one.end)),
        many => Err(format!("section names one section here; {wanted} matches {}", many.iter().map(|m| m.name.as_str()).collect::<Vec<_>>().join(", "))),
    }
}

/// Which effects a find, edit or copy works on.
struct Filter {
    spans: Vec<(f64, f64)>,
    overlap: bool,
    types: Option<HashSet<Uuid>>,
    targets: Option<HashSet<Uuid>>,
    build: Option<String>,
    layer: Option<Uuid>,
}

fn filter_schema() -> serde_json::Map<String, Value> {
    let string = json!({"type":"string","minLength":1,"maxLength":256});
    let number = json!({"type":"number","minimum":0,"maximum":3600000});
    let names = |examples: Value| json!({"anyOf":[string.clone(),{"type":"array","items":string.clone(),"minItems":1,"maxItems":64}],"examples":examples});
    let mut section = names(json!(["Chorus 1", "Chorus*", ["Verse*", "Bridge 1"]]));
    section["description"] = json!("A section name from the Sections marks, or a list of names; * matches anything (\"Chorus*\" is every chorus)");
    let mut types = names(json!([["Pulse", "Twinkle"]]));
    types["description"] = json!("Effect names or type ids (\"Pulse\", \"Wipe\")");
    json!({
        "sequenceId": string,
        "range": {"type":"array","items":number,"minItems":2,"maxItems":2,"description":"[startMs, endMs]","examples":[[50468,69868]]},
        "section": section,
        "sectionsCollection": {"type":"string","minLength":1,"maxLength":256,"description":"The mark collection holding the sections (default \"Sections\")"},
        "match": {"type":"string","enum":["starts","overlaps"],"description":"starts (default): effects that start inside the range or section; overlaps: any effect touching it"},
        "types": types,
        "targetIds": {"type":"array","items":string,"minItems":1,"maxItems":4096,"description":"Effects on these elements or groups, or on anything under them"},
        "fromBuild": {"type":"string","minLength":1,"maxLength":64,"description":"Only effects that build placed (the buildId of section_rebuild, effects_batch, passage_copy or effects_edit)"},
        "layerId": {"type":"string","minLength":1,"maxLength":256}
    })
    .as_object()
    .unwrap()
    .clone()
}

fn read_filter(core: &AppCore, s: &SequenceSession, a: &Value, need_time: bool) -> Result<Filter, String> {
    let length = s.length().total_ms();
    let spans = match (a.get("range"), a.get("section")) {
        (Some(_), Some(_)) => return Err("Give range or section, not both".into()),
        (Some(range), None) => {
            let r: Vec<f64> = range.as_array().map(|r| r.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
            if r.len() != 2 || r[0] >= r[1] || r[0] >= length {
                return Err(format!("range must be [startMs, endMs] with startMs before endMs, inside the sequence (0..{length})"));
            }
            vec![(r[0], r[1].min(length))]
        }
        (None, Some(_)) => named_sections(s, a, "section")?.into_iter().map(|sec| (sec.start, sec.end)).collect(),
        (None, None) if need_time => return Err("Give range [startMs, endMs] or section (a section name)".into()),
        (None, None) => vec![(0.0, length)],
    };
    let overlap = match a.get("match").and_then(Value::as_str) {
        None | Some("starts") => false,
        Some("overlaps") => true,
        Some(_) => return Err("match must be starts or overlaps".into()),
    };
    let types = match a.get("types") {
        None => None,
        Some(v) => {
            let list: Vec<&str> = match v {
                Value::String(one) => vec![one.as_str()],
                Value::Array(l) => l.iter().filter_map(Value::as_str).collect(),
                _ => return Err("types must be an effect name or a list of them".into()),
            };
            let mut ids = HashSet::new();
            for wanted in list {
                let found = Uuid::parse_str(wanted).ok().and_then(|id| vixen_effect_registry::by_type_id(&id)).or_else(|| {
                    vixen_effect_registry::all().iter().copied().find(|d| d.name.eq_ignore_ascii_case(wanted.trim()))
                });
                ids.insert(found.ok_or(format!("Unknown effect type {wanted}; effect_catalog lists them"))?.type_id);
            }
            Some(ids)
        }
    };
    let targets = match a.get("targetIds") {
        None => None,
        Some(v) => {
            let system = core.system.read();
            let tree = &system.tree;
            let mut ids = HashSet::new();
            for value in v.as_array().ok_or("targetIds must be a list of element ids")? {
                let id = value.as_str().and_then(|t| Uuid::parse_str(t).ok()).ok_or("targetIds must be element UUIDs")?;
                let node = tree.find(&id).ok_or(format!("Target {id} not found"))?;
                let mut stack = vec![node];
                while let Some(n) = stack.pop() {
                    if ids.insert(tree.node(n).id) {
                        stack.extend(tree.node(n).children.iter().copied());
                    }
                }
            }
            Some(ids)
        }
    };
    let build = a.get("fromBuild").map(|_| string(a, "fromBuild").map(str::to_string)).transpose()?;
    let layer = a.get("layerId").map(|_| uuid(a, "layerId")).transpose()?;
    Ok(Filter { spans, overlap, types, targets, build, layer })
}

/// The effects a filter picks, in time order.
fn picked(s: &SequenceSession, f: &Filter) -> Vec<EffectId> {
    let mut out: Vec<(f64, EffectId)> = s
        .effects()
        .filter(|e| {
            let (start, end) = (e.node.start.total_ms(), (e.node.start + e.node.time_span).total_ms());
            f.spans.iter().any(|(a, b)| if f.overlap { start < *b && end > *a } else { start >= *a && start < *b })
                && f.types.as_ref().is_none_or(|t| t.contains(&e.node.type_id))
                && f.targets.as_ref().is_none_or(|t| e.node.targets.iter().any(|(id, _)| t.contains(id)))
                && f.build.as_ref().is_none_or(|b| s.studio_builds.placed.get(&e.node.instance_id).is_some_and(|p| &p.build == b))
                && f.layer.is_none_or(|l| s.layer_of(e.id) == Some(l))
        })
        .map(|e| (e.node.start.total_ms(), e.id))
        .collect();
    out.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1 .0.cmp(&b.1 .0)));
    out.into_iter().map(|(_, id)| id).collect()
}

fn type_name(type_id: &Uuid) -> &'static str {
    vixen_effect_registry::by_type_id(type_id).map_or("Unknown", |d| d.name)
}

fn effect_row(s: &SequenceSession, id: EffectId) -> Value {
    let e = s.effect(id).unwrap();
    let mut row = json!({
        "id": id.0,
        "type": type_name(&e.node.type_id),
        "typeId": e.node.type_id.to_string(),
        "startMs": e.node.start.total_ms(),
        "durationMs": e.node.time_span.total_ms(),
        "targets": e.node.targets.iter().map(|(id, name)| json!({"id": id.to_string(), "name": name})).collect::<Vec<_>>(),
    });
    if let Some(layer) = s.layer_of(id) {
        row["layerId"] = json!(layer.to_string());
    }
    if let Some(p) = s.studio_builds.placed.get(&e.node.instance_id) {
        row["buildId"] = json!(p.build);
    }
    let colors = super::seq_palette::colors(s, e);
    if !colors.is_empty() {
        row["colors"] = json!(colors);
    }
    row
}

pub(super) const FIND_DESCRIPTION: &str = "Find effects by where and what they are, without listing the whole sequence: range [startMs,endMs] or section (a name from the Sections marks, or a list; * matches anything, so \"Chorus*\" is every chorus), types (effect names or type ids), targetIds (effects on those elements or anything under them), fromBuild (effects a build placed) and layerId, all combined. match starts (default) takes effects that start inside the range or section, overlaps any that touch it. Returns total, counts byType and byTarget, and the effects themselves in time order (id, type, startMs, durationMs, targets, colours, layerId, and buildId when a build placed it), limit at a time (default 200, at most 2000) from offset. Read-only. effects_edit and passage_copy take the same filter. Example: {sequenceId, section:\"Chorus*\", types:[\"Pulse\"]}.";

pub(super) const EDIT_DESCRIPTION: &str = "Change every effect a filter picks in one undo step, with one render pass: the bulk edit agents otherwise do with effects_find, effect_parameters and an effects_batch of params ops. Filter as effects_find (range or section, types, targetIds, fromBuild, layerId, match); at least one of them. Edits, applied in this order to each effect: params (name to value as effect_parameters reports them; an effect whose type lacks one of them is left alone and counted in skipped), color (#RRGGBB set as the effect's colour, solid; effects with no colour parameter are skipped), levelScale (multiply the effect's level curve and level settings, e.g. Twinkle's Max and Min, by this: 0.5 halves it, 1.2 brightens; clamped to 100%), shiftMs (move by this much, negative is earlier; an effect that would leave the sequence is refused) and stretch (multiply each duration). delete:true removes them instead (needs confirm:true). buildId given with edits re-tags the edited effects as that build, so a later section_rebuild doesn't treat the agent's own edit as a hand edit. dryRun:true reports what would change without changing anything. Returns matched, edited, skipped (with why), removed and the effect ids. Example: {sequenceId, section:\"Verse*\", types:[\"Twinkle\"], levelScale:0.6}.";

pub(super) const COPY_DESCRIPTION: &str = "Copy a choreographed passage to other places in one undo step, with every parameter exactly as it is (the native copy, as paste does): the source is range [startMs,endMs] or section, optionally narrowed by targetIds/types (effects starting inside it are copied, keeping their offsets from the source start). Destinations: toMs (a list of start times) or toSections (section names or patterns, e.g. \"Chorus*\": every matching section other than the source, each copy starting at that section's start). targetMap {sourceId: destinationId} retargets on the way (e.g. Mini Tree L to Mini Tree R to mirror a passage); unmapped targets keep their own. Each copy is cut at its destination's end (the section's end, or the next toMs, or the sequence end) unless fit:\"keep\": effects starting past it are dropped, ones crossing it trimmed. repeat:true lays the passage end to end until each destination is full, so a short chorus pattern fills a longer chorus (the last pass is cut at the end). replace:true first deletes what is already in each destination span on the targets the copy uses (the copy replaces it; undo brings it back). Copies keep their source effect's layer unless toLayerId is given. buildId tags every copy. dryRun:true reports the plan without changing anything. Returns per destination: startMs, endMs, passes, copied, trimmed, dropped and replaced; and effectIds. Example: {sequenceId, section:\"Chorus 1\", toSections:\"Chorus*\", replace:true}.";

pub(super) const SUMMARY_DESCRIPTION: &str = "The whole sequence in one read, section by section, for planning and reviewing a show: length, song and its length, palette, layers, mark collections, and for each section (the Sections marks, or sectionMs windows when there are none) its start, end, effect count, effects by type, and the props it uses with how many effects each. Problems it flags, each with times: sections with no effects, effects running past the song's end, duplicate effects (same type, target, time and parameters: usually a placement run twice), props used in no section (with previewId and the display's props), and with previewId props that have effects in a section but stay dark through all of it (the effect can't show on that prop, e.g. a colour the prop can't show (a red effect on a white-only string), a Wipe on a one-channel string, or a prop with no Location property); that check renders the schedule (a few seconds). Read-only. Use it before and after a build, and before render_analyze for taste. Example: {sequenceId, previewId}.";

pub(super) fn tools() -> Vec<(&'static str, &'static str, Value, Vec<&'static str>, bool)> {
    let string = json!({"type":"string","minLength":1,"maxLength":256});
    let number = json!({"type":"number","minimum":0,"maximum":3600000});
    let filter = filter_schema();
    let with = |extra: Value| {
        let mut m = filter.clone();
        m.extend(extra.as_object().unwrap().clone());
        Value::Object(m)
    };
    let color = json!({"type":"string","pattern":"^#[0-9a-fA-F]{6}$","examples":["#ff0000"]});
    vec![
        ("effects_find", FIND_DESCRIPTION, with(json!({"limit":{"type":"integer","minimum":1,"maximum":LIST_LIMIT},"offset":{"type":"integer","minimum":0}})), vec!["sequenceId"], true),
        ("effects_edit", EDIT_DESCRIPTION, with(json!({
            "params": {"type":"object","description":"Parameter name to value, as effect_parameters reports them","examples":[{"Speed":4}]},
            "color": color,
            "levelScale": {"type":"number","minimum":0,"maximum":10,"examples":[0.5,1.2]},
            "shiftMs": {"type":"number","minimum":-3600000,"maximum":3600000,"examples":[-35]},
            "stretch": {"type":"number","minimum":0.01,"maximum":100,"examples":[0.5]},
            "buildId": {"type":"string","minLength":1,"maxLength":64,"examples":["chorus v2"]},
            "delete": {"type":"boolean"},
            "confirm": {"type":"boolean"},
            "dryRun": {"type":"boolean"}
        })), vec!["sequenceId"], false),
        ("passage_copy", COPY_DESCRIPTION, with(json!({
            "toMs": {"type":"array","items":number,"minItems":1,"maxItems":256,"examples":[[98998,147481]]},
            "toSections": {"anyOf":[string.clone(),{"type":"array","items":string.clone(),"minItems":1,"maxItems":64}],"examples":["Chorus*"]},
            "targetMap": {"type":"object","description":"Source element id to destination element id","examples":[{"<Mini Tree L id>":"<Mini Tree R id>"}]},
            "buildId": {"type":"string","minLength":1,"maxLength":64,"examples":["chorus v2"]},
            "toLayerId": {"type":"string","minLength":1,"maxLength":256},
            "fit": {"type":"string","enum":["trim","keep"]},
            "repeat": {"type":"boolean","description":"Repeat the passage end to end until each destination is full (a 20 s chorus pattern filling a 40 s chorus)"},
            "replace": {"type":"boolean"},
            "dryRun": {"type":"boolean"}
        })), vec!["sequenceId"], false),
        ("sequence_summary", SUMMARY_DESCRIPTION, json!({
            "sequenceId": string,
            "previewId": string,
            "sectionsCollection": string,
            "sectionMs": {"type":"number","minimum":1000,"maximum":600000,"description":"Window length when the sequence has no Sections marks (default 30000)"}
        }), vec!["sequenceId"], true),
    ]
}

pub(super) fn find(core: &AppCore, s: &SequenceSession, a: &Value) -> Result<Value, String> {
    let f = read_filter(core, s, a, false)?;
    let ids = picked(s, &f);
    let limit = a.get("limit").map_or(Some(LISTED as u64), Value::as_u64).filter(|n| (1..=LIST_LIMIT as u64).contains(n)).ok_or(format!("limit must be 1..{LIST_LIMIT}"))? as usize;
    let offset = a.get("offset").map_or(Some(0), Value::as_u64).ok_or("offset must be a whole number")? as usize;
    let mut by_type: BTreeMap<&str, usize> = BTreeMap::new();
    let mut by_target: BTreeMap<String, usize> = BTreeMap::new();
    for id in &ids {
        let e = s.effect(*id).unwrap();
        *by_type.entry(type_name(&e.node.type_id)).or_default() += 1;
        for (_, name) in &e.node.targets {
            *by_target.entry(name.clone()).or_default() += 1;
        }
    }
    let rows: Vec<Value> = ids.iter().skip(offset).take(limit).map(|id| effect_row(s, *id)).collect();
    let next = if offset + rows.len() < ids.len() { Some(offset + rows.len()) } else { None };
    Ok(json!({"total":ids.len(),"byType":by_type,"byTarget":by_target,"effects":rows,"nextOffset":next}))
}

pub(super) fn edit(core: &AppCore, s: &mut SequenceSession, a: &Value, agent: bool) -> Result<Value, String> {
    if !["range", "section", "types", "targetIds", "fromBuild", "layerId"].iter().any(|k| a.get(*k).is_some()) {
        return Err("Give a filter (range, section, types, targetIds, fromBuild or layerId): effects_edit never edits the whole sequence by accident".into());
    }
    let f = read_filter(core, s, a, false)?;
    let ids = picked(s, &f);
    let dry = a["dryRun"] == true;
    let edits = ["params", "color", "levelScale", "shiftMs", "stretch"];
    if a["delete"] == true {
        if edits.iter().any(|k| a.get(*k).is_some()) {
            return Err("delete:true removes the effects; it doesn't combine with edits".into());
        }
        if dry {
            return Ok(json!({"dryRun":true,"matched":ids.len(),"removed":ids.len(),"effectIds":ids.iter().map(|i| i.0).collect::<Vec<_>>()}));
        }
        confirm(a).map_err(|_| "delete:true needs confirm:true".to_string())?;
        if !ids.is_empty() {
            let effects = s.remove_effects(&ids);
            record(s, "Delete effects", UndoAction::EffectsRemoved { kind: AddRemoveKind::AddedOrRemoved, effects }, agent);
        }
        return Ok(json!({"matched":ids.len(),"removed":ids.len(),"effectIds":ids.iter().map(|i| i.0).collect::<Vec<_>>(),"effects":s.effect_count()}));
    }
    if !edits.iter().any(|k| a.get(*k).is_some()) {
        return Err("Give an edit: params, color, levelScale, shiftMs or stretch (or delete:true)".into());
    }
    let length = s.length().total_ms();
    let level = a.get("levelScale").map(|v| v.as_f64().filter(|n| (0.0..=10.0).contains(n)).ok_or("levelScale must be 0..10")).transpose()?;
    let shift = a.get("shiftMs").map(|v| v.as_f64().filter(|n| n.abs() <= 3_600_000.0).ok_or("shiftMs must be -3600000..3600000")).transpose()?.unwrap_or(0.0);
    let stretch = a.get("stretch").map(|v| v.as_f64().filter(|n| (0.01..=100.0).contains(n)).ok_or("stretch must be 0.01..100")).transpose()?.unwrap_or(1.0);
    let color = a.get("color").map(|_| string(a, "color")).transpose()?;
    if let Some(c) = color {
        if !(c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|ch| ch.is_ascii_hexdigit())) {
            return Err("color must be #RRGGBB".into());
        }
    }
    // Plan every change first; nothing is applied when any effect would leave the sequence.
    let mut plans = Vec::new();
    let mut skipped: BTreeMap<String, usize> = BTreeMap::new();
    for id in &ids {
        let e = s.effect(*id).unwrap();
        let Some(descriptor) = e.descriptor else {
            *skipped.entry("unknown effect type".into()).or_default() += 1;
            continue;
        };
        let mut values = Vec::new();
        if let Some(params) = a.get("params") {
            match parameter_values(descriptor, params) {
                Ok(v) => values.extend(v),
                Err(err) if err.starts_with("Unknown parameter") => {
                    *skipped.entry(format!("{}: {err}", descriptor.name)).or_default() += 1;
                    continue;
                }
                Err(err) => return Err(format!("params: {err}")),
            }
        }
        if let Some(c) = color {
            match mark_effects::color_param(descriptor) {
                Some(p) => {
                    let value = mark_effects::solid_color(p.kind, &json!(c));
                    values.push(super::parameter_value(descriptor, p.name, &value)?);
                }
                None => {
                    *skipped.entry(format!("{}: no colour parameter", descriptor.name)).or_default() += 1;
                    continue;
                }
            }
        }
        if let Some(factor) = level {
            // Level curves and the level sliders (Twinkle's Max and Min, Chase's Default Level...) all scale.
            let levels: Vec<&'static str> = descriptor
                .params
                .iter()
                .filter(|p| mark_effects::LEVEL_PARAMS.contains(&p.name) || matches!(p.editor, vixen_effects::framework::Editor::Level))
                .map(|p| p.name)
                .collect();
            if levels.is_empty() {
                *skipped.entry(format!("{}: no level parameter", descriptor.name)).or_default() += 1;
                continue;
            }
            for name in levels {
                let current = values.iter().find(|(n, _)| *n == name).map(|(_, v)| v.clone()).unwrap_or_else(|| e.node.params.get(name).clone());
                values.retain(|(n, _)| *n != name);
                values.push((name, mark_effects::scale_level(&current, factor)));
            }
        }
        let (start, span) = (e.node.start.total_ms(), e.node.time_span.total_ms());
        let (new_start, new_span) = (start + shift, (span * stretch).max(1.0));
        let timing = shift != 0.0 || stretch != 1.0;
        if timing && (new_start < 0.0 || new_start + new_span > length + END_SLACK_MS) {
            return Err(format!("Effect {} ({} at {:.0} ms) would leave the sequence (0..{length:.0} ms)", id.0, descriptor.name, start));
        }
        plans.push((*id, values, timing.then(|| (new_start, new_span.min(length - new_start)))));
    }
    let edited: Vec<u64> = plans.iter().map(|p| p.0 .0).collect();
    if dry {
        return Ok(json!({"dryRun":true,"matched":ids.len(),"edited":edited.len(),"skipped":skipped,"effectIds":edited}));
    }
    let mut actions = Vec::new();
    let mut old_params = Vec::new();
    let mut old_times = Vec::new();
    let mut new_times = Vec::new();
    for (id, values, timing) in &plans {
        let e = s.effect(*id).unwrap();
        if !values.is_empty() {
            old_params.push((*id, e.node.params.clone()));
        }
        if let Some((start, span)) = timing {
            old_times.push((*id, e.node.start, e.node.time_span));
            new_times.push((*id, TimeSpan::from_ms(*start), TimeSpan::from_ms(*span)));
        }
        for (name, value) in values.clone() {
            s.set_param(*id, name, value);
        }
    }
    if !old_params.is_empty() {
        actions.push(UndoAction::ParamsChanged { label: "Edit effects".into(), params: old_params });
    }
    if !new_times.is_empty() {
        s.set_times(&new_times);
        actions.push(UndoAction::TimesChanged { kind: MoveKind::Moved, times: old_times });
    }
    let plan_ids: Vec<EffectId> = plans.iter().map(|p| p.0).collect();
    if let Some(tags) = builds::tag_batch(s, a, &plan_ids)? {
        actions.push(tags);
    }
    if !actions.is_empty() {
        let group = UndoAction::Group { label: format!("Edit effects{}", if agent { " (Agent)" } else { "" }), actions };
        if agent {
            s.studio_agent_flash.note(&group);
        }
        s.record_undo(group);
        s.render_all_dirty();
    }
    Ok(super::seq_palette::warn(s, json!({"matched":ids.len(),"edited":edited.len(),"skipped":skipped,"effectIds":edited}), &edited))
}

/// One destination of a copy: where it starts, where it must end, and its name when it is a section.
struct Destination {
    name: Option<String>,
    start: f64,
    end: f64,
}

pub(super) fn copy(core: &AppCore, s: &mut SequenceSession, a: &Value, agent: bool) -> Result<Value, String> {
    let f = read_filter(core, s, a, true)?;
    let length = s.length().total_ms();
    let source = match f.spans.as_slice() {
        [one] => *one,
        _ => return Err("passage_copy copies one source: give range, or a section that matches one section".into()),
    };
    if f.overlap {
        return Err("passage_copy copies effects that start inside the source; match doesn't apply".into());
    }
    let ids = picked(s, &f);
    if ids.is_empty() {
        return Err("Nothing to copy: no effects start inside the source with that filter".into());
    }
    let keep = match a.get("fit").and_then(Value::as_str) {
        None | Some("trim") => false,
        Some("keep") => true,
        Some(_) => return Err("fit must be trim or keep".into()),
    };
    let mut destinations = match (a.get("toMs"), a.get("toSections")) {
        (Some(_), Some(_)) | (None, None) => return Err("Give exactly one of toMs or toSections".into()),
        (Some(times), None) => {
            let mut starts: Vec<f64> = Vec::new();
            for t in times.as_array().filter(|t| !t.is_empty() && t.len() <= 256).ok_or("toMs must list 1..256 start times")? {
                let t = t.as_f64().filter(|t| (0.0..length).contains(t)).ok_or(format!("toMs times must be inside the sequence (0..{length})"))?;
                starts.push(t);
            }
            starts.sort_by(f64::total_cmp);
            // One pass ends a source's width later; repeat:true runs on to the next start or the end.
            let width = if a["repeat"] == true { f64::INFINITY } else { source.1 - source.0 };
            starts
                .iter()
                .enumerate()
                .map(|(i, t)| Destination { name: None, start: *t, end: starts.get(i + 1).copied().unwrap_or(length).min(t + width).min(length) })
                .collect::<Vec<_>>()
        }
        (None, Some(_)) => named_sections(s, a, "toSections")?
            .into_iter()
            .filter(|sec| (sec.start - source.0).abs() > 0.5)
            .map(|sec| Destination { name: Some(sec.name), start: sec.start, end: sec.end })
            .collect(),
    };
    if destinations.is_empty() {
        return Err("toSections matches only the source section; nothing to copy to".into());
    }
    if keep {
        for d in &mut destinations {
            d.end = length;
        }
    }
    // Retargeting, checked up front.
    let system = core.system.read();
    let tree = &system.tree;
    let mut map: HashMap<Uuid, (Uuid, String)> = HashMap::new();
    if let Some(m) = a.get("targetMap") {
        let m = m.as_object().filter(|m| !m.is_empty() && m.len() <= 4096).ok_or("targetMap must be an object of source id to destination id")?;
        for (from, to) in m {
            let from = Uuid::parse_str(from).map_err(|_| format!("targetMap key {from} must be an element UUID"))?;
            let to = to.as_str().and_then(|t| Uuid::parse_str(t).ok()).ok_or("targetMap values must be element UUIDs")?;
            let node = tree.find(&to).ok_or(format!("targetMap destination {to} not found"))?;
            map.insert(from, (to, tree.node(node).name.clone()));
        }
    }
    drop(system);
    let layer = a.get("toLayerId").map(|_| uuid(a, "toLayerId")).transpose()?;
    if let Some(l) = layer {
        if !s.document().layers.layers.iter().any(|x| x.id == l) {
            return Err("Layer not found".into());
        }
    }
    // Plan: (destination index, source effect, target, start, span).
    struct Planned {
        dest: usize,
        source: EffectId,
        target: (Uuid, String),
        start: f64,
        span: f64,
    }
    let mut plan = Vec::new();
    let mut report: Vec<Value> = Vec::new();
    let repeat = a["repeat"] == true;
    let width = source.1 - source.0;
    for (i, d) in destinations.iter().enumerate() {
        let (mut copied, mut trimmed, mut dropped) = (0usize, 0usize, 0usize);
        // repeat:true lays the passage end to end until the destination is full; the last pass is cut at its end.
        let passes = if repeat { ((d.end - d.start) / width).ceil().max(1.0) as usize } else { 1 };
        for (pass, id) in (0..passes).flat_map(|k| ids.iter().map(move |id| (k, id))) {
            let e = s.effect(*id).unwrap();
            let start = d.start + pass as f64 * width + (e.node.start.total_ms() - source.0);
            let mut span = e.node.time_span.total_ms();
            if start >= d.end - 0.5 {
                dropped += e.node.targets.len().max(1);
                continue;
            }
            if start + span > d.end + if d.end >= length { END_SLACK_MS } else { 0.0 } {
                trimmed += 1;
            }
            span = span.min(d.end - start);
            if span < 1.0 {
                dropped += 1;
                continue;
            }
            for (target, name) in &e.node.targets {
                let target = map.get(target).cloned().unwrap_or((*target, name.clone()));
                plan.push(Planned { dest: i, source: *id, target, start, span });
                copied += 1;
            }
        }
        report.push(json!({"section":d.name,"startMs":d.start,"endMs":d.end,"passes":passes,"copied":copied,"trimmed":trimmed,"dropped":dropped}));
    }
    if plan.len() > BATCH_LIMIT {
        return Err(format!("The copy would add {} effects, more than {BATCH_LIMIT}", plan.len()));
    }
    // What replace removes: effects in each destination span on the targets the copy uses there.
    let mut doomed: Vec<EffectId> = Vec::new();
    if a["replace"] == true {
        let source_ids: HashSet<EffectId> = ids.iter().copied().collect();
        for (i, d) in destinations.iter().enumerate() {
            let targets: HashSet<Uuid> = plan.iter().filter(|p| p.dest == i).map(|p| p.target.0).collect();
            let hit: Vec<EffectId> = s
                .effects()
                .filter(|e| !source_ids.contains(&e.id))
                .filter(|e| {
                    let st = e.node.start.total_ms();
                    st >= d.start && st < d.end && e.node.targets.iter().any(|(t, _)| targets.contains(t))
                })
                .map(|e| e.id)
                .filter(|id| !doomed.contains(id))
                .collect();
            report[i]["replaced"] = json!(hit.len());
            doomed.extend(hit);
        }
    }
    if a["dryRun"] == true {
        return Ok(json!({"dryRun":true,"source":{"startMs":source.0,"endMs":source.1,"effects":ids.len()},"destinations":report,"added":plan.len(),"replaced":doomed.len()}));
    }
    let mut actions = Vec::new();
    if !doomed.is_empty() {
        let effects = s.remove_effects(&doomed);
        actions.push(UndoAction::EffectsRemoved { kind: AddRemoveKind::AddedOrRemoved, effects });
    }
    let mut added = Vec::with_capacity(plan.len());
    for p in plan {
        let node = s.effect(p.source).unwrap().node.clone();
        let own_layer = s.layer_of(p.source);
        let Some(id) = s.add_copy(node, p.target.0, &p.target.1, TimeSpan::from_ms(p.start), TimeSpan::from_ms(p.span)) else { continue };
        if let Some(l) = layer.or(own_layer) {
            s.set_layer(&[id], l);
        }
        added.push(id);
    }
    actions.push(UndoAction::EffectsAdded { kind: AddRemoveKind::Pasted, ids: added.clone() });
    if let Some(tags) = builds::tag_batch(s, a, &added)? {
        actions.push(tags);
    }
    let group = UndoAction::Group { label: format!("Copy passage{}", if agent { " (Agent)" } else { "" }), actions };
    if agent {
        s.studio_agent_flash.note(&group);
    }
    s.record_undo(group);
    s.render_all_dirty();
    Ok(json!({"source":{"startMs":source.0,"endMs":source.1,"effects":ids.len()},"destinations":report,"added":added.len(),"replaced":doomed.len(),"effectIds":added.iter().map(|i| i.0).collect::<Vec<_>>(),"effects":s.effect_count()}))
}

/// sequence_summary: reads the document under its lock, then measures lighting per section without holding it.
pub(super) fn summary(core: &AppCore, shared: &Arc<Mutex<SequenceSession>>, a: &Value) -> Result<Value, String> {
    let s = shared.lock();
    let length = s.length().total_ms();
    let collection = a.get("sectionsCollection").map(|_| string(a, "sectionsCollection")).transpose()?;
    let window = a.get("sectionMs").map(|v| v.as_f64().filter(|n| (1000.0..=600_000.0).contains(n)).ok_or("sectionMs must be 1000..600000")).transpose()?.unwrap_or(30_000.0);
    let (secs, from_marks) = match sections(&s, collection) {
        Ok(list) if !list.is_empty() => {
            let mut list = list;
            // Time before the first section mark is a section of its own.
            if list[0].start > 0.5 {
                list.insert(0, Section { name: "(before sections)".into(), start: 0.0, end: list[0].start });
            }
            (list, true)
        }
        Err(e) if collection.is_some() => return Err(e),
        _ => {
            let n = (length / window).ceil().max(1.0) as usize;
            ((0..n).map(|i| Section { name: format!("{}–{}", clock(i as f64 * window), clock(((i + 1) as f64 * window).min(length))), start: i as f64 * window, end: ((i + 1) as f64 * window).min(length) }).collect(), false)
        }
    };
    let song = mark_effects::sequence_audio(&s);
    let song_ms = s.audio().map(|clip| clip.duration().total_ms());
    // Per section: effects by type and by target.
    struct Use {
        effects: usize,
        by_type: BTreeMap<&'static str, usize>,
        by_target: BTreeMap<Uuid, (String, usize)>,
    }
    let mut uses: Vec<Use> = secs.iter().map(|_| Use { effects: 0, by_type: BTreeMap::new(), by_target: BTreeMap::new() }).collect();
    let mut past_song = Vec::new();
    let mut seen: HashMap<(Uuid, Vec<Uuid>, i64, i64, u64), u64> = HashMap::new();
    let mut duplicates = Vec::new();
    let mut used_targets: HashSet<Uuid> = HashSet::new();
    for e in s.effects() {
        let start = e.node.start.total_ms();
        let end = start + e.node.time_span.total_ms();
        let name = type_name(&e.node.type_id);
        if let Some(i) = secs.iter().position(|sec| start >= sec.start && start < sec.end) {
            let u = &mut uses[i];
            u.effects += 1;
            *u.by_type.entry(name).or_default() += 1;
            for (id, target) in &e.node.targets {
                u.by_target.entry(*id).or_insert((target.clone(), 0)).1 += 1;
            }
        }
        used_targets.extend(e.node.targets.iter().map(|(id, _)| *id));
        if let Some(song_end) = song_ms {
            if end > song_end + 50.0 {
                past_song.push(json!({"id":e.id.0,"type":name,"startMs":start,"endMs":end}));
            }
        }
        let key = (e.node.type_id, e.node.targets.iter().map(|(id, _)| *id).collect::<Vec<_>>(), start.round() as i64, end.round() as i64, fingerprint(&format!("{:?}", e.node.params)));
        match seen.get(&key) {
            Some(first) => duplicates.push(json!({"id":e.id.0,"sameAs":first,"type":name,"startMs":start,"target":e.node.targets.first().map(|t| t.1.clone())})),
            None => {
                seen.insert(key, e.id.0);
            }
        }
    }
    let palette = super::seq_palette::current(&s).map(|(colors, distance)| json!({"colors":colors,"distance":distance}));
    let layers: Vec<Value> = s.document().layers.layers.iter().map(|l| json!({"id":l.id.to_string(),"name":l.name})).collect();
    let marks: Vec<Value> = s.marks().iter().map(|c| json!({"name":c.name,"marks":c.marks.len()})).collect();
    let effect_total = s.effect_count();
    drop(s);

    // Lighting: per section, the props with effects there that stay dark the whole time.
    let mut dark_props: Vec<Vec<Value>> = secs.iter().map(|_| Vec::new()).collect();
    let mut unused: Vec<Value> = Vec::new();
    if a.get("previewId").is_some() {
        let preview = uuid(a, "previewId")?;
        for (i, sec) in secs.iter().enumerate() {
            let targets: Vec<(Uuid, String)> = uses[i].by_target.iter().map(|(id, (name, _))| (*id, name.clone())).take(64).collect();
            if targets.is_empty() || sec.end - sec.start < 200.0 {
                continue;
            }
            let mut elements = Vec::new();
            for (id, _) in &targets {
                if let Ok((name, below)) = super::element_and_below(core, *id) {
                    elements.push((*id, name, below));
                }
            }
            let range = crate::studio_preview::Range { preview_id: preview, start_ms: sec.start, end_ms: sec.end, fps: 10.0, glow: false };
            let thresholds = crate::studio_preview::activity::Thresholds { level: 0.05, min_lit: 0.01, min_ms: 0.0 };
            let measured = crate::studio_preview::activity(core, shared, &range, thresholds, &elements, false)?;
            for el in measured["elements"].as_array().into_iter().flatten() {
                if el["lights"].as_u64().unwrap_or(0) > 0 && el["lit"]["max"].as_f64().unwrap_or(0.0) <= 0.0 {
                    dark_props[i].push(json!({"id":el["id"],"name":el["name"]}));
                }
            }
        }
        // Props in the display that no effect uses, from the preview's own props.
        let system = core.system.read();
        if let Ok(layout) = crate::studio_preview::geometry::layout_props(&system, Some(preview)) {
            let tree = &system.tree;
            for p in layout {
                let node = tree.find(&p.0);
                let covered = node.is_some_and(|n| {
                    let mut stack = vec![n];
                    let mut hit = false;
                    while let Some(x) = stack.pop() {
                        let node = tree.node(x);
                        if used_targets.contains(&node.id) {
                            hit = true;
                            break;
                        }
                        stack.extend(node.children.iter().copied());
                    }
                    // An effect on a group above the prop lights it too.
                    hit || tree.node(n).parents.iter().any(|p| used_targets.contains(&tree.node(*p).id))
                });
                if !covered {
                    unused.push(json!({"id":p.0.to_string(),"name":p.1}));
                }
            }
        }
    }
    let rows: Vec<Value> = secs
        .iter()
        .zip(uses.iter())
        .zip(dark_props.iter())
        .map(|((sec, u), dark)| {
            let mut props: Vec<(&String, &usize)> = u.by_target.values().map(|(n, c)| (n, c)).collect();
            props.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
            let mut row = json!({"name":sec.name,"startMs":sec.start,"endMs":sec.end,"start":clock(sec.start),"effects":u.effects,"byType":u.by_type,"props":props.iter().map(|(n,c)| json!({"name":n,"effects":c})).collect::<Vec<_>>()});
            if !dark.is_empty() {
                row["darkProps"] = json!(dark);
            }
            row
        })
        .collect();
    let empty: Vec<Value> = rows.iter().filter(|r| r["effects"] == 0).map(|r| json!({"name":r["name"],"startMs":r["startMs"],"endMs":r["endMs"]})).collect();
    let dark_count: usize = dark_props.iter().map(Vec::len).sum();
    let mut problems = Vec::new();
    if !empty.is_empty() {
        problems.push(json!({"kind":"emptySections","count":empty.len(),"detail":"sections with no effects starting in them","items":empty}));
    }
    if !past_song.is_empty() {
        problems.push(json!({"kind":"pastSongEnd","count":past_song.len(),"detail":format!("effects that run past the song's end ({})", song_ms.map(clock).unwrap_or_default()),"items":past_song.into_iter().take(50).collect::<Vec<_>>()}));
    }
    if !duplicates.is_empty() {
        problems.push(json!({"kind":"duplicates","count":duplicates.len(),"detail":"effects identical to an earlier one (type, target, times, parameters): usually a placement run twice; effects_edit or sequence_undo removes them","items":duplicates.into_iter().take(50).collect::<Vec<_>>()}));
    }
    if dark_count > 0 {
        problems.push(json!({"kind":"darkProps","count":dark_count,"detail":"props with effects in a section that stay dark through all of it: the effect can't show there: a colour the prop can't show (display_layout lists each prop's colors), a Wipe or other movement on a one-channel prop, or a prop without the Location property"}));
    }
    if !unused.is_empty() {
        problems.push(json!({"kind":"unusedProps","count":unused.len(),"detail":"props in the preview no effect lights","items":unused}));
    }
    Ok(json!({
        "lengthMs": length, "length": clock(length), "song": song, "songMs": song_ms, "effects": effect_total,
        "palette": palette, "layers": layers, "markCollections": marks,
        "sectionsFrom": if from_marks { collection.unwrap_or(SECTIONS).to_string() } else { format!("{}-second windows (no Sections marks; sequence_section_marks writes them)", window / 1000.0) },
        "sections": rows, "problems": problems, "ok": problems.is_empty()
    }))
}

fn fingerprint(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3))
}

/// m:ss.cc, as the timeline shows times.
fn clock(ms: f64) -> String {
    let cs = (ms / 10.0).round() as i64;
    format!("{}:{:02}.{:02}", cs / 6000, (cs / 100) % 60, cs % 100)
}

#[cfg(test)]
mod tests {
    use super::name_matches;

    #[test]
    fn section_patterns() {
        assert!(name_matches("Chorus*", "Chorus 2"));
        assert!(name_matches("chorus 1", "Chorus 1"));
        assert!(!name_matches("Chorus", "Chorus 1"));
        assert!(name_matches("*1", "Verse 1"));
        assert!(name_matches("V*e*1", "Verse 1"));
        assert!(!name_matches("Verse*2", "Verse 1"));
        assert!(name_matches("*", "Intro"));
    }
}
