//! effects_port_from_sequence (B79): an old show's effects copied onto this profile's props, for porting Braden's old sequences. The
//! `.tim` is read in place (`studio_marks_from_sequence.rs`), its elements grouped into zones (given, or by name without a trailing
//! number), and each zone or element routed to Studio targets. Every copy keeps its effect's type and parameters; its start moves by
//! the offset (an old rig's lag) and snaps to a mark collection or the song's music grid, as the timing import does. A dry run reports
//! the zones, routes, unmapped elements and what moves; the port itself is one step on the sequence's Undo.
use super::marks_from_sequence::{self as timing, Moves};
use super::{builds, string, targets};
use crate::{
    app::AppCore,
    sequence::SequenceSession,
    undo::{AddRemoveKind, UndoAction},
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use uuid::Uuid;
use vixen_core::TimeSpan;

/// The most copies one port places, as effects_batch.
const LIMIT: usize = 100_000;

/// Where one zone's or element's effects go.
struct Route {
    from: String,
    targets: Vec<(Uuid, String)>,
    effects: usize,
}

/// One copy to place.
struct Copy {
    node: usize,
    target: (Uuid, String),
    start_ms: f64,
    duration_ms: f64,
}

pub(super) fn tools() -> Vec<(&'static str, &'static str, Value, Vec<&'static str>, bool)> {
    let mut schema = timing::shared_schema();
    schema.insert(
        "map".into(),
        json!({"type":"array","minItems":1,"maxItems":10000,"items":{"type":"object","properties":{
            "from":{"type":"string","minLength":1,"maxLength":256},
            "targetId":{"type":"string","minLength":1,"maxLength":64},
            "targetIds":{"type":"array","items":{"type":"string","minLength":1,"maxLength":64},"minItems":1,"maxItems":4096},
            "targetSlice":{"type":"object"}},"required":["from"],"additionalProperties":false},
            "examples":[[{"from":"Roofline","targetId":"5f0c1d9e-2a7b-4c1e-9a43-6d2f8b7e1a00"},{"from":"Relay 7","targetIds":["0b6e4d2a-8f13-4c55-b1d7-3e9a0c6f2b11","9c2d7e5f-1a04-4b8e-a6c3-5f7d2e9b0c22"]}]]}),
    );
    schema.insert("snapEnds".into(), json!({"type":"boolean"}));
    schema.insert("buildId".into(), json!({"type":"string","minLength":1,"maxLength":64,"examples":["port v1"]}));
    vec![(
        "effects_port_from_sequence",
        "Port an old sequence's effects onto this profile's props in one undo step (B79). path is an absolute path to the old .tim (another profile is fine; read only). Its elements are grouped into zones: zones [{name, elements}] names them, and autoZones:true puts every other element in a zone by its name without a trailing number (\"Relay 12\" joins \"Relay\"); an element in neither is its own zone. map [{from, targetId|targetIds|targetSlice}] routes a zone or a single element (an element's own route wins over its zone's) to Studio targets, as effect_add targets them: every effect on that zone is copied to every target, keeping its type and parameters, once per target even when it covered several of the zone's elements. offsetMs moves every start, e.g. -35 to take out the old rig's 35 ms lag; then each start snaps to snapTo (a mark collection id or name in this sequence) or grid (the 16th, 8th, beat or bar of the song's bars as music_rhythm reads them, from media or the sequence's audio). snapToleranceMs leaves starts further than that from the grid where they are (offGrid). Durations are kept, or with snapEnds:true each end snaps too. Effects landing before 0 or after the sequence's end are skipped, ones running past the end are trimmed, and effect types this build doesn't know are skipped. dryRun:true (map optional) changes nothing and reports zones (each zone's elements and effect count, to plan the map), routes (targets and copies per route), unmapped elements, skipped counts and moves: events, snapped, alreadyOnGrid, offGrid, dropped, meanSnapMs, maxSnapMs and the furthest moves to check by ear. buildId tags the copies as placed by that build (build_edits, section_rebuild). Returns the same report with effectIds.",
        Value::Object(schema),
        vec!["sequenceId", "path"],
        false,
    )]
}

pub(super) fn execute(core: &AppCore, s: &mut SequenceSession, a: &Value, agent: bool, grid: Option<Vec<f64>>) -> Result<Value, String> {
    let source = timing::read(timing::source_path(a)?)?;
    let options = timing::options(s.marks(), a, &source, grid)?;
    let dry = a["dryRun"] == true;
    builds::check_batch(a)?;
    let zones = timing::zones(&source, &options);
    let mut routes: Vec<Route> = Vec::new();
    for entry in a.get("map").and_then(Value::as_array).into_iter().flatten() {
        let from = string(entry, "from")?.to_string();
        if !source.elements.iter().any(|(n, _)| *n == from) && !zones.iter().any(|(z, _)| z.name == from) {
            let names: Vec<&str> = zones.iter().map(|(z, _)| z.name.as_str()).collect();
            return Err(format!("map from \"{from}\" is neither an element nor a zone of {}; its zones are {names:?}", source.name));
        }
        if routes.iter().any(|r| r.from == from) {
            return Err(format!("map routes \"{from}\" twice"));
        }
        let mut fields = entry.clone();
        fields.as_object_mut().unwrap().remove("from");
        let targets = targets(core, &fields).map_err(|e| format!("map from \"{from}\": {e}"))?;
        routes.push(Route { from, targets, effects: 0 });
    }
    if routes.is_empty() && !dry {
        return Err("map is required unless dryRun is true; a dry run lists the zones to route".into());
    }
    let route_of = |element: &str| -> Option<usize> {
        routes.iter().position(|r| r.from == element).or_else(|| {
            let zone = timing::zone_of(element, &options);
            routes.iter().position(|r| r.from == zone)
        })
    };
    let length = s.length().total_ms();
    let mut moves = Moves::default();
    let mut unmapped: Vec<(String, usize)> = Vec::new();
    let (mut past_end, mut trimmed) = (0usize, 0usize);
    let mut unsupported: BTreeMap<String, usize> = BTreeMap::new();
    let mut copies: Vec<Copy> = Vec::new();
    let mut hits = vec![0usize; routes.len()];
    for (i, node) in source.nodes.iter().enumerate() {
        let mut destinations: Vec<(usize, (Uuid, String))> = Vec::new();
        for element in timing::target_names(node) {
            match route_of(&element) {
                Some(r) => {
                    for t in &routes[r].targets {
                        if !destinations.iter().any(|(_, d)| d.0 == t.0) {
                            destinations.push((r, t.clone()));
                        }
                    }
                }
                None => match unmapped.iter_mut().find(|(n, _)| *n == element) {
                    Some((_, count)) => *count += 1,
                    None => unmapped.push((element, 1)),
                },
            }
        }
        // A dry run without a map still reports what every effect's start would do.
        if destinations.is_empty() && !routes.is_empty() {
            continue;
        }
        let start = node.start.total_ms();
        let landing = timing::land(start, &options);
        let name = timing::effect_name(node);
        moves.note(&name, start, &landing);
        let mut duration = node.time_span.total_ms();
        if a["snapEnds"] == true {
            let end = timing::land(start + duration, &options).to_ms;
            if end > landing.to_ms {
                duration = end - landing.to_ms;
            }
        }
        let start = landing.to_ms.round();
        if start < 0.0 {
            moves.dropped += 1;
            continue;
        }
        if start >= length {
            past_end += 1;
            continue;
        }
        if start + duration > length {
            duration = length - start;
            trimmed += 1;
        }
        if vixen_effect_registry::by_type_id(&node.type_id).is_none() {
            *unsupported.entry(node.type_id.to_string()).or_default() += 1;
            continue;
        }
        if destinations.is_empty() {
            continue;
        }
        for (r, target) in destinations {
            hits[r] += 1;
            copies.push(Copy { node: i, target, start_ms: start, duration_ms: duration.round().max(1.0) });
        }
    }
    for (route, count) in routes.iter_mut().zip(hits) {
        route.effects = count;
    }
    if copies.len() > LIMIT {
        return Err(format!("The port would place {} effects; at most {LIMIT} at once (narrow the map)", copies.len()));
    }
    let mut report = json!({
        "source": {"file": source.name, "effects": source.effect_count, "elements": source.elements.len()},
        "zones": zones.iter().map(|(z, count)| json!({"name": z.name, "elements": z.elements, "effects": count})).collect::<Vec<_>>(),
        "routes": routes.iter().map(|r| json!({"from": r.from, "targets": r.targets.iter().map(|t| &t.1).collect::<Vec<_>>(), "effects": r.effects})).collect::<Vec<_>>(),
        "unmapped": unmapped.iter().map(|(e, count)| json!({"element": e, "zone": timing::zone_of(e, &options), "effects": count})).collect::<Vec<_>>(),
        "skipped": {"beforeZero": moves.dropped, "pastEnd": past_end, "trimmed": trimmed, "unsupportedTypes": unsupported},
        "moves": moves.json(&options),
        "effects": copies.len(),
    });
    if dry {
        report["dryRun"] = json!(true);
        return Ok(report);
    }
    if copies.is_empty() {
        return Err(format!("Nothing in {} lands on the mapped targets inside this sequence", source.name));
    }
    let mut ids = Vec::with_capacity(copies.len());
    for copy in copies {
        let node = source.nodes[copy.node].clone();
        if let Some(id) = s.add_copy(node, copy.target.0, &copy.target.1, TimeSpan::from_ms(copy.start_ms), TimeSpan::from_ms(copy.duration_ms)) {
            ids.push(id);
        }
    }
    let mut actions = vec![UndoAction::EffectsAdded { kind: AddRemoveKind::Pasted, ids: ids.clone() }];
    if let Some(tags) = builds::tag_batch(s, a, &ids)? {
        actions.push(tags);
    }
    let step = UndoAction::Group { label: format!("Port Effects{}", if agent { " (Agent)" } else { "" }), actions };
    if agent {
        s.studio_agent_flash.note(&step);
    }
    s.record_undo(step);
    s.render_all_dirty();
    report["effectIds"] = json!(ids.iter().map(|id| id.0).collect::<Vec<_>>());
    Ok(report)
}

#[cfg(test)]
mod tests {
    use crate::app::AppCore;
    use serde_json::{json, Value};

    #[test]
    fn an_old_shows_effects_are_ported_onto_new_props_with_lag_and_snap() {
        let base = std::env::temp_dir().join(format!("studio-port-seq-{}", uuid::Uuid::new_v4()));
        let file = crate::studio_sequence::marks_from_sequence::tests::old_show(&base);
        let core = AppCore::open(base.join("new"), true).unwrap();
        let run = |name: &str, args: Value| crate::studio_sequence::execute(&core, name, &args, false).unwrap();
        let fail = |name: &str, args: Value| crate::studio_sequence::execute(&core, name, &args, false).unwrap_err();
        let display = |name: &str| core.studio_display.execute(&core, "element_create", &json!({"name": name}), false).unwrap()["result"]["ids"][0].clone();
        let (roof, tree) = (display("Roof"), display("Tree"));
        let id = run("sequence_create", json!({"lengthMs": 6200}))["sequenceId"].clone();
        run("marks_create", json!({"sequenceId": id, "name": "Beats", "timesMs": [0, 1000, 2000, 3000, 4000, 5000, 6000]}));
        let effects = || run("sequence_effects", json!({"sequenceId": id}))["effects"].as_array().unwrap().clone();

        // A dry run without a map lists the zones to route and what every start would do.
        let survey = run("effects_port_from_sequence", json!({"sequenceId": id, "path": file, "autoZones": true, "offsetMs": -35, "snapTo": "Beats", "dryRun": true}));
        let zones: Vec<(&str, u64)> = survey["zones"].as_array().unwrap().iter().map(|z| (z["name"].as_str().unwrap(), z["effects"].as_u64().unwrap())).collect();
        assert_eq!(zones, [("Relay", 2), ("Pixel Strip", 2)], "{survey}");
        assert_eq!((survey["effects"].as_u64(), survey["moves"]["events"].as_u64(), survey["moves"]["snapped"].as_u64()), (Some(0), Some(4), Some(4)), "{survey}");
        assert_eq!(survey["unmapped"].as_array().unwrap().len(), 2, "{survey}");

        // The relay zone goes to both new props, the pixel strip to the tree; the effect at 6 s runs past the 6.2 s end and is trimmed.
        let port = json!({"sequenceId": id, "path": file, "autoZones": true, "offsetMs": -35, "snapTo": "Beats", "buildId": "port v1",
                          "map": [{"from": "Relay", "targetIds": [roof, tree]}, {"from": "Pixel Strip", "targetId": tree}]});
        let mut dry = port.clone();
        dry["dryRun"] = json!(true);
        let plan = run("effects_port_from_sequence", dry);
        assert_eq!(plan["effects"], 6, "{plan}");
        assert_eq!(plan["routes"][0]["effects"], 4, "{plan}");
        assert_eq!(plan["skipped"]["trimmed"], 1, "{plan}");
        assert!(effects().is_empty(), "a dry run adds nothing");

        let done = run("effects_port_from_sequence", port);
        assert_eq!(done["effectIds"].as_array().unwrap().len(), 6, "{done}");
        let placed = effects();
        let mut on_tree: Vec<(f64, f64)> = placed
            .iter()
            .filter(|e| e["targets"][0] == tree)
            .map(|e| (e["startMs"].as_f64().unwrap(), e["durationMs"].as_f64().unwrap()))
            .collect();
        on_tree.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        assert_eq!(on_tree, [(1000.0, 500.0), (1000.0, 800.0), (3000.0, 250.0), (6000.0, 200.0)], "lag taken out and snapped to the beats");
        assert_eq!(placed.iter().filter(|e| e["targets"][0] == roof).count(), 2);
        let status = run("sequence_status", json!({"sequenceId": id}));
        assert_eq!(status["undo"][0], "Port Effects", "{status}");
        let edits = run("build_edits", json!({"sequenceId": id}));
        assert!(edits.to_string().contains("port v1"), "the copies are tagged with the build: {edits}");
        run("sequence_undo", json!({"sequenceId": id}));
        assert!(effects().is_empty(), "one Undo takes the whole port back");

        // An element's own route wins over its zone's.
        let own = run("effects_port_from_sequence", json!({"sequenceId": id, "path": file, "autoZones": true, "dryRun": true,
                                                          "map": [{"from": "Relay", "targetId": roof}, {"from": "Relay 1", "targetId": tree}]}));
        assert_eq!(own["routes"][0]["effects"], 0, "{own}");
        assert_eq!(own["routes"][1]["effects"], 2, "{own}");

        assert!(fail("effects_port_from_sequence", json!({"sequenceId": id, "path": file})).contains("map is required"));
        assert!(fail("effects_port_from_sequence", json!({"sequenceId": id, "path": file, "map": [{"from": "Arch", "targetId": roof}]})).contains("zones"));
        assert!(fail("effects_port_from_sequence", json!({"sequenceId": id, "path": file, "map": [{"from": "Relay 1"}]})).contains("Relay 1"));
        core.studio_display.stop();
        drop(core);
        let _ = std::fs::remove_dir_all(&base);
    }
}
