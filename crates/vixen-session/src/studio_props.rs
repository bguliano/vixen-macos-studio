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
/// The prop kinds a new element can be shown as, linked: the template kinds whose shape the native designer builds from the element.
pub const LINKED_KINDS: [&str; 3] = ["megatree", "matrix", "arch"];

/// Adds a preview shape for an element of the working copy, as the native designer does when a prop is drawn with that element
/// selected: the strings, lights per string and every light's link come from the element, in its order (a megatree's S1 Px1 at
/// the bottom of the first string, a matrix's Row1 at the bottom, an arch from the left). The shape is sized for its lights and
/// placed to the right of the preview's other props. Refuses an element the shape can't take light for light.
pub fn add_linked(s: &mut SetupSession, preview: Uuid, node: Uuid, kind: &str, name: Option<&str>) -> Result<Value, String> {
    let entry = crate::preview::preview_entries(&s.system)
        .into_iter()
        .find(|p| p.id == preview)
        .ok_or("Preview not found")?;
    let data = crate::preview::load_preview_data(&s.system.module_store, &entry).map_err(|e| e.to_string())?;
    // The working tree: a template's new elements are there before the working copy is published.
    let tree = Arc::new(s.tree.to_element_tree(s.system.tree.color_sets.clone()));
    let element = tree.find(&node).ok_or("Element not found")?;
    let (tool, label) = match kind {
        "megatree" => (Tool::MegaTree, "Mega Tree"),
        "matrix" => (Tool::PixelGrid, "Pixel Grid"),
        "arch" => (Tool::Arch, "Arch"),
        _ => return Err("A linked preview shape can be a megatree, matrix or arch".into()),
    };
    let mut lights = Vec::new();
    let mut stack = vec![element];
    while let Some(n) = stack.pop() {
        if tree.is_leaf(n) {
            lights.push(tree.node(n).id);
        } else {
            stack.extend(tree.node(n).children.iter().rev());
        }
    }
    // Where the preview's props end, before this one is added.
    let right = data
        .display_items
        .iter()
        .flat_map(|i| i.shape.pixels_vec())
        .map(|p| p.x)
        .max();
    let width_limit = if data.width > 0 { data.width } else { 1920 };
    let mut design = DesignSession::new(data, Some(tree.clone()));
    design.set_selected_nodes(&[node]);
    let prop = *design.add_shape(tool).shapes_added.first().ok_or("Prop could not be created")?;
    let shape = &design.data().item(prop).ok_or("Prop could not be created")?.shape;
    let strings = shape.strings().map_or(1, |l| l.len());
    let mut linked = Vec::new();
    shape.for_each_pixel(&mut |p| linked.push(p.node_id));
    // Every preview light shows one of the element's lights and every one of them is shown. A string of standard lights shows its
    // one element on all of its preview lights.
    let wanted: std::collections::HashSet<_> = lights.iter().collect();
    let shown: std::collections::HashSet<_> = linked.iter().collect();
    if linked.iter().any(|id| !wanted.contains(id)) || shown.len() != wanted.len() {
        return Err(match kind {
            "megatree" => "A linked Mega Tree needs at least 4 strings of 5 or more lights each, all the same length",
            "matrix" => "A linked Pixel Grid needs at least 2 rows of 5 or more lights each, all the same length",
            _ => "A linked Arch needs at least 4 lights",
        }
        .into());
    }
    let per_string = (linked.len() / strings.max(1)) as i32;
    let (w, h) = match kind {
        "megatree" => (200, 300),
        "matrix" => ((per_string * 8).clamp(80, 480), (strings as i32 * 8).clamp(80, 480)),
        _ => ((lights.len() as i32 * 6).clamp(160, 480), 120),
    };
    let mut x = right.map_or(40, |r| r + 40);
    if x + w > width_limit {
        x = 40;
    }
    let y = 40;
    for (property, value) in [
        ("TopLeft", PropertyValue::Point { x, y }),
        ("BottomRight", PropertyValue::Point { x: x + w, y: y + h }),
        ("Name", PropertyValue::Text(name.unwrap_or(&tree.node(element).name).chars().take(256).collect())),
    ] {
        design.set_shape_property(prop, property, value).map_err(|e| format!("{e:?}"))?;
    }
    crate::preview_design::save_preview_data(&mut s.system, &entry, design.data());
    s.dirty.studio_module = true;
    Ok(json!({"previewId":preview.to_string(),"propId":prop.0,"shape":label,"strings":strings,"lightsPerString":per_string,"linkedLights":lights.len(),"x":x,"y":y,"width":w,"height":h}))
}

pub fn edit(s: &mut SetupSession, name: &str, a: &Value) -> Result<Value, String> {
    let id = Uuid::parse_str(text(a, "previewId")?).map_err(|_| "Invalid previewId")?;
    if name == "preview_prop_create" && a.get("elementId").is_some() {
        let node = Uuid::parse_str(text(a, "elementId")?).map_err(|_| "Invalid elementId")?;
        let label = a.get("name").map(|_| text(a, "name")).transpose()?;
        return add_linked(s, id, node, text(a, "kind")?, label);
    }
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
        ("preview_prop_create","Add a native preview prop at the default location; change geometry with preview_prop_set. With elementId (kind megatree, matrix or arch) the shape is built from that element and already linked light for light, as the designer does when a prop is drawn with the element selected: strings, lights per string and order come from the element, it is sized for its lights and placed right of the preview's other props, and name defaults to the element's. element_template's previewId does the same for a new prop.",json!({"previewId":string,"name":string,"elementId":string,"kind":{"type":"string","enum":["string","arch","rectangle","ellipse","megatree","matrix","star"]}}),vec!["previewId","kind"]),
        ("preview_prop_link","Link a preview prop to element node UUIDs using native linking rules. Requires snapshot expectedRevision.",json!({"previewId":string,"propId":{"type":"integer","minimum":0},"nodeIds":{"type":"array","items":string,"minItems":1,"maxItems":4096}}),vec!["previewId","propId","nodeIds","expectedRevision"]),
        ("preview_prop_set","Set a native preview property. Use metadata IDs from preview_props and the matching valueType. Requires snapshot expectedRevision.",json!({"previewId":string,"propId":{"type":"integer","minimum":0},"property":string,"valueType":{"type":"string","enum":["int","bool","text","enum","double"]},"value":{}}),vec!["previewId","propId","property","valueType","value","expectedRevision"]),
    ].into_iter().map(|(name,description,mut properties,required)|{properties["expectedRevision"]=json!({"type":"integer","minimum":0});json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":false,"openWorldHint":false}})}).collect()
}
