// ---- relief (feature `relief`) --------------------------------------------------
// Baked brick, cobble, tile and plank relief (src/relief): colour, slope,
// height and roughness tiles in kit_data from record MP_RELIEF, sampled with
// a footprint-chosen mip, turned into a bent normal and a roughness for the
// kit-surface hook, with wet, puddle and moss layers on top.
//
// Block (records from MP_RELIEF):
//   0: [magic, surfaces, tier mode (0 flat, 1 colour, 2 full), height strength]
//   1: [wetness, puddles, moss, puddle scale (m)]
//   2: [moss colour rgb, moss scale (m)]
//   3: [free surface (or -1), 0, 0, 0]
//   4 + 4k: [first float (from the block), size, levels, slope scale]
//           [tile u, tile v (m), depth (m), 0]
//           [mean colour rgb, mean roughness]
//           [colour multiplier rgb, 0]
// Each texel is two exact-integer floats (src/relief/bake.rs): colour
// (sqrt, 8 bits each) and slope u, slope v (7 bits, 63 flat), height
// (6 bits), roughness (4 bits).

const RL_MAGIC: f32 = 7177.0;
const RL_FIRST: u32 = 32u;

struct RlTex {
    c: vec3<f32>,
    s: vec2<f32>,
    h: f32,
    r: f32,
};

fn rl_on() -> bool {
    return arrayLength(&kit_data) > MP_RELIEF + 4u && kit_data[MP_RELIEF].x == RL_MAGIC;
}

fn rl_decode(e: vec2<f32>, scale: f32) -> RlTex {
    let a = u32(e.x);
    let b = u32(e.y);
    let c = vec3<f32>(f32(a & 255u), f32((a >> 8u) & 255u), f32((a >> 16u) & 255u)) * (1.0 / 255.0);
    var t: RlTex;
    t.c = c * c;
    t.s = (vec2<f32>(f32(b & 127u), f32((b >> 7u) & 127u)) - 63.0) * (scale / 63.0);
    t.h = f32((b >> 14u) & 63u) * (1.0 / 63.0);
    t.r = f32((b >> 20u) & 15u) * (1.0 / 15.0);
    return t;
}

fn rl_mixt(a: RlTex, b: RlTex, w: f32) -> RlTex {
    var t: RlTex;
    t.c = mix(a.c, b.c, w);
    t.s = mix(a.s, b.s, w);
    t.h = mix(a.h, b.h, w);
    t.r = mix(a.r, b.r, w);
    return t;
}

// Texel (i, j), wrapped, of an m x m level whose first float is f0.
fn rl_fetch(f0: u32, m: u32, i: i32, j: i32, scale: f32) -> RlTex {
    let mi = i32(m);
    let ii = u32(((i % mi) + mi) % mi);
    let jj = u32(((j % mi) + mi) % mi);
    let f = f0 + 2u * (jj * m + ii);
    let r = kit_data[f >> 2u];
    return rl_decode(select(r.xy, r.zw, (f & 2u) != 0u), scale);
}

fn rl_bilinear(f0: u32, m: u32, uv01: vec2<f32>, scale: f32) -> RlTex {
    let x = uv01 * f32(m) - 0.5;
    let fl = floor(x);
    let w = x - fl;
    let i = vec2<i32>(fl);
    let a = rl_fetch(f0, m, i.x, i.y, scale);
    let b = rl_fetch(f0, m, i.x + 1, i.y, scale);
    let c = rl_fetch(f0, m, i.x, i.y + 1, scale);
    let d = rl_fetch(f0, m, i.x + 1, i.y + 1, scale);
    return rl_mixt(rl_mixt(a, b, w.x), rl_mixt(c, d, w.x), w.y);
}

// Texels in the levels before level l of an n tile: (n^2 - m^2) * 4 / 3.
fn rl_off(n: u32, l: u32) -> u32 {
    let m = n >> l;
    return ((n * n - m * m) * 4u) / 3u;
}

// Surface k at pattern coordinates uv (m), pixel footprint fw (m): the two
// mips around the footprint, blended; past the last mip, the mean.
fn rl_sample(k: u32, uv: vec2<f32>, fw: f32) -> RlTex {
    let o = MP_RELIEF + 4u + 4u * k;
    let r0 = kit_data[o];
    let r1 = kit_data[o + 1u];
    let r2 = kit_data[o + 2u];
    let n = u32(r0.y);
    let levels = max(u32(r0.z), 1u);
    let tile = max(r1.xy, vec2<f32>(1e-3));
    let uv01 = fract(uv / tile);
    let texm = min(tile.x, tile.y) / f32(max(n, 1u));
    let lod = log2(max(fw, 1e-6) / texm);
    let last = f32(levels - 1u);
    let lf = clamp(lod, 0.0, last);
    let l0 = u32(floor(lf));
    let l1 = min(l0 + 1u, levels - 1u);
    let base = 4u * MP_RELIEF + u32(r0.x);
    let t0 = rl_bilinear(base + 2u * rl_off(n, l0), n >> l0, uv01, r0.w);
    let t1 = rl_bilinear(base + 2u * rl_off(n, l1), n >> l1, uv01, r0.w);
    let t = rl_mixt(t0, t1, lf - floor(lf));
    var mean: RlTex;
    mean.c = r2.rgb;
    mean.s = vec2<f32>(0.0);
    mean.h = 0.5;
    mean.r = r2.w;
    return rl_mixt(t, mean, clamp(lod - last, 0.0, 1.0));
}

fn rl_axis(a: u32) -> vec3<f32> {
    if (a == 0u) {
        return vec3<f32>(1.0, 0.0, 0.0);
    }
    if (a == 1u) {
        return vec3<f32>(0.0, 1.0, 0.0);
    }
    return vec3<f32>(0.0, 0.0, 1.0);
}

fn rl_rot(q: vec4<f32>, v: vec3<f32>) -> vec3<f32> {
    let t = 2.0 * cross(q.xyz, v);
    return v + q.w * t + cross(q.xyz, t);
}

// 2D coordinates on the plane facing n (for the layers' noise).
fn rl_plane(p: vec3<f32>, n: vec3<f32>) -> vec2<f32> {
    let a = abs(n);
    if (a.y >= a.x && a.y >= a.z) {
        return p.xz;
    }
    if (a.x >= a.z) {
        return p.zy;
    }
    return p.xy;
}

// The surface's flat colour (the Lite tier, and the shadow / fallback path).
fn rl_flat(k: u32) -> vec3<f32> {
    let o = MP_RELIEF + 4u + 4u * k;
    return kit_data[o + 2u].rgb * kit_data[o + 3u].rgb;
}

// Colour, bent normal and roughness of relief surface k at p, from the
// sample t and its world-space slope `grad` (m per m along the face).
fn rl_shade(k: u32, t: RlTex, p: vec3<f32>, n: vec3<f32>, grad: vec3<f32>, nr: ptr<function, vec4<f32>>) -> vec3<f32> {
    let h0 = kit_data[MP_RELIEF];
    let mode = u32(h0.z);
    if (mode == 0u) {
        return rl_flat(k);
    }
    let lay = kit_data[MP_RELIEF + 1u];
    let mos = kit_data[MP_RELIEF + 2u];
    let mul = kit_data[MP_RELIEF + 4u + 4u * k + 3u].rgb;
    let q = rl_plane(p, n);
    // A slow world-space tint, so the tile's repeat doesn't read.
    let wv = mp_fbm(q * 0.45 + vec2<f32>(p.y * 0.3, 0.0), 311u);
    var c = t.c * mul * (0.86 + 0.28 * wv);
    var g = grad * h0.w;
    var rough = t.r;
    let up = clamp(n.y, 0.0, 1.0);
    // Moss grows where the surface is low: in joints and cracks (cavity from
    // the baked height), creeping onto the stone only when overgrown, in
    // patches (noise). A face (height 0.85 and up) never gets it by noise
    // alone.
    let cav = 1.0 - t.h;
    let mn = smoothstep(0.3, 0.7, mp_fbm(q / max(mos.w, 0.02), 313u));
    let mv = 0.9 * cav + 0.25 * mn + 0.1 * up;
    let thr = 0.95 - 0.7 * lay.z;
    let mm = smoothstep(thr, thr + 0.12, mv) * min(lay.z * 8.0, 1.0);
    let mfine = mp_fbm(q * 45.0, 314u);
    c = mix(c, mos.rgb * (0.7 + 0.6 * mfine), mm);
    rough = mix(rough, 1.0, mm);
    g = g * (1.0 - 0.7 * mm);
    // Wet: porous surfaces darken and gloss over.
    let wet = clamp(lay.x, 0.0, 1.0) * (1.0 - 0.5 * mm);
    c = c * (1.0 - 0.45 * wet);
    rough = mix(rough, rough * 0.3 + 0.05, wet);
    // Water: puddles in the low spots of upward faces; with wetness the
    // joints fill first. `P` is the puddle field (positive over the
    // `puddles` share of the ground); the water level starts above the
    // joints (0.55) and climbs past the stone tops (0.7 to 1) in the middle
    // of a puddle, so a puddle is a flat sheet with stones breaking out at
    // its rim.
    let upm = smoothstep(0.75, 0.95, n.y);
    let pnn = smoothstep(0.3, 0.7, mp_fbm(p.xz / max(lay.w, 0.1), 317u));
    let pf = pnn + clamp(lay.y, 0.0, 1.0) - 1.0;
    let wl = max(select(0.0, 0.55 + 2.5 * pf, pf > 0.0), 0.45 * wet) * upm;
    let dp = wl - t.h;
    let pm = clamp(dp / 0.05, 0.0, 1.0) * select(0.0, 1.0, wl > 0.0);
    // Just above the water line the stone is soaked.
    c = c * mix(1.0, 0.7, smoothstep(-0.15, 0.0, dp) * select(0.0, 1.0, wl > 0.0));
    // The water body: dark with a faint blue-green, the sky does the rest.
    c = mix(c, c * 0.2 + vec3<f32>(0.004, 0.012, 0.016), pm);
    rough = mix(rough, 0.03, pm);
    g = g * (1.0 - pm);
//$DEBUG
    if (mode >= 2u) {
        *nr = vec4<f32>(normalize(n - g), clamp(rough, 0.02, 1.0));
    }
    return c;
}

// Relief surface k on face f of box bi (planar on the face's pattern uv).
fn rl_face(k: u32, s: MpS, bi: u32, n: vec3<f32>, nr: ptr<function, vec4<f32>>) -> vec3<f32> {
    if (u32(kit_data[MP_RELIEF].z) == 0u) {
        return rl_flat(k);
    }
    let o = u32(kit_data[1].y) + 9u * bi;
    let flags = u32(kit_data[o].w);
    let q = kit_data[o + 1u];
    let a = s.face >> 1u;
    var u = rl_axis(mp_ua(a));
    var v = rl_axis(mp_va(a));
    if ((flags & 1u) == 0u) {
        u = rl_rot(q, u);
        v = rl_rot(q, v);
    }
    let t = rl_sample(k, s.uv, s.fw);
    return rl_shade(k, t, s.p, n, u * t.s.x + v * t.s.y, nr);
}

// Relief surface k on any other world mesh (rocks): triplanar.
fn rl_free(k: u32, p: vec3<f32>, n: vec3<f32>, fw: f32, nr: ptr<function, vec4<f32>>) -> vec3<f32> {
    if (u32(kit_data[MP_RELIEF].z) == 0u) {
        return rl_flat(k);
    }
    let w2 = n * n;
    var w = w2 * w2;
    w = w / max(w.x + w.y + w.z, 1e-5);
    var t: RlTex;
    t.c = vec3<f32>(0.0);
    t.s = vec2<f32>(0.0);
    t.h = 0.0;
    t.r = 0.0;
    var g = vec3<f32>(0.0);
    if (w.x > 0.02) {
        let a = rl_sample(k, p.zy, fw);
        t.c = t.c + a.c * w.x;
        t.h = t.h + a.h * w.x;
        t.r = t.r + a.r * w.x;
        g = g + w.x * vec3<f32>(0.0, a.s.y, a.s.x);
    }
    if (w.y > 0.02) {
        let a = rl_sample(k, p.xz, fw);
        t.c = t.c + a.c * w.y;
        t.h = t.h + a.h * w.y;
        t.r = t.r + a.r * w.y;
        g = g + w.y * vec3<f32>(a.s.x, 0.0, a.s.y);
    }
    if (w.z > 0.02) {
        let a = rl_sample(k, p.xy, fw);
        t.c = t.c + a.c * w.z;
        t.h = t.h + a.h * w.z;
        t.r = t.r + a.r * w.z;
        g = g + w.z * vec3<f32>(a.s.x, a.s.y, 0.0);
    }
    let kept = select(0.0, w.x, w.x > 0.02) + select(0.0, w.y, w.y > 0.02) + select(0.0, w.z, w.z > 0.02);
    let inv = 1.0 / max(kept, 1e-5);
    t.c = t.c * inv;
    t.h = t.h * inv;
    t.r = t.r * inv;
    // Only the slope across the surface bends it.
    g = (g - n * dot(g, n)) * inv;
    return rl_shade(k, t, p, n, g, nr);
}

// mp_world with relief surfaces. `full` false paints them flat (the shadow
// pass and the plain kit_material path need no relief).
fn rl_world(p: vec3<f32>, n: vec3<f32>, fw: f32, full: bool, nr: ptr<function, vec4<f32>>) -> vec3<f32> {
    let on = rl_on();
    let ns = select(0u, u32(kit_data[MP_RELIEF].y), on);
    let hit = mp_find(p, n, false);
    if (hit.x < 0) {
        let fs = select(-1.0, kit_data[MP_RELIEF + 3u].x, on);
        if (fs >= 0.0 && u32(fs) < ns) {
            if (full) {
                return rl_free(u32(fs), p, n, fw, nr);
            }
            return rl_flat(u32(fs));
        }
        return vec3<f32>(0.5);
    }
    var s = mp_ctx(u32(hit.x), u32(hit.y), p, n, fw);
    var surf = s.surf;
    if (surf == 0u) {
        let q = p + n * 0.35;
        let r = mp_region(q.x, q.z);
        surf = u32(r.x);
        s.variant = u32(r.z);
    } else if (surf == 1u) {
        let r = mp_region(p.x, p.z);
        surf = u32(r.y);
        s.variant = u32(r.z);
    }
    s.surf = surf;
    var c: vec3<f32>;
    if (surf >= RL_FIRST && surf - RL_FIRST < ns) {
        if (full) {
            c = rl_face(surf - RL_FIRST, s, u32(hit.x), n, nr);
        } else {
            c = rl_flat(surf - RL_FIRST);
        }
    } else {
        c = mp_paint(surf, s) * mp_tune(surf, 0u).rgb;
    }
    // The baked AO and its warm bounce, as mp_world.
    let h = kit_data[7];
    let ao = clamp(s.ao, 0.0, 1.0);
    let warm = mix(vec3<f32>(1.1, 1.0, 0.84), vec3<f32>(1.2, 1.0, 0.7), clamp(s.up, 0.0, 1.0));
    c = c * mix(vec3<f32>(1.0), warm, clamp((1.0 - ao) * 1.6 * h.w, 0.0, 1.0));
    return c * pow(ao, h.z);
}
