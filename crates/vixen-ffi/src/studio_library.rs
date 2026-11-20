//! Studio: the show summary and sequence library shown in the show window. The library itself (header reading and
//! each sequence's state) lives in `vixen_session::studio_sequence::library`, shared with the MCP tools.

use crate::core::VixenCore;

/// The show at a glance, for the show window's sidebar and overview.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct StudioShowOverview {
    /// Top-level items in the display tree (props and groups as people named them).
    pub top_level_items: u32,
    /// Elements that carry light: tree nodes with an element and no children.
    pub elements: u32,
    pub groups: u32,
    pub controllers: u32,
    pub outputs: u32,
    pub previews: u32,
}

#[uniffi::export]
impl VixenCore {
    pub fn studio_show_overview(&self) -> StudioShowOverview {
        let system = self.inner.system.read();
        let tree = &system.tree;
        let (mut elements, mut groups) = (0u32, 0u32);
        for (_, node) in tree.all_nodes() {
            if !node.children.is_empty() {
                groups += 1;
            } else if node.element_id.is_some() {
                elements += 1;
            }
        }
        StudioShowOverview {
            top_level_items: tree.roots.len() as u32,
            elements,
            groups,
            controllers: system.controllers.len() as u32,
            outputs: system.controllers.iter().map(|c| c.outputs.len() as u32).sum(),
            previews: system.previews.len() as u32,
        }
    }
}

/// One sequence file in the library, with its state for the badges (`vixen_session::studio_sequence::library`).
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct StudioSequenceSummary {
    pub path: String,
    /// The file name without its extension.
    pub name: String,
    /// Seconds since 1970, from the file's modification date.
    pub modified: f64,
    pub size_bytes: u64,
    /// The sequence length in milliseconds, when the header states it.
    pub length_ms: Option<f64>,
    /// The audio file's name (not its full path, which may be a Windows path), when the sequence has audio.
    pub audio_name: Option<String>,
    /// The profile's Media folder lacks that audio file, so the sequence plays silent.
    pub audio_missing: bool,
    /// Open in this Studio session, in a window or by an agent.
    pub open: bool,
    /// Open with changes not saved yet.
    pub unsaved: bool,
    /// An open sequence's effects and how many are rendered; None when closed.
    pub effects: Option<u32>,
    pub rendered: Option<u32>,
    /// `Export/<name>.fseq` is newer than the saved file.
    pub fseq_current: bool,
}

#[uniffi::export]
impl VixenCore {
    /// The sequences in the profile's Sequence folder, newest first, as the library_list command lists them. Read
    /// directly rather than through the command gate, so a long export never stalls the show window.
    pub fn studio_sequence_library(&self) -> Vec<StudioSequenceSummary> {
        vixen_session::studio_sequence::library::list(&self.inner)
            .into_iter()
            .map(|e| StudioSequenceSummary {
                path: e.path,
                name: e.name,
                modified: e.modified,
                size_bytes: e.size_bytes,
                length_ms: e.length_ms,
                audio_name: e.audio_name,
                audio_missing: e.audio_missing,
                open: e.open,
                unsaved: e.unsaved,
                effects: e.effects,
                rendered: e.rendered,
                fseq_current: e.fseq_current,
            })
            .collect()
    }
}
