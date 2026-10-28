//! The automatic process checks of `cargo xtask review` (docs/KIT_PROCESS.md,
//! "Done when" items 11 to 15). Each check is one small pure function from
//! measurements to a [`Verdict`]; `review.rs` gathers the measurements and
//! keeps the registry that maps each check to its checklist item. A new check
//! is one function here, one `AutoCheck` variant and one gather step there.
//!
//! The measurements come from `tools/live-check/capture.mjs` (one headless
//! Chrome session driving the live bridge) and from git and the bench's saved
//! results. Nothing in this file touches the network, the browser or git.

use serde_json::Value;

/// One check's outcome and the evidence the card prints for it.
#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    /// Counts as passed (PASS, or FLAG: passed but shown to the critic and Chris).
    pub pass: bool,
    pub evidence: String,
    pub kind: Kind,
}

/// How a verdict was reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Measured: PASS or FAIL.
    Measured,
    /// The check could not run: the kit has not provided what it reads (a
    /// dump, a test, a report). Not a pass: it blocks the hand-over like a FAIL.
    NotRun,
    /// Passed, with something big to look at (a visual-regression difference).
    Flag,
    /// The check does not apply to this kit (a 2D-only check on a 3D kit).
    NotApplicable,
}

impl Verdict {
    pub fn pass(evidence: impl Into<String>) -> Self {
        Self { pass: true, evidence: evidence.into(), kind: Kind::Measured }
    }

    pub fn fail(evidence: impl Into<String>) -> Self {
        Self { pass: false, evidence: evidence.into(), kind: Kind::Measured }
    }

    pub fn not_run(evidence: impl Into<String>) -> Self {
        Self { pass: false, evidence: evidence.into(), kind: Kind::NotRun }
    }

    pub fn flag(evidence: impl Into<String>) -> Self {
        Self { pass: true, evidence: evidence.into(), kind: Kind::Flag }
    }

    /// Not run, and that is expected for now, so it does not block (the
    /// visual regression on a kit's first review: nothing accepted yet).
    pub fn waiting(evidence: impl Into<String>) -> Self {
        Self { pass: true, evidence: evidence.into(), kind: Kind::NotRun }
    }

    pub fn not_applicable(evidence: impl Into<String>) -> Self {
        Self { pass: true, evidence: evidence.into(), kind: Kind::NotApplicable }
    }

    /// The card's word for it: PASS, FAIL, FLAG, not run or n/a.
    pub fn label(&self) -> &'static str {
        match (self.kind, self.pass) {
            (Kind::NotRun, _) => "not run",
            (Kind::NotApplicable, _) => "n/a",
            (Kind::Flag, _) => "FLAG",
            (Kind::Measured, true) => "PASS",
            (Kind::Measured, false) => "FAIL",
        }
    }
}

fn join_list(items: &[String]) -> String {
    items.join("; ")
}

/// `items` for a card line: the first `n`, then how many more there are.
fn first_n(items: &[String], n: usize) -> Vec<String> {
    if items.len() <= n {
        return items.to_vec();
    }
    let mut v = items[..n].to_vec();
    v.push(format!("and {} more (see the table below)", items.len() - n));
    v
}


// ---- 1. tier cost order -------------------------------------------------------------

/// The tiers whose GPU cost must rise, cheapest first (the bench's list).
pub const COST_ORDER: [&str; 5] = evani_bench::check::COST_ORDER;

fn cap(id: &str) -> String {
    let mut c = id.chars();
    c.next().map_or(String::new(), |f| f.to_uppercase().collect::<String>() + c.as_str())
}

/// GPU time must not fall Lite -> Low -> Medium -> High -> Ultra, judged exactly as
/// `cargo xtask bench` judges it ([`evani_bench::order_failures`]): median
/// GPU ms per frame, and a tier may measure up to
/// [`evani_bench::check::ORDER_TOLERANCE_PCT`] % (and
/// [`evani_bench::check::ORDER_TOLERANCE_MS`] ms) under the one below before
/// it counts as cheaper, because GPU timestamps swing with the card's clock
/// state. `gpu` is `(tier id, gpu ms)`; `source` says where the numbers
/// come from (the stress scene's median of runs, or the bench path). A tier
/// with no GPU timing can't be checked and fails the check.
pub fn tier_cost_order(gpu: &[(String, Option<f32>)], source: &str) -> Verdict {
    let mut seen: Vec<(&str, f32)> = Vec::new();
    let mut missing: Vec<String> = Vec::new();
    for id in COST_ORDER {
        match gpu.iter().find(|(t, _)| t == id).and_then(|(_, g)| *g) {
            Some(g) => seen.push((id, g)),
            None => missing.push(cap(id)),
        }
    }
    if !missing.is_empty() {
        return Verdict::fail(format!(
            "No GPU time for {} (no bench result or no timestamp queries), so the cost order can't be shown.",
            missing.join(", ")
        ));
    }
    let line = seen.iter().map(|(t, g)| format!("{} {g:.2}", cap(t))).collect::<Vec<_>>().join(" -> ");
    let bad = evani_bench::order_failures(&seen);
    let (pct, ms) = (evani_bench::check::ORDER_TOLERANCE_PCT, evani_bench::check::ORDER_TOLERANCE_MS);
    if bad.is_empty() {
        Verdict::pass(format!(
            "GPU ms {line} ({source}): no tier is cheaper than the one below (tolerance {pct:.0}% / {ms} ms)."
        ))
    } else {
        Verdict::fail(format!("GPU ms {line} ({source}): {}.", join_list(&bad)))
    }
}

// ---- 2. architecture conformance -----------------------------------------------------

/// Live ECS entity count and registered kit params from the review session.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ArchProbe {
    /// `(where it was read, entity count)`; `None` when the session didn't say.
    pub entities: Vec<(String, Option<u64>)>,
    /// Names of the params the kit registered (owner `kit.<id>`).
    pub kit_params: Vec<String>,
    pub total_params: usize,
}

/// A kit must keep its gameplay in the ECS (entities > 0 in every view of the
/// running game) and register its tunables (at least one `kit.<id>.*` param).
pub fn architecture(kit: &str, p: &ArchProbe) -> Verdict {
    let mut bad: Vec<String> = Vec::new();
    if p.entities.is_empty() {
        bad.push("the live session reported no entity count".into());
    }
    let zero: Vec<&str> = p.entities.iter().filter(|(_, n)| *n == Some(0)).map(|(w, _)| w.as_str()).collect();
    if !zero.is_empty() {
        bad.push(format!("0 entities while running ({})", zero.join(", ")));
    }
    let unknown: Vec<&str> = p.entities.iter().filter(|(_, n)| n.is_none()).map(|(w, _)| w.as_str()).collect();
    if !unknown.is_empty() {
        bad.push(format!("no entity count at {}", unknown.join(", ")));
    }
    if p.kit_params.is_empty() {
        bad.push(format!("no param named kit.{kit}.* is registered ({} params in the whole game)", p.total_params));
    }
    if !bad.is_empty() {
        return Verdict::fail(format!("{}.", join_list(&bad)));
    }
    let counts: Vec<u64> = p.entities.iter().filter_map(|(_, n)| *n).collect();
    let (lo, hi) = (counts.iter().min().copied().unwrap_or(0), counts.iter().max().copied().unwrap_or(0));
    Verdict::pass(format!(
        "{lo} to {hi} entities in {} views of the running game; {} kit.{kit}.* params registered ({} in the whole game).",
        counts.len(),
        p.kit_params.len(),
        p.total_params
    ))
}

// ---- 3. captures: overlay hidden, HUD visible ------------------------------------------

/// One declared HUD selector as the page saw it.
#[derive(Debug, Clone, PartialEq)]
pub struct HudProbe {
    pub selector: String,
    pub matched: u32,
    pub visible: bool,
    pub text_len: u32,
}

/// What the page showed in one review capture.
#[derive(Debug, Clone, PartialEq)]
pub struct CaptureProbe {
    /// The capture's name (a moment, `t=20`, ...).
    pub name: String,
    /// The debug overlay or debug panel was visible.
    pub overlay_visible: bool,
    pub hud: Vec<HudProbe>,
}

impl CaptureProbe {
    /// Some declared HUD element is shown and has something in it.
    pub fn hud_visible(&self) -> bool {
        self.hud.iter().any(|h| h.visible)
    }
}

/// Every capture hides the debug overlay and shows the game's HUD. `declared`
/// is `[review] hud` from evani.toml: a game with no HUD declared fails, so
/// "no HUD" is a visible finding rather than a pass by default.
pub fn capture_hud(declared: &[String], caps: &[CaptureProbe]) -> Verdict {
    if caps.is_empty() {
        return Verdict::fail("No page captures were taken, so neither the overlay nor the HUD could be checked.");
    }
    let mut bad: Vec<String> = Vec::new();
    let shown: Vec<&str> = caps.iter().filter(|c| c.overlay_visible).map(|c| c.name.as_str()).collect();
    if !shown.is_empty() {
        bad.push(format!("the debug overlay is visible in {}", shown.join(", ")));
    }
    if declared.is_empty() {
        bad.push("evani.toml lists no HUD (`[review] hud = [\"#selector\"]`), so none could be verified".into());
    } else {
        let hidden: Vec<&str> = caps.iter().filter(|c| !c.hud_visible()).map(|c| c.name.as_str()).collect();
        if !hidden.is_empty() {
            bad.push(format!("no HUD element ({}) is visible and filled in {}", declared.join(", "), hidden.join(", ")));
        }
    }
    if bad.is_empty() {
        Verdict::pass(format!(
            "Overlay hidden and HUD ({}) visible and filled in all {} captures.",
            declared.join(", "),
            caps.len()
        ))
    } else {
        Verdict::fail(format!("{}.", join_list(&bad)))
    }
}

// ---- 4. world edge (geocheck) ------------------------------------------------------------

/// A camera as `capture.frame` reports it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub eye: [f32; 3],
    pub target: [f32; 3],
    pub up: [f32; 3],
    pub fov_y_deg: f32,
}

impl Pose {
    pub fn from_json(v: &Value) -> Option<Self> {
        let vec3 = |k: &str| -> Option<[f32; 3]> {
            let a = v.get(k)?.as_array()?;
            Some([a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32, a.get(2)?.as_f64()? as f32])
        };
        Some(Self {
            eye: vec3("eye")?,
            target: vec3("target")?,
            up: vec3("up")?,
            fov_y_deg: v.get("fov_y_deg")?.as_f64()? as f32,
        })
    }
}

/// An 8-bit RGB image, rows top to bottom.
#[derive(Debug, Clone, PartialEq)]
pub struct Rgb {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

/// Decode a PNG (8-bit grey, grey+alpha, RGB or RGBA; not interlaced) to RGB.
pub fn decode_png(bytes: &[u8]) -> Result<Rgb, String> {
    use std::io::Read as _;
    const SIG: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];
    if bytes.len() < 8 || bytes[..8] != SIG {
        return Err("not a PNG".into());
    }
    let (mut w, mut h, mut depth, mut color, mut interlace) = (0usize, 0usize, 0u8, 0u8, 0u8);
    let mut idat: Vec<u8> = Vec::new();
    let mut at = 8;
    while at + 8 <= bytes.len() {
        let len = u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) as usize;
        let kind = &bytes[at + 4..at + 8];
        let body = bytes.get(at + 8..at + 8 + len).ok_or("truncated PNG chunk")?;
        match kind {
            b"IHDR" if body.len() >= 13 => {
                w = u32::from_be_bytes([body[0], body[1], body[2], body[3]]) as usize;
                h = u32::from_be_bytes([body[4], body[5], body[6], body[7]]) as usize;
                (depth, color, interlace) = (body[8], body[9], body[12]);
            }
            b"IDAT" => idat.extend_from_slice(body),
            b"IEND" => break,
            _ => {}
        }
        at += 12 + len;
    }
    if w == 0 || h == 0 || w.saturating_mul(h) > 64_000_000 {
        return Err("PNG has no usable size".into());
    }
    if depth != 8 || interlace != 0 {
        return Err(format!("PNG depth {depth} / interlace {interlace} is not supported (8-bit, not interlaced)"));
    }
    let chans = match color {
        0 => 1,
        2 => 3,
        4 => 2,
        6 => 4,
        c => return Err(format!("PNG colour type {c} is not supported")),
    };
    let mut raw = Vec::new();
    flate2::read::ZlibDecoder::new(&idat[..]).read_to_end(&mut raw).map_err(|e| format!("PNG data: {e}"))?;
    let stride = w * chans;
    if raw.len() < (stride + 1) * h {
        return Err("PNG data is short".into());
    }
    let mut prev = vec![0u8; stride];
    let mut cur = vec![0u8; stride];
    let mut px = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        let line = &raw[y * (stride + 1)..(y + 1) * (stride + 1)];
        let filter = line[0];
        for i in 0..stride {
            let a = if i >= chans { cur[i - chans] } else { 0 };
            let b = prev[i];
            let c = if i >= chans { prev[i - chans] } else { 0 };
            let x = line[1 + i];
            cur[i] = match filter {
                0 => x,
                1 => x.wrapping_add(a),
                2 => x.wrapping_add(b),
                3 => x.wrapping_add(((u16::from(a) + u16::from(b)) / 2) as u8),
                4 => x.wrapping_add(paeth(a, b, c)),
                f => return Err(format!("PNG filter {f}")),
            };
        }
        for p in cur.chunks_exact(chans) {
            match chans {
                1 | 2 => px.extend_from_slice(&[p[0]; 3]),
                _ => px.extend_from_slice(&p[..3]),
            }
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    Ok(Rgb { w, h, px })
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (ia, ib, ic) = (i16::from(a), i16::from(b), i16::from(c));
    let p = ia + ib - ic;
    let (pa, pb, pc) = ((p - ia).abs(), (p - ib).abs(), (p - ic).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// How the world-edge check reads a pair of renders.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeParams {
    /// A pixel is background when it changed by at least this much in every
    /// channel between the black and the white clear colour (0..255).
    pub bg_min_diff: u8,
    /// ... and by at least the common background change minus this.
    pub bg_slack: u8,
    /// The world has to reach at least this far (metres) before background
    /// may show below the horizon (fog may swallow the ground only beyond it).
    pub min_reach_m: f32,
    /// ... and background within this many degrees under the horizon is
    /// always allowed (a fogged far plane, the pixel rounding of the horizon).
    pub min_tolerance_deg: f32,
    /// A view fails when more than this share of its pixels is edge.
    pub max_edge_share: f32,
    /// A hard edge: background under the horizon (by more than
    /// `min_tolerance_deg`) with solid world within `hard_rows` pixels below
    /// it. Solid means it changed by less than this share of the background
    /// change (fog ramps never jump that much in a few pixels; the end of a
    /// ground plane does).
    pub hard_solid_share: f32,
    pub hard_rows: usize,
    /// A view fails when hard-edge pixels show in more than this share of the
    /// picture's columns, at any camera height.
    pub max_hard_width: f32,
}

impl Default for EdgeParams {
    fn default() -> Self {
        Self {
            bg_min_diff: 120,
            bg_slack: 24,
            min_reach_m: 600.0,
            min_tolerance_deg: 1.5,
            max_edge_share: 0.002,
            hard_solid_share: 0.6,
            hard_rows: 3,
            max_hard_width: 0.02,
        }
    }
}

/// What one pair of renders showed.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EdgeView {
    /// Pixels that show the clear colour.
    pub bg_px: usize,
    /// ... of those, the ones under the horizon by more than the tolerance.
    pub edge_px: usize,
    pub total_px: usize,
    /// The deepest edge pixel below the horizon, degrees.
    pub max_below_deg: f32,
    pub tolerance_deg: f32,
    /// Columns with a hard edge (background under the horizon right above
    /// solid world: the end of the ground seen from any height).
    pub hard_cols: usize,
    pub width: usize,
}

impl EdgeView {
    pub fn share(&self) -> f32 {
        self.edge_px as f32 / self.total_px.max(1) as f32
    }

    /// Share of the picture's width a hard edge runs across.
    pub fn hard_share(&self) -> f32 {
        self.hard_cols as f32 / self.width.max(1) as f32
    }

    #[cfg(test)]
    pub fn fails(&self, p: &EdgeParams) -> bool {
        self.share() > p.max_edge_share || self.hard_share() > p.max_hard_width
    }
}

fn norm(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-9);
    [v[0] / l, v[1] / l, v[2] / l]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// Degrees below the horizontal for every pixel of a `w` x `h` view from `pose`
/// (negative above the horizon). World up is +Y; the ground is the plane y = 0.
pub fn depression_deg(pose: &Pose, w: usize, h: usize, x: usize, y: usize) -> f32 {
    let f = norm([pose.target[0] - pose.eye[0], pose.target[1] - pose.eye[1], pose.target[2] - pose.eye[2]]);
    let r = norm(cross(f, pose.up));
    let u = cross(r, f);
    let tan_y = (pose.fov_y_deg.to_radians() * 0.5).tan();
    let tan_x = tan_y * w as f32 / h.max(1) as f32;
    let sx = 2.0 * (x as f32 + 0.5) / w as f32 - 1.0;
    let sy = 1.0 - 2.0 * (y as f32 + 0.5) / h as f32;
    let ray = norm([
        f[0] + sx * tan_x * r[0] + sy * tan_y * u[0],
        f[1] + sx * tan_x * r[1] + sy * tan_y * u[1],
        f[2] + sx * tan_x * r[2] + sy * tan_y * u[2],
    ]);
    -ray[1].clamp(-1.0, 1.0).asin().to_degrees()
}

/// Compare the same view drawn with a black and with a white clear colour
/// (`render.clear_color`, which is also the fog colour). Pixels that follow
/// the clear colour are background; background that shows under the horizon by
/// more than the tolerance is the edge of the world (the ground ends short of
/// where the world should reach). Separately, background under the horizon
/// with solid world right below it is a hard edge at any camera height: a
/// high title or orbit camera gets a large reach tolerance (fog may swallow
/// the far ground), but a crisp end of the ground plane is never fog.
pub fn edge_view(black: &Rgb, white: &Rgb, pose: &Pose, p: &EdgeParams) -> Result<EdgeView, String> {
    if black.w != white.w || black.h != white.h || black.px.len() != white.px.len() {
        return Err(format!("the two renders differ in size ({}x{} and {}x{})", black.w, black.h, white.w, white.h));
    }
    let (w, h) = (black.w, black.h);
    let total = w * h;
    // Per pixel: the smallest change over the three channels.
    let diff: Vec<u8> = (0..total)
        .map(|i| (0..3).map(|c| black.px[i * 3 + c].abs_diff(white.px[i * 3 + c])).min().unwrap_or(0))
        .collect();
    // The common background change: the busiest bin of 8 levels above the floor.
    let mut bins = [0usize; 32];
    for &d in &diff {
        if d >= p.bg_min_diff {
            bins[usize::from(d) / 8] += 1;
        }
    }
    let best = bins.iter().enumerate().rev().max_by_key(|(_, n)| **n).map(|(b, n)| (b, *n));
    // The reach tolerance for this camera height, in degrees below the horizon.
    let tolerance_deg = p.min_tolerance_deg.max((pose.eye[1].max(0.0) / p.min_reach_m).atan().to_degrees());
    let Some((bin, _)) = best.filter(|(_, n)| *n > 0) else {
        return Ok(EdgeView { total_px: total, tolerance_deg, width: w, ..EdgeView::default() });
    };
    let level = (bin * 8 + 4) as i32;
    let floor = (level - i32::from(p.bg_slack)).max(i32::from(p.bg_min_diff));
    // Solid: changed by less than this (fog ramps never reach it within a few rows).
    let solid = (level as f32 * p.hard_solid_share) as i32;
    let (mut bg, mut edge, mut deepest) = (0usize, 0usize, 0.0f32);
    let mut hard_col = vec![false; w];
    for y in 0..h {
        for x in 0..w {
            if i32::from(diff[y * w + x]) < floor {
                continue;
            }
            bg += 1;
            let below = depression_deg(pose, w, h, x, y);
            if below > tolerance_deg {
                edge += 1;
                deepest = deepest.max(below);
            }
            // Background under the horizon right above solid world: the ground
            // (or whatever stands on it) ends here, whatever the camera height.
            if below > p.min_tolerance_deg
                && !hard_col[x]
                && (1..=p.hard_rows).any(|d| y + d < h && i32::from(diff[(y + d) * w + x]) < solid)
            {
                hard_col[x] = true;
                deepest = deepest.max(below);
            }
        }
    }
    let hard_cols = hard_col.iter().filter(|c| **c).count();
    Ok(EdgeView { bg_px: bg, edge_px: edge, total_px: total, max_below_deg: deepest, tolerance_deg, hard_cols, width: w })
}

/// Every scripted camera renders without a visible world edge. `views` is
/// `(camera name, result)`; a view that couldn't be analysed fails the check.
pub fn world_edge(views: &[(String, Result<EdgeView, String>)], p: &EdgeParams) -> Verdict {
    if views.is_empty() {
        return Verdict::fail("No camera was rendered for the world-edge check.");
    }
    let mut bad: Vec<String> = Vec::new();
    let mut clear = 0usize;
    for (name, v) in views {
        match v {
            Err(e) => bad.push(format!("{name}: could not be checked ({e})")),
            Ok(v) if v.hard_share() > p.max_hard_width => bad.push(format!(
                "{name}: edge of the world visible, the ground ends against the background across {:.0}% of the width, up to {:.0} degrees under the horizon",
                v.hard_share() * 100.0,
                v.max_below_deg
            )),
            Ok(v) if v.share() > p.max_edge_share => bad.push(format!(
                "{name}: edge of the world visible, {:.1}% of the picture is background up to {:.0} degrees under the horizon",
                v.share() * 100.0,
                v.max_below_deg
            )),
            Ok(_) => clear += 1,
        }
    }
    if bad.is_empty() {
        Verdict::pass(format!(
            "No visible world edge from any of the {} scripted cameras ({}).",
            views.len(),
            views.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(", ")
        ))
    } else {
        Verdict::fail(format!("{}. {clear} of {} cameras are clear.", join_list(&first_n(&bad, 3)), views.len()))
    }
}

// ---- 5. hand-over: size and engine commit --------------------------------------------------

/// What the hand-over states.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HandOver {
    /// The measured published size: shipped build, gzipped, no live module.
    pub size_gz: u64,
    /// The newest commit that touched the engine (`crates/`, `assets/tiers/`).
    pub engine: Option<String>,
    /// The same for the main line.
    pub main_ref: String,
    pub main: Option<String>,
    /// The engine files have uncommitted changes.
    pub dirty: bool,
}

/// The hand-over states the measured published size and the engine commit,
/// and the engine commit is the main line's.
pub fn handover(h: &HandOver, size_text: &str) -> Verdict {
    let mut bad: Vec<String> = Vec::new();
    if h.size_gz == 0 {
        bad.push("no published size was measured".into());
    }
    match (&h.engine, &h.main) {
        (Some(e), Some(m)) if e == m => {}
        (Some(e), Some(m)) => bad.push(format!("the engine is at {e} but {} is at {m}: not the current engine", h.main_ref)),
        (None, _) => bad.push("the engine commit could not be read".into()),
        (_, None) => bad.push(format!("the engine commit of {} could not be read", h.main_ref)),
    }
    if h.dirty {
        bad.push("the engine has uncommitted changes".into());
    }
    let stated = format!(
        "Published size {size_text}; engine commit {} (main line {}: {})",
        h.engine.as_deref().unwrap_or("unknown"),
        h.main_ref,
        h.main.as_deref().unwrap_or("unknown")
    );
    if bad.is_empty() {
        Verdict::pass(format!("{stated}; they match."))
    } else {
        Verdict::fail(format!("{stated}. {}.", cap(&join_list(&bad))))
    }
}

// ---- the probe the capture script returns ------------------------------------------------------

/// Pull the probe parts out of capture.mjs's summary: per-capture page probes,
/// entity counts, kit params, and the clear-colour pairs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Probe {
    pub arch: ArchProbe,
    pub captures: Vec<CaptureProbe>,
    /// Page screenshots next to the captures: `(capture name, file)`.
    pub page_shots: Vec<(String, String)>,
    pub geo: Vec<GeoFiles>,
}

/// One camera's black and white renders and its pose.
#[derive(Debug, Clone, PartialEq)]
pub struct GeoFiles {
    pub name: String,
    pub black: String,
    pub white: String,
    pub pose: Option<Pose>,
}

pub fn probe_from(v: &Value, kit: &str) -> Probe {
    let p = v.get("probe").unwrap_or(&Value::Null);
    let s = |x: &Value, k: &str| x.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let entities = p
        .get("entities")
        .and_then(Value::as_array)
        .map(|a| a.iter().map(|e| (s(e, "at"), e.get("count").and_then(Value::as_u64))).collect())
        .unwrap_or_default();
    let params = p.get("params").and_then(Value::as_array).cloned().unwrap_or_default();
    // A kit's params are `kit.<id>.*`, and a kit whose id has a hyphen writes
    // it with an underscore (`kit.kart_racer.accel`, owner `kit.kart-racer`).
    let ids = [kit.to_string(), kit.replace('-', "_")];
    let kit_params = params
        .iter()
        .filter(|x| ids.iter().any(|id| s(x, "name").starts_with(&format!("kit.{id}.")) || s(x, "owner") == format!("kit.{id}")))
        .map(|x| s(x, "name"))
        .collect();
    let captures = p
        .get("captures")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|c| CaptureProbe {
                    name: s(c, "name"),
                    overlay_visible: c.pointer("/overlay/visible").and_then(Value::as_bool).unwrap_or(true),
                    hud: c
                        .get("hud")
                        .and_then(Value::as_array)
                        .map(|h| {
                            h.iter()
                                .map(|x| HudProbe {
                                    selector: s(x, "selector"),
                                    matched: x.get("matched").and_then(Value::as_u64).unwrap_or(0) as u32,
                                    visible: x.get("visible").and_then(Value::as_bool).unwrap_or(false),
                                    text_len: x.get("text_len").and_then(Value::as_u64).unwrap_or(0) as u32,
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default();
    let page_shots = p
        .get("captures")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter(|c| c.get("page_png").is_some_and(|f| !f.is_null())).map(|c| (s(c, "name"), s(c, "page_png"))).collect())
        .unwrap_or_default();
    let geo = p
        .get("geo")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|g| GeoFiles {
                    name: s(g, "name"),
                    black: s(g, "black"),
                    white: s(g, "white"),
                    pose: g.get("camera").and_then(Pose::from_json),
                })
                .collect()
        })
        .unwrap_or_default();
    Probe { arch: ArchProbe { entities, kit_params, total_params: params.len() }, captures, page_shots, geo }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write as _;

    fn gpu(v: [Option<f32>; 5]) -> Vec<(String, Option<f32>)> {
        COST_ORDER.iter().zip(v).map(|(t, g)| ((*t).to_string(), g)).collect()
    }

    const SRC: &str = "stress scene, median of 5 runs";

    #[test]
    fn tier_cost_rises_passes() {
        let v = tier_cost_order(&gpu([Some(0.2), Some(0.4), Some(0.9), Some(1.8), Some(3.0)]), SRC);
        assert!(v.pass, "{}", v.evidence);
        assert!(v.evidence.contains("Lite 0.20 -> Low 0.40 -> Medium 0.90 -> High 1.80 -> Ultra 3.00 (stress scene, median of 5 runs)"), "{}", v.evidence);
    }

    #[test]
    fn tier_cost_uses_the_bench_rule() {
        // A real inversion fails, worded as the bench words it.
        let inv = tier_cost_order(&gpu([Some(0.2), Some(0.4), Some(0.9), Some(0.7), Some(3.0)]), SRC);
        assert!(!inv.pass);
        assert!(inv.evidence.contains("high 0.70 ms is under medium 0.90 ms"), "{}", inv.evidence);
        // The bench's tolerance: a flat step (1.0 -> 1.03), exactly equal, or up
        // to 10 % cheaper is clock noise, not an inversion; 0.05 ms neither.
        for v in [
            [Some(0.2), Some(1.0), Some(1.03), Some(2.0), Some(3.0)],
            [Some(0.2), Some(1.0), Some(1.0), Some(2.0), Some(3.0)],
            [Some(0.2), Some(1.0), Some(0.91), Some(2.0), Some(3.0)],
            [Some(0.2), Some(0.30), Some(0.26), Some(2.0), Some(3.0)],
        ] {
            let r = tier_cost_order(&gpu(v), SRC);
            assert!(r.pass, "{v:?}: {}", r.evidence);
        }
        assert!(!tier_cost_order(&gpu([Some(0.2), Some(1.0), Some(0.85), Some(2.0), Some(3.0)]), SRC).pass);
        // Same verdict as the bench's own order check on the same numbers.
        let seen = [("low", 1.0), ("medium", 0.85), ("high", 2.0), ("ultra", 3.0)];
        assert_eq!(evani_bench::order_failures(&seen).len(), 1);
    }

    #[test]
    fn tier_cost_missing_timing_fails() {
        let v = tier_cost_order(&gpu([Some(0.2), Some(0.4), None, Some(1.8), Some(3.0)]), SRC);
        assert!(!v.pass);
        assert!(v.evidence.contains("No GPU time for Medium"), "{}", v.evidence);
        assert!(!tier_cost_order(&[], SRC).pass);
        // Tiers outside the five (vr) are ignored; order of input doesn't matter.
        let mut g = gpu([Some(0.2), Some(0.4), Some(0.9), Some(1.8), Some(3.0)]);
        g.reverse();
        g.push(("vr".into(), Some(0.1)));
        assert!(tier_cost_order(&g, SRC).pass);
    }

    fn arch(entities: &[(&str, Option<u64>)], params: &[&str]) -> ArchProbe {
        ArchProbe {
            entities: entities.iter().map(|(w, n)| ((*w).to_string(), *n)).collect(),
            kit_params: params.iter().map(|s| (*s).to_string()).collect(),
            total_params: 40,
        }
    }

    #[test]
    fn architecture_needs_entities_and_registered_params() {
        let ok = architecture("kart-racer", &arch(&[("boot", Some(120)), ("drift", Some(900))], &["kit.kart-racer.max_speed"]));
        assert!(ok.pass, "{}", ok.evidence);
        assert!(ok.evidence.contains("120 to 900 entities in 2 views"), "{}", ok.evidence);
        let zero = architecture("k", &arch(&[("boot", Some(5)), ("path t=0", Some(0))], &["kit.k.a"]));
        assert!(!zero.pass);
        assert!(zero.evidence.contains("0 entities while running (path t=0)"), "{}", zero.evidence);
        let none = architecture("k", &arch(&[("boot", Some(5))], &[]));
        assert!(!none.pass);
        assert!(none.evidence.contains("kit.k.*"), "{}", none.evidence);
        assert!(!architecture("k", &arch(&[], &["kit.k.a"])).pass);
        assert!(!architecture("k", &arch(&[("boot", None)], &["kit.k.a"])).pass);
    }

    fn cap_probe(name: &str, overlay: bool, hud: bool) -> CaptureProbe {
        CaptureProbe {
            name: name.into(),
            overlay_visible: overlay,
            hud: vec![HudProbe { selector: "#hud0".into(), matched: 1, visible: hud, text_len: if hud { 12 } else { 0 } }],
        }
    }

    #[test]
    fn captures_need_hidden_overlay_and_visible_hud() {
        let sel = vec!["#hud0".to_string()];
        let ok = capture_hud(&sel, &[cap_probe("drift", false, true), cap_probe("finish", false, true)]);
        assert!(ok.pass, "{}", ok.evidence);
        let shown = capture_hud(&sel, &[cap_probe("drift", true, true)]);
        assert!(!shown.pass);
        assert!(shown.evidence.contains("overlay is visible in drift"), "{}", shown.evidence);
        let nohud = capture_hud(&sel, &[cap_probe("drift", false, true), cap_probe("finish", false, false)]);
        assert!(!nohud.pass);
        assert!(nohud.evidence.contains("filled in finish"), "{}", nohud.evidence);
        assert!(!capture_hud(&sel, &[]).pass);
    }

    #[test]
    fn a_game_with_no_declared_hud_fails_visibly() {
        let v = capture_hud(&[], &[cap_probe("t=0", false, false)]);
        assert!(!v.pass);
        assert!(v.evidence.contains("lists no HUD"), "{}", v.evidence);
    }

    fn pose(eye_y: f32, pitch_down: f32) -> Pose {
        // Looks along -Z, tipped down by `pitch_down` degrees.
        let p = pitch_down.to_radians();
        Pose { eye: [0.0, eye_y, 0.0], target: [0.0, eye_y - p.sin(), -p.cos()], up: [0.0, 1.0, 0.0], fov_y_deg: 60.0 }
    }

    #[test]
    fn depression_is_zero_on_the_horizon_and_follows_pitch() {
        let level = pose(5.0, 0.0);
        // The middle row of a level camera sits on the horizon.
        assert!(depression_deg(&level, 100, 101, 50, 50).abs() < 0.05);
        // The top of the picture is half the field of view above it, the bottom below it.
        assert!((depression_deg(&level, 100, 100, 50, 0) + 29.7).abs() < 0.5);
        assert!((depression_deg(&level, 100, 100, 50, 99) - 29.7).abs() < 0.5);
        // Tipped down by 20 degrees the picture centre points 20 below the horizon.
        assert!((depression_deg(&pose(5.0, 20.0), 100, 101, 50, 50) - 20.0).abs() < 0.1);
    }

    /// Black and white renders of a scene: `ground_from` is the first row that
    /// shows ground; rows above show the clear colour (black -> white).
    fn pair(w: usize, h: usize, ground_from: usize) -> (Rgb, Rgb) {
        let mut b = vec![0u8; w * h * 3];
        let mut wh = vec![0u8; w * h * 3];
        for y in 0..h {
            for x in 0..w {
                for c in 0..3 {
                    let i = (y * w + x) * 3 + c;
                    if y < ground_from {
                        (b[i], wh[i]) = (0, 220);
                    } else {
                        (b[i], wh[i]) = (60, 60);
                    }
                }
            }
        }
        (Rgb { w, h, px: b }, Rgb { w, h, px: wh })
    }

    #[test]
    fn sky_above_the_horizon_is_not_an_edge() {
        // Level camera: horizon at row 50; sky above, ground below.
        let (b, w) = pair(100, 101, 50);
        let v = edge_view(&b, &w, &pose(5.0, 0.0), &EdgeParams::default()).unwrap();
        assert!(v.bg_px > 4000);
        assert_eq!(v.edge_px, 0);
        assert!(world_edge(&[("path t=0".into(), Ok(v))], &EdgeParams::default()).pass);
    }

    #[test]
    fn background_under_the_horizon_is_the_world_edge() {
        // Ground only starts at row 80, so rows 50..80 show background below the horizon.
        let (b, w) = pair(100, 101, 80);
        let v = edge_view(&b, &w, &pose(5.0, 0.0), &EdgeParams::default()).unwrap();
        assert!(v.edge_px > 2000, "{v:?}");
        assert!(v.max_below_deg > 10.0);
        let verdict = world_edge(&[("a".into(), Ok(v.clone())), ("kart title".into(), Ok(v))], &EdgeParams::default());
        assert!(!verdict.pass);
        assert!(verdict.evidence.contains("kart title: edge of the world visible"), "{}", verdict.evidence);
        assert!(verdict.evidence.contains("0 of 2 cameras are clear"), "{}", verdict.evidence);
    }

    #[test]
    fn a_long_list_of_bad_cameras_is_cut_short_on_the_card() {
        let (b, w) = pair(100, 101, 80);
        let v = edge_view(&b, &w, &pose(5.0, 0.0), &EdgeParams::default()).unwrap();
        let views: Vec<_> = (0..6).map(|i| (format!("cam{i}"), Ok(v.clone()))).collect();
        let verdict = world_edge(&views, &EdgeParams::default());
        assert!(!verdict.pass);
        assert!(verdict.evidence.contains("cam2: edge") && !verdict.evidence.contains("cam3: edge"), "{}", verdict.evidence);
        assert!(verdict.evidence.contains("and 3 more") && verdict.evidence.contains("0 of 6 cameras are clear"), "{}", verdict.evidence);
    }

    #[test]
    fn looking_down_at_a_void_is_an_edge_and_a_thin_horizon_band_is_not() {
        // Everything is background and the camera looks 40 degrees down: all edge.
        let (b, w) = pair(64, 64, 64);
        let v = edge_view(&b, &w, &pose(5.0, 40.0), &EdgeParams::default()).unwrap();
        assert!(v.share() > 0.9, "{v:?}");
        // Only the 1 degree under the horizon is background: inside the tolerance.
        let level = pose(5.0, 0.0);
        let (b, w) = pair(100, 101, 53);
        let v = edge_view(&b, &w, &level, &EdgeParams::default()).unwrap();
        assert_eq!(v.edge_px, 0, "{v:?}");
    }

    #[test]
    fn a_picture_with_no_background_passes_and_a_size_mismatch_does_not() {
        let (b, _) = pair(20, 20, 0);
        let v = edge_view(&b, &b, &pose(5.0, 30.0), &EdgeParams::default()).unwrap();
        assert_eq!((v.bg_px, v.edge_px), (0, 0));
        let (b2, _) = pair(21, 20, 0);
        assert!(edge_view(&b, &b2, &pose(5.0, 0.0), &EdgeParams::default()).is_err());
        let bad = world_edge(&[("x".into(), Err("no camera".into()))], &EdgeParams::default());
        assert!(!bad.pass);
        assert!(bad.evidence.contains("x: could not be checked (no camera)"), "{}", bad.evidence);
        assert!(!world_edge(&[], &EdgeParams::default()).pass);
    }

    #[test]
    fn a_high_camera_forgives_more_degrees_under_the_horizon() {
        let p = EdgeParams::default();
        let low = edge_view(&pair(10, 10, 0).0, &pair(10, 10, 0).1, &pose(5.0, 0.0), &p).unwrap();
        let high = edge_view(&pair(10, 10, 0).0, &pair(10, 10, 0).1, &pose(120.0, 0.0), &p).unwrap();
        assert!((low.tolerance_deg - 1.5).abs() < 0.01);
        assert!((high.tolerance_deg - (120.0f32 / p.min_reach_m).atan().to_degrees()).abs() < 0.01);
    }

    #[test]
    fn a_crisp_end_of_the_ground_fails_even_from_a_high_camera() {
        // A 60 m high camera tipped 20 degrees down: the reach tolerance
        // forgives several degrees, but ground that stops dead against the
        // background (rows 0..40 background, solid ground from row 40) is a
        // hard edge across the whole width.
        let mut cam = pose(60.0, 20.0);
        cam.fov_y_deg = 55.0;
        let (b, w) = pair(160, 90, 40);
        let v = edge_view(&b, &w, &cam, &EdgeParams::default()).unwrap();
        assert!(v.hard_share() > 0.9, "{v:?}");
        assert!(v.fails(&EdgeParams::default()));
        let verdict = world_edge(&[("title".into(), Ok(v))], &EdgeParams::default());
        assert!(!verdict.pass && verdict.evidence.contains("title: edge of the world visible, the ground ends"), "{}", verdict.evidence);
    }

    #[test]
    fn a_fog_ramp_into_the_background_is_not_a_hard_edge() {
        // Background above row 30, then ground whose difference fades from the
        // background's 220 down to 0 over 30 rows (fog thinning towards the camera).
        let (w, h) = (100usize, 90usize);
        let (mut b, mut wh) = (vec![0u8; w * h * 3], vec![0u8; w * h * 3]);
        for y in 0..h {
            let d = if y < 30 { 220 } else { 220usize.saturating_sub((y - 30) * 220 / 30) as u8 as usize };
            for x in 0..w {
                for c in 0..3 {
                    let i = (y * w + x) * 3 + c;
                    (b[i], wh[i]) = (10, (10 + d).min(255) as u8);
                }
            }
        }
        let mut cam = pose(60.0, 20.0);
        cam.fov_y_deg = 55.0;
        let v = edge_view(&Rgb { w, h, px: b }, &Rgb { w, h, px: wh }, &cam, &EdgeParams::default()).unwrap();
        assert_eq!(v.hard_cols, 0, "{v:?}");
    }

    fn fixture(name: &str) -> Rgb {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures").join(name);
        decode_png(&std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))).unwrap()
    }

    /// Regression: the kart racer's title screen (the boot view, its orbit
    /// camera 72 m up) showed the end of the ground plane at the top left, and
    /// the first version of this check passed it because the height-scaled
    /// reach (250 m) forgave 16 degrees under the horizon. Renders and pose
    /// from that review run (process-checks f16e62b, kart at 03979fc).
    #[test]
    fn the_kart_title_screen_ground_edge_fails() {
        let p = EdgeParams::default();
        let title = Pose { eye: [54.198_57, 71.932_46, 167.64], target: [5.205_902, 0.0, 17.661_652], up: [0.0, 1.0, 0.0], fov_y_deg: 55.0 };
        let v = edge_view(&fixture("kart-title-black.png"), &fixture("kart-title-white.png"), &title, &p).unwrap();
        assert!(v.hard_share() > 0.2, "{v:?}");
        let verdict = world_edge(&[("boot".into(), Ok(v))], &p);
        assert!(!verdict.pass, "{}", verdict.evidence);
        // The same build's bench orbit shows the far edge of the plane too.
        let orbit = Pose { eye: [141.845_35, 61.364_78, 96.550_46], target: [5.205_902, 0.0, 17.661_652], up: [0.0, 1.0, 0.0], fov_y_deg: 55.0 };
        let v = edge_view(&fixture("kart-orbit-black.png"), &fixture("kart-orbit-white.png"), &orbit, &p).unwrap();
        assert!(v.fails(&p), "{v:?}");
        // A chase camera 6 m up in the middle of the track sees no edge.
        let drift = Pose {
            eye: [112.628_18, 5.927_24, 37.089_66],
            target: [101.984_955, 3.847_616, 38.848_32],
            up: [-0.002_323_484, 0.999_898_4, -0.014_061_487],
            fov_y_deg: 62.0,
        };
        let v = edge_view(&fixture("kart-drift-black.png"), &fixture("kart-drift-white.png"), &drift, &p).unwrap();
        assert!(!v.fails(&p), "{v:?}");
    }

    fn png(w: u32, h: u32, color: u8, filter: u8, pixels: &[u8]) -> Vec<u8> {
        let chans = match color {
            0 => 1,
            2 => 3,
            4 => 2,
            _ => 4,
        };
        let stride = w as usize * chans;
        let mut raw = Vec::new();
        for y in 0..h as usize {
            raw.push(filter);
            let row = &pixels[y * stride..(y + 1) * stride];
            let prev: Vec<u8> = if y == 0 { vec![0; stride] } else { pixels[(y - 1) * stride..y * stride].to_vec() };
            for i in 0..stride {
                let a = if i >= chans { row[i - chans] } else { 0 };
                let b = prev[i];
                let c = if i >= chans { prev[i - chans] } else { 0 };
                raw.push(match filter {
                    0 => row[i],
                    1 => row[i].wrapping_sub(a),
                    2 => row[i].wrapping_sub(b),
                    3 => row[i].wrapping_sub(((u16::from(a) + u16::from(b)) / 2) as u8),
                    _ => row[i].wrapping_sub(paeth(a, b, c)),
                });
            }
        }
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        z.write_all(&raw).unwrap();
        let idat = z.finish().unwrap();
        let mut out = vec![137, 80, 78, 71, 13, 10, 26, 10];
        let mut chunk = |kind: &[u8; 4], body: &[u8]| {
            out.extend_from_slice(&(body.len() as u32).to_be_bytes());
            out.extend_from_slice(kind);
            out.extend_from_slice(body);
            out.extend_from_slice(&[0, 0, 0, 0]); // the CRC isn't read
        };
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&w.to_be_bytes());
        ihdr.extend_from_slice(&h.to_be_bytes());
        ihdr.extend_from_slice(&[8, color, 0, 0, 0]);
        chunk(b"IHDR", &ihdr);
        chunk(b"IDAT", &idat);
        chunk(b"IEND", &[]);
        out
    }

    #[test]
    fn png_decoder_handles_every_filter_and_colour_type() {
        let rgb: Vec<u8> = (0..4 * 3 * 3).map(|i| (i * 7 % 256) as u8).collect();
        for f in 0..5 {
            let img = decode_png(&png(4, 3, 2, f, &rgb)).unwrap();
            assert_eq!((img.w, img.h), (4, 3));
            assert_eq!(img.px, rgb, "filter {f}");
        }
        let rgba: Vec<u8> = (0..2 * 2 * 4).map(|i| (i * 11 % 256) as u8).collect();
        let img = decode_png(&png(2, 2, 6, 4, &rgba)).unwrap();
        assert_eq!(&img.px[..6], &[rgba[0], rgba[1], rgba[2], rgba[4], rgba[5], rgba[6]]);
        let grey = decode_png(&png(2, 1, 0, 1, &[10, 200])).unwrap();
        assert_eq!(grey.px, [10, 10, 10, 200, 200, 200]);
        assert!(decode_png(b"nope").is_err());
        assert!(decode_png(&png(4, 3, 2, 0, &rgb)[..30]).is_err());
    }

    #[test]
    fn handover_needs_the_size_and_the_main_line_engine() {
        let ok = HandOver { size_gz: 305_000, engine: Some("d6b6b46".into()), main_ref: "m1-foundation".into(), main: Some("d6b6b46".into()), dirty: false };
        let v = handover(&ok, "298.8 KB gzipped");
        assert!(v.pass, "{}", v.evidence);
        assert!(v.evidence.contains("298.8 KB gzipped") && v.evidence.contains("d6b6b46"), "{}", v.evidence);
        let stale = HandOver { engine: Some("03979fc".into()), ..ok.clone() };
        let v = handover(&stale, "298.8 KB gzipped");
        assert!(!v.pass);
        assert!(v.evidence.contains("engine is at 03979fc but m1-foundation is at d6b6b46"), "{}", v.evidence);
        assert!(!handover(&HandOver { dirty: true, ..ok.clone() }, "x").pass);
        assert!(!handover(&HandOver { size_gz: 0, ..ok.clone() }, "x").pass);
        assert!(!handover(&HandOver { engine: None, ..ok.clone() }, "x").pass);
        assert!(!handover(&HandOver { main: None, ..ok }, "x").pass);
    }

    #[test]
    fn probe_is_read_from_the_capture_summary() {
        let v = json!({
            "probe": {
                "entities": [{ "at": "boot", "count": 12 }, { "at": "drift", "count": null }],
                "params": [{ "name": "kit.kart-racer.max_speed", "owner": "kit.kart-racer" }, { "name": "fx.bloom.intensity" }],
                "captures": [{
                    "name": "drift", "page_png": "page-drift.png",
                    "overlay": { "present": true, "visible": false },
                    "hud": [{ "selector": "#hud0", "matched": 1, "visible": true, "text_len": 9 }]
                }],
                "geo": [{ "name": "boot", "black": "b.png", "white": "w.png",
                          "camera": { "eye": [0, 5, 10], "target": [0, 0, 0], "up": [0, 1, 0], "fov_y_deg": 50 } },
                        { "name": "no camera", "black": "b2.png", "white": "w2.png" }]
            }
        });
        let p = probe_from(&v, "kart-racer");
        assert_eq!(p.arch.entities, [("boot".to_string(), Some(12)), ("drift".to_string(), None)]);
        assert_eq!(p.arch.kit_params, ["kit.kart-racer.max_speed"]);
        assert_eq!(p.arch.total_params, 2);
        assert!(!p.captures[0].overlay_visible && p.captures[0].hud_visible());
        assert_eq!(p.page_shots, [("drift".to_string(), "page-drift.png".to_string())]);
        assert_eq!(p.geo[0].pose.unwrap().fov_y_deg, 50.0);
        assert!(p.geo[1].pose.is_none());
        // A hyphenated kit id is written with an underscore in the param names.
        let real = json!({ "probe": { "params": [
            { "name": "kit.kart_racer.accel", "owner": "kit.kart-racer" },
            { "name": "kit.kart_racer.laps", "owner": "kit.kart-racer" },
            { "name": "kit.other.speed", "owner": "kit.other" },
            { "name": "render.exposure", "owner": "render" }] } });
        let q = probe_from(&real, "kart-racer");
        assert_eq!(q.arch.kit_params, ["kit.kart_racer.accel", "kit.kart_racer.laps"]);
        assert_eq!(q.arch.total_params, 4);
        // No probe at all reads as empty, which the checks fail rather than pass.
        assert_eq!(probe_from(&json!({}), "k"), Probe::default());
    }
}
