// evani.shader.rc.ceramic
// Ceramic (recipe 23, batch 1 build plan section 3, package P4). Written
// fresh for Evani.Dev. A body under a glaze.
//   numbers: na = glaze gloss, glaze depth, crackle amount, crackle scale
//            (1-100 per uv); nb = speck amount, drip, tile zoom (1-8: tiles
//            per uv are 8 times it), tile ratio (0.5-4: a subway tile's or
//            roof tile's width over its height); nc = barrel (0-1: a roof
//            tile's half-round curve), lustre (0-1, P8-4: how much of the
//            glaze the lustre film covers)
//   colours: A body, B glaze, C speck
//   variants: 0 earthenware, 1 stoneware, 2 porcelain, 3 terracotta
//             (unglazed), 4 enamel, 5 tile (8 x 8 tiles per uv, grout in A),
//             6 subway (ratio : 1, running bond), 7 hex (hexagons), 8 mosaic
//             (tesserae a third of a tile, the motif in a pattern of B and C),
//             9 roof (clay tiles in shingle courses, each over the next, the
//             lap shadowed; barrel curves the terracotta ones)
//   extras: crackle (bit 0), glaze depth (bit 1), motif (bit 2: a painted
//           motif on every tile of the 8 x 8 grid, from the layout helper,
//           batch 3 B3-P1: petals and corners in C, the centre in A)
//           lustre (bit 3, P8-4: a thin metal film on the glaze, colour B
//           shifting a little through the hues with the view;
//           metallic by the lustre number; every tier)
// Levels (q, the detail group): lite draws what low draws
// (every tier keeps the pattern; before critic round 1 lite was one gloss lobe over the mixed
// colour); low adds the glaze thinning on edges and pooling in creases, drips
// and pooling swells (the drip number scales both), specks, crackle, enamel
// chips and tile grout, anti-aliased; medium adds glaze depth (a deeper colour
// and a second, clear rim at grazing angles), porcelain's warm translucent
// edge and tile bevels within 25 m; high and ultra draw as medium. Thick glaze
// runs deeper in colour (creamy brown for earthenware and stoneware, cool
// blue-white for porcelain, enamel and tile). Enamel has no crackle, so its
// crackle amount and scale mean chips (the dark body shows, mostly on edges);
// each tile is fired a little differently. The motif draws anti-aliased on
// every tier (each tile's hand-painted wobble its own).

// The tile of the layout at p (tile units; a tile is about 1 across): e the
// distance to its grout, id its own number, uv where p is inside it. The
// square tile (5) is batch 1's unchanged.
fn rc_ceramic_cell(b: RecBlock, p: vec2<f32>) -> LayCell {
    let v = b.variant;
    let s = b.seed + 13u;
    if (v == 6u) {
        return rc_lay_bond(p, vec2<f32>(rc_range(b.nb.w, 0.5, 4.0), 1.0), 0u, s);
    }
    if (v == 7u) {
        return rc_lay_hex(p, 1.0, s);
    }
    if (v == 8u) {
        return rc_lay_bond(p * 3.0, vec2<f32>(1.0), 1u, s);
    }
    if (v == 9u) {
        return rc_lay_shingle(p, vec2<f32>(rc_range(b.nb.w, 0.5, 4.0), 1.0), 0.12, s);
    }
    let t = fract(p);
    let d = min(t, vec2<f32>(1.0) - t);
    return LayCell(min(d.x, d.y), rc_u01(rc_hash2(vec2<i32>(floor(p)), s)), t, 0.0);
}

// A tile's relief at p: 0 in the grout, rising over a bevel to 1; a roof
// tile also stands out at its free edge and curves across (barrel).
fn rc_ceramic_h(b: RecBlock, p: vec2<f32>) -> f32 {
    let c = rc_ceramic_cell(b, p);
    var h = smoothstep(0.0, 0.06, c.e);
    if (b.variant == 9u) {
        h = h * (1.0 - 0.5 * c.uv.y + 0.5 * b.nc.x * sin(3.14159 * c.uv.x));
    }
    return h;
}

// The painted motif's ink at p (tile units), one per tile: x the petals and
// corners (colour C), y the centre (colour A); edges eased over wd.
fn rc_ceramic_motif(b: RecBlock, p: vec2<f32>, wd: f32) -> vec2<f32> {
    let m = rc_lay_motif(fract(p) * 2.0 - vec2<f32>(1.0), b.seed, 0.08, rc_u01(rc_hash2(vec2<i32>(floor(p)), b.seed + 13u)));
    return vec2<f32>(1.0 - smoothstep(-wd, wd, min(m.x, m.z)), 1.0 - smoothstep(-wd, wd, m.y));
}

// A mosaic's colour from its tesserae: B, with a big motif (rosette, star,
// corner circles) picked out in C and its centre lighter, each tessera cut
// to the pattern at its own centre so the edges step. Tesserae are mosaic
// cells q (a third of a tile) of a 12-cell motif tile.
fn rc_ceramic_mosaic(b: RecBlock, q: vec2<f32>) -> vec3<f32> {
    let m = rc_lay_motif(fract((floor(q) + vec2<f32>(0.5)) / 12.0) * 2.0 - vec2<f32>(1.0), b.seed, 0.0, 0.5);
    var c = b.col_b;
    if (min(m.x, m.z) < 0.0) {
        c = b.col_c;
    }
    if (m.y < 0.0) {
        c = mix(b.col_b, b.col_c, 0.5) * 1.25;
    }
    return c;
}

fn rc_ceramic(b: RecBlock, f: MatFrame, i: MatIn, q: u32) -> MatOut {
    let v = b.variant;
    // How much glaze covers the body (a roof tile is mostly bare clay).
    let cover = select(select(select(select(1.0, 0.3, v == 9u), 0.0, v == 3u), 0.85, v == 1u), 0.75, v == 0u);
    let gr = mix(0.5, 0.04, b.na.x);
    // Tile units: 8 tiles per uv times the zoom.
    let zm = rc_range(b.nb.z, 1.0, 8.0);
    let sc = 8.0 * zm;
    let p = i.uv * sc;
    // Glaze thickness: thin on convex edges (the body breaks through),
    // pooled in creases and in broad swells, run down in drips.
    let dr = rc_vnoise2(vec2<f32>((i.world.x + i.world.z) * 9.0, i.world.y * 1.2), b.seed);
    let pool = rc_vnoise3(i.world * 7.0, b.seed + 9u) * 0.65 + rc_vnoise3(i.world * 19.0, b.seed + 11u) * 0.35;
    let th = b.na.y - smoothstep(3.0, 25.0, f.curv) * 0.6 + smoothstep(3.0, 25.0, -f.curv) * 0.5 + (dr + pool - 1.0) * b.nb.y * 1.5;
    let a = cover * clamp(0.35 + th * 0.9, 0.0, 1.0);
    var cl = LayCell(1.0, 0.5, vec2<f32>(0.5), 0.0);
    var tone = 0.0;
    if (v >= 5u) {
        cl = rc_ceramic_cell(b, p);
        tone = cl.id - 0.5;
    }
    let body = b.col_a * (0.9 + 0.2 * rc_fade(rc_vnoise2(i.uv * 30.0, b.seed + 1u), 0.5, rc_aa(f, 0.03)));
    let deep = select(vec3<f32>(0.74, 0.84, 1.0), vec3<f32>(0.9, 0.78, 0.55), v <= 1u);
    var gc = b.col_b;
    if (v == 8u) {
        // Each tessera its own cut of the pattern's colour.
        gc = mix(b.col_b, b.col_c, 0.3) + (rc_ceramic_mosaic(b, p * 3.0) - mix(b.col_b, b.col_c, 0.3)) * rc_aa(f, 0.3 / sc);
    }
    let glaze = gc * mix(vec3<f32>(1.0), deep, clamp((th - 0.35) * 3.0, 0.0, 1.0)) * (1.0 + tone * 0.08 * rc_aa(f, 0.125 / zm));
    var c = mix(body, glaze, a);
    // Iron specks bleed up through the glaze.
    let sp = rc_voronoi2(i.uv * 60.0, b.seed + 3u, 1.0);
    c = mix(c, b.col_c, (1.0 - smoothstep(0.08, 0.14, sp.x)) * step(sp.w, b.nb.x * 0.5) * rc_aa(f, 0.017));
    if (rc_extra(b, RC_X_CERAMIC, 2u)) {
        // The motif, painted under the glaze; a tile under 1.5 px shows
        // its average.
        let ka = rc_aa(f, 0.02 / zm);
        let k = rc_ceramic_motif(b, p, max(0.03, f.uv_px * sc * 2.0));
        c = mix(mix(c, b.col_c, rc_fade(k.x, 0.3, ka)), b.col_a, rc_fade(k.y, 0.08, ka));
    }
    if (rc_extra(b, RC_X_CERAMIC, 0u)) {
        // Crackle: a net of fine stained cracks in the glaze.
        let cs = rc_range(b.na.w, 1.0, 100.0);
        let cr = rc_fade(1.0 - smoothstep(0.02, 0.06, rc_voronoi2(i.uv * cs, b.seed + 5u, 1.0).z), 0.15, rc_aa(f, 0.06 / cs));
        c = mix(c, c * 0.45, cr * b.na.z * a);
    }
    var chip = 0.0;
    if (v == 4u) {
        // Enamel chips, mostly on convex edges.
        let cp = vec2<f32>(i.world.x + i.world.z * 0.6, i.world.y * 1.1 + i.world.z * 0.3) * (rc_range(b.na.w, 1.0, 100.0) * 0.75);
        let cv = rc_voronoi2(cp, b.seed + 7u, 1.0);
        let reach = 0.12 + 0.2 * rc_vnoise2(cp * 4.0, b.seed + 8u);
        let want = b.na.z * (0.3 + 1.2 * smoothstep(2.0, 20.0, f.curv));
        chip = step(cv.w, want) * (1.0 - smoothstep(reach, reach + 0.04, cv.x)) * rc_aa_world(f, 0.02);
        c = mix(c, b.col_a, chip);
    }
    var gt = 0.0;
    var occ_k = 0.4;
    if (v >= 5u) {
        // Grout between the tiles (a mosaic's is wider, a roof's the gaps
        // between clay tiles) and, on a roof, the shadow each course throws on
        // the one below.
        let gw = select(0.03, 0.07, v == 8u);
        let ga = select(0.006 / zm, 0.0042 / zm, v == 8u);
        gt = rc_fade(1.0 - smoothstep(gw, select(0.05, 0.12, v == 8u), cl.e), 0.14, rc_aa(f, ga));
        c = mix(c, b.col_a * 0.8, gt);
        if (v == 9u) {
            let lap = smoothstep(0.55, 1.0, cl.uv.y);
            c = c * (1.0 - 0.45 * lap * rc_aa(f, 0.1 / zm));
            // The barrel: darker toward the sides, light on the crown.
            c = c * (1.0 - 0.3 * b.nc.x * (1.0 - sin(3.14159 * cl.uv.x)) * rc_aa(f, 0.1 / zm));
            occ_k = 0.6;
        }
    }
    var lm = 0.0;
    if (rc_extra(b, RC_X_CERAMIC, 3u)) {
        // Lustre: a thin metal film on the glaze, its hue drifting with the
        // view, over colour B.
        lm = b.nc.y * a;
        c = mix(c, b.col_b * (vec3<f32>(0.6) + 0.4 * cos(6.2832 * (vec3<f32>(dot(i.n, f.v) * 1.2) + vec3<f32>(0.0, 0.33, 0.67)))), lm);
    }
    var o = rc_out(i, c, mix(mix(mix(0.85, gr, a), 0.5, chip), 0.9, gt), lm);
    o.occ = i.occ * (1.0 - occ_k * gt);
    if (q >= 2u) {
        if (rc_extra(b, RC_X_CERAMIC, 1u)) {
            // Glaze depth: at grazing angles the eye looks through more glaze
            // (a deeper colour) and its clear top coat adds a second rim.
            let fr = pow(1.0 - clamp(dot(i.n, f.v), 0.0, 1.0), 3.0) * a * b.na.y;
            o.base = vec4<f32>(mix(o.base.rgb, o.base.rgb * b.col_b, fr), o.base.a);
            o.sheen = vec4<f32>(vec3<f32>(0.3 * a * b.na.x), 5.0);
        }
        if (v == 2u) {
            // Thin porcelain glows warm where light passes through its edge.
            o.sheen = vec4<f32>(vec3<f32>(1.0, 0.92, 0.84) * (0.45 * a), 2.5);
        }
        let near = rc_near(f);
        if (v >= 5u && near > 0.0) {
            // Tile bevels: each tile's edge rounds down into the grout (a
            // roof tile also stands over the one below and curves across).
            let e = 0.002;
            let h = rc_ceramic_h(b, p);
            let d = vec2<f32>(rc_ceramic_h(b, (i.uv + vec2<f32>(e, 0.0)) * sc), rc_ceramic_h(b, (i.uv + vec2<f32>(0.0, e)) * sc)) - vec2<f32>(h);
            o.n = rc_bump(i.n, rc_tuv(f, i.n), d * (0.06 * near / (e * zm)));
        }
    }
    return o;
}
