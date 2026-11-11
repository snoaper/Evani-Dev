// evani.shader.studio_copy
// The studio passes' copy home (feature `studio-passes`, pass `studio.back`):
// `studio.tmp` back into `ldr` (a pass cannot read the target it writes).

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: VsOut;
    o.pos = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(p.x, 1.0 - p.y);
    return o;
}

struct Copy {
    size: vec2<f32>,
    pad: vec2<f32>,
};

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> u: Copy;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let t = clamp(vec2<i32>(in.pos.xy), vec2<i32>(0), vec2<i32>(u.size) - vec2<i32>(1));
    return vec4<f32>(textureLoad(src, t, 0).rgb, 1.0);
}
