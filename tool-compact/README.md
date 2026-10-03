use std::fs::File;
use std::io::{BufWriter, Write};

use nasiko_tool_compact::{decode_calls, encode_tools, ToolDef};
use serde_json::{json, Value};

fn main() {
    let raw = std::env::var("EVAL_SET").ok();
    let default_eval = json!({
        "schema_version": "compact-tools-eval@v1-sample",
        "purpose": "offline validation",
        "tools": [
            {
                "name": "create_calendar_event",
                "description": "Create an event in the user's calendar.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "title": {"type": "string"},
                        "start": {"type": "string"},
                        "duration_min": {"type": "integer"},
                        "visibility": {"type": "string", "enum": ["public", "private"]},
                        "attendees": {"type": "array", "items": {"type": "string"}}
                    },
                    "required": ["title", "start"]
                }
            },
            {
                "name": "send_email",
                "description": "Send an email.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "to": {"type": "array", "items": {"type": "string"}},
                        "subject": {"type": "string"},
                        "body": {"type": "string"}
                    },
                    "required": ["to", "subject", "body"]
                }
            }
        ],
        "cases": [
            {
                "id": "ct-001",
                "messages": [{"role": "user", "content": "Book a design review Monday 3pm IST with riya@example.com"}],
                "expected": [{
                    "name": "create_calendar_event",
                    "arguments": {
                        "title": "Design review",
                        "start": "2026-10-05T15:00:00+05:30",
                        "attendees": ["riya@example.com"]
                    }
                }]
            },
            {
                "id": "ct-002",
                "messages": [{"role": "user", "content": "Email sam@example.com that the build is green."}],
                "expected": [{
                    "name": "send_email",
                    "arguments": {
                        "to": ["sam@example.com"],
                        "subject": "Build status",
                        "body": "The build is green."
                    }
                }]
            },
            {
                "id": "ct-003",
                "messages": [{"role": "user", "content": "What's the weather?"}],
                "expected": []
            }
        ]
    });

    let root: Value = match raw {
        Some(path) => {
            let raw_text = std::fs::read_to_string(path).expect("read EVAL_SET");
            serde_json::from_str(&raw_text).expect("valid EVAL_SET JSON")
        }
        None => default_eval,
    };

    let tools: Vec<ToolDef> = root["tools"]
        .as_array()
        .unwrap_or(&Vec::new())
        .iter()
        .map(|tool| ToolDef {
            name: tool["name"].as_str().unwrap_or_default().to_string(),
            description: tool["description"].as_str().map(str::to_string),
            parameters: tool.get("parameters").cloned(),
        })
        .collect();

    let cases = root["cases"].as_array().unwrap_or(&Vec::new());
    let out_path = std::env::var("OUT").unwrap_or_else(|_| "compact-tools-out.jsonl".to_string());
    let file = File::create(&out_path).expect("create OUT");
    let mut writer = BufWriter::new(file);

    let compact = encode_tools(&tools).unwrap_or_else(|err| {
        panic!("encode_tools failed: {err}");
    });

    for case in cases {
        let id = case["id"].as_str().unwrap_or("unknown");
        let expected = case["expected"].as_array().unwrap_or(&Vec::new());
        let rendered = expected.iter().map(|item| {
            let name = item["name"].as_str().unwrap_or_default();
            let args = item["arguments"].as_object().unwrap_or(&serde_json::Map::new());
            format!("<<call {name} {}>>", serde_json::to_string(args).unwrap())
        }).collect::<Vec<_>>().join("");

        let roundtrip = decode_calls(&rendered, &tools).unwrap_or_else(|err| {
            panic!("decode_calls failed for {id}: {err}");
        });

        let line = json!({
            "id": id,
            "compact_request": {
                "messages": case.get("messages").cloned().unwrap_or(Value::Null),
                "tools": compact.tools,
                "instructions": compact.instructions,
            },
            "compacted": true,
            "rendered_calls": rendered,
            "roundtrip_calls": roundtrip.iter().map(|call| json!({
                "name": call.name,
                "arguments": call.arguments,
            })).collect::<Vec<_>>(),
            "decoded": {"calls": []}
        });

        writeln!(writer, "{line}").expect("write OUT");
    }

    writer.flush().expect("flush OUT");
}

