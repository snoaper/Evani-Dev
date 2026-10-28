//! The render phase: captures in the running game (through
//! `tools/live-check/capture.mjs --plan`), then the render-based checks and
//! the highlight crops.
//!
//! One plan, in this order:
//! 1. Sky calibration: the camera points straight up with the magenta sky
//!    on, then with a green sky; the colours the page really shows (after
//!    tone mapping) are what the gap search looks for.
//! 2. Gap and flicker views (orbit poses and low track stations): each shot
//!    four times with the magenta sky, jitter (0,0), (1,0), (0.5,0) and (0,0)
//!    again, then once more at (0,0) with the green sky. A gap pixel must
//!    follow the sky in the first and the green shot, so a magenta object is
//!    not a hole. Overlap and flicker are about static objects only: when the
//!    game has a still-world switch (`Setup::still_param`, a bool param) it
//!    is set first, so the game holds its sim and draws no movers (karts,
//!    items, their effects), and every shot of a view holds the clock at the
//!    same time ([`HOLD_S`]), so nothing animates between the jitter shots;
//!    the switch goes back off at the end. Whatever still changed between the
//!    two magenta (0,0) shots is masked out before the flicker test (without
//!    the switch, the game runs on between captures and the mask catches the
//!    moving karts). (The green shot is not used for that: the fog follows
//!    the sky colour, so distant geometry changes too.)
//! 3. Debug-view shots (the live debug view switched on once for them), kept in `views/` for the critic.
//! 4. Highlight shots: a camera framed on each chosen issue with the debug
//!    view on; the issue's box is outlined in its check colour and cropped
//!    into `crops/`.

use std::path::{Path, PathBuf};
use std::process::Command;

use evani_core::camera::CameraPose;
use evani_geocheck::geom::{Aabb, V3, add, mul};
use evani_geocheck::image::{Camera, FlickerParams, GapParams, Image, Outlines, flicker_where, gaps, ground_hit};
use evani_geocheck::report::{Check, Issue, Report, Severity, group};
use evani_geocheck::scene::{Kind, Scene};
use serde_json::{Value, json};

use super::png;

/// A named camera for the plan.
#[derive(Debug, Clone)]
pub struct Shot {
    pub name: String,
    pub pose: CameraPose,
}

pub struct Setup<'a> {
    pub root: &'a Path,
    pub app: &'a str,
    /// The report folder (`.vibedeck/geocheck/<app>`).
    pub out: &'a Path,
    pub port: u16,
    pub tier: Option<String>,
    /// Gap and flicker views.
    pub views: Vec<Shot>,
    /// Debug-view shots for the critic.
    pub debug_views: Vec<Shot>,
    /// Most highlight shots.
    pub highlights: usize,
    pub keep_raw: bool,
    /// The game's still-world switch (a bool param): on, it holds the sim and
    /// draws only static objects. Set for the whole render phase.
    pub still_param: Option<&'a str>,
}

/// The sim clock every gap, flicker and debug shot holds (s), so
/// time-driven animation is the same in all of them.
const HOLD_S: f64 = 30.0;

const JITTER: [(&str, [f32; 2]); 4] = [("a", [0.0, 0.0]), ("b", [1.0, 0.0]), ("c", [0.5, 0.0]), ("d", [0.0, 0.0])];
/// The second sky colour, used for the extra "g" shot of each view (no
/// jitter) that confirms gap pixels.
const SKY2: [f32; 3] = [0.0, 1.0, 0.0];
/// Wider than any canvas: the capture comes back at the render size.
const FULL: u32 = 4096;
/// Most render-check crops written per view and check.
const CROPS_PER_VIEW: usize = 3;

fn pose_json(p: &CameraPose) -> Value {
    json!({ "eye": p.eye, "target": p.target, "up": p.up, "fov_y_deg": p.fov_y_deg })
}

pub(crate) fn shot_step(name: &str, save: &str, pose: &CameraPose, extra: Value, note: &str) -> Value {
    let mut params = json!({ "width": FULL, "camera": pose_json(pose) });
    if let (Some(p), Some(e)) = (params.as_object_mut(), extra.as_object()) {
        for (k, v) in e {
            p.insert(k.clone(), v.clone());
        }
    }
    json!({ "name": name, "cmd": "capture.frame", "params": params, "save": save, "note": note, "timeout_ms": 60000 })
}

/// The plan step that turns the game's still-world switch on or off.
pub(crate) fn still_step(param: &str, on: bool) -> Value {
    let mut values = serde_json::Map::new();
    values.insert(param.to_string(), Value::Bool(on));
    let (name, note) = if on {
        ("still-on", "Geometry check: hold the world still, static objects only")
    } else {
        ("still-off", "Geometry check done: the world runs again")
    };
    json!({ "name": name, "cmd": "param.set", "params": { "values": values }, "note": note, "timeout_ms": 30000 })
}

/// The plan step that switches the live geometry debug view on (it answers
/// once the page has analysed the scene, which takes a while) or off. While
/// it is on the analysis is not repeated (`every_s` at its longest).
fn debug_step(on: bool) -> Value {
    let (name, note) = if on {
        ("debug-on", "Geometry check: switch the geometry debug view on for the debug and highlight shots")
    } else {
        ("debug-off", "Geometry check done: switch the geometry debug view off")
    };
    let params = if on { json!({ "on": true, "sky": true, "every_s": 30.0 }) } else { json!({ "on": false }) };
    json!({ "name": name, "cmd": "debug.geometry", "params": params, "note": note, "timeout_ms": 180000 })
}

fn rgb8(c: [f32; 3]) -> [u8; 3] {
    c.map(|x| (x.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn safe(s: &str) -> String {
    let t: String = s
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    let mut out = String::new();
    for c in t.chars() {
        if c == '-' && out.ends_with('-') {
            continue;
        }
        out.push(c);
    }
    out.trim_matches('-').chars().take(40).collect()
}

/// Issues to highlight: the worst of each untagged group first (errors
/// first, bigger groups first), then the rest of the untagged ones in
/// order, up to `n`.
pub fn pick_highlights(issues: &[Issue], n: usize) -> Vec<usize> {
    let idx = |p: &Issue| issues.iter().position(|q| std::ptr::eq(p, q));
    let mut heads: Vec<(Severity, usize, usize)> = group(issues.iter().filter(|i| i.tagged.is_none()))
        .into_iter()
        .filter_map(|g| {
            let worst = g
                .issues
                .iter()
                .copied()
                .max_by(|a, b| a.measure.partial_cmp(&b.measure).unwrap_or(std::cmp::Ordering::Equal))?;
            Some((worst.severity, g.issues.len(), idx(worst)?))
        })
        .collect();
    heads.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    let mut out: Vec<usize> = heads.into_iter().map(|h| h.2).collect();
    for (i, is) in issues.iter().enumerate() {
        if is.tagged.is_none() && !out.contains(&i) {
            out.push(i);
        }
    }
    out.truncate(n);
    out
}

fn issue_box(i: &Issue) -> Aabb {
    i.aabb.unwrap_or(Aabb { min: i.at, max: i.at }).grow(0.25)
}

fn highlight_pose(i: &Issue) -> CameraPose {
    let b = issue_box(i);
    let c = Camera::framing(&b, [1.0, 0.8, 1.0], 50.0, 16.0 / 9.0);
    let f = |v: V3| v.map(|x| x as f32);
    CameraPose {
        eye: f(c.eye),
        target: f(c.target),
        up: [0.0, 1.0, 0.0],
        fov_y_deg: 50.0,
    }
}

fn load(raw: &Path, file: &str) -> Option<Image> {
    let bytes = std::fs::read(raw.join(file)).ok()?;
    png::decode(&bytes).ok()
}

fn write_png(path: &Path, img: &Image) -> Result<(), String> {
    std::fs::write(path, png::encode(img)).map_err(|e| format!("write {}: {e}", path.display()))
}

/// The median colour of the central block (the calibration shot's sky).
fn centre_colour(img: &Image) -> [u8; 3] {
    let (cx, cy) = (img.w / 2, img.h / 2);
    let r = (img.w.min(img.h) / 8).max(1);
    let mut ch: [Vec<u8>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for y in cy.saturating_sub(r)..(cy + r).min(img.h) {
        for x in cx.saturating_sub(r)..(cx + r).min(img.w) {
            let p = img.px(x, y);
            for k in 0..3 {
                ch[k].push(p[k]);
            }
        }
    }
    ch.map(|mut v| {
        v.sort_unstable();
        v.get(v.len() / 2).copied().unwrap_or(0)
    })
}

/// Pixels that changed between the two unjittered shots (things that moved
/// between captures), grown by `grow` pixels.
fn motion_mask(a: &Image, d: &Image, threshold: u8, grow: u32) -> Vec<bool> {
    let (w, h) = (a.w, a.h);
    let mut m = vec![false; (w * h) as usize];
    if d.w != w || d.h != h {
        return vec![true; (w * h) as usize];
    }
    for y in 0..h {
        for x in 0..w {
            let p = a.px(x, y);
            let q = d.px(x, y);
            if (0..3).any(|k| p[k].abs_diff(q[k]) > threshold) {
                m[(y * w + x) as usize] = true;
            }
        }
    }
    // Grow: a horizontal then a vertical pass.
    let g = grow as i64;
    let mut hpass = m.clone();
    for y in 0..h {
        for x in 0..w {
            if m[(y * w + x) as usize] {
                for dx in -g..=g {
                    let xx = i64::from(x) + dx;
                    if xx >= 0 && xx < i64::from(w) {
                        hpass[(y * w + xx as u32) as usize] = true;
                    }
                }
            }
        }
    }
    let mut out = hpass.clone();
    for y in 0..h {
        for x in 0..w {
            if hpass[(y * w + x) as usize] {
                for dy in -g..=g {
                    let yy = i64::from(y) + dy;
                    if yy >= 0 && yy < i64::from(h) {
                        out[(yy as u32 * w + x) as usize] = true;
                    }
                }
            }
        }
    }
    out
}

fn blacken(img: &Image, mask: &[bool]) -> Image {
    let mut out = img.clone();
    if out.w != img.w || mask.len() != (img.w * img.h) as usize {
        return out;
    }
    for (i, &m) in mask.iter().enumerate() {
        if m {
            out.data[i * 4..i * 4 + 3].copy_from_slice(&[0, 0, 0]);
        }
    }
    out
}

/// What a pixel looks at: the first static hit (point and object label), or
/// the ground plane, or a point 50 m out.
fn look_at_pixel(stat: &Scene, cam: &Camera, w: u32, h: u32, p: [f64; 2]) -> (V3, Option<(String, usize)>) {
    let (o, d) = cam.pixel_ray(w, h, p[0], p[1]);
    if let Some((t, id)) = stat.ray(o, d, 5000.0, &|i| stat.items[i].kind == Kind::Decal) {
        let it = &stat.items[id];
        return (add(o, mul(d, t)), Some((stat.objects[it.object].label.clone(), it.index)));
    }
    let at = ground_hit(o, d, stat.ground_y()).unwrap_or_else(|| add(o, mul(d, 50.0)));
    (at, None)
}

/// Run the render phase: adds gap and flicker issues to `report`, sets
/// crops on highlighted issues, writes `views/` and `crops/` under the
/// report folder. Failures of single captures go to the notes.
pub fn run(s: &Setup, stat: &Scene, report: &mut Report) -> Result<(), String> {
    let raw = s.out.join("raw");
    let crops = s.out.join("crops");
    let views_dir = s.out.join("views");
    for d in [&raw, &crops, &views_dir] {
        if d.exists() {
            std::fs::remove_dir_all(d).map_err(|e| format!("clear {}: {e}", d.display()))?;
        }
        std::fs::create_dir_all(d).map_err(|e| format!("create {}: {e}", d.display()))?;
    }

    // ---- the plan
    let mut steps: Vec<Value> = Vec::new();
    let world = stat.bounds;
    let c = world.center();
    let up_pose = CameraPose {
        eye: [c[0] as f32, (world.max[1] + 20.0) as f32, c[2] as f32],
        target: [c[0] as f32, (world.max[1] + 120.0) as f32, c[2] as f32],
        up: [0.0, 0.0, 1.0],
        fov_y_deg: 40.0,
    };
    steps.push(shot_step("sky", "sky.png", &up_pose, json!({ "sky": true }), "Geometry check: measure the magenta sky colour"));
    steps.push(shot_step("sky2", "sky2.png", &up_pose, json!({ "sky": SKY2 }), "Geometry check: measure the second sky colour"));
    // Static objects only: the still-world switch on for every view.
    if let Some(p) = s.still_param {
        steps.push(still_step(p, true));
    }
    for v in &s.views {
        for (tag, j) in JITTER {
            steps.push(shot_step(
                &format!("{}-{tag}", v.name),
                &format!("{}-{tag}.png", v.name),
                &v.pose,
                json!({ "sky": true, "jitter_px": j, "sim_time": HOLD_S }),
                "Geometry check: gap and flicker view",
            ));
        }
        steps.push(shot_step(
            &format!("{}-g", v.name),
            &format!("{}-g.png", v.name),
            &v.pose,
            json!({ "sky": SKY2, "jitter_px": [0.0, 0.0], "sim_time": HOLD_S }),
            "Geometry check: gap view with the second sky colour",
        ));
    }
    // The debug view analyses the whole draw list in the page (some 25 s in
    // a big world), so it is switched on once for all the debug and highlight
    // shots and switched off after them, not per shot. The analysis is the
    // same from any camera and the world is held still.
    steps.push(debug_step(true));
    for v in &s.debug_views {
        steps.push(shot_step(
            &format!("debug-{}", v.name),
            &format!("debug-{}.png", v.name),
            &v.pose,
            json!({ "sky": true, "sim_time": HOLD_S }),
            "Geometry check: debug view for the critic",
        ));
    }
    let picks = pick_highlights(&report.issues, s.highlights);
    let mut hl: Vec<(usize, String, CameraPose)> = Vec::new();
    for (k, &i) in picks.iter().enumerate() {
        let is = &report.issues[i];
        let name = format!("{:02}-{}-{}", k + 1, is.check.name(), safe(&is.kind));
        let pose = highlight_pose(is);
        steps.push(shot_step(
            &format!("hl-{name}"),
            &format!("hl-{name}.png"),
            &pose,
            json!({ "sim_time": HOLD_S }),
            "Geometry check: highlight an issue",
        ));
        hl.push((i, name, pose));
    }
    steps.push(debug_step(false));
    if let Some(p) = s.still_param {
        steps.push(still_step(p, false));
    }
    let plan = json!({ "tier": s.tier, "steps": steps });
    let plan_path = raw.join("plan.json");
    std::fs::write(&plan_path, serde_json::to_string_pretty(&plan).map_err(|e| e.to_string())?)
        .map_err(|e| format!("write plan: {e}"))?;

    // ---- run it
    let script = s.root.join("tools").join("live-check").join("capture.mjs");
    eprintln!("geocheck: {} captures in the live game (port {})", steps.len(), s.port);
    let status = Command::new("node")
        .arg(&script)
        .args(["--game", s.app, "--port", &s.port.to_string(), "--plan"])
        .arg(&plan_path)
        .arg("--out")
        .arg(&raw)
        .current_dir(s.root)
        .status()
        .map_err(|e| format!("node {}: {e}", script.display()))?;
    let result: Value = std::fs::read_to_string(raw.join("plan-result.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .ok_or_else(|| format!("the capture run wrote no plan-result.json (node exited with {status})"))?;
    let failed: Vec<String> = result["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| r["ok"] != Value::Bool(true))
        .map(|r| format!("{}: {}", r["name"].as_str().unwrap_or("?"), r["error"].as_str().unwrap_or("failed")))
        .collect();
    let ok = steps.len() - failed.len();
    report.notes.push(format!(
        "Render phase: {ok} of {} captures in the live game (tier {}).",
        steps.len(),
        result["tier"].as_str().unwrap_or("?")
    ));
    if !failed.is_empty() {
        let shown: Vec<&String> = failed.iter().take(5).collect();
        report.notes.push(format!("Captures that failed ({}): {:?}", failed.len(), shown));
    }

    // ---- sky colour
    let sky = match load(&raw, "sky.png") {
        Some(img) => {
            let c = centre_colour(&img);
            report.notes.push(format!("Sky calibration: the magenta sky renders as rgb({}, {}, {}).", c[0], c[1], c[2]));
            c
        }
        None => {
            report.notes.push("Sky calibration shot missing: assumed rgb(255, 0, 255).".into());
            [255, 0, 255]
        }
    };
    let sky2 = match load(&raw, "sky2.png") {
        Some(img) => {
            let c = centre_colour(&img);
            report.notes.push(format!("Second sky calibration: the green sky renders as rgb({}, {}, {}).", c[0], c[1], c[2]));
            c
        }
        None => {
            report.notes.push("Second sky calibration shot missing: assumed rgb(0, 255, 0).".into());
            [0, 255, 0]
        }
    };
    let gp = GapParams {
        sky,
        ..GapParams::default()
    };
    let fp = FlickerParams::default();
    let ground_y = stat.ground_y();
    let ground_fp = stat.ground_footprint();
    report.notes.push(format!(
        "Gaps: ground at y = {ground_y:.2}, drawn from x {:.0} to {:.0} and z {:.0} to {:.0}; sky past that is the world's edge (its own check), sky past {:.0} m is past the far plane.",
        ground_fp.min[0], ground_fp.max[0], ground_fp.min[2], ground_fp.max[2], gp.far
    ));

    // ---- gaps and flicker per view
    let mut gap_n = 0usize;
    let mut flick_n = 0usize;
    let mut outline_px = 0usize;
    let mut views_done = 0usize;
    for v in &s.views {
        let imgs: Vec<Option<Image>> = JITTER.iter().map(|(t, _)| load(&raw, &format!("{}-{t}.png", v.name))).collect();
        let [Some(a), Some(b), Some(cc), Some(d)] = <[Option<Image>; 4]>::try_from(imgs).unwrap_or([None, None, None, None]) else {
            report.notes.push(format!("View {}: a capture is missing, gaps and flicker not checked there.", v.name));
            continue;
        };
        views_done += 1;
        let cam = Camera::from_pose(&v.pose, f64::from(a.w) / f64::from(a.h.max(1)));
        // Gaps: confirmed against the green-sky shot when there is one.
        let g = load(&raw, &format!("{}-g.png", v.name));
        if g.is_none() {
            report.notes.push(format!("View {}: the green-sky shot is missing; gaps there are not confirmed against it.", v.name));
        }
        let mut blobs = gaps(&a, g.as_ref().map(|g| (g, sky2)), &cam, ground_y, &ground_fp, &gp);
        blobs.sort_by_key(|b| std::cmp::Reverse(b.hits));
        for (k, bl) in blobs.iter().enumerate() {
            let island = !bl.border && bl.pixels <= gp.island_max;
            // Report the pixels that are wrong, not the whole sky region
            // they belong to.
            let (rect, centre, wrong) = (bl.hit_rect, bl.hit_center, bl.hits);
            let (at, hit) = look_at_pixel(stat, &cam, a.w, a.h, centre);
            let mut is = Issue::new(Check::Gap, if island { "crack (enclosed sky)" } else { "sky where the ground should be" }, at);
            is.objects = vec![hit.as_ref().map_or_else(|| "ground".to_string(), |h| h.0.clone())];
            if let Some(h) = &hit {
                is.items = vec![h.1];
            }
            is.aabb = Some(Aabb { min: at, max: at }.grow(1.0));
            is.measure = wrong as f64;
            is.unit = "px".into();
            is.pixels = Some(rect);
            is.detail = if island {
                format!(
                    "view {}: an enclosed crack of {} sky pixels ({} of them where the ground should be) at x={} y={} w={} h={}",
                    v.name, bl.pixels, bl.hits, rect[0], rect[1], rect[2], rect[3]
                )
            } else {
                format!(
                    "view {}: {} sky pixels look down onto the world's ground within the drawn distance, at x={} y={} w={} h={} (part of a {}-pixel sky region)",
                    v.name, bl.hits, rect[0], rect[1], rect[2], rect[3], bl.pixels
                )
            };
            is.fix = "Close the hole: build roads, walls and verges as one continuous spline mesh (evani_render::spline::sweep) instead of separate slabs, extend the ground under the edges, or overlap the pieces that meet there.".into();
            if k < CROPS_PER_VIEW {
                let file = format!("gap-{}-{}.png", v.name, k + 1);
                write_png(&crops.join(&file), &a.crop_outlined(rect, 24, rgb8(Check::Gap.color())))?;
                is.crop = Some(format!("crops/{file}"));
            }
            report.issues.push(is);
            gap_n += 1;
        }
        // Flicker, with what moved between captures masked out.
        let mask = motion_mask(&a, &d, 20, 3);
        let moved = mask.iter().filter(|m| **m).count();
        let (am, bm, cm) = (blacken(&a, &mask), blacken(&b, &mask), blacken(&cc, &mask));
        // Changed pixels on an object's outline are aliasing, not fighting.
        let outlines = Outlines::new(stat, cam, a.w, a.h);
        let dropped = std::cell::Cell::new(0usize);
        let mut fl = flicker_where(&am, &bm, &cm, &fp, &|x, y| {
            let on = outlines.at(x, y);
            dropped.set(dropped.get() + usize::from(on));
            on
        });
        outline_px += dropped.get();
        fl.sort_by_key(|b| std::cmp::Reverse(b.pixels));
        for (k, bl) in fl.iter().enumerate() {
            let (at, hit) = look_at_pixel(stat, &cam, a.w, a.h, bl.center);
            let mut is = Issue::new(Check::ZFight, "flicker under sub-pixel jitter", at);
            is.objects = vec![hit.as_ref().map_or_else(|| "unknown surface".to_string(), |h| h.0.clone())];
            if let Some(h) = &hit {
                is.items = vec![h.1];
            }
            is.aabb = Some(Aabb { min: at, max: at }.grow(1.0));
            is.measure = bl.pixels as f64;
            is.unit = "px".into();
            is.pixels = Some(bl.rect);
            is.detail = format!(
                "view {}: {} pixels change surface when the camera moves under a pixel ({} moving pixels masked)",
                v.name, bl.pixels, moved
            );
            is.fix = "Two surfaces fight for the same depth: lift paint and decals with the decal pass (RenderWorld::draw_decal), remove the hidden face, or separate the surfaces by a few centimetres.".into();
            if k < CROPS_PER_VIEW {
                let file = format!("flicker-{}-{}.png", v.name, k + 1);
                write_png(&crops.join(&file), &a.crop_outlined(bl.rect, 24, rgb8(Check::ZFight.color())))?;
                is.crop = Some(format!("crops/{file}"));
            }
            report.issues.push(is);
            flick_n += 1;
        }
    }
    report.ran.push((
        Check::Gap,
        format!(
            "{views_done} of {} views (orbit and track-level) rendered with a magenta sky; sky pixels whose view ray lands on the world's ground within the drawn distance (and still does 2 pixels higher) and that follow the sky in a second, green-sky shot are gaps (small enclosed ones are cracks): {gap_n} found.",
            s.views.len()
        ),
    ));
    report.notes.push(format!(
        "Flicker: the same {views_done} views rendered at jitter (0,0), (1,0), (0.5,0) and (0,0) again{}; pixels that change surface (not just shift) in flat areas are z-fighting, except within 1.5 px of an object's outline (found by ray casting the scene: aliasing, {outline_px} such pixels set aside): {flick_n} found.",
        match s.still_param {
            Some(p) => format!(" with the world held still and only static objects drawn ({p} on, clock held at {HOLD_S} s)"),
            None => " (the game runs on between captures; what moved is masked out)".to_string(),
        }
    ));

    // ---- debug views for the critic
    for v in &s.debug_views {
        let f = format!("debug-{}.png", v.name);
        if let Some(img) = load(&raw, &f) {
            write_png(&views_dir.join(&f), &img)?;
        }
    }

    // ---- highlight crops
    let mut done = 0usize;
    for (i, name, pose) in &hl {
        let Some(img) = load(&raw, &format!("hl-{name}.png")) else { continue };
        let is = &mut report.issues[*i];
        let cam = Camera::from_pose(pose, f64::from(img.w) / f64::from(img.h.max(1)));
        let rect = cam.project_box(img.w, img.h, &issue_box(is)).unwrap_or([0, 0, img.w, img.h]);
        let file = format!("{name}.png");
        write_png(&crops.join(&file), &img.crop_outlined(rect, 24, rgb8(is.check.color())))?;
        is.crop = Some(format!("crops/{file}"));
        is.pixels = Some(rect);
        done += 1;
    }
    report
        .notes
        .push(format!("Highlights: {done} issue captures with the debug view on (the issue's box outlined), in crops/."));

    if !s.keep_raw {
        let _ = std::fs::remove_dir_all(&raw);
    }
    Ok(())
}

/// Where the report goes.
pub fn out_dir(root: &Path, app: &str) -> PathBuf {
    root.join(".vibedeck").join("geocheck").join(app)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motion_mask_grows_changes() {
        let mut a = Image::new(20, 10);
        let d = a.clone();
        a.set(10, 5, [200, 0, 0, 255]);
        let m = motion_mask(&a, &d, 20, 3);
        assert!(m[(5 * 20 + 13) as usize] && m[(2 * 20 + 10) as usize]);
        assert!(!m[(5 * 20 + 14) as usize] && !m[(5 * 20 + 6) as usize]);
    }

    #[test]
    fn highlights_take_group_heads_first() {
        let mut v = Vec::new();
        for k in 0..3 {
            let mut i = Issue::new(Check::Overlap, "seam", [0.0; 3]);
            i.objects = vec!["road".into()];
            i.measure = f64::from(k);
            v.push(i);
        }
        let mut f = Issue::new(Check::Floating, "gap", [0.0; 3]);
        f.objects = vec!["tree".into()];
        v.push(f);
        let p = pick_highlights(&v, 2);
        assert_eq!(p.len(), 2);
        assert!(p.contains(&2) && p.contains(&3), "{p:?}");
    }

    #[test]
    fn safe_names() {
        assert_eq!(safe("sky where the ground"), "sky-where-the-ground");
        assert_eq!(safe("a  (b)"), "a-b");
    }
}
