//! Solid stand-ins for hair the source mesh doesn't carry: the horse's
//! mane and tail are strands for the realistic styles (hair v2), so its
//! tiers have no hair on the body mesh, only the anchors the strand job
//! grows from. The geometry-led styles need a solid shape there (a voxel
//! horse with no mane reads as a mule), so these build one from the tier's
//! own anchors and joints, in file units (metres), all our own numbers:
//!
//! - the **mane**: a fin standing along the crest (`anchor:mane_01` at the
//!   poll to `anchor:mane_05` at the withers), sunk into the neck so it is
//!   one piece with it, highest mid-neck, with a short forelock;
//! - the **tail**: from the dock's root along the tail joints, then falling
//!   to about hock height, fuller in the middle.
//!
//! Each is a closed tube of elliptic sections (`SIDES` round), so the
//! voxeliser fills it and the decimator keeps it closed.

use crate::math::{V3, add, cross, dot, len, lerp, normalize, scale, sub};
use crate::rest::RestMesh;

/// Corners of each tube section. Every section's axes keep `a x b = -t`
/// (t: the path's way), so the side quads face out.
const SIDES: usize = 8;

/// A closed triangle mesh (file units).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Solid {
    pub positions: Vec<V3>,
    pub normals: Vec<V3>,
    pub indices: Vec<u32>,
}

/// One tube section: centre, its two axes (unit) and radii along them.
struct Section {
    c: V3,
    a: V3,
    b: V3,
    ra: f32,
    rb: f32,
}

/// A closed tube through `sections` (at least 2), capped at both ends.
fn tube(sections: &[Section]) -> Solid {
    let mut s = Solid::default();
    for sec in sections {
        for k in 0..SIDES {
            let t = std::f32::consts::TAU * k as f32 / SIDES as f32;
            let (cs, sn) = (t.cos(), t.sin());
            s.positions.push(add(sec.c, add(scale(sec.a, sec.ra * cs), scale(sec.b, sec.rb * sn))));
            // The ellipse's outward normal.
            s.normals.push(normalize(add(scale(sec.a, cs / sec.ra.max(1e-4)), scale(sec.b, sn / sec.rb.max(1e-4))), sec.a));
        }
    }
    let n = SIDES as u32;
    for i in 0..sections.len().saturating_sub(1) as u32 {
        for k in 0..n {
            let (a, b) = (i * n + k, i * n + (k + 1) % n);
            let (c, d) = (a + n, b + n);
            s.indices.extend([a, c, b, b, c, d]);
        }
    }
    // Caps: a centre vertex fanned to the end rings.
    // With a x b = -t the ring runs clockwise seen from ahead: the start
    // cap keeps the ring's order, the end cap reverses it (both outward).
    for (end, flip) in [(0usize, false), (sections.len() - 1, true)] {
        let sec = &sections[end];
        let other = &sections[if end == 0 { 1.min(sections.len() - 1) } else { end - 1 }];
        let out = normalize(sub(sec.c, other.c), sec.a);
        let ci = s.positions.len() as u32;
        s.positions.push(sec.c);
        s.normals.push(out);
        let ring = end as u32 * n;
        for k in 0..n {
            let (a, b) = (ring + k, ring + (k + 1) % n);
            if flip { s.indices.extend([ci, b, a]) } else { s.indices.extend([ci, a, b]) }
        }
    }
    s
}

/// A smooth path through `pts` (Catmull-Rom, ends repeated), `per` samples
/// a span, ending on the last point.
fn spline(pts: &[V3], per: usize) -> Vec<V3> {
    let mut out = Vec::new();
    let n = pts.len();
    if n < 2 {
        return pts.to_vec();
    }
    for i in 0..n - 1 {
        let p0 = pts[i.saturating_sub(1)];
        let (p1, p2) = (pts[i], pts[i + 1]);
        let p3 = pts[(i + 2).min(n - 1)];
        for s in 0..per {
            let t = s as f32 / per as f32;
            let (t2, t3) = (t * t, t * t * t);
            let w = [-0.5 * t3 + t2 - 0.5 * t, 1.5 * t3 - 2.5 * t2 + 1.0, -1.5 * t3 + 2.0 * t2 + 0.5 * t, 0.5 * t3 - 0.5 * t2];
            out.push(add(add(scale(p0, w[0]), scale(p1, w[1])), add(scale(p2, w[2]), scale(p3, w[3]))));
        }
    }
    out.push(pts[n - 1]);
    out
}

/// The tangent of a path at `i`.
fn tangent(path: &[V3], i: usize) -> V3 {
    let a = path[i.saturating_sub(1)];
    let b = path[(i + 1).min(path.len() - 1)];
    normalize(sub(b, a), [0.0, 0.0, -1.0])
}

/// The rest mesh's normal nearest to `p` (the body's outward direction).
fn body_normal(rest: &RestMesh, p: V3) -> V3 {
    let mut best = (f32::MAX, [0.0, 1.0, 0.0]);
    for (q, n) in rest.positions.iter().zip(&rest.normals) {
        let d = sub(*q, p);
        let d2 = dot(d, d);
        if d2 < best.0 {
            best = (d2, *n);
        }
    }
    best.1
}

/// The withers height: the crest's last point above the lowest vertex.
fn ground(rest: &RestMesh) -> f32 {
    rest.positions.iter().map(|p| p[1]).fold(f32::MAX, f32::min)
}

/// The mane: a fin along the crest, from the tier's mane anchors, plus a
/// forelock tuft from `anchor:forelock_root`. `None` without the anchors.
pub fn mane(rest: &RestMesh) -> Option<Solid> {
    let crest: Vec<V3> = (1..=5).map(|i| rest.anchor(&format!("mane_{i:02}"))).collect::<Option<_>>()?;
    let path = spline(&crest, 4);
    let length: f32 = path.windows(2).map(|w| len(sub(w[1], w[0]))).sum();
    // Height and half-thickness in metres, along the crest (0 poll, 1 withers).
    let height = |f: f32| 0.07 + 0.09 * (std::f32::consts::PI * (0.15 + 0.75 * f)).sin();
    let sink = 0.05;
    let mut acc = 0.0;
    let mut sections = Vec::new();
    for i in 0..path.len() {
        if i > 0 {
            acc += len(sub(path[i], path[i - 1]));
        }
        let f = if length > 0.0 { acc / length } else { 0.0 };
        let t = tangent(&path, i);
        let n = body_normal(rest, path[i]);
        // Up: the body's normal, kept square to the crest and leaning a
        // little back toward the tail.
        let up = normalize(sub(n, scale(t, dot(n, t))), [0.0, 1.0, 0.0]);
        let side = normalize(cross(t, up), [1.0, 0.0, 0.0]);
        let h = height(f);
        let c = add(path[i], scale(up, 0.5 * h - sink));
        sections.push(Section { c, a: side, b: up, ra: 0.03 - 0.012 * f, rb: 0.5 * h + sink });
    }
    let mut solid = tube(&sections);
    if let Some(root) = rest.anchor("forelock_root") {
        // A short tuft falling forward over the forehead.
        let first = crest[0];
        let fwd = normalize(sub(root, first), [0.0, 0.0, 1.0]);
        let down = scale(body_normal(rest, root), -1.0);
        let pts = [first, root, add(add(root, scale(fwd, 0.06)), scale(down, 0.02))];
        let path = spline(&pts, 3);
        let secs: Vec<Section> = path
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let t = tangent(&path, i);
                let up = normalize(sub(body_normal(rest, *p), scale(t, dot(body_normal(rest, *p), t))), [0.0, 1.0, 0.0]);
                let side = normalize(cross(t, up), [1.0, 0.0, 0.0]);
                let f = i as f32 / (path.len() - 1).max(1) as f32;
                Section { c: add(*p, scale(up, 0.01)), a: side, b: up, ra: 0.035 - 0.015 * f, rb: 0.04 - 0.015 * f }
            })
            .collect();
        let tuft = tube(&secs);
        let base = solid.positions.len() as u32;
        solid.positions.extend(tuft.positions);
        solid.normals.extend(tuft.normals);
        solid.indices.extend(tuft.indices.iter().map(|i| base + i));
    }
    Some(solid)
}

/// The tail: from the dock's root through the tail joints, then falling to
/// about hock height. `None` without the dock anchor and joints.
pub fn tail(rest: &RestMesh) -> Option<Solid> {
    let root = rest.anchor("tail_dock").or_else(|| rest.joint("tail.001"))?;
    let mut pts = vec![root];
    for j in ["tail.001", "tail.002", "tail.003", "tail.004", "tail.005"] {
        if let Some(p) = rest.joint(j)
            && len(sub(p, *pts.last()?)) > 0.02
        {
            pts.push(p);
        }
    }
    let ground_y = ground(rest);
    let dock_end = *pts.last()?;
    // The hair falls from the dock: back a little, then down to about hock
    // height (a third of the root's height above the ground).
    let drop_to = ground_y + 0.36 * (root[1] - ground_y);
    let back = [0.0, 0.0, -1.0];
    let fall = |f: f32| add(lerp(dock_end, [dock_end[0], drop_to, dock_end[2]], f), scale(back, 0.10 * (std::f32::consts::PI * f.min(1.0)).sin() + 0.05 * f));
    for f in [0.25, 0.5, 0.75, 1.0] {
        pts.push(fall(f));
    }
    let path = spline(&pts, 3);
    let total: f32 = path.windows(2).map(|w| len(sub(w[1], w[0]))).sum();
    let mut acc = 0.0;
    let sections = path
        .iter()
        .enumerate()
        .map(|(i, p)| {
            if i > 0 {
                acc += len(sub(*p, path[i - 1]));
            }
            let f = if total > 0.0 { acc / total } else { 0.0 };
            let t = tangent(&path, i);
            let side = normalize(sub([1.0, 0.0, 0.0], scale(t, t[0])), [1.0, 0.0, 0.0]);
            let other = normalize(cross(side, t), [0.0, 0.0, -1.0]);
            // Fuller hair in the middle, a narrow tip.
            let r = 0.045 + 0.05 * (std::f32::consts::PI * f.powf(0.8)).sin() - 0.015 * f;
            Section { c: *p, a: side, b: other, ra: r, rb: r * 0.85 }
        })
        .collect::<Vec<_>>();
    Some(tube(&sections))
}
