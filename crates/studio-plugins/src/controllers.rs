//! Plugin controller types: the `PluginOutputModule` that drives a plugin's controller instance, the provider that makes
//! plugin types load, save and run like built-in ones (`vixen_output::modules::studio_external`), and the routing of a
//! plugin's `controller.status` / `controller.stats` / `controller.message` to the instance they name.
//!
//! The engine never waits on a plugin. A module's start, stop, configuration and output-count changes go to a worker
//! thread of its own, which also notices the plugin restarting and creates, configures and starts the instance again.
//! Frames go into the plugin's per-instance frame slot: a plugin that reads slowly loses frames (counted as dropped)
//! and never holds up the controller thread or any other controller.
//!
//! Module data in `ModuleStore.xml`, under the plugin's `typeId`:
//!
//! ```xml
//! <PluginControllerData>
//!   <ModuleInstanceId>…</ModuleInstanceId><ModuleTypeId>…</ModuleTypeId>
//!   <PluginId>io.github.someone.govee-output</PluginId><PluginName>Govee Output</PluginName>
//!   <PluginVersion>1.2.0</PluginVersion><Config>{"devices":[…]}</Config>
//! </PluginControllerData>
//! ```
use crate::host::{Inner, State};
use crate::manifest::{ControllerType, Manifest};
use crate::process::PluginProcess;
use crate::rpc::{self, RpcError};
use crate::semver::Version;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;
use vixen_output::modules::studio_external::{ExternalConfig, ExternalControllers, ExternalLight, ExternalOutputs, ExternalType};
use vixen_output::modules::{ConfigMismatch, ControllerMessage, ModuleShared, ModuleStatus};
use vixen_output::{ControllerConfig, DataPolicy, ModuleIds, OutputCommand, OutputModule};
use vixen_xml::XElement;

pub const DATA_ELEMENT: &str = "PluginControllerData";
/// `controller.start` may take longer than other requests (a device handshake).
const START_TIMEOUT: Duration = Duration::from_secs(15);
/// The JSON-RPC error a plugin answers `controller.configure` with when it refuses a configuration.
pub const INVALID_CONFIG: i64 = -32004;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

// ---- Module data --------------------------------------------------------------------------------------------------

/// What a plugin controller's module data says.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginData {
    pub plugin_id: String,
    pub plugin_name: String,
    pub plugin_version: Option<String>,
    /// The configuration, or `Err` with the text that isn't valid JSON.
    pub config: Result<Value, String>,
}

impl PluginData {
    pub fn read(data: &XElement) -> Option<PluginData> {
        if data.local != DATA_ELEMENT {
            return None;
        }
        let plugin_id = data.child_text("PluginId").filter(|id| crate::manifest::valid_plugin_id(id))?;
        let config_text = data.child_text("Config").unwrap_or_default();
        let config = if config_text.trim().is_empty() {
            Ok(json!({}))
        } else {
            serde_json::from_str::<Value>(&config_text).map_err(|_| config_text.clone())
        };
        Some(PluginData {
            plugin_name: data.child_text("PluginName").filter(|n| !n.is_empty()).unwrap_or_else(|| plugin_id.clone()),
            plugin_version: data.child_text("PluginVersion").filter(|v| Version::parse(v).is_ok()),
            plugin_id,
            config,
        })
    }

    /// The range a show needs: the same major version, at least the minor it was set up with (`^1.2`).
    pub fn version_range(&self) -> String {
        match self.plugin_version.as_deref().and_then(|v| Version::parse(v).ok()) {
            Some(v) if v.major == 0 => format!("^0.{}.{}", v.minor, v.patch),
            Some(v) => format!("^{}.{}", v.major, v.minor),
            None => "*".into(),
        }
    }
}

/// New module data for an instance of a plugin's type.
pub fn write_data(ids: &ModuleIds, manifest: &Manifest, config: &Value) -> XElement {
    let mut e = XElement::new("", DATA_ELEMENT);
    e.push(XElement::with_text("", "ModuleInstanceId", ids.instance_id.to_string()));
    e.push(XElement::with_text("", "ModuleTypeId", ids.type_id.to_string()));
    e.push(XElement::with_text("", "PluginId", manifest.id.clone()));
    e.push(XElement::with_text("", "PluginName", manifest.name.clone()));
    e.push(XElement::with_text("", "PluginVersion", manifest.version.to_string()));
    e.push(XElement::with_text("", "Config", config.to_string()));
    e
}

/// Replaces the `<Config>` text, keeping everything else in the element as it was.
pub fn set_config_text(data: &mut XElement, config: &Value) {
    let text = XElement::with_text("", "Config", config.to_string());
    match data.child_mut("Config") {
        Some(existing) => *existing = text,
        None => {
            data.push(text);
        }
    }
}

// ---- What the host knows about plugin types ---------------------------------------------------------------------

/// A controller type found in a plugin's manifest, with the plugin's state.
#[derive(Clone, Debug)]
pub struct TypeInfo {
    pub plugin_id: String,
    pub plugin_name: String,
    pub version: Version,
    pub state: State,
    pub runnable: bool,
    pub controller: ControllerType,
}

pub(crate) fn find_type(inner: &Inner, type_id: &Uuid) -> Option<TypeInfo> {
    crate::host::controller_types(inner).into_iter().find(|t| Uuid::parse_str(&t.controller.type_id).ok().as_ref() == Some(type_id))
}

/// Why a controller with this data can't send now, if it can't.
fn problem(inner: &Inner, type_id: &Uuid, data: &PluginData) -> Option<String> {
    let needs = || format!("Needs plugin {} ({}) {}", data.plugin_name, data.plugin_id, data.version_range());
    let types = crate::host::controller_types(inner);
    let found = types.iter().find(|t| t.plugin_id == data.plugin_id);
    let state = crate::host::plugin_state(inner, &data.plugin_id);
    let Some(state) = state else { return Some(format!("{} — not installed", needs())) };
    let manifest_version = crate::host::plugin_version(inner, &data.plugin_id);
    if let (Some(installed), Ok(range)) = (&manifest_version, crate::semver::Range::parse(&data.version_range())) {
        if !range.matches(installed) {
            return Some(format!("{} — version {installed} is installed", needs()));
        }
    }
    match state {
        State::Running | State::Starting | State::Stopped => {}
        State::Restarting => return Some(format!("Plugin {} ({}) is restarting after a crash", data.plugin_name, data.plugin_id)),
        State::Disabled => return Some(format!("{} — it is disabled", needs())),
        State::Crashed => return Some(format!("{} — it crashed and waits to be reloaded", needs())),
        State::Incompatible => return Some(format!("{} — it needs a different plugin API", needs())),
        State::Invalid => return Some(format!("{} — its manifest is invalid", needs())),
    }
    let offered = types.iter().any(|t| t.plugin_id == data.plugin_id && Uuid::parse_str(&t.controller.type_id).ok().as_ref() == Some(type_id));
    if found.is_some() && !offered {
        return Some(format!("Plugin {} ({}) no longer offers this controller type", data.plugin_name, data.plugin_id));
    }
    if let Err(text) = &data.config {
        let preview: String = text.chars().take(60).collect();
        return Some(format!("The saved configuration isn't valid JSON: {preview}"));
    }
    None
}

// ---- Instances ---------------------------------------------------------------------------------------------------

/// One controller instance's live state, shared by its module, its worker and the host's message routing.
pub(crate) struct Instance {
    pub id: String,
    pub plugin_id: String,
    pub type_id: Uuid,
    shared: Arc<ModuleShared>,
    /// Created and started in the plugin: frames flow.
    live: AtomicBool,
    process: Mutex<Option<Arc<PluginProcess>>>,
    frames_sent: AtomicU64,
    frames_dropped: AtomicU64,
    last_frame: Mutex<Option<Instant>>,
    /// The plugin's field errors from its last refused `controller.configure`.
    config_errors: Mutex<Vec<Value>>,
    /// The plugin has set this instance's status itself since it was last created.
    reported: AtomicBool,
}

/// One lock per instance id, held by a worker for its whole life, so the worker of a module that replaced another for
/// the same controller waits until the old one has sent `controller.destroy` before it sends `controller.create`.
fn gate(instance_id: &str) -> Arc<Mutex<()>> {
    static GATES: Mutex<Option<HashMap<String, Weak<Mutex<()>>>>> = Mutex::new(None);
    let mut guard = lock(&GATES);
    let gates = guard.get_or_insert_with(HashMap::new);
    gates.retain(|_, g| g.strong_count() > 0);
    if let Some(existing) = gates.get(instance_id).and_then(Weak::upgrade) {
        return existing;
    }
    let made = Arc::new(Mutex::new(()));
    gates.insert(instance_id.to_string(), Arc::downgrade(&made));
    made
}

impl Instance {
    pub fn to_json(&self) -> Value {
        let status = match self.shared.status() {
            ModuleStatus::Stopped => json!({"state": "stopped"}),
            ModuleStatus::Running => json!({"state": "running"}),
            ModuleStatus::Unavailable(m) => json!({"state": "unavailable", "message": m}),
            ModuleStatus::UnsupportedOnMacOs(m) | ModuleStatus::Error(m) => json!({"state": "error", "message": m}),
        };
        json!({
            "instanceId": self.id,
            "typeId": self.type_id.to_string(),
            "live": self.live.load(Ordering::SeqCst),
            "status": status,
            "framesSent": self.frames_sent.load(Ordering::Relaxed),
            "framesDropped": self.frames_dropped.load(Ordering::Relaxed),
            "lastFrameAgeMs": lock(&self.last_frame).map(|t| t.elapsed().as_millis() as u64),
            "packetsSent": self.shared.packets_sent(),
            "bytesSent": self.shared.bytes_sent(),
            "errors": self.shared.error_count(),
            "lastError": self.shared.last_error(),
            "configErrors": lock(&self.config_errors).clone(),
        })
    }
}

/// Answers a plugin's `controller.*` message for one of its own instances.
pub(crate) fn route(inner: &Inner, plugin_id: &str, method: &str, params: &Value) -> Result<Value, RpcError> {
    let invalid = |m: &str| RpcError::new(rpc::INVALID_PARAMS, m);
    let instance_id = params.get("instanceId").and_then(Value::as_str).ok_or_else(|| invalid("needs an \"instanceId\""))?;
    let instance = crate::host::instance(inner, instance_id)
        .filter(|i| i.plugin_id == plugin_id)
        .ok_or_else(|| invalid(&format!("no controller instance {instance_id} belongs to this plugin")))?;
    let text = |key: &str, max: usize| params.get(key).and_then(Value::as_str).map(|t| t.chars().take(max).collect::<String>());
    match method {
        "controller.status" => {
            let message = text("message", 500).unwrap_or_default();
            let status = match params.get("state").and_then(Value::as_str) {
                Some("running") => ModuleStatus::Running,
                Some("unavailable") => ModuleStatus::Unavailable(if message.is_empty() { "The device can't be reached".into() } else { message }),
                Some("error") => ModuleStatus::Error(if message.is_empty() { "The plugin reported an error".into() } else { message }),
                _ => return Err(invalid("state must be running, unavailable or error")),
            };
            // A state is shown while it's current, not counted as a send error: plugins say `unavailable` while they look
            // for their devices at every start, then `running`. Send errors come from `controller.stats`.
            instance.reported.store(true, Ordering::SeqCst);
            instance.shared.set_status(status);
        }
        "controller.stats" => {
            let n = |key: &str| params.get(key).and_then(Value::as_u64).unwrap_or(0);
            instance.shared.raise_counters(n("packetsSent"), n("bytesSent"), n("errors"));
        }
        "controller.message" => {
            let title = text("title", 200).ok_or_else(|| invalid("needs a \"title\""))?;
            let body = text("text", 4000).ok_or_else(|| invalid("needs a \"text\""))?;
            instance.shared.post_message(ControllerMessage { title, heading: text("heading", 500), text: body });
        }
        _ => return Err(RpcError::new(rpc::METHOD_NOT_FOUND, format!("unknown method {method}"))),
    }
    Ok(json!({}))
}

// ---- Frames ------------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    U8,
    Json,
    U16,
}

impl Encoding {
    /// The first encoding the plugin lists.
    pub fn preferred(controller: &ControllerType) -> Encoding {
        match controller.frame_encodings.first().map(String::as_str) {
            Some("json") => Encoding::Json,
            Some("u16") => Encoding::U16,
            _ => Encoding::U8,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Encoding::U8 => "u8",
            Encoding::Json => "json",
            Encoding::U16 => "u16",
        }
    }

    /// One frame's `values`. `u8`: each output's 8-bit value (0 for anything else, as every byte-oriented controller
    /// sends). `u16`: 16-bit values, 8-bit ones scaled to the full range (×257), little-endian. `json`: every command as
    /// it is: null, an integer, `{r,g,b}` or a string.
    pub fn encode(self, commands: &[OutputCommand]) -> Value {
        match self {
            Encoding::U8 => {
                let bytes: Vec<u8> = commands.iter().map(OutputCommand::byte).collect();
                json!(vixen_xml::nbfx::base64_encode(&bytes))
            }
            Encoding::U16 => {
                let mut bytes = Vec::with_capacity(commands.len() * 2);
                for c in commands {
                    let v = match c {
                        OutputCommand::U16(v) => *v,
                        OutputCommand::U8(v) => u16::from(*v) * 257,
                        _ => 0,
                    };
                    bytes.extend_from_slice(&v.to_le_bytes());
                }
                json!(vixen_xml::nbfx::base64_encode(&bytes))
            }
            Encoding::Json => Value::Array(
                commands
                    .iter()
                    .map(|c| match c {
                        OutputCommand::None => Value::Null,
                        OutputCommand::U8(v) => json!(v),
                        OutputCommand::U16(v) => json!(v),
                        OutputCommand::U32(v) => json!(v),
                        OutputCommand::U64(v) => json!(v),
                        OutputCommand::Color(c) => json!({"r": c.r, "g": c.g, "b": c.b}),
                        OutputCommand::Str(s) => json!(s.as_ref()),
                    })
                    .collect(),
            ),
        }
    }
}

// ---- The module --------------------------------------------------------------------------------------------------

enum Command {
    Configure,
    Outputs,
    Pause(bool),
    Stop,
}

/// What the module and its worker share.
struct Shared {
    settings: Mutex<Value>,
    outputs: AtomicUsize,
}

/// A plugin controller instance as the engine sees it.
pub struct PluginOutputModule {
    config: ExternalConfig,
    data: Option<PluginData>,
    host: Weak<Inner>,
    instance: Arc<Instance>,
    shared: Arc<Shared>,
    encoding: Encoding,
    worker: Option<Sender<Command>>,
    seq: u64,
    clock: Instant,
}

impl PluginOutputModule {
    fn new(host: Weak<Inner>, config: ExternalConfig) -> PluginOutputModule {
        let data = PluginData::read(&config.data);
        let controller = host.upgrade().and_then(|inner| find_type(&inner, &config.ids.type_id)).map(|t| t.controller);
        let settings = data.as_ref().and_then(|d| d.config.clone().ok()).unwrap_or_else(|| json!({}));
        let instance = Arc::new(Instance {
            id: config.ids.instance_id.to_string(),
            plugin_id: data.as_ref().map(|d| d.plugin_id.clone()).unwrap_or_default(),
            type_id: config.ids.type_id,
            shared: ModuleShared::new(),
            live: AtomicBool::new(false),
            process: Mutex::new(None),
            frames_sent: AtomicU64::new(0),
            frames_dropped: AtomicU64::new(0),
            last_frame: Mutex::new(None),
            config_errors: Mutex::new(Vec::new()),
            reported: AtomicBool::new(false),
        });
        PluginOutputModule {
            config,
            data,
            host,
            instance,
            shared: Arc::new(Shared { settings: Mutex::new(settings), outputs: AtomicUsize::new(0) }),
            encoding: controller.as_ref().map_or(Encoding::U8, Encoding::preferred),
            worker: None,
            seq: 0,
            clock: Instant::now(),
        }
    }

    fn tell(&self, command: Command) {
        if let Some(worker) = &self.worker {
            let _ = worker.send(command);
        }
    }

    /// Frames sent and dropped so far (for tests and diagnostics).
    pub fn frame_counts(&self) -> (u64, u64) {
        (self.instance.frames_sent.load(Ordering::Relaxed), self.instance.frames_dropped.load(Ordering::Relaxed))
    }
}

impl OutputModule for PluginOutputModule {
    fn type_id(&self) -> Uuid {
        self.config.ids.type_id
    }

    fn start(&mut self) {
        if self.worker.is_some() {
            return;
        }
        let Some(data) = self.data.clone() else {
            self.instance.shared.set_status(ModuleStatus::Error("The plugin controller's saved data can't be read".into()));
            return;
        };
        let Some(inner) = self.host.upgrade() else {
            self.instance.shared.set_status(ModuleStatus::Error("The plugin host isn't running".into()));
            return;
        };
        crate::host::add_instance(&inner, &self.instance);
        let problem = problem(&inner, &self.config.ids.type_id, &data);
        drop(inner);
        self.instance.shared.set_status(match problem {
            Some(problem) => ModuleStatus::Error(problem),
            None => ModuleStatus::Unavailable("Waiting for the plugin".into()),
        });
        let (tx, rx) = mpsc::channel();
        let worker = Worker {
            host: self.host.clone(),
            instance: self.instance.clone(),
            shared: self.shared.clone(),
            data,
            type_id: self.config.ids.type_id,
        };
        let spawned = thread::Builder::new().name(format!("plugin-controller-{}", self.instance.id)).spawn(move || worker.run(rx));
        match spawned {
            Ok(_) => self.worker = Some(tx),
            Err(e) => self.instance.shared.set_status(ModuleStatus::Error(format!("Couldn't start the controller's worker: {e}"))),
        }
    }

    fn stop(&mut self) {
        self.instance.live.store(false, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.send(Command::Stop);
        }
        self.instance.shared.set_status(ModuleStatus::Stopped);
    }

    fn pause(&mut self) {
        self.tell(Command::Pause(true));
    }

    fn resume(&mut self) {
        self.tell(Command::Pause(false));
    }

    fn update(&mut self, commands: &[OutputCommand]) {
        if !self.instance.live.load(Ordering::Acquire) {
            return;
        }
        let Some(process) = lock(&self.instance.process).clone() else { return };
        self.seq += 1;
        let params = json!({
            "instanceId": self.instance.id,
            "seq": self.seq,
            "timeMs": self.clock.elapsed().as_millis() as u64,
            "encoding": self.encoding.name(),
            "values": self.encoding.encode(commands),
        });
        match process.send_frame(&self.instance.id, params) {
            Ok(crate::process::FrameSent::Queued) => {
                self.instance.frames_sent.fetch_add(1, Ordering::Relaxed);
            }
            Ok(crate::process::FrameSent::Replaced) => {
                self.instance.frames_sent.fetch_add(1, Ordering::Relaxed);
                self.instance.frames_dropped.fetch_add(1, Ordering::Relaxed);
            }
            Err(_) => {
                self.instance.frames_dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
        *lock(&self.instance.last_frame) = Some(Instant::now());
    }

    fn output_count(&self) -> usize {
        self.shared.outputs.load(Ordering::SeqCst)
    }

    fn set_output_count(&mut self, count: usize) {
        if self.shared.outputs.swap(count, Ordering::SeqCst) != count {
            self.tell(Command::Outputs);
        }
    }

    fn output_limit(&self) -> usize {
        let limit = self.host.upgrade().and_then(|inner| find_type(&inner, &self.config.ids.type_id)).map(|t| t.controller.max_outputs);
        limit.map_or(crate::manifest::MAX_OUTPUTS as usize, |n| n as usize)
    }

    fn config(&self) -> ControllerConfig {
        let mut config = self.config.clone();
        if self.data.as_ref().is_some_and(|d| d.config.is_ok()) {
            set_config_text(&mut config.data, &lock(&self.shared.settings));
        }
        ControllerConfig::External(config)
    }

    fn set_config(&mut self, config: ControllerConfig) -> Result<(), ConfigMismatch> {
        let ControllerConfig::External(config) = config else { return Err(ConfigMismatch) };
        if config.ids.type_id != self.config.ids.type_id {
            return Err(ConfigMismatch);
        }
        let data = PluginData::read(&config.data);
        let settings = data.as_ref().and_then(|d| d.config.clone().ok());
        self.config = config;
        self.data = data;
        if let Some(settings) = settings {
            let changed = {
                let mut current = lock(&self.shared.settings);
                let changed = *current != settings;
                *current = settings;
                changed
            };
            if changed {
                self.tell(Command::Configure);
            }
        }
        Ok(())
    }

    fn data_policy(&self) -> DataPolicy {
        self.host.upgrade().and_then(|inner| find_type(&inner, &self.config.ids.type_id)).map_or(DataPolicy::HIGHEST_WINS_8BIT, |t| policy(&t.controller))
    }

    fn shared(&self) -> &Arc<ModuleShared> {
        &self.instance.shared
    }
}

impl Drop for PluginOutputModule {
    fn drop(&mut self) {
        self.stop();
    }
}

fn policy(controller: &ControllerType) -> DataPolicy {
    match controller.data_policy.as_str() {
        "discardExcess8Bit" => DataPolicy::DISCARD_EXCESS_8BIT,
        "average8Bit" => DataPolicy::AVERAGE_8BIT,
        "oneChannelColor" => DataPolicy::ONE_CHANNEL_COLOR,
        _ => DataPolicy::HIGHEST_WINS_8BIT,
    }
}

// ---- The worker --------------------------------------------------------------------------------------------------

struct Worker {
    host: Weak<Inner>,
    instance: Arc<Instance>,
    shared: Arc<Shared>,
    data: PluginData,
    type_id: Uuid,
}

/// How often the worker looks for the plugin, and how long it waits after a failed create or start.
const TICK: Duration = Duration::from_millis(200);
const RETRY: Duration = Duration::from_secs(5);

impl Worker {
    fn run(self, commands: Receiver<Command>) {
        let gate = gate(&self.instance.id);
        let _turn = lock(&gate);
        let mut bound: Option<Arc<PluginProcess>> = None;
        let mut paused = false;
        let mut retry_at = Instant::now();
        loop {
            match commands.recv_timeout(TICK) {
                Ok(Command::Stop) | Err(RecvTimeoutError::Disconnected) => break,
                Ok(Command::Configure) => {
                    if let Some(process) = &bound {
                        self.configure(process);
                    }
                }
                Ok(Command::Outputs) => {
                    if let Some(process) = &bound {
                        let count = self.shared.outputs.load(Ordering::SeqCst);
                        if let Err(e) = self.request(process, "controller.setOutputCount", json!({"instanceId": self.instance.id, "outputCount": count}), None) {
                            self.instance.shared.record_error(format!("controller.setOutputCount: {}", e.message));
                        }
                    }
                }
                Ok(Command::Pause(pause)) => {
                    paused = pause;
                    if let Some(process) = &bound {
                        let method = if pause { "controller.pause" } else { "controller.resume" };
                        let _ = self.request(process, method, json!({"instanceId": self.instance.id}), None);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
            let Some(inner) = self.host.upgrade() else {
                self.unbind(&mut bound, Some("The plugin host has stopped".into()));
                continue;
            };
            let process = crate::host::running_process(&inner, &self.data.plugin_id);
            let problem = problem(&inner, &self.type_id, &self.data);
            if process.is_none() && problem.is_none() && Instant::now() >= retry_at && crate::host::start_for_controller(&inner, &self.data.plugin_id) {
                retry_at = Instant::now() + TICK * 5;
            }
            drop(inner);
            match (process, problem) {
                (Some(process), None) => {
                    let current = bound.as_ref().is_some_and(|b| Arc::ptr_eq(b, &process) && !b.is_dead());
                    if !current && Instant::now() >= retry_at {
                        self.unbind(&mut bound, None);
                        match self.bind(&process, paused) {
                            Ok(()) => bound = Some(process),
                            Err(why) => {
                                self.instance.shared.set_status(ModuleStatus::Error(why.clone()));
                                self.instance.shared.record_error(why);
                                retry_at = Instant::now() + RETRY;
                            }
                        }
                    }
                }
                (None, None) => self.unbind(&mut bound, Some(format!("Waiting for plugin {} to start", self.data.plugin_name))),
                (_, Some(problem)) => self.unbind(&mut bound, Some(problem)),
            }
        }
        if let Some(process) = bound.take() {
            self.instance.live.store(false, Ordering::SeqCst);
            let id = json!({"instanceId": self.instance.id});
            let _ = self.request(&process, "controller.stop", id.clone(), None);
            let _ = self.request(&process, "controller.destroy", id, None);
        }
        *lock(&self.instance.process) = None;
        if let Some(inner) = self.host.upgrade() {
            crate::host::remove_instance(&inner, &self.instance);
        }
    }

    fn request(&self, process: &PluginProcess, method: &str, params: Value, timeout: Option<Duration>) -> Result<Value, RpcError> {
        let timeout = timeout.or_else(|| self.host.upgrade().map(|inner| crate::host::request_timeout(&inner))).unwrap_or(Duration::from_secs(5));
        process.request(method, params, timeout)
    }

    /// Creates and starts the instance in a (new) plugin process.
    fn bind(&self, process: &Arc<PluginProcess>, paused: bool) -> Result<(), String> {
        let settings = lock(&self.shared.settings).clone();
        self.instance.reported.store(false, Ordering::SeqCst);
        let params = json!({
            "instanceId": self.instance.id,
            "typeId": self.type_id.to_string(),
            "config": settings,
            "outputCount": self.shared.outputs.load(Ordering::SeqCst),
        });
        let created = self.request(process, "controller.create", params, None).map_err(|e| format!("controller.create failed: {}", e.message))?;
        if let Some(config) = created.get("config").filter(|c| c.is_object()) {
            *lock(&self.shared.settings) = config.clone();
        }
        let id = json!({"instanceId": self.instance.id});
        self.request(process, "controller.start", id.clone(), Some(START_TIMEOUT)).map_err(|e| format!("controller.start failed: {}", e.message))?;
        if paused {
            let _ = self.request(process, "controller.pause", id, None);
        }
        *lock(&self.instance.process) = Some(process.clone());
        self.instance.live.store(true, Ordering::Release);
        // The plugin may already have reported a status of its own while starting; that one stands.
        if !self.instance.reported.load(Ordering::SeqCst) {
            self.instance.shared.set_status(ModuleStatus::Running);
        }
        Ok(())
    }

    fn unbind(&self, bound: &mut Option<Arc<PluginProcess>>, status: Option<String>) {
        if bound.take().is_some() {
            self.instance.live.store(false, Ordering::SeqCst);
            *lock(&self.instance.process) = None;
        }
        if let Some(status) = status {
            if self.instance.shared.status() != ModuleStatus::Error(status.clone()) {
                self.instance.shared.set_status(ModuleStatus::Error(status));
            }
        }
    }

    fn configure(&self, process: &PluginProcess) {
        let settings = lock(&self.shared.settings).clone();
        match self.request(process, "controller.configure", json!({"instanceId": self.instance.id, "config": settings}), None) {
            Ok(result) => {
                if let Some(config) = result.get("config").filter(|c| c.is_object()) {
                    *lock(&self.shared.settings) = config.clone();
                }
                let refused = !lock(&self.instance.config_errors).is_empty();
                lock(&self.instance.config_errors).clear();
                if refused {
                    self.instance.shared.set_status(ModuleStatus::Running);
                }
            }
            Err(e) => {
                let errors = e.data.as_ref().and_then(|d| d.get("errors")).and_then(Value::as_array).cloned().unwrap_or_default();
                *lock(&self.instance.config_errors) = errors;
                let why = format!("The plugin refused the configuration: {}", e.message);
                self.instance.shared.record_error(why.clone());
                self.instance.shared.set_status(ModuleStatus::Error(why));
            }
        }
    }
}

// ---- The provider ------------------------------------------------------------------------------------------------

/// Makes one host's plugin controller types available to the engine. Register it with
/// `vixen_output::modules::studio_external::register`.
pub struct PluginControllers {
    host: Weak<Inner>,
}

impl PluginControllers {
    pub(crate) fn new(host: Weak<Inner>) -> PluginControllers {
        PluginControllers { host }
    }

    fn inner(&self) -> Option<Arc<Inner>> {
        self.host.upgrade()
    }
}

impl ExternalControllers for PluginControllers {
    fn claims(&self, type_id: &Uuid, data: Option<&XElement>) -> bool {
        if data.and_then(PluginData::read).is_some() {
            return true;
        }
        // Data some other module wrote under an id a plugin also uses stays that module's.
        data.is_none() && self.inner().is_some_and(|inner| find_type(&inner, type_id).is_some())
    }

    fn new_default(&self, type_id: &Uuid, ids: &ModuleIds) -> Option<XElement> {
        let inner = self.inner()?;
        let found = find_type(&inner, type_id)?;
        let manifest = crate::host::manifest(&inner, &found.plugin_id)?;
        let config = crate::schema::defaults(&found.controller.config_schema);
        let config = if config.is_object() { config } else { json!({}) };
        Some(write_data(ids, &manifest, &config))
    }

    fn create(&self, config: ExternalConfig) -> Box<dyn OutputModule> {
        Box::new(PluginOutputModule::new(self.host.clone(), config))
    }

    fn data_policy(&self, config: &ExternalConfig) -> DataPolicy {
        self.inner().and_then(|inner| find_type(&inner, &config.ids.type_id)).map_or(DataPolicy::HIGHEST_WINS_8BIT, |t| policy(&t.controller))
    }

    fn types(&self) -> Vec<ExternalType> {
        let Some(inner) = self.inner() else { return Vec::new() };
        crate::host::controller_types(&inner)
            .into_iter()
            .filter(|t| t.runnable)
            .filter_map(|t| {
                Some(ExternalType {
                    type_id: Uuid::parse_str(&t.controller.type_id).ok()?,
                    name: t.controller.name.clone(),
                    default_outputs: t.controller.default_outputs.min(u64::from(u32::MAX)) as u32,
                    max_outputs: t.controller.max_outputs.min(i32::MAX as u64) as u32,
                    outputs_from_config: t.controller.outputs_from_config,
                })
            })
            .collect()
    }

    fn problem(&self, config: &ExternalConfig) -> Option<String> {
        let Some(data) = PluginData::read(&config.data) else { return Some("The plugin controller's saved data can't be read".into()) };
        match self.inner() {
            Some(inner) => problem(&inner, &config.ids.type_id, &data),
            None => Some(format!("Needs plugin {} ({}) {} — the plugin host isn't running", data.plugin_name, data.plugin_id, data.version_range())),
        }
    }

    fn type_name(&self, type_id: &Uuid, data: Option<&XElement>) -> Option<String> {
        if let Some(found) = self.inner().and_then(|inner| find_type(&inner, type_id)) {
            return Some(found.controller.name);
        }
        data.and_then(PluginData::read).map(|d| format!("{} (plugin missing)", d.plugin_name))
    }

    fn describe(&self, config: &ExternalConfig) -> String {
        let data = PluginData::read(&config.data);
        let found = self.inner().and_then(|inner| find_type(&inner, &config.ids.type_id));
        json!({
            "pluginId": data.as_ref().map(|d| d.plugin_id.clone()),
            "pluginName": data.as_ref().map(|d| d.plugin_name.clone()),
            "pluginVersion": data.as_ref().and_then(|d| d.plugin_version.clone()),
            "versionRange": data.as_ref().map(PluginData::version_range),
            "installedVersion": found.as_ref().map(|t| t.version.to_string()),
            "pluginState": found.as_ref().map(|t| t.state.as_str()),
            "config": data.as_ref().and_then(|d| d.config.clone().ok()),
            "configSchema": found.as_ref().map(|t| t.controller.config_schema.clone()),
            "dataPolicy": found.as_ref().map(|t| t.controller.data_policy.clone()),
            "frameEncoding": found.as_ref().map(|t| Encoding::preferred(&t.controller).name()),
            "maxOutputs": found.as_ref().map(|t| t.controller.max_outputs),
            // `outputCount: "fromConfig"`: the plugin sets the output count from the settings; the inspector shows it read-only.
            "outputsFromConfig": found.as_ref().is_some_and(|t| t.controller.outputs_from_config),
            "actions": found.as_ref().map(|t| t.controller.actions.iter().map(|a| json!({"id": a.id, "title": a.title, "description": a.description})).collect::<Vec<_>>()).unwrap_or_default(),
            "problem": self.problem(config),
            // The plugin's own field errors from its last refused `controller.configure`, for the inspector.
            "configErrors": self.inner().and_then(|inner| crate::host::instance(&inner, &config.ids.instance_id.to_string())).map(|i| lock(&i.config_errors).clone()).unwrap_or_default(),
        })
        .to_string()
    }

    fn reconfigure(&self, config: &ExternalConfig, settings: &str) -> Result<XElement, String> {
        let errors = |list: Vec<crate::schema::FieldError>| Value::Array(list.iter().map(crate::schema::FieldError::to_json).collect()).to_string();
        let one = |message: &str| errors(vec![crate::schema::FieldError { field: "/".into(), message: message.into() }]);
        let settings: Value = serde_json::from_str(settings).map_err(|_| one("must be a JSON object"))?;
        if !settings.is_object() {
            return Err(one("must be a JSON object"));
        }
        let Some(found) = self.inner().and_then(|inner| find_type(&inner, &config.ids.type_id)) else {
            let name = PluginData::read(&config.data).map_or_else(|| "its plugin".to_string(), |d| format!("{} ({})", d.plugin_name, d.plugin_id));
            return Err(one(&format!("can't be checked: {name} isn't installed")));
        };
        let problems = crate::schema::validate(&found.controller.config_schema, &settings);
        if !problems.is_empty() {
            return Err(errors(problems));
        }
        let mut data = config.to_xml();
        set_config_text(&mut data, &settings);
        Ok(data)
    }

    fn outputs_for(&self, config: &ExternalConfig, settings: &str) -> Option<Result<ExternalOutputs, String>> {
        let inner = self.inner()?;
        let found = find_type(&inner, &config.ids.type_id).filter(|t| t.controller.outputs_from_config)?;
        Some(outputs_for(&inner, &found, settings))
    }

    fn run_action(&self, config: &ExternalConfig, settings: &str, action: &str) -> Result<String, String> {
        let inner = self.inner().ok_or("the plugin host isn't running")?;
        let found = find_type(&inner, &config.ids.type_id).ok_or("its plugin isn't installed")?;
        let wanted = found.controller.actions.iter().find(|a| a.id == action).ok_or_else(|| format!("{} has no action {action}", found.controller.name))?;
        let settings: Value = serde_json::from_str(settings).map_err(|_| "the settings aren't a JSON object".to_string())?;
        let arguments = json!({"instanceId": config.ids.instance_id.to_string(), "typeId": config.ids.type_id.to_string(), "config": settings});
        let answer = crate::host::call_tool_raw(&inner, &found.plugin_id, &wanted.tool, arguments)?;
        action_result(&found.plugin_name, &wanted.title, &settings, &answer).map(|v| v.to_string())
    }

    fn system_saved(&self, profile: &Path, configs: &[ExternalConfig]) {
        let mut needs: HashMap<String, String> = HashMap::new();
        for data in configs.iter().filter_map(|c| PluginData::read(&c.data)) {
            needs.entry(data.plugin_id.clone()).or_insert_with(|| data.version_range());
        }
        let needs: Vec<crate::profile::Requirement> = needs.into_iter().map(|(id, version_range)| crate::profile::Requirement { id, version_range }).collect();
        if let Err(why) = crate::profile::replace(profile, &needs) {
            eprintln!("studio-plugins: couldn't write {}: {why}", crate::profile::FILE);
        }
    }
}

// ---- Output counts and settings actions -------------------------------------------------------------------------

/// Longest output name taken from a plugin.
const MAX_OUTPUT_NAME: usize = 80;

/// `controller.outputsFor {typeId, config}` → `{outputCount, outputNames?, lights?}`, checked against the type's limit.
/// Names are used only when there is one per output; lights only when [`parse_lights`] takes them.
fn outputs_for(inner: &Arc<Inner>, found: &TypeInfo, settings: &str) -> Result<ExternalOutputs, String> {
    let settings: Value = serde_json::from_str(settings).map_err(|_| "the settings aren't a JSON object".to_string())?;
    let process = crate::host::ensure_running(inner, &found.plugin_id).map_err(|why| format!("{} can't work out the outputs: {why}", found.plugin_name))?;
    let params = json!({"typeId": found.controller.type_id, "config": settings});
    let answer = process.request("controller.outputsFor", params, crate::host::request_timeout(inner)).map_err(|e| {
        if e.code == rpc::METHOD_NOT_FOUND {
            format!("{} doesn't answer controller.outputsFor, which its outputCount \"fromConfig\" needs", found.plugin_name)
        } else {
            format!("{} couldn't work out the outputs: {}", found.plugin_name, e.message)
        }
    })?;
    parse_outputs(&found.plugin_name, found.controller.max_outputs, &answer)
}

fn parse_outputs(plugin: &str, max: u64, answer: &Value) -> Result<ExternalOutputs, String> {
    let count = answer.get("outputCount").and_then(Value::as_u64).ok_or_else(|| format!("{plugin} answered controller.outputsFor without an outputCount"))?;
    if count > max.min(u64::from(u32::MAX)) {
        return Err(format!("these settings need {count} outputs; {plugin} allows at most {max}"));
    }
    let names = answer.get("outputNames").and_then(Value::as_array).filter(|names| names.len() as u64 == count).and_then(|names| {
        names.iter().map(|n| n.as_str().map(|t| t.chars().take(MAX_OUTPUT_NAME).collect::<String>())).collect::<Option<Vec<String>>>()
    });
    let lights = answer.get("lights").and_then(|l| parse_lights(l, count));
    Ok(ExternalOutputs { count: count as u32, names, lights })
}

/// The most lights one answer lists.
const MAX_LIGHTS: usize = 1000;

/// `lights` (B62): `[{name, outputCount}]` in output order, each on the next `outputCount` outputs (1, 3 or 4: a single
/// colour, RGB or RGBW bulb). `None` unless every entry is a named light of 1, 3 or 4 outputs and together they fit in
/// `count`, so Studio never patches a bulb to outputs that aren't there.
pub fn parse_lights(lights: &Value, count: u64) -> Option<Vec<ExternalLight>> {
    let list = lights.as_array().filter(|l| l.len() <= MAX_LIGHTS)?;
    let lights = list
        .iter()
        .map(|l| {
            let name = l.get("name")?.as_str()?.trim().chars().take(MAX_OUTPUT_NAME).collect::<String>();
            let outputs = l.get("outputCount")?.as_u64().filter(|n| matches!(n, 1 | 3 | 4))?;
            (!name.is_empty()).then_some(ExternalLight { name, outputs: outputs as u32 })
        })
        .collect::<Option<Vec<_>>>()?;
    (lights.iter().map(|l| u64::from(l.outputs)).sum::<u64>() <= count).then_some(lights)
}

/// RFC 7396: objects merge key by key, `null` removes a key, anything else replaces.
pub fn merge_patch(target: &Value, patch: &Value) -> Value {
    let Some(patch) = patch.as_object() else { return patch.clone() };
    let mut out = target.as_object().cloned().unwrap_or_default();
    for (key, value) in patch {
        if value.is_null() {
            out.remove(key);
        } else {
            let merged = merge_patch(out.get(key).unwrap_or(&Value::Null), value);
            out.insert(key.clone(), merged);
        }
    }
    Value::Object(out)
}

/// A settings action's tool answer as `{config, message}`: `structuredContent` (or the first text item, when it is a
/// JSON object) carries `config` (the whole new configuration) or `mergePatch` (RFC 7396 against the current one), and
/// optionally `message`. A plain text answer is only a message.
pub fn action_result(plugin: &str, title: &str, current: &Value, answer: &Value) -> Result<Value, String> {
    let content = answer.get("content").and_then(Value::as_array).cloned().unwrap_or_default();
    let text = content.iter().find(|c| c["type"] == "text").and_then(|c| c["text"].as_str()).unwrap_or_default().to_string();
    if answer.get("isError").and_then(Value::as_bool).unwrap_or(false) {
        return Err(if text.is_empty() { format!("{title} failed in {plugin}") } else { text });
    }
    let payload = answer
        .get("structuredContent")
        .filter(|v| v.is_object())
        .cloned()
        .or_else(|| serde_json::from_str::<Value>(&text).ok().filter(Value::is_object));
    let Some(payload) = payload else {
        return Ok(json!({"config": null, "message": (!text.is_empty()).then_some(text)}));
    };
    let config = match (payload.get("config"), payload.get("mergePatch")) {
        (Some(config), _) if config.is_object() => Some(config.clone()),
        (_, Some(patch)) if patch.is_object() => Some(merge_patch(current, patch)),
        (None, None) => None,
        _ => return Err(format!("{plugin} answered {title} with a config or mergePatch that isn't an object")),
    };
    let message = payload.get("message").and_then(Value::as_str).map(|m| m.chars().take(2000).collect::<String>());
    Ok(json!({"config": config, "message": message}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use vixen_core::Color;

    #[test]
    fn frames_encode_each_command_kind() {
        let commands = [OutputCommand::U8(255), OutputCommand::None, OutputCommand::U16(0x1234), OutputCommand::Color(Color { a: 255, r: 1, g: 2, b: 3 })];
        assert_eq!(Encoding::U8.encode(&commands), json!(vixen_xml::nbfx::base64_encode(&[255, 0, 0, 0])));
        assert_eq!(Encoding::U16.encode(&commands), json!(vixen_xml::nbfx::base64_encode(&[0xff, 0xff, 0, 0, 0x34, 0x12, 0, 0])));
        assert_eq!(Encoding::Json.encode(&commands), json!([255, null, 4660, {"r": 1, "g": 2, "b": 3}]));
    }

    #[test]
    fn outputs_answers_are_checked_and_names_need_one_per_output() {
        let three = parse_outputs("Lamp", 100, &json!({"outputCount": 3, "outputNames": ["A R", "A G", "A B"]})).unwrap();
        assert_eq!(three, ExternalOutputs { count: 3, names: Some(vec!["A R".into(), "A G".into(), "A B".into()]), lights: None });
        assert_eq!(parse_outputs("Lamp", 100, &json!({"outputCount": 3, "outputNames": ["A"]})).unwrap().names, None);
        assert!(parse_outputs("Lamp", 100, &json!({"outputCount": 101})).unwrap_err().contains("at most 100"));
        assert!(parse_outputs("Lamp", 100, &json!({})).is_err());
    }

    #[test]
    fn lights_are_taken_only_when_every_one_is_named_and_they_fit() {
        let answer = json!({"outputCount": 7, "lights": [{"name": "Ceiling", "outputCount": 3}, {"name": " Lamp ", "outputCount": 4}]});
        let lights = parse_outputs("Lamp", 100, &answer).unwrap().lights.unwrap();
        assert_eq!(lights, [ExternalLight { name: "Ceiling".into(), outputs: 3 }, ExternalLight { name: "Lamp".into(), outputs: 4 }]);
        // Too many outputs for the count, an unnamed light or a light of 2 outputs: no lights, the count still stands.
        for bad in [json!([{"name": "A", "outputCount": 3}, {"name": "B", "outputCount": 3}]), json!([{"name": " ", "outputCount": 3}]), json!([{"name": "A", "outputCount": 2}]), json!("A")] {
            let outputs = parse_outputs("Lamp", 100, &json!({"outputCount": 5, "lights": bad})).unwrap();
            assert_eq!((outputs.count, outputs.lights), (5, None));
        }
    }

    #[test]
    fn action_answers_give_a_whole_config_a_merge_patch_or_only_a_message() {
        let current = json!({"devices": [{"id": "a"}], "maxFps": 20});
        let whole = json!({"content": [{"type": "text", "text": "ok"}], "structuredContent": {"config": {"devices": []}, "message": "None found"}});
        assert_eq!(action_result("P", "Detect", &current, &whole).unwrap(), json!({"config": {"devices": []}, "message": "None found"}));
        let patch = json!({"content": [{"type": "text", "text": "{\"mergePatch\": {\"devices\": [{\"id\": \"b\"}], \"maxFps\": null}}"}]});
        assert_eq!(action_result("P", "Detect", &current, &patch).unwrap()["config"], json!({"devices": [{"id": "b"}]}));
        let text = json!({"content": [{"type": "text", "text": "Nothing to do"}]});
        assert_eq!(action_result("P", "Detect", &current, &text).unwrap(), json!({"config": null, "message": "Nothing to do"}));
        let failed = json!({"content": [{"type": "text", "text": "no network"}], "isError": true});
        assert_eq!(action_result("P", "Detect", &current, &failed).unwrap_err(), "no network");
        assert!(action_result("P", "Detect", &current, &json!({"content": [], "structuredContent": {"config": 3}})).is_err());
    }

    #[test]
    fn module_data_reads_and_keeps_unknown_members() {
        let mut data = XElement::parse(
            r#"<PluginControllerData><ModuleInstanceId>x</ModuleInstanceId><PluginId>com.example.lamp</PluginId><PluginName>Lamp</PluginName><PluginVersion>1.2.3</PluginVersion><Config>{"host":"10.0.0.2"}</Config><Future>kept</Future></PluginControllerData>"#,
        )
        .unwrap();
        let read = PluginData::read(&data).unwrap();
        assert_eq!(read.plugin_id, "com.example.lamp");
        assert_eq!(read.config, Ok(json!({"host": "10.0.0.2"})));
        assert_eq!(read.version_range(), "^1.2");
        set_config_text(&mut data, &json!({"host": "10.0.0.3"}));
        assert_eq!(data.child_text("Future").as_deref(), Some("kept"));
        assert_eq!(PluginData::read(&data).unwrap().config, Ok(json!({"host": "10.0.0.3"})));
        let zero = PluginData { plugin_version: Some("0.3.1".into()), ..read };
        assert_eq!(zero.version_range(), "^0.3.1");
        assert!(PluginData::read(&XElement::parse("<OtherData><PluginId>com.example.lamp</PluginId></OtherData>").unwrap()).is_none());
    }
}
