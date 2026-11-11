// evani.shader.rc.scales
// Scales (recipe 14, batch 1 build plan section 3, package P7). Written fresh
// for Evani.Dev: fish, snake, lizard and crocodile skin. Cells of a hex
// lattice (alternate rows offset by half a cell; plates use square, aligned
// rows) cut by a round-to-hex radius, so each scale is a round shield with
// gaps (shape 0) or a hex tile with none (shape 1); a per-scale tone, a
// pattern layer, a rim on the free edge, relief normals and a fish shimmer
// (a hue by view angle). Uv space: u along the row, v down the body, the free
// edge of an overlapping scale faces +v.
//   numbers: na = scale size (0.003-0.2 uv), shape, relief depth, pattern
//            scale (1-50 per uv); nb = pattern contrast, iridescence, rough,
//            pattern kind (P7-2: 0 by pattern scale as below, 1 rings, 2
//            stripes, 3 beans, 4 wavy lines)
//   colours: A colour, B pattern (and the gaps), C shimmer hue
//   variants: 0 overlapping, 1 hex, 2 plates, 3 bumps
//   extras: relief (bit 0), shimmer (bit 1)
// The pattern kind follows pattern scale and cross-fades: a diamond chain
// (to 8), blotches (8 to 18), bands across v (18 to 30), speckles (30 up).
// A pattern kind above 0 is read at each scale's own place (its centre down
// v, a step across u by its random), so the edges follow whole scales as on
// a snake: rings across v (A, B, C, B per repeat: red, black, yellow, black;
// C is the shimmer hue, so a ringed row keeps the shimmer off), stripes
// along v (B, three repeats on a body ring at pattern scale 3), beans (pale
// B spots twice as long along v as across), wavy lines across v (B).
// Levels (q, the detail group): lite draws what low draws
// (every tier keeps the pattern; before critic round 1 lite was the flat pattern); low adds the scales
// (tone, edge, rim) anti-aliased (cells under 1.5 px fade to their average);
// medium adds relief normals within 25 m, the shimmer and its sheen; high adds
// per-scale ridges and a view-angle glint; ultra draws as high.

// The pattern mask at uv: x 0 body colour, 1 pattern colour; y the rings'
// third colour (C).
fn rc_scales_p(b: RecBlock, f: MatFrame, uv: vec2<f32>) -> vec2<f32> {
    let ps = rc_range(b.na.w, 1.0, 50.0);
    let p = uv * ps;
    let k = round(rc_range(b.nb.w, 0.0, 4.0));
    let ka = rc_aa(f, 1.0 / ps);
    if (k > 0.5) {
        var m = vec2<f32>(0.0);
        let r = fract(p.y);
        if (k < 1.5) {
            m = vec2<f32>(smoothstep(0.44, 0.45, r) - smoothstep(0.6, 0.61, r) + smoothstep(0.76, 0.77, r) - smoothstep(0.92, 0.93, r), smoothstep(0.6, 0.61, r) - smoothstep(0.76, 0.77, r));
        } else if (k < 2.5) {
            m.x = 1.0 - smoothstep(0.08, 0.1, abs(fract(p.x) - 0.5));
        } else if (k < 3.5) {
            let c = rc_voronoi2(p * vec2<f32>(1.0, 0.5), b.seed + 3u, 0.8);
            m.x = (1.0 - smoothstep(0.24, 0.3, c.x)) * step(c.w, 0.8);
        } else {
            m.x = 1.0 - smoothstep(0.07, 0.12, abs(fract(p.y + 0.22 * sin(p.x * 2.6 + 1.5 * rc_vnoise2(p * 0.8, b.seed + 2u))) - 0.5));
        }
        return vec2<f32>(rc_fade(m.x, 0.3, ka), rc_fade(m.y, 0.16, ka));
    }
    let s0 = smoothstep(6.0, 9.0, ps);
    let s1 = smoothstep(14.0, 18.0, ps);
    let s2 = smoothstep(26.0, 34.0, ps);
    var m = 0.0;
    if (s0 < 1.0) {
        // Diamond chain: dark edges, alternate cells a little darker.
        let a = (p.x + p.y) * 0.5;
        let c = (p.x - p.y) * 0.5;
        let e = max(abs(fract(a) - 0.5), abs(fract(c) - 0.5));
        m = max(smoothstep(0.38, 0.47, e), 0.35 * step(0.5, fract(0.5 * (floor(a) + floor(c))))) * (1.0 - s0);
    }
    if (s0 > 0.0 && s1 < 1.0) {
        m = m + smoothstep(0.5, 0.58, rc_fbm2(p * 0.35, b.seed + 1u, 3u)) * s0 * (1.0 - s1);
    }
    if (s1 > 0.0 && s2 < 1.0) {
        m = m + smoothstep(0.2, 0.6, sin(p.y * 1.5708 + (rc_vnoise2(p * 0.4, b.seed + 2u) - 0.5) * 2.0)) * s1 * (1.0 - s2);
    }
    if (s2 > 0.0) {
        let c = rc_voronoi2(p * 0.9, b.seed + 3u, 1.0);
        m = m + (1.0 - smoothstep(0.1, 0.18, c.x)) * step(c.w, 0.6) * s2;
    }
    return vec2<f32>(rc_fade(m, 0.2, ka), 0.0);
}

// The scale at cell position q (unit cells): x radius (0 centre, 1 edge),
// y height, z per-scale random, w position along v (-1 top, 1 free edge).
// `k` 1 adds the per-scale ridges.
fn rc_scales_c(b: RecBlock, q: vec2<f32>, k: f32) -> vec4<f32> {
    let v = b.variant;
    var d = vec2<f32>(0.0);
    var id = vec2<f32>(0.0);
    var r = 0.0;
    if (v == 2u) {
        id = floor(q) + vec2<f32>(0.5);
        d = (q - id) * 2.0;
        r = mix(length(d), max(abs(d.x), abs(d.y)), b.na.y);
    } else if (v == 0u) {
        // Shingles: the two scales one row up (towards the head) lie over this
        // one where they reach it; round edges, no hex cell showing.
        let h = rc_hex(q);
        id = h.zw;
        d = h.xy;
        let da = d + vec2<f32>(-0.5, 0.866);
        let db = d + vec2<f32>(0.5, 0.866);
        let rr = 0.62 + 0.12 * b.na.y;
        if (min(length(da), length(db)) < rr) {
            d = select(db, da, length(da) < length(db));
            id = id + select(vec2<f32>(-0.5, -0.866), vec2<f32>(0.5, -0.866), length(da) < length(db));
        }
        d = d / rr;
        r = length(d);
    } else {
        let h = rc_hex(q);
        id = h.zw;
        d = h.xy * 2.0;
        r = mix(length(d), 2.0 * max(abs(h.x), abs(h.x) * 0.5 + abs(h.y) * 0.866), b.na.y);
    }
    let z = rc_u01(rc_hash2(vec2<i32>(round(id * 2.0)), b.seed));
    var h = 0.0;
    if (v == 0u) {
        h = 1.0 - smoothstep(0.3, 1.05, r) + 0.25 * (0.5 + 0.5 * d.y);
    } else if (v == 3u) {
        h = (1.0 - smoothstep(0.0, 1.0, r)) * (0.6 + 0.4 * z);
    } else {
        h = 1.0 - smoothstep(0.75, 1.0, r) + 0.2 * (1.0 - min(r, 1.0));
    }
    h = h + k * 0.07 * sin(r * 16.0 + z * 6.0) * (1.0 - min(r, 1.0));
    return vec4<f32>(r, h, z, d.y);
}

fn rc_scales(b: RecBlock, f: MatFrame, i: MatIn, q: u32) -> MatOut {
    // A whole number of cells round u, so a trunk or ball shows no seam.
    let cs = 1.0 / max(round(1.0 / rc_range(b.na.x, 0.003, 0.2)), 1.0);
    let hi = select(0.0, 1.0, q >= 3u);
    let ka = rc_aa(f, cs);
    let c = rc_scales_c(b, i.uv / cs, hi);
    let m = rc_scales_p(b, f, i.uv + select(vec2<f32>(0.0), vec2<f32>(c.z - 0.5, -0.6 * c.w) * cs, b.nb.w > 0.1)) * b.nb.x;
    let edge = rc_fade(smoothstep(0.8, 1.05, c.x), 0.2, ka);
    let rim = rc_fade(smoothstep(0.62, 0.95, c.x) * smoothstep(0.0, 0.7, c.w), 0.1, ka) * select(0.0, 1.0, b.variant == 0u);
    var col = mix(mix(b.col_a, b.col_b, m.x), b.col_c, m.y) * rc_fade(0.85 + 0.3 * c.z, 1.0, ka);
    col = mix(col, b.col_b * 0.4, edge * 0.7) + vec3<f32>(rim * 0.12);
    var o = rc_out(i, col, b.nb.z, 0.0);
    o.occ = i.occ * (1.0 - 0.4 * edge * b.na.z);
    if (q >= 2u) {
        let ndv = clamp(dot(i.n, f.v), 0.0, 1.0);
        let irid = b.nb.y * select(0.0, 1.0, rc_extra(b, RC_X_SCALES, 1u));
        if (irid > 0.0) {
            // A hue that turns with the view and differs scale to scale.
            let hue = vec3<f32>(0.5) + 0.5 * cos(6.2831853 * (vec3<f32>(0.0, 0.33, 0.67) + vec3<f32>(ndv * 1.1 + c.z * 0.25)));
            let tint = mix(b.col_c, hue, 0.4);
            o.base = vec4<f32>(mix(o.base.rgb, tint * (0.5 + 0.5 * c.y), irid * 0.45 * (0.3 + 0.7 * (1.0 - ndv))), o.base.a);
            o.sheen = vec4<f32>(tint * (irid * 0.5 * (1.0 - 0.5 * ndv)), 2.0);
            if (q >= 3u) {
                // One scale at a time catches the sun.
                o.glint = tint * (irid * 0.5 * ka * (1.0 - smoothstep(0.0, 0.1, abs(ndv - (0.3 + 0.6 * fract(c.z * 13.7))))));
            }
        }
        let near = rc_near(f) * ka * select(0.0, 1.0, rc_extra(b, RC_X_SCALES, 0u));
        if (near > 0.0) {
            let e = 0.1;
            let g = vec2<f32>(rc_scales_c(b, i.uv / cs + vec2<f32>(e, 0.0), hi).y, rc_scales_c(b, i.uv / cs + vec2<f32>(0.0, e), hi).y) - vec2<f32>(c.y);
            o.n = rc_bump(i.n, rc_tuv(f, i.n), g * (1.4 * b.na.z * near));
        }
    }
    return o;
}
