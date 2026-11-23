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

/// One light a controller drives (B62): a bulb on `outputs` consecutive outputs from `first` (zero-based).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControllerLight {
    pub name: String,
    pub first: u32,
    pub outputs: u32,
}

impl ControllerLight {
    /// The colour handling a bulb on this many outputs takes: 1 single colour, 3 RGB, 4 RGBW.
    pub fn mode(&self) -> &'static str {
        match self.outputs {
            1 => "single",
            4 => "rgbw",
            _ => "rgb",
        }
    }
}

/// What [`lights_of`] needs of a controller, copied out of the working copy so the plugin can be asked without holding it.
#[derive(Clone, Debug)]
pub struct ControllerSnapshot {
    pub id: Uuid,
    pub name: String,
    pub type_id: Uuid,
    pub config: Option<vixen_output::ControllerConfig>,
    /// Each output's name and whether it's patched.
    pub outputs: Vec<(String, bool)>,
}

impl ControllerSnapshot {
    pub fn of(s: &SetupSession, slot: u32) -> ControllerSnapshot {
        let c = &s.controllers[slot as usize];
        let outputs = c.outputs.iter().map(|o| (o.name.clone(), s.flow.source(o.cid).is_some())).collect();
        ControllerSnapshot { id: c.id, name: c.name.clone(), type_id: c.type_id, config: c.config.clone(), outputs }
    }

    /// The controller's lights, in output order, and where they came from: `plugin` when its plugin lists them
    /// (`controller.outputsFor` answers `lights`), else `outputNames` when its outputs are named per light, R G B (W) each
    /// ("Ceiling R", "Ceiling G", "Ceiling B"), as a plugin controller's are. An error says why there are none.
    pub fn lights(&self) -> Result<(Vec<ControllerLight>, &'static str), String> {
        use vixen_output::modules::studio_external;
        let mut why = None;
        if let Some(vixen_output::ControllerConfig::External(config)) = &self.config {
            if studio_external::outputs_from_config(&self.type_id) {
                let settings = studio_external::describe(config).and_then(|d| serde_json::from_str::<Value>(&d).ok()).map(|d| d["config"].clone()).filter(Value::is_object);
                match settings.and_then(|settings| studio_external::outputs_for(config, &settings.to_string())) {
                    Some(Ok(answer)) => {
                        let mut first = 0;
                        let lights: Vec<ControllerLight> = answer
                            .lights
                            .unwrap_or_default()
                            .into_iter()
                            .map(|l| {
                                let light = ControllerLight { name: l.name, first, outputs: l.outputs };
                                first += l.outputs;
                                light
                            })
                            .collect();
                        // Lights past the controller's outputs (settings not applied yet) can't be patched: use the names.
                        if !lights.is_empty() && first as usize <= self.outputs.len() {
                            return Ok((lights, "plugin"));
                        }
                    }
                    Some(Err(e)) => why = Some(e),
                    None => {}
                }
            }
        }
        let names: Vec<&str> = self.outputs.iter().map(|(n, _)| n.as_str()).collect();
        lights_from_names(&names).map(|l| (l, "outputNames")).ok_or_else(|| {
            let base = format!("{} doesn't list its lights: its plugin gives no light list and its outputs aren't named per light (\"Ceiling R\", \"Ceiling G\", \"Ceiling B\")", self.name);
            match why {
                Some(why) => format!("{base}; {why}"),
                None => base,
            }
        })
    }

    /// Whether any of a light's outputs is patched.
    pub fn patched(&self, light: &ControllerLight) -> bool {
        (light.first..light.first + light.outputs).any(|o| self.outputs.get(o as usize).is_some_and(|(_, p)| *p))
    }

    /// `controller_lights`' answer: the lights, outputs one-based, each saying whether it's patched.
    pub fn lights_json(&self) -> Result<Value, String> {
        let (lights, source) = self.lights()?;
        let list: Vec<Value> = lights
            .iter()
            .map(|l| json!({"name":l.name,"firstOutput":l.first + 1,"lastOutput":l.first + l.outputs,"outputCount":l.outputs,"color":l.mode(),"patched":self.patched(l)}))
            .collect();
        let unpatched = lights.iter().filter(|l| !self.patched(l)).count();
        Ok(json!({"controllerId":self.id.to_string(),"name":self.name,"source":source,"lights":list,"unpatched":unpatched}))
    }
}

/// A controller's lights ([`ControllerSnapshot::lights`]).
pub fn lights_of(s: &SetupSession, slot: u32) -> Result<(Vec<ControllerLight>, &'static str), String> {
    ControllerSnapshot::of(s, slot).lights()
}

/// Lights from output names: each run of `<light> R`, `<light> G`, `<light> B` (and `<light> W` after it) is one light. `None`
/// when there are no outputs or any output isn't part of such a run.
pub fn lights_from_names(names: &[&str]) -> Option<Vec<ControllerLight>> {
    let split = |n: &str| n.rsplit_once(' ').map(|(light, c)| (light.trim().to_string(), c.to_string()));
    let mut lights = Vec::new();
    let mut i = 0;
    while i < names.len() {
        let (light, _) = split(names[i])?;
        let channels: Vec<String> = names[i..].iter().take(4).map_while(|n| split(n).filter(|(l, _)| *l == light).map(|(_, c)| c)).collect();
        let outputs = match channels.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
            ["R", "G", "B", "W", ..] => 4,
            ["R", "G", "B", ..] => 3,
            _ => return None,
        };
        if light.is_empty() {
            return None;
        }
        lights.push(ControllerLight { name: light, first: i as u32, outputs });
        i += outputs as usize;
    }
    (!lights.is_empty()).then_some(lights)
}

/// `controller_lights`.
pub fn controller_lights(s: &SetupSession, a: &Value) -> Result<Value, String> {
    let id = uuid_arg(a, "id")?.ok_or("id is required")?;
    let slot = s.slot_of(id).ok_or("Controller not found")?;
    ControllerSnapshot::of(s, slot).lights_json()
}

pub fn lights_tool() -> Value {
    json!({"name":"controller_lights","description":"The lights (devices) a controller drives, in output order, as element_single_bulbs fromController uses them: each light's name, its outputs (firstOutput/lastOutput, one-based), outputCount (1 single colour, 3 RGB, 4 RGBW), the color handling that fits and whether any of its outputs is patched. source plugin: its plugin lists them (a Govee Output controller's Devices table, for example); outputNames: its outputs are named per light, R G B (W) each. An error when the controller doesn't list its lights.","inputSchema":{"type":"object","properties":{"id":{"type":"string","description":"Controller id."}},"required":["id"],"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false,"openWorldHint":false}})
}

/// `element_single_bulbs` with `fromController`: one bulb per light the controller lists (not yet patched, unless
/// `includePatched`), named after it (or `names`, one per light, in order), with the colour handling its outputs take, each
/// patched to its own outputs.
fn create_from_controller(s: &mut SetupSession, a: &Value) -> Result<Value, String> {
    if let Some(key) = ["text", "count", "prefix", "startOutput"].into_iter().find(|k| a.get(*k).is_some()) {
        return Err(format!("{key} doesn't apply with fromController: each bulb takes its own light's outputs"));
    }
    let controller = uuid_arg(a, "controllerId")?.ok_or("fromController needs controllerId")?;
    let slot = s.slot_of(controller).ok_or("Controller not found")?;
    let snapshot = ControllerSnapshot::of(s, slot);
    let (all, source) = snapshot.lights()?;
    let include_patched = a.get("includePatched").map(|v| v.as_bool().ok_or("includePatched must be true or false")).transpose()?.unwrap_or(false);
    let (lights, skipped): (Vec<ControllerLight>, Vec<ControllerLight>) = all.into_iter().partition(|l| include_patched || !snapshot.patched(l));
    let controller_name = s.controllers[slot as usize].name.clone();
    if lights.is_empty() {
        return Err(format!("Every light on {controller_name} is already patched; pass includePatched to add bulbs for them anyway"));
    }
    let names = match a.get("names") {
        Some(_) => {
            let names = names_of(&json!({"names":a["names"]}))?;
            if names.len() != lights.len() {
                return Err(format!("names must give one name per light: {controller_name} has {} to add", lights.len()));
            }
            names
        }
        None => lights.iter().map(|l| l.name.clone()).collect(),
    };
    let chosen = match a.get("color") {
        Some(_) => {
            let (mode, color) = crate::studio_color::parse(a, "color")?;
            let per = crate::studio_color::channels_per_pixel(&mode).unwrap_or(3);
            let misfits: Vec<String> = lights.iter().filter(|l| l.outputs != per).map(|l| format!("{} ({} outputs)", l.name, l.outputs)).collect();
            if !misfits.is_empty() {
                return Err(format!("color {mode} takes {per} outputs a bulb, which doesn't fit {}; leave color out to fit each light", misfits.join(", ")));
            }
            Some((mode, color))
        }
        None => None,
    };
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
    // Colour handling, one call per kind of bulb.
    let mut colors = Vec::new();
    for mode in ["rgb", "rgbw", "single"] {
        let nodes: Vec<_> = heads.iter().zip(&lights).filter(|(_, l)| chosen.as_ref().map_or(l.mode(), |(m, _)| m.as_str()) == mode).map(|(n, _)| *n).collect();
        if !nodes.is_empty() {
            colors.push(crate::studio_color::apply(s, &nodes, mode, chosen.as_ref().and_then(|(_, c)| *c))?);
        }
    }
    let mut patched_total = 0;
    for (head, light) in heads.iter().zip(&lights) {
        let path = s.row_path_of(*head);
        let outputs = crate::setup::controllers::OutputSelection::single(slot, light.first..light.first + light.outputs);
        patched_total += s.patch_elements(&[path], &outputs, &Default::default(), false);
    }
    let ids: Vec<Uuid> = heads.iter().map(|n| s.tree.node(*n).id).collect();
    let made: Vec<Value> = heads.iter().map(|n| json!({"id":s.tree.node(*n).id.to_string(),"name":s.tree.node(*n).name})).collect();
    let wanted: u32 = lights.iter().map(|l| l.outputs).sum();
    let ranges: Vec<Value> = ids.iter().map(|id| crate::studio_patch::ranges_of(s, &json!({"id":id.to_string()})).unwrap_or(Value::Null)).collect();
    let text = ranges.iter().filter_map(|l| Some(format!("{}: {}", l["name"].as_str()?, l["text"].as_str()?))).collect::<Vec<_>>();
    let patch = json!({"controllerId":controller.to_string(),"patched":patched_total,"needed":wanted,"short":wanted.saturating_sub(patched_total),"lights":ranges,"text":text});
    let preview = match preview {
        Some(preview) => Some(crate::studio_props::add_single_bulbs(s, preview, &ids, None)?),
        None => None,
    };
    let from = json!({"source":source,"lights":lights.iter().map(|l| l.name.clone()).collect::<Vec<_>>(),"skipped":skipped.iter().map(|l| l.name.clone()).collect::<Vec<_>>()});
    Ok(json!({"ids":ids.iter().map(|i| i.to_string()).collect::<Vec<_>>(),"bulbs":made,"createdNodes":heads.len(),"color":colors,"patch":patch,"preview":preview,"fromController":from}))
}

/// `element_single_bulbs`. Everything is checked before anything changes; a preview or patch that fails after the elements are
/// made leaves the working copy partly changed, so callers publish only on success (the shared transaction does).
pub fn create(s: &mut SetupSession, a: &Value) -> Result<Value, String> {
    match a.get("fromController") {
        Some(Value::Bool(true)) => return create_from_controller(s, a),
        Some(Value::Bool(false)) | None => {}
        Some(_) => return Err("fromController must be true or false".into()),
    }
    if a.get("includePatched").is_some() {
        return Err("includePatched applies with fromController".into());
    }
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
        "Add single bulbs in one step: smart bulbs, lamps or ceiling lights that are one light each (a Govee bulb on 3 RGB outputs, for example). Makes one Single Item element per name, at the top of the tree, from names (a list), text (a pasted list, one per line; commas and tabs also separate) or count with prefix (\"Bulb 1\", \"Bulb 2\", …); taken names get a number. color is their colour handling (rgb by default: 3 outputs per bulb; rgbw 4; single 1 with singleColor). controllerId patches them in the order given to that controller's free outputs from startOutput (zero-based, default 0), each bulb's channels together, so bulb 1 takes outputs 1-3, bulb 2 outputs 4-6 and so on; patch.short counts channels left unpatched when the controller ran out. previewId also shows each in that preview as a Single Light bulb linked to it and named after it, in a row right of the other props. fromController (with controllerId) makes one bulb per light that controller lists instead (controller_lights shows them: a plugin controller's devices, such as Govee Output's Devices table, or outputs named per light), named after each light (or names, one per light in order), with the colour handling its outputs take unless color is given, each patched to its own light's outputs; lights already patched are skipped (fromController.skipped) unless includePatched. One undo step for all of it.",
        json!({"fromController":{"type":"boolean"},"includePatched":{"type":"boolean"},"names":{"type":"array","items":string,"minItems":1,"maxItems":MAX_BULBS},"text":{"type":"string","maxLength":65536},"count":{"type":"integer","minimum":1,"maximum":MAX_BULBS},"prefix":string,"color":{"type":"string","enum":["rgb","rgbw","single"]},"singleColor":{"type":"string","pattern":"^#[0-9a-fA-F]{6}$"},"controllerId":string,"startOutput":{"type":"integer","minimum":0},"previewId":string}),
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
        // A full-intensity white Set Level reaches every output at 255, as bright as the commissioning tests' allOn at 100%.
        let white = vixen_core::CombinedState { lighting: Some(vixen_core::LightingValue::new(vixen_core::Color::WHITE, 1.0)), ..Default::default() };
        graph.evaluate(&vec![white; 5]);
        assert!(graph.commands(0).iter().all(|c| matches!(c, vixen_output::OutputCommand::U8(255))), "{:?}", graph.commands(0));
        // One step: Undo removes elements, patches and bulbs together.
        run("display_undo", json!({})).unwrap();
        assert_eq!(core.system.read().tree.len(), elements_before);
        drop(core);
        std::fs::remove_dir_all(root).unwrap();
    }

    /// Five RGBW bulbs (Govee Output rows with Channels RGBW) on 20 outputs: patched 20 to 20, and Vixen's RGBW breakdown puts
    /// white on W alone, colour on R G B, and a white with a tint on both.
    #[test]
    fn rgbw_bulbs_take_four_outputs_each_and_white_goes_to_w() {
        let root = std::env::temp_dir().join(format!("studio-bulbs-{}", Uuid::new_v4()));
        let core = AppCore::open(root.clone(), true).unwrap();
        let run = |name: &str, args: Value| core.studio_display.execute(&core, name, &args, true);
        let controller = run("controller_create", json!({"name":"Govee","typeId":vixen_output::ids::E131.to_string(),"outputs":20})).unwrap()["result"]["id"].clone();
        let names = ["Kitchen 1", "Kitchen 2", "Kitchen 3", "Kitchen 4", "Living Room Lamp"];
        let made = run("element_single_bulbs", json!({"names":names,"color":"rgbw","controllerId":controller})).unwrap()["result"].clone();
        assert_eq!(made["color"]["channelsPerPixel"], 4);
        assert_eq!((made["patch"]["patched"].clone(), made["patch"]["needed"].clone(), made["patch"]["short"].clone()), (json!(20), json!(20), json!(0)));
        let system = core.system.read().clone();
        let elements: Vec<Uuid> = made["ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| system.tree.node(system.tree.find(&Uuid::parse_str(id.as_str().unwrap()).unwrap()).unwrap()).element_id.unwrap())
            .collect();
        let mut graph = crate::show::build_output_graph(&system, &elements);
        let lit = |color: vixen_core::Color, intensity: f64| vixen_core::CombinedState { lighting: Some(vixen_core::LightingValue::new(color, intensity)), ..Default::default() };
        graph.evaluate(&[
            lit(vixen_core::Color::WHITE, 1.0),
            lit(vixen_core::Color::RED, 1.0),
            lit(vixen_core::Color::WHITE, 0.5),
            lit(vixen_core::Color::rgb(255, 200, 150), 1.0),
            vixen_core::CombinedState::default(),
        ]);
        let sent: Vec<u8> = graph
            .commands(0)
            .iter()
            .map(|c| match c {
                vixen_output::OutputCommand::U8(v) => *v,
                other => panic!("unexpected command {other:?}"),
            })
            .collect();
        assert_eq!(
            sent.chunks(4).collect::<Vec<_>>(),
            [[0, 0, 0, 255], [255, 0, 0, 0], [0, 0, 0, 127], [105, 50, 0, 150], [0, 0, 0, 0]],
            "R G B W per bulb"
        );
        drop(core);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn output_names_group_into_lights_r_g_b_and_w() {
        let lights = lights_from_names(&["Ceiling R", "Ceiling G", "Ceiling B", "Strip S1 R", "Strip S1 G", "Strip S1 B", "Strip S1 W", "Lamp R", "Lamp G", "Lamp B"]).unwrap();
        let got: Vec<_> = lights.iter().map(|l| (l.name.as_str(), l.first, l.outputs, l.mode())).collect();
        assert_eq!(got, [("Ceiling", 0, 3, "rgb"), ("Strip S1", 3, 4, "rgbw"), ("Lamp", 7, 3, "rgb")]);
        // Default names, a light missing a channel, or no outputs: no lights.
        assert_eq!(lights_from_names(&["Output #1", "Output #2", "Output #3"]), None);
        assert_eq!(lights_from_names(&["A R", "A G", "B B"]), None);
        assert_eq!(lights_from_names(&[]), None);
    }

    /// A controller whose outputs are named per light (as a plugin controller names them): one bulb per light, each on its own
    /// outputs; a second run skips the lights already patched; names rename; a colour that doesn't fit is refused.
    #[test]
    fn bulbs_from_a_controllers_lights_patch_each_to_its_own_outputs() {
        let root = std::env::temp_dir().join(format!("studio-bulbs-{}", Uuid::new_v4()));
        let core = AppCore::open(root.clone(), true).unwrap();
        let run = |name: &str, args: Value| core.studio_display.execute(&core, name, &args, true);
        let controller = run("controller_create", json!({"name":"Govee","typeId":vixen_output::ids::E131.to_string(),"outputs":11})).unwrap()["result"]["id"].clone();
        let mut s = SetupSession::open(&core);
        let slot = s.slot_of(Uuid::parse_str(controller.as_str().unwrap()).unwrap()).unwrap();
        assert!(controller_lights(&s, &json!({"id":controller})).unwrap_err().contains("doesn't list its lights"));
        let names = ["Kitchen 1 R", "Kitchen 1 G", "Kitchen 1 B", "Kitchen 2 R", "Kitchen 2 G", "Kitchen 2 B", "Lamp R", "Lamp G", "Lamp B", "Lamp W"];
        for (o, name) in s.controllers[slot as usize].outputs.iter_mut().zip(names) {
            o.name = name.to_string();
        }
        // Output 11 has no light: the names don't group.
        assert!(controller_lights(&s, &json!({"id":controller})).is_err());
        s.controllers[slot as usize].outputs.pop();
        let listed = controller_lights(&s, &json!({"id":controller})).unwrap();
        assert_eq!(listed["source"], "outputNames");
        assert_eq!(listed["lights"][2], json!({"name":"Lamp","firstOutput":7,"lastOutput":10,"outputCount":4,"color":"rgbw","patched":false}));
        assert!(create(&mut s, &json!({"fromController":true,"controllerId":controller,"color":"rgb"})).unwrap_err().contains("Lamp (4 outputs)"));
        assert!(create(&mut s, &json!({"fromController":true,"controllerId":controller,"startOutput":0})).is_err());
        assert!(create(&mut s, &json!({"fromController":true,"controllerId":controller,"names":["Only one"]})).unwrap_err().contains("one name per light"));

        // Patch Kitchen 2's outputs by hand first: only Kitchen 1 and the lamp are added.
        let other = create(&mut s, &json!({"names":["Old"],"controllerId":controller,"startOutput":3})).unwrap();
        assert_eq!(other["patch"]["patched"], 3);
        let made = create(&mut s, &json!({"fromController":true,"controllerId":controller,"names":["Kitchen Left","Living Room Lamp"]})).unwrap();
        assert_eq!(made["fromController"]["skipped"], json!(["Kitchen 2"]));
        assert_eq!(made["bulbs"].as_array().unwrap().iter().map(|b| b["name"].clone()).collect::<Vec<_>>(), ["Kitchen Left", "Living Room Lamp"]);
        assert_eq!((made["patch"]["patched"].clone(), made["patch"]["short"].clone()), (json!(7), json!(0)));
        let ranges: Vec<_> = made["patch"]["lights"].as_array().unwrap().iter().map(|l| (l["ranges"][0]["firstOutput"].clone(), l["ranges"][0]["lastOutput"].clone())).collect();
        assert_eq!(ranges, [(json!(1), json!(3)), (json!(7), json!(10))]);
        let modes: Vec<_> = made["color"].as_array().unwrap().iter().map(|c| c["channelsPerPixel"].clone()).collect();
        assert_eq!(modes, [json!(3), json!(4)]);
        assert_eq!(controller_lights(&s, &json!({"id":controller})).unwrap()["unpatched"], 0);
        assert!(create(&mut s, &json!({"fromController":true,"controllerId":controller})).unwrap_err().contains("already patched"));
        drop(s);
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
