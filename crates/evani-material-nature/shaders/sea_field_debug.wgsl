// The field sea's foam view (dev builds only: src/sea/mod.rs puts this file in
// with the `capture` feature, ship builds get sea_field_debug_off.wgsl). Turned
// on by `material.nature.water.foam_debug` (the live tools set it): the water
// in grey, its foam painted by cause (F_DBG, sea_field.wgsl):
//   red    new foam from an event now: a crest folding or going over, the shore;
//   blue   foam an event left a moment ago, carried with the water;
//   white  that foam fading;
//   yellow foam with no event behind it (there should be none).
fn fd_debug_out(c: vec3<f32>) -> vec3<f32> {
    if (((u32(max(u.spray, 0.0) + 0.5) >> 16u) & 1u) == 0u) {
        return c;
    }
    let g = vec3<f32>(dot(c, vec3<f32>(0.3, 0.5, 0.2)) * 0.45);
    let d = clamp(F_DBG * 5.0, vec4<f32>(0.0), vec4<f32>(1.0));
    var o = mix(g, vec3<f32>(1.0), d.z);
    o = mix(o, vec3<f32>(0.15, 0.45, 1.0), d.y);
    o = mix(o, vec3<f32>(1.0, 0.12, 0.08), d.x);
    o = mix(o, vec3<f32>(1.0, 0.9, 0.0), clamp(F_DBG.w * 4.0, 0.0, 1.0));
    return o;
}
