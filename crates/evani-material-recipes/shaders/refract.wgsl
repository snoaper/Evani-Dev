// evani.shader.recipes.refract
// Screen-space refraction for frosted, wavy and block glass (batch 3 build
// plan 3.5, B3-P3; feature `refract`, the glass pipeline only). Written fresh
// for Evani.Dev; the shape (a copy of the opaque colour with a few mips, a
// thin-glass offset, the mip picked from roughness, tint by absorption) is
// the usual one (three.js transmission, Filament's thin model, Babylon.js:
// ideas only).
//
// The copy (group 2) is the opaque colour at half resolution with three
// mips, made after the opaque draws, on every tier (so a phone sees the same
// frosting as a PC). Glass does not see other refracting glass: it is drawn
// after the copy.
//   offset: the recipe's bent normal minus the face's (waviness, ripples,
//           a block's lumps), times a thin pane's thickness, projected to the
//           screen; the live strength rides in bits 16-23 of the off-bits word
//           (percent, material.recipe.blend.refraction)
//   blur:   the mip from roughness (frost), with four taps a mip texel apart,
//           so a strong frost reads soft rather than blocky
//   tint:   what comes through is the copy times tint^(0.5 + 2 thickness)
//           (exp(-absorption x thickness) with absorption = -ln(tint)) times
//           what the milky body lets through (1 - alpha); the lit body,
//           reflections and highlights (c, premultiplied) go on top.

@group(2) @binding(0) var rc_refr: texture_2d<f32>;
@group(2) @binding(1) var rc_refr_s: sampler;

fn mat_refract(c: vec3<f32>, o: MatOut, a: f32, pos: vec4<f32>, g: vec3<f32>, world: vec3<f32>) -> vec4<f32> {
    let b = rc_unpack(rc_mat);
    let k = f32((rc_frame.misc.z >> 16u) & 255u) / 100.0;
    let th = b.na.x;
    let half = vec2<f32>(textureDimensions(rc_refr, 0));
    let d = (o.n - g) * ((0.015 + 0.05 * th) * k);
    let p0 = frame.view_proj * vec4<f32>(world, 1.0);
    let p1 = frame.view_proj * vec4<f32>(world + d, 1.0);
    let uv = pos.xy / (half * 2.0) + (p1.xy / p1.w - p0.xy / p0.w) * vec2<f32>(0.5, -0.5);
    let lod = clamp((o.rough - 0.04) * 3.0, 0.0, 2.0);
    let r = exp2(lod) * (0.5 + lod * 0.35) / half;
    var s = textureSampleLevel(rc_refr, rc_refr_s, uv + vec2<f32>(r.x, r.y * 0.4), lod).rgb;
    s = s + textureSampleLevel(rc_refr, rc_refr_s, uv - vec2<f32>(r.x, r.y * 0.4), lod).rgb;
    s = s + textureSampleLevel(rc_refr, rc_refr_s, uv + vec2<f32>(-r.x * 0.4, r.y), lod).rgb;
    s = s + textureSampleLevel(rc_refr, rc_refr_s, uv + vec2<f32>(r.x * 0.4, -r.y), lod).rgb;
    let tint = pow(max(b.col_a, vec3<f32>(1e-3)), vec3<f32>(0.5 + 2.0 * th));
    return vec4<f32>(c + s * (0.25 * (1.0 - a)) * tint, 1.0);
}
