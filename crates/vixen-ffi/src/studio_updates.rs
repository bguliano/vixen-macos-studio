//! Automatic updates (B49): the app's Sparkle updater reports its state here for the MCP tools `app_update_status` and
//! `app_update_check`, registers the hook that starts a check, and asks whether a show is running before it prompts or
//! relaunches.
use crate::core::{FfiError, VixenCore};

/// Starts a check for update information (never a download or install). Called on an MCP command thread: dispatch to
/// the main actor and return at once, or return an error naming why a check can't start now.
#[uniffi::export(callback_interface)]
pub trait StudioUpdateChecker: Send + Sync {
    fn check(&self) -> Option<String>;
}

/// The updater's state as a JSON object (see `app_update_status`); `finished_check` marks the report that ends a check.
#[uniffi::export]
pub fn studio_updates_publish(status_json: String, finished_check: bool) -> Result<(), FfiError> {
    let status: serde_json::Value = serde_json::from_str(&status_json).map_err(|e| FfiError::new(e.to_string()))?;
    vixen_session::studio_updates::publish(status, finished_check).map_err(FfiError::new)
}

/// The app's check hook for `app_update_check`; replaces any earlier one.
#[uniffi::export]
pub fn studio_updates_register(checker: Box<dyn StudioUpdateChecker>) {
    vixen_session::studio_updates::register_check(Box::new(move || match checker.check() {
        Some(reason) => Err(reason),
        None => Ok(()),
    }));
}

#[uniffi::export]
impl VixenCore {
    /// Whether a show is running: JSON {playing, liveOutput, busy}. The updater keeps its prompt and relaunch for later
    /// while busy.
    pub fn studio_show_activity(&self) -> String {
        vixen_session::studio_updates::show_activity(&self.inner).to_string()
    }
}
