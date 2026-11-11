// The field sea's foam view is a dev tool: ship builds draw the water as it is
// (sea_field_debug.wgsl has the view; src/sea/mod.rs picks one).
fn fd_debug_out(c: vec3<f32>) -> vec3<f32> {
    return c;
}
