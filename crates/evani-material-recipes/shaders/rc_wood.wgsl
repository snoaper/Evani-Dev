// evani.shader.rc.wood
// Wood (recipe 4, batch 1 build plan section 3, package P4). Written fresh
// for Evani.Dev. The grain runs along u.
//   numbers: na = rough, grain scale (1-100 rings per uv), ring warp,
//            ring contrast; nb = knot amount, plank width (0.05-1 uv), gap
//            depth, fleck; nc = paint peel (0-1), opening (0-0.9, lattice)
//   colours: A early grain (light), B late grain (dark), C knot (also the
//            paint, and the chinking between logs)
//   variants: 0 flat-sawn, 1 quarter-sawn, 2 end grain, 3 planks, then the
//             (15 and 16 solid: the wood as a log's volume, the rings round a
//             pith axis along x outside the piece, so every face, bevel and
//             end shows the cut it would, with no seam; see rc_wood_solid),
//             batch 3 layouts (plank width is the unit): 4 clapboard (long
//             boards, each lapping the one below), 5 board and batten
//             (vertical boards, a narrow strip over every joint), 6 logs
//             (round, chinking in C between), 7 shingle (courses, each over
//             the one below), 8 parquet (herringbone blocks), 9 panels (a
//             frame round a raised field), 10 culm (bamboo: vertical poles
//             with nodes), 11 granular (cork: grains of colours A and B),
//             12 lattice (slats at 45 degrees both ways, the open share of
//             each period by `opening`; the holes and the slat shadows are
//             the cut-out's, B3-P2, shaders/cut.wgsl, feature `cutout`);
//             then P8-4's: 13 charred (a layout: the char split into domed
//             blocks plank width high, rows offset at random, the cracks
//             wandering; the plank gaps extra opens the cracks, gap depth
//             darkens them; ash C on each crown; `opening` reads as embers
//             glowing in the cracks), 14 marquetry (on flat-sawn grain: (tiles plank
//             width across inlaid with the layout motif in veneers A, B and
//             C, each piece's grain its own way, a dark line round each;
//             without the layouts part it draws as flat-sawn A))
//   The layouts (4 to 11) are their own part (feature `recipe-wood-layouts`,
//   shaders/rc_wood_layouts.wgsl); without it shaders/rc_wood_planks.wgsl
//   stands in: every layout draws as planks and cork as flat colour A.
//   extras: grain relief (bit 0), plank gaps (bit 1; the joints of every
//           layout), studs (bit 2: nail heads on planks, from the layout
//           helper, batch 3 B3-P1), painted (bit 3: a coat of colour C that
//           peels by `paint_peel`, more at the joints), adze (bit 4:
//           scalloped tool marks)
// Levels (q, the detail group): lite draws what low draws
// (every tier keeps the pattern; before critic round 1 lite was a flat two-tone grain); low draws the
// warped rings, pores, flecks, knots and planks, anti-aliased (rings closer
// than 1.5 px fade to their average); medium adds the raised late grain and
// the plank bevels as normals within 25 m; high and ultra draw as medium.
// Nail heads draw on every tier (faded out under 1.5 px), domed from medium.

// The ring coordinate at q (x along the grain, y across, in rings; rings sit
// at whole numbers). Flat-sawn cuts a log beside its pith, so the rings
// arch (cathedrals) as the pith's depth wanders; quarter-sawn cuts toward
// the pith (straight lines); end grain cuts across it (circles).
fn rc_wood_r(b: RecBlock, q: vec2<f32>, v: u32) -> f32 {
    let w = (rc_fbm2(q * vec2<f32>(0.03, 0.2), b.seed, 2u) - 0.5) * b.na.z * 6.0;
    if (v == 1u) {
        return q.y + w;
    }
    if (v == 2u) {
        return length(q) + w;
    }
    return length(vec2<f32>(q.y, 2.0 + 1.5 * sin(q.x * 0.04 + f32(b.seed)))) + w;
}

// How much late (dark, dense) wood: it grows slowly through each ring, then
// the next ring's early wood starts sharply.
fn rc_wood_late(r: f32) -> f32 {
    return smoothstep(0.55, 0.92, fract(r));
}

// Boards plank-width high, lf times as long, ends staggered per row: x, y
// inside the board (y centred), the distance to its nearest edge, and the
// board's own random number.
fn rc_wood_plank(b: RecBlock, uv: vec2<f32>, pw: f32, lf: f32) -> vec4<f32> {
    let row = floor(uv.y / pw);
    let len = pw * lf;
    let x = uv.x + rc_u01(rc_hash2(vec2<i32>(i32(row), 0), b.seed)) * len;
    let seg = floor(x / len);
    let lx = x - seg * len;
    let ly = uv.y - row * pw;
    let ed = min(min(ly, pw - ly), min(lx, len - lx));
    return vec4<f32>(lx, ly - pw * 0.5, ed, rc_u01(rc_hash2(vec2<i32>(i32(row), i32(seg)), b.seed + 1u)));
}

fn rc_wood(b: RecBlock, f: MatFrame, i: MatIn, q: u32) -> MatOut {
    if (b.variant == 11u) {
        return rc_wood_cork(b, f, i, q);
    }
    if (b.variant > 14u) {
        return rc_wood_solid(b, f, i, q);
    }
    let gs = rc_range(b.na.y, 1.0, 100.0);
    let kr = rc_aa(f, 1.0 / gs);
    let pw = rc_range(b.nb.y, 0.05, 1.0);
    let gw = pw * 0.04;
    var v = b.variant;
    let nv = v;
    var lu = i.uv - vec2<f32>(0.5);
    var shade = 1.0;
    var gap = 0.0;
    var gwid = gw;
    var pl = vec4<f32>(0.0);
    var form = vec2<f32>(1.0);
    var mq = vec4<f32>(0.0);
    // Vertical layouts (board and batten, bamboo) are laid out turned.
    let sw = nv == 5u || nv == 10u;
    let qq = select(i.uv, i.uv.yx, sw);
    if (nv == 12u) {
        // Lattice: the +45 degree slats lie over the others; the grain runs
        // along each slat, each slat its own tone (as cut.wgsl lays them).
        let r = vec2<f32>(i.uv.x + i.uv.y, i.uv.x - i.uv.y) * 0.70710678;
        let pp = pw / max(1.0 - rc_range(b.nc.y, 0.0, 0.9), 0.05);
        let k = floor(r / pp + vec2<f32>(0.5));
        let top = abs(r.x - k.x * pp) < pw * 0.5;
        let ks = select(k.y, k.x, top);
        lu = select(vec2<f32>(r.x, r.y - k.y * pp), vec2<f32>(r.y, r.x - k.x * pp), top) + vec2<f32>(ks * 7.3, 0.0);
        shade = 0.88 + 0.24 * rc_u01(rc_hash2(vec2<i32>(i32(ks), i32(!top)), b.seed + 3u));
        v = 0u;
    } else if (nv == 14u) {
        // Marquetry lies on flat-sawn grain.
        mq = rc_wood_mq(b, i.uv, pw);
        lu = mq.xy;
        shade = 1.0 - 0.7 * rc_fade(mq.w, 0.1, rc_aa(f, pw * 0.03));
        v = 0u;
    } else if (v >= 3u) {
        // Each board, shingle or block is its own piece with its own tone
        // (a bamboo pole's grain runs straight).
        pl = rc_wood_u(b, qq, pw);
        form = rc_wood_form(b, pl, pw);
        lu = pl.xy + vec2<f32>(pl.w * 97.0, 0.0);
        shade = (0.85 + 0.3 * fract(pl.w * 13.0)) * form.x;
        v = select(0u, 1u, nv == 10u);
        gwid = select(gw, pw * 0.1, nv == 6u);
        if (rc_extra(b, RC_X_WOOD, 1u) && nv != 9u) {
            gap = rc_fade(1.0 - smoothstep(gwid * 0.5, gwid, pl.z), 0.08, rc_aa(f, gwid));
        }
    }
    let p = lu * gs;
    var c: vec3<f32>;
    var rough = b.na.x;
    // Knots: some cells of a coarse, grain-stretched cell grid hold one;
    // the rings swirl round it and its core is colour C.
    let kv = rc_voronoi2(p * vec2<f32>(0.05, 0.25), b.seed + 5u, 0.8);
    let knot = f32(kv.w < b.nb.x * 0.4);
    let r = rc_wood_r(b, p, v) + knot * 2.5 * (1.0 - smoothstep(0.0, 0.5, kv.x));
    let late = rc_fade(rc_wood_late(r), 0.4, kr);
    c = mix(b.col_a, b.col_b, mix(0.35, late, b.na.w));
    // Pores: long thin streaks along the grain.
    let s = rc_fade(rc_vnoise2(vec2<f32>(p.x * 0.4, p.y * 10.0), b.seed + 9u), 0.5, rc_aa(f, 0.1 / gs));
    c *= (0.88 + 0.24 * s);
    // Flecks (the rays quarter-sawn oak shows): lighter, smoother.
    let fl = smoothstep(0.72, 0.9, rc_vnoise2(vec2<f32>(p.x * 0.6, p.y * 0.12), b.seed + 11u)) * b.nb.w * kr;
    c = mix(c, min(b.col_a * 1.2, vec3<f32>(1.0)), fl * 0.6);
    c = mix(c, b.col_c, knot * (1.0 - smoothstep(0.1, 0.18, kv.x)));
    rough *= (1.0 - 0.2 * late) * (1.0 - 0.3 * fl);
    var nail = 0.0;
    var pq = vec2<f32>(0.0);
    let pt = vec2<f32>(pw * 3.5, pw * 0.5);
    if (nv == 3u && rc_extra(b, RC_X_WOOD, 2u)) {
        // Nail heads: two across each plank at every joist, the joists in
        // straight lines across the deck, a rust stain round each.
        pq = vec2<f32>(i.uv.x, pl.y + pw * 0.5);
        let kn = rc_aa(f, pw * 0.1);
        nail = rc_fade(smoothstep(0.0, 0.3, rc_lay_studs(pq, pt, pw * 0.05)), 0.0, kn);
        c = mix(c * (1.0 - 0.3 * rc_fade(rc_lay_studs(pq, pt, pw * 0.11), 0.0, kn)), b.col_c * 0.5, nail);
    }
    if (nv == 6u) {
        // Chinking fills the gaps between logs.
        c = mix(c, b.col_c, gap);
    }
    if (rc_extra(b, RC_X_WOOD, 3u)) {
        // A coat of colour C, thinner at the joints, peeling in patches to
        // the grain below (the wear layer peels it further).
        let cover = smoothstep(b.nc.x * 0.6, b.nc.x * 0.6 + 0.1, rc_fbm2(i.uv * 9.0, b.seed + 15u, 3u) - gap * 0.25);
        c = mix(c, b.col_c, cover);
        rough = mix(rough, 0.55, cover);
    }
    var ad = vec4<f32>(0.0, 0.0, 1.0, 0.0);
    if (rc_extra(b, RC_X_WOOD, 4u)) {
        // Adze marks: shallow scoops across the beam, a ridge between each.
        ad = rc_voronoi2(vec2<f32>(i.uv.x * 5.0, i.uv.y * 18.0), b.seed + 14u, 0.8);
        c *= (0.9 + 0.1 * rc_fade(smoothstep(0.0, 0.15, ad.z), 0.8, rc_aa(f, 0.05)));
    }
    if (nv == 13u) {
        // Char: ash C on the crown of each domed block.
        c = mix(c, b.col_c, 0.5 * form.y);
    }
    c *= shade * (1.0 - gap * b.nb.z * 0.85);
    var o = rc_out(i, c, mix(rough + gap * 0.3, 0.45, nail), 0.4 * nail);
    o.occ = i.occ * (1.0 - gap * b.nb.z * 0.7);
    if (nv == 13u) {
        // Embers glowing in the char's cracks, by `opening`.
        o.emit = vec3<f32>(1.0, 0.3, 0.06) * (gap * b.nc.y * 2.0 * rc_vnoise2(i.uv * 9.0 / pw, b.seed + 19u));
    }
    if (q >= 2u) {
        let near = rc_near(f);
        var g = vec2<f32>(0.0);
        if (near > 0.0 && rc_extra(b, RC_X_WOOD, 0u) && (nv < 7u || nv >= 9u)) {
            // Raised late grain (weathered wood), across the rings.
            let h = rc_wood_late(rc_wood_r(b, p, v));
            g = (vec2<f32>(rc_wood_late(rc_wood_r(b, p + vec2<f32>(0.15, 0.0), v)), rc_wood_late(rc_wood_r(b, p + vec2<f32>(0.0, 0.15), v))) - vec2<f32>(h)) * kr;
        }
        if (near > 0.0 && (gap > 0.0 || nv >= 4u) && nv != 12u && nv != 14u) {
            // Plank bevels: the board falls away into its gap (logs and
            // poles round off, shingles lie over the one below).
            let e = gwid * 0.5;
            let h = form.y;
            g += (vec2<f32>(rc_wood_form(b, rc_wood_u(b, qq + vec2<f32>(e, 0.0), pw), pw).y, rc_wood_form(b, rc_wood_u(b, qq + vec2<f32>(0.0, e), pw), pw).y) - vec2<f32>(h)) * (3.0 * b.nb.z);
        }
        if (near > 0.0 && nail > 0.0) {
            // Each head a low dome.
            let e = pw * 0.025;
            let h = rc_lay_studs(pq, pt, pw * 0.05);
            g += (vec2<f32>(rc_lay_studs(pq + vec2<f32>(e, 0.0), pt, pw * 0.05), rc_lay_studs(pq + vec2<f32>(0.0, e), pt, pw * 0.05)) - vec2<f32>(h)) * 0.8;
        }
        if (near > 0.0 && rc_extra(b, RC_X_WOOD, 4u)) {
            let e = 0.004;
            let h = smoothstep(0.0, 0.3, ad.z);
            g += (vec2<f32>(smoothstep(0.0, 0.3, rc_voronoi2(vec2<f32>((i.uv.x + e) * 5.0, i.uv.y * 18.0), b.seed + 14u, 0.8).z), smoothstep(0.0, 0.3, rc_voronoi2(vec2<f32>(i.uv.x * 5.0, (i.uv.y + e) * 18.0), b.seed + 14u, 0.8).z)) - vec2<f32>(h)) * 0.4;
        }
        o.n = rc_bump(i.n, rc_tuv(f, i.n), select(g, g.yx, sw) * near);
    }
    return o;
}

// Solid wood (15, 16; the oak block's live session, after rounds 8 and 9):
// the piece cut from a log, in object space (the object-local position, in
// metres), so every face, bevel and end shows the cut it would, with no seam
// or chevron. The pith runs along x at (y, z) = numbers 5 and 9 (-1..1 m),
// a little tilted and wandering. Each year's ring has its own width and the
// rings wander in 3D at two scales (ring warp), so a long face, cut almost
// along them, shows fine, uneven, wavy lines that split and merge, and the
// ends show the rings round the pith. Each ring opens with its band of
// large pores (oak is ring-porous: dark, broken into dashes and dots, a
// little rough and sunk by `relief`, number 6), then the late wood darkens
// slowly (ring contrast). Rays cross the rings (fleck: fine radial lines
// on the ends, short flecks along the grain), pin knots (knot amount) swirl
// the rings round them, long tone streaks run with the grain (number 8),
// fine fibres lie between. Every feature fades to its average below a pixel
// or two (the ring's step across the surface, from the pith's direction).
// Ideas from the Cornell solid-wood papers (Liu, Dong, Hasan, Marschner;
// Liu, Marschner, Dye); no code.
fn rc_wood_solid(b: RecBlock, f: MatFrame, i: MatIn, q: u32) -> MatOut {
    let p = i.local;
    // Ring spacing (m): grain scale is a quarter of the rings a metre (2.5
    // to 250 mm).
    let sp = 0.25 / rc_range(b.na.y, 1.0, 100.0);
    let pc = vec2<f32>(rc_range(b.nb.y, -1.0, 1.0), rc_range(b.nc.y, -1.0, 1.0)) + vec2<f32>(0.03, -0.02) * p.x + (vec2<f32>(rc_vnoise2(vec2<f32>(p.x * 1.5, 0.0), b.seed), rc_vnoise2(vec2<f32>(p.x * 1.5, 9.0), b.seed)) - vec2<f32>(0.5)) * 0.04;
    let dv = p.yz - pc;
    let rho = max(length(dv), 1e-4);
    let an = atan2(dv.y, dv.x);
    let a = an * rho;
    // Pin knots: a few cells of a grid along x and round the rings hold one,
    // a branch stub a few millimetres across, wider further out.
    let kv = rc_voronoi2(vec2<f32>(p.x * 5.0, a * 4.0), b.seed + 5u, 0.8);
    let kd = kv.x * 0.2 / (0.0015 + 0.003 * rho);
    let kn = f32(kv.w < b.nb.x * 0.5);
    // The rings: grown off centre (wider on one side), wandering slowly
    // along the grain (long, gentle figure, no loops) and in waves a few
    // tens of centimetres long, bulging round a knot; then each year its own
    // width.
    let wv = ((rc_vnoise3(p * vec3<f32>(0.7, 8.0, 8.0), b.seed + 3u) - 0.5) * 0.012 + (rc_vnoise3(p * vec3<f32>(5.0, 12.0, 12.0), b.seed + 4u) - 0.5) * 0.006) * b.na.z * 2.0;
    let r = (rho + dot(dv, vec2<f32>(0.1, -0.07)) + wv - kn * 0.006 / (1.0 + kd * kd)) / sp;
    // The wood's cells (fibres, pores) in its volume, waving with the rings.
    let pv = vec3<f32>(p.x, p.yz + dv / rho * wv);
    let t = r + 1.4 * rc_vnoise2(vec2<f32>(r * 0.15, 0.5), b.seed + 6u) + 0.26 * rc_vnoise2(vec2<f32>(r * 1.1, 2.5), b.seed + 6u);
    let fr = fract(t);
    // Rings per pixel across this surface (from the pith's direction).
    let nn = normalize(select(i.n, i.ln, dot(i.ln, i.ln) > 0.0));
    let gr = vec3<f32>(0.0, dv / rho);
    let gt = gr - nn * dot(nn, gr);
    let dt = length(gt) * f.world_px / sp;
    // The pore band, widened to a pixel or so with its darkness kept, broken
    // into dashes along the grain and dots round the ring; the rings fade to
    // their average closer than about 2.5 px.
    let sg = sqrt(0.0032 * (1.0 + 2.0 * nn.x * nn.x) + 0.09 * dt * dt);
    let kb = 1.0 - smoothstep(0.3, 0.6, dt);
    let x = fr - 0.12;
    let pb = exp(-x * x / (2.0 * sg * sg)) * min(0.075 / sg, 1.0) * mix(0.2, 1.0, smoothstep(0.3, 0.7, rc_vnoise3(vec3<f32>(p.x * 25.0, t * 1.3, a * 120.0), b.seed + 9u))) * kb * (0.3 + 0.7 * rc_u01(rc_hash2(vec2<i32>(i32(floor(t)), 7), b.seed)));
    let pore = pb + 0.09 * (1.0 - kb);
    let lw = mix(0.45, smoothstep(0.25, 1.0, fr), kb) * (1.0 - 0.6 * nn.x * nn.x);
    // Fibres: streaks along the grain that follow the rings, three scales
    // (each about 30 times longer than wide, the finest a pixel or two
    // across here), drawn as thin darker lines; and the open pores, sparse
    // dark flecks in the early wood (dashes along the grain, dots on the
    // ends).
    let sd = max(1.0 - 3.0 * nn.x * nn.x, 0.0);
    let fib = rc_wood_fib(b, pv, f.world_px) * sd;
    let pr = smoothstep(0.62, 0.85, rc_vnoise3(pv * vec3<f32>(40.0, 700.0, 700.0), b.seed + 8u)) * exp(-(fr - 0.15) * (fr - 0.15) * 20.0) * (1.0 - smoothstep(0.7, 1.4, f.world_px * 700.0));
    // Hairline checks on the end grain (number 7, the rays they open
    // along): some of 24 sectors round the pith hold a fine dark split along
    // a wandering ray, starting some
    // way out and running a few centimetres to decimetres.
    let sj = floor(an * 3.8197);
    let hj = rc_hash2(vec2<i32>(i32(sj), 3), b.seed);
    let r0 = 0.03 + 0.25 * rc_u01(rc_pcg(hj));
    let hc = (1.0 - smoothstep(0.0004, 0.0004 + f.world_px, abs(an - (sj + 0.2 + 0.6 * rc_u01(hj)) / 3.8197 - 0.03 * (rc_vnoise2(vec2<f32>(rho * 60.0, sj), b.seed) - 0.5)) * rho)) * f32(rc_u01(rc_pcg(hj + 1u)) < b.nb.w * 1.8) * smoothstep(r0, r0 + 0.02, rho) * (1.0 - smoothstep(r0 + 0.08, r0 + 0.3, rho)) * nn.x * nn.x;
    // Long, faint tone streaks with the grain; end grain a little darker
    // and duller; a check's inside dark (the surface's crease).
    let tn = rc_fbm2(vec2<f32>(p.x * 0.35, rho * 20.0), b.seed + 13u, 3u) - 0.5;
    var c = mix(b.col_a, b.col_b, clamp(0.12 + 0.3 * lw + 1.8 * pore * b.na.w + 0.5 * pr, 0.0, 1.0));
    c *= (1.0 - 0.35 * fib) * (1.0 - 0.6 * hc) * (vec3<f32>(1.0) + tn * b.nc.x * vec3<f32>(1.6, 1.3, 1.0)) * (1.0 - 0.8 * max(smoothstep(100.0, 600.0, -f.curv), smoothstep(0.05, 0.4, f.nv))) * (1.0 - kn * 0.35 * (1.0 - smoothstep(1.0, 3.5, kd)));
    c = mix(c, b.col_c, kn * (1.0 - smoothstep(0.7, 1.0, kd)));
    var o = rc_out(i, c, b.na.x * (1.0 + 0.4 * pore + 0.3 * pr + 0.2 * fib), 0.0);
    if (q >= 2u) {
        // The pore band, pores and fibres sunk by `relief`: their slope
        // along the ring's step (the fibres' by one step across them).
        o.n = normalize(i.n + mat_l2w(f, gt * ((-x / (sg * sg) * pb * 0.3 / sp - (rc_wood_fib(b, pv + gr * 0.0004, f.world_px) * sd - fib) * 1250.0) * rc_near(f) * b.nb.z * 0.0004)));
    }
    return o;
}

// The solid wood's fibres at volume point v (m), for px metres a pixel:
// thin, crisp lines along the grain (where a stretched noise crosses its
// middle), broken so each starts and stops, at three scales; 0 between them.
fn rc_wood_fib(b: RecBlock, v: vec3<f32>, px: f32) -> f32 {
    var s = 0.0;
    var k = 1.0;
    for (var o = 0u; o < 3u; o = o + 1u) {
        let q = v * vec3<f32>(25.0, 300.0, 300.0) * k;
        s += smoothstep(0.8, 0.95, 1.0 - abs(2.0 * rc_vnoise3(q, b.seed + 20u + o) - 1.0)) * smoothstep(0.35, 0.6, rc_vnoise3(q * vec3<f32>(0.3, 0.3, 0.3) + vec3<f32>(7.1), b.seed + 30u + o)) * (1.0 - smoothstep(0.3, 0.6, px * 300.0 * k)) / sqrt(k);
        k *= 2.2;
    }
    return s * 0.6;
}
