// evani.shader.recipes.shared
// Helpers the recipes share (batch 1 build plan 2.3, package P4). Appended
// once after the core when any recipe is compiled (shader::stage_wgsl_with),
// so a game with no recipe never downloads them. Everything is written fresh
// for Evani.Dev.

// The recipe's own colour c (linear; times the texture and the vertex colour,
// never model_mat.base, so the fallback colour is never applied twice), its
// roughness and metalness. Normal and AO stay as the lit stage had them; no
// post-light terms.
fn rc_out(i: MatIn, c: vec3<f32>, rough: f32, metal: f32) -> MatOut {
    return MatOut(vec4<f32>(c * i.tex.rgb * i.vcol, i.base.a), clamp(rough, 0.03, 1.0), clamp(metal, 0.0, 1.0), i.n, i.occ, vec3<f32>(0.0), vec4<f32>(0.0), vec3<f32>(0.0));
}

// How much of a procedural normal to draw: all of it near the camera, none
// from 25 m (plan 2.5: Medium adds normals within 25 m; Ultra parallaxes
// everywhere, its normals keep the same reach).
fn rc_near(f: MatFrame) -> f32 {
    return 1.0 - smoothstep(18.0, 25.0, f.dist);
}

// The surface's uv tangents (column 0 along u, column 1 along v) from the
// screen derivatives mat_frame took: a cotangent frame, so no vertex tangents
// are needed. Scaled so the longer column is 1.
fn rc_tuv(f: MatFrame, n: vec3<f32>) -> mat2x3<f32> {
    let a = cross(f.dpy, n);
    let c = cross(n, f.dpx);
    let t = a * f.duvx.x + c * f.duvy.x;
    let s = a * f.duvx.y + c * f.duvy.y;
    let k = inverseSqrt(max(max(dot(t, t), dot(s, s)), 1e-20));
    return mat2x3<f32>(t * k, s * k);
}

// Two unit tangents across n, for world-space patterns.
fn rc_tw(n: vec3<f32>) -> mat2x3<f32> {
    let t = normalize(cross(n, select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(n.y) > 0.9)));
    return mat2x3<f32>(t, cross(n, t));
}

// Weathering at the foot (procedural realism round 8): splash grime and
// damp over the lowest ~12 cm above the ground (world y 0; the stable
// object-local height is CORE_REQUESTS `object-local-position`), up to a
// fifth darker and a little browner at the contact, its top edge broken by
// the noise n (0..1).
fn rc_foot(c: vec3<f32>, y: f32, n: f32) -> vec3<f32> {
    let k = 1.0 - smoothstep(0.0, 0.12, y + 0.05 * (n - 0.5));
    return c * mix(vec3<f32>(1.0), vec3<f32>(0.78, 0.76, 0.72), k);
}

// n tilted by a height slope g: the height change per unit step along t's
// two columns (a recipe scales g by its relief depth and rc_near).
fn rc_bump(n: vec3<f32>, t: mat2x3<f32>, g: vec2<f32>) -> vec3<f32> {
    return normalize(n - t * g);
}

// The shared height-to-normal step: n tilted by the height's slope, from
// the height at the point (h0) and one step e along each of the two tangents
// t (hx, hy), scaled by k.
fn rc_h2n(n: vec3<f32>, t: mat2x3<f32>, h0: f32, hx: f32, hy: f32, e: f32, k: f32) -> vec3<f32> {
    return rc_bump(n, t, vec2<f32>(hx - h0, hy - h0) * (k / e));
}
