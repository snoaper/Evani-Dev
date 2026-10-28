//! What a bench run reports and how it is judged. Pure data and arithmetic,
//! so the same code checks a browser GPU run, a native CPU-only run and the
//! unit tests. Every limit comes from the tier preset ([`TierPreset`], i.e.
//! `assets/tiers/*.json`); nothing tier-specific is written here.

use core::fmt::Write;

use evani_core::tiers::TierPreset;
use serde::{Deserialize, Serialize};

/// How far over a tier's frame-time target the average may be and still
/// count as holding it (the same tolerance the first-run warm-up uses).
pub const FRAME_TOLERANCE_PCT: f32 = 10.0;
/// A stress phase has to draw at least this share of what it was sized to
/// draw, otherwise the run proves nothing (content not drawn).
pub const MIN_DRAWN_FRACTION: f32 = 0.8;

/// One phase of a tier's run: the 3D phase (triangles) or the 2D phase
/// (sprites). Frame times are real milliseconds between frames in the
/// browser, CPU milliseconds in a native run.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PhaseResult {
    pub name: String,
    pub frames: u32,
    pub avg_ms: f32,
    pub p95_ms: f32,
    pub p99_ms: f32,
    pub max_ms: f32,
    /// Mean GPU ms (timestamp queries); `None` when unavailable.
    pub gpu_ms: Option<f32>,
    /// Median GPU ms of the measured frames: what the cost order check
    /// judges, because the mean swings with the GPU's clock state and a few
    /// slow frames (see [`tier_order_failures`]).
    #[serde(default)]
    pub gpu_p50_ms: Option<f32>,
    /// Mean GPU ms per pass (timestamp queries), in the order the passes
    /// ran; empty when unavailable.
    #[serde(default)]
    pub gpu_passes: Vec<PassMs>,
    /// GPU readings the GPU numbers come from: one per timestamp readback
    /// that arrived during the measured frames (a readback lands a frame
    /// or more late and is not repeated, so a frame without a new one adds
    /// no sample).
    #[serde(default)]
    pub gpu_samples: u32,
    /// Mean CPU ms the bench itself spent putting the content into the
    /// render world each frame (a diagnostic; not judged).
    #[serde(default)]
    pub submit_ms: f32,
    /// Largest value seen over the measured frames.
    pub draw_calls: u32,
    pub triangles: u32,
    pub sprites: u32,
    pub points_used: u32,
    /// What the phase was sized to draw (about 90% of the limit).
    pub planned_triangles: u32,
    pub planned_sprites: u32,
    /// Particle phase: most live particles seen, and the tier cap it filled.
    #[serde(default)]
    pub particles: u32,
    #[serde(default)]
    pub planned_particles: u32,
}

/// The limits a tier was judged against (echoed so a result stands alone).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Limits {
    pub target_ms: f32,
    pub floor_ms: Option<f32>,
    pub max_draw_calls: u32,
    pub max_triangles: u32,
    pub max_sprites: u32,
    #[serde(default)]
    pub max_particles: u32,
    pub points: u32,
}

impl Limits {
    pub fn of(tier: &TierPreset) -> Self {
        Self {
            target_ms: tier.frame_budget_ms(),
            floor_ms: tier.fps_floor.map(|f| 1000.0 / f.max(1) as f32),
            max_draw_calls: tier.max_draw_calls,
            max_triangles: tier.max_triangles,
            max_sprites: tier.max_sprites,
            max_particles: tier.max_particles,
            points: tier.points,
        }
    }
}

/// One row of the table.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TierResult {
    pub tier: String,
    pub label: String,
    /// `browser-gpu` or `native-cpu`.
    pub mode: String,
    /// Measured frames over both phases.
    pub frames: u32,
    /// Frame-time columns: the worse of the two phases.
    pub avg_ms: f32,
    pub p95_ms: f32,
    pub p99_ms: f32,
    pub gpu_ms: Option<f32>,
    /// Median GPU ms per frame (the larger of the two phases); over several
    /// runs, the median of the runs. The cost order check judges this.
    #[serde(default)]
    pub gpu_p50_ms: Option<f32>,
    /// Over several runs: the lowest and highest run median (the spread
    /// behind `gpu_p50_ms`); `None` for a single run.
    #[serde(default)]
    pub gpu_p50_min_ms: Option<f32>,
    #[serde(default)]
    pub gpu_p50_max_ms: Option<f32>,
    pub draw_calls: u32,
    /// Largest in the 3D phase.
    pub triangles: u32,
    /// Largest in the 2D phase.
    pub sprites: u32,
    /// Largest in the particle phase.
    #[serde(default)]
    pub particles: u32,
    pub points_used: u32,
    pub points_total: u32,
    /// Mean GPU ms per pass of the 3D phase (the scene passes).
    #[serde(default)]
    pub gpu_passes: Vec<PassMs>,
    /// Render target size in pixels (browser runs).
    pub render_size: Option<[u32; 2]>,
    pub limits: Limits,
    pub phases: Vec<PhaseResult>,
    pub pass: bool,
    /// Names of the failed checks, each with the numbers.
    pub failures: Vec<String>,
    /// Extra remark for the table (VR: mono, preset only).
    pub note: String,
}

/// One pass's GPU time.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PassMs {
    pub name: String,
    /// Mean ms per GPU reading (a reading without the pass counts as 0).
    pub ms: f32,
    /// Median ms over the readings the pass ran in; over several runs, the
    /// median of the run medians.
    #[serde(default)]
    pub p50_ms: Option<f32>,
    /// Over several runs: the lowest and highest run median.
    #[serde(default)]
    pub p50_min_ms: Option<f32>,
    #[serde(default)]
    pub p50_max_ms: Option<f32>,
}

/// Per-pass mean and median from one phase's readings. `samples` holds each
/// pass's ms per reading it ran in (sorted in place); `readings` is how
/// many GPU readings there were in all (the mean's divisor).
pub fn pass_stats(samples: &mut [(&'static str, Vec<f32>)], readings: usize) -> Vec<PassMs> {
    let n = readings.max(1) as f64;
    samples
        .iter_mut()
        .map(|(name, v)| {
            let sum: f64 = v.iter().map(|x| f64::from(*x)).sum();
            v.sort_by(|a, b| a.total_cmp(b));
            PassMs {
                name: (*name).to_string(),
                ms: (sum / n) as f32,
                p50_ms: (!v.is_empty()).then(|| median_sorted(v)),
                ..PassMs::default()
            }
        })
        .collect()
}

/// The tiers whose GPU cost must rise in this order (VR is mono here and
/// runs at its own size, so it is not part of the order).
pub const COST_ORDER: [&str; 5] = ["lite", "low", "medium", "high", "ultra"];

/// How much cheaper (in %) a tier's GPU time may measure than the tier
/// below it before the order check fails. Run-to-run noise of one tier's
/// mean GPU time on the same machine is a few percent (clock boost,
/// browser compositing, timestamp rounding); 10 % is about twice that, and
/// far under the real step between two tiers (the next tier draws two to
/// four times the triangles at the same or a larger size).
pub const ORDER_TOLERANCE_PCT: f32 = 10.0;

/// Below this many ms a difference is under the timestamp resolution and
/// is never an order failure.
pub const ORDER_TOLERANCE_MS: f32 = 0.05;

/// The GPU cost order check: every tier of [`COST_ORDER`] that was run
/// must cost at least as much GPU time as the one below it, within the
/// tolerance. Returns one line per tier that breaks the order (empty when
/// it holds, or when GPU times are missing).
///
/// It judges the median GPU ms per frame ([`TierResult::gpu_p50_ms`], the
/// median of the runs when there are several), falling back to the mean.
/// GPU timestamps on a desktop card swing several times over with its clock
/// state (a lightly loaded GPU clocks down, and every pass then measures
/// slower), and a few spiking frames move a mean but not a median. The
/// bench also runs uncapped (no vsync) and settles after the last pipeline
/// compile, which keeps the clock up while it measures.
pub fn tier_order_failures(results: &[TierResult]) -> Vec<String> {
    let ordered: Vec<(&str, f32)> = COST_ORDER
        .iter()
        .filter_map(|id| results.iter().find(|r| r.tier == *id))
        .filter_map(|r| r.gpu_p50_ms.or(r.gpu_ms).map(|g| (r.tier.as_str(), g)))
        .collect();
    order_failures(&ordered)
}

/// The order rule on its own: `ordered` is `(tier, GPU ms)` cheapest tier
/// first; each must be no more than [`ORDER_TOLERANCE_PCT`] (and
/// [`ORDER_TOLERANCE_MS`]) under the one before it. One line per break.
/// The review card's cost-order check uses this too, so both judge alike.
pub fn order_failures(ordered: &[(&str, f32)]) -> Vec<String> {
    let mut out = Vec::new();
    for w in ordered.windows(2) {
        let ((lo, a), (hi, b)) = (w[0], w[1]);
        let allowed = (a * (1.0 - ORDER_TOLERANCE_PCT / 100.0)).min(a - ORDER_TOLERANCE_MS);
        if b < allowed {
            out.push(format!(
                "GPU cost order: {hi} {b:.2} ms is under {lo} {a:.2} ms (more than {ORDER_TOLERANCE_PCT:.0}% cheaper)"
            ));
        }
    }
    out
}

/// Median and min/max of the present values (`None`s when there are none).
fn median_spread(mut v: Vec<f32>) -> (Option<f32>, Option<f32>, Option<f32>) {
    if v.is_empty() {
        return (None, None, None);
    }
    v.sort_by(|a, b| a.total_cmp(b));
    (Some(median_sorted(&v)), v.first().copied(), v.last().copied())
}

/// The mean of several runs, tier by tier (first run's order): GPU ms,
/// frame times and per-pass GPU ms are averaged over the runs that have
/// them; the medians (whole frame and per pass) become the median of the
/// run medians, with the lowest and highest run median as the spread when
/// there is more than one run; a tier passes only if it passed in every
/// run.
pub fn mean_results(runs: &[Vec<TierResult>]) -> Vec<TierResult> {
    let Some(first) = runs.first() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for base in first {
        let all: Vec<&TierResult> = runs.iter().filter_map(|r| r.iter().find(|t| t.tier == base.tier)).collect();
        let n = all.len() as f32;
        let mean = |f: fn(&TierResult) -> f32| all.iter().map(|t| f(t)).sum::<f32>() / n;
        let gpus: Vec<f32> = all.iter().filter_map(|t| t.gpu_ms).collect();
        let mut m = base.clone();
        m.avg_ms = mean(|t| t.avg_ms);
        m.p95_ms = mean(|t| t.p95_ms);
        m.p99_ms = mean(|t| t.p99_ms);
        m.gpu_ms = (!gpus.is_empty()).then(|| gpus.iter().sum::<f32>() / gpus.len() as f32);
        let several = all.len() > 1;
        let (p50, lo, hi) = median_spread(all.iter().filter_map(|t| t.gpu_p50_ms).collect());
        m.gpu_p50_ms = p50;
        (m.gpu_p50_min_ms, m.gpu_p50_max_ms) = if several { (lo, hi) } else { (None, None) };
        m.gpu_passes = base
            .gpu_passes
            .iter()
            .map(|p| {
                let runs: Vec<&PassMs> = all
                    .iter()
                    .filter_map(|t| t.gpu_passes.iter().find(|q| q.name == p.name))
                    .collect();
                let (p50, lo, hi) = median_spread(runs.iter().filter_map(|q| q.p50_ms).collect());
                PassMs {
                    name: p.name.clone(),
                    ms: runs.iter().map(|q| q.ms).sum::<f32>() / runs.len().max(1) as f32,
                    p50_ms: p50,
                    p50_min_ms: if several { lo } else { None },
                    p50_max_ms: if several { hi } else { None },
                }
            })
            .collect();
        m.pass = all.iter().all(|t| t.pass);
        m.failures = all.iter().flat_map(|t| t.failures.iter().cloned()).collect();
        m.failures.dedup();
        out.push(m);
    }
    out
}

/// `forward 0.52, outline 0.10, ...`
pub fn format_passes(passes: &[PassMs]) -> String {
    passes
        .iter()
        .map(|p| format!("{} {:.2}", p.name, p.ms))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Median of a sorted, non-empty slice (the mean of the middle two when
/// the length is even).
pub fn median_sorted(sorted: &[f32]) -> f32 {
    let n = sorted.len();
    if n % 2 == 1 { sorted[n / 2] } else { (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0 }
}

/// Percentile by nearest rank on a sorted slice (0 for an empty one).
pub fn percentile(sorted: &[f32], p: f32) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (p / 100.0 * sorted.len() as f32).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

/// Summarise one phase's samples. `frame_ms` is consumed (sorted in place).
pub fn summarize_phase(name: &str, frame_ms: &mut [f32], gpu_ms: &[f32]) -> PhaseResult {
    frame_ms.sort_by(|a, b| a.total_cmp(b));
    let n = frame_ms.len();
    let avg = if n == 0 { 0.0 } else { frame_ms.iter().sum::<f32>() / n as f32 };
    PhaseResult {
        name: name.to_string(),
        frames: n as u32,
        avg_ms: avg,
        p95_ms: percentile(frame_ms, 95.0),
        p99_ms: percentile(frame_ms, 99.0),
        max_ms: frame_ms.last().copied().unwrap_or(0.0),
        gpu_ms: (!gpu_ms.is_empty()).then(|| gpu_ms.iter().sum::<f32>() / gpu_ms.len() as f32),
        gpu_p50_ms: (!gpu_ms.is_empty()).then(|| {
            let mut g = gpu_ms.to_vec();
            g.sort_by(|a, b| a.total_cmp(b));
            median_sorted(&g)
        }),
        ..PhaseResult::default()
    }
}

/// Judge a tier. `phases` is `[3D, 2D]`; `render_errors` is how many errors
/// the renderer logged during the tier (with the first message).
pub fn evaluate(
    tier: &TierPreset,
    phases: &[PhaseResult],
    render_errors: Option<(u32, String)>,
) -> TierResult {
    let limits = Limits::of(tier);
    let worst = |f: fn(&PhaseResult) -> f32| phases.iter().map(f).fold(0.0f32, f32::max);
    let by_name = |n: &str| phases.iter().find(|p| p.name == n);
    let triangles = by_name("3D").map_or(0, |p| p.triangles).max(by_name("models").map_or(0, |p| p.triangles)).max(by_name("characters").map_or(0, |p| p.triangles));
    let sprites = by_name("2D").map_or(0, |p| p.sprites);
    let particles = by_name("particles").map_or(0, |p| p.particles);
    let draw_calls = phases.iter().map(|p| p.draw_calls).max().unwrap_or(0);
    let points_used = phases.iter().map(|p| p.points_used).max().unwrap_or(0);
    let gpu_ms = phases
        .iter()
        .filter_map(|p| p.gpu_ms)
        .fold(None, |m: Option<f32>, v| Some(m.map_or(v, |m| m.max(v))));
    let gpu_p50_ms = phases
        .iter()
        .filter_map(|p| p.gpu_p50_ms)
        .fold(None, |m: Option<f32>, v| Some(m.map_or(v, |m| m.max(v))));
    let (avg_ms, p95_ms, p99_ms) = (worst(|p| p.avg_ms), worst(|p| p.p95_ms), worst(|p| p.p99_ms));

    let mut failures = Vec::new();
    if phases.is_empty() || phases.iter().any(|p| p.frames == 0) {
        failures.push("no frames measured".to_string());
    }
    let allowed = limits.target_ms * (1.0 + FRAME_TOLERANCE_PCT / 100.0);
    if avg_ms > allowed {
        failures.push(format!(
            "frame-time target (avg {avg_ms:.2} ms > {:.2} ms for {} fps +{FRAME_TOLERANCE_PCT:.0}%)",
            allowed, tier.fps_target
        ));
    }
    if let Some(floor_ms) = limits.floor_ms
        && p99_ms > floor_ms
    {
        failures.push(format!(
            "frame-time floor (p99 {p99_ms:.2} ms > {floor_ms:.2} ms)"
        ));
    }
    if draw_calls > limits.max_draw_calls {
        failures.push(format!("draw calls ({draw_calls} > {})", limits.max_draw_calls));
    }
    if triangles > limits.max_triangles {
        failures.push(format!("triangles ({triangles} > {})", limits.max_triangles));
    }
    if sprites > limits.max_sprites {
        failures.push(format!("sprites ({sprites} > {})", limits.max_sprites));
    }
    if particles > limits.max_particles {
        failures.push(format!("particles ({particles} > {})", limits.max_particles));
    }
    if points_used > limits.points {
        failures.push(format!("budget points ({points_used} > {})", limits.points));
    }
    // The run must really have drawn what it was sized to draw.
    for p in phases {
        let (got, want, what) = match p.name.as_str() {
            "3D" | "models" | "characters" => (p.triangles, p.planned_triangles, "triangles"),
            "particles" => (p.particles, p.planned_particles, "particles"),
            _ => (p.sprites, p.planned_sprites, "sprites"),
        };
        if (got as f32) < want as f32 * MIN_DRAWN_FRACTION {
            failures.push(format!(
                "content not drawn ({} phase drew {got} of {want} {what})",
                p.name
            ));
        }
    }
    if let Some((n, first)) = render_errors
        && n > 0
    {
        failures.push(format!("render errors ({n}: {first})"));
    }

    TierResult {
        tier: tier.id.clone(),
        label: tier.label.clone(),
        mode: String::new(),
        frames: phases.iter().map(|p| p.frames).sum(),
        avg_ms,
        p95_ms,
        p99_ms,
        gpu_ms,
        gpu_p50_ms,
        gpu_p50_min_ms: None,
        gpu_p50_max_ms: None,
        gpu_passes: by_name("3D").map(|p| p.gpu_passes.clone()).unwrap_or_default(),
        draw_calls,
        triangles,
        sprites,
        particles,
        points_used,
        points_total: limits.points,
        render_size: None,
        limits,
        phases: phases.to_vec(),
        pass: failures.is_empty(),
        failures,
        note: if tier.stereo { "(mono, preset only)".into() } else { String::new() },
    }
}

/// Checks a stress-scene run is judged on. The stress scene is the same for
/// every tier and deliberately heavier than a tier's own limits, so frame
/// time and the count limits say nothing there; only whether it really ran
/// and drew what it should.
pub const STRESS_CHECKS: [&str; 3] = ["no frames", "content not drawn", "render errors"];

/// Judge a stress-scene tier (`?stress=1`): [`evaluate`], keeping only the
/// [`STRESS_CHECKS`] failures.
pub fn evaluate_stress(
    tier: &TierPreset,
    phases: &[PhaseResult],
    render_errors: Option<(u32, String)>,
) -> TierResult {
    let mut r = evaluate(tier, phases, render_errors);
    r.failures.retain(|f| STRESS_CHECKS.iter().any(|c| f.starts_with(c)));
    r.pass = r.failures.is_empty();
    r.note = if r.note.is_empty() { "stress scene".into() } else { format!("stress scene {}", r.note) };
    r
}

/// `0.52` or `0.52 (0.49-0.57)` when there is a run spread.
fn p50_text(p50: Option<f32>, lo: Option<f32>, hi: Option<f32>) -> String {
    match (p50, lo, hi) {
        (Some(m), Some(a), Some(b)) => format!("{m:.3} ({a:.3}-{b:.3})"),
        (Some(m), _, _) => format!("{m:.3}"),
        _ => "n/a".to_string(),
    }
}

/// The stress-scene table: per tier the median GPU ms per frame and per
/// pass (median of the run medians, with the run spread in brackets), and
/// the order check on the frame medians.
pub fn format_stress_table(results: &[TierResult]) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "stress scene: median GPU ms per frame, median of the run medians (lowest-highest run median)"
    );
    for r in results {
        let total = p50_text(r.gpu_p50_ms, r.gpu_p50_min_ms, r.gpu_p50_max_ms);
        let samples: u32 = r.phases.iter().map(|p| p.gpu_samples).sum();
        // The fill layers are the sprites pass; what is left is the scene and
        // the tier's effects (difference of medians, for reading only).
        let scene = r
            .gpu_p50_ms
            .zip(r.gpu_passes.iter().find(|p| p.name == "sprites").and_then(|p| p.p50_ms))
            .map_or(String::new(), |(t, s)| format!("  without the fill layers ~{:.3}", t - s));
        let _ = writeln!(
            out,
            "{:<7} total {total}{scene}  ({} frames, {samples} GPU readings, render {}){}",
            r.tier,
            r.frames,
            r.render_size.map_or("n/a".to_string(), |[w, h]| format!("{w}x{h}")),
            if r.pass { String::new() } else { format!("  FAIL {}", r.failures.join("; ")) }
        );
        for p in &r.gpu_passes {
            let v = p50_text(p.p50_ms, p.p50_min_ms, p.p50_max_ms);
            let _ = writeln!(out, "        {:<18} {v}", p.name);
        }
    }
    let order = tier_order_failures(results);
    let judged = COST_ORDER.iter().filter(|id| results.iter().any(|r| r.tier == **id && r.gpu_p50_ms.is_some())).count();
    if judged >= 2 {
        if order.is_empty() {
            let _ = writeln!(
                out,
                "GPU cost order (stress scene): holds (each tier within {ORDER_TOLERANCE_PCT:.0}% / {ORDER_TOLERANCE_MS} ms of or above the one below)"
            );
        } else {
            for f in order {
                let _ = writeln!(out, "FAIL {f} (stress scene)");
            }
        }
    }
    out
}

/// `1.2k`, `3.4M` (compact counts for the table).
fn count(n: u32) -> String {
    if n >= 1_000_000 {
        format!("{:.2}M", n as f32 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f32 / 1_000.0)
    } else {
        n.to_string()
    }
}

/// The table printed in the terminal and shown on the page.
pub fn format_table(results: &[TierResult]) -> String {
    format_table_judging(results, None)
}

/// The short names of a result's failed checks (`frame-time target, ...`).
fn failure_names(r: &TierResult) -> String {
    r.failures
        .iter()
        .map(|f| f.split(" (").next().unwrap_or(f.as_str()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Device mode's verdict (`?device=1`): only the tier the device's own
/// first-run warm-up picked is judged. `device picked lite: PASS`, or
/// `FAIL` with the failed checks, or `measuring` before its row is in. The
/// short verdict comes first on a line of its own, so it fits a phone held
/// upright; the explanation follows on the next line.
pub fn device_line(pick: &str, results: &[TierResult]) -> String {
    let verdict = match results.iter().find(|r| r.tier == pick) {
        None => "measuring".to_string(),
        Some(r) if r.pass => "PASS".to_string(),
        Some(r) => format!("FAIL {}", failure_names(r)),
    };
    format!("device picked {pick}: {verdict}\n(only {pick} is judged; the other tiers are information)")
}

/// [`format_table`]; with `judged` (device mode), every other tier's
/// result reads `info PASS` / `info FAIL ...`: listed, not judged.
pub fn format_table_judging(results: &[TierResult], judged: Option<&str>) -> String {
    let mut out = String::new();
    let mode = results.first().map_or("", |r| r.mode.as_str());
    let _ = writeln!(
        out,
        "{:<7} {:>6} {:>8} {:>8} {:>8} {:>8} {:>6} {:>8} {:>8} {:>9} {:>7}  result",
        "tier", "frames", "avg ms", "p95 ms", "p99 ms", "gpu ms", "draws", "tris", "sprites", "particles", "budget"
    );
    for r in results {
        let gpu = r.gpu_ms.map_or("n/a".to_string(), |g| format!("{g:.2}"));
        let mut verdict = if r.pass { "PASS".to_string() } else { format!("FAIL {}", failure_names(r)) };
        if judged.is_some_and(|j| j != r.tier) {
            verdict.insert_str(0, "info ");
        }
        let _ = writeln!(
            out,
            "{:<7} {:>6} {:>8.2} {:>8.2} {:>8.2} {:>8} {:>6} {:>8} {:>8} {:>9} {:>7}  {verdict}",
            r.tier,
            r.frames,
            r.avg_ms,
            r.p95_ms,
            r.p99_ms,
            gpu,
            r.draw_calls,
            count(r.triangles),
            count(r.sprites),
            count(r.particles),
            format!("{}/{}", r.points_used, r.points_total),
        );
    }
    let _ = if mode == "native-cpu" {
        writeln!(out, "avg/p95/p99 are the engine's CPU ms per frame (native run, no GPU); worse of the 3D, 2D and particle phase")
    } else {
        writeln!(out, "avg/p95/p99 are real ms between frames; worse of the 3D, 2D and particle phase; tris from the 3D phase, sprites from the 2D phase, particles (live) from the particle phase")
    };
    for r in results.iter().filter(|r| !r.gpu_passes.is_empty()) {
        let _ = writeln!(out, "gpu passes {} (3D phase): {}", r.tier, format_passes(&r.gpu_passes));
    }
    for r in results {
        let parts = r.phases.iter().find(|p| p.name == "particles").filter(|p| !p.gpu_passes.is_empty());
        if let Some(p) = parts {
            let _ = writeln!(out, "gpu passes {} (particle phase): {}", r.tier, format_passes(&p.gpu_passes));
        }
    }
    for r in results {
        if !r.note.is_empty() {
            let _ = writeln!(out, "note {}: {}", r.tier, r.note);
        }
    }
    for r in results.iter().filter(|r| !r.pass) {
        let info = if judged.is_some_and(|j| j != r.tier) { "info " } else { "" };
        for f in &r.failures {
            let _ = writeln!(out, "{info}FAIL {}: {f}", r.tier);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::tiers::Tiers;

    fn tier(id: &str) -> TierPreset {
        Tiers::builtin().unwrap().get(id).cloned().unwrap()
    }

    fn phase(name: &str, avg: f32, tris: u32, sprites: u32) -> PhaseResult {
        PhaseResult {
            name: name.into(),
            frames: 100,
            avg_ms: avg,
            p95_ms: avg,
            p99_ms: avg,
            max_ms: avg,
            draw_calls: 8,
            triangles: tris,
            sprites,
            points_used: 5,
            planned_triangles: tris,
            planned_sprites: sprites,
            ..PhaseResult::default()
        }
    }

    #[test]
    fn percentiles_use_nearest_rank() {
        let v: Vec<f32> = (1..=100).map(|x| x as f32).collect();
        assert_eq!(percentile(&v, 95.0), 95.0);
        assert_eq!(percentile(&v, 99.0), 99.0);
        assert_eq!(percentile(&v, 100.0), 100.0);
        assert_eq!(percentile(&[], 99.0), 0.0);
        assert_eq!(percentile(&[4.0], 1.0), 4.0);
    }

    #[test]
    fn summarize_sorts_and_averages() {
        let mut f = [3.0, 1.0, 2.0, 10.0];
        let p = summarize_phase("3D", &mut f, &[1.0, 3.0]);
        assert_eq!(p.frames, 4);
        assert_eq!(p.avg_ms, 4.0);
        assert_eq!(p.max_ms, 10.0);
        assert_eq!(p.gpu_ms, Some(2.0));
        assert!(p.gpu_p50_ms.is_some());
        assert_eq!(summarize_phase("2D", &mut [], &[]).gpu_ms, None);
    }

    #[test]
    fn a_good_run_passes() {
        let t = tier("high");
        let r = evaluate(&t, &[phase("3D", 8.0, 1_800_000, 0), phase("2D", 9.0, 5, 90_000)], None);
        assert!(r.pass, "{:?}", r.failures);
        assert_eq!((r.triangles, r.sprites, r.avg_ms), (1_800_000, 90_000, 9.0));
        assert_eq!(r.points_total, 16);
    }

    #[test]
    fn each_limit_fails_by_name() {
        let t = tier("low");
        // Slow, over every count limit, over the points, and with an error.
        let mut a = phase("3D", 40.0, 120_000, 0);
        a.points_used = 9;
        a.draw_calls = 151;
        a.planned_triangles = 90_000;
        let mut b = phase("2D", 12.0, 5, 6_000);
        b.planned_sprites = 4_500;
        let r = evaluate(&t, &[a, b], Some((2, "boom".into())));
        assert!(!r.pass);
        let text = r.failures.join(" | ");
        for want in [
            "frame-time target",
            "frame-time floor",
            "draw calls",
            "triangles (120000 > 100000)",
            "sprites (6000 > 5000)",
            "budget points (9 > 7)",
            "render errors (2: boom)",
        ] {
            assert!(text.contains(want), "{want} missing in {text}");
        }
    }

    #[test]
    fn floor_only_where_the_json_gives_one() {
        // Only low has a floor (30 fps = 33.33 ms); a 20 ms p99 passes it.
        let mut p = phase("3D", 10.0, 90_000, 0);
        p.p99_ms = 20.0;
        let r = evaluate(&tier("low"), &[p.clone(), phase("2D", 10.0, 5, 4_500)], None);
        assert!(r.pass, "{:?}", r.failures);
        p.p99_ms = 40.0;
        let r = evaluate(&tier("low"), &[p.clone(), phase("2D", 10.0, 5, 4_500)], None);
        assert!(r.failures.iter().any(|f| f.starts_with("frame-time floor")));
        assert!(tier("high").fps_floor.is_none());
        let r = evaluate(&tier("high"), &[p, phase("2D", 10.0, 5, 90_000)], None);
        assert!(!r.failures.iter().any(|f| f.starts_with("frame-time floor")));
    }

    #[test]
    fn vacuous_runs_fail() {
        let t = tier("medium");
        let mut a = phase("3D", 5.0, 100, 0);
        a.planned_triangles = 360_000;
        let r = evaluate(&t, &[a, phase("2D", 5.0, 5, 18_000)], None);
        assert!(r.failures.iter().any(|f| f.starts_with("content not drawn")));
        assert!(!evaluate(&t, &[], None).pass);
    }

    #[test]
    fn table_lists_every_row_and_the_vr_note() {
        let ok = |id: &str| {
            let t = tier(id);
            let mut r = evaluate(&t, &[phase("3D", 5.0, 1, 0), phase("2D", 5.0, 5, 1)], None);
            r.mode = "browser-gpu".into();
            r
        };
        let table = format_table(&[ok("low"), ok("vr")]);
        assert!(table.contains("low"));
        assert!(table.contains("PASS"));
        assert!(table.contains("note vr: (mono, preset only)"));
        let bad = evaluate(&tier("low"), &[phase("3D", 99.0, 90_000, 0), phase("2D", 99.0, 5, 4_500)], None);
        let table = format_table(std::slice::from_ref(&bad));
        assert!(table.contains("FAIL frame-time target"), "{table}");
        // Device mode: only the picked tier is judged; the rest is information.
        let rows = [ok("lite"), bad, ok("vr")];
        let table = format_table_judging(&rows, Some("lite"));
        assert!(table.contains("  PASS\n"), "{table}");
        assert!(table.contains("info FAIL frame-time target") && table.contains("info PASS"), "{table}");
        assert!(table.contains("info FAIL low: frame-time target") && !table.contains("\nFAIL low:"), "{table}");
        assert!(format_table_judging(&rows, Some("low")).contains("\nFAIL low: frame-time target"));
        assert!(device_line("lite", &rows).starts_with("device picked lite: PASS\n"));
        assert!(device_line("low", &rows).starts_with("device picked low: FAIL frame-time target"), "{}", device_line("low", &rows));
        assert!(device_line("medium", &rows).starts_with("device picked medium: measuring"));
    }

    fn gpu(tier: &str, ms: f32) -> TierResult {
        TierResult {
            tier: tier.into(),
            gpu_ms: Some(ms),
            pass: true,
            gpu_passes: vec![PassMs { name: "forward".into(), ms: ms * 0.5, ..PassMs::default() }],
            ..TierResult::default()
        }
    }

    #[test]
    fn pass_stats_give_mean_per_reading_and_median_where_it_ran() {
        let mut s = vec![("forward", vec![1.0, 9.0, 2.0, 3.0]), ("bloom.composite", vec![0.5, 0.2])];
        let p = pass_stats(&mut s, 4);
        assert_eq!(p[0].name, "forward");
        assert_eq!(p[0].ms, 3.75);
        // A spike moves the mean, not the median.
        assert_eq!(p[0].p50_ms, Some(2.5));
        // Present in 2 of 4 readings: mean counts the others as 0.
        assert!((p[1].ms - 0.175).abs() < 1e-6, "{}", p[1].ms);
        assert!((p[1].p50_ms.unwrap() - 0.35).abs() < 1e-6);
        assert!(pass_stats(&mut [("x", Vec::new())], 0)[0].p50_ms.is_none());
    }

    #[test]
    fn runs_give_the_median_of_run_medians_with_the_spread() {
        let run = |total: f32, fwd: f32| {
            let mut t = gpu("high", total);
            t.gpu_p50_ms = Some(total);
            t.gpu_passes[0].p50_ms = Some(fwd);
            vec![t]
        };
        let m = mean_results(&[run(2.0, 1.0), run(2.4, 1.3), run(1.9, 0.8), run(2.1, 1.1), run(5.0, 3.0)]);
        assert_eq!(m[0].gpu_p50_ms, Some(2.1));
        assert_eq!((m[0].gpu_p50_min_ms, m[0].gpu_p50_max_ms), (Some(1.9), Some(5.0)));
        let f = &m[0].gpu_passes[0];
        assert_eq!((f.p50_ms, f.p50_min_ms, f.p50_max_ms), (Some(1.1), Some(0.8), Some(3.0)));
        // One run: no spread.
        let one = mean_results(&[run(2.0, 1.0)]);
        assert_eq!((one[0].gpu_p50_min_ms, one[0].gpu_passes[0].p50_max_ms), (None, None));
    }

    #[test]
    fn order_rule_is_shared_and_pure() {
        assert!(order_failures(&[("low", 1.0), ("medium", 0.91), ("high", 2.0)]).is_empty());
        let f = order_failures(&[("low", 1.0), ("medium", 0.89)]);
        assert_eq!(f.len(), 1, "{f:?}");
        assert!(f[0].contains("medium 0.89") && f[0].contains("low 1.00"), "{f:?}");
        // Under the timestamp resolution never fails.
        assert!(order_failures(&[("low", 0.30), ("medium", 0.26)]).is_empty());
        assert!(order_failures(&[]).is_empty());
    }

    #[test]
    fn a_stress_run_is_judged_only_on_running_and_drawing() {
        // Far over Low's frame time and triangle limit: not judged here.
        let mut a = phase("3D", 40.0, 360_000, 0);
        a.draw_calls = 999;
        let r = evaluate_stress(&tier("low"), &[a.clone()], None);
        assert!(r.pass, "{:?}", r.failures);
        assert_eq!(r.note, "stress scene");
        a.planned_triangles = 360_000;
        a.triangles = 1_000;
        let r = evaluate_stress(&tier("low"), &[a], Some((1, "lost".into())));
        assert!(!r.pass);
        assert_eq!(r.failures.len(), 2, "{:?}", r.failures);
        assert!(r.failures[0].starts_with("content not drawn"));
        assert!(r.failures[1].starts_with("render errors"));
        assert!(!evaluate_stress(&tier("low"), &[], None).pass);
    }

    #[test]
    fn stress_table_shows_medians_spread_and_the_order() {
        let t = |id: &str, ms: f32| {
            let mut r = gpu(id, ms);
            r.gpu_p50_ms = Some(ms);
            r.gpu_p50_min_ms = Some(ms * 0.9);
            r.gpu_p50_max_ms = Some(ms * 1.1);
            r.gpu_passes[0].p50_ms = Some(ms / 2.0);
            r
        };
        let table = format_stress_table(&[t("low", 1.0), t("medium", 2.0), t("high", 3.0), t("ultra", 6.0)]);
        assert!(table.contains("low     total 1.000 (0.900-1.100)"), "{table}");
        assert!(table.contains("forward            3.000"), "{table}");
        assert!(table.contains("GPU cost order (stress scene): holds"), "{table}");
        let table = format_stress_table(&[t("low", 2.0), t("medium", 1.0)]);
        assert!(table.contains("FAIL GPU cost order: medium 1.00 ms is under low 2.00 ms"), "{table}");
    }

    #[test]
    fn gpu_cost_order_holds_within_the_tolerance() {
        let ok = [gpu("lite", 0.3), gpu("low", 0.5), gpu("medium", 1.0), gpu("high", 2.0), gpu("ultra", 6.0), gpu("vr", 0.1)];
        assert_eq!(tier_order_failures(&ok), Vec::<String>::new());
        // 5 % cheaper is noise; under the timestamp resolution too.
        let noise = [gpu("low", 1.0), gpu("medium", 0.95), gpu("high", 0.92)];
        assert_eq!(tier_order_failures(&noise), Vec::<String>::new());
        let tiny = [gpu("low", 0.12), gpu("medium", 0.08)];
        assert_eq!(tier_order_failures(&tiny), Vec::<String>::new());
        // Low costing more than High (the kart racer's 2.53 vs 1.72 ms) fails.
        let bad = [gpu("low", 2.53), gpu("high", 1.72), gpu("ultra", 3.0)];
        let f = tier_order_failures(&bad);
        assert_eq!(f.len(), 1, "{f:?}");
        assert!(f[0].contains("high 1.72") && f[0].contains("low 2.53"), "{f:?}");
        // Missing GPU times: nothing to judge.
        let none = [TierResult { tier: "low".into(), ..TierResult::default() }, gpu("high", 1.0)];
        assert_eq!(tier_order_failures(&none), Vec::<String>::new());
    }

    #[test]
    fn the_order_check_judges_medians_not_spiky_means() {
        // Low's mean is pushed up by a few clocked-down frames; its median
        // is not. The check judges the median.
        let mut low = gpu("low", 2.5);
        low.gpu_p50_ms = Some(0.3);
        let mut high = gpu("high", 1.7);
        high.gpu_p50_ms = Some(1.0);
        assert_eq!(tier_order_failures(&[low.clone(), high.clone()]), Vec::<String>::new());
        // A real inversion of the medians still fails.
        low.gpu_p50_ms = Some(1.5);
        assert_eq!(tier_order_failures(&[low, high]).len(), 1);
        // Over runs: the median of the runs, so one run at low clocks is outvoted.
        let run = |l: f32| {
            let mut a = gpu("low", l);
            a.gpu_p50_ms = Some(l);
            vec![a]
        };
        let m = mean_results(&[run(0.3), run(3.0), run(0.25)]);
        assert_eq!(m[0].gpu_p50_ms, Some(0.3));
        let frames = summarize_phase("3D", &mut [1.0, 1.0, 1.0, 1.0], &[0.2, 0.25, 0.3, 4.0]);
        assert_eq!(frames.gpu_p50_ms, Some(0.275));
    }

    #[test]
    fn runs_are_averaged_tier_by_tier() {
        let a = vec![gpu("low", 1.0), gpu("high", 3.0)];
        let mut b = vec![gpu("low", 2.0), gpu("high", 5.0)];
        b[1].pass = false;
        b[1].failures = vec!["x".into()];
        let m = mean_results(&[a, b]);
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].gpu_ms, Some(1.5));
        assert_eq!(m[1].gpu_ms, Some(4.0));
        assert_eq!(m[1].gpu_passes[0].ms, 2.0);
        assert!(m[0].pass && !m[1].pass);
        assert_eq!(format_passes(&m[1].gpu_passes), "forward 2.00");
    }
}
