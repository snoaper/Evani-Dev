// evani.shader.studio_paper
// The studio's paper pass (feature `studio-passes`, pass `studio.apply`,
// mode `paper`): Watercolour, Ink, Gothic and Storybook painted on paper.
//
// * Wobble: the image is read through a slow screen-fixed noise offset, so
//   straight edges waver like a hand-drawn line (`bleed` scales it).
// * Bleed: a ring of samples around the point is mixed in, so washes run a
//   little past their edges and into each other.
// * Edge darkening: where the colour changes inside the ring (the edge of a
//   wash) or the depth jumps (the edge of a shape), pigment pools: darker,
//   a touch more saturated (`edge`).
// * Paper: the sky becomes paper, and a two-scale grain fixed to the sheet
//   (not the object) lightens the tooth and settles pigment in the valleys
//   (`grain`), more in dark washes than in light ones.

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

struct Paper {
    size: vec2<f32>,
    inv: vec2<f32>,
    grain: f32,
    edge: f32,
    bleed: f32,
    pad0: f32,
    // The paper's colour (display values).
    paper: vec3<f32>,
    pad1: f32,
};

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> u: Paper;
@group(0) @binding(3) var depth_tex: texture_depth_2d;

fn texel(p: vec2<f32>) -> vec2<i32> {
    return clamp(vec2<i32>(floor(p)), vec2<i32>(0), vec2<i32>(u.size) - vec2<i32>(1));
}

fn sky(p: vec2<f32>) -> bool {
    return textureLoad(depth_tex, texel(p), 0) <= 0.0;
}

fn dist(p: vec2<f32>) -> f32 {
    let d = textureLoad(depth_tex, texel(p), 0);
    return 1.0 / max(u.inv.x + u.inv.y * max(d, 1e-7), 1e-9);
}

fn hash(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2<f32>(0.1031, 0.1030));
    let r = q + dot(q, q.yx + 33.33);
    return fract((r.x + r.y) * r.x);
}

fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let w = f * f * (3.0 - 2.0 * f);
    let a = hash(i);
    let b = hash(i + vec2<f32>(1.0, 0.0));
    let c = hash(i + vec2<f32>(0.0, 1.0));
    let d = hash(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, w.x), mix(c, d, w.x), w.y);
}

// The colour at a screen point, sky replaced by paper.
fn wash(p: vec2<f32>) -> vec3<f32> {
    let c = textureSampleLevel(src, samp, p / u.size, 0.0).rgb;
    return select(c, mix(c, u.paper, 0.85), sky(p));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let p = in.pos.xy;
    let wob = vec2<f32>(vnoise(p / 23.0), vnoise(p / 23.0 + vec2<f32>(17.0, 5.0))) - 0.5;
    let q = p + wob * (1.0 + 5.0 * u.bleed);
    let c0 = wash(q);
    // A ring of 8 samples, its radius from the bleed.
    let r = 1.5 + 5.0 * u.bleed;
    var ring = vec3<f32>(0.0);
    var jump = 0.0;
    let z0 = dist(q);
    for (var k = 0u; k < 8u; k = k + 1u) {
        let a = f32(k) * 0.785398 + 0.39;
        let o = q + vec2<f32>(cos(a), sin(a)) * r;
        ring = ring + wash(o);
        let z = dist(o);
        jump = max(jump, abs(z - z0) / max(min(z, z0), 0.01));
    }
    ring = ring / 8.0;
    var c = mix(c0, ring, 0.55 * u.bleed);
    // Pigment pools at the edge of a wash and of a shape.
    let change = clamp(length(c0 - ring) * 3.0, 0.0, 1.0);
    let rim = clamp((jump - 0.05) * 4.0, 0.0, 1.0);
    let pool = u.edge * max(change * 0.55, rim * 0.45);
    let l = dot(c, vec3<f32>(0.3, 0.59, 0.11));
    c = mix(c, (c - vec3<f32>(l)) * 1.25 + vec3<f32>(l) * 0.55, pool);
    // Paper grain fixed to the sheet.
    let g = vnoise(p / 1.7) * 0.5 + vnoise(p / 7.0) * 0.35 + vnoise(p / 31.0) * 0.15;
    let dark = 1.0 - clamp(dot(c, vec3<f32>(0.3, 0.59, 0.11)), 0.0, 1.0);
    c = c * (1.0 - u.grain * (0.1 + 0.22 * dark) * (g - 0.35));
    c = mix(c, u.paper, u.grain * 0.06 * (1.0 - g));
    return vec4<f32>(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
