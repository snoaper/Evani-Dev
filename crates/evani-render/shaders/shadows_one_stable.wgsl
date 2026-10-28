
// One shadow map with stable soft shadows (feature `pcss-stable` without
// cascades): shadows_one.wgsl, passing the receiver's normal to the filter.
fn shadow_vis(world: vec3<f32>, n: vec3<f32>) -> f32 {
    if (frame.shadow.x < 0.5) {
        return 1.0;
    }
    let s = shadow_pos(frame.light_vp, world, n);
    if (s.w >= 1.0) {
        return 1.0;
    }
    let v = mix(1.0, shadow_filter(frame.light_vp, s, 0.0, 1.0, n), clamp(frame.shadow.y, 0.0, 1.0));
    return mix(v, 1.0, smoothstep(0.85, 1.0, s.w));
}
