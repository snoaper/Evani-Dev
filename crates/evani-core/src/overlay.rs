//! Debug overlay data and formatting, plus the small resources it reads.
//!
//! The text is formatted into reused `String`s (no per-frame allocation once
//! the buffers have grown) and only a few times per second
//! ([`Overlay::interval_ms`]). On wasm the runner copies it into one `<pre>`
//! element when [`Overlay::revision`] changes, so the DOM is touched about
//! four times per second, never per frame.

use core::fmt::Write;
use std::collections::VecDeque;

use crate::effects::BudgetStatus;
// Numbers go through `fixed`, not `{:.N}`, so the overlay ships no float
// formatting code (same text; README "How data ships").
use crate::num::fixed;
use crate::quality::QualityState;

/// GPU time of one render pass (timestamp queries, when available).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PassTiming {
    pub name: &'static str,
    pub gpu_ms: f32,
}

/// Written by the renderer each frame. World resource.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RenderStats {
    /// Total GPU ms; `None` when timestamp queries are unavailable.
    pub gpu_ms: Option<f32>,
    pub draw_calls: u32,
    pub triangles: u32,
    pub sprites: u32,
    /// Per-pass GPU timings; the renderer clears and refills it (keeps
    /// capacity).
    pub passes: Vec<PassTiming>,
    /// Why `gpu_ms` is `None` (e.g. `timestamp-query unsupported`); shown
    /// as `gpu n/a (<note>)`. Empty when there is nothing to say.
    pub gpu_note: &'static str,
    /// `gpu_ms` is the submit-to-done estimate (`onSubmittedWorkDone`),
    /// not timestamp queries; shown as `gpu ~1.20 ms`.
    pub gpu_estimated: bool,
    /// Bumped every time `gpu_ms` (and `passes`) take a new reading. GPU
    /// readbacks arrive a frame or more late and not every frame, so a
    /// reader that wants one sample per reading (the overlay's median
    /// window, the bench) samples only when this changes.
    pub gpu_samples: u64,
    /// Live particles drawn this frame (the shared particle system; 0
    /// without one).
    pub particles: u32,
}

/// How far back the overlay's GPU median looks (ms).
pub const GPU_WINDOW_MS: f64 = 2000.0;
/// Readings the window keeps. At more than 512 readings per second the
/// window covers the last 1024 readings (under 2 s) instead.
const GPU_WINDOW_CAP: usize = 1024;
/// Pass names the window tracks at once; a pass with no reading in the
/// window gives its column up to a new name.
const GPU_WINDOW_PASSES: usize = 16;

/// The overlay's GPU timings as medians over the last [`GPU_WINDOW_MS`] of
/// readings, so one slow or fast frame (or the GPU clocking up or down for
/// a moment) does not swing the numbers. One sample per new GPU reading
/// ([`RenderStats::gpu_samples`]), whatever the frame rate. Fixed-size
/// buffers allocated once; [`GpuWindow::push`] and [`GpuWindow::update`]
/// never allocate, and the medians are worked out only when the overlay
/// text is (a few times per second).
#[derive(Debug, Clone)]
pub struct GpuWindow {
    window_ms: f64,
    /// Time of each reading (ms); ring indexed like `totals`.
    times: Vec<f64>,
    /// Total GPU ms of each reading; NaN when it had none.
    totals: Vec<f32>,
    /// `GPU_WINDOW_CAP` rows of `GPU_WINDOW_PASSES` columns: GPU ms of each
    /// pass per reading, NaN when the pass did not run in that reading.
    values: Vec<f32>,
    /// Pass name of each column.
    names: Vec<&'static str>,
    /// Next row to write.
    head: usize,
    len: usize,
    last_seen: u64,
    scratch: Vec<f32>,
    /// Results of the last [`GpuWindow::update`].
    total: Option<f32>,
    passes: Vec<PassTiming>,
}

impl Default for GpuWindow {
    fn default() -> Self {
        Self::new(GPU_WINDOW_MS)
    }
}

impl GpuWindow {
    pub fn new(window_ms: f64) -> Self {
        Self {
            window_ms,
            times: vec![f64::NEG_INFINITY; GPU_WINDOW_CAP],
            totals: vec![f32::NAN; GPU_WINDOW_CAP],
            values: vec![f32::NAN; GPU_WINDOW_CAP * GPU_WINDOW_PASSES],
            names: Vec::with_capacity(GPU_WINDOW_PASSES),
            head: 0,
            len: 0,
            last_seen: 0,
            scratch: Vec::with_capacity(GPU_WINDOW_CAP),
            total: None,
            passes: Vec::with_capacity(GPU_WINDOW_PASSES),
        }
    }

    /// Take the reading in `stats` if it is new (its `gpu_samples` moved
    /// on since the last call). Cheap; call it every frame.
    pub fn observe(&mut self, now_ms: f64, stats: &RenderStats) {
        if stats.gpu_samples != self.last_seen {
            self.last_seen = stats.gpu_samples;
            self.push(now_ms, stats.gpu_ms, &stats.passes);
        }
    }

    /// Add one reading taken at `now_ms`.
    pub fn push(&mut self, now_ms: f64, total: Option<f32>, passes: &[PassTiming]) {
        let row = self.head;
        self.times[row] = now_ms;
        self.totals[row] = total.unwrap_or(f32::NAN);
        let base = row * GPU_WINDOW_PASSES;
        self.values[base..base + GPU_WINDOW_PASSES].fill(f32::NAN);
        for p in passes {
            if let Some(col) = self.column(p.name, now_ms) {
                self.values[base + col] = p.gpu_ms;
            }
        }
        self.head = (row + 1) % GPU_WINDOW_CAP;
        self.len = (self.len + 1).min(GPU_WINDOW_CAP);
    }

    /// Column of `name`, claiming a free one (or one with no reading in the
    /// window) for a new name. `None` when every column is in use.
    fn column(&mut self, name: &'static str, now_ms: f64) -> Option<usize> {
        if let Some(i) = self.names.iter().position(|n| *n == name) {
            return Some(i);
        }
        if self.names.len() < GPU_WINDOW_PASSES {
            self.names.push(name);
            return Some(self.names.len() - 1);
        }
        let stale = (0..GPU_WINDOW_PASSES).find(|&c| !self.has_reading(c, now_ms))?;
        for r in 0..GPU_WINDOW_CAP {
            self.values[r * GPU_WINDOW_PASSES + stale] = f32::NAN;
        }
        self.names[stale] = name;
        Some(stale)
    }

    fn in_window(&self, row: usize, now_ms: f64) -> bool {
        now_ms - self.times[row] <= self.window_ms
    }

    /// The rows written so far, newest first.
    fn rows(&self) -> impl Iterator<Item = usize> + '_ {
        (1..=self.len).map(|k| (self.head + GPU_WINDOW_CAP - k) % GPU_WINDOW_CAP)
    }

    fn has_reading(&self, col: usize, now_ms: f64) -> bool {
        self.rows()
            .take_while(|&r| self.in_window(r, now_ms))
            .any(|r| !self.values[r * GPU_WINDOW_PASSES + col].is_nan())
    }

    /// Median of `column(row)` over the rows in the window, skipping NaN.
    fn median_of(&mut self, now_ms: f64, column: impl Fn(&Self, usize) -> f32) -> Option<f32> {
        let mut scratch = core::mem::take(&mut self.scratch);
        scratch.clear();
        for r in self.rows().take_while(|&r| self.in_window(r, now_ms)) {
            let v = column(self, r);
            if !v.is_nan() {
                scratch.push(v);
            }
        }
        let m = median(&mut scratch);
        self.scratch = scratch;
        m
    }

    /// Work out the medians at `now_ms` for the passes in `current` (the
    /// latest reading's pass list, so a pass that stopped running drops out
    /// and the order matches the renderer's). A pass with no reading in the
    /// window keeps its latest value.
    pub fn update(&mut self, now_ms: f64, current: &[PassTiming]) {
        self.total = self.median_of(now_ms, |w, r| w.totals[r]);
        self.passes.clear();
        for p in current.iter().take(GPU_WINDOW_PASSES) {
            let ms = match self.names.iter().position(|n| *n == p.name) {
                Some(col) => self
                    .median_of(now_ms, |w, r| w.values[r * GPU_WINDOW_PASSES + col])
                    .unwrap_or(p.gpu_ms),
                None => p.gpu_ms,
            };
            self.passes.push(PassTiming {
                name: p.name,
                gpu_ms: ms,
            });
        }
    }

    /// Median total GPU ms at the last [`GpuWindow::update`]; `None` when no
    /// reading in the window had one.
    pub fn total(&self) -> Option<f32> {
        self.total
    }

    /// Median GPU ms per pass at the last [`GpuWindow::update`].
    pub fn passes(&self) -> &[PassTiming] {
        &self.passes
    }
}

/// Median of `v` (reorders it); the mean of the two middle values for an
/// even count. `None` when empty.
fn median(v: &mut [f32]) -> Option<f32> {
    let n = v.len();
    if n == 0 {
        return None;
    }
    let mid = n / 2;
    let (lower, m, _) = v.select_nth_unstable_by(mid, f32::total_cmp);
    let m = *m;
    if n % 2 == 1 {
        return Some(m);
    }
    let below = lower.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    Some((below + m) / 2.0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Worth knowing, not a fault (e.g. a baked value for a module that is
    /// compiled out of this build).
    Note,
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LogEntry {
    pub severity: Severity,
    pub message: String,
    /// When it was logged ([`crate::time::now_ms`]: ms since the page
    /// started).
    pub at_ms: f64,
}

/// Recent errors and notes, shown in the overlay instead of panicking.
/// World resource.
#[derive(Debug, Clone)]
pub struct ErrorLog {
    entries: VecDeque<LogEntry>,
    cap: usize,
    errors: u32,
    revision: u64,
}

impl Default for ErrorLog {
    fn default() -> Self {
        Self {
            entries: VecDeque::with_capacity(16),
            cap: 16,
            errors: 0,
            revision: 0,
        }
    }
}

impl ErrorLog {
    fn push(&mut self, severity: Severity, message: String) {
        self.push_at(severity, message, crate::time::now_ms());
    }

    fn push_at(&mut self, severity: Severity, message: String, at_ms: f64) {
        #[cfg(all(target_arch = "wasm32", not(evani_headless)))]
        crate::web::console_log(severity == Severity::Error, &message);
        if self.entries.len() == self.cap {
            self.entries.pop_front();
        }
        self.entries.push_back(LogEntry {
            severity,
            message,
            at_ms,
        });
        self.revision += 1;
    }

    pub fn error(&mut self, message: impl Into<String>) {
        self.errors += 1;
        self.push(Severity::Error, message.into());
    }

    pub fn note(&mut self, message: impl Into<String>) {
        self.push(Severity::Note, message.into());
    }

    /// Oldest first; at most the last 16.
    pub fn entries(&self) -> impl Iterator<Item = &LogEntry> {
        self.entries.iter()
    }

    /// Errors logged since start (including ones that scrolled out).
    pub fn error_count(&self) -> u32 {
        self.errors
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.revision += 1;
    }
}

/// Facts about the page and device, filled in by the web runner. World
/// resource.
#[derive(Debug, Clone, PartialEq)]
pub struct Platform {
    /// Bytes transferred so far for this page (performance resource entries).
    pub download_bytes: Option<u64>,
    /// `prefers-reduced-motion: reduce`. Games should tone down camera shake,
    /// flashes and parallax when true.
    pub reduced_motion: bool,
    /// Tab hidden (the loop is paused while true).
    pub hidden: bool,
    pub device_pixel_ratio: f32,
    /// `navigator.gpu` present; `None` until checked.
    pub webgpu: Option<bool>,
    /// `true` in the browser, `false` for native/headless runs.
    pub web: bool,
}

impl Default for Platform {
    fn default() -> Self {
        Self {
            download_bytes: None,
            reduced_motion: false,
            hidden: false,
            device_pixel_ratio: 1.0,
            webgpu: None,
            web: cfg!(all(target_arch = "wasm32", not(evani_headless))),
        }
    }
}

/// This page is a development build (the live module is in it): what it
/// downloads is much more than players download. World resource, inserted
/// by evani-live only, so shipped games never carry it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DevBuild {
    /// What players download (the ship build, gzip, every file) according
    /// to the last `cargo xtask size-report`, when the build knew it.
    pub published_gz: Option<u64>,
}

/// The overlay text and its update cadence. World resource.
#[derive(Debug, Clone)]
pub struct Overlay {
    pub enabled: bool,
    /// How often the text is reformatted (ms).
    pub interval_ms: f64,
    text: String,
    revision: u64,
    pub(crate) last_ms: f64,
}

impl Default for Overlay {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_ms: 250.0,
            text: String::with_capacity(1024),
            revision: 0,
            last_ms: f64::NEG_INFINITY,
        }
    }
}

impl Overlay {
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Bumped whenever the text changes.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Replace the text (copies into the reused buffer).
    pub fn set_text(&mut self, s: &str) {
        if self.text != s {
            self.text.clear();
            self.text.push_str(s);
            self.revision += 1;
        }
    }

    pub(crate) fn due(&self, now_ms: f64) -> bool {
        self.enabled && now_ms - self.last_ms >= self.interval_ms
    }
}

/// Numbers for the stats block of the overlay.
#[derive(Debug, Clone, Copy)]
pub struct OverlayStats<'a> {
    pub fps: f32,
    pub cpu_ms: f32,
    pub render: &'a RenderStats,
    pub entities: usize,
    pub tier: &'a str,
    pub budget: BudgetStatus,
    pub download_bytes: Option<u64>,
    /// Set in development builds: the download line says so and shows the
    /// published size next to it.
    pub dev_build: Option<DevBuild>,
    pub hidden: bool,
    /// Adaptive quality: the smoothed work time it judges, its target and
    /// the render-scale multiplier (absent without a renderer).
    pub quality: Option<&'a QualityState>,
    /// GPU medians over the last 2 s ([`GpuWindow`], already updated). When
    /// it has readings, the total GPU line and the passes line show them
    /// instead of the latest frame's.
    pub gpu_median: Option<&'a GpuWindow>,
}

/// `1.2k`, `3.4M`.
fn write_count(out: &mut String, n: u32) {
    let _ = if n >= 1_000_000 {
        write!(out, "{}M", fixed(n as f32 / 1_000_000.0, 1))
    } else if n >= 10_000 {
        write!(out, "{}k", fixed(n as f32 / 1_000.0, 0))
    } else if n >= 1_000 {
        write!(out, "{}k", fixed(n as f32 / 1_000.0, 1))
    } else {
        write!(out, "{n}")
    };
}

/// `812 B`, `14.2 KB`, `1.31 MB`.
pub fn write_bytes(out: &mut String, n: u64) {
    let _ = if n >= 1_048_576 {
        write!(out, "{} MB", fixed(n as f64 / 1_048_576.0, 2))
    } else if n >= 1024 {
        write!(out, "{} KB", fixed(n as f64 / 1024.0, 1))
    } else {
        write!(out, "{n} B")
    };
}

/// Write the stats block (4 lines, each ending in `\n`) into `out`.
pub fn write_stats(out: &mut String, s: &OverlayStats) {
    let _ = write!(out, "fps {}  cpu {} ms  gpu ", fixed(s.fps, 0), fixed(s.cpu_ms, 2));
    let median_total = s.gpu_median.and_then(GpuWindow::total);
    let median_passes = s
        .gpu_median
        .map(GpuWindow::passes)
        .filter(|p| !p.is_empty());
    let median_note = |out: &mut String, median: bool| {
        if median {
            out.push_str(" (median)");
        }
    };
    match s.render.gpu_ms.map(|g| median_total.unwrap_or(g)) {
        Some(g) if s.render.gpu_estimated => {
            let _ = write!(out, "~{} ms", fixed(g, 2));
            median_note(out, median_total.is_some());
        }
        Some(g) => {
            let _ = write!(out, "{} ms", fixed(g, 2));
            median_note(out, median_total.is_some());
        }
        None if s.render.gpu_note.is_empty() => out.push_str("n/a"),
        None => {
            let _ = write!(out, "n/a ({})", s.render.gpu_note);
        }
    }
    if let Some(q) = s.quality
        && let Some(w) = q.work_ms
    {
        let _ = write!(out, "  work {}/{} ms", fixed(w, 2), fixed(q.target_ms, 1));
    }
    if s.hidden {
        out.push_str("  (paused)");
    }
    out.push('\n');
    let _ = write!(out, "draws {}  tris ", s.render.draw_calls);
    write_count(out, s.render.triangles);
    out.push_str("  sprites ");
    write_count(out, s.render.sprites);
    let _ = writeln!(out, "  entities {}", s.entities);
    let _ = write!(out, "tier {}", s.tier);
    // The warm-up's "picked ultra" log line stays on screen after a
    // `tier.set`, a capture script or the menu switched tiers: say so here.
    if let Some(q) = s.quality
        && let Some(pick) = q.warmup_pick.as_deref()
        && pick != s.tier
    {
        let _ = write!(out, " (set after the warm-up picked {pick})");
    }
    let _ = write!(out, "  budget {}/{} pts", s.budget.used, s.budget.total);
    if let Some(q) = s.quality {
        if q.scale < 1.0 {
            let _ = write!(out, "  scale x{}", fixed(q.scale, 2));
        }
        if q.held {
            out.push_str("  (quality holding: capture)");
        } else if q.settling {
            out.push_str("  (quality settling)");
        }
    }
    out.push('\n');
    out.push_str("download ");
    match s.download_bytes {
        Some(b) => write_bytes(out, b),
        None => out.push_str("n/a"),
    }
    if let Some(dev) = s.dev_build {
        out.push_str(" (dev build; published ");
        match dev.published_gz {
            Some(b) => {
                out.push('~');
                write_bytes(out, b);
                out.push_str(" gzip)");
            }
            None => out.push_str("size not measured yet)"),
        }
    }
    out.push('\n');
    if !s.render.passes.is_empty() {
        out.push_str("passes");
        for p in median_passes.unwrap_or(&s.render.passes) {
            let _ = write!(out, "  {} {}", p.name, fixed(p.gpu_ms, 2));
        }
        // Worded so `name number` parsers of this line (live-check) do not
        // read it as a pass.
        if let (Some(_), Some(w)) = (median_passes, s.gpu_median) {
            let _ = write!(out, "  ({} s median)", fixed(w.window_ms / 1000.0, 0));
        }
        out.push('\n');
    }
}

/// `m:ss.s` since the page started.
fn write_clock(out: &mut String, ms: f64) {
    let tenths = (ms.max(0.0) / 100.0).round() as u64;
    let (m, rest) = (tenths / 600, tenths % 600);
    let _ = write!(out, "{m}:{:02}.{}", rest / 10, rest % 10);
}

/// Append the newest `max` log entries, each with its time
/// (`m:ss.s` since the page started).
pub fn write_log(out: &mut String, log: &ErrorLog, max: usize) {
    let n = log.entries.len();
    for e in log.entries.iter().skip(n.saturating_sub(max)) {
        out.push_str(match e.severity {
            Severity::Error => "! ",
            Severity::Note => "· ",
        });
        write_clock(out, e.at_ms);
        out.push(' ');
        out.push_str(&e.message);
        out.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_format() {
        let render = RenderStats {
            gpu_ms: Some(1.5),
            draw_calls: 12,
            triangles: 1_260_000,
            sprites: 4_500,
            passes: vec![PassTiming {
                name: "bloom",
                gpu_ms: 0.25,
            }],
            gpu_note: "",
            gpu_estimated: false,
            gpu_samples: 1,
            particles: 0,
        };
        let mut out = String::new();
        write_stats(
            &mut out,
            &OverlayStats {
                fps: 59.9,
                cpu_ms: 1.234,
                render: &render,
                entities: 340,
                tier: "high",
                budget: BudgetStatus { used: 9, total: 16 },
                download_bytes: Some(1_500_000),
                dev_build: None,
                hidden: false,
                quality: None,
                gpu_median: None,
            },
        );
        assert_eq!(
            out,
            "fps 60  cpu 1.23 ms  gpu 1.50 ms\n\
             draws 12  tris 1.3M  sprites 4.5k  entities 340\n\
             tier high  budget 9/16 pts\n\
             download 1.43 MB\n\
             passes  bloom 0.25\n"
        );
    }

    /// Review follow-up 5a: a development build says so, with what
    /// players really download next to it.
    #[test]
    fn dev_build_download_line() {
        let render = RenderStats::default();
        let stats = OverlayStats {
            fps: 60.0,
            cpu_ms: 1.0,
            render: &render,
            entities: 1,
            tier: "high",
            budget: BudgetStatus::default(),
            download_bytes: Some(5_872_026),
            dev_build: Some(DevBuild {
                published_gz: Some(234_190),
            }),
            hidden: false,
            quality: None,
            gpu_median: None,
        };
        let mut out = String::new();
        write_stats(&mut out, &stats);
        assert!(
            out.contains("download 5.60 MB (dev build; published ~228.7 KB gzip)\n"),
            "{out}"
        );
        out.clear();
        write_stats(
            &mut out,
            &OverlayStats {
                dev_build: Some(DevBuild::default()),
                ..stats
            },
        );
        assert!(
            out.contains("download 5.60 MB (dev build; published size not measured yet)\n"),
            "{out}"
        );
    }

    /// Review captures read "tier warm-up picked ultra" in the log while the
    /// tier line said medium (the capture script's `tier.set`): the tier
    /// line now says the tier was set after the warm-up.
    #[test]
    fn tier_line_names_the_warmup_pick_when_another_tier_is_active() {
        let render = RenderStats::default();
        let mut q = QualityState::default();
        q.warmup_pick = Some("ultra".into());
        let stats = |q: &QualityState, tier: &'static str| {
            let mut out = String::new();
            write_stats(
                &mut out,
                &OverlayStats {
                    fps: 60.0,
                    cpu_ms: 1.0,
                    render: &render,
                    entities: 1,
                    tier,
                    budget: BudgetStatus { used: 6, total: 10 },
                    download_bytes: None,
                    dev_build: None,
                    hidden: false,
                    quality: Some(q),
                    gpu_median: None,
                },
            );
            out
        };
        assert!(
            stats(&q, "medium").contains("tier medium (set after the warm-up picked ultra)  budget 6/10 pts\n"),
            "{}",
            stats(&q, "medium")
        );
        assert!(stats(&q, "ultra").contains("tier ultra  budget"));
        q.warmup_pick = None;
        assert!(stats(&q, "medium").contains("tier medium  budget"));
    }

    #[test]
    fn no_realloc_after_warmup() {
        let render = RenderStats::default();
        let stats = OverlayStats {
            fps: 60.0,
            cpu_ms: 1.0,
            render: &render,
            entities: 1,
            tier: "low",
            budget: BudgetStatus::default(),
            download_bytes: None,
            dev_build: None,
            hidden: true,
            quality: None,
            gpu_median: None,
        };
        let mut out = String::with_capacity(256);
        write_stats(&mut out, &stats);
        let cap = out.capacity();
        let ptr = out.as_ptr();
        for _ in 0..100 {
            out.clear();
            write_stats(&mut out, &stats);
        }
        assert_eq!(out.capacity(), cap);
        assert_eq!(out.as_ptr(), ptr);
        assert!(out.contains("gpu n/a  (paused)"));
        let render = RenderStats {
            gpu_note: "timestamp-query unsupported",
            ..RenderStats::default()
        };
        out.clear();
        write_stats(
            &mut out,
            &OverlayStats {
                render: &render,
                ..stats
            },
        );
        assert!(
            out.contains("gpu n/a (timestamp-query unsupported)"),
            "{out}"
        );
    }

    #[test]
    fn log_ring_and_overlay_revision() {
        let mut log = ErrorLog::default();
        for i in 0..20 {
            log.error(format!("e{i}"));
        }
        log.push_at(Severity::Error, "late".into(), 75_349.0);
        log.push_at(Severity::Note, "hello".into(), 1_500.0);
        assert_eq!(log.entries().count(), 16);
        assert_eq!(log.error_count(), 20);
        let mut out = String::new();
        write_log(&mut out, &log, 2);
        // Follow-up 5b: every line carries its time since the page started.
        assert_eq!(out, "! 1:15.3 late\n· 0:01.5 hello\n");

        let mut o = Overlay::default();
        o.set_text("a");
        o.set_text("a");
        assert_eq!(o.revision(), 1);
        assert_eq!(write_bytes_s(900), "900 B");
        assert_eq!(write_bytes_s(2048), "2.0 KB");
    }

    fn pass(name: &'static str, gpu_ms: f32) -> PassTiming {
        PassTiming { name, gpu_ms }
    }

    fn pass_ms(w: &GpuWindow, name: &str) -> Option<f32> {
        w.passes().iter().find(|p| p.name == name).map(|p| p.gpu_ms)
    }

    /// Gate 0 follow-up: the window is time-based. With each reading's
    /// value equal to its time in seconds, the median at t = 5 s is the
    /// middle of the last 2 s (4.0) at any reading rate, and nothing older
    /// counts.
    #[test]
    fn gpu_median_covers_the_last_two_seconds_at_any_rate() {
        for hz in [30.0, 60.0, 144.0, 240.0] {
            let mut w = GpuWindow::default();
            let step = 1000.0 / hz;
            let mut t = 0.0;
            while t <= 5000.0 {
                let v = (t / 1000.0) as f32;
                w.push(t, Some(v), &[pass("forward", v)]);
                t += step;
            }
            w.update(5000.0, &[pass("forward", 5.0)]);
            let total = w.total().unwrap();
            assert!((total - 4.0).abs() <= 1.5 / hz as f32, "{hz} Hz: {total}");
            let fwd = pass_ms(&w, "forward").unwrap();
            assert!((fwd - 4.0).abs() <= 1.5 / hz as f32, "{hz} Hz: {fwd}");
        }
    }

    /// One slow frame (or a GPU clock dip for a few frames) does not move
    /// the number; a lasting change does, within the window.
    #[test]
    fn gpu_median_ignores_a_spike_and_follows_a_lasting_change() {
        let mut w = GpuWindow::default();
        let mut t = 0.0;
        for i in 0..240 {
            let ms = if i == 200 { 9.0 } else { 1.0 };
            w.push(t, Some(ms), &[pass("bloom.composite", ms * 0.2)]);
            t += 1000.0 / 60.0;
        }
        w.update(t, &[pass("bloom.composite", 1.8)]);
        assert_eq!(w.total(), Some(1.0));
        assert_eq!(pass_ms(&w, "bloom.composite"), Some(0.2));
        // 3 s at 2.0 ms: the 1.0 readings have left the window.
        for _ in 0..180 {
            w.push(t, Some(2.0), &[pass("bloom.composite", 0.4)]);
            t += 1000.0 / 60.0;
        }
        w.update(t, &[pass("bloom.composite", 0.4)]);
        assert_eq!(w.total(), Some(2.0));
        assert_eq!(pass_ms(&w, "bloom.composite"), Some(0.4));
    }

    /// Passes come and go (an effect paused, bloom switched off): a pass
    /// that stopped running drops out, a new one is the median of its own
    /// readings only, and the order follows the latest reading.
    #[test]
    fn gpu_median_handles_changing_pass_sets() {
        let mut w = GpuWindow::default();
        let mut t = 0.0;
        for _ in 0..60 {
            w.push(t, Some(2.0), &[pass("forward", 1.0), pass("bloom.blur", 0.5)]);
            t += 1000.0 / 60.0;
        }
        for _ in 0..30 {
            w.push(t, Some(1.5), &[pass("outline", 0.3), pass("forward", 1.2)]);
            t += 1000.0 / 60.0;
        }
        w.update(t, &[pass("outline", 0.3), pass("forward", 1.2)]);
        let names: Vec<_> = w.passes().iter().map(|p| p.name).collect();
        assert_eq!(names, ["outline", "forward"]);
        assert_eq!(pass_ms(&w, "outline"), Some(0.3));
        assert_eq!(pass_ms(&w, "forward"), Some(1.0)); // 60 of 90 readings at 1.0
        assert_eq!(w.total(), Some(2.0));

        // More pass names over time than columns: a pass with no reading in
        // the window gives its column (cleared) to a new name.
        static NAMES: [&str; 17] = [
            "p0", "p1", "p2", "p3", "p4", "p5", "p6", "p7", "p8", "p9", "p10", "p11", "p12",
            "p13", "p14", "p15", "late",
        ];
        let mut w = GpuWindow::default();
        let all: Vec<_> = NAMES[..16].iter().map(|n| pass(n, 7.0)).collect();
        w.push(0.0, Some(9.0), &all);
        w.push(3000.0, Some(1.0), &[pass("late", 0.25)]);
        w.update(3000.0, &[pass("late", 0.25)]);
        assert_eq!(pass_ms(&w, "late"), Some(0.25));
        assert_eq!(w.total(), Some(1.0));
        // A pass the window never saw shows its latest value.
        w.update(3000.0, &[pass("unseen", 0.5)]);
        assert_eq!(pass_ms(&w, "unseen"), Some(0.5));
    }

    /// One sample per GPU reading, not per frame: readbacks arrive late and
    /// not every frame, so the same reading seen on several frames counts
    /// once.
    #[test]
    fn gpu_median_takes_each_reading_once() {
        let mut w = GpuWindow::default();
        let mut stats = RenderStats {
            gpu_ms: Some(5.0),
            gpu_samples: 1,
            ..RenderStats::default()
        };
        for f in 0..10 {
            w.observe(f64::from(f) * 16.0, &stats);
        }
        stats.gpu_ms = Some(1.0);
        stats.gpu_samples = 2;
        w.observe(170.0, &stats);
        stats.gpu_samples = 3;
        w.observe(190.0, &stats);
        w.update(200.0, &[]);
        assert_eq!(w.total(), Some(1.0)); // 5, 1, 1 (not ten 5s)
    }

    #[test]
    fn gpu_median_never_allocates_after_start() {
        let mut w = GpuWindow::default();
        let ptrs = |w: &GpuWindow| {
            (
                w.times.as_ptr() as usize,
                w.values.as_ptr() as usize,
                w.scratch.as_ptr() as usize,
                w.passes.as_ptr() as usize,
                w.names.as_ptr() as usize,
            )
        };
        let before = ptrs(&w);
        let passes = [pass("forward", 1.0), pass("bloom.blur", 0.5), pass("tonemap", 0.1)];
        // 1000 readings per second for 3 s: more than the ring holds.
        for i in 0..3000 {
            w.push(f64::from(i), Some(1.0), &passes);
            if i % 250 == 0 {
                w.update(f64::from(i), &passes);
            }
        }
        w.update(3000.0, &passes);
        assert_eq!(ptrs(&w), before);
        assert_eq!(w.len, GPU_WINDOW_CAP);
        assert_eq!(w.total(), Some(1.0));
    }

    #[test]
    fn median_of_odd_even_and_empty() {
        assert_eq!(median(&mut []), None);
        assert_eq!(median(&mut [3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median(&mut [4.0, 1.0, 3.0, 2.0]), Some(2.5));
    }

    /// The overlay prints the medians and says so, in words the
    /// live-check parser of the passes line (`name number`) skips.
    #[test]
    fn stats_show_the_median() {
        let render = RenderStats {
            gpu_ms: Some(9.0),
            passes: vec![pass("forward", 8.0), pass("bloom.composite", 0.7)],
            gpu_samples: 3,
            ..RenderStats::default()
        };
        let mut w = GpuWindow::default();
        w.push(0.0, Some(1.5), &[pass("forward", 1.1), pass("bloom.composite", 0.17)]);
        w.push(10.0, Some(1.6), &[pass("forward", 1.2), pass("bloom.composite", 0.18)]);
        w.push(20.0, Some(9.0), &render.passes);
        w.update(20.0, &render.passes);
        let mut out = String::new();
        write_stats(
            &mut out,
            &OverlayStats {
                fps: 60.0,
                cpu_ms: 1.0,
                render: &render,
                entities: 1,
                tier: "high",
                budget: BudgetStatus::default(),
                download_bytes: None,
                dev_build: None,
                hidden: false,
                quality: None,
                gpu_median: Some(&w),
            },
        );
        assert!(out.starts_with("fps 60  cpu 1.00 ms  gpu 1.60 ms (median)\n"), "{out}");
        assert!(
            out.ends_with("passes  forward 1.20  bloom.composite 0.18  (2 s median)\n"),
            "{out}"
        );
    }

    fn write_bytes_s(n: u64) -> String {
        let mut s = String::new();
        write_bytes(&mut s, n);
        s
    }
}
