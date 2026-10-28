// evani.shader.kit_shadows
// Shadow-map depth pass with the kit material's cut-out (feature
// `kit-material`): the vertex stage of `shadows_depth.wgsl` plus a fragment
// stage that discards where `kit_material` (appended at build time with the
// `kit_data` binding) returns alpha below 0.5, so holes let light through.
// Used only while a kit has set material code of its own.

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

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) @interpolate(flat) material: u32,
};

fn rotate(q: vec4<f32>, v: vec3<f32>) -> vec3<f32> {
    let t = 2.0 * cross(q.xyz, v);
    return v + q.w * t + cross(q.xyz, t);
}

@vertex
fn vs_main(in: VsIn) -> VsOut {
    var o: VsOut;
    let m = min(u32(max(in.inst_pos.w, 0.0)), 15u);
    o.material = m;
    o.world = vec3<f32>(0.0);
    o.normal = vec3<f32>(0.0, 1.0, 0.0);
    if (s.unlit[m / 4u][m % 4u] > 0.5) {
        o.pos = vec4<f32>(0.0, 0.0, -2.0, 1.0);
        return o;
    }
    let world = rotate(in.inst_rot, in.pos * in.inst_scale.xyz) + in.inst_pos.xyz;
    o.pos = s.light_vp * vec4<f32>(world, 1.0);
    o.world = world;
    o.normal = rotate(in.inst_rot, in.normal / max(abs(in.inst_scale.xyz), vec3<f32>(1e-4)) * sign(in.inst_scale.xyz));
    return o;
}

@fragment
fn fs_main(in: VsOut) {
    if (kit_material(in.world, normalize(in.normal), in.material).a < 0.5) {
        discard;
    }
}
