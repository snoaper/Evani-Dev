// evani.shader.sprite
// Sprite pass (`sprites`): instanced screen-space quads from the 4x4 sprite
// atlas, alpha-blended over the 3D scene in `hdr`. Six vertices per
// instance, no vertex buffer for the quad itself. Positions are CSS pixels
// from the top-left of the canvas, so sprites keep their size when the
// render scale or DPR changes.

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
    // Centre (xy) and size (zw), CSS pixels.
    @location(0) rect: vec4<f32>,
    // Atlas uv rectangle: u0, v0, u1, v1.
    @location(1) uv: vec4<f32>,
    // Linear RGBA tint.
    @location(2) color: vec4<f32>,
    // x: rotation in radians, clockwise on screen.
    @location(3) rot: vec4<f32>,
};

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(in: VsIn) -> VsOut {
    // Two triangles: corners 0 1 2, 2 1 3 of the unit quad.
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
    );
    let k = corners[in.vi % 6u];
    let local = (k - vec2<f32>(0.5)) * in.rect.zw;
    let c = cos(in.rot.x);
    let s = sin(in.rot.x);
    // y points down on screen, so this turns clockwise.
    let p = in.rect.xy + vec2<f32>(local.x * c - local.y * s, local.x * s + local.y * c);
    let vp = max(u.viewport, vec2<f32>(1.0));
    var o: VsOut;
    o.pos = vec4<f32>(p.x / vp.x * 2.0 - 1.0, 1.0 - p.y / vp.y * 2.0, 0.0, 1.0);
    o.uv = mix(in.uv.xy, in.uv.zw, k);
    o.color = in.color;
    return o;
}

