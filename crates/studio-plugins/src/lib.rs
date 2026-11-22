//! Vixen Studio plugins: the host side.
//!
//! A plugin is a folder with a `vixen-plugin.json` manifest and an executable in any language. Studio runs each enabled
//! plugin as a child process and speaks JSON-RPC 2.0 with it over stdin/stdout, one message per line; the plugin logs to
//! stderr. A plugin crash never takes the show down: the host notices, restarts it with backoff and gives up after three
//! restarts in a minute. See `studio/PLUGINS-DESIGN.md` for the whole design and `studio/plugins/schema/` for the
//! manifest and protocol schemas.
//!
//! Same-user permissions are the trust boundary, as for the MCP bridge: a plugin runs as the user, not in a sandbox.
//! Declared permissions are enforced where Studio mediates (host API calls such as storage).
pub mod host;
pub mod logs;
pub mod manifest;
pub mod process;
pub mod profile;
pub mod registry;
pub mod rpc;
pub mod semver;

pub use host::{HostOptions, PluginHost, State};
pub use manifest::Manifest;

use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::OnceLock;

/// The plugin API this host implements. Additive changes bump the minor version, breaking ones the major.
pub const API_VERSION: semver::Version = semver::Version::new(1, 0, 0);
pub const HOST_VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "-studio");

/// What `initialize` tells plugins this host can do; later stages turn the rest on.
pub fn host_capabilities() -> Value {
    json!({"log": true, "storage": true, "events": ["liveOutput.changed"], "controllers": false, "tools": false})
}

/// `~/Library/Application Support/Vixen Studio/Plugins`.
pub fn default_root() -> Result<PathBuf, String> {
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
