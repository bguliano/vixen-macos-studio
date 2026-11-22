//! Studio: the plugin host for Settings ▸ Plugins (`studio/PLUGINS-DESIGN.md`). One host per app process, opened on
//! first use at `~/Library/Application Support/Vixen Studio/Plugins`; nothing starts until `studio_plugins_start_enabled`.
//! Every call returns the JSON the matching MCP tool will return.
use crate::core::{FfiError, VixenCore};
use std::path::Path;
use studio_plugins::logs::Level;

fn host() -> Result<&'static studio_plugins::PluginHost, FfiError> {
    studio_plugins::shared().map_err(FfiError::new)
}

/// Makes installed plugins' controller types load, save and run like built-in ones. Called before a profile loads, so
/// its plugin controllers compile into the output graph; a controller starts its enabled plugin when it starts itself.
pub(crate) fn register_controllers() {
    match studio_plugins::shared() {
        Ok(host) => vixen_output::modules::studio_external::register(Some(host.controllers())),
        Err(why) => vixen_session::logging::error("Vixen.Studio.Plugins", "Could not open the plugin folder", Some(&why)),
    }
}

/// `{apiVersion, folder, muted, plugins: [{id, name, version, source, enabled, state, folder, pid?, uptimeMs?, restartCount,
/// problem?, lastError?, nextRestartMs?, controllerTypes, tools, commands, events}]}`.
#[uniffi::export]
pub fn studio_plugins_list() -> Result<String, FfiError> {
    host()?.refresh();
    Ok(host()?.list().to_string())
}

/// One plugin with its manifest, permissions, negotiated API version, data folder and log file.
#[uniffi::export]
pub fn studio_plugin_info(id: String) -> Result<String, FfiError> {
    host()?.info(&id).map(|v| v.to_string()).map_err(FfiError::new)
}

/// The newest `limit` log entries at or above `level` (debug, info, warn, error): `{id, logFile, entries: [{seq, timeMs,
/// level, source: stderr|plugin|host, message}]}`.
#[uniffi::export]
pub fn studio_plugin_logs(id: String, limit: u32, level: String) -> Result<String, FfiError> {
    let level = Level::parse(&level).ok_or_else(|| FfiError::new("level must be debug, info, warn or error"))?;
    host()?.logs(&id, limit as usize, level).map(|v| v.to_string()).map_err(FfiError::new)
}

/// Enables (and starts) or disables (and stops) a plugin; remembered across launches.
#[uniffi::export]
pub fn studio_plugin_set_enabled(id: String, enabled: bool) -> Result<String, FfiError> {
    host()?.set_enabled(&id, enabled).map(|v| v.to_string()).map_err(FfiError::new)
}

/// Restarts a plugin with its manifest read again, clearing a crashed state.
#[uniffi::export]
pub fn studio_plugin_reload(id: String) -> Result<String, FfiError> {
    host()?.reload(&id).map(|v| v.to_string()).map_err(FfiError::new)
}

/// Links a development folder (absolute path, not a symlink) and starts its plugin; edits to it restart the plugin.
#[uniffi::export]
pub fn studio_plugin_link_dev(path: String) -> Result<String, FfiError> {
    host()?.link_dev(Path::new(&path)).map(|v| v.to_string()).map_err(FfiError::new)
}

/// Removes a development link; the folder stays.
#[uniffi::export]
pub fn studio_plugin_unlink_dev(id: String) -> Result<(), FfiError> {
    host()?.unlink_dev(&id).map_err(FfiError::new)
}

/// Starts every enabled plugin in the background; returns the ids being started as a JSON array.
#[uniffi::export]
pub fn studio_plugins_start_enabled() -> Result<String, FfiError> {
    Ok(serde_json::json!(host()?.start_enabled()).to_string())
}

/// Tells plugins whether real output may flow (Live Output on and not muted).
#[uniffi::export]
pub fn studio_plugins_set_muted(muted: bool) -> Result<(), FfiError> {
    host()?.set_muted(muted);
    Ok(())
}

/// The plugins the show at `profile` needs and whether each is ready, for the missing-plugin banner.
#[uniffi::export]
pub fn studio_plugins_required(profile: String) -> Result<String, FfiError> {
    host()?.requirements(Path::new(&profile)).map(|v| v.to_string()).map_err(FfiError::new)
}

/// The local "Allow agents to manage plugins" switch in Settings ▸ Plugins: off at every launch, and only this (the
/// app's own UI) turns it on. MCP's plugin_install/uninstall/enable/disable/link_dev/unlink_dev/reload need it.
#[uniffi::export]
pub fn studio_plugins_agents_may_manage() -> bool {
    studio_plugins::agents_may_manage()
}

#[uniffi::export]
pub fn studio_plugins_set_agents_may_manage(allowed: bool) {
    studio_plugins::set_agents_may_manage(allowed);
}

/// Installs a plugin folder or `.vixenplugin` package (absolute path) and starts it; the JSON of `plugin_install`.
#[uniffi::export]
pub fn studio_plugin_install(path: String) -> Result<String, FfiError> {
    host()?.install(Path::new(&path)).map(|v| v.to_string()).map_err(FfiError::new)
}

/// Stops and removes an installed plugin; its data and log stay. The JSON of `plugin_uninstall`.
#[uniffi::export]
pub fn studio_plugin_uninstall(id: String) -> Result<String, FfiError> {
    host()?.uninstall(&id).map(|v| v.to_string()).map_err(FfiError::new)
}

/// Stops every plugin. Call when the app quits.
#[uniffi::export]
pub fn studio_plugins_shutdown() {
    studio_plugins::shutdown_shared();
}

/// The inspector form for a plugin controller type's `configSchema` (JSON), as `studio_plugins::ui::form` describes it.
#[uniffi::export]
pub fn studio_plugin_config_form(schema: String) -> String {
    let schema: serde_json::Value = serde_json::from_str(&schema).unwrap_or(serde_json::Value::Null);
    studio_plugins::ui::form(&schema).to_string()
}

/// Every problem with `config` against `schema` (both JSON), as `[{field, message}]`: the inspector's live check, the
/// same one `controller_configure` makes before it applies a configuration.
#[uniffi::export]
pub fn studio_plugin_check_config(schema: String, config: String) -> String {
    let parse = |text: &str| serde_json::from_str::<serde_json::Value>(text);
    match (parse(&schema), parse(&config)) {
        (Ok(schema), Ok(config)) => serde_json::Value::Array(studio_plugins::schema::validate(&schema, &config).iter().map(|e| e.to_json()).collect()).to_string(),
        _ => serde_json::json!([{"field": "/", "message": "isn't valid JSON"}]).to_string(),
    }
}

/// What installing a folder or `.vixenplugin` (absolute path) would install, without installing it: for the install
/// sheet's permissions. Nothing is kept.
#[uniffi::export]
pub fn studio_plugin_inspect_package(path: String) -> Result<String, FfiError> {
    host()?.inspect_package(Path::new(&path)).map(|v| v.to_string()).map_err(FfiError::new)
}

/// The commands plugins contribute to the Plugins menu and ⌘K: `[{pluginId, pluginName, id, title, tool, enabled}]`.
#[uniffi::export]
pub fn studio_plugin_commands() -> Result<String, FfiError> {
    host()?.refresh();
    Ok(host()?.commands().to_string())
}

/// Runs a contributed command (its tool, no arguments), starting the plugin if needed: `{content, isError}`. Blocks up to
/// the tool timeout, so call it off the main actor.
#[uniffi::export]
pub fn studio_plugin_run_command(plugin_id: String, command_id: String) -> Result<String, FfiError> {
    host()?.run_command(&plugin_id, &command_id).map(|v| v.to_string()).map_err(FfiError::new)
}

#[uniffi::export]
impl VixenCore {
    /// Keeps the plugin host in step with the open show, called by the show window when it appears and on its poll: tells
    /// it the profile, how to read its display for `display.snapshot`, and whether real output may flow (Live Output on
    /// and Studio not muted), starts the enabled plugins the first time, and returns `studio_plugins_required` for the
    /// show (the missing-plugin banner).
    pub fn studio_plugins_sync(&self) -> Result<String, FfiError> {
        static STARTED: std::sync::Once = std::sync::Once::new();
        let host = host()?;
        let root = self.inner.paths.root().to_path_buf();
        let name = self.inner.profile_name().unwrap_or_else(|| root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
        host.set_profile(Some((name, root.to_string_lossy().into_owned())));
        // Weak, so a closed show isn't kept alive by the host: its snapshot then answers "no show is open".
        let core = std::sync::Arc::downgrade(&self.inner);
        host.set_display(Some(std::sync::Arc::new(move |params: &serde_json::Value| {
            core.upgrade().map(|core| vixen_session::studio_plugin_display::snapshot(&core, params)).ok_or_else(|| "no show is open".to_string())
        })));
        let muted_by_launch = std::env::var_os("VIXEN_MUTE").is_some() || std::env::var_os("VIXEN_UI_TEST").is_some();
        host.set_muted(muted_by_launch || !self.inner.execution().is_open());
        STARTED.call_once(|| {
            host.start_enabled();
        });
        host.requirements(&root).map(|v| v.to_string()).map_err(FfiError::new)
    }
}
