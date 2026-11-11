//! 1. Core geometry and primitives.
//!
//! Built: `geometry-cube-cuboid` (the reference CPU asset). Inspired by
//! three.js BoxGeometry (MIT): a box whose every face is a grid of
//! segments, so later deformers have vertices to move; written fresh here.
//!
//! Batch B, the basic generators (one feature each): the other 14
//! primitives over the shared base (`evani_procedural::build`, `shapes`).
//! Inspired by three.js (MIT) SphereGeometry, IcosahedronGeometry,
//! CylinderGeometry, ConeGeometry, CapsuleGeometry, TorusGeometry,
//! PlaneGeometry, RingGeometry, LatheGeometry, ExtrudeGeometry,
//! TubeGeometry and RoundedBoxGeometry; written fresh here.

#[cfg(feature = "geometry-cube-cuboid")]
pub use cube::CubeCuboid;

#[cfg(feature = "geometry-cube-cuboid")]
mod cube {
    use evani_procedural::surface::{FINISH_HELP, finish_tinted};
    use evani_procedural::{AssetInfo, Output, ParamSpec, Part, ProcAsset, Values, detail};

    /// A cube or cuboid: width, height, depth and face segments, its base on
    /// the ground. Realism: a cast block in a catalogue finish (cast
    /// concrete by default), its edges softly rounded and worn.
    pub struct CubeCuboid;

    /// The colour param's default: the cube's near-neutral grey concrete
    /// (linear; the Asset Studio session's round 1: best.png's grey, which
    /// its warm sun turns beige on the lit side; the first live session's
    /// warm brown-grey read as sandstone).
    /// Column 6 round 2: 2.5% lighter (its stronger smudges and the pores'
    /// lighter insides left the lit face 3 levels under best.png's).
    pub const COLOR: [f32; 3] = [0.3175, 0.323, 0.328];
    /// The cube's tone for each concrete preset (the shared presets' colours
    /// scaled as `COLOR` is to preset 1's).
    const WARM: [[f32; 3]; 5] = [[0.355, 0.362, 0.37], [0.3175, 0.323, 0.328], [0.281, 0.284, 0.281], [0.29, 0.298, 0.306], [0.281, 0.293, 0.306]];
    const INFO: AssetInfo = AssetInfo::cpu("geometry", "cube-cuboid", "Cube / cuboid").material("accent");
    /// The concrete's own layers, live (the live session with Chris,
    /// 2026-11-04): each below 0 keeps the `concrete` preset's number, 0..1
    /// replaces it (rc_plaster_concrete.wgsl's slot in brackets).
    const LAYERS: [(&str, usize); 6] = [("rough", 0), ("mottle", 1), ("aggregate", 2), ("pores", 5), ("weathering", 6), ("blowholes", 7)];
    const PARAMS: [ParamSpec; 26] = [
        ParamSpec::num("width", 1.0, 0.05, 20.0).unit("m").describe("Width along x (m)"),
        ParamSpec::num("height", 1.0, 0.05, 20.0).unit("m").describe("Height (m)"),
        ParamSpec::num("depth", 1.0, 0.05, 20.0).unit("m").describe("Depth along z (m)"),
        ParamSpec::num("segments", 1.0, 1.0, 16.0).step(1.0).describe("Segments across each face (more: finer edge wear)"),
        ParamSpec::color("color", COLOR).describe("Colour (at its default the finish keeps its own)"),
        ParamSpec::num("finish", 0.0, 0.0, 6.0).step(1.0).describe(FINISH_HELP),
        ParamSpec::num("bevel", 0.004, 0.0, 0.2).unit("m").describe("Edge rounding (m)"),
        ParamSpec::num("wear", 0.85, 0.0, 1.0).describe("Edge wear: broken chips on a few edges and corners (0: new)"),
        ParamSpec::num("preview", 0.0, 0.0, 6.0).step(1.0).describe(evani_procedural::forms::HELP),
        ParamSpec::num("crack", 0.05, 0.0, 1.0).describe("Chance of a shrinkage crack on the front face (the seed decides; 0: never, 1: always)"),
        ParamSpec::num("crack_length", 0.35, 0.05, 1.0).describe("Crack length, as a share of the face"),
        ParamSpec::num("crack_width", 4.0, 0.5, 15.0).unit("mm").describe("Crack width at its mouth (mm)"),
        ParamSpec::num("crack_depth", 8.0, 1.0, 30.0).unit("mm").describe("Crack depth (mm)"),
        ParamSpec::num("crack_branches", 1.0, 0.0, 3.0).step(1.0).describe("Branches off the crack"),
        ParamSpec::num("hairline", 1.0, 0.0, 1.0).describe("A hairline shrinkage crack carved down one side from near the top (0: none; the seed picks the side; none on a new block, wear 0)"),
        ParamSpec::num("concrete", 1.0, 0.0, 4.0).step(1.0).describe(evani_procedural::surface::CONCRETE_HELP),
        ParamSpec::num("view", 0.0, 0.0, 6.0).step(1.0).describe("Diagnostic view of the concrete: 0 the final picture, 1 base colour, 2 normal, 3 roughness, 4 porosity, 5 edge wear and cavity, 6 height"),
        ParamSpec::num("rough", -1.0, -1.0, 1.0).describe("Concrete roughness (the shader keeps 0.6-0.98; below 0: the preset's)"),
        ParamSpec::num("mottle", -1.0, -1.0, 1.0).describe("Concrete macro tone: pour clouds, warm and cool drift, lift lines (below 0: the preset's)"),
        ParamSpec::num("aggregate", 0.04, -1.0, 1.0).describe("Concrete aggregate showing through (below 0: the preset's)"),
        ParamSpec::num("pores", 0.14, -1.0, 1.0).describe("Concrete tiny and small pores (below 0: the preset's)"),
        ParamSpec::num("weathering", -1.0, -1.0, 1.0).describe("Concrete weathering: worn arrises and grime (below 0: the preset's)"),
        ParamSpec::num("blowholes", 0.035, -1.0, 1.0).describe("Concrete large lobed blowholes (below 0: the preset's)"),
        ParamSpec::num("relief", 2.0, 0.0, 4.0).describe("Concrete surface relief: the pores' and grain's normal strength (1 as built, 0 flat)"),
        ParamSpec::num("stains", 1.0, 0.0, 1.0).describe("Concrete stains: cloudy darker and paler patches and a wider tone drift (0: as round 9)"),
        ParamSpec::num("lift", 1.0, 0.0, 1.0).describe("Concrete formwork lift line: a level groove across the two x sides at 0.78 m, its strength (0: none)"),
    ];

    /// The cube's own made block (the live session, 2026-11-04): as
    /// [`detail::made_block_at`] (the shared, edge-bunched grid and its
    /// chips), but with crisp arrises: the grid's first row sits 2.9 cm in
    /// and an arris vertex's 45 degree normal faded across that row (the
    /// soft roll the review saw), so every arris vertex no chip moved is
    /// split, each face keeping its own copy with that face's normal (a hard
    /// edge); chipped places keep their smoothed normals. A packed grid did
    /// the same but its slivers made the chips refine past the 40k budget.
    fn sharp_block(size: [f32; 3], bevel: f32, seg: u32, wear: f32, seed: u32, density: f32, budget: usize) -> evani_procedural::geo::Mesh {
        let r = bevel.max(0.0005);
        let mut m = evani_procedural::build::rounded_box(size, r, seg, [0.0; 3]);
        detail::weld(&mut m);
        detail::box_normals(&mut m, size, [0.0; 3], r);
        let edges = detail::box_arrises(size, [0.0; 3], r);
        // The cube's own chips ([`chip_plan`]: runs of touching chips along
        // the top arrises, a corner fragment, fine nicks, a few on the
        // others), all before the arris split below. Broken a few at a time
        // while the mesh is under `budget` triangles: a chip's refinement
        // varies a lot by seed (some seeds took the cube to 50k), so no seed
        // passes the 40k budget; the later, smaller ones are dropped first.
        // Round 4: the band along the top arrises (1.2 cm each side) refined
        // once to about 1 cm, so the chips chewing them need no refinement
        // of their own (each chip refining the coarse grid alone cost about
        // 1,000 triangles, so the budget stopped a third of the way along).
        let (hx, hz, top) = (size[0] * 0.5, size[2] * 0.5, size[1]);
        let band = 0.012;
        if wear > 0.0 {
            detail::refine_near(&mut m, 0.01 / density.clamp(0.25, 1.0), 14, |c, e| {
                c[1] > top - band - 0.5 * e && (c[0].abs() > hx - band - 0.5 * e || c[2].abs() > hz - band - 0.5 * e)
            });
        }
        let mut moved = erode_arrises(&mut m, &edges, size[1], wear, seed);
        let chips = chip_plan(&edges, size, wear, seed, density);
        for batch in chips.chunks(8) {
            if m.indices.len() / 3 > budget {
                break;
            }
            let mv = detail::spalls(&mut m, batch);
                detail::merge_flags(&mut moved, &mv);
        }
        detail::smooth_normals_where(&mut m, &moved);
        // Arris vertices: not moved, normal off every axis.
        let arris: Vec<bool> = m
            .vertices
            .iter()
            .enumerate()
            .map(|(i, v)| !moved.get(i).copied().unwrap_or(true) && v[3].abs().max(v[4].abs()).max(v[5].abs()) < 0.999)
            .collect();
        let mut copies: std::collections::HashMap<(u32, usize), u32> = std::collections::HashMap::new();
        for t in 0..m.indices.len() / 3 {
            let ix = [m.indices[t * 3], m.indices[t * 3 + 1], m.indices[t * 3 + 2]];
            if !ix.iter().any(|&i| arris[i as usize]) {
                continue;
            }
            let p: [[f32; 3]; 3] = ix.map(|i| {
                let v = m.vertices[i as usize];
                [v[0], v[1], v[2]]
            });
            let (a, b) = (std::array::from_fn::<f32, 3, _>(|k| p[1][k] - p[0][k]), std::array::from_fn::<f32, 3, _>(|k| p[2][k] - p[0][k]));
            let n = [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
            let k = if n[0].abs() >= n[1].abs() && n[0].abs() >= n[2].abs() { 0 } else if n[1].abs() >= n[2].abs() { 1 } else { 2 };
            let axis = k * 2 + usize::from(n[k] < 0.0);
            for (c, &i) in ix.iter().enumerate() {
                if !arris[i as usize] {
                    continue;
                }
                let j = *copies.entry((i, axis)).or_insert_with(|| {
                    let mut v = m.vertices[i as usize];
                    v[3] = 0.0;
                    v[4] = 0.0;
                    v[5] = 0.0;
                    v[3 + k] = n[k].signum();
                    m.vertices.push(v);
                    (m.vertices.len() - 1) as u32
                });
                m.indices[t * 3 + c] = j;
            }
        }
        m
    }

    /// Worn, chewed arrises (round 4; best.png's top arrises are broken along
    /// about 90% of their length and its uprights wobble by a pixel or two):
    /// every vertex within reach of an arris `edges` (as
    /// [`detail::box_arrises`] gives them) of a box `height` tall is pushed
    /// in along its normal by a profile whose width and depth wander along
    /// the arris (seeded fBm): on the top arrises 4-16 mm wide and up to
    /// 7.5 mm deep (their band refined to about 1 cm), worn over about half
    /// their length in stretches with clean arris between (column 6 round 1:
    /// the tab's review asked for separate breaks, not one even chewed band);
    /// none on the uprights or the foot (column 6 round 1: their smoothed
    /// normals drew a smoky dark band and a pale strip beside each upright,
    /// the review's visible seams). Scaled by `wear`; returns which vertices
    /// moved.
    fn erode_arrises(m: &mut evani_procedural::geo::Mesh, edges: &[([f32; 3], [f32; 3])], height: f32, wear: f32, seed: u32) -> Vec<bool> {
        let mut moved = vec![false; m.vertices.len()];
        let w = wear.clamp(0.0, 1.0);
        if w <= 0.0 {
            return moved;
        }
        // fBm (about -1..1) spread to 0..1.
        let fb = |x: f32, y: f32, k: u32| (evani_procedural::noise::fbm2(x, y, seed ^ k, 3, 2.0, 0.5) + 0.5).clamp(0.0, 1.0);
        for (i, v) in m.vertices.iter_mut().enumerate() {
            let p = [v[0], v[1], v[2]];
            let mut push = 0.0f32;
            for (e, (a, b)) in edges.iter().enumerate() {
                let d: [f32; 3] = std::array::from_fn(|k| b[k] - a[k]);
                let len2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
                if len2 <= 1e-8 {
                    continue;
                }
                let t = ((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1] + (p[2] - a[2]) * d[2]) / len2;
                let q: [f32; 3] = std::array::from_fn(|k| a[k] + d[k] * t.clamp(0.0, 1.0) - p[k]);
                let dist = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2]).sqrt();
                if dist > 0.018 {
                    continue;
                }
                // Only the top arrises wear (the foot's drew a dark strip above
                // the floor's line, round 5; the uprights', column 6 round 1).
                if d[1].abs() >= 1e-4 || a[1] <= height * 0.5 {
                    continue;
                }
                // Along the arris in metres, each arris its own row of noise;
                // n3 gates the wear into stretches (about half the length).
                let s = t * len2.sqrt();
                let row = e as f32 * 7.31;
                let (n1, n2, n3) = (fb(s * 30.0, row, 0x3a1), fb(s * 55.0 + 3.0, row, 0x5c7), fb(s * 9.0 + 5.0, row, 0x7d3));
                let gate = ((n3 - 0.42) / 0.2).clamp(0.0, 1.0);
                let (wd, dp) = (0.004 + 0.012 * n1, 0.0075 * n2 * n2 * gate);
                if dist < wd {
                    push = push.max(dp * w * (1.0 - dist / wd).powf(1.3));
                }
            }
            if push > 0.0 {
                v[0] -= v[3] * push;
                v[1] -= v[4] * push;
                v[2] -= v[5] * push;
                moved[i] = true;
            }
        }
        moved
    }

    /// The triangle count past which no more chips are broken (a cube with
    /// its carved crack: lower, as the crack refines the mesh after them;
    /// the hairline takes about 3,500 after them).
    const CHIP_BUDGET: (usize, usize) = (29_500, 20_000);

    /// A point on side `face` (0 +x, 1 -x, 2 +z, 3 -z) of the box `size`
    /// (base centre at the origin): `u` across the face (m from its middle),
    /// `y` up.
    fn on_side(size: [f32; 3], face: u32, u: f32, y: f32) -> [f32; 3] {
        let (hx, hz) = (size[0] * 0.5, size[2] * 0.5);
        match face {
            0 => [hx, y, u],
            1 => [-hx, y, u],
            2 => [u, y, hz],
            _ => [u, y, -hz],
        }
    }

    /// The round 3 hairline (the Asset Studio session; the review asked for
    /// a crack in the shape, not painted): a shrinkage crack carved wandering
    /// down one side (the seed picks it) from 3 cm under the top, 20-45% of
    /// the height, 1.5 mm at its mouth closing to nothing, 2.5 mm deep, 4 cm
    /// clear of the arrises; `hairline` scales it (0: none). About 3,500
    /// triangles (a lift line carved the same way cost 10,000: the concrete
    /// draws its lift lines as a groove in its relief instead).
    fn carve_hairline(m: &mut evani_procedural::geo::Mesh, size: [f32; 3], hairline: f32, seed: u32) {
        let h = |k: u32| evani_procedural::noise::hash(seed.wrapping_mul(0x27d4_eb2d) ^ k.wrapping_mul(0x1656_67b1) ^ 0x7c3) as f32 / u32::MAX as f32;
        let f0 = (h(1) * 4.0) as u32 % 4;
        let width = |f: u32| if f < 2 { size[2] } else { size[0] };
        if hairline > 0.0 {
            let (wd, ht) = (width(f0), size[1]);
            let margin = 0.04f32.min(wd * 0.2);
            let (mut u, mut y) = ((h(2) - 0.5) * (wd - 2.0 * margin), ht - 0.03f32.min(ht * 0.1));
            let len = (0.2 + 0.25 * h(3)) * ht;
            let lean = (h(4) - 0.5) * 1.2;
            let mut path = vec![on_side(size, f0, u, y)];
            for k in 0..12 {
                let a = lean + (h(10 + k) - 0.5) * 1.0;
                u = (u + a.sin() * len / 12.0).clamp(-wd * 0.5 + margin, wd * 0.5 - margin);
                y = (y - a.cos() * len / 12.0).max(0.05);
                path.push(on_side(size, f0, u, y));
            }
            detail::carve_crack_moved(m, &path, 0.0015 * hairline, 0.0025 * hairline);
        }
    }

    /// The cube's chips along its arrises `edges` (as [`detail::box_arrises`]
    /// gives them) for a box of `size`, at wear `wear` (the Asset Studio
    /// session, rounds 2-3; best.png's top arrises are a pale, ragged line of
    /// touching breaks 1-3 cm across along most of their length, its others
    /// nicked so their outlines wobble): in order,
    /// - the uprights' nicks in two or three damaged sections each, and a
    ///   softened corner at each upright's foot (column 6 round 2);
    /// - a broken-off fragment at one top corner (4-5 cm across);
    /// - three to six chunky spalls (20-40 mm) on each top arris (round 5);
    /// - each top arris chewed along its whole length (round 4): shallow
    ///   chips 8-24 mm across and 1-7 mm deep, touching, the odd clean gap,
    ///   each pushed a little into the top or the side so the outline
    ///   wanders; the four arrises' chips interleaved.
    ///
    /// Each is a real break ([`detail::spalls`]: it notches the silhouette,
    /// and the concrete paints its facets as fresh breaks), refined only to
    /// about its own radius (a chip's facets are planes; finer, a run cost
    /// about 400 triangles a chip), so the runs fit the triangle budget.
    /// Fewer on the lower tiers (`density`).
    fn chip_plan(edges: &[([f32; 3], [f32; 3])], size: [f32; 3], wear: f32, seed: u32, density: f32) -> Vec<detail::Chip> {
        let h = |k: u32| evani_procedural::noise::hash(seed.wrapping_mul(0x9e37_79b9) ^ k.wrapping_mul(0x85eb_ca6b) ^ 0x6e1c) as f32 / u32::MAX as f32;
        let w = wear.clamp(0.0, 1.0);
        let dn = density.clamp(0.25, 1.0);
        // Chips scale with the block (a small block is knocked less deeply).
        let sc = (size[0].min(size[1]).min(size[2]) / 0.6).min(1.0);
        let chip = |at: [f32; 3], r: f32, depth: f32, k: u32, coarse: f32| detail::Chip {
            at,
            radius: r,
            depth,
            fine: (r * coarse / dn).max(0.0025),
            seed: seed ^ k.wrapping_mul(0x2545_f491) ^ 0x51ab,
        };
        let (mut corner, mut runs, mut rest): (Vec<detail::Chip>, Vec<Vec<detail::Chip>>, Vec<detail::Chip>) = (Vec::new(), Vec::new(), Vec::new());
        let tops: Vec<usize> = (0..edges.len())
            .filter(|&i| {
                let (a, b) = edges[i];
                (b[1] - a[1]).abs() < 1e-4 && a[1] > size[1] * 0.5
            })
            .collect();
        if w > 0.0 && !tops.is_empty() && h(1) < 0.4 + 0.6 * w {
            let (a, b) = edges[tops[(h(2) * tops.len() as f32) as usize % tops.len()]];
            let at = if h(3) < 0.5 { a } else { b };
            let r = (0.02 + 0.006 * h(4)) * sc;
            corner.push(chip(at, r, r * 0.45, 9_000, 0.5));
        }
        for (i, (a, b)) in edges.iter().enumerate() {
            let top = tops.contains(&i);
            let i = i as u32;
            let d: [f32; 3] = std::array::from_fn(|k| b[k] - a[k]);
            let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            if len <= 1e-4 || w <= 0.0 {
                continue;
            }
            let at = |t: f32| -> [f32; 3] { std::array::from_fn(|k| a[k] + d[k] * t) };
            if top {
                // A chewed arris (round 4: best.png's top arrises are broken
                // along about 90% of their length, the breaks paler than the
                // face): shallow chips walked along the whole arris, touching,
                // with the odd clean gap (2-5 cm); the outward side across it
                // (level, square to the arris, away from the block's axis).
                let mut side = [d[2], 0.0, -d[0]];
                let mid = at(0.5);
                if side[0] * mid[0] + side[2] * mid[2] < 0.0 {
                    side = [-side[0], 0.0, -side[2]];
                }
                let sl = (side[0] * side[0] + side[2] * side[2]).sqrt().max(1e-6);
                // Round 5: three to six chunky spalls (20-40 mm across) along
                // each top arris, best.png's crumbly bites (round 4's even
                // wear alone read as a bevel), refined to half their radius.
                for b in 0..(3 + (h(i * 64 + 5) * 3.99 * w) as u32) {
                    let k = i * 4096 + 3000 + b * 4;
                    let r = (0.01 + 0.01 * h(k)) * sc;
                    let mut p = at(0.06 + 0.88 * h(k + 1));
                    let off = (h(k + 2) - 0.5) * 0.5 * r;
                    if off > 0.0 {
                        p[1] -= off;
                    } else {
                        p[0] += side[0] / sl * off;
                        p[2] += side[2] / sl * off;
                    }
                    corner.push(chip(p, r, r * (0.3 + 0.25 * h(k + 3)) * w, k, 0.5));
                }
                let mut walk = Vec::new();
                let mut t = 0.02 + 0.02 * h(i * 64);
                let mut j = 0;
                while t < 0.98 && j < 200 {
                    let k = i * 4096 + 8 + j * 4;
                    let r = (0.004 + 0.008 * h(k) * h(k)) * sc;
                    // Column 6 round 1: clean gaps of 3-10 cm about one step
                    // in five (round 4's touching run read as an even band).
                    if h(k + 3) < 0.3 * (1.5 - w) {
                        t += (0.03 + 0.07 * h(k + 1)) / len;
                    } else {
                        // Pushed a little into the top (down) or the side (in).
                        let off = (h(k + 1) - 0.5) * 0.6 * r;
                        let mut p = at(t);
                        if off > 0.0 {
                            p[1] -= off;
                        } else {
                            p[0] += side[0] / sl * off;
                            p[2] += side[2] / sl * off;
                        }
                        // Refined no finer than the band (1 cm).
                        walk.push(chip(p, r, r * (0.3 + 0.3 * h(k + 2)) * w, k, (0.01 / r).max(1.0)));
                        t += r * (1.0 + 0.4 * h(k + 3)) / len;
                    }
                    j += 1;
                }
                runs.push(walk);
            }
            // Column 6 round 2 (the tab's review: the uprights and lower
            // corners read pristine): each upright nicked in two or three
            // damaged sections 3-11 cm long with clean arris between, two
            // to four chips each, 8-24 mm across and 30-70% of their radius
            // deep, and a softened corner where it meets the foot (a
            // shallow 16-28 mm chip). The foot's arrises: none (round 5).
            // Column 6 round 3 (the review: the central upright still read
            // ruler-straight; measured, round 2's nicks were 3-5 mm deep and
            // 1-5 cm long, a pixel or two): three or four sections, weighted
            // towards the top (more knocks near the upper junction), two to
            // four chips each 12-32 mm across and 35-75% of their radius
            // deep, and a loss at the upright's top end too (each top corner
            // softened, 16-28 mm) beside the one at its foot.
            if !top && d[1].abs() > 1e-4 {
                // `a` is the upright's foot, `b` its top (box_arrises).
                let secs = 3 + (h(i * 64 + 9) * 1.99 * w) as u32;
                for sct in 0..secs {
                    let k0 = i * 4096 + 2048 + sct * 64;
                    let mid = ((sct as f32 + 0.15 + 0.7 * h(k0)) / secs as f32).sqrt();
                    let span = (0.03 + 0.08 * h(k0 + 2)) / len;
                    // Round 4 (the review: isolated outer-edge scallops read
                    // as conspicuous): smaller and shallower lower down, the
                    // larger losses near the top (half size at the foot).
                    let up = 0.5 + 0.5 * mid;
                    for j in 0..2 + (h(k0 + 1) * 2.99 * dn) as u32 {
                        let k = k0 + 4 + j * 4;
                        let r = (0.005 + 0.01 * h(k + 1)) * sc * up;
                        rest.push(chip(at((mid + (h(k) - 0.5) * span).clamp(0.03, 0.97)), r, r * (0.3 + 0.4 * h(k + 2)) * w, k, 1.0));
                    }
                }
                for (end, k) in [(0.0, i * 4096 + 3900), (1.0, i * 4096 + 3904)] {
                    let r = (0.008 + 0.006 * h(k)) * sc;
                    rest.push(chip(at(end), r, r * 0.4 * w, k, 1.0));
                }
                // Round 4 (the review: clean stretches of the central upright
                // read very sharp; best.png's wear is subtle and continuous at
                // a tiny scale): a walk of tiny nicks, 4-8 mm across and about
                // a millimetre deep, every 3-8 cm up the upright, so its line
                // wavers by a pixel without a worn stripe.
                let mut t = 0.05 + 0.03 * h(i * 64 + 11);
                let mut j = 0;
                while t < 0.95 && j < 32 {
                    let k = i * 4096 + 3200 + j * 4;
                    let r = (0.002 + 0.002 * h(k)) * sc;
                    rest.push(chip(at(t), r, r * (0.25 + 0.25 * h(k + 1)) * w, k, 1.0));
                    t += (0.03 + 0.05 * h(k + 2)) / len;
                    j += 1;
                }
            }
        }
        // The uprights' nicks first (column 6 round 2: queued last, the
        // budget dropped them; each is small and refined only to its own
        // radius), then the corner fragment and the spalls, then the top
        // arrises' chips interleaved (one from each in turn), so a budget
        // cut trims all four alike.
        rest.append(&mut corner);
        let most = runs.iter().map(Vec::len).max().unwrap_or(0);
        for k in 0..most {
            for walk in &runs {
                if let Some(c) = walk.get(k) {
                    rest.push(*c);
                }
            }
        }
        rest
    }

    impl ProcAsset for CubeCuboid {
        fn info(&self) -> &AssetInfo {
            &INFO
        }

        fn params(&self) -> &[ParamSpec] {
            &PARAMS
        }

        fn generate(&self, v: &Values, seed: u64, _style: &str) -> Output {
            let size = [v.f("width").max(0.01), v.f("height").max(0.01), v.f("depth").max(0.01)];
            let seg = 10 + 2 * v.f("segments").round().max(1.0) as u32;
            let bevel = v.f("bevel").min(size[0].min(size[1]).min(size[2]) * 0.5);
            // Round 9: exact flat faces (no cushion) and real broken chips
            // along the arrises (`wear`), refined by the tier's density.
            let chance = v.f("crack").clamp(0.0, 1.0);
            let roll = evani_procedural::shapes::rnd(seed as u32, 0, 91);
            let cracked = chance > 0.0 && roll < chance;
            let mut m = sharp_block(size, bevel, seg, v.f("wear"), seed as u32, v.density(), if cracked { CHIP_BUDGET.1 } else { CHIP_BUDGET.0 });
            // Round 8: a real crack, carved into the mesh (it notches the
            // top edge in silhouette), its course from the seed. Round 9:
            // where it meets the top edge the arris is broken out in an
            // irregular, faceted chip (r8's notch read as a square cut).
            // Round 9 (Chris's concrete spec): a crack is optional and
            // parameterised, its chance (`crack`, very low by default),
            // length, width, depth and branches; the seed decides.
            if cracked {
                let small = size[0].min(size[1]).min(size[2]);
                let face = size[0].min(size[1]);
                let len = face * v.f("crack_length").clamp(0.05, 1.0);
                let (cw, cd) = ((v.f("crack_width") * 0.001).min(small * 0.03), (v.f("crack_depth") * 0.001).min(small * 0.05));
                let path = detail::box_crack_path(size, [0.0; 3], len, 0.03, seed as u32);
                let e = 0.293 * bevel;
                let at = [path[1][0], size[1] - e, size[2] * 0.5 - e];
                let r = (cw * 3.0).clamp(0.006, small * 0.05);
                let moved = detail::spall(&mut m, at, r, r * 0.55, (r * 0.25 / v.density().clamp(0.25, 2.0)).max(0.002), seed as u32 ^ 0xc4ac);
                detail::smooth_normals_where(&mut m, &moved);
                detail::carve_crack_moved(&mut m, &path, cw, cd);
                // Branches: shorter, narrower cracks leaving the main one.
                for k in 0..v.f("crack_branches").round().clamp(0.0, 3.0) as u32 {
                    let from = path[(3 + (path.len() - 4) * (k as usize + 1) / 4).min(path.len() - 1)];
                    let turn = if k % 2 == 0 { 0.7 } else { -0.7 };
                    let mut b = vec![from];
                    let (mut x, mut y) = (from[0], from[1]);
                    for j in 0..8u32 {
                        let a = turn + (evani_procedural::shapes::rnd(seed as u32, k * 16 + j, 92) - 0.5) * 0.9;
                        x = (x + a.sin() * len * 0.05).clamp(-size[0] * 0.5 + 0.05, size[0] * 0.5 - 0.05);
                        y = (y - a.cos() * len * 0.05).max(0.05);
                        b.push([x, y, from[2]]);
                    }
                    detail::carve_crack_moved(&mut m, &b, cw * 0.6, cd * 0.6);
                }
            }
            // A new block (wear 0) stays intact: the hairline grows with wear
            // up to 0.5.
            carve_hairline(&mut m, size, v.f("hairline").clamp(0.0, 1.0) * (v.f("wear") * 2.0).clamp(0.0, 1.0), seed as u32);
            // The concrete preset and the diagnostic view (round 9).
            let preset = v.f("concrete").round().clamp(0.0, 4.0) as usize;
            let view = v.f("view");
            let layered = |x| {
                let mut c = evani_procedural::surface::concrete_preset(x, preset).tint(WARM[preset]);
                for (name, slot) in LAYERS {
                    let n = v.f(name);
                    if n >= 0.0 {
                        c = c.number(slot, n);
                    }
                }
                // Relief: slot 4's upper half (the catalogue rows keep it
                // under 0.5, so every other concrete draws as before).
                let relief = v.f("relief").clamp(0.0, 4.0);
                if (relief - 1.0).abs() > 0.01 {
                    c = c.number(4, 0.5 + relief / 8.0);
                }
                // Stains and the lift line: numbers 8 and 9 (0 on every other
                // concrete; round 4: number 9 was faint vertical streaks).
                c = c.number(8, v.f("stains").clamp(0.0, 1.0)).number(9, v.f("lift").clamp(0.0, 1.0));
                evani_procedural::surface::concrete_view(c, view)
            };
            let s = finish_tinted(v.f("finish"), v.color("color"), COLOR, 0.0, layered).seed(seed as u32).scale(2.0);
            if let Some(o) = super::preview(v, s, seed as u32) {
                return o;
            }
            Output::Parts(vec![Part::surfaced(m, s)])
        }
    }
}

/// The realism preview (feature `preview`, the Material library view): the
/// asset's finish `sf` on an everyday form (a ball, a block, a door, a table,
/// a fence, steps; [`evani_procedural::forms`]) when its `preview` param
/// asks for one, any fittings in a dark steel. `None`: the asset's own shape.
#[allow(dead_code, unused_variables)]
fn preview(v: &evani_procedural::Values, sf: evani_procedural::surface::Surface, s: u32) -> Option<evani_procedural::Output> {
    #[cfg(feature = "preview")]
    if let Some(parts) = evani_procedural::forms::form(v.f("preview"), 0.3, s) {
        let steel = evani_procedural::surface::presets::METAL_STEEL_BRUSHED.tint([0.06, 0.06, 0.065]).seed(s);
        let sf = sf.uv(evani_procedural::Uv::Box);
        return Some(evani_procedural::Output::Parts(parts.into_iter().map(|(m, fin)| evani_procedural::Part::surfaced(m, if fin { sf } else { steel })).collect()));
    }
    None
}

/// The oak block's wood, its live-tuned defaults: ring spacing (mm), warp,
/// ring contrast, knots, relief, rays, tone streaks, roughness, the pith's
/// height (share of the height) and front-to-back place (share of the depth
/// from the middle).
#[cfg(feature = "cat-geometry")]
const OAK_WOOD: [f32; 10] = [11.0, 0.45, 0.32, 0.6, 0.5, 0.25, 0.6, 0.5, 0.13, -0.12];

/// The faceted concrete ball's shape and concrete (its own file; the live
/// faceted-ball session, 2026-11-04).
#[cfg(feature = "geometry-icosphere")]
mod facet_ball;

/// The batch-B primitives (one feature each).
#[cfg(feature = "cat-geometry")]
#[allow(unused_imports, dead_code)]
pub fn register(_r: &mut evani_procedural::Registry) {
    use evani_procedural::{Output, ParamSpec as P, Values, build as b, geo, shapes as sh};
    const COLOR: P = P::color("color", [0.6, 0.6, 0.64]).describe("Colour");

    // Realism: a solid oak block by default (any finish), its rounded
    // edges catching the light along the grain. The oak's live session: the
    // wood's layers as live params (each sets the solid oak's number; the
    // asset's own finish only), the pith placed in the block (its rings on
    // the ends, its checks running in towards it).
    const OAK: [f32; 3] = evani_procedural::surface::OAK;
    basic!(_r, "geometry-bevelled-box", "geometry", "bevelled-box", "Bevelled box", "accent",
        [P::num("width", 1.0, 0.1, 10.0).unit("m").describe("Width (m)"), P::num("height", 0.8, 0.1, 10.0).unit("m").describe("Height (m)"), P::num("bevel", 0.09, 0.0, 0.5).unit("m").describe("Edge rounding (m)"),
         P::color("color", OAK).describe("Colour (at its default the finish keeps its own)"), P::num("finish", 4.0, 0.0, 6.0).step(1.0).describe(evani_procedural::surface::FINISH_HELP), P::num("wear", 0.2, 0.0, 1.0).describe("Edge wear: dents and rubbed edges (0: new)"), P::num("preview", 0.0, 0.0, 6.0).step(1.0).describe(evani_procedural::forms::HELP),
         P::num("checks", 0.6, 0.0, 1.0).describe("Seasoning checks: splits carved into the end grain, from the edge in towards the pith (0: none)"),
         P::num("ring_mm", OAK_WOOD[0], 2.0, 20.0).unit("mm").describe("Oak: growth ring spacing (mm; each year its own width round this)"),
         P::num("warp", OAK_WOOD[1], 0.0, 1.0).describe("Oak: how far the rings wander (fine wavy lines and figure on the long faces)"),
         P::num("contrast", OAK_WOOD[2], 0.0, 1.0).describe("Oak: how dark each ring's pore band is"),
         P::num("knots", OAK_WOOD[3], 0.0, 1.0).describe("Oak: pin knots, the rings swirling round them"),
         P::num("relief", OAK_WOOD[4], 0.0, 1.0).describe("Oak: the open pores sunk into the surface (0 flat)"),
         P::num("rays", OAK_WOOD[5], 0.0, 1.0).describe("Oak: fine hairline checks along the rays on the end grain (0: none)"),
         P::num("streaks", OAK_WOOD[6], 0.0, 1.0).describe("Oak: long tone streaks with the grain"),
         P::num("rough", OAK_WOOD[7], 0.0, 1.0).describe("Oak: roughness of the oiled finish"),
         P::num("pith_y", OAK_WOOD[8], -1.0, 2.0).describe("Oak: the pith's height, as a share of the block's height (its rings centre there on the ends)"),
         P::num("pith_z", OAK_WOOD[9], -2.0, 2.0).describe("Oak: the pith's place front to back, as a share of the depth from the middle")],
        |v: &Values, s: u32| {
            let (w, h) = (v.f("width"), v.f("height"));
            let size = [w, h, w * 0.8];
            let mut m = oak_block(size, v.f("bevel"), v.f("wear") * 0.25, s, v.density());
            // The pith (m from the base centre: y, z).
            let pith = [v.f("pith_y") * h, v.f("pith_z") * size[2]];
            // Seasoning checks carved into the wood (real geometry, live
            // `checks`): on the end grain, from the edge in along a ray
            // towards the pith, each dark inside; fewer on the lower tiers.
            oak_checks(&mut m, size, v.f("bevel"), v.f("checks").clamp(0.0, 1.0) * v.density().clamp(0.25, 1.0), pith, s);
            use evani_procedural::surface::{finish_tinted, solid_oak};
            let look = |x| {
                let sp = v.f("ring_mm").clamp(2.0, 20.0) * 0.001;
                solid_oak(x)
                    .number(0, v.f("rough"))
                    .number(1, (0.25 / sp - 1.0) / 99.0)
                    .number(2, v.f("warp"))
                    .number(3, v.f("contrast"))
                    .number(4, v.f("knots"))
                    .number(5, (pith[0] + 1.0) * 0.5)
                    .number(6, v.f("relief"))
                    .number(7, v.f("rays"))
                    .number(8, v.f("streaks"))
                    .number(9, (pith[1] + 1.0) * 0.5)
            };
            let sf = finish_tinted(v.f("finish"), v.color("color"), OAK, 4.0, look).seed(s).scale(0.5);
            if let Some(o) = preview(v, sf, s) {
                return o;
            }
            // Oak is solid wood (round 8): the grain runs along x through
            // the volume, so the ends and the rounding into them show end
            // grain by themselves, one piece with no seam.
            Output::Parts(vec![evani_procedural::Part::surfaced(m, sf)])
        });

    // Realism: a polished granite sphere by default (any finish), wrapped
    // pole to pole so the stone's speckle runs round it. Round 9: back to
    // r7's fine grain and honed roughness (r8's coarser crystals and satin
    // polish read as terrazzo with a glassy rim), the crystals now blended.
    // Live granite session round A: the granite's own sliders (they draw only
    // on the default granite finish): crystals of four minerals with mixed
    // sizes and ragged, interlocking edges, each varied inside (rc_stone.wgsl
    // variant 9 reads numbers 1, 2, 4, 6 and 7 for them).
    const GRANITE: [f32; 3] = [0.3, 0.3, 0.29];
    basic!(_r, "geometry-uv-sphere", "geometry", "uv-sphere", "UV sphere", "accent",
        [P::num("radius", 0.5, 0.05, 10.0).unit("m").describe("Radius (m)"), P::num("segments", 64.0, 4.0, 96.0).step(1.0).describe("Segments round (rings are half as many)"),
         P::color("color", GRANITE).describe("Colour (at its default the finish keeps its own)"), P::num("finish", 3.0, 0.0, 6.0).step(1.0).describe(evani_procedural::surface::FINISH_HELP), P::num("preview", 0.0, 0.0, 6.0).step(1.0).describe(evani_procedural::forms::HELP),
         P::num("rough", 0.38, 0.05, 1.0).describe("Granite: polish roughness (lower: a tighter, brighter highlight)"), P::num("crystal", 12.0, 4.0, 30.0).unit("mm").describe("Granite: average feldspar and quartz crystal size (mm); the dark flakes are a third of it"),
         P::num("dark", 0.22, 0.0, 0.5).describe("Granite: share of black mica and hornblende crystals (they gather in clusters)"), P::num("sizes", 0.35, 0.0, 0.5).describe("Granite: how much crystal sizes vary"),
         P::num("buff", 0.2, 0.0, 0.8).describe("Granite: buff feldspar in its patches"), P::num("quartz", 0.35, 0.0, 0.6).describe("Granite: share of smoky grey quartz"),
         P::num("inner", 1.0, 0.0, 2.0).describe("Granite: variation inside each crystal (mottle and cleavage)")],
        |v: &Values, s: u32| {
            let (r, n) = (v.f("radius"), v.f("segments").round().max(4.0) as u32);
            let rings = (n / 2).max(2);
            let prof: Vec<[f32; 2]> = (0..=rings)
                .map(|i| {
                    let a = -sh::PI * 0.5 + sh::PI * i as f32 / rings as f32;
                    [r * a.cos(), r + r * a.sin()]
                })
                .collect();
            let granite = |x: evani_procedural::surface::Surface| {
                x.tint(GRANITE)
                    .variant(9)
                    .extras(0)
                    .number(0, v.f("rough"))
                    .number(1, v.f("dark") / 0.5)
                    .number(2, v.f("sizes") / 0.5)
                    .number(3, (1000.0 / v.f("crystal").clamp(4.0, 30.0) - 1.0) / 199.0)
                    .number(4, v.f("buff") / 0.8)
                    .number(5, 0.0)
                    .number(6, v.f("quartz") / 0.6)
                    .number(7, v.f("inner") / 2.0)
            };
            let sf = evani_procedural::surface::finish_tinted(v.f("finish"), v.color("color"), GRANITE, 3.0, granite).seed(s).uv(evani_procedural::Uv::Sphere).scale(1.6);
            if let Some(o) = preview(v, sf, s) {
                return o;
            }
            Output::Parts(vec![evani_procedural::Part::surfaced(b::revolve(&prof, n), sf)])
        });

    // Realism: a faceted cast-concrete sculpture by default (any finish),
    // its facets hand-finished (slightly uneven). The live faceted-ball
    // session (2026-11-04, best.png): 80 cast panels, each a shallow
    // three-sided pyramid with crisp creases and a chamfered rim, hairline
    // mid-grey joints, panels a little different in tone; the concrete's own
    // layers live as on the cube, its pores at a real size, clustered medium
    // pits, soft smeared cement patches and water marks (`facet_ball`).
    #[cfg(feature = "geometry-icosphere")]
    const POLISHED: [f32; 3] = facet_ball::COLOR;
    basic!(_r, "geometry-icosphere", "geometry", "icosphere", "Icosphere", "accent",
        [P::num("radius", 0.5, 0.05, 10.0).unit("m").describe("Radius (m)"), P::num("detail", 1.0, 0.0, 4.0).step(1.0).describe("Subdivisions (0: an icosahedron)"),
         P::color("color", POLISHED).describe("Colour (at its default the finish keeps its own)"), P::num("finish", 1.0, 0.0, 6.0).step(1.0).describe(evani_procedural::surface::FINISH_HELP), P::num("preview", 0.0, 0.0, 6.0).step(1.0).describe(evani_procedural::forms::HELP),
         P::num("relief", facet_ball::RELIEF, 0.0, 0.2).describe("Each panel a shallow three-sided pyramid: its centre raised by this share of the radius (0: flat panels)"),
         P::num("seam", 0.5, 0.0, 20.0).unit("mm").describe("The joint between the flat cast panels: its width each side (mm; 0: none)"),
         P::num("seam_depth", 1.5, 0.0, 10.0).unit("mm").describe("The joint's depth (mm)"),
         P::num("seam_tone", 0.7, 0.05, 1.0).describe("How light the joint's walls are next to the panels (the shadowed gap; 1: as light)"),
         P::num("patches", 0.85, 0.0, 1.0).describe("Soft light and darker cement patches a few centimetres across, smeared down the ball (0: none)"),
         P::num("concrete", 1.0, 0.0, 4.0).step(1.0).describe(evani_procedural::surface::CONCRETE_HELP),
         P::num("view", 0.0, 0.0, 6.0).step(1.0).describe("Diagnostic view of the concrete: 0 the final picture, 1 base colour, 2 normal, 3 roughness, 4 porosity, 5 edge wear and cavity, 6 height"),
         P::num("rough", 0.45, -1.0, 1.0).describe("Concrete roughness (below 0: the preset's)"),
         P::num("mottle", 1.0, -1.0, 1.0).describe("Concrete macro tone: pour clouds, warm and cool drift (below 0: the preset's)"),
         P::num("aggregate", -1.0, -1.0, 1.0).describe("Concrete aggregate showing through (below 0: the preset's)"),
         P::num("pores", 0.12, -1.0, 1.0).describe("Concrete tiny and small pores (below 0: the preset's)"),
         P::num("weathering", 0.35, -1.0, 1.0).describe("Concrete weathering: worn ridges and grime (below 0: the preset's)"),
         P::num("blowholes", 0.025, -1.0, 1.0).describe("Medium pits: air voids a few millimetres across, in clusters (0: none; below 0: the preset's)"),
         P::num("grain", 0.45, 0.0, 4.0).describe("Concrete surface relief: the pores' and grain's normal strength (1 as built, 0 flat)"),
         P::num("stains", 0.3, 0.0, 1.0).describe("Soft darker water marks, larger and fainter than the patches (0: none)"),
         P::num("streaks", 0.0, 0.0, 1.0).describe("Thin darker runs down the sides (0: none)"),
         P::num("pattern", 2.2, 1.0, 8.0).describe("How much finer the concrete's pattern is than a wall's: stains, pores and grain at a small sculpture's size (1: as the cube)")],
        |v: &Values, s: u32| {
            let sf = facet_ball::surface(v, s, 1.0);
            if let Some(o) = preview(v, sf, s) {
                return o;
            }
            Output::Parts(facet_ball::parts(v, v.f("radius"), s))
        });

    // Realism: a dressed sandstone column drum by default (any finish), its
    // rims softened, its surface a little off true like cut stone.
    const SANDSTONE: [f32; 3] = evani_procedural::surface::SANDSTONE;
    basic!(_r, "geometry-cylinder", "geometry", "cylinder", "Cylinder", "accent",
        [P::num("radius", 0.4, 0.02, 10.0).unit("m").describe("Radius (m)"), P::num("height", 1.2, 0.02, 20.0).unit("m").describe("Height (m)"), P::num("sides", 64.0, 3.0, 128.0).step(1.0).describe("Sides round"),
         P::color("color", SANDSTONE).describe("Colour (at its default the finish keeps its own)"), P::num("finish", 6.0, 0.0, 6.0).step(1.0).describe(evani_procedural::surface::FINISH_HELP), P::num("preview", 0.0, 0.0, 6.0).step(1.0).describe(evani_procedural::forms::HELP),
         P::num("chips", 0.6, 0.0, 1.0).describe("Broken chips on the rims and at the foot, and a tooled top (0: a clean, new drum)")],
        |v: &Values, s: u32| {
            let (r, h) = (v.f("radius"), v.f("height"));
            let rim = (r.min(h) * 0.03).clamp(0.002, 0.03);
            let sf = evani_procedural::surface::finish_tinted(v.f("finish"), v.color("color"), SANDSTONE, 6.0, |x| x.tint(SANDSTONE).variant(10).number(2, 0.35).number(3, 0.9).condition("mud", 0.04)).seed(s).uv(evani_procedural::Uv::Cylinder).scale(1.2);
            let bedded = (v.f("finish").round() - 6.0).abs() < 0.5;
            let m = column_drum(r, h, rim, v.f("sides").round().max(3.0) as u32, v.f("chips").clamp(0.0, 1.0), bedded, &sf, s, v.density());
            if let Some(o) = preview(v, sf, s) {
                return o;
            }
            Output::Parts(vec![evani_procedural::Part::surfaced(m, sf)])
        });

    // Realism: a cast-concrete garden cone by default (any finish), its tip
    // and base rim rounded as a cast piece comes from its mould. Round 8 (the
    // reference): a blunt tip of about 4 cm and a small flared foot ring.
    // The live cone session, round 8: a barely warm grey (round 5's
    // grey-beige read sand-tan, round 7's grey too cool).
    const CONE_CONCRETE: [f32; 3] = [0.349, 0.33, 0.296];
    basic!(_r, "geometry-cone", "geometry", "cone", "Cone", "accent",
        [P::num("radius", 0.5, 0.02, 10.0).unit("m").describe("Base radius (m)"), P::num("height", 1.2, 0.02, 20.0).unit("m").describe("Height (m)"), P::num("sides", 64.0, 3.0, 128.0).step(1.0).describe("Sides round"),
         P::color("color", CONE_CONCRETE).describe("Colour (at its default the finish keeps its own)"), P::num("finish", 0.0, 0.0, 6.0).step(1.0).describe(evani_procedural::surface::FINISH_HELP), P::num("preview", 0.0, 0.0, 6.0).step(1.0).describe(evani_procedural::forms::HELP),
         P::num("chips", 0.0, 0.0, 1.0).describe("Wear: small chips knocked out of the foot ring's rounded top and the tip (0: a new casting)"),
         P::num("rough", -1.0, -1.0, 1.0).describe("Concrete roughness (below 0: the cone's own)"),
         P::num("mottle", -1.0, -1.0, 1.0).describe("Concrete macro tone: pour clouds, warm and cool drift (below 0: the cone's own)"),
         P::num("aggregate", -1.0, -1.0, 1.0).describe("Concrete aggregate showing through (below 0: the cone's own)"),
         P::num("pores", -1.0, -1.0, 1.0).describe("Concrete tiny and small pores (below 0: the cone's own)"),
         P::num("weathering", -1.0, -1.0, 1.0).describe("Concrete weathering: worn edges and grime (below 0: the cone's own)"),
         P::num("blowholes", -1.0, -1.0, 1.0).describe("Concrete large blowholes (below 0: the cone's own)"),
         P::num("blotches", -1.0, -1.0, 1.0).describe("Cast-piece blotches: darker grey clouds and pale patches, their strength (below 0: the cone's own)"),
         P::num("coverage", -1.0, -1.0, 1.0).describe("How much of the cone the blotches cover (below 0: the cone's own)"),
         P::num("tip", 0.22, 0.0, 0.5).describe("Tip roundness: the cap's radius as a share of the smaller of base radius and height"),
         P::num("view", 0.0, 0.0, 6.0).step(1.0).describe("Diagnostic view of the concrete: 0 the final picture, 1 base colour, 2 normal, 3 roughness, 4 pores and patches, 5 edge wear and cavity, 6 height")],
        |v: &Values, s: u32| {
            let (r, h) = (v.f("radius"), v.f("height"));
            // The live cone session (best.png measured against a render
            // from its solved camera): the top is a broad round cap, about
            // 11 cm across at the defaults, standing at the full height (r9's
            // 4 cm cap took 6 cm off the top), so the sides run from the
            // base radius at the floor up to the cap's tangent, a little
            // steeper than base over height (19.4 degrees at the defaults).
            let rt0 = (h.min(r) * v.f("tip").clamp(0.0, 0.5)).clamp(0.003, 0.15).min(h * 0.45);
            let (big, phi) = ((r * r + (h - rt0) * (h - rt0)).sqrt().max(1e-6), (h - rt0).atan2(r));
            let th = ((rt0 / big).clamp(-1.0, 1.0).acos() - phi).max(0.0);
            // Round 4 (best.png measured on both edges: the same flank angle,
            // the flanks 13 px, 1 cm a side, further in at every height): the
            // side line 2% of the radius in, parallel, and the cap re-fitted
            // tangent to it with its top still at the full height (9.5 cm at
            // the defaults); the collar where it was.
            let rs = r * 0.98;
            let (sn, cs) = th.sin_cos();
            let rt = ((rs * cs - h * sn) / (1.0 - sn).max(1e-3)).clamp(0.003, rt0);
            let side = |y: f32| rs - y * th.tan();
            // The foot: a cast collar (best.png's foot), an upright band
            // with a rounded bottom, its top a broad rounded shoulder that
            // runs into a small groove where the cone rises from it (r9's
            // half torus stood too far out; the collar is about 2.5 cm proud
            // of the cone and 8.5 cm high at the defaults).
            let lip = (r * 0.07).min(h * 0.05);
            // Round 4 (the scored review): a taller bullnose, its top edge
            // rounded over about half its height, the same width as before.
            let ro = r - lip * 2.45 * th.tan() + lip * 0.7;
            let hc = lip * 2.95;
            // Round 5: the face more upright, only its top edge rounded.
            let (rb, rr) = (lip * 0.3, lip * 0.95);
            let mut prof = vec![[0.0, 0.0], [ro - rb, 0.0]];
            for k in 1..=3 {
                let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::FRAC_PI_2 * k as f32 / 3.0;
                prof.push([ro - rb + rb * a.cos(), rb + rb * a.sin()]);
            }
            // The shoulder: a quarter round from the band's face over its
            // top, cut where it meets the cone's side.
            let (cx, cy) = (ro - rr, hc - rr);
            for k in 0..=10 {
                let a = std::f32::consts::FRAC_PI_2 * k as f32 / 10.0;
                let q = [cx + rr * a.cos(), cy + rr * a.sin()];
                if q[0] < side(q[1]) + lip * 0.06 {
                    break;
                }
                prof.push(q);
            }
            // The groove: a small step in, then the cone's side.
            let yg = hc + lip * 0.08;
            prof.push([side(yg) - lip * 0.09, yg]);
            prof.push([side(yg + lip * 0.35), yg + lip * 0.35]);
            // The cap: from its tangent with the side over to the axis.
            // Round 5 (best.png's crown is flat-topped with shoulders): the
            // cap flattens towards its top, a superellipse whose exponent runs
            // from 2 at the flank's tangent to 2.6 at the crown.
            let a0 = th;
            for k in 0..=10 {
                let t = k as f32 / 10.0;
                let a = a0 + (std::f32::consts::FRAC_PI_2 - a0) * t;
                let e = 2.0 / (2.0 + 0.6 * t);
                prof.push([if k == 10 { 0.0 } else { rt * a.cos().max(0.0).powf(e) }, h - rt + rt * a.sin().max(0.0).powf(e)]);
            }
            let yc = h - rt;
            let lx = ro - rr;
            // Twice the sides asked at High and Ultra (round 9: 64 showed as
            // faint vertical strips down the cone in a close-up), as asked
            // below.
            let sides = v.f("sides").round().max(3.0) as u32;
            let n = if v.density() >= 0.75 { sides * 2 } else { sides };
            let mut m = b::revolve(&prof, n);
            evani_procedural::detail::perturb(&mut m, (r.min(h) * 0.04).clamp(0.002, 0.04) * 0.15, r.max(0.2), s);
            // The live cone session: a few small chips knocked out of the
            // foot ring's rounded top and one or two at the tip (real
            // geometry, live `chips`), as a garden piece that has been
            // moved about.
            let chips = v.f("chips").clamp(0.0, 1.0);
            if chips > 0.0 {
                use evani_procedural::detail;
                detail::weld(&mut m);
                let mut all = Vec::new();
                let k = (1.0 + 4.0 * chips).round() as u32;
                for i in 0..k {
                    let a = sh::rnd(s, i, 41) * std::f32::consts::TAU;
                    let (at, cr) = if i < k {
                        // On the ring's top, a little to its outside.
                        let (rr, y) = (lx + lip * 0.62, hc - lip * 0.2);
                        ([rr * a.cos(), y, rr * a.sin()], lip * (0.22 + 0.25 * sh::rnd(s, i, 42)))
                    } else {
                        // Just under the tip's cap.
                        let y = yc + rt * 0.2 - (i - k) as f32 * rt * 0.9;
                        let rr = side(y.max(0.0)) * 0.98;
                        ([rr * a.cos(), y, rr * a.sin()], rt * (0.15 + 0.12 * sh::rnd(s, i, 42)))
                    };
                    let cr = cr * (0.6 + 0.4 * chips);
                    all.push(detail::Chip { at, radius: cr, depth: cr * (0.3 + 0.25 * sh::rnd(s, i, 43)), fine: (cr * 0.55 / v.density().clamp(0.25, 2.0)).max(0.004), seed: s ^ i.wrapping_mul(0x2545_f491) });
                }
                let moved = detail::spalls(&mut m, &all);
                detail::smooth_normals_where(&mut m, &moved);
            }
            let layered = |x| {
                let mut c = cone_concrete(evani_procedural::surface::cast_concrete(x).tint(CONE_CONCRETE));
                for (name, slot) in [("rough", 0), ("mottle", 1), ("aggregate", 2), ("pores", 5), ("weathering", 6), ("blowholes", 7), ("blotches", 8), ("coverage", 9)] {
                    let n = v.f(name);
                    if n >= 0.0 {
                        c = c.number(slot, n);
                    }
                }
                evani_procedural::surface::concrete_view(c, v.f("view"))
            };
            let sf = evani_procedural::surface::finish_tinted(v.f("finish"), v.color("color"), CONE_CONCRETE, 0.0, layered).seed(s).uv(evani_procedural::Uv::Cylinder).scale(1.5);
            if let Some(o) = preview(v, sf, s) {
                return o;
            }
            Output::Parts(vec![evani_procedural::Part::surfaced(m, sf)])
        });

    basic!(_r, "geometry-capsule", "geometry", "capsule", "Capsule", "accent",
        [P::num("radius", 0.3, 0.02, 5.0).unit("m").describe("Radius (m)"), P::num("length", 0.8, 0.0, 10.0).unit("m").describe("Straight middle length (m)"), COLOR],
        |v: &Values, _s: u32| {
            let (r, l) = (v.f("radius"), v.f("length"));
            let mut prof = Vec::new();
            for i in 0..=8 {
                let a = -sh::PI * 0.5 + sh::PI * 0.5 * i as f32 / 8.0;
                prof.push([r * a.cos(), r + r * a.sin()]);
            }
            for i in 0..=8 {
                let a = sh::PI * 0.5 * i as f32 / 8.0;
                prof.push([r * a.cos(), r + l + r * a.sin()]);
            }
            Output::Mesh(b::revolve(&prof, 32))
        });

    basic!(_r, "geometry-torus", "geometry", "torus", "Torus", "accent",
        [P::num("radius", 0.6, 0.05, 10.0).unit("m").describe("Ring radius (m)"), P::num("tube", 0.18, 0.01, 3.0).unit("m").describe("Tube radius (m)"), COLOR],
        |v: &Values, _s: u32| {
            let (big, t) = (v.f("radius"), v.f("tube").min(v.f("radius") * 0.95));
            // A circle walked counter-clockwise round (big, t), turned round y.
            let prof: Vec<[f32; 2]> = (0..=24)
                .map(|i| {
                    let a = -sh::PI * 0.5 + i as f32 / 24.0 * sh::TAU;
                    [big + t * a.cos(), t + t * a.sin()]
                })
                .collect();
            Output::Mesh(b::revolve(&prof, 48))
        });

    basic!(_r, "geometry-plane-grid", "geometry", "plane-grid", "Plane / grid", "accent",
        [P::num("size", 2.0, 0.2, 40.0).unit("m").describe("Side (m)"), P::num("cells", 8.0, 1.0, 64.0).step(1.0).describe("Cells a side (raised grid lines)"), COLOR],
        |v: &Values, _s: u32| {
            let (size, n) = (v.f("size"), v.f("cells").round().max(1.0) as u32);
            let mut m = sh::block([size, 0.01, size], [0.0; 3]);
            let w = (size / n as f32 * 0.06).max(0.004);
            for i in 0..=n {
                let x = (i as f32 / n as f32 - 0.5) * size;
                geo::merge(&mut m, &sh::block([w, 0.02, size], [x, 0.0, 0.0]));
                geo::merge(&mut m, &sh::block([size, 0.02, w], [0.0, 0.0, x]));
            }
            Output::Mesh(m)
        });

    basic!(_r, "geometry-disc-ring", "geometry", "disc-ring", "Disc / ring", "accent",
        [P::num("radius", 0.6, 0.05, 10.0).unit("m").describe("Outer radius (m)"), P::num("hole", 0.4, 0.0, 0.95).describe("Hole, as a share of the radius (0: a disc)"), COLOR],
        |v: &Values, _s: u32| {
            let (r, k) = (v.f("radius"), v.f("hole"));
            let t = (r * 0.04).max(0.01);
            let ri = r * k;
            let prof = if ri > 1e-3 { vec![[ri, 0.0], [r, 0.0], [r, t], [ri, t], [ri, 0.0]] } else { vec![[0.0, 0.0], [r, 0.0], [r, t], [0.0, t]] };
            Output::Mesh(b::revolve(&prof, 64))
        });

    basic!(_r, "geometry-pyramid", "geometry", "pyramid", "Pyramid", "accent",
        [P::num("base", 1.0, 0.05, 20.0).unit("m").describe("Base width (m)"), P::num("height", 0.9, 0.05, 20.0).unit("m").describe("Height (m)"), P::num("sides", 4.0, 3.0, 12.0).step(1.0).describe("Sides"), COLOR],
        |v: &Values, _s: u32| {
            let n = v.f("sides").round().max(3.0) as u32;
            let mut m = sh::frustum(v.f("base") * 0.5 / (sh::PI / n as f32).cos(), 0.0, v.f("height"), n);
            sh::rotate(&mut m, [0.0, 1.0, 0.0], sh::PI / n as f32);
            sh::flat_shade(&mut m);
            Output::Mesh(m)
        });

    basic!(_r, "geometry-wedge-ramp", "geometry", "wedge-ramp", "Wedge / ramp", "accent",
        [P::num("length", 2.0, 0.1, 20.0).unit("m").describe("Length (m)"), P::num("height", 0.6, 0.05, 10.0).unit("m").describe("Height at the top (m)"), P::num("width", 1.0, 0.1, 10.0).unit("m").describe("Width (m)"), COLOR],
        |v: &Values, _s: u32| {
            let (l, h) = (v.f("length"), v.f("height"));
            Output::Mesh(b::extrude(&[[-l * 0.5, 0.0], [l * 0.5, 0.0], [l * 0.5, h]], v.f("width")))
        });

    basic!(_r, "geometry-lathed-rotational-mesh", "geometry", "lathed-rotational-mesh", "Lathed rotational mesh", "accent",
        [P::num("height", 1.0, 0.1, 10.0).unit("m").describe("Height (m)"), P::num("belly", 0.35, 0.05, 3.0).unit("m").describe("Widest radius (m)"), P::num("neck", 0.4, 0.1, 1.0).describe("Neck, as a share of the belly"), COLOR],
        |v: &Values, s: u32| {
            let (h, r, k) = (v.f("height"), v.f("belly"), v.f("neck"));
            // A vase: a seeded swell along a smooth profile, walled and open.
            let n = 14;
            let mut prof: Vec<[f32; 2]> = (0..=n)
                .map(|i| {
                    let t = i as f32 / n as f32;
                    let swell = (t * sh::PI).sin().max(0.0).powf(0.8);
                    let rr = r * (0.45 + 0.55 * swell) * (1.0 - (1.0 - k) * t.powi(3)) * (1.0 + 0.06 * sh::range(s, i, 1, -1.0, 1.0));
                    [rr, t * h]
                })
                .collect();
            prof.insert(0, [0.0, 0.0]);
            let wall = (r * 0.06).max(0.005);
            let top = prof[prof.len() - 1];
            prof.push([top[0] - wall, top[1]]);
            prof.push([top[0] - wall, h * 0.85]);
            Output::Mesh(b::revolve(&prof, 40))
        });

    basic!(_r, "geometry-extruded-polygon", "geometry", "extruded-polygon", "Extruded polygon", "accent",
        [P::num("points", 5.0, 3.0, 16.0).step(1.0).describe("Points (or sides)"), P::num("radius", 0.6, 0.05, 10.0).unit("m").describe("Radius (m)"), P::num("inner", 0.5, 0.2, 1.0).describe("Inner corners, as a share of the radius (1: a plain polygon)"), P::num("depth", 0.25, 0.01, 5.0).unit("m").describe("Extrusion depth (m)"), COLOR],
        |v: &Values, _s: u32| Output::Mesh(b::prism(&b::star(v.f("points").round() as u32, v.f("radius"), v.f("inner")), 0.0, v.f("depth"))));

    basic!(_r, "geometry-swept-spline-tube", "geometry", "swept-spline-tube", "Swept spline / tube", "accent",
        [P::num("length", 2.5, 0.2, 30.0).unit("m").describe("Length (m)"), P::num("radius", 0.08, 0.005, 2.0).unit("m").describe("Tube radius (m)"), P::num("curl", 0.6, 0.0, 2.0).describe("How far the spline wanders"), COLOR],
        |v: &Values, s: u32| {
            let (l, r, c) = (v.f("length"), v.f("radius"), v.f("curl"));
            // Seeded control points along x, joined by a Catmull-Rom spline.
            let ctrl: Vec<[f32; 3]> = (0..6u32)
                .map(|i| {
                    let t = i as f32 / 5.0;
                    [(t - 0.5) * l, r + c * l * 0.25 * sh::rnd(s, i, 1), c * l * 0.3 * (sh::rnd(s, i, 2) - 0.5)]
                })
                .collect();
            let mut pts = Vec::new();
            for i in 0..ctrl.len() - 1 {
                let p0 = ctrl[i.saturating_sub(1)];
                let (p1, p2) = (ctrl[i], ctrl[i + 1]);
                let p3 = ctrl[(i + 2).min(ctrl.len() - 1)];
                for k in 0..12 {
                    let t = k as f32 / 12.0;
                    let (t2, t3) = (t * t, t * t * t);
                    pts.push(std::array::from_fn(|j| 0.5 * (2.0 * p1[j] + (p2[j] - p0[j]) * t + (2.0 * p0[j] - 5.0 * p1[j] + 4.0 * p2[j] - p3[j]) * t2 + (3.0 * p1[j] - p0[j] - 3.0 * p2[j] + p3[j]) * t3)));
                }
            }
            pts.push(ctrl[ctrl.len() - 1]);
            Output::Mesh(b::sweep(&pts, &b::circle(r, 16), true))
        });
}

/// The oak block's rounded box (the oak's live session): as
/// [`evani_procedural::detail::made_block_at`], but its grid puts 8 of each
/// face's 12 half-steps on the rounding, so every rounding is 16 facets of
/// under 6 degrees (the shared grid's bunching left 4 or 5 there, and the
/// solid wood's rings, nearly tangent to a rounding, showed each facet as a
/// stair-step), and 4 on the flat, where nothing bends. 6,912 triangles
/// before the checks; edge wear as the shared block's.
#[cfg(feature = "cat-geometry")]
#[allow(dead_code)]
fn oak_block(size: [f32; 3], bevel: f32, wear: f32, seed: u32, density: f32) -> evani_procedural::geo::Mesh {
    use evani_procedural::{detail, geo};
    let h = [size[0] * 0.5, size[1] * 0.5, size[2] * 0.5];
    let r = bevel.clamp(0.0005, h[0].min(h[1]).min(h[2]));
    let mut m = geo::segmented_box(size, 24);
    for v in &mut m.vertices {
        let p: [f32; 3] = std::array::from_fn(|k| {
            let u = ((if k == 1 { v[1] - h[1] } else { v[k] }) / h[k].max(1e-6)).clamp(-1.0, 1.0);
            let a = u.abs();
            let f = h[k] - r;
            u.signum() * if a <= 1.0 / 3.0 { a * 3.0 * f } else { f + (a - 1.0 / 3.0) * 1.5 * r }
        });
        let inner: [f32; 3] = std::array::from_fn(|k| p[k].clamp(-(h[k] - r), h[k] - r));
        let d: [f32; 3] = std::array::from_fn(|k| p[k] - inner[k]);
        let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        let n = if l > 1e-6 { d.map(|x| x / l) } else { [v[3], v[4], v[5]] };
        let q: [f32; 3] = if l > 1e-6 { std::array::from_fn(|k| inner[k] + n[k] * r) } else { p };
        *v = [q[0], q[1] + h[1], q[2], n[0], n[1], n[2]];
    }
    detail::weld(&mut m);
    detail::box_normals(&mut m, size, [0.0; 3], r);
    let small = size[0].min(size[1]).min(size[2]);
    let mut ew = detail::EdgeWear::new(wear, seed);
    ew.size = ew.size.min(small * 0.06);
    let moved = detail::edge_wear(&mut m, &detail::box_arrises(size, [0.0; 3], r), &ew, density);
    detail::smooth_normals_where(&mut m, &moved);
    m
}

/// The oak block's seasoning checks (`checks` 0..1; the oak's live
/// session): radial splits on the end grain, as a drying log opens along its
/// rays. Each starts just over the end face's edge (on the rounding into the
/// next face, where it notches the silhouette), widest there, and runs in
/// along a ray towards the pith (`pith`: y, z from the block's base centre,
/// where the solid-wood material centres its rings), closing to a hairline
/// before it. Mostly upwards and sideways (none into the floor). Real
/// geometry: its inside reads dark (the recipe stage's cavity term).
#[cfg(feature = "cat-geometry")]
#[allow(dead_code)]
fn oak_checks(m: &mut evani_procedural::geo::Mesh, size: [f32; 3], bevel: f32, checks: f32, pith: [f32; 2], s: u32) {
    use evani_procedural::{detail, shapes as sh};
    if checks <= 0.0 {
        return;
    }
    let (hw, top, hd) = (size[0] * 0.5, size[1], size[2] * 0.5);
    let on = |p: [f32; 3]| detail::onto_rounded_box(p, size, [0.0; 3], bevel);
    let rnd = |k: u32| sh::rnd(s, k, 7);
    let (w, d) = (0.003 + 0.004 * checks, 0.01 + 0.014 * checks);
    let ends = if checks > 0.3 { 2 } else { 1 };
    let per = 1 + (checks * 2.2) as u32;
    for e in 0..ends {
        let sx = if e == 0 { -1.0 } else { 1.0 };
        for j in 0..per {
            let k = e * 16 + j;
            // The ray's angle from straight up (y), towards +z.
            let phi = -1.5 + 3.0 * (j as f32 + 0.2 + 0.6 * rnd(30 + k)) / per as f32;
            let (dy, dz) = (phi.cos(), phi.sin());
            // Where the ray leaves the end face.
            let mut te = f32::MAX;
            for (dir, lo, hi, at) in [(dy, 0.0, top, pith[0]), (dz, -hd, hd, pith[1])] {
                if dir > 1e-4 {
                    te = te.min((hi - at) / dir);
                } else if dir < -1e-4 {
                    te = te.min((lo - at) / dir);
                }
            }
            if !(te > 0.03 && te < 10.0) {
                continue;
            }
            let tin = te * (0.4 + 0.35 * rnd(50 + k)) * (1.1 - 0.3 * checks);
            // From just over the edge (on the next face, the rounding in
            // from the end) in along the ray, 1 cm steps, a little off line.
            let mut path = vec![on([sx * (hw - bevel * 0.6 - 0.02), pith[0] + dy * (te + 0.05), pith[1] + dz * (te + 0.05)])];
            let n = (((te - tin) / 0.01).ceil() as u32).max(2);
            for i in 0..=n {
                let t = te - (te - tin) * i as f32 / n as f32;
                let wob = 0.004 * (sh::rnd(s ^ k, i, 3) - 0.5) + 0.006 * (t * 9.0 + k as f32).sin();
                path.push(on([sx * (hw + 0.05), pith[0] + dy * t - dz * wob, pith[1] + dz * t + dy * wob]));
            }
            let size_j = 0.6 + 0.4 * rnd(70 + k);
            detail::carve_crack(m, &path, w * size_j, d * size_j);
        }
    }
}

/// The dressed sandstone column drum (round 9): a lathe with rows every
/// 2 cm up the side and rings over the top at High (fewer on the lower
/// tiers), welded smooth; where the finish is the bedded sandstone its soft
/// beds are weathered back a millimetre or two exactly where the material
/// draws them (the recipe stage's bed field on the CPU,
/// [`evani_procedural::noise::gpu`]; the drum stands at its placement's
/// origin, so the world position the shader reads is the mesh's); the top is
/// dressed with shallow tool scoops; `chips` breaks pieces out of the top
/// rim and one large one, with a few small ones, at the foot.
#[cfg(feature = "cat-geometry")]
#[allow(dead_code, clippy::too_many_arguments)]
fn column_drum(r: f32, h: f32, rim: f32, sides: u32, chips: f32, bedded: bool, sf: &evani_procedural::surface::Surface, s: u32, density: f32) -> evani_procedural::geo::Mesh {
    use evani_procedural::noise::gpu;
    use evani_procedural::{build as b, detail, geo, noise, shapes as sh};
    let dn = density.clamp(0.25, 2.0);
    let prof = detail::round_profile(&[[0.0, 0.0], [r, 0.0], [r, h], [0.0, h]], rim, 4);
    let prof = detail::resample_profile(&prof, 0.02 / dn, true);
    let n = sides.max((r * std::f32::consts::TAU / (0.025 / dn)).round() as u32).min(256);
    let mut m = b::revolve(&prof, n);
    detail::weld(&mut m);
    geo::smooth_normals(&mut m);
    let (seed, vs) = (sf.shader_seed(), 1.0 + 49.0 * sf.number_of(2));
    // The shader's bed at plane point p, height y (rc_stone_bed): its
    // weathering depth (soft beds back, a groove at each bed's edge).
    let bed = |p: [f32; 2], y: f32| -> f32 {
        let mm = gpu::fbm2([p[0] * 1.5, p[1] * 1.5], seed, 3);
        let bd = (y + p[0] * 0.05 + p[1] * 0.02 + (mm - 0.5) * 0.05) * vs * 0.5;
        let bu = bd + 0.9 * (gpu::vnoise2([bd * 0.37, 0.5], seed + 61) - 0.5);
        let bf = bu - bu.floor();
        let bh = gpu::u01(gpu::hash2([bu.floor() as i32, 3], seed + 62));
        let soft = noise::smoothstep(0.55, 0.15, bh) * (std::f32::consts::PI * bf).sin().max(0.0).sqrt();
        let edge = 1.0 - noise::smoothstep(0.0, 0.08, bf.min(1.0 - bf));
        0.0022 * soft + 0.0008 * edge
    };
    let top = h - rim * 0.5;
    for v in &mut m.vertices {
        let (p, nrm) = ([v[0], v[1], v[2]], [v[3], v[4], v[5]]);
        let mut push = 0.0;
        if bedded && nrm[1].abs() < 0.6 && p[1] > rim && p[1] < h - rim {
            // From High the shader blends the two nearest planes.
            let (a, b2, wt) = gpu::proj2(p, nrm);
            push += bed(a, p[1]) * (1.0 - wt) + bed(b2, p[1]) * wt;
        }
        if chips > 0.0 && nrm[1] > 0.9 && p[1] > top {
            // Tool scoops over the top, a few centimetres across.
            let (f1, _) = noise::cells2(p[0] / 0.055, p[2] / 0.055, s ^ 0x7001);
            push += chips * (0.0028 * (1.0 - (f1 * 1.2).min(1.0).powi(2)) + 0.0012 * noise::fbm(p[0] * 9.0, p[2] * 9.0, s ^ 0x7002, 3, 2.0, 0.5).abs());
        }
        v[0] -= nrm[0] * push;
        v[1] -= nrm[1] * push;
        v[2] -= nrm[2] * push;
    }
    geo::smooth_normals(&mut m);
    if chips > 0.0 {
        let mut all = Vec::new();
        let k = (2.0 + 6.0 * chips).round() as u32;
        let e = rim * 0.3;
        for i in 0..k + 3 {
            let a = sh::rnd(s, i, 31) * std::f32::consts::TAU;
            let foot = i >= k;
            let (y, cr) = if foot { (e, if i == k { 0.065 } else { 0.015 + 0.015 * sh::rnd(s, i, 32) }) } else { (h - e, 0.02 + 0.035 * sh::rnd(s, i, 32) * sh::rnd(s, i, 33)) };
            let cr = cr * (0.5 + 0.5 * chips);
            let at = [(r - e) * a.cos(), y, (r - e) * a.sin()];
            all.push(detail::Chip { at, radius: cr, depth: cr * (0.35 + 0.3 * sh::rnd(s, i, 34)), fine: (cr * 0.3 / dn).max(0.0025), seed: s ^ i.wrapping_mul(0x2545_f491) });
        }
        let moved = detail::spalls(&mut m, &all);
        detail::smooth_normals_where(&mut m, &moved);
    }
    m
}

/// The garden cone's own concrete (the live cone session, 2026-11-04): the
/// layered concrete with the cone's switch on (extras bit 5: cast-piece
/// blotches towards colour B, darker, and colour C, paler, both nearly the
/// base's hue (round 8: the reference's damp patches change lightness, not
/// colour), numbers 8 and 9 their strength and coverage), fewer large
/// blowholes than the cube's (best.png: fine pinholes, a few small holes).
#[cfg(feature = "geometry-cone")]
fn cone_concrete(c: evani_procedural::surface::Surface) -> evani_procedural::surface::Surface {
    let a = c.base_rgb();
    c.extras(32).set_colour(1, [a[0] * 0.635, a[1] * 0.625, a[2] * 0.6]).set_colour(2, [a[0] * 1.19, a[1] * 1.197, a[2] * 1.21]).number(1, 0.3).number(2, 0.0).number(5, 0.44).number(7, 0.075).number(8, 0.9).number(9, 0.47)
}
