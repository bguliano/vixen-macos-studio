mod support;
use serde_json::{json, Value};
use std::collections::HashSet;
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
/// Removes the test's profile copy when the test ends, pass or panic.
struct ProfileDir(std::path::PathBuf);
impl Drop for ProfileDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn ids(v: &Value) -> Vec<u64> {
    v["effectIds"].as_array().unwrap().iter().map(|i| i.as_u64().unwrap()).collect()
}
/// B81 through the bridge: targetRegion picks a megatree's lights by angle and height as display_geometry measures them
/// (whole strings as one target, the whole tree as the group), the sequence palette is one undo step that is saved with
/// the sequence, warns about effects outside it and is render_analyze's default, and effect_catalog/effects_library
/// describe each effect's look on a tree, which the render bears out for Spin.
#[test]
fn tree_regions_palette_and_effect_looks() {
    let target = support::target_dir();
    let source = target.join("show-copy");
    let profile = ProfileDir(target.join(format!("studio-mcp-tree-targeting-{}", std::process::id())));
    let root = profile.0.clone();
    let _ = std::fs::remove_dir_all(&root);
    copy_dir(&source.join("SystemData"), &root.join("SystemData"));
    copy_dir(&source.join("Module Data Files"), &root.join("Module Data Files"));
    std::fs::create_dir_all(root.join("Sequence")).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_vixen-studio-mcp"))
        .args(["--profile", root.to_str().unwrap()])
        .env("VIXEN_MUTE", "1")
        .env("VIXEN_SETTINGS_DIR", root.join("settings"))
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
    let tools = rpc("tools/list", json!({}));
    let tool = |name: &str| tools["tools"].as_array().unwrap().iter().find(|t| t["name"] == name).cloned().unwrap_or_else(|| panic!("{name} not listed"));
    assert!(tool("effect_add")["inputSchema"]["properties"]["targetRegion"]["examples"].is_array());
    assert!(tool("effects_batch")["inputSchema"]["properties"]["ops"]["items"]["properties"]["targetRegion"].is_object());
    assert_eq!(tool("sequence_palette")["annotations"]["readOnlyHint"], false);
    let mut try_call = |name: &str, args: Value| -> Result<Value, String> {
        let result = rpc("tools/call", json!({"name":name,"arguments":args}));
        let text = result["content"][0]["text"].as_str().unwrap().to_string();
        if result["isError"] == true {
            Err(text)
        } else {
            Ok(serde_json::from_str(&text).unwrap())
        }
    };
    macro_rules! call {
        ($name:expr, $args:expr) => {
            try_call($name, $args).unwrap_or_else(|e| panic!("{}: {e}", $name))
        };
    }

    // An 8-string, 10-light megatree shown in a preview, and where display_geometry says each light sits.
    let preview = call!("preview_list", json!({}))["items"][0]["id"].as_str().unwrap().to_string();
    let tree = call!("element_template", json!({"kind":"megatree","name":"Region Tree","count":8,"pixels":10,"previewId":preview}))["result"]["id"].as_str().unwrap().to_string();
    let geometry = call!("display_geometry", json!({"previewId":preview,"elementId":tree}));
    let pixels = geometry["pixels"].as_array().unwrap().clone();
    assert_eq!(pixels.len(), 80, "{geometry}");
    let angle = |p: &Value| p["angleDeg"].as_f64().unwrap();
    let height = |p: &Value| p["heightPct"].as_f64().unwrap();
    // Ranges are inclusive, so strings hanging exactly at 45 and 315 degrees belong to the front quarter.
    let front = |a: f64| a <= 45.05 || a >= 314.95;
    let front_lights: HashSet<String> = pixels.iter().filter(|p| front(angle(p))).map(|p| p["nodeId"].as_str().unwrap().to_string()).collect();
    let front_strings: HashSet<u64> = pixels.iter().filter(|p| front(angle(p))).map(|p| p["string"].as_u64().unwrap()).collect();
    assert!(!front_strings.is_empty() && front_strings.len() < 8, "{geometry}");

    let sequence = call!("sequence_create", json!({"lengthMs":20000}))["sequenceId"].clone();
    let catalog = call!("effect_catalog", json!({"limit":500}));
    let effect = |name: &str| catalog["items"].as_array().unwrap().iter().find(|e| e["name"] == name).unwrap().clone();
    let pulse = effect("Pulse")["id"].clone();
    let targets_of = |effects: &Value, wanted: &[u64]| -> Vec<String> {
        effects["effects"].as_array().unwrap().iter().filter(|e| wanted.contains(&e["id"].as_u64().unwrap())).map(|e| e["targets"][0].as_str().unwrap().to_string()).collect()
    };

    // The front quarter by angle: whole strings, one effect each, covering exactly the front lights.
    let added = call!("effect_add", json!({"sequenceId":sequence,"typeId":pulse,"targetRegion":{"elementId":tree,"angleDeg":[315,45]},"startMs":0,"durationMs":1000}));
    assert_eq!(ids(&added).len(), front_strings.len(), "{added}");
    let effects = call!("sequence_effects", json!({"sequenceId":sequence}));
    let mut covered = HashSet::new();
    for string in targets_of(&effects, &ids(&added)) {
        let lights = call!("display_geometry", json!({"previewId":preview,"elementId":string}))["pixels"].as_array().unwrap().clone();
        assert_eq!(lights.len(), 10, "front targets are whole strings: {string}");
        covered.extend(lights.iter().map(|p| p["nodeId"].as_str().unwrap().to_string()));
    }
    assert_eq!(covered, front_lights);

    // The top half by height: single lights (no whole string matches), as display_geometry measures heightPct.
    let top: Vec<&Value> = pixels.iter().filter(|p| height(p) >= 50.0).collect();
    let added = call!("effect_add", json!({"sequenceId":sequence,"typeId":pulse,"targetRegion":{"elementId":tree,"heightPct":[50,100]},"startMs":1000,"durationMs":1000}));
    assert_eq!(ids(&added).len(), top.len(), "{added}");
    // Angle and height together; select strings widens to the strings; no ranges at all is the tree itself.
    let both = pixels.iter().filter(|p| height(p) >= 50.0 && front(angle(p))).count();
    let added = call!("effect_add", json!({"sequenceId":sequence,"typeId":pulse,"targetRegion":{"elementId":tree,"heightPct":[50,100],"angleDeg":[315,45]},"startMs":2000,"durationMs":1000}));
    assert_eq!(ids(&added).len(), both);
    let added = call!("effect_add", json!({"sequenceId":sequence,"typeId":pulse,"targetRegion":{"elementId":tree,"heightPct":[50,100],"select":"strings"},"startMs":3000,"durationMs":1000}));
    assert_eq!(ids(&added).len(), 8);
    let added = call!("effect_add", json!({"sequenceId":sequence,"typeId":pulse,"targetRegion":{"elementId":tree},"startMs":4000,"durationMs":1000}));
    let effects = call!("sequence_effects", json!({"sequenceId":sequence}));
    assert_eq!(targets_of(&effects, &ids(&added)), vec![tree.clone()]);
    // One undo step per call.
    let before = call!("sequence_status", json!({"sequenceId":sequence}))["effects"].as_u64().unwrap();
    call!("sequence_undo", json!({"sequenceId":sequence}));
    assert_eq!(call!("sequence_status", json!({"sequenceId":sequence}))["effects"].as_u64().unwrap(), before - 1);
    call!("sequence_redo", json!({"sequenceId":sequence}));

    // Bands take an angle region (a ring on the front only) but set their own height.
    let band = call!("effect_band", json!({"sequenceId":sequence,"targetRegion":{"elementId":tree,"angleDeg":[315,45]},"startMs":5000,"durationMs":1000,"fromPct":40,"toPct":60}));
    assert_eq!(ids(&band).len(), front_strings.len(), "{band}");
    let refused = try_call("effect_band", json!({"sequenceId":sequence,"targetRegion":{"elementId":tree,"heightPct":[0,50]},"startMs":5000,"durationMs":1000,"fromPct":40,"toPct":60})).unwrap_err();
    assert!(refused.contains("fromPct"), "{refused}");
    // Batch ops take regions too; bad regions fail with a reason.
    let batch = call!("effects_batch", json!({"sequenceId":sequence,"ops":[{"op":"add","typeId":pulse,"targetRegion":{"elementId":tree,"angleDeg":[315,45]},"startMs":6000,"durationMs":500}]}));
    assert_eq!(batch["added"].as_u64().unwrap() as usize, front_strings.len());
    assert!(try_call("effect_add", json!({"sequenceId":sequence,"typeId":pulse,"targetRegion":{"elementId":tree,"heightPct":[60,40]},"startMs":0,"durationMs":100})).unwrap_err().contains("from at most to"));
    let unknown = try_call("effect_add", json!({"sequenceId":sequence,"typeId":pulse,"targetRegion":{"elementId":tree,"height":[0,10]},"startMs":0,"durationMs":100})).unwrap_err();
    assert!(unknown.contains("height"), "{unknown}");
    assert!(try_call("effect_add", json!({"sequenceId":sequence,"typeId":pulse,"targetId":tree,"targetRegion":{"elementId":tree},"startMs":0,"durationMs":100})).unwrap_err().contains("exactly one"));

    // The palette: read, set (one undo step), warnings on adds and edits outside it, render_analyze's default.
    let read = call!("sequence_palette", json!({"sequenceId":sequence}));
    assert_eq!(read["palette"], json!([]), "{read}");
    let set = call!("sequence_palette", json!({"sequenceId":sequence,"colors":["#FF0000","#ffffff"]}));
    assert_eq!(set["palette"], json!(["#ff0000", "#ffffff"]), "{set}");
    assert_eq!(set["changed"], true);
    let undo = call!("sequence_status", json!({"sequenceId":sequence}))["undo"].clone();
    assert_eq!(undo[0], "Set palette (Agent)", "{undo}");
    call!("sequence_undo", json!({"sequenceId":sequence}));
    assert_eq!(call!("sequence_palette", json!({"sequenceId":sequence}))["palette"], json!([]));
    call!("sequence_redo", json!({"sequenceId":sequence}));
    assert_eq!(call!("sequence_palette", json!({"sequenceId":sequence}))["palette"], json!(["#ff0000", "#ffffff"]));
    let red = call!("effect_add", json!({"sequenceId":sequence,"typeId":pulse,"targetId":tree,"startMs":7000,"durationMs":1000,"params":{"LevelCurve":[[0,100],[100,100]],"ColorGradient":["#ff0000"]}}));
    assert!(red.get("paletteWarnings").is_none(), "{red}");
    let dim_red = call!("effect_add", json!({"sequenceId":sequence,"typeId":pulse,"targetId":tree,"startMs":8000,"durationMs":1000,"params":{"ColorGradient":["#800000"]}}));
    assert!(dim_red.get("paletteWarnings").is_none(), "a dim red is still red: {dim_red}");
    let blue = call!("effect_add", json!({"sequenceId":sequence,"typeId":pulse,"targetId":tree,"startMs":9000,"durationMs":3000,"params":{"LevelCurve":[[0,100],[100,100]],"ColorGradient":["#0000ff"]}}));
    assert_eq!(blue["paletteWarningCount"], 1, "{blue}");
    assert_eq!(blue["paletteWarnings"][0]["colors"], json!(["#0000ff"]), "{blue}");
    let edited = call!("effect_set_parameter", json!({"sequenceId":sequence,"effectId":red["effectId"],"parameter":"ColorGradient","value":["#00ff00"]}));
    assert_eq!(edited["paletteWarnings"][0]["effectId"], red["effectId"], "{edited}");
    let read = call!("sequence_palette", json!({"sequenceId":sequence}));
    assert_eq!(read["offPaletteCount"], 2, "{read}");
    let report = call!("render_analyze", json!({"sequenceId":sequence,"previewId":preview,"scope":tree,"startMs":7000,"endMs":12000,"sectionMs":5000}));
    assert_eq!(report["paletteFrom"], "sequence", "{report}");
    assert!(report["flags"]["offPalette"].as_u64().unwrap() >= 1, "{report}");
    let given = call!("render_analyze", json!({"sequenceId":sequence,"previewId":preview,"scope":tree,"startMs":7000,"endMs":12000,"palette":["#0000ff","#00ff00"]}));
    assert_eq!(given["paletteFrom"], "argument", "{given}");
    // Saved beside the sequence and read back on open.
    call!("sequence_save", json!({"sequenceId":sequence,"file":"Region Palette.tim","confirm":true}));
    assert!(root.join("Sequence/Region Palette.studio-builds.json").is_file());
    call!("sequence_close", json!({"sequenceId":sequence,"confirm":true}));
    let reopened = call!("sequence_open", json!({"file":"Region Palette.tim"}))["sequenceId"].clone();
    assert_eq!(call!("sequence_palette", json!({"sequenceId":reopened}))["palette"], json!(["#ff0000", "#ffffff"]));
    assert_eq!(call!("sequence_palette", json!({"sequenceId":reopened,"clear":true}))["palette"], json!([]));
    assert!(try_call("sequence_palette", json!({"sequenceId":reopened,"colors":["#000000"]})).is_err());

    // Looks: what each effect does on a tree, in the catalog and the library.
    let wipe = effect("Wipe")["looks"].as_str().unwrap().to_string();
    assert!(wipe.contains("DOWNWARD"), "{wipe}");
    let spin = effect("Spin");
    assert!(spin["looks"].as_str().unwrap().contains("sliver"));
    assert!(effect("Chase")["looks"].as_str().unwrap().contains("DepthOfEffect 1"));
    let library = call!("effects_library", json!({"favorite":spin["id"],"on":true}));
    assert!(library["favorites"][0]["looks"].as_str().unwrap().contains("sliver"), "{library}");
    // The render bears out Spin's look: by default a sliver of the tree, with DepthOfEffect 1 whole strings.
    let mut lit = |depth: i64| -> f64 {
        let s = call!("sequence_create", json!({"lengthMs":4000}))["sequenceId"].clone();
        call!("effect_add", json!({"sequenceId":s,"typeId":spin["id"],"targetId":tree,"startMs":0,"durationMs":4000,"params":{"DepthOfEffect":depth}}));
        let activity = call!("sequence_activity", json!({"sequenceId":s,"previewId":preview,"elements":[tree],"minMs":0}));
        call!("sequence_close", json!({"sequenceId":s,"confirm":true}));
        activity["elements"][0]["lit"]["mean"].as_f64().unwrap_or_else(|| panic!("{activity}"))
    };
    let (sliver, strings) = (lit(0), lit(1));
    assert!(sliver < 0.125, "Spin lit {sliver} of the tree on average");
    assert!(strings >= 0.08 && strings > 5.0 * sliver, "Spin by strings lit {strings}, by pixels {sliver}");

    drop(input);
    let _ = child.wait();
}
