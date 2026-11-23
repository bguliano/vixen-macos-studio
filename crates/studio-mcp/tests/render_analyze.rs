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
/// B78 through the bridge: render_analyze catches the two things Braden rejected sequences for, a dark intro and
/// "spazzing" around 1:00, plus a blue section, colours off the palette and changes on no beat, with times to jump to;
/// steady passages and a pulse on every beat are not flagged.
#[test]
fn render_analyze_flags_a_dark_intro_and_spazzing_at_one_minute() {
    let target = support::target_dir();
    let source = target.join("show-copy");
    let profile = ProfileDir(target.join(format!("studio-mcp-taste-{}", std::process::id())));
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

    // A 70 s sequence on an 8 × 8 tree, beats every 500 ms from 8 s:
    // 0..8 dark, 8..30 red, 30..40 blue, 40..58 white with a green blip off the beat at 45.23 s,
    // 58..64 red/green flipping every 100 ms (the spazz), 64..70 a white pulse on every beat.
    let preview = call!("preview_list", json!({}))["items"][0]["id"].as_str().unwrap().to_string();
    let tree = call!("element_template", json!({"kind":"matrix","name":"Taste Tree","count":8,"pixels":8,"previewId":preview}))["result"]["id"].clone();
    let sequence = call!("sequence_create", json!({"lengthMs":70000}))["sequenceId"].clone();
    let beats: Vec<f64> = (16..140).map(|i| i as f64 * 500.0).collect();
    call!("marks_create", json!({"sequenceId":sequence,"name":"Beats","timesMs":beats}));
    let catalog = call!("effect_catalog", json!({"limit":500}));
    let pulse = catalog["items"].as_array().unwrap().iter().find(|e| e["name"] == "Pulse").unwrap()["id"].clone();
    let flat = |start: f64, duration: f64, color: &str| {
        json!({"op":"add","typeId":pulse,"targetId":tree,"startMs":start,"durationMs":duration,"params":{"LevelCurve":[[0,100],[100,100]],"ColorGradient":[color]}})
    };
    let mut ops = vec![
        flat(8000.0, 22000.0, "#ff0000"),
        flat(30000.0, 10000.0, "#0000ff"),
        flat(40000.0, 5230.0, "#ffffff"),
        flat(45230.0, 400.0, "#00ff00"),
        flat(45630.0, 12370.0, "#ffffff"),
    ];
    ops.extend((0..60).map(|i| flat(58000.0 + i as f64 * 100.0, 100.0, if i % 2 == 0 { "#ff0000" } else { "#00ff00" })));
    ops.extend((0..12).map(|i| flat(64000.0 + i as f64 * 500.0, 250.0, "#ffffff")));
    call!("effects_batch", json!({"sequenceId":sequence,"ops":ops}));
    let effects = call!("sequence_status", json!({"sequenceId":sequence}))["effects"].as_u64().unwrap();
    let undo = call!("sequence_status", json!({"sequenceId":sequence}))["undo"].clone();

    let palette = json!(["#ff0000", "#ffffff"]);
    let report = call!("render_analyze", json!({"sequenceId":sequence,"previewId":preview,"scope":tree,"palette":palette}));
    assert_eq!(report["frames"], 1400, "{report}");
    assert_eq!(report["lights"], 64, "{report}");
    assert_eq!(report["ok"], false, "{report}");
    // The dark intro.
    let dark = report["dark"].as_array().unwrap();
    assert_eq!(dark.len(), 1, "{report}");
    assert_eq!(dark[0]["startMs"], 0.0, "{report}");
    assert!((7950.0..=8100.0).contains(&dark[0]["endMs"].as_f64().unwrap()), "{report}");
    // The spazz around 1:00, and nowhere else: not the steady passages, not the pulse on every beat.
    let spazz = report["changes"]["spazz"].as_array().unwrap();
    assert_eq!(spazz.len(), 1, "{report}");
    let (from, to) = (spazz[0]["startMs"].as_f64().unwrap(), spazz[0]["endMs"].as_f64().unwrap());
    assert!((57900.0..=58200.0).contains(&from) && to >= 63500.0 && to <= 65000.0, "{report}");
    assert!(spazz[0]["peakPerSec"].as_f64().unwrap() >= 9.0, "{report}");
    assert_eq!(report["flags"]["dark"], 1, "{report}");
    assert_eq!(report["flags"]["spazz"], 1, "{report}");
    // Jump points, sorted, in m:ss.cc.
    let jumps = report["jumpTo"].as_array().unwrap();
    assert_eq!((jumps[0]["kind"].as_str(), jumps[0]["time"].as_str()), (Some("dark"), Some("0:00.00")), "{report}");
    let spazz_jump = jumps.iter().find(|j| j["kind"] == "spazz").unwrap();
    assert!(spazz_jump["time"].as_str().unwrap().starts_with("0:58"), "{report}");
    assert!(jumps.windows(2).all(|w| w[0]["timeMs"].as_f64() <= w[1]["timeMs"].as_f64()), "{report}");
    // Sections of 10 s: red, then a blue one off the palette, then green flips off the palette in 60..70.
    let sections = report["sections"].as_array().unwrap();
    assert_eq!(sections.len(), 7, "{report}");
    assert_eq!(sections[1]["dominant"][0]["name"], "red", "{report}");
    assert_eq!(sections[1]["flags"], json!([]), "{report}");
    assert_eq!(sections[3]["dominant"][0]["name"], "blue", "{report}");
    assert_eq!(sections[3]["flags"], json!(["bluePurple", "offPalette"]), "{report}");
    assert_eq!(sections[4]["flags"], json!([]), "a short green blip is under the palette limit: {report}");
    assert_eq!(sections[6]["flags"], json!(["offPalette"]), "{report}");
    // Frames on an effect boundary mix the red and green into yellow, as the preview shows them.
    assert!(sections[6]["offPaletteColors"].as_array().unwrap().iter().any(|c| c["name"] == "green"), "{report}");
    assert!(jumps.iter().any(|j| j["kind"] == "bluePurple" && j["timeMs"] == 30000.0), "{report}");
    // Changes on no beat: the green blip at 45.23 s (first seen in the 45.25 s frame) and back to white 400 ms later,
    // 2 of 17 onsets (the flips are one run of changes, so one onset): under the default 25%, flagged at 10%.
    let marks = &report["marks"];
    assert_eq!(marks["collections"], json!(["Beats"]), "{report}");
    assert_eq!(marks["offMarkMs"], json!([45250.0, 45650.0]), "{report}");
    assert_eq!(marks["flagged"], false, "{report}");
    let strict = call!("render_analyze", json!({"sequenceId":sequence,"previewId":preview,"scope":tree,"startMs":40000,"endMs":50000,"offMarkMax":0.1}));
    assert_eq!(strict["marks"]["flagged"], true, "{strict}");
    assert!(strict["jumpTo"].as_array().unwrap().iter().any(|j| j["kind"] == "offMark" && j["timeMs"] == 45250.0), "{strict}");

    // A steady passage and the pulse on every beat are clean.
    let calm = call!("render_analyze", json!({"sequenceId":sequence,"previewId":preview,"scope":tree,"palette":palette,"startMs":10000,"endMs":28000,"sectionMs":6000}));
    assert_eq!(calm["ok"], true, "{calm}");
    assert_eq!(calm["jumpTo"], json!([]), "{calm}");
    let on_beat = call!("render_analyze", json!({"sequenceId":sequence,"previewId":preview,"scope":tree,"startMs":64000,"endMs":70000}));
    assert_eq!(on_beat["changes"]["spazz"], json!([]), "two changes a beat at 120 bpm is not spazzing: {on_beat}");
    assert_eq!(on_beat["marks"]["offMark"], 0, "{on_beat}");
    assert!(on_beat["marks"]["onsets"].as_u64().unwrap() >= 11, "{on_beat}");
    assert_eq!(on_beat["ok"], true, "{on_beat}");

    // Per prop in the whole preview: the tree's own findings; the preview as a whole is mostly unlit.
    let whole = call!("render_analyze", json!({"sequenceId":sequence,"previewId":preview,"elements":[tree],"marks":[]}));
    assert_eq!(whole["marks"], Value::Null, "{whole}");
    let prop = &whole["elements"][0];
    assert_eq!(prop["name"], "Taste Tree", "{whole}");
    assert_eq!(prop["changes"]["spazz"].as_array().unwrap().len(), 1, "{whole}");
    assert!(whole["jumpTo"].as_array().unwrap().iter().any(|j| j["kind"] == "spazz" && j["element"] == "Taste Tree"), "{whole}");
    // Sections from a mark collection, named by mark text.
    call!("marks_create", json!({"sequenceId":sequence,"name":"Sections","timesMs":[0,30000,58000],"texts":["Intro","Verse","Drop"]}));
    let named = call!("render_analyze", json!({"sequenceId":sequence,"previewId":preview,"scope":tree,"sections":"Sections","marks":["Beats"]}));
    let names: Vec<&str> = named["sections"].as_array().unwrap().iter().map(|s| s["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Intro", "Verse", "Drop"], "{named}");

    // Bad input is refused; the tool changed nothing.
    assert!(try_call("render_analyze", json!({"sequenceId":sequence,"previewId":preview,"palette":["blue"]})).unwrap_err().contains("palette"));
    assert!(try_call("render_analyze", json!({"sequenceId":sequence,"previewId":preview,"marks":["Nope"]})).unwrap_err().contains("not found"));
    assert!(try_call("render_analyze", json!({"sequenceId":sequence,"previewId":preview,"sections":"Sections","sectionMs":5000})).unwrap_err().contains("not both"));
    assert!(try_call("render_analyze", json!({"sequenceId":sequence,"previewId":preview,"fps":2})).unwrap_err().contains("fps"));
    let after = call!("sequence_status", json!({"sequenceId":sequence}));
    assert_eq!(after["effects"].as_u64().unwrap(), effects);
    assert_eq!(after["undo"].as_array().unwrap().len(), undo.as_array().unwrap().len() + 1, "only the Sections marks were added since: {after}");

    drop(try_call);
    drop(rpc);
    drop(input);
    drop(output);
    assert!(child.wait().unwrap().success());
}
