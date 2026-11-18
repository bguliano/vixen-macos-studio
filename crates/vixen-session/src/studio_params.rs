//! JSON parameter codec using native effect descriptors; unsupported raw XML is never accepted.
use serde_json::{json, Value};
use vixen_core::{Color, ColorGradient, Curve, TimeSpan};
use vixen_effects::framework::{ParamKind, ParamValue, Params};

pub fn schema(kind: ParamKind) -> Value {
    let color = json!({"type":"string","pattern":"^#[0-9a-fA-F]{6}$","description":"#RRGGBB"});
    match kind {
        ParamKind::Bool => json!({"type":"boolean"}),
        ParamKind::Int => json!({"type":"integer","minimum":i32::MIN,"maximum":i32::MAX}),
        ParamKind::Long => json!({"type":"integer"}),
        ParamKind::Double => json!({"type":"number"}),
        ParamKind::Percentage => json!({"type":"number","minimum":0,"maximum":1}),
        ParamKind::Time => {
            json!({"type":"number","minimum":0,"maximum":3600000,"description":"milliseconds"})
        }
        ParamKind::Str => json!({"type":"string","maxLength":4096}),
        ParamKind::Guid => json!({"type":"string","format":"uuid"}),
        ParamKind::Enum(desc) => {
            json!({"type":"string","enum":desc.members.iter().map(|m|m.name).collect::<Vec<_>>()})
        }
        ParamKind::Color | ParamKind::Rgb => color,
        ParamKind::ColorList | ParamKind::Gradient => {
            json!({"type":"array","items":color,"minItems":1,"maxItems":64,"description":"Evenly spaced colors; for gradient, interpolated by Vixen"})
        }
        ParamKind::GradientList => {
            json!({"type":"array","items":schema(ParamKind::Gradient),"maxItems":64})
        }
        ParamKind::Curve => {
            json!({"type":"array","items":{"type":"array","items":{"type":"number","minimum":0,"maximum":100},"minItems":2,"maxItems":2},"minItems":2,"maxItems":128,"description":"[x,y] curve control points, both 0..100"})
        }
        ParamKind::Record(desc) => {
            json!({"type":"object","properties":desc.params.iter().map(|p|(p.name.to_string(),schema(p.kind))).collect::<serde_json::Map<_,_>>(),"additionalProperties":false})
        }
        ParamKind::RecordList(desc) => {
            json!({"type":"array","items":schema(ParamKind::Record(desc)),"maxItems":128})
        }
        _ => {
            json!({"not":{},"description":"Not yet supported by Studio JSON; use the native editor"})
        }
    }
}
fn color(value: &Value) -> Result<Color, String> {
    let s = value.as_str().ok_or("Expected #RRGGBB")?;
    if s.len() != 7 || !s.starts_with('#') {
        return Err("Expected #RRGGBB".into());
    }
    let n = u32::from_str_radix(&s[1..], 16).map_err(|_| "Invalid hex color")?;
    Ok(Color::from_argb_u32(n | 0xff000000))
}
fn array(value: &Value, max: usize) -> Result<&Vec<Value>, String> {
    value
        .as_array()
        .filter(|a| a.len() <= max)
        .ok_or(format!("Expected array with at most {max} entries"))
}
pub fn decode(kind: ParamKind, v: &Value) -> Result<ParamValue, String> {
    Ok(match kind {
        ParamKind::Bool => ParamValue::Bool(v.as_bool().ok_or("Expected boolean")?),
        ParamKind::Int => ParamValue::Int(
            v.as_i64()
                .and_then(|n| i32::try_from(n).ok())
                .ok_or("Expected 32-bit integer")?,
        ),
        ParamKind::Long => ParamValue::Long(v.as_i64().ok_or("Expected 64-bit integer")?),
        ParamKind::Double => ParamValue::Double(
            v.as_f64()
                .filter(|n| n.is_finite())
                .ok_or("Expected finite number")?,
        ),
        ParamKind::Percentage => ParamValue::Percentage(
            v.as_f64()
                .filter(|n| n.is_finite() && (0.0..=1.0).contains(n))
                .ok_or("Expected percentage fraction 0..1")?,
        ),
        ParamKind::Time => ParamValue::Time(TimeSpan::from_ms(
            v.as_f64()
                .filter(|n| n.is_finite() && (0.0..=3600000.0).contains(n))
                .ok_or("Expected milliseconds 0..3600000")?,
        )),
        ParamKind::Str => ParamValue::Str(
            v.as_str()
                .filter(|s| s.len() <= 4096)
                .ok_or("Expected string <=4096 bytes")?
                .into(),
        ),
        ParamKind::Guid => ParamValue::Guid(
            v.as_str()
                .and_then(|s| uuid::Uuid::parse_str(s).ok())
                .ok_or("Expected UUID")?,
        ),
        ParamKind::Enum(desc) => {
            let value = v.as_str().ok_or("Expected enum member name")?;
            if !desc.members.iter().any(|m| m.name == value) {
                return Err("Unknown enum member".into());
            }
            ParamValue::Enum(value.into())
        }
        ParamKind::Color => ParamValue::Color(color(v)?),
        ParamKind::Rgb => ParamValue::Rgb(vixen_core::color::Rgb::from_color(color(v)?)),
        ParamKind::ColorList | ParamKind::Gradient => {
            let values = array(v, 64)?;
            if values.is_empty() {
                return Err("At least one color required".into());
            }
            let colors = values.iter().map(color).collect::<Result<Vec<_>, _>>()?;
            if matches!(kind, ParamKind::ColorList) {
                ParamValue::ColorList(colors)
            } else {
                ParamValue::Gradient(ColorGradient::from_colors(&colors))
            }
        }
        ParamKind::GradientList => ParamValue::GradientList(
            array(v, 64)?
                .iter()
                .map(|item| match decode(ParamKind::Gradient, item)? {
                    ParamValue::Gradient(g) => Ok(g),
                    _ => unreachable!(),
                })
                .collect::<Result<Vec<_>, String>>()?,
        ),
        ParamKind::Curve => {
            let items = array(v, 128)?;
            if items.len() < 2 {
                return Err("At least two curve points required".into());
            }
            let mut points = Vec::new();
            for pair in items {
                let pair = array(pair, 2)?;
                if pair.len() != 2 {
                    return Err("Curve points are [x,y]".into());
                }
                let x = pair[0]
                    .as_f64()
                    .filter(|n| n.is_finite() && (0.0..=100.0).contains(n))
                    .ok_or("Curve x must be 0..100")?;
                let y = pair[1]
                    .as_f64()
                    .filter(|n| n.is_finite() && (0.0..=100.0).contains(n))
                    .ok_or("Curve y must be 0..100")?;
                points.push((x, y));
            }
            points.sort_by(|a, b| a.0.total_cmp(&b.0));
            if points.windows(2).any(|p| p[0].0 == p[1].0) {
                return Err("Curve x positions must be distinct".into());
            }
            ParamValue::Curve(Curve::from_points(points))
        }
        ParamKind::Record(desc) => {
            let values = v.as_object().ok_or("Expected record object")?;
            let mut p = Params::defaults(desc.params);
            for (key, value) in values {
                let field = desc
                    .params
                    .iter()
                    .find(|f| f.name == key)
                    .ok_or("Unknown record field")?;
                p.set(key, decode(field.kind, value)?);
            }
            ParamValue::Record(Box::new(p))
        }
        ParamKind::RecordList(desc) => ParamValue::RecordList(
            array(v, 128)?
                .iter()
                .map(|value| match decode(ParamKind::Record(desc), value)? {
                    ParamValue::Record(p) => Ok(*p),
                    _ => unreachable!(),
                })
                .collect::<Result<Vec<_>, String>>()?,
        ),
        _ => return Err("This parameter type is not supported by Studio JSON yet".into()),
    })
}

pub fn encode(value: &ParamValue) -> Value {
    let hex = |c: Color| format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b);
    match value {
        ParamValue::None => Value::Null,
        ParamValue::Bool(v) => json!(v),
        ParamValue::Int(v) => json!(v),
        ParamValue::Long(v) => json!(v),
        ParamValue::Double(v) | ParamValue::Percentage(v) => json!(v),
        ParamValue::Str(v) | ParamValue::Enum(v) => json!(v),
        ParamValue::Guid(v) => json!(v.to_string()),
        ParamValue::Time(v) => json!(v.total_ms()),
        ParamValue::Color(v) => json!(hex(*v)),
        ParamValue::Rgb(v) => json!(hex(v.to_color())),
        ParamValue::ColorList(v) => json!(v.iter().map(|c| hex(*c)).collect::<Vec<_>>()),
        ParamValue::Curve(v) => json!(v.points),
        ParamValue::Gradient(v) => {
            json!({"colors":v.colors().iter().map(|c|json!({"color":hex(c.rgb_color()),"position":c.position})).collect::<Vec<_>>(),"note":"Writable JSON uses evenly-spaced color arrays; this reports native stop positions"})
        }
        ParamValue::GradientList(v) => json!(v
            .iter()
            .map(|g| encode(&ParamValue::Gradient(g.clone())))
            .collect::<Vec<_>>()),
        ParamValue::Record(v) => json!(v
            .desc
            .iter()
            .map(|p| (p.name.to_string(), encode(v.get(p.name))))
            .collect::<serde_json::Map<_, _>>()),
        ParamValue::RecordList(v) => json!(v
            .iter()
            .map(|p| encode(&ParamValue::Record(Box::new(p.clone()))))
            .collect::<Vec<_>>()),
        _ => json!({"supported":false,"note":"Native value requires the native editor"}),
    }
}
