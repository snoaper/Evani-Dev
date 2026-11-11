// evani.shader.rc.masonry
// Masonry (recipe 7, batch 1 build plan section 3, package P5). Written fresh
// for Evani.Dev: bonds, herringbone and basket weave from the layout helper
// (rc_layout.wgsl, batch 3 B3-P1), stones from the core's cellular noise.
// World space (plan 2.3): one dominant-axis projection up to Medium, two
// blended from High, so a cube or a slope shows no seam; rows run along world
// height.
//   numbers: na = unit width (0.05-1 m), unit height (0.03-1 m), joint width
//            (0.002-0.05 m), unevenness; nb = rough, variation, joint depth,
//            fleck amount; nc.x = joint cover (0 default: thin joints fade to
//            their average; 1: each pixel takes the share the joint covers)
//   colours: A unit, B mortar, C unit tint (and the paint, and the flecks)
//   variants: 0 running bond, 1 stack, 2 cobble, 3 setts, 4 ashlar,
//             5 flagstone, 6 herringbone, 7 basket weave (B3-P1: units unit
//             height wide, round(unit width / unit height) widths long),
//             8 rubble (irregular stones in wide mortar), 9 drystone (courses
//             of flat stones, dark gaps: set the mortar colour dark), 10
//             shingle (slates in courses, each over the one below, the lap
//             shadowed)
//   extras: parallax (bit 0), unit variation (bit 1), painted (bit 2: one
//           coat of colour C over units and mortar, the bumps kept), glaze
//           (bit 3: glossy units), flecks (bit 4: straw or fossil specks in C
//           on the units, `fleck` of them)
// Levels (q, the detail group): lite draws what low draws
// (every tier keeps the pattern; before critic round 1 lite was the flat pattern (unit, tint by unit,
// joints)); low adds the unit variation, surface noise and wobble, anti-aliased
// (joints under 1.5 px fade to their average); medium adds joint bevels and
// domed stones as normals within 25 m; high adds the blended projection and
// 2-step parallax within 8 m; ultra parallaxes everywhere.

// The distance to the nearest unit edge (x, metres), the unit's id (y) and,
// for slates, how far under the next course the point lies (z, 0 to 1), at
// plane point p0.
fn rc_masonry_u(b: RecBlock, p0: vec2<f32>) -> vec3<f32> {
    let uw = rc_range(b.na.x, 0.05, 1.0);
    let uh = rc_range(b.na.y, 0.03, 1.0);
    let v = b.variant;
    let wb = vec2<f32>(rc_vnoise2(p0 * 6.0, b.seed + 2u), rc_vnoise2(p0 * 6.0 + vec2<f32>(9.1, 3.7), b.seed + 3u)) - vec2<f32>(0.5);
    let p = p0 + wb * (0.3 * b.na.w * min(uh, uw));
    if (v == 2u || v == 5u || v == 8u) {
        // Cobbles, flagstones and rubble: stones are the cells, the joint the
        // bisector (rubble's cells are the most ragged).
        let cs = select(uw, uw * 3.0, v == 5u);
        let c = rc_voronoi2(p / cs, b.seed, select(select(0.85, 1.0, v == 8u), 0.7, v == 5u));
        return vec3<f32>(c.z * 0.5 * cs, c.w, 0.0);
    }
    if (v == 10u) {
        let s = rc_lay_shingle(p, vec2<f32>(uw, uh), 0.15, b.seed + 1u);
        return vec3<f32>(s.e, s.id, s.uv.y);
    }
    var c: LayCell;
    if (v == 6u) {
        // Herringbone: units uh wide and a whole number of widths long (at
        // least 2), zigzag rows along the wall or path.
        c = rc_lay_herring(rc_lay_rot45(p), uh, max(round(uw / uh), 2.0), b.seed + 1u);
    } else if (v == 7u) {
        // Basket weave: square blocks of that many units, turned in turn.
        c = rc_lay_basket(p, uh, max(round(uw / uh), 2.0), b.seed + 1u);
    } else {
        // The bonds (running, stack, setts, ashlar): the layout helper's,
        // moved there unchanged from here (B3-P1).
        c = rc_lay_bond(p, vec2<f32>(uw, uh), select(v, 4u, v == 9u), b.seed);
    }
    return vec3<f32>(c.e, c.id, 0.0);
}

// The gap between units (x, metres; cobbles keep a deep one) and the width
// of the bevel or dome that falls into it (y).
fn rc_masonry_g(b: RecBlock) -> vec2<f32> {
    let uw = rc_range(b.na.x, 0.05, 1.0);
    let jw = rc_range(b.na.z, 0.002, 0.05);
    let v = b.variant;
    let gap = select(jw, max(jw, uw * 0.08), v == 2u);
    return vec2<f32>(gap, select(gap * 1.5, uw * select(select(0.35, 0.15, v == 8u), 0.2, v == 3u), v == 2u || v == 3u || v == 8u));
}

// The unit height at p: 0 in a joint, 1 on a unit's face.
fn rc_masonry_h(b: RecBlock, p: vec2<f32>) -> f32 {
    let g = rc_masonry_g(b);
    let u = rc_masonry_u(b, p);
    // A slate stands out at its free edge and lies under the next course.
    return smoothstep(g.x * 0.5, g.x * 0.5 + g.y, u.x) * (1.0 - 0.5 * u.z);
}

// The colour at p (w the joint mask); kj and kn how much joint and surface
// detail survive the anti-alias fade; fw the metres a pixel covers.
fn rc_masonry_c(b: RecBlock, p: vec2<f32>, kj: f32, kn: f32, fw: f32) -> vec4<f32> {
    let g = rc_masonry_g(b);
    let u = rc_masonry_u(b, p);
    let avg = min(1.0, g.x * (1.0 / rc_range(b.na.x, 0.05, 1.0) + 1.0 / rc_range(b.na.y, 0.03, 1.0)));
    var j = rc_fade(1.0 - smoothstep(g.x * 0.4, g.x * 0.6, u.x), avg, kj);
    if (b.nc.x > 0.0) {
        // Joint cover (number 8, off by default; Asset Studio column 37): the
        // share of the pixel the joint covers, so a joint under a pixel wide
        // still draws as a faint line (as in a photograph) instead of fading
        // into the units; to the average once a course is under 3 pixels.
        let hw = g.x * 0.5;
        let cov = clamp((min(u.x + fw * 0.5, hw) - max(u.x - fw * 0.5, -hw)) / max(fw, 1e-6), 0.0, 1.0);
        let kc = smoothstep(1.5, 3.0, rc_range(b.na.y, 0.03, 1.0) / max(fw, 1e-6));
        j = mix(j, mix(avg, cov, kc), b.nc.x);
    }
    let vr = select(0.0, b.nb.y, rc_extra(b, RC_X_MASONRY, 1u));
    var c = mix(b.col_a, b.col_c, clamp(fract(u.y * 13.7) * vr * 1.6, 0.0, 1.0)) * (1.0 + (fract(u.y * 5.1) - 0.5) * vr * 0.5);
    c = c * (0.85 + 0.3 * rc_fade(rc_fbm2(p * 24.0, b.seed + 5u, 2u), 0.5, kn));
    if (rc_extra(b, RC_X_MASONRY, 4u)) {
        // Flecks, short and wide: straw in adobe, fossils in limestone.
        let fv = rc_voronoi2(p * vec2<f32>(26.0, 70.0), b.seed + 7u, 1.0);
        c = mix(c, b.col_c, step(fv.w, b.nb.w) * (1.0 - smoothstep(0.1, 0.18, fv.x)) * kn);
    }
    var m = b.col_b * (0.85 + 0.3 * rc_fade(rc_vnoise2(p * 70.0, b.seed + 6u), 0.5, kn));
    if (rc_extra(b, RC_X_MASONRY, 2u)) {
        // One coat of paint over both; the joints keep a shade darker.
        c = mix(c, b.col_c * (0.93 + 0.14 * rc_fade(rc_vnoise2(p * 40.0, b.seed + 8u), 0.5, kn)), 0.92);
        m = mix(m, b.col_c * 0.8, 0.88);
    }
    var col = mix(c, m, j);
    if (b.variant == 10u) {
        col = col * (1.0 - 0.4 * smoothstep(0.6, 1.0, u.z));
    }
    return vec4<f32>(col, j);
}

fn rc_masonry(b: RecBlock, f: MatFrame, i: MatIn, q: u32) -> MatOut {
    var w = i.world;
    let ng = normalize(i.g);
    let kj = rc_aa_world(f, rc_range(b.na.z, 0.002, 0.05));
    let kn = rc_aa_world(f, 0.04);
    let jd = 0.02 * b.nb.z;
    if (q >= 3u && rc_extra(b, RC_X_MASONRY, 0u) && (q == 4u || f.dist < 8.0)) {
        // Parallax, two steps: the view slides into the joints.
        let dn = dot(f.v, ng);
        let ofs = (f.v - ng * dn) / max(dn, 0.3) * (jd * kj);
        let h1 = rc_masonry_h(b, rc_proj(w, ng));
        let h2 = rc_masonry_h(b, rc_proj(w - ofs * (1.0 - h1), ng));
        w = w - ofs * (1.0 - h2);
    }
    var s: vec4<f32>;
    // The pixel's shorter step on the surface: a wall seen at a slant keeps
    // its bed joints (joint cover only).
    let fw = min(length(f.dpx), length(f.dpy));
    if (q >= 3u) {
        let pp = rc_proj2(w, ng);
        s = mix(rc_masonry_c(b, pp.a, kj, kn, fw), rc_masonry_c(b, pp.b, kj, kn, fw), pp.w);
    } else {
        s = rc_masonry_c(b, rc_proj(w, ng), kj, kn, fw);
    }
    var o = rc_out(i, s.rgb, select(b.nb.x + 0.1 * s.w, mix(0.12, b.nb.x + 0.1, s.w), rc_extra(b, RC_X_MASONRY, 3u)), 0.0);
    o.occ = i.occ * (1.0 - 0.5 * s.w * b.nb.z);
    if (q >= 2u) {
        let near = rc_near(f) * kj;
        if (near > 0.0) {
            // Bevels: each unit's face falls away into its joint; cobbles and
            // setts are domed, so they tilt more.
            let t = rc_tw(ng);
            let g = rc_masonry_g(b);
            let e = max(g.y * 0.5, 0.0007);
            let h = rc_masonry_h(b, rc_proj(w, ng));
            let d = vec2<f32>(rc_masonry_h(b, rc_proj(w + t[0] * e, ng)), rc_masonry_h(b, rc_proj(w + t[1] * e, ng))) - vec2<f32>(h);
            o.n = rc_bump(i.n, t, d * ((0.015 + 0.03 * f32(b.variant == 2u || b.variant == 3u || b.variant == 10u)) * b.nb.z * near / e));
        }
    }
    return o;
}
