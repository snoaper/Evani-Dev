//! Category 3, Rocks, minerals and geology: basic generators (batch A). Seeded
//! noise-pushed icospheres, scattered and stacked; prisms, vaults and
//! tubes for crystals, arches and veins. Inspired by three.js (MIT)
//! IcosahedronGeometry, ConeGeometry, TubeGeometry and the threejs-trees
//! rock idea (a noise-displaced sphere); written fresh here.

use evani_procedural::Registry;

#[allow(unused_imports, dead_code)]
pub fn register(_r: &mut Registry) {
    use evani_procedural::{Output, ParamSpec as P, Part, Values, geo, noise, shapes as sh};
    const STONE: [f32; 3] = [0.3, 0.29, 0.27];

    basic!(_r, "rocks-small-stones", "rocks", "small-stones", "Small stones", "stone",
        [P::num("count", 14.0, 1.0, 80.0).step(1.0).describe("Stones"), P::num("spread", 1.2, 0.1, 10.0).unit("m").describe("Radius they lie in (m)"), P::num("size", 0.18, 0.02, 1.0).unit("m").describe("Typical stone size (m)"), P::color("color", STONE).describe("Stone colour")],
        |v: &Values, s: u32| {
            let n = v.f("count").round().max(1.0) as u32;
            let mut m = geo::Mesh::default();
            for (i, p) in sh::scatter_disc(n, v.f("spread"), s).into_iter().enumerate() {
                let k = v.f("size") * sh::range(s, i as u32, 9, 0.6, 1.4);
                let r = sh::rock([k, k * 0.6, k * 0.85], s.wrapping_add(i as u32 * 17), 0.5, true);
                geo::merge(&mut m, &sh::place(&r, 1.0, sh::rnd(s, i as u32, 3) * sh::TAU, 0.0, [p[0], 0.0, p[1]]));
            }
            Output::Mesh(m)
        });

    // Realism (the Asset Studio pebbles session, 2026-11-05): a group of
    // water-worn stones of mixed rock, each in the stone recipe, with real
    // dents, chips and a ledge carved in (crate::pebbles).
    basic!(_r, "rocks-pebbles", "rocks", "pebbles", "Pebbles", "stone",
        [P::num("count", 10.0, 1.0, 200.0).step(1.0).describe("Pebbles"), P::num("spread", 0.3, 0.05, 10.0).unit("m").describe("Radius they lie in (m)"), P::num("size", 0.07, 0.01, 0.3).unit("m").describe("Typical pebble size (m)"),
         P::color("color", crate::pebbles::COLOR).describe("Pebble colour (at its default each rock keeps its own)"),
         P::num("mix", 1.0, 0.0, 1.0).describe("Rock types mixed: 0 one grey stone (the colour), 1 granites, sandstones, slate and basalt"),
         P::num("wear", 1.0, 0.0, 2.0).describe("Dents, chips and a stepped ledge carved into the stones (0: none)"),
         P::num("round", 0.0, 0.0, 1.0).describe("How water-worn: 0 as found, 1 every stone rounded smooth")],
        |v: &Values, s: u32| {
            Output::Parts(crate::pebbles::group(v.f("count").round().max(1.0) as u32, v.f("spread"), v.f("size"), v.color("color"), v.f("mix"), v.f("wear"), v.f("round"), s, v.density()))
        });

    // Realism (the Asset Studio medium rocks session, column 23,
    // 2026-11-07): a group of broken rocks of different stone (granite,
    // vein quartz, basalt, red sandstone, green schist), each in the stone
    // recipe, with crisp fracture faces, stepped bedding, shell-like breaks
    // and chips carved in (crate::medium_rocks).
    basic!(_r, "rocks-medium-rocks", "rocks", "medium-rocks", "Medium rocks", "stone",
        [P::num("size", 0.4, 0.1, 5.0).unit("m").describe("Typical rock length (m)"), P::num("roughness", 0.6, 0.0, 1.0).describe("Smooth to craggy: how uneven the broken faces are"),
         P::color("color", crate::medium_rocks::COLOR).describe("Rock colour (at its default each rock keeps its own)"),
         P::num("count", 5.0, 1.0, 40.0).step(1.0).describe("Rocks"),
         P::num("mix", 1.0, 0.0, 1.0).describe("Rock types mixed: 0 one dark stone (the colour), 1 granite, quartz, basalt, sandstone and schist"),
         P::num("wear", 1.0, 0.0, 2.0).describe("Chips and shell-like breaks carved into the rocks (0: none)")],
        |v: &Values, s: u32| {
            Output::Parts(crate::medium_rocks::group(v.f("count").round().max(1.0) as u32, v.f("size"), v.color("color"), v.f("roughness"), v.f("mix"), v.f("wear"), s, v.density()))
        });

    basic!(_r, "rocks-large-boulders", "rocks", "large-boulders", "Large boulders", "stone",
        [P::num("size", 3.0, 1.0, 15.0).unit("m").describe("Boulder size (m)"), P::num("roughness", 0.5, 0.0, 1.0).describe("Smooth to craggy"), P::color("color", [0.32, 0.3, 0.27]).describe("Rock colour")],
        |v: &Values, s: u32| {
            let k = v.f("size");
            let mut m = sh::rock([k, k * 0.72, k * 0.9], s, v.f("roughness"), false);
            // Two smaller rocks settled against it.
            for i in 0..2u32 {
                let a = sh::range(s, i, 4, 0.0, sh::TAU);
                let r = sh::rock([k * 0.35, k * 0.25, k * 0.3], s.wrapping_add(i + 5), v.f("roughness"), false);
                geo::merge(&mut m, &sh::place(&r, 1.0, a, 0.0, [a.cos() * k * 0.55, 0.0, a.sin() * k * 0.5]));
            }
            Output::Mesh(m)
        });

    // Realism (the Asset Studio jagged rock formations session, column 24,
    // 2026-11-07): an outcrop of foliated grey schist broken into leaning
    // blades and spires that share one foliation, their sheets stepped and
    // their edges chipped as real geometry, blocks at its foot and rubble in
    // front, the rust and pale quartz-rich stone in patches across it, all in
    // the stone recipe (crate::jagged_rocks).
    basic!(_r, "rocks-jagged-rock-formations", "rocks", "jagged-rock-formations", "Jagged rock formations", "stone",
        [P::num("height", 3.5, 0.5, 30.0).unit("m").describe("Tallest spire (m)"), P::num("spires", 6.0, 1.0, 20.0).step(1.0).describe("Spires"),
         P::color("color", crate::jagged_rocks::COLOR).describe("Rock colour (at its default the grey, rusty and pale stones keep their own)"),
         P::num("rubble", 1.0, 0.0, 2.0).describe("Fallen stones and gravel in front (0: none)"),
         P::num("rust", 0.5, 0.0, 1.0).describe("How much of the rock is rusty iron-stained weathering"),
         P::num("wear", 1.0, 0.0, 2.0).describe("Chips broken out of the blades' edges (0: none)")],
        |v: &Values, s: u32| {
            Output::Parts(crate::jagged_rocks::formation(v.f("height"), v.f("spires").round().max(1.0) as u32, v.color("color"), v.f("rubble"), v.f("rust"), v.f("wear"), s, v.density()))
        });

    // Realism (the Asset Studio rounded river rocks session, column 25,
    // 2026-11-07): a group of water-polished stones of different rock (a dark
    // veined greywacke, a pale granite, a tan sandstone, a dark gneiss and a
    // pale speckled stone), each in the stone recipe, with broad worn facets
    // and smooth dents in the form (crate::river_rocks).
    basic!(_r, "rocks-rounded-river-rocks", "rocks", "rounded-river-rocks", "Rounded river rocks", "stone",
        [P::num("count", 5.0, 1.0, 40.0).step(1.0).describe("Rocks"), P::num("size", 0.4, 0.1, 3.0).unit("m").describe("Typical rock length (m)"),
         P::color("color", crate::river_rocks::COLOR).describe("Rock colour (at its default each rock keeps its own)"),
         P::num("mix", 1.0, 0.0, 1.0).describe("Rock types mixed: 0 one dark stone (the colour), 1 greywacke, granite, sandstone, gneiss and a pale stone"),
         P::num("wear", 1.0, 0.0, 2.0).describe("Smooth dents worn into the stones (0: none)")],
        |v: &Values, s: u32| {
            Output::Parts(crate::river_rocks::group(v.f("count").round().max(1.0) as u32, v.f("size"), v.color("color"), v.f("mix"), v.f("wear"), s, v.density()))
        });

    // Realism (the Asset Studio rock piles session, column 26, 2026-11-08):
    // a heap of freshly broken rock (a big leaning grey schist block, slabs
    // and blocks of grey-green schist, rusty, pale quartzite, tan and
    // blue-grey stone leaning on it and on each other, stones and gravel
    // spilled round the foot), each rock dropped onto those before it so it
    // rests on them, chips and stepped sheets carved in, in the stone recipe
    // (crate::rock_piles).
    basic!(_r, "rocks-rock-piles", "rocks", "rock-piles", "Rock piles", "stone",
        [P::num("height", 1.2, 0.2, 8.0).unit("m").describe("Pile height (m)"), P::num("count", crate::rock_piles::COUNT, 4.0, 120.0).step(1.0).describe("Rocks"),
         P::color("color", crate::rock_piles::COLOR).describe("Rock colour (at its default each stone keeps its own)"),
         P::num("gravel", 1.0, 0.0, 3.0).describe("Fine gravel spilled round the foot (0: none)"),
         P::num("rust", 0.5, 0.0, 1.0).describe("How much of the grey rock is rusty iron-stained weathering"),
         P::num("wear", 1.0, 0.0, 2.0).describe("Chips broken out of the rocks' edges (0: none)")],
        |v: &Values, s: u32| {
            Output::Parts(crate::rock_piles::pile(v.f("height"), v.f("count").round().max(4.0) as u32, v.color("color"), v.f("gravel"), v.f("rust"), v.f("wear"), s, v.density()))
        });

    // Realism (the Asset Studio scree slopes session, column 27,
    // 2026-11-08): a talus slope at its angle of repose under a schist wall,
    // rock ribs between its chutes, cones of scree, stones of every size on a
    // gravel bed as real geometry, the blocks further down, mats of dwarf
    // shrub; in the stone and ground recipes (crate::scree).
    #[cfg(feature = "rocks-scree-slopes")]
    _r.add(Box::new(evani_procedural::FnAsset {
        info: evani_procedural::AssetInfo::cpu("rocks", "scree-slopes", "Scree slopes").material("stone"),
        params: &crate::scree::PARAMS,
        make: crate::scree::generate,
    }));

    basic!(_r, "rocks-rock-walls", "rocks", "rock-walls", "Rock walls", "stone",
        [P::num("length", 4.0, 0.5, 40.0).unit("m").describe("Wall length (m)"), P::num("height", 1.0, 0.2, 5.0).unit("m").describe("Wall height (m)"), P::num("stone_size", 0.35, 0.1, 1.5).unit("m").describe("Stone size (m)"), P::color("color", [0.36, 0.34, 0.3]).describe("Stone colour")],
        |v: &Values, s: u32| {
            let (len, h, k) = (v.f("length"), v.f("height"), v.f("stone_size"));
            let rows = (h / (k * 0.6)).ceil().max(1.0) as u32;
            let cols = (len / k).ceil().max(1.0) as u32;
            let mut m = geo::Mesh::default();
            for j in 0..rows {
                for i in 0..cols {
                    let id = j * cols + i;
                    let x = -len * 0.5 + (i as f32 + 0.5 + if j % 2 == 1 { 0.5 } else { 0.0 }) * k;
                    if x > len * 0.5 {
                        continue;
                    }
                    let w = k * sh::range(s, id, 1, 0.9, 1.15);
                    let r = sh::rock([w, k * 0.62, k * 0.75], s.wrapping_add(id * 23), 0.3, true);
                    geo::merge(&mut m, &sh::place(&r, 1.0, sh::range(s, id, 2, -0.15, 0.15), 0.0, [x, j as f32 * k * 0.58, sh::range(s, id, 3, -0.04, 0.04)]));
                }
            }
            Output::Mesh(m)
        });

    basic!(_r, "rocks-stone-arches", "rocks", "stone-arches", "Stone arches", "stone",
        [P::num("span", 3.0, 0.5, 30.0).unit("m").describe("Width of the opening (m)"), P::num("thickness", 1.2, 0.2, 10.0).unit("m").describe("Depth of the arch (m)"), P::color("color", [0.42, 0.28, 0.17]).describe("Rock colour")],
        |v: &Values, s: u32| {
            let r = v.f("span") * 0.5;
            Output::Mesh(sh::vault(v.f("thickness"), r, r * 1.5, 0.7, s))
        });

    basic!(_r, "rocks-crystal-clusters", "rocks", "crystal-clusters", "Crystal clusters", "glass",
        [P::num("height", 0.6, 0.05, 5.0).unit("m").describe("Tallest crystal (m)"), P::num("count", 9.0, 1.0, 40.0).step(1.0).describe("Crystals"), P::color("color", [0.45, 0.25, 0.7]).describe("Crystal colour")],
        |v: &Values, s: u32| {
            let (h, n) = (v.f("height"), v.f("count").round().max(1.0) as u32);
            let mut m = geo::Mesh::default();
            for i in 0..n {
                let t = sh::range(s, i, 1, 0.35, 1.0);
                let (len, r) = (h * t, h * 0.09 * sh::range(s, i, 2, 0.7, 1.3));
                // A six-sided prism with a pointed tip, leaning out from the middle.
                let mut c = sh::frustum(r, r, len * 0.8, 6);
                let mut tip = sh::frustum(r, 0.0, len * 0.2, 6);
                geo::transform(&mut tip, [1.0; 3], [0.0, len * 0.8, 0.0]);
                geo::merge(&mut c, &tip);
                sh::flat_shade(&mut c);
                let lean = if i == 0 { 0.0 } else { sh::range(s, i, 3, 0.2, 0.75) };
                geo::merge(&mut m, &sh::place(&c, 1.0, sh::rnd(s, i, 4) * sh::TAU, lean, [0.0; 3]));
            }
            sh::to_ground(&mut m);
            Output::Parts(vec![Part::new(m, v.color("color")).alpha(0.9).material("glass")])
        });

    basic!(_r, "rocks-mineral-deposits", "rocks", "mineral-deposits", "Mineral deposits", "stone",
        [P::num("size", 1.2, 0.2, 8.0).unit("m").describe("Rock size (m)"), P::num("nuggets", 14.0, 1.0, 60.0).step(1.0).describe("Mineral nuggets"), P::color("color", [0.26, 0.24, 0.22]).describe("Rock colour"), P::color("mineral", [0.85, 0.6, 0.15]).describe("Mineral colour")],
        |v: &Values, s: u32| {
            let k = v.f("size");
            let rock = sh::rock([k, k * 0.7, k * 0.85], s, 0.5, false);
            let mut ore = geo::Mesh::default();
            let n = v.f("nuggets").round().max(1.0) as u32;
            for i in 0..n {
                // On the rock's surface: a vertex of it, picked by the seed.
                let vi = (sh::rnd(s, i, 1) * rock.vertices.len() as f32) as usize;
                let Some(p) = rock.vertices.get(vi.min(rock.vertices.len().saturating_sub(1))) else { continue };
                let q = k * 0.07 * sh::range(s, i, 2, 0.6, 1.4);
                // Only where it shows: not under the ground.
                if p[1] < q * 1.2 {
                    continue;
                }
                let mut g = sh::icosphere(0);
                sh::flat_shade(&mut g);
                geo::merge(&mut ore, &sh::place(&g, q, sh::rnd(s, i, 3) * sh::TAU, 0.0, [p[0], p[1], p[2]]));
            }
            Output::Parts(vec![Part::new(rock, v.color("color")), Part::new(ore, v.color("mineral")).material("metal")])
        });

    basic!(_r, "rocks-ore-veins", "rocks", "ore-veins", "Ore veins", "stone",
        [P::num("size", 1.5, 0.3, 10.0).unit("m").describe("Rock size (m)"), P::num("veins", 3.0, 1.0, 10.0).step(1.0).describe("Veins"), P::color("color", [0.22, 0.21, 0.2]).describe("Rock colour"), P::color("ore", [0.65, 0.35, 0.15]).describe("Ore colour")],
        |v: &Values, s: u32| {
            let k = v.f("size");
            let rock = sh::rock([k, k * 0.75, k], s, 0.35, false);
            let mut ore = geo::Mesh::default();
            let (lo, hi) = geo::bounds(&rock);
            let c = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5];
            for i in 0..v.f("veins").round().max(1.0) as u32 {
                // A band round the rock at a seeded tilt, hugging its surface.
                let (tilt, yaw) = (sh::range(s, i, 1, -0.8, 0.8), sh::range(s, i, 2, 0.0, sh::TAU));
                let pts: Vec<[f32; 3]> = (0..=24)
                    .map(|j| {
                        let a = j as f32 / 24.0 * sh::TAU;
                        let (x, y) = (a.cos(), a.sin() * tilt.cos());
                        let z = a.sin() * tilt.sin() + 0.15 * (a * 3.0 + yaw).sin();
                        let d = [x * yaw.cos() - z * yaw.sin(), y, x * yaw.sin() + z * yaw.cos()];
                        let rr = [(hi[0] - lo[0]) * 0.47, (hi[1] - lo[1]) * 0.47, (hi[2] - lo[2]) * 0.47];
                        [c[0] + d[0] * rr[0], (c[1] + d[1] * rr[1]).max(k * 0.03), c[2] + d[2] * rr[2]]
                    })
                    .collect();
                geo::merge(&mut ore, &sh::tube(&pts, |_| k * 0.025, 5));
            }
            Output::Parts(vec![Part::new(rock, v.color("color")), Part::new(ore, v.color("ore")).material("metal")])
        });

    basic!(_r, "rocks-stalagmites", "rocks", "stalagmites", "Stalagmites", "stone",
        [P::num("height", 1.5, 0.2, 10.0).unit("m").describe("Tallest stalagmite (m)"), P::num("count", 7.0, 1.0, 30.0).step(1.0).describe("Stalagmites"), P::color("color", [0.5, 0.44, 0.36]).describe("Mineral colour")],
        |v: &Values, s: u32| {
            let (h, n) = (v.f("height"), v.f("count").round().max(1.0) as u32);
            let mut m = geo::Mesh::default();
            for (i, p) in sh::scatter_disc(n, h * 0.6, s).into_iter().enumerate() {
                let t = h * sh::range(s, i as u32, 1, 0.3, 1.0);
                let prof: Vec<[f32; 2]> = (0..=8).map(|j| {
                    let u = j as f32 / 8.0;
                    [t * 0.16 * (1.0 - u).powf(1.4) * (1.0 + 0.12 * (u * 17.0 + i as f32).sin()) + 0.003, u * t]
                }).collect();
                let c = geo::lathe(&prof, 10);
                geo::merge(&mut m, &sh::place(&c, 1.0, 0.0, 0.0, [p[0], 0.0, p[1]]));
            }
            Output::Mesh(m)
        });

    basic!(_r, "rocks-stalactites", "rocks", "stalactites", "Stalactites", "stone",
        [P::num("ceiling", 3.0, 0.5, 20.0).unit("m").describe("Ceiling height (m)"), P::num("length", 1.2, 0.1, 8.0).unit("m").describe("Longest stalactite (m)"), P::num("count", 9.0, 1.0, 40.0).step(1.0).describe("Stalactites"), P::color("color", [0.5, 0.44, 0.36]).describe("Mineral colour")],
        |v: &Values, s: u32| {
            let (top, l, n) = (v.f("ceiling"), v.f("length").min(v.f("ceiling") * 0.9), v.f("count").round().max(1.0) as u32);
            let mut m = sh::rock([l * 2.6, 0.5, l * 2.2], s, 0.4, false);
            geo::transform(&mut m, [1.0; 3], [0.0, top, 0.0]);
            for (i, p) in sh::scatter_disc(n, l * 0.9, s).into_iter().enumerate() {
                let t = l * sh::range(s, i as u32, 1, 0.3, 1.0);
                let prof: Vec<[f32; 2]> = (0..=8).map(|j| {
                    let u = j as f32 / 8.0;
                    [t * 0.12 * u.powf(1.3) + 0.003, top + 0.05 - (1.0 - u) * t]
                }).collect();
                let c = geo::lathe(&prof, 10);
                geo::merge(&mut m, &sh::place(&c, 1.0, 0.0, 0.0, [p[0], 0.0, p[1]]));
            }
            Output::Mesh(m)
        });
}
