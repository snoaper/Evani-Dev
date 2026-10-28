// evani.shader.layers_brdf
// Layer BRDF functions for the layered model pass (evani-render feature
// `material-layers`): pure functions, prefixed ly_, no bindings. Lobes are
// D * V * F terms; the caller multiplies by N.L, pi (the engine's lights
// carry no 1/pi on the diffuse side) and the light.
// Formulas ported from, not copied as code:
//   Filament (Google, Apache-2.0): Charlie sheen D (Estevez and Kulla 2017),
//     Neubelt cloth V, clear coat GGX with the Kelemen V, anisotropic GGX
//     D and its height-correlated Smith V, the cloth subsurface wrap.
//   three.js MeshPhysicalMaterial (MIT): evalIridescence and
//     evalSensitivity (Belcour and Barla 2017, "A Practical Extension to
//     Microfacet Theory for the Modeling of Varying Iridescence"),
//     IBLSheenBRDF and the sheen energy term 1 - 0.157 * max3(sheen).
//   glTF-Sample-Viewer (Khronos, Apache-2.0): the KHR_materials_anisotropy
//     roughness split (alpha_t = mix(alpha, 1, s^2), alpha_b = alpha) and
//     the bent normal for reflections.

const LY_PI: f32 = 3.14159265;

fn ly_schlick3(f0: vec3<f32>, f90: f32, c: f32) -> vec3<f32> {
    let k = 1.0 - clamp(c, 0.0, 1.0);
    let k2 = k * k;
    return f0 + (vec3<f32>(f90) - f0) * (k2 * k2 * k);
}

fn ly_schlick1(f0: f32, f90: f32, c: f32) -> f32 {
    let k = 1.0 - clamp(c, 0.0, 1.0);
    let k2 = k * k;
    return f0 + (f90 - f0) * (k2 * k2 * k);
}

// ---- cloth sheen ------------------------------------------------------------

// Charlie distribution: a sheen roughness (perceptual) to an inverted
// Gaussian-like lobe that peaks at grazing half vectors.
fn ly_d_charlie(rough: f32, ndh: f32) -> f32 {
    let r = max(rough, 0.07);
    let inv = 1.0 / (r * r);
    let sin2 = max(1.0 - ndh * ndh, 0.0078125);
    return (2.0 + inv) * pow(sin2, inv * 0.5) / (2.0 * LY_PI);
}

// Neubelt and Pettineo's cloth visibility.
fn ly_v_neubelt(ndl: f32, ndv: f32) -> f32 {
    return clamp(1.0 / (4.0 * (ndl + ndv - ndl * ndv) + 1e-4), 0.0, 1.0);
}

// Direct sheen for one light (times N.L, pi and the light by the caller).
fn ly_sheen_direct(color: vec3<f32>, rough: f32, ndh: f32, ndl: f32, ndv: f32) -> vec3<f32> {
    return color * (ly_d_charlie(rough, ndh) * ly_v_neubelt(ndl, ndv));
}

// Sheen of the surroundings: three.js's fitted directional albedo of the
// Charlie lobe, without its 1/pi (the engine's ambient has none).
fn ly_sheen_env(ndv: f32, rough: f32) -> f32 {
    let r = clamp(rough, 0.0, 1.0);
    let r2 = r * r;
    let lo = r < 0.25;
    let a = select(-8.48 * r2 + 14.3 * r - 9.95, -339.2 * r2 + 161.4 * r - 25.9, lo);
    let b = select(1.97 * r2 - 3.27 * r + 0.72, 44.0 * r2 - 23.7 * r + 3.26, lo);
    let dg = exp(a * ndv + b) + select(0.1 * (r - 0.25), 0.0, lo);
    return clamp(dg, 0.0, LY_PI);
}

// How much of the base survives under a sheen of `color`.
fn ly_sheen_scale(color: vec3<f32>) -> f32 {
    return 1.0 - 0.157 * max(color.r, max(color.g, color.b));
}

// Filament's cloth diffuse with a subsurface colour: an energy-conserving
// wrap (w = 0.5) tinted towards `sss` where the light grazes. Replaces the
// N.L ramp, so the caller doesn't multiply by N.L again.
fn ly_cloth_diffuse(ndl: f32, sss: vec3<f32>) -> vec3<f32> {
    let wrap = clamp((ndl + 0.5) / 2.25, 0.0, 1.0);
    return wrap * clamp(sss + vec3<f32>(ndl), vec3<f32>(0.0), vec3<f32>(1.0));
}

// ---- clear coat ---------------------------------------------------------------

fn ly_d_ggx(rough: f32, ndh: f32) -> f32 {
    let a = rough * rough;
    let a2 = a * a;
    let d = ndh * ndh * (a2 - 1.0) + 1.0;
    return a2 / (LY_PI * d * d + 1e-7);
}

// Kelemen's visibility, cheap and fine for a smooth coat.
fn ly_v_kelemen(ldh: f32) -> f32 {
    return 0.25 / max(ldh * ldh, 1e-4);
}

// The coat's lobe and its Fresnel (F0 0.04, a 1.5 IOR varnish), for one
// light: x = D * V * F (times N.L, pi and the light), y = F at L.H (what
// the coat takes from the light before it reaches the layers under it).
fn ly_coat_direct(rough: f32, ndh: f32, ldh: f32) -> vec2<f32> {
    let r = clamp(rough, 0.045, 1.0);
    let f = ly_schlick1(0.04, 1.0, ldh);
    return vec2<f32>(min(ly_d_ggx(r, ndh) * ly_v_kelemen(ldh), 64.0) * f, f);
}

// ---- anisotropy ------------------------------------------------------------------

// The glTF roughness split: (alpha along t, alpha along b).
fn ly_aniso_alphas(rough: f32, strength: f32) -> vec2<f32> {
    let a = max(rough * rough, 0.002);
    return vec2<f32>(mix(a, 1.0, strength * strength), a);
}

// Anisotropic GGX D times its height-correlated Smith V. `t` and `b` the
// anisotropy frame (`t` the direction highlights stretch along).
fn ly_aniso_dv(al: vec2<f32>, t: vec3<f32>, b: vec3<f32>, n: vec3<f32>, l: vec3<f32>, v: vec3<f32>, h: vec3<f32>) -> f32 {
    let at = al.x;
    let ab = al.y;
    let ndh = dot(n, h);
    let a2 = at * ab;
    let f = vec3<f32>(ab * dot(t, h), at * dot(b, h), a2 * ndh);
    let w2 = a2 / max(dot(f, f), 1e-9);
    let d = a2 * w2 * w2 / LY_PI;
    let ndl = max(dot(n, l), 0.0);
    let ndv = max(dot(n, v), 1e-3);
    let gv = ndl * length(vec3<f32>(at * dot(t, v), ab * dot(b, v), ndv));
    let gl = ndv * length(vec3<f32>(at * dot(t, l), ab * dot(b, l), ndl));
    let vis = 0.5 / max(gv + gl, 1e-5);
    return min(d * vis, 64.0);
}

// Phones: a Kajiya-Kay band across `b` (it stretches along `t`), roughly
// normalised for a band and an average visibility.
fn ly_aniso_kajiya(b: vec3<f32>, h: vec3<f32>, rough: f32) -> f32 {
    let a = max(rough * rough, 0.002);
    let e = clamp(2.0 / (a * a) - 2.0, 1.0, 2048.0);
    let bh = dot(b, h);
    let s = sqrt(max(1.0 - bh * bh, 0.0));
    return min(pow(s, e) * sqrt(e) * (0.25 / 15.75), 64.0);
}

// The bent normal reflections take on an anisotropic surface (glTF Sample
// Viewer): the normal leans across the stretch so the reflection of the
// surroundings smears along `t`.
fn ly_aniso_bent(n: vec3<f32>, b: vec3<f32>, v: vec3<f32>, strength: f32, rough: f32) -> vec3<f32> {
    let at = cross(b, v);
    let an = cross(at, b);
    let bend = 1.0 - strength * (1.0 - rough);
    let k = bend * bend * bend * bend;
    let m = mix(an, n, k);
    return m / max(length(m), 1e-4);
}

// ---- iridescence ----------------------------------------------------------------

// Spectral sensitivity of the eye to an optical path difference (nm) with a
// phase shift, as linear Rec. 709 RGB (Belcour and Barla's Fourier fit).
fn ly_eval_sensitivity(opd: f32, shift: vec3<f32>) -> vec3<f32> {
    let phase = 2.0 * LY_PI * opd * 1.0e-9;
    let val = vec3<f32>(5.4856e-13, 4.4201e-13, 5.2481e-13);
    let pos = vec3<f32>(1.6810e+06, 1.7953e+06, 2.2084e+06);
    let vr = vec3<f32>(4.3278e+09, 9.3046e+09, 6.6121e+09);
    var xyz = val * sqrt(2.0 * LY_PI * vr) * cos(pos * phase + shift) * exp(-(phase * phase) * vr);
    xyz.x = xyz.x + 9.7470e-14 * sqrt(2.0 * LY_PI * 4.5282e+09) * cos(2.2399e+06 * phase + shift.x) * exp(-4.5282e+09 * phase * phase);
    xyz = xyz / 1.0685e-7;
    let to_rgb = mat3x3<f32>(
        vec3<f32>(3.2404542, -0.9692660, 0.0556434),
        vec3<f32>(-1.5371385, 1.8760108, -0.2040259),
        vec3<f32>(-0.4985314, 0.0415560, 1.0572252),
    );
    return to_rgb * xyz;
}

fn ly_ior_to_f0(t: vec3<f32>, i: f32) -> vec3<f32> {
    let r = (t - vec3<f32>(i)) / (t + vec3<f32>(i));
    return r * r;
}

// The thin film's Fresnel: air over a film of index `eta2` and thickness
// `thickness` (nm) over a base of reflectance `f0`, seen at `cos1`.
fn ly_iridescence(eta2: f32, cos1: f32, thickness: f32, f0: vec3<f32>) -> vec3<f32> {
    // The film fades to air as it gets thinner than 30 nm.
    let ior = mix(1.0, eta2, smoothstep(0.0, 30.0, thickness));
    let sin2sq = (1.0 / (ior * ior)) * (1.0 - cos1 * cos1);
    let cos2sq = 1.0 - sin2sq;
    if (cos2sq < 0.0) {
        return vec3<f32>(1.0);
    }
    let cos2 = sqrt(cos2sq);
    // First interface (air to film).
    let r0 = ((ior - 1.0) / (ior + 1.0)) * ((ior - 1.0) / (ior + 1.0));
    let r12 = ly_schlick1(r0, 1.0, cos1);
    let t121 = 1.0 - r12;
    let phi12 = select(0.0, LY_PI, ior < 1.0);
    let phi21 = LY_PI - phi12;
    // Second interface (film to base).
    let sf0 = sqrt(clamp(f0, vec3<f32>(0.0), vec3<f32>(0.9999)));
    let base_ior = (vec3<f32>(1.0) + sf0) / (vec3<f32>(1.0) - sf0);
    let r1 = ly_ior_to_f0(base_ior, ior);
    let r23 = ly_schlick3(r1, 1.0, cos2);
    let phi23 = select(vec3<f32>(0.0), vec3<f32>(LY_PI), base_ior < vec3<f32>(ior));
    // Phase shift and the first two interference orders.
    let opd = 2.0 * ior * thickness * cos2;
    let phi = vec3<f32>(phi21) + phi23;
    let r123 = clamp(r12 * r23, vec3<f32>(1e-5), vec3<f32>(0.9999));
    let rr = sqrt(r123);
    let rs = (t121 * t121) * r23 / (vec3<f32>(1.0) - r123);
    var i = vec3<f32>(r12) + rs;
    var cm = rs - vec3<f32>(t121);
    cm = cm * rr;
    i = i + cm * (2.0 * ly_eval_sensitivity(opd, phi));
    cm = cm * rr;
    i = i + cm * (2.0 * ly_eval_sensitivity(2.0 * opd, 2.0 * phi));
    return max(i, vec3<f32>(0.0));
}
