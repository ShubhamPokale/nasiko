use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompactTool {
    pub name: String,
    pub signature: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompactTools {
    pub tools: Vec<CompactTool>,
    pub instructions: String,
}

#[derive(thiserror::Error, Debug)]
pub enum CompactError {
    #[error("invalid tool schema for `{0}`")]
    InvalidSchema(String),
    #[error("unknown tool `{0}`")]
    UnknownTool(String),
    #[error("invalid tool call for `{0}`")]
    InvalidArguments(String),
    #[error("malformed compact tool call")]
    MalformedCall,
    #[error("invalid JSON in compact tool call")]
    InvalidJson,
    #[error("tool definition is incomplete")]
    IncompleteDefinition,
    #[error("{0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, CompactError>;

pub fn encode_tools(tools: &[ToolDef]) -> Result<CompactTools> {
    let compact_tools = tools
        .iter()
        .map(|tool| {
            let signature = render_signature(tool)?;
            Ok(CompactTool {
                name: tool.name.clone(),
                description: tool.description.clone(),
                signature,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(CompactTools {
        tools: compact_tools,
        instructions: "To call a tool, emit: <<call name {json args}>>".to_string(),
    })
}

pub fn decode_tools(compact: &CompactTools) -> Result<Vec<ToolDef>> {
    let mut out = Vec::new();
    for tool in &compact.tools {
        out.push(ToolDef {
            name: tool.name.clone(),
            description: tool.description.clone(),
            parameters: None,
        });
    }
    Ok(out)
}

pub fn decode_calls(text: &str, tools: &[ToolDef]) -> Result<Vec<ToolCall>> {
    let mut out = Vec::new();
    let mut cursor = 0usize;

    while let Some(start) = text[cursor..].find("<<call") {
        let start = cursor + start;
        let after = &text[start + 7..];
        let Some(end) = find_call_end(after) else {
            break;
        };
        let block = &after[..end];
        out.push(parse_and_validate_call(block, tools)?);
        cursor = start + 7 + end + 2;
    }

    Ok(out)
}

pub struct StreamDecoder {
    buffer: String,
}

impl Default for StreamDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamDecoder {
    pub fn new() -> Self {
        Self { buffer: String::new() }
    }

    pub fn push_chunk(&mut self, chunk: &str, tools: &[ToolDef]) -> Result<Vec<ToolCall>> {
        self.buffer.push_str(chunk);
        let mut out = Vec::new();
        let mut cursor = 0usize;

        while let Some(start) = self.buffer[cursor..].find("<<call") {
            let start = cursor + start;
            let after = &self.buffer[start + 7..];
            let Some(end) = find_call_end(after) else {
                break;
            };
            let block = &after[..end];
            out.push(parse_and_validate_call(block, tools)?);
            cursor = start + 7 + end + 2;
        }

        if cursor > 0 {
            self.buffer = self.buffer[cursor..].to_string();
        }
        Ok(out)
    }

    pub fn finish(mut self, tools: &[ToolDef]) -> Result<Vec<ToolCall>> {
        if self.buffer.trim().is_empty() {
            return Ok(Vec::new());
        }
        let calls = decode_calls(self.buffer.as_str(), tools)?;
        Ok(calls)
    }
}

fn render_signature(tool: &ToolDef) -> Result<String> {
    let params = match tool.parameters.as_ref() {
        Some(v) => v,
        None => return Ok(format!("{}()", tool.name)),
    };

    let schema = params.as_object().ok_or(CompactError::InvalidSchema(tool.name.clone()))?;
    let required = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();

    let props = schema.get("properties").and_then(Value::as_object).unwrap_or_default();
    let mut args = Vec::new();
    for (name, value) in props {
        let suffix = if required.contains(name.as_str()) { "" } else { "?" };
        args.push(format!("{name}{suffix}:{}", infer_type_name(value)));
    }

    let description = tool
        .description
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| format!(" - {s}"))
        .unwrap_or_default();

    Ok(format!("{}({}){}", tool.name, args.join(", "), description))
}

fn infer_type_name(value: &Value) -> String {
    if let Some(enum_values) = value.get("enum").and_then(Value::as_array) {
        if !enum_values.is_empty() {
            let parts = enum_values
                .iter()
                .filter_map(|item| match item {
                    Value::String(s) => Some(s.clone()),
                    Value::Number(n) => Some(n.to_string()),
                    Value::Bool(b) => Some(if *b { "true".to_string() } else { "false".to_string() }),
                    _ => None,
                })
                .collect::<Vec<_>>();
            if !parts.is_empty() {
                return parts.join("|");
            }
        }
    }

    if value.get("type").and_then(Value::as_str) == Some("array") {
        let item = value.get("items").unwrap_or(&Value::String("value".into()));
        return format!("[{}]", infer_type_name(item));
    }

    match value.get("type").and_then(Value::as_str) {
        Some("string") => "str".to_string(),
        Some("integer") | Some("int") => "int".to_string(),
        Some("number") => "float".to_string(),
        Some("boolean") => "bool".to_string(),
        Some("object") => "obj".to_string(),
        _ => "value".to_string(),
    }
}

fn parse_and_validate_call(block: &str, tools: &[ToolDef]) -> Result<ToolCall> {
    let trimmed = block.trim();
    let after_call = trimmed
        .strip_prefix("<<call")
        .ok_or(CompactError::MalformedCall)?
        .trim_start();

    let (name, rest) = split_name_and_rest(after_call).ok_or(CompactError::MalformedCall)?;
    let Some(json_start) = rest.find('{') else {
        return Err(CompactError::MalformedCall);
    };
    let raw_json = &rest[json_start..];
    let end = find_json_object_end(raw_json).ok_or(CompactError::MalformedCall)?;

    let value = serde_json::from_str::<Value>(&raw_json[..end])
        .map_err(|_| CompactError::InvalidJson)?;

    let tool = tools
        .iter()
        .find(|candidate| candidate.name == name)
        .ok_or_else(|| CompactError::UnknownTool(name.to_string()))?;

    let schema = tool.parameters.as_ref().ok_or(CompactError::IncompleteDefinition)?;
    validate_against_schema(&value, schema)?;

    Ok(ToolCall {
        name: name.to_string(),
        arguments: value,
    })
}

fn split_name_and_rest(input: &str) -> Option<(&str, &str)> {
    let trimmed = input.trim_start();
    let idx = trimmed
        .chars()
        .position(|ch| ch.is_whitespace())
        .unwrap_or(trimmed.len());
    if idx == 0 {
        return None;
    }
    let name = &trimmed[..idx];
    Some((name, &trimmed[idx..]))
}

fn validate_against_schema(value: &Value, schema: &Value) -> Result<()> {
    let Some(schema_obj) = schema.as_object() else {
        return Err(CompactError::InvalidSchema("schema".to_string()));
    };

    if let Some(enum_values) = schema_obj.get("enum").and_then(Value::as_array) {
        if !enum_values.iter().any(|candidate| candidate == value) {
            return Err(CompactError::InvalidArguments("enum".to_string()));
        }
    }

    if let Some(expected_type) = schema_obj.get("type").and_then(Value::as_str) {
        match expected_type {
            "string" => {
                if !value.is_string() {
                    return Err(CompactError::InvalidArguments("string".to_string()));
                }
            }
            "integer" => {
                if !value.is_i64() && !value.is_u64() {
                    return Err(CompactError::InvalidArguments("integer".to_string()));
                }
            }
            "number" => {
                if !value.is_number() {
                    return Err(CompactError::InvalidArguments("number".to_string()));
                }
            }
            "boolean" => {
                if !value.is_boolean() {
                    return Err(CompactError::InvalidArguments("boolean".to_string()));
                }
            }
            "array" => {
                let array = value.as_array().ok_or_else(|| CompactError::InvalidArguments("array".to_string()))?;
                if let Some(item_schema) = schema_obj.get("items") {
                    for item in array {
                        validate_against_schema(item, item_schema)?;
                    }
                }
            }
            "object" => {
                let obj = value.as_object().ok_or_else(|| CompactError::InvalidArguments("object".to_string()))?;
                let properties = schema_obj.get("properties").and_then(Value::as_object);
                if let Some(props) = properties {
                    let required = schema_obj
                        .get("required")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .collect::<HashSet<_>>();

                    for (key, prop_schema) in props {
                        if let Some(prop_value) = obj.get(key) {
                            validate_against_schema(prop_value, prop_schema)?;
                        } else if required.contains(key.as_str()) {
                            return Err(CompactError::InvalidArguments(key.clone()));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    Ok(())
}

fn find_call_end(input: &str) -> Option<usize> {
    let bytes = input.as_bytes();
    let mut in_string = false;
    let mut escaped = false;

    for i in 0..bytes.len() {
        let byte = bytes[i];
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }

        if byte == b'"' {
            in_string = true;
            continue;
        }

        if byte == b'>' && i + 1 < bytes.len() && bytes[i + 1] == b'>' {
            return Some(i);
        }
    }

    None
}

fn find_json_object_end(input: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (idx, byte) in input.as_bytes().iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                in_string = false;
            }
            continue;
        }

        match *byte {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(idx + 1);
                }
            }
            _ => {}
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_tools() -> Vec<ToolDef> {
        vec![
            ToolDef {
                name: "create_calendar_event".to_string(),
                description: Some("Create an event in the user's calendar.".to_string()),
                parameters: Some(json!({
                    "type": "object",
                    "properties": {
                        "title": {"type": "string"},
                        "start": {"type": "string"},
                        "duration_min": {"type": "integer"},
                        "visibility": {"type": "string", "enum": ["public", "private"]},
                        "attendees": {"type": "array", "items": {"type": "string"}}
                    },
                    "required": ["title", "start"]
                })),
            },
            ToolDef {
                name: "send_email".to_string(),
                description: Some("Send an email.".to_string()),
                parameters: Some(json!({
                    "type": "object",
                    "properties": {
                        "to": {"type": "array", "items": {"type": "string"}},
                        "subject": {"type": "string"},
                        "body": {"type": "string"}
                    },
                    "required": ["to", "subject", "body"]
                })),
            },
        ]
    }

    #[test]
    fn encode_and_decode_round_trip() {
        let tools = sample_tools();
        let compact = encode_tools(&tools).unwrap();
        assert_eq!(compact.tools[0].name, "create_calendar_event");
        assert!(compact.instructions.contains("<<call"));

        let text = "<<call create_calendar_event {\"title\":\"Design review\",\"start\":\"2026-10-05T15:00:00+05:30\",\"attendees\":[\"riya@example.com\"]}>>";
        let calls = decode_calls(text, &tools).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "create_calendar_event");
        assert_eq!(calls[0].arguments["attendees"][0], "riya@example.com");
    }

    #[test]
    fn fail_closed_on_unknown_and_invalid_calls() {
        let tools = sample_tools();

        let unknown = decode_calls("<<call unknown_tool {\"title\":\"x\"}>>", &tools).unwrap_err();
        assert!(matches!(unknown, CompactError::UnknownTool(_)));

        let missing = decode_calls("<<call create_calendar_event {\"title\":\"x\"}>>", &tools).unwrap_err();
        assert!(matches!(missing, CompactError::InvalidArguments(_)));

        let enum_err = decode_calls(
            "<<call create_calendar_event {\"title\":\"Design review\",\"start\":\"2026-10-05T15:00:00+05:30\",\"visibility\":\"secret\"}>>",
            &tools,
        )
        .unwrap_err();
        assert!(matches!(enum_err, CompactError::InvalidArguments(_)));
    }

    #[test]
    fn stream_decoder_handles_split_markers() {
        let tools = sample_tools();
        let mut stream = StreamDecoder::new();
        let no_call = stream.push_chunk("<<ca", &tools).unwrap();
        assert!(no_call.is_empty());

        let chunked = stream.push_chunk("ll create_calendar_event {\"title\":\"Retro\",\"start\":\"2026-10-04T10:00:00+05:30\"}>>", &tools).unwrap();
        assert_eq!(chunked.len(), 1);
        assert_eq!(chunked[0].name, "create_calendar_event");
    }

    #[test]
    fn plain_text_around_calls_is_ignored() {
        let tools = sample_tools();
        let text = "Before <<call send_email {\"to\":[\"sam@example.com\"],\"subject\":\"Build status\",\"body\":\"The build is green.\"}>> after";
        let calls = decode_calls(text, &tools).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "send_email");
    }
}
