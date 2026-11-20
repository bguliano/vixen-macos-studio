//! Studio: the show window's Previews pane. Its edits are display commands (`studio_display_command`), shared with agents;
//! this adds the pane's list and thumbnails, and the live windows' titles.
use crate::core::{FfiError, VixenCore};

#[uniffi::export]
impl VixenCore {
    /// The pane's rows as JSON: `{revision, liveOutput, items: [{id, name, title, enabled, running, props, lights, linkedLights, camera}]}`.
    pub fn studio_preview_list(&self) -> String {
        vixen_session::studio_previews::list(&self.inner).to_string()
    }

    /// A PNG of the preview's layout, at most `max_width` × `max_height` pixels. Call it off the main actor.
    pub fn studio_preview_thumbnail(&self, id: String, max_width: u32, max_height: u32) -> Result<Vec<u8>, FfiError> {
        let id = id.parse().map_err(|_| FfiError::new("Invalid preview id"))?;
        vixen_session::studio_previews::thumbnail(&self.inner, id, max_width, max_height).map_err(FfiError::new)
    }
}

/// A preview's window title: its name without an "(OpenGL)" tag.
#[uniffi::export]
pub fn studio_preview_window_title(name: String) -> String {
    vixen_session::studio_previews::title(&name)
}
