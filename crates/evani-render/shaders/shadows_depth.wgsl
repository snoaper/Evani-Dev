// evani.shader.shadows_depth
// Shadow-map depth pass (`shadows`, feature `shadows`): the forward pass's
// mesh instances drawn from the light into a depth-only target (no fragment
// stage). Unlit materials (glows, sparks, flames) cast nothing: their
// vertices are moved outside the clip volume.

struct Shadow {
    light_vp: mat4x4<f32>,
    // 1.0 for each unlit material slot (16, four per vec4).
    unlit: array<vec4<f32>, 4>,
};

@group(0) @binding(0) var<uniform> s: Shadow;

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) inst_pos: vec4<f32>,
    @location(3) inst_rot: vec4<f32>,
    @location(4) inst_scale: vec4<f32>,
    @location(5) inst_color: vec4<f32>,
};

fn rotate(q: vec4<f32>, v: vec3<f32>) -> vec3<f32> {
    let t = 2.0 * cross(q.xyz, v);
    return v + q.w * t + cross(q.xyz, t);
}

@vertex
fn vs_main(in: VsIn) -> @builtin(position) vec4<f32> {
    let m = min(u32(max(in.inst_pos.w, 0.0)), 15u);
    if (s.unlit[m / 4u][m % 4u] > 0.5) {
        return vec4<f32>(0.0, 0.0, -2.0, 1.0);
    }
    let world = rotate(in.inst_rot, in.pos * in.inst_scale.xyz) + in.inst_pos.xyz;
    return s.light_vp * vec4<f32>(world, 1.0);
}
