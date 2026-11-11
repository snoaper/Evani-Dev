// evani.shader.model_cut_shadows
// Shadow-map and AO depth for alpha-masked materials (feature
// `material-recipes-cut` with `shadows`, batch 3 B3-P2): appended to the model
// shadow module (shadows_depth.wgsl's `s` and model_shadows.wgsl's
// ModelShadowIn), then evani-material-recipes' cut.wgsl. One cut-out path:
// the glTF alpha mask. The vertex stage is model_shadow_vs's plus the uv; the
// fragment stage drops what the colour pass drops: the base texture's alpha
// under the material's cutoff (a leaf card, batch 2) and, for a cut-out block,
// the procedural holes (grate, chain link, mesh, lattice: the block, the uv
// transform, the same 2x2 threshold on the shadow map's texels, so the shadow
// filter blends partial cover). So light comes through the holes. Only MASK
// parts use it; every other part keeps the depth-only pipeline.

struct RecRaw {
    w0: vec4<u32>,
    w1: vec4<u32>,
};

// The model material uniform (model.wgsl's ModelMaterial): base alpha, the
// cutoff and the uv transform are read here.
struct CutMat {
    base: vec4<f32>,
    emissive_cutoff: vec4<f32>,
    flags: vec4<f32>,
    uv0: vec4<f32>,
    uv1: vec4<f32>,
};

@group(1) @binding(0) var<uniform> cut_mat: CutMat;
@group(1) @binding(1) var cut_base: texture_2d<f32>;
@group(1) @binding(4) var cut_sampler: sampler;
@group(1) @binding(12) var<uniform> cut_block: RecRaw;
// The recipe frame uniform (core.wgsl's RecFrame) as words: [2].z the off
// bits and the cut-out's min_px.
@group(1) @binding(13) var<uniform> cut_frame: array<vec4<u32>, 8>;

struct CutShadowOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn model_cut_shadow_vs(in: ModelShadowIn, @location(3) uv: vec2<f32>, @location(9) uv_dq: vec4<f32>) -> CutShadowOut {
    var o: CutShadowOut;
    let local = vec4<f32>(in.pos_offset.xyz + in.pos_scale.xyz * in.pos.xyz, 1.0);
    o.pos = s.light_vp * vec4<f32>(dot(in.row0, local), dot(in.row1, local), dot(in.row2, local), 1.0);
    o.uv = uv_dq.zw + uv_dq.xy * uv;
    return o;
}

@fragment
fn model_cut_shadow_fs(in: CutShadowOut) {
    let uv3 = vec3<f32>(in.uv, 1.0);
    let uv = vec2<f32>(dot(cut_mat.uv0.xyz, uv3), dot(cut_mat.uv1.xyz, uv3));
    let dx = dpdx(uv);
    let dy = dpdy(uv);
    let a = textureSample(cut_base, cut_sampler, uv).a * cut_mat.base.a;
    // The alpha mask applies to MASK parts only (B3-P3: see-through parts
    // come here too, and their alpha is the recipe's, not a cutoff).
    if (cut_mat.flags.x > 0.5 && a < cut_mat.emissive_cutoff.w) {
        discard;
    }
    // The cut-out's holes and the see-through stipple (assembled by
    // shaders::model_cut_shadows from what the build has).
    if (rc_shadow_drop(cut_block, uv, dx, dy, in.pos.xy)) {
        discard;
    }
}
