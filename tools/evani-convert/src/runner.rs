//! The quality gate's renderer, seen from the converter (STAGE3_PLAN S3-6):
//! builds apps/model-view's gate page once (trunk, feature `gate`), starts
//! tools/convert-runner/run.mjs (one headless, muted Chrome at below-normal
//! priority) and sends it one job per model and tier: the camera poses go
//! out as a JSON line, the pictures come back as raw RGBA files.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::SystemTime;

use serde_json::{Value, json};

use crate::metric::Rgba;
use crate::tools::{low, s};

/// One camera pose (evani-core's `CameraPose`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub eye: [f64; 3],
    pub target: [f64; 3],
    pub up: [f64; 3],
    pub fov_y_deg: f64,
}

impl Pose {
    pub fn to_json(&self) -> Value {
        json!({ "eye": self.eye, "target": self.target, "up": self.up, "fov_y_deg": self.fov_y_deg })
    }
}

/// One render: a model under the `--serve` folder, placed by `place`
/// (`[x, y, z, uniform scale]`), seen through every pose.
#[derive(Clone, Debug)]
pub struct Job {
    pub tier: String,
    pub width: u32,
    pub height: u32,
    /// The model's path under the served folder (`/m/<path>` on the page).
    pub model: String,
    pub place: [f64; 4],
    pub views: Vec<Pose>,
    /// Where the runner writes `view<i>.rgba`.
    pub out: PathBuf,
}

/// The built gate page, where the converter keeps it.
pub fn gate_dist_dir(root: &Path) -> PathBuf {
    let target = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| root.join("target"));
    target.join("evani-gate").join("model-view")
}

fn newest(dir: &Path, best: &mut SystemTime) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || name == "target" || name == "dist" || name == "node_modules" {
            continue;
        }
        if p.is_dir() {
            newest(&p, best);
        } else if matches!(p.extension().and_then(|x| x.to_str()), Some("rs" | "toml" | "wgsl" | "html" | "json" | "js" | "lock"))
            && let Ok(m) = e.metadata().and_then(|m| m.modified())
            && m > *best
        {
            *best = m;
        }
    }
}

/// The gate page's sources: the app, its kit and look, and the engine crates.
fn sources_newest(root: &Path) -> SystemTime {
    let mut t = SystemTime::UNIX_EPOCH;
    for d in ["apps/model-view", "kits/evani-kit-model-view", "looks/evani-look-toon", "crates"] {
        newest(&root.join(d), &mut t);
    }
    if let Ok(m) = std::fs::metadata(root.join("Cargo.lock")).and_then(|m| m.modified()) {
        t = t.max(m);
    }
    t
}

/// Build the gate page when it is missing or older than its sources
/// (`EVANI_GATE_DIST` names a ready build instead). About a minute the
/// first time; skipped after that.
pub fn gate_dist(root: &Path) -> Result<PathBuf, String> {
    if let Some(d) = std::env::var_os("EVANI_GATE_DIST").map(PathBuf::from) {
        return if d.join("index.html").is_file() { Ok(d) } else { Err(format!("EVANI_GATE_DIST={}: no index.html there", d.display())) };
    }
    let dist = gate_dist_dir(root);
    let stamp = dist.join(".evani-gate.stamp");
    let built = std::fs::metadata(&stamp).and_then(|m| m.modified()).ok();
    if built.is_some_and(|b| b >= sources_newest(root)) && dist.join("index.html").is_file() {
        return Ok(dist);
    }
    eprintln!("  building the quality gate's page (apps/model-view, feature gate; trunk build --release)");
    let mut cmd = Command::new("trunk");
    cmd.env_remove("NO_COLOR"); // trunk refuses NO_COLOR=1
    cmd.current_dir(root.join("apps").join("model-view"))
        .args(["build", "--release", "--no-default-features", "--features", "outline,shadows,gate", "--public-url", "./", "--dist"])
        .arg(&dist);
    if std::env::var_os("CARGO_BUILD_JOBS").is_none() {
        cmd.env("CARGO_BUILD_JOBS", "2");
    }
    low(&mut cmd);
    let out = cmd.output().map_err(|e| format!("cannot run trunk ({e}); install it with `cargo install trunk --locked`"))?;
    if !out.status.success() {
        let log = String::from_utf8_lossy(&out.stderr);
        let tail: Vec<&str> = log.lines().rev().take(25).collect::<Vec<_>>().into_iter().rev().collect();
        return Err(format!("trunk build of the gate page failed:\n{}", tail.join("\n")));
    }
    std::fs::write(&stamp, b"built by evani-convert\n").map_err(|e| format!("{}: {e}", stamp.display()))?;
    Ok(dist)
}

/// A running convert-runner (and its Chrome). Dropping it closes the
/// runner's input, which stops Chrome and the server, and waits.
pub struct Runner {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    /// The folder served under `/m/`.
    pub serve: PathBuf,
    /// Chrome's path and version, for the report.
    pub chrome: String,
    next: u64,
}

impl Runner {
    /// Start the runner on the gate page `dist`, serving `serve` under `/m/`.
    pub fn start(root: &Path, node: &Path, dist: &Path, serve: &Path) -> Result<Runner, String> {
        let script = root.join("tools").join("convert-runner").join("run.mjs");
        if !script.is_file() {
            return Err(format!("{}: missing", script.display()));
        }
        let mut cmd = Command::new(node);
        cmd.arg(&script).arg("--dist").arg(dist).arg("--serve").arg(serve).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit());
        low(&mut cmd);
        let mut child = cmd.spawn().map_err(|e| format!("cannot start node {} ({e})", script.display()))?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().ok_or("the runner has no output")?;
        let mut r = Runner { child, stdin, stdout: BufReader::new(stdout), serve: serve.to_path_buf(), chrome: String::new(), next: 0 };
        let ready = r.line()?;
        if ready["ready"].as_bool() != Some(true) {
            return Err(format!("the quality gate's runner did not start: {}", ready["error"].as_str().unwrap_or("no reason given")));
        }
        r.chrome = format!("{} {}", ready["chrome"].as_str().unwrap_or("chrome"), ready["version"].as_str().unwrap_or(""));
        Ok(r)
    }

    fn line(&mut self) -> Result<Value, String> {
        let mut l = String::new();
        loop {
            l.clear();
            let n = self.stdout.read_line(&mut l).map_err(|e| format!("the runner's output: {e}"))?;
            if n == 0 {
                return Err("the quality gate's runner stopped".into());
            }
            if l.trim_start().starts_with('{') {
                return serde_json::from_str(l.trim()).map_err(|e| format!("the runner's output: {e}: {l}"));
            }
        }
    }

    /// Render one job: one picture per pose.
    pub fn render(&mut self, job: &Job) -> Result<Vec<Rgba>, String> {
        std::fs::create_dir_all(&job.out).map_err(|e| format!("{}: {e}", job.out.display()))?;
        for i in 0..job.views.len() {
            let _ = std::fs::remove_file(job.out.join(format!("view{i}.rgba")));
        }
        self.next += 1;
        let id = format!("j{}", self.next);
        let line = json!({
            "id": id,
            "tier": job.tier,
            "width": job.width,
            "height": job.height,
            "model": format!("/m/{}", job.model.trim_start_matches('/')),
            "place": job.place,
            "views": job.views.iter().map(Pose::to_json).collect::<Vec<_>>(),
            "out": s(&job.out),
        });
        let stdin = self.stdin.as_mut().ok_or("the runner is closed")?;
        writeln!(stdin, "{line}").and_then(|()| stdin.flush()).map_err(|e| format!("the runner's input: {e}"))?;
        let r = self.line()?;
        if r["ok"].as_bool() != Some(true) {
            let console: Vec<&str> = r["console"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            return Err(format!("render {} ({}): {}{}", job.model, job.tier, r["error"].as_str().unwrap_or("failed"), if console.is_empty() { String::new() } else { format!("\n  {}", console.join("\n  ")) }));
        }
        let (w, h) = (r["w"].as_u64().unwrap_or(0) as usize, r["h"].as_u64().unwrap_or(0) as usize);
        let mut out = Vec::new();
        for i in 0..job.views.len() {
            let p = job.out.join(format!("view{i}.rgba"));
            let px = std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            out.push(Rgba::new(w, h, px).map_err(|e| format!("{}: {e}", p.display()))?);
        }
        Ok(out)
    }
}

impl Drop for Runner {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.wait();
    }
}
