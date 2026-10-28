// evani.shader.sprite_lights2d
// Sprite pass with 2D dynamic lights (feature `lights2d`, catalog effect
// `lights2d`; replaces sprite.wgsl + sprite_fs_art.wgsl). The uniform is the
// light buffer: up to 7 point lights, summed per pixel. A sprite's rot.yzw
// (filled by `lights2d::mark`) say whether the lights reach it: x 0 unlit
// (drawn exactly as without lights), -1 flat-facing, -2 normal-mapped with
// yz the uv offset from its frame to the normal map frame.

struct Light {
    // Centre (CSS px), radius (CSS px), height above the sprites (CSS px).
    a: vec4<f32>,
    // Linear RGB times intensity.
    b: vec4<f32>,
};

struct Sprites {
    // Canvas size in CSS pixels.
    viewport: vec2<f32>,
    count: f32,
    normal_strength: f32,
    // Ambient light (linear RGB, level folded in); w: rim strength.
    ambient: vec4<f32>,
    lights: array<Light, 7>,
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
    // x: rotation in radians, clockwise on screen; yzw: light mark.
    @location(3) rot: vec4<f32>,
};

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    // Position in CSS pixels.
    @location(2) css: vec2<f32>,
    @location(3) @interpolate(flat) mark: vec3<f32>,
    // cos and sin of the rotation.
    @location(4) @interpolate(flat) turn: vec2<f32>,
};

@vertex
fn vs_main(in: VsIn) -> VsOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
    );
    let k = corners[in.vi % 6u];
    let local = (k - vec2<f32>(0.5)) * in.rect.zw;
    let c = cos(in.rot.x);
    let s = sin(in.rot.x);
    let p = in.rect.xy + vec2<f32>(local.x * c - local.y * s, local.x * s + local.y * c);
    let vp = max(u.viewport, vec2<f32>(1.0));
    var o: VsOut;
    o.pos = vec4<f32>(p.x / vp.x * 2.0 - 1.0, 1.0 - p.y / vp.y * 2.0, 0.0, 1.0);
    o.uv = mix(in.uv.xy, in.uv.zw, k);
    o.color = in.color;
    o.css = p;
    o.mark = vec3<f32>(-in.rot.y, in.rot.z, in.rot.w);
    o.turn = vec2<f32>(c, s);
    return o;
}

// The art fragment read (as sprite_fs_art.wgsl): pixel-art frames (u moved
// by 2.0) at level 0 on texel centres, the rest filtered.
fn texel(uv_in: vec2<f32>) -> vec4<f32> {
    let pixel = uv_in.x >= 2.0;
    let uv = select(uv_in, uv_in - vec2<f32>(2.0, 0.0), pixel);
    let soft = textureSample(atlas, samp, uv);
    let size = vec2<f32>(textureDimensions(atlas, 0));
    let crisp = textureSampleLevel(atlas, samp, (floor(uv * size) + 0.5) / size, 0.0);
    return select(soft, crisp, pixel);
}

// Normal maps are stored as raw bytes in the sRGB atlas: undo the decode.
fn raw_bytes(c: vec3<f32>) -> vec3<f32> {
    let hi = 1.055 * pow(max(c, vec3<f32>(0.0031308)), vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, c * 12.92, c <= vec3<f32>(0.0031308));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let base = texel(in.uv) * in.color;
    let nm = raw_bytes(texel(in.uv + in.mark.yz).rgb) * 2.0 - 1.0;
    if (in.mark.x < 0.5) {
        return base;
    }
    // Map space: x right, y up; the screen's y points down. The normal
    // turns with the sprite.
    let t = vec2<f32>(nm.x, -nm.y) * u.normal_strength;
    let r = vec2<f32>(t.x * in.turn.x - t.y * in.turn.y, t.x * in.turn.y + t.y * in.turn.x);
    let n = select(vec3<f32>(0.0, 0.0, 1.0), normalize(vec3<f32>(r, max(nm.z, 0.05))), in.mark.x > 1.5);
    let rim = (1.0 - n.z) * (1.0 - n.z) * u.ambient.w;
    var light = u.ambient.rgb;
    let count = min(u32(u.count), 7u);
    for (var i = 0u; i < count; i++) {
        let l = u.lights[i];
        let d = vec3<f32>(l.a.xy - in.css, l.a.w);
        let fall = saturate(1.0 - length(d.xy) / max(l.a.z, 1.0));
        light += l.b.rgb * (fall * fall) * (max(dot(n, normalize(d)), 0.0) + rim);
    }
    return vec4<f32>(base.rgb * light, base.a);
}
