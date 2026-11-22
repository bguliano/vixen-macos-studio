//! Reads and writes the Vixen 3 data files: sequences, the system configuration, the module store, the
//! curve, gradient and colour libraries, and the application data.
//!
//! Files are read through versioned migrations ported from the C#, typed where the app needs types, and kept
//! as `XElement`s where it does not. Saving writes everything back, so a file this port opens and saves is one
//! real Vixen on Windows can still open. See `docs/PORTING_GUIDE.md` for the general rules.
//!
//! Loading never writes. C#'s migration backups (`Song.tim.8`) are made when a migrated file is saved over its
//! original, and a member an older file lacks stays out of the file until its value changes.
//!
//! - Sequences: [`load_sequence`], [`save_sequence`], [`new_sequence`], [`TimedSequence`].
//! - System files: [`load_system`], [`save_system`], [`SystemData`], [`ModuleStore`].
//! - Libraries in the module store: [`curve_library`], [`gradient_library`], [`color_sets`] and their setters.
//! - Application data: [`load_application_data`], [`save_application_data`].
//! - Settings beside the profiles: [`settings`], [`profiles`], [`lock`], [`audio_prefs`].
//! - Effect data from any table: [`params`].

pub mod appdata;
pub mod audio_prefs;
pub mod datetime;
pub mod dcs;
mod emit_default;
pub mod error;
mod fidelity;
pub mod file;
mod fixups;
pub mod library;
pub mod effect_defaults;
pub mod lock;
pub mod marks_files;
pub mod module_store;
pub mod params;
pub mod paths;
pub mod profiles;
pub mod sequence;
mod sequence_migrate;
pub mod settings;
mod studio_effect_members; // STUDIO: effect members only Studio has, kept out of untouched files (#67)
pub mod system;
pub mod values;
pub mod xml;

pub use appdata::{
    load_application_data, save_application_data, ApplicationData, FilterShapePosition, RecentSequence, MAX_RECENT_SEQUENCES,
};
pub use audio_prefs::{load_audio_preferences, save_audio_preferences, AudioPreferences};
pub use datetime::LocalDateTime;
pub use error::IoError;
pub use library::{
    color_sets, curve_library, gradient_library, set_color_sets, set_curve_library, set_gradient_library, CurveLibrary,
    GradientLibrary, lip_sync_map_names};
pub use module_store::{load_module_store_file, save_module_store, ModuleEntry, ModuleStore};
pub use params::{
    effect_model_to_xml, effect_params_from_xml, hint_count, is_hint, params_from_xml, params_to_members, strip_hints,
};
pub use paths::DataPaths;
pub use settings::{settings_dir, ProfileSettings, SettingSection};
pub use sequence::{
    load_sequence, load_sequence_with_store, new_sequence, save_sequence, EffectNode, FidelityReport, MediaEntry,
    RowSetting, SelectedTimingProvider, SequenceFilterNode, TimedSequence,
};
pub use system::{
    load_system, save_system, Channel, Controller, ControllerOutput, OutputFilter, Patch, Preview, SystemData,
    TagDefinition,
};

/// A fresh folder for a unit test, under the target folder the test binary was built in.
#[cfg(test)]
pub(crate) fn test_dir(name: &str) -> std::path::PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let unique = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let exe = std::env::current_exe().unwrap();
    let dir = exe.parent().unwrap().join(format!("vixen-io-test-{name}-{}-{unique}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
