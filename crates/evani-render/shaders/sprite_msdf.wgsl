// evani.shader.sprite_msdf
// Sprite pass plus MSDF glyphs (font atlas in group 1; see msdf.rs).
// rot.yzw of a glyph: pxrange (> 0 marks it), mode, width.

struct Sprites {
    // Canvas size in CSS pixels.
    viewport: vec2<f32>,
    _pad: vec2<f32>,
};

@group(0) @binding(0) var atlas: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> u: Sprites;
@group(1) @binding(0) var font_atlas: texture_2d<f32>;
@group(1) @binding(1) var font_samp: sampler;

struct VsIn {
    @builtin(vertex_index) vi: u32,
    // Centre (xy) and size (zw), CSS pixels.
    @location(0) rect: vec4<f32>,
    // Atlas uv rectangle: u0, v0, u1, v1.
    @location(1) uv: vec4<f32>,
    // Linear RGBA tint.
    @location(2) color: vec4<f32>,
    // x: rotation in radians, clockwise on screen; yzw: glyph parameters.
    @location(3) rot: vec4<f32>,
};

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) glyph: vec3<f32>,
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
    o.glyph = in.rot.yzw;
    return o;
}

fn median3(v: vec3<f32>) -> f32 {
    return max(min(v.r, v.g), min(max(v.r, v.g), v.b));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Sprite art: a pixel-art frame (uv moved by 2.0 on u) reads level 0 on
    // the texel centre, every other frame filters normally.
    let pixel = in.uv.x >= 2.0;
    let uv = select(in.uv, in.uv - vec2<f32>(2.0, 0.0), pixel);
    let soft = textureSample(atlas, samp, uv);
    let size = vec2<f32>(textureDimensions(atlas, 0));
    let crisp = textureSampleLevel(atlas, samp, (floor(uv * size) + 0.5) / size, 0.0);
    let art = select(soft, crisp, pixel) * in.color;

    // MSDF glyph: distance 0.5 is the edge, one distance range is pxrange
    // atlas pixels; screen pixels per range from the uv derivatives.
    let mtsdf = textureSample(font_atlas, font_samp, in.uv);
    let font_px = vec2<f32>(textureDimensions(font_atlas, 0));
    let unit = vec2<f32>(in.glyph.x) / font_px;
    let screen = vec2<f32>(1.0) / max(fwidth(in.uv), vec2<f32>(1e-6));
    let px_range = max(0.5 * dot(unit, screen), 1.0);
    let fill = clamp(px_range * (median3(mtsdf.rgb) - 0.5) + 0.5, 0.0, 1.0);
    var outline = clamp(px_range * (mtsdf.a - 0.5 + in.glyph.z) + 0.5, 0.0, 1.0);
    let halo = smoothstep(0.5 - in.glyph.z, 0.5, mtsdf.a);
    let mode = in.glyph.y;
    // Outline pinholes: a counter just wider than the outline leaves a hole
    // finer than the atlas can place (or than a screen pixel shows) as a
    // speck. Close it: a pixel counts as outline when the outline covers it
    // on both sides along any of four directions within r, the larger of
    // half an atlas texel and 0.75 screen pixels (holes under about 1.5 px
    // wide, or 1 texel). Open counters and outer edges are left alone.
    let r = max(0.75 * fwidth(in.uv), vec2<f32>(0.5) / font_px);
    if (mode > 0.5 && mode < 1.5 && outline < 1.0) {
        var dirs = array<vec2<f32>, 4>(vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0), vec2<f32>(0.7071, 0.7071), vec2<f32>(0.7071, -0.7071));
        for (var i = 0; i < 4; i = i + 1) {
            let d = dirs[i] * r;
            let a0 = textureSampleLevel(font_atlas, font_samp, in.uv + d, 0.0).a;
            let a1 = textureSampleLevel(font_atlas, font_samp, in.uv - d, 0.0).a;
            let both = min(a0, a1);
            outline = max(outline, clamp(px_range * (both - 0.5 + in.glyph.z) + 0.5, 0.0, 1.0));
        }
    }
    let cover = select(select(fill, outline, mode > 0.5), halo, mode > 1.5);
    let text = vec4<f32>(in.color.rgb, in.color.a * cover);
    // Mode 3 is a plain rectangle (box fills): the tint, hard edges.
    return select(select(art, text, in.glyph.x > 0.0), in.color, mode > 2.5);
}
