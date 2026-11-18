//! The editing session behind the sequencer window: the effect table, undo, timeline rows and thumbnails.

pub mod app;
pub mod studio_display; // STUDIO: shared live display commands and undo
pub mod archive;
pub mod boot;
pub mod compare;
mod collation;
pub mod cpu;
pub mod dialogs;
pub mod effect_editor;
pub mod element_mapper;
pub mod execution;
pub mod export;
pub mod export_dialog;
pub mod export_wizard;
pub mod instrumentation;
pub mod library_links;
pub mod live;
pub mod web;
pub mod logging;
pub mod marks;
pub mod modules;
pub mod playback;
pub mod preview;
pub mod preview_fixtures;
pub mod preview_design;
pub mod preview_devices;
#[cfg(feature = "prop-import")]
pub mod preview_import;
pub mod preview_live;
pub mod render;
pub mod scheduler;
pub mod sequence;
pub mod sequence_package;
pub mod sequencer;
pub mod setup;
pub mod show;
pub mod song_inventory;
pub mod shows;
pub mod thumbnail;
pub mod timeline;
pub mod tools;
pub mod transport;
pub mod undo;
pub mod updates;
pub mod version;
pub mod vixen2;

use thiserror::Error;

/// The renderer and scene types, for the bindings, which don't otherwise depend on those crates.
pub mod render_support {
    pub use vixen_preview::Scene;
    pub use vixen_render::LiveRenderer;

    /// What the preview designer's bindings use.
    pub mod design {
        pub use vixen_preview::session::apply_location_updates;
        pub use vixen_preview::tools::{window_is_recoverable, Rect};
        pub use vixen_preview::{
            fixture_nodes, valid_template_name, Change, Cursor, DesignSession, ElementLinker, Key, MenuItem, Modifiers, MouseButton, PropertyKind, PropertyValue, ShapeId, ShapeKind, ShapeProperty, TemplateStore, Tool, ToolbarState,
        };
    }
}

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("{0}")]
    Io(#[from] vixen_io::IoError),
    #[error("{0}")]
    Export(#[from] vixen_export::ExportError),
    #[error("{0}")]
    Other(String),
}
