// evani.shader.shadows_sample
// Shadow-map lookup with cascades or soft shadows (S5-R4, features
// `shadow-cascades` and `pcss`). Replaces forward_shadows.wgsl at compile time
// (shaders.rs, FORWARD_EXT): same bindings and `shadow_vis` signature, so the
// forward, model and skinned stages are unchanged. With two cascades both
// maps sit side by side in one depth atlas: cascade `x0` (0 = far, 1 = near)
// covers atlas u in [x0 * xs, (x0 + 1) * xs], xs = 0.5 (1 with one cascade).
@group(0) @binding(1) var shadow_map: texture_depth_2d;
@group(0) @binding(2) var shadow_cmp: sampler_comparison;

// Where `world` lands in the cascade of light matrix `m`: cascade uv (xy),
// compare depth (z, bias applied) and edge (w: 0 at the box centre, 1 at its
// side, 2 outside the box's depth).
fn shadow_pos(m: mat4x4<f32>, world: vec3<f32>, n: vec3<f32>) -> vec4<f32> {
    let p = m * vec4<f32>(world + n * frame.blob_info.z, 1.0);
    let edge = select(max(abs(p.x), abs(p.y)), 2.0, p.z <= 0.0 || p.z >= 1.0);
    return vec4<f32>(p.x * 0.5 + 0.5, 0.5 - p.y * 0.5, p.z - frame.blob_info.w, edge);
}

// Atlas uv of cascade uv `uv`, kept half a texel inside its own cascade so a
// filter tap never reads the neighbouring one.
fn shadow_uv(uv: vec2<f32>, x0: f32, xs: f32) -> vec2<f32> {
    let h = 0.5 * frame.shadow.z;
    let c = clamp(uv, vec2<f32>(h), vec2<f32>(1.0 - h));
    return vec2<f32>((x0 + c.x) * xs, c.y);
}
