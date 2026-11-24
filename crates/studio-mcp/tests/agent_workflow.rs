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
/// B73 through the bridge, on the show copy's real display: display_layout finds the props, which are one channel and
/// which colours they can show, in left-to-right order with mirror pairs; tools take a section name in place of a range;
/// effects_find, effects_edit and passage_copy work by section (copies repeat to fill a longer section, retarget, and
/// undo in one step); sequence_summary flags empty sections, duplicates and props that stay dark; an effect ending a
/// hair past the sequence end is trimmed instead of refused.
#[test]
fn agent_workflow_tools_work_by_section_and_layout() {
    let target = support::target_dir();
    let source = target.join("show-copy");
    let profile = ProfileDir(target.join(format!("studio-mcp-agent-workflow-{}", std::process::id())));
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
    for (name, read_only) in [("display_layout", true), ("effects_find", true), ("effects_edit", false), ("passage_copy", false), ("sequence_summary", true)] {
        let tool = tools.iter().find(|t| t["name"] == name).unwrap_or_else(|| panic!("{name} is listed"));
        assert_eq!(tool["annotations"]["readOnlyHint"], read_only, "{name}");
        assert!(tool["description"].as_str().unwrap().contains("Example"), "{name} has an example");
    }
    for name in ["effects_on_marks", "motif_place", "effect_step"] {
        let tool = tools.iter().find(|t| t["name"] == name).unwrap();
        assert!(tool["inputSchema"]["properties"].get("section").is_some(), "{name} takes section");
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

    // The display: one-channel, fixed-colour relay strings and two pixel strips, ordered and paired in space.
    let layout = call!("display_layout", json!({}));
    let props = layout["props"].as_array().unwrap();
    let prop = |name: &str| props.iter().find(|p| p["name"] == name).unwrap_or_else(|| panic!("{name} in {layout}")).clone();
    let (mini_l, mini_r, star, strip_l) = (prop("Mini Tree L"), prop("Mini Tree R"), prop("Star"), prop("Light Strip L"));
    assert_eq!((mini_l["channels"].as_u64(), mini_l["control"].as_str(), mini_l["colors"].clone()), (Some(1), Some("one channel"), json!(["#ffffff"])), "{mini_l}");
    assert_eq!((strip_l["control"].as_str(), strip_l["colors"].as_str()), (Some("pixels"), Some("full")), "{strip_l}");
    assert_eq!(star["colors"], json!(["#0000ff"]));
    assert!(mini_l["centre"]["xPct"].as_f64().unwrap() < 50.0 && mini_r["centre"]["xPct"].as_f64().unwrap() > 50.0);
    let order = &layout["order"]["leftToRight"];
    assert_eq!(order[0], mini_l["elementId"], "{order}");
    assert_eq!(order[order.as_array().unwrap().len() - 1], mini_r["elementId"]);
    let outermost = layout["order"]["insideOut"].as_array().unwrap().last().unwrap().clone();
    assert!(outermost == mini_l["elementId"] || outermost == mini_r["elementId"], "{}", layout["order"]["insideOut"]);
    assert!(layout["mirrorPairs"].as_array().unwrap().iter().any(|p| p["left"]["name"] == "Mini Tree L" && p["right"]["name"] == "Mini Tree R"), "{}", layout["mirrorPairs"]);
    let (l, r, star) = (mini_l["elementId"].clone(), mini_r["elementId"].clone(), star["elementId"].clone());
    let preview = layout["previewId"].clone();

    let catalog = call!("effect_catalog", json!({}));
    let pulse = catalog["items"].as_array().unwrap().iter().find(|e| e["name"] == "Pulse").unwrap()["id"].clone();
    let add = |target: &Value, start: f64, len: f64, color: &str| json!({"op":"add","typeId":pulse,"targetId":target,"startMs":start,"durationMs":len,"params":{"ColorGradient":[color]}});
    let id = call!("sequence_create", json!({"lengthMs":20000}))["sequenceId"].clone();
    call!("sequence_section_marks", json!({"sequenceId":id,"sections":[{"name":"Intro","startMs":0},{"name":"Chorus 1","startMs":4000},{"name":"Verse","startMs":8000},{"name":"Chorus 2","startMs":12000}]}));
    // Chorus 1: two white hits on the mini trees and a white pulse on the blue-only star, which can't show it.
    call!("effects_batch", json!({"sequenceId":id,"ops":[add(&l,4000.0,1000.0,"#ffffff"),add(&r,5000.0,1000.0,"#ffffff"),add(&star,4500.0,2000.0,"#ffffff")]}));

    // effects_find by section pattern, type and target.
    let found = call!("effects_find", json!({"sequenceId":id,"section":"Chorus*"}));
    assert_eq!((found["total"].as_u64(), found["byType"]["Pulse"].as_u64()), (Some(3), Some(3)), "{found}");
    assert_eq!(call!("effects_find", json!({"sequenceId":id,"section":"chorus 1","targetIds":[r]}))["total"], 1);
    assert!(try_call("effects_find", json!({"sequenceId":id,"section":"Bridge"})).unwrap_err().contains("the sections are: Intro, Chorus 1, Verse, Chorus 2"));

    // sequence_summary: empty sections and the dark star, from the rendered schedule.
    let summary = call!("sequence_summary", json!({"sequenceId":id,"previewId":preview}));
    let problem = |s: &Value, kind: &str| s["problems"].as_array().unwrap().iter().find(|p| p["kind"] == kind).cloned();
    assert_eq!(problem(&summary, "emptySections").unwrap()["count"], 3, "{summary}");
    let chorus = &summary["sections"][1];
    assert_eq!((chorus["name"].as_str(), chorus["effects"].as_u64()), (Some("Chorus 1"), Some(3)));
    assert_eq!(chorus["darkProps"], json!([{"id":star,"name":"Star"}]), "{chorus}");
    assert!(problem(&summary, "duplicates").is_none());

    // effects_edit: the star in its own colour; then it is no longer dark. No filter, no edit.
    assert!(try_call("effects_edit", json!({"sequenceId":id,"color":"#0000ff"})).unwrap_err().contains("Give a filter"));
    let edited = call!("effects_edit", json!({"sequenceId":id,"section":"Chorus 1","targetIds":[star],"color":"#0000ff"}));
    assert_eq!(edited["edited"], 1, "{edited}");
    let summary = call!("sequence_summary", json!({"sequenceId":id,"previewId":preview}));
    assert!(summary["sections"][1].get("darkProps").is_none(), "{summary}");
    // levelScale halves the pulse's curve.
    call!("effects_edit", json!({"sequenceId":id,"range":[4000,4001],"types":["Pulse"],"levelScale":0.5}));
    let first = call!("effects_find", json!({"sequenceId":id,"range":[4000,4001]}))["effects"][0]["id"].clone();
    let params = call!("effect_parameters", json!({"sequenceId":id,"effectId":first}));
    let curve = params["parameters"].as_array().unwrap().iter().find(|p| p["name"] == "LevelCurve").unwrap()["value"].to_string();
    assert!(curve.contains("50"), "{curve}");

    // passage_copy: Chorus 1 (4 s) fills Chorus 2 (8 s) twice over; dry run first changes nothing; one undo takes it back.
    let plan = call!("passage_copy", json!({"sequenceId":id,"section":"Chorus 1","toSections":"Chorus*","repeat":true,"dryRun":true}));
    assert_eq!((plan["added"].as_u64(), plan["destinations"][0]["passes"].as_u64()), (Some(6), Some(2)), "{plan}");
    assert_eq!(call!("effects_find", json!({"sequenceId":id}))["total"], 3);
    let copied = call!("passage_copy", json!({"sequenceId":id,"section":"Chorus 1","toSections":"Chorus*","repeat":true,"buildId":"chorus v1"}));
    assert_eq!(copied["added"], 6, "{copied}");
    let in_chorus2 = call!("effects_find", json!({"sequenceId":id,"section":"Chorus 2","fromBuild":"chorus v1"}));
    let starts: Vec<f64> = in_chorus2["effects"].as_array().unwrap().iter().map(|e| e["startMs"].as_f64().unwrap()).collect();
    assert_eq!(starts, [12000.0, 12500.0, 13000.0, 16000.0, 16500.0, 17000.0], "{in_chorus2}");
    call!("sequence_undo", json!({"sequenceId":id}));
    assert_eq!(call!("effects_find", json!({"sequenceId":id}))["total"], 3, "one undo step");

    // Mirrored into the verse: Mini Tree L's hit lands on Mini Tree R.
    call!("passage_copy", json!({"sequenceId":id,"section":"Chorus 1","toMs":[8000],"targetMap":{l.as_str().unwrap():r}}));
    let verse = call!("effects_find", json!({"sequenceId":id,"section":"Verse"}));
    assert_eq!((verse["total"].as_u64(), verse["byTarget"]["Mini Tree R"].as_u64()), (Some(3), Some(2)), "{verse}");

    // A placement run twice shows up as duplicates; deleting needs confirm.
    let twice = json!({"sequenceId":id,"ops":[add(&l,1000.0,500.0,"#ffffff")]});
    call!("effects_batch", twice.clone());
    call!("effects_batch", twice);
    let summary = call!("sequence_summary", json!({"sequenceId":id}));
    assert_eq!(problem(&summary, "duplicates").unwrap()["count"], 1, "{summary}");
    assert!(try_call("effects_edit", json!({"sequenceId":id,"section":"Intro","delete":true})).unwrap_err().contains("confirm"));
    assert_eq!(call!("effects_edit", json!({"sequenceId":id,"section":"Intro","delete":true,"confirm":true}))["removed"], 2);

    // A section name in place of a range for mark-driven tools.
    call!("marks_create", json!({"sequenceId":id,"name":"Hits","timesMs":[1000,5000,9000,10000,13000]}));
    let placed = call!("effects_on_marks", json!({"sequenceId":id,"markCollection":"Hits","section":"Verse","template":{"typeId":pulse,"targetId":l}}));
    assert_eq!(placed["effectIds"].as_array().unwrap().len(), 2, "{placed}");
    assert!(try_call("effects_on_marks", json!({"sequenceId":id,"markCollection":"Hits","section":"Chorus*","template":{"typeId":pulse,"targetId":l}})).unwrap_err().contains("matches Chorus 1, Chorus 2"));

    // An effect ending within a millisecond past the end (a section end that is the length, give or take rounding).
    let end = call!("effects_batch", json!({"sequenceId":id,"ops":[{"op":"add","typeId":pulse,"targetId":l,"startMs":19000,"durationMs":1000.4}]}));
    let last = call!("effects_find", json!({"sequenceId":id,"range":[19000,20000]}));
    assert_eq!((end["added"].as_u64(), last["effects"][0]["durationMs"].as_f64()), (Some(1), Some(1000.0)), "{last}");

    drop(try_call);
    drop(rpc);
    drop(input);
    drop(output);
    assert!(child.wait().unwrap().success());
}
