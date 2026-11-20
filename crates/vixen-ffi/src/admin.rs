//! The Administration window and the app-level services behind it: starting up, profiles and their lock, the execution engine, saving
//! and reloading the config, Setup Previews, Options, the archive wizard, updates, and the small windows.

use crate::core::{FfiError, VixenCore};
use parking_lot::Mutex;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use uuid::Uuid;
use vixen_io::profiles::{self, ProfileEditor, ProfileLoadError, StartupProfile};
use vixen_io::{DataPaths, LocalDateTime, ProfileSettings};
use vixen_session::app::{AppCore, MAX_RECENT_SEQUENCES};
use vixen_session::archive::{self, ArchiveProgress, ArchiveRequest, ArchiveSelection};
use vixen_session::boot::{self, AppOptions, LaunchOptions, StartupProgress};
use vixen_session::execution::{ExecutionState, ExecutionStatus};
use vixen_session::updates::{self, UpdateCheckRequest, UpdateService};
use vixen_session::version::{current_version, VersionInfo};
use vixen_session::{cpu, logging, modules};

/// One row of the Recent Sequences list.
#[derive(uniffi::Record)]
pub struct RecentSequenceRow {
    /// The file name, as the list shows it.
    pub name: String,
    /// Where that file is in this profile.
    pub path: String,
}

/// How many recent sequences the list keeps.
#[uniffi::export]
pub fn max_recent_sequences() -> u32 {
    MAX_RECENT_SEQUENCES as u32
}

// Launching.

#[derive(uniffi::Record, Clone)]
pub struct LaunchOptionsRecord {
    pub data_dir: Option<String>,
    pub data_dir_from_environment: bool,
    pub disable_controllers: bool,
    pub open_execution: bool,
    pub under_test: bool,
    pub muted: bool,
}

impl From<LaunchOptions> for LaunchOptionsRecord {
    fn from(o: LaunchOptions) -> LaunchOptionsRecord {
        LaunchOptionsRecord {
            data_dir: o.data_dir.map(|p| p.to_string_lossy().into_owned()),
            data_dir_from_environment: o.data_dir_from_environment,
            disable_controllers: o.disable_controllers,
            open_execution: o.open_execution,
            under_test: o.under_test,
            muted: o.muted,
        }
    }
}

impl From<LaunchOptionsRecord> for LaunchOptions {
    fn from(o: LaunchOptionsRecord) -> LaunchOptions {
        LaunchOptions {
            data_dir: o.data_dir.map(PathBuf::from),
            data_dir_from_environment: o.data_dir_from_environment,
            disable_controllers: o.disable_controllers,
            open_execution: o.open_execution,
            under_test: o.under_test,
            muted: o.muted,
        }
    }
}

/// The very first thing the app does: the log files, and logging panics.
#[uniffi::export]
pub fn start_logging() {
    if let Some(logs) = DataPaths::shared_logs_dir() {
        logging::init(&logs);
    }
    logging::install_crash_hooks();
    logging::info("VixenApplication.Program", "Vixen app starting.");
}

/// `LogMessageAndExit`, for an uncaught exception.
#[uniffi::export]
pub fn fatal_exit(message: String) {
    logging::fatal_exit(&message);
}

/// One line in the log, from Swift, under one of the original's logger names.
#[uniffi::export]
pub fn log_info(logger: String, message: String) {
    logging::info(&logger, &message);
}

#[uniffi::export]
pub fn log_warn(logger: String, message: String) {
    logging::warn(&logger, &message);
}

#[uniffi::export]
pub fn log_error(logger: String, message: String, error: Option<String>) {
    logging::error(&logger, &message, error.as_deref());
}

/// The command line and the environment.
#[uniffi::export]
pub fn parse_launch_options(args: Vec<String>) -> LaunchOptionsRecord {
    boot::parse_launch_options(&args, |name| std::env::var(name).ok()).into()
}

/// The folder to use, given the profile chosen, if any.
#[uniffi::export]
pub fn resolve_launch_folder(options: LaunchOptionsRecord, chosen: Option<String>) -> String {
    let options: LaunchOptions = options.into();
    options.data_folder(chosen.map(PathBuf::from)).to_string_lossy().into_owned()
}

/// Why a profile can't load, with F4's text.
#[derive(uniffi::Record)]
pub struct ProfileLoadProblem {
    pub name: String,
    pub folder: String,
    pub message: String,
    pub is_locked: bool,
    /// The machine named by a lock from somewhere else.
    pub locked_by_machine: Option<String>,
}

impl From<ProfileLoadError> for ProfileLoadProblem {
    fn from(e: ProfileLoadError) -> ProfileLoadProblem {
        ProfileLoadProblem { message: e.message(), name: e.name, folder: e.folder, is_locked: e.is_locked, locked_by_machine: e.locked_by_machine }
    }
}

#[derive(uniffi::Enum)]
pub enum StartupProfileChoice {
    NoProfiles,
    Load { folder: String, name: String },
    Ask { error: Option<ProfileLoadProblem> },
}

/// What to do about a profile at launch.
#[uniffi::export]
pub fn decide_startup_profile() -> StartupProfileChoice {
    match profiles::decide_startup_profile(&ProfileSettings::shared()) {
        StartupProfile::NoProfiles => StartupProfileChoice::NoProfiles,
        StartupProfile::Load { folder, name } => StartupProfileChoice::Load { folder, name },
        StartupProfile::Ask { error } => StartupProfileChoice::Ask { error: error.map(Into::into) },
    }
}

/// A row of Select a Profile, or an item of the Data Profile dialog.
#[derive(uniffi::Record)]
pub struct ProfileRow {
    pub name: String,
    pub data_folder: String,
    pub number: i32,
    pub is_locked: bool,
}

/// Select a Profile's rows. Off the main actor: it looks at each folder's lock, and clears stale ones.
#[uniffi::export]
pub fn list_profiles_for_selection() -> Vec<ProfileRow> {
    profiles::list_profiles_for_selection(&ProfileSettings::shared())
        .into_iter()
        .map(|p| ProfileRow { name: p.name, data_folder: p.data_folder, number: p.number, is_locked: p.is_locked })
        .collect()
}

/// `None` if the profile can load.
#[uniffi::export]
pub fn check_profile(folder: String, name: String) -> Option<ProfileLoadProblem> {
    profiles::check_profile(&folder, &name).err().map(Into::into)
}

/// Select a Profile's Load.
#[uniffi::export]
pub fn mark_profile_loaded(number: i32) -> Result<(), FfiError> {
    Ok(profiles::mark_profile_loaded(&ProfileSettings::shared(), number, LocalDateTime::now())?)
}

const APPLICATION_LOGGER: &str = "VixenApplication.VixenApplication";

/// `GetUniqueProcessId`'s log line, written the first time the id is wanted: by a check that finds a lock file, or by taking the lock.
fn log_process_info_once() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        logging::info(APPLICATION_LOGGER, &format!("Process info: {}@{}", std::process::id(), vixen_io::lock::machine_name()));
    });
}

/// `IsProfileLocked`, which clears a stale lock.
#[uniffi::export]
pub fn is_profile_locked(folder: String) -> bool {
    if Path::new(&folder).join(vixen_io::lock::LOCK_FILE).is_file() {
        log_process_info_once();
    }
    let check = vixen_io::lock::check_profile_lock(Path::new(&folder));
    if let Some(error) = &check.remove_error {
        logging::error(APPLICATION_LOGGER, "An error occurred removing the profile lock file.", Some(error));
    }
    if let Some(error) = &check.check_error {
        logging::error(APPLICATION_LOGGER, "An error occurred checking the profile lock file.", Some(error));
    }
    check.locked
}

/// `RemoveLockFile` with its log line.
fn remove_lock_logged(path: &Path) -> bool {
    vixen_io::lock::try_remove_lock_file(path).unwrap_or_else(|error| {
        logging::error(APPLICATION_LOGGER, "An error occurred removing the profile lock file.", Some(&error.to_string()));
        false
    })
}

/// Takes the profile's lock and remembers it for the crash path. Fails if a lock is there.
#[uniffi::export]
pub fn create_lock_file(folder: String) -> Result<String, FfiError> {
    let already_there = Path::new(&folder).join(vixen_io::lock::LOCK_FILE).exists();
    if !already_there {
        log_process_info_once();
    }
    let path = vixen_io::lock::create_lock_file(Path::new(&folder)).inspect_err(|error| {
        if !already_there {
            logging::error(APPLICATION_LOGGER, "An error occurred creating the profile lock file.", Some(&error.to_string()));
        }
    })?;
    logging::set_crash_lock(Some(path.clone()));
    Ok(path.to_string_lossy().into_owned())
}

#[uniffi::export]
pub fn remove_lock_file(path: String) -> bool {
    logging::set_crash_lock(None);
    remove_lock_logged(Path::new(&path))
}

/// The other machine a folder's lock names, if it names one. Only that kind of lock can be opened anyway, since that machine won't clear it.
#[uniffi::export]
pub fn foreign_lock_machine(folder: String) -> Option<String> {
    vixen_io::lock::lock_owner(Path::new(&folder)).filter(|o| !o.is_on_this_machine()).map(|o| o.machine)
}

/// Removes a lock from another machine, because the user chose to open the profile anyway.
#[uniffi::export]
pub fn clear_foreign_lock(folder: String) -> bool {
    let path = Path::new(&folder).join(vixen_io::lock::LOCK_FILE);
    logging::warn(
        APPLICATION_LOGGER,
        &format!("The lock on {folder} named another machine, and was removed at the user's request."),
    );
    remove_lock_logged(&path)
}

// The Data Profile dialog.

/// Why a new profile's name was refused.
#[derive(uniffi::Record)]
pub struct AddProfileProblem {
    pub title: String,
    pub message: String,
}

/// OK's first pass.
#[derive(uniffi::Record)]
pub struct ProfileIssuesRecord {
    pub blank_count: u32,
    pub blank_message: String,
    pub duplicates_message: Option<String>,
    pub invalid_message: Option<String>,
}

/// A folder OK has to ask about.
#[derive(uniffi::Record)]
pub struct FolderPrompt {
    pub folder: String,
    pub prompt: String,
    pub failed_message: String,
}

/// The Data Profile dialog's state, in Rust so Swift only binds.
#[derive(uniffi::Object)]
pub struct ProfileEditorSession {
    editor: Mutex<ProfileEditor>,
}

#[uniffi::export]
impl ProfileEditorSession {
    #[uniffi::constructor]
    pub fn new() -> Arc<ProfileEditorSession> {
        Arc::new(ProfileEditorSession { editor: Mutex::new(ProfileEditor::open(&ProfileSettings::shared())) })
    }

    pub fn items(&self) -> Vec<ProfileRow> {
        self.editor
            .lock()
            .items()
            .into_iter()
            .map(|p| ProfileRow { name: p.name, data_folder: p.data_folder, number: p.number, is_locked: false })
            .collect()
    }

    pub fn selected(&self) -> Option<u32> {
        self.editor.lock().selected().map(|i| i as u32)
    }

    pub fn select(&self, index: u32) {
        self.editor.lock().select(index as usize);
    }

    pub fn set_name(&self, text: String) {
        self.editor.lock().set_name(&text);
    }

    pub fn set_data_folder(&self, text: String) {
        self.editor.lock().set_data_folder(&text);
    }

    /// `None` if it was added.
    pub fn add(&self, name: String) -> Option<AddProfileProblem> {
        self.editor
            .lock()
            .add(&name)
            .err()
            .map(|e| AddProfileProblem { title: e.title().to_string(), message: e.message() })
    }

    pub fn delete(&self, index: u32) {
        self.editor.lock().delete(index as usize);
    }

    pub fn ask_every_time(&self) -> bool {
        self.editor.lock().ask_every_time()
    }

    pub fn set_ask_every_time(&self, ask: bool) {
        self.editor.lock().set_ask_every_time(ask);
    }

    pub fn load_selection(&self) -> Option<u32> {
        self.editor.lock().load_selection().map(|i| i as u32)
    }

    pub fn set_load_selection(&self, index: Option<u32>) {
        self.editor.lock().set_load_selection(index.map(|i| i as usize));
    }

    pub fn validate(&self) -> ProfileIssuesRecord {
        let issues = self.editor.lock().validate();
        ProfileIssuesRecord {
            blank_count: issues.blank_count as u32,
            blank_message: profiles::ProfileIssues::BLANK_MESSAGE.to_string(),
            duplicates_message: issues.duplicates_message(),
            invalid_message: issues.invalid_message(),
        }
    }

    pub fn folders_to_create(&self) -> Vec<FolderPrompt> {
        self.editor
            .lock()
            .folders_to_create()
            .into_iter()
            .map(|(name, folder)| FolderPrompt {
                prompt: profiles::create_folder_prompt(&name, &folder),
                failed_message: profiles::create_folder_failed_message(&folder),
                folder,
            })
            .collect()
    }

    /// False if the folder couldn't be made.
    pub fn create_folder(&self, folder: String) -> bool {
        !folder.is_empty() && std::fs::create_dir_all(&folder).is_ok()
    }

    pub fn save(&self) -> Result<(), FfiError> {
        Ok(self.editor.lock().save(&ProfileSettings::shared())?)
    }
}

// Starting the core.

/// `IProgress<Tuple<int, string>>`, called off the main actor.
#[uniffi::export(callback_interface)]
pub trait StartupProgressListener: Send + Sync {
    fn report(&self, percent: i32, text: String);
}

struct ProgressForwarder(Box<dyn StartupProgressListener>);

impl StartupProgress for ProgressForwarder {
    fn report(&self, percent: i32, text: &str) {
        self.0.report(percent, text.to_string());
    }
}

/// Told each execution state as it's entered, off the main actor.
#[uniffi::export(callback_interface)]
pub trait ExecutionStateListener: Send + Sync {
    fn state_changed(&self, status: ExecutionStatusRecord);
}

/// What the status bar and the Execution Engine menu show.
#[derive(uniffi::Record, Clone)]
pub struct ExecutionStatusRecord {
    pub text: String,
    pub light_red: u8,
    pub light_green: u8,
    pub light_blue: u8,
    pub start_enabled: bool,
    pub stop_enabled: bool,
}

impl From<ExecutionStatus> for ExecutionStatusRecord {
    fn from(s: ExecutionStatus) -> ExecutionStatusRecord {
        ExecutionStatusRecord {
            text: s.text,
            light_red: s.light_rgb.0,
            light_green: s.light_rgb.1,
            light_blue: s.light_rgb.2,
            start_enabled: s.start_enabled,
            stop_enabled: s.stop_enabled,
        }
    }
}

/// A row of Setup Previews. Named apart from the preview area's `PreviewRow`.
#[derive(uniffi::Record)]
pub struct SetupPreviewRow {
    pub id: String,
    pub name: String,
    pub type_name: String,
    pub is_running: bool,
}

/// The Options dialog.
#[derive(uniffi::Record, Clone)]
pub struct OptionsRecord {
    pub update_interval_ms: i32,
    pub audio_latency_ms: i32,
    pub clear_effect_cache_on_exit: bool,
    pub video_effect_cache_file_type: String,
}

fn parse_ids(ids: &[String]) -> Vec<Uuid> {
    ids.iter().filter_map(|id| id.parse().ok()).collect()
}

#[uniffi::export]
impl VixenCore {
    /// `VixenSystem.Start`. Call it off the main actor. `lock_path` is the lock taken for this launch.
    #[uniffi::constructor]
    pub fn start(
        root: String,
        profile_name: Option<String>,
        options: LaunchOptionsRecord,
        lock_path: Option<String>,
        progress: Box<dyn StartupProgressListener>,
    ) -> Result<Arc<VixenCore>, FfiError> {
        let forwarder = ProgressForwarder(progress);
        let core = AppCore::start(PathBuf::from(root), profile_name, options.into(), lock_path.map(PathBuf::from), &forwarder)
            .inspect_err(|error| logging::error("Vixen.Sys.VixenSystem", "Error during system startup!", Some(&error.to_string())))?;
        let weak = Arc::downgrade(&core);
        logging::set_crash_is_saving(Box::new(move || weak.upgrade().is_some_and(|core| core.is_saving())));
        // STUDIO-BEGIN mcp-app-start: serve the same running profile core to local agents
        if let Err(error)=crate::studio_agent::start_server(&core) {
            logging::error("Vixen.Studio.MCP", "Could not start MCP server", Some(&error.to_string()));
        }
        // STUDIO-END mcp-app-start
        vixen_session::studio_heartbeat::start(); // STUDIO: log memory, and report a previous session that died silently
        Ok(Arc::new(VixenCore { inner: core }))
    }

    /// `PopulateRecentSequencesList`: only the files that exist, most recent first.
    pub fn recent_sequences(&self) -> Vec<RecentSequenceRow> {
        self.inner
            .recent_sequences()
            .into_iter()
            .map(|path| RecentSequenceRow {
                name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                path: path.to_string_lossy().into_owned(),
            })
            .collect()
    }

    /// `AddSequenceToRecentList`, saving the list.
    pub fn add_recent_sequence(&self, path: String) -> Result<(), FfiError> {
        Ok(self.inner.add_recent_sequence(&path)?)
    }

    /// `ClearRecentSequencesList`: empties the list. It reaches the disk with the next save, as in the original.
    pub fn clear_recent_sequences(&self) {
        self.inner.clear_recent_sequences();
    }

    /// `VixenApplicationData.SaveData`.
    pub fn save_application_data(&self) -> Result<(), FfiError> {
        Ok(self.inner.save_application_data()?)
    }

    pub fn profile_name(&self) -> Option<String> {
        self.inner.profile_name()
    }

    pub fn execution_status(&self) -> ExecutionStatusRecord {
        self.inner.execution().status().into()
    }

    /// `OpenExecution`. Call it off the main actor, since starting a device can block on a serial port.
    pub fn execution_open(&self) {
        self.inner.execution().open();
    }

    /// `CloseExecution`. Call it off the main actor.
    pub fn execution_close(&self) {
        self.inner.execution().close();
    }

    pub fn subscribe_execution(&self, listener: Box<dyn ExecutionStateListener>) {
        self.inner.execution().subscribe(Box::new(move |state: ExecutionState| {
            listener.state_changed(ExecutionStatus::of(state).into());
        }));
    }

    /// `SaveSystemConfigAsync`. False if one is already running.
    pub fn save_system_config(&self) -> Result<bool, FfiError> {
        Ok(self.inner.save_system_config()?)
    }

    /// `SaveModuleConfigAsync`. False if one is already running.
    pub fn save_module_config(&self) -> Result<bool, FfiError> {
        Ok(self.inner.save_module_config()?)
    }

    /// `ReloadSystemConfig`.
    pub fn reload_system_config(&self) -> Result<(), FfiError> {
        Ok(self.inner.reload_system_config()?)
    }

    pub fn is_saving(&self) -> bool {
        self.inner.is_saving()
    }

    /// `VixenSystem.Stop`, then the application data and the lock.
    pub fn shutdown(&self) {
        self.inner.shutdown();
    }

    pub fn setup_preview_rows(&self) -> Vec<SetupPreviewRow> {
        self.inner
            .previews()
            .into_iter()
            .map(|p| SetupPreviewRow { id: p.id.to_string(), name: p.name, type_name: p.type_name, is_running: p.is_running })
            .collect()
    }

    pub fn setup_add_preview(&self) -> String {
        self.inner.add_preview().to_string()
    }

    pub fn setup_duplicate_previews(&self, ids: Vec<String>) -> Vec<String> {
        self.inner.duplicate_previews(&parse_ids(&ids)).into_iter().map(|id| id.to_string()).collect()
    }

    pub fn setup_delete_previews(&self, ids: Vec<String>) {
        self.inner.delete_previews(&parse_ids(&ids));
    }

    pub fn setup_rename_preview(&self, id: String, name: String) {
        if let Ok(id) = id.parse() {
            self.inner.rename_preview(&id, &name);
        }
    }

    /// Whether it's running after the call.
    pub fn setup_set_preview_running(&self, id: String, running: bool) -> bool {
        id.parse().is_ok_and(|id| self.inner.set_preview_running(&id, running))
    }

    pub fn setup_mark_preview_changed(&self) {
        self.inner.mark_preview_changed();
    }

    /// Cancel has to ask first.
    pub fn setup_preview_changes_made(&self) -> bool {
        self.inner.preview_changes_made()
    }

    /// Cancel has to reload.
    pub fn setup_preview_needs_reload(&self) -> bool {
        self.inner.preview_needs_reload()
    }

    pub fn setup_reset_preview_changes(&self) {
        self.inner.reset_preview_changes();
    }

    pub fn instrumentation_lines(&self) -> Vec<String> {
        self.inner.instrumentation_lines()
    }

    pub fn instrumentation_reset(&self) {
        self.inner.instrumentation_reset();
    }

    pub fn options(&self) -> OptionsRecord {
        let o = self.inner.options();
        OptionsRecord {
            update_interval_ms: o.update_interval_ms,
            audio_latency_ms: o.audio_latency_ms,
            clear_effect_cache_on_exit: o.clear_effect_cache_on_exit,
            video_effect_cache_file_type: o.video_effect_cache_file_type,
        }
    }

    pub fn set_options(&self, options: OptionsRecord) -> Result<(), FfiError> {
        Ok(self.inner.set_options(&AppOptions {
            update_interval_ms: options.update_interval_ms,
            audio_latency_ms: options.audio_latency_ms,
            clear_effect_cache_on_exit: options.clear_effect_cache_on_exit,
            video_effect_cache_file_type: options.video_effect_cache_file_type,
        })?)
    }

    pub fn save_options_if_changed(&self) -> Result<bool, FfiError> {
        Ok(self.inner.save_options_if_changed()?)
    }
}

// Installed Modules, and what New and Open offer.

#[derive(uniffi::Record)]
pub struct ModuleRowRecord {
    pub type_name: String,
    pub description: String,
    pub author: String,
    pub version: String,
    pub file: String,
    pub type_id_text: String,
}

#[derive(uniffi::Record)]
pub struct ModuleGroupRecord {
    pub name: String,
    pub rows: Vec<ModuleRowRecord>,
}

#[uniffi::export]
pub fn installed_modules() -> Vec<ModuleGroupRecord> {
    modules::installed_modules()
        .into_iter()
        .map(|g| ModuleGroupRecord {
            name: g.name,
            rows: g
                .rows
                .into_iter()
                .map(|r| ModuleRowRecord {
                    type_name: r.type_name,
                    description: r.description,
                    author: r.author,
                    version: r.version,
                    file: r.file,
                    type_id_text: r.type_id_text,
                })
                .collect(),
        })
        .collect()
}

#[derive(uniffi::Record)]
pub struct SequenceTypeRecord {
    pub type_name: String,
    pub file_extension: String,
    pub can_create_new: bool,
}

#[uniffi::export]
pub fn sequence_types() -> Vec<SequenceTypeRecord> {
    modules::sequence_types()
        .into_iter()
        .map(|t| SequenceTypeRecord { type_name: t.type_name, file_extension: t.file_extension, can_create_new: t.can_create_new })
        .collect()
}

#[derive(uniffi::Record)]
pub struct FileFilterRecord {
    pub label: String,
    pub extensions: Vec<String>,
}

#[uniffi::export]
pub fn open_panel_filters() -> Vec<FileFilterRecord> {
    modules::open_panel_filters().into_iter().map(|f| FileFilterRecord { label: f.label, extensions: f.extensions }).collect()
}

// The version.

#[derive(uniffi::Record)]
pub struct VersionInfoRecord {
    pub version_name: String,
    pub build_name: String,
    pub current_version_type: String,
    pub release_version: String,
    pub is_test_build: bool,
    pub is_dev_build: bool,
    pub is_release_build: bool,
    pub release_notes_tag: Option<String>,
}

fn version_from_bundle(bundle_build: &str) -> VersionInfo {
    current_version(bundle_build.trim().parse().unwrap_or(0))
}

/// `VersionInfo`. `bundle_build` is `CFBundleVersion`.
#[uniffi::export]
pub fn version_info(bundle_build: String) -> VersionInfoRecord {
    let v = version_from_bundle(&bundle_build);
    VersionInfoRecord {
        version_name: v.version_name(),
        build_name: v.build_name(),
        current_version_type: v.current_version_type().to_string(),
        release_version: v.release_version(),
        is_test_build: v.is_test_build(),
        is_dev_build: v.is_dev_build(),
        is_release_build: v.is_release_build(),
        release_notes_tag: v.release_notes_tag(),
    }
}

// The status bar's CPU reading.

#[derive(uniffi::Object)]
pub struct CpuReader {
    usage: Mutex<cpu::CpuUsage>,
}

#[uniffi::export]
impl CpuReader {
    #[uniffi::constructor]
    pub fn new() -> Arc<CpuReader> {
        Arc::new(CpuReader { usage: Mutex::new(cpu::CpuUsage::new()) })
    }

    /// `None` until there's a reading to show.
    pub fn usage(&self) -> Option<i16> {
        self.usage.lock().get_usage()
    }
}

// Logs and bundled files.

/// Where the log files are.
#[uniffi::export]
pub fn logs_folder() -> String {
    logging::logs_folder().or_else(DataPaths::shared_logs_dir).map(|folder| folder.to_string_lossy().into_owned()).unwrap_or_default()
}

/// System > Logs: the files directly in the log folder, by name.
#[uniffi::export]
pub fn list_log_files() -> Vec<String> {
    logging::logs_folder().or_else(DataPaths::shared_logs_dir).map(|folder| logging::list_log_files(&folder)).unwrap_or_default()
}

/// A bundled text file, decoded by its byte order mark. `None` if it can't be read.
#[uniffi::export]
pub fn bundled_text(path: String) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    Some(boot::decode_text(&bytes))
}

// Updates.

/// A GET's outcome.
#[derive(uniffi::Enum)]
pub enum UpdateHttpOutcome {
    Response { status: u16, body: String, rate_limit_remaining: Option<String> },
    TimedOut,
    Failed { reason: String },
}

/// The HTTP client, in Swift. `get` blocks: it's only called off the main actor.
#[uniffi::export(callback_interface)]
pub trait UpdateHttpClient: Send + Sync {
    fn get(&self, relative_uri: String) -> UpdateHttpOutcome;
}

struct ClientTransport(Box<dyn UpdateHttpClient>);

impl updates::HttpTransport for ClientTransport {
    fn get(&self, relative_uri: &str) -> Result<updates::HttpResponse, updates::TransportError> {
        match self.0.get(relative_uri.to_string()) {
            UpdateHttpOutcome::Response { status, body, rate_limit_remaining } => {
                Ok(updates::HttpResponse { status, body, rate_limit_remaining })
            }
            UpdateHttpOutcome::TimedOut => Err(updates::TransportError::TimedOut),
            UpdateHttpOutcome::Failed { reason } => Err(updates::TransportError::Failed(reason)),
        }
    }
}

/// What the HTTP client sends with every request.
#[derive(uniffi::Record)]
pub struct UpdateHttpSettings {
    pub base: String,
    pub user_agent: String,
    pub accept: String,
    pub api_version_header: String,
    pub api_version: String,
    pub timeout_seconds: u64,
}

#[uniffi::export]
pub fn update_http_settings() -> UpdateHttpSettings {
    UpdateHttpSettings {
        base: updates::API_BASE.to_string(),
        user_agent: updates::USER_AGENT.to_string(),
        accept: updates::ACCEPT.to_string(),
        api_version_header: updates::API_VERSION_HEADER.to_string(),
        api_version: updates::API_VERSION.to_string(),
        timeout_seconds: updates::TIMEOUT_SECONDS,
    }
}

/// What Check for Updates shows.
#[derive(uniffi::Record)]
pub struct UpdateDialogRecord {
    pub heading: String,
    pub version_text: String,
    pub show_download: bool,
    pub show_notes: bool,
    pub notes: String,
    pub release_page_uri: Option<String>,
    /// The status bar's notice, when this check found something newer.
    pub notice: Option<String>,
}

/// `IUpdateService`, with its caches.
#[derive(uniffi::Object)]
pub struct UpdateChecker {
    service: UpdateService,
    version: VersionInfo,
}

#[uniffi::export]
impl UpdateChecker {
    #[uniffi::constructor]
    pub fn github(client: Box<dyn UpdateHttpClient>, bundle_build: String) -> Arc<UpdateChecker> {
        Arc::new(UpdateChecker {
            service: UpdateService::github(Box::new(ClientTransport(client))),
            version: version_from_bundle(&bundle_build),
        })
    }

    /// What a UI test gets: every answer is "unavailable", and nothing reaches the network.
    #[uniffi::constructor]
    pub fn unavailable(bundle_build: String) -> Arc<UpdateChecker> {
        Arc::new(UpdateChecker { service: UpdateService::unavailable(), version: version_from_bundle(&bundle_build) })
    }

    /// Check for Updates. Call it off the main actor, and only because the user asked.
    pub fn check_for_updates(&self) -> UpdateDialogRecord {
        let result = self.service.check(&UpdateCheckRequest::for_version(&self.version, true));
        let content = updates::update_dialog_content(&self.version, result.as_ref());
        let notice = result.as_ref().and_then(|r| updates::update_notice(&self.version, r));
        if let Some(notice) = &notice {
            logging::info("VixenApplication.VixenApplication", notice);
        }
        UpdateDialogRecord {
            heading: content.heading,
            version_text: content.version_text,
            show_download: content.show_download,
            show_notes: content.show_notes,
            notes: content.notes,
            release_page_uri: content.release_page_uri,
            notice,
        }
    }

    /// Release Notes' text. `bundle_dir` holds `Release Notes.txt`. Call it off the main actor, and only because the user asked.
    pub fn release_notes_text(&self, bundle_dir: String) -> String {
        updates::release_notes_text(&self.service, self.version.release_notes_tag().as_deref(), Path::new(&bundle_dir))
    }
}

// The Profile Archive Wizard.

/// A choice in Profile to archive.
#[derive(uniffi::Record)]
pub struct ArchiveProfileChoice {
    pub name: String,
    pub folder: String,
}

#[uniffi::export]
pub fn archive_profile_choices() -> Vec<ArchiveProfileChoice> {
    profiles::archive_profile_choices(&ProfileSettings::shared())
        .into_iter()
        .map(|(name, folder)| ArchiveProfileChoice { name, folder })
        .collect()
}

/// Everything Start reads from the window.
#[derive(uniffi::Record, Clone)]
pub struct ArchiveRequestRecord {
    pub profile_folder: String,
    pub save_folder: String,
    pub file_name: String,
    pub module: bool,
    pub program: bool,
    pub sequence: bool,
    pub system: bool,
    pub template: bool,
    pub logs: bool,
    pub media: bool,
    pub user_settings: bool,
}

impl From<&ArchiveRequestRecord> for ArchiveRequest {
    fn from(r: &ArchiveRequestRecord) -> ArchiveRequest {
        ArchiveRequest {
            profile_folder: PathBuf::from(&r.profile_folder),
            save_folder: r.save_folder.clone(),
            file_name: r.file_name.clone(),
            selection: ArchiveSelection {
                module: r.module,
                program: r.program,
                sequence: r.sequence,
                system: r.system,
                template: r.template,
                logs: r.logs,
                media: r.media,
                user_settings: r.user_settings,
            },
        }
    }
}

/// Where Start has to stop.
#[derive(uniffi::Record)]
pub struct ArchiveIssueRecord {
    pub title: String,
    pub message: String,
    /// A question, answered yes with `resolve_archive_prompt`.
    pub is_question: bool,
    /// Whether yes means replacing a file, rather than making a folder.
    pub replaces_file: bool,
    pub is_error: bool,
}

/// `buttonStartCancel_Click`'s checks. `None` if it can start.
#[uniffi::export]
pub fn check_archive_request(request: ArchiveRequestRecord) -> Option<ArchiveIssueRecord> {
    archive::check_archive_request(&(&request).into()).err().map(|issue| ArchiveIssueRecord {
        title: issue.title().to_string(),
        message: issue.message().to_string(),
        is_question: issue.is_question(),
        replaces_file: issue == archive::ArchiveIssue::FileExists,
        is_error: issue.is_error(),
    })
}

/// The file name with a trailing `.zip` removed, which goes back in the field.
#[uniffi::export]
pub fn trimmed_archive_file_name(name: String) -> String {
    archive::trimmed_file_name(&name).to_string()
}

/// The yes to a question: makes the folder, or deletes the file being replaced.
#[uniffi::export]
pub fn resolve_archive_prompt(request: ArchiveRequestRecord) -> Result<(), FfiError> {
    archive::resolve_archive_prompt(&(&request).into())?;
    Ok(())
}

/// Told how a run goes, off the main actor.
#[uniffi::export(callback_interface)]
pub trait ArchiveListener: Send + Sync {
    fn status(&self, text: String);
    fn value(&self, percent: i32);
    /// The end of the run. `error_message` is the alert to show, if it failed.
    fn finished(&self, error_message: Option<String>);
}

struct ListenerProgress(Arc<dyn ArchiveListener>);

impl ArchiveProgress for ListenerProgress {
    fn status(&self, text: &str) {
        self.0.status(text.to_string());
    }

    fn value(&self, percent: i32) {
        self.0.value(percent);
    }
}

/// One run of the wizard, on its own thread.
#[derive(uniffi::Object)]
pub struct ArchiveTask {
    cancel: Arc<AtomicBool>,
}

#[uniffi::export]
impl ArchiveTask {
    /// Starts the run. Check the request first and answer its questions: anything still wrong is reported through `finished`.
    #[uniffi::constructor]
    pub fn start(request: ArchiveRequestRecord, listener: Box<dyn ArchiveListener>) -> Arc<ArchiveTask> {
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let listener: Arc<dyn ArchiveListener> = Arc::from(listener);
        let request: ArchiveRequest = (&request).into();
        let spawned = {
            let listener = listener.clone();
            std::thread::Builder::new().name("profile archive".to_string()).spawn(move || {
                let error = match archive::check_archive_request(&request) {
                    Ok(prepared) => archive::run_archive(&prepared, &flag, &ListenerProgress(listener.clone()))
                        .err()
                        .map(|e| archive::error_alert_text(&e)),
                    Err(issue) => Some(issue.message().to_string()),
                };
                listener.finished(error);
            })
        };
        if let Err(error) = spawned {
            listener.finished(Some(format!("An error occurred during the zip process.\n\r {error}")));
        }
        Arc::new(ArchiveTask { cancel })
    }

    /// Stop: the run ends after the current file.
    pub fn stop(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
