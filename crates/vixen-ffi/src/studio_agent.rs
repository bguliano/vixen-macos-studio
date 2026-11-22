//! Studio-only live command/event boundary; no Swift view ownership or styling.
use crate::core::{FfiError, VixenCore};
use crate::sequence::SequencerSession;
use crate::setup::DisplaySetupSession;
use std::sync::Arc;

#[derive(Clone, uniffi::Record)]
pub struct StudioChangeEvent {
    pub document_id: Option<String>,
    pub revision: u64,
    pub kind: String,
    pub command: String,
    pub agent: bool,
}
#[uniffi::export(callback_interface)]
pub trait StudioChangeObserver: Send + Sync {
    fn changed(&self, event: StudioChangeEvent);
}

#[uniffi::export]
impl VixenCore {
    /// Execute a UI command through the exact service MCP uses. JSON object arguments use stable UUIDs.
    /// Agent identity is never taken from JSON. MCP supplies it through the Rust-only adapter.
    pub fn studio_display_command(
        &self,
        command: String,
        arguments_json: String,
    ) -> Result<String, FfiError> {
        let args: serde_json::Value =
            serde_json::from_str(&arguments_json).map_err(|e| FfiError::new(e.to_string()))?;
        if !args.is_object() {
            return Err(FfiError::new("arguments must be an object"));
        }
        self.inner
            .studio_display
            .execute(&self.inner, &command, &args, false)
            .map(|v| v.to_string())
            .map_err(FfiError::new)
    }
    pub fn studio_stop_observing_display(&self) {
        self.inner.studio_display.unsubscribe();
    }
    pub fn studio_display_command_catalog(&self) -> String {
        serde_json::Value::Array(vixen_session::studio_display::command_tools()).to_string()
    }
    pub fn studio_display_snapshot(&self) -> String {
        self.inner.studio_display.snapshot(&self.inner).to_string()
    }
    pub fn studio_display_history(&self) -> String {
        self.inner.studio_display.history().to_string()
    }
    /// Callback is on the command thread, outside engine locks: dispatch to MainActor in Swift.
    pub fn studio_observe_display(&self, observer: Box<dyn StudioChangeObserver>) {
        self.inner.studio_display.subscribe(Arc::new(move |e| {
            observer.changed(StudioChangeEvent {
                document_id: e.document_id,
                revision: e.revision,
                kind: e.kind,
                command: e.command,
                agent: e.agent,
            })
        }));
    }
    pub fn studio_display_events(&self, after_revision: u64) -> Vec<StudioChangeEvent> {
        self.inner
            .studio_display
            .events(after_revision)
            .into_iter()
            .map(|e| StudioChangeEvent {
                document_id: e.document_id,
                revision: e.revision,
                kind: e.kind,
                command: e.command,
                agent: e.agent,
            })
            .collect()
    }
}

/// Installed once per AppCore. Startup failures remain visible through the explicit FFI method.
pub(crate) fn start_server(core: &Arc<vixen_session::app::AppCore>) -> Result<(), FfiError> {
    let mut runtime = core.studio_display.runtime.lock();
    if runtime.is_none() {
        *runtime = Some(Box::new(
            studio_mcp::socket::SocketServer::start(core)
                .map_err(|e| FfiError::new(e.to_string()))?,
        ));
    }
    Ok(())
}
#[uniffi::export]
impl VixenCore {
    pub fn studio_start_mcp(&self) -> Result<(), FfiError> {
        start_server(&self.inner)
    }
}

#[uniffi::export]
impl VixenCore {
    pub fn studio_sequence_command(
        &self,
        command: String,
        arguments_json: String,
    ) -> Result<String, FfiError> {
        let args: serde_json::Value =
            serde_json::from_str(&arguments_json).map_err(|e| FfiError::new(e.to_string()))?;
        vixen_session::studio_sequence::execute(&self.inner, &command, &args, false)
            .map(|v| v.to_string())
            .map_err(FfiError::new)
    }
    pub fn studio_sequence_command_catalog(&self) -> String {
        serde_json::Value::Array(vixen_session::studio_sequence::tools()).to_string()
    }
    pub fn studio_open_sequence_session(
        &self,
        sequence_id: String,
    ) -> Result<Arc<crate::sequence::SequencerSession>, FfiError> {
        let id = uuid::Uuid::parse_str(&sequence_id).map_err(|e| FfiError::new(e.to_string()))?;
        let shared =
            vixen_session::studio_sequence::find(&self.inner, id).map_err(FfiError::new)?;
        Ok(crate::sequence::SequencerSession::studio_wrap_shared(
            self.inner.clone(),
            shared,
        ))
    }
}

#[uniffi::export]
impl SequencerSession {
    /// A Studio sequence command (studioSequenceCommand) on this window's own document, its sequenceId filled in, so a
    /// sequencer sheet runs the same authoring code as the MCP tool and lands as one step on the window's Undo.
    pub fn studio_command(&self, command: String, arguments_json: String) -> Result<String, FfiError> {
        let mut args: serde_json::Value =
            serde_json::from_str(&arguments_json).map_err(|e| FfiError::new(e.to_string()))?;
        let object = args.as_object_mut().ok_or_else(|| FfiError::new("arguments must be a JSON object"))?;
        let id = self.session.lock().playback_id().to_string();
        object.insert("sequenceId".into(), serde_json::Value::String(id));
        vixen_session::studio_sequence::execute(&self.core, &command, &args, false)
            .map(|v| v.to_string())
            .map_err(FfiError::new)
    }
}

#[uniffi::export]
impl VixenCore {
    pub fn studio_allow_agents_live_output(&self) -> bool {
        vixen_session::studio_live::allowed(&self.inner)
    }
    pub fn studio_set_allow_agents_live_output(&self, allowed: bool) {
        vixen_session::studio_live::set_allowed_locally(&self.inner, allowed);
    }
    /// Changes whenever the Effects library's favorites or recents do (`effects_library`, the star, a placement), so the
    /// sequencer's poll reads them again only then. No locks, no file access.
    pub fn studio_effects_library_revision(&self) -> u64 {
        vixen_session::studio_effects_library::revision()
    }
}

#[uniffi::export]
impl VixenCore {
    /// The Test submenu for one element row or one controller of the Display pane's working copy, as JSON
    /// (`studio_commission::menu`): {name, lights, problem?, block?, running?, defaultBrightnessPct}, with the element's or
    /// controller's id added as targetId or controllerId.
    pub fn studio_commission_menu(
        &self,
        session: Arc<crate::setup::DisplaySetupSession>,
        element_row: Option<String>,
        controller_id: Option<String>,
    ) -> String {
        let working = session.inner.read();
        let target = element_row
            .and_then(|row| working.tree.resolve(&row).and_then(|path| path.last().copied()))
            .map(|node| working.tree.node(node).id);
        let controller = controller_id.and_then(|id| uuid::Uuid::parse_str(&id).ok());
        let mut menu = vixen_session::studio_commission::menu(&self.inner, &working, target, controller);
        if let Some(id) = target {
            menu["targetId"] = serde_json::json!(id.to_string());
        }
        if let Some(id) = controller {
            menu["controllerId"] = serde_json::json!(id.to_string());
        }
        menu.to_string()
    }
    /// Starts the user's commissioning test from {targetId | controllerId, pattern, brightnessPct}: the code the MCP
    /// `output_test_pattern` tool runs, gated by Live Output instead of the agent switch. Returns what's running as JSON.
    pub fn studio_commission_start(&self, arguments_json: String) -> Result<String, FfiError> {
        let args: serde_json::Value =
            serde_json::from_str(&arguments_json).map_err(|e| FfiError::new(e.to_string()))?;
        if let Some(block) = vixen_session::studio_commission::local_block(&self.inner) {
            return Err(FfiError::new(block.to_string()));
        }
        let result = vixen_session::studio_commission::start(
            &self.inner,
            vixen_session::studio_commission::Origin::Local,
            &args,
        )
        .map_err(FfiError::new)?;
        self.inner.studio_display.activity("outputChanged", "commission_start");
        Ok(result.to_string())
    }
    /// Stops the running commissioning test, whoever started it. False when nothing was running.
    pub fn studio_commission_stop(&self) -> bool {
        let stopped = vixen_session::studio_commission::stop(&self.inner);
        self.inner.studio_display.activity("outputChanged", "commission_stop");
        stopped
    }
    /// The running commissioning test as JSON {pattern, title, name, kind, lights, brightnessPct, remainingMs, agent, ...},
    /// or nil.
    pub fn studio_commission_status(&self) -> Option<String> {
        vixen_session::studio_commission::status(&self.inner).map(|v| v.to_string())
    }
}

#[uniffi::export]
impl VixenCore {
    pub fn studio_agent_snapshot(&self) -> String {
        self.inner.studio_display.activity_snapshot().to_string()
    }
    pub fn studio_mcp_is_running(&self) -> bool {
        self.inner.studio_display.runtime.lock().is_some()
    }
    /// Drops the MCP listener, which removes its socket, without the rest of shutdown. For exits that skip `shutdown`, such as
    /// the UI test harness's `quit`. Commands already running finish; new connections are refused.
    pub fn studio_stop_mcp(&self) {
        self.inner.studio_display.runtime.lock().take();
    }
    /// Logs why the app is exiting without shutting down, and waits for the line to reach the disk: `exit` follows at once.
    pub fn studio_log_exit(&self, status: i32, reason: String) {
        vixen_session::logging::info(
            "VixenApplication.Program",
            &format!("{reason}; exiting with status {status} without shutting down."),
        );
        vixen_session::logging::flush();
    }
}

#[uniffi::export]
impl VixenCore {
    /// Publishes the live Display pane's working copy as one shared-undo transaction, named `label` when given. Returns JSON
    /// {changed, revision, label?}.
    /// Fails with "Display changed; refresh before retrying" when the display moved past `expected_revision`, or with
    /// "Live Output is on" while non-muted execution is open.
    pub fn studio_publish_display_setup(
        &self,
        session: Arc<crate::setup::DisplaySetupSession>,
        expected_revision: u64,
        cleanup: bool,
        label: Option<String>,
    ) -> Result<String, FfiError> {
        let mut working = session.inner.write();
        self.inner
            .studio_display
            .publish_setup(&self.inner, &mut working, expected_revision, cleanup, label.as_deref())
            .map(|v| v.to_string())
            .map_err(FfiError::new)
    }
    /// Why local display edits are paused right now, or nil.
    pub fn studio_display_edit_block(&self) -> Option<String> {
        self.inner.studio_display.ui_edit_block(&self.inner).map(Into::into)
    }
}

#[uniffi::export]
impl DisplaySetupSession {
    /// The controller inspector's settings for one controller of this working copy, as JSON
    /// (`studio_controller_network::info`), or nil when it's gone.
    pub fn studio_controller_network(&self, id: String) -> Option<String> {
        let id = uuid::Uuid::parse_str(&id).ok()?;
        let working = self.inner.read();
        let slot = working.slot_of(id)?;
        Some(vixen_session::studio_controller_network::info(&working.controllers[slot as usize]).to_string())
    }
    /// `controller_configure` on this working copy: the same code the MCP tool runs. Returns JSON {controller, newId?}.
    pub fn studio_configure_controller_network(&self, arguments_json: String) -> Result<String, FfiError> {
        let args: serde_json::Value =
            serde_json::from_str(&arguments_json).map_err(|e| FfiError::new(e.to_string()))?;
        // `configure` checks every field before changing anything, so a refused edit leaves the working copy as it was.
        let mut working = self.inner.write();
        vixen_session::studio_controller_network::configure(&mut working, &args)
            .map(|v| v.to_string())
            .map_err(FfiError::new)
    }
    /// The Patch submenu for one element row of this working copy, as JSON (`studio_patch::menu`): where it's patched as
    /// universe:channel ranges, whether it can be reversed, what it can swap with and where it can move. Nil when the row is gone.
    pub fn studio_patch_menu(&self, row: String) -> Option<String> {
        let working = self.inner.read();
        let node = *working.tree.resolve(&row)?.last()?;
        let id = working.tree.node(node).id;
        vixen_session::studio_patch::menu(&working, id).map(|v| v.to_string())
    }
    /// Colour handling on this working copy: {ids:[UUID], mode:"rgb"|"rgbw"|"single", color?:"#RRGGBB"}, the code the MCP
    /// `element_color` tool and the template's `color` field run. Returns JSON {mode, updatedLeaves, filters, channelsPerPixel}.
    pub fn studio_apply_color_handling(&self, arguments_json: String) -> Result<String, FfiError> {
        let args: serde_json::Value =
            serde_json::from_str(&arguments_json).map_err(|e| FfiError::new(e.to_string()))?;
        let mut working = self.inner.write();
        vixen_session::studio_color::apply_to_ids(&mut working, &args)
            .map(|v| v.to_string())
            .map_err(FfiError::new)
    }
    /// The previews a template's new prop can be shown in, as JSON [{id, name}], in the profile's order.
    pub fn studio_preview_choices(&self) -> String {
        let working = self.inner.read();
        serde_json::Value::from(
            vixen_session::preview::preview_entries(&working.system)
                .into_iter()
                .map(|p| serde_json::json!({"id":p.id.to_string(),"name":p.name}))
                .collect::<Vec<_>>(),
        )
        .to_string()
    }
    /// A preview shape for an element of this working copy, already linked light for light: {previewId, elementId,
    /// kind:"megatree"|"matrix"|"arch"}, the code the MCP `element_template` previewId and `preview_prop_create` elementId run.
    /// Returns JSON {previewId, propId, shape, strings, lightsPerString, linkedLights}. A refusal changes nothing.
    pub fn studio_add_preview_shape(&self, arguments_json: String) -> Result<String, FfiError> {
        let args: serde_json::Value =
            serde_json::from_str(&arguments_json).map_err(|e| FfiError::new(e.to_string()))?;
        let uuid = |key: &str| {
            args[key].as_str().and_then(|s| uuid::Uuid::parse_str(s).ok()).ok_or_else(|| FfiError::new(format!("{key} must be a UUID")))
        };
        let (preview, element) = (uuid("previewId")?, uuid("elementId")?);
        let kind = args["kind"].as_str().unwrap_or_default();
        let mut working = self.inner.write();
        vixen_session::studio_props::add_linked(&mut working, preview, element, kind, None)
            .map(|v| v.to_string())
            .map_err(FfiError::new)
    }
}

#[uniffi::export]
impl DisplaySetupSession {
    /// The Megatree and Pixel Grid sheets' generator: the native template with Studio's wiring (start corner as drawn,
    /// reverse every other string, `hidden_pixels` per string in their own group), as `studio_wiring::generate`, the code
    /// the MCP `element_template` tool runs. Other templates generate natively. Selects the prop and its hidden group.
    pub fn studio_generate_template(
        &self,
        draft: crate::setup::SetupTemplateDraft,
        hidden_pixels: u32,
    ) -> Result<crate::setup::SetupTemplateOutcome, FfiError> {
        use vixen_session::setup::templates::{TemplateError, BAD_PATTERN_MESSAGE};
        let mut guard = self.inner.write();
        let s = &mut *guard;
        let built = vixen_session::studio_wiring::generate(&mut s.tree, &crate::setup::params_of(&draft), hidden_pixels)
            .map_err(|e| match e {
                TemplateError::EmptyName => FfiError::new("Could not create elements."),
                TemplateError::BadPattern => FfiError::new(BAD_PATTERN_MESSAGE),
            })?;
        let heads: Vec<_> = built.result.head.into_iter().chain(built.hidden_head).collect();
        let created = built.result.created.clone();
        s.apply_template_result(built.result);
        let mut edit = vixen_session::setup::elements::EditResult { reload_roots: true, scroll_to_top: true, ..Default::default() };
        edit.select = (!heads.is_empty()).then(|| heads.iter().map(|h| s.row_path_of(*h)).collect());
        let created_paths = created.iter().map(|n| s.row_path_of(*n)).collect();
        Ok(crate::setup::SetupTemplateOutcome {
            edit: crate::setup::edit_result(s, edit),
            created: created.iter().map(|n| n.0).collect(),
            created_paths,
        })
    }
}

/// The template sheet's wiring diagram for these fields, as JSON (`studio_wiring::plan`): the strings in wire order,
/// the start corner's labels and the sentences under the diagram.
#[uniffi::export]
pub fn studio_template_wiring(draft: crate::setup::SetupTemplateDraft, hidden_pixels: u32) -> String {
    vixen_session::studio_wiring::plan(&crate::setup::params_of(&draft), hidden_pixels).to_string()
}

/// The commit this app was built from, as JSON {commit, date, dirty} (`studio_mcp::build`; About, #71).
#[uniffi::export]
pub fn studio_build_info() -> String {
    studio_mcp::build().to_string()
}

/// The template Colour picker's choices as JSON: [{mode, label, channelsPerPixel, note}].
#[uniffi::export]
pub fn studio_color_choices() -> String {
    vixen_session::studio_color::choices().to_string()
}

/// The Color Handling helper's one-line note for a colour order such as "RGBW".
#[uniffi::export]
pub fn studio_color_order_note(order: String) -> String {
    vixen_session::studio_color::order_note(&order)
}

#[uniffi::export]
impl VixenCore {
    /// The controller inspector's status line for a published controller, as JSON (`studio_controller_network::status`).
    pub fn studio_controller_status(&self, id: String) -> String {
        match uuid::Uuid::parse_str(&id) {
            Ok(id) => vixen_session::studio_controller_network::status(&self.inner, id).to_string(),
            Err(_) => serde_json::json!({"text":""}).to_string(),
        }
    }
    /// The Controllers section's list for the live pane's working copy, as JSON (`studio_controller_network::overview`),
    /// the code the MCP `controller_overview` tool runs: run state, address, universes, patching and output health.
    pub fn studio_controller_overview(&self, session: Arc<crate::setup::DisplaySetupSession>) -> String {
        let working = session.inner.read();
        vixen_session::studio_controller_network::overview(&working, Some(&self.inner)).to_string()
    }
    /// Whether anything answers at an IPv4 address (TCP port 80; no lighting data). Refused while muted. Blocks up to 0.8 s.
    pub fn studio_probe_controller(&self, address: String) -> String {
        if !self.inner.playback.hardware_allowed() {
            return serde_json::json!({"reachable":null,"text":"Muted: reachability checks are off"}).to_string();
        }
        vixen_session::studio_controller_network::probe(&address).to_string()
    }
}

/// The command palette's menu list for one kind of front window ("show" or "sequencer"), as JSON
/// `[{path:[..], shortcut, enabled, checked}]`: kept for `studio_palette_search` and the MCP `command_palette` tool.
#[uniffi::export]
pub fn studio_palette_publish(window: String, entries_json: String) -> Result<u32, FfiError> {
    let value: serde_json::Value = serde_json::from_str(&entries_json).map_err(|e| FfiError::new(e.to_string()))?;
    let entries = vixen_session::studio_palette::parse_entries(&value).map_err(FfiError::new)?;
    let count = entries.len() as u32;
    vixen_session::studio_palette::publish(&window, entries);
    Ok(count)
}

/// Ranked palette matches for `query` among what was last published for `window`, as the MCP tool returns them:
/// JSON {window,total,disabled,matches:[{index,title,path,shortcut,enabled,checked,reason?,agentTool?}]}.
#[uniffi::export]
pub fn studio_palette_search(window: String, query: String, limit: u32) -> Result<String, FfiError> {
    vixen_session::studio_palette::execute(&serde_json::json!({"window":window,"query":query,"limit":limit.clamp(1, 500)}))
        .map(|v| v.to_string())
        .map_err(FfiError::new)
}
