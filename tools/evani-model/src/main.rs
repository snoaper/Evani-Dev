//! `evani-model`: the Evani.Model command line and its stdio MCP server.
//!
//! ```text
//! evani-model describe-system
//! evani-model make <intent.evmodel | text | model-id>
//! evani-model compile <intent | model-id> [--tiers low,ultra] [--out DIR]
//! evani-model check <intent | model-id> [--tiers ...]
//! evani-model sheet <intent | model-id> [--kind blueprint] [--out FILE]
//! evani-model compare <intent | model-id | ours.glb> --target <master.glb> [--tier ultra] [--size 512] [--out FILE] [--threads N]
//! evani-model mcp            (stdio MCP server: model_* tools)
//! ```
//! Every command prints one compact JSON reply. Work state goes to
//! `.evani/model/` under the current folder (or `--project DIR`).

mod mcp;

use evani_model::api;
use serde_json::Value;
use std::path::PathBuf;
use std::process::ExitCode;

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn usage() -> String {
    "usage: evani-model <describe-system | make | compile | check | sheet | compare | mcp> [intent] [--tiers a,b] [--out DIR] [--kind K] [--project DIR] [--threads N]".into()
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let project = flag(&args, "--project").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    let mut ws = api::Workspace::new(&project);
    if let Some(n) = flag(&args, "--threads").and_then(|n| n.parse::<usize>().ok()) {
        ws.threads = n.clamp(1, 64);
    }
    let tiers: Option<Vec<String>> = flag(&args, "--tiers").map(|t| t.split(',').map(|s| s.trim().to_string()).collect());
    let cmd = args.first().map(String::as_str).unwrap_or("");
    let target = args.get(1).cloned().unwrap_or_default();
    let reply: Result<Value, Value> = match cmd {
        "describe-system" => Ok(api::describe_system()),
        "make" => api::make(&ws, &target).map_err(|d| api::error_json(&d)),
        "compile" => api::compile_model(&ws, &target, tiers.as_deref(), flag(&args, "--out").map(PathBuf::from).as_deref()).map_err(|d| api::error_json(&d)),
        "check" => api::check(&ws, &target, tiers.as_deref()).map_err(|d| api::error_json(&d)),
        "sheet" => api::sheet(&ws, &target, flag(&args, "--kind").as_deref().unwrap_or("blueprint"), flag(&args, "--out").map(PathBuf::from).as_deref()).map_err(|d| api::error_json(&d)),
        "compare" => {
            let mut w = api::Workspace::new(&project);
            if let Some(t) = flag(&args, "--threads").and_then(|t| t.parse::<usize>().ok()) {
                w.threads = t.max(1);
            }
            match flag(&args, "--target") {
                Some(tg) => {
                    let size = flag(&args, "--size").and_then(|s| s.parse::<usize>().ok()).unwrap_or(512);
                    let tier = flag(&args, "--tier").unwrap_or_else(|| "ultra".into());
                    api::compare(&w, &target, std::path::Path::new(&tg), &tier, size, flag(&args, "--out").map(PathBuf::from).as_deref()).map_err(|d| api::error_json(&d))
                }
                None => Err(serde_json::json!({ "error": "no_target", "msg": "compare needs --target <master.glb>", "fix": "evani-model compare <intent | model-id | ours.glb> --target <master.glb> [--tier ultra] [--size 512] [--out sheet.png]" })),
            }
        }
        "mcp" => {
            mcp::serve(&ws);
            return ExitCode::SUCCESS;
        }
        _ => {
            eprintln!("{}", usage());
            return ExitCode::FAILURE;
        }
    };
    match reply {
        Ok(v) => {
            println!("{v}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            println!("{e}");
            ExitCode::FAILURE
        }
    }
}
