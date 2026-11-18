//! Studio-only live command/event boundary; no Swift view ownership or styling.
use crate::core::{FfiError, VixenCore};
use std::sync::Arc;

#[derive(Clone, uniffi::Record)]
pub struct StudioChangeEvent {
    pub revision: u64,
    pub kind: String,
    pub command: String,
    pub agent: bool,
}
#[uniffi::export(callback_interface)]
pub trait StudioChangeObserver: Send + Sync {
    fn changed(&self, event: StudioChangeEvent);
}

#[uniffi::export]
impl VixenCore {
    /// Execute a UI command through the exact service MCP uses. JSON object arguments use stable UUIDs.
    /// Agent identity is never taken from JSON. MCP supplies it through the Rust-only adapter.
    pub fn studio_display_command(
        &self,
        command: String,
        arguments_json: String,
    ) -> Result<String, FfiError> {
        let args: serde_json::Value =
            serde_json::from_str(&arguments_json).map_err(|e| FfiError::new(e.to_string()))?;
        if !args.is_object() {
            return Err(FfiError::new("arguments must be an object"));
        }
        self.inner
            .studio_display
            .execute(&self.inner, &command, &args, false)
            .map(|v| v.to_string())
            .map_err(FfiError::new)
    }
    pub fn studio_stop_observing_display(&self) {
        self.inner.studio_display.unsubscribe();
    }
    pub fn studio_display_command_catalog(&self) -> String {
        serde_json::Value::Array(vixen_session::studio_display::command_tools()).to_string()
    }
    pub fn studio_display_snapshot(&self) -> String {
        self.inner.studio_display.snapshot(&self.inner).to_string()
    }
    pub fn studio_display_history(&self) -> String {
        self.inner.studio_display.history().to_string()
    }
    /// Callback is on the command thread, outside engine locks: dispatch to MainActor in Swift.
    pub fn studio_observe_display(&self, observer: Box<dyn StudioChangeObserver>) {
        self.inner.studio_display.subscribe(Arc::new(move |e| {
            observer.changed(StudioChangeEvent {
                revision: e.revision,
                kind: e.kind,
                command: e.command,
                agent: e.agent,
            })
        }));
    }
    pub fn studio_display_events(&self, after_revision: u64) -> Vec<StudioChangeEvent> {
        self.inner
            .studio_display
            .events(after_revision)
            .into_iter()
            .map(|e| StudioChangeEvent {
                revision: e.revision,
                kind: e.kind,
                command: e.command,
                agent: e.agent,
            })
            .collect()
    }
}

/// Installed once per AppCore. Startup failures remain visible through the explicit FFI method.
pub(crate) fn start_server(core: &Arc<vixen_session::app::AppCore>) -> Result<(), FfiError> {
    let mut runtime = core.studio_display.runtime.lock();
    if runtime.is_none() {
        *runtime = Some(Box::new(
            studio_mcp::socket::SocketServer::start(core)
                .map_err(|e| FfiError::new(e.to_string()))?,
        ));
    }
    Ok(())
}
#[uniffi::export]
impl VixenCore {
    pub fn studio_start_mcp(&self) -> Result<(), FfiError> {
        start_server(&self.inner)
    }
}
