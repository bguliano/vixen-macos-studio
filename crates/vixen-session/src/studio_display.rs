//! Profile-wide live display transactions. UI and agents call this same service.
use crate::{app::AppCore, setup::SetupSession};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::{collections::VecDeque, sync::Arc};
use uuid::Uuid;
use vixen_io::SystemData;

const HISTORY_LIMIT: usize = 20;
#[derive(Clone, Debug)]
pub struct StudioEvent {
    pub revision: u64,
    pub kind: String,
    pub command: String,
    pub agent: bool,
}
struct Entry {
    label: String,
    system: SystemData,
}
#[derive(Default)]
struct State {
    revision: u64,
    undo: Vec<Entry>,
    redo: Vec<Entry>,
}
#[derive(Default)]
pub struct StudioDisplay {
    state: Mutex<State>,
    event_revision: std::sync::atomic::AtomicU64,
    stopped: std::sync::atomic::AtomicBool,
    pub runtime: Mutex<Option<Box<dyn Send + Sync>>>,
    events: Mutex<VecDeque<StudioEvent>>,
    listeners: Mutex<Option<Arc<dyn Fn(StudioEvent) + Send + Sync>>>,
}
impl StudioDisplay {
    pub fn stop(&self) {
        let _state = self.state.lock();
        self.stopped
            .store(true, std::sync::atomic::Ordering::Release);
        self.runtime.lock().take();
        self.unsubscribe();
    }
    pub fn subscribe(&self, listener: Arc<dyn Fn(StudioEvent) + Send + Sync>) {
        *self.listeners.lock() = Some(listener);
    }
    pub fn unsubscribe(&self) {
        *self.listeners.lock() = None;
    }
    pub fn events(&self, after: u64) -> Vec<StudioEvent> {
        self.events
            .lock()
            .iter()
            .filter(|e| e.revision > after)
            .cloned()
            .collect()
    }
    pub fn activity(&self, kind: &str, command: &str) {
        self.publish(StudioEvent {
            revision: 0,
            kind: kind.into(),
            command: command.into(),
            agent: true,
        });
    }
    fn publish(&self, mut event: StudioEvent) {
        event.revision = self
            .event_revision
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel)
            + 1;
        {
            let mut events = self.events.lock();
            events.push_back(event.clone());
            while events.len() > 100 {
                events.pop_front();
            }
        }
        let listeners = self.listeners.lock().clone();
        if let Some(listener) = listeners {
            listener(event.clone());
        }
    }
    pub fn history(&self) -> Value {
        let s = self.state.lock();
        json!({"revision":s.revision,"undo":s.undo.iter().rev().map(|e|&e.label).collect::<Vec<_>>(),"redo":s.redo.iter().rev().map(|e|&e.label).collect::<Vec<_>>()})
    }
    /// One validated, undoable transaction; no filesystem writes or controller starts.
    /// expected_revision prevents stale inspectors from overwriting newer agent/UI work.
    pub fn execute(
        &self,
        core: &AppCore,
        command: &str,
        args: &Value,
        agent: bool,
    ) -> Result<Value, String> {
        let tool = command_tools()
            .into_iter()
            .find(|t| t["name"] == command)
            .ok_or("Unknown display command")?;
        let obj = args.as_object().ok_or("arguments must be an object")?;
        let properties = tool["inputSchema"]["properties"].as_object().unwrap();
        if obj.keys().any(|key| !properties.contains_key(key)) {
            return Err("Unknown command argument".into());
        }
        let mut state = self.state.lock();
        if self.stopped.load(std::sync::atomic::Ordering::Acquire) {
            return Err("Profile is closing".into());
        }
        let actual = core.display.revision();
        if state.revision != actual {
            state.undo.clear();
            state.redo.clear();
            state.revision = actual;
        }
        if let Some(expected) = args.get("expectedRevision") {
            if expected.as_u64() != Some(actual) {
                return Err("Display changed; refresh before retrying".into());
            }
        }
        // apply_display can rebuild a running graph: do not let even an ordinary agent edit send output.
        if agent && core.playback.hardware_allowed() && core.execution().is_open() {
            return Err("Stop live output locally before agent display edits".into());
        }
        let before = core.system.read().clone();
        let label = format!("{}{}", command, if agent { " (Agent)" } else { "" });
        let result;
        match command {
            "display_undo" | "display_redo" => {
                let entry = if command == "display_undo" {
                    state.undo.pop()
                } else {
                    state.redo.pop()
                }
                .ok_or("Nothing to undo/redo")?;
                let inverse = Entry {
                    label: entry.label.clone(),
                    system: before,
                };
                core.apply_display(entry.system);
                if command == "display_undo" {
                    state.redo.push(inverse)
                } else {
                    state.undo.push(inverse)
                }
                result = json!({"label":entry.label});
            }
            _ => {
                // A failed edit affects only this candidate. GUIDs, not arena row paths, cross this API.
                let enabled = before
                    .controllers
                    .iter()
                    .filter(|c| !before.disabled_devices.contains(&c.id))
                    .map(|c| c.id)
                    .collect();
                let mut candidate = SetupSession::from_system(before.clone(), &enabled, false);
                result = edit(&mut candidate, command, args)?;
                if !candidate.is_dirty() {
                    return Ok(json!({"changed":false,"revision":actual,"result":result}));
                }
                candidate.studio_materialize();
                core.apply_display(candidate.system);
                state.undo.push(Entry {
                    label,
                    system: before,
                });
                if state.undo.len() > HISTORY_LIMIT {
                    state.undo.remove(0);
                }
                state.redo.clear();
            }
        }
        state.revision = core.display.revision();
        let revision = state.revision;
        drop(state); // callbacks may re-enter history/query; never hold engine/service locks here.
        self.publish(StudioEvent {
            revision,
            kind: "displayChanged".into(),
            command: command.into(),
            agent,
        });
        Ok(json!({"changed":true,"revision":revision,"result":result}))
    }
}
fn text<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args[key]
        .as_str()
        .filter(|s| !s.trim().is_empty() && s.len() <= 256)
        .ok_or(format!(
            "{key} must be a nonempty string of at most 256 bytes"
        ))
}
fn id(args: &Value, key: &str) -> Result<Uuid, String> {
    Uuid::parse_str(text(args, key)?).map_err(|_| format!("{key} must be a UUID"))
}
fn path(s: &SetupSession, id: Uuid) -> Result<String, String> {
    s.tree
        .find(&id)
        .map(|n| s.row_path_of(n))
        .ok_or("Element does not exist".into())
}
fn confirm(args: &Value) -> Result<(), String> {
    if args["confirm"] == true {
        Ok(())
    } else {
        Err("Explicit confirm:true is required".into())
    }
}
fn edit(s: &mut SetupSession, command: &str, args: &Value) -> Result<Value, String> {
    match command {
        "element_create" => {
            let name = text(args, "name")?;
            let parent = if args.get("parentId").is_some() {
                Some(path(s, id(args, "parentId")?)?)
            } else {
                None
            };
            let result = s.add_element(parent.as_deref(), name);
            let ids: Vec<_> = result
                .select
                .unwrap_or_default()
                .iter()
                .filter_map(|p| s.tree.resolve(p)?.last().copied())
                .map(|n| s.tree.node(n).id.to_string())
                .collect();
            Ok(json!({"ids":ids}))
        }
        "element_rename" => {
            let p = path(s, id(args, "id")?)?;
            s.rename_rows(&[p], &[text(args, "name")?.to_string()]);
            Ok(json!({}))
        }
        "element_delete" => {
            confirm(args)?;
            let p = path(s, id(args, "id")?)?;
            s.delete_rows(&[p]);
            Ok(json!({}))
        }
        "element_group" => {
            let ids = args["ids"]
                .as_array()
                .filter(|a| !a.is_empty() && a.len() <= 1000)
                .ok_or("ids must contain 1..1000 UUIDs")?;
            let paths = ids
                .iter()
                .map(|v| {
                    v.as_str()
                        .and_then(|s| Uuid::parse_str(s).ok())
                        .ok_or("Invalid UUID".into())
                        .and_then(|id| path(s, id))
                })
                .collect::<Result<Vec<_>, String>>()?;
            let result = s.create_group(&paths, text(args, "name")?);
            let p = &result.select.as_ref().unwrap()[0];
            let n = *s.tree.resolve(p).unwrap().last().unwrap();
            Ok(json!({"id":s.tree.node(n).id.to_string()}))
        }
        "controller_create" => {
            let outputs = args["outputs"]
                .as_u64()
                .filter(|n| *n > 0 && *n <= 65536)
                .ok_or("outputs must be 1..65536")? as u32;
            let type_id = id(args, "typeId")?;
            if outputs > crate::setup::controllers::output_limit(&type_id) {
                return Err("Output count exceeds controller limit".into());
            }
            let slot = s.add_controller(type_id, text(args, "name")?, outputs)?;
            // New hardware is always disabled; no implicit start from apply_display.
            s.controllers[slot as usize].running = false;
            Ok(json!({"id":s.controllers[slot as usize].id.to_string()}))
        }
        "controller_rename" => {
            let id = id(args, "id")?;
            s.slot_of(id).ok_or("Controller not found")?;
            s.rename_controller(id, text(args, "name")?);
            Ok(json!({}))
        }
        "controller_delete" => {
            confirm(args)?;
            let slot = s.slot_of(id(args, "id")?).ok_or("Controller not found")?;
            s.delete_controllers(None, &[slot]);
            Ok(json!({}))
        }
        "patch_elements" => {
            let p = path(s, id(args, "elementId")?)?;
            let slot = s
                .slot_of(id(args, "controllerId")?)
                .ok_or("Controller not found")?;
            let count = s.controllers[slot as usize].outputs.len() as u32;
            let start = args
                .get("startOutput")
                .map(|v| v.as_u64().ok_or("startOutput must be an integer"))
                .transpose()?
                .unwrap_or(0);
            if start >= u64::from(count) {
                return Err("startOutput is out of range".into());
            }
            let outputs =
                crate::setup::controllers::OutputSelection::single(slot, start as u32..count);
            let n = s.patch_elements(&[p], &outputs, &Default::default(), false);
            Ok(json!({"patched":n}))
        }
        "unpatch_elements" => {
            confirm(args)?;
            let p = path(s, id(args, "id")?)?;
            s.unpatch_elements(&[p], false);
            Ok(json!({}))
        }
        _ => Err("Unknown display command".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ui_agent_share_history_validation_and_notifications() {
        let root = std::env::temp_dir().join(format!("studio-display-{}", Uuid::new_v4()));
        let core = Arc::new(AppCore::open(root.clone(), true).unwrap());
        let weak = Arc::downgrade(&core);
        core.studio_display.subscribe(Arc::new(move |_| {
            // reentrant observer must not deadlock
            let c = weak.upgrade().unwrap();
            let _ = c.studio_display.history();
        }));
        let created = core
            .studio_display
            .execute(&core, "element_create", &json!({"name":"Test"}), false)
            .unwrap();
        let id = created["result"]["ids"][0].as_str().unwrap();
        core.studio_display
            .execute(
                &core,
                "element_rename",
                &json!({"id":id,"name":"Agent edit"}),
                true,
            )
            .unwrap();
        assert!(core.studio_display.history()["undo"][0]
            .as_str()
            .unwrap()
            .ends_with("(Agent)"));
        assert!(core
            .studio_display
            .execute(&core, "element_delete", &json!({"id":id}), true)
            .is_err());
        assert!(core
            .studio_display
            .execute(
                &core,
                "element_rename",
                &json!({"id":id,"name":"stale","expectedRevision":0}),
                true
            )
            .is_err());
        core.studio_display
            .execute(&core, "display_undo", &json!({}), false)
            .unwrap();
        assert!(core
            .system
            .read()
            .tree
            .all_nodes()
            .any(|(_, n)| n.name == "Test"));
        core.studio_display
            .execute(&core, "display_redo", &json!({}), false)
            .unwrap();
        assert!(core
            .system
            .read()
            .tree
            .all_nodes()
            .any(|(_, n)| n.name == "Agent edit"));
        assert_eq!(core.studio_display.events(0).len(), 4);
        drop(core);
        std::fs::remove_dir_all(root).unwrap();
    }
}

/// The single command inventory used by both FFI clients and MCP tools/list.
pub fn command_tools() -> Vec<Value> {
    let string = json!({"type":"string","minLength":1,"maxLength":256});
    let boolean = json!({"type":"boolean"});
    let entries=vec![
        ("element_create","Create an element, optionally under a parent UUID.",json!({"name":string,"parentId":string}),vec!["name"],false),
        ("element_rename","Rename an element by stable UUID.",json!({"id":string,"name":string}),vec!["id","name"],false),
        ("element_delete","Delete an element link. Requires explicit confirmation.",json!({"id":string,"confirm":boolean}),vec!["id","confirm"],true),
        ("element_group","Create a group linking existing element UUIDs.",json!({"name":string,"ids":{"type":"array","items":string,"minItems":1,"maxItems":1000}}),vec!["name","ids"],false),
        ("controller_create","Create a disabled controller. Never starts hardware.",json!({"name":string,"typeId":string,"outputs":{"type":"integer","minimum":1,"maximum":65536}}),vec!["name","typeId","outputs"],false),
        ("controller_rename","Rename a controller by UUID.",json!({"id":string,"name":string}),vec!["id","name"],false),
        ("controller_delete","Delete a controller and its patches. Requires confirmation.",json!({"id":string,"confirm":boolean}),vec!["id","confirm"],true),
        ("patch_elements","Patch unconnected element/group channels to unused controller outputs in tree order. startOutput is zero-based.",json!({"elementId":string,"controllerId":string,"startOutput":{"type":"integer","minimum":0}}),vec!["elementId","controllerId"],false),
        ("unpatch_elements","Remove element/group patch connections; retain filters. Requires confirmation.",json!({"id":string,"confirm":boolean}),vec!["id","confirm"],true),
        ("display_undo","Undo the latest shared UI/agent display edit.",json!({}),vec![],false),
        ("display_redo","Redo the latest shared UI/agent display edit.",json!({}),vec![],false),
    ];
    entries.into_iter().map(|(name,description,mut props,required,destructive)| {
        props["expectedRevision"]=json!({"type":"integer","minimum":0});
        json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":props,"required":required,"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":destructive,"openWorldHint":false}})
    }).collect()
}

impl StudioDisplay {
    pub fn snapshot(&self, core: &AppCore) -> Value {
        let _state = self.state.lock();
        let system = core.system.read();
        json!({"revision":core.display.revision(),"elements":system.tree.all_nodes().map(|(_,n)|json!({"id":n.id.to_string(),"name":n.name,"elementId":n.element_id.map(|id|id.to_string()),"children":n.children.iter().map(|id|system.tree.node(*id).id.to_string()).collect::<Vec<_>>()})).collect::<Vec<_>>(),"controllers":system.controllers.iter().map(|c|json!({"id":c.id.to_string(),"name":c.name,"typeId":c.hardware_type_id.to_string(),"outputs":c.outputs.len(),"enabled":!system.disabled_devices.contains(&c.id)})).collect::<Vec<_>>()})
    }
}
