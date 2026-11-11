// evani.shader.rc.fire
// Fire (recipe 28, batch 1 build plan section 3, package P9). Written fresh
// for Evani.Dev: flame, embers, lava, smoke and steam from scrolling noise,
// for a card, a lava ball or any uv surface (v runs down the page, so the
// tip of a flame is at v = 0). The surface is emissive (`o.emit`); a model
// whose material blends also gets the soft edge as alpha. All motion reads
// rc_time() (sim time from the fixed step), never a clock, so two runs of the
// same sim frames give the same picture.
//   numbers: na = intensity (0-4), flicker speed (0-20), flicker amount,
//            softness; nb = scroll speed (0-5), embers
//   colours: A core, B edge, C smoke
//   variants: 0 flame (blue base), 1 embers (coals with glowing cracks),
//   2 lava (crust over bright cracks), 3 smoke, 4 steam
//   extras: flicker (bit 0)
// Levels (q, the detail group): lite draws what low draws
// (every tier keeps the pattern; before critic round 1 lite was a static glowing card (no noise, no
// time)); low scrolls two octaves and flickers; medium adds a third octave,
// the soft alpha edge and the rising embers (flame and smoke); high adds a
// fourth octave and a domain warp so the tongues lick; ultra draws as high.

// The flicker factor at time t: slow value noise around 1.
fn rc_fire_f(b: RecBlock, t: f32) -> f32 {
    if (!rc_extra(b, RC_X_FIRE, 0u)) {
        return 1.0;
    }
    return 1.0 + (rc_vnoise2(vec2<f32>(t * rc_range(b.na.y, 0.0, 20.0) * 0.5, f32(b.seed)), b.seed) - 0.5) * 2.0 * b.na.z;
}

fn rc_fire(b: RecBlock, f: MatFrame, i: MatIn, q: u32) -> MatOut {
    let it = rc_range(b.na.x, 0.0, 4.0);
    let u = i.uv;
    let hh = clamp(1.0 - u.y, 0.0, 1.0);
    let shape = clamp((1.0 - abs(u.x - 0.5) * 2.0) * (1.35 - hh * 1.2), 0.0, 1.0);
    var o = rc_out(i, b.col_c * 0.15, 1.0, 0.0);
    let t = rc_time();
    let sc = rc_range(b.nb.x, 0.0, 5.0);
    let oc = select(2u, select(3u, 4u, q >= 3u), q >= 2u);
    var p = vec2<f32>(u.x * 4.0, u.y * 4.0 + t * sc);
    if (q >= 3u) {
        p = p + (rc_vnoise2(p * 1.7 + vec2<f32>(0.0, t * sc * 0.7), b.seed + 7u) - 0.5) * 1.2;
    }
    let n = rc_fbm2(p, b.seed, oc);
    let fl = rc_fire_f(b, t);
    let soft = 0.05 + b.na.w * 0.5;
    var col = vec3<f32>(0.0);
    var a = 1.0;
    if (b.variant == 0u) {
        let d = clamp(shape + (n - 0.5) * (0.5 + 0.8 * hh), 0.0, 1.0) * fl;
        col = mix(b.col_b, b.col_a, smoothstep(0.45, 0.9, d)) * smoothstep(0.1, 0.4, d);
        col = mix(col, vec3<f32>(0.2, 0.4, 1.0) * smoothstep(0.1, 0.5, d), (1.0 - smoothstep(0.0, 0.18, hh)) * 0.35);
        a = smoothstep(0.0, soft, d - 0.15);
    } else if (b.variant <= 2u) {
        let c = rc_voronoi2(vec2<f32>(u.x, u.y + t * sc * 0.03) * 7.0, b.seed + 1u, 1.0);
        let crack = 1.0 - smoothstep(0.0, 0.12 + b.na.w * 0.25, c.z);
        var d = crack * (0.6 + 0.8 * n) * fl;
        if (b.variant == 1u) {
            let pulse = select(1.0, 0.65 + 0.35 * sin(t * rc_range(b.na.y, 0.0, 20.0) * 0.5 + c.w * 6.2832), rc_extra(b, RC_X_FIRE, 0u));
            d = crack * (0.3 + 0.7 * step(c.w, 0.35 + 0.65 * b.nb.y)) * pulse;
        }
        col = mix(b.col_b, b.col_a, d) * d;
        o.base = vec4<f32>(mix(b.col_c * 0.12, b.col_c * 0.3, n) * i.tex.rgb, i.base.a);
    } else {
        let sd = clamp(n * 1.3 - 0.35 - select(0.0, hh * 0.6, b.variant == 4u), 0.0, 1.0) * (0.5 + 0.5 * shape);
        a = smoothstep(0.0, soft + 0.2, sd) * 0.8;
        o.base = vec4<f32>(select(b.col_c * (0.4 + 1.2 * sd), mix(b.col_c, b.col_a, sd), b.variant == 4u) * i.tex.rgb, i.base.a * a);
        col = b.col_b * (0.15 * (1.0 - hh) * it * a);
    }
    if (q >= 2u && b.variant != 4u && b.variant != 1u && b.nb.y > 0.0) {
        let e = rc_voronoi2(vec2<f32>(u.x * 24.0, u.y * 24.0 + t * (sc + 1.0) * 3.0), b.seed + 3u, 1.0);
        let sp = (1.0 - smoothstep(0.05, 0.16, e.x)) * step(e.w, b.nb.y * 0.5) * smoothstep(0.0, 0.2, hh);
        col = col + mix(b.col_b, b.col_a, 0.5) * (sp * 1.5);
    }
    if (b.variant == 0u) {
        o.base.a = i.base.a * a;
    }
    o.emit = col * it;
    return o;
}
