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
// 16 lamp colour, intensity     17 lamp absorb (1/m), shade thickness (m)
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
    let g = water_slope(p.xz, u.time) * fade;
    let n = normalize(vec3<f32>(-g.x, 1.0, -g.y));
    let v = -px.rd;
    let f0 = mat(6u).x;
    let fr = schlick(f0, max(dot(n, v), 0.0)) * w5.z;
    // Shoreline foam: where the view leaves the water within the foam
    // width (a bed or a wall right under the surface), broken up by moving
    // noise. The path along the view, not the depth straight down, so a
    // steep wall gets a thin line at the waterline and not a wide band.
    let fw = max(mat(3u).w, 0.001);
    let near = select(0.0, 1.0 - smoothstep(0.0, fw, path), px.scene);
    let fn1 = vnoise(p.xz * 7.0 + vec2<f32>(u.time * 0.35, u.time * 0.2)).x;
    let fn2 = vnoise(p.xz * 17.0 - vec2<f32>(u.time * 0.25, 0.0)).x;
    let foam = clamp(near * (0.35 + fn1 * 0.9 + fn2 * 0.4) * w5.x - (1.0 - near) * 0.2, 0.0, 1.0);
    let ambient = mat(0u).rgb * sky_k() * 0.6 + u.sun_col * max(u.sun.y, 0.0) * 0.12;
    let avg_t = (trans.r + trans.g + trans.b) / 3.0;
    let inscatter = mat(3u).rgb * ambient * (1.0 - avg_t);
    let refl = sky(reflect(px.rd, n));
    let spec = glint(n, v, max(w5.w, 0.02), max(f0, 0.02)) * w4.w;
    o.m = (1.0 - fr) * trans * (1.0 - foam);
    o.a = fr * refl + spec * (1.0 - foam) + (1.0 - fr) * (1.0 - foam) * inscatter + foam * (ambient * 1.4 + u.sun_col * max(u.sun.y, 0.0) * 0.25);
    o.off = screen_bend(n) * w5.y * min(path, 2.0) * 0.05;
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

// Foliage, thin blobs and lamps on the ellipsoid the depth shows.
fn blob(s: Surf, px: Px, kind: i32) -> Hit {
    var o = miss();
    if (!px.scene) {
        return o;
    }
    let h = max(s.h.xyz, vec3<f32>(1e-3));
    let q = rot_y(px.ps - s.c.xyz, -s.h.w) / h;
    let d = length(q);
    if (d > 1.06) {
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
    if (flat) {
        return miss();
    }
    if (kind == 5 || kind == 7 || kind == 9) {
        return region(s, px, kind);
    }
    return blob(s, px, kind);
}

// ---- the pixel ----------------------------------------------------------

struct Shade {
    m: vec3<f32>,
    a: vec3<f32>,
    off: vec2<f32>,
    // Nearest see-through surface (view-depth units), FAR_T if none.
    tnear: f32,
    hit: bool,
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

fn shade(uv: vec2<f32>, z: f32, flat: bool) -> Shade {
    var px: Px;
    px.ray = pixel_ray(uv);
    px.rl = length(px.ray);
    px.rd = px.ray / px.rl;
    px.ts = scene_dist(z);
    px.scene = px.ts < FAR_T;
    px.ps = u.eye + px.ray * select(0.0, px.ts, px.scene);
    var ts: array<f32, 5>;
    var ms: array<vec3<f32>, 5>;
    var adds: array<vec3<f32>, 5>;
    var offs: array<vec2<f32>, 5>;
    let count = min(i32(u.count + 0.5), 5);
    var n = 0;
    for (var i = 0; i < count; i = i + 1) {
        let h = surface(u.s[i], px, flat);
        if (h.t > 0.0) {
            ts[n] = h.t;
            ms[n] = h.m;
            adds[n] = h.a;
            offs[n] = h.off;
            n = n + 1;
        }
    }
    var out: Shade;
    out.m = vec3<f32>(1.0);
    out.a = vec3<f32>(0.0);
    out.off = vec2<f32>(0.0);
    out.tnear = FAR_T;
    out.hit = n > 0;
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
        }
    }
    return out;
}
