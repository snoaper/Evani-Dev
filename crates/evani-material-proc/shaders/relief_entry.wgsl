// ---- relief entry ----------------------------------------------------------------

// `$SKY_CALL` becomes `mp_sky(n)` with feature `sky`, else `vec3<f32>(1.0)`.
// kit_surface: the forward pass's call (colour, cut-out, bent normal and
// roughness through `nr`).
fn kit_surface(p: vec3<f32>, n: vec3<f32>, material: u32, nr: ptr<function, vec4<f32>>) -> vec4<f32> {
    // Before any branch: derivatives need uniform control flow.
    let fw = length(fwidth(p)) * 0.6;
    if ((material != MP_WORLD && material != MP_DECAL && material != MP_SKY) || arrayLength(&kit_data) < 8u) {
        return vec4<f32>(1.0);
    }
    if (material == MP_SKY) {
        return vec4<f32>($SKY_CALL, 1.0);
    }
    if (material == MP_DECAL) {
        return mp_decal(p, n, fw);
    }
    return vec4<f32>(rl_world(p, n, fw, true, nr), 1.0);
}

// kit_material: the shadow pass and any forward source without the surface
// splice. Relief surfaces paint flat there (their alpha is always 1).
fn kit_material(p: vec3<f32>, n: vec3<f32>, material: u32) -> vec4<f32> {
    let fw = length(fwidth(p)) * 0.6;
    if ((material != MP_WORLD && material != MP_DECAL && material != MP_SKY) || arrayLength(&kit_data) < 8u) {
        return vec4<f32>(1.0);
    }
    if (material == MP_SKY) {
        return vec4<f32>($SKY_CALL, 1.0);
    }
    if (material == MP_DECAL) {
        return mp_decal(p, n, fw);
    }
    var nr = vec4<f32>(n, 1.0);
    return vec4<f32>(rl_world(p, n, fw, false, &nr), 1.0);
}
