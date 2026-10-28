// evani.shader.pseudo3d
// Pseudo-3D road (feature `pseudo3d`): the road view's quads (sky,
// backdrop, verge, kerbs, tarmac, lane marks and roadside sprites), drawn
// back to front at the start of the sprites pass. One instance per quad,
// six vertices each, no vertex buffer for the quad itself. Corners are CSS
// pixels from the top-left of the canvas; group 0 is the sprite pass's
// (atlas, sampler, viewport), so sprites and solid fills share the atlas.

struct Sprites {
    // Canvas size in CSS pixels.
    viewport: vec2<f32>,
    _pad: vec2<f32>,
};

@group(0) @binding(0) var atlas: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> u: Sprites;

struct VsIn {
    @builtin(vertex_index) vi: u32,
    // Top-left (xy) and top-right (zw) corners.
    @location(0) top: vec4<f32>,
    // Bottom-left (xy) and bottom-right (zw) corners.
    @location(1) bottom: vec4<f32>,
    // Atlas uv rectangle: u0, v0, u1, v1.
    @location(2) uv: vec4<f32>,
    // Linear RGBA at the top and bottom edges.
    @location(3) color_top: vec4<f32>,
    @location(4) color_bottom: vec4<f32>,
    // Fog colour (rgb) and amount (a).
    @location(5) fog: vec4<f32>,
};

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) fog: vec4<f32>,
};

@vertex
fn vs_main(in: VsIn) -> VsOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
    );
    let k = corners[in.vi % 6u];
    let p = mix(mix(in.top.xy, in.top.zw, k.x), mix(in.bottom.xy, in.bottom.zw, k.x), k.y);
    let vp = max(u.viewport, vec2<f32>(1.0));
    var o: VsOut;
    o.pos = vec4<f32>(p.x / vp.x * 2.0 - 1.0, 1.0 - p.y / vp.y * 2.0, 0.0, 1.0);
    o.uv = mix(in.uv.xy, in.uv.zw, k);
    o.color = mix(in.color_top, in.color_bottom, k.y);
    o.fog = in.fog;
    return o;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let c = textureSample(atlas, samp, in.uv) * in.color;
    return vec4<f32>(mix(c.rgb, in.fog.rgb, in.fog.a), c.a);
}
