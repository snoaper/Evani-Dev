// evani.shader.recipes.refract_stub
// Refract-mode glass in the blend pipeline (no copy of what is behind):
// drawn as premultiplied over, like clear glass (batch 3 B3-P3).
fn mat_refract(c: vec3<f32>, o: MatOut, a: f32, pos: vec4<f32>, g: vec3<f32>, world: vec3<f32>) -> vec4<f32> {
    return vec4<f32>(c, a);
}
