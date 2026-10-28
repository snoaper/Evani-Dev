// evani.shader.tonemap
// Tonemap pass (`tonemap`): HDR `hdr` -> display-referred `ldr`.
// colour * render.exposure * slot_exposure(luminance), then slot_tonemap,
// then render.saturation. Both slot bodies are spliced in at the markers;
// the exposure slot gets a mid-grey placeholder luminance until a
// luminance-average pass exists.

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

struct Tonemap {
    // render.exposure.
    exposure: f32,
    // render.saturation (1 = unchanged).
    saturation: f32,
    _pad0: f32,
    _pad1: f32,
};

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> u: Tonemap;

//@slot exposure
//@slot tonemap

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Screen-door fades (the forward pass drops pixels of a faded instance on
    // a 2x2 pattern and marks the ones it keeps with alpha 0): round a marked
    // pixel, a 3x3 tent filter (four bilinear taps on the pixel's corners)
    // turns any 2x2 pattern into an even blend, so a fading kart reads as
    // see-through instead of dotted. Elsewhere the pixel is left sharp.
    let h = 0.5 / vec2<f32>(textureDimensions(src));
    let t0 = textureSample(src, samp, in.uv + vec2<f32>(-h.x, -h.y));
    let t1 = textureSample(src, samp, in.uv + vec2<f32>(h.x, -h.y));
    let t2 = textureSample(src, samp, in.uv + vec2<f32>(-h.x, h.y));
    let t3 = textureSample(src, samp, in.uv + vec2<f32>(h.x, h.y));
    let centre = textureSample(src, samp, in.uv);
    let marked = min(min(t0.a, t1.a), min(t2.a, t3.a)) < 0.999;
    let hdr = select(centre.rgb, (t0.rgb + t1.rgb + t2.rgb + t3.rgb) * 0.25, marked);
    let exposed = max(hdr, vec3<f32>(0.0)) * u.exposure * slot_exposure(0.18);
    let mapped = slot_tonemap(exposed);
    let l = dot(mapped, vec3<f32>(0.2126, 0.7152, 0.0722));
    let c = max(mix(vec3<f32>(l), mapped, u.saturation), vec3<f32>(0.0));
    return vec4<f32>(c, 1.0);
}
