// evani.shader.rc.keratin
// Keratin (recipe 19, batch 1 build plan section 3, package P6; batch 5 B5-P2
// adds hoof, scutes, banded and the stripes). Written fresh for Evani.Dev:
// horn, bone, antler, teeth, shell and beetle casing.
// Growth rings are a sawtooth across v (a sharp edge, then a slow fade),
// wobbled by noise; bone and antler add long fibres and grooves along v;
// edge translucency is a warm rim; nacre a hue by view angle (thin film).
// Hoof: fine tubules running along v, a pale sole edge at the end of v.
// Scutes: hexagonal plates, each with growth rings round a point between its
// middle and one corner, a dark seam where two meet. Banded: rows of small
// plates across v, staggered, with soft joints between the rows (ring scale
// sets the rows: 1/4 of it per uv). Stripes: `bands` dark and pale stripes per
// unit of u (rounded to a whole number, so they close round a model).
// P7-2 (batch 7): spiral, a log spiral round the middle of each uv square
// (snail): whorls growing outwards, each a dome with a dark seam (rings sets
// how dark), growth lines across them, and `bands` stripes along each whorl
// in B instead of round u; spots, round spots in B on a jittered grid, each
// kept inside its own cell, so they close round a model (spot scale, made a
// whole number, cells per uv; any variant: a ladybird is beetle + spots).
// Uv space: v runs along the horn or tooth, u around it.
//   numbers: na = translucency, rings, ring scale (1-100 per uv), rough;
//            nb = nacre, bands (0-16 stripe pairs per u), spot scale (1-40)
//   colours: A colour, B ring edge (and the dark stripes, the spots)
//   variants: 0 horn, 1 bone, 2 antler, 3 tooth, 4 shell, 5 beetle, 6 hoof,
//             7 scutes, 8 banded, 9 spiral
//   extras: edge translucency (bit 0), nacre (bit 1), spots (bit 2)
// Levels (q, the detail group): lite draws what low draws
// (every tier keeps the pattern; before critic round 1 lite was a flat glossy colour); low draws the
// rings, fibres and the horn's dark tip, anti-aliased (rings under 1.5 px
// fade to their average); medium adds ring and groove normals within 25 m,
// the translucent edge and nacre; high adds fine growth striations; ultra
// draws as high.

// The keratin at uv: x ring phase (0 at a ring's edge, 1 just before the
// next), y surface height, z the plate's (or tubule's) own number. `k` 1 adds
// the fine striations.
fn rc_keratin_s(b: RecBlock, uv: vec2<f32>, k: f32) -> vec3<f32> {
    let v = b.variant;
    let rs = rc_range(b.na.z, 1.0, 100.0);
    if (v == 9u) {
        // Spiral: the whorl phase grows by one a turn, outwards.
        let d = fract(uv) - 0.5;
        let a = atan2(d.y, d.x) * 0.159155;
        let ph = log(max(length(d), 1e-3)) * rs * 0.1 - a;
        let x = fract(ph);
        return vec3<f32>(x, sin(3.14159 * x) * (0.85 + 0.15 * sin(a * rs * 12.0 + 3.0 * rc_vnoise2(vec2<f32>(a * 40.0, ph), b.seed))), x);
    }
    if (v >= 7u) {
        var x = 0.0;
        var y = 0.0;
        var id = 0u;
        if (v == 7u) {
            // Scutes: rings round a point between the plate's middle and a corner.
            let h = rc_hex(uv * rs * 0.15);
            id = rc_hash2(vec2<i32>(floor(h.zw * 2.0)), b.seed);
            let d = length(h.xy - (vec2<f32>(rc_u01(id), rc_u01(rc_pcg(id))) - 0.5) * 0.7);
            let ph = fract(d * 7.0 - 0.6 * rc_vnoise2(h.xy * 6.0 + h.zw, b.seed));
            let g = 1.0 - smoothstep(0.4, 0.5, max(abs(h.x), 0.5 * abs(h.x) + 0.866 * abs(h.y)));
            x = min(ph, g);
            y = g * (0.45 + 0.4 * ph);
        } else {
            // Banded: rows across v, plates along each row, offset row to row.
            let p = uv * rs * 0.25;
            let r = floor(p.y);
            let u = p.x * 2.0 + 0.5 * r;
            id = rc_hash2(vec2<i32>(floor(vec2<f32>(u, r))), b.seed);
            let j = smoothstep(0.0, 0.2, 0.5 - abs(fract(p.y) - 0.5));
            let g = smoothstep(0.0, 0.12, 0.5 - abs(fract(u) - 0.5));
            x = min(j, g * 2.0);
            y = j * (0.4 + 0.6 * g);
        }
        return vec3<f32>(x, y, rc_u01(id));
    }
    let w = (rc_vnoise2(vec2<f32>(uv.x * 6.0, uv.y * rs * 0.3), b.seed) - 0.5) * 1.5;
    let ph = fract(uv.y * rs + w);
    var h = ph * 0.6 * b.na.y;
    var z = 0.5;
    if (v == 1u || v == 2u) {
        // Long fibres and grooves along v; antler's are deep.
        let g = rc_vnoise2(vec2<f32>(uv.x * rs * 0.9, uv.y * rs * 0.08), b.seed + 1u);
        h = mix(h, 1.0 - abs(2.0 * g - 1.0), select(0.4, 0.8, v == 2u));
    }
    if (v == 6u) {
        // Hoof: tubules down the wall.
        z = rc_vnoise2(vec2<f32>(uv.x * rs * 3.0, uv.y * rs * 0.08), b.seed + 4u);
        h = 0.5 * z + 0.3 * h;
    }
    if (k > 0.5) {
        h = h + 0.15 * rc_vnoise2(vec2<f32>(uv.x * 60.0, uv.y * rs * 4.0), b.seed + 2u);
    }
    return vec3<f32>(select(ph, 0.5, v == 1u || v == 5u), h, z);
}

fn rc_keratin(b: RecBlock, f: MatFrame, i: MatIn, q: u32) -> MatOut {
    let v = b.variant;
    let rs = rc_range(b.na.z, 1.0, 100.0);
    let hi = select(0.0, 1.0, q >= 3u);
    let s = rc_keratin_s(b, i.uv, hi);
    // A spiral's whorls narrow towards its middle.
    let ka = rc_aa(f, select(1.0, length(fract(i.uv) - 0.5) * 10.0, v == 9u) / rs);
    // A ring: dark at its sharp edge, fading to the colour.
    let ring = rc_fade(1.0 - smoothstep(0.0, 0.3, s.x), 0.15, ka) * b.na.y;
    var c = mix(b.col_a * (0.92 + 0.16 * rc_fade(s.y, 0.5, ka)), b.col_b, ring);
    if (v == 0u) {
        // Horn darkens towards its tip.
        c = mix(c, b.col_b * 0.3, smoothstep(0.3, 0.85, i.uv.y));
    }
    if (v == 4u) {
        // Shell: mottled between growth lines.
        c = mix(c, b.col_b, rc_fade(smoothstep(0.55, 0.75, rc_vnoise2(i.uv * vec2<f32>(6.0, rs * 0.6), b.seed + 3u)), 0.2, ka) * 0.6);
    }
    if (v >= 6u) {
        // Hoof tubules, or each plate a little lighter or darker than its neighbours.
        c = c * (0.8 + 0.4 * rc_fade(s.z, 0.5, rc_aa(f, 0.3 / rs)));
    }
    if (v == 6u) {
        // Hoof: a paler, thinner sole edge.
        c = mix(c, c * 1.4, smoothstep(0.88, 1.0, i.uv.y));
    }
    let nd = round(rc_range(b.nb.y, 0.0, 16.0));
    if (nd > 0.0) {
        // Stripes round the model (a spiral's along its whorls).
        c = mix(c, b.col_b, rc_fade(smoothstep(0.2, 0.3, abs(fract(select(i.uv.x, s.x, v == 9u) * nd) - 0.5)), 0.5, select(rc_aa(f, 0.2 / nd), ka, v == 9u)));
    }
    if (v == 9u) {
        // The seam between whorls.
        c = c * (1.0 - b.na.y * rc_fade(1.0 - smoothstep(0.0, 0.25, s.y), 0.15, ka));
    }
    if (rc_extra(b, RC_X_KERATIN, 2u)) {
        let p = i.uv * round(rc_range(b.nb.z, 1.0, 40.0));
        let h = rc_hash2(vec2<i32>(floor(p)), b.seed + 5u);
        let d = fract(p) - 0.5 - (vec2<f32>(rc_u01(h), rc_u01(rc_pcg(h))) - 0.5) * 0.36;
        c = mix(c, b.col_b, rc_fade((1.0 - smoothstep(0.24, 0.28, length(d))) * step(rc_u01(rc_pcg(h + 1u)), 0.7), 0.13, rc_aa(f, 0.3 / round(rc_range(b.nb.z, 1.0, 40.0)))));
    }
    var o = rc_out(i, c, b.na.w, 0.0);
    if (q >= 2u) {
        let ndv = clamp(dot(i.n, f.v), 0.0, 1.0);
        var rim = vec3<f32>(0.0);
        if (rc_extra(b, RC_X_KERATIN, 0u)) {
            // Light through thin edges: a warm rim.
            rim = mix(c, vec3<f32>(1.0, 0.85, 0.7), 0.5) * b.na.x;
        }
        if (rc_extra(b, RC_X_KERATIN, 1u) && b.nb.x > 0.0) {
            // Nacre: a hue that turns with the view, strongest at grazing.
            let hue = vec3<f32>(0.5) + 0.5 * cos(6.2831853 * (vec3<f32>(0.0, 0.33, 0.67) + vec3<f32>(ndv * 1.2)));
            o.base = vec4<f32>(mix(o.base.rgb, hue * (0.1 + 0.4 * (1.0 - ndv)), b.nb.x * 0.45 * (1.0 - ndv)), o.base.a);
            rim = rim + hue * (b.nb.x * 0.3);
        }
        o.sheen = vec4<f32>(rim, 2.0);
        let near = rc_near(f) * ka;
        if (near > 0.0) {
            let e = 0.1 / rs;
            let g = vec2<f32>(rc_keratin_s(b, i.uv + vec2<f32>(0.0, e), hi).y, rc_keratin_s(b, i.uv + vec2<f32>(e * 4.0, 0.0), hi).y) - vec2<f32>(s.y);
            o.n = rc_bump(i.n, rc_tuv(f, i.n), vec2<f32>(g.y, g.x) * (1.2 * near));
        }
    }
    return o;
}
