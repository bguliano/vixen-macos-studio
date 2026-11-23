//! Single bulbs in one step (B60): smart bulbs, lamps and ceiling lights that are one light each (a Govee bulb on three RGB
//! outputs, say). `element_single_bulbs` makes one Single Item per name with its colour handling, can patch them in order to a
//! controller's outputs and show each in a preview as a linked Single Light bulb, all in one step on the shared history. The
//! Display pane's Add ▸ Single Bulbs… sheet and the MCP tool run this code; the preview part is `studio_props::add_single_bulbs`.
use crate::setup::templates::TemplateParams;
use crate::setup::SetupSession;
use serde_json::{json, Value};
use uuid::Uuid;

/// The most bulbs one call makes.
pub const MAX_BULBS: usize = 1000;

/// The names in a pasted list: one per line (commas and tabs separate too, as a spreadsheet row pastes), trimmed, blanks
/// dropped, each at most 256 characters.
pub fn parse_names(text: &str) -> Vec<String> {
    text.split(['\n', '\r', ',', '\t'])
        .map(|n| n.trim().chars().take(256).collect::<String>())
        .filter(|n| !n.is_empty())
        .collect()
}

/// The names a call gives: `names` (a list) or `text` (a pasted list), or `count` bulbs named `prefix` 1, 2, … .
pub fn names_of(a: &Value) -> Result<Vec<String>, String> {
    let names = if let Some(list) = a.get("names") {
        list.as_array()
            .ok_or("names must be a list of names")?
            .iter()
            .map(|n| n.as_str().map(|n| n.trim().chars().take(256).collect::<String>()).filter(|n| !n.is_empty()).ok_or("names must be non-empty text"))
            .collect::<Result<Vec<_>, _>>()?
    } else if let Some(text) = a.get("text") {
        parse_names(text.as_str().ok_or("text must be the names, one per line")?)
    } else if let Some(count) = a.get("count") {
        let count = count.as_u64().filter(|n| *n >= 1 && *n as usize <= MAX_BULBS).ok_or(format!("count must be 1..{MAX_BULBS}"))?;
        let prefix = a.get("prefix").map(|p| p.as_str().map(str::trim).filter(|p| !p.is_empty()).ok_or("prefix must be text")).transpose()?.unwrap_or("Bulb");
        (1..=count).map(|i| format!("{prefix} {i}")).collect()
    } else {
        return Err("Give names, text or count".into());
    };
    if names.is_empty() {
        return Err("Give at least one name".into());
    }
    if names.len() > MAX_BULBS {
        return Err(format!("At most {MAX_BULBS} bulbs at once"));
    }
    Ok(names)
}

fn uuid_arg(a: &Value, key: &str) -> Result<Option<Uuid>, String> {
    a.get(key)
        .map(|v| v.as_str().and_then(|t| Uuid::parse_str(t).ok()).ok_or(format!("{key} must be a UUID")))
        .transpose()
}

/// `element_single_bulbs`. Everything is checked before anything changes; a preview or patch that fails after the elements are
/// made leaves the working copy partly changed, so callers publish only on success (the shared transaction does).
pub fn create(s: &mut SetupSession, a: &Value) -> Result<Value, String> {
    let names = names_of(a)?;
    let (mode, color) = match a.get("color") {
        Some(_) => crate::studio_color::parse(a, "color")?,
        None => ("rgb".to_string(), None),
    };
    let per_light = crate::studio_color::channels_per_pixel(&mode).unwrap_or(3);
    let controller = uuid_arg(a, "controllerId")?;
    let start = a.get("startOutput").map(|v| v.as_u64().ok_or("startOutput must be a zero-based output number")).transpose()?;
    if start.is_some() && controller.is_none() {
        return Err("startOutput needs controllerId".into());
    }
    let slot = match controller {
        Some(id) => Some(s.slot_of(id).ok_or("Controller not found")?),
        None => None,
    };
    let start = start.unwrap_or(0);
    if let Some(slot) = slot {
        if start >= s.controllers[slot as usize].outputs.len() as u64 {
            return Err("startOutput is past the controller's last output".into());
        }
    }
    let preview = uuid_arg(a, "previewId")?;
    if let Some(preview) = preview {
        if !crate::preview::preview_entries(&s.system).iter().any(|p| p.id == preview) {
            return Err("Preview not found".into());
        }
    }

    let mut heads = Vec::with_capacity(names.len());
    for name in &names {
        let built = crate::studio_wiring::generate(&mut s.tree, &TemplateParams::Single { name: name.clone() }, 0).map_err(|e| format!("{e:?}"))?;
        heads.extend(built.result.head);
        s.apply_template_result(built.result);
    }
    let color = crate::studio_color::apply(s, &heads, &mode, color)?;
    let ids: Vec<Uuid> = heads.iter().map(|n| s.tree.node(*n).id).collect();
    let made: Vec<Value> = heads.iter().map(|n| json!({"id":s.tree.node(*n).id.to_string(),"name":s.tree.node(*n).name})).collect();

    let patch = match slot {
        Some(slot) => {
            let paths: Vec<String> = heads.iter().map(|n| s.row_path_of(*n)).collect();
            let count = s.controllers[slot as usize].outputs.len() as u32;
            let outputs = crate::setup::controllers::OutputSelection::single(slot, start as u32..count);
            let patched = s.patch_elements(&paths, &outputs, &Default::default(), false);
            let wanted = names.len() as u32 * per_light;
            let lights: Vec<Value> = ids
                .iter()
                .map(|id| crate::studio_patch::ranges_of(s, &json!({"id":id.to_string()})).unwrap_or(Value::Null))
                .collect();
            let text = lights.iter().filter_map(|l| Some(format!("{}: {}", l["name"].as_str()?, l["text"].as_str()?))).collect::<Vec<_>>();
            Some(json!({"controllerId":controller.map(|c| c.to_string()),"patched":patched,"needed":wanted,"short":wanted.saturating_sub(patched),"outputsPerLight":per_light,"lights":lights,"text":text}))
        }
        None => None,
    };
    let preview = match preview {
        Some(preview) => Some(crate::studio_props::add_single_bulbs(s, preview, &ids, None)?),
        None => None,
    };
    Ok(json!({"ids":ids.iter().map(|i| i.to_string()).collect::<Vec<_>>(),"bulbs":made,"createdNodes":heads.len(),"color":color,"patch":patch,"preview":preview}))
}

pub fn tool() -> (&'static str, &'static str, Value, Vec<&'static str>) {
    let string = json!({"type":"string","minLength":1,"maxLength":256});
    (
        "element_single_bulbs",
        "Add single bulbs in one step: smart bulbs, lamps or ceiling lights that are one light each (a Govee bulb on 3 RGB outputs, for example). Makes one Single Item element per name, at the top of the tree, from names (a list), text (a pasted list, one per line; commas and tabs also separate) or count with prefix (\"Bulb 1\", \"Bulb 2\", …); taken names get a number. color is their colour handling (rgb by default: 3 outputs per bulb; rgbw 4; single 1 with singleColor). controllerId patches them in the order given to that controller's free outputs from startOutput (zero-based, default 0), each bulb's channels together, so bulb 1 takes outputs 1-3, bulb 2 outputs 4-6 and so on; patch.short counts channels left unpatched when the controller ran out. previewId also shows each in that preview as a Single Light bulb linked to it and named after it, in a row right of the other props. One undo step for all of it.",
        json!({"names":{"type":"array","items":string,"minItems":1,"maxItems":MAX_BULBS},"text":{"type":"string","maxLength":65536},"count":{"type":"integer","minimum":1,"maximum":MAX_BULBS},"prefix":string,"color":{"type":"string","enum":["rgb","rgbw","single"]},"singleColor":{"type":"string","pattern":"^#[0-9a-fA-F]{6}$"},"controllerId":string,"startOutput":{"type":"integer","minimum":0},"previewId":string}),
        vec![],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AppCore;

    /// Every preview light's element, in the preview's order.
    fn linked_lights(core: &AppCore, preview: &Value) -> Vec<String> {
        let system = core.system.read();
        let entry = crate::preview::preview_entries(&system).into_iter().find(|p| p.id.to_string() == preview.as_str().unwrap()).unwrap();
        let data = crate::preview::load_preview_data(&system.module_store, &entry).unwrap();
        let mut out = Vec::new();
        for item in &data.display_items {
            item.shape.for_each_pixel(&mut |p| out.push(p.node_id.to_string()));
        }
        out
    }

    #[test]
    fn pasted_names_split_on_lines_commas_and_tabs() {
        assert_eq!(parse_names("Kitchen 1\r\nKitchen 2, Kitchen 3\t Lamp \n\n"), ["Kitchen 1", "Kitchen 2", "Kitchen 3", "Lamp"]);
        assert!(names_of(&json!({"text":" \n ,"})).is_err());
        assert_eq!(names_of(&json!({"count":2,"prefix":"Ceiling"})).unwrap(), ["Ceiling 1", "Ceiling 2"]);
    }

    /// Five Govee-shaped bulbs (3 RGB outputs each) on a 15-output controller: patched 1-15 in order, linked in the preview,
    /// one undo step that takes everything away again.
    #[test]
    fn govee_shaped_bulbs_patch_in_order_and_show_linked() {
        let root = std::env::temp_dir().join(format!("studio-bulbs-{}", Uuid::new_v4()));
        let core = AppCore::open(root.clone(), true).unwrap();
        let run = |name: &str, args: Value| core.studio_display.execute(&core, name, &args, true);
        let controller = run("controller_create", json!({"name":"Govee","typeId":vixen_output::ids::E131.to_string(),"outputs":15})).unwrap()["result"]["id"].clone();
        let preview = run("preview_create", json!({})).unwrap()["result"]["previewId"].clone();
        let elements_before = core.system.read().tree.len();
        let names = ["Kitchen 1", "Kitchen 2", "Kitchen 3", "Kitchen 4", "Living Room Lamp"];
        let made = run("element_single_bulbs", json!({"names":names,"controllerId":controller,"previewId":preview})).unwrap()["result"].clone();
        assert_eq!(made["createdNodes"], 5);
        assert_eq!(made["color"]["channelsPerPixel"], 3);
        assert_eq!(made["patch"]["patched"], 15);
        assert_eq!(made["patch"]["short"], 0);
        for (i, light) in made["patch"]["lights"].as_array().unwrap().iter().enumerate() {
            assert_eq!(light["name"], names[i]);
            assert_eq!(light["ranges"][0]["firstOutput"], 3 * i + 1, "{light}");
            assert_eq!(light["ranges"][0]["lastOutput"], 3 * i + 3, "{light}");
        }
        assert_eq!(made["preview"]["linkedLights"], 5);
        // Each bulb in the preview shows its own element.
        let ids: Vec<String> = made["ids"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
        assert_eq!(linked_lights(&core, &preview), ids);
        // The designer keeps every bulb when it opens the preview: none sits where opening drops a sizeless shape (x = y).
        {
            let system = core.system.read();
            let entry = crate::preview::preview_entries(&system).into_iter().find(|p| p.id.to_string() == preview.as_str().unwrap()).unwrap();
            let data = crate::preview::load_preview_data(&system.module_store, &entry).unwrap();
            let mut design = vixen_preview::DesignSession::new(data, Some(std::sync::Arc::new(system.tree.clone())));
            let (_, report) = design.setup_open();
            assert!(report.removed.is_empty(), "opening removed {:?}", report.removed);
            assert_eq!(design.data().display_items.len(), 5);
        }
        // Live Output: each bulb's colour reaches its own three outputs, R G B, through the output graph Live Output runs.
        let system = core.system.read().clone();
        let elements: Vec<Uuid> = ids
            .iter()
            .map(|id| system.tree.node(system.tree.find(&Uuid::parse_str(id).unwrap()).unwrap()).element_id.unwrap())
            .collect();
        let mut graph = crate::show::build_output_graph(&system, &elements);
        let states: Vec<vixen_core::CombinedState> = (0..5u8)
            .map(|i| vixen_core::CombinedState { rgb: Some(vixen_core::value::RgbValue { r: 10 + i, g: 100 + i, b: 200 + i }), ..Default::default() })
            .collect();
        graph.evaluate(&states);
        let sent: Vec<u8> = graph
            .commands(0)
            .iter()
            .map(|c| match c {
                vixen_output::OutputCommand::U8(v) => *v,
                other => panic!("unexpected command {other:?}"),
            })
            .collect();
        assert_eq!(sent, (0..5u8).flat_map(|i| [10 + i, 100 + i, 200 + i]).collect::<Vec<_>>());
        // One step: Undo removes elements, patches and bulbs together.
        run("display_undo", json!({})).unwrap();
        assert_eq!(core.system.read().tree.len(), elements_before);
        drop(core);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn several_existing_lights_get_one_linked_bulb_each() {
        let root = std::env::temp_dir().join(format!("studio-bulbs-{}", Uuid::new_v4()));
        let core = AppCore::open(root.clone(), true).unwrap();
        let run = |name: &str, args: Value| core.studio_display.execute(&core, name, &args, true);
        let preview = run("preview_create", json!({})).unwrap()["result"]["previewId"].clone();
        let lamp = run("element_template", json!({"kind":"single","name":"Lamp","count":1,"color":"rgb","previewId":preview})).unwrap()["result"].clone();
        assert_eq!(lamp["preview"]["linkedLights"], 1);
        let group = run("element_template", json!({"kind":"group","name":"Porch","count":3})).unwrap()["result"]["id"].clone();
        let added = run("preview_prop_create", json!({"previewId":preview,"kind":"single","elementIds":[group]})).unwrap()["result"].clone();
        assert_eq!(added["linkedLights"], 3);
        assert_eq!(added["propIds"].as_array().unwrap().len(), 3);
        assert_eq!(linked_lights(&core, &preview).len(), 4);
        // The second row goes beside the first bulb, never on top of it.
        {
            let system = core.system.read();
            let entry = crate::preview::preview_entries(&system).into_iter().find(|p| p.id.to_string() == preview.as_str().unwrap()).unwrap();
            let data = crate::preview::load_preview_data(&system.module_store, &entry).unwrap();
            let spots: std::collections::HashSet<_> = data.display_items.iter().flat_map(|i| i.shape.pixels_vec()).map(|p| (p.x, p.y)).collect();
            assert_eq!(spots.len(), 4);
        }
        assert!(run("preview_prop_create", json!({"previewId":preview,"kind":"arch","elementIds":[group]})).is_err());
        drop(core);
        std::fs::remove_dir_all(root).unwrap();
    }
}
