//! Plugin-contributed agent tools as MCP tools. Each plugin tool is listed as `plugin__<short-id>__<tool>`: the short
//! id is the last part of the plugin id (`io.github.someone.govee-output` → `govee_output`), or the whole id when two
//! plugins share a last part. A call is proxied to the plugin as `tool.call`.
use crate::manifest::Tool;
use serde_json::{json, Value};
use std::time::Duration;

pub const PREFIX: &str = "plugin__";
/// How long a plugin has to answer `tool.call` (discovery on a network takes a while).
pub const CALL_TIMEOUT: Duration = Duration::from_secs(30);
/// Longer names are cut to fit what MCP clients accept.
const MAX_NAME: usize = 64;

/// One plugin tool as MCP clients see it.
#[derive(Clone, Debug, PartialEq)]
pub struct Contributed {
    pub mcp_name: String,
    pub plugin_id: String,
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub read_only: bool,
}

impl Contributed {
    /// The `tools/list` entry.
    pub fn to_mcp(&self) -> Value {
        let mut schema = self.input_schema.clone();
        if !schema.is_object() || schema.get("type").is_none() {
            // An empty manifest schema means "no arguments"; MCP wants an object schema.
            let properties = schema.get("properties").cloned().unwrap_or_else(|| json!({}));
            schema = json!({"type": "object", "properties": properties});
        }
        json!({
            "name": self.mcp_name,
            "description": format!("[plugin {}] {}", self.plugin_id, self.description),
            "inputSchema": schema,
            "annotations": {"readOnlyHint": self.read_only, "destructiveHint": !self.read_only, "openWorldHint": true},
        })
    }
}

fn sanitize(text: &str) -> String {
    text.chars().map(|c| if c.is_ascii_lowercase() || c.is_ascii_digit() { c } else { '_' }).collect()
}

fn short_id(id: &str) -> String {
    sanitize(id.rsplit('.').next().unwrap_or(id))
}

/// Names every plugin's tools, plugins in id order.
pub fn name_all(plugins: Vec<(String, Vec<Tool>)>) -> Vec<Contributed> {
    let shorts: Vec<String> = plugins.iter().map(|(id, _)| short_id(id)).collect();
    let mut out = Vec::new();
    for (index, (id, tools)) in plugins.into_iter().enumerate() {
        let shared = shorts.iter().filter(|s| **s == shorts[index]).count() > 1;
        let base = if shared { sanitize(&id) } else { shorts[index].clone() };
        for tool in tools {
            let room = MAX_NAME.saturating_sub(PREFIX.len() + 2 + tool.name.len()).max(1);
            let short = &base[..room.min(base.len())];
            out.push(Contributed {
                mcp_name: format!("{PREFIX}{short}__{}", tool.name),
                plugin_id: id.clone(),
                name: tool.name,
                description: tool.description,
                input_schema: tool.input_schema,
                read_only: tool.read_only,
            });
        }
    }
    out
}

/// Checks a plugin's `tool.call` answer is MCP-shaped: `{content: [...], isError?}`.
pub fn check_answer(plugin: &str, answer: Value) -> Result<Value, String> {
    let content = answer.get("content").and_then(Value::as_array).ok_or_else(|| format!("{plugin} answered tool.call without a content array"))?;
    if content.iter().any(|item| !item.get("type").is_some_and(Value::is_string)) {
        return Err(format!("{plugin} answered tool.call with a content item that has no type"));
    }
    let is_error = answer.get("isError").and_then(Value::as_bool).unwrap_or(false);
    Ok(json!({"content": content, "isError": is_error}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(name: &str) -> Tool {
        Tool { name: name.into(), description: "d".into(), input_schema: json!({}), read_only: true }
    }

    #[test]
    fn names_use_the_last_part_of_the_id_unless_two_plugins_share_it() {
        let named = name_all(vec![
            ("io.github.a.govee-output".into(), vec![tool("discover")]),
            ("com.example.lamp".into(), vec![tool("ping")]),
            ("org.other.lamp".into(), vec![tool("ping")]),
        ]);
        let names: Vec<&str> = named.iter().map(|t| t.mcp_name.as_str()).collect();
        assert_eq!(names, ["plugin__govee_output__discover", "plugin__com_example_lamp__ping", "plugin__org_other_lamp__ping"]);
        assert!(named.iter().all(|t| t.mcp_name.len() <= MAX_NAME));
    }

    #[test]
    fn long_ids_are_cut_to_fit_and_empty_schemas_become_object_schemas() {
        let id = format!("io.github.{}", "x".repeat(100));
        let named = name_all(vec![(id, vec![tool("a_rather_long_tool_name")])]);
        assert!(named[0].mcp_name.len() <= MAX_NAME, "{}", named[0].mcp_name);
        assert!(named[0].mcp_name.ends_with("__a_rather_long_tool_name"));
        assert_eq!(named[0].to_mcp()["inputSchema"], json!({"type": "object", "properties": {}}));
    }

    #[test]
    fn answers_must_be_mcp_shaped() {
        assert_eq!(check_answer("p", json!({"content": [{"type": "text", "text": "hi"}]})).unwrap()["isError"], json!(false));
        assert!(check_answer("p", json!({"text": "hi"})).is_err());
        assert!(check_answer("p", json!({"content": [{"text": "hi"}]})).is_err());
    }
}
