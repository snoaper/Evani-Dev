// evani.shader.recipes.cut
// Cut-out holes (batch 3 build plan 3.5, B3-P2; feature `cutout`): grates,
// chain-link, wire mesh and lattice, computed from the material's block alone
// (no texture), so the colour pass (cut_stage.wgsl) and the model shadow and
// depth passes (evani-render's model_cut_shadows.wgsl) cut the very same holes.
// This text uses nothing but RecRaw, which both modules declare. Written fresh
// for Evani.Dev.
//
// Every pattern is one or two sets of wires (bars, slats) in uv. A set's
// coverage of a pixel is the exact overlap of its nearest wire with the
// pixel's footprint across it (a box filter), so a wire thinner than a pixel
// still covers its true share and never vanishes; once the set's period is
// under 2 pixels the pattern fades to its average coverage (the open area
// shown as an even see-through, never a shimmer). The two sets combine as
// a + b - ab. The caller keeps a pixel where its coverage reaches the 2x2
// screen-door threshold (rc_cut_th, the forward pass's own fade pattern), so
// partial coverage resolves to an even blend in the tonemap pass (colour) or
// the shadow filter (shadows). Every tier keeps the same holes.
//
// Kinds (recipe and variant, block bytes 0 and 1):
//   1 metal grate: bearing bars 1 / (2 ls) apart, cross bars every fourth;
//   2 metal chain link: two sets of wires at 45 degrees, 1 / ls apart;
//   3 metal mesh: hexagonal wire, cells 1 / (2 ls) across;
//   4 wood lattice: slats at 45 degrees both ways, plank width wide, the open
//     share of each period by the `opening` number;
//   5 thin lace (batch 1's cloth.lace.white): a fine hexagonal net, cells a
//     seventh of a motif, and in each motif cell a six-petal flower of solid
//     thread with an eyelet in every petal (motif scale, n2: motifs per uv);
//   6 cloth open (B4-P3a: muslin, gauze): warp and weft threads, eight per
//     weave step (n0, 1-400), half a period wide, the gaps between them cut.
// ls is metal's layout scale (n8, 1-20 per uv).

// The pattern at a point: per set (x: set A, y: set B) the signed offset from
// the nearest wire's centre line, its half width (0: no such set) and its
// period, all in uv; the across direction of each set (unit, in uv); and
// which set lies on top where they cross (0 A, 1 B).
struct RcCutG {
    d: vec2<f32>,
    hw: vec2<f32>,
    p: vec2<f32>,
    na: vec2<f32>,
    nb: vec2<f32>,
    top: f32,
    // A solid motif (lace): its signed distance (uv, below 0 inside) and its
    // cell (0: none).
    m: f32,
    mc: f32,
};

// Block byte i (0-31) as unorm.
fn rc_cut_byte(w: RecRaw, i: u32) -> f32 {
    let v = select(w.w1, w.w0, i < 16u)[(i >> 2u) & 3u];
    return f32((v >> ((i & 3u) * 8u)) & 255u) / 255.0;
}

// The cut kind of a block (0: not cut).
fn rc_cut_kind(w: RecRaw) -> u32 {
    let r = w.w0.x & 0xffffu;
    if (r == 0xc04u) {
        return 4u;
    }
    if (r == 0x715u) {
        return 5u;
    }
    if (r == 0x70au) {
        return 6u;
    }
    if ((r & 0xffu) == 1u && r >= 0x601u && r <= 0x801u) {
        return (r >> 8u) - 5u;
    }
    return 0u;
}

// Signed offset from the nearest of lines s = k * p (s across the lines).
fn rc_cut_off(s: f32, p: f32) -> f32 {
    return (fract(s / p + 0.5) - 0.5) * p;
}

// Pointy-top hexagons of cell c (uv) at uv: x the offset from the nearest
// edge's wire centre along its outward normal, yz that normal.
fn rc_cut_hex(uv: vec2<f32>, c: f32) -> vec3<f32> {
    let q = uv / c;
    let s = vec2<f32>(1.0, 1.7320508);
    let a = q - (floor(q / s) + vec2<f32>(0.5)) * s;
    let qb = q - s * 0.5;
    let b = qb - (floor(qb / s) + vec2<f32>(0.5)) * s;
    let h = select(b, a, dot(a, a) < dot(b, b));
    let m = abs(h);
    let side = m.x > dot(m, vec2<f32>(0.5, 0.8660254));
    let n = select(vec2<f32>(0.5, 0.8660254), vec2<f32>(1.0, 0.0), side) * sign(h + vec2<f32>(1e-6));
    return vec3<f32>((max(m.x, dot(m, vec2<f32>(0.5, 0.8660254))) - 0.5) * c, n);
}

fn rc_cut_geo(w: RecRaw, kind: u32, uv: vec2<f32>) -> RcCutG {
    var g: RcCutG;
    let ls = mix(1.0, 20.0, rc_cut_byte(w, 21u));
    let r = vec2<f32>(uv.x + uv.y, uv.x - uv.y) * 0.70710678;
    g.na = vec2<f32>(0.70710678);
    g.nb = vec2<f32>(0.70710678, -0.70710678);
    if (kind == 1u) {
        let pa = 0.5 / ls;
        g.p = vec2<f32>(pa, pa * 4.0);
        g.d = vec2<f32>(rc_cut_off(uv.x, g.p.x), rc_cut_off(uv.y, g.p.y));
        g.hw = vec2<f32>(0.09, 0.11) * pa;
        g.na = vec2<f32>(1.0, 0.0);
        g.nb = vec2<f32>(0.0, 1.0);
    } else if (kind == 2u) {
        g.p = vec2<f32>(1.0 / ls);
        g.d = vec2<f32>(rc_cut_off(r.x, g.p.x), rc_cut_off(r.y, g.p.x));
        g.hw = vec2<f32>(0.045 / ls);
        // Woven: the wires change places at every crossing.
        g.top = fract((floor(r.x * ls + 0.5) + floor(r.y * ls + 0.5)) * 0.5) * 2.0;
    } else if (kind == 3u || kind == 5u) {
        // Hexagonal wire (mesh) or net (lace, cells a seventh of a motif).
        let mc = 1.0 / mix(1.0, 32.0, rc_cut_byte(w, 15u));
        let c = select(0.5 / ls, mc / 7.0, kind == 5u);
        let h = rc_cut_hex(uv, c);
        g.na = h.yz;
        g.d = vec2<f32>(h.x, 0.0);
        g.hw = vec2<f32>(select(0.06, 0.05, kind == 5u) * c, 0.0);
        g.p = vec2<f32>(0.5 * c, 1.0);
        if (kind == 5u) {
            // A six-petal flower in each motif cell, an eyelet in each petal.
            let q = uv / mc - floor(uv / mc) - vec2<f32>(0.5);
            let r = length(q);
            let an = atan2(q.y, q.x);
            let k = round(an * 0.95492966) * 1.0471976;
            let e = 0.035 - length(q - vec2<f32>(cos(k), sin(k)) * 0.19);
            g.m = max(max(r - 0.28 - 0.1 * cos(6.0 * an), 0.07 - r), e) * mc;
            g.mc = mc;
        }
    } else if (kind == 6u) {
        let p = 0.125 / mix(1.0, 400.0, rc_cut_byte(w, 13u));
        g.p = vec2<f32>(p);
        g.d = vec2<f32>(rc_cut_off(uv.x - 0.5 * p, p), rc_cut_off(uv.y - 0.5 * p, p));
        g.hw = vec2<f32>(0.25 * p);
        g.na = vec2<f32>(1.0, 0.0);
        g.nb = vec2<f32>(0.0, 1.0);
    } else if (kind == 4u) {
        let pw = mix(0.05, 1.0, rc_cut_byte(w, 18u));
        let pp = pw / max(1.0 - mix(0.0, 0.9, rc_cut_byte(w, 22u)), 0.05);
        g.p = vec2<f32>(pp);
        g.d = vec2<f32>(rc_cut_off(r.x, pp), rc_cut_off(r.y, pp));
        g.hw = vec2<f32>(pw * 0.5);
    }
    return g;
}

// One set's coverage of a pixel whose uv footprint is dx, dy (the screen
// derivatives), with every wire drawn at least mpx pixels wide.
fn rc_cut_set(d: f32, hw0: f32, p: f32, n: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>, mpx: f32) -> f32 {
    if (hw0 <= 0.0) {
        return 0.0;
    }
    let px = max(abs(dot(dx, n)) + abs(dot(dy, n)), 1e-7);
    let hw = max(hw0, 0.5 * mpx * px);
    let e = 0.5 * px;
    let c = clamp((min(d + e, hw) - max(d - e, -hw)) / px, 0.0, 1.0);
    return mix(min(2.0 * hw / p, 1.0), c, smoothstep(1.0, 2.0, p / px));
}

// The solid share of the pixel (1 solid, 0 a hole). A motif covers by its
// distance over the pixel, fading to a fifth of its cell under 3 pixels.
fn rc_cut_cov(g: RcCutG, dx: vec2<f32>, dy: vec2<f32>, mpx: f32) -> f32 {
    let a = rc_cut_set(g.d.x, g.hw.x, g.p.x, g.na, dx, dy, mpx);
    let b = rc_cut_set(g.d.y, g.hw.y, g.p.y, g.nb, dx, dy, mpx);
    var c = a + b - a * b;
    if (g.mc > 0.0) {
        let px = max(0.5 * (length(dx) + length(dy)), 1e-7);
        let m = mix(0.2, clamp(0.5 - g.m / px, 0.0, 1.0), smoothstep(2.0, 4.0, g.mc / px));
        c = c + m - c * m;
    }
    return c;
}

// material.recipe.cut.min_px from the off-bits word (bits 8-15, 1/85 px).
fn rc_cut_mpx(z: u32) -> f32 {
    return f32((z >> 8u) & 255u) / 85.0;
}

// The 2x2 screen-door threshold at pixel p (forward.wgsl's fade pattern).
fn rc_cut_th(p: vec2<f32>) -> f32 {
    let q = vec2<u32>(p) & vec2<u32>(1u);
    return select(select(0.125, 0.625, q.x == 1u), select(0.875, 0.375, q.x == 1u), q.y == 1u);
}
