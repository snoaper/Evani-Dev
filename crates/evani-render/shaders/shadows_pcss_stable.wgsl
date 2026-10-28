
// evani.shader.shadows_pcss_stable
// Stable soft shadows (evani-render feature `pcss-stable`, ChimpDuty intake
// limit 5). Same shadow map, bindings, `pcss_params` slot and penumbra model
// as shadows_pcss.wgsl (which stays as it is); it takes the receiver's normal
// (shadows_one_stable.wgsl and shadows_two_stable.wgsl pass it) and fixes
// the speckle seen under bodies lying on a floor:
//   * Cause 1, self-blocking: the PCSS search compared every tap with the
//     centre's depth, so on a floor at a low sun the floor's own texels a few
//     texels towards the light were "blockers" (a slope the constant bias
//     cannot cover), the blocker distance came out near zero and the filter
//     self-shadowed part of its taps. Here each tap is compared with the
//     receiver plane's own depth at that tap (receiver-plane depth bias,
//     from the normal through the light matrix). The filter blends each
//     tap's four texels by hand, each compared with the plane at its own
//     centre, so the margin is only a fifth of a texel of slope (a margin of
//     a whole texel, as the hardware compare needs, is tens of centimetres
//     of depth at a low sun on a coarse map and lost real shadows).
//   * Cause 2, noise: the taps were spun by a hash per shadow texel, so the
//     part-shadowed taps changed from texel to texel into a blocky speckle.
//     Here the tap pattern is fixed (golden-angle spiral, no hash), so a
//     penumbra is a smooth gradient.
//   * The filter radius is at least 1.5 texels (the slot's minimum when
//     larger), so a contact edge is always filtered, never a raw texel step.
// Taps: 16 on the tiers with a PCF radius (High, Ultra), 8 without.

//@slot pcss_params

// Tap i of n on a disc of radius 1 (golden-angle spiral, even area).
fn pcss_tap(i: i32, n: i32, spin: f32) -> vec2<f32> {
    let r = sqrt((f32(i) + 0.5) / f32(n));
    let a = f32(i) * 2.3999632 + spin;
    return vec2<f32>(cos(a), sin(a)) * r;
}

// The receiver plane: light-space depth change per unit of cascade uv on the
// surface of normal `n` (world). The light matrix is an orthographic
// projection of a look-at, so its rows are orthogonal and the normal in
// light clip space is row . n / |row|^2. The slope is capped at `cap`
// (grazing surfaces would otherwise get an unbounded bias).
fn pcss_plane(m: mat4x4<f32>, n: vec3<f32>, cap: f32) -> vec2<f32> {
    if (dot(n, n) < 1e-8) {
        return vec2<f32>(0.0);
    }
    let rx = vec3<f32>(m[0].x, m[1].x, m[2].x);
    let ry = vec3<f32>(m[0].y, m[1].y, m[2].y);
    let rz = vec3<f32>(m[0].z, m[1].z, m[2].z);
    let lx = dot(rx, n) / max(dot(rx, rx), 1e-12);
    let ly = dot(ry, n) / max(dot(ry, ry), 1e-12);
    let lz0 = dot(rz, n) / max(dot(rz, rz), 1e-12);
    let lz = select(-1e-6, lz0, abs(lz0) > 1e-6);
    // Cascade uv = (x * 0.5 + 0.5, 0.5 - y * 0.5): dx = 2 du, dy = -2 dv.
    let g = vec2<f32>(-2.0 * lx / lz, 2.0 * ly / lz);
    let len = length(g);
    return select(g, g * (cap / len), len > cap);
}

// The receiver's depth at atlas texel `c` (its centre on the plane `g`
// through `s`), less `margin`, against the depth stored there: 1 lit, 0 not.
fn pcss_texel_lit(c: vec2<f32>, dims: vec2<f32>, s: vec4<f32>, x0: f32, xs: f32, g: vec2<f32>, margin: f32) -> f32 {
    let d = textureLoad(shadow_map, vec2<i32>(c), 0);
    let at = (c + vec2<f32>(0.5)) / dims;
    let o = vec2<f32>(at.x / xs - x0, at.y) - s.xy;
    return select(0.0, 1.0, s.z + dot(g, o) - margin <= d);
}

fn shadow_filter(m: mat4x4<f32>, s: vec4<f32>, x0: f32, xs: f32, n: vec3<f32>) -> f32 {
    let prm = slot_pcss_params(0u);
    let texel = frame.shadow.z;
    let dims = vec2<f32>(textureDimensions(shadow_map));
    // This cascade's texel columns in the atlas: a tap never reads the next.
    let lo = vec2<f32>(floor(x0 * xs * dims.x + 0.5), 0.0);
    let hi = vec2<f32>(floor((x0 + 1.0) * xs * dims.x + 0.5) - 1.0, dims.y - 1.0);
    let row_x = vec3<f32>(m[0].x, m[1].x, m[2].x);
    let row_z = vec3<f32>(m[0].z, m[1].z, m[2].z);
    let uv_per_m = 0.5 * length(row_x);
    let z_per_m = length(row_z);
    let m_per_z = 1.0 / max(z_per_m, 1e-6);
    let taps = select(8, 16, frame.shadow.w > 0.5);
    // A slope of at most 16 (about 86 degrees from the light) in z per uv.
    let g = pcss_plane(m, n, 16.0 * z_per_m / max(uv_per_m, 1e-6));
    // Every compare is against the plane at the stored texel's own centre,
    // so the margin only covers rounding: a fifth of a texel of slope.
    let margin = 0.2 * length(g) * texel;
    var blockers = 0.0;
    var count = 0.0;
    let search = prm.y * texel;
    for (var i = 0; i < taps; i = i + 1) {
        let t = clamp(floor(shadow_uv(s.xy + pcss_tap(i, taps, 0.0) * search, x0, xs) * dims), lo, hi);
        let d = textureLoad(shadow_map, vec2<i32>(t), 0);
        let at = (t + vec2<f32>(0.5)) / dims;
        let zr = s.z + dot(g, vec2<f32>(at.x / xs - x0, at.y) - s.xy) - margin;
        if (d < zr) {
            blockers = blockers + (zr - d);
            count = count + 1.0;
        }
    }
    if (count < 0.5) {
        return 1.0;
    }
    let penumbra_m = (blockers / count) * m_per_z * prm.x;
    let radius = clamp(penumbra_m * uv_per_m, max(prm.z, 1.5) * texel, max(prm.w, 1.5) * texel);
    // Filter: each tap blends its four texels by hand (as the hardware
    // compare would), but each texel against the plane at its own centre.
    var sum = 0.0;
    for (var i = 0; i < taps; i = i + 1) {
        let t = shadow_uv(s.xy + pcss_tap(i, taps, 1.7) * radius, x0, xs) * dims - vec2<f32>(0.5);
        let b = floor(t);
        let f = t - b;
        let c00 = clamp(b, lo, hi);
        let c11 = clamp(b + vec2<f32>(1.0), lo, hi);
        let l00 = pcss_texel_lit(c00, dims, s, x0, xs, g, margin);
        let l10 = pcss_texel_lit(vec2<f32>(c11.x, c00.y), dims, s, x0, xs, g, margin);
        let l01 = pcss_texel_lit(vec2<f32>(c00.x, c11.y), dims, s, x0, xs, g, margin);
        let l11 = pcss_texel_lit(c11, dims, s, x0, xs, g, margin);
        sum = sum + mix(mix(l00, l10, f.x), mix(l01, l11, f.x), f.y);
    }
    return sum / f32(taps);
}
