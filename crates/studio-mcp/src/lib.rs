//! Studio's bounded, newline-delimited MCP adapter. No mutation or hardware path.
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::Path;
use vixen_session::setup::{flow::CompKind, SetupSession};

pub const MAX_MESSAGE: usize = 1024 * 1024;
const TOOLS: &[(&str, &str)] = &[
    ("studio_status", "Profile identity, safety state and implementation capabilities."),
    ("display_elements", "List element nodes with stable GUIDs and child GUIDs; paginated."),
    ("controller_list", "List configured controllers and output counts. Does not start output."),
    ("sequence_list", "List sequence filenames in the selected profile."),
    ("media_list", "List media filenames, read-only; does not decode or play audio."),
    ("effect_catalog", "List registered effects and parameter metadata. Kind is Vixen's native parameter kind, not a writable schema."),
    ("diagnostics_report", "Report unpatched elements, unused outputs and invalid patch references. Other diagnostic checks are explicitly not covered."),
];

pub struct Server {
    root: std::path::PathBuf,
    setup: SetupSession,
    initialized: bool,
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
        })
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
                json!({"protocolVersion":"2024-11-05","capabilities":{"tools":{},"resources":{},"prompts":{}},"serverInfo":{"name":"vixen-studio","version":env!("CARGO_PKG_VERSION")},"instructions":"Read-only milestone. No live output or mutations are exposed."})
            }
            "ping" => json!({}),
            "tools/list" => {
                json!({"tools":TOOLS.iter().map(|(name, description)| json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":{"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":500}},"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false,"openWorldHint":false}})).collect::<Vec<_>>()})
            }
            "tools/call" => {
                let name = p["name"].as_str().unwrap_or("");
                if !TOOLS.iter().any(|(n, _)| *n == name) {
                    return Some(error(id, -32602, "Unknown tool"));
                }
                let args = p.get("arguments").cloned().unwrap_or(json!({}));
                match self.call(name, &args) {
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
                json!({"messages":[{"role":"user","content":{"type":"text","text":"Inspect studio_status, display_elements, controller_list and diagnostics_report. Plan music analysis, beat marks, effects, rendering and validation before confirmed saving/export. This milestone is read-only: do not claim to create shows or enable hardware. Request a mutation-capable Studio release for execution."}}]})
            }
            _ => return Some(error(id, -32601, "Method not found")),
        };
        Some(json!({"jsonrpc":"2.0","id":id,"result":result}))
    }
    fn call(&self, name: &str, args: &Value) -> Result<Value, String> {
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
            "studio_status" => return Ok(json!({"profile":self.root,"mode":"readOnlySnapshot","liveOutputAllowed":false,"mutationsAvailable":false,"version":env!("CARGO_PKG_VERSION"),"elements":s.tree.len(),"controllers":s.controllers.len()})),
            "display_elements" => s.tree.live_nodes().map(|(_,n)|json!({"id":n.id.to_string(),"name":n.name,"elementId":n.element_id.map(|i|i.to_string()),"children":n.children.iter().map(|id|s.tree.node(*id).id.to_string()).collect::<Vec<_>>()})).collect(),
            "controller_list" => s.controllers.iter().filter(|c|c.alive).map(|c|json!({"id":c.id.to_string(),"name":c.name,"type":c.type_name(),"typeId":c.type_id.to_string(),"outputs":c.outputs.len(),"running":false})).collect(),
            "sequence_list" | "media_list" => {
                let folder=self.root.join(if name=="sequence_list" {"Sequence"} else {"Media"});
                let mut paths=std::fs::read_dir(folder).map_err(|e|e.to_string())?.map(|e|e.map(|e| e.file_name().to_string_lossy().into_owned())).collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
                paths.sort(); paths.into_iter().map(|p|json!({"name":p})).collect()
            }
            "effect_catalog" => vixen_effect_registry::all().iter().map(|d|json!({"id":d.type_id.to_string(),"name":d.name,"description":d.description,"parameters":d.params.iter().map(|p|json!({"name":p.name,"label":p.label,"description":p.description,"kind":format!("{:?}",p.kind).split(['(', '{']).next().unwrap_or("Unknown").trim()})).collect::<Vec<_>>()})).collect(),
            "diagnostics_report" => {
                let unpatched=s.flow.components().filter(|(id,c)|c.kind==CompKind::Element && s.flow.count_patch_points(*id).0==0).count();
                let unused=s.flow.components().filter(|(_,c)|c.kind==CompKind::Output && c.source.is_none()).count();
                return Ok(json!({"unpatchedElements":unpatched,"unusedOutputs":unused,"invalidPatches":s.warnings.iter().map(|w|format!("{w:?}")).collect::<Vec<_>>(),"notChecked":["overlapping universes","missing sequence media","effect color compatibility","controller monitor"]}));
            }
            _ => return Err("Unknown tool".into()),
        };
        let total = items.len();
        let page: Vec<_> = items.into_iter().skip(offset).take(limit).collect();
        Ok(
            json!({"items":page,"total":total,"nextOffset":if offset.saturating_add(limit)<total {Some(offset+limit)} else {None}}),
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
