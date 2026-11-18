//! Preview props through native DesignSession, committed by the shared profile transaction.
use crate::setup::SetupSession;
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;
use vixen_preview::{DesignSession, PropertyValue, ShapeId, Tool};
fn text<'a>(a: &'a Value, k: &str) -> Result<&'a str, String> {
    a[k].as_str()
        .filter(|s| !s.is_empty() && s.len() <= 256)
        .ok_or(format!("Invalid {k}"))
}
pub fn edit(s: &mut SetupSession, name: &str, a: &Value) -> Result<Value, String> {
    let id = Uuid::parse_str(text(a, "previewId")?).map_err(|_| "Invalid previewId")?;
    let entry = crate::preview::preview_entries(&s.system)
        .into_iter()
        .find(|p| p.id == id)
        .ok_or("Preview not found")?;
    let data = crate::preview::load_preview_data(&s.system.module_store, &entry)
        .map_err(|e| e.to_string())?;
    let tree = Arc::new(s.system.tree.clone());
    let mut design = DesignSession::new(data, Some(tree.clone()));
    let prop = if name == "preview_prop_create" {
        let tool = match text(a, "kind")? {
            "string" => Tool::String,
            "arch" => Tool::Arch,
            "rectangle" => Tool::Rectangle,
            "ellipse" => Tool::Ellipse,
            "megatree" => Tool::MegaTree,
            "matrix" => Tool::PixelGrid,
            "star" => Tool::Star,
            _ => return Err("Unsupported prop kind".into()),
        };
        let change = design.add_shape(tool);
        let prop = *change
            .shapes_added
            .first()
            .ok_or("Prop could not be created")?;
        design
            .set_shape_property(prop, "Name", PropertyValue::Text(text(a, "name")?.into()))
            .map_err(|e| format!("{e:?}"))?;
        prop
    } else {
        if a.get("expectedRevision").is_none() {
            return Err("expectedRevision is required for existing preview prop edits".into());
        }
        let prop = ShapeId(
            a["propId"]
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or("Invalid propId")?,
        );
        if design.data().item(prop).is_none() {
            return Err("Prop not found".into());
        }
        prop
    };
    match name {
        "preview_prop_create" => {}
        "preview_prop_link" => {
            let ids = a["nodeIds"]
                .as_array()
                .filter(|v| !v.is_empty() && v.len() <= 4096)
                .ok_or("nodeIds needs 1..4096 UUIDs")?;
            let nodes = ids
                .iter()
                .map(|v| {
                    v.as_str()
                        .and_then(|v| Uuid::parse_str(v).ok())
                        .and_then(|id| tree.find(&id))
                        .ok_or("Element node not found")
                })
                .collect::<Result<Vec<_>, _>>()?;
            design.link_elements(prop, &nodes);
        }
        "preview_prop_set" => {
            let kind = text(a, "valueType")?;
            let v = &a["value"];
            let value = match kind {
                "int" => PropertyValue::Int(
                    v.as_i64()
                        .filter(|n| (-4096..=4096).contains(n))
                        .ok_or("Integer must be -4096..4096")? as i32,
                ),
                "bool" => PropertyValue::Bool(v.as_bool().ok_or("Expected boolean")?),
                "text" => PropertyValue::Text(
                    v.as_str()
                        .filter(|v| v.len() <= 256)
                        .ok_or("Expected text <=256 bytes")?
                        .into(),
                ),
                "enum" => PropertyValue::Enum(
                    v.as_str()
                        .filter(|v| v.len() <= 256)
                        .ok_or("Expected enum string")?
                        .into(),
                ),
                "double" => PropertyValue::Double(
                    v.as_f64()
                        .filter(|v| v.is_finite() && v.abs() <= 4096.0)
                        .ok_or("Number must be finite and bounded by 4096")?,
                ),
                _ => return Err("Unsupported valueType".into()),
            };
            design
                .set_shape_property(prop, text(a, "property")?, value)
                .map_err(|e| format!("{e:?}"))?;
        }
        _ => return Err("Unknown preview prop command".into()),
    }
    crate::preview_design::save_preview_data(&mut s.system, &entry, design.data());
    s.dirty.studio_module = true;
    Ok(json!({"propId":prop.0,"previewId":id.to_string()}))
}
pub fn tools() -> Vec<Value> {
    let string = json!({"type":"string","minLength":1,"maxLength":256});
    vec![
        ("preview_prop_create","Add a native preview prop at the default location; change geometry with preview_prop_set.",json!({"previewId":string,"name":string,"kind":{"type":"string","enum":["string","arch","rectangle","ellipse","megatree","matrix","star"]}}),vec!["previewId","name","kind"]),
        ("preview_prop_link","Link a preview prop to element node UUIDs using native linking rules. Requires snapshot expectedRevision.",json!({"previewId":string,"propId":{"type":"integer","minimum":0},"nodeIds":{"type":"array","items":string,"minItems":1,"maxItems":4096}}),vec!["previewId","propId","nodeIds","expectedRevision"]),
        ("preview_prop_set","Set a native preview property. Use metadata IDs from preview_props and the matching valueType. Requires snapshot expectedRevision.",json!({"previewId":string,"propId":{"type":"integer","minimum":0},"property":string,"valueType":{"type":"string","enum":["int","bool","text","enum","double"]},"value":{}}),vec!["previewId","propId","property","valueType","value","expectedRevision"]),
    ].into_iter().map(|(name,description,mut properties,required)|{properties["expectedRevision"]=json!({"type":"integer","minimum":0});json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":false,"openWorldHint":false}})}).collect()
}
