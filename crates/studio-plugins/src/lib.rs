//! Vixen Studio plugins: the host side.
//!
//! A plugin is a folder with a `vixen-plugin.json` manifest and an executable in any language. Studio runs each enabled
//! plugin as a child process and speaks JSON-RPC 2.0 with it over stdin/stdout, one message per line; the plugin logs to
//! stderr. A plugin crash never takes the show down: the host notices, restarts it with backoff and gives up after three
//! restarts in a minute. See `studio/PLUGINS-DESIGN.md` for the whole design and `studio/plugins/schema/` for the
//! manifest and protocol schemas.
//!
//! Same-user permissions are the trust boundary, as for the MCP bridge: a plugin runs as the user. Declared permissions
//! are enforced where Studio mediates (host API calls such as storage), and the plugin process runs under a macOS
//! sandbox profile derived from them (`sandbox`), which narrows files and network but isn't a security boundary.
pub mod cli;
pub mod controllers;
pub mod display;
pub mod harness;
pub mod host;
pub mod install;
pub mod logs;
pub mod manifest;
pub mod mcp;
pub mod pack;
pub mod process;
pub mod profile;
pub mod registry;
pub mod rpc;
pub mod sandbox;
pub mod scaffold;
pub mod schema;
pub mod semver;
pub mod tools;
pub mod ui;

pub use host::{DisplaySource, HostOptions, PluginHost, State};
pub use manifest::Manifest;

use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

static AGENTS_MAY_MANAGE: AtomicBool = AtomicBool::new(false);

/// The local "Allow agents to manage plugins" switch (Settings ▸ Plugins). Off at every launch and only the app's own UI
/// turns it on: there is no MCP setter. Installing, enabling, linking, reloading and removing plugins through MCP need it.
pub fn agents_may_manage() -> bool {
    AGENTS_MAY_MANAGE.load(Ordering::SeqCst)
}

pub fn set_agents_may_manage(allowed: bool) {
    AGENTS_MAY_MANAGE.store(allowed, Ordering::SeqCst);
}

/// The plugin API this host implements. Additive changes bump the minor version, breaking ones the major.
pub const API_VERSION: semver::Version = semver::Version::new(1, 0, 0);
pub const HOST_VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "-studio");

/// What `initialize` tells plugins this host can do; later stages turn the rest on.
pub fn host_capabilities() -> Value {
    json!({"log": true, "storage": true, "events": ["liveOutput.changed"], "controllers": true, "frameEncodings": ["u8", "json", "u16"], "tools": true, "display": true})
}

/// `~/Library/Application Support/Vixen Studio/Plugins`, or `VIXEN_STUDIO_PLUGINS` when set (an absolute folder: UI tests
/// use a throwaway one, so they never start the user's plugins).
pub fn default_root() -> Result<PathBuf, String> {
    if let Some(folder) = std::env::var_os("VIXEN_STUDIO_PLUGINS").map(PathBuf::from).filter(|p| p.is_absolute()) {
        return Ok(folder);
    }
    let home = std::env::var_os("HOME").filter(|h| !h.is_empty()).ok_or("HOME is not set")?;
    Ok(PathBuf::from(home).join("Library/Application Support/Vixen Studio/Plugins"))
}

static SHARED: OnceLock<Result<PluginHost, String>> = OnceLock::new();

/// The app's one plugin host, opened at [`default_root`] on first use. Opening it starts no plugin.
pub fn shared() -> Result<&'static PluginHost, String> {
    SHARED
        .get_or_init(|| default_root().and_then(|root| PluginHost::new(&root, HostOptions::default())))
        .as_ref()
        .map_err(Clone::clone)
}

/// Stops every plugin of the shared host, if it was ever opened. Call when the app quits.
pub fn shutdown_shared() {
    if let Some(Ok(host)) = SHARED.get() {
        host.shutdown_all();
    }
}
