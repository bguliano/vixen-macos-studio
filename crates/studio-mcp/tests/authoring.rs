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
#[test]
fn real_music_thirty_second_show_via_stdio_only() {
    let target = support::target_dir();
    let source = target.join("show-copy");
    let profile = ProfileDir(target.join(format!("studio-mcp-e2e-{}", std::process::id())));
    let root = profile.0.clone();
    let _ = std::fs::remove_dir_all(&root);
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
    // A motif on the same hits: swoosh then glow per hit, previewed by a dry run, placed and undone as one step (B75).
    let motifs = call("motif_list", json!({}));
    assert_eq!(motifs["motifs"].as_array().unwrap().iter().filter(|m| m["builtIn"] == true).count(), 8, "{motifs}");
    let motif = json!({"sequenceId":sequence,"markCollection":low["id"],"minStrength":0.3,"motif":"swoosh_glow","targetSets":[{"targetId":element}],"ramp":[60,100],"maxMs":400});
    let mut dry = motif.clone();
    dry["dryRun"] = json!(true);
    let plan = call("motif_place", dry);
    let events = plan["events"].as_u64().unwrap();
    assert!(events > 4 && plan["total"].as_u64().unwrap() == events * 2, "{plan}");
    assert!(plan["effects"].as_array().unwrap().iter().all(|e| e["durationMs"].as_f64().unwrap() <= 400.0), "{plan}");
    let placed_motif = call("motif_place", motif);
    assert_eq!(placed_motif["effectIds"].as_array().unwrap().len() as u64, events * 2, "{placed_motif}");
    let rendered_motif = call("sequence_render", json!({"sequenceId":sequence}));
    assert_eq!(rendered_motif["effects"].as_u64().unwrap(), selected.len() as u64 + events * 2);
    call("sequence_undo", json!({"sequenceId":sequence}));
    let status = call("sequence_status", json!({"sequenceId":sequence}));
    assert_eq!(status["effects"].as_u64().unwrap(), selected.len() as u64, "{status}");
    // Rhythm by bar, then named sections written as marks, read back by music_rhythm and undone in one step (B77).
    let rhythm = call("music_rhythm", json!({"sequenceId":sequence,"rangeMs":[0,30000]}));
    let bars = rhythm["bars"].as_array().unwrap();
    assert!(bars.len() >= 4 && bars[0]["low"].as_str().unwrap().len() == 19, "{rhythm}");
    assert!(bars.windows(2).all(|w| w[1]["bar"].as_u64().unwrap() == w[0]["bar"].as_u64().unwrap() + 1), "{rhythm}");
    assert!(rhythm["tempoMap"].as_array().unwrap().iter().all(|t| (40.0..250.0).contains(&t["bpm"].as_f64().unwrap())), "{rhythm}");
    assert!(rhythm["sections"].as_array().unwrap().iter().all(|s| s["source"] == "estimate"), "{rhythm}");
    assert!(!rhythm["energy"]["db"].as_array().unwrap().is_empty() && rhythm["onsets"]["low"].is_array(), "{rhythm}");
    let written = call("sequence_section_marks", json!({"sequenceId":sequence,"sections":[{"name":"Intro","startMs":0},{"name":"Verse 1","bar":3}]}));
    let verse = &written["sections"][1];
    assert_eq!(verse["startBar"], 3, "{written}");
    assert_eq!(verse["startMs"].as_f64().unwrap().round(), bars[2]["downbeatMs"].as_f64().unwrap().round(), "{written}");
    let reread = call("music_rhythm", json!({"sequenceId":sequence,"include":["sections"]}));
    let names: Vec<&str> = reread["sections"].as_array().unwrap().iter().map(|s| s["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Intro", "Verse 1"], "{reread}");
    call("sequence_undo", json!({"sequenceId":sequence}));
    let undone = call("music_rhythm", json!({"sequenceId":sequence,"include":["sections"]}));
    assert!(undone["sections"].as_array().unwrap().iter().all(|s| s["source"] == "estimate"), "{undone}");
    call(
        "sequence_save",
        json!({"sequenceId":sequence,"file":"MCP-music-test.tim","confirm":true}),
    );
    // Porting an old show onto this one (B79): the saved file's effects, lag taken out and snapped to the song's 16ths, as a
    // dry run, then for real as one undo step; its timing as zone marks on the same grid.
    let old_show = root.join("Sequence/MCP-music-test.tim").to_string_lossy().into_owned();
    let survey = call("effects_port_from_sequence", json!({"sequenceId":sequence,"path":old_show,"offsetMs":-35,"grid":"16th","dryRun":true}));
    let map: Vec<Value> = survey["zones"].as_array().unwrap().iter().map(|z| json!({"from":z["name"],"targetId":element})).collect();
    let moved = &survey["moves"];
    assert_eq!(moved["events"].as_u64().unwrap(), selected.len() as u64, "{survey}");
    assert!(moved["snapped"].as_u64().unwrap() + moved["alreadyOnGrid"].as_u64().unwrap() == selected.len() as u64, "{survey}");
    let sixteenth = 60_000.0 / 40.0 / 4.0;
    assert!(moved["maxSnapMs"].as_f64().unwrap() <= sixteenth / 2.0, "a 16th snap moves at most half a 16th: {survey}");
    let rhythm = call("music_rhythm", json!({"sequenceId":sequence,"include":["bars"]}));
    let bars = rhythm["bars"].as_array().unwrap();
    let on_grid = |t: f64| bars.iter().any(|b| {
        let (start, length) = (b["downbeatMs"].as_f64().unwrap(), b["lengthMs"].as_f64().unwrap());
        let k = ((t - start) / (length / 16.0)).round();
        (0.0..=16.0).contains(&k) && (start + k * length / 16.0 - t).abs() < 1.0
    });
    assert!(moved["furthest"].as_array().unwrap().iter().all(|m| on_grid(m["toMs"].as_f64().unwrap())), "{survey}");
    let ported = call("effects_port_from_sequence", json!({"sequenceId":sequence,"path":old_show,"offsetMs":-35,"grid":"16th","map":map}));
    let ported_count = ported["effectIds"].as_array().unwrap().len() as u64;
    assert_eq!(ported_count, selected.len() as u64, "{ported}");
    let status = call("sequence_status", json!({"sequenceId":sequence}));
    assert_eq!(status["effects"].as_u64().unwrap(), selected.len() as u64 * 2, "{status}");
    assert!(status["undo"][0].as_str().unwrap().starts_with("Port Effects"), "{status}");
    call("sequence_undo", json!({"sequenceId":sequence}));
    let status = call("sequence_status", json!({"sequenceId":sequence}));
    assert_eq!(status["effects"].as_u64().unwrap(), selected.len() as u64, "{status}");
    let marks = call("marks_import_from_sequence", json!({"sequenceId":sequence,"path":old_show,"includeMarks":false,"offsetMs":-35,"grid":"16th","snapToleranceMs":40,"dryRun":true}));
    assert_eq!(marks["collections"].as_array().unwrap().len(), survey["zones"].as_array().unwrap().len(), "{marks}");
    assert_eq!(marks["moves"]["events"].as_u64().unwrap(), selected.len() as u64, "{marks}");
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
        assert!(video["dark"].is_array() && video["darkThresholds"]["minMs"] == 1000.0, "{video}");
        // Dead air from the schedule's intensities: the Pulsed prop is lit on its beats and dark between them.
        let active = call(
            "sequence_activity",
            json!({"sequenceId":sequence,"previewId":preview["id"],"startMs":start,"endMs":start+4000.0,"elements":[element],"minMs":0,"series":true}),
        );
        assert_eq!(active["frames"], 80, "{active}");
        assert_eq!(active["series"].as_array().unwrap().len(), 80, "{active}");
        assert!(active["lights"].as_u64().unwrap() > 0, "{active}");
        let prop = &active["elements"][0];
        assert!(prop["lights"].as_u64().unwrap() > 0, "{active}");
        assert!(prop["lit"]["max"].as_f64().unwrap() > 0.5, "{active}");
        assert!(prop["lit"]["min"].as_f64().unwrap() < 0.05, "{active}");
        assert!(!prop["dark"].as_array().unwrap().is_empty(), "{active}");
        // A sequence with no effects is one dark stretch from start to end.
        let empty = call("sequence_create", json!({"lengthMs":5000}));
        let quiet = call("sequence_activity", json!({"sequenceId":empty["sequenceId"],"previewId":preview["id"]}));
        assert_eq!(quiet["frames"], 100, "{quiet}");
        assert_eq!(quiet["darkMs"], 5000.0, "{quiet}");
        assert_eq!(quiet["dark"][0]["startMs"], 0.0, "{quiet}");
        assert_eq!(quiet["lit"]["max"], 0.0, "{quiet}");
        call("sequence_close", json!({"sequenceId":empty["sequenceId"],"confirm":true}));
        let mp4 = std::fs::read(root.join("Export/MCP-music-test.mp4")).unwrap();
        assert_eq!(&mp4[4..8], b"ftyp");
        // Drawn by the app's preview renderer at twice the canvas size by default, not upscaled afterwards.
        let canvas_width = frame["width"].as_u64().unwrap();
        assert_eq!(video["width"].as_u64().unwrap(), canvas_width * 2 + canvas_width * 2 % 2, "{video}");
        assert!(video["renderer"].as_str().unwrap().starts_with("app preview renderer"), "{video}");
        assert!(video["codec"].as_str().unwrap().starts_with("H.264 High"), "{video}");
        // Framed on one prop at four times its size.
        let fitted = call(
            "preview_video",
            json!({"sequenceId":sequence,"previewId":preview["id"],"startMs":start,"endMs":start+1000.0,"fit":element,"padding":4,"scale":4,"file":"MCP-music-fit.mp4","confirm":true}),
        );
        assert_eq!(fitted["frames"], 25, "{fitted}");
        assert_eq!(fitted["scale"], 4.0, "{fitted}");
        let crop_width = fitted["crop"][2].as_f64().unwrap();
        assert!(crop_width < canvas_width as f64, "{fitted}");
        assert!((fitted["width"].as_f64().unwrap() - crop_width * 4.0).abs() <= 2.0, "{fitted}");
        // A ProRes master sized to a width bound, from CPU frames.
        let master = call(
            "preview_video",
            json!({"sequenceId":sequence,"previewId":preview["id"],"startMs":start,"endMs":start+500.0,"width":640,"renderer":"cpu","codec":"prores","withAudio":false,"file":"MCP-music-master.mov","confirm":true}),
        );
        assert!(master["width"].as_u64().unwrap() <= 640, "{master}");
        assert!(master["codec"].as_str().unwrap().starts_with("Apple ProRes 4444"), "{master}");
        assert!(master["renderer"].as_str().unwrap().starts_with("CPU"), "{master}");
        let mov = std::fs::read(root.join("Export/MCP-music-master.mov")).unwrap();
        assert_eq!(&mov[4..12], b"ftypqt  ");
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
    // The user phoneme dictionary: a word mapped, listed, removed, and both steps undone off the shared history.
    let before = call("phoneme_mapping_list", json!({}));
    let mapped = call("phoneme_mapping_set", json!({"word":"zorbleplex","phonemes":["ETC","O","MBP"]}));
    assert_eq!(mapped["result"]["word"], "ZORBLEPLEX");
    let listed = call("phoneme_mapping_list", json!({}));
    assert!(listed["mappings"].as_array().unwrap().iter().any(|m| m["word"] == "ZORBLEPLEX" && m["phonemes"] == json!(["ETC","O","MBP"])), "{listed}");
    call("phoneme_mapping_remove", json!({"word":"ZORBLEPLEX"}));
    assert_eq!(call("phoneme_mapping_list", json!({})), before);
    call("display_undo", json!({}));
    assert!(call("phoneme_mapping_list", json!({}))["mappings"].as_array().unwrap().iter().any(|m| m["word"] == "ZORBLEPLEX"));
    call("display_undo", json!({}));
    assert_eq!(call("phoneme_mapping_list", json!({})), before);
    // Face mapping: an element's Face property set, listed, and the step undone off the shared history.
    let faces = call("face_mapping_list", json!({}));
    let face_target = elements["items"].as_array().unwrap().iter().find(|e| e["elementId"].is_string()).and_then(|e| e["id"].as_str()).unwrap().to_string();
    let set = call("face_mapping_set", json!({"nodeIds":[face_target],"phonemes":["AI","MBP"],"components":["Outlines"]}));
    assert_eq!(set["result"]["elements"][0]["phonemes"], json!(["AI","MBP"]), "{set}");
    let listed = call("face_mapping_list", json!({}));
    assert!(listed["elements"].as_array().unwrap().iter().any(|e| e["id"] == face_target.as_str() && e["components"] == json!(["Outlines"])), "{listed}");
    call("display_undo", json!({}));
    assert_eq!(call("face_mapping_list", json!({}))["elements"], faces["elements"]);
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
        "MCP authored and rendered {} beat-synced effects over 30000 ms; TIM/FSEQ artifacts verified",
        selected.len()
    );
}
