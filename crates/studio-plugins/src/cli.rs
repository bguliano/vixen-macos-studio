//! `vixen-studio-mcp plugin …`: the plugin developer's command line, for people and CI. Nothing here needs Studio
//! running or touches the user's installed plugins: `new` writes a folder from the templates `plugin_scaffold` uses,
//! `validate` runs the conformance harness behind `plugin_validate`, `pack` makes a `.vixenplugin`, and `run` drives one
//! controller with a test pattern while printing what the plugin logs and reports.
use crate::controllers::Encoding;
use crate::display::ProfileDisplay;
use crate::harness::{self, Scratch};
use crate::logs::{Level, LogBuffer};
use crate::manifest::{ControllerType, Manifest};
use crate::process::{self, Launch, PluginProcess};
use crate::{schema, API_VERSION};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use vixen_output::OutputCommand;

pub const USAGE: &str = "usage: vixen-studio-mcp plugin <command>
  new --id ID --name NAME --language python|typescript|rust [--kind controller|tools] [--dir PARENT] [--folder NAME]
      Write a working plugin from the templates plugin_scaffold uses, with the language's SDK, to PARENT/NAME
      (default: ./<last part of the id>). Never overwrites anything.
  validate DIR [--live] [--burst-ms N] [--profile PROFILE] [--json]
      Check the manifest and run the conformance harness. --live also sends controller.start and 40 fps frame bursts:
      a real output plugin lights real lights. Exits 1 when a check fails.
  pack DIR [--out FOLDER] [--force] [--json]
      Make <id>-<version>.vixenplugin (a zip of DIR, manifest at its root; .git, caches, target/ and earlier packages
      left out) and <that>.sha256, in FOLDER (default: DIR's parent).
  run DIR [--frames chase|ramp|blink|rgb|off] [--seconds N] [--fps N] [--type TYPE] [--outputs N] [--config JSON]
      [--profile PROFILE]
      Start one controller of TYPE (typeId or name; default: the first) with the settings in JSON (default: the
      configSchema's defaults), send the pattern for N seconds (default 10), and print the plugin's log, status, stats
      and messages as they arrive. This is live output: point it at a test receiver, not your show.
  validate and run answer a \"display\": \"read\" plugin's display.snapshot with a small sample show, or with
      --profile, the show saved in that Vixen profile folder (read only; Studio can have it open).
  sandbox DIR
      Print the macOS sandbox profile Studio starts DIR's plugin under, made from its permissions (validate and run
      use it too). VIXEN_PLUGIN_SANDBOX=off in Studio's environment starts plugins without it.";

/// Runs `args` (after `plugin`) and returns the exit code. `profile` reads `--profile` folders for `display.snapshot`.
pub fn main(args: &[String], profile: ProfileDisplay) -> i32 {
    let Some((command, rest)) = args.split_first() else {
        println!("{USAGE}");
        return 2;
    };
    let outcome = match command.as_str() {
        "new" => new(rest),
        "validate" => validate(rest, profile),
        "pack" => pack(rest),
        "run" => run(rest, profile),
        "sandbox" => sandbox(rest),
        "help" | "--help" | "-h" => {
            println!("{USAGE}");
            return 0;
        }
        other => Err(format!("unknown plugin command {other}\n{USAGE}")),
    };
    match outcome {
        Ok(code) => code,
        Err(why) => {
            eprintln!("{why}");
            2
        }
    }
}

/// Flags as `--name value` (or bare `--name` for switches) and positional arguments.
struct Args {
    positional: Vec<String>,
    flags: Vec<(String, Option<String>)>,
}

impl Args {
    fn parse(args: &[String], switches: &[&str], valued: &[&str]) -> Result<Args, String> {
        let mut out = Args { positional: Vec::new(), flags: Vec::new() };
        let mut iter = args.iter();
        while let Some(arg) = iter.next() {
            if let Some(name) = arg.strip_prefix("--") {
                if switches.contains(&name) {
                    out.flags.push((name.to_string(), None));
                } else if valued.contains(&name) {
                    let value = iter.next().ok_or_else(|| format!("--{name} needs a value\n{USAGE}"))?;
                    out.flags.push((name.to_string(), Some(value.clone())));
                } else {
                    return Err(format!("unknown option --{name}\n{USAGE}"));
                }
            } else {
                out.positional.push(arg.clone());
            }
        }
        Ok(out)
    }

    fn has(&self, name: &str) -> bool {
        self.flags.iter().any(|(n, _)| n == name)
    }

    fn value(&self, name: &str) -> Option<&str> {
        self.flags.iter().rev().find(|(n, _)| n == name).and_then(|(_, v)| v.as_deref())
    }

    fn number(&self, name: &str, min: u64, max: u64) -> Result<Option<u64>, String> {
        match self.value(name) {
            None => Ok(None),
            Some(text) => match text.parse::<u64>() {
                Ok(n) if (min..=max).contains(&n) => Ok(Some(n)),
                _ => Err(format!("--{name} must be a whole number from {min} to {max}")),
            },
        }
    }

    /// What answers `display.snapshot`: the `--profile` folder's show, or the sample one.
    fn display(&self, profile: ProfileDisplay) -> Result<process::DisplaySink, String> {
        match self.value("profile") {
            Some(folder) => profile(&absolute(Path::new(folder))?).map_err(|why| format!("--profile {folder}: {why}")),
            None => Ok(crate::display::sample_sink()),
        }
    }

    /// The one folder argument, made absolute.
    fn folder(&self) -> Result<PathBuf, String> {
        match self.positional.as_slice() {
            [dir] => absolute(Path::new(dir)),
            _ => Err(format!("give exactly one plugin folder\n{USAGE}")),
        }
    }
}

fn absolute(path: &Path) -> Result<PathBuf, String> {
    let path = if path.is_absolute() { path.to_path_buf() } else { std::env::current_dir().map_err(|e| e.to_string())?.join(path) };
    path.canonicalize().map_err(|e| format!("{}: {e}", path.display()))
}

fn new(args: &[String]) -> Result<i32, String> {
    let args = Args::parse(args, &["json"], &["id", "name", "language", "kind", "dir", "folder"])?;
    if !args.positional.is_empty() {
        return Err(format!("plugin new takes only options\n{USAGE}"));
    }
    let need = |name: &str| args.value(name).ok_or_else(|| format!("plugin new needs --{name}\n{USAGE}"));
    let parent = match args.value("dir") {
        Some(dir) => {
            let dir = Path::new(dir);
            if dir.is_absolute() { dir.to_path_buf() } else { std::env::current_dir().map_err(|e| e.to_string())?.join(dir) }
        }
        None => std::env::current_dir().map_err(|e| e.to_string())?,
    };
    let language = need("language")?;
    let out = crate::scaffold::create(&parent, need("id")?, need("name")?, language, args.value("kind").unwrap_or("controller"), args.value("folder"))?;
    if args.has("json") {
        println!("{}", serde_json::to_string_pretty(&out).expect("serializes"));
        return Ok(0);
    }
    let folder = out["folder"].as_str().unwrap_or_default();
    println!("Created {folder}");
    for file in out["files"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        println!("  {file}");
    }
    println!("\nNext:");
    println!("  edit {}/{} (the TODO parts) and vixen-plugin.json", folder, out["main"].as_str().unwrap_or_default());
    if let Some(build) = crate::scaffold::template(language).and_then(|t| t.build) {
        println!("  build it: {build}");
    }
    println!("  vixen-studio-mcp plugin validate {folder}");
    println!("  vixen-studio-mcp plugin run {folder}");
    println!("  link the folder in Studio: Settings ▸ Plugins ▸ Link Development Folder…");
    Ok(0)
}

fn sandbox(args: &[String]) -> Result<i32, String> {
    let dir = Args::parse(args, &[], &[])?.folder()?;
    let manifest = Manifest::load(&dir)?;
    let program = crate::process::resolve_command(&dir, &manifest.entry.command)?;
    // The data folder Studio would give an installed copy; validate and run use a throwaway one instead.
    let data_dir = crate::default_root().map(|root| root.join("data").join(&manifest.id))?;
    print!("{}", crate::sandbox::profile(&manifest, &crate::sandbox::Paths::for_launch(&dir, &data_dir, &program)));
    if !crate::sandbox::enabled() {
        eprintln!("(the sandbox is off here: {} is missing or {}=off)", crate::sandbox::SANDBOX_EXEC, crate::sandbox::DISABLE_ENV);
    }
    Ok(0)
}

fn validate(args: &[String], profile: ProfileDisplay) -> Result<i32, String> {
    let args = Args::parse(args, &["live", "json"], &["burst-ms", "profile"])?;
    let dir = args.folder()?;
    let burst = args.number("burst-ms", 100, 10_000)?.unwrap_or(1000);
    let display = args.display(profile)?;
    let options = harness::Options { live_output: args.has("live"), burst: Duration::from_millis(burst), display: Some(display), ..harness::Options::default() };
    let report = harness::validate(&dir, &options);
    if args.has("json") {
        println!("{}", serde_json::to_string_pretty(&report).expect("serializes"));
    } else {
        print!("{}", describe_report(&report));
    }
    Ok(if report["passed"] == true { 0 } else { 1 })
}

/// A validation report as lines for a terminal.
pub fn describe_report(report: &Value) -> String {
    let mut out = format!("Validating {}\n", report["folder"].as_str().unwrap_or_default());
    let mut counts = [0usize; 4];
    for check in report["checks"].as_array().into_iter().flatten() {
        let (mark, slot) = match check["status"].as_str() {
            Some("pass") => ("PASS", 0),
            Some("warn") => ("WARN", 1),
            Some("fail") => ("FAIL", 2),
            _ => ("skip", 3),
        };
        counts[slot] += 1;
        let detail = check["detail"].as_str().map(|d| format!(": {d}")).unwrap_or_default();
        out.push_str(&format!("  {mark} {} ({} ms){detail}\n", check["name"].as_str().unwrap_or("?"), check["ms"].as_u64().unwrap_or(0)));
    }
    let failed = report["passed"] != true;
    if failed {
        let logs: Vec<String> = report["logs"].as_array().into_iter().flatten().rev().take(10).filter_map(|e| e["message"].as_str().map(str::to_string)).collect();
        if !logs.is_empty() {
            out.push_str("Last log lines:\n");
            for line in logs.iter().rev() {
                out.push_str(&format!("  {line}\n"));
            }
        }
    }
    out.push_str(&format!(
        "{}: {} passed, {} warning(s), {} failed, {} skipped\n",
        if failed { "FAILED" } else { "Passed" },
        counts[0],
        counts[1],
        counts[2],
        counts[3]
    ));
    out
}

fn pack(args: &[String]) -> Result<i32, String> {
    let args = Args::parse(args, &["force", "json"], &["out"])?;
    let dir = args.folder()?;
    let out_dir = args.value("out").map(|p| absolute(Path::new(p))).transpose()?;
    let result = crate::pack::pack(&dir, out_dir.as_deref(), args.has("force"))?;
    if args.has("json") {
        println!("{}", serde_json::to_string_pretty(&result).expect("serializes"));
    } else {
        println!("{}", result["path"].as_str().unwrap_or_default());
        println!("  {} files, {} bytes", result["files"], result["bytes"]);
        println!("  sha256 {}", result["sha256"].as_str().unwrap_or_default());
    }
    Ok(0)
}

/// The values for one frame of `pattern` at `t` seconds (`seq` counts frames).
pub fn pattern_values(pattern: &str, outputs: usize, seq: u64, t: f64) -> Vec<u8> {
    match pattern {
        "chase" => (0..outputs).map(|i| if i == seq as usize % outputs.max(1) { 255 } else { 0 }).collect(),
        "ramp" => vec![((t % 2.0) / 2.0 * 255.0).round() as u8; outputs],
        "blink" => vec![if ((t * 2.0) as u64).is_multiple_of(2) { 255 } else { 0 }; outputs],
        "rgb" => {
            let colour = (t as u64 % 3) as usize;
            (0..outputs).map(|i| if i % 3 == colour { 255 } else { 0 }).collect()
        }
        _ => vec![0; outputs],
    }
}

/// What the plugin reported during `run`, waiting to be printed.
#[derive(Default)]
struct Reported {
    lines: Vec<String>,
    packets: u64,
    bytes: u64,
    errors: u64,
}

fn run(args: &[String], profile: ProfileDisplay) -> Result<i32, String> {
    let args = Args::parse(args, &[], &["frames", "seconds", "fps", "type", "outputs", "config", "profile"])?;
    let dir = args.folder()?;
    let pattern = args.value("frames").unwrap_or("chase");
    if !["chase", "ramp", "blink", "rgb", "off"].contains(&pattern) {
        return Err("--frames must be chase, ramp, blink, rgb or off".into());
    }
    let seconds = args.number("seconds", 1, 3600)?.unwrap_or(10);
    let fps = args.number("fps", 1, 60)?.unwrap_or(40);
    let manifest = Manifest::load(&dir)?;
    let controller: &ControllerType = match args.value("type") {
        None => manifest.controllers.first().ok_or("this plugin declares no controller types; run drives controllers (try plugin validate)")?,
        Some(wanted) => manifest.controllers.iter().find(|c| c.type_id == wanted || c.name == wanted).ok_or_else(|| format!("no controller type {wanted}"))?,
    };
    let outputs = args.number("outputs", 1, controller.max_outputs)?.unwrap_or(controller.default_outputs);
    let config = match args.value("config") {
        Some(text) => serde_json::from_str::<Value>(text).map_err(|e| format!("--config isn't JSON: {e}"))?,
        None => schema::defaults(&controller.config_schema),
    };
    let config = if config.is_object() { config } else { json!({}) };
    let errors = schema::validate(&controller.config_schema, &config);
    if !errors.is_empty() {
        let list: Vec<String> = errors.iter().map(|e| format!("{} {}", e.field, e.message)).collect();
        return Err(format!("the settings don't fit configSchema: {}", list.join("; ")));
    }
    let encoding = controller.frame_encodings.first().map(String::as_str).unwrap_or("u8");
    let codec = match encoding {
        "json" => Encoding::Json,
        "u16" => Encoding::U16,
        _ => Encoding::U8,
    };

    let display = args.display(profile)?;
    let scratch = Scratch::new()?;
    let logs = Arc::new(LogBuffer::new(None));
    let reported = Arc::new(Mutex::new(Reported::default()));
    let sink = reported.clone();
    let controllers: process::ControllerSink = Arc::new(move |method: &str, params: &Value| {
        let mut r = sink.lock().unwrap_or_else(|p| p.into_inner());
        match method {
            "controller.status" => {
                let message = params["message"].as_str().map(|m| format!(" — {m}")).unwrap_or_default();
                r.lines.push(format!("status {}{message}", params["state"].as_str().unwrap_or("?")));
            }
            "controller.message" => r.lines.push(format!("message {}: {}", params["title"].as_str().unwrap_or(""), params["text"].as_str().unwrap_or(""))),
            _ => {
                r.packets += params["packetsSent"].as_u64().unwrap_or(0);
                r.bytes += params["bytesSent"].as_u64().unwrap_or(0);
                r.errors += params["errors"].as_u64().unwrap_or(0);
            }
        }
        Ok(json!({}))
    });
    let tools: process::ToolsSink = Arc::new(|_: &Value| Ok(json!({})));
    let launch = Launch { manifest: &manifest, dir: &dir, data_dir: &scratch.0, logs: logs.clone(), controllers: Some(controllers), tools: Some(tools), display: Some(display) };
    let plugin = PluginProcess::spawn(launch)?;
    println!(
        "Running {} ({} {}): {} with {outputs} outputs, {pattern} at {fps} fps for {seconds} s ({encoding} frames)",
        manifest.name, manifest.id, manifest.version, controller.name
    );
    let started = Instant::now();
    let mut printer = Printer { logs: logs.clone(), reported: reported.clone(), started, last_seq: 0 };
    let outcome = drive(&plugin, controller, &config, outputs, pattern, fps, seconds, codec, encoding, &scratch.0, &mut printer);
    printer.flush();
    let code = match outcome {
        Ok((sent, dropped)) => {
            let r = reported.lock().unwrap_or_else(|p| p.into_inner());
            println!(
                "Done: {sent} frames sent, {dropped} replaced before the plugin read them; it reported {} packets, {} bytes, {} errors",
                r.packets, r.bytes, r.errors
            );
            0
        }
        Err(why) => {
            println!("Stopped: {why}");
            1
        }
    };
    if !plugin.is_dead() && plugin.try_wait().is_none() {
        let _ = plugin.request("shutdown", json!({}), Duration::from_secs(3));
        plugin.shutdown(Duration::from_secs(3));
    } else {
        plugin.kill("run finished");
    }
    printer.flush();
    Ok(code)
}

struct Printer {
    logs: Arc<LogBuffer>,
    reported: Arc<Mutex<Reported>>,
    started: Instant,
    last_seq: u64,
}

impl Printer {
    /// Prints the log entries and reports that arrived since the last call.
    fn flush(&mut self) {
        let at = self.started.elapsed().as_secs_f64();
        for entry in self.logs.tail(2000, Level::Debug) {
            if entry.seq > self.last_seq {
                self.last_seq = entry.seq;
                println!("[{at:6.2} s] {:5} {}", entry.level.as_str(), entry.message);
            }
        }
        let lines = std::mem::take(&mut self.reported.lock().unwrap_or_else(|p| p.into_inner()).lines);
        for line in lines {
            println!("[{at:6.2} s] {line}");
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn drive(
    plugin: &Arc<PluginProcess>,
    controller: &ControllerType,
    config: &Value,
    outputs: u64,
    pattern: &str,
    fps: u64,
    seconds: u64,
    codec: Encoding,
    encoding: &str,
    data_dir: &Path,
    printer: &mut Printer,
) -> Result<(u64, u64), String> {
    let timeout = Duration::from_secs(5);
    let fail = |step: &str, e: crate::rpc::RpcError| format!("{step} failed: {} ({})", e.message, e.code);
    let initialize = json!({
        "apiVersion": API_VERSION.to_string(), "hostVersion": crate::HOST_VERSION, "pluginDataDir": data_dir.to_string_lossy(),
        "profile": null, "muted": false, "capabilities": crate::host_capabilities(),
    });
    plugin.request("initialize", initialize, timeout).map_err(|e| fail("initialize", e))?;
    let instance = uuid::Uuid::new_v4().to_string();
    let created = plugin
        .request("controller.create", json!({"instanceId": instance, "typeId": controller.type_id, "config": config, "outputCount": outputs}), timeout)
        .map_err(|e| fail("controller.create", e))?;
    if let Some(normalised) = created.get("config").filter(|c| *c != config) {
        println!("The plugin normalised the settings to {normalised}");
    }
    plugin.request("controller.start", json!({"instanceId": instance}), Duration::from_secs(15)).map_err(|e| fail("controller.start", e))?;
    printer.flush();
    let interval = Duration::from_millis(1000 / fps);
    let started = Instant::now();
    let (mut sent, mut dropped, mut seq) = (0u64, 0u64, 0u64);
    let mut next_report = Duration::from_secs(1);
    while started.elapsed() < Duration::from_secs(seconds) {
        let tick = Instant::now();
        seq += 1;
        let values = pattern_values(pattern, outputs as usize, seq, started.elapsed().as_secs_f64());
        let commands: Vec<OutputCommand> = values.into_iter().map(OutputCommand::U8).collect();
        let params = json!({"instanceId": instance, "seq": seq, "timeMs": started.elapsed().as_millis() as u64, "encoding": encoding, "values": codec.encode(&commands)});
        match plugin.send_frame(&instance, params) {
            Ok(process::FrameSent::Queued) => sent += 1,
            Ok(process::FrameSent::Replaced) => {
                sent += 1;
                dropped += 1;
            }
            Err(_) => return Err(plugin.describe_exit(plugin.try_wait())),
        }
        if started.elapsed() >= next_report {
            next_report += Duration::from_secs(1);
            printer.flush();
            println!("[{:6.2} s] {sent} frames sent, {dropped} replaced", printer.started.elapsed().as_secs_f64());
        }
        std::thread::sleep(interval.saturating_sub(tick.elapsed()));
    }
    plugin.request("controller.stop", json!({"instanceId": instance}), timeout).map_err(|e| fail("controller.stop", e))?;
    plugin.request("controller.destroy", json!({"instanceId": instance}), timeout).map_err(|e| fail("controller.destroy", e))?;
    Ok((sent, dropped))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_and_arguments() {
        assert_eq!(pattern_values("chase", 3, 4, 0.0), vec![0, 255, 0]);
        assert_eq!(pattern_values("blink", 2, 1, 0.75), vec![0, 0]);
        assert_eq!(pattern_values("rgb", 6, 1, 1.2), vec![0, 255, 0, 0, 255, 0]);
        assert_eq!(pattern_values("ramp", 1, 1, 1.0), vec![128]);
        let strings: Vec<String> = ["dir", "--live", "--burst-ms", "200"].iter().map(|s| s.to_string()).collect();
        let args = Args::parse(&strings, &["live"], &["burst-ms"]).unwrap();
        assert!(args.has("live"));
        assert_eq!(args.number("burst-ms", 100, 1000).unwrap(), Some(200));
        assert!(Args::parse(&strings[..3], &["live"], &["burst-ms"]).err().unwrap().contains("needs a value"));
        assert_eq!(main(&["bogus".to_string()], |_| Err("unused".into())), 2);
        // --profile goes through the given reader, its error named; without it the sample show answers.
        let with = Args::parse(&["dir".to_string(), "--profile".to_string(), "/".to_string()], &[], &["profile"]).unwrap();
        let why = with.display(|_| Err("not a profile".into())).err().unwrap();
        assert!(why.contains("--profile /") && why.contains("not a profile"), "{why}");
        let without = Args::parse(&strings[..1], &[], &["profile"]).unwrap();
        assert_eq!(without.display(|_| Err("unused".into())).unwrap()(&json!({})).unwrap()["elementCount"], 4);
    }
}
