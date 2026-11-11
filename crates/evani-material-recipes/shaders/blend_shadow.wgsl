// evani.shader.recipes.blend_shadow
// See-through shadows (batch 3 B3-P3, feature `blend`): appended to
// evani-render's hole-aware depth module, which the shadow map and its
// cascades draw blended parts through. A screen-door (stipple) shadow on the
// existing cut-out path, no new shadow target: a 4x4 threshold on the shadow
// map's texels keeps the share of texels the material blocks, and the shadow
// filter (PCF) blends them into a partial shadow. Written fresh for
// Evani.Dev; src/blend.rs mirrors the density for its tests.
//   glass: blocks 1 - what comes through: the body's alpha (frost milks it,
//          stained and block glass are denser) and the tint's brightness to
//          the power the thickness gives (as the colour pass tints), so clear
//          panes cast a faint shadow, stained glass a deeper one
//   smoke and steam: a light shadow (a third of the light)
//   flames and holograms: none (they give light, they do not block it)
//   a clear gem (extras bit 2) 0.4, clear organic slime (bit 1) 0.6
// rc_blend_drop: -1 not a see-through block (the cut-out decides), 0 keep
// this texel, 1 drop it.

fn rc_bs_th(p: vec2<f32>) -> f32 {
    // The 4x4 Bayer matrix: its bits are the bits of x ^ y and y, interleaved
    // (src/blend.rs's table has the same values).
    let i = vec2<u32>(p) & vec2<u32>(3u);
    let d = i.x ^ i.y;
    return (f32((d & 1u) * 8u + (i.y & 1u) * 4u + (d & 2u) + (i.y >> 1u)) + 0.5) / 16.0;
}

fn rc_blend_density(w: RecRaw) -> f32 {
    let r = w.w0.x & 0xffu;
    let v = (w.w0.x >> 8u) & 0xffu;
    if ((r == 28u && v == 0u) || (r == 29u && v == 4u)) {
        return 0.0;
    }
    if (r == 28u && (v == 3u || v == 4u)) {
        return 0.33;
    }
    let x = (w.w0.x >> 16u) & 0xffu;
    if ((r == 25u && (x & 4u) != 0u) || (r == 22u && (x & 2u) != 0u)) {
        return select(0.6, 0.4, r == 25u);
    }
    if (r != 24u) {
        return -1.0;
    }
    let c = unpack4x8unorm(w.w0.y).xyz;
    let th = f32((w.w0.w >> 8u) & 0xffu) / 255.0;
    let frost = f32((w.w0.w >> 16u) & 0xffu) / 255.0;
    var a = 0.05 + 0.08 * th + 0.35 * frost;
    if (v == 1u) {
        a = 0.5;
    } else if (v == 3u) {
        a = 0.55 + 0.25 * th;
    } else if (v == 6u) {
        a = a + 0.3;
    }
    let lum = dot(c * c, vec3<f32>(0.2126, 0.7152, 0.0722));
    return 1.0 - (1.0 - clamp(a, 0.0, 1.0)) * pow(max(lum, 1e-4), 0.5 + 2.0 * th);
}

fn rc_blend_drop(w: RecRaw, p: vec2<f32>) -> i32 {
    let d = rc_blend_density(w);
    if (d < 0.0) {
        return -1;
    }
    return select(0, 1, d < rc_bs_th(p));
}
