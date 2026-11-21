//! Tools ▸ LipSync ▸ Edit Element Face Mapping as live edits on the shared undo history (`studio_display`, rest of R40). Face
//! Setup run from it writes the elements' Face properties as it's used; the whole visit is one step ("Edit Face Mapping"), and agents
//! set the same properties with `face_mapping_set`, one step per call. `face_mapping_list` (studio-mcp) reads them.
//!
//! The Face property only tells LipSync effects which mouth, outline and eye shapes an element draws, so a step publishes without
//! rebuilding the output graph (open to agents while Live Output is on): the system is replaced, the open sequences' LipSync effects
//! render again against it and the configuration is saved, as upstream's Face Setup OK does (F111, `face_mapping_applied`).
use crate::app::AppCore;
use crate::dialogs::face::{face_helper_apply, face_helper_model, FaceRow, COMPONENTS, PHONEMES};
use crate::dialogs::world::{DialogWorld, SystemWorld};
use serde_json::{json, Value};
use uuid::Uuid;
use vixen_core::element::property_ids;
use vixen_core::Color;
use vixen_io::SystemData;

/// The commands that edit face mapping on the shared history.
pub const COMMANDS: [&str; 1] = ["face_mapping_set"];

/// The history's label for Face Setup's visit and for the command, in the words Edit ▸ Undo shows.
pub const LABEL: &str = "Edit Face Mapping";

/// The most elements one call or list covers.
const NODE_LIMIT: usize = 2000;

pub fn is_command(command: &str) -> bool {
    COMMANDS.contains(&command)
}

/// Puts a step's system in place: LipSync effects in the open sequences render again against it, and the configuration is saved.
pub fn publish(core: &AppCore, system: SystemData) {
    *core.system.write() = system;
    core.display.studio_bump();
    core.face_mapping_applied();
}

/// What the elements' Face properties hold, to tell whether a visit changed anything: one entry per node, `None` without one.
pub fn fingerprint(system: &SystemData, nodes: &[Uuid]) -> Vec<Option<String>> {
    nodes
        .iter()
        .map(|id| {
            let node = system.tree.node(system.tree.find(id)?);
            node.properties.iter().find(|p| p.type_id == property_ids::FACE).map(|p| format!("{:?}", p.data))
        })
        .collect()
}

fn hex(color: Color) -> String {
    format!("#{:06X}", color.to_argb_u32() & 0x00FF_FFFF)
}

fn parse_hex(text: &str) -> Result<Color, String> {
    let digits = text.strip_prefix('#').filter(|d| d.len() == 6).ok_or("color must be #RRGGBB")?;
    let value = u32::from_str_radix(digits, 16).map_err(|_| "color must be #RRGGBB")?;
    Ok(Color::from_argb_u32(0xFF00_0000 | value))
}

fn describe_row(row: &FaceRow) -> Value {
    json!({
        "id": row.node.to_string(),
        "name": row.name,
        "phonemes": PHONEMES.iter().zip(&row.phonemes).filter(|(_, on)| **on).map(|(p, _)| *p).collect::<Vec<_>>(),
        "components": COMPONENTS.iter().zip(&row.components).filter(|(_, on)| **on).map(|((key, _), _)| *key).collect::<Vec<_>>(),
        "color": hex(row.color),
    })
}

/// Every element with a Face property, as Face Setup shows it: `{revision, phonemes, components, elements: [{id, name, phonemes,
/// components, color}]}`, the phonemes and components each element draws.
pub fn describe(system: &SystemData, revision: u64) -> Value {
    let ids: Vec<Uuid> = system
        .tree
        .all_nodes()
        .filter(|(_, node)| node.properties.iter().any(|p| p.type_id == property_ids::FACE))
        .map(|(_, node)| node.id)
        .take(NODE_LIMIT + 1)
        .collect();
    let truncated = ids.len() > NODE_LIMIT;
    let mut system = system.clone();
    let world = SystemWorld::new(&mut system);
    let nodes: Vec<_> = ids.iter().take(NODE_LIMIT).filter_map(|id| world.find_node(id)).collect();
    let rows = face_helper_model(&world, &nodes);
    json!({
        "revision": revision,
        "phonemes": PHONEMES,
        "components": COMPONENTS.iter().map(|(key, _)| *key).collect::<Vec<_>>(),
        "elements": rows.iter().map(describe_row).collect::<Vec<_>>(),
        "truncated": truncated,
    })
}

fn names(args: &Value, key: &str, allowed: &[&str]) -> Result<Option<Vec<bool>>, String> {
    let Some(list) = args.get(key) else { return Ok(None) };
    let list = list.as_array().ok_or(format!("{key} must be a list"))?;
    let mut on = vec![false; allowed.len()];
    for item in list {
        let text = item.as_str().unwrap_or_default();
        let index = allowed
            .iter()
            .position(|a| *a == text)
            .ok_or(format!("{key} takes {}; not {text:?}", allowed.join(", ")))?;
        on[index] = true;
    }
    Ok(Some(on))
}

/// `face_mapping_set` on `system`: the result, and whether anything changed. Each element keeps what the call doesn't give.
pub fn edit(system: &mut SystemData, command: &str, args: &Value) -> Result<(Value, bool), String> {
    if command != "face_mapping_set" {
        return Err("Unknown face mapping command".into());
    }
    let ids = args["nodeIds"].as_array().ok_or("nodeIds must be a list")?;
    let ids: Vec<Uuid> = ids
        .iter()
        .map(|v| v.as_str().and_then(|s| Uuid::parse_str(s).ok()).ok_or("nodeIds must be UUIDs".to_string()))
        .collect::<Result<_, _>>()?;
    let phonemes = names(args, "phonemes", &PHONEMES)?;
    let component_keys: Vec<&str> = COMPONENTS.iter().map(|(key, _)| *key).collect();
    let components = names(args, "components", &component_keys)?;
    let color = args.get("color").map(|c| parse_hex(c.as_str().unwrap_or_default())).transpose()?;
    let before = fingerprint(system, &ids);
    let mut world = SystemWorld::new(system);
    let mut nodes = Vec::new();
    for id in &ids {
        nodes.push(world.find_node(id).ok_or(format!("Element {id} does not exist"))?);
    }
    let rows: Vec<FaceRow> = face_helper_model(&world, &nodes)
        .into_iter()
        .map(|mut row| {
            if let Some(on) = &phonemes {
                row.phonemes = on.clone();
            }
            if let Some(on) = &components {
                row.components = on.clone();
            }
            if let Some(color) = color {
                row.color = color;
            }
            row
        })
        .collect();
    face_helper_apply(&mut world, &rows);
    let after = face_helper_model(&world, &nodes);
    drop(world);
    let changed = fingerprint(system, &ids) != before;
    Ok((json!({"elements": after.iter().map(describe_row).collect::<Vec<_>>()}), changed))
}

pub fn tools() -> Vec<Value> {
    let phoneme = json!({"type":"string","enum":PHONEMES});
    let component = json!({"type":"string","enum":COMPONENTS.iter().map(|(key, _)| *key).collect::<Vec<_>>()});
    vec![json!({
        "name": "face_mapping_set",
        "description": "Set elements' Face properties, as Tools > LipSync > Edit Element Face Mapping's Face Setup does: phonemes lists the mouth shapes each element draws (AI, E, ETC, FV, L, MBP, O, REST, U, WQ), components the Outlines, EyesOpen and EyesClosed it draws, color its #RRGGBB (one the element's Color property doesn't allow becomes its first allowed colour). An element missing a Face property gets one; what a call leaves out stays as it is. LipSync effects in open sequences render again. One undo step on the shared history; works while Live Output is on.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "nodeIds": {"type":"array","items":{"type":"string","minLength":1,"maxLength":64},"minItems":1,"maxItems":NODE_LIMIT},
                "phonemes": {"type":"array","items":phoneme,"maxItems":PHONEMES.len()},
                "components": {"type":"array","items":component,"maxItems":COMPONENTS.len()},
                "color": {"type":"string","minLength":7,"maxLength":7},
                "expectedRevision": {"type":"integer","minimum":0},
            },
            "required": ["nodeIds"],
            "additionalProperties": false,
        },
        "annotations": {"readOnlyHint":false,"destructiveHint":false,"openWorldHint":false},
    })]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialogs::world::{CoreWorld, WorldProvider};
    use std::sync::Arc;

    fn exec(core: &AppCore, command: &str, args: Value) -> Value {
        core.studio_display.execute(core, command, &args, false).unwrap_or_else(|e| panic!("{command}: {e}"))
    }

    fn open() -> (std::path::PathBuf, Arc<AppCore>, Uuid) {
        let root = std::env::temp_dir().join(format!("studio-face-{}", Uuid::new_v4().simple()));
        let core = Arc::new(AppCore::open(root.clone(), true).unwrap());
        let mouth = {
            let mut system = core.system.write();
            let node = system.tree.add_element("Mouth");
            system.tree.node(node).id
        };
        (root, core, mouth)
    }

    fn listed(core: &AppCore) -> Vec<Value> {
        describe(&core.system.read(), 0)["elements"].as_array().unwrap().clone()
    }

    #[test]
    fn the_command_is_one_step_that_undoes_and_redoes() {
        let (root, core, mouth) = open();
        assert!(listed(&core).is_empty());
        let reply = exec(&core, "face_mapping_set", json!({"nodeIds":[mouth.to_string()],"phonemes":["AI","O"],"color":"#FF0000"}));
        assert_eq!(reply["changed"], true);
        let elements = listed(&core);
        assert_eq!(elements.len(), 1);
        assert_eq!(elements[0]["phonemes"], json!(["AI", "O"]));
        assert_eq!(elements[0]["components"], json!([]));
        assert_eq!(elements[0]["color"], "#FF0000");
        assert_eq!(core.studio_display.history()["undo"][0], LABEL);
        // What a call leaves out stays; the same values again change nothing.
        exec(&core, "face_mapping_set", json!({"nodeIds":[mouth.to_string()],"components":["EyesOpen"]}));
        assert_eq!(listed(&core)[0]["phonemes"], json!(["AI", "O"]));
        assert_eq!(listed(&core)[0]["components"], json!(["EyesOpen"]));
        let again = exec(&core, "face_mapping_set", json!({"nodeIds":[mouth.to_string()],"components":["EyesOpen"]}));
        assert_eq!(again["changed"], false);

        exec(&core, "display_undo", json!({}));
        assert_eq!(listed(&core)[0]["components"], json!([]));
        exec(&core, "display_undo", json!({}));
        assert!(listed(&core).is_empty(), "undo takes the new Face property away again");
        exec(&core, "display_redo", json!({}));
        assert_eq!(listed(&core)[0]["phonemes"], json!(["AI", "O"]));
        // The configuration on disk follows each step.
        let saved = std::fs::read_to_string(root.join("SystemData").join("ModuleStore.xml")).unwrap();
        assert!(saved.contains("PhonemeList"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn bad_arguments_change_nothing() {
        let (root, core, mouth) = open();
        let args = |v: Value| core.studio_display.execute(&core, "face_mapping_set", &v, false);
        assert!(args(json!({"nodeIds":[Uuid::new_v4().to_string()]})).is_err());
        assert!(args(json!({"nodeIds":[mouth.to_string()],"color":"red"})).is_err());
        assert!(args(json!({"nodeIds":[mouth.to_string()],"phonemes":["XX"]})).is_err());
        assert!(listed(&core).is_empty());
        assert!(core.studio_display.history()["undo"].as_array().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_face_setup_visit_is_one_step() {
        let (root, core, mouth) = open();
        let before = core.studio_display.face_setup_begin(&core);
        // Face Setup writes straight to the profile as it's used, several times in one visit.
        for on in [true, false, true] {
            CoreWorld(Arc::clone(&core)).with_world(&mut |world| {
                let row = FaceRow {
                    node: mouth,
                    name: "Mouth".into(),
                    phonemes: vec![on; PHONEMES.len()],
                    components: vec![false; COMPONENTS.len()],
                    color: Color::WHITE,
                };
                face_helper_apply(world, &[row]);
            });
        }
        assert_eq!(core.studio_display.face_setup_finish(&core, before, &[mouth]).label.as_deref(), Some(LABEL));
        assert_eq!(core.studio_display.history()["undo"].as_array().unwrap().len(), 1);
        assert_eq!(listed(&core).len(), 1);
        exec(&core, "display_undo", json!({}));
        assert!(listed(&core).is_empty());
        exec(&core, "display_redo", json!({}));
        assert_eq!(listed(&core)[0]["phonemes"].as_array().unwrap().len(), PHONEMES.len());

        // A visit that leaves the properties as they were is no step.
        let unchanged = core.studio_display.face_setup_begin(&core);
        assert_eq!(
            core.studio_display.face_setup_finish(&core, unchanged, &[mouth]),
            crate::studio_display::FaceSetupFinished { changed: false, rendered: 0, label: None }
        );
        assert_eq!(core.studio_display.history()["undo"].as_array().unwrap().len(), 1);

        // Another edit while Face Setup is up: Undo of the visit would take it back too, so the visit applies without a step.
        let moved = core.studio_display.face_setup_begin(&core);
        exec(&core, "face_mapping_set", json!({"nodeIds":[mouth.to_string()],"components":["Outlines"]}));
        exec(&core, "face_mapping_set", json!({"nodeIds":[mouth.to_string()],"color":"#00FF00"}));
        let finished = core.studio_display.face_setup_finish(&core, moved, &[mouth]);
        assert!(finished.changed && finished.label.is_none());
        assert_eq!(core.studio_display.history()["undo"][0], LABEL);
        let _ = std::fs::remove_dir_all(&root);
    }
}
