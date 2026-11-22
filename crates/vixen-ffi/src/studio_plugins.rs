//! Studio: the plugin host for Settings ▸ Plugins (`studio/PLUGINS-DESIGN.md`). One host per app process, opened on
//! first use at `~/Library/Application Support/Vixen Studio/Plugins`; nothing starts until `studio_plugins_start_enabled`.
//! Every call returns the JSON the matching MCP tool will return.
use crate::core::FfiError;
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
