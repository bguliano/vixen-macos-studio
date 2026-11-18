//! Starting and stopping the app: `VixenSystem.Start` and `Stop`, the saves, and the command line. See F1, F2, F7, F8, F9 and F12 in
//! `docs/spec/administration.md`.

use crate::app::{AppCore, Saving};
use crate::logging;
use crate::SessionError;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use uuid::Uuid;
use vixen_core::element::ElementTree;
use vixen_io::system::{load_system_with, TagDefinition};
use vixen_io::{DataPaths, SystemData};
use vixen_preview::VIXEN_PREVIEW_MODULE_TYPE;

const SYSTEM_LOGGER: &str = "Vixen.Sys.VixenSystem";

/// What the command line and the environment ask for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchOptions {
    /// `data_dir=`, or `VIXEN_DATA`.
    pub data_dir: Option<PathBuf>,
    /// The folder came from `VIXEN_DATA`, a test's own folder, which is made if it's missing rather than falling back to the real one.
    pub data_dir_from_environment: bool,
    /// `no_controllers`.
    pub disable_controllers: bool,
    /// False for `no_execution`.
    pub open_execution: bool,
    /// `VIXEN_UI_TEST`: output stays off without that reaching any save, and nothing touches the network.
    pub under_test: bool,
    /// `VIXEN_MUTE` or `VIXEN_UI_TEST`: no sound and no output hardware.
    pub muted: bool,
}

impl Default for LaunchOptions {
    fn default() -> LaunchOptions {
        LaunchOptions {
            data_dir: None,
            data_dir_from_environment: false,
            disable_controllers: false,
            open_execution: true,
            under_test: false,
            muted: false,
        }
    }
}

impl LaunchOptions {
    /// The folder to use, from whichever profile was chosen. A test's own folder is taken as it is, and anything else follows F7.
    pub fn data_folder(&self, chosen: Option<PathBuf>) -> PathBuf {
        if self.data_dir_from_environment {
            if let Some(dir) = &self.data_dir {
                return dir.clone();
            }
        }
        resolve_data_folder(chosen.or_else(|| self.data_dir.clone()))
    }
}

/// `ProcessCommandLine`: each argument, the program's name too, splits on `=` and its first part matches exactly. The text after a
/// second `=` is lost, as upstream. `VIXEN_DATA` acts as `data_dir=`, and an argument overrides it.
pub fn parse_launch_options(args: &[String], env: impl Fn(&str) -> Option<String>) -> LaunchOptions {
    let mut options = LaunchOptions::default();
    if let Some(dir) = env("VIXEN_DATA") {
        options.data_dir = Some(PathBuf::from(dir));
        options.data_dir_from_environment = true;
    }
    for arg in args {
        let mut parts = arg.split('=');
        match parts.next() {
            Some("no_controllers") => options.disable_controllers = true,
            Some("no_execution") => options.open_execution = false,
            Some("data_dir") => {
                options.data_dir = parts.next().map(PathBuf::from);
                options.data_dir_from_environment = false;
            }
            _ => {}
        }
    }
    options.under_test = env("VIXEN_UI_TEST").is_some();
    options.muted = options.under_test || env("VIXEN_MUTE").is_some();
    options
}

/// `VixenApplicationData.LoadData`'s choice of folder: one that exists is kept, and anything else becomes the default, silently, as
/// upstream.
pub fn resolve_data_folder(requested: Option<PathBuf>) -> PathBuf {
    if let Some(folder) = requested {
        if folder.is_dir() {
            return folder;
        }
    }
    DataPaths::default_root().unwrap_or_else(|| PathBuf::from(vixen_io::paths::VIXEN_DATA_DIR))
}

/// `IProgress<Tuple<int, string>>`. Called from the thread running `start`.
pub trait StartupProgress: Send + Sync {
    fn report(&self, percent: i32, text: &str);
}

/// Text a bundled file holds, by its BOM. Upstream's `License.txt` is UTF-16.
pub fn decode_text(bytes: &[u8]) -> String {
    match bytes {
        [0xFF, 0xFE, rest @ ..] => {
            let units: Vec<u16> = rest.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            String::from_utf16_lossy(&units)
        }
        [0xFE, 0xFF, rest @ ..] => {
            let units: Vec<u16> = rest.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
            String::from_utf16_lossy(&units)
        }
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// `VixenSystem.EnsureBuiltInElementTags` with `BuiltInElementTags.CreateDefaults`.
pub fn ensure_built_in_element_tags(system: &mut SystemData) {
    const BUILT_IN: [(Uuid, &str, &str, i32, &str); 3] = [
        (uuid::uuid!("753115da-e27a-4261-a136-8222ccc3f22e"), "deprecated", "Deprecated", 0, "#FF0000"),
        (uuid::uuid!("2f21bfcb-07a3-4cac-bdc5-4b6e37773c04"), "hidden", "Hidden", 1, "#000000"),
        (uuid::uuid!("6db1852d-71d8-449d-afa5-193a6def26a0"), "prop", "Prop", 2, "#0000FF"),
    ];
    for (id, key, name, sort_order, color) in BUILT_IN {
        if !system.tags.iter().any(|t| t.id == id) {
            system.tags.push(TagDefinition {
                id,
                key: key.to_string(),
                name: name.to_string(),
                is_built_in: true,
                sort_order,
                description: None,
                display_color: Some(color.to_string()),
            });
            logging::info(SYSTEM_LOGGER, &format!("Seeded built-in element tag '{name}' ({key})."));
        }
    }
}

/// `CleanUpOrphanedPreviewModuleData` and `CleanUpOrphanedElementTagAssignments`. In memory: a later save writes the result.
pub fn cleanup_orphans(system: &mut SystemData) {
    let orphans: Vec<Uuid> = system
        .module_store
        .instance_entries()
        .iter()
        .filter(|entry| entry.module_type() == Some(VIXEN_PREVIEW_MODULE_TYPE))
        .filter_map(|entry| entry.module_instance())
        .filter(|id| !system.previews.iter().any(|p| p.instance_id == *id))
        .collect();
    for id in orphans {
        system.module_store.remove_instance(VIXEN_PREVIEW_MODULE_TYPE, id);
        logging::warn(SYSTEM_LOGGER, &format!("Removed orphaned Preview data for instance {id}"));
    }

    let valid: Vec<Uuid> = system.tags.iter().map(|t| t.id).collect();
    remove_orphaned_tags(&mut system.tree, &valid);
}

fn remove_orphaned_tags(tree: &mut ElementTree, valid: &[Uuid]) {
    let found: Vec<_> = tree
        .all_nodes()
        .filter(|(_, node)| node.tags.iter().any(|t| !valid.contains(t)))
        .map(|(id, _)| id)
        .collect();
    for id in found {
        let node = tree.node_mut(id);
        let (name, node_id) = (node.name.clone(), node.id);
        node.tags.retain(|tag| {
            let keep = valid.contains(tag);
            if !keep {
                logging::warn(SYSTEM_LOGGER, &format!("Removed orphaned element tag assignment {tag} from node '{name}' ({node_id})."));
            }
            keep
        });
    }
}

/// `Modules.PopulateRepositories`' app modules: something that starts with the app and stops with it. In the original's module file order,
/// each failing on its own without stopping the rest.
pub trait AppService: Send + Sync {
    fn name(&self) -> &str;
    fn start(&self, core: &AppCore) -> Result<(), String>;
    fn stop(&self, core: &AppCore);
    /// The system config was loaded again. A service holding a copy of its data reads it from `core.system` again here, so what it changes
    /// later reaches the module store. In the original a module keeps writing to the old store's object, and those changes are lost.
    fn reloaded(&self, _core: &AppCore) {}
}

const APP_DATA_LOGGER: &str = "VixenApplication.VixenApplicationData";

/// `VixenApplicationData.LoadData`: a damaged file never stops the app. It logs and carries on with an empty list, and the next save replaces the
/// file. A version it doesn't know reads as empty too, and keeps the file's content.
fn load_application_data_or_empty(paths: &DataPaths) -> vixen_io::ApplicationData {
    match vixen_io::load_application_data(paths) {
        Ok(data) => {
            if data.data_format_version > vixen_io::appdata::CURRENT_APPLICATION_DATA_VERSION {
                logging::error(
                    APP_DATA_LOGGER,
                    &format!("VixenApplication: error reading application data; given data version was too high: {}", data.data_format_version),
                    None,
                );
            }
            data
        }
        Err(error) => {
            let text = error.to_string();
            if text.contains("couldn't find data format version") {
                logging::error(APP_DATA_LOGGER, "VixenApplication: loading application data: couldn't find data format version", None);
            } else {
                logging::error(APP_DATA_LOGGER, "VixenApplication: error loading application data", Some(&text));
            }
            vixen_io::ApplicationData::new()
        }
    }
}

impl AppCore {
    /// `VixenSystem.Start`: folders, then the config, then services, then the engine. Reports progress in the original's words. `lock_path` is
    /// the lock this launch took, which shutdown removes.
    pub fn start(
        root: PathBuf,
        profile_name: Option<String>,
        options: LaunchOptions,
        lock_path: Option<PathBuf>,
        progress: &dyn StartupProgress,
    ) -> Result<Arc<AppCore>, SessionError> {
        logging::info(SYSTEM_LOGGER, "Vixen System starting up...");
        progress.report(10, "Preparing modules");
        let paths = DataPaths::new(trim_trailing_separator(root));
        paths.ensure_directories()?;
        progress.report(20, "Loading modules");

        progress.report(30, "Loading system config");
        let mut system = load_system_with(&paths, || progress.report(50, "Module config loaded"))?;
        progress.report(70, "System config loaded");
        ensure_built_in_element_tags(&mut system);
        cleanup_orphans(&mut system);
        // The LipSync app module's `Loading()`: once at startup, not on a reload.
        crate::marks::string_maps::migrate_string_maps_at_load(&mut system);

        // `no_controllers`, all devices and previews alike. A later save writes them all, as upstream. Under test the same goes for
        // controllers alone, through `muted`, and nothing reaches a save.
        if options.disable_controllers && !options.under_test {
            let mut all: Vec<Uuid> = system.controllers.iter().map(|c| c.id).collect();
            all.extend(system.previews.iter().map(|p| p.instance_id));
            system.disabled_devices = all;
        }

        let application_data = load_application_data_or_empty(&paths);

        let migrated = system.source_version < system.version || system.module_store.source_version < system.module_store.version;
        let core = Arc::new(AppCore::assemble(paths, system, application_data, profile_name, options.clone(), lock_path));
        core.start_services();

        if migrated {
            progress.report(80, "Saving migrated config");
            // GUARD: this finishes before the engine opens, and a failure stops the launch, so a half-saved show can't reach the lights.
            core.save_system_config()?;
            core.save_module_config()?;
        }
        progress.report(70, "Config loaded");

        if options.open_execution {
            core.execution().open();
        }
        logging::info(SYSTEM_LOGGER, "Vixen System successfully started.");
        Ok(core)
    }

    /// The profile's name when one was chosen. `VixenSystem.ProfileName`.
    pub fn profile_name(&self) -> Option<String> {
        self.profile_name.clone()
    }

    pub fn launch_options(&self) -> &LaunchOptions {
        &self.options
    }

    /// Adds a service, in the order of the original's modules. Once the app has started, it starts at once.
    pub fn add_service(&self, service: Arc<dyn AppService>) {
        let mut services = self.services.lock();
        if services.started {
            start_service(self, service.as_ref());
        }
        services.list.push(service);
    }

    /// Starts every service added so far. A failure is logged and never fatal. Skipped under test, which must start no web server.
    pub fn start_services(&self) {
        let mut services = self.services.lock();
        if services.started {
            return;
        }
        services.started = true;
        for service in services.list.iter() {
            start_service(self, service.as_ref());
        }
    }

    /// `VixenSystem.Stop(false)`, then saving the application data and removing the lock. Neither system file is written: only explicit saves count,
    /// so a controller toggled and never saved comes back as it was.
    pub fn shutdown(&self) {
        self.studio_display.stop(); // STUDIO: stop agent commands before releasing the profile lock
        logging::info(SYSTEM_LOGGER, "Vixen System stopping...");
        self.execution().close();
        self.live.release_all();
        {
            let mut services = self.services.lock();
            for service in services.list.iter() {
                service.stop(self);
            }
            services.started = false;
        }
        self.playback.stop_all_contexts();
        self.live.release_all();
        self.video_cache.shutdown();
        logging::info(SYSTEM_LOGGER, "Vixen System successfully stopped.");
        if let Err(error) = self.save_application_data() {
            logging::error(APP_DATA_LOGGER, "VixenApplication: error saving application data", Some(&error.to_string()));
        }
        self.release_lock();
        crate::marks::lyric_converter::release_dictionary(&self.paths.root().join("user_dictionary"));
        logging::flush();
    }

    /// Removes this launch's lock file, if it holds one.
    pub fn release_lock(&self) {
        if let Some(path) = self.lock_path.lock().take() {
            if let Err(error) = vixen_io::lock::try_remove_lock_file(&path) {
                logging::error(
                    "VixenApplication.VixenApplication",
                    "An error occurred removing the profile lock file.",
                    Some(&error.to_string()),
                );
            }
        }
        logging::set_crash_lock(None);
    }

    /// `SaveSystemConfigAsync`. False with an error logged if one is already running. Records which devices are off only while the engine is open.
    /// Upstream records them whenever the system has started, so saving with the engine stopped marks every device disabled for the next launch.
    pub fn save_system_config(&self) -> Result<bool, SessionError> {
        // Before the check, as upstream, so a skipped save logs both lines. The module save logs after its check.
        logging::info(SYSTEM_LOGGER, "Saving System Config.");
        let Some(_guard) = Saving::begin(&self.saving_system) else {
            logging::error(SYSTEM_LOGGER, "System config is already being saved. Skipping duplicate request.", None);
            return Ok(false);
        };
        self.snapshot_disabled_devices();
        vixen_io::system::save_system_config_file(&self.system.read(), &self.paths)?;
        logging::info(SYSTEM_LOGGER, "System Config saved.");
        Ok(true)
    }

    /// `SaveModuleConfigAsync`. False with an error logged if one is already running.
    pub fn save_module_config(&self) -> Result<bool, SessionError> {
        let Some(_guard) = Saving::begin(&self.saving_module) else {
            logging::error(SYSTEM_LOGGER, "Module config is already being saved. Skipping duplicate request.", None);
            return Ok(false);
        };
        logging::info(SYSTEM_LOGGER, "Saving Module Config.");
        vixen_io::system::save_module_config_file(&self.system.read(), &self.paths)?;
        logging::info(SYSTEM_LOGGER, "Module Config saved.");
        Ok(true)
    }

    /// `IsSaving`.
    pub fn is_saving(&self) -> bool {
        self.saving_system.load(Ordering::Relaxed) || self.saving_module.load(Ordering::Relaxed)
    }

    /// `SaveDisabledDevices`. Skipped while muted, whose devices never run, so a test can't record them all as off.
    fn snapshot_disabled_devices(&self) {
        if !self.execution().is_open() || self.options.muted {
            return;
        }
        let mut disabled = self.playback.controllers_not_running(&self.system.read());
        disabled.extend(self.playback.preview_devices().not_running());
        self.playback.preview_devices().apply_disabled(&disabled);
        self.playback.apply_disabled_controllers(&disabled);
        self.system.write().disabled_devices = disabled;
    }

    /// `ReloadSystemConfig`: remember whether the engine was open, close it, load the files again, reopen it if it was.
    pub fn reload_system_config(&self) -> Result<(), SessionError> {
        while self.is_saving() {
            logging::info(SYSTEM_LOGGER, "Reload Requested while save in progress. Waiting 5 ms.");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let was_open = self.execution().is_open();
        self.execution().close();
        let mut system = vixen_io::load_system(&self.paths)?;
        ensure_built_in_element_tags(&mut system);
        cleanup_orphans(&mut system);
        {
            let mut current = self.system.write();
            *current = system;
            self.rebuild_output_from(&current);
        }
        self.reset_preview_changes();
        for service in self.services.lock().list.iter() {
            service.reloaded(self);
        }
        if was_open {
            self.execution().open();
        }
        Ok(())
    }

    /// Rebuilds the output graph and controllers after the element tree, controllers, filters or patches changed. Stops whatever's running while
    /// it does, and starts it again if the engine is open.
    pub fn rebuild_output(&self) {
        let was_open = self.execution().is_open();
        self.execution().close();
        {
            let system = self.system.read();
            self.rebuild_output_from(&system);
        }
        if was_open {
            self.execution().open();
        }
    }

    fn rebuild_output_from(&self, system: &SystemData) {
        let elements = Arc::new(vixen_engine::schedule::ElementMap::from_tree(&system.tree));
        self.playback.rebuild(system, elements);
    }

    /// The Options dialog's four values, with the update interval clamped to what its control can show. A stored value above 500 throws upstream.
    pub fn options(&self) -> AppOptions {
        let system = self.system.read();
        AppOptions {
            update_interval_ms: system.default_update_interval.clamp(0, 500),
            // A value outside these ranges throws upstream when the dialog loads.
            audio_latency_ms: vixen_io::load_audio_preferences(&vixen_io::settings_dir()).latency_ms.clamp(5, 500),
            clear_effect_cache_on_exit: system.clear_effect_cache_on_exit,
            video_effect_cache_file_type: system.video_effect_cache_file_type.clone(),
        }
    }

    /// Applies the values. Latency saves at once, as upstream. The rest wait for [`AppCore::save_options_if_changed`].
    pub fn set_options(&self, options: &AppOptions) -> Result<(), SessionError> {
        let current = self.options();
        if options.audio_latency_ms != current.audio_latency_ms {
            let dir = vixen_io::settings_dir();
            let mut preferences = vixen_io::load_audio_preferences(&dir);
            preferences.latency_ms = options.audio_latency_ms;
            vixen_io::save_audio_preferences(&preferences, &dir)?;
            self.playback.set_audio_latency_ms(options.audio_latency_ms);
        }
        let same = options.update_interval_ms == current.update_interval_ms
            && options.clear_effect_cache_on_exit == current.clear_effect_cache_on_exit
            && options.video_effect_cache_file_type == current.video_effect_cache_file_type;
        if same {
            return Ok(());
        }
        {
            let mut system = self.system.write();
            system.default_update_interval = options.update_interval_ms;
            system.clear_effect_cache_on_exit = options.clear_effect_cache_on_exit;
            system.video_effect_cache_file_type = options.video_effect_cache_file_type.clone();
        }
        self.video_cache.set_options(vixen_video::cache::CacheOptions { clear_on_exit: options.clear_effect_cache_on_exit, file_type: options.video_effect_cache_file_type.clone() });
        self.playback.set_update_interval(options.update_interval_ms);
        self.options_changed.store(true, Ordering::Relaxed);
        Ok(())
    }

    /// What the original does at OK, for when the Settings window closes: saves the system config if something changed.
    pub fn save_options_if_changed(&self) -> Result<bool, SessionError> {
        if !self.options_changed.swap(false, Ordering::Relaxed) {
            return Ok(false);
        }
        self.save_system_config()
    }

    /// The Instrumentation window's lines. The controllers' rows come and go as they start and stop.
    pub fn instrumentation_lines(&self) -> Vec<String> {
        let mut lines = self.playback.engine_instrumentation_lines();
        lines.extend(self.instrumentation.lines());
        lines.extend(self.playback.controller_instrumentation_lines());
        lines
    }

    /// The Reset button.
    pub fn instrumentation_reset(&self) {
        self.playback.reset_instrumentation();
        self.instrumentation.reset_all();
    }
}

fn start_service(core: &AppCore, service: &dyn AppService) {
    if core.options.under_test {
        return;
    }
    if let Err(error) = service.start(core) {
        logging::error("Vixen.Sys.Modules", &format!("Error occurred while adding module {}", service.name()), Some(&error));
    }
}

/// `Paths.DataRootPath`'s setter.
fn trim_trailing_separator(root: PathBuf) -> PathBuf {
    let text = root.to_string_lossy();
    match text.strip_suffix('/') {
        Some(trimmed) if !trimmed.is_empty() => PathBuf::from(trimmed),
        _ => root,
    }
}

/// The Options dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppOptions {
    pub update_interval_ms: i32,
    pub audio_latency_ms: i32,
    pub clear_effect_cache_on_exit: bool,
    pub video_effect_cache_file_type: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::ExecutionState;
    use std::collections::HashMap;
    use std::path::Path;

    fn args(list: &[&str]) -> Vec<String> {
        std::iter::once("Vixen").chain(list.iter().copied()).map(str::to_string).collect()
    }

    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn plain_launch_defaults() {
        let options = parse_launch_options(&args(&[]), no_env);
        assert_eq!(options, LaunchOptions::default());
        assert!(options.open_execution);
    }

    #[test]
    fn tokens_set_their_fields() {
        let options = parse_launch_options(&args(&["no_controllers", "no_execution", "data_dir=/x"]), no_env);
        assert!(options.disable_controllers);
        assert!(!options.open_execution);
        assert_eq!(options.data_dir, Some(PathBuf::from("/x")));
    }

    #[test]
    fn data_dir_alone_gives_none_and_a_second_equals_loses_the_rest() {
        assert_eq!(parse_launch_options(&args(&["data_dir"]), no_env).data_dir, None);
        assert_eq!(parse_launch_options(&args(&["data_dir=/a=b"]), no_env).data_dir, Some(PathBuf::from("/a")));
    }

    #[test]
    fn matching_is_exact() {
        let options = parse_launch_options(&args(&["No_Controllers", "no_execution_please", "data_dirs=/x"]), no_env);
        assert_eq!(options, LaunchOptions::default());
    }

    #[test]
    fn the_environment_supplies_the_folder_and_an_argument_wins() {
        let env = |name: &str| (name == "VIXEN_DATA").then(|| "/env".to_string());
        let options = parse_launch_options(&args(&[]), env);
        assert_eq!(options.data_dir, Some(PathBuf::from("/env")));
        assert!(options.data_dir_from_environment);
        assert_eq!(options.data_folder(None), PathBuf::from("/env"));
        let options = parse_launch_options(&args(&["data_dir=/arg"]), env);
        assert_eq!(options.data_dir, Some(PathBuf::from("/arg")));
        assert!(!options.data_dir_from_environment);
    }

    #[test]
    fn under_test_and_mute_come_from_the_environment() {
        let env = |name: &str| (name == "VIXEN_UI_TEST").then(|| "script.json".to_string());
        let options = parse_launch_options(&args(&[]), env);
        assert!(options.under_test && options.muted);
        let env = |name: &str| (name == "VIXEN_MUTE").then(|| "1".to_string());
        let options = parse_launch_options(&args(&[]), env);
        assert!(!options.under_test && options.muted);
    }

    #[test]
    fn an_existing_folder_is_kept_and_a_missing_one_falls_back_to_the_default() {
        let dir = temp_dir("resolve");
        assert_eq!(resolve_data_folder(Some(dir.clone())), dir);
        let default = DataPaths::default_root().unwrap();
        assert_eq!(resolve_data_folder(Some(dir.join("missing"))), default);
        assert_eq!(resolve_data_folder(None), default);
    }

    #[test]
    fn text_decodes_by_its_byte_order_mark() {
        let utf16: Vec<u8> = [0xFF, 0xFE].into_iter().chain("Hi é".encode_utf16().flat_map(u16::to_le_bytes)).collect();
        assert_eq!(decode_text(&utf16), "Hi é");
        assert_eq!(decode_text(b"\xEF\xBB\xBFplain"), "plain");
        assert_eq!(decode_text(b"plain"), "plain");
    }

    struct NoProgress;
    impl StartupProgress for NoProgress {
        fn report(&self, _: i32, _: &str) {}
    }

    #[derive(Default)]
    struct Recording(parking_lot::Mutex<Vec<(i32, String)>>);
    impl StartupProgress for Recording {
        fn report(&self, percent: i32, text: &str) {
            self.0.lock().push((percent, text.to_string()));
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vixen-boot-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn muted() -> LaunchOptions {
        LaunchOptions { muted: true, under_test: true, ..LaunchOptions::default() }
    }

    fn start(dir: &Path, options: LaunchOptions) -> Arc<AppCore> {
        AppCore::start(dir.to_path_buf(), None, options, None, &NoProgress).unwrap()
    }

    #[test]
    fn starting_makes_the_folders_opens_the_engine_and_reports_in_the_original_words() {
        let dir = temp_dir("start").join("Vixen 3");
        let progress = Recording::default();
        let core = AppCore::start(dir.clone(), Some("Show".to_string()), muted(), None, &progress).unwrap();
        assert!(dir.join("Module Data Files/VixenDisplayPreview").is_dir());
        assert_eq!(core.execution().state(), ExecutionState::Open);
        assert_eq!(core.profile_name().as_deref(), Some("Show"));
        let reported: Vec<(i32, String)> = progress.0.lock().clone();
        assert_eq!(reported.first(), Some(&(10, "Preparing modules".to_string())));
        assert_eq!(reported.last(), Some(&(70, "Config loaded".to_string())));
        assert!(reported.contains(&(30, "Loading system config".to_string())));
        assert!(reported.contains(&(50, "Module config loaded".to_string())));
        assert!(reported.contains(&(70, "System config loaded".to_string())));
    }

    #[test]
    fn no_execution_leaves_the_engine_closed() {
        let dir = temp_dir("no-exec");
        let core = start(&dir, LaunchOptions { open_execution: false, ..muted() });
        assert_eq!(core.execution().state(), ExecutionState::Closed);
    }

    #[test]
    fn a_new_profile_gets_the_built_in_tags() {
        let dir = temp_dir("tags");
        let core = start(&dir, muted());
        let system = core.system.read();
        let keys: Vec<&str> = system.tags.iter().map(|t| t.key.as_str()).collect();
        assert_eq!(keys, ["deprecated", "hidden", "prop"]);
        assert_eq!(system.tags[1].id, uuid::uuid!("2f21bfcb-07a3-4cac-bdc5-4b6e37773c04"));
        assert_eq!(system.tags[2].display_color.as_deref(), Some("#0000FF"));
    }

    #[test]
    fn built_in_tags_already_there_are_left_alone() {
        let mut system = SystemData::default();
        ensure_built_in_element_tags(&mut system);
        system.tags[0].display_color = Some("#123456".to_string());
        ensure_built_in_element_tags(&mut system);
        assert_eq!(system.tags.len(), 3);
        assert_eq!(system.tags[0].display_color.as_deref(), Some("#123456"));
    }

    #[test]
    fn orphans_are_removed_in_memory() {
        let mut system = SystemData::default();
        ensure_built_in_element_tags(&mut system);
        let kept_preview = Uuid::new_v4();
        let orphan_preview = Uuid::new_v4();
        system.previews.push(vixen_io::system::Preview { name: "Kept".to_string(), type_id: VIXEN_PREVIEW_MODULE_TYPE, instance_id: kept_preview });
        for id in [kept_preview, orphan_preview] {
            let data = vixen_xml::XElement::new("", "VixenPreviewData");
            system.module_store.set_instance(vixen_io::ModuleEntry::new("x", &VIXEN_PREVIEW_MODULE_TYPE, &id, "y", data));
        }
        let node = system.tree.add_element("E");
        let kept_tag = system.tags[0].id;
        let bad_tag = Uuid::new_v4();
        system.tree.node_mut(node).tags = vec![kept_tag, bad_tag];
        cleanup_orphans(&mut system);
        assert!(system.module_store.instance(VIXEN_PREVIEW_MODULE_TYPE, kept_preview).is_some());
        assert!(system.module_store.instance(VIXEN_PREVIEW_MODULE_TYPE, orphan_preview).is_none());
        assert_eq!(system.tree.node(node).tags, vec![kept_tag]);
    }

    #[test]
    fn the_built_in_tags_are_three_built_in_definitions_with_stable_ids() {
        let ids = |system: &SystemData| system.tags.iter().map(|t| t.id).collect::<Vec<_>>();
        let mut first = SystemData::default();
        ensure_built_in_element_tags(&mut first);
        let mut second = SystemData::default();
        ensure_built_in_element_tags(&mut second);
        assert_eq!(first.tags.len(), 3);
        assert!(first.tags.iter().all(|t| t.is_built_in));
        assert_eq!(ids(&first), ids(&second));
    }

    #[test]
    fn a_damaged_application_data_file_starts_with_an_empty_list_and_the_next_save_replaces_it() {
        for (name, text) in [("bad-xml", "<VixenApplicationData><Recent"), ("no-version", "<VixenApplicationData><RecentSequences/></VixenApplicationData>")] {
            let dir = temp_dir(name);
            let file = dir.join("VixenApplicationData.xml");
            std::fs::write(&file, text).unwrap();
            let core = start(&dir, muted());
            assert!(core.recent_sequences().is_empty());
            core.save_application_data().unwrap();
            assert!(vixen_io::load_application_data(&core.paths).is_ok(), "{name}");
        }
    }

    #[test]
    fn a_save_that_fails_returns_the_error_and_clears_the_flag() {
        let dir = temp_dir("save-fails");
        let core = start(&dir, muted());
        let folder = dir.join("SystemData");
        std::fs::remove_dir_all(&folder).unwrap();
        std::fs::write(&folder, "a file where the folder was").unwrap();
        assert!(core.save_system_config().is_err());
        assert!(core.save_module_config().is_err());
        assert!(!core.is_saving());
        std::fs::remove_file(&folder).unwrap();
        assert!(core.save_system_config().unwrap(), "a later save runs");
    }

    #[test]
    fn a_backup_that_fails_returns_the_error_and_leaves_the_old_file() {
        let dir = temp_dir("backup-fails");
        let core = start(&dir, muted());
        core.save_system_config().unwrap();
        let file = dir.join("SystemData/SystemConfig.xml");
        let before = std::fs::read(&file).unwrap();
        // A file where the backup folder goes, so the backup can't be made.
        std::fs::write(dir.join("SystemData/auto_backup"), "").unwrap();
        core.system.write().default_update_interval = 123;
        assert!(core.save_system_config().is_err());
        assert_eq!(std::fs::read(&file).unwrap(), before);
        assert!(!core.is_saving());
    }

    #[test]
    fn after_a_reload_a_module_store_edit_saves_normally() {
        let dir = temp_dir("reload-store");
        let core = start(&dir, muted());
        core.save_system_config().unwrap();
        core.save_module_config().unwrap();
        core.reload_system_config().unwrap();
        let id = Uuid::new_v4();
        let data = vixen_xml::XElement::new("", "VixenPreviewData");
        core.system.write().module_store.set_instance(vixen_io::ModuleEntry::new("x", &VIXEN_PREVIEW_MODULE_TYPE, &id, "y", data));
        core.save_module_config().unwrap();
        let saved = vixen_io::load_system(&core.paths).unwrap();
        assert!(saved.module_store.instance(VIXEN_PREVIEW_MODULE_TYPE, id).is_some());
    }

    #[test]
    fn a_reload_tells_each_service() {
        #[derive(Default)]
        struct Listening(std::sync::atomic::AtomicUsize);
        impl AppService for Listening {
            fn name(&self) -> &str {
                "Listening"
            }
            fn start(&self, _: &AppCore) -> Result<(), String> {
                Ok(())
            }
            fn stop(&self, _: &AppCore) {}
            fn reloaded(&self, _: &AppCore) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }
        let dir = temp_dir("reload-service");
        let core = start(&dir, muted());
        let service = Arc::new(Listening::default());
        core.add_service(service.clone());
        core.reload_system_config().unwrap();
        assert_eq!(service.0.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn reload_from_open_ends_open_and_from_closed_ends_closed() {
        let dir = temp_dir("reload");
        let core = start(&dir, muted());
        let events = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let log = events.clone();
        core.execution().subscribe(Box::new(move |state| log.lock().push(state)));
        core.reload_system_config().unwrap();
        assert_eq!(core.execution().state(), ExecutionState::Open);
        assert_eq!(
            *events.lock(),
            [ExecutionState::Closing, ExecutionState::Closed, ExecutionState::Opening, ExecutionState::Open]
        );
        core.execution().close();
        events.lock().clear();
        core.reload_system_config().unwrap();
        assert_eq!(core.execution().state(), ExecutionState::Closed);
        assert!(events.lock().is_empty());
    }

    #[test]
    fn reload_throws_away_what_was_never_saved() {
        let dir = temp_dir("reload-drop");
        let core = start(&dir, muted());
        core.save_system_config().unwrap();
        core.save_module_config().unwrap();
        core.system.write().default_update_interval = 123;
        core.reload_system_config().unwrap();
        assert_ne!(core.system.read().default_update_interval, 123);
    }

    fn with_two_previews(system: &mut SystemData) -> (Uuid, Uuid) {
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        for (id, name) in [(a, "A"), (b, "B")] {
            system.previews.push(vixen_io::system::Preview { name: name.to_string(), type_id: VIXEN_PREVIEW_MODULE_TYPE, instance_id: id });
        }
        (a, b)
    }

    fn real_core(dir: &Path) -> (Arc<AppCore>, Uuid, Uuid) {
        // Written to disk first, so `start` loads a profile with devices. Not muted: there are no controllers, and previews touch nothing.
        let paths = DataPaths::new(dir);
        paths.ensure_directories().unwrap();
        let mut system = SystemData::default();
        let (a, b) = with_two_previews(&mut system);
        system.disabled_devices = vec![b];
        vixen_io::save_system(&system, &paths).unwrap();
        (start(dir, LaunchOptions::default()), a, b)
    }

    #[test]
    fn a_disabled_device_does_not_start_with_the_engine() {
        let dir = temp_dir("disabled");
        let (core, a, b) = real_core(&dir);
        let rows: HashMap<Uuid, bool> = core.previews().into_iter().map(|r| (r.id, r.is_running)).collect();
        assert_eq!(rows[&a], true);
        assert_eq!(rows[&b], false);
    }

    #[test]
    fn stop_then_start_restarts_what_disabled_devices_says_not_what_was_running() {
        let dir = temp_dir("restart");
        let (core, a, b) = real_core(&dir);
        core.set_preview_running(&a, false);
        core.set_preview_running(&b, true);
        core.execution().close();
        core.execution().open();
        assert_eq!(core.running_previews(), vec![a]);
    }

    #[test]
    fn controllers_start_before_previews_and_stop_before_them() {
        use vixen_output::modules::{create_module, ControllerConfig};
        const DUMMY: Uuid = uuid::uuid!("b6ee6308-189c-4268-8996-32a4bab8ab5f");
        let dir = temp_dir("device-order");
        let (core, ..) = real_core(&dir);
        core.execution().close();
        let index = core.playback.with_output(|_, runner| {
            let config = ControllerConfig::new_default(&DUMMY, Uuid::new_v4()).expect("the dummy controller");
            runner.add_controller(Uuid::new_v4(), "Dummy", create_module(config), true)
        });
        let seen = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let (log, playback) = (seen.clone(), Arc::downgrade(&core.playback));
        core.subscribe_previews(Arc::new(move || {
            if let Some(playback) = playback.upgrade() {
                log.lock().push(playback.with_output(|_, runner| runner.is_running(index)));
            }
        }));
        core.execution().open();
        core.execution().close();
        assert_eq!(*seen.lock(), [true, false], "running when previews start, stopped by the time they stop");
    }

    #[test]
    fn a_controller_that_cannot_start_does_not_stop_the_others() {
        use vixen_output::modules::{create_module, ControllerConfig};
        const DUMMY: Uuid = uuid::uuid!("b6ee6308-189c-4268-8996-32a4bab8ab5f");
        // With no port named, its start fails without reaching any hardware.
        const GENERIC_SERIAL: Uuid = uuid::uuid!("d22c9233-16e9-4008-a215-e577f78a7a2e");
        let dir = temp_dir("controller-fails");
        let core = start(&dir, LaunchOptions { open_execution: false, ..LaunchOptions::default() });
        let (failing, working) = core.playback.with_output(|_, runner| {
            let serial = ControllerConfig::new_default(&GENERIC_SERIAL, Uuid::new_v4()).expect("the serial controller");
            let dummy = ControllerConfig::new_default(&DUMMY, Uuid::new_v4()).expect("the dummy controller");
            (
                runner.add_controller(Uuid::new_v4(), "Serial", create_module(serial), true),
                runner.add_controller(Uuid::new_v4(), "Dummy", create_module(dummy), true),
            )
        });
        core.execution().open();
        std::thread::sleep(std::time::Duration::from_millis(80));
        let (status, updates) = core.playback.with_output(|_, runner| (runner.status(failing), runner.stats(working).updates));
        assert!(updates > 0, "the second controller still runs, with the first at {status:?}");
        assert_eq!(core.execution().state(), ExecutionState::Open);
        core.execution().close();
    }

    #[test]
    fn muted_opens_the_engine_without_starting_a_controller() {
        use vixen_output::modules::{create_module, ControllerConfig};
        const DUMMY: Uuid = uuid::uuid!("b6ee6308-189c-4268-8996-32a4bab8ab5f");
        let dir = temp_dir("muted-controller");
        let core = start(&dir, LaunchOptions { open_execution: false, ..muted() });
        let index = core.playback.with_output(|_, runner| {
            let config = ControllerConfig::new_default(&DUMMY, Uuid::new_v4()).expect("the dummy controller");
            runner.add_controller(Uuid::new_v4(), "Dummy", create_module(config), true)
        });
        core.execution().open();
        assert_eq!(core.execution().state(), ExecutionState::Open);
        assert!(!core.playback.with_output(|_, runner| runner.is_running(index)));
    }

    #[test]
    fn saving_with_the_engine_closed_leaves_disabled_devices_alone() {
        let dir = temp_dir("q7-closed");
        let (core, _, b) = real_core(&dir);
        core.execution().close();
        core.save_system_config().unwrap();
        assert_eq!(core.system.read().disabled_devices, vec![b]);
        assert_eq!(vixen_io::load_system(&core.paths).unwrap().disabled_devices, vec![b]);
    }

    #[test]
    fn saving_with_the_engine_open_records_what_is_not_running() {
        let dir = temp_dir("q7-open");
        let (core, a, b) = real_core(&dir);
        assert!(core.set_preview_running(&b, true));
        assert!(!core.set_preview_running(&a, false));
        core.save_system_config().unwrap();
        assert_eq!(core.system.read().disabled_devices, vec![a]);
        // And the engine starts by that from now on.
        core.execution().close();
        core.execution().open();
        assert_eq!(core.running_previews(), vec![b]);
    }

    #[test]
    fn saving_while_muted_records_nothing() {
        let dir = temp_dir("muted-save");
        let paths = DataPaths::new(&dir);
        paths.ensure_directories().unwrap();
        let mut system = SystemData::default();
        let (_, b) = with_two_previews(&mut system);
        system.disabled_devices = vec![b];
        vixen_io::save_system(&system, &paths).unwrap();
        let core = start(&dir, muted());
        core.save_system_config().unwrap();
        assert_eq!(vixen_io::load_system(&paths).unwrap().disabled_devices, vec![b]);
    }

    #[test]
    fn no_controllers_disables_everything_and_a_save_writes_it() {
        let dir = temp_dir("no-controllers");
        let paths = DataPaths::new(&dir);
        paths.ensure_directories().unwrap();
        let mut system = SystemData::default();
        let (a, b) = with_two_previews(&mut system);
        vixen_io::save_system(&system, &paths).unwrap();
        let core = start(&dir, LaunchOptions { disable_controllers: true, ..LaunchOptions::default() });
        assert!(core.running_previews().is_empty());
        core.save_system_config().unwrap();
        assert_eq!(vixen_io::load_system(&paths).unwrap().disabled_devices, vec![a, b]);
    }

    #[test]
    fn under_test_no_controllers_never_reaches_a_save() {
        let dir = temp_dir("under-test");
        let paths = DataPaths::new(&dir);
        paths.ensure_directories().unwrap();
        let mut system = SystemData::default();
        with_two_previews(&mut system);
        vixen_io::save_system(&system, &paths).unwrap();
        let core = start(&dir, LaunchOptions { disable_controllers: true, ..muted() });
        core.save_system_config().unwrap();
        assert!(vixen_io::load_system(&paths).unwrap().disabled_devices.is_empty());
    }

    #[test]
    fn a_second_save_while_one_runs_returns_false() {
        let dir = temp_dir("double-save");
        let core = start(&dir, muted());
        let guard = Saving::begin(&core.saving_system).unwrap();
        assert!(core.is_saving());
        assert!(!core.save_system_config().unwrap());
        drop(guard);
        assert!(!core.is_saving());
        assert!(core.save_system_config().unwrap());
        assert!(core.save_module_config().unwrap());
        assert!(dir.join("SystemData/SystemConfig.xml").is_file());
        assert!(dir.join("SystemData/ModuleStore.xml").is_file());
    }

    #[test]
    fn a_skipped_system_save_logs_saving_first_and_a_skipped_module_save_does_not() {
        let dir = temp_dir("double-save-log");
        let core = start(&dir, muted());
        let system = Saving::begin(&core.saving_system).unwrap();
        let module = Saving::begin(&core.saving_module).unwrap();
        logging::take_captured();
        assert!(!core.save_system_config().unwrap());
        assert!(!core.save_module_config().unwrap());
        assert_eq!(
            logging::take_captured(),
            [
                "Info|Vixen.Sys.VixenSystem|Saving System Config.",
                "Error|Vixen.Sys.VixenSystem|System config is already being saved. Skipping duplicate request.",
                "Error|Vixen.Sys.VixenSystem|Module config is already being saved. Skipping duplicate request.",
            ]
        );
        drop((system, module));
        assert!(core.save_system_config().unwrap());
        assert!(core.save_module_config().unwrap());
        assert_eq!(
            logging::take_captured(),
            [
                "Info|Vixen.Sys.VixenSystem|Saving System Config.",
                "Info|Vixen.Sys.VixenSystem|System Config saved.",
                "Info|Vixen.Sys.VixenSystem|Saving Module Config.",
                "Info|Vixen.Sys.VixenSystem|Module Config saved.",
            ]
        );
    }

    #[test]
    fn shutdown_saves_application_data_and_only_that_and_removes_the_lock() {
        let dir = temp_dir("shutdown");
        let lock = vixen_io::lock::create_lock_file(&dir).unwrap();
        let core = AppCore::start(dir.clone(), None, muted(), Some(lock.clone()), &NoProgress).unwrap();
        core.system.write().default_update_interval = 123;
        core.shutdown();
        assert_eq!(core.execution().state(), ExecutionState::Closed);
        assert!(!lock.exists());
        assert!(dir.join("VixenApplicationData.xml").is_file());
        assert!(!dir.join("SystemData/SystemConfig.xml").exists(), "only explicit saves write it");
    }

    #[test]
    fn a_lock_that_cannot_be_removed_at_shutdown_is_logged() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = temp_dir("shutdown-lock-stuck");
        let locked_folder = dir.join("elsewhere");
        let lock = vixen_io::lock::create_lock_file(&locked_folder).unwrap();
        let core = AppCore::start(dir.clone(), None, muted(), Some(lock.clone()), &NoProgress).unwrap();
        std::fs::set_permissions(&locked_folder, std::fs::Permissions::from_mode(0o555)).unwrap();
        logging::take_captured();
        core.shutdown();
        std::fs::set_permissions(&locked_folder, std::fs::Permissions::from_mode(0o755)).unwrap();
        let lines = logging::take_captured();
        assert!(lock.exists());
        assert!(
            lines.contains(&"Error|VixenApplication.VixenApplication|An error occurred removing the profile lock file.".to_string()),
            "{lines:?}"
        );
    }

    #[derive(Default)]
    struct Service {
        calls: parking_lot::Mutex<Vec<String>>,
        fail: bool,
    }

    impl AppService for Service {
        fn name(&self) -> &str {
            "Test"
        }
        fn start(&self, _: &AppCore) -> Result<(), String> {
            self.calls.lock().push("start".to_string());
            if self.fail {
                Err("no".to_string())
            } else {
                Ok(())
            }
        }
        fn stop(&self, _: &AppCore) {
            self.calls.lock().push("stop".to_string());
        }
    }

    #[test]
    fn services_start_at_once_once_the_app_has_and_failing_is_not_fatal() {
        let dir = temp_dir("services");
        let core = start(&dir, LaunchOptions::default());
        let first = Arc::new(Service { fail: true, ..Service::default() });
        let second = Arc::new(Service::default());
        core.add_service(first.clone());
        core.add_service(second.clone());
        assert_eq!(*first.calls.lock(), ["start"]);
        assert_eq!(*second.calls.lock(), ["start"]);
        core.shutdown();
        assert_eq!(*second.calls.lock(), ["start", "stop"]);
    }

    #[test]
    fn under_test_services_do_not_start() {
        let dir = temp_dir("services-test");
        let core = start(&dir, muted());
        let service = Arc::new(Service::default());
        core.add_service(service.clone());
        assert!(service.calls.lock().is_empty());
    }

    #[test]
    fn options_read_write_and_save_only_if_changed() {
        let dir = temp_dir("options");
        let core = start(&dir, muted());
        let options = core.options();
        assert_eq!(options.update_interval_ms, 50);
        assert!(options.clear_effect_cache_on_exit);
        assert_eq!(options.video_effect_cache_file_type, "bmp");
        assert!(!core.save_options_if_changed().unwrap());
        core.set_options(&AppOptions {
            update_interval_ms: 73,
            clear_effect_cache_on_exit: false,
            video_effect_cache_file_type: "png".to_string(),
            ..options
        })
        .unwrap();
        assert_eq!(core.session_environment().frame_ms, 73);
        assert!(!dir.join("SystemData/SystemConfig.xml").exists(), "not until the window closes");
        assert!(core.save_options_if_changed().unwrap());
        let saved = vixen_io::load_system(&core.paths).unwrap();
        assert_eq!(saved.default_update_interval, 73);
        assert!(!saved.clear_effect_cache_on_exit);
        assert_eq!(saved.video_effect_cache_file_type, "png");
        assert!(!core.save_options_if_changed().unwrap());
    }

    #[test]
    fn a_stored_interval_above_the_controls_range_clamps_for_display() {
        let dir = temp_dir("clamp");
        let core = start(&dir, muted());
        core.system.write().default_update_interval = 900;
        assert_eq!(core.options().update_interval_ms, 500);
    }

    #[test]
    fn instrumentation_lists_and_resets() {
        let dir = temp_dir("instrumentation");
        let core = start(&dir, muted());
        let lines = core.instrumentation_lines();
        assert!(lines[0].starts_with("System allowed update: "), "{lines:?}");
        assert!(lines.iter().any(|l| l.starts_with("   Contexts update: ")), "{lines:?}");
        assert!(lines.iter().any(|l| l.starts_with("   Filters update: ")), "{lines:?}");
        core.instrumentation_reset();
        assert_eq!(core.instrumentation_lines()[0], "System allowed update: min 999 ms,  avg 0 ms,  max 0 ms,  cnt 1");
    }

    #[test]
    fn a_running_controller_has_its_rows_and_loses_them_when_it_stops() {
        use vixen_output::modules::{create_module, ControllerConfig};
        const DUMMY: Uuid = uuid::uuid!("b6ee6308-189c-4268-8996-32a4bab8ab5f");
        let dir = temp_dir("instrumentation-controller");
        let core = start(&dir, LaunchOptions { open_execution: false, ..LaunchOptions::default() });
        core.playback.with_output(|_, runner| {
            let config = ControllerConfig::new_default(&DUMMY, Uuid::new_v4()).expect("the dummy controller");
            runner.add_controller(Uuid::new_v4(), "Yard", create_module(config), true);
        });
        assert!(!core.instrumentation_lines().iter().any(|l| l.starts_with("Yard ")));
        core.execution().open();
        std::thread::sleep(std::time::Duration::from_millis(80));
        let lines = core.instrumentation_lines();
        let names: Vec<&str> = lines.iter().filter_map(|l| l.strip_prefix("Yard ")).map(|l| l.split(':').next().unwrap()).collect();
        assert_eq!(names, ["Output Total Update", "delta", "refresh rate", "sleep time"], "{lines:?}");
        let total = lines.iter().find(|l| l.starts_with("Yard Output Total Update")).unwrap();
        assert!(!total.contains("cnt 1"), "it has updated: {total}");
        core.instrumentation_reset();
        let lines = core.instrumentation_lines();
        let delta = lines.iter().find(|l| l.starts_with("Yard delta")).unwrap();
        assert!(delta.contains("min 999 ms") || delta.ends_with("cnt 2") || delta.ends_with("cnt 3"), "{delta}");
        core.execution().close();
        assert!(!core.instrumentation_lines().iter().any(|l| l.starts_with("Yard ")));
    }

    #[test]
    fn add_duplicate_rename_delete_previews() {
        let dir = temp_dir("previews");
        let core = start(&dir, muted());
        assert!(!core.preview_changes_made());
        let id = core.add_preview();
        assert!(core.preview_changes_made());
        let rows = core.previews();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Vixen Display Preview");
        assert_eq!(rows[0].type_name, "Vixen Display Preview");
        assert!(rows[0].is_running, "added while the engine is open");
        assert!(core.system.read().module_store.instance(VIXEN_PREVIEW_MODULE_TYPE, id).is_some());

        core.reset_preview_changes();
        core.rename_preview(&id, "Yard");
        assert!(core.preview_changes_made());
        assert_eq!(core.previews()[0].name, "Yard");

        let copies = core.duplicate_previews(&[id]);
        assert_eq!(copies.len(), 1);
        let rows = core.previews();
        assert_eq!(rows[1].name, "Yard-copy");
        {
            let system = core.system.read();
            let entry = system.module_store.instance(VIXEN_PREVIEW_MODULE_TYPE, copies[0]).expect("cloned data");
            assert_eq!(entry.module_instance(), Some(copies[0]));
            assert_eq!(
                entry.data().and_then(|d| d.child_text("ModuleInstanceId")).as_deref(),
                Some(copies[0].to_string().as_str())
            );
        }

        core.delete_previews(&[id]);
        let rows = core.previews();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, copies[0]);
        assert!(core.system.read().module_store.instance(VIXEN_PREVIEW_MODULE_TYPE, id).is_none());
    }

    #[test]
    fn ticking_with_the_engine_closed_does_nothing_and_says_so() {
        let dir = temp_dir("tick");
        let (core, a, b) = real_core(&dir);
        core.execution().close();
        assert!(!core.set_preview_running(&a, true));
        assert!(!core.previews().iter().any(|r| r.is_running));
        core.execution().open();
        assert!(core.set_preview_running(&b, true));
    }

    #[test]
    fn an_edit_sets_both_flags_and_a_tick_only_the_reload() {
        let dir = temp_dir("flags");
        let (core, a, _) = real_core(&dir);
        assert!(!core.preview_changes_made() && !core.preview_needs_reload(), "a fresh session has neither");
        core.set_preview_running(&a, false);
        assert!(!core.preview_changes_made() && core.preview_needs_reload());
        core.reset_preview_changes();
        let edits: [&dyn Fn(); 5] = [
            &|| {
                core.add_preview();
            },
            &|| {
                core.duplicate_previews(&[a]);
            },
            &|| core.rename_preview(&a, "New"),
            &|| core.mark_preview_changed(),
            &|| core.delete_previews(&[a]),
        ];
        for edit in edits {
            edit();
            assert!(core.preview_changes_made() && core.preview_needs_reload());
            core.reset_preview_changes();
        }
    }

    #[test]
    fn the_preview_area_hears_about_changes() {
        let dir = temp_dir("listen");
        let core = start(&dir, muted());
        let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = count.clone();
        core.subscribe_previews(Arc::new(move || {
            counted.fetch_add(1, Ordering::Relaxed);
        }));
        core.add_preview();
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }
}
