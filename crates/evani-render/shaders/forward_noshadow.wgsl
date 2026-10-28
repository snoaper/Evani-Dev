
// No shadow map in this build (feature `shadows` off): full light.
fn shadow_vis(world: vec3<f32>, n: vec3<f32>) -> f32 {
    return 1.0;
}
