//! MCP stdio adapter. Tools map 1:1 onto Command / Query.

use std::io::{self, BufRead, Write};

use dochwp_api::{Command, Engine, ExportTarget, Query};
use serde_json::{json, Value};

fn main() {
    let mut engine = Engine::new();
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(req) => handle(&mut engine, req),
            Err(e) => json!({"jsonrpc":"2.0","error":{"code":-32700,"message":e.to_string()}}),
        };
        let _ = writeln!(stdout, "{reply}");
        let _ = stdout.flush();
    }
}

fn handle(engine: &mut Engine, req: Value) -> Value {
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
    match method {
        "initialize" => json!({
            "jsonrpc":"2.0","id":id,
            "result":{"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"dochwp","version":env!("CARGO_PKG_VERSION")}}
        }),
        "tools/list" => json!({
            "jsonrpc":"2.0","id":id,
            "result":{"tools":[
                {"name":"convert","description":"Command::Convert","inputSchema":{"type":"object","properties":{"input_b64":{"type":"string"},"targets":{"type":"array","items":{"type":"string"}}},"required":["input_b64"]}},
                {"name":"query","description":"Query enum","inputSchema":{"type":"object","properties":{"kind":{"type":"string"}}}}
            ]}
        }),
        "tools/call" => {
            let name = req
                .pointer("/params/name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let args = req.pointer("/params/arguments").cloned().unwrap_or(json!({}));
            let result = match name {
                "convert" => {
                    let b64 = args.get("input_b64").and_then(|v| v.as_str()).unwrap_or("");
                    let bytes = decode_b64(b64);
                    match engine.execute(Command::Convert {
                        input: bytes,
                        input_kind: None,
                        targets: vec![ExportTarget::IrJson, ExportTarget::Html],
                    }) {
                        Ok(out) => json!({"content":[{"type":"text","text":serde_json::to_string(&out.capsule).unwrap_or_default()}]}),
                        Err(e) => json!({"isError":true,"content":[{"type":"text","text":e.to_string()}]}),
                    }
                }
                "query" => {
                    let v = engine.query(Query::EngineVersion);
                    json!({"content":[{"type":"text","text":v.to_string()}]})
                }
                _ => json!({"isError":true,"content":[{"type":"text","text":"unknown tool"}]}),
            };
            json!({"jsonrpc":"2.0","id":id,"result":result})
        }
        _ => json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"method not found"}}),
    }
}

fn decode_b64(s: &str) -> Vec<u8> {
    fn val(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 3 < bytes.len() {
        let a = val(bytes[i]).unwrap_or(0);
        let b = val(bytes[i + 1]).unwrap_or(0);
        let c = val(bytes[i + 2]).unwrap_or(0);
        let d = val(bytes[i + 3]).unwrap_or(0);
        out.push((a << 2) | (b >> 4));
        if bytes[i + 2] != b'=' {
            out.push((b << 4) | (c >> 2));
        }
        if bytes[i + 3] != b'=' {
            out.push((c << 6) | d);
        }
        i += 4;
    }
    out
}
