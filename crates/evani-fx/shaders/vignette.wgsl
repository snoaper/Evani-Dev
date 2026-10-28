// evani.shader.vignette
// Vignette (one pass, `vignette`): darken towards the screen edges. Runs on
// the tonemapped `ldr` image with multiplicative blending (dst * src), so it
// never reads the target it writes.

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

struct Vignette {
    // fx.vignette.color (linear RGB the edges fade towards).
    color: vec3<f32>,
    // fx.vignette.intensity.
    intensity: f32,
    // fx.vignette.radius: distance from the centre where darkening starts.
    radius: f32,
    // fx.vignette.softness: width of the fade.
    softness: f32,
    // fx.vignette.roundness: 0 = follows the screen shape, 1 = a circle.
    roundness: f32,
    // Render target width / height (set by the renderer).
    aspect: f32,
};

@group(0) @binding(0) var<uniform> u: Vignette;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    var d = in.uv - vec2<f32>(0.5);
    d.x = d.x * mix(1.0, u.aspect, u.roundness);
    let dist = length(d) * 2.0;
    let edge = smoothstep(u.radius, u.radius + max(u.softness, 1e-4), dist);
    let k = edge * u.intensity;
    return vec4<f32>(mix(vec3<f32>(1.0), u.color, k), 1.0);
}
