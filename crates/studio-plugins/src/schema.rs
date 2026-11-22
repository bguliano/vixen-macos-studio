//! Checks a controller configuration against the plugin's `configSchema`: the JSON Schema subset the inspector form
//! renders (object properties, required, additionalProperties, string/number/integer/boolean/array/object/null types,
//! enum, const, minimum/maximum, minLength/maxLength, pattern-free formats `ipv4` and `hostname`, items, minItems and
//! maxItems). Keywords outside the subset are ignored, so the plugin's own `controller.configure` stays the final word.
use serde_json::{json, Map, Value};

/// One problem: the JSON Pointer of the field and what is wrong with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldError {
    pub field: String,
    pub message: String,
}

impl FieldError {
    pub fn to_json(&self) -> Value {
        json!({"field": self.field, "message": self.message})
    }
}

/// Every problem with `value`, at most 50.
pub fn validate(schema: &Value, value: &Value) -> Vec<FieldError> {
    let mut errors = Vec::new();
    check(schema, value, "", &mut errors);
    errors.truncate(50);
    errors
}

/// The configuration a new instance starts with: each property's `default`, objects filled in recursively.
pub fn defaults(schema: &Value) -> Value {
    if let Some(default) = schema.get("default") {
        return default.clone();
    }
    match schema.get("properties").and_then(Value::as_object) {
        Some(properties) => {
            let mut out = Map::new();
            for (name, property) in properties {
                let value = defaults(property);
                if !value.is_null() {
                    out.insert(name.clone(), value);
                }
            }
            Value::Object(out)
        }
        None if type_names(schema).contains(&"object") => json!({}),
        None => Value::Null,
    }
}

fn type_names(schema: &Value) -> Vec<&str> {
    match schema.get("type") {
        Some(Value::String(t)) => vec![t.as_str()],
        Some(Value::Array(list)) => list.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    }
}

fn type_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(n) if n.is_i64() || n.is_u64() || n.as_f64().is_some_and(|f| f.fract() == 0.0) => "integer",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn check(schema: &Value, value: &Value, at: &str, errors: &mut Vec<FieldError>) {
    let mut push = |message: String| errors.push(FieldError { field: if at.is_empty() { "/".into() } else { at.into() }, message });
    let types = type_names(schema);
    let actual = type_of(value);
    if !types.is_empty() && !types.iter().any(|t| *t == actual || (*t == "number" && actual == "integer")) {
        push(format!("must be {}", types.join(" or ")));
        return;
    }
    if let Some(allowed) = schema.get("enum").and_then(Value::as_array) {
        if !allowed.contains(value) {
            let list: Vec<String> = allowed.iter().map(Value::to_string).collect();
            push(format!("must be one of {}", list.join(", ")));
        }
    }
    if let Some(constant) = schema.get("const") {
        if constant != value {
            push(format!("must be {constant}"));
        }
    }
    match value {
        Value::Number(n) => {
            let n = n.as_f64().unwrap_or(0.0);
            if let Some(min) = schema.get("minimum").and_then(Value::as_f64).filter(|m| n < *m) {
                push(format!("must be at least {min}"));
            }
            if let Some(max) = schema.get("maximum").and_then(Value::as_f64).filter(|m| n > *m) {
                push(format!("must be at most {max}"));
            }
        }
        Value::String(text) => {
            let chars = text.chars().count() as u64;
            if let Some(min) = schema.get("minLength").and_then(Value::as_u64).filter(|m| chars < *m) {
                push(format!("must be at least {min} characters"));
            }
            if let Some(max) = schema.get("maxLength").and_then(Value::as_u64).filter(|m| chars > *m) {
                push(format!("must be at most {max} characters"));
            }
            match schema.get("format").and_then(Value::as_str) {
                Some("ipv4") if text.parse::<std::net::Ipv4Addr>().is_err() => push("must be an IPv4 address such as 192.168.1.50".into()),
                Some("hostname") if !valid_hostname(text) => push("must be a host name".into()),
                _ => {}
            }
        }
        Value::Array(items) => {
            if let Some(min) = schema.get("minItems").and_then(Value::as_u64).filter(|m| (items.len() as u64) < *m) {
                push(format!("must have at least {min} items"));
            }
            if let Some(max) = schema.get("maxItems").and_then(Value::as_u64).filter(|m| (items.len() as u64) > *m) {
                push(format!("must have at most {max} items"));
            }
            if let Some(item_schema) = schema.get("items").filter(|s| s.is_object()) {
                for (i, item) in items.iter().enumerate() {
                    check(item_schema, item, &format!("{at}/{i}"), errors);
                }
            }
        }
        Value::Object(map) => {
            let properties = schema.get("properties").and_then(Value::as_object);
            for name in schema.get("required").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
                if !map.contains_key(name) {
                    errors.push(FieldError { field: format!("{at}/{}", escape(name)), message: "is required".into() });
                }
            }
            for (name, item) in map {
                let field = format!("{at}/{}", escape(name));
                match properties.and_then(|p| p.get(name)) {
                    Some(property) => check(property, item, &field, errors),
                    None => match schema.get("additionalProperties") {
                        Some(Value::Bool(false)) => errors.push(FieldError { field, message: "is not a known setting".into() }),
                        Some(extra @ Value::Object(_)) => check(extra, item, &field, errors),
                        _ => {}
                    },
                }
            }
        }
        _ => {}
    }
}

fn escape(name: &str) -> String {
    name.replace('~', "~0").replace('/', "~1")
}

fn valid_hostname(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 253
        && text.split('.').all(|label| {
            !label.is_empty() && label.len() <= 63 && !label.starts_with('-') && !label.ends_with('-') && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema() -> Value {
        json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["host"],
            "properties": {
                "host": {"type": "string", "format": "ipv4"},
                "port": {"type": "integer", "minimum": 1, "maximum": 65535, "default": 4048},
                "order": {"type": "string", "enum": ["rgb", "grb"], "default": "rgb"},
                "fast": {"type": "boolean"},
                "devices": {"type": "array", "maxItems": 2, "items": {"type": "object", "required": ["mac"], "properties": {"mac": {"type": "string", "minLength": 12}}}}
            }
        })
    }

    #[test]
    fn a_valid_configuration_passes() {
        assert!(validate(&schema(), &json!({"host": "10.0.0.2", "port": 80, "order": "grb", "devices": [{"mac": "aabbccddeeff"}]})).is_empty());
    }

    #[test]
    fn each_problem_names_its_field() {
        let errors = validate(&schema(), &json!({"port": 0, "order": "bgr", "fast": "yes", "colour": 1, "devices": [{"mac": "x"}, {}, {}]}));
        let fields: Vec<(&str, &str)> = errors.iter().map(|e| (e.field.as_str(), e.message.as_str())).collect();
        assert!(fields.contains(&("/host", "is required")), "{fields:?}");
        assert!(fields.contains(&("/port", "must be at least 1")), "{fields:?}");
        assert!(fields.contains(&("/order", "must be one of \"rgb\", \"grb\"")), "{fields:?}");
        assert!(fields.contains(&("/fast", "must be boolean")), "{fields:?}");
        assert!(fields.contains(&("/colour", "is not a known setting")), "{fields:?}");
        assert!(fields.contains(&("/devices", "must have at most 2 items")), "{fields:?}");
        assert!(fields.contains(&("/devices/0/mac", "must be at least 12 characters")), "{fields:?}");
        assert!(fields.contains(&("/devices/1/mac", "is required")), "{fields:?}");
        assert_eq!(validate(&schema(), &json!({"host": "lamp.local"}))[0].message, "must be an IPv4 address such as 192.168.1.50");
    }

    #[test]
    fn defaults_fill_in_each_property() {
        assert_eq!(defaults(&schema()), json!({"port": 4048, "order": "rgb"}));
        assert_eq!(defaults(&json!({})), Value::Null);
        assert_eq!(defaults(&json!({"type": "object"})), json!({}));
    }
}
