// evani.shader.model_shadows
// Shadow-map depth for models (features `models` and `shadows`): quantised
// positions through the instance's world rows, from the light. No fragment
// stage, so alpha-masked parts cast solid shadows.

struct ModelShadowIn {
    @location(0) pos: vec4<f32>,
    @location(4) row0: vec4<f32>,
    @location(5) row1: vec4<f32>,
    @location(6) row2: vec4<f32>,
    @location(7) pos_scale: vec4<f32>,
    @location(8) pos_offset: vec4<f32>,
};

@vertex
fn model_shadow_vs(in: ModelShadowIn) -> @builtin(position) vec4<f32> {
    let local = vec4<f32>(in.pos_offset.xyz + in.pos_scale.xyz * in.pos.xyz, 1.0);
    return s.light_vp * vec4<f32>(dot(in.row0, local), dot(in.row1, local), dot(in.row2, local), 1.0);
}
