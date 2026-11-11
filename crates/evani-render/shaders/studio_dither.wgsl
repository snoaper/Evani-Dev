// evani.shader.studio_dither
// The studio's engraving pass (feature `studio-passes`, pass `studio.apply`,
// mode `dither`): the Monochrome Dither / Engraving style in two tones, ink
// and paper, done on the screen so it reads the same on any mesh (cut per
// triangle, a dense model broke up into random blotches). The shade decides
// how much ink: light areas stay paper, mid tones get one set of diagonal
// lines whose width follows the shade, darker ones a crossing set, and the
// darkest a fine ordered dither between them. Lines are fixed to the screen
// at `period` pixels, so they stay crisp at any distance. The sky is paper.

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

struct Dither {
    size: vec2<f32>,
    inv: vec2<f32>,
    // Ink colour and the line period in pixels.
    ink: vec3<f32>,
    period: f32,
    // Paper colour and the tone lift (higher = lighter overall).
    paper: vec3<f32>,
    lift: f32,
};

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> u: Dither;
@group(0) @binding(3) var depth_tex: texture_depth_2d;

fn texel(p: vec2<f32>) -> vec2<i32> {
    return clamp(vec2<i32>(floor(p)), vec2<i32>(0), vec2<i32>(u.size) - vec2<i32>(1));
}

// 1 where ink goes for a line set at angle `dir` covering `cover` of it.
fn lines(p: vec2<f32>, dir: vec2<f32>, cover: f32) -> f32 {
    let t = fract(dot(p, dir) / u.period);
    return select(0.0, 1.0, abs(t - 0.5) < cover * 0.5);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let p = in.pos.xy;
    if (textureLoad(depth_tex, texel(p), 0) <= 0.0) {
        return vec4<f32>(u.paper, 1.0);
    }
    let c = textureLoad(src, texel(p), 0).rgb;
    let shade = clamp(dot(c, vec3<f32>(0.3, 0.59, 0.11)) * u.lift, 0.0, 1.0);
    let dark = 1.0 - shade;
    var ink = lines(p, vec2<f32>(0.7071, 0.7071), clamp((dark - 0.2) * 1.4, 0.0, 0.8));
    ink = max(ink, lines(p, vec2<f32>(0.7071, -0.7071), clamp((dark - 0.55) * 1.6, 0.0, 0.7)));
    // The deepest shade fills in with a fine 2x2 dither.
    let q = vec2<i32>(p) & vec2<i32>(1);
    let b = f32((q.x * 2 + q.y * 3) & 3) / 4.0 + 0.125;
    ink = max(ink, select(0.0, 1.0, dark > 0.82 + 0.15 * b));
    return vec4<f32>(mix(u.paper, u.ink, ink), 1.0);
}
