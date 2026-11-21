//! The Mark Collections panel's edits on the shared Undo (R40). Upstream's panel has no undo and commits through OK/Cancel dialogs;
//! here every edit is live and one step on the sequence's Undo, which puts back the whole collection list and the parameters of the
//! effects that read marks (deleting a collection clears their references to it). The panel (`vixen-ffi` `MarksPanel`) and the
//! agent tools (`marks_collection_edit`, `marks_collection_delete`, `marks_import`) run the same `marks` logic through `apply`.
use super::{record, string, uuid};
use crate::{marks as logic, sequence::SequenceSession, undo::UndoAction};
use serde_json::{json, Value};
use std::path::Path;
use uuid::Uuid;
use vixen_core::marks::{MarkCollection, MarkCollectionType};
use vixen_core::Color;
use vixen_io::marks_files::PangolinMode;

/// Undo label for an offset, which the panel's live offset field extends rather than repeats.
pub const OFFSET_LABEL: &str = "Offset Marks";

/// Runs one edit on a copy of the list, hands it to the session as the change requires and, when anything changed, records one undo
/// step named `label`.
pub fn apply(s: &mut SequenceSession, label: &str, agent: bool, f: impl FnOnce(&mut Vec<MarkCollection>) -> logic::MarksChange) -> logic::MarksChange {
    let before = s.marks().to_vec();
    let params = s.studio_mark_reader_params();
    let mut marks = before.clone();
    let change = f(&mut marks);
    if change != logic::MarksChange::None {
        logic::publish(s, marks, change);
        record(s, label, UndoAction::StudioMarkCollections { label: label.into(), marks: before, params }, agent);
    }
    change
}

/// The panel's live offset field: `total_ms` from where the field started. The first change records "Offset Marks" with the list as it
/// was; later ones, while that step is still the newest and nothing else has changed the sequence, apply the new total to that same
/// starting list without another step, so one Undo takes the whole adjustment back. `base` keeps the starting list between calls.
pub fn offset_live(s: &mut SequenceSession, id: Uuid, total_ms: i32, continuing: bool, base: &mut Option<Vec<MarkCollection>>) {
    let newest_is_offset = s.undo_stack().undo_descriptions().first().is_some_and(|d| d == OFFSET_LABEL);
    match base.as_ref().filter(|_| continuing && newest_is_offset) {
        Some(start) => {
            let mut marks = start.clone();
            logic::offset_time(&mut marks, id, total_ms);
            logic::publish(s, marks, logic::MarksChange::Collections);
        }
        None => {
            let start = s.marks().to_vec();
            if apply(s, OFFSET_LABEL, false, |marks| logic::offset_time(marks, id, total_ms)) != logic::MarksChange::None {
                *base = Some(start);
            }
        }
    }
}

/// The undo label of a row checkbox, header toggle or check menu item.
pub fn flag_label(flag: logic::MarkFlag) -> &'static str {
    match flag {
        logic::MarkFlag::Lines => "Mark Lines",
        logic::MarkFlag::TailLines => "Tail Mark Lines",
        logic::MarkFlag::Bar => "Mark Bar",
        logic::MarkFlag::Lock => "Lock Mark Collection",
        logic::MarkFlag::SolidLine => "Solid Mark Line",
        logic::MarkFlag::Bold => "Bold Mark Line",
    }
}

pub fn tools() -> Vec<(&'static str, &'static str, Value, Vec<&'static str>, bool)> {
    let string = json!({"type":"string","minLength":1,"maxLength":256});
    let boolean = json!({"type":"boolean"});
    let kinds = json!({"type":"string","enum":["generic","phrase","word","phoneme","state"]});
    vec![
        ("marks_collection_edit","Change a mark collection as the Mark Collections panel does, all given fields in one undo step: name (unique, ignoring case), active:true (the collection new marks go to), showLines, showTailLines, showBar, locked, solidLine, bold, color (#RRGGBB), weight (1-6), type, linkedTo (a phrase collection for a word collection, a word collection for a phoneme one; \"\" unlinks), offsetMs (moves every mark's start, -30000 to 30000). Returns changed and the collection.",json!({"sequenceId":string,"collectionId":string,"name":string,"active":{"const":true},"showLines":boolean,"showTailLines":boolean,"showBar":boolean,"locked":boolean,"solidLine":boolean,"bold":boolean,"color":string,"weight":{"type":"integer","minimum":1,"maximum":6},"type":kinds,"linkedTo":{"type":"string","maxLength":256},"offsetMs":{"type":"integer","minimum":-30000,"maximum":30000}}),vec!["sequenceId","collectionId"],false),
        ("marks_collection_delete","Delete a mark collection and its marks, as the panel's Delete does, in one undo step (Undo also restores the effects that referred to it). A deleted active collection passes to the first visible one. Requires confirm:true.",json!({"sequenceId":string,"collectionId":string,"confirm":boolean}),vec!["sequenceId","collectionId","confirm"],false),
        ("marks_import","Import mark collections from a file, as the panel's Import menu does, in one undo step. kind: vampBars, vampBeats, audacityBeats, vixen3 (.v3m), xTiming (.xtiming), papagayo (.pgo) or pangolinBeyond. path is an absolute path to the file (read only). collections picks which of the file's collections to add by name (default all); names already in the sequence get a number. pangolinBeyond makes one collection per colour unless groupByColor:false, then one collection in color (#RRGGBB, default white). Returns the added collections.",json!({"sequenceId":string,"kind":{"type":"string","enum":["vampBars","vampBeats","audacityBeats","vixen3","xTiming","papagayo","pangolinBeyond"]},"path":{"type":"string","minLength":1,"maxLength":1024},"collections":{"type":"array","items":string,"minItems":1,"maxItems":1000},"groupByColor":boolean,"color":string}),vec!["sequenceId","kind","path"],false),
    ]
}

pub fn execute(s: &mut SequenceSession, name: &str, a: &Value, agent: bool) -> Result<Value, String> {
    match name {
        "marks_collection_edit" => edit(s, a, agent),
        "marks_collection_delete" => {
            if a["confirm"] != true {
                return Err("Deleting a mark collection requires confirm:true".into());
            }
            let id = existing(s, a)?;
            apply(s, "Delete Mark Collection", agent, |marks| logic::delete_collection(marks, id));
            Ok(json!({"deleted": id.to_string()}))
        }
        "marks_import" => import(s, a, agent),
        _ => Err("Unknown marks command".into()),
    }
}

fn existing(s: &SequenceSession, a: &Value) -> Result<Uuid, String> {
    let id = uuid(a, "collectionId")?;
    s.marks().iter().any(|c| c.id == id).then_some(id).ok_or_else(|| format!("Mark collection {id} not found"))
}

fn hex(a: &Value, key: &str) -> Result<Option<Color>, String> {
    let Some(text) = a.get(key) else { return Ok(None) };
    let digits = text.as_str().and_then(|t| t.strip_prefix('#')).filter(|d| d.len() == 6).ok_or(format!("{key} must be #RRGGBB"))?;
    let rgb = u32::from_str_radix(digits, 16).map_err(|_| format!("{key} must be #RRGGBB"))?;
    Ok(Some(Color::from_argb_u32(0xFF00_0000 | rgb)))
}

fn kind(text: &str) -> MarkCollectionType {
    match text {
        "phrase" => MarkCollectionType::Phrase,
        "word" => MarkCollectionType::Word,
        "phoneme" => MarkCollectionType::Phoneme,
        "state" => MarkCollectionType::State,
        _ => MarkCollectionType::Generic,
    }
}

fn edit(s: &mut SequenceSession, a: &Value, agent: bool) -> Result<Value, String> {
    let id = existing(s, a)?;
    let color = hex(a, "color")?;
    if a.get("name").is_some() {
        let name = string(a, "name").map_err(|_| "name must be a nonempty string".to_string())?;
        if let Some(error) = logic::rename_error(s.marks(), id, name).or_else(|| logic::normalize_name(name).is_empty().then(|| "name must not be blank".into())) {
            return Err(error);
        }
    }
    let link = match a.get("linkedTo").and_then(Value::as_str) {
        None => None,
        Some("") => Some(None),
        Some(text) => {
            let parent = Uuid::parse_str(text).map_err(|_| "linkedTo must be a collection id or \"\"".to_string())?;
            Some(Some(parent))
        }
    };
    let flags = [
        ("showLines", logic::MarkFlag::Lines),
        ("showTailLines", logic::MarkFlag::TailLines),
        ("showBar", logic::MarkFlag::Bar),
        ("locked", logic::MarkFlag::Lock),
        ("solidLine", logic::MarkFlag::SolidLine),
        ("bold", logic::MarkFlag::Bold),
    ];
    let mut error = None;
    let change = apply(s, "Edit Mark Collection", agent, |marks| {
        let mut change = logic::MarksChange::None;
        let mut note = |c: logic::MarksChange| {
            if c == logic::MarksChange::Collections || change == logic::MarksChange::None {
                change = c;
            }
        };
        if a.get("name").is_some() && logic::end_rename(marks, id, a["name"].as_str().unwrap_or_default()) == logic::RenameEnd::Renamed {
            note(logic::MarksChange::Properties);
        }
        if a["active"] == true {
            note(logic::set_active(marks, id));
        }
        for (key, flag) in flags {
            if let Some(on) = a[key].as_bool() {
                note(logic::set_flag(marks, id, flag, on));
            }
        }
        if let Some(color) = color {
            note(logic::set_color(marks, id, color));
        }
        if let Some(level) = a["weight"].as_i64() {
            note(logic::set_level(marks, id, level as i32));
        }
        if let Some(t) = a["type"].as_str() {
            note(logic::set_kind(marks, id, kind(t)));
        }
        if let Some(parent) = link {
            // Checked after the type, so one call can make a collection a word collection and link it.
            match logic::link_choices(marks, id) {
                Some(choices) if choices.iter().any(|c| c.id == parent) => note(logic::set_link(marks, id, parent)),
                Some(_) => error = Some("linkedTo must be a collection of the type this one links to (word to phrase, phoneme to word)"),
                None => error = Some("Only word and phoneme collections link to another collection"),
            }
        }
        if let Some(ms) = a["offsetMs"].as_i64() {
            note(logic::offset_time(marks, id, ms as i32));
        }
        if error.is_some() {
            return logic::MarksChange::None;
        }
        change
    });
    if let Some(error) = error {
        return Err(error.into());
    }
    let row = logic::rows(s.marks()).into_iter().find(|r| r.id == id).ok_or("Mark collection vanished")?;
    Ok(json!({"changed": change != logic::MarksChange::None, "collection": row_json(&row)}))
}

fn row_json(r: &logic::MarkRow) -> Value {
    json!({"id": r.id.to_string(), "name": r.name, "color": format!("#{:06X}", r.color.to_argb_u32() & 0xFF_FFFF), "active": r.is_default,
           "showLines": r.show_lines, "showTailLines": r.show_tail, "showBar": r.show_bar, "locked": r.locked, "solidLine": r.solid,
           "bold": r.bold, "weight": r.level, "linkedTo": r.linked_to.map(|id| id.to_string()), "marks": r.mark_count})
}

fn import(s: &mut SequenceSession, a: &Value, agent: bool) -> Result<Value, String> {
    let import_kind = match a["kind"].as_str().unwrap_or_default() {
        "vampBars" => logic::ImportKind::VampBars,
        "vampBeats" => logic::ImportKind::VampBeats,
        "audacityBeats" => logic::ImportKind::AudacityBeats,
        "vixen3" => logic::ImportKind::Vixen3,
        "xTiming" => logic::ImportKind::XTiming,
        "papagayo" => logic::ImportKind::Papagayo,
        _ => logic::ImportKind::PangolinBeyond,
    };
    let path = Path::new(a["path"].as_str().unwrap_or_default());
    if !path.is_absolute() {
        return Err("path must be absolute".into());
    }
    let size = std::fs::metadata(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?.len();
    if size > 64 * 1024 * 1024 {
        return Err("Import files are limited to 64 MiB".into());
    }
    let bytes = std::fs::read(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let color = hex(a, "color")?.unwrap_or(Color::from_argb_u32(0xFFFF_FFFF));
    let candidates = match logic::read_candidates(import_kind, &bytes, &stem) {
        logic::ImportRead::Candidates(c) | logic::ImportRead::CandidatesAfterMessage { candidates: c, .. } => c,
        logic::ImportRead::PangolinChoice(records) => {
            let mode = if a["groupByColor"] == false { PangolinMode::Single(color) } else { PangolinMode::GroupByColor };
            logic::pangolin_candidates(&records, mode)
        }
        logic::ImportRead::Failed { title, message, .. } => return Err(format!("{title}: {message}")),
        logic::ImportRead::NotBuilt(message) => return Err(message),
    };
    let chosen: Vec<MarkCollection> = match a.get("collections").and_then(Value::as_array) {
        None => candidates,
        Some(names) => {
            let names: Vec<&str> = names.iter().filter_map(Value::as_str).collect();
            if let Some(missing) = names.iter().find(|n| !candidates.iter().any(|c| c.name == **n)) {
                let offered: Vec<&str> = candidates.iter().map(|c| c.name.as_str()).collect();
                return Err(format!("The file has no collection named \"{missing}\"; it has {offered:?}"));
            }
            candidates.into_iter().filter(|c| names.contains(&c.name.as_str())).collect()
        }
    };
    let ids: Vec<Uuid> = chosen.iter().map(|c| c.id).collect();
    apply(s, "Import Marks", agent, |marks| if logic::commit_import(marks, chosen) { logic::MarksChange::Collections } else { logic::MarksChange::None });
    let added: Vec<Value> = logic::rows(s.marks()).iter().filter(|r| ids.contains(&r.id)).map(row_json).collect();
    Ok(json!({"added": added}))
}

#[cfg(test)]
mod tests {
    use crate::app::AppCore;
    use serde_json::{json, Value};

    fn names(o: &Value) -> Vec<String> {
        o.as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap().to_string()).collect()
    }

    #[test]
    fn panel_edits_are_one_undo_step_each_and_delete_comes_back_whole() {
        let base = std::env::temp_dir().join(format!("studio-marks-panel-{}", uuid::Uuid::new_v4()));
        let core = AppCore::open(base.join("profile"), true).unwrap();
        let run = |name: &str, args: Value| crate::studio_sequence::execute(&core, name, &args, false).unwrap();
        let fail = |name: &str, args: Value| crate::studio_sequence::execute(&core, name, &args, false).unwrap_err();
        let id = run("sequence_create", json!({"lengthMs": 30000}))["sequenceId"].clone();
        let beats = run("marks_create", json!({"sequenceId": id, "name": "Beats", "timesMs": [1000, 2000]}))["collectionId"].clone();
        run("marks_create", json!({"sequenceId": id, "name": "Bars", "timesMs": [1000]}));
        let marks = || run("sequence_marks", json!({"sequenceId": id}));
        let undo = || run("sequence_status", json!({"sequenceId": id}))["undo"].clone();

        let edited = run("marks_collection_edit", json!({"sequenceId": id, "collectionId": beats, "name": "Kicks", "color": "#FF8000", "weight": 3, "offsetMs": 500}));
        assert_eq!(edited["changed"], true);
        assert_eq!((edited["collection"]["color"].as_str(), edited["collection"]["weight"].as_i64()), (Some("#FF8000"), Some(3)));
        assert_eq!(undo()[0], "Edit Mark Collection");
        assert_eq!(marks()[0]["marks"][0]["timeMs"], 1500.0);
        assert!(fail("marks_collection_edit", json!({"sequenceId": id, "collectionId": beats, "name": "bars"})).contains("already exists"));
        run("sequence_undo", json!({"sequenceId": id}));
        assert_eq!(names(&marks()), ["Beats", "Bars"], "one Undo takes back the name, colour, weight and offset together");
        assert_eq!(marks()[0]["marks"][0]["timeMs"], 1000.0);

        let active = run("marks_collection_edit", json!({"sequenceId": id, "collectionId": beats, "active": true, "type": "word"}));
        assert_eq!((active["collection"]["active"].as_bool(), active["changed"].as_bool()), (Some(true), Some(true)), "{active}");
        assert!(fail("marks_collection_edit", json!({"sequenceId": id, "collectionId": beats, "linkedTo": beats})).contains("linkedTo"));
        assert!(fail("marks_collection_delete", json!({"sequenceId": id, "collectionId": beats, "confirm": false})).contains("confirm"));
        run("marks_collection_delete", json!({"sequenceId": id, "collectionId": beats, "confirm": true}));
        assert_eq!(names(&marks()), ["Bars"]);
        assert_eq!(undo()[0], "Delete Mark Collection");
        run("sequence_undo", json!({"sequenceId": id}));
        assert_eq!(names(&marks()), ["Beats", "Bars"], "Undo brings the collection back with its marks");
        assert_eq!(marks()[0]["marks"].as_array().unwrap().len(), 2);
        run("sequence_redo", json!({"sequenceId": id}));
        assert_eq!(names(&marks()), ["Bars"]);

        // An Audacity label file imports as one collection named after the file, renamed when the name is taken.
        let file = base.join("Bars.txt");
        std::fs::write(&file, "1.000000\t1.000000\t1\n2.500000\t2.500000\t2\n").unwrap();
        let added = run("marks_import", json!({"sequenceId": id, "kind": "audacityBeats", "path": file.to_string_lossy()}));
        assert_eq!(added["added"].as_array().unwrap().len(), 1, "{added}");
        assert_ne!(added["added"][0]["name"], "Bars", "{added}");
        assert_eq!(undo()[0], "Import Marks");
        run("sequence_undo", json!({"sequenceId": id}));
        assert_eq!(names(&marks()), ["Bars"]);
        assert!(fail("marks_import", json!({"sequenceId": id, "kind": "audacityBeats", "path": "Bars.txt"})).contains("absolute"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn undoing_a_delete_restores_the_effects_that_read_it() {
        let base = std::env::temp_dir().join(format!("studio-marks-refs-{}", uuid::Uuid::new_v4()));
        let core = AppCore::open(base.join("profile"), true).unwrap();
        let run = |name: &str, args: Value| crate::studio_sequence::execute(&core, name, &args, false).unwrap();
        let element = core.studio_display.execute(&core, "element_create", &json!({"name": "Tree"}), false).unwrap()["result"]["ids"][0].clone();
        let id = run("sequence_create", json!({"lengthMs": 30000}))["sequenceId"].clone();
        run("marks_create", json!({"sequenceId": id, "name": "Bars", "timesMs": [1000]}));
        let beats = run("marks_create", json!({"sequenceId": id, "name": "Beats", "timesMs": [1000, 2000]}))["collectionId"].clone();
        let shapes = vixen_effect_registry::all().iter().find(|d| d.name == "Shapes").unwrap().type_id.to_string();
        let effect = run("effect_add", json!({"sequenceId": id, "typeId": shapes, "targetId": element, "startMs": 0, "durationMs": 5000,
                                              "params": {"ShapeMode": "CreateShapesMarkCollection", "MarkCollectionId": beats}}))["effectId"].clone();
        let reads = || {
            let p = run("effect_parameters", json!({"sequenceId": id, "effectId": effect}));
            p["parameters"].as_array().unwrap().iter().find(|v| v["name"] == "MarkCollectionId").unwrap()["value"].clone()
        };
        assert_eq!(reads(), beats);
        run("marks_collection_delete", json!({"sequenceId": id, "collectionId": beats, "confirm": true}));
        assert_ne!(reads(), beats, "the delete moves the effect to another collection");
        run("sequence_undo", json!({"sequenceId": id}));
        assert_eq!(reads(), beats, "Undo points the effect at the restored collection again");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_live_offset_is_one_undo_step() {
        use crate::marks as logic;
        let base = std::env::temp_dir().join(format!("studio-marks-offset-{}", uuid::Uuid::new_v4()));
        let core = AppCore::open(base.join("profile"), true).unwrap();
        let run = |name: &str, args: Value| crate::studio_sequence::execute(&core, name, &args, false).unwrap();
        let id = run("sequence_create", json!({"lengthMs": 30000}))["sequenceId"].clone();
        let beats = run("marks_create", json!({"sequenceId": id, "name": "Beats", "timesMs": [1000, 2000]}))["collectionId"].clone();
        let shared = crate::studio_sequence::find(&core, uuid::Uuid::parse_str(id.as_str().unwrap()).unwrap()).unwrap();
        let mut s = shared.lock();
        let collection = uuid::Uuid::parse_str(beats.as_str().unwrap()).unwrap();
        let mut start = None;
        for (total, continuing) in [(100, false), (250, true), (-40, true)] {
            super::offset_live(&mut s, collection, total, continuing, &mut start);
        }
        assert_eq!(s.marks()[0].marks[0].start_time.total_ms(), 960.0, "the last total applies to the starting marks");
        let steps = s.undo_stack().undo_descriptions();
        assert_eq!(steps.iter().filter(|d| *d == super::OFFSET_LABEL).count(), 1, "{steps:?}");
        s.undo(1);
        assert_eq!(s.marks()[0].marks[0].start_time.total_ms(), 1000.0);
        // A fresh field after something else changed records its own step.
        super::apply(&mut s, "Mark Bar", false, |m| logic::set_flag(m, collection, logic::MarkFlag::Bar, true));
        super::offset_live(&mut s, collection, 10, true, &mut start);
        assert_eq!(s.undo_stack().undo_descriptions()[0], super::OFFSET_LABEL);
        assert_eq!(s.marks()[0].marks[0].start_time.total_ms(), 1010.0);
        drop(s);
        let _ = std::fs::remove_dir_all(&base);
    }
}
