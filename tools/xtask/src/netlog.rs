//! `cargo xtask netlog <app> [--tier low,ultra]`: which files each tier
//! really downloads (S3-4). Serves the ship build on a port in 8240-8249,
//! opens it in headless, muted Chrome once per tier (the tier saved in the
//! page's storage, so there is no warm-up; a slow line, so the placeholder is
//! on screen before the model streams in), records every request (CDP
//! `Network.requestWillBeSent`) and judges the files under `models/`:
//!
//! * FAIL a file of another tier (`<id>.<other>.glb`, `<id>.<other>.base.jpg`);
//! * FAIL a shared file the tier has its own copy of (`hero.glb` on Low when
//!   `hero.low.glb` exists);
//! * FAIL a file that did not arrive (status other than 200 or 206);
//! * FAIL a run that fetched no model at all (the model never streamed).
//!
//! Writes `netlog-<tier>.json`, the screenshots and `netlog-report.md` into
//! `--out` (default `.evani-shot-netlog-<app>/`, with a `.gitignore`).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

use crate::bench::Server;

pub fn usage() -> &'static str {
    "  cargo xtask netlog <app> [--tier low,ultra] [--no-build] [--out <dir>] [--page-ms 20000] [--shots-at 1500,4000,19000] [--throttle-kbps 6000] [--chrome <exe>] [--device phone|desktop]"
}

/// One request the page sent: URL and status (`None` while unfinished,
/// a text for a failed load).
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub url: String,
    pub status: Option<String>,
    /// When the body finished loading (ms after the navigation).
    pub done_ms: Option<u64>,
}

/// The verdict for one tier's run.
#[derive(Debug, Default, PartialEq)]
pub struct Verdict {
    /// The files fetched from `models/` (path under it), in order.
    pub model_files: Vec<String>,
    pub failures: Vec<String>,
    /// When the last `models/` file had arrived (ms after the navigation):
    /// the model is ready to draw from then on. `None` if one never finished.
    pub ready_ms: Option<u64>,
}

/// Is the shared `name` one that `tier` has its own copy of in `names`?
fn superseded(names: &[String], name: &str, tier: &str) -> bool {
    if evani_guide::stream::tier_of(name).is_some() {
        return false;
    }
    let (dir, file) = name.rsplit_once('/').unwrap_or(("", name));
    let Some((id, rest)) = file.split_once('.') else { return false };
    let own = if dir.is_empty() { format!("{id}.{tier}.{rest}") } else { format!("{dir}/{id}.{tier}.{rest}") };
    names.contains(&own)
}

/// Judge one tier's requests against the manifest's file names.
pub fn judge(tier: &str, requests: &[Request], names: &[String]) -> Verdict {
    let mut v = Verdict::default();
    let mut ready = Some(0u64);
    for r in requests {
        let path = r.url.split(['?', '#']).next().unwrap_or(&r.url);
        let Some((_, file)) = path.split_once("/models/") else { continue };
        if file == "stream.json" {
            continue;
        }
        v.model_files.push(file.to_string());
        ready = ready.zip(r.done_ms).map(|(a, b)| a.max(b));
        if let Some(t) = evani_guide::stream::tier_of(file).filter(|t| *t != tier) {
            v.failures.push(format!("{tier} fetched {file}, a file of the {t} tier"));
        }
        if superseded(names, file, tier) {
            v.failures.push(format!("{tier} fetched the shared {file} although it has its own copy"));
        }
        match r.status.as_deref() {
            Some("200" | "206") => {}
            Some(s) => v.failures.push(format!("{file}: status {s}")),
            None => v.failures.push(format!("{file}: still unfinished when the page closed (raise --page-ms)")),
        }
    }
    if !v.model_files.iter().any(|f| f.ends_with(".glb") || f.ends_with(".gltf")) {
        v.failures.push(format!("{tier} fetched no model file: the model never streamed in"));
        ready = None;
    }
    v.ready_ms = ready;
    v
}

/// A device the run stands for (S7-6): a phone gets ASTC textures, a desktop
/// BC7, and neither may ever fetch the other's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Device {
    Phone,
    Desktop,
}

impl Device {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "phone" => Ok(Device::Phone),
            "desktop" => Ok(Device::Desktop),
            _ => Err(format!("netlog: unknown device {s} (phone or desktop)")),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Device::Phone => "phone",
            Device::Desktop => "desktop",
        }
    }

    /// Its own compressed family and the one it must never fetch.
    pub fn families(self) -> (&'static str, &'static str) {
        match self {
            Device::Phone => ("astc", "bc7"),
            Device::Desktop => ("bc7", "astc"),
        }
    }
}

/// The device rule on a run's `models/` files: FAIL every file of the other
/// device's texture family. Returns the failures and the families seen.
pub fn judge_device(device: Device, model_files: &[String]) -> (Vec<String>, Vec<&'static str>) {
    let (_, never) = device.families();
    let mut seen: Vec<&'static str> = Vec::new();
    let mut fails = Vec::new();
    for f in model_files {
        let Some(fam) = evani_guide::tier_fetch::family_of(f) else { continue };
        if !seen.contains(&fam) {
            seen.push(fam);
        }
        if fam == never {
            fails.push(format!("the {} fetched {f}, a {} texture (only the other kind of device can use it)", device.label(), never.to_ascii_uppercase()));
        }
    }
    (fails, seen)
}

/// Run `adb` with `args`; its trimmed stdout.
fn adb(args: &[&str]) -> Result<String, String> {
    let o = Command::new("adb").args(args).output().map_err(|e| format!("netlog --device phone: cannot run adb: {e} (install the Android platform tools)"))?;
    if !o.status.success() {
        return Err(format!("netlog --device phone: adb {}: {}", args.join(" "), String::from_utf8_lossy(&o.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// The phone over USB: the page's port reversed onto the phone (so its
/// `127.0.0.1` is this PC's server) and Chrome's DevTools socket forwarded
/// here. Returns what to undo afterwards.
fn phone_setup(port: u16) -> Result<(String, String), String> {
    let devices = adb(&["devices"])?;
    if !devices.lines().skip(1).any(|l| l.trim_end().ends_with("\tdevice")) {
        return Err("netlog --device phone: no phone answers over adb (lease it, plug it in and allow USB debugging)".into());
    }
    adb(&["reverse", &format!("tcp:{port}"), &format!("tcp:{port}")])?;
    // Chrome must be running for its DevTools socket to exist.
    let _ = adb(&["shell", "am", "start", "-n", "com.android.chrome/com.google.android.apps.chrome.Main", "-d", "about:blank"]);
    let fwd = adb(&["forward", "tcp:0", "localabstract:chrome_devtools_remote"])?;
    Ok((format!("tcp:{port}"), format!("tcp:{fwd}")))
}

/// The requests of one runner job.
fn requests_of(job: &Value) -> Vec<Request> {
    job.get("requests")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|r| Request {
                    url: r.get("url").and_then(Value::as_str).unwrap_or("").to_string(),
                    status: match r.get("status") {
                        Some(Value::Number(n)) => Some(n.to_string()),
                        Some(Value::String(s)) => Some(s.clone()),
                        _ => None,
                    },
                    done_ms: r.get("done_ms").and_then(Value::as_u64),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn opt<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(String::as_str)
}

pub fn cmd_netlog(args: &[String]) -> Result<(), String> {
    let mut positional = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--no-build" {
            i += 1;
        } else if args[i].starts_with("--") {
            i += 2;
        } else {
            positional.push(args[i].clone());
            i += 1;
        }
    }
    let app = positional.first().ok_or_else(|| format!("usage:\n{}", usage()))?.clone();
    let (tiers, device) = tiers_and_device(args)?;
    let root = crate::workspace_root();
    if !args.iter().any(|a| a == "--no-build") {
        crate::cmd_build(&app, true, false)?;
    }
    let dist = root.join("target").join("evani-sizes").join(format!("{app}-ship"));
    run(&app, &app, args, &root, &dist, &tiers, device)
}

/// `cargo xtask netlog --app-dir <folder> [..]`: the same run for a game
/// outside the engine: its ship build (`build --app-dir --ship`) in its own
/// workspace's `target/evani-sizes`, the output by default in
/// `<game workspace>/.evani-shot-netlog-<id>/` (a relative `--out` is from
/// the game workspace). The bench runner is the engine's.
pub(crate) fn cmd_netlog_app(app: &crate::AppDir, args: &[String]) -> Result<(), String> {
    let (tiers, device) = tiers_and_device(args)?;
    if !args.iter().any(|a| a == "--no-build") {
        crate::build_app(app, true, false)?;
    }
    let home = app.external.as_ref().map_or_else(crate::workspace_root, |e| e.root.clone());
    let dist = app.sizes_dir().join(format!("{}-ship", app.name));
    let arg = format!("--app-dir {}", app.dir.display());
    run(&app.name, &arg, args, &home, &dist, &tiers, device)
}

/// The `--tier` list (checked) and `--device`.
fn tiers_and_device(args: &[String]) -> Result<(Vec<String>, Option<Device>), String> {
    let tiers: Vec<String> = opt(args, "--tier").unwrap_or("low,ultra").split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    for t in &tiers {
        if !evani_guide::stream::TIERS.contains(&t.as_str()) {
            return Err(format!("netlog: unknown tier {t} (one of {})", evani_guide::stream::TIERS.join(", ")));
        }
    }
    Ok((tiers, opt(args, "--device").map(Device::parse).transpose()?))
}

/// The run on the ship folder `dist` of game `app` (named `arg` in
/// commands); `--out` and the default output folder are under `home`.
fn run(app: &str, arg: &str, args: &[String], home: &Path, dist: &Path, tiers: &[String], device: Option<Device>) -> Result<(), String> {
    let root = crate::workspace_root();
    let manifest_text = std::fs::read_to_string(dist.join("models/stream.json"))
        .map_err(|e| format!("netlog: {}/models/stream.json: {e} (run `cargo xtask manifest {arg}`, then build)", dist.display()))?;
    let manifest: Value = serde_json::from_str(&manifest_text).map_err(|e| format!("netlog: stream.json: {e}"))?;
    let names: Vec<String> = manifest["assets"].as_array().map(|a| a.iter().filter_map(|e| e["name"].as_str().map(str::to_string)).collect()).unwrap_or_default();

    let out = opt(args, "--out").map_or_else(|| home.join(format!(".evani-shot-netlog-{app}")), |o| home.join(o));
    std::fs::create_dir_all(&out).map_err(|e| format!("netlog: {}: {e}", out.display()))?;
    if !out.join(".gitignore").exists() {
        std::fs::write(out.join(".gitignore"), "*\n").map_err(|e| e.to_string())?;
    }
    let server = Server::start_in(dist.to_path_buf(), 8240..=8249)?;
    let url = format!("http://127.0.0.1:{}/", server.port);
    eprintln!("  serving the ship build at {url}");
    // The phone runs under its own labels, so a desktop run beside it keeps its files.
    let label = |t: &str| if device == Some(Device::Phone) { format!("{t}-phone") } else { t.to_string() };
    let jobs: Vec<Value> = tiers
        .iter()
        .map(|t| json!({ "tier": t, "label": label(t), "width": 1280, "height": 720, "storage": { format!("evani:{app}:render.tier"): t } }))
        .collect();
    let phone = if device == Some(Device::Phone) { Some(phone_setup(server.port)?) } else { None };
    let runner = root.join("tools").join("bench-runner").join("run.mjs");
    let mut cmd = Command::new("node");
    cmd.arg(&runner)
        .args(["--url", &url, "--jobs", &Value::Array(jobs).to_string()])
        .args(["--page-ms", opt(args, "--page-ms").unwrap_or("20000")])
        .args(["--shots-at", opt(args, "--shots-at").unwrap_or("1500,4000,19000")])
        .args(["--throttle-kbps", opt(args, "--throttle-kbps").unwrap_or("6000")])
        .arg("--shots")
        .arg(&out)
        .arg("--netlog")
        .arg(&out)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    if let Some(c) = opt(args, "--chrome") {
        cmd.args(["--chrome", c]);
    }
    if let Some((_, fwd)) = &phone {
        cmd.args(["--connect", &format!("http://127.0.0.1:{}", fwd.trim_start_matches("tcp:"))]);
    }
    let o = cmd.output().map_err(|e| format!("netlog: cannot run node: {e}"));
    if let Some((rev, fwd)) = &phone {
        let _ = adb(&["reverse", "--remove", rev]);
        let _ = adb(&["forward", "--remove", fwd]);
    }
    let o = o?;
    drop(server);
    if !o.status.success() {
        return Err(format!("netlog: the bench runner failed ({})", o.status));
    }
    let res: Value = serde_json::from_slice(&o.stdout).map_err(|e| format!("netlog: runner output: {e}"))?;
    report(app, &out, &res, &names, device)
}

fn report(app: &str, out: &Path, res: &Value, names: &[String], device: Option<Device>) -> Result<(), String> {
    let line = res["throttle_kbps"].as_u64().map_or_else(|| "not throttled".to_string(), |k| format!("throttled to {k} kbit/s"));
    println!("netlog: line {line}");
    let mut md = format!("# Network log: {app}\n\nChrome: {}\n\nLine: {line}\n\n", res["version"].as_str().unwrap_or("?"));
    let mut failed = false;
    for job in res["jobs"].as_array().into_iter().flatten() {
        let tier = job["tier"].as_str().unwrap_or("?");
        let reqs = requests_of(job);
        let mut v = judge(tier, &reqs, names);
        if let Some(e) = job["error"].as_str() {
            v.failures.push(format!("the run failed: {e}"));
        }
        if let Some(e) = job["page"]["error"].as_str() {
            v.failures.push(format!("engine error: {e}"));
        }
        let families = device.map(|d| {
            let (fails, seen) = judge_device(d, &v.model_files);
            v.failures.extend(fails);
            format!("; {} textures seen: {}", d.label(), if seen.is_empty() { "none".to_string() } else { seen.join(", ") })
        });
        let families = families.unwrap_or_default();
        let verdict = if v.failures.is_empty() { "PASS" } else { "FAIL" };
        failed |= !v.failures.is_empty();
        let ready = v.ready_ms.map_or_else(|| "never".to_string(), |ms| format!("{:.1} s", ms as f64 / 1000.0));
        println!("netlog {tier}: {verdict}: {} requests, {} from models/: {}; model ready at {ready}{families}", reqs.len(), v.model_files.len(), v.model_files.join(", "));
        for f in &v.failures {
            println!("  FAIL {f}");
        }
        md.push_str(&format!("## {tier}: {verdict}\n\n{} requests in all; model ready (last `models/` file arrived) at {ready}{families}; from `models/`:\n\n", reqs.len()));
        for f in &v.model_files {
            md.push_str(&format!("- {f}\n"));
        }
        for f in &v.failures {
            md.push_str(&format!("\nFAIL: {f}\n"));
        }
        md.push('\n');
    }
    let path: PathBuf = out.join("netlog-report.md");
    std::fs::write(&path, md).map_err(|e| format!("netlog: {}: {e}", path.display()))?;
    println!("netlog: report in {}", path.display());
    if failed { Err("netlog: a tier downloaded the wrong files (see above)".into()) } else { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(url: &str, status: Option<&str>) -> Request {
        Request { url: url.into(), status: status.map(str::to_string), done_ms: status.map(|_| 100) }
    }

    #[test]
    fn ready_time_is_the_last_model_file() {
        let mut a = req("/models/hero.low.glb", Some("200"));
        a.done_ms = Some(2400);
        let mut b = req("/models/hero.low.base.jpg", Some("200"));
        b.done_ms = Some(3900);
        let mut page = req("/late.js", Some("200"));
        page.done_ms = Some(9000);
        assert_eq!(judge("low", &[a.clone(), b, page], &names()).ready_ms, Some(3900));
        let unfinished = req("/models/hero.low.base.jpg", None);
        assert_eq!(judge("low", &[a, unfinished], &names()).ready_ms, None);
    }

    fn names() -> Vec<String> {
        ["hero.glb", "hero.low.glb", "hero.base.jpg", "hero.low.base.jpg", "hero.base.bc7.ktx2", "prop.glb"].map(String::from).to_vec()
    }

    #[test]
    fn low_passes_with_its_own_and_shared_files() {
        let r = [
            req("http://127.0.0.1:8240/index.html", Some("200")),
            req("http://127.0.0.1:8240/models/stream.json", Some("200")),
            req("http://127.0.0.1:8240/models/hero.low.glb", Some("200")),
            req("http://127.0.0.1:8240/models/hero.low.base.jpg?x=1", Some("200")),
            req("http://127.0.0.1:8240/models/prop.glb", Some("206")),
        ];
        let v = judge("low", &r, &names());
        assert_eq!(v.failures, Vec::<String>::new());
        assert_eq!(v.model_files, ["hero.low.glb", "hero.low.base.jpg", "prop.glb"]);
    }

    #[test]
    fn another_tiers_file_fails() {
        let v = judge("low", &[req("/models/hero.ultra.glb", Some("200"))], &names());
        assert!(v.failures.iter().any(|f| f.contains("a file of the ultra tier")), "{:?}", v.failures);
        let v = judge("ultra", &[req("/models/hero.glb", Some("200")), req("/models/hero.low.base.jpg", Some("200"))], &names());
        assert_eq!(v.failures.len(), 1, "{:?}", v.failures);
        assert!(v.failures[0].contains("of the low tier"));
    }

    #[test]
    fn a_shared_file_the_tier_replaces_fails() {
        let v = judge("low", &[req("/models/hero.low.glb", Some("200")), req("/models/hero.glb", Some("200"))], &names());
        assert_eq!(v.failures.len(), 1, "{:?}", v.failures);
        assert!(v.failures[0].contains("shared hero.glb"));
        // Ultra has no copy of its own, so the shared file is right there.
        assert!(judge("ultra", &[req("/models/hero.glb", Some("200")), req("/models/hero.base.bc7.ktx2", Some("200"))], &names()).failures.is_empty());
    }

    #[test]
    fn missing_or_unfinished_files_and_no_model_fail() {
        let v = judge("low", &[req("/models/hero.low.glb", Some("404")), req("/models/hero.low.base.jpg", None)], &names());
        assert_eq!(v.failures.len(), 2, "{:?}", v.failures);
        let v = judge("low", &[req("/index.html", Some("200")), req("/models/stream.json", Some("200"))], &names());
        assert!(v.failures.iter().any(|f| f.contains("no model file")), "{:?}", v.failures);
    }

    fn files(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn phone_never_fetches_bc7() {
        let ok = files(&["hero.low.glb", "hero.low.base.astc.ktx2", "logo.png"]);
        let (fails, seen) = judge_device(Device::Phone, &ok);
        assert!(fails.is_empty(), "{fails:?}");
        assert_eq!(seen, ["astc", "rgba"]);
        // Forced to fetch the desktop's file, the run fails and names it.
        let forced = files(&["hero.low.glb", "hero.low.base.astc.ktx2", "hero.low.base.BC7.ktx2?v=2"]);
        let (fails, _) = judge_device(Device::Phone, &forced);
        assert_eq!(fails.len(), 1, "{fails:?}");
        assert!(fails[0].contains("hero.low.base.BC7.ktx2") && fails[0].contains("BC7"), "{fails:?}");
        // And the whole tier verdict carries it next to the LOD rule.
        let mut v = judge("low", &[req("/models/hero.low.glb", Some("200")), req("/models/hero.glb", Some("200"))], &names());
        v.failures.extend(judge_device(Device::Phone, &files(&["hero.base.bc7.ktx2"])).0);
        assert_eq!(v.failures.len(), 2, "{:?}", v.failures);
    }

    #[test]
    fn desktop_never_fetches_astc() {
        let (fails, seen) = judge_device(Device::Desktop, &files(&["hero.glb", "hero.base.bc7.ktx2", "hero.mr.bc7.ktx2"]));
        assert!(fails.is_empty(), "{fails:?}");
        assert_eq!(seen, ["bc7"]);
        let (fails, _) = judge_device(Device::Desktop, &files(&["hero.glb", "hero.base.bc7.ktx2", "hero.n.astc.ktx2"]));
        assert_eq!(fails.len(), 1, "{fails:?}");
        assert!(fails[0].contains("desktop fetched hero.n.astc.ktx2") && fails[0].contains("ASTC"), "{fails:?}");
        assert_eq!(Device::parse("phone"), Ok(Device::Phone));
        assert!(Device::parse("tablet").is_err());
    }
}
