use crate::{
    app::AppCore,
    setup::{flow::CompKind, SetupSession},
};
use serde_json::{json, Value};
pub fn report(setup: &SetupSession, core: Option<&AppCore>) -> Value {
    let unpatched = setup
        .flow
        .components()
        .filter(|(id, c)| c.kind == CompKind::Element && setup.flow.count_patch_points(*id).0 == 0)
        .count();
    let unused = setup
        .flow
        .components()
        .filter(|(_, c)| c.kind == CompKind::Output && c.source.is_none())
        .count();
    let mut conflicts = Vec::new();
    let mut invalid = Vec::new();
    for (i, a) in setup.controllers.iter().enumerate() {
        if let Some(vixen_output::ControllerConfig::E131(config)) = &a.config {
            for message in config.validate(a.outputs.len()) {
                invalid.push(json!({"controllerId":a.id.to_string(),"message":message}));
            }
            for b in setup.controllers.iter().skip(i + 1) {
                if let Some(vixen_output::ControllerConfig::E131(other)) = &b.config {
                    if let Some(universe) = config.conflicts_with(other) {
                        conflicts.push(json!({"first":a.id.to_string(),"second":b.id.to_string(),"universe":universe}));
                    }
                }
            }
        }
    }
    let mut media = Vec::new();
    let mut colors = Vec::new();
    let mut missing_targets = Vec::new();
    if let Some(core) = core {
        for shared in core.open_sequence_sessions() {
            let session = shared.lock();
            for item in &session.document().media {
                if !core.paths.media_dir().join(&item.file_name).is_file() {
                    media.push(json!({"sequenceId":session.playback_id().to_string(),"media":item.file_name}));
                }
            }
            for effect in session.effects() {
                if effect.targets.is_empty() {
                    missing_targets.push(json!({"sequenceId":session.playback_id().to_string(),"effectId":effect.id.0}));
                }
                if let Some(validate) = effect.descriptor.and_then(|d| d.validate) {
                    let valid = session.tree().valid_colors_for(&effect.targets);
                    if !valid.is_empty() {
                        let mut corrected = effect.node.params.clone();
                        validate(&mut corrected, &valid);
                        if corrected.values != effect.node.params.values {
                            colors.push(json!({"sequenceId":session.playback_id().to_string(),"effectId":effect.id.0,"message":"Effect's native validator suggests a color correction for these targets"}));
                        }
                    }
                }
            }
        }
    }
    json!({"unpatchedElements":unpatched,"unusedOutputs":unused,"invalidPatches":setup.warnings.iter().map(|w|format!("{w:?}")).collect::<Vec<_>>(),"overlappingUniverses":conflicts,"controllerConfigurationErrors":invalid,"missingMedia":media,"colorCompatibility":colors,"missingEffectTargets":missing_targets,"sequenceScope":"Open sequences only; color checks use each effect's native validator"})
}
pub fn monitor(core: &AppCore) -> Value {
    let stats = core.playback.stats();
    json!({"execution":core.execution().state().name(),"frames":stats.frames,"lateFrames":stats.late_frames,"lastFrameUs":stats.last_frame_us,"maxFrameUs":stats.max_frame_us,"controllers":core.playback.controller_states().iter().map(|(id,running,paused)|json!({"id":id.to_string(),"running":running,"paused":paused})).collect::<Vec<_>>()})
}

pub fn recent_logs(root: &std::path::Path, limit: usize) -> Result<Value, String> {
    use std::io::{Read, Seek, SeekFrom};
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let directory = root.join("logs");
    if !directory.exists() {
        return Ok(json!({"lines":[],"note":"No profile logs"}));
    }
    let directory = directory.canonicalize().map_err(|e| e.to_string())?;
    if !directory.starts_with(&root) {
        return Err("Log directory escapes profile".into());
    }
    let mut files = Vec::new();
    for entry in std::fs::read_dir(&directory).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let metadata = entry.metadata().map_err(|e| e.to_string())?;
        if metadata.is_file() && !entry.file_type().map_err(|e| e.to_string())?.is_symlink() {
            files.push((metadata.modified().ok(), entry.path()));
        }
    }
    files.sort_by(|a, b| b.0.cmp(&a.0));
    let Some((_, path)) = files.first() else {
        return Ok(json!({"lines":[]}));
    };
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let len = file.metadata().map_err(|e| e.to_string())?.len();
    let offset = len.saturating_sub(65536);
    file.seek(SeekFrom::Start(offset))
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(65536)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&bytes);
    let mut lines: Vec<_> = text.lines().collect();
    if offset > 0 && !lines.is_empty() {
        lines.remove(0);
    }
    let lines: Vec<_> = lines
        .into_iter()
        .rev()
        .take(limit.min(500))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    Ok(json!({"file":path.file_name().map(|s|s.to_string_lossy()),"lines":lines,"tailBytes":65536}))
}

pub fn controller_configuration(c: &crate::setup::controllers::ControllerEntry) -> Value {
    match &c.config {
        Some(vixen_output::ControllerConfig::Ddp(v)) => {
            json!({"address":v.address,"hostName":v.host_name})
        }
        Some(vixen_output::ControllerConfig::E131(v)) => {
            json!({"unicast":v.unicast,"multicast":v.multicast,"priority":v.priority,"universes":v.universes.iter().flatten().map(|u|json!({"universe":u.universe,"startOutput":u.start,"size":u.size,"active":u.active,"unicast":u.unicast})).collect::<Vec<_>>()})
        }
        _ => json!({"nativeEditorRequired":true}),
    }
}
