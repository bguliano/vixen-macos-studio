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
    pub document_id: Option<String>,
    pub revision: u64,
    pub kind: String,
    pub command: String,
    pub agent: bool,
}
struct Entry {
    label: String,
    system: SystemData,
    kind: Kind,
    /// An image map step's picture folders as they were (`studio_lipsync`).
    pictures: Option<crate::studio_lipsync::Pictures>,
    /// A user dictionary step's file as it was (`studio_phoneme_mappings`).
    dictionary: Option<crate::studio_phoneme_mappings::Saved>,
}
/// What a step changed, which decides how undoing it publishes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// The display: undoing it rebuilds the output graph (`apply_display`), so it waits for Live Output to be off.
    Display,
    /// Only previews (`studio_previews`): republished without rebuilding the output.
    Preview,
    /// Only shows and schedules (`studio_schedule`): republished without rebuilding the output.
    Schedule,
    /// Only the LipSync image map library and its pictures (`studio_lipsync`): republished without rebuilding the output.
    LipSync,
    /// Only the user phoneme dictionary file (`studio_phoneme_mappings`): nothing in the configuration to republish.
    Dictionary,
    /// Only elements' Face properties (`studio_face_mapping`): LipSync effects render again, without rebuilding the output.
    Face,
}
impl Kind {
    fn of(command: &str) -> Kind {
        if crate::studio_previews::is_command(command) {
            Kind::Preview
        } else if crate::studio_schedule::is_command(command) {
            Kind::Schedule
        } else if crate::studio_lipsync::is_command(command) {
            Kind::LipSync
        } else if crate::studio_phoneme_mappings::is_command(command) {
            Kind::Dictionary
        } else if crate::studio_face_mapping::is_command(command) {
            Kind::Face
        } else {
            Kind::Display
        }
    }
}
#[derive(Default)]
struct State {
    revision: u64,
    undo: Vec<Entry>,
    redo: Vec<Entry>,
}
#[derive(Default)]
struct Activity {
    clients: std::collections::BTreeSet<String>,
    tools: Vec<String>,
}
#[derive(Default)]
pub struct StudioDisplay {
    activity_state: Mutex<Activity>,
    state: Mutex<State>,
    pub(crate) allow_live: std::sync::atomic::AtomicBool,
    pub(crate) agent_started: Mutex<std::collections::HashSet<Uuid>>,
    /// The commissioning test running now (`studio_commission`).
    pub(crate) commission: Mutex<Option<crate::studio_commission::Running>>,
    pub(crate) sequence_gate: Mutex<()>,
    pub(crate) owned_sequences:
        Mutex<std::collections::HashMap<Uuid, Arc<Mutex<crate::sequence::SequenceSession>>>>,

    event_revision: std::sync::atomic::AtomicU64,
    stopped: std::sync::atomic::AtomicBool,
    pub runtime: Mutex<Option<Box<dyn Send + Sync>>>,
    /// The profile's running scheduler, which keeps its own copy of the schedules (`studio_schedule::publish` updates it).
    scheduler: Mutex<Option<std::sync::Weak<crate::scheduler::run::Scheduler>>>,
    events: Mutex<VecDeque<StudioEvent>>,
    listeners: Mutex<Option<Arc<dyn Fn(StudioEvent) + Send + Sync>>>,
}
impl StudioDisplay {
    /// The running scheduler made for this profile, so schedule edits reach it at once.
    pub fn attach_scheduler(&self, scheduler: &Arc<crate::scheduler::run::Scheduler>) {
        *self.scheduler.lock() = Some(Arc::downgrade(scheduler));
    }
    pub fn scheduler(&self) -> Option<Arc<crate::scheduler::run::Scheduler>> {
        self.scheduler.lock().as_ref().and_then(std::sync::Weak::upgrade)
    }
    pub(crate) fn is_stopped(&self) -> bool {
        self.stopped.load(std::sync::atomic::Ordering::Acquire)
    }
    pub fn stop(&self) {
        let _sequences = self.sequence_gate.lock();
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
    pub fn document_changed(&self, command: &str, id: Option<String>, agent: bool) {
        self.publish(StudioEvent {
            revision: 0,
            kind: "documentChanged".into(),
            command: command.into(),
            agent,
            document_id: id,
        });
    }
    pub fn activity(&self, kind: &str, command: &str) {
        self.publish(StudioEvent {
            document_id: None,
            revision: 0,
            kind: kind.into(),
            command: command.into(),
            agent: true,
        });
    }
    fn publish(&self, mut event: StudioEvent) {
        {
            let mut events = self.events.lock();
            event.revision = self
                .event_revision
                .fetch_add(1, std::sync::atomic::Ordering::AcqRel)
                + 1;
            let mut activity = self.activity_state.lock();
            match event.kind.as_str() {
                "clientConnected" => {
                    activity.clients.insert(event.command.clone());
                }
                "clientDisconnected" => {
                    activity.clients.remove(&event.command);
                }
                "commandStarted" => activity.tools.push(event.command.clone()),
                "commandFinished" | "commandFailed" => {
                    if let Some(index) = activity.tools.iter().position(|t| t == &event.command) {
                        activity.tools.remove(index);
                    }
                }
                _ => {}
            }
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
    pub fn activity_snapshot(&self) -> Value {
        let events = self.events.lock();
        let activity = self.activity_state.lock();
        json!({"revision":events.back().map_or(0,|e|e.revision),"clients":activity.clients.iter().collect::<Vec<_>>(),"tools":activity.tools,"actions":events.iter().filter(|e|matches!(e.kind.as_str(),"commandFinished"|"commandFailed")).map(|e|json!({"id":e.revision,"command":e.command,"summary":e.command,"succeeded":e.kind=="commandFinished"})).collect::<Vec<_>>()})
    }
    pub fn history(&self) -> Value {
        let s = self.state.lock();
        json!({"revision":s.revision,"undo":s.undo.iter().rev().map(|e|&e.label).collect::<Vec<_>>(),"redo":s.redo.iter().rev().map(|e|&e.label).collect::<Vec<_>>(),"undoPreviewOnly":s.undo.last().is_some_and(|e|e.kind!=Kind::Display),"redoPreviewOnly":s.redo.last().is_some_and(|e|e.kind!=Kind::Display)})
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
        crate::studio_schema::arguments(&tool["inputSchema"], args)?;
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
        // Preview list, show and schedule edits never touch the graph (their own publish), so they stay open to agents.
        if agent
            && core.playback.hardware_allowed()
            && core.execution().is_open()
            && Kind::of(command) == Kind::Display
        {
            return Err("Stop live output locally before agent display edits".into());
        }
        if command == "profile_save" {
            confirm(args)?;
            let root = core
                .paths
                .root()
                .canonicalize()
                .map_err(|e| e.to_string())?;
            for file in [
                core.paths.system_config_file(),
                core.paths.module_store_file(),
            ] {
                let parent = file
                    .parent()
                    .ok_or("Invalid config path")?
                    .canonicalize()
                    .map_err(|e| e.to_string())?;
                crate::studio_paths::reject_links(&parent, 0)?;
                if !parent.starts_with(&root)
                    || std::fs::symlink_metadata(&file).is_ok_and(|m| m.file_type().is_symlink())
                {
                    return Err("Config output escapes profile or is a symlink".into());
                }
            }
            let _system = crate::app::Saving::begin(&core.saving_system)
                .ok_or("System save already in progress")?;
            let _module = crate::app::Saving::begin(&core.saving_module)
                .ok_or("Module save already in progress")?;
            let system = core.system.read().clone();
            vixen_io::system::save_system_config_file(&system, &core.paths)
                .map_err(|e| e.to_string())?;
            vixen_io::system::save_module_config_file(&system, &core.paths)
                .map_err(|e| format!("System config saved but module save failed: {e}"))?;
            crate::studio_plugin_controllers::system_saved(&system, &core.paths);
            drop(state);
            self.activity("profileSaved", command);
            return Ok(json!({"saved":true,"revision":actual}));
        }
        let before = core.system.read().clone();
        let name = match Kind::of(command) {
            Kind::Preview => crate::studio_previews::label(command, args),
            Kind::Schedule => crate::studio_schedule::label(command, args),
            Kind::LipSync => crate::studio_lipsync::label(command),
            Kind::Dictionary => "",
            Kind::Face => crate::studio_face_mapping::LABEL,
            Kind::Display if command == "element_single_bulbs" => "Add Single Bulbs",
            Kind::Display => command,
        };
        let name = if Kind::of(command) == Kind::Dictionary { crate::studio_phoneme_mappings::label(command, args) } else { name.to_string() };
        let label = format!("{}{}", name, if agent { " (Agent)" } else { "" });
        let result;
        match command {
            "display_undo" | "display_redo" => {
                let stack = if command == "display_undo" { &state.undo } else { &state.redo };
                let kind = stack.last().ok_or("Nothing to undo/redo")?.kind;
                // Undoing a display edit rebuilds the graph, which Live Output must not see; a preview or schedule step never does.
                if kind == Kind::Display && !agent && self.ui_edit_block(core).is_some() {
                    return Err(LIVE_OUTPUT_BLOCK.into());
                }
                let entry = if command == "display_undo" {
                    state.undo.pop()
                } else {
                    state.redo.pop()
                }
                .ok_or("Nothing to undo/redo")?;
                let pictures =
                    entry.pictures.as_ref().map(|p| p.restore(&crate::studio_lipsync::lipsync_dir(core)));
                let dictionary = entry
                    .dictionary
                    .as_ref()
                    .map(|d| d.restore(&crate::studio_phoneme_mappings::user_dictionary(core)));
                let inverse = Entry {
                    label: entry.label.clone(),
                    system: before,
                    kind,
                    pictures,
                    dictionary,
                };
                match kind {
                    Kind::Preview => crate::studio_previews::publish(core, entry.system),
                    Kind::Schedule => crate::studio_schedule::publish(core, entry.system),
                    Kind::LipSync => crate::studio_lipsync::publish(core, entry.system),
                    Kind::Dictionary => {}
                    Kind::Face => crate::studio_face_mapping::publish(core, entry.system),
                    Kind::Display => core.apply_display(entry.system),
                }
                if command == "display_undo" {
                    state.redo.push(inverse)
                } else {
                    state.undo.push(inverse)
                }
                result = json!({"label":entry.label});
            }
            _ if crate::studio_previews::is_command(command) => {
                let mut system = before.clone();
                let (value, changed) = crate::studio_previews::edit(&mut system, command, args)?;
                result = value;
                if !changed {
                    return Ok(json!({"changed":false,"revision":actual,"result":result}));
                }
                crate::studio_previews::publish(core, system);
                state.undo.push(Entry {
                    label,
                    system: before,
                    kind: Kind::Preview,
                    pictures: None,
            dictionary: None,
                });
                if state.undo.len() > HISTORY_LIMIT {
                    state.undo.remove(0);
                }
                state.redo.clear();
            }
            _ if crate::studio_schedule::is_command(command) => {
                let mut system = before.clone();
                let (value, changed) =
                    crate::studio_schedule::edit(&mut system, &core.paths.sequence_dir(), command, args)?;
                result = value;
                if !changed {
                    return Ok(json!({"changed":false,"revision":actual,"result":result}));
                }
                crate::studio_schedule::publish(core, system);
                state.undo.push(Entry {
                    label,
                    system: before,
                    kind: Kind::Schedule,
                    pictures: None,
            dictionary: None,
                });
                if state.undo.len() > HISTORY_LIMIT {
                    state.undo.remove(0);
                }
                state.redo.clear();
            }
            _ if crate::studio_lipsync::is_command(command) => {
                let mut system = before.clone();
                let (value, changed, pictures) = crate::studio_lipsync::edit(
                    &mut system,
                    &crate::studio_lipsync::lipsync_dir(core),
                    command,
                    args,
                )?;
                result = value;
                if !changed {
                    return Ok(json!({"changed":false,"revision":actual,"result":result}));
                }
                crate::studio_lipsync::publish(core, system);
                state.undo.push(Entry {
                    label,
                    system: before,
                    kind: Kind::LipSync,
                    pictures: Some(pictures),
                    dictionary: None,
                });
                if state.undo.len() > HISTORY_LIMIT {
                    state.undo.remove(0);
                }
                state.redo.clear();
            }
            _ if crate::studio_face_mapping::is_command(command) => {
                let mut system = before.clone();
                let (value, changed) = crate::studio_face_mapping::edit(&mut system, command, args)?;
                result = value;
                if !changed {
                    return Ok(json!({"changed":false,"revision":actual,"result":result}));
                }
                crate::studio_face_mapping::publish(core, system);
                state.undo.push(Entry {
                    label,
                    system: before,
                    kind: Kind::Face,
                    pictures: None,
                    dictionary: None,
                });
                if state.undo.len() > HISTORY_LIMIT {
                    state.undo.remove(0);
                }
                state.redo.clear();
            }
            _ if crate::studio_phoneme_mappings::is_command(command) => {
                let (value, changed, saved) = crate::studio_phoneme_mappings::edit(
                    &crate::studio_phoneme_mappings::user_dictionary(core),
                    command,
                    args,
                )?;
                result = value;
                if !changed {
                    return Ok(json!({"changed":false,"revision":actual,"result":result}));
                }
                state.undo.push(Entry {
                    label,
                    system: before,
                    kind: Kind::Dictionary,
                    pictures: None,
                    dictionary: Some(saved),
                });
                if state.undo.len() > HISTORY_LIMIT {
                    state.undo.remove(0);
                }
                state.redo.clear();
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
                    kind: Kind::Display,
                    pictures: None,
            dictionary: None,
                });
                if state.undo.len() > HISTORY_LIMIT {
                    state.undo.remove(0);
                }
                state.redo.clear();
            }
        }
        if command.starts_with("preview_prop_")
            || crate::studio_previews::is_command(command)
            || matches!(command, "display_undo" | "display_redo")
        {
            core.mark_preview_changed();
        }
        state.revision = core.display.revision();
        let revision = state.revision;
        drop(state); // callbacks may re-enter history/query; never hold engine/service locks here.
        self.publish(StudioEvent {
            document_id: None,
            revision,
            kind: "displayChanged".into(),
            command: command.into(),
            agent,
        });
        Ok(json!({"changed":true,"revision":revision,"result":result}))
    }
}
/// Why the local Display pane can't publish right now, if it can't. Publishing rebuilds the running graph, which can restart controllers.
pub const LIVE_OUTPUT_BLOCK: &str = "Live Output is on";
impl StudioDisplay {
    pub fn ui_edit_block(&self, core: &AppCore) -> Option<&'static str> {
        (core.playback.hardware_allowed() && core.execution().is_open()).then_some(LIVE_OUTPUT_BLOCK)
    }
    /// The Display pane's live edit. Its working copy already holds edits made through Display Setup's own algorithms; this publishes
    /// them as one undoable transaction on the shared history, as `execute` publishes a command's candidate. `expected_revision` is the
    /// display revision the working copy was opened at or last published: anything newer (an agent, undo, another writer) refuses, and
    /// the pane reloads. Afterwards the working copy matches the display and is clean again. `label` names the step when the
    /// pane knows better than the dirty flags, such as a setup sheet's visit ("Configure Controller").
    pub fn publish_setup(
        &self,
        core: &AppCore,
        session: &mut SetupSession,
        expected_revision: u64,
        cleanup: bool,
        label: Option<&str>,
    ) -> Result<Value, String> {
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
        if expected_revision != actual {
            return Err("Display changed; refresh before retrying".into());
        }
        if let Some(block) = self.ui_edit_block(core) {
            return Err(block.into());
        }
        if cleanup {
            session.studio_confirm_cleanup();
        }
        if !session.is_dirty() {
            return Ok(json!({"changed":false,"revision":actual}));
        }
        let d = &session.dirty;
        let label = if let Some(label) = label.filter(|l| !l.is_empty()) {
            label
        } else if d.controllers && !d.tree && !d.flow {
            "Edit Controllers"
        } else if d.run_state && !d.tree && !d.flow && !d.controllers {
            "Start or Stop Controllers"
        } else if d.flow && !d.tree {
            "Patching"
        } else if d.tree && !d.controllers {
            "Edit Elements"
        } else if d.tags && !d.tree {
            "Edit Tags"
        } else if cleanup && !d.tree {
            "Remove Unused Filters"
        } else {
            "Edit Display"
        };
        let before = core.system.read().clone();
        let preview_changed = session.dirty.studio_module;
        session.studio_materialize();
        core.apply_display(session.system.clone());
        session.dirty = Default::default();
        if preview_changed {
            // A template's linked preview shape (studio_props::add_linked) rides in the same publication.
            core.mark_preview_changed();
        }
        state.undo.push(Entry {
            label: label.into(),
            system: before,
            kind: Kind::Display,
            pictures: None,
            dictionary: None,
        });
        if state.undo.len() > HISTORY_LIMIT {
            state.undo.remove(0);
        }
        state.redo.clear();
        state.revision = core.display.revision();
        let revision = state.revision;
        drop(state);
        self.publish(StudioEvent {
            document_id: None,
            revision,
            kind: "displayChanged".into(),
            command: "display_setup".into(),
            agent: false,
        });
        Ok(json!({"changed":true,"revision":revision,"label":label}))
    }
}
impl StudioDisplay {
    /// A change made outside the commands that touches only previews, such as the preview designer writing its layout back
    /// when it closes: one step on the shared history, so Edit ▸ Undo in the show window takes it back. `changed` false applies
    /// `edit` without a step. The display revision moves, so stale inspectors refresh.
    pub fn record_preview_edit(&self, core: &AppCore, label: &str, changed: bool, edit: impl FnOnce(&mut SystemData)) {
        let mut state = self.state.lock();
        if !changed || self.is_stopped() {
            edit(&mut core.system.write());
            return;
        }
        let actual = core.display.revision();
        if state.revision != actual {
            state.undo.clear();
            state.redo.clear();
        }
        let before = core.system.read().clone();
        edit(&mut core.system.write());
        core.display.studio_bump();
        core.mark_preview_changed();
        state.undo.push(Entry {
            label: label.into(),
            system: before,
            kind: Kind::Preview,
            pictures: None,
            dictionary: None,
        });
        if state.undo.len() > HISTORY_LIMIT {
            state.undo.remove(0);
        }
        state.redo.clear();
        state.revision = core.display.revision();
        let revision = state.revision;
        drop(state);
        self.publish(StudioEvent {
            document_id: None,
            revision,
            kind: "displayChanged".into(),
            command: "preview_design".into(),
            agent: false,
        });
    }
}
/// What a Face Setup visit did: whether the Face properties changed, how many LipSync effects rendered again, and the visit's step
/// on the shared history (none when nothing changed, or when the display moved during the visit so Undo would take that back too).
#[derive(Debug, PartialEq, Eq)]
pub struct FaceSetupFinished {
    pub changed: bool,
    pub rendered: usize,
    pub label: Option<String>,
}
/// The profile as Face Setup found it, from [`StudioDisplay::face_setup_begin`].
pub struct FaceSetupStart {
    system: SystemData,
    revision: u64,
}
impl StudioDisplay {
    /// Tools ▸ LipSync ▸ Edit Element Face Mapping is about to open Face Setup, which writes the profile as it's used.
    pub fn face_setup_begin(&self, core: &AppCore) -> FaceSetupStart {
        let _state = self.state.lock();
        FaceSetupStart { system: core.system.read().clone(), revision: core.display.revision() }
    }
    /// Face Setup closed: when it changed `nodes`' Face properties, the whole visit becomes one step on the shared history
    /// (`studio_face_mapping`) and LipSync effects in the open sequences render again. No step when the display moved meanwhile
    /// (an agent, an Undo): Undo of the visit would take that back too.
    pub fn face_setup_finish(&self, core: &AppCore, start: FaceSetupStart, nodes: &[Uuid]) -> FaceSetupFinished {
        let mut state = self.state.lock();
        let changed = crate::studio_face_mapping::fingerprint(&start.system, nodes)
            != crate::studio_face_mapping::fingerprint(&core.system.read(), nodes);
        if !changed {
            return FaceSetupFinished { changed: false, rendered: 0, label: None };
        }
        let actual = core.display.revision();
        core.display.studio_bump();
        let rendered = core.face_mapping_applied();
        let recorded = actual == start.revision && !self.is_stopped();
        if recorded {
            if state.revision != actual {
                state.redo.clear();
                state.undo.clear();
            }
            state.undo.push(Entry {
                label: crate::studio_face_mapping::LABEL.into(),
                system: start.system,
                kind: Kind::Face,
                pictures: None,
                dictionary: None,
            });
            if state.undo.len() > HISTORY_LIMIT {
                state.undo.remove(0);
            }
            state.redo.clear();
            state.revision = core.display.revision();
        }
        let revision = core.display.revision();
        drop(state);
        self.publish(StudioEvent {
            document_id: None,
            revision,
            kind: "displayChanged".into(),
            command: "face_setup".into(),
            agent: false,
        });
        FaceSetupFinished { changed: true, rendered, label: recorded.then(|| crate::studio_face_mapping::LABEL.to_string()) }
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
            // Several elements patch as one run (a prop and its hidden pixels), in their wire order when every light has one.
            let mut paths = vec![path(s, id(args, "elementId")?)?];
            if let Some(more) = args.get("alsoIds") {
                for extra in more.as_array().ok_or("alsoIds must be a list of element UUIDs")? {
                    let extra = extra.as_str().and_then(|t| Uuid::parse_str(t).ok()).ok_or("alsoIds must be element UUIDs")?;
                    paths.push(path(s, extra)?);
                }
            }
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
            let use_order = s.order_check(&paths) == crate::setup::patching::OrderCheck::UseOrder;
            let n = s.patch_elements(&paths, &outputs, &Default::default(), use_order);
            Ok(json!({"patched":n,"wireOrder":use_order}))
        }
        "unpatch_elements" => {
            confirm(args)?;
            let p = path(s, id(args, "id")?)?;
            s.unpatch_elements(&[p], false);
            Ok(json!({}))
        }
        _ => extra_edit(s, command, args),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn display_pane_publishes_live_with_shared_undo() {
        let root = std::env::temp_dir().join(format!("studio-pane-{}", Uuid::new_v4()));
        let core = Arc::new(AppCore::open(root.clone(), true).unwrap());
        let start = core.display.revision();
        let mut pane = SetupSession::open(&core);
        // Nothing edited: nothing published.
        let clean = core.studio_display.publish_setup(&core, &mut pane, start, false, None).unwrap();
        assert_eq!(clean["changed"], false);
        pane.add_element(None, "Arch");
        let published = core.studio_display.publish_setup(&core, &mut pane, start, false, None).unwrap();
        assert_eq!(published["changed"], true);
        assert_eq!(published["label"], "Edit Elements");
        assert!(!pane.is_dirty());
        assert_eq!(core.system.read().tree.len(), 1);
        let after = published["revision"].as_u64().unwrap();
        assert_eq!(core.studio_display.history()["undo"][0], "Edit Elements");
        // A stale working copy refuses rather than overwriting newer work.
        pane.add_element(None, "Tree");
        assert!(core.studio_display.publish_setup(&core, &mut pane, start, false, None).is_err());
        // The pane's step undoes on the same history agents use.
        let mut fresh = SetupSession::open(&core);
        fresh.add_element(None, "Star");
        core.studio_display.publish_setup(&core, &mut fresh, after, false, None).unwrap();
        assert_eq!(core.system.read().tree.len(), 2);
        core.studio_display.execute(&core, "display_undo", &json!({}), false).unwrap();
        assert_eq!(core.system.read().tree.len(), 1);
        core.studio_display.execute(&core, "display_undo", &json!({}), false).unwrap();
        assert_eq!(core.system.read().tree.len(), 0);
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn a_setup_sheet_visit_is_one_named_step() {
        // B9: everything a setup sheet changed while it was open publishes once, under the sheet's name.
        let root = std::env::temp_dir().join(format!("studio-pane-sheet-{}", Uuid::new_v4()));
        let core = Arc::new(AppCore::open(root.clone(), true).unwrap());
        let start = core.display.revision();
        let mut pane = SetupSession::open(&core);
        pane.add_element(None, "Arch");
        pane.add_element(None, "Tree");
        let published = core.studio_display.publish_setup(&core, &mut pane, start, false, Some("Configure Controller")).unwrap();
        assert_eq!(published["label"], "Configure Controller");
        assert_eq!(core.studio_display.history()["undo"].as_array().unwrap().len(), 1);
        core.studio_display.execute(&core, "display_undo", &json!({}), false).unwrap();
        assert_eq!(core.system.read().tree.len(), 0);
        // An empty name falls back to the dirty flags.
        let mut again = SetupSession::open(&core);
        again.add_element(None, "Star");
        let fallback = core.studio_display.publish_setup(&core, &mut again, core.display.revision(), false, Some("")).unwrap();
        assert_eq!(fallback["label"], "Edit Elements");
        let _ = std::fs::remove_dir_all(root);
    }
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
        ("profile_save","Save current live display configuration and module data. Explicit confirmation required; retains undo history.",json!({"confirm":boolean}),vec!["confirm"],true),
        ("element_template","Create a bounded single item, arch, group, megatree or matrix using native template algorithms. count=segments/strings/rows (1 for single); pixels=visible pixels per string/columns. Megatree and matrix wiring, as the template sheet's diagram shows it (megatree drawn flat: S1 on the left, each string's Px1 at the bottom; matrix: Row1 at the bottom, Pixel1 on the left): start = the corner where the wire enters (default bottomLeft); reverseEveryOtherString (zigZag is the same option) makes each string run back the way the previous one came; hiddenPixels = dark pixels at the Px1 end (bottom) of every string, such as lights tucked under a tree's base, made in a separate group \"<name> Hidden\" (hiddenId) that keeps their wire position but is not part of the prop. Patch both together: patch_elements with alsoIds:[hiddenId]. The result's wiring gives the order in words. color sets colour handling on every new light in the same step, as the template's Colour picker does: rgb (3 channels per pixel, R G B), rgbw (4, R G B W) or single (1; singleColor, default royal blue). Without color the lights get no colour handling. previewId (megatree, matrix or arch) also adds the matching shape to that preview in the same step, already linked light for light in the prop's order, as preview_prop_create with elementId does; the result's preview names it (propId, strings, lightsPerString). For single, previewId adds a linked Single Light bulb. To add several single bulbs at once (smart bulbs, lamps), patched in order, use element_single_bulbs.",json!({"previewId":string,"name":string,"kind":{"type":"string","enum":["single","arch","group","megatree","matrix"]},"count":{"type":"integer","minimum":1,"maximum":4096},"pixels":{"type":"integer","minimum":1,"maximum":4096},"zigZag":boolean,"reverseEveryOtherString":boolean,"start":{"type":"string","enum":["bottomLeft","bottomRight","topLeft","topRight"]},"hiddenPixels":{"type":"integer","minimum":0,"maximum":1000},"color":{"type":"string","enum":["rgb","rgbw","single"]},"singleColor":{"type":"string","pattern":"^#[0-9a-fA-F]{6}$"}}),vec!["name","kind","count"],false),
        ("element_color","Set colour handling on an element/group's leaves, as the Color Handling helper does: the Color property plus a Color Breakdown filter, replacing existing ones. rgb = 3 channels per pixel (R, G, B), rgbw = 4 (R, G, B, W), single = 1 in color (default royal blue). Patch afterwards; the breakdown's channels are what patching connects.",json!({"id":string,"mode":{"type":"string","enum":["rgb","rgbw","single"]},"color":{"type":"string","pattern":"^#[0-9a-fA-F]{6}$"}}),vec!["id","mode"],false),
        crate::studio_locations::tool_entry(),
        ("element_wire_order","Reverse the selected element/group row order using the native setup command.",json!({"id":string}),vec!["id"],false),
        ("controller_configure","Set up a DDP or E1.31 controller's network in one step, as the controller inspector does: protocol (changing it replaces the controller, keeping name, outputs and patches, and returns newId), an IPv4 unicast address, output count, and for E1.31 the first universe and channels per universe (510 or 512; or preset wledRgb=510 / wledRgbw=512), with universes generated from the output count. Fields left out keep their current values (new controllers: universe 1, 510 channels). A plugin controller takes config instead (an object matching the plugin's configSchema, shown by controller_network as plugin.configSchema; field errors are refused) and/or outputs. When the plugin sets its own output count (plugin.outputsFromConfig, controller_types outputsFromConfig) outputs is refused: the controller is resized and its outputs named to match the new config in the same undo step, and new settings that would drop patched outputs are refused unless removePatchedOutputs is true. Does not connect or start output. Read back with controller_network.",json!({"id":string,"config":{"type":"object","description":"Plugin controllers only: the whole configuration object."},"removePatchedOutputs":{"type":"boolean","description":"Plugin controllers that set their own output count: allow new settings to drop outputs that are patched (their patches go)."},"protocol":{"type":"string","enum":["e131","ddp"]},"address":string,"outputs":{"type":"integer","minimum":1,"maximum":65536},"firstUniverse":{"type":"integer","minimum":1,"maximum":63999},"universeSize":{"type":"integer","minimum":1,"maximum":512},"preset":{"type":"string","enum":["wledRgb","wledRgbw"]}}),vec!["id"],false),

        ("element_create","Create an element, optionally under a parent UUID.",json!({"name":string,"parentId":string}),vec!["name"],false),
        ("element_rename","Rename an element by stable UUID.",json!({"id":string,"name":string}),vec!["id","name"],false),
        ("element_delete","Delete an element link. Requires explicit confirmation.",json!({"id":string,"confirm":boolean}),vec!["id","confirm"],true),
        ("element_group","Create a group linking existing element UUIDs.",json!({"name":string,"ids":{"type":"array","items":string,"minItems":1,"maxItems":1000}}),vec!["name","ids"],false),
        ("controller_create","Create a disabled controller. Never starts hardware.",json!({"name":string,"typeId":string,"outputs":{"type":"integer","minimum":1,"maximum":65536}}),vec!["name","typeId","outputs"],false),
        ("controller_rename","Rename a controller by UUID.",json!({"id":string,"name":string}),vec!["id","name"],false),
        ("controller_delete","Delete a controller and its patches. Requires confirmation.",json!({"id":string,"confirm":boolean}),vec!["id","confirm"],true),
        ("patch_elements","Patch unconnected element/group channels to unused controller outputs as one run. alsoIds adds more elements to the same run (a template's hidden-pixel group). When every light has a wire position (the Order property element_template sets for a start corner, reversing or hidden pixels) they patch in that order, otherwise in tree order; wireOrder in the result says which. startOutput is zero-based.",json!({"elementId":string,"alsoIds":{"type":"array","items":string,"maxItems":1000},"controllerId":string,"startOutput":{"type":"integer","minimum":0}}),vec!["elementId","controllerId"],false),
        ("unpatch_elements","Remove element/group patch connections; retain filters. Requires confirmation.",json!({"id":string,"confirm":boolean}),vec!["id","confirm"],true),
        ("patch_reverse","Reverse the order an element or group's lights are patched in, within the outputs it already uses: a string wired backwards, fixed in one undoable step. Each light keeps its channels together and in order (an RGB pixel stays R, G, B). The result gives the outputs as universe:channel ranges.",json!({"id":string}),vec!["id"],false),
        ("patch_swap","Swap where two elements or groups are patched (two strings or groups hung in each other's place) in one undoable step. Both must use the same number of outputs and share no lights; each keeps its own light order. The result gives both as universe:channel ranges.",json!({"id":string,"withId":string}),vec!["id","withId"],false),
        ("patch_move","Move an element or group's patched outputs, in order, to a controller's outputs from startOutput (zero-based, as patch_elements counts), in one undoable step. The destination must be unpatched or the element's own outputs. patch_ranges shows where it is now. The result gives the new universe:channel ranges.",json!({"id":string,"controllerId":string,"startOutput":{"type":"integer","minimum":0,"maximum":1048576}}),vec!["id","controllerId","startOutput"],false),
        // Several single bulbs in one step (studio_single_bulbs.rs, B60).
        { let (name, description, props, required) = crate::studio_single_bulbs::tool(); (name, description, props, required, false) },
        ("display_undo","Undo the latest shared UI/agent display edit.",json!({}),vec![],false),
        ("display_redo","Redo the latest shared UI/agent display edit.",json!({}),vec![],false),
    ];
    let mut tools:Vec<Value>=entries.into_iter().map(|(name,description,mut props,required,destructive)| {
        props["expectedRevision"]=json!({"type":"integer","minimum":0});
        json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":props,"required":required,"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":destructive,"openWorldHint":false}})
    }).collect();
    tools.extend(crate::studio_schedule::tools());
    tools.extend(crate::studio_props::tools());
    tools.extend(crate::studio_previews::tools());
    tools.extend(crate::studio_lipsync::tools());
    tools.extend(crate::studio_phoneme_mappings::tools());
    tools.extend(crate::studio_face_mapping::tools());
    tools
}

impl StudioDisplay {
    pub fn snapshot(&self, core: &AppCore) -> Value {
        let _state = self.state.lock();
        let system = core.system.read();
        json!({"revision":core.display.revision(),"elements":system.tree.all_nodes().map(|(_,n)|json!({"id":n.id.to_string(),"name":n.name,"elementId":n.element_id.map(|id|id.to_string()),"children":n.children.iter().map(|id|system.tree.node(*id).id.to_string()).collect::<Vec<_>>()})).collect::<Vec<_>>(),"controllers":system.controllers.iter().map(|c|json!({"id":c.id.to_string(),"name":c.name,"typeId":c.hardware_type_id.to_string(),"outputs":c.outputs.len(),"enabled":!system.disabled_devices.contains(&c.id)})).collect::<Vec<_>>()})
    }
}

fn extra_edit(s: &mut SetupSession, command: &str, a: &Value) -> Result<Value, String> {
    match command {
        "element_template" => {
            use crate::setup::templates::{StartLocation, TemplateParams};
            let name = text(a, "name")?.to_string();
            let count = a["count"]
                .as_u64()
                .filter(|n| *n > 0 && *n <= 4096)
                .ok_or("count must be 1..4096")? as u32;
            let pixels = a
                .get("pixels")
                .map(|n| n.as_u64().ok_or("pixels must be an integer"))
                .transpose()?
                .unwrap_or(1);
            let hidden = a
                .get("hiddenPixels")
                .map(|n| n.as_u64().filter(|n| *n <= u64::from(crate::studio_wiring::MAX_HIDDEN)).ok_or("hiddenPixels must be 0..1000"))
                .transpose()?
                .unwrap_or(0);
            let start = match a.get("start") {
                Some(v) => v.as_str().and_then(crate::studio_wiring::parse_corner).ok_or("start must be bottomLeft, bottomRight, topLeft or topRight")?,
                None => StartLocation::BottomLeft,
            };
            let reverse = a["zigZag"] == true || a["reverseEveryOtherString"] == true;
            if pixels == 0 || (pixels + hidden) * u64::from(count) > 4096 {
                return Err("Template limit is 4096 leaf elements".into());
            }
            let color = match a.get("color") {
                Some(_) => Some(crate::studio_color::parse(a, "color")?),
                None => None,
            };
            let kind = text(a, "kind")?;
            let preview = match a.get("previewId") {
                Some(_) if !crate::studio_props::LINKED_KINDS.contains(&kind) => {
                    return Err("previewId applies to a single item, megatree, matrix or arch".into())
                }
                Some(_) => Some(id(a, "previewId")?),
                None => None,
            };
            let params = match kind {
                "single" => TemplateParams::Single { name },
                "arch" | "group" => TemplateParams::Numbered {
                    group: name,
                    prefix: "Pixel".into(),
                    count,
                },
                "megatree" => TemplateParams::Megatree {
                    name,
                    strings: count,
                    string_prefix: "String".into(),
                    pixel: true,
                    pixels: pixels as u32,
                    pixel_prefix: "Pixel".into(),
                    start,
                    zig_zag: reverse,
                },
                "matrix" => TemplateParams::PixelGrid {
                    name,
                    horizontal: true,
                    rows: count,
                    columns: pixels as u32,
                    first_prefix: "Row".into(),
                    second_prefix: "Pixel".into(),
                    start,
                    zig_zag: reverse,
                },
                _ => return Err("kind must be single, arch, group, megatree or matrix".into()),
            };
            let hidden = if matches!(params, TemplateParams::Megatree { .. } | TemplateParams::PixelGrid { .. }) { hidden as u32 } else { 0 };
            let wiring = crate::studio_wiring::plan(&params, hidden);
            let built = crate::studio_wiring::generate(&mut s.tree, &params, hidden).map_err(|e| format!("{e:?}"))?;
            let head_node = built.result.head;
            let hidden_node = built.hidden_head;
            let head = head_node.map(|n| s.tree.node(n).id.to_string());
            let hidden_id = hidden_node.map(|n| s.tree.node(n).id.to_string());
            let count = built.result.created.len();
            s.apply_template_result(built.result);
            let color = match (color, head_node) {
                (Some((mode, single)), Some(node)) => {
                    let nodes: Vec<_> = std::iter::once(node).chain(hidden_node).collect();
                    Some(crate::studio_color::apply(s, &nodes, &mode, single)?)
                }
                _ => None,
            };
            let wiring = (wiring["applies"] == true).then(|| {
                json!({"start":wiring["start"],"reverseEveryOtherString":wiring["reverseEveryOtherString"],"hiddenPixels":wiring["hidden"],"summary":wiring["summary"],"hidden":wiring["hiddenText"],"totals":wiring["totals"]})
            });
            // The hidden group is not part of the prop, so the shape shows only the visible lights.
            let preview = match (preview, head_node) {
                (Some(preview), Some(node)) => {
                    let node = s.tree.node(node).id;
                    Some(crate::studio_props::add_linked(s, preview, node, kind, None)?)
                }
                _ => None,
            };
            Ok(json!({"id":head,"hiddenId":hidden_id,"createdNodes":count,"color":color,"wiring":wiring,"preview":preview}))
        }
        "element_color" => {
            let node = s.tree.find(&id(a, "id")?).ok_or("Element not found")?;
            let (mode, color) = crate::studio_color::parse(a, "mode")?;
            crate::studio_color::apply(s, &[node], &mode, color)
        }
        "element_location" => crate::studio_locations::apply(s, a),
        "element_single_bulbs" => crate::studio_single_bulbs::create(s, a),
        "element_wire_order" => {
            let p = path(s, id(a, "id")?)?;
            s.reverse_rows(&[p]);
            Ok(json!({}))
        }
        "controller_configure" => crate::studio_controller_network::configure(s, a),
        "patch_reverse" => crate::studio_patch::reverse(s, a),
        "patch_swap" => crate::studio_patch::swap(s, a),
        "patch_move" => crate::studio_patch::move_to(s, a),
        _ => {
            if command.starts_with("preview_prop_") {
                crate::studio_props::edit(s, command, a)
            } else {
                Err("Unknown display command".into())
            }
        }
    }
}

#[cfg(test)]
mod configuration_tests {
    use super::*;
    #[test]
    fn templates_colors_network_configuration_save_and_undo() {
        let root = std::env::temp_dir().join(format!("studio-config-{}", Uuid::new_v4()));
        let core = AppCore::open(root.clone(), true).unwrap();
        let run = |name: &str, args: Value| {
            core.studio_display
                .execute(&core, name, &args, true)
                .unwrap()
        };
        let tree = run(
            "element_template",
            json!({"kind":"megatree","name":"Tree","count":2,"pixels":5}),
        );
        let id = tree["result"]["id"].clone();
        assert_eq!(
            run("element_color", json!({"id":id,"mode":"rgb"}))["result"]["updatedLeaves"],
            10
        );
        let c = run(
            "controller_create",
            json!({"name":"Test controller","typeId":vixen_output::ids::E131.to_string(),"outputs":30}),
        );
        let controller = c["result"]["id"].clone();
        run(
            "controller_configure",
            json!({"id":controller,"address":"127.0.0.1","firstUniverse":1,"universeSize":30}),
        );
        assert!(core
            .system
            .read()
            .disabled_devices
            .contains(&Uuid::parse_str(controller.as_str().unwrap()).unwrap()));
        assert!(!core.execution().is_open());
        run(
            "patch_elements",
            json!({"elementId":id,"controllerId":controller}),
        );
        assert!(core
            .studio_display
            .execute(&core, "profile_save", &json!({}), true)
            .is_err());
        run("profile_save", json!({"confirm":true}));
        let loaded = vixen_io::load_system(&core.paths).unwrap();
        assert_eq!(loaded.controllers.len(), 1);
        assert!(loaded.tree.len() >= 13);
        run("display_undo", json!({}));
        drop(core);
        std::fs::remove_dir_all(root).unwrap();
    }
}
