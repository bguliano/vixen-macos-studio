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
/// B76 through the bridge, measured on the preview's own frames: a feathered band fades its inner edges and keeps the
/// prop's end lit, featherLights is rows of the prop's lights, and an exclusive band cuts what lights its lights during its
/// time (splitting, removing) but leaves other rows and other props alone, all undone in one step.
#[test]
fn feathered_and_exclusive_bands_measured_on_preview_frames() {
    let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
    let source = target.join("show-copy");
    let profile = ProfileDir(target.join(format!("studio-mcp-bands-{}", std::process::id())));
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

    // A 21 × 21 matrix linked in the preview, with Locations from it: rows at every 5% of its height.
    let preview = call!("preview_list", json!({}))["items"][0]["id"].as_str().unwrap().to_string();
    let tree = call!("element_template", json!({"kind":"matrix","name":"Band Grid","count":21,"pixels":21,"previewId":preview}))["result"]["id"].clone();
    call!("element_location", json!({"id":tree,"previewId":preview}));
    let arch = call!("element_template", json!({"kind":"arch","name":"Band Arch","count":6}))["result"]["id"].clone();
    let geometry = call!("display_geometry", json!({"previewId":preview,"elementId":tree,"limit":5000}));
    let pixels: Vec<(String, f64)> = geometry["pixels"].as_array().unwrap().iter().map(|p| (p["nodeId"].as_str().unwrap().to_string(), p["y"].as_f64().unwrap())).collect();
    assert_eq!(pixels.len(), 441, "{geometry}");
    let (min_y, max_y) = pixels.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p.1), hi.max(p.1)));
    // One light per row, bottom (0%) to top (100%).
    let mut rows: Vec<(String, f64)> = Vec::new();
    for (id, y) in &pixels {
        let h = ((max_y - y) / (max_y - min_y) * 100.0).round();
        if !rows.iter().any(|r| r.1 == h) {
            rows.push((id.clone(), h));
        }
    }
    rows.sort_by(|a, b| a.1.total_cmp(&b.1));
    assert_eq!(rows.len(), 21, "{rows:?}");
    let ids: Vec<&str> = rows.iter().map(|r| r.0.as_str()).collect();

    let sequence = call!("sequence_create", json!({"lengthMs":8000}))["sequenceId"].clone();
    let band = |start: f64, extra: Value| {
        let mut a = json!({"sequenceId":sequence,"targetId":tree,"startMs":start,"durationMs":1000,"color":"#ff0000"});
        a.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        a
    };
    // Which rows the preview shows lit at `level` or brighter, 100 ms into a moment.
    macro_rules! lit_rows {
        ($at:expr, $level:expr) => {{
            call!("sequence_render", json!({"sequenceId":sequence}));
            let active = call!("sequence_activity", json!({"sequenceId":sequence,"previewId":preview,"startMs":$at,"endMs":$at+100.0,"fps":20,"level":$level,"minMs":0,"elements":ids}));
            rows.iter().zip(active["elements"].as_array().unwrap()).filter(|(_, e)| e["lit"]["max"].as_f64().unwrap() > 0.5).map(|(r, _)| r.1).collect::<Vec<f64>>()
        }};
    }
    macro_rules! count {
        () => {
            call!("sequence_status", json!({"sequenceId":sequence}))["effects"].as_u64().unwrap()
        };
    }

    // 30..70 feathered 20%: full from 40 to 60, fading to dark at 20 and 80, half at the edges themselves.
    call!("effect_band", band(0.0, json!({"fromPct":30,"toPct":70,"featherPct":20})));
    let bright = lit_rows!(400.0, 0.85);
    let half = lit_rows!(400.0, 0.4);
    let any = lit_rows!(400.0, 0.05);
    // Rows land on the preview's pixel grid, a point or two off every 5%: check bands of height, not exact rows.
    let heights = |lo: f64, hi: f64| -> Vec<f64> { rows.iter().map(|r| r.1).filter(|h| (lo..=hi).contains(h)).collect() };
    let covers = |lit: &[f64], lo: f64, hi: f64| heights(lo, hi).iter().all(|h| lit.contains(h));
    let within = |lit: &[f64], lo: f64, hi: f64| lit.iter().all(|h| (lo..=hi).contains(h));
    let touches = |lit: &[f64], lo: f64, hi: f64| lit.iter().any(|h| (lo..=hi).contains(h));
    assert!(within(&bright, 34.0, 66.0) && covers(&bright, 39.0, 61.0), "full core: {bright:?}");
    assert!(within(&half, 29.0, 71.0) && touches(&half, 31.0, 38.0) && touches(&half, 62.0, 69.0), "half: {half:?}");
    assert!(within(&any, 17.0, 83.0) && touches(&any, 21.0, 28.0) && touches(&any, 72.0, 79.0), "fade reaches 20..80: {any:?}");
    let frame = call!("preview_frame", json!({"sequenceId":sequence,"previewId":preview,"timeMs":400,"file":"B76-feather.png","confirm":true}));
    assert!(frame["illuminated"].as_u64().unwrap() > 0, "{frame}");

    // The same fade as rows of the grid's own lights: 4 rows of 20 gaps is 20%.
    call!("effect_band", band(1000.0, json!({"fromPct":30,"toPct":70,"featherLights":4})));
    for level in [0.85, 0.4, 0.05] {
        assert_eq!(lit_rows!(1400.0, level), lit_rows!(400.0, level), "featherLights 4 = featherPct 20 at {level}");
    }

    // A band from the bottom never fades there: the lowest light stays at full level (#63).
    call!("effect_band", band(2000.0, json!({"fromPct":0,"toPct":50,"featherPct":20})));
    let bottom = lit_rows!(2400.0, 0.85);
    assert!(covers(&bottom, 0.0, 36.0) && within(&bottom, 0.0, 46.0), "no dark end: {bottom:?}");

    // Bad feathers add nothing.
    let before = count!();
    assert!(try_call("effect_band", band(0.0, json!({"fromPct":40,"toPct":50,"featherPct":20}))).unwrap_err().contains("at most the band's width"));
    assert!(try_call("effect_band", band(0.0, json!({"fromPct":40,"toPct":60,"featherPct":5,"featherLights":1}))).unwrap_err().contains("not both"));
    assert_eq!(count!(), before);

    // Exclusive: older effects on the grid's lower rows during 3800..4800 are cut; other rows and other props are not.
    let catalog = call!("effect_catalog", json!({"limit":500}));
    let pulse = catalog["items"].as_array().unwrap().iter().find(|e| e["name"] == "Pulse").unwrap()["id"].clone();
    let flat = json!({"LevelCurve":[[0,100],[100,100]]});
    let whole = call!("effect_add", json!({"sequenceId":sequence,"typeId":pulse,"targetId":tree,"startMs":3000,"durationMs":3000,"params":flat}))["effectId"].clone();
    let arch_pulse = call!("effect_add", json!({"sequenceId":sequence,"typeId":pulse,"targetId":arch,"startMs":3000,"durationMs":3000,"params":flat}))["effectId"].clone();
    let high = call!("effect_band", json!({"sequenceId":sequence,"targetId":tree,"startMs":3000,"durationMs":3000,"fromPct":60,"toPct":90}))["effectId"].clone();
    let inside = call!("effect_band", json!({"sequenceId":sequence,"targetId":tree,"startMs":4000,"durationMs":500,"fromPct":20,"toPct":50}))["effectId"].clone();
    let before = count!();
    let effects_before = call!("sequence_effects", json!({"sequenceId":sequence}));
    let placed = call!("effect_band", json!({"sequenceId":sequence,"targetId":tree,"startMs":3800,"durationMs":1000,"fromPct":0,"toPct":30,"featherPct":10,"exclusive":true}));
    assert_eq!(placed["cut"], 2, "{placed}");
    assert_eq!(count!(), before + 1, "one removed, one copy, one band");
    let effects = call!("sequence_effects", json!({"sequenceId":sequence}))["effects"].as_array().unwrap().clone();
    let times = |id: &Value| effects.iter().find(|e| &e["id"] == id).map(|e| (e["startMs"].as_f64().unwrap(), e["durationMs"].as_f64().unwrap()));
    assert_eq!(times(&whole), Some((3000.0, 800.0)), "the whole grid's Pulse ends where the band starts");
    let whole_target = effects.iter().find(|e| e["id"] == whole).unwrap()["targets"].clone();
    assert!(effects.iter().any(|e| e["targets"] == whole_target && e["startMs"] == 4800.0 && e["durationMs"] == 1200.0), "and resumes after it");
    assert_eq!(times(&inside), None, "the band wholly inside is removed");
    assert_eq!(times(&high), Some((3000.0, 3000.0)), "rows 60..90 are not the band's");
    assert_eq!(times(&arch_pulse), Some((3000.0, 3000.0)), "another prop is not the band's");
    let during = lit_rows!(4200.0, 0.4);
    assert!(covers(&during, 0.0, 26.0), "the band: {during:?}");
    assert!(!touches(&during, 34.0, 56.0) && !touches(&during, 94.0, 100.0), "nothing else on the lower rows: {during:?}");
    assert!(covers(&during, 64.0, 86.0), "the 60..90 band carries on: {during:?}");
    assert!(covers(&lit_rows!(3500.0, 0.4), 0.0, 100.0), "the grid's Pulse before the cut");
    assert!(covers(&lit_rows!(5000.0, 0.4), 0.0, 100.0), "and after it");
    call!("preview_frame", json!({"sequenceId":sequence,"previewId":preview,"timeMs":4200,"file":"B76-exclusive.png","confirm":true}));

    // One undo puts everything back.
    call!("sequence_undo", json!({"sequenceId":sequence}));
    assert_eq!(call!("sequence_effects", json!({"sequenceId":sequence})), effects_before);

    drop(try_call);
    drop(rpc);
    drop(input);
    drop(output);
    assert!(child.wait().unwrap().success());
    if let Ok(keep) = std::env::var("B76_KEEP_FRAMES") {
        for name in ["B76-feather.png", "B76-exclusive.png"] {
            let _ = std::fs::copy(root.join("Export").join(name), Path::new(&keep).join(name));
        }
    }
}
