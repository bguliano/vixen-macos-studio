//! Explicit local permission gate. MCP cannot grant itself permission or open execution.
use crate::app::AppCore;
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use uuid::Uuid;
pub fn tools() -> Vec<Value> {
    [
        ("output_start","Start one E1.31/DDP controller only after execution is opened locally and the local Allow agents to control live output setting is enabled.",json!({"controllerId":{"type":"string"},"confirm":{"type":"boolean"}}),vec!["controllerId","confirm"]),
        ("output_stop","Stop one controller. Requires explicit confirmation. Can stop even if agent output permission has been revoked.",json!({"controllerId":{"type":"string"},"confirm":{"type":"boolean"}}),vec!["controllerId","confirm"]),
        ("output_test_pattern","Render a temporary Pulse on one target for 25..5000ms. Requires local agent-output permission, already-open execution and explicit confirmation.",json!({"targetId":{"type":"string"},"durationMs":{"type":"integer","minimum":25,"maximum":5000},"confirm":{"type":"boolean"}}),vec!["targetId","durationMs","confirm"]),
    ].into_iter().map(|(name,description,properties,required)|json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":true,"openWorldHint":true}})).collect()
}
pub fn execute(core: &AppCore, name: &str, a: &Value) -> Result<Value, String> {
    let tool = tools()
        .into_iter()
        .find(|t| t["name"] == name)
        .ok_or("Unknown live command")?;
    crate::studio_schema::arguments(&tool["inputSchema"], a)?;
    let obj = a.as_object().ok_or("arguments must be object")?;
    if obj
        .keys()
        .any(|k| tool["inputSchema"]["properties"].get(k).is_none())
    {
        return Err("Unknown live argument".into());
    }
    let _guard = core.studio_display.sequence_gate.lock();
    if core.studio_display.is_stopped() {
        return Err("Profile is closing".into());
    }
    if a["confirm"] != true {
        return Err("Explicit confirm:true is required".into());
    }
    if name != "output_stop" {
        if !core.studio_display.allow_live.load(Ordering::Acquire) {
            return Err("Allow agents to control live output is OFF; enable it locally".into());
        }
        if !core.playback.hardware_allowed() || std::env::var("VIXEN_MUTE").is_ok_and(|v| v == "1")
        {
            return Err("Live output is forbidden in muted/headless mode".into());
        }
        if !core.execution().is_open() {
            return Err(
                "Open execution locally first; agents cannot implicitly start all controllers"
                    .into(),
            );
        }
    }
    let result = if name == "output_test_pattern" {
        let target = a["targetId"]
            .as_str()
            .and_then(|s| Uuid::parse_str(s).ok())
            .ok_or("Invalid targetId")?;
        let duration = a["durationMs"]
            .as_u64()
            .filter(|n| (25..=5000).contains(n))
            .ok_or("durationMs must be 25..5000")?;
        let effect = vixen_effect_registry::all()
            .iter()
            .find(|d| d.name == "Pulse")
            .ok_or("Pulse unavailable")?;
        core.live.render(
            core,
            "studio-agent-test",
            target,
            effect,
            effect.new_params(),
            vixen_core::TimeSpan::from_ms(duration as f64),
        )?;
        json!({"durationMs":duration,"targetId":target.to_string()})
    } else {
        let id = a["controllerId"]
            .as_str()
            .and_then(|s| Uuid::parse_str(s).ok())
            .ok_or("Invalid controllerId")?;
        let system = core.system.read();
        let c = system
            .controllers
            .iter()
            .find(|c| c.id == id)
            .ok_or("Controller not found")?;
        if name == "output_start"
            && c.hardware_type_id != vixen_output::ids::E131
            && c.hardware_type_id != vixen_output::ids::DDP
        {
            return Err("Agent startup is limited to E1.31/DDP controllers".into());
        }
        drop(system);
        core.playback
            .set_controller_running(&id, name == "output_start");
        if name == "output_start" {
            core.studio_display.agent_started.lock().insert(id);
        } else {
            core.studio_display.agent_started.lock().remove(&id);
        }
        if name == "output_stop" {
            core.live.release("studio-agent-test");
        }
        json!({"controllerId":id.to_string(),"running":core.playback.running_controllers().is_some_and(|ids|ids.contains(&id))})
    };
    drop(_guard);
    core.studio_display.activity("outputChanged", name);
    Ok(result)
}

pub fn allowed(core: &AppCore) -> bool {
    core.studio_display.allow_live.load(Ordering::Acquire)
}
pub fn set_allowed_locally(core: &AppCore, allowed: bool) {
    let _guard = core.studio_display.sequence_gate.lock();
    core.studio_display
        .allow_live
        .store(allowed, Ordering::Release);
    if !allowed {
        for id in core.studio_display.agent_started.lock().drain() {
            core.playback.set_controller_running(&id, false);
        }
        core.live.release("studio-agent-test");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_opt_in_cannot_override_muted_output() {
        let root = std::env::temp_dir().join(format!("studio-live-gate-{}", Uuid::new_v4()));
        let core = AppCore::open(root.clone(), true).unwrap();
        let args = json!({"controllerId":Uuid::new_v4().to_string(),"confirm":true});
        assert!(!allowed(&core));
        assert!(execute(&core, "output_start", &args)
            .unwrap_err()
            .contains("OFF"));
        set_allowed_locally(&core, true);
        assert!(execute(&core, "output_start", &args)
            .unwrap_err()
            .contains("muted"));
        set_allowed_locally(&core, false);
        drop(core);
        std::fs::remove_dir_all(root).unwrap();
    }
}
