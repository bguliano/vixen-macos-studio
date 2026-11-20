//! What a sequencer window's title bar and toolbar show about its sequence: the name, the song, unsaved changes and
//! how rendering is going. The window reads it on every refresh and `sequence_status` reports the same fields, so an
//! agent sees exactly what the toolbar shows.
use crate::sequence::SequenceSession;
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowChrome {
    /// The sequence's name: its file name without the extension, or "Untitled".
    pub title: String,
    /// The window's subtitle: the song, "<song> (audio missing)" when Media lacks it, or "No audio".
    pub subtitle: String,
    /// The song's file name, when the sequence has audio.
    pub song: Option<String>,
    pub song_missing: bool,
    /// Changes not saved yet: the title bar's edited dot.
    pub unsaved: bool,
    pub effects: u32,
    pub rendered: u32,
    /// Some effects are still waiting for a render, so playback would show them late.
    pub rendering: bool,
    /// Rendered effects out of all effects, rounded down; 100 with no effects.
    pub rendering_percent: u32,
    /// The render ring's tooltip.
    pub render_help: String,
}

pub fn window_chrome(s: &SequenceSession, media_dir: &Path) -> WindowChrome {
    let song = s.document().media.first().map(|m| m.file_name.clone()).filter(|n| !n.trim().is_empty());
    let song_missing = song.as_ref().is_some_and(|n| !media_dir.join(n).is_file());
    let subtitle = match &song {
        None => "No audio".to_string(),
        Some(name) => {
            let stem = song_title(name);
            if song_missing { format!("{stem} (audio missing)") } else { stem }
        }
    };
    let effects = s.effect_count() as u32;
    let rendered = s.effects().filter(|e| e.is_rendered()).count() as u32;
    let (rendering, rendering_percent, render_help) = render_state(effects, rendered);
    WindowChrome {
        title: s.name(),
        subtitle,
        song,
        song_missing,
        unsaved: s.is_modified(),
        effects,
        rendered,
        rendering,
        rendering_percent,
        render_help,
    }
}

/// The song's file name without its folder or extension. Sequences from Windows can name it by a Windows path.
fn song_title(name: &str) -> String {
    let file = name.rsplit(['/', '\\']).next().unwrap_or(name);
    Path::new(file).file_stem().map_or_else(|| file.to_string(), |s| s.to_string_lossy().into_owned())
}

/// Whether rendering is still going, its percentage (rounded down, so 100 only when every effect is done) and the
/// ring's tooltip.
fn render_state(effects: u32, rendered: u32) -> (bool, u32, String) {
    let rendering = rendered < effects;
    let percent = if effects == 0 { 100 } else { (u64::from(rendered.min(effects)) * 100 / u64::from(effects)) as u32 };
    let help = match effects {
        0 => "No effects to render yet".to_string(),
        _ if rendering => format!("Rendering effects: {rendered} of {effects} ({percent}%). Playback shows the rest once they finish."),
        1 => "The effect is rendered, so it plays without waiting".to_string(),
        _ => format!("All {effects} effects are rendered, so the sequence plays without waiting"),
    };
    (rendering, percent, help)
}

impl WindowChrome {
    /// The fields `sequence_status` adds, named as the toolbar's state.
    pub fn json_fields(&self) -> serde_json::Map<String, serde_json::Value> {
        let serde_json::Value::Object(map) = serde_json::json!({
            "title": self.title,
            "subtitle": self.subtitle,
            "song": self.song,
            "songMissing": self.song_missing,
            "rendering": self.rendering,
            "renderingPercent": self.rendering_percent,
        }) else {
            unreachable!()
        };
        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AppCore;
    use serde_json::{json, Value};

    fn silent_wav(path: &Path) {
        let data = vec![0u8; 8000 * 2];
        let mut wav = b"RIFF".to_vec();
        wav.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        for v in [16u32] { wav.extend_from_slice(&v.to_le_bytes()); }
        for v in [1u16, 1] { wav.extend_from_slice(&v.to_le_bytes()); }
        for v in [8000u32, 16000] { wav.extend_from_slice(&v.to_le_bytes()); }
        for v in [2u16, 16] { wav.extend_from_slice(&v.to_le_bytes()); }
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
        wav.extend_from_slice(&data);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, wav).unwrap();
    }

    #[test]
    fn song_title_drops_folders_and_extension() {
        assert_eq!(song_title("Wizards in Winter - Trans-Siberian Orchestra.mp3"), "Wizards in Winter - Trans-Siberian Orchestra");
        assert_eq!(song_title("C:\\Users\\me\\Documents\\Vixen 3\\Media\\Carol of the Bells.mp3"), "Carol of the Bells");
    }

    #[test]
    fn render_ring_counts_rendered_effects() {
        assert_eq!(render_state(0, 0), (false, 100, "No effects to render yet".to_string()));
        let (rendering, percent, help) = render_state(692, 290);
        assert!(rendering && percent == 41 && help.contains("290 of 692"), "{help}");
        // Never 100% while one effect is still waiting.
        assert_eq!(render_state(1000, 999).1, 99);
        let (rendering, percent, help) = render_state(692, 692);
        assert!(!rendering && percent == 100 && help.starts_with("All 692"), "{help}");
    }

    #[test]
    fn status_reports_title_song_unsaved_and_render_state() {
        let base = std::env::temp_dir().join(format!("studio-chrome-{}", uuid::Uuid::new_v4()));
        let core = AppCore::open(base.join("profile"), true).unwrap();
        let song = base.join("Downloads/Carol of the Bells.wav");
        silent_wav(&song);
        let run = |name: &str, args: Value| crate::studio_sequence::execute(&core, name, &args, false).unwrap();
        run("library_new_from_audio", json!({"audio": song.to_string_lossy(), "name": "Front Yard"}));
        let id = run("sequence_open", json!({"file": "Front Yard.tim"}))["sequenceId"].clone();
        let status = || run("sequence_status", json!({"sequenceId": id}));

        // The title is the sequence, the subtitle the song (not the profile folder).
        let s = status();
        assert_eq!((s["title"].as_str(), s["subtitle"].as_str()), (Some("Front Yard"), Some("Carol of the Bells")), "{s}");
        assert_eq!(s["song"], "Carol of the Bells.wav");
        assert_eq!((s["modified"].as_bool(), s["rendering"].as_bool(), s["renderingPercent"].as_u64()), (Some(false), Some(false), Some(100)));

        // An edit shows as unsaved until it's saved.
        run("sequence_length", json!({"sequenceId": id, "lengthMs": 4000}));
        assert_eq!(status()["modified"], true);
        run("sequence_save", json!({"sequenceId": id, "file": "Front Yard.tim", "confirm": true}));
        assert_eq!(status()["modified"], false);

        // A song Media no longer has is named as missing.
        std::fs::remove_file(core.paths.media_dir().join("Carol of the Bells.wav")).unwrap();
        let s = status();
        assert_eq!((s["subtitle"].as_str(), s["songMissing"].as_bool()), (Some("Carol of the Bells (audio missing)"), Some(true)));
        let _ = std::fs::remove_dir_all(&base);
    }
}
