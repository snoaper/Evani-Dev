// evani.shader.bloom_composite
// Bloom pass 3 (bloom.composite): add the blurred bright parts (bloom.mip1)
// back onto the HDR image. Drawn into `hdr` with additive blending, so the
// pass never reads the target it writes.

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

struct Composite {
    // fx.bloom.tint (linear RGB).
    tint: vec3<f32>,
    // fx.bloom.intensity.
    intensity: f32,
};

@group(0) @binding(0) var bloom: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> u: Composite;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let b = textureSample(bloom, samp, in.uv).rgb;
    return vec4<f32>(b * u.tint * u.intensity, 0.0);
}
