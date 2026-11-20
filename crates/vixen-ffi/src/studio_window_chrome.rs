//! Studio: a sequencer window's title bar and toolbar state (`vixen_session::studio_sequence::window_chrome`), the
//! same fields `sequence_status` gives agents.

use crate::sequence::SequencerSession;

#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct StudioWindowChrome {
    /// The sequence's name, or "Untitled".
    pub title: String,
    /// The song, "<song> (audio missing)", or "No audio".
    pub subtitle: String,
    /// Changes not saved yet: the title bar's edited dot.
    pub unsaved: bool,
    pub effects: u32,
    pub rendered: u32,
    /// Effects are still waiting for a render.
    pub rendering: bool,
    pub rendering_percent: u32,
    /// The render ring's tooltip.
    pub render_help: String,
}

#[uniffi::export]
impl SequencerSession {
    pub fn studio_window_chrome(&self) -> StudioWindowChrome {
        let c = vixen_session::studio_sequence::window_chrome::window_chrome(&self.session.lock(), &self.core.paths.media_dir());
        StudioWindowChrome {
            title: c.title,
            subtitle: c.subtitle,
            unsaved: c.unsaved,
            effects: c.effects,
            rendered: c.rendered,
            rendering: c.rendering,
            rendering_percent: c.rendering_percent,
            render_help: c.render_help,
        }
    }
}
