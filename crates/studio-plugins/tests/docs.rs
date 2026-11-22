//! The developer documentation in `docs/plugins/` can't drift from the code: PROTOCOL.md against
//! `protocol.schema.json` (every method, direction, kind, timeout, field, type, required flag, error code, and every
//! JSON example validated against its schema), MANIFEST.md against `manifest.schema.json` and the host's validator,
//! and across every page: relative links and their anchors, `plugins://` resources, CLI flags and the pages
//! `plugins://docs/<page>` serves. (studio-mcp's tests check the MCP tool names the pages mention.)
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use studio_plugins::{mcp, HostOptions, PluginHost};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn read(path: &str) -> String {
    std::fs::read_to_string(repo().join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn schema(name: &str) -> Value {
    serde_json::from_str(&read(&format!("studio/plugins/schema/{name}.schema.json"))).unwrap()
}

/// The text from `## heading` (or `### heading`) to the next heading of the same or a higher level.
fn section<'a>(text: &'a str, heading: &str) -> &'a str {
    let level = heading.chars().take_while(|c| *c == '#').count();
    let start = text.find(&format!("\n{heading}\n")).unwrap_or_else(|| panic!("no heading {heading:?}")) + heading.len() + 2;
    let rest = &text[start..];
    let end = rest
        .match_indices('\n')
        .map(|(i, _)| i + 1)
        .find(|&i| {
            let hashes = rest[i..].chars().take_while(|c| *c == '#').count();
            hashes > 0 && hashes <= level && rest[i..].chars().nth(hashes) == Some(' ')
        })
        .unwrap_or(rest.len());
    &rest[..end]
}

/// `### \`name\`` headings directly inside a section, in order.
fn methods(text: &str) -> Vec<String> {
    text.lines().filter_map(|l| l.strip_prefix("### `").and_then(|l| l.strip_suffix('`'))).map(str::to_string).collect()
}

/// The rows of the Markdown table whose first header cell is `first`: name (without backticks) → cells.
fn table(text: &str, first: &str) -> Option<BTreeMap<String, Vec<String>>> {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.iter().position(|l| l.starts_with(&format!("| {first} |")))?;
    let mut rows = BTreeMap::new();
    for line in lines[start + 2..].iter().take_while(|l| l.starts_with('|')) {
        let cells: Vec<String> = line
            .replace("\\|", "\u{1}")
            .trim_matches('|')
            .split('|')
            .map(|c| c.trim().replace('\u{1}', "|"))
            .collect();
        let name = cells[0].trim_matches('`').to_string();
        assert!(rows.insert(name.clone(), cells).is_none(), "{name} listed twice under {first}");
    }
    Some(rows)
}

/// Fenced code blocks: (info string, body).
fn code_blocks(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        if let Some(info) = line.strip_prefix("```") {
            let body: Vec<&str> = lines.by_ref().take_while(|l| !l.starts_with("```")).collect();
            out.push((info.trim().to_string(), body.join("\n")));
        }
    }
    out
}

struct Schemas {
    protocol: Value,
    manifest: Value,
}

impl Schemas {
    fn resolve<'a>(&'a self, node: &'a Value) -> &'a Value {
        match node.get("$ref").and_then(Value::as_str) {
            Some(r) if r.starts_with("#/$defs/") => &self.protocol["$defs"][&r[8..]],
            Some(r) if r.starts_with("manifest.schema.json#/$defs/") => &self.manifest["$defs"][&r[28..]],
            Some(r) => panic!("unknown $ref {r}"),
            None => node,
        }
    }

    /// The Type column for a property: an enum's values, a type list, `array of …`, a referenced definition's name.
    fn type_text(&self, node: &Value) -> String {
        if let Some(r) = node.get("$ref").and_then(Value::as_str).filter(|r| !r.starts_with("#/")) {
            return r.rsplit('/').next().unwrap().to_string();
        }
        let node = self.resolve(node);
        if let Some(values) = node.get("enum").and_then(Value::as_array) {
            return values.iter().map(Value::to_string).collect::<Vec<_>>().join(" | ");
        }
        match node.get("type") {
            None => "any".into(),
            Some(Value::Array(types)) => types.iter().map(|t| t.as_str().unwrap()).collect::<Vec<_>>().join(" | "),
            Some(t) if t == "array" && node.get("items").is_some() => format!("array of {}", self.type_text(&node["items"])),
            Some(t) => t.as_str().unwrap().to_string(),
        }
    }

    /// Problems with `value` against `node`: types, required, unknown fields where properties are listed, enums, items.
    fn check(&self, node: &Value, value: &Value, at: &str, errors: &mut Vec<String>) {
        let node = self.resolve(node);
        if let Some(values) = node.get("enum").and_then(Value::as_array) {
            if !values.contains(value) {
                errors.push(format!("{at}: {value} is not one of {values:?}"));
            }
        }
        let types: Vec<&str> = match node.get("type") {
            Some(Value::String(t)) => vec![t.as_str()],
            Some(Value::Array(ts)) => ts.iter().filter_map(Value::as_str).collect(),
            _ => vec![],
        };
        let kind = match value {
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Number(n) if n.is_i64() || n.is_u64() => "integer",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
        };
        if !types.is_empty() && !types.contains(&kind) && !(kind == "integer" && types.contains(&"number")) {
            errors.push(format!("{at}: {kind}, expected {types:?}"));
        }
        if let (Some(object), Some(properties)) = (value.as_object(), node.get("properties").and_then(Value::as_object)) {
            for required in node.get("required").and_then(Value::as_array).into_iter().flatten() {
                if !object.contains_key(required.as_str().unwrap()) {
                    errors.push(format!("{at}: missing {required}"));
                }
            }
            for (key, item) in object {
                match properties.get(key) {
                    Some(property) => self.check(property, item, &format!("{at}/{key}"), errors),
                    None => errors.push(format!("{at}: {key} isn't in the schema")),
                }
            }
        }
        if let (Some(items), Some(each)) = (value.as_array(), node.get("items")) {
            for (i, item) in items.iter().enumerate() {
                self.check(each, item, &format!("{at}/{i}"), errors);
            }
        }
    }

    fn validate(&self, node: &Value, value: &Value, what: &str) {
        let mut errors = Vec::new();
        self.check(node, value, "", &mut errors);
        assert!(errors.is_empty(), "{what}: {errors:?}\n{value}");
    }
}

/// A field table (or the `Param: \`{}\`.` line) for one schema object, cell by cell.
fn check_fields(schemas: &Schemas, node: Option<&Value>, text: &str, label: &str, method: &str) {
    let node = node.map(|n| schemas.resolve(n));
    let properties = node.and_then(|n| n.get("properties")).and_then(Value::as_object);
    let Some(properties) = properties.filter(|p| !p.is_empty()) else {
        if node.is_some() {
            assert!(text.contains(&format!("\n{label}: `{{}}`.")), "{method}: expected \"{label}: `{{}}`.\"");
        }
        assert!(table(text, label).is_none(), "{method}: has a {label} table but the schema has no fields");
        return;
    };
    let node = node.unwrap();
    let rows = table(text, label).unwrap_or_else(|| panic!("{method}: no {label} table"));
    let documented: BTreeSet<&String> = rows.keys().collect();
    let declared: BTreeSet<&String> = properties.keys().collect();
    assert_eq!(documented, declared, "{method}: {label} fields");
    let required: Vec<&str> = node.get("required").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
    for (name, property) in properties {
        let cells = &rows[name];
        assert_eq!(cells[1], schemas.type_text(property), "{method}: {label} {name} type");
        assert_eq!(cells[2], if required.contains(&name.as_str()) { "yes" } else { "no" }, "{method}: {label} {name} required");
    }
}

#[test]
fn protocol_md_matches_the_protocol_schema() {
    let doc = read("docs/plugins/PROTOCOL.md");
    let schemas = Schemas { protocol: schema("protocol"), manifest: schema("manifest") };
    let protocol = &schemas.protocol;
    assert!(doc.contains(&format!("**{} bytes**", protocol["x-framing"]["maxLineBytes"])), "framing limit");
    assert!(doc.contains(&format!("**{}**", protocol["x-apiVersion"].as_str().unwrap())), "API version");

    // Error codes: the table is exactly the schema's list.
    let errors = table(section(&doc, "## Errors"), "Code").expect("error table");
    let declared: BTreeMap<String, String> = protocol["x-errors"].as_object().unwrap().iter().map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string())).collect();
    let documented: BTreeMap<String, String> = errors.iter().map(|(k, v)| (k.clone(), v[1].clone())).collect();
    assert_eq!(documented, declared);

    let mut all = BTreeMap::new();
    for (key, heading, direction) in [("x-hostToPlugin", "## Host → plugin", "Host → plugin"), ("x-pluginToHost", "## Plugin → host", "Plugin → host")] {
        let part = section(&doc, heading);
        let declared = protocol[key].as_object().unwrap();
        let documented = methods(part);
        assert_eq!(documented.iter().collect::<BTreeSet<_>>(), declared.keys().collect::<BTreeSet<_>>(), "{heading}");
        for (method, spec) in declared {
            let text = section(part, &format!("### `{method}`"));
            let kind = spec["kind"].as_str().unwrap();
            let timeout = match (key, kind) {
                ("x-hostToPlugin", "request") => format!(" · timeout {} ms", spec.get("timeoutMs").and_then(Value::as_u64).unwrap_or(5000)),
                _ => String::new(),
            };
            let meta = format!("{direction} · {kind}{timeout}");
            assert_eq!(text.trim_start().lines().next(), Some(meta.as_str()), "{method}: first line");
            if !["P1", "P2", "P3"].contains(&spec["available"].as_str().unwrap()) {
                assert!(text.contains("Not available yet"), "{method} isn't shipped; the page must say so");
            }
            check_fields(&schemas, spec.get("params"), text, "Param", method);
            match spec.get("result") {
                Some(result) => check_fields(&schemas, Some(result), text, "Result", method),
                None => assert!(!text.contains("\nResult") && table(text, "Result").is_none(), "{method}: a notification has no result"),
            }
            let fields = spec["params"].get("properties").and_then(Value::as_object).map_or(0, |p| p.keys().filter(|k| *k != "instanceId").count());
            let examples = code_blocks(text).into_iter().filter(|(info, _)| info == &format!("json {method} params")).count();
            assert!(fields == 0 || examples > 0, "{method}: needs a params example");
            all.insert(method.clone(), spec.clone());
        }
    }

    // Every JSON example parses and fits its schema.
    let mut checked = 0;
    for (info, body) in code_blocks(&doc) {
        let Some(label) = info.strip_prefix("json ") else {
            assert!(info.is_empty() || info == "sh", "untagged code block {info:?}");
            continue;
        };
        let value: Value = serde_json::from_str(&body).unwrap_or_else(|e| panic!("{info}: {e}\n{body}"));
        let (name, part) = label.rsplit_once(' ').unwrap();
        if name == "example" {
            assert_eq!(value["jsonrpc"], json!("2.0"), "{info}");
            if let Some(method) = value["method"].as_str() {
                schemas.validate(&all.get(method).unwrap_or_else(|| panic!("{info}: unknown method {method}"))["params"], &value["params"], &info);
            }
            if part == "error" {
                let code = value["error"]["code"].to_string();
                assert!(protocol["x-errors"].get(&code).is_some(), "{info}: code {code}");
            }
        } else {
            let spec = all.get(name).unwrap_or_else(|| panic!("{info}: unknown method"));
            schemas.validate(spec.get(part).unwrap_or_else(|| panic!("{info}: no {part} in the schema")), &value, &info);
        }
        checked += 1;
    }
    assert!(checked >= 20, "only {checked} examples checked");
}

#[test]
fn manifest_md_matches_the_manifest_schema_and_validator() {
    let doc = read("docs/plugins/MANIFEST.md");
    let manifest = schema("manifest");
    let defs = &manifest["$defs"];
    for (heading, node) in [
        ("## Top level", &manifest),
        ("## `entry`", &defs["entry"]),
        ("## `permissions`", &defs["permissions"]),
        ("## `contributes`", &defs["contributes"]),
        ("## `contributes.controllers[]`", &defs["controller"]),
        ("## `contributes.tools[]`", &defs["tool"]),
        ("## `contributes.commands[]`", &defs["command"]),
    ] {
        let text = section(&doc, heading);
        let rows = table(text, "Field").unwrap_or_else(|| panic!("{heading}: no table"));
        let properties = node["properties"].as_object().unwrap();
        assert_eq!(rows.keys().collect::<BTreeSet<_>>(), properties.keys().collect::<BTreeSet<_>>(), "{heading}");
        let required: Vec<&str> = node.get("required").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
        for (name, property) in properties {
            let cells = &rows[name];
            assert_eq!(cells[2], if required.contains(&name.as_str()) { "yes" } else { "no" }, "{heading} {name} required");
            // Every value an enum allows is named in its row.
            let values = property.get("enum").or_else(|| property.get("items").and_then(|i| i.get("enum")));
            for value in values.and_then(Value::as_array).into_iter().flatten() {
                assert!(cells[3].contains(&format!("`{}`", value.as_str().unwrap())), "{heading} {name}: {value} not described");
            }
        }
    }
    let network = &section(&doc, "## `permissions`");
    let pattern = defs["permissions"]["properties"]["network"]["items"]["pattern"].as_str().unwrap();
    let protocols = pattern.trim_start_matches("^(").split(')').next().unwrap();
    for protocol in protocols.split('|') {
        assert!(network.contains(&format!("`{protocol}`")), "network protocol {protocol}");
    }

    // The examples on the manifest and controller pages are manifests the host accepts.
    let example = code_blocks(&doc).into_iter().find(|(info, _)| info == "json vixen-plugin.json").expect("the complete example").1;
    studio_plugins::manifest::Manifest::parse(&example).unwrap();
    let controllers = read("docs/plugins/CONTROLLERS.md");
    let recipe = code_blocks(&controllers).into_iter().find(|(info, _)| info == "json recipe configSchema").expect("recipe").1;
    let mut with_recipe: Value = serde_json::from_str(&example).unwrap();
    with_recipe["contributes"]["controllers"][0]["configSchema"] = serde_json::from_str(&recipe).unwrap();
    studio_plugins::manifest::Manifest::from_value(with_recipe).unwrap();
}

fn pages() -> Vec<(String, String)> {
    let mut pages: Vec<(String, String)> = std::fs::read_dir(repo().join("docs/plugins"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".md"))
        .map(|n| (n.clone(), read(&format!("docs/plugins/{n}"))))
        .collect();
    pages.sort();
    pages
}

/// GitHub's anchor for a heading: lowercase, punctuation dropped, spaces to hyphens.
fn anchor(heading: &str) -> String {
    heading.to_lowercase().chars().filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-' || *c == '_').map(|c| if c == ' ' { '-' } else { c }).collect()
}

fn anchors(text: &str) -> BTreeSet<String> {
    text.lines().filter(|l| l.starts_with('#')).map(|l| anchor(l.trim_start_matches('#').trim())).collect()
}

#[test]
fn pages_link_to_real_files_anchors_resources_and_flags() {
    let pages = pages();
    // plugins://docs serves exactly these pages, unchanged.
    let served: BTreeSet<String> = mcp::DOCS.iter().map(|(name, _)| format!("{name}.md")).collect();
    assert_eq!(served, pages.iter().map(|(n, _)| n.clone()).collect::<BTreeSet<_>>());
    for (name, text) in mcp::DOCS {
        assert_eq!(text, read(&format!("docs/plugins/{name}.md")), "{name}");
    }

    let base = std::env::temp_dir().join(format!("studio-plugins-docs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let host = PluginHost::new(&base.join("Plugins"), HostOptions::default()).unwrap();
    let usage = studio_plugins::cli::USAGE;

    for (page, text) in &pages {
        let folder = repo().join("docs/plugins");
        // Relative links: the file exists and the anchor is a heading in it.
        for (start, _) in text.match_indices("](") {
            let target = &text[start + 2..start + 2 + text[start + 2..].find(')').unwrap()];
            if target.starts_with("http") {
                continue;
            }
            let (file, fragment) = target.split_once('#').unwrap_or((target, ""));
            let path = if file.is_empty() { folder.join(page) } else { folder.join(file) };
            assert!(path.exists(), "{page}: link to {target}");
            if !fragment.is_empty() {
                let linked = std::fs::read_to_string(&path).unwrap();
                assert!(anchors(&linked).contains(fragment), "{page}: no heading for #{fragment} in {target}");
            }
        }
        // plugins:// resources: concrete URIs read; patterns name a real family.
        for (start, _) in text.match_indices("plugins://") {
            let uri: String = text[start..].chars().take_while(|c| !matches!(c, '`' | ' ' | ')' | ',' | '\n')).collect();
            let uri = uri.trim_end_matches(['.', ':']);
            if uri.contains('<') {
                let family = uri.split('<').next().unwrap();
                assert!(["plugins://docs/", "plugins://examples/", "plugins://sdk/", "plugins://"].contains(&family), "{page}: {uri}");
                continue;
            }
            let read = mcp::read_resource(&host, uri).unwrap_or_else(|| panic!("{page}: {uri} isn't a plugin resource"));
            assert!(read.is_ok(), "{page}: {uri}: {read:?}");
        }
        // Every CLI flag shown with `vixen-studio-mcp plugin` exists.
        for line in text.lines().filter(|l| l.contains("vixen-studio-mcp plugin ")) {
            for flag in line.split_whitespace().map(|w| w.trim_matches(|c: char| !(c.is_alphanumeric() || c == '-'))).filter(|w| w.starts_with("--")) {
                assert!(usage.contains(&format!("{flag} ")) || usage.contains(&format!("{flag}]")), "{page}: {flag} isn't a plugin CLI flag");
            }
        }
    }
    let _ = std::fs::remove_dir_all(&base);
}
