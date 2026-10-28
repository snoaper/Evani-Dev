
// evani.shader.fur_surface_stub
// Without feature `fur`: fur materials shade as plain surfaces (a = 0).
fn fur_sample(p: vec3<f32>, n: vec3<f32>, scale: f32) -> vec4<f32> {
    return vec4<f32>(1.0, 1.0, 0.5, 0.0);
}
