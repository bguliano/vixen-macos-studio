//! The boundary the Swift UI calls. Generated Swift bindings come from these exports.
//!
//! `core`, `sequence` and `kit` (the Rust side of VixenKit's shared controls) are always built. Each other module is one window area and may only depend on those and on the
//! crates below this one, never on another area, so it can be built alone.

uniffi::setup_scaffolding!();

pub mod core;
pub mod studio_agent; // STUDIO: shared agent/UI display commands and change observer
pub mod studio_previews; // STUDIO: the Previews pane's list, thumbnails and window titles
pub mod studio_schedule; // STUDIO: the Schedule pane's list
pub mod studio_lipsync; // STUDIO: the live Image Maps sheet's list, pictures and Image Mapper choices
mod naming;
mod wire_diagram;
pub mod kit;
mod fixture_editor;
pub mod sequence;
pub mod studio_window_chrome; // STUDIO: the sequencer window's title, song, unsaved and render state

#[cfg(feature = "admin")]
pub mod admin;
#[cfg(feature = "admin")]
mod web;
#[cfg(feature = "admin")]
pub mod studio_library; // STUDIO: the show window's sequence library
#[cfg(feature = "dialogs")]
pub mod dialogs;
#[cfg(feature = "effecteditor")]
pub mod effecteditor;
#[cfg(feature = "marks")]
pub mod marks;
#[cfg(feature = "marks")]
pub mod studio_phoneme_mappings; // STUDIO: the Text Conversion Failure sheet's word mapping on the shared undo
pub mod studio_face_mapping; // STUDIO: Edit Element Face Mapping's Face Setup visit as one step on the shared undo
pub mod studio_effect_gallery; // STUDIO: the sequencer's Effect Gallery thumbnails (effect_gallery, #62)
#[cfg(feature = "preview")]
pub mod preview;
#[cfg(feature = "scheduler")]
pub mod scheduler;
#[cfg(feature = "shell")]
pub mod sequencer;
#[cfg(feature = "setup")]
pub mod setup;
// STUDIO-BEGIN graph-access: the patching graph's VoiceOver labels and keyboard Connect To (R43)
#[cfg(feature = "setup")]
pub mod studio_graph_access;
// STUDIO-END graph-access
#[cfg(feature = "timeline")]
pub mod timeline;
#[cfg(feature = "tools")]
pub mod tools;
