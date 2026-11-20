//! The scheduler and export area: the Export Sequence dialog, so far. See `docs/spec/shows-scheduler-export.md` and its parity table.

use crate::core::{FfiError, VixenCore};
mod element_mapper;
mod package;
mod song_inventory;
mod vixen2;
use crate::sequence::SequencerSession;
use parking_lot::Mutex;
use std::path::Path;
use std::sync::Arc;
use vixen_io::settings::ProfileSettings;
use vixen_session::app::AppCore;
use vixen_session::export::controller_info_with_network;
use vixen_session::export_dialog::{self as dialog, ExportDialogState, ExportListener, ExportResult, ExportRun, PreviewHold};
use vixen_session::logging;
use vixen_session::sequence::SequenceSession;

const LOGGER: &str = "VixenModules.Editor.TimedSequenceEditor.ExportDialog";

/// One row of the Network list.
#[derive(uniffi::Record, Clone, PartialEq, Eq)]
pub struct ExportControllerRow {
    pub id: String,
    pub name: String,
    pub channels: i32,
    pub active: bool,
    pub start: Option<i32>,
    pub end: Option<i32>,
}

/// Everything the dialog's controls show.
#[derive(uniffi::Record, Clone, PartialEq, Eq)]
pub struct ExportDialogView {
    /// The Format list, in its sorted order.
    pub format_names: Vec<String>,
    pub format_index: u32,
    /// The Timing list's items.
    pub timing_choices: Vec<String>,
    pub timing_text: String,
    pub generate: bool,
    /// FPP 2.x, rather than FPP 1.x.
    pub fpp2x: bool,
    pub compress: bool,
    pub radios_enabled: bool,
    pub compress_enabled: bool,
    pub rows: Vec<ExportControllerRow>,
    /// F6's text.
    pub note: String,
    /// The sequence's file name without extension, for `Saving: {name}`.
    pub sequence_name: String,
}

/// What the save panel starts with.
#[derive(uniffi::Record, Clone, PartialEq, Eq)]
pub struct ExportSavePanel {
    pub directory: String,
    pub file_name: String,
    /// The filter's name, the format's.
    pub format_name: String,
    pub extension: String,
}

/// Start's answer before any work begins.
#[derive(uniffi::Enum, Clone, PartialEq, Eq)]
pub enum ExportStartOutcome {
    Started,
    /// Upstream's alert: title `Error`, a warning icon, OK.
    InvalidTiming { message: String },
}

/// How a run ended.
#[derive(uniffi::Record, Clone, PartialEq, Eq)]
pub struct ExportFinish {
    pub cancelled: bool,
    pub frames: i32,
    /// For an alert titled `Export Failed`.
    pub error: Option<String>,
    /// For an alert titled `Warning`, with a warning icon.
    pub falcon_warning: Option<String>,
    pub controller_file: Option<String>,
    /// The status strip afterwards: `Export Complete` or `Export Canceled`.
    pub status_text: String,
}

/// Where a run reports, from its own thread.
#[uniffi::export(callback_interface)]
pub trait ExportDialogListener: Send + Sync {
    fn progress(&self, percent: i32);
    fn finished(&self, result: ExportFinish);
}

struct Forward(Box<dyn ExportDialogListener>);

impl ExportListener for Forward {
    fn progress(&self, percent: i32) {
        self.0.progress(percent);
    }

    fn finished(&self, result: ExportResult) {
        let status_text = if result.error.is_some() {
            "Export Failed"
        } else if result.cancelled {
            "Export Canceled"
        } else {
            "Export Complete"
        };
        if let Some(error) = &result.error {
            logging::error(LOGGER, "Export failed", Some(error));
        }
        self.0.finished(ExportFinish {
            cancelled: result.cancelled,
            frames: result.frames,
            error: result.error,
            falcon_warning: result.falcon_warning.then(|| dialog::FALCON_WARNING.to_string()),
            controller_file: result.controller_file.map(|p| p.to_string_lossy().into_owned()),
            status_text: status_text.to_string(),
        });
    }
}

/// The Export Sequence dialog for one open sequence: `ExportDialog`. While it exists the Effect Preview stays stopped (F1). Dropping it
/// stops a run that's still going and waits for it.
#[derive(uniffi::Object)]
pub struct ExportDialog {
    core: Arc<AppCore>,
    session: Arc<Mutex<SequenceSession>>,
    settings: Arc<ProfileSettings>,
    state: Mutex<ExportDialogState>,
    run: Mutex<Option<ExportRun>>,
    _hold: PreviewHold,
}

impl ExportDialog {
    fn put(&self, result: Result<(), vixen_io::IoError>) {
        // A setting that can't be saved isn't worth stopping an export for. Upstream would have thrown.
        if let Err(error) = result {
            logging::error(LOGGER, "Could not save the export dialog's settings", Some(&error.to_string()));
        }
    }
}

#[uniffi::export]
impl SequencerSession {
    /// F1: the Export Sequence dialog for this sequence as it stands, unsaved edits included. The controller list reads each controller's
    /// network configuration, which can look up a host name, so call this off the main thread.
    pub fn export_dialog(&self) -> Arc<ExportDialog> {
        let controllers = controller_info_with_network(&self.core.system.read());
        let settings = ProfileSettings::shared();
        let state = ExportDialogState::load(&settings, controllers);
        Arc::new(ExportDialog {
            core: Arc::clone(&self.core),
            session: Arc::clone(&self.session),
            settings,
            state: Mutex::new(state),
            run: Mutex::new(None),
            _hold: PreviewHold::new(Arc::clone(&self.core)),
        })
    }
}

#[uniffi::export]
impl ExportDialog {
    pub fn view(&self) -> ExportDialogView {
        let state = self.state.lock();
        ExportDialogView {
            format_names: dialog::format_names().into_iter().map(str::to_string).collect(),
            format_index: state.format_index() as u32,
            timing_choices: dialog::TIMING_CHOICES.iter().map(|s| s.to_string()).collect(),
            timing_text: state.timing_text().to_string(),
            generate: state.generate(),
            fpp2x: state.fpp2x(),
            compress: state.compress(),
            radios_enabled: state.universe_radios_enabled(),
            compress_enabled: state.compress_enabled(),
            rows: state
                .rows()
                .into_iter()
                .map(|r| ExportControllerRow { id: r.id.to_string(), name: r.name, channels: r.channels, active: r.active, start: r.start, end: r.end })
                .collect(),
            note: dialog::NOTE.to_string(),
            sequence_name: self.session.lock().path().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
        }
    }

    pub fn set_format(&self, index: u32) {
        let result = self.state.lock().set_format_index(index as usize, &self.settings);
        self.put(result);
    }

    /// A Timing list item.
    pub fn choose_timing(&self, index: u32) {
        let result = self.state.lock().choose_timing(index as usize, &self.settings);
        self.put(result);
    }

    /// Text typed into the Timing box: used, never saved.
    pub fn set_timing_text(&self, text: String) {
        self.state.lock().set_timing_text(&text);
    }

    pub fn set_generate(&self, on: bool) {
        let result = self.state.lock().set_generate(on, &self.settings);
        self.put(result);
    }

    pub fn set_fpp2x(&self, on: bool) {
        let result = self.state.lock().set_fpp2x(on, &self.settings);
        self.put(result);
    }

    pub fn set_compress(&self, on: bool) {
        let result = self.state.lock().set_compress(on, &self.settings);
        self.put(result);
    }

    pub fn set_row_active(&self, row: u32, on: bool) {
        self.state.lock().set_row_active(row as usize, on);
    }

    /// A drag in the list, in `move(fromOffsets:toOffset:)`'s terms.
    pub fn move_rows(&self, from: Vec<u32>, to: u32) {
        let from: Vec<usize> = from.into_iter().map(|i| i as usize).collect();
        self.state.lock().move_rows(&from, to as usize);
    }

    /// F9 steps 1 to 3. `None` for a sequence never saved, for which Start does nothing.
    pub fn save_panel(&self) -> Result<Option<ExportSavePanel>, FfiError> {
        let path = self.session.lock().path().map(Path::to_path_buf);
        let request = self.state.lock().save_request(path.as_deref(), &self.core.paths)?;
        Ok(request.map(|r| ExportSavePanel {
            directory: r.directory.to_string_lossy().into_owned(),
            file_name: r.file_name,
            format_name: r.format_name,
            extension: r.extension,
        }))
    }

    /// F9 from step 4, to the file the save panel gave. The run reports to `listener` from its own thread.
    pub fn start(&self, out_path: String, listener: Box<dyn ExportDialogListener>) -> Result<ExportStartOutcome, FfiError> {
        let mut run = self.run.lock();
        if run.as_ref().is_some_and(ExportRun::is_running) {
            return Err(FfiError::new("An export is already running."));
        }
        let job = match self.state.lock().job(Path::new(&out_path)) {
            Ok(job) => job,
            Err(message) => return Ok(ExportStartOutcome::InvalidTiming { message }),
        };
        let started = dialog::start_export(Arc::clone(&self.core), Arc::clone(&self.session), job, Arc::new(Forward(listener)))
            .map_err(|e| FfiError::new(e.to_string()))?;
        *run = Some(started);
        Ok(ExportStartOutcome::Started)
    }

    /// Stop: the run ends at its next interval and reports `finished`.
    pub fn stop(&self) {
        if let Some(run) = self.run.lock().as_ref() {
            run.cancel();
        }
    }

    pub fn is_running(&self) -> bool {
        self.run.lock().as_ref().is_some_and(ExportRun::is_running)
    }

    /// Stop and Close: stops a run and waits for it to finish cleaning up.
    pub fn close(&self) {
        if let Some(run) = self.run.lock().take() {
            run.cancel();
            run.join();
        }
    }
}

// The Export Wizard, F14 to F36.

use vixen_session::export::default_controller_info;
use vixen_session::export_wizard::profile::{load_profiles, save_profiles};
use vixen_session::export_wizard::run::{start_bulk, BulkListener, BulkResult, BulkRun};
use vixen_session::export_wizard::{device, ExportWizard, Stage, FALCON_NOTE};

/// A refused profile name: the alert's title and text.
#[derive(uniffi::Record, Clone, PartialEq, Eq)]
pub struct WizardAlert {
    pub title: String,
    pub message: String,
}

/// The Summary stage's rows.
#[derive(uniffi::Record, Clone, PartialEq, Eq)]
pub struct WizardSummary {
    pub sequences: u32,
    pub format: String,
    pub timing: String,
    pub destination: String,
    pub audio: String,
    pub audio_destination: Option<String>,
    pub universe_destination: Option<String>,
    pub warning: Option<String>,
    /// F28, Direct Upload: `FPP Device Info`, or that it couldn't be had, once the device has been asked.
    pub device_heading: Option<String>,
    pub device_rows: Vec<WizardDeviceRow>,
}

#[derive(uniffi::Record, Clone, PartialEq, Eq)]
pub struct WizardDeviceRow {
    pub label: String,
    pub value: String,
}

/// Everything the wizard's window shows.
#[derive(uniffi::Record, Clone, PartialEq, Eq)]
pub struct WizardView {
    /// 0 Config, 1 Sources, 2 Controllers, 3 Output Format, 4 Summary, 5 Finished.
    pub stage: u32,
    pub can_next: bool,
    pub can_previous: bool,
    pub running: bool,
    pub profile_names: Vec<String>,
    pub selected_profile: u32,
    /// Full paths; the list shows file names.
    pub sequence_files: Vec<String>,
    pub controllers: Vec<ExportControllerRow>,
    pub enable_all_enabled: bool,
    pub disable_all_enabled: bool,
    pub format_names: Vec<String>,
    pub format_index: u32,
    pub timing_choices: Vec<String>,
    pub interval_text: String,
    pub shows_falcon_group: bool,
    pub shows_universe_options: bool,
    pub compress_enabled: bool,
    pub falcon_include_audio: bool,
    pub include_audio: bool,
    pub rename_audio: bool,
    pub create_universe_file: bool,
    pub backup_universe_file: bool,
    pub compress: bool,
    pub sequence_folder: String,
    pub audio_folder: String,
    pub falcon_folder: String,
    pub falcon_note: String,
    /// F26: the `File Path` and `Direct Upload` radios show, and which is chosen.
    pub shows_direct_upload: bool,
    pub direct_upload: bool,
    pub host_text: String,
    pub preparing_network: bool,
    pub network_error: Option<String>,
    pub summary: Option<WizardSummary>,
    pub save_settings: bool,
    pub save_name: String,
    pub final_text: String,
}

/// Where a wizard run reports, from its own thread.
#[uniffi::export(callback_interface)]
pub trait ExportWizardListener: Send + Sync {
    fn task(&self, text: String, percent: i32);
    fn overall(&self, text: String, percent: i32);
    /// `Error Loading Sequence`: block until the user answers; true continues with the rest.
    fn load_failed(&self, path: String, message: String) -> bool;
    /// The Summary's rows changed while the run was going: the device turned out to be an ESPixelStick.
    fn summary_changed(&self);
    /// The run has ended and the wizard is on its last stage. `error` is for an alert titled `Error`, and `upload_alert` is F34's.
    fn finished(&self, final_text: String, error: Option<String>, upload_alert: Option<WizardAlert>);
}

/// The open Export Wizard.
#[derive(uniffi::Object)]
pub struct ExportWizardSession {
    core: Arc<AppCore>,
    wizard: Arc<Mutex<ExportWizard>>,
    run: Mutex<Option<BulkRun>>,
}

struct WizardForward {
    listener: Box<dyn ExportWizardListener>,
    wizard: Arc<Mutex<ExportWizard>>,
}

impl BulkListener for WizardForward {
    fn task(&self, text: &str, percent: i32) {
        self.listener.task(text.to_string(), percent);
    }
    fn overall(&self, text: &str, percent: i32) {
        self.listener.overall(text.to_string(), percent);
    }
    fn load_failed(&self, path: &str, message: &str) -> bool {
        self.listener.load_failed(path.to_string(), message.to_string())
    }
    fn esp_pixel_stick(&self, supports_zip: bool) {
        self.wizard.lock().set_esp_detected(supports_zip);
        self.listener.summary_changed();
    }
    fn finished(&self, result: BulkResult) {
        self.wizard.lock().finish(&result.final_text);
        if let Some(error) = &result.error {
            logging::error("VixenModules.App.ExportWizard.BulkExportSummaryStage", "Export failed", Some(error));
        }
        let upload_alert = result.upload_error.map(|message| WizardAlert { title: "FPP Upload Error".to_string(), message: format!("FPP direct upload failed:\n{message}") });
        self.listener.finished(result.final_text, result.error, upload_alert);
    }
}

fn folder_exists(path: &str) -> bool {
    Path::new(path).is_dir()
}

impl ExportWizardSession {
    fn live(&self) -> Vec<dialog::ControllerInfo> {
        default_controller_info(&self.core.system.read())
    }

    /// `SaveModuleConfigAsync`, with the message for a failure.
    fn save(&self) -> Option<String> {
        let profiles = self.wizard.lock().profiles().to_vec();
        save_profiles(&mut self.core.system.write().module_store, &profiles);
        self.core.save_module_config().err().map(|e| e.to_string())
    }
}

#[uniffi::export]
impl VixenCore {
    /// F14: the wizard over the saved profiles, on its first stage.
    pub fn export_wizard(&self) -> Arc<ExportWizardSession> {
        let core = Arc::clone(&self.inner);
        let wizard = {
            let system = core.system.read();
            ExportWizard::new(core.paths.clone(), load_profiles(&system.module_store), &system)
        };
        Arc::new(ExportWizardSession { core, wizard: Arc::new(Mutex::new(wizard)), run: Mutex::new(None) })
    }
}

#[uniffi::export]
impl ExportWizardSession {
    pub fn view(&self) -> WizardView {
        let live = self.live();
        let running = self.run.lock().as_ref().is_some_and(BulkRun::is_running);
        let w = self.wizard.lock();
        let active = w.active();
        let text = |f: Option<&Option<String>>| f.and_then(|v| v.clone()).unwrap_or_default();
        let summary = if w.stage() == Stage::Summary {
            w.summary(&live).map(|s| WizardSummary {
                sequences: s.sequences as u32,
                format: s.format,
                timing: s.timing,
                destination: s.destination,
                audio: s.audio,
                audio_destination: s.audio_destination,
                universe_destination: s.universe_destination,
                warning: s.warning,
                device_heading: s.device_heading,
                device_rows: s.device_rows.into_iter().map(|(label, value)| WizardDeviceRow { label, value }).collect(),
            })
        } else {
            None
        };
        WizardView {
            stage: w.stage().index() as u32,
            can_next: !running && w.can_move_next(&folder_exists),
            can_previous: !running && w.can_move_previous(),
            running,
            profile_names: w.profile_names().into_iter().map(str::to_string).collect(),
            selected_profile: w.selected() as u32,
            sequence_files: w.sequence_files().to_vec(),
            controllers: w
                .controller_rows(&live)
                .into_iter()
                .map(|r| ExportControllerRow { id: r.id.to_string(), name: r.name, channels: r.channels, active: r.active, start: r.start, end: r.end })
                .collect(),
            enable_all_enabled: w.enable_all_enabled(),
            disable_all_enabled: w.disable_all_enabled(),
            format_names: dialog::format_names().into_iter().map(str::to_string).collect(),
            format_index: active
                .and_then(|a| dialog::format_names().iter().position(|n| *n == a.format_text()))
                .unwrap_or(0) as u32,
            timing_choices: dialog::TIMING_CHOICES.iter().map(|s| s.to_string()).collect(),
            interval_text: w.interval_text().to_string(),
            shows_falcon_group: w.shows_falcon_group(),
            shows_universe_options: w.shows_universe_options(),
            compress_enabled: w.compress_enabled(),
            falcon_include_audio: w.falcon_audio_box(),
            include_audio: w.audio_box(),
            rename_audio: active.is_some_and(|a| a.rename_audio),
            create_universe_file: active.is_some_and(|a| a.create_universe_file),
            backup_universe_file: active.is_some_and(|a| a.backup_universe_file),
            compress: active.is_some_and(|a| a.enable_compression),
            sequence_folder: text(active.map(|a| &a.output_folder)),
            audio_folder: text(active.map(|a| &a.audio_output_folder)),
            falcon_folder: text(active.map(|a| &a.falcon_output_folder)),
            falcon_note: FALCON_NOTE.to_string(),
            shows_direct_upload: w.shows_direct_upload(),
            direct_upload: w.direct_upload(),
            host_text: w.host_text().to_string(),
            preparing_network: w.network_request().is_some(),
            network_error: w.network_error().map(str::to_owned),
            summary,
            save_settings: w.save_settings,
            save_name: w.save_name.clone(),
            final_text: w.final_text().to_string(),
        }
    }

    /// `Next >>` on the first four stages. The Summary's Next is [`ExportWizardSession::start`].
    pub fn next(&self) -> bool {
        let system = self.core.system.read();
        self.wizard.lock().next(&system, &folder_exists)
    }

    pub fn previous(&self) -> bool {
        let system = self.core.system.read();
        self.wizard.lock().previous(&system)
    }

    pub fn select_profile(&self, index: u32) {
        self.wizard.lock().select_profile(index as usize);
    }

    /// F17. `None` when added; otherwise the alert, after which the prompt asks again.
    pub fn add_profile(&self, name: String) -> Option<WizardAlert> {
        let system = self.core.system.read();
        self.wizard.lock().add_profile(&name, &system).err().map(|e| WizardAlert { title: e.title().to_string(), message: e.message() })
    }

    /// F20. A refused name gives its alert; a failed save gives `Save Error`'s.
    pub fn rename_profile(&self, name: String) -> Option<WizardAlert> {
        let renamed = self.wizard.lock().rename_profile(&name);
        match renamed {
            Err(e) => Some(WizardAlert { title: e.title().to_string(), message: e.message() }),
            Ok(false) => None,
            Ok(true) => self.save().map(|m| WizardAlert {
                title: "Save Error".to_string(),
                message: format!("The profile was renamed but could not be saved to disk:\n{m}"),
            }),
        }
    }

    /// F18, after `Are you sure you want to delete {name}?`.
    pub fn delete_profile(&self) -> Option<WizardAlert> {
        {
            let system = self.core.system.read();
            self.wizard.lock().delete_profile(&system);
        }
        self.save().map(|m| WizardAlert { title: "Save Error".to_string(), message: format!("The profile was removed but could not be saved to disk:\n{m}") })
    }

    pub fn add_sequences(&self, paths: Vec<String>) {
        let files: Vec<std::path::PathBuf> = paths.into_iter().map(std::path::PathBuf::from).collect();
        self.wizard.lock().add_sequences(&files);
    }

    pub fn add_all_sequences(&self) {
        self.wizard.lock().add_all_sequences();
    }

    pub fn remove_sequences(&self, rows: Vec<u32>) {
        let rows: Vec<usize> = rows.into_iter().map(|r| r as usize).collect();
        self.wizard.lock().remove_sequences(&rows);
    }

    /// The folder the Add panel offers, `<data root>/Sequence`.
    pub fn sequence_folder_path(&self) -> String {
        self.core.paths.sequence_dir().to_string_lossy().into_owned()
    }

    pub fn set_controllers_active(&self, rows: Vec<u32>, on: bool) {
        let live = self.live();
        let rows: Vec<usize> = rows.into_iter().map(|r| r as usize).collect();
        self.wizard.lock().set_controllers_active(&rows, on, &live);
    }

    pub fn set_all_controllers(&self, on: bool) {
        let live = self.live();
        self.wizard.lock().set_all_controllers(on, &live);
    }

    pub fn move_controllers(&self, from: Vec<u32>, to: u32) {
        let live = self.live();
        let from: Vec<usize> = from.into_iter().map(|r| r as usize).collect();
        self.wizard.lock().move_controllers(&from, to as usize, &live);
    }

    pub fn set_format(&self, index: u32) {
        self.wizard.lock().set_format(index as usize);
    }

    pub fn set_interval_text(&self, text: String) {
        self.wizard.lock().set_interval_text(&text);
    }

    pub fn set_include_audio(&self, falcon_box: bool, on: bool) {
        self.wizard.lock().set_include_audio(falcon_box, on);
    }

    pub fn set_rename_audio(&self, on: bool) {
        self.wizard.lock().set_rename_audio(on);
    }

    pub fn set_create_universe_file(&self, on: bool) {
        self.wizard.lock().set_create_universe_file(on);
    }

    pub fn set_backup_universe_file(&self, on: bool) {
        self.wizard.lock().set_backup_universe_file(on);
    }

    pub fn set_compression(&self, on: bool) {
        self.wizard.lock().set_compression(on);
    }

    pub fn set_sequence_folder(&self, path: String) {
        self.wizard.lock().set_output_folder(&path);
    }

    pub fn set_audio_folder(&self, path: String) {
        self.wizard.lock().set_audio_folder(&path);
    }

    pub fn set_falcon_folder(&self, path: String) {
        self.wizard.lock().set_falcon_folder(&path);
    }

    /// F26: the `File Path` (false) and `Direct Upload` (true) radios.
    pub fn set_direct_upload(&self, on: bool) {
        self.wizard.lock().set_direct_upload(on);
    }

    /// Typing in the `FPP Host / IP:` box.
    pub fn set_host(&self, text: String) {
        self.wizard.lock().set_host_text(&text);
    }

    /// F26's reachability check for the host box as it is now, which waits up to half a second: call it off the main thread, on
    /// leaving the box (`alert` true) and on opening the stage (false). The alert is for a host that didn't answer.
    pub fn check_host(&self, alert: bool) -> Option<WizardAlert> {
        device::check_host(&self.wizard, alert).map(|(title, message)| WizardAlert { title, message })
    }

    /// F28: asks a direct upload's device what it is, which waits on the network: call it off the main thread when the Summary
    /// opens. True when the Summary's rows changed.
    pub fn ask_device(&self) -> bool {
        device::ask_device(&self.wizard)
    }

    /// F27: call on a worker after entering Summary. A result for an abandoned visit is ignored.
    pub fn prepare_network(&self) -> bool {
        let Some(generation) = self.wizard.lock().network_request() else { return false; };
        let test_error = if std::env::var_os("VIXEN_UI_TEST").is_some() {
            if let Ok(delay) = std::env::var("VIXEN_TEST_NETWORK_PREPARE_DELAY_MS") {
                std::thread::sleep(std::time::Duration::from_millis(delay.parse::<u64>().unwrap_or(0).min(5000)));
            }
            std::env::var("VIXEN_TEST_NETWORK_PREPARE_ERROR").ok()
        } else { None };
        let result = if let Some(message) = test_error {
            Err(message)
        } else {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| controller_info_with_network(&self.core.system.read())))
                .map_err(|_| "Could not read controller network configuration".to_string())
        };
        self.wizard.lock().complete_network(generation, result)
    }

    /// Leaving a typed folder box: whether to offer `The selected output path does not exist. Do you want to create it?`. GUARD: a path that
    /// isn't an absolute one on this Mac is never offered.
    pub fn should_offer_to_create(&self, path: String) -> bool {
        !path.trim().is_empty() && !vixen_session::export_wizard::profile::is_foreign_path(&path) && !Path::new(&path).is_dir()
    }

    /// The prompt's OK. The error's text goes in `Unable to create the target directory.\n{message}`.
    pub fn create_folder(&self, path: String) -> Result<(), FfiError> {
        std::fs::create_dir_all(&path).map_err(|e| FfiError::new(e.to_string()))
    }

    pub fn set_save_settings(&self, on: bool) {
        self.wizard.lock().save_settings = on;
    }

    pub fn set_save_name(&self, name: String) {
        self.wizard.lock().save_name = name;
    }

    /// The Summary's Next: F29's save, then the run. The wizard moves to Finished when the run ends.
    pub fn start(&self, listener: Box<dyn ExportWizardListener>) -> Result<(), FfiError> {
        let mut run = self.run.lock();
        if run.as_ref().is_some_and(BulkRun::is_running) {
            return Err(FfiError::new("The export is already running."));
        }
        let (profile, changed) = {
            let mut w = self.wizard.lock();
            if w.stage() != Stage::Summary || !w.can_move_next(&folder_exists) {
                return Err(FfiError::new("The wizard isn't ready to export."));
            }
            let changed = w.apply_save_settings();
            let profile = w.active().cloned().ok_or_else(|| FfiError::new("No profile is chosen."))?;
            (profile, changed)
        };
        if changed {
            // As upstream, a failed save here is only logged.
            if let Some(message) = self.save() {
                logging::error("VixenModules.App.ExportWizard.BulkExportSummaryStage", "Could not save the export profiles", Some(&message));
            }
        }
        let forward = WizardForward { listener, wizard: Arc::clone(&self.wizard) };
        *run = Some(start_bulk(Arc::clone(&self.core), profile, Arc::new(forward)).map_err(|e| FfiError::new(e.to_string()))?);
        Ok(())
    }

    /// Cancel during a run.
    pub fn cancel(&self) {
        self.wizard.lock().cancel_network();
        if let Some(run) = self.run.lock().as_ref() {
            run.cancel();
        }
    }

    pub fn is_running(&self) -> bool {
        self.run.lock().as_ref().is_some_and(BulkRun::is_running)
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------
// Shows (F37-F48b)

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Weak;
use vixen_io::datetime::LocalDateTime;
use vixen_session::boot::AppService;
use vixen_session::scheduler::actions::{launch, Done, Launched, StandardRunner};
use vixen_session::scheduler::run::{Deps, Scheduler};
use vixen_session::scheduler::sequences::SequenceRunner;
use vixen_session::scheduler::{self as sched, Date, ScheduleItem, TimeOfDay};
use vixen_session::shows::{self, ActionType, Show, ShowEditor, ShowItem, ShowItemType};

/// An alert's title and text.
#[derive(uniffi::Record, Clone, PartialEq, Eq)]
pub struct ShowAlert {
    pub title: String,
    pub message: String,
}

fn with_shows<T>(core: &AppCore, change: impl FnOnce(&mut Vec<Show>) -> T) -> T {
    let mut system = core.system.write();
    let mut list = shows::load_shows(&system.module_store);
    let before = list.clone();
    let result = change(&mut list);
    if list != before {
        shows::save_shows(&mut system.module_store, &list);
    }
    result
}

fn save_modules(core: &AppCore) -> Option<String> {
    core.save_module_config().err().map(|e| e.to_string())
}

/// Everything the Setup a Show sheet shows.
#[derive(uniffi::Record, Clone, PartialEq, Eq)]
pub struct ShowEditorView {
    pub name: String,
    /// Startup, Background, Sequential, Shutdown.
    pub tab: u32,
    pub help: String,
    pub rows: Vec<String>,
    pub selected: Option<u32>,
    pub action_names: Vec<String>,
    /// The selected item's place in `action_names`; none with no selection, or for an action this Vixen doesn't have.
    pub action: Option<u32>,
    pub sequence_name: String,
    pub sequence_location: String,
    pub launch_program: String,
    pub launch_command_line: String,
    pub launch_show_command_window: bool,
    pub launch_wait_for_exit: bool,
    pub pause_seconds: i32,
    pub pause_min: i32,
    pub pause_max: i32,
    /// Where the sequence panel starts, and the only folder it accepts.
    pub sequence_dir: String,
}

/// OK's answer: an alert that keeps the sheet open, or closed with any failure to save.
#[derive(uniffi::Record, Clone, PartialEq, Eq)]
pub struct ShowEditorOutcome {
    pub alert: Option<ShowAlert>,
    pub save_error: Option<String>,
}

/// The Setup a Show sheet, on a copy of the show (F39, F52).
#[derive(uniffi::Object)]
pub struct ShowEditorSession {
    core: Arc<AppCore>,
    editor: Mutex<ShowEditor>,
}

#[uniffi::export]
impl ShowEditorSession {
    pub fn view(&self) -> ShowEditorView {
        let editor = self.editor.lock();
        let item = editor.selected_item();
        let text = |value: Option<&Option<String>>| value.and_then(|v| v.clone()).unwrap_or_default();
        ShowEditorView {
            name: editor.show().name_text().to_string(),
            tab: ShowItemType::TABS.iter().position(|t| t == editor.tab()).unwrap_or(0) as u32,
            help: editor.tab().help().to_string(),
            rows: editor.rows().iter().map(|i| i.name_text().to_string()).collect(),
            selected: editor.selected().map(|i| i as u32),
            action_names: ActionType::CHOICES.iter().map(|a| a.text().to_string()).collect(),
            action: item.and_then(|i| ActionType::CHOICES.iter().position(|a| *a == i.action)).map(|i| i as u32),
            sequence_name: item.map(|i| i.sequence_file().to_string()).unwrap_or_default(),
            sequence_location: item
                .filter(|i| !i.sequence_file().is_empty())
                .map(|i| shows::sequence_path(i, editor.sequence_dir()).to_string_lossy().into_owned())
                .unwrap_or_default(),
            launch_program: text(item.map(|i| &i.launch_program_name)),
            launch_command_line: text(item.map(|i| &i.launch_command_line)),
            launch_show_command_window: item.is_some_and(|i| i.launch_show_command_window),
            launch_wait_for_exit: item.is_some_and(|i| i.launch_wait_for_exit),
            pause_seconds: item.map_or(shows::PAUSE_MIN, ShowItem::pause_shown),
            pause_min: shows::PAUSE_MIN,
            pause_max: shows::PAUSE_MAX,
            sequence_dir: editor.sequence_dir().to_string_lossy().into_owned(),
        }
    }

    pub fn set_name(&self, name: String) {
        self.editor.lock().set_name(&name);
    }

    pub fn set_tab(&self, tab: u32) {
        if let Some(tab) = ShowItemType::TABS.get(tab as usize) {
            self.editor.lock().set_tab(tab.clone());
        }
    }

    pub fn select(&self, row: Option<u32>) {
        self.editor.lock().select(row.map(|r| r as usize));
    }

    pub fn add_item(&self) {
        self.editor.lock().add_item();
    }

    pub fn delete_item(&self) {
        self.editor.lock().delete_item();
    }

    pub fn move_item(&self, from: u32, to: u32) {
        self.editor.lock().move_item(from as usize, to as usize);
    }

    pub fn set_action(&self, action: u32) {
        if let Some(action) = ActionType::CHOICES.get(action as usize) {
            self.editor.lock().set_action(action.clone());
        }
    }

    /// The message when the file isn't under the Sequence folder.
    pub fn set_sequence(&self, path: String) -> Option<String> {
        self.editor.lock().set_sequence(Path::new(&path)).err().map(|e| e.to_string())
    }

    pub fn set_launch_program(&self, text: String) {
        self.editor.lock().set_launch_program(&text);
    }

    pub fn set_launch_command_line(&self, text: String) {
        self.editor.lock().set_launch_command_line(&text);
    }

    pub fn set_launch_show_command_window(&self, on: bool) {
        self.editor.lock().set_launch_show_command_window(on);
    }

    pub fn set_launch_wait_for_exit(&self, on: bool) {
        self.editor.lock().set_launch_wait_for_exit(on);
    }

    pub fn set_pause(&self, seconds: i32) {
        self.editor.lock().set_pause(seconds);
    }

    pub fn cancel_alert(&self) -> ShowAlert {
        ShowAlert { title: shows::CANCEL_TITLE.to_string(), message: shows::CANCEL_MESSAGE.to_string() }
    }

    /// F48b. The edited copy takes the place of the show with its id, and the module data is saved.
    pub fn ok(&self) -> ShowEditorOutcome {
        let show = match self.editor.lock().finish() {
            Ok(show) => show,
            Err(error) => {
                let alert = ShowAlert { title: error.title().to_string(), message: error.to_string() };
                return ShowEditorOutcome { alert: Some(alert), save_error: None };
            }
        };
        with_shows(&self.core, |list| match list.iter_mut().find(|s| s.id == show.id) {
            Some(slot) => *slot = show,
            None => list.push(show),
        });
        ShowEditorOutcome { alert: None, save_error: save_modules(&self.core) }
    }

    /// F48a: runs the selected Launch item as it stands. Nothing with no selection.
    pub fn test_launch(&self, listener: Box<dyn LaunchTestListener>) -> Option<Arc<LaunchTest>> {
        let item = self.editor.lock().selected_item()?.clone();
        let output = Arc::new(Mutex::new(String::new()));
        let (seen, listener) = (output.clone(), Arc::new(listener));
        // KEEP: the text is whatever had arrived when the action completed, so a program that isn't waited for shows none.
        let done = Done::new(move || listener.complete(seen.lock().clone()));
        let launched = launch(&item, Box::new(move |text| *output.lock() = text), done);
        Some(Arc::new(LaunchTest { launched: Mutex::new(launched) }))
    }
}

/// The Program Output Window's run.
#[uniffi::export(callback_interface)]
pub trait LaunchTestListener: Send + Sync {
    /// `Status: Run Complete`, with the text to show. From another thread.
    fn complete(&self, text: String);
}

#[derive(uniffi::Object)]
pub struct LaunchTest {
    launched: Mutex<Launched>,
}

#[uniffi::export]
impl LaunchTest {
    /// Close. FIX: the window closing by any means comes here.
    pub fn stop(&self) {
        self.launched.lock().kill();
    }
}

#[uniffi::export]
impl VixenCore {
    /// The Shows list, in order.
    pub fn show_names(&self) -> Vec<String> {
        shows::load_shows(&self.inner.system.read().module_store).iter().map(|s| s.name_text().to_string()).collect()
    }

    /// F38: `New Show`, in memory until the next save. Its row.
    pub fn show_add(&self) -> u32 {
        with_shows(&self.inner, shows::add_show) as u32
    }

    /// F40. FIX: saved at once. The message if the save failed.
    pub fn show_rename(&self, index: u32, name: String) -> Option<String> {
        if with_shows(&self.inner, |list| shows::rename_show(list, index as usize, &name)) {
            save_modules(&self.inner)
        } else {
            None
        }
    }

    pub fn show_delete_message(&self, index: u32) -> String {
        let list = shows::load_shows(&self.inner.system.read().module_store);
        list.get(index as usize).map(shows::delete_show_message).unwrap_or_default()
    }

    /// F41. The message if the save failed.
    pub fn show_delete(&self, index: u32) -> Option<String> {
        let removed = with_shows(&self.inner, |list| {
            let found = (index as usize) < list.len();
            if found {
                list.remove(index as usize);
            }
            found
        });
        if removed { save_modules(&self.inner) } else { None }
    }

    /// F39, F52: the editor on a copy of the show, on its Sequential tab.
    pub fn show_editor(&self, index: u32) -> Option<Arc<ShowEditorSession>> {
        let show = shows::load_shows(&self.inner.system.read().module_store).get(index as usize)?.clone();
        let mut editor = ShowEditor::new(show, self.inner.paths.sequence_dir());
        editor.set_tab(ShowItemType::Sequential);
        Some(Arc::new(ShowEditorSession { core: Arc::clone(&self.inner), editor: Mutex::new(editor) }))
    }

    pub fn shows_help_url(&self) -> String {
        shows::HELP_URL.to_string()
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------
// The scheduler (F49-F62)

struct SchedulerService(Arc<Scheduler>);

impl AppService for SchedulerService {
    fn name(&self) -> &str {
        "Scheduler"
    }

    /// F59: one check at startup, then the timer.
    fn start(&self, _core: &AppCore) -> Result<(), String> {
        self.0.tick(LocalDateTime::now());
        self.0.run();
        Ok(())
    }

    fn stop(&self, _core: &AppCore) {
        self.0.quit();
    }
}

#[uniffi::export(callback_interface)]
pub trait SchedulerListener: Send + Sync {
    /// The status, the log or the schedule changed. From the scheduler's thread.
    fn changed(&self);
}

/// A row of the Scheduled Shows list.
#[derive(uniffi::Record, Clone, PartialEq, Eq)]
pub struct ScheduleRow {
    pub show: String,
    pub schedule: String,
    pub status: String,
}

/// The Scheduler Status window.
#[derive(uniffi::Record, Clone, PartialEq, Eq)]
pub struct SchedulerStatusView {
    pub text: String,
    /// The window is on screen exactly while this is on.
    pub enabled: bool,
    pub can_start: bool,
    pub can_stop: bool,
    /// Newest first.
    pub log: Vec<String>,
    pub position_x: i32,
    pub position_y: i32,
}

/// The Schedule a Show sheet's values.
#[derive(uniffi::Record, Clone, PartialEq, Eq)]
pub struct ScheduleEdit {
    pub show_names: Vec<String>,
    pub show: Option<u32>,
    pub enabled: bool,
    pub start_year: i32,
    pub start_month: u8,
    pub start_day: u8,
    pub end_year: i32,
    pub end_month: u8,
    pub end_day: u8,
    /// Sunday first.
    pub days: Vec<bool>,
    /// Seconds into the day.
    pub start_seconds: u32,
    pub end_seconds: u32,
}

/// Fixed texts of the Scheduler window.
#[derive(uniffi::Record, Clone, PartialEq, Eq)]
pub struct SchedulerTexts {
    pub shows_blurb: String,
    pub delete: ShowAlert,
    pub no_show: ShowAlert,
    pub help_url: String,
}

fn time_of(seconds: u32) -> TimeOfDay {
    let seconds = seconds.min(86_399);
    TimeOfDay { hour: (seconds / 3600) as u8, minute: (seconds / 60 % 60) as u8, second: (seconds % 60) as u8 }
}

/// The scheduler of an open profile: its schedule, its executor and its status.
#[derive(uniffi::Object)]
pub struct SchedulerSession {
    core: Weak<AppCore>,
    scheduler: Arc<Scheduler>,
    changed: AtomicBool,
}

static SCHEDULERS: Mutex<Vec<Arc<SchedulerSession>>> = Mutex::new(Vec::new());

impl SchedulerSession {
    fn shows(&self) -> Vec<Show> {
        self.core.upgrade().map(|core| shows::load_shows(&core.system.read().module_store)).unwrap_or_default()
    }

    /// The schedule into the module data in memory, as upstream's data object always is.
    fn sync(&self) {
        if let Some(core) = self.core.upgrade() {
            sched::save_scheduler(&mut core.system.write().module_store, &self.scheduler.data(), &LocalDateTime::now());
        }
    }

    fn save(&self) -> Option<String> {
        self.sync();
        self.core.upgrade().and_then(|core| save_modules(&core))
    }
}

#[uniffi::export]
impl VixenCore {
    /// The profile's scheduler. The first call makes it, and from then on it checks the schedule as the app runs.
    pub fn scheduler(&self) -> Arc<SchedulerSession> {
        let mut sessions = SCHEDULERS.lock();
        sessions.retain(|s| s.core.strong_count() > 0);
        if let Some(found) = sessions.iter().find(|s| s.core.as_ptr() == Arc::as_ptr(&self.inner)) {
            return found.clone();
        }
        let data = sched::load_scheduler(&self.inner.system.read().module_store);
        let lookup = Arc::downgrade(&self.inner);
        let runner = Arc::new(StandardRunner { sequence: Arc::new(SequenceRunner::new(&self.inner)) });
        let deps = Deps::threaded(
            runner,
            Arc::new(move |id| {
                let core = lookup.upgrade()?;
                let list = shows::load_shows(&core.system.read().module_store);
                list.into_iter().find(|s| s.id == id)
            }),
        );
        let scheduler = Scheduler::new(data, deps);
        self.inner.add_service(Arc::new(SchedulerService(scheduler.clone())));
        self.inner.studio_display.attach_scheduler(&scheduler); // STUDIO: the Schedule pane's edits reach the running scheduler
        let session = Arc::new(SchedulerSession { core: Arc::downgrade(&self.inner), scheduler, changed: AtomicBool::new(false) });
        sessions.push(session.clone());
        session
    }
}

#[uniffi::export]
impl SchedulerSession {
    pub fn set_listener(&self, listener: Option<Box<dyn SchedulerListener>>) {
        self.scheduler.set_listener(listener.map(|l| -> Arc<dyn Fn() + Send + Sync> { Arc::new(move || l.changed()) }));
    }

    pub fn texts(&self) -> SchedulerTexts {
        let alert = |title: &str, message: &str| ShowAlert { title: title.to_string(), message: message.to_string() };
        SchedulerTexts {
            shows_blurb: sched::SHOWS_BLURB.to_string(),
            delete: alert(sched::DELETE_TITLE, sched::DELETE_MESSAGE),
            no_show: alert(sched::NO_SHOW_TITLE, sched::NO_SHOW_MESSAGE),
            help_url: sched::HELP_URL.to_string(),
        }
    }

    /// F49's rows.
    pub fn rows(&self) -> Vec<ScheduleRow> {
        let shows = self.shows();
        let name = |id| shows.iter().find(|s| s.id == id).map_or("None Selected".to_string(), |s| s.name_text().to_string());
        self.scheduler
            .data()
            .items
            .iter()
            .map(|item| ScheduleRow { show: name(item.show_id), schedule: item.schedule_string(), status: item.status_string().to_string() })
            .collect()
    }

    /// The Shows-list row of a schedule's show, for F52.
    pub fn show_of(&self, index: u32) -> Option<u32> {
        let id = self.scheduler.data().items.get(index as usize)?.show_id;
        self.shows().iter().position(|s| s.id == id).map(|i| i as u32)
    }

    /// The sheet's values for a schedule, or for a new one. Dates that were never set show today (F53).
    pub fn edit_values(&self, index: Option<u32>) -> ScheduleEdit {
        let data = self.scheduler.data();
        let item = index.and_then(|i| data.items.get(i as usize).cloned()).unwrap_or_default();
        let shows = self.shows();
        let today = Date::of(&LocalDateTime::now());
        let (start, end) = (item.start_date.shown(today), item.end_date.shown(today));
        ScheduleEdit {
            show_names: shows.iter().map(|s| s.name_text().to_string()).collect(),
            show: shows.iter().position(|s| s.id == item.show_id).map(|i| i as u32),
            enabled: item.enabled,
            start_year: start.year,
            start_month: start.month,
            start_day: start.day,
            end_year: end.year,
            end_month: end.month,
            end_day: end.day,
            days: item.days.to_vec(),
            start_seconds: item.start_time.seconds() as u32,
            end_seconds: item.end_time.seconds() as u32,
        }
    }

    pub fn duration_text(&self, start_seconds: u32, end_seconds: u32) -> String {
        sched::duration_text(time_of(start_seconds), time_of(end_seconds))
    }

    /// OK in the sheet: an alert that keeps it open, or the schedule added (no index) or changed.
    pub fn apply_edit(&self, index: Option<u32>, edit: ScheduleEdit) -> Option<ShowAlert> {
        let shows = self.shows();
        let show = edit.show.and_then(|i| shows.get(i as usize)).map(|s| s.id);
        let start = Date { year: edit.start_year, month: edit.start_month, day: edit.start_day };
        let end = Date { year: edit.end_year, month: edit.end_month, day: edit.end_day };
        if let Err(error) = sched::validate_schedule_edit(start, end, show) {
            return Some(ShowAlert { title: error.title().to_string(), message: error.to_string() });
        }
        let mut days = [false; 7];
        for (day, on) in days.iter_mut().zip(&edit.days) {
            *day = *on;
        }
        let item = ScheduleItem {
            show_id: show.unwrap_or_default(),
            days,
            enabled: edit.enabled,
            start_date: start,
            end_date: end,
            start_time: time_of(edit.start_seconds),
            end_time: time_of(edit.end_seconds),
        };
        match index {
            Some(index) => self.scheduler.set_item(index as usize, item),
            None => self.scheduler.add_item(item),
        }
        self.changed.store(true, Ordering::SeqCst);
        self.sync();
        None
    }

    /// F51. FIX: a schedule that's running is stopped first.
    pub fn delete(&self, index: u32) {
        self.scheduler.delete_item(index as usize);
        self.changed.store(true, Ordering::SeqCst);
        self.sync();
    }

    /// The Scheduler window closed: saves if anything changed (F49). The message if the save failed.
    pub fn window_closed(&self) -> Option<String> {
        if self.changed.swap(false, Ordering::SeqCst) { self.save() } else { None }
    }

    pub fn is_enabled(&self) -> bool {
        self.scheduler.data().is_enabled
    }

    /// F54: Scheduler > Enabled.
    pub fn set_enabled(&self, on: bool) -> Option<String> {
        self.scheduler.set_enabled(on);
        self.save()
    }

    pub fn status(&self) -> SchedulerStatusView {
        let status = self.scheduler.status();
        let (position_x, position_y) = self.scheduler.data().status_position;
        SchedulerStatusView {
            text: status.text,
            enabled: status.enabled,
            can_start: status.can_start,
            can_stop: status.can_stop,
            log: self.scheduler.log_lines(),
            position_x,
            position_y,
        }
    }

    /// F57.
    pub fn start(&self) {
        self.scheduler.start();
    }

    pub fn stop(&self, gracefully: bool) {
        self.scheduler.stop(gracefully);
    }

    /// The Status window moved: kept for the next module save (F55).
    pub fn set_status_position(&self, x: i32, y: i32) {
        self.scheduler.set_status_position(x, y);
        self.sync();
    }

    /// One check of the schedule now, for tests, where the timer doesn't run.
    pub fn check_now(&self) {
        self.scheduler.tick(LocalDateTime::now());
    }
}
