// evani.shader.decals
// Decals (one pass, `decals`): marks projected onto whatever the scene depth
// shows inside each decal's reach. Rebuilds the world position of each pixel
// from the camera basis and the depth, measures its distance to the decal's
// segment (a == b for a round spot) and multiplies the mark's colour into
// `hdr` (dst * src) before bloom and tonemap, so it darkens lit surfaces the
// way a scorch, a bullet hole or a tyre mark does. Four decals per draw.

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

struct Decal {
    // Segment start (the spot for a round decal), reach in metres.
    a: vec3<f32>,
    radius: f32,
    // Segment end, kind: 0 scorch, 1 hole, 2 skid.
    b: vec3<f32>,
    kind: f32,
    // Colour the surface is multiplied towards, and strength (0 = unused).
    color: vec4<f32>,
};

struct Decals {
    // Camera position; 1 / view distance = inv.x + inv.y * depth.
    eye: vec3<f32>,
    inv0: f32,
    // Camera axes: ray = fwd + right * ndc.x + up * ndc.y (right and up are
    // scaled by the field of view and aspect), world = eye + ray * distance.
    fwd: vec3<f32>,
    inv1: f32,
    right: vec3<f32>,
    // fx.decals.grit.
    grit: f32,
    up: vec3<f32>,
    // fx.decals.softness.
    soft: f32,
    d: array<Decal, 4>,
};

@group(0) @binding(0) var<uniform> u: Decals;
@group(0) @binding(1) var depth_tex: texture_depth_2d;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    var m = vec3<f32>(1.0);
    let z = textureLoad(depth_tex, vec2<i32>(in.pos.xy), 0);
    // Reversed depth: 0 is the far plane (the sky), which no decal marks.
    if (z <= 0.0) {
        return vec4<f32>(m, 1.0);
    }
    let dist = 1.0 / max(u.inv0 + u.inv1 * z, 1e-12);
    let ndc = vec2<f32>(in.uv.x * 2.0 - 1.0, 1.0 - in.uv.y * 2.0);
    let w = u.eye + (u.fwd + u.right * ndc.x + u.up * ndc.y) * dist;
    // Cheap per-cell noise to break up the edges.
    let n = fract(sin(dot(floor(w * 9.0), vec3<f32>(12.9898, 78.233, 37.719))) * 43758.5453);
    for (var i = 0; i < 4; i = i + 1) {
        let c = u.d[i];
        if (c.color.a <= 0.0) {
            continue;
        }
        let ab = c.b - c.a;
        let t = clamp(dot(w - c.a, ab) / max(dot(ab, ab), 1e-6), 0.0, 1.0);
        let d = length(w - (c.a + ab * t)) / max(c.radius * (1.0 - u.grit * 0.4 * n), 1e-4);
        var s = 1.0 - smoothstep(1.0 - u.soft, 1.0, d);
        if (c.kind > 1.5) {
            // Skid: streaky.
            s = s * (0.8 + 0.2 * n);
        } else if (c.kind > 0.5) {
            // Hole: a dark core inside a faint scorch ring.
            s = max(1.0 - smoothstep(0.25, 0.4, d), 0.4 * s);
        }
        m = m * mix(vec3<f32>(1.0), c.color.rgb, s * c.color.a);
    }
    return vec4<f32>(m, 1.0);
}
