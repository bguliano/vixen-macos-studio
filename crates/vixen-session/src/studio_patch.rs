//! Patching by intent: Reverse, Swap with… and Move to output… on any element or group, shared by the Display pane's
//! context menu and the MCP `patch_reverse` / `patch_swap` / `patch_move` / `patch_ranges` tools. Each works on the
//! controller outputs an element's lights already reach, so nobody has to work out output numbers by hand. A light (a
//! leaf) keeps its channels together and in order (an RGB pixel's R, G, B stay R, G, B); only whole lights move. Results
//! and the menu show where things are patched as universe:channel ranges (E1.31) or output numbers (other controllers).
use crate::setup::controllers::ControllerEntry;
use crate::setup::flow::{Cid, CompKind, SourceRef};
use crate::setup::tree::Nid;
use crate::setup::SetupSession;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;
use vixen_output::ControllerConfig;

/// How many swap candidates and free spots per controller the context menu lists.
const MENU_SWAP_LIMIT: usize = 40;
const MENU_MOVE_LIMIT: usize = 8;

/// One controller output: its controller slot, zero-based index and component.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct Position {
    slot: u32,
    index: u32,
    cid: Cid,
}

/// What an element's lights reach, in output order: every position, and the source each one carries.
struct Patched {
    leaves: HashSet<Nid>,
    positions: Vec<Position>,
    sources: Vec<SourceRef>,
    /// Each light's sources, in order of the light's first output.
    lights: Vec<Vec<SourceRef>>,
}

/// Every patched controller output, in controller order and then output order, with the light (leaf) feeding it.
fn patched_outputs(s: &SetupSession) -> Vec<(Position, Nid, SourceRef)> {
    let mut out = Vec::new();
    for &slot in &s.controller_order {
        let Some(c) = s.controllers.get(slot as usize).filter(|c| c.alive) else { continue };
        for (i, o) in c.outputs.iter().enumerate() {
            let Some(source) = s.flow.source(o.cid) else { continue };
            let root = s.flow.comp(s.flow.root_source(o.cid));
            if root.kind != CompKind::Element {
                continue;
            }
            if let Some(leaf) = s.tree.find_by_element(&root.id) {
                out.push((Position { slot, index: i as u32, cid: o.cid }, leaf, source));
            }
        }
    }
    out
}

/// Each patched light once, in the order of its first output (controllers in order, then outputs), optionally on one
/// controller slot only. Commissioning tests walk lights in this order, so a moving pixel follows the wiring.
pub(crate) fn lights_in_output_order(s: &SetupSession, slot: Option<u32>) -> Vec<Nid> {
    let mut seen = HashSet::new();
    patched_outputs(s)
        .into_iter()
        .filter(|(p, _, _)| slot.is_none_or(|slot| p.slot == slot))
        .filter_map(|(_, leaf, _)| seen.insert(leaf).then_some(leaf))
        .collect()
}

fn patched(s: &SetupSession, node: Nid, all: &[(Position, Nid, SourceRef)]) -> Patched {
    let leaves: HashSet<Nid> = s.tree.distinct_leaves([node]).into_iter().collect();
    let mut positions = Vec::new();
    let mut sources = Vec::new();
    let mut order: Vec<Nid> = Vec::new();
    let mut by_light: HashMap<Nid, Vec<SourceRef>> = HashMap::new();
    for (p, leaf, source) in all {
        if !leaves.contains(leaf) {
            continue;
        }
        positions.push(*p);
        sources.push(*source);
        by_light
            .entry(*leaf)
            .or_insert_with(|| {
                order.push(*leaf);
                Vec::new()
            })
            .push(*source);
    }
    let lights = order.into_iter().map(|l| by_light.remove(&l).unwrap_or_default()).collect();
    Patched { leaves, positions, sources, lights }
}

/// An output's address: universe:channel within an E1.31 universe, otherwise its one-based output number.
fn address(c: &ControllerEntry, index: u32) -> String {
    if let Some(ControllerConfig::E131(config)) = &c.config {
        let i = index as i64;
        let found = config.universes.as_deref().unwrap_or(&[]).iter().find(|u| u.active && i >= u.start as i64 && i < u.start as i64 + u.size as i64);
        if let Some(u) = found {
            return format!("{}:{}", u.universe, i - u.start as i64 + 1);
        }
    }
    format!("{}", index + 1)
}

/// Contiguous runs of positions, each with its controller, one-based outputs and universe:channel ends.
fn ranges(s: &SetupSession, positions: &[Position]) -> Vec<Value> {
    let mut sorted = positions.to_vec();
    let rank: HashMap<u32, usize> = s.controller_order.iter().enumerate().map(|(r, slot)| (*slot, r)).collect();
    sorted.sort_by_key(|p| (rank.get(&p.slot).copied().unwrap_or(usize::MAX), p.index));
    let mut runs: Vec<(u32, u32, u32)> = Vec::new();
    for p in sorted {
        match runs.last_mut() {
            Some((slot, _, last)) if *slot == p.slot && *last + 1 == p.index => *last = p.index,
            _ => runs.push((p.slot, p.index, p.index)),
        }
    }
    runs.into_iter().map(|(slot, first, last)| range(&s.controllers[slot as usize], first, last)).collect()
}

fn range(c: &ControllerEntry, first: u32, last: u32) -> Value {
    let (from, to) = (address(c, first), address(c, last));
    let outputs = if first == last { format!("output {}", first + 1) } else { format!("outputs {}–{}", first + 1, last + 1) };
    let span = if first == last { from.clone() } else { format!("{from}–{to}") };
    let text = if from.contains(':') { format!("{} {span} ({outputs})", c.name) } else { format!("{} {outputs}", c.name) };
    json!({"controllerId":c.id.to_string(),"controller":c.name,"firstOutput":first + 1,"lastOutput":last + 1,"count":last - first + 1,"from":from,"to":to,"text":text})
}

fn summary(s: &SetupSession, node: Nid, p: &Patched) -> Value {
    let ranges = ranges(s, &p.positions);
    let text = if ranges.is_empty() {
        "Not patched".to_string()
    } else {
        ranges.iter().filter_map(|r| r["text"].as_str()).collect::<Vec<_>>().join(", ")
    };
    json!({"id":s.tree.node(node).id.to_string(),"name":s.tree.node(node).name,"lights":p.lights.len(),"outputs":p.positions.len(),"ranges":ranges,"text":text})
}

fn node_arg(s: &SetupSession, a: &Value, key: &str) -> Result<Nid, String> {
    let id = a[key].as_str().and_then(|t| Uuid::parse_str(t).ok()).ok_or(format!("{key} must be an element UUID"))?;
    s.tree.find(&id).ok_or_else(|| "Element does not exist".to_string())
}

fn changed(s: &mut SetupSession) {
    s.patching_changed();
    s.bump_graph();
}

/// Every patched run, for plugins' `display.snapshot`: consecutive outputs of one controller fed by the same light, in
/// controller order and then output order, as `{elementId, controllerId, firstOutput, lastOutput, from, to}` with
/// one-based outputs and `from`/`to` as [`address`] gives them. An RGB pixel on three outputs is one run.
pub(crate) fn light_runs(s: &SetupSession) -> Vec<Value> {
    let mut runs: Vec<(u32, u32, u32, Nid)> = Vec::new();
    for (p, leaf, _) in patched_outputs(s) {
        match runs.last_mut() {
            Some((slot, _, last, light)) if *slot == p.slot && *last + 1 == p.index && *light == leaf => *last = p.index,
            _ => runs.push((p.slot, p.index, p.index, leaf)),
        }
    }
    runs.into_iter()
        .map(|(slot, first, last, leaf)| {
            let c = &s.controllers[slot as usize];
            json!({"elementId":s.tree.node(leaf).id.to_string(),"controllerId":c.id.to_string(),"firstOutput":first + 1,"lastOutput":last + 1,"from":address(c, first),"to":address(c, last)})
        })
        .collect()
}

/// The read-only `patch_ranges` tool.
pub fn ranges_tool() -> Value {
    json!({"name":"patch_ranges","description":"Where an element or group is patched: how many lights and outputs, and each contiguous run as controller, one-based outputs and universe:channel ends (E1.31; other controllers give output numbers). Use before patch_reverse, patch_swap or patch_move.","inputSchema":{"type":"object","properties":{"id":{"type":"string","minLength":1,"maxLength":64}},"required":["id"],"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false,"openWorldHint":false}})
}

/// `patch_ranges`: where an element or group is patched, as universe:channel ranges.
pub fn ranges_of(s: &SetupSession, a: &Value) -> Result<Value, String> {
    if let Some(extra) = a.as_object().ok_or("arguments must be an object")?.keys().find(|k| k.as_str() != "id") {
        return Err(format!("Unknown argument {extra}"));
    }
    let node = node_arg(s, a, "id")?;
    let all = patched_outputs(s);
    Ok(summary(s, node, &patched(s, node, &all)))
}

/// `patch_reverse`: the element's lights take its outputs in the opposite order, each light's channels still in order.
pub fn reverse(s: &mut SetupSession, a: &Value) -> Result<Value, String> {
    let node = node_arg(s, a, "id")?;
    let all = patched_outputs(s);
    let p = patched(s, node, &all);
    if p.lights.len() < 2 {
        return Err(if p.lights.is_empty() { "Nothing is patched to reverse".into() } else { "Only one light is patched; there is nothing to reverse".into() });
    }
    let order: Vec<SourceRef> = p.lights.iter().rev().flatten().copied().collect();
    for (position, source) in p.positions.iter().zip(order) {
        s.flow.set_source(position.cid, source);
    }
    changed(s);
    Ok(summary(s, node, &p))
}

/// `patch_swap`: two elements trade outputs. They must reach the same number of outputs and share no light.
pub fn swap(s: &mut SetupSession, a: &Value) -> Result<Value, String> {
    let first = node_arg(s, a, "id")?;
    let second = node_arg(s, a, "withId")?;
    let all = patched_outputs(s);
    let x = patched(s, first, &all);
    let y = patched(s, second, &all);
    let name = |n: Nid| s.tree.node(n).name.clone();
    if x.positions.is_empty() || y.positions.is_empty() {
        return Err(format!("{} is not patched", if x.positions.is_empty() { name(first) } else { name(second) }));
    }
    if !x.leaves.is_disjoint(&y.leaves) {
        return Err(format!("{} and {} share lights, so they can't swap", name(first), name(second)));
    }
    if x.positions.len() != y.positions.len() {
        return Err(format!(
            "{} uses {} outputs and {} uses {}; only elements using the same number of outputs can swap",
            name(first),
            x.positions.len(),
            name(second),
            y.positions.len()
        ));
    }
    for (position, source) in x.positions.iter().zip(&y.sources) {
        s.flow.set_source(position.cid, *source);
    }
    for (position, source) in y.positions.iter().zip(&x.sources) {
        s.flow.set_source(position.cid, *source);
    }
    changed(s);
    Ok(json!({"element":summary(s, first, &patched(s, first, &patched_outputs(s))),"with":summary(s, second, &patched(s, second, &patched_outputs(s)))}))
}

/// The outputs `start..start+len` of a controller, if they exist and hold nothing but `own` or nothing at all.
fn free_run(s: &SetupSession, slot: u32, start: u32, len: usize, own: &HashSet<Cid>) -> Result<Vec<Cid>, String> {
    let c = &s.controllers[slot as usize];
    let end = start as usize + len;
    if end > c.outputs.len() {
        return Err(format!("{} has {} outputs; {} outputs from output {} don't fit", c.name, c.outputs.len(), len, start + 1));
    }
    let run: Vec<Cid> = c.outputs[start as usize..end].iter().map(|o| o.cid).collect();
    if let Some((i, _)) = run.iter().enumerate().find(|(_, cid)| s.flow.source(**cid).is_some() && !own.contains(cid)) {
        let taken = start + i as u32;
        return Err(format!("{} {} is already patched to something else", c.name, range(c, taken, taken)["from"].as_str().unwrap_or("")));
    }
    Ok(run)
}

/// `patch_move`: the element's outputs move, in order, to a controller's outputs from `startOutput` (zero-based, as
/// `patch_elements` counts). The destination has to be unpatched, or the element's own outputs.
pub fn move_to(s: &mut SetupSession, a: &Value) -> Result<Value, String> {
    let node = node_arg(s, a, "id")?;
    let controller = a["controllerId"].as_str().and_then(|t| Uuid::parse_str(t).ok()).ok_or("controllerId must be a controller UUID")?;
    let slot = s.slot_of(controller).ok_or("Controller not found")?;
    let start = a["startOutput"].as_u64().and_then(|n| u32::try_from(n).ok()).ok_or("startOutput must be a nonnegative integer")?;
    let all = patched_outputs(s);
    let p = patched(s, node, &all);
    if p.positions.is_empty() {
        return Err("Nothing is patched to move; patch it first with patch_elements".into());
    }
    let own: HashSet<Cid> = p.positions.iter().map(|q| q.cid).collect();
    let run = free_run(s, slot, start, p.positions.len(), &own)?;
    if p.positions.iter().map(|q| q.cid).eq(run.iter().copied()) {
        return Err("It is already patched there".into());
    }
    for position in &p.positions {
        s.flow.reset_source(position.cid);
    }
    for (cid, source) in run.iter().zip(&p.sources) {
        s.flow.set_source(*cid, *source);
    }
    changed(s);
    let after = patched(s, node, &patched_outputs(s));
    Ok(summary(s, node, &after))
}

/// Patched-output counts per node, counting a light under two branches of one group twice; only for picking candidates.
fn counts(s: &SetupSession, all: &[(Position, Nid, SourceRef)]) -> Vec<usize> {
    let mut per_leaf = vec![0usize; s.tree.slots()];
    for (_, leaf, _) in all {
        per_leaf[leaf.0 as usize] += 1;
    }
    let mut memo: Vec<Option<usize>> = vec![None; s.tree.slots()];
    fn total(s: &SetupSession, n: Nid, per_leaf: &[usize], memo: &mut [Option<usize>], depth: usize) -> usize {
        if let Some(v) = memo[n.0 as usize] {
            return v;
        }
        let children = &s.tree.node(n).children;
        let v = if children.is_empty() || depth > 64 {
            per_leaf[n.0 as usize]
        } else {
            children.clone().into_iter().map(|c| total(s, c, per_leaf, memo, depth + 1)).sum()
        };
        memo[n.0 as usize] = Some(v);
        v
    }
    let nodes: Vec<Nid> = s.tree.live_nodes().map(|(n, _)| n).collect();
    for n in nodes {
        total(s, n, &per_leaf, &mut memo, 0);
    }
    memo.into_iter().map(|v| v.unwrap_or(0)).collect()
}

/// What the Display pane's Patch submenu shows for one element: where it's patched, the elements it can swap with
/// (same number of outputs, no shared lights; siblings first) and the free spots on each controller it fits.
pub fn menu(s: &SetupSession, id: Uuid) -> Option<Value> {
    let node = s.tree.find(&id)?;
    let all = patched_outputs(s);
    let p = patched(s, node, &all);
    let mut v = summary(s, node, &p);
    v["canReverse"] = json!(p.lights.len() >= 2);
    if p.positions.is_empty() {
        v["swapWith"] = json!([]);
        v["moveTo"] = json!([]);
        return Some(v);
    }
    let need = p.positions.len();
    let counts = counts(s, &all);
    let parents: HashSet<Nid> = s.tree.node(node).parents.iter().copied().collect();
    let mut candidates: Vec<(bool, Nid)> = s
        .tree
        .live_nodes()
        .filter(|(n, _)| *n != node && counts[n.0 as usize] == need)
        .map(|(n, t)| (t.parents.iter().any(|q| parents.contains(q)), n))
        .collect();
    // Siblings first, then tree order.
    candidates.sort_by_key(|(sibling, _)| !*sibling);
    let mut swap = Vec::new();
    for (_, n) in candidates {
        if swap.len() == MENU_SWAP_LIMIT {
            break;
        }
        let other = patched(s, n, &all);
        if other.positions.len() == need && other.leaves.is_disjoint(&p.leaves) {
            swap.push(summary(s, n, &other));
        }
    }
    v["swapWith"] = json!(swap);
    let own: HashSet<Cid> = p.positions.iter().map(|q| q.cid).collect();
    let mut move_to = Vec::new();
    for &slot in &s.controller_order {
        let Some(c) = s.controllers.get(slot as usize).filter(|c| c.alive) else { continue };
        let mut spots = Vec::new();
        let mut index = 0usize;
        while index < c.outputs.len() && spots.len() < MENU_MOVE_LIMIT {
            let free = |i: usize| s.flow.source(c.outputs[i].cid).is_none() || own.contains(&c.outputs[i].cid);
            if !free(index) {
                index += 1;
                continue;
            }
            let begin = index;
            while index < c.outputs.len() && free(index) {
                index += 1;
            }
            // Each free run's start, unless that's exactly where it already is.
            let here = p.positions.first().is_some_and(|q| q.slot == slot && q.index as usize == begin);
            if index - begin >= need && !here {
                let mut spot = range(c, begin as u32, (begin + need - 1) as u32);
                spot["startOutput"] = json!(begin);
                spots.push(spot);
            }
        }
        if !spots.is_empty() {
            move_to.push(json!({"controllerId":c.id.to_string(),"controller":c.name,"spots":spots}));
        }
    }
    v["moveTo"] = json!(move_to);
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setup::test_support::*;

    fn id_of(s: &SetupSession, name: &str) -> String {
        s.tree.node(s.tree.live_nodes().find(|(_, n)| n.name == name).unwrap().0).id.to_string()
    }

    /// Which light feeds each output of the first controller, by name, with the channel for RGB lights.
    fn wired(s: &SetupSession) -> Vec<Option<String>> {
        s.controllers[0]
            .outputs
            .iter()
            .map(|o| {
                let source = s.flow.source(o.cid)?;
                let root = s.flow.root_source(source.component);
                let name = s.tree.node(s.tree.find_by_element(&s.flow.comp(root).id).unwrap()).name.clone();
                Some(if s.flow.comp(source.component).kind == CompKind::Filter { format!("{name}{}", ["r", "g", "b"][source.output as usize]) } else { name })
            })
            .collect()
    }

    fn names(values: &[&str]) -> Vec<Option<String>> {
        values.iter().map(|v| (!v.is_empty()).then(|| v.to_string())).collect()
    }

    /// Lights A..F, A B C grouped as "Left" and D E F as "Right", each an RGB pixel, patched in order to 18 of 24 outputs.
    fn rig() -> SetupSession {
        let mut s = session(&["A", "B", "C", "D", "E", "F"], 24);
        for name in ["A", "B", "C", "D", "E", "F"] {
            let node = s.tree.live_nodes().find(|(_, n)| n.name == name).unwrap().1;
            let element = s.flow.find(&node.element_id.unwrap()).unwrap();
            add_breakdown(&mut s, element, 3);
        }
        let rows = |s: &SetupSession, names: &[&str]| -> Vec<String> {
            names.iter().map(|n| s.row_path_of(s.tree.live_nodes().find(|(_, x)| x.name == *n).unwrap().0)).collect()
        };
        let left = rows(&s, &["A", "B", "C"]);
        s.create_group(&left, "Left");
        let right = rows(&s, &["D", "E", "F"]);
        s.create_group(&right, "Right");
        let both = rows(&s, &["Left", "Right"]);
        let outputs = s.all_outputs_selection();
        assert_eq!(s.patch_elements(&both, &outputs, &Default::default(), false), 18);
        s
    }

    #[test]
    fn reverse_keeps_channels_in_order() {
        let mut s = rig();
        let left = id_of(&s, "Left");
        let result = reverse(&mut s, &json!({"id":left})).unwrap();
        assert_eq!(result["lights"], 3);
        assert_eq!(
            wired(&s)[..9].to_vec(),
            names(&["Cr", "Cg", "Cb", "Br", "Bg", "Bb", "Ar", "Ag", "Ab"])
        );
        // Right is untouched.
        assert_eq!(wired(&s)[9..12].to_vec(), names(&["Dr", "Dg", "Db"]));
        // Twice is back where it started.
        reverse(&mut s, &json!({"id":left})).unwrap();
        assert_eq!(wired(&s)[..3].to_vec(), names(&["Ar", "Ag", "Ab"]));
        let a = id_of(&s, "A");
        assert!(reverse(&mut s, &json!({"id":a})).unwrap_err().contains("Only one light"));
    }

    #[test]
    fn swap_trades_outputs_and_refuses_mismatches() {
        let mut s = rig();
        let (left, right) = (id_of(&s, "Left"), id_of(&s, "Right"));
        swap(&mut s, &json!({"id":left,"withId":right})).unwrap();
        assert_eq!(
            wired(&s)[..18].to_vec(),
            names(&["Dr", "Dg", "Db", "Er", "Eg", "Eb", "Fr", "Fg", "Fb", "Ar", "Ag", "Ab", "Br", "Bg", "Bb", "Cr", "Cg", "Cb"])
        );
        let a = id_of(&s, "A");
        assert!(swap(&mut s, &json!({"id":left,"withId":a})).unwrap_err().contains("share lights"));
        let d = id_of(&s, "D");
        assert!(swap(&mut s, &json!({"id":left,"withId":d})).unwrap_err().contains("same number of outputs"));
        // Two single lights swap.
        swap(&mut s, &json!({"id":a,"withId":d})).unwrap();
        assert_eq!(wired(&s)[..3].to_vec(), names(&["Ar", "Ag", "Ab"]));
        assert_eq!(wired(&s)[9..12].to_vec(), names(&["Dr", "Dg", "Db"]));
    }

    #[test]
    fn move_needs_free_outputs_and_may_overlap_itself() {
        let mut s = rig();
        let controller = s.controllers[0].id.to_string();
        let right = id_of(&s, "Right");
        // Outputs 18..24 are free but only 6 of the 9 needed fit.
        assert!(move_to(&mut s, &json!({"id":right,"controllerId":controller,"startOutput":18})).unwrap_err().contains("don't fit"));
        // Onto Left's outputs: refused.
        assert!(move_to(&mut s, &json!({"id":right,"controllerId":controller,"startOutput":3})).unwrap_err().contains("already patched"));
        // Three outputs later, overlapping its own.
        let result = move_to(&mut s, &json!({"id":right,"controllerId":controller,"startOutput":12})).unwrap();
        assert_eq!(result["ranges"][0]["firstOutput"], 13);
        assert_eq!(result["ranges"][0]["lastOutput"], 21);
        let w = wired(&s);
        assert_eq!(w[9..12].to_vec(), names(&["", "", ""]));
        assert_eq!(w[12..21].to_vec(), names(&["Dr", "Dg", "Db", "Er", "Eg", "Eb", "Fr", "Fg", "Fb"]));
        assert!(move_to(&mut s, &json!({"id":right,"controllerId":controller,"startOutput":12})).unwrap_err().contains("already patched there"));
    }

    #[test]
    fn menu_lists_swaps_and_free_spots() {
        let s = rig();
        let left = Uuid::parse_str(&id_of(&s, "Left")).unwrap();
        let m = menu(&s, left).unwrap();
        assert_eq!(m["outputs"], 9);
        assert_eq!(m["canReverse"], true);
        assert_eq!(m["swapWith"].as_array().unwrap().len(), 1);
        assert_eq!(m["swapWith"][0]["name"], "Right");
        // No run of 9 free outputs elsewhere, and its own run is where it already is.
        assert!(m["moveTo"].as_array().unwrap().is_empty());
        let a = Uuid::parse_str(&id_of(&s, "A")).unwrap();
        let m = menu(&s, a).unwrap();
        // B and C first (siblings), then D, E, F; 18..24 has two 3-output spots.
        let swaps: Vec<_> = m["swapWith"].as_array().unwrap().iter().map(|v| v["name"].as_str().unwrap().to_string()).collect();
        assert_eq!(swaps[..2], ["B".to_string(), "C".to_string()]);
        assert_eq!(swaps.len(), 5);
        assert_eq!(m["moveTo"][0]["spots"][0]["startOutput"], 18);
    }

    #[test]
    fn universe_channel_labels() {
        let mut s = rig();
        let universes = vec![
            vixen_output::modules::e131::UniverseEntry::new(true, 6, 0, 10),
            vixen_output::modules::e131::UniverseEntry::new(true, 7, 10, 14),
        ];
        s.controllers[0].config = Some(ControllerConfig::E131(vixen_output::modules::e131::E131Config { universes: Some(universes), ..Default::default() }));
        let right = id_of(&s, "Right");
        let r = ranges_of(&s, &json!({"id":right})).unwrap();
        assert_eq!(r["ranges"][0]["from"], "6:10");
        assert_eq!(r["ranges"][0]["to"], "7:8");
        assert!(r["text"].as_str().unwrap().contains("6:10–7:8 (outputs 10–18)"), "{}", r["text"]);
    }
    /// Through the shared display service, as the Display pane and MCP call it: one undo step each, labelled, and undone exactly.
    #[test]
    fn commands_are_single_undo_steps() {
        let root = std::env::temp_dir().join(format!("studio-patch-{}", Uuid::new_v4()));
        let core = crate::app::AppCore::open(root.clone(), true).unwrap();
        let run = |name: &str, args: Value| core.studio_display.execute(&core, name, &args, true);
        let tree = run("element_template", json!({"kind":"megatree","name":"Tree","count":2,"pixels":4,"color":"rgb"})).unwrap();
        let id = tree["result"]["id"].as_str().unwrap().to_string();
        let controller = run("controller_create", json!({"name":"C","typeId":vixen_output::ids::E131.to_string(),"outputs":48})).unwrap();
        let controller = controller["result"]["id"].as_str().unwrap().to_string();
        run("patch_elements", json!({"elementId":id,"controllerId":controller})).unwrap();
        let strings: Vec<String> = {
            let system = core.system.read();
            let node = system.tree.all_nodes().find(|(_, n)| n.name == "Tree").unwrap().1.clone();
            node.children.iter().map(|c| system.tree.node(*c).id.to_string()).collect()
        };
        let before = core.system.read().patches.clone();
        let reversed = run("patch_reverse", json!({"id":strings[0]})).unwrap();
        assert_eq!(reversed["result"]["outputs"], 12);
        assert_eq!(core.studio_display.history()["undo"][0], "patch_reverse (Agent)");
        assert_ne!(core.system.read().patches, before);
        run("display_undo", json!({})).unwrap();
        assert_eq!(core.system.read().patches, before);
        run("patch_swap", json!({"id":strings[0],"withId":strings[1]})).unwrap();
        run("display_undo", json!({})).unwrap();
        assert_eq!(core.system.read().patches, before);
        let moved = run("patch_move", json!({"id":strings[1],"controllerId":controller,"startOutput":30})).unwrap();
        assert_eq!(moved["result"]["ranges"][0]["firstOutput"], 31);
        run("display_undo", json!({})).unwrap();
        assert_eq!(core.system.read().patches, before);
        // A refused edit publishes nothing.
        assert!(run("patch_move", json!({"id":strings[1],"controllerId":controller,"startOutput":0})).is_err());
        assert_eq!(core.studio_display.history()["undo"][0], "patch_elements (Agent)");
        drop(core);
        let _ = std::fs::remove_dir_all(root);
    }

}
