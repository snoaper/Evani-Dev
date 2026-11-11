// evani.shader.nature_sea_waves
// The field sea's wave textures (`material.nature.water.wave_field`, and
// every storm sea): drawn once a frame into the small atlas
// `nature.sea.field` (layout: sea_field_bands.wgsl), before the field sea
// reads it in its mesh (sea_field.wgsl). Each texel sums its band's
// components of the field table (sea_field_table.wgsl) at its point of the
// band's periodic tile, every wave vector snapped onto the tile's lattice:
// the height, the sideways squeeze towards the crests (Gerstner, so crests
// stand peaked and troughs broad), the slope and the folding tensor (the
// pixel works out the whole surface's slope and folding from all bands),
// the folding and the height a moment ago and the curvature. So the
// cost of the waves is this draw's few hundred thousand texels, whatever
// the waves' height, and the sea draw reads a few texels a pixel.
//
// The idea of precomputed wave cascades read by a displaced mesh, and the
// short-wave fade, are Ultra-Water's (Humpalumps, MIT; DECISIONS.md "Field
// sea"); this is our own code and a direct sum, not an FFT.

struct VsOut {
    @builtin(position) pos: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: VsOut;
    o.pos = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    return o;
}

// The sea's uniform words (sea_common.wgsl's `Sea`; the field sea reads
// swell.z as the squeeze, swell.w as the longest wave, shape.x as the
// foam's folding: src/sea/field.rs `field_words`).
struct Sea {
    eye: vec3<f32>,
    inv0: f32,
    fwd: vec3<f32>,
    inv1: f32,
    right: vec3<f32>,
    tl: f32,
    up: vec3<f32>,
    t: f32,
    sun: vec3<f32>,
    px: f32,
    sun_col: vec3<f32>,
    sky_tint: f32,
    centre: vec3<f32>,
    level: f32,
    half: vec2<f32>,
    v_unres: f32,
    spray: f32,
    wind: vec4<f32>,
    swell: vec4<f32>,
    shape: vec4<f32>,
    cols: vec4<f32>,
    glow: vec4<f32>,
    light: vec4<f32>,
    foam: vec4<f32>,
    beach: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Sea;

const TAU: f32 = 6.28318531;
const GRAV: f32 = 9.81;
const W0: f32 = 0.0209439510;
// The short waves' fade on their amplitude, exp(-FIELD_FADE k^2): Ultra-Water's
// `shortWavesFade` 0.04 m on the energy (MIT, Humpalumps), so 0.0008 on the
// amplitude (it was twice that, and wave faces close to the eye lost their
// centimetre ripples). src/sea/field.rs `FIELD_FADE`.
const FIELD_FADE: f32 = 0.0008;

// Component i in band b with tile length L, or a zero amplitude when it
// lies in another band (or the level leaves it out): (snapped wave vector,
// amplitude, phase now); `.w` of the second: angular frequency.
struct FComp {
    k: vec2<f32>,
    a: f32,
    ph: f32,
    om: f32,
    // The squeeze's share (`f_chop`).
    cf: f32,
};

fn fcomp(i: u32, b: u32, l: f32, level: u32, mid: f32) -> FComp {
    var o = FComp(vec2<f32>(0.0), 0.0, 0.0, 0.0, 1.0);
    let c = FIELD_C[i];
    let swell = i >= FIELD_NW;
    // Medium and Low read every other wind component (twice the energy).
    if (level < 2u && !swell && (i & 1u) == 1u) {
        return o;
    }
    let kp = select(u.wind.x, u.wind.w, swell);
    let k = kp * c.x;
    let lam = TAU / max(k, 1e-6);
    let hi = fband_hi(b);
    if ((b > 0u && lam > hi) || lam <= hi / F_RHO) {
        return o;
    }
    let hd = select(u.wind.z, u.swell.y, swell);
    let r = vec2<f32>(cos(hd), sin(hd));
    let d = vec2<f32>(r.x * c.y - r.y * c.z, r.y * c.y + r.x * c.z);
    let dk = TAU / l;
    let ks = round(d * k / dk) * dk;
    let km = length(ks);
    if (km <= 0.0) {
        return o;
    }
    var a = select(u.wind.y, u.swell.x, swell) * c.w * exp(-FIELD_FADE * km * km);
    if (!swell) {
        // Storm seas' mid waves (k / k_p 2.56..36): raised by mid_waves.
        let g = select(1.0, mid, mid > 0.0);
        a = a * (1.0 + (g - 1.0) * smoothstep(2.0, 2.56, c.x) * (1.0 - smoothstep(36.0, 46.0, c.x)));
        // Waves under about 40 m stand a third taller (measured against
        // Ultra-Water by wavelength: tools/nature/uw-probe.mjs; src/sea/field.rs).
        a = a * (1.0 + 0.35 * smoothstep(0.1, 0.4, km));
        a = a * select(1.0, 1.41421356, level < 2u);
    }
    let n = max(round(sqrt(GRAV * km) / W0), 1.0);
    o.k = ks;
    o.a = a;
    o.ph = TAU * (FIELD_P[i] - fract(n * u.tl));
    o.om = n * W0;
    o.cf = f_chop(c.x);
    return o;
}

// What every component of band b shares in a texel of its tile (length l):
// worked out once a texel, not once a component.
struct FCtx {
    hi: f32,
    lo: f32,
    dk: f32,
    rw: vec2<f32>,
    rs: vec2<f32>,
    gm: f32,
    lv: f32,
    b0: bool,
};

fn fctx(b: u32, l: f32, level: u32, mid: f32) -> FCtx {
    let hi = fband_hi(b);
    return FCtx(hi, hi / F_RHO, TAU / l, vec2<f32>(cos(u.wind.z), sin(u.wind.z)), vec2<f32>(cos(u.swell.y), sin(u.swell.y)), select(1.0, mid, mid > 0.0), select(1.0, 1.41421356, level < 2u), b == 0u);
}

// `fcomp` with the band's shared numbers read from `x` (the same component).
fn fcomp_in(i: u32, x: FCtx, level: u32) -> FComp {
    var o = FComp(vec2<f32>(0.0), 0.0, 0.0, 0.0, 1.0);
    let c = FIELD_C[i];
    let swell = i >= FIELD_NW;
    if (level < 2u && !swell && (i & 1u) == 1u) {
        return o;
    }
    let k = select(u.wind.x, u.wind.w, swell) * c.x;
    let lam = TAU / max(k, 1e-6);
    if ((!x.b0 && lam > x.hi) || lam <= x.lo) {
        return o;
    }
    let r = select(x.rw, x.rs, swell);
    let d = vec2<f32>(r.x * c.y - r.y * c.z, r.y * c.y + r.x * c.z);
    let ks = round(d * k / x.dk) * x.dk;
    let km = length(ks);
    if (km <= 0.0) {
        return o;
    }
    var a = select(u.wind.y, u.swell.x, swell) * c.w * exp(-FIELD_FADE * km * km);
    if (!swell) {
        a = a * (1.0 + (x.gm - 1.0) * smoothstep(2.0, 2.56, c.x) * (1.0 - smoothstep(36.0, 46.0, c.x)));
        a = a * (1.0 + 0.35 * smoothstep(0.1, 0.4, km));
        a = a * x.lv;
    }
    let n = max(round(sqrt(GRAV * km) / W0), 1.0);
    o.k = ks;
    o.a = a;
    o.ph = TAU * (FIELD_P[i] - fract(n * u.tl));
    o.om = n * W0;
    o.cf = f_chop(c.x);
    return o;
}

// The first of the table's components s0..s1 (sorted by k / k_p, longest
// wave first) with k / k_p of `x` or more.
fn f_first(s0: u32, s1: u32, x: f32) -> u32 {
    var lo = s0;
    var hi = s1;
    loop {
        if (lo >= hi) {
            break;
        }
        let mid = (lo + hi) / 2u;
        if (FIELD_C[mid].x < x) {
            lo = mid + 1u;
        } else {
            hi = mid;
        }
    }
    return lo;
}

// The components that can fall in band b, of the wind set or the swell's
// (a little wider than the band: `fcomp` keeps exactly the band's own). A
// texel reads its band's few dozen, not the whole table.
fn f_range(b: u32, swell: bool) -> vec2<u32> {
    let kp = max(select(u.wind.x, u.wind.w, swell), 1e-6);
    let hi = fband_hi(b);
    let s0 = select(0u, FIELD_NW, swell);
    let s1 = select(FIELD_NW, FIELD_N, swell);
    let xlo = select(TAU / (kp * hi) * 0.999, 0.0, b == 0u);
    let xhi = TAU / (kp * hi / F_RHO) * 1.001;
    return vec2<u32>(f_first(s0, s1, xlo), f_first(s0, s1, xhi));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let px = vec2<u32>(in.pos.xy);
    let word = u32(max(u.level, 0.0) + 0.5);
    let level = word & 3u;
    let mid = f32((word >> 2u) & 127u) * 0.02;
    let nb = fbands(level);
    if (px.y >= F_INFO_Y) {
        // The band numbers: x = b one band, x = FB all of them.
        let all = px.x == FB;
        if (px.x > FB) {
            return vec4<f32>(0.0);
        }
        var v = vec4<f32>(0.0);
        for (var b = 0u; b < nb; b = b + 1u) {
            if (!all && b != px.x) {
                continue;
            }
            let l = ftile(b);
            for (var i = 0u; i < FIELD_N; i = i + 1u) {
                let c = fcomp(i, b, l, level, mid);
                if (c.a <= 0.0) {
                    continue;
                }
                v = v + 0.5 * c.a * c.a * vec4<f32>(c.k.x * c.k.x, c.k.y * c.k.y, 1.0, c.k.x * c.k.y);
            }
        }
        return v;
    }
    let b = px.y / FT;
    let kind = min(px.x / FT, 2u);
    // Bands the level leaves out, and tile C below High: nothing there (a
    // folding of 1 in tile C: no foam from it).
    if (b >= nb || (kind == 2u && level < 2u)) {
        return select(vec4<f32>(0.0), vec4<f32>(1.0, 0.0, 0.0, 0.0), kind == 2u);
    }
    let j = (vec2<i32>(i32(px.x % FT), i32(px.y % FT)) - 1 + i32(FN)) % i32(FN);
    let l = ftile(b);
    let p = vec2<f32>(j) * (l / f32(FN));
    let lam = u.swell.z;
    let lamf = u.shape.x;
    let linger = max(u.foam.z, 0.05);
    let x = fctx(b, l, level, mid);
    let rw = f_range(b, false);
    let rs = f_range(b, true);
    let nw = rw.y - rw.x;
    let nt = nw + rs.y - rs.x;
    // (Each tile kind sums only the numbers it stores: the three tiles of a
    // band share their components, not their sums.)
    if (kind == 0u) {
        var dsp = vec2<f32>(0.0);
        var h = 0.0;
        var mz = 0.0;
        for (var t = 0u; t < nt; t = t + 1u) {
            let c = fcomp_in(select(rw.x + t, rs.x + t - nw, t >= nw), x, level);
            if (c.a <= 0.0) {
                continue;
            }
            let th = dot(c.k, p) + c.ph;
            let sn = sin(th);
            let km = length(c.k);
            h = h + c.a * sn;
            dsp = dsp + c.k * (c.cf * c.a * cos(th) / km);
            mz = mz + c.k.x * c.k.y / km * (c.cf * c.a * sn);
        }
        return vec4<f32>(lam * dsp.x, h, lam * dsp.y, mz);
    }
    if (kind == 1u) {
        var g0 = vec2<f32>(0.0);
        var m = vec2<f32>(0.0);
        for (var t = 0u; t < nt; t = t + 1u) {
            let c = fcomp_in(select(rw.x + t, rs.x + t - nw, t >= nw), x, level);
            if (c.a <= 0.0) {
                continue;
            }
            let th = dot(c.k, p) + c.ph;
            let km = length(c.k);
            g0 = g0 + c.k * (c.a * cos(th));
            m = m + c.k * c.k / km * (c.cf * c.a * sin(th));
        }
        return vec4<f32>(g0, m.x, m.y);
    }
    var mw = vec3<f32>(0.0);
    var lap = 0.0;
    var hw = 0.0;
    for (var t = 0u; t < nt; t = t + 1u) {
        let c = fcomp_in(select(rw.x + t, rs.x + t - nw, t >= nw), x, level);
        if (c.a <= 0.0) {
            continue;
        }
        let th = dot(c.k, p) + c.ph;
        let km = length(c.k);
        lap = lap - km * km * c.a * sin(th);
        let sw = sin(th - c.om * linger);
        mw = mw + vec3<f32>(c.k.x * c.k.x, c.k.y * c.k.y, c.k.x * c.k.y) / km * (c.cf * c.a * sw);
        hw = hw + c.a * sw;
    }
    let jw = (1.0 - lamf * mw.x) * (1.0 - lamf * mw.y) - lamf * lamf * mw.z * mw.z;
    return vec4<f32>(jw, lap, hw, 0.0);
}
