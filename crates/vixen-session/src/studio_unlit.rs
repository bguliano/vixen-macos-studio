//! Placement warnings for effects that can't show on their targets (B97): a movement or pixel effect on a one-channel
//! prop, a Wipe on lights without a Location, or colours a fixed-colour prop can't show. Placement tools add
//! `unlitTargets` so an agent hears about it at once, without rendering; sequence_summary still confirms by rendering.
use crate::sequence::{EffectEntry, EffectId, SequenceSession};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};
use vixen_core::element::{property_ids, NodeId, PropertyData};
use vixen_core::ElementTree;

/// Effects that render nothing on a target with a single light (one channel): they draw across several lights. Found by
/// rendering every effect with its defaults on one light and on twenty located lights; a test keeps the list honest.
pub(super) const ONE_CHANNEL_DARK: &[&str] = &[
    "Bars", "Border", "Butterfly", "Circles", "Colorwash", "Count Down", "Fire", "Garlands", "Life", "Liquid", "Meteors",
    "Pattern", "Picture", "Pinwheel", "Plasma", "Shockwave", "Snowflakes", "Spiral", "Tree", "Wave", "Whirlpool", "Wipe",
];
/// Warning groups listed in a result; `unlitCount` always counts every effect.
const LISTED: usize = 20;
/// Effect ids listed per warning group.
const IDS_LISTED: usize = 5;

/// Why an effect can't show on its target's lights, and how many of them stay dark.
struct Finding {
    reason: &'static str,
    detail: String,
    lights: usize,
    dark: usize,
}

fn distinct_leaves(tree: &ElementTree, targets: &[NodeId]) -> Vec<NodeId> {
    let mut seen = HashSet::new();
    targets.iter().flat_map(|t| tree.leaves(*t)).filter(|l| seen.insert(*l)).collect()
}

fn hex(c: vixen_core::Color) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
}

fn finding(s: &SequenceSession, e: &EffectEntry) -> Option<Finding> {
    let name = e.descriptor?.name;
    let tree = s.tree();
    let leaves = distinct_leaves(tree, &e.targets);
    let lights = leaves.len();
    if lights == 0 {
        return None;
    }
    if lights == 1 && ONE_CHANNEL_DARK.contains(&name) {
        return Some(Finding {
            reason: "oneChannel",
            detail: format!("{name} moves across several lights and shows nothing on a one-channel prop: use Pulse, Set Level, Twinkle or Chase, or place it across several props"),
            lights,
            dark: 1,
        });
    }
    if name == "Wipe" {
        // Wipe skips lights without a Location whose X is above 0.
        let unlocated = leaves
            .iter()
            .filter(|l| !matches!(tree.property(**l, &property_ids::LOCATION).map(|p| &p.data), Some(PropertyData::Location { x, .. }) if *x > 0))
            .count();
        if unlocated > 0 {
            return Some(Finding {
                reason: "noLocation",
                detail: "Wipe only lights lights with a Location (X above 0): element_location gives them one from the preview".into(),
                lights,
                dark: unlocated,
            });
        }
    }
    // A fixed-colour light shows only its own colours, matched exactly: an effect in none of them leaves it dark.
    let colors: Vec<String> = super::seq_palette::colors(s, e).into_iter().filter(|c| c != "#000000").collect();
    if colors.is_empty() {
        return None;
    }
    let mut fixed: Vec<String> = Vec::new();
    let dark = leaves
        .iter()
        .filter(|l| {
            if !tree.is_discrete(**l) {
                return false;
            }
            let valid: Vec<String> = tree.valid_colors(**l, false).into_iter().map(hex).collect();
            let shown = valid.iter().any(|v| colors.contains(v));
            if !shown {
                for v in valid {
                    if !fixed.contains(&v) {
                        fixed.push(v);
                    }
                }
            }
            !shown
        })
        .count();
    (dark > 0).then(|| Finding {
        reason: "colors",
        detail: format!("its colours {} aren't among the colours these lights can show ({}); display_layout lists each prop's colors", colors.join(", "), fixed.join(", ")),
        lights,
        dark,
    })
}

/// The placement tools that report `unlitTargets`.
const PLACERS: &[&str] = &["effect_add", "effect_band", "effects_batch", "effects_on_marks", "effect_step", "motif_place", "passage_copy"];
const NOTE: &str = " Effects it adds that can't show on their targets are listed in unlitTargets ({targetId, targetName, effect, reason, detail, lights, darkLights, effects, effectIds}; unlitCount counts every such effect): reason oneChannel (a movement or pixel effect such as Wipe, Meteors, Bars or Fire on a one-channel prop), noLocation (a Wipe on lights without a Location) or colors (colours a fixed-colour prop can't show; display_layout lists its colors). Checked without rendering; sequence_summary with previewId confirms by rendering.";

/// A placement tool's description with the unlitTargets note; other tools' as they are.
pub(super) fn described(name: &str, description: &'static str) -> std::borrow::Cow<'static, str> {
    if PLACERS.contains(&name) {
        std::borrow::Cow::Owned(format!("{description}{NOTE}"))
    } else {
        std::borrow::Cow::Borrowed(description)
    }
}

/// Adds `unlitTargets` (and `unlitCount`) to a placement tool's result for the effects it names in effectIds/effectId that
/// can't show on their targets, grouped by target, effect and reason. A clean placement's result is unchanged.
pub(super) fn warn(s: &SequenceSession, mut result: Value) -> Value {
    let mut ids: Vec<u64> = result["effectIds"].as_array().map(|l| l.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
    ids.extend(result["effectId"].as_u64());
    ids.sort_unstable();
    ids.dedup();
    // (target ids, effect, reason) -> (target names, detail, lights, dark, effect ids)
    let mut groups: BTreeMap<(String, String, &'static str), (String, String, usize, usize, Vec<u64>)> = BTreeMap::new();
    let mut count = 0;
    for e in ids.into_iter().filter_map(|id| s.effect(EffectId(id))) {
        let Some(f) = finding(s, e) else { continue };
        count += 1;
        let tree = s.tree();
        let target_ids = e.targets.iter().map(|t| tree.node(*t).id.to_string()).collect::<Vec<_>>().join(",");
        let names = e.targets.iter().map(|t| tree.node(*t).name.clone()).collect::<Vec<_>>().join(", ");
        let effect = e.descriptor.map_or("Effect", |d| d.name).to_string();
        let g = groups.entry((target_ids, effect, f.reason)).or_insert_with(|| (names, f.detail, f.lights, f.dark, Vec::new()));
        g.3 = g.3.max(f.dark);
        g.4.push(e.id.0);
    }
    if count == 0 {
        return result;
    }
    let listed: Vec<Value> = groups
        .into_iter()
        .take(LISTED)
        .map(|((target, effect, reason), (name, detail, lights, dark, ids))| {
            json!({
                "targetId": target, "targetName": name, "effect": effect, "reason": reason, "detail": detail,
                "lights": lights, "darkLights": dark, "effects": ids.len(), "effectIds": &ids[..ids.len().min(IDS_LISTED)],
            })
        })
        .collect();
    result["unlitTargets"] = json!(listed);
    result["unlitCount"] = json!(count);
    result
}

#[cfg(test)]
mod tests {
    use super::super::execute;
    use crate::app::AppCore;
    use serde_json::{json, Value};
    use std::sync::atomic::AtomicBool;
    use uuid::Uuid;
    use vixen_core::element::{property_ids, PropertyData, PropertyInstance};
    use vixen_core::{Color, ElementColorType, ElementTree, NodeId, StateValue, TimeSpan};
    use vixen_effects::framework::{EffectDescriptor, ParamValue, Params, RenderCtx};

    /// How many of the targets' lights the effect lights at some frame of a 4 s render.
    fn lit(tree: &ElementTree, targets: &[NodeId], d: &'static EffectDescriptor, set: &dyn Fn(&mut Params)) -> usize {
        let cancel = AtomicBool::new(false);
        let ctx = RenderCtx { video_cache: None, tree, targets, start_time: TimeSpan::ZERO, time_span: TimeSpan::from_ms_i(4000), frame_ms: 50,
            marks: &[], audio: None, module_data_root: None, cancel: &cancel };
        let mut params = Params::defaults(d.params);
        d.load_data(&mut params, &ctx.target_info());
        set(&mut params);
        let Ok(out) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (d.render)(&ctx, &params))) else { return 0 };
        let leaves: Vec<NodeId> = targets.iter().flat_map(|t| tree.leaves(*t)).collect();
        leaves
            .iter()
            .filter(|l| {
                let id = tree.node(**l).element_id.unwrap();
                out.map.get(&id).into_iter().flatten().any(|n| {
                    (0..80).map(|f| TimeSpan::from_ms_i(f * 50)).any(|t| {
                        n.contains(t) && match n.intent.state_at(t - n.start) {
                            StateValue::Lighting(v) => v.intensity > 0.01,
                            StateValue::Discrete(v) => v.intensity > 0.01 && tree.valid_colors(**l, false).contains(&v.color),
                            StateValue::Rgb(v) => u32::from(v.r) + u32::from(v.g) + u32::from(v.b) > 3,
                            _ => false,
                        }
                    })
                })
            })
            .count()
    }

    #[test]
    fn the_one_channel_list_and_colour_rule_match_rendering() {
        let mut tree = ElementTree::new();
        let single = tree.add_node(Uuid::new_v4(), "single", Some(Uuid::new_v4()));
        let located = tree.add_node(Uuid::new_v4(), "located", None);
        for i in 0..20 {
            let n = tree.add_node(Uuid::new_v4(), &format!("l{i}"), Some(Uuid::new_v4()));
            tree.node_mut(n).properties.push(PropertyInstance { type_id: property_ids::LOCATION, instance_id: Uuid::new_v4(), data: PropertyData::Location { x: 10 + i * 5, y: 10 + i * 5, z: 0 } });
            tree.add_child(located, n);
        }
        let mut dark: Vec<&str> = vixen_effect_registry::all()
            .iter()
            .filter(|d| lit(&tree, &[single], d, &|_| {}) == 0 && lit(&tree, &[located], d, &|_| {}) > 0)
            .map(|d| d.name)
            .collect();
        dark.sort_unstable();
        assert_eq!(dark, super::ONE_CHANNEL_DARK, "effects with their defaults that light many lights but not one");

        // A white-only light shows a white Pulse and stays dark under a red one.
        let white = tree.add_node(Uuid::new_v4(), "white", Some(Uuid::new_v4()));
        tree.node_mut(white).properties.push(PropertyInstance { type_id: property_ids::COLOR, instance_id: Uuid::new_v4(),
            data: PropertyData::Color { color_type: ElementColorType::SingleColor, single_color: Color::rgb(255, 255, 255), color_set_name: None } });
        let pulse = vixen_effect_registry::all().iter().find(|d| d.name == "Pulse").unwrap();
        let colored = |c: Color| move |p: &mut Params| { p.set("ColorGradient", ParamValue::Gradient(vixen_core::ColorGradient::solid(c))); };
        assert_eq!(lit(&tree, &[white], pulse, &colored(Color::rgb(255, 255, 255))), 1);
        assert_eq!(lit(&tree, &[white], pulse, &colored(Color::rgb(255, 0, 0))), 0);
    }

    #[test]
    fn placements_warn_about_targets_the_effect_cannot_light() {
        let root = std::env::temp_dir().join(format!("studio-unlit-{}", Uuid::new_v4()));
        let core = AppCore::open(root.clone(), true).unwrap();
        let display = |name: &str, args: Value| core.studio_display.execute(&core, name, &args, false).unwrap()["result"].clone();
        let star = display("element_create", json!({"name":"Star"}))["ids"][0].clone();
        display("element_color", json!({"id":star,"mode":"single","color":"#ffffff"}));
        let tree = display("element_template", json!({"kind":"megatree","name":"Tree","count":4,"pixels":5,"color":"rgb"}))["id"].clone();
        let run = |name: &str, args: Value| execute(&core, name, &args, true).unwrap();
        let id = run("sequence_create", json!({"lengthMs":10_000}))["sequenceId"].clone();
        let type_of = |n: &str| vixen_effect_registry::all().iter().find(|d| d.name == n).unwrap().type_id.to_string();
        let add = |effect: &str, target: &Value, params: Value| run("effect_add", json!({"sequenceId":id,"typeId":type_of(effect),"targetId":target,"startMs":0,"durationMs":1000,"params":params}));

        let meteors = add("Meteors", &star, json!({}));
        assert_eq!(meteors["unlitCount"], 1);
        let w = &meteors["unlitTargets"][0];
        assert_eq!((w["reason"].as_str(), w["targetName"].as_str(), w["effect"].as_str()), (Some("oneChannel"), Some("Star"), Some("Meteors")));
        assert_eq!(w["effectIds"][0], meteors["effectId"]);

        let red = add("Pulse", &star, json!({"ColorGradient":["#ff0000"]}));
        assert_eq!(red["unlitTargets"][0]["reason"], "colors");
        assert!(red["unlitTargets"][0]["detail"].as_str().unwrap().contains("#ffffff"));
        for clean in [add("Pulse", &star, json!({"ColorGradient":["#ffffff"]})), add("Chase", &star, json!({})), add("Meteors", &tree, json!({}))] {
            assert!(clean.get("unlitTargets").is_none(), "{clean}");
        }

        let wipe = add("Wipe", &tree, json!({}));
        let w = &wipe["unlitTargets"][0];
        assert_eq!((w["reason"].as_str(), w["lights"].as_u64(), w["darkLights"].as_u64()), (Some("noLocation"), Some(20), Some(20)));

        // A batch groups repeats of one problem: one entry, every effect counted.
        let ops: Vec<Value> = (0..3).map(|i| json!({"op":"add","typeId":type_of("Bars"),"targetId":star,"startMs":i * 1000,"durationMs":500})).collect();
        let batch = run("effects_batch", json!({"sequenceId":id,"ops":ops}));
        assert_eq!(batch["unlitCount"], 3);
        assert_eq!(batch["unlitTargets"].as_array().unwrap().len(), 1);
        assert_eq!(batch["unlitTargets"][0]["effects"], 3);

        // effects_on_marks takes the same check.
        run("marks_create", json!({"sequenceId":id,"name":"Hits","timesMs":[100,2000]}));
        let marks = run("effects_on_marks", json!({"sequenceId":id,"markCollection":"Hits","duration":300,"template":{"typeId":type_of("Fire"),"targetId":star}}));
        assert_eq!(marks["unlitCount"], 2, "{marks}");
        core.studio_display.stop();
        drop(core);
        let _ = std::fs::remove_dir_all(root);
    }
}
