//! {{NAME}}: a Vixen Studio plugin (plugin API 1.0), made by plugin new / plugin_scaffold.
//!
//! src/vixen_plugin_sdk.rs speaks the protocol; this file is only your plugin. Only protocol messages go to stdout:
//! use eprintln! for anything else, which Studio keeps in the plugin's log (plugin_logs). Build with ./build.sh, which
//! puts the program in bin/, where vixen-plugin.json's entry points. The protocol reference: the MCP resource
//! plugins://api. Replace the parts marked TODO.
mod vixen_plugin_sdk;

use serde_json::{json, Value};
use std::time::Instant;
use vixen_plugin_sdk::{ConfigError, Controller, Frame, Instance, Plugin};

/// One controller of this type in the show. vixen-plugin.json declares its settings (configSchema).
struct Output {
    prefix: String,
    last_log: Option<Instant>,
}

impl Controller for Output {
    fn configure(&mut self, _instance: &Instance, config: &Value) -> Result<Option<Value>, ConfigError> {
        // Called on create and after every edit, with settings already checked against configSchema and defaults
        // filled in. TODO: check anything the schema can't say; return Err(ConfigError::new("/prefix", "why")).
        self.prefix = config["prefix"].as_str().unwrap_or_default().to_string();
        Ok(None)
    }

    fn start(&mut self, instance: &Instance) -> Result<(), String> {
        // Live output began. TODO: connect to your device here.
        instance.status("running", None);
        Ok(())
    }

    fn frame(&mut self, _instance: &Instance, frame: &Frame) {
        // TODO: send frame.levels (one 0-255 level per output) to your device. This prints them twice a second.
        if self.last_log.is_none_or(|t| t.elapsed().as_millis() >= 500) {
            self.last_log = Some(Instant::now());
            let values: Vec<String> = frame.levels.iter().take(32).map(u16::to_string).collect();
            eprintln!("{}{}", self.prefix, values.join(" "));
        }
    }

    fn stop(&mut self, _instance: &Instance) {
        // Live output ended. TODO: disconnect.
    }
}

fn main() {
    Plugin::new()
        .controller(None, |_| Box::new(Output { prefix: String::new(), last_log: None }))
        // TODO: your agent tools, as listed in vixen-plugin.json's contributes.tools.
        .tool("hello", |_, args| {
            let who = args["name"].as_str().filter(|s| !s.is_empty()).unwrap_or("there");
            Ok(json!(format!("Hello, {who}, from {{NAME}}.")))
        })
        .run();
}
