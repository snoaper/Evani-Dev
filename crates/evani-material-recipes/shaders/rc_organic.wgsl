// evani.shader.rc.organic
// Organic (recipe 22, batch 1 build plan section 3, package P9). Written fresh
// for Evani.Dev: flesh, bread, fruit, slime and mushroom, with wetness, a
// scatter rim, crumb (holes, spots or seeds, by variant) and marbling, in uv
// space.
//   numbers: na = wetness, scatter, crumb amount, crumb scale (1-100 per uv);
//            nb = rough, marbling
//   colours: A colour 1, B colour 2 (marbling, crumb spots, seeds)
//   variants: 0 flesh (fat flecks; with the food part, fibres or flakes:
//   crumb amount shades the gaps, crumb scale sets how many), 1 bread (dark
//   holes), 2 fruit (mottle and seeds), 3 slime (always wet, bubbles), 4
//   mushroom (pale spots), 6 pastry (with the food part a woven lattice crust
//   over a filling in B; without it a plain crust), 5 kernels (with the food
//   part; without it plain)
//   (P8-4: rows of rounded glossy kernels, crumb scale of them along v and
//   across u, each its own tone, B dark between; on every tier, domed from
//   medium)
//   extras: scatter (bit 0), clear (bit 1, material-view round 4: a
//           see-through body, crate::blend draws it over what is behind)
// Slime (material-view round 4; before it the bubbles were only promised):
// the light it carries inside glows in colour B (times scatter), strongest
// at the thin rim and looking into the sun, on every tier. With clear on,
// its body lets a share of what is behind through (more with scatter),
// densest at the rim, and holds bubbles (rings with a lit cap, crumb scale
// sets their size); an opaque slime (a sauce) shows none.
// The food part (material-view round 6: meat, fish, jerky and pie did not
// read as themselves): shaders/rc_organic_food.wgsl, feature
// `recipe-organic-food`, defines rc_organic_f (colour and roughness) and
// rc_organic_fh (height) for flesh, kernels and pastry;
// shaders/rc_organic_plain.wgsl stands in without it.
// Levels (q, the detail group): lite draws what low draws
// (every tier keeps the pattern; before critic round 1 lite was the flat colour); low adds the mottle,
// marbling and crumb, anti-aliased; medium adds lump and crumb normals within
// 25 m and the scatter rim; high adds a finer second grain; ultra draws as high.

// Crumb: 1 in a hole, spot or seed of the cellular noise.
fn rc_organic_c(b: RecBlock, uv: vec2<f32>, cs: f32) -> f32 {
    let c = rc_voronoi2(uv * cs, b.seed, 1.0);
    return (1.0 - smoothstep(0.1, 0.32, c.x)) * step(c.w, b.na.z);
}

fn rc_organic_h(b: RecBlock, uv: vec2<f32>, cs: f32) -> f32 {
    // The lumps are gentle (round 5: at 0.4 the foods read as cauliflower on
    // Ultra) and follow the marbling (round 6: a smooth cheese came out
    // lumpy); crumb holes, spots and seeds keep their full relief; flesh and
    // pastry take the food part's.
    // Bread's holes sink by the crumb amount (round 6: a sparse cheese's
    // deep holes caught white rims on Ultra only).
    let s = select(0.5, -b.na.z, b.variant == 1u);
    return rc_vnoise2(uv * cs * 0.4, b.seed + 2u) * 0.1 * b.nb.y + select(rc_organic_c(b, uv, cs) * s, rc_organic_fh(b, uv, cs), b.variant == 0u || b.variant > 4u);
}

fn rc_organic(b: RecBlock, f: MatFrame, i: MatIn, q: u32) -> MatOut {
    let cs = rc_range(b.na.w, 1.0, 100.0);
    let fl = b.variant == 0u;
    let wet = max(b.na.x, select(0.0, 0.85, b.variant == 3u));
    var c = b.col_a;
    var rough = mix(b.nb.x, 0.06, wet);
    let ka = rc_aa(f, 1.0 / cs);
    let m = rc_fbm2(i.uv * (cs * 0.35), b.seed + 1u, 3u);
    // Meat (flesh whose fat, colour B, is lighter than A): the food part
    // draws its fat as seams between muscle bundles (rc_organic_mx is false
    // without the part, which keeps the plain flecks below).
    let meat = rc_organic_mx(b);
    c = mix(c, b.col_b, (m - 0.5) * 0.5 * select(select(1.0, 0.3, b.variant == 3u || fl), 0.0, meat));
    if (b.nb.y > 0.0 && !meat) {
        // Broad bands; in flesh small flecks of fat along the grain (round 6:
        // round 5's thin contour veins read as marbled candy).
        let mf = rc_fbm2(i.uv * vec2<f32>(cs * 0.3, cs * 0.1), b.seed + 4u, 3u);
        let t = select(0.5, 0.6, fl);
        c = mix(c, b.col_b, rc_fade(smoothstep(t, t + 0.08, mf), select(0.25, 0.1, fl), rc_aa(f, 2.0 / cs)) * b.nb.y);
    }
    if (b.na.z > 0.0 && !fl) {
        let k = rc_fade(rc_organic_c(b, i.uv, cs), b.na.z * 0.1, ka);
        c = mix(c, select(b.col_b, b.col_b * 0.4, b.variant == 1u), k * 0.85);
        // A hole's inside is matte (material-view critic round 7: on Ultra
        // the cheese's holes caught the light as white beads, dark on Low).
        rough = mix(rough, 0.95, k * select(0.0, 1.0, b.variant == 1u));
    }
    rc_organic_f(b, i.uv, &c, &rough, cs, ka);
    if (q >= 3u) {
        c = c * (0.94 + 0.12 * rc_fade(rc_vnoise2(i.uv * cs * 4.0, b.seed + 3u), 0.5, rc_aa(f, 0.25 / cs)));
    }
    c = c * (1.0 - 0.25 * wet);
    let n0 = clamp(dot(i.n, f.v), 0.0, 1.0);
    let rim = (1.0 - n0) * (1.0 - n0);
    var bub = 0.0;
    let clr = rc_extra(b, RC_X_ORGANIC, 1u);
    if (b.variant == 3u && clr) {
        let v = rc_voronoi2(i.uv * (cs * 0.5), b.seed + 5u, 1.0);
        let r = 0.1 + 0.2 * fract(v.w * 13.7);
        bub = rc_fade((smoothstep(r - 0.06, r, v.x) - smoothstep(r, r + 0.03, v.x) + 0.8 * (1.0 - smoothstep(0.0, 0.05, length(v.xy - vec2<f32>(r * 0.4, r + 0.3)) - r * 0.3))) * step(v.w, 0.45), 0.05, ka);
        c = mix(c * (0.7 + 0.3 * rim), b.col_b * 1.2, bub);
    }
    var o = rc_out(i, c, rough, 0.0);
    if (b.variant == 3u) {
        let l = frame.light_dir.xyz;
        o.emit = b.col_b * i.tex.rgb * i.vcol * (b.na.y * (0.1 + 0.55 * rim + 0.6 * pow(max(-dot(l, f.v), 0.0), 2.0)));
        if (clr) {
            o.base.a = o.base.a * clamp(mix(0.95, 0.4, b.na.y) + 0.6 * rim + 0.5 * bub, 0.0, 1.0);
        }
    }
    if (q >= 2u) {
        if (rc_extra(b, RC_X_ORGANIC, 0u)) {
            o.sheen = vec4<f32>(mix(b.col_a, b.col_b, 0.3) * (1.6 * b.na.y), 2.0);
        }
        let near = rc_near(f) * ka;
        if (near > 0.0) {
            let e = 0.15 / cs;
            let h0 = rc_organic_h(b, i.uv, cs);
            let g = vec2<f32>(rc_organic_h(b, i.uv + vec2<f32>(e, 0.0), cs), rc_organic_h(b, i.uv + vec2<f32>(0.0, e), cs)) - vec2<f32>(h0);
            // Bread's holes dip gently: at full relief their rims tilted to
            // the sun and read as beads on Ultra only (critic round 7).
            o.n = rc_bump(i.n, rc_tuv(f, i.n), g * (4.0 * near / (cs * e)) * select(1.0, 0.35, b.variant == 1u));
        }
    }
    return o;
}
