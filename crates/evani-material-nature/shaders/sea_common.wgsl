struct Sea {
    // Camera position; 1 / view distance = inv0 + inv1 * depth.
    eye: vec3<f32>,
    inv0: f32,
    fwd: vec3<f32>,
    inv1: f32,
    // ray = fwd + right * ndc.x + up * ndc.y. tl: the sim time in turns of
    // the 300 s loop the sea repeats over.
    right: vec3<f32>,
    tl: f32,
    // t: sim seconds (times wave speed), wrapped, for the moving detail.
    up: vec3<f32>,
    t: f32,
    // Unit vector towards the sun; radians per pixel.
    sun: vec3<f32>,
    px: f32,
    sun_col: vec3<f32>,
    sky_tint: f32,
    // The sea's centre (y: the water level); level: quality (0 Low, 1
    // Medium, 2 High, 3 Ultra).
    centre: vec3<f32>,
    level: f32,
    // Half size (x, z); the unresolved slope variance (Cox-Munk for the wind
    // less what the components carry); spray strength.
    half: vec2<f32>,
    v_unres: f32,
    spray: f32,
    // Wind sea: peak wave number, height scale (m), heading (rad); swell
    // peak wave number.
    wind: vec4<f32>,
    // Swell height scale (m), heading (rad); the crest profile's sharpness s
    // and its mean m.
    swell: vec4<f32>,
    // Profile normaliser, sea depth (m), choppiness, clarity (m).
    shape: vec4<f32>,
    // Packed colours (sqrt-encoded 8-bit): sky horizon, zenith, deep, scatter.
    cols: vec4<f32>,
    // Crest glow colour (packed), its strength, reflection, roughness.
    glow: vec4<f32>,
    // Sun glint, glitter, refraction, caustics.
    light: vec4<f32>,
    // Whitecap amount and threshold, how long foam lingers (s), shore foam.
    foam: vec4<f32>,
    // Beach slope (rise per metre, 0: no beach), distance from the centre
    // to the waterline (m), heading towards land (rad), breakers packed as
    // 8 floor(wavelength m) + height (m).
    beach: vec4<f32>,
};

const PI: f32 = 3.14159265;
const TAU: f32 = 6.28318531;
const GRAV: f32 = 9.81;
const FAR_T: f32 = 1.0e9;
// The sea repeats every 300 s: each frequency is a whole multiple of W0.
const W0: f32 = 0.0209439510;
const WATER_F0: f32 = 0.02;

// ---- per-pixel setup ------------------------------------------------------

struct Lv {
    // This level's height set: start, wind and swell components; how many
    // detail components; march steps (the envelope's, then the waves') and
    // refinements.
    base: u32,
    gw: u32,
    gs: u32,
    nd: u32,
    coarse: i32,
    steps: i32,
    bisect: i32,
};

fn quality() -> i32 {
    let l = u32(max(u.level, 0.0) + 0.5);
    return i32(l & 3u);
}

fn lv() -> Lv {
    let q = u32(quality());
    let coarse = array<i32, 4>(0, 32, 40, 48);
    let steps = array<i32, 4>(0, 16, 16, 20);
    let bis = array<i32, 4>(0, 6, 7, 8);
    return Lv(SEA_GEO_BASE[q], SEA_GEO_W[q], SEA_GEO_S[q], SEA_DET_N[q], coarse[q], steps[q], bis[q]);
}

// The height set this pixel: (k dir.x, k dir.z, amplitude, phase) and k.
var<private> GW: array<vec4<f32>, 14>;
var<private> GK: array<f32, 14>;
var<private> NG: u32;
// Height bounds above and below the level, the slope bound.
var<private> H_HI: f32;
var<private> H_LO: f32;
var<private> SLOPE_MAX: f32;
// The open sea's own trough bound (without the breakers).
var<private> AMP_LO: f32;
// Headings of the wind sea and the swell (cos, sin).
var<private> RW: vec2<f32>;
var<private> RS: vec2<f32>;
// The choppy squeeze: points move towards the crests by up to this times
// each wave's amplitude (Gerstner), so crests stand sharp and troughs flat.
var<private> LAMBDA: f32;
// The longest waves of the set (they carry most of the height and all of
// the squeeze): the envelope march reads only these, plus a margin that
// bounds the rest, so its steps are long and cheap.
var<private> GB: array<vec4<f32>, 5>;
var<private> GBK: array<f32, 5>;
var<private> NB: u32;
var<private> ENV_M: f32;
var<private> ENV_B: f32;
var<private> ENV_L: f32;

fn unpack_col(p: f32) -> vec3<f32> {
    let q = u32(max(p, 0.0) + 0.5);
    let v = vec3<f32>(f32(q & 255u), f32((q >> 8u) & 255u), f32((q >> 16u) & 255u)) / 255.0;
    return v * v;
}

fn sky_k() -> vec3<f32> {
    let p = u32(max(u.sky_tint, 0.0) + 0.5);
    return vec3<f32>(f32(p & 255u), f32((p >> 8u) & 255u), f32((p >> 16u) & 255u)) * (4.0 / 255.0);
}

// Component i of the table for this frame: (k dir, amplitude, phase in
// radians) at time offset `dt` seconds (0 = now).
fn comp(i: u32, swell: bool, dt: f32) -> vec4<f32> {
    let c = SEA_C[i];
    let kp = select(u.wind.x, u.wind.w, swell);
    let a0 = select(u.wind.y, u.swell.x, swell);
    let r = select(RW, RS, swell);
    let k = kp * c.x;
    let d = vec2<f32>(r.x * c.y - r.y * c.z, r.y * c.y + r.x * c.z);
    let n = round(sqrt(GRAV * k) / W0);
    let turns = fract(n * u.tl) + n * W0 * dt / TAU;
    // Waves much shorter than a few centimetres fade (Ultra-Water's short
    // wave fade): keeps the far field glossy, not brushed metal.
    let fade = exp(-0.0016 * k * k);
    return vec4<f32>(d * k, a0 * c.w * fade, TAU * (SEA_P[i] - turns));
}

fn comp_k(i: u32, swell: bool) -> f32 {
    return select(u.wind.x, u.wind.w, swell) * SEA_C[i].x;
}

// The crest profile and its first and second derivatives at sin, cos:
// exp(s (sin - 1)), centred and scaled to a sine's variance.
fn prof(sn: f32, cs: f32) -> vec3<f32> {
    let s = u.swell.z;
    let e = exp(s * (sn - 1.0));
    let k = u.shape.x;
    return vec3<f32>((e - u.swell.w) * k, s * cs * e * k, s * e * (s * cs * cs - sn) * k);
}

// The slope bound of what the footprint fp keeps: the sum of a k over the
// kept components, times the profile's and the squeeze's steepening, plus
// the breakers'. Far out only the long waves stay, so steps grow.
var<private> SLOPE_K: f32;

// The open sea's slope bound for what footprint fp keeps (the breakers'
// is added only over the surf zone, see `surf_span`).
fn slope_bound(fp: f32) -> f32 {
    var s = 0.0;
    for (var i = 0u; i < NG; i = i + 1u) {
        s = s + GW[i].z * GK[i] * keep(GK[i], fp);
    }
    return s * SLOPE_K;
}

// Largest |d prof / d theta|: max of |cos| exp(s (sin - 1)) times s k.
fn prof_slope_max() -> f32 {
    let s = max(u.swell.z, 1e-3);
    let y = (-1.0 + sqrt(1.0 + 4.0 * s * s)) / (2.0 * s);
    return s * u.shape.x * sqrt(max(1.0 - y * y, 0.0)) * exp(s * (y - 1.0));
}

fn setup() {
    RW = vec2<f32>(cos(u.wind.z), sin(u.wind.z));
    RS = vec2<f32>(cos(u.swell.y), sin(u.swell.y));
    let l = lv();
    var n = 0u;
    var amp = 0.0;
    var slope = 0.0;
    var nb = 0u;
    var big_amp = 0.0;
    var big_slope = 0.0;
    var rest = 0.0;
    for (var i = 0u; i < l.gw + l.gs; i = i + 1u) {
        let swell = i >= l.gw;
        let c = comp(l.base + i, swell, 0.0);
        let k = comp_k(l.base + i, swell);
        GW[n] = c;
        GK[n] = k;
        amp = amp + c.z;
        slope = slope + c.z * k;
        // The longest three of the wind sea and two of the swell.
        if ((!swell && i < 3u) || (swell && i < l.gw + 2u)) {
            GB[nb] = c;
            GBK[nb] = k;
            nb = nb + 1u;
            big_amp = big_amp + c.z;
            big_slope = big_slope + c.z * k;
        } else {
            rest = rest + c.z;
        }
        n = n + 1u;
    }
    NG = n;
    NB = nb;
    let pmax = (1.0 - u.swell.w) * u.shape.x;
    let pmin = (u.swell.w - exp(-2.0 * u.swell.z)) * u.shape.x;
    let br = breaker_bound();
    AMP_LO = amp * pmin;
    H_HI = amp * pmax + br.x;
    H_LO = AMP_LO + br.y;
    // The squeeze (from the longest waves) never folds the surface over:
    // lambda sum(a k) stays at or under 0.6 however tall the waves are set
    // (real waves break first), so it steepens slopes by at most 2.5x.
    LAMBDA = min(0.9 * u.shape.z, 0.6 / max(big_slope, 1e-6));
    let squeeze = 1.0 / max(1.0 - LAMBDA * big_slope, 0.2);
    SLOPE_K = prof_slope_max() * squeeze;
    SLOPE_MAX = slope * SLOPE_K + br.z;
    // The envelope: the longest waves where the ray is, plus every other
    // wave at its crest, plus how far the squeeze can shift the longest
    // ones (their slope times the largest shift), plus the breakers.
    let pslope = prof_slope_max();
    ENV_M = rest * pmax + big_slope * pslope * LAMBDA * big_amp;
    ENV_B = br.x;
    // Its slope: the longest waves' (unsqueezed) and the depth's change
    // over a beach.
    let dq = select(0.0, 0.45 * max(u.beach.x, 0.0) / max(AMP_LO, 1e-4), u.beach.x > 0.0);
    ENV_L = big_slope * pslope + dq * (big_amp * pmax + ENV_M);
}

// ---- depth and the beach -----------------------------------------------------

fn land_dir() -> vec2<f32> {
    return vec2<f32>(cos(u.beach.z), sin(u.beach.z));
}

// Metres seaward of the waterline (negative on the sand); a huge number
// with no beach.
fn shore_s(p: vec2<f32>) -> f32 {
    if (u.beach.x <= 0.0) {
        return 1.0e7;
    }
    return u.beach.y - dot(p - u.centre.xz, land_dir());
}

// Water depth under p (m): the sea depth, or the beach's.
fn depth_at(p: vec2<f32>) -> f32 {
    if (u.beach.x <= 0.0) {
        return u.shape.y;
    }
    return min(u.shape.y, shore_s(p) * u.beach.x);
}

// How much of the open sea's waves stand over depth d: they shrink so the
// deepest trough stays above 45 % of the depth (the breakers add at most
// another 33 %, so a trough never reaches the bed).
fn sea_q(d: f32) -> f32 {
    return clamp(0.45 * d / max(AMP_LO, 1e-4), 0.0, 1.0);
}

// ---- shore breakers (a straight beach: the travel time is exact) ----------

fn breaker_len() -> f32 {
    return floor(u.beach.w / 8.0);
}

fn breaker_h() -> f32 {
    return u.beach.w - 8.0 * floor(u.beach.w / 8.0);
}

// A breaker train's angular frequency for wavelength lam, a whole multiple
// of W0 so the loop stays seamless; and its phase now (radians, wrapped).
fn br_w(lam: f32) -> f32 {
    return max(round(sqrt(GRAV * TAU / lam) / W0), 1.0) * W0;
}

fn br_turns(w: f32) -> f32 {
    return fract(round(w / W0) * u.tl);
}

// Slow drifting noise along the beach on a closed loop through the noise
// (seamless every 300 s).
fn drift_noise(x: f32, r: f32) -> f32 {
    let a = TAU * u.tl;
    return vnoise(vec2<f32>(x + r * cos(a), r * sin(a) + 11.0)).x;
}

fn breakers_on() -> bool {
    return u.beach.x > 0.0 && breaker_h() > 0.0 && breaker_len() > 0.0;
}

// Bounds the breakers add: (above, below, slope).
fn breaker_bound() -> vec3<f32> {
    if (!breakers_on()) {
        return vec3<f32>(0.0);
    }
    let a = min(breaker_h() * 2.5, 0.42 * u.shape.y);
    let trains = select(1.0, 2.0, quality() >= 2);
    let k = TAU / (0.61 * breaker_len());
    // Slope: the profile's steepest face (2 x 1.6 x 0.74, leaned 1.62x)
    // over the shortened shallow-water wavelength is about 1.3 a k; 1.5 a k
    // leaves room for the peeling's along-shore phase.
    return vec3<f32>(1.22 * a * trains, 0.78 * a * trains, a * trains * k * 1.5);
}

// The surf zone: 0..220 m seaward of the waterline, the only place the
// breakers stand (`breakers` is zero outside it).
const SURF_OUT: f32 = 220.0;

fn in_surf(p: vec2<f32>) -> bool {
    let s = shore_s(p);
    return breakers_on() && s >= 0.0 && s <= SURF_OUT;
}

// The stretch [x, y] of the ray (unit rd from ro) over the surf zone; empty
// (x > y) with no breakers. The march adds the breakers' margins only there.
fn surf_span(ro: vec3<f32>, rd: vec3<f32>) -> vec2<f32> {
    if (!breakers_on()) {
        return vec2<f32>(1.0, -1.0);
    }
    let s0 = shore_s(ro.xz);
    let ds = -dot(rd.xz, land_dir());
    if (abs(ds) < 1e-6) {
        return select(vec2<f32>(1.0, -1.0), vec2<f32>(-FAR_T, FAR_T), s0 >= 0.0 && s0 <= SURF_OUT);
    }
    let a = -s0 / ds;
    let b = (SURF_OUT - s0) / ds;
    return vec2<f32>(min(a, b), max(a, b));
}

// Shallow-water travel time (s) from the waterline out to s metres, the
// depth slope * s clamped to 0.3..14 m (the eikonal |grad tau| = 1/c with
// c = sqrt(g d), in closed form for a straight beach).
fn travel(s: f32) -> f32 {
    let m = max(u.beach.x, 1e-4);
    let sa = 0.3 / m;
    let sb = 14.0 / m;
    let ss = max(s, 0.0);
    let ta = min(ss, sa) / sqrt(0.3 * GRAV);
    let tb = 2.0 * (sqrt(clamp(ss, sa, sb)) - sqrt(sa)) / sqrt(GRAV * m);
    let tc = max(ss - sb, 0.0) / sqrt(14.0 * GRAV);
    return ta + tb + tc;
}

fn hash2(c: vec2<i32>) -> f32 {
    let h = pcg(bitcast<u32>(c.x) ^ pcg(bitcast<u32>(c.y)));
    return f32(h >> 8u) * (1.0 / 16777216.0);
}

fn pcg(x: u32) -> u32 {
    let s = x * 747796405u + 2891336453u;
    let w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (w >> 22u) ^ w;
}

fn hash3(c: vec3<i32>) -> f32 {
    let h = pcg(bitcast<u32>(c.x) ^ pcg(bitcast<u32>(c.y) ^ pcg(bitcast<u32>(c.z))));
    return f32(h >> 8u) * (1.0 / 16777216.0);
}

// Value noise with its gradient: (value, d/dx, d/dy), value in 0..1.
fn vnoise(p: vec2<f32>) -> vec3<f32> {
    let i = vec2<i32>(floor(p));
    let f = p - floor(p);
    let w = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let dw = 30.0 * f * f * (f * (f - 2.0) + 1.0);
    let a = hash2(i);
    let b = hash2(i + vec2<i32>(1, 0));
    let c = hash2(i + vec2<i32>(0, 1));
    let d = hash2(i + vec2<i32>(1, 1));
    let k = a - b - c + d;
    return vec3<f32>(a + (b - a) * w.x + (c - a) * w.y + k * w.x * w.y, dw * vec2<f32>(b - a + k * w.y, c - a + k * w.x));
}

fn vn1(x: f32) -> f32 {
    let i = i32(floor(x));
    let f = x - floor(x);
    let w = f * f * (3.0 - 2.0 * f);
    return mix(hash2(vec2<i32>(i, 17)), hash2(vec2<i32>(i + 1, 17)), w);
}

struct Br {
    // Height, slope (d/dx, d/dz), whitewater (0..1), lip glow (0..1),
    // breaking (0..1).
    h: f32,
    g: vec2<f32>,
    white: f32,
    lip: f32,
    brk: f32,
};

fn breakers(p: vec2<f32>, full: bool) -> Br {
    var o = Br(0.0, vec2<f32>(0.0), 0.0, 0.0, 0.0);
    if (!breakers_on()) {
        return o;
    }
    let s = shore_s(p);
    if (s < 0.0 || s > SURF_OUT) {
        return o;
    }
    let d = s * u.beach.x;
    let land = land_dir();
    let along = dot(p - u.centre.xz, vec2<f32>(-land.y, land.x));
    let env = smoothstep(150.0, 55.0, s) * smoothstep(0.0, 0.35, d);
    let shoal = clamp(pow(max(d, 0.05) / 14.0, -0.25), 0.3, 2.5);
    // Slow sets: groups of bigger waves drift along the beach.
    let group = 0.45 + 0.55 * drift_noise(along * 0.025, 1.0);
    let tau = travel(s);
    let trains = select(1, 2, quality() >= 2);
    // Both trains together stay under the breaking limit (0.42 x depth),
    // so with the open sea's 0.45 the trough never reaches the bed.
    var room = 0.42 * d;
    for (var j = 0; j < trains; j = j + 1) {
        let lam = breaker_len() * select(1.0, 0.61, j == 1);
        let w = br_w(lam);
        let a = min(breaker_h() * env * shoal * group * select(1.0, 0.55, j == 1), room);
        room = room - max(a, 0.0);
        if (a <= 1e-4) {
            continue;
        }
        // Peeling: each cycle breaks early in some stretches.
        let off = (vn1(along * 0.018) - 0.5) * 4.5 + (vn1(along * 0.07 + 9.0) - 0.5) * 1.4 + 2.1 * f32(j);
        let ph = w * tau + TAU * br_turns(w) + off;
        let cyc = floor(ph / TAU);
        let e = vn1(along * 0.04 + cyc * 3.1 + f32(j) * 7.0);
        let rel = a / max(d, 0.05);
        let brk = smoothstep(0.12 + 0.14 * e, 0.3 + 0.2 * e, rel);
        // Lean: the shoreward face steepens as it breaks.
        let lean = 0.62 * brk;
        let f = ph - lean * (1.0 - cos(ph));
        let df = 1.0 - lean * sin(ph);
        let c = 0.5 + 0.5 * cos(f);
        let cp = pow(max(c, 1e-4), 1.6);
        o.h = o.h + a * (2.0 * cp - 0.78);
        // d/ds of the profile: dprof/dc * dc/df * df/dph * dph/ds.
        let dcdf = -0.5 * sin(f);
        let dtau = 1.0 / sqrt(GRAV * clamp(d, 0.3, 14.0));
        let dh_ds = a * 2.0 * 1.6 * pow(max(c, 1e-4), 0.6) * dcdf * df * w * dtau;
        // s grows seaward (against the land heading).
        o.g = o.g - land * dh_ds;
        if (full) {
            let psi = atan2(sin(ph), cos(ph));
            o.white = max(o.white, brk * smoothstep(-0.45, 0.0, psi) * exp(-max(psi, 0.0) * 0.35));
            o.lip = max(o.lip, brk * pow(c, 5.0) * smoothstep(0.6, -0.2, psi));
            // Lace left in the surf zone after a wave has broken.
            o.white = max(o.white, smoothstep(0.16, 0.36, rel) * (0.35 + 0.5 * exp(-0.3 * max(psi, 0.0))) * 0.75);
            o.brk = max(o.brk, brk);
        }
    }
    return o;
}

// ---- the height field and the march ------------------------------------------

// Fraction of component k kept at footprint fp (m): waves shorter than a
// few footprints fade (their slopes go into the roughness instead).
fn keep(k: f32, fp: f32) -> f32 {
    return 1.0 - smoothstep(0.45, 1.1, k * fp);
}

// Where the water now at p came from before the squeeze moved it (one
// fixed-point step of x = x0 + lambda q sum(k^ a cos)).
fn unsqueeze(p: vec2<f32>, fp: f32, q: f32) -> vec2<f32> {
    if (LAMBDA <= 0.0) {
        return p;
    }
    var d = vec2<f32>(0.0);
    for (var i = 0u; i < NB; i = i + 1u) {
        let w = keep(GBK[i], fp);
        let c = GB[i];
        d = d + c.xy * (c.z * w * cos(dot(c.xy, p) + c.w) / max(GBK[i], 1e-6));
    }
    return p - LAMBDA * q * d;
}

// The envelope over the sea at p: no wave can stand higher there.
fn envelope(p: vec2<f32>, fp: f32) -> f32 {
    let q = sea_q(depth_at(p));
    var h = 0.0;
    for (var i = 0u; i < NB; i = i + 1u) {
        let c = GB[i];
        h = h + c.z * keep(GBK[i], fp) * prof(sin(dot(c.xy, p) + c.w), 0.0).x;
    }
    // The breakers stand outside the shallow-water shrink (q), and only
    // over the surf zone.
    return q * max(h + ENV_M, 0.0) + select(0.0, ENV_B, in_surf(p));
}

fn height(p: vec2<f32>, fp: f32) -> f32 {
    let q = sea_q(depth_at(p));
    let p0 = unsqueeze(p, fp, q);
    var h = 0.0;
    for (var i = 0u; i < NG; i = i + 1u) {
        let w = keep(GK[i], fp);
        if (w <= 0.0) {
            continue;
        }
        let c = GW[i];
        let th = dot(c.xy, p0) + c.w;
        h = h + c.z * w * prof(sin(th), 0.0).x;
    }
    var out = h * q;
    if (breakers_on()) {
        out = out + breakers(p, false).h;
    }
    return out;
}

fn foot(t: f32) -> f32 {
    return max(t * u.px, 1e-4);
}

// Height of the ray above the sea at t.
fn above(ro: vec3<f32>, rd: vec3<f32>, t: f32) -> f32 {
    let q = ro + rd * t;
    return q.y - u.centre.y - height(q.xz, foot(t));
}

// How many safe steps the secant may jump. Measured against the mirror's
// first-crest test: a ray that runs out of steps skips more crests than a
// long secant jump does, so one generous cap serves every level.
const REACH_CAP: f32 = 8.0;

// The first t where the ray (unit rd) meets the sea before `tmax`, or -1.
// Steps never pass the slope bound's safe distance by more than 8x; any
// crossing that is bracketed is bisected onto, so a hit always lies on the
// waves (never on a fallback plane).
fn march(ro: vec3<f32>, rd: vec3<f32>, tmax: f32) -> f32 {
    let l = lv();
    let top = u.centre.y + H_HI + 0.02;
    let bot = u.centre.y - H_LO - 0.02;
    var t0 = 0.0;
    if (ro.y > top) {
        if (rd.y >= 0.0) {
            return -1.0;
        }
        t0 = (top - ro.y) / rd.y;
    }
    var t1 = tmax;
    if (rd.y < 0.0) {
        t1 = min(t1, (bot - ro.y) / rd.y);
    } else if (rd.y > 0.0) {
        t1 = min(t1, (top - ro.y) / rd.y);
    }
    t1 = min(t1, 6.0e4);
    if (t1 <= t0) {
        return -1.0;
    }
    let lat = length(rd.xz);
    // Stage 1: march to the envelope (cheap: the longest waves only). The
    // waves lie under it, so no crossing is skipped; a step that overshoots
    // it goes back to the last point above it.
    let span = surf_span(ro, rd);
    var ta = t0;
    var tlast = t0;
    for (var i = 0; i < l.coarse; i = i + 1) {
        let q = ro + rd * ta;
        let fe = q.y - u.centre.y - envelope(q.xz, foot(ta));
        if (fe <= 0.0) {
            ta = tlast;
            break;
        }
        if (fe < 0.03 + 2.0 * foot(ta)) {
            break;
        }
        tlast = ta;
        var nt = ta + max(fe / max(abs(rd.y) + ENV_L * lat, 0.02), 0.002 * ta + 0.01);
        // The envelope rises by the breakers' margin at the surf zone: stop
        // at its edge.
        if (ta < span.x && nt > span.x) {
            nt = span.x;
        }
        ta = nt;
        if (ta >= t1) {
            // Never under the envelope before the end: is it under at the end?
            let qe = ro + rd * t1;
            if (qe.y - u.centre.y - envelope(qe.xz, foot(t1)) > 0.0) {
                return -1.0;
            }
            ta = tlast;
            break;
        }
    }
    {
        // Out of steps past the envelope: back to the last point above it.
        let q = ro + rd * ta;
        if (q.y - u.centre.y - envelope(q.xz, foot(ta)) <= 0.0) {
            ta = tlast;
        }
    }
    // Stage 2: the waves themselves.
    var fa = above(ro, rd, ta);
    if (fa <= 0.0) {
        return select(-1.0, ta, ta > 0.0);
    }
    let sb = slope_bound(foot(ta));
    let sbb = breaker_bound().z;
    var tb = ta;
    var fb = fa;
    var found = false;
    var tp = ta;
    var fp_ = fa;
    for (var i = 0; i < l.steps; i = i + 1) {
        // Within a pixel's height of the waves: there.
        if (fa < 0.004 + 1.5 * foot(ta)) {
            return ta;
        }
        // The safe step (no crest can rise to the ray sooner, by the slope
        // bound of what this distance keeps); when the ray is closing in,
        // the secant's guess at the crossing, held to a few safe steps
        // near the camera and more far out.
        let surf = ta >= span.x - (0.01 + 1e-4 * ta) && ta <= span.y;
        let safe = fa / max(abs(rd.y) + (sb + select(0.0, sbb, surf)) * lat, 0.02);
        var step = safe;
        let closing = fp_ - fa;
        if (closing > 0.0) {
            let guess = fa * (ta - tp) / closing * 1.05;
            step = clamp(guess, safe, safe * REACH_CAP + 0.004 * ta);
        }
        step = max(step, 0.004 * ta + 0.02);
        // Outside the surf zone the breakers' slope is left out: stop at
        // its edge.
        if (!surf && ta < span.x) {
            step = min(step, span.x - ta);
        }
        tp = ta;
        fp_ = fa;
        tb = min(ta + step, t1);
        fb = above(ro, rd, tb);
        if (fb <= 0.0) {
            found = true;
            break;
        }
        if (tb >= t1) {
            return -1.0;
        }
        ta = tb;
        fa = fb;
    }
    if (!found) {
        tb = t1;
        fb = above(ro, rd, tb);
        if (fb > 0.0) {
            return -1.0;
        }
    }
    // Onto the crossing in [ta, tb]: Illinois regula falsi (the secant, with
    // the end kept twice in a row given half its weight), every other step
    // a plain halving, so the bracket shrinks however steep the crest.
    var wa = fa;
    var wb = fb;
    var side = 0;
    // Out of steps, the bracket runs to the slab's end: halve it more.
    let n = l.bisect + select(0, 8, !found);
    for (var i = 0; i < n; i = i + 1) {
        let f = select(clamp(wa / max(wa - wb, 1e-6), 0.05, 0.95), 0.5, (i & 1) == 1 || i >= l.bisect);
        let tm = mix(ta, tb, f);
        let fm = above(ro, rd, tm);
        if (fm > 0.0) {
            ta = tm;
            fa = fm;
            wa = fm;
            if (side == 1) {
                wb = wb * 0.5;
            }
            side = 1;
        } else {
            tb = tm;
            fb = fm;
            wb = fm;
            if (side == -1) {
                wa = wa * 0.5;
            }
            side = -1;
        }
    }
    return mix(ta, tb, clamp(fa / max(fa - fb, 1e-6), 0.0, 1.0));
}

// ---- the surface at a point: normal, roughness, folding -------------------------

struct Surf {
    // Slope (dh/dx, dh/dz).
    g: vec2<f32>,
    // Slope variance of what the pixel can't resolve (x, z).
    var2: vec2<f32>,
    // The folding (Jacobian, 1 = no compression) now and a moment ago.
    jac: f32,
    jac_was: f32,
    // Height above the level (m).
    h: f32,
    // Hessian (xx, zz, xz) for the caustics.
    hess: vec3<f32>,
};

// The surface at p: the height set (squeezed, as the march sees it) and the
// first `nd` detail components, filtered by the pixel's footprint `fp`
// (along x and z).
fn surface_at(p: vec2<f32>, fp: vec2<f32>, nd: u32, with_past: bool) -> Surf {
    var o: Surf;
    o.g = vec2<f32>(0.0);
    o.var2 = vec2<f32>(0.0);
    o.hess = vec3<f32>(0.0);
    let q = sea_q(depth_at(p));
    let iso = min(fp.x, fp.y);
    let p0 = unsqueeze(p, iso, q);
    // Folding for the foam: as the squeeze, a little stronger, and on even
    // with no squeeze (steep seas still break).
    let lam_f = 1.0 + 1.6 * u.shape.z;
    let linger = max(u.foam.z, 0.05);
    var g0 = vec2<f32>(0.0);
    var m = vec3<f32>(0.0);
    var mf = vec3<f32>(0.0);
    var mp = vec3<f32>(0.0);
    var h = 0.0;
    // The squeeze's Jacobian (the longest waves move the water).
    for (var i = 0u; i < NB; i = i + 1u) {
        let c = GB[i];
        let k = max(GBK[i], 1e-6);
        let w = keep(k, iso);
        let kk = vec3<f32>(c.x * c.x, c.y * c.y, c.x * c.y);
        m = m + kk * (c.z * w * sin(dot(c.xy, p0) + c.w) / k);
    }
    for (var i = 0u; i < NG; i = i + 1u) {
        let c = GW[i];
        let k = max(GK[i], 1e-6);
        let d = c.xy / k;
        // Anisotropic footprint: the pixel is longer along the view.
        let kept = keep(k, length(d * fp));
        let w = kept * q;
        let a = c.z;
        let th = dot(c.xy, p0) + c.w;
        let sn = sin(th);
        let pr = prof(sn, cos(th));
        g0 = g0 + c.xy * (a * w * pr.y);
        // What is filtered goes into the variance (a sine's slope variance
        // (a k)^2 / 2, by axis).
        o.var2 = o.var2 + d * d * ((1.0 - kept * kept) * a * a * k * k * 0.5 * q * q);
        h = h + a * w * pr.x;
        let kk = vec3<f32>(c.x * c.x, c.y * c.y, c.x * c.y);
        // Folding for the foam: d(k^ a cos)/dx = -(k k^T / k) a sin.
        mf = mf + kk * (a * w * sn / k);
        o.hess = o.hess + kk * (a * w * pr.z);
        if (with_past) {
            // The same waves `linger` seconds ago: where a crest folded then,
            // foam is still lying now.
            let th2 = th - round(sqrt(GRAV * k) / W0) * W0 * linger;
            mp = mp + kk * (a * w * sin(th2) / k);
        }
    }
    for (var j = 0u; j < nd; j = j + 1u) {
        let i = SEA_DET_BASE + j;
        let c = comp(i, false, 0.0);
        let k = max(comp_k(i, false), 1e-6);
        let d = c.xy / k;
        let kept = keep(k, length(d * fp));
        let w = kept * q;
        let a = c.z;
        let lost = (1.0 - kept * kept) * a * a * k * k * 0.5 * q * q;
        o.var2 = o.var2 + d * d * lost;
        if (w <= 0.0) {
            continue;
        }
        let th = dot(c.xy, p0) + c.w;
        let sn = sin(th);
        let pr = prof(sn, cos(th));
        g0 = g0 + c.xy * (a * w * pr.y);
        let kk = vec3<f32>(c.x * c.x, c.y * c.y, c.x * c.y);
        mf = mf + kk * (a * w * sn / k);
        o.hess = o.hess + kk * (a * w * pr.z);
    }
    // The squeeze's map x = x0 + D(x0) has Jacobian I - lambda q M; the
    // slope over x is its inverse transpose times the slope over x0.
    let lq = LAMBDA * q;
    let a11 = 1.0 - lq * m.x;
    let a22 = 1.0 - lq * m.y;
    let a12 = -lq * m.z;
    let det = max(a11 * a22 - a12 * a12, 0.15);
    o.g = vec2<f32>(a22 * g0.x - a12 * g0.y, -a12 * g0.x + a11 * g0.y) / det;
    let mt = mf;
    o.jac = (1.0 - lam_f * mt.x) * (1.0 - lam_f * mt.y) - lam_f * lam_f * mt.z * mt.z;
    o.jac_was = select(o.jac, (1.0 - lam_f * mp.x) * (1.0 - lam_f * mp.y) - lam_f * lam_f * mp.z * mp.z, with_past);
    o.h = h;
    return o;
}

// ---- foam lace -----------------------------------------------------------------

// Bubble walls: 1 on the borders of jittered cells, 0 in their middles.
fn cells(p: vec2<f32>) -> f32 {
    let b = vec2<i32>(floor(p));
    let f = p - floor(p);
    var d1 = 9.0;
    var d2 = 9.0;
    for (var y = -1; y <= 1; y = y + 1) {
        for (var x = -1; x <= 1; x = x + 1) {
            let c = b + vec2<i32>(x, y);
            let o = vec2<f32>(hash2(c), hash2(c + vec2<i32>(91, 37)));
            let r = vec2<f32>(f32(x), f32(y)) + o * 0.85 + 0.075 - f;
            let dd = dot(r, r);
            if (dd < d1) {
                d2 = d1;
                d1 = dd;
            } else if (dd < d2) {
                d2 = dd;
            }
        }
    }
    return 1.0 - smoothstep(0.0, 0.32, sqrt(d2) - sqrt(d1));
}

// The foam pattern (0..1): bubble lace at two sizes over a patchy density.
fn lace(p: vec2<f32>, cheap: bool) -> f32 {
    let n = vnoise(p * 0.31).x * 0.6 + vnoise(p * 1.13 + 3.7).x * 0.4;
    if (cheap) {
        return n;
    }
    // The bubble walls bent by noise, so the lace runs in streaks and
    // clumps rather than a regular net.
    let w = vnoise(p * 0.7 + 1.3).yz * 0.55;
    let a = cells(p * 1.6 + w);
    let b = cells(p * 4.3 + vec2<f32>(5.1, 2.3) + w * 2.0);
    return clamp(0.55 * n + 0.3 * a + 0.15 * b, 0.0, 1.0);
}

// Foam cover from a foam amount (0..1): dense foam is solid, thin foam
// erodes into lace.
fn cover(foam: f32, pat: f32) -> f32 {
    let f = clamp(foam, 0.0, 1.0);
    // Below a trace nothing shows (thin foam doesn't draw lines on every
    // wave); above, dense foam is solid and thinner foam erodes into lace.
    return smoothstep(1.05 - f, 1.3 - f, pat) * sqrt(f) * smoothstep(0.08, 0.25, f);
}

// ---- light ----------------------------------------------------------------------

fn sky(dir: vec3<f32>) -> vec3<f32> {
    let hor = unpack_col(u.cols.x);
    let zen = unpack_col(u.cols.y);
    return mix(hor, zen, sqrt(clamp(dir.y, 0.0, 1.0))) * sky_k();
}

fn d_ggx(noh: f32, a: f32) -> f32 {
    let a2 = a * a;
    let d = noh * noh * (a2 - 1.0) + 1.0;
    return a2 / (PI * d * d);
}

fn v_smith(nov: f32, nol: f32, a: f32) -> f32 {
    let a2 = a * a;
    let gv = nol * sqrt(nov * nov * (1.0 - a2) + a2);
    let gl = nov * sqrt(nol * nol * (1.0 - a2) + a2);
    return 0.5 / max(gv + gl, 1e-5);
}

// A standard normal from two uniforms.
fn gauss(u1: f32, u2: f32) -> f32 {
    return sqrt(-2.0 * log(max(u1, 1e-6))) * cos(TAU * u2);
}

// Sun glitter: the lobe as a finite number of mirror facets per footprint
// cell, re-drawn three times a second; the mean stays 1 so no light is lost.
fn glitter(p: vec2<f32>, fp: f32, d: f32, noh: f32, voh: f32, dist: f32) -> f32 {
    let g = u.light.y;
    if (g <= 0.0 || quality() < 2) {
        return 1.0;
    }
    let cell = max(fp * 2.0, 0.02);
    let lam = 2.0e4 * d * noh * 6.8e-5 / (4.0 * max(voh, 0.05)) * cell * cell;
    // Only inside the sun's lobe, where a cell expects a facet or more: a
    // lone facet where almost none is expected would be a white speck or,
    // far out, a bright block.
    if (lam < 1.0) {
        return 1.0;
    }
    let c = vec2<i32>(floor(p / cell));
    let ts = u.t * 3.0;
    let k0 = i32(floor(ts));
    let fr = smoothstep(0.0, 1.0, fract(ts));
    var r = vec2<f32>(0.0);
    for (var j = 0; j < 2; j = j + 1) {
        let s = vec3<i32>(c, k0 + j);
        let u1 = hash3(s);
        let u2 = hash3(s + vec3<i32>(13, 71, 5));
        var n = 0.0;
        if (lam < 6.0) {
            // Poisson by inverse CDF.
            var pk = exp(-lam);
            var cdf = pk;
            for (var m = 0; m < 12; m = m + 1) {
                if (u1 <= cdf) {
                    break;
                }
                n = n + 1.0;
                pk = pk * lam / n;
                cdf = cdf + pk;
            }
        } else {
            n = max(lam + sqrt(lam) * gauss(u1, u2), 0.0);
        }
        r[j] = min(n / max(lam, 1e-4), 6.0);
    }
    let ratio = mix(r.x, r.y, fr);
    return mix(1.0, ratio, g * smoothstep(3.0, 20.0, dist));
}

struct Px {
    ray: vec3<f32>,
    rl: f32,
    rd: vec3<f32>,
    // Scene distance along rd (m), FAR_T for the sky.
    ts: f32,
    scene: bool,
};

fn pixel_ray(uv: vec2<f32>) -> vec3<f32> {
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    return u.fwd + u.right * ndc.x + u.up * ndc.y;
}

fn scene_dist(z: f32) -> f32 {
    if (z <= 0.0) {
        return FAR_T;
    }
    return 1.0 / max(u.inv0 + u.inv1 * z, 1e-12);
}

fn pixel(uv: vec2<f32>, z: f32) -> Px {
    var px: Px;
    px.ray = pixel_ray(uv);
    px.rl = length(px.ray);
    px.rd = px.ray / px.rl;
    // The depth gives view-ray units (|ray| per unit): metres along rd.
    let ts = scene_dist(z);
    px.scene = ts < FAR_T;
    px.ts = select(FAR_T, ts * px.rl, px.scene);
    return px;
}

fn inside(p: vec2<f32>) -> bool {
    let l = abs(p - u.centre.xz);
    return l.x <= u.half.x && l.y <= u.half.y;
}

// What a pixel of the sea gives: the colour that replaces the scene's
// (a multiplier for what is seen through it and the light it adds).
struct Shade {
    hit: bool,
    t: f32,
    m: vec3<f32>,
    a: vec3<f32>,
    off: vec2<f32>,
    // Caustic gain on what is seen through the water (1 = none).
    caustic: f32,
};

fn screen_bend(n: vec3<f32>) -> vec2<f32> {
    return vec2<f32>(dot(n, normalize(u.right)), -dot(n, normalize(u.up)));
}

fn sea_shade(uv: vec2<f32>, z: f32, flat: bool) -> Shade {
    var o = Shade(false, -1.0, vec3<f32>(1.0), vec3<f32>(0.0), vec2<f32>(0.0), 1.0);
    let px = pixel(uv, z);
    let l = lv();
    let ro = u.eye;
    // Where the view meets the sea.
    var t = -1.0;
    if (flat || l.steps == 0) {
        if (ro.y > u.centre.y && px.rd.y < -1e-5) {
            let tw = (u.centre.y - ro.y) / px.rd.y;
            if (tw < px.ts) {
                t = tw;
            }
        }
    } else {
        t = march(ro, px.rd, px.ts);
    }
    let swash = swash_shade(px);
    if (t <= 0.0) {
        if (swash.hit) {
            return swash;
        }
        return o;
    }
    let p3 = ro + px.rd * t;
    if (!inside(p3.xz)) {
        return o;
    }
    o.hit = true;
    o.t = t;
    let v = -px.rd;
    let fp_iso = foot(t);
    // The footprint stretches along the view as the view grazes.
    let along = normalize(px.rd.xz + vec2<f32>(1e-6, 0.0));
    let stretch = 1.0 / max(abs(px.rd.y), 0.08);
    let fp = vec2<f32>(fp_iso) + abs(along) * fp_iso * (stretch - 1.0);
    let full_foam = quality() >= 1 && !flat;
    let sf = surface_at(p3.xz, fp, l.nd, quality() >= 2 && u.foam.x > 0.0);
    let br = breakers(p3.xz, true);
    let g = sf.g + br.g;
    // Detail too small for any component: three moving noise octaves (60,
    // 20 and 7 cm), along the wind, faded by the footprint, looped every
    // 7.5 s (two phases). They carry most of the unresolved slope near the
    // camera, as real facets: the sun breaks into sparkles there.
    let ph = fract(u.tl * 40.0);
    let wgt = abs(2.0 * ph - 1.0);
    let wdir = RW * 0.6;
    // Each octave fades as the pixel's footprint reaches its size (what it
    // loses goes to the roughness), so nothing aliases into grain.
    let fmax = max(fp.x, fp.y);
    let o1 = 1.0 - smoothstep(0.08, 0.25, fmax);
    let o2 = 1.0 - smoothstep(0.025, 0.08, fmax);
    let o3 = 1.0 - smoothstep(0.008, 0.027, fmax);
    var micro = vec2<f32>(0.0);
    for (var j = 0; j < 2; j = j + 1) {
        let f = fract(ph + 0.5 * f32(j));
        let q = p3.xz * 1.6 - wdir * f * 7.5 + f32(j) * 17.3;
        let wj = select(wgt, 1.0 - wgt, j == 1);
        if (o1 > 0.0) {
            micro = micro + vnoise(q).yz * (0.8 * o1 * wj);
        }
        if (o2 > 0.0) {
            micro = micro + vnoise(q * 3.1).yz * (1.6 * o2 * wj);
        }
        if (o3 > 0.0) {
            micro = micro + vnoise(q * 9.3).yz * (2.7 * o3 * wj);
        }
    }
    // World slope units, scaled back to unit variance (the three octaves'
    // slope std is about 3.4 when all are kept).
    micro = micro / 3.4;
    let mfade = (0.2 * o1 * o1 + 0.35 * o2 * o2 + 0.45 * o3 * o3);
    let v_micro = 0.5 * max(u.v_unres, 0.0) * mfade;
    let gm = g + micro * sqrt(0.5 * max(u.v_unres, 0.0));
    let n = normalize(vec3<f32>(-gm.x, 1.0, -gm.y));
    // Roughness: base + unresolved (less what the micro facets carry) +
    // what the footprint filtered out of the components.
    let a2 = u.glow.w * u.glow.w + max(u.v_unres - v_micro, 0.0) + sf.var2.x + sf.var2.y;
    let alpha = clamp(sqrt(a2), 0.02, 1.0);
    let nov = max(dot(n, v), 1e-3);
    let f90 = max(1.0 - 0.7 * sqrt(alpha), WATER_F0);
    let fres = clamp((WATER_F0 + (f90 - WATER_F0) * pow(1.0 - nov, 5.0)) * u.glow.z, 0.0, 1.0);
    // Reflection: the sky. The rougher the water, the more of the
    // reflection lobe dips under the horizon and sees the next wave's dark
    // face instead of sky.
    var r = reflect(px.rd, n);
    let under_lobe = smoothstep(alpha * 0.9, -alpha * 0.9, r.y);
    r.y = abs(r.y);
    let dark = sky(vec3<f32>(0.0, 0.05, 0.0)) * 0.22 + unpack_col(u.cols.w) * 0.6;
    let refl = mix(mix(sky(r), sky(vec3<f32>(0.0, 0.35, 0.0)), clamp(alpha * 0.6, 0.0, 0.4)), dark, under_lobe * 0.85);
    // The sun: GGX with the sun as a small area light (alpha floor).
    let lsun = u.sun;
    let hv = normalize(v + lsun);
    let nol = max(dot(n, lsun), 0.0);
    let noh = max(dot(n, hv), 0.0);
    let voh = max(dot(v, hv), 0.0);
    let as_ = max(alpha, 0.015);
    let dd = d_ggx(noh, as_);
    let fs = WATER_F0 + (1.0 - WATER_F0) * pow(1.0 - voh, 5.0);
    var spec = u.sun_col * (PI * dd * v_smith(nov, nol, as_) * fs * nol) * u.light.x;
    spec = spec * glitter(p3.xz, fp_iso, dd, noh, voh, t) * smoothstep(0.0, 0.06, nov);
    spec = min(spec, vec3<f32>(60.0));
    // The water's body: what is behind, through the water, and the light
    // the water scatters back.
    let deep = clamp(unpack_col(u.cols.z), vec3<f32>(0.004), vec3<f32>(1.0));
    let path = select(400.0, max(px.ts - t, 0.0), px.scene);
    let trans = pow(deep, vec3<f32>(min(path, 400.0) / max(u.shape.w, 0.05)));
    let amb = sky(vec3<f32>(0.0, 1.0, 0.0)) * 0.55 + sky(vec3<f32>(0.0, 0.1, 0.0)) * 0.25;
    let sunlit = smoothstep(-0.05, 0.12, lsun.y);
    let scat = unpack_col(u.cols.w);
    let inscatter = scat * (amb + u.sun_col * (0.3 + 0.7 * sunlit) * max(lsun.y, 0.0));
    // Crest glow: sunlight through the thin top of a tall wave, seen
    // against the sun (height-weighted, back-lit), plus a view term.
    let hh = max(sf.h + br.h, 0.0);
    // The glow colour at a scattering strength like the body's (the colour
    // param is its hue and brightness, 0.1 x).
    let sss_c = unpack_col(u.glow.x) * 0.1;
    let toward = pow(clamp(dot(lsun, px.rd), 0.0, 1.0), 4.0);
    let k1 = u.glow.y * min(hh, 5.0) * toward * pow(0.5 - 0.5 * dot(n, lsun), 3.0);
    let k2 = u.glow.y * 0.06 * nov * nov;
    let sss = (k1 + k2) * sss_c * u.sun_col * sunlit + br.lip * sss_c * u.sun_col * 2.5 * u.glow.y;
    let body_light = (inscatter + sss) * (vec3<f32>(1.0) - trans);
    // Foam: whitecaps where the surface folds (now, and where it folded a
    // moment ago), the breakers' whitewater, and contact foam where the
    // water is thin over sand or rock.
    let thr = 1.0 - (0.04 + 0.5 * (1.0 - u.foam.y));
    let now = clamp((thr - sf.jac) * 2.6, 0.0, 1.0);
    let was = clamp((thr - sf.jac_was) * 2.6, 0.0, 1.0) * 0.75;
    var foam = max(now, was) * u.foam.x;
    let thick = path * abs(px.rd.y);
    let contact = select(0.0, (1.0 - smoothstep(0.0, 0.35 + 0.5 * u.foam.w, thick)) * smoothstep(6.0, 2.0, depth_at(p3.xz)), px.scene);
    foam = max(foam, max(br.white * 1.15, contact * 0.9) * u.foam.w);
    var cov = 0.0;
    if (foam > 0.08) {
        cov = cover(foam, lace(p3.xz, !full_foam)) * (1.0 - smoothstep(0.2, 1.4, fp_iso * 3.0) * 0.4);
    }
    let solid = smoothstep(0.55, 0.95, foam);
    let alb = mix(vec3<f32>(0.62, 0.78, 0.82), vec3<f32>(0.95), solid);
    let fol = max(dot(normalize(n + vec3<f32>(0.0, 0.8, 0.0)), lsun), 0.0);
    let foam_lit = alb * (amb * 1.1 + u.sun_col * (0.25 + 0.75 * fol) * max(lsun.y, 0.0) * 0.85)
        + u.sun_col * pow(clamp(dot(lsun, px.rd), 0.0, 1.0), 4.0) * 0.4 * (1.0 - solid) * sunlit;
    // Aerial haze far out: the sea melts into the horizon's colour.
    let haze = 1.0 - exp(-t / 9000.0);
    let hz = sky(vec3<f32>(px.rd.x, 0.02, px.rd.z));
    // Spray mist over breaking crests (Ultra).
    var mist = 0.0;
    if (quality() >= 3 && u.spray > 0.0 && br.brk > 0.0) {
        for (var j = 0; j < 4; j = j + 1) {
            let tj = t - (0.4 + 0.9 * f32(j));
            if (tj <= 0.0) {
                continue;
            }
            let q = ro + px.rd * tj;
            let hb = breakers(q.xz, true);
            let above_w = q.y - u.centre.y - hb.h;
            let sp = fract(u.tl * 100.0);
            let nz = vnoise(q.xz * 1.3 + vec2<f32>(0.0, sp * 5.0)).x * vnoise(vec2<f32>(q.y * 2.0 - sp * 7.0, q.x * 0.7)).x * (1.0 - abs(2.0 * sp - 1.0))
                + vnoise(q.xz * 1.3 + vec2<f32>(9.0, fract(sp + 0.5) * 5.0)).x * vnoise(vec2<f32>(q.y * 2.0 - fract(sp + 0.5) * 7.0, q.x * 0.7 + 4.0)).x * abs(2.0 * sp - 1.0);
            mist = mist + hb.brk * max(hb.lip, 0.35 * hb.white) * exp(-max(above_w, 0.0) / 1.4) * smoothstep(0.12, 0.55, nz) * 0.7;
        }
        mist = clamp(mist * u.spray, 0.0, 1.0);
    }
    let mist_c = amb * 1.2 + u.sun_col * (0.4 + 0.6 * pow(clamp(dot(lsun, px.rd), 0.0, 1.0), 2.0)) * max(lsun.y, 0.0) * 0.8;
    // The caustics on what is seen through the water (High and Ultra).
    if (quality() >= 2 && u.light.w > 0.0 && px.scene && !flat) {
        o.caustic = caustic_gain(p3, px, t);
    }
    // Put together: m multiplies what is behind (through the water), a is
    // the light added on top.
    let keep_c = (1.0 - fres) * (1.0 - cov);
    o.m = keep_c * trans * (1.0 - haze) * (1.0 - mist);
    var add = (1.0 - fres) * body_light + fres * refl + spec;
    add = add * (1.0 - cov) + foam_lit * cov;
    add = mix(add, hz, haze);
    add = mix(add, mist_c, mist);
    o.a = add;
    let tilt = n - vec3<f32>(0.0, 1.0, 0.0);
    o.off = select(screen_bend(tilt) * u.light.z * clamp(thick * 0.25, 0.0, 1.0) * 0.06 / max(t * 0.02, 1.0), vec2<f32>(0.0), flat);
    return o;
}

// Light patterns on the bed under shallow water: the area ratio of the
// sunlight bent by the waves above (where they focus it, the bed is
// brighter), from the surface's curvature: 1 / det(I - d (1 - 1/1.33) H).
fn caustic_gain(p3: vec3<f32>, px: Px, t: f32) -> f32 {
    let bed = u.eye + px.rd * px.ts;
    let d = u.centre.y - bed.y;
    if (d <= 0.02) {
        return 1.0;
    }
    // The sunlight entered the water above the bed point, up the sun's
    // refracted ray.
    let sr = refract(-u.sun, vec3<f32>(0.0, 1.0, 0.0), 1.0 / 1.33);
    let entry = bed.xz - sr.xz / min(sr.y, -0.2) * d;
    // The curvature of the short waves (the long ones barely focus light
    // over a few metres): the first 16 detail waves, blurred by depth.
    let fpc = max(0.02 * d, 0.05);
    var hs = vec3<f32>(0.0);
    let q = sea_q(depth_at(entry));
    for (var j = 0u; j < 16u; j = j + 1u) {
        let i = SEA_DET_BASE + j;
        let c = comp(i, false, 0.0);
        let kc = comp_k(i, false);
        let w = keep(kc, fpc) * q;
        if (w <= 0.0) {
            continue;
        }
        let th = dot(c.xy, entry) + c.w;
        let pr = prof(sin(th), cos(th));
        hs = hs + vec3<f32>(c.x * c.x, c.y * c.y, c.x * c.y) * (c.z * w * pr.z);
    }
    let k = d * (1.0 - 1.0 / 1.33);
    let det = (1.0 - k * hs.x) * (1.0 - k * hs.y) - k * k * hs.z * hs.z;
    let raw = clamp(1.0 / max(abs(det), 0.06), 0.0, 6.0);
    // Blurs out with depth, needs sun.
    let c = mix(1.0, raw, exp(-d / 7.0) * smoothstep(0.0, 0.3, u.sun.y));
    _ = t;
    return mix(1.0, c, u.light.w);
}

// Swash: the thin sheet that runs up the sand after each breaker, its
// foam edge, and the dark wet line it leaves (on the beach above the water).
fn swash_shade(px: Px) -> Shade {
    var o = Shade(false, -1.0, vec3<f32>(1.0), vec3<f32>(0.0), vec2<f32>(0.0), 1.0);
    if (!breakers_on() || !px.scene || quality() < 1) {
        return o;
    }
    let ps = u.eye + px.rd * px.ts;
    let s = shore_s(ps.xz);
    let e = ps.y - u.centre.y;
    if (s > 2.0 || e < -0.05 || e > breaker_h() * 1.2 + 0.3) {
        return o;
    }
    let land = land_dir();
    let along = dot(ps.xz - u.centre.xz, vec2<f32>(-land.y, land.x));
    let lam = breaker_len();
    let w = br_w(lam);
    let group = 0.45 + 0.55 * drift_noise(along * 0.025, 1.0);
    let run = 0.55 * breaker_h() * group * 0.9;
    let f = fract(br_turns(w) + ((vn1(along * 0.018) - 0.5) * 4.5 + (vn1(along * 0.07 + 9.0) - 0.5) * 1.4) / TAU);
    var eta = run * cos((f - 0.3) / 0.7 * PI * 0.5);
    if (f < 0.3) {
        eta = run * sin(f / 0.3 * PI * 0.5);
    }
    let th = eta - e;
    // Time since the sheet last left this height (backwash), for the wet line.
    let fe = 0.3 + 0.7 * (2.0 / PI) * acos(clamp(e / max(run, 1e-3), -1.0, 1.0));
    let dry = select(0.0, (f - fe) * TAU / w, f > fe && f >= 0.3);
    let wet = select(exp(-max(dry, 0.0) / 5.0), 1.0, th > 0.0) * smoothstep(run + 0.05, run * 0.7, e);
    o.hit = wet > 0.002 || th > 0.0;
    if (!o.hit) {
        return o;
    }
    o.t = px.ts;
    // Wet sand darkens; the sheet reflects the sky and carries a foam edge.
    var m = mix(vec3<f32>(1.0), vec3<f32>(0.55, 0.58, 0.62), wet);
    var a = vec3<f32>(0.0);
    if (th > 0.0) {
        let film = clamp(th / 0.04, 0.0, 1.0);
        let fres = mix(0.02, 0.2, pow(1.0 - abs(px.rd.y), 5.0)) * film;
        let edge = smoothstep(0.0, 0.03, th) * (1.0 - smoothstep(0.03, 0.12, th));
        let pat = lace(ps.xz * 1.4, quality() < 2);
        let fo = cover(edge * 1.1 + 0.15 * film * select(1.0, 0.0, f > 0.3), pat) * u.foam.w;
        let amb = sky(vec3<f32>(0.0, 1.0, 0.0)) * 0.6;
        m = m * (1.0 - fres) * (1.0 - fo);
        a = fres * sky(reflect(px.rd, vec3<f32>(0.0, 1.0, 0.0))) + fo * (amb + u.sun_col * max(u.sun.y, 0.0) * 0.8) * 0.95;
    }
    o.m = m;
    o.a = a;
    return o;
}

// Under the surface (the camera dipped into a wave): the water's colour
// over everything, the sky through the surface inside Snell's window.
fn under(uv: vec2<f32>, z: f32) -> Shade {
    var o = Shade(true, 0.0, vec3<f32>(1.0), vec3<f32>(0.0), vec2<f32>(0.0), 1.0);
    let px = pixel(uv, z);
    let deep = clamp(unpack_col(u.cols.z), vec3<f32>(0.004), vec3<f32>(1.0));
    let scat = unpack_col(u.cols.w);
    let amb = sky(vec3<f32>(0.0, 1.0, 0.0)) * 0.55;
    // Inside the water the light scattered from every side reaches the eye,
    // not just what comes back up out of it (the scatter colour is tuned as
    // seen from above): a few times brighter, so a camera that dips into a
    // wave sees murky lit water, not a black frame.
    let glow = scat * (amb * 4.0 + u.sun_col * max(u.sun.y, 0.0) * 1.5);
    var dist = min(px.ts, 200.0);
    var top = vec3<f32>(0.0);
    var keep_scene = 1.0;
    if (px.rd.y > 0.0) {
        // Up to the surface.
        let ts = (u.centre.y + H_HI - u.eye.y) / px.rd.y;
        if (ts < px.ts) {
            dist = min(ts, dist);
            let window = smoothstep(0.6, 0.7, px.rd.y);
            // Outside the window the surface mirrors the water below.
            top = sky(px.rd) * window + glow * (1.0 - window);
            keep_scene = 0.0;
        }
    }
    let tr = pow(deep, vec3<f32>(dist / max(u.shape.w, 0.05)));
    o.m = tr * keep_scene;
    o.a = top * tr + glow * (vec3<f32>(1.0) - tr);
    return o;
}

fn eye_under() -> bool {
    if (lv().steps == 0 || !inside(u.eye.xz)) {
        return false;
    }
    return u.eye.y < u.centre.y + height(u.eye.xz, 0.05);
}
