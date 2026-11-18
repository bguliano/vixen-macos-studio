//! Studio: the sequence library shown in the show window. Lists every sequence in the profile's `Sequence` folder with
//! the details people choose a sequence by, read from each file's header so even large shows list instantly.

use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::time::UNIX_EPOCH;

use crate::core::VixenCore;

/// The show at a glance, for the show window's sidebar and overview.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct StudioShowOverview {
    /// Top-level items in the display tree (props and groups as people named them).
    pub top_level_items: u32,
    /// Elements that carry light: tree nodes with an element and no children.
    pub elements: u32,
    pub groups: u32,
    pub controllers: u32,
    pub outputs: u32,
    pub previews: u32,
}

#[uniffi::export]
impl VixenCore {
    pub fn studio_show_overview(&self) -> StudioShowOverview {
        let system = self.inner.system.read();
        let tree = &system.tree;
        let (mut elements, mut groups) = (0u32, 0u32);
        for (_, node) in tree.all_nodes() {
            if !node.children.is_empty() {
                groups += 1;
            } else if node.element_id.is_some() {
                elements += 1;
            }
        }
        StudioShowOverview {
            top_level_items: tree.roots.len() as u32,
            elements,
            groups,
            controllers: system.controllers.len() as u32,
            outputs: system.controllers.iter().map(|c| c.outputs.len() as u32).sum(),
            previews: system.previews.len() as u32,
        }
    }
}

/// One sequence file in the library.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct StudioSequenceSummary {
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
}

/// The sequences in `<data_root>/Sequence`, newest first. Backups and other files are skipped.
#[uniffi::export]
pub fn studio_sequence_library(data_root: String) -> Vec<StudioSequenceSummary> {
    let folder = Path::new(&data_root).join("Sequence");
    let Ok(entries) = std::fs::read_dir(&folder) else { return Vec::new() };
    let mut rows: Vec<StudioSequenceSummary> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext.eq_ignore_ascii_case("tim")))
        .filter_map(|entry| summarize(&entry.path()))
        .collect();
    rows.sort_by(|a, b| b.modified.total_cmp(&a.modified));
    rows
}

fn summarize(path: &Path) -> Option<StudioSequenceSummary> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let modified = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs_f64();
    let header = read_header(path, 96 * 1024);
    Some(StudioSequenceSummary {
        path: path.to_string_lossy().into_owned(),
        name: path.file_stem()?.to_string_lossy().into_owned(),
        modified,
        size_bytes: meta.len(),
        length_ms: element_text(&header, "Length").and_then(parse_iso_duration_ms),
        audio_name: audio_file_name(&header),
    })
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
            _ if c.is_ascii_digit() || c == '.' => { number.push(c); continue }
            _ => return None,
        }
        number.clear();
    }
    for c in time.chars() {
        let unit = match c {
            'H' => 3_600_000.0,
            'M' => 60_000.0,
            'S' => 1_000.0,
            _ if c.is_ascii_digit() || c == '.' => { number.push(c); continue }
            _ => return None,
        };
        total += number.parse::<f64>().ok()? * unit;
        number.clear();
    }
    Some(total)
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

    #[test]
    fn header_fields() {
        let xml = r#"<TimedSequenceData><Length xmlns="">PT1M0S</Length><_dataModels><d1p1:anyType i:type="d2p1:AudioData">
            <d2p1:FilePath>C:\Users\me\Documents\Vixen 3\Media\Song Name.mp3</d2p1:FilePath></d1p1:anyType></_dataModels>"#;
        assert_eq!(element_text(xml, "Length"), Some("PT1M0S"));
        assert_eq!(audio_file_name(xml).as_deref(), Some("Song Name.mp3"));
        assert_eq!(audio_file_name("<Length>PT1S</Length>"), None);
    }
}
