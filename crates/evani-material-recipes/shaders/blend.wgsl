// evani.shader.recipes.blend
// The see-through pass's stage (batch 3 build plan 3.5, package B3-P3;
// feature `blend`). Written fresh for Evani.Dev. Appended to the recipe stage
// when the feature is on; two patch edits call it: `mat_premul` where the
// recipe's colour lands (before any lighting) and `mat_blend` around the
// stage's final return.
//
// One module serves the opaque and the blend pipelines: REC_BLEND is a
// pipeline-overridable constant, 0 in the opaque pipeline (every line here
// then returns its input, so a blended material drawn opaque, the light
// version, looks as it did) and 1 in the blend pipeline. The mode is the
// material uniform's free word uv0.w (crate::blend: 1 over, 2 add, 3 refract).
//
// Over (premultiplied alpha): the surface colour is premultiplied by its
// alpha before lighting, so the diffuse body fades with alpha while
// reflections, highlights and emission stay whole (a clear pane still shows
// the sky). Fog fades the body towards the fog colour times alpha, since what
// is behind is fogged already. Add: the lit colour without its fog, times
// alpha, with alpha 0, so the same blend state adds it. Refract: as over, then
// mat_refract composites the bent copy of what is behind (the copy's stage,
// refract.wgsl, in the glass pipeline only; here the stub draws it as over).

override REC_BLEND: u32 = 0u;

fn rc_blend_mode() -> u32 {
    return u32(model_mat.uv0.w + 0.5);
}

fn mat_premul(b: vec4<f32>) -> vec4<f32> {
    let m = rc_blend_mode();
    if (REC_BLEND == 0u || m == 0u || m == 2u) {
        return b;
    }
    return vec4<f32>(b.rgb * b.a, b.a);
}

fn mat_blend(v: vec4<f32>, o: MatOut, fog: f32, pos: vec4<f32>, g: vec3<f32>, world: vec3<f32>) -> vec4<f32> {
    let m = rc_blend_mode();
    if (REC_BLEND == 0u || m == 0u) {
        return v;
    }
    let a = clamp(o.base.a, 0.0, 1.0);
    if (m == 2u) {
        return vec4<f32>(max(v.rgb - frame.fog_color.rgb * fog, vec3<f32>(0.0)) * a, 0.0);
    }
    let c = v.rgb - frame.fog_color.rgb * (fog * (1.0 - a));
    if (m == 3u) {
        return mat_refract(c, o, a, pos, g, world);
    }
    return vec4<f32>(c, a);
}
