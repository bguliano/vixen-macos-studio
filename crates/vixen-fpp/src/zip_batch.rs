//! `EspPixelStickZipBatchUploader`: an ESPixelStick that takes archives gets its sequences five to a ZIP, then one reboot (F78).

use crate::{Cancel, FppError};
use std::collections::HashSet;
use std::fs::File;
use std::hash::{BuildHasher, Hasher};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

pub const BATCH_SIZE: usize = 5;

#[derive(Debug, Clone, PartialEq)]
pub struct ArchiveEntry {
    /// The exported file on disk.
    pub path: PathBuf,
    /// Its name on the device.
    pub filename: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BatchEvent {
    Task { text: String, percent: u32 },
    /// An archive was written, an archive was uploaded, or the reboot was acknowledged: one step of overall progress each.
    PhaseCompleted,
}

/// How many `PhaseCompleted` events a run over `count` files ends up sending.
pub fn phase_count(count: usize) -> usize {
    if count == 0 {
        0
    } else {
        count.div_ceil(BATCH_SIZE) * 2 + 1
    }
}

pub fn validate_entries(entries: &[ArchiveEntry]) -> Result<(), FppError> {
    let mut names = HashSet::new();
    for entry in entries {
        let name = &entry.filename;
        if entry.path.as_os_str().to_string_lossy().trim().is_empty() || name.trim().is_empty() {
            return Err(FppError::Argument("Each ESPixelStick archive entry needs a source path and filename.".into()));
        }
        let flat = !name.contains(['/', '\\']);
        let fseq = name.rfind('.').is_some_and(|dot| name[dot..].eq_ignore_ascii_case(".fseq"));
        if !flat || !fseq {
            return Err(FppError::Argument(format!("'{name}' is not a flat FSEQ filename.")));
        }
        if !names.insert(name.to_lowercase()) {
            return Err(FppError::Argument(format!("Duplicate FSEQ filename '{name}' cannot be archived.")));
        }
        if !entry.path.is_file() {
            return Err(FppError::Argument(format!("FSEQ file '{name}' is missing.")));
        }
    }
    Ok(())
}

/// Everything is checked before anything is written or sent. A failure stops the run with no reboot, and no archive is left
/// in `temp_dir` either way.
pub fn upload_batches(
    entries: &[ArchiveEntry],
    temp_dir: &Path,
    upload_archive: &mut dyn FnMut(&Path, &str) -> Result<(), FppError>,
    reboot: &mut dyn FnMut() -> Result<(), FppError>,
    events: &mut dyn FnMut(BatchEvent),
    cancel: &Cancel,
) -> Result<(), FppError> {
    validate_entries(entries)?;
    if entries.is_empty() {
        return Ok(());
    }
    let total = entries.len().div_ceil(BATCH_SIZE);
    for (index, batch) in entries.chunks(BATCH_SIZE).enumerate() {
        upload_batch(batch, index + 1, total, temp_dir, upload_archive, events, cancel)?;
    }
    request_reboot(reboot, events, cancel)
}

/// `UploadBatchAsync`: one archive of up to five files, written to `temp_dir`, sent, and removed whatever happens. The export
/// wizard calls this as each five sequences are ready.
pub fn upload_batch(
    batch: &[ArchiveEntry],
    part: usize,
    total: usize,
    temp_dir: &Path,
    upload_archive: &mut dyn FnMut(&Path, &str) -> Result<(), FppError>,
    events: &mut dyn FnMut(BatchEvent),
    cancel: &Cancel,
) -> Result<(), FppError> {
    if batch.is_empty() || batch.len() > BATCH_SIZE {
        return Err(FppError::Argument(format!("A ZIP batch must contain between one and {BATCH_SIZE} FSEQ files.")));
    }
    if part == 0 || total == 0 || part > total {
        return Err(FppError::Argument("The ZIP batch number is out of range.".into()));
    }
    validate_entries(batch)?;
    let name = format!("vixen-export-{}-part-{part:03}.zip", random_hex());
    let path = temp_dir.join(&name);
    let outcome = (|| {
        write_archive(&path, batch, part, total, events, cancel)?;
        events(BatchEvent::PhaseCompleted);
        task(events, format!("Uploading ZIP {part}/{total}"), 0);
        upload_archive(&path, &name)?;
        task(events, format!("Uploaded ZIP {part}/{total}"), 100);
        events(BatchEvent::PhaseCompleted);
        Ok(())
    })();
    let _ = std::fs::remove_file(&path);
    outcome
}

/// `RequestRebootAsync`: the one reboot after the last archive.
pub fn request_reboot(reboot: &mut dyn FnMut() -> Result<(), FppError>, events: &mut dyn FnMut(BatchEvent), cancel: &Cancel) -> Result<(), FppError> {
    if cancel.is_cancelled() {
        return Err(FppError::Cancelled);
    }
    task(events, "Requesting ESPixelStick reboot".into(), 0);
    reboot()?;
    task(events, "ESPixelStick reboot acknowledged".into(), 100);
    events(BatchEvent::PhaseCompleted);
    Ok(())
}

fn task(events: &mut dyn FnMut(BatchEvent), text: String, percent: u32) {
    events(BatchEvent::Task { text, percent });
}

fn write_archive(path: &Path, batch: &[ArchiveEntry], part: usize, total: usize, events: &mut dyn FnMut(BatchEvent), cancel: &Cancel) -> Result<(), FppError> {
    let io = |e: std::io::Error| FppError::Io(e.to_string());
    let zip = |e: zip::result::ZipError| FppError::Io(e.to_string());
    task(events, format!("Creating ZIP {part}/{total}"), 0);
    let mut archive = ZipWriter::new(File::create_new(path).map_err(io)?);
    let mut chunk = vec![0u8; 80 * 1024];
    for (index, entry) in batch.iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(FppError::Cancelled);
        }
        let mut source = File::open(&entry.path).map_err(io)?;
        let large = source.metadata().map_err(io)?.len() >= u32::MAX as u64;
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated).compression_level(Some(9)).large_file(large);
        archive.start_file(entry.filename.as_str(), options).map_err(zip)?;
        loop {
            if cancel.is_cancelled() {
                return Err(FppError::Cancelled);
            }
            match source.read(&mut chunk).map_err(io)? {
                0 => break,
                n => archive.write_all(&chunk[..n]).map_err(io)?,
            }
        }
        task(events, format!("Creating ZIP {part}/{total}"), (((index + 1) * 100 / batch.len()) as u32).min(99));
    }
    archive.finish().map_err(zip)?.sync_all().map_err(io)?;
    task(events, format!("Created ZIP {part}/{total}"), 100);
    Ok(())
}

/// 32 hex digits that won't repeat, where upstream has a new GUID.
fn random_hex() -> String {
    let word = || std::collections::hash_map::RandomState::new().build_hasher().finish();
    format!("{:016x}{:016x}", word(), word())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn files(dir: &Path, count: usize) -> Vec<ArchiveEntry> {
        (0..count)
            .map(|i| {
                let path = dir.join(format!("temp-{i}.bin"));
                std::fs::write(&path, vec![i as u8; 5000 + i]).unwrap();
                ArchiveEntry { path, filename: format!("Song {i}.fseq") }
            })
            .collect()
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vixen-fpp-{name}-{}", random_hex()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn entries_are_checked_before_anything_else() {
        let dir = scratch("validate");
        let good = files(&dir, 2);
        assert_eq!(validate_entries(&good), Ok(()));
        let message = |entries: &[ArchiveEntry]| validate_entries(entries).unwrap_err().to_string();
        let with = |name: &str| vec![ArchiveEntry { path: good[0].path.clone(), filename: name.into() }];
        assert_eq!(message(&with(" ")), "Each ESPixelStick archive entry needs a source path and filename.");
        assert_eq!(message(&[ArchiveEntry { path: PathBuf::new(), filename: "a.fseq".into() }]), "Each ESPixelStick archive entry needs a source path and filename.");
        assert_eq!(message(&with("sub/a.fseq")), "'sub/a.fseq' is not a flat FSEQ filename.");
        assert_eq!(message(&with("a.eseq")), "'a.eseq' is not a flat FSEQ filename.");
        assert_eq!(validate_entries(&with("A.FSEQ")), Ok(()));
        let twice = vec![good[0].clone(), ArchiveEntry { path: good[1].path.clone(), filename: "SONG 0.FSEQ".into() }];
        assert_eq!(message(&twice), "Duplicate FSEQ filename 'SONG 0.FSEQ' cannot be archived.");
        assert_eq!(message(&[ArchiveEntry { path: dir.join("gone.bin"), filename: "gone.fseq".into() }]), "FSEQ file 'gone.fseq' is missing.");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn twelve_files_go_up_as_three_archives_and_one_reboot() {
        let dir = scratch("batches");
        let temp = dir.join("out");
        std::fs::create_dir_all(&temp).unwrap();
        let entries = files(&dir, 12);
        let mut uploaded: Vec<(String, Vec<(String, Vec<u8>)>)> = Vec::new();
        let mut reboots = 0;
        let mut events = Vec::new();
        upload_batches(
            &entries,
            &temp,
            &mut |path, name| {
                assert_eq!(path.file_name().unwrap().to_str().unwrap(), name);
                let mut archive = zip::ZipArchive::new(File::open(path).unwrap()).unwrap();
                let mut inside = Vec::new();
                for i in 0..archive.len() {
                    let mut file = archive.by_index(i).unwrap();
                    let mut bytes = Vec::new();
                    file.read_to_end(&mut bytes).unwrap();
                    inside.push((file.name().unwrap().into_owned(), bytes));
                }
                uploaded.push((name.to_string(), inside));
                Ok(())
            },
            &mut || {
                reboots += 1;
                Ok(())
            },
            &mut |event| events.push(event),
            &Cancel::new(),
        )
        .unwrap();

        assert_eq!(reboots, 1);
        assert_eq!(uploaded.iter().map(|(_, inside)| inside.len()).collect::<Vec<_>>(), [5, 5, 2]);
        for (part, (name, _)) in uploaded.iter().enumerate() {
            assert!(name.starts_with("vixen-export-") && name.ends_with(&format!("-part-{:03}.zip", part + 1)), "{name}");
            assert_eq!(name.len(), "vixen-export-".len() + 32 + "-part-000.zip".len());
        }
        let all: Vec<&(String, Vec<u8>)> = uploaded.iter().flat_map(|(_, inside)| inside).collect();
        for (i, (name, bytes)) in all.iter().enumerate() {
            assert_eq!(name, &format!("Song {i}.fseq"));
            assert_eq!(bytes, &vec![i as u8; 5000 + i]);
        }
        assert_eq!(std::fs::read_dir(&temp).unwrap().count(), 0, "no archive is left behind");
        assert_eq!(events.iter().filter(|e| **e == BatchEvent::PhaseCompleted).count(), phase_count(12));

        let tasks: Vec<(String, u32)> = events.iter().filter_map(|e| if let BatchEvent::Task { text, percent } = e { Some((text.clone(), *percent)) } else { None }).collect();
        let t = |text: &str, percent: u32| (text.to_string(), percent);
        assert_eq!(
            tasks[..10],
            [
                t("Creating ZIP 1/3", 0),
                t("Creating ZIP 1/3", 20),
                t("Creating ZIP 1/3", 40),
                t("Creating ZIP 1/3", 60),
                t("Creating ZIP 1/3", 80),
                t("Creating ZIP 1/3", 99),
                t("Created ZIP 1/3", 100),
                t("Uploading ZIP 1/3", 0),
                t("Uploaded ZIP 1/3", 100),
                t("Creating ZIP 2/3", 0)
            ]
        );
        assert_eq!(tasks[tasks.len() - 2..], [t("Requesting ESPixelStick reboot", 0), t("ESPixelStick reboot acknowledged", 100)]);
        assert!(tasks.contains(&t("Creating ZIP 3/3", 50)) && tasks.contains(&t("Creating ZIP 3/3", 99)));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_failed_upload_stops_with_no_reboot_and_no_archive_left() {
        let dir = scratch("fail");
        let entries = files(&dir, 7);
        let temp = dir.join("out");
        std::fs::create_dir_all(&temp).unwrap();
        let mut calls = 0;
        let mut reboots = 0;
        let outcome = upload_batches(
            &entries,
            &temp,
            &mut |_, _| {
                calls += 1;
                Err(FppError::Status { message: "ESPixelStick archive upload of 'x' failed with HTTP 500.".into(), status: 500 })
            },
            &mut || {
                reboots += 1;
                Ok(())
            },
            &mut |_| {},
            &Cancel::new(),
        );
        assert_eq!(outcome.unwrap_err().status(), Some(500));
        assert_eq!((calls, reboots), (1, 0));
        assert_eq!(std::fs::read_dir(&temp).unwrap().count(), 0);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn nothing_to_send_and_a_cancel() {
        let dir = scratch("cancel");
        let mut touched = false;
        upload_batches(&[], &dir, &mut |_, _| unreachable!(), &mut || unreachable!(), &mut |_| touched = true, &Cancel::new()).unwrap();
        assert!(!touched);
        assert_eq!(phase_count(0), 0);

        let cancel = Cancel::new();
        cancel.cancel();
        let outcome = upload_batches(&files(&dir, 1), &dir.join("."), &mut |_, _| unreachable!(), &mut || unreachable!(), &mut |_| {}, &cancel);
        assert_eq!(outcome, Err(FppError::Cancelled));
        assert!(!std::fs::read_dir(&dir).unwrap().any(|e| e.unwrap().file_name().to_string_lossy().ends_with(".zip")));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
