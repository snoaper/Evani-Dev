// evani.shader.bloom_bright
// Bloom pass 1 (bloom.bright): extract the bright parts of the HDR image
// into bloom.mip1 (half resolution) with a soft knee. The 4-tap box filter
// doubles as the first downsample and tames single-pixel fireflies.
// Uniforms come from fx.bloom.threshold / fx.bloom.knee / fx.bloom.clamp.

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    // One full-screen triangle, no vertex buffer.
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: VsOut;
    o.pos = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(p.x, 1.0 - p.y);
    return o;
}

struct Bright {
    // 1 / source size in pixels.
    texel: vec2<f32>,
    threshold: f32,
    knee: f32,
    clamp_max: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
};

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> u: Bright;

fn prefilter(c: vec3<f32>) -> vec3<f32> {
    let col = min(c, vec3<f32>(u.clamp_max));
    let lum = max(col.r, max(col.g, col.b));
    let knee = max(u.knee, 1e-4);
    // Quadratic soft knee around the threshold.
    var soft = clamp(lum - u.threshold + knee, 0.0, 2.0 * knee);
    soft = soft * soft / (4.0 * knee);
    let w = max(soft, lum - u.threshold) / max(lum, 1e-4);
    return col * w;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let o = u.texel * 0.5;
    var sum = prefilter(textureSample(src, samp, in.uv + vec2<f32>(-o.x, -o.y)).rgb);
    sum += prefilter(textureSample(src, samp, in.uv + vec2<f32>(o.x, -o.y)).rgb);
    sum += prefilter(textureSample(src, samp, in.uv + vec2<f32>(-o.x, o.y)).rgb);
    sum += prefilter(textureSample(src, samp, in.uv + vec2<f32>(o.x, o.y)).rgb);
    return vec4<f32>(sum * 0.25, 1.0);
}
