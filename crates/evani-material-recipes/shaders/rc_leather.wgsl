// evani.shader.rc.leather
// Leather (recipe 11, batch 1 build plan section 3, package P6; batch 4's
// B4-P2 adds the embossed variant and the crazed extra). Written
// fresh for Evani.Dev: pebbled grain from the F2-F1 ridges of a fine
// cellular noise, creases from the same noise at a much larger size, suede's
// nap a velvety rim, tooling a diamond lattice pressed into the surface,
// embossing overlapping round scales with dark seams, crazing a fine
// F2-F1 crack network that opens with the wear layer. Uv space.
//   numbers: na = rough, grain scale (1-200 per uv), grain depth, crease
//            amount; nb = nap, wear lighten, tooling, craze scale
//            (1-50, times 2 cells per uv)
//   colours: A colour, B crease
//   variants: 0 smooth, 1 suede, 2 rawhide, 3 tooled, 4 embossed (scales
//             about an eighth of the grain scale across a uv), 5 tufted (P8-4: deep-buttoned diamonds, the
//             craze scale across a uv, the tooling number their puff;
//             every tier, relief from medium with crease relief)
//   extras: crease relief (bit 0: the normals), nap (bit 1), crazed (bit 2)
// Levels (q, the detail group): lite draws what low draws
// (every tier keeps the pattern; before critic round 1 lite was a flat colour with a soft sheen and
// a few broad creases); low draws grain, creases, wear, tooling, scales and
// cracks, anti-aliased (grain under 1.5 px fades to its average); medium adds grain
// and crease normals within 25 m and suede's nap rim; high adds a second,
// finer crease network; ultra draws as high.

// A scale at p (scale units): x dome height (0 at its rim, 1 on its crown,
// darker toward the rim facing the scale under it), y seam (an outline on the
// top scale and a shadow on the scale beneath it), z the scale's id in [0, 1).
fn rc_leather_e(p: vec2<f32>) -> vec3<f32> {
    let fa = fract(p) - vec2<f32>(0.5);
    let fb = fract(p + vec2<f32>(0.5)) - vec2<f32>(0.5);
    let ca = floor(p) + vec2<f32>(0.5);
    let cb = floor(p + vec2<f32>(0.5));
    let ah = ca.y > cb.y;
    let ph = select(fb, fa, ah);
    let pl = select(fa, fb, ah);
    let dh = length(ph);
    let r = 0.68;
    if (dh < r) {
        let ch = select(cb, ca, ah);
        return vec3<f32>(smoothstep(0.0, 0.4, r - dh) * (0.65 + 0.35 * clamp(0.5 - ph.y, 0.0, 1.0)), 1.0 - smoothstep(0.0, 0.08, r - dh), rc_u01(rc_hash2(vec2<i32>(ch * 2.0), 3u)));
    }
    let cl = select(ca, cb, ah);
    return vec3<f32>(smoothstep(0.0, 0.4, r - length(pl)), 1.0 - smoothstep(0.0, 0.14, dh - r), rc_u01(rc_hash2(vec2<i32>(cl * 2.0), 3u)));
}

// The leather at uv: x grain height (0 in a groove, 1 on a pebble or a scale
// crown), y crease or seam mask, z tooling mask, w crack mask.
fn rc_leather_s(b: RecBlock, uv: vec2<f32>, k: f32) -> vec4<f32> {
    let v = b.variant;
    let gs = rc_range(b.na.y, 1.0, 200.0);
    var gh = 0.0;
    var cr = 0.0;
    if (v == 4u) {
        // Embossed: overlapping round scales (two staggered lattices of discs,
        // the one with the higher centre on top); x is the dome, y the seam.
        let e = rc_leather_e(uv * gs * 0.12);
        gh = e.x;
        cr = e.y * b.na.w;
    } else {
        gh = smoothstep(0.0, 0.35, rc_voronoi2(uv * gs, b.seed, 1.0).z);
        if (v == 1u) {
            gh = 0.5 + 0.5 * rc_vnoise2(uv * gs * 2.0, b.seed + 6u);
        }
        cr = 1.0 - smoothstep(0.0, 0.14, rc_voronoi2(uv * max(gs * 0.08, 2.0), b.seed + 1u, 1.0).z);
        if (k > 0.5) {
            cr = max(cr, (1.0 - smoothstep(0.0, 0.1, rc_voronoi2(uv * max(gs * 0.2, 4.0), b.seed + 7u, 1.0).z)) * 0.5);
        }
        cr = cr * b.na.w;
    }
    var tool = 0.0;
    if (v == 3u) {
        let t = fract(uv * 8.0);
        let d = abs(t.x - 0.5) + abs(t.y - 0.5);
        tool = (1.0 - smoothstep(0.015, 0.045, abs(d - 0.36))) * b.nb.z;
    } else if (v == 5u) {
        // Tufted: diamond pillows, craze scale across a uv, puffed up
        // between buttons at their corners, pleats running button to
        // button (crease colour B, as the buttons); tooling the puff.
        let l = fract(vec2<f32>(uv.x + uv.y, uv.x - uv.y) * rc_range(b.nb.w, 1.0, 50.0)) - vec2<f32>(0.5);
        let m = max(abs(l.x), abs(l.y));
        tool = 4.0 * m * m * b.nb.z;
        cr = max(cr, max(0.6 * (1.0 - smoothstep(0.0, 0.04, 0.5 - m)), 1.0 - smoothstep(0.07, 0.1, length(vec2<f32>(0.5) - abs(l)))));
    }
    var cz = 0.0;
    if (rc_extra(b, RC_X_LEATHER, 2u)) {
        // Crazing: a network of dry cracks whose width grows with wear.
        cz = 1.0 - smoothstep(0.0, 0.05 + 0.1 * rc_amount(b, 4u), rc_voronoi2(uv * rc_range(b.nb.w, 1.0, 50.0) * 2.0, b.seed + 11u, 1.0).z);
    }
    return vec4<f32>(gh, cr, tool, cz);
}

// The surface height of a sample of rc_leather_s.
fn rc_leather_h(b: RecBlock, s: vec4<f32>) -> f32 {
    return s.x * b.na.z * 0.5 - s.y * 1.6 - s.z - s.w * 1.2;
}

// A per-cell tint in [0, 1): the scale cell (embossed) or crack plate.
fn rc_leather_t(b: RecBlock, uv: vec2<f32>, gs: f32) -> f32 {
    if (b.variant == 4u) {
        return rc_leather_e(uv * gs * 0.12).z;
    }
    return rc_voronoi2(uv * rc_range(b.nb.w, 1.0, 50.0) * 2.0, b.seed + 11u, 1.0).w;
}

fn rc_leather(b: RecBlock, f: MatFrame, i: MatIn, q: u32) -> MatOut {
    let gs = rc_range(b.na.y, 1.0, 200.0);
    let hi = select(0.0, 1.0, q >= 3u);
    let s = rc_leather_s(b, i.uv, hi);
    let emb = b.variant == 4u;
    let ka = rc_aa(f, 0.5 / gs);
    let gh = rc_fade(s.x, 0.7, ka);
    let cm = rc_fade(s.y, select(0.15, 0.1, emb), rc_aa(f, select(0.3 / (gs * 0.08), 0.4 / gs, emb)));
    var c = mix(b.col_a, b.col_b, cm);
    c = c * (0.88 + 0.24 * gh) * (1.0 - 0.5 * s.z);
    let cx = rc_extra(b, RC_X_LEATHER, 2u);
    var cw = 0.0;
    if (emb || cx) {
        // Each scale or crack plate a little lighter or darker.
        let tn = rc_fade(rc_leather_t(b, i.uv, gs), 0.5, rc_aa(f, 1.5 / gs));
        c = c * (0.92 + 0.16 * tn);
    }
    if (cx) {
        // Dry cracks: dark, and the leather around them lifted and dull.
        cw = rc_fade(s.w, 0.1, rc_aa(f, 0.06 / (2.0 * rc_range(b.nb.w, 1.0, 50.0))));
        c = mix(c, b.col_b * 0.4, cw);
    }
    // Wear lightens the high pebbles and the crease edges.
    c = mix(c, c * 1.6 + vec3<f32>(0.04), b.nb.y * smoothstep(0.7, 1.0, gh) * (1.0 - cm) * (1.0 - cw));
    if (b.variant == 2u) {
        // Rawhide: stiff, blotched and veined.
        c = c * (0.75 + 0.5 * rc_fbm2(i.uv * gs * 0.1, b.seed + 2u, 3u));
    }
    var o = rc_out(i, c, b.na.x + 0.25 * (1.0 - gh) * b.na.z + 0.3 * cw, 0.0);
    if (q >= 2u) {
        if (rc_extra(b, RC_X_LEATHER, 1u) && b.nb.x > 0.0) {
            // Suede's nap: a velvet rim, rougher, with brushed fibres.
            let fib = rc_vnoise2(vec2<f32>(i.uv.x * gs * 3.0, i.uv.y * gs * 0.6), b.seed + 3u);
            o.base = vec4<f32>(o.base.rgb * (0.9 + 0.2 * rc_fade(fib, 0.5, ka)), o.base.a);
            o.rough = mix(o.rough, 1.0, b.nb.x);
            o.sheen = vec4<f32>(b.col_a * (1.4 * b.nb.x) + vec3<f32>(0.04 * b.nb.x), 1.8);
        }
        let near = rc_near(f) * ka;
        if ((rc_extra(b, RC_X_LEATHER, 0u) || cx) && near > 0.0) {
            let e = 0.4 / gs;
            // Height: pebbles up, creases, cracks and tooling down.
            let h = rc_leather_h(b, s);
            let g = vec2<f32>(rc_leather_h(b, rc_leather_s(b, i.uv + vec2<f32>(e, 0.0), hi)), rc_leather_h(b, rc_leather_s(b, i.uv + vec2<f32>(0.0, e), hi))) - vec2<f32>(h);
            o.n = rc_bump(i.n, rc_tuv(f, i.n), g * (1.2 * near));
        }
    }
    return o;
}
