// evani.shader.recipes
// The recipe stage core (batch 1 build plan 2.3, package P2a). The patch in
// src/shader.rs appends this text, then every compiled recipe's rc_<name>
// function, then the generated rc_dispatch and mat_conditions, to the
// forward module, and wires three calls into the model lit stage:
// mat_frame (before the discard), mat_surface (after occ is set, before any
// lighting) and mat_extra (before fog). Everything here is written fresh for
// Evani.Dev; the noise and cell maths follow the well-known algorithms only.
//
// Deterministic: all noise comes from integer hashes, seeded by the block's
// seed byte; animation reads sim time from the fixed step (rc_frame), never a
// clock. No textures: a recipe is pure maths on the material's 32 bytes.

// The material's 32-byte block (catalog/materials, crate block.rs): bytes
// 0-3 recipe, variant, extras mask, condition mask; 4-12 colours A, B, C
// (sRGB8); 13-22 numbers n0-n9 (unorm over each number's range); 23 seed;
// 24-31 default condition amounts. Little-endian words. All zero = no recipe.
struct RecRaw {
    w0: vec4<u32>,
    w1: vec4<u32>,
};

// The shared per-frame recipe uniform, 128 bytes (shader.rs FRAME_BYTES).
// Zero is a safe default: recipes and conditions on, every quality group at
// lite, nothing denied.
//   cond_a, cond_b: the 8 global condition amounts (wet mud dust snow, wear
//     oxide growth blood);
//   misc.x: the 8 quality levels, 4 bits each (group g at bits 4g..4g+3:
//     detail fur water glass skin foliage conditions reflections; level
//     0 lite, 1 low, 2 medium, 3 high, 4 ultra);
//   misc.y: sim time in seconds (f32 bits, from the fixed step);
//   misc.z: off bits (1 = recipes off, 2 = conditions off, 4 = cut-out off),
//     in bits 8-15 the cut-out's material.recipe.cut.min_px times 85, in
//     16-23 the refraction strength (percent), in 24-31 cloth's sheen
//     strength (params.rs cloth_sheen_bits; 0 = as authored);
//   misc.w: paint's coat strength minus 1 (f32 bits; 0 = strength 1), when
//     paint is compiled (params.rs, material.recipe.paint.coat_strength);
//   deny: the global extras deny bits, bit k = extra k of shader.rs STAGE
//     order (RC_X_<name> + the recipe's own extras bit);
//   ka, kb, kc, kd: the condition layers' look numbers (conditions.rs LOOKS):
//     ka wet.puddles, wet.puddle_scale, mud.ground, mud.height;
//     kb dust.cavity, cos(snow.angle), snow.frost, wear.edge;
//     kc wear.scratches, oxide.strength, growth.scale, blood.age;
//     kd mud.color, dust.color, growth.color (sRGB8), reserved.
struct RecFrame {
    cond_a: vec4<f32>,
    cond_b: vec4<f32>,
    misc: vec4<u32>,
    deny: vec4<u32>,
    ka: vec4<f32>,
    kb: vec4<f32>,
    kc: vec4<f32>,
    kd: vec4<u32>,
};

@group(1) @binding(12) var<uniform> rc_mat: RecRaw;
@group(1) @binding(13) var<uniform> rc_frame: RecFrame;

// The block, unpacked. Colours are linear; numbers stay unorm (each recipe
// maps them over its own range with rc_range).
struct RecBlock {
    recipe: u32,
    variant: u32,
    extras: u32,
    cond_mask: u32,
    col_a: vec3<f32>,
    col_b: vec3<f32>,
    col_c: vec3<f32>,
    na: vec4<f32>,
    nb: vec4<f32>,
    nc: vec2<f32>,
    seed: u32,
    cond_a: vec4<f32>,
    cond_b: vec4<f32>,
};

// Screen-derivative data, taken once in uniform control flow (before the
// discard), as char_frame is. curv is signed curvature in 1/m: above 0 on
// convex edges, below 0 in creases.
struct MatFrame {
    dpx: vec3<f32>,
    dpy: vec3<f32>,
    duvx: vec2<f32>,
    duvy: vec2<f32>,
    uv_px: f32,
    world_px: f32,
    curv: f32,
    v: vec3<f32>,
    dist: f32,
    // How fast the geometric normal turns across the pixel (the sum of its
    // two screen derivatives' squares), for specular anti-aliasing.
    nv: f32,
    // The object-local position's two screen derivatives (round 9: equal to
    // dpx, dpy where the stage has no object-local position).
    dlx: vec3<f32>,
    dly: vec3<f32>,
};

// What the lit stage hands the recipe: its current colour (base, already
// tex x model colour x vertex colour, kept for recipe 0), the raw texture
// sample and vertex colour (a recipe builds its own colour from these, never
// from model_mat.base, so the fallback colour is never applied twice), the
// shading and geometric normals, world position, uv and the PBR inputs.
struct MatIn {
    base: vec4<f32>,
    tex: vec4<f32>,
    vcol: vec3<f32>,
    n: vec3<f32>,
    g: vec3<f32>,
    world: vec3<f32>,
    // The object-local position and normal (procedural realism round 9,
    // CORE_REQUESTS object-local-position): object-space patterns stay on a
    // moving, turning object. ln is zero where the stage has none (local is
    // then the world position).
    local: vec3<f32>,
    ln: vec3<f32>,
    uv: vec2<f32>,
    rough: f32,
    metal: f32,
    occ: f32,
};

// What the recipe gives back. base, rough, metal, n and occ replace the lit
// stage's values; emit, sheen and glint feed mat_extra after lighting:
//   emit: added as is (fire, glow, hologram);
//   sheen: rgb colour, w the rim power (0 = none; fur's rim);
//   glint: rgb sparkle already masked by the recipe, lit by the sun (flake,
//     gem, ice, mica).
struct MatOut {
    base: vec4<f32>,
    rough: f32,
    metal: f32,
    n: vec3<f32>,
    occ: f32,
    emit: vec3<f32>,
    sheen: vec4<f32>,
    glint: vec3<f32>,
};

// ---- block -------------------------------------------------------------

fn rc_lin(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

fn rc_unpack(r: RecRaw) -> RecBlock {
    let h = r.w0.x;
    let a = unpack4x8unorm(r.w0.y);
    let b = unpack4x8unorm(r.w0.z);
    let c = unpack4x8unorm(r.w0.w);
    let d = unpack4x8unorm(r.w1.x);
    let e = unpack4x8unorm(r.w1.y);
    var o: RecBlock;
    o.recipe = h & 0xffu;
    o.variant = (h >> 8u) & 0xffu;
    o.extras = (h >> 16u) & 0xffu;
    o.cond_mask = h >> 24u;
    o.col_a = rc_lin(a.xyz);
    o.col_b = rc_lin(vec3<f32>(a.w, b.x, b.y));
    o.col_c = rc_lin(vec3<f32>(b.z, b.w, c.x));
    o.na = vec4<f32>(c.y, c.z, c.w, d.x);
    o.nb = vec4<f32>(d.y, d.z, d.w, e.x);
    o.nc = e.yz;
    o.seed = r.w1.y >> 24u;
    o.cond_a = unpack4x8unorm(r.w1.z);
    o.cond_b = unpack4x8unorm(r.w1.w);
    return o;
}

fn rc_range(x: f32, lo: f32, hi: f32) -> f32 {
    return mix(lo, hi, x);
}

// The level (0 lite .. 4 ultra) of quality group g.
fn rc_q(g: u32) -> u32 {
    return min((rc_frame.misc.x >> (g * 4u)) & 15u, 4u);
}

fn rc_time() -> f32 {
    return bitcast<f32>(rc_frame.misc.y);
}

// The material uses its extras bit k (of the recipe whose first global bit
// is base) and the global switch for that extra is not off.
fn rc_extra(b: RecBlock, base: u32, k: u32) -> bool {
    let bit = base + k;
    let denied = (rc_frame.deny[bit >> 5u] >> (bit & 31u)) & 1u;
    return ((b.extras >> k) & 1u) == 1u && denied == 0u;
}

// ---- hash and noise -----------------------------------------------------

// A permuted congruential step: one multiply-add, then an output shuffle
// keyed by the state's top bits.
fn rc_pcg(v: u32) -> u32 {
    let s = v * 747796405u + 2891336453u;
    let w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (w >> 22u) ^ w;
}

fn rc_hash2(c: vec2<i32>, seed: u32) -> u32 {
    return rc_pcg(bitcast<u32>(c.y) ^ rc_pcg(bitcast<u32>(c.x) ^ rc_pcg(seed)));
}

fn rc_hash3(c: vec3<i32>, seed: u32) -> u32 {
    return rc_pcg(bitcast<u32>(c.z) ^ rc_hash2(c.xy, seed));
}

// 24 bits of h as [0, 1).
fn rc_u01(h: u32) -> f32 {
    return f32(h >> 8u) * (1.0 / 16777216.0);
}

fn rc_fade5(t: vec3<f32>) -> vec3<f32> {
    return t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
}

// Value noise in [0, 1], C2-smooth (quintic fade).
fn rc_vnoise2(p: vec2<f32>, seed: u32) -> f32 {
    let fl = floor(p);
    let i = vec2<i32>(fl);
    let u = rc_fade5(vec3<f32>(p - fl, 0.0)).xy;
    let a = rc_u01(rc_hash2(i, seed));
    let b = rc_u01(rc_hash2(i + vec2<i32>(1, 0), seed));
    let c = rc_u01(rc_hash2(i + vec2<i32>(0, 1), seed));
    let d = rc_u01(rc_hash2(i + vec2<i32>(1, 1), seed));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

fn rc_vnoise3(p: vec3<f32>, seed: u32) -> f32 {
    let fl = floor(p);
    let i = vec3<i32>(fl);
    let u = rc_fade5(p - fl);
    let x00 = mix(rc_u01(rc_hash3(i, seed)), rc_u01(rc_hash3(i + vec3<i32>(1, 0, 0), seed)), u.x);
    let x10 = mix(rc_u01(rc_hash3(i + vec3<i32>(0, 1, 0), seed)), rc_u01(rc_hash3(i + vec3<i32>(1, 1, 0), seed)), u.x);
    let x01 = mix(rc_u01(rc_hash3(i + vec3<i32>(0, 0, 1), seed)), rc_u01(rc_hash3(i + vec3<i32>(1, 0, 1), seed)), u.x);
    let x11 = mix(rc_u01(rc_hash3(i + vec3<i32>(0, 1, 1), seed)), rc_u01(rc_hash3(i + vec3<i32>(1, 1, 1), seed)), u.x);
    return mix(mix(x00, x10, u.y), mix(x01, x11, u.y), u.z);
}

// Fractal sum of value noise, octaves clamped to 1..6, normalised to [0, 1].
// Each octave turns the domain a little so the lattice never lines up.
fn rc_fbm2(p: vec2<f32>, seed: u32, octaves: u32) -> f32 {
    var q = p;
    var amp = 0.5;
    var sum = 0.0;
    var norm = 0.0;
    let n = clamp(octaves, 1u, 6u);
    for (var k = 0u; k < n; k = k + 1u) {
        sum = sum + rc_vnoise2(q, seed + k * 101u) * amp;
        norm = norm + amp;
        q = vec2<f32>(q.x * 1.6 - q.y * 1.2, q.x * 1.2 + q.y * 1.6);
        amp = amp * 0.5;
    }
    return sum / norm;
}

// Cellular (Worley) distances over a 3x3 cell search: x F1, y F2, z F2-F1,
// w the nearest cell's id in [0, 1). jitter 0 = a square grid of points,
// 1 = a point anywhere in its cell.
fn rc_voronoi2(p: vec2<f32>, seed: u32, jitter: f32) -> vec4<f32> {
    let fl = floor(p);
    let fr = p - fl;
    let ic = vec2<i32>(fl);
    var f1 = 8.0;
    var f2 = 8.0;
    var id = 0u;
    for (var y = -1; y <= 1; y = y + 1) {
        for (var x = -1; x <= 1; x = x + 1) {
            let off = vec2<i32>(x, y);
            let h = rc_hash2(ic + off, seed);
            let r = vec2<f32>(rc_u01(h), rc_u01(rc_pcg(h)));
            let pt = vec2<f32>(off) + vec2<f32>(0.5) + (r - vec2<f32>(0.5)) * jitter;
            let d = length(pt - fr);
            if (d < f1) {
                f2 = f1;
                f1 = d;
                id = h;
            } else if (d < f2) {
                f2 = d;
            }
        }
    }
    return vec4<f32>(f1, f2, f2 - f1, rc_u01(id));
}

// Hex cells of unit spacing (pointy-top): xy the offset from the cell's
// centre, zw the centre itself (a stable cell id: hash floor(zw * 2.0)).
// The nearest centre of two offset rectangular lattices wins.
fn rc_hex(p: vec2<f32>) -> vec4<f32> {
    let s = vec2<f32>(1.0, 1.7320508);
    let h = s * 0.5;
    let ca = (floor(p / s) + vec2<f32>(0.5)) * s;
    let cb = (floor((p - h) / s) + vec2<f32>(0.5)) * s + h;
    let a = p - ca;
    let b = p - cb;
    if (dot(a, a) < dot(b, b)) {
        return vec4<f32>(a, ca);
    }
    return vec4<f32>(b, cb);
}

// ---- anti-aliasing and pattern space -------------------------------------

// How much of a pattern to keep for cells of size `cell` (uv units): 1 while
// a cell spans 1.5 pixels or more, fading to 0 (draw the pattern's average)
// by 0.75 pixels, so fine detail never shimmers.
fn rc_aa(f: MatFrame, cell: f32) -> f32 {
    return smoothstep(0.75, 1.5, cell / max(f.uv_px, 1e-7));
}

// The same for world-space patterns (cell size in metres).
fn rc_aa_world(f: MatFrame, cell: f32) -> f32 {
    return smoothstep(0.75, 1.5, cell / max(f.world_px, 1e-7));
}

fn rc_fade(v: f32, average: f32, keep: f32) -> f32 {
    return mix(average, v, keep);
}

// World-space pattern coordinates on the plane facing the dominant axis of
// n (Lite to Medium for ground, stone and masonry).
fn rc_proj(p: vec3<f32>, n: vec3<f32>) -> vec2<f32> {
    let a = abs(n);
    if (a.y >= a.x && a.y >= a.z) {
        return p.xz;
    }
    if (a.x >= a.z) {
        return p.zy;
    }
    return p.xy;
}

// The two planes nearest n and the weight of the second (High and Ultra
// blend two lookups, so a slope never shows a hard seam).
struct RecProj {
    a: vec2<f32>,
    b: vec2<f32>,
    w: f32,
};

fn rc_proj2(p: vec3<f32>, n: vec3<f32>) -> RecProj {
    let a = abs(n);
    var o: RecProj;
    if (a.y >= a.x && a.y >= a.z) {
        o.a = p.xz;
        o.b = select(p.xy, p.zy, a.x >= a.z);
        o.w = max(a.x, a.z) / (a.y + max(a.x, a.z) + 1e-6);
    } else if (a.x >= a.z) {
        o.a = p.zy;
        o.b = select(p.xy, p.xz, a.y >= a.z);
        o.w = max(a.y, a.z) / (a.x + max(a.y, a.z) + 1e-6);
    } else {
        o.a = p.xy;
        o.b = select(p.zy, p.xz, a.y >= a.x);
        o.w = max(a.x, a.y) / (a.z + max(a.x, a.y) + 1e-6);
    }
    return o;
}

// ---- the three calls the patch wires in ----------------------------------

fn mat_frame(world: vec3<f32>, local: vec3<f32>, uv: vec2<f32>, ng: vec3<f32>) -> MatFrame {
    var f: MatFrame;
    f.dpx = dpdx(world);
    f.dpy = dpdy(world);
    f.dlx = dpdx(local);
    f.dly = dpdy(local);
    f.duvx = dpdx(uv);
    f.duvy = dpdy(uv);
    let dnx = dpdx(ng);
    let dny = dpdy(ng);
    f.uv_px = max(length(f.duvx), length(f.duvy));
    f.world_px = max(length(f.dpx), length(f.dpy));
    f.curv = (dot(dnx, f.dpx) + dot(dny, f.dpy)) / max(dot(f.dpx, f.dpx) + dot(f.dpy, f.dpy), 1e-12);
    f.nv = dot(dnx, dnx) + dot(dny, dny);
    let d = frame.eye.xyz - world;
    f.dist = length(d);
    f.v = d / max(f.dist, 1e-4);
    return f;
}

// An object-space vector t lying in the surface, in world space: t as a
// sum of the pixel's two object-space screen steps, the same sum of its two
// world-space steps (the instance's rotation and scale, no matrix needed;
// round 9, for normals perturbed by an object-space height).
fn mat_l2w(f: MatFrame, t: vec3<f32>) -> vec3<f32> {
    let kx = inverseSqrt(max(dot(f.dlx, f.dlx), 1e-30));
    let ky = inverseSqrt(max(dot(f.dly, f.dly), 1e-30));
    let a = f.dlx * kx;
    let b = f.dly * ky;
    let ab = dot(a, b);
    let r = vec2<f32>(dot(t, a), dot(t, b));
    let s = vec2<f32>(r.x - ab * r.y, r.y - ab * r.x) / max(1.0 - ab * ab, 1e-4);
    return f.dpx * (kx * s.x) + f.dpy * (ky * s.y);
}

// The lit stage's own values, untouched: what recipe 0 returns.
fn mat_identity(i: MatIn) -> MatOut {
    return MatOut(i.base, i.rough, i.metal, i.n, i.occ, vec3<f32>(0.0), vec4<f32>(0.0), vec3<f32>(0.0));
}

// The global amount of condition layer k (0 wet .. 7 blood).
fn rc_global_amount(k: u32) -> f32 {
    if (k < 4u) {
        return rc_frame.cond_a[k];
    }
    return rc_frame.cond_b[k - 4u];
}

// A layer's final amount: the larger of the material's default and the
// global one, and only where the material's condition mask has the layer.
fn rc_amount(b: RecBlock, k: u32) -> f32 {
    if (((b.cond_mask >> k) & 1u) == 0u) {
        return 0.0;
    }
    let own = select(b.cond_b[k & 3u], b.cond_a[k & 3u], k < 4u);
    return max(own, rc_global_amount(k));
}

// The recipe, then the condition layers, on this pixel's inputs.
fn mat_surface(f: MatFrame, i: MatIn) -> MatOut {
    let b = rc_unpack(rc_mat);
    let o = mat_identity(i);
    if (b.recipe == 0u || (rc_frame.misc.z & 1u) != 0u) {
        return o;
    }
    return mat_conditions(b, f, i, rc_dispatch(b, f, i, o));
}

// Specular anti-aliasing (procedural realism round 8): the recipe's
// roughness widened by how fast the normal turns inside the pixel, so a
// rounded edge or a tight curve shows a soft sheen, never a pinpoint glint
// or a sparkle. The lit stage takes the recipe's roughness through it. The
// kernel-variance idea of Kaplanyan et al. as Google Filament describes it
// (Apache-2.0; formula only, written fresh).
fn mat_saa(f: MatFrame, rough: f32) -> f32 {
    let a = rough * rough;
    return sqrt(sqrt(min(a * a + min(0.5 * f.nv, 0.18), 1.0)));
}

// The post-light term: emission, the rim sheen and sun-lit glints, added to
// the lit colour c0; then rc_post (generated: paint's clear coat when paint is
// compiled, else c unchanged), which may also dim what lies under it. g is the
// geometric normal (a coat lies over the normal map and the recipe's normals).
fn mat_extra(c0: vec3<f32>, o: MatOut, n: vec3<f32>, g: vec3<f32>, v: vec3<f32>, l: vec3<f32>, sun: vec3<f32>, vis: f32) -> vec3<f32> {
    var c = o.emit;
    let ndl = clamp(dot(n, l), 0.0, 1.0);
    if (o.sheen.w > 0.0) {
        let rim = pow(1.0 - clamp(dot(n, v), 0.0, 1.0), o.sheen.w);
        c = c + o.sheen.rgb * sun * (rim * mix(0.2, 1.0, ndl * vis));
    }
    c = c + o.glint * sun * (vis * step(1e-4, ndl));
    return rc_post(c0 + c, o, g, v, l, sun, vis);
}
