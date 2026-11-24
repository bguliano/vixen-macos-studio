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

const SANDSTORM: &str = "Darude_Sandstorm.mp3";
const RUDOLPH: &str = "10 Run Rudolph Run.mp3";

/// B77 on two real songs of different tempo, against downbeats checked by hand on the level envelope (where a section
/// enters after a break): Darude's Sandstorm (136 BPM; a drumless intro the tracker reads at ~207 BPM, and a breakdown
/// that isn't whole bars) and Chuck Berry's Run Rudolph Run (153 BPM shuffle with a backbeat on 2 and 4).
#[test]
fn music_rhythm_downbeats_match_hand_checked_entrances() {
    let target = support::target_dir();
    let source = target.join("show-copy");
    let media = source.join("Media");
    if ![SANDSTORM, RUDOLPH].iter().all(|song| media.join(song).exists()) {
        eprintln!("skipped: {SANDSTORM} and {RUDOLPH} are not in the show copy's Media");
        return;
    }
    let profile = ProfileDir(target.join(format!("studio-mcp-rhythm-{}", std::process::id())));
    let root = profile.0.clone();
    let _ = std::fs::remove_dir_all(&root);
    copy_dir(&source.join("SystemData"), &root.join("SystemData"));
    copy_dir(&source.join("Module Data Files"), &root.join("Module Data Files"));
    std::os::unix::fs::symlink(media.canonicalize().unwrap(), root.join("Media")).unwrap();
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
        writeln!(input, "{}", json!({"jsonrpc":"2.0","id":next,"method":method,"params":params})).unwrap();
        next += 1;
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        let reply: Value = serde_json::from_str(&line).unwrap();
        assert!(reply.get("error").is_none(), "{reply}");
        reply["result"].clone()
    };
    rpc("initialize", json!({"protocolVersion":"2024-11-05","clientInfo":{},"capabilities":{}}));
    let mut rhythm = |song: &str| -> Value {
        let result = rpc("tools/call", json!({"name":"music_rhythm","arguments":{"media":song,"include":["bars"]}}));
        assert_eq!(result["isError"], false, "{result}");
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
    };
    let downbeats = |v: &Value| -> Vec<f64> { v["bars"].as_array().unwrap().iter().map(|b| b["downbeatMs"].as_f64().unwrap()).collect() };
    let on_a_downbeat = |v: &Value, entrance: f64| {
        let nearest = downbeats(v).into_iter().map(|d| (d - entrance).abs()).fold(f64::MAX, f64::min);
        assert!(nearest <= 60.0, "{entrance} ms is {nearest} ms from the nearest downbeat: {:?}", downbeats(v));
    };

    let sandstorm = rhythm(SANDSTORM);
    let first = &sandstorm["tempoMap"][0];
    assert!((first["bpm"].as_f64().unwrap() - 136.0).abs() < 1.5 && first["bar"] == 1, "{}", sandstorm["tempoMap"]);
    // The music starts at ~2.15 s; no bar starts in the silence before it.
    assert!(sandstorm["firstDownbeatMs"].as_f64().unwrap() >= 2100.0, "{}", sandstorm["firstDownbeatMs"]);
    for entrance in [16_600.0, 94_650.0] {
        on_a_downbeat(&sandstorm, entrance);
    }

    let rudolph = rhythm(RUDOLPH);
    assert!((rudolph["tempoMap"][0]["bpm"].as_f64().unwrap() - 153.0).abs() < 2.0, "{}", rudolph["tempoMap"]);
    assert!((rudolph["firstDownbeatMs"].as_f64().unwrap() - 400.0).abs() <= 60.0, "{}", rudolph["firstDownbeatMs"]);
    for entrance in [12_950.0, 44_300.0, 56_825.0, 132_050.0] {
        on_a_downbeat(&rudolph, entrance);
    }
    // The backbeat: mid hits are stronger on 2 and 4 than on 1 and 3, summed over the song.
    let strength = |c: char| c.to_digit(10).unwrap_or(0);
    let (mut on_backbeat, mut on_downbeat) = (0, 0);
    for bar in rudolph["bars"].as_array().unwrap() {
        let mid: Vec<char> = bar["mid"].as_str().unwrap().chars().filter(|c| *c != ' ').collect();
        on_downbeat += strength(mid[0]) + strength(mid[8]);
        on_backbeat += strength(mid[4]) + strength(mid[12]);
    }
    assert!(on_backbeat as f64 > 1.3 * on_downbeat as f64, "backbeat {on_backbeat} vs 1 and 3 {on_downbeat}");
    drop(input);
    child.wait().unwrap();
}
