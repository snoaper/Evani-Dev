//! Part generators behind one interface (docs/model_research/04).
//!
//! A generator turns named, unit-bearing parameters into a part in its own
//! frame: a field (never a mesh), analytic anchors, a guaranteed box (from
//! the field), the simplest collision proxy that covers it, its smallest
//! dimension (for per-tier thickening) and its natural parameterisation
//! (for UV charts). Every generator honours `min_feature`: no dimension is
//! made thinner than it, which is how a coarse render tier keeps thin parts.
//!
//! M1 generators: rounded_box, panel, shell, cylinder, tube, sweep, ellipsoid,
//! wheel, lathe, loft, frame, planks, bricks, noise_rock, branching_tree,
//! blended_body. External generator services (OmniPart, TRELLIS.2) plug in
//! behind `external`, which is off in M1 (decision D8).

use crate::rng::Pcg32;
use crate::sdf::Node;
use crate::vec::*;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub enum PVal {
    N(f64),
    V(Vec<f64>),
    W(String),
    P(Vec<V3>),
}

pub type Params = BTreeMap<String, PVal>;

#[derive(Clone, Debug, PartialEq)]
pub struct Anchor {
    pub name: String,
    pub p: V3,
    /// Outward direction (or the anchor's main axis for interior anchors).
    pub n: V3,
    pub on_surface: bool,
}

/// Collision proxy in the part's frame.
#[derive(Clone, Debug, PartialEq)]
pub enum Proxy {
    Box { c: V3, half: V3, q: Quat },
    Cylinder { c: V3, q: Quat, r: f64, half_h: f64 },
    Capsule { a: V3, b: V3, r: f64 },
    Sphere { c: V3, r: f64 },
    Hull { pts: Vec<V3> },
}

/// How the surface maps to 2D (used for UV charts).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chart {
    Faces,
    Lathe,
    Sweep,
    Free,
}

#[derive(Clone, Debug)]
pub struct GenOut {
    pub node: Node,
    pub anchors: Vec<Anchor>,
    pub proxy: Proxy,
    pub min_dim: f64,
    pub chart: Chart,
}

impl GenOut {
    pub fn anchor(&self, name: &str) -> Option<&Anchor> {
        self.anchors.iter().find(|a| a.name == name)
    }
}

fn num(p: &Params, k: &str, d: f64) -> f64 {
    match p.get(k) {
        Some(PVal::N(v)) => *v,
        Some(PVal::V(v)) if v.len() == 1 => v[0],
        _ => d,
    }
}
fn vec3(p: &Params, k: &str, d: V3) -> V3 {
    match p.get(k) {
        Some(PVal::V(v)) if v.len() == 3 => [v[0], v[1], v[2]],
        _ => d,
    }
}
fn word<'a>(p: &'a Params, k: &str, d: &'a str) -> &'a str {
    match p.get(k) {
        Some(PVal::W(w)) => w.as_str(),
        _ => d,
    }
}
fn pts(p: &Params, k: &str) -> Vec<V3> {
    match p.get(k) {
        Some(PVal::P(v)) => v.clone(),
        Some(PVal::V(v)) if v.len() % 3 == 0 => v.chunks(3).map(|c| [c[0], c[1], c[2]]).collect(),
        _ => Vec::new(),
    }
}

fn a(name: &str, p: V3, n: V3, on: bool) -> Anchor {
    Anchor { name: name.to_string(), p, n, on_surface: on }
}

fn box_anchors(h: V3) -> Vec<Anchor> {
    vec![
        a("centre", [0.0; 3], [0.0, 1.0, 0.0], false),
        a("top", [0.0, h[1], 0.0], [0.0, 1.0, 0.0], true),
        a("bottom", [0.0, -h[1], 0.0], [0.0, -1.0, 0.0], true),
        a("front", [0.0, 0.0, h[2]], [0.0, 0.0, 1.0], true),
        a("back", [0.0, 0.0, -h[2]], [0.0, 0.0, -1.0], true),
        a("left", [h[0], 0.0, 0.0], [1.0, 0.0, 0.0], true),
        a("right", [-h[0], 0.0, 0.0], [-1.0, 0.0, 0.0], true),
    ]
}

/// Rotation taking +y to the named axis.
fn axis_rot(axis: &str) -> Quat {
    match axis {
        "x" => quat_axis([0.0, 0.0, 1.0], -core::f64::consts::FRAC_PI_2),
        "z" => quat_axis([1.0, 0.0, 0.0], core::f64::consts::FRAC_PI_2),
        _ => QID,
    }
}

/// The kinds this module builds.
pub const KINDS: [&str; 16] = [
    "rounded_box", "panel", "shell", "cylinder", "tube", "sweep", "ellipsoid", "wheel", "lathe", "loft", "frame", "planks", "bricks", "noise_rock",
    "branching_tree", "blended_body",
];

/// Build a part. `min_feature` is the smallest dimension allowed (metres).
pub fn generate(kind: &str, p: &Params, min_feature: f64, seed: u32) -> Result<GenOut, String> {
    let mf = min_feature.max(0.0);
    let half_min = |v: f64| v.max(mf * 0.5);
    match kind {
        "rounded_box" | "panel" => {
            let s = vec3(p, "size", [1.0, 1.0, 1.0]);
            let h = [half_min(s[0] * 0.5), half_min(s[1] * 0.5), half_min(s[2] * 0.5)];
            let small = h[0].min(h[1]).min(h[2]);
            let r = num(p, "bevel", small * 0.3).clamp(0.0, small * 0.95);
            // taper t: the back (-z) keeps its size, the front (+z) shrinks to
            // (1 - t) in width and height.
            let taper = num(p, "taper", 0.0).clamp(-0.9, 0.9);
            let mut node = Node::RoundBox { half: h, r };
            let mut sc = 1.0;
            if taper != 0.0 {
                // Rotate z onto y, taper along y with the back at y = 0, rotate back.
                let to_y = quat_axis([1.0, 0.0, 0.0], -core::f64::consts::FRAC_PI_2);
                let hz = [h[0], h[2], h[1]];
                let slope = -taper / (2.0 * hz[1]);
                let shifted = Node::RoundBox { half: hz, r }.at(QID, [0.0, hz[1], 0.0]);
                node = Node::Taper { slope, child: shifted.boxed() }.at(QID, [0.0, -hz[1], 0.0]).at(qconj(to_y), [0.0; 3]);
                sc = 1.0 - taper * 0.5;
            }
            let mut anchors = box_anchors([h[0] * sc, h[1] * sc, h[2]]);
            if kind == "panel" {
                anchors.push(a("face", [0.0, 0.0, h[2]], [0.0, 0.0, 1.0], true));
            }
            let grow = if taper < 0.0 { 1.0 - taper } else { 1.0 };
            Ok(GenOut { node, anchors, proxy: Proxy::Box { c: [0.0; 3], half: [h[0] * grow, h[1] * grow, h[2]], q: QID }, min_dim: small * 2.0, chart: Chart::Faces })
        }
        "shell" => {
            // An open-topped hollow box (a crate, a chest base, a planter).
            let s = vec3(p, "size", [1.0, 1.0, 1.0]);
            let h = [half_min(s[0] * 0.5), half_min(s[1] * 0.5), half_min(s[2] * 0.5)];
            let t = num(p, "thickness", 0.05).max(mf);
            let r = num(p, "bevel", t * 0.4).min(t * 0.49);
            let outer = Node::RoundBox { half: h, r };
            let inner = Node::RoundBox { half: [h[0] - t, h[1], h[2] - t], r }.at(QID, [0.0, t, 0.0]);
            let node = Node::Subtract { a: outer.boxed(), b: inner.boxed(), k: 0.0 };
            let mut anchors = box_anchors(h);
            anchors.retain(|x| x.name != "top");
            anchors.push(a("floor", [0.0, -h[1] + t, 0.0], [0.0, 1.0, 0.0], true));
            anchors.push(a("rim", [h[0] - t * 0.5, h[1], 0.0], [0.0, 1.0, 0.0], true));
            Ok(GenOut { node, anchors, proxy: Proxy::Box { c: [0.0; 3], half: h, q: QID }, min_dim: t, chart: Chart::Faces })
        }
        "cylinder" => {
            let r = half_min(num(p, "radius", 0.5));
            let hh = half_min(num(p, "length", 1.0) * 0.5);
            let bevel = num(p, "bevel", r.min(hh) * 0.3).clamp(0.0, r.min(hh) * 0.95);
            let q = axis_rot(word(p, "axis", "y"));
            let node = Node::Cylinder { r, half_h: hh, round: bevel }.at(q, [0.0; 3]);
            let ax = qrot(q, [0.0, 1.0, 0.0]);
            let side = qrot(q, [1.0, 0.0, 0.0]);
            let anchors = vec![
                a("centre", [0.0; 3], ax, false),
                a("top", mul(ax, hh), ax, true),
                a("bottom", mul(ax, -hh), neg(ax), true),
                a("side", mul(side, r), side, true),
            ];
            Ok(GenOut { node, anchors, proxy: Proxy::Cylinder { c: [0.0; 3], q, r, half_h: hh }, min_dim: (2.0 * r).min(2.0 * hh), chart: Chart::Lathe })
        }
        "tube" | "sweep" => {
            let path = pts(p, "points");
            if path.len() < 2 {
                return Err(format!("{kind} needs at least two points"));
            }
            let r0 = half_min(num(p, "radius", 0.05));
            let r1 = half_min(num(p, "radius_end", r0));
            let total: f64 = path.windows(2).map(|w| dist(w[0], w[1])).sum();
            let mut acc = 0.0;
            let mut kids = Vec::new();
            for w in path.windows(2) {
                let l = dist(w[0], w[1]);
                let ra = r0 + (r1 - r0) * (acc / total.max(1e-12));
                acc += l;
                let rb = r0 + (r1 - r0) * (acc / total.max(1e-12));
                kids.push(if (ra - rb).abs() < 1e-12 { Node::Capsule { a: w[0], b: w[1], r: ra } } else { Node::RoundCone { a: w[0], b: w[1], ra, rb } });
            }
            let node = if kids.len() == 1 { kids.remove(0) } else { Node::Union(kids) };
            let first = path[0];
            let last = path[path.len() - 1];
            let d0 = norm_or(sub(path[1], first), [0.0, 1.0, 0.0]);
            let d1 = norm_or(sub(last, path[path.len() - 2]), [0.0, 1.0, 0.0]);
            let mid = path[path.len() / 2];
            let anchors = vec![
                a("start", sub(first, mul(d0, r0)), neg(d0), true),
                a("end", add(last, mul(d1, r1)), d1, true),
                a("start_centre", first, d0, false),
                a("end_centre", last, d1, false),
                a("mid", mid, d0, false),
            ];
            let proxy = if path.len() == 2 { Proxy::Capsule { a: first, b: last, r: r0.max(r1) } } else { Proxy::Hull { pts: hull_points(&path, r0.max(r1)) } };
            Ok(GenOut { node, anchors, proxy, min_dim: 2.0 * r0.min(r1), chart: Chart::Sweep })
        }
        "ellipsoid" => {
            let s = vec3(p, "size", [1.0, 1.0, 1.0]);
            let h = [half_min(s[0] * 0.5), half_min(s[1] * 0.5), half_min(s[2] * 0.5)];
            let node = Node::Ellipsoid { radii: h };
            let small = h[0].min(h[1]).min(h[2]);
            let big = h[0].max(h[1]).max(h[2]);
            let proxy = if big - small < 0.1 * big { Proxy::Sphere { c: [0.0; 3], r: big } } else { Proxy::Box { c: [0.0; 3], half: h, q: QID } };
            Ok(GenOut { node, anchors: box_anchors(h), proxy, min_dim: 2.0 * small, chart: Chart::Free })
        }
        "wheel" => {
            // A tyre around x, a rim proud of both faces and a hub bar.
            let r = half_min(num(p, "radius", 0.35));
            let w = (num(p, "width", 0.3)).max(mf);
            let rim = num(p, "rim", 0.6).clamp(0.2, 0.95);
            let round = num(p, "bevel", (w * 0.3).min(r * 0.3)).clamp(0.0, (w * 0.49).min(r * 0.49));
            let qx = axis_rot("x");
            let tyre = Node::Cylinder { r, half_h: w * 0.5, round }.at(qx, [0.0; 3]);
            let rim_r = r * rim;
            let rim_n = Node::Cylinder { r: rim_r, half_h: w * 0.5 + 0.02, round: round.min(rim_r * 0.3) }.at(qx, [0.0; 3]);
            let mut kids = vec![tyre, rim_n];
            let bar = num(p, "hub_bar", 1.0) > 0.5;
            let mut half_x = w * 0.5 + 0.02;
            if bar {
                let bh = [w * 0.5 + 0.04, (r * 0.625).max(mf * 0.5), (r * 0.12).max(mf * 0.5)];
                half_x = bh[0];
                kids.push(Node::RoundBox { half: bh, r: bh[2] * 0.4 });
            }
            let node = Node::Union(kids);
            let anchors = vec![
                a("hub", [0.0; 3], [1.0, 0.0, 0.0], false),
                a("contact", [0.0, -r, 0.0], [0.0, -1.0, 0.0], true),
                a("top", [0.0, r, 0.0], [0.0, 1.0, 0.0], true),
                a("outer", [half_x, 0.0, 0.0], [1.0, 0.0, 0.0], true),
                a("inner", [-half_x, 0.0, 0.0], [-1.0, 0.0, 0.0], true),
            ];
            Ok(GenOut { node, anchors, proxy: Proxy::Cylinder { c: [0.0; 3], q: qx, r, half_h: half_x }, min_dim: w.min(2.0 * r), chart: Chart::Lathe })
        }
        "lathe" => {
            // `profile`: points (radius, height) from the axis round to the axis.
            let prof: Vec<[f64; 2]> = match p.get("profile") {
                Some(PVal::V(v)) if v.len() >= 4 && v.len() % 2 == 0 => v.chunks(2).map(|c| [c[0].max(0.0), c[1]]).collect(),
                _ => return Err("lathe needs a profile of (radius, height) pairs".into()),
            };
            let mut closed = prof.clone();
            // Close along the axis.
            if closed[0][0] > 0.0 {
                closed.insert(0, [0.0, prof[0][1]]);
            }
            if let Some(l) = closed.last().copied()
                && l[0] > 0.0
            {
                closed.push([0.0, l[1]]);
            }
            let rmax = closed.iter().map(|q| q[0]).fold(0.0, f64::max);
            let ymin = closed.iter().map(|q| q[1]).fold(f64::INFINITY, f64::min);
            let ymax = closed.iter().map(|q| q[1]).fold(f64::NEG_INFINITY, f64::max);
            let node = Node::Lathe { profile: closed };
            let anchors = vec![
                a("centre", [0.0, (ymin + ymax) * 0.5, 0.0], [0.0, 1.0, 0.0], false),
                a("top", [0.0, ymax, 0.0], [0.0, 1.0, 0.0], true),
                a("bottom", [0.0, ymin, 0.0], [0.0, -1.0, 0.0], true),
            ];
            let hh = (ymax - ymin) * 0.5;
            Ok(GenOut { node, anchors, proxy: Proxy::Cylinder { c: [0.0, (ymin + ymax) * 0.5, 0.0], q: QID, r: rmax, half_h: hh }, min_dim: (2.0 * rmax).min(2.0 * hh), chart: Chart::Lathe })
        }
        "loft" => {
            // From a bottom rectangle `size0` [x, z] to a top `size1` over `height`.
            let s0 = match p.get("size0") {
                Some(PVal::V(v)) if v.len() == 2 => [v[0], v[1]],
                _ => [1.0, 1.0],
            };
            let s1 = match p.get("size1") {
                Some(PVal::V(v)) if v.len() == 2 => [v[0], v[1]],
                _ => [0.6, 0.6],
            };
            let hgt = num(p, "height", 1.0).max(mf);
            let r = num(p, "bevel", 0.0).max(0.0);
            let (hb, ht) = ([half_min(s0[0] * 0.5) - r, half_min(s0[1] * 0.5) - r], [half_min(s1[0] * 0.5) - r, half_min(s1[1] * 0.5) - r]);
            let hh = hgt * 0.5 - r;
            let mut planes = vec![
                Node::Plane { n: [0.0, 1.0, 0.0], off: hh },
                Node::Plane { n: [0.0, -1.0, 0.0], off: hh },
            ];
            for (ax, sign) in [(0usize, 1.0f64), (0, -1.0), (2, 1.0), (2, -1.0)] {
                let i = if ax == 0 { 0 } else { 1 };
                // Plane through (hb at y = -hh) and (ht at y = +hh).
                let (b0, b1) = (hb[i], ht[i]);
                let mut nrm = [0.0, (b0 - b1), 0.0];
                nrm[ax] = 2.0 * hh * sign;
                let nn = norm_or(nrm, [0.0, 1.0, 0.0]);
                let mut pt = [0.0, -hh, 0.0];
                pt[ax] = b0 * sign;
                planes.push(Node::Plane { n: nn, off: dot(nn, pt) });
            }
            // A bounding box keeps the intersection's bounds finite (it never cuts the shape).
            planes.push(Node::RoundBox { half: [hb[0].max(ht[0]), hh, hb[1].max(ht[1])], r: 0.0 });
            let node = Node::Round { r, child: Node::Intersect(planes).boxed() };
            let hx = hb[0].max(ht[0]) + r;
            let hz = hb[1].max(ht[1]) + r;
            let mut anchors = vec![
                a("centre", [0.0; 3], [0.0, 1.0, 0.0], false),
                a("top", [0.0, hgt * 0.5, 0.0], [0.0, 1.0, 0.0], true),
                a("bottom", [0.0, -hgt * 0.5, 0.0], [0.0, -1.0, 0.0], true),
            ];
            anchors.push(a("front", [0.0, 0.0, (hb[1] + ht[1]) * 0.5 + r], [0.0, 0.0, 1.0], false));
            Ok(GenOut { node, anchors, proxy: Proxy::Box { c: [0.0; 3], half: [hx, hgt * 0.5, hz], q: QID }, min_dim: (2.0 * hx).min(hgt).min(2.0 * hz), chart: Chart::Faces })
        }
        "frame" => {
            // Rectangular tubes round a rectangle `size` [x, y] in the xy plane.
            let s = match p.get("size") {
                Some(PVal::V(v)) if v.len() >= 2 => [v[0], v[1]],
                _ => [1.0, 2.0],
            };
            let t = num(p, "thickness", 0.1).max(mf);
            let d = num(p, "depth", t).max(mf);
            let r = num(p, "bevel", t * 0.2);
            let (hx, hy) = (s[0] * 0.5, s[1] * 0.5);
            let bars = vec![
                Node::RoundBox { half: [hx, t * 0.5, d * 0.5], r }.at(QID, [0.0, hy - t * 0.5, 0.0]),
                Node::RoundBox { half: [hx, t * 0.5, d * 0.5], r }.at(QID, [0.0, -hy + t * 0.5, 0.0]),
                Node::RoundBox { half: [t * 0.5, hy, d * 0.5], r }.at(QID, [hx - t * 0.5, 0.0, 0.0]),
                Node::RoundBox { half: [t * 0.5, hy, d * 0.5], r }.at(QID, [-hx + t * 0.5, 0.0, 0.0]),
            ];
            let anchors = vec![
                a("centre", [0.0; 3], [0.0, 0.0, 1.0], false),
                a("top", [0.0, hy, 0.0], [0.0, 1.0, 0.0], true),
                a("bottom", [0.0, -hy, 0.0], [0.0, -1.0, 0.0], true),
                a("hinge", [hx - t * 0.5, 0.0, d * 0.5], [0.0, 1.0, 0.0], false),
                a("left", [hx, 0.0, 0.0], [1.0, 0.0, 0.0], true),
                a("right", [-hx, 0.0, 0.0], [-1.0, 0.0, 0.0], true),
            ];
            Ok(GenOut { node: Node::Union(bars), anchors, proxy: Proxy::Box { c: [0.0; 3], half: [hx, hy, d * 0.5], q: QID }, min_dim: t.min(d), chart: Chart::Faces })
        }
        "planks" => {
            // `count` boards of `size` [w, t, l] side by side along x, gap `gap`,
            // with a little seeded length and tilt jitter.
            let n = num(p, "count", 4.0).max(1.0) as usize;
            let s = vec3(p, "size", [0.1, 0.03, 1.0]);
            let gap = num(p, "gap", 0.01).max(0.0);
            let jitter = num(p, "jitter", 0.0).max(0.0);
            let h = [half_min(s[0] * 0.5), half_min(s[1] * 0.5), half_min(s[2] * 0.5)];
            let r = num(p, "bevel", h[1] * 0.5).min(h[0].min(h[1]) * 0.95);
            let span = n as f64 * 2.0 * h[0] + (n as f64 - 1.0) * gap;
            let mut rng = Pcg32::new(seed as u64, 11);
            let mut kids = Vec::new();
            for i in 0..n {
                let x = -span * 0.5 + h[0] + i as f64 * (2.0 * h[0] + gap);
                let dl = rng.range(-jitter, jitter) * h[2];
                let tilt = rng.range(-jitter, jitter) * 0.05;
                kids.push(Node::RoundBox { half: [h[0], h[1], h[2] + dl.min(0.0)], r }.at(quat_axis([0.0, 0.0, 1.0], tilt), [x, 0.0, 0.0]));
            }
            let anchors = vec![
                a("centre", [0.0; 3], [0.0, 1.0, 0.0], false),
                a("top", [0.0, h[1], 0.0], [0.0, 1.0, 0.0], false),
                a("left", [span * 0.5, 0.0, 0.0], [1.0, 0.0, 0.0], true),
                a("right", [-span * 0.5, 0.0, 0.0], [-1.0, 0.0, 0.0], true),
            ];
            Ok(GenOut { node: Node::Union(kids), anchors, proxy: Proxy::Box { c: [0.0; 3], half: [span * 0.5, h[1] * 1.2, h[2]], q: QID }, min_dim: 2.0 * h[1], chart: Chart::Faces })
        }
        "bricks" => {
            // A wall `size` [x, y] of bricks `brick` [w, h, d] in running bond.
            let s = match p.get("size") {
                Some(PVal::V(v)) if v.len() >= 2 => [v[0], v[1]],
                _ => [2.0, 1.0],
            };
            let b = vec3(p, "brick", [0.24, 0.08, 0.12]);
            let gap = num(p, "gap", 0.01);
            let r = num(p, "bevel", b[1] * 0.15);
            let rows = ((s[1] + gap) / (b[1] + gap)).floor().max(1.0) as usize;
            let cols = ((s[0] + gap) / (b[0] + gap)).floor().max(1.0) as usize;
            let w = cols as f64 * (b[0] + gap) - gap;
            let hgt = rows as f64 * (b[1] + gap) - gap;
            let core = Node::RoundBox { half: [w * 0.5, hgt * 0.5, (b[2] * 0.5 - 0.01).max(mf * 0.5)], r: 0.0 };
            let mut kids = vec![core];
            for row in 0..rows {
                let y = -hgt * 0.5 + b[1] * 0.5 + row as f64 * (b[1] + gap);
                let off = if row % 2 == 1 { (b[0] + gap) * 0.5 } else { 0.0 };
                for c in 0..cols {
                    let x0 = -w * 0.5 + b[0] * 0.5 + c as f64 * (b[0] + gap) + off;
                    let x1 = x0.min(w * 0.5 - b[0] * 0.5);
                    kids.push(Node::RoundBox { half: [b[0] * 0.5, b[1] * 0.5, b[2] * 0.5], r }.at(QID, [x1, y, 0.0]));
                }
            }
            let anchors = vec![
                a("centre", [0.0; 3], [0.0, 0.0, 1.0], false),
                a("top", [0.0, hgt * 0.5, 0.0], [0.0, 1.0, 0.0], false),
                a("front", [0.0, 0.0, b[2] * 0.5], [0.0, 0.0, 1.0], false),
            ];
            Ok(GenOut { node: Node::Union(kids), anchors, proxy: Proxy::Box { c: [0.0; 3], half: [w * 0.5 + 0.01, hgt * 0.5, b[2] * 0.5], q: QID }, min_dim: b[1].min(b[2]), chart: Chart::Faces })
        }
        "noise_rock" => {
            let s = vec3(p, "size", [1.2, 0.9, 1.6]);
            let lumps = num(p, "lumps", 4.0).clamp(1.0, 12.0) as usize;
            let amp = num(p, "noise", 0.03).max(0.0);
            let freq = num(p, "frequency", 2.5);
            let mut rng = Pcg32::new(seed as u64, 23);
            let h = mul(s, 0.5);
            let mut kids = vec![Node::Ellipsoid { radii: [h[0] * 0.8, h[1] * 0.95, h[2] * 0.8] }.at(QID, [0.0, 0.0, 0.0])];
            for _ in 1..lumps {
                let rr = [h[0] * rng.range(0.35, 0.6), h[1] * rng.range(0.4, 0.7), h[2] * rng.range(0.35, 0.6)];
                let c = [rng.range(-1.0, 1.0) * (h[0] - rr[0]), rng.range(-0.6, 0.2) * (h[1] - rr[1]), rng.range(-1.0, 1.0) * (h[2] - rr[2])];
                kids.push(Node::Ellipsoid { radii: rr }.at(quat_axis([0.0, 1.0, 0.0], rng.range(0.0, 3.0)), c));
            }
            let blob = Node::SmoothUnion { k: h[0].min(h[2]) * 0.3, children: kids };
            let noisy = Node::Noise { amp, freq, seed, child: blob.boxed() };
            let node = Node::Intersect(vec![noisy, Node::Plane { n: [0.0, -1.0, 0.0], off: h[1] * 0.75 }]);
            let anchors = vec![a("centre", [0.0; 3], [0.0, 1.0, 0.0], false), a("bottom", [0.0, -h[1] * 0.75, 0.0], [0.0, -1.0, 0.0], false)];
            let mut hp = Vec::new();
            for sx in [-1.0, 1.0] {
                for sy in [-0.75, 1.0] {
                    for sz in [-1.0, 1.0] {
                        hp.push([sx * (h[0] + amp), sy * (h[1] + amp), sz * (h[2] + amp)]);
                    }
                }
            }
            Ok(GenOut { node, anchors, proxy: Proxy::Hull { pts: hp }, min_dim: 2.0 * h[1].min(h[0]), chart: Chart::Free })
        }
        "branching_tree" => {
            let height = num(p, "height", 2.4).max(mf);
            let r0 = half_min(num(p, "trunk_radius", 0.25));
            let r1 = half_min(num(p, "top_radius", r0 * 0.6));
            let branches = num(p, "branches", 3.0).clamp(0.0, 8.0) as usize;
            let canopy = num(p, "canopy", 2.0).max(0.0);
            let puffs = num(p, "puffs", 5.0).clamp(0.0, 12.0) as usize;
            let mut rng = Pcg32::new(seed as u64, 31);
            let top = [0.0, height, 0.0];
            let mut kids = vec![Node::RoundCone { a: [0.0, 0.0, 0.0], b: top, ra: r0, rb: r1 }];
            for i in 0..branches {
                let ang = (i as f64 / branches.max(1) as f64) * 2.0 * core::f64::consts::PI + rng.range(-0.3, 0.3);
                let (sn, cs) = crate::dmath::sin_cos(ang);
                let len = canopy * 0.3 * rng.range(0.8, 1.2);
                let base = [0.0, height * rng.range(0.75, 0.92), 0.0];
                let tip = add(base, [cs * len, len * 0.8, sn * len]);
                kids.push(Node::RoundCone { a: base, b: tip, ra: (r1 * 0.7).max(mf * 0.5), rb: (r1 * 0.4).max(mf * 0.5) });
            }
            let ccentre = [0.0, height + canopy * 0.35, 0.0];
            for i in 0..puffs {
                let rr = canopy * 0.5 * if i == 0 { 1.0 } else { rng.range(0.6, 0.85) };
                let off = if i == 0 { [0.0; 3] } else { [rng.range(-1.0, 1.0) * canopy * 0.4, rng.range(-0.3, 0.3) * canopy, rng.range(-1.0, 1.0) * canopy * 0.4] };
                kids.push(Node::Sphere { r: rr }.at(QID, add(ccentre, off)));
            }
            let node = Node::Intersect(vec![Node::SmoothUnion { k: r0.min(0.1), children: kids }, Node::Plane { n: [0.0, -1.0, 0.0], off: 0.0 }]);
            let anchors = vec![a("base", [0.0; 3], [0.0, -1.0, 0.0], true), a("crown", ccentre, [0.0, 1.0, 0.0], false), a("trunk_top", top, [0.0, 1.0, 0.0], false)];
            let reach = canopy * 1.0 + 0.1;
            let mut hp = hull_points(&[ccentre], reach);
            hp.extend(hull_points(&[[0.0, r0, 0.0]], r0));
            Ok(GenOut { node, anchors, proxy: Proxy::Hull { pts: hp }, min_dim: 2.0 * r1, chart: Chart::Free })
        }
        "blended_body" => {
            // `blobs`: groups of 7 numbers per ellipsoid (cx, cy, cz, rx, ry, rz, 0)
            // or capsule (ax, ay, az, bx, by, bz, r) marked by a last value > 0.
            let raw = match p.get("blobs") {
                Some(PVal::V(v)) if !v.is_empty() && v.len() % 7 == 0 => v.clone(),
                _ => return Err("blended_body needs blobs: groups of 7 numbers".into()),
            };
            let k = num(p, "blend", 0.04);
            let mut kids = Vec::new();
            let mut proxy_pts = Vec::new();
            for c in raw.chunks(7) {
                if c[6] > 0.0 {
                    let (pa, pb, r) = ([c[0], c[1], c[2]], [c[3], c[4], c[5]], c[6].max(mf * 0.5));
                    kids.push(Node::Capsule { a: pa, b: pb, r });
                    proxy_pts.extend(hull_points(&[pa, pb], r));
                } else {
                    let (cc, rr) = ([c[0], c[1], c[2]], [half_min(c[3]), half_min(c[4]), half_min(c[5])]);
                    kids.push(Node::Ellipsoid { radii: rr }.at(QID, cc));
                    proxy_pts.extend(hull_points(&[cc], rr[0].max(rr[1]).max(rr[2])));
                }
            }
            let node = Node::SmoothUnion { k, children: kids };
            let b = node.bounds();
            let anchors = vec![a("centre", b.centre(), [0.0, 1.0, 0.0], false), a("top", [b.centre()[0], b.max[1], b.centre()[2]], [0.0, 1.0, 0.0], false)];
            Ok(GenOut { node, anchors, proxy: Proxy::Hull { pts: proxy_pts }, min_dim: mf.max(0.02), chart: Chart::Free })
        }
        "external" => Err("external generator services are off in M1 (decision D8)".into()),
        other => Err(format!("unknown generator `{other}`")),
    }
}

/// A few points whose hull covers spheres of radius `r` at `c`.
fn hull_points(cs: &[V3], r: f64) -> Vec<V3> {
    let mut out = Vec::new();
    for c in cs {
        for d in [[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, -1.0]] {
            out.push(add(*c, mul(d, r)));
        }
        let s = r * 0.8;
        for sx in [-s, s] {
            for sy in [-s, s] {
                for sz in [-s, s] {
                    out.push(add(*c, [sx, sy, sz]));
                }
            }
        }
    }
    out
}

/// Does the proxy contain `p` (with tolerance `tol`)?
pub fn proxy_contains(pr: &Proxy, p: V3, tol: f64) -> bool {
    match pr {
        Proxy::Box { c, half, q } => {
            let l = qrot(qconj(*q), sub(p, *c));
            (0..3).all(|i| l[i].abs() <= half[i] + tol)
        }
        Proxy::Cylinder { c, q, r, half_h } => {
            let l = qrot(qconj(*q), sub(p, *c));
            l[1].abs() <= half_h + tol && (l[0] * l[0] + l[2] * l[2]).sqrt() <= r + tol
        }
        Proxy::Capsule { a, b, r } => Node::Capsule { a: *a, b: *b, r: *r }.dist(p) <= tol,
        Proxy::Sphere { c, r } => dist(p, *c) <= r + tol,
        Proxy::Hull { pts } => {
            // Inside the hull's box at least (hull test in collide.rs).
            let mut bb = Aabb::EMPTY;
            for q in pts {
                bb.add_point(*q);
            }
            bb.grow(tol).contains(p)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(kind: &str) -> Params {
        let mut p = Params::new();
        let v = |x: &[f64]| PVal::V(x.to_vec());
        match kind {
            "rounded_box" => {
                p.insert("size".into(), v(&[1.0, 0.4, 0.6]));
                p.insert("taper".into(), PVal::N(0.3));
            }
            "panel" => {
                p.insert("size".into(), v(&[1.0, 0.8, 0.06]));
            }
            "shell" => {
                p.insert("size".into(), v(&[0.8, 0.6, 0.8]));
                p.insert("thickness".into(), PVal::N(0.06));
            }
            "cylinder" => {
                p.insert("radius".into(), PVal::N(0.3));
                p.insert("length".into(), PVal::N(0.5));
                p.insert("axis".into(), PVal::W("z".into()));
            }
            "tube" | "sweep" => {
                p.insert("points".into(), PVal::P(vec![[0.0, 0.0, 0.0], [0.0, 0.8, 0.0], [0.4, 1.1, 0.0]]));
                p.insert("radius".into(), PVal::N(0.06));
                if kind == "sweep" {
                    p.insert("radius_end".into(), PVal::N(0.03));
                }
            }
            "ellipsoid" => {
                p.insert("size".into(), v(&[0.8, 0.5, 0.6]));
            }
            "wheel" => {
                p.insert("radius".into(), PVal::N(0.4));
                p.insert("width".into(), PVal::N(0.3));
            }
            "lathe" => {
                p.insert("profile".into(), v(&[0.39, 0.0, 0.45, 0.6, 0.39, 1.2]));
            }
            "loft" => {
                p.insert("size0".into(), v(&[0.7, 0.7]));
                p.insert("size1".into(), v(&[0.4, 0.4]));
                p.insert("height".into(), PVal::N(0.5));
                p.insert("bevel".into(), PVal::N(0.02));
            }
            "frame" => {
                p.insert("size".into(), v(&[1.0, 2.0]));
                p.insert("thickness".into(), PVal::N(0.1));
            }
            "planks" => {
                p.insert("count".into(), PVal::N(5.0));
                p.insert("size".into(), v(&[0.18, 0.04, 1.6]));
                p.insert("jitter".into(), PVal::N(0.5));
            }
            "bricks" => {
                p.insert("size".into(), v(&[1.2, 0.6]));
            }
            "noise_rock" => {}
            "branching_tree" => {}
            "blended_body" => {
                p.insert("blobs".into(), v(&[0.0, 1.1, 0.0, 0.28, 0.28, 0.28, 0.0, 0.0, 0.6, 0.0, 0.25, 0.25, 0.18, 0.0, 0.2, 0.7, 0.0, 0.4, 0.4, 0.0, 0.07]));
            }
            _ => {}
        }
        p
    }

    #[test]
    fn every_generator_builds_with_surface_anchors_bounds_and_proxy() {
        for kind in KINDS {
            let g = generate(kind, &params(kind), 0.0, 7).unwrap_or_else(|e| panic!("{kind}: {e}"));
            let f = g.node.clone().prepare();
            for an in &g.anchors {
                if an.on_surface {
                    let d = f.dist(an.p);
                    assert!(d.abs() < 1e-4, "{kind}.{} is {d} m off the surface", an.name);
                }
            }
            // Bounds and proxy cover inside points.
            let b = f.bounds();
            let mut r = Pcg32::new(1, 1);
            let big = b.grow(0.2);
            let mut inside = 0;
            for _ in 0..4000 {
                let p = [r.range(big.min[0], big.max[0]), r.range(big.min[1], big.max[1]), r.range(big.min[2], big.max[2])];
                if f.dist(p) < 0.0 {
                    inside += 1;
                    assert!(b.grow(1e-9).contains(p), "{kind}: inside point outside bounds");
                    assert!(proxy_contains(&g.proxy, p, 0.05), "{kind}: inside point {p:?} outside proxy {:?}", g.proxy);
                }
            }
            assert!(inside > 20, "{kind}: almost empty");
        }
    }

    #[test]
    fn min_feature_thickens_thin_parts() {
        let mut p = Params::new();
        p.insert("size".into(), PVal::V(vec![1.0, 0.02, 0.5]));
        let thin = generate("rounded_box", &p, 0.0, 0).unwrap();
        let thick = generate("rounded_box", &p, 0.08, 0).unwrap();
        assert!((thin.min_dim - 0.02).abs() < 1e-12);
        assert!((thick.min_dim - 0.08).abs() < 1e-12);
    }

    #[test]
    fn seeded_generators_repeat_and_vary() {
        let p = params("noise_rock");
        let a = generate("noise_rock", &p, 0.0, 5).unwrap().node.prepare();
        let b = generate("noise_rock", &p, 0.0, 5).unwrap().node.prepare();
        let c = generate("noise_rock", &p, 0.0, 6).unwrap().node.prepare();
        let q = [0.3, 0.1, 0.2];
        assert_eq!(a.eval(q), b.eval(q));
        assert_ne!(a.dist(q), c.dist(q));
        assert!(generate("external", &p, 0.0, 0).is_err());
    }
}
