//! The editing session behind the sequencer window: the effect table, undo, timeline rows and thumbnails.

pub mod app;
pub mod studio_display; // STUDIO: shared live display commands and undo
pub mod studio_controller_network; // STUDIO: the controller inspector's network settings and status
pub mod studio_color; // STUDIO: RGB / RGBW / Single colour handling for templates, the Display pane and MCP
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

// STUDIO-BEGIN authoring-modules: shared sequence, parameter and offscreen preview commands
pub mod studio_sequence;
pub mod studio_params;
pub mod studio_preview;
// STUDIO-END authoring-modules

pub mod studio_live; // STUDIO: explicit locally gated agent hardware operations

pub mod studio_show; // STUDIO: shared show and disabled schedule authoring

pub mod studio_heartbeat; // STUDIO: memory heartbeat and unclean-exit report in the logs

pub mod studio_diagnostics; // STUDIO: reusable agent and UI profile diagnostics

pub mod studio_paths; // STUDIO: protect profile output and backup boundaries

pub mod studio_props; // STUDIO: shared native preview prop commands

pub mod studio_schema; // STUDIO: validate the shared published command schemas
