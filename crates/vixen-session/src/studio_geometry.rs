//! Preview geometry for agents: where each linked light sits in a preview, and for megatrees, arches and matrices
//! which way the prop runs (Px1 end, S1 position, numbering direction), read from the same shapes the preview draws.
//!
//! Positions are preview coordinates at zoom 1, the same space as preview_frame's PNG at scale 1. `string` and
//! `pixel` count from 1 in element order: `string` is the string element's position in the prop's group (as
//! effect_add's targetSlice counts), `pixel` the light's position in its string, so S1 Px1 is string 1 pixel 1.
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;
use vixen_core::{ElementTree, NodeId};
use vixen_io::SystemData;
use vixen_preview::{Shape, ShapeKind};

/// How megatree angles are measured, repeated in every megatree summary so the numbers can't be misread.
const ANGLES: &str = "Degrees around the trunk seen from above: 0 = front (nearest the viewer, lowest on the preview's base ellipse), 90 = viewer's right, 180 = back, 270 = viewer's left.";

struct Light {
    node: NodeId,
    x: f32,
    y: f32,
    /// Index of the string shape this light belongs to, for props built from strings.
    shape_string: Option<usize>,
    /// Fraction from the string's base end to its top end, for megatrees.
    along: Option<f32>,
}

struct Prop {
    preview_id: Uuid,
    preview_name: String,
    prop_id: u32,
    name: String,
    kind: String,
    unlinked: usize,
    lights: Vec<Light>,
    /// Per light: (string number, pixel number), both from 1, in element order.
    logical: Vec<(Option<usize>, Option<usize>)>,
    element: Option<NodeId>,
    summary: Value,
    angles: Vec<Option<f64>>,
}

pub fn tool() -> Value {
    let string = json!({"type":"string","minLength":1,"maxLength":64});
    json!({"name":"display_geometry","description":"Where each linked light sits in a preview, and which way each prop runs: per light its preview (x, y), 0-100 xPct/yPct within the prop (y from the bottom), string and pixel numbers (from 1, element order: S1 Px1 = string 1 pixel 1); megatrees add heightPct along the string and angleDeg around the trunk, arches positionPct. Each prop's summary gives its type, size, where Px1 sits, S1's angle and position (megatree), and the numbering direction. Filter by previewId and/or elementId (a group or string: only its lights). pixels:false returns summaries only. The same summaries are the display://summary resource.","inputSchema":{"type":"object","properties":{"previewId":string,"elementId":string,"pixels":{"type":"boolean"},"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":5000}},"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false,"openWorldHint":false}})
}

pub fn execute(system: &SystemData, revision: u64, a: &Value) -> Result<Value, String> {
    let obj = a.as_object().ok_or("arguments must be an object")?;
    if let Some(extra) = obj.keys().find(|k| !["previewId", "elementId", "pixels", "offset", "limit"].contains(&k.as_str())) {
        return Err(format!("Unknown argument {extra}"));
    }
    let id = |key: &str| -> Result<Option<Uuid>, String> {
        a.get(key).map(|v| v.as_str().and_then(|s| Uuid::parse_str(s).ok()).ok_or(format!("{key} must be a UUID"))).transpose()
    };
    let (preview, element) = (id("previewId")?, id("elementId")?);
    let pixels = match a.get("pixels") {
        None => true,
        Some(v) => v.as_bool().ok_or("pixels must be true or false")?,
    };
    let number = |key: &str, default: u64| a.get(key).map_or(Some(default), Value::as_u64).ok_or(format!("{key} must be a nonnegative integer"));
    let offset = number("offset", 0)? as usize;
    let limit = number("limit", 2000)? as usize;
    if !(1..=5000).contains(&limit) {
        return Err("limit must be 1..5000".into());
    }
    let tree = &system.tree;
    let only = match element {
        None => None,
        Some(e) => {
            let node = tree.find(&e).ok_or("elementId not found")?;
            let mut set: HashSet<NodeId> = tree.leaves(node).into_iter().collect();
            set.insert(node);
            Some(set)
        }
    };
    let entries = crate::preview::preview_entries(system);
    if let Some(p) = preview {
        if !entries.iter().any(|e| e.id == p) {
            return Err("Preview not found".into());
        }
    }
    let props = collect(system, preview, only.as_ref());
    let mut summaries = Vec::new();
    let mut rows = Vec::new();
    for prop in &props {
        summaries.push(json!({"previewId":prop.preview_id.to_string(),"previewName":prop.preview_name,"propId":prop.prop_id,"name":prop.name,"kind":prop.kind,"elementId":prop.element.map(|n|tree.node(n).id.to_string()),"elementName":prop.element.map(|n|tree.node(n).name.clone()),"lights":prop.lights.len(),"unlinkedLights":prop.unlinked,"summary":prop.summary}));
        if !pixels {
            continue;
        }
        let (min_x, max_x, min_y, max_y) = bounds(&prop.lights);
        let pct = |v: f32, lo: f32, hi: f32| if hi > lo { round1(100.0 * (v - lo) / (hi - lo)) } else { 0.0 };
        let arch = prop.summary["type"] == "arch";
        let count = prop.lights.len();
        for (i, light) in prop.lights.iter().enumerate() {
            let node = tree.node(light.node);
            let (string, pixel) = prop.logical[i];
            let mut row = json!({"nodeId":node.id.to_string(),"name":node.name,"propId":prop.prop_id,"x":round1(light.x),"y":round1(light.y),"xPct":pct(light.x,min_x,max_x),"yPct":pct(max_y-light.y,0.0,max_y-min_y)});
            if let Some(s) = string {
                row["string"] = json!(s);
            }
            if let Some(p) = pixel {
                row["pixel"] = json!(p);
                if arch && count > 1 {
                    row["positionPct"] = json!(round1(100.0 * (p - 1) as f32 / (count - 1) as f32));
                }
            }
            if let Some(f) = light.along {
                row["heightPct"] = json!(round1(100.0 * f));
            }
            if let Some(angle) = prop.angles[i] {
                row["angleDeg"] = json!(round1(angle as f32));
            }
            rows.push(row);
        }
    }
    let mut out = json!({"revision":revision,"props":summaries});
    if pixels {
        let total = rows.len();
        out["pixels"] = json!(rows.into_iter().skip(offset).take(limit).collect::<Vec<_>>());
        out["total"] = json!(total);
        out["nextOffset"] = json!(if offset.saturating_add(limit) < total { Some(offset + limit) } else { None });
    }
    Ok(out)
}

/// The `targetRegion` schema: effect_add, effect_band and effects_batch take it in place of targetId (B81).
pub fn region_schema() -> Value {
    let range = |lo: f64, hi: f64, example: Value| json!({"type":"array","items":{"type":"number","minimum":lo,"maximum":hi},"minItems":2,"maxItems":2,"examples":[example]});
    json!({"type":"object","description":"Lights picked by where they sit in a preview, as display_geometry measures them: elementId (a prop's group, or any element) and any of heightPct (megatrees: along the strings from the base; other props: yPct), angleDeg (megatrees: around the trunk, 0 front, 90 viewer's right, 180 back, 270 viewer's left; [300,60] wraps through the front), xPct and yPct (0-100 within the prop, y from the bottom), all inclusive. Whole strings (and whole props) that match are targeted as one element; partly matched strings give one effect per light, unless select is lights (always per light) or strings (strings with any matching light)","properties":{"elementId":{"type":"string","minLength":1,"maxLength":256},"previewId":{"type":"string","minLength":1,"maxLength":256,"description":"Needed when elementId's lights are linked in more than one preview"},"heightPct":range(0.0,100.0,json!([60,100])),"angleDeg":range(0.0,360.0,json!([315,45])),"xPct":range(0.0,100.0,json!([0,50])),"yPct":range(0.0,100.0,json!([0,30])),"select":{"type":"string","enum":["auto","lights","strings"]}},"required":["elementId"],"additionalProperties":false,"examples":[{"elementId":"<megatree UUID>","angleDeg":[315,45]},{"elementId":"<megatree UUID>","heightPct":[66,100],"angleDeg":[90,270]}]})
}

/// The nodes a `targetRegion` picks, in element order: whole elements where every light under them matches (unless
/// select is lights), otherwise single lights; select strings takes every string with a matching light.
pub fn region(system: &SystemData, spec: &Value) -> Result<Vec<NodeId>, String> {
    const KEYS: [&str; 7] = ["elementId", "previewId", "heightPct", "angleDeg", "xPct", "yPct", "select"];
    let fields = spec.as_object().ok_or("targetRegion must be an object: {elementId, heightPct?, angleDeg?, xPct?, yPct?, previewId?, select?}")?;
    if let Some(extra) = fields.keys().find(|k| !KEYS.contains(&k.as_str())) {
        return Err(format!("{extra} does not apply to targetRegion"));
    }
    let id = |key: &str| -> Result<Option<Uuid>, String> {
        spec.get(key).map(|v| v.as_str().and_then(|s| Uuid::parse_str(s).ok()).ok_or(format!("targetRegion {key} must be a UUID"))).transpose()
    };
    let element = id("elementId")?.ok_or("targetRegion needs elementId, the prop's group (or any element)")?;
    let preview = id("previewId")?;
    let range = |key: &str, max: f64| -> Result<Option<(f64, f64)>, String> {
        let Some(v) = spec.get(key) else { return Ok(None) };
        match v.as_array().map(|r| r.iter().map(Value::as_f64).collect::<Vec<_>>()).as_deref() {
            Some([Some(a), Some(b)]) if (0.0..=max).contains(a) && (0.0..=max).contains(b) => Ok(Some((*a, *b))),
            _ => Err(format!("targetRegion {key} must be [from, to], each 0..{max}")),
        }
    };
    let (height, angle, x, y) = (range("heightPct", 100.0)?, range("angleDeg", 360.0)?, range("xPct", 100.0)?, range("yPct", 100.0)?);
    for (key, r) in [("heightPct", height), ("xPct", x), ("yPct", y)] {
        if r.is_some_and(|(a, b)| a > b) {
            return Err(format!("targetRegion {key} must be [from, to] with from at most to"));
        }
    }
    let select = spec.get("select").map_or(Some("auto"), Value::as_str).filter(|s| ["auto", "lights", "strings"].contains(s)).ok_or("targetRegion select must be auto, lights or strings")?;
    let tree = &system.tree;
    let root = tree.find(&element).ok_or("targetRegion elementId not found")?;
    let under: HashSet<NodeId> = tree.distinct_leaves(root).into_iter().collect();
    if let Some(p) = preview {
        if !crate::preview::preview_entries(system).iter().any(|e| e.id == p) {
            return Err("targetRegion previewId: preview not found".into());
        }
    }
    // Percentages are measured on the whole prop, so a region on one string still means the prop's height.
    let props: Vec<Prop> = collect(system, preview, None).into_iter().filter(|p| p.lights.iter().any(|l| under.contains(&l.node))).collect();
    let previews: HashSet<Uuid> = props.iter().map(|p| p.preview_id).collect();
    if previews.is_empty() {
        return Err("targetRegion: elementId has no lights linked in a preview, so it has no geometry (link it with preview_prop_link or element_template previewId)".into());
    }
    if previews.len() > 1 {
        let names: Vec<&str> = props.iter().filter(|p| previews.contains(&p.preview_id)).map(|p| p.preview_name.as_str()).collect::<std::collections::BTreeSet<_>>().into_iter().collect();
        return Err(format!("targetRegion: elementId is linked in more than one preview ({}); give previewId", names.join(", ")));
    }
    let within = |v: f64, (a, b): (f64, f64)| v >= a - 0.05 && v <= b + 0.05;
    let around = |v: f64, (a, b): (f64, f64)| if a <= b { within(v, (a, b)) } else { v >= a - 0.05 || v <= b + 0.05 };
    let (mut matched, mut linked) = (HashSet::new(), HashSet::new());
    let mut angle_missing = false;
    for prop in &props {
        let (min_x, max_x, min_y, max_y) = bounds(&prop.lights);
        let pct = |v: f32, lo: f32, hi: f32| if hi > lo { f64::from(100.0 * (v - lo) / (hi - lo)) } else { 0.0 };
        for (i, light) in prop.lights.iter().enumerate() {
            if !under.contains(&light.node) {
                continue;
            }
            linked.insert(light.node);
            let (lx, ly) = (pct(light.x, min_x, max_x), pct(max_y - light.y, 0.0, max_y - min_y));
            let h = light.along.map_or(ly, |f| f64::from(100.0 * f));
            let a = prop.angles[i];
            angle_missing |= angle.is_some() && a.is_none();
            let ok = height.is_none_or(|r| within(h, r))
                && x.is_none_or(|r| within(lx, r))
                && y.is_none_or(|r| within(ly, r))
                && angle.is_none_or(|r| a.is_some_and(|a| around(a, r)));
            if ok {
                matched.insert(light.node);
            }
        }
    }
    if matched.is_empty() {
        return Err(if angle_missing && angle.is_some() {
            "targetRegion angleDeg applies to megatrees only (other props have no angle); use xPct for left/right".into()
        } else {
            "targetRegion picks no lights: widen the ranges (display_geometry shows each light's heightPct, angleDeg, xPct and yPct)".to_string()
        });
    }
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    match select {
        "lights" => out.extend(tree.distinct_leaves(root).into_iter().filter(|l| matched.contains(l))),
        "strings" => {
            let leaves: Vec<NodeId> = tree.distinct_leaves(root).into_iter().filter(|l| matched.contains(l)).collect();
            let parents = best_parents(tree, &leaves);
            for leaf in leaves {
                let string = parents.get(&leaf).copied().unwrap_or(leaf);
                if seen.insert(string) {
                    out.push(string);
                }
            }
        }
        _ => cover(tree, root, &matched, &linked, &mut seen, &mut out),
    }
    Ok(out)
}

/// Top-down: a node whose every linked light matched (and that has a linked light) is taken whole; otherwise its children.
fn cover(tree: &ElementTree, node: NodeId, matched: &HashSet<NodeId>, linked: &HashSet<NodeId>, seen: &mut HashSet<NodeId>, out: &mut Vec<NodeId>) {
    let leaves = tree.distinct_leaves(node);
    let lit: Vec<&NodeId> = leaves.iter().filter(|l| linked.contains(l)).collect();
    if lit.is_empty() || !lit.iter().any(|l| matched.contains(l)) {
        return;
    }
    // Whole only when every leaf is linked and matched: an unlinked light under it has no place, so it isn't lit.
    if leaves.iter().all(|l| matched.contains(l)) {
        if seen.insert(node) {
            out.push(node);
        }
        return;
    }
    for child in tree.node(node).children.clone() {
        cover(tree, child, matched, linked, seen, out);
    }
}

/// Every prop's summary, for the display://summary resource.
pub fn summary(system: &SystemData, revision: u64) -> Value {
    execute(system, revision, &json!({"pixels":false})).unwrap_or_else(|e| json!({"error":e}))
}

fn round1(v: f32) -> f32 {
    (v * 10.0).round() / 10.0
}

fn bounds(lights: &[Light]) -> (f32, f32, f32, f32) {
    lights.iter().fold((f32::MAX, f32::MIN, f32::MAX, f32::MIN), |(a, b, c, d), l| (a.min(l.x), b.max(l.x), c.min(l.y), d.max(l.y)))
}

fn collect(system: &SystemData, preview: Option<Uuid>, only: Option<&HashSet<NodeId>>) -> Vec<Prop> {
    let tree = &system.tree;
    let mut props = Vec::new();
    for entry in crate::preview::preview_entries(system) {
        if preview.is_some_and(|p| p != entry.id) {
            continue;
        }
        let Ok(data) = crate::preview::load_preview_data(&system.module_store, &entry) else { continue };
        for item in &data.display_items {
            let shape = &item.shape;
            let mut lights = Vec::new();
            let mut unlinked = 0;
            let mut push = |p: &vixen_preview::pixel::PreviewPixel, string: Option<usize>| match tree.find(&p.node_id) {
                Some(node) if only.is_none_or(|set| set.contains(&node)) => {
                    let (x, y) = p.draw_position();
                    lights.push(Light { node, x, y, shape_string: string, along: None });
                }
                Some(_) => {}
                None => unlinked += 1,
            };
            match shape.strings().filter(|s| !s.is_empty()) {
                Some(strings) if matches!(shape.kind(), ShapeKind::MegaTree { .. } | ShapeKind::PixelGrid { .. }) => {
                    for (i, s) in strings.iter().enumerate() {
                        s.for_each_pixel(&mut |p| push(p, Some(i)));
                    }
                }
                _ => shape.for_each_pixel(&mut |p| push(p, None)),
            }
            if lights.is_empty() {
                continue;
            }
            let (logical, element) = logical(tree, &lights);
            let mut prop = Prop {
                preview_id: entry.id,
                preview_name: entry.name.clone(),
                prop_id: item.id.0,
                name: shape.name().unwrap_or("").to_string(),
                kind: shape.type_name().to_string(),
                unlinked,
                angles: vec![None; lights.len()],
                lights,
                logical,
                element,
                summary: Value::Null,
            };
            prop.summary = match shape.kind() {
                ShapeKind::MegaTree { .. } => mega_tree(shape, &mut prop),
                ShapeKind::Arch => arch(&prop),
                ShapeKind::PixelGrid { string_orientation, .. } => matrix(&prop, &format!("{string_orientation:?}").to_lowercase()),
                _ => json!({"type":prop.kind.to_lowercase(),"lights":prop.lights.len()}),
            };
            props.push(prop);
        }
    }
    props
}

/// The parent of each node that holds the most of `nodes`, so a pixel also listed in an "all pixels" group still
/// resolves to its string.
fn best_parents(tree: &ElementTree, nodes: &[NodeId]) -> HashMap<NodeId, NodeId> {
    let mut held: HashMap<NodeId, usize> = HashMap::new();
    let distinct: HashSet<NodeId> = nodes.iter().copied().collect();
    for n in &distinct {
        for p in &tree.node(*n).parents {
            *held.entry(*p).or_default() += 1;
        }
    }
    distinct
        .iter()
        .filter_map(|n| tree.node(*n).parents.iter().copied().max_by_key(|p| (held[p], std::cmp::Reverse(p.0))).map(|p| (*n, p)))
        .collect()
}

/// (string, pixel) per light in element order, and the element the prop shows: the group of strings, or the one
/// string when every light shares a parent.
fn logical(tree: &ElementTree, lights: &[Light]) -> (Vec<(Option<usize>, Option<usize>)>, Option<NodeId>) {
    let leaves: Vec<NodeId> = lights.iter().map(|l| l.node).collect();
    let strings = best_parents(tree, &leaves);
    let string_nodes: Vec<NodeId> = {
        let mut seen = HashSet::new();
        leaves.iter().filter_map(|l| strings.get(l).copied()).filter(|s| seen.insert(*s)).collect()
    };
    let groups = if string_nodes.len() > 1 { best_parents(tree, &string_nodes) } else { HashMap::new() };
    let group = string_nodes.first().and_then(|s| groups.get(s).copied()).filter(|g| string_nodes.iter().all(|s| groups.get(s) == Some(g)));
    let position = |parent: NodeId, child: NodeId| tree.node(parent).children.iter().position(|c| *c == child).map(|i| i + 1);
    let logical = leaves
        .iter()
        .map(|leaf| {
            let string = strings.get(leaf).copied();
            let pixel = string.and_then(|s| position(s, *leaf));
            let number = match (group, string) {
                (Some(g), Some(s)) => position(g, s),
                _ => None,
            };
            (number, pixel)
        })
        .collect();
    let element = group.or(if string_nodes.len() == 1 { string_nodes.first().copied() } else { None });
    (logical, element)
}

fn sector(angle: f64) -> &'static str {
    const NAMES: [&str; 8] = ["front", "front-right", "right", "back-right", "back", "back-left", "left", "front-left"];
    NAMES[((angle.rem_euclid(360.0) + 22.5) / 45.0) as usize % 8]
}

/// The angle `get_ellipse_points` gives each point of a megatree's base, in its own convention (0 = right of the
/// ellipse, 90 = bottom), as `layout_mega_tree` asks for them.
fn ellipse_angles(degrees: i32, offset: i32, total: f64) -> Vec<f64> {
    let degrees = degrees as f64;
    let increment = if degrees <= 180.0 { 180.0 / (total / 2.0 - 1.0) } else { 360.0 / total };
    if !increment.is_finite() || increment <= 0.0 {
        return Vec::new();
    }
    let (start, end) = (offset as f64, offset as f64 + degrees);
    (0..).map(|k| start + k as f64 * increment).take_while(|t| *t < end + increment / 10.0).take(8192).collect()
}

fn mega_tree(shape: &Shape, prop: &mut Prop) -> Value {
    let ShapeKind::MegaTree { string_count, lights_per_string, degrees, degree_offset, .. } = *shape.kind() else { return Value::Null };
    // Each string runs from the base ellipse (lower on the preview) to the top one.
    let mut ends: HashMap<usize, ((f32, f32), (f32, f32))> = HashMap::new();
    if let Some(strings) = shape.strings() {
        for (i, s) in strings.iter().enumerate() {
            let pixels = s.pixels_vec();
            if let (Some(a), Some(b)) = (pixels.first(), pixels.last()) {
                let (a, b) = (a.draw_position(), b.draw_position());
                ends.insert(i, if a.1 >= b.1 { (a, b) } else { (b, a) });
            }
        }
    }
    let total = ((360.0 / degrees.max(1) as f64) * string_count as f64).ceil();
    let ellipse = ellipse_angles(degrees, degree_offset, total);
    for (i, light) in prop.lights.iter_mut().enumerate() {
        let Some(s) = light.shape_string else { continue };
        if let Some(((bx, by), (tx, ty))) = ends.get(&s).copied() {
            let (dx, dy) = (tx - bx, ty - by);
            let length = dx * dx + dy * dy;
            light.along = Some(if length > 0.0 { (((light.x - bx) * dx + (light.y - by) * dy) / length).clamp(0.0, 1.0) } else { 0.0 });
        }
        // layout_mega_tree puts string shape s on base point count-1-s.
        prop.angles[i] = (string_count as usize).checked_sub(1 + s).and_then(|k| ellipse.get(k)).map(|t| (90.0 - t).rem_euclid(360.0));
    }
    let mut summary = json!({"type":"megatree","strings":string_count,"pixelsPerString":lights_per_string,"degrees":degrees,"angleConvention":ANGLES});
    let first = |string: usize| (0..prop.lights.len()).find(|i| prop.logical[*i].0 == Some(string));
    if let Some(i) = (0..prop.lights.len()).find(|i| prop.logical[*i] == (Some(1), Some(1))).or_else(|| (0..prop.lights.len()).find(|i| prop.logical[*i].1 == Some(1))) {
        if let Some(f) = prop.lights[i].along {
            summary["px1"] = json!(if f < 0.5 { "bottom" } else { "top" });
        }
    }
    let angle_of = |string: usize| first(string).and_then(|i| prop.angles[i]);
    if let Some(a1) = angle_of(1) {
        summary["s1AngleDeg"] = json!(round1(a1 as f32));
        summary["s1Position"] = json!(sector(a1));
        if let Some(a2) = angle_of(2) {
            let step = (a2 - a1 + 540.0).rem_euclid(360.0) - 180.0;
            if step.abs() > 1e-6 {
                // Toward the viewer's right means south to east seen from above: counter-clockwise.
                summary["direction"] = json!(if step > 0.0 { "ccw" } else { "cw" });
                summary["directionNote"] = json!(format!("Seen from above, string numbers run {} ({}{:.1}° per string), so from the front S2 is toward the viewer's {}.", if step > 0.0 { "counter-clockwise" } else { "clockwise" }, if step > 0.0 { "+" } else { "" }, step, if step > 0.0 { "right" } else { "left" }));
            }
        }
    }
    summary
}

fn arch(prop: &Prop) -> Value {
    let mut summary = json!({"type":"arch","lights":prop.lights.len()});
    let at = |n: usize| (0..prop.lights.len()).find(|i| prop.logical[*i].1 == Some(n)).map(|i| &prop.lights[i]);
    if let (Some(a), Some(b)) = (at(1), at(prop.lights.len())) {
        summary["px1"] = json!(if a.x <= b.x { "left" } else { "right" });
    }
    summary
}

fn direction(dx: f32, dy: f32) -> &'static str {
    if dx.abs() >= dy.abs() {
        if dx >= 0.0 { "right" } else { "left" }
    } else if dy > 0.0 {
        "down"
    } else {
        "up"
    }
}

fn matrix(prop: &Prop, orientation: &str) -> Value {
    let mut summary = json!({"type":"matrix","orientation":orientation,"lights":prop.lights.len()});
    let strings = prop.logical.iter().filter_map(|l| l.0).max();
    let per_string = prop.logical.iter().filter_map(|l| l.1).max();
    summary["strings"] = json!(strings);
    summary["pixelsPerString"] = json!(per_string);
    let find = |s: usize, p: usize| (0..prop.lights.len()).find(|i| prop.logical[*i] == (Some(s), Some(p))).map(|i| &prop.lights[i]);
    let (min_x, max_x, min_y, max_y) = bounds(&prop.lights);
    if let Some(a) = find(1, 1) {
        let vertical = if a.y - min_y > max_y - a.y { "bottom" } else { "top" };
        let horizontal = if a.x - min_x > max_x - a.x { "right" } else { "left" };
        summary["px1"] = json!(format!("{vertical}-{horizontal}"));
        if let Some(b) = per_string.and_then(|n| find(1, n)) {
            summary["stringRuns"] = json!(direction(b.x - a.x, b.y - a.y));
        }
        if let Some(c) = find(2, 1) {
            summary["nextString"] = json!(direction(c.x - a.x, c.y - a.y));
        }
    }
    summary
}
