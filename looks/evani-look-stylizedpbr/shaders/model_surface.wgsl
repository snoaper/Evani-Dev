// evani.shader.model_surface
// evani-look-stylizedpbr: smooth stylised-realistic shading for models.
// Appended after model.wgsl (same module: frame, model_mat, the maps and the
// shadow lookup are shared) and drawn with model_lit_vs / model_lit_fs in
// place of model_vs / model_fs. On top of the engine's model shading it:
//   * reads the glTF metallicRoughness map (G roughness, B metal) and the
//     occlusion map (R), scaled by the ModelSurface factors;
//   * lights with the look's smooth toon_ramp body (wrapped diffuse), a
//     roughness/metal specular lobe (slot `specular`), a hemisphere ambient
//     with a cheap environment reflection and the rim slot;
//   * calls detail_sample (biplanar.wgsl or its stub) and fur_sample
//     (fur_surface.wgsl or its stub) before any discard.
// Varyings 0-5 match model.wgsl; 7 and 8 carry the part-local position (m)
// and normal for the tiling layers. Location 6 is never used.

struct ModelSurface {
    // Metal factor, roughness factor, occlusion strength, fur flag (> 0.5).
    pbr: vec4<f32>,
    // Fur: length (m), density (strands per cm^2), shells drawn on this tier
    // (0 = the clump-map fallback, no shells), clump scale (m).
    fur: vec4<f32>,
    // Fur tip colour (linear rgb), detail flag (w > 0.5).
    tip: vec4<f32>,
    // Detail: tile size (m, 0 = the look's), strength, unused, unused.
    detail: vec4<f32>,
};

@group(1) @binding(5) var mr_map: texture_2d<f32>;
@group(1) @binding(6) var occlusion_map: texture_2d<f32>;
@group(1) @binding(8) var<uniform> surface: ModelSurface;

struct LitOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) tangent: vec4<f32>,
    @location(3) uv: vec2<f32>,
    @location(4) color: vec4<f32>,
    @location(5) @interpolate(flat) mirror: f32,
    @location(7) obj: vec3<f32>,
    @location(8) obj_n: vec3<f32>,
};

//@slot specular
//@slot surface_params

@vertex
fn model_lit_vs(in: ModelIn) -> LitOut {
    let local = in.pos_offset.xyz + in.pos_scale.xyz * in.pos.xyz;
    let l4 = vec4<f32>(local, 1.0);
    let world = vec3<f32>(dot(in.row0, l4), dot(in.row1, l4), dot(in.row2, l4));
    let c0 = vec3<f32>(in.row0.x, in.row1.x, in.row2.x);
    let c1 = vec3<f32>(in.row0.y, in.row1.y, in.row2.y);
    let c2 = vec3<f32>(in.row0.z, in.row1.z, in.row2.z);
    let x12 = cross(c1, c2);
    let s = select(1.0, -1.0, dot(c0, x12) < 0.0);
    var o: LitOut;
    o.pos = frame.view_proj * vec4<f32>(world, 1.0);
    o.world = world;
    o.normal = mat3x3<f32>(x12, cross(c2, c0), cross(c0, c1)) * in.normal.xyz * s;
    o.tangent = vec4<f32>(mat3x3<f32>(c0, c1, c2) * in.tangent.xyz, in.tangent.w * s * in.pos_scale.w);
    o.uv = in.uv_dq.zw + in.uv_dq.xy * in.uv;
    o.color = in.tint;
    o.mirror = s;
    // Part-local metres (the instance's uniform scale applied), so tiling
    // layers stay glued to the part however it moves.
    o.obj = local * length(c0);
    o.obj_n = in.normal.xyz;
    return o;
}

// Sky / ground hemisphere colour for a direction (ambient and reflections).
fn surface_hemi(dir_y: f32) -> vec3<f32> {
    let sky = slot_surface_params(2u).rgb;
    let ground = slot_surface_params(3u).rgb;
    return frame.ambient.rgb * mix(ground, sky, clamp(dir_y * 0.5 + 0.5, 0.0, 1.0));
}

// The detail layer: modulates colour and roughness around neutral 0.5.
fn surface_detail(base: vec3<f32>, rough: f32, d: vec2<f32>, strength: f32) -> vec4<f32> {
    let k = clamp(strength, 0.0, 1.0);
    let lum = 1.0 + (d.x - 0.5) * 0.9 * k;
    let r = clamp(rough + (0.5 - d.y) * 0.3 * k, 0.04, 1.0);
    return vec4<f32>(base * lum, r);
}

// Fur under the shells (and alone on Lite/Low). `f` is the baked clump map
// at the material's clump scale, `fc` the same map four times larger (rgb =
// strand height, clump shade, noise; a = 1 when the map is compiled in).
fn surface_fur(base: vec3<f32>, f: vec4<f32>, fc: vec4<f32>, n_dot_v: f32) -> vec3<f32> {
    let on = select(0.0, 1.0, surface.pbr.w > 0.5) * f.a;
    // Shells drawn (fur.z >= 1): this surface is the skin at the roots,
    // seen only between strands, so it stays dark.
    let roots = base * 0.65 * mix(0.85, 1.05, f.g);
    // No shells (Lite/Low, fur.z = 0): the clump map paints the fur instead.
    // Big clumps (fc) break the coat into tufts with dark partings between
    // them; the fine map adds strand-scale grain inside each tuft. Low parts
    // of a tuft are root colour, high parts lean to the lighter tip colour.
    let tuft = smoothstep(0.4, 1.0, clamp(fc.r, 0.0, 1.0));
    let grain = smoothstep(0.45, 1.0, clamp(f.r, 0.0, 1.0));
    let s = clamp(tuft * 0.5 + grain * 0.5 + (f.b - 0.5) * 0.1, 0.0, 1.0);
    // Root and tip levels matched by eye to the High shells on the chimp and
    // the look-lab plush, so switching tiers keeps the coat's tone.
    let root_col = base * 0.25;
    let tip_col = mix(base * 0.8, surface.tip.rgb, 0.15);
    var col = mix(root_col, tip_col, s) * mix(0.7, 1.15, fc.g);
    // Fuzzy edge: towards the silhouette the eye looks along the strands
    // and sees mostly their lighter tips, so the outline goes soft and pale
    // instead of ending on a hard dark edge.
    let e = 1.0 - clamp(n_dot_v, 0.0, 1.0);
    col = mix(col, tip_col * mix(0.8, 1.05, fc.g), smoothstep(0.35, 0.95, e) * 0.65);
    return mix(base, select(col, roots, surface.fur.z >= 0.5), on);
}

// Roughness-limited Schlick term for reflections of the surroundings.
fn surface_env_fresnel(n_dot_v: f32, f0: vec3<f32>, rough: f32) -> vec3<f32> {
    let k = 1.0 - clamp(n_dot_v, 0.0, 1.0);
    let k2 = k * k;
    return f0 + (max(vec3<f32>(1.0 - rough), f0) - f0) * (k2 * k2 * k);
}

@fragment
fn model_lit_fs(in: LitOut, @builtin(front_facing) facing: bool) -> @location(0) vec4<f32> {
    let front = facing != (in.mirror < 0.0);
    let uv3 = vec3<f32>(in.uv, 1.0);
    let uv = vec2<f32>(dot(model_mat.uv0.xyz, uv3), dot(model_mat.uv1.xyz, uv3));
    // Every map before any discard (uniform control flow).
    let tex = textureSample(base_map, map_sampler, uv);
    let nm = textureSample(normal_map, map_sampler, uv).xyz;
    let em = textureSample(emissive_map, map_sampler, uv).rgb;
    let mr = textureSample(mr_map, map_sampler, uv);
    let occ_tex = textureSample(occlusion_map, map_sampler, uv).r;
    let sp1 = slot_surface_params(1u);
    let dscale = select(sp1.y, surface.detail.x, surface.detail.x > 0.0);
    let det = detail_sample(in.obj, in.obj_n, dscale);
    let fur = fur_sample(in.obj, in.obj_n, surface.fur.w);
    let fur_c = fur_sample(in.obj, in.obj_n, surface.fur.w * 4.0);
    var base = tex * model_mat.base * vec4<f32>(in.color.rgb, 1.0);
    if ((!front && model_mat.flags.y < 0.5) || (model_mat.flags.x > 0.5 && base.a < model_mat.emissive_cutoff.w)) {
        discard;
    }
    let side = select(-1.0, 1.0, front);
    let ng = normalize(in.normal);
    var n = ng;
    if (model_mat.flags.z > 0.5 && abs(in.tangent.w) > 0.5) {
        let t = normalize(in.tangent.xyz - ng * dot(ng, in.tangent.xyz));
        let b = cross(ng, t) * in.tangent.w;
        let m = (nm * 2.0 - 1.0) * vec3<f32>(model_mat.flags.w, model_mat.flags.w, 1.0);
        n = normalize(t * m.x + b * m.y + ng * m.z);
    }
    n = n * side;
    let g = ng * side;

    var rough = clamp(surface.pbr.y * mr.g, 0.04, 1.0);
    let metal = clamp(surface.pbr.x * mr.b, 0.0, 1.0);
    let occ = mix(1.0, occ_tex, clamp(surface.pbr.z, 0.0, 1.0));
    let dk = select(0.0, surface.detail.y * sp1.x, surface.tip.w > 0.5);
    let dd = surface_detail(base.rgb, rough, det, dk);
    rough = dd.w;

    let dist = length(in.world - frame.eye.xyz);
    let v = (frame.eye.xyz - in.world) / max(dist, 1e-4);
    let l = frame.light_dir.xyz;
    let ndl = dot(n, l);
    let ndv = max(dot(n, v), 1e-3);
    let albedo = surface_fur(dd.rgb, fur, fur_c, ndv);
    let h = normalize(l + v);
    let ndh = max(dot(n, h), 0.0);
    // Blob shadows (only on tiers without a shadow map; the engine sends none
    // with a map) also take some sky light, so grounding survives the bright
    // hemisphere ambient on Low and Lite.
    let blob = blob_vis(in.world, g);
    let vis = shadow_vis(in.world, g) * blob * self_shadow(dot(g, l));
    let sp0 = slot_surface_params(0u);
    // Direct light: smooth ramp, shadowed, and (by `direct_ao`) the baked
    // occlusion also darkens creases in sunlight.
    let direct_occ = mix(1.0, occ, clamp(sp0.x, 0.0, 1.0));
    let diffuse = slot_toon_ramp(ndl) * vis * direct_occ;
    let f0 = mix(vec3<f32>(0.04), albedo, metal);
    let spec = slot_specular(ndh, max(ndl, 0.0), ndv, rough, metal, albedo) * vis * direct_occ;
    // Ambient: hemisphere by the normal. Reflections: the hemisphere seen in
    // the mirror direction, which rough surfaces see blurred (the lookup
    // leans to the normal and the sky's brightness, sp0.z times the ambient,
    // falls to the ambient's own). Metals take all their ambient light this
    // way, tinted by their base colour; `env_spec` (sp0.y) scales it only on
    // dielectrics.
    let ambient = surface_hemi(n.y) * occ * blob;
    let r = reflect(-v, n);
    let r2 = rough * rough;
    let env_y = mix(r.y, n.y, r2);
    let env_k = mix(sp0.y, 1.0, metal) * mix(max(sp0.z, 1.0), 1.0, r2);
    let env = surface_hemi(env_y) * surface_env_fresnel(ndv, f0, rough) * (env_k * occ * mix(1.0, blob, 0.5));
    let diff_col = albedo * (1.0 - metal);
    var c = diff_col * (ambient + frame.light_color.rgb * diffuse) + frame.light_color.rgb * spec + env
        + model_mat.emissive_cutoff.rgb * em + slot_rim(ndv, ndl, dist, albedo) * occ;
    // Shadow tint, as the engine's model shading does it.
    let k = clamp(1.0 - vis, 0.0, 1.0) * clamp(frame.shadow_tint.w, 0.0, 1.0);
    let w = vec3<f32>(0.2126, 0.7152, 0.0722);
    let ts = sqrt(max(frame.shadow_tint.rgb, vec3<f32>(0.0)));
    let hs = ts / max(dot(ts, w), 1e-3);
    let tt = hs * dot(sqrt(max(c, vec3<f32>(0.0))), w);
    let tc = tt / max(1.0, max(tt.r, max(tt.g, tt.b)));
    c = mix(c, tc * tc, k);
    let f = clamp(slot_fog(dist, in.world.y), 0.0, 1.0);
    return vec4<f32>(mix(c, frame.fog_color.rgb, f), 1.0);
}
