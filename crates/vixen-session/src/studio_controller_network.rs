//! The controller inspector's network settings (Display ▸ Controllers), shared by the Display pane and the MCP
//! `controller_configure` / `controller_network` tools: protocol, a unicast IPv4 address, universes generated from the
//! output count, WLED presets, and a status line (packets sent, an optional reachability check).
use crate::app::AppCore;
use crate::setup::controllers::{output_limit, ControllerEntry};
use crate::setup::SetupSession;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use uuid::Uuid;
use vixen_output::modules::e131::UniverseEntry;
use vixen_output::modules::studio_external;
use vixen_output::{ids, ControllerConfig, ModuleStatus};

pub const MAX_UNIVERSE: u64 = 63999;
pub const DEFAULT_UNIVERSE_SIZE: u64 = 510;

/// WLED's E1.31 "DMX mode" presets: how many channels it reads from each universe.
pub fn presets() -> Value {
    json!([
        {"id":"wledRgb","name":"WLED: Multiple RGB","universeSize":510,"channelsPerPixel":3},
        {"id":"wledRgbw","name":"WLED: Multiple RGBW","universeSize":512,"channelsPerPixel":4},
    ])
}

fn protocol_of(c: &ControllerEntry) -> Option<&'static str> {
    match c.type_id {
        ids::E131 => Some("e131"),
        ids::DDP => Some("ddp"),
        _ => None,
    }
}

/// The universes `configure` would generate, for comparing with what a controller has.
fn generate(outputs: usize, first: u64, size: u64, address: &str) -> Result<Vec<UniverseEntry>, String> {
    let mut universes = Vec::new();
    let mut offset = 0usize;
    while offset < outputs {
        let universe = first + universes.len() as u64;
        if universe > MAX_UNIVERSE {
            return Err(format!("{outputs} outputs at {size} channels per universe need universes past {MAX_UNIVERSE}"));
        }
        let count = (outputs - offset).min(size as usize);
        let mut entry = UniverseEntry::new(true, universe as i32, offset as i32, count as i32);
        entry.unicast = Some(address.to_string());
        universes.push(entry);
        offset += count;
    }
    Ok(universes)
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{} {}", group_digits(n), if n == 1 { one } else { many })
}

fn group_digits(n: usize) -> String {
    let text = n.to_string();
    let mut out = String::new();
    for (i, ch) in text.chars().enumerate() {
        if i > 0 && (text.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// The channels per universe last chosen for each controller, for a controller whose outputs fit in one universe: its
/// layout can't record the choice, and the inspector shouldn't snap back to 510 after 512 was picked.
static CHOSEN_SIZE: Mutex<Option<HashMap<Uuid, u64>>> = Mutex::new(None);

fn chosen_size(id: Uuid) -> Option<u64> {
    CHOSEN_SIZE.lock().as_ref().and_then(|m| m.get(&id).copied())
}

fn remember_size(id: Uuid, size: u64) {
    CHOSEN_SIZE.lock().get_or_insert_with(HashMap::new).insert(id, size);
}

/// What the inspector shows for one controller.
pub fn info(c: &ControllerEntry) -> Value {
    let outputs = c.outputs.len();
    let pixels = json!({"rgb": outputs / 3, "rgbw": outputs / 4});
    let base = json!({"id":c.id.to_string(),"name":c.name,"typeName":c.type_name(),"outputs":outputs,"outputLimit":output_limit(&c.type_id),"pixels":pixels});
    let mut v = base;
    match &c.config {
        Some(ControllerConfig::E131(config)) => {
            let universes = config.universes.as_deref().unwrap_or(&[]);
            let address = config
                .unicast
                .clone()
                .or_else(|| universes.iter().find_map(|u| u.unicast.clone()))
                .filter(|a| !a.is_empty());
            let first = universes.first().map(|u| u.universe.max(1) as u64);
            // Every universe but the last holds the chosen size. One universe doesn't show it, so the size chosen this
            // run is used if it fits; otherwise a short universe reads as the default 510 and one past 510 as 512.
            let size = match universes {
                [] => None,
                [only] => Some(
                    chosen_size(c.id)
                        .filter(|size| only.size as u64 <= *size)
                        .unwrap_or(if only.size > DEFAULT_UNIVERSE_SIZE as i32 { 512 } else { DEFAULT_UNIVERSE_SIZE }),
                ),
                [first, ..] => Some(first.size.max(1) as u64),
            };
            let generated = match (&address, first, size) {
                (Some(address), Some(first), Some(size)) if (1..=512).contains(&size) => generate(outputs, first, size, address)
                    .is_ok_and(|g| g.len() == universes.len() && g.iter().zip(universes).all(|(a, b)| a.universe == b.universe && a.start == b.start && a.size == b.size && b.active)),
                _ => false,
            };
            let destination = if address.is_some() {
                "unicast"
            } else if config.multicast.as_deref().is_some_and(|m| !m.is_empty()) {
                "multicast"
            } else {
                "none"
            };
            let layout = if address.is_none() {
                "Enter the address; universes are generated from the outputs".to_string()
            } else if universes.is_empty() {
                "No universes yet".to_string()
            } else if generated {
                let (first, size) = (first.unwrap_or(1), size.unwrap_or(DEFAULT_UNIVERSE_SIZE));
                let last = first + universes.len() as u64 - 1;
                let span = if universes.len() == 1 { format!("universe {first}") } else { format!("universes {first}–{last}") };
                format!("{span}, {} channels each", size)
            } else {
                format!("{} set by hand (Advanced…)", plural(universes.len(), "universe", "universes"))
            };
            v["protocol"] = json!("e131");
            v["address"] = json!(address);
            v["destination"] = json!(destination);
            v["firstUniverse"] = json!(first);
            v["universeSize"] = json!(size);
            v["universeCount"] = json!(universes.len());
            v["generated"] = json!(generated);
            v["layout"] = json!(layout);
        }
        Some(ControllerConfig::Ddp(config)) => {
            v["protocol"] = json!("ddp");
            v["address"] = json!(config.address);
            v["destination"] = json!(if config.address.is_some() { "unicast" } else { "none" });
            v["layout"] = json!(format!("{} in one DDP stream", plural(outputs, "channel", "channels")));
        }
        Some(ControllerConfig::External(config)) => {
            v["protocol"] = Value::Null;
            v["plugin"] = studio_external::describe(config).and_then(|d| serde_json::from_str(&d).ok()).unwrap_or(Value::Null);
        }
        _ => {
            v["protocol"] = Value::Null;
        }
    }
    v
}

/// How the refusal starts when new settings would drop outputs that are patched; the inspector looks for it to offer
/// Remove Outputs, which repeats the edit with `removePatchedOutputs: true`.
pub const REMOVES_PATCHED: &str = "These settings remove ";

/// Output numbers as compact ranges: `13–15, 20`.
fn ranges(numbers: &[usize]) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut i = 0;
    while i < numbers.len() {
        let mut j = i;
        while j + 1 < numbers.len() && numbers[j + 1] == numbers[j] + 1 {
            j += 1;
        }
        parts.push(if i == j { numbers[i].to_string() } else { format!("{}–{}", numbers[i], numbers[j]) });
        i = j + 1;
    }
    parts.join(", ")
}

/// `controller_configure` for a plugin controller: its configuration (checked against the plugin's `configSchema`) and
/// output count. The running plugin gets the new configuration through `controller.configure` once the edit is applied.
/// A type with `outputCount: "fromConfig"` sets its own count: the plugin answers `controller.outputsFor` for the new
/// settings and the controller is resized (and its outputs named) in the same edit, so one Undo step covers both.
/// Outputs past the new count that are patched are only dropped with `removePatchedOutputs: true`.
fn configure_plugin(s: &mut SetupSession, slot: usize, a: &Value) -> Result<Value, String> {
    if let Some(key) = a.as_object().into_iter().flat_map(|o| o.keys()).find(|k| !matches!(k.as_str(), "id" | "config" | "outputs" | "removePatchedOutputs")) {
        return Err(format!("{key} doesn't apply to a plugin controller; give config (an object matching the plugin's configSchema) and/or outputs"));
    }
    let type_id = s.controllers[slot].type_id;
    let owned = studio_external::outputs_from_config(&type_id);
    if owned && a.get("outputs").is_some_and(|v| !v.is_null()) {
        return Err(format!("{} sets its output count from its settings; change config instead", s.controllers[slot].type_name()));
    }
    let outputs = number(a, "outputs", 1..=u64::from(output_limit(&type_id)))?;
    let Some(ControllerConfig::External(current)) = s.controllers[slot].config.clone() else { unreachable!("checked by the caller") };
    let config_text = match a.get("config") {
        None | Some(Value::Null) => None,
        Some(config @ Value::Object(_)) => Some(config.to_string()),
        Some(_) => return Err("config must be an object".into()),
    };
    let data = match &config_text {
        None => None,
        Some(text) => Some(studio_external::reconfigure(&current, text).map_err(|errors| {
            let list: Vec<String> = serde_json::from_str::<Value>(&errors)
                .ok()
                .and_then(|v| v.as_array().cloned())
                .unwrap_or_default()
                .iter()
                .map(|e| format!("{}: {}", e["field"].as_str().unwrap_or("/"), e["message"].as_str().unwrap_or_default()))
                .collect();
            format!("Invalid configuration: {}", list.join("; "))
        })?),
    };
    // Ask the plugin before changing anything. A plugin that can't answer (not running, an error) leaves the count as it
    // is; the settings still apply, and the warning says why the outputs didn't follow.
    let mut warning = None;
    let plan = match (&config_text, owned) {
        (Some(text), true) => match studio_external::outputs_for(&current, text) {
            Some(Ok(plan)) => Some(plan),
            Some(Err(why)) => {
                warning = Some(format!("The output count wasn't changed: {why}"));
                None
            }
            None => None,
        },
        _ => None,
    };
    if let Some(plan) = &plan {
        let c = &s.controllers[slot];
        let dropped: Vec<usize> = c.outputs.iter().enumerate().skip(plan.count as usize).filter(|(_, o)| s.flow.source(o.cid).is_some()).map(|(i, _)| i + 1).collect();
        if !dropped.is_empty() && a.get("removePatchedOutputs") != Some(&Value::Bool(true)) {
            let what = if dropped.len() == 1 { "output" } else { "outputs" };
            return Err(format!(
                "{REMOVES_PATCHED}{} patched {what} ({what} {} of {}): they need {} outputs. Their patches go with them; pass removePatchedOutputs: true to apply anyway (Undo brings them back).",
                dropped.len(),
                ranges(&dropped),
                c.outputs.len(),
                plan.count
            ));
        }
    }
    let id = s.controllers[slot].id;
    if let Some(n) = outputs {
        s.set_output_count(id, n as u32);
    }
    if let Some(plan) = &plan {
        s.set_output_count(id, plan.count);
        if let Some(names) = &plan.names {
            for (output, name) in s.controllers[slot].outputs.iter_mut().zip(names) {
                output.name = name.clone();
            }
        }
    }
    let c = &mut s.controllers[slot];
    if let Some(data) = data {
        c.config = Some(ControllerConfig::External(studio_external::ExternalConfig { ids: current.ids, data }));
    }
    c.config_changed = true;
    s.controllers_changed();
    let mut result = json!({"configured":true,"outputStarted":false,"controller":info(&s.controllers[slot])});
    if let Some(warning) = warning {
        result["warning"] = json!(warning);
    }
    Ok(result)
}

fn number(a: &Value, key: &str, range: std::ops::RangeInclusive<u64>) -> Result<Option<u64>, String> {
    match a.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .filter(|n| range.contains(n))
            .map(Some)
            .ok_or(format!("{key} must be an integer {}..{}", range.start(), range.end())),
    }
}

/// Replaces a controller with one of another protocol. It keeps its name, place, run state and outputs, so every
/// patch survives; it gets a new id, because a running controller's module can't change type. Returns the new slot.
fn switch_protocol(s: &mut SetupSession, slot: usize, type_id: Uuid) -> u32 {
    let old = &mut s.controllers[slot];
    old.alive = false;
    let outputs = std::mem::take(&mut old.outputs);
    let instance_id = Uuid::new_v4();
    let entry = ControllerEntry {
        id: Uuid::new_v4(),
        name: old.name.clone(),
        type_id,
        instance_id,
        outputs,
        alive: true,
        running: old.running,
        running_at_open: old.running_at_open,
        is_new: true,
        config: ControllerConfig::new_default(&type_id, instance_id),
        config_changed: true,
    };
    if !old.is_new {
        let removed = (old.type_id, old.instance_id);
        s.removed_controller_data.push(removed);
    }
    let new_slot = s.controllers.len() as u32;
    s.controllers.push(entry);
    for place in s.controller_order.iter_mut().filter(|p| **p as usize == slot) {
        *place = new_slot;
    }
    new_slot
}

/// `controller_configure`: protocol, address, output count and generated universes in one step. Fields left out keep
/// their current values; an E1.31 controller's universes are regenerated from the output count every time.
pub fn configure(s: &mut SetupSession, a: &Value) -> Result<Value, String> {
    let id = a["id"].as_str().and_then(|t| Uuid::parse_str(t).ok()).ok_or("id must be a controller UUID")?;
    let mut slot = s.slot_of(id).ok_or("Controller not found")? as usize;
    if matches!(s.controllers[slot].config, Some(ControllerConfig::External(_))) {
        return configure_plugin(s, slot, a);
    }
    let preset = match a.get("preset").and_then(Value::as_str) {
        None => None,
        Some(name) => Some(
            presets()
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["id"] == name)
                .cloned()
                .ok_or("preset must be wledRgb or wledRgbw")?,
        ),
    };
    let protocol = match a.get("protocol").and_then(Value::as_str) {
        None => preset.as_ref().map(|_| "e131"),
        Some(p @ ("e131" | "ddp")) => Some(p),
        Some(_) => return Err("protocol must be e131 or ddp".into()),
    };
    if preset.is_some() && protocol == Some("ddp") {
        return Err("WLED presets set E1.31 universe sizes; DDP has no universes".into());
    }
    let current = protocol_of(&s.controllers[slot]).ok_or("Network configuration supports DDP and E1.31 only")?;
    let address_arg = match a.get("address").and_then(Value::as_str) {
        None => None,
        Some(text) => Some(
            text.trim()
                .parse::<std::net::Ipv4Addr>()
                .map_err(|_| "address must be an IPv4 address such as 192.168.1.50; host names are not looked up")?
                .to_string(),
        ),
    };
    let previous = info(&s.controllers[slot]);
    let outputs = number(a, "outputs", 1..=u64::from(output_limit(&s.controllers[slot].type_id)))?;
    let first = number(a, "firstUniverse", 1..=MAX_UNIVERSE)?;
    let size = match (number(a, "universeSize", 1..=512)?, &preset) {
        (Some(_), Some(_)) => return Err("Give either preset or universeSize, not both".into()),
        (size, Some(p)) => size.or(p["universeSize"].as_u64()),
        (size, None) => size,
    };
    // Check everything before changing anything, so a refused call leaves the controller as it was.
    let target = protocol.unwrap_or(current);
    let address = address_arg
        .or_else(|| previous["address"].as_str().map(str::to_string))
        .ok_or("address is required: the controller's IPv4 address, such as 192.168.1.50")?;
    let output_count = outputs.map_or(s.controllers[slot].outputs.len(), |n| n as usize);
    let first = first.or(previous["firstUniverse"].as_u64()).unwrap_or(1);
    let size = size.or(previous["universeSize"].as_u64().filter(|_| previous["generated"] == true)).unwrap_or(DEFAULT_UNIVERSE_SIZE);
    let universes = if target == "e131" {
        let universes = generate(output_count, first, size, &address)?;
        let check = vixen_output::modules::e131::E131Config { universes: Some(universes.clone()), ..Default::default() };
        let errors = check.validate(output_count);
        if !errors.is_empty() {
            return Err(errors.join("; "));
        }
        Some(universes)
    } else {
        None
    };

    let size_given = a.get("universeSize").is_some_and(|v| !v.is_null()) || preset.is_some();
    let mut switched = None;
    if target != current {
        let type_id = if target == "e131" { ids::E131 } else { ids::DDP };
        if output_count as u64 > u64::from(output_limit(&type_id)) {
            return Err(format!("{} supports at most {} outputs", target.to_uppercase(), output_limit(&type_id)));
        }
        slot = switch_protocol(s, slot, type_id) as usize;
        switched = Some(s.controllers[slot].id);
    }
    let controller_id = s.controllers[slot].id;
    if target == "e131" && (size_given || switched.is_some()) {
        remember_size(controller_id, size);
    }
    if let Some(n) = outputs {
        s.set_output_count(controller_id, n as u32);
    }
    let c = &mut s.controllers[slot];
    match c.config.as_mut().ok_or("No controller configuration")? {
        ControllerConfig::Ddp(config) => {
            config.address = Some(address);
            config.host_name = None;
        }
        ControllerConfig::E131(config) => {
            config.unicast = Some(address);
            config.multicast = None;
            config.universes = universes;
            config.output_count = c.outputs.len() as i32;
            config.auto_populate = false;
        }
        _ => unreachable!("checked above"),
    }
    c.config_changed = true;
    s.controllers_changed();
    let mut result = json!({"configured":true,"outputStarted":false,"controller":info(&s.controllers[slot])});
    if let Some(new_id) = switched {
        result["previousId"] = json!(id.to_string());
        result["newId"] = json!(new_id.to_string());
        result["newSlot"] = json!(slot);
        result["note"] = json!("Changing protocol replaces the controller: same name, outputs and patches, new id.");
    }
    Ok(result)
}

/// When each controller's packet count last moved, as seen by `status` calls. The engine counts packets but keeps no
/// clock, so "last packet" is as fresh as the caller's polling (the inspector polls once a second).
static LAST_SEEN: Mutex<Option<HashMap<Uuid, (u64, Instant)>>> = Mutex::new(None);

/// When each controller's send-error count last moved, the same way: the engine keeps the last error but not its time.
static LAST_ERROR: Mutex<Option<HashMap<Uuid, (u64, Instant)>>> = Mutex::new(None);

/// Send errors older than this, on a controller that is sending again, are a warning rather than an error.
const RECENT_ERROR: Duration = Duration::from_secs(60);

/// The status line: whether output is running and how long ago it last sent a packet, and any send errors with how
/// long ago the last one was (`problem`).
pub fn status(core: &AppCore, id: Uuid) -> Value {
    let muted = !core.playback.hardware_allowed();
    let open = core.execution().is_open();
    let found = core.playback.with_output(|_, runner| {
        runner.controller_index(&id).map(|i| {
            let shared = runner.shared(i);
            (
                runner.is_running(i),
                shared.map_or(0, |s| s.packets_sent()),
                shared.map_or(0, |s| s.error_count()),
                shared.and_then(|s| s.last_error()),
                runner.status(i),
            )
        })
    });
    let (running, packets, errors, last_error, module_status) = found.unwrap_or((false, 0, 0, None, ModuleStatus::Stopped));
    let now = Instant::now();
    let ago = {
        let mut guard = LAST_SEEN.lock();
        let seen = guard.get_or_insert_with(HashMap::new);
        match seen.get(&id) {
            Some((count, _)) if packets > *count => {
                seen.insert(id, (packets, now));
                Some(Duration::ZERO)
            }
            Some((count, at)) if packets == *count && packets > 0 => Some(now - *at),
            _ => {
                // First look, or the counter restarted with the controller.
                seen.insert(id, (packets, now));
                None
            }
        }
    };
    let text = if muted {
        "Muted: nothing is sent to controllers".to_string()
    } else if !open {
        "Live output is off".to_string()
    } else if !running {
        "Stopped: not sending".to_string()
    } else {
        match ago {
            Some(d) if d < Duration::from_secs(2) => format!("Sending: {} packets so far", group_digits(packets as usize)),
            Some(d) => format!("Last packet {} s ago", d.as_secs()),
            None if packets > 0 => format!("{} packets sent", group_digits(packets as usize)),
            None => "Running, no packets sent yet".to_string(),
        }
    };
    let error_ago = {
        let mut guard = LAST_ERROR.lock();
        let seen = guard.get_or_insert_with(HashMap::new);
        match seen.get(&id) {
            _ if errors == 0 => {
                seen.remove(&id);
                None
            }
            Some((count, at)) if errors == *count => Some(now - *at),
            // New errors, a first look, or the counter restarted with the controller.
            _ => {
                seen.insert(id, (errors, now));
                Some(Duration::ZERO)
            }
        }
    };
    let sending = ago.is_some_and(|d| d < Duration::from_secs(2));
    let problem = match (&last_error, error_ago) {
        (Some(error), Some(age)) => {
            // Old errors stay visible, but only as a warning unless packets stopped. A plugin that reports no packet
            // counts can't show that, so for it only the age counts.
            let stalled = open && !muted && running && packets > 0 && !sending;
            let level = if age < RECENT_ERROR || stalled { "error" } else { "warning" };
            json!({"level":level,"text":format!("Send errors ({}), last {}: {error}", group_digits(errors as usize), age_text(age))})
        }
        _ => Value::Null,
    };
    let (module_state, module_problem) = match module_status {
        ModuleStatus::Unavailable(m) => (Some("unavailable"), Some(m)),
        ModuleStatus::Error(m) | ModuleStatus::UnsupportedOnMacOs(m) => (Some("error"), Some(m)),
        _ => (None, None),
    };
    json!({"running":running,"liveOutput":open && !muted,"muted":muted,"packetsSent":packets,"lastPacketMsAgo":ago.map(|d|d.as_millis() as u64),"errors":errors,"lastError":last_error,"lastErrorMsAgo":error_ago.map(|d|d.as_millis() as u64),"problem":problem,"moduleState":module_state,"moduleProblem":module_problem,"text":text})
}

/// "just now", "40 s ago", "3 min ago", "2 h ago".
fn age_text(age: Duration) -> String {
    match age.as_secs() {
        s if s < 5 => "just now".into(),
        s if s < 60 => format!("{s} s ago"),
        s if s < 3600 => format!("{} min ago", s / 60),
        s => format!("{} h ago", s / 3600),
    }
}

/// Whether anything answers at `address`: a TCP connection to port 80, where WLED and most pixel controllers serve
/// their web page. A refusal also counts, since only a running host refuses. Sends no lighting data.
pub fn probe(address: &str) -> Value {
    let Ok(ip) = address.trim().parse::<std::net::Ipv4Addr>() else {
        return json!({"reachable":false,"text":"Not an IPv4 address"});
    };
    let started = Instant::now();
    let result = std::net::TcpStream::connect_timeout(&(ip, 80).into(), Duration::from_millis(800));
    let ms = started.elapsed().as_millis() as u64;
    match result {
        Ok(_) => json!({"reachable":true,"ms":ms,"text":format!("{ip} answered in {ms} ms (web page on port 80)")}),
        Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
            json!({"reachable":true,"ms":ms,"text":format!("{ip} answered in {ms} ms (no web page)")})
        }
        Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
            json!({"reachable":false,"text":format!("No answer from {ip} within 0.8 s: check the address and that it's powered and on this network")})
        }
        Err(e) => json!({"reachable":false,"text":format!("Can't reach {ip}: {e}")}),
    }
}

/// The Controllers section's list (`controller_overview`): every controller in pane order with its run state, address,
/// universes, patched outputs and output health. `status` is the engine's status for a published controller, or `None`
/// headless or for one added since the last publish.
pub fn overview(s: &SetupSession, core: Option<&AppCore>) -> Value {
    let rows: Vec<Value> = s
        .controller_order
        .iter()
        .filter_map(|slot| s.controllers.get(*slot as usize).map(|c| (*slot, c)))
        .filter(|(_, c)| c.alive)
        .map(|(slot, c)| {
            let info = info(c);
            let status = core.filter(|_| !c.is_new).map(|core| status(core, c.id));
            overview_row(s, slot, c, &info, status.as_ref())
        })
        .collect();
    let running = rows.iter().filter(|r| r["running"] == true).count();
    let attention = rows.iter().filter(|r| matches!(r["health"]["level"].as_str(), Some("warning" | "error"))).count();
    json!({"controllers":rows,"running":running,"needAttention":attention})
}

fn overview_row(s: &SetupSession, slot: u32, c: &ControllerEntry, info: &Value, status: Option<&Value>) -> Value {
    let outputs = c.outputs.len();
    let patched = c.outputs.iter().filter(|o| s.flow.source(o.cid).is_some()).count();
    let universes = match (info["protocol"].as_str(), info["universeCount"].as_u64().unwrap_or(0)) {
        (Some("e131"), 0) => "None".to_string(),
        (Some("e131"), count) => match (info["generated"].as_bool(), info["firstUniverse"].as_u64(), info["universeSize"].as_u64()) {
            (Some(true), Some(first), Some(size)) if count == 1 => format!("{first} ({size} ch)"),
            (Some(true), Some(first), Some(size)) => format!("{first}–{} ({size} ch)", first + count - 1),
            _ => plural(count as usize, "universe", "universes"),
        },
        (Some("ddp"), _) => "One DDP stream".to_string(),
        _ => String::new(),
    };
    let problems: Vec<String> = match &c.config {
        Some(ControllerConfig::E131(config)) => config.validate(outputs).into_iter().map(|p| p.lines().next().unwrap_or_default().to_string()).collect(),
        _ => Vec::new(),
    };
    let (level, text) = health(c, info, status, &problems, patched);
    json!({
        "id":c.id.to_string(),"slot":slot,"name":c.name,"typeName":c.type_name(),"protocol":info["protocol"],
        "address":info["address"],"destination":info["destination"],"universes":universes,"outputs":outputs,
        "patchedOutputs":patched,"running":c.running,"health":{"level":level,"text":text},"problems":problems,
        "status":status.cloned().unwrap_or(Value::Null),
    })
}

/// One word for the list's health column and the sentence behind it. Levels: `error` (it can't send as set up or the
/// engine reports errors), `warning` (no address, nothing patched, or packets stalled), `ok` (sending), `stopped`, `off`
/// (Live Output off or muted) and `pending` (added since the last publish).
fn health(c: &ControllerEntry, info: &Value, status: Option<&Value>, problems: &[String], patched: usize) -> (&'static str, String) {
    // A plugin controller whose plugin is missing, disabled or too old stays in the show, silent, and says what it needs.
    if let Some(problem) = c.config.as_ref().and_then(studio_external::problem) {
        return ("error", problem);
    }
    // Universes are generated once there's an address, so a missing one comes before the table's problems.
    if info["protocol"].is_string() && info["destination"] == "none" {
        return ("warning", "No address: enter the controller's IP address".into());
    }
    // A new DDP controller starts at the loopback address.
    if info["address"].as_str().and_then(|a| a.parse::<std::net::Ipv4Addr>().ok()).is_some_and(|ip| ip.is_loopback()) {
        return ("warning", "Address is this Mac: enter the controller's IP address".into());
    }
    if let Some(first) = problems.first() {
        return ("error", first.clone());
    }
    if patched == 0 && !c.outputs.is_empty() {
        return ("warning", "Nothing patched: no element sends to these outputs".into());
    }
    let Some(status) = status else {
        return ("pending", if c.running { "Starts with Live Output".into() } else { "Stopped".into() });
    };
    if status["muted"] == true || status["liveOutput"] != true {
        let text = status["text"].as_str().unwrap_or_default();
        return ("off", if c.running { text.to_string() } else { "Stopped".into() });
    }
    if !c.running || status["running"] != true {
        return ("stopped", "Stopped: not sending".into());
    }
    // A plugin's own state shows while it's current: `unavailable` (still looking for its devices) is a warning.
    if let Some(problem) = status["moduleProblem"].as_str().filter(|_| matches!(c.config, Some(ControllerConfig::External(_)))) {
        return (if status["moduleState"] == "unavailable" { "warning" } else { "error" }, problem.to_string());
    }
    if let Some(text) = status["problem"]["text"].as_str() {
        return (if status["problem"]["level"] == "error" { "error" } else { "warning" }, text.to_string());
    }
    if status["lastPacketMsAgo"].as_u64().is_some_and(|ms| ms >= 2000) {
        return ("warning", status["text"].as_str().unwrap_or_default().to_string());
    }
    ("ok", status["text"].as_str().unwrap_or_default().to_string())
}

/// The MCP `controller_overview` tool: the Controllers section's list.
pub fn overview_tool() -> Value {
    json!({"name":"controller_overview","description":"Every controller as the show window's Controllers section lists it, in pane order: name, type, protocol, address, universes, outputs and how many are patched, run state (running = set to send while Live Output is on), live status (packets sent, last packet, send errors with how long ago the last one was) and health {level: ok|stopped|off|pending|warning|error, text}, with problems (universe table errors). needAttention counts warning and error rows. Read-only; change settings with controller_configure, run state with output_start/output_stop.","inputSchema":{"type":"object","properties":{},"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false,"openWorldHint":false}})
}

pub fn overview_execute(s: &SetupSession, core: Option<&AppCore>, a: &Value) -> Result<Value, String> {
    if let Some(extra) = a.as_object().ok_or("arguments must be an object")?.keys().next() {
        return Err(format!("Unknown argument {extra}"));
    }
    Ok(overview(s, core))
}

/// The MCP `controller_network` tool: settings, presets and status for one controller; `check:true` adds a
/// reachability check, refused while muted (headless sessions always are).
pub fn tool() -> Value {
    json!({"name":"controller_network","description":"One E1.31/DDP controller's network settings as the controller inspector shows them: protocol, address, destination, first universe, channels per universe, generated universe layout, pixel counts, WLED presets, and status (running, packets sent, last packet). check:true also tests whether the address answers (a TCP connection to port 80; no lighting data), refused while muted. Change settings with controller_configure.","inputSchema":{"type":"object","properties":{"id":{"type":"string","minLength":1,"maxLength":64},"check":{"type":"boolean"}},"required":["id"],"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false,"openWorldHint":true}})
}

pub fn execute(s: &SetupSession, core: Option<&AppCore>, a: &Value) -> Result<Value, String> {
    let obj = a.as_object().ok_or("arguments must be an object")?;
    if let Some(extra) = obj.keys().find(|k| !["id", "check"].contains(&k.as_str())) {
        return Err(format!("Unknown argument {extra}"));
    }
    let id = a["id"].as_str().and_then(|t| Uuid::parse_str(t).ok()).ok_or("id must be a controller UUID")?;
    let slot = s.slot_of(id).ok_or("Controller not found")?;
    let c = &s.controllers[slot as usize];
    let mut v = info(c);
    v["presets"] = presets();
    v["status"] = match core {
        Some(core) => status(core, id),
        None => json!({"running":false,"liveOutput":false,"muted":true,"text":"Muted: nothing is sent to controllers"}),
    };
    if a.get("check").and_then(Value::as_bool) == Some(true) {
        let muted = core.is_none_or(|core| !core.playback.hardware_allowed());
        v["check"] = match (muted, v["address"].as_str()) {
            (true, _) => json!({"reachable":null,"text":"Muted: reachability checks are off"}),
            (false, None) => json!({"reachable":false,"text":"No address set"}),
            (false, Some(address)) => probe(address),
        };
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setup::SetupSession;

    fn session() -> (SetupSession, Uuid) {
        let mut s = SetupSession::from_system(vixen_io::SystemData::default(), &Default::default(), false);
        let slot = s.add_controller(ids::E131, "WLED", 10).unwrap();
        let id = s.controllers[slot as usize].id;
        (s, id)
    }

    #[test]
    fn send_error_ages_read_as_words() {
        let ages = [0, 4, 5, 59, 60, 3599, 7200].map(|s| age_text(Duration::from_secs(s)));
        assert_eq!(ages, ["just now", "just now", "5 s ago", "59 s ago", "1 min ago", "59 min ago", "2 h ago"]);
    }

    #[test]
    fn wled_rgbw_tree_generates_seven_full_universes_without_the_table() {
        let (mut s, id) = session();
        let r = configure(&mut s, &json!({"id":id.to_string(),"address":"192.168.1.60","outputs":3584,"preset":"wledRgbw"})).unwrap();
        let c = &r["controller"];
        assert_eq!(c["outputs"], 3584);
        assert_eq!(c["universeCount"], 7);
        assert_eq!(c["universeSize"], 512);
        assert_eq!(c["firstUniverse"], 1);
        assert_eq!(c["destination"], "unicast");
        assert_eq!(c["generated"], true);
        assert_eq!(c["pixels"]["rgbw"], 896);
        assert_eq!(c["layout"], "universes 1–7, 512 channels each");
        let ControllerConfig::E131(config) = s.controllers[0].config.as_ref().unwrap() else { panic!() };
        assert!(config.multicast.is_none());
        let u = config.universes.as_ref().unwrap();
        assert_eq!(u.iter().map(|u| (u.universe, u.start, u.size)).collect::<Vec<_>>(), (0..7).map(|i| (i + 1, i * 512, 512)).collect::<Vec<_>>());
        assert!(config.validate(3584).is_empty());
    }

    #[test]
    fn later_edits_keep_unspecified_fields_and_regenerate_universes() {
        let (mut s, id) = session();
        let id = id.to_string();
        configure(&mut s, &json!({"id":id,"address":"10.0.0.5","firstUniverse":20,"universeSize":512,"outputs":1024})).unwrap();
        // Only the output count changes: same address, first universe and size.
        let r = configure(&mut s, &json!({"id":id,"outputs":1100})).unwrap();
        assert_eq!(r["controller"]["address"], "10.0.0.5");
        assert_eq!(r["controller"]["layout"], "universes 20–22, 512 channels each");
        // The default size for a fresh controller is still 510.
        let (mut fresh, other) = session();
        let r = configure(&mut fresh, &json!({"id":other.to_string(),"address":"10.0.0.6"})).unwrap();
        assert_eq!(r["controller"]["universeSize"], 510);
    }

    #[test]
    fn a_size_chosen_for_one_universe_is_shown_back() {
        let (mut s, id) = session();
        let r = configure(&mut s, &json!({"id":id.to_string(),"address":"10.0.0.5","preset":"wledRgbw"})).unwrap();
        assert_eq!(r["controller"]["universeSize"], 512);
        assert_eq!(r["controller"]["universeCount"], 1);
        // And growing past one universe keeps it.
        let r = configure(&mut s, &json!({"id":id.to_string(),"outputs":600})).unwrap();
        assert_eq!(r["controller"]["layout"], "universes 1–2, 512 channels each");
    }

    #[test]
    fn refusals_change_nothing() {
        let (mut s, id) = session();
        let id = id.to_string();
        assert!(configure(&mut s, &json!({"id":id})).unwrap_err().contains("address is required"));
        assert!(configure(&mut s, &json!({"id":id,"address":"wled.local"})).is_err());
        assert!(configure(&mut s, &json!({"id":id,"address":"10.0.0.5","preset":"wledRgbw","protocol":"ddp"})).is_err());
        assert!(configure(&mut s, &json!({"id":id,"address":"10.0.0.5","firstUniverse":63999,"outputs":1100})).unwrap_err().contains("63999"));
        assert_eq!(s.controllers[0].outputs.len(), 10);
        assert!(!s.controllers[0].config_changed);
    }

    #[test]
    fn switching_protocol_keeps_name_place_and_outputs() {
        let (mut s, id) = session();
        let other = s.add_controller(ids::DDP, "Other", 3).unwrap();
        let output_ids: Vec<Uuid> = s.controllers[0].outputs.iter().map(|o| o.id).collect();
        let r = configure(&mut s, &json!({"id":id.to_string(),"protocol":"ddp","address":"10.0.0.7"})).unwrap();
        let new_id = Uuid::parse_str(r["newId"].as_str().unwrap()).unwrap();
        assert_ne!(new_id, id);
        assert!(s.slot_of(id).is_none());
        let slot = s.slot_of(new_id).unwrap() as usize;
        assert_eq!(s.controller_order, vec![slot as u32, other]);
        let c = &s.controllers[slot];
        assert_eq!((c.name.as_str(), c.type_id), ("WLED", ids::DDP));
        assert_eq!(c.outputs.iter().map(|o| o.id).collect::<Vec<_>>(), output_ids);
        assert_eq!(r["controller"]["protocol"], "ddp");
        assert_eq!(r["controller"]["address"], "10.0.0.7");
        // And back, keeping the address.
        let r = configure(&mut s, &json!({"id":new_id.to_string(),"preset":"wledRgb"})).unwrap();
        assert_eq!(r["controller"]["protocol"], "e131");
        assert_eq!(r["controller"]["address"], "10.0.0.7");
        assert_eq!(r["controller"]["universeSize"], 510);
    }

    #[test]
    fn agent_sets_up_the_tree_controller_in_one_call_and_undo_reverts_it() {
        let root = std::env::temp_dir().join(format!("studio-network-{}", Uuid::new_v4()));
        let core = AppCore::open(root.clone(), true).unwrap();
        let run = |name: &str, args: Value| core.studio_display.execute(&core, name, &args, true);
        let created = run("controller_create", json!({"name":"Tree","typeId":ids::E131.to_string(),"outputs":1})).unwrap();
        let id = created["result"]["id"].as_str().unwrap().to_string();
        let r = run("controller_configure", json!({"id":id,"address":"192.168.1.60","outputs":3584,"preset":"wledRgbw"})).unwrap();
        assert_eq!(r["result"]["controller"]["layout"], "universes 1–7, 512 channels each");
        let read = |core: &AppCore| {
            let s = SetupSession::from_system(core.system.read().clone(), &Default::default(), false);
            execute(&s, Some(core), &json!({"id":id})).unwrap()
        };
        let v = read(&core);
        assert_eq!((v["outputs"].as_u64(), v["universeCount"].as_u64(), v["address"].as_str()), (Some(3584), Some(7), Some("192.168.1.60")));
        assert_eq!(v["status"]["text"], "Muted: nothing is sent to controllers");
        assert_eq!(execute(&SetupSession::from_system(core.system.read().clone(), &Default::default(), false), Some(&core), &json!({"id":id,"check":true})).unwrap()["check"]["reachable"], Value::Null);
        run("display_undo", json!({})).unwrap();
        let v = read(&core);
        assert_eq!(v["outputs"], 1);
        assert_eq!(v["destination"], "none");
        drop(core);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn overview_lists_controllers_in_order_with_universes_patching_and_health() {
        let mut s = crate::setup::test_support::session(&["Tree"], 0);
        let tree = s.add_controller(ids::E131, "Tree", 10).unwrap();
        let roof = s.add_controller(ids::DDP, "Roof", 6).unwrap();
        let tree_id = s.controllers[tree as usize].id.to_string();
        configure(&mut s, &json!({"id":tree_id,"address":"192.168.1.60","outputs":3584,"preset":"wledRgbw"})).unwrap();
        // One of the tree's outputs is patched to the element.
        let element = s.flow.components().find(|(_, c)| matches!(c.kind, crate::setup::flow::CompKind::Element)).map(|(cid, _)| cid).unwrap();
        let output = s.controllers[tree as usize].outputs[0].cid;
        assert!(s.flow.set_source(output, crate::setup::flow::SourceRef { component: element, output: 0 }));
        s.controller_order = vec![roof, tree];

        let v = overview(&s, None);
        let rows = v["controllers"].as_array().unwrap();
        assert_eq!(rows.iter().map(|r| r["name"].as_str().unwrap()).collect::<Vec<_>>(), ["Roof", "Tree"]);
        let (roof_row, tree_row) = (&rows[0], &rows[1]);
        assert_eq!(tree_row["universes"], "1–7 (512 ch)");
        assert_eq!((tree_row["outputs"].as_u64(), tree_row["patchedOutputs"].as_u64()), (Some(3584), Some(1)));
        assert_eq!(tree_row["address"], "192.168.1.60");
        assert_eq!(tree_row["running"], true);
        // Added since the last publish, so the engine has no status for it yet.
        assert_eq!(tree_row["health"]["level"], "pending");
        assert_eq!(roof_row["universes"], "One DDP stream");
        assert_eq!(roof_row["health"]["level"], "warning");
        assert!(roof_row["health"]["text"].as_str().unwrap().starts_with("Address is this Mac"));
        assert_eq!((v["running"].as_u64(), v["needAttention"].as_u64()), (Some(2), Some(1)));

        // With an address but nothing patched it's still a warning, and a universe table that doesn't add up is an error.
        let roof_id = s.controllers[roof as usize].id.to_string();
        configure(&mut s, &json!({"id":roof_id,"address":"10.0.0.9"})).unwrap();
        assert!(overview(&s, None)["controllers"][0]["health"]["text"].as_str().unwrap().starts_with("Nothing patched"));
        let Some(ControllerConfig::E131(config)) = s.controllers[tree as usize].config.as_mut() else { panic!() };
        config.universes.as_mut().unwrap().pop();
        let row = &overview(&s, None)["controllers"][1];
        assert_eq!(row["health"]["level"], "error");
        assert_eq!(row["universes"], "6 universes");
        assert!(row["problems"][0].as_str().unwrap().contains("does not match"));
        assert!(overview_execute(&s, None, &json!({"x":1})).is_err());
    }

    #[test]
    fn overview_reports_live_status_for_published_controllers() {
        let root = std::env::temp_dir().join(format!("studio-overview-{}", Uuid::new_v4()));
        let core = AppCore::open(root.clone(), true).unwrap();
        let run = |name: &str, args: Value| core.studio_display.execute(&core, name, &args, true);
        run("controller_create", json!({"name":"Tree","typeId":ids::E131.to_string(),"outputs":3})).unwrap();
        let s = SetupSession::open(&core);
        let row = &overview(&s, Some(&core))["controllers"][0];
        assert_eq!(row["name"], "Tree");
        assert_eq!(row["status"]["muted"], true);
        // No address comes before what the engine is doing.
        assert_eq!(row["health"]["level"], "warning");
        drop(core);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn probe_counts_a_refusal_as_reachable() {
        // A loopback port nobody listens on refuses at once; nothing leaves the machine.
        let v = probe("127.0.0.1");
        assert_eq!(v["reachable"], true, "{v}");
        assert_eq!(probe("not an address")["reachable"], false);
    }
}
