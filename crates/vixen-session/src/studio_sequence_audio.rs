//! Changing or removing a sequence's song as one undo step (B3): the media entries, the timing source and the decoded
//! clip go back together, with the length when the change set it. The sequencer's Add, Change and Remove Audio record
//! it (`before` and `settled` around their edit), and `sequence_audio` does the same for agents and the inspector. An
//! open window's waveform and speed controls follow the session's song on their own (`studio_audio_stale`, `poll`).
use super::{library, record};
use crate::{sequence::SequenceSession, undo::UndoAction};
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;
use vixen_core::TimeSpan;

pub fn tools() -> Vec<(&'static str, &'static str, Value, Vec<&'static str>, bool)> {
    vec![(
        "sequence_audio",
        "Change or remove the sequence's song, as the sequencer's Change Audio… and Remove Audio do, in one undo step that puts back the old song, its timing source and its length. audio is an absolute path to an audio file anywhere (copied into the show's Media folder unless it is already there; a different file with that name is refused, never replaced) or a filename already in Media; remove:true removes the song instead. The sequence takes the song's length when it still has the default length, or when fitLength is true (refused if an effect ends after the song). The song becomes the timing source, so playback follows it. Returns song, durationMs, lengthMs, copiedAudio and changed.",
        json!({"sequenceId":{"type":"string","minLength":1,"maxLength":256},"audio":{"type":"string","minLength":1,"maxLength":1024},"remove":{"type":"boolean"},"fitLength":{"type":"boolean"}}),
        vec!["sequenceId"],
        false,
    )]
}

/// The song as it is now, to record once the edit is done (`settled`).
pub fn before(s: &SequenceSession, label: &str) -> UndoAction {
    UndoAction::StudioAudio {
        label: label.into(),
        media: s.document().media.clone(),
        timing: s.document().selected_timing_provider.clone(),
        clip: s.audio().cloned(),
        length: Some(s.length()),
    }
}

/// `before`'s action, keeping the length only if the edit changed it, so Undo never undoes a resize made since.
pub fn settled(action: UndoAction, s: &SequenceSession) -> UndoAction {
    match action {
        UndoAction::StudioAudio { label, media, timing, clip, length } => {
            let length = length.filter(|l| *l != s.length());
            UndoAction::StudioAudio { label, media, timing, clip, length }
        }
        other => other,
    }
}

/// "Add Audio" with none yet, otherwise "Change Audio".
pub fn change_label(s: &SequenceSession) -> &'static str {
    if s.document().media.is_empty() { "Add Audio" } else { "Change Audio" }
}

/// `sequence_audio`.
pub fn execute(core: &crate::app::AppCore, s: &mut SequenceSession, a: &Value, agent: bool) -> Result<Value, String> {
    if a["remove"] == true {
        if a.get("audio").is_some() || a.get("fitLength").is_some() {
            return Err("Give audio, or remove:true, not both".into());
        }
        if s.document().media.is_empty() {
            return Ok(json!({"song": null, "durationMs": null, "lengthMs": s.length().total_ms(), "copiedAudio": false, "changed": false}));
        }
        let undo = before(s, "Remove Audio");
        // As the sequencer's Remove Audio: the timing source stays, as upstream's does.
        s.document_mut().media.clear();
        s.set_audio_clip(None);
        s.set_modified();
        let undo = settled(undo, s);
        record(s, "Remove Audio", undo, agent);
        return Ok(json!({"song": null, "durationMs": null, "lengthMs": s.length().total_ms(), "copiedAudio": false, "changed": true}));
    }
    if a.get("audio").is_none() {
        return Err("Give audio (a path or a Media filename), or remove:true".into());
    }
    let (source, name) = library::audio_source(core, a)?;
    // Decode first, so a file Vixen can't play changes nothing and adds nothing to Media.
    let clip = Arc::new(vixen_audio::AudioClip::load(&source).map_err(|e| format!("Can't read {name}: {e}"))?);
    let duration = clip.duration();
    if duration < TimeSpan::from_ms(25.0) {
        return Err(format!("{name} has no audio Vixen can play"));
    }
    let fit = a["fitLength"] == true;
    if fit && s.effects().any(|e| e.end() > duration) {
        return Err(format!("An effect ends after the song ({}); move it or leave fitLength off", duration.total_ms()));
    }
    let (path, copied) = library::import_audio(core, &source, &name)?;
    let label = change_label(s);
    let undo = before(s, label);
    let instance_id = Uuid::new_v4();
    let document = s.document_mut();
    document.media.clear();
    document.media.push(vixen_io::MediaEntry {
        type_id: crate::export::AUDIO_MEDIA_TYPE,
        instance_id,
        file_name: name.clone(),
        data: Some(library::audio_data_model(&instance_id, &path)),
    });
    document.selected_timing_provider =
        vixen_io::SelectedTimingProvider { provider_type: Some("Media".to_string()), source_name: Some(name.clone()) };
    s.set_audio_clip(Some(clip));
    if fit || s.length() == crate::sequencer::DEFAULT_SEQUENCE_LENGTH {
        s.set_length(duration);
    }
    s.set_modified();
    s.render_all_dirty();
    let undo = settled(undo, s);
    record(s, label, undo, agent);
    Ok(json!({"song": name, "durationMs": duration.total_ms(), "lengthMs": s.length().total_ms(), "copiedAudio": copied, "changed": true}))
}

#[cfg(test)]
mod tests {
    use crate::app::AppCore;
    use serde_json::{json, Value};
    use std::path::Path;

    /// A short mono 16-bit WAV of silence.
    fn wav(path: &Path, seconds: f64) {
        let rate = 8000u32;
        let frames = (rate as f64 * seconds) as u32;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + frames * 2).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&rate.to_le_bytes());
        bytes.extend_from_slice(&(rate * 2).to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&(frames * 2).to_le_bytes());
        bytes.resize(bytes.len() + frames as usize * 2, 0);
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn changing_and_removing_the_song_are_one_undo_step_each() {
        let base = std::env::temp_dir().join(format!("studio-audio-{}", uuid::Uuid::new_v4()));
        let core = AppCore::open(base.join("profile"), true).unwrap();
        let songs = base.join("songs");
        std::fs::create_dir_all(&songs).unwrap();
        wav(&songs.join("first.wav"), 2.0);
        wav(&songs.join("second.wav"), 3.0);
        let run = |name: &str, args: Value| crate::studio_sequence::execute(&core, name, &args, false);
        let id = run("sequence_create", json!({"lengthMs": 10000})).unwrap()["sequenceId"].clone();
        let song = || run("sequence_overview", json!({"sequenceId": id})).unwrap()["audio"].clone();
        let length = || run("sequence_overview", json!({"sequenceId": id})).unwrap()["lengthMs"].as_f64().unwrap();
        let undo_list = || run("sequence_status", json!({"sequenceId": id})).unwrap()["undo"].clone();
        let timing = || {
            let s = crate::studio_sequence::find(&core, uuid::Uuid::parse_str(id.as_str().unwrap()).unwrap()).unwrap();
            let s = s.lock();
            (s.document().selected_timing_provider.source_name.clone(), s.audio().map(|c| c.duration().total_ms()))
        };

        // A non-default length stays unless fitLength asks.
        let first = run("sequence_audio", json!({"sequenceId": id, "audio": songs.join("first.wav")})).unwrap();
        assert_eq!((first["song"].as_str(), first["copiedAudio"].as_bool(), first["lengthMs"].as_f64()), (Some("first.wav"), Some(true), Some(10000.0)));
        assert!(base.join("profile/Media/first.wav").is_file(), "copied into Media");
        assert_eq!(undo_list()[0], "Add Audio");
        assert_eq!(timing(), (Some("first.wav".into()), Some(2000.0)));

        let second = run("sequence_audio", json!({"sequenceId": id, "audio": songs.join("second.wav"), "fitLength": true})).unwrap();
        assert_eq!((second["song"].as_str(), second["lengthMs"].as_f64()), (Some("second.wav"), Some(3000.0)));
        assert_eq!(undo_list()[0], "Change Audio");
        assert_eq!(timing(), (Some("second.wav".into()), Some(3000.0)));

        // Undo puts the first song back whole: media, timing source, decoded clip and the length it changed.
        run("sequence_undo", json!({"sequenceId": id})).unwrap();
        assert_eq!(song()["song"], "first.wav");
        assert_eq!(timing(), (Some("first.wav".into()), Some(2000.0)));
        assert_eq!(length(), 10000.0);
        run("sequence_redo", json!({"sequenceId": id})).unwrap();
        assert_eq!((song()["song"].as_str(), length()), (Some("second.wav"), 3000.0));

        let removed = run("sequence_audio", json!({"sequenceId": id, "remove": true})).unwrap();
        assert_eq!(removed["changed"], true);
        assert!(song().is_null());
        assert_eq!(timing().1, None);
        assert_eq!(undo_list()[0], "Remove Audio");
        assert_eq!(run("sequence_audio", json!({"sequenceId": id, "remove": true})).unwrap()["changed"], false, "nothing left to remove");
        run("sequence_undo", json!({"sequenceId": id})).unwrap();
        assert_eq!(timing(), (Some("second.wav".into()), Some(3000.0)));
        assert_eq!(length(), 3000.0, "removing never changed the length");

        // A Media filename works; a different file with a taken name and a non-audio file are refused, changing nothing.
        assert_eq!(run("sequence_audio", json!({"sequenceId": id, "audio": "first.wav"})).unwrap()["copiedAudio"], false);
        let other = base.join("other");
        std::fs::create_dir_all(&other).unwrap();
        wav(&other.join("first.wav"), 1.0);
        assert!(run("sequence_audio", json!({"sequenceId": id, "audio": other.join("first.wav")})).is_err());
        std::fs::write(other.join("notes.txt"), "x").unwrap();
        assert!(run("sequence_audio", json!({"sequenceId": id, "audio": other.join("notes.txt")})).is_err());
        assert!(run("sequence_audio", json!({"sequenceId": id, "remove": true, "audio": "first.wav"})).is_err());
        assert_eq!(song()["song"], "first.wav");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn the_sequencer_add_and_remove_audio_are_undoable() {
        let base = std::env::temp_dir().join(format!("studio-audio-ui-{}", uuid::Uuid::new_v4()));
        let core = std::sync::Arc::new(AppCore::open(base.join("profile"), true).unwrap());
        wav(&base.join("song.wav"), 2.0);
        let session = std::sync::Arc::new(parking_lot::Mutex::new(crate::sequence::SequenceSession::new_untitled(&core.session_environment())));
        let settings = std::sync::Arc::new(vixen_io::ProfileSettings::open(base.join("Settings.xml")));
        let mut state = crate::sequencer::SequencerState::with_settings(std::sync::Arc::clone(&core), std::sync::Arc::clone(&session), settings, base.clone());
        state.finish_load();
        let speed = |state: &crate::sequencer::SequencerState| state.transport.speed_controls().gauge_enabled;
        let default_length = session.lock().length();
        assert!(matches!(state.add_audio(&base.join("song.wav"), false), crate::sequencer::AddAudioOutcome::Added { .. }));
        assert_eq!(session.lock().length().total_ms(), 2000.0, "the default length takes the song's");
        assert!(speed(&state), "speed controls on with a song");
        state.remove_audio();
        assert!(session.lock().audio().is_none());
        assert_eq!(state.undo(1), 1);
        assert!(session.lock().audio().is_some(), "Undo Remove Audio brings the clip back");
        assert_eq!(state.undo(1), 1);
        let s = session.lock();
        assert!(s.audio().is_none() && s.document().media.is_empty(), "Undo Add Audio removes it");
        assert_eq!(s.length(), default_length, "and the length it set");
        drop(s);
        state.poll(std::time::Instant::now());
        assert!(!speed(&state), "speed controls follow the undo");
        let _ = std::fs::remove_dir_all(&base);
    }
}
