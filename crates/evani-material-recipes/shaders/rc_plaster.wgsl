// evani.shader.rc.plaster
// Plaster (recipe 8, batch 1 build plan section 3, package P5). Written fresh
// for Evani.Dev: trowel marks are the contour lines of a noise, cracks the
// bisectors of a warped cellular noise shown only where a second noise
// allows. World space (plan 2.3): one dominant-axis projection up to Medium,
// two blended from High.
//   numbers: na = rough, trowel amount, trowel scale (1-50 per m), crack
//            amount; nb = crack scale (1-50 per m), aggregate (air holes,
//            sand flecks, and how many cells hold a pebble), chunk amount,
//            aggregate size (0.005-0.05 m)
//   colours: A colour, B crack and stain, C lath (what shows where chunks
//            are missing)
//   variants: 0 smooth, 1 stucco, 2 concrete, 3 limewash, 4 polished (a
//             soft cloudy mottle; set the roughness low), 5 cast (the layered
//             concrete surface in object space: rc_plaster_concrete.wgsl, its own
//             numbers there)
//   extras: cracks (bit 0), trowel relief (bit 1), chunks (bit 2: patches
//           broken off, narrow lath strips in C showing), aggregate (bit 3:
//           pebbles standing out of the surface, each its own grey),
//           boards (bit 4: board-marked concrete, planks 15 cm high with
//           grain streaks)
// Levels (q, the detail group): lite draws what low draws
// (every tier keeps the pattern; before critic round 1 lite was the colour with one soft noise); low
// draws the mottle, trowel lines, stipple, air holes and cracks, anti-aliased
// (detail under 1.5 px fades to its average); medium adds the relief as
// normals within 25 m; high adds sand flecks and the blended projection;
// ultra draws as high.

// Where plaster is broken off at p (0 to 1), in patches of a slow noise the
// chunk amount widens.
fn rc_plaster_chunk(b: RecBlock, p: vec2f) -> f32 {
    return smoothstep(0.0, 0.03, rc_fbm2(p * 2.2 + vec2f(3.0, 7.0), b.seed + 13u, 3u) - (0.72 - 0.3 * b.nb.z));
}

// The surface at plane point p. k holds how much trowel, grain and crack
// detail survive the anti-alias fade (w the pebbles). x brightness, y crack
// mask, z air hole mask, w height (metres).
fn rc_plaster_s(b: RecBlock, p: vec2f, k: vec4f) -> vec4f {
    let v = b.variant;
    let ts = rc_range(b.na.z, 1.0, 50.0);
    var tone = 1.0 + (rc_fbm2(p * 0.9, b.seed, 3u) - 0.5) * select(0.14, 0.3, v == 3u);
    var pit = 0.0;
    var h = 0.0;
    if (v == 3u) {
        // Limewash: soft cloudy brush streaks.
        tone = tone + (rc_fbm2(p * vec2f(0.4, 2.5) * ts, b.seed + 1u, 3u) - 0.5) * 0.6 * b.na.y;
    } else {
        // Trowel marks: where one pass overlaps the last, a faint line and a
        // low ridge, along the contour of a noise.
        let t = rc_fbm2(p * ts * 0.8, b.seed + 1u, 3u);
        let l = 1.0 - smoothstep(0.0, 0.03, abs(t - 0.5));
        tone = tone + (t - 0.5) * 0.3 * b.na.y - l * 0.08 * b.na.y * k.x;
        if (rc_extra(b, RC_X_PLASTER, 1u)) {
            h = smoothstep(0.47, 0.53, t) * 0.0012 * b.na.y;
        }
    }
    if (v == 1u) {
        // Stucco: a stipple of two scales.
        let s = rc_vnoise2(p * 45.0, b.seed + 2u) * 0.6 + rc_vnoise2(p * 110.0, b.seed + 3u) * 0.4;
        tone = tone + (rc_fade(s, 0.5, k.y) - 0.5) * 0.25;
        if (rc_extra(b, RC_X_PLASTER, 1u)) {
            h = h + (s - 0.5) * 0.006;
        }
    }
    if (v == 2u) {
        // Concrete: air holes, some cells of a fine grid.
        let c = rc_voronoi2(p * 55.0, b.seed + 4u, 1.0);
        pit = (1.0 - smoothstep(0.0, 0.1 + 0.1 * c.w, c.x)) * step(c.w, b.nb.y * 1.5 + 0.1) * k.y;
        h = h - pit * 0.002;
    }
    if (v == 4u) {
        tone = tone + (rc_fbm2(p * 0.4, b.seed + 10u, 3u) - 0.5) * 0.3;
    }
    if (rc_extra(b, RC_X_PLASTER, 3u)) {
        // Pebbles: a round, domed stone in most cells of a grid the size
        // asked for.
        let a = rc_voronoi2(p / rc_range(b.nb.w, 0.005, 0.05), b.seed + 10u, 1.0);
        let r = 0.3 + 0.2 * fract(a.w * 7.0);
        let m = (1.0 - smoothstep(r - 0.06, r, a.x)) * step(fract(a.w * 13.0), 0.35 + b.nb.y * 0.65) * k.w;
        tone = mix(tone, 0.55 + 0.9 * fract(a.w * 29.0), m);
        h = h + m * (1.0 - a.x / r) * 0.004;
    }
    if (rc_extra(b, RC_X_PLASTER, 4u)) {
        // Board marks: planks with their own tone, a line between, grain.
        let r = floor(p.y / 0.15);
        let ly = p.y - r * 0.15;
        let st = rc_vnoise2(vec2f(p.x * 0.7, p.y * 40.0), b.seed + 11u);
        let jn = 1.0 - smoothstep(0.002, 0.005, min(ly, 0.15 - ly));
        tone = tone + (rc_u01(rc_hash2(vec2<i32>(i32(r), 0), b.seed + 12u)) - 0.5) * 0.14 + (rc_fade(st, 0.5, k.y) - 0.5) * 0.3 - jn * 0.15 * k.y;
        h = h + (st - 0.5) * 0.0015 - jn * 0.002;
    }
    if (rc_extra(b, RC_X_PLASTER, 2u)) {
        h = h - rc_plaster_chunk(b, p) * 0.005;
    }
    var crack = 0.0;
    if (rc_extra(b, RC_X_PLASTER, 0u) && b.na.w > 0.0) {
        // Cracks: a jagged net of cell boundaries, only where a slow noise
        // passes a threshold the crack amount lowers.
        let cs = rc_range(b.nb.x, 1.0, 50.0);
        var q = p * cs;
        q = q + (vec2f(rc_vnoise2(q * 3.0, b.seed + 5u), rc_vnoise2(q * 3.0 + vec2f(5.2, 1.3), b.seed + 6u)) - vec2f(0.5)) * 0.5;
        let z = rc_voronoi2(q, b.seed + 7u, 1.0).z;
        let th = 0.7 - 0.4 * b.na.w;
        let g = smoothstep(th, th + 0.04, rc_fbm2(p * cs * 0.35, b.seed + 8u, 2u));
        crack = (1.0 - smoothstep(0.0, 0.05, z)) * g * k.z;
        tone = tone * (1.0 - 0.25 * g * (1.0 - smoothstep(0.0, 0.4, z)));
        h = h - crack * 0.003;
    }
    return vec4f(tone, crack, pit, h);
}

fn rc_plaster(b: RecBlock, f: MatFrame, i: MatIn, q: u32) -> MatOut {
    if (b.variant == 5u) {
        // Cast concrete: the layered concrete surface (rc_plaster_concrete.wgsl).
        return rc_plaster_cs(b, f, i, q);
    }
    if (b.variant == 6u) {
        // The faceted ball's cast panels (only it uses 6): the same concrete
        // with its pattern 1 + 7 * colour C's red times finer (pores and
        // grain at a sculpture's size), and joints and worn ridges read from
        // that much gentler hollows (convex edges from sharper ones, so panel
        // edges never whiten into a pale rim). Written compactly (vec3f,
        // .5 for 0.5): the ball's block fills the plaster stage's share.
        let s = 1. + 7. * b.col_c.x;
        var b5 = b;
        b5.variant = 5u;
        // Round 5: the shared layers' big blowholes and the cube's stain
        // layer (whose lip ringed every pit with light) off; the ball draws
        // its own pits, patches and water marks below.
        b5.nb.w = .0;
        b5.nc = vec2f();
        var f5 = f;
        f5.dlx *= s;
        f5.dly *= s;
        f5.curv *= select(.5, s, f.curv < .0);
        var i5 = i;
        i5.local *= s;
        if (dot(i.ln, i.ln) > .25) {
            // One cast piece over a panel's three faces and its creases: the
            // projection planes follow the direction from the ball's centre
            // (colour C's blue: its height, a tenth of a metre a unit), not
            // each flat face's normal (round 3 drew a tone step at every
            // crease). Sharpened, so mostly one plane is drawn.
            let rn = select(i.ln, normalize(i.local - vec3f(.0, b.col_c.z * 10., .0)), b.col_c.z > .0);
            i5.ln = normalize(rn * rn * rn);
        }
        var o = rc_plaster_cs(b5, f5, i5, q);
        let p = i.local;
        // Round 5's middle scale (colour C's green, `patches`): a soft cloudy
        // mottle of five octaves (about 8 cm down to a 4 mm sand grain),
        // smeared down the ball as laitance and wash run (round 4: hard-edged
        // islands, a camouflage look); larger soft water marks (`stains`,
        // nc.x) and thin runs down the sides (`streaks`, nc.y). In 3D, so a
        // panel's faces share it.
        let wn = rc_vnoise3(p * 9., b.seed + 70u);
        var cq = p * vec3f(13., 3., 13.) + wn * .5;
        var m = .0;
        for (var k = 1u; k < 6u; k++) {
            m += (rc_vnoise3(cq, b.seed + 90u + k) - .5) / f32(k);
            cq *= 2.1;
        }
        // Round 7 (round 6's review: dark smoky swirls, marble): the mottle
        // mostly paler than the paste (laitance and efflorescence), its
        // darker side a fifth as strong, the warp halved (broad soft clouds,
        // not veins); a fine sand grit (2 mm grains, faded where a grain is
        // under a pixel and a half).
        // Round 8 (round 7's review: the pale clouds read as sky): the
        // deposits a dry mask, its edge broken by the grit and stretched
        // down the ball into run-off streaks; round 9 (its review: hard
        // on/off edges like peeling paint, too much fine detail): the edge
        // feathered, the grit softer.
        let g = rc_vnoise3(p * 500., b.seed + 85u);
        let e = smoothstep(.0, .2, m + (g - .5) * .04);
        let t = 1. + b.col_c.y * (e * .22 + m * .2) + (g - .5) * .02 * rc_aa_world(f, .003) - b.nc.x * max(rc_vnoise3(p * 3., b.seed + 81u) - .4, .0) - b.nc.y * max(rc_vnoise3(p * vec3f(45., 2.5, 45.), b.seed + 82u) - .6, .0);
        // The darker paste smoother: a satin from the `rough` slider down to
        // 0.35 (the shared layers stop at 0.6 without the cube's stains);
        // round 6 three times round 5's range, for a soft sheen on the lit side.
        // round 8: the deposits rougher, the paste between them smoother
        // (a satin sheen on the lit side), in the deposits' broad shapes.
        // Round 10 (round 9's review: still a two-level map, no satin): one
        // broad range (the deposits fade in over a wide band of the mottle,
        // and the low cloud wavers it inside each zone), the paste glossier
        // (down to 0.22; the ball's `rough` 0.45) and the deposits closer to
        // it, so the sunlit faces take an even satin, not wet-looking spots.
        // Round 11: the paste down to 0.12 (still no satin at 0.22), the
        // grit a fifth quieter (the close-up still busier than the reference).
        let dr = b.na.x - clamp(b.na.x, .6, .98) + (smoothstep(-.2, .4, m) * .25 + wn * .2 - .4) * b.col_c.y;
        // The pits (`blowholes`): air voids in 3D cells 10 mm across (round on
        // any face, running on across a crease), in clusters with clean
        // concrete between (about 60% of the area), each sized by its own
        // draw (mostly small, 1.5 mm up to a rare 12 mm that reads from the
        // reference's distance; round 7), its outline broken by the mottle's
        // finer octaves (round 5's were perfect circles: drilled); a dent
        // with no lip, darker towards its heart. x how far inside, yzw the
        // offset from its centre (cells).
        let u = p * 100.;
        let dn = b.nb.w * 6. * smoothstep(.45, .8, wn);
        var pt = vec4f();
        for (var k = 0; k < 27; k++) {
            let c = vec3i(floor(u) - 1.) + vec3i(k % 3, k / 3 % 3, k / 9);
            let h = rc_hash3(c, b.seed + 84u);
            let d = u - vec3f(c) - fract(rc_u01(h) * vec3f(1., 7.13, 13.7));
            let z = rc_u01(rc_pcg(h + 3u)) / max(dn, 1e-3);
            let r = z * z * z * z * 3. + .06;
            let x = 1. - dot(d, d) * 9. / r;
            if (x > pt.x && z < 1.) {
                pt = vec4f(x, d / sqrt(r));
            }
        }
        // Round 8 (round 7's review: the big pits round craters with lit
        // lips): a void, not a bowl: a sharp, jagged edge (the grit and the
        // mottle break it), dark inside and shut off from the sky, its
        // surface's normal left as it is (no lit wall). Round 10 (round 9's
        // review: printed on): lighter inside (about 0.7 of the paste) and
        // shaded across, darkest on its upper right (the side the light
        // comes from, under the void's own lip), lightest on the far wall;
        // the offset kept in the void's own size (a third at its edge).
        // Round 11 (its review: still a faint shaded side): nearly twice the
        // swing, so the far wall even catches a little light at its rim.
        // Round 12 (its review: still stamped): darkest at the heart (about
        // half the paste), lighter towards the rim.
        let ph = smoothstep(.0, .12, pt.x - 2. * abs(m) - g * .3);
        let cm = t * (1. - ph * (.12 + pt.x * .4 + (pt.y + pt.z) * .9));
        let dv = u32(round(b.na.w * 6.));
        if (dv > 0u) {
            // The diagnostic views unlit: no reflection at the silhouette
            // (round 5's showed a pale sliver of the sun's on the right).
            o.metal = 1.;
            o.occ = .0;
        }
        if (dv == 0u) {
            o.base *= vec4f(cm, cm, cm, 1.);
            o.rough = clamp(o.rough + dr, .12, .98);
            o.occ *= 1. - .3 * ph;
            // Round 13 (rounds 11-12's rim sheen lit every edge, the shaded
            // one too, and the satin still fell short: lit p99 215 against
            // best.png's 250): a broad second lobe of the paste's own colour
            // round the sun's mirror direction (as foliage's glints read the
            // sun), which the lit stage adds only where the sun reaches; the
            // glossier the paste (`rough`), the stronger. Round 14 (its review:
            // the whole lit side lifted, warm and plastic): a tighter, stronger
            // lobe whose peaks the tone map whitens, less on the matte deposits
            // (broken up), and scaled by the surface's own brightness again, so
            // the darker joints take about a quarter (no bright joint lines).
            // Round 15 (its review: still no near-white peak): much tighter and
            // four times stronger, so the tone map rolls its peaks towards
            // white, and grainy (the grit's own noise). Round 16 (its review:
            // peach, not white): the paste's colour turned round (bgr), a
            // touch cool against the warm sun.
            o.glint = o.base.bgr * o.base.g * pow(max(dot(i.n, normalize(frame.light_dir.xyz + f.v)), 0.), 300.) * (1. - b.na.x) * (400. - e * 240.) * (g + .5);
        } else if (dv == 1u) {
            o.emit *= cm;
        } else if (dv == 3u) {
            o.emit += dr;
        } else if (dv == 6u) {
            o.emit *= 1. - .5 * ph;
        }
        return o;
    }
    let w = i.world;
    let ng = normalize(i.g);
    let ts = rc_range(b.na.z, 1.0, 50.0);
    let k = vec4f(rc_aa_world(f, 0.2 / ts), rc_aa_world(f, 0.018), rc_aa_world(f, 0.05 / rc_range(b.nb.x, 1.0, 50.0)), rc_aa_world(f, rc_range(b.nb.w, 0.005, 0.05)));
    var s: vec4f;
    var pq: vec2f;
    if (q >= 3u) {
        let pp = rc_proj2(w, ng);
        s = mix(rc_plaster_s(b, pp.a, k), rc_plaster_s(b, pp.b, k), pp.w);
        pq = pp.a;
    } else {
        pq = rc_proj(w, ng);
        s = rc_plaster_s(b, pq, k);
    }
    var c = mix(b.col_a * s.x, b.col_b, s.y * 0.85);
    c = mix(c, c * 0.5, s.z);
    if (rc_extra(b, RC_X_PLASTER, 2u)) {
        // Where plaster is gone: narrow lath strips, dark between.
        let ly = fract(pq.y * 22.0);
        let lt = mix(0.35, 1.0, smoothstep(0.1, 0.25, ly) * (1.0 - smoothstep(0.75, 0.9, ly)));
        c = mix(c, b.col_c * lt * (0.8 + 0.4 * rc_vnoise2(vec2f(pq.x * 3.0, floor(pq.y * 22.0)), b.seed + 14u)), rc_plaster_chunk(b, pq));
    }
    if (q >= 3u) {
        // Sand flecks: single grains, some lighter, some darker, each a soft
        // round speck of a fine value noise (round 9: one random value per
        // 4.5 mm grid square drew as a checker of squares in a close-up).
        let r = rc_vnoise2(pq * 260.0, b.seed + 9u);
        c = c * (1.0 + (smoothstep(0.78, 0.95, r) - smoothstep(0.05, 0.2, 1.0 - r) * 0.8 + 0.08) * b.nb.y * 0.5 * rc_aa_world(f, 1.0 / 260.0));
    }
    // Cracks and carved breaks (round 9): where the surface folds in tightly
    // (a crack's groove, the root of a chip), grime and shadow collect.
    c = c * (1.0 - 0.6 * smoothstep(250.0, 1200.0, -f.curv));
    var o = rc_out(i, c, b.na.x * select(1.0, 0.9, b.variant == 2u), 0.0);
    if (q >= 2u) {
        let near = rc_near(f) * rc_aa_world(f, 0.003);
        if (near > 0.0) {
            let t = rc_tw(ng);
            let e = 0.0012;
            let h = rc_plaster_s(b, rc_proj(w, ng), k).w;
            let d = vec2f(rc_plaster_s(b, rc_proj(w + t[0] * e, ng), k).w, rc_plaster_s(b, rc_proj(w + t[1] * e, ng), k).w) - vec2f(h);
            o.n = rc_bump(i.n, t, d * (near / e));
        }
    }
    return o;
}
