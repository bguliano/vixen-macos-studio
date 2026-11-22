//! A working plugin from a template: `plugin_scaffold` writes it to a new folder under `Plugins/dev/` and nowhere else,
//! `vixen-studio-mcp plugin new` to a new folder wherever the developer runs it. The templates live in
//! `studio/plugins/templates/`; each comes with its SDK (`studio/plugins/sdk/`, `crates/studio-plugin-sdk`), copied in
//! next to the main file so the plugin is self-contained.
use crate::manifest::Manifest;
use crate::registry::{private_dir, write_atomic};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const LANGUAGES: [&str; 3] = ["python", "typescript", "rust"];
pub const KINDS: [&str; 2] = ["controller", "tools"];

pub const PYTHON_SDK: &str = include_str!("../../../studio/plugins/sdk/python/vixen_plugin.py");
pub const TYPESCRIPT_SDK: &str = include_str!("../../../studio/plugins/sdk/typescript/vixen-plugin.mjs");
pub const TYPESCRIPT_TYPES: &str = include_str!("../../../studio/plugins/sdk/typescript/vixen-plugin.d.ts");
pub const RUST_SDK: &str = include_str!("../../../crates/studio-plugin-sdk/src/lib.rs");

/// One file a template writes: its path in the plugin folder, contents (with `{{NAME}}`, `{{ID}}` and `{{CRATE}}`
/// still in them) and whether it is a program.
#[derive(Clone, Copy, Debug)]
pub struct TemplateFile {
    pub path: &'static str,
    pub contents: &'static str,
    pub executable: bool,
}

const fn file(path: &'static str, contents: &'static str) -> TemplateFile {
    TemplateFile { path, contents, executable: false }
}

/// A language's template: its files, the file to edit, and the manifest's `entry`.
pub struct Template {
    pub files: Vec<TemplateFile>,
    pub main: &'static str,
    pub entry: Value,
    /// What has to happen before the plugin can run, if anything.
    pub build: Option<&'static str>,
}

pub fn template(language: &str) -> Option<Template> {
    Some(match language {
        "python" => Template {
            files: vec![file("main.py", include_str!("../../../studio/plugins/templates/python/main.py")), file("vixen_plugin.py", PYTHON_SDK)],
            main: "main.py",
            entry: json!({"command": "python3", "args": ["main.py"]}),
            build: None,
        },
        "typescript" => Template {
            files: vec![
                file("main.mjs", include_str!("../../../studio/plugins/templates/typescript/main.mjs")),
                file("vixen-plugin.mjs", TYPESCRIPT_SDK),
                file("vixen-plugin.d.ts", TYPESCRIPT_TYPES),
            ],
            main: "main.mjs",
            entry: json!({"command": "node", "args": ["main.mjs"]}),
            build: None,
        },
        "rust" => Template {
            files: vec![
                file("Cargo.toml", include_str!("../../../studio/plugins/templates/rust/Cargo.toml")),
                TemplateFile { path: "build.sh", contents: include_str!("../../../studio/plugins/templates/rust/build.sh"), executable: true },
                file(".gitignore", include_str!("../../../studio/plugins/templates/rust/gitignore")),
                file("src/main.rs", include_str!("../../../studio/plugins/templates/rust/src/main.rs")),
                file("src/vixen_plugin_sdk.rs", RUST_SDK),
            ],
            main: "src/main.rs",
            entry: json!({"command": "bin/{{CRATE}}"}),
            build: Some("./build.sh (cargo build --release, then copies the program to bin/, where entry.command points)"),
        },
        _ => return None,
    })
}

/// A Cargo package name from a folder name: lowercase letters, digits and `-`, starting with a letter.
pub fn crate_name(folder: &str) -> String {
    let mut name: String = folder.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    name = name.trim_matches('-').to_string();
    if !name.starts_with(|c: char| c.is_ascii_lowercase()) {
        name = format!("plugin-{name}").trim_end_matches('-').to_string();
    }
    name
}

fn fill(text: &str, id: &str, name: &str, krate: &str) -> String {
    text.replace("{{NAME}}", name).replace("{{ID}}", id).replace("{{CRATE}}", krate)
}

/// The manifest a new plugin starts with.
pub fn manifest(id: &str, name: &str, language: &str, kind: &str, krate: &str) -> Value {
    let entry = template(language).expect("checked language").entry;
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
    let entry: Value = serde_json::from_str(&fill(&entry.to_string(), id, name, krate)).expect("entry stays JSON");
    json!({
        "id": id,
        "name": name,
        "version": "0.1.0",
        "apiVersion": "^1.0",
        "description": format!("{name}, a Vixen Studio plugin."),
        "entry": entry,
        "permissions": {"files": ["plugin-data"]},
        "contributes": contributes,
    })
}

fn readme(name: &str, template: &Template, kind: &str) -> String {
    let what = if kind == "controller" {
        "a controller type that prints the channel values it receives to its log"
    } else {
        "an agent tool, `hello`"
    };
    let build = template.build.map(|b| format!("Build it first: {b}.\n\n")).unwrap_or_default();
    format!(
        "# {name}\n\nA Vixen Studio plugin: {what}.\n\n\
         - `vixen-plugin.json` declares what it contributes (MCP resource `plugins://schema/manifest`).\n\
         - `{}` is your plugin; edit the parts marked TODO. The SDK file next to it speaks the plugin protocol (MCP \
         resource `plugins://api`); leave it as it is.\n\n{build}\
         Check it with `vixen-studio-mcp plugin validate <folder>` (or the MCP tool `plugin_validate`), watch it run with \
         `vixen-studio-mcp plugin run <folder>`, then link the folder in Settings ▸ Plugins (Link Development Folder…, \
         or `plugin_link_dev`) so Studio runs it and reloads it when you save. `vixen-studio-mcp plugin pack <folder>` \
         makes the `.vixenplugin` to share.\n",
        template.main
    )
}

fn valid_name(name: &str) -> bool {
    (1..=80).contains(&name.chars().count())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || " ._()-".contains(c))
        && !name.trim().is_empty()
}

/// `plugin_scaffold`: writes the plugin to `<root>/dev/<folder>` (default: the last part of its id).
pub fn scaffold(root: &Path, id: &str, name: &str, language: &str, kind: &str, folder: Option<&str>) -> Result<Value, String> {
    let dev = root.join("dev");
    let mut out = create(&dev, id, name, language, kind, folder)?;
    let dir = out["folder"].as_str().unwrap_or_default().to_string();
    let mut steps = vec![format!("Edit {} (the TODO parts) and vixen-plugin.json with your own file tools.", out["main"].as_str().unwrap_or_default())];
    if let Some(build) = template(language).and_then(|t| t.build) {
        steps.push(format!("Build it: {build}. Every rebuild then reloads it once linked."));
    }
    steps.push(format!("plugin_validate {{\"path\": \"{dir}\"}} until every check passes."));
    steps.push(format!("plugin_link_dev {{\"path\": \"{dir}\", \"confirm\": true}} (needs Allow agents to manage plugins); Studio reloads it on every save."));
    steps.push("plugin_logs to read what it prints; the build_plugin prompt has the whole workflow.".to_string());
    out["nextSteps"] = json!(steps);
    Ok(out)
}

/// Writes a new plugin to `<parent>/<folder>` (created with 0700 if missing). Refuses an existing folder, so nothing
/// is ever overwritten. Returns `{folder, main, files, manifest}`.
pub fn create(parent: &Path, id: &str, name: &str, language: &str, kind: &str, folder: Option<&str>) -> Result<Value, String> {
    if !crate::manifest::valid_plugin_id(id) {
        return Err("id must be lowercase reverse-DNS, like io.github.someone.my-output".into());
    }
    if !valid_name(name) {
        return Err("name must be 1 to 80 letters, digits, spaces and . _ ( ) -".into());
    }
    let template = template(language).ok_or_else(|| format!("language must be one of {}", LANGUAGES.join(", ")))?;
    if !KINDS.contains(&kind) {
        return Err(format!("kind must be one of {}", KINDS.join(", ")));
    }
    let folder = folder.unwrap_or_else(|| id.rsplit('.').next().unwrap_or(id));
    let simple = !folder.is_empty() && folder.len() <= 64 && !folder.starts_with('.') && folder.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c));
    if !simple {
        return Err("folder must be a plain folder name (letters, digits, . _ -)".into());
    }
    private_dir(parent)?;
    let dir: PathBuf = parent.join(folder);
    if std::fs::symlink_metadata(&dir).is_ok() {
        return Err(format!("{} already exists; pick another folder", dir.display()));
    }
    let krate = crate_name(folder);
    let manifest = manifest(id, name, language, kind, &krate);
    Manifest::from_value(manifest.clone()).map_err(|why| format!("the template's manifest is invalid: {why}"))?;
    private_dir(&dir)?;
    let mut files = vec![
        ("vixen-plugin.json".to_string(), serde_json::to_string_pretty(&manifest).expect("serializes") + "\n", false),
        ("README.md".to_string(), readme(name, &template, kind), false),
    ];
    for f in &template.files {
        files.push((f.path.to_string(), fill(f.contents, id, name, &krate), f.executable));
    }
    for (path, contents, executable) in &files {
        let target = dir.join(path);
        if let Some(sub) = target.parent().filter(|p| *p != dir) {
            private_dir(sub)?;
        }
        write_atomic(&target, contents.as_bytes())?;
        if *executable {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o700)).map_err(|e| format!("{}: {e}", target.display()))?;
        }
    }
    Ok(json!({
        "folder": dir.to_string_lossy(),
        "main": template.main,
        "files": files.iter().map(|(n, _, _)| n).collect::<Vec<_>>(),
        "manifest": manifest,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_template_makes_a_valid_manifest_and_names_are_checked() {
        for language in LANGUAGES {
            for kind in KINDS {
                Manifest::from_value(manifest("com.example.thing", "Thing", language, kind, "thing")).unwrap();
            }
        }
        assert_eq!(manifest("com.example.thing", "Thing", "rust", "tools", "thing")["entry"]["command"], "bin/thing");
        assert!(valid_name("Lamp Output (LAN)"));
        assert!(!valid_name("Bad \"quote\""));
        assert!(!valid_name("{brace}"));
        assert_eq!(crate_name("Lamp_Output.2"), "lamp-output-2");
        assert_eq!(crate_name("2lamp"), "plugin-2lamp");
    }

    #[test]
    fn create_writes_each_language_and_never_overwrites() {
        let parent = std::env::temp_dir().join(format!("vixen-scaffold-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&parent);
        for language in LANGUAGES {
            let out = create(&parent, "com.example.lamp", "Lamp (LAN)", language, "controller", Some(language)).unwrap();
            let dir = PathBuf::from(out["folder"].as_str().unwrap());
            let main = std::fs::read_to_string(dir.join(out["main"].as_str().unwrap())).unwrap();
            assert!(main.contains("Lamp (LAN)") && !main.contains("{{"), "{language}: placeholders filled");
            assert!(create(&parent, "com.example.lamp", "Lamp", language, "tools", Some(language)).unwrap_err().contains("already exists"));
        }
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(parent.join("rust/build.sh")).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
        assert!(std::fs::read_to_string(parent.join("rust/Cargo.toml")).unwrap().contains("name = \"rust\""));
        std::fs::remove_dir_all(&parent).unwrap();
    }
}
