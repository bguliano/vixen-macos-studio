//! Shared Studio sequence authoring, analysis and export commands. No transport or hardware startup.
use crate::{
    app::AppCore,
    sequence::{EffectId, SequenceSession},
    undo::{AddRemoveKind, MoveKind, UndoAction},
};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::{
    path::{Component, Path, PathBuf},
    sync::{atomic::AtomicBool, Arc},
};
use uuid::Uuid;
use vixen_core::{marks::Mark, MarkCollection, TimeSpan};
use vixen_effects::framework::{EffectDescriptor, ParamDesc, ParamValue};

#[path = "studio_onsets.rs"]
mod onsets;
#[path = "studio_mark_effects.rs"]
mod mark_effects;
#[path = "studio_step_effects.rs"]
mod step_effects;

fn string<'a>(a: &'a Value, k: &str) -> Result<&'a str, String> {
    a[k].as_str()
        .filter(|s| !s.is_empty() && s.len() <= 256)
        .ok_or(format!("{k} must be a nonempty string (max 256 bytes)"))
}
fn uuid(a: &Value, k: &str) -> Result<Uuid, String> {
    Uuid::parse_str(string(a, k)?).map_err(|_| format!("{k} must be a UUID"))
}
fn ms(a: &Value, k: &str) -> Result<f64, String> {
    a[k].as_f64()
        .filter(|n| n.is_finite() && *n >= 0.0 && *n <= 3_600_000.0)
        .ok_or(format!("{k} must be milliseconds in 0..3600000"))
}
fn confirm(a: &Value) -> Result<(), String> {
    if a["confirm"] == true {
        Ok(())
    } else {
        Err("Explicit confirm:true required".into())
    }
}
fn filename(a: &Value, k: &str) -> Result<String, String> {
    let s = string(a, k)?;
    if s.contains(['/', '\\']) || s == "." || s == ".." {
        Err("Use a filename, not a path".into())
    } else {
        Ok(s.into())
    }
}
fn output(core: &AppCore, folder: &str, name: &str) -> Result<PathBuf, String> {
    let root = core
        .paths
        .root()
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let candidate = root.join(folder);
    if !candidate.exists() {
        std::fs::create_dir_all(&candidate).map_err(|e| e.to_string())?;
    }
    let directory = candidate.canonicalize().map_err(|e| e.to_string())?;
    if !directory.starts_with(&root) {
        return Err("Output directory escapes profile".into());
    }
    crate::studio_paths::reject_links(&directory, 0)?;
    let out = directory.join(name);
    if let Ok(meta) = std::fs::symlink_metadata(&out) {
        if meta.file_type().is_symlink() || !meta.is_file() {
            return Err("Output must be an ordinary file, not a symlink".into());
        }
    }
    Ok(out)
}
/// Scratch sequences live here: under Sequence, so writes stay in the profile, but hidden from the sequence list, which
/// only reads Sequence itself.
const SCRATCH_FOLDER: &str = "Sequence/.scratch";
/// The file sequence_open reads: a profile Sequence filename (Sequence/.scratch with scratch:true), or an absolute path to
/// an ordinary .tim file anywhere, opened in place. Saving still writes only into the profile.
fn sequence_source(core: &AppCore, a: &Value) -> Result<PathBuf, String> {
    let file = a["file"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 1024)
        .ok_or("file must be a nonempty string (max 1024 bytes)")?;
    if !file.to_lowercase().ends_with(".tim") {
        return Err("Sequence must be a .tim file".into());
    }
    if !file.starts_with('/') {
        let name = filename(a, "file")?;
        return output(core, if a["scratch"] == true { SCRATCH_FOLDER } else { "Sequence" }, &name);
    }
    if a["scratch"] == true {
        return Err("scratch takes a filename, not a path".into());
    }
    let path = Path::new(file);
    if path.components().any(|c| matches!(c, Component::ParentDir | Component::CurDir)) {
        return Err("Use an absolute path without . or .. components".into());
    }
    let meta = std::fs::symlink_metadata(path).map_err(|e| format!("{file}: {e}"))?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err("The sequence must be an ordinary file, not a symlink or folder".into());
    }
    let path = path.canonicalize().map_err(|e| e.to_string())?;
    if path.components().any(|c| c.as_os_str() == "vixen-show-original") {
        return Err("Use a show copy, not the original archive".into());
    }
    Ok(path)
}
/// Frees a document this Studio session opened so its file can be read again. Refuses one an app window holds, and
/// unsaved changes without confirm:true.
fn release_for_reload(core: &AppCore, shared: Arc<Mutex<SequenceSession>>, a: &Value) -> Result<(), String> {
    let session_id = shared.lock().playback_id();
    let mut owned = core.studio_display.owned_sequences.lock();
    let held = owned.get(&session_id).is_some_and(|o| Arc::ptr_eq(o, &shared));
    if Arc::strong_count(&shared) - 1 - usize::from(held) > 0 {
        return Err("The app has this sequence open in a window; close it there (or revert it in the app) before reloading".into());
    }
    if shared.lock().is_modified() && a["confirm"] != true {
        return Err("Unsaved changes would be lost: sequence_save first, or pass confirm:true to discard them".into());
    }
    core.playback.stop_session(session_id);
    if held {
        owned.remove(&session_id);
    }
    Ok(())
}
fn canonical(path: &std::path::Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}
/// The agent-facing ID of a document: derived from its canonical file path once it has one, so every connection to this
/// core, and a reopen after the app closed and reopened the file, get the same ID. An unsaved document uses its session ID.
pub fn document_id(s: &SequenceSession) -> Uuid {
    let Some(path) = s.path() else {
        return s.playback_id();
    };
    // FNV-1a over the canonical path: stable across processes and builds, unlike std's hasher.
    let mut hash: u128 = 0x6c62272e07bb014262b821756295c58d;
    for byte in b"vixen-studio-sequence:"
        .iter()
        .chain(canonical(path).to_string_lossy().as_bytes())
    {
        hash = (hash ^ u128::from(*byte)).wrapping_mul(0x0000000001000000000000000000013b);
    }
    uuid::Builder::from_custom_bytes(hash.to_be_bytes()).into_uuid()
}
/// Accepts the document ID or the session ID. Several sessions on one file resolve to the oldest, the one the app reuses.
pub fn find(core: &AppCore, id: Uuid) -> Result<Arc<Mutex<SequenceSession>>, String> {
    core.open_sequence_sessions()
        .into_iter()
        .find(|s| {
            let s = s.lock();
            s.playback_id() == id || document_id(&s) == id
        })
        .ok_or("Sequence is not open in this Studio session; sequence_open its file again (an unsaved document closed in the app, or made by another headless connection, is gone)".into())
}
fn document_json(s: &SequenceSession, already_open: bool) -> Value {
    json!({"sequenceId":document_id(s).to_string(),"sessionId":s.playback_id().to_string(),"alreadyOpen":already_open,"name":s.name(),"file":s.path().and_then(|p|p.file_name()).map(|n|n.to_string_lossy().into_owned()),"path":s.path(),"modified":s.is_modified()})
}
fn record(s: &mut SequenceSession, label: &str, action: UndoAction, agent: bool) {
    s.record_undo(UndoAction::Group {
        label: format!("{label}{}", if agent { " (Agent)" } else { "" }),
        actions: vec![action],
    });
}
fn collections_json(collections: &[MarkCollection]) -> Value {
    json!(collections.iter().map(|c|json!({"id":c.id.to_string(),"name":c.name,"marks":c.marks.iter().map(|m|json!({"timeMs":m.start_time.total_ms(),"durationMs":m.duration.total_ms(),"text":m.text})).collect::<Vec<_>>()})).collect::<Vec<_>>())
}
/// Loads a profile Media file for analysis, within the input-size and length limits.
fn analysis_audio(core: &AppCore, name: &str) -> Result<vixen_audio::AudioClip, String> {
    let path = core.paths.media_dir().join(name);
    if std::fs::metadata(&path).map_err(|e| e.to_string())?.len() > 100 * 1024 * 1024 {
        return Err("Audio analysis input limit is 100 MiB".into());
    }
    let audio = vixen_audio::AudioClip::load(&path).map_err(|e| e.to_string())?;
    if audio.duration().total_ms() > 900_000.0 {
        return Err("Analysis is limited to 15 minutes".into());
    }
    Ok(audio)
}
fn analyze(core: &AppCore, a: &Value) -> Result<(Value, Vec<MarkCollection>), String> {
    let name = filename(a, "media")?;
    let audio = analysis_audio(core, &name)?;
    let duration = audio.duration().total_ms();
    let tracker = vixen_analysis::BeatsAndBars::from_samples(
        audio.mono().to_vec(),
        audio.sample_rate() as f32,
        1,
    );
    let mut settings = vixen_analysis::BeatBarSettingsData::new("Music");
    settings.beats_per_bar = 4;
    settings.note_size = 4;
    settings.all_features_enabled = true;
    settings.bars_enabled = true;
    let mut collections = Vec::new();
    tracker
        .do_beat_bar_detection(
            &mut collections,
            &settings,
            &mut |_| {},
            &AtomicBool::new(false),
        )
        .map_err(|e| format!("{e:?}"))?;
    // Vixen's "All" collection repeats the first beat; drop that mark from the collection and the beat list.
    for c in collections.iter_mut().filter(|c| c.name == "Music - All") {
        c.marks.dedup_by(|b, a| a.start_time == b.start_time);
    }
    let bars = onsets::dedupe(
        collections
            .iter()
            .filter(|c| c.name.contains("(Bar)"))
            .flat_map(|c| c.marks.iter().map(|m| m.start_time.total_ms()))
            .collect(),
        1.0,
    );
    // Downbeats are beats too; they restore the last beat when it starts a bar ("All" never includes the last beat).
    let beats = onsets::dedupe(
        collections
            .iter()
            .filter(|c| c.name == "Music - All")
            .flat_map(|c| c.marks.iter().map(|m| m.start_time.total_ms()))
            .chain(bars.iter().copied())
            .collect(),
        1.0,
    );
    let mut intervals: Vec<_> = beats
        .windows(2)
        .map(|p| p[1] - p[0])
        .filter(|n| *n > 0.0)
        .collect();
    intervals.sort_by(f64::total_cmp);
    let tempo = intervals.get(intervals.len() / 2).map(|d| 60_000.0 / d);
    let chunk = (audio.sample_rate() as usize / 10).max(1);
    let levels: Vec<f64> = audio
        .mono()
        .chunks(chunk)
        .map(|samples| {
            (samples.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>()
                / samples.len().max(1) as f64)
                .sqrt()
        })
        .collect();
    let average = levels.iter().sum::<f64>() / levels.len().max(1) as f64;
    let detected = onsets::detect(audio.mono(), audio.sample_rate());
    let round = |v: f64, places: i32| (v * 10f64.powi(places)).round() / 10f64.powi(places);
    let onset_list: Vec<Value> = detected
        .iter()
        .map(|o| {
            let mut item = json!({"timeMs":round(o.time_ms, 1),"band":onsets::BANDS[o.band],"strength":round(o.strength, 3)});
            if let Some((bar, slot, offset)) = onsets::slot(&bars, o.time_ms) {
                item["bar"] = json!(bar);
                item["slot"] = json!(slot);
                item["slotOffsetMs"] = json!(round(offset, 1));
            }
            item
        })
        .collect();
    // Any band, with hits closer than 25 ms merged: one time per audible attack.
    let onsets_ms: Vec<f64> =
        onsets::dedupe(detected.iter().map(|o| round(o.time_ms, 0)).collect(), 25.0);
    let slot_grid: Vec<Value> = onsets::slot_grid(&bars, &detected)
        .iter()
        .zip(&bars)
        .map(|(bands, bar)| {
            let row = |b: usize| bands[b].iter().map(|v| round(*v, 2)).collect::<Vec<_>>();
            json!({"barMs":bar,"low":row(0),"mid":row(1),"high":row(2)})
        })
        .collect();
    let energy: Vec<Value> = levels
        .iter()
        .enumerate()
        .map(|(i, rms)| json!({"timeMs":i as f64*100.0,"rms":rms}))
        .collect();
    let section_candidates: Vec<f64> = levels
        .chunks(50)
        .map(|c| c.iter().sum::<f64>() / c.len() as f64)
        .collect::<Vec<_>>()
        .windows(2)
        .enumerate()
        .filter(|(_, w)| (w[1] - w[0]).abs() > average * 0.5)
        .map(|(i, _)| (i + 1) as f64 * 5000.0)
        .collect();
    Ok((
        json!({"media":name,"durationMs":duration,"tempoBpm":tempo,"beatsMs":beats,"barsMs":bars,"assumedBeatsPerBar":4,"energy":energy,"onsetsMs":onsets_ms,"onsets":onset_list,"slotsPerBar":onsets::SLOTS_PER_BAR,"slotGrid":slot_grid,"sectionCandidatesMs":section_candidates,"onsetMethod":"Per-band spectral flux (low <200 Hz, mid 200-2000 Hz, high >2000 Hz), ~46 ms window, ~6 ms hop, adaptive median+MAD threshold; times are estimates, typically within 10 ms","slotMethod":"bar = index into barsMs, slot = nearest 16th of that bar (assumes 4/4); slotGrid holds each band's strongest onset strength per slot","sectionMethod":"5s RMS change candidates, not semantic song sections","notAnalyzed":["lyrics","phonemes"]}),
        collections,
    ))
}

fn execute_inner(core: &AppCore, name: &str, a: &Value, agent: bool) -> Result<Value, String> {
    let tool = tools()
        .into_iter()
        .find(|t| t["name"] == name)
        .ok_or("Unknown sequence command")?;
    crate::studio_schema::arguments(&tool["inputSchema"], a)?;
    let obj = a.as_object().ok_or("arguments must be an object")?;
    let props = tool["inputSchema"]["properties"].as_object().unwrap();
    if obj.keys().any(|k| !props.contains_key(k)) {
        return Err("Unknown command argument".into());
    }
    if name == "music_analyze" {
        return Ok(analyze(core, a)?.0);
    }
    if name == "sequence_open" || name == "sequence_create" {
        let mut reloaded = false;
        let mut session = if name == "sequence_open" {
            let path = sequence_source(core, a)?;
            // Compare canonical paths, as the app's own open does, so a file the app opened by another spelling is reused.
            let wanted = canonical(&path);
            if let Some(existing) = core
                .open_sequence_sessions()
                .into_iter()
                .find(|s| s.lock().path().is_some_and(|p| canonical(p) == wanted))
            {
                if a["reload"] != true {
                    return Ok(document_json(&existing.lock(), true));
                }
                release_for_reload(core, existing, a)?;
                reloaded = true;
            }
            SequenceSession::open(&path, &core.session_environment()).map_err(|e| e.to_string())?
        } else {
            let length = ms(a, "lengthMs")?;
            if length < 25.0 {
                return Err("lengthMs must be at least 25".into());
            }
            let mut s = SequenceSession::new_untitled(&core.session_environment());
            s.set_length(TimeSpan::from_ms(length));
            if a.get("media").is_some() {
                let media = filename(a, "media")?;
                let clip = Arc::new(
                    vixen_audio::AudioClip::load(&core.paths.media_dir().join(&media))
                        .map_err(|e| e.to_string())?,
                );
                s.document_mut().media.push(vixen_io::sequence::MediaEntry {
                    type_id: crate::export::AUDIO_MEDIA_TYPE,
                    instance_id: Uuid::new_v4(),
                    file_name: media,
                    data: None,
                });
                s.set_audio_clip(Some(clip));
            }
            s
        };
        let id = session.playback_id();
        let mut result = document_json(&session, false);
        if reloaded {
            result["reloaded"] = json!(true);
        }
        session.render_all_dirty();
        let shared = Arc::new(Mutex::new(session));
        core.register_sequence_session(&shared);
        core.studio_display
            .owned_sequences
            .lock()
            .insert(id, shared);

        return Ok(result);
    }
    let shared = find(core, uuid(a, "sequenceId")?)?;
    let session_id = shared.lock().playback_id();
    if agent
        && tool["annotations"]["readOnlyHint"] == false
        && !matches!(
            name,
            "sequence_pause" | "sequence_stop" | "sequence_play" | "sequence_seek"
        )
        && core.playback.hardware_allowed()
        && core.playback.session_transport(session_id) != crate::playback::Transport::Stopped
    {
        return Err(
            "Stop this sequence locally before agent edits that may alter live output".into(),
        );
    }
    if name == "sequence_close" {
        return close(core, shared, a);
    }
    if name == "preview_video" || name == "preview_sheet" {
        return preview_range(core, &shared, name, a);
    }
    if name == "sequence_onset_marks" {
        return mark_effects::onset_marks(core, &shared, a, agent);
    }
    if name == "sequence_music_marks" {
        let (analysis, marks) = analyze(core, a)?;
        let mut s = shared.lock();
        let mut old = s.marks().to_vec();
        let mut combined = old.clone();
        // Undo restores marks into collections it knows and never removes a collection, so it gets the new ones empty.
        old.extend(marks.iter().map(|c| MarkCollection { marks: Vec::new(), ..c.clone() }));
        combined.extend(marks);
        s.set_marks(combined);
        record(
            &mut s,
            "Music marks",
            UndoAction::MarksChanged {
                label: "Music marks".into(),
                marks: old,
            },
            agent,
        );
        drop(s);

        return Ok(analysis);
    }
    let mut s = shared.lock();
    let result = match name {
        "sequence_status" => {
            s.apply_finished_renders();
            let mut status = document_json(&s, true);
            let fields = status.as_object_mut().unwrap();
            fields.remove("alreadyOpen");
            fields.insert("lengthMs".into(), json!(s.length().total_ms()));
            fields.insert("effects".into(), json!(s.effect_count()));
            fields.insert("rendered".into(), json!(s.effects().filter(|e| e.is_rendered()).count()));
            fields.insert("undo".into(), json!(s.undo_stack().undo_descriptions()));
            fields.insert("redo".into(), json!(s.undo_stack().redo_descriptions()));
            status
        }
        "sequence_effects" => {
            json!({"effects":s.effects().take(10000).map(|e|json!({"id":e.id.0,"typeId":e.node.type_id.to_string(),"startMs":e.node.start.total_ms(),"durationMs":e.node.time_span.total_ms(),"targets":e.node.targets.iter().map(|(id,_)|id.to_string()).collect::<Vec<_>>()})).collect::<Vec<_>>(),"total":s.effect_count()})
        }
        "effect_add" | "effect_band" => {
            let planned = if name == "effect_band" { plan_band(core, &s, a)? } else { plan_add(core, &s, a)? };
            let ids: Vec<EffectId> = planned.into_iter().map(|new| apply_add(&mut s, new)).collect();
            let first = ids[0].0;
            let all: Vec<u64> = ids.iter().map(|id| id.0).collect();
            record(
                &mut s,
                if ids.len() == 1 { "Add effect" } else { "Add effects" },
                UndoAction::EffectsAdded {
                    kind: AddRemoveKind::AddedOrRemoved,
                    ids,
                },
                agent,
            );
            json!({"effectId":first,"effectIds":all})
        }
        "effects_batch" => return effects_batch(core, &mut s, a, agent),
        "effects_on_marks" => return mark_effects::effects_on_marks(core, &mut s, a, agent),
        "effect_step" => return step_effects::effect_step(core, &mut s, a, agent),
        "sequence_clear" => {
            confirm(a)?;
            let ids: Vec<EffectId> = s.effects().map(|e| e.id).collect();
            if !ids.is_empty() {
                let effects = s.remove_effects(&ids);
                record(
                    &mut s,
                    "Clear effects",
                    UndoAction::EffectsRemoved {
                        kind: AddRemoveKind::AddedOrRemoved,
                        effects,
                    },
                    agent,
                );
            }
            json!({"removed":ids.len()})
        }
        "effect_delete" => {
            confirm(a)?;
            let id = effect_id(&s, a)?;
            let effects = s.remove_effects(&[id]);
            record(
                &mut s,
                "Delete effect",
                UndoAction::EffectsRemoved {
                    kind: AddRemoveKind::AddedOrRemoved,
                    effects,
                },
                agent,
            );
            json!({})
        }
        "effect_timing" => {
            let id = effect_id(&s, a)?;
            let start = ms(a, "startMs")?;
            let span = ms(a, "durationMs")?;
            if span < 1.0 || start + span > s.length().total_ms() {
                return Err("Effect must fit within sequence".into());
            }
            let old = s.effect(id).unwrap();
            let times = vec![(id, old.node.start, old.node.time_span)];
            s.set_times(&[(id, TimeSpan::from_ms(start), TimeSpan::from_ms(span))]);
            record(
                &mut s,
                "Move/resize effect",
                UndoAction::TimesChanged {
                    kind: MoveKind::Moved,
                    times,
                },
                agent,
            );
            json!({})
        }
        "effect_parameters"=>{let effect=effect_id(&s,a)?;let entry=s.effect(effect).unwrap();let desc=entry.descriptor.ok_or("Unknown effect type")?;json!({"parameters":desc.params.iter().map(|p|{let mut o=json!({"name":p.name,"value":crate::studio_params::encode(entry.node.params.get(p.name)),"schema":crate::studio_params::param_schema(p)});if let Some(n)=crate::studio_params::note(desc.name,p.name){o["note"]=json!(n);}o}).collect::<Vec<_>>()})},
        "effect_set_parameter" => {
            let effect = effect_id(&s, a)?;
            let entry = s.effect(effect).unwrap();
            let descriptor = entry.descriptor.ok_or("Unknown effect descriptor")?;
            let (parameter, value) = parameter_value(descriptor, string(a, "parameter")?, &a["value"])?;
            let old = entry.node.params.clone();
            s.set_param(effect, parameter, value);
            record(
                &mut s,
                "Set effect parameter",
                UndoAction::ParamsChanged {
                    label: parameter.into(),
                    params: vec![(effect, old)],
                },
                agent,
            );
            json!({})
        }
        "preview_frame" => {
            confirm(a)?;
            let file = filename(a, "file")?;
            if !file.to_lowercase().ends_with(".png") {
                return Err("Use .png extension".into());
            }
            let path = output(core, "Export", &file)?;
            let time = ms(a, "timeMs")?;
            if time > s.length().total_ms() {
                return Err("Time outside sequence".into());
            }
            return crate::studio_preview::frame(core, &mut s, uuid(a, "previewId")?, time, &path);
        }
        "sequence_length" => {
            let length=ms(a,"lengthMs")?;
            if length<25.0||s.effects().any(|e|e.end().total_ms()>length){return Err("Length must contain all effects and be at least 25ms".into())}
            let old=UndoAction::StudioSettings{label:"Sequence length".into(),length:s.length(),layers:s.document().layers.clone()};
            s.set_length(TimeSpan::from_ms(length));record(&mut s,"Sequence length",old,agent);json!({})
        }
        "sequence_layers" => json!(s.document().layers.layers.iter().map(|l|json!({"id":l.id.to_string(),"name":l.name,"filter":l.filter.as_ref().map(crate::sequencer::layers::filter_type_name).unwrap_or_else(||"Highest Value (Default)".into())})).collect::<Vec<_>>()),
        "layer_create" | "layer_rename" | "layer_delete" => {
            let old=UndoAction::StudioSettings{label:"Edit layers".into(),length:s.length(),layers:s.document().layers.clone()};
            let layer=if name=="layer_create" {
                let name=string(a,"name")?;let id=crate::sequencer::layers::add_layer(&mut s.document_mut().layers,vixen_core::layer::MixingFilter::HighestValue);
                crate::sequencer::layers::rename_layer(&mut s.document_mut().layers,id,name);id
            }else{let id=uuid(a,"layerId")?;if name=="layer_delete" {confirm(a)?;if !crate::sequencer::layers::remove_layer(&mut s.document_mut().layers,id){return Err("Layer cannot be removed".into())}}
            else if !crate::sequencer::layers::rename_layer(&mut s.document_mut().layers,id,string(a,"name")?){return Err("Layer cannot be renamed".into())}id};
            s.layers_changed();record(&mut s,"Edit layers",old,agent);json!({"layerId":layer.to_string()})
        }
        "effect_layer" => {
            let effect=effect_id(&s,a)?;let layer=uuid(a,"layerId")?;
            if !s.document().layers.layers.iter().any(|l|l.id==layer){return Err("Layer not found".into())}
            let old=s.layer_of(effect).ok_or("Effect layer not found")?;
            s.set_layer(&[effect],layer);record(&mut s,"Assign layer",UndoAction::LayersChanged{layers:vec![(effect,old)]},agent);json!({})
        }
        "effect_copy" => {
            let effect=effect_id(&s,a)?;let target=uuid(a,"targetId")?;let start=ms(a,"startMs")?;
            let node=s.effect(effect).unwrap().node.clone();let span=node.time_span;
            if start+span.total_ms()>s.length().total_ms(){return Err("Copied effect exceeds sequence length".into())}
            let target_name={let system=core.system.read();system.tree.find(&target).map(|n|system.tree.node(n).name.clone()).ok_or("Target not found")?};
            let id=s.add_copy(node,target,&target_name,TimeSpan::from_ms(start),span).ok_or("Effect could not be copied")?;
            record(&mut s,"Copy effect",UndoAction::EffectsAdded{kind:AddRemoveKind::Pasted,ids:vec![id]},agent);json!({"effectId":id.0})
        }
        "sequence_play"=>{
            if core.playback.hardware_allowed() && std::env::var("VIXEN_MUTE").is_ok_and(|v|v=="1"){return Err("Playback is disabled by VIXEN_MUTE".into())}
            if core.playback.hardware_allowed() {confirm(a)?;if !crate::studio_live::allowed(core){return Err("App is not muted: local agent-output permission and confirmation required for playback".into())}}
            let start=ms(a,"timeMs")?;let end=s.length();core.playback.play(&mut s,TimeSpan::from_ms(start),end,false);json!({"playing":true,"muted":!core.playback.hardware_allowed()})
        }
        "sequence_pause"=>{core.playback.pause_session(s.playback_id());json!({})}
        "sequence_stop"=>{core.playback.stop_session(s.playback_id());json!({})}
        "sequence_seek"=>{if core.playback.hardware_allowed(){confirm(a)?;if !crate::studio_live::allowed(core)||std::env::var("VIXEN_MUTE").is_ok_and(|v|v=="1"){return Err("Live seeking requires local output permission and no mute override".into())}}core.playback.seek_session(s.playback_id(),TimeSpan::from_ms(ms(a,"timeMs")?));json!({})}
        "sequence_marks" => collections_json(s.marks()),
        "marks_create" => {
            let times = a["timesMs"]
                .as_array()
                .filter(|v| v.len() <= 10000)
                .ok_or("timesMs must be an array of at most 10000 times")?;
            let mut marks = Vec::new();
            for t in times {
                let value = t
                    .as_f64()
                    .filter(|n| n.is_finite() && *n >= 0.0 && *n < s.length().total_ms())
                    .ok_or("Mark time is outside sequence")?;
                marks.push(Mark::new(TimeSpan::from_ms(value)));
            }
            if let Some(texts)=a.get("texts"){let texts=texts.as_array().ok_or("texts must be an array")?;if texts.len()!=marks.len(){return Err("texts must match timesMs length".into())}for (mark,text) in marks.iter_mut().zip(texts){mark.text=text.as_str().filter(|s|s.len()<=256).ok_or("Mark text must be <=256 bytes")?.into();}}
            let mut old = s.marks().to_vec();
            let mut collections = old.clone();
            let mut collection = MarkCollection::default();
            collection.collection_type=match a["collectionType"].as_str().unwrap_or("generic"){"generic"=>vixen_core::marks::MarkCollectionType::Generic,"phrase"=>vixen_core::marks::MarkCollectionType::Phrase,"word"=>vixen_core::marks::MarkCollectionType::Word,"phoneme"=>vixen_core::marks::MarkCollectionType::Phoneme,"state"=>vixen_core::marks::MarkCollectionType::State,_=>return Err("Invalid collectionType".into())};
            collection.name = string(a, "name")?.into();
            collection.marks = marks;
            collection.sort_marks();
            let id = collection.id;
            // Undo restores marks into collections it knows and never removes a collection, so it gets the new one empty.
            old.push(MarkCollection { marks: Vec::new(), ..collection.clone() });
            collections.push(collection);
            s.set_marks(collections);
            record(
                &mut s,
                "Add marks",
                UndoAction::MarksChanged {
                    label: "Add marks".into(),
                    marks: old,
                },
                agent,
            );
            json!({"collectionId":id.to_string()})
        }
        "sequence_undo" => json!({"steps":s.undo(1)}),
        "sequence_redo" => json!({"steps":s.redo(1)}),
        "sequence_render" => {
            s.render_all_dirty();
            s.wait_for_renders();
            json!({"effects":s.effect_count(),"rendered":s.effects().filter(|e|e.is_rendered()).count(),"scheduleEntries":s.schedule().entry_count()})
        }
        "sequence_save" => {
            confirm(a)?;
            let file = filename(a, "file")?;
            if !file.to_lowercase().ends_with(".tim") {
                return Err("Use .tim extension".into());
            }
            let folder = if a["scratch"] == true { SCRATCH_FOLDER } else { "Sequence" };
            let path = output(core, folder, &file)?;
            if a["backup"] == false {
                // The same atomic write, without moving the old file to auto_backup or keeping an old-version copy.
                let document = s.begin_save();
                let written = vixen_io::sequence::sequence_to_document(&document)
                    .and_then(|text| vixen_io::file::write_atomic(&path, text.as_bytes()))
                    .map_err(|e| e.to_string());
                s.finish_save(Some(path.clone()), written.is_ok());
                written?;
            } else {
                s.save(Some(&path)).map_err(|e| e.to_string())?;
            }
            // The document now has a file, so its ID is the file's; the previous ID keeps working while it stays open.
            json!({"path":path,"sequenceId":document_id(&s).to_string()})
        }
        "sequence_export_fseq" => {
            confirm(a)?;
            let file = filename(a, "file")?;
            if !file.to_lowercase().ends_with(".fseq") {
                return Err("Use .fseq extension".into());
            }
            let path = output(core, "Export", &file)?;
            let system = core.system.read().clone();
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
            let export = crate::export::PreparedExport::prepare(&mut s, &system, &request)
                .map_err(|e| e.to_string())?;
            drop(s);
            export
                .run(&AtomicBool::new(false), &mut |_, _| {})
                .map_err(|e| e.to_string())?;
            return Ok(json!({"path":path}));
        }
        _ => return Err("Unknown sequence command".into()),
    };
    let read_only = tool["annotations"]["readOnlyHint"] == true;
    if !read_only {
        s.render_all_dirty();
    }
    drop(s);

    Ok(result)
}
fn effect_id(s: &SequenceSession, a: &Value) -> Result<EffectId, String> {
    let id = EffectId(
        a["effectId"]
            .as_u64()
            .ok_or("effectId must be an integer")?,
    );
    s.effect(id).ok_or("Effect not found")?;
    Ok(id)
}

/// Decodes one parameter through the native descriptor and its editor's numeric bounds. Nothing is applied.
fn parameter_value(
    descriptor: &'static EffectDescriptor,
    parameter: &str,
    value: &Value,
) -> Result<(&'static str, ParamValue), String> {
    let param: &ParamDesc = descriptor
        .params
        .iter()
        .find(|p| p.name == parameter)
        .ok_or_else(|| format!("Unknown parameter {parameter}"))?;
    if matches!(param.editor, vixen_effects::framework::Editor::Path(_)) {
        return Err("File-path parameters require a future profile-media selector".into());
    }
    let decoded = crate::studio_params::decode(param.kind, value)
        .map_err(|e| format!("{parameter}: {e}"))?;
    if let (Some(number), Some((min, max))) = (value.as_f64(), crate::studio_params::range(param)) {
        if number < min || number > max {
            return Err(format!("{parameter} must be within {min}..{max}"));
        }
    }
    Ok((param.name, decoded))
}
/// Decodes a `params` object of parameter name to value, as effect_parameters reports them.
fn parameter_values(
    descriptor: &'static EffectDescriptor,
    params: &Value,
) -> Result<Vec<(&'static str, ParamValue)>, String> {
    let object = params
        .as_object()
        .filter(|o| o.len() <= 256)
        .ok_or("params must be an object of parameter name to value (max 256)")?;
    object
        .iter()
        .map(|(name, value)| parameter_value(descriptor, name, value))
        .collect()
}
const TARGET_LIMIT: usize = 4096;
/// The targets of an add, with their names: targetId, targetIds, or targetSlice (a group's children picked by position).
fn targets(core: &AppCore, a: &Value) -> Result<Vec<(Uuid, String)>, String> {
    if ["targetId", "targetIds", "targetSlice"].iter().filter(|k| a.get(**k).is_some()).count() != 1 {
        return Err("Give exactly one of targetId, targetIds or targetSlice".into());
    }
    let system = core.system.read();
    let tree = &system.tree;
    let named = |id: Uuid| tree.find(&id).map(|n| (id, tree.node(n).name.clone())).ok_or(format!("Target {id} not found"));
    if a.get("targetId").is_some() {
        return Ok(vec![named(uuid(a, "targetId")?)?]);
    }
    if let Some(list) = a.get("targetIds") {
        let list = list
            .as_array()
            .filter(|l| (1..=TARGET_LIMIT).contains(&l.len()))
            .ok_or(format!("targetIds must list 1..{TARGET_LIMIT} target UUIDs"))?;
        let mut seen = std::collections::HashSet::new();
        return list
            .iter()
            .map(|v| {
                let id = v.as_str().and_then(|s| Uuid::parse_str(s).ok()).ok_or("targetIds must be UUIDs")?;
                if !seen.insert(id) {
                    return Err(format!("Target {id} is listed twice"));
                }
                named(id)
            })
            .collect();
    }
    let slice = &a["targetSlice"];
    let fields = slice.as_object().ok_or("targetSlice must be an object: {groupId, strings?:[from,to], every?, offset?}")?;
    if let Some(extra) = fields.keys().find(|k| !["groupId", "strings", "every", "offset"].contains(&k.as_str())) {
        return Err(format!("{extra} does not apply to targetSlice"));
    }
    let group = tree.find(&uuid(slice, "groupId")?).ok_or("targetSlice group not found")?;
    let children = &tree.node(group).children;
    if children.is_empty() {
        return Err("targetSlice group has no children to pick from".into());
    }
    let count = children.len() as u64;
    let (from, to) = match slice.get("strings") {
        None => (1, count),
        Some(range) => match range.as_array().map(|r| r.iter().map(Value::as_u64).collect::<Vec<_>>()).as_deref() {
            Some([Some(from), Some(to)]) if 1 <= *from && from <= to && *to <= count => (*from, *to),
            _ => return Err(format!("targetSlice strings must be [from, to], counting the group's children from 1 (it has {count})")),
        },
    };
    let every = slice.get("every").map_or(Some(1), Value::as_u64).filter(|n| *n >= 1).ok_or("targetSlice every must be a whole number of at least 1")?;
    let offset = slice.get("offset").map_or(Some(0), Value::as_u64).filter(|n| *n < every).ok_or("targetSlice offset must be a whole number below every")?;
    let picked: Vec<(Uuid, String)> = (from - 1 + offset..to)
        .step_by(every as usize)
        .map(|i| {
            let child = tree.node(children[i as usize]);
            (child.id, child.name.clone())
        })
        .collect();
    if picked.is_empty() {
        return Err("targetSlice picks no children".into());
    }
    Ok(picked)
}
/// A validated effect_add, ready to apply without failing.
#[derive(Clone)]
struct NewEffect {
    type_id: Uuid,
    target: Uuid,
    target_name: String,
    start: TimeSpan,
    duration: TimeSpan,
    params: Vec<(&'static str, ParamValue)>,
    layer: Option<Uuid>,
    /// A level parameter to scale by this factor after params are set (effects_on_marks levelFromStrength).
    level: Option<(&'static str, f64)>,
}
/// One planned effect per target, each with the same type, time, parameters and layer.
fn plan_add(core: &AppCore, s: &SequenceSession, a: &Value) -> Result<Vec<NewEffect>, String> {
    let targets = targets(core, a)?;
    let type_id = uuid(a, "typeId")?;
    let start = ms(a, "startMs")?;
    let duration = ms(a, "durationMs")?;
    if duration < 1.0 || start + duration > s.length().total_ms() {
        return Err("Effect must fit within sequence and have positive duration".into());
    }
    let descriptor = vixen_effect_registry::by_type_id(&type_id).ok_or("Effect type not found")?;
    let params = match a.get("params") {
        Some(params) => parameter_values(descriptor, params)?,
        None => Vec::new(),
    };
    let layer = match a.get("layerId") {
        Some(_) => {
            let layer = uuid(a, "layerId")?;
            if !s.document().layers.layers.iter().any(|l| l.id == layer) {
                return Err("Layer not found".into());
            }
            Some(layer)
        }
        None => None,
    };
    Ok(targets
        .into_iter()
        .map(|(target, target_name)| NewEffect {
            type_id,
            target,
            target_name,
            start: TimeSpan::from_ms(start),
            duration: TimeSpan::from_ms(duration),
            params: params.clone(),
            layer,
            level: None,
        })
        .collect())
}
const BAND_FIELDS: &[&str] = &["fromPct", "toPct", "color", "level"];
/// Wipe parameters that hold a still horizontal band from `from` to `to` percent of the targets' height, counted from the
/// bottom: Vertical, Movement mode, a flat MovementCurve and a pulse as wide as the band.
fn band_params(from: f64, to: f64, color: &str, level: f64) -> Value {
    // Movement mode spreads the curve over the height plus one band width, and lights the band below that point.
    let position = 100.0 * to / (100.0 + (to - from));
    json!({
        "Direction": "Vertical",
        "WipeMovement": "Movement",
        "TargetNodeHandling": "Group",
        "MovementCurve": [[0, position], [100, position]],
        "PulsePercent": to - from,
        "ColorGradient": [color],
        "Curve": [[0, level], [100, level]],
    })
}
/// effect_band and the batch band op: a native Wipe set up as a still band, so the show stays readable everywhere.
fn plan_band(core: &AppCore, s: &SequenceSession, a: &Value) -> Result<Vec<NewEffect>, String> {
    let pct = |k: &str, default: Option<f64>| {
        a.get(k).map_or(default, Value::as_f64).filter(|n| (0.0..=100.0).contains(n)).ok_or(format!("{k} must be a percentage in 0..100"))
    };
    let (from, to) = (pct("fromPct", None)?, pct("toPct", None)?);
    if to - from < 1.0 {
        return Err("toPct must be at least 1 above fromPct".into());
    }
    let level = pct("level", Some(100.0))?;
    let color = a.get("color").map_or(Some("#ffffff"), Value::as_str).ok_or("color must be #RRGGBB")?;
    let wipe = vixen_effect_registry::all().iter().find(|d| d.name == "Wipe").ok_or("Wipe effect not available")?;
    let mut add = a.as_object().cloned().unwrap_or_default();
    add.retain(|k, _| !BAND_FIELDS.contains(&k.as_str()) && k != "op");
    add.insert("typeId".into(), json!(wipe.type_id.to_string()));
    add.insert("params".into(), band_params(from, to, color, level));
    plan_add(core, s, &Value::Object(add))
}
/// Adds a planned effect with its parameters and layer, with no undo record. The added effect's undo keeps both.
fn apply_add(s: &mut SequenceSession, new: NewEffect) -> EffectId {
    let id = s
        .add_new_effect(new.type_id, new.target, &new.target_name, new.start, new.duration)
        .expect("type checked by plan_add");
    for (name, value) in new.params {
        s.set_param(id, name, value);
    }
    if let Some((name, factor)) = new.level {
        let scaled = mark_effects::scale_level(s.effect(id).unwrap().node.params.get(name), factor);
        s.set_param(id, name, scaled);
    }
    if let Some(layer) = new.layer {
        s.set_layer(&[id], layer);
    }
    id
}
const BATCH_LIMIT: usize = 100_000;
const OPS_FILE_LIMIT: u64 = 64 * 1024 * 1024;
/// Reads batch ops from a JSON file inside the profile: a bare array, or an object with an `ops` array.
fn ops_file(core: &AppCore, relative: &str) -> Result<Vec<Value>, String> {
    use std::path::{Component, Path};
    let rel = Path::new(relative);
    if relative.len() > 1024
        || !relative.to_lowercase().ends_with(".json")
        || !rel.components().all(|c| matches!(c, Component::Normal(_)))
    {
        return Err("opsFile must be a .json path relative to the profile folder, without .. or a leading /".into());
    }
    let root = core.paths.root().canonicalize().map_err(|e| e.to_string())?;
    let mut path = root.clone();
    for component in rel.components() {
        path.push(component);
        let meta = std::fs::symlink_metadata(&path).map_err(|_| "opsFile not found in the profile folder")?;
        if meta.file_type().is_symlink() {
            return Err("opsFile must not be or pass through a symlink".into());
        }
    }
    let meta = std::fs::metadata(&path).map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.len() > OPS_FILE_LIMIT {
        return Err("opsFile must be an ordinary file of at most 64 MiB".into());
    }
    let text = std::fs::read(&path).map_err(|e| e.to_string())?;
    let value: Value = serde_json::from_slice(&text).map_err(|e| format!("opsFile is not valid JSON: {e}"))?;
    match value {
        Value::Array(ops) => Ok(ops),
        Value::Object(mut object) => match object.remove("ops") {
            Some(Value::Array(ops)) => Ok(ops),
            _ => Err("opsFile must hold an array of ops, or an object with an ops array".into()),
        },
        _ => Err("opsFile must hold an array of ops, or an object with an ops array".into()),
    }
}
fn op_schema() -> Value {
    let string = json!({"type":"string","minLength":1,"maxLength":256});
    let number = json!({"type":"number","minimum":0,"maximum":3600000});
    let percent = json!({"type":"number","minimum":0,"maximum":100});
    json!({"type":"object","properties":{"op":{"type":"string","enum":["add","band","delete","timing","params"]},"typeId":string,"targetId":string,"targetIds":target_ids_schema(),"targetSlice":target_slice_schema(),"startMs":number,"durationMs":number,"params":{"type":"object"},"layerId":string,"effectId":{"type":"integer","minimum":0},"fromPct":percent,"toPct":percent,"color":string,"level":percent},"required":["op"],"additionalProperties":false})
}
fn target_ids_schema() -> Value {
    json!({"type":"array","items":{"type":"string","minLength":1,"maxLength":256},"minItems":1,"maxItems":TARGET_LIMIT})
}
fn target_slice_schema() -> Value {
    let whole = json!({"type":"integer","minimum":0});
    json!({"type":"object","properties":{"groupId":{"type":"string","minLength":1,"maxLength":256},"strings":{"type":"array","items":whole,"minItems":2,"maxItems":2},"every":whole,"offset":whole},"required":["groupId"],"additionalProperties":false})
}
enum BatchOp {
    Add(Vec<NewEffect>),
    Delete(EffectId),
    Timing(EffectId, TimeSpan, TimeSpan),
    Params(EffectId, Vec<(&'static str, ParamValue)>),
}
/// Validates every op before changing anything, applies them in order as one undo step, and renders once at the end.
fn effects_batch(core: &AppCore, s: &mut SequenceSession, a: &Value, agent: bool) -> Result<Value, String> {
    let file;
    let ops: &[Value] = match (a.get("ops"), a.get("opsFile")) {
        (Some(ops), None) => ops.as_array().unwrap(),
        (None, Some(path)) => {
            file = ops_file(core, path.as_str().unwrap_or_default())?;
            &file
        }
        _ => return Err("Give exactly one of ops or opsFile".into()),
    };
    if ops.is_empty() || ops.len() > BATCH_LIMIT {
        return Err(format!("A batch needs 1..{BATCH_LIMIT} ops"));
    }
    let schema = op_schema();
    let mut deleted = std::collections::HashSet::new();
    let mut planned = Vec::with_capacity(ops.len());
    let mut adds = 0usize;
    for (index, op) in ops.iter().enumerate() {
        let step = (|| -> Result<BatchOp, String> {
            crate::studio_schema::arguments(&schema, op)?;
            let kind = op["op"].as_str().unwrap();
            let allowed: &[&str] = match kind {
                "add" => &["op", "typeId", "targetId", "targetIds", "targetSlice", "startMs", "durationMs", "params", "layerId"],
                "band" => &["op", "targetId", "targetIds", "targetSlice", "startMs", "durationMs", "layerId", "fromPct", "toPct", "color", "level"],
                "delete" => &["op", "effectId"],
                "timing" => &["op", "effectId", "startMs", "durationMs"],
                _ => &["op", "effectId", "params"],
            };
            if let Some(extra) = op.as_object().unwrap().keys().find(|k| !allowed.contains(&k.as_str())) {
                return Err(format!("{extra} does not apply to a {kind} op"));
            }
            if kind == "add" {
                return Ok(BatchOp::Add(plan_add(core, s, op)?));
            }
            if kind == "band" {
                return Ok(BatchOp::Add(plan_band(core, s, op)?));
            }
            let id = effect_id(s, op)?;
            if deleted.contains(&id) {
                return Err("Effect was deleted earlier in this batch".into());
            }
            Ok(match kind {
                "delete" => {
                    confirm(a).map_err(|_| "Batches that delete effects require confirm:true".to_string())?;
                    deleted.insert(id);
                    BatchOp::Delete(id)
                }
                "timing" => {
                    let start = ms(op, "startMs")?;
                    let span = ms(op, "durationMs")?;
                    if span < 1.0 || start + span > s.length().total_ms() {
                        return Err("Effect must fit within sequence and have positive duration".into());
                    }
                    BatchOp::Timing(id, TimeSpan::from_ms(start), TimeSpan::from_ms(span))
                }
                _ => {
                    let descriptor = s.effect(id).unwrap().descriptor.ok_or("Unknown effect descriptor")?;
                    BatchOp::Params(id, parameter_values(descriptor, op.get("params").ok_or("params is required")?)?)
                }
            })
        })()
        .map_err(|e| format!("ops[{index}]: {e}"))?;
        if let BatchOp::Add(new) = &step {
            adds += new.len();
            if adds > BATCH_LIMIT {
                return Err(format!("ops[{index}]: a batch adds at most {BATCH_LIMIT} effects, counting one per target"));
            }
        }
        planned.push(step);
    }
    // Runs of adds and of deletes become one undo action each, so undo and redo stay one pass over the schedule.
    let mut actions = Vec::new();
    let (mut added, mut adding, mut deleting) = (Vec::new(), Vec::new(), Vec::new());
    let (mut removed, mut changed) = (0usize, 0usize);
    for op in planned {
        if !matches!(op, BatchOp::Add(_)) && !adding.is_empty() {
            actions.push(UndoAction::EffectsAdded { kind: AddRemoveKind::AddedOrRemoved, ids: std::mem::take(&mut adding) });
        }
        if !matches!(op, BatchOp::Delete(_)) && !deleting.is_empty() {
            let effects = s.remove_effects(&std::mem::take(&mut deleting));
            actions.push(UndoAction::EffectsRemoved { kind: AddRemoveKind::AddedOrRemoved, effects });
        }
        match op {
            BatchOp::Add(planned) => {
                for new in planned {
                    let id = apply_add(s, new);
                    added.push(id.0);
                    adding.push(id);
                }
            }
            BatchOp::Delete(id) => {
                deleting.push(id);
                removed += 1;
            }
            BatchOp::Timing(id, start, span) => {
                let old = s.effect(id).unwrap();
                let times = vec![(id, old.node.start, old.node.time_span)];
                s.set_times(&[(id, start, span)]);
                actions.push(UndoAction::TimesChanged { kind: MoveKind::Moved, times });
                changed += 1;
            }
            BatchOp::Params(id, values) => {
                let old = s.effect(id).unwrap().node.params.clone();
                for (name, value) in values {
                    s.set_param(id, name, value);
                }
                actions.push(UndoAction::ParamsChanged { label: "Batch parameters".into(), params: vec![(id, old)] });
                changed += 1;
            }
        }
    }
    if !adding.is_empty() {
        actions.push(UndoAction::EffectsAdded { kind: AddRemoveKind::AddedOrRemoved, ids: adding });
    }
    if !deleting.is_empty() {
        let effects = s.remove_effects(&deleting);
        actions.push(UndoAction::EffectsRemoved { kind: AddRemoveKind::AddedOrRemoved, effects });
    }
    s.record_undo(UndoAction::Group {
        label: format!("Batch edit{}", if agent { " (Agent)" } else { "" }),
        actions,
    });
    s.render_all_dirty();
    Ok(json!({"effectIds":added,"removed":removed,"changed":changed,"effects":s.effect_count()}))
}
/// Drops this Studio session's hold on a document. An agent-made or agent-opened document that nothing else holds is closed;
/// one the app still has open in a window stays open there.
fn close(core: &AppCore, shared: Arc<Mutex<SequenceSession>>, a: &Value) -> Result<Value, String> {
    let session_id = shared.lock().playback_id();
    let mut owned = core.studio_display.owned_sequences.lock();
    let held = owned.get(&session_id).is_some_and(|o| Arc::ptr_eq(o, &shared));
    // Holders besides this call and the owned map: app windows.
    let others = Arc::strong_count(&shared) - 1 - usize::from(held);
    if others == 0 {
        if shared.lock().is_modified() && a["confirm"] != true {
            return Err("Unsaved changes would be lost: sequence_save first, or pass confirm:true to discard them".into());
        }
        core.playback.stop_session(session_id);
    }
    if held {
        owned.remove(&session_id);
    }
    drop(owned);
    drop(shared);
    Ok(json!({"closed":others == 0,"openInApp":others > 0}))
}

/// preview_video and preview_sheet: many frames from one rendered schedule, without holding the sequence lock while painting.
fn preview_range(core: &AppCore, shared: &Arc<Mutex<SequenceSession>>, name: &str, a: &Value) -> Result<Value, String> {
    confirm(a)?;
    let video = name == "preview_video";
    let file = filename(a, "file")?;
    let extension = if video { ".mp4" } else { ".png" };
    if !file.to_lowercase().ends_with(extension) {
        return Err(format!("Use {extension} extension"));
    }
    let path = output(core, "Export", &file)?;
    let (length, audio) = {
        let s = shared.lock();
        let audio = crate::export::audio_file_name(s.document(), &core.paths);
        (s.length().total_ms(), (!audio.is_empty()).then(|| PathBuf::from(audio)).filter(|p| p.is_file()))
    };
    let start = if a.get("startMs").is_some() { ms(a, "startMs")? } else { 0.0 };
    let end = if a.get("endMs").is_some() { ms(a, "endMs")?.min(length) } else { length };
    if start >= end {
        return Err(format!("startMs must be before endMs and inside the sequence (0..{length})"));
    }
    let fps = a.get("fps").map_or(Some(if video { 25.0 } else { 2.0 }), Value::as_f64).ok_or("fps must be a number")?;
    let range = crate::studio_preview::Range {
        preview_id: uuid(a, "previewId")?,
        start_ms: start,
        end_ms: end,
        fps,
        glow: a["glow"] == true,
    };
    if video {
        if fps.fract() != 0.0 || !(1.0..=60.0).contains(&fps) {
            return Err("fps must be a whole number in 1..60".into());
        }
        if end - start > 900_000.0 {
            return Err("A preview video is limited to 15 minutes".into());
        }
        let scale = a.get("scale").map_or(Some(1.0), Value::as_f64).filter(|s| (0.1..=1.0).contains(s)).ok_or("scale must be in 0.1..1")?;
        let audio = if a["withAudio"] == false { None } else { audio };
        crate::studio_preview::video(core, shared, &range, scale as f32, audio.as_deref(), &path)
    } else {
        if !(0.1..=30.0).contains(&fps) {
            return Err("fps must be in 0.1..30".into());
        }
        let whole = |k: &str, default: u32, range: std::ops::RangeInclusive<u32>| {
            a.get(k).map_or(Some(default), |v| v.as_u64().and_then(|n| u32::try_from(n).ok())).filter(|n| range.contains(n)).ok_or(format!("{k} must be a whole number in {}..{}", range.start(), range.end()))
        };
        crate::studio_preview::sheet(core, shared, &range, whole("cols", 8, 1..=30)?, whole("tileWidth", 160, 64..=640)?, &path)
    }
}

pub fn tools() -> Vec<Value> {
    let string = json!({"type":"string","minLength":1,"maxLength":256});
    let number = json!({"type":"number","minimum":0,"maximum":3600000});
    let boolean = json!({"type":"boolean"});
    let entries=vec![
        ("effect_parameters","Read current typed effect parameter values and writable schemas. Native gradient stop positions are reported separately from the evenly-spaced write format.",json!({"sequenceId":string,"effectId":{"type":"integer","minimum":0}}),vec!["sequenceId","effectId"],true),
        ("sequence_length","Set length without truncating effects, with shared undo.",json!({"sequenceId":string,"lengthMs":number}),vec!["sequenceId","lengthMs"],false),
        ("sequence_layers","List layers and mixing filters.",json!({"sequenceId":string}),vec!["sequenceId"],true),
        ("layer_create","Create a highest-value mixing layer with undo.",json!({"sequenceId":string,"name":string}),vec!["sequenceId","name"],false),
        ("layer_rename","Rename a layer with undo.",json!({"sequenceId":string,"layerId":string,"name":string}),vec!["sequenceId","layerId","name"],false),
        ("layer_delete","Delete a non-default layer with confirmation and undo.",json!({"sequenceId":string,"layerId":string,"confirm":boolean}),vec!["sequenceId","layerId","confirm"],false),
        ("effect_layer","Assign an effect to a layer with undo.",json!({"sequenceId":string,"effectId":{"type":"integer","minimum":0},"layerId":string}),vec!["sequenceId","effectId","layerId"],false),
        ("effect_copy","Copy an effect and parameters to another target/time in one undo step.",json!({"sequenceId":string,"effectId":{"type":"integer","minimum":0},"targetId":string,"startMs":number}),vec!["sequenceId","effectId","targetId","startMs"],false),
        ("sequence_play","Play muted in headless mode. A non-muted app requires local agent-output permission and confirm:true because sound/output may occur.",json!({"sequenceId":string,"timeMs":number,"confirm":boolean}),vec!["sequenceId","timeMs"],false),
        ("sequence_stop","Stop this sequence if it owns transport.",json!({"sequenceId":string}),vec!["sequenceId"],false),
        ("sequence_pause","Pause this sequence if it owns transport.",json!({"sequenceId":string}),vec!["sequenceId"],false),
        ("sequence_seek","Seek in milliseconds. Non-muted seeking needs local permission and confirm:true.",json!({"sequenceId":string,"timeMs":number,"confirm":boolean}),vec!["sequenceId","timeMs"],false),

        ("effect_set_parameter","Set one typed parameter through native setters, with undo. See effect_catalog schemas. File-path parameters are not accepted.",json!({"sequenceId":string,"effectId":{"type":"integer","minimum":0},"parameter":string,"value":{}}),vec!["sequenceId","effectId","parameter","value"],false),
        ("preview_frame","Render real preview light geometry at a sequence time to a PNG inspection frame. Omits background/fixture beams. Requires confirmed output filename.",json!({"sequenceId":string,"previewId":string,"timeMs":number,"file":string,"confirm":boolean}),vec!["sequenceId","previewId","timeMs","file","confirm"],false),
        ("preview_video","Render a sequence span as an H.264 MP4 of the preview's lights, with the sequence audio for that span as AAC, under profile Export. One call replaces a preview_frame per frame plus ffmpeg. Defaults: whole sequence, 25 fps, audio on, glow off, scale 1. Up to 15 minutes. Requires confirmed output filename.",json!({"sequenceId":string,"previewId":string,"startMs":number,"endMs":number,"fps":{"type":"integer","minimum":1,"maximum":60},"withAudio":boolean,"glow":boolean,"scale":{"type":"number","minimum":0.1,"maximum":1},"file":string,"confirm":boolean}),vec!["sequenceId","previewId","file","confirm"],false),
        ("preview_sheet","Render a sequence span as one PNG contact sheet under profile Export: frames every 1/fps seconds tiled left to right, top to bottom, each captioned with its m:ss.cc time. A cheap way to review 10-20 s for dead air, mush or off-beat motion. Defaults: 2 fps, 8 columns, 160 px tiles. At most 400 tiles. Requires confirmed output filename.",json!({"sequenceId":string,"previewId":string,"startMs":number,"endMs":number,"fps":{"type":"number","minimum":0.1,"maximum":30},"cols":{"type":"integer","minimum":1,"maximum":30},"tileWidth":{"type":"integer","minimum":64,"maximum":640},"glow":boolean,"file":string,"confirm":boolean}),vec!["sequenceId","previewId","startMs","endMs","file","confirm"],false),
        ("music_analyze","Analyze actual profile media with Vixen's beat/bar tracker and per-band spectral-flux onsets; returns millisecond beats, bars, tempo, onsets with bar/16th slots and a per-bar slot-strength grid. Assumes 4/4; no playback.",json!({"media":string}),vec!["media"],true),
        ("sequence_create","Create a new in-memory sequence with optional profile audio; never plays sound. Its sequenceId changes to the file's after sequence_save (the old one keeps working).",json!({"lengthMs":number,"media":string}),vec!["lengthMs"],false),
        ("sequence_open","Open a profile Sequence filename, a Sequence/.scratch filename with scratch:true, or an absolute path to a .tim file anywhere (opened in place; sequence_save still writes only into the profile). sequenceId is derived from the file, so every connection gets the same ID. alreadyOpen:true means this Studio session already had the file open (in the app or by an agent) and you are attached to that same document and undo stack; reload:true instead discards that copy and reads the file again (same sequenceId; unsaved changes need confirm:true; refused while an app window has it open). sharedWithApp:false means a headless copy the app does not see. Returns file, path and modified.",json!({"file":{"type":"string","minLength":1,"maxLength":1024},"scratch":boolean,"reload":boolean,"confirm":boolean}),vec!["file"],false),
        ("sequence_status","Shared document file, path, dirty/render/undo status and sharedWithApp.",json!({"sequenceId":string}),vec!["sequenceId"],true),
        ("sequence_effects","List up to 10000 effects with stable in-session IDs, target UUIDs and millisecond times.",json!({"sequenceId":string}),vec!["sequenceId"],true),
        ("effect_add","Add an undoable effect. Give one target: targetId (an element or group UUID); targetIds (up to 4096 UUIDs, one effect each); or targetSlice {groupId, strings?:[from,to], every?, offset?}, which picks the group's children by position without changing the display (strings counts children from 1, inclusive; every:2 takes every second one, starting offset children into the range). Several targets add one effect per target, all undone together. Optional params (name to value, as effect_parameters reports them) and layerId are applied in the same undo step. Returns effectId (the first) and effectIds.",json!({"sequenceId":string,"typeId":string,"targetId":string,"targetIds":target_ids_schema(),"targetSlice":target_slice_schema(),"startMs":number,"durationMs":number,"params":{"type":"object"},"layerId":string}),vec!["sequenceId","typeId","startMs","durationMs"],false),
        ("effect_band","Add a still horizontal band of light from fromPct to toPct of the targets' height, counted from the bottom (0 = lowest light, 100 = highest), e.g. a ring on a megatree or rows of a matrix. It is a native Wipe (Vertical, Movement, flat MovementCurve, pulse as wide as the band) that the effect editor and Windows Vixen open normally; band edges land on the nearest preview row. Targets as effect_add (targetId, targetIds or targetSlice); several targets give one band each, measured on each target's own height. color is #RRGGBB (default white), level the brightness in percent (default 100). Like any Wipe it only lights elements that have Vixen's Location property; elements without one stay dark (element_location gives them one from their preview positions). Returns effectId and effectIds.",json!({"sequenceId":string,"targetId":string,"targetIds":target_ids_schema(),"targetSlice":target_slice_schema(),"startMs":number,"durationMs":number,"fromPct":{"type":"number","minimum":0,"maximum":100},"toPct":{"type":"number","minimum":0,"maximum":100},"color":string,"level":{"type":"number","minimum":0,"maximum":100},"layerId":string}),vec!["sequenceId","startMs","durationMs","fromPct","toPct"],false),
        ("effects_batch","Apply many effect edits as one undo step with one render pass. Every op is validated before anything changes; an invalid op fails the whole batch and names its index. Ops: {op:\"add\",typeId,targetId|targetIds|targetSlice,startMs,durationMs,params?,layerId?} (one effect per target, as effect_add), {op:\"band\",targetId|targetIds|targetSlice,startMs,durationMs,fromPct,toPct,color?,level?,layerId?} (as effect_band), {op:\"delete\",effectId}, {op:\"timing\",effectId,startMs,durationMs}, {op:\"params\",effectId,params}. Give ops inline, or opsFile: a .json path relative to the profile folder holding the same array (or {ops:[...]}), up to 100000 ops and 64 MiB, for shows too large for one request; at most 100000 added effects in all. Deletes need confirm:true. Returns effectIds of the adds in order, every target's effect in its op's order.",json!({"sequenceId":string,"ops":{"type":"array","items":op_schema(),"minItems":1,"maxItems":BATCH_LIMIT},"opsFile":{"type":"string","minLength":1,"maxLength":1024},"confirm":boolean}),vec!["sequenceId"],false),
        ("sequence_clear","Delete every effect in the sequence as one undo step; marks, layers and length stay. Requires confirmation.",json!({"sequenceId":string,"confirm":boolean}),vec!["sequenceId","confirm"],false),
        ("sequence_close","Close a sequence this Studio session created or opened, freeing it. If the app has it open in a window it stays open there (openInApp:true). Discarding unsaved changes requires confirm:true; save first to keep them.",json!({"sequenceId":string,"confirm":boolean}),vec!["sequenceId"],false),
        ("effect_delete","Delete an effect; requires confirmation.",json!({"sequenceId":string,"effectId":{"type":"integer","minimum":0},"confirm":boolean}),vec!["sequenceId","effectId","confirm"],false),
        ("effect_timing","Move/resize an effect in milliseconds, in one undo step.",json!({"sequenceId":string,"effectId":{"type":"integer","minimum":0},"startMs":number,"durationMs":number}),vec!["sequenceId","effectId","startMs","durationMs"],false),
        ("sequence_marks","List mark collections and timestamps.",json!({"sequenceId":string}),vec!["sequenceId"],true),
        ("marks_create","Create undoable timestamp marks, optionally authored phrase/word/phoneme text. No automatic speech recognition.",json!({"sequenceId":string,"name":string,"timesMs":{"type":"array","items":number,"maxItems":10000},"texts":{"type":"array","items":string,"maxItems":10000},"collectionType":{"type":"string","enum":["generic","phrase","word","phoneme","state"]}}),vec!["sequenceId","name","timesMs"],false),
        ("sequence_music_marks","Analyze profile media and append beat/bar collections to the sequence in one undo step.",json!({"sequenceId":string,"media":string}),vec!["sequenceId","media"],false),
        ("sequence_onset_marks","Detect per-band spectral-flux onsets (as music_analyze) in the sequence's audio, or media, and add them as mark collections in one undo step: one per band, named \"<prefix> - low\", \"- mid\" and \"- high\" (prefix default \"Onsets\"). Each mark's text is the hit's strength, 0..1 with two decimals, so the rhythm shows on the timeline and can be hand-edited (move, delete or add marks; retype a strength) before effects_on_marks places effects on it. bands picks which bands to add (default all three); minStrength drops weaker hits (default 0). Onset times are estimates, typically within 10 ms. Returns each collection's id, name, band and mark count.",json!({"sequenceId":string,"media":string,"bands":{"type":"array","items":{"type":"string","enum":["low","mid","high"]},"minItems":1,"maxItems":3},"minStrength":{"type":"number","minimum":0,"maximum":1},"prefix":string}),vec!["sequenceId"],false),
        ("effects_on_marks",mark_effects::DESCRIPTION,mark_effects::schema(),vec!["sequenceId","markCollection","template"],false),
        ("effect_step",step_effects::DESCRIPTION,step_effects::schema(),vec!["sequenceId","mode","segments"],false),
        ("sequence_undo","Undo one shared sequence edit.",json!({"sequenceId":string}),vec!["sequenceId"],false),
        ("sequence_redo","Redo one shared sequence edit.",json!({"sequenceId":string}),vec!["sequenceId"],false),
        ("sequence_render","Render dirty effects and wait for the real sequence schedule; no playback/output.",json!({"sequenceId":string}),vec!["sequenceId"],false),
        ("sequence_save","Save/overwrite a .tim filename under profile Sequence. scratch:true saves under Sequence/.scratch instead, hidden from the app's sequence list, for throwaway builds and previews (sequence_open with scratch:true reads it back). backup:false overwrites without copying the old file into auto_backup. Requires confirmation.",json!({"sequenceId":string,"file":string,"scratch":boolean,"backup":boolean,"confirm":boolean}),vec!["sequenceId","file","confirm"],false),
        ("sequence_export_fseq","Render and export uncompressed FSEQ v2 at 50 ms under profile Export. Requires confirmation; no network transfer.",json!({"sequenceId":string,"file":string,"confirm":boolean}),vec!["sequenceId","file","confirm"],false),
    ];
    entries.into_iter().map(|(name,description,properties,required,read_only)|json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false},"annotations":{"readOnlyHint":read_only,"destructiveHint":matches!(name,"effect_delete"|"effects_batch"|"sequence_clear"|"sequence_close"|"layer_delete"|"sequence_save"|"sequence_export_fseq"|"preview_frame"|"preview_video"|"preview_sheet"|"sequence_play"|"sequence_seek"),"openWorldHint":matches!(name,"sequence_play"|"sequence_seek")}})).collect()
}

pub fn execute(core: &AppCore, name: &str, args: &Value, agent: bool) -> Result<Value, String> {
    let guard = core.studio_display.sequence_gate.lock();
    if core.studio_display.is_stopped() {
        return Err("Profile is closing".into());
    }
    let result = execute_inner(core, name, args, agent);
    drop(guard);
    if result.is_ok()
        && tools()
            .iter()
            .any(|t| t["name"] == name && t["annotations"]["readOnlyHint"] == false)
    {
        core.studio_display.document_changed(
            name,
            args["sequenceId"].as_str().map(str::to_string).or_else(|| {
                result
                    .as_ref()
                    .ok()
                    .and_then(|v| v["sequenceId"].as_str().map(str::to_string))
            }),
            agent,
        );
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rapid_agent_edits_schedule_each_effect_once() {
        let root = std::env::temp_dir().join(format!("studio-rapid-{}", Uuid::new_v4()));
        let core = AppCore::open(root.clone(), true).unwrap();
        let group = core.studio_display
            .execute(&core, "element_template", &json!({"kind":"megatree","name":"Tree","count":4,"pixels":50}), false)
            .unwrap()["result"]["id"]
            .clone();
        let run = |name: &str, args: Value| execute(&core, name, &args, true).unwrap();
        let id = run("sequence_create", json!({"lengthMs":60000}))["sequenceId"].clone();
        let pulse = vixen_effect_registry::all().iter().find(|d| d.name == "Pulse").unwrap().type_id.to_string();
        // One effect's entries, measured alone after its render has finished.
        run("effect_add", json!({"sequenceId":id,"typeId":pulse,"targetId":group,"startMs":0,"durationMs":400}));
        let one = run("sequence_render", json!({"sequenceId":id}))["scheduleEntries"].as_u64().unwrap();
        assert!(one > 0);
        // Back-to-back edits, as an agent sends them, without waiting for renders in between.
        for i in 1..120u64 {
            run("effect_add", json!({"sequenceId":id,"typeId":pulse,"targetId":group,"startMs":i*450,"durationMs":400}));
        }
        let all = run("sequence_render", json!({"sequenceId":id}));
        assert_eq!(all["rendered"], 120);
        assert_eq!(all["scheduleEntries"].as_u64().unwrap(), one * 120, "every effect is scheduled exactly once");
        core.studio_display.stop();
        drop(core);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn shared_length_layers_effects_and_parameter_undo() {
        let root = std::env::temp_dir().join(format!("studio-sequence-{}", Uuid::new_v4()));
        let core = AppCore::open(root.clone(), true).unwrap();
        let target = core
            .studio_display
            .execute(&core, "element_create", &json!({"name":"Pixel"}), false)
            .unwrap()["result"]["ids"][0]
            .clone();
        core.studio_display
            .execute(
                &core,
                "element_color",
                &json!({"id":target,"mode":"rgb"}),
                false,
            )
            .unwrap();
        let run = |name: &str, args: Value| execute(&core, name, &args, true).unwrap();
        let id = run("sequence_create", json!({"lengthMs":30000}))["sequenceId"].clone();
        run("sequence_length", json!({"sequenceId":id,"lengthMs":40000}));
        run("sequence_undo", json!({"sequenceId":id}));
        assert_eq!(
            run("sequence_status", json!({"sequenceId":id}))["lengthMs"],
            30000.0
        );
        let layers = run("sequence_layers", json!({"sequenceId":id}))
            .as_array()
            .unwrap()
            .len();
        run("layer_create", json!({"sequenceId":id,"name":"Overlay"}));
        assert_eq!(
            run("sequence_layers", json!({"sequenceId":id}))
                .as_array()
                .unwrap()
                .len(),
            layers + 1
        );
        run("sequence_undo", json!({"sequenceId":id}));
        assert_eq!(
            run("sequence_layers", json!({"sequenceId":id}))
                .as_array()
                .unwrap()
                .len(),
            layers
        );
        let desc = *vixen_effect_registry::all()
            .iter()
            .find(|d| d.name == "Pulse")
            .unwrap();
        let effect=run("effect_add",json!({"sequenceId":id,"typeId":desc.type_id.to_string(),"targetId":target,"startMs":100,"durationMs":500}))["effectId"].clone();
        let parameter = desc
            .params
            .iter()
            .find(|p| matches!(p.kind, vixen_effects::framework::ParamKind::Curve))
            .unwrap();
        run(
            "effect_set_parameter",
            json!({"sequenceId":id,"effectId":effect,"parameter":parameter.name,"value":[[0,0],[50,100],[100,0]]}),
        );
        let values = run(
            "effect_parameters",
            json!({"sequenceId":id,"effectId":effect}),
        );
        assert!(values["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == parameter.name));
        assert!(execute(
            &core,
            "sequence_save",
            &json!({"sequenceId":id,"file":"../escape.tim","confirm":true}),
            true
        )
        .is_err());
        core.studio_display.stop();
        assert!(execute(
            &core,
            "sequence_length",
            &json!({"sequenceId":id,"lengthMs":50000}),
            true
        )
        .is_err());
        drop(core);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn batch_clear_and_close() {
        let root = std::env::temp_dir().join(format!("studio-batch-{}", Uuid::new_v4()));
        let core = AppCore::open(root.clone(), true).unwrap();
        let target = core.studio_display.execute(&core, "element_create", &json!({"name":"Pixel"}), false).unwrap()["result"]["ids"][0].clone();
        let run = |name: &str, args: Value| execute(&core, name, &args, true).unwrap();
        let fail = |name: &str, args: Value| execute(&core, name, &args, true).unwrap_err();
        let id = run("sequence_create", json!({"lengthMs":3_600_000}))["sequenceId"].clone();
        let pulse = vixen_effect_registry::all().iter().find(|d| d.name == "Pulse").unwrap().type_id.to_string();
        let curve = json!([[0,0],[50,100],[100,0]]);

        // Inline params on effect_add, kept by one undo/redo.
        let single = run("effect_add", json!({"sequenceId":id,"typeId":pulse,"targetId":target,"startMs":0,"durationMs":400,"params":{"LevelCurve":curve,"ColorGradient":["#ff0000","#0000ff"]}}))["effectId"].clone();
        let level = |effect: &Value| run("effect_parameters", json!({"sequenceId":id,"effectId":effect}))["parameters"].as_array().unwrap().iter().find(|p| p["name"]=="LevelCurve").unwrap()["value"].clone();
        let shaped = level(&single);
        let default = vixen_effect_registry::all().iter().find(|d| d.name == "Pulse").unwrap().params.iter().find(|p| p.name == "LevelCurve").unwrap().default;
        assert_ne!(shaped, crate::studio_params::encode(&default()), "the inline curve was applied");
        run("sequence_undo", json!({"sequenceId":id}));
        assert_eq!(run("sequence_status", json!({"sequenceId":id}))["effects"], 0);
        run("sequence_redo", json!({"sequenceId":id}));
        assert_eq!(level(&single), shaped, "redo restores the inline parameters");
        // Color-array gradients keep evenly spaced stops through every write path.
        let stops = |effect: &Value| {
            let parameters = run("effect_parameters", json!({"sequenceId":id,"effectId":effect}))["parameters"].clone();
            let gradient = parameters.as_array().unwrap().iter().find(|p| p["name"] == "ColorGradient").unwrap()["value"]["colors"].clone();
            gradient.as_array().unwrap().iter().map(|c| c["position"].as_f64().unwrap()).collect::<Vec<_>>()
        };
        assert_eq!(stops(&single), vec![0.0, 1.0], "effect_add params");
        run("effect_set_parameter", json!({"sequenceId":id,"effectId":single,"parameter":"ColorGradient","value":["#ff0000","#00ff00","#0000ff"]}));
        assert_eq!(stops(&single), vec![0.0, 0.5, 1.0], "effect_set_parameter");
        run("effects_batch", json!({"sequenceId":id,"ops":[{"op":"params","effectId":single,"params":{"ColorGradient":["#ff0000","#00ff00"]}}]}));
        assert_eq!(stops(&single), vec![0.0, 1.0], "effects_batch");
        for _ in 0..2 {
            run("sequence_undo", json!({"sequenceId":id}));
        }
        assert!(fail("effect_add", json!({"sequenceId":id,"typeId":pulse,"targetId":target,"startMs":0,"durationMs":400,"params":{"Nope":1}})).contains("Unknown parameter"));
        assert_eq!(run("sequence_status", json!({"sequenceId":id}))["effects"], 1, "a rejected add adds nothing");

        // A show-sized batch: one undo step, one render pass.
        let ops: Vec<Value> = (0..6000u64).map(|i| json!({"op":"add","typeId":pulse,"targetId":target,"startMs":i*500,"durationMs":400,"params":{"LevelCurve":curve}})).collect();
        let started = std::time::Instant::now();
        let batch = run("effects_batch", json!({"sequenceId":id,"ops":ops}));
        let applied = started.elapsed();
        let ids = batch["effectIds"].as_array().unwrap().clone();
        assert_eq!(ids.len(), 6000);
        assert_eq!(batch["effects"], 6001);
        assert_eq!(level(&ids[5999]), shaped);
        let render = run("sequence_render", json!({"sequenceId":id}));
        eprintln!("6000-effect batch applied in {applied:?}, rendered in {:?}", started.elapsed());
        assert_eq!(render["rendered"], 6001);
        assert!(applied < std::time::Duration::from_secs(20));
        let undo = run("sequence_status", json!({"sequenceId":id}))["undo"].clone();
        assert_eq!(undo[0], "Batch edit (Agent)");

        // Mixed ops in order, validated first: a bad op changes nothing and names its index.
        let before = run("sequence_status", json!({"sequenceId":id}))["undo"].as_array().unwrap().len();
        let bad = fail("effects_batch", json!({"sequenceId":id,"confirm":true,"ops":[{"op":"delete","effectId":ids[0]},{"op":"timing","effectId":ids[0],"startMs":0,"durationMs":10}]}));
        assert!(bad.starts_with("ops[1]:"), "{bad}");
        assert!(fail("effects_batch", json!({"sequenceId":id,"ops":[{"op":"delete","effectId":ids[0]}]})).contains("confirm"));
        assert!(fail("effects_batch", json!({"sequenceId":id,"ops":[{"op":"delete","effectId":ids[0],"typeId":pulse}]})).contains("does not apply"));
        assert_eq!(run("sequence_status", json!({"sequenceId":id}))["effects"], 6001);
        assert_eq!(run("sequence_status", json!({"sequenceId":id}))["undo"].as_array().unwrap().len(), before);
        let mixed = run("effects_batch", json!({"sequenceId":id,"confirm":true,"ops":[
            {"op":"delete","effectId":ids[0]},{"op":"delete","effectId":ids[1]},
            {"op":"timing","effectId":ids[2],"startMs":5,"durationMs":50},
            {"op":"params","effectId":ids[3],"params":{"LevelCurve":[[0,100],[100,100]]}},
            {"op":"add","typeId":pulse,"targetId":target,"startMs":10,"durationMs":20}]}));
        assert_eq!((mixed["removed"].clone(), mixed["changed"].clone(), mixed["effects"].clone()), (json!(2), json!(2), json!(6000)));
        run("sequence_undo", json!({"sequenceId":id}));
        assert_eq!(run("sequence_status", json!({"sequenceId":id}))["effects"], 6001);
        assert_eq!(level(&ids[3]), shaped);
        let timing = run("sequence_effects", json!({"sequenceId":id}))["effects"].as_array().unwrap().iter().find(|e| e["id"]==ids[2]).unwrap()["startMs"].clone();
        assert_eq!(timing, 1000.0);

        // Ops from a profile file, never outside it or through a link.
        std::fs::create_dir_all(root.join("_show_tools")).unwrap();
        std::fs::write(root.join("_show_tools/spec.json"), json!({"name":"spec","ops":[{"op":"add","typeId":pulse,"targetId":target,"startMs":0,"durationMs":100}]}).to_string()).unwrap();
        assert_eq!(run("effects_batch", json!({"sequenceId":id,"opsFile":"_show_tools/spec.json"}))["effectIds"].as_array().unwrap().len(), 1);
        let outside = root.with_extension("ops.json");
        std::fs::write(&outside, "[]").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("link.json")).unwrap();
        assert!(fail("effects_batch", json!({"sequenceId":id,"opsFile":"link.json"})).contains("symlink"));
        assert!(fail("effects_batch", json!({"sequenceId":id,"opsFile":"../x.json"})).contains("relative"));
        assert!(fail("effects_batch", json!({"sequenceId":id,"opsFile":outside.to_string_lossy()})).contains("relative"));
        assert!(fail("effects_batch", json!({"sequenceId":id})).contains("exactly one"));

        // Clear is one undo step.
        assert!(fail("sequence_clear", json!({"sequenceId":id,"confirm":false})).contains("confirm"));
        assert_eq!(run("sequence_clear", json!({"sequenceId":id,"confirm":true}))["removed"], 6002);
        assert_eq!(run("sequence_status", json!({"sequenceId":id}))["effects"], 0);
        run("sequence_undo", json!({"sequenceId":id}));
        assert_eq!(run("sequence_status", json!({"sequenceId":id}))["effects"], 6002);

        // Close: unsaved changes need confirm; afterwards the document is gone.
        assert!(fail("sequence_close", json!({"sequenceId":id})).contains("Unsaved"));
        assert_eq!(run("sequence_close", json!({"sequenceId":id,"confirm":true})), json!({"closed":true,"openInApp":false}));
        assert!(fail("sequence_status", json!({"sequenceId":id})).contains("not open"));
        // A window's hold keeps it open in the app.
        let kept = run("sequence_create", json!({"lengthMs":1000}))["sequenceId"].clone();
        let window = find(&core, Uuid::parse_str(kept.as_str().unwrap()).unwrap()).unwrap();
        assert_eq!(run("sequence_close", json!({"sequenceId":kept})), json!({"closed":false,"openInApp":true}));
        assert!(core.studio_display.owned_sequences.lock().is_empty());
        drop(window);
        assert!(fail("sequence_status", json!({"sequenceId":kept})).contains("not open"));

        core.studio_display.stop();
        drop(core);
        let _ = std::fs::remove_file(outside);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn scratch_saves_skip_backups_and_open_reads_paths_and_reloads() {
        let root = std::env::temp_dir().join(format!("studio-scratch-{}", Uuid::new_v4()));
        let core = AppCore::open(root.clone(), true).unwrap();
        let run = |name: &str, args: Value| execute(&core, name, &args, true).unwrap();
        let fail = |name: &str, args: Value| execute(&core, name, &args, true).unwrap_err();
        let sequence_dir = core.paths.sequence_dir();
        let scratch_dir = sequence_dir.join(".scratch");
        let id = run("sequence_create", json!({"lengthMs":1000}))["sequenceId"].clone();

        // Scratch saves land in Sequence/.scratch, never beside the user's sequences, and backup:false keeps no copies.
        let saved = run("sequence_save", json!({"sequenceId":id,"file":"_preview.tim","scratch":true,"backup":false,"confirm":true}));
        assert_eq!(PathBuf::from(saved["path"].as_str().unwrap()), scratch_dir.canonicalize().unwrap().join("_preview.tim"));
        assert!(!sequence_dir.join("_preview.tim").exists());
        let scratch_id = saved["sequenceId"].clone();
        run("sequence_length", json!({"sequenceId":scratch_id,"lengthMs":2000}));
        run("sequence_save", json!({"sequenceId":scratch_id,"file":"_preview.tim","scratch":true,"backup":false,"confirm":true}));
        assert!(!scratch_dir.join("auto_backup").exists(), "backup:false copies nothing into auto_backup");
        assert!(std::fs::read_dir(sequence_dir.join("auto_backup")).map_or(true, |mut d| d.next().is_none()));
        assert_eq!(run("sequence_status", json!({"sequenceId":scratch_id}))["modified"], false);
        // The default still keeps Vixen's backup, inside the hidden folder.
        run("sequence_length", json!({"sequenceId":scratch_id,"lengthMs":3000}));
        run("sequence_save", json!({"sequenceId":scratch_id,"file":"_preview.tim","scratch":true,"confirm":true}));
        assert_eq!(std::fs::read_dir(scratch_dir.join("auto_backup")).unwrap().count(), 1);
        assert!(fail("sequence_save", json!({"sequenceId":id,"file":"../x.tim","scratch":true,"confirm":true})).contains("filename"));

        // Reopening by scratch name attaches; reload reads the file again under the same ID, and guards unsaved edits.
        let opened = run("sequence_open", json!({"file":"_preview.tim","scratch":true}));
        assert_eq!((opened["sequenceId"].clone(), opened["alreadyOpen"].clone()), (scratch_id.clone(), json!(true)));
        run("sequence_length", json!({"sequenceId":scratch_id,"lengthMs":9000}));
        assert!(fail("sequence_open", json!({"file":"_preview.tim","scratch":true,"reload":true})).contains("Unsaved"));
        let reloaded = run("sequence_open", json!({"file":"_preview.tim","scratch":true,"reload":true,"confirm":true}));
        assert_eq!((reloaded["sequenceId"].clone(), reloaded["alreadyOpen"].clone(), reloaded["reloaded"].clone()), (scratch_id.clone(), json!(false), json!(true)));
        assert_eq!(run("sequence_status", json!({"sequenceId":scratch_id}))["lengthMs"], 3000.0);
        // A window's hold keeps the document; reload refuses rather than swapping it under the app.
        let window = find(&core, Uuid::parse_str(scratch_id.as_str().unwrap()).unwrap()).unwrap();
        assert!(fail("sequence_open", json!({"file":"_preview.tim","scratch":true,"reload":true})).contains("window"));
        drop(window);

        // An absolute path outside the profile opens in place; links, relative paths and the original archive are refused.
        let outside = root.with_extension("builds");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::copy(scratch_dir.join("_preview.tim"), outside.join("Build.tim")).unwrap();
        let build = outside.join("Build.tim").canonicalize().unwrap();
        let external = run("sequence_open", json!({"file":build.to_string_lossy()}));
        assert_eq!(PathBuf::from(external["path"].as_str().unwrap()), build);
        assert_eq!(external["alreadyOpen"], false);
        std::os::unix::fs::symlink(&build, outside.join("Link.tim")).unwrap();
        assert!(fail("sequence_open", json!({"file":outside.join("Link.tim").to_string_lossy()})).contains("symlink"));
        assert!(fail("sequence_open", json!({"file":format!("{}/../Build.tim", outside.display())})).contains(".."));
        assert!(fail("sequence_open", json!({"file":build.to_string_lossy(),"scratch":true})).contains("filename"));
        let original = outside.join("vixen-show-original");
        std::fs::create_dir_all(&original).unwrap();
        std::fs::copy(&build, original.join("Show.tim")).unwrap();
        assert!(fail("sequence_open", json!({"file":original.join("Show.tim").to_string_lossy()})).contains("original"));
        assert!(!sequence_dir.join("Build.tim").exists(), "opening by path copies nothing into the profile");

        core.studio_display.stop();
        drop(core);
        let _ = std::fs::remove_dir_all(outside);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn several_targets_and_slices_add_one_effect_each_in_one_undo_step() {
        let root = std::env::temp_dir().join(format!("studio-targets-{}", Uuid::new_v4()));
        let core = AppCore::open(root.clone(), true).unwrap();
        let tree = core.studio_display
            .execute(&core, "element_template", &json!({"kind":"megatree","name":"Tree","count":8,"pixels":10}), false)
            .unwrap()["result"]["id"]
            .clone();
        let strings: Vec<String> = {
            let system = core.system.read();
            let node = system.tree.find(&Uuid::parse_str(tree.as_str().unwrap()).unwrap()).unwrap();
            system.tree.node(node).children.iter().map(|c| system.tree.node(*c).id.to_string()).collect()
        };
        assert_eq!(strings.len(), 8);
        let run = |name: &str, args: Value| execute(&core, name, &args, true).unwrap();
        let fail = |name: &str, args: Value| execute(&core, name, &args, true).unwrap_err();
        let id = run("sequence_create", json!({"lengthMs":10_000}))["sequenceId"].clone();
        let pulse = vixen_effect_registry::all().iter().find(|d| d.name == "Pulse").unwrap().type_id.to_string();
        let targets_of = |ids: &Value| -> Vec<String> {
            let effects = run("sequence_effects", json!({"sequenceId":id}))["effects"].as_array().unwrap().clone();
            ids.as_array().unwrap().iter().map(|e| effects.iter().find(|x| x["id"] == *e).unwrap()["targets"][0].as_str().unwrap().to_string()).collect()
        };

        // targetIds: one effect per target, one undo step.
        let added = run("effect_add", json!({"sequenceId":id,"typeId":pulse,"targetIds":&strings[..4],"startMs":0,"durationMs":200,"params":{"LevelCurve":[[0,100],[100,0]]}}));
        assert_eq!(targets_of(&added["effectIds"]), strings[..4].to_vec());
        assert_eq!(added["effectId"], added["effectIds"][0]);
        assert_eq!(run("sequence_status", json!({"sequenceId":id}))["undo"][0], "Add effects (Agent)");
        run("sequence_undo", json!({"sequenceId":id}));
        assert_eq!(run("sequence_status", json!({"sequenceId":id}))["effects"], 0, "one undo removes every target's effect");

        // targetSlice: alternating halves, and a range, without touching the display.
        let revision = core.studio_display.snapshot(&core)["revision"].clone();
        let even = run("effect_add", json!({"sequenceId":id,"typeId":pulse,"targetSlice":{"groupId":tree,"every":2},"startMs":0,"durationMs":200}));
        assert_eq!(targets_of(&even["effectIds"]), vec![strings[0].clone(), strings[2].clone(), strings[4].clone(), strings[6].clone()]);
        let odd = run("effect_add", json!({"sequenceId":id,"typeId":pulse,"targetSlice":{"groupId":tree,"every":2,"offset":1},"startMs":200,"durationMs":200}));
        assert_eq!(targets_of(&odd["effectIds"]), vec![strings[1].clone(), strings[3].clone(), strings[5].clone(), strings[7].clone()]);
        let range = run("effect_add", json!({"sequenceId":id,"typeId":pulse,"targetSlice":{"groupId":tree,"strings":[3,5]},"startMs":400,"durationMs":200}));
        assert_eq!(targets_of(&range["effectIds"]), strings[2..5].to_vec());
        assert_eq!(core.studio_display.snapshot(&core)["revision"].clone(), revision, "slicing leaves the display alone");

        // Bad targets add nothing.
        let before = run("sequence_status", json!({"sequenceId":id}))["effects"].clone();
        assert!(fail("effect_add", json!({"sequenceId":id,"typeId":pulse,"targetId":strings[0],"targetIds":[strings[1]],"startMs":0,"durationMs":200})).contains("exactly one"));
        assert!(fail("effect_add", json!({"sequenceId":id,"typeId":pulse,"startMs":0,"durationMs":200})).contains("exactly one"));
        assert!(fail("effect_add", json!({"sequenceId":id,"typeId":pulse,"targetIds":[strings[1],strings[1]],"startMs":0,"durationMs":200})).contains("twice"));
        assert!(fail("effect_add", json!({"sequenceId":id,"typeId":pulse,"targetIds":[strings[1],Uuid::new_v4().to_string()],"startMs":0,"durationMs":200})).contains("not found"));
        assert!(fail("effect_add", json!({"sequenceId":id,"typeId":pulse,"targetSlice":{"groupId":tree,"strings":[5,9]},"startMs":0,"durationMs":200})).contains("has 8"));
        assert!(fail("effect_add", json!({"sequenceId":id,"typeId":pulse,"targetSlice":{"groupId":tree,"every":2,"offset":2},"startMs":0,"durationMs":200})).contains("offset"));
        assert_eq!(run("sequence_status", json!({"sequenceId":id}))["effects"], before);

        // Batch adds and bands take the same targets; effectIds follow op order, then target order.
        let batch = run("effects_batch", json!({"sequenceId":id,"ops":[
            {"op":"add","typeId":pulse,"targetSlice":{"groupId":tree,"strings":[1,2]},"startMs":1000,"durationMs":100},
            {"op":"band","targetId":tree,"startMs":1000,"durationMs":500,"fromPct":40,"toPct":60,"color":"#00ff00"},
            {"op":"band","targetIds":[strings[0],strings[1]],"startMs":2000,"durationMs":500,"fromPct":0,"toPct":25}]}));
        let ids = batch["effectIds"].as_array().unwrap();
        assert_eq!(ids.len(), 5);
        assert_eq!(targets_of(&batch["effectIds"]), vec![strings[0].clone(), strings[1].clone(), tree.as_str().unwrap().to_string(), strings[0].clone(), strings[1].clone()]);
        let band = run("effect_parameters", json!({"sequenceId":id,"effectId":ids[2]}))["parameters"].as_array().unwrap().clone();
        let param = |n: &str| band.iter().find(|p| p["name"] == n).unwrap()["value"].clone();
        assert_eq!(param("WipeMovement"), "Movement");
        assert_eq!(param("Direction"), "Vertical");
        assert!(fail("effects_batch", json!({"sequenceId":id,"ops":[{"op":"band","targetId":tree,"startMs":0,"durationMs":100,"fromPct":50,"toPct":50}]})).contains("ops[0]"));
        assert!(fail("effects_batch", json!({"sequenceId":id,"ops":[{"op":"band","typeId":pulse,"targetId":tree,"startMs":0,"durationMs":100,"fromPct":0,"toPct":50}]})).contains("typeId does not apply"));
        let single = run("effect_band", json!({"sequenceId":id,"targetId":tree,"startMs":3000,"durationMs":500,"fromPct":10,"toPct":30,"level":50}));
        assert_eq!(single["effectIds"].as_array().unwrap().len(), 1);
        run("sequence_render", json!({"sequenceId":id}));

        core.studio_display.stop();
        drop(core);
        let _ = std::fs::remove_dir_all(root);
    }

    /// Renders band_params on a megatree-like grid and checks that exactly the requested height range lights.
    #[test]
    fn band_lights_the_requested_rows_and_holds_still() {
        use vixen_core::element::{property_ids, PropertyData, PropertyInstance};
        let wipe = vixen_effect_registry::all().iter().find(|d| d.name == "Wipe").unwrap();
        assert_eq!(vixen_effect_registry::all().iter().filter(|d| d.name == "Wipe").count(), 1);
        for (pixels, spacing) in [(50usize, 2i32), (37, 3), (100, 1), (24, 5)] {
            let mut tree = vixen_core::ElementTree::new();
            let group = vixen_effects::testing::grid(&mut tree, "T", 6, pixels);
            let mut located = Vec::new();
            for (s, string) in tree.node(group).children.clone().into_iter().enumerate() {
                for (p, leaf) in tree.node(string).children.clone().into_iter().enumerate() {
                    // Pixel 0 at the top, as preview coordinates grow downward.
                    let (x, y) = (10 + s as i32 * 4, 20 + p as i32 * spacing);
                    tree.node_mut(leaf).properties.push(PropertyInstance { type_id: property_ids::LOCATION, instance_id: Uuid::new_v4(), data: PropertyData::Location { x, y, z: 0 } });
                    located.push((tree.node(leaf).element_id.unwrap(), y));
                }
            }
            let max_y = located.iter().map(|l| l.1).max().unwrap();
            let height = (max_y - 20) as f64;
            for (from, to) in [(40.0, 60.0), (0.0, 20.0), (80.0, 100.0), (10.0, 90.0), (0.0, 100.0), (45.0, 50.0)] {
                let values = parameter_values(wipe, &band_params(from, to, "#ff0000", 100.0)).unwrap();
                let out = vixen_effects::testing::render_with(wipe, &tree, &[group], 1000, |p| {
                    for (name, value) in values {
                        p.set(name, value);
                    }
                });
                let lit_at = |ms: f64| -> Vec<f64> {
                    let mut rows: Vec<f64> = located
                        .iter()
                        .filter(|(e, _)| out.nodes_for(e).is_some_and(|n| n.iter().any(|i| i.contains(TimeSpan::from_ms(ms)))))
                        .map(|(_, y)| (max_y - y) as f64 / height * 100.0)
                        .collect();
                    rows.sort_by(f64::total_cmp);
                    rows.dedup();
                    rows
                };
                let rows = lit_at(500.0);
                assert!(!rows.is_empty(), "{pixels}px {from}-{to}: nothing lit");
                // Edges land on the nearest preview row: within one coordinate unit plus one row spacing.
                let slack = (1.0 + spacing as f64) / height * 100.0 + 1e-9;
                assert!(rows[0] >= from - slack && rows[0] <= from + slack, "{pixels}px {from}-{to}: bottom {rows:?}");
                assert!(*rows.last().unwrap() <= to + slack && *rows.last().unwrap() >= to - slack, "{pixels}px {from}-{to}: top {rows:?}");
                assert_eq!(lit_at(10.0), rows, "{pixels}px {from}-{to}: the band holds still");
                assert_eq!(lit_at(990.0), rows, "{pixels}px {from}-{to}: the band holds still");
            }
        }
    }

    /// A 16-bit mono WAV of decaying sine bursts: 60 Hz kicks and 6 kHz hats.
    fn click_track(path: &std::path::Path, seconds: f64, kicks: &[f64], hats: &[f64]) {
        let sr = 44_100u32;
        let mut samples = vec![0.0f64; (f64::from(sr) * seconds) as usize];
        for (times, freq) in [(kicks, 60.0), (hats, 6000.0)] {
            for t in times {
                let start = (t / 1000.0 * f64::from(sr)) as usize;
                for i in 0..(f64::from(sr) * 0.08) as usize {
                    let Some(sample) = samples.get_mut(start + i) else { break };
                    let env = (-(i as f64) / (f64::from(sr) * 0.015)).exp();
                    *sample += 0.4 * env * (2.0 * std::f64::consts::PI * freq * i as f64 / f64::from(sr)).sin();
                }
            }
        }
        let data: Vec<u8> = samples.iter().flat_map(|v| ((v.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes()).collect();
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&sr.to_le_bytes());
        wav.extend_from_slice(&(sr * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
        wav.extend_from_slice(&data);
        std::fs::write(path, wav).unwrap();
    }

    #[test]
    fn marks_create_and_music_marks_undo_empties_added_collections() {
        let root = std::env::temp_dir().join(format!("studio-marks-undo-{}", Uuid::new_v4()));
        let core = AppCore::open(root.clone(), true).unwrap();
        std::fs::create_dir_all(core.paths.media_dir()).unwrap();
        let beats: Vec<f64> = (1..16).map(|i| i as f64 * 500.0).collect();
        click_track(&core.paths.media_dir().join("clicks.wav"), 8.0, &beats, &[]);
        let run = |name: &str, args: Value| execute(&core, name, &args, true).unwrap();
        let id = run("sequence_create", json!({"lengthMs":8000,"media":"clicks.wav"}))["sequenceId"].clone();
        let marks = || run("sequence_marks", json!({"sequenceId":id}));
        let empty = |v: &Value| v.as_array().unwrap().iter().all(|c| c["marks"].as_array().unwrap().is_empty());

        run("marks_create", json!({"sequenceId":id,"name":"Hits","timesMs":[100,200,300]}));
        let created = marks();
        assert!(!empty(&created));
        run("sequence_undo", json!({"sequenceId":id}));
        assert!(empty(&marks()), "{}", marks());
        run("sequence_redo", json!({"sequenceId":id}));
        assert_eq!(marks(), created, "redo puts every mark back");

        run("sequence_music_marks", json!({"sequenceId":id,"media":"clicks.wav"}));
        let music = marks();
        let music_count = |v: &Value| v.as_array().unwrap().iter().filter(|c| c["name"].as_str().unwrap().starts_with("Music")).map(|c| c["marks"].as_array().unwrap().len()).sum::<usize>();
        assert!(music_count(&music) > 0, "{music}");
        run("sequence_undo", json!({"sequenceId":id}));
        let undone = marks();
        assert_eq!(music_count(&undone), 0, "{undone}");
        assert_eq!(undone.as_array().unwrap().iter().find(|c| c["name"] == "Hits").unwrap()["marks"].as_array().unwrap().len(), 3, "earlier marks stay");
        run("sequence_redo", json!({"sequenceId":id}));
        assert_eq!(marks(), music, "redo puts every mark back");
        core.studio_display.stop();
        drop(core);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn onset_marks_then_effects_on_marks() {
        let root = std::env::temp_dir().join(format!("studio-onset-marks-{}", Uuid::new_v4()));
        let core = AppCore::open(root.clone(), true).unwrap();
        std::fs::create_dir_all(core.paths.media_dir()).unwrap();
        let kicks = [500.0, 1500.0, 2500.0, 3500.0];
        let hats = [1000.0, 2000.0, 3000.0];
        click_track(&core.paths.media_dir().join("clicks.wav"), 4.5, &kicks, &hats);
        let element = |name: &str| {
            let id = core.studio_display.execute(&core, "element_create", &json!({"name":name}), false).unwrap()["result"]["ids"][0].clone();
            core.studio_display.execute(&core, "element_color", &json!({"id":id,"mode":"rgb"}), false).unwrap();
            id
        };
        let (a, b) = (element("A"), element("B"));
        let run = |name: &str, args: Value| execute(&core, name, &args, true).unwrap();
        let fail = |name: &str, args: Value| execute(&core, name, &args, true).unwrap_err();
        let id = run("sequence_create", json!({"lengthMs":4500,"media":"clicks.wav"}))["sequenceId"].clone();
        let pulse = vixen_effect_registry::all().iter().find(|d| d.name == "Pulse").unwrap();
        let effects = || run("sequence_effects", json!({"sequenceId":id}))["effects"].as_array().unwrap().clone();

        // Onsets become one collection per band, from the sequence's own audio, each mark's text its strength.
        let made = run("sequence_onset_marks", json!({"sequenceId":id}));
        assert_eq!(made["collections"].as_array().unwrap().len(), 3, "{made}");
        let marks = run("sequence_marks", json!({"sequenceId":id}));
        let low = marks.as_array().unwrap().iter().find(|c| c["name"] == "Onsets - low").unwrap()["marks"].as_array().unwrap().clone();
        let near = |t: f64| low.iter().any(|m| (m["timeMs"].as_f64().unwrap() - t).abs() <= 12.0);
        assert!(kicks.iter().all(|t| near(*t)) && !hats.iter().any(|t| near(*t)), "{low:?}");
        assert!(low.iter().all(|m| m["text"].as_str().unwrap().parse::<f64>().is_ok_and(|s| s > 0.0 && s <= 1.0)));
        run("sequence_undo", json!({"sequenceId":id}));
        // Undo never removes a collection (as with the native panel), so it empties the new ones in one step.
        let undone = run("sequence_marks", json!({"sequenceId":id}));
        assert!(undone.as_array().unwrap().iter().all(|c| c["marks"].as_array().unwrap().is_empty()), "{undone}");
        run("sequence_redo", json!({"sequenceId":id}));
        assert_eq!(run("sequence_marks", json!({"sequenceId":id})), marks, "redo puts every mark back");

        // Snapping needs a beat grid; the error says how to get one.
        let template = json!({"typeId":pulse.type_id.to_string(),"targetId":a});
        assert!(fail("effects_on_marks", json!({"sequenceId":id,"markCollection":"Onsets - low","snap":"8th","template":template})).contains("sequence_music_marks"));
        assert!(fail("effects_on_marks", json!({"sequenceId":id,"markCollection":"Nope","template":template})).contains("\"Onsets - mid\""));
        run("marks_create", json!({"sequenceId":id,"name":"Music - All","timesMs":[500,1000,1500,2000,2500,3000,3500,4000]}));

        // One Pulse per kick, snapped onto the 8th grid, lasting 80% of the gap, level scaled by strength.
        let placed = run("effects_on_marks", json!({"sequenceId":id,"markCollection":"Onsets - low","snap":"8th","template":template,"levelFromStrength":true}));
        assert_eq!(placed["effectIds"].as_array().unwrap().len(), 4, "{placed}");
        let list = effects();
        let starts: Vec<f64> = list.iter().map(|e| e["startMs"].as_f64().unwrap()).collect();
        assert_eq!(starts, kicks);
        assert!(list.iter().all(|e| (e["durationMs"].as_f64().unwrap() - 800.0).abs() < 0.01), "{list:?}");
        let default_peak = match (pulse.params.iter().find(|p| p.name == "LevelCurve").unwrap().default)() {
            ParamValue::Curve(c) => c.points.iter().map(|p| p.1).fold(0.0, f64::max),
            _ => unreachable!(),
        };
        for (effect, mark) in list.iter().zip(low.iter().filter(|m| kicks.iter().any(|k| (m["timeMs"].as_f64().unwrap() - k).abs() <= 12.0))) {
            let curve = run("effect_parameters", json!({"sequenceId":id,"effectId":effect["id"]}))["parameters"].as_array().unwrap().iter().find(|p| p["name"] == "LevelCurve").unwrap()["value"].clone();
            let peak = curve.as_array().unwrap().iter().map(|p| p[1].as_f64().unwrap()).fold(0.0, f64::max);
            let strength: f64 = mark["text"].as_str().unwrap().parse().unwrap();
            assert!((peak - default_peak * strength).abs() < 0.01, "peak {peak} for strength {strength}");
        }
        run("sequence_undo", json!({"sequenceId":id}));
        assert!(effects().is_empty(), "one undo step removes every placed effect");

        // range and minStrength limit the marks (a faint ghost trails one kick); hand-made marks: minStrength, non-numeric text counts as 1, targets and colours rotate.
        let ranged = run("effects_on_marks", json!({"sequenceId":id,"markCollection":"Onsets - low","range":[1000,4500],"minStrength":0.2,"template":template}));
        assert_eq!(ranged["effectIds"].as_array().unwrap().len(), 3, "{low:?}");
        run("sequence_undo", json!({"sequenceId":id}));
        run("marks_create", json!({"sequenceId":id,"name":"Hits","timesMs":[100,200,260,1200],"texts":["0.9","0.2","accent","0.5"]}));
        let rotating = json!({"typeId":pulse.type_id.to_string(),"targetIds":[a,b],"rotateTargets":true});
        let cycle = json!({"param":"ColorGradient","values":[["#ff0000"],["#0000ff"]]});
        let hits = run("effects_on_marks", json!({"sequenceId":id,"markCollection":"Hits","minStrength":0.3,"duration":50,"template":rotating,"cycle":cycle}));
        assert_eq!(hits["skipped"]["belowMinStrength"], 1, "{hits}");
        let list = effects();
        let summary: Vec<(f64, f64, String)> = list.iter().map(|e| (e["startMs"].as_f64().unwrap(), e["durationMs"].as_f64().unwrap(), e["targets"][0].as_str().unwrap().to_string())).collect();
        let (a, b) = (a.as_str().unwrap().to_string(), b.as_str().unwrap().to_string());
        assert_eq!(summary, vec![(100.0, 50.0, a.clone()), (260.0, 50.0, b), (1200.0, 50.0, a)]);
        let color = |e: &Value| run("effect_parameters", json!({"sequenceId":id,"effectId":e["id"]}))["parameters"].as_array().unwrap().iter().find(|p| p["name"] == "ColorGradient").unwrap()["value"].clone();
        assert_ne!(color(&list[0]), color(&list[1]));
        assert_eq!(color(&list[0]), color(&list[2]));
        assert!(fail("effects_on_marks", json!({"sequenceId":id,"markCollection":"Hits","template":template,"cycle":{"param":"Nope","values":[1]}})).starts_with("cycle[0]"));
        run("sequence_undo", json!({"sequenceId":id}));

        // dryRun counts without adding (the Place Effect on Marks sheet's summary); cycleColors finds the colour parameter itself.
        let undo_before = run("sequence_status", json!({"sequenceId":id}))["undo"].clone();
        let dry = run("effects_on_marks", json!({"sequenceId":id,"markCollection":"Hits","minStrength":0.3,"duration":50,"template":rotating,"cycleColors":["#ff0000","#0000ff"],"dryRun":true}));
        assert_eq!((dry["dryRun"].clone(), dry["effects"].clone(), dry["marksUsed"].clone(), dry["skipped"]["belowMinStrength"].clone()), (json!(true), json!(3), json!(3), json!(1)), "{dry}");
        assert!(effects().is_empty() && run("sequence_status", json!({"sequenceId":id}))["undo"] == undo_before, "a dry run changes nothing");
        run("effects_on_marks", json!({"sequenceId":id,"markCollection":"Hits","minStrength":0.3,"duration":50,"template":rotating,"cycleColors":["#ff0000","#0000ff"]}));
        let list = effects();
        assert_eq!(color(&list[0]), color(&list[2]));
        assert_ne!(color(&list[0]), color(&list[1]));
        assert_eq!(color(&list[1])["colors"][0]["color"], "#0000ff", "{}", color(&list[1]));
        assert!(fail("effects_on_marks", json!({"sequenceId":id,"markCollection":"Hits","template":template,"cycle":cycle,"cycleColors":["#ff0000"]})).contains("use one"));
        assert!(fail("effects_on_marks", json!({"sequenceId":id,"markCollection":"Hits","template":template,"cycleColors":["#zzzzzz"]})).starts_with("cycleColors[0]"));
        core.studio_display.stop();
        drop(core);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn effect_step_walks_alternates_stacks_and_jumps_in_one_undo_step() {
        let root = std::env::temp_dir().join(format!("studio-step-{}", Uuid::new_v4()));
        let core = AppCore::open(root.clone(), true).unwrap();
        let tree = core.studio_display
            .execute(&core, "element_template", &json!({"kind":"megatree","name":"Tree","count":8,"pixels":10}), false)
            .unwrap()["result"]["id"]
            .clone();
        let strings: Vec<String> = {
            let system = core.system.read();
            let node = system.tree.find(&Uuid::parse_str(tree.as_str().unwrap()).unwrap()).unwrap();
            system.tree.node(node).children.iter().map(|c| system.tree.node(*c).id.to_string()).collect()
        };
        let run = |name: &str, args: Value| execute(&core, name, &args, true).unwrap();
        let fail = |name: &str, args: Value| execute(&core, name, &args, true).unwrap_err();
        let id = run("sequence_create", json!({"lengthMs":10_000}))["sequenceId"].clone();
        let effects = || run("sequence_effects", json!({"sequenceId":id}))["effects"].as_array().unwrap().clone();
        let summary = || -> Vec<(f64, f64, String)> {
            effects().iter().map(|e| (e["startMs"].as_f64().unwrap(), e["durationMs"].as_f64().unwrap(), e["targets"][0].as_str().unwrap().to_string())).collect()
        };
        let param = |effect: &Value, name: &str| run("effect_parameters", json!({"sequenceId":id,"effectId":effect["id"]}))["parameters"].as_array().unwrap().iter().find(|p| p["name"] == name).unwrap()["value"].clone();

        // walk: one string per hit around the tree, 80% of each gap lit, colours cycling, Set Level effects.
        let around = json!({"axis":"around","groupId":tree});
        let walk = run("effect_step", json!({"sequenceId":id,"mode":"walk","segments":around,"stepsMs":[1000,1500,2000,2500],"colors":["#ff0000","#00ff00"]}));
        assert_eq!(walk["steps"], 4, "{walk}");
        assert_eq!(walk["endMs"], 3000.0, "without cutAtMs the last hit lasts the median gap");
        assert_eq!(summary(), vec![(1000.0, 400.0, strings[0].clone()), (1500.0, 400.0, strings[1].clone()), (2000.0, 400.0, strings[2].clone()), (2500.0, 400.0, strings[3].clone())]);
        let list = effects();
        let set_level = vixen_effect_registry::all().iter().find(|d| d.name == "Set Level").unwrap().type_id.to_string();
        assert!(list.iter().all(|e| e["typeId"] == set_level));
        assert_eq!(param(&list[0], "Color"), "#ff0000");
        assert_eq!(param(&list[1], "Color"), "#00ff00");
        assert_eq!(param(&list[2], "Color"), "#ff0000");
        assert_eq!(run("sequence_status", json!({"sequenceId":id}))["undo"][0], "Step effect (Agent)");
        run("sequence_undo", json!({"sequenceId":id}));
        assert!(effects().is_empty(), "one undo removes the whole figure");

        // walk reversed, two of four segments (two strings each), cut short.
        run("effect_step", json!({"sequenceId":id,"mode":"walk","segments":{"axis":"around","groupId":tree,"count":4,"width":2},"reverse":true,"stepsMs":[0,1000,2000],"cutAtMs":2500,"holdFraction":1}));
        let lit: Vec<(f64, Vec<String>)> = [0.0, 1000.0, 2000.0].iter().map(|t| (*t, summary().into_iter().filter(|e| e.0 == *t).map(|e| e.2).collect())).collect();
        let s = |i: usize| strings[i].clone();
        assert_eq!(lit[0].1, vec![s(0), s(1), s(2), s(3)], "segments 0-1 = strings 1-4");
        assert_eq!(lit[1].1, vec![s(0), s(1), s(6), s(7)], "reverse steps to segment 3, wrapping to 0");
        assert_eq!(lit[2].1, vec![s(4), s(5), s(6), s(7)]);
        assert!(summary().iter().filter(|e| e.0 == 2000.0).all(|e| e.1 == 500.0), "the last hit ends at cutAtMs");
        run("sequence_undo", json!({"sequenceId":id}));

        // alternate halves on marks; minStrength skips a weak mark.
        run("marks_create", json!({"sequenceId":id,"name":"Stabs","timesMs":[1000,1250,1500,1750],"texts":["1","0.1","1","1"]}));
        let alt = run("effect_step", json!({"sequenceId":id,"mode":"alternate","segments":{"axis":"around","groupId":tree,"count":2},"markCollection":"Stabs","minStrength":0.5,"cutAtMs":2000}));
        assert_eq!(alt["skipped"]["belowMinStrength"], 1, "{alt}");
        let front: Vec<String> = strings[..4].to_vec();
        let at = |t: f64| -> Vec<String> { summary().into_iter().filter(|e| e.0 == t).map(|e| e.2).collect() };
        assert_eq!(at(1000.0), front);
        assert_eq!(at(1500.0), strings[4..].to_vec());
        assert_eq!(at(1750.0), front);
        run("sequence_undo", json!({"sequenceId":id}));

        // stack: vertical bands accumulate bottom to top and all cut together, then start again.
        let stack = run("effect_step", json!({"sequenceId":id,"mode":"stack","segments":{"axis":"vertical","targetId":tree,"count":4},"stepsMs":[0,500,1000,1500,2000],"cutAtMs":2500,"holdFraction":1,"colors":["#ffffff","#0000ff"]}));
        assert_eq!(stack["effectIds"].as_array().unwrap().len(), 5, "{stack}");
        let list = effects();
        let wipe = vixen_effect_registry::all().iter().find(|d| d.name == "Wipe").unwrap().type_id.to_string();
        assert!(list.iter().all(|e| e["typeId"] == wipe && e["targets"][0] == tree));
        let spans: Vec<(f64, f64)> = list.iter().map(|e| (e["startMs"].as_f64().unwrap(), e["startMs"].as_f64().unwrap() + e["durationMs"].as_f64().unwrap())).collect();
        assert_eq!(spans, vec![(0.0, 2000.0), (500.0, 2000.0), (1000.0, 2000.0), (1500.0, 2000.0), (2000.0, 2500.0)]);
        let pulse_width: Vec<f64> = list.iter().map(|e| param(e, "PulsePercent").as_f64().unwrap()).collect();
        assert_eq!(pulse_width, vec![25.0; 5]);
        let positions: Vec<f64> = list.iter().map(|e| param(e, "MovementCurve")[0][1].as_f64().unwrap()).collect();
        let expected: Vec<f64> = [25.0, 50.0, 75.0, 100.0, 25.0].iter().map(|to| 100.0 * to / (100.0 + 25.0)).collect();
        assert!(positions.iter().zip(&expected).all(|(p, e)| (p - e).abs() < 0.01), "{positions:?}");
        assert_eq!(param(&list[1], "ColorGradient"), param(&list[3], "ColorGradient"));
        assert_ne!(param(&list[0], "ColorGradient"), param(&list[1], "ColorGradient"));
        run("sequence_render", json!({"sequenceId":id}));
        run("sequence_undo", json!({"sequenceId":id}));

        // jump: seeded, two strings per hit, never the previous hit's strings.
        let args = json!({"sequenceId":id,"mode":"jump","segments":{"axis":"around","groupId":tree,"width":2},"stepsMs":[0,200,400,600,800,1000],"seed":3});
        run("effect_step", args.clone());
        let first = summary();
        run("sequence_undo", json!({"sequenceId":id}));
        run("effect_step", args);
        assert_eq!(summary(), first, "the same seed gives the same figure");
        for t in [200.0, 400.0, 600.0, 800.0, 1000.0] {
            let now = at(t);
            assert_eq!(now.len(), 2);
            assert!(now.iter().all(|s| !at(t - 200.0).contains(s)), "{t}: {now:?}");
        }
        run("sequence_undo", json!({"sequenceId":id}));

        // Bad arguments add nothing.
        assert!(fail("effect_step", json!({"sequenceId":id,"mode":"walk","segments":around})).contains("exactly one of markCollection or stepsMs"));
        assert!(fail("effect_step", json!({"sequenceId":id,"mode":"walk","segments":around,"stepsMs":[100]})).contains("cutAtMs"));
        assert!(fail("effect_step", json!({"sequenceId":id,"mode":"jump","segments":{"axis":"around","groupId":tree,"width":5},"stepsMs":[0,100]})).contains("1..4"));
        assert!(fail("effect_step", json!({"sequenceId":id,"mode":"alternate","segments":{"axis":"around","groupId":tree,"width":2},"stepsMs":[0,100]})).contains("width does not apply"));
        assert!(fail("effect_step", json!({"sequenceId":id,"mode":"stack","segments":{"axis":"vertical","targetId":tree},"stepsMs":[0,100]})).contains("count is required"));
        assert!(fail("effect_step", json!({"sequenceId":id,"mode":"walk","segments":{"axis":"vertical","groupId":tree,"count":4},"stepsMs":[0,100]})).contains("groupId does not apply"));
        assert!(fail("effect_step", json!({"sequenceId":id,"mode":"walk","segments":{"axis":"around","groupId":tree,"count":9},"stepsMs":[0,100]})).contains("2..8"));
        assert!(fail("effect_step", json!({"sequenceId":id,"mode":"walk","segments":around,"stepsMs":[0,100],"colors":["red"]})).contains("#RRGGBB"));
        assert!(effects().is_empty());
        core.studio_display.stop();
        drop(core);
        let _ = std::fs::remove_dir_all(root);
    }
}
