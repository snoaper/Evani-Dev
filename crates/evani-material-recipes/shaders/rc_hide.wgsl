// evani.shader.rc.hide
// Hide (recipe 15, batch 1 build plan section 3, package P6; batch 5 B5-P2
// adds bristles and plates). Written fresh for Evani.Dev: skin that is only a
// surface, no layers. Wrinkles are ridged noise, warts the domes of a cellular
// noise, blotches the cells of a second one; wetness is a lower roughness and a
// darker, glossier colour; scatter is a wrap of light bleeding through thin
// edges (a rim tint). Plates are large cellular cells split by deep folds
// (rhino). Bristles are sparse short hairs standing along v, drawn in a colour
// that contrasts with the skin under them (pale on pink, black on dark), so a
// spotted pig has both. Uv space.
//   numbers: na = wrinkle depth, wrinkle scale (1-50 per uv), wetness,
//            scatter; nb = rough, spots (blotch amount), bristles (how many
//            of the cells have a hair), plate scale (1-20 per uv); nc =
//            bristle scale (10-200 cells per uv)
//   colours: A colour, B wrinkle (and blotches, and the folds), C scatter
//   variants: 0 plain, 1 wrinkled, 2 warty, 3 slick, 4 plates
//   extras: wrinkle relief (bit 0: the normals), scatter (bit 1), bristles
//           (bit 2)
// Levels (q, the skin group): lite draws what low draws
// (every tier keeps the pattern; before critic round 1 lite was the lit colour with painted wrinkles
// and wetness); low adds blotches and pore shade, anti-aliased (detail under
// 1.5 px fades to its average), wrinkle normals within 25 m and the scatter
// rim (critic round 3 of material-view: without the normals Low read as
// smooth veins where Ultra showed a deep crackle); high adds a finer second
// wrinkle octave; ultra draws as high.

// The wrinkle (or plate) scale, per uv.
fn rc_hide_w(b: RecBlock) -> f32 {
    return select(rc_range(b.na.y, 1.0, 50.0), rc_range(b.nb.w, 1.0, 20.0), b.variant == 4u);
}

// The skin at uv: x height (0 deep in a crease, 1 high), y crease mask, z a
// plate's own number. `k` 1 adds the finer octave. `px` is a pixel in
// pattern units: a crease is never drawn thinner than about a pixel, so the
// crease mesh still reads where a sphere is small (critic round 5: plain on
// a phone page while wrinkled on desktop).
fn rc_hide_s(b: RecBlock, uv: vec2<f32>, k: f32, px: f32) -> vec3<f32> {
    let v = b.variant;
    let p = uv * rc_hide_w(b);
    if (v == 2u) {
        // Warts: domes over a pitted skin.
        let c = rc_voronoi2(p, b.seed, 1.0);
        let dome = (1.0 - smoothstep(0.1, 0.42, c.x)) * step(c.w, 0.6);
        return vec3<f32>(0.3 + 0.7 * dome, (1.0 - dome) * 0.5, 0.0);
    }
    if (v == 4u) {
        // Plates: a fold wherever two cells meet, a fine crackle on each.
        let c = rc_voronoi2(p, b.seed, 0.85);
        let fold = 1.0 - smoothstep(0.03, 0.14, c.z);
        return vec3<f32>((1.0 - 0.85 * fold) * (0.8 + 0.2 * rc_vnoise2(p * 5.0, b.seed + 5u)), fold, c.w);
    }
    let w = (rc_vnoise2(p * 0.5, b.seed + 2u) - 0.5) * 1.2;
    let n = rc_fbm2(p * 0.6 + vec2<f32>(w, -w), b.seed + 1u, 3u);
    // Ridged: creases where the noise crosses its middle.
    let wid = max(select(0.12, select(0.04, 0.09, v == 1u), v != 0u), 0.9 * px);
    var cr = (1.0 - smoothstep(0.0, wid, abs(n - 0.5))) * select(1.0, 0.35, v == 0u);
    if (v == 1u) {
        // Wrinkled (round 5): a second crease set, stretched across the
        // first, so the creases cross in a mesh of small cells (elephant
        // skin), not lone wandering lines (a map).
        let n2 = rc_vnoise2(vec2<f32>(p.x * 1.4 + p.y * 0.5, p.y * 0.4 - p.x * 0.3) + vec2<f32>(-w, w), b.seed + 7u);
        cr = max(cr, 0.85 * (1.0 - smoothstep(0.0, max(0.07, 0.9 * px), abs(n2 - 0.5))));
    }
    var hgt = 1.0 - cr * 0.8;
    if (k > 0.5) {
        // The finer octave (round 5): small creases between the main ones,
        // in the height only, so High and Ultra keep Low's crease network.
        hgt = hgt - 0.25 * (1.0 - smoothstep(0.0, 0.1, abs(rc_vnoise2(p * 3.1, b.seed + 3u) - 0.5))) * (1.0 - cr);
    }
    return vec3<f32>(hgt, cr * select(1.0, 0.25, v == 3u), 0.0);
}

// A bristle at uv: 1 on a hair, else 0. One hair at most per cell, rooted low
// in it, leaning a little as it rises along v.
fn rc_hide_h(b: RecBlock, uv: vec2<f32>, hs: f32) -> f32 {
    let p = uv * hs;
    let h = rc_hash2(vec2<i32>(floor(p)), b.seed + 6u);
    let r = vec3<f32>(rc_u01(h), rc_u01(rc_pcg(h)), rc_u01(rc_pcg(h + 1u)));
    let q = fract(p) - vec2<f32>(0.25 + 0.5 * r.x, 0.1);
    let a = q.y * 1.25;
    let w = 0.14 * (1.0 - 0.6 * a);
    return step(r.y, b.nb.z) * step(0.0, a) * step(a, 1.0) * (1.0 - smoothstep(0.5 * w, w, abs(q.x - (r.z - 0.5) * 0.2 * a)));
}

fn rc_hide(b: RecBlock, f: MatFrame, i: MatIn, q: u32) -> MatOut {
    let ws = rc_hide_w(b);
    let px = f.uv_px * ws;
    let s = rc_hide_s(b, i.uv, select(0.0, 1.0, q >= 3u), px);
    // Kept while the creases (about two cells apart) span a few pixels.
    let ka = rc_aa(f, 0.5 / ws);
    let cr = rc_fade(s.y, 0.2, ka) * b.na.x;
    var c = mix(b.col_a, b.col_b, cr);
    // Blotches: a second cellular noise, darker, with soft rims.
    let bl = rc_voronoi2(i.uv * ws * 0.35, b.seed + 4u, 1.0);
    let bm = (1.0 - smoothstep(0.15 + 0.2 * fract(bl.w * 5.3), 0.4, bl.x)) * step(bl.w, b.nb.y * 0.8) * rc_aa(f, 0.3 / ws);
    c = mix(c, b.col_b * 0.55, bm * 0.8) * (0.92 + 0.16 * rc_fade(s.x, 0.6, ka)) * (1.0 - 0.3 * b.na.z) * select(1.0, 0.88 + 0.24 * s.z, b.variant == 4u);
    if (b.nb.z > 0.0 && rc_extra(b, RC_X_HIDE, 2u)) {
        // Bristles: pale on light skin, black on dark.
        let hs = rc_range(b.nc.x, 10.0, 200.0);
        let hv = rc_fade(rc_hide_h(b, i.uv, hs), b.nb.z * 0.05, rc_aa(f, 0.14 / hs));
        c = mix(c, mix(c * 0.12, vec3<f32>(0.85, 0.62, 0.3), smoothstep(0.08, 0.22, dot(c, vec3<f32>(0.3, 0.6, 0.1)))), hv);
    }
    var o = rc_out(i, c, mix(b.nb.x, 0.1, b.na.z), 0.0);
    if (q >= 1u) {
        if (rc_extra(b, RC_X_HIDE, 1u)) {
            // Wrap scatter: light bleeds through thin, grazing edges.
            o.sheen = vec4<f32>(b.col_c * (1.2 * b.na.w), 2.2);
        }
        // The relief only while a fifth of a cell spans 1.5 px (sooner, its
        // normals sparkle on a small sphere; the creases' colour stays).
        let near = rc_near(f) * rc_aa(f, 0.15 / ws);
        if (rc_extra(b, RC_X_HIDE, 0u) && near > 0.0) {
            let e = select(0.12, 0.05, b.variant == 4u) / ws;
            let k = select(0.0, 1.0, q >= 3u);
            let g = vec2<f32>(rc_hide_s(b, i.uv + vec2<f32>(e, 0.0), k, px).x, rc_hide_s(b, i.uv + vec2<f32>(0.0, e), k, px).x) - vec2<f32>(s.x);
            o.n = rc_bump(i.n, rc_tuv(f, i.n), g * (6.0 * b.na.x * near * select(1.0, 2.0, b.variant == 4u)));
        }
    }
    return o;
}
