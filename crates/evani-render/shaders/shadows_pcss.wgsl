
// evani.shader.shadows_pcss
// Origin: ChimpDuty's prepared `pcss.wgsl` (handover/evani-dev/gaps/
// evani-look-stylizedpbr/shaders/pcss.wgsl, Chris Pearson's own project,
// written for Evani.Dev), adopted by S5-R4 as evani-render feature `pcss`
// (alias `soft-shadows`, the stylizedpbr look's name). Changed here: the
// filter works on one cascade (`m` is that cascade's light matrix, so the
// penumbra's uv per metre is right in each) and its taps stay inside it.
// Soft shadows from the same shadow map, bindings and `shadow_vis` signature
// as the engine's PCF lookup. Contact shadows stay sharp and long ones blur,
// as under a real sun of angular size `look.stylizedpbr.pcss_light_deg`:
//   1. blocker search: average depth of the shadow-map texels nearer to the
//      light than this point, over a disc of `pcss_search` texels;
//   2. penumbra width = (receiver - blocker) distance in metres x the sun's
//      angular size, turned into shadow-map uv from the light matrix (its z
//      row's length is 1 / depth range, its x row's length 1 / half width);
//   3. filter: hardware-compared taps over a disc of that width.
// Taps lie on a golden-angle spiral turned per texel by a hash, 16 on the
// tiers with a PCF radius (High, Ultra) and 8 without (Medium, VR).
// `pcss_params`: penumbra per metre, search radius, min and max filter
// radius (texels).

//@slot pcss_params

fn pcss_spin(p: vec2<f32>) -> f32 {
    let c = vec2<u32>(vec2<i32>(floor(p)));
    var h = (c.x * 0x8f1bbcdcu) ^ (c.y * 0x5f356495u);
    h = h ^ (h >> 16u);
    h = h * 0x6b43a9b5u;
    h = h ^ (h >> 16u);
    return f32(h & 0xffffu) / 65535.0 * 6.2831853;
}

// Tap i of n on a disc of radius 1 (golden-angle spiral, even area).
fn pcss_tap(i: i32, n: i32, spin: f32) -> vec2<f32> {
    let r = sqrt((f32(i) + 0.5) / f32(n));
    let a = f32(i) * 2.3999632 + spin;
    return vec2<f32>(cos(a), sin(a)) * r;
}

fn shadow_filter(m: mat4x4<f32>, s: vec4<f32>, x0: f32, xs: f32) -> f32 {
    let prm = slot_pcss_params(0u);
    let texel = frame.shadow.z;
    let dims = vec2<f32>(textureDimensions(shadow_map));
    let row_x = vec3<f32>(m[0].x, m[1].x, m[2].x);
    let row_z = vec3<f32>(m[0].z, m[1].z, m[2].z);
    let uv_per_m = 0.5 * length(row_x);
    let m_per_z = 1.0 / max(length(row_z), 1e-6);
    let taps = select(8, 16, frame.shadow.w > 0.5);
    let spin = pcss_spin(s.xy / max(texel, 1e-6));
    var blockers = 0.0;
    var count = 0.0;
    let search = prm.y * texel;
    for (var i = 0; i < taps; i = i + 1) {
        let t = clamp(shadow_uv(s.xy + pcss_tap(i, taps, spin) * search, x0, xs) * dims, vec2<f32>(0.0), dims - vec2<f32>(1.0));
        let d = textureLoad(shadow_map, vec2<i32>(t), 0);
        if (d < s.z) {
            blockers = blockers + d;
            count = count + 1.0;
        }
    }
    if (count < 0.5) {
        return 1.0;
    }
    let blocker = blockers / count;
    let penumbra_m = max(s.z - blocker, 0.0) * m_per_z * prm.x;
    let radius = clamp(penumbra_m * uv_per_m, prm.z * texel, prm.w * texel);
    var sum = 0.0;
    for (var i = 0; i < taps; i = i + 1) {
        sum = sum + textureSampleCompareLevel(shadow_map, shadow_cmp, shadow_uv(s.xy + pcss_tap(i, taps, spin + 1.7) * radius, x0, xs), s.z);
    }
    return sum / f32(taps);
}
