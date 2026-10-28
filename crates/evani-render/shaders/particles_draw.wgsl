// evani.shader.particles_draw
// Shared particles, drawn from the instances `particles.sim` wrote (one
// per ring slot, as a per-instance vertex buffer). One instanced draw per
// group, six vertices per instance, corners from the vertex index;
// instances of another group (or dead) are moved off screen. Screen groups
// draw inside the sprites pass between the sprite layers; the world group
// draws inside the forward pass as camera-facing quads, depth-tested
// without writing depth. Premultiplied alpha, so alpha and additive
// particles share the draw; no sorting.

struct U {
    // World: view-projection and eye. Screen: unused.
    vp: mat4x4<f32>,
    eye: vec3<f32>,
    // The group this draw shows.
    key: u32,
    // Screen: canvas size in CSS pixels.
    viewport: vec2<f32>,
    _pad: vec2<f32>,
};

@group(0) @binding(0) var atlas: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> u: U;

struct VsIn {
    @builtin(vertex_index) vi: u32,
    // Centre (xyz) and width.
    @location(0) cw: vec4<f32>,
    // Height, rotation (radians, clockwise on screen), unused.
    @location(1) hr: vec3<f32>,
    @location(2) key: u32,
    @location(3) uv: vec4<f32>,
    // Premultiplied colour.
    @location(4) color: vec4<f32>,
};

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

// Corner of the unit quad (two triangles: 0 1 2, 2 1 3) and its offset
// from the centre, rotated, in the particle's own units (y down).
fn corner(vi: u32) -> vec2<f32> {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
    );
    return corners[vi % 6u];
}

fn offset(k: vec2<f32>, in: VsIn) -> vec2<f32> {
    let local = (k - vec2<f32>(0.5)) * vec2<f32>(in.cw.w, in.hr.x);
    let c = cos(in.hr.y);
    let s = sin(in.hr.y);
    return vec2<f32>(local.x * c - local.y * s, local.x * s + local.y * c);
}

fn culled() -> VsOut {
    var o: VsOut;
    o.pos = vec4<f32>(2.0, 2.0, 2.0, 1.0);
    o.uv = vec2<f32>(0.0);
    o.color = vec4<f32>(0.0);
    return o;
}

@vertex
fn vs_screen(in: VsIn) -> VsOut {
    if (in.key != u.key) {
        return culled();
    }
    let k = corner(in.vi);
    let p = in.cw.xy + offset(k, in);
    let vp = max(u.viewport, vec2<f32>(1.0));
    var o: VsOut;
    o.pos = vec4<f32>(p.x / vp.x * 2.0 - 1.0, 1.0 - p.y / vp.y * 2.0, 0.0, 1.0);
    o.uv = mix(in.uv.xy, in.uv.zw, k);
    o.color = in.color;
    return o;
}

@vertex
fn vs_world(in: VsIn) -> VsOut {
    if (in.key != u.key) {
        return culled();
    }
    let k = corner(in.vi);
    let d = offset(k, in);
    // Face the eye; straight up or down falls back to world x as right.
    let to_eye = u.eye - in.cw.xyz;
    var right = cross(vec3<f32>(0.0, 1.0, 0.0), to_eye);
    if (dot(right, right) < 1e-8) {
        right = vec3<f32>(1.0, 0.0, 0.0);
    }
    right = normalize(right);
    let up = normalize(cross(to_eye, right));
    let p = in.cw.xyz + right * d.x - up * d.y;
    var o: VsOut;
    o.pos = u.vp * vec4<f32>(p, 1.0);
    o.uv = mix(in.uv.xy, in.uv.zw, k);
    o.color = in.color;
    return o;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let t = textureSample(atlas, samp, in.uv);
    return vec4<f32>(in.color.rgb * t.rgb * t.a, in.color.a * t.a);
}
