// evani.shader.nature_sea_quick
// The spectral sea's stand-in: the flat sea at its level with the sea's
// colour, the sky in it and the sun's glint, nothing else. A tiny shader,
// so it is ready almost at once; it draws every frame under the full sea
// (which replaces every pixel once its own pipeline is built), so a game
// shows water from its first frames instead of a bare bed while the full
// sea compiles. Pixels the flat sea doesn't cover are left alone.

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

// The sea's uniform words (the same layout as sea_common.wgsl's `Sea`).
struct Sea {
    eye: vec3<f32>,
    inv0: f32,
    fwd: vec3<f32>,
    inv1: f32,
    right: vec3<f32>,
    tl: f32,
    up: vec3<f32>,
    t: f32,
    sun: vec3<f32>,
    px: f32,
    sun_col: vec3<f32>,
    sky_tint: f32,
    centre: vec3<f32>,
    level: f32,
    half: vec2<f32>,
    v_unres: f32,
    spray: f32,
    wind: vec4<f32>,
    swell: vec4<f32>,
    shape: vec4<f32>,
    cols: vec4<f32>,
    glow: vec4<f32>,
    light: vec4<f32>,
    foam: vec4<f32>,
    beach: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Sea;
@group(0) @binding(1) var depth_tex: texture_depth_2d;

fn col(p: f32) -> vec3<f32> {
    let q = u32(max(p, 0.0) + 0.5);
    let v = vec3<f32>(f32(q & 255u), f32((q >> 8u) & 255u), f32((q >> 16u) & 255u)) / 255.0;
    return v * v;
}

fn tint() -> vec3<f32> {
    let p = u32(max(u.sky_tint, 0.0) + 0.5);
    return vec3<f32>(f32(p & 255u), f32((p >> 8u) & 255u), f32((p >> 16u) & 255u)) * (4.0 / 255.0);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let z = textureLoad(depth_tex, vec2<i32>(in.pos.xy), 0);
    let ndc = vec2<f32>(in.uv.x * 2.0 - 1.0, 1.0 - in.uv.y * 2.0);
    let ray = u.fwd + u.right * ndc.x + u.up * ndc.y;
    let rl = length(ray);
    let rd = ray / rl;
    if (u.eye.y <= u.centre.y || rd.y > -1e-4) {
        discard;
    }
    let t = (u.centre.y - u.eye.y) / rd.y;
    // The scene in front of the flat sea keeps its pixel.
    if (z > 0.0 && t > 1.0 / max(u.inv0 + u.inv1 * z, 1e-12) * rl) {
        discard;
    }
    let p = u.eye + rd * t;
    let l = abs(p.xz - u.centre.xz);
    if (l.x > u.half.x || l.y > u.half.y) {
        discard;
    }
    let sky_r = mix(col(u.cols.x), col(u.cols.y), sqrt(clamp(-rd.y, 0.0, 1.0))) * tint();
    let nov = clamp(-rd.y, 0.0, 1.0);
    let f = 0.02 + 0.98 * pow(1.0 - nov, 5.0) * 0.7;
    let body = col(u.cols.w) * (col(u.cols.y) * 0.6 + u.sun_col * max(u.sun.y, 0.0) * 0.6);
    let h = normalize(-rd + u.sun);
    let glint = u.sun_col * pow(max(h.y, 0.0), 300.0) * 2.0 * u.light.x;
    let haze = 1.0 - exp(-t / 9000.0);
    let c = mix(body * (1.0 - f) + sky_r * f + glint, col(u.cols.x) * tint(), haze);
    return vec4<f32>(c, 1.0);
}
