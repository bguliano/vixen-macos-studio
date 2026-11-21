//! The Profile Archive Wizard's work: checking what the user asked for and writing the zip. Port of `DataZipForm`.
//!
//! One streaming writer covers all three passes, where the original reopens the archive in update mode for each, which keeps entries in
//! memory and can run out with gigabytes of `Media`.

use crate::logging;
use crate::SessionError;
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, BufWriter};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use vixen_io::{DataPaths, LocalDateTime};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

const LOGGER: &str = "VixenApplication.DataZipForm";

/// The check boxes. All on by default.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ArchiveSelection {
    pub module: bool,
    pub program: bool,
    pub sequence: bool,
    pub system: bool,
    pub template: bool,
    pub logs: bool,
    pub media: bool,
    pub user_settings: bool,
}

impl Default for ArchiveSelection {
    fn default() -> ArchiveSelection {
        ArchiveSelection {
            module: true,
            program: true,
            sequence: true,
            system: true,
            template: true,
            logs: true,
            media: true,
            user_settings: true,
        }
    }
}

impl ArchiveSelection {
    /// What unticking adds to the exclusions.
    fn exclusions(&self) -> Vec<&'static str> {
        let mut list = Vec::new();
        for (on, folder) in [
            (self.module, "/Module Data Files"),
            (self.program, "/Program"),
            (self.sequence, "/Sequence"),
            (self.system, "/SystemData"),
            (self.template, "/Template"),
            (self.media, "/Media"),
        ] {
            if !on {
                list.push(folder);
            }
        }
        list
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveRequest {
    pub profile_folder: PathBuf,
    pub save_folder: String,
    pub file_name: String,
    pub selection: ArchiveSelection,
}

/// What Start can stop at, in the order it checks. The last two are questions: answer yes with [`resolve_archive_prompt`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArchiveIssue {
    MissingSaveFolder,
    InvalidSaveFolder,
    MissingFileName,
    InvalidFileName,
    FolderMissing,
    FileExists,
}

impl ArchiveIssue {
    pub fn title(&self) -> &'static str {
        match self {
            ArchiveIssue::MissingSaveFolder => "Missing save folder",
            ArchiveIssue::InvalidSaveFolder => "Invalid Folder Path.",
            ArchiveIssue::MissingFileName => "Missing Zip file name",
            ArchiveIssue::InvalidFileName => "Invalid Zip file name",
            ArchiveIssue::FolderMissing => "Folder not found",
            ArchiveIssue::FileExists => "File exists",
        }
    }

    pub fn message(&self) -> &'static str {
        match self {
            ArchiveIssue::MissingSaveFolder => "Please choose a folder to create the zip file in.",
            ArchiveIssue::InvalidSaveFolder => "The folder path for the zip file contains invalid characters.",
            ArchiveIssue::MissingFileName => "Please choose a filename for the zip file.",
            ArchiveIssue::InvalidFileName => "The filename for the zip file contains invalid characters.",
            ArchiveIssue::FolderMissing => "The destination folder does not exist, would you like to create it ?",
            ArchiveIssue::FileExists => "The file name you have enter already exists, do you wish to overwrite it ?",
        }
    }

    pub fn is_question(&self) -> bool {
        matches!(self, ArchiveIssue::FolderMissing | ArchiveIssue::FileExists)
    }

    /// The original shows these with its error icon. A missing file name only gets information, and the questions a question mark.
    pub fn is_error(&self) -> bool {
        matches!(self, ArchiveIssue::MissingSaveFolder | ArchiveIssue::InvalidSaveFolder | ArchiveIssue::InvalidFileName)
    }
}

/// A request that passed its checks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedArchive {
    pub profile_folder: PathBuf,
    /// `{folder}/{name}.zip`.
    pub output: PathBuf,
    /// The name with a trailing `.zip` removed, which the original puts back in its field.
    pub file_name: String,
    pub selection: ArchiveSelection,
}

/// A name ending in exactly `.zip` loses it. `foo.ZIP` stays, and writes `foo.ZIP.zip`, as upstream.
pub fn trimmed_file_name(name: &str) -> &str {
    name.strip_suffix(".zip").unwrap_or(name)
}

fn prepared(request: &ArchiveRequest) -> PreparedArchive {
    let file_name = trimmed_file_name(&request.file_name).to_string();
    PreparedArchive {
        profile_folder: request.profile_folder.clone(),
        output: Path::new(&request.save_folder).join(format!("{file_name}.zip")),
        file_name,
        selection: request.selection,
    }
}

/// `buttonStartCancel_Click`'s checks. The character rules are the Mac's: NUL for a folder, and `/`, `:` and NUL for a name, since
/// Finder shows `:` as `/`.
pub fn check_archive_request(request: &ArchiveRequest) -> Result<PreparedArchive, ArchiveIssue> {
    if request.save_folder.is_empty() {
        return Err(ArchiveIssue::MissingSaveFolder);
    }
    if request.save_folder.contains('\0') {
        return Err(ArchiveIssue::InvalidSaveFolder);
    }
    if request.file_name.is_empty() {
        return Err(ArchiveIssue::MissingFileName);
    }
    if request.file_name.contains(['/', ':', '\0']) {
        return Err(ArchiveIssue::InvalidFileName);
    }
    let archive = prepared(request);
    if !Path::new(&request.save_folder).is_dir() {
        return Err(ArchiveIssue::FolderMissing);
    }
    if archive.output.exists() {
        return Err(ArchiveIssue::FileExists);
    }
    Ok(archive)
}

/// The yes to a question: creates the missing folder, or deletes the file being replaced.
pub fn resolve_archive_prompt(request: &ArchiveRequest) -> Result<PreparedArchive, SessionError> {
    let archive = prepared(request);
    let folder = Path::new(&request.save_folder);
    if !folder.is_dir() {
        fs::create_dir_all(folder).map_err(|e| SessionError::Other(format!("{}: {e}", folder.display())))?;
    } else if archive.output.exists() {
        fs::remove_file(&archive.output).map_err(|e| SessionError::Other(format!("{}: {e}", archive.output.display())))?;
    }
    Ok(archive)
}

/// `Excluded`: a path ending in `.lock`, or one under an unticked folder. The test stays a substring match, but on the path below the
/// profile. The original tests the whole absolute path, so unticking Media empties an archive of a profile under `/Volumes/Media/`.
pub fn excluded(relative_dir: &str, file_name: &str, exclusions: &[&str]) -> bool {
    if file_name.ends_with(".lock") {
        return true;
    }
    let dir = format!("/{relative_dir}");
    exclusions.iter().any(|folder| dir.contains(folder))
}

/// `(int)(filesComplete / (double)fileCount * 80.0)`: tops out at 80 until a pass's end jumps to 100.
pub fn progress_value(files_complete: usize, file_count: usize) -> i32 {
    if file_count == 0 {
        return 0;
    }
    (files_complete as f64 / file_count as f64 * 80.0) as i32
}

/// The status strip's two parts. Called from the worker thread.
pub trait ArchiveProgress: Send + Sync {
    fn status(&self, text: &str);
    fn value(&self, percent: i32);
}

/// Runs the passes. Stopping ends after the current file, leaving a valid zip of what it has, with the rest of the passes ending at once.
/// An error stops the passes too, also leaving a valid zip, and is returned for the alert. Either way the status ends at `Complete`.
pub fn run_archive(archive: &PreparedArchive, cancel: &AtomicBool, progress: &dyn ArchiveProgress) -> Result<(), SessionError> {
    let logs = DataPaths::shared_logs_dir();
    let settings = vixen_io::settings_dir();
    let result = write_archive(archive, logs.as_deref(), &settings, cancel, progress);
    progress.status("Complete");
    if let Err(error) = &result {
        logging::error(LOGGER, "An error occurred adding files to archive", Some(&error.to_string()));
    }
    result
}

/// `{ex.Message}` in the original's alert.
pub fn error_alert_text(error: &SessionError) -> String {
    format!("An error occurred during the zip process.\n\r {error}")
}

fn write_archive(
    archive: &PreparedArchive,
    logs_folder: Option<&Path>,
    settings_folder: &Path,
    cancel: &AtomicBool,
    progress: &dyn ArchiveProgress,
) -> Result<(), SessionError> {
    // Split from the text, as the original does: a folder stored with a trailing separator gives no top folder at all.
    let folder_text = archive.profile_folder.to_string_lossy();
    let parent = folder_text.rsplit('/').next().unwrap_or("").to_string();
    let exclusions = archive.selection.exclusions();
    let output = File::create(&archive.output).map_err(|e| other(&archive.output, e))?;
    let mut writer = ZipWriter::new(BufWriter::new(output));

    let result = (|| {
        progress.status("Zipping profile please wait...");
        add_folder(&mut writer, &archive.profile_folder, &exclusions, &parent, &archive.output, cancel, progress)?;
        // No log folder without `HOME`: nothing was logged, so there is nothing to add.
        if let (true, Some(logs_folder)) = (archive.selection.logs, logs_folder) {
            progress.status("Zipping logs please wait...");
            add_folder(&mut writer, logs_folder, &[], &entry_name(&parent, "Core Logs"), &archive.output, cancel, progress)?;
        }
        if archive.selection.user_settings {
            progress.status("Zipping user settings please wait...");
            // Upstream's folder always exists, since every `XMLProfileSettings` it constructs makes it, the wizard's own included. Nothing here
            // makes it until a setting is saved, so on a new Mac the default run would fail on this pass.
            let _ = fs::create_dir_all(settings_folder);
            add_folder(&mut writer, settings_folder, &[], &entry_name(&parent, "User Settings"), &archive.output, cancel, progress)?;
        }
        Ok(())
    })();
    let finished = writer.finish().map(|_| ()).map_err(|e| SessionError::Other(e.to_string()));
    result.and(finished)
}

fn entry_name(parent: &str, below: &str) -> String {
    if parent.is_empty() {
        below.to_string()
    } else {
        format!("{parent}/{below}")
    }
}

/// One pass of `Archive`. `file_count` counts every file, excluded or not, so the bar rarely reaches 80 before the jump, as upstream.
fn add_folder<W: io::Write + io::Seek>(
    writer: &mut ZipWriter<W>,
    folder: &Path,
    exclusions: &[&str],
    parent: &str,
    output: &Path,
    cancel: &AtomicBool,
    progress: &dyn ArchiveProgress,
) -> Result<(), SessionError> {
    // GUARD: the original lists files outside its try, so a missing folder ends the worker with the button stuck on Stop.
    let files = list_files(folder).map_err(|e| other(folder, e))?;
    let output = output.canonicalize().unwrap_or_else(|_| output.to_path_buf());
    let mut complete = 0;
    let mut last_percent = -1;
    for file in &files {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let relative = file.strip_prefix(folder).unwrap_or(file);
        let relative_dir = relative.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        let name = relative.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if excluded(&relative_dir, &name, exclusions) {
            continue;
        }
        if file.canonicalize().is_ok_and(|p| p == output) {
            continue;
        }
        add_file(writer, file, &entry_name(parent, &relative.to_string_lossy()))?;
        complete += 1;
        let percent = progress_value(complete, files.len());
        if percent != last_percent {
            progress.value(percent);
            last_percent = percent;
        }
    }
    progress.value(100);
    Ok(())
}

fn add_file<W: io::Write + io::Seek>(writer: &mut ZipWriter<W>, file: &Path, entry: &str) -> Result<(), SessionError> {
    let mut source = File::open(file).map_err(|e| other(file, e))?;
    let meta = source.metadata().map_err(|e| other(file, e))?;
    let mut options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .large_file(meta.len() >= u32::MAX as u64)
        .unix_permissions(0o644);
    if let Some(time) = meta.modified().ok().and_then(zip_time) {
        options = options.last_modified_time(time);
    }
    writer.start_file(entry, options).map_err(|e| SessionError::Other(format!("{entry}: {e}")))?;
    io::copy(&mut source, writer).map_err(|e| other(file, e))?;
    Ok(())
}

fn zip_time(time: std::time::SystemTime) -> Option<zip::DateTime> {
    let t = LocalDateTime::from_system_time(time);
    zip::DateTime::from_date_and_time(t.year as u16, t.month, t.day, t.hour, t.minute, t.second).ok()
}

fn other(path: &Path, error: io::Error) -> SessionError {
    SessionError::Other(format!("{}: {error}", path.display()))
}

/// Every file below the folder, in a repeatable order. Links are followed, as `Media` can be one, and a link back up is only walked once.
fn list_files(folder: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut seen = HashSet::new();
    walk(folder, &mut seen, &mut files)?;
    Ok(files)
}

fn walk(folder: &Path, seen: &mut HashSet<PathBuf>, files: &mut Vec<PathBuf>) -> io::Result<()> {
    if !seen.insert(folder.canonicalize()?) {
        return Ok(());
    }
    let mut entries: Vec<PathBuf> = fs::read_dir(folder)?.map(|e| e.map(|e| e.path())).collect::<io::Result<_>>()?;
    entries.sort();
    for path in entries {
        match fs::metadata(&path) {
            Ok(meta) if meta.is_dir() => walk(&path, seen, files)?,
            Ok(_) => files.push(path),
            // A dangling link has nothing to archive.
            Err(_) => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;
    use std::io::Read as _;

    #[derive(Default)]
    struct Recorder {
        statuses: Mutex<Vec<String>>,
        values: Mutex<Vec<i32>>,
    }

    impl ArchiveProgress for Recorder {
        fn status(&self, text: &str) {
            self.statuses.lock().push(text.to_string());
        }
        fn value(&self, percent: i32) {
            self.values.lock().push(percent);
        }
    }

    struct Fixture {
        dir: PathBuf,
        profile: PathBuf,
        logs: PathBuf,
        settings: PathBuf,
    }

    fn fixture(name: &str) -> Fixture {
        fixture_at(name, "Vixen 3")
    }

    fn fixture_at(name: &str, profile_path: &str) -> Fixture {
        let dir = std::env::temp_dir().join(format!("vixen-archive-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let profile = dir.join(profile_path);
        for file in [
            "SystemData/SystemConfig.xml",
            "Sequence/A.tim",
            "Sequence/auto_backup/A_1.tim",
            "Media/song.mp3",
            "Program/p.txt",
            "Template/Element/t.xml",
            "Module Data Files/Picture/pic.png",
            "Effect Defaults/d.xml",
            "logs/own.log",
            "VixenApplicationData.xml",
            ".lock",
        ] {
            let path = profile.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, file.as_bytes()).unwrap();
        }
        let logs = dir.join("shared logs");
        fs::create_dir_all(&logs).unwrap();
        fs::write(logs.join("Application.Info.log"), "log").unwrap();
        let settings = dir.join("settings");
        fs::create_dir_all(&settings).unwrap();
        fs::write(settings.join("Settings.xml"), "<settings/>").unwrap();
        Fixture { dir, profile, logs, settings }
    }

    fn run_with_folder(f: &Fixture, folder: PathBuf, selection: ArchiveSelection) -> (Result<(), SessionError>, Vec<String>) {
        let archive = PreparedArchive { profile_folder: folder, output: f.dir.join("out.zip"), file_name: "out".to_string(), selection };
        let flag = AtomicBool::new(false);
        let result = write_archive(&archive, Some(&f.logs), &f.settings, &flag, &Recorder::default());
        (result, names_in(&f.dir.join("out.zip")))
    }

    fn run(f: &Fixture, selection: ArchiveSelection, cancel: bool) -> (Result<(), SessionError>, Vec<String>, Recorder) {
        let archive = PreparedArchive {
            profile_folder: f.profile.clone(),
            output: f.dir.join("out.zip"),
            file_name: "out".to_string(),
            selection,
        };
        let recorder = Recorder::default();
        let flag = AtomicBool::new(cancel);
        let result = write_archive(&archive, Some(&f.logs), &f.settings, &flag, &recorder);
        (result, names_in(&f.dir.join("out.zip")), recorder)
    }

    fn names_in(zip_path: &Path) -> Vec<String> {
        let Ok(file) = File::open(zip_path) else {
            return Vec::new();
        };
        let archive = zip::ZipArchive::new(file).expect("a valid zip");
        let mut names: Vec<String> = archive.file_names().map(|n| n.unwrap().into_owned()).collect();
        names.sort();
        names
    }

    fn only_profile() -> ArchiveSelection {
        ArchiveSelection { logs: false, user_settings: false, ..ArchiveSelection::default() }
    }

    #[test]
    fn everything_on_takes_all_but_the_lock_under_the_folders_name() {
        let f = fixture("all");
        let (result, names, _) = run(&f, only_profile(), false);
        result.unwrap();
        assert_eq!(
            names,
            [
                "Vixen 3/Effect Defaults/d.xml",
                "Vixen 3/Media/song.mp3",
                "Vixen 3/Module Data Files/Picture/pic.png",
                "Vixen 3/Program/p.txt",
                "Vixen 3/Sequence/A.tim",
                "Vixen 3/Sequence/auto_backup/A_1.tim",
                "Vixen 3/SystemData/SystemConfig.xml",
                "Vixen 3/Template/Element/t.xml",
                "Vixen 3/VixenApplicationData.xml",
                "Vixen 3/logs/own.log",
            ]
        );
    }

    #[test]
    fn a_trailing_separator_gives_entries_no_top_folder() {
        let f = fixture("trailing");
        let folder = PathBuf::from(format!("{}/", f.profile.display()));
        let (result, names) = run_with_folder(&f, folder, ArchiveSelection { logs: true, user_settings: true, ..ArchiveSelection::default() });
        result.unwrap();
        assert!(names.iter().any(|n| n == "SystemData/SystemConfig.xml"), "{names:?}");
        assert!(names.iter().any(|n| n == "Core Logs/Application.Info.log"), "{names:?}");
        assert!(names.iter().any(|n| n == "User Settings/Settings.xml"), "{names:?}");
    }

    #[test]
    fn contents_survive_the_round_trip() {
        let f = fixture("contents");
        run(&f, only_profile(), false).0.unwrap();
        let mut archive = zip::ZipArchive::new(File::open(f.dir.join("out.zip")).unwrap()).unwrap();
        let mut text = String::new();
        archive.by_name("Vixen 3/Sequence/A.tim").unwrap().read_to_string(&mut text).unwrap();
        assert_eq!(text, "Sequence/A.tim");
    }

    #[test]
    fn unticking_leaves_the_folder_out() {
        let f = fixture("media-off");
        let (_, names, _) = run(&f, ArchiveSelection { media: false, ..only_profile() }, false);
        assert!(!names.iter().any(|n| n.contains("/Media/")), "{names:?}");
        assert!(names.iter().any(|n| n.contains("/Sequence/")));

        let f = fixture("sequence-off");
        let (_, names, _) = run(&f, ArchiveSelection { sequence: false, ..only_profile() }, false);
        assert!(!names.iter().any(|n| n.contains("/Sequence/")), "auto_backup goes too: {names:?}");

        for (selection, gone) in [
            (ArchiveSelection { module: false, ..only_profile() }, "/Module Data Files/"),
            (ArchiveSelection { program: false, ..only_profile() }, "/Program/"),
            (ArchiveSelection { system: false, ..only_profile() }, "/SystemData/"),
            (ArchiveSelection { template: false, ..only_profile() }, "/Template/"),
        ] {
            let f = fixture(&format!("off{}", gone.replace('/', "").replace(' ', "")));
            let (_, names, _) = run(&f, selection, false);
            assert!(!names.iter().any(|n| n.contains(gone)), "{gone}: {names:?}");
            assert!(names.iter().any(|n| n.ends_with("VixenApplicationData.xml")), "always included");
        }
    }

    #[test]
    fn exclusions_only_look_below_the_profile_folder() {
        let f = fixture_at("media-parent", "Media Backup/Vixen 3");
        let (_, names, _) = run(&f, ArchiveSelection { media: false, ..only_profile() }, false);
        assert!(names.iter().any(|n| n == "Vixen 3/Sequence/A.tim"), "{names:?}");
        assert!(!names.iter().any(|n| n.starts_with("Vixen 3/Media/")), "{names:?}");
        assert_eq!(names.len(), 9);
    }

    #[test]
    fn logs_and_user_settings_land_under_their_folders() {
        let f = fixture("extras");
        let (result, names, recorder) = run(&f, ArchiveSelection::default(), false);
        result.unwrap();
        assert!(names.iter().any(|n| n == "Vixen 3/Core Logs/Application.Info.log"), "{names:?}");
        assert!(names.iter().any(|n| n == "Vixen 3/User Settings/Settings.xml"), "{names:?}");
        assert!(names.iter().any(|n| n == "Vixen 3/logs/own.log"), "the profile's own logs come along whatever the box says");
        assert_eq!(
            *recorder.statuses.lock(),
            ["Zipping profile please wait...", "Zipping logs please wait...", "Zipping user settings please wait..."]
        );
        assert!(recorder.values.lock().iter().filter(|v| **v == 100).count() == 3);
    }

    #[test]
    fn progress_is_the_originals_formula() {
        assert_eq!(progress_value(1, 4), 20);
        assert_eq!(progress_value(3, 4), 60);
        assert_eq!(progress_value(4, 4), 80);
        assert_eq!(progress_value(0, 0), 0);
    }

    #[test]
    fn cancelled_before_starting_leaves_a_valid_empty_zip() {
        let f = fixture("cancel");
        let (result, names, _) = run(&f, ArchiveSelection::default(), true);
        result.unwrap();
        assert!(names.is_empty());
        assert!(f.dir.join("out.zip").is_file());
    }

    #[test]
    fn a_missing_profile_folder_is_an_error_not_a_hang_and_the_zip_stays_valid() {
        let f = fixture("missing");
        fs::remove_dir_all(&f.profile).unwrap();
        let (result, names, _) = run(&f, only_profile(), false);
        assert!(result.is_err());
        assert!(names.is_empty());
    }

    #[test]
    fn a_user_settings_folder_nothing_has_made_yet_does_not_fail_the_default_run() {
        let f = fixture("no-settings");
        fs::remove_dir_all(&f.settings).unwrap();
        let (result, names, recorder) = run(&f, ArchiveSelection::default(), false);
        result.unwrap();
        assert!(names.iter().any(|n| n == "Vixen 3/Core Logs/Application.Info.log"), "{names:?}");
        assert!(!names.iter().any(|n| n.contains("User Settings")), "{names:?}");
        assert_eq!(recorder.statuses.lock().len(), 3);
        assert!(f.settings.is_dir(), "made, as upstream's settings class makes it");
    }

    #[test]
    fn a_later_pass_is_skipped_after_a_failure() {
        let f = fixture("skip");
        fs::remove_dir_all(&f.logs).unwrap();
        let (result, names, recorder) = run(&f, ArchiveSelection::default(), false);
        assert!(result.is_err());
        assert!(names.iter().any(|n| n.starts_with("Vixen 3/Sequence/")), "the first pass stands");
        assert!(!names.iter().any(|n| n.contains("User Settings")));
        assert_eq!(recorder.statuses.lock().len(), 2);
    }

    #[test]
    fn the_archive_never_includes_itself() {
        let f = fixture("self");
        let archive = PreparedArchive {
            profile_folder: f.profile.clone(),
            output: f.profile.join("backup.zip"),
            file_name: "backup".to_string(),
            selection: only_profile(),
        };
        let flag = AtomicBool::new(false);
        write_archive(&archive, Some(&f.logs), &f.settings, &flag, &Recorder::default()).unwrap();
        let names = names_in(&f.profile.join("backup.zip"));
        assert!(!names.iter().any(|n| n.ends_with("backup.zip")), "{names:?}");
        assert!(!names.is_empty());
    }

    #[test]
    fn a_link_to_a_folder_is_followed_and_a_loop_ends() {
        let f = fixture("links");
        std::os::unix::fs::symlink(f.dir.join("settings"), f.profile.join("Linked")).unwrap();
        std::os::unix::fs::symlink(&f.profile, f.profile.join("Sequence/up")).unwrap();
        let (result, names, _) = run(&f, only_profile(), false);
        result.unwrap();
        assert!(names.iter().any(|n| n == "Vixen 3/Linked/Settings.xml"), "{names:?}");
        assert!(!names.iter().any(|n| n.contains("/up/")), "{names:?}");
    }

    fn request(save_folder: &str, file_name: &str) -> ArchiveRequest {
        ArchiveRequest {
            profile_folder: PathBuf::from("/p/Vixen 3"),
            save_folder: save_folder.to_string(),
            file_name: file_name.to_string(),
            selection: ArchiveSelection::default(),
        }
    }

    #[test]
    fn the_checks_run_in_order() {
        assert_eq!(check_archive_request(&request("", "")), Err(ArchiveIssue::MissingSaveFolder));
        assert_eq!(check_archive_request(&request("/a\0b", "")), Err(ArchiveIssue::InvalidSaveFolder));
        assert_eq!(check_archive_request(&request("/a", "")), Err(ArchiveIssue::MissingFileName));
        for bad in ["a/b", "a:b", "a\0b"] {
            assert_eq!(check_archive_request(&request("/a", bad)), Err(ArchiveIssue::InvalidFileName), "{bad:?}");
        }
        assert_eq!(ArchiveIssue::MissingSaveFolder.message(), "Please choose a folder to create the zip file in.");
        assert_eq!(ArchiveIssue::MissingFileName.title(), "Missing Zip file name");
    }

    #[test]
    fn a_zip_extension_trims_only_when_it_is_exact() {
        let f = fixture("trim");
        let folder = f.dir.to_string_lossy().into_owned();
        let archive = check_archive_request(&request(&folder, "foo.zip")).unwrap();
        assert_eq!(archive.file_name, "foo");
        assert_eq!(archive.output, f.dir.join("foo.zip"));
        let archive = check_archive_request(&request(&folder, "foo.ZIP")).unwrap();
        assert_eq!(archive.file_name, "foo.ZIP");
        assert_eq!(archive.output, f.dir.join("foo.ZIP.zip"));
    }

    #[test]
    fn a_missing_folder_asks_and_yes_creates_it() {
        let f = fixture("mkdir");
        let folder = f.dir.join("new/folder").to_string_lossy().into_owned();
        let req = request(&folder, "x");
        assert_eq!(check_archive_request(&req), Err(ArchiveIssue::FolderMissing));
        assert!(ArchiveIssue::FolderMissing.is_question());
        let archive = resolve_archive_prompt(&req).unwrap();
        assert!(Path::new(&folder).is_dir());
        assert_eq!(archive.output, Path::new(&folder).join("x.zip"));
    }

    #[test]
    fn an_existing_file_asks_and_yes_deletes_it() {
        let f = fixture("replace");
        fs::write(f.dir.join("x.zip"), "old").unwrap();
        let req = request(&f.dir.to_string_lossy(), "x.zip");
        assert_eq!(check_archive_request(&req), Err(ArchiveIssue::FileExists));
        resolve_archive_prompt(&req).unwrap();
        assert!(!f.dir.join("x.zip").exists());
    }

    #[test]
    fn a_folder_that_cannot_be_made_is_an_error_not_a_panic() {
        let f = fixture("cant-make");
        fs::write(f.dir.join("a file"), "").unwrap();
        let req = request(&f.dir.join("a file/below").to_string_lossy(), "x");
        assert!(resolve_archive_prompt(&req).is_err());
    }

    #[test]
    fn a_zip_that_cannot_be_deleted_is_an_error_not_a_panic() {
        let f = fixture("cant-delete");
        // A folder under the zip's name, which remove_file refuses.
        fs::create_dir_all(f.dir.join("x.zip")).unwrap();
        let req = request(&f.dir.to_string_lossy(), "x");
        assert_eq!(check_archive_request(&req), Err(ArchiveIssue::FileExists));
        assert!(resolve_archive_prompt(&req).is_err());
    }

    #[test]
    fn excluded_follows_the_rules() {
        assert!(excluded("", ".lock", &[]));
        assert!(excluded("Sequence", "x.lock", &[]));
        assert!(!excluded("Media", "a.mp3", &[]));
        assert!(excluded("Media", "a.mp3", &["/Media"]));
        assert!(excluded("Media/sub", "a.mp3", &["/Media"]));
        assert!(excluded("Sequence/Media Files", "a", &["/Media"]), "still a substring test");
        assert!(!excluded("", "VixenApplicationData.xml", &["/Media", "/Program"]));
    }

    #[test]
    fn the_error_alert_has_the_originals_text() {
        assert_eq!(
            error_alert_text(&SessionError::Other("boom".to_string())),
            "An error occurred during the zip process.\n\r boom"
        );
    }
}
