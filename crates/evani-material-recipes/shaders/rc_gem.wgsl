// evani.shader.rc.gem
// Gem (recipe 25, batch 1 build plan section 3, package P8). Written fresh for
// Evani.Dev: a cheap fake of a cut stone, no refraction (the nature pass's
// glass is the real see-through). The surface normal is split into facets by
// a cellular noise over the normal itself (plus a little uv, so a flat face
// still splits): each facet gets its own tilt, its own brightness and its own
// slot on a rainbow ramp that turns with the view (dispersion). The one
// environment read is the lit stage's own, through the faceted normal and a low
// roughness; a Fresnel rim, a sun-lit sparkle per facet and an emissive core
// are added after lighting.
//   numbers: na = dispersion, sparkle, facet scale (1-30 per unit of normal),
//            inclusions; nb = clarity, core glow
//   colours: A colour, B inclusion (and the milk of low clarity), C core glow
//   variants: 0 brilliant (a crush of facets), 1 step (square rings), 2 cabochon,
//             3 raw crystal (tall facets), 4 amber (bubbles)
//   extras: sparkle (bit 0), inner glow (bit 1), clear (bit 2, material-view
//           round 4: a see-through body, crate::blend draws it over what is
//           behind; the body's alpha falls with clarity and rises where the
//           view grazes, at the seams between facets and in clouds, while
//           reflections, rim and sparkle stay whole)
// Levels (q, the glass group): lite draws what low draws
// (every tier keeps the pattern; before critic round 1 lite was a tinted gloss with one sparkle band);
// low adds the facets' brightness, seams, inclusions and the glow, fading to
// their average once a facet is under 1.5 px, and the facet normals within
// 25 m, the dispersion tint and the Fresnel rim (they cost no extra noise;
// critic round 3 of material-view: Low drew an opaque white ball where Ultra
// showed a cut stone); high adds the per-facet sparkle and its rainbow
// flash; ultra draws as high.

// The facet cell at this pixel: x F1, y F2, z F2-F1, w its id. The smooth
// variants (cabochon, amber) are one cell.
fn rc_gem_c(b: RecBlock, i: MatIn, s: f32) -> vec4<f32> {
    let v = b.variant;
    if (v == 2u || v == 4u) {
        return vec4<f32>(0.5, 1.0, 1.0, 0.5);
    }
    var p = (i.n.xy + vec2<f32>(0.35 * i.n.z)) * s + i.uv * (0.3 * s);
    if (v == 1u) {
        // Step cut: square rings of flat steps round the table.
        let q = max(abs(p.x), abs(p.y));
        let e = fract(q);
        return vec4<f32>(0.5, 1.0, min(e, 1.0 - e), rc_u01(rc_hash2(vec2<i32>(i32(floor(q)), 7), b.seed)));
    }
    if (v == 3u) {
        p.y = p.y * 0.45;
    }
    return rc_voronoi2(p, b.seed, select(0.85, 0.5, v == 3u));
}

fn rc_gem(b: RecBlock, f: MatFrame, i: MatIn, q: u32) -> MatOut {
    let v = b.variant;
    let s = rc_range(b.na.z, 1.0, 30.0);
    let milk = 1.0 - b.nb.x;
    let rough = mix(0.06, 0.5, milk);
    let spark = b.na.y * select(0.0, 1.0, rc_extra(b, RC_X_GEM, 0u));
    let n0 = clamp(dot(i.n, f.v), 0.0, 1.0);
    let c = rc_gem_c(b, i, s);
    let ka = smoothstep(0.75, 1.5, 1.0 / max((abs(f.curv) * f.world_px + 0.3 * f.uv_px) * s, 1e-5));
    let r = fract(vec3<f32>(c.w * 37.17, c.w * 91.31, c.w * 143.57));
    var tl = 0.45;
    if (v == 1u) {
        tl = 0.2;
    } else if (v == 2u) {
        tl = 0.0;
    } else if (v == 3u) {
        tl = 0.5;
    } else if (v == 4u) {
        tl = 0.05;
    }
    let ft = normalize(i.n + (r - vec3<f32>(0.5)) * (2.0 * tl));
    let ndv = clamp(dot(ft, f.v), 0.0, 1.0);
    let fl = rc_fade(0.35 + 1.1 * r.x, 0.9, ka);
    var col = b.col_a * fl * (0.55 + 0.45 * ndv);
    col = col * (1.0 - 0.35 * rc_fade(1.0 - smoothstep(0.0, 0.05, c.z), 0.0, ka));
    // Inclusions: cloud in a stone, bubbles in amber.
    var inc = smoothstep(0.55, 0.8, rc_vnoise2(i.n.xy * (s * 0.6) + i.uv * (s * 0.3), b.seed + 1u)) * b.na.w;
    if (v == 4u) {
        let bb = rc_voronoi2(i.uv * 24.0, b.seed + 2u, 1.0);
        inc = (1.0 - smoothstep(0.08, 0.16, bb.x)) * step(bb.w, b.na.w * 3.0);
    }
    col = mix(col, b.col_b * (0.6 + 0.6 * fl), inc * 0.75);
    col = mix(col, b.col_b * 0.9, milk * 0.6);
    var hue = vec3<f32>(1.0);
    if (q >= 1u) {
        // A rainbow that turns with the view and differs facet to facet.
        hue = vec3<f32>(0.5) + 0.5 * cos(6.2831853 * (vec3<f32>(0.0, 0.33, 0.67) + vec3<f32>(ndv * 1.2 + r.y * 0.6)));
        col = mix(col, col * (0.3 + 1.4 * hue), b.na.x * 0.25 * ka);
    }
    var o = rc_out(i, col, rough, 0.0);
    if (rc_extra(b, RC_X_GEM, 2u)) {
        let fr = 1.0 - ndv;
        o.base.a = o.base.a * clamp(mix(1.0, 0.16, b.nb.x) + 0.7 * fr * fr * fr + 0.5 * inc + 0.3 * rc_fade(1.0 - smoothstep(0.0, 0.05, c.z), 0.0, ka), 0.0, 1.0);
    }
    if (rc_extra(b, RC_X_GEM, 1u)) {
        o.emit = b.col_c * (b.nb.y * 2.5 * n0 * n0 * n0 * (0.7 + 0.6 * r.x));
    }
    if (q >= 1u) {
        o.sheen = vec4<f32>(mix(b.col_a, hue, b.na.x) * 0.5, 3.0);
        o.n = normalize(mix(i.n, ft, rc_near(f) * ka));
    }
    if (q >= 3u) {
        // One facet at a time catches the sun, with a flash of the rainbow.
        o.glint = mix(vec3<f32>(1.0), hue, b.na.x) * (spark * 1.4 * ka * (1.0 - smoothstep(0.0, 0.055, abs(ndv - (0.15 + 0.75 * fract(r.z * 7.7))))));
    }
    return o;
}
