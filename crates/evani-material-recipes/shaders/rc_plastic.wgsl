// evani.shader.rc.plastic
// Plastic (recipe 3, batch 1 build plan section 3, package P9). Written fresh
// for Evani.Dev: plain, rubber, foam and tread, a translucent wrap, specks and
// tyre tread, all in uv space.
//   numbers: na = rough, translucency, speck amount, speck scale (1-200 per uv);
//            nb = tread scale (1-64 per uv), tread depth, mottle, seam scale
//            (0-16 sheets per uv)
//   colours: A colour, B speck (and the sheet's mottle)
//   variants: 0 plain, 1 rubber (matte, fine mottle), 2 foam (open pores), 3 tread,
//             4 sheet (batch 3: linoleum and roofing felt: a mottle of B
//             streaked along u by the mottle amount, and lapped seams across v,
//             a seam scale apart, each strip its own tone)
//   extras: translucency (bit 0), specks (bit 1), tread (bit 2: chevron blocks
//   and ring grooves, any variant)
// Levels (q, the detail group): lite draws what low draws
// (every tier keeps the pattern; before critic round 1 lite was the plain lit colour); low adds the
// specks, the mottle, the pores and the tread, anti-aliased (cells under 1.5 px
// fade to their average); medium adds the tread, pore and grain normals within
// 25 m and the translucent rim; high adds a finer grain in the roughness;
// ultra draws as high.

// Tread: 1 in a groove. Chevron lines slanting away from a centre line, and a
// ring groove every four blocks.
fn rc_plastic_g(uv: vec2<f32>, ts: f32) -> f32 {
    let p = uv * ts;
    let c = fract(p.x + abs(fract(p.y * 0.5) * 2.0 - 1.0) * 0.5);
    let r = fract(p.x * 0.25);
    return max(1.0 - smoothstep(0.06, 0.18, min(c, 1.0 - c)), 1.0 - smoothstep(0.02, 0.06, min(r, 1.0 - r)));
}

// Foam: 1 in an open pore.
fn rc_plastic_p(b: RecBlock, uv: vec2<f32>, sp: f32) -> f32 {
    let c = rc_voronoi2(uv * sp * 0.25, b.seed + 1u, 1.0);
    return (1.0 - smoothstep(0.12, 0.3, c.x)) * step(c.w, 0.6);
}

// Sheet seams: x the distance across v to the nearest seam (in strips), y
// the strip's number. A seam scale of 0 has none.
fn rc_plastic_s(uv: vec2<f32>, ss: f32) -> vec2<f32> {
    let v = uv.y * ss;
    return vec2<f32>(select(9.0, abs(fract(v + 0.5) - 0.5), ss > 0.01), floor(v + 0.5));
}

fn rc_plastic_h(b: RecBlock, uv: vec2<f32>, sp: f32, ts: f32, d: f32) -> f32 {
    var h = -rc_plastic_g(uv, ts) * d;
    if (b.variant == 4u) {
        // The lap: each strip's lower edge stands proud of the next.
        let ss = rc_range(b.nb.w, 0.0, 16.0);
        h = h + 0.3 * (1.0 - smoothstep(0.0, 0.04, rc_plastic_s(uv, ss).x)) * step(0.5, fract(uv.y * ss + 0.5));
    }
    if (b.variant == 2u) {
        h = h - rc_plastic_p(b, uv, sp) * 0.5;
    } else if (b.variant == 1u) {
        h = h + rc_vnoise2(uv * sp * 0.5, b.seed + 2u) * 0.04;
    }
    return h;
}

fn rc_plastic(b: RecBlock, f: MatFrame, i: MatIn, q: u32) -> MatOut {
    var c = b.col_a;
    var rough = b.na.x;
    if (b.variant == 1u) {
        rough = max(rough, 0.6);
    } else if (b.variant == 2u) {
        rough = max(rough, 0.9);
    }
    let sp = rc_range(b.na.w, 1.0, 200.0);
    let ts = rc_range(b.nb.x, 1.0, 64.0);
    var d = 0.0;
    if (rc_extra(b, RC_X_PLASTIC, 2u)) {
        d = b.nb.y;
    }
    if (rc_extra(b, RC_X_PLASTIC, 1u)) {
        let s = rc_voronoi2(i.uv * sp, b.seed, 1.0);
        let sd = (1.0 - smoothstep(0.1, 0.22, s.x)) * step(s.w, b.na.z * 0.7);
        c = mix(c, b.col_b, rc_fade(sd, b.na.z * 0.05, rc_aa(f, 0.5 / sp)) * 0.85);
    }
    if (b.variant == 4u) {
        // Mottle: B streaked along u through a warped noise; a dark line under
        // each lap, each strip a little lighter or darker.
        let m = rc_fbm2(i.uv * vec2<f32>(3.0, 18.0) + 3.0 * rc_fbm2(i.uv * vec2<f32>(2.0, 12.0), b.seed + 4u, 3u), b.seed + 5u, 4u);
        c = mix(c, b.col_b, rc_fade(smoothstep(0.45, 0.75, m), 0.25, rc_aa(f, 0.02)) * b.nb.z);
        let ss = rc_range(b.nb.w, 0.0, 16.0);
        let s = rc_plastic_s(i.uv, ss);
        c = c * (1.0 + step(0.01, ss) * (0.12 * rc_u01(rc_hash2(vec2<i32>(i32(s.y), 0), b.seed)) - 0.06 - 0.5 * rc_fade(1.0 - smoothstep(0.0, 0.03, s.x), 0.03, rc_aa(f, 0.03 / max(ss, 0.01)))));
    } else if (b.variant == 2u) {
        c = c * (1.0 - 0.4 * rc_fade(rc_plastic_p(b, i.uv, sp), 0.15, rc_aa(f, 1.0 / sp)));
    } else if (b.variant == 1u) {
        c = c * (0.94 + 0.12 * rc_fade(rc_vnoise2(i.uv * sp * 0.5, b.seed + 2u), 0.5, rc_aa(f, 2.0 / sp)));
    }
    if (d > 0.0) {
        let g = rc_fade(rc_plastic_g(i.uv, ts), 0.3, rc_aa(f, 0.1 / ts)) * d;
        c = c * (1.0 - 0.5 * g);
        rough = mix(rough, 0.95, g);
    }
    if (q >= 3u) {
        rough = rough + 0.1 * (rc_vnoise2(i.uv * sp * 3.0, b.seed + 3u) - 0.5) * rc_aa(f, 0.3 / sp);
    }
    var o = rc_out(i, c, rough, 0.0);
    if (q >= 2u) {
        if (rc_extra(b, RC_X_PLASTIC, 0u)) {
            o.sheen = vec4<f32>(b.col_a * (1.5 * b.na.y), 2.0);
        }
        let near = rc_near(f);
        if ((d > 0.0 || b.variant == 1u || b.variant == 2u || b.variant == 4u) && near > 0.0) {
            let fq = select(select(sp * 0.3, ts, d > 0.0), rc_range(b.nb.w, 1.0, 16.0) * 8.0, b.variant == 4u);
            let e = 0.15 / fq;
            let h0 = rc_plastic_h(b, i.uv, sp, ts, d);
            let g = vec2<f32>(rc_plastic_h(b, i.uv + vec2<f32>(e, 0.0), sp, ts, d), rc_plastic_h(b, i.uv + vec2<f32>(0.0, e), sp, ts, d)) - vec2<f32>(h0);
            o.n = rc_bump(i.n, rc_tuv(f, i.n), g * (4.0 * near * rc_aa(f, 0.3 / fq) / fq / e));
        }
    }
    return o;
}
