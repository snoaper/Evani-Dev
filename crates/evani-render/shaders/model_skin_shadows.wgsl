// evani.shader.model_skin_shadows
// Shadow-map depth for skinned models (features `skinning` and `shadows`):
// the position skinned as model_skin_vs does it (joints at group 1 here),
// then through the instance's world rows, from the light.

@group(1) @binding(0) var<storage, read> joints: array<mat3x4<f32>>;

@vertex
fn model_skin_shadow_vs(in: ModelShadowIn, @location(11) j: vec4<u32>, @location(12) wt: vec4<f32>) -> @builtin(position) vec4<f32> {
    let b = u32(in.pos_offset.w);
    let m = joints[b + j.x] * wt.x + joints[b + j.y] * wt.y + joints[b + j.z] * wt.z + joints[b + j.w] * wt.w;
    let p = vec4<f32>(vec4<f32>(in.pos_offset.xyz + in.pos_scale.xyz * in.pos.xyz, 1.0) * m, 1.0);
    return s.light_vp * vec4<f32>(dot(in.row0, p), dot(in.row1, p), dot(in.row2, p), 1.0);
}
