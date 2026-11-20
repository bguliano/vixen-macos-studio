use serde_json::{json, Value};
use std::{
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
/// A megatree built and linked through MCP reports its strings, Px1 end, S1 angle and numbering direction, and the
/// reported angles agree with where each string's base actually sits on the preview's base ellipse.
#[test]
fn megatree_geometry_matches_the_drawn_preview() {
    let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
    let source = target.join("show-copy");
    let root = target.join(format!("studio-mcp-geometry-{}", std::process::id()));
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
    let mut call = |name: &str, args: Value| -> Value {
        let result = rpc("tools/call", json!({"name":name,"arguments":args}));
        assert_eq!(result["isError"], false, "{name}: {result}");
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
    };
    let tree = call("element_template", json!({"kind":"megatree","name":"Geometry Tree","count":8,"pixels":10}));
    let group = tree["result"]["id"].as_str().unwrap().to_string();
    let preview = call("preview_list", json!({}))["items"][0]["id"].as_str().unwrap().to_string();
    let prop = call("preview_prop_create", json!({"previewId":preview,"name":"Geometry Tree","kind":"megatree"}))["result"]["propId"].clone();
    // A pixel tree takes one string element per string, as the native link dialog fills it.
    for (property, kind, value) in [("StringType", "enum", json!("Pixel")), ("StringCount", "int", json!(8)), ("LightsPerString", "int", json!(10)), ("BaseHeight", "int", json!(30))] {
        let revision = call("studio_status", json!({}))["revision"].clone();
        call("preview_prop_set", json!({"previewId":preview,"propId":prop,"property":property,"valueType":kind,"value":value,"expectedRevision":revision}));
    }
    let mut strings = Value::Null;
    let mut offset = Some(0);
    while let Some(at) = offset.filter(|_| strings.is_null()) {
        let page = call("display_elements", json!({"offset":at,"limit":500}));
        if let Some(e) = page["items"].as_array().unwrap().iter().find(|e| e["id"] == json!(group)) {
            strings = e["children"].clone();
        }
        offset = page["nextOffset"].as_u64();
    }
    assert_eq!(strings.as_array().map(Vec::len), Some(8));
    let revision = call("studio_status", json!({}))["revision"].clone();
    call("preview_prop_link", json!({"previewId":preview,"propId":prop,"nodeIds":strings,"expectedRevision":revision}));
    let geometry = call("display_geometry", json!({"previewId":preview,"elementId":group}));
    let props = geometry["props"].as_array().unwrap();
    assert_eq!(props.len(), 1, "{geometry}");
    let summary = &props[0]["summary"];
    assert_eq!(props[0]["elementId"], json!(group), "{geometry}");
    assert_eq!(summary["type"], "megatree", "{summary}");
    assert_eq!(summary["strings"], 8, "{summary}");
    assert_eq!(summary["pixelsPerString"], 10, "{summary}");
    assert_eq!(summary["px1"], "bottom", "{summary}");
    let pixels = geometry["pixels"].as_array().unwrap();
    assert_eq!(pixels.len(), 80);
    assert_eq!(geometry["total"], 80);

    // Each string's base light, its angle and its position.
    let mut bases = Vec::new();
    for s in 1..=8u64 {
        let string: Vec<&Value> = pixels.iter().filter(|p| p["string"] == s).collect();
        assert_eq!(string.len(), 10, "string {s}");
        let heights: Vec<f64> = (1..=10u64).map(|n| string.iter().find(|p| p["pixel"] == n).unwrap()["heightPct"].as_f64().unwrap()).collect();
        assert!(heights.windows(2).all(|w| w[0] < w[1]), "Px1 at the base, rising: {heights:?}");
        let base = string.iter().find(|p| p["pixel"] == 1).unwrap();
        bases.push((base["angleDeg"].as_f64().unwrap(), base["x"].as_f64().unwrap(), base["y"].as_f64().unwrap()));
    }
    assert_eq!(summary["s1AngleDeg"].as_f64().unwrap(), bases[0].0);
    let steps: Vec<f64> = bases.windows(2).map(|w| (w[1].0 - w[0].0 + 540.0).rem_euclid(360.0) - 180.0).collect();
    assert!(steps.iter().all(|d| (d.abs() - 45.0).abs() < 0.01 && d.signum() == steps[0].signum()), "{steps:?}");
    assert_eq!(summary["direction"], if steps[0] > 0.0 { "ccw" } else { "cw" });
    // 0 = front (lowest on the base ellipse), 90 = viewer's right: check against where the bases are drawn.
    let cx = bases.iter().map(|b| b.1).sum::<f64>() / 8.0;
    let cy = bases.iter().map(|b| b.2).sum::<f64>() / 8.0;
    for (angle, x, y) in &bases {
        let (sin, cos) = angle.to_radians().sin_cos();
        if sin.abs() > 0.5 {
            assert_eq!((x - cx).signum(), sin.signum(), "angle {angle} at x {x} (centre {cx})");
        }
        if cos.abs() > 0.5 {
            assert_eq!((y - cy).signum(), cos.signum(), "angle {angle} at y {y} (centre {cy})");
        }
    }

    // Summaries only, and the resource that carries them.
    let only = call("display_geometry", json!({"elementId":group,"pixels":false}));
    assert!(only.get("pixels").is_none() && only["props"][0]["summary"] == *summary, "{only}");
    // An arch linked to a numbered group: one string, Px1 at one end, positions running along it.
    let arch = call("element_template", json!({"kind":"arch","name":"Geometry Arch","count":12}))["result"]["id"].as_str().unwrap().to_string();
    let prop = call("preview_prop_create", json!({"previewId":preview,"name":"Geometry Arch","kind":"arch"}))["result"]["propId"].clone();
    for (property, kind, value) in [("StringType", "enum", json!("Pixel")), ("PixelCount", "int", json!(12))] {
        let revision = call("studio_status", json!({}))["revision"].clone();
        call("preview_prop_set", json!({"previewId":preview,"propId":prop,"property":property,"valueType":kind,"value":value,"expectedRevision":revision}));
    }
    let revision = call("studio_status", json!({}))["revision"].clone();
    call("preview_prop_link", json!({"previewId":preview,"propId":prop,"nodeIds":[arch],"expectedRevision":revision}));
    let geometry = call("display_geometry", json!({"elementId":arch}));
    let summary = &geometry["props"][0]["summary"];
    assert_eq!(summary["type"], "arch", "{geometry}");
    assert_eq!(geometry["props"][0]["elementId"], json!(arch), "{geometry}");
    let pixels = geometry["pixels"].as_array().unwrap();
    let at = |n: u64| pixels.iter().find(|p| p["pixel"] == n).unwrap();
    assert_eq!(pixels.len(), 12);
    assert_eq!((at(1)["positionPct"].as_f64(), at(12)["positionPct"].as_f64()), (Some(0.0), Some(100.0)));
    assert_eq!(summary["px1"], if at(1)["x"].as_f64() <= at(12)["x"].as_f64() { "left" } else { "right" });

    let resource = rpc("resources/read", json!({"uri":"display://summary"}));
    let text: Value = serde_json::from_str(resource["contents"][0]["text"].as_str().unwrap()).unwrap();
    assert!(text["props"].as_array().unwrap().iter().any(|p| p["summary"] == *summary), "{text}");
    let bad = rpc("tools/call", json!({"name":"display_geometry","arguments":{"elementId":"nope"}}));
    assert_eq!(bad["isError"], true);

    drop(input);
    child.wait().unwrap();
    std::fs::remove_dir_all(&root).unwrap();
}
