//! Per-style shape treatments: a style's `render` data (the kit's
//! `styles.json`, [`Render`]) built into each shape's model, so the shapes
//! look like the style (toon bands, ink outlines, voxels, low-poly facets...)
//! and can be compared with its reference pictures. No engine pass and no
//! shader: every treatment is geometry plus flat colours, written as a model
//! of several parts by [`shapes::glb_parts`].
//!
//! - [`geometry`] reshapes a mesh (clay lumps, knit ridges, the bump slider,
//!   low-poly clustering, voxels, flat facets); the scene caches it per
//!   shape, style and bump.
//! - [`shade`] cuts it along light bands (or into a two-tone dither) from a
//!   key light, the scene's sun turned toward the home camera
//!   ([`key_light`]), and adds an inside-out outline shell: one part per
//!   flat colour.
//!
//! A flat colour is a material with a black base and the colour as its
//! glow, which the engine's model shader draws as that colour. The outline
//! shell is single-sided with its winding flipped, so the shader drops the
//! half that faces the camera and only a rim round the silhouette shows.

use std::collections::{BTreeMap, HashMap, HashSet};

use evani_kit_material_view::styles::{Finish, Render};

use crate::shapes::{self, Mesh, Pbr, add, cross, dot, normalize, scale, sub, value_noise};

/// One part of a treated model.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub mesh: Mesh,
    pub pbr: Pbr,
    /// Both faces draw (low-poly clustering can flip a few triangles).
    pub double_sided: bool,
    /// The shape's own surface (a band, a lit patch, ink or paper), as
    /// opposed to a line drawn over it (the outline, neon trim): a catalogue
    /// material's pattern goes on the surface only.
    pub body: bool,
}

/// The engine's default sun direction (toward the light).
pub const SUN: [f32; 3] = [0.45, 0.8, 0.4];

/// The light the bands and the dither are cut from: the sun, turned toward
/// the view from the scene's home camera (`toward_eye`, target to eye) only
/// as far as needed to sit within `KEY_MAX_ANGLE` of it. Toon and cel
/// pictures light the side facing the viewer and keep a narrow shadow side;
/// a sun behind the shapes (Sunset, Night lamp) would light the side the
/// camera can't see. Decision: a limit, not a fixed halfway blend, because
/// halfway put the Studio sun (44 degrees from the home view) so close to
/// the camera that the cel shadow shrank to a sliver under the outline. It
/// stays fixed in the world (the home view, not the turning camera), so a
/// shape is built once, not every frame of a spin, and the floor shadows
/// still follow the real sun (a small mismatch, kept).
///
/// `side_light` (degrees, the style's `render.side_light`) is the least
/// angle from the view: a two-tone style (cel, the dither) turns the light
/// further round, on the sun's side (or from above when the sun sits on the
/// view line), so each shape shows a real shadow side rather than a sliver
/// at its rim. 0 keeps the sun as it is.
pub fn key_light(sun: [f32; 3], toward_eye: [f32; 3], side_light: f32) -> [f32; 3] {
    let s = normalize(sun, SUN);
    let e = normalize(toward_eye, s);
    let c = dot(s, e).clamp(-1.0, 1.0);
    let least = if side_light.is_finite() { side_light.to_radians().clamp(0.0, KEY_SIDE_MAX) } else { 0.0 };
    let most = KEY_MAX_ANGLE.max(least);
    let angle = c.acos();
    let want = angle.clamp(least, most);
    if (want - angle).abs() < 1e-5 {
        return s;
    }
    let perp = sub(s, scale(e, c));
    let side = if dot(perp, perp) >= 1e-8 {
        normalize(perp, s)
    } else if c > 0.0 {
        // The sun on the view line: light from above instead.
        normalize(sub([0.0, 1.0, 0.0], scale(e, e[1])), [1.0, 0.0, 0.0])
    } else {
        return s;
    };
    add(scale(e, want.cos()), scale(side, want.sin()))
}
/// The most a style's side light may turn the light from the view
/// (radians, 80 degrees): further and the lit side would shrink to a rim.
const KEY_SIDE_MAX: f32 = 1.396;
/// How far the key light may sit from the home view (radians, 50 degrees):
/// the shadow side then shows as a crescent about a quarter of a sphere's
/// radius wide, and the Studio sun stays as it is.
const KEY_MAX_ANGLE: f32 = 0.873;
/// The most triangles subdividing may make.
const MAX_TRIS: usize = 40_000;
/// Mean edge the knit ridges need (their rows are about 0.17 apart).
const RIDGE_EDGE: f32 = 0.035;
/// Mean edge for clay lumps.
const LUMP_EDGE: f32 = 0.07;
/// Mean edge for the dither's triangle "dots".
const DITHER_EDGE: f32 = 0.06;
/// The dither's ordered-pattern cell (shape units).
pub const DITHER_CELL: f32 = 0.045;
/// Pigment patches: noise frequency (patches about 0.35 across).
const MOTTLE_FREQ: f32 = 2.9;
/// Light bands: the shadow edge (light, -1 to 1), a little below side-on so
/// the lit side is a little more than half, and where the top band starts
/// at most.
const TERMINATOR: f32 = -0.1;
const BAND_TOP: f32 = 0.75;
/// The dither's light in shadow (0 to 1) for a white material, so a pale
/// material still shows some paper on its dark side (falls off with the
/// square of the material's tone, so a dark one is solid ink there).
const DITHER_AMBIENT: f32 = 0.22;
/// The dither's light ramp (light, -1 to 1): all shadow below the first,
/// the material's full paper share above the second. Decision: a steep ramp,
/// as the old soft one gave every ball the same mid-grey speckle.
const DITHER_DARK: f32 = -0.05;
const DITHER_LIGHT: f32 = 0.5;
/// Voxel face shading by direction (up, front or back, sides, down), so the
/// cubes read as cubes under any light: each face a step darker, as in
/// voxel games, with the material's own tint kept on top.
const VOXEL_FACE_TONE: [f32; 4] = [1.0, 0.74, 0.55, 0.38];
/// Neon trim rings: spacing up the body, the bright core's half width, and
/// lift off it (shape units, radius 1); sparse and thin, so the body reads
/// as dark glass with lit seams, not a striped ball.
const NEON_GAP: f32 = 0.5;
const NEON_HALF: f32 = 0.008;
const NEON_LIFT: f32 = 0.003;
/// The glow round neon lines: how many times wider than the line, and its
/// strength against the line's.
const NEON_HALO: f32 = 3.5;
const NEON_HALO_LIGHT: f32 = 0.32;
/// A highlight's size (light, 0 to 1, below full): the least, and how much
/// a rougher surface widens it. A pea-sized shine, not a stuck-on disc.
const HIGHLIGHT_MIN: f32 = 0.006;
const HIGHLIGHT_ROUGH: f32 = 0.02;
/// Strengths that push the highlight, the dither's paper and neon's lines
/// to full strength (pure white, full cyan). The model's glow tops out at
/// 1, and the scene's tone curve shows that as about 0.6 (205 on screen,
/// the backdrop's grey: measured), so whites can't go brighter than the
/// backdrop from here; that is the scene's exposure, not the shapes.
const HIGHLIGHT_BOOST: f32 = 1.6;
const PAPER_BOOST: f32 = 1.3;
const NEON_LINE_BOOST: f32 = 1.6;
/// Each voxel cube's own tone (see [`voxel_jitter`]).
const VOXEL_CUBE_TONE: [f32; 4] = [0.84, 0.94, 1.04, 1.14];
/// Neon outlines take the material's own hue only when it has this much
/// colour (max - min channel); greyer materials use the style's line colour.
const NEON_HUE: f32 = 0.2;
/// A 4 x 4 ordered-dither (Bayer) matrix.
const BAYER: [u8; 16] = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];
/// For each axis, the two others in the order that makes it their cross.
const AXES: [(usize, usize); 3] = [(1, 2), (2, 0), (0, 1)];

// ------------------------------------------------------------- geometry

/// The treated shape: `base` reshaped by the style, before any colour.
/// Steps, in order: lumps or knit ridges (pushed along the normals), the
/// bump slider (not on voxels, bands or the dither), low-poly clustering,
/// voxels, flat facets, then finer triangles for the dither.
///
/// Decision: bands and the dither are flat colour cut by the light, so a
/// bumped normal changes nothing but their edges, which it frays (ragged
/// cel shadows, a blotchy toon highlight); they keep the smooth normal.
pub fn geometry(base: &Mesh, r: &Render, bump: f32) -> Mesh {
    let mut m = with_normals(base);
    if r.lumpy > 0.0 || r.ridges > 0.0 {
        m = displaced(&m, r);
    }
    let cut_by_light = r.bands > 0 || r.dither;
    if bump != 0.0 && r.voxel <= 0.0 && !cut_by_light {
        m = shapes::bumped(&m, bump);
    }
    if r.cluster > 0.0 {
        m = clustered(&m, r.cluster);
    }
    if r.voxel > 0.0 {
        m = voxels(&m, r.voxel);
    }
    if r.facets {
        m = faceted(&m);
    }
    if r.dither {
        m = refined(m, DITHER_EDGE);
    }
    m
}

/// The triangles whose three indices are in range.
fn tris(m: &Mesh) -> impl Iterator<Item = [usize; 3]> + '_ {
    let n = m.positions.len();
    m.indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|t| [t[0] as usize, t[1] as usize, t[2] as usize])
        .filter(move |t| t.iter().all(|&i| i < n))
}

fn normal_at(m: &Mesh, i: usize) -> [f32; 3] {
    m.normals.get(i).copied().unwrap_or([0.0, 1.0, 0.0])
}

/// A triangle's normal times twice its area.
fn area_normal(m: &Mesh, t: [usize; 3]) -> [f32; 3] {
    let [a, b, c] = t.map(|i| m.positions[i]);
    cross(sub(b, a), sub(c, a))
}

fn centroid(m: &Mesh, t: [usize; 3]) -> [f32; 3] {
    let [a, b, c] = t.map(|i| m.positions[i]);
    scale(add(add(a, b), c), 1.0 / 3.0)
}

/// A position as a welding key (to 1e-5 of a unit).
fn weld_key(p: [f32; 3]) -> [i32; 3] {
    p.map(|x| (x * 1e5).round() as i32)
}

/// A copy whose normals match its positions (missing ones from the faces).
fn with_normals(m: &Mesh) -> Mesh {
    let mut out = m.clone();
    if out.normals.len() != out.positions.len() {
        out.normals = welded_normals(&out);
    }
    out
}

/// Each vertex's normal as the area-weighted mean of every face at its
/// position, so split corners (a cube's edges) share one direction.
fn welded_normals(m: &Mesh) -> Vec<[f32; 3]> {
    let mut sum: HashMap<[i32; 3], [f32; 3]> = HashMap::new();
    for t in tris(m) {
        let n = area_normal(m, t);
        for i in t {
            let e = sum.entry(weld_key(m.positions[i])).or_insert([0.0; 3]);
            *e = add(*e, n);
        }
    }
    m.positions
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let own = normal_at(m, i);
            sum.get(&weld_key(*p)).map_or(own, |&s| normalize(s, own))
        })
        .collect()
}

/// Normals again from the faces at each position, keeping hard edges: a
/// face counts for a corner only if it roughly agrees with its old normal.
fn renormal(m: &mut Mesh) {
    let mut faces: HashMap<[i32; 3], Vec<[f32; 3]>> = HashMap::new();
    for t in tris(m) {
        let n = area_normal(m, t);
        for i in t {
            faces.entry(weld_key(m.positions[i])).or_default().push(n);
        }
    }
    let normals = m
        .positions
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let old = normal_at(m, i);
            let mut s = [0.0; 3];
            for &n in faces.get(&weld_key(*p)).map_or(&[][..], Vec::as_slice) {
                if dot(normalize(n, old), old) > 0.5 {
                    s = add(s, n);
                }
            }
            normalize(s, old)
        })
        .collect();
    m.normals = normals;
}

/// The mean edge length.
fn mean_edge(m: &Mesh) -> f32 {
    let (mut sum, mut n) = (0.0f64, 0u32);
    for [a, b, c] in tris(m) {
        for (i, j) in [(a, b), (b, c), (c, a)] {
            let d = sub(m.positions[i], m.positions[j]);
            sum += f64::from(dot(d, d).sqrt());
            n += 1;
        }
    }
    if n == 0 { 0.0 } else { (sum / f64::from(n)) as f32 }
}

/// Splits every triangle in four until the mean edge is at most `edge` (or
/// the next split would pass [`MAX_TRIS`]).
fn refined(mut m: Mesh, edge: f32) -> Mesh {
    while mean_edge(&m) > edge && m.indices.len() / 3 * 4 <= MAX_TRIS {
        m = subdivided(&m);
    }
    m
}

/// Every triangle split in four at its edges' midpoints (shared, so no
/// cracks; split corners at one position get the same midpoints too).
fn subdivided(m: &Mesh) -> Mesh {
    let mut out = with_normals(m);
    out.indices = Vec::with_capacity(m.indices.len() * 4);
    let mut mids: HashMap<(usize, usize), u32> = HashMap::new();
    let mut mid = |out: &mut Mesh, a: usize, b: usize| -> u32 {
        let (a, b) = (a.min(b), a.max(b));
        *mids.entry((a, b)).or_insert_with(|| {
            let p = scale(add(out.positions[a], out.positions[b]), 0.5);
            let n = normalize(add(out.normals[a], out.normals[b]), out.normals[a]);
            out.positions.push(p);
            out.normals.push(n);
            (out.positions.len() - 1) as u32
        })
    };
    for [a, b, c] in tris(m) {
        let (ab, bc, ca) = (mid(&mut out, a, b), mid(&mut out, b, c), mid(&mut out, c, a));
        let (a, b, c) = (a as u32, b as u32, c as u32);
        out.indices.extend([a, ab, ca, ab, b, bc, ca, bc, c, ab, bc, ca]);
    }
    out
}

/// Clay lumps or knit ridges: each vertex pushed along its welded normal by
/// a field of its position (so split corners move together).
fn displaced(m: &Mesh, r: &Render) -> Mesh {
    let edge = if r.ridges > 0.0 { RIDGE_EDGE } else { LUMP_EDGE };
    let mut m = refined(m.clone(), edge);
    let dirs = welded_normals(&m);
    for (p, d) in m.positions.iter_mut().zip(&dirs) {
        let h = r.lumpy * lump(*p) + r.ridges * ridge(*p);
        *p = add(*p, scale(*d, h));
    }
    renormal(&mut m);
    m
}

/// Clay lumps: two octaves of smooth noise (about -1.5 to 1.5).
fn lump(p: [f32; 3]) -> f32 {
    value_noise(scale(p, 2.2)) + 0.5 * value_noise(add(scale(p, 4.7), [5.3, 1.7, 8.9]))
}

/// Knit rows: rounded ridges round the shape, gently wavering (0 to 1).
fn ridge(p: [f32; 3]) -> f32 {
    (p[1] * 18.0 + 1.5 * value_noise(scale(p, 3.0))).sin().abs()
}

/// Low-poly: every vertex snaps to the mean of its grid cell (cells `c`
/// wide, centred on the origin's grid); collapsed and repeated triangles go,
/// and each kept one faces the way its corners' old normals point.
fn clustered(m: &Mesh, c: f32) -> Mesh {
    let mut cell_of: HashMap<[i32; 3], u32> = HashMap::new();
    let mut sums: Vec<([f32; 3], [f32; 3], f32)> = Vec::new();
    let mut remap = Vec::with_capacity(m.positions.len());
    for (i, p) in m.positions.iter().enumerate() {
        let key = p.map(|x| (x / c + 0.5).floor() as i32);
        let j = *cell_of.entry(key).or_insert_with(|| {
            sums.push(([0.0; 3], [0.0; 3], 0.0));
            (sums.len() - 1) as u32
        });
        if let Some(s) = sums.get_mut(j as usize) {
            s.0 = add(s.0, *p);
            s.1 = add(s.1, normal_at(m, i));
            s.2 += 1.0;
        }
        remap.push(j);
    }
    let positions: Vec<[f32; 3]> = sums.iter().map(|(p, _, n)| scale(*p, 1.0 / n)).collect();
    let normals: Vec<[f32; 3]> = sums.iter().map(|(_, n, _)| normalize(*n, [0.0, 1.0, 0.0])).collect();
    let mut seen = HashSet::new();
    let mut indices = Vec::new();
    for t in tris(m) {
        let [a, b, k] = t.map(|i| remap[i]);
        if a == b || b == k || a == k {
            continue;
        }
        let mut sorted = [a, b, k];
        sorted.sort_unstable();
        if !seen.insert(sorted) {
            continue;
        }
        let [pa, pb, pk] = [a, b, k].map(|i| positions[i as usize]);
        let n = cross(sub(pb, pa), sub(pk, pa));
        if dot(n, n) < 1e-12 {
            continue;
        }
        let along = add(add(normals[a as usize], normals[b as usize]), normals[k as usize]);
        if dot(n, along) < 0.0 {
            indices.extend([a, k, b]);
        } else {
            indices.extend([a, b, k]);
        }
    }
    Mesh { positions, normals, indices }
}

/// Voxels: the cubes (`v` wide, centred on the origin's grid) the surface
/// passes through, filled inside, drawn as their outward faces only.
fn voxels(m: &Mesh, v: f32) -> Mesh {
    let cell = |p: [f32; 3]| p.map(|x| (x / v + 0.5).floor() as i32);
    let mut hit: Vec<[i32; 3]> = Vec::new();
    for t in tris(m) {
        let [a, b, c] = t.map(|i| m.positions[i]);
        let longest = [sub(b, a), sub(c, b), sub(a, c)].iter().map(|d| dot(*d, *d).sqrt()).fold(0.0, f32::max);
        // Samples at most half a cube apart, so no cube the surface crosses is missed.
        let steps = (longest / (0.5 * v)).ceil().clamp(1.0, 64.0) as usize;
        for i in 0..=steps {
            for j in 0..=steps - i {
                let (u, w) = (i as f32 / steps as f32, j as f32 / steps as f32);
                hit.push(cell(add(a, add(scale(sub(b, a), u), scale(sub(c, a), w)))));
            }
        }
    }
    let Some(first) = hit.first().copied() else { return Mesh::default() };
    // The box round the hit cubes, with one empty cube of padding all round.
    let (mut lo, mut hi) = (first, first);
    for c in &hit {
        lo = [0, 1, 2].map(|k| lo[k].min(c[k] - 1));
        hi = [0, 1, 2].map(|k| hi[k].max(c[k] + 1));
    }
    let dims = [0, 1, 2].map(|k| (hi[k] - lo[k] + 1) as usize);
    if dims.iter().product::<usize>() > 8_000_000 {
        return m.clone();
    }
    let at = |c: [i32; 3]| -> Option<usize> {
        let d = [c[0] - lo[0], c[1] - lo[1], c[2] - lo[2]];
        if (0..3).any(|k| d[k] < 0 || d[k] as usize >= dims[k]) {
            return None;
        }
        Some(d[0] as usize + dims[0] * (d[1] as usize + dims[1] * d[2] as usize))
    };
    // 0 inside (or not yet reached), 1 surface, 2 outside.
    let mut grid = vec![0u8; dims.iter().product()];
    for c in &hit {
        if let Some(i) = at(*c) {
            grid[i] = 1;
        }
    }
    // Flood the outside in from a corner (the padding is all empty).
    let steps6 = [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]];
    let mut stack = vec![lo];
    if let Some(i) = at(lo) {
        grid[i] = 2;
    }
    while let Some(c) = stack.pop() {
        for s in steps6 {
            let n = [c[0] + s[0], c[1] + s[1], c[2] + s[2]];
            if let Some(i) = at(n)
                && grid[i] == 0
            {
                grid[i] = 2;
                stack.push(n);
            }
        }
    }
    let mut out = Mesh::default();
    for z in lo[2]..=hi[2] {
        for y in lo[1]..=hi[1] {
            for x in lo[0]..=hi[0] {
                let c = [x, y, z];
                if at(c).is_none_or(|i| grid[i] == 2) {
                    continue;
                }
                for axis in 0..3 {
                    for sign in [1, -1] {
                        let mut n = c;
                        n[axis] += sign;
                        if at(n).is_none_or(|i| grid[i] == 2) {
                            quad(&mut out, c, v, axis, sign as f32);
                        }
                    }
                }
            }
        }
    }
    out
}

/// One cube face, wound to face along `axis` times `sign`.
fn quad(out: &mut Mesh, c: [i32; 3], v: f32, axis: usize, sign: f32) {
    let (u, w) = AXES[axis];
    let centre = c.map(|k| k as f32 * v);
    let corner = |du: f32, dw: f32| {
        let mut p = centre;
        p[axis] += 0.5 * v * sign;
        p[u] += 0.5 * v * du;
        p[w] += 0.5 * v * dw;
        p
    };
    let mut n = [0.0; 3];
    n[axis] = sign;
    let base = out.positions.len() as u32;
    out.positions.extend([corner(-1.0, -1.0), corner(1.0, -1.0), corner(1.0, 1.0), corner(-1.0, 1.0)]);
    out.normals.extend([n; 4]);
    let order = if sign > 0.0 { [0, 1, 2, 0, 2, 3] } else { [0, 2, 1, 0, 3, 2] };
    out.indices.extend(order.map(|k| base + k));
}

/// Flat faces: every triangle on its own corners with its face normal
/// (turned to agree with its corners' normals).
fn faceted(m: &Mesh) -> Mesh {
    let mut out = Mesh::default();
    for t in tris(m) {
        let mut p = t.map(|i| m.positions[i]);
        let along = add(add(normal_at(m, t[0]), normal_at(m, t[1])), normal_at(m, t[2]));
        let mut n = normalize(cross(sub(p[1], p[0]), sub(p[2], p[0])), normalize(along, [0.0, 1.0, 0.0]));
        if dot(n, along) < 0.0 {
            p.swap(1, 2);
            n = scale(n, -1.0);
        }
        let base = out.positions.len() as u32;
        out.positions.extend(p);
        out.normals.extend([n; 3]);
        out.indices.extend([base, base + 1, base + 2]);
    }
    out
}

// --------------------------------------------------------------- colour

/// The treated model's parts: `geo` coloured by the style for this finish
/// under a sun toward `light`. Bands 0 keeps the engine's lighting (lit
/// parts, one per pigment patch); 1 or more cuts flat colour bands; the
/// dither draws ink and paper; an outline adds its shell last.
pub fn shade(geo: &Mesh, r: &Render, f: &Finish, light: [f32; 3]) -> Vec<Part> {
    let l = normalize(light, normalize(SUN, [0.0, 1.0, 0.0]));
    let mut parts = if r.dither {
        dithered(geo, r, f, l)
    } else if r.bands == 0 {
        lit(geo, r, f)
    } else {
        banded(geo, r, f, l)
    };
    let double_sided = r.cluster > 0.0;
    for p in &mut parts {
        p.double_sided = double_sided;
    }
    if r.outline > 0.0 {
        parts.push(outline(geo, r, f));
    }
    if r.outline_glow {
        // Glow without a bloom pass: a dim, wider band under each bright
        // line and round the outline, as a glow spreads on a dark body.
        // (A wider dim shell round the outline read as a glass bubble.)
        let c = line_colour(r, f);
        parts.push(neon_lines(geo, scale(c, NEON_HALO_LIGHT), NEON_HALF * NEON_HALO, NEON_LIFT));
        parts.push(neon_lines(geo, scale(c, NEON_LINE_BOOST), NEON_HALF, 2.0 * NEON_LIFT));
    }
    parts.retain(|p| !p.mesh.indices.is_empty());
    parts
}

/// A corner of a part: a source vertex, or where an edge (lower index
/// first) crosses band edge `k`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Corner {
    Src(usize),
    Cut(usize, usize, usize),
}

/// A part corner with its position and normal.
type Vertex = (Corner, [f32; 3], [f32; 3]);

/// One part's mesh as it is built (corners shared within the part).
#[derive(Default)]
struct PartMesh {
    mesh: Mesh,
    at: HashMap<Corner, u32>,
}

impl PartMesh {
    fn corner(&mut self, (key, p, n): Vertex) -> u32 {
        let mesh = &mut self.mesh;
        *self.at.entry(key).or_insert_with(|| {
            mesh.positions.push(p);
            mesh.normals.push(n);
            (mesh.positions.len() - 1) as u32
        })
    }

    fn src(geo: &Mesh, i: usize) -> Vertex {
        (Corner::Src(i), geo.positions[i], normal_at(geo, i))
    }

    /// A whole source triangle.
    fn whole(&mut self, geo: &Mesh, t: [usize; 3]) {
        for i in t {
            let k = self.corner(Self::src(geo, i));
            self.mesh.indices.push(k);
        }
    }

    /// A convex polygon (in winding order) as a fan, without slivers.
    fn fan(&mut self, poly: &[Vertex]) {
        let Some(&first) = poly.first() else { return };
        for w in poly[1..].windows(2) {
            let tri = [first, w[0], w[1]];
            let n = cross(sub(tri[1].1, tri[0].1), sub(tri[2].1, tri[0].1));
            if dot(n, n) < 1e-14 {
                continue;
            }
            for c in tri {
                let i = self.corner(c);
                self.mesh.indices.push(i);
            }
        }
    }
}

fn luma(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

fn mul(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] * b[0], a[1] * b[1], a[2] * b[2]]
}

fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn tint(f: &Finish) -> [f32; 3] {
    f.tint.map(|c| if c.is_finite() { c.clamp(0.0, 1.0) } else { 0.0 })
}

/// The style's last colour steps: posterize (brightness levels, in
/// square-root space so the dark steps aren't crushed; the hue is kept, as
/// rounding each channel on its own turns sand yellow and its shadow grey),
/// grey, brightness.
fn graded(c: [f32; 3], r: &Render) -> [f32; 3] {
    let mut c = c.map(|x| x.clamp(0.0, 1.0));
    if r.posterize > 1 {
        let k = (r.posterize - 1) as f32;
        let top = c[0].max(c[1]).max(c[2]);
        if top > 1e-4 {
            let s = ((top.sqrt() * k).round() / k).max(1.0 / k);
            let gain = s * s / top;
            c = c.map(|x| (x * gain).clamp(0.0, 1.0));
        }
    }
    if r.desaturate > 0.0 {
        let (y, d) = (luma(c), r.desaturate.clamp(0.0, 1.0));
        c = c.map(|x| x + (y - x) * d);
    }
    c.map(|x| (x * r.value).clamp(0.0, 1.0))
}

/// Which pigment patch a point is in: 0 thinner (darker, cooler), 1 as
/// mixed, 2 richer (lighter, warmer). Always 1 without mottle.
fn patch(p: [f32; 3], r: &Render) -> usize {
    if r.mottle <= 0.0 {
        return 1;
    }
    let n = value_noise(scale(p, MOTTLE_FREQ));
    if n < -0.2 {
        0
    } else if n > 0.2 {
        2
    } else {
        1
    }
}

fn mottled(c: [f32; 3], patch: usize, r: &Render) -> [f32; 3] {
    let d = (patch as f32 - 1.0) * r.mottle;
    [c[0] * (1.0 + d), c[1] * (1.0 + 0.85 * d), c[2] * (1.0 + 0.6 * d)]
}

/// A flat colour (the finish's glow added on top).
fn flat(c: [f32; 3], f: &Finish) -> Pbr {
    let g = if f.glow.is_finite() { f.glow.clamp(0.0, 1.0) } else { 0.0 };
    emissive(add(c, scale(tint(f), g)))
}

/// A flat colour, unlit: all emissive, clamped to 0 to 1, on a black metal
/// base. Decision: black metal, not black plastic, because plastic keeps a
/// 4% sheen (the sun's highlight, the sky's reflection) that laid soft grey
/// shading under the dither's dots and a pale fuzz in the outlines; a black
/// metal reflects nothing, so the colour reaches the screen flat.
fn emissive(c: [f32; 3]) -> Pbr {
    glowing(c.map(|x| if x.is_finite() { x.clamp(0.0, 1.0) } else { 0.0 }))
}

fn part(mesh: Mesh, pbr: Pbr) -> Part {
    Part { mesh, pbr, double_sided: false, body: true }
}

/// Bands 0: the engine's own light, one lit part per pigment patch (and,
/// for voxels, per face direction: see [`VOXEL_FACE_TONE`]).
fn lit(geo: &Mesh, r: &Render, f: &Finish) -> Vec<Part> {
    let mut parts: BTreeMap<(usize, usize, usize), PartMesh> = BTreeMap::new();
    for t in tris(geo) {
        let (face, cube) = if r.voxel > 0.0 {
            let n = normal_at(geo, t[0]);
            (voxel_face(n), voxel_jitter(sub(centroid(geo, t), scale(n, 0.5 * r.voxel)), r.voxel))
        } else {
            (0, 0)
        };
        parts.entry((patch(centroid(geo, t), r), face, cube)).or_default().whole(geo, t);
    }
    let g = if f.glow.is_finite() { f.glow.clamp(0.0, 1.0) } else { 0.0 };
    parts
        .into_iter()
        .map(|((k, face, cube), m)| {
            let c = graded(mottled(tint(f), k, r), r);
            if r.voxel > 0.0 {
                // Voxels draw each face flat in the material's own tint, a
                // step darker per direction: the engine's light washed the
                // brick tint pale and left the faces nearly one shade.
                // Each cube also takes one of four tones of its own, so a
                // ball seen head-on (nearly all front faces) still shows
                // its cubes, not one flat stepped blob.
                return part(m.mesh, flat(scale(c, VOXEL_FACE_TONE[face] * VOXEL_CUBE_TONE[cube]), f));
            }
            let pbr = Pbr { base: [c[0], c[1], c[2], 1.0], metallic: f.metallic, roughness: f.roughness, emissive: tint(f).map(|x| x * g) };
            part(m.mesh, pbr)
        })
        .collect()
}

/// A cube's slot in [`VOXEL_CUBE_TONE`]: a fixed hash of the cube holding
/// point `p` (cubes `v` wide, on the grid [`voxels`] uses).
fn voxel_jitter(p: [f32; 3], v: f32) -> usize {
    let c = p.map(|x| (x / v + 0.5).floor() as i32 as u32);
    let mut h = c[0].wrapping_mul(73_856_093) ^ c[1].wrapping_mul(19_349_663) ^ c[2].wrapping_mul(83_492_791);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h % 4) as usize
}

/// A voxel face's slot in [`VOXEL_FACE_TONE`] by the way it faces: 0 up,
/// 1 front or back, 2 left or right, 3 down.
fn voxel_face(n: [f32; 3]) -> usize {
    let a = n.map(f32::abs);
    if a[1] >= a[0] && a[1] >= a[2] {
        if n[1] >= 0.0 { 0 } else { 3 }
    } else if a[2] >= a[0] {
        1
    } else {
        2
    }
}

/// The band edges (in light, -1 to 1) and each band's colour, darkest
/// first, with the highlight as the last band when there is one.
///
/// `bands` steps from `shadow` (deeper on metal) to full colour, the shadow
/// hue strongest in the darkest; the shadow edge sits at `TERMINATOR`, so
/// the lit side is a little more than half whatever the band count, and
/// the other edges are spread evenly from there to `BAND_TOP`. Shine (low
/// roughness) brings a highlight even where the style has none, smaller and
/// sharper the shinier.
///
/// Also returns each band's light level (0 to 1), which scales a
/// glowing material's glow: added at full strength on every band, the glow
/// clipped a bright material's bands to one colour (Rune light showed no
/// shadow band at all).
fn band_levels(r: &Render, f: &Finish) -> (Vec<f32>, Vec<[f32; 3]>, Vec<f32>) {
    let n = r.bands.clamp(1, 6) as usize;
    let metal = f.metallic.clamp(0.0, 1.0);
    let tint = tint(f);
    let shadow = (r.shadow * (1.0 - 0.5 * metal)).clamp(0.0, 1.0);
    let step = if n > 1 { (BAND_TOP - TERMINATOR) / (n - 1) as f32 } else { 0.0 };
    let mut edges: Vec<f32> = (1..n).map(|k| TERMINATOR + (k - 1) as f32 * step).collect();
    let mut levels: Vec<f32> = (0..n).map(|k| if n == 1 { 1.0 } else { shadow + (1.0 - shadow) * k as f32 / (n - 1) as f32 }).collect();
    let mut colours: Vec<[f32; 3]> = (0..n)
        .map(|k| {
            if n == 1 {
                return tint;
            }
            let t = k as f32 / (n - 1) as f32;
            mul(scale(tint, levels[k]), mix3(r.shadow_hue, [1.0; 3], t))
        })
        .collect();
    let rough = f.roughness.clamp(0.0, 1.0);
    // Neon's dark glass shows its glowing lines, not a pale specular disc.
    let shine = if r.outline_glow { 0.0 } else { r.highlight.max(2.0 * (0.35 - rough)).clamp(0.0, 1.0) };
    if n >= 2 && shine > 0.0 {
        edges.push(1.0 - (HIGHLIGHT_MIN + HIGHLIGHT_ROUGH * (1.0 - rough)));
        // Metal keeps half its tint in the highlight: its top band is
        // already the full tint, so a fully tinted highlight would vanish.
        colours.push(mix3(colours[n - 1], mix3([1.0; 3], tint, 0.5 * metal), shine));
        levels.push(1.0);
    }
    (edges, colours, levels)
}

/// Flat light bands: each triangle cut along the band edges of its
/// corners' light, one flat part per band and pigment patch.
fn banded(geo: &Mesh, r: &Render, f: &Finish, l: [f32; 3]) -> Vec<Part> {
    let (edges, colours, levels) = band_levels(r, f);
    let glow = if f.glow.is_finite() { f.glow.clamp(0.0, 1.0) } else { 0.0 };
    let shine = (colours.len() > r.bands.clamp(1, 6) as usize).then(|| colours.len() - 1);
    let light: Vec<f32> = (0..geo.positions.len()).map(|i| dot(normal_at(geo, i), l)).collect();
    let band_of = |s: f32| edges.iter().filter(|&&e| s >= e).count();
    let mut parts: BTreeMap<(usize, usize), PartMesh> = BTreeMap::new();
    for t in tris(geo) {
        let p = patch(centroid(geo, t), r);
        let s = t.map(|i| light[i]);
        let (lo, hi) = (s[0].min(s[1]).min(s[2]), s[0].max(s[1]).max(s[2]));
        let (first, last) = (band_of(lo), band_of(hi));
        if first == last {
            parts.entry((first, p)).or_default().whole(geo, t);
            continue;
        }
        for k in first..=last {
            let poly = clip(geo, t, &light, &edges, k);
            parts.entry((k, p)).or_default().fan(&poly);
        }
    }
    parts
        .into_iter()
        .map(|((k, p), m)| {
            let c = graded(mottled(colours[k], p, r), r);
            let g = glow * levels.get(k).copied().unwrap_or(1.0);
            let pbr = emissive(add(c, scale(tint(f), g)));
            // The highlight at full strength (see `HIGHLIGHT_BOOST`).
            part(m.mesh, if shine == Some(k) { glowing(scale(pbr.emissive, HIGHLIGHT_BOOST)) } else { pbr })
        })
        .collect()
}

/// Triangle `t` cut to band `k` (closed at both edges): its corners in the
/// band and where its edges cross the band's edges, in winding order.
fn clip(geo: &Mesh, t: [usize; 3], light: &[f32], edges: &[f32], k: usize) -> Vec<Vertex> {
    let lo = if k == 0 { None } else { edges.get(k - 1).map(|&e| (e, k - 1)) };
    let hi = edges.get(k).map(|&e| (e, k));
    let inside = |s: f32| lo.is_none_or(|(e, _)| s >= e) && hi.is_none_or(|(e, _)| s <= e);
    let mut poly = Vec::with_capacity(5);
    for (i, j) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
        if inside(light[i]) {
            poly.push(PartMesh::src(geo, i));
        }
        // Crossings strictly inside the edge, in order from i to j.
        let mut cuts = [lo, hi];
        if light[j] < light[i] {
            cuts.reverse();
        }
        for (e, ek) in cuts.into_iter().flatten() {
            if (light[i] - e) * (light[j] - e) < 0.0 {
                poly.push(crossing(geo, light, i, j, e, ek));
            }
        }
    }
    poly
}

/// Where edge i-j crosses light `e`, worked out from the lower index so
/// both triangles on the edge get the same point (no cracks).
fn crossing(geo: &Mesh, light: &[f32], i: usize, j: usize, e: f32, ek: usize) -> Vertex {
    let (a, b) = (i.min(j), i.max(j));
    let u = (e - light[a]) / (light[b] - light[a]);
    let p = add(geo.positions[a], scale(sub(geo.positions[b], geo.positions[a]), u));
    let (na, nb) = (normal_at(geo, a), normal_at(geo, b));
    (Corner::Cut(a, b, ek), p, normalize(add(scale(na, 1.0 - u), scale(nb, u)), na))
}

/// The dither's paper share (0 to 1) at light `s` (-1 to 1) for a material
/// of lightness `tone` (the square root of its luma, 0 to 1): solid ink in
/// shadow (a little paper on a pale material), rising steeply to the
/// material's own share on the lit side (about half for near black, all
/// paper for white), and paper in a shiny material's glint.
fn dither_level(s: f32, tone: f32, glint: Option<f32>) -> f32 {
    if glint.is_some_and(|e| s >= e) {
        return 1.0;
    }
    let tone = tone.clamp(0.0, 1.0);
    let amb = DITHER_AMBIENT * tone * tone;
    let top = (0.45 + 0.65 * tone).min(1.0);
    let d = ((s - DITHER_DARK) / (DITHER_LIGHT - DITHER_DARK)).clamp(0.0, 1.0);
    amb + (top - amb) * d
}

/// Two-tone ordered dither: each triangle is ink or paper by its light on
/// the smooth normal (from the key light, so each shape reads lit on one
/// side and shaded on the other) and the material's own lightness, against
/// a 4 x 4 Bayer pattern laid on the surface's two flattest axes. Metal and
/// shiny materials get a paper glint where the light is strongest.
fn dithered(geo: &Mesh, r: &Render, f: &Finish, l: [f32; 3]) -> Vec<Part> {
    let tone = luma(tint(f)).sqrt();
    let rough = f.roughness.clamp(0.0, 1.0);
    let glint = (f.metallic >= 0.5 || rough < 0.35).then_some(1.0 - (HIGHLIGHT_MIN + HIGHLIGHT_ROUGH * (1.0 - rough)));
    let (mut ink, mut paper) = (PartMesh::default(), PartMesh::default());
    for t in tris(geo) {
        let along = add(add(normal_at(geo, t[0]), normal_at(geo, t[1])), normal_at(geo, t[2]));
        let face = normalize(area_normal(geo, t), normalize(along, [0.0, 1.0, 0.0]));
        let lightness = dither_level(dot(normalize(along, face), l), tone, glint);
        let axis = (0..3).fold(0, |a, k| if face[k].abs() > face[a].abs() { k } else { a });
        let (u, w) = AXES[axis];
        let c = centroid(geo, t);
        let x = ((c[u] / DITHER_CELL).floor() as i32).rem_euclid(4) as usize;
        let y = ((c[w] / DITHER_CELL).floor() as i32).rem_euclid(4) as usize;
        let threshold = (f32::from(BAYER[y * 4 + x]) + 0.5) / 16.0;
        if lightness > threshold { paper.whole(geo, t) } else { ink.whole(geo, t) }
    }
    // Paper at full strength (see `HIGHLIGHT_BOOST`).
    let paper_pbr = glowing(scale(flat(graded(r.paper, r), f).emissive, PAPER_BOOST));
    vec![part(ink.mesh, flat(r.outline_color, f)), part(paper.mesh, paper_pbr)]
}

/// The ink outline: the shape pushed out along its welded normals by the
/// outline width, wound inside out and single-sided, so only the rim round
/// the silhouette (and inner edges, as a monkey's ears) shows. Neon glows
/// at full strength in the material's own hue, or in the style's line
/// colour when the material is too grey to give one (a pale grey rim
/// vanishes against a pale backdrop).
fn outline(geo: &Mesh, r: &Render, f: &Finish) -> Part {
    let dirs = welded_normals(geo);
    let positions = geo.positions.iter().zip(&dirs).map(|(p, d)| add(*p, scale(*d, r.outline))).collect();
    let indices = tris(geo).flat_map(|[a, b, c]| [a as u32, c as u32, b as u32]).collect();
    // The shell is seen from inside (its winding is flipped), so its normals
    // point inward, toward the eye: with outward normals every visible face
    // was lit from behind and caught the sky's full grazing reflection, a
    // pale halo instead of ink (the studio's horse showed it plainly).
    let normals = dirs.iter().map(|d| scale(*d, -1.0)).collect();
    Part { body: false, ..part(Mesh { positions, normals, indices }, glowing(line_colour(r, f))) }
}

/// The outline's colour: the style's, or for neon the material's own hue at
/// full strength when it has one.
fn line_colour(r: &Render, f: &Finish) -> [f32; 3] {
    if !r.outline_glow {
        return r.outline_color;
    }
    let t = tint(f);
    let (top, low) = (t[0].max(t[1]).max(t[2]), t[0].min(t[1]).min(t[2]));
    if top - low >= NEON_HUE { t.map(|x| x / top) } else { r.outline_color }
}

/// An unlit line colour.
fn glowing(c: [f32; 3]) -> Pbr {
    Pbr { base: [0.0, 0.0, 0.0, 1.0], metallic: 1.0, roughness: 1.0, emissive: c }
}

/// Neon trim: thin glowing rings round the body every `NEON_GAP` up its
/// height (lit seams on dark bodies), cut with clean edges from
/// a skin lifted just off the surface so they never fight it for depth.
fn neon_lines(geo: &Mesh, colour: [f32; 3], half: f32, lift: f32) -> Part {
    let dirs = welded_normals(geo);
    let positions = geo.positions.iter().zip(&dirs).map(|(p, d)| add(*p, scale(*d, lift))).collect();
    let skin = Mesh { positions, normals: dirs, indices: geo.indices.clone() };
    let height: Vec<f32> = skin.positions.iter().map(|p| p[1]).collect();
    let (lo, hi) = height.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), &y| (a.min(y), b.max(y)));
    let mut edges = Vec::new();
    if lo <= hi {
        for k in (lo / NEON_GAP).floor() as i32..=(hi / NEON_GAP).ceil() as i32 {
            let c = k as f32 * NEON_GAP;
            edges.extend([c - half, c + half]);
        }
    }
    let band_of = |s: f32| edges.iter().filter(|&&e| s >= e).count();
    let mut out = PartMesh::default();
    for t in tris(&skin) {
        let s = t.map(|i| height[i]);
        let (first, last) = (band_of(s[0].min(s[1]).min(s[2])), band_of(s[0].max(s[1]).max(s[2])));
        // Odd bands lie inside a ring.
        for k in (first..=last).filter(|k| k % 2 == 1) {
            if first == last {
                out.whole(&skin, t);
            } else {
                out.fan(&clip(&skin, t, &height, &edges, k));
            }
        }
    }
    Part { body: false, ..part(out.mesh, glowing(colour)) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shapes::Shape;
    use evani_kit_material_view::styles::{STYLES_JSON, Styles};

    fn sphere() -> Mesh {
        shapes::build(Shape::Sphere).expect("sphere")
    }

    fn finish() -> Finish {
        Finish { tint: [0.8, 0.3, 0.2], roughness: 0.5, metallic: 0.0, bump: 0.0, glow: 0.0 }
    }

    fn area(m: &Mesh) -> f64 {
        tris(m).map(|t| f64::from(dot(area_normal(m, t), area_normal(m, t)).sqrt()) * 0.5).sum()
    }

    /// Six times the signed volume (positive when the faces point out).
    fn volume6(m: &Mesh) -> f64 {
        tris(m)
            .map(|[a, b, c]| f64::from(dot(m.positions[a], cross(m.positions[b], m.positions[c]))))
            .sum()
    }

    fn well_formed(m: &Mesh) -> bool {
        let n = m.positions.len();
        m.normals.len() == n
            && m.indices.len().is_multiple_of(3)
            && m.indices.iter().all(|&i| (i as usize) < n)
            && m.positions.iter().flatten().chain(m.normals.iter().flatten()).all(|x| x.is_finite())
    }

    fn bands(n: u32) -> Render {
        Render { label: "t".into(), bands: n, highlight: 0.7, ..Render::default() }
    }

    /// Colour steps change brightness only: sand stays sand, never olive.
    #[test]
    fn posterize_keeps_the_hue() {
        let r = Render { posterize: 6, ..Render::default() };
        let near = |a: [f32; 3], b: [f32; 3]| (0..3).all(|k| (a[k] - b[k]).abs() < 1e-4);
        for c in [[0.93, 0.81, 0.63], [0.465, 0.405, 0.315], [0.58, 0.45, 0.31]] {
            let g = graded(c, &r);
            assert!(near([g[1] / g[0], g[2] / g[0], 1.0], [c[1] / c[0], c[2] / c[0], 1.0]), "{c:?} -> {g:?}");
        }
        // Few brightness levels: two close shades land on the same step.
        assert!(near(graded([0.5, 0.4, 0.3], &r), graded([0.52, 0.416, 0.312], &r)));
    }

    /// Cutting into light bands keeps every bit of the surface, once.
    #[test]
    fn band_split_keeps_the_area() {
        let geo = sphere();
        for n in [2, 3, 6] {
            let parts = shade(&geo, &bands(n), &finish(), SUN);
            assert!(parts.len() as u32 >= n, "{n} bands: {} parts", parts.len());
            let total: f64 = parts.iter().map(|p| area(&p.mesh)).sum();
            let want = area(&geo);
            assert!((total - want).abs() < want * 1e-3, "{n} bands: {total} vs {want}");
            assert!(parts.iter().all(|p| well_formed(&p.mesh) && !p.double_sided));
        }
    }

    /// The darkest band faces away from the sun, the highlight toward it.
    #[test]
    fn bands_follow_the_sun() {
        let parts = shade(&sphere(), &bands(3), &finish(), SUN);
        let l = normalize(SUN, [0.0, 1.0, 0.0]);
        let mean_light = |p: &Part| {
            let m = &p.mesh;
            m.normals.iter().map(|n| dot(*n, l)).sum::<f32>() / m.normals.len() as f32
        };
        let first = parts.first().map(mean_light).unwrap_or(0.0);
        let last = parts.last().map(mean_light).unwrap_or(0.0);
        assert!(first < -0.3 && last > 0.85, "{first} {last}");
        let glow = |p: &Part| luma(p.pbr.emissive);
        assert!(parts.windows(2).all(|w| glow(&w[0]) < glow(&w[1])), "darkest first");
    }

    /// The key light keeps a sun already near the home view (Studio), turns
    /// one from behind (Night lamp) to the limit on the sun's side, and falls
    /// back to the sun when the two point opposite ways.
    #[test]
    fn key_light_turns_toward_the_camera() {
        let eye = normalize([0.0, 0.21, 0.98], SUN);
        let studio = normalize([0.46, 0.67, 0.59], SUN);
        assert!(dot(key_light(studio, eye, 0.0), studio) > 1.0 - 1e-6);
        let behind = normalize([0.55, 0.53, -0.65], SUN);
        let k = key_light(behind, eye, 0.0);
        assert!((dot(k, k) - 1.0).abs() < 1e-4, "{k:?}");
        assert!((dot(k, eye) - KEY_MAX_ANGLE.cos()).abs() < 1e-4, "{k:?}");
        assert!(dot(cross(eye, k), cross(eye, behind)) > 0.0, "sun's side: {k:?}");
        assert_eq!(key_light([0.0, 1.0, 0.0], [0.0, -1.0, 0.0], 0.0), [0.0, 1.0, 0.0]);
    }

    /// A metal's highlight still shows: lighter than its top band, though
    /// both carry its tint.
    #[test]
    fn metal_highlight_shows() {
        let gold = Finish { tint: [1.0, 0.7, 0.12], roughness: 0.22, metallic: 1.0, bump: 0.0, glow: 0.0 };
        let (_, colours, _) = band_levels(&bands(3), &gold);
        let (top, hi) = (colours[colours.len() - 2], colours[colours.len() - 1]);
        assert!(luma(hi) > luma(top) + 0.05, "{top:?} {hi:?}");
    }

    /// Each triangle's light on its smooth normal.
    fn tri_light(m: &Mesh, l: [f32; 3]) -> Vec<f32> {
        tris(m).map(|t| dot(normalize(add(add(normal_at(m, t[0]), normal_at(m, t[1])), normal_at(m, t[2])), [0.0, 1.0, 0.0]), l)).collect()
    }

    /// The dither reads lit on one side and shaded on the other, and a
    /// paler material shows more paper: (paper share where light > 0.5,
    /// paper share where light < -0.2).
    #[test]
    fn dither_follows_the_light() {
        let r = Render { label: "t".into(), dither: true, paper: [0.86, 0.85, 0.78], outline_color: [0.05, 0.06, 0.08], ..Render::default() };
        let l = key_light(SUN, [0.0, 0.2, 1.0], 0.0);
        let geo = geometry(&sphere(), &r, 0.2);
        let shares = |tint: [f32; 3]| {
            let f = Finish { tint, roughness: 0.85, metallic: 0.0, bump: 0.2, glow: 0.0 };
            let parts = shade(&geo, &r, &f, l);
            let paper = parts.iter().find(|p| p.pbr.emissive[0] > 0.5).map(|p| tri_light(&p.mesh, l)).unwrap_or_default();
            let ink = parts.iter().find(|p| p.pbr.emissive[0] < 0.2).map(|p| tri_light(&p.mesh, l)).unwrap_or_default();
            let share = |keep: &dyn Fn(f32) -> bool| {
                let (p, i) = (paper.iter().filter(|&&s| keep(s)).count(), ink.iter().filter(|&&s| keep(s)).count());
                p as f32 / (p + i).max(1) as f32
            };
            (share(&|s| s > 0.5), share(&|s| s < -0.2))
        };
        let (grey_lit, grey_dark) = shares([0.30, 0.30, 0.28]);
        assert!(grey_lit > 0.6 && grey_dark < 0.15, "{grey_lit} {grey_dark}");
        let (black_lit, black_dark) = shares([0.03, 0.03, 0.03]);
        let (white_lit, _) = shares([0.85, 0.82, 0.72]);
        assert!(black_lit > 0.3 && black_dark < 0.05, "{black_lit} {black_dark}");
        assert!(white_lit > grey_lit + 0.1 && grey_lit > black_lit + 0.1, "{white_lit} {grey_lit} {black_lit}");
    }

    /// Bands and the dither keep the smooth normal under the bump slider,
    /// so their edges stay clean.
    #[test]
    fn bands_ignore_the_bump() {
        let cel = bands(2);
        assert_eq!(geometry(&sphere(), &cel, 0.35).normals, geometry(&sphere(), &cel, 0.0).normals);
        let plain = Render { label: "t".into(), ..Render::default() };
        assert_ne!(geometry(&sphere(), &plain, 0.35).normals, geometry(&sphere(), &plain, 0.0).normals);
    }

    /// A style's side light turns the light that far from the view, so a
    /// real share of the ball the camera sees is in shadow (the plain key
    /// light leaves a sliver at the rim), and 0 keeps the old light.
    #[test]
    fn side_light_shows_a_shadow_side() {
        let eye = normalize([0.0, 0.2, 1.0], [0.0, 0.0, 1.0]);
        let side = key_light(SUN, eye, 75.0);
        assert!((dot(side, eye) - 75f32.to_radians().cos()).abs() < 1e-3, "{side:?}");
        assert_eq!(key_light(SUN, eye, 0.0), key_light(SUN, eye, f32::NAN));
        // The sun on the view line still turns, from above.
        assert!(key_light(eye, eye, 75.0)[1] > 0.5);
        // The share of the ball's disc on screen in the shadow band.
        let right = normalize(cross([0.0, 1.0, 0.0], eye), [1.0, 0.0, 0.0]);
        let up = cross(eye, right);
        let shadow = |l: [f32; 3]| {
            let (mut seen, mut dark) = (0, 0);
            for i in -50..=50 {
                for j in -50..=50 {
                    let (x, y) = (i as f32 / 50.0, j as f32 / 50.0);
                    if x * x + y * y >= 1.0 {
                        continue;
                    }
                    let n = add(add(scale(right, x), scale(up, y)), scale(eye, (1.0 - x * x - y * y).sqrt()));
                    seen += 1;
                    dark += usize::from(dot(n, l) < TERMINATOR);
                }
            }
            dark as f32 / seen as f32
        };
        let (plain, turned) = (shadow(key_light(SUN, eye, 0.0)), shadow(side));
        assert!(plain < 0.15 && turned > 0.25 && turned > 2.0 * plain, "{plain} {turned}");
    }

    /// Voxel cubes draw flat in the material's tint, shaded by face
    /// direction: four tones, the top well over twice the bottom.
    /// The ink shell's normals face inward, toward the eye that sees its
    /// inside: outward ones caught the sky's grazing reflection, a pale halo.
    #[test]
    fn the_outline_shell_faces_the_eye() {
        let r = Render { label: "t".into(), bands: 2, outline: 0.04, ..Render::default() };
        let parts = shade(&sphere(), &r, &finish(), SUN);
        let line = parts.iter().find(|p| !p.body).expect("an outline part");
        for (p, n) in line.mesh.positions.iter().zip(&line.mesh.normals) {
            assert!(p[0] * n[0] + p[1] * n[1] + p[2] * n[2] < 0.0, "{p:?} {n:?}");
        }
    }

    #[test]
    fn voxel_faces_shade_by_direction() {
        let r = Render { label: "t".into(), voxel: 0.12, ..Render::default() };
        let f = Finish { tint: [0.6, 0.3, 0.2], roughness: 0.8, metallic: 0.0, bump: 0.0, glow: 0.0 };
        let parts = shade(&geometry(&sphere(), &r, 0.0), &r, &f, SUN);
        let mut reds: Vec<f32> = parts.iter().map(|p| p.pbr.emissive[0]).collect();
        reds.sort_by(f32::total_cmp);
        reds.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
        // Four face directions times four cube tones.
        assert!(reds.len() >= 12, "{reds:?}");
        assert!(reds[reds.len() - 1] / reds[0] > 3.0, "{reds:?}");
    }

    /// Neon: dark bands with no pale highlight disc, a glowing outline and
    /// glowing rings round the body, every ring corner within its line.
    #[test]
    fn neon_glows_in_lines_without_a_highlight() {
        let r = Render {
            label: "t".into(),
            bands: 2,
            shadow: 0.4,
            value: 0.18,
            outline: 0.045,
            outline_color: [0.15, 0.95, 1.0],
            outline_glow: true,
            ..Render::default()
        };
        let glossy = Finish { tint: [0.1, 0.1, 0.12], roughness: 0.1, metallic: 0.0, bump: 0.0, glow: 0.0 };
        assert_eq!(band_levels(&r, &glossy).1.len(), 2);
        let parts = shade(&geometry(&sphere(), &r, 0.0), &r, &glossy, SUN);
        assert_eq!(parts.iter().filter(|p| p.pbr.emissive == [0.15, 0.95, 1.0]).count(), 1, "the outline");
        let core = scale([0.15, 0.95, 1.0], NEON_LINE_BOOST);
        let Some(rings) = parts.iter().find(|p| p.pbr.emissive == core).map(|p| &p.mesh) else { panic!("no rings") };
        assert!(rings.indices.len() > 30);
        for p in &rings.positions {
            let off = p[1] - (p[1] / NEON_GAP).round() * NEON_GAP;
            assert!(off.abs() <= NEON_HALF + 1e-4, "{p:?}");
        }
        let halo = scale([0.15, 0.95, 1.0], NEON_HALO_LIGHT);
        assert_eq!(parts.iter().filter(|p| p.pbr.emissive == halo).count(), 1, "the rings' glow");
        assert!(parts.iter().filter(|p| ![[0.15, 0.95, 1.0], halo, core].contains(&p.pbr.emissive)).all(|p| luma(p.pbr.emissive) < 0.25));
        // Flat colours reflect nothing: the engine adds no sheen.
        assert!(parts.iter().all(|p| p.pbr.metallic == 1.0 && p.pbr.base[..3] == [0.0; 3]));
    }

    /// A glowing material still shows its shadow band.
    #[test]
    fn a_glowing_material_keeps_its_shadow_band() {
        let r = Render { label: "t".into(), bands: 2, shadow: 0.5, shadow_hue: [0.75, 0.85, 1.3], ..Render::default() };
        let rune = Finish { tint: [0.10, 0.70, 0.95], roughness: 0.30, metallic: 0.0, bump: 0.0, glow: 0.6 };
        let parts = shade(&sphere(), &r, &rune, SUN);
        let lumas: Vec<f32> = parts.iter().map(|p| luma(p.pbr.emissive)).collect();
        let (lo, hi) = lumas.iter().fold((f32::MAX, 0.0f32), |(a, b), &y| (a.min(y), b.max(y)));
        assert!(hi - lo > 0.25, "{lumas:?}");
    }

    /// Voxel faces lie on the cube grid, point out, and close the shape.
    #[test]
    fn voxel_faces_sit_on_the_grid_and_face_out() {
        for (v, shape) in [(0.12, Shape::Sphere), (0.16, Shape::Ring), (0.08, Shape::Cube)] {
            let base = shapes::build(shape).expect("shape");
            let m = voxels(&base, v);
            assert!(well_formed(&m) && !m.indices.is_empty(), "{shape:?}");
            for p in m.positions.iter().flatten() {
                let g = p / v - 0.5;
                assert!((g - g.round()).abs() < 1e-3, "{shape:?}: {p} off the grid");
            }
            for t in tris(&m) {
                let n = normalize(area_normal(&m, t), [0.0; 3]);
                assert!(dot(n, m.normals[t[0]]) > 0.999, "{shape:?}: winding matches the normal");
            }
            // The cubes cover the whole shape, plus at most about a cube layer outside it.
            let (got, want) = (volume6(&m) / 6.0, volume6(&base) / 6.0);
            let most = want + area(&base) * f64::from(v) * 1.8;
            assert!(got > want && got < most, "{shape:?}: volume {got}, shape {want}, most {most}");
        }
    }

    /// The outline shell is the shape grown by the width, wound inside out.
    #[test]
    fn the_outline_hull_is_bigger() {
        let geo = sphere();
        let r = Render { label: "t".into(), outline: 0.05, ..Render::default() };
        let parts = shade(&geo, &r, &finish(), SUN);
        let hull = parts.last().expect("hull");
        assert!(!hull.double_sided && hull.pbr.base == [0.0, 0.0, 0.0, 1.0]);
        let radius = |m: &Mesh| m.positions.iter().map(|p| dot(*p, *p).sqrt()).fold(0.0, f32::max);
        assert!((radius(&hull.mesh) - radius(&geo) - 0.05).abs() < 1e-3);
        assert!(volume6(&hull.mesh) < -volume6(&geo), "inside out and bigger");
    }

    /// Low-poly clustering keeps far fewer corners and every face points out.
    #[test]
    fn low_poly_is_coarse_and_faces_out() {
        let geo = geometry(&sphere(), &Render { label: "t".into(), cluster: 0.32, facets: true, ..Render::default() }, 0.0);
        let tris_n = geo.indices.len() / 3;
        assert!(tris_n > 40 && tris_n < 2_000, "{tris_n}");
        for t in tris(&geo) {
            assert!(dot(normalize(area_normal(&geo, t), [0.0; 3]), centroid(&geo, t)) > 0.0);
        }
    }

    /// Every shipped style builds on every generated shape, with each of
    /// its materials, into sound parts; shaped styles differ from plain.
    #[test]
    fn every_style_builds_on_every_shape() {
        let data = Styles::parse(STYLES_JSON).expect("styles");
        for shape in Shape::ALL {
            let Some(base) = shapes::build(shape) else { continue };
            for style in &data.styles {
                let Some(r) = style.render.as_ref().filter(|r| r.shaped()) else { continue };
                let geo = geometry(&base, r, 0.4);
                assert!(well_formed(&geo) && !geo.indices.is_empty(), "{} on {shape:?}", style.id);
                assert!(geo.indices.len() / 3 <= MAX_TRIS * 2, "{} on {shape:?}: {}", style.id, geo.indices.len() / 3);
                for m in &style.materials {
                    let f = Finish { tint: m.color, roughness: m.roughness, metallic: m.metallic, bump: m.bump, glow: m.glow };
                    let parts = shade(&geo, r, &f, SUN);
                    assert!(!parts.is_empty(), "{} {} on {shape:?}", style.id, m.id);
                    for p in &parts {
                        assert!(well_formed(&p.mesh), "{} {} on {shape:?}", style.id, m.id);
                    }
                    let list: Vec<(&Mesh, &Pbr, bool)> = parts.iter().map(|p| (&p.mesh, &p.pbr, p.double_sided)).collect();
                    let glb = shapes::glb_parts(&list);
                    assert_eq!(&glb[0..4], b"glTF");
                }
            }
        }
    }

    /// Same input, same bytes.
    #[test]
    fn treatments_are_deterministic() {
        let data = Styles::parse(STYLES_JSON).expect("styles");
        let base = shapes::build(Shape::Cube).expect("cube");
        for style in &data.styles {
            let Some(r) = style.render.as_ref().filter(|r| r.shaped()) else { continue };
            let a = shade(&geometry(&base, r, 0.3), r, &finish(), SUN);
            let b = shade(&geometry(&base, r, 0.3), r, &finish(), SUN);
            assert_eq!(a, b, "{}", style.id);
        }
    }
}
