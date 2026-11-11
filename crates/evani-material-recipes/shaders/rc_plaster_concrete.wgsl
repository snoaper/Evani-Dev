// Plaster's concrete part: cast concrete (procedural realism round 9, Chris's concrete spec): one
// composable surface for cast concrete and its kin (plaster variant 5), laid
// in object space (the object-local position, so it stays on a moving or
// turning object) over three planes blended by the object-space normal.
// Each layer is a function; a preset is only numbers (surface.rs
// `concrete_preset`: fresh poured, standard structural, aged exposed, rough
// industrial, polished):
//   A macro: warm-neutral tone drifts (domain-warped fBm at two scales with
//     a warmer and a cooler shift) and faint casting lift lines;
//   B meso: sparse aggregate showing through (warped cells), lighter or
//     darker, a little proud and smoother;
//   C pores: clustered, many tiny, fewer small and rare large lobed
//     blowholes, each a real recess (height) whose inside darkens with depth;
//   D grain: fine sand in the normals, faded with distance.
// rc_plaster_cs returns base colour, roughness, the perturbed normal (from the
// height by the shared rc_h2n) and, for the diagnostic views, height,
// porosity, edge wear and cavity.
//   numbers (plaster's slots): na.x roughness (0.75-1), na.y macro tone,
//   na.z aggregate showing, na.w diagnostic view (plaster's crack slot:
//   concrete's cracks are geometry; 0 off, then base, normal, roughness,
//   porosity, edge wear, height), nb.y pores (tiny and small), nb.z
//   weathering (edge wear and grime), nb.w large blowholes.
// Levels: lite and low the dominant plane's colour (pores as dark specks);
// medium adds the normals; high and ultra blend the planes and add the
// aggregate and the grain.

struct RcPlasterCs {
    c: vec3f,
    // height (m), porosity (0-1), aggregate (0-1), pore depth (0-1)
    h: vec4f,
};

// One pore layer at plane point q: cells `cell` m across, radii r0..r1 of a
// cell (mostly small: cubed), present in `dens` of the cells; the sampling
// point warped by `wp` of a cell, so a pore's outline is irregular (lobed
// for the big ones). x the pore mask, y its depth profile (the cube's lip
// ring is gone: its lit rims read as stamped craters, the Asset Studio
// session's round 1).
fn rc_plaster_po(q: vec2f, s: u32, cell: f32, r0: f32, r1: f32, dens: f32, wp: f32) -> vec2f {
    let u = q / cell;
    let w = u + (vec2f(rc_vnoise2(u * 1.7, s + 1u), rc_vnoise2(u * 1.7 + vec2f(5.3, 1.1), s + 2u)) - vec2f(.5)) * wp;
    let c = rc_voronoi2(w, s, 1.);
    let r = mix(r0, r1, c.w * c.w * c.w);
    let on = step(fract(c.w * 7.13), dens);
    let m = (1. - smoothstep(r * .6, r, c.x)) * on;
    return vec2f(m, m * (1. - c.x / max(r, 1e-4)));
}

// The cone's patch field, shared by its colour and its relief: the domain
// warp at p, one octave turn (so the lattice never lines up) and the larger
// drift at the warped point w.
fn rc_plaster_cw(p: vec3f, s: u32) -> vec3f {
    return p + (vec3f(rc_vnoise3(p * 1.3, s + 80u), rc_vnoise3(p * 1.3 + vec3f(5.2, 1.3, 7.7), s + 81u), rc_vnoise3(p * 1.3 + vec3f(2.9, 8.4, 3.1), s + 82u)) - vec3f(.5)) * .35;
}

fn rc_plaster_cr(x: vec3f) -> vec3f {
    return vec3f(.8 * x.y + .6 * x.z, -.8 * x.x + .36 * x.y - .48 * x.z, -.6 * x.x - .48 * x.y + .64 * x.z) * 2.03;
}

fn rc_plaster_cb(w: vec3f, s: u32) -> f32 {
    return rc_vnoise3(w * 2. + vec3f(7.9, .4, 3.3), s + 73u) * .65 + rc_vnoise3(w * 4.1 + vec3f(2.2, 6.6, .9), s + 76u) * .35;
}

// The garden cone's patches at object point p (the live cone session, round
// 2; best.png's cloudy pour and curing marks over the whole piece): one
// signed field of 3D value noise, domain-warped, mostly patches of 0.1 to
// 0.4 m with a faint larger drift, its edges broken by a fine noise. Above it the darker
// patches (x), below it the paler (y), nc.x their strength, nc.y the share
// that is dark. Worked out in 3D, so a round piece has no projection seams.
fn rc_plaster_cp(b: RecBlock, p: vec3f) -> vec3f {
    let s = b.seed;
    let w = rc_plaster_cw(p, s);
    var x = w * 5.8;
    var sum = .0;
    var amp = .5;
    for (var o = 0u; o < 4u; o = o + 1u) {
        sum = sum + rc_vnoise3(x, s + 72u + o * 101u) * amp;
        x = rc_plaster_cr(x);
        amp = amp * .5;
    }
    let big = rc_plaster_cb(w, s);
    // The fine mottle (2-8 cm): the patches' edges break up in it, and it
    // tints the surface between them, as a cast face is never one tone.
    let fm = rc_vnoise3(p * 14. + vec3f(3.1, .7, 5.9), s + 70u) * .65 + rc_vnoise3(p * 38. + vec3f(8.3, 2.2, 1.4), s + 71u) * .35;
    let br = (fm - .5) * .22 + (rc_vnoise3(p * 90., s + 74u) - .5) * .06;
    // Round 2b: the mid-sized patches lead (round 2's big field laid one
    // pale lobe over the whole front). The octaves' weights sum to 0.9375.
    // pu: the field without the fine breakup (round 10: the cores' insides
    // grade on it, smooth, not sooty).
    let pu = (sum / .9375 - .5) * 1.1 + (big - .5) * .15 + (b.nc.y - .5) * .2;
    let pv = pu + br;
    // Damp stains (best.png's broad, soft, shallow clouds): a wide soft
    // halo (0.45, about 40% of the surface) and a graded core, a soft edge
    // (0.3, 1-4 cm) darkening inwards (0.3), about 30% shallower than round
    // 10's (round 12: measured inside the cone, best.png's stains are
    // shallower than rounds 9-11's; round 11's deep cores read as a spill);
    // a thin tide line (a few mm) on each stain's outline, dark (0.22) on
    // about half of it and pale (0.6, a lacy breakup) on the other half;
    // the paler about 32%, feathered over 2-3 cm.
    let tl = (1. - smoothstep(.004, .01, abs(pv - .1))) * vec2f(.22, -.6) * (step(.5, fm) - vec2f(.0, 1.));
    return vec3f((vec2f(smoothstep(-.02, .14, pv) * .45 + smoothstep(.1, .15 + .15 * big * big, pv) * .3 + smoothstep(.12, .36, pu) * .3, smoothstep(.05, .17, -pv)) + tl) * b.nc.x, fm);
}

// The cone's patch relief at p (m of height), from its masks m at p (round
// 9, the same as its colour's): the paler patches 0.4 mm proud (a skin of
// laitance), the stains eroded, their halo 0.15 mm, their core 0.45, a deep
// centre 0.6, over the form's 2-4 cm unevenness (about +-0.8 mm).
fn rc_plaster_cl(b: RecBlock, p: vec3f, m: vec2f) -> f32 {
    return m.y * .00045 - m.x * .0005 + (rc_vnoise3(p * 30., b.seed + 77u) - .5) * .0018 * b.nc.x;
}

// The surface at plane point q (m) of the object: k.x keeps the tiny pores,
// k.y the small ones, k.z the grain (anti-alias fades), q3 whether the
// aggregate is drawn, up whether the plane's second axis is the object's up.
// The live cube's layers (numbers 8 and 9, nc: stains and its lift line;
// every other concrete leaves them 0 and draws as before): v2 widens the
// macro tone, adds cloudy stains, smudges and a level formwork lift line on
// its x sides, and draws its pores rounder and darker inside.
// The garden cone (extras bit 5, the live cone session; every other concrete
// leaves the extras 0 and draws as before) reads numbers 8 and 9 as its own:
// cast-piece blotches towards colour B (darker) and colour C (paler), nc.x
// their strength, nc.y their coverage; it never takes the cube's v2 path.
fn rc_plaster_ca(b: RecBlock, q: vec2f, k: vec3f, q3: bool, ed: f32, up: bool, pk: vec2f) -> RcPlasterCs {
    var o: RcPlasterCs;
    let s = b.seed;
    let cone = (b.extras & 32u) != 0u;
    let v2 = b.nc.x + b.nc.y > .0 && !cone;
    // A: macro tone, warmer and cooler drifts, lift lines.
    let wq = q + (vec2f(rc_fbm2(q * .9, s + 40u, 2u), rc_fbm2(q * .9 + vec2f(3.7, 8.1), s + 41u, 2u)) - vec2f(.5)) * .8;
    let m1 = rc_fbm2(wq * 1.6, s + 42u, 3u);
    let m2 = rc_fbm2(wq * 5.5, s + 43u, 2u);
    let tone = 1. + ((m1 - .5) * .26 + (m2 - .5) * .1) * (.4 + b.na.y) * (1. + 2.5 * b.nc.x * select(1., .4, v2 && !up));
    let hue = rc_fbm2(wq * .7 + vec2f(11., 2.), s + 44u, 2u) - .5;
    var c = b.col_a * tone * (vec3f(1.) + vec3f(.05, .0, -.06) * hue * b.na.y * 2.);
    let lift = 1. - smoothstep(.0, .025, abs(fract(q.y * 1.4 + m1 * .25) - .5)) * .035 * b.na.y;
    c = c * lift;
    // The cube's formwork lift line (round 4: best.png's straight, level
    // joint across the upper right face): a 5 mm groove at 0.78 m on the x
    // sides (planes with q.x the object's z), in the relief only and broken
    // into lengths (round 5: round 4's darkened, unbroken 1 mm groove read
    // as a pencil line); nc.y its strength.
    // Column 6 round 3 (the review: an incised line, best.png's is a broad,
    // irregular, softly bounded trace): a band about 3 cm wide whose height
    // wanders with the macro field, in 20-40 cm lengths, a shallow dip and
    // a slightly darker tone (in the stains below).
    let lg = select(.0, b.nc.y, v2 && q.x < 9.) * (1. - smoothstep(.003, .03, abs(q.y - .77 + m1 * .02))) * smoothstep(.3, .5, rc_vnoise2(q * 4., s));
    // Column 6 round 2: the trowelled top's strokes (best.png's top is
    // scraped in one direction, not cloudy): a noise stretched 8:1 along
    // the warped plane, a faint tone and a shallow relief.
    // Column 6 round 3 (the review: the top too smooth and even): scraped
    // patches of 10-20 cm (the macro field m2 past a threshold, edges over
    // about a sixth of its range), paler and a hair proud, over the strokes;
    // column 6 round 4 (the review: still a smooth slab, strokes too
    // faint): the strokes 1.6 times as strong and the patches directional
    // (a noise stretched about 3:1, 7 by 20 cm), the tone 0.13, the relief
    // half (narrow edges drew contour lines);
    // the top a little paler and cooler again (with the scene's dimmer
    // ambient it measured 3 levels dark and warm).
    // Column 6 round 5 (the review: round 4's stretched strokes formed broad
    // wavy bands): a patchwork of short scraped patches instead, the cells
    // of a Voronoi field stretched about 3:1 on the warped plane, so their
    // direction wanders, each its own flat tone (round 6: faded, below).
    // Round 6 (the review: thin bright and dark seams outlined a tiled cell
    // network): each patch fades from its middle to the shared base before
    // it meets a neighbour (the cell's distance, x), its tone about twice
    // as strong at the middle (the top's spread held at round 5's), and no relief, so height and normals stay
    // continuous; patches whose tone is near the base vanish.
    let vc = rc_voronoi2(wq * vec2f(9., 3.5), s + 62u, 1.);
    let tr = select(.0, (vc.w - .5) * 3.3 * (1. - smoothstep(.12, .5, vc.x)), v2 && !up);
    if (v2) {
        // Stains: cloudy darker patches of 15-40 cm and paler ones between
        // them, one field, its edges broken by a 2 cm speckle (the Asset
        // Studio session's round 1: 0.5-2 m soft blotches read as
        // camouflage), and darker smudges of 2-10 cm over them (round 2:
        // best.png's mid-scale mottling is about twice round 1's). Column 6
        // round 1: the darker cooler (more grey, less brown: round 5's read
        // as soot), the paler warmer; the smudges half as strong; the
        // trowelled top a little cooler and paler than the cast sides.
        // Column 6 round 2 (the tab's review: the lit face short of
        // mid-scale texture, the top cloudy): the smudges paler as well as
        // darker (2-7 cm, zero-mean, so no new dark dots) and twice as
        // strong; the darker stains' edges firmer (over 0.17 of the field,
        // 0.35 before: soft edges read as smoke), broken by the 2 cm speckle,
        // and lighter (0.2, 0.3 before: best.png's lit face has no heavy blots);
        // column 6 round 4 (the review: the lit face uniform between its
        // pores): the darker kept at 0.2 (0.24 read heavy) and paler casting patches, firm-edged
        // (0.38-0.5 of the field, mean 0.56) and up to 16% paler, so the face carries a
        // few uneven light and dark patches; round 5: faint vertical drag
        // traces (a noise 3 cm across and 50 cm long, +-6%) on the cast sides,
        // half on the top, no extra stain contrast; round 6: their strength
        // follows the stain field (about +-3% to +-8%), so some runs are
        // marked and others faint, of varied lengths;
        // on the top the stains at half strength and (above) the macro
        // tone's widening at 40% (the cube's alone: the cone reads nc too).
        let st = rc_fbm2(wq * 2.4, s + 60u, 4u) + rc_vnoise2(q * 45., s) * .12;
        let sm = rc_fbm2(q * 14., s + 9u, 3u) - .5;
        c *= (1. - (.2 * smoothstep(.55, .72, st) + .22 * sm + .16 * smoothstep(.38, .5, st) - .16 + (rc_vnoise2(q * vec2f(30., 2.), s + 63u) - .5) * .2 * st) * b.nc.x * select(.5, 1., up) * vec3f(1.1, 1., .85)) * select(vec3f(1.01, 1.04, 1.13), vec3f(1.), up) * (1. + tr * .13 - lg * .05);
    }
    // C: pores, clustered by a slow field.
    let cl = smoothstep(.3, .75, rc_fbm2(q * 3., s + 45u, 2u));
    // Arrises crumble: more and bigger pores along a convex edge (ed).
    // The cone: its pinholes a little denser in the darker patches (round 10:
    // evenly spread, as best.png's; gathered, they read as soot).
    // The cube's top (trowelled, not cast against a form): a few of them.
    let dn = b.nb.y * (.35 + 1.3 * cl) * select(1. + 2.5 * ed, 1., cone) * select(1., (.55 + .5 * pk.x) * (.06 + 1.44 * pk.y), cone) * select(1., .15, v2 && !up);
    // v2: rounder pinholes (less warp), as cast concrete's.
    let wf = select(1., .35, v2 || cone);
    let p0 = rc_plaster_po(q, s + 46u, .0026, .14, .36, dn, .35 * wf) * k.x;
    // The cone's small pores stay under about 5 mm across.
    let p1 = rc_plaster_po(q, s + 47u, select(.009, .006, cone), .14, .42, dn * .55, .3 * wf) * k.y;
    // The cube's blowholes 4-8 mm across (column 6 round 1: best.png's), a
    // third as many on its trowelled top (column 6 round 2).
    let p2 = rc_plaster_po(q, s + 48u, select(select(.035, .02, v2), .011, cone), .2, .42, b.nb.w * (.3 + cl) * select(select(1., .3, v2 && !up), .15 + .3 * pk.y, cone), .4 * select(1., .6, v2));
    let inner = rc_vnoise2(q * 900., s + 49u);
    // The cone's pits 45% as deep in shade (round 9), about 30% in relief
    // (rounds 10-11: their lit side read as strong black pits); the cube's
    // 70% and 57% (round 3's 45% read as stains, round 2's full relief drew
    // striped insides in its lobed pits), their insides darker below; the
    // cube's dark over most of the hole, a crisp rim (column 6 round 1: its
    // soft cones read as smudged dots against best.png's sharp pinholes).
    let pd = select(select(1., .7, v2), .45, cone);
    let depth = min(max(max(p0.y, p1.y), p2.y * (.75 + .25 * inner)) * select(1., 2.5, v2), 1.) * pd;
    var h = -(p0.y * .0005 + p1.y * .0014 + p2.y * (.004 + .0015 * inner)) * pd * (.4 + .6 * pd);
    h -= lg * 3e-4;
    let pore = max(max(p0.x, p1.x), p2.x);
    // B: aggregate showing through, sparse, a little proud.
    var agg = .0;
    if (q3 && b.na.z > .0) {
        let g = rc_voronoi2(q * 130. + vec2f(rc_vnoise2(q * 40., s + 50u)) * .8, s + 51u, 1.);
        agg = smoothstep(.42, .2, g.x) * step(g.w, b.na.z * smoothstep(.35, .7, rc_vnoise2(q * 6., s + 52u)));
        c = mix(c, b.col_a * (.82 + .4 * fract(g.w * 13.)), agg * .7);
        h = h + agg * .00025;
    }
    // D: grain.
    // The cube's grain relief patchy (column 6 round 3, the review: a
    // uniformly stippled face): from 0.25 in smooth cast skin to 1.05
    // where the pores cluster (cl, poorly compacted), so flatter areas and
    // rougher patches alternate over 15-40 cm (round 4: round 3's 1.7 read
    // as raised granular plaster; round 5: 0.12 to 0.62, round 4's still
    // read as angular stipple beside the edges against best.png's smooth
    // skin with distinct pits).
    // The cube's sandy speckle about the cone's (column 6 round 1: round
    // 5's, twice the cone's, read as sandpaper; round 2: round 1's two
    // thirds left the lit face finer-textured than best.png's).
    let sandy = (cone || v2) && (b.extras & 64u) == 0u;
    h = h + ((rc_vnoise2(q * 700., s + 53u) - .5) * .0003 + (rc_vnoise2(q * 260., s + 54u) - .5) * .0003) * k.z * select(1., 2., sandy) * select(1., .12 + .5 * cl, v2);
    if (sandy) {
        // A sandy speckle in the colour, 4 mm, faded with the small pores'
        // size (round 11: 2 mm faded out at the default view's distance, and
        // the upper body read plastered).
        c = c * (1. + (rc_vnoise2(q * 260., s + 57u) - .5) * select(.24, .26, v2) * k.y);
    }
    // Pore insides darken with their depth (no light reaches the bottom);
    // pores under a pixel darken the tone by their average.
    // The cone's a soft grey (the scored review of its round 7: a dark pepper).
    // The cube's at half (column 6 round 2: with the sky light's occlusion
    // below, its pores read as pepper on the shaded face, 7 times
    // best.png's dark cores there); in sun their relief still shades them.
    c = c * (1. - select(select(.6, .3, cone), .5, v2) * depth) * (1. - .05 * dn * (2. - k.x - k.y));
    o.c = c;
    o.h = vec4f(h, pore, agg, depth);
    return o;
}

fn rc_plaster_cs(b: RecBlock, f: MatFrame, i: MatIn, q: u32) -> MatOut {
    // Object space: the local position and normal (the world ones where the
    // stage has none).
    let has = dot(i.ln, i.ln) > .25;
    let cn = (b.extras & 32u) != 0u;
    let p = select(i.world, i.local, has);
    let nl = normalize(select(i.g, i.ln, has));
    let lpx = max(length(f.dlx), length(f.dly));
    let k = smoothstep(vec3f(.75), vec3f(1.5), vec3f(.0013, .004, .0015) / max(lpx, 1e-7)) * vec3f(1., 1., rc_near(f));
    var w = pow(abs(nl), vec3f(4.));
    w = w / (w.x + w.y + w.z);
    let top = select(select(2u, 0u, w.x >= w.z), 1u, w.y >= w.x && w.y >= w.z);
    let q3 = q >= 3u;
    let ed = smoothstep(60., 260., f.curv);
    var c = vec3f(.0);
    var hh = vec4f(.0);
    var g = vec3f(.0);
    // The garden cone's patches (extras bit 5; 0 on every other concrete).
    // The cone's ring up to the outer edge of its rounded top (rounds 10-11;
    // 0 on the body, 0 on a floor at height 0).
    let rg = step(.001, p.y) * (1. - smoothstep(.085, .098, p.y));
    var pm = vec2f(.0);
    var pr = pm;
    var fm = .5;
    if (cn) {
        let cp = rc_plaster_cp(b, p);
        pr = cp.xy;
        // On the ring, the darker at half strength (larger than the ring is
        // tall, they read there as hard blocks) and no paler (round 10: they
        // read as cream paint; its top's chalk is below); elsewhere the paler
        // broken by the mottle, dried, not painted.
        pm = cp.xy * vec2f(1. - .5 * rg, (1. - rg) * (.4 + 1.2 * cp.z));
        fm = cp.z;
    }
    // The cone's pore density: the darker patches, and the body above the
    // ring (round 9: even over the body, about a quarter of that on the ring;
    // round 8's ring had 3.6 times the reference's strong pits).
    let pk = vec2f(pm.x, smoothstep(.11, .16, p.y));
    let e = .0006;
    for (var a = 0u; a < 3u; a = a + 1u) {
        let wa = select(w[a], 1., !q3 && a == top);
        if ((q3 && wa > .02) || (!q3 && a == top)) {
            // Plane a's coordinates and its two object-space tangents.
            let tu = select(vec3f(1., .0, .0), vec3f(.0, .0, 1.), a == 0u);
            let tv = select(vec3f(.0, 1., .0), vec3f(.0, .0, 1.), a == 1u);
            let qa = vec2f(dot(p, tu), dot(p, tv)) + vec2f(f32(a) * 17.3);
            let s = rc_plaster_ca(b, qa, k, q3, ed, a != 1u, pk);
            c = c + s.c * wa;
            hh = hh + s.h * wa;
            if (q >= 2u) {
                let hx = rc_plaster_ca(b, qa + vec2f(e, .0), k, q3, ed, a != 1u, pk).h.x;
                let hy = rc_plaster_ca(b, qa + vec2f(.0, e), k, q3, ed, a != 1u, pk).h.x;
                g = g + (tu * (hx - s.h.x) + tv * (hy - s.h.x)) * (wa / e);
            }
        }
    }
    if (cn && q >= 2u) {
        // The cone's patch relief, along two tangents of the surface.
        let up = select(vec3f(.0, 1., .0), vec3f(1., .0, .0), abs(nl.y) > .95);
        let t1 = normalize(cross(up, nl));
        let t2 = cross(nl, t1);
        let ep = .012;
        let r0 = rc_plaster_cl(b, p, pr);
        let r1 = rc_plaster_cl(b, p + t1 * ep, rc_plaster_cp(b, p + t1 * ep).xy);
        let r2 = rc_plaster_cl(b, p + t2 * ep, rc_plaster_cp(b, p + t2 * ep).xy);
        g = g + (t1 * (r1 - r0) + t2 * (r2 - r0)) * (1. / ep);
        hh.x = hh.x + r0;
    }
    // Weathering: grime at the foot (object height), worn arrises lighter
    // and rougher (convex curvature), dirt in tight folds (cracks, chip roots).
    let wear = ed * smoothstep(.35, .65, rc_vnoise2(p.xz * 9. + vec2f(p.y * 7.), b.seed + 55u)) * b.nb.z;
    let cav = smoothstep(250., 1200., -f.curv);
    if (cn) {
        // The cone's patches: darker towards colour B, paler towards C,
        // keeping the tone drift under them.
        let tn = c / max(b.col_a, vec3f(1e-4));
        c = mix(mix(c, b.col_b * tn, pm.x), b.col_c * tn, pm.y);
        // The fine mottle, about +-10% at full strength, neutral, a little more
        // inside the patches (their own tonal variation), over a 3 cm grain
        // of about +-4% (round 10: the field between the stains read plain).
        let g3 = rc_vnoise3(p * 30., b.seed + 75u);
        c = c * (1. + ((fm - .5) * .28 * (1.4 + .2 * (pm.x + pm.y)) + (g3 - .5) * .12) * b.nc.x);
        // Round 10: the ring's face above 5 cm 16% darker, the front of its
        // rounded top (7-10 cm) a chalky pale, about 85% lighter, in ragged patches
        // over about two thirds of it (round 11: soft smears before); the
        // runs 14% darker, on more of the body, up to 0.7 m.
        // Ground grime: the ring's lower face, the bottom 4-6 cm, up to 37%
        // darker and warmer, its top edge broken by the mottle (the ring's
        // top stays pale; none on a floor at height 0); rain runs: 1-2 cm
        // wide, 10-25 cm long, up to 12% darker, over the lower body above
        // the ring (pk.y; none on the floor, which shares this concrete),
        // noise in the angle round the axis (its seam at the back) and the
        // height.
        let rn = rc_vnoise3(vec3f(atan2(p.x, p.z) * 16., p.y * 5., .0), b.seed + 78u);
        c = c * (1. - vec3f(.31, .35, .4) * (step(.001, p.y) - smoothstep(.0, .045 + .03 * fm, p.y))) * (1. - .14 * smoothstep(.5, .78, rn) * pk.y * (1. - smoothstep(.55, .95, p.y + .5 * fm))) * (1. + rg * (.85 * smoothstep(.066, .08, p.y) * smoothstep(.55, .75, fm + g3 * .5) - .16 * smoothstep(.047, .057, p.y)));
    }
    // The shared foot darkening, not on the cone (its collar's face read
    // far too dark; its contact band above does the cone's).
    if (!cn) {
        c = rc_foot(c, select(i.world.y, p.y, has), hh.y);
    }
    c = mix(c, b.col_a * 1.12, wear * .5) * (1. - .6 * cav);
    // The cube (nc set, not the cone): its chips' fracture facets (normals
    // off the faces' axes, so the geometry's own breaks) a paler, fresh
    // grey speckled with darker and paler grains of aggregate, paler than
    // the face (round 4: best.png's broken arris is a pale band; round 3's
    // read as dark pits), matte, its grain in stronger relief (column 6
    // round 5: twice, not four times: the uprights' small nicks drew a
    // stippled band beside the edges; round 1's read
    // as glass), and the tone grading from 32% darker at the foot to its own
    // at 0.85 m (best.png's lit face darkens over its lower 60%; round 3's
    // 20% over 0.5 m read flat).
    let v2 = f32(b.nc.x + b.nc.y > .0 && !cn);
    let fr = v2 - v2 * smoothstep(.8, .97, dot(w, w));
    c = mix(c, b.col_a * (1.15 + .4 * rc_vnoise3(p * 300., b.seed)), fr * .7) * (1. - v2 * .32 * (1. - smoothstep(.0, .85, p.y)));
    // The live cube (nc set) may go glossier than 0.6 (its polished preset).
    var rough = clamp(b.na.x + (hh.y * .08 - hh.z * .1 + wear * .06 + fr) + (rc_vnoise2(p.xy * 3. + p.zz, b.seed + 56u) - .5) * .06, select(.6, .12, b.nc.x + b.nc.y > .0), .98);
    if (cn) {
        // The cone's patches: the darker ones a little smoother, the paler
        // ones rougher.
        rough = clamp(mix(rough, .8, pm.x) + .04 * pm.y, .12, .98);
    }
    var o = rc_out(i, c, rough, .0);
    // Pores take sky light from less of the sky; not on the cube (column 6
    // round 2: with its own darkening above they read as pepper in shade).
    o.occ = i.occ * (1. - .5 * hh.w * (1. - v2));
    if (q >= 2u) {
        // The object-space slope, kept in the surface, carried to world space.
        let gt = g - nl * dot(g, nl);
        // Relief (nb.x's upper half, the cube's live `relief`; under 0.5 as built).
        let rel = select(1., (b.nb.x - .5) * 8., b.nb.x >= .5);
        o.n = normalize(i.n - mat_l2w(f, gt) * rc_near(f) * rel * (1. + fr));
    }
    // Diagnostic views (nc.y): the layers, unlit, as the emission.
    let dv = u32(round(b.na.w * 6.));
    if (dv > 0u) {
        let n01 = o.n * .5 + vec3f(.5);
        // The cone's porosity view also shows its patch masks (red pores,
        // green the darker patches, blue the paler).
        let por = vec3f(hh.y, select(vec2f(hh.y), pm, cn));
        var ch = array<vec3f, 6>(c, n01, vec3f(rough), por, vec3f(wear + cav), vec3f(clamp(.5 + hh.x * select(150., 600., cn), .0, 1.)));
        o.emit = ch[min(dv, 6u) - 1u];
        o.base = vec4f(.0, .0, .0, o.base.a);
        o.rough = 1.;
    }
    return o;
}
