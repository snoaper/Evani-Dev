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
    let inner = p2.rgb;
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
    s.n_spec = nn;
    let ndl = dot(nn, l);
    // Red light scatters under small bumps: it sees a smoother normal.
    let ndl_r = dot(normalize(mix(nn, g, clamp(p8.z, 0.0, 1.0))), l);
    let nl = vec3<f32>(ndl_r, ndl, ndl);
    let plain = vec3<f32>(max(ndl, 0.0));
    if (q >= 1.5) {
        let c = cf.curv * 0.001 * p7.y;
        let t = sqrt(clamp(c, 0.0, 1.0)) * 7.0;
        s.lit3 = max(plain + (char_lut(nl, t) - plain) * p7.x, vec3<f32>(0.0));
    } else {
        let w = p6.w * scatter / max(max(scatter.r, max(scatter.g, scatter.b)), 1e-3);
        let x = clamp((nl + w) / (vec3<f32>(1.0) + w), vec3<f32>(0.0), vec3<f32>(1.0));
        let ramp = 0.65 * x + 0.35 * x * x * (vec3<f32>(3.0) - 2.0 * x);
        let band = 4.0 * x.r * (1.0 - x.r) * 0.12 * p7.x;
        s.lit3 = ramp + scatter * band;
    }
    s.own_lit = 1.0;
    s.albedo = alb;
    return s;
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
    // averaged once a lock is narrower than the pixel.
    let x = abs(fract(uv.x) - 0.5);
    let hw = 0.5 * mix(1.0, tipw, vv);
    let aa = cf.uvw.x;
    var cov = clamp((hw - x) / max(soft * hw + aa, 1e-4), 0.0, 1.0);
    cov = mix(cov, 2.0 * hw, smoothstep(0.3, 1.0, aa));
    s.alpha = cov * (1.0 - smoothstep(0.94, 1.0, vv));

    // Strands within the lock and darker roots.
    let strands = char_fcos((uv.x + 0.37 * lock) * 6.2831853 * 7.0, 6.2831853 * 7.0 * aa);
    s.albedo = base * (0.88 + 0.12 * strands) * mix(1.0 - p12.w, 1.0, smoothstep(0.0, 0.6, vv));

    let h = normalize(l + v);
    let t1 = normalize(tt + n * (pshift + jit));
    let t2 = normalize(tt + n * (p11.y + jit));
    let spark = 0.55 + 0.45 * char_vnoise(vec2<f32>(uv.x * 7.0, uv.y * 30.0));
    let wl = 0.25 + 0.75 * clamp(dot(n, l) * 0.5 + 0.5, 0.0, 1.0);
    s.extra = (tint * (p9.w * char_kk(t1, h, p11.w)) + p10.rgb * base * (p10.w * char_kk(t2, h, p12.z) * spark)) * wl;
    s.spec_k = 0.0;
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
fn char_ign(p: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(p, vec2<f32>(0.06711056, 0.00583715))));
}

// The colour with a coverage mask: with 4x MSAA, round(alpha x 4) samples
// (dithered, turned per pixel); without, all or none against the noise.
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
        o.mask = select(0u, 0xffffffffu, a > z);
    }
    return o;
}
