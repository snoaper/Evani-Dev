//! Wear on the asphalt: faint big stains of darker and lighter road, a few
//! repair patches of newer or older asphalt, and skid marks (braking before a corner
//! and a drift through it). All of it is decals lying on the road (the same
//! kind as the start strip and the grass patches), so geocheck classes them as
//! decals; the layout keeps them clear of each other, of the painted lines
//! (the marks stay inside the lanes), of the start grid and of the item box
//! rows, because two decals in one plane fight for depth.
//!
//! Subtle by design: the tones stay within about 10 per cent of the road, the
//! skid marks within 45 per cent. A `lean` (Low) tier gets a third as many.

use evani_core::num::smooth01 as smooth;
use evani_render::{MeshId, MeshInstance};

use crate::draw::{DECAL_THICK, Mat, inst};
use crate::track::{Track, hash01};
use crate::trackmesh::ROAD_CLEAR;

/// Lane interior either side of the centre line (m from it): clear of the
/// dashes (0.125) and the edge lines (7 m road: 6.54 and out), with margin
/// for a straight piece laid on a bend.
pub const LANE_IN: f32 = 0.7;
const LANE_OUT_FROM_EDGE: f32 = 1.05;

/// Most marks of each kind on a full tier and on a lean one.
pub const STAINS: (u32, u32) = (12, 4);
pub const REPAIRS: (u32, u32) = (6, 3);
pub const CORNERS: (usize, usize) = (6, 3);
/// A repair patch's tone against the road: newer (darker) or older
/// (lighter) asphalt, within about 10 per cent so it never reads as a slab.
pub const REPAIR_TONES: (f32, f32) = (0.9, 1.08);

/// Most bend sharpness (1/m) a stain or repair patch may lie on.
const STRAIGHT_CURVATURE: f32 = 0.012;
/// Bend sharpness (1/m) a corner needs to get skid marks.
const CORNER_CURVATURE: f32 = 0.018;
/// Skid-mark tyre width and the gap between the tyres' centres (m).
const TYRE_W: f32 = 0.2;
const TYRE_GAP: f32 = 1.15;

/// A region of the road in (distance along, lateral) coordinates, to keep
/// marks off each other: `[s0, s1, lat0, lat1]`.
type Box2 = [f32; 4];

fn overlaps(a: &Box2, b: &Box2) -> bool {
    a[0] < b[1] && b[0] < a[1] && a[2] < b[3] && b[2] < a[3]
}

/// Rotation turning the mesh axes (x, y, z) into the given orthonormal basis.
pub fn basis_quat(x: [f32; 3], y: [f32; 3], z: [f32; 3]) -> [f32; 4] {
    // Matrix columns x, y, z.
    let (m00, m01, m02) = (x[0], y[0], z[0]);
    let (m10, m11, m12) = (x[1], y[1], z[1]);
    let (m20, m21, m22) = (x[2], y[2], z[2]);
    let tr = m00 + m11 + m22;
    let q = if tr > 0.0 {
        let s = (tr + 1.0).sqrt() * 2.0;
        [(m21 - m12) / s, (m02 - m20) / s, (m10 - m01) / s, 0.25 * s]
    } else if m00 > m11 && m00 > m22 {
        let s = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
        [0.25 * s, (m01 + m10) / s, (m02 + m20) / s, (m21 - m12) / s]
    } else if m11 > m22 {
        let s = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
        [(m01 + m10) / s, 0.25 * s, (m12 + m21) / s, (m02 - m20) / s]
    } else {
        let s = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
        [(m02 + m20) / s, (m12 + m21) / s, 0.25 * s, (m10 - m01) / s]
    };
    let l = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt().max(1e-9);
    [q[0] / l, q[1] / l, q[2] / l, q[3] / l]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn norm(a: [f32; 3]) -> [f32; 3] {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt().max(1e-9);
    [a[0] / l, a[1] / l, a[2] / l]
}

/// A flat decal strip lying on the road from `a` to `b` (each: distance
/// along, lateral), `width` wide: centred between them, lying along the
/// road's slope, [`seat`]ed on the road mesh (`ring`: [`mesh_rings`]).
#[allow(clippy::too_many_arguments)]
pub fn strip(track: &Track, ring: &[f32], a: (f32, f32), b: (f32, f32), width: f32, mat: Mat, color: [f32; 3]) -> MeshInstance {
    // Level across the road (this road has no camber).
    let mut m = slab(track.point_at(a.0, a.1), track.point_at(b.0, b.1), width, DECAL_THICK, mat, color);
    seat(track, ring, &mut m, (a.0 + b.0) * 0.5);
    m
}

/// A painted rectangle `width` across and `len` along the track, centred at
/// (`s`, `lat`), square to the track's heading there (its length stays
/// `len` on a bend, unlike a [`strip`] between two track points), lying
/// along the road mesh's slope and [`seat`]ed on it.
#[allow(clippy::too_many_arguments)]
pub fn plate(track: &Track, ring: &[f32], s: f32, lat: f32, width: f32, len: f32, mat: Mat, color: [f32; 3]) -> MeshInstance {
    let c = track.point_at(s, lat);
    let t = track.tangent_at(s);
    let end = |k: f32| {
        let (x, z) = (c[0] + t[0] * k, c[2] + t[1] * k);
        [x, crate::trackmesh::road_top(track, ring, x, z, s).unwrap_or(c[1]), z]
    };
    let mut m = slab(end(-0.5 * len), end(0.5 * len), width, DECAL_THICK, mat, color);
    seat(track, ring, &mut m, s);
    m
}

/// The y scale a stain or repair patch piece carries (one millimetre):
/// the one-sided top ignores it, so it only tells the draped pieces apart
/// from the painted marks ([`DECAL_THICK`]). Like every mark, a piece is
/// [`seat`]ed [`ROAD_CLEAR`] over the road mesh. (A closed patch 1 cm thick
/// showed its side faces to the outline pass's crease test, which inked its
/// edges like a raised slab, critic round 2; a top has no sides.)
pub const PATCH_THICK: f32 = 0.001;

/// Lay a flat decal (the plane mesh, or with `round` the kit's shadow disc,
/// both a unit footprint at y = 0 before scaling) on the road mesh: move it
/// up or down so that its lowest point over the road's real triangles is
/// [`ROAD_CLEAR`] above them. Its footprint is sampled about every 10 cm and
/// along its edges, so a ring's crease under it or the quad's diagonal can't
/// poke through between the corners. `s_hint` is its distance along the
/// track. Points off the road (a disc's edge on the verge) don't count.
pub fn seat(track: &Track, ring: &[f32], m: &mut MeshInstance, s_hint: f32) {
    if let Some(lift) = seat_lift(track, ring, m, s_hint, false) {
        m.position[1] += lift;
    }
}

/// How far [`seat`] would move `m` (m); `None` when no point is over the road.
pub fn seat_lift(track: &Track, ring: &[f32], m: &MeshInstance, s_hint: f32, round: bool) -> Option<f32> {
    let (sx, sz) = (m.scale[0], m.scale[2]);
    let mut local: Vec<[f32; 2]> = Vec::new();
    if round {
        // Rings about 10 cm apart, points about 10 cm apart round each.
        let reach = 0.5 * sx.max(sz);
        let nr = ((reach / 0.1).ceil() as usize).clamp(4, 40);
        for k in 0..=nr {
            let r = 0.5 * k as f32 / nr as f32;
            let n = if k == 0 { 1 } else { ((core::f32::consts::TAU * r * sx.max(sz) / 0.1).ceil() as usize).clamp(24, 256) };
            for a in 0..n {
                let a = a as f32 / n as f32 * core::f32::consts::TAU;
                local.push([r * a.cos(), r * a.sin()]);
            }
        }
    } else {
        let nu = ((sx / 0.1).ceil() as usize).clamp(1, 64);
        let nv = ((sz / 0.1).ceil() as usize).clamp(1, 128);
        for i in 0..=nu {
            for j in 0..=nv {
                local.push([i as f32 / nu as f32 - 0.5, j as f32 / nv as f32 - 0.5]);
            }
        }
    }
    let mut need: Option<f32> = None;
    for [u, v] in local {
        let o = crate::draw::quat_rotate(m.rotation, [u * sx, 0.0, v * sz]);
        let p = [m.position[0] + o[0], m.position[1] + o[1], m.position[2] + o[2]];
        if let Some(h) = crate::trackmesh::road_top(track, ring, p[0], p[2], s_hint) {
            let d = h + ROAD_CLEAR - p[1];
            need = Some(need.map_or(d, |n: f32| n.max(d)));
        }
    }
    need
}

/// The road mesh's surface at (`s`, `lat`) in the quad between kept rings
/// `ring[i]` and `ring[i + 1]` (`ring` holds the kept rings' distances,
/// ascending, closed by the track length): straight between the two rings,
/// as the mesh is, rather than on the smooth curve, which bulges above or
/// sags below it by up to a centimetre on a 4 m quad.
fn mesh_point(track: &Track, ring: &[f32], i: usize, s: f32, lat: f32) -> [f32; 3] {
    let (a, b) = (ring[i], ring[i + 1]);
    let u = ((s - a) / (b - a).max(1e-6)).clamp(0.0, 1.0);
    let (pa, pb) = (track.point_at(a, lat), track.point_at(b, lat));
    [pa[0] + (pb[0] - pa[0]) * u, pa[1] + (pb[1] - pa[1]) * u, pa[2] + (pb[2] - pa[2]) * u]
}

/// A wide patch from `s0` to `s1` at `lat`, `width` wide, draped on the road
/// mesh: one thin piece per mesh quad it crosses, each lying in its quad's
/// plane, so the patch can sit [`PATCH_THICK`] proud without the road
/// poking through it. Neighbouring pieces meet end to end on the ring line.
#[allow(clippy::too_many_arguments)]
fn patch(track: &Track, ring: &[f32], s0: f32, s1: f32, lat: f32, width: f32, mat: Mat, color: [f32; 3]) -> Vec<MeshInstance> {
    let mut cuts = vec![s0];
    cuts.extend(ring.iter().copied().filter(|&r| r > s0 + 0.05 && r < s1 - 0.05));
    cuts.push(s1);
    cuts.windows(2)
        .map(|c| {
            let mid = (c[0] + c[1]) * 0.5;
            let i = ring.iter().rposition(|&r| r <= mid).unwrap_or(0).min(ring.len() - 2);
            let (pa, pb) = (mesh_point(track, ring, i, c[0], lat), mesh_point(track, ring, i, c[1], lat));
            let mut m = slab(pa, pb, width, PATCH_THICK, mat, color);
            seat(track, ring, &mut m, mid);
            m
        })
        .collect()
}

/// A flat one-sided top from `pa` to `pb` (its centre line on the surface),
/// `width` wide, level across: the plane mesh, lying where the top of a box
/// `thick` thick would (half of `thick` above the centre line). It has no
/// underside, so nothing dark can show through it from far away (a closed
/// box this thin did: the engine draws back faces and lights them by their
/// own downward normal). `thick` stays in the scale's middle slot, which a
/// plane ignores, so the pieces can still be told apart by it.
fn slab(pa: [f32; 3], pb: [f32; 3], width: f32, thick: f32, mat: Mat, color: [f32; 3]) -> MeshInstance {
    let z = sub(pb, pa);
    let len = (z[0] * z[0] + z[1] * z[1] + z[2] * z[2]).sqrt().max(0.01);
    let z = norm(z);
    let x = norm(cross([0.0, 1.0, 0.0], z));
    let y = cross(z, x);
    let lift = thick * 0.5;
    let mid = [
        (pa[0] + pb[0]) * 0.5 + y[0] * lift,
        (pa[1] + pb[1]) * 0.5 + y[1] * lift,
        (pa[2] + pb[2]) * 0.5 + y[2] * lift,
    ];
    inst(MeshId::PLANE, mid, basis_quat(x, y, z), [width, thick, len], mat, color)
}

/// The kept rings' distances (the road mesh's quad ends), ascending and
/// closed by the track length.
pub fn mesh_rings(track: &Track, verge: f32) -> Vec<f32> {
    let ss = crate::trackmesh::rings(track);
    let lim = crate::trackmesh::limits(track, &ss);
    let mut ring: Vec<f32> = crate::trackmesh::kept(track, verge, &ss, &lim).into_iter().map(|j| ss[j]).collect();
    ring.push(track.length);
    ring
}

/// Whether the road at `s` is clear for marks: not the start grid, not an
/// item box row.
fn usable(track: &Track, s: f32) -> bool {
    if crate::trackmesh::near_grid(track, s - 3.0) || crate::trackmesh::near_grid(track, s + 3.0) {
        return false;
    }
    !track.item_rows.iter().any(|r| (s - r * track.length).abs() < 5.0)
}

/// Whether the road from `s0` to `s1` is clear for marks and straight enough
/// (a straight piece on a tight bend would leave its lane).
fn clear_straight(track: &Track, s0: f32, s1: f32, straight: bool) -> bool {
    let mut s = s0;
    while s <= s1 {
        if !usable(track, s) || (straight && track.curvature_ahead(s, 2.0).abs() > STRAIGHT_CURVATURE) {
            return false;
        }
        s += 1.0;
    }
    usable(track, s1)
}

/// Lane lateral range `[from, to]` (m, positive) the marks may fill.
fn lane(track: &Track) -> (f32, f32) {
    (LANE_IN, track.half_width - LANE_OUT_FROM_EDGE)
}

/// The corners worth skid marks, sharpest first: `(distance, sign)` with the
/// sign positive for a left bend, at least 45 m apart.
pub fn corners(track: &Track, max: usize) -> Vec<(f32, f32)> {
    let step = 3.0;
    let n = (track.length / step) as usize;
    let mut found: Vec<(f32, f32, f32)> = Vec::new();
    for i in 0..n {
        let s = i as f32 * step;
        let k = track.curvature_ahead(s, 6.0);
        if k.abs() >= CORNER_CURVATURE && s > 30.0 && s < track.length - 45.0 && clear_straight(track, s - 26.0, s + 5.0, false) {
            found.push((s, k.signum(), k.abs()));
        }
    }
    found.sort_by(|a, b| b.2.total_cmp(&a.2).then(a.0.total_cmp(&b.0)));
    let mut out: Vec<(f32, f32)> = Vec::new();
    for (s, sign, _) in found {
        if out.len() >= max {
            break;
        }
        if out.iter().all(|&(o, _)| track.delta_s(s, o).abs() > 45.0) {
            out.push((s, sign));
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

/// All the wear decals for `track`, in the `road` material. `lean`: a small
/// triangle budget (Low), a third as many marks.
pub fn road_marks(track: &Track, road: Mat, lean: bool, verge: f32) -> Vec<MeshInstance> {
    let mut out: Vec<MeshInstance> = Vec::new();
    let ring = mesh_rings(track, verge);
    let mut taken: Vec<Box2> = Vec::new();
    let (lane_in, lane_out) = lane(track);
    let pick = |p: (u32, u32)| if lean { p.1 } else { p.0 };
    let base = 0.98;

    // Skid marks first (they have the fixed places).
    for (c, &(sc, sign)) in corners(track, if lean { CORNERS.1 } else { CORNERS.0 }).iter().enumerate() {
        let seed = 1000 + c as u32;
        // Braking: straight twin strips in the bend's inside lane before it
        // (the marks never cross the dashed centre line).
        let (b0, b1) = (sc - 24.0, sc - 17.0);
        let out_lat = sign * (2.4 + 0.6 * hash01(seed, 1));
        // The drift: the tyres slide out towards the lane's edge after the
        // turn-in.
        let (d0, d1) = (sc - 8.0, sc + 3.0);
        let apex = sign * (4.2 + 0.5 * hash01(seed, 2));
        let step = if lean { 3.0 } else { 1.4 };
        let lat_of = |s: f32| {
            let u = smooth((s - d0) / (d1 - d0) * 1.4);
            out_lat + (apex - out_lat) * u
        };
        let edge = |lat: f32| lat.abs() + TYRE_GAP * 0.5 + TYRE_W * 0.5;
        if edge(out_lat) > lane_out || edge(apex) > lane_out {
            continue;
        }
        taken.push([b0 - 1.0, d1 + 1.0, -lane_out, lane_out]);
        for side in [-0.5f32, 0.5] {
            let o = side * TYRE_GAP;
            // Braking marks: darkest at the end.
            let pieces = if lean { 1 } else { 2 };
            for j in 0..pieces {
                let (s0, s1) = (b0 + (b1 - b0) * j as f32 / pieces as f32, b0 + (b1 - b0) * (j + 1) as f32 / pieces as f32);
                let tone = base * (0.78 - 0.18 * (j + 1) as f32 / pieces as f32);
                // Trimmed 4 cm at each end, like the drift pieces.
                out.push(strip(track, &ring, (s0 + 0.04, out_lat + o), (s1 - 0.04, out_lat + o), TYRE_W, road, [tone; 3]));
            }
            // The drift: darker as the tyres bite, fading out at the end.
            let n = ((d1 - d0) / step).round().max(2.0) as usize;
            for j in 0..n {
                let (s0, s1) = (d0 + (d1 - d0) * j as f32 / n as f32, d0 + (d1 - d0) * (j + 1) as f32 / n as f32);
                let f = j as f32 / (n - 1).max(1) as f32;
                let tone = base * (0.80 - 0.30 * (1.0 - (2.0 * f - 1.0).abs()));
                // Trimmed 4 cm at each end so the pieces never overlap at the
                // kink where the line sweeps sideways.
                let (t0, t1) = (s0 + 0.04, s1 - 0.04);
                out.push(strip(track, &ring, (t0, lat_of(t0) + o), (t1, lat_of(t1) + o), TYRE_W, road, [tone; 3]));
            }
        }
    }

    // Repair patches: a rectangle of newer (darker) or older (lighter)
    // asphalt.
    let mut placed = 0;
    let mut tries = 0;
    while placed < pick(REPAIRS) && tries < 200 {
        let i = tries;
        tries += 1;
        let side = if hash01(i, 11) < 0.5 { -1.0 } else { 1.0 };
        let (w, l) = (1.4 + 1.2 * hash01(i, 12), 2.4 + 2.0 * hash01(i, 13));
        let s = 20.0 + (track.length - 90.0) * hash01(i, 14);
        let lat_lo = lane_in + 0.3 + w * 0.5;
        let lat_hi = lane_out - 0.3 - w * 0.5;
        let lat = side * (lat_lo + (lat_hi - lat_lo) * hash01(i, 15));
        let seam = 0.14;
        let region: Box2 = [s - l * 0.5 - seam - 1.0, s + l * 0.5 + seam + 1.0, lat - w * 0.5 - seam - 0.3, lat + w * 0.5 + seam + 0.3];
        if !clear_straight(track, region[0], region[1], true) || taken.iter().any(|t| overlaps(t, &region)) {
            continue;
        }
        taken.push(region);
        placed += 1;
        // One low-contrast rectangle, no seam frame: a dark seam read as an
        // ink outline round a raised slab (critic round 2). The layout still
        // keeps the old seam's margin, so the marks stay where they were.
        let tone = base * if hash01(i, 16) < 0.6 { REPAIR_TONES.0 } else { REPAIR_TONES.1 };
        out.extend(patch(track, &ring, s - l * 0.5, s + l * 0.5, lat, w, road, [tone; 3]));
    }

    // Faint stains: big low-contrast patches of darker and lighter road.
    let mut placed = 0;
    let mut tries = 0;
    while placed < pick(STAINS) && tries < 300 {
        let i = 500 + tries;
        tries += 1;
        let side = if hash01(i, 21) < 0.5 { -1.0 } else { 1.0 };
        let (w, l) = (2.0 + 1.4 * hash01(i, 22), 4.5 + 2.5 * hash01(i, 23));
        let s = 12.0 + (track.length - 70.0) * hash01(i, 24);
        let lat_lo = lane_in + 0.1 + w * 0.5;
        let lat_hi = lane_out - 0.1 - w * 0.5;
        let lat = side * (lat_lo + (lat_hi - lat_lo) * hash01(i, 25));
        let region: Box2 = [s - l * 0.5 - 1.0, s + l * 0.5 + 1.0, lat - w * 0.5 - 0.3, lat + w * 0.5 + 0.3];
        if !clear_straight(track, region[0], region[1], true) || taken.iter().any(|t| overlaps(t, &region)) {
            continue;
        }
        taken.push(region);
        placed += 1;
        let tone = base * if placed % 2 == 0 { 0.9 } else { 1.1 };
        out.extend(patch(track, &ring, s - l * 0.5, s + l * 0.5, lat, w, road, [tone; 3]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw::resolve_materials;
    use crate::track::Track;
    use evani_core::plugin::LookData;

    fn track() -> Track {
        Track::from_json(include_str!("../data/track.json")).unwrap()
    }

    fn mat() -> Mat {
        resolve_materials(&LookData::default())[crate::draw::role::ROAD]
    }

    /// The decal's footprint corners on the ground (x, z).
    fn rect(m: &MeshInstance) -> [[f32; 2]; 4] {
        let q = m.rotation;
        let rot = |v: [f32; 3]| {
            // Rotate v by quaternion q.
            let u = [q[0], q[1], q[2]];
            let s = q[3];
            let c = cross(u, v);
            let d = cross(u, c);
            [v[0] + 2.0 * (s * c[0] + d[0]), v[1] + 2.0 * (s * c[1] + d[1]), v[2] + 2.0 * (s * c[2] + d[2])]
        };
        let (hx, hz) = (m.scale[0] * 0.5, m.scale[2] * 0.5);
        let at = |a: f32, b: f32| {
            let p = rot([a, 0.0, b]);
            [m.position[0] + p[0], m.position[2] + p[2]]
        };
        [at(-hx, -hz), at(hx, -hz), at(hx, hz), at(-hx, hz)]
    }

    /// Separating-axis test on two ground rectangles; a sliver under 1 mm
    /// deep counts as touching.
    fn touch(a: &[[f32; 2]; 4], b: &[[f32; 2]; 4]) -> bool {
        for poly in [a, b] {
            for i in 0..4 {
                let (p, q) = (poly[i], poly[(i + 1) % 4]);
                let ax = [-(q[1] - p[1]), q[0] - p[0]];
                let len = (ax[0] * ax[0] + ax[1] * ax[1]).sqrt();
                let proj = |r: &[[f32; 2]; 4]| {
                    let v: Vec<f32> = r.iter().map(|c| c[0] * ax[0] + c[1] * ax[1]).collect();
                    (v.iter().cloned().fold(f32::MAX, f32::min), v.iter().cloned().fold(f32::MIN, f32::max))
                };
                let (pa, pb) = (proj(a), proj(b));
                if pa.1 <= pb.0 + 0.001 * len || pb.1 <= pa.0 + 0.001 * len {
                    return false;
                }
            }
        }
        true
    }

    #[test]
    fn marks_are_deterministic_and_fewer_on_low() {
        let t = track();
        let full = road_marks(&t, mat(), false, 7.0);
        assert_eq!(full, road_marks(&t, mat(), false, 7.0));
        let lean = road_marks(&t, mat(), true, 7.0);
        assert!(full.len() > 60, "{} marks", full.len());
        assert!(lean.len() * 2 < full.len(), "low {} vs full {}", lean.len(), full.len());
        assert!(lean.len() > 10);
        // Cheap: 12 triangles each.
        assert!(full.len() as u32 * 12 < 4000);
    }

    #[test]
    fn corners_found_are_real_bends_far_apart() {
        let t = track();
        let c = corners(&t, 6);
        assert!(c.len() >= 3, "{c:?}");
        for (i, a) in c.iter().enumerate() {
            assert!(t.curvature_ahead(a.0, 6.0).abs() >= CORNER_CURVATURE);
            for b in &c[i + 1..] {
                assert!(t.delta_s(a.0, b.0).abs() > 45.0);
            }
        }
    }

    #[test]
    fn marks_lie_on_the_road_inside_the_lanes_and_never_on_each_other() {
        let t = track();
        for lean in [false, true] {
            let marks = road_marks(&t, mat(), lean, 7.0);
            let hw = t.half_width;
            for m in &marks {
                let r = rect(m);
                for c in r {
                    // On the road surface within 3 cm (geocheck's decal rule).
                    let loc = t.locate([c[0], m.position[1], c[1]], 0);
                    assert!(loc.lateral.abs() < hw - 0.5, "a corner is {} m off the centre", loc.lateral);
                    assert!(loc.lateral.abs() > 0.3, "over the centre dashes: {}", loc.lateral);
                }
                let ground = t.locate(m.position, 0).height;
                assert!((m.position[1] - ground).abs() < 0.03, "{} vs road {}", m.position[1], ground);
                assert_eq!(m.mesh, MeshId::PLANE);
                // Painted marks are 1 cm decals; stains and repair patches
                // are thin draped pieces.
                assert!((m.scale[1] - DECAL_THICK).abs() < 1e-6 || (m.scale[1] - PATCH_THICK).abs() < 1e-6, "thickness {}", m.scale[1]);
            }
            for (i, a) in marks.iter().enumerate() {
                let ra = rect(a);
                for b in &marks[i + 1..] {
                    // Pieces of one polyline meet end to end: allow touching.
                    // Pieces of one draped patch (same width and tone) meet
                    // on a ring line: allow the sliver a slight bend leaves.
                    let rb = rect(b);
                    let one_patch = (a.scale[1] - PATCH_THICK).abs() < 1e-6 && a.scale[1] == b.scale[1] && a.scale[0] == b.scale[0] && a.color == b.color;
                    if one_patch {
                        let gap = ((a.position[0] - b.position[0]).powi(2) + (a.position[2] - b.position[2]).powi(2)).sqrt();
                        assert!(gap <= (a.scale[2] + b.scale[2]) * 0.5 + 0.01 || !touch(&ra, &rb), "two patches share road at {:?} and {:?}", a.position, b.position);
                        continue;
                    }
                    assert!(!touch(&ra, &rb), "two decals share road at {:?} {:?} {:?} and {:?} {:?} {:?}", a.position, a.scale, a.color, b.position, b.scale, b.color);
                }
            }
        }
    }

    /// Every mark (stain and repair pieces, skid strips) lies just over the
    /// road mesh's own triangles, found by brute force over the built mesh
    /// (not through `road_top`): nowhere under [`ROAD_CLEAR`] (a mark that
    /// dips into the asphalt flickers against it), seated at that gap at its
    /// lowest point, and never floating.
    #[test]
    fn marks_lie_just_over_the_road_triangles() {
        use crate::trackmesh::tests::{height_over, road_tops};
        let t = track();
        let tops = road_tops(&crate::trackmesh::build(&t, 7.0)[crate::trackmesh::ROAD]);
        let mut pieces = 0;
        for lean in [false, true] {
            for m in road_marks(&t, mat(), lean, 7.0) {
                pieces += 1;
                let (nu, nv) = (((m.scale[0] / 0.1).ceil() as usize).max(2), ((m.scale[2] / 0.1).ceil() as usize).max(2));
                let (mut lo, mut hi) = (f32::MAX, f32::MIN);
                for i in 0..=nu {
                    for j in 0..=nv {
                        let (a, b) = ((i as f32 / nu as f32 - 0.5) * m.scale[0], (j as f32 / nv as f32 - 0.5) * m.scale[2]);
                        let p = crate::draw::quat_rotate(m.rotation, [a, 0.0, b]);
                        let w = [m.position[0] + p[0], m.position[1] + p[1], m.position[2] + p[2]];
                        let Some(h) = height_over(&tops, w[0], w[2]) else {
                            panic!("a mark is off the road at {w:?}");
                        };
                        lo = lo.min(w[1] - h);
                        hi = hi.max(w[1] - h);
                    }
                }
                assert!(lo > ROAD_CLEAR - 0.0003, "a mark at {:?} comes within {lo} m of the road", m.position);
                assert!(lo < ROAD_CLEAR + 0.0003, "a mark at {:?} floats: its lowest point is {lo} m up", m.position);
                assert!(hi < ROAD_CLEAR + 0.012, "a mark at {:?} stands {hi} m clear of the road at one end", m.position);
            }
        }
        assert!(pieces > 60, "{pieces} marks");
    }

    #[test]
    fn marks_stay_off_the_start_grid_and_item_box_rows() {
        let t = track();
        for m in road_marks(&t, mat(), false, 7.0) {
            let s = t.locate(m.position, 0).s;
            assert!(!crate::trackmesh::near_grid(&t, s), "mark at {s} m is on the grid");
            for r in &t.item_rows {
                assert!((s - r * t.length).abs() > 3.0, "mark at {s} m is on an item row");
            }
        }
    }

    #[test]
    fn the_basis_quaternion_is_the_yaw_on_flat_ground() {
        let yaw = 0.7f32;
        let z = [yaw.sin(), 0.0, yaw.cos()];
        let x = [yaw.cos(), 0.0, -yaw.sin()];
        let q = basis_quat(x, [0.0, 1.0, 0.0], z);
        let want = crate::draw::yaw_q(yaw);
        let dot: f32 = q.iter().zip(want).map(|(a, b)| a * b).sum();
        assert!(dot.abs() > 0.9999, "{q:?} vs {want:?}");
    }
}
