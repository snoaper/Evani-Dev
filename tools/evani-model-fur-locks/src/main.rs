//! `fur-locks`: bake fur locks into a model's `.glb` at build time.
//!
//! `cargo run --release -j 2 -p evani-model-fur-locks --features locks --bin fur-locks -- <model.evmodel> [--tier low|medium|high|ultra] [--params <locks.json>] [--set kit.fur_locks.<name>=<value>]... [--out <file.glb>] [--check]`
//!
//! Compiles the tier, grows the locks over its fur regions and writes the
//! file with the locks as the solid `fur_locks`. `--check` bakes twice and
//! fails unless both runs give the same bytes. Prints one JSON line.

use std::process::ExitCode;

use evani_model_fur_locks::{LockParams, bake};
use serde_json::json;

fn run(args: &[String]) -> Result<serde_json::Value, String> {
    let after = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let Some(src_path) = args.first().filter(|a| !a.starts_with("--")) else {
        return Err("usage: fur-locks <model.evmodel> [--tier high] [--params <locks.json>] [--set kit.fur_locks.<name>=<value>]... [--out <file.glb>] [--check]".into());
    };
    let src = std::fs::read_to_string(src_path).map_err(|e| format!("{src_path}: {e}"))?;
    let tier = after("--tier").unwrap_or_else(|| "high".into());
    // The file and every --set go into one JSON object, read once, so
    // region fields (`kit.fur_locks.region.<key>.length_mm`) work in both.
    let mut obj = match after("--params") {
        Some(p) => {
            let text = std::fs::read_to_string(&p).map_err(|e| format!("{p}: {e}"))?;
            let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{p}: {e}"))?;
            v.as_object().cloned().ok_or(format!("{p}: parameters must be a JSON object"))?
        }
        None => serde_json::Map::new(),
    };
    for (i, a) in args.iter().enumerate() {
        if a == "--set" {
            let kv = args.get(i + 1).ok_or("--set needs name=value")?;
            let (k, v) = kv.split_once('=').ok_or("--set needs name=value")?;
            let v: f64 = v.parse().map_err(|_| format!("--set {k}: not a number"))?;
            obj.insert(k.to_string(), json!(v));
        }
    }
    let params = LockParams::from_json(&serde_json::Value::Object(obj)).map_err(|e| e.to_string())?;
    let t0 = std::time::Instant::now();
    let a = bake::bake_glb(&src, &tier, &params, 2).map_err(|e| e.to_string())?;
    let ms = t0.elapsed().as_millis() as u64;
    let same = if args.iter().any(|x| x == "--check") {
        let b = bake::bake_glb(&src, &tier, &params, 2).map_err(|e| e.to_string())?;
        if a.glb != b.glb {
            return Err("two bakes of the same input gave different bytes".into());
        }
        Some(true)
    } else {
        None
    };
    if let Some(out) = after("--out") {
        std::fs::write(&out, &a.glb).map_err(|e| format!("{out}: {e}"))?;
    }
    let note = (a.stats.locks == 0).then(|| {
        format!("no locks: no triangle is in a fur region ({}); name the model's fur region with --set kit.fur_locks.region.<region>.length_mm=<mm>", params.regions.iter().map(|r| r.key.as_str()).collect::<Vec<_>>().join(", "))
    });
    Ok(json!({
        "note": note,
        "tier": a.tier, "body_tris": a.body_tris, "locks": a.stats.locks, "lock_tris": a.stats.triangles,
        "near_gear": a.stats.near_gear, "covered": a.stats.covered, "clipped": a.stats.clipped,
        "glb_kb": (a.glb.len() as f64 / 1024.0).round() as u64, "ms": ms, "same_bytes": same,
    }))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(v) => {
            println!("{v}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("fur-locks: {e}");
            ExitCode::FAILURE
        }
    }
}
