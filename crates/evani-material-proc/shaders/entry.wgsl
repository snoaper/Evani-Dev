// ---- entry ---------------------------------------------------------------------

// `$SKY_CALL` becomes `mp_sky(n)` with feature `sky`, else `vec3<f32>(1.0)`
// (MP_SKY is then the no-slot number, so the branch never runs).
fn kit_material(p: vec3<f32>, n: vec3<f32>, material: u32) -> vec4<f32> {
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
    return vec4<f32>(mp_world(p, n, fw), 1.0);
}
