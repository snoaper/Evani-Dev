// evani.shader.rc.paint
// Paint (recipe 2, batch 1 build plan section 3, packages P9 and P10a).
// Written fresh for Evani.Dev. The recipe draws what the coat lies on (the
// colour, metal flakes, a colour-shift film and a carbon twill) and, after
// lighting, the clear coat itself (rc_paint_coat, called from mat_extra
// through the generated rc_post): a smooth sun lobe on the geometric normal
// and its reflection of the sky, which take their Fresnel share from what
// lies under them. Flakes are tiny mirrors with a random tilt that flash
// where their mirror direction meets the view, so they follow the sun.
//   numbers: na = rough, coat, coat rough, flake amount;
//            nb = flake size (0.2-5, in mm), film, film shift, twill scale
//            (1-64 per uv); nc.x = stud scale (1-20 plates per uv)
//   colours: A base, B under layer (the twill's gaps), C flake
//   variants: 0 solid, 1 metallic, 2 pearl, 3 colour-shift, 4 matte, 5 carbon
//   extras: clearcoat (bit 0: off, no coat and a satin finish), flake (bit 1),
//   film (bit 2), twill (bit 3), studs (bit 4: rivets and plate seams under
//   the paint, girders and hulls, from the layout helper, batch 3 B3-P1)
// Levels (q, the detail group): lite draws what low draws
// (every tier keeps the pattern; before critic round 1 lite was one gloss lobe (the coat folded into
// the base's roughness) with no flakes); low adds the flake speckle, the film
// hue, the twill and the separate coat lobe, anti-aliased; medium adds the tow
// normals within 25 m; high adds the flake glints and a finer thread line;
// ultra draws as high. Studs: lite and low draw the paint pooled dark in the
// seams and round each head, medium the domes and grooves.
// The coat's sky reflection follows the reflections
// group (lite: none). Live: material.recipe.paint.clearcoat (on/off) and
// material.recipe.paint.coat_strength (times every block's coat, misc.w).

// The film's hue at view cosine x (a cosine palette round the hue circle).
fn rc_paint_hue(x: f32) -> vec3<f32> {
    return vec3<f32>(0.5) + 0.5 * cos(6.2832 * (vec3<f32>(x) + vec3<f32>(0.0, 0.33, 0.67)));
}

// Twill: the tow height (0 to 1) of a two-over-two weave at p, and which way
// the tow runs (1 along u, 0 along v).
fn rc_paint_tw(p: vec2<f32>) -> vec3<f32> {
    let c = floor(p);
    let along = step(2.0, (c.x + c.y) - floor((c.x + c.y) * 0.25) * 4.0);
    let fp = fract(p);
    let prof = 1.0 - abs(2.0 * select(fp.x, fp.y, along > 0.5) - 1.0);
    return vec3<f32>(prof, along, 0.0);
}

fn rc_paint(b: RecBlock, f: MatFrame, i: MatIn, q: u32) -> MatOut {
    var c = b.col_a;
    var rough = b.na.x;
    if (b.variant == 4u) {
        rough = max(rough, 0.8);
    }
    if (!rc_extra(b, RC_X_PAINT, 0u)) {
        rough = max(rough, 0.45);
    }
    let ls = rc_range(b.nc.x, 1.0, 20.0);
    if (q == 0u && rc_extra(b, RC_X_PAINT, 0u)) {
        rough = mix(rough, b.na.z, b.na.y);
    }
    let ndv = clamp(dot(i.n, f.v), 0.0, 1.0);
    let fa = b.na.w;
    var fm = 0.0;
    let fs = rc_range(b.nb.x, 0.2, 5.0);
    let cell = 0.002 * fs;
    if (rc_extra(b, RC_X_PAINT, 1u) && fa > 0.0) {
        let h = rc_hash3(vec3<i32>(floor(i.world / cell)), b.seed);
        fm = step(rc_u01(h), fa) * rc_aa_world(f, cell);
        c = mix(c, b.col_c, fm * 0.35 + (1.0 - rc_aa_world(f, cell)) * fa * 0.1);
    }
    if (rc_extra(b, RC_X_PAINT, 2u)) {
        // Pearl and soap film cycle the hue with the view (thin-film rings);
        // colour-shift sweeps once, over half the hue circle from face-on to
        // the edge (faster towards the edge), so it never wraps into rings.
        var x = ndv * 1.2;
        var film = b.nb.y * 0.5;
        if (b.variant == 3u) {
            x = 0.25 * (1.0 - ndv) * (2.0 - ndv);
            film = b.nb.y;
        }
        c = mix(c, rc_paint_hue(x + b.nb.z) * (b.col_a + vec3<f32>(0.25)), film * 0.7);
    }
    var o = rc_out(i, c, rough, 0.0);
    var tw = vec3<f32>(0.0);
    let ts = rc_range(b.nb.w, 1.0, 64.0);
    if (b.variant == 5u && rc_extra(b, RC_X_PAINT, 3u)) {
        tw = rc_paint_tw(i.uv * ts);
        let k = rc_aa(f, 1.0 / ts);
        let tow = mix(0.5, tw.x, k);
        var tc = mix(b.col_b, b.col_a, 0.3 + 0.7 * tow);
        tc = tc * (0.55 + 0.45 * mix(0.5, tw.y, k) + 0.3 * (tow - 0.5));
        if (q >= 3u) {
            tc = tc * (0.92 + 0.08 * step(0.5, fract(i.uv.y * ts * 6.0)) * rc_aa(f, 0.16 / ts));
        }
        o.base = vec4<f32>(tc * i.tex.rgb * i.vcol, i.base.a);
        o.rough = clamp(rough * (0.6 + 0.8 * tw.y), 0.05, 1.0);
    }
    var riv = vec3<f32>(0.0, 1.0, 0.0);
    if (rc_extra(b, RC_X_PAINT, 4u)) {
        // Rivets and plate seams under the paint: it pools dark in the
        // seams and round each head.
        riv = rc_lay_rivets(i.uv, ls, b.seed + 21u);
        let seam = rc_fade(1.0 - smoothstep(0.002, 0.004, riv.y * ls), 0.02, rc_aa(f, 0.004 / ls));
        let ring = rc_fade(smoothstep(0.0, 0.3, riv.x) * (1.0 - riv.x), 0.0, rc_aa(f, 0.06 / ls));
        o.base = vec4<f32>(o.base.rgb * (1.0 - 0.45 * seam - 0.3 * ring), o.base.a);
        o.occ = i.occ * (1.0 - 0.4 * seam);
        let nr = rc_near(f) * rc_aa(f, 0.06 / ls);
        if (q >= 2u && nr > 0.0) {
            let e = 0.004 / ls;
            let r1 = rc_lay_rivets(i.uv + vec2<f32>(e, 0.0), ls, b.seed + 21u);
            let r2 = rc_lay_rivets(i.uv + vec2<f32>(0.0, e), ls, b.seed + 21u);
            let h = riv.x * 0.012 + min(riv.y * ls, 0.006) * 0.5;
            let d = vec2<f32>(r1.x * 0.012 + min(r1.y * ls, 0.006) * 0.5, r2.x * 0.012 + min(r2.y * ls, 0.006) * 0.5) - vec2<f32>(h);
            o.n = rc_bump(o.n, rc_tuv(f, i.n), d * (nr / (e * ls)));
        }
    }
    if (q >= 2u && tw.x > 0.0) {
        let near = rc_near(f) * rc_aa(f, 0.3 / ts);
        if (near > 0.0) {
            let e = 0.1 / ts;
            let g = vec2<f32>(rc_paint_tw((i.uv + vec2<f32>(e, 0.0)) * ts).x, rc_paint_tw((i.uv + vec2<f32>(0.0, e)) * ts).x) - vec2<f32>(tw.x);
            o.n = rc_bump(i.n, rc_tuv(f, i.n), g * (0.6 * near / e / ts));
        }
    }
    if (q >= 3u && fm > 0.0) {
        let h = rc_hash3(vec3<i32>(floor(i.world / cell)), b.seed + 1u);
        let r = vec3<f32>(rc_u01(h), rc_u01(rc_pcg(h)), rc_u01(rc_pcg(h + 1u))) - vec3<f32>(0.5);
        let hv = normalize(frame.light_dir.xyz + f.v);
        o.glint = b.col_c * (pow(max(dot(normalize(i.n + r * 1.4), hv), 0.0), 40.0) * fm * 4.0);
    }
    return o;
}

// The clear coat (P10a), after lighting: c is the lit colour with the other
// post-light terms. Off for other recipes, with recipes off, the clearcoat
// extra off, or at lite (folded into the base there).
fn rc_paint_coat(c: vec3<f32>, o: MatOut, g: vec3<f32>, v: vec3<f32>, l: vec3<f32>, sun: vec3<f32>, vis: f32) -> vec3<f32> {
    let b = rc_unpack(rc_mat);
    if (b.recipe != 2u || (rc_frame.misc.z & 1u) != 0u || !rc_extra(b, RC_X_PAINT, 0u) || rc_q(0u) == 0u) {
        return c;
    }
    let cc = clamp(b.na.y * (1.0 + bitcast<f32>(rc_frame.misc.w)), 0.0, 1.0);
    if (cc <= 0.0) {
        return c;
    }
    let r = max(b.na.z, 0.03);
    let a2 = r * r * r * r;
    let h = normalize(l + v);
    let ndh = max(dot(g, h), 0.0);
    let lh = max(dot(l, h), 0.05);
    let dd = ndh * ndh * (a2 - 1.0) + 1.0;
    let fv = 0.04 + 0.96 * pow(1.0 - clamp(dot(g, v), 0.0, 1.0), 5.0);
    let fs = 0.04 + 0.96 * pow(1.0 - lh, 5.0);
    // GGX times pi (the lit stage's diffuse has no 1/pi), Kelemen's visibility.
    var add = sun * (a2 / (dd * dd * 4.0 * lh * lh) * fs * max(dot(g, l), 0.0) * vis);
    if (rc_q(7u) > 0u) {
        let rv = reflect(-v, g);
        add = add + lit_env(normalize(mix(rv, g, r * r)), r) * (fv * o.occ);
    }
    return c * (1.0 - cc * fv) + add * cc;
}
