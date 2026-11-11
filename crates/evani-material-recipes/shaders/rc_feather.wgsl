// evani.shader.rc.feather
// Feather (recipe 13, batch 1 build plan section 3, package P7). Written
// fresh for Evani.Dev: plumage as rows of feathers, each a hex-lattice cell
// (alternate rows offset, stretched along v, each lying over the next row) with a shaft down its middle,
// barb stripes slanting away from the shaft towards the tip, barring, spots or
// an eye, a darker overlap edge and an iridescent hue by view angle. Uv space:
// the tip of a feather faces +v.
//   numbers: na = feather size (0.005-0.3 uv), barring, barb scale
//            (10-400 per uv), iridescence; nb = rough, edge relief
//   colours: A colour, B barring (spots, lacing, the eye's core), C iridescent
//            hue (the eye's middle ring)
//   variants: 0 plain (barring laces the tip edge), 1 barred, 2 speckled,
//             3 eyed; P7-2 (batches 7 and 8): 4 plume (loose wispy barbs
//             twice as long, dark gaps between them, B at the tips and a rim
//             of light through them), 5 scalloped (a crescent mark in B
//             inside each tip, the rim itself in C: pale-edged goose, black-
//             marked pheasant), 6 quill (one large feather over each uv
//             square, a card's way: u across, the tip at v 0, the base at
//             v 1; a tapering shaft in C, vanes of barbs, a few split apart, down
//             at the base; feather size is not read)
//   extras: iridescence (bit 0), edge relief (bit 1)
// Levels (q, the detail group): lite draws what low draws
// (every tier keeps the pattern; before critic round 1 lite was the flat barred colour); low adds the
// feathers, shaft and barbs, anti-aliased (cells and barbs under 1.5 px fade
// to their average); medium adds the iridescence with its sheen and the
// shaft, dome and barb normals within 25 m; high adds a finer second barb
// layer and a view-angle glint; ultra draws as high.

// The feather size in uv: a whole number round u, so there is no seam.
fn rc_feather_fs(b: RecBlock) -> f32 {
    return 1.0 / max(round(1.0 / rc_range(b.na.x, 0.005, 0.3)), 1.0);
}

// The feather at uv: x across (-1 to 1), y along (-1 base, 1 tip), z per
// feather random, w outline radius (0 centre, 1 edge). Each feather is an
// ellipse; the two one row up (towards the head) lie over this one where they
// reach it, so only rounded tips show.
fn rc_feather_c(b: RecBlock, uv: vec2<f32>) -> vec4<f32> {
    if (b.variant == 6u) {
        let p = fract(uv) * 2.0 - 1.0;
        return vec4<f32>(p.x, -p.y, 0.5, abs(p.x));
    }
    let fs = rc_feather_fs(b);
    let h = rc_hex(vec2<f32>(uv.x / fs, uv.y / (fs * select(1.5, 3.0, b.variant == 4u))));
    let k = vec2<f32>(1.667, 0.77);
    var d = h.xy;
    var id = h.zw;
    let da = d + vec2<f32>(-0.5, 0.866);
    let db = d + vec2<f32>(0.5, 0.866);
    let la = length(da * k);
    let lb = length(db * k);
    if (min(la, lb) < 1.0) {
        d = select(db, da, la < lb);
        id = id + select(vec2<f32>(-0.5, -0.866), vec2<f32>(0.5, -0.866), la < lb);
    }
    d = d * k;
    return vec4<f32>(d.x, d.y, rc_u01(rc_hash2(vec2<i32>(round(id * 2.0)), b.seed)), length(d));
}

// The feather's height: a dome, a lip at the tip, the raised shaft.
fn rc_feather_h(c: vec4<f32>) -> f32 {
    return 0.6 * (1.0 - min(c.w, 1.0) * min(c.w, 1.0)) + 0.2 * (0.5 + 0.5 * c.y) + 0.15 * (1.0 - smoothstep(0.02, 0.08, abs(c.x)));
}

fn rc_feather(b: RecBlock, f: MatFrame, i: MatIn, q: u32) -> MatOut {
    let v = b.variant;
    let qv = v == 6u;
    let fs = select(rc_feather_fs(b), 1.0, qv);
    let bs = rc_range(b.na.z, 10.0, 400.0);
    let ka = rc_aa(f, fs * select(1.0, 0.03, qv));
    let kb = rc_aa(f, 1.0 / bs);
    let c = rc_feather_c(b, i.uv);
    // Wisp: plumes, and a quill's down below its vanes; their barbs wave.
    let wp = select(0.0, 1.0, v == 4u) + select(0.0, 1.0 - smoothstep(-0.8, -0.5, c.y), qv);
    // The barb phase (one per barb, lines slanting out from the shaft).
    let t = (c.y - abs(c.x)) * fs * bs * 0.35 + c.z * 7.0 + wp * (rc_vnoise2(c.xy * vec2<f32>(2.0, 4.0) + vec2<f32>(c.z * 9.0), b.seed) - 0.5);
    var barb = 0.5 + 0.5 * sin(6.2831853 * t);
    if (q >= 3u) {
        barb = barb * 0.7 + 0.3 * (0.5 + 0.5 * sin(6.2831853 * t * 3.3));
    }
    // A plume's barbs stand apart: the feathers under them show dark between.
    let gap = rc_fade(1.0 - smoothstep(0.3, 0.8, barb), 0.45, kb) * wp;
    barb = rc_fade(barb, 0.5, kb);
    let shaft = 1.0 - smoothstep(0.02, 0.07, abs(c.x) / select(1.0, 1.07 - 0.72 * c.y, qv));
    let edge = rc_fade(smoothstep(0.86, 1.0, c.w), 0.1, ka) * select(1.0, 0.3, v >= 4u);
    var col = b.col_a * rc_fade(0.9 + 0.2 * c.z, 1.0, ka);
    if (v == 0u) {
        col = mix(col, b.col_b, rc_fade(smoothstep(0.78, 0.95, c.w) * smoothstep(-0.1, 0.4, c.y), 0.1, ka) * b.na.y);
    } else if (v == 1u) {
        col = mix(col, b.col_b, rc_fade(smoothstep(0.35, 0.55, sin((c.y + 0.4 * c.x * c.x) * 9.0 + c.z * 3.0)), 0.5, ka) * b.na.y);
    } else if (v == 2u) {
        let s = rc_voronoi2(c.xy * 3.0 + vec2<f32>(c.z * 20.0), b.seed + 1u, 1.0);
        col = mix(col, b.col_b, rc_fade((1.0 - smoothstep(0.12, 0.2, s.x)) * step(s.w, 0.55), 0.1, ka) * b.na.y);
    } else if (v == 4u) {
        col = mix(col, b.col_b, smoothstep(0.3, 1.0, c.y) * b.na.y);
    } else if (v == 5u) {
        let tip = smoothstep(-0.3, 0.3, c.y);
        col = mix(col, b.col_b, rc_fade(smoothstep(0.5, 0.6, c.w) * (1.0 - smoothstep(0.76, 0.84, c.w)) * tip, 0.1, ka) * b.na.y);
        col = mix(col, b.col_c, rc_fade(smoothstep(0.8, 0.9, c.w) * tip, 0.1, ka));
    } else if (qv) {
        // A few barbs split apart, leaving a gap through the vane.
        let s = rc_u01(rc_hash2(vec2<i32>(i32(floor(t * 0.2)), i32(sign(c.x))), b.seed));
        col = col * (1.0 - 0.55 * rc_fade(step(s, 0.25) * smoothstep(0.36, 0.48, abs(fract(t * 0.2) - 0.5)), 0.0, kb));
    } else {
        let er = length(vec2<f32>(c.x, (c.y - 0.3) * 0.8));
        col = mix(col, col * 1.35 + vec3<f32>(0.03), rc_fade(1.0 - smoothstep(0.5, 0.56, er), 0.0, ka));
        col = mix(col, b.col_c, rc_fade(1.0 - smoothstep(0.32, 0.37, er), 0.0, ka));
        col = mix(col, b.col_b, rc_fade(1.0 - smoothstep(0.13, 0.18, er), 0.0, ka));
    }
    col = col * (0.88 + 0.24 * barb) * (1.0 - 0.4 * edge) * (1.0 - 0.7 * gap);
    col = mix(col, select(col * 1.5 + vec3<f32>(0.04), b.col_c, qv), shaft * 0.6 * smoothstep(0.95, 0.7, c.y));
    var o = rc_out(i, col, b.nb.x, 0.0);
    o.occ = i.occ * (1.0 - 0.3 * edge);
    if (q >= 2u) {
        let ndv = clamp(dot(i.n, f.v), 0.0, 1.0);
        // Light through a plume's loose tips: a soft rim.
        o.sheen = vec4<f32>(b.col_b * (wp * 0.35 * smoothstep(-0.2, 0.9, c.y)), 2.0);
        let irid = b.na.w * select(0.0, 1.0, rc_extra(b, RC_X_FEATHER, 0u));
        if (irid > 0.0) {
            // A hue that turns with the view, strongest at grazing angles.
            let hue = vec3<f32>(0.5) + 0.5 * cos(6.2831853 * (vec3<f32>(0.0, 0.33, 0.67) + vec3<f32>(ndv * 0.9 + barb * 0.2)));
            let tint = mix(b.col_c, hue, 0.45);
            o.base = vec4<f32>(mix(o.base.rgb, tint * (0.3 + 0.7 * barb), irid * 0.5 * (0.3 + 0.7 * (1.0 - ndv))), o.base.a);
            o.sheen = vec4<f32>(tint * (irid * 0.45 * (0.4 + 0.6 * (1.0 - ndv))), 2.5);
            if (q >= 3u) {
                o.glint = tint * (irid * 0.4 * kb * (1.0 - smoothstep(0.0, 0.12, abs(ndv - (0.35 + 0.5 * barb)))));
            }
        }
        let near = rc_near(f) * ka * b.nb.y * select(0.0, 1.0, rc_extra(b, RC_X_FEATHER, 1u));
        if (near > 0.0) {
            let e = select(0.1 * fs, 0.01, qv);
            let h = rc_feather_h(c);
            var g = vec2<f32>(rc_feather_h(rc_feather_c(b, i.uv + vec2<f32>(e, 0.0))), rc_feather_h(rc_feather_c(b, i.uv + vec2<f32>(0.0, e * 1.5)))) - vec2<f32>(h);
            g = g * 1.4 + vec2<f32>(-0.8 * sign(c.x), 0.7) * (0.12 * kb * cos(6.2831853 * t));
            o.n = rc_bump(i.n, rc_tuv(f, i.n), g * near);
        }
    }
    return o;
}
