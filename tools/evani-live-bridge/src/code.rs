//! Live code swapping (docs/LIVE_API.md 10.4): `serve` builds the game with
//! a pinned `dx` (Dioxus CLI, git main) instead of Trunk, so a saved Rust
//! change is compiled into a small patch and swapped into the running page
//! between two frames, with no reload.
//!
//! * dx runs as a child at below-normal priority with capped jobs, on a
//!   private port, into `target/evani-live/dx`. The page never talks to it:
//!   the bridge serves dx's build (and its patch files) from its own origin.
//! * The bridge listens on dx's devserver socket (`/_dioxus`) and turns its
//!   messages into the live relay's additive `code_*` messages (hub.rs).
//! * Compile errors come from dx's `--json-output` log lines.
//! * No dx, or dx failing before its first build: the caller falls back to
//!   the Trunk build and `/live/status` `code` says why.
//!
//! dx's devserver message shapes follow `dioxus-devtools-types`
//! (Copyright (c) Dioxus Labs, MIT OR Apache-2.0,
//! https://github.com/DioxusLabs/dioxus); only the JSON shape is used here.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;

use crate::hub::{Hub, now_ms};
use crate::server::App;

/// The dioxus git revision the pinned dx is built from (`cargo xtask dx-install`).
pub const DX_REV: &str = "f420ca3d6cb6812ed27a021e239d7ec0941ff317";
/// How long to wait after a compile error line for a fuller one.
const ERROR_SETTLE: Duration = Duration::from_millis(400);
/// Most of a compiler message sent to the page and controllers.
const MAX_ERROR: usize = 8 * 1024;

/// Where dx is, in order: `EVANI_DX`, the project's own install
/// (`target/evani-tools/dx/bin`, from `cargo xtask dx-install`).
pub fn find_dx(root: &Path) -> Result<PathBuf, String> {
    if let Some(p) = std::env::var_os("EVANI_DX") {
        let p = PathBuf::from(p);
        return if p.is_file() {
            Ok(p)
        } else {
            Err(format!("EVANI_DX points at {}, which doesn't exist", p.display()))
        };
    }
    let exe = if cfg!(windows) { "dx.exe" } else { "dx" };
    let local = root.join("target").join("evani-tools").join("dx").join("bin").join(exe);
    if local.is_file() {
        return Ok(local);
    }
    Err("the pinned dx isn't installed (run `cargo xtask dx-install`)".into())
}

/// `[package] name` from the app's `Cargo.toml`.
pub fn package_name(app_dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(app_dir.join("Cargo.toml")).ok()?;
    let mut in_package = false;
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            in_package = l == "[package]";
            continue;
        }
        if in_package
            && let Some(rest) = l.strip_prefix("name")
            && let Some(v) = rest.trim_start().strip_prefix('=')
        {
            let v = v.trim().trim_matches('"');
            return (!v.is_empty()).then(|| v.to_string());
        }
    }
    None
}

/// Everything needed to run dx for one game.
#[derive(Debug, Clone)]
pub struct Plan {
    pub dx: PathBuf,
    pub pkg: String,
    pub root: PathBuf,
    pub published_gz: Option<u64>,
    /// dx's cargo target dir: `target/evani-live/dx`, or the bridge's own
    /// folder's when another bridge already serves this game (2.6).
    pub target: PathBuf,
}

impl Plan {
    /// `Err` says why live code swapping is off (shown in `/live/status`).
    pub fn new(root: &Path, game: &crate::game::Game) -> Result<Self, String> {
        let pkg = package_name(&game.app_dir).ok_or_else(|| format!("no package name in {}", game.app_dir.join("Cargo.toml").display()))?;
        Ok(Self { dx: find_dx(root)?, pkg, root: root.to_path_buf(), published_gz: game.published_gz(), target: game.dx_dir() })
    }

    pub fn target_dir(&self) -> PathBuf {
        self.target.clone()
    }

    /// dx's web output: `index.html`, `wasm/<pkg>.js`, `wasm/<pkg>_bg.wasm`
    /// and each patch as `wasm/lib<pkg>-patch-<n>.wasm`.
    pub fn dist(&self) -> PathBuf {
        self.target_dir().join("dx").join(&self.pkg).join("debug").join("web").join("public")
    }
}

/// A free localhost port for dx (`EVANI_DX_PORT` picks one).
fn dx_port() -> Result<u16, String> {
    if let Some(p) = std::env::var("EVANI_DX_PORT").ok().and_then(|v| v.parse().ok()) {
        return Ok(p);
    }
    let l = std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(|e| format!("no free port for dx: {e}"))?;
    l.local_addr().map(|a| a.port()).map_err(|e| e.to_string())
}

// ---- the dx child ---------------------------------------------------------------------

/// dx and everything it starts (cargo, rustc, wasm-bindgen). Dropping it
/// ends the whole tree; on Windows a job object also ends it if the bridge
/// itself is killed.
struct DxProc {
    child: tokio::process::Child,
    #[cfg(windows)]
    _job: Option<job::Job>,
}

impl Drop for DxProc {
    fn drop(&mut self) {
        if let Some(pid) = self.child.id() {
            kill_tree(pid);
        }
    }
}

#[cfg(windows)]
pub(crate) fn kill_tree(pid: u32) {
    use std::os::windows::process::CommandExt;
    let _ = std::process::Command::new("taskkill")
        .args(["/T", "/F", "/PID", &pid.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .status();
}

#[cfg(not(windows))]
pub(crate) fn kill_tree(pid: u32) {
    let _ = std::process::Command::new("kill").arg(pid.to_string()).status();
}

#[cfg(windows)]
pub(crate) mod job {
    //! A job object that kills its processes when its last handle closes
    //! (the bridge exiting in any way, even killed).
    #![allow(unsafe_code)]

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation, SetInformationJobObject,
    };

    pub struct Job(HANDLE);

    // SAFETY: a job handle may be used and closed from any thread.
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    impl Job {
        /// Put the process (by its handle) into a new kill-on-close job.
        pub fn adopt(process: HANDLE) -> Option<Self> {
            // SAFETY: plain Win32 calls with valid arguments; every handle
            // made here is owned by `Job` and closed once in `Drop`.
            unsafe {
                let h = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if h.is_null() {
                    return None;
                }
                let job = Job(h);
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let ok = SetInformationJobObject(
                    h,
                    JobObjectExtendedLimitInformation,
                    (&raw const info).cast(),
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                if ok == 0 || AssignProcessToJobObject(h, process) == 0 {
                    return None;
                }
                Some(job)
            }
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // SAFETY: the handle came from CreateJobObjectW and is closed once.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

/// The engine's short git commit, for `EVANI_GIT_COMMIT` (`unknown` without git).
fn git_commit(root: &Path) -> String {
    let mut cmd = std::process::Command::new("git");
    cmd.current_dir(root).args(["rev-parse", "--short=7", "HEAD"]).stdin(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd.output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

fn spawn(plan: &Plan, port: u16, baked_stamp: &str) -> Result<DxProc, String> {
    let mut cmd = tokio::process::Command::new(&plan.dx);
    cmd.current_dir(&plan.root)
        .args(["serve", "--web", "--hot-patch", "-p", &plan.pkg, "--features", "live"])
        .args(["--addr", "127.0.0.1", "--port", &port.to_string()])
        .args(["--open", "false", "--interactive", "false", "--json-output"])
        // Its own target dir: a dx build never waits on the Trunk build's
        // or the bridge's cargo lock.
        .env("CARGO_TARGET_DIR", plan.target_dir())
        // Data and git files out of dx's watch list: a bake, a restore of
        // baked files or a commit never sets off a code swap (baked.rs,
        // tools/bake-index.rs, crates/evani-core/build.rs).
        .env("EVANI_BAKED_STAMP", baked_stamp)
        .env("EVANI_GIT_COMMIT", git_commit(&plan.root))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if std::env::var_os("CARGO_BUILD_JOBS").is_none() {
        cmd.env("CARGO_BUILD_JOBS", "2");
    }
    match plan.published_gz {
        Some(gz) => cmd.env("EVANI_PUBLISHED_GZ", gz.to_string()),
        None => cmd.env_remove("EVANI_PUBLISHED_GZ"),
    };
    crate::game::below_normal(&mut cmd);
    let child = cmd.spawn().map_err(|e| format!("cannot run {} ({e})", plan.dx.display()))?;
    #[cfg(windows)]
    let job = child.raw_handle().and_then(|h| job::Job::adopt(h as _));
    Ok(DxProc {
        child,
        #[cfg(windows)]
        _job: job,
    })
}

// ---- dx's messages --------------------------------------------------------------------

/// What reaches the main loop.
enum Ev {
    /// One `--json-output` line: `{"timestamp","level","message"}`.
    Log { level: String, message: String },
    /// One devserver message (`"HotPatchStart"`, `{"HotReload":{…}}`, …).
    Ws(Value),
    WsClosed,
}

/// Drop terminal colour codes (`ESC [ … letter`).
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\u{1b}' {
            if it.peek() == Some(&'[') {
                it.next();
                for d in it.by_ref() {
                    if d.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// A compiler failure from dx's log, cleaned: no colours, dx's 3-space
/// indent removed, capped. And one line for the tab (`error[E0308]:
/// mismatched types (kits/…/lib.rs:580)`).
pub fn error_texts(raw: &str) -> (String, String) {
    let clean = strip_ansi(raw);
    let lines: Vec<&str> = clean.lines().map(|l| l.strip_prefix("   ").unwrap_or(l)).collect();
    let mut message = lines.join("\n");
    if message.len() > MAX_ERROR {
        let mut cut = MAX_ERROR;
        while !message.is_char_boundary(cut) {
            cut -= 1;
        }
        message.truncate(cut);
        message.push_str("\n…");
    }
    let first = lines
        .iter()
        .position(|l| {
            let t = l.trim_start();
            t.starts_with("error") && !t.starts_with("error: aborting")
        });
    let summary = match first {
        Some(i) => {
            let err = lines[i].trim();
            let at = lines[i + 1..]
                .iter()
                .take(3)
                .find_map(|l| l.trim_start().strip_prefix("--> "))
                .map(|loc| {
                    let loc = loc.trim().replace('\\', "/");
                    // file:line:col -> file:line
                    match loc.rsplit_once(':') {
                        Some((fl, col)) if col.chars().all(|c| c.is_ascii_digit()) && fl.contains(':') => fl.to_string(),
                        _ => loc,
                    }
                });
            match at {
                Some(at) => format!("Code change didn't build: {err} ({at})"),
                None => format!("Code change didn't build: {err}"),
            }
        }
        None => format!("Code change didn't build: {}", lines.first().map_or("", |l| l.trim())),
    };
    (message, summary)
}

/// Does dx's "Build failed" text already carry rustc's rendered errors?
/// (It does for a kit or engine crate: "Failed to compile workspace dep
/// crate …" plus the full diagnostics. For the app crate it is only "Cargo
/// build failed - no output location".)
fn has_diagnostics(raw: &str) -> bool {
    strip_ansi(raw).lines().any(|l| {
        let t = l.trim_start();
        (t.starts_with("error[") || t.starts_with("error:")) && !t.starts_with("error: aborting")
    })
}

/// In non-interactive mode dx logs each rustc diagnostic of the app crate
/// as one ERROR line holding only rustc's short message (no code, no
/// place): `no field `lapz` on type `&PlayerHud``, then `aborting due to 1
/// previous error`, then `For more information about this error, try
/// `rustc --explain E0609`.` This turns those lines back into rustc's
/// shape (`error[E0609]: no field …`) and adds where it most likely is.
/// `None` when there is no compiler message among them.
pub fn short_diagnostics(lines: &[String], at: Option<&str>) -> Option<String> {
    let mut msgs: Vec<&str> = Vec::new();
    let mut codes: Vec<String> = Vec::new();
    for l in lines {
        let t = l.trim();
        if let Some(rest) = t.strip_prefix("For more information about this error, try `rustc --explain ") {
            codes.push(rest.trim_end_matches(['`', '.']).to_string());
        } else if let Some(rest) = t.strip_prefix("Some errors have detailed explanations: ") {
            codes.extend(rest.trim_end_matches('.').split(", ").map(|c| c.to_string()));
        } else if t.is_empty()
            || t.starts_with("aborting due to")
            || t.starts_with("Build failed")
            || t.starts_with("For more information about an error")
            || t.starts_with("could not compile")
        {
        } else {
            msgs.push(t);
        }
    }
    if msgs.is_empty() {
        return None;
    }
    let mut out = String::new();
    for (i, m) in msgs.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let m = m.strip_prefix("error: ").unwrap_or(m);
        if msgs.len() == 1 && codes.len() == 1 {
            out.push_str(&format!("error[{}]: {m}", codes[0]));
        } else {
            out.push_str(&format!("error: {m}"));
        }
        if i == 0
            && let Some(at) = at
        {
            out.push_str(&format!("\n  --> {at}"));
        }
    }
    if msgs.len() > 1 && !codes.is_empty() {
        out.push_str(&format!("\n(error codes: {})", codes.join(", ")));
    }
    Some(out)
}

/// Where an app-crate compile error most likely is, since dx doesn't say:
/// the `.rs` file under `root` saved most recently (within the last five
/// minutes), plus the first line in it naming the first plain identifier
/// quoted in `message` (`lapz` in "no field `lapz` on type …"). Gives
/// `apps/kart-racer/src/ui.rs:57`, or just the file.
pub fn guess_location(root: &Path, message: &str) -> Option<String> {
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    let cutoff = std::time::SystemTime::now().checked_sub(Duration::from_secs(300))?;
    let mut stack: Vec<PathBuf> = ["apps", "kits", "crates", "looks", "tools"]
        .iter()
        .map(|d| root.join(d))
        .filter(|p| p.is_dir())
        .collect();
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name();
            let name = name.to_string_lossy();
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                if !(name.starts_with('.') || name == "target" || name == "node_modules" || name == "dist") {
                    stack.push(p);
                }
            } else if name.ends_with(".rs")
                && let Ok(t) = e.metadata().and_then(|m| m.modified())
                && t >= cutoff
                && best.as_ref().is_none_or(|(b, _)| t > *b)
            {
                best = Some((t, p));
            }
        }
    }
    let (_, file) = best?;
    let rel = file.strip_prefix(root).unwrap_or(&file).to_string_lossy().replace('\\', "/");
    let ident = message
        .split('`')
        .skip(1)
        .step_by(2)
        .find(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
    let line = ident.and_then(|id| {
        let src = std::fs::read_to_string(&file).ok()?;
        src.lines().position(|l| has_word(l, id)).map(|i| i + 1)
    });
    Some(match line {
        Some(n) => format!("{rel}:{n}"),
        None => rel,
    })
}

/// `word` appears in `line` as a whole identifier.
fn has_word(line: &str, word: &str) -> bool {
    let is_id = |c: char| c.is_ascii_alphanumeric() || c == '_';
    line.match_indices(word).any(|(i, _)| {
        let before = line[..i].chars().next_back();
        let after = line[i + word.len()..].chars().next();
        !before.is_some_and(is_id) && !after.is_some_and(is_id)
    })
}

/// The patch file name from dx's jump table `lib` (`/wasm\libx-patch-1.wasm`).
pub fn patch_file(lib: &str) -> Option<&str> {
    let name = lib.rsplit(['/', '\\']).next()?;
    let ok = name.ends_with(".wasm")
        && !name.starts_with('.')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    ok.then_some(name)
}

/// How many table slots a patch module's active element segments fill.
/// dx main counts only the named functions (an undercount for workspace
/// patches), so the bridge counts the module itself. `None` if the bytes
/// aren't a wasm module it can read.
pub fn wasm_table_elems(bytes: &[u8]) -> Option<u32> {
    if bytes.len() < 8 || &bytes[..4] != b"\0asm" {
        return None;
    }
    let mut r = Reader { b: bytes, at: 8 };
    while r.at < bytes.len() {
        let id = r.byte()?;
        let size = r.u32()? as usize;
        let end = r.at.checked_add(size)?;
        if end > bytes.len() {
            return None;
        }
        if id == 9 {
            let mut s = Reader { b: &bytes[..end], at: r.at };
            let mut total = 0u32;
            for _ in 0..s.u32()? {
                let flags = s.u32()?;
                if flags > 7 {
                    return None;
                }
                let active = flags & 1 == 0;
                if flags & 3 == 2 {
                    s.u32()?; // table index
                }
                if active {
                    s.const_expr()?;
                }
                if flags & 3 != 0 {
                    s.byte()?; // elemkind / reftype
                }
                let n = s.u32()?;
                for _ in 0..n {
                    if flags & 4 != 0 {
                        s.const_expr()?;
                    } else {
                        s.u32()?;
                    }
                }
                if active {
                    total = total.checked_add(n)?;
                }
            }
            return Some(total);
        }
        r.at = end;
    }
    Some(0)
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn byte(&mut self) -> Option<u8> {
        let v = *self.b.get(self.at)?;
        self.at += 1;
        Some(v)
    }

    fn u32(&mut self) -> Option<u32> {
        let mut v: u64 = 0;
        for i in 0..5 {
            let b = self.byte()?;
            v |= u64::from(b & 0x7f) << (7 * i);
            if b & 0x80 == 0 {
                return u32::try_from(v).ok();
            }
        }
        None
    }

    fn skip_leb(&mut self) -> Option<()> {
        for _ in 0..10 {
            if self.byte()? & 0x80 == 0 {
                return Some(());
            }
        }
        None
    }

    /// A constant expression, up to and including its `end`.
    fn const_expr(&mut self) -> Option<()> {
        loop {
            match self.byte()? {
                0x0b => return Some(()),
                0x41 | 0x42 => self.skip_leb()?,  // i32/i64.const
                0x23 | 0xd2 => self.skip_leb()?,  // global.get, ref.func
                0xd0 => {
                    self.byte()?; // ref.null t
                }
                0x43 => self.at += 4, // f32.const
                0x44 => self.at += 8, // f64.const
                0x6a..=0x6c | 0x7c..=0x7e => {} // extended const: add, sub, mul
                _ => return None,
            }
        }
    }
}

// ---- running it -----------------------------------------------------------------------

/// Start dx and relay its patches until it ends. `Err(reason)` when dx
/// can't start or stops before its first good build (the caller falls back
/// to Trunk). Once the first build is up, `app`'s dist switches to dx's.
pub async fn run(plan: Plan, hub: Arc<Hub>, app: Arc<App>, quiet: bool) -> Result<(), String> {
    let port = dx_port()?;
    let dist = plan.dist();
    let started = Instant::now();
    // The baked files dx's build embeds (a bake after this reaches the page
    // as data, never as a code swap; baked.rs).
    let built = crate::baked::snapshot(&plan.root);
    let stamp = crate::baked::stamp(&built);
    app.set_baked(dist.clone(), built);
    let mut proc = spawn(&plan, port, &stamp)?;
    let mode = |state: &str| {
        json!({ "mode": "hot", "builder": "dx", "dx": plan.dx.display().to_string(), "dx_rev": DX_REV,
                "dx_port": port, "state": state })
    };
    hub.set_code_mode(mode("building"));
    if !quiet {
        eprintln!(
            "evani-live-bridge: building {} with dx for live code swapping (port {port}, first build takes a few minutes)…",
            plan.pkg
        );
    }

    let (tx, mut rx) = mpsc::unbounded_channel::<Ev>();
    let mut tail: Vec<String> = Vec::new();
    // dx's whole output, for "see the live server's output" (overwritten each start).
    let _ = std::fs::create_dir_all(plan.target_dir());
    let mut dx_log = std::fs::File::create(plan.target_dir().join("dx.log")).ok();
    if let Some(out) = proc.child.stdout.take() {
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(out).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let (level, message) = match serde_json::from_str::<Value>(&line) {
                    Ok(v) => (
                        v["level"].as_str().unwrap_or("").to_string(),
                        v["message"].as_str().unwrap_or("").to_string(),
                    ),
                    Err(_) => (String::new(), line),
                };
                if tx.send(Ev::Log { level, message }).is_err() {
                    return;
                }
            }
        });
    }
    if let Some(err) = proc.child.stderr.take() {
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(err).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if tx.send(Ev::Log { level: "STDERR".into(), message: line }).is_err() {
                    return;
                }
            }
        });
    }

    let mut ready = false;
    let mut ws_task: Option<tokio::task::JoinHandle<()>> = None;
    let mut patch_started: Option<u64> = None;
    let mut files: Vec<String> = Vec::new();
    let mut full_rebuild = false;
    // A compile error being collected (dx logs a short then a full line).
    let mut error: Option<(Instant, String)> = None;
    // dx said "FullReloadFailed": an error line should follow.
    let mut failed_at: Option<Instant> = None;
    // dx's ERROR lines before a "Build failed" (rustc's short messages).
    let mut diag: Vec<String> = Vec::new();

    let result = loop {
        let wake = tokio::time::sleep(Duration::from_millis(150));
        tokio::select! {
            status = proc.child.wait() => {
                let code = status.ok().and_then(|s| s.code());
                let why = format!("dx stopped (exit {code:?}){}", last_lines(&tail));
                break if ready { Ok(why) } else { Err(why) };
            }
            ev = rx.recv() => {
                let Some(ev) = ev else { continue };
                match ev {
                    Ev::Log { level, message } => {
                        let text = strip_ansi(&message);
                        if let Some(f) = dx_log.as_mut() {
                            use std::io::Write as _;
                            let _ = writeln!(f, "{level}\t{text}");
                        }
                        tail.push(text.lines().next().unwrap_or("").to_string());
                        if tail.len() > 12 {
                            tail.remove(0);
                        }
                        if level == "ERROR" && text.contains("Build failed") {
                            match &mut error {
                                Some((_, m)) if m.len() >= message.len() => {}
                                Some((at, m)) => { *at = Instant::now(); *m = message; }
                                None => error = Some((Instant::now(), message)),
                            }
                            failed_at = None;
                        } else if level == "ERROR" {
                            // rustc's short messages for the app crate (see short_diagnostics).
                            if diag.len() < 64 {
                                diag.push(text);
                            }
                        } else if let Some(rest) = text.strip_prefix("Hot-patching: ") {
                            diag.clear();
                            // "Hot-patching: kits\…\lib.rs took 2147ms"
                            if let Some((file, _)) = rest.rsplit_once(" took ") {
                                files = vec![file.replace('\\', "/")];
                            }
                        } else if text.contains("Build completed successfully") && !ready {
                            if !dist.join("index.html").is_file() {
                                continue;
                            }
                            ready = true;
                            app.set_dist(dist.clone());
                            hub.set_code_mode(mode("ready"));
                            hub.set_build(crate::hub::BuildState { phase: "idle", error: None,
                                build_ms: started.elapsed().as_millis() as u64 });
                            if !quiet {
                                eprintln!("evani-live-bridge: game built with dx in {:.1} s; code changes now swap in live",
                                    started.elapsed().as_secs_f64());
                            }
                            ws_task = Some(tokio::spawn(listen(port, tx.clone())));
                        }
                    }
                    Ev::Ws(msg) => {
                        on_devserver(&msg, &hub, &dist, &mut patch_started, &mut files, &mut full_rebuild,
                                     &mut failed_at, quiet).await;
                    }
                    Ev::WsClosed => {
                        // dx's devserver went away; reconnect while dx lives.
                        if ready {
                            let tx = tx.clone();
                            ws_task = Some(tokio::spawn(async move {
                                tokio::time::sleep(Duration::from_secs(1)).await;
                                listen(port, tx).await;
                            }));
                        }
                    }
                }
            }
            _ = wake => {}
        }
        // A collected compile error, once dx has settled on its full text.
        if let Some((at, _)) = &error
            && at.elapsed() >= ERROR_SETTLE
            && let Some((_, mut raw)) = error.take()
        {
            if !has_diagnostics(&raw)
                && let Some(short) = short_diagnostics(&diag, None)
            {
                let at = guess_location(&plan.root, &short);
                raw = short_diagnostics(&diag, at.as_deref()).unwrap_or(short);
            }
            diag.clear();
            let (message, summary) = error_texts(&raw);
            patch_started = None;
            if ready {
                hub.code_error(&message, &summary);
            } else {
                hub.set_code_mode(mode("error"));
                hub.set_build(crate::hub::BuildState { phase: "error", error: Some(message.clone()), build_ms: 0 });
                hub.dev_news(&json!({ "type": "live_status", "at": now_ms(), "state": "error", "kind": "code",
                                      "message": summary }));
            }
            if !quiet {
                eprintln!("evani-live-bridge: {summary}");
            }
        }
        if let Some(at) = failed_at
            && at.elapsed() >= Duration::from_millis(1500)
        {
            failed_at = None;
            patch_started = None;
            hub.code_error("The code change didn't build; see the live server's output.", "Code change didn't build");
        }
    };
    if let Some(t) = ws_task {
        t.abort();
    }
    drop(proc);
    match result {
        Ok(why) => {
            hub.set_code_mode(json!({ "mode": "off", "reason": why }));
            if !quiet {
                eprintln!("evani-live-bridge: {why}; live code swapping stopped (the game keeps its last build)");
            }
            Ok(())
        }
        Err(e) => Err(e),
    }
}

fn last_lines(tail: &[String]) -> String {
    let t: Vec<&str> = tail.iter().map(String::as_str).filter(|l| !l.trim().is_empty()).collect();
    if t.is_empty() { String::new() } else { format!(":\n{}", t[t.len().saturating_sub(6)..].join("\n")) }
}

/// Connect to dx's devserver and forward its messages until it closes.
async fn listen(port: u16, tx: mpsc::UnboundedSender<Ev>) {
    let url = format!("ws://127.0.0.1:{port}/_dioxus?build_id=0");
    let mut stream = None;
    for _ in 0..40 {
        if let Ok((s, _)) = tokio_tungstenite::connect_async(url.as_str()).await {
            stream = Some(s);
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let Some(mut s) = stream else {
        let _ = tx.send(Ev::WsClosed);
        return;
    };
    while let Some(Ok(m)) = s.next().await {
        if let tokio_tungstenite::tungstenite::Message::Text(t) = m
            && let Ok(v) = serde_json::from_str::<Value>(t.as_str())
            && tx.send(Ev::Ws(v)).is_err()
        {
            return;
        }
    }
    let _ = tx.send(Ev::WsClosed);
}

#[allow(clippy::too_many_arguments)]
async fn on_devserver(
    msg: &Value,
    hub: &Hub,
    dist: &Path,
    patch_started: &mut Option<u64>,
    files: &mut Vec<String>,
    full_rebuild: &mut bool,
    failed_at: &mut Option<Instant>,
    quiet: bool,
) {
    if let Some(tag) = msg.as_str() {
        match tag {
            "HotPatchStart" => {
                let at = now_ms();
                *patch_started = Some(at);
                files.clear();
                hub.code_patching(at, &[]);
            }
            "FullReloadStart" => {
                *full_rebuild = true;
                hub.code_patching(now_ms(), &[]);
                if !quiet {
                    eprintln!("evani-live-bridge: this change needs a full rebuild");
                }
            }
            "FullReloadCommand" if *full_rebuild => {
                *full_rebuild = false;
                hub.code_reload("This change can't be swapped into the running game (a full rebuild was needed), so the game restarts with the new build.");
            }
            "FullReloadFailed" => {
                *full_rebuild = false;
                *failed_at = Some(Instant::now());
            }
            _ => {}
        }
        return;
    }
    let Some(hr) = msg.get("HotReload") else { return };
    let Some(jt) = hr.get("jump_table").filter(|j| j.is_object()) else {
        return; // templates or assets only: nothing for the engine
    };
    let mut table = jt.clone();
    let Some(file) = table["lib"].as_str().and_then(patch_file).map(String::from) else {
        hub.code_error(&format!("dx sent a patch with an unexpected file name: {}", table["lib"]), "Code change not applied (unexpected patch file)");
        return;
    };
    let counted = tokio::fs::read(dist.join("wasm").join(&file)).await.ok().and_then(|b| wasm_table_elems(&b));
    let dx_count = table["ifunc_count"].as_u64().unwrap_or(0);
    table["ifunc_count"] = json!(dx_count.max(counted.map_or(0, u64::from)));
    table["lib"] = json!(format!("/wasm/{file}"));
    let functions = table["map"].as_object().map_or(0, serde_json::Map::len);
    let build_ms = hr["ms_elapsed"].as_u64().unwrap_or(0);
    let started_at = patch_started.take().unwrap_or_else(now_ms);
    let seq = hub.code_patch(table, functions, build_ms, started_at, files);
    if !quiet {
        eprintln!(
            "evani-live-bridge: code patch {seq} ready ({functions} functions, {:.1} s since the save was seen)",
            (now_ms().saturating_sub(started_at)) as f64 / 1000.0
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_names() {
        let dir = std::env::temp_dir().join(format!("evani-code-pkg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"evani-kart-racer\"\nversion.workspace = true\n\n[dependencies]\nname = \"x\"\n",
        )
        .unwrap();
        assert_eq!(package_name(&dir).as_deref(), Some("evani-kart-racer"));
        std::fs::write(dir.join("Cargo.toml"), "[dependencies]\nname = \"x\"\n").unwrap();
        assert_eq!(package_name(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dist_layout() {
        let game = crate::game::Game::new(Path::new("C:/r"), "sandbox");
        let p = Plan { dx: "dx".into(), pkg: "evani-sandbox".into(), root: "C:/r".into(), published_gz: None, target: game.dx_dir() };
        assert!(p.dist().ends_with(Path::new("target/evani-live/dx/dx/evani-sandbox/debug/web/public")));
    }

    #[test]
    fn patch_file_names() {
        assert_eq!(patch_file("/wasm\\libevani-kart-racer-patch-1792005397782.wasm"), Some("libevani-kart-racer-patch-1792005397782.wasm"));
        assert_eq!(patch_file("/wasm/lib_a.wasm"), Some("lib_a.wasm"));
        for bad in ["/wasm/..", "/wasm/x.js", "/wasm/a b.wasm", "", "/wasm/.wasm"] {
            assert_eq!(patch_file(bad), None, "{bad}");
        }
    }

    /// The exact text dx main logged for a type error in a kit (one line of
    /// its `--json-output`).
    #[test]
    fn compile_errors_are_cleaned_and_summed_up() {
        let raw = "\u{1b}[31mBuild failed\u{1b}[0m: Failed to replay workspace crate 'evani_kit_kart_racer'\n\u{1b}[33m1\u{1b}[0m: Failed to compile workspace dep crate 'evani_kit_kart_racer':\n   \u{1b}[1m\u{1b}[91merror[E0308]\u{1b}[0m\u{1b}[1m\u{1b}[97m: mismatched types\u{1b}[0m\n      \u{1b}[1m\u{1b}[96m--> \u{1b}[0mkits\\evani-kit-kart-racer\\src\\lib.rs:580:62\n       \u{1b}[1m\u{1b}[96m|\u{1b}[0m\n   \u{1b}[1m\u{1b}[91merror\u{1b}[0m\u{1b}[1m\u{1b}[97m: aborting due to 1 previous error\u{1b}[0m";
        let (message, summary) = error_texts(raw);
        assert!(!message.contains('\u{1b}'));
        assert!(message.starts_with("Build failed: Failed to replay workspace crate"));
        assert!(message.contains("\nerror[E0308]: mismatched types\n   --> kits\\evani-kit-kart-racer\\src\\lib.rs:580:62"), "{message}");
        assert_eq!(summary, "Code change didn't build: error[E0308]: mismatched types (kits/evani-kit-kart-racer/src/lib.rs:580)");
        let (_, s) = error_texts("Build failed: linker exploded");
        assert_eq!(s, "Code change didn't build: Build failed: linker exploded");
    }

    /// What dx main logged (ERROR lines of `--json-output`) for a mistyped
    /// field in the app crate, where its "Build failed" line has no detail.
    #[test]
    fn app_crate_errors_get_their_code_and_place_back() {
        let dx_build_failed = "Build failed: Cargo build failed - no output location. Toggle tracing mode (press `t`) for more information.";
        assert!(!has_diagnostics(dx_build_failed));
        let lines: Vec<String> = [
            "no field `lapz` on type `&PlayerHud`",
            "aborting due to 1 previous error",
            "For more information about this error, try `rustc --explain E0609`.",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let short = short_diagnostics(&lines, None).unwrap();
        assert_eq!(short, "error[E0609]: no field `lapz` on type `&PlayerHud`");

        // The place: the newest saved .rs file and the line naming `lapz`.
        let root = std::env::temp_dir().join(format!("evani-code-guess-{}", std::process::id()));
        let src = root.join("apps").join("demo").join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("ui.rs"), "fn a() {}\nlet laps = 3;\nlet x = h.lapz;\n").unwrap();
        let at = guess_location(&root, &short);
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(at.as_deref(), Some("apps/demo/src/ui.rs:3"));

        let raw = short_diagnostics(&lines, at.as_deref()).unwrap();
        assert!(has_diagnostics(&raw));
        let (message, summary) = error_texts(&raw);
        assert_eq!(message, "error[E0609]: no field `lapz` on type `&PlayerHud`\n  --> apps/demo/src/ui.rs:3");
        assert_eq!(summary, "Code change didn't build: error[E0609]: no field `lapz` on type `&PlayerHud` (apps/demo/src/ui.rs:3)");

        // Several errors: each kept, codes listed once.
        let many: Vec<String> = ["mismatched types", "cannot find value `q` in this scope", "aborting due to 2 previous errors",
            "Some errors have detailed explanations: E0308, E0425."].iter().map(|s| s.to_string()).collect();
        assert_eq!(short_diagnostics(&many, None).unwrap(),
            "error: mismatched types\nerror: cannot find value `q` in this scope\n(error codes: E0308, E0425)");
        // Nothing from rustc: no text.
        assert_eq!(short_diagnostics(&["aborting due to 1 previous error".to_string()], None), None);
        assert!(has_word("h.lapz;", "lapz") && !has_word("laps", "lap"));
    }

    /// A hand-made module: an active segment of 3 functions at
    /// `global.get 0`, a passive one (not counted), an active expression
    /// segment of 2 at `i32.const 5`.
    #[test]
    fn element_segments_are_counted() {
        let mut m = b"\0asm\x01\0\0\0".to_vec();
        // a custom section before it is skipped
        m.extend([0, 3, 1, b'x', 7]);
        let elem: Vec<u8> = vec![
            3, // segments
            0, 0x23, 0, 0x0b, 3, 0, 1, 2, // flags 0: expr, 3 funcs
            1, 0, 2, 0, 1, // flags 1: passive, elemkind, 2 funcs
            4, 0x41, 5, 0x0b, 2, 0xd2, 0, 0x0b, 0xd2, 1, 0x0b, // flags 4: expr, 2 exprs
        ];
        m.push(9);
        m.push(elem.len() as u8);
        m.extend(&elem);
        assert_eq!(wasm_table_elems(&m), Some(5));
        assert_eq!(wasm_table_elems(&m[..m.len() - 2]), None, "truncated");
        assert_eq!(wasm_table_elems(b"\0asm\x01\0\0\0"), Some(0));
        assert_eq!(wasm_table_elems(b"nope"), None);
    }

    #[test]
    fn ansi_is_stripped() {
        assert_eq!(strip_ansi("\u{1b}[33m117.46s\u{1b}[0m ok"), "117.46s ok");
    }
}
