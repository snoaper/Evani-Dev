// evani.shader.nature_sea_field
// The field sea (`material.nature.water.wave_field`, and every storm sea):
// the sea as one camera-tied mesh whose vertices read the wave textures the
// waves draw made this frame (sea_waves.wgsl, layout sea_field_bands.wgsl)
// and move by them, up and sideways towards the crests, so crests stand
// peaked against the sky; each pixel then reads the slopes and folding of
// every band its footprint can resolve and lights them as the classic sea
// does (sky, sun and glitter on a roughness made of what it can't resolve,
// back-lit crest glow, foam where the surface folds, spray). There is no
// march: what a frame costs doesn't change with the waves' height.
//
// The mesh: rings around the eye's foot, spaced by the finer of 2.6 % of
// the distance and an even step of view angle down from the eye (so a deck
// and an aircraft both get a few pixels between rings), and segments over
// the view's headings only; vertices come from the vertex index (no vertex
// buffer). Its depth is its own (cleared each frame); the scene in front
// of the water keeps its pixel. Over a beach the waves shrink as the
// classic sea's do and no vertex goes under the bed.
//
// Ideas from Ultra-Water (Humpalumps, MIT): cascades read by a displaced
// mesh, slopes from the squeezed surface, foam from the folding, the far
// haze at 18 km. Our own code. Appended to it: sea_common.wgsl (the classic
// sea's shared light, beach and foam code), sea_field_bands.wgsl,
// sea_field_break.wgsl and the classic table.

@group(0) @binding(0) var scene_tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> u: Sea;
@group(0) @binding(3) var depth_tex: texture_depth_2d;
@group(0) @binding(4) var field_tex: texture_2d<f32>;

struct MeshOut {
    @builtin(position) pos: vec4<f32>,
    // The water's rest point (x, z) the textures are read at, and where
    // the mesh put it.
    @location(0) p0: vec2<f32>,
    @location(1) wp: vec3<f32>,
    // A breaking crest here (sea_field_break.wgsl): how far over it has
    // folded, how far it has collapsed.
    @location(2) bk: vec2<f32>,
    // The fold's own normal there (a smooth one from the lip's shape: the mesh
    // facets of a thin curling sheet would light it in stripes); up elsewhere.
    @location(3) ln: vec3<f32>,
};

// Rings and segments per level (Low, Medium, High, Ultra); src/sea/field.rs
// `GRID` draws 6 (rings - 1) segments vertices.
fn fd_grid(level: u32) -> vec2<u32> {
    let g = array<vec2<u32>, 4>(vec2<u32>(160u, 192u), vec2<u32>(384u, 384u), vec2<u32>(576u, 576u), vec2<u32>(768u, 768u));
    return g[min(level, 3u)];
}

const F_NEAR: f32 = 0.05;
const F_RMAX: f32 = 30000.0;
// Radial spacing over angular spacing (as a share of the distance, per
// radian of view angle).
const F_AB: f32 = 7.5;

// The sea's rms height (m) and its summed slope variance, from the atlas'
// total texel.
var<private> F_HRMS: f32;
var<private> F_VS: f32;
// The storm settings (src/sea/mod.rs `storm_code`).
var<private> F_MID: f32;
var<private> F_STREAK: f32;
var<private> F_DRIFT: f32;
var<private> F_UNDER: bool;
// The wind speed the facets scale with (m/s), from the unresolved budget's
// own terms: the sea's slope variance plus what is left unresolved.
var<private> F_WIND: f32;
// What made this pixel's foam (the foam view, dev builds:
// sea_field_debug.wgsl): new from an event now (a crest folding or breaking,
// the shore), carried from an event a moment ago, fading, and none.
var<private> F_DBG: vec4<f32>;

fn fd_setup() {
    let word = u32(max(u.level, 0.0) + 0.5);
    let code = word >> 2u;
    F_MID = f32(code & 127u) * 0.02;
    F_STREAK = f32((code >> 7u) & 63u) * 0.02;
    F_DRIFT = f32((code >> 13u) & 127u) * 0.02;
    F_UNDER = ((code >> 21u) & 1u) == 1u;
    let tot = textureLoad(field_tex, vec2<i32>(i32(FB), i32(F_INFO_Y)), 0);
    F_HRMS = sqrt(max(tot.z, 0.0));
    F_VS = tot.x + tot.y;
    F_WIND = max(sqrt(GRAV / max(u.wind.x, 1e-4)), 1.0);
    // The classic code's bounds: the deepest trough (for the shallow-water
    // shrink) and the tallest crest (for the view from under the water).
    AMP_LO = max(4.0 * F_HRMS, 1e-3);
    H_HI = 4.5 * F_HRMS;
    RW = vec2<f32>(cos(u.wind.z), sin(u.wind.z));
    RS = vec2<f32>(cos(u.swell.y), sin(u.swell.y));
}

fn fd_eye_h() -> f32 {
    return max(abs(u.eye.y - u.centre.y), 0.5);
}

// Ring i of n (0: the eye's foot), out to F_RMAX.
fn fd_ring(i: u32, n: u32, hh: f32) -> f32 {
    if (i == 0u) {
        return 0.0;
    }
    let r0 = max(0.05 * hh, 0.03);
    let r1 = max(0.136 * hh, r0);
    let r2 = max(7.364 * hh, r1);
    let a1 = atan(r1 / hh);
    let c1 = log(r1 / r0) / F_AB;
    let c2 = atan(r2 / hh) - a1;
    // (Out to the horizon's last pixel: far enough from an aircraft too.)
    let rmax = max(F_RMAX, 2000.0 * hh);
    let c3 = log(max(rmax / r2, 1.0)) / F_AB;
    let x = f32(i - 1u) / f32(max(n, 3u) - 2u) * (c1 + c2 + c3);
    if (x < c1) {
        return r0 * exp(F_AB * x);
    }
    if (x < c1 + c2) {
        return hh * tan(a1 + x - c1);
    }
    return r2 * exp(F_AB * (x - c1 - c2));
}

// The rest point the waves' squeeze carries under the eye (src/sea/field.rs
// `eye_rest`): the rings go round it, so the water in front of the eye always
// has rings to draw it, whichever way the waves are carrying it just now.
fn fd_centre() -> vec2<f32> {
    let w = u32(max(u.shape.z, 0.0) + 0.5);
    return u.eye.xz + (vec2<f32>(f32(w & 4095u), f32(w >> 12u)) - 2048.0) * 0.0625;
}

// How much of the squeeze a vertex keeps, from the squeeze's own stretch
// tensor there (lambda M of the bands the mesh carries): where it would bunch
// the water up past FD_CMAX along its strongest axis the squeeze eases off,
// so the mesh never folds over itself (two layers fighting for one pixel
// read as dotted lines); where it bunches the water from every side at once
// (its weaker axis past FD_CMIN) it eases off sooner, so a crest stands as
// a rounded ridge, not a lone pyramid. src/sea/field.rs `squeeze_cap`.
const FD_CMAX: f32 = 0.65;
const FD_CMIN: f32 = 0.18;
// The long bands, whose squeeze is capped by their own tensor (`vs_main`).
const FD_LONG: u32 = 3u;
// The fine ripples' width along the wind over their length across it.
const FD_RIPPLE: f32 = 0.35;
// The most a band's own squeeze steepens its slope by: 1 / this (Ultra-Water's 0.2).
const FD_SHARP_MIN: f32 = 0.2;

fn fd_cap(e: f32, c: f32) -> f32 {
    let r = max(e, 0.0) / c;
    let r2 = r * r;
    return inverseSqrt(sqrt(1.0 + r2 * r2));
}

fn fd_squeeze_cap(lm: vec3<f32>) -> f32 {
    let mean = 0.5 * (lm.x + lm.y);
    let dev = sqrt(0.25 * (lm.x - lm.y) * (lm.x - lm.y) + lm.z * lm.z);
    return min(fd_cap(mean + dev, FD_CMAX), fd_cap(mean - dev, FD_CMIN));
}

fn fd_wrap(a: f32) -> f32 {
    return a - TAU * floor((a + PI) / TAU);
}

// The view's headings: (centre, half span); all round when the view takes
// in the vertical (straight down or up).
fn fd_span() -> vec2<f32> {
    let f = normalize(u.fwd);
    for (var e = 0; e < 2; e = e + 1) {
        let s = select(-f.y, f.y, e == 1);
        if (s > 1e-4) {
            let sy = select(-1.0, 1.0, e == 1);
            let nx = sy * u.right.y / dot(u.right, u.right) / s;
            let ny = sy * u.up.y / dot(u.up, u.up) / s;
            if (abs(nx) <= 1.1 && abs(ny) <= 1.1) {
                return vec2<f32>(0.0, PI);
            }
        }
    }
    let c0 = atan2(f.z, f.x);
    var span = 0.0;
    for (var k = 0; k < 4; k = k + 1) {
        let c = f + u.right * select(-1.0, 1.0, (k & 1) == 1) + u.up * select(-1.0, 1.0, k >= 2);
        if (length(c.xz) < 1e-4 * length(c)) {
            return vec2<f32>(0.0, PI);
        }
        span = max(span, abs(fd_wrap(atan2(c.z, c.x) - c0)));
    }
    return vec2<f32>(c0, min(span + 0.12, PI));
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> MeshOut {
    fd_setup();
    let level = u32(quality());
    let g = fd_grid(level);
    let quad = vi / 6u;
    let cnr = vi % 6u;
    let di = array<u32, 6>(0u, 1u, 0u, 0u, 1u, 1u);
    let dj = array<u32, 6>(0u, 0u, 1u, 1u, 0u, 1u);
    let i = min(quad / g.y + di[cnr], g.x - 1u);
    let jseg = quad % g.y + dj[cnr];
    let hh = fd_eye_h();
    let r = fd_ring(i, g.x, hh);
    let sp0 = fd_span();
    // Near the eye the squeeze can carry water from outside the view's
    // headings into it: those rings reach a little wider.
    let sp = vec2<f32>(sp0.x, min(sp0.y + min(3.0 * u.swell.z * F_HRMS / max(r, 0.5), 0.6), PI));
    let th = sp.x - sp.y + 2.0 * sp.y * f32(jseg) / f32(g.y);
    let p0 = fd_centre() + r * vec2<f32>(cos(th), sin(th));
    // How far apart the vertices are here: a band's waves stay in the
    // geometry while its shortest has about five vertices a wavelength,
    // gone by two and a half (the light keeps them, in the pixel's own read): a far
    // crest is drawn round, never as a few straight edges meeting in a point.
    let r_in = fd_ring(select(i - 1u, 0u, i == 0u), g.x, hh);
    let r_out = fd_ring(min(i + 1u, g.x - 1u), g.x, hh);
    let spacing = max(max(r_out - r, r - r_in), r * 2.0 * sp.y / f32(g.y));
    // (The squeeze in two parts: the long bands', which carries the water
    // tens of metres in a storm, and the rest's.)
    var dl = vec2<f32>(0.0);
    var ds = vec2<f32>(0.0);
    var h = 0.0;
    var ml = vec3<f32>(0.0);
    var ms = vec3<f32>(0.0);
    if (level > 0u) {
        let nb = fbands(level);
        for (var b = 0u; b < nb; b = b + 1u) {
            let w = 1.0 - smoothstep(0.2, 0.4, spacing / (fband_hi(b) / F_RHO));
            if (w <= 0.0) {
                break;
            }
            let a = textureSampleLevel(field_tex, samp, fuv(p0, b, 0u), 0.0);
            let tb = textureSampleLevel(field_tex, samp, fuv(p0, b, 1u), 0.0);
            h = h + w * a.y;
            if (b < FD_LONG) {
                dl = dl + w * a.xz;
                ml = ml + w * vec3<f32>(tb.z, tb.w, a.w);
            } else {
                ds = ds + w * a.xz;
                ms = ms + w * vec3<f32>(tb.z, tb.w, a.w);
            }
        }
    }
    let q = sea_q(depth_at(p0));
    // (Each part capped by its own stretch tensor: a cap read off the short
    // waves varies over a few metres, and scaling the long waves' tens of
    // metres of squeeze by it folded the mesh into stacked shelves beside a
    // storm crest. The long part's cap varies only over the long waves; the
    // short part gets what the long one leaves.)
    let lq = q * u.swell.z;
    let sql = fd_squeeze_cap(ml * lq);
    let sqs = fd_squeeze_cap((ml * sql + ms) * lq);
    let dsp = dl * sql + ds * sqs;
    var wp = vec3<f32>(p0.x + q * dsp.x, u.centre.y + q * h, p0.y + q * dsp.y);
    if (level > 0u && breakers_on()) {
        wp.y = wp.y + breakers(p0, false).h;
    }
    // A breaking crest (sea_field_break.wgsl `brk_fold`). Medium only leans; the
    // shape is left out where the mesh is too coarse to carry it (the white
    // water and the spray still show).
    var bk = vec2<f32>(0.0);
    var ln = vec3<f32>(0.0, 1.0, 0.0);
    let chance = brk_chance();
    if (chance > 0.0 && level > 0u) {
        let pr = brk_params();
        let ev = brk_at(p0, chance, pr);
        if (ev.x > 0.0 && ev.w > 0.0) {
            let wv = brk_wave(p0, pr, fbands(level));
            let env = max(q * wv.y, 0.0);
            let s_wide = smoothstep(0.9, 1.7, wv.z) * ev.w;
            if (s_wide > 0.0) {
                // (Tongues along the crest, not one even roll: where it goes
                // over wanders from one moment and one wave to the next.)
                // (Gently: the whole crest goes, more in some places than others,
                // not separate lumps.)
                let tongues = mix(0.4, 1.0, smoothstep(0.2, 0.75, vnoise(p0 / (0.11 * pr.lm) + vec2<f32>(u.tl * 300.0 / pr.ev)).x));
                let s = brk_tall(wv.z) * ev.w * tongues;
                let xf = wv.x * pr.lm / TAU;
                let fine = 1.0 - smoothstep(0.12, 0.32, spacing / max(brk_lip(env, pr), 1e-3));
                let sh = brk_fold(xf, env, min(s, 1.0), ev, pr, select(1.0, 0.45, level == 1u)) * fine;
                wp.x = wp.x + RW.x * sh.x;
                wp.z = wp.z + RW.y * sh.x;
                wp.y = wp.y + sh.y;
                // (The lip's tangent along the wind, from the fold a step
                // further on: its normal turns over with it, under the tip too.)
                let e = 0.02 * pr.lm;
                let sh2 = brk_fold(xf + e, env, min(s, 1.0), ev, pr, select(1.0, 0.45, level == 1u)) * fine;
                let ta = e + sh2.x - sh.x;
                let tb = sh2.y - sh.y;
                ln = normalize(vec3<f32>(-RW.x * tb, ta, -RW.y * tb) + vec3<f32>(0.0, 1e-4, 0.0));
                bk = brk_white(xf, env, min(s, 1.0), s_wide, ev, pr);
            }
        }
    }
    // No vertex under the bed.
    wp.y = max(wp.y, u.centre.y - depth_at(p0) + 0.15);
    var o: MeshOut;
    let d = wp - u.eye;
    let f = normalize(u.fwd);
    let s = dot(d, f);
    let nx = dot(d, u.right) / dot(u.right, u.right);
    let ny = dot(d, u.up) / dot(u.up, u.up);
    o.pos = vec4<f32>(nx, ny, F_NEAR, s);
    o.p0 = p0;
    o.wp = wp;
    o.bk = bk;
    o.ln = ln;
    return o;
}

// The field's surface at rest point p0 for footprint fpa (m), shrunk by q:
// the whole surface's slope (the kept bands' slope through the inverse
// transpose of the squeeze's Jacobian I - lambda M, so where the long waves
// bunch the water up every short wave on it steepens: crest lines), height,
// folding now and a moment ago, a crest measure (the tensor's trace in rms
// slopes), curvature and the slope variance the footprint filtered out.
struct FSurf {
    g: vec2<f32>,
    h: f32,
    jac: f32,
    jac_was: f32,
    peak: f32,
    lap: f32,
    // The slope variance the footprint filtered out, along the view and
    // across it.
    lost: vec2<f32>,
    // The height a moment ago (High and Ultra; 0 below).
    h_was: f32,
};

// The pixel's footprint on the water is an ellipse: `fpc` (m) across the
// view, and along it longer the more the view grazes the surface there; `rd`
// the view, `fdir` its heading on the water. How far it grazes is read off the
// surface the longer bands make (summed so far, longest first: a smooth
// tilt, no mesh facets), so a slope facing the eye isn't stretched and a
// wave's back turned away is. Each band's slope is split along and across the
// view and each part faded by the footprint in its own direction, so the
// ripples whose slopes run across the view (crests along it) stay sharp out to
// where the plain footprint resolves them, and only those running along the
// view, which would alias into rows of dots, fade by the long one; a band the
// long footprint fades is also read twice, a quarter of it either way along
// the view, and averaged (as anisotropic texture filtering does). Height and
// the folding tensor (foam, glow) fade between the two.
fn fd_surface(p0: vec2<f32>, q: f32, rd: vec3<f32>, fdir: vec2<f32>, fpc: f32) -> FSurf {
    var o = FSurf(vec2<f32>(0.0), 0.0, 1.0, 1.0, 0.0, 0.0, vec2<f32>(0.0), 0.0);
    let level = u32(quality());
    let nb = fbands(level);
    var g0 = vec2<f32>(0.0);
    var m = vec3<f32>(0.0);
    var jw = 0.0;
    var hprev = 0.0;
    var rprev = 1.0;
    let lf = u.shape.x;
    let fs = vec2<f32>(-fdir.y, fdir.x);
    for (var b = 0u; b < nb; b = b + 1u) {
        let lo = fband_hi(b) / F_RHO;
        let gs = g0 * q;
        let stretch = min(1.0 / max(abs(dot(normalize(vec3<f32>(-gs.x, 1.0, -gs.y)), rd)), 0.06), 16.0);
        let fpf = fpc * stretch;
        let fpa = fpc * pow(stretch, 0.75);
        let w = 1.0 - smoothstep(0.25, 0.5, fpa / lo);
        // (Across the view the footprint grows a little with the stretch too:
        // at most about 6:1 along to across, so the faded ripples don't leave a
        // brushed, smeared grain running down the view.)
        let wc = 1.0 - smoothstep(0.35, 0.9, fpc * pow(stretch, 0.35) / lo);
        let wf = 1.0 - smoothstep(0.35, 0.9, fpf / lo);
        let info = textureLoad(field_tex, vec2<i32>(i32(b), i32(F_INFO_Y)), 0);
        // (Its slope variance along and across the view, from its variances
        // and covariance.)
        let vf = info.x * fdir.x * fdir.x + info.y * fdir.y * fdir.y + 2.0 * info.w * fdir.x * fdir.y;
        let vc = info.x * fs.x * fs.x + info.y * fs.y * fs.y + 2.0 * info.w * fs.x * fs.y;
        o.lost = o.lost + vec2<f32>((1.0 - wf * wf) * max(vf, 0.0), (1.0 - wc * wc) * max(vc, 0.0));
        if (wc <= 0.0) {
            continue;
        }
        var ta = textureSampleLevel(field_tex, samp, fuv(p0, b, 0u), 0.0);
        var tb = textureSampleLevel(field_tex, samp, fuv(p0, b, 1u), 0.0);
        let two = wf < wc - 0.02;
        let off = fdir * (0.25 * fpf);
        if (two) {
            ta = 0.5 * (textureSampleLevel(field_tex, samp, fuv(p0 + off, b, 0u), 0.0) + textureSampleLevel(field_tex, samp, fuv(p0 - off, b, 0u), 0.0));
            tb = 0.5 * (textureSampleLevel(field_tex, samp, fuv(p0 + off, b, 1u), 0.0) + textureSampleLevel(field_tex, samp, fuv(p0 - off, b, 1u), 0.0));
        }
        let gr = f_group(b, hprev, rprev);
        let wg = w * gr;
        // (Each band's slope through its own squeeze, as Ultra-Water's cascades:
        // every scale's crests stand sharp and thin over broad troughs, so the
        // light breaks into fine lines, not smooth lumps.)
        let db = max(vec2<f32>(1.0 - lf * q * tb.z, 1.0 - lf * q * tb.w), vec2<f32>(FD_SHARP_MIN));
        let gb = tb.xy / db;
        g0 = g0 + gr * (fdir * (wf * dot(gb, fdir)) + fs * (wc * dot(gb, fs)));
        m = m + wg * vec3<f32>(tb.z, tb.w, ta.w);
        o.h = o.h + w * ta.y;
        if (level >= 2u && w > 0.0) {
            var tc = textureSampleLevel(field_tex, samp, fuv(p0, b, 2u), 0.0);
            if (two) {
                tc = 0.5 * (textureSampleLevel(field_tex, samp, fuv(p0 + off, b, 2u), 0.0) + textureSampleLevel(field_tex, samp, fuv(p0 - off, b, 2u), 0.0));
            }
            jw = jw + wg * (tc.x - 1.0);
            o.lap = o.lap + wg * tc.y;
            o.h_was = o.h_was + w * tc.z;
        }
        hprev = ta.y;
        rprev = sqrt(max(info.z, 0.0));
    }
    let ms = m * q;
    o.g = g0 * q;
    o.jac = (1.0 - lf * ms.x) * (1.0 - lf * ms.y) - lf * lf * ms.z * ms.z;
    o.jac_was = 1.0 + jw * q;
    o.peak = (ms.x + ms.y) / max(sqrt(F_VS), 1e-3);
    o.h = o.h * q;
    o.lap = o.lap * q;
    o.h_was = o.h_was * q;
    return o;
}

// Storm foam's pattern: patches and streaks drawn out along the wind (wind
// frame: along, across), no bubble net.
fn fd_lace(w: vec2<f32>, fp: f32) -> f32 {
    let wp = vnoise(vec2<f32>(w.x * 0.03, w.y * 0.2) + 2.3).x - 0.5;
    let q = vec2<f32>(w.x, w.y + wp * 4.0) * 2.2;
    let a = vnoise(vec2<f32>(q.x * 0.11, q.y * 0.36)).x;
    let b = vnoise(vec2<f32>(q.x * 0.4, q.y * 1.25) + 5.7).x;
    let c = vnoise(vec2<f32>(q.x * 1.1, q.y * 2.7) + 1.3).x;
    let dk = 1.0 - smoothstep(0.05, 0.2, fp);
    let d = mix(0.5, vnoise(q * 3.3 + 8.1).x, dk);
    let e = mix(0.5, vnoise(q * 8.7 + 2.9).x, dk * dk);
    return clamp(0.2 * a + 0.22 * b + 0.26 * c + 0.2 * d + 0.12 * e, 0.0, 1.0);
}

// GGX with its own roughness along the view's heading `f` (a.x) and across it
// (a.y), in the surface's frame round normal n; h the half vector.
fn fd_ggx_aniso(n: vec3<f32>, h: vec3<f32>, f: vec3<f32>, a: vec2<f32>) -> f32 {
    let tf = normalize(f - n * dot(n, f) + vec3<f32>(1e-6, 0.0, 0.0));
    let ts = cross(n, tf);
    let x = dot(h, tf) / a.x;
    let y = dot(h, ts) / a.y;
    let z = max(dot(h, n), 0.0);
    let d = x * x + y * y + z * z;
    return 1.0 / (PI * a.x * a.y * d * d);
}

fn fd_shade(p0: vec2<f32>, p3: vec3<f32>, px: Px, bk: vec2<f32>, ngv: vec3<f32>, ln: vec3<f32>) -> Shade {
    var o = Shade(true, 0.0, vec3<f32>(1.0), vec3<f32>(0.0), vec2<f32>(0.0), 1.0);
    let ro = u.eye;
    let dv = p3 - ro;
    let t = length(dv);
    let rd = dv / max(t, 1e-6);
    o.t = t;
    let v = -rd;
    let level = quality();
    let fp_iso = max(t * u.px, 1e-4);
    // The footprint along the view as the view grazes, on the slope the mesh
    // shows here, not flat water (on a wave's back, tilted away from the eye, a
    // pixel spans far more water along the view); across the view it stays
    // the plain one. Each band's slope fades by the footprint in its own
    // direction and those the long one fades are read twice along it
    // (`fd_surface`), so no band aliases into rows of dotted streaks however
    // far the view grazes, and the fine ripples stay sharp across it. What
    // the fade filters out goes to the roughness, along and across the view.
    let fdir = rd.xz / max(length(rd.xz), 1e-4);
    let q = sea_q(depth_at(p0));
    let sf = fd_surface(p0, q, rd, fdir, fp_iso);
    // (The footprint along the view for what is finer than the bands, on the
    // whole surface's tilt.)
    let stretch = min(1.0 / max(abs(dot(normalize(vec3<f32>(-sf.g.x, 1.0, -sf.g.y)), rd)), 0.06), 16.0);
    let fpf = fp_iso * stretch;
    let br = breakers(p3.xz, true);
    let g = sf.g + br.g;
    let jac = sf.jac;
    let jac_was = sf.jac_was;
    // Detail too small for the finest band: three moving noise octaves (60,
    // 20 and 7 cm) along the wind, faded by the footprint, carrying half of
    // the unresolved slope as real facets (the sun breaks into sparkles).
    let ph = fract(u.tl * 40.0);
    let wgt = abs(2.0 * ph - 1.0);
    let wdir = RW * 0.6;
    // (Their slopes run along the wind, mostly: the footprint they fade by is
    // the ellipse's reach along the wind, and a share of its reach across.)
    let cwd = abs(dot(RW, fdir));
    let swd = sqrt(max(1.0 - cwd * cwd, 0.0));
    let fmax = max(length(vec2<f32>(fp_iso * swd, fpf * cwd)), FD_RIPPLE * length(vec2<f32>(fp_iso * cwd, fpf * swd)));
    let o1 = 1.0 - smoothstep(0.08, 0.25, fmax);
    let o2 = 1.0 - smoothstep(0.025, 0.08, fmax);
    let o3 = 1.0 - smoothstep(0.008, 0.027, fmax);
    // (In the wind's frame and drawn out across it, as wind ripples are: long
    // crests across the wind, slopes mostly along it, so the sun's glints run
    // in streaks, not round dimples.)
    let wfr = vec2<f32>(dot(p3.xz, RW), dot(p3.xz, vec2<f32>(-RW.y, RW.x)));
    var mw = vec2<f32>(0.0);
    for (var j = 0; j < 2; j = j + 1) {
        let f = fract(ph + 0.5 * f32(j));
        let qq = vec2<f32>(wfr.x * 1.6 - length(wdir) * f * 7.5, wfr.y * 1.6 * FD_RIPPLE) + f32(j) * 17.3;
        let wj = select(wgt, 1.0 - wgt, j == 1);
        if (o1 > 0.0) {
            mw = mw + vnoise(qq).yz * (0.8 * o1 * wj);
        }
        if (o2 > 0.0) {
            mw = mw + vnoise(qq * 3.1 + 4.1).yz * (1.6 * o2 * wj);
        }
        if (o3 > 0.0) {
            mw = mw + vnoise(qq * 9.3 + 8.7).yz * (2.7 * o3 * wj);
        }
    }
    mw = vec2<f32>(mw.x, mw.y * FD_RIPPLE) * inverseSqrt(0.5 + 0.5 * FD_RIPPLE * FD_RIPPLE);
    let micro = (RW * mw.x + vec2<f32>(-RW.y, RW.x) * mw.y) / 3.4;
    let mfade = 0.2 * o1 * o1 + 0.35 * o2 * o2 + 0.45 * o3 * o3;
    let vu = max(u.v_unres, 0.0);
    // (The facets always carry a share of the wind's whole slope: with
    // tall waves the bands alone exceed it, and the near water would have
    // no sparkle at all.)
    let vm = max(0.5 * vu, 0.3 * (0.003 + 0.00512 * F_WIND));
    let v_micro = vm * mfade;
    let gm = g + micro * sqrt(vm);
    // (On a breaking lip the light follows the mesh's own curl, with the waves'
    // ripples laid over it: the lip reads as water curling over, not a flat
    // plate mirroring the sky nor smooth glass; its underside, turned down to
    // the viewer, falls into its own shadow.)
    let nb = normalize(vec3<f32>(-gm.x, 1.0, -gm.y));
    let under_lip = smoothstep(0.15, -0.25, ln.y) * step(0.02, bk.x);
    let n_lip = normalize(ln + vec3<f32>(nb.x, 0.0, nb.z));
    let n_raw = normalize(mix(nb, n_lip, max(smoothstep(0.0, 0.5, bk.x), under_lip)));
    // (A shading normal turned away from the eye (at a crest's outline, where
    // the mesh goes edge on, the ripples' slopes can tip it past) would see
    // the water at full grazing: Fresnel near one, the sky mirrored in a
    // one-pixel bright thread along every outline. It is bent back until it
    // faces the eye a little, as the mesh there does.)
    let n = normalize(n_raw + v * max(0.25 - dot(n_raw, v), 0.0));
    // Roughness: base, the unresolved slope (less the facets'), and what
    // the footprint filtered out of the bands (as Ultra-Water's stack).
    let ab = 0.5 * u.glow.w;
    let lowsun = 1.0 - smoothstep(0.1, 0.55, u.sun.y);
    let dusk = 1.0 - smoothstep(0.08, 0.2, u.sun.y);
    // (A setting sun's road stays a road: less of the filtered slope in its
    // lobe, and no extra round roughness at dusk: that made broad white blobs
    // of the low sun; its glitter reaches further down the road instead.)
    let a2 = ab * ab + max(vu - v_micro, 0.0);
    // (What the footprint filtered out widens the lobe: half of it round,
    // half in its own direction (along the view where the view grazes, so the
    // faded ripples leave a sheen running down the view). Only a share of it:
    // all of it turned the sun's glints into broad bright blotches.)
    let lost = (vec2<f32>(0.35 * (sf.lost.x + sf.lost.y)) + 0.7 * sf.lost) * q * q * mix(1.0, 0.45, dusk);
    // (Where the mesh turns edge on to the eye, against how flat water there
    // would face it, the pixel spans the outline of a crest: its normals change
    // across the pixel, so the lobe widens there, as Ultra-Water's specular
    // anti-aliasing does, and the outline is no dotted thread of grazing glare.)
    let outline = 1.0 - smoothstep(0.05, 0.5, abs(dot(ngv, v)) / max(abs(v.y), 0.02));
    // (A curling lip is torn water: rougher than the face it comes off.)
    let alv = clamp(sqrt(vec2<f32>(a2 + 0.12 * outline + 0.08 * smoothstep(0.0, 0.5, bk.x)) + lost), vec2<f32>(0.02), vec2<f32>(1.0));
    let alpha = sqrt(alv.x * alv.y);
    let nov = max(dot(n, v), 1e-3);
    let f90 = max(1.0 - 0.7 * sqrt(alpha), WATER_F0);
    let fres = clamp((WATER_F0 + (f90 - WATER_F0) * pow(1.0 - nov, 5.0)) * u.glow.z, 0.0, 1.0);
    // The sky in it; the rougher the water, the more of the lobe dips under
    // the horizon and sees the next wave's dark face.
    var r = reflect(rd, n);
    let lobe_w = max(alpha, 0.18);
    // (Towards a low sun the lobe sees the bright sky over the horizon: its
    // road stays lit to the horizon.)
    let under_lobe = smoothstep(lobe_w * 0.9, -lobe_w * 0.9, r.y) * (1.0 - 0.85 * pow(max(dot(normalize(vec3<f32>(r.x, max(r.y, 0.0), r.z)), u.sun), 0.0), 6.0) * dusk);
    r.y = abs(r.y);
    let dark = sky(vec3<f32>(0.0, 0.05, 0.0)) * 0.22 + unpack_col(u.cols.w) * 0.6;
    // (A low sun warms the whole sky near it, not only its glitter.)
    // (At dusk the glow round the sun gathers close to it, so its road stands
    // out over darker water either side, as a setting sun's does.)
    let rsky = sky(vec3<f32>(r.x, r.y * r.y, r.z)) + u.sun_col * (pow(max(dot(r, u.sun), 0.0), mix(6.0, 20.0, dusk)) * 0.22 + pow(max(dot(r, u.sun), 0.0), mix(8.0, 40.0, dusk)) * 0.35 * lowsun) * smoothstep(-0.05, 0.1, u.sun.y);
    let warm = mix(vec3<f32>(1.0), normalize(u.sun_col + vec3<f32>(1e-3)) * 1.55, lowsun * 0.3);
    let refl = mix(mix(rsky, sky(vec3<f32>(0.0, 0.35, 0.0)), clamp(alpha * 0.6, 0.0, 0.4)), dark, under_lobe * 0.85) * warm;
    // The sun: GGX (sun as a small area light), Smith visibility, glitter.
    let lsun = u.sun;
    let hv = normalize(v + lsun);
    let nol = max(dot(n, lsun), 0.0);
    let noh = max(dot(n, hv), 0.0);
    let voh = max(dot(v, hv), 0.0);
    let as_ = max(alpha, 0.015);
    let dd = fd_ggx_aniso(n, hv, vec3<f32>(fdir.x, 0.0, fdir.y), max(alv, vec2<f32>(0.015)));
    let fs = WATER_F0 + (1.0 - WATER_F0) * pow(1.0 - voh, 5.0);
    var spec = u.sun_col * (PI * dd * v_smith(nov, nol, as_) * fs * nol) * u.light.x;
    // (The sparkle fades where the view grazes: each pixel spans many
    // facets along the view and the draws would turn into streaks.)
    let glit = glitter(p3.xz, fp_iso, dd, noh, voh, t);
    spec = spec * mix(1.0, glit, 1.0 - smoothstep(mix(6.0, 10.0, dusk), mix(14.0, 16.5, dusk), stretch)) * smoothstep(0.0, 0.015, nov);
    // Highlights stay bright without blowing out to a white sheet (Ultra-
    // Water's tone curve holds them; ours is steeper).
    spec = min(spec, vec3<f32>(60.0));
    spec = mix(0.6, 1.4, dusk) * spec / (1.0 + mix(0.45, 0.2, dusk) * max(spec.x, max(spec.y, spec.z)));
    // The water's body: what is behind, through the water, and the light it
    // scatters back.
    let deep = clamp(unpack_col(u.cols.z), vec3<f32>(0.004), vec3<f32>(1.0));
    let path = select(400.0, max(px.ts - t, 0.0), px.scene);
    let trans = pow(deep, vec3<f32>(min(path, 400.0) / max(u.shape.w, 0.05)));
    let amb = sky(vec3<f32>(0.0, 1.0, 0.0)) * 0.55 + sky(vec3<f32>(0.0, 0.1, 0.0)) * 0.25;
    let sunlit = smoothstep(-0.05, 0.12, lsun.y);
    let scat = unpack_col(u.cols.w);
    // (Thin water at the crests lets more light back than the deep troughs,
    // so the body isn't one flat colour.)
    let wrap = clamp(0.5 + 0.5 * dot(n, normalize(vec3<f32>(lsun.x, max(lsun.y, 0.25), lsun.z))), 0.0, 1.0);
    let depthlight = mix(0.7, 1.45, smoothstep(-1.5, 1.8, sf.h / max(F_HRMS, 1e-3))) * mix(0.45, 1.5, wrap * wrap);
    let inscatter = (scat + vec3<f32>(0.045, 0.026, 0.009) * dusk) * (amb + u.sun_col * ((0.3 + 0.7 * sunlit) * max(lsun.y, 0.0) + 0.3 * lowsun * sunlit)) * depthlight;
    // Crest glow: sunlight through the thin upper part of a tall crest, seen
    // against the sun. Only above the waves' middle and where the surface
    // curves over (a peak, by the curvature), so faces and troughs stay dark.
    let hs = max(F_HRMS * q, 1e-3);
    let up = clamp(sf.h / hs, 0.0, 3.0);
    let peak = smoothstep(0.3, 1.5, sf.peak);
    let hh = 1.8 * up * smoothstep(0.4, 1.6, up) * (0.35 + 0.65 * peak) + max(br.h, 0.0);
    let sss_c = unpack_col(u.glow.x) * 0.1;
    let toward = pow(clamp(dot(lsun, rd), 0.0, 1.0), 4.0);
    // (A curling lip is the thinnest water of all.)
    // (A curling lip is thin, but the crest it curls from is no lamp: on a
    // breaking crest the glow eases off, so the lip and its hollow aren't one
    // flat saturated sheet.)
    let k1 = u.glow.y * min(0.5 * hh + 0.9 * bk.x * (1.0 - smoothstep(0.5, 1.0, bk.x)), 2.5) * toward * pow(0.5 - 0.5 * dot(n, lsun), 3.0) * mix(1.0, 0.3, lowsun) * (1.0 - 0.65 * smoothstep(0.0, 0.4, bk.x));
    let k2 = u.glow.y * 0.025 * nov * nov;
    let sss = (k1 + k2) * sss_c * u.sun_col * sunlit + br.lip * sss_c * u.sun_col * 2.5 * u.glow.y;
    let body_light = (inscatter + sss) * (vec3<f32>(1.0) - trans);
    // Foam: where the surface folds now, and where it folded a moment ago
    // (the folding at the geometry's squeeze, a little stronger: Ultra-
    // Water's Jacobian foam; the threshold param works as the classic
    // sea's, higher gives more).
    let thr = 0.1 + 0.5 * u.foam.y;
    let fold_now = clamp((thr - jac) * 2.6, 0.0, 1.0);
    let fold_was = clamp((thr - jac_was) * 2.6, 0.0, 1.0) * 0.75 * select(0.0, 1.0, level >= 2);
    // Every patch of foam has a cause. Folding makes foam only where a crest
    // stands (the upper half of a wave: a crest going over); folding down in
    // a trough is the water bunching, not breaking, and makes none. Foam
    // from a moment ago stays only if the water then was a crest folding
    // (the textures keep that moment's height: tile C); it is carried with
    // the water since (the rest point follows it).
    let hs0 = max(F_HRMS * q, 1e-3);
    let crest_now = smoothstep(0.0, 0.5, sf.h / hs0);
    let crest_was = select(crest_now, smoothstep(0.0, 0.5, sf.h_was / hs0), level >= 2);
    let now = fold_now * crest_now;
    let was = fold_was * crest_was;
    let windy = mix(0.35, 1.0, smoothstep(10.0, 18.0, F_WIND));
    var foam = max(now, was) * u.foam.x * windy;
    var dbg_r = now * u.foam.x * windy;
    var dbg_b = was * u.foam.x * windy;
    var dbg_w = 0.0;
    let wind_x = vec2<f32>(dot(p3.xz, RW), dot(p3.xz, vec2<f32>(-RW.y, RW.x)));
    // A gale's foam builds up where crests keep breaking and ages into long
    // streaks along the wind (High and Ultra).
    if (F_STREAK > 0.0 && level >= 2) {
        let dvs = 0.03 * sqrt(GRAV / max(u.wind.x, 1e-4));
        let sph = fract(u.tl * 15.0);
        let sw = abs(2.0 * sph - 1.0);
        var streak = 0.0;
        for (var j = 0; j < 2; j = j + 1) {
            let f = fract(sph + 0.5 * f32(j));
            let qq = vec2<f32>(wind_x.x - dvs * 20.0 * f + 31.0 * f32(j), wind_x.y);
            let s1 = vnoise(vec2<f32>(qq.x * 0.04, qq.y * 0.5)).x * 0.65 + vnoise(vec2<f32>(qq.x * 0.11, qq.y * 1.6) + 7.3).x * 0.35;
            streak = streak + s1 * select(sw, 1.0 - sw, j == 1);
        }
        // (The streaks only shape foam an earlier break left: no streak of foam
        // without one.)
        let aged = was * mix(1.0, 0.2 + 1.2 * smoothstep(0.45, 0.85, streak), F_STREAK);
        foam = clamp(max(now, aged) + now * aged * 0.8 * F_STREAK, 0.0, 1.0) * u.foam.x;
        dbg_r = now * u.foam.x;
        dbg_b = max(dbg_b, aged * u.foam.x);
    }
    // The caps ride the waves' upper parts; lower down a quarter stays (old
    // foam in the troughs). Far off they thin out.
    let thin = mix(0.25, 1.0, smoothstep(0.0, 0.6, up)) * (1.0 - 0.6 * smoothstep(250.0, 1500.0, t));
    foam = foam * thin;
    dbg_r = dbg_r * thin;
    dbg_b = dbg_b * thin;
    let thick = path * abs(rd.y);
    let contact = select(0.0, (1.0 - smoothstep(0.0, 0.35 + 0.5 * u.foam.w, thick)) * smoothstep(6.0, 2.0, depth_at(p3.xz)), px.scene);
    let shore = max(br.white * 1.15, contact * 0.9) * u.foam.w;
    foam = max(foam, shore);
    dbg_r = max(dbg_r, shore);
    // A breaking crest: white water where the lip goes over and falls, and
    // the patch it leaves behind (High and Ultra), fading as the wave moves on.
    var white = 0.0;
    let chance = brk_chance();
    if (chance > 0.0 && level >= 1) {
        // The lip whitens as it goes over; the collapse spills white down the
        // wave's front.
        white = max(smoothstep(0.3, 0.9, bk.x), smoothstep(0.05, 0.8, bk.y));
        // (A lip seen edge on thins to spray at its rim: no hard white plate.)
        white = white * mix(0.35, 1.0, smoothstep(0.05, 0.4, abs(dot(ngv, rd))));
        // (Its edge torn along the crest, not cut straight: the white comes in
        // as the lip curls over, in tongues.)
        white = white * smoothstep(0.15, 0.55, vnoise(vec2<f32>(dot(p3.xz, vec2<f32>(-RW.y, RW.x)) * 0.35, dot(p3.xz, RW) * 0.9) + 3.7).x + 0.6 * bk.x);
        if (level >= 2) {
            let pr = brk_params();
            // (Thin on a crest's back seen edge on, where the crest's own outline
            // would cut it into a hard white plate; elsewhere it streaks.)
            let edge_on = mix(0.6, 1.0, smoothstep(0.1, 0.45, abs(dot(ngv, rd)) / max(-rd.y, 0.03)));
            // The foam the break left: only where a crest really went over,
            // along that crest where it fell, carried back as the wave moves
            // on, fading (`brk_foam_src`: young, old).
            let src = brk_foam_src(p3.xz, chance, pr, 3.0 * u.foam.z + 3.0, fbands(u32(level)));
            foam = max(foam, 0.45 * edge_on * max(src.x, src.y));
            dbg_b = max(dbg_b, 0.45 * edge_on * src.x);
            dbg_w = max(dbg_w, 0.45 * edge_on * src.y);
        }
        dbg_r = max(dbg_r, white);
    }
    // Whatever foam no cause above accounts for (none: the view shows it
    // yellow if it ever appears).
    F_DBG = vec4<f32>(dbg_r, dbg_b, dbg_w, foam - min(dbg_r + dbg_b + dbg_w, 1.0) - 1e-3);
    var cov = 0.0;
    // Lace with clear water through it, however much foam lies here.
    var lace_p = 0.5;
    if (foam > 0.05) {
        let f = min(clamp(foam * 1.5, 0.0, 1.0), 0.55);
        lace_p = fd_lace(wind_x, fp_iso);
        cov = smoothstep(0.82 - f, 1.12 - f, lace_p) * (0.35 + 0.65 * f) * smoothstep(0.05, 0.3, f);
        // (Close to, foam is flecks and threads with water between them.)
        if (t < 40.0 && cov > 0.0) {
            let flecks = 0.6 * vnoise(wind_x * 6.1 + 4.7).x + 0.4 * vnoise(wind_x * 17.0 + 9.2).x;
            cov = cov * mix(smoothstep(0.34, 0.6, flecks), 1.0, smoothstep(10.0, 40.0, t));
        }
    }
    // White water tears into clots a metre or two across, drawn out along the
    // wind, with finer ones in it while the pixel can resolve them: its edge is
    // ragged and soft, and even at its thickest some water shows through.
    var clots = 0.0;
    if (white > 0.01) {
        let fine_k = 1.0 - smoothstep(0.1, 0.5, fp_iso);
        let wq = vec2<f32>(wind_x.x * 0.55, wind_x.y);
        let streaks = vnoise(vec2<f32>(wq.x * 0.25, wq.y * 2.2) + 6.3).x;
        // (On the curl itself the streaks give way to round clots: white water
        // tumbling off a lip, not even stripes down it.)
        let on_lip = smoothstep(0.2, 0.8, bk.x);
        clots = 0.28 * vnoise(wq * 0.5 + 3.1).x + 0.2 * vnoise(wq * 1.4 + 7.7).x + 0.17 * mix(0.5, vnoise(wq * 4.3 + 1.9).x, fine_k) + 0.35 * mix(streaks, vnoise(wind_x * 1.9 + 5.3).x, 0.7 * on_lip);
        // (Solid where the crest has just collapsed; further down the face, as
        // the white thins, only its streaks along the wind and then flecks.)
        // (Even at its thickest the white water tears: about a quarter of the
        // curl shows through it as water, so a lip is never one pale plate.)
        var wc = smoothstep(1.0 - 0.7 * white, 1.25 - 0.7 * white, clots);
        if (t < 40.0) {
            let fl = 0.6 * vnoise(wind_x * 6.1 + 4.7).x + 0.4 * vnoise(wind_x * 17.0 + 9.2).x;
            wc = wc * mix(smoothstep(0.3, 0.58, fl), 1.0, smoothstep(10.0, 40.0, t));
        }
        cov = max(cov, wc * (0.65 + 0.2 * white));
    }
    let solid = smoothstep(0.55, 0.95, max(foam, white));
    let alb = mix(vec3<f32>(0.8, 0.86, 0.88), vec3<f32>(0.95), solid);
    let fol = max(dot(normalize(n + vec3<f32>(0.0, 0.8, 0.0)), lsun), 0.0);
    // (White water has body: its clots shade, its deep folds darken.)
    let foam_lit = (alb * (amb * 1.1 + u.sun_col * (0.25 + 0.75 * fol) * max(lsun.y, 0.0) * 0.85)
        + u.sun_col * pow(clamp(dot(lsun, rd), 0.0, 1.0), 4.0) * 0.4 * (1.0 - solid) * sunlit) * select(1.0, mix(0.8, 1.0, solid), F_MID > 0.0) * select(0.72 + 0.56 * lace_p, 0.72 + 0.5 * clots, white > 0.01);
    // The far haze (Ultra-Water's 18 km).
    let haze = 1.0 - exp(-t / 18000.0);
    let hz = sky(vec3<f32>(rd.x, 0.02, rd.z));
    // Spray mist over the beach's breaking lips (Ultra).
    var mist = 0.0;
    let spray_k = brk_spray();
    if (level >= 3 && spray_k > 0.0 && br.brk > 0.0) {
        for (var j = 0; j < 4; j = j + 1) {
            let tj = t - (0.4 + 0.9 * f32(j));
            if (tj <= 0.0) {
                continue;
            }
            let qp = ro + rd * tj;
            let hb = breakers(qp.xz, true);
            let above_w = qp.y - u.centre.y - hb.h;
            let spp = fract(u.tl * 100.0);
            let nz = vnoise(qp.xz * 1.3 + vec2<f32>(0.0, spp * 5.0)).x * vnoise(vec2<f32>(qp.y * 2.0 - spp * 7.0, qp.x * 0.7)).x * (1.0 - abs(2.0 * spp - 1.0))
                + vnoise(qp.xz * 1.3 + vec2<f32>(9.0, fract(spp + 0.5) * 5.0)).x * vnoise(vec2<f32>(qp.y * 2.0 - fract(spp + 0.5) * 7.0, qp.x * 0.7 + 4.0)).x * abs(2.0 * spp - 1.0);
            mist = mist + hb.brk * max(hb.lip, 0.35 * hb.white) * exp(-max(above_w, 0.0) / 1.4) * smoothstep(0.12, 0.55, nz) * 0.7;
        }
        mist = clamp(mist * spray_k, 0.0, 1.0);
    }
    // Spindrift (High and Ultra): spray torn off the crests that are breaking
    // now, on their upper part, blown downwind in streaks at the wind's
    // speed; brighter towards the sun.
    if (F_DRIFT > 0.0 && level >= 2) {
        let top = smoothstep(1.0, 2.0, up);
        let brk = clamp((thr + 0.05 - jac) * 3.0, 0.0, 1.0);
        if (brk * top > 0.0) {
            let vw = 0.9 * sqrt(GRAV / max(u.wind.x, 1e-4));
            let sph = fract(u.tl * 100.0);
            let sw = abs(2.0 * sph - 1.0);
            var spv = 0.0;
            for (var j = 0; j < 2; j = j + 1) {
                let f = fract(sph + 0.5 * f32(j));
                let qq = vec2<f32>(wind_x.x - vw * 3.0 * f + 17.0 * f32(j), wind_x.y);
                let nn = vnoise(vec2<f32>(qq.x * 0.35, qq.y * 1.4)).x * 0.6 + vnoise(vec2<f32>(qq.x * 1.1, qq.y * 3.7) + 3.1).x * 0.4;
                spv = spv + nn * select(sw, 1.0 - sw, j == 1);
            }
            mist = max(mist, clamp(F_DRIFT * brk * top * smoothstep(0.38, 0.74, spv) * 0.45, 0.0, 0.7));
        }
    }
    let mist_c = amb * 1.2 + u.sun_col * (0.4 + 0.6 * pow(clamp(dot(lsun, rd), 0.0, 1.0), 2.0)) * max(lsun.y, 0.0) * 0.8;
    // Caustics on what is seen through shallow water (High and Ultra), from
    // the curvature of the bands near the bed's scale.
    if (level >= 2 && u.light.w > 0.0 && px.scene) {
        let bed = u.eye + px.rd * px.ts;
        let dep = u.centre.y - bed.y;
        if (dep > 0.02) {
            let kc = dep * (1.0 - 1.0 / 1.33);
            let raw = clamp(1.0 / max(abs(1.0 - 0.5 * kc * sf.lap), 0.06), 0.0, 6.0);
            let cg = mix(1.0, raw, exp(-dep / 7.0) * smoothstep(0.0, 0.3, u.sun.y));
            o.caustic = mix(1.0, cg, u.light.w);
        }
    }
    let keep_c = (1.0 - fres) * (1.0 - cov);
    o.m = keep_c * trans * (1.0 - haze) * (1.0 - mist);
    var add = (1.0 - fres) * body_light + fres * refl + spec;
    add = add * (1.0 - cov) + foam_lit * cov;
    // Under a lip: its underside in its own shadow, the sky hidden by it, its
    // ripples and the light through the water still there.
    if (bk.x > 0.02) {
        let und = smoothstep(-0.02, -0.3, ln.y);
        add = add * mix(1.0, 0.6, und) + (sss * 0.5 + inscatter * 0.6) * und;
    }
    add = mix(add, hz, haze);
    add = mix(add, mist_c, mist);
    o.a = add;
    let tilt = n - vec3<f32>(0.0, 1.0, 0.0);
    o.off = screen_bend(tilt) * u.light.z * clamp(thick * 0.25, 0.0, 1.0) * 0.06 / max(t * 0.02, 1.0);
    return o;
}

@fragment
fn fs_main(in: MeshOut) -> @location(0) vec4<f32> {
    // (Worked out first: derivatives need uniform control flow.)
    let wdx = dpdx(in.wp);
    let wdy = dpdy(in.wp);
    let px_i = vec2<i32>(in.pos.xy);
    let size = vec2<f32>(textureDimensions(depth_tex));
    let uv = in.pos.xy / size;
    let z = textureLoad(depth_tex, px_i, 0);
    let px = pixel(uv, z);
    let t = length(in.wp - u.eye);
    // The scene in front of the water keeps its pixel; so does what lies
    // outside the sea.
    if ((px.scene && t > px.ts) || !inside(in.wp.xz)) {
        discard;
    }
    fd_setup();
    var s: Shade;
    if (F_UNDER) {
        s = under(uv, z);
    } else {
        // The facet's own normal, turned to the viewer (a lip's underside
        // points down).
        var ngv = cross(wdx, wdy);
        let lg = length(ngv);
        ngv = select(vec3<f32>(0.0, 1.0, 0.0), ngv / max(lg, 1e-9), lg > 1e-9);
        if (dot(ngv, u.eye - in.wp) < 0.0) {
            ngv = -ngv;
        }
        s = fd_shade(in.p0, in.wp, px, in.bk, ngv, normalize(in.ln));
    }
    var buv = uv;
    if (s.t > 0.0) {
        // Bend only onto what lies behind the surface with about as much
        // water in front of it as in front of this pixel's own scene (as
        // the classic sea does: no bright rims past an object's edge).
        let bent = clamp(uv + s.off, vec2<f32>(0.0), vec2<f32>(1.0));
        let p0 = px.ts - s.t;
        let b0 = vec2<i32>(floor(bent * size - 0.5));
        let hi = vec2<i32>(size) - 1;
        var fits = true;
        for (var k = 0; k < 4; k = k + 1) {
            let qd = clamp(b0 + vec2<i32>(k & 1, k >> 1u), vec2<i32>(0), hi);
            let pb = pixel((vec2<f32>(qd) + 0.5) / size, textureLoad(depth_tex, qd, 0)).ts - s.t;
            fits = fits && pb > s.t * 0.02 && abs(pb - p0) < 0.35 * p0 + 0.5;
        }
        if (fits) {
            buv = bent;
        }
    }
    let c = textureSampleLevel(scene_tex, samp, buv, 0.0).rgb;
    return vec4<f32>(fd_debug_out(c * s.m * s.caustic + s.a), 1.0);
}
