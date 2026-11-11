// evani.shader.rc.fur
// Fur (recipe 12, batch 1 build plan section 3, package P6). Written fresh
// for Evani.Dev; ShellFurGodot (Arnklit, MIT) is the idea source only (a coat
// painted from a base colour to a tip colour, darker at the roots, strands and
// locks that vary, a pattern layer, a translucent rim); no shell layers, no
// extra draws, one pass at every level. The coat is painted in uv space:
// pattern space p has x along the hair (the flow angle) and y across it, in
// pattern cells (pattern scale per uv).
//   numbers: na = length, density, sheen, sheen shift; nb = pattern scale
//            (1-50 per uv), pattern contrast, flow angle (0-360 degrees),
//            clump; nc.x = points, nc.y = mix
//   colours: A base (root), B tip, C pattern
//   variants: 0 solid, 1 stripes (across the flow), 2 spots, 3 patches,
//             4 rosettes, 5 dapple (pale round dapples A/B in a
//             net of C); batch 5 (B5-P1):
//             6 roan (single hairs of colour C mixed into the coat; mix = the
//             share, clump = how patchy it is), 7 merle (ragged marbled
//             patches of C over a coat diluted toward B; mix = the dilution),
//             8 mottle (tortoiseshell, calico: coarse patches of B and of C
//             over A, the tip colour is A; mix = how much B against C),
//             9 crimp (wool: crimped bands of C along each lock, soft dark
//             gaps between locks; mix = the gap), 10 reticulated (polygon
//             patches of C split by lines of the coat, the giraffe).
//             With pattern contrast 0 and mix 0 every new variant draws the
//             solid coat (mottle with the tip colour A); the pattern bit
//             switches the new patterns off too.
//   extras: shells (bit 0: reserved for a later shell pass; on, the painted
//           strands soften so shells can draw them), pattern (bit 1)
// Points (dark legs, socks, muzzle) are the thin convex parts of the model
// (curvature), not a uv mask. The sheen rim is a Kajiya-Kay-style term: the
// hair's tangent against the view (the hair shines most where it lies across
// the view), written to o.sheen for mat_extra.
// Levels (q, the fur group): lite draws what low draws
// (every tier keeps the pattern; before critic round 1 lite was the painted coat (base and tip mix,
// pattern, points) and the rim); low adds strands, locks, root darkening and
// the hair-tangent rim, anti-aliased (detail under 1.5 px fades to its
// average); medium adds fibre and lock normals within 25 m; high adds a
// second, finer strand layer and fibre octave; ultra draws as high.

// The pattern mask at pattern position p: 0 coat, 1 pattern colour.
fn rc_fur_pat(b: RecBlock, p: vec2<f32>) -> f32 {
    let v = b.variant;
    if (v == 0u) {
        return 0.0;
    }
    if (v == 1u) {
        return smoothstep(0.1, 0.5, sin(p.x * 3.1416 + (rc_vnoise2(p * 0.6, b.seed) - 0.5) * 5.0));
    }
    if (v == 3u) {
        return smoothstep(0.5, 0.58, rc_fbm2(p * 0.3, b.seed + 2u, 3u));
    }
    if (v == 6u) {
        // Roan (round 5): a white hair is a thin streak along the flow, tapered
        // at both ends, in some of the cells of a grid 2 across and 1 along
        // per pattern cell, each column shifted along at random so the hairs
        // never line up in rows; wide enough to stay over 1.5 px on a phone.
        // Studio material pass (horse.coat.grey_steel): the grid is 3 across
        // and 2 along, so a hair is half a cell long and a third as wide:
        // single fine hairs, not rain dashes (cover per cell unchanged).
        let s = clamp(2.0 * b.nc.y * (1.0 + (rc_fbm2(p * 0.15, b.seed + 6u, 2u) - 0.5) * (1.0 + 3.0 * b.nb.w)), 0.0, 1.0);
        let g = vec2<f32>(p.y * 3.0, p.x * 2.0);
        let col = floor(g.x);
        let ga = g.y + rc_u01(rc_hash2(vec2<i32>(i32(col), 7), b.seed + 7u));
        let h = rc_hash2(vec2<i32>(i32(col), i32(floor(ga))), b.seed);
        let fx = fract(g.x) - 0.5 - (rc_u01(rc_pcg(h)) - 0.5) * 0.4;
        return step(rc_u01(h), s) * (1.0 - smoothstep(0.06, 0.2, abs(fx))) * sqrt(sin(3.1416 * fract(ga)));
    }
    if (v == 7u) {
        return smoothstep(0.54, 0.6, rc_fbm2(p * 0.3 + (rc_fbm2(p * 0.6, b.seed + 8u, 2u) - 0.5) * 1.4, b.seed + 9u, 3u));
    }
    if (v == 8u) {
        return smoothstep(0.55, 0.62, rc_fbm2(p * 0.3, b.seed + 2u, 3u));
    }
    if (v == 9u) {
        return smoothstep(0.3, 0.7, 0.5 + 0.5 * sin(p.x * 9.0 + rc_voronoi2(p * 1.5, b.seed + 5u, 1.0).w * 6.2832 + rc_vnoise2(vec2<f32>(p.y * 4.0, p.x * 0.5), b.seed + 2u) * 4.0));
    }
    let c = rc_voronoi2(p, b.seed + 1u, 0.9);
    if (v == 10u) {
        return smoothstep(0.03, 0.1, c.z) * (0.7 + 0.3 * fract(c.w * 7.31));
    }
    let r = 0.16 + 0.26 * fract(c.w * 7.31);
    if (v == 2u) {
        return (1.0 - smoothstep(r, r + 0.05, c.x)) * step(c.w, 0.6);
    }
    if (v == 4u) {
        return (1.0 - smoothstep(0.03, 0.09, abs(c.x - r * 1.4))) * step(c.w, 0.7);
    }
    // Dapple (studio material pass, horse.coat.grey_dapple): the coat is the
    // pale dapples, round and soft-edged, sizes varied, their edges wobbled by
    // a slow noise so no two are true circles; the pattern colour is the
    // darker net between them (and so also the points: darker legs).
    let rd = 0.24 + 0.14 * fract(c.w * 7.31);
    let d = c.x + (rc_vnoise2(p * 2.2, b.seed + 12u) - 0.5) * 0.14;
    return smoothstep(rd, rd + 0.26, d);
}

// What the pattern fades to far away, and its smallest feature (pattern cells).
// Roan (round 5): twice the mix of the cells hold a hair, which covers about
// 0.23 of its cell, so the far coat is 0.45 of the mix. Dapple: the net
// between the dapples covers about 0.47 of the coat.
fn rc_fur_avg(b: RecBlock) -> f32 {
    if (b.variant == 6u) {
        return b.nc.y * 0.45;
    }
    if (b.variant == 5u) {
        return 0.47;
    }
    return select(0.25, 0.55, b.variant == 9u || b.variant == 10u);
}

fn rc_fur_cell(b: RecBlock) -> f32 {
    return select(select(0.4, 0.17, b.variant == 6u), 0.07, b.variant == 10u);
}

// Relief: lumps of locks plus fibres, in pattern space.
fn rc_fur_h(b: RecBlock, p: vec2<f32>) -> f32 {
    return (1.0 - smoothstep(0.0, 0.6, rc_voronoi2(p * 1.5, b.seed + 5u, 1.0).x)) * (0.3 + b.nb.w) + rc_vnoise2(vec2<f32>(p.y * 6.0, p.x * 1.2), b.seed + 3u) * 0.35;
}

fn rc_fur(b: RecBlock, f: MatFrame, i: MatIn, q: u32) -> MatOut {
    let sc = rc_range(b.nb.x, 1.0, 50.0);
    let ang = b.nb.z * 6.2831853;
    let dir = vec2<f32>(cos(ang), sin(ang));
    let across = vec2<f32>(-dir.y, dir.x);
    let p = vec2<f32>(dot(i.uv, dir), dot(i.uv, across)) * sc;
    var m = 0.0;
    var mb = 0.0;
    let mv = b.variant == 8u;
    if (rc_extra(b, RC_X_FUR, 1u)) {
        let k = rc_aa(f, rc_fur_cell(b) / sc);
        if (b.variant == 6u) {
            // A ladder (material-view critic round 10: the red roan was a
            // plain pink ball at phone size): as single hairs fade under a
            // pixel, tufts of white hairs three times their size take over,
            // softened toward the same average (full contrast read as
            // dashes), so the coat still reads as hair.
            let avg = rc_fur_avg(b);
            let coarse = mix(avg, rc_fade(rc_fur_pat(b, p * 0.33 + vec2<f32>(3.7, 1.3)), avg, rc_aa(f, 3.0 * rc_fur_cell(b) / sc)), 0.7);
            m = mix(coarse, rc_fur_pat(b, p), k) * b.nb.y;
        } else {
            m = rc_fade(rc_fur_pat(b, p), rc_fur_avg(b), k) * b.nb.y;
        }
        if (mv) {
            mb = rc_fade(smoothstep(0.55, 0.62, rc_fbm2(p * 0.3, b.seed + 11u, 3u) + (b.nc.y - 0.5) * 0.3), 0.3, k) * b.nb.y;
        }
        if (b.variant == 7u) {
            mb = b.nc.y * (1.0 - m);
        }
    }
    m = max(m, b.nc.x * smoothstep(8.0, 30.0, f.curv));
    // Strands (fibres: fine across, slow along) and locks (cells of the coat).
    // A short coat lies flat (sleek 1 at length 0.05, 0 from 0.35): its
    // strands show at 0.4 of their contrast, its relief is gentler and it
    // shines like satin, so a horse or dog coat never reads as wood grain or
    // felt (studio material pass, horse.coat.bay).
    let sleek = 1.0 - smoothstep(0.05, 0.35, b.na.x);
    let sk = select(1.0, 0.4, rc_extra(b, RC_X_FUR, 0u)) * (1.0 - 0.6 * sleek);
    var fib = rc_vnoise2(vec2<f32>(p.y * 6.0, p.x * 1.2), b.seed + 3u);
    if (q >= 3u) {
        fib = fib * 0.7 + 0.3 * rc_vnoise2(vec2<f32>(p.y * 28.0, p.x * 4.0), b.seed + 4u);
    }
    // The same ladder for the strands: where they fade under a pixel, a
    // grain four times coarser along the same flow keeps the coat's
    // direction (a small coat never goes flat).
    let fibc = rc_fade(rc_vnoise2(vec2<f32>(p.y * 1.5, p.x * 0.3), b.seed + 13u), 0.5, rc_aa(f, 0.68 / sc));
    fib = mix(fibc, fib, rc_aa(f, 0.17 / sc));
    let lock = rc_fade(1.0 - smoothstep(0.0, 0.6, rc_voronoi2(p * 1.5, b.seed + 5u, 1.0).x), 0.4, rc_aa(f, 0.5 / sc));
    let cl = b.nb.w + select(0.0, 0.8 * b.nc.y, b.variant == 9u);
    let tip = clamp(0.25 + 0.5 * b.na.x + (fib - 0.5) * 0.9 * sk + (lock - 0.4) * 0.5 * cl, 0.0, 1.0);
    var coat = mix(b.col_a, select(b.col_b, b.col_a * 0.85, mv), tip) * mix(0.55, 1.0, b.na.y) * (1.0 - 0.5 * cl * (1.0 - lock));
    coat = mix(coat, b.col_b, mb);
    var o = rc_out(i, mix(coat, b.col_c * (0.85 + 0.3 * fib), m), 0.85 - 0.25 * b.na.z * (1.0 + 0.9 * sleek), 0.0);
    o.occ = i.occ * (1.0 - 0.35 * b.na.x * b.na.y * (1.0 - lock));
    // Kajiya-Kay-style rim: strongest where the hair lies across the view.
    let t = rc_tuv(f, i.n);
    let ht = normalize(t[0] * dir.x + t[1] * dir.y);
    let kk = sqrt(max(1.0 - pow(dot(ht, f.v), 2.0), 0.0));
    o.sheen = vec4<f32>(mix(b.col_a * 1.5, vec3<f32>(0.5) + b.col_b * 0.5, b.na.w) * (b.na.z * (0.35 + 0.65 * kk)), 3.0);
    if (q >= 2u) {
        let near = rc_near(f) * rc_aa(f, 0.3 / sc);
        if (near > 0.0) {
            let e = 0.15;
            let h = rc_fur_h(b, p);
            let g = vec2<f32>(rc_fur_h(b, p + vec2<f32>(e, 0.0)), rc_fur_h(b, p + vec2<f32>(0.0, e))) - vec2<f32>(h);
            o.n = rc_bump(i.n, rc_tuv(f, i.n), (dir * g.x + across * g.y) * (0.9 * near * (0.3 - 0.15 * sleek + b.na.x)));
        }
    }
    return o;
}
