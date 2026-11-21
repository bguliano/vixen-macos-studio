//! Prop files: the schema-1 package (`Persistence/PropPackageReader.cs`, `PropPackageWriter.cs`, `PropDocumentValidator.cs`,
//! `PropDocumentMapper.cs`, `AtomicPropFileWriter.cs`) and the load and save routing of `PropModelPersistenceService`.

use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use serde_json::Value;
use uuid::Uuid;
use vixen_core::Color;

use crate::legacy;
use crate::model::*;

const FORMAT: &str = "vixen.custom-prop";
const SCHEMA_VERSION: i64 = 1;
const JSON_ENTRY: &str = "prop.json";
const IMAGE_ENTRY: &str = "background.jpg";
const IMAGE_MEDIA_TYPE: &str = "image/jpeg";
const MAXIMUM_JSON_BYTES: u64 = 4 * 1024 * 1024;
const MAXIMUM_IMAGE_BYTES: u64 = 64 * 1024 * 1024;
const MAXIMUM_COMPRESSION_RATIO: u64 = 100;
const ZIP_SIGNATURE: [u8; 4] = [0x50, 0x4B, 0x03, 0x04];
pub const LEGACY_BACKUP_SUFFIX: &str = ".legacy-litedb.bak";

fn invalid(detail: &str) -> PropError {
    PropError::new("The prop package is invalid.", detail)
}

fn not_json() -> PropError {
    invalid("The manifest is not valid JSON.")
}

fn not_saved(detail: impl Into<String>) -> PropError {
    PropError::new("The prop could not be saved.", detail)
}

// ----- The document, as the JSON holds it. `None` is a JSON null where the C# type allows one. -----

#[derive(Clone, Debug)]
pub(crate) struct PackageDoc {
    format: Option<String>,
    schema_version: i64,
    prop: Option<PropDoc>,
    root_element_id: Uuid,
    elements: Option<Vec<Option<ElementDoc>>>,
    image: Option<(Option<String>, Option<String>)>,
}

#[derive(Clone, Debug)]
struct PropDoc {
    id: Uuid,
    name: String,
    prop_type: String,
    created_by: String,
    creation_date: String,
    modified_date: String,
    opacity: f64,
    width: f64,
    height: f64,
    vendor: Option<VendorMetadata>,
    physical: Option<PhysicalDoc>,
    information: Option<String>,
}

#[derive(Clone, Debug, Default)]
struct PhysicalDoc {
    height: String,
    width: String,
    depth: String,
    material: String,
    node_count: String,
    bulb_type: String,
    color_mode: String,
}

#[derive(Clone, Debug)]
pub(crate) struct ElementDoc {
    pub id: Uuid,
    pub state_property_id: Uuid,
    pub name: String,
    pub order: i32,
    pub light_size: i32,
    pub model_type: String,
    pub face: Option<(Option<String>, Option<String>)>,
    pub state_definitions: Option<Vec<Option<StateDefinitionDoc>>>,
    pub child_ids: Option<Vec<Uuid>>,
    pub lights: Option<Vec<Option<Light>>>,
}

#[derive(Clone, Debug)]
pub(crate) struct StateDefinitionDoc {
    id: Uuid,
    name: String,
    description: String,
    items: Option<Vec<Option<StateItemDoc>>>,
}

#[derive(Clone, Debug)]
struct StateItemDoc {
    id: Uuid,
    name: String,
    color: Option<String>,
    element_ids: Option<Vec<Uuid>>,
}

// ----- Reading JSON the way System.Text.Json does: a missing property keeps its default, a wrong type fails the whole read. -----

type Object = serde_json::Map<String, Value>;

fn as_object(value: &Value) -> Result<&Object, PropError> {
    value.as_object().ok_or_else(not_json)
}

fn string_or(object: &Object, key: &str, default: &str) -> Result<Option<String>, PropError> {
    match object.get(key) {
        None => Ok(Some(default.to_string())),
        Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err(not_json()),
    }
}

/// A string where nothing checks for null: null reads as empty.
fn text(object: &Object, key: &str) -> Result<String, PropError> {
    Ok(string_or(object, key, "")?.unwrap_or_default())
}

fn parse_guid(value: &Value) -> Result<Uuid, PropError> {
    let text = value.as_str().ok_or_else(not_json)?;
    // System.Text.Json takes only the hyphenated form.
    if text.len() != 36 {
        return Err(not_json());
    }
    Uuid::parse_str(text).map_err(|_| not_json())
}

fn guid(object: &Object, key: &str) -> Result<Uuid, PropError> {
    object.get(key).map_or(Ok(Uuid::nil()), parse_guid)
}

fn integer(object: &Object, key: &str, default: i64) -> Result<i64, PropError> {
    match object.get(key) {
        None => Ok(default),
        Some(value) => value.as_i64().filter(|v| i32::try_from(*v).is_ok()).ok_or_else(not_json),
    }
}

fn double(object: &Object, key: &str) -> Result<f64, PropError> {
    match object.get(key) {
        None => Ok(0.0),
        Some(value) => value.as_f64().ok_or_else(not_json),
    }
}

fn date(object: &Object, key: &str) -> Result<String, PropError> {
    match object.get(key) {
        None => Ok("0001-01-01T00:00:00".to_string()),
        Some(Value::String(text)) if looks_like_a_date(text) => Ok(text.clone()),
        Some(_) => Err(not_json()),
    }
}

fn looks_like_a_date(text: &str) -> bool {
    let b = text.as_bytes();
    b.len() >= 10 && b[..4].iter().all(u8::is_ascii_digit) && b[4] == b'-' && b[7] == b'-' && (b.len() == 10 || b[10] == b'T')
}

/// An object-typed property: missing gives the default-constructed document, null gives `None`.
fn nested<T>(object: &Object, key: &str, parse: impl Fn(&Object) -> Result<T, PropError>) -> Result<Option<T>, PropError> {
    match object.get(key) {
        None => parse(&Object::new()).map(Some),
        Some(Value::Null) => Ok(None),
        Some(value) => parse(as_object(value)?).map(Some),
    }
}

/// A list-typed property: missing gives an empty list, null gives `None`.
fn list<T>(object: &Object, key: &str, parse: impl Fn(&Value) -> Result<T, PropError>) -> Result<Option<Vec<T>>, PropError> {
    match object.get(key) {
        None => Ok(Some(Vec::new())),
        Some(Value::Null) => Ok(None),
        Some(Value::Array(values)) => values.iter().map(parse).collect::<Result<Vec<T>, PropError>>().map(Some),
        Some(_) => Err(not_json()),
    }
}

fn nullable<T>(value: &Value, parse: impl Fn(&Object) -> Result<T, PropError>) -> Result<Option<T>, PropError> {
    match value {
        Value::Null => Ok(None),
        other => parse(as_object(other)?).map(Some),
    }
}

fn parse_package(json: &[u8]) -> Result<PackageDoc, PropError> {
    let value: Value = serde_json::from_slice(json).map_err(|_| not_json())?;
    if value.is_null() {
        return Err(invalid("The manifest contains no document."));
    }
    let root = as_object(&value)?;
    Ok(PackageDoc {
        format: string_or(root, "format", FORMAT)?,
        schema_version: integer(root, "schemaVersion", SCHEMA_VERSION)?,
        prop: nested(root, "prop", parse_prop)?,
        root_element_id: guid(root, "rootElementId")?,
        elements: list(root, "elements", |v| nullable(v, parse_element))?,
        image: nested(root, "image", |o| Ok((string_or(o, "entryName", IMAGE_ENTRY)?, string_or(o, "mediaType", IMAGE_MEDIA_TYPE)?)))?,
    })
}

fn parse_prop(o: &Object) -> Result<PropDoc, PropError> {
    Ok(PropDoc {
        id: guid(o, "id")?,
        name: text(o, "name")?,
        prop_type: text(o, "type")?,
        created_by: text(o, "createdBy")?,
        creation_date: date(o, "creationDate")?,
        modified_date: date(o, "modifiedDate")?,
        opacity: double(o, "opacity")?,
        width: double(o, "width")?,
        height: double(o, "height")?,
        vendor: nested(o, "vendor", |v| {
            Ok(VendorMetadata {
                name: text(v, "name")?,
                website: text(v, "website")?,
                contact: text(v, "contact")?,
                email: text(v, "email")?,
                phone: text(v, "phone")?,
            })
        })?,
        physical: nested(o, "physical", |p| {
            Ok(PhysicalDoc {
                height: text(p, "height")?,
                width: text(p, "width")?,
                depth: text(p, "depth")?,
                material: text(p, "material")?,
                node_count: text(p, "nodeCount")?,
                bulb_type: text(p, "bulbType")?,
                color_mode: text(p, "colorMode")?,
            })
        })?,
        information: nested(o, "information", |i| text(i, "notes"))?,
    })
}

pub(crate) fn parse_element(o: &Object) -> Result<ElementDoc, PropError> {
    Ok(ElementDoc {
        id: guid(o, "id")?,
        state_property_id: guid(o, "statePropertyId")?,
        name: text(o, "name")?,
        order: integer(o, "order", 0)? as i32,
        light_size: integer(o, "lightSize", 0)? as i32,
        model_type: text(o, "modelType")?,
        face: nested(o, "face", |f| Ok((string_or(f, "component", "None")?, string_or(f, "defaultColor", "#FFFFFFFF")?)))?,
        state_definitions: list(o, "stateDefinitions", |v| nullable(v, parse_state_definition))?,
        child_ids: list(o, "childIds", parse_guid)?,
        lights: list(o, "lights", |v| {
            nullable(v, |l| {
                Ok(Light { id: guid(l, "id")?, x: double(l, "x")?, y: double(l, "y")?, z: double(l, "z")?, size: double(l, "size")? })
            })
        })?,
    })
}

fn parse_state_definition(o: &Object) -> Result<StateDefinitionDoc, PropError> {
    Ok(StateDefinitionDoc {
        id: guid(o, "id")?,
        name: text(o, "name")?,
        description: text(o, "description")?,
        items: list(o, "items", |v| {
            nullable(v, |i| {
                Ok(StateItemDoc {
                    id: guid(i, "id")?,
                    name: text(i, "name")?,
                    color: string_or(i, "color", "#FFFFFFFF")?,
                    element_ids: list(i, "elementIds", parse_guid)?,
                })
            })
        })?,
    })
}

// ----- PropDocumentValidator -----

fn is_color(color: &Option<String>) -> bool {
    color.as_deref().is_some_and(|c| c.len() == 9 && c.starts_with('#') && c[1..].bytes().all(|b| b.is_ascii_hexdigit()))
}

fn validate(document: &PackageDoc) -> Result<HashMap<Uuid, &ElementDoc>, PropError> {
    if document.format.as_deref() != Some(FORMAT) || document.schema_version != SCHEMA_VERSION {
        return Err(invalid("The prop package uses an unsupported schema version."));
    }
    let (Some(prop), Some(image), Some(listed)) = (&document.prop, &document.image, &document.elements) else {
        return Err(invalid("The prop package document is incomplete."));
    };
    if prop.vendor.is_none() || prop.physical.is_none() || prop.information.is_none() {
        return Err(invalid("The prop package document is incomplete."));
    }
    if prop.id.is_nil() || document.root_element_id.is_nil() {
        return Err(invalid("The prop package contains an empty identifier."));
    }
    if image.0.as_deref() != Some(IMAGE_ENTRY) || image.1.as_deref() != Some(IMAGE_MEDIA_TYPE) {
        return Err(invalid("The prop package image declaration is invalid."));
    }
    if !prop.opacity.is_finite() || !prop.width.is_finite() || !prop.height.is_finite() || prop.width < 1.0 || prop.height < 1.0 {
        return Err(invalid("The prop package contains invalid canvas dimensions."));
    }

    let mut elements: HashMap<Uuid, &ElementDoc> = HashMap::with_capacity(listed.len());
    let mut lights = HashSet::new();
    for element in listed {
        let Some(element) = element.as_ref().filter(|e| !e.id.is_nil()) else {
            return Err(invalid("The prop package contains duplicate element identifiers."));
        };
        if elements.insert(element.id, element).is_some() {
            return Err(invalid("The prop package contains duplicate element identifiers."));
        }
        let (Some(_), Some(element_lights), Some(_), Some(_)) = (&element.child_ids, &element.lights, &element.state_definitions, &element.face)
        else {
            return Err(invalid("The prop package contains an invalid element."));
        };
        if element.state_property_id.is_nil() || element.light_size < 1 {
            return Err(invalid("The prop package contains an invalid element."));
        }
        for light in element_lights {
            let ok = light.as_ref().is_some_and(|l| {
                !l.id.is_nil() && lights.insert(l.id) && l.x.is_finite() && l.y.is_finite() && l.z.is_finite() && l.size.is_finite() && l.size > 0.0
            });
            if !ok {
                return Err(invalid("The prop package contains an invalid light."));
            }
        }
    }
    if !elements.contains_key(&document.root_element_id) {
        return Err(invalid("The prop package root element is missing."));
    }

    let mut state_definitions = HashSet::new();
    let mut state_items = HashSet::new();
    // Upstream walks a dictionary here, so which fault it names first when a file has several is not fixed. File order is used.
    for element in listed.iter().flatten() {
        let children = element.child_ids.as_deref().unwrap_or_default();
        let distinct: HashSet<&Uuid> = children.iter().collect();
        if distinct.len() != children.len() || children.iter().any(|id| !elements.contains_key(id)) {
            return Err(invalid("The prop package contains an invalid element reference."));
        }
        for definition in element.state_definitions.as_deref().unwrap_or_default() {
            let Some((definition, items)) = definition.as_ref().and_then(|d| d.items.as_ref().map(|items| (d, items))) else {
                return Err(invalid("The prop package contains an invalid State definition."));
            };
            if definition.id.is_nil() || !state_definitions.insert(definition.id) {
                return Err(invalid("The prop package contains an invalid State definition."));
            }
            for item in items {
                let ok = item.as_ref().is_some_and(|i| {
                    !i.id.is_nil()
                        && state_items.insert(i.id)
                        && i.element_ids.as_ref().is_some_and(|ids| ids.iter().all(|id| elements.contains_key(id)))
                        && is_color(&i.color)
                });
                if !ok {
                    return Err(invalid("The prop package contains an invalid State item."));
                }
            }
        }
    }

    let visited = visit(document.root_element_id, &elements)?;
    if visited != elements.len() {
        return Err(invalid("The prop package contains unreachable elements."));
    }
    Ok(elements)
}

fn visit(root: Uuid, elements: &HashMap<Uuid, &ElementDoc>) -> Result<usize, PropError> {
    let mut visited = HashSet::new();
    let mut active = HashSet::new();
    let mut pending = vec![(root, false)];
    while let Some((id, leaving)) = pending.pop() {
        if leaving {
            active.remove(&id);
            continue;
        }
        if !active.insert(id) {
            return Err(invalid("The prop package contains an element cycle."));
        }
        if !visited.insert(id) {
            active.remove(&id);
            continue;
        }
        pending.push((id, true));
        for child in elements[&id].child_ids.as_deref().unwrap_or_default() {
            pending.push((*child, false));
        }
    }
    Ok(visited.len())
}

// ----- PropDocumentMapper -----

fn color_text(color: Color) -> String {
    format!("#{:08X}", color.to_argb_u32())
}

fn parse_color(text: Option<&str>, what: &str) -> Result<Color, PropError> {
    // Upstream checks State item colours and then parses the face colour unchecked. A bad one is a plain failure here, not a crash.
    text.filter(|t| t.len() == 9 && t.starts_with('#'))
        .and_then(|t| u32::from_str_radix(&t[1..], 16).ok())
        .map(Color::from_argb_u32)
        .ok_or_else(|| invalid(&format!("The {what} is invalid.")))
}

pub(crate) fn element_doc(prop: &Prop, id: Uuid) -> Option<ElementDoc> {
    let e = prop.element(id)?;
    Some(ElementDoc {
        id: e.id,
        state_property_id: e.state_property_id,
        name: e.name().to_string(),
        // The properties, not the fields: a group saves -1 and its first leaf's light size.
        order: prop.order(id),
        light_size: prop.light_size(id),
        model_type: e.model_type.name().to_string(),
        face: Some((Some(e.face.component.name().to_string()), Some(color_text(e.face.default_color)))),
        state_definitions: Some(
            e.state_definitions
                .iter()
                .map(|d| {
                    Some(StateDefinitionDoc {
                        id: d.id,
                        name: d.name.clone(),
                        description: d.description.clone(),
                        items: Some(
                            d.items
                                .iter()
                                .map(|i| {
                                    Some(StateItemDoc {
                                        id: i.id,
                                        name: i.name.clone(),
                                        color: Some(color_text(i.color)),
                                        element_ids: Some(i.element_ids.clone()),
                                    })
                                })
                                .collect(),
                        ),
                    })
                })
                .collect(),
        ),
        child_ids: Some(e.children().to_vec()),
        lights: Some(e.lights().iter().cloned().map(Some).collect()),
    })
}

fn to_document(prop: &Prop) -> PackageDoc {
    PackageDoc {
        format: Some(FORMAT.to_string()),
        schema_version: SCHEMA_VERSION,
        prop: Some(PropDoc {
            id: prop.id,
            name: prop.name().to_string(),
            prop_type: prop.prop_type.clone(),
            created_by: prop.created_by.clone(),
            creation_date: prop.creation_date.clone(),
            modified_date: prop.modified_date.clone(),
            opacity: prop.opacity,
            width: prop.width,
            height: prop.height,
            vendor: Some(prop.vendor.clone()),
            physical: Some(PhysicalDoc {
                height: prop.physical.height.clone(),
                width: prop.physical.width.clone(),
                depth: prop.physical.depth.clone(),
                material: prop.physical.material.clone(),
                node_count: prop.physical.node_count.clone(),
                bulb_type: prop.physical.bulb_type.clone(),
                color_mode: prop.physical.color_mode.name().to_string(),
            }),
            information: Some(prop.information.notes.clone()),
        }),
        root_element_id: prop.root_id(),
        elements: Some(prop.breadth_first().into_iter().map(|id| element_doc(prop, id)).collect()),
        image: Some((Some(IMAGE_ENTRY.to_string()), Some(IMAGE_MEDIA_TYPE.to_string()))),
    }
}

/// One element out of its document, with no links yet.
pub(crate) fn element_from_doc(source: &ElementDoc) -> Result<ElementModel, PropError> {
    let face = source.face.as_ref().ok_or_else(|| invalid("The prop package contains an invalid element."))?;
    let mut element = ElementModel::new(&source.name);
    element.id = source.id;
    element.state_property_id = source.state_property_id;
    element.order = source.order;
    element.light_size = source.light_size.max(1);
    element.model_type = ElementModelType::parse(&source.model_type).ok_or_else(|| invalid("The element model type is invalid."))?;
    element.face = FaceDefinition {
        component: face.0.as_deref().and_then(FaceComponent::parse).ok_or_else(|| invalid("The face component is invalid."))?,
        default_color: parse_color(face.1.as_deref(), "face default color")?,
    };
    for definition in source.state_definitions.iter().flatten().flatten() {
        let mut items = Vec::new();
        for item in definition.items.iter().flatten().flatten() {
            items.push(StateItemModel {
                id: item.id,
                name: item.name.clone(),
                color: parse_color(item.color.as_deref(), "State item color")?,
                element_ids: item.element_ids.clone().unwrap_or_default(),
            });
        }
        element.state_definitions.push(StateDefinitionModel {
            id: definition.id,
            name: definition.name.clone(),
            description: definition.description.clone(),
            items,
        });
    }
    element.normalize_state_model_data();
    Ok(element)
}

fn to_model(document: &PackageDoc, image: PropImage) -> Result<Prop, PropError> {
    validate(document)?;
    let (Some(source), Some(listed)) = (&document.prop, &document.elements) else {
        return Err(invalid("The prop package document is incomplete."));
    };
    let mut elements: Vec<ElementModel> = Vec::with_capacity(listed.len());
    let mut index = HashMap::with_capacity(listed.len());
    for doc in listed.iter().flatten() {
        index.insert(doc.id, elements.len());
        elements.push(element_from_doc(doc)?);
    }
    for doc in listed.iter().flatten() {
        let at = index[&doc.id];
        for child in doc.child_ids.iter().flatten() {
            elements[at].children.push(*child);
            elements[index[child]].parents.push(doc.id);
        }
        elements[at].lights = doc.lights.iter().flatten().flatten().cloned().collect();
    }
    let physical = source.physical.clone().unwrap_or_default();
    let header = PropHeader {
        id: source.id,
        prop_type: source.prop_type.clone(),
        created_by: source.created_by.clone(),
        creation_date: source.creation_date.clone(),
        modified_date: source.modified_date.clone(),
        opacity: source.opacity,
        width: source.width,
        height: source.height,
        vendor: source.vendor.clone().unwrap_or_default(),
        physical: PhysicalMetadata {
            height: physical.height,
            width: physical.width,
            depth: physical.depth,
            material: physical.material,
            node_count: physical.node_count,
            bulb_type: physical.bulb_type,
            color_mode: ColorMode::parse(&physical.color_mode).ok_or_else(|| invalid("The color mode is invalid."))?,
        },
        information: InformationMetadata { notes: source.information.clone().unwrap_or_default() },
    };
    let mut prop = Prop::from_parts(header, document.root_element_id, elements, image);
    prop.set_name(&source.name);
    Ok(prop)
}

// ----- Writing JSON in the order System.Text.Json writes the documents' properties -----

struct Json(String);

impl Json {
    fn key(&mut self, first: bool, name: &str) {
        if !first {
            self.0.push(',');
        }
        self.0.push('"');
        self.0.push_str(name);
        self.0.push_str("\":");
    }

    fn string(&mut self, first: bool, name: &str, value: &str) {
        self.key(first, name);
        self.raw_string(value);
    }

    fn raw_string(&mut self, value: &str) {
        self.0.push_str(&serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string()));
    }

    fn guid(&mut self, first: bool, name: &str, value: Uuid) {
        self.key(first, name);
        self.0.push('"');
        self.0.push_str(&value.hyphenated().to_string());
        self.0.push('"');
    }

    fn int(&mut self, first: bool, name: &str, value: i64) {
        self.key(first, name);
        self.0.push_str(&value.to_string());
    }

    fn double(&mut self, first: bool, name: &str, value: f64) {
        self.key(first, name);
        // A whole number is written without a fraction, as .NET writes it.
        if value.fract() == 0.0 && value.abs() < 1e15 {
            self.0.push_str(&(value as i64).to_string());
        } else {
            match serde_json::Number::from_f64(value) {
                Some(number) => self.0.push_str(&number.to_string()),
                None => self.0.push('0'),
            }
        }
    }

    fn guids(&mut self, first: bool, name: &str, values: &[Uuid]) {
        self.key(first, name);
        self.0.push('[');
        for (i, value) in values.iter().enumerate() {
            if i > 0 {
                self.0.push(',');
            }
            self.0.push('"');
            self.0.push_str(&value.hyphenated().to_string());
            self.0.push('"');
        }
        self.0.push(']');
    }
}

pub(crate) fn write_element(json: &mut String, e: &ElementDoc) {
    let mut out = Json(std::mem::take(json));
    out.0.push('{');
    out.guid(true, "id", e.id);
    out.guid(false, "statePropertyId", e.state_property_id);
    out.string(false, "name", &e.name);
    out.int(false, "order", e.order as i64);
    out.int(false, "lightSize", e.light_size as i64);
    out.string(false, "modelType", &e.model_type);
    out.key(false, "face");
    out.0.push('{');
    let (component, color) = e.face.clone().unwrap_or_default();
    out.string(true, "component", component.as_deref().unwrap_or("None"));
    out.string(false, "defaultColor", color.as_deref().unwrap_or("#FFFFFFFF"));
    out.0.push('}');
    out.key(false, "stateDefinitions");
    out.0.push('[');
    for (d, definition) in e.state_definitions.iter().flatten().flatten().enumerate() {
        if d > 0 {
            out.0.push(',');
        }
        out.0.push('{');
        out.guid(true, "id", definition.id);
        out.string(false, "name", &definition.name);
        out.string(false, "description", &definition.description);
        out.key(false, "items");
        out.0.push('[');
        for (i, item) in definition.items.iter().flatten().flatten().enumerate() {
            if i > 0 {
                out.0.push(',');
            }
            out.0.push('{');
            out.guid(true, "id", item.id);
            out.string(false, "name", &item.name);
            out.string(false, "color", item.color.as_deref().unwrap_or("#FFFFFFFF"));
            out.guids(false, "elementIds", item.element_ids.as_deref().unwrap_or_default());
            out.0.push('}');
        }
        out.0.push_str("]}");
    }
    out.0.push(']');
    out.guids(false, "childIds", e.child_ids.as_deref().unwrap_or_default());
    out.key(false, "lights");
    out.0.push('[');
    for (i, light) in e.lights.iter().flatten().flatten().enumerate() {
        if i > 0 {
            out.0.push(',');
        }
        out.0.push('{');
        out.guid(true, "id", light.id);
        out.double(false, "x", light.x);
        out.double(false, "y", light.y);
        out.double(false, "z", light.z);
        out.double(false, "size", light.size);
        out.0.push('}');
    }
    out.0.push_str("]}");
    *json = out.0;
}

fn write_document(document: &PackageDoc) -> String {
    let mut out = Json(String::new());
    out.0.push('{');
    out.string(true, "format", document.format.as_deref().unwrap_or(FORMAT));
    out.int(false, "schemaVersion", document.schema_version);
    if let Some(p) = &document.prop {
        out.key(false, "prop");
        out.0.push('{');
        out.guid(true, "id", p.id);
        out.string(false, "name", &p.name);
        out.string(false, "type", &p.prop_type);
        out.string(false, "createdBy", &p.created_by);
        out.string(false, "creationDate", &p.creation_date);
        out.string(false, "modifiedDate", &p.modified_date);
        out.double(false, "opacity", p.opacity);
        out.double(false, "width", p.width);
        out.double(false, "height", p.height);
        let vendor = p.vendor.clone().unwrap_or_default();
        out.key(false, "vendor");
        out.0.push('{');
        out.string(true, "name", &vendor.name);
        out.string(false, "website", &vendor.website);
        out.string(false, "contact", &vendor.contact);
        out.string(false, "email", &vendor.email);
        out.string(false, "phone", &vendor.phone);
        out.0.push('}');
        let physical = p.physical.clone().unwrap_or_default();
        out.key(false, "physical");
        out.0.push('{');
        out.string(true, "height", &physical.height);
        out.string(false, "width", &physical.width);
        out.string(false, "depth", &physical.depth);
        out.string(false, "material", &physical.material);
        out.string(false, "nodeCount", &physical.node_count);
        out.string(false, "bulbType", &physical.bulb_type);
        out.string(false, "colorMode", &physical.color_mode);
        out.0.push('}');
        out.key(false, "information");
        out.0.push('{');
        out.string(true, "notes", p.information.as_deref().unwrap_or_default());
        out.0.push_str("}}");
    }
    out.guid(false, "rootElementId", document.root_element_id);
    out.key(false, "elements");
    out.0.push('[');
    for (i, element) in document.elements.iter().flatten().flatten().enumerate() {
        if i > 0 {
            out.0.push(',');
        }
        write_element(&mut out.0, element);
    }
    out.0.push(']');
    out.key(false, "image");
    out.0.push('{');
    out.string(true, "entryName", IMAGE_ENTRY);
    out.string(false, "mediaType", IMAGE_MEDIA_TYPE);
    out.0.push_str("}}");
    out.0
}

/// The `prop.json` a save would write, after the same checks.
pub fn manifest_json(prop: &Prop) -> Result<String, PropError> {
    let document = to_document(prop);
    validate(&document)?;
    Ok(write_document(&document))
}

// ----- The package -----

fn zip_failed<E>(_: E) -> PropError {
    invalid("The ZIP archive could not be read.")
}

/// `PropPackageReader.ReadAsync` and `PropDocumentMapper.ToModel`.
pub fn read_package(bytes: &[u8]) -> Result<Prop, PropError> {
    if !bytes.starts_with(&ZIP_SIGNATURE) {
        return Err(PropError::new("The prop file is not a valid package.", "The file does not have a ZIP local-header signature."));
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(zip_failed)?;
    if archive.len() != 2 {
        return Err(invalid("The package must contain exactly two entries."));
    }
    let mut names = HashSet::new();
    let mut json_index = None;
    let mut image_index = None;
    for index in 0..archive.len() {
        let entry = archive.by_index_raw(index).map_err(zip_failed)?;
        let unsafe_name = || invalid("The package contains an unsafe or unexpected entry name.");
        let name = entry.name().map_err(|_| unsafe_name())?.into_owned();
        if !names.insert(name.clone()) || (name != JSON_ENTRY && name != IMAGE_ENTRY) {
            return Err(unsafe_name());
        }
        let (length, compressed) = (entry.size(), entry.compressed_size());
        if length < 1 || compressed < 1 || length > MAXIMUM_IMAGE_BYTES || length / compressed > MAXIMUM_COMPRESSION_RATIO {
            return Err(invalid("The package contains an entry with an invalid size or compression ratio."));
        }
        if name == JSON_ENTRY {
            if length > MAXIMUM_JSON_BYTES {
                return Err(invalid("The package is missing a required entry or its manifest is too large."));
            }
            json_index = Some(index);
        } else {
            image_index = Some(index);
        }
    }
    let (Some(json_index), Some(image_index)) = (json_index, image_index) else {
        return Err(invalid("The package is missing a required entry or its manifest is too large."));
    };

    let json = read_entry(&mut archive, json_index, MAXIMUM_JSON_BYTES)?;
    let document = parse_package(&json)?;
    validate(&document)?;

    let image_bytes = read_entry(&mut archive, image_index, MAXIMUM_IMAGE_BYTES)?;
    if !image_bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Err(PropError::new("The prop package image is invalid.", "The image does not have a JPEG signature."));
    }
    let image = PropImage::decode(image_bytes)
        .map_err(|e| PropError::new("The prop package image is invalid.", format!("The JPEG decoder rejected the image. {e}")))?;
    to_model(&document, image)
}

fn read_entry(archive: &mut zip::ZipArchive<Cursor<&[u8]>>, index: usize, limit: u64) -> Result<Vec<u8>, PropError> {
    let entry = archive.by_index(index).map_err(zip_failed)?;
    let mut out = Vec::new();
    // The declared size was checked, but a crafted entry can hold more than it declares.
    entry.take(limit + 1).read_to_end(&mut out).map_err(zip_failed)?;
    if out.len() as u64 > limit {
        return Err(invalid("The package contains an entry with an invalid size or compression ratio."));
    }
    Ok(out)
}

/// The bytes of a package for this prop: `prop.json` deflated, then `background.jpg` stored.
pub fn write_package(prop: &Prop) -> Result<Vec<u8>, PropError> {
    let json = manifest_json(prop)?;
    let image = prop.image.jpeg_bytes().map_err(|e| not_saved(format!("The background image could not be encoded. {e}")))?;
    if image.is_empty() {
        return Err(not_saved("The background image could not be encoded."));
    }
    let failed = |e: String| not_saved(format!("Package creation or publication failed. {e}"));
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let deflated = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    writer.start_file(JSON_ENTRY, deflated).map_err(|e| failed(e.to_string()))?;
    writer.write_all(json.as_bytes()).map_err(|e| failed(e.to_string()))?;
    writer.start_file(IMAGE_ENTRY, stored).map_err(|e| failed(e.to_string()))?;
    writer.write_all(&image).map_err(|e| failed(e.to_string()))?;
    let cursor = writer.finish().map_err(|e| failed(e.to_string()))?;
    Ok(cursor.into_inner())
}

// ----- Files -----

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PropSourceFormat {
    Package,
    LegacyLiteDbV4,
}

#[derive(Clone, Debug)]
pub struct LoadedProp {
    pub prop: Prop,
    pub format: PropSourceFormat,
}

/// `PropModelPersistenceService.LoadAsync`: a package by its ZIP signature, else a LiteDB v4 file, else not supported.
/// `now` stamps a legacy prop, whose file holds no dates.
pub fn load_prop(path: &Path, now: &str) -> Result<LoadedProp, PropError> {
    let bytes = std::fs::read(path).map_err(|e| PropError::new("The prop file could not be read.", format!("{}: {e}", path.display())))?;
    load_prop_bytes(&bytes, now)
}

pub fn load_prop_bytes(bytes: &[u8], now: &str) -> Result<LoadedProp, PropError> {
    if bytes.starts_with(&ZIP_SIGNATURE) {
        return Ok(LoadedProp { prop: read_package(bytes)?, format: PropSourceFormat::Package });
    }
    if !legacy::is_v4_header(bytes) {
        return Err(PropError::new(
            "The prop file format is not supported.",
            "The file is neither a schema-1 package nor a LiteDB v4 data file.",
        ));
    }
    Ok(LoadedProp { prop: legacy::read_legacy(bytes, now)?, format: PropSourceFormat::LegacyLiteDbV4 })
}

fn full_path(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

fn same_file_path(a: &Path, b: &Path) -> bool {
    full_path(a).to_string_lossy().to_lowercase() == full_path(b).to_string_lossy().to_lowercase()
}

pub fn legacy_backup_path(destination: &Path) -> PathBuf {
    let mut text = full_path(destination).into_os_string();
    text.push(LEGACY_BACKUP_SUFFIX);
    PathBuf::from(text)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SaveOutcome {
    /// Where the legacy original went, when this save replaced one.
    pub legacy_backup: Option<PathBuf>,
}

/// `PropModelPersistenceService.SaveAsync`. `legacy_source` is the file a prop was read from when that was a legacy one and it
/// hasn't been saved since: saving over it keeps the original beside it as `<name>.legacy-litedb.bak`, once.
pub fn save_prop(prop: &Prop, path: &Path, legacy_source: Option<&Path>) -> Result<SaveOutcome, PropError> {
    if path.as_os_str().is_empty() {
        return Err(PropError::new("The prop cannot be saved.", "No destination was given."));
    }
    let over_legacy = legacy_source.is_some_and(|source| same_file_path(source, path));
    if over_legacy {
        let backup = legacy_backup_path(path);
        if backup.exists() {
            let header = read_header(&backup);
            if !legacy::is_v4_header(&header) {
                return Err(not_saved("The existing legacy backup is not a LiteDB v4 file and will not be overwritten."));
            }
            write_file(prop, path, None)?;
            return Ok(SaveOutcome::default());
        }
        write_file(prop, path, Some(&backup))?;
        return Ok(SaveOutcome { legacy_backup: backup.exists().then_some(backup) });
    }
    write_file(prop, path, None)?;
    Ok(SaveOutcome::default())
}

fn read_header(path: &Path) -> Vec<u8> {
    let mut header = Vec::new();
    if let Ok(file) = std::fs::File::open(path) {
        let _ = file.take(53).read_to_end(&mut header);
    }
    header
}

/// `PropPackageWriter.WriteAsync`: written beside the destination, read back, then moved into place.
fn write_file(prop: &Prop, destination: &Path, legacy_backup: Option<&Path>) -> Result<(), PropError> {
    let bytes = write_package(prop)?;
    let destination = full_path(destination);
    let (Some(directory), Some(file_name)) = (destination.parent(), destination.file_name()) else {
        return Err(PropError::new("The prop cannot be saved.", "The destination has no directory."));
    };
    let failed = |e: std::io::Error| not_saved(format!("Package creation or publication failed. {e}"));
    std::fs::create_dir_all(directory).map_err(failed)?;
    let temporary = directory.join(format!(".{}.{}.tmp", file_name.to_string_lossy(), Uuid::new_v4().simple()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temporary).map_err(failed)?;
        file.write_all(&bytes).map_err(failed)?;
        file.sync_all().map_err(failed)?;
        drop(file);
        read_package(&std::fs::read(&temporary).map_err(failed)?)?;
        publish(&temporary, &destination, legacy_backup)
    })();
    if temporary.exists() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

/// `AtomicPropFileWriter.PublishAsync`.
fn publish(temporary: &Path, destination: &Path, legacy_backup: Option<&Path>) -> Result<(), PropError> {
    let failed = |e: std::io::Error| not_saved(format!("Package creation or publication failed. {e}"));
    if let Some(backup) = legacy_backup.filter(|_| destination.exists()) {
        if backup.exists() {
            return Err(not_saved("The requested legacy backup already exists and will not be overwritten."));
        }
        std::fs::rename(destination, backup).map_err(failed)?;
        if let Err(e) = std::fs::rename(temporary, destination) {
            let _ = std::fs::rename(backup, destination);
            return Err(failed(e));
        }
        return Ok(());
    }
    std::fs::rename(temporary, destination).map_err(failed)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn sample() -> Prop {
        let mut p = Prop::new("Star", "tester", "2026-11-09T14:03:12.1230000-05:00");
        p.prop_type = "Tree Topper".into();
        p.opacity = 0.5;
        p.vendor.name = "Acme".into();
        p.vendor.website = "https://example.invalid".into();
        p.physical.height = "48 in".into();
        p.physical.node_count = "100".into();
        p.physical.color_mode = ColorMode::Multiple;
        p.information.notes = "Line one\nLine \"two\" \u{e9}".into();
        let arm = p.create_node("Arm", None, false);
        let a = p.add_light(Some(arm), 10.5, 20.25, 4).unwrap();
        p.add_light(Some(a), 30.0, 40.0, 4).unwrap();
        let b = p.add_light(Some(arm), 50.0, 60.0, 3).unwrap();
        let face = p.create_node("Face", None, false);
        p.add_to_parent(b, face);
        let e = p.element_mut(b).unwrap();
        e.face = FaceDefinition { component: FaceComponent::EyesOpen, default_color: Color::argb(255, 1, 2, 3) };
        e.model_type = ElementModelType::FaceInfo;
        let mut definition = StateDefinitionModel::create_default("Doors");
        definition.items[0].element_ids = vec![a, b];
        definition.items[0].color = Color::argb(255, 255, 0, 0);
        p.element_mut(arm).unwrap().state_definitions.push(definition);
        p
    }

    fn same(a: &Prop, b: &Prop) {
        assert_eq!(a.id, b.id);
        assert_eq!(a.root_id(), b.root_id());
        assert_eq!(a.name(), b.name());
        assert_eq!((a.width, a.height, a.opacity), (b.width, b.height, b.opacity));
        assert_eq!((&a.prop_type, &a.created_by, &a.creation_date, &a.modified_date), (&b.prop_type, &b.created_by, &b.creation_date, &b.modified_date));
        assert_eq!((&a.vendor, &a.physical, &a.information), (&b.vendor, &b.physical, &b.information));
        assert_eq!(a.element_count(), b.element_count());
        for id in a.breadth_first() {
            let (x, y) = (a.element(id).unwrap(), b.element(id).unwrap());
            assert_eq!(x.name(), y.name());
            assert_eq!(x.children(), y.children());
            assert_eq!(x.lights(), y.lights());
            assert_eq!((x.state_property_id, x.model_type, x.face), (y.state_property_id, y.model_type, y.face));
            assert_eq!(x.state_definitions, y.state_definitions);
            assert_eq!((a.order(id), a.light_size(id)), (b.order(id), b.light_size(id)));
            let mut xp = x.parents().to_vec();
            let mut yp = y.parents().to_vec();
            xp.sort();
            yp.sort();
            assert_eq!(xp, yp);
        }
    }

    #[test]
    fn a_package_reads_back_as_it_was_written() {
        let prop = sample();
        let bytes = write_package(&prop).unwrap();
        let back = read_package(&bytes).unwrap();
        same(&prop, &back);
        assert_eq!(back.image.bytes, prop.image.bytes, "a JPEG background is stored as it is, not encoded again");
        let again = write_package(&back).unwrap();
        assert_eq!(read_package(&again).unwrap().image.bytes, prop.image.bytes);
        assert_eq!(manifest_json(&back).unwrap(), manifest_json(&prop).unwrap());
    }

    #[test]
    fn the_manifest_has_upstreams_shape() {
        let prop = sample();
        let json = manifest_json(&prop).unwrap();
        assert!(json.starts_with(r#"{"format":"vixen.custom-prop","schemaVersion":1,"prop":{"id":""#), "{json}");
        assert!(json.ends_with(r#""image":{"entryName":"background.jpg","mediaType":"image/jpeg"}}"#));
        assert!(json.contains(r#""opacity":0.5,"width":800,"height":600,"#), "{json}");
        assert!(json.contains(r#""creationDate":"2026-11-09T14:03:12.1230000-05:00""#));
        assert!(json.contains(r#""colorMode":"Multiple""#));
        assert!(json.contains(r##""face":{"component":"EyesOpen","defaultColor":"#FF010203"}"##));
        assert!(json.contains(r#""x":10.5,"y":20.25,"z":0,"size":4}"#), "{json}");
        let value: Value = serde_json::from_str(&json).unwrap();
        let elements = value["elements"].as_array().unwrap();
        assert_eq!(elements[0]["id"].as_str().unwrap(), prop.root_id().to_string());
        assert_eq!(elements[0]["order"], -1, "a group saves the Order property, which is -1");
        assert_eq!(elements[1]["name"], "Arm");
        assert_eq!(elements[1]["lightSize"], 4, "a group saves its first leaf's light size");
        assert_eq!(elements.len(), prop.element_count(), "a shared element is listed once");
    }

    fn package_with(json: &str, image: &[u8]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        writer.start_file(JSON_ENTRY, options).unwrap();
        writer.write_all(json.as_bytes()).unwrap();
        writer.start_file(IMAGE_ENTRY, options).unwrap();
        writer.write_all(image).unwrap();
        writer.finish().unwrap().into_inner()
    }

    fn detail_of(json: &str) -> String {
        let image = PropImage::solid_black(4, 4).bytes;
        let error = read_package(&package_with(json, &image)).unwrap_err();
        assert_eq!(error.message, "The prop package is invalid.");
        error.detail
    }

    fn edited(edit: impl Fn(&mut Value)) -> String {
        let mut value: Value = serde_json::from_str(&manifest_json(&sample()).unwrap()).unwrap();
        edit(&mut value);
        value.to_string()
    }

    #[test]
    fn the_validator_names_each_fault() {
        assert_eq!(detail_of("{"), "The manifest is not valid JSON.");
        assert_eq!(detail_of("null"), "The manifest contains no document.");
        assert_eq!(detail_of(&edited(|v| v["schemaVersion"] = 2.into())), "The prop package uses an unsupported schema version.");
        assert_eq!(detail_of(&edited(|v| v["format"] = "other".into())), "The prop package uses an unsupported schema version.");
        assert_eq!(detail_of(&edited(|v| v["prop"]["vendor"] = Value::Null)), "The prop package document is incomplete.");
        assert_eq!(detail_of(&edited(|v| v["elements"] = Value::Null)), "The prop package document is incomplete.");
        assert_eq!(
            detail_of(&edited(|v| v["prop"]["id"] = "00000000-0000-0000-0000-000000000000".into())),
            "The prop package contains an empty identifier."
        );
        assert_eq!(detail_of(&edited(|v| v["image"]["mediaType"] = "image/png".into())), "The prop package image declaration is invalid.");
        assert_eq!(detail_of(&edited(|v| v["prop"]["width"] = 0.5.into())), "The prop package contains invalid canvas dimensions.");
        assert_eq!(
            detail_of(&edited(|v| {
                let first = v["elements"][1].clone();
                v["elements"].as_array_mut().unwrap().push(first);
            })),
            "The prop package contains duplicate element identifiers."
        );
        assert_eq!(detail_of(&edited(|v| v["elements"][1]["lightSize"] = 0.into())), "The prop package contains an invalid element.");
        assert_eq!(detail_of(&edited(|v| v["elements"][1]["face"] = Value::Null)), "The prop package contains an invalid element.");
        assert_eq!(detail_of(&edited(|v| v["elements"][3]["lights"][0]["size"] = 0.into())), "The prop package contains an invalid light.");
        assert_eq!(
            detail_of(&edited(|v| {
                let id = v["elements"][3]["lights"][0]["id"].clone();
                v["elements"][3]["lights"][1]["id"] = id;
            })),
            "The prop package contains an invalid light."
        );
        assert_eq!(
            detail_of(&edited(|v| v["rootElementId"] = "11111111-1111-1111-1111-111111111111".into())),
            "The prop package root element is missing."
        );
        assert_eq!(
            detail_of(&edited(|v| v["elements"][1]["childIds"][0] = "11111111-1111-1111-1111-111111111111".into())),
            "The prop package contains an invalid element reference."
        );
        assert_eq!(
            detail_of(&edited(|v| {
                let child = v["elements"][1]["childIds"][0].clone();
                v["elements"][1]["childIds"].as_array_mut().unwrap().push(child);
            })),
            "The prop package contains an invalid element reference."
        );
        assert_eq!(
            detail_of(&edited(|v| v["elements"][1]["stateDefinitions"][0]["items"] = Value::Null)),
            "The prop package contains an invalid State definition."
        );
        assert_eq!(
            detail_of(&edited(|v| v["elements"][1]["stateDefinitions"][0]["items"][0]["color"] = "red".into())),
            "The prop package contains an invalid State item."
        );
        assert_eq!(
            detail_of(&edited(|v| {
                let root = v["rootElementId"].clone();
                v["elements"][3]["childIds"] = Value::Array(vec![root]);
                v["elements"][3]["lights"] = Value::Array(vec![]);
            })),
            "The prop package contains an element cycle."
        );
        assert_eq!(
            detail_of(&edited(|v| v["elements"][0]["childIds"].as_array_mut().unwrap().truncate(1))),
            "The prop package contains unreachable elements."
        );
        assert_eq!(detail_of(&edited(|v| v["elements"][1]["modelType"] = "Nope".into())), "The element model type is invalid.");
        assert_eq!(detail_of(&edited(|v| v["elements"][1]["face"]["component"] = "Nope".into())), "The face component is invalid.");
        assert_eq!(detail_of(&edited(|v| v["prop"]["physical"]["colorMode"] = "".into())), "The color mode is invalid.");
        assert_eq!(detail_of(&edited(|v| v["elements"][1]["order"] = "1".into())), "The manifest is not valid JSON.");
        assert_eq!(detail_of(&edited(|v| v["elements"][1]["id"] = "not-a-guid".into())), "The manifest is not valid JSON.");
    }

    #[test]
    fn the_package_itself_is_checked() {
        let json = manifest_json(&sample()).unwrap();
        let image = PropImage::solid_black(4, 4).bytes;

        let error = read_package(b"not a zip at all").unwrap_err();
        assert_eq!((error.message.as_str(), error.detail.as_str()), ("The prop file is not a valid package.", "The file does not have a ZIP local-header signature."));

        let mut truncated = package_with(&json, &image);
        truncated.truncate(truncated.len() / 2);
        assert_eq!(read_package(&truncated).unwrap_err().detail, "The ZIP archive could not be read.");

        let options = zip::write::SimpleFileOptions::default();
        let build = |entries: &[(&str, &[u8])]| {
            let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
            for (name, data) in entries {
                writer.start_file(*name, options).unwrap();
                writer.write_all(data).unwrap();
            }
            writer.finish().unwrap().into_inner()
        };
        let three = build(&[(JSON_ENTRY, json.as_bytes()), (IMAGE_ENTRY, &image), ("extra.txt", b"x")]);
        assert_eq!(read_package(&three).unwrap_err().detail, "The package must contain exactly two entries.");
        let odd = build(&[(JSON_ENTRY, json.as_bytes()), ("sub/background.jpg", &image)]);
        assert_eq!(read_package(&odd).unwrap_err().detail, "The package contains an unsafe or unexpected entry name.");
        let empty = build(&[(JSON_ENTRY, json.as_bytes()), (IMAGE_ENTRY, b"")]);
        assert_eq!(read_package(&empty).unwrap_err().detail, "The package contains an entry with an invalid size or compression ratio.");
        let bomb = build(&[(JSON_ENTRY, json.as_bytes()), (IMAGE_ENTRY, &vec![0u8; 2_000_000])]);
        assert_eq!(read_package(&bomb).unwrap_err().detail, "The package contains an entry with an invalid size or compression ratio.");

        let png = {
            let mut out = Vec::new();
            image::DynamicImage::ImageRgb8(image::RgbImage::new(2, 2)).write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
            out
        };
        let error = read_package(&package_with(&json, &png)).unwrap_err();
        assert_eq!((error.message.as_str(), error.detail.as_str()), ("The prop package image is invalid.", "The image does not have a JPEG signature."));
        let error = read_package(&package_with(&json, &[0xFF, 0xD8, 0xFF, 0x00, 0x01])).unwrap_err();
        assert_eq!(error.message, "The prop package image is invalid.");
        assert!(error.detail.starts_with("The JPEG decoder rejected the image."));
    }

    #[test]
    fn a_non_jpeg_background_is_saved_as_jpeg() {
        let mut prop = sample();
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(image::RgbImage::new(20, 10)).write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        prop.set_image(PropImage::decode(png).unwrap());
        assert_eq!((prop.width, prop.height), (20.0, 10.0));
        let back = read_package(&write_package(&prop).unwrap()).unwrap();
        assert!(back.image.is_jpeg());
        assert_eq!((back.image.width, back.image.height), (20, 10));
    }

    #[test]
    fn an_unusable_prop_is_refused_before_anything_is_written() {
        let mut prop = sample();
        prop.width = 0.0;
        let dir = scratch("refused");
        let path = dir.join("bad.prp");
        let error = save_prop(&prop, &path, None).unwrap_err();
        assert_eq!(error.detail, "The prop package contains invalid canvas dimensions.");
        assert!(!path.exists());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "no temporary file is left behind");
    }

    pub(crate) fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vixen-prop-{name}-{}", Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn saving_writes_a_file_that_loads_and_replaces_the_old_one() {
        let dir = scratch("save");
        let path = dir.join("sub").join("Star.prp");
        let mut prop = sample();
        assert_eq!(save_prop(&prop, &path, None).unwrap(), SaveOutcome::default());
        let loaded = load_prop(&path, "now").unwrap();
        assert_eq!(loaded.format, PropSourceFormat::Package);
        same(&prop, &loaded.prop);

        prop.set_name("Star Two");
        save_prop(&prop, &path, None).unwrap();
        assert_eq!(load_prop(&path, "now").unwrap().prop.name(), "Star Two");
        let names: Vec<String> = std::fs::read_dir(path.parent().unwrap()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["Star.prp"], "no temporary or backup file beside it");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_file_that_is_neither_format_is_not_supported() {
        let error = load_prop_bytes(b"<?xml version=\"1.0\"?><custommodel/>", "now").unwrap_err();
        assert_eq!(error.message, "The prop file format is not supported.");
        assert_eq!(error.detail, "The file is neither a schema-1 package nor a LiteDB v4 data file.");
        assert_eq!(load_prop_bytes(b"", "now").unwrap_err().message, "The prop file format is not supported.");
    }
}
