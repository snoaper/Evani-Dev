//! The studio's code-built sources (WP8 of docs/studio/WORK_PACKAGES.md,
//! taken by WP2): our own generated geometry, closed and consistently wound,
//! with a region index per vertex in the asset row's order.
//!
//! - `sphere`, `cube`: the test shapes (one region, `surface`), built in the
//!   app at no download cost.
//! - `wall`: a garden wall (`face`) with a weathered coping (`cap`) on a
//!   footing (`plinth`).
//! - `gable_roof`: a pitched roof of overlapping tile courses (`tiles`), a
//!   rounded ridge capping (`ridge`), its two triangular gable ends
//!   (`gable`), and the boards round its edges (`fascia`: a fascia along each
//!   eave and a barge board along each verge, which also closes the stepped
//!   ends of the tile courses).
//!
//! Each region is its own part with its own material, so every style keeps
//! them apart; [`code_uv`] lays a pattern on each at world scale the way the
//! real thing runs (tiles down the slope, ridge tiles along the ridge,
//! masonry level).
//! - `slope`: a patch of rolling ground rising to a bank, with its earth
//!   sides down to a flat base (`ground`).
//!
//! The wall, roof and patch are written as model tiers by
//! `examples/studio_assets.rs` and converted with `cargo xtask convert`
//! (their rows' `source.model`); [`code_mesh`] is in metres, [`code_source`]
//! at radius 1 like every source.

use evani_studio::geo::SourceMesh;

use crate::math::{V3, add, cross, normalize, scale, sub};
use crate::source::to_radius_one;

/// The shapes this file builds.
pub const SHAPES: [&str; 5] = ["sphere", "cube", "wall", "gable_roof", "slope"];

/// Region names per shape, in the order their indices use (they must equal
/// the asset rows' region ids; a test holds them to it).
pub fn regions(shape: &str) -> &'static [&'static str] {
    match shape {
        "sphere" | "cube" => &["surface"],
        "wall" => &["face", "cap", "plinth"],
        "gable_roof" => &["tiles", "ridge", "gable", "fascia"],
        "slope" => &["ground"],
        _ => &[],
    }
}

/// `shape` at radius 1 around its bounds' centre.
pub fn code_source(shape: &str) -> Option<SourceMesh> {
    let mut m = code_mesh(shape)?;
    to_radius_one(&mut m);
    Some(m)
}

/// `shape` in metres, standing on y = 0.
pub fn code_mesh(shape: &str) -> Option<SourceMesh> {
    let mut b = Builder::default();
    match shape {
        "sphere" => b.sphere(),
        "cube" => {
            let h = 0.5;
            b.cuboid([-h, 0.0, -h], [h, 2.0 * h, h], 0);
        }
        "wall" => wall(&mut b),
        "gable_roof" => roof(&mut b),
        "slope" => slope(&mut b),
        _ => return None,
    }
    Some(b.mesh)
}

/// Flat-shaded pieces: each face has its own vertices and normal.
#[derive(Default)]
struct Builder {
    mesh: SourceMesh,
}

impl Builder {
    /// A planar polygon, counter-clockwise seen from outside (a fan).
    fn face(&mut self, pts: &[V3], region: u8) {
        let n = normalize(cross(sub(pts[1], pts[0]), sub(pts[2], pts[0])), [0.0, 1.0, 0.0]);
        let base = self.mesh.positions.len() as u32;
        for p in pts {
            self.mesh.positions.push(*p);
            self.mesh.normals.push(n);
            self.mesh.regions.push(region);
        }
        for i in 1..pts.len() as u32 - 1 {
            self.mesh.indices.extend([base, base + i, base + i + 1]);
        }
    }

    /// A closed hexahedron from its 8 corners: bottom `b[0..4]` and top
    /// `t[0..4]`, both counter-clockwise seen from above.
    fn hexa(&mut self, b: [V3; 4], t: [V3; 4], region: u8) {
        self.face(&[b[0], b[3], b[2], b[1]], region);
        self.face(&[t[0], t[1], t[2], t[3]], region);
        for i in 0..4 {
            let j = (i + 1) % 4;
            self.face(&[b[i], b[j], t[j], t[i]], region);
        }
    }

    fn cuboid(&mut self, lo: V3, hi: V3, region: u8) {
        let at = |x: f32, z: f32, y: f32| [x, y, z];
        let ring = |y: f32| [at(lo[0], lo[2], y), at(lo[0], hi[2], y), at(hi[0], hi[2], y), at(hi[0], lo[2], y)];
        self.hexa(ring(lo[1]), ring(hi[1]), region);
    }

    /// A smooth sphere of radius 0.5 on y = 0 (24 x 48).
    fn sphere(&mut self) {
        let (rings, segs) = (24u32, 48u32);
        for r in 0..=rings {
            let th = std::f32::consts::PI * r as f32 / rings as f32;
            for s in 0..=segs {
                let ph = std::f32::consts::TAU * s as f32 / segs as f32;
                let n = [th.sin() * ph.cos(), th.cos(), th.sin() * ph.sin()];
                self.mesh.positions.push([n[0] * 0.5, n[1] * 0.5 + 0.5, n[2] * 0.5]);
                self.mesh.normals.push(n);
                self.mesh.regions.push(0);
            }
        }
        let w = segs + 1;
        for r in 0..rings {
            for s in 0..segs {
                let (a, b, c, d) = (r * w + s, r * w + s + 1, (r + 1) * w + s + 1, (r + 1) * w + s);
                self.mesh.indices.extend([a, b, c, a, c, d]);
            }
        }
    }
}

/// A garden wall: 2.4 m long, 1.2 m to the top of its coping, 0.23 m thick
/// (one brick's length) on a footing a course wider each side, with a
/// coping that overhangs 4 cm and is weathered to a shallow ridge.
fn wall(b: &mut Builder) {
    let (len, thick, body_top) = (2.4f32, 0.23f32, 1.10f32);
    let (x0, x1) = (-len / 2.0, len / 2.0);
    let (z0, z1) = (-thick / 2.0, thick / 2.0);
    // Footing (the plinth): one course (7.5 cm) high, 5 cm proud each side.
    b.cuboid([x0 - 0.05, 0.0, z0 - 0.05], [x1 + 0.05, 0.075, z1 + 0.05], 2);
    // Body, sunk a little into the footing so they are one piece.
    b.cuboid([x0, 0.07, z0], [x1, body_top + 0.005, z1], 0);
    // Coping: a slab with drip overhang and a weathered (ridged) top.
    let (o, slab, rise) = (0.04f32, 0.06f32, 0.04f32);
    let (cx0, cx1, cz0, cz1) = (x0 - o, x1 + o, z0 - o, z1 + o);
    let y0 = body_top;
    let y1 = body_top + slab;
    // Under side and the four slab sides up to y1, then the two weathered
    // slopes to the ridge, closed by the two end triangles.
    let p = |x: f32, y: f32, z: f32| [x, y, z];
    b.face(&[p(cx0, y0, cz0), p(cx1, y0, cz0), p(cx1, y0, cz1), p(cx0, y0, cz1)], 1);
    b.face(&[p(cx0, y0, cz1), p(cx1, y0, cz1), p(cx1, y1, cz1), p(cx0, y1, cz1)], 1);
    b.face(&[p(cx1, y0, cz0), p(cx0, y0, cz0), p(cx0, y1, cz0), p(cx1, y1, cz0)], 1);
    let ridge = y1 + rise;
    b.face(&[p(cx0, y1, cz1), p(cx1, y1, cz1), p(cx1, ridge, 0.0), p(cx0, ridge, 0.0)], 1);
    b.face(&[p(cx1, y1, cz0), p(cx0, y1, cz0), p(cx0, ridge, 0.0), p(cx1, ridge, 0.0)], 1);
    b.face(&[p(cx0, y0, cz0), p(cx0, y0, cz1), p(cx0, y1, cz1), p(cx0, ridge, 0.0), p(cx0, y1, cz0)], 1);
    b.face(&[p(cx1, y0, cz1), p(cx1, y0, cz0), p(cx1, y1, cz0), p(cx1, ridge, 0.0), p(cx1, y1, cz1)], 1);
}

/// The roof's pitch (degrees), span across the eaves and depth along the
/// ridge (metres): [`code_uv`] lays the tiles down this slope.
pub const ROOF_PITCH_DEG: f32 = 40.0;
const ROOF_SPAN: f32 = 2.6;
const ROOF_DEPTH: f32 = 2.2;
/// The tile courses' thickness (metres).
const TILE_THICK: f32 = 0.035;

/// The roof's regions, in [`regions`]'s order.
const TILES: u8 = 0;
const RIDGE: u8 = 1;
const GABLE: u8 = 2;
const FASCIA: u8 = 3;

/// A gable roof: 2.6 m across the eaves, 2.2 m along the ridge, pitched
/// 40 degrees, of eight overlapping tile courses a side (each course a
/// slab tilted a little steeper than the roof, so its lower edge stands
/// proud of the next: the stepped look tiles have), a rounded ridge
/// capping, the two triangular gable ends under it, a fascia board along
/// each eave and a barge board along each verge (over the courses' stepped
/// ends, so the roof's edge reads as one straight line).
fn roof(b: &mut Builder) {
    let pitch = ROOF_PITCH_DEG.to_radians();
    let half = ROOF_SPAN / 2.0;
    let rise = half * pitch.tan();
    let (z0, z1) = (-ROOF_DEPTH / 2.0, ROOF_DEPTH / 2.0);
    let (overhang, thick, courses) = (0.12f32, TILE_THICK, 8u32);
    // Gable ends: triangles set in by the overhang, as thin closed slabs.
    let wall_half = half - 0.02;
    let gable_rise = wall_half * pitch.tan();
    for (zf, zb) in [(z0 + overhang, z0 + overhang + 0.12), (z1 - overhang - 0.12, z1 - overhang)] {
        let tri = |z: f32| [[-wall_half, 0.0, z], [wall_half, 0.0, z], [0.0, gable_rise, z]];
        let (f, k) = (tri(zf), tri(zb));
        b.face(&[f[0], f[2], f[1]], GABLE);
        b.face(&[k[0], k[1], k[2]], GABLE);
        b.face(&[f[0], f[1], k[1], k[0]], GABLE);
        b.face(&[f[1], f[2], k[2], k[1]], GABLE);
        b.face(&[f[2], f[0], k[0], k[2]], GABLE);
    }
    // Tile courses on each side, from the eaves (a little out past the
    // gable walls) up to the ridge.
    let eave = half + 0.15;
    let at = |p: V3, z: f32| [p[0], p[1], z];
    for side in [-1.0f32, 1.0] {
        // Along the slope from the eave point to the ridge.
        let foot: V3 = [side * eave, -(eave - half) * pitch.tan(), 0.0];
        let top: V3 = [0.0, rise, 0.0];
        let slope = sub(top, foot);
        let l = (slope[0] * slope[0] + slope[1] * slope[1]).sqrt();
        let dir = scale(slope, 1.0 / l);
        // Outward normal of this roof plane.
        let up: V3 = [side * pitch.sin(), pitch.cos(), 0.0];
        let course = l / courses as f32;
        for c in 0..courses {
            // Each course runs from its lower edge a little past the next
            // course's lower edge (the lap), its lower edge lifted off the
            // plane by one tile thickness (the kick).
            let s0 = c as f32 * course;
            let s1 = ((c + 1) as f32 * course + course * 0.35).min(l);
            let low = add(foot, scale(dir, s0));
            let high = add(foot, scale(dir, s1));
            let kick = scale(up, thick * 1.1);
            let (lo_in, hi_in) = (add(low, kick), high);
            let (lo_out, hi_out) = (add(lo_in, scale(up, thick)), add(hi_in, scale(up, thick)));
            let (za, zb) = (z0, z1);
            // Bottom ring (under side), top ring (outer side), both as quads
            // walked so the faces wind outward on either roof side.
            let bot = [at(lo_in, za), at(hi_in, za), at(hi_in, zb), at(lo_in, zb)];
            let topq = [at(lo_out, za), at(hi_out, za), at(hi_out, zb), at(lo_out, zb)];
            let first = b.mesh.positions.len();
            hexa_between(b, wind(side, bot), wind(side, topq), TILES);
            // The course's front edge (the step down to the next course)
            // faces down the slope, so the floor's bounce light lit it
            // almost white: the thin white lines across the slope. It shades
            // as the slope it belongs to, like a bevel.
            for n in &mut b.mesh.normals[first..] {
                if crate::math::dot(*n, dir) < -0.9 {
                    *n = up;
                }
            }
        }
        // The fascia: a board under the eave, along the ridge's direction,
        // from verge to verge.
        let fz = 0.03f32;
        let (fo, fi) = (side * eave, side * (eave - 0.035));
        let (fy0, fy1) = (foot[1] - 0.14, foot[1] + thick * 1.2);
        let ring = |y: f32| [[fi, y, z0 - fz], [fi, y, z1 + fz], [fo, y, z1 + fz], [fo, y, z0 - fz]];
        let (lo, hi) = (ring(fy0), ring(fy1));
        // Counter-clockwise from above on the +x side; mirrored on -x.
        let flip = |q: [V3; 4]| if side > 0.0 { q } else { [q[0], q[3], q[2], q[1]] };
        b.hexa(flip(lo), flip(hi), FASCIA);
        // The barge boards: one along each verge, just outside the tile
        // courses' ends, from below the courses to over their steps.
        let (below, above, board) = (0.06f32, thick * 2.8, 0.03f32);
        let foot_b = add(foot, scale(dir, -0.02));
        let top_b = add(top, scale(dir, 0.02));
        for (za, zb) in [(z0 - board, z0), (z1, z1 + board)] {
            let inner = [at(add(foot_b, scale(up, -below)), za), at(add(top_b, scale(up, -below)), za), at(add(top_b, scale(up, -below)), zb), at(add(foot_b, scale(up, -below)), zb)];
            let outer = [at(add(foot_b, scale(up, above)), za), at(add(top_b, scale(up, above)), za), at(add(top_b, scale(up, above)), zb), at(add(foot_b, scale(up, above)), zb)];
            hexa_between(b, wind(side, inner), wind(side, outer), FASCIA);
        }
    }
    // Ridge capping: half-round ridge tiles along the ridge (a closed
    // prism, 8 segments over the top), a little past both verges.
    let (rr, segs) = (0.12f32, 8u32);
    let ry = rise + thick * 1.2;
    let arc: Vec<V3> = (0..=segs)
        .map(|k| {
            let a = std::f32::consts::PI * k as f32 / segs as f32;
            [rr * a.cos(), ry + rr * 0.75 * a.sin(), 0.0]
        })
        .collect();
    let (ra, rb) = (z0 - 0.04, z1 + 0.04);
    let base_y = ry - 0.03;
    // The section: the arc from +x over the top to -x, then down and across
    // the flat underside.
    let mut sec = arc.clone();
    sec.push([-rr, base_y, 0.0]);
    sec.push([rr, base_y, 0.0]);
    let n = sec.len();
    for k in 0..n {
        let (p, q) = (sec[k], sec[(k + 1) % n]);
        b.face(&[at(p, rb), at(p, ra), at(q, ra), at(q, rb)], RIDGE);
    }
    let front: Vec<V3> = sec.iter().map(|p| at(*p, rb)).collect();
    let back: Vec<V3> = sec.iter().rev().map(|p| at(*p, ra)).collect();
    b.face(&front, RIDGE);
    b.face(&back, RIDGE);
}

/// A tile course's quad walked so the slab winds outward on its roof side.
fn wind(side: f32, q: [V3; 4]) -> [V3; 4] {
    if side > 0.0 { q } else { [q[0], q[3], q[2], q[1]] }
}

/// The pattern coordinates of point `p` (normal `n`) on region `region` of
/// code shape `shape`, at radius 1 like [`code_source`]: the same scale as
/// the studio's own projection (half a unit of uv per unit of length), laid
/// the way the real thing runs. `None`: the studio's projection along the
/// face's main axis (masonry level, the wall, the gable ends, the ground).
///
/// - Roof tiles: v runs down the slope from the ridge (along the surface, so
///   the tiles aren't stretched) and u along the ridge, so each tile points
///   downhill and its courses run level along the roof.
/// - Ridge tiles: v along the ridge, u round the capping.
/// - Fascia and barge boards: the grain along the board.
pub fn code_uv(shape: &str, region: u8, p: [f32; 3], n: [f32; 3]) -> Option<[f32; 2]> {
    if shape != "gable_roof" {
        return None;
    }
    let pitch = ROOF_PITCH_DEG.to_radians();
    let s = 0.5;
    match region {
        TILES => {
            // Distance down the slope (the plane through the ridge line).
            let down = p[0].abs() * pitch.cos() - p[1] * pitch.sin();
            Some([p[2] * s + 0.5, down * s])
        }
        RIDGE => {
            let round = p[0].atan2(p[1].max(1e-3)) * 0.15;
            Some([round * s + 0.5, p[2] * s])
        }
        FASCIA => {
            if n[2].abs() > 0.7 {
                // A barge board's face: grain along the slope.
                let along = p[0].abs() * pitch.cos() + p[1] * pitch.sin();
                let across = -p[0].abs() * pitch.sin() + p[1] * pitch.cos();
                Some([along * s, across * s + 0.5])
            } else if n[1].abs() > 0.7 && p[1] > 0.0 {
                // A barge board's top edge: along the slope too.
                let along = p[0].abs() * pitch.cos() + p[1] * pitch.sin();
                Some([along * s, p[2] * s + 0.5])
            } else {
                // A fascia board (and its ends): grain along the eave.
                Some([p[2] * s + 0.5, -p[1] * s])
            }
        }
        _ => None,
    }
}

/// A closed slab between an inner quad and an outer quad (both wound the
/// same way); faces are wound so the slab is closed and outward.
fn hexa_between(b: &mut Builder, inner: [V3; 4], outer: [V3; 4], region: u8) {
    // inner walked counter-clockwise seen from outside the slab's outer face.
    b.face(&[outer[0], outer[1], outer[2], outer[3]], region);
    b.face(&[inner[0], inner[3], inner[2], inner[1]], region);
    for i in 0..4 {
        let j = (i + 1) % 4;
        b.face(&[inner[i], inner[j], outer[j], outer[i]], region);
    }
}

/// A 2.4 m patch of rolling ground rising to a bank at the back, its four
/// earth sides dropping to a flat base 0.3 m below its lowest point.
fn slope(b: &mut Builder) {
    let (size, n) = (2.4f32, 24usize);
    let h = |x: f32, z: f32| {
        // The bank: up 0.45 m over the back third; then gentle swells.
        let t = ((-z / size) + 0.5).clamp(0.0, 1.0);
        let bank = 0.45 * (t * t * (3.0 - 2.0 * t));
        let swell = 0.05 * (x * 2.1 + 0.4).sin() * (z * 1.7 - 0.3).cos() + 0.025 * (x * 4.3 - z * 3.1).sin();
        0.3 + bank + swell
    };
    let step = size / n as f32;
    let at = |i: usize, j: usize| {
        let (x, z) = (-size / 2.0 + i as f32 * step, -size / 2.0 + j as f32 * step);
        [x, h(x, z), z]
    };
    // The top: a smooth grid (shared vertices, averaged normals).
    let base = b.mesh.positions.len() as u32;
    for j in 0..=n {
        for i in 0..=n {
            let p = at(i, j);
            let e = 0.01;
            let nx = h(p[0] - e, p[2]) - h(p[0] + e, p[2]);
            let nz = h(p[0], p[2] - e) - h(p[0], p[2] + e);
            b.mesh.positions.push(p);
            b.mesh.normals.push(normalize([nx, 2.0 * e, nz], [0.0, 1.0, 0.0]));
            b.mesh.regions.push(0);
        }
    }
    let w = (n + 1) as u32;
    for j in 0..n as u32 {
        for i in 0..n as u32 {
            let (a, bb, c, d) = (base + j * w + i, base + j * w + i + 1, base + (j + 1) * w + i + 1, base + (j + 1) * w + i);
            b.mesh.indices.extend([a, c, bb, a, d, c]);
        }
    }
    // The four earth sides down to y = 0, and the base.
    let edge = |k: usize, side: usize| match side {
        0 => at(k, 0),
        1 => at(n, k),
        2 => at(n - k, n),
        _ => at(0, n - k),
    };
    for side in 0..4 {
        for k in 0..n {
            let (p, q) = (edge(k, side), edge(k + 1, side));
            b.face(&[[q[0], 0.0, q[2]], [p[0], 0.0, p[2]], p, q], 0);
        }
    }
    // The base: the same grid flat on y = 0, facing down, so its edges meet
    // the sides' (no T-junctions).
    let flat = |i: usize, j: usize| [-size / 2.0 + i as f32 * step, 0.0, -size / 2.0 + j as f32 * step];
    for j in 0..n {
        for i in 0..n {
            b.face(&[flat(i, j), flat(i + 1, j), flat(i + 1, j + 1), flat(i, j + 1)], 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use evani_studio::Catalogs;

    use super::*;
    use crate::GeometryGenerator;

    /// Each code shape is a valid source for its row (regions in the row's
    /// order, every region present), closed (each edge used twice, once each
    /// way: watertight and consistently wound), facing out (positive
    /// volume), and passes the geometry stage's plain pass.
    #[test]
    fn every_code_shape_is_closed_outward_and_fits_its_row() {
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        let c = Catalogs::load(root).unwrap();
        for shape in SHAPES {
            let row = c.assets.assets.iter().find(|a| a.source.shape.as_deref() == Some(shape)).unwrap_or_else(|| panic!("no row builds `{shape}`"));
            let names: Vec<&str> = row.regions.iter().map(|r| r.id.as_str()).collect();
            assert_eq!(names, regions(shape), "{shape}: the row's regions");
            let src = code_source(shape).unwrap();
            src.validate(row.regions.len()).unwrap();
            assert!(crate::source::coverage_problems(&src, row, 3).is_empty(), "{shape}");
            assert!(crate::Passthrough.generate(&src, row, &crate::Geometry::None).is_ok(), "{shape}");
            let m = code_mesh(shape).unwrap();
            // Welded by position, every directed edge has its reverse.
            let key = |p: V3| p.map(|x| (x * 1e4).round() as i64);
            let mut edges = std::collections::BTreeMap::<([i64; 3], [i64; 3]), i32>::new();
            for t in m.indices.as_chunks::<3>().0 {
                for k in 0..3 {
                    let (a, bb) = (key(m.positions[t[k] as usize]), key(m.positions[t[(k + 1) % 3] as usize]));
                    *edges.entry((a, bb)).or_default() += 1;
                }
            }
            let open: Vec<_> = edges.iter().filter(|((a, bb), n)| edges.get(&(*bb, *a)).copied().unwrap_or(0) != **n).take(3).collect();
            // Overlapping closed pieces (the wall's footing and body, the
            // roof's lapped courses) are closed each on their own: count per
            // directed edge must match its reverse.
            assert!(open.is_empty(), "{shape}: open or mis-wound edges, e.g. {open:?}");
            let vol: f32 = m.indices.as_chunks::<3>().0.iter().map(|t| {
                let [a, bb, cc] = t.map(|i| m.positions[i as usize]);
                crate::math::dot(a, cross(bb, cc)) / 6.0
            }).sum();
            assert!(vol > 0.0, "{shape}: faces point inward (volume {vol})");
            assert_eq!(code_mesh(shape), code_mesh(shape), "{shape}: deterministic");
        }
        assert!(code_source("teapot").is_none());
    }

    /// The roof's tile uvs run down the slope: walking down a slope (away
    /// from the ridge, along the surface) grows v at the uv scale and leaves
    /// u alone; walking along the ridge grows u and leaves v. The same on
    /// both sides, so tiles point downhill on each.
    #[test]
    fn roof_tile_uvs_run_down_the_slope() {
        let src = code_source("gable_roof").unwrap();
        let p = ROOF_PITCH_DEG.to_radians();
        for side in [-1.0f32, 1.0] {
            let n = [side * p.sin(), p.cos(), 0.0];
            let down = [side * p.cos(), -p.sin(), 0.0];
            let a = [side * 0.3, 0.1, 0.2];
            let b = add(a, scale(down, 0.2));
            let (ua, ub) = (code_uv("gable_roof", TILES, a, n).unwrap(), code_uv("gable_roof", TILES, b, n).unwrap());
            assert!((ub[1] - ua[1] - 0.1).abs() < 1e-4 && (ub[0] - ua[0]).abs() < 1e-5, "side {side}: {ua:?} -> {ub:?}");
            let c = add(a, [0.0, 0.0, 0.2]);
            let uc = code_uv("gable_roof", TILES, c, n).unwrap();
            assert!((uc[0] - ua[0] - 0.1).abs() < 1e-4 && (uc[1] - ua[1]).abs() < 1e-5, "side {side}: along the ridge {ua:?} -> {uc:?}");
        }
        // Every tile vertex gets a tile uv; masonry and the other shapes keep
        // the studio's level projection.
        assert!(src.positions.iter().zip(&src.normals).zip(&src.regions).filter(|(_, r)| **r == TILES).all(|((p, n), _)| code_uv("gable_roof", TILES, *p, *n).is_some()));
        assert!(code_uv("gable_roof", GABLE, [0.0; 3], [0.0, 0.0, 1.0]).is_none());
        assert!(code_uv("wall", 0, [0.0; 3], [0.0, 0.0, 1.0]).is_none());
    }

    /// Every working style keeps the roof's and the wall's parts apart: a
    /// voxel roof still has tile voxels, ridge, brick gable and board
    /// voxels, each part on its own region.
    #[test]
    fn styles_keep_the_parts_apart() {
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        let c = Catalogs::load(root).unwrap();
        for shape in ["gable_roof", "wall"] {
            let row = c.assets.assets.iter().find(|a| a.source.shape.as_deref() == Some(shape)).unwrap();
            let src = code_source(shape).unwrap();
            for s in c.styles.styles.iter().filter(|s| evani_studio::geo::generator_version(s.geometry.kind()) > 0) {
                let out = crate::generator(s.geometry.kind()).generate(&src, row, &s.geometry).unwrap_or_else(|e| panic!("{shape} {}: {e}", s.id));
                let mut seen = vec![0usize; row.regions.len()];
                for p in &out.parts {
                    seen[usize::from(p.region)] += p.indices.len() / 3;
                }
                assert!(seen.iter().all(|n| *n > 0), "{shape} in {} ({:?}): triangles per region {seen:?}", s.id, s.geometry.kind());
            }
        }
    }

    /// The wall, roof and ground patch also pass the engine's model
    /// pipeline: written by `studio_assets code`, converted with `cargo xtask
    /// convert --tiers low` (tests/fixtures/models/studio/), they read back
    /// through `source_from_glb` with every region and the same bounds as the
    /// code source.
    #[test]
    fn the_converted_props_read_back_as_their_code_source() {
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        let c = Catalogs::load(root).unwrap();
        for shape in ["wall", "gable_roof", "slope"] {
            let row = c.assets.assets.iter().find(|a| a.source.shape.as_deref() == Some(shape)).unwrap();
            let glb = std::fs::read(root.join(format!("tests/fixtures/models/studio/{shape}.low.glb"))).unwrap();
            let read = crate::source::source_from_glb(&glb, row).unwrap();
            read.validate(row.regions.len()).unwrap();
            assert!(crate::source::coverage_problems(&read, row, 3).is_empty(), "{shape}: {:?}", crate::source::region_counts(&read, row.regions.len()));
            let built = code_source(shape).unwrap();
            let (lo, hi) = crate::math::bounds(&read.positions);
            let (blo, bhi) = crate::math::bounds(&built.positions);
            for k in 0..3 {
                assert!((lo[k] - blo[k]).abs() < 0.02 && (hi[k] - bhi[k]).abs() < 0.02, "{shape}: bounds {lo:?}..{hi:?} vs {blo:?}..{bhi:?}");
            }
        }
    }
}
