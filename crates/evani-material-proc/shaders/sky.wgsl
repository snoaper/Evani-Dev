// ---- sky ----------------------------------------------------------------------
//
// The sky dome, ported from three.js examples/jsm/objects/Sky.js (its
// Preetham sky and its clouds), with three fbm octaves instead of four. The
// model: A. J. Preetham, P. Shirley, B. Smits, "A Practical Analytic Model
// for Daylight", SIGGRAPH 1999. three.js licence:
//
//   The MIT License
//
//   Copyright © 2010-2026 three.js authors
//
//   Permission is hereby granted, free of charge, to any person obtaining a
//   copy of this software and associated documentation files (the
//   "Software"), to deal in the Software without restriction, including
//   without limitation the rights to use, copy, modify, merge, publish,
//   distribute, sublicense, and/or sell copies of the Software, and to
//   permit persons to whom the Software is furnished to do so, subject to
//   the following conditions:
//
//   The above copyright notice and this permission notice shall be included
//   in all copies or substantial portions of the Software.
//
//   THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
//   OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
//   MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
//   IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
//   CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT,
//   TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE
//   SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
//
// Records: 3 = sun direction (towards the sun, unit) and time (s); 4 =
// turbidity, rayleigh, mie coefficient, mie directional g; 5 = cloud scale,
// speed, coverage, density; 6 = cloud elevation, brightness, sun disc (0/1).
// The CPU mirror (src/sky.rs `radiance`) must follow any change here.

const MP_PI: f32 = 3.141592653589793;
// 8 pi^3 (n^2 - 1)^2 (6 + 3 pn) / (3 N lambda^4 (6 - 7 pn)) per primary.
const MP_TOTAL_RAYLEIGH: vec3<f32> = vec3<f32>(5.804542996261093e-6, 1.3562911419845635e-5, 3.0265902468824876e-5);
// pi ((2 pi) / lambda)^(v - 2) K per primary.
const MP_MIE_CONST: vec3<f32> = vec3<f32>(1.8399918514433978e14, 2.7798023919660528e14, 4.0790479543861094e14);
// The sun's angular diameter (as three.js has it), as a cosine.
const MP_SUN_COS: f32 = 0.9999566769464484;

// Gradient at a lattice corner (a sinless hash: the same on every GPU).
fn mp_sky_grad(i: vec2<f32>) -> vec2<f32> {
    var q = fract(i.xyx * vec3<f32>(0.1031, 0.1030, 0.0973));
    q = q + dot(q, q.yzx + 33.33);
    return fract((q.xx + q.yz) * q.zy) * 2.0 - 1.0;
}

// 2D gradient noise, about -1..1.
fn mp_sky_noise(x: vec2<f32>) -> f32 {
    let i = floor(x);
    let f = fract(x);
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let a = dot(mp_sky_grad(i), f);
    let b = dot(mp_sky_grad(i + vec2<f32>(1.0, 0.0)), f - vec2<f32>(1.0, 0.0));
    let c = dot(mp_sky_grad(i + vec2<f32>(0.0, 1.0)), f - vec2<f32>(0.0, 1.0));
    let d = dot(mp_sky_grad(i + vec2<f32>(1.0, 1.0)), f - vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y) * 1.6;
}

// Rounded puffs: soft balls, one at a jittered point in each lattice cell
// (radius by `k`), merged smoothly (a sum of Gaussians), so outlines bulge
// in round lobes like the tops of cumulus. Returns the field (about 0..1)
// and how far this point sits on the `light` side of the balls it is in
// (about -1..1; `light` is a 2D direction in the field's own frame): each
// ball shades like a sphere, bright on one side, grey on the other.
// `top` above 1 flattens each ball's crown, so a ball barely reaching the
// coverage threshold still forms a cloud of some size, not a tiny dot.
fn mp_puffs(x: vec2<f32>, k: f32, top: f32, light: vec2<f32>) -> vec2<f32> {
    let i = floor(x);
    let f = fract(x);
    var s = 0.0;
    var side = 0.0;
    for (var cy = -1; cy <= 1; cy = cy + 1) {
        for (var cx = -1; cx <= 1; cx = cx + 1) {
            let g = vec2<f32>(f32(cx), f32(cy));
            let h = mp_sky_grad(i + g + 17.0);
            let r = g + h * 0.35 + 0.5 - f;
            // Most cells hold a full ball or none, few a small one.
            let w = min(exp(-dot(r, r) * k) * top, 1.0) * smoothstep(-0.3, 0.15, h.x);
            s = s + w;
            side = side - w * dot(r, light);
        }
    }
    return vec2<f32>(min(s, 1.2), side / max(s, 1e-3) * sqrt(k));
}

// A field of fair-weather cumulus at `q` for the coverage threshold `thr`,
// in three octaves: rounded cloud bodies (puffs), a gradient noise that
// varies their size and merges neighbours into banks and, when `detail` >
// 0, small puffs that bulge the outlines into cauliflower lobes. The drift
// makes clouds billow, not scroll; the octaves are rotated against each
// other so lattices never line up. Returns the density (0..1, rising over a
// band past the threshold, so outlines feather instead of cutting), and
// how far this point sits on the `light` side of its body and of its lobe
// (about -1..1 each, for shading).
fn mp_cloud(q: vec2<f32>, drift: f32, thr: f32, detail: f32, light: vec2<f32>) -> vec4<f32> {
    let q2 = mat2x2<f32>(0.6, 0.8, -0.8, 0.6) * q + drift;
    let body = mp_puffs(q * 0.7, 7.0, 1.7, light);
    var v = body.x * 0.75 + mp_sky_noise(q2) * 0.25 + 0.36;
    var lobe = vec2<f32>(0.5, 0.0);
    if (detail > 0.0) {
        // Rotation and scale by 4.6; the light turns with the field. The
        // lobes mostly bite into the outline (no loose droplets) and bulge
        // most on the lit tops, leaving the bases smoother.
        let m = mat2x2<f32>(3.6, -2.9, 2.9, 3.6);
        lobe = mp_puffs(m * q - drift, 10.0, 1.0, normalize(m * light));
        let tops = clamp(0.6 + 0.6 * body.y, 0.2, 1.0);
        let bite = detail * 0.36 * (min(lobe.x, 1.0) - 0.65) * tops;
        v = v + min(bite, 0.0) + max(bite, 0.0) * smoothstep(thr + 0.02, thr + 0.14, v);
    }
    return vec4<f32>(smoothstep(thr, thr + 0.16, v), body.y, lobe.y * detail, v - thr);
}

fn mp_sky(dir_in: vec3<f32>) -> vec3<f32> {
    let r3 = kit_data[3];
    let r4 = kit_data[4];
    let r5 = kit_data[5];
    let r6 = kit_data[6];
    let sun = r3.xyz;
    let time = r3.w;
    let dir = normalize(dir_in);

    // Sun intensity by its height (three.js computes this per vertex).
    let zc = clamp(sun.y, -1.0, 1.0);
    let sun_e = 1000.0 * max(0.0, 1.0 - exp(-((1.6110731556870734 - acos(zc)) / 1.5)));
    // Extinction coefficients. (three.js's "sunfade" is 1 for a unit sun
    // position, so the rayleigh coefficient is the parameter as given.)
    let beta_r = MP_TOTAL_RAYLEIGH * r4.y;
    let beta_m = 0.434 * (0.2 * r4.x * 10e-18) * MP_MIE_CONST * r4.z;

    // Optical length (zenith angle capped at 90 degrees).
    let za = acos(max(0.0, dir.y));
    let inv = 1.0 / (cos(za) + 0.15 * pow(93.885 - za * 180.0 / MP_PI, -1.253));
    let fex = exp(-(beta_r * (8.4e3 * inv) + beta_m * (1.25e3 * inv)));

    // In-scattering.
    let cos_t = dot(dir, sun);
    let rc = cos_t * 0.5 + 0.5;
    let r_phase = 0.05968310365946075 * (1.0 + rc * rc);
    let g = r4.w;
    let m_phase = 0.07957747154594767 * (1.0 - g * g) / pow(1.0 - 2.0 * g * cos_t + g * g, 1.5);
    let ratio = (beta_r * r_phase + beta_m * m_phase) / (beta_r + beta_m);
    var lin = pow(sun_e * ratio * (vec3<f32>(1.0) - fex), vec3<f32>(1.5));
    lin = lin * mix(vec3<f32>(1.0), sqrt(sun_e * ratio * fex), clamp(pow(1.0 - sun.y, 5.0), 0.0, 1.0));
    let l0 = vec3<f32>(0.1) * fex;

    // Composition and the sun disc.
    let disc = clamp((cos_t - MP_SUN_COS) * 50000.0, 0.0, 1.0) * r6.z;
    let disc_col = (760.0 * disc) * min(sun_e * fex, vec3<f32>(80.0));
    let night = vec3<f32>(0.0, 0.0003, 0.00075);
    var col = (lin + l0) * 0.04 + disc_col + night;

    // Clouds on a curved layer above. A flat layer at zenith angle z lies
    // tan(z) away, so low clouds squash into thin pancakes and stretch to
    // streaks at the horizon. Here the layer lies z exp(c z^2) away: about
    // as far as the flat one up to 75 degrees from the zenith (the same
    // perspective shrink), but squashed only to about half height (puffy
    // heaps, as cumulus look), and at a finite reach on the horizon. The
    // clouds fade out below about 15 degrees into the haze.
    let coverage = r5.z;
    if (dir.y > 0.02 && coverage > 0.0) {
        let z = acos(clamp(dir.y, 0.0, 1.0));
        let c = mix(0.35, 0.65, r6.x);
        let reach = z * exp(c * z * z) / max(length(dir.xz), 1e-4);
        let horizon_fade = smoothstep(0.03, 0.27, dir.y);
        let q = dir.xz * reach * (r5.x * 1000.0) + time * r5.y * 1000.0;
        let evolve = time * r5.y * 300.0;
        // Large-scale coverage: clear gaps next to dense banks.
        let region = mp_sky_noise(q * 0.21 + 7.3) * 0.37 + 0.5;
        let cov = clamp(coverage + (region - 0.5) * 0.6, 0.0, 1.0);
        let thr = 1.0 - cov;
        // The puff detail fades out low down, where it would alias to streaks.
        let detail = smoothstep(0.1, 0.4, dir.y);
        // The side of each heap and lobe that catches the light: towards
        // the sun, and up (further out along the view is higher on screen).
        let to_sun = sun.xz / max(length(sun.xz), 1e-3);
        let out_dir = dir.xz / max(length(dir.xz), 1e-3);
        let lv = to_sun + out_dir * 0.8 + vec2<f32>(0.0, 1e-3);
        let light = lv / max(length(lv), 1e-4);
        let main = mp_cloud(q, evolve, thr, detail, light);
        let dens = main.x;
        // Lit by the sun through the air and by the sky's own light (mostly
        // grey: a cloud sees the whole sky and the sunlit ground, not just
        // the blue overhead).
        let day = smoothstep(-0.08, 0.3, sun.y);
        let sun_col = sun_e * fex * 0.22 * 0.04;
        let sky_amb = lin * 0.04 + night;
        let amb_grey = vec3<f32>(dot(sky_amb, vec3<f32>(0.2126, 0.7152, 0.0722)));
        let amb = mix(amb_grey, sky_amb, 0.3) * 1.5 + vec3<f32>(0.06, 0.06, 0.065);
        // A two-tap light march towards the sun through the layer: cloud
        // there shades this point (bright rims on the sun side, grey sides
        // away from it).
        let s1 = mp_cloud(q + to_sun * 0.2, evolve, thr, 0.0, light).x;
        let s2 = mp_cloud(q + to_sun * 0.55, evolve, thr, 0.0, light).x;
        let trans = exp(-(dens * 0.5 + s1 * 0.8 + s2 * 0.6) * 1.5);
        // Each heap and lobe shades like a ball (bright tops and sun sides,
        // grey bases between), and the thick middles are grey bellies when
        // looking straight up.
        let lit = clamp(0.5 + 0.4 * main.y + 0.3 * main.z * (1.0 - dir.y * dir.y), 0.0, 1.0);
        let belly = smoothstep(0.0, 0.45, main.w) * 0.6 * dir.y * dir.y;
        // Thin edges glow towards the sun (Henyey-Greenstein, g = 0.7).
        let silver = clamp(0.51 / pow(1.49 - cos_t * 1.4, 1.5) - 0.4, 0.0, 3.0);
        var cloud_col = amb * (0.8 + 0.3 * lit) * (1.0 - 0.3 * belly);
        cloud_col = cloud_col + sun_col * (0.12 + 0.95 * trans) * (0.15 + 0.95 * lit) * (1.0 - 0.6 * belly);
        cloud_col = cloud_col + sun_col * silver * (1.0 - dens) * dens * 1.2;
        cloud_col = cloud_col * max(day, 0.03);
        // Thin cloud is see-through (squared), so small wisps stay faint.
        let alpha = (1.0 - exp(-dens * dens * r5.w * 12.0)) * horizon_fade;
        col = col - l0 * 0.04 * alpha;
        let aerial = mix(col, cloud_col, fex);
        col = mix(col, aerial, alpha);
    }
    return max(col, vec3<f32>(0.0)) * r6.y;
}
