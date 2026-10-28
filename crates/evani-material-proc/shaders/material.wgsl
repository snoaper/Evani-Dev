// evani-material-proc: procedural surfaces for box-built worlds.
//
// Appended to the forward and shadow modules through the kit-material hook
// (evani-render `kit-material`). `kit_material(p, n, material)` returns the
// albedo (rgb, linear; it multiplies the instance colour, which the module
// sets to 1 / the slot's base colour) and a cut-out alpha. The three slot
// numbers below are filled in by `shader::wgsl`, which appends the sky part
// (shaders/sky.wgsl, feature `sky`) and the entry (shaders/entry.wgsl).
//
// * WORLD slot: find the box face under `p` in `kit_data` (a 2 m grid of box
//   lists), paint its surface (procedural, no textures) and darken it by the
//   baked ambient occlusion of that face. Alpha is always 1, so the shadow
//   pass (which only reads alpha) has nothing to evaluate.
// * DECAL slot: the stencil surfaces (letters, arrows): alpha is the paint
//   mask, so the decal box cuts down to the stroke.
// * SKY slot (feature `sky`): the sky dome (a large unlit sphere round the
//   camera): a Preetham daylight sky with drifting clouds from the view
//   direction (the dome's normal). Its parameters are kit_data records 3 to 6.
// * Any other slot: vec4(1): untouched.
//
// The live material params (src/params.rs) are kit_data record 7 (`[tune
// base, tune count, AO strength, AO warmth]`) and two records per surface id
// from the tune base: `[colour multiplier rgb, wear]`, `[grime, streaks, 0, 0]`.
// At their defaults every multiplier is 1, so the surfaces paint as authored.
//
// Every name is prefixed `mp_`/`Mp` to stay clear of the host module.

const MP_WORLD: u32 = $WORLD_SLOTu;
const MP_DECAL: u32 = $DECAL_SLOTu;
const MP_SKY: u32 = $SKY_SLOTu;

struct MpS {
    // World position.
    p: vec3<f32>,
    // Pattern coordinates (m): world-aligned for world-pattern boxes, else
    // the face's own from its min corner plus a per-box offset.
    uv: vec2<f32>,
    // Face coordinates from the face centre, and the face's half size (m).
    lc: vec2<f32>,
    hf: vec2<f32>,
    // Distance to the nearest convex edge (m; 100 when none).
    edge: f32,
    // Baked ambient occlusion (1 = open).
    ao: f32,
    // World normal's y.
    up: f32,
    seed: u32,
    variant: u32,
    surf: u32,
    // Face index (axis * 2 + positive).
    face: u32,
    // +1 / -1: the u direction as seen from in front (letters read right).
    flip: f32,
    // Pixel footprint (m), for fading out detail smaller than a pixel.
    fw: f32,
};

struct MpBond {
    id: vec2<i32>,
    f: vec2<f32>,
    d: f32,
};

// ---- maths ------------------------------------------------------------------

fn mp_rot_inv(q: vec4<f32>, v: vec3<f32>) -> vec3<f32> {
    let qv = -q.xyz;
    let t = 2.0 * cross(qv, v);
    return v + q.w * t + cross(qv, t);
}

fn mp_comp(v: vec3<f32>, i: u32) -> f32 {
    return v[i];
}

fn mp_ua(a: u32) -> u32 {
    return select(0u, 2u, a == 0u);
}

fn mp_va(a: u32) -> u32 {
    return select(1u, 2u, a == 1u);
}

fn mp_f(i: u32) -> f32 {
    let r = kit_data[i >> 2u];
    return r[i & 3u];
}

// The live tuning of surface `surf`: `[colour multiplier rgb, wear]` and
// `[grime, streaks, 0, 0]` (all 1 when the surface has no record).
fn mp_tune(surf: u32, k: u32) -> vec4<f32> {
    let h = kit_data[7];
    if (surf >= u32(h.y)) {
        return vec4<f32>(1.0);
    }
    return kit_data[u32(h.x) + 2u * surf + k];
}

fn mp_pcg(v: u32) -> u32 {
    let s = v * 747796405u + 2891336453u;
    let w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (w >> 22u) ^ w;
}

fn mp_h2(i: vec2<i32>, s: u32) -> f32 {
    let h = mp_pcg(bitcast<u32>(i.x) ^ mp_pcg(bitcast<u32>(i.y) + s * 1013u));
    return f32(h >> 8u) * (1.0 / 16777216.0);
}

fn mp_vnoise(x: vec2<f32>, s: u32) -> f32 {
    let fl = floor(x);
    let i = vec2<i32>(fl);
    let f = x - fl;
    let u = f * f * (3.0 - 2.0 * f);
    let a = mp_h2(i, s);
    let b = mp_h2(i + vec2<i32>(1, 0), s);
    let c = mp_h2(i + vec2<i32>(0, 1), s);
    let d = mp_h2(i + vec2<i32>(1, 1), s);
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

fn mp_fbm(x: vec2<f32>, s: u32) -> f32 {
    var p = x;
    var a = 0.5;
    var t = 0.0;
    for (var k = 0u; k < 4u; k = k + 1u) {
        t = t + a * mp_vnoise(p, s + k * 7u);
        p = vec2<f32>(p.x * 1.6 - p.y * 1.2, p.x * 1.2 + p.y * 1.6) + 17.3;
        a = a * 0.5;
    }
    return t / 0.9375;
}

// 1 while a detail of size `size` (m) spans several pixels, 0 once it is
// under about a third of one.
fn mp_lod(fw: f32, size: f32) -> f32 {
    return 1.0 - smoothstep(size * 0.35, size, fw);
}

// A line of half width `w` at distance `d`: antialiased, and thinned to its
// average coverage once it is narrower than a pixel.
fn mp_line(d: f32, w: f32, fw: f32) -> f32 {
    let f = max(fw, 1e-5);
    let cov = clamp(2.0 * w / f, 0.0, 1.0);
    let ww = max(w, f * 0.5);
    return (1.0 - smoothstep(ww - f * 0.5, ww + f * 0.5, d)) * cov;
}

// Running bond: block id, position inside the block, distance to its edge.
fn mp_bond(uv: vec2<f32>, size: vec2<f32>) -> MpBond {
    let row = floor(uv.y / size.y);
    let x = uv.x + select(0.0, size.x * 0.5, (i32(row) & 1) == 1);
    let col = floor(x / size.x);
    var b: MpBond;
    b.id = vec2<i32>(i32(col), i32(row));
    b.f = vec2<f32>(x - col * size.x, uv.y - row * size.y);
    b.d = min(min(b.f.x, size.x - b.f.x), min(b.f.y, size.y - b.f.y));
    return b;
}

// ---- lookup -------------------------------------------------------------------

// The box (x) and face (y) under `p` with normal `n`, or -1. Mirrors
// `layout::find` exactly.
fn mp_find(p: vec3<f32>, n: vec3<f32>, decal: bool) -> vec2<i32> {
    let g = kit_data[0];
    let h1 = kit_data[1];
    let nx = u32(g.w);
    let nz = u32(h1.x);
    let fx = floor((p.x - g.x) / g.z);
    let fz = floor((p.z - g.y) / g.z);
    if (fx < 0.0 || fz < 0.0 || fx >= f32(nx) || fz >= f32(nz)) {
        return vec2<i32>(-1);
    }
    let ci = u32(fz) * nx + u32(fx);
    let box_base = u32(h1.y);
    let list_base = u32(h1.z);
    let cell_base = u32(h1.w);
    let cr = kit_data[cell_base + (ci >> 1u)];
    let odd = (ci & 1u) == 1u;
    let start = u32(select(cr.x, cr.z, odd));
    let count = u32(select(cr.y, cr.w, odd));
    var best = vec2<i32>(-1);
    var best_d = 0.03;
    for (var k = start; k < start + count; k = k + 1u) {
        let bi = u32(kit_data[list_base + (k >> 2u)][k & 3u]);
        let o = box_base + 9u * bi;
        let r0 = kit_data[o];
        if (((u32(r0.w) & 2u) != 0u) != decal) {
            continue;
        }
        let q = kit_data[o + 1u];
        let half = kit_data[o + 2u].xyz;
        let l = mp_rot_inv(q, p - r0.xyz);
        let ln = mp_rot_inv(q, n);
        let an = abs(ln);
        var a = 2u;
        if (an.x >= an.y && an.x >= an.z) {
            a = 0u;
        } else if (an.y >= an.z) {
            a = 1u;
        }
        let la = mp_comp(ln, a);
        if (abs(la) <= 0.7) {
            continue;
        }
        let s = select(-1.0, 1.0, la > 0.0);
        let dist = abs(mp_comp(l, a) - s * mp_comp(half, a));
        if (dist >= best_d) {
            continue;
        }
        let ua = mp_ua(a);
        let va = mp_va(a);
        if (abs(mp_comp(l, ua)) > mp_comp(half, ua) + 0.03 || abs(mp_comp(l, va)) > mp_comp(half, va) + 0.03) {
            continue;
        }
        best_d = dist;
        best = vec2<i32>(i32(bi), i32(a * 2u + select(0u, 1u, s > 0.0)));
    }
    return best;
}

// Everything the painters need about face `f` of box `bi` at `p`.
fn mp_ctx(bi: u32, f: u32, p: vec3<f32>, n: vec3<f32>, fw: f32) -> MpS {
    let o = u32(kit_data[1].y) + 9u * bi;
    let r0 = kit_data[o];
    let q = kit_data[o + 1u];
    let r2 = kit_data[o + 2u];
    let fr = kit_data[o + 3u + f];
    let flags = u32(r0.w);
    let a = f >> 1u;
    let pos = (f & 1u) == 1u;
    let ua = mp_ua(a);
    let va = mp_va(a);
    let l = mp_rot_inv(q, p - r0.xyz);
    let half = r2.xyz;
    var s: MpS;
    s.p = p;
    s.fw = fw;
    s.up = n.y;
    s.face = f;
    s.lc = vec2<f32>(mp_comp(l, ua), mp_comp(l, va));
    s.hf = vec2<f32>(mp_comp(half, ua), mp_comp(half, va));
    s.seed = u32(r2.w);
    s.variant = flags >> 8u;
    let off = vec2<f32>(f32(s.seed & 255u) * 1.37, f32(s.seed >> 8u) * 0.91);
    // World-pattern faces share one pattern across merged coplanar boxes; the
    // plane's position and facing shift it so that facing walls differ.
    let wo = vec2<f32>(mp_comp(p, a) * 0.731 + f32(f) * 17.3, 0.0);
    s.uv = select(s.lc + s.hf + off, vec2<f32>(mp_comp(p, ua), mp_comp(p, va)) + wo, (flags & 1u) != 0u);
    if (a == 0u) {
        s.flip = select(1.0, -1.0, pos);
    } else if (a == 2u) {
        s.flip = select(-1.0, 1.0, pos);
    } else {
        s.flip = 1.0;
    }
    let sf = u32(fr.w);
    s.surf = sf & 255u;
    let e = sf >> 8u;
    var ed = 100.0;
    if ((e & 1u) != 0u) {
        ed = min(ed, s.lc.x + s.hf.x);
    }
    if ((e & 2u) != 0u) {
        ed = min(ed, s.hf.x - s.lc.x);
    }
    if ((e & 4u) != 0u) {
        ed = min(ed, s.lc.y + s.hf.y);
    }
    if ((e & 8u) != 0u) {
        ed = min(ed, s.hf.y - s.lc.y);
    }
    s.edge = max(ed, 0.0);
    s.ao = 1.0;
    if (fr.x >= 0.0) {
        let nu = max(u32(fr.y), 1u);
        let nv = max(u32(fr.z), 1u);
        let t = (s.lc + s.hf) / max(2.0 * s.hf, vec2<f32>(1e-4)) * vec2<f32>(f32(nu), f32(nv)) - 0.5;
        let tc = clamp(t, vec2<f32>(0.0), vec2<f32>(f32(nu - 1u), f32(nv - 1u)));
        let i0 = vec2<u32>(floor(tc));
        let i1 = min(i0 + 1u, vec2<u32>(nu - 1u, nv - 1u));
        let w = tc - floor(tc);
        let b = u32(fr.x);
        let a00 = mp_f(b + i0.y * nu + i0.x);
        let a10 = mp_f(b + i0.y * nu + i1.x);
        let a01 = mp_f(b + i1.y * nu + i0.x);
        let a11 = mp_f(b + i1.y * nu + i1.x);
        s.ao = mix(mix(a00, a10, w.x), mix(a01, a11, w.x), w.y);
    }
    return s;
}

// The region record ([wall, floor, variant, 0]) at (x, z): first match.
fn mp_region(x: f32, z: f32) -> vec4<f32> {
    let h2 = kit_data[2];
    let base = u32(h2.y);
    let nr = u32(h2.z);
    for (var i = 0u; i < nr; i = i + 1u) {
        let r = kit_data[base + 2u * i];
        if (x >= r.x && x <= r.z && z >= r.y && z <= r.w) {
            return kit_data[base + 2u * i + 1u];
        }
    }
    return vec4<f32>(2.0, 18.0, 0.0, 0.0);
}

// ---- shared wear ------------------------------------------------------------

// Height above the bottom of the box (vertical faces of upright boxes).
fn mp_hb(s: MpS) -> f32 {
    return s.lc.y + s.hf.y;
}

// Dirt kicked up the base of a vertical face, `height` m high.
fn mp_grime(c: vec3<f32>, s: MpS, height: f32, amt: f32) -> vec3<f32> {
    if (abs(s.up) > 0.5) {
        return c;
    }
    let nz = mp_fbm(s.uv * vec2<f32>(1.3, 2.5), 7u);
    let g = clamp((1.0 - smoothstep(0.0, height * (0.6 + 0.8 * nz), mp_hb(s))) * amt * mp_tune(s.surf, 1u).x, 0.0, 1.0);
    return mix(c, c * vec3<f32>(0.55, 0.49, 0.42), g);
}

// Rain and run-off streaks from the top of a vertical face.
fn mp_streaks(c: vec3<f32>, s: MpS, amt: f32, reach: f32) -> vec3<f32> {
    if (abs(s.up) > 0.5) {
        return c;
    }
    let st = mp_fbm(vec2<f32>(s.uv.x * 3.1, s.uv.y * 0.11), 11u);
    let top = s.hf.y - s.lc.y;
    let k = clamp(smoothstep(0.52, 0.8, st) * (1.0 - smoothstep(0.0, reach, top) * 0.7) * amt * mp_tune(s.surf, 1u).y, 0.0, 1.0);
    return c * (1.0 - k);
}

// Chips along convex edges (1 = chipped), `w` m deep at most.
// Scaled by the surface's live `wear`.
fn mp_chip(s: MpS, w: f32) -> f32 {
    if (s.edge > w * 2.0) {
        return 0.0;
    }
    let nz = mp_fbm(s.uv * 7.0 + 1.7, 131u);
    let ww = w * (0.15 + 1.7 * nz * nz);
    return clamp((1.0 - smoothstep(ww * 0.6, ww, s.edge)) * mp_lod(s.fw, w * 0.5) * mp_tune(s.surf, 0u).w, 0.0, 1.0);
}

// ---- materials ----------------------------------------------------------------

fn mp_stone_blocks(s: MpS, size: vec2<f32>, base: vec3<f32>) -> vec3<f32> {
    let b = mp_bond(s.uv, size);
    let h = mp_h2(b.id, 21u);
    let h2 = mp_h2(b.id, 22u);
    var c = base * (0.8 + 0.32 * h) * mix(vec3<f32>(1.05, 1.0, 0.92), vec3<f32>(0.94, 1.0, 1.07), h2);
    let n1 = mp_fbm(s.uv * 2.3, 23u);
    let n2 = mp_fbm(s.uv * 9.0, 24u);
    c = c * (0.84 + 0.28 * n1);
    c = c * mix(1.0, 0.88 + 0.24 * n2, mp_lod(s.fw, 0.1));
    // Pits.
    let pc = floor(s.uv * 26.0);
    let ph = mp_h2(vec2<i32>(pc), 25u);
    let pd = length(s.uv * 26.0 - pc - 0.5);
    c = c * (1.0 - 0.4 * select(0.0, 1.0 - smoothstep(0.12, 0.32, pd), ph > 0.9) * mp_lod(s.fw, 0.03));
    // Rounded arrises darken towards the joint, then the mortar.
    let lod = mp_lod(s.fw, 0.05);
    c = c * mix(1.0, 0.72 + 0.28 * smoothstep(0.0, 0.035, b.d), lod);
    c = mix(c, vec3<f32>(0.27, 0.23, 0.18) * (0.8 + 0.3 * n2), mp_line(b.d, 0.006, s.fw));
    return c;
}

fn mp_slabs(s: MpS, size: f32, base: vec3<f32>, seed: u32) -> vec3<f32> {
    let pid = floor(s.uv / size);
    let pf = s.uv - pid * size;
    let n1 = mp_fbm(s.uv * 1.1, seed);
    let n2 = mp_fbm(s.uv * 7.0, seed + 1u);
    var c = base * (0.86 + 0.24 * n1) * mix(1.0, 0.9 + 0.2 * n2, mp_lod(s.fw, 0.1));
    c = c * (0.9 + 0.18 * mp_h2(vec2<i32>(pid), seed + 2u));
    let d = min(min(pf.x, size - pf.x), min(pf.y, size - pf.y));
    c = c * mix(1.0, 0.85 + 0.15 * smoothstep(0.0, 0.05, d), mp_lod(s.fw, 0.06));
    c = mix(c, vec3<f32>(0.2, 0.18, 0.15), mp_line(d, 0.006, s.fw));
    return c;
}

fn mp_plaster(s: MpS) -> vec3<f32> {
    let vert = abs(s.up) < 0.5;
    var base = vec3<f32>(0.58, 0.46, 0.28);
    if (s.variant == 1u) {
        base = vec3<f32>(0.68, 0.63, 0.53);
    } else if (s.variant == 2u) {
        base = vec3<f32>(0.64, 0.58, 0.46);
    }
    let m1 = mp_fbm(s.uv * 0.35, 1u);
    let m2 = mp_fbm(s.uv * 1.7 + 3.1, 2u);
    var c = base * (0.84 + 0.26 * m1) * (0.94 + 0.12 * m2);
    c = c * mix(vec3<f32>(1.04, 1.0, 0.94), vec3<f32>(0.96, 1.0, 1.05), mp_fbm(s.uv * 0.12 + 7.0, 3u));
    // Speckle.
    let sp = mp_h2(vec2<i32>(floor(s.uv * 60.0)), 4u);
    c = c * (1.0 + mp_lod(s.fw, 0.07) * (select(0.0, -0.14, sp > 0.95) + select(0.0, 0.07, sp < 0.03)));
    if (vert) {
        let hb = mp_hb(s);
        if (s.variant == 2u) {
            // A painted dado, chipped and faded.
            let dado = 1.2 + 0.015 * (mp_vnoise(vec2<f32>(s.uv.x * 3.0, 0.0), 5u) - 0.5);
            if (hb < dado) {
                let fade = mp_fbm(s.uv * 0.9 + 2.0, 6u);
                c = mix(vec3<f32>(0.09, 0.19, 0.22), vec3<f32>(0.16, 0.26, 0.27), fade * 0.6) * (0.9 + 0.2 * m2);
                let flake = smoothstep(0.66, 0.7, mp_fbm(s.uv * 2.5 + 9.0, 8u));
                c = mix(c, base * 0.85, flake);
            }
            c = mix(c, vec3<f32>(0.04, 0.05, 0.05), mp_line(abs(hb - dado), 0.01, s.fw));
        }
        // Fallen patches showing the stone underneath.
        let pm = mp_fbm(s.uv * vec2<f32>(0.42, 0.6) + vec2<f32>(f32(s.variant) * 5.3, 1.0), 9u);
        let th = 0.67;
        if (pm > th) {
            c = mp_stone_blocks(s, vec2<f32>(0.6, 0.3), vec3<f32>(0.46, 0.33, 0.19)) * 0.9;
        }
        let lip = 1.0 - smoothstep(0.0, 0.018, abs(pm - th));
        c = c * (1.0 - 0.4 * lip * select(1.0, 0.3, pm < th));
        c = mp_streaks(c, s, 0.3, 2.5);
        c = mp_grime(c, s, 0.7, 0.55);
        if (hb < 0.16 && s.variant != 2u) {
            c = vec3<f32>(0.3, 0.28, 0.25) * (0.85 + 0.3 * m2);
            c = mp_grime(c, s, 0.3, 0.4);
        }
        c = c * (1.0 - 0.45 * mp_line(abs(hb - 0.16), 0.004, s.fw));
    }
    c = mix(c, vec3<f32>(0.34, 0.27, 0.18) * (0.8 + 0.4 * m2), mp_chip(s, 0.05));
    return c;
}

fn mp_sandstone(s: MpS) -> vec3<f32> {
    var base = vec3<f32>(0.55, 0.35, 0.16);
    if (s.variant == 1u) {
        base = vec3<f32>(0.4, 0.3, 0.19);
    }
    var c = mp_stone_blocks(s, vec2<f32>(0.6, 0.3), base);
    if (s.variant == 1u) {
        // Weathered: lichen-grey blotches and salt bloom.
        let w = smoothstep(0.5, 0.8, mp_fbm(s.uv * 0.5 + 3.0, 26u));
        c = mix(c, vec3<f32>(0.36, 0.34, 0.3) * (0.8 + 0.4 * mp_fbm(s.uv * 4.0, 27u)), w * 0.6);
    }
    c = mp_streaks(c, s, 0.25, 3.0);
    c = mp_grime(c, s, 0.6, 0.45);
    c = mix(c, c * 1.18, mp_chip(s, 0.04));
    return c;
}

fn mp_concrete(s: MpS, base: vec3<f32>) -> vec3<f32> {
    let n1 = mp_fbm(s.uv * 0.8, 31u);
    let n2 = mp_fbm(s.uv * 6.0, 32u);
    var c = base * (0.85 + 0.25 * n1) * mix(1.0, 0.92 + 0.16 * n2, mp_lod(s.fw, 0.12));
    if (abs(s.up) < 0.5) {
        // Form panels 1.2 x 0.6 m with their tie holes.
        let ps = vec2<f32>(1.2, 0.6);
        let pid = floor(s.uv / ps);
        let pf = s.uv - pid * ps;
        c = c * (0.93 + 0.14 * mp_h2(vec2<i32>(pid), 33u));
        let d = min(min(pf.x, ps.x - pf.x), min(pf.y, ps.y - pf.y));
        c = c * (1.0 - 0.45 * mp_line(d, 0.004, s.fw));
        let tp = vec2<f32>(select(0.3, 0.9, pf.x > 0.6), 0.3);
        let td = length(pf - tp);
        c = c * (1.0 - 0.6 * (1.0 - smoothstep(0.011, 0.011 + s.fw, td)) * mp_lod(s.fw, 0.025));
        c = c * (1.0 + 0.06 * (1.0 - smoothstep(0.015, 0.05, td)) * mp_lod(s.fw, 0.05));
        c = mp_streaks(c, s, 0.3, 3.0);
        c = mp_grime(c, s, 0.6, 0.45);
    } else {
        // Floor slabs with saw cuts and oil stains.
        let pid = floor(s.uv / 2.4);
        let pf = s.uv - pid * 2.4;
        c = c * (0.92 + 0.15 * mp_h2(vec2<i32>(pid), 35u));
        let d = min(min(pf.x, 2.4 - pf.x), min(pf.y, 2.4 - pf.y));
        c = c * (1.0 - 0.4 * mp_line(d, 0.005, s.fw));
        c = c * (1.0 - 0.2 * smoothstep(0.55, 0.8, mp_fbm(s.uv * 0.5 + 9.0, 36u)));
    }
    c = mix(c, c * 1.25, mp_chip(s, 0.03));
    return c;
}

fn mp_ground(s: MpS) -> vec3<f32> {
    let base = vec3<f32>(0.51, 0.39, 0.22);
    let n1 = mp_fbm(s.uv * 0.25, 41u);
    let n2 = mp_fbm(s.uv * 3.0, 42u);
    var c = base * (0.84 + 0.28 * n1) * mix(1.0, 0.9 + 0.2 * n2, mp_lod(s.fw, 0.1));
    // Packed slabs 2.4 m, worn joints.
    let pid = floor(s.uv / 2.4);
    let pf = s.uv - pid * 2.4;
    c = c * (0.93 + 0.13 * mp_h2(vec2<i32>(pid), 43u));
    let d = min(min(pf.x, 2.4 - pf.x), min(pf.y, 2.4 - pf.y));
    c = c * (1.0 - 0.32 * mp_line(d, 0.008, s.fw));
    // Drifted sand where the walls shelter it (low AO) and in patches.
    let drift = clamp((1.0 - s.ao) * 2.2 + (mp_fbm(s.uv * 0.6 + 4.0, 44u) - 0.56) * 1.8, 0.0, 1.0);
    c = mix(c, vec3<f32>(0.62, 0.49, 0.3) * (0.95 + 0.1 * n2), drift);
    // Pebbles.
    let g = s.uv * 12.0;
    let cell = floor(g);
    let ci = vec2<i32>(cell);
    let off = vec2<f32>(mp_h2(ci, 46u), mp_h2(ci, 47u)) * 0.5 + 0.25;
    let pd = length(g - cell - off);
    let peb = select(0.0, 1.0 - smoothstep(0.1, 0.2, pd), mp_h2(ci, 45u) > 0.8) * mp_lod(s.fw, 0.05) * (1.0 - drift * 0.7);
    c = mix(c, base * (0.5 + 0.8 * mp_h2(ci, 48u)), peb);
    return c;
}

fn mp_wood_crate(s: MpS) -> vec3<f32> {
    let plank = vec3<f32>(0.3, 0.14, 0.045);
    let frame = vec3<f32>(0.22, 0.105, 0.037);
    let lc = s.lc;
    let hf = s.hf;
    let sd = f32(s.seed & 1023u) * 0.37;
    let db = min(hf.x - abs(lc.x), hf.y - abs(lc.y));
    let fwid = min(0.11, min(hf.x, hf.y) * 0.3);
    var c: vec3<f32>;
    if (db < fwid) {
        // Frame: grain along each member.
        let along_u = (hf.y - abs(lc.y)) < (hf.x - abs(lc.x));
        let gx = select(lc.y, lc.x, along_u);
        let gy = select(lc.x, lc.y, along_u);
        let g = mp_fbm(vec2<f32>(gx * 1.2, gy * 30.0) + sd, 51u);
        c = frame * (0.78 + 0.38 * g);
        c = c * (1.0 - 0.55 * mp_line(fwid - db, 0.004, s.fw));
        // Nails down the posts, one per plank, and at the corners.
        let nxp = sign(lc.x) * (hf.x - fwid * 0.5);
        let row = floor((lc.y + hf.y) / 0.14);
        let nyp = clamp((row + 0.5) * 0.14 - hf.y, -hf.y + fwid * 0.5, hf.y - fwid * 0.5);
        let nd = length(vec2<f32>(lc.x - nxp, lc.y - nyp));
        c = mix(c, vec3<f32>(0.05, 0.045, 0.04), (1.0 - smoothstep(0.005, 0.005 + s.fw, nd)) * mp_lod(s.fw, 0.012));
    } else {
        let ix = hf.x - fwid;
        let iy = hf.y - fwid;
        let ln = max(length(vec2<f32>(ix, iy)), 1e-3);
        let dl = abs(lc.x * iy - lc.y * ix) / ln;
        if (dl < fwid * 0.5) {
            // The diagonal brace.
            let t = (lc.x * ix + lc.y * iy) / ln;
            let g = mp_fbm(vec2<f32>(t * 1.2, dl * 30.0) + sd, 52u);
            c = frame * (0.82 + 0.34 * g);
            c = c * (1.0 - 0.5 * mp_line(fwid * 0.5 - dl, 0.004, s.fw));
        } else {
            // Planks 0.14 m.
            let pi = floor((lc.y + hf.y) / 0.14);
            let pf = (lc.y + hf.y) - pi * 0.14;
            let ph = mp_h2(vec2<i32>(i32(pi), i32(s.seed)), 53u);
            let g = mp_fbm(vec2<f32>(lc.x * 0.9 + ph * 7.0, lc.y * 26.0) + sd, 54u);
            let rings = 0.5 + 0.5 * sin((lc.x * 2.5 + g * 5.0) * 6.2832);
            c = plank * (0.74 + 0.32 * ph) * (0.8 + 0.32 * g) * (0.92 + 0.12 * rings * mp_lod(s.fw, 0.02));
            c = c * (1.0 - 0.65 * mp_line(min(pf, 0.14 - pf), 0.003, s.fw));
            // Shadow cast by the brace and frame onto the recessed planks.
            c = c * (0.82 + 0.18 * smoothstep(0.0, 0.03, min(db - fwid, dl - fwid * 0.5)));
        }
    }
    // Sun-bleached top, worn light edges, dirt at the foot.
    c = mix(c, vec3<f32>(0.3, 0.25, 0.19) * (0.85 + 0.3 * mp_fbm(s.uv * 3.0, 55u)), clamp(s.up, 0.0, 1.0) * 0.45);
    c = mix(c, c * 1.7 + 0.015, mp_chip(s, 0.025));
    c = mp_grime(c, s, 0.35, 0.4);
    return c;
}

fn mp_metal_crate(s: MpS) -> vec3<f32> {
    let k = s.seed % 3u;
    var paint = vec3<f32>(0.075, 0.11, 0.05);
    if (k == 1u) {
        paint = vec3<f32>(0.035, 0.07, 0.14);
    } else if (k == 2u) {
        paint = vec3<f32>(0.17, 0.05, 0.03);
    }
    let n1 = mp_fbm(s.uv * 1.5, 61u);
    let n2 = mp_fbm(s.uv * 12.0, 62u);
    var c = paint * (0.82 + 0.34 * n1);
    let lc = s.lc;
    let hf = s.hf;
    let db = min(hf.x - abs(lc.x), hf.y - abs(lc.y));
    if (abs(s.up) < 0.5) {
        if (db > 0.06) {
            // Ribs every 0.15 m.
            let rp = fract((lc.x + hf.x) / 0.15);
            c = c * (1.0 + 0.18 * sin(rp * 6.2832) * mp_lod(s.fw, 0.05));
        } else {
            c = c * 0.86;
        }
        c = c * (1.0 - 0.45 * mp_line(abs(db - 0.06), 0.003, s.fw));
        // Rust running down from the top.
        let st = mp_fbm(vec2<f32>(lc.x * 3.5, lc.y * 0.35) + f32(s.seed & 255u), 63u);
        let top = hf.y - lc.y;
        let rs = smoothstep(0.55, 0.85, st) * (1.0 - smoothstep(0.0, hf.y * 2.0, top) * 0.8);
        c = mix(c, vec3<f32>(0.25, 0.09, 0.03) * (0.7 + 0.5 * n2), rs * 0.7);
        // Stencilled stripe.
        if (abs(lc.y - hf.y * 0.35) < 0.03 && db > 0.1) {
            // Ochre, half worn away.
            let keep = smoothstep(0.42, 0.62, mp_fbm(s.uv * vec2<f32>(9.0, 4.0), 65u));
            c = mix(c, vec3<f32>(0.36, 0.26, 0.07) * (0.85 + 0.3 * n2), 0.8 * keep);
        }
    } else {
        c = c * (1.0 - 0.45 * mp_line(abs(db - 0.06), 0.003, s.fw));
        c = mix(c, vec3<f32>(0.45, 0.38, 0.27) * (0.85 + 0.3 * n2), clamp(s.up, 0.0, 1.0) * (0.25 + 0.45 * n1));
    }
    // Paint worn through to bare steel at the edges, with a rust fringe.
    let wn = mp_fbm(s.uv * 8.0 + 3.3, 64u);
    let ww = 0.01 + 0.045 * wn * wn;
    let wear = (1.0 - smoothstep(ww * 0.6, ww, s.edge)) * mp_lod(s.fw, 0.01);
    let fringe = clamp((1.0 - smoothstep(ww, ww * 1.7, s.edge)) - wear, 0.0, 1.0);
    c = mix(c, vec3<f32>(0.2, 0.08, 0.035), fringe * 0.6);
    c = mix(c, vec3<f32>(0.35, 0.34, 0.32) * (0.8 + 0.3 * n2), wear);
    c = mp_grime(c, s, 0.3, 0.35);
    return c;
}

fn mp_step(s: MpS) -> vec3<f32> {
    let base = vec3<f32>(0.47, 0.41, 0.32);
    let n1 = mp_fbm(s.uv * 1.8, 71u);
    let n2 = mp_fbm(s.uv * 10.0, 72u);
    var c = base * (0.84 + 0.28 * n1) * mix(1.0, 0.9 + 0.2 * n2, mp_lod(s.fw, 0.08));
    c = c * (0.92 + 0.16 * mp_h2(vec2<i32>(i32(s.seed), 0), 73u));
    if (s.up > 0.7) {
        let w = (1.0 - smoothstep(0.2, 1.0, abs(s.lc.x) / max(s.hf.x, 0.01))) * (1.0 - smoothstep(0.2, 1.0, abs(s.lc.y) / max(s.hf.y, 0.01)));
        c = c * (1.0 + 0.14 * w);
        // Worn, rounded nosing.
        c = mix(c, c * 1.3, (1.0 - smoothstep(0.0, 0.06, s.edge)) * (0.6 + 0.4 * n1));
    } else {
        c = c * 0.86;
        c = mp_grime(c, s, 0.12, 0.4);
    }
    return c;
}

fn mp_platform(s: MpS) -> vec3<f32> {
    if (s.up > 0.7) {
        var t = mp_slabs(s, 1.2, vec3<f32>(0.5, 0.44, 0.34), 74u);
        return mix(t, t * 1.2, mp_chip(s, 0.04));
    }
    var c = mp_stone_blocks(s, vec2<f32>(0.8, 0.4), vec3<f32>(0.5, 0.38, 0.22));
    c = mix(c, c * 1.22, mp_chip(s, 0.04));
    return mp_grime(c, s, 0.4, 0.5);
}

fn mp_ramp(s: MpS) -> vec3<f32> {
    var c = mp_concrete(s, vec3<f32>(0.3, 0.29, 0.27));
    if (s.face == 3u) {
        let t = select(s.lc.y, s.lc.x, s.hf.x > s.hf.y);
        let g = fract(t / 0.1);
        let d = min(g, 1.0 - g) * 0.1;
        c = c * (1.0 - 0.5 * mp_line(d, 0.01, s.fw));
    }
    return c;
}

fn mp_paving(s: MpS) -> vec3<f32> {
    let rh = 0.5;
    let row = floor(s.uv.y / rh);
    let rs = mp_h2(vec2<i32>(i32(row), 7), 81u);
    let w = 0.55 + 0.35 * rs;
    let x = s.uv.x + rs * 3.7;
    let col = floor(x / w);
    let f = vec2<f32>(x - col * w, s.uv.y - row * rh);
    let d = min(min(f.x, w - f.x), min(f.y, rh - f.y));
    let id = vec2<i32>(i32(col), i32(row));
    let h = mp_h2(id, 82u);
    let base = vec3<f32>(0.36, 0.3, 0.22);
    let tint = mix(1.0, 0.84 + 0.32 * h, mp_lod(s.fw, 0.12));
    var c = base * tint * (0.85 + 0.25 * mp_fbm(s.uv * 1.7, 83u));
    c = c * mix(1.0, 0.84 + 0.16 * smoothstep(0.0, 0.04, d), mp_lod(s.fw, 0.05));
    c = mix(c, vec3<f32>(0.22, 0.19, 0.15), mp_line(d, 0.008, s.fw));
    return c;
}

fn mp_bricks(s: MpS) -> vec3<f32> {
    // Variant 1: larger, warm grey concrete pavers.
    var size = vec2<f32>(0.22, 0.11);
    var base = vec3<f32>(0.25, 0.13, 0.08);
    var spread = 0.26;
    if (s.variant == 1u) {
        size = vec2<f32>(0.4, 0.2);
        base = vec3<f32>(0.21, 0.195, 0.175);
        spread = 0.18;
    }
    let b = mp_bond(s.uv, size);
    let h = mp_h2(b.id, 91u);
    // Per-brick tint fades out once a brick is a few pixels (no sparkle).
    let tint = mix(1.0, 1.0 - 0.5 * spread + spread * h, mp_lod(s.fw, size.y * 0.5));
    var c = base * tint * (0.8 + 0.3 * mp_fbm(s.uv * 0.8, 92u));
    c = mix(c, vec3<f32>(0.3, 0.27, 0.22), mp_line(b.d, 0.005, s.fw));
    // Sand blown over the pavers, thickest in drifts and at sheltered edges.
    let dust = clamp((1.0 - s.ao) * 2.0 + (mp_fbm(s.uv * 0.45 + 2.0, 93u) - 0.45) * 1.6, 0.0, 0.85);
    c = mix(c, vec3<f32>(0.5, 0.4, 0.26) * (0.9 + 0.2 * mp_fbm(s.uv * 5.0, 94u)), dust * 0.75);
    return c;
}

fn mp_trim_stone(s: MpS) -> vec3<f32> {
    let base = vec3<f32>(0.62, 0.56, 0.46);
    let n1 = mp_fbm(s.uv * 2.0, 111u);
    let n2 = mp_fbm(s.uv * 11.0, 112u);
    var c = base * (0.85 + 0.25 * n1) * mix(1.0, 0.9 + 0.2 * n2, mp_lod(s.fw, 0.06));
    let along = select(s.lc.y + s.hf.y, s.lc.x + s.hf.x, s.hf.x >= s.hf.y);
    let j = fract(along / 0.9) * 0.9;
    c = mix(c, vec3<f32>(0.3, 0.27, 0.22), mp_line(min(j, 0.9 - j), 0.004, s.fw));
    c = c * (1.0 - 0.3 * mp_chip(s, 0.03));
    if (s.up < -0.5) {
        c = c * 0.75;
    }
    return c;
}

fn mp_trim_wood(s: MpS) -> vec3<f32> {
    let base = vec3<f32>(0.12, 0.07, 0.035);
    let long_u = s.hf.x >= s.hf.y;
    let a = select(s.lc.y, s.lc.x, long_u);
    let b = select(s.lc.x, s.lc.y, long_u);
    let sd = f32(s.seed & 255u);
    let g = mp_fbm(vec2<f32>(a * 0.8, b * 35.0) + sd, 121u);
    let g2 = mp_fbm(vec2<f32>(a * 3.0, b * 90.0) + sd, 122u);
    var c = base * (0.7 + 0.5 * g) * mix(1.0, 0.85 + 0.3 * g2, mp_lod(s.fw, 0.01));
    // Checks (cracks) along the grain.
    let ck = smoothstep(0.86, 0.9, mp_fbm(vec2<f32>(a * 0.5, b * 60.0) + 5.0, 123u));
    c = c * (1.0 - 0.5 * ck * mp_lod(s.fw, 0.01));
    c = mix(c, c * 1.7 + 0.01, mp_chip(s, 0.02));
    return c;
}

fn mp_paint(surf: u32, s: MpS) -> vec3<f32> {
    switch surf {
        case 2u: {
            return mp_plaster(s);
        }
        case 3u: {
            return mp_sandstone(s);
        }
        case 4u: {
            return mp_concrete(s, vec3<f32>(0.35, 0.32, 0.28));
        }
        case 5u: {
            return mp_concrete(s, vec3<f32>(0.23, 0.21, 0.18));
        }
        case 6u: {
            return mp_wood_crate(s);
        }
        case 7u: {
            return mp_metal_crate(s);
        }
        case 8u: {
            return mp_step(s);
        }
        case 9u: {
            return mp_platform(s);
        }
        case 10u: {
            return mp_ramp(s);
        }
        case 11u: {
            return mp_paving(s);
        }
        case 12u: {
            return mp_bricks(s);
        }
        case 13u: {
            return mp_trim_stone(s);
        }
        case 14u: {
            return mp_trim_wood(s);
        }
        case 18u: {
            return mp_ground(s);
        }
        default: {
            return vec3<f32>(0.5);
        }
    }
}

fn mp_world(p: vec3<f32>, n: vec3<f32>, fw: f32) -> vec3<f32> {
    let hit = mp_find(p, n, false);
    if (hit.x < 0) {
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
    // The painters' grime, streaks and wear read the resolved surface's tuning.
    s.surf = surf;
    var c = mp_paint(surf, s) * mp_tune(surf, 0u).rgb;
    // Shaded, covered spots are lit by warm bounce from stone and earth, not
    // by the blue sky the engine's flat ambient assumes: warm them as AO
    // drops (by the live AO warmth). Upward faces get the strongest
    // correction: the engine's sky fill is zenith-blue for them even under a
    // roof.
    let h = kit_data[7];
    let ao = clamp(s.ao, 0.0, 1.0);
    let warm = mix(vec3<f32>(1.1, 1.0, 0.84), vec3<f32>(1.2, 1.0, 0.7), clamp(s.up, 0.0, 1.0));
    c = c * mix(vec3<f32>(1.0), warm, clamp((1.0 - ao) * 1.6 * h.w, 0.0, 1.0));
    return c * pow(ao, h.z);
}

// ---- stencils -------------------------------------------------------------------

fn mp_seg(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = clamp(dot(pa, ba) / dot(ba, ba), 0.0, 1.0);
    return length(pa - ba * h);
}

fn mp_sdf_a(q: vec2<f32>) -> f32 {
    var d = mp_seg(q, vec2<f32>(-0.62, -0.9), vec2<f32>(0.0, 0.9));
    d = min(d, mp_seg(q, vec2<f32>(0.62, -0.9), vec2<f32>(0.0, 0.9)));
    d = min(d, mp_seg(q, vec2<f32>(-0.33, -0.25), vec2<f32>(0.33, -0.25)));
    return d;
}

fn mp_sdf_b(q: vec2<f32>) -> f32 {
    var d = mp_seg(q, vec2<f32>(-0.5, -0.9), vec2<f32>(-0.5, 0.9));
    d = min(d, mp_seg(q, vec2<f32>(-0.5, 0.9), vec2<f32>(0.05, 0.9)));
    d = min(d, mp_seg(q, vec2<f32>(-0.5, 0.0), vec2<f32>(0.1, 0.0)));
    d = min(d, mp_seg(q, vec2<f32>(-0.5, -0.9), vec2<f32>(0.1, -0.9)));
    let c1 = vec2<f32>(0.05, 0.45);
    if (q.x > c1.x) {
        d = min(d, abs(length(q - c1) - 0.45));
    }
    let c2 = vec2<f32>(0.1, -0.45);
    if (q.x > c2.x) {
        d = min(d, abs(length(q - c2) - 0.45));
    }
    return d;
}

fn mp_sdf_arrow(q: vec2<f32>) -> f32 {
    var d = mp_seg(q, vec2<f32>(-0.85, 0.0), vec2<f32>(0.55, 0.0));
    d = min(d, mp_seg(q, vec2<f32>(0.85, 0.0), vec2<f32>(0.3, 0.6)));
    d = min(d, mp_seg(q, vec2<f32>(0.85, 0.0), vec2<f32>(0.3, -0.6)));
    return d;
}

fn mp_decal(p: vec3<f32>, n: vec3<f32>, fw: f32) -> vec4<f32> {
    let hit = mp_find(p, n, true);
    if (hit.x < 0) {
        return vec4<f32>(0.0);
    }
    let s = mp_ctx(u32(hit.x), u32(hit.y), p, n, fw);
    // Only the broad faces carry paint (not the 1 cm sides).
    if (min(s.hf.x, s.hf.y) < 0.02 || s.surf < 15u || s.surf > 17u) {
        return vec4<f32>(0.0);
    }
    var d: f32;
    if (s.surf == 17u) {
        let q = vec2<f32>(s.lc.x / s.hf.x * select(1.0, -1.0, (s.variant & 1u) == 1u), s.lc.y / s.hf.y);
        d = mp_sdf_arrow(q);
    } else {
        let q = vec2<f32>(s.lc.x * s.flip / s.hf.y, s.lc.y / s.hf.y);
        d = select(mp_sdf_b(q), mp_sdf_a(q), s.surf == 15u);
    }
    let px = fw / s.hf.y;
    let rough = (mp_vnoise(s.uv * 30.0, 101u) - 0.5) * 0.06 + (mp_vnoise(s.uv * 90.0, 102u) - 0.5) * 0.035;
    let width = 0.13;
    let m = 1.0 - smoothstep(width - px, width + px, d + rough);
    // Overspray specks just outside the stroke.
    let near = 1.0 - smoothstep(width, width + 0.16, d);
    let sp = mp_h2(vec2<i32>(floor(s.uv * 110.0)), 103u);
    let dots = select(0.0, 1.0, sp > 1.0 - 0.22 * near) * mp_lod(fw, 0.012);
    if (max(m, dots) < 0.5) {
        return vec4<f32>(0.0);
    }
    let ci = s.variant >> 1u;
    var col = vec3<f32>(0.035, 0.033, 0.03);
    if (ci == 1u) {
        col = vec3<f32>(0.32, 0.03, 0.018);
    } else if (ci == 2u) {
        col = vec3<f32>(0.66, 0.63, 0.56);
    } else if (ci == 3u) {
        col = vec3<f32>(0.6, 0.38, 0.03);
    }
    // Worn, sun-faded paint.
    let fade = mp_fbm(s.uv * 2.2 + 11.0, 104u);
    col = mix(col, col * 0.6 + vec3<f32>(0.12, 0.1, 0.08), smoothstep(0.5, 0.85, fade) * 0.7 * mp_tune(s.surf, 0u).w);
    col = col * mp_tune(s.surf, 0u).rgb;
    // The surface's own occlusion under the paint.
    var ao = 1.0;
    let under = mp_find(p - n * 0.01, n, false);
    if (under.x >= 0) {
        ao = mp_ctx(u32(under.x), u32(under.y), p - n * 0.01, n, fw).ao;
    }
    return vec4<f32>(col * pow(clamp(ao, 0.0, 1.0), kit_data[7].z), 1.0);
}
