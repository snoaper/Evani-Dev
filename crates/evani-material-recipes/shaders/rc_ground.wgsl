// evani.shader.rc.ground
// Ground (recipe 9, batch 1 build plan section 3, package P5). Written fresh
// for Evani.Dev: stones are the nearest cells of a cellular noise (each a
// parabolic dome) over noise soil, with a sand ripple and sparkle grains.
// World space (plan 2.3): one dominant-axis projection up to Medium, two
// blended from High, so a slope shows no hard seam.
//   numbers: na = rough, stone size (0.005-0.5 m), stone amount, ripple
//            amount; nb = ripple scale (1-50 per m), sparkle, clump, crack
//            scale (1-50 per m); nc = furrow scale (1-50), cover
//   colours: A soil, B stone, C sand tint (also the crust of cracked plates,
//            the cover strokes and the edges of ruts)
//   variants: 0 soil, 1 sand, 2 gravel, 3 mud, 4 road, 5 crushed (angular
//             fragments, each a whole cell with straight sides, a flat top
//             chamfered to the gaps; scree, ballast, cinders; clump = pores)
//   extras: parallax pebbles (bit 0), sparkle (bit 1), cracks (bit 2),
//           furrows (bit 3), cover (bit 4)
// Levels (q, the detail group): lite draws what low draws
// (every tier keeps the pattern; before critic round 1 lite was a flat two-octave noise); low draws
// the clods, stones, ripples and standing water, anti-aliased (detail under
// 1.5 px fades to its average); medium adds the relief of stones, ripples and
// clods as normals within 25 m; high adds sparkle, the blended projection and
// 2-step parallax of the stones within 8 m; ultra parallaxes everywhere.
// Batch 2 options (all off unless the row lists the extra, so every earlier
// row draws as before): cracks are the F2-F1 lines of a cellular noise (dark
// gaps between shaded plates; with sparkle high the plates turn to the crust
// colour: alkali); furrows are uv-space rows (ploughed: along u, one per
// 1/scale of v) or, on the road variant, two ruts at fixed v with edges tinted
// colour C; cover is cells of short strokes (twigs, needles, straw) over
// up-facing faces only. Cracks parallax at high with the stones' path.
// Crumb (Asset Studio column 69, dark loam; no new number or extra, so every
// row draws as before): on soil, cracks with clump over 0.8 break the plates
// up into fine porous earth, a pattern in space (no projection: the grain
// keeps its size on any slope). Soft lumps of a third and a sixth of a plate,
// crumbs (balls in cells a quarter of a plate wide) and two sizes of grain
// (an eighth and a twelfth), each grain a tone of its own, a few pale ones in
// the stone colour, dark pores where no grain stands, more of them in the
// hollows between the lumps. Clump says how loose it lies: 1 a tilled crumb
// full of hollows (in patches, with closer earth between), just over 0.8 a
// clod's compact face (low lumps, few pores). From medium the relief, and its own shade: a lump standing between
// a point and the sun darkens it. No parallax, no grazing glint.

// How much of the ground is stone, for the variant.
fn rc_ground_sa(b: RecBlock) -> f32 {
    let v = b.variant;
    if (v == 2u || v == 5u) {
        return max(b.na.z, 0.9);
    }
    if (v == 1u) {
        return b.na.z * 0.3;
    }
    if (v == 3u) {
        return b.na.z * 0.15;
    }
    if (v == 4u) {
        return b.na.z * 0.6;
    }
    return b.na.z;
}

// The stone at plane point p: x the dome (0..1), y its id, z its mask.
fn rc_ground_k(b: RecBlock, p: vec2<f32>) -> vec3<f32> {
    let cr = b.variant == 5u;
    let c = rc_voronoi2(p / (rc_range(b.na.y, 0.005, 0.5) * 2.0), b.seed, select(0.8, 0.6, cr));
    let r = fract(c.w * 17.3);
    let rad = select(0.22 + 0.2 * r, 0.38 + 0.12 * r, b.variant == 2u);
    let has = step(c.w, rc_ground_sa(b));
    let t = c.x / rad;
    if (cr) {
        // Crushed: each cell is one fragment with straight sides (the cell's
        // own borders), a flat top chamfered towards the gaps, a height of its own.
        return vec3<f32>(min(c.z * 3.0, 1.0) * (0.55 + 0.45 * r) * has, c.w, smoothstep(0.02, 0.07, c.z) * has);
    }
    return vec3<f32>(max(1.0 - t * t, 0.0) * has, c.w, (1.0 - smoothstep(0.85, 1.0, t)) * has);
}

// The crack plates at p: x the gap mask (1 on a crack), y the plate's id.
fn rc_ground_cr(b: RecBlock, p: vec2<f32>) -> vec2<f32> {
    let c = rc_voronoi2(p * rc_range(b.nb.w, 1.0, 50.0), b.seed + 9u, 1.0);
    return vec2<f32>(1.0 - smoothstep(0.0, 0.07, c.z), c.w);
}

// The furrows at uv: x how deep in a furrow or rut, y the rut's edge band.
fn rc_ground_f(b: RecBlock, uv: vec2<f32>) -> vec2<f32> {
    let fs = rc_range(b.nc.x, 1.0, 50.0);
    let a = abs(uv.y - 0.5);
    let w = 0.2 / fs;
    if (b.variant == 4u) {
        return vec2<f32>(1.0 - smoothstep(0.5 * w, w, abs(a - 0.5 / fs)), smoothstep(0.5 / fs + w, 0.5 / fs + 2.0 * w, a));
    }
    return vec2<f32>(0.5 - 0.5 * cos(6.2832 * uv.y * fs), 0.0);
}

// The cover at p: x a stroke's mask, y its shade.
fn rc_ground_cv(b: RecBlock, p: vec2<f32>) -> vec2<f32> {
    let q = p / (rc_range(b.na.y, 0.005, 0.5) * 1.5);
    let h = rc_hash2(vec2<i32>(floor(q)), b.seed + 11u);
    let r = vec2<f32>(rc_u01(h), rc_u01(rc_pcg(h)));
    let a = 6.2832 * rc_u01(rc_pcg(h + 1u));
    let d = fract(q) - vec2<f32>(0.5) - (r - vec2<f32>(0.5)) * 0.2;
    let l = vec2<f32>(d.x * cos(a) + d.y * sin(a), d.y * cos(a) - d.x * sin(a));
    return vec2<f32>((1.0 - smoothstep(0.05, 0.1, abs(l.y))) * (1.0 - smoothstep(0.22, 0.3, abs(l.x))) * step(r.y, b.nc.y), r.x);
}

// The sand ripple at p, 0 in a trough to 1 on a crest.
fn rc_ground_r(b: RecBlock, p: vec2<f32>) -> f32 {
    let rs = rc_range(b.nb.x, 1.0, 50.0);
    return 0.5 + 0.5 * sin(6.2832 * (dot(p, vec2<f32>(0.94, 0.34)) * rs + rc_fbm2(p * rs * 0.12, b.seed + 4u, 2u) * 3.0));
}

// Soil whose cracked plates break up into crumb (clump over 0.8).
fn rc_ground_cm(b: RecBlock) -> bool {
    return b.variant == 0u && b.nb.z > 0.8 && rc_extra(b, RC_X_GROUND, 2u);
}

// The crumb's grains: balls anywhere in the cells of p's lattice, each with a
// radius of its own under half a cell (so the 8 cells round p hold every
// ball that reaches it). x the tallest cap at p (0: none reaches it), y that
// ball's number (0..1; the larger the ball, the higher).
fn rc_ground_gr(p: vec3<f32>, seed: u32) -> vec2<f32> {
    let c0 = vec3<i32>(floor(p - vec3<f32>(0.5)));
    var o = vec2<f32>(0.0);
    for (var k = 0; k < 8; k = k + 1) {
        let c = c0 + vec3<i32>(k & 1, (k >> 1u) & 1, k >> 2u);
        let h = rc_hash3(c, seed);
        let d = vec3<f32>(c) + vec3<f32>(vec3<u32>(h, h >> 10u, h >> 20u) & vec3<u32>(1023u)) / 1023.0 - p;
        let g = rc_u01(rc_pcg(h));
        let r = 0.3 + 0.2 * g;
        let s = 1.0 - dot(d, d) / (r * r);
        if (s > o.x) {
            o = vec2<f32>(s, g);
        }
    }
    return o;
}

// The crumb at world point w: x how open it lies to the light (0 deep in a
// pore or a hollow to 1), y its tone, z its relief in metres, w a pale
// grain. kb says how much of the lumps, the crumbs and the grains survive
// the anti-alias fade; full: with the tone and the pores (the relief alone
// needs neither).
fn rc_ground_cb(b: RecBlock, w: vec3<f32>, kb: vec3<f32>, full: bool) -> vec4<f32> {
    let cs = rc_range(b.nb.w, 1.0, 50.0);
    let q = w * cs;
    // Loose here, lying closer there, in patches of a plate or two.
    let lo = smoothstep(0.82, 1.0, b.nb.z) * mix(0.35, 1.0, smoothstep(0.3, 0.7, rc_vnoise3(q * 0.55, b.seed + 14u)));
    let n0 = mix(0.5, rc_vnoise3(q * 2.9 + vec3<f32>(3.1, 0.0, 0.0), b.seed + 13u), kb.x);
    let n1 = mix(0.5, rc_vnoise3(q.yzx * 6.2 + vec3<f32>(0.0, 7.7, 0.0), b.seed + 12u), kb.y);
    let n2 = mix(0.5, rc_vnoise3(q.zxy * 13.6 + vec3<f32>(1.3, 0.0, 0.0), b.seed + 11u), kb.z);
    let a = rc_ground_gr(q.zxy * 4.2 + vec3<f32>(0.0, 5.3, 0.0), b.seed + 21u).x * kb.y;
    let g = rc_ground_gr(q * 7.5 + vec3<f32>(1.7, 0.0, 0.0), b.seed + 22u) * vec2<f32>(kb.z, 1.0);
    let z = (mix(0.2, 1.0, lo) * (0.45 * n0 + 0.3 * n1) + 0.08 * n2 + 0.16 * a + 0.05 * g.x) / cs;
    if (!full) {
        return vec4<f32>(0.0, 0.0, z, 0.0);
    }
    let f = rc_ground_gr(q.yzx * 12.0 + vec3<f32>(0.0, 2.9, 0.0), b.seed + 23u);
    // How high the point stands among its neighbours (0 in a hollow).
    let top = smoothstep(0.3, 0.75, 0.5 * n0 + 0.35 * n1 + 0.15 * n2 + 0.25 * a);
    let has = step(1e-4, max(g.x, f.x) * kb.z);
    let t = select(f.y, g.y, g.x > 0.0);
    // Pores: where no grain stands, most of them in the hollows.
    let pore = mix(0.3, (1.0 - has) * mix(0.4, 1.0, lo), kb.z) * (1.0 - 0.8 * top);
    return vec4<f32>(
        (1.0 - 0.62 * pore) * (1.0 - 0.62 * (1.0 - top) * mix(0.15, 1.0, lo)),
        (0.55 + mix(0.3, t * t, kb.z)) * (0.5 + top) * (1.0 + 0.2 * a),
        z,
        step(0.9, t) * has * 0.8 * top
    );
}

// The height at p in metres: stone domes, ripples and clods (soil and mud),
// less the crack gaps.
fn rc_ground_h(b: RecBlock, p: vec2<f32>) -> f32 {
    let clump = select(0.0, b.nb.z, b.variant == 0u || b.variant == 3u);
    var h = rc_ground_k(b, p).x * rc_range(b.na.y, 0.005, 0.5) * 0.35 + rc_ground_r(b, p) * 0.006 * b.na.w + smoothstep(0.35, 0.65, rc_fbm2(p * 14.0, b.seed + 3u, 3u)) * 0.02 * clump;
    if (rc_extra(b, RC_X_GROUND, 2u)) {
        h = h - rc_ground_cr(b, p).x * 0.012;
    }
    return h;
}

// The colour at p (w how wet it is: standing water in mud); kg and kr how
// much stone and ripple detail survive the anti-alias fade (kc the cracks').
fn rc_ground_c(b: RecBlock, p: vec2<f32>, kg: f32, kr: f32, kc: f32) -> vec4<f32> {
    let v = b.variant;
    let k = rc_ground_k(b, p);
    let cl = rc_fbm2(p * 14.0, b.seed + 3u, 3u);
    let gr = 0.9 + 0.2 * rc_fade(rc_vnoise2(p * 120.0, b.seed + 5u), 0.5, kg);
    let rip = rc_fade(rc_ground_r(b, p), 0.5, kr);
    var c = b.col_a * (0.7 + 0.6 * mix(0.5, cl, b.nb.z)) * gr;
    var wet = 0.0;
    if (v == 1u) {
        c = mix(b.col_c * (0.8 + 0.3 * rip), b.col_a, (1.0 - rip) * b.na.w * 0.25) * (0.92 + 0.16 * gr);
    } else if (v == 2u || v == 5u) {
        c = b.col_a * 0.45 * gr;
    } else if (v == 3u) {
        wet = 1.0 - smoothstep(0.3, 0.45, cl);
        c = mix(c * 0.8, b.col_a * 0.4, wet);
    } else if (v == 4u) {
        c = mix(b.col_a, b.col_b, 0.55) * (0.8 + 0.4 * rc_fade(rc_vnoise2(p * 30.0, b.seed + 6u), 0.5, kg));
    }
    let sc = mix(b.col_b * 0.65, b.col_b * 1.2, fract(k.y * 11.7));
    var st = mix(sc, b.col_c, 0.3 * fract(k.y * 3.3)) * (0.75 + 0.25 * k.x);
    if (v == 5u) {
        // Pores (clump says how many): dark pits in each fragment.
        st = st * (1.0 - 0.75 * b.nb.z * rc_fade(smoothstep(0.55, 0.65, rc_vnoise2(p / rc_range(b.na.y, 0.005, 0.5) * 2.5, b.seed + 8u)), 0.0, kg));
    }
    c = mix(c, st, rc_fade(k.z, 0.5 * rc_ground_sa(b), kg));
    if (rc_extra(b, RC_X_GROUND, 2u) && !rc_ground_cm(b)) {
        // Cracks: each plate shaded alone (the crust colour at high sparkle),
        // dark in the gaps.
        let x = rc_ground_cr(b, p);
        c = mix(c * (0.85 + 0.3 * fract(x.y * 9.1)), b.col_c, b.nb.y * 0.85);
        c = c * (1.0 - 0.7 * rc_fade(x.x, 0.15, kc));
    }
    return vec4<f32>(c, wet);
}

// The parallax height at p: the stones, or the crack plates.
fn rc_ground_ph(b: RecBlock, p: vec2<f32>, cx: bool) -> f32 {
    if (cx) {
        return 1.0 - rc_ground_cr(b, p).x;
    }
    return rc_ground_k(b, p).x;
}

fn rc_ground(b: RecBlock, f: MatFrame, i: MatIn, q: u32) -> MatOut {
    var w = i.world;
    let ng = normalize(i.g);
    let ssz = rc_range(b.na.y, 0.005, 0.5);
    let rs = rc_range(b.nb.x, 1.0, 50.0);
    let v = b.variant;
    let kg = rc_aa_world(f, ssz);
    let kr = rc_aa_world(f, 1.0 / rs);
    let cx = rc_extra(b, RC_X_GROUND, 2u);
    let cs = rc_range(b.nb.w, 1.0, 50.0);
    let kc = rc_aa_world(f, 1.0 / cs);
    let cm = rc_ground_cm(b);
    // The crumb's lumps, crumbs and grains: 0.3, 0.16 and 0.07 of a plate.
    let kb = vec3<f32>(rc_aa_world(f, 0.3 / cs), rc_aa_world(f, 0.16 / cs), rc_aa_world(f, 0.07 / cs));
    if (q >= 3u && !cm && ((rc_extra(b, RC_X_GROUND, 0u) && rc_ground_sa(b) > 0.0) || cx) && (q == 4u || f.dist < 8.0)) {
        // Parallax, two steps: the view slides across stones standing proud
        // (or into the cracks).
        let dn = dot(f.v, ng);
        let ofs = (f.v - ng * dn) / max(dn, 0.3) * select(0.35 * ssz * kg, 0.01 * kc, cx);
        let h1 = rc_ground_ph(b, rc_proj(w, ng), cx);
        let h2 = rc_ground_ph(b, rc_proj(w - ofs * (1.0 - h1), ng), cx);
        w = w - ofs * (1.0 - h2);
    }
    var s: vec4<f32>;
    if (q >= 3u && !cm) {
        let pp = rc_proj2(w, ng);
        s = mix(rc_ground_c(b, pp.a, kg, kr, kc), rc_ground_c(b, pp.b, kg, kr, kc), pp.w);
    } else {
        s = rc_ground_c(b, rc_proj(w, ng), kg, kr, kc);
    }
    var cb = vec4<f32>(0.0);
    // The crumb's own shade, on average (lite and low, and far off).
    var shade = 0.25;
    if (cm) {
        // Crumb: each grain its own tone, the pale ones in the stone's
        // colour (dry crumbs, bits of root), the pores and hollows dark and
        // a little cool, as shade is.
        cb = rc_ground_cb(b, w, kb, true);
        s = vec4<f32>(mix(s.rgb * cb.y, b.col_b * 0.85, cb.w) * cb.x * mix(vec3<f32>(0.85, 1.0, 1.2), vec3<f32>(1.0), cb.x), s.w);
    }
    let fx = rc_extra(b, RC_X_GROUND, 3u);
    let kf = rc_aa(f, 0.1 / rc_range(b.nc.x, 1.0, 50.0));
    if (fx) {
        // Furrows and ruts: darker in the trough, the edge band in colour C.
        let u = rc_ground_f(b, i.uv);
        s = vec4<f32>(mix(s.rgb, b.col_c, rc_fade(u.y, 0.0, kf) * 0.8) * (1.0 - 0.5 * rc_fade(u.x, 0.5, kf)), max(s.w, 0.5 * u.x * kf));
    }
    if (rc_extra(b, RC_X_GROUND, 4u)) {
        // Cover: strokes of twigs, needles or straw, on faces that look up.
        let u = rc_ground_cv(b, rc_proj(w, ng));
        let m = rc_fade(u.x, 0.3 * b.nc.y, kg) * smoothstep(0.5, 0.8, ng.y);
        s = vec4<f32>(mix(s.rgb, b.col_c * (0.6 + 0.8 * u.y), m * 0.9), s.w * (1.0 - m));
    }
    var o = rc_out(i, s.rgb, mix(select(b.na.x, min(b.na.x, 0.3), v == 3u), 0.04, s.w), 0.0);
    if (q >= 2u) {
        let near = rc_near(f) * max(max(kg, kr), select(0.0, kb.y, cm));
        if (near > 0.0 && cm) {
            // The crumb's relief (the step is a grain's rim), and its own
            // shade: two looks towards the light, 0.06 and 0.175 of a plate
            // off; a lump standing over the line to the light shades the
            // point.
            let t = rc_tw(ng);
            let e = 0.03 / cs;
            let d = vec2<f32>(rc_ground_cb(b, w + t[0] * e, kb, false).z, rc_ground_cb(b, w + t[1] * e, kb, false).z) - vec2<f32>(cb.z);
            o.n = rc_bump(i.n, t, d * (near / e));
            let l = frame.light_dir.xyz;
            let lt = l - ng * dot(l, ng);
            let ll = max(length(lt), 1e-4);
            let u = lt / (ll * cs);
            let up = max(dot(l, ng), 0.0) / (ll * cs);
            shade = mix(shade, max(
                smoothstep(0.0, e, rc_ground_cb(b, w + u * 0.06, kb, false).z - cb.z - 0.06 * up),
                smoothstep(0.0, e, rc_ground_cb(b, w + u * 0.175, kb, false).z - cb.z - 0.175 * up)), near);
        } else if (near > 0.0) {
            let t = rc_tw(ng);
            let e = 0.1 * min(ssz * 2.0, 1.0 / rs);
            let h = rc_ground_h(b, rc_proj(w, ng));
            let d = vec2<f32>(rc_ground_h(b, rc_proj(w + t[0] * e, ng)), rc_ground_h(b, rc_proj(w + t[1] * e, ng))) - vec2<f32>(h);
            o.n = rc_bump(i.n, t, d * (near / e));
        }
        if (fx && rc_near(f) * kf > 0.0) {
            // Furrow relief, along the uv tangents; the step is 1/20 of a row.
            let e = 0.05 / rc_range(b.nc.x, 1.0, 50.0);
            let h = rc_ground_f(b, i.uv).x;
            let d = vec2<f32>(rc_ground_f(b, i.uv + vec2<f32>(e, 0.0)).x, rc_ground_f(b, i.uv + vec2<f32>(0.0, e)).x) - vec2<f32>(h);
            o.n = rc_bump(o.n, rc_tuv(f, i.n), d * (-3.0 * rc_near(f) * kf));
        }
    }
    if (cm) {
        // A crumb never shines: its normal stays off the view's horizon, so
        // no lump shows a grazing glint.
        o.n = normalize(o.n + f.v * max(0.3 - dot(o.n, f.v), 0.0));
        // Its shade is a little cool, as the pores are.
        o.base = vec4<f32>(o.base.rgb * (vec3<f32>(1.0) - vec3<f32>(0.8, 0.75, 0.68) * shade), o.base.a);
    }
    if (q >= 3u && rc_extra(b, RC_X_GROUND, 1u) && b.nb.y > 0.0) {
        // Sparkle: grains of mica and quartz catch the sun from some angles.
        let r = rc_u01(rc_hash2(vec2<i32>(floor(rc_proj(w, ng) * 180.0)), b.seed + 7u));
        let tw = step(1.0 - 0.12 * b.nb.y, fract(r * 13.0 + dot(f.v, vec3<f32>(3.1, 5.3, 7.7))));
        o.glint = vec3<f32>(tw * b.nb.y * 2.5 * rc_aa_world(f, 1.0 / 180.0));
    }
    return o;
}
