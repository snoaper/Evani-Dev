//! Townhouses (`buildings-townhouses`, Asset Studio column 37, 2026-11-08):
//! a photoreal Georgian and Victorian terrace. A row of houses (five by
//! default) under one long slate roof: red brick, cream ashlar, yellow stock
//! brick, white stucco and rose brick fronts on a grey stone plinth. Each
//! house has a raised ground floor up stone steps with black iron railings,
//! a panelled door under a fanlight in a round arch, two tall six-over-six
//! sash windows with window boxes on the ground floor and three over them;
//! the brick fronts carry rubbed-brick arches (segmental over the ground
//! floor, flat gauged over the first), the ashlar and stucco fronts moulded
//! architraves and cornices. The roof is laid slate by slate in courses, a
//! lead flashing over each party wall, a brick stack with three clay pots on
//! the ridge at each party wall, ridge tiles, a cast-iron gutter on a black
//! fascia and downpipes. Behind the glass are dark rooms and curtains.
//!
//! Everything is real geometry in catalogue materials (red common brick
//! recoloured for each front, limestone, plaster, slate, gloss paint, black
//! iron, brass, window glass, cloth, leafy foliage), so a game that lists the
//! masonry, stone, plaster, paint, metal, glass, cloth and foliage recipes
//! (and the see-through pass for the glass) draws the same terrace the
//! library shows. Chipped arrises on the stone (plinth, steps, sills,
//! ashlar) are notches in the geometry, not paint; each slate is its own
//! tilted plate, so the courses cast real shadow lines.
//!
//! The front faces +z, the first house is at -x. Helpers (`Wall`, `cut`,
//! `course`) adapted from the shop (crate::shop on the column 15 branch) and
//! the office building (crate::office, column 13).

use evani_procedural::surface::presets as sp;
use evani_procedural::{Output, ParamSpec as P, Part, Surface, Values, build as b, geo, shapes as sh};
use std::f32::consts::{FRAC_PI_2, PI};

type Mesh = evani_render::mesh::Mesh;

/// The red brick's main colour (linear): a warm orange-red (round 2: a
/// little brighter and oranger; the darkest units are about half of it).
pub const BRICK: [f32; 3] = [0.24, 0.08, 0.046];

pub const PARAMS: [P; 6] = [
    P::num("count", 5.0, 1.0, 8.0).step(1.0).describe("Houses in the row"),
    P::num("width", 6.4, 5.4, 9.0).unit("m").describe("Each house's frontage (m)"),
    P::num("depth", 9.5, 7.0, 14.0).unit("m").describe("Depth (m)"),
    P::num("floors", 2.0, 1.0, 4.0).step(1.0).describe("Storeys (the raised ground floor and those over it)"),
    P::num("pitch", 33.0, 22.0, 55.0).unit("deg").describe("Roof pitch (degrees)"),
    P::color("color", BRICK).describe("The red brick's colour"),
];

/// Wall thickness (m).
const T: f32 = 0.34;
/// The stone plinth's top (the ground floor's level, the door's threshold)
/// and how far it stands out of the walls.
const PL: f32 = 1.1;
const PL_OUT: f32 = 0.05;
/// Stone joints (m).
const JOINT: f32 = 0.012;
/// The ground floor's windows: sill and head.
const G_SILL: f32 = 2.1;
const G_HEAD: f32 = 4.18;
/// The first floor's windows: sill and head (each storey over it 2.95 m up).
const U_SILL: f32 = 5.68;
const U_HEAD: f32 = 7.5;
const STOREY: f32 = 2.95;
/// The eaves (the walls' top) over two storeys.
const EAVE2: f32 = 8.6;
/// Half a window's opening.
const HW: f32 = 0.5;
/// Half the door's opening (the fanlight's radius) and its arch's springing.
const DOOR_R: f32 = 0.55;
const SPRING: f32 = 3.63;
/// The eaves' overhang.
const OV: f32 = 0.3;
/// The front steps: risers, going, the landing's depth.
const STEPS: u32 = 5;
const RUN: f32 = 0.3;
const LAND: f32 = 0.5;
/// The bays' centres along a house's front (as a share of its width): two
/// windows and the door on the ground floor, three windows over them.
const BAYS: [f32; 3] = [0.211, 0.508, 0.813];
/// Ashlar: course height and block length.
const CH: f32 = 0.36;
const BL: f32 = 0.86;

/// A house's front.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Face {
    Red,
    Stone,
    Yellow,
    Stucco,
    Rose,
}

/// The fronts along the row, repeating.
const ORDER: [Face; 5] = [Face::Red, Face::Stone, Face::Yellow, Face::Stucco, Face::Rose];

impl Face {
    fn wall(self) -> usize {
        match self {
            Face::Red => 0,
            Face::Stone => 1,
            Face::Yellow => 2,
            Face::Stucco => 3,
            Face::Rose => 4,
        }
    }

    fn brick(self) -> bool {
        matches!(self, Face::Red | Face::Yellow | Face::Rose)
    }
}

/// The meshes, one per material.
#[derive(Default)]
struct Kit {
    /// Each front's walling: red brick, ashlar (plain), yellow brick,
    /// stucco, rose brick.
    wall: [Mesh; 5],
    /// Rubbed brick for the arches: orange (red fronts), red (the yellow).
    rubbed: [Mesh; 2],
    /// The arches' mortar behind their joints.
    mortar: Mesh,
    /// Pale stone: the ashlar's blocks, sills, architraves, cornices.
    dress: Mesh,
    /// The stucco front's mouldings.
    trim: Mesh,
    /// The ashlar's and the plinth's joints.
    joint: Mesh,
    /// Grey stone: the plinth, the steps, the planters, the stacks' caps.
    plinth: Mesh,
    slate: [Mesh; 3],
    deck: Mesh,
    ridge: Mesh,
    lead: Mesh,
    stack: Mesh,
    pots: Mesh,
    /// Gloss white paint: the sashes, the door cases, the fanlights.
    white: Mesh,
    door: [Mesh; 5],
    /// Black painted iron: railings, gutter, fascia, pipes, lanterns,
    /// grilles, window boxes.
    black: Mesh,
    brass: Mesh,
    glass: Mesh,
    /// The rooms' dim lining.
    dark: Mesh,
    /// Near-black: soot in the pots, the grilles' backing, the planters'
    /// soil and the box balls' stems.
    soot: Mesh,
    curtain: Mesh,
    leaves: [Mesh; 2],
    flowers: [Mesh; 4],
    lamp: Mesh,
}

fn add(m: &mut Mesh, part: Mesh) {
    geo::merge(m, &part);
}

/// An axis-aligned box over `x`, `y` and `z`.
fn bx(x: [f32; 2], y: [f32; 2], z: [f32; 2]) -> Mesh {
    let lo = [x[0].min(x[1]), y[0].min(y[1]), z[0].min(z[1])];
    let hi = [x[0].max(x[1]), y[0].max(y[1]), z[0].max(z[1])];
    b::cbox([(hi[0] - lo[0]).max(1e-3), (hi[1] - lo[1]).max(1e-3), (hi[2] - lo[2]).max(1e-3)], [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5])
}

/// A polygon (x, y) in the front's plane, from `z[0]` to `z[1]`.
fn slab(poly: &[[f32; 2]], z: [f32; 2]) -> Mesh {
    b::extrude_axes(poly, z[0], z[1], [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]])
}

/// A one-sided pane facing +z over `x` by `y` at `z`.
fn pane(x: [f32; 2], y: [f32; 2], z: f32) -> Mesh {
    let mut m = geo::MeshBuilder::new();
    let n = [0.0, 0.0, 1.0];
    let a = m.vertex([x[0], y[0], z], n);
    let b2 = m.vertex([x[1], y[0], z], n);
    let c = m.vertex([x[1], y[1], z], n);
    let e = m.vertex([x[0], y[1], z], n);
    m.quad(a, b2, c, e);
    let mut out = m.finish();
    sh::fix_winding(&mut out);
    out
}

/// `n + 1` points round `c` at radius `r` from angle `a0` to `a1`.
fn arc(c: [f32; 2], r: f32, a0: f32, a1: f32, n: u32) -> Vec<[f32; 2]> {
    (0..=n).map(|i| {
        let a = a0 + (a1 - a0) * i as f32 / n as f32;
        [c[0] + r * a.cos(), c[1] + r * a.sin()]
    }).collect()
}

/// A band between radii `r0` and `r1` round `c` from `a0` to `a1`.
fn band(c: [f32; 2], r0: f32, r1: f32, a0: f32, a1: f32, n: u32) -> Vec<[f32; 2]> {
    let mut p = arc(c, r0, a0, a1, n);
    let mut o = arc(c, r1, a0, a1, n);
    o.reverse();
    p.append(&mut o);
    p
}

fn mirror(poly: &[[f32; 2]], xc: f32) -> Vec<[f32; 2]> {
    poly.iter().rev().map(|p| [2.0 * xc - p[0], p[1]]).collect()
}

/// One wall of the building: its start corner `o` (on the ground), its
/// direction `t` (horizontal) and outward normal `n`, its length.
#[derive(Clone, Copy)]
struct Wall {
    o: [f32; 3],
    t: [f32; 3],
    n: [f32; 3],
}

impl Wall {
    fn at(&self, u: f32, v: f32, d: f32) -> [f32; 3] {
        [self.o[0] + self.t[0] * u + self.n[0] * d, self.o[1] + v, self.o[2] + self.t[2] * u + self.n[2] * d]
    }

    /// A box over `u` along the wall, `v` up and `d` out from its face.
    fn bx(&self, u: [f32; 2], v: [f32; 2], d: [f32; 2]) -> Mesh {
        let (a, c) = (self.at(u[0], v[0], d[0]), self.at(u[1], v[1], d[1]));
        bx([a[0], c[0]], [a[1], c[1]], [a[2], c[2]])
    }
}

/// A block over `u` by `v` (face at `d[1]`) of wall `w`; with `chip` its top
/// front arris loses a real notch (a ragged step half way down it).
fn block(m: &mut Mesh, w: &Wall, u: [f32; 2], v: [f32; 2], d: [f32; 2], chip: Option<u32>) {
    let Some(seed) = chip.filter(|_| u[1] - u[0] > 0.3) else {
        add(m, w.bx(u, v, d));
        return;
    };
    let c = sh::range(seed, 1, 2, 0.015, 0.04).min((v[1] - v[0]) * 0.4);
    let len = sh::range(seed, 1, 3, 0.06, 0.2).min((u[1] - u[0]) * 0.4);
    let at = sh::range(seed, 1, 4, u[0] + 0.02, u[1] - len - 0.02);
    add(m, w.bx(u, [v[0], v[1] - c], d));
    add(m, w.bx([u[0], at], [v[1] - c, v[1]], d));
    add(m, w.bx([at + len, u[1]], [v[1] - c, v[1]], d));
    add(m, w.bx([at, at + len], [v[1] - c, v[1] - c * 0.45], [d[0], d[1] - c * 0.6]));
    add(m, w.bx([at + len * 0.3, at + len * 0.75], [v[1] - c * 0.45, v[1]], [d[0], d[1] - c]));
    add(m, w.bx([at, at + len], [v[1] - c * 0.45, v[1]], [d[0], d[1] - c * 1.4]));
}

/// A course of stone blocks over `u` by `v` (face at `d[1]`), blocks at most
/// `pu` long with real joints (the darker backing in them); now and then
/// (`chips` of them) a block's top front arris is chipped.
#[allow(clippy::too_many_arguments)]
fn course(m: &mut Mesh, joint: &mut Mesh, w: &Wall, u: [f32; 2], v: [f32; 2], d: [f32; 2], pu: f32, seed: u32, chips: f32) {
    let n = ((u[1] - u[0]) / pu).ceil().max(1.0) as u32;
    let du = (u[1] - u[0]) / n as f32;
    let g = JOINT * 0.5;
    for i in 0..n {
        let u0 = u[0] + i as f32 * du + if i == 0 { 0.0 } else { g };
        let u1 = u[0] + (i + 1) as f32 * du - if i + 1 == n { 0.0 } else { g };
        let chip = (sh::rnd(seed, i, 1) < chips).then(|| seed.wrapping_add(i * 977));
        block(m, w, [u0, u1], v, d, chip);
    }
    if n > 1 {
        add(joint, w.bx([u[0] + 0.004, u[1] - 0.004], [v[0] + 0.004, v[1] - 0.004], [d[0], d[1] - 0.012]));
    }
}

type Hole = [[f32; 2]; 2];

/// A box over `x` by `y` (depth `z`) into `m`, the openings `holes` cut
/// through it (column by column between the openings' sides).
fn cut(m: &mut Mesh, x: [f32; 2], y: [f32; 2], z: [f32; 2], holes: &[Hole]) {
    for (c, spans) in columns(x, y, holes) {
        for s in spans {
            add(m, bx(c, s, z));
        }
    }
}

/// The wall `x` by `y` split into columns between the holes' sides, each
/// with its solid spans.
fn columns(x: [f32; 2], y: [f32; 2], holes: &[Hole]) -> Vec<([f32; 2], Vec<[f32; 2]>)> {
    let mut cuts = vec![x[0], x[1]];
    for h in holes {
        for xx in h[0] {
            if xx > x[0] && xx < x[1] {
                cuts.push(xx);
            }
        }
    }
    cuts.sort_by(f32::total_cmp);
    cuts.dedup();
    let mut out = Vec::new();
    for c in cuts.windows(2) {
        if c[1] - c[0] < 1e-3 {
            continue;
        }
        let mid = (c[0] + c[1]) * 0.5;
        let mut edges = vec![y[0], y[1]];
        for h in holes {
            if mid > h[0][0] && mid < h[0][1] {
                edges.push(h[1][0].clamp(y[0], y[1]));
                edges.push(h[1][1].clamp(y[0], y[1]));
            }
        }
        edges.sort_by(f32::total_cmp);
        let spans = edges.chunks(2).filter(|s| s.len() == 2 && s[1] - s[0] > 1e-3).map(|s| [s[0], s[1]]).collect();
        out.push(([c[0], c[1]], spans));
    }
    out
}

/// Ashlar over `x` by `y` (face at `z[1]`): courses `CH` high of blocks
/// about `BL` long, staggered, with real joints over a dark backing; blocks
/// are cut round the openings `holes`; now and then an arris is chipped.
fn ashlar(k: &mut Kit, x: [f32; 2], y: [f32; 2], z: [f32; 2], holes: &[Hole], seed: u32) {
    cut(&mut k.joint, x, y, [z[0], z[1] - 0.012], holes);
    let g = JOINT * 0.5;
    let front = Wall { o: [0.0, 0.0, z[1]], t: [1.0, 0.0, 0.0], n: [0.0, 0.0, 1.0] };
    let jit = |kk: i32, j: i32| sh::range(seed, ((kk + 64) * 257 + j + 64) as u32, 3, -0.14, 0.14);
    for (c, spans) in columns(x, y, holes) {
        for s in spans {
            let k0 = ((s[0] - y[0]) / CH).floor() as i32;
            let k1 = ((s[1] - y[0]) / CH).ceil() as i32;
            for kk in k0..k1 {
                let (b0, b1) = (y[0] + kk as f32 * CH, y[0] + (kk + 1) as f32 * CH);
                let (lo, hi) = (b0.max(s[0]), b1.min(s[1]));
                if hi - lo < 1e-3 {
                    continue;
                }
                let lo = if (lo - b0).abs() < 1e-4 && kk > 0 { lo + g } else { lo };
                let hi = if (hi - b1).abs() < 1e-4 && b1 < y[1] - 1e-4 { hi - g } else { hi };
                let off = if kk % 2 == 0 { 0.0 } else { BL * 0.5 };
                let mut edges = vec![c[0]];
                let j0 = ((c[0] - x[0] + off) / BL).floor() as i32 - 1;
                let j1 = ((c[1] - x[0] + off) / BL).ceil() as i32 + 1;
                for j in j0..=j1 {
                    let xj = x[0] - off + j as f32 * BL + jit(kk, j);
                    if xj > c[0] + 0.06 && xj < c[1] - 0.06 {
                        edges.push(xj);
                    }
                }
                edges.push(c[1]);
                edges.sort_by(f32::total_cmp);
                for (q, e) in edges.windows(2).enumerate() {
                    let a = if e[0] > c[0] + 1e-4 { e[0] + g } else { e[0] };
                    let bb = if e[1] < c[1] - 1e-4 { e[1] - g } else { e[1] };
                    if bb - a < 0.01 {
                        continue;
                    }
                    let id = ((kk + 64) * 97 + q as i32) as u32 ^ (c[0] * 1000.0) as u32;
                    // Round 2: fewer chips (at the reference's distance each
                    // notch drew as a dark dash).
                    let chip = (sh::rnd(seed, id, 9) < 0.05 && hi - lo > 0.2).then(|| seed.wrapping_add(id * 31));
                    block(&mut k.dress, &front, [a, bb], [lo, hi], [z[0] - z[1], 0.0], chip);
                }
            }
        }
    }
}

/// A segmental arch of rubbed brick over the opening `x` (its head at
/// `y1`): voussoirs on edge between two arcs (`rise` at the soffit, `dep`
/// deep), springing from the opening's top corners, the mortar behind them,
/// the walling round them and a painted head panel under the soffit (over
/// the sash's flat head). Returns the zone cut out of the wall.
#[allow(clippy::too_many_arguments)]
fn seg_arch(k: &mut Kit, wi: usize, rub: usize, x: [f32; 2], y1: f32, rise: f32, dep: f32, zf: f32, seed: u32) -> Hole {
    let s = (x[1] - x[0]) * 0.5;
    let xc = (x[0] + x[1]) * 0.5;
    let ri = (s * s + rise * rise) / (2.0 * rise);
    let cy = y1 + rise - ri;
    let ro = ri + dep;
    let c = [xc, cy];
    let al = (y1 - cy).atan2(x[0] - xc);
    let ar = (y1 - cy).atan2(x[1] - xc);
    let top = cy + ro + 0.06;
    let pl = [xc + ro * al.cos(), cy + ro * al.sin()];
    let zl = pl[0] - 0.08;
    let zw = [zf - T, zf];
    // The walling round the arch.
    let mut left = vec![[zl, y1], [x[0], y1]];
    left.extend(arc(c, ro, al, FRAC_PI_2, 10));
    left.push([xc, top]);
    left.push([zl, top]);
    let right = mirror(&left, xc);
    add(&mut k.wall[wi], slab(&left, zw));
    add(&mut k.wall[wi], slab(&right, zw));
    // The voussoirs, each a little proud or shy of the next.
    let n = ((ar - al).abs() * (ri + dep * 0.5) / 0.078).round().max(5.0) as u32;
    let gap = 0.007;
    for i in 0..n {
        let a0 = al + (ar - al) * i as f32 / n as f32;
        let a1 = al + (ar - al) * (i + 1) as f32 / n as f32;
        let (gi, go) = (gap * 0.5 / ri, gap * 0.5 / ro);
        let mut p = arc(c, ri, a0 - gi, a1 + gi, 2);
        let mut o = arc(c, ro, a0 - go, a1 + go, 2);
        o.reverse();
        p.append(&mut o);
        let proud = 0.008 + sh::range(seed, i, 5, -0.004, 0.004);
        add(&mut k.rubbed[rub], slab(&p, [zf - T, zf + proud]));
    }
    add(&mut k.mortar, slab(&band(c, ri + 0.004, ro - 0.004, al, ar, 16), [zf - T, zf - 0.006]));
    // The head panel between the sash's flat head and the soffit.
    let mut tym = vec![[x[0], y1], [x[1], y1]];
    tym.extend(arc(c, ri, ar, al, 12).into_iter().skip(1));
    tym.pop();
    add(&mut k.white, slab(&tym, [zf - 0.2, zf - 0.1]));
    [[zl, 2.0 * xc - zl], [y1, top]]
}

/// A flat gauged arch of rubbed brick over the opening `x` (head `y1`):
/// voussoirs fanning out between the soffit and a top 0.3 m up, splayed
/// ends, a keystone a little proud; the mortar behind; the walling in the
/// two splays. Returns the zone cut out of the wall.
fn flat_arch(k: &mut Kit, wi: usize, rub: usize, x: [f32; 2], y1: f32, zf: f32, seed: u32) -> Hole {
    let (h, sb, st) = (0.3, 0.02, 0.13);
    let (b0, b1) = (x[0] - sb, x[1] + sb);
    let (t0, t1) = (x[0] - st, x[1] + st);
    let n = 13u32;
    let gap = 0.007;
    let zw = [zf - T, zf];
    for i in 0..n {
        let f0 = i as f32 / n as f32;
        let f1 = (i + 1) as f32 / n as f32;
        let (ba, bb2) = (b0 + (b1 - b0) * f0 + gap * 0.5, b0 + (b1 - b0) * f1 - gap * 0.5);
        let (ta, tb) = (t0 + (t1 - t0) * f0 + gap * 0.5, t0 + (t1 - t0) * f1 - gap * 0.5);
        let key = i == n / 2;
        let (lo, hi) = if key { (y1 - 0.035, y1 + h + 0.035) } else { (y1, y1 + h) };
        let proud = if key { 0.03 } else { 0.008 + sh::range(seed, i, 6, -0.004, 0.004) };
        add(&mut k.rubbed[rub], slab(&[[ba, lo], [bb2, lo], [tb, hi], [ta, hi]], [zf - T, zf + proud]));
    }
    add(&mut k.mortar, slab(&[[b0, y1], [b1, y1], [t1, y1 + h], [t0, y1 + h]], [zf - T, zf - 0.006]));
    add(&mut k.wall[wi], slab(&[[t0, y1], [b0, y1], [t0, y1 + h]], zw));
    add(&mut k.wall[wi], slab(&[[b1, y1], [t1, y1], [t1, y1 + h]], zw));
    [[t0, t1], [y1, y1 + h]]
}

/// The moulded dressings round a window of an ashlar or stucco front: an
/// architrave (a band with a bead and an outer step), a frieze and a
/// cornice over it.
fn dressings(m: &mut Mesh, x: [f32; 2], y: [f32; 2], zf: f32, cornice: bool) {
    let (w, p) = (0.15, 0.045);
    let z = [zf - 0.02, zf + p];
    add(m, bx([x[0] - w, x[0]], [y[0], y[1] + w], z));
    add(m, bx([x[1], x[1] + w], [y[0], y[1] + w], z));
    add(m, bx([x[0], x[1]], [y[1], y[1] + w], z));
    // The bead along the opening and the outer step.
    let zb = [zf - 0.02, zf + p + 0.018];
    add(m, bx([x[0] - 0.035, x[0]], [y[0], y[1] + 0.035], zb));
    add(m, bx([x[1], x[1] + 0.035], [y[0], y[1] + 0.035], zb));
    add(m, bx([x[0], x[1]], [y[1], y[1] + 0.035], zb));
    let zs = [zf - 0.02, zf + p * 0.5];
    add(m, bx([x[0] - w - 0.035, x[0] - w], [y[0], y[1] + w + 0.035], zs));
    add(m, bx([x[1] + w, x[1] + w + 0.035], [y[0], y[1] + w + 0.035], zs));
    add(m, bx([x[0] - w - 0.035, x[1] + w + 0.035], [y[1] + w, y[1] + w + 0.035], zs));
    if cornice {
        let y2 = y[1] + w + 0.035;
        add(m, bx([x[0] - w, x[1] + w], [y2, y2 + 0.13], [zf - 0.02, zf + p * 0.7]));
        add(m, bx([x[0] - w - 0.05, x[1] + w + 0.05], [y2 + 0.13, y2 + 0.17], [zf - 0.02, zf + 0.1]));
        add(m, bx([x[0] - w - 0.1, x[1] + w + 0.1], [y2 + 0.17, y2 + 0.27], [zf - 0.02, zf + 0.15]));
        // The cornice's top weathers back.
        add(m, bx([x[0] - w - 0.08, x[1] + w + 0.08], [y2 + 0.27, y2 + 0.3], [zf - 0.02, zf + 0.11]));
    }
}

/// A stone sill over `x` with its top at `y`, out past the wall, its arris
/// chipped here and there.
fn sill(k: &mut Kit, x: [f32; 2], y: f32, zf: f32, seed: u32, stucco: bool) {
    let w = Wall { o: [0.0, 0.0, zf], t: [1.0, 0.0, 0.0], n: [0.0, 0.0, 1.0] };
    let m = if stucco { &mut k.trim } else { &mut k.dress };
    course(m, &mut k.joint, &w, x, [y - 0.1, y], [-T + 0.12, 0.07], 3.0, seed, 0.6);
}

/// A six-over-six sash window in the opening `x` by `y`: the painted box
/// frame set back in the reveal, the upper sash behind the lower, glazing
/// bars, the glass; inside, a dark room and curtains drawn to the sides.
fn sash(k: &mut Kit, x: [f32; 2], y: [f32; 2], zf: f32, seed: u32) {
    let f = 0.07;
    let fz = [zf - 0.22, zf - 0.1];
    add(&mut k.white, bx([x[0], x[0] + f], y, fz));
    add(&mut k.white, bx([x[1] - f, x[1]], y, fz));
    add(&mut k.white, bx([x[0] + f, x[1] - f], [y[1] - f, y[1]], fz));
    add(&mut k.white, bx([x[0] + f, x[1] - f], [y[0], y[0] + 0.05], [zf - 0.22, zf - 0.08]));
    let (ix, iy) = ([x[0] + f, x[1] - f], [y[0] + 0.05, y[1] - f]);
    let mid = (iy[0] + iy[1]) * 0.5 + 0.03;
    // A few lower sashes are lifted a little.
    let lift = if sh::rnd(seed, 1, 1) < 0.2 { sh::range(seed, 1, 2, 0.04, 0.12) } else { 0.0 };
    for (lo, hi, z, bottom) in [(mid - 0.02, iy[1], [zf - 0.18, zf - 0.14], 0.045), (iy[0] + lift, mid + 0.02 + lift, [zf - 0.14, zf - 0.1], 0.07)] {
        let s = 0.045;
        add(&mut k.white, bx([ix[0], ix[0] + s], [lo, hi], z));
        add(&mut k.white, bx([ix[1] - s, ix[1]], [lo, hi], z));
        add(&mut k.white, bx([ix[0] + s, ix[1] - s], [hi - s, hi], z));
        add(&mut k.white, bx([ix[0] + s, ix[1] - s], [lo, lo + bottom], z));
        // Glazing bars: three panes across, two up.
        let (gx, gy) = ([ix[0] + s, ix[1] - s], [lo + bottom, hi - s]);
        let bz = [z[0] + 0.008, z[1] - 0.012];
        for q in 1..3 {
            let xx = gx[0] + (gx[1] - gx[0]) * q as f32 / 3.0;
            add(&mut k.white, bx([xx - 0.011, xx + 0.011], gy, bz));
        }
        let yy = (gy[0] + gy[1]) * 0.5;
        add(&mut k.white, bx(gx, [yy - 0.011, yy + 0.011], bz));
        add(&mut k.glass, pane(gx, gy, (z[0] + z[1]) * 0.5));
    }
    // The gap under a lifted sash shows the room.
    // The curtains: two folded panels just inside the wall (round 2: wider,
    // each window drawn its own amount, so both read from the street).
    let cz = zf - T - 0.035;
    let (wl, wr) = (sh::range(seed, 2, 3, 0.3, 0.42), sh::range(seed, 2, 4, 0.3, 0.42));
    for (a, bb) in [(x[0] - 0.08, x[0] + wl), (x[1] - wr, x[1] + 0.08)] {
        let n = 10;
        let mut top: Vec<[f32; 2]> = Vec::new();
        for i in 0..=n {
            let t = i as f32 / n as f32;
            let xx = a + (bb - a) * t;
            let zz = cz + (t * PI * 4.0 + sh::rnd(seed, i, 8)).sin() * 0.025;
            top.push([xx, zz + 0.005]);
        }
        let mut poly: Vec<[f32; 2]> = top.clone();
        poly.extend(top.iter().rev().map(|p| [p[0], p[1] - 0.01]));
        add(&mut k.curtain, b::prism(&poly, y[0] - 0.05, y[1] - y[0] + 0.12));
    }
}

/// The rooms behind a house's front: a dark lining (floor, ceiling, back
/// and side walls) for each storey `ys`, between `x`.
fn rooms(k: &mut Kit, x: [f32; 2], ys: &[[f32; 2]], zf: f32) {
    let back = zf - T - 2.6;
    for y in ys {
        let z = [back, zf - T];
        add(&mut k.dark, bx([x[0], x[1]], [y[0], y[0] + 0.02], z));
        add(&mut k.dark, bx([x[0], x[1]], [y[1] - 0.02, y[1]], z));
        add(&mut k.dark, bx([x[0], x[1]], *y, [back - 0.02, back]));
        add(&mut k.dark, bx([x[0], x[0] + 0.02], *y, z));
        add(&mut k.dark, bx([x[1] - 0.02, x[1]], *y, z));
    }
}

/// A leaf about `r` long at `p` (twenty triangles): a thin, slightly
/// lumpy blade turned to its own tilt and heading, `droop` tipping it down.
fn leaf(m: &mut Mesh, p: [f32; 3], r: f32, seed: u32, i: u32, droop: f32) {
    let mut l = sh::blob([r, r * 0.14, r * 0.5], [0.0; 3], 0.35, seed.wrapping_add(i * 7), 0);
    sh::rotate(&mut l, [0.0, 0.0, 1.0], sh::range(seed, i, 41, -0.7, 0.7) - droop);
    sh::rotate(&mut l, [1.0, 0.0, 0.0], sh::range(seed, i, 42, -0.8, 0.8));
    sh::rotate(&mut l, [0.0, 1.0, 0.0], sh::rnd(seed, i, 43) * std::f32::consts::TAU);
    geo::transform(&mut l, [1.0; 3], p);
    add(m, l);
}

/// A painted window box on the sill under `x` (its foot at `y`), planted in
/// style `fl` (round 2: each house its own): 0 trailing ivy and pink
/// geraniums, 1 a low mound of white flowers, 2 upright purple spikes, 3
/// long green trails and a few flowers. Leaves are thin tilted blades in
/// layers (round 1's were round lumps), strands hang over the front.
fn window_box(k: &mut Kit, x: [f32; 2], y: f32, zf: f32, fl: usize, seed: u32, dens: f32) {
    let (x0, x1) = (x[0] + 0.03, x[1] - 0.03);
    add(&mut k.black, bx([x0, x1], [y, y + 0.17], [zf + 0.0, zf + 0.21]));
    add(&mut k.black, bx([x0 - 0.01, x1 + 0.01], [y + 0.15, y + 0.18], [zf - 0.005, zf + 0.22]));
    add(&mut k.soot, bx([x0 + 0.01, x1 - 0.01], [y + 0.17, y + 0.185], [zf + 0.01, zf + 0.2]));
    // (mound leaves, mound height, strands, strand length, flowers)
    let (nl, mh, ns, sl, nf) = match fl % 4 {
        0 => (70.0, 0.13, 10.0, [0.15, 0.42], 22.0),
        1 => (55.0, 0.1, 4.0, [0.08, 0.2], 46.0),
        2 => (60.0, 0.12, 3.0, [0.08, 0.18], 16.0),
        _ => (75.0, 0.11, 15.0, [0.25, 0.55], 6.0),
    };
    let n = (nl * dens) as u32 + 12;
    for i in 0..n {
        let px = sh::range(seed, i, 1, x0 + 0.03, x1 - 0.03);
        // Lower at the ends, a dome over the middle.
        let t = ((px - (x0 + x1) * 0.5) / ((x1 - x0) * 0.5)).abs();
        let py = y + 0.18 + sh::range(seed, i, 2, 0.0, mh) * (1.0 - 0.45 * t * t);
        let pz = zf + sh::range(seed, i, 3, 0.02, 0.23);
        let r = sh::range(seed, i, 4, 0.034, 0.055);
        leaf(&mut k.leaves[(sh::rnd(seed, i, 5) * 2.0) as usize % 2], [px, py, pz], r, seed, i, 0.0);
    }
    // Strands hanging over the front, each its own length and sway.
    let strands = (ns * dens) as u32 + 2;
    for s in 0..strands {
        let px = sh::range(seed, 100 + s, 1, x0 + 0.02, x1 - 0.02);
        let len = sh::range(seed, 100 + s, 2, sl[0], sl[1]);
        let n = (len / 0.035) as u32 + 2;
        let sway = sh::range(seed, 100 + s, 3, -0.04, 0.04);
        for j in 0..n {
            let t = j as f32 / n as f32;
            let p = [px + sway * t + (t * 7.0 + s as f32).sin() * 0.012, y + 0.17 - len * t, zf + 0.225 + t * 0.03 + (t * 3.0).sin() * 0.012];
            leaf(&mut k.leaves[((s + j) % 2) as usize], p, 0.03 - t * 0.008, seed, 1000 + s * 64 + j, 0.9);
        }
    }
    // The flowers.
    let nf = (nf * dens) as u32 + 2;
    for i in 0..nf {
        let px = sh::range(seed, 300 + i, 1, x0 + 0.04, x1 - 0.04);
        let pz = zf + sh::range(seed, 300 + i, 3, 0.04, 0.22);
        let top = y + 0.2 + mh * sh::range(seed, 300 + i, 2, 0.5, 1.05);
        let c = if fl % 4 != 1 && sh::rnd(seed, 300 + i, 5) < 0.12 { 1 } else { fl % 4 };
        match fl % 4 {
            2 => {
                // A spike: small florets up a stem.
                for q in 0..4u32 {
                    let r = 0.016 - q as f32 * 0.002;
                    add(&mut k.flowers[c], sh::blob([r, r * 1.2, r], [px, top + q as f32 * 0.026, pz], 0.4, seed.wrapping_add(i * 9 + q), 0));
                }
            }
            1 => {
                let r = sh::range(seed, 300 + i, 4, 0.014, 0.024);
                add(&mut k.flowers[c], sh::blob([r, r * 0.7, r], [px, top, pz], 0.5, seed.wrapping_add(i * 5), 0));
            }
            _ => {
                // A head of a few florets.
                let r = sh::range(seed, 300 + i, 4, 0.016, 0.024);
                for q in 0..3u32 {
                    let o = [sh::range(seed, 400 + i, q, -0.02, 0.02), sh::range(seed, 500 + i, q, 0.0, 0.015), sh::range(seed, 600 + i, q, -0.02, 0.02)];
                    add(&mut k.flowers[c], sh::blob([r, r * 0.75, r], [px + o[0], top + o[1], pz + o[2]], 0.5, seed.wrapping_add(i * 5 + q), 0));
                }
            }
        }
    }
}

/// A clipped box ball in a square stone planter at (x, z), `size` its scale
/// (round 2: a dense dark core under layers of small leaves, not lumps).
fn topiary(k: &mut Kit, x: f32, z: f32, seed: u32, size: f32) {
    let mut pot = sh::frustum(0.21, 0.27, 0.46, 4);
    sh::rotate(&mut pot, [0.0, 1.0, 0.0], PI * 0.25);
    geo::transform(&mut pot, [1.0; 3], [x, 0.0, z]);
    add(&mut k.plinth, pot);
    add(&mut k.plinth, bx([x - 0.205, x + 0.205], [0.44, 0.5], [z - 0.205, z + 0.205]));
    add(&mut k.soot, bx([x - 0.17, x + 0.17], [0.5, 0.505], [z - 0.17, z + 0.17]));
    let r = 0.3 * size;
    let cy = 0.72 + r;
    add(&mut k.soot, b::rod([x, 0.5, z], [x + 0.01, cy - r * 0.6, z], 0.02, 6));
    add(&mut k.leaves[0], sh::blob([r * 0.86, r * 0.82, r * 0.86], [x, cy, z], 0.15, seed, 2));
    for i in 0..130u32 {
        let u = sh::range(seed, i, 1, -0.95, 1.0);
        let a = sh::rnd(seed, i, 2) * std::f32::consts::TAU;
        let q = (1.0 - u * u).max(0.0).sqrt();
        let d = [q * a.cos(), u, q * a.sin()];
        let rr = r * sh::range(seed, i, 3, 0.86, 0.98);
        let p = [x + d[0] * rr, cy + d[1] * rr * 0.94, z + d[2] * rr];
        leaf(&mut k.leaves[(sh::rnd(seed, i, 4) < 0.6) as usize], p, sh::range(seed, i, 5, 0.04, 0.06), seed, i, 0.0);
    }
}

/// A black carriage lantern on a bracket at `x` (its foot at `y`).
fn lantern(k: &mut Kit, x: f32, y: f32, zf: f32) {
    add(&mut k.black, bx([x - 0.045, x + 0.045], [y + 0.2, y + 0.46], [zf, zf + 0.022]));
    add(&mut k.black, b::rod([x, y + 0.4, zf + 0.02], [x, y + 0.4, zf + 0.15], 0.012, 6));
    add(&mut k.black, b::rod([x, y + 0.27, zf + 0.02], [x, y + 0.4, zf + 0.15], 0.008, 5));
    let zc = zf + 0.17;
    let h = 0.085;
    for (dx, dz) in [(-1.0f32, -1.0f32), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        add(&mut k.black, bx([x + dx * h - 0.008, x + dx * h + 0.008], [y + 0.04, y + 0.3], [zc + dz * h - 0.008, zc + dz * h + 0.008]));
    }
    add(&mut k.black, bx([x - h - 0.015, x + h + 0.015], [y, y + 0.045], [zc - h - 0.015, zc + h + 0.015]));
    let mut cap = sh::frustum(0.14, 0.025, 0.1, 4);
    sh::rotate(&mut cap, [0.0, 1.0, 0.0], PI * 0.25);
    geo::transform(&mut cap, [1.0; 3], [x, y + 0.3, zc]);
    add(&mut k.black, cap);
    add(&mut k.black, b::rod([x, y + 0.39, zc], [x, y + 0.45, zc], 0.012, 6));
    add(&mut k.black, b::rod([x, y - 0.06, zc], [x, y, zc], 0.015, 6));
    add(&mut k.lamp, bx([x - h + 0.004, x + h - 0.004], [y + 0.045, y + 0.3], [zc - h + 0.004, zc + h - 0.004]));
}

/// The front steps up to the door at `dc`: `STEPS` stone steps with
/// nosings (their arrises chipped), black iron railings on both sides
/// (balusters, a bottom rail and a handrail, a newel at the foot).
fn steps(k: &mut Kit, dc: f32, zf: f32, seed: u32) {
    let hw = 0.78;
    let rise = PL / STEPS as f32;
    let front = |s: u32| zf + PL_OUT + LAND + RUN * (STEPS - 1 - s) as f32;
    let mut none = Mesh::default();
    for s in 0..STEPS {
        let top = rise * (s + 1) as f32;
        let fz = front(s);
        add(&mut k.plinth, bx([dc - hw, dc + hw], [0.0, top - 0.06], [zf, fz - 0.02]));
        let w = Wall { o: [dc - hw - 0.02, 0.0, fz], t: [1.0, 0.0, 0.0], n: [0.0, 0.0, 1.0] };
        course(&mut k.plinth, &mut none, &w, [0.0, 2.0 * hw + 0.04], [top - 0.06, top], [zf - fz, 0.025], 3.0, seed.wrapping_add(s * 31), 0.7);
    }
    // The railings, a little in from each side.
    let nos = |z: f32| -> f32 {
        // The nosing line's height at z (the flight, then the landing).
        let (zb, zt) = (front(0), front(STEPS - 1));
        if z <= zt { PL } else { rise + (PL - rise) * ((zb - z) / (zb - zt)).clamp(0.0, 1.0) }
    };
    let tread = |z: f32| -> f32 {
        for s in (0..STEPS).rev() {
            if z <= front(s) {
                return rise * (s + 1) as f32;
            }
        }
        rise
    };
    for side in [-1.0f32, 1.0] {
        let x = dc + side * (hw - 0.05);
        let zb = front(0) - 0.08;
        let zw = zf + 0.06;
        let rail = |z: f32, up: f32| [x, nos(z) + up, z];
        let pts = [zb, front(STEPS - 1), zw];
        for (up, wd, ht) in [(0.9, 0.05, 0.025), (0.1, 0.022, 0.03)] {
            add(&mut k.black, b::bar(rail(pts[0], up), rail(pts[1], up), wd, ht));
            add(&mut k.black, b::bar(rail(pts[1], up), rail(pts[2], up), wd, ht));
        }
        // The newel at the foot, a ball on it.
        add(&mut k.black, b::bar([x, tread(zb), zb], [x, nos(zb) + 1.0, zb], 0.04, 0.04));
        add(&mut k.black, sh::blob([0.035, 0.035, 0.035], [x, nos(zb) + 1.03, zb], 0.0, 1, 1));
        // A scroll at the handrail's foot.
        add(&mut k.black, b::rod([x, nos(zb) + 0.9, zb], [x, nos(zb) + 0.78, zb + 0.07], 0.012, 5));
        let mut z = zb - 0.12;
        while z > zw + 0.04 {
            add(&mut k.black, b::bar([x, tread(z), z], [x, nos(z) + 0.9, z], 0.016, 0.016));
            z -= 0.12;
        }
    }
}

/// The door at `dc`: a painted case, a six-panel leaf in the house's colour
/// (stiles, rails and raised fields), a brass letterbox, knob and knocker.
fn door(k: &mut Kit, dc: f32, zf: f32, colour: usize) {
    let (x0, x1) = (dc - DOOR_R, dc + DOOR_R);
    let top = SPRING - 0.06;
    let fz = [zf - 0.24, zf - 0.11];
    add(&mut k.white, bx([x0, x0 + 0.07], [PL, top], fz));
    add(&mut k.white, bx([x1 - 0.07, x1], [PL, top], fz));
    let (l0, l1) = (x0 + 0.07, x1 - 0.07);
    let lo = PL + 0.005;
    let d = &mut k.door[colour];
    add(d, bx([l0, l1], [lo, top], [zf - 0.23, zf - 0.19]));
    let fr = [zf - 0.2, zf - 0.145];
    let st = 0.12;
    add(d, bx([l0, l0 + st], [lo, top], fr));
    add(d, bx([l1 - st, l1], [lo, top], fr));
    let mc = (l0 + l1) * 0.5;
    add(d, bx([mc - 0.05, mc + 0.05], [lo, top], fr));
    let rails = [[lo, lo + 0.24], [PL + 0.92, PL + 1.1], [PL + 1.95, PL + 2.07], [top - 0.12, top]];
    for r in rails {
        add(d, bx([l0, l1], r, fr));
    }
    // The panels (round 2: deeper): sunk 3.5 cm behind the stiles, a
    // bevelled ring, a raised field, and a bolection moulding standing proud
    // round each opening (its highlight and shadow read from the street).
    for w in rails.windows(2) {
        let (y0, y1) = (w[0][1], w[1][0]);
        for (a, c) in [(l0 + st, mc - 0.05), (mc + 0.05, l1 - st)] {
            add(d, bx([a + 0.03, c - 0.03], [y0 + 0.03, y1 - 0.03], [zf - 0.2, zf - 0.18]));
            add(d, bx([a + 0.06, c - 0.06], [y0 + 0.06, y1 - 0.06], [zf - 0.2, zf - 0.164]));
            let (mw, mz) = (0.024, [zf - 0.2, zf - 0.136]);
            add(d, bx([a, a + mw], [y0, y1], mz));
            add(d, bx([c - mw, c], [y0, y1], mz));
            add(d, bx([a + mw, c - mw], [y0, y0 + mw], mz));
            add(d, bx([a + mw, c - mw], [y1 - mw, y1], mz));
        }
    }
    // The letter plate with its flap, the knob, and a ring knocker on a
    // boss (round 2: shaped, not plain bars).
    add(&mut k.brass, bx([mc - 0.15, mc + 0.15], [PL + 0.97, PL + 1.04], [zf - 0.146, zf - 0.136]));
    add(&mut k.soot, bx([mc - 0.12, mc + 0.12], [PL + 0.995, PL + 1.015], [zf - 0.136, zf - 0.134]));
    add(&mut k.brass, bx([mc - 0.125, mc + 0.125], [PL + 1.015, PL + 1.03], [zf - 0.136, zf - 0.128]));
    add(&mut k.brass, sh::blob([0.032, 0.032, 0.025], [mc + 0.39, PL + 1.0, zf - 0.11], 0.0, 1, 1));
    add(&mut k.brass, b::rod([mc + 0.39, PL + 1.0, zf - 0.145], [mc + 0.39, PL + 1.0, zf - 0.12], 0.012, 6));
    add(&mut k.brass, sh::blob([0.038, 0.038, 0.014], [mc, PL + 1.6, zf - 0.135], 0.0, 1, 1));
    let ring: Vec<[f32; 3]> = (0..=18).map(|q| {
        let a = q as f32 / 18.0 * std::f32::consts::TAU;
        [mc + 0.07 * a.cos(), PL + 1.49 + 0.075 * a.sin(), zf - 0.124]
    }).collect();
    add(&mut k.brass, b::sweep(&ring, &b::circle(0.012, 6), false));
}

/// The door's round arch at `dc` springing at `SPRING`: rubbed brick
/// voussoirs (brick fronts) or a moulded ring with a keystone, jamb
/// pilasters and imposts (ashlar and stucco); the walling round it; the
/// fanlight (a half-round frame, a hub and radiating bars, the glass).
/// Returns the zone cut out of the wall.
fn door_arch(k: &mut Kit, face: Face, dc: f32, zf: f32, seed: u32) -> Hole {
    let c = [dc, SPRING];
    let ri = DOOR_R;
    let brick = face.brick();
    let ro = if brick { ri + 0.3 } else { ri + 0.2 };
    let m = 0.1;
    let top = SPRING + ro + m;
    let zw = [zf - T, zf];
    let mut left = vec![[dc - ro - m, SPRING], [dc - ro, SPRING]];
    left.extend(arc(c, ro, PI, FRAC_PI_2, 14).into_iter().skip(1));
    left.push([dc, top]);
    left.push([dc - ro - m, top]);
    let right = mirror(&left, dc);
    {
        let wm = match face {
            Face::Stone => &mut k.dress,
            _ => &mut k.wall[face.wall()],
        };
        add(wm, slab(&left, zw));
        add(wm, slab(&right, zw));
    }
    if brick {
        let rub = if face == Face::Yellow { 1 } else { 0 };
        let n = 25u32;
        let gap = 0.007;
        for i in 0..n {
            let a0 = PI - PI * i as f32 / n as f32;
            let a1 = PI - PI * (i + 1) as f32 / n as f32;
            let (gi, go) = (gap * 0.5 / ri, gap * 0.5 / ro);
            let mut p = arc(c, ri, a0 - gi, a1 + gi, 2);
            let mut o = arc(c, ro, a0 - go, a1 + go, 2);
            o.reverse();
            p.append(&mut o);
            let proud = 0.012 + sh::range(seed, i, 7, -0.004, 0.004);
            add(&mut k.rubbed[rub], slab(&p, [zf - T, zf + proud]));
        }
        add(&mut k.mortar, slab(&band(c, ri + 0.004, ro - 0.004, PI, 0.0, 20), [zf - T, zf - 0.006]));
    } else {
        let mm = if face == Face::Stone { &mut k.dress } else { &mut k.trim };
        add(mm, slab(&band(c, ri, ro, PI, 0.0, 24), [zf - T, zf + 0.035]));
        add(mm, slab(&band(c, ri + 0.03, ri + 0.07, PI, 0.0, 24), [zf, zf + 0.055]));
        add(mm, slab(&band(c, ro - 0.04, ro, PI, 0.0, 24), [zf, zf + 0.05]));
        // The keystone.
        add(mm, slab(&[[dc - 0.085, SPRING + ri - 0.03], [dc + 0.085, SPRING + ri - 0.03], [dc + 0.13, SPRING + ro + 0.07], [dc - 0.13, SPRING + ro + 0.07]], [zf - T, zf + 0.085]));
        // The jambs' pilasters and imposts.
        for s in [-1.0f32, 1.0] {
            let (a, bb) = (dc + s * ri, dc + s * (ri + 0.2));
            add(mm, bx([a, bb], [PL, SPRING - 0.12], [zf - 0.02, zf + 0.045]));
            add(mm, bx([a + s * 0.03, bb], [PL, SPRING - 0.12], [zf - 0.02, zf + 0.06]));
            add(mm, bx([a, bb + s * 0.04], [SPRING - 0.12, SPRING], [zf - 0.02, zf + 0.085]));
            add(mm, bx([a, bb + s * 0.02], [PL, PL + 0.25], [zf - 0.02, zf + 0.075]));
        }
    }
    // The fanlight.
    let fz = [zf - 0.2, zf - 0.13];
    add(&mut k.white, slab(&band(c, ri - 0.055, ri + 0.004, PI, 0.0, 24), fz));
    add(&mut k.white, bx([dc - ri, dc + ri], [SPRING - 0.06, SPRING + 0.025], [zf - 0.22, zf - 0.11]));
    add(&mut k.white, slab(&arc(c, 0.11, 0.0, PI, 10), fz));
    for i in 1..8 {
        let a = PI * i as f32 / 8.0;
        let (ca, sa) = (a.cos(), a.sin());
        let (r0, r1, hw) = (0.1, ri - 0.05, 0.007);
        let p = [[dc + r0 * ca + hw * sa, SPRING + r0 * sa - hw * ca], [dc + r1 * ca + hw * sa, SPRING + r1 * sa - hw * ca], [dc + r1 * ca - hw * sa, SPRING + r1 * sa + hw * ca], [dc + r0 * ca - hw * sa, SPRING + r0 * sa + hw * ca]];
        add(&mut k.white, slab(&p, [zf - 0.19, zf - 0.14]));
    }
    let mut g = geo::MeshBuilder::new();
    let n = [0.0, 0.0, 1.0];
    let zc = zf - 0.165;
    let o = g.vertex([dc, SPRING + 0.02, zc], n);
    let rim = arc([dc, SPRING + 0.02], ri - 0.02, 0.0, PI, 20);
    let first = g.vertex([rim[0][0], rim[0][1], zc], n);
    for p in rim.iter().skip(1) {
        g.vertex([p[0], p[1], zc], n);
    }
    for i in 0..20u32 {
        g.tri(o, first + i, first + i + 1);
    }
    let mut glass = g.finish();
    sh::fix_winding(&mut glass);
    add(&mut k.glass, glass);
    // The room behind the fanlight.
    add(&mut k.dark, bx([dc - ri - 0.2, dc + ri + 0.2], [SPRING - 0.1, SPRING + ri + 0.2], [zf - T - 1.2, zf - T - 1.18]));
    [[dc - ro - m, dc + ro + m], [SPRING, top]]
}

/// A cast-iron air grille in the plinth under `xc`.
fn grille(k: &mut Kit, xc: f32, zf: f32) {
    let z = zf + PL_OUT;
    let (x0, x1, y0, y1) = (xc - 0.22, xc + 0.22, 0.32, 0.56);
    add(&mut k.soot, bx([x0, x1], [y0, y1], [z, z + 0.004]));
    add(&mut k.black, bx([x0 - 0.02, x0 + 0.01], [y0 - 0.02, y1 + 0.02], [z, z + 0.018]));
    add(&mut k.black, bx([x1 - 0.01, x1 + 0.02], [y0 - 0.02, y1 + 0.02], [z, z + 0.018]));
    add(&mut k.black, bx([x0, x1], [y0 - 0.02, y0 + 0.01], [z, z + 0.018]));
    add(&mut k.black, bx([x0, x1], [y1 - 0.01, y1 + 0.02], [z, z + 0.018]));
    for i in 1..6 {
        let y = y0 + (y1 - y0) * i as f32 / 6.0;
        add(&mut k.black, bx([x0, x1], [y - 0.007, y + 0.007], [z, z + 0.014]));
    }
}

/// A downpipe at `x` on the front: a swan neck from the gutter back to the
/// wall, the pipe down to a shoe at the foot, brackets.
fn downpipe(k: &mut Kit, x: f32, zf: f32, eave: f32) {
    let r = 0.045;
    let (zw, zg) = (zf + 0.1, zf + OV + 0.07);
    let y1 = eave - 0.32;
    add(&mut k.black, b::rod([x, eave - 0.06, zg], [x, eave - 0.18, zg], r, 10));
    add(&mut k.black, b::rod([x, eave - 0.18, zg], [x, y1, zw], r, 10));
    add(&mut k.black, b::rod([x, y1 + 0.02, zw], [x, PL + 0.2, zw], r, 10));
    add(&mut k.black, b::rod([x, PL + 0.22, zw], [x, PL - 0.1, zw + PL_OUT], r, 10));
    add(&mut k.black, b::rod([x, PL - 0.08, zw + PL_OUT], [x, 0.25, zw + PL_OUT], r, 10));
    add(&mut k.black, b::rod([x, 0.27, zw + PL_OUT], [x, 0.06, zw + PL_OUT + 0.16], r, 10));
    let n = ((y1 - PL) / 1.6).floor().max(1.0) as u32;
    for i in 0..=n {
        let y = PL + 0.4 + (y1 - PL - 0.6) * i as f32 / n as f32;
        add(&mut k.black, bx([x - 0.06, x + 0.06], [y - 0.022, y + 0.022], [zf, zw + 0.06]));
    }
}

/// The house `i` of the row at `xa` (`w` wide): its front with its
/// openings, arches, dressings, door, steps, windows, boxes and lantern;
/// the rooms behind.
#[allow(clippy::too_many_arguments)]
fn house(k: &mut Kit, i: usize, xa: f32, w: f32, zf: f32, eave: f32, floors: u32, seed: u32, dens: f32, last: bool) {
    let face = ORDER[i % ORDER.len()];
    let wi = face.wall();
    let brick = face.brick();
    let rub = if face == Face::Yellow { 1 } else { 0 };
    let stucco = face == Face::Stucco;
    let seed = seed.wrapping_add(i as u32 * 7919);
    let bay = |j: usize| xa + BAYS[j] * w;
    let mut holes: Vec<Hole> = Vec::new();
    // The ground floor's windows.
    for j in 0..2 {
        let c = bay(j);
        let x = [c - HW, c + HW];
        holes.push([[c - HW - 0.12, c + HW + 0.12], [G_SILL - 0.1, G_SILL]]);
        holes.push([x, [G_SILL, G_HEAD]]);
        if brick {
            holes.push(seg_arch(k, wi, rub, x, G_HEAD, 0.13, 0.25, zf, seed.wrapping_add(j as u32 * 11)));
        } else {
            let m = if stucco { &mut k.trim } else { &mut k.dress };
            dressings(m, x, [G_SILL, G_HEAD], zf, true);
        }
        sill(k, [c - HW - 0.12, c + HW + 0.12], G_SILL, zf, seed ^ (0x31 + j as u32), stucco);
        sash(k, x, [G_SILL, G_HEAD], zf, seed.wrapping_add(40 + j as u32));
        window_box(k, x, G_SILL, zf, i % 4, seed.wrapping_add(90 + j as u32), dens);
        grille(k, c, zf);
    }
    // The door, its arch, its steps and its lantern.
    let dc = bay(2);
    holes.push([[dc - DOOR_R, dc + DOOR_R], [PL, SPRING]]);
    holes.push(door_arch(k, face, dc, zf, seed ^ 0x77));
    door(k, dc, zf, i % 5);
    steps(k, dc, zf, seed ^ 0x55);
    lantern(k, dc - DOOR_R - if brick { 0.62 } else { 0.66 }, 3.12, zf);
    if !last {
        topiary(k, dc - 1.16, zf + 0.42, seed ^ 0x99, 0.88 + 0.08 * ((i * 3) % 4) as f32);
    }
    // The storeys over it.
    for f in 1..floors {
        let (y0, y1) = (U_SILL + (f - 1) as f32 * STOREY, U_HEAD + (f - 1) as f32 * STOREY - if f > 1 { 0.2 } else { 0.0 });
        for j in 0..3 {
            let c = bay(j);
            let x = [c - HW, c + HW];
            holes.push([[c - HW - 0.12, c + HW + 0.12], [y0 - 0.1, y0]]);
            holes.push([x, [y0, y1]]);
            if brick {
                holes.push(flat_arch(k, wi, rub, x, y1, zf, seed.wrapping_add(200 + f * 10 + j as u32)));
            } else {
                let m = if stucco { &mut k.trim } else { &mut k.dress };
                dressings(m, x, [y0, y1], zf, true);
            }
            sill(k, [c - HW - 0.12, c + HW + 0.12], y0, zf, seed ^ (0x41 + f * 3 + j as u32), stucco);
            sash(k, x, [y0, y1], zf, seed.wrapping_add(60 + f * 5 + j as u32));
        }
    }
    // The walling, the openings cut through it.
    if face == Face::Stone {
        ashlar(k, [xa, xa + w], [PL, eave], [zf - T, zf], &holes, seed);
    } else {
        cut(&mut k.wall[wi], [xa, xa + w], [PL, eave], [zf - T, zf], &holes);
    }
    // A stucco front's plain band under the eaves and a rusticated ground
    // floor's top course.
    if stucco {
        add(&mut k.trim, bx([xa, xa + w], [eave - 0.32, eave - 0.2], [zf - 0.02, zf + 0.03]));
    }
    // The rooms.
    let mut ys = vec![[PL, U_SILL - 0.75]];
    for f in 1..floors {
        let y0 = U_SILL - 0.75 + (f - 1) as f32 * STOREY;
        ys.push([y0, (y0 + STOREY).min(eave)]);
    }
    rooms(k, [xa + 0.05, xa + w - 0.05], &ys, zf);
}

/// The roof: slates laid course by course on the front slope (each a
/// tilted plate resting on the course under it), a plain deck behind,
/// ridge tiles, a lead flashing over each party wall, the verges.
#[allow(clippy::too_many_arguments)]
fn roof(k: &mut Kit, x: [f32; 2], zf: f32, eave: f32, pitch: f32, parties: &[f32], seed: u32, dens: f32) -> f32 {
    let tp = pitch.tan();
    let (sp, cp) = pitch.sin_cos();
    let ze = zf + OV;
    let ye = eave + 0.06;
    let yr = ye + ze * tp;
    let s_len = ze / cp;
    let at = |xx: f32, s: f32, o: f32| [xx, yr - sp * s + cp * o, cp * s + sp * o];
    let at_b = |xx: f32, s: f32, o: f32| [xx, yr - sp * s + cp * o, -(cp * s + sp * o)];
    let (xa, xb) = (x[0] - 0.14, x[1] + 0.14);
    let tilted = |size: [f32; 3], a: f32, c: [f32; 3]| {
        let mut m = b::cbox(size, [0.0; 3]);
        sh::rotate(&mut m, [1.0, 0.0, 0.0], a);
        geo::transform(&mut m, [1.0; 3], c);
        m
    };
    // The decks (the back one in slate, plain).
    let ls = s_len + 0.06;
    add(&mut k.deck, tilted([xb - xa, 0.03, ls], pitch, at((xa + xb) * 0.5, ls * 0.5, -0.016)));
    add(&mut k.slate[1], tilted([xb - xa, 0.04, ls], PI - pitch, at_b((xa + xb) * 0.5, ls * 0.5, 0.0)));
    // The slates.
    let (g, t, sw) = (0.22f32, 0.016f32, 0.29f32);
    let dl = (t / g).asin();
    let s_eave = s_len + 0.06;
    let courses = (s_eave / g).ceil() as u32;
    // The under-eaves course.
    add(&mut k.slate[1], tilted([xb - xa, t, g], pitch, at((xa + xb) * 0.5, s_eave - g * 0.5, t * 1.5)));
    for c in 0..courses {
        let s_bot = s_eave - c as f32 * g;
        if s_bot < 0.08 {
            break;
        }
        let len = (s_bot - (s_bot - 2.0 * g).max(0.0)).max(0.08);
        let mid = s_bot - len * 0.5;
        let o = t * 0.5 + len * 0.5 * dl.sin();
        if dens < 0.5 {
            add(&mut k.slate[(c % 3) as usize], tilted([xb - xa, t, len], pitch - dl, at((xa + xb) * 0.5, mid, o)));
            continue;
        }
        let off = if c % 2 == 0 { 0.0 } else { sw * 0.5 };
        let mut xx = xa - off;
        let mut i = 0u32;
        while xx < xb {
            let (a, e) = (xx.max(xa), (xx + sw).min(xb));
            if e - a > 0.04 {
                let id = c * 1013 + i;
                let r = sh::rnd(seed, id, 1);
                let q = if r < 0.45 { 0 } else if r < 0.75 { 1 } else { 2 };
                let jo = sh::range(seed, id, 2, -0.002, 0.003);
                let ja = sh::range(seed, id, 3, -0.006, 0.006);
                let wj = e - a - 0.006 - sh::range(seed, id, 4, 0.0, 0.006);
                let c = at((a + e) * 0.5, mid + sh::range(seed, id, 5, -0.006, 0.006), o + jo);
                if sh::rnd(seed, id, 6) < 0.28 {
                    // A chipped slate (round 2): a corner of its lower,
                    // exposed edge broken off (real geometry, its own shadow).
                    let (hw, hl) = (wj * 0.5, len * 0.5);
                    let (c1, c2) = (sh::range(seed, id, 7, 0.025, 0.07), sh::range(seed, id, 8, 0.02, 0.06));
                    let poly = if sh::rnd(seed, id, 9) < 0.5 {
                        vec![[-hw, -hl], [hw, -hl], [hw, hl - c1], [hw - c2, hl], [-hw, hl]]
                    } else {
                        vec![[-hw, -hl], [hw, -hl], [hw, hl], [-hw + c2, hl], [-hw, hl - c1]]
                    };
                    let mut m = b::extrude_axes(&poly, -t * 0.5, t * 0.5, [[1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]]);
                    sh::rotate(&mut m, [1.0, 0.0, 0.0], pitch - dl + ja);
                    geo::transform(&mut m, [1.0; 3], c);
                    add(&mut k.slate[q], m);
                } else {
                    add(&mut k.slate[q], tilted([wj, t, len], pitch - dl + ja, c));
                }
            }
            xx += sw;
            i += 1;
        }
    }
    // The ridge: angled tiles in short lengths.
    let ridge_y = |z: f32| yr - z.abs() * tp;
    let lip = 0.04;
    let prof = [[-0.17, ridge_y(0.17) - yr + lip], [0.0, lip + 0.012], [0.17, ridge_y(0.17) - yr + lip], [0.17, ridge_y(0.17) - yr + lip + 0.03], [0.0, lip + 0.045], [-0.17, ridge_y(0.17) - yr + lip + 0.03]];
    let nt = ((xb - xa) / 0.45).ceil() as u32;
    for i in 0..nt {
        let (a, e) = (xa + (xb - xa) * i as f32 / nt as f32 + 0.005, xa + (xb - xa) * (i + 1) as f32 / nt as f32 - 0.005);
        add(&mut k.ridge, b::sweep(&[[a, yr, 0.0], [e, yr, 0.0]], &prof, true));
    }
    // A lead flashing over each party wall, front and back.
    for px in parties {
        add(&mut k.lead, b::bar(at(*px, 0.05, 0.04), at(*px, s_eave - 0.03, 0.04), 0.22, 0.028));
        add(&mut k.lead, b::bar(at_b(*px, 0.05, 0.05), at_b(*px, s_eave - 0.03, 0.05), 0.22, 0.028));
    }
    // The verges: a dark undercloak along each gable under the slates.
    for vx in [x[0] - 0.08, x[1] + 0.08] {
        add(&mut k.ridge, b::bar(at(vx, 0.0, -0.01), at(vx, s_eave, -0.01), 0.14, 0.05));
        add(&mut k.ridge, b::bar(at_b(vx, 0.0, -0.02), at_b(vx, s_eave, -0.02), 0.14, 0.05));
    }
    yr
}

/// A brick stack astride the ridge at `x`: corbelled courses, a cement cap,
/// three clay pots, a lead apron where it meets the slates.
fn chimney(k: &mut Kit, x: f32, yr: f32, pitch: f32, seed: u32) {
    let (hx, hz) = (0.7, 0.42);
    let top = yr + 1.45;
    add(&mut k.stack, bx([x - hx, x + hx], [yr - 1.6, top - 0.32], [-hz, hz]));
    add(&mut k.stack, bx([x - hx - 0.05, x + hx + 0.05], [top - 0.32, top - 0.24], [-hz - 0.05, hz + 0.05]));
    add(&mut k.stack, bx([x - hx - 0.1, x + hx + 0.1], [top - 0.24, top - 0.14], [-hz - 0.1, hz + 0.1]));
    add(&mut k.stack, bx([x - hx - 0.03, x + hx + 0.03], [top - 0.14, top], [-hz - 0.03, hz + 0.03]));
    add(&mut k.plinth, bx([x - hx + 0.03, x + hx - 0.03], [top, top + 0.07], [-hz + 0.03, hz - 0.03]));
    for (i, dx) in [-0.44f32, 0.0, 0.44].into_iter().enumerate() {
        let h = 0.66 + sh::range(seed, i as u32, 1, -0.06, 0.08);
        let prof = [[0.0, 0.0], [0.15, 0.0], [0.15, 0.08], [0.12, 0.12], [0.105, h - 0.08], [0.13, h - 0.05], [0.13, h], [0.095, h], [0.09, h - 0.25], [0.0, h - 0.25]];
        let mut p = b::revolve(&prof, 14);
        geo::transform(&mut p, [1.0; 3], [x + dx, top + 0.06, 0.0]);
        add(&mut k.pots, p);
        let mut soot = sh::disc(0.09, top + 0.06 + h - 0.02, 10, true);
        geo::transform(&mut soot, [1.0; 3], [x + dx, 0.0, 0.0]);
        add(&mut k.soot, soot);
    }
    // The lead apron down the front slope and the back gutter behind.
    let (sp, cp) = pitch.sin_cos();
    let s = hz / cp;
    let at = |s: f32, o: f32| [x, yr - sp * s + cp * o, cp * s + sp * o];
    add(&mut k.lead, b::bar(at(s - 0.04, 0.04), at(s + 0.28, 0.04), 2.0 * hx + 0.14, 0.02));
    let at_b = |s: f32, o: f32| [x, yr - sp * s + cp * o, -(cp * s + sp * o)];
    add(&mut k.lead, b::bar(at_b(s - 0.04, 0.04), at_b(s + 0.28, 0.04), 2.0 * hx + 0.14, 0.02));
}

fn push(parts: &mut Vec<Part>, m: Mesh, s: Surface) {
    if !m.indices.is_empty() {
        parts.push(Part::surfaced(m, s));
    }
}

fn mul(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] * b[0], a[1] * b[1], a[2] * b[2]]
}

/// The asset.
pub fn generate(v: &Values, seed: u32) -> Output {
    let count = v.f("count").round().clamp(1.0, 8.0) as u32;
    let w = v.f("width").clamp(5.4, 9.0);
    let depth = v.f("depth").clamp(7.0, 14.0);
    let floors = v.f("floors").round().clamp(1.0, 4.0) as u32;
    let pitch = v.f("pitch").clamp(22.0, 55.0).to_radians();
    let dens = v.density().clamp(0.2, 1.0);
    let mut k = Kit::default();
    let zf = depth * 0.5;
    let len = w * count as f32;
    let x = [-len * 0.5, len * 0.5];
    let eave = if floors == 1 { G_HEAD + 0.75 } else { EAVE2 + (floors - 2) as f32 * STOREY };

    // The houses.
    for i in 0..count as usize {
        house(&mut k, i, x[0] + i as f32 * w, w, zf, eave, floors, seed, dens, i + 1 == count as usize);
    }
    let first = ORDER[0].wall();
    let lastw = ORDER[(count as usize - 1) % ORDER.len()].wall();

    // The plinth along the front: blocks with a chipped capping course.
    let fw = Wall { o: [x[0], 0.0, zf], t: [1.0, 0.0, 0.0], n: [0.0, 0.0, 1.0] };
    course(&mut k.plinth, &mut k.joint, &fw, [-PL_OUT, len + PL_OUT], [0.0, PL - 0.1], [-T, PL_OUT], 1.3, seed ^ 0x101, 0.0);
    course(&mut k.plinth, &mut k.joint, &fw, [-PL_OUT - 0.025, len + PL_OUT + 0.025], [PL - 0.1, PL], [-T, PL_OUT + 0.025], 1.1, seed ^ 0x102, 0.35);
    // The ends and the back: plinth, walls, the gables up to the roof.
    let ends = [
        (Wall { o: [x[0], 0.0, -zf], t: [0.0, 0.0, 1.0], n: [-1.0, 0.0, 0.0] }, first),
        (Wall { o: [x[1], 0.0, zf], t: [0.0, 0.0, -1.0], n: [1.0, 0.0, 0.0] }, lastw),
    ];
    let tp = pitch.tan();
    let ye = eave + 0.06;
    let yr = ye + (zf + OV) * tp;
    for (e, (ew, wi)) in ends.iter().enumerate() {
        course(&mut k.plinth, &mut k.joint, ew, [T, 2.0 * zf - T], [0.0, PL - 0.1], [-T, PL_OUT], 1.3, seed ^ (0x111 + e as u32), 0.0);
        course(&mut k.plinth, &mut k.joint, ew, [T, 2.0 * zf - T], [PL - 0.1, PL], [-T, PL_OUT + 0.025], 1.1, seed ^ (0x121 + e as u32), 0.3);
        let m = &mut k.wall[*wi];
        add(m, ew.bx([T, 2.0 * zf - T], [PL, eave], [-T, 0.0]));
        // The gable: the wall's triangle up under the roof.
        let under = |z: f32| ye + (zf + OV - z.abs()) * tp - 0.04;
        let poly = [[-zf, eave], [zf, eave], [zf, under(zf)], [0.0, yr - 0.04], [-zf, under(-zf)]];
        let xs = if e == 0 { [x[0], x[0] + T] } else { [x[1] - T, x[1]] };
        add(m, b::extrude_axes(&poly, xs[0], xs[1], [[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]]));
    }
    let back = Wall { o: [x[1], 0.0, -zf], t: [-1.0, 0.0, 0.0], n: [0.0, 0.0, -1.0] };
    course(&mut k.plinth, &mut k.joint, &back, [-PL_OUT, len + PL_OUT], [0.0, PL], [-T, PL_OUT], 1.3, seed ^ 0x131, 0.0);
    add(&mut k.wall[0], back.bx([0.0, len], [PL, eave], [-T, 0.0]));

    // The roof, the party walls' flashings and stacks.
    let parties: Vec<f32> = (1..count).map(|i| x[0] + i as f32 * w).collect();
    let yr = roof(&mut k, x, zf, eave, pitch, &parties, seed ^ 0x201, dens);
    for (i, px) in parties.iter().enumerate() {
        chimney(&mut k, *px, yr, pitch, seed.wrapping_add(i as u32 * 131));
    }
    // The eaves: a black fascia and soffit, a half-round cast-iron gutter.
    let (xa, xb) = (x[0] - 0.14, x[1] + 0.14);
    add(&mut k.black, bx([xa, xb], [eave - 0.2, eave + 0.06], [zf + OV - 0.03, zf + OV]));
    add(&mut k.black, bx([xa, xb], [eave - 0.2, eave - 0.18], [zf, zf + OV]));
    add(&mut k.black, bx([xa, xb], [eave - 0.2, eave + 0.06], [-zf - OV, -zf - OV + 0.03]));
    add(&mut k.black, bx([xa, xb], [eave - 0.2, eave - 0.18], [-zf - OV, -zf]));
    let mut gp = arc([0.0, 0.0], 0.072, PI, 2.0 * PI, 10);
    gp.extend(arc([0.0, 0.0], 0.064, 2.0 * PI, PI, 10));
    for (zg, flip) in [(zf + OV + 0.075, false), (-zf - OV - 0.075, true)] {
        let path = if flip { [[xb, eave - 0.02, zg], [xa, eave - 0.02, zg]] } else { [[xa, eave - 0.02, zg], [xb, eave - 0.02, zg]] };
        add(&mut k.black, b::sweep(&path, &gp, true));
    }
    // The downpipes: at the ends and at every party wall but the first.
    let mut pipes = vec![x[0] + 0.16, x[1] - 0.16];
    pipes.extend(parties.iter().skip(1).copied());
    for px in pipes {
        downpipe(&mut k, px, zf, eave);
    }

    // The materials.
    let red = v.color("color");
    // Brick (round 2): larger units in pale lime mortar a little over 1.5 cm
    // wide, drawn by the masonry recipe's joint cover (number 8) so the
    // joints still read as fine pale lines when a joint is under a pixel (at
    // the reference's distance the 1 cm joints faded into the brick); a wide
    // spread from burnt dark to orange units.
    let bricks = |a: [f32; 3], mortar: [f32; 3], dark: [f32; 3], var: f32, o: u32| {
        sp::BRICK_RED_COMMON
            .set_colour(0, a)
            .set_colour(1, mortar)
            .set_colour(2, dark)
            .number(0, (0.215 - 0.05) / 0.95)
            .number(1, (0.085 - 0.03) / 0.97)
            .number(2, (0.016 - 0.002) / 0.048)
            .number(5, var)
            .number(6, 0.3)
            .number(8, 1.0)
            .seed(seed ^ o)
    };
    let mortar = [0.4, 0.38, 0.34];
    let pale = |rgb: [f32; 3], o: u32| {
        sp::ROCK_LIMESTONE.variant(0).extras(0).set_colour(0, rgb).set_colour(1, mul(rgb, [0.93, 0.93, 0.94])).set_colour(2, mul(rgb, [1.05, 1.05, 1.04])).number(1, 0.0).number(3, 0.6).number(7, 0.0).seed(seed ^ o).scale(1.0)
    };
    // Weathered stone: the limestone's mottle and pits stronger (sills,
    // plinth, steps, the ashlar's blocks).
    let aged = |rgb: [f32; 3], o: u32| {
        sp::ROCK_LIMESTONE.variant(0).extras(0).set_colour(0, rgb).set_colour(1, mul(rgb, [0.88, 0.88, 0.9])).set_colour(2, mul(rgb, [1.08, 1.08, 1.06])).number(1, 0.0).number(3, 0.75).number(7, 0.25).seed(seed ^ o).scale(1.0)
    };
    let stone = [0.465, 0.4, 0.335];
    let grey = [0.255, 0.226, 0.2];
    let paint = |rgb: [f32; 3], o: u32| sp::FINISH_PAINTED_CREAM.tint(rgb).seed(seed ^ o);
    let mut parts = Vec::new();
    push(&mut parts, std::mem::take(&mut k.wall[0]), bricks(red, mortar, mul(red, [0.45, 0.5, 0.7]), 0.75, 1));
    push(&mut parts, std::mem::take(&mut k.wall[1]), pale(stone, 2));
    push(&mut parts, std::mem::take(&mut k.wall[2]), bricks([0.34, 0.2, 0.09], [0.46, 0.43, 0.37], [0.085, 0.072, 0.058], 0.75, 3));
    // Stucco (round 2): a little darker, the plaster recipe's stucco stipple
    // (fine pores), the trowel's broad ripple stronger (its cracks drew as
    // marble veins at this distance, so none).
    let stucco = |rgb: [f32; 3], o: u32| sp::PLASTER_WHITE.tint(rgb).variant(1).number(1, 0.9).seed(seed ^ o);
    push(&mut parts, std::mem::take(&mut k.wall[3]), stucco([0.36, 0.348, 0.33], 4));
    push(&mut parts, std::mem::take(&mut k.wall[4]), bricks([0.25, 0.13, 0.095], mortar, [0.13, 0.06, 0.048], 0.75, 5));
    // Rubbed brick: fine, even, a little lighter and oranger than the walling.
    push(&mut parts, std::mem::take(&mut k.rubbed[0]), pale([0.31, 0.13, 0.072], 6));
    push(&mut parts, std::mem::take(&mut k.rubbed[1]), pale([0.24, 0.08, 0.05], 7));
    push(&mut parts, k.mortar, pale(mortar, 8));
    push(&mut parts, k.dress, aged(stone, 9));
    push(&mut parts, k.trim, sp::PLASTER_WHITE.tint([0.42, 0.41, 0.39]).variant(1).number(1, 0.5).seed(seed ^ 10));
    // The ashlar's joints (round 2): a soft lime grey, not dark lines.
    push(&mut parts, k.joint, pale(mul(stone, [0.68, 0.68, 0.67]), 11));
    push(&mut parts, k.plinth, aged(grey, 12));
    // Slate (round 2): three greys further apart and a shade darker, the
    // stone's grain and pits up (mineral spots), each slate's own.
    let slate = |c: [f32; 3], o: u32| sp::ROCK_SLATE.set_colour(0, c).set_colour(1, mul(c, [0.78, 0.8, 0.84])).set_colour(2, mul(c, [1.22, 1.2, 1.15])).number(7, 0.3).seed(seed ^ o).scale(0.3);
    for (i, (m, c)) in k.slate.into_iter().zip([[0.09, 0.092, 0.1], [0.124, 0.126, 0.136], [0.064, 0.066, 0.074]]).enumerate() {
        push(&mut parts, m, slate(c, 13 + i as u32));
    }
    push(&mut parts, k.deck, sp::WALL_PAINT_MATTE.tint([0.03, 0.03, 0.032]).seed(seed ^ 16));
    push(&mut parts, k.ridge, slate([0.055, 0.055, 0.06], 17));
    // Old lead (round 2): dull, dark, mottled grey.
    push(&mut parts, k.lead, sp::ROCK_SLATE.set_colour(0, [0.085, 0.09, 0.095]).set_colour(1, [0.06, 0.064, 0.068]).set_colour(2, [0.12, 0.125, 0.13]).seed(seed ^ 18).scale(0.5));
    push(&mut parts, k.stack, bricks([0.21, 0.085, 0.055], [0.38, 0.35, 0.31], [0.1, 0.05, 0.04], 0.7, 19));
    push(&mut parts, k.pots, aged([0.33, 0.12, 0.068], 20));
    // Gloss white (round 2): an aged off-white, so the bars don't glare.
    push(&mut parts, k.white, paint([0.5, 0.49, 0.46], 21));
    let doors = [[0.045, 0.055, 0.07], [0.15, 0.17, 0.11], [0.085, 0.022, 0.022], [0.025, 0.025, 0.025], [0.09, 0.03, 0.025]];
    for (i, (m, c)) in k.door.into_iter().zip(doors).enumerate() {
        push(&mut parts, m, paint(c, 22 + i as u32));
    }
    push(&mut parts, k.black, paint([0.02, 0.02, 0.021], 27));
    push(&mut parts, k.brass, sp::METAL_STEEL_POLISHED.tint([0.55, 0.4, 0.18]).seed(seed ^ 28));
    // The rooms (round 2): a dim interior, not black.
    push(&mut parts, k.dark, sp::WALL_PAINT_MATTE.tint([0.085, 0.078, 0.07]).seed(seed ^ 29));
    push(&mut parts, k.soot, sp::WALL_PAINT_MATTE.tint([0.02, 0.019, 0.018]).seed(seed ^ 39));
    push(&mut parts, k.curtain, sp::CLOTH_KNIT_JERSEY.tint([0.52, 0.5, 0.45]).seed(seed ^ 30).scale(0.3));
    let leafy = |c: [f32; 3], o: u32| sp::GROUND_MOSS_CARPET.set_colour(0, c).set_colour(1, mul(c, [1.5, 1.45, 1.2])).set_colour(2, mul(c, [0.5, 0.55, 0.55])).seed(seed ^ o).scale(0.12);
    for (i, (m, c)) in k.leaves.into_iter().zip([[0.04, 0.085, 0.022], [0.07, 0.12, 0.03]]).enumerate() {
        push(&mut parts, m, leafy(c, 31 + i as u32));
    }
    for (i, (m, c)) in k.flowers.into_iter().zip([[0.6, 0.1, 0.25], [0.72, 0.72, 0.68], [0.26, 0.13, 0.42], [0.55, 0.05, 0.07]]).enumerate() {
        push(&mut parts, m, sp::PLASTER_WHITE.tint(c).seed(seed ^ (33 + i as u32)));
    }
    // The lanterns' panes (round 2): glass drawn as a tinted gloss (the one
    // see-through part stays the windows), the lamp's warm grey through it.
    push(&mut parts, k.lamp, sp::GLASS_WINDOW_CLEAR.set_colour(0, [0.42, 0.4, 0.33]).extras(0).seed(seed ^ 37));
    // The glass last (drawn over what is behind it): old window glass, a
    // faint grey-blue, a little bloom in it (round 2: the panes read as
    // black holes; now a soft sheen over the dim rooms).
    push(&mut parts, k.glass, sp::GLASS_WINDOW_CLEAR.set_colour(0, [0.5, 0.56, 0.62]).extras(0).number(0, 0.5).number(1, 0.22).number(2, 0.25).seed(seed ^ 38).scale(3.0).see_through());
    Output::Parts(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make(d: f32) -> Output {
        generate(&Values::defaults(&PARAMS).with_density(d), 7)
    }

    #[test]
    fn the_terrace_stands_on_the_ground_within_its_budget() {
        let (hi, lo) = (make(1.0), make(evani_procedural::detail::Density::for_tier("low")));
        let (t_hi, t_lo) = (hi.triangles(), lo.triangles());
        println!("buildings-townhouses: {t_hi} triangles at High, {t_lo} at Low");
        assert!(hi.is_valid() && t_hi < 260_000 && t_lo < t_hi, "{t_hi} / {t_lo}");
        let (lo_b, hi_b) = hi.bounds().unwrap_or(([0.0; 3], [0.0; 3]));
        assert!(lo_b[1] > -1e-3, "base at {}", lo_b[1]);
        // Two storeys: eaves at 8.6 m, the ridge near 12 m, the pots over it.
        assert!(hi_b[1] > 12.5 && hi_b[1] < 15.5, "top at {}", hi_b[1]);
        // Five houses of 6.4 m.
        assert!(hi_b[0] - lo_b[0] > 31.0 && hi_b[0] - lo_b[0] < 34.0, "length {}", hi_b[0] - lo_b[0]);
        let Output::Parts(ps) = &hi else { panic!("parts") };
        let see = ps.iter().filter(|p| p.surface.map(|s| s.blend).unwrap_or(false)).count();
        assert_eq!(see, 1, "one glazing part");
    }

    #[test]
    fn a_long_tall_terrace_keeps_its_budget() {
        let mut v = Values::defaults(&PARAMS);
        for (name, x) in [("floors", 4.0), ("count", 8.0), ("width", 9.0), ("depth", 14.0)] {
            v.set(name, evani_procedural::spec::Value::F32(x));
        }
        let o = generate(&v, 3);
        assert!(o.is_valid() && o.triangles() < 600_000, "{} triangles", o.triangles());
        let (_, hi_b) = o.bounds().unwrap_or(([0.0; 3], [0.0; 3]));
        assert!(hi_b[1] > EAVE2 + 2.0 * STOREY, "top at {}", hi_b[1]);
    }

    #[test]
    fn brick_fronts_keep_their_mortar_lines_at_a_distance() {
        // Round 2 (Asset Studio column 37): at the reference's distance a
        // 1 cm joint is under a pixel and faded into the brick; the brick
        // parts draw their joints by cover (masonry number 8) instead.
        let Output::Parts(ps) = make(1.0) else { panic!("parts") };
        let bricks: Vec<_> = ps.iter().filter_map(|p| p.surface).filter(|s| s.block[0] == 7).collect();
        assert!(bricks.len() >= 4, "{} brick parts", bricks.len());
        for s in bricks {
            assert!(s.number_of(8) > 0.99, "joint cover {}", s.number_of(8));
            assert!(s.number_of(2) * 0.048 + 0.002 > 0.014, "joints {} m", s.number_of(2) * 0.048 + 0.002);
        }
    }

    #[test]
    fn one_house_of_one_storey_still_builds() {
        let mut v = Values::defaults(&PARAMS);
        for (name, x) in [("floors", 1.0), ("count", 1.0)] {
            v.set(name, evani_procedural::spec::Value::F32(x));
        }
        let o = generate(&v, 11);
        assert!(o.is_valid() && o.triangles() > 1000, "{} triangles", o.triangles());
    }
}
