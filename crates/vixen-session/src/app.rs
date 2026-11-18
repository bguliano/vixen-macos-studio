//! The application state all windows share: the open profile.

use crate::boot::{AppService, LaunchOptions};
use crate::execution::ExecutionEngine;
use crate::instrumentation::Instrumentation;
use crate::playback::ShowPlayback;
use crate::sequence::SessionEnvironment;
use crate::SessionError;
use parking_lot::{Mutex, RwLock};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use vixen_engine::schedule::ElementMap;
use vixen_io::{load_application_data, load_system, save_application_data, ApplicationData, DataPaths, SystemData};

pub use vixen_io::MAX_RECENT_SEQUENCES;

/// Marks a save as running until it's dropped. `begin` is `None` when one already is.
pub(crate) struct Saving<'a>(&'a AtomicBool);

impl<'a> Saving<'a> {
    pub(crate) fn begin(flag: &'a AtomicBool) -> Option<Saving<'a>> {
        (!flag.swap(true, Ordering::AcqRel)).then_some(Saving(flag))
    }
}

impl Drop for Saving<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// What runs alongside the app, and whether it has been started.
#[derive(Default)]
pub(crate) struct Services {
    pub(crate) started: bool,
    pub(crate) list: Vec<Arc<dyn AppService>>,
}

/// The open profile. `VixenSystem` and the application object in the original.
pub struct AppCore {
    pub studio_display: crate::studio_display::StudioDisplay, // STUDIO: one command and undo service per profile
    pub video_cache: Arc<vixen_video::cache::VideoCache>,
    pub paths: DataPaths,
    pub system: RwLock<SystemData>,
    pub application_data: RwLock<ApplicationData>,
    pub playback: Arc<ShowPlayback>,
    pub live: crate::live::LiveContexts,
    pub(crate) web: Mutex<Option<Arc<crate::web::WebService>>>,
    /// The Instrumentation window's list, for whatever measures something of its own, such as a preview.
    pub instrumentation: Arc<Instrumentation>,
    /// What depends on the display: see `setup::commit`.
    pub display: crate::setup::commit::DisplayHub,
    pub(crate) profile_name: Option<String>,
    pub(crate) options: LaunchOptions,
    pub(crate) lock_path: Mutex<Option<PathBuf>>,
    pub(crate) saving_system: AtomicBool,
    pub(crate) saving_module: AtomicBool,
    pub(crate) services: Mutex<Services>,
    pub(crate) preview_changes: AtomicBool,
    pub(crate) preview_needs_reload: AtomicBool,
    pub(crate) options_changed: AtomicBool,
}

impl AppCore {
    /// Opens a profile's data folder. `muted` means no sound and no output hardware, as tests need.
    pub fn open(root: PathBuf, muted: bool) -> Result<AppCore, SessionError> {
        let paths = DataPaths::new(root);
        paths.ensure_directories()?;
        let system = load_system(&paths)?;
        let application_data = load_application_data(&paths)?;
        let options = LaunchOptions { muted, ..LaunchOptions::default() };
        Ok(AppCore::assemble(paths, system, application_data, None, options, None))
    }

    /// Everything is loaded: puts the parts together.
    pub(crate) fn assemble(
        paths: DataPaths,
        system: SystemData,
        application_data: ApplicationData,
        profile_name: Option<String>,
        options: LaunchOptions,
        lock_path: Option<PathBuf>,
    ) -> AppCore {
        let elements = Arc::new(ElementMap::from_tree(&system.tree));
        let playback = Arc::new(ShowPlayback::new(&system, elements, options.muted));
        let video_cache = vixen_video::cache::VideoCache::new(vixen_video::cache::profile_cache_root(paths.root()),
            vixen_video::cache::CacheOptions { clear_on_exit: system.clear_effect_cache_on_exit, file_type: system.video_effect_cache_file_type.clone() });
        AppCore {
            studio_display: Default::default(), // STUDIO: shared live display service
            video_cache,
            paths,
            system: RwLock::new(system),
            application_data: RwLock::new(application_data),
            playback,
            live: Default::default(),
            web: Mutex::new(None),
            instrumentation: Arc::new(Instrumentation::new()),
            display: Default::default(),
            profile_name,
            options,
            lock_path: Mutex::new(lock_path),
            saving_system: AtomicBool::new(false),
            saving_module: AtomicBool::new(false),
            services: Mutex::new(Services::default()),
            preview_changes: AtomicBool::new(false),
            preview_needs_reload: AtomicBool::new(false),
            options_changed: AtomicBool::new(false),
        }
    }

    /// The execution engine's state machine.
    pub fn execution(&self) -> &ExecutionEngine {
        self.playback.execution()
    }

    /// What a sequence window needs from the profile, as it stands now.
    pub fn session_environment(&self) -> SessionEnvironment {
        let system = self.system.read();
        SessionEnvironment { video_cache: Some(self.video_cache.clone()),
            tree: Arc::new(system.tree.clone()),
            store: Arc::new(vixen_io::system::effective_store(&system).into_owned()),
            module_data_root: Some(self.paths.module_data_dir()),
            frame_ms: system.default_update_interval,
            media_dir: self.paths.media_dir(),
        }
    }

    /// The recent sequences that exist, most recent first. A list saved on Windows holds Windows paths, so each
    /// resolves by file name in this profile, and two old paths to one file show once.
    pub fn recent_sequences(&self) -> Vec<PathBuf> {
        self.application_data.read().visible_recent_sequences(&self.paths).into_iter().map(|r| r.path).collect()
    }

    /// `AddSequenceToRecentList`: moves the file to the front, trims the list and saves it.
    pub fn add_recent_sequence(&self, path: &str) -> Result<(), SessionError> {
        self.application_data.write().add_recent_sequence(path, &self.paths);
        self.save_application_data()
    }

    /// `ClearRecentSequencesList`.
    pub fn clear_recent_sequences(&self) {
        self.application_data.write().clear_recent_sequences();
    }

    pub fn save_application_data(&self) -> Result<(), SessionError> {
        Ok(save_application_data(&self.application_data.read(), &self.paths)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_profile(name: &str) -> AppCore {
        let root = std::env::temp_dir().join(format!("vixen-app-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let core = AppCore::open(root, true).unwrap();
        for file in ["A.tim", "B.tim"] {
            std::fs::write(core.paths.sequence_dir().join(file), "").unwrap();
        }
        core
    }

    #[test]
    fn windows_paths_resolve_by_file_name_and_show_once() {
        let core = temp_profile("resolve");
        core.application_data.write().recent_sequences = vec![
            "C:\\Users\\x\\Documents\\Vixen 3\\Sequence\\A.tim".into(),
            "C:\\Users\\y\\Documents\\Vixen 3\\Sequence\\B.tim".into(),
            "C:\\Users\\y\\Documents\\Vixen 3\\Sequence\\A.tim".into(),
            "C:\\Users\\y\\Documents\\Vixen 3\\Sequence\\Gone.tim".into(),
        ];
        let names: Vec<String> =
            core.recent_sequences().iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["A.tim", "B.tim"]);
    }

    #[test]
    fn adding_moves_to_the_front_and_keeps_twenty() {
        let core = temp_profile("add");
        for i in 0..25 {
            core.add_recent_sequence(&format!("/s/{i}.tim")).unwrap();
        }
        core.add_recent_sequence("/s/20.tim").unwrap();
        let list = core.application_data.read().recent_sequences.clone();
        assert_eq!(list.len(), MAX_RECENT_SEQUENCES);
        assert_eq!(list[0], "/s/20.tim");
        assert_eq!(list[1], "/s/24.tim");
        assert_eq!(list.iter().filter(|p| *p == "/s/20.tim").count(), 1);
        // It reached the disk.
        let saved = load_application_data(&core.paths).unwrap();
        assert_eq!(saved.recent_sequences, list);
    }

    #[test]
    fn clearing_only_reaches_the_disk_with_the_next_save() {
        let core = temp_profile("clear");
        core.add_recent_sequence("/s/a.tim").unwrap();
        core.clear_recent_sequences();
        assert!(core.recent_sequences().is_empty());
        assert_eq!(load_application_data(&core.paths).unwrap().recent_sequences.len(), 1);
        core.save_application_data().unwrap();
        assert!(load_application_data(&core.paths).unwrap().recent_sequences.is_empty());
    }
}
