@group(0) @binding(1) var shadow_map: texture_depth_2d;
@group(0) @binding(2) var shadow_cmp: sampler_comparison;

// evani.shader.shadows_pcss
// evani-look-stylizedpbr feature `pcss`: soft shadows from the same shadow
// map, bindings and `shadow_vis` signature as the engine's PCF lookup (this
// file replaces it). Contact shadows stay sharp and long ones blur, as under
// a real sun of angular size `look.stylizedpbr.pcss_light_deg`:
//   1. blocker search: average depth of the shadow-map texels nearer to the
//      light than this point, over a disc of `pcss_search` texels;
//   2. penumbra width = (receiver - blocker) distance in metres x the sun's
//      angular size, turned into shadow-map uv from the light matrix (its z
//      row's length is 1 / depth range, its x row's length 1 / half width);
//   3. filter: hardware-compared taps over a disc of that width.
// Taps lie on a golden-angle spiral turned per texel by a hash, 16 on the
// tiers with a PCF radius (High) and 8 without (Medium).

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

fn shadow_vis(world: vec3<f32>, n: vec3<f32>) -> f32 {
    if (frame.shadow.x < 0.5) {
        return 1.0;
    }
    let p = frame.light_vp * vec4<f32>(world + n * frame.blob_info.z, 1.0);
    let uv = vec2<f32>(p.x * 0.5 + 0.5, 0.5 - p.y * 0.5);
    let edge = max(abs(p.x), abs(p.y));
    if (edge >= 1.0 || p.z <= 0.0 || p.z >= 1.0) {
        return 1.0;
    }
    let z = p.z - frame.blob_info.w;
    let prm = slot_pcss_params(0u);
    let texel = frame.shadow.z;
    let dims = vec2<f32>(textureDimensions(shadow_map));
    let row_x = vec3<f32>(frame.light_vp[0].x, frame.light_vp[1].x, frame.light_vp[2].x);
    let row_z = vec3<f32>(frame.light_vp[0].z, frame.light_vp[1].z, frame.light_vp[2].z);
    let uv_per_m = 0.5 * length(row_x);
    let m_per_z = 1.0 / max(length(row_z), 1e-6);
    let taps = select(8, 16, frame.shadow.w > 0.5);
    let spin = pcss_spin(uv * dims);
    var blockers = 0.0;
    var count = 0.0;
    let search = prm.y * texel;
    for (var i = 0; i < taps; i = i + 1) {
        let t = clamp((uv + pcss_tap(i, taps, spin) * search) * dims, vec2<f32>(0.0), dims - vec2<f32>(1.0));
        let d = textureLoad(shadow_map, vec2<i32>(t), 0);
        if (d < z) {
            blockers = blockers + d;
            count = count + 1.0;
        }
    }
    if (count < 0.5) {
        return 1.0;
    }
    let blocker = blockers / count;
    let penumbra_m = max(z - blocker, 0.0) * m_per_z * prm.x;
    let radius = clamp(penumbra_m * uv_per_m, prm.z * texel, prm.w * texel);
    var sum = 0.0;
    for (var i = 0; i < taps; i = i + 1) {
        sum = sum + textureSampleCompareLevel(shadow_map, shadow_cmp, uv + pcss_tap(i, taps, spin + 1.7) * radius, z);
    }
    let lit = sum / f32(taps);
    let v = mix(1.0, lit, clamp(frame.shadow.y, 0.0, 1.0));
    return mix(v, 1.0, smoothstep(0.85, 1.0, edge));
}
