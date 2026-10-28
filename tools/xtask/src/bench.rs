//! `cargo xtask bench`: build apps/bench with Trunk (release), serve the dist
//! on a free localhost port, let `tools/bench-runner/run.mjs` drive headless
//! Chrome over CDP (WebGPU), then print the table and exit non-zero if any
//! tier fails or the run errors.
//!
//! The tier limits are never read here: the page judges every tier against
//! the tier JSON through `evani_core::tiers::Tiers`. This side only needs
//! the tier list and each tier's target resolution (for the viewport).

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use evani_bench::{Report, TierResult, format_stress_table, format_table, mean_results, tier_order_failures};
use evani_core::tiers::Tiers;
use serde_json::{Value, json};

/// Viewport for tiers whose JSON gives no target resolution.
const DEFAULT_VIEWPORT: [u32; 2] = [1920, 1080];

#[derive(Debug, Default, PartialEq)]
pub struct BenchArgs {
    pub tiers: Vec<String>,
    pub no_build: bool,
    pub frames: Option<u32>,
    pub warmup: Option<u32>,
    pub chrome: Option<String>,
    pub timeout_s: Option<u32>,
    /// Save a picture of each phase (target/evani-bench/shots).
    pub shots: bool,
    /// Run the whole bench this many times (a fresh browser each time) and
    /// judge the mean (default 1).
    pub runs: Option<u32>,
    /// Force this MSAA count on every tier (1 or 4).
    pub msaa: Option<u32>,
    /// Effects switched off for the run.
    pub off: Vec<String>,
    /// The stress scene (`?stress=1`): the same GPU-heavy content on Low to
    /// Ultra, 5 runs unless `--runs` says otherwise, results in
    /// `target/evani-bench/stress[-off-x]/`.
    pub stress: bool,
    /// Fill layers of the stress scene (tuning on another GPU).
    pub fill: Option<u32>,
}

/// Runs of the stress scene unless `--runs` says otherwise.
pub const STRESS_RUNS: u32 = 5;

impl BenchArgs {
    pub fn parse(args: &[String]) -> Result<Self, String> {
        let mut a = Self::default();
        let mut it = args.iter();
        let value = |name: &str, it: &mut std::slice::Iter<String>| {
            it.next().cloned().ok_or_else(|| format!("{name} needs a value"))
        };
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "--no-build" => a.no_build = true,
                "--shots" => a.shots = true,
                "--stress" => a.stress = true,
                "--tier" => {
                    a.tiers = value("--tier", &mut it)?
                        .split(',')
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .collect();
                }
                "--frames" => a.frames = Some(num("--frames", &value("--frames", &mut it)?)?),
                "--warmup" => a.warmup = Some(num("--warmup", &value("--warmup", &mut it)?)?),
                "--timeout-s" => a.timeout_s = Some(num("--timeout-s", &value("--timeout-s", &mut it)?)?),
                "--chrome" => a.chrome = Some(value("--chrome", &mut it)?),
                "--fill" => a.fill = Some(num("--fill", &value("--fill", &mut it)?)?),
                "--runs" => a.runs = Some(num("--runs", &value("--runs", &mut it)?)?.max(1)),
                "--msaa" => {
                    let n = num("--msaa", &value("--msaa", &mut it)?)?;
                    if n != 1 && n != 4 {
                        return Err("--msaa takes 1 or 4".into());
                    }
                    a.msaa = Some(n);
                }
                "--off" => {
                    a.off = value("--off", &mut it)?
                        .split(',')
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .collect();
                }
                other => return Err(format!("unknown bench option {other}\n{}", usage())),
            }
        }
        Ok(a)
    }
}

fn num(name: &str, v: &str) -> Result<u32, String> {
    v.parse().map_err(|_| format!("{name}: '{v}' is not a number"))
}

pub fn usage() -> &'static str {
    "  cargo xtask bench [--stress [--fill N]] [--tier low,high] [--runs N] [--frames N] [--warmup N] [--msaa 1|4] [--off bloom,outline] [--chrome <path>] [--timeout-s N] [--shots] [--no-build]"
}

/// Where a stress run keeps its files, under `target/evani-bench/`:
/// `stress`, or `stress-off-bloom`, `stress-msaa-1` for variants.
pub fn stress_dir_name(a: &BenchArgs) -> String {
    let mut name = "stress".to_string();
    if !a.off.is_empty() {
        name.push_str("-off-");
        name.push_str(&a.off.join("-"));
    }
    if let Some(n) = a.msaa {
        name.push_str(&format!("-msaa-{n}"));
    }
    if let Some(n) = a.fill {
        name.push_str(&format!("-fill-{n}"));
    }
    name
}

/// `nvidia-smi` logging the graphics clock and GPU use every 500 ms while
/// the stress bench runs (best effort: nothing happens without it).
struct ClockLog {
    child: std::process::Child,
    path: PathBuf,
}

impl ClockLog {
    fn start(path: PathBuf) -> Option<Self> {
        let file = std::fs::File::create(&path).ok()?;
        let child = Command::new("nvidia-smi")
            .args(["--query-gpu=timestamp,clocks.gr,utilization.gpu", "--format=csv,noheader,nounits", "-lms", "500"])
            .stdout(file)
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        Some(Self { child, path })
    }

    /// Stop logging and summarise.
    fn finish(mut self) -> Option<ClockSummary> {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let text = std::fs::read_to_string(&self.path).ok()?;
        Some(clock_summary(&text))
    }
}

impl Drop for ClockLog {
    fn drop(&mut self) {
        // Never leave nvidia-smi running (an error return skips `finish`).
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The graphics clock over a run: all samples, and the samples where the
/// GPU was busy (use at or over [`BUSY_PCT`]), which is when the bench
/// measures; page loads between tiers idle the GPU and are left out there.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClockSummary {
    pub samples: usize,
    pub busy_samples: usize,
    /// MHz while busy: lowest, median, highest.
    pub busy_mhz: Option<[u32; 3]>,
    /// GPU use % while busy: median.
    pub busy_util_p50: Option<u32>,
    /// MHz over every sample: lowest and highest.
    pub all_mhz: Option<[u32; 2]>,
}

impl ClockSummary {
    pub fn to_json(&self) -> Value {
        json!({
            "samples": self.samples,
            "busy_samples": self.busy_samples,
            "busy_pct_from": BUSY_PCT,
            "busy_mhz_min_p50_max": self.busy_mhz,
            "busy_util_p50": self.busy_util_p50,
            "all_mhz_min_max": self.all_mhz,
        })
    }

    pub fn line(&self) -> String {
        match (self.busy_mhz, self.busy_util_p50) {
            (Some([lo, mid, hi]), Some(u)) => format!(
                "GPU clock while busy (use >= {BUSY_PCT}%, {} of {} samples at 500 ms): {lo}-{hi} MHz, median {mid} MHz, median use {u}%{}",
                self.busy_samples,
                self.samples,
                self.all_mhz.map_or(String::new(), |[a, b]| format!("; whole run {a}-{b} MHz"))
            ),
            _ => format!("GPU clock: no busy samples in {} (nvidia-smi)", self.samples),
        }
    }
}

/// GPU use (%) from which a sample counts as "busy".
/// (A page loading between tiers idles at under 20 %; the cheapest tier
/// of the stress scene runs at about 50-60 % under the 60 Hz cap.)
pub const BUSY_PCT: u32 = 40;

/// Summarise `nvidia-smi --format=csv,noheader,nounits` lines of
/// `timestamp, clock MHz, use %`.
pub fn clock_summary(csv: &str) -> ClockSummary {
    let rows: Vec<(u32, u32)> = csv
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split(',').map(str::trim).collect();
            Some((f.get(1)?.parse().ok()?, f.get(2)?.parse().ok()?))
        })
        .collect();
    let mut busy: Vec<u32> = rows.iter().filter(|r| r.1 >= BUSY_PCT).map(|r| r.0).collect();
    let mut util: Vec<u32> = rows.iter().filter(|r| r.1 >= BUSY_PCT).map(|r| r.1).collect();
    busy.sort_unstable();
    util.sort_unstable();
    ClockSummary {
        samples: rows.len(),
        busy_samples: busy.len(),
        busy_mhz: (!busy.is_empty()).then(|| [busy[0], busy[busy.len() / 2], busy[busy.len() - 1]]),
        busy_util_p50: (!util.is_empty()).then(|| util[util.len() / 2]),
        all_mhz: rows
            .iter()
            .map(|r| r.0)
            .min()
            .zip(rows.iter().map(|r| r.0).max())
            .map(|(a, b)| [a, b]),
    }
}

/// One runner job per tier: a fresh page (and GPU device) at the tier's size.
pub fn jobs_for(tiers: &Tiers, wanted: &[String], query: &str) -> Result<Vec<Value>, String> {
    let ids: Vec<String> = if wanted.is_empty() {
        tiers.presets().iter().map(|p| p.id.clone()).collect()
    } else {
        wanted.to_vec()
    };
    ids.iter()
        .map(|id| {
            let t = tiers.get(id).ok_or_else(|| format!("unknown tier '{id}'"))?;
            let [w, h] = t.target_resolution.unwrap_or(DEFAULT_VIEWPORT);
            Ok(json!({ "tier": id, "width": w, "height": h, "dpr": 1, "query": query }))
        })
        .collect()
}

fn root() -> PathBuf {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    here.ancestors().nth(2).map_or(here.clone(), Path::to_path_buf)
}

fn trunk_build(dist: &Path) -> Result<(), String> {
    eprintln!("  building the bench (trunk build --release)");
    let mut cmd = Command::new("trunk");
    cmd.env_remove("NO_COLOR"); // trunk refuses NO_COLOR=1
    cmd.current_dir(root().join("apps").join("bench"))
        .args(["build", "--release", "--dist"])
        .arg(dist);
    if std::env::var_os("CARGO_BUILD_JOBS").is_none() {
        cmd.env("CARGO_BUILD_JOBS", "4");
    }
    let out = cmd
        .output()
        .map_err(|e| format!("cannot run trunk ({e}); install it with `cargo install trunk --locked`"))?;
    if !out.status.success() {
        let log = String::from_utf8_lossy(&out.stderr);
        let tail: Vec<&str> = log.lines().rev().take(25).collect::<Vec<_>>().into_iter().rev().collect();
        return Err(format!("trunk build failed:\n{}", tail.join("\n")));
    }
    Ok(())
}

// ---- a tiny static file server ------------------------------------------------------

pub(crate) fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        _ => "application/octet-stream",
    }
}

/// Map a request target to a file inside `dist` (None for anything that
/// would leave the folder).
pub fn resolve(dist: &Path, target: &str) -> Option<PathBuf> {
    let path = target.split(['?', '#']).next().unwrap_or("/");
    let rel = path.trim_start_matches('/');
    let rel = if rel.is_empty() { "index.html" } else { rel };
    if rel.split(['/', '\\']).any(|p| p == ".." || p.contains(':')) {
        return None;
    }
    Some(dist.join(rel))
}

fn respond(mut s: TcpStream, dist: &Path) {
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buf = [0u8; 4096];
    let mut got = Vec::new();
    while !got.windows(4).any(|w| w == b"\r\n\r\n") && got.len() < 16 * 1024 {
        match s.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => got.extend_from_slice(&buf[..n]),
        }
    }
    let head = String::from_utf8_lossy(&got);
    let mut parts = head.lines().next().unwrap_or("").split(' ');
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/"));
    let file = resolve(dist, target).and_then(|p| std::fs::read(&p).ok().map(|b| (p, b)));
    let (status, ctype, body) = match (method, file) {
        ("GET" | "HEAD", Some((p, b))) => ("200 OK", content_type(&p), b),
        (_, _) => ("404 Not Found", "text/plain", b"not found".to_vec()),
    };
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = s.write_all(header.as_bytes());
    if method != "HEAD" {
        let _ = s.write_all(&body);
    }
}

/// Serve `dist` on a free localhost port. Dropping the handle stops it.
pub struct Server {
    pub port: u16,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Server {
    pub fn start(dist: PathBuf) -> Result<Self, String> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("cannot bind a port: {e}"))?;
        Self::serve(listener, dist)
    }

    /// Serve on the first free port of `ports`.
    pub fn start_in(dist: PathBuf, ports: std::ops::RangeInclusive<u16>) -> Result<Self, String> {
        let (lo, hi) = (*ports.start(), *ports.end());
        let listener = ports
            .into_iter()
            .find_map(|p| TcpListener::bind(("127.0.0.1", p)).ok())
            .ok_or_else(|| format!("no free port in {lo}-{hi}"))?;
        Self::serve(listener, dist)
    }

    fn serve(listener: TcpListener, dist: PathBuf) -> Result<Self, String> {
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let dist = Arc::new(dist);
        let thread = std::thread::spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((s, _)) => {
                        let _ = s.set_nonblocking(false);
                        let d = dist.clone();
                        std::thread::spawn(move || respond(s, &d));
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(10)),
                }
            }
        });
        Ok(Self {
            port,
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

// ---- the run ------------------------------------------------------------------------

/// What came back from the runner, ready to print.
#[derive(Debug, Default)]
pub struct Collected {
    pub results: Vec<TierResult>,
    /// Runner or page errors, one per tier that did not finish.
    pub errors: Vec<String>,
}

/// Turn the runner's JSON into results and errors.
pub fn collect(runner_output: &Value) -> Collected {
    let mut c = Collected::default();
    let Some(jobs) = runner_output.get("jobs").and_then(Value::as_array) else {
        c.errors.push("the runner returned no jobs".into());
        return c;
    };
    for job in jobs {
        let tier = job.get("tier").and_then(Value::as_str).unwrap_or("?");
        let console: Vec<&str> = job
            .get("console")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        if let Some(e) = job.get("error").and_then(Value::as_str) {
            c.errors.push(format!("{tier}: {e}"));
        }
        match job.get("bench").filter(|b| !b.is_null()).map(|b| serde_json::from_value::<Report>(b.clone())) {
            Some(Ok(report)) => {
                if let Some(e) = &report.error {
                    c.errors.push(format!("{tier}: {e}"));
                }
                if !report.done {
                    c.errors.push(format!("{tier}: the bench did not finish"));
                }
                c.results.extend(report.results);
            }
            Some(Err(e)) => c.errors.push(format!("{tier}: unreadable result ({e})")),
            None => {
                if job.get("error").is_none_or(Value::is_null) {
                    c.errors.push(format!("{tier}: no result from the page"));
                }
            }
        }
        if !console.is_empty() && job.get("error").is_some_and(|e| !e.is_null()) {
            for line in console.iter().take(5) {
                c.errors.push(format!("{tier}: page said: {line}"));
            }
        }
    }
    c
}

pub fn cmd_bench(args: &[String]) -> Result<(), String> {
    let a = BenchArgs::parse(args)?;
    let tiers = Tiers::builtin()?;
    let mut query = String::new();
    // Phases must last long enough to catch with a picture.
    if let Some(n) = a.frames.or(a.shots.then_some(1500)) {
        query.push_str(&format!("frames={n}"));
    }
    let mut add = |kv: String| {
        if !query.is_empty() {
            query.push('&');
        }
        query.push_str(&kv);
    };
    if let Some(n) = a.warmup {
        add(format!("warmup={n}"));
    }
    if let Some(n) = a.msaa {
        add(format!("msaa={n}"));
    }
    if !a.off.is_empty() {
        add(format!("off={}", a.off.join(",")));
    }
    if a.stress {
        add("stress=1".to_string());
    }
    if let Some(n) = a.fill {
        add(format!("fill={n}"));
    }
    // The stress scene measures the cost order: Lite to Ultra by default.
    let wanted: Vec<String> = if a.stress && a.tiers.is_empty() {
        evani_bench::check::COST_ORDER.iter().map(|s| (*s).to_string()).collect()
    } else {
        a.tiers.clone()
    };
    let jobs = jobs_for(&tiers, &wanted, &query)?;
    let out_dir = if a.stress {
        root().join("target").join("evani-bench").join(stress_dir_name(&a))
    } else {
        root().join("target").join("evani-bench")
    };
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("cannot make {}: {e}", out_dir.display()))?;

    let dist = root().join("target").join("evani-bench").join("dist");
    if a.no_build && dist.join("index.html").exists() {
        eprintln!("  using the existing build in {}", dist.display());
    } else {
        trunk_build(&dist)?;
    }
    let server = Server::start(dist)?;
    let url = format!("http://127.0.0.1:{}/", server.port);
    eprintln!("  serving the bench at {url}");

    let runner = root().join("tools").join("bench-runner").join("run.mjs");
    let runs = a.runs.unwrap_or(if a.stress { STRESS_RUNS } else { 1 });
    let clock = if a.stress { ClockLog::start(out_dir.join("gpu-clock.csv")) } else { None };
    let mut all = Vec::new();
    let mut errors = Vec::new();
    let mut last_json = Value::Null;
    for run in 1..=runs {
        if runs > 1 {
            eprintln!("  run {run} of {runs}");
        }
        let mut cmd = Command::new("node");
        cmd.arg(&runner)
            .args(["--url", &url, "--jobs", &Value::Array(jobs.clone()).to_string()])
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        if let Some(c) = &a.chrome {
            cmd.args(["--chrome", c]);
        }
        // The stress scene keeps Chrome's 60 Hz cap: uncapped, the page floods
        // the GPU process and timestamp readbacks come back a few times a
        // second (5 readings in 600 frames); capped, every frame is read.
        // The scene itself keeps the GPU busy.
        if a.stress {
            cmd.args(["--capped", "1"]);
        }
        if a.shots {
            let dir = root().join("target").join("evani-bench").join("shots");
            cmd.args(["--shots", &dir.to_string_lossy()]);
        }
        if let Some(s) = a.timeout_s {
            cmd.args(["--timeout-ms", &(u64::from(s) * 1000).to_string()]);
        }
        let out = cmd
            .output()
            .map_err(|e| format!("cannot run node ({e}); the bench runner needs Node 22 or newer"))?;
        if !out.status.success() {
            return Err("the bench runner failed (see the lines above)".into());
        }
        let json: Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("unreadable runner output: {e}"))?;
        let name = if runs > 1 || a.stress { format!("results-{run}.json") } else { "results.json".to_string() };
        let saved = out_dir.join(name);
        let _ = std::fs::write(&saved, serde_json::to_vec_pretty(&json).unwrap_or_default());
        let c = collect(&json);
        if runs > 1 {
            print_report(&json, &c, &format!("run {run} of {runs}"));
        }
        errors.extend(c.errors.iter().map(|e| format!("run {run}: {e}")));
        all.push(c.results);
        last_json = json;
    }
    drop(server);
    let clocks = clock.and_then(ClockLog::finish);

    let c = Collected {
        results: mean_results(&all),
        errors,
    };
    let title = if runs > 1 { format!("mean of {runs} runs") } else { String::new() };
    print_report(&last_json, &c, &title);
    if a.stress {
        return finish_stress(&a, &query, runs, &out_dir, &last_json, &c, clocks.as_ref());
    }
    // The GPU cost order (Lite < Low < Medium < High < Ultra), on the median GPU ms
    // per frame (median of the runs): robust to clock state and spikes.
    let order = tier_order_failures(&c.results);
    let medians: Vec<String> = evani_bench::check::COST_ORDER
        .iter()
        .filter_map(|id| c.results.iter().find(|r| r.tier == *id))
        .filter_map(|r| r.gpu_p50_ms.map(|g| format!("{} {g:.2}", r.tier)))
        .collect();
    if !medians.is_empty() {
        println!("gpu median ms per frame (judged by the order check): {}", medians.join(", "));
    }
    if order.is_empty() {
        if c.results.iter().filter(|r| r.gpu_ms.is_some()).count() >= 2 {
            println!("GPU cost order: holds (each tier within {:.0}% of or above the one below)", evani_bench::check::ORDER_TOLERANCE_PCT);
        }
    } else {
        for f in &order {
            println!("FAIL {f}");
        }
    }
    let failed = c.results.iter().filter(|r| !r.pass).count();
    if !c.errors.is_empty() {
        Err(format!("{} run error(s)", c.errors.len()))
    } else if failed > 0 {
        Err(format!("{failed} of {} tiers failed", c.results.len()))
    } else if !order.is_empty() {
        Err("the GPU cost order does not hold".into())
    } else {
        Ok(())
    }
}

/// The stress run's table, order check and files: `summary.json` (the
/// median-of-runs results, the order check and the GPU clock) and
/// `table.txt` next to the per-run `results-N.json`.
fn finish_stress(
    a: &BenchArgs,
    query: &str,
    runs: u32,
    out_dir: &Path,
    last_json: &Value,
    c: &Collected,
    clocks: Option<&ClockSummary>,
) -> Result<(), String> {
    let mut table = format_stress_table(&c.results);
    match clocks {
        Some(k) => table.push_str(&format!("{}\n", k.line())),
        None => table.push_str("GPU clock: not logged (no nvidia-smi)\n"),
    }
    println!();
    print!("{table}");
    let order = tier_order_failures(&c.results);
    let summary = json!({
        "stress": true,
        "runs": runs,
        "query": query,
        "off": a.off,
        "msaa": a.msaa,
        "version": last_json.get("version"),
        "adapter": last_json.get("adapter"),
        "results": serde_json::to_value(&c.results).unwrap_or(Value::Null),
        "order_failures": order,
        "gpu_clock": clocks.map(ClockSummary::to_json),
        "errors": c.errors,
    });
    let _ = std::fs::write(out_dir.join("summary.json"), serde_json::to_vec_pretty(&summary).unwrap_or_default());
    let _ = std::fs::write(out_dir.join("table.txt"), &table);
    eprintln!("  saved {}", out_dir.join("summary.json").display());
    let failed = c.results.iter().filter(|r| !r.pass).count();
    if !c.errors.is_empty() {
        Err(format!("{} run error(s)", c.errors.len()))
    } else if failed > 0 {
        Err(format!("{failed} of {} tiers did not run the stress scene cleanly", c.results.len()))
    } else if !order.is_empty() {
        Err("the GPU cost order does not hold on the stress scene".into())
    } else {
        Ok(())
    }
}

/// The saved stress summary's order inputs: `(tier, median GPU ms)` for
/// the [`evani_bench::check::COST_ORDER`] tiers it has, plus the number of
/// runs. `None` when there is no usable summary.
pub fn stress_order_inputs(summary: &Value) -> Option<(Vec<(String, f32)>, u64)> {
    let results: Vec<TierResult> = serde_json::from_value(summary.get("results")?.clone()).ok()?;
    let runs = summary.get("runs").and_then(Value::as_u64).unwrap_or(1);
    let v: Vec<(String, f32)> = evani_bench::check::COST_ORDER
        .iter()
        .filter_map(|id| results.iter().find(|r| r.tier == *id))
        .filter_map(|r| r.gpu_p50_ms.map(|g| (r.tier.clone(), g)))
        .collect();
    (!v.is_empty()).then_some((v, runs))
}

fn print_report(json: &Value, c: &Collected, title: &str) {
    let text = |k: &str| json.get(k).and_then(Value::as_str).unwrap_or("?").to_string();
    println!();
    println!(
        "Evani.Dev bench: {} (headless, WebGPU){}",
        text("version"),
        if title.is_empty() { String::new() } else { format!(", {title}") }
    );
    if let Some(a) = json.get("adapter").filter(|a| !a.is_null()) {
        let s = |k: &str| a.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        println!(
            "GPU: {} {} {} (timestamp-query {})",
            s("vendor"),
            s("architecture"),
            s("description"),
            if a.get("features").and_then(Value::as_bool).unwrap_or(false) { "available" } else { "unavailable" }
        );
    }
    let sizes: Vec<String> = c
        .results
        .iter()
        .filter_map(|r| r.render_size.map(|[w, h]| format!("{} {w}x{h}", r.tier)))
        .collect();
    if !sizes.is_empty() {
        println!("render size: {}", sizes.join(", "));
    }
    println!();
    print!("{}", format_table(&c.results));
    for e in &c.errors {
        println!("ERROR {e}");
    }
    let passed = c.results.iter().filter(|r| r.pass).count();
    println!(
        "\n{} of {} tiers passed{}",
        passed,
        c.results.len(),
        if c.errors.is_empty() { "" } else { " (the run had errors)" }
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn parses_options() {
        let a = BenchArgs::parse(&s(&["--tier", "low,vr", "--frames", "10", "--no-build"])).unwrap();
        assert_eq!(a.tiers, ["low", "vr"]);
        assert_eq!(a.frames, Some(10));
        assert!(a.no_build);
        assert!(BenchArgs::parse(&s(&["--frames"])).is_err());
        assert!(BenchArgs::parse(&s(&["--frames", "x"])).is_err());
        assert!(BenchArgs::parse(&s(&["--nope"])).is_err());
        let a = BenchArgs::parse(&s(&["--stress", "--off", "bloom", "--tier", "high"])).unwrap();
        assert!(a.stress);
        assert_eq!(stress_dir_name(&a), "stress-off-bloom");
        let a = BenchArgs::parse(&s(&["--stress", "--msaa", "1"])).unwrap();
        assert_eq!(stress_dir_name(&a), "stress-msaa-1");
        assert_eq!(stress_dir_name(&BenchArgs { stress: true, ..BenchArgs::default() }), "stress");
    }

    #[test]
    fn clock_log_is_summarised_over_busy_samples() {
        let csv = "2026/10/14 10:00:00.000, 210, 0\n\
                   2026/10/14 10:00:00.500, 2520, 97\n\
                   2026/10/14 10:00:01.000, 2610, 99\n\
                   garbage line\n\
                   2026/10/14 10:00:01.500, 2580, 88\n\
                   2026/10/14 10:00:02.000, 1200, 20\n";
        let k = clock_summary(csv);
        assert_eq!((k.samples, k.busy_samples), (5, 3));
        assert_eq!(k.busy_mhz, Some([2520, 2580, 2610]));
        assert_eq!(k.busy_util_p50, Some(97));
        assert_eq!(k.all_mhz, Some([210, 2610]));
        assert!(k.line().contains("2520-2610 MHz, median 2580 MHz"), "{}", k.line());
        assert_eq!(clock_summary("").busy_mhz, None);
    }

    #[test]
    fn stress_summary_gives_the_order_inputs() {
        let t = |id: &str, p50: Option<f32>| TierResult { tier: id.into(), gpu_p50_ms: p50, ..TierResult::default() };
        let results = vec![t("ultra", Some(4.0)), t("low", Some(1.0)), t("medium", None), t("vr", Some(0.5))];
        let summary = json!({ "runs": 5, "results": serde_json::to_value(&results).unwrap() });
        let (v, runs) = stress_order_inputs(&summary).unwrap();
        assert_eq!(runs, 5);
        assert_eq!(v, [("low".to_string(), 1.0), ("ultra".to_string(), 4.0)]);
        assert!(stress_order_inputs(&json!({})).is_none());
    }

    #[test]
    fn jobs_use_the_tier_json_for_size() {
        let t = Tiers::builtin().unwrap();
        let jobs = jobs_for(&t, &[], "").unwrap();
        let ids: Vec<&str> = jobs.iter().filter_map(|j| j["tier"].as_str()).collect();
        assert_eq!(ids, ["lite", "low", "medium", "high", "ultra", "vr"]);
        let ultra = jobs.iter().find(|j| j["tier"] == "ultra").unwrap();
        assert_eq!((ultra["width"].as_u64(), ultra["height"].as_u64()), (Some(2560), Some(1440)));
        let low = jobs.iter().find(|j| j["tier"] == "low").unwrap();
        assert_eq!(low["width"].as_u64(), Some(1920));
        assert!(jobs_for(&t, &s(&["potato"]), "").is_err());
    }

    #[test]
    fn server_paths_stay_inside_dist() {
        let d = Path::new("dist");
        assert_eq!(resolve(d, "/"), Some(d.join("index.html")));
        assert_eq!(resolve(d, "/a.wasm?x=1"), Some(d.join("a.wasm")));
        assert_eq!(resolve(d, "/../secret"), None);
        assert_eq!(resolve(d, "/C:/x"), None);
    }

    #[test]
    fn server_serves_files_on_a_free_port() {
        let dir = std::env::temp_dir().join(format!("evani-bench-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.html"), "hello").unwrap();
        let server = Server::start(dir.clone()).unwrap();
        let mut c = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
        c.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        let mut got = String::new();
        c.read_to_string(&mut got).unwrap();
        assert!(got.starts_with("HTTP/1.1 200"), "{got}");
        assert!(got.ends_with("hello"));
        let mut c = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
        c.write_all(b"GET /nope HTTP/1.1\r\n\r\n").unwrap();
        let mut got = String::new();
        c.read_to_string(&mut got).unwrap();
        assert!(got.starts_with("HTTP/1.1 404"), "{got}");
        drop(server);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn collects_results_and_errors() {
        let good = json!({ "jobs": [
            { "tier": "low", "error": null, "bench": { "done": true, "error": null, "mode": "browser-gpu", "progress": "done", "results": [] } },
            { "tier": "high", "error": "timed out after 5 s", "bench": null },
            { "tier": "vr", "error": null, "bench": { "done": false, "error": "WebGPU did not come up", "mode": "browser-gpu", "progress": "", "results": [] } },
        ]});
        let c = collect(&good);
        assert!(c.results.is_empty());
        assert_eq!(c.errors.len(), 3, "{:?}", c.errors);
        assert!(c.errors.iter().any(|e| e.contains("timed out")));
        assert!(collect(&json!({})).errors.len() == 1);
    }
}
