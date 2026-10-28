// evani.shader.model_lit_layers
// Layered materials (evani-render feature `material-layers`, crate
// evani-material-layers): the lit surface pass (shaders/model_lit.wgsl,
// spliced just before this file, with its helpers and bindings 5, 6 and 8)
// plus cloth sheen, a clear coat, brushed-metal anisotropy and thin-film
// iridescence, read from the material's layer record (binding 10, what the
// glTF KHR_materials_sheen / clearcoat / anisotropy / iridescence
// extensions deliver) and scaled by the live strengths in the shared
// per-frame uniform (binding 11). The ly_* lobes come from the crate's
// layers_brdf.wgsl, spliced between model_lit.wgsl and this file. A
// material with no layer flag draws exactly as model_lit_fs does.
// Layer order (three.js): base, then sheen, then clear coat.
// Tiers: the frame's path word picks full (stretched GGX anisotropy, thin
// film, a three-tap weave with relief) or cheap (a Kajiya-Kay band mixed in,
// no thin film, a one-tap weave).

struct LayerMat {
    // Sheen colour (linear) and sheen roughness.
    sheen: vec4<f32>,
    // Clear coat factor, clear coat roughness, anisotropy strength, rotation.
    coat: vec4<f32>,
    // Iridescence factor, film IOR, film thickness (nm), flag bits.
    iri: vec4<f32>,
    // Cloth subsurface colour, weave repeats per UV unit.
    cloth: vec4<f32>,
};

struct LayerFrame {
    // Live strengths: sheen, clear coat, anisotropy, iridescence (0 cheap).
    k: vec4<f32>,
    // Weave strength, weave scale, path (1 full, 0 cheap), unused.
    w: vec4<f32>,
};

@group(1) @binding(9) var ly_weave_map: texture_2d<f32>;
@group(1) @binding(10) var<uniform> ly_mat: LayerMat;
@group(1) @binding(11) var<uniform> ly_frame: LayerFrame;

fn ly_has(flags: u32, bit: u32) -> bool {
    return (flags & bit) != 0u;
}

// The look's specular scale: its lobe at a fixed reference point over the
// plain lobe's value there (N.H = N.L = N.V = 1, roughness 0.5, dielectric:
// 0.04 / (pi * 0.0625) * 0.25 = 0.0509296). Zero when the look has no
// specular, so the layers' highlights match the look's brightness.
fn ly_look_spec() -> f32 {
    return slot_specular(1.0, 1.0, 1.0, 0.5, 0.0, vec3<f32>(1.0)).x / 0.0509296;
}

// A surface frame (t, b) round `n`: the model's tangent where it has one,
// else one built from the screen derivatives of position and UV.
fn ly_tbn(n: vec3<f32>, tangent: vec4<f32>, dpx: vec3<f32>, dpy: vec3<f32>, duvx: vec2<f32>, duvy: vec2<f32>) -> mat2x3<f32> {
    var t = tangent.xyz;
    var hand = select(1.0, -1.0, tangent.w < 0.0);
    if (abs(tangent.w) <= 0.5) {
        let p2 = cross(dpy, n);
        let p1 = cross(n, dpx);
        t = p2 * duvx.x + p1 * duvy.x;
        let b = p2 * duvx.y + p1 * duvy.y;
        hand = select(1.0, -1.0, dot(cross(n, t), b) < 0.0);
    }
    t = t - n * dot(n, t);
    if (dot(t, t) < 1e-10) {
        t = cross(n, select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(n.y) > 0.9));
    }
    t = normalize(t);
    return mat2x3<f32>(t, cross(n, t) * hand);
}

@fragment
fn model_layered_fs(in: ModelOut, @builtin(front_facing) facing: bool) -> @location(0) vec4<f32> {
    let front = facing != (in.mirror < 0.0);
    let uv3 = vec3<f32>(in.uv, 1.0);
    let uv = vec2<f32>(dot(model_mat.uv0.xyz, uv3), dot(model_mat.uv1.xyz, uv3));
    // Every map and derivative before any discard (uniform control flow).
    let tex = textureSample(base_map, map_sampler, uv);
    let nm = textureSample(normal_map, map_sampler, uv).xyz;
    let em = textureSample(emissive_map, map_sampler, uv).rgb;
    let mr = textureSample(mr_map, map_sampler, uv);
    let occ_tex = textureSample(occlusion_map, map_sampler, uv).r;
    let dpx = dpdx(in.world);
    let dpy = dpdy(in.world);
    let duvx = dpdx(uv);
    let duvy = dpdy(uv);
    let flags = u32(ly_mat.iri.w + 0.5);
    let full = ly_frame.w.z > 0.5;
    // The weave: repeats per UV unit times the live scale; it fades out
    // where one texel of the 64-texel tile gets smaller than a pixel (the
    // tile has no mips).
    let wuv = uv * (ly_mat.cloth.w * ly_frame.w.y);
    let foot = max(length(duvx), length(duvy)) * (ly_mat.cloth.w * ly_frame.w.y) * 64.0;
    let w0 = textureSampleLevel(ly_weave_map, map_sampler, wuv, 0.0);
    var wu = w0;
    var wv = w0;
    if (full) {
        wu = textureSampleLevel(ly_weave_map, map_sampler, wuv + vec2<f32>(1.0 / 64.0, 0.0), 0.0);
        wv = textureSampleLevel(ly_weave_map, map_sampler, wuv + vec2<f32>(0.0, 1.0 / 64.0), 0.0);
    }
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
    let tb = ly_tbn(n, vec4<f32>(in.tangent.xyz, in.tangent.w), dpx, dpy, duvx, duvy);

    // Cloth weave: brightness (centred on 0.5) on the base colour and, on
    // the full path, the thread relief on the normal.
    let weave_on = ly_has(flags, 16u) || ly_has(flags, 32u);
    let wk = select(0.0, clamp(ly_frame.w.x, 0.0, 2.0) * (1.0 - smoothstep(0.75, 1.5, foot)), weave_on);
    if (wk > 0.0) {
        let knit = ly_has(flags, 32u);
        let s0 = select(w0.rg, w0.ba, knit);
        base = vec4<f32>(base.rgb * max(1.0 + (s0.x - 0.5) * 1.3 * wk, 0.0), base.a);
        if (full) {
            let hu = select(wu.g, wu.a, knit) - s0.y;
            let hv = select(wv.g, wv.a, knit) - s0.y;
            n = normalize(n - (tb[0] * hu + tb[1] * hv) * (0.9 * wk));
        }
    }

    var rough = clamp(surface.pbr.y * mr.g, 0.04, 1.0);
    let metal = clamp(surface.pbr.x * mr.b, 0.0, 1.0);
    let occ = mix(1.0, occ_tex, clamp(surface.pbr.z, 0.0, 1.0));
    let kind = surface.kind.z;
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
    let cast_vis = select(sun_cone(dot(normalize(in.sun.xyz), l), in.sun.w), shadow_vis(in.world, g), frame.shadow.x > 0.5);
    let blob = blob_vis(in.world, g);
    let vis = cast_vis * blob * self_shadow(dot(g, l));
    let sp0 = slot_surface_params(0u);
    let direct_occ = mix(1.0, occ, clamp(sp0.x, 0.0, 1.0));
    var lit3 = vec3<f32>(slot_toon_ramp(ndl));
    if (abs(kind - 1.0) < 0.5 && (sp10.x > 0.0 || sp10.y > 0.0)) {
        lit3 = mix(lit3, lit_skin(ndl, sp10.x, sp10.y), 1.0 - wet);
    }
    // Cloth with a subsurface colour: Filament's wrapped cloth diffuse.
    let sss = ly_mat.cloth.rgb;
    if (dot(sss, sss) > 0.0) {
        lit3 = ly_cloth_diffuse(ndl, sss);
    }
    let sun_rgb = frame.light_color.rgb;
    let direct = lit3 * vis * direct_occ;
    let f0 = mix(vec3<f32>(0.04), base.rgb, metal);
    let look_k = ly_look_spec();
    let ndl0 = max(ndl, 0.0);
    var spec = slot_specular(ndh, ndl0, ndv, rough, metal, base.rgb);

    // Anisotropy: highlights stretch along the tangent turned by the
    // material's rotation. Full: stretched GGX in place of the look's lobe
    // (at the look's brightness); cheap: a Kajiya-Kay band mixed in.
    var env_n = n;
    let an = clamp(ly_mat.coat.z * ly_frame.k.z, 0.0, 1.0);
    if (ly_has(flags, 4u) && an > 0.0) {
        let cr = cos(ly_mat.coat.w);
        let sr = sin(ly_mat.coat.w);
        var ta = tb[0] * cr + tb[1] * sr;
        ta = normalize(ta - n * dot(n, ta));
        let ba = cross(n, ta);
        let e = 1.0 - clamp(ndv, 0.0, 1.0);
        let fr = f0 + (vec3<f32>(1.0) - f0) * (e * e * e * e * e);
        if (full) {
            spec = fr * ly_aniso_dv(ly_aniso_alphas(rough, an), ta, ba, n, l, v, h) * ndl0 * look_k;
        } else {
            spec = mix(spec, fr * ly_aniso_kajiya(ba, h, rough) * ndl0 * look_k, an);
        }
        env_n = ly_aniso_bent(n, ba, v, an, rough);
    }

    // Iridescence (full path only: its strength is 0 on the cheap one): the
    // thin film's Fresnel in place of Schlick, on the look's lobe and on
    // the reflections.
    let ik = select(0.0, clamp(ly_mat.iri.x * ly_frame.k.w, 0.0, 1.0), ly_has(flags, 8u));
    var f_iri = vec3<f32>(1.0);
    if (ik > 0.0) {
        let e = 1.0 - clamp(ndv, 0.0, 1.0);
        let fr = f0 + (vec3<f32>(1.0) - f0) * (e * e * e * e * e);
        let fi = ly_iridescence(ly_mat.iri.y, ndv, ly_mat.iri.z, f0);
        f_iri = mix(vec3<f32>(1.0), fi / max(fr, vec3<f32>(1e-3)), ik);
        spec = spec * f_iri;
    }
    spec = spec * (vis * direct_occ);

    let ao = min(occ, lit_ao(in.pos));
    let ambient = lit_hemi(n) * ao * blob;
    let r = reflect(-v, env_n);
    let r2 = rough * rough;
    let env_dir = normalize(mix(r, env_n, r2));
    let env_k = mix(sp0.y, 1.0, metal) * mix(max(sp0.z, 1.0), 1.0, r2);
    let env = lit_env(env_dir, rough) * (lit_env_fresnel(ndv, f0, rough) * f_iri) * (env_k * ao * mix(1.0, blob, 0.5));
    let diff_col = base.rgb * (1.0 - metal);
    var c = diff_col * (ambient + lit_points(in.world, n, in.pos) + sun_rgb * direct) + sun_rgb * spec + env
        + model_mat.emissive_cutoff.rgb * em + slot_rim(ndv, ndl, dist, base.rgb) * occ + lit_rig(n, v, diff_col, occ)
        + vec3<f32>(lit_catch(n, v, wet, occ));

    // Sheen (Charlie D, Neubelt V): the base dims by the sheen's energy and
    // the sheen adds its own lobe of the sun and of the surroundings.
    let sheen_col = max(ly_mat.sheen.rgb * ly_frame.k.x, vec3<f32>(0.0));
    if (ly_has(flags, 1u) && dot(sheen_col, sheen_col) > 0.0) {
        let sr = ly_mat.sheen.w;
        let sd = ly_sheen_direct(sheen_col, sr, ndh, ndl0, ndv) * (ndl0 * 3.14159265) * (vis * direct_occ);
        let se = sheen_col * (ly_sheen_env(ndv, sr) * ao * blob) * lit_hemi(n);
        c = c * ly_sheen_scale(min(sheen_col, vec3<f32>(1.0))) + sun_rgb * sd + se;
    }

    // Clear coat on the geometric normal (it sits over the normal map): it
    // takes its Fresnel share from everything under it, then adds its own
    // smooth lobe of the sun and its reflection of the surroundings.
    let cc = select(0.0, clamp(ly_mat.coat.x * ly_frame.k.y, 0.0, 1.0), ly_has(flags, 2u));
    if (cc > 0.0) {
        let ccr = clamp(ly_mat.coat.y, 0.03, 1.0);
        let ndv_c = max(dot(g, v), 1e-3);
        let ndl_c = max(dot(g, l), 0.0);
        let coat = ly_coat_direct(ccr, max(dot(g, h), 0.0), max(dot(l, h), 0.0));
        let fcc = ly_schlick1(0.04, 1.0, ndv_c);
        let coat_k = select(3.14159265, look_k, look_k > 0.0);
        let rc = reflect(-v, g);
        let rc2 = ccr * ccr;
        let coat_env = lit_env(normalize(mix(rc, g, rc2)), ccr) * (fcc * max(sp0.z, 1.0) * ao * mix(1.0, blob, 0.5));
        c = c * (1.0 - cc * fcc) + (sun_rgb * (coat.x * ndl_c * coat_k * vis * direct_occ) + coat_env) * cc;
    }

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
