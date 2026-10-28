
// ---- kit surface (feature `kit-surface`) -----------------------------------
// Appended to the forward module when the kit code defines `kit_surface`.
// The sun highlight of a surface with roughness `rough` and normal `n` at
// `p`, already shadowed by `vis`, plus a faint sky reflection on smooth
// surfaces (wet stone, glaze, varnish). Rough surfaces (0.85 and up) get
// none, so a kit that reports roughness 1 lights exactly as before.
//
// The GGX distribution, the fast correlated Smith visibility and Schlick's
// Fresnel follow Google Filament's `brdf.fs` (Apache-2.0, Copyright (C) 2016
// The Android Open Source Project; https://github.com/google/filament).
fn kit_spec(n: vec3<f32>, p: vec3<f32>, rough: f32, vis: f32) -> vec3<f32> {
    if (rough >= 0.99) {
        return vec3<f32>(0.0);
    }
    let v = normalize(frame.eye.xyz - p);
    let l = frame.light_dir.xyz;
    let h = normalize(l + v);
    let ndl = clamp(dot(n, l), 0.0, 1.0);
    let ndh = clamp(dot(n, h), 0.0, 1.0);
    let ndv = clamp(dot(n, v), 1e-3, 1.0);
    // The sun is a disc, not a point: widen the lobe so a glaze (rough 0.2)
    // still shows it.
    let a = min(max(rough * rough, 0.002) + 0.02, 1.0);
    let a2 = a * a;
    let d = ndh * ndh * (a2 - 1.0) + 1.0;
    let dist = a2 / (3.14159265 * d * d);
    let vis_term = 0.5 / max(mix(2.0 * ndl * ndv, ndl + ndv, a), 1e-4);
    let fh = pow(1.0 - clamp(dot(l, h), 0.0, 1.0), 5.0);
    let fres = 0.04 + 0.96 * fh;
    let sun = frame.light_color.rgb * min(dist * vis_term * fres * ndl, 64.0) * vis;
    // The sky seen in a smooth surface: a horizon-to-zenith gradient from the
    // fog colour, looked up along the reflected view ray, by Fresnel. Water
    // and glaze (rough under 0.35) start from a higher reflectance than the
    // physical 4 per cent, so a puddle or a glazed tile reads at a glance.
    let r = reflect(-v, n);
    let horizon = frame.fog_color.rgb;
    let sky = mix(horizon * 0.8, horizon * vec3<f32>(0.3, 0.55, 1.0), smoothstep(0.0, 0.55, r.y));
    let env_col = mix(horizon * 0.4, sky, smoothstep(-0.2, 0.1, r.y));
    let f0 = 0.04 + 0.2 * smoothstep(0.35, 0.05, rough);
    let fv = f0 + (1.0 - f0) * pow(1.0 - ndv, 5.0);
    let smooth_k = (1.0 - rough) * (1.0 - rough);
    let env = env_col * (fv * smooth_k);
    return (sun + env) * (1.0 - smoothstep(0.85, 0.99, rough));
}
