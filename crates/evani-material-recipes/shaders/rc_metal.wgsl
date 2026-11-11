// evani.shader.rc.metal
// Metal (recipe 1, batch 1 build plan section 3, package P4). Written fresh
// for Evani.Dev.
//   numbers: na = rough, metal, aniso, grain scale (1-200 per uv);
//            nb = grain strength, edge polish, engrave depth, engrave scale
//            (1-50 per uv); nc.x = layout scale (1-20 plates per uv), nc.y =
//            spot amount
//   colours: A base, B edge polish, C oxide (the oxide layer reads C)
//   variants: 0 plain, 1 brushed, 2 hammered, 3 cast, then batch 3's patterns
//             over the layout scale (cells per uv): 4 diamond plate (raised
//             bars crossing in a chequer), 5 embossed squares (tin ceiling);
//             then the cut-out wire (B3-P2, holes and wire shading from
//             shaders/cut.wgsl, feature `cutout`): 6 grate, 7 chain link,
//             8 mesh, each plain metal here; then batch 9's finishes (P8-4)
//             over the grain scale: 9 damascus (layers of A and B folded into
//             flowing wavy lines), 10 case colours (mottled straw, brown,
//             purple and blue swirls), 11 spangle (zinc crystals about ten
//             grain cells across, each its own tone and gloss, dendrite lines
//             inside), 12 sequins (overlapping discs two grain cells across in
//             staggered rows, each flashing colour B as the view moves, a
//             hole in each)
//   extras: grain (bit 0), engraving (bit 1), relief (bit 2: normal edits),
//           studs (bit 3: riveted plates from the layout helper, batch 3
//           B3-P1; plates 2 by 1 over the layout scale, in a running bond),
//           seams (bit 4: standing seams, raised ridges one sheet apart, each
//           sheet its own tone; the sheet is 1 / layout scale across),
//           spots (bit 5: an old mirror's lost silver, batch 3: dark blotches
//           and specks in colour C, more toward a uv tile's edge, by the
//           spot amount), temper (bit 6, P8-4: the temper colours over any
//           variant, bare steel to straw, brown, purple and blue along v,
//           mirrored every 1 / layout scale: one ramp over a uv tile at 1)
// The finishes and temper draw on every tier (colour and gloss only).
// Levels (q, the detail group): lite draws what low draws
// (every tier keeps the pattern; before critic round 1 lite was the plain metal-rough); low adds the
// grain, the brushed streak, the cast mottle, edge polish and the engraved
// lines, all anti-aliased; medium adds hammered dents, cast pits and engraved
// grooves as normals within 25 m; high and ultra draw as medium. Riveted
// plates: lite draws the seams flat; low adds each plate's tone and the
// polished heads; medium domes the heads and grooves the seams.
// Brushed rows under evani-render's material-layers also get real anisotropy
// from KHR_materials_anisotropy (P10a); this streak is the cheap stand-in and
// draws either way.

// Engraved scrollwork: one spiral groove per hex cell, faded out before the
// cell's edge so cells never meet in a seam. 1 in a groove, 0 outside.
fn rc_metal_eng(uv: vec2<f32>, es: f32) -> f32 {
    let h = rc_hex(uv * es);
    let r = length(h.xy);
    let s = r * 5.0 + atan2(h.y, h.x) * 0.159155;
    return (1.0 - smoothstep(0.15, 0.35, abs(fract(s) - 0.5) * 2.0)) * (1.0 - smoothstep(0.36, 0.48, r));
}

// The relief height at uv: hammered dents (lowest at each cell's point),
// cast pits, then the engraved grooves cut eng deep.
fn rc_metal_h(b: RecBlock, uv: vec2<f32>, gs: f32, es: f32, eng: f32) -> f32 {
    var h = 0.0;
    if (b.variant == 2u) {
        let c = rc_voronoi2(uv * gs * 0.12, b.seed, 0.8);
        h = c.x * c.x;
    } else if (b.variant == 3u) {
        h = rc_vnoise2(uv * gs * 0.6, b.seed) * 0.3;
    }
    return h - rc_metal_eng(uv, es) * eng;
}

// The raised height (0 to 1) of a pattern variant or of the standing seams
// at uv, over the layout scale ls.
fn rc_metal_sp(b: RecBlock, uv: vec2<f32>, ls: f32) -> f32 {
    var h = 0.0;
    if (b.variant == 4u) {
        let q = uv * ls;
        let l = fract(q) - vec2<f32>(0.5);
        let r = rc_lay_rot45(select(l, vec2<f32>(l.x, -l.y), rc_lay_par(floor(q.x) + floor(q.y)) > 0.5));
        h = 1.0 - smoothstep(0.0, 0.05, max(abs(r.x) - 0.3, abs(r.y) - 0.08));
    } else if (b.variant == 5u) {
        h = rc_lay_panel(fract(uv * ls), vec2<f32>(1.0), 0.1, 0.1);
    }
    if (rc_extra(b, RC_X_METAL, 4u)) {
        h = max(h, 1.0 - smoothstep(0.02 / ls, 0.05 / ls, rc_lay_stripe(uv.x, 1.0 / ls)));
    }
    return h;
}

// The temper colours of steel a at heat t (0 bare, then straw, brown,
// purple, blue): the oxide film's hue turned backwards round the hue circle
// from straw through brown, then quickly (and darker) round through red
// and purple to blue.
fn rc_metal_heat(a: vec3<f32>, t: f32) -> vec3<f32> {
    let x = clamp(t, 0.0, 1.0);
    let s = smoothstep(0.35, 0.6, x);
    return mix(a, vec3<f32>(0.5) + 0.5 * cos(6.2832 * (vec3<f32>(0.86 + 0.15 * x + 0.4 * s) + vec3<f32>(0.0, 0.33, 0.67))), 0.7 * smoothstep(0.0, 0.15, x)) * (1.0 - 2.0 * s * (1.0 - s));
}

fn rc_metal(b: RecBlock, f: MatFrame, i: MatIn, q: u32) -> MatOut {
    var c = b.col_a;
    var rough = b.na.x;
    let ls = rc_range(b.nc.x, 1.0, 20.0);
    let gs = rc_range(b.na.w, 1.0, 200.0);
    let es = rc_range(b.nb.w, 1.0, 50.0);
    let p = i.uv * gs;
    if (rc_extra(b, RC_X_METAL, 0u)) {
        var s = rc_vnoise2(p * 2.0, b.seed);
        var k = rc_aa(f, 0.5 / gs);
        if (b.variant == 1u) {
            // Brushed: fine streaks along u; aniso stretches them longer.
            s = rc_fbm2(vec2<f32>(p.x * mix(0.5, 0.02, b.na.z), p.y * 6.0), b.seed, 2u);
            k = rc_aa(f, 0.17 / gs);
        }
        let d = (rc_fade(s, 0.5, k) - 0.5) * b.nb.x;
        c = c * (1.0 + d * 0.8);
        rough = rough * (1.0 + d * 1.2);
        if (b.variant == 3u) {
            // Cast: a duller, mottled skin.
            let m = rc_vnoise2(p * 0.15, b.seed + 3u);
            c = c * (0.85 + 0.2 * m);
            rough = mix(rough, 0.7, 0.4 * m);
        }
    }
    if (b.variant > 8u) {
        // Batch 9's finishes over the grain scale (p is uv times it).
        let w = rc_fbm2(p * 0.15, b.seed + 30u, 3u);
        let k = rc_aa(f, 2.0 / gs);
        if (b.variant == 9u) {
            // Pattern-welded: layers of two steels (A, B), folded and drawn
            // out into flowing wavy lines.
            c = mix(c, b.col_b, rc_fade(smoothstep(0.3, 0.7, abs(fract(p.y * 0.5 + w * 4.0) - 0.5) * 2.0), 0.5, k));
        } else if (b.variant == 10u) {
            // Case colours: mottled swirls of the temper colours, darker.
            c = mix(c, rc_metal_heat(c, rc_fade(rc_fbm2(p * 0.2 + vec2<f32>(w * 3.0), b.seed + 31u, 3u) * 1.8 - 0.4, 0.5, k)), 0.6) * 0.6;
        } else if (b.variant == 11u) {
            // Spangle: zinc crystals, each its own tone and gloss, with fine
            // dendrite lines its own way and a dark line where crystals meet.
            let s = rc_voronoi2(p * 0.1 + vec2<f32>(w), b.seed + 32u, 1.0);
            let a = s.w * 6.2832;
            c = c * rc_fade((0.75 + 0.5 * s.w) * (0.9 + 0.2 * rc_vnoise2(vec2<f32>(dot(p, vec2<f32>(cos(a), sin(a))) * 3.0, s.w * 50.0), b.seed)) * mix(0.6, 1.0, smoothstep(0.0, 0.04, s.z)), 1.0, k);
            rough = rough * mix(1.0, fract(s.w * 7.0) * 1.6, k);
        } else {
            // Sequins: overlapping discs in staggered rows, each lower disc
            // over the ones above it; each a small mirror tilted its own way,
            // so it flashes colour B as the view moves; a hole in the middle.
            let qs = p * 0.5;
            let ca = floor(qs) + vec2<f32>(0.5);
            let cb = floor(qs + vec2<f32>(0.5));
            let ch = select(cb, ca, ca.y > cb.y);
            let cc = select(ca + cb - ch, ch, length(qs - ch) < 0.62);
            let r = length(qs - cc);
            let h = rc_u01(rc_hash2(vec2<i32>(cc * 2.0), b.seed + 33u));
            c = mix(c * (0.6 + 0.6 * h), b.col_b, smoothstep(0.7, 0.95, fract(h * 9.0 + dot(f.v, vec3<f32>(2.1, 3.3, 1.7))))) * rc_fade(smoothstep(0.06, 0.1, r) * (1.0 - 0.6 * smoothstep(0.45, 0.62, r)), 0.8, k);
        }
    }
    if (rc_extra(b, RC_X_METAL, 6u)) {
        // Temper colours along v (a triangle wave of layout-scale periods).
        c = rc_metal_heat(c, 1.0 - abs(1.0 - 2.0 * fract(i.uv.y * ls * 0.5)) + 0.08 * rc_vnoise2(i.uv * 8.0, b.seed + 34u));
    }
    if (rc_extra(b, RC_X_METAL, 5u)) {
        // Lost silver: blotches of a warped noise with ragged, speckled
        // edges, likelier toward the edge of the uv tile (a mirror's frame);
        // the dark backing in C still reflects a little, as old silvering does.
        let t = abs(fract(i.uv) - 0.5);
        let s = rc_fade(smoothstep(1.0, 1.04, rc_fbm2(i.uv * 4.0, b.seed + 5u, 4u) + 0.35 * smoothstep(0.3, 0.5, max(t.x, t.y)) + 0.45 * b.nc.y + 0.15 * rc_vnoise2(p, b.seed + 6u)), 0.3 * b.nc.y, rc_aa(f, 0.5 / gs));
        c = mix(c, b.col_c, s);
    }
    // Polished edges: convex edges (not the gentle curve of a whole ball)
    // brighten toward colour B and smooth out.
    let e = smoothstep(4.0, 30.0, f.curv) * b.nb.y;
    c = mix(c, b.col_b, e);
    rough = rough * (1.0 - 0.6 * e);
    var eng = 0.0;
    var g = 0.0;
    if (rc_extra(b, RC_X_METAL, 1u)) {
        let k = rc_aa(f, 0.06 / es);
        eng = b.nb.z * k;
        g = rc_fade(rc_metal_eng(i.uv, es), 0.12, k) * b.nb.z;
        c = mix(c, c * 0.3, g);
    }
    var riv = vec3<f32>(0.0, 1.0, 0.0);
    var seam = 0.0;
    if (rc_extra(b, RC_X_METAL, 3u)) {
        // Riveted plates: each plate rolled a little differently, a dark
        // seam between them, the heads rubbed bright toward colour B with a
        // dark ring where they meet the plate.
        riv = rc_lay_rivets(i.uv, ls, b.seed + 21u);
        seam = rc_fade(1.0 - smoothstep(0.002, 0.004, riv.y * ls), 0.02, rc_aa(f, 0.004 / ls));
        c = mix(c * (0.92 + 0.16 * riv.z), c * 0.35, seam);
        let kr = rc_aa(f, 0.06 / ls);
        let head = smoothstep(0.0, 0.3, riv.x);
        c = mix(c * (1.0 - 0.4 * rc_fade(head * (1.0 - riv.x), 0.0, kr)), mix(c, b.col_b, 0.6), rc_fade(head * riv.x, 0.0, kr));
    }
    var sp = 0.0;
    let sps = b.variant == 4u || b.variant == 5u || rc_extra(b, RC_X_METAL, 4u);
    if (sps) {
        // Raised bars, squares or seams catch the light; each sheet of a
        // seamed roof is rolled a little differently.
        sp = rc_metal_sp(b, i.uv, ls);
        c = c * (0.88 + 0.28 * rc_fade(sp, 0.4, rc_aa(f, 0.06 / ls)));
        if (rc_extra(b, RC_X_METAL, 4u)) {
            c = c * (0.93 + 0.14 * rc_u01(rc_hash2(vec2<i32>(i32(floor(i.uv.x * ls)), 0), b.seed + 22u)));
        }
    }
    var o = rc_out(i, c, rough, b.na.y);
    o.occ = i.occ * (1.0 - 0.5 * g) * (1.0 - 0.5 * seam);
    if (q >= 2u && rc_extra(b, RC_X_METAL, 2u)) {
        let near = rc_near(f);
        if (near > 0.0) {
            let ep = min(0.4 / gs, 0.04 / es);
            let h = rc_metal_h(b, i.uv, gs, es, eng);
            let d = vec2<f32>(rc_metal_h(b, i.uv + vec2<f32>(ep, 0.0), gs, es, eng), rc_metal_h(b, i.uv + vec2<f32>(0.0, ep), gs, es, eng)) - vec2<f32>(h);
            o.n = rc_bump(i.n, rc_tuv(f, i.n), d * (3.0 * near / (gs * ep)));
        }
    }
    let nr = rc_near(f) * rc_aa(f, 0.06 / ls);
    if (q >= 2u && nr > 0.0 && rc_extra(b, RC_X_METAL, 3u)) {
        // Rivet domes and the seams' grooves.
        let e = 0.004 / ls;
        let r1 = rc_lay_rivets(i.uv + vec2<f32>(e, 0.0), ls, b.seed + 21u);
        let r2 = rc_lay_rivets(i.uv + vec2<f32>(0.0, e), ls, b.seed + 21u);
        let h = riv.x * 0.012 + min(riv.y * ls, 0.006) * 0.5;
        let d = vec2<f32>(r1.x * 0.012 + min(r1.y * ls, 0.006) * 0.5, r2.x * 0.012 + min(r2.y * ls, 0.006) * 0.5) - vec2<f32>(h);
        o.n = rc_bump(o.n, rc_tuv(f, i.n), d * (nr / (e * ls)));
    }
    let ns = rc_near(f) * rc_aa(f, 0.06 / ls);
    if (q >= 2u && ns > 0.0 && sps) {
        // The pattern's or the seams' relief.
        let e = 0.003 / ls;
        let d = vec2<f32>(rc_metal_sp(b, i.uv + vec2<f32>(e, 0.0), ls), rc_metal_sp(b, i.uv + vec2<f32>(0.0, e), ls)) - vec2<f32>(sp);
        o.n = rc_bump(o.n, rc_tuv(f, i.n), d * (0.02 * ns / (e * ls)));
    }
    return o;
}
