//! JSON parameter codec using native effect descriptors; unsupported raw XML is never accepted.
use serde_json::{json, Value};
use vixen_core::{Color, ColorGradient, ColorPoint, Curve, GradientLevelPair, TimeSpan};
use vixen_effects::framework::{Display, Editor, ParamDesc, ParamKind, ParamValue, Params};

/// Largest magnitude accepted for a number whose native editor gives no range.
pub const NUMERIC_SAFETY_LIMIT: f64 = 100000.0;

/// The stored-value range a numeric parameter accepts: its native editor's range taken through the property's
/// getter/setter conversion, the 0..1 of level and percentage editors, or the safety limit when the editor has none.
pub fn range(p: &ParamDesc) -> Option<(f64, f64)> {
    match p.kind {
        ParamKind::Percentage => return Some((0.0, 1.0)),
        ParamKind::Time => return Some((0.0, 3600000.0)),
        ParamKind::Int | ParamKind::Long | ParamKind::Double => {}
        _ => return None,
    }
    Some(match p.editor {
        Editor::Number { min, max, .. } | Editor::Slider { min, max, .. } | Editor::SliderDouble { min, max, .. } => {
            match p.display {
                Display::Stored => (min, max),
                Display::Times(k) => (min / k as f64, max / k as f64),
                Display::Minus(k) => (k as f64 - max, k as f64 - min),
            }
        }
        Editor::Level | Editor::Percent => (0.0, 1.0),
        _ => (-NUMERIC_SAFETY_LIMIT, NUMERIC_SAFETY_LIMIT),
    })
}

/// The writable schema of one native parameter: its type's shape with the real range, unit and default.
pub fn param_schema(p: &ParamDesc) -> Value {
    let mut s = match p.kind {
        ParamKind::Record(desc) => record_schema(desc.params),
        ParamKind::RecordList(desc) => {
            json!({"type":"array","items":record_schema(desc.params),"maxItems":128})
        }
        kind => schema(kind),
    };
    if let Some((min, max)) = range(p) {
        let integer = matches!(p.kind, ParamKind::Int | ParamKind::Long);
        let bound = |n: f64| if integer { json!(n.round() as i64) } else { json!(n) };
        s["minimum"] = bound(min);
        s["maximum"] = bound(max);
    }
    let unit = match (p.kind, p.editor) {
        (ParamKind::Time, _) => Some("milliseconds"),
        (ParamKind::Percentage, _) | (_, Editor::Level | Editor::Percent) => Some("fraction"),
        _ => None,
    };
    if let Some(unit) = unit {
        s["unit"] = json!(unit);
    }
    let shown = match (p.editor, p.display) {
        (Editor::Level | Editor::Percent, _) => Some("0..1; the native editor shows it as 0..100%".to_string()),
        (Editor::Number { min, max, .. } | Editor::Slider { min, max, .. }, Display::Times(k)) => {
            Some(format!("Stored value; the native editor shows it multiplied by {k} ({min}..{max})"))
        }
        (Editor::Number { .. } | Editor::Slider { .. }, Display::Minus(k)) => {
            Some(format!("Stored value; the native editor shows {k} minus it"))
        }
        _ => None,
    };
    if let Some(text) = shown {
        s["description"] = json!(text);
    }
    if let Some(default) = writable(&(p.default)()) {
        s["default"] = default;
    }
    s
}

fn record_schema(params: &[ParamDesc]) -> Value {
    json!({"type":"object","properties":params.iter().map(|p|(p.name.to_string(),param_schema(p))).collect::<serde_json::Map<_,_>>(),"additionalProperties":false})
}

/// A gradient through the given colours at evenly spaced stops, as the writable JSON describes it.
fn even_gradient(colors: &[Color]) -> ColorGradient {
    let mut gradient = ColorGradient::solid(colors[0]);
    if colors.len() > 1 {
        let last = (colors.len() - 1) as f64;
        gradient.set_colors(colors.iter().enumerate().map(|(i, &c)| ColorPoint::new(c, i as f64 / last)).collect());
    }
    gradient
}

/// The value in the form decode accepts, when that form can express it exactly.
fn writable(value: &ParamValue) -> Option<Value> {
    let hex = |c: Color| json!(format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b));
    let gradient = |g: &ColorGradient| {
        let colors = g.colors().iter().map(|p| p.rgb_color()).collect::<Vec<_>>();
        (!colors.is_empty() && *g == even_gradient(&colors)).then(|| json!(colors.iter().map(|&c| hex(c)).collect::<Vec<_>>()))
    };
    Some(match value {
        ParamValue::Gradient(g) => gradient(g)?,
        ParamValue::GradientList(v) => json!(v.iter().map(gradient).collect::<Option<Vec<_>>>()?),
        ParamValue::GradientLevels(v) => json!(v
            .iter()
            .map(|g| Some(json!({"gradient":gradient(&g.color_gradient)?,"curve":g.curve.points})))
            .collect::<Option<Vec<_>>>()?),
        ParamValue::Record(p) => json!(p
            .desc
            .iter()
            .map(|d| Some((d.name.to_string(), writable(p.get(d.name))?)))
            .collect::<Option<serde_json::Map<_, _>>>()?),
        ParamValue::RecordList(v) => json!(v
            .iter()
            .map(|p| writable(&ParamValue::Record(Box::new(p.clone()))))
            .collect::<Option<Vec<_>>>()?),
        ParamValue::None => return None,
        other => match encode(other) {
            Value::Object(o) if o.contains_key("supported") => return None,
            v => v,
        },
    })
}

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
        ParamKind::ColorList => {
            json!({"title":"colorList","type":"array","items":color,"minItems":1,"maxItems":64,"description":"A flat list of #RRGGBB colors, each used on its own, e.g. [\"#ff0000\",\"#00ff00\"]"})
        }
        ParamKind::Gradient => {
            json!({"title":"gradient","type":"array","items":color,"minItems":1,"maxItems":64,"description":"One gradient: a flat list of #RRGGBB colors at evenly spaced stops, blended by Vixen; one color is solid. E.g. [\"#ffffff\",\"#00b4ff\"]"})
        }
        ParamKind::GradientList => {
            json!({"title":"gradientList","type":"array","items":schema(ParamKind::Gradient),"maxItems":64,"description":"A list of gradients, each a flat list of #RRGGBB colors, e.g. [[\"#ff0000\"],[\"#ffffff\",\"#00b4ff\"]]"})
        }
        ParamKind::GradientLevels => {
            json!({"type":"array","items":{"type":"object","properties":{"gradient":schema(ParamKind::Gradient),"curve":schema(ParamKind::Curve)},"required":["gradient"],"additionalProperties":false},"minItems":1,"maxItems":64,"description":"Gradient and brightness curve pairs, used in order (Alternating cycles through them). curve defaults to flat 100%"})
        }
        ParamKind::Curve => {
            json!({"type":"array","items":{"type":"array","items":{"type":"number","minimum":0,"maximum":100},"minItems":2,"maxItems":2},"minItems":2,"maxItems":128,"description":"[x,y] curve control points, both 0..100"})
        }
        ParamKind::Record(desc) => record_schema(desc.params),
        ParamKind::RecordList(desc) => {
            json!({"type":"array","items":record_schema(desc.params),"maxItems":128})
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
            let shape = if matches!(kind, ParamKind::ColorList) { "colorList" } else { "gradient" };
            let values = array(v, 64).map_err(|e| format!("{e}: a {shape} is a flat list of #RRGGBB colors"))?;
            if values.is_empty() {
                return Err("At least one color required".into());
            }
            if values.iter().any(Value::is_array) {
                return Err(format!(
                    "Expected a {shape}: a flat list of #RRGGBB colors, not a list of lists (that is a gradientList)"
                ));
            }
            let colors = values.iter().map(color).collect::<Result<Vec<_>, _>>()?;
            if matches!(kind, ParamKind::ColorList) {
                ParamValue::ColorList(colors)
            } else {
                ParamValue::Gradient(even_gradient(&colors))
            }
        }
        ParamKind::GradientList => {
            let items = array(v, 64)?;
            if items.iter().any(|item| !item.is_array()) {
                return Err(
                    "Expected a gradientList: a list of gradients, each a list of #RRGGBB colors, e.g. [[\"#ff0000\"]]"
                        .into(),
                );
            }
            ParamValue::GradientList(
                items
                    .iter()
                    .map(|item| match decode(ParamKind::Gradient, item)? {
                        ParamValue::Gradient(g) => Ok(g),
                        _ => unreachable!(),
                    })
                    .collect::<Result<Vec<_>, String>>()?,
            )
        }
        ParamKind::GradientLevels => {
            let items = array(v, 64)?;
            if items.is_empty() {
                return Err("At least one gradient required".into());
            }
            let mut pairs = Vec::new();
            for item in items {
                let item = item
                    .as_object()
                    .ok_or("Expected {gradient, curve} object")?;
                if let Some(key) = item.keys().find(|k| *k != "gradient" && *k != "curve") {
                    return Err(format!("Unknown gradient level field {key}"));
                }
                let ParamValue::Gradient(gradient) = decode(
                    ParamKind::Gradient,
                    item.get("gradient").ok_or("gradient is required")?,
                )?
                else {
                    unreachable!()
                };
                let curve = match item.get("curve") {
                    Some(c) => match decode(ParamKind::Curve, c)? {
                        ParamValue::Curve(c) => c,
                        _ => unreachable!(),
                    },
                    None => Curve::flat100(),
                };
                pairs.push(GradientLevelPair::new(gradient, curve));
            }
            ParamValue::GradientLevels(pairs)
        }
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
            json!({"colors":v.colors().iter().map(|c|json!({"color":hex(c.rgb_color()),"position":c.position})).collect::<Vec<_>>(),"note":"Native stop positions (0..1). Writable JSON is a flat color list placed at evenly spaced stops"})
        }
        ParamValue::GradientList(v) => json!(v
            .iter()
            .map(|g| encode(&ParamValue::Gradient(g.clone())))
            .collect::<Vec<_>>()),
        ParamValue::GradientLevels(v) => json!(v
            .iter()
            .map(|g| json!({"gradient":encode(&ParamValue::Gradient(g.color_gradient.clone())),"curve":g.curve.points}))
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

/// What a parameter does where the native label doesn't say: how the grouping parameters pick the nodes an
/// effect works on, and the motion parameters whose zero isn't "still" or that a mode ignores.
pub fn note(effect: &str, param: &str) -> Option<&'static str> {
    if let Some(n) = motion_note(effect, param) {
        return Some(n);
    }
    let depth_zero = match effect {
        "Alternating" => "0 = the targets themselves",
        "Dissolve" => "0 = leaf pixels (same as EnableDepth false)",
        _ => return None,
    };
    Some(match (param, depth_zero) {
        ("EnableDepth", _) => "false: the effect works on leaf pixels, so GroupLevel counts pixels. true: it works on the nodes DepthOfEffect levels below each target, e.g. a megatree group -> strings -> pixels with DepthOfEffect 1 works on whole strings.",
        ("DepthOfEffect", "0 = the targets themselves") => "Used only when EnableDepth is true. Levels to descend from each target: 0 = the targets themselves, 1 = their children (a megatree group's strings), 2 = grandchildren. If that level is empty, leaf pixels are used.",
        ("DepthOfEffect", _) => "Used only when EnableDepth is true. Levels to descend from each target: 0 = leaf pixels (same as EnableDepth false), 1 = their children (a megatree group's strings), 2 = grandchildren. Values deeper than the targets' tree reset to 0. If that level is empty, leaf pixels are used.",
        ("GroupLevel", _) => "How many consecutive nodes (chosen by EnableDepth/DepthOfEffect) share one state. With EnableDepth false these are pixels: to alternate whole strings, set EnableDepth true, DepthOfEffect 1 and GroupLevel 1.",
        ("Colors", _) if effect == "Alternating" => "Each group takes the next entry in turn, cycling through the list.",
        _ => return None,
    })
}

/// Bars and Wipe motion, matching upstream Vixen (Bars.CalculateSpeed, Wipe.RenderMovement).
fn motion_note(effect: &str, param: &str) -> Option<&'static str> {
    Some(match (effect, param) {
        ("Bars", "MovementType") => "Iterations: Speed (labelled Iterations) sets how many times the bars cycle over the effect; Speed 0 is static. Speed: SpeedCurve sets the speed over time. Zig Zag bars always use Speed.",
        ("Bars", "Speed") => "Used when MovementType is Iterations: how many times the bars cycle over the effect. 0 keeps the bars still.",
        ("Bars", "SpeedCurve") => "Used when MovementType is Speed (and by Zig Zag bars). Flat bars: curve 0..100 maps to speed -15..+15, so 50 is still, values below and above 50 move in opposite directions, and 0 and 100 are the fastest. Zig Zag bars: 0 is still.",
        ("Wipe", "WipeMovement") => "Count and PulseLength sweep across the targets on their own. Movement places the pulse band by MovementCurve instead; it ignores ReverseDirection, WipeOn, WipeOff, PassCount and ColorHandling.",
        ("Wipe", "MovementCurve") => "Used when WipeMovement is Movement: the leading edge of the pulse band over time. 0 = just before the first row in Direction's order (nothing lit), 100 = the band has fully passed the last row (nothing lit); the range covers the targets plus one band width. To run the other way, make the curve fall. Only the band is lit; passed rows go dark.",
        ("Wipe", "ReverseDirection") => "Count and PulseLength only. In Movement mode the direction comes from MovementCurve: a falling curve moves backwards.",
        ("Wipe", "WipeOn") => "Count only: on the last pass, rows stay lit once the wipe reaches them. Movement mode has no equivalent; only the pulse band is lit.",
        ("Wipe", "ReverseColorDirection") => "Movement only: flips the gradient across the band whenever MovementCurve falls, so the band's leading colour follows its travel.",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn effect(name: &str) -> &'static vixen_effects::framework::EffectDescriptor {
        vixen_effect_registry::all()
            .iter()
            .copied()
            .find(|d| d.name == name)
            .unwrap()
    }

    #[test]
    fn colors_are_writable_on_gradient_level_effects() {
        for name in ["Alternating", "Pinwheel", "Dissolve"] {
            let colors = effect(name)
                .params
                .iter()
                .find(|p| p.name == "Colors")
                .unwrap();
            assert!(
                schema(colors.kind).get("not").is_none(),
                "{name} Colors schema rejects everything"
            );
            let value = json!([
                {"gradient":["#0000ff"]},
                {"gradient":["#ffff00","#ff8000"],"curve":[[0,0],[100,50]]}
            ]);
            let ParamValue::GradientLevels(pairs) = decode(colors.kind, &value).unwrap() else {
                panic!()
            };
            assert_eq!(pairs.len(), 2);
            assert_eq!(
                pairs[0].color_gradient,
                ColorGradient::from_colors(&[Color::from_argb_u32(0xff0000ff)])
            );
            assert_eq!(pairs[0].curve, Curve::flat100());
            assert_eq!(pairs[1].curve.points, vec![(0.0, 0.0), (100.0, 50.0)]);
            let encoded = encode(&ParamValue::GradientLevels(pairs));
            assert_eq!(encoded[1]["curve"], json!([[0.0, 0.0], [100.0, 50.0]]));
            assert_eq!(encoded[0]["gradient"]["colors"][0]["color"], "#0000ff");
        }
    }

    fn param(effect_name: &str, name: &str) -> &'static ParamDesc {
        effect(effect_name).params.iter().find(|p| p.name == name).unwrap()
    }

    #[test]
    fn schemas_carry_native_ranges_units_and_defaults() {
        let speed = param_schema(param("Bars", "Speed"));
        assert_eq!((speed["minimum"].clone(), speed["maximum"].clone()), (json!(0), json!(20)));
        assert_eq!(speed["default"], json!(1));
        let level = param_schema(param("Twinkle", "MaximumLevel"));
        assert_eq!((level["minimum"].clone(), level["maximum"].clone()), (json!(0.0), json!(1.0)));
        assert_eq!(level["unit"], "fraction");
        assert_eq!(level["default"], json!(1.0));
        // Editors without a range report the limit validation enforces, not the full int32 range.
        let pulse = param_schema(param("Wipe", "PulseTime"));
        assert_eq!(pulse["maximum"], json!(NUMERIC_SAFETY_LIMIT as i64));
        // Gain is stored as a tenth of what its 0..200 slider shows.
        let gain = param("Fireworks", "Gain");
        assert_eq!(range(gain), Some((0.0, 20.0)));
        assert!(param_schema(gain)["description"].as_str().unwrap().contains("multiplied by 10"));
        for descriptor in vixen_effect_registry::all() {
            for p in descriptor.params {
                let s = param_schema(p);
                assert_ne!(s["maximum"], json!(i32::MAX), "{}.{}", descriptor.name, p.name);
                if let (Some(default), Some(_)) = (s.get("default"), s.get("not")) {
                    panic!("{}.{} has default {default} but no writable form", descriptor.name, p.name);
                }
            }
        }
    }

    #[test]
    fn color_shapes_are_named_and_mismatches_explained() {
        assert_eq!(param_schema(param("Spiral", "Colors"))["title"], "gradientList");
        assert_eq!(param_schema(param("Spiral", "Colors"))["items"]["title"], "gradient");
        assert_eq!(param_schema(param("Spiral", "Colors"))["default"], json!([["#ff0000"], ["#00ff00"], ["#0000ff"]]));
        let gradient_err = decode(ParamKind::Gradient, &json!([["#ff0000"]])).unwrap_err();
        assert!(gradient_err.contains("gradientList"), "{gradient_err}");
        let list_err = decode(ParamKind::GradientList, &json!(["#ff0000"])).unwrap_err();
        assert!(list_err.contains("list of gradients"), "{list_err}");
    }

    #[test]
    fn multi_color_gradients_get_evenly_spaced_stops() {
        let ParamValue::Gradient(g) = decode(ParamKind::Gradient, &json!(["#ffffff", "#808080", "#00b4ff"])).unwrap() else {
            panic!()
        };
        let positions = g.colors().iter().map(|c| c.position).collect::<Vec<_>>();
        assert_eq!(positions, vec![0.0, 0.5, 1.0]);
        assert_eq!(g.get_color_at(0.0), Color::from_argb_u32(0xffffffff));
        assert_eq!(g.get_color_at(1.0), Color::from_argb_u32(0xff00b4ff));
        let ParamValue::Gradient(two) = decode(ParamKind::Gradient, &json!(["#ff0000", "#00ff00"])).unwrap() else {
            panic!()
        };
        let middle = two.get_color_at(0.5);
        assert!(middle.r > 0 && middle.g > 0 && middle.b == 0, "red to green blends, got {middle:?}");
        let encoded = encode(&ParamValue::Gradient(g.clone()));
        assert_eq!(encoded["colors"][2]["position"], json!(1.0));
        assert_eq!(writable(&ParamValue::Gradient(g)), Some(json!(["#ffffff", "#808080", "#00b4ff"])));
    }

    #[test]
    fn gradient_levels_reject_bad_input() {
        let kind = ParamKind::GradientLevels;
        assert!(decode(kind, &json!([])).is_err());
        assert!(decode(kind, &json!(["#ff0000"])).is_err());
        assert!(decode(kind, &json!([{"curve":[[0,100],[100,100]]}])).is_err());
        assert!(decode(kind, &json!([{"gradient":["#ff0000"],"extra":1}])).is_err());
        assert!(decode(kind, &json!([{"gradient":["#ff0000"],"curve":[[0,100]]}])).is_err());
    }

    #[test]
    fn grouping_parameters_are_explained() {
        for name in ["Alternating", "Dissolve"] {
            for param in ["GroupLevel", "DepthOfEffect", "EnableDepth"] {
                assert!(effect(name).params.iter().any(|p| p.name == param));
                assert!(note(name, param).is_some(), "{name}.{param}");
            }
        }
        assert!(note("Chase", "DepthOfEffect").is_none());
    }

    #[test]
    fn motion_parameters_are_explained() {
        for (name, params) in [
            ("Bars", &["MovementType", "Speed", "SpeedCurve"][..]),
            ("Wipe", &["WipeMovement", "MovementCurve", "ReverseDirection", "WipeOn", "ReverseColorDirection"][..]),
        ] {
            for param in params {
                assert!(effect(name).params.iter().any(|p| p.name == *param), "{name}.{param} missing");
                assert!(note(name, param).is_some(), "{name}.{param}");
            }
        }
        assert!(note("Bars", "SpeedCurve").unwrap().contains("50 is still"));
    }
}
