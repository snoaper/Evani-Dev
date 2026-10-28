
// evani.shader.shadows_sample
// Shadow-map lookup for the forward pass (feature `shadows`): the light's
// depth from the `shadows` pass, compared with hardware 2x2 filtering, over
// a (2r+1)^2 PCF kernel (r from the tier). Faded out at the edge of the
// shadow box so there is no hard border.
@group(0) @binding(1) var shadow_map: texture_depth_2d;
@group(0) @binding(2) var shadow_cmp: sampler_comparison;

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
    let r = i32(clamp(frame.shadow.w, 0.0, 2.0));
    var sum = 0.0;
    var taps = 0.0;
    for (var y = -r; y <= r; y = y + 1) {
        for (var x = -r; x <= r; x = x + 1) {
            let o = vec2<f32>(f32(x), f32(y)) * frame.shadow.z;
            sum = sum + textureSampleCompareLevel(shadow_map, shadow_cmp, uv + o, z);
            taps = taps + 1.0;
        }
    }
    let lit = sum / taps;
    let v = mix(1.0, lit, clamp(frame.shadow.y, 0.0, 1.0));
    return mix(v, 1.0, smoothstep(0.85, 1.0, edge));
}
