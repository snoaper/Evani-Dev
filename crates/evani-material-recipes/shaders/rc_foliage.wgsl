// evani.shader.rc.foliage
// Foliage (recipe 20, batch 2 build plan 3.2, package B2-P1). Written fresh
// for Evani.Dev; no outside code. Leaves, needles, grass and straw are cards
// (a uv quad cut by its texture's alpha, which the model pass already masks);
// turf and litter are ground seen from above, in world space (rc_proj), so a
// terrain needs no uv. Being a recipe-stage recipe, every condition layer
// (rain, snow, mud...) runs on it.
//   numbers: na = variation, thickness, vein, gloss; nb = edge dry, blade
//            scale (2-200: side veins, needles or blades per uv; blades or
//            fallen leaves per metre for turf and litter; moss lumps), bare
//            (turf: share of bare soil; needle: berry specks; grass: share of
//            strands with a seed head), underside; nc.x = fan (material-view
//            round 4: above 0.5 a leaf's veins fan from the stalk end of
//            the blade, uv (0.5, 0.8), as five main veins 0.85 rad apart,
//            each with its own side veins: maple and plane leaves)
//   colours: A colour 1, B colour 2 (variation, berries), C the back-light
//            colour (leaf, needle, grass, straw, moss) or the soil showing
//            through (turf, litter; their back-light is A brightened)
//   variants: 0 leaf (midrib along u = 0.5, tip at v = 0), 1 needle (a spray:
//             twig along u = 0.5), 2 grass and 3 straw (strands along v, root
//             at v = 1), 4 turf, 5 moss, 6 litter
//   extras: back_light (bit 0: sun through the leaf), veins (bit 1), bare
//           (bit 2: bare patches, berry specks, seed heads)
// Thickness: how little light passes; for turf and litter the blade width
// (above 0.66 a turf blade becomes three round leaflets: clover).
// Levels (q, the foliage group): lite draws what low draws
// (every tier keeps the pattern; before critic round 1 lite was flat colour 1 to colour 2 variation,
// no back-light); low draws blades, needles, litter cells, vein colour and dry
// edges, anti-aliased (edges soften over the pixel, cells under 1.5 px fade
// to their average); medium adds vein and blade normals within 25 m, the
// back-light wrap, the underside tint and gloss; high adds a finer second
// blade layer for turf and moss and sun glints on waxy leaves; ultra as high.

// One blade or fallen leaf per cell of p, turned at random: x its coverage
// (edges softened over e cells), y a hash, z along it (0 root, 1 tip). ln.x
// (0..1) lays the blades towards angle ln.y (a mown lawn), 0 any way.
fn rc_foliage_bl(p: vec2<f32>, s: u32, w: f32, e: f32, ln: vec2<f32>) -> vec3<f32> {
    let c = floor(p);
    let h = rc_hash2(vec2<i32>(c), s);
    let a = mix(rc_u01(h) * 6.2832, ln.y + (rc_u01(h) - 0.5) * 1.1, ln.x);
    let k = rc_pcg(h);
    let d = p - c - 0.5 - (vec2<f32>(rc_u01(k), rc_u01(rc_pcg(k))) - 0.5) * 0.2;
    let r = vec2<f32>(d.x * cos(a) + d.y * sin(a), d.y * cos(a) - d.x * sin(a));
    let t = clamp(r.y * 1.3 + 0.5, 0.0, 1.0);
    var m = (1.0 - smoothstep(-e, e, abs(r.x) - w * 0.35 * (1.0 - t * t))) * step(abs(r.y), 0.38);
    if (w > 0.66) {
        // Clover: three round leaflets.
        m = 1.0 - smoothstep(-e, e, length(d) - 0.28 * sqrt(0.5 + 0.5 * cos(3.0 * (atan2(d.y, d.x) - a))));
    }
    return vec3<f32>(m, rc_u01(rc_pcg(k + 7u)), t);
}

// The dry brown an edge or tip turns, amount ed, where x (0 inside .. 1 the
// edge or tip, plus noise) is past 1 - ed.
fn rc_foliage_dry(b: RecBlock, c: vec3<f32>, x: f32) -> vec3<f32> {
    let ed = b.nb.x;
    return mix(c, vec3<f32>(0.3, 0.18, 0.07), ed * smoothstep(1.0 - ed, 1.01 - 0.4 * ed, x));
}

// Turf and litter at plane point p (in blade cells), n layers of blades over
// the soil: rgb the colour, w the height (0 soil .. 1 a blade top).
fn rc_foliage_w(b: RecBlock, p: vec2<f32>, e: f32, n: u32) -> vec4<f32> {
    let lit = b.variant == 6u;
    let dens = smoothstep(b.nb.z * 0.9 - 0.05, b.nb.z * 0.9 + 0.1, select(1.0, rc_fbm2(p * 0.08, b.seed + 9u, 2u), rc_extra(b, RC_X_FOLIAGE, 2u)));
    var c = select(mix(b.col_c * 0.6, b.col_a * 0.4, dens), b.col_c * 0.6, lit);
    var h = 0.0;
    let ms = rc_foliage_mow(b, p);
    let ln = vec2<f32>(select(b.nb.w, 0.0, lit), 1.5708 + 3.1416 * ms);
    for (var k = 0u; k < n; k = k + 1u) {
        let z = select(1.0, 2.2, k == 2u);
        let bl = rc_foliage_bl(p * z + vec2<f32>(f32(k) * 0.5), b.seed + k * 31u, b.na.y, e * z, ln);
        let col = mix(b.col_a, b.col_b, select(b.na.x, 1.0, lit) * bl.y) * select(0.6 + 0.4 * bl.z, 0.75 + 0.5 * fract(bl.y * 7.3), lit);
        let m = bl.x * step(bl.y, dens);
        c = mix(c, rc_foliage_dry(b, col, bl.z), m);
        h = mix(h, select(0.4 + 0.6 * bl.z, 0.5 + 0.2 * f32(k), lit), m);
    }
    return vec4<f32>(c * rc_foliage_band(b, ms), h);
}

// Standing turf blades (material-view critic rounds 8-10: the lawn read as
// a flat blurry card). The surface is the top of a layer of blades STAND_H
// cells deep; the eye ray is marched down through it in n steps and the
// blades it meets are laid front to back: each cell of two interleaved grids
// holds one blade with its own height, lean, taper and tint, dark at the
// root and light at the tip, and darker the deeper the ray has gone (the
// blades shade each other); what the ray reaches at the bottom is thatch. p
// is in blade cells, vt the eye direction across the plane per unit of
// height, e the pixel size in cells. rgb the colour, w the height seen (0
// the bottom .. 1 the top).
const STAND_H: f32 = 2.4;

fn rc_foliage_stand(b: RecBlock, p: vec2<f32>, vt: vec2<f32>, e: f32, n: u32, dens: f32) -> vec4<f32> {
    let ms = rc_foliage_mow(b, p * 0.5);
    let lay = 1.5708 + 3.1416 * ms;
    let lk = clamp(b.nb.w, 0.0, 1.0) * 0.7;
    var acc = vec3<f32>(0.0);
    var a = 0.0;
    var hw = 0.0;
    for (var i = 0u; i < n; i = i + 1u) {
        let t = (f32(i) + 0.5) / f32(n);
        let h = STAND_H * (1.0 - t);
        let q0 = p - vt * (STAND_H - h);
        for (var g = 0u; g < 2u; g = g + 1u) {
            let q = select(q0, q0 * 1.31 + vec2<f32>(0.37, 0.71), g == 1u);
            let c = floor(q);
            let h0 = rc_hash2(vec2<i32>(c), b.seed + 101u + g * 17u);
            let h1 = rc_pcg(h0);
            let h2 = rc_pcg(h1);
            let h3 = rc_pcg(h2);
            let r0 = rc_u01(h0);
            let hb = STAND_H * (0.5 + 0.5 * r0);
            if (h < hb && rc_u01(rc_pcg(h3)) < dens) {
                let fh = h / hb;
                let an = mix(rc_u01(h1) * 6.2832, lay + (rc_u01(h1) - 0.5) * 1.3, lk);
                let root = c + 0.5 + (vec2<f32>(rc_u01(h2), rc_u01(h3)) - 0.5) * 0.45;
                let ctr = root + vec2<f32>(cos(an), sin(an)) * (0.12 + 0.22 * rc_u01(h2)) * fh * fh;
                let w = (0.10 + 0.12 * b.na.y) * (1.0 - 0.85 * fh * fh);
                let m = (1.0 - smoothstep(w - e, w + e, length(q - ctr))) * (1.0 - a);
                if (m > 0.0) {
                    let tint = mix(b.col_a, b.col_b, rc_u01(h1 * 7u + 3u)) * (0.85 + 0.3 * r0);
                    var col = tint * (0.32 + 0.68 * pow(fh, 0.8)) * mix(1.0, 0.5, t * t);
                    col = rc_foliage_dry(b, col, fh);
                    acc = acc + col * m;
                    hw = hw + (1.0 - t) * m;
                    a = a + m;
                }
            }
        }
        if (a > 0.98) {
            break;
        }
    }
    let thatch = mix(b.col_c, b.col_a, 0.35) * 0.32;
    return vec4<f32>((acc + thatch * (1.0 - a)) * rc_foliage_band(b, ms), hw);
}

// Mowing stripes (round 5; turf with underside above 0): which stripe plane
// point p (in blade cells) is in, 0 or 1; the stripes are 0.5 m wide.
fn rc_foliage_mow(b: RecBlock, p: vec2<f32>) -> f32 {
    return step(0.5, fract(p.x / rc_range(b.nb.y, 2.0, 200.0)));
}

// A stripe's shade: blades laid away from the eye light, towards it dark.
fn rc_foliage_band(b: RecBlock, ms: f32) -> f32 {
    return 1.0 + select(0.0, b.nb.w * 0.16 * (2.0 * ms - 1.0), b.variant == 4u);
}

// Far turf (procedural realism round 6: the far lawn read as a blurred
// square mosaic, one octave of value noise on a world grid): grass as a
// photograph shows it from a few metres to a few tens, its tone at three
// scales on turned lattices (swells of about 1.5 m, clumps of about 12 cm,
// blade flecks of about 2 cm drawn out along a slowly turning lean), each
// faded to its average once it is under a pixel, and dark gaps between the
// clumps where soil and shade show. pm is the plane point in metres. rgb the
// colour, w the clump height (0..1, for the far bump).
fn rc_foliage_far(b: RecBlock, f: MatFrame, pm: vec2<f32>) -> vec4<f32> {
    let sw = rc_fbm2(pm * 0.65, b.seed + 21u, 3u);
    let kc = rc_aa_world(f, 0.12);
    let cl = rc_fade(rc_fbm2(pm * 8.0 + vec2<f32>(3.1, 1.7), b.seed + 22u, 2u), 0.5, kc);
    let a = 0.9 * (rc_vnoise2(pm * 0.35, b.seed + 23u) - 0.5);
    let r = vec2<f32>(pm.x * cos(a) + pm.y * sin(a), pm.y * cos(a) - pm.x * sin(a));
    let kf = rc_aa_world(f, 0.025);
    let fl = rc_fade(rc_vnoise2(r * vec2<f32>(48.0, 14.0), b.seed + 24u), 0.5, kf);
    let col = mix(b.col_a, b.col_b, clamp(b.na.x * (0.4 + 1.1 * (sw - 0.5) + 0.7 * (fl - 0.5)), 0.0, 1.0));
    let gap = (1.0 - smoothstep(0.28, 0.46, cl)) * kc;
    var c = col * (0.5 + 0.55 * cl + 0.45 * (fl - 0.5) * kf);
    c = mix(c, mix(b.col_c * 0.45, col * 0.3, 0.5), clamp(gap * 0.7 + 0.1 * b.nb.z, 0.0, 1.0));
    return vec4<f32>(c * (0.8 + 0.4 * sw), cl);
}

// Ground patches over turf (numbers n9 `patches`, 0 off: every older block
// draws as it did): dry, straw-coloured grass in patches of 3 to 6 m and
// worn bare soil where a warped field passes a threshold the amount sets,
// more of it on slopes (up: the surface normal's y) and with a ragged,
// thinning edge. Smooth in the shader, so no patch edge follows the mesh.
fn rc_foliage_patch(b: RecBlock, f: MatFrame, pm: vec2<f32>, up: f32, c: vec3<f32>, cl: f32) -> vec3<f32> {
    let p = b.nc.y;
    if (p <= 0.0) {
        return c;
    }
    let w = vec2<f32>(rc_fbm2(pm * 0.21, b.seed + 31u, 2u), rc_fbm2(pm * 0.21 + vec2<f32>(7.3, 2.9), b.seed + 32u, 2u)) - 0.5;
    let q = pm + w * 3.5;
    let lum = dot(c, vec3<f32>(0.3, 0.6, 0.1));
    let dry = lum * vec3<f32>(1.5, 1.2, 0.55);
    let dm = smoothstep(0.5, 0.7, rc_fbm2(q * 0.2, b.seed + 33u, 3u)) * min(1.0, p * 1.8);
    var o = mix(c, dry, 0.7 * dm);
    // Round 9: smaller worn patches (about 3 m, r8's 6 m read as mud stains
    // over a quarter of the field) with soft, grassy edges.
    let sm = rc_fbm2(q * 0.3, b.seed + 34u, 4u) + clamp(1.0 - up, 0.0, 1.0) * 0.55 + (cl - 0.5) * 0.1;
    let th = 0.84 - 0.3 * p;
    let m = smoothstep(th - 0.05, th + 0.08, sm);
    let kp = rc_aa_world(f, 0.03);
    let cr = rc_fade(rc_vnoise2(pm * 38.0, b.seed + 35u), 0.5, kp);
    // The soil's own brown from colour C's lightness (a grass tint moves
    // C's hue too, round 6's mauve patches).
    let sl = dot(b.col_c, vec3<f32>(0.3, 0.6, 0.1));
    let soil = sl * vec3<f32>(2.5, 1.95, 1.3) * (0.6 + 0.55 * (cr - 0.5)) * (0.8 + 0.4 * rc_fbm2(pm * 1.7, b.seed + 36u, 2u));
    // Thin, dry grass at the patch edge.
    let edge = smoothstep(th - 0.08, th - 0.01, sm) * (1.0 - m);
    o = mix(o, mix(o, dry * 0.8, 0.6), edge);
    return mix(o, soil, m);
}

// The card variants at uv: rgb the colour, w the height. e is the pixel size
// in uv, k how much of the detail survives the anti-alias fade, bs the scale.
fn rc_foliage_u(b: RecBlock, uv: vec2<f32>, e: f32, k: f32, q: u32, bs: f32) -> vec4<f32> {
    let v = b.variant;
    let n = rc_vnoise2(uv * 5.0, b.seed);
    let x = abs(uv.x - 0.5);
    var c = mix(b.col_a, b.col_b, b.na.x * n);
    var h = 0.0;
    var dry = max(x * 2.0, 1.0 - uv.y);
    if (v == 0u) {
        // Leaf: a midrib, and side veins slanting to the tip.
        let ve = select(0.0, b.na.z, rc_extra(b, RC_X_FOLIAGE, 1u)) * k;
        var vx = x;
        var vy = uv.y;
        if (b.nc.x > 0.5) {
            let p = uv - vec2<f32>(0.5, 0.8);
            let a = atan2(p.x, -p.y);
            let t = a - 0.85 * clamp(round(a / 0.85), -2.0, 2.0);
            let r = length(p);
            vx = abs(r * sin(t));
            vy = 1.0 - r * cos(t);
        }
        let mid = 1.0 - smoothstep(0.0, 0.006 + 0.01 * vy + e, vx);
        let s = abs(fract((vy + vx * 0.8) * bs) - 0.5) / bs;
        let side = (1.0 - smoothstep(0.0, 0.002 + e, s)) * smoothstep(0.0, 0.02, vx) * (1.0 - smoothstep(0.3, 0.48, vx));
        let vm = max(mid, side * 0.7) * ve;
        c = mix(c, c * vec3<f32>(1.35, 1.35, 0.8) + 0.02, vm);
        h = -vm;
    } else if (v == 5u) {
        // Moss: cushions filling the ground, lighter on top, dark creases
        // between them, and fine fronds.
        let dome = smoothstep(0.3, 0.7, rc_fbm2(uv * bs * 0.3, b.seed + 5u, 3u));
        var fr = rc_vnoise2(uv * bs * 2.0, b.seed + 6u);
        if (q >= 3u) {
            fr = fr * 0.7 + 0.3 * rc_vnoise2(uv * bs * 5.0, b.seed + 7u);
        }
        fr = rc_fade(fr, 0.5, k);
        c = mix(b.col_a * 0.6, mix(b.col_a, b.col_b, dome * (0.5 + b.na.x)), smoothstep(0.0, 0.8, dome + fr * 0.3)) * (0.8 + 0.4 * fr * (0.5 + 0.5 * b.na.z));
        h = dome + fr * 0.15;
        dry = dome;
    } else {
        // Needles from the twig, or grass and straw strands along v.
        let nd = v == 1u;
        let sv = select(uv.x * bs + rc_vnoise2(vec2<f32>(uv.y * 3.0, uv.x * 4.0), b.seed + 2u), (uv.y + x * 0.7) * bs, nd);
        let a = fract(sv);
        let hh = rc_u01(rc_hash2(vec2<i32>(i32(floor(sv)), 0), b.seed + 3u));
        let al = select(1.0 - uv.y, x * 2.0, nd);
        let rd = sin(a * 3.1416);
        c = mix(b.col_a, b.col_b, b.na.x * hh) * mix(1.0, select(0.35 + 0.75 * rd, 0.65 + 0.45 * rd, v == 3u) * select(1.0, 0.6 + 0.4 * al, v == 2u), k);
        h = rd * 0.5;
        dry = al + 0.15 * hh;
        if (nd && rc_extra(b, RC_X_FOLIAGE, 2u)) {
            // A berry on some needles, part way along.
            let bm = (1.0 - smoothstep(0.3, 0.4 + e * bs, length(vec2<f32>(a - 0.5, (al - fract(hh * 7.0)) * bs * 0.4)))) * step(hh, b.nb.z * 0.4) * k;
            c = mix(c, b.col_b, bm);
            h = max(h, bm);
        } else if (v == 2u && rc_extra(b, RC_X_FOLIAGE, 2u)) {
            // Seed heads (wheat ears, meadow grass): the top of a share of
            // the strands turns colour B, grainy with rows of kernels.
            let hd = smoothstep(0.6, 0.68, al) * step(hh, b.nb.z);
            c = mix(c, b.col_b * (1.1 + 0.3 * rc_fade(fract(al * bs * 1.6 + hh), 0.5, k)), hd);
        }
    }
    return vec4<f32>(rc_foliage_dry(b, c, dry + (n - 0.5) * 0.2), h);
}

fn rc_foliage(b: RecBlock, f: MatFrame, i: MatIn, q: u32) -> MatOut {
    let v = b.variant;
    let wv = v == 4u || v == 6u;
    let bs = rc_range(b.nb.y, 2.0, 200.0);
    let ng = normalize(i.g);
    let pw = rc_proj(i.world, ng) * bs;
    let n0 = rc_vnoise2(select(i.uv * 5.0, pw * 0.1, wv), b.seed);
    var fc = mix(b.col_a, b.col_b, b.na.x * n0);
    let pm = rc_proj(i.world, ng);
    var far = vec4<f32>(0.0);
    if (v == 4u) {
        // Far turf: rc_foliage_far (round 6), the mowing stripes kept.
        far = rc_foliage_far(b, f, pm);
        fc = mix(far.rgb, far.rgb * mix(0.7 + 0.6 * n0, 0.9 + 0.2 * n0, b.nb.w), b.nb.w) * rc_foliage_band(b, rc_foliage_mow(b, pw));
    } else if (wv) {
        // Far litter: clumps (about 10 blades across) with lit tops
        // and dark creases, so a lawn keeps its relief where single blades
        // are under a pixel, and on Low and Lite, which edit no normals.
        fc = mix(fc * 0.75, b.col_c * 0.6, 0.25 + 0.5 * b.nb.z) * mix(0.7 + 0.6 * n0, 0.9 + 0.2 * n0, b.nb.w) * rc_foliage_band(b, rc_foliage_mow(b, pw));
    }
    let rough = mix(select(0.72, 0.9, v >= 4u), 0.2, b.na.w);
    var k = rc_aa(f, 0.5 / bs);
    var s: vec4<f32>;
    let ew = max(0.03, f.world_px * bs);
    let nw = select(2u, 3u, q >= 3u);
    // Turf from Low up stands its blades up (rc_foliage_stand): 6 steps on
    // Low, 8 on Medium, 10 on High and Ultra; Lite and far turf keep the
    // painted blades.
    // (Only on faces turned up: the lawn shape's blade spikes take the
    // painted colour.)
    let stand = v == 4u && q >= 1u && ng.y > 0.5;
    let vn = max(abs(dot(f.v, ng)), 0.15);
    if (stand) {
        let dens = smoothstep(b.nb.z * 0.9 - 0.05, b.nb.z * 0.9 + 0.1, select(1.0, rc_fbm2(pw * 0.08, b.seed + 9u, 2u), rc_extra(b, RC_X_FOLIAGE, 2u)));
        let ns = select(select(6u, 8u, q >= 2u), 10u, q >= 3u);
        let st = rc_foliage_stand(b, pw * 2.0, rc_proj(f.v, ng) / vn, ew * 2.0, ns, dens);
        k = smoothstep(0.6, 1.6, 0.5 / (bs * max(f.world_px, 1e-7)));
        s = vec4<f32>(mix(fc, st.rgb, k), st.w);
    } else if (wv) {
        k = rc_aa_world(f, 0.3 / bs);
        s = rc_foliage_w(b, pw, ew, nw);
        s = vec4<f32>(mix(fc, s.rgb, k), s.w);
    } else {
        s = rc_foliage_u(b, i.uv, f.uv_px, k, q, bs);
    }
    if (v == 4u) {
        s = vec4<f32>(rc_foliage_patch(b, f, pm, normalize(i.n).y, s.rgb, far.w), s.w);
    }
    var o = rc_out(i, s.rgb, rough, 0.0);
    if (v == 4u && q >= 2u && k < 0.99) {
        // The far clumps' relief, so a low sun rakes across the lawn where
        // single blades are under a pixel.
        let t = rc_tw(ng);
        let es = 0.05;
        let d = vec2<f32>(rc_foliage_far(b, f, pm + vec2<f32>(es, 0.0)).w, rc_foliage_far(b, f, pm + vec2<f32>(0.0, es)).w) - far.w;
        o.n = rc_bump(o.n, t, d * (1.6 * (1.0 - k)));
    }
    if (v == 4u && q >= 1u) {
        // Seen low over the lawn the eye meets blade sides, which hide the
        // sky's sheen and the look's rim (round 10's pale veil on the far
        // half of the lawn): fully rough, and occluded at grazing views.
        o.rough = 1.0;
        o.occ = i.occ * mix(0.2, 1.0, smoothstep(0.15, 0.75, vn));
    }
    // Seen from behind, a card's uv turns the other way round on screen.
    let back = !wv && f.duvx.x * f.duvy.y - f.duvx.y * f.duvy.x < 0.0;
    if (q >= 2u) {
        if (back) {
            o.base = vec4<f32>(mix(o.base.rgb, b.col_c * i.tex.rgb * i.vcol, b.nb.w), o.base.a);
        }
        let near = rc_near(f) * k;
        if (near > 0.0) {
            let es = 0.25 / bs;
            if (stand) {
                // The blades' own shading is in their colour; no bump.
            } else if (wv) {
                let t = rc_tw(ng);
                let d = vec2<f32>(rc_foliage_w(b, rc_proj(i.world + t[0] * es, ng) * bs, ew, nw).w, rc_foliage_w(b, rc_proj(i.world + t[1] * es, ng) * bs, ew, nw).w) - s.w;
                o.n = rc_bump(i.n, t, d * (0.8 * near));
            } else {
                let d = vec2<f32>(rc_foliage_u(b, i.uv + vec2<f32>(es, 0.0), f.uv_px, k, q, bs).w, rc_foliage_u(b, i.uv + vec2<f32>(0.0, es), f.uv_px, k, q, bs).w) - s.w;
                o.n = rc_bump(i.n, rc_tuv(f, i.n), d * (0.8 * near));
            }
        }
        if (rc_extra(b, RC_X_FOLIAGE, 0u)) {
            // Back-light: the sun through the leaf, strongest looking into it;
            // snow on the leaf stops it.
            let l = frame.light_dir.xyz;
            let fs = pow(max(-dot(l, f.v), 0.0), 3.0);
            let tr = (1.0 - b.na.y) * select(max(-dot(ng, l), 0.0) * (0.9 + 3.0 * fs), 0.7 * fs * s.w * k, wv);
            let bc = select(b.col_c, b.col_a * 1.6 + 0.03, wv);
            o.emit = bc * i.tex.rgb * i.vcol * frame.light_color.rgb * (tr * i.occ * (1.0 - 0.9 * rc_amount(b, 3u)));
        }
    }
    if (q >= 3u && b.na.w > 0.3 && !back) {
        // Sun glints on waxy leaves.
        let hv = normalize(frame.light_dir.xyz + f.v);
        o.glint = vec3<f32>(pow(max(dot(o.n, hv), 0.0), 200.0) * (b.na.w - 0.3) * 1.5);
    }
    return o;
}
