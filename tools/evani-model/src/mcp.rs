//! A minimal stdio MCP server: JSON-RPC 2.0, one message per line.
//! Tools are registered with underscores (decision D10); replies are compact
//! JSON text, errors carry a fix line, and no vertex data is ever returned.

use evani_model::api;
use serde_json::{Value, json};
use std::io::{BufRead, Write};

fn tools() -> Value {
    let intent = json!({ "type": "string", "description": "the .evmodel text, canonical JSON, a path to an .evmodel file, or a model id from model_make" });
    let tiers = json!({ "type": "array", "items": { "type": "string", "enum": ["low", "medium", "high", "ultra", "vr"] }, "description": "render tiers (default: every tier the intent's tri_budget lists)" });
    json!([
        { "name": "model_describe_system", "description": "This machine's compute class, CPU threads, GPU (if the driver tool reports it) and time budgets.",
          "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false } },
        { "name": "model_make", "description": "Make a model from intent (docs/MODEL_API.md 1): validates every word, builds it from the class template and style, compiles every tier and runs every must test. Returns a scorecard (M1: one candidate, k0).",
          "inputSchema": { "type": "object", "properties": { "intent": intent }, "required": ["intent"], "additionalProperties": false } },
        { "name": "model_compile", "description": "Write one glTF 2.0 .glb per render tier (quantised, with LODs, anchors and collision) and a metadata file. Returns files, triangles, sizes, hash and checks.",
          "inputSchema": { "type": "object", "properties": { "model": intent, "tiers": tiers, "out": { "type": "string", "description": "output folder (default .evani/model/<id>/v1)" } }, "required": ["model"], "additionalProperties": false } },
        { "name": "model_check", "description": "Run every must test and the mesh checks (watertight, manifold, self-intersections, budget) per tier.",
          "inputSchema": { "type": "object", "properties": { "model": intent, "tiers": tiers }, "required": ["model"], "additionalProperties": false } },
        { "name": "model_sheet", "description": "Draw one combined picture of the model (kind blueprint: front, side, top and three-quarter views). Returns the PNG path and a caption.",
          "inputSchema": { "type": "object", "properties": { "model": intent, "kind": { "type": "string", "enum": ["blueprint"], "description": "blueprint (the variants sheet arrives in M3)" } }, "required": ["model"], "additionalProperties": false } },
        { "name": "model_compare", "description": "Measure a model against a target .glb (e.g. a Meshy master): both aligned on the ground, never rescaled; silhouette IoU for front, side, back and three-quarter views, region measures (height, arm span, brim, head, shoulders, coat hem, boot top, depth) and a side-by-side sheet PNG.",
          "inputSchema": { "type": "object", "properties": { "model": { "type": "string", "description": "intent text, .evmodel path, model id, or a .glb of ours" }, "target": { "type": "string", "description": "path of the target .glb" }, "tier": { "type": "string", "description": "tier to compile (default ultra)" }, "size": { "type": "integer", "description": "pixels per view side (default 512)" }, "out": { "type": "string", "description": "sheet PNG path" } }, "required": ["model", "target"], "additionalProperties": false } }
    ])
}

fn call(ws: &api::Workspace, name: &str, a: &Value) -> Result<Value, Value> {
    let s = |k: &str| a.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let tiers: Option<Vec<String>> = a.get("tiers").and_then(Value::as_array).map(|v| v.iter().filter_map(|x| x.as_str().map(String::from)).collect());
    let e = |d: Vec<evani_model::intent::Diag>| api::error_json(&d);
    match name {
        "model_describe_system" => Ok(api::describe_system()),
        "model_make" => api::make(ws, &s("intent")).map_err(e),
        "model_compile" => {
            let out = a.get("out").and_then(Value::as_str).map(std::path::PathBuf::from);
            api::compile_model(ws, &s("model"), tiers.as_deref(), out.as_deref()).map_err(e)
        }
        "model_check" => api::check(ws, &s("model"), tiers.as_deref()).map_err(e),
        "model_sheet" => api::sheet(ws, &s("model"), a.get("kind").and_then(Value::as_str).unwrap_or("blueprint"), None).map_err(e),
        "model_compare" => {
            let out = a.get("out").and_then(Value::as_str).map(std::path::PathBuf::from);
            let tier = a.get("tier").and_then(Value::as_str).unwrap_or("ultra");
            let size = a.get("size").and_then(Value::as_u64).unwrap_or(512) as usize;
            api::compare(ws, &s("model"), std::path::Path::new(&s("target")), tier, size, out.as_deref()).map_err(e)
        }
        other => Err(json!({ "error": "unknown_tool", "msg": format!("no tool `{other}`"), "fix": "call tools/list for the model_* tools" })),
    }
}

/// Handle one JSON-RPC message; `None` for notifications.
pub fn handle(ws: &api::Workspace, msg: &Value) -> Option<Value> {
    let id = msg.get("id").cloned()?;
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    let result = match method {
        "initialize" => {
            let version = msg.pointer("/params/protocolVersion").and_then(Value::as_str).unwrap_or("2025-06-18");
            json!({
                "protocolVersion": version,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": "evani-model", "version": api::TOOL_VERSION },
                "instructions": "Evani.Model: make game-ready models from intent. Call model_make with an .evmodel intent (docs/MODEL_API.md), then model_compile with the returned model id. Replies are compact JSON; errors carry a fix line."
            })
        }
        "ping" => json!({}),
        "tools/list" => json!({ "tools": tools() }),
        "tools/call" => {
            let name = msg.pointer("/params/name").and_then(Value::as_str).unwrap_or("");
            let args = msg.pointer("/params/arguments").cloned().unwrap_or(json!({}));
            match call(ws, name, &args) {
                Ok(v) => json!({ "content": [{ "type": "text", "text": v.to_string() }], "isError": false }),
                Err(v) => json!({ "content": [{ "type": "text", "text": v.to_string() }], "isError": true }),
            }
        }
        _ => return Some(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": format!("method `{method}` not found") } })),
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

pub fn serve(ws: &api::Workspace) {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => handle(ws, &msg),
            Err(e) => Some(json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": format!("parse error: {e}") } })),
        };
        if let Some(r) = reply
            && (writeln!(out, "{r}").is_err() || out.flush().is_err()) {
                break;
            }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_list_and_errors() {
        let ws = api::Workspace::new(&std::env::temp_dir().join("evani_model_mcp_test"));
        let r = handle(&ws, &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18" } })).unwrap();
        assert_eq!(r["result"]["serverInfo"]["name"], "evani-model");
        let r = handle(&ws, &json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" })).unwrap();
        let names: Vec<&str> = r["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"model_make") && names.contains(&"model_compile"));
        assert!(handle(&ws, &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).is_none());
        let r = handle(&ws, &json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "model_make", "arguments": { "intent": "make: kartt\n" } } })).unwrap();
        assert_eq!(r["result"]["isError"], true);
        let text: Value = serde_json::from_str(r["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(text["error"], "unknown_class");
        assert!(text["fix"].as_str().unwrap().contains("kart"));
    }
}
