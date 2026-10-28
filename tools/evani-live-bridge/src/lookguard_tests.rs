//! Look guard on the bridge: the bake gate, proposals, approvals and the
//! audit log, with a fake page and a temporary Evani.App store signed by the
//! public TEST key (docs/LIVE_API.md 15).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use evani_live::img;
use evani_lookguard::{hex, hmac, testkey};
use tokio::sync::mpsc;

use super::*;
use crate::hub::HubInfo;

pub(crate) const SECRET: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
pub(crate) const LOOK: &str = "evani-look-basic";
pub(crate) const PROJECT: &str = "proj-1";
pub(crate) const SESSION: &str = "s-test";

pub(crate) struct Fx {
    pub hub: Arc<Hub>,
    pub root: PathBuf,
    pub store: PathBuf,
    /// The fake page's next captures are much brighter.
    pub bright: Arc<AtomicBool>,
    /// `capture.frame` calls the fake page answered.
    pub shots: Arc<AtomicUsize>,
}

pub(crate) fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("evani-bridge-lg-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

pub(crate) fn stdin_line(pubkey: &[u8; 32]) -> String {
    json!({ "approve_secret": SECRET, "approvals_pubkey": hex::encode(pubkey), "project_id": PROJECT }).to_string()
}

/// A project root with `apps/sandbox/evani.toml` naming the basic look, an
/// Evani.App store pinning the TEST key, and (if `stdin`) the first stdin
/// line Evani.App would send.
pub(crate) fn fixture(name: &str, port: u16, stdin: bool) -> Fx {
    let base = temp(name);
    let root = base.join("root");
    let store = base.join("store");
    std::fs::create_dir_all(root.join("apps/sandbox")).unwrap();
    std::fs::write(root.join("apps/sandbox/evani.toml"), "name = \"sandbox\"\nlook = \"basic\" # the look\n").unwrap();
    std::fs::create_dir_all(&store).unwrap();
    std::fs::write(store.join("pubkey"), format!("{}\n", hex::encode(&testkey::public()))).unwrap();
    let hub = Arc::new(Hub::with_timeout(
        HubInfo { port, root: root.display().to_string(), game: "sandbox".into(), session: SESSION.into() },
        Duration::from_secs(5),
    ));
    hub.lookguard.set_store_dir(Some(store.clone()));
    if stdin {
        hub.lookguard.on_stdin_line(&stdin_line(&testkey::public()));
    }
    Fx { hub, root, store, bright: Arc::new(AtomicBool::new(false)), shots: Arc::new(AtomicUsize::new(0)) }
}

/// A 64×40 test picture: a soft gradient, or the same much brighter.
pub(crate) fn picture(bright: bool) -> Vec<u8> {
    let (w, h) = (64u32, 40u32);
    let mut px = Vec::with_capacity((w * h * 3) as usize);
    let lift: u8 = if bright { 90 } else { 0 };
    for y in 0..h {
        for x in 0..w {
            let r = 40 + (x * 2) as u8;
            let g = 50 + (y * 2) as u8;
            px.extend_from_slice(&[r.saturating_add(lift), g.saturating_add(lift), 90u8.saturating_add(lift)]);
        }
    }
    img::encode_png(&img::Rgb { w, h, px })
}

fn bundle(dry_run: bool, value: f64) -> Value {
    json!({
        "title": "tune", "dry_run": dry_run, "from_tx": 1, "to_tx": 2, "game": "sandbox",
        "look_crate": LOOK, "kit_crate": "evani-kit-example",
        "params": [{ "name": "look.basic.saturation", "bake": "look", "value": value }],
        "slots": [], "curves": [], "tiers": [], "notes": [format!("**look.basic.saturation** 1.0 → {value}")]
    })
}

/// A fake page: answers `capture.frame` with [`picture`] and `bake.export`
/// with a bundle (params `dry_run`, `value`). Returns its engine id.
pub(crate) fn fake_page(fx: &Fx) -> u64 {
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let id = fx.hub.attach_engine(tx);
    let (h, bright, shots) = (fx.hub.clone(), fx.bright.clone(), fx.shots.clone());
    tokio::spawn(async move {
        while let Some(text) = rx.recv().await {
            let m: Value = serde_json::from_str(&text).unwrap();
            if m["type"] != "cmd" {
                continue;
            }
            let p = &m["params"];
            let result = match m["cmd"].as_str() {
                Some("capture.frame") => {
                    shots.fetch_add(1, Ordering::SeqCst);
                    let png = picture(bright.load(Ordering::SeqCst));
                    json!({ "png_base64": img::base64(&png), "width": 64, "height": 40,
                            "tier": p["tier"], "values": p["values"] })
                }
                Some("bake.export") => {
                    json!({ "bundle": bundle(p["dry_run"] == true, p["value"].as_f64().unwrap_or(1.2)) })
                }
                _ => json!({}),
            };
            let reply = json!({ "type": "result", "v": 1, "id": m["id"], "ok": true, "result": result });
            h.on_engine_message(id, &reply.to_string());
        }
    });
    id
}

pub(crate) async fn send(h: &Hub, source: &str, cmd: &str, params: Value, note: &str) -> Value {
    let mut req = json!({ "type": "cmd", "v": 1, "id": "t-1", "cmd": cmd, "params": params, "opts": { "telemetry_ms": 0 } });
    if !note.is_empty() {
        req["note"] = json!(note);
    }
    h.command(h.http_conn(), source, req).await
}

async fn bake_now(fx: &Fx, value: f64) -> Value {
    send(&fx.hub, "mcp", "bake.export", json!({ "title": "tune", "dry_run": false, "value": value }), "").await
}

/// Evani.App's approval of one request: (header MAC, body).
pub(crate) fn signed_approval(id: &str, kind: &str, look: &str, hash: &str, expiry: u64) -> (String, Value) {
    let a = record::Approval {
        approval_id: id.into(),
        kind: kind.into(),
        look: look.into(),
        hash: hash.into(),
        expiry,
        session: SESSION.into(),
    };
    let sig = testkey::sign(a.canonical().as_bytes());
    let mac = hex::encode(&hmac::hmac_sha256(&hex::decode(SECRET).unwrap(), format!("{id}|{hash}|{expiry}").as_bytes()));
    let body = json!({ "approvalId": id, "kind": kind, "look": look, "hash": hash, "expiry": expiry,
                       "session": SESSION, "sig": hex::encode(&sig) });
    (mac, body)
}

/// The approval for a request as the engine reported it (`approvalId`,
/// `look`, `hash`, `expiry`).
pub(crate) fn approval_for(req: &Value, kind: &str) -> (String, Value) {
    signed_approval(
        req["approvalId"].as_str().unwrap(),
        kind,
        req["look"].as_str().unwrap(),
        req["hash"].as_str().unwrap(),
        req["expiry"].as_u64().unwrap(),
    )
}

/// What Evani.App writes when Chris approves limits: the signed record.
pub(crate) fn sign_record(fx: &Fx, hash: &str, seq: u64) {
    let mut r = record::Record {
        project_id: PROJECT.into(),
        project_path: fx.root.display().to_string(),
        look: LOOK.into(),
        seq,
        approved_at: "2026-10-14T15:00:00Z".into(),
        hash: hash.into(),
        sig: [0; 64],
    };
    r.sig = testkey::sign(r.canonical().as_bytes());
    std::fs::create_dir_all(fx.store.join(PROJECT)).unwrap();
    std::fs::write(fx.store.join(PROJECT).join(format!("{LOOK}.json")), r.to_json().to_string()).unwrap();
}

/// An approved guard set written straight to disk (no page needed), as if
/// a baseline had been approved earlier. Returns its hash.
pub(crate) fn approved_guard(fx: &Fx) -> String {
    let moments = Moments::from_json(&json!({
        "format": limits::MOMENTS_FORMAT, "game": "sandbox", "tier": "medium", "width": 64,
        "moments": [{ "name": "start", "capture": { "sim_time": 1 } }]
    }))
    .unwrap();
    let shot = Shot { moment: "start".into(), on: picture(false), off: picture(false) };
    let shots = vec![shot];
    let lim = Limits::generate(LOOK, shots::stats_of(&shots).unwrap(), None);
    let dir = guardset::guard_dir(&fx.root, LOOK);
    for (rel, bytes) in shots::staged_files(&moments, &lim, &shots) {
        let p = dir.join(&rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }
    let hash = guardset::guard_hash(&fx.root, LOOK).unwrap().unwrap().hash;
    sign_record(fx, &hash, 1);
    hash
}

fn events(fx: &Fx) -> Vec<String> {
    audit::read(&fx.root).iter().map(|e| e["event"].as_str().unwrap_or("").to_string()).collect()
}

fn params_json(fx: &Fx) -> String {
    std::fs::read_to_string(fx.root.join(format!("looks/{LOOK}/params.json"))).unwrap_or_default()
}

#[tokio::test]
async fn baseline_is_pending_until_approved_then_bakes_are_checked() {
    let fx = fixture("flow", 8096, true);
    fake_page(&fx);
    let (ctl_tx, mut ctl_rx) = mpsc::unbounded_channel::<String>();
    fx.hub.attach_ctl(ctl_tx);

    // Unguarded: a real bake is refused, a dry run still previews.
    let s = send(&fx.hub, "mcp", "look.status", json!({}), "").await;
    assert_eq!(s["result"]["guard"], "unguarded", "{s}");
    assert_eq!(s["result"]["look"], LOOK);
    assert_eq!(s["result"]["approvals"]["approvals"], "enabled");
    let r = bake_now(&fx, 1.2).await;
    assert_eq!(r["error"]["code"], "look_unapproved", "{r}");
    assert_eq!(r["error"]["data"]["reason"], "unguarded");
    let r = send(&fx.hub, "mcp", "bake.export", json!({ "dry_run": true }), "").await;
    assert_eq!(r["ok"], true, "{r}");
    assert!(!fx.root.join(format!("looks/{LOOK}/params.json")).exists());

    // Propose a baseline: it needs a note, and stays pending.
    let moments = json!({ "tier": "medium", "width": 64, "moments": [{ "name": "start", "capture": { "sim_time": 1 } }] });
    let r = send(&fx.hub, "mcp", "look.baseline.propose", json!({ "moments": moments }), "").await;
    assert_eq!(r["error"]["code"], "bad_request", "{r}");
    let r = send(&fx.hub, "mcp", "look.baseline.propose", json!({ "moments": moments }), "first baseline").await;
    assert_eq!(r["result"]["status"], "pending", "{r}");
    assert_eq!(fx.shots.load(Ordering::SeqCst), 2, "one bloom-on and one bloom-off shot per moment");
    let proposal = r["result"].clone();
    assert!(!guardset::guard_dir(&fx.root, LOOK).exists(), "nothing is written before approval");
    let ev = ctl_rx.try_recv().unwrap();
    assert!(ev.contains("look_guard.approval_requested") && ev.contains("\"approvePort\""), "{ev}");
    let s = send(&fx.hub, "mcp", "look.status", json!({}), "").await;
    assert_eq!(s["result"]["guard"], "unguarded");
    assert_eq!(s["result"]["pending"].as_array().unwrap().len(), 1);

    // Chris approves in Evani.App: it signs the record, then posts the approval.
    sign_record(&fx, proposal["hash"].as_str().unwrap(), 1);
    let (mac, body) = approval_for(&proposal, "limits");
    let ok = fx.hub.lookguard.approve(&fx.hub, &mac, &body).unwrap();
    assert_eq!(ok["verify"]["ok"], true, "{ok}");
    assert_eq!(ok["written"].as_array().unwrap().len(), 4, "{ok}");
    assert!(guardset::guard_dir(&fx.root, LOOK).join("baseline/start.nobloom.png").exists());
    assert!(ctl_rx.try_recv().unwrap().contains("look_guard.approved"));
    // An approval works once.
    assert_eq!(fx.hub.lookguard.approve(&fx.hub, &mac, &body).unwrap_err().code, "already_used");

    // A clean bake passes and records the guard in its manifest.
    let r = bake_now(&fx, 1.2).await;
    assert_eq!(r["ok"], true, "{r}");
    let lg = &r["result"]["look_guard"];
    assert_eq!(lg["check"], "passed", "{r}");
    assert_eq!(lg["key_id"], record::key_id(&testkey::public()));
    assert_eq!(lg["test_key"], true);
    assert!(lg["warnings"].to_string().contains("TEST key"));
    let manifest: Value =
        serde_json::from_str(&std::fs::read_to_string(fx.root.join(manifest_rel(LOOK))).unwrap()).unwrap();
    assert_eq!(manifest["guard_hash"], proposal["hash"]);
    assert_eq!(manifest["check"], "passed");
    assert_eq!(manifest["seq"], 1);
    let changes = std::fs::read_to_string(fx.root.join(format!("looks/{LOOK}/CHANGES.md"))).unwrap();
    assert!(changes.contains("Look check passed against Chris's approved limits"), "{changes}");

    // An AI-style exposure change: the check flags it in plain sentences.
    fx.bright.store(true, Ordering::SeqCst);
    let c = send(&fx.hub, "mcp", "look.check", json!({}), "").await;
    assert_eq!(c["result"]["passed"], false, "{c}");
    assert!(c["result"]["sentences"][0].as_str().unwrap().contains("on moment 'start'"), "{c}");

    // A flagged bake is refused and writes nothing.
    let r = bake_now(&fx, 1.5).await;
    assert_eq!(r["error"]["code"], "look_flagged", "{r}");
    assert!(r["error"]["message"].as_str().unwrap().contains("average brightness"), "{r}");
    assert!(!params_json(&fx).contains("1.5"));
    let flagged = r["error"]["data"].clone();

    // Wrong MAC, a signature over something else, an edited body: refused.
    let (mac, body) = approval_for(&flagged, "bake");
    assert_eq!(fx.hub.lookguard.approve(&fx.hub, &"0".repeat(64), &body).unwrap_err().code, "bad_auth");
    let mut forged = body.clone();
    forged["sig"] = json!(hex::encode(&testkey::sign(b"something else")));
    assert_eq!(fx.hub.lookguard.approve(&fx.hub, &mac, &forged).unwrap_err().code, "bad_signature");
    let other = "f".repeat(64);
    let (mac2, body2) = signed_approval(flagged["approvalId"].as_str().unwrap(), "bake", LOOK, &other, flagged["expiry"].as_u64().unwrap());
    assert_eq!(fx.hub.lookguard.approve(&fx.hub, &mac2, &body2).unwrap_err().code, "mismatch");

    // Chris approves the bake.
    let ok = fx.hub.lookguard.approve(&fx.hub, &mac, &body).unwrap();
    assert_eq!(ok["kind"], "bake");
    // The grant doesn't cover a different bundle…
    let r = bake_now(&fx, 1.6).await;
    assert_eq!(r["error"]["code"], "look_flagged", "{r}");
    // …only the same bake, once.
    let r = bake_now(&fx, 1.5).await;
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["result"]["look_guard"]["check"], "approved");
    assert_eq!(r["result"]["look_guard"]["approval_id"], flagged["approvalId"]);
    assert!(params_json(&fx).contains("1.5"));
    let changes = std::fs::read_to_string(fx.root.join(format!("looks/{LOOK}/CHANGES.md"))).unwrap();
    assert!(changes.contains("Chris approved it"), "{changes}");
    let r = bake_now(&fx, 1.5).await;
    assert_eq!(r["error"]["code"], "look_flagged", "the approval can't be reused: {r}");

    // The audit log has every step, and its chain holds.
    let ev = events(&fx);
    for e in ["bake_refused", "proposed", "approved", "approve_refused", "baked", "check", "bake_flagged"] {
        assert!(ev.iter().any(|x| x == e), "audit has no {e}: {ev:?}");
    }
    let text = std::fs::read_to_string(audit::path(&fx.root)).unwrap();
    assert_eq!(audit::verify_chain(&text).unwrap(), ev.len());
    assert!(!text.contains(SECRET), "the secret never reaches the audit log");
}

#[tokio::test]
async fn the_live_path_can_only_propose() {
    let fx = fixture("live", 8096, true);
    fake_page(&fx);
    let hash = approved_guard(&fx);
    let limits_before = std::fs::read(guardset::guard_dir(&fx.root, LOOK).join("limits.json")).unwrap();

    // No approve command on the live path, whatever its spelling.
    for c in ["look.approve", "look.approve.limits", "look.approve_bake"] {
        let r = send(&fx.hub, "mcp", c, json!({ "approvalId": "la-x" }), "").await;
        assert_eq!(r["error"]["code"], "forbidden", "{c}: {r}");
    }

    // A looser limit is only a proposal: nothing on disk changes.
    let loose = json!({ "tolerances": { "mean_luma": { "mode": "abs", "value": 0.5 } } });
    let r = send(&fx.hub, "mcp", "look.limits.propose", loose.clone(), "").await;
    assert_eq!(r["error"]["code"], "bad_request", "a proposal needs a note: {r}");
    let r = send(&fx.hub, "mcp", "look.limits.propose", loose.clone(), "brighter look").await;
    assert_eq!(r["result"]["status"], "pending", "{r}");
    assert_eq!(r["result"]["looser"], true, "{r}");
    assert_eq!(r["result"]["oldHash"], hash.as_str());
    let first = r["result"].clone();
    assert_eq!(std::fs::read(guardset::guard_dir(&fx.root, LOOK).join("limits.json")).unwrap(), limits_before);
    let s = send(&fx.hub, "mcp", "look.status", json!({}), "").await;
    assert_eq!(s["result"]["guard"], "verified", "{s}");
    assert_eq!(s["result"]["guard_hash"], hash.as_str());

    // A newer proposal replaces the older one; the older can't be approved.
    let r = send(&fx.hub, "mcp", "look.limits.propose", json!({ "band_widen": { "fraction": 0.5 } }), "wider band").await;
    let second = r["result"].clone();
    let (mac, body) = approval_for(&first, "limits");
    assert_eq!(fx.hub.lookguard.approve(&fx.hub, &mac, &body).unwrap_err().code, "already_used");

    // Tighten-then-loosen by hand: editing the limits on disk breaks the
    // signed record, so bakes stop until Chris approves again…
    let lp = guardset::guard_dir(&fx.root, LOOK).join("limits.json");
    let edited = String::from_utf8(limits_before.clone()).unwrap().replace("\"value\": 0.04", "\"value\": 0.9");
    assert_ne!(edited.as_bytes(), limits_before.as_slice());
    std::fs::write(&lp, edited).unwrap();
    let s = send(&fx.hub, "mcp", "look.status", json!({}), "").await;
    assert_eq!(s["result"]["guard"], "failed", "{s}");
    assert_eq!(s["result"]["problem"]["code"], "hash_mismatch");
    let r = bake_now(&fx, 1.2).await;
    assert_eq!(r["error"]["code"], "look_unapproved", "{r}");
    assert_eq!(r["error"]["data"]["reason"], "hash_mismatch");
    // …and a proposal made before the edit is stale.
    let (mac, body) = approval_for(&second, "limits");
    assert_eq!(fx.hub.lookguard.approve(&fx.hub, &mac, &body).unwrap_err().code, "stale");

    // Put the approved limits back; a fresh proposal approved properly
    // (record signed first, as Evani.App does) takes effect, with seq 2.
    std::fs::write(&lp, &limits_before).unwrap();
    let r = send(&fx.hub, "mcp", "look.limits.propose", loose, "brighter look").await;
    let third = r["result"].clone();
    sign_record(&fx, third["hash"].as_str().unwrap(), 2);
    let (mac, body) = approval_for(&third, "limits");
    let ok = fx.hub.lookguard.approve(&fx.hub, &mac, &body).unwrap();
    assert_eq!(ok["verify"]["ok"], true, "{ok}");
    assert_eq!(ok["verify"]["seq"], 2);
    assert_eq!(ok["guard_hash"], third["hash"]);

    let ev = events(&fx);
    assert_eq!(ev.iter().filter(|e| *e == "approve_refused").count(), 5, "{ev:?}");
    // The origin is the door the request came in on, set by the server.
    assert!(audit::read(&fx.root).iter().all(|e| e["event"] != "proposed" || e["origin"] == "mcp"));
}

#[tokio::test]
async fn a_paused_page_keeps_its_reason_through_the_look_check() {
    let fx = fixture("paused", 8096, true);
    let page = fake_page(&fx);
    approved_guard(&fx);
    // Hidden, and the frame counter stood still between two beats (3.5).
    for _ in 0..2 {
        let m = json!({ "type": "page_state", "v": 1, "reason": "beat", "frames": 40, "visibility": "hidden", "host": null });
        fx.hub.on_engine_message(page, &m.to_string());
    }
    let t = std::time::Instant::now();
    let r = send(&fx.hub, "mcp", "look.check", json!({}), "").await;
    assert!(t.elapsed() < Duration::from_millis(200), "{:?}", t.elapsed());
    assert_eq!(r["error"]["code"], "paused", "{r}");
    assert_eq!(r["error"]["data"]["reason"], "paused (tab hidden)", "{r}");
    assert!(r["error"]["data"]["fix"].as_str().unwrap().contains("Show the Evani.dev tab"), "{r}");
    assert_eq!(r["error"]["data"]["page"]["state"], "paused", "{r}");
    assert_eq!(fx.shots.load(Ordering::SeqCst), 0, "nothing was sent to the paused page");
}

#[tokio::test]
async fn approvals_stay_off_without_evani_app() {
    // No stdin line: off, with the plain reason.
    let fx = fixture("off", 8096, false);
    approved_guard(&fx);
    let st = fx.hub.lookguard.status();
    assert_eq!(st["approvals"], "disabled");
    assert_eq!(st["reason"], OFF_REASON);
    assert_eq!(st["key_id"], record::key_id(&testkey::public()), "the pinned key's id is shown");
    let r = send(&fx.hub, "mcp", "look.limits.propose", json!({ "band_widen": { "fraction": 0.3 } }), "x").await;
    assert!(r["result"]["message"].as_str().unwrap().contains("approvals are off"), "{r}");
    let (mac, body) = approval_for(&r["result"], "limits");
    assert_eq!(fx.hub.lookguard.approve(&fx.hub, &mac, &body).unwrap_err().code, "approvals_disabled");

    // Someone starting the engine with their own key gets nowhere.
    // (Keys::new compares bytes with the pinned key before anything else.)
    let mut other = testkey::public();
    other[0] ^= 1;
    let line = fx.hub.lookguard.on_stdin_line(&stdin_line(&other));
    assert!(line.contains("approvals off") && !line.contains(SECRET), "{line}");
    let st = fx.hub.lookguard.status();
    assert_eq!(st["approvals"], "disabled");
    assert!(st["reason"].as_str().unwrap().contains("not the one Evani.App pinned"), "{st}");
    assert!(!st.to_string().contains(SECRET));
    // Only the first line counts.
    let line = fx.hub.lookguard.on_stdin_line(&stdin_line(&testkey::public()));
    assert!(line.contains("ignoring"), "{line}");
    assert_eq!(fx.hub.lookguard.status()["approvals"], "disabled");
}

#[test]
fn key_change_warning_and_small_parsers() {
    let fx = fixture("key", 8096, false);
    assert_eq!(key_warning(&fx.root, LOOK, "abcd"), None, "no manifest yet");
    bake::write(&fx.root, &manifest_rel(LOOK), "{\"key_id\":\"1111222233334444\"}").unwrap();
    assert_eq!(key_warning(&fx.root, LOOK, "1111222233334444"), None);
    let w = key_warning(&fx.root, LOOK, "aaaabbbbccccdddd").unwrap();
    assert!(w.contains("1111222233334444") && w.contains("aaaabbbbccccdddd"), "{w}");

    assert_eq!(look_from_manifest("look = \"basic\""), Some("evani-look-basic".into()));
    assert_eq!(look_from_manifest("# look = \"x\"\nlook=\"evani-look-neon\""), Some("evani-look-neon".into()));
    assert_eq!(look_from_manifest("looks = 1"), None);
    assert_eq!(endpoint_port(&json!(8123)), Some(8123));
    assert_eq!(endpoint_port(&json!({ "port": 8124 })), Some(8124));
    assert_eq!(endpoint_port(&json!("http://127.0.0.1:8125/look-approvals/v1/seq")), Some(8125));
    assert_eq!(endpoint_port(&json!("http://evil.example:8125")), None);
    assert_eq!(endpoint_port(&json!(0)), None);
}
