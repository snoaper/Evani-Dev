// evani.shader.rc.cloth
// Cloth (recipe 10; batch 3 build plan 3.3, B3-P10b; batch 4 B4-P3a and
// B4-P3b). Written fresh for Evani.Dev: weaves, knits, felt and coloured-yarn
// patterns in uv space, and the cloth's own sheen and fuzz after lighting
// (rc_cloth_post), so a game that lists cloth needs no layers family.
//   numbers: na = weave scale (threads per uv; plain, knit and open draw 8
//            threads per step, as the layers' weave tile did), sheen, sheen
//            rough, fuzz; nb = rough, motif scale (1-32 per uv), motif
//            contrast, sett scale (1-64 repeats per uv); nc = sett kind
//            (0-7), slub
//   colours: A colour, B sheen (a twill's weft is A toward B; the thin line
//            of a plaid or tartan), C motif, the sett's stripes, slub flecks
//   variants: 0 plain (threads over and under), 1 twill (2/1; 2/2 under a
//             houndstooth sett), 2 knit (V stitches), 3 pile (carpet tufts),
//             4 satin, 5 flatweave (a rug's weft rows), 6 felt (matted
//             fibres, no weave), 7 open (a loose plain weave: the cut-out
//             cuts its gaps when the game lists it, else they draw dark),
//             8 herringbone (a 2/1 twill whose diagonal flips every 12
//             threads)
//   extras: sheen (bit 0), weave relief (1: every weave's threads, tufts or
//           fibres as normals), fuzz rim (2: light wraps past the edge,
//           tinted by the cloth), motif (3: a pile's medallions, a
//           flatweave's kilim diamonds, in C), sett (4: coloured yarns, kind
//           0 chambray, 1 gingham, 2 pinstripe, 3 ticking, 4 plaid, 5 tartan,
//           6 houndstooth, 7 windowpane; the weave decides whether warp or
//           weft shows), slubs (5: thick and thin yarn, flecks in C; on a
//           knit a heather of short lighter and darker fibres)
// Levels (q, the detail group): lite draws what low draws (every tier keeps
// the pattern): threads, stripes, slubs and motifs, each faded to its
// average once it is under 1.5 px (a far tartan is its mixed colour); medium
// adds the relief within 25 m; high and ultra as medium. The sheen and fuzz
// draw on every level.

// A herringbone's thread position across u: mirrored every 12 threads.
fn rc_cloth_x(b: RecBlock, p: vec2<f32>) -> f32 {
    return select(p.x, -p.x, b.variant == 8u && fract(floor(p.x / 12.0) * 0.5) > 0.25);
}

// The weave's height at thread position p (threads per uv times uv): the
// relief by finite difference, and (0..1) the shading of plain, knit and felt.
fn rc_cloth_h(b: RecBlock, p: vec2<f32>) -> f32 {
    let v = b.variant;
    let ip = floor(p);
    let fp = p - ip;
    if (v == 1u || v == 8u) {
        return sin(2.0944 * (rc_cloth_x(b, p) + p.y));
    } else if (v == 3u) {
        return 1.0 - rc_voronoi2(p * 0.5, b.seed + 1u, 0.9).x;
    } else if (v == 2u) {
        // Each stitch a V: two round legs leaning in to the cell's foot.
        let x = abs(fp.x - 0.5) - 0.3 * (1.1 - fp.y);
        return sqrt(max(1.0 - x * x * 22.0, 0.0)) * (0.85 + 0.3 * rc_u01(rc_hash2(vec2<i32>(ip), b.seed)));
    } else if (v == 6u) {
        // Felt: fibres matted at two crossing angles over a cloudy density.
        let a = rc_vnoise2(vec2<f32>(p.x + p.y, (p.y - p.x) * 5.0) * 0.4, b.seed);
        return 0.55 * max(a, rc_vnoise2(vec2<f32>(p.x * 4.0 - p.y, p.x + p.y) * 0.4, b.seed + 3u)) + 0.45 * rc_fbm2(p * 0.05, b.seed, 2u);
    } else if (v == 0u || v == 7u) {
        // Plain: warp on top where ix + iy is even, the weft where odd; a
        // round thread that dips at each crossing.
        let w = fract((ip.x + ip.y) * 0.5) > 0.25;
        let a = 2.0 * select(fp.x, fp.y, w) - 1.0;
        return sqrt(max(1.0 - a * a, 0.0)) * (0.6 + 0.4 * sin(3.14159 * select(fp.y, fp.x, w)));
    }
    return sin(3.14159 * fract(p.y * 0.34)) - 0.15 * cos(6.28318 * p.x);
}

// One band of the sett at folded position t: 1 inside [a, e], faded to its
// share once it is under 1.5 px (s: folded repeats per uv).
fn rc_cloth_bd(f: MatFrame, t: f32, a: f32, e: f32, s: f32) -> f32 {
    return rc_fade(step(a, t) * step(t, e), max(min(e, 1.0) - a, 0.0), rc_aa(f, (e - a) / s));
}

// A warp or weft thread's colour at u (sett repeats): a mirrored sequence
// from the repeat's centre (t 0) to its edge (t 1) of a band in C, a dark
// band (C darkened) and a thin line in B, per kind; houndstooth's kind is
// gingham's, by whole threads.
fn rc_cloth_st(b: RecBlock, f: MatFrame, u: f32, k: u32, s: f32) -> vec3<f32> {
    // Per kind, bytes x/255 (255: no such band): C's edge, the dark band's
    // two edges, the line's start.
    var tb = array<u32, 8>(0xffffffffu, 0xffffff80u, 0xffffff1au, 0xffa89933u, 0xdbb2805cu, 0xd6a86b4cu, 0xffffff80u, 0xffffff14u);
    let e = unpack4x8unorm(tb[k]);
    let t = abs(fract(u) - 0.5) * 2.0;
    let s2 = 2.0 * s;
    var c = mix(b.col_a, b.col_c, rc_cloth_bd(f, t, 0.0, e.x, s2));
    c = mix(c, b.col_c * 0.15, rc_cloth_bd(f, t, e.y, e.z, s2));
    return mix(c, b.col_b, rc_cloth_bd(f, t, e.w, e.w + 0.04, s2));
}

// The motif's colour weight at uv: x how much of C, y how much darker A (the
// kilim's dark rings, the medallion's centre), faded to their averages by k.
fn rc_cloth_m(b: RecBlock, uv: vec2<f32>, ws: f32, k: f32) -> vec2<f32> {
    let ms = rc_range(b.nb.y, 1.0, 32.0);
    let p = uv * ms;
    let cell = floor(p);
    if (b.variant == 5u) {
        // Kilim: diamonds stepped to the weft rows, three rings each.
        let rows = max(floor(ws / (3.0 * ms)), 4.0);
        let qy = (floor((p.y - cell.y) * rows) + 0.5) / rows;
        let d = abs(p.x - cell.x - 0.5) + abs(qy - 0.5);
        let ring = floor(d * 6.0);
        let cc = select(0.0, 1.0, ring == 0.0 || ring == 2.0);
        let dk = select(0.0, 1.0, ring == 1.0);
        return vec2<f32>(rc_fade(cc, 0.3, k), rc_fade(dk, 0.17, k));
    }
    let m = rc_lay_motif((p - cell) * 2.0 - vec2<f32>(1.0), b.seed, 0.08, rc_u01(rc_hash2(vec2<i32>(cell), b.seed)));
    let petal = 1.0 - smoothstep(0.0, 0.06, m.x);
    let corner = 1.0 - smoothstep(0.0, 0.06, m.z);
    let eye = 1.0 - smoothstep(0.0, 0.05, m.y);
    return vec2<f32>(rc_fade(max(petal, corner), 0.35, k), rc_fade(eye, 0.05, k));
}

fn rc_cloth(b: RecBlock, f: MatFrame, i: MatIn, q: u32) -> MatOut {
    let v = b.variant;
    let ws0 = rc_range(b.na.x, 1.0, 400.0);
    let fine = v == 0u || v == 2u || v == 7u;
    let ws = ws0 * select(1.0, 8.0, fine);
    let p = i.uv * ws;
    let ip = floor(p);
    let fp = p - ip;
    // Fine threads fade from 6.7 px down to 3.3 px, as the layers' weave
    // tile did (so plain cloth and knits read smooth at a normal distance);
    // the others from 1.5 px.
    let kt = rc_aa(f, select(1.0, 0.224, fine) / ws);
    let tw = v == 1u || v == 8u;
    let sk = u32(b.nc.x * 7.0 + 0.5);
    let sett = rc_extra(b, RC_X_CLOTH, 4u);
    let hs = sett && sk == 6u;
    // Warp and weft colours: the sett's yarns, else A (a twill's weft toward
    // B); sm is how much of the warp a cloth without crossing threads shows.
    var wa = b.col_a;
    var wf = select(b.col_a, mix(b.col_a, b.col_b, 0.8), tw);
    var sm = 0.5;
    if (sett) {
        let s = rc_range(b.nb.w, 1.0, 64.0);
        let st = sk == 0u || sk == 2u || sk == 3u;
        // Houndstooth: four whole threads of A, four of C, both ways.
        let u = select(i.uv * s, (ip + vec2<f32>(0.5)) * 0.125, hs);
        let s8 = select(s, ws * 0.125, hs);
        wa = rc_cloth_st(b, f, u.x, sk, s8);
        wf = select(rc_cloth_st(b, f, u.y, sk, s8), b.col_a, st);
        sm = select(0.5, 1.0, st);
    }
    var c = mix(wf, wa, sm);
    let h = rc_cloth_h(b, p);
    if (tw || v == 0u || v == 7u) {
        // Which thread lies on top (1 the weft) and the weft's far share: plain
        // and open alternate; a twill shows its weft every third thread up the
        // diagonal, a houndstooth's 2/2 twill every other pair.
        var w = fract((ip.x + ip.y) * 0.5) * 2.0;
        var sw = 0.5;
        if (tw) {
            let t = select(0.6667, 0.5, hs);
            w = step(t, fract(select(rc_cloth_x(b, p) + p.y, ip.x + ip.y, hs) / select(3.0, 4.0, hs)));
            sw = 1.0 - t;
        }
        // Round threads: a twill's across each thread, plain's by its height.
        let sh = select(vec2<f32>(0.6 + 0.6 * h), vec2<f32>(0.78, 0.8) + vec2<f32>(0.22, 0.2) * sin(3.14159 * fp), tw);
        c = mix(mix(wa, wf, sw) * select(1.0, 0.9, tw), mix(wa * sh.x, wf * sh.y, w), kt);
        if (tw) {
            // As the threads fade, a broad wale every twelve threads up the
            // diagonal keeps a twill reading as twill while it spans 3 px
            // (critic round 5: denim was a smooth blue pouf on a phone).
            c = c * (1.0 + 0.12 * (1.0 - kt) * rc_aa(f, 6.0 / ws) * sin(0.5236 * (rc_cloth_x(b, p) + p.y)));
        }
        if (v == 7u) {
            // The open weave's gaps, where neither thread passes (each thread
            // half its period wide, as the cut-out cuts them).
            let g = smoothstep(0.22, 0.28, abs(fp.x - 0.5)) * smoothstep(0.22, 0.28, abs(fp.y - 0.5));
            c = mix(c, b.col_a * 0.3, rc_fade(g, 0.25, kt));
        }
    } else if (v == 2u || v == 6u) {
        // Knit stitches in strong light and shade; felt's fibres softly.
        c = c * rc_fade(select(0.52 + 0.8 * h, 0.78 + 0.4 * h, v == 6u), 1.0, kt);
    } else if (v == 3u) {
        // Pile: each tuft its own shade, darker down between them, and the
        // broad light and dark of the pile lying different ways.
        let t = rc_voronoi2(p * 0.5, b.seed + 1u, 0.9);
        let kp = rc_aa(f, 2.0 / ws);
        c = c * rc_fade((0.86 + 0.24 * t.w) * (1.0 - 0.35 * smoothstep(0.35, 0.75, t.x)), 0.9, kp);
        c = c * (0.93 + 0.14 * rc_fbm2(i.uv * 3.0, b.seed + 2u, 3u));
    } else if (v == 5u) {
        // Flatweave: weft rows three threads deep, each thread's wave.
        let r = fract(p.y * 0.34);
        c = c * rc_fade(0.84 + 0.16 * sin(3.14159 * r) + 0.04 * cos(6.28318 * p.x), 0.94, kt);
        c = c * (0.95 + 0.1 * rc_u01(rc_hash2(vec2<i32>(0, i32(floor(p.y * 0.34))), b.seed + 3u)));
    }
    if (rc_extra(b, RC_X_CLOTH, 5u) && v == 2u) {
        // A knit's slubs are a heather: short fibres of lighter and darker
        // yarn laid along the courses (smooth flecks a few loops long and
        // under one high, at two sizes, each its own shade; round 4: no
        // hashed cells, which read as blocks close up), read as fine flecks
        // a few metres off, never clouds or streaks across the garment; a
        // faint broad drift under them. Round 5: the fibres shorter (at most
        // twice as long as high; round 4's ran nearly four times and read
        // as horizontal stripes) over a fine even fleck.
        let hp = i.uv * ws0 * vec2<f32>(0.75, 1.5);
        let n1 = rc_vnoise2(hp, b.seed + 7u) - 0.5;
        let n2 = rc_vnoise2(i.uv * ws0 * vec2<f32>(1.5, 2.6), b.seed + 8u) - 0.5;
        let n3 = rc_vnoise2(i.uv * ws0 * 3.4, b.seed + 9u) - 0.5;
        // Most fibres near the mean, a few clearly lighter or darker.
        let fib = 0.5 * n1 * (0.8 + 4.0 * abs(n1)) + 0.35 * n2 * (0.8 + 4.0 * abs(n2)) + 0.3 * n3;
        let drift = rc_vnoise2(i.uv * ws0 * 0.08, b.seed + 10u) - 0.5;
        let k1 = rc_aa(f, 0.67 / ws0);
        c = c * (1.0 + drift * 0.25 * b.nc.y) * rc_fade(1.0 + fib * 0.7 * b.nc.y, 1.0, k1);
    } else if (rc_extra(b, RC_X_CLOTH, 5u)) {
        // Slubs: thick and thin stretches of yarn along the weft, and flecks
        // of C, both by the weave step (not the fine threads).
        let sl = b.nc.y;
        // Only the noise's peaks thicken a stretch of yarn: sparse slubs,
        // not a grain (mean about 1).
        let n = smoothstep(0.62, 0.9, rc_vnoise2(i.uv * ws0 * vec2<f32>(0.25, 3.0), b.seed + 5u));
        c = c * rc_fade(1.0 + (n - 0.15) * 0.6 * sl, 1.0, rc_aa(f, 0.33 / ws0));
        // A fleck: a short soft dash in some cells of a grid five times
        // wider than tall.
        let fc = i.uv * ws0 * vec2<f32>(0.6, 3.0);
        let fd = length((fract(fc) - vec2<f32>(0.5)) * vec2<f32>(1.0, 0.3));
        let fl = step(rc_u01(rc_hash2(vec2<i32>(floor(fc)), b.seed + 6u)), 0.08 * sl) * (1.0 - smoothstep(0.12, 0.2, fd));
        c = mix(c, b.col_c, rc_fade(fl, 0.02 * sl, rc_aa(f, 0.05 / ws0)));
    }
    if (rc_extra(b, RC_X_CLOTH, 3u)) {
        let m = rc_cloth_m(b, i.uv, ws, rc_aa(f, 0.04 / rc_range(b.nb.y, 1.0, 32.0)));
        let lum = dot(c, vec3<f32>(0.3333)) / max(dot(b.col_a, vec3<f32>(0.3333)), 1e-3);
        c = mix(c, mix(c * 0.35, b.col_c * lum, m.x), max(m.x, m.y) * b.nb.z);
    }
    var o = rc_out(i, c, b.nb.x, 0.0);
    // The sheen and fuzz for rc_cloth_post: B times the sheen number, and in
    // w (below -1) the sheen roughness and the fuzz in 31sts.
    o.sheen = vec4<f32>(select(vec3<f32>(0.0), b.col_b * b.na.y, rc_extra(b, RC_X_CLOTH, 0u)), -1.0 - 0.99 * b.na.z - 2.0 * round(select(0.0, b.na.w, rc_extra(b, RC_X_CLOTH, 2u)) * 31.0));
    let near = rc_near(f) * kt;
    if (q >= 2u && near > 0.0 && v != 4u && rc_extra(b, RC_X_CLOTH, 1u)) {
        let e = 0.25;
        let d = vec2<f32>(rc_cloth_h(b, p + vec2<f32>(e, 0.0)), rc_cloth_h(b, p + vec2<f32>(0.0, e))) - vec2<f32>(h);
        o.n = rc_bump(i.n, rc_tuv(f, i.n), d * (0.15 * near / e));
    }
    return o;
}

// After lighting (rc_post): the sheen and the fuzz (B4-P3b). Filament's cloth
// model as the idea (Estevez and Kulla's Charlie sheen, Neubelt and
// Pettineo's visibility, the wrapped diffuse with a subsurface colour) and
// for the light of the surroundings a curve fitted to three.js's sheen
// albedo (within 0.1 of it for rough 0.15 to 0.9); code written fresh. The base dims by the sheen's energy;
// material.recipe.cloth.sheen_strength (frame word 10, bits 24-31) scales the
// sheen.
fn rc_cloth_post(c: vec3<f32>, o: MatOut, v: vec3<f32>, l: vec3<f32>, sun: vec3<f32>, vis: f32) -> vec3<f32> {
    if (o.sheen.w > -0.5) {
        return c;
    }
    let t = -1.0 - o.sheen.w;
    let fz = floor(t * 0.5);
    let r = max(t - 2.0 * fz, 0.07);
    let n = o.n;
    let ndl = dot(n, l);
    let x = max(ndl, 0.0);
    let ndv = max(dot(n, v), 1e-3);
    let ndh = dot(n, normalize(l + v));
    let d = o.base.rgb;
    // The fuzz: the look's light ramp gives way to a wrap tinted by the cloth.
    let k = c + d * sun * (vis * step(0.5, fz)) * (saturate((ndl + 0.5) / 2.25) * saturate(d * (fz / 31.0) + vec3<f32>(ndl)) - vec3<f32>(slot_toon_ramp(ndl)));
    let s = o.sheen.rgb * (f32(((rc_frame.misc.z >> 24u) + 100u) & 255u) * 0.01);
    let ri = 1.0 / (r * r);
    let dc = (2.0 + ri) * pow(max(1.0 - ndh * ndh, 0.0078125), ri * 0.5) * 0.15915;
    let vn = min(1.0 / (4.0 * (x + ndv - x * ndv) + 1e-4), 1.0);
    return k * (1.0 - 0.157 * min(max(s.r, max(s.g, s.b)), 1.0)) + s * (sun * (dc * vn * x * 3.14159 * vis) + lit_hemi(n) * ((0.2 + 0.25 / r) * exp(-(1.25 + 1.35 / r) * ndv) * o.occ));
}
