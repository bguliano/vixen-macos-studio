//! Layer mixing filters for MCP (#66): `layer_create`'s `filter` and `settings`, `layer_set_filter`, and the filter `sequence_layers`
//! reports. The same filters and defaults as the Layers panel's Type combo and its Chroma Key, Luma Key and Mask And Fill sheets.
use crate::dialogs::layers::chroma_key_from_dialog;
use crate::sequencer::layers::filter_choices;
use serde_json::{json, Map, Value};
use vixen_core::layer::{ChromaKeySettings, MixingFilter};
use vixen_core::Color;

/// The `filter` names, in the order the schema lists them.
pub const NAMES: [&str; 9] =
    ["highestValue", "intensityOverlay", "mask", "maskFill", "lumaKey", "chromaKey", "proportionalMix", "multiplyColor", "colorChange"];

pub fn name(filter: &MixingFilter) -> &'static str {
    match filter {
        MixingFilter::HighestValue => "highestValue",
        MixingFilter::IntensityOverlay => "intensityOverlay",
        MixingFilter::Mask => "mask",
        MixingFilter::MaskFill { .. } => "maskFill",
        MixingFilter::LumaKey { .. } => "lumaKey",
        MixingFilter::ChromaKey(_) => "chromaKey",
        MixingFilter::ProportionalMix => "proportionalMix",
        MixingFilter::MultiplyColor => "multiplyColor",
        MixingFilter::ColorChange => "colorChange",
    }
}

/// What `sequence_layers` reports as `settings`: the configurable values, empty for a filter with none.
pub fn settings_json(filter: &MixingFilter) -> Value {
    match filter {
        MixingFilter::LumaKey { lower_limit, upper_limit } => json!({"lowerLimit":lower_limit,"upperLimit":upper_limit}),
        MixingFilter::ChromaKey(c) => json!({
            "lowerLimit":c.lower_limit,"upperLimit":c.upper_limit,"keyColor":format!("#{:06X}", c.key_color.to_argb_u32() & 0x00FF_FFFF),
            "hueTolerance":c.hue_tolerance,"saturationTolerance":c.saturation_tolerance,"transparentOnZeroBrightness":c.transparent_on_zero_brightness
        }),
        MixingFilter::MaskFill { exclude_zero_values, requires_mixing_partner } => {
            json!({"excludeZeroValues":exclude_zero_values,"requiresMixingPartner":requires_mixing_partner})
        }
        _ => json!({}),
    }
}

/// The settings each filter takes.
fn keys(filter: &MixingFilter) -> &'static [&'static str] {
    match filter {
        MixingFilter::LumaKey { .. } => &["lowerLimit", "upperLimit"],
        MixingFilter::ChromaKey(_) => &["lowerLimit", "upperLimit", "keyColor", "hueTolerance", "saturationTolerance", "transparentOnZeroBrightness"],
        MixingFilter::MaskFill { .. } => &["excludeZeroValues", "requiresMixingPartner"],
        _ => &[],
    }
}

fn number(settings: &Map<String, Value>, key: &str, old: f64, max: f64) -> Result<f64, String> {
    match settings.get(key) {
        None => Ok(old),
        Some(v) => v.as_f64().filter(|n| (0.0..=max).contains(n)).ok_or(format!("settings.{key} must be a number in 0..{max}")),
    }
}

fn flag(settings: &Map<String, Value>, key: &str, old: bool) -> Result<bool, String> {
    match settings.get(key) {
        None => Ok(old),
        Some(v) => v.as_bool().ok_or(format!("settings.{key} must be true or false")),
    }
}

fn limits(settings: &Map<String, Value>, lower: f64, upper: f64) -> Result<(f64, f64), String> {
    let (lower, upper) = (number(settings, "lowerLimit", lower, 1.0)?, number(settings, "upperLimit", upper, 1.0)?);
    if lower > upper {
        return Err("settings.lowerLimit must not be above settings.upperLimit".into());
    }
    Ok((lower, upper))
}

fn key_color(settings: &Map<String, Value>, old: Color) -> Result<Color, String> {
    let Some(text) = settings.get("keyColor") else { return Ok(old) };
    let digits = text.as_str().and_then(|t| t.strip_prefix('#')).filter(|d| d.len() == 6).ok_or("settings.keyColor must be #RRGGBB")?;
    let rgb = u32::from_str_radix(digits, 16).map_err(|_| "settings.keyColor must be #RRGGBB")?;
    Ok(Color::from_argb_u32(0xFF00_0000 | rgb))
}

/// The filter `a` asks for: `filter` by name (default: `current`'s kind, else highest value) and `settings` laid over the current
/// filter's values when the kind stays, otherwise over the Type combo's defaults. A setting the filter doesn't take is an error.
pub fn parse(a: &Value, current: Option<&MixingFilter>) -> Result<MixingFilter, String> {
    let base = match a.get("filter") {
        None => current.cloned().unwrap_or(MixingFilter::HighestValue),
        Some(v) => {
            let wanted = v.as_str().ok_or("filter must be a string")?;
            if !NAMES.contains(&wanted) {
                return Err(format!("filter must be one of {}", NAMES.join(", ")));
            }
            match current {
                Some(c) if name(c) == wanted => c.clone(),
                _ => filter_choices().into_iter().find(|f| name(f) == wanted).expect("every name is a choice"),
            }
        }
    };
    let empty = Map::new();
    let settings = match a.get("settings") {
        None => &empty,
        Some(v) => v.as_object().ok_or("settings must be an object")?,
    };
    if let Some(bad) = settings.keys().find(|k| !keys(&base).contains(&k.as_str())) {
        let allowed = keys(&base);
        return Err(if allowed.is_empty() {
            format!("{} takes no settings (got {bad})", name(&base))
        } else {
            format!("settings.{bad} doesn't apply to {}; it takes {}", name(&base), allowed.join(", "))
        });
    }
    Ok(match base {
        MixingFilter::LumaKey { lower_limit, upper_limit } => {
            let (lower_limit, upper_limit) = limits(settings, lower_limit, upper_limit)?;
            MixingFilter::LumaKey { lower_limit, upper_limit }
        }
        MixingFilter::ChromaKey(old) => {
            let (lower, upper) = limits(settings, old.lower_limit, old.upper_limit)?;
            let key = key_color(settings, old.key_color)?;
            // The Chroma Key sheet's OK derives the key's hue and saturation from the colour; an unchanged colour keeps them.
            let derived = chroma_key_from_dialog(0, 100, key, 0.0, 0, false);
            let same = key == old.key_color;
            MixingFilter::ChromaKey(ChromaKeySettings {
                lower_limit: lower,
                upper_limit: upper,
                key_hue: if same { old.key_hue } else { derived.key_hue },
                key_saturation: if same { old.key_saturation } else { derived.key_saturation },
                key_color: key,
                hue_tolerance: number(settings, "hueTolerance", old.hue_tolerance, 180.0)?,
                saturation_tolerance: number(settings, "saturationTolerance", old.saturation_tolerance, 0.99)?,
                transparent_on_zero_brightness: flag(settings, "transparentOnZeroBrightness", old.transparent_on_zero_brightness)?,
            })
        }
        MixingFilter::MaskFill { exclude_zero_values, requires_mixing_partner } => MixingFilter::MaskFill {
            exclude_zero_values: flag(settings, "excludeZeroValues", exclude_zero_values)?,
            requires_mixing_partner: flag(settings, "requiresMixingPartner", requires_mixing_partner)?,
        },
        other => other,
    })
}

/// The tool schema's `filter` and `settings` properties.
pub fn schema() -> (Value, Value) {
    (
        json!({"type":"string","enum":NAMES,"description":"Mixing filter. intensityOverlay: this layer's brightness shapes the layers below it (e.g. Pulses over a texture). mask: lit pixels here hide the layers below. lumaKey/chromaKey: the layers below show through where this layer's brightness, or colour, is in range. highestValue (a new layer's default): brightest wins."}),
        json!({"type":"object","additionalProperties":false,"description":"Only for lumaKey (lowerLimit, upperLimit: brightness 0..1), chromaKey (those plus keyColor #RRGGBB, hueTolerance degrees 0..180, saturationTolerance 0..0.99, transparentOnZeroBrightness) and maskFill (excludeZeroValues, requiresMixingPartner). Omitted values keep the layer's current ones, or the Layers panel's defaults when the filter changes.","properties":{
            "lowerLimit":{"type":"number","minimum":0,"maximum":1},"upperLimit":{"type":"number","minimum":0,"maximum":1},
            "keyColor":{"type":"string","pattern":"^#[0-9A-Fa-f]{6}$"},"hueTolerance":{"type":"number","minimum":0,"maximum":180},
            "saturationTolerance":{"type":"number","minimum":0,"maximum":0.99},"transparentOnZeroBrightness":{"type":"boolean"},
            "excludeZeroValues":{"type":"boolean"},"requiresMixingPartner":{"type":"boolean"}
        }}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_name_round_trips_and_defaults_match_the_type_combo() {
        for f in filter_choices() {
            assert_eq!(parse(&json!({"filter":name(&f)}), None).unwrap(), f);
        }
        assert_eq!(parse(&json!({}), None).unwrap(), MixingFilter::HighestValue);
        assert!(parse(&json!({"filter":"lighten"}), None).unwrap_err().contains("intensityOverlay"));
    }

    #[test]
    fn settings_apply_merge_and_are_checked() {
        let luma = parse(&json!({"filter":"lumaKey","settings":{"lowerLimit":0.2}}), None).unwrap();
        assert_eq!(luma, MixingFilter::LumaKey { lower_limit: 0.2, upper_limit: 1.0 });
        // Same kind: settings lay over the current values.
        let luma = parse(&json!({"settings":{"upperLimit":0.6}}), Some(&luma)).unwrap();
        assert_eq!(luma, MixingFilter::LumaKey { lower_limit: 0.2, upper_limit: 0.6 });
        assert!(parse(&json!({"settings":{"lowerLimit":0.9}}), Some(&luma)).is_err());
        assert!(parse(&json!({"filter":"intensityOverlay","settings":{"lowerLimit":0.1}}), None).unwrap_err().contains("takes no settings"));
        assert!(parse(&json!({"filter":"lumaKey","settings":{"keyColor":"#FF0000"}}), None).unwrap_err().contains("lowerLimit, upperLimit"));

        let MixingFilter::ChromaKey(c) =
            parse(&json!({"filter":"chromaKey","settings":{"keyColor":"#FF0000","hueTolerance":20}}), None).unwrap()
        else {
            panic!("chroma key")
        };
        assert_eq!((c.key_hue, c.key_saturation, c.hue_tolerance), (0.0, 1.0, 20.0));
        assert_eq!(settings_json(&MixingFilter::ChromaKey(c))["keyColor"], "#FF0000");
        assert!(parse(&json!({"filter":"chromaKey","settings":{"keyColor":"red"}}), None).is_err());
    }
}
