// evani.shader.bloom_down
// Bloom pass 2a (bloom.blur, downsample chain): dual-Kawase downsample from
// bloom.mip<n> into bloom.mip<n+1> (half the size each step), repeated for
// fx.bloom.mips levels.

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
    // Source mip level (0 = half resolution).
    mip: u32,
};

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> u: Blur;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let o = u.texel * u.offset;
    var sum = textureSample(src, samp, in.uv).rgb * 4.0;
    sum += textureSample(src, samp, in.uv - o).rgb;
    sum += textureSample(src, samp, in.uv + o).rgb;
    sum += textureSample(src, samp, in.uv + vec2<f32>(o.x, -o.y)).rgb;
    sum += textureSample(src, samp, in.uv + vec2<f32>(-o.x, o.y)).rgb;
    return vec4<f32>(sum * 0.125, 1.0);
}
