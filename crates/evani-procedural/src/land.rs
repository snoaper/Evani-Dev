//! Realism for land: height fields that read as weathered ground, and the
//! ground cover laid over them by slope, height and a noisy patch field
//! (grass on the flats, soil where it is worn, rock where it is steep, snow
//! on the tops), each layer in its own catalogue material
//! ([`crate::surface`]). Terrain assets are a height function plus a list
//! of [`Layer`]s; [`cover`] does the rest.

use evani_render::mesh::Mesh;

use crate::asset::Part;
use crate::detail::split_by;
use crate::noise::{fbm, fbm3p, warp};
use crate::surface::Surface;

/// One ground-cover layer: where it lies and what it is. A face takes the
/// first layer whose rules it meets; the last layer is the fallback.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layer {
    pub surface: Surface,
    /// Steepness range (0 flat, 1 vertical) the layer covers.
    pub slope: [f32; 2],
    /// Height range (m over the asset's base).
    pub height: [f32; 2],
    /// A patch field: the layer only where low-frequency noise (features
    /// about `patch_size` m) is over `patch` (-1: everywhere, 0: about half).
    pub patch: f32,
    pub patch_size: f32,
}

impl Layer {
    /// A layer everywhere (the fallback).
    pub const fn all(surface: Surface) -> Self {
        Self { surface, slope: [-1.0, 2.0], height: [f32::MIN, f32::MAX], patch: -2.0, patch_size: 8.0 }
    }

    pub const fn slope(mut self, from: f32, to: f32) -> Self {
        self.slope = [from, to];
        self
    }

    pub const fn height(mut self, from: f32, to: f32) -> Self {
        self.height = [from, to];
        self
    }

    pub const fn patches(mut self, over: f32, size: f32) -> Self {
        self.patch = over;
        self.patch_size = size;
        self
    }
}

/// Lay `layers` over the terrain mesh `m`: each face to the first layer
/// whose slope, height and patch rules it meets (each boundary moved by
/// noise, `wobble` in the rule's own units, so it meanders), one part per
/// layer that got faces. Deterministic.
pub fn cover(m: &Mesh, layers: &[Layer], wobble: f32, seed: u32) -> Vec<Part> {
    if layers.is_empty() {
        return Vec::new();
    }
    let parts = split_by(m, layers.len(), |c, n| {
        let steep = 1.0 - n[1].clamp(-1.0, 1.0).abs();
        let jitter = |k: u32, size: f32| wobble * fbm3p([c[0] / size, c[1] / size, c[2] / size], seed ^ k.wrapping_mul(0x9e37_79b9), 3, 0.55);
        for (i, l) in layers.iter().enumerate() {
            let s = steep + jitter(i as u32 * 3 + 1, 2.5);
            let h = c[1] + jitter(i as u32 * 3 + 2, 3.0) * 4.0;
            let p = fbm(c[0] / l.patch_size.max(0.1), c[2] / l.patch_size.max(0.1), seed ^ (0x51ed + i as u32), 4, 2.0, 0.55) + jitter(i as u32 * 3 + 3, 1.0);
            if s >= l.slope[0] && s <= l.slope[1] && h >= l.height[0] && h <= l.height[1] && p > l.patch {
                return i;
            }
        }
        layers.len() - 1
    });
    parts.into_iter().zip(layers).filter(|(p, _)| !p.indices.is_empty()).map(|(p, l)| Part::surfaced(p, l.surface)).collect()
}

/// Weathered rolling ground in about [0, 1]: warped gradient fBm (`octaves`
/// layers, each `gain` as strong), its lows flattened (valley floors fill
/// in) and its highs rounded. `x`, `z` in feature units.
pub fn rolling(x: f32, z: f32, seed: u32, octaves: u32, gain: f32) -> f32 {
    let (wx, wz) = warp(x, z, seed ^ 0x2d51, 0.35, 0.6);
    let n = fbm(wx, wz, seed, octaves, 2.0, gain) * 0.5 + 0.5;
    // Lowlands fill (soil and water settle there), tops stay round.
    let t = n.clamp(0.0, 1.0);
    t * t * (1.6 - 0.6 * t)
}

/// A turf (lawn) surface as ground seen from a few metres up: fine blades
/// (0.25 m per uv unit, so no stripe pattern reads), high blade variation,
/// soil showing between the blades and a light dust film for broad
/// patchiness (foliage numbers 0 variation and 6 bare).
pub fn turf(s: Surface) -> Surface {
    // Round 8: no mowing stripes (number 7): a field is pasture, and the
    // stripes read as a regular graphic pattern (the shared review's V1).
    // Round 9: the blade-scale soil (number 6) down to a trace: at 0.3 it
    // drew sharp, dark mud blobs over the near half of every field; the
    // worn soil is the soft patches (`patchy`).
    s.number(0, 0.9).number(6, 0.1).number(7, 0.0).condition("dust", 0.2).scale(0.25)
}

/// A turf with ground patches (procedural realism round 6): dry-grass and
/// worn-soil patches a few metres across, more soil on slopes, drawn
/// smoothly by the turf shader (foliage number 9 `patches`, 0..1 the worn
/// share), so no patch edge follows the mesh's faces as [`cover`]'s split
/// does.
pub fn patchy(s: Surface, amount: f32) -> Surface {
    s.number(9, amount)
}

/// The soil sides of a cut plot: walls from the border of `m`, a
/// [`crate::geo::height_grid`] of `cells` cells per side, down to y =
/// `floor`, facing out (flat normals). Drawn in a soil material they make a
/// terrain read as a cut block of ground standing on the floor rather than a
/// thin card.
pub fn soil_sides(m: &Mesh, cells: u32, floor: f32) -> Mesh {
    let n = cells.clamp(1, 512) as usize;
    let row = n + 1;
    let mut b = crate::geo::MeshBuilder::new();
    if m.vertices.len() < row * row {
        return b.finish();
    }
    let sides: [(Vec<usize>, [f32; 3]); 4] = [
        ((0..row).collect(), [0.0, 0.0, -1.0]),
        ((0..row).map(|i| n * row + i).collect(), [0.0, 0.0, 1.0]),
        ((0..row).map(|j| j * row).collect(), [-1.0, 0.0, 0.0]),
        ((0..row).map(|j| j * row + n).collect(), [1.0, 0.0, 0.0]),
    ];
    for (edge, nrm) in sides {
        for w in edge.windows(2) {
            let (pa, pb) = (m.vertices[w[0]], m.vertices[w[1]]);
            let top = |p: [f32; 6]| [p[0], p[1], p[2]];
            let bot = |p: [f32; 6]| [p[0], floor, p[2]];
            let (a, bb) = (top(pa), top(pb));
            // Wind each quad counter-clockwise seen from outside.
            let e = [bb[0] - a[0], 0.0, bb[2] - a[2]];
            let out = e[0] * nrm[2] - e[2] * nrm[0] > 0.0;
            let q = [b.vertex(a, nrm), b.vertex(bot(pa), nrm), b.vertex(bot(pb), nrm), b.vertex(bb, nrm)];
            if out {
                b.quad(q[0], q[1], q[2], q[3]);
            } else {
                b.quad(q[0], q[3], q[2], q[1]);
            }
        }
    }
    b.finish()
}

/// Rock, grass and snow over a landform (Chris, 2026-11-04: "if you are
/// looking at a mountain then you may want a mountain with more rock less
/// grass or a mountain with snow"). `rock` (0..1) is how much of the ground
/// is bare rock: from the steepest faces only down to fairly gentle slopes,
/// and over the high ground when it is past a half; `snow` (0..1) is how far
/// down from the top the snow lies (not on faces too steep to hold it);
/// grass, a patchy turf, is the rest. Each boundary meanders ([`cover`]).
pub fn landform(m: &Mesh, rock_surface: Surface, grass: Surface, rock: f32, snow: f32, seed: u32) -> Vec<Part> {
    let (lo, hi) = crate::geo::bounds(m);
    let top = (hi[1] - lo[1]).max(0.01);
    let (rock, snow) = (rock.clamp(0.0, 1.0), snow.clamp(0.0, 1.0));
    let mut layers = Vec::with_capacity(4);
    if snow > 0.0 {
        layers.push(Layer::all(crate::surface::presets::GROUND_SNOW_PACKED.seed(seed)).height(hi[1] - top * 0.9 * snow, f32::MAX).slope(-1.0, 0.42));
    }
    layers.push(Layer::all(rock_surface).slope(0.34 - 0.3 * rock, 2.0));
    if rock > 0.5 {
        layers.push(Layer::all(rock_surface).height(hi[1] - top * (rock - 0.5) * 1.6, f32::MAX));
    }
    layers.push(Layer::all(grass));
    cover(m, &layers, 0.08, seed)
}

/// The landform mix presets every asset with `rock` and `snow` params
/// offers: (id, name, rock, snow).
pub const MIX_PRESETS: [(&str, &str, f32, f32); 3] = [("rocky", "Rocky", 0.85, 0.0), ("grassy", "Grassy", 0.15, 0.0), ("snow-capped", "Snow-capped", 0.55, 0.7)];

/// Rock outcrops on a terrain mesh `m` (round 9: the hills' reference shows
/// pale bedrock breaking through the turf): `clusters` groups of three to
/// six weathered rocks on the steepest ground away from the edges, the
/// largest `size` m across, each a flattish slab tilted with the slope and
/// sunk about a third into it, so it reads as rock showing through the
/// ground, not stones set on top. One mesh; `density` (the tier's) picks the
/// rocks' subdivision. Deterministic.
pub fn outcrops(m: &Mesh, clusters: u32, size: f32, density: f32, seed: u32) -> Mesh {
    use crate::shapes as sh;
    let mut out = Mesh::default();
    if clusters == 0 || m.indices.is_empty() || size <= 0.0 {
        return out;
    }
    let (lo, hi) = crate::geo::bounds(m);
    let span = [hi[0] - lo[0], hi[2] - lo[2]];
    // Candidate faces: steep, off the border, ranked by steepness.
    let mut cand: Vec<([f32; 3], f32, [f32; 3])> = Vec::new();
    for t in m.indices.as_chunks::<3>().0 {
        let p: [[f32; 3]; 3] = std::array::from_fn(|i| {
            let v = m.vertices[t[i] as usize];
            [v[0], v[1], v[2]]
        });
        let c: [f32; 3] = std::array::from_fn(|k| (p[0][k] + p[1][k] + p[2][k]) / 3.0);
        let e1 = [p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]];
        let e2 = [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]];
        let mut n = crate::geo::normalize([e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]]);
        if n[1] < 0.0 {
            n = [-n[0], -n[1], -n[2]];
        }
        let inside = (c[0] - lo[0]).min(hi[0] - c[0]) > span[0] * 0.12 && (c[2] - lo[2]).min(hi[2] - c[2]) > span[1] * 0.12;
        if inside && n[1] < 0.97 {
            cand.push((c, 1.0 - n[1], n));
        }
    }
    cand.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut centres: Vec<([f32; 3], [f32; 3])> = Vec::new();
    for (c, _, n) in &cand {
        if centres.len() as u32 >= clusters {
            break;
        }
        if centres.iter().all(|(o, _)| (o[0] - c[0]).powi(2) + (o[2] - c[2]).powi(2) > (size * 5.0).powi(2)) {
            centres.push((*c, *n));
        }
    }
    // The ground height nearest (x, z): the closest vertex's.
    let ground = |x: f32, z: f32| -> f32 { m.vertices.iter().min_by(|a, b| ((a[0] - x).powi(2) + (a[2] - z).powi(2)).total_cmp(&((b[0] - x).powi(2) + (b[2] - z).powi(2)))).map_or(0.0, |v| v[1]) };
    for (i, (c, n)) in centres.iter().enumerate() {
        let k = 3 + (sh::rnd(seed, i as u32, 61) * 4.0) as u32;
        for j in 0..k {
            let id = i as u32 * 16 + j;
            let s = size * (0.35 + 0.65 * sh::rnd(seed, id, 62)) * if j == 0 { 1.0 } else { 0.75 };
            let (dx, dz) = if j == 0 { (0.0, 0.0) } else { ((sh::rnd(seed, id, 63) - 0.5) * size * 3.0, (sh::rnd(seed, id, 64) - 0.5) * size * 3.0) };
            let (x, z) = (c[0] + dx, c[2] + dz);
            let dims = [s, s * (0.4 + 0.25 * sh::rnd(seed, id, 65)), s * (0.6 + 0.4 * sh::rnd(seed, id, 66))];
            let mut r = sh::rock(dims, seed ^ id.wrapping_mul(0x9e37_79b9), 0.9, density < 0.5);
            sh::rotate(&mut r, [0.0, 1.0, 0.0], sh::rnd(seed, id, 67) * std::f32::consts::TAU);
            // Tilt with the slope (most of the way).
            let axis = crate::geo::normalize([n[2], 0.0, -n[0]]);
            sh::rotate(&mut r, axis, n[1].clamp(-1.0, 1.0).acos() * 0.8);
            let y = ground(x, z) - dims[1] * 0.5;
            crate::geo::transform(&mut r, [1.0; 3], [x, y, z]);
            crate::geo::merge(&mut out, &r);
        }
    }
    out
}
