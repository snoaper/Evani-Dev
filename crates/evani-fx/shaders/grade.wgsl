// evani.shader.grade
// Colour grade (pass `grade.apply`, and the same shader copies the result
// back in `grade.back` with `copy` = 1): samples the tonemapped `ldr` image
// and writes it graded into `grade.tmp`. The grade is the crossfaded pair of
// presets the CPU mixed into this one uniform: gain and warmth in linear
// light, then contrast, saturation, split tone and black lift in a
// perceptual (gamma 2.2) space. `amount` blends graded with original.

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

struct Grade {
    // Shadow tint (white = none) and gain.
    shadow: vec3<f32>,
    gain: f32,
    // Highlight tint and warmth (+ warmer, - cooler).
    highlight: vec3<f32>,
    warmth: f32,
    contrast: f32,
    saturation: f32,
    tone: f32,
    lift: f32,
    // fx.grade.amount, and 1 for the plain copy-back draw.
    amount: f32,
    is_copy: f32,
    pad0: f32,
    pad1: f32,
};

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> u: Grade;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let c0 = textureSample(src, samp, in.uv).rgb;
    if (u.is_copy > 0.5) {
        return vec4<f32>(c0, 1.0);
    }
    var c = c0 * u.gain * vec3<f32>(1.0 + u.warmth, 1.0, 1.0 - u.warmth);
    var g = pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.2));
    g = (g - 0.5) * u.contrast + 0.5;
    let l = dot(g, vec3<f32>(0.2126, 0.7152, 0.0722));
    g = mix(vec3<f32>(l), g, u.saturation);
    g = g * mix(vec3<f32>(1.0), mix(u.shadow, u.highlight, smoothstep(0.1, 0.9, l)), u.tone);
    g = g + u.lift * (vec3<f32>(1.0) - g);
    c = pow(clamp(g, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(2.2));
    return vec4<f32>(mix(c0, c, u.amount), 1.0);
}
