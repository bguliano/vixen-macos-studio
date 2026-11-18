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
    }
    let show = call(
        "show_create",
        json!({"name":"MCP test show","sequences":["MCP-music-test.tim"]}),
    );
    call(
        "schedule_create",
        json!({"id":show["result"]["id"],"start":"2026-12-01T18:00:00","end":"2026-12-31T21:00:00","days":[true,true,true,true,true,true,true]}),
    );
    let schedules = call("schedule_list", json!({}));
    assert!(schedules["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["showId"] == show["result"]["id"] && item["enabled"] == false));
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
