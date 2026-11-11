//! The pattern layouts on the CPU (batch 3 build plan 3.1, package B3-P1):
//! the same cell maths as `shaders/rc_layout.wgsl`, line for line, so its
//! tests can hold the layouts to their promises before any recipe builds on
//! them: every cell function covers the plane with no gap or overlap, a
//! unit keeps one id across its whole face, and the distance to the nearest
//! edge is continuous across joints (it changes no faster than the point
//! moves). Tools may call it too (a viewer's unit picker); no game does, so
//! no game downloads it.
//!
//! The hashes are the core's (`shaders/core.wgsl`): `rc_pcg`, `rc_hash2`,
//! `rc_hash3`, `rc_u01`, with WGSL's wrapping `u32` arithmetic.

/// One unit of a layout at the point asked about (WGSL `LayCell`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cell {
    /// The distance to the unit's nearest edge.
    pub e: f32,
    /// The unit's own number in `[0, 1)`.
    pub id: f32,
    /// Where the point is inside the unit (0..1 along, across; for shingles
    /// `uv[1]` is the lap).
    pub uv: [f32; 2],
    /// Which way the unit lies (0 along x, 1 along y), or the chequer colour.
    pub k: f32,
}

/// `rc_pcg`.
pub fn pcg(v: u32) -> u32 {
    let s = v.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let w = ((s >> ((s >> 28) + 4)) ^ s).wrapping_mul(277_803_737);
    (w >> 22) ^ w
}

/// `rc_hash2`.
pub fn hash2(c: [i32; 2], seed: u32) -> u32 {
    pcg(c[1] as u32 ^ pcg(c[0] as u32 ^ pcg(seed)))
}

/// `rc_hash3`.
pub fn hash3(c: [i32; 3], seed: u32) -> u32 {
    pcg(c[2] as u32 ^ hash2([c[0], c[1]], seed))
}

/// `rc_u01`.
pub fn u01(h: u32) -> f32 {
    (h >> 8) as f32 * (1.0 / 16_777_216.0)
}

fn fract(x: f32) -> f32 {
    x - x.floor()
}

fn step(edge: f32, x: f32) -> f32 {
    if x < edge { 0.0 } else { 1.0 }
}

/// `rc_lay_par`: x mod 2 for whole numbers.
pub fn par(x: f32) -> f32 {
    x - 2.0 * (x * 0.5).floor()
}

/// `rc_lay_rot45`.
pub fn rot45(p: [f32; 2]) -> [f32; 2] {
    [(p[0] + p[1]) * core::f32::consts::FRAC_1_SQRT_2, (p[1] - p[0]) * core::f32::consts::FRAC_1_SQRT_2]
}

/// `rc_lay_bond`: courses of units `u[0]` long and `u[1]` high; mode 0
/// running, 3 setts, 4 ashlar, else stack.
pub fn bond(p: [f32; 2], u: [f32; 2], m: u32, seed: u32) -> Cell {
    let row = (p[1] / u[1]).floor();
    let rh = u01(hash2([row as i32, 0], seed));
    let (mut len, mut off) = (u[0], 0.0);
    if m == 0 {
        off = 0.5 * u[0] * step(0.5, fract(row * 0.5));
    } else if m == 3 {
        off = rh * u[0];
    } else if m == 4 {
        len = u[0] * (0.7 + 0.6 * rh);
        off = rh * 5.0 * u[0];
    }
    let x = p[0] + off;
    let seg = (x / len).floor();
    let lx = x - seg * len;
    let ly = p[1] - row * u[1];
    Cell {
        e: ly.min(u[1] - ly).min(lx.min(len - lx)),
        id: u01(hash2([seg as i32, row as i32], seed.wrapping_add(1))),
        uv: [lx / len, ly / u[1]],
        k: 0.0,
    }
}

/// `rc_lay_hb`: strip `m` of the herringbone; `e < 0` when `q` is in neither
/// of its units.
pub fn hb(q: [f32; 2], n: f32, m: f32, seed: u32) -> Cell {
    let r = [q[0] - m * n, q[1] + m * n];
    let ky = r[1].floor();
    let kx = r[0].floor();
    let h = r[0] >= ky && r[0] < ky + n;
    let k = if h { ky } else { kx };
    let a = if h { [r[0] - k, r[1] - k] } else { [r[1] - k - 1.0, r[0] - k] };
    let ok = h || (r[1] >= kx + 1.0 && r[1] < kx + n + 1.0);
    let id = u01(hash3([k as i32, m as i32, i32::from(h)], seed));
    Cell {
        e: if ok { a[0].min(n - a[0]).min(a[1].min(1.0 - a[1])) } else { -1.0 },
        id,
        uv: [a[0] / n, a[1]],
        k: if h { 0.0 } else { 1.0 },
    }
}

/// `rc_lay_herring`: units `w` wide and `n * w` long.
pub fn herring(p: [f32; 2], w: f32, n: f32, seed: u32) -> Cell {
    let q = [p[0] / w, p[1] / w];
    let m = ((q[0] - q[1] + n + 1.0) / (2.0 * n)).floor();
    let mut c = hb(q, n, m, seed);
    if c.e < 0.0 {
        c = hb(q, n, m - 1.0, seed);
    }
    c.e = c.e.max(0.0) * w;
    c
}

/// `rc_lay_basket`: blocks of `n` units `w` wide, turned in turn.
pub fn basket(p: [f32; 2], w: f32, n: f32, seed: u32) -> Cell {
    let s = w * n;
    let bc = [(p[0] / s).floor(), (p[1] / s).floor()];
    let k = par(bc[0] + bc[1]);
    let l = [p[0] - bc[0] * s, p[1] - bc[1] * s];
    let a = if k > 0.5 { [l[1], l[0]] } else { l };
    let row = (a[1] / w).floor();
    let ly = a[1] - row * w;
    Cell {
        e: a[0].min(s - a[0]).min(ly.min(w - ly)),
        id: u01(hash3([bc[0] as i32, bc[1] as i32, row as i32], seed)),
        uv: [a[0] / s, ly / w],
        k,
    }
}

/// The chequer: [`basket`] with one unit a block (`rc_lay_basket(p, s, 1.0,
/// seed)`), square tiles `s` wide, `k` their colour.
pub fn chequer(p: [f32; 2], s: f32, seed: u32) -> Cell {
    basket(p, s, 1.0, seed)
}

/// The core's `rc_hex`: offset from the nearest hex centre (unit spacing,
/// pointy-top) and the centre.
pub fn hex_cell(p: [f32; 2]) -> [f32; 4] {
    let s = [1.0, 3f32.sqrt()];
    let h = [0.5, 3f32.sqrt() * 0.5];
    let ca = [((p[0] / s[0]).floor() + 0.5) * s[0], ((p[1] / s[1]).floor() + 0.5) * s[1]];
    let cb = [(((p[0] - h[0]) / s[0]).floor() + 0.5) * s[0] + h[0], (((p[1] - h[1]) / s[1]).floor() + 0.5) * s[1] + h[1]];
    let a = [p[0] - ca[0], p[1] - ca[1]];
    let b = [p[0] - cb[0], p[1] - cb[1]];
    if a[0] * a[0] + a[1] * a[1] < b[0] * b[0] + b[1] * b[1] {
        [a[0], a[1], ca[0], ca[1]]
    } else {
        [b[0], b[1], cb[0], cb[1]]
    }
}

/// `rc_lay_hex`: hexagons `s` apart.
pub fn hex(p: [f32; 2], s: f32, seed: u32) -> Cell {
    let h = hex_cell([p[0] / s, p[1] / s]);
    let a = [h[0].abs(), h[1].abs()];
    Cell {
        e: (0.5 - a[0].max(a[0] * 0.5 + a[1] * 3f32.sqrt() * 0.5)) * s,
        id: u01(hash2([(h[2] * 2.0).floor() as i32, (h[3] * 2.0).floor() as i32], seed)),
        uv: [h[0] + 0.5, h[1] + 0.5],
        k: 0.0,
    }
}

/// `rc_lay_sb`: course `r` at `x`: its free edge, the position along the
/// unit, the unit's id.
pub fn sb(r: f32, x: f32, u: [f32; 2], j: f32, seed: u32) -> [f32; 3] {
    let xo = x + 0.5 * u[0] * par(r);
    let s = (xo / u[0]).floor();
    let h = hash2([s as i32, r as i32], seed);
    [(r - j * u01(h)) * u[1], xo - s * u[0], u01(pcg(h))]
}

/// `rc_lay_shingle`: units `u[0]` wide in courses `u[1]` high, free edges
/// shortened up to `j` of a course.
pub fn shingle(p: [f32; 2], u: [f32; 2], j: f32, seed: u32) -> Cell {
    let mut r = (p[1] / u[1]).floor() + 1.0;
    let mut a = sb(r, p[0], u, j, seed);
    if a[0] > p[1] {
        r -= 1.0;
        a = sb(r, p[0], u, j, seed);
    }
    let t = sb(r + 1.0, p[0], u, j, seed)[0];
    Cell {
        e: a[1].min(u[0] - a[1]).min(p[1] - a[0]),
        id: a[2],
        uv: [a[1] / u[0], (p[1] - a[0]) / (t - a[0]).max(1e-6)],
        k: 0.0,
    }
}

/// `rc_lay_studs`: a dome of radius `r` on each lattice point `(i + 0.5) * t`.
pub fn studs(q: [f32; 2], t: [f32; 2], r: f32) -> f32 {
    let d = [q[0] - ((q[0] / t[0]).floor() + 0.5) * t[0], q[1] - ((q[1] / t[1]).floor() + 0.5) * t[1]];
    (1.0 - (d[0] * d[0] + d[1] * d[1]) / (r * r)).max(0.0)
}

/// `rc_lay_rim`: a stud every `t` along each side of a unit of size `u`, `i`
/// in from it.
pub fn rim(q: [f32; 2], u: [f32; 2], i: f32, t: f32, r: f32) -> f32 {
    let d = [q[0].min(u[0] - q[0]), q[1].min(u[1] - q[1])];
    let a = if d[0] < d[1] { [d[0], q[1]] } else { [d[1], q[0]] };
    let s = [a[0] - i, a[1] - ((a[1] / t).floor() + 0.5) * t];
    (1.0 - (s[0] * s[0] + s[1] * s[1]) / (r * r)).max(0.0)
}

/// `rc_lay_rivets`: riveted plates `2/s` by `1/s`; the rivet dome, the
/// distance to the seam, the plate's id.
pub fn rivets(q: [f32; 2], s: f32, seed: u32) -> [f32; 3] {
    let u = [2.0 / s, 1.0 / s];
    let c = bond(q, u, 0, seed);
    [rim([c.uv[0] * u[0], c.uv[1] * u[1]], u, u[1] / 12.0, u[1] / 6.0, u[1] * 0.03), c.e, c.id]
}

/// `rc_lay_stripe`: the distance to the nearest stripe's centre line.
pub fn stripe(x: f32, t: f32) -> f32 {
    (fract(x / t + 0.5) - 0.5).abs() * t
}

/// `rc_lay_grid`: the distance to the nearest of two sets of lines.
pub fn grid(p: [f32; 2], t: [f32; 2]) -> f32 {
    stripe(p[0], t[0]).min(stripe(p[1], t[1]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Points over a few units in every direction, off the lattice (so no
    /// sample sits exactly on a joint), both sides of zero.
    fn points(span: f32, n: i32) -> impl Iterator<Item = [f32; 2]> {
        (-n..n).flat_map(move |y| (-n..n).map(move |x| [(x as f32 + 0.317) * span / n as f32, (y as f32 + 0.129) * span / n as f32]))
    }

    type Layout = Box<dyn Fn([f32; 2]) -> Cell>;

    /// Every cell function that tiles the plane, by name.
    fn tilings() -> Vec<(&'static str, Layout)> {
        let mut v: Vec<(&'static str, Layout)> = vec![
            ("running", Box::new(|p| bond(p, [0.22, 0.07], 0, 5))),
            ("stack", Box::new(|p| bond(p, [0.3, 0.3], 1, 5))),
            ("setts", Box::new(|p| bond(p, [0.12, 0.1], 3, 9))),
            ("ashlar", Box::new(|p| bond(p, [0.5, 0.3], 4, 2))),
            ("basket2", Box::new(|p| basket(p, 0.1, 2.0, 4))),
            ("basket3", Box::new(|p| basket(p, 0.07, 3.0, 4))),
            ("chequer", Box::new(|p| chequer(p, 0.4, 13))),
            ("hex", Box::new(|p| hex(p, 0.15, 3))),
        ];
        for n in [2.0f32, 3.0, 4.0, 5.0] {
            v.push(("herring", Box::new(move |p| herring(p, 0.1, n, 6))));
            v.push(("herring45", Box::new(move |p| herring(rot45(p), 0.08, n, 6))));
        }
        v
    }

    /// The edge distance never changes faster than the point moves: no
    /// gap, overlap or jump at a joint (a wrong unit there would show as a
    /// jump), and it is never below zero.
    #[test]
    fn edges_are_continuous_and_the_plane_is_covered() {
        let d = 0.0013f32;
        for (name, f) in tilings() {
            for p in points(1.3, 70) {
                let a = f(p);
                assert!(a.e >= 0.0, "{name} {p:?}: e {}", a.e);
                for q in [[p[0] + d, p[1]], [p[0], p[1] + d], [p[0] + d * 0.6, p[1] - d * 0.8]] {
                    let b = f(q);
                    assert!((a.e - b.e).abs() <= d * 1.001 + 1e-5, "{name} {p:?} -> {q:?}: e {} vs {}", a.e, b.e);
                    if a.e > d * 2.0 {
                        // Well inside a unit: the same unit.
                        assert_eq!(a.id, b.id, "{name} {p:?}: id");
                        assert_eq!(a.k, b.k, "{name} {p:?}: k");
                    }
                }
                assert!((0.0..1.0).contains(&a.id), "{name} {p:?}: {a:?}");
                if name != "hex" {
                    // (A hexagon's uv is its offset from the centre plus a half.)
                    assert!((-1e-4..=1.0001).contains(&a.uv[0]) && (-1e-4..=1.0001).contains(&a.uv[1]), "{name} {p:?}: {a:?}");
                }
            }
        }
    }

    /// The herringbone's two strips: exactly one unit of all nearby strips
    /// holds each point, and the lookup finds it.
    #[test]
    fn herringbone_units_cover_the_plane_once() {
        for n in [2.0f32, 3.0, 4.0, 6.0] {
            for p in points(9.0, 60) {
                let m0 = ((p[0] - p[1] + n + 1.0) / (2.0 * n)).floor();
                let holds: Vec<f32> = (-3..=3).map(|k| m0 + k as f32).filter(|&m| hb(p, n, m, 1).e >= 0.0).collect();
                assert_eq!(holds.len(), 1, "n={n} {p:?}: held by strips {holds:?}");
                assert!(holds[0] == m0 || holds[0] == m0 - 1.0, "n={n} {p:?}: strip {} not searched", holds[0]);
                let c = herring(p, 1.0, n, 1);
                assert_eq!(c.id, hb(p, n, holds[0], 1).id);
            }
            // Both directions are laid.
            let ks: std::collections::BTreeSet<u8> = points(9.0, 30).map(|p| herring(p, 1.0, n, 1).k as u8).collect();
            assert_eq!(ks.into_iter().collect::<Vec<_>>(), [0, 1], "n={n}");
        }
    }

    /// Shingles: the unit drawn is the top one (its free edge at or below
    /// the point, the course above's edge above it), the lap runs 0 to 1, and
    /// the free edges really are jittered.
    #[test]
    fn shingle_courses_lie_over_the_one_below() {
        let u = [0.25, 0.12];
        for j in [0.0f32, 0.3, 0.9] {
            for p in points(2.0, 60) {
                let c = shingle(p, u, j, 8);
                assert!(c.e >= -1e-6, "j={j} {p:?}: {c:?}");
                assert!((-1e-4..=1.0001).contains(&c.uv[1]), "j={j} {p:?}: lap {}", c.uv[1]);
                let r0 = (p[1] / u[1]).floor();
                // Every course from two above on starts above p: nothing covers it.
                for r in [r0 + 2.0, r0 + 3.0] {
                    assert!(sb(r, p[0], u, j, 8)[0] > p[1], "j={j} {p:?}: course {r} covers it");
                }
                // The same unit across its own face.
                if c.e > 0.004 && c.uv[1] < 0.95 {
                    assert_eq!(c.id, shingle([p[0] + 0.002, p[1]], u, j, 8).id, "j={j} {p:?}");
                }
            }
        }
        let edges: std::collections::BTreeSet<i64> = (0..40).map(|x| (sb(3.0, x as f32 * 0.25 + 0.1, u, 0.5, 8)[0] * 1e4) as i64).collect();
        assert!(edges.len() > 10, "jittered free edges");
        assert_eq!(sb(3.0, 0.1, u, 0.0, 8)[0], 3.0 * u[1], "no jitter, straight courses");
    }

    /// The bonds are masonry's batch 1 maths, moved unchanged: the same edge
    /// distance and id for every mode at every point.
    #[test]
    fn the_bonds_are_masonrys_old_maths() {
        // Batch 1's rc_masonry_u (P5), for the bond variants.
        fn old(p: [f32; 2], uw: f32, uh: f32, v: u32, seed: u32) -> [f32; 2] {
            let row = (p[1] / uh).floor();
            let rh = u01(hash2([row as i32, 0], seed));
            let (mut len, mut off) = (uw, 0.0);
            if v == 0 {
                off = 0.5 * uw * step(0.5, fract(row * 0.5));
            } else if v == 3 {
                off = rh * uw;
            } else if v == 4 {
                len = uw * (0.7 + 0.6 * rh);
                off = rh * 5.0 * uw;
            }
            let x = p[0] + off;
            let seg = (x / len).floor();
            let lx = x - seg * len;
            let ly = p[1] - row * uh;
            [ly.min(uh - ly).min(lx.min(len - lx)), u01(hash2([seg as i32, row as i32], seed + 1))]
        }
        for v in [0u32, 1, 3, 4] {
            for p in points(3.0, 40) {
                let c = bond(p, [0.225, 0.075], v, 7);
                assert_eq!([c.e, c.id], old(p, 0.225, 0.075, v, 7), "variant {v} at {p:?}");
            }
        }
    }

    /// The chequer's colours alternate in both directions.
    #[test]
    fn the_chequer_alternates() {
        let k = |x: f32, y: f32| chequer([x, y], 0.4, 13).k;
        assert_eq!([k(0.1, 0.1), k(0.5, 0.1), k(0.1, 0.5), k(0.5, 0.5), k(-0.1, 0.1)], [0.0, 1.0, 1.0, 0.0, 1.0]);
    }

    #[test]
    fn studs_rims_rivets_and_lines() {
        assert_eq!(studs([0.5, 0.25], [1.0, 0.5], 0.1), 1.0);
        assert_eq!(studs([0.0, 0.0], [1.0, 0.5], 0.1), 0.0);
        // Two rows meeting at a corner share its rivet.
        let u = [2.0, 1.0];
        assert!((rim([1.0 / 12.0, 1.0 / 12.0], u, 1.0 / 12.0, 1.0 / 6.0, 0.03) - 1.0).abs() < 1e-5);
        assert_eq!(rim([1.0, 0.5], u, 1.0 / 12.0, 1.0 / 6.0, 0.03), 0.0, "none in the middle");
        // Six rivets up a short side, twelve along a long one.
        let up = (0..600).map(|k| rim([1.0 / 12.0, k as f32 / 600.0], u, 1.0 / 12.0, 1.0 / 6.0, 0.03)).collect::<Vec<_>>();
        let peaks = up.windows(3).filter(|w| w[1] > 0.9 && w[1] >= w[0] && w[1] >= w[2]).count();
        assert_eq!(peaks, 6);
        let along = (0..1200).map(|k| rim([k as f32 / 600.0, 1.0 / 12.0], u, 1.0 / 12.0, 1.0 / 6.0, 0.03)).collect::<Vec<_>>();
        assert_eq!(along.windows(3).filter(|w| w[1] > 0.9 && w[1] >= w[0] && w[1] >= w[2]).count(), 12);
        let r = rivets([0.3, 0.7], 2.0, 21);
        assert!(r[1] >= 0.0 && (0.0..1.0).contains(&r[2]));
        assert!((stripe(1.0, 0.5)).abs() < 1e-6 && (stripe(1.25, 0.5) - 0.25).abs() < 1e-6);
        assert!((grid([0.1, 0.48], [0.5, 0.5]) - 0.02).abs() < 1e-5);
    }

    #[test]
    fn the_shader_has_every_function_mirrored_here() {
        let src = include_str!("../shaders/rc_layout.wgsl");
        let fns: Vec<&str> = src.lines().filter_map(|l| l.strip_prefix("fn rc_lay_")).map(|l| &l[..l.find('(').unwrap_or(0)]).collect();
        assert_eq!(fns, ["par", "rot45", "bond", "hb", "herring", "basket", "hex", "sb", "shingle", "panel", "studs", "rim", "rivets", "stripe", "grid", "motif"]);
        // panel and motif draw, they lay out nothing: not mirrored.
    }
}
