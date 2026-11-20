//! Colour handling as one choice (RGB, RGBW or Single colour), shared by the element templates' Colour picker, the
//! Display pane and the MCP `element_color` / `element_template` tools. It runs the Color Handling helper's own code
//! (`dialogs::color::color_helper_apply`), so each leaf gets the Color property and a Color Breakdown filter whose items
//! (Red, Green, Blue and, for RGBW, White, in that order) become the channels patching connects.
use crate::dialogs::color::{color_helper_apply, ColorRequest, DEFAULT_SINGLE_COLOR};
use crate::dialogs::setup_world::SetupWorld;
use crate::dialogs::world::NodeKey;
use crate::setup::tree::Nid;
use crate::setup::SetupSession;
use serde_json::{json, Value};
use vixen_core::element::ElementColorType;
use vixen_core::Color;

/// The choices, in the picker's order.
pub const MODES: [&str; 3] = ["rgb", "rgbw", "single"];

/// Channels each pixel takes once patched.
pub fn channels_per_pixel(mode: &str) -> Option<u32> {
    match mode {
        "rgb" => Some(3),
        "rgbw" => Some(4),
        "single" => Some(1),
        _ => None,
    }
}

/// The picker's label for a mode.
pub fn label(mode: &str) -> &'static str {
    match mode {
        "rgbw" => "RGBW",
        "single" => "Single color",
        _ => "RGB",
    }
}

/// The one-line note under the picker: what the choice does to the channel count.
pub fn note(mode: &str) -> &'static str {
    match mode {
        "rgbw" => "4 channels per pixel, in R, G, B, W order: for RGBW strings with a white LED.",
        "single" => "1 channel per light, dimmed in one fixed color: for plain single-color strings.",
        _ => "3 channels per pixel, in R, G, B order: for ordinary RGB pixels.",
    }
}

/// The note for a Color Handling colour order such as "GRB" or "RGBW".
pub fn order_note(order: &str) -> String {
    let n = order.chars().filter(|c| matches!(c, 'R' | 'G' | 'B' | 'W')).count();
    let letters: Vec<String> = order.chars().map(String::from).collect();
    format!("{n} channels per pixel, in {} order.", letters.join(", "))
}

/// Applies a mode to every leaf under `nodes`, replacing any colour property and Color Breakdown filter they had.
/// `color` is Single colour's colour (default royal blue). Returns {updatedLeaves, filters, channelsPerPixel, mode}.
pub fn apply(s: &mut SetupSession, nodes: &[Nid], mode: &str, color: Option<Color>) -> Result<Value, String> {
    let channels = channels_per_pixel(mode).ok_or("mode must be rgb, rgbw or single")?;
    let request = ColorRequest {
        color_type: if mode == "single" { ElementColorType::SingleColor } else { ElementColorType::FullColor },
        single_color: color.unwrap_or(DEFAULT_SINGLE_COLOR),
        set_name: None,
        color_order: if mode == "rgbw" { "RGBW" } else { "RGB" }.to_string(),
    };
    let leaves = s.tree.distinct_leaves(nodes.iter().copied()).len();
    let keys: Vec<NodeKey> = nodes.iter().map(|n| NodeKey(n.0)).collect();
    let report = color_helper_apply(&mut SetupWorld(s), &keys, &request, true, true);
    s.tree_changed();
    s.bump_graph();
    Ok(json!({
        "mode": mode,
        "updatedLeaves": leaves,
        "filters": report.filters_configured,
        "channelsPerPixel": channels,
    }))
}

/// Reads `mode` and `color` from command arguments (`element_color`, `element_template`, the Display pane).
pub fn parse(args: &Value, mode_key: &str) -> Result<(String, Option<Color>), String> {
    let mode = args[mode_key].as_str().ok_or(format!("{mode_key} must be rgb, rgbw or single"))?;
    if channels_per_pixel(mode).is_none() {
        return Err(format!("{mode_key} must be rgb, rgbw or single"));
    }
    let color = match args.get("singleColor").or_else(|| args.get("color")) {
        Some(value) if mode == "single" => {
            match crate::studio_params::decode(vixen_effects::framework::ParamKind::Color, value)? {
                vixen_effects::framework::ParamValue::Color(c) => Some(c),
                _ => None,
            }
        }
        _ => None,
    };
    Ok((mode.to_string(), color))
}

/// The Display pane's entry point: {ids:[UUID], mode, color?} on a working copy.
pub fn apply_to_ids(s: &mut SetupSession, args: &Value) -> Result<Value, String> {
    let (mode, color) = parse(args, "mode")?;
    let ids = args["ids"].as_array().ok_or("ids must be a list of element UUIDs")?;
    let mut nodes = Vec::new();
    for id in ids {
        let id = id.as_str().and_then(|s| uuid::Uuid::parse_str(s).ok()).ok_or("ids must be element UUIDs")?;
        nodes.push(s.tree.find(&id).ok_or("Element not found")?);
    }
    apply(s, &nodes, &mode, color)
}

/// The picker's choices with labels, channel counts and notes, for the UI.
pub fn choices() -> Value {
    Value::Array(
        MODES
            .iter()
            .map(|m| json!({"mode":m,"label":label(m),"channelsPerPixel":channels_per_pixel(m),"note":note(m)}))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use crate::app::AppCore;
    use crate::dialogs::world::{DialogWorld, SystemWorld};
    use serde_json::{json, Value};
    use uuid::Uuid;
    use vixen_output::filters::FilterConfig;

    /// Each patched output of `controller`, in output order, as the colour name of the breakdown item feeding it.
    fn patched_colors(core: &AppCore, controller: &Value) -> Vec<String> {
        let controller = Uuid::parse_str(controller.as_str().unwrap()).unwrap();
        let mut system = core.system.read().clone();
        let outputs: Vec<Uuid> =
            system.controllers.iter().find(|c| c.id == controller).unwrap().outputs.iter().map(|o| o.id).collect();
        let patches = system.patches.clone();
        let world = SystemWorld::new(&mut system);
        outputs
            .iter()
            .filter_map(|output| patches.iter().find(|p| p.component_id == *output && p.source_id.is_some()))
            .map(|p| match world.filter_config(&p.source_id.unwrap()) {
                Some(FilterConfig::ColorBreakdown(c)) => c.breakdown_items[p.output as usize].name.clone(),
                other => panic!("output not fed by a colour breakdown: {other:?}"),
            })
            .collect()
    }

    fn rgbw_tree_patches_to_exact_outputs(template_color: bool) {
        let root = std::env::temp_dir().join(format!("studio-color-{}", Uuid::new_v4()));
        let core = AppCore::open(root.clone(), true).unwrap();
        let run = |name: &str, args: Value| core.studio_display.execute(&core, name, &args, true).unwrap()["result"].clone();
        let mut args = json!({"kind":"megatree","name":"Tree","count":32,"pixels":28});
        if template_color {
            args["color"] = json!("rgbw");
        }
        let tree = run("element_template", args);
        let id = tree["id"].clone();
        if template_color {
            assert_eq!(tree["color"]["channelsPerPixel"], 4);
            assert_eq!(tree["color"]["updatedLeaves"], 896);
        } else {
            let result = run("element_color", json!({"id":id,"mode":"rgbw"}));
            assert_eq!(result["updatedLeaves"], 896);
            assert_eq!(result["filters"], 896);
        }
        let controller = run(
            "controller_create",
            json!({"name":"Tree controller","typeId":vixen_output::ids::E131.to_string(),"outputs":4000}),
        )["id"]
            .clone();
        let patched = run("patch_elements", json!({"elementId":id,"controllerId":controller}));
        assert_eq!(patched["patched"], 3584);
        let colors = patched_colors(&core, &controller);
        assert_eq!(colors.len(), 3584);
        for (i, color) in colors.iter().enumerate() {
            assert_eq!(color, ["Red", "Green", "Blue", "White"][i % 4], "output {i}");
        }
        drop(core);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rgbw_megatree_from_the_template_patches_to_3584_outputs_in_rgbw_order() {
        rgbw_tree_patches_to_exact_outputs(true);
    }

    #[test]
    fn element_color_rgbw_patches_to_3584_outputs_in_rgbw_order() {
        rgbw_tree_patches_to_exact_outputs(false);
    }

    #[test]
    fn switching_modes_replaces_the_breakdown_and_single_takes_one_channel() {
        let root = std::env::temp_dir().join(format!("studio-color-{}", Uuid::new_v4()));
        let core = AppCore::open(root.clone(), true).unwrap();
        let run = |name: &str, args: Value| core.studio_display.execute(&core, name, &args, true).unwrap()["result"].clone();
        let id = run("element_template", json!({"kind":"single","name":"Lamp","count":1,"color":"rgbw"}))["id"].clone();
        run("element_color", json!({"id":id,"mode":"single","color":"#ff0000"}));
        assert_eq!(core.system.read().filters.len(), 1, "the RGBW breakdown is reused, not doubled");
        let controller = run(
            "controller_create",
            json!({"name":"C","typeId":vixen_output::ids::E131.to_string(),"outputs":8}),
        )["id"]
            .clone();
        assert_eq!(run("patch_elements", json!({"elementId":id,"controllerId":controller}))["patched"], 1);
        assert!(core.studio_display.execute(&core, "element_color", &json!({"id":id,"mode":"rgbx"}), true).is_err());
        drop(core);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn notes_name_the_channel_count() {
        assert!(super::note("rgbw").starts_with("4 channels per pixel"));
        assert_eq!(super::order_note("GRB"), "3 channels per pixel, in G, R, B order.");
        assert_eq!(super::choices().as_array().unwrap().len(), 3);
    }
}
