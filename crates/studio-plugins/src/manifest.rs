//! `vixen-plugin.json`: parsing and validation. `studio/plugins/schema/manifest.schema.json` describes the same rules
//! for SDKs, docs and agents; `tests::schema_matches_validator` keeps the two from drifting.
use crate::semver::{Range, Version};
use serde_json::{Map, Value};
use std::path::Path;

pub const MANIFEST_FILE: &str = "vixen-plugin.json";
/// Manifests are small; anything larger is refused before parsing.
pub const MAX_MANIFEST_BYTES: u64 = 256 * 1024;

pub const EVENTS: [&str; 5] = ["show.started", "show.stopped", "sequence.playing", "sequence.stopped", "liveOutput.changed"];
pub const DATA_POLICIES: [&str; 4] = ["highestWins8Bit", "discardExcess8Bit", "average8Bit", "oneChannelColor"];
pub const FRAME_ENCODINGS: [&str; 3] = ["u8", "json", "u16"];
pub const NETWORK_PROTOCOLS: [&str; 6] = ["udp", "tcp", "http", "https", "ws", "wss"];

// The keys each object may have, shared with the schema drift test.
pub(crate) const TOP_KEYS: [&str; 11] =
    ["id", "name", "version", "apiVersion", "description", "author", "license", "homepage", "entry", "permissions", "contributes"];
pub(crate) const TOP_REQUIRED: [&str; 5] = ["id", "name", "version", "apiVersion", "entry"];
pub(crate) const ENTRY_KEYS: [&str; 2] = ["command", "args"];
pub(crate) const PERMISSION_KEYS: [&str; 4] = ["network", "files", "display", "serial"];
pub(crate) const CONTRIBUTES_KEYS: [&str; 4] = ["controllers", "tools", "commands", "events"];
pub(crate) const CONTROLLER_KEYS: [&str; 8] =
    ["typeId", "name", "configSchema", "defaultOutputs", "maxOutputs", "dataPolicy", "outputNaming", "frameEncodings"];
pub(crate) const CONTROLLER_REQUIRED: [&str; 2] = ["typeId", "name"];
pub(crate) const TOOL_KEYS: [&str; 4] = ["name", "description", "inputSchema", "readOnly"];
pub(crate) const TOOL_REQUIRED: [&str; 2] = ["name", "description"];
pub(crate) const COMMAND_KEYS: [&str; 3] = ["id", "title", "tool"];
pub(crate) const COMMAND_REQUIRED: [&str; 3] = ["id", "title", "tool"];

pub const MAX_OUTPUTS: u64 = 100_000;

#[derive(Clone, Debug, PartialEq)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: Version,
    pub api_version: Range,
    pub description: Option<String>,
    pub entry: Entry,
    pub permissions: Permissions,
    pub controllers: Vec<ControllerType>,
    pub tools: Vec<Tool>,
    pub commands: Vec<Command>,
    pub events: Vec<String>,
    /// The manifest as written, for `plugin_info` and the UI.
    pub raw: Value,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub command: String,
    pub args: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Permissions {
    pub network: Vec<String>,
    pub files: Vec<String>,
    /// `"none"` or `"read"`.
    pub display: String,
    pub serial: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ControllerType {
    pub type_id: String,
    pub name: String,
    pub config_schema: Value,
    pub default_outputs: u64,
    pub max_outputs: u64,
    pub data_policy: String,
    pub output_naming: Option<String>,
    pub frame_encodings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub read_only: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    pub id: String,
    pub title: String,
    pub tool: String,
}

impl Manifest {
    /// Reads `<dir>/vixen-plugin.json`, refusing a symlinked or oversized file.
    pub fn load(dir: &Path) -> Result<Manifest, String> {
        let path = dir.join(MANIFEST_FILE);
        let meta = std::fs::symlink_metadata(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        if !meta.is_file() {
            return Err(format!("{} is not a regular file", path.display()));
        }
        if meta.len() > MAX_MANIFEST_BYTES {
            return Err(format!("{} is larger than {} KiB", path.display(), MAX_MANIFEST_BYTES / 1024));
        }
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        Manifest::parse(&text)
    }

    pub fn parse(text: &str) -> Result<Manifest, String> {
        let value: Value = serde_json::from_str(text).map_err(|e| format!("vixen-plugin.json is not valid JSON: {e}"))?;
        Manifest::from_value(value)
    }

    /// Validates every field, collecting all problems so a developer sees them at once.
    pub fn from_value(value: Value) -> Result<Manifest, String> {
        let mut errors = Errors::default();
        let Some(top) = value.as_object() else {
            return Err("vixen-plugin.json must be a JSON object".into());
        };
        errors.keys("", top, &TOP_KEYS, &TOP_REQUIRED);

        let id = errors.string(top, "", "id", 128).unwrap_or_default();
        if !id.is_empty() && !valid_plugin_id(&id) {
            errors.push("id", "must be lowercase reverse-DNS, like io.github.someone.my-output");
        }
        let name = errors.string(top, "", "name", 80).unwrap_or_default();
        let version = errors.string(top, "", "version", 64).and_then(|text| errors.ok("version", Version::parse(&text)));
        let api_version = errors.string(top, "", "apiVersion", 200).and_then(|text| errors.ok("apiVersion", Range::parse(&text)));
        let description = errors.optional_string(top, "", "description", 500);
        errors.optional_string(top, "", "author", 120);
        errors.optional_string(top, "", "license", 64);
        if let Some(homepage) = errors.optional_string(top, "", "homepage", 500) {
            if !(homepage.starts_with("https://") || homepage.starts_with("http://")) {
                errors.push("homepage", "must be an http(s) URL");
            }
        }

        let entry = match errors.object(top, "", "entry") {
            Some(entry) => {
                errors.keys("entry", entry, &ENTRY_KEYS, &["command"]);
                let command = errors.string(entry, "entry", "command", 1024).unwrap_or_default();
                let args = errors.string_array(entry, "entry", "args", 64, 4096);
                Entry { command, args }
            }
            None => Entry { command: String::new(), args: Vec::new() },
        };

        let mut permissions = Permissions { display: "none".into(), ..Permissions::default() };
        if let Some(object) = errors.optional_object(top, "", "permissions") {
            errors.keys("permissions", object, &PERMISSION_KEYS, &[]);
            permissions.network = errors.string_array(object, "permissions", "network", 32, 260);
            for (index, rule) in permissions.network.iter().enumerate() {
                if let Err(why) = network_rule(rule) {
                    errors.push(&format!("permissions.network[{index}]"), &why);
                }
            }
            permissions.files = errors.string_array(object, "permissions", "files", 8, 64);
            for (index, rule) in permissions.files.iter().enumerate() {
                if rule != "plugin-data" {
                    errors.push(&format!("permissions.files[{index}]"), "must be \"plugin-data\"");
                }
            }
            if let Some(display) = errors.optional_string(object, "permissions", "display", 8) {
                if display != "none" && display != "read" {
                    errors.push("permissions.display", "must be \"none\" or \"read\"");
                }
                permissions.display = display;
            }
            permissions.serial = errors.optional_bool(object, "permissions", "serial").unwrap_or(false);
        }

        let (mut controllers, mut tools, mut commands, mut events) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        if let Some(contributes) = errors.optional_object(top, "", "contributes") {
            errors.keys("contributes", contributes, &CONTRIBUTES_KEYS, &[]);
            for (index, item) in errors.object_array(contributes, "contributes", "controllers", 32).iter().enumerate() {
                let at = format!("contributes.controllers[{index}]");
                if let Some(controller) = controller(&mut errors, &at, item) {
                    if controllers.iter().any(|c: &ControllerType| c.type_id == controller.type_id) {
                        errors.push(&format!("{at}.typeId"), "is used by another controller type");
                    }
                    controllers.push(controller);
                }
            }
            for (index, item) in errors.object_array(contributes, "contributes", "tools", 64).iter().enumerate() {
                let at = format!("contributes.tools[{index}]");
                errors.keys(&at, item, &TOOL_KEYS, &TOOL_REQUIRED);
                let name = errors.string(item, &at, "name", 48).unwrap_or_default();
                if !name.is_empty() && !valid_tool_name(&name) {
                    errors.push(&format!("{at}.name"), "must be lowercase letters, digits and _ starting with a letter");
                }
                if tools.iter().any(|t: &Tool| t.name == name) {
                    errors.push(&format!("{at}.name"), "is used by another tool");
                }
                let description = errors.string(item, &at, "description", 1000).unwrap_or_default();
                let input_schema = errors.optional_schema(item, &at, "inputSchema");
                let read_only = errors.optional_bool(item, &at, "readOnly").unwrap_or(false);
                tools.push(Tool { name, description, input_schema, read_only });
            }
            for (index, item) in errors.object_array(contributes, "contributes", "commands", 64).iter().enumerate() {
                let at = format!("contributes.commands[{index}]");
                errors.keys(&at, item, &COMMAND_KEYS, &COMMAND_REQUIRED);
                let id = errors.string(item, &at, "id", 48).unwrap_or_default();
                if !id.is_empty() && !valid_tool_name(&id) {
                    errors.push(&format!("{at}.id"), "must be lowercase letters, digits and _ starting with a letter");
                }
                if commands.iter().any(|c: &Command| c.id == id) {
                    errors.push(&format!("{at}.id"), "is used by another command");
                }
                let title = errors.string(item, &at, "title", 80).unwrap_or_default();
                let tool = errors.string(item, &at, "tool", 48).unwrap_or_default();
                commands.push(Command { id, title, tool });
            }
            for command in &commands {
                if !command.tool.is_empty() && !tools.iter().any(|t| t.name == command.tool) {
                    errors.push("contributes.commands", &format!("command \"{}\" names unknown tool \"{}\"", command.id, command.tool));
                }
            }
            events = errors.string_array(contributes, "contributes", "events", EVENTS.len(), 64);
            for (index, event) in events.iter().enumerate() {
                if !EVENTS.contains(&event.as_str()) {
                    errors.push(&format!("contributes.events[{index}]"), &format!("must be one of {}", EVENTS.join(", ")));
                } else if events[..index].contains(event) {
                    errors.push(&format!("contributes.events[{index}]"), "is listed twice");
                }
            }
        }

        errors.finish()?;
        Ok(Manifest {
            id,
            name,
            version: version.expect("checked"),
            api_version: api_version.expect("checked"),
            description,
            entry,
            permissions,
            controllers,
            tools,
            commands,
            events,
            raw: value,
        })
    }

    /// The summary `plugin_list` and the Settings list show.
    pub fn summary(&self) -> Value {
        serde_json::json!({
            "id": self.id,
            "name": self.name,
            "version": self.version.to_string(),
            "apiVersion": self.api_version.as_str(),
            "description": self.description,
            "controllerTypes": self.controllers.iter().map(|c| serde_json::json!({"typeId": c.type_id, "name": c.name})).collect::<Vec<_>>(),
            "tools": self.tools.iter().map(|t| t.name.clone()).collect::<Vec<_>>(),
            "commands": self.commands.iter().map(|c| c.id.clone()).collect::<Vec<_>>(),
            "events": self.events,
        })
    }
}

fn controller(errors: &mut Errors, at: &str, item: &Map<String, Value>) -> Option<ControllerType> {
    errors.keys(at, item, &CONTROLLER_KEYS, &CONTROLLER_REQUIRED);
    let type_id = errors.string(item, at, "typeId", 36).unwrap_or_default();
    if !type_id.is_empty() && !valid_uuid(&type_id) {
        errors.push(&format!("{at}.typeId"), "must be a lowercase UUID the plugin keeps forever");
    }
    let name = errors.string(item, at, "name", 80).unwrap_or_default();
    let config_schema = errors.optional_schema(item, at, "configSchema");
    let default_outputs = errors.optional_count(item, at, "defaultOutputs").unwrap_or(1);
    let max_outputs = errors.optional_count(item, at, "maxOutputs").unwrap_or(MAX_OUTPUTS);
    if default_outputs > max_outputs {
        errors.push(&format!("{at}.defaultOutputs"), "is more than maxOutputs");
    }
    let data_policy = errors.optional_string(item, at, "dataPolicy", 32).unwrap_or_else(|| DATA_POLICIES[0].into());
    if !DATA_POLICIES.contains(&data_policy.as_str()) {
        errors.push(&format!("{at}.dataPolicy"), &format!("must be one of {}", DATA_POLICIES.join(", ")));
    }
    let output_naming = errors.optional_string(item, at, "outputNaming", 80);
    let mut frame_encodings = errors.string_array(item, at, "frameEncodings", FRAME_ENCODINGS.len(), 8);
    for encoding in &frame_encodings {
        if !FRAME_ENCODINGS.contains(&encoding.as_str()) {
            errors.push(&format!("{at}.frameEncodings"), &format!("\"{encoding}\" must be one of {}", FRAME_ENCODINGS.join(", ")));
        }
    }
    if frame_encodings.is_empty() {
        frame_encodings.push("u8".into());
    }
    Some(ControllerType { type_id, name, config_schema, default_outputs, max_outputs, data_policy, output_naming, frame_encodings })
}

pub fn valid_plugin_id(id: &str) -> bool {
    let labels: Vec<&str> = id.split('.').collect();
    (3..=128).contains(&id.len())
        && labels.len() >= 2
        && labels.iter().all(|label| {
            !label.is_empty()
                && label.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                && !label.starts_with('-')
                && !label.ends_with('-')
        })
}

fn valid_tool_name(name: &str) -> bool {
    name.len() <= 48
        && name.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

pub fn valid_uuid(text: &str) -> bool {
    text.len() == 36
        && text.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_digit() || (b'a'..=b'f').contains(&b),
        })
}

/// `<protocol>:<target>`: `lan` (the local network), or a host name with an optional port.
fn network_rule(rule: &str) -> Result<(), String> {
    let (protocol, target) = rule.split_once(':').ok_or("must look like udp:lan or https:api.example.com")?;
    if !NETWORK_PROTOCOLS.contains(&protocol) {
        return Err(format!("protocol must be one of {}", NETWORK_PROTOCOLS.join(", ")));
    }
    if target == "lan" {
        return Ok(());
    }
    let (host, port) = match target.rsplit_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (target, None),
    };
    let host_ok = !host.is_empty()
        && host.len() <= 253
        && host.split('.').all(|l| !l.is_empty() && l.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'));
    let port_ok = port.is_none_or(|p| p.parse::<u16>().is_ok_and(|p| p > 0));
    if host_ok && port_ok {
        Ok(())
    } else {
        Err("target must be \"lan\" or a lowercase host name with an optional :port".into())
    }
}

#[derive(Default)]
struct Errors(Vec<String>);

impl Errors {
    fn push(&mut self, at: &str, message: &str) {
        self.0.push(if at.is_empty() { message.to_string() } else { format!("{at} {message}") });
    }

    fn path(parent: &str, key: &str) -> String {
        if parent.is_empty() { key.to_string() } else { format!("{parent}.{key}") }
    }

    fn ok<T>(&mut self, at: &str, result: Result<T, String>) -> Option<T> {
        result.map_err(|why| self.push(at, &format!("is invalid: {why}"))).ok()
    }

    fn keys(&mut self, at: &str, object: &Map<String, Value>, allowed: &[&str], required: &[&str]) {
        for key in object.keys() {
            if !allowed.contains(&key.as_str()) {
                self.push(&Errors::path(at, key), "is not a known field");
            }
        }
        for key in required {
            if !object.contains_key(*key) {
                self.push(&Errors::path(at, key), "is required");
            }
        }
    }

    fn checked_string(&mut self, at: &str, value: &Value, max: usize) -> Option<String> {
        match value.as_str() {
            Some(text) if text.trim().is_empty() => self.push(at, "must not be empty"),
            Some(text) if text.chars().count() > max => self.push(at, &format!("must be at most {max} characters")),
            Some(text) if text.chars().any(char::is_control) => self.push(at, "must not contain control characters"),
            Some(text) => return Some(text.to_string()),
            None => self.push(at, "must be a string"),
        }
        None
    }

    fn string(&mut self, object: &Map<String, Value>, parent: &str, key: &str, max: usize) -> Option<String> {
        // A missing required key is reported by `keys`.
        object.get(key).and_then(|value| self.checked_string(&Errors::path(parent, key), value, max))
    }

    fn optional_string(&mut self, object: &Map<String, Value>, parent: &str, key: &str, max: usize) -> Option<String> {
        self.string(object, parent, key, max)
    }

    fn optional_bool(&mut self, object: &Map<String, Value>, parent: &str, key: &str) -> Option<bool> {
        let value = object.get(key)?;
        value.as_bool().or_else(|| {
            self.push(&Errors::path(parent, key), "must be true or false");
            None
        })
    }

    fn optional_count(&mut self, object: &Map<String, Value>, parent: &str, key: &str) -> Option<u64> {
        let value = object.get(key)?;
        match value.as_u64() {
            Some(n) if (1..=MAX_OUTPUTS).contains(&n) => Some(n),
            _ => {
                self.push(&Errors::path(parent, key), &format!("must be a whole number from 1 to {MAX_OUTPUTS}"));
                None
            }
        }
    }

    fn object<'a>(&mut self, object: &'a Map<String, Value>, parent: &str, key: &str) -> Option<&'a Map<String, Value>> {
        let value = object.get(key)?;
        value.as_object().or_else(|| {
            self.push(&Errors::path(parent, key), "must be an object");
            None
        })
    }

    fn optional_object<'a>(&mut self, object: &'a Map<String, Value>, parent: &str, key: &str) -> Option<&'a Map<String, Value>> {
        self.object(object, parent, key)
    }

    /// A JSON Schema the plugin supplies: any object whose `type`, when present, is `"object"`.
    fn optional_schema(&mut self, object: &Map<String, Value>, parent: &str, key: &str) -> Value {
        let Some(value) = object.get(key) else {
            return serde_json::json!({"type": "object", "properties": {}});
        };
        match value.as_object() {
            Some(schema) if schema.get("type").is_none_or(|t| t == "object") => value.clone(),
            Some(_) => {
                self.push(&Errors::path(parent, key), "must describe an object (\"type\": \"object\")");
                Value::Null
            }
            None => {
                self.push(&Errors::path(parent, key), "must be a JSON Schema object");
                Value::Null
            }
        }
    }

    fn array<'a>(&mut self, object: &'a Map<String, Value>, parent: &str, key: &str, max: usize) -> &'a [Value] {
        let Some(value) = object.get(key) else { return &[] };
        match value.as_array() {
            Some(items) if items.len() <= max => items,
            Some(_) => {
                self.push(&Errors::path(parent, key), &format!("must have at most {max} items"));
                &[]
            }
            None => {
                self.push(&Errors::path(parent, key), "must be an array");
                &[]
            }
        }
    }

    fn string_array(&mut self, object: &Map<String, Value>, parent: &str, key: &str, max: usize, max_len: usize) -> Vec<String> {
        let items = self.array(object, parent, key, max);
        let at = Errors::path(parent, key);
        items.iter().enumerate().filter_map(|(i, item)| self.checked_string(&format!("{at}[{i}]"), item, max_len)).collect()
    }

    fn object_array(&mut self, object: &Map<String, Value>, parent: &str, key: &str, max: usize) -> Vec<Map<String, Value>> {
        let items = self.array(object, parent, key, max);
        let at = Errors::path(parent, key);
        let mut out = Vec::new();
        for (i, item) in items.iter().enumerate() {
            match item.as_object() {
                Some(item) => out.push(item.clone()),
                None => self.push(&format!("{at}[{i}]"), "must be an object"),
            }
        }
        out
    }

    fn finish(self) -> Result<(), String> {
        if self.0.is_empty() { Ok(()) } else { Err(format!("vixen-plugin.json: {}", self.0.join("; "))) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    pub(crate) fn example() -> Value {
        json!({
            "id": "io.github.someone.udp-output",
            "name": "UDP Output",
            "version": "1.2.0",
            "apiVersion": "^1.0",
            "description": "Sends channel values over UDP.",
            "author": "Someone", "license": "MIT", "homepage": "https://example.com/udp-output",
            "entry": {"command": "python3", "args": ["main.py"]},
            "permissions": {"network": ["udp:lan", "https:api.example.com", "tcp:device.local:8080"], "files": ["plugin-data"], "display": "read"},
            "contributes": {
                "controllers": [{
                    "typeId": "6f1c2a8e-3b5d-4c7e-9f10-2a3b4c5d6e7f", "name": "UDP (LAN)",
                    "configSchema": {"type": "object", "properties": {"host": {"type": "string", "format": "ipv4"}}},
                    "defaultOutputs": 3, "maxOutputs": 3000, "dataPolicy": "highestWins8Bit",
                    "outputNaming": "Device {n} {channel}", "frameEncodings": ["u8", "json"]
                }],
                "tools": [{"name": "discover", "description": "Find devices.", "inputSchema": {}, "readOnly": true}],
                "commands": [{"id": "discover", "title": "Discover Devices…", "tool": "discover"}],
                "events": ["show.started", "liveOutput.changed"]
            }
        })
    }

    fn errors(mut edit: impl FnMut(&mut Value)) -> String {
        let mut value = example();
        edit(&mut value);
        Manifest::from_value(value).expect_err("should be refused")
    }

    #[test]
    fn the_example_manifest_is_valid() {
        let manifest = Manifest::from_value(example()).unwrap();
        assert_eq!(manifest.id, "io.github.someone.udp-output");
        assert_eq!(manifest.version, Version::new(1, 2, 0));
        assert!(manifest.api_version.matches(&crate::API_VERSION));
        assert_eq!(manifest.controllers[0].default_outputs, 3);
        assert_eq!(manifest.controllers[0].frame_encodings, ["u8", "json"]);
        assert_eq!(manifest.permissions.display, "read");
        assert_eq!(manifest.summary()["tools"], json!(["discover"]));
    }

    #[test]
    fn a_minimal_manifest_gets_defaults() {
        let manifest = Manifest::parse(r#"{"id":"com.example.min","name":"Min","version":"0.1.0","apiVersion":"^1","entry":{"command":"./run"}}"#).unwrap();
        assert!(manifest.entry.args.is_empty() && manifest.controllers.is_empty());
        assert_eq!(manifest.permissions.display, "none");
    }

    #[test]
    fn every_problem_is_reported_with_its_path() {
        let message = errors(|v| {
            v["id"] = json!("Not.Valid");
            v["version"] = json!("1.2");
            v["surprise"] = json!(true);
            v["contributes"]["controllers"][0]["typeId"] = json!("nope");
            v["contributes"]["events"] = json!(["show.started", "teleport"]);
        });
        for expected in ["id must be lowercase", "version is invalid", "surprise is not a known field", "contributes.controllers[0].typeId", "contributes.events[1]"] {
            assert!(message.contains(expected), "{expected} missing from {message}");
        }
    }

    #[test]
    fn contributions_are_checked_for_consistency() {
        assert!(errors(|v| {
            let first = v["contributes"]["controllers"][0].clone();
            v["contributes"]["controllers"].as_array_mut().unwrap().push(first);
        })
        .contains("is used by another controller type"));
        assert!(errors(|v| v["contributes"]["commands"][0]["tool"] = json!("missing")).contains("unknown tool"));
        assert!(errors(|v| v["contributes"]["controllers"][0]["defaultOutputs"] = json!(5000)).contains("more than maxOutputs"));
        assert!(errors(|v| v["contributes"]["controllers"][0]["configSchema"] = json!({"type": "string"})).contains("must describe an object"));
        assert!(errors(|v| { v["entry"].as_object_mut().unwrap().remove("command"); }).contains("entry.command is required"));
    }

    #[test]
    fn permissions_are_a_closed_vocabulary() {
        assert!(errors(|v| v["permissions"]["network"] = json!(["udp:*"])).contains("permissions.network[0]"));
        assert!(errors(|v| v["permissions"]["network"] = json!(["ftp:lan"])).contains("protocol must be"));
        assert!(errors(|v| v["permissions"]["files"] = json!(["home"])).contains("plugin-data"));
        assert!(errors(|v| v["permissions"]["display"] = json!("write")).contains("permissions.display"));
        assert!(errors(|v| v["permissions"]["camera"] = json!(true)).contains("permissions.camera is not a known field"));
    }

    fn schema() -> Value {
        serde_json::from_str(include_str!("../../../studio/plugins/schema/manifest.schema.json")).unwrap()
    }

    fn names(value: &Value) -> Vec<String> {
        let mut names: Vec<String> = match value {
            Value::Object(map) => map.keys().cloned().collect(),
            Value::Array(items) => items.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
            _ => panic!("expected keys in {value}"),
        };
        names.sort();
        names
    }

    fn sorted(keys: &[&str]) -> Vec<String> {
        let mut keys: Vec<String> = keys.iter().map(|k| k.to_string()).collect();
        keys.sort();
        keys
    }

    #[test]
    fn schema_matches_validator() {
        let schema = schema();
        let defs = &schema["$defs"];
        let pairs: [(&Value, &[&str], &[&str]); 7] = [
            (&schema, &TOP_KEYS, &TOP_REQUIRED),
            (&defs["entry"], &ENTRY_KEYS, &["command"]),
            (&defs["permissions"], &PERMISSION_KEYS, &[]),
            (&defs["contributes"], &CONTRIBUTES_KEYS, &[]),
            (&defs["controller"], &CONTROLLER_KEYS, &CONTROLLER_REQUIRED),
            (&defs["tool"], &TOOL_KEYS, &TOOL_REQUIRED),
            (&defs["command"], &COMMAND_KEYS, &COMMAND_REQUIRED),
        ];
        for (object, keys, required) in pairs {
            assert_eq!(names(&object["properties"]), sorted(keys), "properties of {object}");
            assert_eq!(object.get("required").map(names).unwrap_or_default(), sorted(required), "required of {object}");
            assert_eq!(object["additionalProperties"], json!(false));
        }
        assert_eq!(names(&defs["contributes"]["properties"]["events"]["items"]["enum"]), sorted(&EVENTS));
        assert_eq!(names(&defs["controller"]["properties"]["dataPolicy"]["enum"]), sorted(&DATA_POLICIES));
        assert_eq!(names(&defs["controller"]["properties"]["frameEncodings"]["items"]["enum"]), sorted(&FRAME_ENCODINGS));
    }
}
