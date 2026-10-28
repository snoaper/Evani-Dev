//! The lock generator.
//!
//! 1. Sort triangles into fur (a [`RegionFur`] entry), bare and gear.
//! 2. Scatter roots over the fur, area times density weighted, at least
//!    `spacing` of the even spacing apart; skip roots with gear straight
//!    above them (fur under a sleeve).
//! 3. Per root: length from its region, a random change, and the gear boost
//!    (`1 + (boost - 1) * (1 - smoothstep(0, band, distance to gear))`);
//!    flow from its region's [`Flow`] projected onto the skin and turned by
//!    noise; lift off the skin.
//! 4. Leading locks are picked `clump_mm` apart; every lock follows the
//!    nearest leader on the same side of the body.
//! 5. Each lock's own centre line bends under gravity (`droop`) and sideways
//!    (`curl`); then its tip is pulled towards its leader's line, more
//!    towards the tip (`clump`), and kept above its root's skin plane.
//! 6. The line becomes a tapered ribbon, rolled about its axis (`twist`),
//!    normals leaning towards the skin's so the locks light like the coat.

use evani_model::compile::{Instance, SolidMesh};
use evani_model::dmath;
use evani_model::mesh::Mesh;
use evani_model::meshcheck;
use evani_model::rng::Pcg32;
use evani_model::vec::{V3, add, cross, dot, len, mul, norm_or, sub};

use crate::grid::{Grid, closest_on_triangle, ray_hits};
use crate::noise;
use crate::params::{Flow, LockParams, MAX_LOCKS, MAX_TRIS, RegionFur};
use crate::surface::FurSurface;
use crate::{LockError, SOLID, lerr};

/// One lock as grown.
#[derive(Clone, Debug, PartialEq)]
pub struct Lock {
    /// Root on the skin (the ribbon starts slightly below it).
    pub root: V3,
    /// Skin normal at the root.
    pub normal: V3,
    /// Surface triangle it grows from.
    pub tri: u32,
    /// The [`RegionFur`] key that set its length.
    pub region: String,
    /// Length (metres) after the random change and the gear boost.
    pub length: f64,
    /// Gear mask at the root, 0 (far from gear) to 1 (touching gear).
    pub gear: f64,
    /// Index of the lock whose tip it follows (itself for a leader).
    pub leader: u32,
}

/// What a run did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    /// Locks asked for after the caps (count, [`MAX_LOCKS`], [`MAX_TRIS`]).
    pub wanted: usize,
    pub locks: usize,
    pub triangles: usize,
    pub vertices: usize,
    pub leaders: usize,
    /// Roots skipped because gear lies right above them.
    pub covered: usize,
    /// Locks shortened so they stop short of gear instead of passing into it.
    pub clipped: usize,
    /// Locks with a gear mask above one half.
    pub near_gear: usize,
    /// Fur area (square metres) and fur triangles on the surface.
    pub fur_area: f64,
    pub fur_tris: usize,
    pub gear_tris: usize,
}

/// The generator's output.
#[derive(Clone, Debug)]
pub struct FurLocks {
    /// The solid `fur_locks` in world space (origin 0, one plain
    /// instance); `mesh.tri_part` copies each root triangle's part.
    pub solid: SolidMesh,
    /// Per vertex: across the ribbon (0 or 1), along it (0 root, 1 tip).
    pub uv: Vec<[f64; 2]>,
    /// Per vertex: the strand direction (unit, root to tip), for a hair
    /// highlight.
    pub strand: Vec<V3>,
    pub locks: Vec<Lock>,
    pub stats: Stats,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Fur(usize),
    Bare,
    Gear,
}

fn smoothstep(e0: f64, e1: f64, x: f64) -> f64 {
    if e1 <= e0 {
        return if x < e0 { 0.0 } else { 1.0 };
    }
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Any unit vector square to `n`.
fn perpendicular(n: V3) -> V3 {
    let a = if n[0].abs() < 0.6 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    norm_or(cross(n, a), [0.0, 0.0, 1.0])
}

/// `v` turned by `angle` about the unit axis `k` (Rodrigues).
fn rotate(v: V3, k: V3, angle: f64) -> V3 {
    let (s, c) = dmath::sin_cos(angle);
    add(add(mul(v, c), mul(cross(k, v), s)), mul(k, dot(k, v) * (1.0 - c)))
}

fn finite(v: V3) -> bool {
    v.iter().all(|x| x.is_finite())
}

fn check_surface(s: &FurSurface) -> Result<(), LockError> {
    let nt = s.tris.len();
    if s.tri_region.len() != nt || s.tri_solid.len() != nt || s.tri_part.len() != nt {
        return Err(lerr(format!(
            "surface labels do not match its {nt} triangles (regions {}, solids {}, parts {})",
            s.tri_region.len(),
            s.tri_solid.len(),
            s.tri_part.len()
        )));
    }
    if !s.nrm.is_empty() && s.nrm.len() != s.pos.len() {
        return Err(lerr(format!("surface has {} normals for {} vertices", s.nrm.len(), s.pos.len())));
    }
    let nv = s.pos.len() as u32;
    if let Some(t) = s.tris.iter().position(|t| t.iter().any(|&i| i >= nv)) {
        return Err(lerr(format!("surface triangle {t} names a vertex past {nv}")));
    }
    if s.tri_region.iter().any(|&r| r as usize >= s.regions.len()) || s.tri_solid.iter().any(|&r| r as usize >= s.solids.len()) {
        return Err(lerr("surface names a region or solid past its tables"));
    }
    if !s.pos.iter().all(|p| finite(*p)) {
        return Err(lerr("surface has a vertex that is not a finite number"));
    }
    Ok(())
}

/// Grow the locks over `surface` (see the module notes). An empty result
/// (no fur, a count of 0) is not an error.
pub fn fur_locks(surface: &FurSurface, params: &LockParams) -> Result<FurLocks, LockError> {
    check_surface(surface)?;
    let pf = |n: &str| params.get(n).unwrap_or(0.0);
    let pm = |n: &str| params.m(n);
    let segs = (pf("kit.fur_locks.segments").round() as usize).clamp(3, 5);
    let tris_per = 2 * segs;
    let wanted = (pf("kit.fur_locks.count").round().max(0.0) as usize).min(MAX_LOCKS).min(MAX_TRIS / tris_per);
    let seed = pf("kit.fur_locks.seed").round().max(0.0) as u64;
    for r in &params.regions {
        if !(r.length_mm.is_finite()
            && r.length_mm > 0.0
            && r.density.is_finite()
            && r.density >= 0.0
            && r.width.is_finite()
            && r.width > 0.0)
        {
            return Err(lerr(format!("region `{}` needs a positive length and width and a density of 0 or more", r.key)));
        }
    }

    // 1. Triangle kinds, areas and face normals.
    let nt = surface.tris.len();
    let corner = |t: usize, k: usize| surface.pos[surface.tris[t][k] as usize];
    let mut kind = Vec::with_capacity(nt);
    let mut fnrm = Vec::with_capacity(nt);
    let mut area = Vec::with_capacity(nt);
    for t in 0..nt {
        let (solid, region) = (surface.solid_of(t), surface.region_of(t));
        let entry = params.region_for(solid, region).and_then(|r| params.regions.iter().position(|x| x.key == r.key));
        kind.push(match entry {
            Some(e) => Kind::Fur(e),
            None if params.bare.iter().any(|b| b == region) => Kind::Bare,
            None => Kind::Gear,
        });
        let c = cross(sub(corner(t, 1), corner(t, 0)), sub(corner(t, 2), corner(t, 0)));
        let a = len(c);
        area.push(0.5 * a);
        fnrm.push(if a > 0.0 { mul(c, 1.0 / a) } else { [0.0, 1.0, 0.0] });
    }
    let mut stats = Stats { wanted, ..Stats::default() };

    // Gear triangles in a grid, for the boost and the cover check.
    let band = pm("kit.fur_locks.gear_band_mm");
    let cover = pm("kit.fur_locks.cover_mm");
    let mut gear = Grid::new(band.max(cover).max(0.01));
    for t in 0..nt {
        if kind[t] == Kind::Gear && area[t] > 0.0 {
            let (a, b, c) = (corner(t, 0), corner(t, 1), corner(t, 2));
            let lo = [a[0].min(b[0]).min(c[0]), a[1].min(b[1]).min(c[1]), a[2].min(b[2]).min(c[2])];
            let hi = [a[0].max(b[0]).max(c[0]), a[1].max(b[1]).max(c[1]), a[2].max(b[2]).max(c[2])];
            gear.insert_box(lo, hi, t as u32);
            stats.gear_tris += 1;
        }
    }
    let has_gear = stats.gear_tris > 0;
    let mut cand = Vec::new();
    let gear_distance = |p: V3, cand: &mut Vec<u32>| -> f64 {
        if band <= 0.0 || !has_gear {
            return f64::INFINITY;
        }
        gear.near(p, band, cand);
        let mut best = f64::INFINITY;
        for &g in cand.iter() {
            let g = g as usize;
            let q = closest_on_triangle(p, corner(g, 0), corner(g, 1), corner(g, 2));
            best = best.min(len(sub(q, p)));
        }
        best
    };
    let covered = |p: V3, n: V3, cand: &mut Vec<u32>| -> bool {
        if cover <= 0.0 || !has_gear {
            return false;
        }
        let end = add(p, mul(n, cover));
        let lo = [p[0].min(end[0]), p[1].min(end[1]), p[2].min(end[2])];
        let hi = [p[0].max(end[0]), p[1].max(end[1]), p[2].max(end[2])];
        gear.in_box(lo, hi, cand);
        cand.iter().any(|&g| {
            let g = g as usize;
            ray_hits(p, n, cover, corner(g, 0), corner(g, 1), corner(g, 2)).is_some()
        })
    };

    // 2. Roots.
    let mut cdf = Vec::new();
    let mut fur_tris = Vec::new();
    let mut total = 0.0;
    for t in 0..nt {
        if let Kind::Fur(e) = kind[t] {
            stats.fur_tris += 1;
            stats.fur_area += area[t];
            let w = area[t] * params.regions[e].density;
            if w > 0.0 {
                total += w;
                cdf.push(total);
                fur_tris.push(t);
            }
        }
    }
    let mut locks: Vec<Lock> = Vec::new();
    let mut lock_entry: Vec<usize> = Vec::new();
    if wanted > 0 && total > 0.0 {
        let even = (total / wanted as f64).sqrt();
        let rmin = pf("kit.fur_locks.spacing") * even;
        let mut roots = Grid::new(rmin.max(1e-4));
        let mut rng = Pcg32::new(seed, 0x6675_725f_6c6f_636b);
        let attempts = wanted * 10 + 64;
        let mut near = Vec::new();
        for _ in 0..attempts {
            if locks.len() >= wanted {
                break;
            }
            let u = rng.next_f64() * total;
            let k = cdf.partition_point(|&c| c <= u).min(fur_tris.len() - 1);
            let t = fur_tris[k];
            let (mut r1, mut r2) = (rng.next_f64(), rng.next_f64());
            if r1 + r2 > 1.0 {
                (r1, r2) = (1.0 - r1, 1.0 - r2);
            }
            let r0 = 1.0 - r1 - r2;
            let p = add(add(mul(corner(t, 0), r0), mul(corner(t, 1), r1)), mul(corner(t, 2), r2));
            let n = if surface.nrm.is_empty() {
                fnrm[t]
            } else {
                let tri = surface.tris[t];
                let v = add(
                    add(mul(surface.nrm[tri[0] as usize], r0), mul(surface.nrm[tri[1] as usize], r1)),
                    mul(surface.nrm[tri[2] as usize], r2),
                );
                let v = norm_or(v, fnrm[t]);
                if dot(v, fnrm[t]) > 0.0 { v } else { fnrm[t] }
            };
            let Kind::Fur(e) = kind[t] else { continue };
            let entry = &params.regions[e];
            let r = rmin / entry.density.max(1e-6).sqrt();
            if r > 0.0 {
                roots.near(p, r, &mut near);
                if near.iter().any(|&j| len(sub(locks[j as usize].root, p)) < r) {
                    continue;
                }
            }
            if covered(add(p, mul(n, 1e-4)), n, &mut cand) {
                stats.covered += 1;
                continue;
            }
            roots.insert_point(p, locks.len() as u32);
            locks.push(Lock { root: p, normal: n, tri: t as u32, region: entry.key.clone(), length: 0.0, gear: 0.0, leader: 0 });
            lock_entry.push(e);
        }
    }

    // 3. Lengths, flow and the per-lock random choices.
    let jitter = pf("kit.fur_locks.length_jitter");
    let boost = pf("kit.fur_locks.gear_boost");
    let flow_noise = pf("kit.fur_locks.flow_noise");
    let flow_scale = pm("kit.fur_locks.flow_scale_mm").max(1e-3);
    let lift = pf("kit.fur_locks.lift").clamp(0.0, 1.0) * 0.45 * std::f64::consts::PI;
    let default_flow = params.flow();
    let mut rng = Pcg32::new(seed, 0x6c6f_636b_5f72_6e64);
    struct Grow {
        dir: V3,
        side: V3,
        curl: f64,
        roll: f64,
        width: f64,
    }
    let width_mm = pm("kit.fur_locks.width_mm");
    let mut grow = Vec::with_capacity(locks.len());
    for (i, (l, &e)) in locks.iter_mut().zip(lock_entry.iter()).enumerate() {
        let entry: &RegionFur = &params.regions[e];
        let (u_len, u_curl, u_roll, u_width) = (rng.next_f64(), rng.next_f64(), rng.next_f64(), rng.next_f64());
        let d = gear_distance(l.root, &mut cand);
        l.gear = if d.is_finite() { 1.0 - smoothstep(0.0, band, d) } else { 0.0 };
        l.length = entry.length_mm * 0.001 * (1.0 + jitter * (2.0 * u_len - 1.0)) * (1.0 + (boost - 1.0) * l.gear);
        if l.gear > 0.5 {
            stats.near_gear += 1;
        }
        let base = match entry.flow {
            None => default_flow,
            Some(Flow::Dir(v)) => v,
            Some(Flow::Away(c)) => sub(l.root, c),
            Some(Flow::Toward(c)) => sub(c, l.root),
        };
        let n = l.normal;
        let t = norm_or(sub(base, mul(n, dot(n, base))), perpendicular(n));
        let wobble = flow_noise * noise::fbm(mul(l.root, 1.0 / flow_scale), seed ^ 0x0f10_4a11);
        let t = rotate(t, n, wobble);
        let (ls, lc) = dmath::sin_cos(lift);
        let dir = norm_or(add(mul(t, lc), mul(n, ls)), n);
        let side = norm_or(cross(n, t), perpendicular(t));
        grow.push(Grow {
            dir,
            side,
            curl: 2.0 * u_curl - 1.0,
            roll: 2.0 * u_roll - 1.0,
            width: width_mm * entry.width * (0.8 + 0.4 * u_width),
        });
        l.leader = i as u32;
    }

    // 4. Clump leaders and followers.
    let clump_r = pm("kit.fur_locks.clump_mm");
    let mut leaders = Grid::new(clump_r.max(1e-4));
    let mut is_leader = vec![false; locks.len()];
    let mut near = Vec::new();
    for i in 0..locks.len() {
        leaders.near(locks[i].root, clump_r, &mut near);
        if !near.iter().any(|&j| len(sub(locks[j as usize].root, locks[i].root)) < clump_r) {
            leaders.insert_point(locks[i].root, i as u32);
            is_leader[i] = true;
            stats.leaders += 1;
        }
    }
    for i in 0..locks.len() {
        if is_leader[i] {
            continue;
        }
        leaders.near(locks[i].root, 2.0 * clump_r, &mut near);
        let mut best = (f64::INFINITY, i as u32);
        for &j in near.iter() {
            let lj = &locks[j as usize];
            let d = len(sub(lj.root, locks[i].root));
            // Same side of the body only: never pull a tip through a crease.
            if d < best.0 && dot(lj.normal, locks[i].normal) > 0.5 {
                best = (d, j);
            }
        }
        locks[i].leader = best.1;
    }

    // 5. Centre lines.
    let droop = pf("kit.fur_locks.droop");
    let curl = pf("kit.fur_locks.curl");
    let clump = pf("kit.fur_locks.clump");
    let gravity = [0.0, -1.0, 0.0];
    let own: Vec<Vec<V3>> = locks
        .iter()
        .zip(grow.iter())
        .map(|(l, g)| {
            let step = l.length / segs as f64;
            let mut pts = Vec::with_capacity(segs + 1);
            // Start just under the skin so no gap shows at the root.
            pts.push(sub(l.root, mul(l.normal, 0.15 * g.width)));
            for k in 1..=segs {
                let s = k as f64 / segs as f64;
                let bend = add(mul(gravity, droop * s), mul(g.side, curl * g.curl * s * s));
                let d = norm_or(add(g.dir, bend), g.dir);
                let prev = pts[k - 1];
                pts.push(add(prev, mul(d, step)));
            }
            pts
        })
        .collect();
    let mut lines = own.clone();
    for (i, l) in locks.iter().enumerate() {
        let j = l.leader as usize;
        if j != i {
            for k in 1..=segs {
                let s = k as f64 / segs as f64;
                let w = clump * s * s.sqrt();
                lines[i][k] = add(own[i][k], mul(sub(own[j][k], own[i][k]), w));
            }
        }
        // Keep the line above the root's skin plane (rising slowly).
        for (k, p) in lines[i].iter_mut().enumerate().skip(1) {
            let s = k as f64 / segs as f64;
            let floor = s * (0.05 * l.length + 0.002);
            let h = dot(sub(*p, l.root), l.normal);
            if h < floor {
                *p = add(*p, mul(l.normal, floor - h));
            }
        }
    }

    // No poke-through: a line that would pass into gear is shrunk towards
    // its root so it ends just short of the gear (same triangle count).
    if has_gear {
        for pts in lines.iter_mut() {
            let mut walked = 0.0;
            let mut stop = None;
            for k in 1..pts.len() {
                let d = sub(pts[k], pts[k - 1]);
                let seg = len(d);
                if seg <= 0.0 {
                    continue;
                }
                let dir = mul(d, 1.0 / seg);
                let (a, b) = (pts[k - 1], pts[k]);
                gear.in_box([a[0].min(b[0]), a[1].min(b[1]), a[2].min(b[2])], [a[0].max(b[0]), a[1].max(b[1]), a[2].max(b[2])], &mut cand);
                let hit = cand.iter().filter_map(|&g| {
                    let g = g as usize;
                    ray_hits(a, dir, seg, corner(g, 0), corner(g, 1), corner(g, 2))
                });
                if let Some(t) = hit.fold(None, |m: Option<f64>, t| Some(m.map_or(t, |m| m.min(t)))) {
                    stop = Some(walked + t);
                    break;
                }
                walked += seg;
            }
            if let Some(at) = stop {
                let total: f64 = pts.windows(2).map(|w| len(sub(w[1], w[0]))).sum();
                // Keep a stub of at least a fifth of the lock.
                let f = ((at - 0.002) / total.max(1e-9)).clamp(0.2, 1.0);
                let root = pts[0];
                for p in pts.iter_mut().skip(1) {
                    *p = add(root, mul(sub(*p, root), f));
                }
                stats.clipped += 1;
            }
        }
    }

    // 6. Ribbons.
    let tip = pf("kit.fur_locks.tip_width");
    let twist = pf("kit.fur_locks.twist");
    let mut mesh = Mesh::default();
    let mut uv = Vec::new();
    let mut strand = Vec::new();
    for (i, l) in locks.iter().enumerate() {
        let pts = &lines[i];
        let g = &grow[i];
        let base = mesh.pos.len() as u32;
        let part = surface.tri_part.get(l.tri as usize).copied().unwrap_or(0);
        for k in 0..=segs {
            let s = k as f64 / segs as f64;
            let t = norm_or(sub(pts[(k + 1).min(segs)], pts[k.saturating_sub(1)]), g.dir);
            let b0 = norm_or(cross(t, l.normal), g.side);
            let b = rotate(b0, t, twist * g.roll * (0.4 + 0.6 * s));
            let face = norm_or(cross(b, t), l.normal);
            let half = 0.5 * g.width * (1.0 - (1.0 - tip) * s);
            let nv = norm_or(add(mul(face, 0.45), mul(l.normal, 0.55)), l.normal);
            for (side, u) in [(-1.0, 0.0), (1.0, 1.0)] {
                mesh.pos.push(add(pts[k], mul(b, side * half)));
                mesh.nrm.push(nv);
                uv.push([u, s]);
                strand.push(t);
            }
        }
        for k in 0..segs as u32 {
            let a = base + 2 * k;
            mesh.tris.push([a, a + 1, a + 3]);
            mesh.tris.push([a, a + 3, a + 2]);
            mesh.tri_part.push(part);
            mesh.tri_part.push(part);
        }
    }
    stats.locks = locks.len();
    stats.triangles = mesh.tris.len();
    stats.vertices = mesh.pos.len();
    if stats.triangles > MAX_TRIS {
        return Err(lerr(format!("{} triangles is over the {MAX_TRIS} cap", stats.triangles)));
    }
    if !mesh.pos.iter().all(|p| finite(*p)) {
        return Err(lerr("a lock vertex is not a finite number"));
    }
    let report =
        meshcheck::check(&mesh.pos, &mesh.tris, &meshcheck::Options { ref_normals: None, expect_closed: false, self_intersections: false });
    let solid = SolidMesh {
        solid: SOLID.to_string(),
        mesh,
        instances: vec![Instance { solid: SOLID.to_string(), t: [0.0; 3], mirror: false }],
        report,
    };
    Ok(FurLocks { solid, uv, strand, locks, stats })
}
