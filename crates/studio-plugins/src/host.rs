//! The plugin host: every known plugin's state, starting and stopping their processes, and a supervisor thread that
//! notices exits, restarts with backoff and hot-reloads development folders.
use crate::logs::{Level, LogBuffer, Source};
use crate::manifest::Manifest;
use crate::process::{Launch, PluginProcess};
use crate::registry::{Discovered, Registry, Source as PluginSource};
use crate::rpc::{self, RpcError};
use crate::API_VERSION;
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::thread;
use std::time::{Duration, Instant, UNIX_EPOCH};

#[derive(Clone, Debug)]
pub struct HostOptions {
    pub request_timeout: Duration,
    pub init_timeout: Duration,
    pub shutdown_grace: Duration,
    /// Delays before the first, second and third restart after a crash.
    pub backoff: [Duration; 3],
    /// Three restarts within this window and the host gives up until the plugin is reloaded.
    pub restart_window: Duration,
    pub tick: Duration,
    /// How often development folders are checked for changes. A change restarts the plugin once it has held for a
    /// second check (the debounce).
    pub dev_poll: Duration,
}

impl Default for HostOptions {
    fn default() -> HostOptions {
        HostOptions {
            request_timeout: Duration::from_secs(5),
            init_timeout: Duration::from_secs(5),
            shutdown_grace: Duration::from_secs(3),
            backoff: [Duration::from_secs(1), Duration::from_secs(5), Duration::from_secs(30)],
            restart_window: Duration::from_secs(60),
            tick: Duration::from_millis(200),
            dev_poll: Duration::from_millis(750),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// The manifest is missing or invalid.
    Invalid,
    /// The plugin needs a plugin API this Studio doesn't have, or answered `initialize` with one.
    Incompatible,
    Disabled,
    Stopped,
    Starting,
    Running,
    /// Exited unexpectedly; restarting after a backoff delay.
    Restarting,
    /// Exited too often; waits for a reload.
    Crashed,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Invalid => "invalid",
            State::Incompatible => "incompatible",
            State::Disabled => "disabled",
            State::Stopped => "stopped",
            State::Starting => "starting",
            State::Running => "running",
            State::Restarting => "restarting",
            State::Crashed => "crashed",
        }
    }
}

/// A cheap summary of a development folder's files, compared between polls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Fingerprint {
    files: u64,
    bytes: u64,
    newest: u128,
}

struct Slot {
    id: String,
    dir: PathBuf,
    source: PluginSource,
    enabled: bool,
    manifest: Option<Manifest>,
    /// Why the plugin can't run (invalid manifest, incompatible API).
    problem: Option<String>,
    state: State,
    process: Option<Arc<PluginProcess>>,
    /// Bumped by every start and stop, so a start that finishes after a stop knows it lost.
    generation: u64,
    restarts: VecDeque<Instant>,
    restart_count: u64,
    next_restart: Option<Instant>,
    last_error: Option<String>,
    plugin_api_version: Option<String>,
    capabilities: Value,
    logs: Arc<LogBuffer>,
    fingerprint: Option<Fingerprint>,
    changed: Option<Fingerprint>,
}

impl Slot {
    fn from(found: Discovered, registry: &Registry) -> Slot {
        let logs = Arc::new(LogBuffer::new(Some(registry.log_path(&found.id))));
        let mut slot = Slot {
            id: found.id.clone(),
            dir: PathBuf::new(),
            source: found.source,
            enabled: false,
            manifest: None,
            problem: None,
            state: State::Stopped,
            process: None,
            generation: 0,
            restarts: VecDeque::new(),
            restart_count: 0,
            next_restart: None,
            last_error: None,
            plugin_api_version: None,
            capabilities: Value::Null,
            logs,
            fingerprint: None,
            changed: None,
        };
        slot.update(found);
        slot
    }

    /// Takes a fresh look at the plugin on disk. A running plugin keeps its manifest until it is restarted.
    fn update(&mut self, found: Discovered) {
        self.enabled = found.enabled;
        if self.process.is_some() {
            return;
        }
        self.dir = found.dir;
        self.source = found.source;
        match found.manifest {
            Ok(manifest) if !manifest.api_version.matches(&API_VERSION) => {
                self.problem = Some(format!("needs plugin API {}; this Vixen Studio has {}", manifest.api_version.as_str(), API_VERSION));
                self.manifest = Some(manifest);
                self.state = State::Incompatible;
            }
            Ok(manifest) => {
                self.problem = None;
                self.manifest = Some(manifest);
                if matches!(self.state, State::Invalid | State::Incompatible | State::Disabled | State::Stopped) {
                    self.state = if self.enabled { State::Stopped } else { State::Disabled };
                }
            }
            Err(why) => {
                self.problem = Some(why);
                self.manifest = None;
                self.state = State::Invalid;
            }
        }
    }

    fn runnable(&self) -> bool {
        self.enabled && self.manifest.is_some() && self.problem.is_none()
    }

    fn idle_state(&self) -> State {
        match (&self.manifest, &self.problem) {
            (None, _) => State::Invalid,
            (Some(_), Some(_)) => State::Incompatible,
            _ if !self.enabled => State::Disabled,
            _ => State::Stopped,
        }
    }

    fn to_json(&self, detail: bool) -> Value {
        let mut out = match &self.manifest {
            Some(manifest) => manifest.summary(),
            None => json!({"id": self.id}),
        };
        out["source"] = json!(self.source.as_str());
        out["enabled"] = json!(self.enabled);
        out["state"] = json!(self.state.as_str());
        out["folder"] = json!(self.dir.to_string_lossy());
        out["restartCount"] = json!(self.restart_count);
        if let Some(process) = &self.process {
            out["pid"] = json!(process.pid);
            out["uptimeMs"] = json!(process.started.elapsed().as_millis() as u64);
        }
        if let Some(problem) = &self.problem {
            out["problem"] = json!(problem);
        }
        if let Some(error) = &self.last_error {
            out["lastError"] = json!(error);
        }
        if let Some(at) = self.next_restart {
            out["nextRestartMs"] = json!(at.saturating_duration_since(Instant::now()).as_millis() as u64);
        }
        if detail {
            if let Some(manifest) = &self.manifest {
                out["manifest"] = manifest.raw.clone();
                out["permissions"] = json!({
                    "network": manifest.permissions.network,
                    "files": manifest.permissions.files,
                    "display": manifest.permissions.display,
                    "serial": manifest.permissions.serial,
                });
            }
            out["negotiatedApiVersion"] = json!(self.plugin_api_version);
            out["capabilities"] = self.capabilities.clone();
            out["lastLatencyMs"] = json!(self.process.as_ref().map(|p| p.last_latency_ms()));
        }
        out
    }
}

struct Inner {
    options: HostOptions,
    registry: Mutex<Registry>,
    slots: Mutex<BTreeMap<String, Slot>>,
    muted: AtomicBool,
    profile: Mutex<Option<(String, String)>>,
    stopping: AtomicBool,
}

impl Inner {
    fn slots(&self) -> MutexGuard<'_, BTreeMap<String, Slot>> {
        self.slots.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn registry(&self) -> MutexGuard<'_, Registry> {
        self.registry.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// Owns every plugin process. Dropping it (or [`PluginHost::shutdown_all`]) stops them all.
pub struct PluginHost {
    inner: Arc<Inner>,
}

impl PluginHost {
    /// Opens the plugin folder at `root` (created 0700) and starts the supervisor. No plugin starts until
    /// [`PluginHost::start_enabled`] or [`PluginHost::start`].
    pub fn new(root: &Path, options: HostOptions) -> Result<PluginHost, String> {
        let registry = Registry::open(root)?;
        let warning = registry.warning.clone();
        let inner = Arc::new(Inner {
            options,
            registry: Mutex::new(registry),
            slots: Mutex::new(BTreeMap::new()),
            muted: AtomicBool::new(true),
            profile: Mutex::new(None),
            stopping: AtomicBool::new(false),
        });
        let host = PluginHost { inner };
        host.refresh();
        if let Some(warning) = warning {
            for slot in host.inner.slots().values() {
                slot.logs.push(Level::Warn, Source::Host, &warning);
            }
        }
        let weak = Arc::downgrade(&host.inner);
        thread::Builder::new().name("plugin-supervisor".into()).spawn(move || supervise(weak)).map_err(|e| e.to_string())?;
        Ok(host)
    }

    pub fn root(&self) -> PathBuf {
        self.inner.registry().root().to_path_buf()
    }

    /// Looks at the registry and the plugin folder again: new plugins appear, removed ones stop and disappear.
    pub fn refresh(&self) {
        refresh(&self.inner);
    }

    /// Starts every enabled plugin that isn't running, each on its own thread. Returns the ids started.
    pub fn start_enabled(&self) -> Vec<String> {
        let ids: Vec<String> = self
            .inner
            .slots()
            .values()
            .filter(|s| s.runnable() && matches!(s.state, State::Stopped))
            .map(|s| s.id.clone())
            .collect();
        for id in &ids {
            let inner = self.inner.clone();
            let id = id.clone();
            let _ = thread::Builder::new().name(format!("plugin-start-{id}")).spawn(move || start(&inner, &id, false));
        }
        ids
    }

    /// Starts one plugin and waits for its `initialize` answer.
    pub fn start(&self, id: &str) -> Result<Value, String> {
        start(&self.inner, id, true)?;
        self.info(id)
    }

    /// Stops one plugin (shutdown, then kill after the grace period). It stays enabled.
    pub fn stop(&self, id: &str) -> Result<Value, String> {
        stop(&self.inner, id)?;
        self.info(id)
    }

    /// Stops the plugin, reads its manifest again, forgets earlier crashes and starts it when enabled.
    pub fn reload(&self, id: &str) -> Result<Value, String> {
        reload(&self.inner, id)?;
        self.info(id)
    }

    /// Enables (and starts) or disables (and stops) a plugin, remembering the choice in the registry.
    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<Value, String> {
        if !self.inner.slots().contains_key(id) {
            return Err(unknown(id));
        }
        self.inner.registry().set_enabled(id, enabled)?;
        if !enabled {
            stop(&self.inner, id)?;
        }
        self.refresh();
        if enabled {
            self.reload(id)
        } else {
            self.info(id)
        }
    }

    /// Links a development folder and starts the plugin in it.
    pub fn link_dev(&self, folder: &Path) -> Result<Value, String> {
        let manifest = self.inner.registry().link_dev(folder)?;
        stop(&self.inner, &manifest.id).ok();
        self.refresh();
        self.reload(&manifest.id)
    }

    /// Removes a development link (the folder itself is left alone).
    pub fn unlink_dev(&self, id: &str) -> Result<(), String> {
        let is_dev = self.inner.registry().record(id).is_some_and(|r| r.source == PluginSource::Dev);
        if !is_dev {
            return Err(format!("{id} is not a linked development folder"));
        }
        stop(&self.inner, id)?;
        self.inner.registry().remove_record(id)?;
        self.refresh();
        Ok(())
    }

    /// `{apiVersion, folder, muted, plugins: [...]}` for `plugin_list` and Settings ▸ Plugins.
    pub fn list(&self) -> Value {
        let plugins: Vec<Value> = self.inner.slots().values().map(|s| s.to_json(false)).collect();
        json!({
            "apiVersion": API_VERSION.to_string(),
            "folder": self.root().to_string_lossy(),
            "muted": self.inner.muted.load(Ordering::SeqCst),
            "plugins": plugins,
        })
    }

    /// One plugin in detail: manifest, permissions, process and negotiated API.
    pub fn info(&self, id: &str) -> Result<Value, String> {
        let mut info = self.inner.slots().get(id).map(|s| s.to_json(true)).ok_or_else(|| unknown(id))?;
        let registry = self.inner.registry();
        info["dataFolder"] = json!(registry.data_dir(id).to_string_lossy());
        info["logFile"] = json!(registry.log_path(id).to_string_lossy());
        Ok(info)
    }

    /// The newest `limit` log entries at or above `min`, oldest first.
    pub fn logs(&self, id: &str, limit: usize, min: Level) -> Result<Value, String> {
        let logs = self.inner.slots().get(id).map(|s| s.logs.clone()).ok_or_else(|| unknown(id))?;
        let entries: Vec<Value> = logs.tail(limit.clamp(1, crate::logs::CAPACITY), min).iter().map(|e| e.to_json()).collect();
        Ok(json!({"id": id, "entries": entries, "logFile": self.inner.registry().log_path(id).to_string_lossy()}))
    }

    /// Whether real output may flow. Plugins learn it in `initialize` and, when they declare it, the
    /// `liveOutput.changed` event.
    pub fn set_muted(&self, muted: bool) {
        if self.inner.muted.swap(muted, Ordering::SeqCst) == muted {
            return;
        }
        for process in self.running_with_event("liveOutput.changed") {
            let _ = process.notify("event", json!({"name": "liveOutput.changed", "data": {"muted": muted}}));
        }
    }

    pub fn muted(&self) -> bool {
        self.inner.muted.load(Ordering::SeqCst)
    }

    /// The open profile, passed to plugins started from now on.
    pub fn set_profile(&self, profile: Option<(String, String)>) {
        *self.inner.profile.lock().unwrap_or_else(|p| p.into_inner()) = profile;
    }

    fn running_with_event(&self, event: &str) -> Vec<Arc<PluginProcess>> {
        self.inner
            .slots()
            .values()
            .filter(|s| s.state == State::Running && s.manifest.as_ref().is_some_and(|m| m.events.iter().any(|e| e == event)))
            .filter_map(|s| s.process.clone())
            .collect()
    }

    /// A request to a running plugin (for the controller and tool stages built on this host).
    pub fn request(&self, id: &str, method: &str, params: Value, timeout: Option<Duration>) -> Result<Value, RpcError> {
        let process = self.inner.slots().get(id).filter(|s| s.state == State::Running).and_then(|s| s.process.clone());
        let process = process.ok_or_else(|| RpcError::new(rpc::PLUGIN_EXITED, format!("{id} is not running")))?;
        process.request(method, params, timeout.unwrap_or(self.inner.options.request_timeout))
    }

    /// The plugins `profile`'s show needs (its StudioPlugins.json) and whether each is installed, enabled and recent
    /// enough: `{plugins: [{id, versionRange, installedVersion, state, satisfied, message?}]}`.
    pub fn requirements(&self, profile: &Path) -> Result<Value, String> {
        let requirements = crate::profile::read(profile)?;
        let slots = self.inner.slots();
        let plugins: Vec<Value> = requirements
            .iter()
            .map(|need| {
                let range = crate::semver::Range::parse(&need.version_range).expect("read validates ranges");
                let slot = slots.get(&need.id);
                let version = slot.and_then(|s| s.manifest.as_ref()).map(|m| m.version.clone());
                let satisfied = slot.is_some_and(Slot::runnable) && version.as_ref().is_some_and(|v| range.matches(v));
                let message = match (slot, &version) {
                    _ if satisfied => None,
                    (None, _) | (Some(_), None) => Some(format!("Needs plugin {} {} — not installed", need.id, need.version_range)),
                    (Some(_), Some(v)) if !range.matches(v) => Some(format!("Needs plugin {} {} — version {v} is installed", need.id, need.version_range)),
                    (Some(slot), Some(_)) => Some(format!("Needs plugin {} — it is {}", need.id, slot.state.as_str())),
                };
                json!({
                    "id": need.id,
                    "versionRange": need.version_range,
                    "installedVersion": version.map(|v| v.to_string()),
                    "state": slot.map(|s| s.state.as_str()),
                    "satisfied": satisfied,
                    "message": message,
                })
            })
            .collect();
        Ok(json!({"plugins": plugins}))
    }

    pub fn state(&self, id: &str) -> Option<State> {
        self.inner.slots().get(id).map(|s| s.state)
    }

    /// Stops the supervisor and every plugin (in parallel), waiting for them to exit.
    pub fn shutdown_all(&self) {
        self.inner.stopping.store(true, Ordering::SeqCst);
        let processes: Vec<Arc<PluginProcess>> = {
            let mut slots = self.inner.slots();
            slots
                .values_mut()
                .filter_map(|slot| {
                    slot.generation += 1;
                    slot.next_restart = None;
                    slot.state = slot.idle_state();
                    slot.process.take()
                })
                .collect()
        };
        let grace = self.inner.options.shutdown_grace;
        let threads: Vec<_> = processes.into_iter().map(|p| thread::spawn(move || p.shutdown(grace))).collect();
        for thread in threads {
            let _ = thread.join();
        }
    }
}

impl Drop for PluginHost {
    fn drop(&mut self) {
        self.shutdown_all();
    }
}

fn refresh(inner: &Inner) {
    let found = inner.registry().discover();
    let mut gone = Vec::new();
    {
        let registry = inner.registry();
        let mut slots = inner.slots();
        let ids: Vec<String> = found.iter().map(|f| f.id.clone()).collect();
        for found in found {
            match slots.get_mut(&found.id) {
                Some(slot) => slot.update(found),
                None => {
                    slots.insert(found.id.clone(), Slot::from(found, &registry));
                }
            }
        }
        slots.retain(|id, slot| {
            let keep = ids.contains(id);
            if !keep {
                gone.extend(slot.process.take());
            }
            keep
        });
    }
    for process in gone {
        process.shutdown(inner.options.shutdown_grace);
    }
}

fn reload(inner: &Arc<Inner>, id: &str) -> Result<(), String> {
    stop(inner, id)?;
    refresh(inner);
    let runnable = {
        let mut slots = inner.slots();
        let slot = slots.get_mut(id).ok_or_else(|| unknown(id))?;
        slot.restarts.clear();
        slot.next_restart = None;
        slot.last_error = None;
        slot.state = slot.idle_state();
        slot.logs.push(Level::Info, Source::Host, "reloaded");
        slot.runnable()
    };
    if runnable {
        start(inner, id, true)?;
    }
    Ok(())
}

fn unknown(id: &str) -> String {
    format!("no plugin with id {id}")
}

/// Starts `id` unless it is already starting or running. `wait` reports the outcome; a background start only logs it.
fn start(inner: &Arc<Inner>, id: &str, wait: bool) -> Result<(), String> {
    // The registry lock is always taken before the slots lock, never inside it.
    let data_dir = inner.registry().data_dir(id);
    let (generation, manifest, dir, logs) = {
        let mut slots = inner.slots();
        let slot = slots.get_mut(id).ok_or_else(|| unknown(id))?;
        if matches!(slot.state, State::Starting | State::Running) {
            return Ok(());
        }
        if !slot.runnable() {
            return Err(slot.problem.clone().unwrap_or_else(|| format!("{id} is disabled")));
        }
        slot.generation += 1;
        slot.state = State::Starting;
        slot.next_restart = None;
        (slot.generation, slot.manifest.clone().expect("runnable"), slot.dir.clone(), slot.logs.clone())
    };
    if inner.stopping.load(Ordering::SeqCst) {
        return Err("the plugin host is shutting down".into());
    }
    let spawned = PluginProcess::spawn(Launch { manifest: &manifest, dir: &dir, data_dir: &data_dir, logs: logs.clone() });
    let process = match spawned {
        Ok(process) => process,
        Err(why) => {
            fail(inner, id, generation, &why, false);
            return if wait { Err(why) } else { Ok(()) };
        }
    };
    {
        let mut slots = inner.slots();
        match slots.get_mut(id) {
            Some(slot) if slot.generation == generation => slot.process = Some(process.clone()),
            _ => {
                drop(slots);
                process.shutdown(inner.options.shutdown_grace);
                return Err(format!("{id} was stopped while starting"));
            }
        }
    }
    logs.push(Level::Info, Source::Host, &format!("started {} {} (pid {})", manifest.name, manifest.version, process.pid));
    let profile = inner.profile.lock().unwrap_or_else(|p| p.into_inner()).clone();
    let params = json!({
        "apiVersion": API_VERSION.to_string(),
        "hostVersion": crate::HOST_VERSION,
        "pluginDataDir": data_dir.to_string_lossy(),
        "profile": profile.map(|(name, id)| json!({"name": name, "id": id})),
        "muted": inner.muted.load(Ordering::SeqCst),
        "capabilities": crate::host_capabilities(),
    });
    let answer = process.request("initialize", params, inner.options.init_timeout);
    let outcome = answer.map_err(|e| (format!("initialize failed: {}", e.message), false)).and_then(|result| {
        let version = result.get("apiVersion").and_then(Value::as_str).ok_or(("initialize answered without an apiVersion".to_string(), false))?;
        let parsed = crate::semver::Version::parse(version).map_err(|why| (format!("initialize answered an invalid apiVersion: {why}"), false))?;
        if parsed.major != API_VERSION.major {
            return Err((format!("the plugin speaks plugin API {version}; this Vixen Studio has {API_VERSION}"), true));
        }
        Ok((version.to_string(), result.get("capabilities").cloned().unwrap_or(Value::Null)))
    });
    match outcome {
        Ok((version, capabilities)) => {
            let mut slots = inner.slots();
            if let Some(slot) = slots.get_mut(id).filter(|s| s.generation == generation) {
                slot.state = State::Running;
                slot.plugin_api_version = Some(version);
                slot.capabilities = capabilities;
            }
            Ok(())
        }
        Err((why, incompatible)) => {
            process.kill(&why);
            fail(inner, id, generation, &why, incompatible);
            if wait { Err(why) } else { Ok(()) }
        }
    }
}

/// Records a failed start or an unexpected exit and schedules a restart, unless the plugin has failed too often.
fn fail(inner: &Inner, id: &str, generation: u64, why: &str, incompatible: bool) {
    let process = {
        let mut slots = inner.slots();
        let Some(slot) = slots.get_mut(id).filter(|s| s.generation == generation) else { return };
        failed(slot, why, incompatible, &inner.options)
    };
    drop(process);
}

fn failed(slot: &mut Slot, why: &str, incompatible: bool, options: &HostOptions) -> Option<Arc<PluginProcess>> {
    let process = slot.process.take();
    slot.last_error = Some(why.to_string());
    slot.logs.push(Level::Error, Source::Host, why);
    if incompatible {
        slot.problem = Some(why.to_string());
        slot.state = State::Incompatible;
        slot.next_restart = None;
        return process;
    }
    let now = Instant::now();
    while slot.restarts.front().is_some_and(|t| now.duration_since(*t) > options.restart_window) {
        slot.restarts.pop_front();
    }
    if slot.restarts.len() >= options.backoff.len() {
        slot.state = State::Crashed;
        slot.next_restart = None;
        slot.logs.push(Level::Error, Source::Host, &format!("gave up after {} restarts within {} s; reload the plugin to try again", slot.restarts.len(), options.restart_window.as_secs()));
    } else {
        let delay = options.backoff[slot.restarts.len()];
        slot.restarts.push_back(now);
        slot.restart_count += 1;
        slot.next_restart = Some(now + delay);
        slot.state = State::Restarting;
        slot.logs.push(Level::Warn, Source::Host, &format!("restarting in {} ms", delay.as_millis()));
    }
    process
}

fn stop(inner: &Inner, id: &str) -> Result<(), String> {
    let process = {
        let mut slots = inner.slots();
        let slot = slots.get_mut(id).ok_or_else(|| unknown(id))?;
        slot.generation += 1;
        slot.next_restart = None;
        let process = slot.process.take();
        slot.state = slot.idle_state();
        if process.is_some() {
            slot.logs.push(Level::Info, Source::Host, "stopped");
        }
        process
    };
    if let Some(process) = process {
        process.shutdown(inner.options.shutdown_grace);
    }
    Ok(())
}

fn supervise(weak: Weak<Inner>) {
    let mut last_dev_poll = Instant::now();
    loop {
        let Some(inner) = weak.upgrade() else { return };
        let tick = inner.options.tick;
        if inner.stopping.load(Ordering::SeqCst) {
            return;
        }
        let now = Instant::now();
        let mut reaped = Vec::new();
        let mut due = Vec::new();
        let mut dev = Vec::new();
        let poll_dev = now.duration_since(last_dev_poll) >= inner.options.dev_poll;
        {
            let mut slots = inner.slots();
            for slot in slots.values_mut() {
                match slot.state {
                    State::Running => {
                        let Some(process) = slot.process.clone() else { continue };
                        let status = process.try_wait();
                        if status.is_some() || process.is_dead() {
                            let why = process.describe_exit(status);
                            reaped.extend(failed(slot, &why, false, &inner.options));
                        }
                    }
                    State::Restarting if slot.next_restart.is_some_and(|at| at <= now) => due.push(slot.id.clone()),
                    _ => {}
                }
                if poll_dev && slot.source == PluginSource::Dev && slot.enabled {
                    dev.push((slot.id.clone(), slot.dir.clone()));
                }
            }
        }
        for process in reaped {
            process.kill("exited");
        }
        for id in due {
            let inner = inner.clone();
            let _ = thread::Builder::new().name(format!("plugin-restart-{id}")).spawn(move || start(&inner, &id, false));
        }
        if poll_dev {
            last_dev_poll = now;
            for (id, dir) in dev {
                let print = fingerprint(&dir);
                let settled = {
                    let mut slots = inner.slots();
                    let Some(slot) = slots.get_mut(&id) else { continue };
                    match slot.fingerprint {
                        None => {
                            slot.fingerprint = print;
                            false
                        }
                        Some(known) if Some(known) == print => {
                            slot.changed = None;
                            false
                        }
                        // Changed and unchanged since the last poll: the edit has settled.
                        Some(_) if slot.changed == print => {
                            slot.fingerprint = print;
                            slot.changed = None;
                            slot.logs.push(Level::Info, Source::Host, "development folder changed; restarting");
                            true
                        }
                        Some(_) => {
                            slot.changed = print;
                            false
                        }
                    }
                };
                if settled {
                    let _ = reload(&inner, &id);
                }
            }
        }
        drop(inner);
        thread::sleep(tick);
    }
}

/// Files, total size and newest modification under `dir`, skipping hidden folders and build/dependency folders.
fn fingerprint(dir: &Path) -> Option<Fingerprint> {
    const SKIP: [&str; 5] = ["node_modules", "target", "__pycache__", "build", "dist"];
    let mut print = Fingerprint { files: 0, bytes: 0, newest: 0 };
    let mut stack = vec![(dir.to_path_buf(), 0)];
    while let Some((folder, depth)) = stack.pop() {
        for entry in std::fs::read_dir(&folder).ok()?.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let Ok(meta) = entry.path().symlink_metadata() else { continue };
            if meta.is_dir() {
                if depth < 6 && !name.starts_with('.') && !SKIP.contains(&name.as_ref()) {
                    stack.push((entry.path(), depth + 1));
                }
            } else if meta.is_file() {
                print.files += 1;
                print.bytes += meta.len();
                let modified = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_nanos()).unwrap_or(0);
                print.newest = print.newest.max(modified);
                if print.files > 5000 {
                    return Some(print);
                }
            }
        }
    }
    Some(print)
}
