//! Long child steps (`cargo test`, ship builds, `trunk build`) that can
//! neither go silent nor hang for ever.
//!
//! `check --full` and `build --ship` used to run each step with
//! `Command::output()`: nothing was printed until the step ended, and one step
//! can take 8 minutes or more on a busy PC (a ship build is several trunk
//! builds). Watchers that judge a command by its output and by the processor
//! time of its live processes (Evani.App's 8-minute rule does both) then saw a
//! working check as frozen, because the work happens in short-lived
//! grandchildren (rustc, test binaries, node) whose processor time vanishes
//! with them, while the parents (xtask, cargo, trunk) sit at almost zero CPU.
//! Workers killed or abandoned healthy checks (26-27 Oct 2026).
//!
//! [`run_watched`] runs a step with its output captured as before, and also:
//! - prints a heartbeat line every [`HEARTBEAT_ENV`] seconds (default 60)
//!   naming the step, how long it has run and its last output line, and says
//!   so plainly when that line is cargo waiting for a file lock;
//! - stops the step (its whole process tree, by PID) when it has printed
//!   nothing for [`STALL_ENV`] seconds (default 20 minutes; `0` turns it
//!   off), and reports it as stalled with a `fix:`, instead of hanging;
//! - gives the child no stdin, so nothing can wait on a keyboard;
//! - stops waiting for output a few seconds after the child itself has ended,
//!   so a background process it left holding the output pipe (a Windows
//!   classic) cannot hold the check for ever.

use std::io::{Read, Write};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

/// Seconds between heartbeat lines (`0`: none).
pub const HEARTBEAT_ENV: &str = "EVANI_STEP_HEARTBEAT_SECS";
/// Seconds without any output before a step is stopped as stalled (`0`: never).
pub const STALL_ENV: &str = "EVANI_STEP_STALL_SECS";

pub const DEFAULT_HEARTBEAT: Duration = Duration::from_secs(60);
/// Longer than any silent stretch seen in a healthy step (a release rustc of
/// a big game crate is the longest, a few minutes under load).
pub const DEFAULT_STALL: Duration = Duration::from_secs(20 * 60);
/// How long to keep reading after the child has ended.
pub const DEFAULT_DRAIN: Duration = Duration::from_secs(10);

/// How a step is watched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Watch {
    pub heartbeat: Option<Duration>,
    pub stall: Option<Duration>,
    pub drain: Duration,
}

impl Default for Watch {
    fn default() -> Self {
        Watch { heartbeat: Some(DEFAULT_HEARTBEAT), stall: Some(DEFAULT_STALL), drain: DEFAULT_DRAIN }
    }
}

impl Watch {
    /// From the two environment values: a positive number of seconds sets it,
    /// `0` turns it off, anything else keeps the default.
    pub fn from_env_values(heartbeat: Option<&str>, stall: Option<&str>) -> Watch {
        let pick = |v: Option<&str>, default: Duration| match v.map(str::trim).and_then(|s| s.parse::<u64>().ok()) {
            Some(0) => None,
            Some(n) => Some(Duration::from_secs(n)),
            None => Some(default),
        };
        Watch { heartbeat: pick(heartbeat, DEFAULT_HEARTBEAT), stall: pick(stall, DEFAULT_STALL), drain: DEFAULT_DRAIN }
    }

    pub fn detect() -> Watch {
        Watch::from_env_values(std::env::var(HEARTBEAT_ENV).ok().as_deref(), std::env::var(STALL_ENV).ok().as_deref())
    }
}

/// A step stopped for printing nothing for too long.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stall {
    /// How long it had been silent.
    pub quiet: Duration,
    /// Its last output line (empty: it never printed).
    pub last_line: String,
}

/// What a watched step did.
#[derive(Debug)]
pub struct Ran {
    /// `None` when it was stopped as stalled or its status could not be read.
    pub status: Option<ExitStatus>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub elapsed: Duration,
    pub stalled: Option<Stall>,
    /// It ended but something it started still held its output open.
    pub held_open: bool,
}

impl Ran {
    pub fn success(&self) -> bool {
        self.stalled.is_none() && self.status.is_some_and(|s| s.success())
    }

    /// stdout then stderr, as text.
    pub fn text(&self) -> String {
        format!("{}\n{}", String::from_utf8_lossy(&self.stdout), String::from_utf8_lossy(&self.stderr))
    }
}

/// `4m05s`, `12s`, `1h02m`.
pub fn fmt_duration(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}h{:02}m", s / 3600, (s % 3600) / 60)
    } else if s >= 60 {
        format!("{}m{:02}s", s / 60, s % 60)
    } else {
        format!("{s}s")
    }
}

/// The last non-empty line of `text`, without colour codes, at most 140 chars.
pub fn last_line(text: &str) -> String {
    let line = text.split(['\n', '\r']).map(strip_ansi).map(|l| l.trim().to_string()).rfind(|l| !l.is_empty()).unwrap_or_default();
    if line.chars().count() > 140 { format!("{}...", line.chars().take(137).collect::<String>()) } else { line }
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // ESC [ ... final byte in @..~
            if chars.next() == Some('[') {
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// When `line` is cargo waiting for a file lock: what it is waiting for, in
/// plain words.
pub fn lock_wait(line: &str) -> Option<&'static str> {
    let l = line.to_ascii_lowercase();
    if !l.contains("blocking waiting for file lock") {
        return None;
    }
    Some(if l.contains("package cache") {
        "another cargo holds the shared package cache (CARGO_HOME); it is downloading or resolving crates"
    } else if l.contains("build directory") || l.contains("artifact directory") || l.contains("build lock") {
        "another cargo is building in the same target folder (CARGO_TARGET_DIR)"
    } else {
        "another cargo holds a lock this step needs"
    })
}

/// `cargo test --quiet -p x`: the program's file name and its arguments, to
/// name a step and to show how to run it alone.
pub fn command_line(cmd: &Command) -> String {
    let prog = std::path::Path::new(cmd.get_program()).file_stem().map_or_else(|| cmd.get_program().to_string_lossy().into_owned(), |s| s.to_string_lossy().into_owned());
    std::iter::once(prog).chain(cmd.get_args().map(|a| a.to_string_lossy().into_owned())).collect::<Vec<_>>().join(" ")
}

/// One heartbeat line.
pub fn heartbeat_line(label: &str, running: Duration, quiet: Duration, last: &str) -> String {
    let mut s = format!("  ... still running `{label}`: {} so far", fmt_duration(running));
    if last.is_empty() {
        s.push_str(", no output yet");
    } else {
        s.push_str(&format!("; last output {} ago: {last}", fmt_duration(quiet)));
    }
    if let Some(why) = lock_wait(last) {
        s.push_str(&format!(" [waiting for a cargo lock: {why}]"));
    }
    s
}

/// The `fix:` text for a step stopped as stalled.
pub fn stall_fix(command: &str, stall: &Stall) -> String {
    let lock = lock_wait(&stall.last_line).map(|why| format!(" Its last line is cargo waiting for a file lock ({why}): give this worker its own CARGO_TARGET_DIR (and, for the package cache, its own CARGO_HOME), or let the other build finish, then run it again.")).unwrap_or_default();
    format!(
        "The step printed nothing for {} and was stopped, so the check did not hang. It was waiting on something that never came: a cargo lock held by another build, a test waiting for a browser or tool, or a process starved of CPU on a very busy PC.{lock} Run it on its own to see where it sits: `{command}`. On a very busy PC give slow steps longer with {STALL_ENV}=<seconds> (0 turns the limit off).",
        fmt_duration(stall.quiet)
    )
}

/// The one-line summary of a stall for the issue text.
pub fn stall_what(stall: &Stall) -> String {
    if stall.last_line.is_empty() {
        format!("stopped after {} with no output at all", fmt_duration(stall.quiet))
    } else {
        format!("stopped after {} with no output (last line: `{}`)", fmt_duration(stall.quiet), stall.last_line)
    }
}

/// Stop `child` and everything it started (by PID).
fn kill_tree(child: &mut Child) {
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill").args(["/PID", &child.id().to_string(), "/T", "/F"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    let _ = child.kill();
}

enum Msg {
    Data(usize, Vec<u8>),
    Eof,
}

fn reader(mut r: impl Read + Send + 'static, idx: usize, tx: mpsc::Sender<Msg>) {
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match r.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send(Msg::Data(idx, buf[..n].to_vec())).is_err() {
                        return;
                    }
                }
            }
        }
        let _ = tx.send(Msg::Eof);
    });
}

/// Run `cmd` (stdin null, stdout and stderr captured) under `watch`, writing
/// heartbeat and note lines to `log` (the caller's stderr in practice).
/// `label` names the step in those lines.
pub fn run_watched(cmd: &mut Command, label: &str, watch: Watch, log: &mut dyn Write) -> std::io::Result<Ran> {
    let start = Instant::now();
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let (tx, rx) = mpsc::channel();
    let mut open = 0;
    if let Some(o) = child.stdout.take() {
        reader(o, 0, tx.clone());
        open += 1;
    }
    if let Some(e) = child.stderr.take() {
        reader(e, 1, tx.clone());
        open += 1;
    }
    drop(tx);

    let mut out: [Vec<u8>; 2] = [Vec::new(), Vec::new()];
    // The last few KB of either stream, for the last line.
    let mut tail = String::new();
    let mut last_out = start;
    let mut next_beat = watch.heartbeat.map(|h| start + h);
    let mut status: Option<ExitStatus> = None;
    let mut ended_at: Option<Instant> = None;
    let mut stalled = None;
    let mut held_open = false;
    loop {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(Msg::Data(i, bytes)) => {
                last_out = Instant::now();
                tail.push_str(&String::from_utf8_lossy(&bytes));
                if tail.len() > 8192 {
                    let mut cut = tail.len() - 4096;
                    while !tail.is_char_boundary(cut) {
                        cut += 1;
                    }
                    tail.drain(..cut);
                }
                out[i].extend_from_slice(&bytes);
            }
            Ok(Msg::Eof) => open -= 1,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => open = 0,
        }
        let now = Instant::now();
        if status.is_none()
            && let Ok(Some(s)) = child.try_wait()
        {
            status = Some(s);
            ended_at = Some(now);
        }
        if open == 0 {
            if status.is_none() {
                status = child.wait().ok();
            }
            break;
        }
        if ended_at.is_some_and(|t| now.duration_since(t) >= watch.drain) {
            held_open = true;
            let _ = writeln!(log, "  note: `{label}` ended, but a process it started still holds its output open; not waiting for it");
            break;
        }
        if status.is_none() && watch.stall.is_some_and(|s| now.duration_since(last_out) >= s) {
            kill_tree(&mut child);
            let _ = child.wait();
            let st = Stall { quiet: now.duration_since(last_out), last_line: last_line(&tail) };
            let _ = writeln!(log, "  STALLED `{label}`: {}; stopped it", stall_what(&st));
            stalled = Some(st);
            break;
        }
        if let (Some(at), Some(h)) = (next_beat, watch.heartbeat)
            && now >= at
        {
            let _ = writeln!(log, "{}", heartbeat_line(label, now.duration_since(start), now.duration_since(last_out), &last_line(&tail)));
            let _ = log.flush();
            next_beat = Some(now + h);
        }
    }
    let [stdout, stderr] = out;
    Ok(Ran { status: if stalled.is_some() { None } else { status }, stdout, stderr, elapsed: start.elapsed(), stalled, held_open })
}

/// [`run_watched`] with [`Watch::detect`] and heartbeats on this process's stderr.
pub fn run_watched_stderr(cmd: &mut Command, label: &str) -> std::io::Result<Ran> {
    run_watched(cmd, label, Watch::detect(), &mut std::io::stderr())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quick(heartbeat_ms: u64, stall_ms: Option<u64>, drain_ms: u64) -> Watch {
        Watch { heartbeat: Some(Duration::from_millis(heartbeat_ms)), stall: stall_ms.map(Duration::from_millis), drain: Duration::from_millis(drain_ms) }
    }

    /// A shell command line, run by cmd on Windows and sh elsewhere.
    fn shell(line_windows: &str, line_unix: &str) -> Command {
        if cfg!(windows) {
            let mut c = Command::new("cmd");
            c.args(["/C", line_windows]);
            c
        } else {
            let mut c = Command::new("sh");
            c.args(["-c", line_unix]);
            c
        }
    }

    #[test]
    fn env_values_set_defaults_and_turn_off() {
        assert_eq!(Watch::from_env_values(None, None), Watch::default());
        let w = Watch::from_env_values(Some("15"), Some("0"));
        assert_eq!(w.heartbeat, Some(Duration::from_secs(15)));
        assert_eq!(w.stall, None);
        let w = Watch::from_env_values(Some("0"), Some(" 90 "));
        assert_eq!(w.heartbeat, None);
        assert_eq!(w.stall, Some(Duration::from_secs(90)));
        assert_eq!(Watch::from_env_values(Some("soon"), Some("-3")), Watch::default());
    }

    #[test]
    fn a_command_line_names_the_program_and_its_arguments() {
        let mut c = Command::new(r"C:\Users\x\.cargo\bin\cargo.exe");
        c.args(["test", "--quiet", "-p", "evani-render"]);
        assert_eq!(command_line(&c), "cargo test --quiet -p evani-render");
    }

    #[test]
    fn durations_read_plainly() {
        assert_eq!(fmt_duration(Duration::from_secs(12)), "12s");
        assert_eq!(fmt_duration(Duration::from_secs(245)), "4m05s");
        assert_eq!(fmt_duration(Duration::from_secs(3720)), "1h02m");
    }

    #[test]
    fn last_line_skips_blanks_colours_and_carriage_returns() {
        assert_eq!(last_line("   Compiling a\n\x1b[1m\x1b[32m   Compiling evani-render v0.1.0\x1b[0m\n\n  \r\n"), "Compiling evani-render v0.1.0");
        assert_eq!(last_line("building 1/3\rbuilding 2/3\r"), "building 2/3");
        assert_eq!(last_line(""), "");
        assert_eq!(last_line(&"x".repeat(300)).chars().count(), 140);
    }

    #[test]
    fn cargo_lock_waits_are_named() {
        assert!(lock_wait("    Blocking waiting for file lock on package cache").is_some_and(|w| w.contains("CARGO_HOME")));
        assert!(lock_wait("Blocking waiting for file lock on build directory").is_some_and(|w| w.contains("CARGO_TARGET_DIR")));
        assert!(lock_wait("Blocking waiting for file lock on artifact directory").is_some_and(|w| w.contains("CARGO_TARGET_DIR")));
        assert_eq!(lock_wait("Compiling evani-core v0.1.0"), None);
        let h = heartbeat_line("cargo test -p x", Duration::from_secs(130), Duration::from_secs(70), "Blocking waiting for file lock on package cache");
        assert!(h.contains("`cargo test -p x`") && h.contains("2m10s") && h.contains("1m10s ago") && h.contains("waiting for a cargo lock"), "{h}");
        assert!(heartbeat_line("trunk build ship", Duration::from_secs(5), Duration::from_secs(5), "").contains("no output yet"));
    }

    #[test]
    fn a_stall_names_its_step_and_fix() {
        let s = Stall { quiet: Duration::from_secs(1200), last_line: "Blocking waiting for file lock on build directory".into() };
        assert!(stall_what(&s).contains("20m00s") && stall_what(&s).contains("file lock"));
        let fix = stall_fix("cargo xtask build sandbox --ship", &s);
        assert!(fix.contains("`cargo xtask build sandbox --ship`") && fix.contains("CARGO_TARGET_DIR") && fix.contains(STALL_ENV), "{fix}");
        let quiet = Stall { quiet: Duration::from_secs(60), last_line: String::new() };
        assert!(stall_what(&quiet).contains("no output at all"));
        assert!(!stall_fix("cargo test", &quiet).contains("file lock ("));
    }

    #[test]
    fn a_working_step_is_captured_and_gets_heartbeats() {
        // Prints, stays quiet for ~2 s (ping waits a second between replies), prints again.
        let mut cmd = shell("echo first & ping -n 3 127.0.0.1 >NUL & echo second 1>&2", "echo first; sleep 2; echo second 1>&2");
        let mut log = Vec::new();
        let ran = run_watched(&mut cmd, "demo step", quick(300, Some(30_000), 2_000), &mut log).expect("runs");
        assert!(ran.success(), "{ran:?}");
        assert!(String::from_utf8_lossy(&ran.stdout).contains("first"));
        assert!(String::from_utf8_lossy(&ran.stderr).contains("second"));
        let log = String::from_utf8_lossy(&log);
        assert!(log.contains("still running `demo step`") && log.contains("last output") && log.contains("first"), "{log}");
        assert!(!ran.held_open && ran.stalled.is_none());
    }

    #[test]
    fn a_failing_step_keeps_its_status_and_output() {
        let mut cmd = shell("echo broken 1>&2 & exit 3", "echo broken 1>&2; exit 3");
        let ran = run_watched(&mut cmd, "fails", quick(10_000, Some(30_000), 2_000), &mut Vec::new()).expect("runs");
        assert!(!ran.success());
        assert_eq!(ran.status.and_then(|s| s.code()), Some(3));
        assert!(ran.text().contains("broken"));
    }

    #[test]
    fn a_silent_step_is_stopped_not_waited_on_for_ever() {
        // Silent for ~30 s; the limit is 1 s.
        let mut cmd = shell("echo starting & ping -n 31 127.0.0.1 >NUL", "echo starting; sleep 30");
        let mut log = Vec::new();
        let t = Instant::now();
        let ran = run_watched(&mut cmd, "silent step", quick(10_000, Some(1_000), 2_000), &mut log).expect("runs");
        assert!(t.elapsed() < Duration::from_secs(15), "took {:?}", t.elapsed());
        let st = ran.stalled.clone().expect("stalled");
        assert_eq!(st.last_line, "starting");
        assert!(!ran.success() && ran.status.is_none());
        assert!(String::from_utf8_lossy(&log).contains("STALLED `silent step`"));
    }

    #[test]
    fn stdin_is_closed_so_nothing_waits_on_a_keyboard() {
        // `set /p` and `read` return at once on a closed stdin.
        let mut cmd = shell("set /p X=ask: & echo answered", "read X; echo answered");
        let t = Instant::now();
        let ran = run_watched(&mut cmd, "asks", quick(10_000, Some(10_000), 2_000), &mut Vec::new()).expect("runs");
        assert!(t.elapsed() < Duration::from_secs(8));
        assert!(ran.text().contains("answered") && ran.stalled.is_none());
    }

    #[test]
    fn a_leftover_process_holding_the_output_does_not_hold_the_step() {
        // The child ends at once; the background process it starts inherits
        // the output pipe and keeps it open for ~4 s.
        let mut cmd = shell("start /B ping -n 5 127.0.0.1 & echo parent done", "(sleep 4; echo late) & echo parent done");
        let mut log = Vec::new();
        let t = Instant::now();
        let ran = run_watched(&mut cmd, "leaves a child", quick(10_000, Some(30_000), 500), &mut log).expect("runs");
        assert!(t.elapsed() < Duration::from_secs(3), "took {:?}", t.elapsed());
        assert!(ran.held_open && ran.success(), "{ran:?}");
        assert!(String::from_utf8_lossy(&log).contains("still holds its output open"));
    }
}
