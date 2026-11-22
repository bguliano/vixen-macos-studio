//! The generic codec between `Params` and XML, driven purely by the `ParamDesc` tables.
//!
//! Values go through their typed form: `ParamValue::Curve` holds a `Curve`, and writing builds its XML from
//! that `Curve`. `ParamKind::Other` is the only kind kept as raw XML on purpose. A value whose XML does not
//! match its table's kind also stays raw, because the alternative is to lose it.
//!
//! `Params::unknown_members` holds two things:
//!
//! - Real members: `(member name, element XML)` for members not in the table, written back in sorted position.
//! - Fidelity hints, whose names start with `@` and which are never a member: `@o:X` and `@p:X` hold what the
//!   file said for member `X` and what typed encoding said at load. See `fidelity.rs` for the idea. They let a
//!   known colour or a nil library name survive unchanged, and they are ignored as soon as the value changes.
//!   UI code should skip entries for which [`is_hint`] is true.

use crate::dcs::{format_bool, format_double, format_guid, parse_bool, parse_double, parse_guid, parse_time};
use crate::emit_default::skipped_when_default;
use crate::error::IoError;
use crate::fixups;
use crate::values::*;
use crate::xml::{escape_text, parse_fragment, same_tree, write_fragment};
use std::collections::HashMap;
use std::sync::OnceLock;
use uuid::Uuid;
use vixen_core::color::Rgb;
use vixen_core::{Color, TimeSpan};
use vixen_effects::framework::{EffectDescriptor, ParamDesc, ParamKind, ParamValue, Params};
use vixen_xml::{contract_ns, XElement, XNode, ARRAYS_NS, CONTRACT_BASE};

const HINT_ORIGINAL: &str = "@o:";
const HINT_PLAIN: &str = "@p:";
const HINT_ORDER: &str = "@order";
const HINT_TYPE: &str = "@type";

/// A table with no members: what an effect type this build does not know decodes against.
pub static NO_PARAMS: [ParamDesc; 0] = [];

/// Whether an `unknown_members` entry is a fidelity hint rather than a real member.
pub fn is_hint(name: &str) -> bool {
    name.starts_with('@')
}

/// The number of members whose XML typed values could not reproduce alone, including nested records.
pub fn hint_count(params: &Params) -> usize {
    let own = params.unknown_members.iter().filter(|(n, _)| n.starts_with(HINT_ORIGINAL)).count();
    own + params.values.iter().map(nested_hint_count).sum::<usize>()
}

fn nested_hint_count(v: &ParamValue) -> usize {
    match v {
        ParamValue::Record(r) => hint_count(r),
        ParamValue::RecordList(l) => l.iter().map(hint_count).sum(),
        _ => 0,
    }
}

/// The members with fidelity hints, for reports. Nested records are not listed.
pub fn hinted_members(params: &Params) -> Vec<&str> {
    params.unknown_members.iter().filter_map(|(n, _)| n.strip_prefix(HINT_ORIGINAL)).collect()
}

/// Removes every member fidelity hint, so encoding uses typed values alone. `@type` stays, because without it
/// an unknown effect could not say what it is.
pub fn strip_hints(params: &mut Params) {
    params.unknown_members.retain(|(n, _)| !is_hint(n) || n == HINT_TYPE);
    for v in &mut params.values {
        match v {
            ParamValue::Record(r) => strip_hints(r),
            ParamValue::RecordList(l) => l.iter_mut().for_each(strip_hints),
            _ => {}
        }
    }
}

/// Reads an object's members against a table, with no base members and no `[OnDeserialized]` logic.
pub fn params_from_xml(e: &XElement, desc: &'static [ParamDesc]) -> Params {
    decode_members(e, desc, false, None)
}

/// Reads an effect's data model: the element inside `_dataModels`. Applies the data class's
/// `[OnDeserialized]` logic, since that decides what the effect renders.
pub fn effect_params_from_xml(model: &XElement, descriptor: &EffectDescriptor) -> Params {
    let mut params = decode_members(model, descriptor.params, true, Some(descriptor.data_type));
    record_type_if_unexpected(&mut params, model, descriptor);
    params
}

fn record_type_if_unexpected(params: &mut Params, model: &XElement, descriptor: &EffectDescriptor) {
    if let Some((ns, local)) = &model.xsi_type {
        let same_ns = ns.strip_prefix(CONTRACT_BASE) == Some(descriptor.data_ns);
        if !same_ns || local != descriptor.data_type {
            record_model_type(params, model);
        }
    }
}

/// Remembers the model's `i:type`, for an effect whose module type id cannot supply it on save.
pub(crate) fn record_model_type(params: &mut Params, model: &XElement) {
    if let Some((ns, local)) = &model.xsi_type {
        params.unknown_members.retain(|(n, _)| n != HINT_TYPE);
        params.unknown_members.push((HINT_TYPE.to_string(), format!("{ns}\n{local}")));
    }
}

/// Reads a data model of an effect type this build does not know: every member stays raw, in file order.
pub fn unknown_params_from_xml(model: &XElement) -> Params {
    let mut unknown: Vec<(String, String)> = Vec::new();
    if let Some((ns, local)) = &model.xsi_type {
        unknown.push((HINT_TYPE.to_string(), format!("{ns}\n{local}")));
    }
    for c in model.elements().filter(|c| !is_base_member(c)) {
        unknown.push((c.local.clone(), write_fragment(c)));
    }
    Params { desc: &NO_PARAMS, values: Vec::new(), unknown_members: unknown }
}

/// Writes an object's members in DataContractSerializer order: base classes first, then ordinal by name.
pub fn params_to_members(params: &Params) -> Vec<XElement> {
    let mut entries: Vec<(i32, XElement)> = Vec::with_capacity(params.desc.len() + params.unknown_members.len());
    for (i, d) in params.desc.iter().enumerate() {
        if d.xml.is_empty() || params.desc[..i].iter().any(|x| x.xml == d.xml) {
            continue;
        }
        let plain = params.values.get(i).and_then(|v| encode_value(d, v));
        let out = match member_hint(params, d.xml) {
            Some((original, at_load)) => {
                let now = plain.as_ref().map(write_fragment).unwrap_or_default();
                if now != at_load {
                    plain
                } else if original.is_empty() {
                    None
                } else {
                    parse_fragment(original).or(plain)
                }
            }
            None => plain,
        };
        if let Some(e) = out {
            entries.push((d.xml_level as i32, e));
        }
    }
    let sorted = !params.desc.is_empty();
    for (name, xml) in &params.unknown_members {
        if !is_hint(name) {
            if let Some(e) = parse_fragment(xml) {
                entries.push((level_of_namespace(&e.ns, params.desc), e));
            }
        }
    }
    if sorted {
        entries.sort_by(|a, b| (a.0, a.1.local.as_str()).cmp(&(b.0, b.1.local.as_str())));
    }
    if let Some(order) = order_hint(params) {
        let order: Vec<&str> = order.split('\n').collect();
        entries.sort_by_key(|(_, e)| order.iter().position(|n| *n == e.local).unwrap_or(usize::MAX));
    }
    entries.into_iter().map(|(_, e)| e).collect()
}

/// Writes one effect's data model as an element named `anyType`, as `_dataModels` holds it.
pub fn effect_model_to_xml(type_id: &Uuid, instance_id: &Uuid, params: &Params) -> Result<XElement, IoError> {
    let xsi_type = model_type(type_id, params).ok_or_else(|| {
        IoError::format(
            format!("effect {instance_id}"),
            format!("type {type_id} is not a known effect and its data has no recorded contract type"),
        )
    })?;
    let mut e = XElement::new(ARRAYS_NS, "anyType");
    e.xsi_type = Some(xsi_type);
    e.push(XElement::with_text("", "ModuleInstanceId", format_guid(instance_id)));
    e.push(XElement::with_text("", "ModuleTypeId", format_guid(type_id)));
    for m in params_to_members(params) {
        e.push(m);
    }
    Ok(e)
}

fn model_type(type_id: &Uuid, params: &Params) -> Option<(String, String)> {
    if let Some((_, v)) = params.unknown_members.iter().find(|(n, _)| n == HINT_TYPE) {
        let (ns, local) = v.split_once('\n')?;
        return Some((ns.to_string(), local.to_string()));
    }
    let d = descriptor(type_id)?;
    Some((contract_ns(d.data_ns), d.data_type.to_string()))
}

/// The effect with this module type id, through an index since a sequence asks once per effect.
pub fn descriptor(type_id: &Uuid) -> Option<&'static EffectDescriptor> {
    static BY_ID: OnceLock<HashMap<Uuid, &'static EffectDescriptor>> = OnceLock::new();
    BY_ID.get_or_init(|| vixen_effect_registry::all().iter().map(|d| (d.type_id, *d)).collect()).get(type_id).copied()
}

fn is_base_member(c: &XElement) -> bool {
    c.ns.is_empty() && (c.local == "ModuleInstanceId" || c.local == "ModuleTypeId")
}

fn member_hint<'a>(params: &'a Params, xml: &str) -> Option<(&'a str, &'a str)> {
    let mut original = None;
    let mut plain = None;
    for (n, v) in &params.unknown_members {
        if !n.starts_with('@') {
            continue;
        }
        if n.strip_prefix(HINT_ORIGINAL) == Some(xml) {
            original = Some(v.as_str());
        } else if n.strip_prefix(HINT_PLAIN) == Some(xml) {
            plain = Some(v.as_str());
        }
    }
    Some((original?, plain?))
}

fn order_hint(params: &Params) -> Option<&str> {
    params.unknown_members.iter().find(|(n, _)| n == HINT_ORDER).map(|(_, v)| v.as_str())
}

fn level_of_namespace(ns: &str, desc: &[ParamDesc]) -> i32 {
    let clr = ns.strip_prefix(CONTRACT_BASE).unwrap_or(ns);
    desc.iter().find(|d| d.xml_ns == clr).map_or(i32::MAX, |d| d.xml_level as i32)
}

fn decode_members(e: &XElement, desc: &'static [ParamDesc], skip_base: bool, data_type: Option<&str>) -> Params {
    let children: Vec<&XElement> = e.elements().filter(|c| !(skip_base && is_base_member(c))).collect();
    let mut used = vec![false; children.len()];
    let mut originals: Vec<Option<&XElement>> = Vec::with_capacity(desc.len());
    let mut values = Vec::with_capacity(desc.len());
    let mut absent: Vec<&'static str> = Vec::new();
    for d in desc {
        if d.xml.is_empty() {
            originals.push(None);
            values.push((d.default)());
            continue;
        }
        match children.iter().position(|c| c.local == d.xml) {
            Some(i) => {
                used[i] = true;
                originals.push(Some(children[i]));
                values.push(decode_value(d, children[i]));
            }
            None => {
                originals.push(None);
                absent.push(d.xml);
                values.push(absent_value(d));
            }
        }
    }
    let unknown = children
        .iter()
        .zip(&used)
        .filter(|(_, used)| !**used)
        .map(|(c, _)| (c.local.clone(), write_fragment(c)))
        .collect();
    let mut params = Params { desc, values, unknown_members: unknown };

    if let Some(data_type) = data_type {
        fixups::on_deserialized(data_type, &mut params, &absent, &originals);
    }

    for (i, d) in desc.iter().enumerate() {
        if d.xml.is_empty() {
            continue;
        }
        let plain = encode_value(d, &params.values[i]);
        let same = match (originals[i], &plain) {
            (Some(o), Some(p)) => same_tree(o, p),
            (None, None) => true,
            _ => false,
        };
        if !same {
            params
                .unknown_members
                .push((format!("{HINT_ORIGINAL}{}", d.xml), originals[i].map(write_fragment).unwrap_or_default()));
            params
                .unknown_members
                .push((format!("{HINT_PLAIN}{}", d.xml), plain.as_ref().map(write_fragment).unwrap_or_default()));
        }
    }

    // A table's level or a file's quirk can put members in an order the sort rule would not.
    let key = |c: &XElement| {
        let level = desc
            .iter()
            .find(|d| d.xml == c.local)
            .map_or_else(|| level_of_namespace(&c.ns, desc), |d| d.xml_level as i32);
        (level, c.local.clone())
    };
    let keys: Vec<(i32, String)> = children.iter().map(|c| key(c)).collect();
    if !desc.is_empty() && keys.windows(2).any(|w| w[0] > w[1]) {
        let order: Vec<&str> = children.iter().map(|c| c.local.as_str()).collect();
        params.unknown_members.push((HINT_ORDER.to_string(), order.join("\n")));
    }
    params
}

/// What a member holds when the file does not have it. DataContractSerializer does not run constructors, so
/// a value type holds `default(T)` rather than the table's constructor default, and `[OnDeserialized]` logic
/// runs after this. A missing reference is `null` in C#, which an effect would throw on, so it takes the
/// table's default instead.
fn absent_value(d: &ParamDesc) -> ParamValue {
    // STUDIO-BEGIN studio-members-absent: a file without a Studio-only effect member reads its table default (#67)
    if crate::studio_effect_members::is_studio_member(d) {
        return (d.default)();
    }
    // STUDIO-END studio-members-absent
    match d.kind {
        ParamKind::Bool => ParamValue::Bool(false),
        ParamKind::Int => ParamValue::Int(0),
        ParamKind::Long => ParamValue::Long(0),
        ParamKind::Double => ParamValue::Double(0.0),
        ParamKind::Percentage => ParamValue::Percentage(0.0),
        ParamKind::Enum(e) => match e.members.first() {
            Some(m) => ParamValue::Enum(m.name.to_string()),
            None => (d.default)(),
        },
        ParamKind::Color => ParamValue::Color(Color::EMPTY),
        ParamKind::Rgb => ParamValue::Rgb(Rgb { r: 0.0, g: 0.0, b: 0.0 }),
        ParamKind::Guid => ParamValue::Guid(Uuid::nil()),
        ParamKind::Time => ParamValue::Time(TimeSpan::ZERO),
        ParamKind::Str => ParamValue::None,
        _ => (d.default)(),
    }
}

fn decode_value(d: &ParamDesc, e: &XElement) -> ParamValue {
    if e.nil {
        return ParamValue::None;
    }
    let text = || e.text();
    let typed = match d.kind {
        ParamKind::Bool => parse_bool(&text()).map(ParamValue::Bool),
        ParamKind::Int => text().trim().parse().ok().map(ParamValue::Int),
        ParamKind::Long => text().trim().parse().ok().map(ParamValue::Long),
        ParamKind::Double => parse_double(&text()).map(ParamValue::Double),
        ParamKind::Str => e.elements().next().is_none().then(|| ParamValue::Str(text())),
        ParamKind::Enum(_) => {
            let t = text();
            (!t.trim().is_empty()).then(|| ParamValue::Enum(t.trim().to_string()))
        }
        ParamKind::Curve => curve_from_xml(e).map(ParamValue::Curve),
        ParamKind::Gradient => gradient_from_xml(e).map(ParamValue::Gradient),
        ParamKind::GradientList => gradient_list_from_xml(e).map(ParamValue::GradientList),
        ParamKind::GradientLevels => level_pair_list_from_xml(e).map(ParamValue::GradientLevels),
        ParamKind::Color => color_from_xml(e).map(ParamValue::Color),
        ParamKind::ColorList => color_list_from_xml(e).map(ParamValue::ColorList),
        ParamKind::Rgb => rgb_from_xml(e).map(ParamValue::Rgb),
        ParamKind::Percentage => percentage_from_xml(e).map(ParamValue::Percentage),
        ParamKind::Font => font_from_xml(e).map(ParamValue::Font),
        ParamKind::Guid => parse_guid(&text()).map(ParamValue::Guid),
        ParamKind::Time => parse_time(&text()).map(ParamValue::Time),
        ParamKind::Record(r) => {
            Some(ParamValue::Record(Box::new(decode_members(e, r.params, false, Some(r.xml_type)))))
        }
        ParamKind::RecordList(r) => Some(ParamValue::RecordList(
            e.elements().map(|item| decode_members(item, r.params, false, Some(r.xml_type))).collect(),
        )),
        ParamKind::Other(_) => None,
    };
    typed.unwrap_or_else(|| ParamValue::Raw(raw_from_element(e)))
}

/// `None` means DataContractSerializer would leave the member out.
fn encode_value(d: &ParamDesc, v: &ParamValue) -> Option<XElement> {
    if holds_type_default(d, v) && skipped_when_default(d.xml_ns, d.xml) {
        return None;
    }
    // STUDIO-BEGIN studio-members-skip: Studio-only effect members stay out of the file at their default (#67)
    if crate::studio_effect_members::skipped(d, v) {
        return None;
    }
    // STUDIO-END studio-members-skip
    let ns = contract_ns(d.xml_ns);
    let local = d.xml;
    Some(match v {
        ParamValue::None => XElement::nil(&ns, local),
        ParamValue::Bool(b) => XElement::with_text(&ns, local, format_bool(*b)),
        ParamValue::Int(i) => XElement::with_text(&ns, local, i.to_string()),
        ParamValue::Long(i) => XElement::with_text(&ns, local, i.to_string()),
        ParamValue::Double(x) => XElement::with_text(&ns, local, format_double(*x)),
        ParamValue::Str(s) | ParamValue::Enum(s) => XElement::with_text(&ns, local, s.as_str()),
        ParamValue::Curve(c) => curve_to_xml(&ns, local, c),
        ParamValue::Gradient(g) => gradient_to_xml(&ns, local, g),
        ParamValue::GradientList(l) => gradient_list_to_xml(&ns, local, l),
        ParamValue::GradientLevels(l) => level_pair_list_to_xml(&ns, local, l),
        ParamValue::Color(c) => color_to_xml(&ns, local, *c),
        ParamValue::ColorList(l) => color_list_to_xml(&ns, local, l),
        ParamValue::Rgb(c) => rgb_to_xml(&ns, local, *c),
        ParamValue::Percentage(x) => percentage_to_xml(&ns, local, *x),
        ParamValue::Font(f) => font_to_xml(&ns, local, f),
        ParamValue::Guid(g) => XElement::with_text(&ns, local, format_guid(g)),
        ParamValue::Time(t) => XElement::with_text(&ns, local, t.to_xml()),
        ParamValue::Record(p) => {
            let mut e = XElement::new(&ns, local);
            for m in params_to_members(p) {
                e.push(m);
            }
            e
        }
        ParamValue::RecordList(items) => {
            let mut e = XElement::new(&ns, local);
            let (item_ns, item_local) = match d.kind {
                ParamKind::RecordList(r) => (contract_ns(r.xml_ns), r.xml_type),
                _ => (ns.clone(), "anyType"),
            };
            for p in items {
                let mut item = XElement::new(&item_ns, item_local);
                for m in params_to_members(p) {
                    item.push(m);
                }
                e.push(item);
            }
            e
        }
        ParamValue::Raw(raw) => raw_to_element(&ns, local, raw),
    })
}

/// `default(T)` for the member's type, for `EmitDefaultValue = false`.
fn holds_type_default(d: &ParamDesc, v: &ParamValue) -> bool {
    match v {
        ParamValue::None => true,
        ParamValue::Bool(b) => !*b,
        ParamValue::Int(i) => *i == 0,
        ParamValue::Long(i) => *i == 0,
        ParamValue::Double(x) | ParamValue::Percentage(x) => *x == 0.0,
        ParamValue::Enum(s) => match d.kind {
            ParamKind::Enum(e) => e.members.first().is_some_and(|m| m.name == s),
            _ => false,
        },
        ParamValue::Color(c) => *c == Color::EMPTY,
        ParamValue::Guid(g) => g.is_nil(),
        ParamValue::Time(t) => *t == TimeSpan::ZERO,
        _ => false,
    }
}

/// `ParamValue::Raw`'s form: the XML of the member's children.
fn raw_from_element(e: &XElement) -> String {
    let has_elements = e.elements().next().is_some();
    let mut out = String::new();
    for n in &e.children {
        match n {
            XNode::Element(c) => out.push_str(&write_fragment(c)),
            XNode::Text(t) => {
                if !has_elements || !t.trim().is_empty() {
                    out.push_str(&escape_text(t));
                }
            }
        }
    }
    out
}

fn raw_to_element(ns: &str, local: &str, raw: &str) -> XElement {
    match XElement::parse(&format!("<r>{raw}</r>")) {
        Ok(wrapper) => {
            let mut e = XElement::new(ns, local);
            e.children = wrapper.children;
            e
        }
        Err(_) => XElement::with_text(ns, local, raw),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vixen_core::{ColorGradient, Curve};
    use vixen_effects::framework::{EnumDesc, EnumMember, FontDesc, RecordDesc};

    const NS: &str = "Spec.Ns";
    const BASE_NS: &str = "Spec.Base";

    fn full(ns: &str) -> String {
        contract_ns(ns)
    }

    macro_rules! param {
        ($name:expr, $xml:expr, $ns:expr, $level:expr, $kind:expr, $default:expr) => {
            ParamDesc {
                name: $name,
                xml: $xml,
                xml_ns: $ns,
                xml_level: $level,
                kind: $kind,
                category: "",
                category_order: 0,
                label: "",
                description: "",
                order: 0,
                browsable: true,
                is_value: true,
                exclude_from_default: false,
                editor: vixen_effects::framework::Editor::Default,
                display: vixen_effects::framework::Display::Stored,
                bool_labels: None,
                default: $default,
            }
        };
    }

    static ENUM: EnumDesc = EnumDesc {
        name: "Dir",
        members: &[EnumMember { name: "Up", label: "Up" }, EnumMember { name: "Down", label: "Down" }],
    };

    static ITEM_PARAMS: [ParamDesc; 1] = [param!("Value", "Value", NS, 0, ParamKind::Int, || ParamValue::Int(7))];
    static ITEM: RecordDesc = RecordDesc { xml_type: "Item", xml_ns: NS, params: &ITEM_PARAMS, visibility: None };

    static TABLE: [ParamDesc; 20] = [
        param!("Target", "TargetPositioning", BASE_NS, 0, ParamKind::Enum(&ENUM), || ParamValue::Enum(
            "Down".to_string()
        )),
        param!("AColor", "AColor", NS, 1, ParamKind::Color, || ParamValue::Color(Color::RED)),
        param!("BList", "BList", NS, 1, ParamKind::ColorList, || ParamValue::ColorList(vec![Color::RED])),
        param!("Curve", "Curve", NS, 1, ParamKind::Curve, || ParamValue::Curve(Curve::flat100())),
        param!("Direction", "Dir", NS, 1, ParamKind::Enum(&ENUM), || ParamValue::Enum("Down".to_string())),
        param!("EFont", "EFont", NS, 1, ParamKind::Font, || ParamValue::Font(FontDesc::new("Arial", 8.0))),
        param!("Flag", "Flag", NS, 1, ParamKind::Bool, || ParamValue::Bool(true)),
        param!("Grad", "Grad", NS, 1, ParamKind::Gradient, || ParamValue::Gradient(ColorGradient::solid(Color::RED))),
        param!("GList", "GList", NS, 1, ParamKind::GradientList, || ParamValue::GradientList(vec![])),
        param!("Id", "Id", NS, 1, ParamKind::Guid, || ParamValue::Guid(Uuid::nil())),
        param!("Int", "Int", NS, 1, ParamKind::Int, || ParamValue::Int(3)),
        param!("Levels", "Levels", NS, 1, ParamKind::GradientLevels, || ParamValue::GradientLevels(vec![])),
        param!("Long", "Long", NS, 1, ParamKind::Long, || ParamValue::Long(3)),
        param!("Other", "Other", NS, 1, ParamKind::Other("Thing"), || ParamValue::None),
        param!("Pct", "Pct", NS, 1, ParamKind::Percentage, || ParamValue::Percentage(0.5)),
        param!("Real", "Real", NS, 1, ParamKind::Double, || ParamValue::Double(1.5)),
        param!("Rec", "Rec", NS, 1, ParamKind::Record(&ITEM), || ParamValue::Record(Box::new(Params::defaults(
            &ITEM_PARAMS
        )))),
        param!("Recs", "Recs", NS, 1, ParamKind::RecordList(&ITEM), || ParamValue::RecordList(vec![])),
        param!("Rgb", "Rgb", NS, 1, ParamKind::Rgb, || ParamValue::Rgb(Rgb::from_color(Color::WHITE))),
        param!("Time", "Time", NS, 1, ParamKind::Time, || ParamValue::Time(TimeSpan::from_ms_i(5))),
    ];

    fn object(params: &Params) -> XElement {
        let mut e = XElement::new(&full(NS), "O");
        for m in params_to_members(params) {
            e.push(m);
        }
        e
    }

    #[test]
    fn every_kind_round_trips_without_hints() {
        let mut p = Params::defaults(&TABLE);
        p.set("Grad", ParamValue::Gradient(ColorGradient::solid(Color::BLUE)));
        p.set("Levels", ParamValue::GradientLevels(vec![vixen_core::GradientLevelPair::solid(Color::RED)]));
        p.set("GList", ParamValue::GradientList(vec![ColorGradient::solid(Color::LIME)]));
        p.set("Recs", ParamValue::RecordList(vec![Params::defaults(&ITEM_PARAMS), Params::defaults(&ITEM_PARAMS)]));
        p.set("Id", ParamValue::Guid(Uuid::from_u128(5)));
        p.set("Other", ParamValue::Raw("<x>1</x>".to_string()));
        let xml = object(&p);
        let back = params_from_xml(&xml, &TABLE);
        assert_eq!(hint_count(&back), 0, "{:?}", back.unknown_members);
        assert_eq!(back.get("Curve"), p.get("Curve"));
        assert_eq!(back.get("Grad"), p.get("Grad"));
        assert_eq!(back.get("Levels"), p.get("Levels"));
        assert_eq!(back.get("GList"), p.get("GList"));
        assert_eq!(back.get("AColor"), p.get("AColor"));
        assert_eq!(back.get("BList"), p.get("BList"));
        assert_eq!(back.get("EFont"), p.get("EFont"));
        assert_eq!(back.get("Rgb"), p.get("Rgb"));
        assert_eq!(back.get("Pct"), &ParamValue::Percentage(0.5));
        assert_eq!(back.get("Time"), &ParamValue::Time(TimeSpan::from_ms_i(5)));
        assert_eq!(back.get("Id"), &ParamValue::Guid(Uuid::from_u128(5)));
        assert_eq!(back.get("Recs"), p.get("Recs"));
        assert_eq!(back.get("Rec"), p.get("Rec"));
        assert_eq!(back.get("Other"), &ParamValue::Raw("<x>1</x>".to_string()));
        assert!(same_tree(&xml, &object(&back)), "{:?}", xml.first_difference(&object(&back), String::new()));
    }

    #[test]
    fn members_are_ordered_base_first_then_ordinal() {
        let xml = object(&Params::defaults(&TABLE));
        let names: Vec<_> = xml.elements().map(|c| c.local.as_str()).collect();
        assert_eq!(names[0], "TargetPositioning");
        let rest = &names[1..];
        let mut sorted = rest.to_vec();
        sorted.sort_unstable();
        assert_eq!(rest, sorted.as_slice());
        // Each member is in its declaring class's namespace.
        assert_eq!(xml.child("TargetPositioning").unwrap().ns, full(BASE_NS));
        assert_eq!(xml.child("Int").unwrap().ns, full(NS));
    }

    #[test]
    fn unknown_members_are_kept_in_their_sorted_position() {
        let mut xml = object(&Params::defaults(&TABLE));
        let at = xml.children.iter().position(|n| matches!(n, XNode::Element(e) if e.local == "Int")).unwrap();
        xml.children.insert(at, XNode::Element(XElement::with_text(&full(NS), "Ignored", "v")));
        xml.children.insert(1, XNode::Element(XElement::with_text(&full(BASE_NS), "Inherited", "w")));
        let p = params_from_xml(&xml, &TABLE);
        assert_eq!(p.unknown_members.iter().filter(|(n, _)| !is_hint(n)).count(), 2);
        assert_eq!(hint_count(&p), 0);
        assert!(same_tree(&xml, &object(&p)), "{:?}", xml.first_difference(&object(&p), String::new()));
    }

    #[test]
    fn a_nil_member_reads_as_none_and_writes_as_nil() {
        let mut xml = object(&Params::defaults(&TABLE));
        *xml.child_mut("Curve").unwrap() = XElement::nil(&full(NS), "Curve");
        let p = params_from_xml(&xml, &TABLE);
        assert_eq!(p.get("Curve"), &ParamValue::None);
        assert_eq!(hint_count(&p), 0);
        assert!(same_tree(&xml, &object(&p)));
    }

    #[test]
    fn an_absent_value_type_is_the_csharp_default_not_the_constructors() {
        let xml = XElement::new(&full(NS), "O");
        let p = params_from_xml(&xml, &TABLE);
        assert_eq!(p.get("Flag"), &ParamValue::Bool(false));
        assert_eq!(p.get("Int"), &ParamValue::Int(0));
        assert_eq!(p.get("Real"), &ParamValue::Double(0.0));
        assert_eq!(p.get("Direction"), &ParamValue::Enum("Up".to_string()));
        assert_eq!(p.get("AColor"), &ParamValue::Color(Color::EMPTY));
        // A missing reference takes the table's value, as an effect could not use null.
        assert_eq!(p.get("Curve"), &ParamValue::Curve(Curve::flat100()));
    }

    #[test]
    fn absent_members_stay_absent_until_they_change() {
        let xml = XElement::new(&full(NS), "O");
        let mut p = params_from_xml(&xml, &TABLE);
        assert!(same_tree(&xml, &object(&p)), "{:?}", xml.first_difference(&object(&p), String::new()));
        p.set("Int", ParamValue::Int(9));
        let out = object(&p);
        assert_eq!(out.child_text("Int").as_deref(), Some("9"));
        assert_eq!(out.elements().count(), 1);
    }

    #[test]
    fn a_known_colour_survives_until_it_changes() {
        let mut xml = object(&Params::defaults(&TABLE));
        let known = DotNetColor::from_known(known_color_named("White").unwrap()).to_xml(&full(NS), "AColor");
        *xml.child_mut("AColor").unwrap() = known;
        let mut p = params_from_xml(&xml, &TABLE);
        assert_eq!(p.get("AColor"), &ParamValue::Color(Color::WHITE));
        assert_eq!(hinted_members(&p), ["AColor"]);
        assert!(same_tree(&xml, &object(&p)));

        p.set("AColor", ParamValue::Color(Color::RED));
        let out = object(&p);
        assert_eq!(out.child("AColor").unwrap().child_text("state").as_deref(), Some("2"));
        assert_eq!(out.child("AColor").unwrap().child_text("value").as_deref(), Some("4294901760"));
    }

    #[test]
    fn a_nil_library_name_survives_inside_a_curve() {
        let mut xml = object(&Params::defaults(&TABLE));
        let name = XElement::nil(NS_CURVES, "_libraryReferenceName");
        *xml.child_mut("Curve").unwrap().child_mut("_libraryReferenceName").unwrap() = name;
        let p = params_from_xml(&xml, &TABLE);
        assert_eq!(p.curve("Curve").library_reference_name, "");
        assert!(same_tree(&xml, &object(&p)));
    }

    #[test]
    fn stripping_hints_leaves_the_typed_encoding() {
        let mut xml = object(&Params::defaults(&TABLE));
        *xml.child_mut("AColor").unwrap() = DotNetColor::from_known(164).to_xml(&full(NS), "AColor");
        let mut p = params_from_xml(&xml, &TABLE);
        strip_hints(&mut p);
        assert_eq!(hint_count(&p), 0);
        assert_eq!(object(&p).child("AColor").unwrap().child_text("state").as_deref(), Some("2"));
    }

    #[test]
    fn text_a_kind_cannot_parse_stays_raw_and_comes_back() {
        let mut xml = object(&Params::defaults(&TABLE));
        *xml.child_mut("Int").unwrap() = XElement::with_text(&full(NS), "Int", "twelve");
        let p = params_from_xml(&xml, &TABLE);
        assert_eq!(p.get("Int"), &ParamValue::Raw("twelve".to_string()));
        assert_eq!(p.int("Int"), 0);
        assert!(same_tree(&xml, &object(&p)));
    }

    #[test]
    fn other_kinds_keep_type_and_nil_through_hints() {
        let mut xml = object(&Params::defaults(&TABLE));
        let mut other = XElement::new(&full(NS), "Other");
        other.xsi_type = Some((full(NS), "Thing".to_string()));
        other.push(XElement::with_text(&full(NS), "Inner", "1"));
        *xml.child_mut("Other").unwrap() = other;
        let p = params_from_xml(&xml, &TABLE);
        assert!(matches!(p.get("Other"), ParamValue::Raw(_)));
        assert!(same_tree(&xml, &object(&p)), "{:?}", xml.first_difference(&object(&p), String::new()));
    }

    #[test]
    fn members_out_of_order_keep_the_files_order() {
        let mut xml = XElement::new(&full(NS), "O");
        xml.push(XElement::with_text(&full(NS), "Int", "1"));
        xml.push(XElement::with_text(&full(NS), "Flag", "true"));
        let p = params_from_xml(&xml, &TABLE);
        assert!(same_tree(&xml, &object(&p)), "{:?}", xml.first_difference(&object(&p), String::new()));
    }

    #[test]
    fn repeated_members_are_not_lost() {
        let mut xml = XElement::new(&full(NS), "O");
        xml.push(XElement::with_text(&full(NS), "Int", "1"));
        xml.push(XElement::with_text(&full(NS), "Int", "2"));
        let p = params_from_xml(&xml, &TABLE);
        assert_eq!(p.int("Int"), 1);
        assert!(same_tree(&xml, &object(&p)));
    }

    #[test]
    fn unknown_effects_keep_every_member_in_file_order() {
        let mut model = XElement::new(ARRAYS_NS, "anyType");
        model.xsi_type = Some((full("Some.Future.Effect"), "FutureData".to_string()));
        let id = Uuid::from_u128(1);
        let ty = Uuid::from_u128(2);
        model.push(XElement::with_text("", "ModuleInstanceId", format_guid(&id)));
        model.push(XElement::with_text("", "ModuleTypeId", format_guid(&ty)));
        model.push(XElement::with_text(&full("Some.Future.Effect"), "Zed", "1"));
        model.push(XElement::nil(&full("Some.Future.Effect"), "Alpha"));
        let p = unknown_params_from_xml(&model);
        assert!(p.desc.is_empty());
        let out = effect_model_to_xml(&ty, &id, &p).unwrap();
        assert!(same_tree(&model, &out), "{:?}", model.first_difference(&out, String::new()));
    }

    #[test]
    fn an_effect_with_no_contract_cannot_be_written() {
        let p = Params { desc: &NO_PARAMS, values: Vec::new(), unknown_members: Vec::new() };
        assert!(effect_model_to_xml(&Uuid::from_u128(2), &Uuid::from_u128(1), &p).is_err());
    }

    #[test]
    fn a_model_written_from_a_table_says_what_it_is() {
        let d = vixen_effect_registry::by_data_type("SetLevelData").unwrap();
        let id = Uuid::from_u128(1);
        let e = effect_model_to_xml(&d.type_id, &id, &d.new_params()).unwrap();
        assert_eq!(e.local, "anyType");
        assert_eq!(e.ns, ARRAYS_NS);
        assert_eq!(e.xsi_type, Some((contract_ns(d.data_ns), "SetLevelData".to_string())));
        assert_eq!(e.child_text("ModuleTypeId"), Some(format_guid(&d.type_id)));
    }

    #[test]
    fn descriptors_are_found_by_type_id() {
        let d = vixen_effect_registry::by_data_type("PulseData").unwrap();
        assert_eq!(descriptor(&d.type_id).unwrap().name, d.name);
        assert!(descriptor(&Uuid::nil()).is_none());
    }
}
