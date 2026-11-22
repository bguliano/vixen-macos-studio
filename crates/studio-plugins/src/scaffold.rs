//! `plugin_scaffold`: a working plugin from a template, written to a new folder under `Plugins/dev/` and nowhere else.
//! The templates live in `studio/plugins/templates/`; the SDKs (P5) will replace them with fuller ones.
use crate::manifest::Manifest;
use crate::registry::{private_dir, write_atomic};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const LANGUAGES: [&str; 2] = ["python", "typescript"];
pub const KINDS: [&str; 2] = ["controller", "tools"];

const PYTHON: &str = include_str!("../../../studio/plugins/templates/python/main.py");
const TYPESCRIPT: &str = include_str!("../../../studio/plugins/templates/typescript/main.mjs");

/// One template's files as `(name, contents)`, before the plugin's name is filled in.
pub fn template_files(language: &str) -> Option<(&'static str, &'static str, Value)> {
    match language {
        "python" => Some(("main.py", PYTHON, json!({"command": "python3", "args": ["main.py"]}))),
        "typescript" => Some(("main.mjs", TYPESCRIPT, json!({"command": "node", "args": ["main.mjs"]}))),
        _ => None,
    }
}

/// The manifest a new plugin starts with.
pub fn manifest(id: &str, name: &str, language: &str, kind: &str) -> Value {
    let (_, _, entry) = template_files(language).expect("checked language");
    let mut contributes = json!({});
    if kind == "controller" {
        contributes["controllers"] = json!([{
            "typeId": uuid::Uuid::new_v4().to_string(),
            "name": name,
            "configSchema": {
                "type": "object",
                "properties": {"prefix": {"type": "string", "title": "Log prefix", "default": "", "maxLength": 40}},
                "additionalProperties": false,
            },
            "defaultOutputs": 3,
            "maxOutputs": 512,
            "frameEncodings": ["u8"],
        }]);
    } else {
        contributes["tools"] = json!([{
            "name": "hello",
            "description": "Say hello. Replace it with your own tool.",
            "inputSchema": {"type": "object", "properties": {"name": {"type": "string"}}, "additionalProperties": false},
            "readOnly": true,
        }]);
    }
    json!({
        "id": id,
        "name": name,
        "version": "0.1.0",
        "apiVersion": "^1.0",
        "description": format!("{name}, made with plugin_scaffold."),
        "entry": entry,
        "permissions": {"files": ["plugin-data"]},
        "contributes": contributes,
    })
}

fn readme(name: &str, file: &str, kind: &str) -> String {
    let what = if kind == "controller" {
        "a controller type that prints the channel values it receives to its log"
    } else {
        "an agent tool, `hello`"
    };
    format!(
        "# {name}\n\nA Vixen Studio plugin made by `plugin_scaffold`: {what}.\n\n\
         - `vixen-plugin.json` declares what it contributes (MCP resource `plugins://schema/manifest`).\n\
         - `{file}` speaks the plugin protocol (MCP resource `plugins://api`). Edit the parts marked TODO.\n\n\
         Next: `plugin_validate` with this folder's path, then `plugin_link_dev` (needs Allow agents to manage plugins in \
         Settings ▸ Plugins) so Studio runs it and reloads it when you save.\n"
    )
}

fn valid_name(name: &str) -> bool {
    (1..=80).contains(&name.chars().count())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || " ._()-".contains(c))
        && !name.trim().is_empty()
}

/// Writes the plugin to `<root>/dev/<folder>` (default: the last part of its id). Returns `{folder, files, manifest,
/// nextSteps}`. Refuses an existing folder, so nothing is ever overwritten.
pub fn scaffold(root: &Path, id: &str, name: &str, language: &str, kind: &str, folder: Option<&str>) -> Result<Value, String> {
    if !crate::manifest::valid_plugin_id(id) {
        return Err("id must be lowercase reverse-DNS, like io.github.someone.my-output".into());
    }
    if !valid_name(name) {
        return Err("name must be 1 to 80 letters, digits, spaces and . _ ( ) -".into());
    }
    let (file, template, _) = template_files(language).ok_or_else(|| format!("language must be one of {} (Rust and Swift templates come with the SDKs)", LANGUAGES.join(", ")))?;
    if !KINDS.contains(&kind) {
        return Err(format!("kind must be one of {}", KINDS.join(", ")));
    }
    let folder = folder.unwrap_or_else(|| id.rsplit('.').next().unwrap_or(id));
    let simple = !folder.is_empty() && folder.len() <= 64 && !folder.starts_with('.') && folder.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c));
    if !simple {
        return Err("folder must be a plain folder name (letters, digits, . _ -), created under Plugins/dev".into());
    }
    let dev = root.join("dev");
    private_dir(&dev)?;
    let dir: PathBuf = dev.join(folder);
    if std::fs::symlink_metadata(&dir).is_ok() {
        return Err(format!("{} already exists; pick another folder", dir.display()));
    }
    let manifest = manifest(id, name, language, kind);
    Manifest::from_value(manifest.clone()).map_err(|why| format!("the template's manifest is invalid: {why}"))?;
    private_dir(&dir)?;
    let files = [
        ("vixen-plugin.json".to_string(), serde_json::to_string_pretty(&manifest).expect("serializes") + "\n"),
        (file.to_string(), template.replace("{{NAME}}", name).replace("{{ID}}", id)),
        ("README.md".to_string(), readme(name, file, kind)),
    ];
    for (name, contents) in &files {
        write_atomic(&dir.join(name), contents.as_bytes())?;
    }
    Ok(json!({
        "folder": dir.to_string_lossy(),
        "files": files.iter().map(|(n, _)| n).collect::<Vec<_>>(),
        "manifest": manifest,
        "nextSteps": [
            format!("Edit {file} (the TODO parts) and vixen-plugin.json with your own file tools."),
            format!("plugin_validate {{\"path\": \"{}\"}} until every check passes.", dir.to_string_lossy()),
            format!("plugin_link_dev {{\"path\": \"{}\", \"confirm\": true}} (needs Allow agents to manage plugins); Studio reloads it on every save.", dir.to_string_lossy()),
            "plugin_logs to read what it prints; the build_plugin prompt has the whole workflow.".to_string(),
        ],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_template_makes_a_valid_manifest_and_names_are_checked() {
        for language in LANGUAGES {
            for kind in KINDS {
                Manifest::from_value(manifest("com.example.thing", "Thing", language, kind)).unwrap();
            }
        }
        assert!(valid_name("Lamp Output (LAN)"));
        assert!(!valid_name("Bad \"quote\""));
        assert!(!valid_name("{brace}"));
    }
}
