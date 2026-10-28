// evani.shader.bloom_up
// Bloom pass 2b (bloom.blur, upsample chain): dual-Kawase upsample from
// bloom.mip<n+1> back into bloom.mip<n>, drawn with additive blending so
// each level accumulates the blurrier ones below it. Each level is weighted
// by the `bloom_weight` shader slot: the renderer replaces the marker line
// below with `fn slot_bloom_weight(mip: u32) -> f32` and the active body.

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

struct Blur {
    // 1 / source size in pixels.
    texel: vec2<f32>,
    // Sample offset in source texels (fx.bloom.spread).
    offset: f32,
    // Source mip level (0 = half resolution, 1 = quarter...).
    mip: u32,
};

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> u: Blur;

//@slot bloom_weight

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let o = u.texel * u.offset;
    var sum = textureSample(src, samp, in.uv + vec2<f32>(-2.0 * o.x, 0.0)).rgb;
    sum += textureSample(src, samp, in.uv + vec2<f32>(-o.x, o.y)).rgb * 2.0;
    sum += textureSample(src, samp, in.uv + vec2<f32>(0.0, 2.0 * o.y)).rgb;
    sum += textureSample(src, samp, in.uv + vec2<f32>(o.x, o.y)).rgb * 2.0;
    sum += textureSample(src, samp, in.uv + vec2<f32>(2.0 * o.x, 0.0)).rgb;
    sum += textureSample(src, samp, in.uv + vec2<f32>(o.x, -o.y)).rgb * 2.0;
    sum += textureSample(src, samp, in.uv + vec2<f32>(0.0, -2.0 * o.y)).rgb;
    sum += textureSample(src, samp, in.uv + vec2<f32>(-o.x, -o.y)).rgb * 2.0;
    let w = slot_bloom_weight(u.mip);
    return vec4<f32>(sum * (w / 12.0), 1.0);
}
