//! What answers `display.snapshot` away from Studio: the conformance harness (`plugin validate`, `plugin_validate`) and
//! `plugin run` have no open show, so plugins with `"display": "read"` get a small sample one, or with `--profile` the
//! snapshot of a profile folder the command line reads (`vixen-studio-mcp` builds that one with vixen-session).
use crate::process::DisplaySink;
use crate::rpc::RpcError;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;

/// Builds `display.snapshot`'s answers from a profile folder: `plugin validate|run --profile DIR`.
pub type ProfileDisplay = fn(&Path) -> Result<DisplaySink, String>;

const PORCH: &str = "5a0f6c1e-2b3d-4e5f-8a9b-0c1d2e3f4a51";
const PORCH_1: &str = "5a0f6c1e-2b3d-4e5f-8a9b-0c1d2e3f4a52";
const PORCH_2: &str = "5a0f6c1e-2b3d-4e5f-8a9b-0c1d2e3f4a53";
const TREE: &str = "5a0f6c1e-2b3d-4e5f-8a9b-0c1d2e3f4a54";
const CONTROLLER: &str = "5a0f6c1e-2b3d-4e5f-8a9b-0c1d2e3f4a60";

/// The whole sample show: a Porch group of two RGB pixels and a single-channel Tree, patched to one stopped E1.31
/// controller on universe 1. Revision 0 never changes.
pub fn sample() -> Value {
    let run = |element: &str, first: u32, last: u32| {
        json!({"elementId":element,"controllerId":CONTROLLER,"firstOutput":first,"lastOutput":last,"from":format!("1:{first}"),"to":format!("1:{last}")})
    };
    json!({
        "revision": 0,
        "roots": [PORCH, TREE],
        "controllers": [{"id":CONTROLLER,"name":"Sample E1.31","typeId":vixen_output::ids::E131.to_string(),"typeName":"E1.31","outputs":512,"running":false}],
        "elements": [
            {"id":PORCH,"name":"Porch","children":[PORCH_1, PORCH_2]},
            {"id":PORCH_1,"name":"Porch Pixel 1","children":[]},
            {"id":PORCH_2,"name":"Porch Pixel 2","children":[]},
            {"id":TREE,"name":"Tree","children":[]},
        ],
        "patches": [run(PORCH_1, 1, 3), run(PORCH_2, 4, 6), run(TREE, 7, 7)],
    })
}

/// Answers `display.snapshot` with [`sample`]. It is small, so each answer is the rest of the show from the offsets asked
/// for, with `more` false.
pub fn sample_sink() -> DisplaySink {
    let full = sample();
    Arc::new(move |params: &Value| Ok::<Value, RpcError>(rest_from(&full, params)))
}

fn rest_from(full: &Value, params: &Value) -> Value {
    let offset = |key: &str| params.get(key).and_then(Value::as_u64).unwrap_or(0) as usize;
    let rest = |key: &str, from: usize| {
        let items = full[key].as_array().cloned().unwrap_or_default();
        let from = from.min(items.len());
        (items.len(), items[from..].to_vec())
    };
    let (element_count, elements) = rest("elements", offset("elementOffset"));
    let (patch_count, patches) = rest("patches", offset("patchOffset"));
    let mut page = full.clone();
    page["elements"] = json!(elements);
    page["patches"] = json!(patches);
    page["elementCount"] = json!(element_count);
    page["patchCount"] = json!(patch_count);
    page["more"] = json!(false);
    page["elementOffset"] = json!(element_count);
    page["patchOffset"] = json!(patch_count);
    page
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sample_answers_whole_and_from_offsets() {
        let sink = sample_sink();
        let page = sink(&json!({})).unwrap();
        assert_eq!((page["elementCount"].clone(), page["patchCount"].clone(), page["more"].clone()), (json!(4), json!(3), json!(false)));
        assert_eq!(page["elements"].as_array().unwrap().len(), 4);
        assert_eq!(page["patches"][0], json!({"elementId":PORCH_1,"controllerId":CONTROLLER,"firstOutput":1,"lastOutput":3,"from":"1:1","to":"1:3"}));
        // Every element id a patch, root or child names is listed.
        let ids: Vec<&Value> = page["elements"].as_array().unwrap().iter().map(|e| &e["id"]).collect();
        for id in page["roots"].as_array().unwrap().iter().chain(page["patches"].as_array().unwrap().iter().map(|p| &p["elementId"])) {
            assert!(ids.contains(&id), "{id} isn't an element");
        }
        let later = sink(&json!({"elementOffset": 3, "patchOffset": 9})).unwrap();
        assert_eq!(later["elements"], json!([{"id":TREE,"name":"Tree","children":[]}]));
        assert_eq!(later["patches"], json!([]));
        assert_eq!((later["elementOffset"].clone(), later["patchOffset"].clone()), (json!(4), json!(3)));
    }
}
