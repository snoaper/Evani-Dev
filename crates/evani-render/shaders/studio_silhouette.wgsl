// evani.shader.studio_silhouette
// The studio's silhouette pass (feature `studio-passes`, pass
// `studio.apply`, mode `silhouette`): shapes as dark cut-outs in fog. The
// sky becomes a fog gradient (`fog_top` to `fog_low`); every drawn surface
// becomes the shape colour, fading into that fog with its distance beyond
// the subject (the depth at the middle of the view), by `fog`. A faint trace
// of the original light stays inside the shape so its parts still read, and
// a thin pale rim lights its outline against what is behind it.

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

struct Silhouette {
    size: vec2<f32>,
    inv: vec2<f32>,
    fog_top: vec3<f32>,
    fog: f32,
    fog_low: vec3<f32>,
    pad0: f32,
    shape: vec3<f32>,
    pad1: f32,
};

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> u: Silhouette;
@group(0) @binding(3) var depth_tex: texture_depth_2d;

const SKY: f32 = 1e6;

fn dist(p: vec2<f32>) -> f32 {
    let t = clamp(vec2<i32>(floor(p)), vec2<i32>(0), vec2<i32>(u.size) - vec2<i32>(1));
    let d = textureLoad(depth_tex, t, 0);
    if (d <= 0.0) {
        return SKY;
    }
    return 1.0 / max(u.inv.x + u.inv.y * d, 1e-9);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let p = in.pos.xy;
    let air = mix(u.fog_top, u.fog_low, smoothstep(0.0, 1.0, in.uv.y));
    let z = dist(p);
    if (z >= SKY) {
        return vec4<f32>(air, 1.0);
    }
    let subject = min(dist(u.size * 0.5), 60.0);
    let far = clamp((z - subject) / max(subject * 2.5, 0.5), 0.0, 1.0);
    let orig = textureLoad(src, vec2<i32>(p), 0).rgb;
    var c = u.shape + orig * 0.06;
    c = mix(c, air, u.fog * far);
    // A pale rim where the shape stands in front of something further away.
    var behind = 0.0;
    for (var k = 0u; k < 4u; k = k + 1u) {
        let a = f32(k) * 1.5708;
        behind = max(behind, min(dist(p + vec2<f32>(cos(a), sin(a)) * 1.5), subject * 4.0) - z);
    }
    let rim = clamp(behind / max(z * 0.15, 0.05) - 0.3, 0.0, 1.0) * (1.0 - far);
    c = mix(c, air, rim * 0.45);
    return vec4<f32>(c, 1.0);
}
