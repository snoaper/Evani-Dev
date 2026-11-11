//! Shared pieces for vehicles and creatures (batch C, "1 out of 10"):
//! sections lofted into bodies (hulls, car and fish bodies, fuselages,
//! torsos), wing and fin panels, wheels, tapered limbs, mirroring, standing
//! a set of parts on the ground, and two body templates whose proportions
//! are params: a humanoid frame (named joints and its bone list) with its
//! body pieces, and a four-legged frame with its body. Plain maths over
//! [`crate::geo`], [`crate::shapes`] and [`crate::build`]: no tables of
//! scanned data, no third-party code. Ideas from three.js (MIT):
//! TubeGeometry's rings along a path, LatheGeometry's turned profile and
//! ExtrudeGeometry's capped sections; the joint list in the spirit of
//! three.js SkinnedMesh's Bone hierarchy (MIT); written fresh here.

use crate::asset::{Output, Part};
use crate::build as b;
use crate::geo::{self, Mesh, MeshBuilder, normalize};
use crate::shapes::{self as sh, PI, TAU};

fn add(a: [f32; 3], c: [f32; 3]) -> [f32; 3] {
    [a[0] + c[0], a[1] + c[1], a[2] + c[2]]
}

fn sub(a: [f32; 3], c: [f32; 3]) -> [f32; 3] {
    [a[0] - c[0], a[1] - c[1], a[2] - c[2]]
}

fn mul(a: [f32; 3], k: f32) -> [f32; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}

fn dot(a: [f32; 3], c: [f32; 3]) -> f32 {
    a[0] * c[0] + a[1] * c[1] + a[2] * c[2]
}

fn cross(a: [f32; 3], c: [f32; 3]) -> [f32; 3] {
    [a[1] * c[2] - a[2] * c[1], a[2] * c[0] - a[0] * c[2], a[0] * c[1] - a[1] * c[0]]
}

fn dist(a: [f32; 3], c: [f32; 3]) -> f32 {
    let d = sub(a, c);
    dot(d, d).sqrt()
}

/// The point a share `t` of the way from `a` to `c`.
pub fn lerp3(a: [f32; 3], c: [f32; 3], t: f32) -> [f32; 3] {
    add(a, mul(sub(c, a), t))
}

/// An ellipse `rx` across by `ry` up, `n` points counter-clockwise.
pub fn ellipse(rx: f32, ry: f32, n: u32) -> Vec<[f32; 2]> {
    squircle(rx, ry, 2.0, n)
}

/// A superellipse `|x/rx|^p + |y/ry|^p = 1`: `p` 2 an ellipse, 4 and up a
/// rounded box (car and bus bodies, boxy hulls), under 2 a diamond.
pub fn squircle(rx: f32, ry: f32, p: f32, n: u32) -> Vec<[f32; 2]> {
    let e = 2.0 / p.max(0.2);
    (0..n.clamp(3, 256)).map(|i| {
        let a = i as f32 / n.clamp(3, 256) as f32 * TAU;
        let (c, s) = (a.cos(), a.sin());
        [rx * c.signum() * c.abs().powf(e), ry * s.signum() * s.abs().powf(e)]
    }).collect()
}

/// One section of a loft: its `profile` (u, v) placed at `at` along the
/// axes `u` and `v`.
#[derive(Debug, Clone)]
pub struct Station {
    pub at: [f32; 3],
    pub u: [f32; 3],
    pub v: [f32; 3],
    pub profile: Vec<[f32; 2]>,
}

impl Station {
    /// A section across x (u along z, v up) centred at `(x, y, 0)`.
    pub fn x(x: f32, y: f32, profile: Vec<[f32; 2]>) -> Self {
        Self { at: [x, y, 0.0], u: [0.0, 0.0, 1.0], v: [0.0, 1.0, 0.0], profile }
    }

    /// A level section (u along x, v along z) at height `y`.
    pub fn y(x: f32, y: f32, z: f32, profile: Vec<[f32; 2]>) -> Self {
        Self { at: [x, y, z], u: [1.0, 0.0, 0.0], v: [0.0, 0.0, 1.0], profile }
    }

    fn point(&self, p: [f32; 2]) -> [f32; 3] {
        add(self.at, add(mul(self.u, p[0]), mul(self.v, p[1])))
    }
}

/// Flip every triangle of `m` from `first` (an index into `indices`) on
/// whose normal faces away from `outward(centroid)`, as a whole: the
/// piece's own winding, made to face out.
fn face_out(m: &mut Mesh, first: usize, outward: impl Fn(usize, [f32; 3]) -> [f32; 3]) {
    let v = &m.vertices;
    let p = |i: u32| v.get(i as usize).map_or([0.0; 3], |x| [x[0], x[1], x[2]]);
    let mut score = 0.0;
    for (k, t) in m.indices[first..].as_chunks::<3>().0.iter().enumerate() {
        let (a, c, d) = (p(t[0]), p(t[1]), p(t[2]));
        let n = cross(sub(c, a), sub(d, a));
        let centre = mul(add(add(a, c), d), 1.0 / 3.0);
        score += dot(n, outward(k, centre));
    }
    if score < 0.0 {
        for t in m.indices[first..].as_chunks_mut::<3>().0 {
            t.swap(1, 2);
        }
    }
}

/// Loft `stations` (each the same number of points) into a skin; `closed`
/// profiles wrap round (an open one, a U, leaves its top open) and `caps`
/// closes the two ends with flat faces. Triangles face away from the line
/// through the stations' centres; smooth normals on the skin.
pub fn loft(st: &[Station], closed: bool, caps: bool) -> Mesh {
    let k = st.iter().map(|s| s.profile.len()).min().unwrap_or(0);
    if st.len() < 2 || k < 2 || (closed && k < 3) {
        return Mesh::default();
    }
    let mut bl = MeshBuilder::new();
    for s in st {
        for p in s.profile.iter().take(k) {
            bl.vertex(s.point(*p), [0.0, 1.0, 0.0]);
        }
    }
    let ku = k as u32;
    let edges = if closed { ku } else { ku - 1 };
    for j in 0..(st.len() - 1) as u32 {
        for i in 0..edges {
            let a = j * ku + i;
            let c = j * ku + (i + 1) % ku;
            bl.quad(a, c, c + ku, a + ku);
        }
    }
    let mut m = bl.finish();
    let per_row = (edges * 2) as usize;
    let centres: Vec<[f32; 3]> = st.iter().map(|s| s.at).collect();
    face_out(&mut m, 0, |t, c| {
        let j = (t / per_row.max(1)).min(centres.len() - 2);
        sub(c, lerp3(centres[j], centres[j + 1], 0.5))
    });
    geo::smooth_normals(&mut m);
    if caps {
        let last = st.len() - 1;
        for (j, nb) in [(0usize, 1usize), (last, last - 1)] {
            let s = &st[j];
            let pts: Vec<[f32; 2]> = s.profile.iter().take(k).copied().collect();
            let nrm = normalize(sub(s.at, st[nb].at));
            let mut cap = MeshBuilder::new();
            for p in &pts {
                cap.vertex(s.point(*p), nrm);
            }
            for t in b::triangulate(&pts) {
                cap.tri(t[0] as u32, t[1] as u32, t[2] as u32);
            }
            let mut cm = cap.finish();
            sh::fix_winding(&mut cm);
            geo::merge(&mut m, &cm);
        }
    }
    m
}

/// A body along x from `x0` to `x1`: at each share `t` of the way a section
/// `profile(t)` (z across, y up) centred at height `y(t)`, `n` sections,
/// ends capped.
pub fn loft_x(x0: f32, x1: f32, n: u32, y: impl Fn(f32) -> f32, profile: impl Fn(f32) -> Vec<[f32; 2]>) -> Mesh {
    let n = n.max(2);
    let st: Vec<Station> = (0..n).map(|i| {
        let t = i as f32 / (n - 1) as f32;
        Station::x(x0 + (x1 - x0) * t, y(t), profile(t))
    }).collect();
    loft(&st, true, true)
}

/// A round body along x (a fuselage, a torpedo, a fish): radius `r(t)`
/// over the share `t` from `x0` to `x1`, squashed to `squash` of its width
/// in height, at height `y`.
pub fn spindle(x0: f32, x1: f32, y: f32, squash: f32, r: impl Fn(f32) -> f32) -> Mesh {
    loft_x(x0, x1, 24, |_| y, |t| {
        let k = r(t).max(1e-4);
        ellipse(k, k * squash, 20)
    })
}

/// The radius share of a rounded nose-to-tail body at `t`: full in the
/// middle, closing to a round end over `nose` (the +x end) and to `tail`'s
/// share (0 a point) over `back`.
pub fn streamline(t: f32, nose: f32, back: f32, tail: f32) -> f32 {
    if t > 1.0 - nose {
        let u = ((t - (1.0 - nose)) / nose.max(1e-3)).min(1.0);
        (1.0 - u * u).max(0.0).sqrt()
    } else if t < back {
        let u = 1.0 - t / back.max(1e-3);
        tail + (1.0 - tail) * (1.0 - u * u).sqrt()
    } else {
        1.0
    }
}

/// A boat hull `len` along x (bow at +x, keel at y = 0), `beam` wide and
/// `depth` from keel to deck; `p` the section's fullness (1 a V, 2 round,
/// 4 boxy) and `shape(t)` (t 0 at the stern, 1 at the bow) gives
/// [width share, keel lift (m), deck lift (m)]. `open`: the hull's skin
/// only, seen from both sides, with a transom (open boats); otherwise a
/// closed deck.
pub fn hull(len: f32, beam: f32, depth: f32, p: f32, open: bool, shape: impl Fn(f32) -> [f32; 3]) -> Mesh {
    let n = 22;
    let e = 2.0 / p.max(0.3);
    let st: Vec<Station> = (0..n).map(|i| {
        let t = i as f32 / (n - 1) as f32;
        let [w, keel, deck] = shape(t);
        let hw = (beam * 0.5 * w).max(1e-4);
        let d = (depth + deck - keel).max(1e-3);
        // A U from the starboard deck edge round the keel to port.
        let prof: Vec<[f32; 2]> = (0..=16).map(|k| {
            let a = k as f32 / 16.0 * PI;
            let (c, s) = (a.cos(), a.sin());
            [hw * c.signum() * c.abs().powf(e), d * (1.0 - s.abs().powf(e))]
        }).collect();
        Station::x((t - 0.5) * len, keel, prof)
    }).collect();
    if !open {
        return loft(&st, true, true);
    }
    let mut m = loft(&st, false, false);
    // The transom, a flat stern panel.
    let s0 = &st[0];
    let mut cap = MeshBuilder::new();
    for q in &s0.profile {
        cap.vertex(s0.point(*q), [-1.0, 0.0, 0.0]);
    }
    for t in b::triangulate(&s0.profile) {
        cap.tri(t[0] as u32, t[1] as u32, t[2] as u32);
    }
    let mut cm = cap.finish();
    sh::fix_winding(&mut cm);
    geo::merge(&mut m, &cm);
    sh::double_sided(&mut m);
    m
}

/// An airfoil-like lens section `chord` long (its leading edge at u = 0,
/// trailing at -chord) and `thick` (share of the chord) deep.
pub fn airfoil(chord: f32, thick: f32, n: u32) -> Vec<[f32; 2]> {
    (0..n.clamp(6, 64)).map(|i| {
        let a = i as f32 / n.clamp(6, 64) as f32 * TAU;
        let x = chord * 0.5 * (a.cos() - 1.0);
        let y = 0.5 * thick * chord * a.sin() * ((1.0 + a.cos()) * 0.5).sqrt().max(0.12);
        [x, y]
    }).collect()
}

/// A wing, fin or blade panel from `root` out along `dir` for `span`: its
/// chord along x from `root_chord` to `tip_chord` (the leading edge at
/// +x), the tip's leading edge `sweep` (m) further back, `thick` its depth
/// as a share of the chord.
pub fn panel(root: [f32; 3], dir: [f32; 3], span: f32, root_chord: f32, tip_chord: f32, sweep: f32, thick: f32) -> Mesh {
    let d = normalize(dir);
    let v = {
        let c = cross(d, [1.0, 0.0, 0.0]);
        if dot(c, c) < 1e-6 { [0.0, 1.0, 0.0] } else { normalize(c) }
    };
    let n = 6;
    let st: Vec<Station> = (0..n).map(|i| {
        let t = i as f32 / (n - 1) as f32;
        let chord = (root_chord + (tip_chord - root_chord) * t).max(1e-3);
        Station { at: add(add(root, mul(d, span * t)), [-sweep * t, 0.0, 0.0]), u: [1.0, 0.0, 0.0], v, profile: airfoil(chord, thick, 16) }
    }).collect();
    loft(&st, true, true)
}

/// A pair of panels, mirrored across z = 0: `dir` is the right-hand one's
/// (z > 0) heading.
pub fn panels(root: [f32; 3], dir: [f32; 3], span: f32, chords: [f32; 2], sweep: f32, thick: f32) -> Mesh {
    let mut m = panel(root, dir, span, chords[0], chords[1], sweep, thick);
    geo::merge(&mut m, &panel([root[0], root[1], -root[2]], [dir[0], dir[1], -dir[2]], span, chords[0], chords[1], sweep, thick));
    m
}

/// `m` mirrored across z = 0 (still facing out).
pub fn mirror_z(m: &Mesh) -> Mesh {
    let mut c = m.clone();
    for v in &mut c.vertices {
        v[2] = -v[2];
        v[5] = -v[5];
    }
    for t in c.indices.as_chunks_mut::<3>().0 {
        t.swap(1, 2);
    }
    c
}

/// `m` mirrored across x = 0 (still facing out).
pub fn mirror_x(m: &Mesh) -> Mesh {
    let mut c = m.clone();
    for v in &mut c.vertices {
        v[0] = -v[0];
        v[3] = -v[3];
    }
    for t in c.indices.as_chunks_mut::<3>().0 {
        t.swap(1, 2);
    }
    c
}

/// `m` with its mirror across x = 0 (a left and a right).
pub fn both_x(m: Mesh) -> Mesh {
    let mut out = mirror_x(&m);
    geo::merge(&mut out, &m);
    out
}

/// `m` with its mirror across z = 0.
pub fn both_z(m: Mesh) -> Mesh {
    let mut out = mirror_z(&m);
    geo::merge(&mut out, &m);
    out
}

/// A wheel centred at `c`, its axle along z: the tyre (radius `r`, `w`
/// wide) and the rim with `spokes` spokes (0: a plain disc hub).
pub fn wheel(c: [f32; 3], r: f32, w: f32, spokes: u32) -> (Mesh, Mesh) {
    let (hw, ri) = (w * 0.5, r - (r * 0.38).min(w * 0.9));
    let mut prof = vec![[ri, -hw], [r - w * 0.18, -hw]];
    for k in 0..=6 {
        let a = -PI * 0.5 + k as f32 / 6.0 * PI;
        prof.push([r - w * 0.18 + w * 0.18 * a.cos(), hw * 0.85 * a.sin()]);
    }
    prof.extend([[r - w * 0.18, hw], [ri, hw], [ri, -hw]]);
    let mut tyre = b::revolve(&prof, 28);
    sh::rotate(&mut tyre, [1.0, 0.0, 0.0], PI * 0.5);
    geo::transform(&mut tyre, [1.0; 3], c);
    let mut rim = if spokes == 0 {
        b::rod([c[0], c[1], c[2] - hw * 0.8], [c[0], c[1], c[2] + hw * 0.8], ri, 20)
    } else {
        let mut m = b::rod([c[0], c[1], c[2] - hw * 0.5], [c[0], c[1], c[2] + hw * 0.5], ri * 0.22, 12);
        let ring: Vec<[f32; 3]> = (0..=24).map(|k| {
            let a = k as f32 / 24.0 * TAU;
            [c[0] + ri * 0.97 * a.cos(), c[1] + ri * 0.97 * a.sin(), c[2]]
        }).collect();
        geo::merge(&mut m, &b::sweep(&ring, &b::rect(w * 0.12, w * 0.5), true));
        for k in 0..spokes {
            let a = k as f32 / spokes as f32 * TAU;
            let th = (r * 0.012).max(0.003);
            geo::merge(&mut m, &b::rod(c, [c[0] + ri * a.cos(), c[1] + ri * a.sin(), c[2]], th, 6));
        }
        m
    };
    geo::merge(&mut rim, &b::rod([c[0], c[1], c[2] - hw * 0.9], [c[0], c[1], c[2] + hw * 0.9], ri * 0.3, 12));
    (tyre, rim)
}

/// A tapered limb with round ends from `a` (radius `ra`) to `c` (`rc`).
pub fn limb(a: [f32; 3], c: [f32; 3], ra: f32, rc: f32, sides: u32) -> Mesh {
    let l = dist(a, c).max(1e-4);
    let d = mul(sub(c, a), 1.0 / l);
    let (p0, p1) = (sub(a, mul(d, ra)), add(c, mul(d, rc)));
    let total = l + ra + rc;
    let n = 14;
    let path: Vec<[f32; 3]> = (0..n).map(|i| {
        let t = i as f32 / (n - 1) as f32;
        // Crowd the rings into the round ends.
        let u = if t < 0.3 { ra * (t / 0.3) } else if t > 0.7 { total - rc * ((1.0 - t) / 0.3) } else { ra + l * ((t - 0.3) / 0.4) };
        lerp3(p0, p1, u / total)
    }).collect();
    let radius = |t: f32| {
        if t < 0.3 {
            let s = 1.0 - t / 0.3;
            ra * (1.0 - s * s).max(0.0).sqrt()
        } else if t > 0.7 {
            let s = 1.0 - (1.0 - t) / 0.3;
            rc * (1.0 - s * s).max(0.0).sqrt()
        } else {
            ra + (rc - ra) * ((t - 0.3) / 0.4)
        }
    };
    sh::tube(&path, radius, sides)
}

/// A limb through several joints: `pts` with a radius each.
pub fn chain(pts: &[([f32; 3], f32)], sides: u32) -> Mesh {
    let mut m = Mesh::default();
    for w in pts.windows(2) {
        geo::merge(&mut m, &limb(w[0].0, w[1].0, w[0].1, w[1].1, sides));
    }
    m
}

/// An ellipsoid of half sizes `half` round `at`.
pub fn ball(half: [f32; 3], at: [f32; 3]) -> Mesh {
    sh::blob(half, at, 0.0, 1, 2)
}

/// An ellipsoid of half sizes `half` (x along `dir`) round `at`, turned so
/// its x axis points along `dir`.
pub fn ball_along(half: [f32; 3], at: [f32; 3], dir: [f32; 3]) -> Mesh {
    let mut m = ball(half, [0.0; 3]);
    let d = normalize(dir);
    let pitch = d[1].clamp(-1.0, 1.0).asin();
    let yaw = (-d[2]).atan2(d[0]);
    sh::rotate(&mut m, [0.0, 0.0, 1.0], pitch);
    sh::rotate(&mut m, [0.0, 1.0, 0.0], yaw);
    geo::transform(&mut m, [1.0; 3], at);
    m
}

/// A cone from `base` (radius `r`) to `tip`.
pub fn cone(base: [f32; 3], tip: [f32; 3], r: f32, sides: u32) -> Mesh {
    sh::tube(&[base, lerp3(base, tip, 0.5), tip], |t| r * (1.0 - t), sides)
}

/// Every part's mesh scaled by `k` round the origin.
pub fn scale_parts(ps: &mut [Part], k: f32) {
    for p in ps {
        geo::transform(&mut p.mesh, [k; 3], [0.0; 3]);
    }
}

/// The parts moved up or down together so the lowest stands at y = 0.
pub fn stand(mut ps: Vec<Part>) -> Output {
    let lo = ps.iter().filter(|p| !p.mesh.vertices.is_empty()).map(|p| geo::bounds(&p.mesh).0[1]).fold(f32::MAX, f32::min);
    if lo < f32::MAX {
        for p in &mut ps {
            geo::transform(&mut p.mesh, [1.0; 3], [0.0, -lo, 0.0]);
        }
    }
    Output::Parts(ps)
}

/// The humanoid frame's joints, by index ([`HUMAN_JOINTS`] names them).
pub mod joint {
    pub const PELVIS: usize = 0;
    pub const CHEST: usize = 1;
    pub const NECK: usize = 2;
    pub const HEAD: usize = 3;
    pub const CROWN: usize = 4;
    pub const SHOULDER_L: usize = 5;
    pub const ELBOW_L: usize = 6;
    pub const WRIST_L: usize = 7;
    pub const HAND_L: usize = 8;
    pub const SHOULDER_R: usize = 9;
    pub const ELBOW_R: usize = 10;
    pub const WRIST_R: usize = 11;
    pub const HAND_R: usize = 12;
    pub const HIP_L: usize = 13;
    pub const KNEE_L: usize = 14;
    pub const ANKLE_L: usize = 15;
    pub const TOE_L: usize = 16;
    pub const HIP_R: usize = 17;
    pub const KNEE_R: usize = 18;
    pub const ANKLE_R: usize = 19;
    pub const TOE_R: usize = 20;
}

/// The joints' names, in [`joint`] order.
pub const HUMAN_JOINTS: [&str; 21] = [
    "pelvis", "chest", "neck", "head", "crown", "shoulder.l", "elbow.l", "wrist.l", "hand.l", "shoulder.r", "elbow.r", "wrist.r", "hand.r",
    "hip.l", "knee.l", "ankle.l", "toe.l", "hip.r", "knee.r", "ankle.r", "toe.r",
];

/// The humanoid bone list: (name, parent bone or none, head joint, tail
/// joint). Bone 0, the spine, is the root.
pub const HUMAN_BONES: [(&str, Option<usize>, usize, usize); 20] = {
    use joint::*;
    [
        ("spine", None, PELVIS, CHEST),
        ("neck", Some(0), CHEST, NECK),
        ("head", Some(1), NECK, CROWN),
        ("clavicle.l", Some(0), CHEST, SHOULDER_L),
        ("upper_arm.l", Some(3), SHOULDER_L, ELBOW_L),
        ("forearm.l", Some(4), ELBOW_L, WRIST_L),
        ("hand.l", Some(5), WRIST_L, HAND_L),
        ("clavicle.r", Some(0), CHEST, SHOULDER_R),
        ("upper_arm.r", Some(7), SHOULDER_R, ELBOW_R),
        ("forearm.r", Some(8), ELBOW_R, WRIST_R),
        ("hand.r", Some(9), WRIST_R, HAND_R),
        ("pelvis.l", Some(0), PELVIS, HIP_L),
        ("thigh.l", Some(11), HIP_L, KNEE_L),
        ("shin.l", Some(12), KNEE_L, ANKLE_L),
        ("foot.l", Some(13), ANKLE_L, TOE_L),
        ("pelvis.r", Some(0), PELVIS, HIP_R),
        ("thigh.r", Some(15), HIP_R, KNEE_R),
        ("shin.r", Some(16), KNEE_R, ANKLE_R),
        ("foot.r", Some(17), ANKLE_R, TOE_R),
        ("jaw", Some(2), HEAD, NECK),
    ]
};

/// A humanoid's proportions: `height` (m), `bulk` (1 average, 0.7 slight,
/// 1.4 heavy), `legs` (hip height as a share of the height, ~0.5), `arms`
/// (each arm's angle out from hanging, radians) and `knees` (bend,
/// radians). It faces +z; its left is +x.
#[derive(Debug, Clone, Copy)]
pub struct Human {
    pub height: f32,
    pub bulk: f32,
    pub legs: f32,
    pub arms: f32,
    pub knees: f32,
}

impl Default for Human {
    fn default() -> Self {
        Self { height: 1.75, bulk: 1.0, legs: 0.5, arms: 0.35, knees: 0.0 }
    }
}

impl Human {
    /// The joints ([`joint`] order) standing on y = 0.
    pub fn frame(&self) -> [[f32; 3]; 21] {
        use joint::*;
        let h = self.height;
        let k = self.bulk.sqrt();
        let hip_y = h * self.legs.clamp(0.35, 0.62);
        let mut j = [[0.0f32; 3]; 21];
        j[PELVIS] = [0.0, hip_y + h * 0.035, 0.0];
        j[CHEST] = [0.0, h * 0.74, 0.0];
        j[NECK] = [0.0, h * 0.85, 0.0];
        j[HEAD] = [0.0, h * 0.92, h * 0.01];
        j[CROWN] = [0.0, h, 0.0];
        let (upper, fore, hand) = (h * 0.17, h * 0.15, h * 0.1);
        for (sh_i, el, wr, ha, side) in [(SHOULDER_L, ELBOW_L, WRIST_L, HAND_L, 1.0f32), (SHOULDER_R, ELBOW_R, WRIST_R, HAND_R, -1.0)] {
            let s = [side * h * 0.12 * k, h * 0.82, 0.0];
            let d = [side * self.arms.sin(), -self.arms.cos(), 0.0];
            let e = add(s, mul(d, upper));
            // The forearm bends a little forward.
            let d2 = normalize(add(d, [0.0, 0.0, 0.25]));
            let w = add(e, mul(d2, fore));
            j[sh_i] = s;
            j[el] = e;
            j[wr] = w;
            j[ha] = add(w, mul(d2, hand));
        }
        // Bent knees go forward and the body sinks, the ankles kept down.
        let ankle_y = h * 0.045;
        let seg = (hip_y - ankle_y) * 0.5;
        let half = self.knees.clamp(0.0, 1.4) * 0.5;
        let sink = 2.0 * seg * (1.0 - half.cos());
        for p in j.iter_mut().take(HAND_R + 1) {
            p[1] -= sink;
        }
        for (hp, kn, an, to, side) in [(HIP_L, KNEE_L, ANKLE_L, TOE_L, 1.0f32), (HIP_R, KNEE_R, ANKLE_R, TOE_R, -1.0)] {
            let hip = [side * h * 0.085 * k, hip_y - sink, 0.0];
            j[hp] = hip;
            j[kn] = [hip[0], hip[1] - seg * half.cos(), seg * half.sin()];
            j[an] = [hip[0], ankle_y, 0.0];
            j[to] = [hip[0], h * 0.015, h * 0.13];
        }
        j
    }
}

/// A humanoid's body pieces round its frame `j`, every radius grown by
/// `grow` (m): 0 the skin, a little more a garment over it.
#[derive(Debug, Clone, Default)]
pub struct Body {
    pub head: Mesh,
    pub neck: Mesh,
    pub torso: Mesh,
    pub hips: Mesh,
    pub arms: Mesh,
    pub hands: Mesh,
    pub legs: Mesh,
    pub feet: Mesh,
}

/// The torso as level sections from `y0` to `y1` (shares of the height,
/// 0 the crotch .. 1 the shoulders), grown by `grow`.
pub fn torso(h: &Human, from: f32, to: f32, grow: f32) -> Mesh {
    use joint::*;
    let j = h.frame();
    let k = h.bulk;
    let (y0, y1) = (j[PELVIS][1] - h.height * 0.06, j[SHOULDER_L][1] + h.height * 0.02);
    let n = 10;
    // (share up, half width, half depth) of the trunk.
    let w = |t: f32| {
        let hips = 0.105;
        let waist = 0.088;
        let chest = 0.115;
        let x = if t < 0.4 { hips + (waist - hips) * (t / 0.4) } else if t < 0.8 { waist + (chest - waist) * ((t - 0.4) / 0.4) } else { chest - (chest - 0.07) * ((t - 0.8) / 0.2).powi(2) };
        x * h.height * k.powf(0.7)
    };
    let d = |t: f32| (0.075 + 0.01 * (t * PI).sin()) * h.height * k.powf(0.8) * (1.0 - 0.35 * ((t - 0.8) / 0.2).max(0.0).powi(2));
    let st: Vec<Station> = (0..n).map(|i| {
        let t = from + (to - from) * i as f32 / (n - 1) as f32;
        Station::y(0.0, y0 + (y1 - y0) * t, 0.0, squircle(w(t) + grow, d(t) + grow, 2.6, 24))
    }).collect();
    loft(&st, true, true)
}

/// One arm (`side` 1 left, -1 right) from the shoulder a share `cover` of
/// the way to the wrist (1 the whole arm), grown by `grow`.
pub fn arm(h: &Human, side: f32, cover: f32, grow: f32) -> Mesh {
    use joint::*;
    let j = h.frame();
    let (s, e, w) = if side > 0.0 { (j[SHOULDER_L], j[ELBOW_L], j[WRIST_L]) } else { (j[SHOULDER_R], j[ELBOW_R], j[WRIST_R]) };
    let r = h.height * h.bulk.powf(0.6);
    let (rs, re, rw) = (r * 0.036 + grow, r * 0.029 + grow, r * 0.022 + grow);
    let c = cover.clamp(0.02, 1.0) * 2.0;
    if c <= 1.0 {
        limb(s, lerp3(s, e, c), rs, rs + (re - rs) * c, 12)
    } else {
        chain(&[(s, rs), (e, re), (lerp3(e, w, c - 1.0), re + (rw - re) * (c - 1.0))], 12)
    }
}

/// One leg from the hip a share `cover` of the way to the ankle.
pub fn leg(h: &Human, side: f32, cover: f32, grow: f32) -> Mesh {
    use joint::*;
    let j = h.frame();
    let (hp, kn, an) = if side > 0.0 { (j[HIP_L], j[KNEE_L], j[ANKLE_L]) } else { (j[HIP_R], j[KNEE_R], j[ANKLE_R]) };
    let r = h.height * h.bulk.powf(0.7);
    let (rh, rk, ra) = (r * 0.058 + grow, r * 0.036 + grow, r * 0.024 + grow);
    let c = cover.clamp(0.02, 1.0) * 2.0;
    if c <= 1.0 {
        limb(hp, lerp3(hp, kn, c), rh, rh + (rk - rh) * c, 14)
    } else {
        // The calf swells a little below the knee.
        let calf = lerp3(kn, an, 0.3);
        let mut pts = vec![(hp, rh), (kn, rk)];
        if c - 1.0 > 0.3 {
            pts.push((calf, rk * 1.08));
        }
        pts.push((lerp3(kn, an, c - 1.0), rk + (ra - rk) * (c - 1.0)));
        chain(&pts, 14)
    }
}

/// A foot (`side`) from the ankle to the toes, grown by `grow`.
pub fn foot(h: &Human, side: f32, grow: f32) -> Mesh {
    use joint::*;
    let j = h.frame();
    let (an, to) = if side > 0.0 { (j[ANKLE_L], j[TOE_L]) } else { (j[ANKLE_R], j[TOE_R]) };
    let len = h.height * 0.15 + grow * 2.0;
    let mid = [an[0], h.height * 0.028 + grow * 0.5, (an[2] + to[2]) * 0.5 - h.height * 0.02];
    let mut m = ball([h.height * 0.032 + grow, h.height * 0.03 + grow, len * 0.5], mid);
    geo::merge(&mut m, &ball([h.height * 0.026 + grow; 3], [an[0], an[1], an[2] - h.height * 0.005]));
    m
}

/// A hand (`side`): a mitten-like palm and a thumb at the wrist.
pub fn hand(h: &Human, side: f32, grow: f32) -> Mesh {
    use joint::*;
    let j = h.frame();
    let (w, tip) = if side > 0.0 { (j[WRIST_L], j[HAND_L]) } else { (j[WRIST_R], j[HAND_R]) };
    let d = normalize(sub(tip, w));
    let mut m = ball_along([h.height * 0.05 + grow, h.height * 0.012 + grow, h.height * 0.028 + grow], lerp3(w, tip, 0.45), d);
    geo::merge(&mut m, &limb(lerp3(w, tip, 0.15), add(lerp3(w, tip, 0.45), [0.0, 0.0, h.height * 0.035]), h.height * 0.011 + grow, h.height * 0.009 + grow, 8));
    m
}

/// The head: a skull ball and a jaw.
pub fn head(h: &Human, grow: f32) -> Mesh {
    use joint::*;
    let j = h.frame();
    let hs = h.height * 0.062;
    let c = [0.0, j[HEAD][1] + hs * 0.15, j[HEAD][2]];
    let mut m = ball([hs * 0.82 + grow, hs + grow, hs * 0.95 + grow], c);
    geo::merge(&mut m, &ball([hs * 0.62 + grow, hs * 0.55 + grow, hs * 0.7 + grow], [0.0, c[1] - hs * 0.55, c[2] + hs * 0.15]));
    m
}

/// Every piece of the body round `h`, grown by `grow`.
pub fn body(h: &Human, grow: f32) -> Body {
    use joint::*;
    let j = h.frame();
    let mut out = Body { head: head(h, grow), torso: torso(h, 0.25, 1.0, grow), hips: torso(h, 0.0, 0.3, grow), ..Body::default() };
    out.neck = limb(j[CHEST], [0.0, j[HEAD][1] - h.height * 0.03, 0.0], h.height * 0.034 + grow, h.height * 0.03 + grow, 12);
    for side in [1.0f32, -1.0] {
        geo::merge(&mut out.arms, &arm(h, side, 1.0, grow));
        geo::merge(&mut out.hands, &hand(h, side, grow));
        geo::merge(&mut out.legs, &leg(h, side, 1.0, grow));
        geo::merge(&mut out.feet, &foot(h, side, grow));
    }
    out
}

/// All of a body's pieces in one mesh.
pub fn body_mesh(b: &Body) -> Mesh {
    let mut m = Mesh::default();
    for p in [&b.head, &b.neck, &b.torso, &b.hips, &b.arms, &b.hands, &b.legs, &b.feet] {
        geo::merge(&mut m, p);
    }
    m
}

/// A four-legged animal's proportions (m): its body `len` and `girth`
/// (the barrel's radius), `leg` length, `neck` length and its `neck_up`
/// angle (radians above level), `head` length and `snout` (a share of the
/// head), `tail` length, `ears` length and `leg_r` (leg radius). It faces
/// +x.
#[derive(Debug, Clone, Copy)]
pub struct Quad {
    pub len: f32,
    pub girth: f32,
    pub leg: f32,
    pub leg_r: f32,
    pub neck: f32,
    pub neck_up: f32,
    pub head: f32,
    pub snout: f32,
    pub tail: f32,
    pub tail_up: f32,
    pub ears: f32,
}

/// The parts of a four-legged body and the points other parts hang on.
#[derive(Debug, Clone, Default)]
pub struct QuadBody {
    /// Barrel, neck, head, ears, legs and tail.
    pub skin: Mesh,
    /// Hooves or paws.
    pub feet: Mesh,
    pub eyes: Mesh,
    /// The nose's tip.
    pub nose: Mesh,
    /// The top of the head (horns, antlers), between the ears.
    pub crown: [f32; 3],
    /// The head's centre and the way it faces.
    pub head_at: [f32; 3],
    pub head_dir: [f32; 3],
    /// The top of the neck's base and of the rump (manes, humps, wings).
    pub withers: [f32; 3],
    pub rump: [f32; 3],
    /// The four feet (front left, front right, back left, back right).
    pub paws: [[f32; 3]; 4],
    /// The neck's base and its top (for a mane along it).
    pub neck_line: [[f32; 3]; 2],
}

/// Build a four-legged body standing on y = 0.
pub fn quadruped(q: &Quad) -> QuadBody {
    let (l, g) = (q.len, q.girth);
    let cy = q.leg + g * 0.75;
    let mut skin = loft_x(-l * 0.5, l * 0.5, 14, |t| cy + g * 0.08 * (t * PI).sin(), |t| {
        let u = (2.0 * t - 1.0).abs();
        let k = (1.0 - u.powi(3)).max(0.0).cbrt().max(0.05);
        ellipse(g * 0.82 * k, g * k, 18)
    });
    let mut feet = Mesh::default();
    let mut paws = [[0.0f32; 3]; 4];
    let legs = [(l * 0.5 - g * 0.55, 1.0f32, true), (l * 0.5 - g * 0.55, -1.0, true), (-l * 0.5 + g * 0.6, 1.0, false), (-l * 0.5 + g * 0.6, -1.0, false)];
    for (i, (x, side, front)) in legs.into_iter().enumerate() {
        let z = side * g * 0.45;
        let top = [x, cy - g * 0.2, z];
        let foot = [x + if front { 0.0 } else { -g * 0.05 }, q.leg_r * 0.9, z];
        // Front knees bend back, hocks forward-high.
        let mid = if front { [x - q.leg_r * 0.3, q.leg * 0.45, z] } else { [x - q.leg_r * 1.8, q.leg * 0.5, z] };
        let upper = if front { g * 0.38 } else { g * 0.48 };
        geo::merge(&mut skin, &chain(&[(top, upper.max(q.leg_r * 1.3)), (mid, q.leg_r * 1.05), (foot, q.leg_r * 0.85)], 10));
        let mut hoof = b::revolve(&[[0.0, 0.0], [q.leg_r * 1.05, 0.0], [q.leg_r * 0.95, q.leg_r * 1.1], [0.0, q.leg_r * 1.1]], 12);
        geo::transform(&mut hoof, [1.0; 3], [foot[0], 0.0, foot[2]]);
        geo::merge(&mut feet, &hoof);
        paws[i] = [foot[0], 0.0, foot[2]];
    }
    // The neck rises from the chest; the head at its end.
    let base = [l * 0.5 - g * 0.5, cy + g * 0.35, 0.0];
    let nd = [q.neck_up.cos(), q.neck_up.sin(), 0.0];
    let top = add(base, mul(nd, q.neck.max(0.01)));
    geo::merge(&mut skin, &limb(base, top, g * 0.55, g * 0.32 + q.head * 0.12, 12));
    let hd = normalize([1.0, -0.35 - q.neck_up * 0.3, 0.0]);
    let hc = add(top, mul(hd, q.head * 0.3));
    geo::merge(&mut skin, &ball_along([q.head * 0.38, q.head * 0.3, q.head * 0.28], hc, hd));
    let snout_end = add(hc, mul(hd, q.head * (0.25 + q.snout * 0.55)));
    geo::merge(&mut skin, &limb(hc, snout_end, q.head * 0.22, q.head * 0.15, 12));
    let nose = ball([q.head * 0.09; 3], add(snout_end, mul(hd, q.head * 0.1)));
    let up = normalize(cross([0.0, 0.0, 1.0], hd));
    let crown = add(hc, mul(up, q.head * 0.26));
    let mut eyes = Mesh::default();
    for side in [1.0f32, -1.0] {
        let e = add(add(hc, mul(hd, q.head * 0.12)), add(mul(up, q.head * 0.12), [0.0, 0.0, side * q.head * 0.24]));
        geo::merge(&mut eyes, &ball([q.head * 0.045; 3], e));
        if q.ears > 0.0 {
            let ear_base = add(add(crown, mul(hd, -q.head * 0.1)), [0.0, 0.0, side * q.head * 0.18]);
            let tip = add(ear_base, add(mul(up, q.ears), [-q.ears * 0.25, 0.0, side * q.ears * 0.35]));
            geo::merge(&mut skin, &cone(ear_base, tip, q.head * 0.09 + q.ears * 0.15, 8));
        }
    }
    // The tail from the rump, curving down (or up).
    let rump = [-l * 0.5 + g * 0.15, cy + g * 0.45, 0.0];
    if q.tail > 0.0 {
        let path: Vec<[f32; 3]> = (0..10).map(|i| {
            let t = i as f32 / 9.0;
            let a = 0.6 - t * (1.8 - q.tail_up * 2.4);
            [rump[0] - q.tail * t * a.cos().abs().max(0.3), rump[1] + q.tail * t * a.sin() * 0.8, 0.0]
        }).collect();
        geo::merge(&mut skin, &sh::tube(&path, |t| g * 0.12 * (1.0 - t * 0.7), 8));
    }
    QuadBody { skin, feet, eyes, nose, crown, head_at: hc, head_dir: hd, withers: [base[0] - g * 0.2, cy + g, 0.0], rump: [rump[0] + g * 0.3, cy + g, 0.0], paws, neck_line: [add(base, [0.0, g * 0.5, 0.0]), add(top, [0.0, q.head * 0.25, 0.0])] }
}

/// The face's morph targets, in order: a smile (mouth corners up and out),
/// the jaw opening, the brows raised and the eyelids closing.
pub const FACE_MORPHS: [&str; 4] = ["smile", "jaw_open", "brow_raise", "blink"];

/// A face's morph targets as per-vertex offsets ([`FACE_MORPHS`] order)
/// for a head mesh `m` whose centre is `c` and half height `hs`, facing +z:
/// each target moves the vertices of its region by falloffs, so any head
/// built round that centre takes them. Blend with [`apply_morphs`].
pub fn face_morphs(m: &Mesh, c: [f32; 3], hs: f32) -> Vec<Vec<[f32; 3]>> {
    let bell = |x: f32, w: f32| (-(x / w) * (x / w)).exp();
    let mut out = vec![Vec::with_capacity(m.vertices.len()); FACE_MORPHS.len()];
    for v in &m.vertices {
        let q = [(v[0] - c[0]) / hs, (v[1] - c[1]) / hs, (v[2] - c[2]) / hs];
        let front = ((q[2] - 0.2) / 0.5).clamp(0.0, 1.0);
        let ax = q[0].abs();
        let smile = bell(ax - 0.33, 0.16) * bell(q[1] + 0.48, 0.14) * front;
        out[0].push(mul([q[0].signum() * 0.05, 0.13, -0.04], hs * smile));
        let jaw = ((-0.5 - q[1]) / 0.25).clamp(0.0, 1.0) * ((q[2] + 0.3) / 0.8).clamp(0.0, 1.0);
        out[1].push(mul([0.0, -0.22, -0.06], hs * jaw));
        let brow = bell(q[1] - 0.32, 0.1) * bell(ax - 0.3, 0.22) * front;
        out[2].push(mul([0.0, 0.1, 0.02], hs * brow));
        let lid = bell(q[1] - 0.14, 0.07) * bell(ax - 0.32, 0.12) * front;
        out[3].push(mul([0.0, -0.05, 0.03], hs * lid));
    }
    out
}

/// `m` with the morph targets `targets` blended in by `weights`.
pub fn apply_morphs(m: &Mesh, targets: &[Vec<[f32; 3]>], weights: &[f32]) -> Mesh {
    let mut out = m.clone();
    for (t, w) in targets.iter().zip(weights) {
        for (v, d) in out.vertices.iter_mut().zip(t) {
            v[0] += d[0] * w;
            v[1] += d[1] * w;
            v[2] += d[2] * w;
        }
    }
    geo::smooth_normals(&mut out);
    out
}

/// The humanoid frame's bones as debug shapes: a long octahedron from each
/// bone's head joint to its tail joint (widest a fifth of the way along).
pub fn bone_shapes(j: &[[f32; 3]; 21], width: f32) -> Mesh {
    let mut m = Mesh::default();
    for (_, _, a, z) in HUMAN_BONES {
        let (p, q) = (j[a], j[z]);
        let d = sub(q, p);
        let l = dot(d, d).sqrt();
        if l < 1e-4 {
            continue;
        }
        let t = mul(d, 1.0 / l);
        let side = if t[1].abs() < 0.9 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] };
        let u = normalize(cross(t, side));
        let w = cross(t, u);
        let mid = add(p, mul(d, 0.2));
        let r = (l * 0.12).min(width);
        let ring: Vec<[f32; 3]> = (0..4).map(|k| {
            let a = k as f32 * PI * 0.5;
            add(mid, add(mul(u, r * a.cos()), mul(w, r * a.sin())))
        }).collect();
        let mut bl = MeshBuilder::new();
        for k in 0..4 {
            for (tip, c2) in [(p, ring[(k + 1) % 4]), (q, ring[(k + 1) % 4])] {
                let n = normalize(cross(sub(ring[k], tip), sub(c2, tip)));
                let i = bl.vertex(tip, n);
                bl.vertex(ring[k], n);
                bl.vertex(c2, n);
                bl.tri(i, i + 1, i + 2);
            }
        }
        let mut b2 = bl.finish();
        face_out(&mut b2, 0, |_, centre| sub(centre, mid));
        sh::flat_shade(&mut b2);
        geo::merge(&mut m, &b2);
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_loft_faces_out_and_keeps_its_size() {
        let m = loft_x(-1.0, 1.0, 6, |_| 1.0, |_| ellipse(0.5, 0.3, 12));
        let (lo, hi) = geo::bounds(&m);
        assert!((hi[0] - 1.0).abs() < 1e-4 && (lo[0] + 1.0).abs() < 1e-4 && (hi[2] - 0.5).abs() < 1e-3);
        // Outward: every skin normal points away from the axis.
        let out = m.vertices.iter().filter(|v| v[0].abs() < 0.9).all(|v| (v[1] - 1.0) * v[4] + v[2] * v[5] > -1e-3);
        assert!(out);
    }

    #[test]
    fn the_human_frame_stands_and_scales() {
        let h = Human::default();
        let j = h.frame();
        assert!((j[joint::CROWN][1] - 1.75).abs() < 1e-4);
        assert!(j[joint::ANKLE_L][1] > 0.0 && j[joint::SHOULDER_L][0] > 0.0 && j[joint::SHOULDER_R][0] < 0.0);
        let b = body_mesh(&body(&h, 0.0));
        let (lo, hi) = geo::bounds(&b);
        assert!(lo[1] > -0.01 && hi[1] > 1.7 && hi[1] < 1.85, "{lo:?} {hi:?}");
        for (name, parent, a, c) in HUMAN_BONES {
            assert!(parent.is_none_or(|p| p < HUMAN_BONES.len()) && a < 21 && c < 21, "{name}");
        }
    }

    #[test]
    fn a_quadruped_stands_on_its_feet() {
        let q = Quad { len: 2.0, girth: 0.35, leg: 1.0, leg_r: 0.06, neck: 0.8, neck_up: 0.9, head: 0.6, snout: 0.5, tail: 0.8, tail_up: 0.0, ears: 0.12 };
        let b = quadruped(&q);
        let (lo, hi) = geo::bounds(&b.skin);
        assert!(lo[1] > -0.05 && hi[1] > 1.8, "{lo:?} {hi:?}");
        assert!(b.paws.iter().all(|p| p[1] == 0.0));
    }

    #[test]
    fn a_wheel_is_round_about_its_axle() {
        let (t, r) = wheel([0.0, 0.4, 0.0], 0.4, 0.2, 6);
        let (lo, hi) = geo::bounds(&t);
        assert!((hi[1] - 0.8).abs() < 1e-3 && lo[1].abs() < 1e-3 && hi[2] < 0.11);
        assert!(!r.indices.is_empty());
    }
}
