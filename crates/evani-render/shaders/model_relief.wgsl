// evani.shader.model_relief
// Surface relief on textured models (feature `relief`, S3-10), appended
// after model.wgsl: the material's height map moves the UV by parallax
// occlusion (every tier; the step count comes from the tier: 2 on Lite, Low,
// Medium and VR, more on High and Ultra) and its micro-detail recipe (slot
// `micro_detail`, supplied by the look) tips the mapped normal and inks the
// pattern's flanks, faded out where the pattern's period is under 1.5 px on
// screen. The micro-detail and the normal map read the same on every tier;
// only the parallax's step count is tier-dependent. `model_relief_fs` replaces
// `model_fs` in a relief build; after the normal it shades exactly as
// model_fs does (a test holds the two tails equal). Every map is sampled
// with the unshifted UV's derivatives, before any discard.

struct ModelRelief {
    // Height map on (x), its depth in UV units (y), the micro-detail mask
    // (z: 0 none, 1 to 4 the base map's r g b a, 5 to 8 the normal map's).
    height: vec4<f32>,
    // Micro pattern id (x, 0 none), period in UV units (y), depth 0..1 (z),
    // angle in radians (w).
    micro: vec4<f32>,
};

@group(1) @binding(5) var height_map: texture_2d<f32>;
@group(1) @binding(6) var<uniform> model_relief: ModelRelief;
// Shared by every material: parallax steps of the active tier (x; 0 = off).
@group(1) @binding(7) var<uniform> relief_frame: vec4<f32>;

//@slot micro_detail

// How much of the micro-detail shows, by its period on screen in pixels:
// none at 1.5 px or less (it would only shimmer), all of it from 3 px. Only
// clamp and arithmetic, so a test can evaluate it at compile time.
fn micro_fade(px: f32) -> f32 {
    return clamp((px - 1.5) / 1.5, 0.0, 1.0);
}

// Parallax occlusion: march from the top (height 1) along the view ray in
// tangent space (`vt`, z towards the viewer) through `steps` layers of
// `depth` UV units, then blend the last two layers. Gradients `gx`, `gy`
// are the unshifted UV's, so the loop can sample in any control flow.
fn relief_parallax(uv: vec2<f32>, vt: vec3<f32>, depth: f32, steps: f32, gx: vec2<f32>, gy: vec2<f32>) -> vec2<f32> {
    let n = clamp(steps, 1.0, 64.0);
    let d = vt.xy / max(vt.z, 0.25) * depth / n;
    var cur = uv;
    var prev = uv;
    var layer = 0.0;
    var below = 1.0 - textureSampleGrad(height_map, map_sampler, uv, gx, gy).r;
    var gap_prev = below;
    var i = 0.0;
    loop {
        if (i >= n || layer >= below) {
            break;
        }
        prev = cur;
        gap_prev = below - layer;
        cur = cur - d;
        layer = layer + 1.0 / n;
        below = 1.0 - textureSampleGrad(height_map, map_sampler, cur, gx, gy).r;
        i = i + 1.0;
    }
    let after = below - layer;
    let w = clamp(after / min(after - gap_prev, -1e-5), 0.0, 1.0);
    return mix(cur, prev, w);
}

@fragment
fn model_relief_fs(in: ModelOut, @builtin(front_facing) facing: bool) -> @location(0) vec4<f32> {
    let front = facing != (in.mirror < 0.0);
    let uv3 = vec3<f32>(in.uv, 1.0);
    let uv0 = vec2<f32>(dot(model_mat.uv0.xyz, uv3), dot(model_mat.uv1.xyz, uv3));
    let gx = dpdx(uv0);
    let gy = dpdy(uv0);
    let ng = normalize(in.normal);
    let side = select(-1.0, 1.0, front);
    let tangents = abs(in.tangent.w) > 0.5;
    let t = normalize(in.tangent.xyz - ng * dot(ng, in.tangent.xyz));
    let b = cross(ng, t) * in.tangent.w;
    var uv = uv0;
    if (tangents && model_relief.height.x > 0.5 && relief_frame.x > 0.5) {
        let v = normalize(frame.eye.xyz - in.world);
        let vt = vec3<f32>(dot(v, t), dot(v, b), dot(v, ng) * side);
        uv = relief_parallax(uv0, vt, model_relief.height.y, relief_frame.x, gx, gy);
    }
    let tex = textureSampleGrad(base_map, map_sampler, uv, gx, gy);
    let nm4 = textureSampleGrad(normal_map, map_sampler, uv, gx, gy);
    let em = textureSampleGrad(emissive_map, map_sampler, uv, gx, gy).rgb;
    // The micro pattern in its own periods, turned by its angle.
    let cs = vec2<f32>(cos(model_relief.micro.w), sin(model_relief.micro.w));
    let p = vec2<f32>(cs.x * uv.x - cs.y * uv.y, cs.y * uv.x + cs.x * uv.y) / max(model_relief.micro.y, 1e-6);
    let fw = abs(vec2<f32>(cs.x * gx.x - cs.y * gx.y, cs.y * gx.x + cs.x * gx.y)) + abs(vec2<f32>(cs.x * gy.x - cs.y * gy.y, cs.y * gy.x + cs.x * gy.y));
    let px = 1.0 / max(max(fw.x, fw.y) / max(model_relief.micro.y, 1e-6), 1e-6);
    let sel = model_relief.height.z;
    let one = select(vec4<f32>(0.0), vec4<f32>(1.0), vec4<i32>(0, 1, 2, 3) == vec4<i32>((i32(round(sel)) + 3) % 4));
    let mask = select(1.0, dot(select(tex, nm4, sel > 4.5), one), sel > 0.5);
    let km = 0.35 * clamp(model_relief.micro.z, 0.0, 1.0) * micro_fade(px) * clamp(mask, 0.0, 1.0) * select(0.0, 1.0, model_relief.micro.x > 0.5);
    var base = tex * model_mat.base * vec4<f32>(in.color.rgb, 1.0);
    if ((!front && model_mat.flags.y < 0.5) || (model_mat.flags.x > 0.5 && base.a < model_mat.emissive_cutoff.w)) {
        discard;
    }
    var n = ng;
    if (tangents) {
        var m = vec3<f32>(0.0, 0.0, 1.0);
        if (model_mat.flags.z > 0.5) {
            m = (nm4.xyz * 2.0 - 1.0) * vec3<f32>(model_mat.flags.w, model_mat.flags.w, 1.0);
        }
        if (km > 0.0) {
            // The pattern's gradient back on the UV axes (tangent and
            // bitangent), tipping the mapped normal away from the slope.
            let gp = slot_micro_detail(p, model_relief.micro);
            let gu = vec2<f32>(cs.x * gp.x + cs.y * gp.y, -cs.y * gp.x + cs.x * gp.y);
            m = vec3<f32>(m.xy - gu * km * m.z, m.z);
            // Slope ink: the pattern's flanks darken a little whatever the
            // light, so a banded ramp's flat bands can't swallow it.
            base = vec4<f32>(base.rgb * (1.0 - km * min(dot(gp, gp), 1.0)), base.a);
        }
        n = normalize(t * m.x + b * m.y + ng * m.z);
    }
    n = n * side;
    let g = ng * side;
    let dist = length(in.world - frame.eye.xyz);
    let ndl = dot(n, frame.light_dir.xyz);
    let cast_vis = select(sun_cone(dot(normalize(in.sun.xyz), frame.light_dir.xyz), in.sun.w), shadow_vis(in.world, g), frame.shadow.x > 0.5);
    let vis = cast_vis * blob_vis(in.world, g) * self_shadow(dot(g, frame.light_dir.xyz));
    let diffuse = slot_toon_ramp(mix(min(ndl, -0.1), ndl, vis));
    let ndv = dot(n, (frame.eye.xyz - in.world) / max(dist, 1e-4));
    var c = base.rgb * (frame.ambient.rgb + frame.light_color.rgb * diffuse) + model_mat.emissive_cutoff.rgb * em
        + slot_rim(ndv, ndl, dist, base.rgb);
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
