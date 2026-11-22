//! JSON-RPC 2.0 framing: one UTF-8 message per line, at most [`MAX_LINE`] bytes.
use serde_json::{json, Value};
use std::io::{BufRead, ErrorKind};

pub const MAX_LINE: usize = 1024 * 1024;

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;
pub const TIMED_OUT: i64 = -32001;
pub const PLUGIN_EXITED: i64 = -32002;
pub const PERMISSION_DENIED: i64 = -32003;

#[derive(Clone, Debug, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    pub data: Option<Value>,
}

impl RpcError {
    pub fn new(code: i64, message: impl Into<String>) -> RpcError {
        RpcError { code, message: message.into(), data: None }
    }

    pub fn to_json(&self) -> Value {
        let mut error = json!({"code": self.code, "message": self.message});
        if let Some(data) = &self.data {
            error["data"] = data.clone();
        }
        error
    }

    fn from_json(value: &Value) -> RpcError {
        RpcError {
            code: value.get("code").and_then(Value::as_i64).unwrap_or(INTERNAL_ERROR),
            message: value.get("message").and_then(Value::as_str).unwrap_or("error without a message").chars().take(2000).collect(),
            data: value.get("data").cloned(),
        }
    }
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.message, self.code)
    }
}

/// One message read from a plugin.
#[derive(Debug, PartialEq)]
pub enum Incoming {
    Request { id: Value, method: String, params: Value },
    Notification { method: String, params: Value },
    Response { id: u64, result: Result<Value, RpcError> },
}

/// Classifies a line. Anything that isn't a JSON-RPC 2.0 message is an error: the host treats it as a broken plugin.
pub fn parse(line: &[u8]) -> Result<Incoming, String> {
    let value: Value = serde_json::from_slice(line).map_err(|e| format!("not JSON ({e})"))?;
    let object = value.as_object().ok_or("not a JSON object")?;
    if object.get("jsonrpc") != Some(&json!("2.0")) {
        return Err("missing \"jsonrpc\": \"2.0\"".into());
    }
    let params = object.get("params").cloned().unwrap_or(Value::Null);
    match (object.get("method"), object.get("id")) {
        (Some(Value::String(method)), Some(id)) if id.is_string() || id.is_number() => {
            Ok(Incoming::Request { id: id.clone(), method: method.clone(), params })
        }
        (Some(Value::String(method)), None) => Ok(Incoming::Notification { method: method.clone(), params }),
        (Some(_), _) => Err("\"method\" must be a string and \"id\" a string or number".into()),
        (None, Some(id)) => {
            let id = id.as_u64().ok_or("a response's \"id\" must be one the host sent")?;
            match (object.get("result"), object.get("error")) {
                (Some(result), None) => Ok(Incoming::Response { id, result: Ok(result.clone()) }),
                (None, Some(error)) => Ok(Incoming::Response { id, result: Err(RpcError::from_json(error)) }),
                _ => Err("a response needs exactly one of \"result\" and \"error\"".into()),
            }
        }
        (None, None) => Err("neither a request, a notification nor a response".into()),
    }
}

pub fn request(id: u64, method: &str, params: Value) -> Vec<u8> {
    line(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
}

pub fn notification(method: &str, params: Value) -> Vec<u8> {
    line(&json!({"jsonrpc": "2.0", "method": method, "params": params}))
}

pub fn response(id: &Value, result: Result<Value, RpcError>) -> Vec<u8> {
    match result {
        Ok(result) => line(&json!({"jsonrpc": "2.0", "id": id, "result": result})),
        Err(error) => line(&json!({"jsonrpc": "2.0", "id": id, "error": error.to_json()})),
    }
}

fn line(value: &Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(value).expect("JSON values always serialize");
    bytes.push(b'\n');
    bytes
}

/// What [`read_line`] found.
#[derive(Debug, PartialEq, Eq)]
pub enum Line {
    Message(Vec<u8>),
    /// More than `limit` bytes without a newline; the rest of that line has been skipped.
    TooLong,
    Eof,
}

/// Reads one line without ever buffering more than `limit` bytes of it. Blank lines are skipped.
pub fn read_line(reader: &mut impl BufRead, limit: usize) -> std::io::Result<Line> {
    let mut line = Vec::new();
    let mut overflow = false;
    loop {
        let available = match reader.fill_buf() {
            Ok(available) => available,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if available.is_empty() {
            return Ok(if overflow { Line::TooLong } else if line.is_empty() { Line::Eof } else { Line::Message(line) });
        }
        let (chunk, done) = match available.iter().position(|b| *b == b'\n') {
            Some(at) => (&available[..at], at + 1),
            None => (available, available.len()),
        };
        let found_newline = done > chunk.len();
        if !overflow {
            if line.len() + chunk.len() > limit {
                overflow = true;
                line.clear();
            } else {
                line.extend_from_slice(chunk);
            }
        }
        reader.consume(done);
        if found_newline {
            if overflow {
                return Ok(Line::TooLong);
            }
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if line.iter().all(u8::is_ascii_whitespace) {
                line.clear();
                continue;
            }
            return Ok(Line::Message(line));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn messages_are_classified() {
        assert_eq!(
            parse(br#"{"jsonrpc":"2.0","id":"a","method":"storage.get","params":{"key":"k"}}"#).unwrap(),
            Incoming::Request { id: json!("a"), method: "storage.get".into(), params: json!({"key": "k"}) }
        );
        assert_eq!(parse(br#"{"jsonrpc":"2.0","method":"log"}"#).unwrap(), Incoming::Notification { method: "log".into(), params: Value::Null });
        assert_eq!(parse(br#"{"jsonrpc":"2.0","id":7,"result":{}}"#).unwrap(), Incoming::Response { id: 7, result: Ok(json!({})) });
        let Incoming::Response { result: Err(error), .. } = parse(br#"{"jsonrpc":"2.0","id":7,"error":{"code":-32602,"message":"bad"}}"#).unwrap() else {
            panic!()
        };
        assert_eq!((error.code, error.message.as_str()), (INVALID_PARAMS, "bad"));
        for bad in [&b"hello"[..], br#"[]"#, br#"{"id":1,"result":1}"#, br#"{"jsonrpc":"2.0","id":1}"#, br#"{"jsonrpc":"2.0","id":"x","result":1}"#] {
            assert!(parse(bad).is_err(), "{}", String::from_utf8_lossy(bad));
        }
    }

    #[test]
    fn encoded_messages_are_single_lines() {
        let bytes = request(3, "initialize", json!({"text": "a\nb"}));
        assert_eq!(bytes.iter().filter(|b| **b == b'\n').count(), 1);
        assert_eq!(parse(&bytes[..bytes.len() - 1]).unwrap(), Incoming::Request { id: json!(3), method: "initialize".into(), params: json!({"text": "a\nb"}) });
    }

    #[test]
    fn lines_are_bounded_and_skip_blanks() {
        let mut input = Cursor::new(b"\n  \r\n{\"a\":1}\r\nxxxxxxxxxxxx\nend".to_vec());
        assert_eq!(read_line(&mut input, 8).unwrap(), Line::Message(b"{\"a\":1}".to_vec()));
        assert_eq!(read_line(&mut input, 8).unwrap(), Line::TooLong);
        assert_eq!(read_line(&mut input, 8).unwrap(), Line::Message(b"end".to_vec()));
        assert_eq!(read_line(&mut input, 8).unwrap(), Line::Eof);
    }

    #[test]
    fn the_protocol_schema_lists_every_method_the_host_handles() {
        let schema: Value = serde_json::from_str(include_str!("../../../studio/plugins/schema/protocol.schema.json")).unwrap();
        assert_eq!(schema["x-apiVersion"], json!(crate::API_VERSION.to_string()));
        assert_eq!(schema["x-framing"]["maxLineBytes"], json!(MAX_LINE));
        for method in ["initialize", "shutdown", "event"] {
            assert_eq!(schema["x-hostToPlugin"][method]["available"], json!("P1"), "{method}");
        }
        for method in ["controller.create", "controller.configure", "controller.setOutputCount", "controller.start", "controller.stop", "controller.pause", "controller.resume", "controller.destroy", "controller.frame"] {
            assert_eq!(schema["x-hostToPlugin"][method]["available"], json!("P2"), "{method}");
        }
        for method in crate::process::HOST_METHODS {
            let stage = match method {
                "tools.changed" => "P3",
                "display.snapshot" => "P2b",
                m if crate::process::CONTROLLER_METHODS.contains(&m) => "P2",
                _ => "P1",
            };
            assert_eq!(schema["x-pluginToHost"][method]["available"], json!(stage), "{method}");
        }
        assert_eq!(schema["x-hostToPlugin"]["tool.call"]["available"], json!("P3"));
    }
}
