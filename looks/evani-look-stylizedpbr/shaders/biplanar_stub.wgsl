
// evani.shader.biplanar_stub
// Without feature `biplanar`: no detail layer (neutral 0.5, 0.5).
fn detail_sample(p: vec3<f32>, n: vec3<f32>, scale: f32) -> vec2<f32> {
    return vec2<f32>(0.5);
}
