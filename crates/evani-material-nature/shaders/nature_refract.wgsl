// evani.shader.nature_refract
// Nature materials with a screen copy (High and Ultra, when
// `material.nature.refraction` is on): `nature_copy` has copied hdr into
// `nature.scene`; this draw replaces each covered pixel with the copy seen
// through the surfaces (bent by the water's waves and a bottle's curve),
// times what they let through, plus the light they add. Pixels no surface
// covers are discarded, so hdr keeps them. The shared body follows
// (nature_common.wgsl).

@group(0) @binding(0) var scene_tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> u: Nature;
@group(0) @binding(3) var depth_tex: texture_depth_2d;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let px = vec2<i32>(in.pos.xy);
    let z = textureLoad(depth_tex, px, 0);
    let s = shade(in.uv, z, false);
    if (!s.hit) {
        discard;
    }
    var uv = in.uv;
    if (s.tnear < FAR_T) {
        // Bend only onto what lies behind the surface, never onto something
        // in front of it.
        let size = vec2<f32>(textureDimensions(depth_tex));
        let bent = clamp(in.uv + s.off, vec2<f32>(0.0), vec2<f32>(1.0));
        let zb = textureLoad(depth_tex, vec2<i32>(bent * size), 0);
        if (scene_dist(zb) > s.tnear) {
            uv = bent;
        }
    }
    let c = textureSampleLevel(scene_tex, samp, uv, 0.0).rgb;
    return vec4<f32>(c * s.m + s.a, 1.0);
}

