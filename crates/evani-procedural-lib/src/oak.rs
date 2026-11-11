//! Oak trees, photoreal (category 5; the Asset Studio oak session): a broad,
//! open-grown oak. A short massive trunk, finely fissured (narrow carved
//! fissures meandering up it, opening and closing into small plates, and the
//! bark paint's fine furrows kept drawn from across a field), swells at its
//! foot into six to eight rounded roots (each its own tube, closed inside
//! the trunk, running out low and sinking into the ground) and forks at
//! about 45% of the height. A few unequal scaffold limbs: big low ones off
//! the trunk at their own heights and three leaders from the fork (a thick
//! crooked middle one, a spreading one, a smaller low one), each bending,
//! drooping at its end and tapering unevenly from a flared collar whose
//! start is closed inside its parent; each splits twice more inside a
//! broad, lifted dome. Leaves hang in small sprays strung along every
//! branchlet and along the big limbs' outer parts, thick on the crown's
//! shell, thin inside and under it and none by the trunk below the fork:
//! flat cards in the catalogue's oak leaf (foliage, leaf), each cut by the
//! cut-out recipe (cut.wgsl kind 7, a game listing "cutout") to four shoot
//! tips of five small lobed leaves, every leaf its own shade and tilt, the
//! sprays deep in a clump darker. The wood is the catalogue's oak bark, its
//! uvs laid along every branch so the furrows run with the wood. `grove`
//! adds smaller, narrower oaks behind the first (seen from +z), each its
//! own tree.
//!
//! Inspired by SeedThree's and threejs-trees' ideas (a trunk and recursive
//! branches, leaves at the tips) and three.js (MIT) TubeGeometry's rings
//! along a path; written fresh here.

use evani_procedural::geo::{Mesh, normalize};
use evani_procedural::surface::{Surface, presets};
use evani_procedural::{Output, Part, Values, noise, shapes as sh};

type V3 = [f32; 3];

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn mul(a: V3, k: f32) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}

fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn length(a: V3) -> f32 {
    dot(a, a).sqrt()
}

/// A seeded unit-ish vector (each component -1..1).
fn jitter(s: u32, i: u32, k: u32) -> V3 {
    [sh::rnd(s, i, k) * 2.0 - 1.0, sh::rnd(s, i, k + 1) * 2.0 - 1.0, sh::rnd(s, i, k + 2) * 2.0 - 1.0]
}

/// The leaf colour's default (linear): a summer oak's yellow-olive green.
pub const LEAVES: [f32; 3] = [0.08, 0.13, 0.032];
/// The bark colour's default (linear): a little greyer and paler than the
/// catalogue's oak bark colour A, as an old oak's weathered plates.
pub const BARK: [f32; 3] = [0.058, 0.053, 0.045];
/// The bark's patches (colour C, linear): moss and green lichen.
const MOSS: [f32; 3] = [0.07, 0.085, 0.032];
/// Metres along a branch per bark uv unit (round it, one turn is one unit).
const BARK_SCALE: f32 = 3.0;

/// The shape of one tree.
#[derive(Debug, Clone, Copy)]
pub struct Shape {
    /// Height (m).
    pub height: f32,
    /// Crown width over height.
    pub spread: f32,
    /// Main limbs below the fork.
    pub limbs: u32,
    /// Leaf sprays per clump, over the default.
    pub leaves: f32,
    /// How crooked the limbs grow (0 straight .. 1 gnarled).
    pub gnarl: f32,
    /// Tier density (1 High .. 0.3 Lite).
    pub density: f32,
    /// Wood detail: the trunk's, roots' and branches' sides round (1 High;
    /// the oak asset uses its tier density; the rivers set many oaks and
    /// ask for less, down to 0.04), apart from how many branches, leaves
    /// and segments grow, which follow `density`.
    pub wood: f32,
    /// Leaf card size over the default (1: the oak asset's own; the rivers'
    /// light oaks ask for bigger cards, so their few sprays still close the
    /// crown at the same triangle count).
    pub card: f32,
}

/// Wood (trunk and branches) and leaf cards, with their uvs (metres).
#[derive(Debug, Default)]
pub struct Tree {
    pub wood: Mesh,
    pub wood_uv: Vec<[f32; 2]>,
    pub leaves: Mesh,
    pub leaf_uv: Vec<[f32; 2]>,
}

impl Tree {
    fn append(&mut self, o: &Tree, at: V3, yaw: f32, scale: f32) {
        let (sy, cy) = yaw.sin_cos();
        let tr = |v: &[f32; 6]| -> [f32; 6] {
            let p = [v[0] * cy + v[2] * sy, v[1], -v[0] * sy + v[2] * cy];
            let n = [v[3] * cy + v[5] * sy, v[4], -v[3] * sy + v[5] * cy];
            [p[0] * scale + at[0], p[1] * scale + at[1], p[2] * scale + at[2], n[0], n[1], n[2]]
        };
        // The bark's v is metres (it scales with the tree); its u is turns
        // round the branch and a leaf card's uv is its spray cell: neither
        // scales.
        for (dst, duv, src, suv, vk) in [(&mut self.wood, &mut self.wood_uv, &o.wood, &o.wood_uv, scale), (&mut self.leaves, &mut self.leaf_uv, &o.leaves, &o.leaf_uv, 1.0)] {
            let base = dst.vertices.len() as u32;
            dst.vertices.extend(src.vertices.iter().map(tr));
            dst.indices.extend(src.indices.iter().map(|i| i + base));
            duv.extend(suv.iter().map(|u| [u[0], u[1] * vk]));
        }
    }
}

/// A tube along `path` with `radius[i]` at each point and `sides` round,
/// its rings carried along the path so it never twists; the bark's uv runs
/// round it (u: one turn is one uv unit, as the bark recipe wants, so its
/// pattern wraps without a seam) and along it (v, metres from `v0`); both
/// are written in metres over [`BARK_SCALE`] (layout_given divides by it).
/// `per` is about how many metres round the tube's thickest ring one turn
/// of the pattern takes (so a limb's collar has no coarser bark than the
/// trunk). `disp(i, a, r)` moves a ring point out (m) at ring `i`, angle `a`.
#[allow(clippy::too_many_arguments)]
fn tube(t: &mut Tree, path: &[V3], radius: &[f32], sides: u32, v0: f32, per: f32, disp: &dyn Fn(usize, f32, f32) -> f32) {
    let n = path.len().min(radius.len());
    if n < 2 {
        return;
    }
    let s = sides.clamp(3, 96) as usize;
    let tangent = |i: usize| normalize(sub(path[(i + 1).min(n - 1)], path[i.saturating_sub(1)]));
    let t0 = tangent(0);
    let mut nrm = normalize(cross(t0, if t0[1].abs() < 0.9 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] }));
    let mut grid: Vec<V3> = Vec::with_capacity(n * s);
    // Each ring's v: metres along the path.
    let mut vs: Vec<f32> = Vec::with_capacity(n);
    let mut along = v0;
    // Along a tapering tube the pattern shortens as the girth does, so the
    // bark keeps its proportions to the tip instead of stretching.
    let r_max = radius[..n].iter().fold(0.0f32, |a, r| a.max(*r));
    for i in 0..n {
        let tg = tangent(i);
        nrm = normalize(sub(nrm, mul(tg, dot(nrm, tg))));
        let bin = cross(tg, nrm);
        if i > 0 {
            along += length(sub(path[i], path[i - 1])) * r_max / radius[i].max(radius[i - 1]).max(r_max * 0.25).max(1e-4);
        }
        vs.push(along);
        for k in 0..s {
            let a = k as f32 / s as f32 * sh::TAU;
            let d = add(mul(nrm, a.cos()), mul(bin, a.sin()));
            let r = (radius[i] + disp(i, a, radius[i])).max(0.0);
            grid.push(add(path[i], mul(d, r)));
        }
    }
    // Whole turns of the bark pattern round the tube: more on a thick trunk
    // or collar (its furrows stay a few centimetres apart), one on a twig.
    let max_r = radius[..n].iter().fold(0.0f32, |a, r| a.max(*r));
    let turns = (sh::TAU * max_r / per).round().max(1.0);
    // Along the tube the pattern keeps the same proportion to its width round
    // it, so a thin branch's bark is not stretched into stripes.
    let vk = (per * turns / (sh::TAU * max_r.max(1e-3))).max(1.0);
    let base = t.wood.vertices.len() as u32;
    let row = s as u32 + 1;
    let at = |i: usize, k: usize| grid[i * s + k % s];
    for i in 0..n {
        for k in 0..=s {
            let p = at(i, k);
            // Normals from the grid itself (round the ring and along it), so
            // the carved furrows shade and the seam is smooth.
            let du = sub(at(i, k + 1), at(i, k + s - 1));
            let dv = sub(at((i + 1).min(n - 1), k), at(i.saturating_sub(1), k));
            let mut nn = normalize(cross(dv, du));
            let out = sub(p, path[i]);
            if dot(nn, out) < 0.0 {
                nn = mul(nn, -1.0);
            }
            if length(out) < 1e-5 {
                nn = tangent(i);
            }
            t.wood.vertices.push([p[0], p[1], p[2], nn[0], nn[1], nn[2]]);
            t.wood_uv.push([k as f32 / s as f32 * turns * BARK_SCALE, vs[i] * vk]);
        }
    }
    for i in 0..(n - 1) as u32 {
        for k in 0..s as u32 {
            let a = base + i * row + k;
            // Counter-clockwise from outside: along the path by round it.
            let (b, c, d) = (a + row, a + row + 1, a + 1);
            let pa = &t.wood.vertices[a as usize];
            let pb = &t.wood.vertices[b as usize];
            let pd = &t.wood.vertices[d as usize];
            let f = cross(sub([pb[0], pb[1], pb[2]], [pa[0], pa[1], pa[2]]), sub([pd[0], pd[1], pd[2]], [pa[0], pa[1], pa[2]]));
            if dot(f, [pa[3], pa[4], pa[5]]) >= 0.0 {
                t.wood.indices.extend_from_slice(&[a, b, c, a, c, d]);
            } else {
                t.wood.indices.extend_from_slice(&[a, c, b, a, d, c]);
            }
        }
    }
}

/// The crown: centre and half sizes of the ellipsoid the branches fill.
#[derive(Debug, Clone, Copy)]
struct Crown {
    c: V3,
    e: V3,
}

impl Crown {
    /// How far from `p` along unit `d` to the crown's surface (0 outside).
    fn reach(&self, p: V3, d: V3) -> f32 {
        let q = [(p[0] - self.c[0]) / self.e[0], (p[1] - self.c[1]) / self.e[1], (p[2] - self.c[2]) / self.e[2]];
        let dd = [d[0] / self.e[0], d[1] / self.e[1], d[2] / self.e[2]];
        let (a, b, c) = (dot(dd, dd), 2.0 * dot(q, dd), dot(q, q) - 1.0);
        let disc = b * b - 4.0 * a * c;
        if disc < 0.0 || a < 1e-9 {
            return 0.0;
        }
        ((-b + disc.sqrt()) / (2.0 * a)).max(0.0)
    }

    /// The outward direction of the crown's shell at `p`.
    fn out(&self, p: V3) -> V3 {
        let e2 = [self.e[0] * self.e[0], self.e[1] * self.e[1], self.e[2] * self.e[2]];
        normalize([(p[0] - self.c[0]) / e2[0], (p[1] - self.c[1]) / e2[1], (p[2] - self.c[2]) / e2[2]])
    }
}

/// One branch to grow.
#[derive(Debug, Clone, Copy)]
struct Branch {
    at: V3,
    dir: V3,
    len: f32,
    r: f32,
    level: u32,
    id: u32,
    /// How much the branch swells where it leaves its parent.
    collar: f32,
}

/// Grow `b` and everything off it: its tube, its children, and at the last
/// level its clumps of leaves.
fn grow(t: &mut Tree, b: Branch, crown: &Crown, sp: &Shape, s: u32, cards: &mut u32) {
    let d = sp.density.clamp(0.25, 1.0);
    let wd = wood_detail(sp);
    let h = sp.height;
    let seed = s.wrapping_add(b.id.wrapping_mul(7919));
    // Segments and sides by level (fewer on the lower tiers).
    let (segs, sides) = match b.level {
        0 => (((b.len / (h * 0.045)).ceil() as u32).clamp(6, 16), (10.0 * wd.sqrt()).round().max(4.0) as u32),
        1 => (((b.len / (h * 0.06)).ceil() as u32).clamp(4, 9), (6.0 * wd.sqrt()).round().max(4.0) as u32),
        _ => (5, 3),
    };
    let up = [0.0, 1.0, 0.0];
    // A twig where no leaves may grow (low by the trunk) is not drawn: no
    // bare spikes.
    if b.level >= 2 && leaf_low(crown, sp, add(b.at, mul(b.dir, b.len))) < 0.05 && leaf_low(crown, sp, add(b.at, mul(b.dir, b.len * 0.5))) < 0.05 {
        return;
    }
    let mut pts = vec![b.at];
    let mut dir = b.dir;
    let step = b.len / segs as f32;
    // Crooked growth: a seeded turn at every segment and a slow bend to one
    // side and back, a gentle rise towards the light near the parent and a
    // droop under its own weight further out (the long low limbs most),
    // outward towards the crown's edge.
    // The middle leader (id 100) grows straighter, carrying the crown's top.
    let central = b.id == 100;
    let kink = sp.gnarl * if central { 0.6 } else { 1.0 } * match b.level {
        0 => 0.45,
        1 => 0.42,
        _ => 0.62,
    };
    let side = normalize(cross(b.dir, if b.dir[1].abs() < 0.9 { up } else { [1.0, 0.0, 0.0] }));
    let (phase, wave) = (sh::rnd(seed, 0, 5) * sh::TAU, sp.gnarl * sh::range(seed, 0, 6, 0.08, 0.2) * if b.level == 0 { 1.5 } else { 1.0 });
    let flat = 1.0 - b.dir[1].clamp(0.0, 1.0);
    for k in 0..segs {
        let p = *pts.last().unwrap_or(&b.at);
        let u = k as f32 / segs as f32;
        let rise = match b.level {
            0 => 0.05 - 0.13 * u * flat,
            1 => 0.04 - 0.09 * u,
            _ => 0.02,
        };
        let bend = mul(side, (phase + u * 5.0).sin() * wave);
        dir = normalize(add(add(add(dir, mul(jitter(seed, k, 3), kink)), bend), add(mul(up, rise), mul(crown.out(p), 0.04))));
        pts.push(add(p, mul(dir, step)));
    }
    let n = pts.len();
    let tip = match b.level {
        0 => 0.28,
        1 => 0.26,
        _ => 0.3,
    };
    // A collar flaring into its parent, then tapering fast near the base and
    // slower further out, a little thicker and thinner along the way.
    let lumpy = sh::rnd(seed, 0, 7) * sh::TAU;
    let radius_at = |u: f32| {
        b.r * (tip + (1.0 - tip) * (1.0 - u).powf(1.35)) * (1.0 + b.collar * (-u * 9.0).exp()) * (1.0 + 0.06 * (lumpy + u * 11.0).sin())
    };
    // The tube: closed at its start by a round end buried in its parent (no
    // open ring where it leaves it), and at its end by a point a little past
    // it.
    let back = normalize(sub(pts[0], pts[1.min(n - 1)]));
    let mut path = vec![add(b.at, mul(back, b.r * 0.45)), add(b.at, mul(back, b.r * 0.2))];
    let mut radii = vec![0.0, b.r * 0.85];
    path.extend_from_slice(&pts);
    radii.extend((0..n).map(|i| radius_at(i as f32 / (n - 1) as f32)));
    if let (Some(last), Some(prev)) = (pts.last().copied(), pts.get(n - 2).copied()) {
        path.push(add(last, mul(normalize(sub(last, prev)), b.r * tip * 1.5)));
        radii.push(0.0);
    }
    // Bark relief on the limbs too (shallower than the trunk's): ridges
    // running along the limb.
    let bump = move |i: usize, a: f32, r: f32| -> f32 {
        if b.level > 1 {
            return 0.0;
        }
        let f = noise::value2(a * r / 0.04, i as f32 * 0.35, seed) - 0.5;
        r * 0.12 * f
    };
    tube(t, &path, &radii, sides, 0.0, 1.8, &bump);
    let point_at = |u: f32| {
        let f = u * (n - 1) as f32;
        let (j, k) = (f.floor() as usize, f.fract());
        let a = pts[j.min(n - 1)];
        let c = pts[(j + 1).min(n - 1)];
        add(a, mul(sub(c, a), k))
    };
    if b.level >= 2 {
        // The leaves: small sprays strung along the branchlet's outer part,
        // the most at its end, so the crown is many little puffs with the
        // wood and the sky between them.
        let rc = h * 0.06;
        for (k, (u, share)) in [(1.0, 1.0), (0.74, 0.8), (0.48, 0.6)].into_iter().enumerate() {
            clump(t, point_at(u), rc * (0.7 + 0.3 * share), share, crown, sp, seed ^ (0x55 * k as u32), cards);
        }
        return;
    }
    // Children: spread along the branch's outer part (its inner part bare
    // wood), turning round it.
    let kids = match b.level {
        0 => (6.0 * d.powf(0.4)).round() as u32,
        _ => (6.0 * d.powf(0.5)).round() as u32,
    };
    let first = if b.level == 0 { 0.2 } else { 0.15 };
    for j in 0..kids {
        // A big limb's first child is a fork: nearly as thick, long, at a
        // narrow angle, so the scaffold divides unevenly rather than
        // sprouting thin side shoots.
        let fork_kid = b.level == 0 && j == 0;
        let u = if fork_kid { sh::range(seed, j, 17, 0.32, 0.5) } else { first + (1.0 - first) * (j as f32 + 0.3 + 0.5 * sh::rnd(seed, j, 10)) / kids as f32 };
        let u = u.min(0.97);
        let at = point_at(u);
        let pd = normalize(sub(point_at((u + 0.05).min(1.0)), point_at((u - 0.05).max(0.0))));
        // A direction off the parent: an angle from it, turned round it.
        let side = normalize(cross(pd, if pd[1].abs() < 0.9 { up } else { [1.0, 0.0, 0.0] }));
        let other = cross(pd, side);
        let az = j as f32 * 2.4 + sh::rnd(seed, j, 11) * 1.2;
        let ang = if fork_kid { sh::range(seed, j, 12, 0.35, 0.55) } else { sh::range(seed, j, 12, 0.55, 0.95) };
        let off = add(mul(side, az.cos()), mul(other, az.sin()));
        let mut cd = normalize(add(mul(pd, ang.cos()), mul(off, ang.sin())));
        // Never steeply down (no hanging spikes).
        if cd[1] < -0.2 {
            cd = normalize([cd[0], -0.2, cd[2]]);
        }
        // Out to the light: out of the crown's middle and sideways (oaks
        // spread their wood flat), up only a little.
        let flat_out = normalize([at[0] - crown.c[0] + 1e-3, 0.0, at[2] - crown.c[2]]);
        // (The middle leader's children reach up, building the dome's top.)
        let lift = if central { 0.08 } else { 0.04 };
        cd = normalize(add(add(cd, mul(crown.out(at), 0.22)), add(mul(flat_out, 0.25), mul(up, lift))));
        let want = b.len * if fork_kid { 0.8 } else { sh::range(seed, j, 13, 0.5, 0.75) * (1.15 - 0.45 * u) };
        let room = crown.reach(at, cd).max(h * 0.05);
        let len = want.min(room * 0.92).max(h * 0.05);
        let r = (radius_at(u) * if fork_kid { 0.8 } else if b.level == 0 { 0.66 } else { 0.55 }).max(h * 0.0015);
        let id = b.id.wrapping_mul(31).wrapping_add(j + 1);
        grow(t, Branch { at, dir: cd, len, r, level: b.level + 1, id, collar: 0.45 }, crown, sp, s, cards);
    }
    if b.level == 0 {
        // Short leafy twigs straight off the big limb's outer two thirds,
        // so leaves hang along the scaffold too, not only at its ends.
        for j in 0..5u32 {
            let u = sh::range(seed, j, 14, 0.3, 0.92);
            let at = point_at(u);
            let td = normalize(add(add(jitter(seed, j, 15), mul(up, 0.35)), mul(crown.out(at), 0.4)));
            let r = (radius_at(u) * 0.2).max(h * 0.0015);
            let id = b.id.wrapping_mul(31).wrapping_add(50 + j);
            grow(t, Branch { at, dir: td, len: h * sh::range(seed, j, 16, 0.07, 0.12), r, level: 2, id, collar: 0.45 }, crown, sp, s, cards);
        }
    }
}

/// How far leaves may grow at `p` (0 none .. 1): none low by the trunk (it
/// stands clear up to the fork), lower out at the crown's edge, where the
/// long limbs droop.
fn leaf_low(crown: &Crown, sp: &Shape, p: V3) -> f32 {
    let out = ((p[0] - crown.c[0]).powi(2) + (p[2] - crown.c[2]).powi(2)).sqrt() / crown.e[0].max(1e-3);
    let floor = 0.5 - 0.3 * out.min(1.0);
    noise::smoothstep(floor - 0.06, floor + 0.06, p[1] / sp.height.max(1.0))
}

/// A clump of leaf sprays round `centre`, `rc` across: cards on its outer
/// and upper side, each facing out of the clump, their light normals bent
/// out of the clump and the crown, so every clump shades as a leafy mound
/// (lit on its top, dark beneath) and the crown as a heap of them.
#[allow(clippy::too_many_arguments)]
fn clump(t: &mut Tree, centre: V3, rc: f32, share: f32, crown: &Crown, sp: &Shape, seed: u32, cards: &mut u32) {
    let d = sp.density.clamp(0.25, 1.0);
    // Thick on the crown's outer shell, thin deep inside it and under it, so
    // the limbs show through from below and the side.
    let q = [(centre[0] - crown.c[0]) / crown.e[0], (centre[1] - crown.c[1]) / crown.e[1], (centre[2] - crown.c[2]) / crown.e[2]];
    let shell = 0.5 + 0.5 * noise::smoothstep(0.3, 0.75, length(q));
    // No leaves low by the trunk (it stands clear up to the fork); lower
    // out at the crown's edge, where the long limbs droop.
    let low = leaf_low(crown, sp, centre);
    let count = (25.0 * share * sp.leaves * d.sqrt() * shell * low).round() as u32;
    // Fewer, larger cards on the lower tiers keep the crown as full.
    let size = sp.height * 0.045 / d.powf(0.25) * sp.card.clamp(0.5, 3.0);
    let up = [0.0, 1.0, 0.0];
    let cout = crown.out(centre);
    for k in 0..count {
        // Mostly the clump's outer, upper side; some through its middle.
        let dir = normalize(add(add(jitter(seed, k, 21), mul(cout, 0.55)), mul(up, 0.3)));
        let rr = sh::range(seed, k, 24, 0.15, 1.0).sqrt();
        // Clumps a little flatter than round: foliage in layers.
        let at = add(centre, mul([dir[0], dir[1] * 0.6, dir[2]], rc * rr));
        // Facing out of the clump, but every spray its own way.
        let nf = normalize(add(mul(dir, 0.6), mul(jitter(seed, k, 27), 0.75)));
        let g = add(mul(up, 0.6), mul(jitter(seed, k, 30), 0.8));
        let mut vup = sub(g, mul(nf, dot(g, nf)));
        if length(vup) < 1e-3 {
            vup = sub([1.0, 0.0, 0.0], mul(nf, dot([1.0, 0.0, 0.0], nf)));
        }
        let vup = normalize(vup);
        let right = cross(vup, nf);
        let sz = size * sh::range(seed, k, 33, 0.8, 1.2);
        let half = mul(right, sz * 0.5);
        let top = mul(vup, sz);
        let base = sub(at, mul(vup, sz * 0.5));
        // The light normal: the clump's mound and the crown's shell.
        // Sprays deep in the clump turn their light normal down, so the
        // clump is lit on its outside and dark inside.
        let deep = (1.0 - rr) * 0.7;
        let ln = normalize(add(mul(add(add(mul(dir, 0.5), mul(crown.out(at), 0.4)), mul(nf, 0.2)), 1.0 - deep), mul(up, -deep)));
        let corners = [sub(base, half), add(base, half), add(add(base, half), top), add(sub(base, half), top)];
        // Each card its own cell of the spray pattern (cut.wgsl kind 7).
        let cell = (*cards % 211) as f32;
        let uvs = [[cell, 1.0], [cell + 1.0, 1.0], [cell + 1.0, 0.0], [cell, 0.0]];
        let b0 = t.leaves.vertices.len() as u32;
        for (p, uv) in corners.iter().zip(uvs) {
            // Each corner's normal bends out from the card's middle, so a
            // spray shades as a little cupped mound, not a flat card.
            let cn = normalize(add(ln, mul(normalize(sub(*p, at)), 0.45)));
            t.leaves.vertices.push([p[0], p[1], p[2], cn[0], cn[1], cn[2]]);
            t.leaf_uv.push(uv);
        }
        // Counter-clockwise seen from the face (right x up = the face).
        t.leaves.indices.extend_from_slice(&[b0, b0 + 1, b0 + 2, b0, b0 + 2, b0 + 3]);
        *cards += 1;
    }
}

/// The wood's detail: `wood` where it asks for less than `density` allows
/// (the oak asset passes its density, so it draws as before).
fn wood_detail(sp: &Shape) -> f32 {
    sp.wood.clamp(0.04, 1.0).min(sp.density.clamp(0.25, 1.0))
}

/// One oak, its base at the origin.
pub fn tree(sp: &Shape, s: u32) -> Tree {
    let h = sp.height.max(1.0);
    let mut t = Tree::default();
    let w = h * sp.spread.clamp(0.4, 2.5);
    // A broad dome, its underside lifted clear of the trunk's lower half.
    let crown = Crown { c: [0.0, h * 0.65, 0.0], e: [w * 0.5, h * 0.38, w * 0.5] };
    // The trunk: a gently leaning, swaying column to the fork.
    let fork = h * 0.45;
    let r0 = h * 0.08;
    let rings = ((fork / 0.11).ceil() as usize).clamp(12, 60);
    let lean = [sh::range(s, 1, 1, -1.0, 1.0) * h * 0.012, 0.0, sh::range(s, 1, 2, -1.0, 1.0) * h * 0.012];
    let mut path: Vec<V3> = (0..=rings)
        .map(|i| {
            // Rings closer together at the foot, where the roots curve.
            let y = -0.15 + (fork + 0.15) * (i as f32 / rings as f32).powf(1.3);
            let f = y / fork;
            let sway = |k: u32| (noise::value2(f * 2.2, k as f32 * 3.3, s) - 0.5) * h * 0.02 * f;
            [lean[0] * f * f + sway(1), y, lean[2] * f * f + sway(2)]
        })
        .collect();
    // An old oak's irregular taper: thinner and thicker by turns going up.
    let mut radius: Vec<f32> =
        path.iter().map(|p| { let f = (p[1] / fork).clamp(0.0, 1.0); r0 * (1.0 - 0.2 * f + 0.14 * (noise::value2(f * 2.5, 7.7, s) - 0.5)) }).collect();
    let top = path.last().copied().unwrap_or([0.0, fork, 0.0]);
    let r_top = radius.last().copied().unwrap_or(r0);
    // A low rounded top over the fork, hidden inside the leaders' collars.
    for (dy, k) in [(0.1, 0.9), (0.2, 0.0)] {
        path.push(add(top, [0.0, r_top * dy, 0.0]));
        radius.push(r_top * k);
    }
    // Buttresses: six to eight roots, each its own heading, girth and
    // reach. The trunk swells towards each root at its foot (the buttress)
    // and the root itself is a tube of its own (below), so it is round
    // across, with hollows between the roots.
    let nb = 6 + (sh::rnd(s, 2, 1) * 3.0) as u32;
    let roots: Vec<[f32; 3]> = (0..nb)
        .map(|j| [j as f32 / nb as f32 * sh::TAU + sh::rnd(s, j, 3) * 0.6, sh::range(s, j, 4, 0.75, 1.25), sh::range(s, j, 5, 0.75, 1.25)])
        .collect();
    let path_y: Vec<f32> = path.iter().map(|p| p[1]).collect();
    // Narrow fissures up the trunk about 14 cm apart, meandering, opening
    // and closing so the ridges between them merge and split into small
    // plates; carved into the mesh one vertex wide (128 sides at High), so
    // the light picks them out from across the field.
    let nf = (sh::TAU * r0 / 0.14).round().max(8.0);
    let bark = |i: usize, a: f32, r: f32| -> f32 {
        let y = path_y[i].max(0.0);
        let lobes = roots.iter().map(|rt| rt[1] * ((a - rt[0]).cos().max(0.0)).powi(3)).fold(0.0f32, f32::max);
        let foot = r * (-y / (r0 * 0.6)).exp() * (0.12 + 0.85 * lobes);
        // Broad bulges on one side or another, so the trunk is not a column.
        let bulge = r0 * 0.1 * (noise::value2(a / sh::TAU * 3.0 + 0.3, y / 1.6, s ^ 0x0e) - 0.5);
        let turn = a / sh::TAU * nf;
        let wander = (noise::value2(y / 0.9, turn * 0.25, s ^ 0x0a) - 0.5) * 1.6;
        let v = 1.0 - ((turn + wander).fract() - 0.5).abs() * 2.0;
        let open = noise::smoothstep(0.3, 0.6, noise::value2(y / 0.35, turn * 0.7, s ^ 0x0b));
        let fissure = v * v * open;
        let plate = noise::value2(turn * 1.3, y / 0.18, s ^ 0x0d) - 0.5;
        let swell = noise::value2(a * r0 / 0.6, y / 1.4, s ^ 0x0c) - 0.5;
        foot + bulge - r0 * 0.055 * fissure + r0 * 0.015 * plate + r0 * 0.05 * swell
    };
    let sides = (128.0 * wood_detail(sp).sqrt()).round().max(12.0) as u32;
    tube(&mut t, &path, &radius, sides, 0.0, 1.8, &bark);
    // The roots: each leaves the trunk's foot well inside its buttress,
    // runs out low over the ground, wandering a little, thinning, and sinks
    // into it two to four trunk radii out, its tip buried.
    let root_sides = (14.0 * wood_detail(sp).sqrt()).round().max(5.0) as u32;
    for (j, rt) in roots.iter().enumerate() {
        let j = j as u32;
        // The trunk's angle a points along (-sin a, 0, -cos a) at its foot.
        let (out, across) = ([-rt[0].sin(), 0.0, -rt[0].cos()], [-rt[0].cos(), 0.0, rt[0].sin()]);
        let wob = sh::range(s, j, 6, -1.0, 1.0);
        let n = 8;
        let pts: Vec<V3> = (0..=n)
            .map(|k| {
                let u = k as f32 / n as f32;
                let x = r0 * (0.3 + 2.3 * rt[2] * u);
                let y = r0 * 0.55 * rt[1] * (1.0 - u).powf(1.4) - 1.3 * u * u - 3.0 * (u - 0.65).max(0.0).powi(2);
                let side = r0 * 0.25 * wob * (u * 2.6).sin();
                [out[0] * x + across[0] * side, y, out[2] * x + across[2] * side]
            })
            .collect();
        let mut radii: Vec<f32> = (0..=n).map(|k| r0 * 0.7 * rt[1] * (1.0 - 0.45 * (k as f32 / n as f32).powf(1.2))).collect();
        // Closed at its start, inside the trunk (no open ring showing).
        let mut path = vec![[out[0] * r0 * 0.05, pts[0][1], out[2] * r0 * 0.05]];
        path.extend_from_slice(&pts);
        radii.insert(0, 0.0);
        tube(&mut t, &path, &radii, root_sides, 0.0, 1.8, &|_, _, _| 0.0);
    }
    // Limbs: a few big low ones off the trunk at their own heights below the
    // fork, then three unequal leaders from the fork itself (a thick, crooked
    // middle one, a spreading one and a smaller low one): scaffold limbs of
    // different sizes rather than a fan from one point.
    let mut cards = 0u32;
    let low = sp.limbs.min(12);
    // Every scaffold limb its own sector round the trunk (the leaders a third
    // of the way round from each other, the low limbs between them), so the
    // crown spreads evenly rather than leaning to one side.
    let total = low + 4;
    // Turned so that, seen from +z (the library's view), big limbs reach out
    // to both sides and the crown balances over the trunk.
    let base = -0.35 + (sh::rnd(s, 3, 60) - 0.5) * 0.4;
    let slots: Vec<f32> = (0..total).map(|k| base + (k as f32 + 0.3 + 0.4 * sh::rnd(s, k, 61)) * sh::TAU / total as f32).collect();
    let lows: Vec<usize> = (0..total).filter(|k| !(0..4).any(|j| j * total / 4 == *k)).map(|k| k as usize).collect();
    let mut limbs: Vec<Branch> = Vec::new();
    for i in 0..low {
        let f = sh::range(s, i, 40, 0.0, 1.0);
        let y = fork * (0.55 + 0.4 * (i as f32 + f) / low as f32);
        let az = slots[lows[i as usize]];
        let el = sh::range(s, i, 42, 0.1, 0.35) + 0.15 * (i as f32 / low as f32);
        let dir = normalize([az.cos() * el.cos(), el.sin(), az.sin() * el.cos()]);
        let at = [top[0] * (y / fork), y, top[2] * (y / fork)];
        let room = crown.reach(at, dir);
        let len = (room * sh::range(s, i, 43, 0.85, 0.97)).max(h * 0.2);
        limbs.push(Branch { at, dir, len, r: r0 * sh::range(s, i, 44, 0.38, 0.5), level: 0, id: i + 1, collar: 0.35 });
    }
    // Four leaders of much the same girth, leaning out round the trunk, so
    // the crown is a broad dome of several forks rather than one plume.
    let leaders = 4u32;
    for j in 0..leaders {
        let az = slots[(j * total / 4) as usize];
        let (el, rk, lk) = match j {
            0 => (sh::range(s, j, 51, 0.95, 1.08), 0.78, 0.85),
            1 => (sh::range(s, j, 51, 0.78, 0.9), 0.72, 0.92),
            2 => (sh::range(s, j, 51, 0.66, 0.78), 0.66, 0.92),
            _ => (sh::range(s, j, 51, 0.56, 0.68), 0.6, 0.9),
        };
        let dir = normalize([az.cos() * el.cos(), el.sin(), az.sin() * el.cos()]);
        // The first carries the trunk on (hardly a collar, so no cap shows at
        // the fork); the others leave the trunk's upper part at their own
        // heights, not all from one hub.
        let y = fork * [1.0, 0.92, 0.84, 0.76][j as usize];
        let at = if j == 0 { sub(top, [0.0, r_top * 0.3, 0.0]) } else { [top[0] * (y / fork), y, top[2] * (y / fork)] };
        let room = crown.reach(at, dir);
        let len = (room * lk).max(h * 0.25);
        let collar = if j == 0 { 0.0 } else { 0.3 };
        limbs.push(Branch { at, dir, len, r: r_top * rk, level: 0, id: 100 + j, collar });
    }
    for b in limbs {
        grow(&mut t, b, &crown, sp, s, &mut cards);
    }
    t
}

/// The extra trees of a grove: (x, z) in heights from the first, and scale.
const GROVE: [[f32; 3]; 4] = [[-0.62, -0.38, 0.74], [0.62, -0.55, 0.55], [-0.15, -1.1, 0.66], [1.0, -1.3, 0.6]];

/// The library asset: `grove` oaks (the first at the origin), wood in oak
/// bark and leaves in oak leaf cards.
pub fn make(v: &Values, s: u32) -> Output {
    let sp = Shape { height: v.f("height"), spread: v.f("spread"), limbs: v.f("branches").round().max(0.0) as u32, leaves: v.f("leaves").max(0.0), gnarl: v.f("gnarl").clamp(0.0, 1.0), density: v.density(), wood: v.density(), card: 1.0 };
    let mut all = tree(&sp, s);
    let n = (v.f("grove").round() as usize).clamp(1, GROVE.len() + 1);
    for (i, g) in GROVE.iter().take(n - 1).enumerate() {
        let k = i as u32 + 1;
        let si = s.wrapping_add(k.wrapping_mul(104_729));
        let sc = g[2] * sh::range(si, 0, 1, 0.92, 1.08);
        // Younger oaks: a little narrower and more crooked.
        let one = tree(&Shape { gnarl: (sp.gnarl * 1.1).min(1.0), spread: sp.spread * 0.72, ..sp }, si);
        let at = [(g[0] + sh::range(si, 0, 2, -0.06, 0.06)) * sp.height, 0.0, (g[1] + sh::range(si, 0, 3, -0.06, 0.06)) * sp.height];
        all.append(&one, at, sh::rnd(si, 0, 4) * sh::TAU, sc);
    }
    // A veteran oak's paint: narrow dark furrows round small matte plates
    // (about 7 cm by 25 cm on the trunk, 24 a turn of the pattern), kept
    // drawn down to a few pixels (far detail, nb.w) so the trunk reads as
    // finely fissured bark from across the field, not a smooth average; its
    // patches moss and green lichen.
    let bark: Surface =
        presets::BARK_OAK.tint(v.color("bark")).set_colour(2, MOSS).number(0, 1.0).number(1, 0.85).number(2, 0.469).number(4, 0.43).number(7, 1.0).seed(s).scale(BARK_SCALE);
    // Oak leaf sprays: the leaf variant cut to five lobed leaves (extras bit
    // 3), back-lit, no single-leaf veins (a card holds five leaves), no dry
    // edge (the card's uv cells would brown every leaf).
    let leaf: Surface = presets::LEAF_OAK.tint(v.color("color")).variant(0).extras(0b1001).number(0, 0.8).number(1, 0.9).number(2, 0.0).number(3, 0.42).number(4, 0.0).number(7, 0.12).seed(s).scale(1.0).cards();
    Output::Parts(vec![Part::surfaced_uv(all.wood, bark, all.wood_uv), Part::surfaced_uv(all.leaves, leaf, all.leaf_uv)])
}
