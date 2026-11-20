//! Shared Studio sequence authoring, analysis and export commands. No transport or hardware startup.
use crate::{
    app::AppCore,
    sequence::{EffectId, SequenceSession},
    undo::{AddRemoveKind, MoveKind, UndoAction},
};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc},
};
use uuid::Uuid;
use vixen_core::{marks::Mark, MarkCollection, TimeSpan};

#[path = "studio_onsets.rs"]
mod onsets;

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
        std::fs::create_dir(&candidate).map_err(|e| e.to_string())?;
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
fn analyze(core: &AppCore, a: &Value) -> Result<(Value, Vec<MarkCollection>), String> {
    let name = filename(a, "media")?;
    let path = core.paths.media_dir().join(&name);
    if std::fs::metadata(&path).map_err(|e| e.to_string())?.len() > 100 * 1024 * 1024 {
        return Err("Audio analysis input limit is 100 MiB".into());
    }
    let audio = vixen_audio::AudioClip::load(&path).map_err(|e| e.to_string())?;
    let duration = audio.duration().total_ms();
    if duration > 900_000.0 {
        return Err("Analysis is limited to 15 minutes".into());
    }
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
        let mut session = if name == "sequence_open" {
            let file = filename(a, "file")?;
            if !file.to_lowercase().ends_with(".tim") {
                return Err("Sequence must be a .tim file".into());
            }
            let path = output(core, "Sequence", &file)?;
            // Compare canonical paths, as the app's own open does, so a file the app opened by another spelling is reused.
            let wanted = canonical(&path);
            if let Some(existing) = core
                .open_sequence_sessions()
                .into_iter()
                .find(|s| s.lock().path().is_some_and(|p| canonical(p) == wanted))
            {
                return Ok(document_json(&existing.lock(), true));
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
        let result = document_json(&session, false);
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
    if name == "sequence_music_marks" {
        let (analysis, marks) = analyze(core, a)?;
        let mut s = shared.lock();
        let old = s.marks().to_vec();
        let mut combined = old.clone();
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
        "effect_add" => {
            let target = uuid(a, "targetId")?;
            let type_id = uuid(a, "typeId")?;
            let start = ms(a, "startMs")?;
            let duration = ms(a, "durationMs")?;
            if duration < 1.0 || start + duration > s.length().total_ms() {
                return Err("Effect must fit within sequence and have positive duration".into());
            }
            let target_name = {
                let system = core.system.read();
                system
                    .tree
                    .find(&target)
                    .map(|n| system.tree.node(n).name.clone())
                    .ok_or("Target not found")?
            };
            let id = s
                .add_new_effect(
                    type_id,
                    target,
                    &target_name,
                    TimeSpan::from_ms(start),
                    TimeSpan::from_ms(duration),
                )
                .ok_or("Effect type not found")?;
            record(
                &mut s,
                "Add effect",
                UndoAction::EffectsAdded {
                    kind: AddRemoveKind::AddedOrRemoved,
                    ids: vec![id],
                },
                agent,
            );
            json!({"effectId":id.0})
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
        "effect_parameters"=>{let effect=effect_id(&s,a)?;let entry=s.effect(effect).unwrap();let desc=entry.descriptor.ok_or("Unknown effect type")?;json!({"parameters":desc.params.iter().map(|p|{let mut o=json!({"name":p.name,"value":crate::studio_params::encode(entry.node.params.get(p.name)),"schema":crate::studio_params::schema(p.kind)});if let Some(n)=crate::studio_params::note(desc.name,p.name){o["note"]=json!(n);}o}).collect::<Vec<_>>()})},
        "effect_set_parameter" => {
            let effect = effect_id(&s, a)?;
            let parameter = string(a, "parameter")?;
            let entry = s.effect(effect).unwrap();
            let descriptor = entry.descriptor.ok_or("Unknown effect descriptor")?;
            let param = descriptor
                .params
                .iter()
                .find(|p| p.name == parameter)
                .ok_or("Unknown parameter")?;
            if matches!(param.editor, vixen_effects::framework::Editor::Path(_)) {
                return Err("File-path parameters require a future profile-media selector".into());
            }
            let value = crate::studio_params::decode(param.kind, &a["value"])?;
            if let Some(number) = a["value"].as_f64() {
                use vixen_effects::framework::Editor;
                match param.editor {
                    Editor::Number { min, max, .. }
                    | Editor::Slider { min, max, .. }
                    | Editor::SliderDouble { min, max, .. } => {
                        if number < min || number > max {
                            return Err(format!("Parameter must be within {min}..{max}"));
                        }
                    }
                    _ => {
                        if number.abs() > 100000.0 {
                            return Err("Numeric parameter safety limit is 100000".into());
                        }
                    }
                }
            }
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
            let old = s.marks().to_vec();
            let mut collections = old.clone();
            let mut collection = MarkCollection::default();
            collection.collection_type=match a["collectionType"].as_str().unwrap_or("generic"){"generic"=>vixen_core::marks::MarkCollectionType::Generic,"phrase"=>vixen_core::marks::MarkCollectionType::Phrase,"word"=>vixen_core::marks::MarkCollectionType::Word,"phoneme"=>vixen_core::marks::MarkCollectionType::Phoneme,"state"=>vixen_core::marks::MarkCollectionType::State,_=>return Err("Invalid collectionType".into())};
            collection.name = string(a, "name")?.into();
            collection.marks = marks;
            collection.sort_marks();
            let id = collection.id;
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
            let path = output(core, "Sequence", &file)?;
            s.save(Some(&path)).map_err(|e| e.to_string())?;
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
        ("music_analyze","Analyze actual profile media with Vixen's beat/bar tracker and per-band spectral-flux onsets; returns millisecond beats, bars, tempo, onsets with bar/16th slots and a per-bar slot-strength grid. Assumes 4/4; no playback.",json!({"media":string}),vec!["media"],true),
        ("sequence_create","Create a new in-memory sequence with optional profile audio; never plays sound. Its sequenceId changes to the file's after sequence_save (the old one keeps working).",json!({"lengthMs":number,"media":string}),vec!["lengthMs"],false),
        ("sequence_open","Open a profile Sequence filename. sequenceId is derived from the file, so every connection gets the same ID. alreadyOpen:true means this Studio session already had the file open (in the app or by an agent) and you are attached to that same document and undo stack. sharedWithApp:false means a headless copy the app does not see. Returns file, path and modified.",json!({"file":string}),vec!["file"],false),
        ("sequence_status","Shared document file, path, dirty/render/undo status and sharedWithApp.",json!({"sequenceId":string}),vec!["sequenceId"],true),
        ("sequence_effects","List up to 10000 effects with stable in-session IDs, target UUIDs and millisecond times.",json!({"sequenceId":string}),vec!["sequenceId"],true),
        ("effect_add","Add an undoable effect to a target UUID.",json!({"sequenceId":string,"typeId":string,"targetId":string,"startMs":number,"durationMs":number}),vec!["sequenceId","typeId","targetId","startMs","durationMs"],false),
        ("effect_delete","Delete an effect; requires confirmation.",json!({"sequenceId":string,"effectId":{"type":"integer","minimum":0},"confirm":boolean}),vec!["sequenceId","effectId","confirm"],false),
        ("effect_timing","Move/resize an effect in milliseconds, in one undo step.",json!({"sequenceId":string,"effectId":{"type":"integer","minimum":0},"startMs":number,"durationMs":number}),vec!["sequenceId","effectId","startMs","durationMs"],false),
        ("sequence_marks","List mark collections and timestamps.",json!({"sequenceId":string}),vec!["sequenceId"],true),
        ("marks_create","Create undoable timestamp marks, optionally authored phrase/word/phoneme text. No automatic speech recognition.",json!({"sequenceId":string,"name":string,"timesMs":{"type":"array","items":number,"maxItems":10000},"texts":{"type":"array","items":string,"maxItems":10000},"collectionType":{"type":"string","enum":["generic","phrase","word","phoneme","state"]}}),vec!["sequenceId","name","timesMs"],false),
        ("sequence_music_marks","Analyze profile media and append beat/bar collections to the sequence in one undo step.",json!({"sequenceId":string,"media":string}),vec!["sequenceId","media"],false),
        ("sequence_undo","Undo one shared sequence edit.",json!({"sequenceId":string}),vec!["sequenceId"],false),
        ("sequence_redo","Redo one shared sequence edit.",json!({"sequenceId":string}),vec!["sequenceId"],false),
        ("sequence_render","Render dirty effects and wait for the real sequence schedule; no playback/output.",json!({"sequenceId":string}),vec!["sequenceId"],false),
        ("sequence_save","Save/overwrite a .tim filename under profile Sequence. Requires confirmation.",json!({"sequenceId":string,"file":string,"confirm":boolean}),vec!["sequenceId","file","confirm"],false),
        ("sequence_export_fseq","Render and export uncompressed FSEQ v2 at 50 ms under profile Export. Requires confirmation; no network transfer.",json!({"sequenceId":string,"file":string,"confirm":boolean}),vec!["sequenceId","file","confirm"],false),
    ];
    entries.into_iter().map(|(name,description,properties,required,read_only)|json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false},"annotations":{"readOnlyHint":read_only,"destructiveHint":matches!(name,"effect_delete"|"layer_delete"|"sequence_save"|"sequence_export_fseq"|"preview_frame"|"sequence_play"|"sequence_seek"),"openWorldHint":matches!(name,"sequence_play"|"sequence_seek")}})).collect()
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
}
