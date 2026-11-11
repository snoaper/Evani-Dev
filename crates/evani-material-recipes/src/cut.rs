//! The cut-out on the CPU (batch 3 build plan 3.5, B3-P2): which blocks cut
//! holes ([`kind`], [`is_cut`]: evani-render picks the hole-aware shadow
//! pipeline with it) and the same pattern and coverage maths as
//! `shaders/cut.wgsl`, line for line, so the tests can hold the holes to
//! their promises: a wire never vanishes however far away, a far fence shows
//! its true open share, and a shadow map filtered like the engine's lets
//! that share of the light through. No game calls the maths, so no game
//! downloads it; `kind` is a few compares.

use crate::block::Block;

/// The cut kinds (`rc_cut_kind`): none, metal grate, chain link, wire mesh,
/// wood lattice, thin lace, cloth's open weave (B4-P3a).
pub const NONE: u32 = 0;
pub const GRATE: u32 = 1;
pub const CHAIN_LINK: u32 = 2;
pub const MESH: u32 = 3;
pub const LATTICE: u32 = 4;
pub const LACE: u32 = 5;
pub const OPEN: u32 = 6;

/// The kinds that draw without the cut-out too (cloth's open weave draws its
/// gaps dark), so a game that doesn't list `cutout` gets a note, not an
/// error ([`is_cut_required`]).
pub const OPTIONAL: [u32; 1] = [OPEN];

/// The recipe variants that cut: (recipe code, variant, kind).
pub const CUT_VARIANTS: [(u8, u8, u32); 6] = [(1, 6, GRATE), (1, 7, CHAIN_LINK), (1, 8, MESH), (4, 12, LATTICE), (21, 7, LACE), (10, 7, OPEN)];

/// The cut kind of block bytes 0 and 1 (`rc_cut_kind`).
pub fn kind_of(recipe: u8, variant: u8) -> u32 {
    CUT_VARIANTS.iter().find(|&&(r, v, _)| r == recipe && v == variant).map_or(NONE, |&(_, _, k)| k)
}

/// The cut kind of `block`.
pub fn kind(block: &Block) -> u32 {
    kind_of(block.0[0], block.0[1])
}

/// Whether `block` cuts holes.
pub fn is_cut(block: &Block) -> bool {
    kind(block) != NONE
}

/// Whether `block` cuts holes it can't draw without (a cut kind not in
/// [`OPTIONAL`]).
pub fn is_cut_required(block: &Block) -> bool {
    let k = kind(block);
    k != NONE && !OPTIONAL.contains(&k)
}

/// The cut kind of a material's block words (binding 12's eight words).
pub fn kind_of_words(w: &[u32; 8]) -> u32 {
    kind_of((w[0] & 0xff) as u8, ((w[0] >> 8) & 0xff) as u8)
}

/// `RcCutG`: per set (A, B) the signed offset from the nearest wire's centre,
/// its half width (0: no set) and period, the sets' across directions, and
/// which set is on top at a crossing.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Geo {
    pub d: [f32; 2],
    pub hw: [f32; 2],
    pub p: [f32; 2],
    pub na: [f32; 2],
    pub nb: [f32; 2],
    pub top: f32,
    pub m: f32,
    pub mc: f32,
}

fn fract(x: f32) -> f32 {
    x - x.floor()
}

fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn dot(a: [f32; 2], b: [f32; 2]) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}

/// `rc_cut_byte`.
fn byte(b: &Block, i: usize) -> f32 {
    f32::from(b.0[i]) / 255.0
}

/// `rc_cut_off`: the signed offset from the nearest line `s = k p`.
pub fn off(s: f32, p: f32) -> f32 {
    (fract(s / p + 0.5) - 0.5) * p
}

const R2: f32 = core::f32::consts::FRAC_1_SQRT_2;

/// `rc_cut_hex`: pointy-top hexagons of cell `c` at `uv`: the offset from the
/// nearest edge's wire centre along its outward normal, and that normal.
pub fn hex(uv: [f32; 2], c: f32) -> (f32, [f32; 2]) {
    let q = [uv[0] / c, uv[1] / c];
    let s = [1.0f32, 1.732_050_8];
    let a = [q[0] - ((q[0] / s[0]).floor() + 0.5) * s[0], q[1] - ((q[1] / s[1]).floor() + 0.5) * s[1]];
    let bq = [q[0] - s[0] * 0.5, q[1] - s[1] * 0.5];
    let bb = [bq[0] - ((bq[0] / s[0]).floor() + 0.5) * s[0], bq[1] - ((bq[1] / s[1]).floor() + 0.5) * s[1]];
    let h = if dot(a, a) < dot(bb, bb) { a } else { bb };
    let m = [h[0].abs(), h[1].abs()];
    let e = [0.5f32, 0.866_025_4];
    let sg = |x: f32| if x + 1e-6 > 0.0 { 1.0 } else if x + 1e-6 < 0.0 { -1.0 } else { 0.0 };
    let n = if m[0] > dot(m, e) { [1.0, 0.0] } else { e };
    ((m[0].max(dot(m, e)) - 0.5) * c, [n[0] * sg(h[0]), n[1] * sg(h[1])])
}

/// `rc_cut_geo`.
pub fn geo(b: &Block, kind: u32, uv: [f32; 2]) -> Geo {
    let mut g = Geo::default();
    let ls = mix(1.0, 20.0, byte(b, 21));
    let r = [(uv[0] + uv[1]) * R2, (uv[0] - uv[1]) * R2];
    g.na = [R2, R2];
    g.nb = [R2, -R2];
    match kind {
        GRATE => {
            let pa = 0.5 / ls;
            g.p = [pa, pa * 4.0];
            g.d = [off(uv[0], g.p[0]), off(uv[1], g.p[1])];
            g.hw = [0.09 * pa, 0.11 * pa];
            g.na = [1.0, 0.0];
            g.nb = [0.0, 1.0];
        }
        CHAIN_LINK => {
            g.p = [1.0 / ls; 2];
            g.d = [off(r[0], g.p[0]), off(r[1], g.p[0])];
            g.hw = [0.045 / ls; 2];
            g.top = fract(((r[0] * ls + 0.5).floor() + (r[1] * ls + 0.5).floor()) * 0.5) * 2.0;
        }
        MESH | LACE => {
            let mc = 1.0 / mix(1.0, 32.0, byte(b, 15));
            let c = if kind == LACE { mc / 7.0 } else { 0.5 / ls };
            let (d, n) = hex(uv, c);
            g.na = n;
            g.d = [d, 0.0];
            g.hw = [if kind == LACE { 0.05 } else { 0.06 } * c, 0.0];
            g.p = [0.5 * c, 1.0];
            if kind == LACE {
                let q = [uv[0] / mc - (uv[0] / mc).floor() - 0.5, uv[1] / mc - (uv[1] / mc).floor() - 0.5];
                let r = (q[0] * q[0] + q[1] * q[1]).sqrt();
                let an = q[1].atan2(q[0]);
                let k = (an / core::f32::consts::FRAC_PI_3).round() * core::f32::consts::FRAC_PI_3;
                let (ex, ey) = (q[0] - k.cos() * 0.19, q[1] - k.sin() * 0.19);
                let e = 0.035 - (ex * ex + ey * ey).sqrt();
                g.m = (r - 0.28 - 0.1 * (6.0 * an).cos()).max(0.07 - r).max(e) * mc;
                g.mc = mc;
            }
        }
        OPEN => {
            let p = 0.125 / mix(1.0, 400.0, byte(b, 13));
            g.p = [p; 2];
            g.d = [off(uv[0] - 0.5 * p, p), off(uv[1] - 0.5 * p, p)];
            g.hw = [0.25 * p; 2];
            g.na = [1.0, 0.0];
            g.nb = [0.0, 1.0];
        }
        LATTICE => {
            let pw = mix(0.05, 1.0, byte(b, 18));
            let pp = pw / (1.0 - mix(0.0, 0.9, byte(b, 22))).max(0.05);
            g.p = [pp; 2];
            g.d = [off(r[0], pp), off(r[1], pp)];
            g.hw = [pw * 0.5; 2];
        }
        _ => {}
    }
    g
}

/// `rc_cut_set`: one set's coverage of a pixel with uv footprint `dx`, `dy`.
pub fn set(d: f32, hw0: f32, p: f32, n: [f32; 2], dx: [f32; 2], dy: [f32; 2], mpx: f32) -> f32 {
    if hw0 <= 0.0 {
        return 0.0;
    }
    let px = (dot(dx, n).abs() + dot(dy, n).abs()).max(1e-7);
    let hw = hw0.max(0.5 * mpx * px);
    let e = 0.5 * px;
    let c = (((d + e).min(hw) - (d - e).max(-hw)) / px).clamp(0.0, 1.0);
    mix((2.0 * hw / p).min(1.0), c, smoothstep(1.0, 2.0, p / px))
}

/// `rc_cut_cov`: the solid share of the pixel.
pub fn cov(g: &Geo, dx: [f32; 2], dy: [f32; 2], mpx: f32) -> f32 {
    let a = set(g.d[0], g.hw[0], g.p[0], g.na, dx, dy, mpx);
    let b = set(g.d[1], g.hw[1], g.p[1], g.nb, dx, dy, mpx);
    let c = a + b - a * b;
    if g.mc <= 0.0 {
        return c;
    }
    let len = |v: [f32; 2]| (v[0] * v[0] + v[1] * v[1]).sqrt();
    let px = (0.5 * (len(dx) + len(dy))).max(1e-7);
    let m = mix(0.2, (0.5 - g.m / px).clamp(0.0, 1.0), smoothstep(2.0, 4.0, g.mc / px));
    c + m - c * m
}

/// `rc_cut_th`: the 2x2 screen-door threshold at pixel `(x, y)`.
pub fn th(x: u32, y: u32) -> f32 {
    match (x & 1, y & 1) {
        (0, 0) => 0.125,
        (1, 0) => 0.625,
        (0, 1) => 0.875,
        _ => 0.375,
    }
}

/// The open share of a kind's pattern (the average coverage it fades to,
/// before `min_px`): `1 - (1 - a)(1 - b)` of the sets' wire shares.
pub fn solid_share(b: &Block, kind: u32) -> f32 {
    let g = geo(b, kind, [0.0, 0.0]);
    let s = |hw: f32, p: f32| if hw > 0.0 { (2.0 * hw / p).min(1.0) } else { 0.0 };
    let (a, c) = (s(g.hw[0], g.p[0]), s(g.hw[1], g.p[1]));
    let w = a + c - a * c;
    if kind == LACE { w + 0.2 - w * 0.2 } else { w }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// A block of recipe `r`, variant `v`, with the layout scale (metal) or
    /// plank width and opening (wood) set.
    fn block(r: u8, v: u8, n5: u8, n8: u8, n9: u8) -> Block {
        let mut b = [0u8; 32];
        b[0] = r;
        b[1] = v;
        b[18] = n5;
        b[21] = n8;
        b[22] = n9;
        Block(b)
    }

    fn rows() -> [(u32, Block); 6] {
        let mut lace = block(21, 7, 0, 0, 0);
        lace.0[15] = 49;
        let mut open = block(10, 7, 0, 0, 0);
        open.0[13] = 19;
        [
            (GRATE, block(1, 6, 0, 202, 0)),
            (CHAIN_LINK, block(1, 7, 0, 255, 0)),
            (MESH, block(1, 8, 0, 255, 0)),
            (LATTICE, block(4, 12, 8, 0, 198)),
            (LACE, lace),
            (OPEN, open),
        ]
    }

    #[test]
    fn only_the_cut_variants_cut() {
        for (k, b) in rows() {
            assert_eq!(kind(&b), k);
            assert!(is_cut(&b));
            assert_eq!(kind_of_words(&b.to_words()), k);
        }
        for (r, v) in [(0, 0), (1, 0), (1, 5), (1, 9), (4, 3), (4, 11), (4, 13), (2, 6), (7, 12), (21, 5), (21, 6), (10, 6), (10, 8)] {
            assert!(!is_cut(&block(r, v, 0, 0, 0)), "{r}/{v}");
        }
        assert_eq!(kind(&Block::EMPTY), NONE);
        // Only cloth's open weave is optional.
        for (k, b) in rows() {
            assert_eq!(is_cut_required(&b), k != OPEN, "kind {k}");
        }
    }

    /// Up close (a pixel a tenth of a wire) every pattern has solid wire and
    /// open holes, and its measured solid share is its average.
    #[test]
    fn close_up_there_are_wires_and_holes() {
        for (k, b) in rows() {
            let g0 = geo(&b, k, [0.0, 0.0]);
            let px = g0.hw[0] * 0.1;
            let (mut solid, mut open, mut sum, n) = (0, 0, 0.0, 400);
            for j in 0..n {
                for i in 0..n {
                    // A square of 4 x 4 periods.
                    let span = if g0.mc > 0.0 { 2.0 * g0.mc } else { 8.0 * g0.p[0] };
                    let uv = [(i as f32 + 0.37) / n as f32 * span, (j as f32 + 0.61) / n as f32 * span];
                    let c = cov(&geo(&b, k, uv), [px, 0.0], [0.0, px], 0.0);
                    solid += usize::from(c > 0.999);
                    open += usize::from(c < 0.001);
                    sum += c;
                }
            }
            let share = solid_share(&b, k);
            assert!(solid > 0 && open > 0, "kind {k}: solid {solid} open {open}");
            // Cloth's open weave is mostly thread (three quarters).
            assert!(share > 0.05 && share < if k == OPEN { 0.8 } else { 0.7 }, "kind {k}: solid share {share}");
            let mean = sum / (n * n) as f32;
            assert!((mean - share).abs() < 0.06, "kind {k}: measured {mean} vs {share}");
        }
    }

    /// However far away (pixels up to 50 periods wide), a pattern's coverage
    /// never drops to nothing on its wires and never rises above solid: thin
    /// wire fades to its true share instead of vanishing.
    #[test]
    fn far_away_a_wire_never_vanishes() {
        for (k, b) in rows() {
            let g0 = geo(&b, k, [0.0, 0.0]);
            let share = solid_share(&b, k);
            // A square of several of the larger period, sampled finely.
            let span = 5.3 * g0.p[0].max(if g0.hw[1] > 0.0 { g0.p[1] } else { 0.0 }).max(g0.mc);
            for f in [0.5f32, 1.0, 2.0, 5.0, 20.0, 50.0] {
                let px = g0.p[0] * f;
                let mut sum = 0.0;
                let n = 96;
                for j in 0..n {
                    for i in 0..n {
                        let uv = [(i as f32 + 0.5) / n as f32 * span, (j as f32 + 0.31) / n as f32 * span];
                        let c = cov(&geo(&b, k, uv), [px, 0.0], [0.0, px], 0.0);
                        assert!((0.0..=1.0).contains(&c));
                        sum += c;
                    }
                }
                let mean = sum / (n * n) as f32;
                if f >= 2.0 {
                    // Lace's flowers are drawn exactly until 4 px, then fade
                    // to the fifth of their cell they cover.
                    let tol = if k == LACE { 0.05 } else { 0.02 };
                    assert!((mean - share).abs() < tol, "kind {k} at {f}: {mean} vs {share}");
                }
                assert!(mean > 0.3 * share, "kind {k} at {f} periods a pixel: {mean}");
            }
        }
    }

    /// `min_px` widens wire to at least that many pixels.
    #[test]
    fn min_px_widens_far_wire() {
        let (k, b) = rows()[1];
        let g = geo(&b, k, [0.0, 0.0]);
        let px = g.hw[0] * 4.0;
        let plain = set(0.0, g.hw[0], g.p[0], g.na, [px, 0.0], [0.0, px], 0.0);
        let wide = set(0.0, g.hw[0], g.p[0], g.na, [px, 0.0], [0.0, px], 3.0);
        assert!(wide > plain && (wide - 1.0).abs() < 1e-6);
    }

    /// The shadow test (on the CPU, as the shadow pass and the forward pass's
    /// filter run it): a fence of each kind rasterised into a shadow map at
    /// several texel sizes, each texel kept where its coverage reaches the
    /// 2x2 threshold (the hole-aware depth pass), then a 4x4 box filter (the
    /// PCF taps) gives the light reaching the ground behind it. A cut fence
    /// lets light through in proportion to its open share; the same plane
    /// without the cut (a solid block) lets none through.
    #[test]
    fn light_comes_through_the_holes_in_the_shadow() {
        for (k, b) in rows() {
            let g0 = geo(&b, k, [0.0, 0.0]);
            let open = 1.0 - solid_share(&b, k);
            // The map spans at least two of the largest repeat (lace's flowers).
            let big = g0.p[0].max(if g0.hw[1] > 0.0 { g0.p[1] } else { 0.0 }).max(g0.mc);
            for texel in [(g0.hw[0] * 0.25).max(big / 32.0), g0.p[0] * 0.3, g0.p[0] * 3.0] {
                let n = 64u32;
                let mut lit = vec![0.0f32; (n * n) as usize];
                for y in 0..n {
                    for x in 0..n {
                        let uv = [(x as f32 + 0.5) * texel, (y as f32 + 0.5) * texel];
                        let c = cov(&geo(&b, k, uv), [texel, 0.0], [0.0, texel], 0.0);
                        // Depth written (in shadow) where the texel is kept.
                        lit[(y * n + x) as usize] = if c >= th(x, y) { 0.0 } else { 1.0 };
                    }
                }
                let mut sum = 0.0;
                let mut cnt = 0.0;
                for y in 2..n - 2 {
                    for x in 2..n - 2 {
                        let mut s = 0.0;
                        for j in 0..4 {
                            for i in 0..4 {
                                s += lit[((y + j - 2) * n + x + i - 2) as usize];
                            }
                        }
                        sum += s / 16.0;
                        cnt += 1.0;
                    }
                }
                let light = sum / cnt;
                // The 2x2 pattern rounds a far pattern's cover to quarters.
                assert!(light > 0.2, "kind {k}, texel {texel}: only {light} of the light gets through");
                assert!((light - open).abs() < 0.15, "kind {k}, texel {texel}: {light} through, open share {open}");
            }
            // The same plane, not cut: solid (the shadow pipeline isn't used).
            assert!(!is_cut(&block(b.0[0], 0, 0, 0, 0)));
        }
    }
}
