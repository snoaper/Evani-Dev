// evani.shader.rc.stone
// Stone (recipe 6, batch 1 build plan section 3, package P4). Written fresh
// for Evani.Dev. World space (plan 2.3): one dominant-axis projection up to
// Medium, two blended from High, so slopes show no hard seam; strata follow
// world height.
//   numbers: na = rough, vein amount, vein scale (1-50 per m), grain scale
//            (1-200 per m); nb = layering, glints, edge translucency, pits;
//            nc = tile size (.1-2 m), chequer (batch 3); crystalline (9)
//            reads 1, 2, 4, 6, 7 as its dark share (0-.5), crystal size
//            spread (0-.5), buff feldspar (0-.8), quartz (0-.6) and the
//            variation inside each crystal (0-2)
//   colours: A colour 1, B colour 2, C vein (and ore, and the clastic matrix)
//   variants: 0 granular, 1 veined, 2 layered, 3 ore; batch 2: 4 porous,
//             5 ropy, 6 clastic, 7 flowstone (the vein scale sets their
//             feature size, `pits` how many holes or clasts); batch 3:
//             8 terrazzo (chips of colours B and C set in a cement of colour
//             A, ground flat; chip size from the grain scale, how many from
//             the vein amount); 9 crystalline (granite: 3D crystals of four
//             minerals, no projection, so curved stone shows no seam);
//             10 bedded (sandstone: uneven, tilted beds of their own tones,
//             iron lines, sand grains and pits); 11 water-worn (pebbles:
//             3D specks, straight veins, fine pits)
//   extras: parallax (bit 0), glints (bit 1), translucency (bit 2), tiles
//           (bit 3: square slabs from the layout helper (its basket with one
//           unit a block, the chequer), batch 3 B3-P1, each its own cut of
//           the stone; with chequer, every other slab takes C as its colour,
//           veined in A: black and white marble)
// Levels (q, the detail group): lite draws what low draws
// (every tier keeps the pattern; before critic round 1 lite was a flat two-colour noise); low draws
// the mottle, grains, veins, strata and ore, anti-aliased; medium adds grain
// normals within 25 m and the edge-light rim; high adds glints, the blended
// projection and 2-step parallax within 8 m; ultra parallaxes everywhere.
// Tiles: lite draws the chequer and the joints flat; low adds each slab's own
// cut and tone; medium a bevel at the joints.

// The grain cells at plane point p (x F1 .. w the cell's id).
fn rc_stone_g(b: RecBlock, p: vec2f) -> vec4f {
    return rc_voronoi2(p * rc_range(b.na.w, 1., 200.), b.seed, 1.);
}

// The batch 2 relief at plane point p: x the height (0..1), y a cell id.
// Porous: holes in a cellular noise; ropy: stretched ridges that fold across
// the flow; clastic: domed pebbles; flowstone: vertical drapery streaks.
fn rc_stone_r(b: RecBlock, p: vec2f) -> vec2f {
    let vs = rc_range(b.na.z, 1., 50.);
    let v = b.variant;
    if (v == 5u) {
        return vec2f(.5 + .5 * sin(6.2832 * (p.y * vs * .8 + p.x * vs * .15 + rc_fbm2(p * vs * .3, b.seed + 12u, 2u) * 2.5)), 0.);
    }
    if (v == 7u) {
        return vec2f(rc_fbm2(vec2f(p.x * vs * 2., p.y * vs * .2), b.seed + 13u, 3u), 0.);
    }
    let c = rc_voronoi2(p * vs, b.seed + 11u, 1.);
    if (v == 6u) {
        let t = c.x / .45;
        return vec2f(max(1. - t * t, 0.) * step(c.w, .35 + .6 * b.nb.w), c.w);
    }
    let rad = .12 + .2 * fract(c.w * 7.);
    return vec2f(1. - (1. - smoothstep(.6 * rad, rad, c.x)) * step(c.w, b.nb.w), c.w);
}

// The stone's colour at plane point p (y the world height, for strata), kg
// and kv how much grain and vein detail survive the anti-alias fade. w is
// the ore mask (metallic flecks).
fn rc_stone_c(b: RecBlock, p: vec2f, y: f32, kg: f32, kv: f32) -> vec4f {
    if (b.variant == 8u) {
        return vec4f(rc_stone_tz(b, p, kg), 0.);
    }
    if (b.variant == 10u) {
        return vec4f(rc_stone_bed(b, p, y, kg, kv), 0.);
    }
    let m = rc_fbm2(p * 1.5, b.seed, 3u);
    var c = mix(b.col_a, b.col_b, smoothstep(.3, .7, m));
    // Grains: each cell a lighter or darker shade of A or B.
    let g = rc_stone_g(b, p);
    let gc = mix(b.col_a, b.col_b, step(.55, g.w)) * (.7 + .6 * fract(g.w * 7.));
    c = mix(c, gc, select(.15, .7, b.variant == 0u || b.variant == 3u) * kg);
    // Strata: bands along world height, warped by the mottle.
    let lay = select(b.nb.x, max(b.nb.x, .5), b.variant == 2u);
    let vs = rc_range(b.na.z, 1., 50.);
    let s = smoothstep(.2, .8, fract(y * vs * .5 + m * .8));
    c = mix(c, mix(b.col_a, b.col_b, s) * (.85 + .3 * m), lay * kv);
    // Veins: thin ridges of a warped noise, wider with the amount.
    let r = 1. - abs(rc_fbm2(p * vs * .25 + vec2f(m * 2.), b.seed + 7u, 4u) * 2. - 1.);
    let vein = smoothstep(.98 - .1 * b.na.y, 1., r) * step(.001, b.na.y) * kv;
    c = mix(c, b.col_c, vein);
    if (b.variant >= 4u) {
        // Batch 2 variants, coloured by their relief (kv keeps its detail).
        let x = rc_stone_r(b, p);
        let hv = rc_fade(x.x, .6, kv);
        if (b.variant == 4u) {
            c = c * (.35 + .65 * hv);
        } else if (b.variant == 5u) {
            c = c * (.55 + .7 * hv);
        } else if (b.variant == 6u) {
            c = mix(b.col_c * (.8 + .4 * m), mix(b.col_a, b.col_b, fract(x.y * 5.)) * (.6 + .6 * hv), rc_fade(smoothstep(0., .1, x.x), .5, kv));
        } else {
            c = mix(c, mix(b.col_a, b.col_c, hv), .6);
        }
    }
    // Ore: some grains are metal flecks in colour C.
    let ore = select(0., step(.85, g.w) * kg, b.variant == 3u);
    return vec4f(mix(c, b.col_c, ore), ore);
}

// Bedded (variant 10, procedural realism round 6: the sandstone's strata
// were evenly spaced, level and alike): beds of uneven thickness (about half
// either way), tilted a degree or two and gently waved, each bed its own
// tone (some more ochre), a few thin rusty iron lines, fine sand grains and
// a sparse scatter of small pits.
fn rc_stone_bed(b: RecBlock, p: vec2f, y: f32, kg: f32, kv: f32) -> vec3f {
    let m = rc_fbm2(p * 1.5, b.seed, 3u);
    let vs = rc_range(b.na.z, 1., 50.);
    let bed = (y + p.x * .05 + p.y * .02 + (m - .5) * .05) * vs * .5;
    let bu = bed + .9 * (rc_vnoise2(vec2f(bed * .37, .5), b.seed + 61u) - .5);
    let bf = fract(bu);
    let bh = rc_u01(rc_hash2(vec2i(i32(floor(bu)), 3), b.seed + 62u));
    var c = mix(b.col_a, b.col_b, .2 + .5 * bh) * (.92 + .16 * m);
    c = mix(c, c * vec3f(1.07, .96, .8), step(.62, fract(bh * 7.)));
    c = c * (1. - .07 * smoothstep(.82, 1., bf) * kv);
    let iron = (1. - smoothstep(0., .03, abs(bf - .5))) * step(.84, fract(bh * 13.)) * kv;
    c = mix(c, b.col_a * vec3f(.55, .36, .22), iron * .75);
    // Laminae (round 9): thin, wavy orange streaks a centimetre or two
    // apart between the beds' edges, broken along their length.
    let lb = bu * 7. + (m - .5) * 1.6;
    let lam = (1. - smoothstep(0., .14, abs(fract(lb) - .5))) * smoothstep(.35, .65, rc_vnoise2(vec2f(p.x * 5., lb * .6), b.seed + 65u)) * kv;
    c = mix(c, c * vec3f(1.1, .84, .6), lam * .5);
    let g = rc_stone_g(b, p);
    c = c * (1. + (fract(g.w * 7.) - .5) * .22 * kg);
    let pc = rc_voronoi2(p * 45., b.seed + 63u, 1.);
    let pit = (1. - smoothstep(.08, .16, pc.x)) * step(pc.w, .08 + .12 * bh) * kv;
    return rc_foot(c * (1. - .45 * pit), y, m);
}

// Terrazzo at plane point p: big chips (cells of the grain, their corners
// ground off) and a scatter of small ones, each chip B or C in its own shade,
// in a lightly mottled cement A. Faded by kg to the cement and chips' mix.
fn rc_stone_tz(b: RecBlock, p: vec2f, kg: f32) -> vec3f {
    let gs = rc_range(b.na.w, 1., 200.);
    var c = b.col_a * (.94 + .12 * rc_vnoise2(p * gs * .2, b.seed));
    let avg = mix(b.col_b, b.col_c, .4);
    for (var k = 0u; k < 2u; k = k + 1u) {
        let s = select(1., 2.7, k == 1u);
        let g = rc_voronoi2(p * gs * s, b.seed + 5u + k, .9);
        let on = smoothstep(.06, .12, g.z) * step(g.w, select(.25 + .6 * b.na.y, .4 * b.na.y, k == 1u));
        let cc = mix(b.col_b, b.col_c, step(.6, fract(g.w * 7.))) * (.7 + .6 * fract(g.w * 13.));
        c = mix(c, mix(avg, cc, kg), rc_fade(on, .45 * b.na.y, kg));
    }
    return c;
}

// The colour at p with the tiles extra: the slab's own cut of the stone (its
// pattern moved by its id), dark slabs of the chequer veined in A, a fine
// dark joint (faded to its average by kt). w is the ore mask.
fn rc_stone_t(b: RecBlock, p: vec2f, y: f32, kg: f32, kv: f32, kt: f32) -> vec4f {
    if (!rc_extra(b, RC_X_STONE, 3u)) {
        return rc_stone_c(b, p, y, kg, kv);
    }
    let ts = rc_range(b.nc.x, .1, 2.);
    let t = rc_lay_basket(p, ts, 1., b.seed + 13u);
    let k = t.k * b.nc.y;
    var bb = b;
    bb.col_a = mix(b.col_a, b.col_c, k);
    bb.col_b = mix(b.col_b, b.col_c * .7, k);
    bb.col_c = mix(b.col_c, b.col_a, k);
    let s = rc_stone_c(bb, p + vec2f(t.id * 41., t.id * 17.), y, kg, kv);
    let j = rc_fade(1. - smoothstep(.0008, .0016, t.e), .003 / ts, kt);
    return vec4f(mix(s.rgb * (.94 + .12 * t.id), (b.col_a + b.col_c) * .3, j), s.w);
}

// Crystalline (variant 9, the granite ball's own; live granite session
// rounds A-B: r9 read as an even pepper of tiny dots on a soft grey): a 3D
// crystal field over the world position, so a curved stone shows no seam.
// Each crystal has its own size (its distance over a weight 1 +- wv: mixed
// sizes, curved boundaries) and the caller warps the point first, so
// boundaries are ragged and interlock instead of running straight. One hash
// per cell gives its seed point (10 bits an axis). f1 the nearest crystal's
// weighted distance (0 at its seed), gap the margin to the second (0 on a
// boundary), a and b the two crystals' ids, c the nearest crystal's seed.
struct RcXtal {
    f1: f32,
    gap: f32,
    a: u32,
    b: u32,
    c: vec3f,
};

fn rc_stone_x3(p: vec3f, seed: u32, wv: f32) -> RcXtal {
    let fl = floor(p);
    let fr = p - fl;
    var o = RcXtal(8., 0., 0u, 0u, fl);
    var f2 = 8.;
    for (var z = -1; z <= 1; z = z + 1) {
        for (var y = -1; y <= 1; y = y + 1) {
            for (var x = -1; x <= 1; x = x + 1) {
                let off = vec3f(f32(x), f32(y), f32(z));
                let h = rc_hash3(vec3i(fl + off), seed);
                let pt = off + vec3f((vec3u(h, h >> 10u, h >> 20u) & vec3u(1023u))) / 1024.;
                let d = length(pt - fr) / (1. + wv * (2. * rc_u01(rc_pcg(h)) - 1.));
                if (d < o.f1) {
                    f2 = o.f1;
                    o.b = o.a;
                    o.f1 = d;
                    o.a = h;
                    o.c = fl + pt;
                } else if (d < f2) {
                    f2 = d;
                    o.b = h;
                }
            }
        }
    }
    o.gap = f2 - o.f1;
    return o;
}

// The granite's minerals' shares at crystal point c: x black biotite and
// hornblende, y smoky quartz, z buff K-feldspar, the rest white
// plagioclase; each share swells and shrinks over a few crystals (its own
// noise), so neighbours often match and the minerals gather in patches and
// clusters, as in real granite, instead of an even pepper.
fn rc_stone_gp(b: RecBlock, c: vec3f) -> vec3f {
    let cl = rc_vnoise3(c * .3, b.seed + 71u);
    return vec3f(rc_range(b.na.y, 0., .5) * (.45 + 1.2 * cl * cl), rc_range(b.nb.z, 0., .6) * (.3 + 1.4 * rc_vnoise3(c * .23, b.seed + 70u)), rc_range(b.nb.x, 0., .8) * 2. * smoothstep(.35, .75, rc_vnoise3(c * .13, b.seed + 72u)));
}

// Crystal h's colour at crystal point q (f its weighted distance from the
// crystal's seed, c the seed, n a fine texture -.5..0.5 at a quarter and an
// eighth of a crystal, fl its bright flecks 0..1): its mineral by its id and
// the shares where it sits (only the largest dark grains are whole crystals;
// most are the flakes in rc_stone_granite), its own shade and hue and,
// scaled by k (number 7), the texture inside it: the feldspars cloudy,
// zoned from the centre, finely twinned along their own direction and
// flecked; quartz glassy blue-grey, darker at its heart. w its roughness
// offset (quartz and mica polish glossier, cloudy patches duller).
fn rc_stone_min(b: RecBlock, h: u32, c: vec3f, q: vec3f, f: f32, n: f32, fl: f32) -> vec4f {
    let u = rc_u01(rc_pcg(h ^ 81u));
    let v = rc_u01(rc_pcg(h ^ 147u));
    let p = rc_stone_gp(b, c);
    let k = rc_range(b.nb.w, 0., 2.);
    let dir = vec3f((vec3u(h >> 3u, h >> 13u, h >> 23u) & vec3u(511u))) / 256. - vec3f(1.);
    let cv = .5 + .5 * sin(dot(q, dir) * 30. + n * 4.);
    let dk = p.x * .35;
    if (u < dk) {
        return vec4f(b.col_b * mix(vec3f(.46, .42, .38), vec3f(.36, .41, .38), step(.5, v)) * max(1. + k * (.6 * n + .3 * cv * cv), .3), -.05);
    }
    if (u < dk + p.y) {
        return vec4f(mix(b.col_b, b.col_a, .62 + .3 * v) * vec3f(.95, .98, 1.05) * (1. + k * (.5 * n - .22 * (1. - smoothstep(0., .5, f)))), -.1 + .05 * n);
    }
    let m = max(1. + k * (.6 * n + .04 * sin(f * 19.) + .05 * (cv - .5) + .15 * fl), .45);
    if (u < dk + p.y + p.z) {
        return vec4f(mix(b.col_a, b.col_c, .22 + .2 * v) * vec3f(1.1, .97, .86) * m, .08 * n);
    }
    return vec4f(mix(b.col_a, b.col_c, .34 + .26 * v) * vec3f(.99, .99, 1.) * m, .04 + .08 * n);
}

// The granite at a point: its colour, roughness and the polish tilt of the
// crystal there (a slope in the surface's two tangents).
struct RcGranite {
    c: vec3f,
    r: f32,
    t: vec2f,
};

// Granite at world point w (crystal scale gs per m; fp the crystal's size
// over the pixel's footprint). Crystals of four minerals, 8 to 18 mm at the
// default 12 mm (the field above at a gently warped point, so they stay
// angular but their edges bend and interlock, never a straight mosaic), meet in a soft
// half-blend about a millimetre wide (no line); inside each a fine texture
// at 1.5 and 3 mm (cloud, flecks); black biotite and hornblende grains,
// 3 to 7 mm, angular and gathered in clusters, lie across them; each crystal's polished face is
// tilted a little its own way, so the highlight breaks up crystal by
// crystal as on honed stone; a faint tone drift over decimetres. Each
// detail fades to its mix as it nears a pixel (no shimmer, and no pale band
// at the limb: the far colour is the minerals' mix by their shares).
fn rc_stone_granite(b: RecBlock, w: vec3f, gs: f32, fp: f32) -> RcGranite {
    let q = w * gs;
    let kg = smoothstep(.75, 1.5, .7 * fp);
    let kf = smoothstep(.75, 1.5, .3 * fp);
    let kt = smoothstep(.75, 1.5, .125 * fp);
    let n2 = vec2f(rc_vnoise3(q * 2.6, b.seed + 77u), rc_vnoise3(q * 2.6, b.seed + 78u)) - vec2f(.5);
    let wq = q + (vec3f(rc_vnoise3(q * .8, b.seed + 74u), rc_vnoise3(q * .8, b.seed + 75u), rc_vnoise3(q * .8, b.seed + 76u)) - vec3f(.5)) * .38 + vec3f(n2, -n2.x) * .1;
    let p = rc_stone_gp(b, q);
    let dk = p.x * .35;
    let sd = min(dk, 1.);
    let sq = min(dk + p.y, 1.) - sd;
    let sk = min(dk + p.y + p.z, 1.) - sd - sq;
    var c = vec4f(mix(b.col_b * .42 * sd + mix(b.col_b, b.col_a, .77) * sq + mix(b.col_a, b.col_c, .32) * vec3f(1.1, .97, .86) * sk + mix(b.col_a, b.col_c, .47) * (1. - sd - sq - sk), b.col_b * .4, min(p.x * .75, 1.)), 0.);
    var t = vec2f(0.);
    if (kg > 0.) {
        let x = rc_stone_x3(wq, b.seed + 57u, rc_range(b.na.z, 0., .5));
        let t2 = rc_vnoise3(q * 8., b.seed + 69u);
        let n = ((rc_vnoise3(q * 3.5, b.seed + 73u) - .5) * .6 + (t2 - .5) * .4 * kt) * 2.2;
        let fl = smoothstep(.74, .8, t2) * kt;
        let ma = rc_stone_min(b, x.a, x.c, q, x.f1, n, fl);
        let ta = vec2f(vec2u(x.a >> 5u, x.a >> 15u) & vec2u(1023u)) / 512. - vec2f(1.);
        var g = ma;
        t = ta;
        if (x.gap < .08) {
            let e = .5 + .5 * smoothstep(0., .08, x.gap);
            g = mix(rc_stone_min(b, x.b, x.c, q, x.f1 + x.gap, n, fl), ma, e);
            t = mix(vec2f(vec2u(x.b >> 5u, x.b >> 15u) & vec2u(1023u)) / 512. - vec2f(1.), ta, e);
        }
        // Biotite and hornblende grains: a finer crystal field (about 2.4
        // to a crystal, so 3 to 7 mm), its cells dark where the dark
        // minerals gather; neighbours merge into angular clusters.
        let y = rc_stone_x3(wq * 2.4 + vec3f(17.), b.seed + 58u, .4);
        let dy = step(rc_u01(rc_pcg(y.a ^ 45u)), p.x * .75);
        let sp = mix(step(rc_u01(rc_pcg(y.b ^ 45u)), p.x * .75), dy, .5 + .5 * smoothstep(0., .1, y.gap)) * kf;
        c = mix(c, mix(g, vec4f(b.col_b * .4 * max(1. + .4 * n, .4), -.05), sp), kg);
        t = t * .035 * kg;
    }
    return RcGranite(c.rgb * (.95 + .1 * rc_vnoise3(q * .04, b.seed + 59u)), b.na.x + c.w, t);
}

// The grain's height at world point w on a surface of normal ng: the
// grain cell's F1 (a dome), or for sandstone (10) a fine value-noise sand
// at twice the grain scale; from High (q 3) the two nearest planes blend.
fn rc_stone_gh(b: RecBlock, w: vec3f, ng: vec3f, q: u32) -> f32 {
    let pp = rc_proj2(w, ng);
    let s = rc_range(b.na.w, 1., 200.) * 2.;
    var a = rc_stone_g(b, pp.a).x;
    var c = rc_stone_g(b, pp.b).x;
    if (b.variant == 10u) {
        a = rc_vnoise2(pp.a * s, b.seed + 64u);
        c = rc_vnoise2(pp.b * s, b.seed + 64u);
    }
    return mix(a, c, select(0., pp.w, q >= 3u));
}

// Water-worn (variant 11, the pebbles' own; Asset Studio pebbles session):
// in 3D over the world position, so a small curved stone shows no seam. A
// two-scale cloud (pale patches towards C by number 6, a few darker ones
// towards B), stains 3 to 8 cm across (number 8), a fine foliation along the
// veins (number 9, slate), a fine sand grain, and a mosaic of irregular
// grains (the crystalline field above at number 3's scale, 100-1000 per m,
// warped): each grain dark (B, share number 4), pale (C, share number 5) or
// the stone's own, in its own shade, blended in at 70% so the stone's tone
// shows through. The grains are the relief too (number 7, column 7 round 2:
// the value-noise crinkle read as a blocky maze, round domes as beads): each
// grain's face tilted its own way and a little domed, a dark crease where
// two meet, with broad soft dimples about 2 cm across. Cloud patches stretch
// along the foliation (streaks). Veins (kept off the horizontal, so a
// lying pebble shows them crossing it rather than ringing it) of
// C (number 1 their width, number 2 per m) wander, swell and thin and break
// along their length (round 2: a second set across them read as a grid).
struct RcWorn {
    c: vec3f,
    t: vec3f,
};

// The water-worn stone's noise at p, its seed offset s.
fn rc_stone_wn(b: RecBlock, p: vec3f, s: u32) -> f32 {
    return rc_vnoise3(p, b.seed + s);
}

fn rc_stone_wc(b: RecBlock, u: f32, d: f32) -> vec2f {
    return vec2f(step(u, b.nb.x * .5 * d), step(1. - b.nb.y * .4 * d, u));
}

// The relief's height (0..1): a soft sand grain at two scales, each turned
// off the noise's grid (no maze), over broad dimples about 2 cm across.
fn rc_stone_wh(b: RecBlock, w: vec3f) -> f32 {
    let q = w * rc_range(b.na.w, 100., 1000.);
    return rc_stone_wn(b, vec3f(q.x + q.z, q.y - q.x, q.z + q.y) * .45, 99u) * .5 + rc_stone_wn(b, vec3f(q.y - q.z, q.x + q.y, q.z - q.x) * 1.1, 95u) * .38 + rc_stone_wn(b, w * 45., 80u) * .12 - .5 * smoothstep(.8, .88, rc_stone_wn(b, w * 420., 74u));
}

fn rc_stone_ww(b: RecBlock, w: vec3f, k: f32) -> RcWorn {
    let q = w * rc_range(b.na.w, 100., 1000.);
    let h = rc_pcg(b.seed + 97u);
    let dv = normalize((vec3f(vec3u(h, h >> 8u, h >> 16u) & vec3u(255u)) / 127.5 - .99) * vec3f(1., .45, 1.));
    let wm = w + dv * dot(w, dv) * 4. * b.nc.y;
    let m = rc_stone_wn(b, wm * 7., 90u) * .6 + rc_stone_wn(b, wm * 19., 91u) * .4;
    var c = mix(b.col_a, b.col_c, smoothstep(.45, .75, m) * b.nb.z);
    c = mix(c, b.col_b, smoothstep(.6, .85, 1. - m) * b.nb.z * .4);
    c *= 1. + (rc_stone_wn(b, w * 22. + m, 89u) - .5) * .8 * b.nc.x;
    c *= 1. + (rc_stone_wn(b, w * 9. + dv * dot(w, dv) * 260., 84u) - .5) * .4 * b.nc.y * k;
    let g = rc_stone_wn(b, q * 3.1, 96u);
    let p = q * .6;
    let qw = q + (vec3f(rc_stone_wn(b, p, 86u), rc_stone_wn(b, p, 87u), rc_stone_wn(b, p, 88u)) - .5) * .7;
    let x = rc_stone_x3(qw, b.seed + 92u, .35);
    let ua = rc_u01(rc_pcg(x.a ^ 45u));
    // The grains gather: their shares swell and thin over a few centimetres.
    let d = .35 + 1.3 * rc_stone_wn(b, w * 23., 94u);
    let mk = mix(rc_stone_wc(b, rc_u01(rc_pcg(x.b ^ 45u)), d), rc_stone_wc(b, ua, d), .5 + .5 * smoothstep(0., .15, x.gap));
    let sh = .8 + .4 * fract(ua * 13.);
    c = mix(c, b.col_b * sh, mix(b.nb.x * .5, mk.x, k) * .7);
    c = mix(c, b.col_c * sh, mix(b.nb.y * .4, mk.y, k) * .7);
    c *= 1. - (.1 - .2 * fract(ua * 7.) + .2 * b.nb.w * (1. - smoothstep(0., .08, x.gap))) * k;
    // Mid-scale tone (3-8 mm), linked irregular weathered patches, pale
    // mineral traces and sparse pores (column 7 round 2: the faces had half
    // the reference's fine and mid-scale contrast).
    let n2 = rc_stone_wn(b, w * 170., 79u);
    let wp = smoothstep(.52, .6, rc_stone_wn(b, w * 32. + n2 * .4, 78u) * .7 + rc_stone_wn(b, w * 95., 77u) * .3);
    let tr = smoothstep(.76, .84, rc_stone_wn(b, vec3f(w.x + w.z, w.y - w.x, w.z + w.y) * 110. + n2 * .5, 76u)) * smoothstep(.45, .7, rc_stone_wn(b, w * 14., 75u));
    c *= (1. + (n2 - .5) * .55 * k) * (1. - .28 * wp) * (1. - .35 * smoothstep(.8, .88, rc_stone_wn(b, w * 420., 74u)) * k);
    c = mix(c, b.col_c, tr * min(.04 + b.nb.y, .18) * k);
    let wd = .12 * b.na.y * (.3 + 1.4 * rc_stone_wn(b, w * 13., 83u));
    let v = (1. - smoothstep(wd * .3, wd, abs(fract(dot(w, dv) * rc_range(b.na.z, 1., 50.) + (m - .5) * .4 + (rc_stone_wn(b, w * 30., 82u) - .5) * .14) - .5))) * smoothstep(.25, .45, rc_stone_wn(b, w * 8., 98u)) * step(.001, b.na.y) * k;
    // Each grain's face tilted its own way, a little domed.
    return RcWorn(mix(c, b.col_c * (.85 + .3 * g), v * .8), ((vec3f(vec3u(x.a >> 3u, x.a >> 11u, x.a >> 19u) & vec3u(255u)) / 255. - .5) * .25 + (qw - x.c) * .1) * k);
}

fn rc_stone(b: RecBlock, f: MatFrame, i: MatIn, q: u32) -> MatOut {
    let gs = rc_range(b.na.w, 1., 200.);
    var w = i.world;
    let ng = normalize(i.g);
    let kg = rc_aa_world(f, 1. / gs);
    if (q >= 3u && rc_extra(b, RC_X_STONE, 0u) && (q == 4u || f.dist < 8.)) {
        // Parallax, two steps: the view slides across raised grains 1 cm high.
        let dn = dot(f.v, ng);
        let ofs = (f.v - ng * dn) / max(dn, .3) * (.01 * kg);
        let h1 = 1. - rc_stone_g(b, rc_proj(w, ng)).x;
        let h2 = 1. - rc_stone_g(b, rc_proj(w - ofs * h1, ng)).x;
        w = w - ofs * (h1 + h2) * .5;
    }
    let kv = rc_aa_world(f, .3 / rc_range(b.na.z, 1., 50.));
    let kt = rc_aa_world(f, .0016);
    let ws = rc_range(b.na.w, 100., 1000.);
    // The water-worn grain fades by the footprint's area, as granite's: by
    // its long side it was wiped off a pale band along each stone's top
    // (column 7 round 2).
    let kw = smoothstep(.75, 1.5, 1. / ws / sqrt(max(length(f.dpx) * length(f.dpy), 1e-14)));
    var s: vec4f;
    var gr = 0.;
    var gt = vec2f(0.);
    var ww: RcWorn;
    if (b.variant == 9u) {
        // Faded by the footprint's area: at the silhouette the footprint
        // stretches one way only, and its long side wiped the crystals off
        // a band round the limb (round 7's plain strip along the top).
        // Number 8 shrinks the crystals (pebbles: 0 for the granite ball).
        let g9 = gs * (1. + 3. * b.nc.x);
        let fp = 1. / g9 / sqrt(max(length(f.dpx) * length(f.dpy), 1e-14));
        let g = rc_stone_granite(b, w, g9, fp);
        s = vec4f(g.c, 0.);
        gr = g.r;
        gt = g.t;
    } else if (b.variant == 11u) {
        ww = rc_stone_ww(b, w, kw);
        s = vec4f(ww.c, 0.);
    } else if (q >= 3u) {
        let pp = rc_proj2(w, ng);
        s = mix(rc_stone_t(b, pp.a, w.y, kg, kv, kt), rc_stone_t(b, pp.b, w.y, kg, kv, kt), pp.w);
    } else {
        s = rc_stone_t(b, rc_proj(w, ng), w.y, kg, kv, kt);
    }
    var o = rc_out(i, s.rgb, select(select(mix(b.na.x, .3, s.w), b.na.x * .35, b.variant == 7u), gr, b.variant == 9u), s.w);
    if (q >= 2u) {
        // Granite: each crystal's polish tilt.
        o.n = rc_bump(o.n, rc_tw(ng), gt * rc_near(f));
        let near = rc_near(f) * kg;
        if (b.variant == 11u) {
            // Water-worn: the relief (about .6 mm at number 7 = 1) and
            // each grain's own small tilt.
            let t = rc_tw(ng);
            let e = .25 / ws;
            let h = rc_stone_wh(b, w);
            o.n = rc_bump(o.n, t, ((vec2f(rc_stone_wh(b, w + t[0] * e), rc_stone_wh(b, w + t[1] * e)) - h) * (6e-4 / e) * kw - ww.t * t) * b.nb.w * rc_near(f));

        } else if (near > 0. && b.variant != 9u) {
            // Grain normals: each grain a low dome, flatter the more polished
            // the stone (scaled by its roughness); sandstone's a fine sand
            // (round 9: its domes read as foam), a third as strong. From
            // High both planes blend, so a column shows no seam.
            let t = rc_tw(ng);
            let e = .2 / gs;
            let h = rc_stone_gh(b, w, ng, q);
            let d = vec2f(rc_stone_gh(b, w + t[0] * e, ng, q), rc_stone_gh(b, w + t[1] * e, ng, q)) - vec2f(h);
            o.n = rc_bump(i.n, t, d * (-.3 * b.na.x * near * select(1., .35, b.variant == 10u) / (gs * e)));
        }
        let nt = rc_near(f) * kt;
        if (nt > 0. && rc_extra(b, RC_X_STONE, 3u)) {
            // Tiles: each slab's edge eases down into its joint.
            let t = rc_tw(ng);
            let ts = rc_range(b.nc.x, .1, 2.);
            let h = smoothstep(0., .004, rc_lay_basket(rc_proj(w, ng), ts, 1., b.seed + 13u).e);
            let d = vec2f(smoothstep(0., .004, rc_lay_basket(rc_proj(w + t[0] * .001, ng), ts, 1., b.seed + 13u).e), smoothstep(0., .004, rc_lay_basket(rc_proj(w + t[1] * .001, ng), ts, 1., b.seed + 13u).e)) - vec2f(h);
            o.n = rc_bump(o.n, t, d * (.6 * nt));
        }
        if (b.variant >= 4u && b.variant < 8u && rc_near(f) * kv > 0.) {
            // Relief of the batch 2 variants: a tenth of a feature step.
            let t = rc_tw(ng);
            let e = .1 / rc_range(b.na.z, 1., 50.);
            let p = rc_proj(w, ng);
            let h = rc_stone_r(b, p).x;
            let d = vec2f(rc_stone_r(b, rc_proj(w + t[0] * e, ng)).x, rc_stone_r(b, rc_proj(w + t[1] * e, ng)).x) - vec2f(h);
            o.n = rc_bump(o.n, t, d * (1.5 * rc_near(f) * kv));
        }
        if (rc_extra(b, RC_X_STONE, 2u) && b.nb.z > 0.) {
            // Edge translucency (marble, alabaster, jade): a lit rim.
            o.sheen = vec4f(s.rgb * b.nb.z * 1.5, 3.);
        }
    }
    if (q >= 3u && rc_extra(b, RC_X_STONE, 1u)) {
        // Glints: mica and quartz grains catch the sun from some angles.
        let g = rc_voronoi2(rc_proj(w, ng) * gs * 2., b.seed + 3u, 1.);
        let tw = step(1. - .15 * b.nb.y, fract(g.w * 13. + dot(f.v, vec3f(3.1, 5.3, 7.7))));
        o.glint = vec3f(tw * b.nb.y * 3. * (1. - smoothstep(.1, .3, g.x)) * rc_aa_world(f, .5 / gs));
    }
    return o;
}
