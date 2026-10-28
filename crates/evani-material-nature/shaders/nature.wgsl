// evani.shader.nature
// Nature materials without a screen copy (every tier but High and Ultra):
// the same shader draws `nature_flat` and `nature_flat_add` (Lite/Low: calm
// water and glass, a tint multiplied into hdr, then the sky and the water's
// body colour added), `nature_mul` (what is behind each surface,
// multiplied: depth fade, glass tint, foam cover, leaf colour variation)
// and `nature_add` (light added: Fresnel sky, sun glint, foam, the
// back-lit glow of leaves, grass, ears and lampshades). The shared body
// follows (nature_common.wgsl).

@group(0) @binding(0) var<uniform> u: Nature;
@group(0) @binding(1) var depth_tex: texture_depth_2d;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let z = textureLoad(depth_tex, vec2<i32>(in.pos.xy), 0);
    let mode = i32(u.mode + 0.5);
    let s = shade(in.uv, z, mode == 0 || mode == 4);
    if (mode == 2 || mode == 4) {
        return vec4<f32>(s.a, 1.0);
    }
    return vec4<f32>(s.m, 1.0);
}

