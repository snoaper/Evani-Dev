// Wood: the layouts part.
// Wood's batch 3 layouts (B3-P5), its own part (feature `recipe-wood-layouts`,
// appended after shaders/rc_wood.wgsl), so a game whose wood is only grain
// and planks never downloads them. Written fresh for Evani.Dev.
// shaders/rc_wood_planks.wgsl is the stand-in with the same three functions
// when the part is off.

// The layout of variants 3 to 10 and 13 at q (uv, turned for the vertical ones):
// x along the unit's grain, y across it (centred), z its edge distance (a
// panel's height instead), w its own number.
fn rc_wood_u(b: RecBlock, q: vec2<f32>, pw: f32) -> vec4<f32> {
    let v = b.variant;
    if (v == 7u) {
        let c = rc_lay_shingle(q, vec2<f32>(pw, pw * 1.1), 0.25, b.seed + 1u);
        return vec4<f32>(c.uv.y * pw * 1.1, (c.uv.x - 0.5) * pw, c.e, c.id);
    }
    if (v == 8u) {
        let c = rc_lay_herring(rc_lay_rot45(q), pw, 4.0, b.seed + 1u);
        return vec4<f32>(c.uv.x * pw * 4.0, (c.uv.y - 0.5) * pw, c.e, c.id);
    }
    if (v == 13u) {
        // Charred (P8-4): blocks of char 2 plank widths long, each row
        // offset at random, the cracks wandering (a diagonal warp).
        let c = rc_lay_bond(q + vec2<f32>(rc_vnoise2(q * 2.0 / pw, b.seed + 17u) * 0.25 * pw), vec2<f32>(pw * 2.0, pw), 3u, b.seed + 1u);
        return vec4<f32>(c.uv.x * pw * 2.0, (c.uv.y - 0.5) * pw, c.e, c.id);
    }
    if (v == 9u) {
        let t = vec2<f32>(pw * 2.0, pw * 3.0);
        let cell = floor(q / t);
        let l = q - cell * t;
        return vec4<f32>(l.x, l.y - t.y * 0.5, rc_lay_panel(l, t, pw * 0.3, pw * 0.2), rc_u01(rc_hash2(vec2<i32>(cell), b.seed)));
    }
    var u = rc_wood_plank(b, q, pw, select(select(40.0, 14.0, v == 4u), 7.0, v == 3u));
    if (v == 5u) {
        // The battens sit over the joints between boards, so their sides are
        // the lines.
        u.z = abs(min(u.y + pw * 0.5, pw * 0.5 - u.y) - pw * 0.12);
    }
    return u;
}

// What a unit looks like: x its shade (lap shadows, round logs and poles,
// panel fields), y its height (1 on the face, 0 in a joint).
fn rc_wood_form(b: RecBlock, u: vec4<f32>, pw: f32) -> vec2<f32> {
    let v = b.variant;
    let t = u.y / pw + 0.5;
    var h = smoothstep(0.0, pw * 0.06, u.z);
    var s = 1.0;
    if (v == 4u) {
        s = mix(0.6, 1.0, smoothstep(0.0, 0.18, t));
    } else if (v == 5u) {
        s = 1.0 + 0.1 * (1.0 - smoothstep(0.1, 0.14, min(t, 1.0 - t)));
    } else if (v == 6u || v == 10u) {
        h = sqrt(max(1.0 - (2.0 * t - 1.0) * (2.0 * t - 1.0), 0.0));
        s = 0.45 + 0.55 * h;
        if (v == 10u) {
            s = s * (1.0 - 0.5 * (1.0 - smoothstep(0.0, pw * 0.07, rc_lay_stripe(u.x + u.w * pw * 6.0, pw * 6.0))));
        }
    } else if (v == 7u) {
        let lap = u.x / (pw * 1.1);
        h = h * (1.0 - 0.5 * lap);
        s = 1.0 - 0.45 * smoothstep(0.6, 1.0, lap);
    } else if (v == 13u) {
        // A char block rises from its cracks to a low, flat crown.
        h = smoothstep(0.0, pw * 0.15, u.z);
    } else if (v == 9u) {
        h = u.z;
        s = mix(0.88, 1.0, h) * (1.0 - 0.35 * smoothstep(0.0, 0.15, h) * (1.0 - smoothstep(0.15, 0.5, h)));
    }
    return vec2<f32>(s, h);
}

// Marquetry (variant 14, P8-4): tiles plank width across, each with the
// layout motif cut in veneer: the ground (piece 0, grain along u), the petals
// (piece 1, grain across) and the centre and corner circles (piece 2, grain
// at 45 degrees). x, y the grain coordinate in the piece, z the piece, w the
// dark line where pieces meet.
fn rc_wood_mq(b: RecBlock, uv: vec2<f32>, pw: f32) -> vec4<f32> {
    let m = rc_lay_motif(fract(uv / pw) * 2.0 - vec2<f32>(1.0), b.seed, 0.0, 0.0);
    let k = select(select(0.0, 1.0, m.x < 0.0), 2.0, min(m.y, m.z) < 0.0);
    let g = select(select(uv, uv.yx, k == 1.0), rc_lay_rot45(uv), k == 2.0);
    return vec4<f32>(g + vec2<f32>(k * 3.7 - 0.5, -0.5), k, 1.0 - smoothstep(0.02, 0.05, min(abs(m.x), min(abs(m.y), abs(m.z)))));
}

// Cork: grains of colours A and B, darker where they meet.
fn rc_wood_cork(b: RecBlock, f: MatFrame, i: MatIn, q: u32) -> MatOut {
    let gs = rc_range(b.na.y, 1.0, 100.0) * 1.5;
    let cv = rc_voronoi2(i.uv * gs, b.seed + 3u, 1.0);
    let ka = rc_aa(f, 0.7 / gs);
    var c = mix(b.col_a, b.col_b, mix(0.3, rc_fade(cv.w, 0.5, ka), b.na.w));
    if (q > 0u) {
        c = c * (0.65 + 0.35 * rc_fade(smoothstep(0.0, 0.3, cv.z), 0.8, ka));
    }
    var o = rc_out(i, c, b.na.x, 0.0);
    if (q >= 2u && rc_near(f) > 0.0) {
        let e = 0.5 / gs;
        let h = smoothstep(0.0, 0.3, cv.z);
        let d = vec2<f32>(smoothstep(0.0, 0.3, rc_voronoi2((i.uv + vec2<f32>(e, 0.0)) * gs, b.seed + 3u, 1.0).z), smoothstep(0.0, 0.3, rc_voronoi2((i.uv + vec2<f32>(0.0, e)) * gs, b.seed + 3u, 1.0).z)) - vec2<f32>(h);
        o.n = rc_bump(i.n, rc_tuv(f, i.n), d * (0.5 * rc_near(f) * ka));
    }
    return o;
}

