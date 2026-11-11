// evani.shader.nature_sea
// The spectral sea (the `spectral-sea` part), every level: Low takes the
// flat path (no wave heights, the waves' light only).
// `nature_sea_copy` has copied hdr into `nature.sea.scene`; this draw gives
// every pixel the copy seen through the sea (bent by the waves, darkened by
// the water, with caustics on the bed) plus the light the sea adds (sky,
// sun, glitter, crest glow, foam, spray). Pixels the sea doesn't cover keep
// the copy. The shared body follows (sea_common.wgsl, then sea_table.wgsl).

@group(0) @binding(0) var scene_tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> u: Sea;
@group(0) @binding(3) var depth_tex: texture_depth_2d;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    setup();
    let px = vec2<i32>(in.pos.xy);
    let z = textureLoad(depth_tex, px, 0);
    var s: Shade;
    if (eye_under()) {
        s = under(in.uv, z);
    } else {
        s = sea_shade(in.uv, z, quality() == 0);
    }
    var uv = in.uv;
    if (s.hit && s.t > 0.0) {
        // Bend only onto what lies behind the sea surface, and only where
        // about as much water lies in front of it as in front of this
        // pixel's own scene: the darkening (s.m) and the water's glow were
        // worked out for that path, so a bent sample past an object's edge
        // (the sky or a far bed behind a hull or a rock) would show through
        // undarkened as a bright rim. All four texels the filtered read
        // blends must pass, or an object's outline blends in the sky.
        let size = vec2<f32>(textureDimensions(depth_tex));
        let bent = clamp(in.uv + s.off, vec2<f32>(0.0), vec2<f32>(1.0));
        let p0 = pixel(in.uv, z).ts - s.t;
        let b0 = vec2<i32>(floor(bent * size - 0.5));
        let hi = vec2<i32>(size) - 1;
        var fits = true;
        for (var k = 0; k < 4; k = k + 1) {
            let q = clamp(b0 + vec2<i32>(k & 1, k >> 1u), vec2<i32>(0), hi);
            let pb = pixel((vec2<f32>(q) + 0.5) / size, textureLoad(depth_tex, q, 0)).ts - s.t;
            fits = fits && pb > s.t * 0.02 && abs(pb - p0) < 0.35 * p0 + 0.5;
        }
        if (fits) {
            uv = bent;
        }
    }
    let c = textureSampleLevel(scene_tex, samp, uv, 0.0).rgb;
    return vec4<f32>(c * s.m * s.caustic + s.a, 1.0);
}
