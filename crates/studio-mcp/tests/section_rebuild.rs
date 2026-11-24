mod support;
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
/// Removes the test's profile copy when the test ends, pass or panic.
struct ProfileDir(std::path::PathBuf);
impl Drop for ProfileDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
/// B80 through the bridge: section_rebuild replaces one section and tags its effects, build_edits reports an edit made
/// since, a rebuild over it refuses until told to keep it, one undo puts the section back, and the build tags and a
/// labelled save survive closing the document; the labelled save restores in place.
#[test]
fn section_rebuilds_report_hand_edits_and_labelled_saves_restore() {
    let target = support::target_dir();
    let source = target.join("show-copy");
    let profile = ProfileDir(target.join(format!("studio-mcp-rebuild-{}", std::process::id())));
    let root = profile.0.clone();
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
    let tools = rpc("tools/list", json!({}))["tools"].as_array().unwrap().clone();
    for (name, read_only) in [("section_rebuild", false), ("build_edits", true), ("build_tag", false), ("sequence_versions", true), ("sequence_version_restore", false)] {
        let tool = tools.iter().find(|t| t["name"] == name).unwrap_or_else(|| panic!("{name} is listed"));
        assert_eq!(tool["annotations"]["readOnlyHint"], read_only, "{name}");
        assert!(tool["description"].as_str().unwrap().contains("Example") || read_only, "{name} has an example");
    }
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

    let tree = call!("element_create", json!({"name":"Rebuild Tree"}))["result"]["ids"][0].clone();
    call!("element_color", json!({"id":tree,"mode":"rgb"}));
    let catalog = call!("effect_catalog", json!({}));
    let pulse = catalog["items"].as_array().unwrap().iter().find(|e| e["name"] == "Pulse").unwrap()["id"].clone();
    let add = |start: f64, len: f64| json!({"op":"add","typeId":pulse,"targetId":tree,"startMs":start,"durationMs":len});
    let created = call!("sequence_create", json!({"lengthMs":6000}))["sequenceId"].clone();
    let id = call!("sequence_save", json!({"sequenceId":created,"file":"Rebuild.tim","confirm":true}))["sequenceId"].clone();

    let v1 = call!("section_rebuild", json!({"sequenceId":id,"range":[1000,3000],"buildId":"verse v1","ops":[add(1000.0,500.0),add(2000.0,500.0)]}));
    let ids = v1["effectIds"].clone();
    call!("effects_batch", json!({"sequenceId":id,"buildId":"outro","ops":[add(4000.0,1000.0)]}));
    assert_eq!(call!("build_edits", json!({"sequenceId":id}))["clean"], true);

    // An edit since the build: build_edits names it and what changed; a rebuild refuses, then keeps it.
    call!("effect_timing", json!({"sequenceId":id,"effectId":ids[0],"startMs":1100,"durationMs":500}));
    let report = call!("build_edits", json!({"sequenceId":id,"sinceBuild":"verse v1"}));
    assert_eq!((report["counts"]["edited"].as_u64(), report["edited"][0]["changed"].clone()), (Some(1), json!(["timing"])), "{report}");
    let rebuild = json!({"sequenceId":id,"range":[1000,3000],"buildId":"verse v2","ops":[add(1500.0,1000.0)]});
    let refused = try_call("section_rebuild", rebuild.clone()).unwrap_err();
    assert!(refused.contains("1 hand edit") && refused.contains("timing"), "{refused}");
    let mut keep = rebuild.clone();
    keep["onHandEdits"] = json!("keep");
    let kept = call!("section_rebuild", keep);
    assert_eq!((kept["removed"].as_u64(), kept["added"].as_u64(), kept["handEditsKept"].as_u64()), (Some(1), Some(1), Some(1)), "{kept}");
    macro_rules! effects {
        ($id:expr) => {
            call!("sequence_effects", json!({"sequenceId":$id}))["effects"].as_array().unwrap().iter().map(|e| (e["startMs"].as_f64().unwrap(), e["durationMs"].as_f64().unwrap())).collect::<Vec<_>>()
        };
    }
    let mut now = effects!(&id);
    now.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(now, [(1100.0, 500.0), (1500.0, 1000.0), (4000.0, 1000.0)], "the edit and the outro stay");
    call!("sequence_undo", json!({"sequenceId":id}));
    assert_eq!(effects!(&id).len(), 3);
    assert_eq!(call!("build_edits", json!({"sequenceId":id}))["counts"]["edited"], 1);
    call!("sequence_redo", json!({"sequenceId":id}));

    // Saved with a label, closed and reopened: the tags still match, and the labelled save restores in place.
    let saved = call!("sequence_save", json!({"sequenceId":id,"file":"Rebuild.tim","label":"WiW v6","confirm":true}));
    assert_eq!(saved["version"]["label"], "WiW v6");
    call!("sequence_close", json!({"sequenceId":id}));
    let id = call!("sequence_open", json!({"file":"Rebuild.tim"}))["sequenceId"].clone();
    let reopened = call!("build_edits", json!({"sequenceId":id}));
    assert_eq!((reopened["counts"]["edited"].as_u64(), reopened["builds"].as_array().unwrap().len()), (Some(1), 3), "{reopened}");
    call!("sequence_clear", json!({"sequenceId":id,"confirm":true}));
    assert_eq!(call!("sequence_versions", json!({"sequenceId":id}))["versions"].as_array().unwrap().len(), 1);
    let restored = call!("sequence_version_restore", json!({"sequenceId":id,"label":"WiW v6","confirm":true}));
    assert_eq!(restored["restored"], 3, "{restored}");
    call!("sequence_undo", json!({"sequenceId":id}));
    assert!(effects!(&id).is_empty(), "one undo step");

    drop(try_call);
    drop(rpc);
    drop(input);
    drop(output);
    assert!(child.wait().unwrap().success());
}
