//! Envelope helpers shared by `serve` and `mcp` (docs/LIVE_API.md 3.3, 5.5).

use std::time::Duration;

use serde_json::{Value, json};

pub const PROTOCOL: &str = "evani-live/1";
pub const ENGINE: &str = "evani.dev";
/// Largest WebSocket frame / message (captures are the big ones).
pub const MAX_MESSAGE: usize = 8 * 1024 * 1024;
/// How long a page gets to answer an ordinary command.
pub const BASE_TIMEOUT: Duration = Duration::from_secs(10);

/// Commands that change state and need a `note` (5.3).
pub const MUTATING: &[&str] = &[
    "param.set",
    "slot.set",
    "curve.set",
    "policy.set",
    "effect.toggle",
    "tier.set",
    "snapshot.restore",
    "batch",
    "history.undo",
    "history.redo",
    "edit.stage",
    "edit.commit",
    "edit.resolve",
    "model.convert",
];

/// A failed `result` envelope.
pub fn error(id: &Value, code: &str, message: &str, data: Option<Value>) -> Value {
    let mut err = json!({ "code": code, "message": message });
    if let Some(d) = data {
        err["data"] = d;
    }
    json!({ "type": "result", "v": 1, "id": id, "ok": false, "error": err })
}

/// How long the page may take to answer `req` (5.5 `timeout`): 10 s, or for
/// `bench.run` / `compare.ab` their own length (warm-ups included, every run
/// of the order for compare) plus 30 s; `bake.export` and `look.*` 90 s
/// (the look check's captures, 15.3); plus the telemetry
/// window the command asked for. The geometry debug view analyses the whole
/// draw list in the page before it answers (seconds in a big world), so
/// `debug.geometry` with `on` and a `capture.frame` with `geometry: true`
/// get 60 s plus the 30 s.
pub fn limit_for(req: &Value, base: Duration) -> Duration {
    let params = &req["params"];
    let num = |v: &Value, default: f64| v.as_f64().filter(|x| x.is_finite()).unwrap_or(default);
    let run = match req["cmd"].as_str() {
        Some("bench.run") => {
            let s = num(&params["seconds"], 10.0).clamp(1.0, 120.0);
            let w = num(&params["warmup_s"], 1.0).clamp(0.0, 60.0);
            Some(s + w)
        }
        Some("compare.ab") => {
            let s = num(&params["seconds"], 4.0).clamp(1.0, 120.0);
            let w = num(&params["warmup_s"], 0.5).clamp(0.0, 60.0);
            let runs = params["order"].as_str().map_or(4, str::len).clamp(2, 8);
            Some((s + w) * runs as f64)
        }
        Some("sweep.run") => {
            // Rows (values or grid combinations, plus the baseline row) times
            // each row's measuring, warm-up and about 2 s of captures.
            let grid_rows = params["grid"].as_object().map(|g| {
                g.values()
                    .map(|v| v.as_array().map_or(1, Vec::len).max(1))
                    .fold(1usize, |a, n| a.saturating_mul(n))
            });
            let rows = grid_rows.unwrap_or_else(|| params["values"].as_array().map_or(1, Vec::len)).clamp(1, 32)
                + usize::from(params["baseline"].as_bool() != Some(false));
            let f = num(&params["frames_per_value"], 120.0).clamp(10.0, 3600.0) / 60.0;
            let w = num(&params["warmup_s"], 0.25).clamp(0.0, 10.0);
            Some(rows as f64 * (f + w + 2.0))
        }
        Some("debug.geometry") if params["on"].as_bool() == Some(true) => Some(60.0),
        Some("capture.frame") if params["geometry"].as_bool() == Some(true) => Some(60.0),
        // The look check captures every moment twice (15.3): 60 + 30 = 90 s.
        Some("bake.export") => Some(60.0),
        Some(c) if c.starts_with("look.") => Some(60.0),
        // The model view fetches and decodes a tier before it answers
        // (EVANI_MODEL_TAB.md 6); an ultra tier is over a megabyte.
        // model.load (16) fetches the master and a tier the same way.
        Some("view.load" | "view.tier" | "model.load") => Some(0.0),
        _ => None,
    };
    let telemetry_ms = num(&req["opts"]["telemetry_ms"], 1000.0).clamp(0.0, 5000.0);
    let extra = Duration::from_millis(telemetry_ms as u64);
    match run {
        Some(secs) => Duration::from_secs_f64(secs) + Duration::from_secs(30) + extra,
        None => base + extra,
    }
}

/// Checked envelope fields of a command (3.3).
pub struct Envelope {
    pub id: String,
    pub cmd: String,
}

/// Check the envelope of a command. Err is the failure result to send back.
pub fn check_envelope(req: &Value) -> Result<Envelope, Value> {
    let echo = req.get("id").cloned().unwrap_or(Value::Null);
    if !req.is_object() {
        return Err(error(&Value::Null, "bad_request", "a command must be a JSON object", None));
    }
    if req["type"].as_str() != Some("cmd") {
        return Err(error(&echo, "bad_request", "type must be \"cmd\"", None));
    }
    if req["v"].as_u64() != Some(1) {
        return Err(error(
            &echo,
            "version_mismatch",
            "only protocol version 1 is supported",
            Some(json!({ "supported": [1] })),
        ));
    }
    let id = match req["id"].as_str() {
        Some(id) if !id.is_empty() && id.len() <= 64 => id.to_string(),
        _ => {
            return Err(error(&echo, "bad_request", "id must be a string of 1-64 chars", None));
        }
    };
    let Some(cmd) = req["cmd"].as_str().filter(|c| !c.is_empty()) else {
        return Err(error(&echo, "bad_request", "missing cmd", None));
    };
    if req.get("params").is_some_and(|p| !p.is_object()) {
        return Err(error(&echo, "bad_request", "params must be an object", None));
    }
    Ok(Envelope {
        id,
        cmd: cmd.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_checks() {
        let ok = json!({"type":"cmd","v":1,"id":"c-1","cmd":"schema.get"});
        let e = check_envelope(&ok).unwrap();
        assert_eq!((e.id.as_str(), e.cmd.as_str()), ("c-1", "schema.get"));
        let code = |v: Value| check_envelope(&v).err().unwrap()["error"]["code"].clone();
        assert_eq!(code(json!([1])), "bad_request");
        assert_eq!(code(json!({"type":"result","v":1,"id":"a","cmd":"x"})), "bad_request");
        assert_eq!(code(json!({"type":"cmd","v":2,"id":"a","cmd":"x"})), "version_mismatch");
        assert_eq!(code(json!({"type":"cmd","v":1,"id":"","cmd":"x"})), "bad_request");
        assert_eq!(code(json!({"type":"cmd","v":1,"id":"a".repeat(65),"cmd":"x"})), "bad_request");
        assert_eq!(code(json!({"type":"cmd","v":1,"id":7,"cmd":"x"})), "bad_request");
        assert_eq!(code(json!({"type":"cmd","v":1,"id":"a"})), "bad_request");
        assert_eq!(code(json!({"type":"cmd","v":1,"id":"a","cmd":"x","params":[]})), "bad_request");
        let v = check_envelope(&json!({"type":"cmd","v":3,"id":"z","cmd":"x"})).err().unwrap();
        assert_eq!(v["id"], "z");
        assert_eq!(v["error"]["data"]["supported"], json!([1]));
    }

    #[test]
    fn limits() {
        let base = BASE_TIMEOUT;
        let l = |v: Value| limit_for(&v, base).as_secs_f64();
        assert_eq!(l(json!({"cmd":"param.get"})), 11.0);
        assert_eq!(l(json!({"cmd":"param.set","opts":{"telemetry_ms":0}})), 10.0);
        assert_eq!(l(json!({"cmd":"param.set","opts":{"telemetry_ms":99999}})), 15.0);
        assert_eq!(l(json!({"cmd":"bench.run","params":{"seconds":20,"warmup_s":2}})), 53.0);
        assert_eq!(l(json!({"cmd":"compare.ab","params":{"seconds":8,"order":"AB"}})), 48.0);
        assert_eq!(l(json!({"cmd":"compare.ab","params":{"seconds":8,"warmup_s":0}})), 63.0);
        assert_eq!(l(json!({"cmd":"compare.ab"})), 49.0);
        // 3 rows (2 values + baseline) x (2 s + 0.25 s + 2 s) + 30 s + 1 s telemetry
        assert_eq!(l(json!({"cmd":"sweep.run","params":{"param":"x","values":[1,2]}})), 43.75);
        // 2 x 3 grid, no baseline, 60 frames, no warm-up: 6 x 3 s + 31 s
        let g = json!({"cmd":"sweep.run","params":{"grid":{"a":[1,2],"b":[1,2,3]},"baseline":false,
                       "frames_per_value":60,"warmup_s":0}});
        assert_eq!(l(g), 49.0);
        // The geometry view's analysis runs in the page before the answer.
        assert_eq!(l(json!({"cmd":"capture.frame","params":{"geometry":true}})), 91.0);
        assert_eq!(l(json!({"cmd":"capture.frame","params":{"geometry":false}})), 11.0);
        assert_eq!(l(json!({"cmd":"debug.geometry","params":{"on":true}})), 91.0);
        assert_eq!(l(json!({"cmd":"debug.geometry","params":{"on":false}})), 11.0);
        assert_eq!(l(json!({"cmd":"bake.export","opts":{"telemetry_ms":0}})), 90.0);
        assert_eq!(l(json!({"cmd":"look.check","opts":{"telemetry_ms":0}})), 90.0);
        assert_eq!(l(json!({"cmd":"view.load","opts":{"telemetry_ms":0}})), 30.0);
    }
}
