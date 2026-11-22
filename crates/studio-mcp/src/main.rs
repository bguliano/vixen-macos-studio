use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use studio_mcp::lock::{self, Note};
use studio_mcp::socket::SocketServer;

const USAGE: &str = "usage: vixen-studio-mcp --profile PATH [--persist | --serve [--idle SECONDS] | call TOOL [JSON|-]]\n       vixen-studio-mcp plugin new|validate|pack|run … (vixen-studio-mcp plugin help)";
/// How long a background session started by --persist or call stays up with no clients.
const PERSIST_IDLE: Duration = Duration::from_secs(600);

fn main() {
    // The plugin developer commands need no profile, session or lock.
    let args: Vec<String> = std::env::args_os().skip(1).map(|a| a.to_string_lossy().into_owned()).collect();
    if args.first().map(String::as_str) == Some("plugin") {
        std::process::exit(studio_plugins::cli::main(&args[1..], profile_display));
    }
    if let Err(error) = run() {
        // Every failing exit says why, on stderr and in the Vixen logs.
        lock::note(Note::Error, &error.to_string());
        std::process::exit(1);
    }
}

/// `plugin validate|run --profile`: answers `display.snapshot` with the show saved in that profile folder, read once.
fn profile_display(root: &Path) -> Result<studio_plugins::process::DisplaySink, String> {
    let full = vixen_session::studio_plugin_display::of_profile(root)?;
    Ok(Arc::new(move |params: &Value| Ok(vixen_session::studio_plugin_display::page_for(full.clone(), params))))
}

enum Mode {
    /// MCP on stdio: attach to whoever serves the profile, otherwise host the session in this process.
    Stdio,
    /// MCP on stdio, through a background session that outlives this process.
    Persist,
    /// Host the session in the foreground until it has had no clients for `idle` (zero: never).
    Serve { idle: Duration },
    /// One tool call through the persistent session; prints the tool's text.
    Call { tool: String, arguments: Option<String> },
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    // First, before any thread starts: a stop signal removes a held lock instead of leaving it behind.
    lock::handle_stop_signals()?;
    let mut args = std::env::args_os().skip(1).map(|a| a.into_string().map_err(|_| USAGE));
    let mut profile = None;
    let mut mode = Mode::Stdio;
    let mut idle = None;
    while let Some(arg) = args.next() {
        match arg?.as_str() {
            "--profile" if profile.is_none() => profile = Some(args.next().ok_or(USAGE)??),
            "--persist" if matches!(mode, Mode::Stdio) => mode = Mode::Persist,
            "--serve" if matches!(mode, Mode::Stdio) => mode = Mode::Serve { idle: Duration::ZERO },
            "--idle" if idle.is_none() => {
                idle = Some(Duration::from_secs(args.next().ok_or(USAGE)??.parse().map_err(|_| USAGE)?))
            }
            "call" if matches!(mode, Mode::Stdio) => {
                let tool = args.next().ok_or(USAGE)??;
                mode = Mode::Call { tool, arguments: args.next().transpose()? };
                if args.next().is_some() {
                    return Err(USAGE.into());
                }
            }
            _ => return Err(USAGE.into()),
        }
    }
    let root = PathBuf::from(profile.ok_or(USAGE)?).canonicalize()?;
    if root
        .components()
        .any(|c| c.as_os_str() == "vixen-show-original")
    {
        return Err("Use a show copy, not the original archive".into());
    }
    match (mode, idle) {
        (Mode::Serve { .. }, Some(idle)) => serve(&root, idle),
        (_, Some(_)) => Err(USAGE.into()),
        (Mode::Serve { idle }, None) => serve(&root, idle),
        (Mode::Stdio, None) => stdio(&root),
        (Mode::Persist, None) => proxy(persistent(&root)?),
        (Mode::Call { tool, arguments }, None) => call(persistent(&root)?, &tool, arguments),
    }
}

/// Connects to the app or a hosting bridge. None: nobody serves the profile.
fn attach(root: &Path) -> Result<Option<UnixStream>, Box<dyn std::error::Error>> {
    // A refused endpoint is a dead app's leftover only if nothing holds the profile: take the lock first (it fails if a live
    // process holds it; a dead process's lock is cleared by lock::take), then remove the endpoint and serve headless.
    match studio_mcp::socket::connect(root) {
        Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
            let held = lock::hold(lock::take(root)
                .map_err(|e| format!("MCP endpoint is refusing connections and the profile is locked by a running process; open the app, or remove the stale socket ({e})"))?);
            let path = studio_mcp::socket::socket_path(root)?;
            let recovered = studio_mcp::socket::recover_stale(&path, studio_mcp::socket::lock_held_here(root));
            drop(held);
            if !recovered? {
                return Err("MCP endpoint is refusing connections; it was not removed".into());
            }
            Ok(studio_mcp::socket::connect(root)?)
        }
        other => Ok(other?),
    }
}

/// `attach`, waiting up to `wait` for a bridge that is starting up to begin serving.
fn attach_within(root: &Path, wait: Duration) -> Result<Option<UnixStream>, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + wait;
    loop {
        if let Some(stream) = attach(root)? {
            return Ok(Some(stream));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// This process's own session: the profile lock, a muted core, and the endpoint other bridges and scripts attach to. Fields drop
/// in order, so the endpoint goes before the core and the lock.
struct Host {
    endpoint: SocketServer,
    core: Arc<vixen_session::app::AppCore>,
    _held: lock::Held,
}
fn host(root: &Path) -> Result<Host, Box<dyn std::error::Error>> {
    // Never steal a running process's profile; only a provably dead owner's lock is cleared. The guard removes the lock on
    // return, including stdin EOF, and the signal thread removes it (and the endpoint) on SIGTERM, SIGINT or SIGHUP.
    let held = lock::hold(lock::take(root)?);
    let core = Arc::new(vixen_session::app::AppCore::open(root.to_path_buf(), true)?);
    let endpoint = SocketServer::start_headless(&core)?;
    endpoint.remove_on_stop_signal();
    Ok(Host { endpoint, core, _held: held })
}

fn proxy(socket: UnixStream) -> Result<(), Box<dyn std::error::Error>> {
    studio_mcp::socket::proxy(socket, std::io::stdin().lock(), std::io::stdout(), studio_mcp::socket::KEEPALIVE)?;
    Ok(())
}

fn stdio(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(socket) = attach(root)? {
        return proxy(socket);
    }
    let host = match host(root) {
        Ok(host) => host,
        // Another bridge may have taken the lock a moment ago and be about to serve.
        Err(error) => return proxy(attach_within(root, Duration::from_secs(3))?.ok_or(error)?),
    };
    let mut server = studio_mcp::Server::attach(host.core.clone(), false);
    studio_mcp::serve(&mut server, std::io::stdin().lock(), std::io::stdout().lock())?;
    drop(server);
    // Scripts that attached to this session keep it until they disconnect.
    while host.endpoint.clients() > 0 {
        std::thread::sleep(Duration::from_millis(200));
    }
    Ok(())
}

fn serve(root: &Path, idle: Duration) -> Result<(), Box<dyn std::error::Error>> {
    if attach(root)?.is_some() {
        return Err("This profile is already being served (by the app or another bridge); attach to it instead".into());
    }
    let host = host(root)?;
    lock::note(Note::Info, &format!("Serving {} until {}", root.display(),
        if idle.is_zero() { "stopped".to_string() } else { format!("{} s pass with no clients", idle.as_secs()) }));
    let mut last_client = Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(250));
        if host.endpoint.clients() > 0 {
            last_client = Instant::now();
        } else if !idle.is_zero() && last_client.elapsed() >= idle {
            return Ok(());
        }
    }
}

/// Attaches to the app or a running session, starting a background `--serve` session when there is none.
fn persistent(root: &Path) -> Result<UnixStream, Box<dyn std::error::Error>> {
    if let Some(stream) = attach(root)? {
        return Ok(stream);
    }
    let mut child = std::process::Command::new(std::env::current_exe()?);
    child.arg("--profile").arg(root).args(["--serve", "--idle", &PERSIST_IDLE.as_secs().to_string()]);
    // Detached: no shared pipes, so a caller capturing this process's output isn't held open by the session.
    let mut child = std::os::unix::process::CommandExt::process_group(&mut child, 0)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(stream) = attach(root)? {
            return Ok(stream);
        }
        // A session that exited either lost a start race to another one, which is about to serve, or failed.
        if Instant::now() >= deadline || child.try_wait()?.is_some() {
            return attach_within(root, Duration::from_secs(3))?.ok_or_else(|| format!(
                "Could not start a session for {0}; run `vixen-studio-mcp --profile {0} --serve` to see why", root.display()).into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn call(stream: UnixStream, tool: &str, arguments: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    let text = match arguments.as_deref() {
        None => "{}".to_string(),
        Some("-") => {
            let mut text = String::new();
            std::io::stdin().read_to_string(&mut text)?;
            text
        }
        Some(text) => text.to_string(),
    };
    let arguments: Value = serde_json::from_str(&text).map_err(|e| format!("Tool arguments are not JSON: {e}"))?;
    if !arguments.is_object() {
        return Err("Tool arguments must be a JSON object".into());
    }
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream);
    for message in [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","clientInfo":{"name":"vixen-studio-mcp call","version":env!("CARGO_PKG_VERSION")},"capabilities":{}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":tool,"arguments":arguments}}),
    ] {
        writeln!(writer, "{message}")?;
    }
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Err("The session closed before answering".into());
        }
        let reply: Value = serde_json::from_str(&line)?;
        if reply["id"] != 2 {
            continue;
        }
        if let Some(error) = reply.get("error") {
            return Err(error["message"].as_str().unwrap_or("Request failed").into());
        }
        let text = reply["result"]["content"][0]["text"].as_str().unwrap_or("");
        if reply["result"]["isError"] != false {
            // The tool's own refusal, not a bridge failure: stderr only, not the Vixen logs.
            eprintln!("{text}");
            std::process::exit(1);
        }
        let mut stdout = std::io::stdout().lock();
        writeln!(stdout, "{text}")?;
        stdout.flush()?;
        return Ok(());
    }
}
