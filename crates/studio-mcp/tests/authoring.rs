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
#[test]
fn real_music_thirty_second_show_via_stdio_only() {
    let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
    let source = target.join("show-copy");
    let root = target.join(format!("studio-mcp-e2e-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    copy_dir(&source.join("SystemData"), &root.join("SystemData"));
    copy_dir(
        &source.join("Module Data Files"),
        &root.join("Module Data Files"),
    );
    std::os::unix::fs::symlink(
        source.join("Media").canonicalize().unwrap(),
        root.join("Media"),
    )
    .unwrap();
    let mut songs: Vec<_> = std::fs::read_dir(root.join("Media"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.to_lowercase().ends_with(".mp3"))
        .collect();
    songs.sort();
    let song = songs
        .first()
        .expect("real show fixture must have music")
        .clone();
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
        writeln!(
            input,
            "{}",
            json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
        )
        .unwrap();
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        let reply: Value = serde_json::from_str(&line)
            .unwrap_or_else(|e| panic!("Non-JSON protocol output: {e}: {line}"));
        assert!(reply.get("error").is_none(), "{reply}");
        reply["result"].clone()
    };
    rpc(
        "initialize",
        json!({"protocolVersion":"2024-11-05","clientInfo":{},"capabilities":{}}),
    );
    let mut call = |name: &str, args: Value| -> Value {
        let result = rpc("tools/call", json!({"name":name,"arguments":args}));
        assert_eq!(result["isError"], false, "{name}: {result}");
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
    };
    let analysis = call("music_analyze", json!({"media":song}));
    let beats: Vec<f64> = analysis["beatsMs"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_f64)
        .filter(|t| *t < 29_500.0)
        .collect();
    assert!(
        beats.len() >= 4,
        "Expected detected real musical beats: {analysis}"
    );
    let created = call("sequence_create", json!({"lengthMs":30000,"media":song}));
    let sequence = created["sequenceId"].clone();
    let elements = call("display_elements", json!({"limit":500}));
    let mut element = elements["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["elementId"].is_string())
        .expect("leaf element")["id"]
        .clone();
    let initial_previews = call("preview_list", json!({}));
    if let Some(id) = initial_previews["items"][0]["linkedNodeIds"]
        .as_array()
        .and_then(|a| a.first())
    {
        element = id.clone();
    }
    let catalog = call("effect_catalog", json!({"limit":500}));
    let effect = catalog["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == "Pulse")
        .expect("Pulse effect")["id"]
        .clone();
    let selected: Vec<_> = beats.iter().step_by(2).take(20).copied().collect();
    for time in &selected {
        call(
            "effect_add",
            json!({"sequenceId":sequence,"typeId":effect,"targetId":element,"startMs":time,"durationMs":250}),
        );
    }
    call(
        "marks_create",
        json!({"sequenceId":sequence,"name":"Detected beats","timesMs":beats}),
    );
    let rendered = call("sequence_render", json!({"sequenceId":sequence}));
    assert_eq!(rendered["effects"].as_u64().unwrap(), selected.len() as u64);
    assert_eq!(rendered["rendered"], rendered["effects"]);
    assert!(
        rendered["scheduleEntries"].as_u64().unwrap() > 0,
        "{rendered}"
    );
    // Onsets as editable marks, then a Pulse on every strong low hit, snapped to the 16th grid, in one undo step.
    let onsets = call("sequence_onset_marks", json!({"sequenceId":sequence}));
    let low = onsets["collections"][0].clone();
    assert!(low["name"] == "Onsets - low" && low["marks"].as_u64().unwrap() > 4, "{onsets}");
    call("sequence_music_marks", json!({"sequenceId":sequence,"media":song}));
    let placed = call(
        "effects_on_marks",
        json!({"sequenceId":sequence,"markCollection":low["id"],"minStrength":0.3,"snap":"16th","template":{"typeId":effect,"targetId":element},"levelFromStrength":true,"maxMs":400}),
    );
    let placed_count = placed["effectIds"].as_array().unwrap().len() as u64;
    assert!(placed_count > 4, "{placed}");
    let rendered_hits = call("sequence_render", json!({"sequenceId":sequence}));
    assert_eq!(rendered_hits["effects"].as_u64().unwrap(), selected.len() as u64 + placed_count);
    call("sequence_undo", json!({"sequenceId":sequence}));
    let status = call("sequence_status", json!({"sequenceId":sequence}));
    assert_eq!(status["effects"].as_u64().unwrap(), selected.len() as u64, "{status}");
    call(
        "sequence_save",
        json!({"sequenceId":sequence,"file":"MCP-music-test.tim","confirm":true}),
    );
    call(
        "sequence_export_fseq",
        json!({"sequenceId":sequence,"file":"MCP-music-test.fseq","confirm":true}),
    );
    let previews = call("preview_list", json!({}));
    if let Some(preview) = previews["items"].as_array().unwrap().first() {
        let frame = call(
            "preview_frame",
            json!({"sequenceId":sequence,"previewId":preview["id"],"timeMs":selected[0]+100.0,"file":"MCP-music-test.png","confirm":true}),
        );
        assert!(frame["lights"].as_u64().unwrap() > 0);
        assert!(frame["illuminated"].as_u64().unwrap() > 0, "{frame}");
        assert!(frame["width"].as_u64().unwrap() >= 64);
        let start = (selected[0] - 1000.0).max(0.0);
        let video = call(
            "preview_video",
            json!({"sequenceId":sequence,"previewId":preview["id"],"startMs":start,"endMs":start+4000.0,"fps":25,"glow":true,"file":"MCP-music-test.mp4","confirm":true}),
        );
        assert_eq!(video["frames"], 100, "{video}");
        assert_eq!(video["audio"], song.as_str(), "{video}");
        assert!(video["maxIlluminated"].as_u64().unwrap() > 0, "{video}");
        let mp4 = std::fs::read(root.join("Export/MCP-music-test.mp4")).unwrap();
        assert_eq!(&mp4[4..8], b"ftyp");
        let sheet = call(
            "preview_sheet",
            json!({"sequenceId":sequence,"previewId":preview["id"],"startMs":start,"endMs":start+4000.0,"fps":4,"cols":4,"file":"MCP-music-test-sheet.png","confirm":true}),
        );
        assert_eq!(sheet["tiles"], 16, "{sheet}");
        assert_eq!(sheet["rows"], 4, "{sheet}");
        let png = std::fs::read(root.join("Export/MCP-music-test-sheet.png")).unwrap();
        let png_width = u32::from_be_bytes(png[16..20].try_into().unwrap());
        assert_eq!(u64::from(png_width), sheet["width"].as_u64().unwrap());
        assert!(std::fs::read_dir(root.join("Export"))
            .unwrap()
            .all(|e| !e.unwrap().file_name().to_string_lossy().contains(".partial.")));
    }
    let show = call(
        "show_create",
        json!({"name":"MCP test show","sequences":["MCP-music-test.tim"]}),
    );
    call(
        "schedule_create",
        json!({"showId":show["result"]["showId"],"startDate":"2026-12-01","endDate":"2026-12-31","startTime":"18:00","endTime":"21:00"}),
    );
    let schedules = call("schedule_list", json!({}));
    assert!(schedules["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["showId"] == show["result"]["showId"] && item["enabled"] == false && item["startTime"] == "18:00"));
    // Image maps through the bridge: a new map with notes, listed, then undone off the shared history.
    let map = call("image_map_create", json!({"name":"MCP test mouths"}));
    call("image_map_update", json!({"name":map["result"]["name"],"notes":"From the bridge"}));
    let maps = call("image_map_list", json!({}));
    assert!(maps["maps"].as_array().unwrap().iter().any(|m| m["name"] == "MCP test mouths" && m["notes"] == "From the bridge" && m["editable"] == true), "{maps}");
    call("display_undo", json!({}));
    call("display_undo", json!({}));
    assert!(!call("image_map_list", json!({}))["maps"].as_array().unwrap().iter().any(|m| m["name"] == "MCP test mouths"));
    if let Some(preview) = previews["items"].as_array().unwrap().first() {
        let prop = call(
            "preview_prop_create",
            json!({"previewId":preview["id"],"name":"MCP test arch","kind":"arch"}),
        );
        call(
            "preview_prop_link",
            json!({"previewId":preview["id"],"propId":prop["result"]["propId"],"nodeIds":[element],"expectedRevision":prop["revision"]}),
        );
    }
    call("profile_save", json!({"confirm":true}));
    drop(call);
    drop(rpc);
    drop(input);
    drop(output);
    assert!(child.wait().unwrap().success());
    assert!(
        std::fs::metadata(root.join("Sequence/MCP-music-test.tim"))
            .unwrap()
            .len()
            > 0
    );
    assert!(
        std::fs::metadata(root.join("Export/MCP-music-test.fseq"))
            .unwrap()
            .len()
            > 32
    );
    println!(
        "MCP authored and rendered {} beat-synced effects over 30000 ms; TIM/FSEQ artifacts in {}",
        selected.len(),
        root.display()
    );
}
