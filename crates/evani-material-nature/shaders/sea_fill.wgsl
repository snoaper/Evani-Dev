// evani.shader.nature_sea_fill
// The field sea's full-screen part, drawn only when needed: under the water
// (the camera dipped into a wave) the water's colour over everything, and
// over a beach the swash on the sand (the thin sheet, its foam edge and the
// wet line), as the classic sea draws them. Other pixels are left alone;
// the field sea's mesh draws the water itself (sea_field.wgsl).

@group(0) @binding(0) var scene_tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> u: Sea;
@group(0) @binding(3) var depth_tex: texture_depth_2d;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let z = textureLoad(depth_tex, vec2<i32>(in.pos.xy), 0);
    let word = u32(max(u.level, 0.0) + 0.5);
    // src/sea/mod.rs `storm_code`: bit 21, the eye under the water.
    let eye_in = ((word >> 23u) & 1u) == 1u;
    var s: Shade;
    if (eye_in) {
        s = under(in.uv, z);
    } else {
        s = swash_shade(pixel(in.uv, z));
        if (!s.hit) {
            discard;
        }
    }
    let c = textureSampleLevel(scene_tex, samp, in.uv, 0.0).rgb;
    return vec4<f32>(c * s.m + s.a, 1.0);
}
