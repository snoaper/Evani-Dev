
// Two cascades with stable soft shadows (features `shadow-cascades` and
// `pcss-stable`): shadows_two.wgsl, passing the receiver's normal to the
// filter.
fn shadow_vis(world: vec3<f32>, n: vec3<f32>) -> f32 {
    if (frame.shadow.x < 0.5) {
        return 1.0;
    }
    let info = frame.blobs[4];
    let far = shadow_pos(frame.light_vp, world, n);
    if (far.w >= 1.0) {
        return 1.0;
    }
    var lit = 1.0;
    let nm = mat4x4<f32>(frame.blobs[0], frame.blobs[1], frame.blobs[2], frame.blobs[3]);
    let near = shadow_pos(nm, world, n);
    if (info.x > 1.5 && near.w < 1.0) {
        lit = shadow_filter(nm, near, 1.0, info.z, n);
        let t = smoothstep(info.y, 1.0, near.w);
        if (t > 0.0) {
            lit = mix(lit, shadow_filter(frame.light_vp, far, 0.0, info.z, n), t);
        }
    } else {
        lit = shadow_filter(frame.light_vp, far, 0.0, info.z, n);
    }
    let v = mix(1.0, lit, clamp(frame.shadow.y, 0.0, 1.0));
    return mix(v, 1.0, smoothstep(0.85, 1.0, far.w));
}
