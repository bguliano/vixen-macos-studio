//! Studio local MCP adapter. Mutations dispatch through shared vixen-session commands.
pub mod lock;
pub mod socket;
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::Path;
use std::sync::Arc;
use vixen_session::app::AppCore;
use vixen_session::setup::SetupSession;

pub const MAX_MESSAGE: usize = 1024 * 1024;
const TOOLS: &[(&str, &str)] = &[
    ("studio_status", "Profile identity, safety state and implementation capabilities."),
    ("recent_logs", "Read up to limit lines from the newest profile log, bounded to 64 KiB; never follows a log symlink."),
    ("controller_monitor", "Actual execution/frame/controller run statistics; no startup or polling side effects."),
    ("show_list", "List shows and sequential items without running them."),
    ("schedule_list", "List saved schedules, activation state and display revision."),
    ("preview_props", "List native prop IDs and property metadata for every preview. Use returned revision for edits."),
    ("preview_list", "List configured preview IDs and names."),
    ("controller_types", "Available controller types with stable UUIDs."),
    ("display_elements", "List element nodes with stable GUIDs and child GUIDs; paginated."),
    ("controller_list", "List configured controllers and output counts. Does not start output."),
    ("sequence_list", "List sequence filenames in the selected profile."),
    ("media_list", "List media filenames, read-only; does not decode or play audio."),
    ("effect_catalog", "List registered effects and JSON parameter schemas. Unsupported native value kinds have a rejecting schema."),
    ("diagnostics_report", "Report patches, controller/universe errors, and missing media/targets/native color corrections for open sequences."),
];

pub struct Server {
    root: std::path::PathBuf,
    setup: SetupSession,
    initialized: bool,
    client_id: Option<String>,
    core: Option<Arc<AppCore>>,
}
impl Server {
    pub fn open(root: &Path) -> Result<Self, String> {
        let root = root.canonicalize().map_err(|e| e.to_string())?;
        // Load only: unlike AppCore::open this creates no folders/cache and starts no services.
        let system = vixen_io::load_system(&vixen_io::DataPaths::new(root.clone()))
            .map_err(|e| e.to_string())?;
        Ok(Self {
            root,
            setup: SetupSession::from_system(system, &Default::default(), false),
            initialized: false,
            client_id: None,
            core: None,
        })
    }
    pub fn from_core(core: Arc<AppCore>) -> Self {
        let setup =
            SetupSession::from_system(core.system.read().clone(), &Default::default(), false);
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let client = format!(
            "client-{}",
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        core.studio_display.activity("clientConnected", &client);
        Self {
            root: core.paths.root().to_path_buf(),
            setup,
            initialized: false,
            client_id: Some(client),
            core: Some(core),
        }
    }
    pub fn dispatch(&mut self, request: Value) -> Option<Value> {
        let id = request.get("id").cloned();
        let method = request.get("method").and_then(Value::as_str);
        if request.get("jsonrpc") != Some(&json!("2.0"))
            || method.is_none()
            || id
                .as_ref()
                .is_some_and(|v| !v.is_string() && !v.is_number() && !v.is_null())
        {
            return Some(error(Value::Null, -32600, "Invalid Request"));
        }
        let Some(id) = id else {
            return None;
        };
        let p = request.get("params").cloned().unwrap_or(json!({}));
        if !p.is_object() {
            return Some(error(id, -32602, "params must be an object"));
        }
        let method = method.unwrap();
        if !self.initialized && method != "initialize" && method != "ping" {
            return Some(error(id, -32002, "Initialize first"));
        }
        let result = match method {
            "initialize" => {
                if !p["protocolVersion"].is_string()
                    || !p["clientInfo"].is_object()
                    || !p["capabilities"].is_object()
                {
                    return Some(error(id, -32602, "Missing initialize fields"));
                }
                self.initialized = true;
                json!({"protocolVersion":"2024-11-05","capabilities":{"tools":{},"resources":{},"prompts":{}},"serverInfo":{"name":"vixen-studio","version":env!("CARGO_PKG_VERSION")},"instructions":"Shared undoable display and sequence authoring. Hardware requires local permission and explicit confirmation; headless stays muted. Save/export is never implicit."})
            }
            "ping" => json!({}),
            "tools/list" => {
                let mut tools=TOOLS.iter().map(|(name,description)|json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":{"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":500}},"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false,"openWorldHint":false}})).collect::<Vec<_>>();
                tools.extend(vixen_session::studio_display::command_tools());
                tools.extend(vixen_session::studio_sequence::tools());
                tools.extend(vixen_session::studio_live::tools());
                json!({"tools":tools})
            }
            "tools/call" => {
                let name = p["name"].as_str().unwrap_or("");
                if !vixen_session::studio_live::tools()
                    .iter()
                    .any(|t| t["name"] == name)
                    && !vixen_session::studio_sequence::tools()
                        .iter()
                        .any(|t| t["name"] == name)
                    && !TOOLS.iter().any(|(n, _)| *n == name)
                    && !vixen_session::studio_display::command_tools()
                        .iter()
                        .any(|t| t["name"] == name)
                {
                    return Some(error(id, -32602, "Unknown tool"));
                }
                let args = p.get("arguments").cloned().unwrap_or(json!({}));
                if let Some(c) = &self.core {
                    c.studio_display.activity("commandStarted", name);
                }
                let outcome = self.call(name, &args);
                if let Some(c) = &self.core {
                    c.studio_display.activity(
                        if outcome.is_ok() {
                            "commandFinished"
                        } else {
                            "commandFailed"
                        },
                        name,
                    );
                }
                match outcome {
                    Ok(value) => {
                        json!({"content":[{"type":"text","text":value.to_string()}],"isError":false})
                    }
                    Err(message) => {
                        json!({"content":[{"type":"text","text":message}],"isError":true})
                    }
                }
            }
            "resources/list" => {
                json!({"resources":[{"uri":"studio://contract","name":"Studio MCP contract","mimeType":"text/markdown"}]})
            }
            "resources/read" if p["uri"] == "studio://contract" => {
                json!({"contents":[{"uri":"studio://contract","mimeType":"text/markdown","text":include_str!("../../../studio/MCP.md")}]})
            }
            "resources/read" => return Some(error(id, -32602, "Unknown resource")),
            "prompts/list" => {
                json!({"prompts":[{"name":"build_show","description":"Safe music-synchronized show workflow and current capability limits"}]})
            }
            "prompts/get" if p["name"] == "build_show" => {
                json!({"messages":[{"role":"user","content":{"type":"text","text":"Inspect studio_status, display_elements, controller_list, preview_list and diagnostics_report. Analyze real profile audio with music_analyze. Create a sequence with media, place effects on detected beats, add marks, then sequence_render and preview_frame. Validate diagnostics and render results before confirmed sequence_save, profile_save and sequence_export_fseq. Use shared undo to correct edits. Never enable hardware merely to author a show. Scheduling tools create disabled drafts for local review. Unsupported parameter codecs and heuristic onset/section estimates must not be misrepresented as exact."}}]})
            }
            _ => return Some(error(id, -32601, "Method not found")),
        };
        Some(json!({"jsonrpc":"2.0","id":id,"result":result}))
    }
    fn call(&mut self, name: &str, args: &Value) -> Result<Value, String> {
        if vixen_session::studio_live::tools()
            .iter()
            .any(|t| t["name"] == name)
        {
            let core = self
                .core
                .as_ref()
                .ok_or("Live output requires the running app, not a headless snapshot")?;
            return vixen_session::studio_live::execute(core, name, args);
        }
        if vixen_session::studio_sequence::tools()
            .iter()
            .any(|t| t["name"] == name)
        {
            if self.core.is_none() {
                self.core = Some(Arc::new(
                    AppCore::open(self.root.clone(), true).map_err(|e| e.to_string())?,
                ));
            }
            return vixen_session::studio_sequence::execute(
                self.core.as_ref().unwrap(),
                name,
                args,
                true,
            );
        }

        if vixen_session::studio_display::command_tools()
            .iter()
            .any(|t| t["name"] == name)
        {
            if self.core.is_none() {
                self.core = Some(Arc::new(
                    AppCore::open(self.root.clone(), true).map_err(|e| e.to_string())?,
                ));
            }
            let core = self.core.as_ref().unwrap();
            return core.studio_display.execute(core, name, args, true);
        }
        if let Some(core) = &self.core {
            self.setup =
                SetupSession::from_system(core.system.read().clone(), &Default::default(), false);
        }
        let obj = args.as_object().ok_or("arguments must be an object")?;
        if obj.keys().any(|k| k != "offset" && k != "limit") {
            return Err("Unknown argument".into());
        }
        let number = |key: &str, default: u64| -> Result<usize, String> {
            match args.get(key) {
                None => Ok(default as usize),
                Some(v) => v
                    .as_u64()
                    .and_then(|n| usize::try_from(n).ok())
                    .ok_or(format!("{key} must be a nonnegative integer")),
            }
        };
        let offset = number("offset", 0)?;
        let limit = number("limit", 100)?;
        if !(1..=500).contains(&limit) {
            return Err("limit must be 1..500".into());
        }
        let s = &self.setup;
        let items:Vec<Value> = match name {
            "studio_status" => return Ok(json!({"profile":self.root,"mode":if self.core.is_some(){"liveSession"}else{"headlessSnapshot"},"liveOutputAllowed":self.core.as_ref().is_some_and(|c|vixen_session::studio_live::allowed(c)),"mutationsAvailable":true,"revision":self.core.as_ref().map_or(0,|c|c.display.revision()),"version":env!("CARGO_PKG_VERSION"),"elements":s.tree.len(),"controllers":s.controllers.len()})),
            "display_elements" => s.tree.live_nodes().map(|(_,n)|json!({"id":n.id.to_string(),"name":n.name,"elementId":n.element_id.map(|i|i.to_string()),"children":n.children.iter().map(|id|s.tree.node(*id).id.to_string()).collect::<Vec<_>>()})).collect(),
            "show_list" => vixen_session::shows::load_shows(&s.system.module_store).iter().map(|show|json!({"id":show.id.to_string(),"name":show.name_text(),"items":show.items.iter().map(|item|json!({"name":item.name_text(),"type":item.item_type.text(),"action":item.action.text(),"sequence":item.sequence_file()})).collect::<Vec<_>>()})).collect(),
            "schedule_list" => {let data=vixen_session::scheduler::load_scheduler(&s.system.module_store);return Ok(json!({"enabled":data.is_enabled,"revision":self.core.as_ref().map_or(0,|c|c.display.revision()),"items":data.items.iter().enumerate().map(|(index,item)|json!({"index":index,"showId":item.show_id.to_string(),"enabled":item.enabled,"schedule":item.schedule_string(),"days":item.days})).collect::<Vec<_>>()}));},
            "preview_props" => {let mut props=Vec::new();for entry in vixen_session::preview::preview_entries(&s.system){if let Ok(data)=vixen_session::preview::load_preview_data(&s.system.module_store,&entry){let design=vixen_session::render_support::design::DesignSession::new(data,Some(Arc::new(s.system.tree.clone())));for item in &design.data().display_items{props.push(json!({"previewId":entry.id.to_string(),"propId":item.id.0,"name":item.shape.name(),"kind":item.shape.type_name(),"properties":design.shape_properties(item.id).iter().map(|p|json!({"id":p.id,"name":p.name,"kind":format!("{:?}",p.kind),"value":format!("{:?}",p.value),"readOnly":p.read_only})).collect::<Vec<_>>()}));}}}props},
            "preview_list" => vixen_session::preview::preview_entries(&s.system).iter().map(|p|{let links=vixen_session::preview::load_preview_data(&s.system.module_store,p).ok().map(|data|vixen_session::render_support::Scene::build(&data,Some(&s.system.tree)).nodes().iter().take(500).map(|id|id.to_string()).collect::<Vec<_>>()).unwrap_or_default();json!({"id":p.id.to_string(),"name":p.name,"linkedNodeIds":links})}).collect(),
            "controller_types" => vixen_session::setup::controllers::controller_types().iter().map(|(id,name)|json!({"id":id.to_string(),"name":name})).collect(),
            "controller_list" => s.controllers.iter().filter(|c|c.alive).map(|c|json!({"id":c.id.to_string(),"name":c.name,"type":c.type_name(),"typeId":c.type_id.to_string(),"outputs":c.outputs.len(),"configuration":vixen_session::studio_diagnostics::controller_configuration(c),"running":self.core.as_ref().and_then(|core|core.playback.running_controllers()).is_some_and(|ids|ids.contains(&c.id))})).collect(),
            "sequence_list" | "media_list" => {
                let folder=self.root.join(if name=="sequence_list" {"Sequence"} else {"Media"});
                let mut paths=std::fs::read_dir(folder).map_err(|e|e.to_string())?.map(|e|e.map(|e| e.file_name().to_string_lossy().into_owned())).collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
                paths.sort(); paths.into_iter().map(|p|json!({"name":p})).collect()
            }
            "effect_catalog" => vixen_effect_registry::all().iter().map(|d|json!({"id":d.type_id.to_string(),"name":d.name,"description":d.description,"parameters":d.params.iter().map(|p|{let mut o=json!({"name":p.name,"label":p.label,"description":p.description,"schema":vixen_session::studio_params::schema(p.kind)});if let Some(n)=vixen_session::studio_params::note(d.name,p.name){o["note"]=json!(n);}o}).collect::<Vec<_>>()})).collect(),
            "diagnostics_report" => return Ok(vixen_session::studio_diagnostics::report(s,self.core.as_deref())),
            "recent_logs" => return vixen_session::studio_diagnostics::recent_logs(&self.root,limit),
            "controller_monitor" => return Ok(self.core.as_ref().map(|c|vixen_session::studio_diagnostics::monitor(c)).unwrap_or(json!({"execution":"Not started","controllers":[]}))),
            _ => return Err("Unknown tool".into()),
        };
        let total = items.len();
        let page: Vec<_> = items.into_iter().skip(offset).take(limit).collect();
        Ok(
            json!({"revision":self.core.as_ref().map_or(0,|c|c.display.revision()),"items":page,"total":total,"nextOffset":if offset.saturating_add(limit)<total {Some(offset+limit)} else {None}}),
        )
    }
}
fn error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}
pub fn serve(
    server: &mut Server,
    mut input: impl BufRead,
    mut output: impl Write,
) -> std::io::Result<()> {
    loop {
        let mut bytes = Vec::new();
        let read = std::io::Read::take(&mut input, (MAX_MESSAGE + 1) as u64)
            .read_until(b'\n', &mut bytes)?;
        if read == 0 {
            return Ok(());
        }
        if bytes.len() > MAX_MESSAGE {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "MCP message exceeds 1 MiB",
            ));
        }
        let response = match serde_json::from_slice(&bytes) {
            Ok(v) => server.dispatch(v),
            Err(_) => Some(error(Value::Null, -32700, "Parse error")),
        };
        if let Some(value) = response {
            let encoded = value.to_string();
            if encoded.len() > 8 * MAX_MESSAGE {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Response exceeds 8 MiB; reduce limit",
                ));
            }
            writeln!(output, "{encoded}")?;
            output.flush()?;
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let (Some(core), Some(id)) = (&self.core, &self.client_id) {
            core.studio_display.activity("clientDisconnected", id);
        }
    }
}
