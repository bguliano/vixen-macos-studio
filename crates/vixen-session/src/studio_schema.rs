//! Structural validation for the closed tool schemas we publish. Native setters validate domain values.
use serde_json::Value;
pub fn arguments(schema: &Value, value: &Value) -> Result<(), String> {
    validate(schema, value, "arguments", 0)
}
fn validate(schema: &Value, value: &Value, path: &str, depth: usize) -> Result<(), String> {
    if depth > 32 {
        return Err("Argument nesting limit exceeded".into());
    }
    if let Some(kind) = schema["type"].as_str() {
        let valid = match kind {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "boolean" => value.is_boolean(),
            "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
            "number" => value.as_f64().is_some_and(f64::is_finite),
            _ => false,
        };
        if !valid {
            return Err(format!("{path} must be {kind}"));
        }
    }
    if let Some(choices) = schema["enum"].as_array() {
        if !choices.contains(value) {
            return Err(format!("{path} is not an allowed value"));
        }
    }
    if let Some(object) = value.as_object() {
        if let Some(required) = schema["required"].as_array() {
            for key in required.iter().filter_map(Value::as_str) {
                if !object.contains_key(key) {
                    return Err(format!("{path}.{key} is required"));
                }
            }
        }
        for (key, value) in object {
            if let Some(child) = schema["properties"].get(key) {
                validate(child, value, &format!("{path}.{key}"), depth + 1)?;
            } else if schema["additionalProperties"] == false {
                return Err(format!("Unknown argument {path}.{key}"));
            }
        }
    }
    if let Some(array) = value.as_array() {
        if schema["minItems"]
            .as_u64()
            .is_some_and(|n| array.len() < (n as usize))
            || schema["maxItems"]
                .as_u64()
                .is_some_and(|n| array.len() > (n as usize))
        {
            return Err(format!("{path} array length is out of range"));
        }
        if let Some(child) = schema.get("items") {
            for (index, item) in array.iter().enumerate() {
                validate(child, item, &format!("{path}[{index}]"), depth + 1)?;
            }
        }
    }
    if let Some(text) = value.as_str() {
        let n = text.chars().count();
        if schema["minLength"]
            .as_u64()
            .is_some_and(|m| n < (m as usize))
            || schema["maxLength"]
                .as_u64()
                .is_some_and(|m| n > (m as usize))
        {
            return Err(format!("{path} string length is out of range"));
        }
    }
    if let Some(n) = value.as_f64() {
        if schema["minimum"].as_f64().is_some_and(|m| n < m)
            || schema["maximum"].as_f64().is_some_and(|m| n > m)
        {
            return Err(format!("{path} number is out of range"));
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn rejects_missing_wrong_type_unknown_and_out_of_range() {
        let schema = json!({"type":"object","properties":{"count":{"type":"integer","minimum":1,"maximum":4}},"required":["count"],"additionalProperties":false});
        for bad in [
            json!({}),
            json!({"count":"1"}),
            json!({"count":0}),
            json!({"count":2,"surprise":true}),
        ] {
            assert!(arguments(&schema, &bad).is_err());
        }
        assert!(arguments(&schema, &json!({"count":2})).is_ok());
    }
}
