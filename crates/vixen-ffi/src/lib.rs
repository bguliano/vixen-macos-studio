//! The boundary the Swift UI calls. Generated Swift bindings come from these exports.
//!
//! `core`, `sequence` and `kit` (the Rust side of VixenKit's shared controls) are always built. Each other module is one window area and may only depend on those and on the
//! crates below this one, never on another area, so it can be built alone.

uniffi::setup_scaffolding!();

pub mod core;
pub mod studio_agent; // STUDIO: shared agent/UI display commands and change observer
pub mod studio_previews; // STUDIO: the Previews pane's list, thumbnails and window titles
pub mod studio_schedule; // STUDIO: the Schedule pane's list
mod naming;
mod wire_diagram;
pub mod kit;
mod fixture_editor;
pub mod sequence;

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
#[cfg(feature = "preview")]
pub mod preview;
#[cfg(feature = "scheduler")]
pub mod scheduler;
#[cfg(feature = "shell")]
pub mod sequencer;
#[cfg(feature = "setup")]
pub mod setup;
#[cfg(feature = "timeline")]
pub mod timeline;
#[cfg(feature = "tools")]
pub mod tools;
