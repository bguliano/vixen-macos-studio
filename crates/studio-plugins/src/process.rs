//! One running plugin: its child process, a writer thread feeding its stdin, and reader threads for its stdout
//! (JSON-RPC) and stderr (log lines). Nothing here blocks on the plugin: writes go through a bounded queue, reads happen
//! on their own threads, and every request has a timeout. Controller frames bypass the queue: each controller instance
//! has one frame slot, and a newer frame replaces one the plugin hasn't read yet (latest frame wins). Frames are only
//! written once the plugin has read what is already in its stdin pipe, so a slow plugin never falls behind by a pipe's
//! worth of stale frames.
use crate::logs::{Level, LogBuffer, Source};
use crate::manifest::Manifest;
use crate::registry::{read_json, write_atomic};
use crate::rpc::{self, Incoming, Line, RpcError};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// Plugin→host methods this host answers.
pub const HOST_METHODS: [&str; 6] = ["log", "storage.get", "storage.set", "controller.status", "controller.stats", "controller.message"];
/// Plugin→host methods of API 1.0 a later stage of the host implements; answered with "not available yet".
const LATER_METHODS: [&str; 3] = ["display.snapshot", "tools.changed", "tool.call"];
/// Plugin→host controller methods, answered by the controller layer (`crate::controllers`).
pub const CONTROLLER_METHODS: [&str; 3] = ["controller.status", "controller.stats", "controller.message"];

/// Handles a plugin's `controller.*` message: `(method, params)`, answered as a request would be.
pub type ControllerSink = Arc<dyn Fn(&str, &Value) -> Result<Value, RpcError> + Send + Sync>;

/// Messages waiting for the plugin to read its stdin. A plugin that stops reading loses messages instead of stalling us.
const WRITE_QUEUE: usize = 1024;
const STDERR_LINE: usize = 16 * 1024;
pub const MAX_STATE_BYTES: usize = 1024 * 1024;
/// The PATH a plugin gets: the system's, plus the usual Homebrew locations for python3/node.
pub const PLUGIN_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin:/usr/local/bin";

type Pending = Arc<Mutex<HashMap<u64, Sender<Result<Value, RpcError>>>>>;
type Writer = Arc<Outbox>;

/// Frames wait while the plugin has more than this many bytes of its stdin still to read.
const FRAME_BACKLOG: usize = 1024;
/// How often waiting frames check whether the plugin has caught up.
const FRAME_POLL: Duration = Duration::from_millis(4);

/// What waits for the plugin to read its stdin: messages in order, then one frame per controller instance.
#[derive(Default)]
struct Pipe {
    queue: VecDeque<Vec<u8>>,
    frames: Vec<(String, Vec<u8>)>,
    /// Stdin is closing: nothing more is accepted, and the writer exits once it has written what's waiting.
    closed: bool,
}

/// The plugin's stdin, shared by requests, notifications, frames and the reader's answers.
#[derive(Default)]
struct Outbox {
    pipe: Mutex<Pipe>,
    ready: Condvar,
    /// A copy of the read end of the plugin's stdin pipe, to ask how much it hasn't read yet. Closed with the outbox, so
    /// a write to a plugin that has exited fails instead of waiting for a reader that will never come.
    probe: Mutex<Option<OwnedFd>>,
}

/// What `send_frame` did with a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameSent {
    Queued,
    /// Queued in place of the instance's earlier frame, which the plugin never read.
    Replaced,
}

impl Outbox {
    fn lock(&self) -> std::sync::MutexGuard<'_, Pipe> {
        self.pipe.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn push(&self, bytes: Vec<u8>) -> Result<(), RpcError> {
        let mut pipe = self.lock();
        if pipe.closed {
            return Err(RpcError::new(rpc::PLUGIN_EXITED, "the plugin is shutting down"));
        }
        if pipe.queue.len() >= WRITE_QUEUE {
            return Err(RpcError::new(rpc::TIMED_OUT, "the plugin is not reading its input"));
        }
        pipe.queue.push_back(bytes);
        self.ready.notify_one();
        Ok(())
    }

    fn push_frame(&self, instance: &str, bytes: Vec<u8>) -> Result<FrameSent, RpcError> {
        let mut pipe = self.lock();
        if pipe.closed {
            return Err(RpcError::new(rpc::PLUGIN_EXITED, "the plugin is shutting down"));
        }
        let sent = match pipe.frames.iter_mut().find(|(id, _)| id == instance) {
            Some((_, waiting)) => {
                *waiting = bytes;
                FrameSent::Replaced
            }
            None => {
                pipe.frames.push((instance.to_string(), bytes));
                FrameSent::Queued
            }
        };
        self.ready.notify_one();
        Ok(sent)
    }

    fn close(&self) {
        self.lock().closed = true;
        self.ready.notify_all();
        self.probe.lock().unwrap_or_else(|p| p.into_inner()).take();
    }

    /// Bytes in the plugin's stdin it hasn't read; 0 when that can't be told.
    fn unread(&self) -> usize {
        let probe = self.probe.lock().unwrap_or_else(|p| p.into_inner());
        let Some(fd) = probe.as_ref() else { return 0 };
        let mut n: libc::c_int = 0;
        // SAFETY: FIONREAD writes one int; the fd is a pipe's read end we own.
        let ok = unsafe { libc::ioctl(fd.as_raw_fd(), libc::FIONREAD, &mut n) } == 0;
        if ok { n.max(0) as usize } else { 0 }
    }

    /// The next bytes to write, waiting for some; `None` once closed and drained.
    fn next(&self) -> Option<Vec<u8>> {
        let mut pipe = self.lock();
        loop {
            if let Some(bytes) = pipe.queue.pop_front() {
                return Some(bytes);
            }
            if !pipe.frames.is_empty() && !pipe.closed && self.unread() > FRAME_BACKLOG {
                pipe = self.ready.wait_timeout(pipe, FRAME_POLL).unwrap_or_else(|p| p.into_inner()).0;
                continue;
            }
            if !pipe.frames.is_empty() {
                // Every waiting frame goes in one write, in the order the instances first queued.
                let mut bytes = Vec::new();
                for (_, frame) in pipe.frames.drain(..) {
                    bytes.extend_from_slice(&frame);
                }
                return Some(bytes);
            }
            if pipe.closed {
                return None;
            }
            pipe = self.ready.wait(pipe).unwrap_or_else(|p| p.into_inner());
        }
    }
}

/// Why the process ended, when we know better than its exit status (it broke the protocol, or we stopped it).
#[derive(Default)]
struct Fate {
    dead: AtomicBool,
    /// Its stdout reached end of file: no answer will ever come.
    closed: AtomicBool,
    reason: Mutex<Option<String>>,
}

impl Fate {
    fn set(&self, reason: String) {
        let mut slot = self.reason.lock().unwrap_or_else(|p| p.into_inner());
        if slot.is_none() {
            *slot = Some(reason);
        }
        self.dead.store(true, Ordering::SeqCst);
    }

    fn reason(&self) -> Option<String> {
        self.reason.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
}

pub struct PluginProcess {
    pub pid: u32,
    pub started: Instant,
    child: Mutex<Child>,
    writer: Writer,
    pending: Pending,
    next_id: AtomicU64,
    fate: Arc<Fate>,
    last_latency_ms: AtomicU64,
}

/// Everything needed to start a plugin.
pub struct Launch<'a> {
    pub manifest: &'a Manifest,
    pub dir: &'a Path,
    pub data_dir: &'a Path,
    pub logs: Arc<LogBuffer>,
    /// Where the plugin's `controller.*` messages go; without one they are refused.
    pub controllers: Option<ControllerSink>,
}

/// Resolves `entry.command`: a path containing `/` is absolute or inside the plugin folder; a bare name comes from PATH.
pub fn resolve_command(dir: &Path, command: &str) -> Result<PathBuf, String> {
    let candidate = if command.contains('/') {
        let path = Path::new(command);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            let base = dir.canonicalize().map_err(|e| format!("{}: {e}", dir.display()))?;
            let resolved = base.join(path).canonicalize().map_err(|e| format!("entry.command \"{command}\": {e}"))?;
            if !resolved.starts_with(&base) {
                return Err(format!("entry.command \"{command}\" points outside the plugin folder"));
            }
            resolved
        }
    } else {
        PLUGIN_PATH
            .split(':')
            .map(|dir| Path::new(dir).join(command))
            .find(|path| path.is_file())
            .ok_or_else(|| format!("entry.command \"{command}\" was not found on PATH ({PLUGIN_PATH})"))?
    };
    if !candidate.is_file() {
        return Err(format!("entry.command {} is not a file", candidate.display()));
    }
    Ok(candidate)
}

/// A pipe for the plugin's stdin: the read end for the child, the write end for us, and a copy of the read end for
/// `FIONREAD`. All close on exec, so no other child inherits them.
fn stdin_pipe() -> std::io::Result<(OwnedFd, File, OwnedFd)> {
    let mut fds = [0 as libc::c_int; 2];
    // SAFETY: pipe() fills two fds on success; each is wrapped in an OwnedFd at once.
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: both fds were just created and are owned by nothing else.
    let (read, write) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
    for fd in [&read, &write] {
        // SAFETY: plain fcntl on an fd we own.
        if unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    // SAFETY: F_DUPFD_CLOEXEC returns a new fd we then own.
    let probe = unsafe { libc::fcntl(read.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 0) };
    if probe < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: probe is a fresh fd owned by nothing else.
    Ok((read, File::from(write), unsafe { OwnedFd::from_raw_fd(probe) }))
}

impl PluginProcess {
    pub fn spawn(launch: Launch<'_>) -> Result<Arc<PluginProcess>, String> {
        let manifest = launch.manifest;
        let program = resolve_command(launch.dir, &manifest.entry.command)?;
        crate::registry::private_dir(launch.data_dir)?;
        let (child_stdin, stdin, probe) = stdin_pipe().map_err(|e| format!("could not make a pipe for the plugin: {e}"))?;
        let mut command = Command::new(&program);
        command
            .args(&manifest.entry.args)
            .current_dir(launch.dir)
            // A plugin gets a clean environment: nothing of Studio's (tokens, paths) leaks into it.
            .env_clear()
            .env("PATH", PLUGIN_PATH)
            .env("LANG", "en_US.UTF-8")
            .env("PYTHONUNBUFFERED", "1")
            .env("VIXEN_PLUGIN_ID", &manifest.id)
            .env("VIXEN_PLUGIN_DATA_DIR", launch.data_dir)
            .stdin(Stdio::from(child_stdin))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // Its own process group, so stopping it also stops anything it started (npm → node).
            .process_group(0);
        for name in ["HOME", "USER", "LOGNAME", "TMPDIR"] {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        let mut child = command.spawn().map_err(|e| format!("could not start {}: {e}", program.display()))?;
        drop(command);
        let pid = child.id();
        let stdout = child.stdout.take().expect("piped");
        let stderr = child.stderr.take().expect("piped");
        let writer: Writer = Arc::new(Outbox { probe: Mutex::new(Some(probe)), ..Outbox::default() });
        let process = Arc::new(PluginProcess {
            pid,
            started: Instant::now(),
            child: Mutex::new(child),
            writer: writer.clone(),
            pending: Arc::default(),
            next_id: AtomicU64::new(1),
            fate: Arc::default(),
            last_latency_ms: AtomicU64::new(0),
        });
        let name = |what: &str| format!("plugin-{what}-{}", manifest.id);
        thread::Builder::new().name(name("stdin")).spawn(move || write_loop(stdin, writer)).map_err(|e| e.to_string())?;
        let logs = launch.logs.clone();
        thread::Builder::new()
            .name(name("stderr"))
            .spawn(move || {
                let mut reader = BufReader::new(stderr);
                loop {
                    match rpc::read_line(&mut reader, STDERR_LINE) {
                        Ok(Line::Message(line)) => logs.push(Level::Info, Source::Stderr, &String::from_utf8_lossy(&line)),
                        Ok(Line::TooLong) => logs.push(Level::Warn, Source::Host, "a stderr line over 16 KiB was skipped"),
                        Ok(Line::Eof) | Err(_) => break,
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        let reader = Reader {
            pid,
            pending: process.pending.clone(),
            fate: process.fate.clone(),
            logs: launch.logs.clone(),
            writer: process.writer.clone(),
            data_dir: launch.data_dir.to_path_buf(),
            storage: manifest.permissions.files.iter().any(|f| f == "plugin-data"),
            controllers: launch.controllers.clone(),
        };
        thread::Builder::new().name(name("stdout")).spawn(move || reader.run(stdout)).map_err(|e| e.to_string())?;
        Ok(process)
    }

    fn send(&self, bytes: Vec<u8>) -> Result<(), RpcError> {
        self.writer.push(bytes)
    }

    /// Queues a `controller.frame` notification for `instance`, replacing its frame the plugin hasn't read yet.
    pub fn send_frame(&self, instance: &str, params: Value) -> Result<FrameSent, RpcError> {
        if self.is_dead() {
            return Err(self.exited_error());
        }
        self.writer.push_frame(instance, rpc::notification("controller.frame", params))
    }

    /// Sends a request and waits up to `timeout` for the answer.
    pub fn request(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, RpcError> {
        if self.is_dead() {
            return Err(self.exited_error());
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx): (Sender<_>, Receiver<_>) = mpsc::channel();
        self.pending.lock().unwrap_or_else(|p| p.into_inner()).insert(id, tx);
        if self.fate.closed.load(Ordering::SeqCst) {
            self.pending.lock().unwrap_or_else(|p| p.into_inner()).remove(&id);
            return Err(self.exited_error());
        }
        let started = Instant::now();
        if let Err(error) = self.send(rpc::request(id, method, params)) {
            self.pending.lock().unwrap_or_else(|p| p.into_inner()).remove(&id);
            return Err(error);
        }
        let answer = rx.recv_timeout(timeout);
        self.pending.lock().unwrap_or_else(|p| p.into_inner()).remove(&id);
        match answer {
            Ok(result) => {
                self.last_latency_ms.store(started.elapsed().as_millis() as u64, Ordering::Relaxed);
                result
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                Err(RpcError::new(rpc::TIMED_OUT, format!("{method} got no answer within {} ms", timeout.as_millis())))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(self.exited_error()),
        }
    }

    pub fn notify(&self, method: &str, params: Value) -> Result<(), RpcError> {
        self.send(rpc::notification(method, params))
    }

    fn exited_error(&self) -> RpcError {
        RpcError::new(rpc::PLUGIN_EXITED, self.fate.reason().unwrap_or_else(|| "the plugin exited".into()))
    }

    /// True once the plugin broke the protocol or was killed; the supervisor then treats it as exited.
    pub fn is_dead(&self) -> bool {
        self.fate.dead.load(Ordering::SeqCst)
    }

    pub fn last_latency_ms(&self) -> u64 {
        self.last_latency_ms.load(Ordering::Relaxed)
    }

    /// The exit status once the process has ended (reaping it), else `None`.
    pub fn try_wait(&self) -> Option<ExitStatus> {
        self.child.lock().unwrap_or_else(|p| p.into_inner()).try_wait().ok().flatten()
    }

    /// Why the process ended, for the UI: our own reason when we have one, else its exit status.
    pub fn describe_exit(&self, status: Option<ExitStatus>) -> String {
        if let Some(reason) = self.fate.reason() {
            return reason;
        }
        use std::os::unix::process::ExitStatusExt;
        match status {
            Some(status) if status.code().is_some() => format!("plugin exited with status {}", status.code().unwrap_or(0)),
            Some(status) => format!("plugin was killed by signal {}", status.signal().unwrap_or(0)),
            None => "plugin exited".into(),
        }
    }

    /// Kills the plugin's whole process group at once, recording why.
    pub fn kill(&self, reason: &str) {
        self.fate.set(reason.to_string());
        self.writer.close();
        kill_group(self.pid);
        let _ = self.child.lock().unwrap_or_else(|p| p.into_inner()).kill();
        fail_pending(&self.pending, &self.fate);
    }

    /// Asks the plugin to shut down, closes its stdin, and kills it if it hasn't exited after `grace`.
    pub fn shutdown(&self, grace: Duration) {
        if !self.is_dead() && self.try_wait().is_none() {
            let _ = self.request("shutdown", json!({}), grace);
        }
        self.fate.set("stopped".into());
        self.writer.close();
        let deadline = Instant::now() + grace;
        while Instant::now() < deadline {
            if self.try_wait().is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        // Whatever is left of its process group goes too.
        kill_group(self.pid);
        let mut child = self.child.lock().unwrap_or_else(|p| p.into_inner());
        let _ = child.kill();
        let _ = child.wait();
        drop(child);
        fail_pending(&self.pending, &self.fate);
    }
}

impl Drop for PluginProcess {
    fn drop(&mut self) {
        self.writer.close();
        // Never leave a plugin behind.
        if self.try_wait().is_none() {
            kill_group(self.pid);
            let mut child = self.child.lock().unwrap_or_else(|p| p.into_inner());
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn kill_group(pid: u32) {
    if pid > 1 {
        // SAFETY: plain syscall; the negative pid names the group spawn() created for this plugin.
        unsafe {
            libc::kill(-(pid as libc::pid_t), libc::SIGKILL);
        }
    }
}

fn fail_pending(pending: &Pending, fate: &Fate) {
    let reason = fate.reason().unwrap_or_else(|| "the plugin exited".into());
    for (_, waiter) in pending.lock().unwrap_or_else(|p| p.into_inner()).drain() {
        let _ = waiter.send(Err(RpcError::new(rpc::PLUGIN_EXITED, reason.clone())));
    }
}

fn write_loop(mut stdin: File, outbox: Writer) {
    while let Some(bytes) = outbox.next() {
        if stdin.write_all(&bytes).and_then(|_| stdin.flush()).is_err() {
            break;
        }
    }
    // A plugin that stopped reading can't take more; refuse new messages rather than queue them.
    outbox.close();
}

struct Reader {
    pid: u32,
    pending: Pending,
    fate: Arc<Fate>,
    logs: Arc<LogBuffer>,
    writer: Writer,
    data_dir: PathBuf,
    storage: bool,
    controllers: Option<ControllerSink>,
}

impl Reader {
    fn run(self, stdout: std::process::ChildStdout) {
        let mut reader = BufReader::new(stdout);
        loop {
            let line = match rpc::read_line(&mut reader, rpc::MAX_LINE) {
                Ok(Line::Message(line)) => line,
                Ok(Line::TooLong) => return self.broken("wrote a line over 1 MiB to stdout"),
                Ok(Line::Eof) | Err(_) => break,
            };
            match rpc::parse(&line) {
                Ok(Incoming::Response { id, result }) => {
                    if let Some(waiter) = self.pending.lock().unwrap_or_else(|p| p.into_inner()).remove(&id) {
                        let _ = waiter.send(result);
                    }
                }
                Ok(Incoming::Request { id, method, params }) => {
                    let answer = self.handle(&method, params);
                    // A full queue means the plugin isn't reading; the answer is dropped like any other message.
                    let _ = self.writer.push(rpc::response(&id, answer));
                }
                Ok(Incoming::Notification { method, params }) => {
                    if method == "log" || CONTROLLER_METHODS.contains(&method.as_str()) {
                        if let Err(error) = self.handle(&method, params) {
                            self.logs.push(Level::Warn, Source::Host, &format!("{method}: {}", error.message));
                        }
                    }
                }
                Err(why) => {
                    let preview: String = String::from_utf8_lossy(&line).chars().take(120).collect();
                    return self.broken(&format!("wrote something other than JSON-RPC to stdout ({why}): {preview}"));
                }
            }
        }
        self.fate.closed.store(true, Ordering::SeqCst);
        // Its stdout closed: it has exited or is about to, so nothing more is written to it.
        self.writer.close();
        fail_pending(&self.pending, &self.fate);
    }

    /// The plugin broke the protocol: stop it now (logs belong on stderr; stdout is only for JSON-RPC).
    fn broken(&self, what: &str) {
        let reason = format!("plugin {what}");
        self.logs.push(Level::Error, Source::Host, &reason);
        self.fate.set(reason);
        self.writer.close();
        kill_group(self.pid);
        fail_pending(&self.pending, &self.fate);
    }

    fn handle(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        match method {
            "log" => {
                let level = params.get("level").and_then(Value::as_str).and_then(Level::parse).unwrap_or(Level::Info);
                let message = params.get("message").and_then(Value::as_str).ok_or_else(|| invalid("log needs a \"message\" string"))?;
                self.logs.push(level, Source::Plugin, message);
                Ok(json!({}))
            }
            "storage.get" | "storage.set" => {
                if !self.storage {
                    return Err(RpcError::new(rpc::PERMISSION_DENIED, "storage needs \"files\": [\"plugin-data\"] in the manifest's permissions"));
                }
                let key = params.get("key").and_then(Value::as_str).filter(|k| (1..=128).contains(&k.len())).ok_or_else(|| invalid("needs a \"key\" of 1 to 128 characters"))?;
                let path = self.data_dir.join("state.json");
                let mut state = match read_json(&path, MAX_STATE_BYTES as u64 + 1) {
                    Ok(Some(Value::Object(map))) => map,
                    Ok(_) => Map::new(),
                    Err(why) => return Err(RpcError::new(rpc::INTERNAL_ERROR, why)),
                };
                if method == "storage.get" {
                    return Ok(json!({"value": state.get(key).cloned().unwrap_or(Value::Null)}));
                }
                match params.get("value") {
                    None | Some(Value::Null) => {
                        state.remove(key);
                    }
                    Some(value) => {
                        state.insert(key.to_string(), value.clone());
                    }
                }
                let bytes = serde_json::to_vec(&Value::Object(state)).expect("serializes");
                if bytes.len() > MAX_STATE_BYTES {
                    return Err(invalid("storage is limited to 1 MiB in total"));
                }
                write_atomic(&path, &bytes).map_err(|why| RpcError::new(rpc::INTERNAL_ERROR, why))?;
                Ok(json!({}))
            }
            method if CONTROLLER_METHODS.contains(&method) => match &self.controllers {
                Some(sink) => sink(method, &params),
                None => Err(RpcError::new(rpc::METHOD_NOT_FOUND, format!("{method}: this plugin has no controller instances"))),
            },
            method if LATER_METHODS.contains(&method) => {
                Err(RpcError::new(rpc::METHOD_NOT_FOUND, format!("{method} is not available in this version of Vixen Studio")))
            }
            method => Err(RpcError::new(rpc::METHOD_NOT_FOUND, format!("unknown method {method}"))),
        }
    }
}

fn invalid(message: &str) -> RpcError {
    RpcError::new(rpc::INVALID_PARAMS, message)
}
