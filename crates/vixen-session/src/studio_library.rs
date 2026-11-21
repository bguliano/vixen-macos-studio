//! Studio: the sequence library behind the show window's Sequences section. Lists every sequence in the profile's
//! `Sequence` folder with what people choose a sequence by (read from each file's header, so even large shows list
//! instantly) and its state: open, unsaved, rendered, missing audio, FSEQ export up to date. Its commands create a
//! sequence from a song, duplicate one, remove a file one of them made (the show window's Undo) and export FSEQ.
use super::{confirm, filename, output};
use crate::{app::AppCore, sequence::SequenceSession};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::{
    fs::File,
    io::Read,
    path::{Component, Path, PathBuf},
    sync::{atomic::AtomicBool, Arc},
    time::UNIX_EPOCH,
};
use uuid::Uuid;
use vixen_core::TimeSpan;

/// One sequence file in the library.
#[derive(Clone, Debug, PartialEq)]
pub struct LibraryEntry {
    pub path: String,
    /// The file name without its extension.
    pub name: String,
    /// Seconds since 1970, from the file's modification date.
    pub modified: f64,
    pub size_bytes: u64,
    /// The sequence length in milliseconds, when the header states it.
    pub length_ms: Option<f64>,
    /// The audio file's name (not its full path, which may be a Windows path), when the sequence has audio.
    pub audio_name: Option<String>,
    /// The sequence names audio that the profile's Media folder doesn't have, so it plays silent.
    pub audio_missing: bool,
    /// Open in this Studio session (an app window or an agent).
    pub open: bool,
    /// Open with changes not yet saved to the file.
    pub unsaved: bool,
    /// For an open sequence: its effects, and how many of them are rendered. None when closed, or busy right now.
    pub effects: Option<u32>,
    pub rendered: Option<u32>,
    /// `Export/<name>.fseq` exists and is newer than the saved sequence, with no unsaved changes since.
    pub fseq_current: bool,
}

impl LibraryEntry {
    pub fn to_json(&self) -> Value {
        json!({
            "file": Path::new(&self.path).file_name().map(|n| n.to_string_lossy().into_owned()),
            "path": self.path,
            "name": self.name,
            "modified": self.modified,
            "sizeBytes": self.size_bytes,
            "lengthMs": self.length_ms,
            "audio": self.audio_name,
            "audioMissing": self.audio_missing,
            "open": self.open,
            "unsaved": self.unsaved,
            "effects": self.effects,
            "rendered": self.rendered,
            "fseqCurrent": self.fseq_current,
        })
    }
}

/// The sequences in the profile's `Sequence` folder, newest first. Backups, scratch files and other files are skipped.
pub fn list(core: &AppCore) -> Vec<LibraryEntry> {
    let folder = core.paths.root().join("Sequence");
    let Ok(entries) = std::fs::read_dir(&folder) else { return Vec::new() };
    let open = open_states(core);
    let media = core.paths.media_dir();
    let export = core.paths.root().join("Export");
    let mut rows: Vec<LibraryEntry> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("tim")))
        .filter_map(|path| summarize(&path, &open, &media, &export))
        .collect();
    rows.sort_by(|a, b| b.modified.total_cmp(&a.modified));
    rows
}

/// What an open session says about its file: unsaved, effects, rendered. Sessions busy right now (a long save or
/// export holds the lock) are skipped rather than waited for, so listing never stalls the window.
struct OpenState {
    path: PathBuf,
    unsaved: bool,
    counts: Option<(u32, u32)>,
}

fn open_states(core: &AppCore) -> Vec<OpenState> {
    core.open_sequence_sessions()
        .into_iter()
        .filter_map(|shared| {
            let mut s = shared.try_lock()?;
            let path = canonical(s.path()?);
            s.apply_finished_renders();
            let effects = s.effect_count() as u32;
            let rendered = s.effects().filter(|e| e.is_rendered()).count() as u32;
            Some(OpenState { path, unsaved: s.is_modified(), counts: Some((effects, rendered)) })
        })
        .collect()
}

fn summarize(path: &Path, open: &[OpenState], media: &Path, export: &Path) -> Option<LibraryEntry> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let modified = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs_f64();
    let (length_ms, audio_name) = header_fields(path, modified, meta.len());
    let name = path.file_stem()?.to_string_lossy().into_owned();
    let wanted = canonical(path);
    let sessions: Vec<&OpenState> = open.iter().filter(|o| o.path == wanted).collect();
    let unsaved = sessions.iter().any(|o| o.unsaved);
    let counts = sessions.iter().find_map(|o| o.counts);
    let fseq_current = !unsaved
        && std::fs::metadata(export.join(format!("{name}.fseq")))
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .is_some_and(|t| t.as_secs_f64() >= modified);
    Some(LibraryEntry {
        path: path.to_string_lossy().into_owned(),
        audio_missing: audio_name.as_ref().is_some_and(|a| !media.join(a).is_file()),
        name,
        modified,
        size_bytes: meta.len(),
        length_ms,
        audio_name,
        open: !sessions.is_empty(),
        unsaved,
        effects: counts.map(|c| c.0),
        rendered: counts.map(|c| c.1),
        fseq_current,
    })
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// The length and audio name from a file's header, read again only when its modification time or size changed, so the
/// show window can refresh open sequences' badges every few seconds without rereading every file.
fn header_fields(path: &Path, modified: f64, size: u64) -> (Option<f64>, Option<String>) {
    type Cached = (f64, u64, Option<f64>, Option<String>);
    static CACHE: std::sync::LazyLock<Mutex<std::collections::HashMap<PathBuf, Cached>>> = std::sync::LazyLock::new(Default::default);
    if let Some((m, s, length, audio)) = CACHE.lock().get(path) {
        if *m == modified && *s == size {
            return (*length, audio.clone());
        }
    }
    let header = read_header(path, 96 * 1024);
    let fields = (element_text(&header, "Length").and_then(parse_iso_duration_ms), audio_file_name(&header));
    let mut cache = CACHE.lock();
    if cache.len() > 4096 {
        cache.clear();
    }
    cache.insert(path.to_path_buf(), (modified, size, fields.0, fields.1.clone()));
    fields
}

fn read_header(path: &Path, limit: usize) -> String {
    let mut buffer = Vec::with_capacity(limit);
    if let Ok(file) = File::open(path) {
        let _ = file.take(limit as u64).read_to_end(&mut buffer);
    }
    String::from_utf8_lossy(&buffer).into_owned()
}

/// The text of the first `<name ...>text</name>` element.
fn element_text<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}");
    let mut from = 0;
    while let Some(found) = xml[from..].find(&open) {
        let start = from + found + open.len();
        let next = xml[start..].chars().next()?;
        if next == '>' || next == ' ' {
            let body = start + xml[start..].find('>')? + 1;
            let end = body + xml[body..].find('<')?;
            return Some(xml[body..end].trim());
        }
        from = start;
    }
    None
}

/// The audio file's name from the `AudioData` media model, the first `FilePath` in the header.
fn audio_file_name(xml: &str) -> Option<String> {
    let audio = xml.find("AudioData")?;
    let rest = &xml[audio..];
    let open = rest.find("FilePath>")? + "FilePath>".len();
    let end = open + rest[open..].find('<')?;
    let full = rest[open..end].trim();
    let name = full.rsplit(['\\', '/']).next()?.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// Milliseconds in an `xs:duration` such as `PT2M36.499S` or `P1DT1H`.
fn parse_iso_duration_ms(text: &str) -> Option<f64> {
    let body = text.strip_prefix('P')?;
    let (days, time) = match body.split_once('T') {
        Some((d, t)) => (d, t),
        None => (body, ""),
    };
    let mut total = 0.0;
    let mut number = String::new();
    for c in days.chars() {
        match c {
            'D' => total += number.parse::<f64>().ok()? * 86_400_000.0,
            _ if c.is_ascii_digit() || c == '.' => {
                number.push(c);
                continue;
            }
            _ => return None,
        }
        number.clear();
    }
    for c in time.chars() {
        let unit = match c {
            'H' => 3_600_000.0,
            'M' => 60_000.0,
            'S' => 1_000.0,
            _ if c.is_ascii_digit() || c == '.' => {
                number.push(c);
                continue;
            }
            _ => return None,
        };
        total += number.parse::<f64>().ok()? * unit;
        number.clear();
    }
    Some(total)
}

// MARK: - Commands

/// A sequence file in the profile's Sequence folder, by filename.
fn library_file(core: &AppCore, a: &Value) -> Result<PathBuf, String> {
    let name = filename(a, "sequence")?;
    if !name.to_lowercase().ends_with(".tim") {
        return Err("sequence must be a .tim filename from library_list".into());
    }
    let path = output(core, "Sequence", &name)?;
    if !path.is_file() {
        return Err(format!("The profile has no sequence named {name}"));
    }
    Ok(path)
}

/// `<stem>.tim`, or `<stem> 2.tim`, `<stem> 3.tim` … the first name the Sequence folder doesn't have yet.
fn unused_sequence_path(core: &AppCore, stem: &str) -> Result<PathBuf, String> {
    let stem = stem.trim();
    if stem.is_empty() || stem.contains(['/', '\\', ':']) || stem.starts_with('.') || stem.len() > 200 {
        return Err("Use a sequence name without slashes, colons or a leading dot".into());
    }
    for n in 1..1000 {
        let file = if n == 1 { format!("{stem}.tim") } else { format!("{stem} {n}.tim") };
        let path = output(core, "Sequence", &file)?;
        if std::fs::symlink_metadata(&path).is_err() {
            return Ok(path);
        }
    }
    Err("Too many sequences with that name".into())
}

/// Copies `from` to a new file at `to` through a temporary file beside it, never replacing a file already there.
fn copy_new(from: &Path, to: &Path) -> Result<(), String> {
    let folder = to.parent().ok_or("No folder")?;
    let temporary = folder.join(format!(".vixen-studio-{}.part", Uuid::new_v4()));
    std::fs::copy(from, &temporary).map_err(|e| e.to_string())?;
    // A hard link fails if the name was taken meanwhile, where a rename would replace it.
    let linked = std::fs::hard_link(&temporary, to).map_err(|e| e.to_string());
    let _ = std::fs::remove_file(&temporary);
    linked
}

/// What library_new_from_audio and library_duplicate return: enough for library_discard to recognise the file later.
fn created_json(path: &Path) -> Result<Value, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    let modified = meta.modified().map_err(|e| e.to_string())?.duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?;
    Ok(json!({
        "file": path.file_name().map(|n| n.to_string_lossy().into_owned()),
        "path": path,
        "name": path.file_stem().map(|n| n.to_string_lossy().into_owned()),
        "modified": modified.as_secs_f64(),
        "sizeBytes": meta.len(),
    }))
}

/// The open session on `path`, the oldest one, as the app reuses.
fn open_session(core: &AppCore, path: &Path) -> Option<Arc<Mutex<SequenceSession>>> {
    let wanted = canonical(path);
    core.open_sequence_sessions()
        .into_iter()
        .find(|s| s.lock().path().is_some_and(|p| canonical(p) == wanted))
}

/// `AudioData`, as the sequencer's Add Audio writes it, so the file names its song for Windows Vixen and the library.
pub(super) fn audio_data_model(instance_id: &Uuid, path: &Path) -> vixen_xml::XElement {
    const NS: &str = "http://schemas.datacontract.org/2004/07/VixenModules.Media.Audio";
    let mut model = vixen_xml::XElement::new("http://schemas.microsoft.com/2003/10/Serialization/Arrays", "anyType");
    model.xsi_type = Some((NS.to_string(), "AudioData".to_string()));
    model.push(vixen_xml::XElement::with_text("", "ModuleInstanceId", instance_id.to_string()));
    model.push(vixen_xml::XElement::with_text("", "ModuleTypeId", crate::export::AUDIO_MEDIA_TYPE.to_string()));
    model.push(vixen_xml::XElement::with_text(NS, "FilePath", path.to_string_lossy().into_owned()));
    model
}

/// The song for library_new_from_audio: an absolute path to an audio file anywhere, or a filename in the profile's
/// Media folder. Returns the file to read and its name in Media.
pub(super) fn audio_source(core: &AppCore, a: &Value) -> Result<(PathBuf, String), String> {
    let audio = a["audio"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 1024)
        .ok_or("audio must be a nonempty string (max 1024 bytes)")?;
    let path = if audio.starts_with('/') {
        let path = Path::new(audio);
        if path.components().any(|c| matches!(c, Component::ParentDir | Component::CurDir)) {
            return Err("Use an absolute path without . or .. components".into());
        }
        path.to_path_buf()
    } else {
        core.paths.media_dir().join(filename(a, "audio")?)
    };
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).ok_or("audio has no file name")?;
    let extension = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if !crate::sequencer::AUDIO_EXTENSIONS.contains(&extension.as_str()) {
        return Err(format!("{name} isn't an audio file Vixen plays (use MP3, WAV, M4A, FLAC, AIFF, OGG and the like)"));
    }
    if !path.is_file() {
        return Err(format!("{audio}: no such audio file"));
    }
    Ok((path, name))
}

/// Puts the song in the profile's Media folder, as the sequencer's Add Audio does (`MediaService.ImportMedia`), unless
/// it's already there. A different file with the same name is never replaced. Returns the Media file and whether this
/// copied it.
pub(super) fn import_audio(core: &AppCore, source: &Path, name: &str) -> Result<(PathBuf, bool), String> {
    let existing = core.paths.media_dir().join(name);
    if existing.is_file() {
        if canonical(&existing) == canonical(source) {
            return Ok((existing, false));
        }
        let same_size = std::fs::metadata(&existing).ok().map(|m| m.len()) == std::fs::metadata(source).ok().map(|m| m.len());
        if same_size {
            return Ok((existing, false));
        }
        return Err(format!(
            "The show's Media folder already has a different file named {name}. Rename the song file and try again, or use the one in Media."
        ));
    }
    // `output` refuses a Media folder that is a link or leads outside the profile, so nothing is ever written elsewhere.
    let target = output(core, "Media", name)?;
    copy_new(source, &target)?;
    Ok((target, true))
}

fn new_from_audio(core: &AppCore, a: &Value) -> Result<Value, String> {
    let (source, media_name) = audio_source(core, a)?;
    // Decode first, so a file Vixen can't play adds nothing to Media or Sequence.
    let probe = vixen_audio::AudioClip::load(&source).map_err(|e| format!("Can't read {media_name}: {e}"))?;
    let length = probe.duration();
    if length < TimeSpan::from_ms(25.0) {
        return Err(format!("{media_name} has no audio Vixen can play"));
    }
    let stem = match a.get("name") {
        Some(_) => super::string(a, "name")?.to_string(),
        None => Path::new(&media_name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or(media_name.clone()),
    };
    let path = unused_sequence_path(core, &stem)?;
    let (media_path, copied) = import_audio(core, &source, &media_name)?;
    let mut s = SequenceSession::new_untitled(&core.session_environment());
    s.set_length(length);
    let instance_id = Uuid::new_v4();
    s.document_mut().media.push(vixen_io::sequence::MediaEntry {
        type_id: crate::export::AUDIO_MEDIA_TYPE,
        instance_id,
        file_name: media_name.clone(),
        data: Some(audio_data_model(&instance_id, &media_path)),
    });
    s.document_mut().selected_timing_provider =
        vixen_io::SelectedTimingProvider { provider_type: Some("Media".to_string()), source_name: Some(media_name.clone()) };
    // Written beside it and linked into place, so a file another writer created meanwhile is never replaced.
    let text = vixen_io::sequence::sequence_to_document(&s.begin_save()).map_err(|e| e.to_string())?;
    let temporary = path.with_file_name(format!(".vixen-studio-{}.part", Uuid::new_v4()));
    std::fs::write(&temporary, text.as_bytes()).map_err(|e| e.to_string())?;
    let linked = std::fs::hard_link(&temporary, &path).map_err(|e| format!("{}: {e}", path.display()));
    let _ = std::fs::remove_file(&temporary);
    linked?;
    let mut result = created_json(&path)?;
    result["lengthMs"] = json!(length.total_ms());
    result["audio"] = json!(media_name);
    result["copiedAudio"] = json!(copied);
    Ok(result)
}

fn duplicate(core: &AppCore, a: &Value) -> Result<Value, String> {
    let source = library_file(core, a)?;
    let stem = source.file_stem().map(|s| s.to_string_lossy().into_owned()).ok_or("No name")?;
    let target = unused_sequence_path(core, &format!("{stem} copy"))?;
    copy_new(&source, &target)?;
    created_json(&target)
}

/// Removes a file library_new_from_audio or library_duplicate made, only while it is exactly as they left it, so Undo
/// never throws away work: same modification time and size, and not open anywhere.
fn discard(core: &AppCore, a: &Value) -> Result<Value, String> {
    confirm(a)?;
    let path = library_file(core, a)?;
    let expected_modified = a["modified"].as_f64().ok_or("modified must be the number library_new_from_audio or library_duplicate returned")?;
    let expected_size = a["sizeBytes"].as_u64().ok_or("sizeBytes must be the number library_new_from_audio or library_duplicate returned")?;
    let meta = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
    let modified = meta.modified().map_err(|e| e.to_string())?.duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_secs_f64();
    if !meta.is_file() || meta.len() != expected_size || (modified - expected_modified).abs() > 0.001 {
        return Err("The sequence has changed since it was created, so it is kept".into());
    }
    if open_session(core, &path).is_some() {
        return Err("The sequence is open; close it first".into());
    }
    std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    Ok(json!({"removed": path}))
}

/// Renders a library sequence and writes it as uncompressed FSEQ v2 at 50 ms under the profile's Export folder. An
/// open sequence is exported as it stands, unsaved changes included; a closed one is read, exported and let go.
fn export_fseq(core: &AppCore, a: &Value) -> Result<Value, String> {
    confirm(a)?;
    let source = library_file(core, a)?;
    let file = match a.get("file") {
        Some(_) => filename(a, "file")?,
        None => format!("{}.fseq", source.file_stem().map(|s| s.to_string_lossy().into_owned()).ok_or("No name")?),
    };
    if !file.to_lowercase().ends_with(".fseq") {
        return Err("Use .fseq extension".into());
    }
    let path = output(core, "Export", &file)?;
    let shared = match open_session(core, &source) {
        Some(shared) => shared,
        None => Arc::new(Mutex::new(SequenceSession::open(&source, &core.session_environment()).map_err(|e| e.to_string())?)),
    };
    let system = core.system.read().clone();
    let mut s = shared.lock();
    let request = crate::export::ExportRequest {
        format: vixen_export::ExportFormat::FseqV2,
        compress: false,
        out_file_name: path.clone(),
        update_interval_ms: 50,
        audio_filename: crate::export::audio_file_name(s.document(), &core.paths),
        match_audio_name: false,
        controllers: Vec::new(),
    };
    s.render_all_dirty();
    let export = crate::export::PreparedExport::prepare(&mut s, &system, &request).map_err(|e| e.to_string())?;
    let unsaved = s.is_modified();
    drop(s);
    export.run(&AtomicBool::new(false), &mut |_, _| {}).map_err(|e| e.to_string())?;
    Ok(json!({"path": path, "includesUnsavedChanges": unsaved}))
}

pub fn execute(core: &AppCore, name: &str, a: &Value) -> Result<Value, String> {
    match name {
        "library_list" => Ok(json!({"sequences": list(core).iter().map(LibraryEntry::to_json).collect::<Vec<_>>()})),
        "library_new_from_audio" => new_from_audio(core, a),
        "library_duplicate" => duplicate(core, a),
        "library_discard" => discard(core, a),
        "library_export_fseq" => export_fseq(core, a),
        _ => Err("Unknown library command".into()),
    }
}

pub fn tools() -> Vec<(&'static str, &'static str, Value, Vec<&'static str>, bool)> {
    let string = json!({"type":"string","minLength":1,"maxLength":256});
    let path = json!({"type":"string","minLength":1,"maxLength":1024});
    vec![
        ("library_list","The show window's Sequences list: every .tim in the profile's Sequence folder, newest first, with file, name, modified (seconds since 1970), sizeBytes, lengthMs, audio (its file name), audioMissing (Media lacks that file, so it plays silent), open (in this Studio session), unsaved (open with changes not saved), effects/rendered counts for open ones (null when closed) and fseqCurrent (Export/<name>.fseq is newer than the saved file). Read-only.",json!({}),vec![],true),
        ("library_new_from_audio","Create a sequence for a song, as dropping an audio file on the show window's Sequences list does: copies the song into the profile's Media folder unless it is already there (a different file with the same name is refused, never replaced), and saves Sequence/<name>.tim as long as the song with it as audio and timing source (name defaults to the song's; ' 2', ' 3' … when taken). audio is an absolute path or a Media filename. Returns file, path, modified and sizeBytes (for library_discard), lengthMs, audio and copiedAudio; open it with sequence_open.",json!({"audio":path,"name":string}),vec!["audio"],false),
        ("library_duplicate","Copy a library sequence (a filename from library_list) to '<name> copy.tim' (' copy 2' … when taken), as its context menu's Duplicate does. Copies the saved file, not unsaved changes in an open window. Returns file, path, modified and sizeBytes.",json!({"sequence":string}),vec!["sequence"],false),
        ("library_discard","Remove a sequence file that library_new_from_audio or library_duplicate made, only while it is unchanged since (modified and sizeBytes as they returned) and not open; this is the show window's Undo for them. A song copied into Media stays. Requires confirmation.",json!({"sequence":string,"modified":{"type":"number"},"sizeBytes":{"type":"integer","minimum":0},"confirm":{"type":"boolean"}}),vec!["sequence","modified","sizeBytes","confirm"],false),
        ("library_export_fseq","Render a library sequence and export uncompressed FSEQ v2 at 50 ms to the profile's Export folder (file defaults to '<name>.fseq', overwritten), as its context menu's Export FSEQ does. An open sequence is exported as it stands, unsaved changes included (includesUnsavedChanges). Requires confirmation; no network transfer.",json!({"sequence":string,"file":string,"confirm":{"type":"boolean"}}),vec!["sequence","confirm"],false),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(parse_iso_duration_ms("PT2M36.499S"), Some(156_499.0));
        assert_eq!(parse_iso_duration_ms("PT1H"), Some(3_600_000.0));
        assert_eq!(parse_iso_duration_ms("P1DT0.5S"), Some(86_400_500.0));
        assert_eq!(parse_iso_duration_ms("garbage"), None);
    }

    /// A silent mono 16-bit WAV of `seconds`, plus `extra` bytes of silence so two songs can differ in size.
    fn silent_wav(path: &Path, seconds: f64, extra: usize) {
        let rate = 8000u32;
        let data = vec![0u8; (rate as f64 * seconds) as usize * 2 + extra * 2];
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&rate.to_le_bytes());
        wav.extend_from_slice(&(rate * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
        wav.extend_from_slice(&data);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, wav).unwrap();
    }

    #[test]
    fn new_from_audio_badges_duplicate_discard_and_export() {
        let base = std::env::temp_dir().join(format!("studio-library-{}", Uuid::new_v4()));
        let core = AppCore::open(base.join("profile"), true).unwrap();
        let song = base.join("Downloads/My Song.wav");
        silent_wav(&song, 3.0, 0);
        let run = |name: &str, args: Value| super::super::execute(&core, name, &args, false).unwrap();
        let fail = |name: &str, args: Value| super::super::execute(&core, name, &args, false).unwrap_err();
        let entry = |name: &str| list(&core).into_iter().find(|e| e.name == name).unwrap();

        // Dropping a song: copied into Media, saved as a sequence named after it, as long as the song.
        let made = run("library_new_from_audio", json!({"audio": song.to_string_lossy()}));
        assert_eq!(made["file"], "My Song.tim");
        assert_eq!(made["copiedAudio"], true);
        assert!((made["lengthMs"].as_f64().unwrap() - 3000.0).abs() < 1.0, "{made}");
        assert!(core.paths.media_dir().join("My Song.wav").is_file());
        let row = entry("My Song");
        assert_eq!(row.audio_name.as_deref(), Some("My Song.wav"));
        assert!(!row.audio_missing && !row.open && !row.unsaved && !row.fseq_current);
        assert_eq!(row.effects, None);
        assert!((row.length_ms.unwrap() - 3000.0).abs() < 1.0);
        let opened = run("sequence_open", json!({"file": "My Song.tim"}));
        let id = opened["sequenceId"].clone();
        assert_eq!(run("sequence_status", json!({"sequenceId": id}))["modified"], false);

        // The same song again reuses the Media copy; a different file by that name is refused, adding nothing.
        let again = run("library_new_from_audio", json!({"audio": song.to_string_lossy()}));
        assert_eq!((again["file"].as_str(), again["copiedAudio"].as_bool()), (Some("My Song 2.tim"), Some(false)));
        let other = base.join("Elsewhere/My Song.wav");
        silent_wav(&other, 3.0, 400);
        assert!(fail("library_new_from_audio", json!({"audio": other.to_string_lossy()})).contains("different file"));
        assert!(!core.paths.root().join("Sequence/My Song 3.tim").exists());
        assert!(fail("library_new_from_audio", json!({"audio": base.join("notes.txt").to_string_lossy()})).contains("audio file"));
        let named = run("library_new_from_audio", json!({"audio": "My Song.wav", "name": "Finale"}));
        assert_eq!(named["file"], "Finale.tim");
        assert!(fail("library_new_from_audio", json!({"audio": "My Song.wav", "name": "../x"})).contains("slashes"));

        // Badges follow the open document: open, then unsaved after an edit, with render counts.
        let row = entry("My Song");
        assert!(row.open && !row.unsaved);
        assert_eq!((row.effects, row.rendered), (Some(0), Some(0)));
        run("sequence_length", json!({"sequenceId": id, "lengthMs": 4000}));
        assert!(entry("My Song").unsaved);
        assert!(!entry("My Song 2").unsaved, "only that file's badge changes");

        // Missing audio: the Media file gone.
        std::fs::rename(core.paths.media_dir().join("My Song.wav"), base.join("moved.wav")).unwrap();
        assert!(entry("My Song 2").audio_missing);
        std::fs::rename(base.join("moved.wav"), core.paths.media_dir().join("My Song.wav")).unwrap();
        assert!(!entry("My Song 2").audio_missing);

        // Duplicate, and Undo's discard: only while unchanged and closed.
        let copy = run("library_duplicate", json!({"sequence": "My Song 2.tim"}));
        assert_eq!(copy["file"], "My Song 2 copy.tim");
        assert_eq!(
            std::fs::read(core.paths.root().join("Sequence/My Song 2 copy.tim")).unwrap(),
            std::fs::read(core.paths.root().join("Sequence/My Song 2.tim")).unwrap()
        );
        assert_eq!(run("library_duplicate", json!({"sequence": "My Song 2.tim"}))["file"], "My Song 2 copy 2.tim");
        let discard = |made: &Value, size: u64| json!({"sequence": made["file"], "modified": made["modified"], "sizeBytes": size, "confirm": true});
        let size = copy["sizeBytes"].as_u64().unwrap();
        assert!(fail("library_discard", discard(&copy, size + 1)).contains("changed"));
        assert!(fail("library_discard", json!({"sequence": "My Song 2 copy.tim", "modified": copy["modified"], "sizeBytes": size})).contains("confirm"));
        run("library_discard", discard(&copy, size));
        assert!(!core.paths.root().join("Sequence/My Song 2 copy.tim").exists());
        assert!(fail("library_discard", discard(&made, made["sizeBytes"].as_u64().unwrap())).contains("open"));
        assert!(fail("library_duplicate", json!({"sequence": "../My Song.tim"})).contains("filename"));
        assert!(fail("library_duplicate", json!({"sequence": "Nope.tim"})).contains("no sequence"));

        // Export FSEQ: an open sequence as it stands (unsaved length included), a closed one read and let go.
        let exported = run("library_export_fseq", json!({"sequence": "My Song.tim", "confirm": true}));
        assert_eq!(exported["includesUnsavedChanges"], true);
        assert!(core.paths.root().join("Export/My Song.fseq").is_file());
        assert!(!entry("My Song").fseq_current, "unsaved changes make the export stale");
        run("library_export_fseq", json!({"sequence": "Finale.tim", "confirm": true}));
        assert!(entry("Finale").fseq_current);
        assert!(!entry("Finale").open, "the export let the closed sequence go");
        assert!(fail("library_export_fseq", json!({"sequence": "Finale.tim"})).contains("confirm"));
        run("sequence_close", json!({"sequenceId": id, "confirm": true}));
        assert!(!entry("My Song").open);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn header_fields() {
        let xml = r#"<TimedSequenceData><Length xmlns="">PT1M0S</Length><_dataModels><d1p1:anyType i:type="d2p1:AudioData">
            <d2p1:FilePath>C:\Users\me\Documents\Vixen 3\Media\Song Name.mp3</d2p1:FilePath></d1p1:anyType></_dataModels>"#;
        assert_eq!(element_text(xml, "Length"), Some("PT1M0S"));
        assert_eq!(audio_file_name(xml).as_deref(), Some("Song Name.mp3"));
        assert_eq!(audio_file_name("<Length>PT1S</Length>"), None);
    }
}
