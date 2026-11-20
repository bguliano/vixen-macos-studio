//! Vixen's Location property for an element's lights, for the MCP `element_location` tool. Wipe (and effect_band and
//! effect_step's vertical bands, which write Wipes) lights only leaves with a Location whose X is above zero, so lights
//! without one stay dark.
//!
//! From a preview, each linked light gets its position there exactly as the preview setup's Save Locations writes it
//! (`SaveLocationDataForElements`: preview coordinates plus the preview's location offset, a prop's centre when it uses
//! a common location). With x and y, every light gets that location, as the Location property's own setup sets it.
//! A light without the property gets one first (`Properties.Add`), keeping its Z when it had one.
use crate::dialogs::properties::property_defaults;
use crate::setup::SetupSession;
use serde_json::{json, Value};
use std::collections::HashMap;
use uuid::Uuid;
use vixen_core::element::{property_ids, PropertyData, PropertyInstance};
use vixen_preview::DesignSession;

pub const DESCRIPTION: &str = "Give an element/group's lights Vixen's Location property, which Wipe needs: Wipe, effect_band and effect_step's vertical bands light only lights that have a Location with X above 0, so lights without one stay dark. By default each light linked in a preview gets its position there, as the preview setup's Save Locations writes it (preview x and y plus the preview's location offset; y grows downward, as Wipe expects). previewId picks the preview; it is required when the lights are linked in more than one. Lights not linked in that preview are left as they are and counted in unlinkedLeaves. Or give x and y (and optionally z, 0..2147483647) to put every light at one location, as the Location property's own setup does. Lights without the property get it. One undo step. Returns updatedLeaves, addedProperties, unlinkedLeaves, unlitLeaves (lights whose X is 0, which Wipe skips) and the preview used.";

pub fn tool_entry() -> (&'static str, &'static str, Value, Vec<&'static str>, bool) {
    let string = json!({"type":"string","minLength":1,"maxLength":256});
    let coordinate = json!({"type":"integer","minimum":0,"maximum":i32::MAX});
    (
        "element_location",
        DESCRIPTION,
        json!({"id":string,"previewId":string,"x":coordinate,"y":coordinate,"z":coordinate}),
        vec!["id"],
        false,
    )
}

fn coordinate(a: &Value, key: &str) -> Result<Option<i32>, String> {
    a.get(key)
        .map(|v| v.as_u64().filter(|n| *n <= i32::MAX as u64).map(|n| n as i32).ok_or(format!("{key} must be an integer in 0..2147483647")))
        .transpose()
}

fn uuid(a: &Value, key: &str) -> Result<Option<Uuid>, String> {
    a.get(key).map(|v| v.as_str().and_then(|s| Uuid::parse_str(s).ok()).ok_or(format!("{key} must be a UUID"))).transpose()
}

/// Each linked light's location in one preview, keyed by node id, as Save Locations computes it. A light shown more than
/// once takes its last position, as the native write does.
fn preview_locations(s: &SetupSession, preview: Uuid) -> Result<HashMap<Uuid, (i32, i32)>, String> {
    let entry = crate::preview::preview_entries(&s.system).into_iter().find(|p| p.id == preview).ok_or("Preview not found")?;
    let mut data = crate::preview::load_preview_data(&s.system.module_store, &entry).map_err(|e| e.to_string())?;
    // Save Locations is the preview's own switch for writing on close; this command is the explicit request.
    data.save_locations = true;
    Ok(DesignSession::new(data, None).location_updates().into_iter().map(|u| (u.node, (u.x, u.y))).collect())
}

pub fn apply(s: &mut SetupSession, a: &Value) -> Result<Value, String> {
    let node = s.tree.find(&uuid(a, "id")?.ok_or("id must be a UUID")?).ok_or("Element not found")?;
    let (x, y, z) = (coordinate(a, "x")?, coordinate(a, "y")?, coordinate(a, "z")?);
    let preview = uuid(a, "previewId")?;
    let fixed = match (x, y) {
        (Some(x), Some(y)) if preview.is_none() => Some((x, y)),
        (Some(_), Some(_)) => return Err("Give either previewId or x and y, not both".into()),
        (None, None) if z.is_none() => None,
        _ => return Err("x and y go together (z is optional)".into()),
    };
    let leaves = s.tree.distinct_leaves([node]);
    let ids: Vec<Uuid> = leaves.iter().map(|n| s.tree.node(*n).id).collect();
    let (locations, used) = match fixed {
        Some(at) => (ids.iter().map(|id| (*id, at)).collect::<HashMap<_, _>>(), None),
        None => {
            let candidates = match preview {
                Some(p) => vec![p],
                None => crate::preview::preview_entries(&s.system).into_iter().map(|p| p.id).collect(),
            };
            let mut found = Vec::new();
            for p in candidates {
                let all = preview_locations(s, p)?;
                if ids.iter().any(|id| all.contains_key(id)) {
                    found.push((p, all));
                }
            }
            match found.len() {
                0 if preview.is_some() => return Err("None of these lights are linked in that preview".into()),
                0 => return Err("No preview shows these lights: link them in a preview first (preview_prop_create with elementId), or give x and y".into()),
                1 => {
                    let (p, all) = found.pop().unwrap();
                    (all, Some(p))
                }
                _ => {
                    let names: Vec<String> = found.iter().map(|(p, _)| p.to_string()).collect();
                    return Err(format!("These lights are linked in more than one preview; pass previewId (one of {})", names.join(", ")));
                }
            }
        }
    };
    let (mut updated, mut added, mut unlinked, mut unlit, mut changed) = (0usize, 0usize, 0usize, 0usize, false);
    for (leaf, id) in leaves.iter().zip(&ids) {
        let Some(&(lx, ly)) = locations.get(id) else {
            unlinked += 1;
            continue;
        };
        let properties = &mut s.tree.node_mut(*leaf).properties;
        let index = match properties.iter().position(|p| p.type_id == property_ids::LOCATION) {
            Some(index) => index,
            None => {
                // `Properties.Add`: a new Location starts at 1, 1, 1.
                let data = property_defaults(&property_ids::LOCATION);
                properties.push(PropertyInstance { type_id: property_ids::LOCATION, instance_id: Uuid::new_v4(), data });
                added += 1;
                properties.len() - 1
            }
        };
        let old_z = match properties[index].data {
            PropertyData::Location { z, .. } => z,
            _ => 1,
        };
        // `LocationData` reads a negative as 0, so a preview position left of the offset is stored as 0.
        let data = PropertyData::Location { x: lx.max(0), y: ly.max(0), z: z.unwrap_or(old_z) };
        if properties[index].data != data {
            properties[index].data = data;
            changed = true;
        }
        updated += 1;
        unlit += usize::from(lx <= 0);
    }
    if changed {
        s.tree_changed();
    }
    Ok(json!({
        "updatedLeaves": updated,
        "addedProperties": added,
        "unlinkedLeaves": unlinked,
        "unlitLeaves": unlit,
        "previewId": used.map(|p| p.to_string()),
    }))
}

#[cfg(test)]
mod tests {
    use crate::app::AppCore;
    use serde_json::{json, Value};
    use uuid::Uuid;
    use vixen_core::element::{property_ids, PropertyData};

    /// How many of `id`'s lights a default Wipe over it lights.
    fn wiped(core: &AppCore, id: &Value) -> usize {
        let tree = core.system.read().tree.clone();
        let node = tree.find(&Uuid::parse_str(id.as_str().unwrap()).unwrap()).unwrap();
        let wipe = vixen_effect_registry::all().iter().find(|d| d.name == "Wipe").unwrap();
        vixen_effects::testing::render_with(wipe, &tree, &[node], 2000, |_| {}).map.len()
    }

    fn location(core: &AppCore, id: &str) -> Option<(i32, i32, i32)> {
        let system = core.system.read();
        let node = system.tree.find(&Uuid::parse_str(id).unwrap()).unwrap();
        system.tree.property(node, &property_ids::LOCATION).and_then(|p| match p.data {
            PropertyData::Location { x, y, z } => Some((x, y, z)),
            _ => None,
        })
    }

    #[test]
    fn preview_positions_let_a_wipe_light_a_tree_and_undo_takes_them_back() {
        let root = std::env::temp_dir().join(format!("studio-locations-{}", Uuid::new_v4()));
        let core = AppCore::open(root.clone(), true).unwrap();
        let run = |name: &str, args: Value| core.studio_display.execute(&core, name, &args, true);
        let preview = run("preview_create", json!({})).unwrap()["result"]["previewId"].clone();
        let tree = run("element_template", json!({"kind":"megatree","name":"Tree","count":8,"pixels":10,"previewId":preview})).unwrap()["result"]["id"].clone();
        assert_eq!(wiped(&core, &tree), 0, "templates make no Location, so a Wipe lights nothing");

        let result = run("element_location", json!({"id":tree})).unwrap()["result"].clone();
        assert_eq!(result["updatedLeaves"], 80);
        assert_eq!(result["addedProperties"], 80);
        assert_eq!((result["unlinkedLeaves"].clone(), result["unlitLeaves"].clone()), (json!(0), json!(0)));
        assert_eq!(result["previewId"], preview);
        assert_eq!(wiped(&core, &tree), 80);
        // Each light sits where display_geometry puts it, and Px1 (the bottom) has the largest y, as Wipe's bands count.
        let system = core.system.read().clone();
        let geometry = crate::studio_preview::geometry::execute(&system, 0, &json!({"elementId":tree})).unwrap();
        for light in geometry["pixels"].as_array().unwrap() {
            let (x, y, z) = location(&core, light["nodeId"].as_str().unwrap()).unwrap();
            assert!((x as f64 - light["x"].as_f64().unwrap()).abs() <= 1.0 && (y as f64 - light["y"].as_f64().unwrap()).abs() <= 1.0, "{light}");
            assert_eq!(z, 1);
        }
        let string1: Vec<_> = geometry["pixels"].as_array().unwrap().iter().filter(|p| p["string"] == 1).collect();
        let y_of = |pixel: u64| location(&core, string1.iter().find(|p| p["pixel"] == pixel).unwrap()["nodeId"].as_str().unwrap()).unwrap().1;
        assert!(y_of(1) > y_of(10), "Px1 is at the bottom");

        let again = run("element_location", json!({"id":tree})).unwrap();
        assert_eq!(again["changed"], false, "an unchanged layout adds no undo step");
        assert!(core.studio_display.history()["undo"][0].as_str().unwrap().starts_with("element_location"));
        run("display_undo", json!({})).unwrap();
        assert_eq!(wiped(&core, &tree), 0);
        drop(core);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn fixed_location_ambiguous_previews_and_unlinked_lights() {
        let root = std::env::temp_dir().join(format!("studio-locations-{}", Uuid::new_v4()));
        let core = AppCore::open(root.clone(), true).unwrap();
        let run = |name: &str, args: Value| core.studio_display.execute(&core, name, &args, true);
        let fail = |name: &str, args: Value| run(name, args).unwrap_err();
        let lamp = run("element_template", json!({"kind":"group","name":"Lamps","count":3})).unwrap()["result"]["id"].clone();
        assert!(fail("element_location", json!({"id":lamp})).contains("No preview shows these lights"));
        assert!(fail("element_location", json!({"id":lamp,"x":5})).contains("x and y go together"));
        let fixed = run("element_location", json!({"id":lamp,"x":5,"y":7,"z":2})).unwrap()["result"].clone();
        assert_eq!(fixed["updatedLeaves"], 3);
        assert_eq!(wiped(&core, &lamp), 3);
        let zero = run("element_location", json!({"id":lamp,"x":0,"y":7})).unwrap()["result"].clone();
        assert_eq!((zero["addedProperties"].clone(), zero["unlitLeaves"].clone()), (json!(0), json!(3)));
        assert_eq!(wiped(&core, &lamp), 0, "Wipe skips X 0, as the result warns");

        let first = run("preview_create", json!({})).unwrap()["result"]["previewId"].clone();
        let second = run("preview_create", json!({})).unwrap()["result"]["previewId"].clone();
        let tree = run("element_template", json!({"kind":"megatree","name":"Tree","count":4,"pixels":5,"previewId":first})).unwrap()["result"]["id"].clone();
        run("preview_prop_create", json!({"previewId":second,"kind":"megatree","elementId":tree})).unwrap();
        assert!(fail("element_location", json!({"id":tree})).contains("more than one preview"));
        assert!(fail("element_location", json!({"id":tree,"previewId":first,"x":1,"y":1})).contains("not both"));
        assert!(fail("element_location", json!({"id":lamp,"previewId":first})).contains("linked in that preview"));
        let picked = run("element_location", json!({"id":tree,"previewId":second})).unwrap()["result"].clone();
        assert_eq!((picked["updatedLeaves"].clone(), picked["previewId"].clone()), (json!(20), second));
        drop(core);
        std::fs::remove_dir_all(root).unwrap();
    }
}
