
// evani.shader.shadows_cascades
// Two cascades (feature `shadow-cascades`). `light_vp` is the far cascade,
// today's whole shadow box; the near cascade's matrix is in `blobs[0..4]`
// (free while the shadow map is on: no blobs then) and `blobs[4]` holds
// (cascades, blend start, atlas x scale, 0). A fragment inside the near box
// uses the near cascade; from `blend start` (in near-box edge units) to its
// side it blends to the far one, so the switch never shows as a line. With
// one cascade (the tier's `shadow_cascades` or a small device) this is the
// single-map lookup.
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
        lit = shadow_filter(nm, near, 1.0, info.z);
        let t = smoothstep(info.y, 1.0, near.w);
        if (t > 0.0) {
            lit = mix(lit, shadow_filter(frame.light_vp, far, 0.0, info.z), t);
        }
    } else {
        lit = shadow_filter(frame.light_vp, far, 0.0, info.z);
    }
    let v = mix(1.0, lit, clamp(frame.shadow.y, 0.0, 1.0));
    return mix(v, 1.0, smoothstep(0.85, 1.0, far.w));
}
