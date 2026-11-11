// evani.shader.character
// Character materials (evani-material-character, through evani-render's
// `char-materials` feature): a wet eye, soft skin and fur/hair shine, for
// close-up models. Appended after the lit surface pass (model_lit.wgsl);
// `shader::patch_lit` rewires model_lit_fs to call char_frame (every screen
// derivative, before any discard), char_shade (per material kind:
// surface.kind.z 1 skin, 2 eye, 3 hair) and char_out (the hand-written
// coverage mask for soft hair edges). Everything else in the lit pass (sun,
// shadows, ambient, reflections, rim, fog) is untouched; a plain material,
// or the slot's zero default, shades exactly as before.
//
// The eye's refracting cornea follows the Godot "Eye" shader by tentabrobpy
// (https://godotshaders.com/shader/eye/, MIT licence; see
// THIRD_PARTY_NOTICES.md): the view ray bends through the cornea
// (refract, ior 1.376) onto an iris plane below it, and its defaults (bulge
// 0.75, ior 1.376, wet roughness 0.05) seeded ours. Rewritten for Evani's
// lit pass, which has no object space: the eye's frame comes from its
// azimuthal UVs (uv 0.5, 0.5 is the pupil axis; the angle from the axis is
// 2 pi times the distance from it) and the screen derivatives.
//
// The skin's pre-integrated lookup (Penner and Borshukov, GPU Pro 2) is
// generated in code at load and arrives as a per-row fit in the
// `char_params` slot (lut.rs); the hair's highlight is two-lobe Kajiya-Kay
// with shifted tangents (Kajiya and Kay 1989; Scheuermann 2004). Ideas
// only; the code is our own.

// The live constants as data (evani-render char_live.rs writes them when a
// param or the tier changes): the slot's body reads them, so neither ever
// recompiles this module. Binding 14 of the model material group.
struct CharLive {
    e: array<vec4<f32>, 30>,
};
@group(1) @binding(14) var<uniform> char_live: CharLive;

//@slot char_params

struct CharFrame {
    // World position per unit of u and of v.
    dpdu: vec3<f32>,
    dpdv: vec3<f32>,
    // UV per pixel (fwidth), for band-limiting patterns.
    uvw: vec2<f32>,
    // Curvature (1/m) and the pixel's size on the surface (m).
    curv: f32,
    px: f32,
};

struct CharShade {
    // The normal the rest of the pass lights with (pores, cornea).
    n_spec: vec3<f32>,
    // Roughness override (negative: the material's).
    rough: f32,
    albedo: vec3<f32>,
    // 1: lit3 below replaces the look's sun ramp.
    own_lit: f32,
    lit3: vec3<f32>,
    // Scale of the look's specular lobe.
    spec_k: f32,
    // Extra sun-lit light (wet lobe, strand highlights), times the sun.
    extra: vec3<f32>,
    // Catchlight, times the sun's brightness and the occlusion.
    catch_l: f32,
    // Coverage (hair edges).
    alpha: f32,
    // Scale of the light a grazing edge catches from the sky: the reflected
    // sky and the look's sky-tinted rim (material-view critic round 6: on
    // skin they drew a cool grey band down the jaw that read as a crease;
    // skin's edge is mostly its own scattered light).
    env_k: f32,
};

struct CharOut {
    @location(0) color: vec4<f32>,
    @builtin(sample_mask) mask: u32,
};

fn char_p(i: u32) -> vec4<f32> {
    return slot_char_params(i);
}

// A material's own value (word >= 0) wins over the live param.
fn char_pick(w: f32, p: f32) -> f32 {
    return select(w, p, w < 0.0);
}

fn char_pick3(w: vec3<f32>, p: vec3<f32>) -> vec3<f32> {
    return select(w, p, w.x < 0.0);
}

// Every derivative the materials need, taken in uniform control flow.
fn char_frame(p: vec3<f32>, uv: vec2<f32>, n: vec3<f32>) -> CharFrame {
    let dp1 = dpdx(p);
    let dp2 = dpdy(p);
    let du1 = dpdx(uv);
    let du2 = dpdy(uv);
    let dn = abs(dpdx(n)) + abs(dpdy(n));
    let det = du1.x * du2.y - du1.y * du2.x;
    let inv = select(0.0, 1.0 / det, abs(det) > 1e-20);
    var cf: CharFrame;
    cf.dpdu = (dp1 * du2.y - dp2 * du1.y) * inv;
    cf.dpdv = (dp2 * du1.x - dp1 * du2.x) * inv;
    cf.uvw = abs(du1) + abs(du2);
    cf.px = max(length(abs(dp1) + abs(dp2)), 1e-7);
    cf.curv = length(dn) / cf.px;
    return cf;
}

fn char_hash_u(x: u32) -> u32 {
    var h = x * 747796405u + 2891336453u;
    h = ((h >> ((h >> 28u) + 4u)) ^ h) * 277803737u;
    return (h >> 22u) ^ h;
}

// A random number in 0..1 for the integer cell of `p`.
fn char_hash(p: vec2<f32>) -> f32 {
    let i = vec2<i32>(floor(p));
    return f32(char_hash_u(bitcast<u32>(i.x) ^ char_hash_u(bitcast<u32>(i.y) + 1013904223u))) * 2.3283064e-10;
}

fn char_vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = p - i;
    let u = f * f * (vec2<f32>(3.0) - 2.0 * f);
    let a = char_hash(i);
    let b = char_hash(i + vec2<f32>(1.0, 0.0));
    let c = char_hash(i + vec2<f32>(0.0, 1.0));
    let d = char_hash(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// Distance to the nearest cell point (x) and the offset from it (yz).
fn char_worley(p: vec2<f32>) -> vec3<f32> {
    let i = floor(p);
    let f = p - i;
    var best = vec3<f32>(8.0, 0.0, 0.0);
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let o = vec2<f32>(f32(x), f32(y));
            let c = i + o;
            let pt = o + vec2<f32>(char_hash(c), char_hash(c + vec2<f32>(37.0, 11.0))) * 0.8 + 0.1;
            let d = f - pt;
            let dd = length(d);
            if (dd < best.x) {
                best = vec3<f32>(dd, d);
            }
        }
    }
    return best;
}

// cos(x), faded out as its period nears the pixel (`w`: x per pixel).
fn char_fcos(x: f32, w: f32) -> f32 {
    return cos(x) * (1.0 - smoothstep(0.0, 3.14159265, w));
}

// The blurred ramp the skin lookup is fitted to (lut.rs `ramp`): the mean
// of max(x + n, 0) for a bell-shaped n of width s.
fn char_ramp(x: f32, s: f32) -> f32 {
    if (s < 1e-4) {
        return max(x, 0.0);
    }
    let z = x / s;
    let cdf = 1.0 / (1.0 + exp(-1.702 * z));
    let pdf = 0.3989423 * exp(-0.5 * z * z);
    return x * cdf + s * pdf;
}

// The skin lookup at N.L (per channel) and fitted row `t` (0 to 7).
fn char_lut(ndl: vec3<f32>, t: f32) -> vec3<f32> {
    let tt = clamp(t, 0.0, 7.0);
    let i0 = u32(floor(tt));
    let i1 = min(i0 + 1u, 7u);
    let f = tt - f32(i0);
    let s = mix(char_p(13u + i0).rgb, char_p(13u + i1).rgb, f);
    let a = mix(char_p(21u + i0).rgb, char_p(21u + i1).rgb, f);
    return a * vec3<f32>(char_ramp(ndl.r, s.r), char_ramp(ndl.g, s.g), char_ramp(ndl.b, s.b));
}

fn char_none(base: vec3<f32>, n: vec3<f32>) -> CharShade {
    var s: CharShade;
    s.n_spec = n;
    s.rough = -1.0;
    s.albedo = base;
    s.own_lit = 0.0;
    s.lit3 = vec3<f32>(0.0);
    s.spec_k = 1.0;
    s.extra = vec3<f32>(0.0);
    s.catch_l = 0.0;
    s.alpha = 1.0;
    s.env_k = 1.0;
    return s;
}

// GGX distribution times Schlick (water's F0), clamped against fireflies.
fn char_ggx(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, a2: f32) -> f32 {
    let h = normalize(l + v);
    let ndh = max(dot(n, h), 0.0);
    let d = ndh * ndh * (a2 - 1.0) + 1.0;
    let ndf = a2 / (3.14159265 * d * d);
    let e = 1.0 - max(dot(v, h), 0.0);
    let fr = 0.025 + 0.975 * (e * e * e * e * e);
    return min(ndf * fr * 0.25, 24.0) * max(dot(n, l), 0.0);
}

fn char_eye(base: vec3<f32>, n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, uv: vec2<f32>, cf: CharFrame, q: f32) -> CharShade {
    var s = char_none(base, n);
    let p1 = char_p(1u);
    let p2 = char_p(2u);
    let p3 = char_p(3u);
    let p4 = char_p(4u);
    let p5 = char_p(5u);
    let ri = clamp(char_pick(surface.spare0.w, p1.w), 0.05, 0.95);
    let outer = char_pick3(surface.spare0.xyz, p1.rgb);
    let sclera = char_pick3(surface.spare1.xyz, p3.rgb);
    let pupil = clamp(char_pick(surface.spare1.w, p2.w), 0.02, 0.95);

    // The eye's frame from its azimuthal UVs.
    let d = uv - vec2<f32>(0.5);
    let r = length(d);
    let dir = select(vec2<f32>(1.0, 0.0), d / max(r, 1e-6), r > 1e-5);
    let th = 6.2831853 * r;
    let st = sin(th);
    let ct = cos(th);
    let radial = cf.dpdu * dir.x + cf.dpdv * dir.y;
    let et0 = radial - n * dot(n, radial);
    let side = normalize(cross(n, select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(n.y) > 0.9)));
    let et = select(side, et0 / max(length(et0), 1e-12), length(et0) > 1e-12);
    // Axis, the outward direction in the iris plane, and the third axis
    // turned the way the UV angle grows.
    let ax = normalize(n * ct - et * st);
    let dd = n * st + et * ct;
    let tang = cf.dpdu * (-dir.y) + cf.dpdv * dir.x;
    let e0 = cross(ax, dd);
    let ee = e0 * select(1.0, -1.0, dot(e0, tang) < 0.0);

    // In the eye's frame (x out along dd, y along ee, z the axis), in
    // units of the eyeball's radius.
    let iv = vec3<f32>(dot(-v, dd), dot(-v, ee), dot(-v, ax));
    let zl = sqrt(max(1.0 - ri * ri, 0.0));
    let inside = (1.0 - smoothstep(ri - 0.015, ri + 0.015, st)) * step(0.0, ct);
    let bulge = p4.x * step(0.5, q);
    let rc = mix(1.0, ri * 1.02, bulge);
    let zc = zl - sqrt(max(rc * rc - ri * ri, 0.0));
    let rho0 = min(st, ri);
    let zcor = zc + sqrt(max(rc * rc - rho0 * rho0, 0.0));
    let nc = normalize(vec3<f32>(rho0, 0.0, zcor - zc));
    // The view ray through the cornea onto the iris plane (flat on Lite).
    var h = vec2<f32>(st, 0.0);
    if (q >= 0.5) {
        let rr = refract(iv, nc, 1.0 / max(p4.y, 1.0));
        let zi = zl - p4.z;
        let t = max(zcor - zi, 0.0) / max(-rr.z, 0.05);
        h = vec2<f32>(rho0, 0.0) + rr.xy * t;
    }
    let rho = length(h) / ri;
    let phi = atan2(dir.y, dir.x) + atan2(h.y, h.x);

    // Iris: colour from the collarette out, fibres and rings fading as they
    // near the pixel, crypts, a soft pupil and the dark limbal ring.
    let fw = length(cf.uvw) * 6.2831853 / ri;
    let wphi = fw / max(rho, 0.05);
    let fib = p5.w;
    let inner = select(vec3<f32>(surface.kind.x, surface.kind.y, surface.kind.w), p2.rgb, surface.kind.x + surface.kind.y + surface.kind.w <= 0.0);
    var col = mix(inner, outer, smoothstep(pupil, min(pupil + 0.45, 1.0), rho));
    let f1 = char_fcos(phi * 46.0 + 2.5 * sin(rho * 9.0 + phi * 3.0), 46.0 * wphi);
    let f2 = char_fcos(phi * 113.0 + 4.0 * rho, 113.0 * wphi);
    let f3 = char_fcos(rho * 28.0, 28.0 * fw);
    let crypt = char_vnoise(vec2<f32>(phi * 4.77, rho * 6.0)) - 0.5;
    col = col * max(1.0 + fib * (0.22 * f1 + 0.1 * f2 + 0.06 * f3 + 0.35 * crypt), 0.2);
    let cx = (rho - pupil - 0.12) * 12.0;
    let collar = exp(-cx * cx);
    col = col + inner * (0.35 * fib * collar);
    let lw = max(p3.w, 0.0);
    col = col * (1.0 - 0.8 * smoothstep(1.0 - lw, 1.0, rho));
    let pe = 0.02 + fw;
    col = mix(vec3<f32>(0.008), col, smoothstep(pupil - pe, pupil + pe, rho));
    col = mix(col, sclera * 0.55, smoothstep(1.0, 1.1, rho));
    // Sclera: a little darker and warmer towards the lids.
    let sc = sclera * mix(vec3<f32>(1.0), vec3<f32>(0.85, 0.72, 0.68), smoothstep(0.7, 1.6, th));
    s.albedo = mix(sc, col, inside);

    let ncw = normalize(nc.x * dd + nc.y * ee + nc.z * ax);
    s.n_spec = normalize(mix(n, ncw, inside));
    // Light reaches the iris through the cornea: lit by the axis.
    let nd = normalize(mix(n, ax, inside * 0.85));
    s.lit3 = vec3<f32>(slot_toon_ramp(dot(nd, l)));
    s.own_lit = 1.0;
    s.rough = p4.w;
    // The sharp wet lobe, widened by how much the normal turns within the
    // pixel (no sparkle on a far eye).
    let a = max(p4.w * 0.5, 0.01);
    let spread = cf.curv * cf.px;
    let a2 = a * a * a * a + 2.0 * spread * spread;
    s.extra = vec3<f32>(p5.x * char_ggx(s.n_spec, v, l, min(a2, 1.0)));
    let cd = lit_rig_dir(vec3<f32>(-0.8, -0.35, 0.5), v);
    let ch = normalize(cd + v);
    let sz = max(p5.z, 1e-3);
    s.catch_l = p5.y * smoothstep(1.0 - sz, 1.0 - sz * 0.35, dot(s.n_spec, ch)) * mix(0.35, 1.0, inside);
    return s;
}

fn char_skin(base: vec3<f32>, n: vec3<f32>, g: vec3<f32>, l: vec3<f32>, uv: vec2<f32>, cf: CharFrame, q: f32) -> CharShade {
    var s = char_none(base, n);
    let p6 = char_p(6u);
    let p7 = char_p(7u);
    let p8 = char_p(8u);
    let scatter = char_pick3(surface.spare0.xyz, p6.rgb);
    let pore_k = char_pick(surface.spare0.w, p7.z);
    let wrinkle_k = char_pick(surface.spare1.x, p7.w);
    let pore_scale = max(char_pick(surface.spare1.y, p8.x), 1.0);
    let tu = cf.dpdu / max(length(cf.dpdu), 1e-12);
    let tv = cf.dpdv / max(length(cf.dpdv), 1e-12);
    let uvpx = max(length(cf.uvw), 1e-9);
    var nn = n;
    var alb = base;
    if (q >= 1.5) {
        // Pores: round pits at Worley cell points, faded as a cell nears
        // the pixel.
        let fade = smoothstep(1.5, 4.0, 1.0 / (pore_scale * uvpx));
        let w = char_worley(uv * pore_scale);
        let rr = 0.22;
        let e = exp(-w.x * w.x / (rr * rr));
        let k = pore_k * fade;
        let grad = w.yz * (2.0 / (rr * rr)) * e * (k * 0.12);
        nn = normalize(nn + grad.x * tu + grad.y * tv);
        alb = alb * (1.0 - 0.18 * e * k);
    }
    if (q >= 2.5) {
        // Fine wrinkles: wavy lines across v, in patches.
        let ws = max(p8.y, 1.0);
        let fade = smoothstep(1.5, 4.0, 1.0 / (ws * uvpx));
        let ph = uv.y * ws * 6.2831853 + 2.5 * char_vnoise(uv * ws * 0.35);
        let amp = smoothstep(0.35, 0.9, char_vnoise(uv * ws * 0.12 + vec2<f32>(7.0)));
        nn = normalize(nn - tv * (cos(ph) * amp * wrinkle_k * fade * 0.2));
    }
    // Spots (freckles, age spots): dark warm dots at a share of the cell
    // points, `spots` of them (a material's own value or none), faded as
    // they near the pixel.
    let spots = char_pick(surface.spare1.z, 0.0);
    if (q >= 0.5 && spots > 0.0) {
        let sc = max(pore_scale * 0.12, 4.0);
        let w = char_worley(uv * sc);
        let cell = floor(uv * sc - w.yz);
        let rad = mix(0.1, 0.24, char_hash(cell + vec2<f32>(5.0, 3.0)));
        let on = step(char_hash(cell + vec2<f32>(91.0, 17.0)), spots);
        let dot_k = on * (1.0 - smoothstep(rad * 0.55, rad, w.x)) * smoothstep(1.0, 3.0, 1.0 / (sc * uvpx));
        alb = mix(alb, alb * vec3<f32>(0.62, 0.42, 0.3), dot_k * 0.85);
    }
    s.n_spec = nn;
    s.lit3 = char_skin_light(nn, g, l, cf, q, scatter);
    s.own_lit = 1.0;
    s.albedo = alb;
    s.env_k = 0.4;
    return s;
}

// Skin's own light for normal `nn` (the hair's ends fade into it on tiers
// without MSAA, char_hair).
fn char_skin_light(nn: vec3<f32>, g: vec3<f32>, l: vec3<f32>, cf: CharFrame, q: f32, scatter: vec3<f32>) -> vec3<f32> {
    let p6 = char_p(6u);
    let p7 = char_p(7u);
    let p8 = char_p(8u);
    let ndl = dot(nn, l);
    // Red light scatters under small bumps: it sees a smoother normal.
    let ndl_r = dot(normalize(mix(nn, g, clamp(p8.z, 0.0, 1.0))), l);
    let nl = vec3<f32>(ndl_r, ndl, ndl);
    let plain = vec3<f32>(max(ndl, 0.0));
    // Low reads the same lookup as Ultra (a few ALU ops; the curvature is
    // taken on every tier): its own wrap lifted the red half-shade about 8
    // levels warmer than Ultra's skin (material-view round 5). Low loses only
    // the pores and wrinkles; Lite keeps the cheaper wrap below.
    if (q >= 0.5) {
        let c = cf.curv * 0.001 * p7.y;
        let t = sqrt(clamp(c, 0.0, 1.0)) * 7.0;
        return max(plain + (char_lut(nl, t) - plain) * p7.x, vec3<f32>(0.0));
    }
    let w = p6.w * scatter / max(max(scatter.r, max(scatter.g, scatter.b)), 1e-3);
    let x = clamp((nl + w) / (vec3<f32>(1.0) + w), vec3<f32>(0.0), vec3<f32>(1.0));
    let ramp = 0.65 * x + 0.35 * x * x * (vec3<f32>(3.0) - 2.0 * x);
    let band = 4.0 * x.r * (1.0 - x.r) * 0.12 * p7.x;
    return ramp + scatter * band;
}

// Kajiya-Kay strand lobe for tangent `t`.
fn char_kk(t: vec3<f32>, h: vec3<f32>, e: f32) -> f32 {
    let th = dot(t, h);
    let sn = sqrt(max(1.0 - th * th, 0.0));
    return smoothstep(-1.0, 0.0, th) * pow(sn, max(e, 1.0));
}

fn char_hair(base: vec3<f32>, n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, uv: vec2<f32>, cf: CharFrame) -> CharShade {
    var s = char_none(base, n);
    let p9 = char_p(9u);
    let p10 = char_p(10u);
    let p11 = char_p(11u);
    let p12 = char_p(12u);
    let tint = char_pick3(surface.spare0.xyz, p9.rgb);
    let pshift = char_pick(surface.spare0.w, p11.x);
    let soft = clamp(char_pick(surface.spare1.x, p12.x), 0.0, 1.0);
    let tipw = clamp(char_pick(surface.spare1.y, p12.y), 0.0, 1.0);

    // Strands run along v (root 0, tip 1); each whole step of u is a lock.
    let t0 = cf.dpdv - n * dot(n, cf.dpdv);
    let side = normalize(cross(n, select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(n.y) > 0.9)));
    let tt = select(side, t0 / max(length(t0), 1e-12), length(t0) > 1e-12);
    let lock = floor(uv.x);
    let jit = (char_hash(vec2<f32>(lock, 17.0)) - 0.5) * p11.z;
    let vv = clamp(uv.y, 0.0, 1.0);

    // Coverage: each lock tapers to its tip with a soft edge across it,
    // averaged once a lock is narrower than the pixel. Where a lock is as wide
    // as its step (a root, or a material whose tips stay full width) it meets
    // its neighbour, so there is no edge to soften: with one, every seam
    // faded to clear and narrow locks (the bust's 1.6 mm brow hairs) read as
    // comb teeth (material-view critic round 7).
    let x = abs(fract(uv.x) - 0.5);
    let hw = 0.5 * mix(1.0, tipw, vv);
    let aa = cf.uvw.x;
    var cov = clamp((hw - x) / max(soft * hw + aa, 1e-4), 0.0, 1.0);
    cov = mix(cov, 2.0 * hw, smoothstep(0.3, 1.0, aa));
    cov = max(cov, smoothstep(0.47, 0.5, hw));
    // A soft hairline: on long strands (|dP/dv| over about 3 cm; brows and
    // lashes are short and keep whole hairs) each fine strand starts at its
    // own height over the first 6 % from the root, so the skin shows between
    // the first hairs; once a strand is under the pixel the fade is their
    // average. It needs v to start at the hairline itself: round 6 put it on
    // a model whose front held v near 0 over a wide band, and the fade showed
    // skin through as a pale band (critic round 7); the bust now starts v at
    // its hairline (tools/blender/bust_build.py).
    let sx = (uv.x + 0.37 * lock) * 28.0;
    let long = smoothstep(0.03, 0.06, length(cf.dpdv));
    let root = 0.004 + 0.06 * char_hash(vec2<f32>(floor(sx * 4.0), 41.0));
    let fringe = mix(smoothstep(root - 0.004, root + 0.004, vv), smoothstep(0.0, 0.064, vv), smoothstep(0.004, 0.012, aa));
    s.alpha = cov * mix(1.0, fringe, long) * (1.0 - smoothstep(0.94, 1.0, vv));
    // One sample a pixel (Lite, Low) cuts this at 40 %, and on a coarse level
    // v runs over big triangles, so the cut through the tip fade wandered into
    // a torn edge that bared the skin (material-view critic rounds 10 and 11),
    // with no fade the tips' own line was hard (round 12), and a stipple of
    // the fade read as a halftone dot band (round 13). There, long strands
    // fade their colour and light into the scalp's
    // (`material.hair.scalp_color`, a material's evani_hair.scalp_color wins)
    // over the fringe's last band, 86 % to 96.5 % of their length (and over
    // their first 8 %), then stop with a clean cut where the tips dive under
    // the skin (the last 2 to 3 %, on the bust's Low level a sawtooth of lock
    // ends), whose lip showed as a ridge of lit and dark facets: the cut runs
    // through skin-coloured hair over skin, so it doesn't show however it
    // wanders. (A fade from 78 % bared the forehead to a receding hairline,
    // and a cut at 87 % to 95 % showed the dark scalp cap at the temples.)
    // A soft, continuous hairline with no dots. Lock edges and short strands
    // keep the clean cut.
    let p29 = char_p(29u);
    let fade_k = clamp(p29.w, 0.0, 1.0) * long * select(0.0, 1.0, CHAR_MSAA == 1u);
    let ends = smoothstep(0.0, 0.08, vv) * (1.0 - smoothstep(0.86, 0.965, vv));
    let hair_k = mix(1.0, ends, fade_k);
    if (fade_k > 0.0) {
        s.alpha = cov * (1.0 - fade_k * smoothstep(0.95, 0.97, vv));
    }

    // Strands within the lock (two sizes, round 6: at 0.12 they were lost
    // and the locks read as one smooth shell), each lock its own tone, and
    // darker roots.
    let strands = 0.6 * char_fcos(sx * 0.25 * 6.2831853, 6.2831853 * 7.0 * aa) + 0.4 * char_fcos(sx * 0.61 * 6.2831853, 6.2831853 * 17.0 * aa);
    let tone = 0.9 + 0.2 * char_hash(vec2<f32>(lock, 3.0));
    s.albedo = base * tone * (0.8 + 0.2 * strands) * mix(1.0 - p12.w, 1.0, smoothstep(0.0, 0.6, vv));

    let h = normalize(l + v);
    let t1 = normalize(tt + n * (pshift + jit));
    let t2 = normalize(tt + n * (p11.y + jit));
    let spark = 0.55 + 0.45 * char_vnoise(vec2<f32>(uv.x * 7.0, uv.y * 30.0));
    let wl = 0.25 + 0.75 * clamp(dot(n, l) * 0.5 + 0.5, 0.0, 1.0);
    s.extra = (tint * (p9.w * char_kk(t1, h, p11.w)) + p10.rgb * base * (p10.w * char_kk(t2, h, p12.z) * spark)) * wl;
    s.spec_k = 0.0;
    // The fade into the scalp (tiers without MSAA, above): the skin's colour
    // and its own light take over from the strands' as hair_k falls. Where
    // the tips start to dive, their faces turn from the light and the view
    // and drew a dashed dark line by the cut: there the light is taken with
    // the normal turned back towards the viewer, as the skin under them
    // faces.
    if (fade_k > 0.0) {
        let scalp = char_pick3(vec3<f32>(surface.kind.x, surface.kind.y, surface.kind.w), p29.rgb);
        let dive = smoothstep(0.9, 0.96, vv) * fade_k;
        let nd = normalize(n + v * (3.0 * dive * clamp(0.7 - dot(n, v), 0.0, 1.0)));
        let lit = char_skin_light(nd, nd, l, cf, char_p(0u).y, char_p(6u).rgb);
        s.albedo = mix(scalp, s.albedo, hair_k);
        s.lit3 = lit;
        s.own_lit = 1.0 - hair_k;
        s.extra *= hair_k;
        s.env_k = mix(0.4, s.env_k, hair_k);
        s.spec_k = 1.0 - hair_k;
    }
    return s;
}

fn char_shade(kind: f32, base: vec3<f32>, n: vec3<f32>, g: vec3<f32>, v: vec3<f32>, l: vec3<f32>, uv: vec2<f32>, cf: CharFrame) -> CharShade {
    let p0 = char_p(0u);
    if (p0.x < 0.5) {
        return char_none(base, n);
    }
    if (abs(kind - 2.0) < 0.5) {
        return char_eye(base, n, v, l, uv, cf, p0.y);
    }
    if (abs(kind - 1.0) < 0.5) {
        return char_skin(base, n, g, l, uv, cf, p0.y);
    }
    if (abs(kind - 3.0) < 0.5) {
        return char_hair(base, n, v, l, uv, cf);
    }
    return char_none(base, n);
}

// Interleaved gradient noise (Jimenez 2014) at a pixel.
// The sun's visibility for a character material. Skin, eyes and hair
// scatter light, and on tiers without a shadow map the baked open-sky cone
// (a coarse per-vertex estimate) left dark blotches in every crease of a
// face: the nostrils, the mouth line, beside the nose (critic round 2). A
// character keeps a fifth of that cone; the face's own side still shades
// it (self_shadow), and with a shadow map nothing changes.
fn char_sun_vis(kind: f32, cast_vis: f32, has_map: bool) -> f32 {
    return select(cast_vis, mix(cast_vis, 1.0, 0.8), kind > 0.5 && !has_map);
}

fn char_ign(p: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(p, vec2<f32>(0.06711056, 0.00583715))));
}

// The colour with a coverage mask: with 4x MSAA, round(alpha x 4) samples
// (dithered, turned per pixel); without, all or none at 40 % coverage.
// Alpha-to-coverage would need alpha < 1 in the colour, which the tonemap
// reads as its screen-door mark.
fn char_out(c: vec4<f32>, a: f32, frag: vec2<f32>) -> CharOut {
    var o: CharOut;
    o.color = c;
    o.mask = 0xffffffffu;
    if (a >= 0.999) {
        return o;
    }
    let z = char_ign(frag);
    if (CHAR_MSAA > 1u) {
        let k = u32(clamp(floor(a * 4.0 + z), 0.0, 4.0));
        let bits = (1u << k) - 1u;
        let rot = u32(char_ign(frag + vec2<f32>(5.3, 9.1)) * 4.0) & 3u;
        o.mask = ((bits << rot) | (bits >> ((4u - rot) & 3u))) & 15u;
    } else {
        // One sample a pixel (Lite, Low): a clean cut at 40 % coverage. A
        // test against the per-pixel noise sprayed loose dots round every
        // hair edge and drew the fur collar as streaks (critic round 2 of
        // material-view); the cut keeps the same locks and tapered tips.
        o.mask = select(0u, 0xffffffffu, a > 0.4);
    }
    return o;
}
