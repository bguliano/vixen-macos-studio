mod support;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Command, Stdio},
};
fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let to = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &to)
        } else if entry.file_type().unwrap().is_file() {
            std::fs::copy(entry.path(), to).unwrap();
        }
    }
}
/// One element_template call makes a 32 × 28 megatree and its preview shape, linked light for light: every light is in the
/// preview, S1 Px1 at the bottom of the first string, and a single-pixel chase in wire order (the order patch_elements uses)
/// steps from each light to a neighbour in the preview, never jumping across the tree. One undo removes both.
#[test]
fn a_new_megatree_appears_linked_in_the_chosen_preview() {
    let target = support::target_dir();
    let source = target.join("show-copy");
    let root = target.join(format!("studio-mcp-linked-preview-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    copy_dir(&source.join("SystemData"), &root.join("SystemData"));
    copy_dir(&source.join("Module Data Files"), &root.join("Module Data Files"));
    let mut child = Command::new(env!("CARGO_BIN_EXE_vixen-studio-mcp"))
        .args(["--profile", root.to_str().unwrap()])
        .env("VIXEN_MUTE", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut next = 1;
    let mut rpc = |method: &str, params: Value| -> Value {
        let id = next;
        next += 1;
        writeln!(input, "{}", json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})).unwrap();
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        let reply: Value = serde_json::from_str(&line).unwrap();
        assert!(reply.get("error").is_none(), "{reply}");
        reply["result"].clone()
    };
    rpc("initialize", json!({"protocolVersion":"2024-11-05","clientInfo":{},"capabilities":{}}));
    let mut try_call = |name: &str, args: Value| -> Result<Value, String> {
        let result = rpc("tools/call", json!({"name":name,"arguments":args}));
        let text = result["content"][0]["text"].as_str().unwrap().to_string();
        if result["isError"] == true {
            Err(text)
        } else {
            Ok(serde_json::from_str(&text).unwrap())
        }
    };
    let preview = try_call("preview_list", json!({})).unwrap()["items"][0]["id"].as_str().unwrap().to_string();
    let lights_before = try_call("display_geometry", json!({"previewId":preview,"pixels":false})).unwrap()["props"].as_array().unwrap().len();

    // Only shapes the designer can build from an element take a previewId.
    let refused = try_call("element_template", json!({"kind":"group","name":"Lone","count":3,"previewId":preview})).unwrap_err();
    assert!(refused.contains("megatree, matrix or arch"), "{refused}");

    let made = try_call(
        "element_template",
        json!({"kind":"megatree","name":"Linked Tree","count":32,"pixels":28,"start":"bottomRight","reverseEveryOtherString":true,"hiddenPixels":2,"previewId":preview}),
    )
    .unwrap();
    let result = &made["result"];
    let tree = result["id"].as_str().unwrap().to_string();
    let hidden = result["hiddenId"].as_str().unwrap().to_string();
    let shape = &result["preview"];
    assert_eq!((shape["strings"].as_u64(), shape["lightsPerString"].as_u64(), shape["linkedLights"].as_u64()), (Some(32), Some(28), Some(896)), "{result}");

    let geometry = try_call("display_geometry", json!({"previewId":preview,"elementId":tree,"limit":5000})).unwrap();
    let summary = &geometry["props"][0]["summary"];
    assert_eq!(geometry["props"][0]["name"], "Linked Tree", "{geometry}");
    assert_eq!(geometry["props"][0]["unlinkedLights"], 0, "{geometry}");
    assert_eq!((summary["type"].as_str(), summary["strings"].as_u64(), summary["pixelsPerString"].as_u64(), summary["degrees"].as_u64()), (Some("megatree"), Some(32), Some(28), Some(360)), "{summary}");
    assert_eq!(summary["px1"], "bottom", "{summary}");
    let pixels = geometry["pixels"].as_array().unwrap();
    assert_eq!(pixels.len(), 896);
    let at: HashMap<&str, (f64, f64)> = pixels.iter().map(|p| (p["nodeId"].as_str().unwrap(), (p["x"].as_f64().unwrap(), p["y"].as_f64().unwrap()))).collect();

    // A single-pixel chase in wire order: patch the tree, then step through its outputs.
    let types = try_call("controller_types", json!({})).unwrap();
    let e131 = types["items"].as_array().or(types.as_array()).unwrap().iter().find(|t| t["name"].as_str().unwrap_or("").contains("E1.31")).unwrap()["id"].clone();
    let controller = try_call("controller_create", json!({"name":"Chase","typeId":e131,"outputs":1000})).unwrap()["result"]["id"].clone();
    let patched = try_call("patch_elements", json!({"elementId":tree,"alsoIds":[hidden],"controllerId":controller})).unwrap();
    assert_eq!(patched["result"]["wireOrder"], true, "{patched}");
    let mut chase: Vec<(u64, (f64, f64))> = pixels
        .iter()
        .map(|p| {
            let id = p["nodeId"].as_str().unwrap();
            let r = try_call("patch_ranges", json!({"id":id})).unwrap();
            (r["ranges"][0]["firstOutput"].as_u64().unwrap(), at[id])
        })
        .collect();
    chase.sort_by_key(|c| c.0);
    let steps: Vec<f64> = chase.windows(2).map(|w| ((w[1].1 .0 - w[0].1 .0).powi(2) + (w[1].1 .1 - w[0].1 .1).powi(2)).sqrt()).collect();
    let along = steps.iter().cloned().fold(f64::INFINITY, f64::min);
    let worst = steps.iter().cloned().fold(0.0, f64::max);
    // The tree is 300 tall: a chase that ran a string the wrong way would jump most of that between strings.
    assert!(worst < 40.0, "the chase jumps {worst} (shortest step {along})");
    // The first visible light on the wire is S32 Px1, the bottom of the last string.
    let first = pixels.iter().find(|p| p["string"] == 32 && p["pixel"] == 1).unwrap();
    assert_eq!(chase[0].1, (first["x"].as_f64().unwrap(), first["y"].as_f64().unwrap()));

    // The same shape for an element that already exists, and a refusal for one it can't take light for light.
    let arch = try_call("element_template", json!({"kind":"arch","name":"Linked Arch","count":12})).unwrap()["result"]["id"].as_str().unwrap().to_string();
    let linked = try_call("preview_prop_create", json!({"previewId":preview,"kind":"arch","elementId":arch})).unwrap();
    assert_eq!(linked["result"]["linkedLights"], 12, "{linked}");
    // A matrix as the template draws it: Row1 at the bottom, Pixel1 on the left.
    let grid = try_call("element_template", json!({"kind":"matrix","name":"Linked Grid","count":4,"pixels":10,"previewId":preview})).unwrap();
    assert_eq!(grid["result"]["preview"]["linkedLights"], 40, "{grid}");
    let cells = try_call("display_geometry", json!({"previewId":preview,"elementId":grid["result"]["id"]})).unwrap();
    assert_eq!(cells["props"][0]["summary"]["type"], "matrix", "{cells}");
    let corner = cells["pixels"].as_array().unwrap().iter().find(|p| p["string"] == 1 && p["pixel"] == 1).unwrap();
    assert_eq!((corner["xPct"].as_f64(), corner["yPct"].as_f64()), (Some(0.0), Some(0.0)), "{corner}");
    let small = try_call("element_template", json!({"kind":"megatree","name":"Small Tree","count":2,"pixels":3})).unwrap()["result"]["id"].as_str().unwrap().to_string();
    let refused = try_call("preview_prop_create", json!({"previewId":preview,"kind":"megatree","elementId":small})).unwrap_err();
    assert!(refused.contains("at least 4 strings"), "{refused}");

    // Undo the grid, the arch and its shape, the controller work and the tree: the preview is as it was.
    for _ in 0..7 {
        try_call("display_undo", json!({})).unwrap();
    }
    let after = try_call("display_geometry", json!({"previewId":preview,"pixels":false})).unwrap();
    assert_eq!(after["props"].as_array().unwrap().len(), lights_before, "{after}");
    drop(input);
    child.wait().unwrap();
    std::fs::remove_dir_all(&root).unwrap();
}
