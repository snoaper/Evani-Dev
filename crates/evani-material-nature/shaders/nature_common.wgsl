// Shared body of the nature shaders (appended after `nature.wgsl` or
// `nature_refract.wgsl`, which declare the bindings and `fs_main`).
//
// Each pixel rebuilds its world position from the scene depth and the
// camera basis (as evani-fx's decals do), then meets up to five analytic
// surfaces of this batch:
//   0 water ellipse, 1 water rectangle  (a flat surface at height c.y)
//   2 glass pane (thin box), 3 glass bottle (vertical capsule)
//   4 foliage (ellipsoid bush), 5 grass (box region of blades)
//   6 thin blob (ellipsoid: ear, fruit, wax), 7 thin patch (thin box: leaf,
//     paper), 8 lamp (ellipsoid shade lit from inside: lampshade, candle),
//   9 leaf (thin box lit through with the foliage colour)
//   10 glow (a lit shade's halo only, Lite and Low: a paper lantern)
// Water and glass sit in front of what the depth shows (it is hidden from
// the solid draw); foliage, grass and thin surfaces are the solid surface
// the depth shows, with light added. Every surface gives a multiplier M for
// what is behind it and light A on top; they are combined back to front.
//
// Maths credits: the back-light term is Filament's subsurface model
// (google/filament shading_model_subsurface.fs, Apache-2.0): forward
// scatter exp2(saturate(V.-L) p - p) mixed with a wrapped N.L; thickness is
// the chord the sunlight travels through the shape, attenuated Beer-Lambert
// style as Babylon.js's thin translucency does (Apache-2.0). Schlick
// Fresnel and the GGX lobe follow glTF-Sample-Viewer (Apache-2.0). The hash
// is the PCG hash of Jarzynski and Olano (JCGT 2020) after O'Neill's PCG
// (Apache-2.0). Integer hashes only, so the noise is the same on every GPU.

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: VsOut;
    o.pos = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(p.x, 1.0 - p.y);
    return o;
}

struct Surf {
    // Centre, shape code (see above).
    c: vec4<f32>,
    // Half size in metres (bottle: radius, half height of the straight
    // part), turn about +y in radians.
    h: vec4<f32>,
};

struct Nature {
    // Camera position; 1 / view distance = inv0 + inv1 * depth.
    eye: vec3<f32>,
    inv0: f32,
    // ray = fwd + right * ndc.x + up * ndc.y; world = eye + ray * distance.
    fwd: vec3<f32>,
    inv1: f32,
    right: vec3<f32>,
    // Seconds of fixed-step simulation (Time.tick * fixed_dt).
    time: f32,
    up: vec3<f32>,
    // 0 flat multiply and 4 flat add (Lite/Low), 1 multiply, 2 add,
    // 3 refract (High/Ultra).
    mode: f32,
    // Unit vector towards the sun, surfaces in this batch.
    sun: vec3<f32>,
    count: f32,
    // Sun colour times intensity; per-channel scale of the reflected sky
    // (the game's background against `sky.horizon`), packed: see sky_k.
    sun_col: vec3<f32>,
    sky_tint: f32,
    s: array<Surf, 5>,
};

// The material table (live params, rebuilt when they change):
//  0 sky horizon (w: match the background, read on the CPU), 1 sky zenith
//  2 water deep colour, clarity (m)    3 water scatter colour, foam width (m)
//  4 wave scale (1/m), speed, strength, glint
//  5 foam strength, refraction, reflection, roughness   6 water F0
//  7 glass tint, thickness (m)   8 glass F0, reflection, roughness, refraction
//  9 foliage colour, strength   10 power, wrap, absorb (1/m), variation
// 11 leaf cells per metre       12 grass colour, strength
// 13 grass power, spacing (m), variation, blade thickness (m)
// 14 thin colour, strength      15 thin power, wrap, absorb (1/m), thickness (m)
// 16 lamp colour, intensity     17 lamp absorb (1/m), shade thickness (m),
//    reach (m), pool strength
// 18 swell direction (deg), swell, choppiness, flow speed (m/s)
// 19 crest glow (sss) colour, strength
// 20 whitecap amount, whitecap threshold, flow direction (deg)
//    (rows 18-20: the ocean upgrade, every default off; see water_more)
// 21 lace, swash, swash spacing (m of depth), caustics
// 22 caustic scale (1/m), ripples, murk
//    (rows 21-22: the coast upgrade, every default off; see coast_more)
//@slot nature_mat

fn mat(i: u32) -> vec4<f32> {
    return slot_nature_mat(i);
}

const PI: f32 = 3.14159265;
const FAR_T: f32 = 1.0e9;

// ---- hashes and noise ---------------------------------------------------

fn pcg(x: u32) -> u32 {
    let s = x * 747796405u + 2891336453u;
    let w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (w >> 22u) ^ w;
}

fn hash2(c: vec2<i32>) -> f32 {
    let h = pcg(bitcast<u32>(c.x) ^ pcg(bitcast<u32>(c.y)));
    return f32(h >> 8u) * (1.0 / 16777216.0);
}

fn hash3(c: vec3<i32>) -> f32 {
    let h = pcg(bitcast<u32>(c.x) ^ pcg(bitcast<u32>(c.y) ^ pcg(bitcast<u32>(c.z))));
    return f32(h >> 8u) * (1.0 / 16777216.0);
}

// Value noise with its gradient: (value, d/dx, d/dy), value in 0..1.
fn vnoise(p: vec2<f32>) -> vec3<f32> {
    let i = vec2<i32>(floor(p));
    let f = p - floor(p);
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let du = 30.0 * f * f * (f * (f - 2.0) + 1.0);
    let a = hash2(i);
    let b = hash2(i + vec2<i32>(1, 0));
    let c = hash2(i + vec2<i32>(0, 1));
    let d = hash2(i + vec2<i32>(1, 1));
    let k = a - b - c + d;
    let v = a + (b - a) * u.x + (c - a) * u.y + k * u.x * u.y;
    let g = du * vec2<f32>(b - a + k * u.y, c - a + k * u.x);
    return vec3<f32>(v, g);
}

// Slope of the water surface (dh/dx, dh/dz) at world xz: four directional
// waves (deep-water speeds, w = sqrt(g k)) and two scrolling noise octaves.
// Mirrored in Rust (water.rs) for the determinism test.
fn water_slope(p: vec2<f32>, t: f32) -> vec2<f32> {
    let w = mat(4u);
    let scale = max(w.x, 0.01);
    let speed = w.y;
    var g = vec2<f32>(0.0);
    let dirs = array<vec2<f32>, 4>(
        vec2<f32>(0.8, 0.6),
        vec2<f32>(-0.39, 0.92),
        vec2<f32>(0.97, -0.24),
        vec2<f32>(-0.71, -0.7),
    );
    let freqs = array<f32, 4>(1.0, 1.73, 2.61, 3.9);
    for (var k = 0; k < 4; k = k + 1) {
        let f = freqs[k] * scale;
        let ph = dot(dirs[k], p) * f + t * speed * sqrt(9.81 * f);
        g = g + dirs[k] * (cos(ph) * 0.55 / freqs[k]);
    }
    let n1 = vnoise(p * scale * 4.3 + vec2<f32>(t * speed * 0.31, t * speed * 0.17));
    let n2 = vnoise(p * scale * 9.1 - vec2<f32>(t * speed * 0.23, -t * speed * 0.29));
    g = g + n1.yz * 0.18 + n2.yz * 0.07;
    return g * w.z;
}

// ---- the ocean upgrade (table rows 18-20) --------------------------------
// Swell direction, choppiness (Gerstner steepness), whitecaps from wave
// compression, a crest glow and river flow drift. Ideas from Ultra-Water
// (Humpalumps, MIT: its crest subsurface tint, Jacobian foam and flow-map
// drift) and GPU Gems ch. 1 (Gerstner normals); our own code, no FFT. Every
// default is off, and with them off the water runs exactly the code it ran
// before (`water_more` is false and `water_slope` is used unchanged).
// Mirrored in Rust (water.rs `waves`) for the determinism test.

// Whether any of the upgrade is on (a value the whole draw shares).
fn water_more() -> bool {
    return any(mat(18u) != vec4<f32>(0.0)) || mat(19u).w != 0.0 || mat(20u).x != 0.0;
}

// How far the flow has carried the surface (m) at sim time t: along
// `flow_direction` (0 deg = +x, 90 = +z) at `flow_speed`.
fn water_drift(t: f32) -> vec2<f32> {
    let a = radians(mat(20u).z);
    return vec2<f32>(cos(a), sin(a)) * (mat(18u).w * t);
}

// v turned by a radians (x towards z).
fn turn2(v: vec2<f32>, a: f32) -> vec2<f32> {
    let s = sin(a);
    let c = cos(a);
    return vec2<f32>(c * v.x - s * v.y, s * v.x + c * v.y);
}

// `water_slope` with the swell turned in, plus how high up the crests the
// point is: (dh/dx, dh/dz, crest), crest -1 (every wave's trough at once)
// to 1 (every crest at once), the wave compression that choppiness and
// whitecaps read. The four directions turn by `swell_direction` and gather
// round the first one by `swell` (a long swell instead of crossing seas).
// p already has the flow drift taken off.
fn water_waves(p: vec2<f32>, t: f32) -> vec3<f32> {
    let w = mat(4u);
    let sw = mat(18u);
    let scale = max(w.x, 0.01);
    let speed = w.y;
    let dirs = array<vec2<f32>, 4>(
        vec2<f32>(0.8, 0.6),
        vec2<f32>(-0.39, 0.92),
        vec2<f32>(0.97, -0.24),
        vec2<f32>(-0.71, -0.7),
    );
    let freqs = array<f32, 4>(1.0, 1.73, 2.61, 3.9);
    // A gathered swell's spread round its heading (radians).
    let spread = array<f32, 4>(0.0, 0.38, -0.52, 0.85);
    let turn = radians(sw.x);
    var g = vec2<f32>(0.0);
    var crest = 0.0;
    for (var k = 0; k < 4; k = k + 1) {
        var d = dirs[k];
        if (sw.y > 0.0) {
            d = normalize(mix(d, turn2(dirs[0], spread[k]), sw.y));
        }
        d = turn2(d, turn);
        let f = freqs[k] * scale;
        let ph = dot(d, p) * f + t * speed * sqrt(9.81 * f);
        g = g + d * (cos(ph) * 0.55 / freqs[k]);
        crest = crest + sin(ph) * 0.55 / freqs[k];
    }
    let n1 = vnoise(p * scale * 4.3 + vec2<f32>(t * speed * 0.31, t * speed * 0.17));
    let n2 = vnoise(p * scale * 9.1 - vec2<f32>(t * speed * 0.23, -t * speed * 0.29));
    g = g + n1.yz * 0.18 + n2.yz * 0.07;
    // 1.21967 = 0.55 x (1 + 1/1.73 + 1/2.61 + 1/3.9), the crests' sum.
    return vec3<f32>(g * w.z, crest / 1.21967);
}

// ---- the coast upgrade (table rows 21-22) --------------------------------
// Clear coastal water (the Asset Studio shoreline, 2026-11-06): shore foam
// broken into lace with holes, swash fronts along the bed's depth contours
// running in to the beach, the sun's caustic net on a shallow bed, fine
// ripples in other directions, and deep water that hides its bed (murk).
// Our own code (cellular noise after Worley 1996). Every default is off:
// until a cell of rows 21-22 is not 0 the water runs the code it ran before.

fn coast_more() -> bool {
    return any(mat(21u) != vec4<f32>(0.0)) || any(mat(22u) != vec4<f32>(0.0));
}

// Cellular noise at p (cell units): the distances to the nearest and the
// second nearest jittered point; their difference is 0 on cell borders.
fn cells2(p: vec2<f32>) -> vec2<f32> {
    let i = vec2<i32>(floor(p));
    let f = p - floor(p);
    var d = vec2<f32>(8.0);
    for (var y = -1; y <= 1; y = y + 1) {
        for (var x = -1; x <= 1; x = x + 1) {
            let c = i + vec2<i32>(x, y);
            let e = length(vec2<f32>(f32(x) + hash2(c), f32(y) + hash2(c + vec2<i32>(17, 59))) - f);
            d = select(vec2<f32>(d.x, min(d.y, e)), vec2<f32>(e, d.x), e < d.x);
        }
    }
    return d;
}

// Foam lace at p (m): thin threads on the borders of slowly drifting cells
// at two sizes, warped, with open holes between them (0..1).
fn coast_lace(p: vec2<f32>, t: f32) -> f32 {
    let w = vnoise(p * 1.3 + vec2<f32>(t * 0.05, 0.0)).x;
    let a = cells2(p * 4.0 + vec2<f32>(w * 1.7, t * 0.08));
    let b = cells2(p * 9.5 - vec2<f32>(t * 0.11, w * 2.3));
    let la = 1.0 - smoothstep(0.05, 0.3, a.y - a.x);
    let lb = 1.0 - smoothstep(0.03, 0.2, b.y - b.x);
    return clamp(max(la, lb * 0.8) * (0.6 + w * 0.6), 0.0, 1.0);
}

// The sun's caustics on a bed at q (m), `k` cells per metre: a net of
// bright lines where light gathers under the ripples, two drifting layers.
fn coast_caustic(q: vec2<f32>, t: f32, k: f32) -> f32 {
    // Warped so the cells' borders curve, the two layers at other sizes.
    let w = vnoise(q * k * 0.3 + vec2<f32>(t * 0.07, -t * 0.05)).x;
    let qw = q * k + vec2<f32>(vnoise(q * k * 0.55).x, vnoise(q * k * 0.55 + vec2<f32>(7.3, 1.9)).x) * 1.1;
    let a = cells2(qw + vec2<f32>(w * 1.2 + t * 0.12, t * 0.05));
    let b = cells2(qw * 1.37 + vec2<f32>(-t * 0.09, w * 1.5 + t * 0.07));
    let ea = 1.0 - smoothstep(0.0, 0.1, a.y - a.x);
    let eb = 1.0 - smoothstep(0.0, 0.1, b.y - b.x);
    return (ea * ea * 0.8 + eb * eb * 0.5 + ea * eb * 1.2) * (0.25 + w * 1.1);
}

// Fine ripples (a slope): three noise octaves turned to other headings
// than the waves, from `scale` (the wave scale) up.
fn coast_ripples(p: vec2<f32>, t: f32, scale: f32) -> vec2<f32> {
    let r1 = vnoise(turn2(p, 0.6) * scale * 3.1 + vec2<f32>(t * 0.6, 0.0)).yz;
    let r2 = vnoise(turn2(p, 2.1) * scale * 6.7 - vec2<f32>(t * 0.9, t * 0.4)).yz;
    let r3 = vnoise(turn2(p, 4.0) * scale * 14.3 + vec2<f32>(0.0, t * 1.3)).yz;
    return turn2(r1, -0.6) * 0.3 + turn2(r2, -2.1) * 0.16 + turn2(r3, -4.0) * 0.08;
}

// ---- shading helpers ----------------------------------------------------

fn schlick(f0: f32, c: f32) -> f32 {
    let x = 1.0 - clamp(c, 0.0, 1.0);
    let x2 = x * x;
    return f0 + (1.0 - f0) * x2 * x2 * x;
}

// GGX normal distribution, alpha = roughness^2.
fn ggx(noh: f32, rough: f32) -> f32 {
    let a = max(rough * rough, 0.002);
    let a2 = a * a;
    let d = noh * noh * (a2 - 1.0) + 1.0;
    return a2 / (PI * d * d);
}

// The sky scale: three 8-bit steps of 4/255 in one exact integer float
// (lib.rs pack_sky_tint).
fn sky_k() -> vec3<f32> {
    let p = u32(max(u.sky_tint, 0.0) + 0.5);
    return vec3<f32>(f32(p & 255u), f32((p >> 8u) & 255u), f32((p >> 16u) & 255u)) * (4.0 / 255.0);
}

// The sky a direction sees: the horizon's bright band is narrow, so the
// zenith colour comes in fast above it (a wave tilting the view up shows
// a bluer sky).
fn sky(dir: vec3<f32>) -> vec3<f32> {
    return mix(mat(0u).rgb, mat(1u).rgb, sqrt(clamp(dir.y, 0.0, 1.0))) * sky_k();
}

// Sun glint on a mirror-like surface.
fn glint(n: vec3<f32>, v: vec3<f32>, rough: f32, f0: f32) -> vec3<f32> {
    let l = u.sun;
    let h = normalize(v + l);
    let nol = max(dot(n, l), 0.0);
    let spec = ggx(max(dot(n, h), 0.0), rough) * schlick(f0, max(dot(v, h), 0.0)) * nol * 0.25;
    return u.sun_col * min(spec, 64.0);
}

// Filament's subsurface term: forward scatter towards the viewer when the
// light is behind, plus a wrapped N.L, both dimmed by the thickness (0..1).
fn subsurface(n: vec3<f32>, rd: vec3<f32>, power: f32, wrap: f32, thick: f32) -> f32 {
    let nol = dot(n, u.sun);
    let fwd = exp2(clamp(dot(rd, u.sun), 0.0, 1.0) * power - power);
    let back = clamp(nol * thick + 1.0 - thick, 0.0, 1.0) * 0.5 * wrap;
    return mix(back, 1.0, fwd) * (1.0 - thick);
}

// A world normal as a screen-space bend (uv units per unit).
fn screen_bend(n: vec3<f32>) -> vec2<f32> {
    return vec2<f32>(dot(n, normalize(u.right)), -dot(n, normalize(u.up)));
}

fn rot_y(v: vec3<f32>, a: f32) -> vec3<f32> {
    let s = sin(a);
    let c = cos(a);
    return vec3<f32>(c * v.x + s * v.z, v.y, -s * v.x + c * v.z);
}

// Distance the sunlight travels inside the unit sphere of the scaled space
// from q (|q| <= 1) along d (already divided by the half sizes).
fn chord(q: vec3<f32>, d: vec3<f32>) -> f32 {
    let a = max(dot(d, d), 1e-8);
    let b = dot(q, d);
    let c = min(dot(q, q) - 1.0, 0.0);
    return (-b + sqrt(max(b * b - a * c, 0.0))) / a;
}

// Leaf clusters: the nearest of the jittered points of the 8 cells round
// p (cell units). Returns (gap to the next nearest, that cluster's two
// hashes).
fn leaf_cells(p: vec3<f32>) -> vec3<f32> {
    let base = vec3<i32>(floor(p - 0.5));
    var f1 = 1.0e9;
    var f2 = 1.0e9;
    var h1 = 0.0;
    var h2 = 0.0;
    for (var k = 0; k < 8; k = k + 1) {
        let c = base + vec3<i32>(k & 1, (k >> 1u) & 1, (k >> 2u) & 1);
        let j = vec3<f32>(hash3(c), hash3(c + vec3<i32>(17, 5, 11)), hash3(c + vec3<i32>(3, 29, 7)));
        let d = length(p - (vec3<f32>(c) + 0.15 + 0.7 * j));
        if (d < f1) {
            f2 = f1;
            f1 = d;
            h1 = j.x;
            h2 = j.y;
        } else if (d < f2) {
            f2 = d;
        }
    }
    return vec3<f32>(f2 - f1, h1, h2);
}

// Grass blades over the ground the depth shows: ten shells from the blade
// tops down to the ground, each a slice of the tapering blades (one per
// cell of `sp` metres, each with its own place, height and sway). Returns
// (height up the blade 0..1, the blade's hash), or (-1, 0) when the view
// reaches the ground between them.
fn grass_blades(s: Surf, px: Px, h: vec3<f32>, sp: f32) -> vec2<f32> {
    let ground = px.ps.y;
    let bh = s.c.y + h.y - ground;
    if (bh < 0.005 || px.ray.y > -1e-5) {
        return vec2<f32>(1.0, hash2(vec2<i32>(floor(px.ps.xz / sp))));
    }
    for (var k = 0; k < 10; k = k + 1) {
        let yf = 1.0 - (f32(k) + 0.5) / 10.0;
        let t = (ground + bh * yf - u.eye.y) / px.ray.y;
        if (t <= 0.0) {
            continue;
        }
        let p = u.eye + px.ray * t;
        let sway = vec2<f32>(sin(u.time * 1.7 + p.x * 2.3), cos(u.time * 1.3 + p.z * 2.1)) * 0.3 * yf * yf;
        let q = p.xz / sp + sway;
        let c = vec2<i32>(floor(q));
        let f = q - floor(q);
        let root = vec2<f32>(0.2 + 0.6 * hash2(c), 0.2 + 0.6 * hash2(c + vec2<i32>(31, 7)));
        let hv = hash2(c + vec2<i32>(5, 57));
        let tall = 0.55 + 0.45 * hv;
        if (yf < tall && length(f - root) < 0.42 * (1.0 - yf / tall)) {
            return vec2<f32>(yf / tall, hv);
        }
    }
    return vec2<f32>(-1.0, 0.0);
}

// ---- one surface --------------------------------------------------------

struct Hit {
    // Ray distance in view-depth units (< 0: missed).
    t: f32,
    m: vec3<f32>,
    a: vec3<f32>,
    // Screen offset for refraction (High/Ultra), uv units.
    off: vec2<f32>,
};

fn miss() -> Hit {
    return Hit(-1.0, vec3<f32>(1.0), vec3<f32>(0.0), vec2<f32>(0.0));
}

// Everything a surface needs about this pixel.
struct Px {
    ray: vec3<f32>,
    rl: f32,
    rd: vec3<f32>,
    // Scene distance (view-depth units), FAR_T for the sky.
    ts: f32,
    ps: vec3<f32>,
    scene: bool,
    // How far the scene's height changes from one pixel to the next (m):
    // the coast upgrade's swash fades where its lines would crowd into
    // stripes (a steep rock side under the surface). 0 where unknown.
    gy: f32,
    // How far the scene point moves across the ground from one pixel to
    // the next (m): with `gy`, the bed's slope (whitecap_depth).
    gx: f32,
};

fn water(s: Surf, px: Px, flat: bool) -> Hit {
    var o = miss();
    if (u.eye.y <= s.c.y || abs(px.ray.y) < 1e-6) {
        return o;
    }
    let tw = (s.c.y - u.eye.y) / px.ray.y;
    if (tw <= 0.0 || tw >= px.ts) {
        return o;
    }
    let p = u.eye + px.ray * tw;
    let l = rot_y(p - s.c.xyz, -s.h.w);
    let shape = s.c.w;
    if (shape < 0.5) {
        let e = (l.x * l.x) / max(s.h.x * s.h.x, 1e-6) + (l.z * l.z) / max(s.h.z * s.h.z, 1e-6);
        if (e > 1.0) {
            return o;
        }
    } else if (abs(l.x) > s.h.x || abs(l.z) > s.h.z) {
        return o;
    }
    o.t = tw;
    let deep = clamp(mat(2u).rgb, vec3<f32>(0.01), vec3<f32>(1.0));
    // Metres of water the view ray crosses before the bed (capped over sky).
    let path = (min(px.ts, tw + 400.0 / px.rl) - tw) * px.rl;
    let trans = pow(deep, vec3<f32>(path / max(mat(2u).w, 0.01)));
    if (flat) {
        // Lite and Low: no waves, no foam. The multiply draw tints by the
        // water colour; the add draw gives the body colour lit by the sky
        // and a calm Fresnel sky, so water reads even over a dark bed.
        let fr0 = schlick(mat(6u).x, max(-px.rd.y, 0.0)) * mat(5u).z;
        let amb = mat(0u).rgb * sky_k() * 0.6 + u.sun_col * max(u.sun.y, 0.0) * 0.12;
        let avg0 = (trans.r + trans.g + trans.b) / 3.0;
        o.m = (1.0 - fr0) * max(trans * mix(vec3<f32>(1.0), deep, 0.4), deep * 0.5);
        o.a = fr0 * sky(reflect(px.rd, vec3<f32>(0.0, 1.0, 0.0))) + (1.0 - fr0) * mat(3u).rgb * amb * max(1.0 - avg0, 0.35);
        return o;
    }
    let w4 = mat(4u);
    let w5 = mat(5u);
    // Waves fade with distance so far water doesn't sparkle.
    let fade = 1.0 / (1.0 + tw * px.rl * 0.03);
    // The ocean upgrade (off by default: then this is the old code path).
    let more = water_more();
    let oc = mat(18u);
    var pf = p.xz;
    var g: vec2<f32>;
    var crest = 0.0;
    var ny = 1.0;
    if (more) {
        pf = p.xz - water_drift(u.time);
        let wv = water_waves(pf, u.time);
        g = wv.xy * fade;
        crest = wv.z;
        // Choppiness, Gerstner style: the normal's up part shrinks on the
        // crests and grows in the troughs (1 - Q sum w A sin), so crests read
        // sharp and steep and troughs flat.
        ny = max(1.0 - oc.z * 0.8 * crest * fade, 0.2);
    } else {
        g = water_slope(p.xz, u.time) * fade;
    }
    // The coast upgrade (off by default: then none of it runs).
    let coast = coast_more();
    let c21 = mat(21u);
    let c22 = mat(22u);
    if (coast) {
        g = g + coast_ripples(pf, u.time, max(mat(4u).x, 0.01)) * (c22.y * fade);
    }
    let n = normalize(vec3<f32>(-g.x, ny, -g.y));
    let v = -px.rd;
    let f0 = mat(6u).x;
    // The Fresnel weight from a calmer normal than the waves' own: steep
    // wave faces at a low view turned the whole pool into milky sky on High
    // and Ultra while Lite and Low showed the bed (critic round 2 of
    // material-view: the same water must read the same on every tier).
    let nf = normalize(mix(vec3<f32>(0.0, 1.0, 0.0), n, 0.3));
    let fr = schlick(f0, max(dot(nf, v), 0.0)) * w5.z;
    // Shoreline foam: where the view leaves the water within the foam
    // width (a bed or a wall right under the surface), broken up by moving
    // noise. The path along the view, not the depth straight down, so a
    // steep wall gets a thin line at the waterline and not a wide band.
    let fw = max(mat(3u).w, 0.001);
    let near = select(0.0, 1.0 - smoothstep(0.0, fw, path), px.scene);
    let fn1 = vnoise(pf * 7.0 + vec2<f32>(u.time * 0.35, u.time * 0.2)).x;
    let fn2 = vnoise(pf * 17.0 - vec2<f32>(u.time * 0.25, 0.0)).x;
    var foam = clamp(near * (0.35 + fn1 * 0.9 + fn2 * 0.4) * w5.x - (1.0 - near) * 0.2, 0.0, 1.0);
    let wc = mat(20u);
    if (more && wc.x > 0.0) {
        // Whitecaps where the waves pile up into crests (Ultra-Water foams
        // where the Jacobian of its displacement drops; here the crest
        // height the slope sum already has), steeper seas break sooner,
        // in patches from one more noise sample, laced by the foam noise.
        let start = 0.3 + wc.y * 0.55;
        let blot = vnoise(pf * max(w4.x, 0.01) * 1.7 + vec2<f32>(u.time * w4.y * 0.05, 0.0)).x;
        var c = crest + oc.z * 0.15 + (blot - 0.5) * 0.6;
        // A river (whitecap_depth > 0): broken water only where the bed
        // runs shallow and level (a riffle; not the shelving margin along
        // every bank), breaking sooner there; calm over the pools and runs.
        var reach = 1.0;
        if (wc.w > 0.0) {
            let dz = select(9.0, s.c.y - px.ps.y, px.scene);
            let lvl = 1.0 - smoothstep(0.12, 0.35, px.gy / max(px.gx, 1e-4));
            reach = (1.0 - smoothstep(wc.w * 0.45, wc.w, dz)) * lvl;
            // Broken into runs some 10 m long with calm stretches between
            // (a riffle breaks where its stones lie, not as one even strip
            // of dashes down the whole margin).
            reach = reach * smoothstep(0.38, 0.62, vnoise(pf * 0.1 + vec2<f32>(3.1, 7.3)).x);
            c = c + reach * 0.3;
        }
        let edge = smoothstep(start, start + 0.1, c);
        let lace = smoothstep(0.5, 1.15, fn1 * 0.7 + fn2 * 0.5 + edge * 0.3);
        let cap = edge * lace * wc.x * reach;
        foam = max(foam, clamp(cap, 0.0, 1.0));
    }
    var caus = 0.0;
    if (coast) {
        // Depth of the bed this pixel sees (m; deep where it sees none).
        let dz = select(9.0, s.c.y - px.ps.y, px.scene);
        // The shore band broken into lace: its threads and a thin lip
        // right at the edge stay, the holes between them open.
        let lace = coast_lace(pf, u.time);
        let lip = select(0.0, 1.0 - smoothstep(0.0, fw * 0.2, path), px.scene);
        foam = foam * mix(1.0, max(lace, lip), c21.x);
        if (c21.y > 0.0) {
            // Swash fronts: foam lines along the depth contours, every
            // `swash_spacing` metres of depth, running in towards the
            // beach, broken into stretches and lace, fading with depth.
            let sp = max(c21.z, 0.005);
            let ph = dz / sp + u.time * 0.1 + (fn1 - 0.5) * 0.6;
            let line = 1.0 - smoothstep(0.04, 0.2, abs(fract(ph + 0.5) - 0.5));
            let run = smoothstep(0.3, 0.6, vnoise(pf * 0.8 + vec2<f32>(u.time * 0.03, 3.7)).x);
            let crowd = 1.0 - smoothstep(0.04 * sp, 0.14 * sp, px.gy);
            let swash = line * run * crowd * (0.55 + 0.45 * lace) * exp(-dz / (sp * 2.2)) * step(sp * 0.6, dz);
            foam = max(foam, clamp(swash * c21.y, 0.0, 1.0));
        }
        if (c21.w > 0.0 && px.scene) {
            // Caustics, strongest a hand's depth down, gone in the deep.
            caus = c21.w * coast_caustic(px.ps.xz, u.time, max(c22.x, 0.1)) * smoothstep(0.0, 0.1, dz) * exp(-dz / 0.8) * max(u.sun.y, 0.0);
        }
    }
    let ambient = mat(0u).rgb * sky_k() * 0.6 + u.sun_col * max(u.sun.y, 0.0) * 0.12;
    // Foam is white lit by the sky and sun; it covers what lies under it
    // only as far as that light shows it. At night its light is near black,
    // and covering a lamp-lit wall at full strength drew dark smudges along
    // the waterline (material-view critic round 13, debug-lamp4).
    let foam_lit = ambient * 1.4 + u.sun_col * max(u.sun.y, 0.0) * 0.25;
    foam = foam * clamp(dot(foam_lit, vec3<f32>(0.2126, 0.7152, 0.0722)) * 4.0, 0.0, 1.0);
    let avg_t = (trans.r + trans.g + trans.b) / 3.0;
    // Body colour and tint as the flat path has them (below), so shallow
    // water keeps its colour on every tier.
    let inscatter = mat(3u).rgb * ambient * max(1.0 - avg_t, 0.35);
    let refl = sky(reflect(px.rd, n));
    let spec = glint(n, v, max(w5.w, 0.02), max(f0, 0.02)) * w4.w;
    o.m = (1.0 - fr) * max(trans * mix(vec3<f32>(1.0), deep, 0.4), deep * 0.5) * (1.0 - foam);
    o.a = fr * refl + spec * (1.0 - foam) + (1.0 - fr) * (1.0 - foam) * inscatter + foam * foam_lit;
    if (coast) {
        // Murk: deep water hides its bed (0 = the floor at half the deep
        // colour as before); caustics brighten the bed under the shallows.
        o.m = (1.0 - fr) * max(trans * mix(vec3<f32>(1.0), deep, 0.4), deep * (0.5 - 0.5 * c22.z)) * (1.0 - foam) * (1.0 + caus);
    }
    let ss = mat(19u);
    if (more && ss.w > 0.0) {
        // Crest glow: sunlight through the thin top of a wave, strongest
        // looking towards a low sun (Ultra-Water's crest subsurface idea).
        let toward = pow(clamp(dot(px.rd, u.sun), 0.0, 1.0), 3.0);
        let h = clamp(crest * fade * 1.6, 0.0, 1.0);
        let lit = smoothstep(-0.05, 0.1, u.sun.y);
        o.a = o.a + (1.0 - fr) * (1.0 - foam) * ss.rgb * u.sun_col * ((0.35 + 0.65 * toward) * h * h * ss.w * 0.35 * lit);
    }
    // The bend comes from the waves' tilt only (the normal less straight
    // up): calm water bends nothing. Projected whole, a flat pool's up
    // normal onto the camera's up axis bent every pixel up the screen by
    // most of 0.05 x its path (9% of the frame at 2 m), so High and Ultra
    // showed the basin's pale far wall all over the water instead of its
    // bed (critic round 4 of material-view: milky water, bed hidden, while
    // Medium and Low, which don't bend, showed the bed).
    o.off = screen_bend(n - vec3<f32>(0.0, 1.0, 0.0)) * w5.y * min(path, 2.0) * 0.05;
    return o;
}

// Ray against an axis-aligned box of half size h at the origin: (near, far,
// axis of the near face 0..2).
fn box_hit(ro: vec3<f32>, rd: vec3<f32>, h: vec3<f32>) -> vec3<f32> {
    let inv = 1.0 / select(rd, vec3<f32>(1e-8), abs(rd) < vec3<f32>(1e-8));
    let t0 = (-h - ro) * inv;
    let t1 = (h - ro) * inv;
    let lo = min(t0, t1);
    let hi = max(t0, t1);
    var near = lo.x;
    var axis = 0.0;
    if (lo.y > near) {
        near = lo.y;
        axis = 1.0;
    }
    if (lo.z > near) {
        near = lo.z;
        axis = 2.0;
    }
    let far = min(min(hi.x, hi.y), hi.z);
    if (far < max(near, 0.0)) {
        return vec3<f32>(-1.0, -1.0, 0.0);
    }
    return vec3<f32>(near, far, axis);
}

// Ray (unit rd, metres) against a vertical capsule: radius r, straight part
// from y = -hh to hh. Returns (near, far) or (-1, -1); n gets the near normal.
fn capsule_hit(ro: vec3<f32>, rd: vec3<f32>, r: f32, hh: f32, n: ptr<function, vec3<f32>>) -> vec2<f32> {
    var near = FAR_T;
    var far = -1.0;
    let a = rd.x * rd.x + rd.z * rd.z;
    if (a > 1e-8) {
        let b = ro.x * rd.x + ro.z * rd.z;
        let c = ro.x * ro.x + ro.z * ro.z - r * r;
        let disc = b * b - a * c;
        if (disc > 0.0) {
            let sq = sqrt(disc);
            let t0 = (-b - sq) / a;
            let t1 = (-b + sq) / a;
            let y0 = ro.y + rd.y * t0;
            let y1 = ro.y + rd.y * t1;
            if (abs(y0) <= hh && t0 < near) {
                near = t0;
                let p = ro + rd * t0;
                *n = vec3<f32>(p.x, 0.0, p.z) / r;
            }
            if (abs(y1) <= hh) {
                far = max(far, t1);
            }
        }
    }
    for (var k = 0; k < 2; k = k + 1) {
        let cy = select(-hh, hh, k == 1);
        let oc = ro - vec3<f32>(0.0, cy, 0.0);
        let b = dot(oc, rd);
        let c = dot(oc, oc) - r * r;
        let disc = b * b - c;
        if (disc > 0.0) {
            let sq = sqrt(disc);
            let t0 = -b - sq;
            let t1 = -b + sq;
            let y0 = ro.y + rd.y * t0;
            let y1 = ro.y + rd.y * t1;
            // Only the cap half beyond the straight part counts.
            if ((y0 - cy) * select(-1.0, 1.0, k == 1) >= 0.0 && t0 < near) {
                near = t0;
                *n = (oc + rd * t0) / r;
            }
            if ((y1 - cy) * select(-1.0, 1.0, k == 1) >= 0.0) {
                far = max(far, t1);
            }
        }
    }
    if (near >= FAR_T || far < max(near, 0.0)) {
        return vec2<f32>(-1.0, -1.0);
    }
    return vec2<f32>(near, far);
}

fn glass(s: Surf, px: Px, flat: bool) -> Hit {
    var o = miss();
    let ro = rot_y(u.eye - s.c.xyz, -s.h.w);
    let rd = rot_y(px.rd, -s.h.w);
    var tn = -1.0;
    var tf = -1.0;
    var nl = vec3<f32>(0.0, 1.0, 0.0);
    var bottle = false;
    if (s.c.w < 2.5) {
        let b = box_hit(ro, rd, s.h.xyz);
        tn = b.x;
        tf = b.y;
        if (b.z < 0.5) {
            nl = vec3<f32>(1.0, 0.0, 0.0);
        } else if (b.z < 1.5) {
            nl = vec3<f32>(0.0, 1.0, 0.0);
        } else {
            nl = vec3<f32>(0.0, 0.0, 1.0);
        }
        nl = select(nl, -nl, dot(nl, rd) > 0.0);
    } else {
        let c = capsule_hit(ro, rd, s.h.x, s.h.y, &nl);
        tn = c.x;
        tf = c.y;
        bottle = true;
    }
    // Metres to view-depth units.
    let tnv = tn / px.rl;
    if (tn <= 0.0 || tnv >= px.ts) {
        return o;
    }
    o.t = tnv;
    // Glass costs the same on every tier (no waves to skip): Lite and Low
    // draw it whole too, only without a bottle's bend.
    let g7 = mat(7u);
    let tint = clamp(g7.rgb, vec3<f32>(0.01), vec3<f32>(1.0));
    let g8 = mat(8u);
    let n = normalize(rot_y(nl, s.h.w));
    let v = -px.rd;
    let nv = max(abs(dot(n, v)), 0.02);
    let fr = schlick(g8.x, nv);
    // Glass crossed: a pane's real path, a bottle's two walls (more at the
    // rim, so its edges read darker).
    var units = 2.0 / max(nv, 0.2);
    var streak = 0.0;
    if (!bottle) {
        units = (min(tf / px.rl, px.ts) - tnv) * px.rl / max(g7.w, 1e-4);
        // The edge: light runs along the glass there, so a pane shows a
        // darker tinted rim (what makes real glass readable). Its width
        // follows the pane (6% of its smaller side), not only the glass
        // thickness, so it stays a few pixels wide at any distance a pane
        // is worth drawing.
        let lp = ro + rd * tn;
        let hb = s.h.xyz;
        var e = min(hb.x - abs(lp.x), hb.y - abs(lp.y));
        var face = min(hb.x, hb.y);
        var pl = lp.xy;
        if (hb.y < hb.z && hb.y <= hb.x) {
            e = min(hb.x - abs(lp.x), hb.z - abs(lp.z));
            face = min(hb.x, hb.z);
            pl = lp.xz;
        } else if (hb.x < hb.z && hb.x < hb.y) {
            e = min(hb.y - abs(lp.y), hb.z - abs(lp.z));
            face = min(hb.y, hb.z);
            pl = lp.zy;
        }
        let rim = max(clamp(g7.w * 2.5, 0.005, 0.05), face * 0.12);
        units = units + 12.0 * (1.0 - smoothstep(0.0, rim, e));
        // Two soft diagonal sheen bands across the pane (the room's light
        // caught in the glass), fixed to the pane so they don't swim.
        let d = (pl.x + pl.y * 0.6) / max(face, 1e-3);
        streak = pow(max(sin(d * 2.6 + 0.8), 0.0), 8.0) + 0.6 * pow(max(sin(d * 6.1 - 0.4), 0.0), 16.0);
    }
    let trans = pow(tint, vec3<f32>(units));
    let refl = sky(reflect(px.rd, n));
    o.m = (1.0 - fr) * (1.0 - fr) * trans;
    o.a = (fr + streak * 0.07) * refl * g8.y + glint(n, v, max(g8.z, 0.02), max(g8.x, 0.02));
    if (bottle && !flat) {
        o.off = screen_bend(n) * g8.w * 0.02;
    }
    return o;
}

// A lamp's glow round its shade where there is no bloom (`flat`: Lite and
// Low): the light a view ray passes close to on its way to what lies
// behind the lamp, falling off over about the shade's own size. Critic round
// 4 of material-view: on Low the lit shade was a flat cream disc, where
// High and Ultra bloom it. High and Ultra are unchanged (their bloom glows).
fn lamp_glow(s: Surf, px: Px, flat: bool) -> vec3<f32> {
    let r = max(s.h.x, max(s.h.y, s.h.z));
    let oc = s.c.xyz - u.eye;
    let tc = dot(oc, px.rd);
    // The lamp lies in front of what the pixel shows (or the pixel is sky).
    let past = select(FAR_T, px.ts * px.rl, px.scene);
    if (!flat || tc <= 0.0 || tc >= past || r <= 0.0) {
        return vec3<f32>(0.0);
    }
    let dd = length(oc - px.rd * tc) / r;
    let g = exp(-max(dd - 0.85, 0.0) * 2.2) * smoothstep(4.0, 1.0, dd);
    return mat(16u).rgb * (0.35 * max(mat(16u).w, 0.0) * g);
}

// A glow (kind 10): the halo of a shade lit from inside whose lit face the
// scene already draws (a paper lantern's paper), where there is no bloom
// (Lite and Low), as `lamp_glow` gives a lamp; `h.w` is its strength against
// the lamp's. No pool of light. Critic round 5 of material-view: the lantern
// glowed on Ultra (bloom) and stood unlit on Low.
fn glow_only(s: Surf, px: Px, flat: bool) -> Hit {
    var o = miss();
    let gl = lamp_glow(s, px, flat) * max(s.h.w, 0.0);
    if (dot(gl, gl) > 0.0) {
        o.t = select(FAR_T * 0.5, px.ts, px.scene);
        o.a = gl;
    }
    return o;
}

// Foliage, thin blobs and lamps on the ellipsoid the depth shows.
fn blob(s: Surf, px: Px, kind: i32, flat: bool) -> Hit {
    var o = miss();
    if (kind == 8 && !px.scene) {
        // A lamp seen against the sky still glows on Lite and Low.
        let gl = lamp_glow(s, px, flat);
        if (dot(gl, gl) > 0.0) {
            o.t = FAR_T * 0.5;
            o.a = gl;
        }
        return o;
    }
    if (!px.scene) {
        return o;
    }
    let h = max(s.h.xyz, vec3<f32>(1e-3));
    let q = rot_y(px.ps - s.c.xyz, -s.h.w) / h;
    let d = length(q);
    if (d > 1.06) {
        // A lamp's pool of light: what lies within its reach is lit warm,
        // fading to nothing at the edge (no normals in this pass, so every
        // face near the lamp takes it). Multiplied in for what is lit
        // already, added for what the night left dark. The same on every
        // tier: no point-light budget.
        let p17 = mat(17u);
        if (kind == 8 && p17.z > 0.0) {
            let dw = length(px.ps - s.c.xyz);
            let f = 1.0 - clamp(dw / p17.z, 0.0, 1.0);
            if (f > 0.0) {
                // Against daylight a lamp's light is lost: it fades out as
                // the sun grows brighter than the lamp's own pool.
                let day = clamp(dot(u.sun_col, vec3<f32>(0.3, 0.55, 0.15)) * max(u.sun.y, 0.0) * 1.5, 0.0, 1.0);
                // A soft edge (smoothstep), full near the lamp: the ground
                // under a 1.5 m lamp sits well inside its reach and must
                // read as lit, not as a faint tint.
                let k = f * f * (3.0 - 2.0 * f) * p17.w * (1.0 - day);
                let lc = mat(16u).rgb;
                o.t = px.ts;
                o.m = vec3<f32>(1.0) + lc * (0.6 * k);
                o.a = lc * (0.3 * k * max(mat(16u).w, 0.0));
            }
            // The glow round the shade, over what lies behind it.
            let gl = lamp_glow(s, px, flat);
            if (dot(gl, gl) > 0.0) {
                o.t = px.ts;
                o.a = o.a + gl;
            }
        }
        return o;
    }
    o.t = px.ts;
    let n = normalize(rot_y(q / h, s.h.w));
    let ls = rot_y(u.sun, -s.h.w) / h;
    let ch = chord(q / max(d, 1.0), ls);
    if (kind == 4) {
        let p9 = mat(9u);
        let p10 = mat(10u);
        let lc = leaf_cells(px.ps * max(mat(11u).x, 0.1));
        let hv = lc.y;
        let hv2 = lc.z;
        let thick = 1.0 - exp(-ch * p10.z);
        let col = mix(p9.rgb, p9.rgb * vec3<f32>(1.3, 1.15, 0.55), hv2 * p10.w);
        let ss = subsurface(n, px.rd, p10.x, p10.y, thick);
        // Each cluster its own shade; the gaps between clusters darker.
        let gap = mix(1.0 - 0.5 * p10.w, 1.0, smoothstep(0.0, 0.22, lc.x));
        o.m = vec3<f32>((1.0 - p10.w * 0.45 * hv) * gap);
        o.a = col * u.sun_col * ss * p9.w * (0.75 + 0.5 * hv) / PI;
    } else if (kind == 6) {
        let p14 = mat(14u);
        let p15 = mat(15u);
        let thick = 1.0 - exp(-ch * p15.z);
        o.a = p14.rgb * u.sun_col * subsurface(n, px.rd, p15.x, p15.y, thick) * p14.w / PI;
    } else {
        // Lamp: light from inside crossing the shade, less where the view
        // runs along it (darker rim), plus the sun from behind.
        let p16 = mat(16u);
        let p17 = mat(17u);
        let nv = max(abs(dot(n, -px.rd)), 0.15);
        let inner = p16.rgb * p16.w * exp(-p17.x * p17.y / nv);
        let p15 = mat(15u);
        let thick = 1.0 - exp(-p17.y * p15.z);
        o.a = inner + mat(14u).rgb * u.sun_col * subsurface(n, px.rd, p15.x, p15.y, thick) * mat(14u).w / PI;
    }
    return o;
}

// Grass blades and thin patches inside a box region of the depth.
fn region(s: Surf, px: Px, kind: i32) -> Hit {
    var o = miss();
    if (!px.scene) {
        return o;
    }
    let l = rot_y(px.ps - s.c.xyz, -s.h.w);
    let h = max(s.h.xyz, vec3<f32>(1e-3));
    if (any(abs(l) > h * 1.02 + vec3<f32>(0.01))) {
        return o;
    }
    o.t = px.ts;
    if (kind == 5) {
        let p12 = mat(12u);
        let p13 = mat(13u);
        let b = grass_blades(s, px, h, max(p13.y, 0.005));
        if (b.x < 0.0) {
            // The view reaches the ground between the blades: shaded soil.
            o.m = vec3<f32>(0.45);
            return o;
        }
        let hf = b.x;
        let hv = b.y;
        // A blade faces the viewer; its tip glows most.
        let n = normalize(vec3<f32>(-px.rd.x, 0.25, -px.rd.z));
        let ch = p13.w / max(abs(dot(n, u.sun)), 0.2);
        let thick = 1.0 - exp(-ch * 40.0);
        let tip = hf * hf;
        let col = mix(p12.rgb, p12.rgb * vec3<f32>(1.35, 1.2, 0.5), hv * p13.z);
        // The blade takes the lit ground's colour, darker at the root.
        let shade = mix(0.55, 1.25, hf) * (1.0 - p13.z * 0.35 * hv);
        o.m = vec3<f32>(shade) * mix(vec3<f32>(1.0), vec3<f32>(1.12, 1.08, 0.7), hv * p13.z);
        o.a = col * u.sun_col * subsurface(n, px.rd, p13.x, 1.0, thick) * p12.w * tip * (0.6 + 0.8 * hv) / PI;
    } else {
        // Thin patch: its normal is its thinnest axis.
        var nl = vec3<f32>(1.0, 0.0, 0.0);
        var th = h.x;
        if (h.y < th) {
            nl = vec3<f32>(0.0, 1.0, 0.0);
            th = h.y;
        }
        if (h.z < th) {
            nl = vec3<f32>(0.0, 0.0, 1.0);
            th = h.z;
        }
        var n = rot_y(nl, s.h.w);
        n = select(n, -n, dot(n, px.rd) > 0.0);
        // Paper and sails glow with the thin colour, leaves with the
        // foliage colour.
        let tc = select(mat(14u), mat(9u), kind == 9);
        let p15 = mat(15u);
        let ch = max(p15.w, th * 2.0) / max(abs(dot(n, u.sun)), 0.1);
        let thick = 1.0 - exp(-ch * p15.z);
        o.a = tc.rgb * u.sun_col * subsurface(n, px.rd, p15.x, p15.y, thick) * tc.w / PI;
    }
    return o;
}

fn surface(s: Surf, px: Px, flat: bool) -> Hit {
    let kind = i32(s.c.w + 0.5);
    if (kind <= 1) {
        return water(s, px, flat);
    }
    if (kind <= 3) {
        return glass(s, px, flat);
    }
    // Foliage, grass and thin surfaces draw on every tier (critic round 1:
    // skipped on Lite and Low, leaves were their dark base colour and grass a
    // bare slab); the flat draws multiply and add as the others do.
    if (kind == 5 || kind == 7 || kind == 9) {
        return region(s, px, kind);
    }
    if (kind == 10) {
        return glow_only(s, px, flat);
    }
    return blob(s, px, kind, flat);
}

// ---- the pixel ----------------------------------------------------------

struct Shade {
    m: vec3<f32>,
    a: vec3<f32>,
    off: vec2<f32>,
    // Nearest see-through surface (view-depth units), FAR_T if none.
    tnear: f32,
    hit: bool,
    // Index in `u.s` of the surface `off` belongs to.
    idx: u32,
};

fn pixel_ray(uv: vec2<f32>) -> vec3<f32> {
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    return u.fwd + u.right * ndc.x + u.up * ndc.y;
}

fn scene_dist(z: f32) -> f32 {
    // Reversed depth: 0 is the far plane (the sky).
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
    px.ts = scene_dist(z);
    px.scene = px.ts < FAR_T;
    px.ps = u.eye + px.ray * select(0.0, px.ts, px.scene);
    return px;
}

// Refraction may only bend onto what is itself seen through surface i: the
// pixel at uv (scene depth z) must have its own view ray cross that surface
// before its scene point. Being behind the surface is not enough: past a
// basin's far rim the bend lands on the lawn behind the basin, which is
// behind the water too, and painted a pale wedge of lawn into a night pool
// on Ultra (material-view round 4).
fn seen_through(i: u32, uv: vec2<f32>, z: f32) -> bool {
    return surface(u.s[i], pixel(uv, z), true).t > 0.0;
}

fn shade(uv: vec2<f32>, z: f32, flat: bool) -> Shade {
    var px = pixel(uv, z);
    // Uniform control flow here (both fragment entries call shade first).
    px.gy = fwidth(px.ps.y);
    px.gx = length(fwidth(px.ps.xz));
    var ts: array<f32, 5>;
    var ms: array<vec3<f32>, 5>;
    var adds: array<vec3<f32>, 5>;
    var offs: array<vec2<f32>, 5>;
    var ids: array<u32, 5>;
    let count = min(i32(u.count + 0.5), 5);
    var n = 0;
    for (var i = 0; i < count; i = i + 1) {
        let h = surface(u.s[i], px, flat);
        if (h.t > 0.0) {
            ts[n] = h.t;
            ms[n] = h.m;
            adds[n] = h.a;
            offs[n] = h.off;
            ids[n] = u32(i);
            n = n + 1;
        }
    }
    var out: Shade;
    out.m = vec3<f32>(1.0);
    out.a = vec3<f32>(0.0);
    out.off = vec2<f32>(0.0);
    out.tnear = FAR_T;
    out.hit = n > 0;
    out.idx = 0u;
    // Back to front: the farthest left first.
    var done = 0u;
    for (var k = 0; k < n; k = k + 1) {
        var best = -1;
        var bt = -1.0;
        for (var j = 0; j < n; j = j + 1) {
            if ((done & (1u << u32(j))) == 0u && ts[j] > bt) {
                bt = ts[j];
                best = j;
            }
        }
        done = done | (1u << u32(best));
        out.m = out.m * ms[best];
        out.a = out.a * ms[best] + adds[best];
        if (bt < px.ts * 0.9999) {
            out.tnear = bt;
            out.off = offs[best];
            out.idx = ids[best];
        }
    }
    return out;
}
