//! The macOS sandbox profile a plugin runs under, derived from its manifest's permissions.
//!
//! Studio launches each plugin through `sandbox-exec` with a generated profile (SBPL, the language of
//! `/usr/share/sandbox/*.sb`). It starts from `(allow default)` so interpreters (python3, node, a Rust binary) work as
//! they would outside, then takes away what the manifest doesn't ask for:
//!
//! - **Files.** Reading file contents inside the home folder is refused, except the plugin's own folder, its data
//!   folder and the folder of the program that runs it (an interpreter installed under the home folder). Names and
//!   sizes stay visible, so path lookups (Python's `site`, Node's `node_modules` walk) behave. Writing is refused
//!   everywhere except the temporary folders, `/dev/null`-style devices and, with `"files": ["plugin-data"]`, the data
//!   folder.
//! - **Network.** Refused unless `permissions.network` has rules. A rule allows its protocol on its port (`udp:lan`
//!   and `tcp:lan` any port; `http`/`ws` port 80 and `https`/`wss` 443 unless the rule names one) plus name lookups.
//!   SBPL can't filter by host name or subnet, so a rule opens its port to any address: the walls are protocols and
//!   ports, not hosts.
//! - **Serial.** `/dev/cu.*` and `/dev/tty.*` are writable only with `"serial": true` (reading them is a device read,
//!   allowed either way, which is harmless without the write side).
//!
//! The same-user trust boundary stays: this narrows what a plugin can do by accident or on a bad day, it isn't a
//! promise against a determined author (mach services the user can reach stay reachable, for instance).
use crate::manifest::Manifest;
use std::path::{Path, PathBuf};

/// The launcher. Missing (it ships with every macOS so far) means plugins start unsandboxed.
pub const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

/// Set to `off` in Studio's environment to start plugins without the sandbox (diagnosing a plugin that only fails
/// inside it). Plugins never see Studio's environment, so a plugin can't set it.
pub const DISABLE_ENV: &str = "VIXEN_PLUGIN_SANDBOX";

/// Whether plugins start under the sandbox: `sandbox-exec` exists and `VIXEN_PLUGIN_SANDBOX` isn't `off`.
pub fn enabled() -> bool {
    std::env::var(DISABLE_ENV).map_or(true, |v| !v.eq_ignore_ascii_case("off")) && Path::new(SANDBOX_EXEC).is_file()
}

/// The folders a profile is about. Each is canonicalized when the profile is made: the sandbox matches real paths
/// (`/tmp` is `/private/tmp`), so an uncanonical path would never match.
pub struct Paths<'a> {
    pub plugin_dir: &'a Path,
    pub data_dir: &'a Path,
    /// The resolved `entry.command`.
    pub program: &'a Path,
    pub home: Option<PathBuf>,
    pub temp_dir: PathBuf,
}

impl<'a> Paths<'a> {
    /// The paths for a launch, with the home and temporary folders of this process.
    pub fn for_launch(plugin_dir: &'a Path, data_dir: &'a Path, program: &'a Path) -> Paths<'a> {
        Paths { plugin_dir, data_dir, program, home: std::env::var_os("HOME").map(PathBuf::from), temp_dir: std::env::temp_dir() }
    }
}

fn real(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// An SBPL string literal.
fn quote(path: &Path) -> String {
    let text = path.to_string_lossy();
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        if c == '"' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// Where the program lives: `…/bin/python3` gives `…` (its install prefix, with `lib/`), anything else its folder.
fn program_root(program: &Path) -> PathBuf {
    let program = real(program);
    let folder = program.parent().map(Path::to_path_buf).unwrap_or_default();
    match folder.file_name().and_then(|n| n.to_str()) {
        Some("bin") => folder.parent().map(Path::to_path_buf).unwrap_or(folder),
        _ => folder,
    }
}

/// The SBPL filters one `permissions.network` rule allows, as `(operation filter)` lines.
fn network_rules(rule: &str) -> Vec<String> {
    let Some((protocol, target)) = rule.split_once(':') else {
        return Vec::new();
    };
    let port = match target.rsplit_once(':') {
        Some((_, port)) => port.to_string(),
        None if target == "lan" => "*".to_string(),
        None => match protocol {
            "http" | "ws" => "80".to_string(),
            "https" | "wss" => "443".to_string(),
            _ => "*".to_string(),
        },
    };
    if protocol == "udp" {
        vec![
            format!("(allow network-outbound (remote udp \"*:{port}\"))"),
            // Replies come back to a socket the plugin bound (explicitly or by its first send).
            "(allow network-bind (local udp \"*:*\"))".to_string(),
            "(allow network-inbound (local udp \"*:*\"))".to_string(),
        ]
    } else {
        vec![format!("(allow network-outbound (remote tcp \"*:{port}\"))")]
    }
}

/// The profile for `manifest` launched with `paths`. For one operation the last matching rule wins, and a rule naming
/// an operation beats a wildcard one, so each section refuses broadly and then allows the same operation narrowly.
pub fn profile(manifest: &Manifest, paths: &Paths<'_>) -> String {
    let permissions = &manifest.permissions;
    let plugin_dir = real(paths.plugin_dir);
    let data_dir = real(paths.data_dir);
    let mut out = vec![
        format!(";; Vixen Studio sandbox for {} {} (generated from its permissions)", manifest.id, manifest.version),
        "(version 1)".to_string(),
        "(allow default)".to_string(),
        String::new(),
        ";; Files: no reading inside the home folder except the plugin's own folders and its program.".to_string(),
    ];
    if let Some(home) = &paths.home {
        out.push(format!("(deny file-read-data (subpath {}))", quote(&real(home))));
    }
    // The same operation as the refusal: an allow for file-read* would lose to the narrower file-read-data deny.
    let mut readable = vec![plugin_dir.clone(), data_dir.clone(), program_root(paths.program), real(&paths.temp_dir)];
    readable.dedup();
    for dir in &readable {
        out.push(format!("(allow file-read-data (subpath {}))", quote(dir)));
    }
    out.push(";; Writing only to the temporary folders, harmless devices and (permissions.files) the data folder.".to_string());
    out.push("(deny file-write*)".to_string());
    let mut writable = vec![real(&paths.temp_dir), PathBuf::from("/private/tmp"), PathBuf::from("/private/var/tmp")];
    if permissions.files.iter().any(|f| f == "plugin-data") {
        writable.insert(0, data_dir);
    }
    writable.dedup();
    let writable: Vec<String> = writable.iter().map(|dir| format!("(subpath {})", quote(dir))).collect();
    out.push(format!("(allow file-write* {})", writable.join(" ")));
    out.push(
        "(allow file-write* (literal \"/dev/null\") (literal \"/dev/zero\") (literal \"/dev/dtracehelper\") (regex #\"^/dev/fd/\") (regex #\"^/dev/tty$\"))"
            .to_string(),
    );
    if permissions.serial {
        out.push(";; Serial ports (permissions.serial).".to_string());
        out.push("(allow file-write* (regex #\"^/dev/cu\\.\") (regex #\"^/dev/tty\\.\"))".to_string());
    }
    out.push(String::new());
    out.push(";; Network: only what permissions.network names.".to_string());
    out.push("(deny network*)".to_string());
    if !permissions.network.is_empty() {
        out.push("(allow network-outbound (remote unix-socket (path-literal \"/private/var/run/mDNSResponder\")))".to_string());
        let mut rules: Vec<String> = permissions.network.iter().flat_map(|rule| network_rules(rule)).collect();
        rules.sort();
        rules.dedup();
        out.extend(rules);
    }
    out.push(String::new());
    out.join("\n")
}

/// The command line that runs `program` under `profile`: `sandbox-exec -p PROFILE PROGRAM`, arguments to follow.
pub fn wrap(profile: &str, program: &Path) -> (PathBuf, Vec<std::ffi::OsString>) {
    (PathBuf::from(SANDBOX_EXEC), vec!["-p".into(), profile.into(), program.as_os_str().to_os_string()])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn manifest(permissions: serde_json::Value) -> Manifest {
        let text = json!({
            "id": "com.example.sandbox", "name": "Sandbox", "version": "1.0.0",
            "apiVersion": "^1.0.0", "entry": {"command": "python3"}, "permissions": permissions,
        });
        Manifest::parse(&text.to_string()).expect("valid manifest")
    }

    fn paths() -> Paths<'static> {
        Paths {
            plugin_dir: Path::new("/nonexistent/plugin"),
            data_dir: Path::new("/nonexistent/data"),
            program: Path::new("/nonexistent/prefix/bin/python3"),
            home: Some(PathBuf::from("/nonexistent/home")),
            temp_dir: PathBuf::from("/nonexistent/tmp"),
        }
    }

    #[test]
    fn no_permissions_means_no_network_and_no_serial() {
        let profile = profile(&manifest(json!({})), &paths());
        assert!(profile.contains("(deny network*)"));
        assert!(!profile.contains("allow network"));
        assert!(!profile.contains("/dev/cu"));
        assert!(profile.contains("(deny file-read-data (subpath \"/nonexistent/home\"))"));
        assert!(profile.contains("(allow file-read-data (subpath \"/nonexistent/plugin\"))"));
        assert!(profile.contains("(allow file-read-data (subpath \"/nonexistent/prefix\"))"), "the interpreter's prefix is readable");
        assert!(!profile.contains("(allow file-write* (subpath \"/nonexistent/data\")"), "no files permission, no writing");
        let profile = super::profile(&manifest(json!({"files": ["plugin-data"]})), &paths());
        assert!(profile.contains("(allow file-write* (subpath \"/nonexistent/data\")"));
    }

    #[test]
    fn network_rules_become_ports() {
        let profile = profile(
            &manifest(json!({"network": ["udp:lan", "https:api.example.com", "tcp:hub.local:8080", "ws:lights.local"], "serial": true})),
            &paths(),
        );
        for line in [
            "(allow network-outbound (remote udp \"*:*\"))",
            "(allow network-inbound (local udp \"*:*\"))",
            "(allow network-outbound (remote tcp \"*:443\"))",
            "(allow network-outbound (remote tcp \"*:8080\"))",
            "(allow network-outbound (remote tcp \"*:80\"))",
            "mDNSResponder",
            "/dev/cu",
        ] {
            assert!(profile.contains(line), "missing {line} in\n{profile}");
        }
    }

    #[test]
    fn paths_are_quoted() {
        assert_eq!(quote(Path::new("/a \"b\\c")), "\"/a \\\"b\\\\c\"");
    }
}
