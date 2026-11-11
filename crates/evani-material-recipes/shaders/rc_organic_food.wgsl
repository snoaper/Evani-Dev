// Organic: the food part.
// Organic's food detail (material-view critic round 6: raw meat read as
// marbled candy, cooked fish as marble, jerky grew white beads on Ultra and
// the pie had no crust) and corn's kernels, its own part (feature
// `recipe-organic-food`, appended after shaders/rc_organic.wgsl), so a game
// whose organic rows are fruit, bread, slime or mushrooms never downloads
// it. Written fresh for
// Evani.Dev. shaders/rc_organic_plain.wgsl is the stand-in with the same two
// functions when the part is off.
//   Flesh (variant 0): fibres or flakes running along v, crumb scale of them
//   per two uv across u, wavy; crumb amount shades the gaps (colour on every
//   tier, grooves from medium), so raw meat shows its grain, fish its flakes
//   and jerky its strands.
//   Kernels (variant 5, P8-4): rows of rounded glossy kernels, each its own
//   tone, B dark between; on every tier, domed from medium.
//   Pastry (variant 6): a woven lattice of golden strips, crumb scale of them
//   per uv each way, rounded and browned at their edges, the upper strip at
//   each crossing alternating, over the glossy filling in colour B (colour
//   and shading on every tier, relief from medium).

// Flesh: 1 in the gap between two fibres or flakes.
fn rc_organic_g(b: RecBlock, uv: vec2<f32>, cs: f32) -> f32 {
    let x = uv.x * cs * 0.5 + rc_vnoise2(uv * vec2<f32>(cs * 0.2 + 4.0, cs * 0.05 + 3.0), b.seed + 6u) * 1.5;
    return smoothstep(0.3, 0.5, abs(fract(x) - 0.5));
}

// Pastry: x the lattice's height (a strip passing under another is in
// its shadow beside it, so the weave reads without relief), y 1 on a strip.
fn rc_organic_l(uv: vec2<f32>, cs: f32) -> vec2<f32> {
    let p = uv * cs;
    let d = abs(fract(p) - vec2<f32>(0.5));
    let s = 1.0 - smoothstep(vec2<f32>(0.2), vec2<f32>(0.27), d);
    let t = select(vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), (i32(floor(p.x) + floor(p.y)) & 1) == 0);
    let h = s * (vec2<f32>(1.0) - d * d * 6.0 + 0.4 * t - 0.6 * (vec2<f32>(1.0) - t) * (vec2<f32>(1.0) - smoothstep(vec2<f32>(0.27), vec2<f32>(0.42), d.yx)));
    return vec2<f32>(max(h.x, h.y), max(s.x, s.y));
}

// Kernels at uv: cs rows across u (round a cob) and cs kernels along v (the
// model's uv sets their shape), every other row half a kernel on; x a
// kernel's dome (a rounded box, 0 in the gaps), yz its cell.
fn rc_organic_k(uv: vec2<f32>, cs: f32) -> vec3<f32> {
    var p = uv * cs;
    p.y = p.y + 0.5 * floor(p.x);
    let d = abs(fract(p) - vec2<f32>(0.5)) * 2.0;
    let e = d * d;
    return vec3<f32>(1.0 - smoothstep(0.1, 0.8, dot(e, e)), floor(p));
}

// The height of flesh's grooves, the kernels and the pastry's strips.
fn rc_organic_fh(b: RecBlock, uv: vec2<f32>, cs: f32) -> f32 {
    if (b.variant == 5u) {
        return rc_organic_k(uv, cs).x;
    }
    if (b.variant == 6u) {
        return rc_organic_l(uv, cs).x * 0.12;
    }
    return select(-0.12, -0.04, rc_organic_mx(b)) * b.na.z * rc_organic_g(b, uv, cs);
}

// Flesh with marbling whose colour B (its fat) is lighter than A: meat.
fn rc_organic_mx(b: RecBlock) -> bool {
    let lum = vec3<f32>(0.2126, 0.7152, 0.0722);
    return b.variant == 0u && b.nb.y > 0.0 && dot(b.col_b, lum) > dot(b.col_a, lum) + 0.15;
}

// Meat (material-view critic rounds 6-10: raw meat's broad fat bands read
// as red-and-white camouflage). The muscle is bundles, longer along the grain
// (v), each its own shade of deep red; the fat is thin seams between some of
// the bundles (warped cell edges, wider in places, broken in others: not
// every bundle is ringed) and small flecks inside them, blushing pink at its
// edges; the meat is wet (glossy), the fat less so. Marbling sets how much
// fat. Flesh is meat when its colour B is lighter than A (rc_organic).
fn rc_organic_meat(b: RecBlock, uv: vec2<f32>, c: ptr<function, vec3<f32>>, r: ptr<function, f32>, cs: f32, ka: f32) {
    let p = uv * cs * vec2<f32>(0.14, 0.06);
    let wp = p + (vec2<f32>(rc_fbm2(p * 1.7, b.seed + 21u, 2u), rc_fbm2(p * 1.7 + vec2<f32>(5.3, 1.9), b.seed + 22u, 2u)) - vec2<f32>(0.5)) * 0.7;
    let v = rc_voronoi2(wp, b.seed + 23u, 0.9);
    let soft = 0.05 + 0.14 * (1.0 - ka);
    let w = b.nb.y * (0.01 + 0.07 * smoothstep(0.35, 0.8, rc_fbm2(p * 0.9, b.seed + 24u, 2u)));
    // Each seam a bright core feathering out into the muscle, not a crack.
    let seam = max(1.0 - smoothstep(w, w + soft, v.z), 0.22 * (1.0 - smoothstep(w, 2.2 * w + soft, v.z)));
    var fat = seam * smoothstep(0.38, 0.6, rc_vnoise2(wp * 2.3, b.seed + 25u));
    // A finer, fainter network of fat threads inside the bundles.
    let v2 = rc_voronoi2(wp * 2.6 + vec2<f32>(7.1, 3.3), b.seed + 28u, 1.0);
    fat = max(fat, 0.3 * (1.0 - smoothstep(0.015, 0.015 + soft * 1.5, v2.z)) * smoothstep(0.55, 0.8, rc_vnoise2(wp * 4.1, b.seed + 29u)) * b.nb.y * 2.0);
    let fk = rc_voronoi2(uv * cs * vec2<f32>(0.5, 0.25), b.seed + 26u, 1.0);
    fat = max(fat, (1.0 - smoothstep(0.06, 0.06 + soft, fk.x)) * step(fk.w, 0.2 * b.nb.y));
    fat = rc_fade(fat, 0.12 * b.nb.y, max(ka, 0.35));
    let tone = 0.8 + 0.38 * v.w + 0.3 * (rc_fbm2(uv * cs * 0.05, b.seed + 27u, 3u) - 0.5);
    let mus = *c * tone * mix(vec3<f32>(0.85, 0.8, 1.0), vec3<f32>(1.0), smoothstep(0.75, 1.1, tone));
    *c = mix(mus, mix(b.col_b, b.col_a * 2.2, 0.3 * (1.0 - fat)), fat);
    *r = mix(0.24, 0.5, fat);
}

// Colour `c` and roughness `r` with flesh's grain, the kernels or the
// pastry's lattice.
fn rc_organic_f(b: RecBlock, uv: vec2<f32>, c: ptr<function, vec3<f32>>, r: ptr<function, f32>, cs: f32, ka: f32) {
    if (b.variant == 0u) {
        // Meat's grain is fine and soft (round 10 trial: at full strength
        // its grooves read as wood grain under the fat).
        let mt = rc_organic_mx(b);
        *c = *c * (1.0 - select(0.6, 0.25, mt) * b.na.z * rc_fade(rc_organic_g(b, uv, cs), 0.2, ka));
        if (mt) {
            rc_organic_meat(b, uv, c, r, cs, ka);
        }
    }
    if (b.variant == 5u) {
        // Each kernel its own tone, glossy on its crown; dark B between.
        let k = rc_organic_k(uv, cs);
        let kh = rc_fade(k.x, 0.7, ka);
        *c = mix(b.col_b * 0.25, *c * (0.85 + 0.3 * rc_u01(rc_hash2(vec2<i32>(k.yz), b.seed))), kh);
        *r = mix(0.8, *r, kh);
    }
    if (b.variant == 6u) {
        let l = rc_organic_l(uv, cs);
        let k = rc_fade(l.y, 0.7, ka);
        *c = mix(b.col_b, *c * (0.15 + 0.75 * rc_fade(l.x, 0.75, ka)), k);
        *r = mix(0.35, *r, k);
    }
}
