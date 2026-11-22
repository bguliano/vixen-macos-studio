//! What Settings ▸ Plugins and the controller inspector show, worked out here so the Swift views only draw it: the form a
//! controller type's `configSchema` becomes, a plugin's permissions in words, and the commands plugins contribute to
//! the Plugins menu and the ⌘K palette.
use crate::manifest::{Manifest, Permissions};
use serde_json::{json, Map, Value};

/// How deep objects and tables nest in a form; deeper settings are edited as JSON text.
const MAX_DEPTH: usize = 4;

/// The inspector form for a `configSchema`: `{fields: [field], unsupported: bool}`. Each field is `{key, pointer,
/// label, help?, kind, required, default?, …}` where `kind` is `string` (with `format` ipv4/hostname, `minLength`,
/// `maxLength`), `number`/`integer` (`minimum`, `maximum`), `boolean`, `choice` (`options: [{value, label}]`),
/// `object` (`fields`), `table` (an array of objects: `columns`, `newRow`, `minItems`, `maxItems`), `list` (an array of
/// strings or numbers: `item`, `minItems`, `maxItems`) or `json` (anything else, edited as text). Properties come in
/// the order of the object's `x-order` array of property names, then by each property's integer `x-order` (lowest
/// first), then by name. A schema that isn't an object of properties gives one `json` field
/// for the whole configuration.
pub fn form(schema: &Value) -> Value {
    if schema.get("properties").and_then(Value::as_object).is_some() {
        json!({"fields": fields(schema, "", 0), "unsupported": false})
    } else if schema.as_object().is_none_or(|o| o.is_empty()) {
        json!({"fields": [], "unsupported": false})
    } else {
        let whole = json!({"key": "", "pointer": "", "label": "Configuration", "kind": "json", "required": true});
        json!({"fields": [whole], "unsupported": true})
    }
}

fn fields(schema: &Value, at: &str, depth: usize) -> Vec<Value> {
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else { return Vec::new() };
    let required: Vec<&str> = schema.get("required").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
    let mut names: Vec<&String> = properties.keys().collect();
    // The documented form is an array of names on the object; an integer on each property also works.
    let listed: Vec<&str> = schema.get("x-order").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
    let order = |name: &String| {
        let position = listed.iter().position(|n| n == name).unwrap_or(usize::MAX);
        (position, properties[name].get("x-order").and_then(Value::as_i64).unwrap_or(i64::MAX))
    };
    names.sort_by(|a, b| order(a).cmp(&order(b)).then(a.cmp(b)));
    names
        .into_iter()
        .map(|name| field(&properties[name], name, &format!("{at}/{}", escape(name)), required.contains(&name.as_str()), depth))
        .collect()
}

fn field(schema: &Value, key: &str, pointer: &str, required: bool, depth: usize) -> Value {
    let mut out = Map::new();
    out.insert("key".into(), json!(key));
    out.insert("pointer".into(), json!(pointer));
    out.insert("label".into(), json!(schema.get("title").and_then(Value::as_str).map_or_else(|| label(key), str::to_string)));
    if let Some(help) = schema.get("description").and_then(Value::as_str) {
        out.insert("help".into(), json!(help));
    }
    out.insert("required".into(), json!(required));
    if let Some(default) = schema.get("default") {
        out.insert("default".into(), default.clone());
    }
    let kind = kind(schema, depth);
    out.insert("kind".into(), json!(kind));
    let copy = |out: &mut Map<String, Value>, keys: &[&str]| {
        for k in keys {
            if let Some(v) = schema.get(*k) {
                out.insert((*k).into(), v.clone());
            }
        }
    };
    match kind {
        "string" => copy(&mut out, &["format", "minLength", "maxLength"]),
        "number" | "integer" => copy(&mut out, &["minimum", "maximum"]),
        "choice" => {
            let values = schema.get("enum").and_then(Value::as_array).cloned().unwrap_or_default();
            let names = schema.get("x-enumNames").and_then(Value::as_array);
            let options: Vec<Value> = values
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    let shown = names.and_then(|n| n.get(i)).and_then(Value::as_str).map_or_else(|| value_label(v), str::to_string);
                    json!({"value": v, "label": shown})
                })
                .collect();
            out.insert("options".into(), json!(options));
        }
        "object" => {
            out.insert("fields".into(), json!(fields(schema, pointer, depth + 1)));
        }
        "table" => {
            let items = &schema["items"];
            out.insert("columns".into(), json!(fields(items, "", depth + 1)));
            out.insert("newRow".into(), crate::schema::defaults(items));
            copy(&mut out, &["minItems", "maxItems"]);
        }
        "list" => {
            out.insert("item".into(), field(&schema["items"], "", "", true, depth + 1));
            copy(&mut out, &["minItems", "maxItems"]);
        }
        _ => {}
    }
    Value::Object(out)
}

fn types(schema: &Value) -> Vec<&str> {
    match schema.get("type") {
        Some(Value::String(t)) => vec![t.as_str()],
        Some(Value::Array(list)) => list.iter().filter_map(Value::as_str).filter(|t| *t != "null").collect(),
        _ => Vec::new(),
    }
}

fn kind(schema: &Value, depth: usize) -> &'static str {
    if schema.get("enum").and_then(Value::as_array).is_some_and(|e| !e.is_empty() && e.iter().all(|v| v.is_string() || v.is_number())) {
        return "choice";
    }
    match types(schema).as_slice() {
        ["string"] => "string",
        ["integer"] => "integer",
        ["number"] => "number",
        ["boolean"] => "boolean",
        ["object"] if depth < MAX_DEPTH && schema.get("properties").is_some_and(Value::is_object) => "object",
        ["array"] if depth < MAX_DEPTH => {
            let items = &schema["items"];
            match types(items).as_slice() {
                ["object"] if items.get("properties").is_some_and(Value::is_object) => {
                    // A table's cells are plain values; nested objects or arrays in a row need the JSON editor.
                    let flat = items["properties"].as_object().is_some_and(|p| p.values().all(|c| matches!(kind(c, MAX_DEPTH), "string" | "integer" | "number" | "boolean" | "choice")));
                    if flat { "table" } else { "json" }
                }
                ["string"] | ["integer"] | ["number"] => "list",
                _ => "json",
            }
        }
        _ => "json",
    }
}

/// `deviceAddress` / `device_address` → "Device address".
fn label(key: &str) -> String {
    let mut words = String::new();
    let mut previous_lower = false;
    for c in key.chars() {
        if c == '_' || c == '-' {
            words.push(' ');
            previous_lower = false;
        } else if c.is_uppercase() && previous_lower {
            words.push(' ');
            words.extend(c.to_lowercase());
            previous_lower = false;
        } else {
            words.push(c);
            previous_lower = c.is_lowercase() || c.is_ascii_digit();
        }
    }
    let mut chars = words.trim().chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn value_label(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn escape(name: &str) -> String {
    name.replace('~', "~0").replace('/', "~1")
}

/// What a plugin may do, one sentence each, for the install sheet and the plugin's details ("This plugin can: …").
pub fn permission_sentences(permissions: &Permissions) -> Vec<String> {
    let mut out = Vec::new();
    for rule in &permissions.network {
        let Some((protocol, target)) = rule.split_once(':') else { continue };
        let what = match protocol {
            "udp" => "send and receive UDP".to_string(),
            "tcp" => "open TCP connections".to_string(),
            "http" => "make HTTP requests".to_string(),
            "https" => "make HTTPS requests".to_string(),
            "ws" | "wss" => "open WebSocket connections".to_string(),
            other => format!("use {other}"),
        };
        if target == "lan" {
            out.push(format!("{} on your local network", capitalised(&what)));
        } else {
            out.push(format!("{} to {target}", capitalised(&what)));
        }
    }
    if permissions.serial {
        out.push("Use serial ports (USB adapters and boards)".into());
    }
    if permissions.files.iter().any(|f| f == "plugin-data") {
        out.push("Keep its own settings in its data folder".into());
    }
    if permissions.display == "read" {
        out.push("Read the show's elements, controllers and patches".into());
    }
    out
}

fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

/// A manifest as the install sheet shows it before anything is installed.
pub fn package_summary(manifest: &Manifest, files: usize, bytes: u64, sha256: Option<&str>) -> Value {
    let mut out = manifest.summary();
    out["author"] = json!(manifest.raw.get("author"));
    out["homepage"] = json!(manifest.raw.get("homepage"));
    out["license"] = json!(manifest.raw.get("license"));
    out["permissions"] = json!(permission_sentences(&manifest.permissions));
    out["files"] = json!(files);
    out["bytes"] = json!(bytes);
    out["sha256"] = json!(sha256);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_schema_becomes_fields_in_order_with_their_limits() {
        let schema = json!({
            "type": "object",
            "required": ["host"],
            "properties": {
                "port": {"type": "integer", "minimum": 1, "maximum": 65535, "default": 4048, "x-order": 2},
                "host": {"type": "string", "format": "ipv4", "description": "The lamp's address.", "x-order": 1},
                "colorOrder": {"enum": ["rgb", "grb"], "x-enumNames": ["RGB", "GRB"]},
                "fast": {"type": "boolean"},
                "a/b": {"type": "number"},
                "devices": {"type": "array", "maxItems": 4, "items": {"type": "object", "properties": {"mac": {"type": "string"}, "on": {"type": "boolean", "default": true}}}},
                "tags": {"type": "array", "items": {"type": "string"}},
                "advanced": {"type": "object", "properties": {"retries": {"type": ["integer", "null"]}}},
                "blob": {"type": "array", "items": {"type": "array"}}
            }
        });
        let form = form(&schema);
        let fields = form["fields"].as_array().unwrap();
        let keys: Vec<&str> = fields.iter().map(|f| f["key"].as_str().unwrap()).collect();
        assert_eq!(keys, ["host", "port", "a/b", "advanced", "blob", "colorOrder", "devices", "fast", "tags"]);
        let by = |key: &str| fields.iter().find(|f| f["key"] == key).unwrap();
        assert_eq!(by("host")["kind"], "string");
        assert_eq!(by("host")["format"], "ipv4");
        assert_eq!(by("host")["required"], true);
        assert_eq!(by("host")["help"], "The lamp's address.");
        assert_eq!(by("port")["maximum"], 65535);
        assert_eq!(by("port")["default"], 4048);
        assert_eq!(by("a/b")["pointer"], "/a~1b");
        assert_eq!(by("colorOrder")["label"], "Color order");
        assert_eq!(by("colorOrder")["options"], json!([{"value": "rgb", "label": "RGB"}, {"value": "grb", "label": "GRB"}]));
        assert_eq!(by("devices")["kind"], "table");
        assert_eq!(by("devices")["newRow"], json!({"on": true}));
        assert_eq!(by("devices")["columns"][0]["key"], "mac");
        assert_eq!(by("tags")["kind"], "list");
        assert_eq!(by("tags")["item"]["kind"], "string");
        assert_eq!(by("advanced")["fields"][0]["pointer"], "/advanced/retries");
        assert_eq!(by("advanced")["fields"][0]["kind"], "integer");
        assert_eq!(by("blob")["kind"], "json");
        assert_eq!(form["unsupported"], false);
    }

    #[test]
    fn an_x_order_array_orders_objects_and_table_columns() {
        let schema = json!({
            "type": "object",
            "x-order": ["zone", "devices"],
            "properties": {
                "alpha": {"type": "string"},
                "devices": {"type": "array", "items": {
                    "type": "object",
                    "x-order": ["name", "mac"],
                    "properties": {"mac": {"type": "string"}, "name": {"type": "string"}, "brightness": {"type": "integer", "x-order": 1}, "apple": {"type": "string"}}
                }},
                "zone": {"type": "string"},
                "beta": {"type": "string", "x-order": 1}
            }
        });
        let form = form(&schema);
        let keys = |list: &Value| list.as_array().unwrap().iter().map(|f| f["key"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        assert_eq!(keys(&form["fields"]), ["zone", "devices", "beta", "alpha"]);
        assert_eq!(keys(&form["fields"][1]["columns"]), ["name", "mac", "brightness", "apple"]);
    }

    #[test]
    fn schemas_without_properties_edit_as_json_or_have_no_fields() {
        assert_eq!(form(&json!({}))["fields"], json!([]));
        assert_eq!(form(&Value::Null)["fields"], json!([]));
        let whole = form(&json!({"type": "array"}));
        assert_eq!(whole["unsupported"], true);
        assert_eq!(whole["fields"][0]["kind"], "json");
    }

    #[test]
    fn labels_read_as_words() {
        assert_eq!(label("deviceAddress"), "Device address");
        assert_eq!(label("max_fps"), "Max fps");
        assert_eq!(label("universe2Start"), "Universe2 start");
        assert_eq!(label("IP"), "IP");
    }

    #[test]
    fn permissions_are_sentences() {
        let permissions = Permissions {
            network: vec!["udp:lan".into(), "https:openapi.example.com".into()],
            files: vec!["plugin-data".into()],
            display: "read".into(),
            serial: true,
        };
        assert_eq!(
            permission_sentences(&permissions),
            [
                "Send and receive UDP on your local network",
                "Make HTTPS requests to openapi.example.com",
                "Use serial ports (USB adapters and boards)",
                "Keep its own settings in its data folder",
                "Read the show's elements, controllers and patches",
            ]
        );
        assert!(permission_sentences(&Permissions { display: "none".into(), ..Default::default() }).is_empty());
    }
}
