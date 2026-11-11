// evani.shader.rc.thin
// Thin (recipe 21, batch 3 build plan 3.3, package B3-P10b). Written fresh for
// Evani.Dev: printed and papery surfaces, in uv space (a uv unit is a metre on
// a wall). Light through paper, petals and membranes stays the nature pass's
// (the adapter writes its kind for those variants); this stage draws only the
// wallpaper and gives every other variant its plain colour, so a material
// that also reaches the nature pass looks as it did.
//   numbers: na = thickness, pattern (how strongly the print shows), motif
//            scale (1-32 repeats per uv), peel; nb = fade, dots, film, cross
//   colours: A colour (the paper), B light through (for wallpaper the print),
//            C under (the plaster a peeled patch shows)
//   variants: 0 paper, 1 wax, 2 petal, 3 membrane, 4 lampshade, 5 wallpaper,
//             6 newsprint (motif scale columns per uv of fake type in colour
//             B: lines of word-like dashes, never letters, headlines and
//             halftone picture boxes), 7 lace (thread colour A; its holes,
//             net and flowers are the cut-out's, shaders/cut.wgsl kind 5);
//             P7-2 (batch 7): 8 wing (a card's uv: u across, the base at v 1;
//             veins in C fanning out from the base, motif scale of them,
//             each forking in two halfway out, cross veins by `cross`
//             (staggered strip to strip, so they mesh like a dragonfly's), a tone
//             per cell, and a margin in C round the tip as wide as `pattern`
//             with pale spots in it by `dots`), 9 segments (motif scale
//             bands across v, each lighter towards its free edge, a dark
//             joint in C, glossy: roughness 0.3)
//   extras: stripes (bit 0: a broad band and two pinstripes per repeat, along
//           v), motif (bit 1: the layout helper's painted motif in a half-drop
//           repeat, leaves in the corners), peel (bit 2: curled-back patches
//           showing C, likelier at the joins between drops, a lit rolled edge
//           and a shadow under it), damask (bit 3, material-view round 4: a
//           woven tone-on-tone figure in colour B, mirrored across each
//           repeat in a half drop: a pointed-oval medallion round a
//           palmette, scrolls outside it and a fleuron between drops; the
//           figure is satin, smooth and glossy with a sheen and its weave
//           across, on a matte ground woven the other way),
//           film (bit 4, P7-2; bit 3 until it joined round 4's damask:
//           every variant but wallpaper, newsprint and lace, a thin-film
//           hue by view angle, the paint recipe's film maths, blue face-on,
//           drifting across the surface in soft patches, strength `film`);
//           dots (a number: scattered pigment dots in C, 4 x motif scale
//           cells per uv, each dot inside its own cell, how many by `dots`;
//           a wing's go in its margin, pale)
// Petal (material-view round 4): deeper towards the base (v = 1, the card's
// stalk end) and a velvet sheen in colour B from medium; dots and film
// apply to it as to every other non-wallpaper variant.
// Levels (q, the detail group): lite draws what low draws (every tier keeps
// the pattern): the print (stripes and motif, anti-aliased), the fade, the
// joins, the paper grain and the peeled patches; medium adds the rolled edges and
// the step down to the plaster as normals within 25 m; high and ultra draw as
// medium.

// The print at uv: x how much of colour B, y how dark it is there (the
// motif's centres darker, its leaves lighter); the pinstripe faded by ks,
// the wide band and the motif by km, to their averages.
fn rc_thin_ink(b: RecBlock, uv: vec2<f32>, ms: f32, ks: f32, km: f32) -> vec2<f32> {
    var o = vec2<f32>(0.0, 1.0);
    let p = uv * ms;
    if (rc_extra(b, RC_X_THIN, 0u)) {
        let d = abs(fract(p.x) - 0.5);
        o.x = max(rc_fade(1.0 - smoothstep(0.16, 0.18, d), 0.35, km) * 0.55, rc_fade(1.0 - smoothstep(0.008, 0.016, abs(d - 0.3)), 0.05, ks));
    }
    if (rc_extra(b, RC_X_THIN, 1u)) {
        let r = p + vec2<f32>(0.0, 0.5 * rc_lay_par(floor(p.x)));
        let m = 1.0 - smoothstep(vec3<f32>(0.0), vec3<f32>(0.05), rc_lay_motif(fract(r) * 2.0 - 1.0, b.seed, 0.12, rc_u01(rc_hash2(vec2<i32>(floor(r)), b.seed))));
        o = vec2<f32>(max(o.x, rc_fade(max(m.x, m.z * 0.7), 0.3, km)), mix(mix(1.0, 1.35, m.z * (1.0 - m.x)), 0.55, m.y * km));
    }
    return o;
}

// The damask figure at p (motif repeats): 1 on the figure, faded by k to
// its average.
fn rc_thin_dam(p: vec2<f32>, k: f32) -> f32 {
    var q = fract(p + vec2<f32>(0.0, 0.5 * rc_lay_par(floor(p.x)))) * 2.0 - 1.0;
    q.x = abs(q.x);
    var d = abs(length(vec2<f32>(q.x + 0.55, q.y * 0.9)) - 1.0) - 0.035;
    let pc = q - vec2<f32>(0.0, -0.1);
    d = min(d, length(pc) - 0.36 * (0.45 + 0.55 * abs(cos(atan2(q.x, -pc.y) * 2.5))));
    d = min(d, max(q.x - 0.03, abs(q.y - 0.5) - 0.32));
    d = min(d, max(abs(length(q - vec2<f32>(0.82, 0.55)) - 0.2) - 0.035, q.y - 0.62));
    let cq = vec2<f32>(1.0 - q.x, 1.0 - abs(q.y));
    d = min(d, length(cq) - 0.17 * (0.55 + 0.45 * abs(cos(atan2(cq.y, cq.x) * 4.0))));
    return rc_fade(1.0 - smoothstep(-0.03, 0.03, d), 0.3, k);
}

// Peeling: x 1 where the paper is gone (the plaster shows), y the rolled edge
// on the paper's side of a patch, z the shadow on the plaster beside it.
// Patches are likelier at the joins between drops (0.53 apart).
fn rc_thin_peel(b: RecBlock, uv: vec2<f32>) -> vec3<f32> {
    let n = rc_fbm2(uv * vec2<f32>(2.5, 1.2), b.seed + 3u, 4u) + 0.22 * (1.0 - smoothstep(0.0, 0.06, rc_lay_stripe(uv.x, 0.53))) - 1.0 + 0.55 * b.na.w;
    let x = smoothstep(0.0, 0.005, n);
    return vec3<f32>(x, smoothstep(-0.03, 0.0, n) - x, x - smoothstep(0.004, 0.04, n));
}

// Newsprint at uv: how much ink (0..1). Columns ms per uv with gutters; ten
// lines a column wide, each a run of character cells (thirty a column) that
// are ink or a space by a hash, so the dashes read as words and never as
// letters; some blocks of lines are headlines (half as many, heavier) or a
// halftone picture box. kc fades the characters, kl the lines, to their
// average.
fn rc_thin_news(b: RecBlock, uv: vec2<f32>, ms: f32, kc: f32, kl: f32) -> f32 {
    let col = uv.x * ms;
    let cx = floor(col);
    let lx = col - cx;
    let gut = step(0.05, lx) * step(lx, 0.95);
    let blk = floor(uv.y * ms * 1.5);
    let kind = rc_u01(rc_hash2(vec2<i32>(i32(cx), i32(blk)), b.seed + 30u));
    if (kind < 0.12) {
        // A picture: a halftone of a soft blob, in a thin frame.
        let p = fract(uv.y * ms * 1.5);
        let tone = rc_fbm2(vec2<f32>(col, uv.y * ms) * 3.0, b.seed + 31u, 3u);
        let dots = 1.0 - smoothstep(0.0, 0.25, length(fract(uv * ms * 60.0) - 0.5) - 0.45 * tone);
        return gut * step(0.06, p) * step(p, 0.94) * rc_fade(dots, tone * 0.8, kc);
    }
    let head = kind < 0.22;
    let lines = select(15.0, 7.5, head);
    let ly = uv.y * ms * lines;
    let row = floor(ly);
    let fy = ly - row;
    let band = smoothstep(0.2, 0.26, fy) * (1.0 - smoothstep(0.7, 0.76, fy));
    let cw = select(30.0, 16.0, head);
    let k = floor(lx * cw);
    let hk = rc_u01(rc_hash2(vec2<i32>(i32(k + cx * 97.0), i32(row)), b.seed + 32u));
    // Spaces between words, and a paragraph's last line ending early.
    let word = step(0.17, hk) * step(k, cw * mix(0.35, 1.0, step(0.15, rc_u01(rc_hash2(vec2<i32>(i32(cx), i32(row)), b.seed + 33u)))));
    let glyph = 1.0 - smoothstep(0.3, 0.42, abs(fract(lx * cw) - 0.5));
    let ink = rc_fade(word * glyph * select(0.75, 1.0, head), select(0.4, 0.62, head), kc);
    return gut * rc_fade(band * ink, select(0.2, 0.34, head), kl);
}

fn rc_thin(b: RecBlock, f: MatFrame, i: MatIn, q: u32) -> MatOut {
    if (b.variant == 6u) {
        let ms = rc_range(b.na.z, 1.0, 32.0);
        let lines = rc_thin_news(b, i.uv, ms, rc_aa(f, 0.03 / ms), rc_aa(f, 0.06 / ms));
        let c = mix(b.col_a * (0.96 + 0.08 * rc_fade(rc_vnoise2(i.uv * 300.0, b.seed + 2u), 0.5, rc_aa(f, 0.003))), b.col_b, lines * b.na.y);
        return rc_out(i, c, 0.85, 0.0);
    }
    if (b.variant == 7u) {
        // Lace: the thread, a little uneven.
        return rc_out(i, b.col_a * (0.9 + 0.12 * rc_fade(rc_vnoise2(i.uv * 220.0, b.seed + 2u), 0.5, rc_aa(f, 0.005))), 0.8, 0.0);
    }
    if (b.variant != 5u) {
        let v = b.variant;
        let ms = rc_range(b.na.z, 1.0, 32.0);
        // A petal deepens towards its base (v = 1, the stalk end).
        var c = b.col_a * select(1.0, mix(1.0, 0.55, smoothstep(0.45, 1.0, i.uv.y)), v == 2u);
        var dp = i.uv * ms * 4.0;
        var mk = 1.0;
        var ink = b.col_c;
        if (v == 8u) {
            // Wing: veins fan out from the base, each forks halfway out.
            let d = vec2<f32>(i.uv.x - 0.5, 1.05 - i.uv.y);
            let r = length(d);
            let n = ms * select(1.0, 2.0, r > 0.5);
            let a = atan2(d.x, d.y) * 0.3183 * n;
            let x = r * ms * 0.5 + fract(floor(a) * 0.618);
            let k = rc_aa(f, r / n);
            let vein = max(1.0 - smoothstep(0.02, 0.05, 0.5 - abs(fract(a) - 0.5)), b.nb.w * (1.0 - smoothstep(0.03, 0.06, 0.5 - abs(fract(x) - 0.5))));
            mk = smoothstep(0.0, 0.03, r - 1.2 + 0.45 * b.na.y);
            c = mix(c * rc_fade(0.92 + 0.16 * rc_u01(rc_hash2(vec2<i32>(floor(vec2<f32>(a, x))), b.seed)), 1.0, k), ink, max(rc_fade(vein, 0.1, k), mk));
            dp = vec2<f32>(a * 2.0, r * ms * 3.0);
            ink = mix(b.col_a, vec3<f32>(0.9), 0.85);
        }
        if (v == 9u) {
            // Segments: each lighter towards its free edge, a dark joint.
            let x = fract(i.uv.y * ms);
            let k = rc_aa(f, 0.15 / ms);
            c = mix(c * (0.8 + 0.35 * rc_fade(x, 0.5, k)), ink, rc_fade(1.0 - smoothstep(0.0, 0.1, x), 0.1, k));
        }
        if (b.nb.y > 0.0) {
            // Dots, each inside its own cell.
            let h = rc_hash2(vec2<i32>(floor(dp)), b.seed + 9u);
            let rr = 0.12 + 0.25 * rc_u01(rc_pcg(h));
            let dd = length(fract(dp) - 0.5 - (vec2<f32>(rc_u01(h), rc_u01(rc_pcg(h + 2u))) - 0.5) * (0.9 - 2.0 * rr));
            c = mix(c, ink, mk * rc_fade((1.0 - smoothstep(rr * 0.5, rr, dd)) * step(rc_u01(rc_pcg(h + 1u)), b.nb.y), b.nb.y * 0.1, rc_aa(f, 0.08 / ms)));
        }
        if (rc_extra(b, RC_X_THIN, 4u)) {
            // Film: the hue turns with the view.
            c = mix(c, (vec3<f32>(0.5) + 0.5 * cos(6.2832 * (vec3<f32>(dot(i.n, f.v) * 1.2 + 0.6 * rc_vnoise2(i.uv * 4.0, b.seed + 3u)) + vec3<f32>(-0.17, 0.16, 0.5)))) * (c + vec3<f32>(0.25)), b.nb.z * 0.7);
        }
        var o = rc_out(i, c, select(i.rough, 0.3, v == 9u), 0.0);
        if (v == 2u && q >= 2u) {
            // The petal's velvet sheen.
            o.sheen = vec4<f32>(b.col_b * 0.5, 3.0);
        }
        return o;
    }
    let ms = rc_range(b.na.z, 1.0, 32.0);
    let peel = rc_extra(b, RC_X_THIN, 2u) && b.na.w > 0.0;
    var c = b.col_a;
    var pe = vec3<f32>(0.0);
    if (peel) {
        pe = rc_thin_peel(b, i.uv);
    }
    let ink = rc_thin_ink(b, i.uv, ms, rc_aa(f, 0.012 / ms), rc_aa(f, 0.05 / ms));
    // Fade: the print paler in broad sun-bleached patches, the paper yellowed.
    let fd = b.nb.x * (0.55 + 0.45 * rc_fbm2(i.uv * 1.3, b.seed + 7u, 3u));
    c = mix(c * mix(vec3<f32>(1.0), vec3<f32>(1.0, 0.94, 0.82), 0.5 * fd), b.col_b * ink.y, ink.x * b.na.y * (1.0 - fd));
    var dm = 0.0;
    if (rc_extra(b, RC_X_THIN, 3u)) {
        dm = rc_thin_dam(i.uv * ms, rc_aa(f, 0.04 / ms));
        let wv = rc_fade(0.5 + 0.5 * sin(select(i.uv.x, i.uv.y, dm > 0.5) * ms * 160.0), 0.5, rc_aa(f, 0.006 / ms));
        c = mix(c, b.col_b, dm * b.na.y * (1.0 - fd)) * (0.97 + 0.06 * wv);
    }
    // The joins between drops, the paper's grain, then the plaster in C
    // where it peeled, a lit rolled edge and a shadow beside it.
    c = c * (1.0 - 0.25 * rc_fade(1.0 - smoothstep(0.0008, 0.0016, rc_lay_stripe(i.uv.x, 0.53)), 0.003, rc_aa(f, 0.002))) * (0.97 + 0.06 * rc_fade(rc_vnoise2(i.uv * 400.0, b.seed + 2u), 0.5, rc_aa(f, 0.0025)));
    c = mix(c * (1.0 + 0.25 * pe.y), b.col_c * (0.88 + 0.2 * rc_vnoise2(i.uv * 30.0, b.seed + 4u)) * (1.0 - 0.45 * pe.z), pe.x);
    var o = rc_out(i, c, mix(0.8, 0.3, dm) + 0.15 * pe.x, 0.0);
    if (q >= 2u && dm > 0.0) {
        o.sheen = vec4<f32>(b.col_b * (0.4 * dm), 4.0);
    }
    let near = rc_near(f) * rc_aa(f, 0.004);
    if (q >= 2u && peel && near > 0.0) {
        // The paper a step above the plaster, its rolled edge raised.
        let e = vec2<f32>(0.003, 0.0);
        let h = vec3<f32>(0.6, -0.4, 0.0);
        o.n = rc_bump(i.n, rc_tuv(f, i.n), (vec2<f32>(dot(rc_thin_peel(b, i.uv + e).yxz, h), dot(rc_thin_peel(b, i.uv + e.yx).yxz, h)) - dot(pe.yxz, h)) * (near * 1.333));
        o.occ = i.occ * (1.0 - 0.3 * pe.z);
    }
    return o;
}
