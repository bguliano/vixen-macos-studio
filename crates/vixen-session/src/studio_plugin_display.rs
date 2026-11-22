//! Studio: the read-only view of the open show that plugins with `"display": "read"` get from `display.snapshot`
//! (`studio/PLUGINS-DESIGN.md`, P2b). Built from a fresh working copy of the profile each time, so it shows what is
//! saved and applied, never a half-made edit, and nothing a plugin does with it can change the show.
use crate::app::AppCore;
use crate::setup::SetupSession;
use serde_json::{json, Value};
use vixen_output::ControllerConfig;

/// A page's elements and patches stop before its line would pass this, leaving room under the protocol's 1 MiB limit
/// for the rest of the answer.
pub const PAGE_BYTES: usize = 768 * 1024;

/// One page of `{revision, roots, controllers, elements, patches, elementCount, patchCount, more, elementOffset,
/// patchOffset}` for the open profile. `params` may carry the `elementOffset`/`patchOffset` the previous page answered
/// with; `more` says another page follows.
pub fn snapshot(core: &AppCore, params: &Value) -> Value {
    let offset = |key: &str| params.get(key).and_then(Value::as_u64).unwrap_or(0) as usize;
    page(of_session(&SetupSession::open(core), core.display.revision()), offset("elementOffset"), offset("patchOffset"), PAGE_BYTES)
}

/// The page of `full` (from [`of_session`]) starting at the two offsets: elements first, then patches, each item whole,
/// until `budget` bytes are used (always at least one item, so paging moves on).
pub(crate) fn page(mut full: Value, element_offset: usize, patch_offset: usize, budget: usize) -> Value {
    let list = |v: &mut Value| match v.take() {
        Value::Array(items) => items,
        _ => Vec::new(),
    };
    let elements = list(&mut full["elements"]);
    let patches = list(&mut full["patches"]);
    // The rest of the answer, plus room for the counts, offsets and JSON-RPC envelope.
    let mut used = full.to_string().len() + 256;
    let mut taken = 0;
    let mut fill = |items: &[Value], from: usize, page: &mut Vec<Value>| -> usize {
        let mut at = from.min(items.len());
        while at < items.len() {
            let size = items[at].to_string().len() + 1;
            if taken > 0 && used + size > budget {
                break;
            }
            used += size;
            taken += 1;
            page.push(items[at].clone());
            at += 1;
        }
        at
    };
    let (mut element_page, mut patch_page) = (Vec::new(), Vec::new());
    let next_element = fill(&elements, element_offset, &mut element_page);
    // Patches only once every element has been sent, so a page never skips ahead.
    let next_patch = if next_element == elements.len() { fill(&patches, patch_offset, &mut patch_page) } else { patch_offset.min(patches.len()) };
    full["elementCount"] = json!(elements.len());
    full["patchCount"] = json!(patches.len());
    full["elements"] = json!(element_page);
    full["patches"] = json!(patch_page);
    full["more"] = json!(next_element < elements.len() || next_patch < patches.len());
    full["elementOffset"] = json!(next_element);
    full["patchOffset"] = json!(next_patch);
    full
}

/// The whole snapshot of a working copy: elements as the tree has them (a light has no children), controllers in pane
/// order, and every patched run (`crate::studio_patch::light_runs`).
pub(crate) fn of_session(s: &SetupSession, revision: u64) -> Value {
    let id = |n| s.tree.node(n).id.to_string();
    let elements: Vec<Value> = s
        .tree
        .live_nodes()
        .map(|(_, n)| json!({"id":n.id.to_string(),"name":n.name,"children":n.children.iter().map(|c| id(*c)).collect::<Vec<_>>()}))
        .collect();
    let controllers: Vec<Value> = s
        .controller_order
        .iter()
        .filter_map(|slot| s.controllers.get(*slot as usize).filter(|c| c.alive))
        .map(|c| {
            let mut out = json!({"id":c.id.to_string(),"name":c.name,"typeId":c.type_id.to_string(),"typeName":c.type_name(),"outputs":c.outputs.len(),"running":c.running});
            // A plugin's own controllers carry the instanceId its controller.* messages use.
            if let Some(ControllerConfig::External(_)) = &c.config {
                out["instanceId"] = json!(c.instance_id.to_string());
            }
            out
        })
        .collect();
    json!({
        "revision": revision,
        "roots": s.tree.roots.iter().filter(|r| s.tree.is_live(**r)).map(|r| id(*r)).collect::<Vec<_>>(),
        "elements": elements,
        "controllers": controllers,
        "patches": crate::studio_patch::light_runs(s),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setup::test_support::*;

    #[test]
    fn a_snapshot_lists_elements_controllers_and_patched_runs() {
        let mut s = session(&["A", "B", "C"], 8);
        for name in ["A", "B"] {
            let node = s.tree.live_nodes().find(|(_, n)| n.name == name).unwrap().1;
            let element = s.flow.find(&node.element_id.unwrap()).unwrap();
            add_breakdown(&mut s, element, 3);
        }
        let rows: Vec<String> = ["A", "B"].iter().map(|n| s.row_path_of(s.tree.live_nodes().find(|(_, x)| x.name == *n).unwrap().0)).collect();
        s.create_group(&rows, "Porch");
        let porch = vec![s.row_path_of(s.tree.live_nodes().find(|(_, x)| x.name == "Porch").unwrap().0)];
        assert_eq!(s.patch_elements(&porch, &s.all_outputs_selection(), &Default::default(), false), 6);

        let snap = of_session(&s, 7);
        assert_eq!(snap["revision"], 7);
        let named = |name: &str| snap["elements"].as_array().unwrap().iter().find(|e| e["name"] == name).unwrap().clone();
        let (a, b, porch) = (named("A"), named("B"), named("Porch"));
        assert_eq!(porch["children"], json!([a["id"], b["id"]]));
        assert_eq!(a["children"], json!([]));
        assert!(snap["roots"].as_array().unwrap().contains(&porch["id"]));
        assert!(snap["roots"].as_array().unwrap().contains(&named("C")["id"]));

        let controller = &snap["controllers"][0];
        assert_eq!(controller["name"], "Main");
        assert_eq!(controller["outputs"], 8);
        assert!(controller.get("instanceId").is_none(), "only plugin controllers carry an instanceId");

        // Each RGB light is one run of three outputs; C isn't patched.
        let id = controller["id"].clone();
        assert_eq!(
            snap["patches"],
            json!([
                {"elementId":a["id"],"controllerId":id,"firstOutput":1,"lastOutput":3,"from":"1","to":"3"},
                {"elementId":b["id"],"controllerId":id,"firstOutput":4,"lastOutput":6,"from":"4","to":"6"},
            ])
        );

        // A small show is one page.
        let one = page(snap.clone(), 0, 0, PAGE_BYTES);
        assert_eq!((one["more"].clone(), one["elementCount"].clone(), one["patchCount"].clone()), (json!(false), json!(4), json!(2)));
        assert_eq!(one["elements"], snap["elements"]);
        assert_eq!(one["patches"], snap["patches"]);

        // A tight budget pages through everything in order, at least one item a page.
        let (mut elements, mut patches, mut offsets, mut pages) = (Vec::new(), Vec::new(), (0, 0), 0);
        loop {
            let p = page(snap.clone(), offsets.0, offsets.1, 1);
            elements.extend(p["elements"].as_array().unwrap().iter().cloned());
            patches.extend(p["patches"].as_array().unwrap().iter().cloned());
            assert_eq!(p["elements"].as_array().unwrap().len() + p["patches"].as_array().unwrap().len(), 1);
            pages += 1;
            if p["more"] == false {
                break;
            }
            offsets = (p["elementOffset"].as_u64().unwrap() as usize, p["patchOffset"].as_u64().unwrap() as usize);
        }
        assert_eq!(pages, 6);
        assert_eq!(json!(elements), snap["elements"]);
        assert_eq!(json!(patches), snap["patches"]);
        // Offsets past the end give an empty last page.
        assert_eq!(page(snap.clone(), 99, 99, PAGE_BYTES)["elements"], json!([]));
    }

    #[test]
    fn a_big_show_pages_under_the_line_limit() {
        let names: Vec<String> = (0..12000).map(|i| format!("Pixel {i}")).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut s = session(&names, 12000);
        let all: Vec<String> = s.tree.live_nodes().map(|(n, _)| s.row_path_of(n)).collect();
        assert_eq!(s.patch_elements(&all, &s.all_outputs_selection(), &Default::default(), false), 12000);
        let full = of_session(&s, 1);
        let (mut offsets, mut seen) = ((0, 0), (0, 0));
        loop {
            let p = page(full.clone(), offsets.0, offsets.1, PAGE_BYTES);
            assert!(p.to_string().len() < PAGE_BYTES + 1024, "page is {} bytes", p.to_string().len());
            seen.0 += p["elements"].as_array().unwrap().len();
            seen.1 += p["patches"].as_array().unwrap().len();
            if p["more"] == false {
                break;
            }
            offsets = (p["elementOffset"].as_u64().unwrap() as usize, p["patchOffset"].as_u64().unwrap() as usize);
        }
        assert_eq!(seen, (12000, 12000));
    }
}
