// ---- sky-only entry -----------------------------------------------------------
//
// For a game that draws the sky dome but no box world (an empty scene): the
// kit code is only the sky part (shaders/sky.wgsl) and this entry, so the
// forward and shadow shaders carry none of the surface painting.
// `$SKY_SLOT` becomes the sky slot's number.

const MP_SKY: u32 = $SKY_SLOTu;

fn kit_material(p: vec3<f32>, n: vec3<f32>, material: u32) -> vec4<f32> {
    if (material != MP_SKY || arrayLength(&kit_data) < 7u) {
        return vec4<f32>(1.0);
    }
    return mp_sky_rgba(n);
}
