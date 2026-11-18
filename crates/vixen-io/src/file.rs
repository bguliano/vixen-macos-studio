//! Atomic writes and the backup rules of `XElementFileWriter` and `ObjectMigratorService`.

use crate::error::IoError;
use crate::paths::BACKUP_FOLDER;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

/// `XElementFileWriter.BackupsToKeep`.
const BACKUPS_TO_KEEP: usize = 3;
/// `XElementFileWriter.DaysToKeep`.
const DAYS_TO_KEEP: usize = 3;

/// Writes the bytes to a temporary file beside `path`, syncs it and renames it over `path`, so a reader or a
/// crash never sees part of a file. Creates the folder if it is missing.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), IoError> {
    ensure_parent(path)?;
    let temp = write_temp(path, bytes)?;
    fs::rename(&temp, path).map_err(|e| {
        let _ = fs::remove_file(&temp);
        IoError::io(path, e)
    })?;
    sync_folder(path);
    Ok(())
}

/// Saves a file the way `XElementFileWriter` does, keeping its backups:
///
/// 1. If the file on disk is older than `current_version`, it is copied to `<file>.<version>`. C# does this in
///    `ObjectMigratorService` when it loads the file. This port does it here instead, so a load never writes.
/// 2. The file on disk goes to `auto_backup/<name>_<MMddyyyy_hhmmss>.<ext>`, stamped with its last write time.
/// 3. Old backups are pruned: three kept for the newest day, one for each of the next two, none after that.
///
/// C# moves the file away and then writes the new one, so a crash in between leaves no file at all. Here the
/// new content is complete before anything moves, the backup is a hard link to the old file, and a rename puts
/// the new file in place, so the path always holds one whole version. If the backup fails, the old file stays
/// and the error is returned, where C# would ask the user whether to continue.
///
/// `missing_version` is what a file with no `version` attribute counts as. `sequence_backup_dir` is `Some` when
/// saving a system file, for the cleanup of VIX-3785.
pub(crate) fn save_with_backups(
    path: &Path,
    bytes: &[u8],
    current_version: i32,
    missing_version: Option<i32>,
    sequence_backup_dir: Option<&Path>,
) -> Result<(), IoError> {
    ensure_parent(path)?;
    let temp = write_temp(path, bytes)?;
    let result = (|| {
        if path.exists() {
            if let Some(version) = root_version(path).or(missing_version) {
                if version < current_version {
                    let copy = with_suffix(path, &format!(".{version}"));
                    fs::copy(path, &copy).map_err(|e| IoError::io(&copy, e))?;
                }
            }
            backup_file(path, sequence_backup_dir)?;
        }
        fs::rename(&temp, path).map_err(|e| IoError::io(path, e))
    })();
    match &result {
        Ok(()) => sync_folder(path),
        Err(_) => {
            let _ = fs::remove_file(&temp);
            protect_backups(path);
        }
    }
    result
}

/// Makes a rename durable where the file system supports it. Failure changes nothing a reader can see.
fn sync_folder(path: &Path) {
    if let Some(dir) = path.parent() {
        if let Ok(d) = fs::File::open(if dir.as_os_str().is_empty() { Path::new(".") } else { dir }) {
            let _ = d.sync_all();
        }
    }
}

fn ensure_parent(path: &Path) -> Result<(), IoError> {
    match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => fs::create_dir_all(dir).map_err(|e| IoError::io(dir, e)),
        _ => Ok(()),
    }
}

fn write_temp(path: &Path, bytes: &[u8]) -> Result<PathBuf, IoError> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let temp =
        path.with_file_name(format!(".{name}.tmp-{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed)));
    // STUDIO-BEGIN exclusive-save-temp: never follow or remove a pre-existing temporary-file symlink
    let mut owned=false;
    let result = (|| {
        let mut f=fs::OpenOptions::new().write(true).create_new(true).open(&temp)?;
        owned=true;
        f.write_all(bytes)?;
        f.sync_all()
    })();
    // STUDIO-END exclusive-save-temp
    match result {
        Ok(()) => Ok(temp),
        Err(e) => {
            if owned { let _ = fs::remove_file(&temp); } // STUDIO: clean up only a temporary file this call created
            Err(IoError::io(&temp, e))
        }
    }
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}

/// The `version` attribute of a file's root element, read without parsing the file.
pub(crate) fn root_version(path: &Path) -> Option<i32> {
    use std::io::Read as _;
    let mut f = fs::File::open(path).ok()?;
    let mut head = vec![0u8; 8192];
    let n = f.read(&mut head).ok()?;
    let text = String::from_utf8_lossy(&head[..n]);
    let mut rest = text.as_ref();
    loop {
        let start = rest.find('<')?;
        rest = &rest[start..];
        if rest.starts_with("<?") || rest.starts_with("<!") {
            rest = &rest[1..];
            let close = rest.find('>')?;
            rest = &rest[close + 1..];
            continue;
        }
        break;
    }
    let tag = &rest[..rest.find('>')?];
    let at = tag.find(" version=\"")? + " version=\"".len();
    let value = &tag[at..];
    value[..value.find('"')?].parse().ok()
}

/// `XElementFileWriter.BackupFile`.
fn backup_file(path: &Path, sequence_backup_dir: Option<&Path>) -> Result<(), IoError> {
    let dir = path.parent().unwrap_or_else(|| Path::new(""));
    let backup_dir = dir.join(BACKUP_FOLDER);
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let extension = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let metadata = fs::metadata(path).map_err(|e| IoError::io(path, e))?;
    let modified = metadata.modified().map_err(|e| IoError::io(path, e))?;
    let backup = backup_dir.join(format!("{stem}_{}{extension}", LocalTime::from(modified).backup_stamp()));
    fs::create_dir_all(&backup_dir).map_err(|e| IoError::io(&backup_dir, e))?;
    if backup.exists() {
        fs::remove_file(&backup).map_err(|e| IoError::io(&backup, e))?;
    }
    // A hard link keeps the last write time, which pruning sorts on, as `File.Move` does.
    if fs::hard_link(path, &backup).is_err() {
        fs::copy(path, &backup).map_err(|e| IoError::io(&backup, e))?;
        if let Ok(f) = fs::File::options().write(true).open(&backup) {
            let _ = f.set_modified(modified);
        }
    }
    // C# overwrites the result of this with `true`, so a failed prune never stops a save.
    let _ = purge_old_backups(path, sequence_backup_dir);
    Ok(())
}

/// `XElementFileWriter.PurgeOldBackups`.
fn purge_old_backups(path: &Path, sequence_backup_dir: Option<&Path>) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new(""));
    let file_name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let backup_dir = dir.join(BACKUP_FOLDER);
    let backup_prefix = format!("{file_name}_backup");

    // Backups in the old naming scheme move into the backup folder.
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(&backup_prefix) {
            fs::rename(entry.path(), backup_dir.join(&name))?;
        }
    }

    // VIX-3785: system backups used to land in the sequence backup folder.
    if let Some(sequence_backups) = sequence_backup_dir {
        if let Ok(entries) = fs::read_dir(sequence_backups) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with("ModuleStore_") || name.starts_with("SystemConfig_") {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
    }

    let stem_prefix = format!("{stem}_");
    let mut backups: Vec<(SystemTime, PathBuf)> = Vec::new();
    for entry in fs::read_dir(&backup_dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if (name.starts_with(&stem_prefix) || name.starts_with(&backup_prefix)) && entry.file_type()?.is_file() {
            backups.push((entry.metadata()?.modified()?, entry.path()));
        }
    }
    if backups.len() <= BACKUPS_TO_KEEP {
        return Ok(());
    }
    backups.sort_by_key(|b| std::cmp::Reverse(b.0));
    for victim in backups_to_delete(&backups) {
        fs::remove_file(victim)?;
    }
    Ok(())
}

/// The day-based retention: `backups` is newest first.
fn backups_to_delete(backups: &[(SystemTime, PathBuf)]) -> Vec<&PathBuf> {
    let mut victims = Vec::new();
    let mut day_number = 0;
    let mut in_day = 0;
    let mut current_day = None;
    for (modified, path) in backups {
        let day = LocalTime::from(*modified).date();
        if current_day != Some(day) {
            if current_day.is_some() {
                day_number += 1;
            }
            current_day = Some(day);
            in_day = 0;
        }
        let keep = match day_number {
            0 => BACKUPS_TO_KEEP,
            n if n < DAYS_TO_KEEP => 1,
            _ => 0,
        };
        if in_day >= keep {
            victims.push(path);
        }
        in_day += 1;
    }
    victims
}

/// `XElementFileWriter.ProtectBackups`: after a failed save, old-style backups are renamed so nothing prunes them.
fn protect_backups(path: &Path) {
    let dir = path.parent().unwrap_or_else(|| Path::new(""));
    let file_name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let prefix = format!("{file_name}_backup");
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(&prefix) {
            let _ = fs::rename(entry.path(), dir.join(name.replace("backup", "protected")));
        }
    }
}

/// A moment on the local clock, since `File.GetLastWriteTime` is local and std has no time zones.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct LocalTime {
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
}

impl LocalTime {
    fn date(&self) -> (i32, u32, u32) {
        (self.year, self.month, self.day)
    }

    /// `timeStamp:MMddyyyy_hhmmss`. `hh` is the 12-hour clock, so 13:05 and 01:05 share a stamp, as in C#.
    fn backup_stamp(&self) -> String {
        let hour12 = match self.hour % 12 {
            0 => 12,
            h => h,
        };
        format!("{:02}{:02}{:04}_{:02}{:02}{:02}", self.month, self.day, self.year, hour12, self.minute, self.second)
    }
}

impl From<SystemTime> for LocalTime {
    fn from(t: SystemTime) -> LocalTime {
        let seconds = match t.duration_since(SystemTime::UNIX_EPOCH) {
            Ok(d) => d.as_secs() as i64,
            Err(e) => -(e.duration().as_secs() as i64),
        };
        local_time(seconds)
    }
}

#[cfg(unix)]
fn local_time(seconds: i64) -> LocalTime {
    use std::ffi::c_char;

    #[repr(C)]
    struct Tm {
        tm_sec: i32,
        tm_min: i32,
        tm_hour: i32,
        tm_mday: i32,
        tm_mon: i32,
        tm_year: i32,
        tm_wday: i32,
        tm_yday: i32,
        tm_isdst: i32,
        tm_gmtoff: i64,
        tm_zone: *const c_char,
    }
    extern "C" {
        fn localtime_r(time: *const i64, result: *mut Tm) -> *mut Tm;
    }

    let mut tm = Tm {
        tm_sec: 0,
        tm_min: 0,
        tm_hour: 0,
        tm_mday: 1,
        tm_mon: 0,
        tm_year: 70,
        tm_wday: 0,
        tm_yday: 0,
        tm_isdst: 0,
        tm_gmtoff: 0,
        tm_zone: std::ptr::null(),
    };
    // SAFETY: both pointers refer to live, properly aligned values for the call's duration, and `Tm` matches
    // the 64-bit `struct tm` layout on macOS and Linux. `localtime_r` writes only to `tm`.
    let converted = unsafe { !localtime_r(&seconds, &mut tm).is_null() };
    if !converted {
        return utc_time(seconds);
    }
    LocalTime {
        year: tm.tm_year + 1900,
        month: (tm.tm_mon + 1) as u32,
        day: tm.tm_mday as u32,
        hour: tm.tm_hour as u32,
        minute: tm.tm_min as u32,
        second: tm.tm_sec as u32,
    }
}

#[cfg(not(unix))]
fn local_time(seconds: i64) -> LocalTime {
    utc_time(seconds)
}

/// Days-to-civil conversion, for when the system cannot supply local time.
fn utc_time(seconds: i64) -> LocalTime {
    let days = seconds.div_euclid(86_400);
    let secs = seconds.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = (yoe + era * 400 + i64::from(month <= 2)) as i32;
    LocalTime {
        year,
        month,
        day,
        hour: (secs / 3600) as u32,
        minute: ((secs % 3600) / 60) as u32,
        second: (secs % 60) as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_dir;
    use std::time::Duration;

    fn set_mtime(path: &Path, t: SystemTime) {
        fs::File::options().write(true).open(path).unwrap().set_modified(t).unwrap();
    }

    #[test]
    fn atomic_write_replaces_and_leaves_no_temp_file() {
        let dir = test_dir("atomic");
        let path = dir.join("a.xml");
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"two");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn saving_moves_the_old_file_to_auto_backup_with_its_timestamp() {
        let dir = test_dir("backup");
        let path = dir.join("Song.tim");
        fs::write(&path, "<a version=\"9\"/>").unwrap();
        let when = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        set_mtime(&path, when);
        save_with_backups(&path, b"<a version=\"9\">new</a>", 9, Some(0), None).unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"<a version=\"9\">new</a>");
        let backups: Vec<_> = fs::read_dir(dir.join("auto_backup")).unwrap().map(|e| e.unwrap()).collect();
        assert_eq!(backups.len(), 1);
        let name = backups[0].file_name().to_string_lossy().into_owned();
        assert_eq!(name, format!("Song_{}.tim", LocalTime::from(when).backup_stamp()));
        assert_eq!(fs::read(backups[0].path()).unwrap(), b"<a version=\"9\"/>");
        assert_eq!(backups[0].metadata().unwrap().modified().unwrap(), when);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn saving_over_an_older_version_keeps_the_version_backup() {
        let dir = test_dir("version");
        let path = dir.join("Song.tim");
        fs::write(&path, "\u{feff}<?xml version=\"1.0\"?>\r\n<T version=\"8\" xmlns=\"x\"/>").unwrap();
        save_with_backups(&path, b"<T version=\"9\"/>", 9, Some(0), None).unwrap();
        let copy = fs::read_to_string(dir.join("Song.tim.8")).unwrap();
        assert!(copy.contains("version=\"8\""));
        // The same version again makes no such copy.
        save_with_backups(&path, b"<T version=\"9\" />", 9, Some(0), None).unwrap();
        assert!(!dir.join("Song.tim.9").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_sequence_with_no_version_counts_as_version_0() {
        let dir = test_dir("no-version");
        let path = dir.join("Old.tim");
        fs::write(&path, "<T />").unwrap();
        save_with_backups(&path, b"<T version=\"9\"/>", 9, Some(0), None).unwrap();
        assert_eq!(fs::read(dir.join("Old.tim.0")).unwrap(), b"<T />");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn saving_a_new_file_makes_no_backups() {
        let dir = test_dir("new");
        let path = dir.join("New.tim");
        save_with_backups(&path, b"x", 9, Some(0), None).unwrap();
        assert!(!dir.join("auto_backup").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn old_style_backups_move_into_the_backup_folder() {
        let dir = test_dir("old-style");
        let path = dir.join("Song.tim");
        fs::write(&path, "x").unwrap();
        fs::write(dir.join("Song.tim_backup.09-12-2024_10_27_46"), "y").unwrap();
        save_with_backups(&path, b"z", 9, None, None).unwrap();
        assert!(!dir.join("Song.tim_backup.09-12-2024_10_27_46").exists());
        assert!(dir.join("auto_backup/Song.tim_backup.09-12-2024_10_27_46").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn retention_keeps_three_then_one_per_day_for_two_more_days() {
        const DAY: u64 = 86_400;
        // Midday on consecutive local days, so the 12 hours either way cannot cross a date.
        let noon = {
            let base = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
            let t = LocalTime::from(base);
            base - Duration::from_secs((t.hour as u64 * 3600 + t.minute as u64 * 60 + t.second as u64) % DAY)
                + Duration::from_secs(12 * 3600)
        };
        let at = |days_ago: u64, minutes: u64| {
            noon - Duration::from_secs(days_ago * DAY) + Duration::from_secs(minutes * 60)
        };
        let list: Vec<(SystemTime, PathBuf)> = [
            (at(0, 5), "d0-5"),
            (at(0, 4), "d0-4"),
            (at(0, 3), "d0-3"),
            (at(0, 2), "d0-2"),
            (at(1, 2), "d1-2"),
            (at(1, 1), "d1-1"),
            (at(2, 1), "d2-1"),
            (at(3, 1), "d3-1"),
            (at(4, 1), "d4-1"),
        ]
        .into_iter()
        .map(|(t, n)| (t, PathBuf::from(n)))
        .collect();
        let victims: Vec<_> = backups_to_delete(&list).into_iter().map(|p| p.to_string_lossy().into_owned()).collect();
        assert_eq!(victims, ["d0-2", "d1-1", "d3-1", "d4-1"]);
    }

    #[test]
    fn a_system_save_cleans_system_backups_out_of_the_sequence_folder() {
        let dir = test_dir("system");
        let sequences = dir.join("Sequence/auto_backup");
        fs::create_dir_all(&sequences).unwrap();
        fs::write(sequences.join("ModuleStore_11212025_112834.xml"), "x").unwrap();
        fs::write(sequences.join("Carey Grinch_12092024_103210.tim"), "x").unwrap();
        let path = dir.join("SystemData/ModuleStore.xml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "x").unwrap();
        save_with_backups(&path, b"y", 5, None, Some(&sequences)).unwrap();
        assert!(!sequences.join("ModuleStore_11212025_112834.xml").exists());
        assert!(sequences.join("Carey Grinch_12092024_103210.tim").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn root_version_reads_past_the_declaration() {
        let dir = test_dir("peek");
        let path = dir.join("a.xml");
        fs::write(
            &path,
            "\u{feff}<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n<SystemConfig version=\"16\">\r\n</SystemConfig>",
        )
        .unwrap();
        assert_eq!(root_version(&path), Some(16));
        fs::write(&path, "<?xml version=\"1.0\"?><a b=\"c\"/>").unwrap();
        assert_eq!(root_version(&path), None);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn stamps_use_the_twelve_hour_clock() {
        let t = LocalTime { year: 2024, month: 12, day: 9, hour: 22, minute: 32, second: 10 };
        assert_eq!(t.backup_stamp(), "12092024_103210");
        let midnight = LocalTime { year: 2025, month: 1, day: 2, hour: 0, minute: 0, second: 5 };
        assert_eq!(midnight.backup_stamp(), "01022025_120005");
    }

    #[test]
    fn utc_conversion_is_correct_at_known_dates() {
        let t = utc_time(1_700_000_000);
        assert_eq!((t.year, t.month, t.day, t.hour, t.minute, t.second), (2023, 11, 14, 22, 13, 20));
        let t = utc_time(0);
        assert_eq!((t.year, t.month, t.day), (1970, 1, 1));
        let t = utc_time(-1);
        assert_eq!((t.year, t.month, t.day, t.hour), (1969, 12, 31, 23));
    }
}
