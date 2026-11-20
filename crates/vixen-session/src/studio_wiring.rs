//! How a Megatree or Pixel Grid is wired, shared by the template sheet's diagram, its generator and the MCP
//! `element_template` tool, so what the diagram draws is what gets built.
//!
//! The drawing convention: strings side by side with S1 on the left and each string's Px1 at the bottom (a megatree laid
//! flat, a vertical grid's columns), or for a horizontal grid rows stacked with R1 at the bottom and Px1 (C1) on the
//! left. The start corner is where the wire enters on that drawing; "reverse every other string" makes each string after
//! the first run back the way the one before it came (zig-zag). Hidden pixels are the first ones at each string's Px1
//! end (a tree's lights tucked under its base): they're patched in their place on the wire but kept in a separate group,
//! so effects on the prop never light them.
use crate::setup::templates::{add_patching_order, generate_template, StartLocation, TemplateError, TemplateParams, TemplateResult};
use crate::setup::tree::{Nid, SetupTree};
use serde_json::{json, Value};

/// The most hidden pixels a string can have.
pub const MAX_HIDDEN: u32 = 1000;

/// What the diagram and the generator need from a template's fields. `None` for templates without strings of pixels, and
/// for a Megatree with Pixel Tree off (its strings are single lights).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wiring {
    /// Strings stacked as rows (a horizontal grid) rather than standing side by side.
    pub rows: bool,
    pub strings: u32,
    /// Visible pixels per string.
    pub pixels: u32,
    pub start: StartLocation,
    pub reverse_alternate: bool,
    pub string_prefix: String,
    pub pixel_prefix: String,
    pub name: String,
    pub megatree: bool,
}

pub fn wiring_of(params: &TemplateParams) -> Option<Wiring> {
    match params {
        TemplateParams::Megatree { name, strings, string_prefix, pixel: true, pixels, pixel_prefix, start, zig_zag } => Some(Wiring {
            rows: false,
            strings: *strings,
            pixels: *pixels,
            start: *start,
            reverse_alternate: *zig_zag,
            string_prefix: string_prefix.clone(),
            pixel_prefix: pixel_prefix.clone(),
            name: name.clone(),
            megatree: true,
        }),
        TemplateParams::PixelGrid { name, horizontal, rows, columns, first_prefix, second_prefix, start, zig_zag } => Some(Wiring {
            rows: *horizontal,
            strings: if *horizontal { *rows } else { *columns },
            pixels: if *horizontal { *columns } else { *rows },
            start: *start,
            reverse_alternate: *zig_zag,
            string_prefix: first_prefix.clone(),
            pixel_prefix: second_prefix.clone(),
            name: name.clone(),
            megatree: false,
        }),
        _ => None,
    }
}

/// The strings in wire order, as (index from 0, runs from its Px1 end).
pub fn sequence(w: &Wiring) -> Vec<(u32, bool)> {
    // Columns: left is S1 and bottom is Px1. Rows: bottom is R1 and left is Px1.
    let left = matches!(w.start, StartLocation::BottomLeft | StartLocation::TopLeft);
    let bottom = matches!(w.start, StartLocation::BottomLeft | StartLocation::BottomRight);
    let (first_is_s1, from_px1) = if w.rows { (bottom, left) } else { (left, bottom) };
    (0..w.strings)
        .map(|k| {
            let index = if first_is_s1 { k } else { w.strings - 1 - k };
            (index, from_px1 ^ (w.reverse_alternate && k % 2 == 1))
        })
        .collect()
}

/// Whether the wire order is plain tree order, which needs no Order property (vanilla's Bottom Left without zig-zag).
fn is_tree_order(w: &Wiring, hidden: u32) -> bool {
    hidden == 0 && sequence(w).iter().enumerate().all(|(k, (i, fwd))| *i == k as u32 && *fwd)
}

/// What `generate` builds: the template's result, plus the group holding the hidden pixels.
pub struct Built {
    pub result: TemplateResult,
    pub hidden_head: Option<Nid>,
}

/// Generates the template with Studio's wiring: the native generator makes the elements (Px1..PxN per string, with the
/// hidden ones counted in), then each leaf gets its place on the wire as its Order, and the hidden pixels move to
/// "<name> Hidden", one group per string, while the visible ones are numbered from Px1 again.
pub fn generate(tree: &mut SetupTree, params: &TemplateParams, hidden: u32) -> Result<Built, TemplateError> {
    let Some(w) = wiring_of(params) else {
        return Ok(Built { result: generate_template(tree, params)?, hidden_head: None });
    };
    let hidden = hidden.min(MAX_HIDDEN);
    let natural = match params.clone() {
        TemplateParams::Megatree { name, strings, string_prefix, pixel, pixels, pixel_prefix, .. } => TemplateParams::Megatree {
            name,
            strings,
            string_prefix,
            pixel,
            pixels: pixels + hidden,
            pixel_prefix,
            start: StartLocation::BottomLeft,
            zig_zag: false,
        },
        TemplateParams::PixelGrid { name, horizontal, rows, columns, first_prefix, second_prefix, .. } => TemplateParams::PixelGrid {
            name,
            horizontal,
            rows: if horizontal { rows } else { rows + hidden },
            columns: if horizontal { columns + hidden } else { columns },
            first_prefix,
            second_prefix,
            start: StartLocation::BottomLeft,
            zig_zag: false,
        },
        other => other,
    };
    let mut result = generate_template(tree, &natural)?;
    let Some(head) = result.head else { return Ok(Built { result, hidden_head: None }) };
    let strings = tree.node(head).children.clone();
    if !is_tree_order(&w, hidden) {
        let mut wire = Vec::new();
        for (index, from_px1) in sequence(&w) {
            let mut leaves = Vec::new();
            tree.collect_leaves(strings[index as usize], &mut leaves);
            if !from_px1 {
                leaves.reverse();
            }
            wire.extend(leaves);
        }
        add_patching_order(tree, &wire);
    }
    if hidden == 0 {
        return Ok(Built { result, hidden_head: None });
    }
    let head_name = tree.node(head).name.clone();
    let (hidden_head, effects) = tree.create_node(None, Some(&format!("{head_name} Hidden")), false, true);
    result.effects.merge(effects);
    result.created.push(hidden_head);
    for string in strings {
        let string_name = tree.node(string).name.clone();
        let leaves = tree.node(string).children.clone();
        let (group, effects) = tree.create_node(Some(hidden_head), Some(&format!("{string_name} Hidden")), false, true);
        result.effects.merge(effects);
        result.created.push(group);
        // Hidden ones first, so the visible ones can take the names they free.
        for (k, leaf) in leaves.iter().take(hidden as usize).enumerate() {
            result.effects.merge(tree.move_node(*leaf, Some(group), Some(string), None));
            tree.rename_node(*leaf, &format!("{string_name} Hidden {}", k + 1));
        }
        for (k, leaf) in leaves.iter().skip(hidden as usize).enumerate() {
            tree.rename_node(*leaf, &format!("{string_name} {}{}", w.pixel_prefix, k + 1));
        }
    }
    Ok(Built { result, hidden_head: Some(hidden_head) })
}

fn corner_name(start: StartLocation) -> &'static str {
    match start {
        StartLocation::BottomLeft => "bottomLeft",
        StartLocation::BottomRight => "bottomRight",
        StartLocation::TopLeft => "topLeft",
        StartLocation::TopRight => "topRight",
    }
}

pub fn parse_corner(text: &str) -> Option<StartLocation> {
    Some(match text {
        "bottomLeft" => StartLocation::BottomLeft,
        "bottomRight" => StartLocation::BottomRight,
        "topLeft" => StartLocation::TopLeft,
        "topRight" => StartLocation::TopRight,
        _ => return None,
    })
}

/// The start-corner choices' labels: a megatree's name its strings' ends, a grid's are plain corners.
fn corner_labels(w: &Wiring) -> Value {
    let last = format!("{}{}", w.string_prefix, w.strings.max(1));
    let first = format!("{}1", w.string_prefix);
    if w.megatree {
        json!({
            "bottomLeft": format!("Bottom of {first}"),
            "topLeft": format!("Top of {first}"),
            "bottomRight": format!("Bottom of {last}"),
            "topRight": format!("Top of {last}"),
        })
    } else {
        json!({"bottomLeft":"Bottom Left","topLeft":"Top Left","bottomRight":"Bottom Right","topRight":"Top Right"})
    }
}

fn number(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The diagram and its sentences for a template's fields, as JSON. `applies:false` (with a `note`) when the template has no
/// strings of pixels to wire.
pub fn plan(params: &TemplateParams, hidden: u32) -> Value {
    let Some(w) = wiring_of(params) else {
        return match params {
            // Pixel Tree off: the wiring section stays visible but disabled, still naming its corners.
            TemplateParams::Megatree { strings, string_prefix, .. } => json!({
                "applies": false,
                "note": "Turn on Pixel Tree to choose how its pixels are wired.",
                "cornerLabels": corner_labels(&Wiring {
                    rows: false,
                    strings: *strings,
                    pixels: 0,
                    start: StartLocation::BottomLeft,
                    reverse_alternate: false,
                    string_prefix: string_prefix.clone(),
                    pixel_prefix: String::new(),
                    name: String::new(),
                    megatree: true,
                }),
            }),
            _ => json!({"applies":false,"note":""}),
        };
    };
    let hidden = hidden.min(MAX_HIDDEN);
    let seq = sequence(&w);
    let label = |i: u32| format!("{}{}", w.string_prefix, i + 1);
    let (along_fwd, along_back, px1_end) = if w.rows { ("right", "left", "left") } else { ("up", "down", "bottom") };
    let mut summary = Vec::new();
    if let Some(&(first, from_px1)) = seq.first() {
        let end = match (w.rows, from_px1) {
            (false, true) => "the bottom",
            (false, false) => "the top",
            (true, true) => "the left end",
            (true, false) => "the right end",
        };
        let pixel = match (from_px1, hidden > 0) {
            (true, true) => "Hidden 1".to_string(),
            (true, false) => format!("{}1", w.pixel_prefix),
            (false, _) => format!("{}{}", w.pixel_prefix, w.pixels),
        };
        summary.push(format!("The wire starts at {end} of {} ({} {pixel}).", label(first), label(first)));
        let dir = |fwd: bool| if fwd { along_fwd } else { along_back };
        if seq.len() > 1 {
            if w.reverse_alternate {
                let steps: Vec<String> = seq.iter().take(3).map(|(i, f)| format!("{} {}", dir(*f), label(*i))).collect();
                summary.push(format!("Every other string is reversed, so the wire snakes: {}, …", steps.join(", ")));
            } else {
                let back = if w.rows { if from_px1 { "left" } else { "right" } } else if from_px1 { "bottom" } else { "top" };
                summary.push(format!(
                    "Every string runs {} the same way, so the wire goes back to the {back} before {}.",
                    dir(from_px1),
                    label(seq[1].0)
                ));
            }
        }
    }
    let name = if w.name.is_empty() { "the prop".to_string() } else { format!("“{}”", w.name) };
    let hidden_text = (hidden > 0).then(|| {
        format!(
            "The {hidden} pixel{} at the {px1_end} of every string stay dark: they go in “{} Hidden”, not in {name}, but keep their place on the wire. Patch both groups together.",
            if hidden == 1 { "" } else { "s" },
            w.name
        )
    });
    let per = u64::from(w.pixels + hidden);
    let total = per * u64::from(w.strings);
    let totals = if hidden > 0 {
        format!(
            "{} strings × {} pixels = {} pixels ({} hidden)",
            w.strings,
            per,
            number(total),
            number(u64::from(hidden) * u64::from(w.strings))
        )
    } else {
        format!("{} strings × {} pixels = {} pixels", w.strings, per, number(total))
    };
    json!({
        "applies": true,
        "layout": if w.rows { "rows" } else { "columns" },
        "strings": w.strings,
        "pixels": w.pixels,
        "hidden": hidden,
        "start": corner_name(w.start),
        "reverseEveryOtherString": w.reverse_alternate,
        "sequence": seq.iter().map(|(i, f)| json!([i, f])).collect::<Vec<_>>(),
        "firstLabel": label(0),
        "lastLabel": label(w.strings.saturating_sub(1)),
        "cornerLabels": corner_labels(&w),
        "summary": summary.join(" "),
        "hiddenText": hidden_text,
        "totals": totals,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use vixen_core::element::{property_ids, PropertyData};

    fn tree(strings: u32, pixels: u32, start: StartLocation, zig_zag: bool) -> TemplateParams {
        TemplateParams::Megatree {
            name: "Tree".into(),
            strings,
            string_prefix: "S".into(),
            pixel: true,
            pixels,
            pixel_prefix: "Px".into(),
            start,
            zig_zag,
        }
    }

    fn order(t: &SetupTree, n: Nid) -> Option<i32> {
        t.node(n).properties.iter().find(|p| p.type_id == property_ids::ORDER).map(|p| match p.data {
            PropertyData::Order { order } => order,
            _ => 0,
        })
    }

    fn leaves(t: &SetupTree, n: Nid) -> Vec<Nid> {
        let mut out = Vec::new();
        t.collect_leaves(n, &mut out);
        out
    }

    #[test]
    fn corners_name_where_the_wire_enters() {
        use StartLocation::*;
        let w = |rows, start, rev| Wiring {
            rows,
            strings: 3,
            pixels: 2,
            start,
            reverse_alternate: rev,
            string_prefix: "S".into(),
            pixel_prefix: "Px".into(),
            name: "T".into(),
            megatree: !rows,
        };
        assert_eq!(sequence(&w(false, BottomLeft, false)), [(0, true), (1, true), (2, true)]);
        assert_eq!(sequence(&w(false, BottomLeft, true)), [(0, true), (1, false), (2, true)]);
        assert_eq!(sequence(&w(false, TopLeft, true)), [(0, false), (1, true), (2, false)]);
        assert_eq!(sequence(&w(false, BottomRight, false)), [(2, true), (1, true), (0, true)]);
        assert_eq!(sequence(&w(false, TopRight, false)), [(2, false), (1, false), (0, false)]);
        // Rows: bottom is R1, left is Px1. Vanilla's horizontal Bottom Right numbers like Top Right; here it starts at R1's right end.
        assert_eq!(sequence(&w(true, BottomRight, false)), [(0, false), (1, false), (2, false)]);
        assert_eq!(sequence(&w(true, TopLeft, false)), [(2, true), (1, true), (0, true)]);
    }

    #[test]
    fn bottom_left_matches_the_native_generator() {
        // Plain Bottom Left leaves tree order alone, and with reversing numbers exactly as vanilla's zig-zag.
        for zig in [false, true] {
            let mut native = SetupTree::new();
            let a = generate_template(&mut native, &tree(3, 4, StartLocation::BottomLeft, zig)).unwrap().head.unwrap();
            let mut studio = SetupTree::new();
            let b = generate(&mut studio, &tree(3, 4, StartLocation::BottomLeft, zig), 0).unwrap().result.head.unwrap();
            let x: Vec<_> = leaves(&native, a).iter().map(|n| order(&native, *n)).collect();
            let y: Vec<_> = leaves(&studio, b).iter().map(|n| order(&studio, *n)).collect();
            assert_eq!(x, y, "zig {zig}");
        }
    }

    #[test]
    fn zig_zag_tree_with_tucked_pixels() {
        // The issue's tree: 32 legs, reversed every other one, 7 hidden pixels at the bottom of each leg, 50 visible.
        let mut t = SetupTree::new();
        let built = generate(&mut t, &tree(32, 50, StartLocation::BottomLeft, true), 7).unwrap();
        let head = built.result.head.unwrap();
        let hidden = built.hidden_head.unwrap();
        assert_eq!(t.node(head).name, "Tree");
        assert_eq!(t.node(hidden).name, "Tree Hidden");
        assert_eq!(leaves(&t, head).len(), 32 * 50);
        assert_eq!(leaves(&t, hidden).len(), 32 * 7);
        let s1 = t.node(head).children[0];
        let s2 = t.node(head).children[1];
        assert_eq!(t.node(t.node(s1).children[0]).name, "Tree S1 Px1");
        assert_eq!(t.node(t.node(s1).children[49]).name, "Tree S1 Px50");
        let h1 = t.node(hidden).children[0];
        assert_eq!(t.node(h1).name, "Tree S1 Hidden");
        assert_eq!(t.node(t.node(h1).children[0]).name, "Tree S1 Hidden 1");
        // Leg 1 runs up: hidden 1-7, then Px1-Px50 as 8-57. Leg 2 runs down: Px50 is 58, its Px1 is 107, its hidden 108-114.
        assert_eq!(order(&t, t.node(h1).children[0]), Some(1));
        assert_eq!(order(&t, t.node(h1).children[6]), Some(7));
        assert_eq!(order(&t, t.node(s1).children[0]), Some(8));
        assert_eq!(order(&t, t.node(s1).children[49]), Some(57));
        let h2 = t.node(hidden).children[1];
        assert_eq!(order(&t, t.node(s2).children[49]), Some(58));
        assert_eq!(order(&t, t.node(s2).children[0]), Some(107));
        assert_eq!(order(&t, t.node(h2).children[6]), Some(108));
        assert_eq!(order(&t, t.node(h2).children[0]), Some(114));
        // Every light has a distinct place on the wire, 1 to 1,824.
        let mut all: Vec<i32> = leaves(&t, head).iter().chain(leaves(&t, hidden).iter()).map(|n| order(&t, *n).unwrap()).collect();
        all.sort();
        assert_eq!(all, (1..=32 * 57).collect::<Vec<_>>());
        // The hidden pixels are in nothing under the tree.
        assert!(leaves(&t, hidden).iter().all(|l| !t.node(*l).parents.iter().any(|p| t.node(head).children.contains(p))));
        assert!(built.result.created.contains(&hidden));
    }

    #[test]
    fn plan_describes_the_tree() {
        let p = plan(&tree(32, 50, StartLocation::BottomLeft, true), 7);
        assert_eq!(p["applies"], true);
        assert_eq!(p["sequence"].as_array().unwrap().len(), 32);
        assert_eq!(p["sequence"][1], json!([1, false]));
        assert_eq!(p["totals"], "32 strings × 57 pixels = 1,824 pixels (224 hidden)");
        let summary = p["summary"].as_str().unwrap();
        assert!(summary.starts_with("The wire starts at the bottom of S1 (S1 Hidden 1)."), "{summary}");
        assert!(summary.contains("snakes: up S1, down S2, up S3"), "{summary}");
        assert!(p["hiddenText"].as_str().unwrap().contains("7 pixels at the bottom"));
        assert_eq!(p["cornerLabels"]["topLeft"], "Top of S1");
        let off = TemplateParams::Megatree {
            name: "T".into(),
            strings: 3,
            string_prefix: "S".into(),
            pixel: false,
            pixels: 2,
            pixel_prefix: "P".into(),
            start: StartLocation::BottomLeft,
            zig_zag: false,
        };
        assert_eq!(plan(&off, 0)["applies"], false);
        assert_eq!(plan(&off, 0)["cornerLabels"]["bottomRight"], "Bottom of S3");
    }

    #[test]
    fn mcp_tree_with_hidden_pixels_patches_along_the_wire() {
        use crate::app::AppCore;
        let root = std::env::temp_dir().join(format!("studio-wiring-{}", uuid::Uuid::new_v4()));
        let core = AppCore::open(root.clone(), true).unwrap();
        let run = |name: &str, args: Value| core.studio_display.execute(&core, name, &args, true).unwrap()["result"].clone();
        let tree = run(
            "element_template",
            json!({"kind":"megatree","name":"Tree","count":32,"pixels":50,"reverseEveryOtherString":true,"hiddenPixels":7}),
        );
        assert_eq!(tree["wiring"]["totals"], "32 strings × 57 pixels = 1,824 pixels (224 hidden)");
        let controller = run(
            "controller_create",
            json!({"name":"C","typeId":vixen_output::ids::E131.to_string(),"outputs":2000}),
        )["id"]
            .clone();
        let patched = run("patch_elements", json!({"elementId":tree["id"],"alsoIds":[tree["hiddenId"]],"controllerId":controller}));
        assert_eq!(patched["patched"], 1824);
        assert_eq!(patched["wireOrder"], true);
        // Each output, in order, named by the light feeding it.
        let system = core.system.read();
        let controller = uuid::Uuid::parse_str(controller.as_str().unwrap()).unwrap();
        let names: std::collections::HashMap<uuid::Uuid, String> =
            system.tree.all_nodes().filter_map(|(_, n)| n.element_id.map(|e| (e, n.name.clone()))).collect();
        let fed: Vec<String> = system.controllers.iter().find(|c| c.id == controller).unwrap().outputs[..1824]
            .iter()
            .map(|o| {
                let patch = system.patches.iter().find(|p| p.component_id == o.id && p.source_id.is_some()).unwrap();
                names[&patch.source_id.unwrap()].clone()
            })
            .collect();
        assert_eq!(fed[0], "Tree String1 Hidden 1");
        assert_eq!(fed[6], "Tree String1 Hidden 7");
        assert_eq!(fed[7], "Tree String1 Pixel1");
        assert_eq!(fed[56], "Tree String1 Pixel50");
        assert_eq!(fed[57], "Tree String2 Pixel50");
        assert_eq!(fed[106], "Tree String2 Pixel1");
        assert_eq!(fed[107], "Tree String2 Hidden 7");
        assert_eq!(fed[113], "Tree String2 Hidden 1");
        assert_eq!(fed[114], "Tree String3 Hidden 1");
        assert_eq!(fed[1823], "Tree String32 Hidden 1");
        drop(system);
        drop(core);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn horizontal_grid_bottom_right_starts_on_r1() {
        let params = TemplateParams::PixelGrid {
            name: "G".into(),
            horizontal: true,
            rows: 2,
            columns: 3,
            first_prefix: "R".into(),
            second_prefix: "C".into(),
            start: StartLocation::BottomRight,
            zig_zag: false,
        };
        let mut t = SetupTree::new();
        let head = generate(&mut t, &params, 0).unwrap().result.head.unwrap();
        let orders: Vec<_> = leaves(&t, head).iter().map(|n| order(&t, *n).unwrap()).collect();
        assert_eq!(orders, [3, 2, 1, 6, 5, 4]);
    }
}
