//! `--zfight`: the native recording and check 4's geometry test only
//! (`evani_geocheck::zfight`), written as `zfight/zfight.json` and
//! `zfight/zfight.md`, plus debug images from the game's own cameras with
//! each flickering patch outlined (skip them with `--no-render`).
//!
//! The images are captures in the live game (`tools/live-check/capture.mjs
//! --plan`, the still-world switch on so only the static world is drawn):
//! `overview.png` from the title camera that sees the most patches, and one
//! `NN-<kind>.png` per patch (at most `--highlights`) from the title camera
//! that sees that patch largest (the race camera when no title camera sees
//! it), its fight polygon outlined in magenta and its overlap box in cyan.

use std::path::{Path, PathBuf};
use std::process::Command;

use evani_core::camera::CameraPose;
use evani_geocheck::geom::*;
use evani_geocheck::image::{Camera, Image};
use evani_geocheck::report::Issue;
use evani_geocheck::scene::Scene;
use evani_geocheck::zfight::{DepthSetup, Viewer};
use serde_json::{Value, json};

use super::png;
use super::render::{shot_step, still_step};

pub struct Setup<'a> {
    pub root: &'a Path,
    pub app: &'a str,
    /// The app's geocheck folder; this writes into its `zfight/`.
    pub out: &'a Path,
    pub port: u16,
    pub tier: Option<String>,
    pub still_param: Option<&'a str>,
    pub render: bool,
    /// Most per-patch images.
    pub shots: usize,
    /// The report's folder under `out`: `zfight`, or `zfight-sample` for a
    /// partial (`--sample`) report, which never replaces the full one.
    pub dir_name: &'a str,
    /// `--sample`: (cameras scanned, cameras recorded). The report says it
    /// is partial and `cargo xtask check` refuses it.
    pub partial: Option<(usize, usize)>,
    /// The capture cache's `shots/`: raw live captures by camera pose,
    /// reused instead of capturing again.
    pub shots_cache: Option<PathBuf>,
    /// `--report-only`: every debug image must come from `shots_cache`.
    pub report_only: bool,
}

/// The folder a z-fight report goes to: a partial one never lands in
/// `zfight/`, where the full report is kept.
pub fn dir_name(partial: bool) -> &'static str {
    if partial { "zfight-sample" } else { "zfight" }
}

/// Overview colours, one per cause group (largest group first); groups past
/// the list are drawn grey. Their names are in the Markdown summary.
const GROUP_COLOURS: [([u8; 4], &str); 8] = [
    ([255, 0, 255, 255], "magenta"),
    ([0, 255, 255, 255], "cyan"),
    ([255, 230, 0, 255], "yellow"),
    ([255, 128, 0, 255], "orange"),
    ([0, 200, 0, 255], "green"),
    ([40, 80, 255, 255], "blue"),
    ([230, 0, 0, 255], "red"),
    ([255, 255, 255, 255], "white"),
];
const GREY: [u8; 4] = [128, 128, 128, 255];
const MAGENTA: [u8; 4] = GROUP_COLOURS[0].0;
const CYAN: [u8; 4] = GROUP_COLOURS[1].0;

/// An issue's cause: its kind and objects. Issues of one cause share a fix.
/// An object against itself (a thin slab, or parts of one object) is named
/// once.
fn cause(i: &Issue) -> String {
    match i.objects.as_slice() {
        [a, b] if a == b => format!("{}: {a} (its own faces)", i.kind),
        o => format!("{}: {}", i.kind, o.join(" / ")),
    }
}

/// Where a group is: its largest patch's position.
fn at_text(i: &Issue) -> String {
    format!("({}, {}, {})", f1(i.at[0]), f1(i.at[1]), f1(i.at[2]))
}

/// One decimal, with no "-0.0".
fn f1(x: f64) -> String {
    let s = format!("{x:.1}");
    if s == "-0.0" { "0.0".into() } else { s }
}

/// Cause groups, largest first (ties: first seen): each group's cause and
/// its issues' indices, and each issue's group.
fn groups(issues: &[Issue]) -> (Vec<(String, Vec<usize>)>, Vec<usize>) {
    let mut gs: Vec<(String, Vec<usize>)> = Vec::new();
    for (n, i) in issues.iter().enumerate() {
        let c = cause(i);
        match gs.iter_mut().find(|g| g.0 == c) {
            Some(g) => g.1.push(n),
            None => gs.push((c, vec![n])),
        }
    }
    gs.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.1[0].cmp(&b.1[0])));
    let mut of = vec![0; issues.len()];
    for (g, (_, list)) in gs.iter().enumerate() {
        for &n in list {
            of[n] = g;
        }
    }
    (gs, of)
}

fn group_colour(g: usize) -> [u8; 4] {
    GROUP_COLOURS.get(g).map_or(GREY, |c| c.0)
}

fn colour_name(g: usize) -> &'static str {
    GROUP_COLOURS.get(g).map_or("grey", |c| c.1)
}

fn polygon(i: &Issue) -> Vec<V3> {
    i.data["polygon"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let a = p.as_array()?;
            Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2)?.as_f64()?])
        })
        .collect()
}

fn pose_of(c: &Camera) -> CameraPose {
    let f = |p: V3| p.map(|x| x as f32);
    CameraPose {
        eye: f(c.eye),
        target: f(c.target),
        up: f(c.up),
        fov_y_deg: c.fov_y_deg as f32,
    }
}

/// Render size of a viewer (pixels).
fn size(v: &Viewer) -> (u32, u32) {
    let h = v.height_px.round().max(1.0) as u32;
    ((v.height_px * v.cam.aspect).round().max(1.0) as u32, h)
}

/// Whether the eye of `v` sees `p` (no static surface in between, the
/// issue's own items and decals aside).
fn in_sight(stat: &Scene, v: &Viewer, p: V3, own: &[usize]) -> bool {
    let r = sub(p, v.cam.eye);
    let t = len(r);
    if t < 1e-6 {
        return true;
    }
    let dir = mul(r, 1.0 / t);
    stat.ray_surface(v.cam.eye, dir, t - 0.05, &|id| own.contains(&stat.items[id].index) || stat.items[id].decal)
        .is_none()
}

/// How well viewer `v` shows the patch: its pixel area on screen when its
/// centre is in the frame, in sight and within reach; 0 otherwise.
fn showing(stat: &Scene, v: &Viewer, i: &Issue, reach: f64) -> f64 {
    let (w, h) = size(v);
    let pts = polygon(i);
    let Some(c) = v.cam.project(w, h, i.at) else { return 0.0 };
    if c[0] < 0.0 || c[1] < 0.0 || c[0] > f64::from(w) || c[1] > f64::from(h) {
        return 0.0;
    }
    if dot(sub(i.at, v.cam.eye), v.cam.basis().0) > reach || !in_sight(stat, v, i.at, &i.items) {
        return 0.0;
    }
    let px: Vec<[f64; 2]> = pts.iter().filter_map(|p| v.cam.project(w, h, *p)).collect();
    if px.len() < 3 {
        return 1.0;
    }
    area_2d(&px).abs().max(1.0)
}

/// The viewers the detector found the fight for (static issues).
fn fight_viewers(i: &Issue) -> Vec<usize> {
    i.data["seen_from"]["viewers"].as_array().into_iter().flatten().filter_map(|v| v.as_u64()).map(|v| v as usize).collect()
}

/// The camera for issue `i`'s image: the title camera where the detector
/// found the fight largest, else its worst camera of any kind, else (a
/// runtime issue) the title viewer (or any) that shows it best.
fn best_viewer(stat: &Scene, viewers: &[Viewer], i: &Issue, reach: f64) -> Option<usize> {
    let from = &i.data["seen_from"];
    for label in ["title", "race", "orbit"] {
        if let Some(k) = from[label]["worst"]["viewer"].as_u64().map(|k| k as usize).filter(|k| *k < viewers.len()) {
            return Some(k);
        }
    }
    let pick = |title: bool| {
        viewers
            .iter()
            .enumerate()
            .filter(|(_, v)| (v.label == "title") == title)
            .map(|(k, v)| (k, showing(stat, v, i, reach)))
            .filter(|(_, s)| *s > 0.0)
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(k, _)| k)
    };
    pick(true).or_else(|| pick(false))
}

/// The title viewer that the most issues fight for (ties: the most pixels
/// in sight).
fn overview_viewer(stat: &Scene, viewers: &[Viewer], issues: &[Issue], reach: f64) -> Option<usize> {
    let titles: Vec<usize> = (0..viewers.len()).filter(|&k| viewers[k].label == "title").collect();
    let pool = if titles.is_empty() { (0..viewers.len()).collect() } else { titles };
    let lists: Vec<Vec<usize>> = issues.iter().map(fight_viewers).collect();
    let by_fight = pool
        .iter()
        .map(|&k| (k, lists.iter().filter(|l| l.contains(&k)).count()))
        .filter(|(_, n)| *n > 0)
        .max_by_key(|(_, n)| *n);
    if let Some((k, _)) = by_fight {
        return Some(k);
    }
    pool.into_iter()
        .map(|k| {
            let s: Vec<f64> = issues.iter().map(|i| showing(stat, &viewers[k], i, reach)).collect();
            (k, s.iter().filter(|x| **x > 0.0).count(), s.iter().sum::<f64>())
        })
        .filter(|(_, n, _)| *n > 0)
        .max_by(|a, b| a.1.cmp(&b.1).then(a.2.total_cmp(&b.2)))
        .map(|(k, _, _)| k)
}

fn line(img: &mut Image, a: [f64; 2], b: [f64; 2], c: [u8; 4]) {
    let n = ((b[0] - a[0]).abs().max((b[1] - a[1]).abs()).ceil() as usize).clamp(1, 20_000);
    for k in 0..=n {
        let t = k as f64 / n as f64;
        let (x, y) = (a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t);
        for (dx, dy) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0)] {
            let (px, py) = ((x + dx).round(), (y + dy).round());
            if px >= 0.0 && py >= 0.0 && px < f64::from(img.w) && py < f64::from(img.h) {
                img.set(px as u32, py as u32, c);
            }
        }
    }
}

/// Draw issue `i`'s fight polygon as `cam` sees it in colour `c`, and
/// return the box round it (padded, so a patch a pixel or two wide still
/// shows), or None when its centre is behind the camera.
fn outline_polygon(img: &mut Image, cam: &Camera, i: &Issue, c: [u8; 4]) -> Option<[f64; 4]> {
    let (w, h) = (img.w, img.h);
    let px: Vec<[f64; 2]> = polygon(i).iter().filter_map(|p| cam.project(w, h, *p)).collect();
    for k in 0..px.len() {
        line(img, px[k], px[(k + 1) % px.len()], c);
    }
    let ctr = cam.project(w, h, i.at)?;
    let (mut lo, mut hi) = (ctr, ctr);
    for p in &px {
        lo = [lo[0].min(p[0]), lo[1].min(p[1])];
        hi = [hi[0].max(p[0]), hi[1].max(p[1])];
    }
    let pad = 6.0;
    Some([lo[0] - pad, lo[1] - pad, hi[0] + pad, hi[1] + pad])
}

/// A box kept inside the image, so one that runs off the edge still closes.
fn draw_box(img: &mut Image, b: [f64; 4], c: [u8; 4]) {
    let (mx, my) = (f64::from(img.w.max(1) - 1), f64::from(img.h.max(1) - 1));
    if b[2] < 0.0 || b[0] > mx || b[3] < 0.0 || b[1] > my {
        return;
    }
    let [x0, y0, x1, y1] = [b[0].clamp(0.0, mx), b[1].clamp(0.0, my), b[2].clamp(0.0, mx), b[3].clamp(0.0, my)];
    for (a, b) in [([x0, y0], [x1, y0]), ([x1, y0], [x1, y1]), ([x1, y1], [x0, y1]), ([x0, y1], [x0, y0])] {
        line(img, a, b, c);
    }
}

/// Outline issue `i` alone: its polygon in magenta, its box in cyan.
fn outline(img: &mut Image, cam: &Camera, i: &Issue) {
    if let Some(b) = outline_polygon(img, cam, i, MAGENTA) {
        draw_box(img, b, CYAN);
    }
}

/// Outline many issues, each polygon and box in its cause group's colour
/// (`group[n]`); touching boxes of one group merge into one, so a row of
/// patches (a banner of many cubes) gets one box.
fn outline_groups(img: &mut Image, cam: &Camera, issues: &[Issue], list: &[usize], group: &[usize]) {
    let mut boxes: Vec<(usize, [f64; 4])> = Vec::new();
    for &n in list {
        let g = group[n];
        if let Some(b) = outline_polygon(img, cam, &issues[n], group_colour(g)) {
            boxes.push((g, b));
        }
    }
    let touch = |a: &[f64; 4], b: &[f64; 4]| a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3];
    let mut merged = true;
    while merged {
        merged = false;
        'outer: for x in 0..boxes.len() {
            for y in x + 1..boxes.len() {
                if boxes[x].0 == boxes[y].0 && touch(&boxes[x].1, &boxes[y].1) {
                    let (a, b) = (boxes[x].1, boxes[y].1);
                    boxes[x].1 = [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])];
                    boxes.swap_remove(y);
                    merged = true;
                    break 'outer;
                }
            }
        }
    }
    for (g, b) in boxes {
        draw_box(img, b, group_colour(g));
    }
}

/// A 3 x 5 pixel glyph (rows top down, bit 2 = left column) for the
/// overview's colour key; unknown characters are blank.
fn glyph(c: char) -> [u8; 5] {
    match c.to_ascii_uppercase() {
        'A' => [2, 5, 7, 5, 5],
        'B' => [6, 5, 6, 5, 6],
        'C' => [3, 4, 4, 4, 3],
        'D' => [6, 5, 5, 5, 6],
        'E' => [7, 4, 6, 4, 7],
        'F' => [7, 4, 6, 4, 4],
        'G' => [3, 4, 5, 5, 3],
        'H' => [5, 5, 7, 5, 5],
        'I' => [7, 2, 2, 2, 7],
        'J' => [1, 1, 1, 5, 2],
        'K' => [5, 5, 6, 5, 5],
        'L' => [4, 4, 4, 4, 7],
        'M' => [5, 7, 7, 5, 5],
        'N' => [6, 5, 5, 5, 5],
        'O' => [2, 5, 5, 5, 2],
        'P' => [6, 5, 6, 4, 4],
        'Q' => [2, 5, 5, 6, 3],
        'R' => [6, 5, 6, 5, 5],
        'S' => [3, 4, 2, 1, 6],
        'T' => [7, 2, 2, 2, 2],
        'U' => [5, 5, 5, 5, 7],
        'V' => [5, 5, 5, 5, 2],
        'W' => [5, 5, 7, 7, 5],
        'X' => [5, 5, 2, 5, 5],
        'Y' => [5, 5, 2, 2, 2],
        'Z' => [7, 1, 2, 4, 7],
        '0' => [7, 5, 5, 5, 7],
        '1' => [2, 6, 2, 2, 7],
        '2' => [6, 1, 2, 4, 7],
        '3' => [6, 1, 2, 1, 6],
        '4' => [5, 5, 7, 1, 1],
        '5' => [7, 4, 6, 1, 6],
        '6' => [3, 4, 7, 5, 7],
        '7' => [7, 1, 2, 2, 2],
        '8' => [7, 5, 7, 5, 7],
        '9' => [7, 5, 7, 1, 6],
        ':' => [0, 2, 0, 2, 0],
        '(' => [1, 2, 2, 2, 1],
        ')' => [4, 2, 2, 2, 4],
        '.' => [0, 0, 0, 0, 2],
        ',' => [0, 0, 0, 2, 4],
        '-' => [0, 0, 7, 0, 0],
        '+' => [0, 2, 7, 2, 0],
        '_' => [0, 0, 0, 0, 7],
        '/' => [1, 1, 2, 4, 4],
        _ => [0; 5],
    }
}

/// Text at (x, y) in 3 x 5 glyphs scaled by `s`; returns the width drawn.
fn text(img: &mut Image, x: u32, y: u32, s: u32, t: &str, c: [u8; 4]) -> u32 {
    let mut cx = x;
    for ch in t.chars() {
        for (row, bits) in glyph(ch).iter().enumerate() {
            for col in 0..3u32 {
                if bits & (4 >> col) != 0 {
                    for (dx, dy) in (0..s).flat_map(|a| (0..s).map(move |b| (a, b))) {
                        let (px, py) = (cx + col * s + dx, y + row as u32 * s + dy);
                        if px < img.w && py < img.h {
                            img.set(px, py, c);
                        }
                    }
                }
            }
        }
        cx += 4 * s;
    }
    cx - x
}

/// The overview's colour key, top left: one line per coloured group drawn
/// in it (group number and short cause), on a pale panel.
fn colour_key(img: &mut Image, lines: &[(usize, String)]) {
    if lines.is_empty() {
        return;
    }
    let (s, line_h, pad, sw) = (2u32, 14u32, 6u32, 16u32);
    let longest = lines.iter().map(|l| l.1.chars().count() as u32).max().unwrap_or(0);
    let (w, h) = ((pad * 3 + sw + longest * 4 * s).min(img.w), (pad * 2 + line_h * lines.len() as u32).min(img.h));
    for y in 0..h {
        for x in 0..w {
            let p = img.px(x, y);
            img.set(x, y, [(p[0] / 4) + 191, (p[1] / 4) + 191, (p[2] / 4) + 191, 255]);
        }
    }
    for (k, (g, t)) in lines.iter().enumerate() {
        let y = pad + line_h * k as u32;
        for (dx, dy) in (0..sw).flat_map(|a| (0..10u32).map(move |b| (a, b))) {
            if pad + dx < img.w && y + dy < img.h {
                img.set(pad + dx, y + dy, group_colour(*g));
            }
        }
        text(img, pad * 2 + sw, y, s, t, [0, 0, 0, 255]);
    }
}

/// The key's line for group `g`: "N COLOUR KIND: FIRST OBJECT", cut short.
fn key_line(g: usize, i: &Issue) -> String {
    let obj = i.objects.first().map_or("", String::as_str);
    let t = format!("{} {} {}: {obj}", g + 1, colour_name(g), i.kind);
    t.chars().take(48).collect()
}

fn safe(s: &str) -> String {
    let t: String = s.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    t.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-")
}

/// Two decimals, with no "-0.00".
fn f2(x: f64) -> String {
    let s = format!("{x:.2}");
    if s == "-0.00" { "0.00".into() } else { s }
}

fn mm(x: f64) -> String {
    f2(x * 1000.0)
}

/// A thin slab's "separation" is its underside below its top: shown as a
/// positive thickness. Other kinds: the separation as it is.
fn gaps_of(i: &Issue) -> [f64; 2] {
    let s = arr2(&i.data["separation_m"]);
    if i.kind == "thin slab" {
        let (a, b) = (s[0].abs(), s[1].abs());
        [a.min(b), a.max(b)]
    } else {
        s
    }
}

/// The gap column: "lo to hi" (mm), "thick" for a thin slab.
fn gap_text(kind: &str, [lo, hi]: [f64; 2]) -> String {
    let r = if mm(lo) == mm(hi) { mm(hi) } else { format!("{} to {}", mm(lo), mm(hi)) };
    if kind == "thin slab" { format!("{r} thick") } else { r }
}

/// The worst title-camera view of a static issue: (distance m, epsilon m).
fn title_view(i: &Issue) -> Option<([f64; 2], f64)> {
    let w = &i.data["seen_from"]["title"]["worst"];
    Some((arr2(&w["view_distance_m"]), w["epsilon_m"].as_f64()?))
}

/// "lo to hi" of `v` times `scale` with `dp` decimals, "-" when empty.
fn span(v: &[f64], scale: f64, dp: usize) -> String {
    if v.is_empty() {
        return "-".into();
    }
    let (lo, hi) = v.iter().fold((f64::MAX, f64::MIN), |(l, h), x| (l.min(*x), h.max(*x)));
    format!("{:.*} to {:.*}", dp, lo * scale, dp, hi * scale)
}

/// The patch whose fix speaks for its group: the one that needs the most
/// separation, so the group's fix holds for every patch in it.
fn group_fix(issues: &[Issue], list: &[usize]) -> usize {
    let need = |n: usize| issues[n].data["fix_epsilon_m"].as_f64().unwrap_or(0.0);
    list.iter().copied().fold(list[0], |best, n| if need(n) > need(best) { n } else { best })
}

fn arr2(v: &Value) -> [f64; 2] {
    [v[0].as_f64().unwrap_or(0.0), v[1].as_f64().unwrap_or(0.0)]
}

/// Write the JSON and Markdown and, with `render`, the debug images.
/// `issues` are already tagged. Returns the number of untagged issues.
pub fn write(s: &Setup, stat: &Scene, viewers: &[Viewer], issues: &mut [Issue], ran: &str, notes: &[String]) -> Result<usize, String> {
    let dir = s.out.join(s.dir_name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| format!("clear {}: {e}", dir.display()))?;
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let set = DepthSetup::engine();
    let reach = stat.cfg.view_distance.min(set.far);
    let mut notes = notes.to_vec();
    // Worst first: most pixels flickering.
    issues.sort_by(|a, b| b.measure.total_cmp(&a.measure));
    let mut shots: Vec<Option<String>> = vec![None; issues.len()];
    let (gs, group) = groups(issues);
    if s.render && !issues.is_empty() {
        notes.extend(render(s, &dir, stat, viewers, issues, reach, &mut shots)?);
    } else if !s.render {
        notes.push("Debug images skipped (--no-render).".into());
    }

    // ---- JSON
    let list: Vec<Value> = issues
        .iter()
        .zip(&shots)
        .map(|(i, f)| {
            let mut v = i.to_json();
            if let Some(o) = v.as_object_mut() {
                o.insert("capture".into(), json!(f));
            }
            v
        })
        .collect();
    let group_list: Vec<Value> = gs
        .iter()
        .enumerate()
        .map(|(g, (c, list))| {
            json!({
                "group": g + 1, "cause": c, "largest_at": issues[list[0]].at, "patches": list.len(),
                "rows": list.iter().map(|n| n + 1).collect::<Vec<_>>(),
                "overview_colour": colour_name(g),
                "fix": issues[group_fix(issues, list)].fix,
            })
        })
        .collect();
    let untagged = issues.iter().filter(|i| i.tagged.is_none()).count();
    let js = json!({
        "app": s.app,
        "check": "zfight",
        "depth": {
            "format": set.format.name(), "near": set.near, "far": set.far, "reversed": set.reversed,
            "a": set.a, "b": set.b, "decal_offset": set.decal_offset, "subpixel_bits": set.subpixel_bits,
            "unit_roundoff": set.unit_roundoff,
        },
        "formula": FORMULA,
        "partial": s.partial.is_some(),
        "cameras": viewers.len(),
        "cameras_total": s.partial.map_or(viewers.len(), |p| p.1),
        "ran": ran,
        "notes": notes,
        "total": issues.len(),
        "untagged": untagged,
        "groups": group_list,
        "issues": list,
    });
    std::fs::write(dir.join("zfight.json"), serde_json::to_string_pretty(&js).map_err(|e| e.to_string())?)
        .map_err(|e| format!("write zfight.json: {e}"))?;

    // ---- Markdown
    let mut md = match s.partial {
        Some((n, m)) => format!(
            "# Z-fighting: {} (PARTIAL: {n} of {m} cameras, --sample {n})\n\n\
             A partial report: a quick check from a fixed sample of the recorded cameras, not proof. \
             `cargo xtask check` refuses it; run without --sample for the full scan.\n\n",
            s.app
        ),
        None => format!("# Z-fighting: {}\n\n", s.app),
    };
    md.push_str(&format!(
        "{} flickering patch(es), {untagged} untagged, seen from {} recorded cameras.\n\n",
        issues.len(),
        viewers.len()
    ));
    md.push_str(&format!(
        "Depth: {}, near {:.2} m, far {:.0} m, {}, decal offset {:.1e}.\n\n{FORMULA}\n\n",
        set.format.name(),
        set.near,
        set.far,
        reversed_words(&set),
        set.decal_offset
    ));
    md.push_str(&format!("What ran: {ran}\n\n"));
    md.push_str(
        "Untagged: not marked as intended in the kit's `geocheck.toml`, so it counts as a bug to fix (or to tag with a reason). \
         Each row of \"Every patch\" is one patch; patches with the same kind and objects share a cause and a fix, grouped under \"By cause\".\n\n",
    );
    if !issues.is_empty() {
        md.push_str("## By cause\n\n");
        md.push_str("| group | overview colour | cause (kind: objects; largest patch at x, y, z) | patches | gap mm | title camera m | title epsilon mm | rows | fix |\n");
        md.push_str("|---|---|---|---|---|---|---|---|---|\n");
        for (g, (c, list)) in gs.iter().enumerate() {
            let fix = &issues[group_fix(issues, list)].fix;
            let gaps: Vec<f64> = list.iter().flat_map(|&n| gaps_of(&issues[n])).collect();
            let (glo, ghi) = gaps.iter().fold((f64::MAX, f64::MIN), |(l, h), x| (l.min(*x), h.max(*x)));
            let tz: Vec<f64> = list.iter().filter_map(|&n| title_view(&issues[n]).map(|t| t.0[0])).collect();
            let te: Vec<f64> = list.iter().filter_map(|&n| title_view(&issues[n]).map(|t| t.1)).collect();
            let rows: Vec<String> = list.iter().take(5).map(|n| (n + 1).to_string()).collect();
            md.push_str(&format!(
                "| {} | {} | {c}; at {} | {} | {} | {} | {} | {}{} | {} |\n",
                g + 1,
                colour_name(g),
                at_text(&issues[list[0]]),
                list.len(),
                gap_text(&issues[list[0]].kind, [glo, ghi]),
                span(&tz, 1.0, 0),
                span(&te, 1000.0, 1),
                rows.join(", "),
                if list.len() > rows.len() { ", ..." } else { "" },
                fix
            ));
        }
        md.push_str(&format!("\n{} patches in {} groups.\n\n## Every patch\n\n", issues.len(), gs.len()));
        md.push_str("| # | group | kind | objects | where (x, y, z) | overlap m2 | gap mm | epsilon mm (worst view) | decal offset mm | worst view m | title camera m | title epsilon mm | from | px | static/runtime | image |\n");
        md.push_str("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|\n");
        for (k, (i, f)) in issues.iter().zip(&shots).enumerate() {
            let d = &i.data;
            let z = arr2(&d["view_distance_m"]);
            let (tz, te) = title_view(i).map_or(("-".to_string(), "-".to_string()), |(t, e)| (format!("{:.0} to {:.0}", t[0], t[1]), mm(e)));
            let vw = &d["viewer"];
            let mut from = format!(
                "{}{}",
                vw["label"].as_str().unwrap_or("?"),
                vw["time"].as_f64().map_or(String::new(), |t| format!(" {t:.1} s"))
            );
            if let Some(m) = d["seen_from"].as_object() {
                let per: Vec<String> = m
                    .iter()
                    .filter(|(l, _)| l.as_str() != "viewers")
                    .map(|(l, v)| {
                        let w = &v["worst"];
                        format!(
                            "{l}: {} cams, worst {:.1} s at {:.0} m, eps {} mm",
                            v["cameras"].as_u64().unwrap_or(0),
                            w["time"].as_f64().unwrap_or(0.0),
                            arr2(&w["view_distance_m"])[0],
                            mm(w["epsilon_m"].as_f64().unwrap_or(0.0))
                        )
                    })
                    .collect();
                if !per.is_empty() {
                    from = format!("{from} ({})", per.join("; "));
                }
            }
            md.push_str(&format!(
                "| {} | {} | {}{} | {} | {}, {}, {} | {:.3} | {} | {} | {} | {:.0} to {:.0} | {} | {} | {} | {:.0} | {} | {} |\n",
                k + 1,
                group[k] + 1,
                i.kind,
                if i.tagged.is_some() { " (tagged)" } else { "" },
                i.objects.join(" / "),
                f2(i.at[0]),
                f2(i.at[1]),
                f2(i.at[2]),
                d["overlap_m2"].as_f64().unwrap_or(0.0),
                gap_text(&i.kind, gaps_of(i)),
                mm(d["epsilon_m"].as_f64().unwrap_or(0.0)),
                mm(d["decal_offset_m"].as_f64().unwrap_or(0.0)),
                z[0],
                z[1],
                tz,
                te,
                from,
                i.measure,
                if d["runtime"] == true { "runtime" } else { "static" },
                f.as_deref().unwrap_or("-")
            ));
        }
        md.push('\n');
        for (k, i) in issues.iter().enumerate() {
            md.push_str(&format!("{}. **{}**: {}\n   Fix: {}\n", k + 1, i.kind, i.detail, i.fix));
            if let Some(t) = &i.tagged {
                md.push_str(&format!("   Tagged: {t}\n"));
            }
        }
        md.push('\n');
    }
    if !notes.is_empty() {
        md.push_str("## Notes\n\n");
        for n in &notes {
            md.push_str(&format!("- {n}\n"));
        }
    }
    std::fs::write(dir.join("zfight.md"), md).map_err(|e| format!("write zfight.md: {e}"))?;
    if s.partial.is_some() {
        // Never committed as evidence.
        std::fs::write(dir.join(".gitignore"), "*\n").map_err(|e| format!("write .gitignore: {e}"))?;
    }
    Ok(untagged)
}

/// How the reports name the depth direction.
pub fn reversed_words(set: &DepthSetup) -> &'static str {
    if set.reversed { "reversed (near 1, far 0)" } else { "not reversed" }
}

/// The formula, for the outputs (docs/GEOCHECK.md "Z-fighting by the depth
/// maths" explains it).
pub const FORMULA: &str = "Two surfaces fight where |delta| < eps at the same pixel: delta = (d_B + t off_B) - (d_A + t off_A) with d = a + b/z \
and t the sign toward the camera (+1 for reversed depth), \
eps = step(d) + xf_A + xf_B + snap_A + snap_B; step = the depth format's spacing at d (f32: one ulp), \
xf = 2u (S2 + |d| S3) / z (f32 vertex transform, u = 2^-24), snap = 0.5 * 2^-8 * |b| * px * (|n.s| + |n.u|) / D (sub-pixel snapping times the depth slope; \
px = 2 tan(fov/2) / height, D = eye-to-plane distance). In metres along the normal: eps / k with k = |b| (r.f) / (z^2 |n.r|).";

/// The cache name of a shot's raw capture: what the game is asked to draw
/// (tier, still-world switch, the step without its name, file and note).
fn shot_key(s: &Setup, step: &Value) -> String {
    let mut v = step.clone();
    if let Some(o) = v.as_object_mut() {
        for k in ["name", "save", "note"] {
            o.remove(k);
        }
    }
    let what = json!({ "tier": s.tier, "still": s.still_param, "step": v });
    super::cache::sha_hex(what.to_string().as_bytes())[..24].to_string()
}

/// The debug images' raw captures `write` would use for these (untagged
/// or tagged) issues: (file, cache name in `shots_cache`).
#[cfg(test)]
pub fn planned_shots(s: &Setup, stat: &Scene, viewers: &[Viewer], issues: &[Issue]) -> Vec<(String, String)> {
    let mut issues = issues.to_vec();
    issues.sort_by(|a, b| b.measure.total_cmp(&a.measure));
    let reach = stat.cfg.view_distance.min(DepthSetup::engine().far);
    let mut shots = vec![None; issues.len()];
    let (_, plan, steps) = plan_shots(s, stat, viewers, &issues, reach, &mut shots);
    plan.into_iter().zip(&steps).map(|((file, _, _), st)| (file, shot_key(s, st))).collect()
}

/// (file, camera, issues outlined) per debug image.
type Plan = Vec<(String, Camera, Vec<usize>)>;

/// Which debug images to make and from which camera, with their notes
/// and capture steps; sets each patch's image path in `shots`.
fn plan_shots(s: &Setup, stat: &Scene, viewers: &[Viewer], issues: &[Issue], reach: f64, shots: &mut [Option<String>]) -> (Vec<String>, Plan, Vec<Value>) {
    let mut notes = Vec::new();
    let mut plan: Plan = Vec::new();
    if let Some(k) = overview_viewer(stat, viewers, issues, reach) {
        let all: Vec<usize> = (0..issues.len()).filter(|&i| fight_viewers(&issues[i]).contains(&k)).collect();
        notes.push(format!(
            "overview.png: the {} camera at {:.1} s, where {} of the {} patches fight, outlined in their cause group's colour (see By cause); touching boxes of one group are merged.",
            viewers[k].label,
            viewers[k].time.unwrap_or(0.0),
            all.len(),
            issues.len()
        ));
        plan.push(("overview.png".into(), viewers[k].cam, all));
    }
    for (n, i) in issues.iter().enumerate().take(s.shots) {
        let Some(k) = best_viewer(stat, viewers, i, reach) else {
            notes.push(format!("Patch {}: no recorded camera has it in sight within {reach} m; no image.", n + 1));
            continue;
        };
        let file = format!("{:02}-{}.png", n + 1, safe(&i.kind));
        shots[n] = Some(format!("{}/{file}", s.dir_name));
        plan.push((file, viewers[k].cam, vec![n]));
    }
    let shot_steps: Vec<Value> = plan
        .iter()
        .map(|(file, cam, _)| {
            shot_step(
                file.trim_end_matches(".png"),
                file,
                &pose_of(cam),
                json!({ "sim_time": 30.0 }),
                "Z-fight check: a recorded game camera",
            )
        })
        .collect();
    (notes, plan, shot_steps)
}

fn render(
    s: &Setup,
    dir: &Path,
    stat: &Scene,
    viewers: &[Viewer],
    issues: &[Issue],
    reach: f64,
    shots: &mut [Option<String>],
) -> Result<Vec<String>, String> {
    let raw = dir.join("raw");
    let group = groups(issues).1;
    let (mut notes, plan, shot_steps) = plan_shots(s, stat, viewers, issues, reach, shots);
    let mut steps: Vec<Value> = Vec::new();
    if let Some(p) = s.still_param {
        steps.push(still_step(p, true));
    }
    // The cached raw capture of each shot, by what the game is asked to
    // draw (tier, still switch, camera, time), not by its file name.
    let cached: Vec<Option<PathBuf>> = shot_steps
        .iter()
        .map(|st| s.shots_cache.as_ref().map(|d| d.join(format!("{}.png", shot_key(s, st)))))
        .collect();
    let missing: Vec<usize> = (0..plan.len()).filter(|&k| !cached[k].as_ref().is_some_and(|p| p.is_file())).collect();
    if s.report_only && !missing.is_empty() {
        return Err(format!(
            "--report-only: {} of the {} debug images are not in the saved capture (the cameras the report picks changed, or their live capture failed); run without --report-only to capture them, or add --no-render.",
            missing.len(),
            plan.len()
        ));
    }
    let mut status = None;
    if missing.is_empty() {
        eprintln!("geocheck: {} z-fight debug images from the cached capture (no live game)", plan.len());
    } else {
        std::fs::create_dir_all(&raw).map_err(|e| format!("create {}: {e}", raw.display()))?;
        steps.extend(missing.iter().map(|&k| shot_steps[k].clone()));
        if let Some(p) = s.still_param {
            steps.push(still_step(p, false));
        }
        let plan_json = json!({ "tier": s.tier, "steps": steps });
        let plan_path = raw.join("plan.json");
        std::fs::write(&plan_path, serde_json::to_string_pretty(&plan_json).map_err(|e| e.to_string())?)
            .map_err(|e| format!("write plan: {e}"))?;
        let script = s.root.join("tools").join("live-check").join("capture.mjs");
        eprintln!(
            "geocheck: {} z-fight captures in the live game (port {}){}",
            missing.len(),
            s.port,
            if missing.len() < plan.len() { format!("; {} from the cached capture", plan.len() - missing.len()) } else { String::new() }
        );
        status = Some(
            Command::new("node")
                .arg(&script)
                .args(["--game", s.app, "--port", &s.port.to_string(), "--plan"])
                .arg(&plan_path)
                .arg("--out")
                .arg(&raw)
                .current_dir(s.root)
                .status()
                .map_err(|e| format!("node {}: {e}", script.display()))?,
        );
        // Keep what came back for the next run (a failed capture is not kept).
        for &k in &missing {
            let got = raw.join(&plan[k].0);
            if let Some(c) = &cached[k]
                && got.is_file()
            {
                let saved = c.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::copy(&got, c).map(|_| ()));
                if let Err(e) = saved {
                    eprintln!("geocheck: warning: could not cache {}: {e}", got.display());
                }
            }
        }
    }
    let mut made = 0;
    for (k, (file, cam, outlined)) in plan.iter().enumerate() {
        let path: PathBuf = match &cached[k] {
            Some(c) if c.is_file() => c.clone(),
            _ => raw.join(file),
        };
        let Some(mut img) = std::fs::read(&path).ok().and_then(|b| png::decode(&b).ok()) else {
            continue;
        };
        // The capture comes back at the render size; the camera's aspect is
        // the recorded one.
        let mut c = *cam;
        c.aspect = f64::from(img.w) / f64::from(img.h.max(1));
        if file == "overview.png" {
            outline_groups(&mut img, &c, issues, outlined, &group);
            let mut lines: Vec<(usize, String)> = Vec::new();
            for g in 0..GROUP_COLOURS.len() {
                if let Some(&n) = outlined.iter().find(|&&n| group[n] == g) {
                    lines.push((g, key_line(g, &issues[n])));
                }
            }
            colour_key(&mut img, &lines);
        } else {
            for &i in outlined {
                outline(&mut img, &c, &issues[i]);
            }
        }
        std::fs::write(dir.join(file), png::encode(&img)).map_err(|e| format!("write {file}: {e}"))?;
        made += 1;
    }
    if made < plan.len() {
        let run = status.map_or_else(|| "none, all from the cache".to_string(), |st| st.to_string());
        notes.push(format!("{} of {} debug images were captured (capture run: {run}).", made, plan.len()));
        let prefix = format!("{}/", s.dir_name);
        for (n, f) in shots.iter_mut().enumerate() {
            if f.as_ref().is_some_and(|f| !dir.join(f.trim_start_matches(prefix.as_str())).exists()) {
                *f = None;
                notes.push(format!("Patch {}: its capture failed.", n + 1));
            }
        }
    }
    Ok(notes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_geocheck::report::Check;

    fn lit(img: &Image, x0: u32, y0: u32, x1: u32, y1: u32, c: [u8; 4]) -> usize {
        (y0..y1).flat_map(|y| (x0..x1).map(move |x| (x, y))).filter(|&(x, y)| img.px(x, y) == c).count()
    }

    #[test]
    fn a_box_off_the_edge_still_closes_inside_the_picture() {
        let mut img = Image::new(100, 50);
        draw_box(&mut img, [60.0, 10.0, 180.0, 30.0], MAGENTA);
        // Its right side runs along the last column.
        assert!(lit(&img, 99, 10, 100, 31, MAGENTA) >= 20);
        // A box wholly outside draws nothing.
        let mut none = Image::new(100, 50);
        draw_box(&mut none, [120.0, 10.0, 180.0, 30.0], MAGENTA);
        assert_eq!(lit(&none, 0, 0, 100, 50, MAGENTA), 0);
    }

    #[test]
    fn the_colour_key_names_each_group() {
        let mut img = Image::new(640, 360);
        let mut i = Issue::new(Check::ZFight, "thin slab", [0.0, 0.0, 0.0]);
        i.objects = vec!["road (cube)".into(), "road (cube)".into()];
        assert_eq!(key_line(0, &i), "1 magenta thin slab: road (cube)");
        assert_eq!(cause(&i), "thin slab: road (cube) (its own faces)");
        colour_key(&mut img, &[(0, key_line(0, &i)), (2, "3 yellow".into())]);
        assert!(lit(&img, 6, 6, 22, 16, MAGENTA) == 160, "the swatch");
        assert!(lit(&img, 28, 6, 300, 16, [0, 0, 0, 255]) > 100, "the text");
        assert!(lit(&img, 6, 20, 22, 30, group_colour(2)) == 160, "the second swatch");
        assert_eq!(glyph('~'), [0; 5]);
    }
}
