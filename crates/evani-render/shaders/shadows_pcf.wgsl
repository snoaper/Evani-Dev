
// The engine's PCF filter for one cascade: hardware 2x2 compare over a
// (2r+1)^2 kernel (r from the tier), offsets in cascade texels.
fn shadow_filter(m: mat4x4<f32>, s: vec4<f32>, x0: f32, xs: f32) -> f32 {
    let r = i32(clamp(frame.shadow.w, 0.0, 2.0));
    var sum = 0.0;
    var taps = 0.0;
    for (var y = -r; y <= r; y = y + 1) {
        for (var x = -r; x <= r; x = x + 1) {
            let o = vec2<f32>(f32(x), f32(y)) * frame.shadow.z;
            sum = sum + textureSampleCompareLevel(shadow_map, shadow_cmp, shadow_uv(s.xy + o, x0, xs), s.z);
            taps = taps + 1.0;
        }
    }
    return sum / taps;
}
