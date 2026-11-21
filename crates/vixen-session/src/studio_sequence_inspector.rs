//! The sequencer's inspector when no effect is selected: the sequence itself. `sequence_overview` reads its length,
//! song, mark collections, layers and render state in one call, for the inspector and for agents alike; the edits it
//! offers are commands with shared Undo (`sequence_length`, `layer_create`, `layer_rename`, and `marks_visibility`
//! here).
use super::{record, uuid, window_chrome};
use crate::{marks as logic, sequence::SequenceSession, undo::UndoAction};
use serde_json::{json, Value};
use std::{collections::HashMap, path::Path};
use uuid::Uuid;
use vixen_core::marks::MarkCollectionType;

pub fn tools() -> Vec<(&'static str, &'static str, Value, Vec<&'static str>, bool)> {
    let string = json!({"type":"string","minLength":1,"maxLength":256});
    vec![
        ("sequence_overview","What the sequencer's inspector shows when no effect is selected: lengthMs; audio {song, title, missing, durationMs} (null with no audio; durationMs null when Media lacks it); marks, each collection's id, name, type, marks (count) and visible (its lines or bar show on the timeline); layers in list order with id, name, filter, isDefault and effects (count); render {effects, rendered, rendering, percent, help} as the toolbar's ring shows it. Read-only.",json!({"sequenceId":string}),vec!["sequenceId"],true),
        ("marks_visibility","Show or hide a mark collection on the timeline (its lines and bar together), as the inspector's toggle does, in one undo step. collectionIds takes several at once.",json!({"sequenceId":string,"collectionId":string,"collectionIds":{"type":"array","items":string,"minItems":1,"maxItems":1000},"visible":{"type":"boolean"}}),vec!["sequenceId","visible"],false),
    ]
}

pub fn overview(s: &SequenceSession, media_dir: &Path) -> Value {
    let chrome = window_chrome::window_chrome(s, media_dir);
    let audio = chrome.song.as_ref().map(|song| {
        let duration = if chrome.song_missing { None } else { s.audio().map(|c| c.duration().total_ms()) };
        json!({"song": song, "title": song_title(&chrome.subtitle, chrome.song_missing), "missing": chrome.song_missing, "durationMs": duration})
    });
    let marks: Vec<Value> = s
        .marks()
        .iter()
        .map(|c| {
            json!({"id": c.id.to_string(), "name": c.name, "type": type_name(c.collection_type), "marks": c.marks.len(),
                   "visible": logic::is_visible(c)})
        })
        .collect();
    let mut per_layer: HashMap<Uuid, usize> = HashMap::new();
    for e in s.effects() {
        if let Some(layer) = s.layer_of(e.id) {
            *per_layer.entry(layer).or_default() += 1;
        }
    }
    let layers = &s.document().layers;
    let default = layers.default_layer_id();
    let layers: Vec<Value> = layers
        .layers
        .iter()
        .map(|l| {
            json!({"id": l.id.to_string(), "name": l.name,
                   "filter": l.filter.as_ref().map(crate::sequencer::layers::filter_type_name).unwrap_or_else(|| "Highest Value (Default)".into()),
                   "isDefault": l.id == default, "effects": per_layer.get(&l.id).copied().unwrap_or(0)})
        })
        .collect();
    json!({
        "lengthMs": s.length().total_ms(),
        "audio": audio,
        "marks": marks,
        "layers": layers,
        "render": {"effects": chrome.effects, "rendered": chrome.rendered, "rendering": chrome.rendering,
                   "percent": chrome.rendering_percent, "help": chrome.render_help},
    })
}

/// The song without folders or extension: the window's subtitle, less its "(audio missing)".
fn song_title(subtitle: &str, missing: bool) -> String {
    if missing { subtitle.trim_end_matches(" (audio missing)").to_string() } else { subtitle.to_string() }
}

fn type_name(t: MarkCollectionType) -> &'static str {
    match t {
        MarkCollectionType::Generic => "generic",
        MarkCollectionType::Phrase => "phrase",
        MarkCollectionType::Word => "word",
        MarkCollectionType::Phoneme => "phoneme",
        MarkCollectionType::State => "state",
    }
}

/// The inspector's length field: `m:ss.fff`, `m:ss`, `h:mm:ss` or plain seconds (`166.5`), up to an hour.
pub fn parse_length(text: &str) -> Option<f64> {
    let mut seconds = 0.0;
    let parts: Vec<&str> = text.trim().split(':').collect();
    if parts.len() > 3 {
        return None;
    }
    for (i, part) in parts.iter().enumerate() {
        let last = i + 1 == parts.len();
        let valid = !part.is_empty() && part.chars().all(|c| c.is_ascii_digit() || (last && c == '.'));
        let value: f64 = part.parse().ok().filter(|_| valid)?;
        if i > 0 && value >= 60.0 {
            return None;
        }
        seconds = seconds * 60.0 + value;
    }
    let ms = (seconds * 1000.0).round();
    (ms.is_finite() && ms <= 3_600_000.0).then_some(ms)
}

/// `marks_visibility`: each collection's lines and bar both on, or both off. Undo puts back each one's own lines and
/// bar, so a collection that showed only its lines shows only its lines again.
pub fn set_visibility(s: &mut SequenceSession, a: &Value, agent: bool) -> Result<Value, String> {
    let visible = a["visible"].as_bool().ok_or("visible must be true or false")?;
    let mut ids = Vec::new();
    if a.get("collectionId").is_some() {
        ids.push(uuid(a, "collectionId")?);
    }
    if let Some(list) = a.get("collectionIds") {
        let list = list.as_array().filter(|l| !l.is_empty() && l.len() <= 1000).ok_or("collectionIds must list 1 to 1000 ids")?;
        for v in list {
            ids.push(Uuid::parse_str(v.as_str().unwrap_or_default()).map_err(|_| "collectionIds must be UUIDs".to_string())?);
        }
    }
    if ids.is_empty() {
        return Err("Give collectionId or collectionIds".into());
    }
    let mut marks = s.marks().to_vec();
    let mut before = Vec::new();
    for id in &ids {
        let c = marks.iter().find(|c| c.id == *id).ok_or_else(|| format!("Mark collection {id} not found"))?;
        if logic::is_visible(c) != visible && !before.iter().any(|(b, _, _)| b == id) {
            before.push((*id, c.show_grid_lines, c.show_marks));
        }
    }
    for (id, _, _) in &before {
        logic::set_flag(&mut marks, *id, logic::MarkFlag::Lines, visible);
        logic::set_flag(&mut marks, *id, logic::MarkFlag::Bar, visible);
    }
    if !before.is_empty() {
        let label = match (visible, before.len()) {
            (true, 1) => "Show Mark Collection",
            (true, _) => "Show Mark Collections",
            (false, 1) => "Hide Mark Collection",
            (false, _) => "Hide Mark Collections",
        };
        logic::publish(s, marks, logic::MarksChange::Properties);
        record(s, label, UndoAction::StudioMarkVisibility { label: label.into(), flags: before.clone() }, agent);
    }
    Ok(json!({"changed": before.len()}))
}

#[cfg(test)]
mod tests {
    use crate::app::AppCore;
    use serde_json::{json, Value};

    #[test]
    fn length_text_takes_minutes_and_seconds() {
        use super::parse_length;
        assert_eq!(parse_length("2:46.500"), Some(166_500.0));
        assert_eq!(parse_length("2:46"), Some(166_000.0));
        assert_eq!(parse_length(" 166.5 "), Some(166_500.0));
        assert_eq!(parse_length("1:02:03"), None, "over an hour");
        assert_eq!(parse_length("0:59:59.5"), Some(3_599_500.0));
        for bad in ["", "2:75", "a:10", "1.5:10", "1::2", "-3"] {
            assert_eq!(parse_length(bad), None, "{bad}");
        }
    }

    #[test]
    fn overview_reports_the_sequence_and_visibility_undoes() {
        let base = std::env::temp_dir().join(format!("studio-inspector-{}", uuid::Uuid::new_v4()));
        let core = AppCore::open(base.join("profile"), true).unwrap();
        let run = |name: &str, args: Value| crate::studio_sequence::execute(&core, name, &args, false).unwrap();
        let id = run("sequence_create", json!({"lengthMs": 30000}))["sequenceId"].clone();
        let beats = run("marks_create", json!({"sequenceId": id, "name": "Beats", "timesMs": [1000, 2000, 3000]}))["collectionId"].clone();
        run("layer_create", json!({"sequenceId": id, "name": "Accents"}));
        let overview = || run("sequence_overview", json!({"sequenceId": id}));

        run("sequence_length", json!({"sequenceId": id, "length": "0:45.250"}));
        let o = overview();
        assert_eq!(o["lengthMs"], 45250.0);
        assert!(o["audio"].is_null(), "{o}");
        let beat = o["marks"].as_array().unwrap().iter().find(|c| c["id"] == beats).unwrap().clone();
        assert_eq!((beat["name"].as_str(), beat["marks"].as_u64(), beat["type"].as_str()), (Some("Beats"), Some(3), Some("generic")));
        assert_eq!(beat["visible"], true, "a new collection shows its lines");
        let layers = o["layers"].as_array().unwrap();
        assert_eq!(layers.len(), 2);
        assert!(layers.iter().any(|l| l["name"] == "Accents" && l["isDefault"] == false && l["effects"] == 0));
        assert!(layers.iter().any(|l| l["isDefault"] == true));
        assert_eq!((o["render"]["effects"].as_u64(), o["render"]["percent"].as_u64()), (Some(0), Some(100)));

        let visible = |o: &Value| o["marks"].as_array().unwrap().iter().find(|c| c["id"] == beats).unwrap()["visible"].clone();
        // Hiding is one undo step; hiding again changes nothing and records nothing.
        assert_eq!(run("marks_visibility", json!({"sequenceId": id, "collectionId": beats, "visible": false}))["changed"], 1);
        assert_eq!(visible(&overview()), false);
        assert_eq!(run("marks_visibility", json!({"sequenceId": id, "collectionIds": [beats], "visible": false}))["changed"], 0);
        let undo = run("sequence_status", json!({"sequenceId": id}))["undo"].clone();
        assert_eq!(undo[0], "Hide Mark Collection", "{undo}");
        run("sequence_undo", json!({"sequenceId": id}));
        assert_eq!(visible(&overview()), true, "Undo shows it again");
        // Undo puts back its own lines-only setting, not the bar the toggle turned on.
        run("sequence_redo", json!({"sequenceId": id}));
        run("marks_visibility", json!({"sequenceId": id, "collectionId": beats, "visible": true}));
        run("sequence_undo", json!({"sequenceId": id}));
        assert_eq!(visible(&overview()), false);
        run("sequence_undo", json!({"sequenceId": id}));
        assert_eq!(visible(&overview()), true);
        assert!(crate::studio_sequence::execute(&core, "marks_visibility", &json!({"sequenceId": id, "visible": true}), false).is_err());
        let _ = std::fs::remove_dir_all(&base);
    }
}
