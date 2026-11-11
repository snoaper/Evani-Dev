// evani-grass: the structs both modules share and the integer-hash noise.
// The CPU packs `Sim` word for word (src/plan.rs, `SIM_WORDS`); a native
// test checks the layout with naga.

struct Kind {
    // height (m), height variation, root width (m), density (blades / m2)
    shape: vec4<f32>,
    // lean (rad), curl (rad), stiffness, share of blades lying flat
    bend: vec4<f32>,
    // root colour (linear rgb), colour variation
    root: vec4<f32>,
    // tip colour, dryness
    tip: vec4<f32>,
    // dry colour, clumping
    dry: vec4<f32>,
    // ear length (share of the stalk, 0 = a plain blade), ear width (m),
    // bare patches, reach (share of the tier's distance it is drawn to)
    ear: vec4<f32>,
    // soil colour between the blades, how muddy it is without rain
    soil: vec4<f32>,
    // share of stalks with a seed head, droop of long leaves, how far the
    // sward grows in tussocks (0: an even sward), share of the weed grid
    // carrying a broadleaf rosette (0: none)
    more: vec4<f32>,
    // how fine the sward is (thin pointed blades that twist, over matted
    // fine undergrowth; 0: the plain blade and ground), patches of deeper
    // green, olive and straw (0: an even colour), how much lighter and paler
    // it turns seen at a low angle (0: none), unused
    look: vec4<f32>,
};

struct Patch {
    // min x, min z, max x, max z
    rect: vec4<f32>,
    // ground height, edge feather (m), kind index, density scale
    info: vec4<f32>,
    // height grid: first word in `ground`, samples a side (0: flat),
    // its top over the ground height, unused
    hf: vec4<f32>,
};

struct Sim {
    view_proj: mat4x4<f32>,
    // Culling planes (xyz normal, w offset): a point is inside when
    // dot(n, p) + w >= 0 for all six.
    planes: array<vec4<f32>, 6>,
    // xyz the eye LOD is measured from, w time (s)
    eye: vec4<f32>,
    // wind heading (x, z), steady strength, gust strength
    wind: vec4<f32>,
    // gust speed (m/s), gust size (m), flutter, sway frequency (Hz)
    wind2: vec4<f32>,
    // full-density distance, end of the middle LOD, max distance, tile size
    lod: vec4<f32>,
    // end of the near LOD, width boost cap, wetness, wet droop (rad)
    lod2: vec4<f32>,
    // trample window: origin cell x, origin cell z, cell size (m), cells
    trample: vec4<f32>,
    // last frame's origin cell x, z, relax factor this frame, unused
    trample2: vec4<f32>,
    // stamps, unused, frustum culling on (1), unused
    stamps_n: vec4<f32>,
    // bucket capacities (near, middle, far), jobs this frame
    caps: vec4<u32>,
    // bucket first blades, unused
    bases: vec4<u32>,
    // per bucket: the share of the full density kept to fit its cap
    scales: vec4<f32>,
    // per stamp: (from x, from z, to x, to z), (radius, strength, motion x, motion z)
    stamps: array<vec4<f32>, 32>,
    kinds: array<Kind, 4>,
    patches: array<Patch, 16>,
};

fn hash_u(x: u32) -> u32 {
    let s = x * 747796405u + 2891336453u;
    let w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (w >> 22u) ^ w;
}

fn unit_f(h: u32) -> f32 {
    return f32(h >> 8u) * (1.0 / 16777216.0);
}

fn rnd(s: ptr<function, u32>) -> f32 {
    *s = hash_u(*s);
    return unit_f(*s);
}

fn lattice(i: vec2<i32>) -> f32 {
    return unit_f(hash_u((u32(i.x) * 1664525u) ^ hash_u(u32(i.y) + 1013904223u)));
}

// Value noise in 0..1, smooth.
fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = p - i;
    let u = f * f * (3.0 - 2.0 * f);
    let c = vec2<i32>(i);
    let a = lattice(c);
    let b = lattice(c + vec2<i32>(1, 0));
    let d = lattice(c + vec2<i32>(0, 1));
    let e = lattice(c + vec2<i32>(1, 1));
    return mix(mix(a, b, u.x), mix(d, e, u.x), u.y);
}

// Share of the full density kept at distance `d` (same rule as the CPU's
// `plan::keep_at`): all of it within the full-density distance, then
// falling with the square of the distance, so the blades per pixel stay
// about even; none past the max distance.
fn keep_at(d: f32, full: f32, far: f32) -> f32 {
    if (d >= far) {
        return 0.0;
    }
    if (d <= full) {
        return 1.0;
    }
    let r = full / d;
    return r * r;
}

// How much of a patch's ground the blades cover at `xz` (bare patches in
// muddy ground; 1 where the kind has none).
fn cover_at(xz: vec2<f32>, bare: f32) -> f32 {
    if (bare <= 0.001) {
        return 1.0;
    }
    let n = vnoise(xz * 0.45 + vec2<f32>(13.1, 7.7)) * 0.65 + vnoise(xz * 1.7 + vec2<f32>(2.0, 5.0)) * 0.35;
    return smoothstep(bare * 0.85, bare * 0.85 + 0.2, n);
}

// Dry, sun-bleached patches (0..1 before the kind's dryness).
fn dry_at(xz: vec2<f32>) -> f32 {
    return smoothstep(0.42, 0.78, vnoise(xz * 0.16 + vec2<f32>(91.0, 3.0)));
}

// Patches of other growth across a sward (a kind's `mottle`): x a slow drift
// a few metres across and, far off, tens of metres (0..1), y clumps of
// deeper, coarser growth, most about a metre across and a few several metres
// (0..1, mostly 0), z dry patches of olive and straw (0..1, mostly 0). `px`
// is the ground one pixel covers (m, the longer way; 0 for a blade): each
// size of patch fades to its mean before it is finer than the pixels, so far
// off, where a pixel is metres deep, the sward keeps only its broad drift
// and never streaks. The blades (compute) and the carpet (draw) read the
// same field, so they agree.
fn mottle_px(xz: vec2<f32>, px: f32) -> vec3<f32> {
    let f1 = 1.0 - smoothstep(1.2, 5.0, px);
    let f2 = 1.0 - smoothstep(0.12, 0.5, px);
    let f3 = 1.0 - smoothstep(0.25, 1.0, px);
    let f4 = 1.0 - smoothstep(0.9, 3.5, px);
    var big = 0.5;
    if (f1 > 0.0) {
        big = mix(0.5, vnoise(xz * 0.071 + vec2<f32>(17.0, 41.0)) * 0.6 + vnoise(xz * 0.23 + vec2<f32>(5.0, 63.0)) * 0.4, f1);
    }
    big += (vnoise(xz * 0.019 + vec2<f32>(7.0, 29.0)) * 0.6 + vnoise(xz * 0.0063 + vec2<f32>(43.0, 3.0)) * 0.4 - 0.5) * 0.7 * (1.0 - smoothstep(14.0, 60.0, px));
    var yc = 0.1;
    if (f2 > 0.0) {
        yc = mix(0.1, smoothstep(0.5, 0.8, vnoise(xz * 0.83 + vec2<f32>(71.0, 9.0)) * 0.62 + vnoise(xz * 2.1 + vec2<f32>(3.0, 27.0)) * 0.38), f2);
    }
    var yb = 0.06;
    if (f4 > 0.0) {
        yb = mix(0.06, 0.7 * smoothstep(0.52, 0.76, vnoise(xz * 0.13 + vec2<f32>(53.0, 21.0)) * 0.6 + vnoise(xz * 0.31 + vec2<f32>(9.0, 77.0)) * 0.4), f4);
    }
    var z = 0.12;
    if (f3 > 0.0) {
        z = mix(0.12, smoothstep(0.52, 0.82, vnoise(xz * 0.37 + vec2<f32>(29.0, 83.0)) * 0.55 + vnoise(xz * 1.3 + vec2<f32>(47.0, 13.0)) * 0.45), f3);
    }
    return vec3<f32>(clamp(big, 0.0, 1.0), max(yc, yb), z);
}

fn mottle_at(xz: vec2<f32>) -> vec3<f32> {
    return mottle_px(xz, 0.0);
}

// A sward colour `c` under its patches (`m` from mottle_at, `k` the kind's
// mottle, `dryc` its dry colour): drifting between yellower and greener,
// deeper in the clumps, a little towards straw in the dry patches (there
// more of the leaves themselves are dead: the blades and the pile count
// them, so a dry patch is straw among green, not a tint).
fn mottled(c: vec3<f32>, dryc: vec3<f32>, m: vec3<f32>, k: f32) -> vec3<f32> {
    var o = c * mix(vec3<f32>(1.0), mix(vec3<f32>(1.13, 1.05, 0.86), vec3<f32>(0.86, 0.96, 1.04), smoothstep(0.25, 0.75, m.x)), k);
    o = mix(o, o * vec3<f32>(0.6, 0.72, 0.64), m.y * k * 0.75);
    return mix(o, dryc * 0.85, m.z * k * 0.2);
}

// ---- a fine sward's ground plan (a kind's `fine`) ---------------------------
// Where the soil lies open in a fine sward (its tufts are further down, with
// the tussocks). The blades (compute) and the carpet (draw) read the same
// fields, so they agree.

// The plane bent a little, so a field read through it does not line up with
// the noise's grid.
fn bent(xz: vec2<f32>) -> vec2<f32> {
    return xz + 0.11 * vec2<f32>(sin(xz.y * 5.7 + 1.3), sin(xz.x * 6.3 + 4.1)) + 0.045 * vec2<f32>(sin(xz.y * 17.0 + xz.x * 3.1), sin(xz.x * 15.0 - xz.y * 4.3));
}

// Open soil in a fine sward (the kind's `bare`, `b`): the reading that is
// open above open_th(b). Openings are irregular, a hand to a stride
// across, some run together, and their edges are ragged (the sward breaks
// up into them a finger at a time).
fn open_n(xz: vec2<f32>) -> f32 {
    let q = bent(xz * 0.4) * 2.5;
    return vnoise(q * 1.2 + vec2<f32>(19.0, 71.0)) * 0.6 + vnoise(q * 3.3 + vec2<f32>(37.0, 9.0)) * 0.4 + (vnoise(xz * 12.0 + vec2<f32>(5.0, 31.0)) - 0.5) * 0.085 + (vnoise(xz * 33.0 + vec2<f32>(23.0, 2.0)) - 0.5) * 0.04;
}

fn open_th(b: f32) -> f32 {
    return 0.86 - 1.3 * b;
}

// 0 under the sward .. 1 on open soil.
fn open_at(xz: vec2<f32>, b: f32) -> f32 {
    if (b <= 0.001) {
        return 0.0;
    }
    let th = open_th(b);
    return smoothstep(th, th + 0.045, open_n(xz));
}

// x: 0 under the sward .. 1 on open soil; y: 0 deep in the sward .. 1 at an
// opening's rim and on it (the sward thins and lies over towards an
// opening over about a hand's width).
fn open_rim(xz: vec2<f32>, b: f32) -> vec2<f32> {
    if (b <= 0.001) {
        return vec2<f32>(0.0);
    }
    let th = open_th(b);
    let n = open_n(xz);
    return vec2<f32>(smoothstep(th, th + 0.045, n), smoothstep(th - 0.1, th + 0.01, n));
}

// Tussocks (a kind's `tuft`): a grazed sward grows in tufts, one to a cell
// of a jittered grid. The tuft nearest `xz`: the offset from `xz` to its
// heart (xy, m) and its size (z, 0.35..1). The blades (compute) and their
// shading (draw) read the same field, so they agree.
const TUFT_CELL: f32 = 0.34;

// The tuft of grid cell `q`: the offset from `xz` to its heart (xy, m) and
// its size (z, 0.35..1).
fn tuft_cell(q: vec2<i32>, xz: vec2<f32>) -> vec3<f32> {
    let h = hash_u((u32(q.x) * 1664525u) ^ hash_u(u32(q.y) + 974711u));
    let h2 = hash_u(h);
    let o = (vec2<f32>(q) + vec2<f32>(unit_f(h), unit_f(h2))) * TUFT_CELL - xz;
    return vec3<f32>(o, 0.35 + 0.65 * unit_f(hash_u(h2)));
}

fn tuft_at(xz: vec2<f32>) -> vec3<f32> {
    let c = vec2<i32>(floor(xz / TUFT_CELL));
    var best = vec3<f32>(0.0);
    var bd = 1e9;
    for (var j = -1; j <= 1; j += 1) {
        for (var i = -1; i <= 1; i += 1) {
            let t = tuft_cell(c + vec2<i32>(i, j), xz);
            let d = dot(t.xy, t.xy);
            if (d < bd) {
                bd = d;
                best = t;
            }
        }
    }
    return best;
}

// A tuft's radius on the ground (m) from its size.
fn tuft_radius(size: f32) -> f32 {
    return TUFT_CELL * (0.35 + 0.5 * size);
}

// A fine sward's tuft of grid cell `q` (a kind's `fine` with `tuft`): as
// [`tuft_cell`], but of every size (z, 0.3..1.2, most of them small; the big
// ones run into their neighbours) and not one to every cell (z 0: none
// here): the sward is grazed even in places, a tuft to one cell in twelve,
// and tussocky in others, changing over a few metres.
fn ftuft_cell(q: vec2<i32>, xz: vec2<f32>) -> vec3<f32> {
    let t = tuft_cell(q, xz);
    let h = hash_u((u32(q.x) * 2246822519u) ^ hash_u(u32(q.y) + 668265263u));
    let many = smoothstep(0.2, 0.75, vnoise(vec2<f32>(q) * 0.11 + vec2<f32>(61.0, 23.0)));
    if (unit_f(h) >= 0.08 + 0.62 * many * many) {
        return vec3<f32>(t.xy, 0.0);
    }
    let u = unit_f(hash_u(h));
    return vec3<f32>(t.xy, 0.3 + (0.3 + 0.6 * many) * u * u);
}

// How much of a tuft of size `size` stands at the offset `o` from its heart
// (0 outside it .. 1 at its heart).
fn tuft_prof(o: vec2<f32>, size: f32) -> f32 {
    let rad = tuft_radius(size);
    return 1.0 - smoothstep(rad * 0.15, rad, length(o));
}

// The tuft of a fine sward that stands highest at `xz`: the offset from `xz`
// to its heart (xy, m), its size (z; 0: none reaches here) and how much of
// it stands here (w: its size at its heart, 0 at its rim).
fn ftuft_at(xz: vec2<f32>) -> vec4<f32> {
    let c = vec2<i32>(floor(xz / TUFT_CELL));
    var best = vec4<f32>(0.0);
    for (var j = -1; j <= 1; j += 1) {
        for (var i = -1; i <= 1; i += 1) {
            let t = ftuft_cell(c + vec2<i32>(i, j), xz);
            let p = t.z * tuft_prof(t.xy, max(t.z, 0.3));
            if (p > best.w) {
                best = vec4<f32>(t, p);
            }
        }
    }
    return best;
}

// A fine sward's height at a tuft reading `p` (ftuft_at's w) against the
// kind's own (`k` the kind's `tuft`): grazed lower between the tufts, up to
// over twice the kind's height in a big tuft's heart.
fn tuft_height(p: f32, k: f32) -> f32 {
    return mix(1.0, 0.7 + 1.45 * p, k);
}

// A fine sward lies in sprays: the blades of a hand's breadth of ground
// lean one way (a unit vector on the ground), the next spray another.
fn spray_dir(xz: vec2<f32>) -> vec2<f32> {
    let a = (vnoise(xz * 1.9 + vec2<f32>(41.0, 7.0)) * 2.0 + vnoise(xz * 5.3 + vec2<f32>(3.0, 59.0))) * 4.2;
    return vec2<f32>(sin(a), cos(a));
}

// How rank a fine sward grows (octave `o` of six, each four times the size
// of the one before: a hand's breadth, a stride, then patches of 4, 15, 60
// and 240 m): 0 grazed low and pale .. 1 rank, taller and deeper green. `p`
// is the ground point times [`RANK_F`] / 4^o. The blades take their height
// from the two smallest; the far sward takes its light and colour from them
// all ([`sward_far`], draw).
const RANK_F: f32 = 4.3;

fn rank_oct(p: vec2<f32>, o: i32) -> f32 {
    return vnoise(p + vec2<f32>(f32(o) * 17.3 + 11.0, f32(o) * 5.1 + 47.0));
}

// Broadleaf weeds (a kind's `weeds`): rosettes of broad leaves, at most one
// to a plot of a jittered grid. They grow in clusters: `share` is the share
// of plots carrying one across the field, a few times that in a cluster
// (a block of plots) and next to none between clusters. The rosette `xz`
// lies in: the offset from `xz` to its heart (xy, m), its radius (z, m; 0:
// no rosette here) and its own random number (w).
const WEED_CELL: f32 = 0.55;

// The rosette of plot `q`, if it carries one: the offset from `xz` to its
// heart (xy, m), its radius (z, m; 0: none) and its own random number (w).
fn weed_cell(q: vec2<i32>, xz: vec2<f32>, share: f32) -> vec4<f32> {
    let h = hash_u((u32(q.x) * 22695477u) ^ hash_u(u32(q.y) + 40503u));
    // (Its cluster: a block of 4 x 4 plots, most blocks nearly empty, a few
    // crowded.)
    let cu = unit_f(hash_u((u32(q.x >> 2u) * 1597334677u) ^ hash_u(u32(q.y >> 2u) + 3812015801u)));
    if (unit_f(h) >= share * (0.05 + 2.6 * cu * cu * cu)) {
        return vec4<f32>(0.0);
    }
    let h2 = hash_u(h + 1u);
    let h3 = hash_u(h2);
    let h4 = hash_u(h3);
    let o = (vec2<f32>(q) + vec2<f32>(unit_f(h2), unit_f(h3))) * WEED_CELL - xz;
    let ru = unit_f(h4);
    return vec4<f32>(o, 0.07 + 0.26 * ru * ru, unit_f(hash_u(h4)));
}

fn weed_at(xz: vec2<f32>, share: f32) -> vec4<f32> {
    let c = vec2<i32>(floor(xz / WEED_CELL));
    var best = vec4<f32>(0.0);
    var bd = 1e9;
    for (var j = -1; j <= 1; j += 1) {
        for (var i = -1; i <= 1; i += 1) {
            let wc = weed_cell(c + vec2<i32>(i, j), xz, share);
            let d = length(wc.xy);
            if (d < wc.z && d < bd) {
                bd = d;
                best = wc;
            }
        }
    }
    return best;
}

fn unpack_rgb(v: u32) -> vec3<f32> {
    let c = vec3<f32>(f32(v & 255u), f32((v >> 8u) & 255u), f32((v >> 16u) & 255u)) / 255.0;
    return c * c;
}
