// evani.shader.model_lit
// The lit surface pass (feature `look-surface`, S5-R5b), appended after the
// whole forward module (forward, model and, with `skinning`, the skinned
// stages and the shadow lookup: frame, model_mat, the maps, shadow_vis,
// blob_vis, sun_cone and self_shadow are shared). The model pipelines draw
// model_lit_fs in place of model_fs, from the same model_vs / model_skin_vs
// varyings (ModelOut). On top of the engine's model shading it:
//   * reads the glTF metallicRoughness map (G roughness, B metal) and the
//     occlusion map (R), scaled by the material's factors (binding 8);
//   * lights with toon_ramp (the look makes it a smooth wrapped diffuse), a
//     roughness/metal specular lobe (slot `specular`), a sky/ground
//     hemisphere ambient with reflections, the point lights and the rim;
//   * with the film preset (slot `surface_params` 4 to 7 and 10 to 11, zero
//     = off): a kick and a fill light that follow the camera, wrapped skin
//     with a warm terminator, and wet eyes with a catchlight (the material's
//     kind in `surface.kind.z`: 0 plain, 1 skin, 2 eye).
// The engine's point lights, sky ambient and ambient occlusion come in by
// the lit_* glue functions after this file (src/shaders.rs), which call
// what the runtime patches add; without those features they are the flat
// frame ambient, no point light and no occlusion.
// Adapted from ChimpDuty's model_surface.wgsl (ChimpDuty, Chris Pearson),
// fur and the tiling detail layer left out.

struct ModelSurface {
    // Metal factor, roughness factor, occlusion strength, unused.
    pbr: vec4<f32>,
    spare0: vec4<f32>,
    spare1: vec4<f32>,
    // Unused, unused, material kind (0 plain, 1 skin, 2 eye), unused.
    kind: vec4<f32>,
};

@group(1) @binding(5) var mr_map: texture_2d<f32>;
@group(1) @binding(6) var occlusion_map: texture_2d<f32>;
@group(1) @binding(8) var<uniform> surface: ModelSurface;

//@slot specular
//@slot surface_params

// Sky / ground hemisphere light for a direction (ambient and reflections).
// Tints with w = 0 multiply the scene's ambient (lit_sky); with w = 1 they
// already are the light. Both tints zero (the engine's default body): white.
fn lit_hemi(d: vec3<f32>) -> vec3<f32> {
    var sky = slot_surface_params(2u);
    var ground = slot_surface_params(3u);
    if (dot(sky.rgb, sky.rgb) + dot(ground.rgb, ground.rgb) <= 0.0) {
        sky = vec4<f32>(1.0, 1.0, 1.0, sky.w);
        ground = vec4<f32>(1.0, 1.0, 1.0, ground.w);
    }
    let k = clamp(d.y * 0.5 + 0.5, 0.0, 1.0);
    return mix(ground.rgb, sky.rgb, k) * mix(lit_sky(d), vec3<f32>(1.0), clamp(sky.w, 0.0, 1.0));
}

fn lit_wrap(ndl: f32, w: f32) -> f32 {
    return clamp((ndl + w) / (1.0 + w), 0.0, 1.0);
}

// Direction towards a rig light from its (back, right, up) parts in the
// camera's frame: `back` points away from the camera along the ground, so
// the light follows the camera round the model.
fn lit_rig_dir(c: vec3<f32>, v: vec3<f32>) -> vec3<f32> {
    let vh = normalize(vec3<f32>(v.x, 0.0, v.z) + vec3<f32>(0.0, 0.0, 1e-4));
    let d = -vh * c.x + vec3<f32>(vh.z, 0.0, -vh.x) * c.y + vec3<f32>(0.0, c.z, 0.0);
    return d / max(length(d), 1e-4);
}

fn lit_sun() -> f32 {
    return dot(frame.light_color.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// The film rig on top of the sun: a back kick and a fill (slot
// surface_params 4 to 7: direction and share of the sun's brightness, colour
// and wrap), wrapped diffuse, no shadow of its own (the occlusion still
// closes creases). All zero: no rig.
fn lit_rig(n: vec3<f32>, v: vec3<f32>, albedo: vec3<f32>, occ: f32) -> vec3<f32> {
    let k = slot_surface_params(4u);
    let kc = slot_surface_params(5u);
    let f = slot_surface_params(6u);
    let fc = slot_surface_params(7u);
    let sun = lit_sun();
    let kick = kc.rgb * (k.w * sun) * lit_wrap(dot(n, lit_rig_dir(k.xyz, v)), kc.w);
    let fill = fc.rgb * (f.w * sun) * lit_wrap(dot(n, lit_rig_dir(f.xyz, v)), fc.w);
    return albedo * (kick + fill) * occ;
}

// Skin: light wraps past the terminator, red further than green and blue
// (light scattered under the skin comes out reddened), with a warm band
// along the terminator. `wrap` and `scatter` from slot 10. Same ramp shape
// as the stylizedpbr look's toon_ramp.
fn lit_skin(ndl: f32, wrap: f32, scatter: f32) -> vec3<f32> {
    let w = wrap * mix(vec3<f32>(1.0), vec3<f32>(1.0, 0.42, 0.26), scatter);
    let x = clamp((vec3<f32>(ndl) + w) / (vec3<f32>(1.0) + w), vec3<f32>(0.0), vec3<f32>(1.0));
    let ramp = 0.65 * x + 0.35 * x * x * (vec3<f32>(3.0) - 2.0 * x);
    let band = 4.0 * x.r * (1.0 - x.r) * scatter * 0.12;
    return ramp + vec3<f32>(1.0, 0.3, 0.12) * band;
}

// How wet a texel is: eye materials all over, skin where its roughness is
// under the wet limit (slot 10 z: a mouth painted on the head); else 0.
fn lit_wet(kind: f32, rough: f32, limit: f32) -> f32 {
    let skin = select(0.0, 1.0 - smoothstep(limit - 0.08, limit + 0.08, rough), abs(kind - 1.0) < 0.5 && limit > 0.0);
    return select(skin, 1.0, abs(kind - 2.0) < 0.5);
}

// The catchlight on wet surfaces: a small soft spot of a light by the
// camera's upper left (the rig's camera frame), as bright as the sun times
// slot 11's y, its size slot 11's z, closed by the occlusion.
fn lit_catch(n: vec3<f32>, v: vec3<f32>, wet: f32, occ: f32) -> f32 {
    let e = slot_surface_params(11u);
    if (e.y <= 0.0) {
        return 0.0;
    }
    let d = lit_rig_dir(vec3<f32>(-0.8, -0.35, 0.5), v);
    let h = normalize(d + v);
    let spot = smoothstep(1.0 - e.z, 1.0 - e.z * 0.35, dot(n, h));
    return e.y * spot * lit_sun() * occ * wet;
}

// Roughness-limited Schlick term for reflections of the surroundings.
fn lit_env_fresnel(n_dot_v: f32, f0: vec3<f32>, rough: f32) -> vec3<f32> {
    let k = 1.0 - clamp(n_dot_v, 0.0, 1.0);
    let k2 = k * k;
    return f0 + (max(vec3<f32>(1.0 - rough), f0) - f0) * (k2 * k2 * k);
}

@fragment
fn model_lit_fs(in: ModelOut, @builtin(front_facing) facing: bool) -> @location(0) vec4<f32> {
    let front = facing != (in.mirror < 0.0);
    let uv3 = vec3<f32>(in.uv, 1.0);
    let uv = vec2<f32>(dot(model_mat.uv0.xyz, uv3), dot(model_mat.uv1.xyz, uv3));
    // Every map before any discard (uniform control flow).
    let tex = textureSample(base_map, map_sampler, uv);
    let nm = textureSample(normal_map, map_sampler, uv).xyz;
    let em = textureSample(emissive_map, map_sampler, uv).rgb;
    let mr = textureSample(mr_map, map_sampler, uv);
    let occ_tex = textureSample(occlusion_map, map_sampler, uv).r;
    let base = tex * model_mat.base * vec4<f32>(in.color.rgb, 1.0);
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
    let kind = surface.kind.z;
    // Film materials (slot 10 and 11, zero = off): wet texels (eyes, a
    // mouth) take the look's wet roughness.
    let sp10 = slot_surface_params(10u);
    let sp11 = slot_surface_params(11u);
    let wet = lit_wet(kind, rough, sp10.z);
    if (sp11.x > 0.0) {
        rough = mix(rough, sp11.x, wet);
    }

    let dist = length(in.world - frame.eye.xyz);
    let v = (frame.eye.xyz - in.world) / max(dist, 1e-4);
    let l = frame.light_dir.xyz;
    let ndl = dot(n, l);
    let ndv = max(dot(n, v), 1e-3);
    let h = normalize(l + v);
    let ndh = max(dot(n, h), 0.0);
    // The engine's sun visibility, as model_fs: the shadow map where there
    // is one, else the baked open-sky cone; blob shadows; the face's own side.
    let cast_vis = select(sun_cone(dot(normalize(in.sun.xyz), l), in.sun.w), shadow_vis(in.world, g), frame.shadow.x > 0.5);
    let blob = blob_vis(in.world, g);
    let vis = cast_vis * blob * self_shadow(dot(g, l));
    let sp0 = slot_surface_params(0u);
    // Direct light: the ramp, shadowed, and (by sp0.x) the baked occlusion
    // also darkens creases in sunlight.
    let direct_occ = mix(1.0, occ, clamp(sp0.x, 0.0, 1.0));
    var lit3 = vec3<f32>(slot_toon_ramp(ndl));
    if (abs(kind - 1.0) < 0.5 && (sp10.x > 0.0 || sp10.y > 0.0)) {
        lit3 = mix(lit3, lit_skin(ndl, sp10.x, sp10.y), 1.0 - wet);
    }
    let sun_rgb = frame.light_color.rgb;
    let direct = lit3 * vis * direct_occ;
    let f0 = mix(vec3<f32>(0.04), base.rgb, metal);
    let spec = slot_specular(ndh, max(ndl, 0.0), ndv, rough, metal, base.rgb) * vis * direct_occ;
    // Ambient: the baked occlusion and the screen-space one, whichever is
    // darker (never both multiplied), and the blob shadows.
    let ao = min(occ, lit_ao(in.pos));
    let ambient = lit_hemi(n) * ao * blob;
    // Reflections: the surroundings seen in the mirror direction, rough
    // surfaces leaning to the normal. Metals take all their ambient light
    // this way; sp0.y scales it on dielectrics, sp0.z (at least 1) is how
    // much brighter the sky is seen in a smooth reflection.
    let r = reflect(-v, n);
    let r2 = rough * rough;
    let env_dir = normalize(mix(r, n, r2));
    let env_k = mix(sp0.y, 1.0, metal) * mix(max(sp0.z, 1.0), 1.0, r2);
    let env = lit_env(env_dir, rough) * lit_env_fresnel(ndv, f0, rough) * (env_k * ao * mix(1.0, blob, 0.5));
    let diff_col = base.rgb * (1.0 - metal);
    var c = diff_col * (ambient + lit_points(in.world, n, in.pos) + sun_rgb * direct) + sun_rgb * spec + env
        + model_mat.emissive_cutoff.rgb * em + slot_rim(ndv, ndl, dist, base.rgb) * occ + lit_rig(n, v, diff_col, occ)
        + vec3<f32>(lit_catch(n, v, wet, occ));
    // Shadow tint, as model_fs does it.
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
