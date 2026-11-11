//! Category 23, Procedural textures and materials: basic generators (batch A).
//! Glass is the nature pass, wrapped as it is (crate::wrapped). The others
//! are swatch tiles whose pattern is relief in the mesh itself (planks and
//! grain, bricks and mortar, cell stones, weave, pebbling, ripples, dents),
//! each in its look material, so the light reads the pattern. Inspired by
//! the Procedural Texture Lab's pattern ideas (bricks, cells, grain, noise
//! layers) and three.js (MIT) PlaneGeometry; written fresh here.

use evani_procedural::Registry;

#[allow(unused_imports, dead_code)]
pub fn register(_r: &mut Registry) {
    use evani_procedural::{Output, ParamSpec as P, Part, Values, geo, noise, shapes as sh};
    const SIZE: P = P::num("size", 1.0, 0.2, 8.0).unit("m").describe("Tile width (m)");
    const RES: u32 = 128;

    /// A square tile `size` wide whose top is `base` plus `relief(x, z)`
    /// (x, z in tile units 0..1), with a skirt down to the ground.
    fn tile(size: f32, base: f32, relief: impl Fn(f32, f32) -> f32) -> geo::Mesh {
        let mut top = geo::height_grid([size; 2], RES, |x, z| base + relief(x / size + 0.5, z / size + 0.5));
        // The tile's sides: a box under the relief, without its top (a top
        // at the base height covered every groove cut below it).
        let mut side = sh::block([size, base, size], [0.0, 0.0, 0.0]);
        let up = |i: u32| side.vertices.get(i as usize).is_some_and(|v| v[4] > 0.99);
        let keep: Vec<u32> = side.indices.as_chunks::<3>().0.iter().filter(|t| !t.iter().all(|&i| up(i))).flatten().copied().collect();
        side.indices = keep;
        geo::merge(&mut top, &side);
        top
    }

    basic!(_r, "textures-wood", "textures", "wood", "Wood", "ground",
        [SIZE, P::num("planks", 5.0, 1.0, 20.0).step(1.0).describe("Planks across the tile"), P::num("grain", 0.004, 0.0, 0.02).unit("m").describe("Depth of the grain (m)"), P::color("color", [0.38, 0.2, 0.08]).describe("Wood colour")],
        |v: &Values, s: u32| {
            let (n, g) = (v.f("planks").max(1.0), v.f("grain"));
            Output::Mesh(tile(v.f("size"), 0.04, |u, w| {
                let p = w * n;
                let (plank, f) = (p.floor(), p.fract());
                let gap = if !(0.03..=0.97).contains(&f) { -0.006 } else { 0.0 };
                let shift = noise::hash2(plank as i32, 0, s) * 10.0;
                // Grain: stretched noise along the plank, wavy rings.
                let rings = ((u * 3.0 + shift) * 6.0 + noise::value2(u * 4.0 + shift, f * 3.0, s) * 4.0).sin();
                gap + g * (0.5 + 0.5 * rings)
            }))
        });

    basic!(_r, "textures-metal", "textures", "metal", "Metal", "metal",
        [SIZE, P::num("panels", 2.0, 1.0, 8.0).step(1.0).describe("Panels across the tile"), P::num("rivets", 6.0, 0.0, 16.0).step(1.0).describe("Rivets along each seam"), P::color("color", [0.3, 0.31, 0.33]).describe("Metal colour")],
        |v: &Values, s: u32| {
            let (size, n, r) = (v.f("size"), v.f("panels").max(1.0), v.f("rivets"));
            let mut m = tile(size, 0.03, |u, w| {
                let (fu, fw) = ((u * n).fract(), (w * n).fract());
                let seam = fu.min(1.0 - fu).min(fw.min(1.0 - fw));
                // Each panel sits at its own slight tilt (as sheet metal
                // does), so each mirrors a different part of the sky
                // instead of the whole tile reading as one white sheet.
                let (pi, pj) = ((u * n).floor() as i32, (w * n).floor() as i32);
                let tilt = 0.02 * size / n * ((noise::hash2(pi, pj, s) - 0.5) * (fu - 0.5) + (noise::hash2(pi, pj, s ^ 0x55) - 0.5) * (fw - 0.5));
                // Brushed: fine streaks along u; seams sunk between panels.
                tilt - 0.008 * (1.0 - (seam / 0.02).min(1.0)) + 0.0004 * noise::value2(u * 2.0, w * 300.0, s)
            });
            let head = sh::icosphere(1);
            for i in 0..=n as u32 {
                for j in 0..r as u32 {
                    let a = (i as f32 / n - 0.5) * size;
                    let b = ((j as f32 + 0.5) / r.max(1.0) - 0.5) * size;
                    for p in [[a, b], [b, a]] {
                        let mut h = sh::place(&head, size * 0.008, 0.0, 0.0, [p[0], 0.03, p[1]]);
                        for vx in &mut h.vertices {
                            vx[1] = vx[1].max(0.03);
                        }
                        geo::merge(&mut m, &h);
                    }
                }
            }
            Output::Mesh(m)
        });

    basic!(_r, "textures-stone", "textures", "stone", "Stone", "stone",
        [SIZE, P::num("stones", 4.0, 1.0, 16.0).describe("Stones across the tile"), P::num("relief", 0.02, 0.0, 0.08).unit("m").describe("How much each stone stands up (m)"), P::color("color", [0.36, 0.34, 0.31]).describe("Stone colour")],
        |v: &Values, s: u32| {
            let (n, h) = (v.f("stones"), v.f("relief"));
            Output::Mesh(tile(v.f("size"), 0.03, |u, w| {
                let (f1, f2) = noise::cells2(u * n, w * n, s);
                // Mortar where two cells meet; each stone domed and rough.
                let edge = ((f2 - f1) / 0.12).min(1.0);
                h * (edge.sqrt() * (0.7 + 0.3 * (1.0 - f1)) + 0.15 * noise::fbm2(u * 30.0, w * 30.0, s, 3, 2.0, 0.5))
            }))
        });

    basic!(_r, "textures-brick", "textures", "brick", "Brick", "stone",
        [SIZE, P::num("rows", 8.0, 2.0, 30.0).step(1.0).describe("Courses of bricks"), P::num("mortar", 0.08, 0.02, 0.3).describe("Mortar joint width (share of a brick)"), P::color("color", [0.45, 0.14, 0.07]).describe("Brick colour")],
        |v: &Values, s: u32| {
            let (rows, mj) = (v.f("rows").max(1.0), v.f("mortar"));
            Output::Mesh(tile(v.f("size"), 0.03, |u, w| {
                let r = w * rows;
                let row = r.floor();
                // Running bond: every other course shifted half a brick.
                let c = u * rows * 0.5 + if row as i32 % 2 == 0 { 0.0 } else { 0.5 };
                let (fr, fc) = (r.fract(), c.fract());
                let joint = fr < mj || fc < mj * 0.5;
                if joint { -0.008 } else { 0.002 * noise::value2(u * 60.0, w * 60.0, s) }
            }))
        });

    basic!(_r, "textures-concrete", "textures", "concrete", "Concrete", "stone",
        [SIZE, P::num("pits", 0.002, 0.0, 0.01).unit("m").describe("Depth of the pits and grain (m)"), P::color("color", [0.5, 0.49, 0.46]).describe("Concrete colour")],
        |v: &Values, s: u32| {
            let p = v.f("pits");
            Output::Mesh(tile(v.f("size"), 0.04, |u, w| {
                let pit = if noise::hash2((u * 90.0) as i32, (w * 90.0) as i32, s) > 0.93 { -2.0 } else { 0.0 };
                // An expansion joint across the middle.
                let joint = if (w - 0.5).abs() < 0.006 { -0.006 } else { 0.0 };
                joint + p * (pit + noise::fbm2(u * 25.0, w * 25.0, s, 4, 2.0, 0.5))
            }))
        });

    basic!(_r, "textures-dirt", "textures", "dirt", "Dirt", "ground",
        [SIZE, P::num("roughness", 0.025, 0.0, 0.1).unit("m").describe("Height of the clods (m)"), P::color("color", [0.24, 0.16, 0.09]).describe("Soil colour")],
        |v: &Values, s: u32| {
            let r = v.f("roughness");
            let mut m = tile(v.f("size"), 0.03, |u, w| r * (0.5 + 0.5 * noise::fbm2(u * 10.0, w * 10.0, s, 5, 2.0, 0.55)));
            let size = v.f("size");
            for (i, p) in sh::scatter_rect(14, [size * 0.45; 2], s).into_iter().enumerate() {
                let k = size * 0.025 * sh::range(s, i as u32, 1, 0.5, 1.4);
                geo::merge(&mut m, &sh::place(&sh::rock([k, k * 0.6, k], s.wrapping_add(i as u32), 0.6, true), 1.0, 0.0, 0.0, [p[0], 0.03 + r * 0.3, p[1]]));
            }
            Output::Mesh(m)
        });

    basic!(_r, "textures-sand", "textures", "sand", "Sand", "ground",
        [SIZE, P::num("ripples", 10.0, 1.0, 40.0).describe("Ripples across the tile"), P::color("color", [0.64, 0.5, 0.3]).describe("Sand colour")],
        |v: &Values, s: u32| {
            let n = v.f("ripples").max(1.0);
            Output::Mesh(tile(v.f("size"), 0.03, |u, w| sh::ripples(u, w, 0.3, 1.0 / n, 0.008, s) + 0.001 * noise::value2(u * 200.0, w * 200.0, s)))
        });

    basic!(_r, "textures-grass", "textures", "grass", "Grass", "grass",
        [SIZE, P::num("density", 900.0, 50.0, 4000.0).step(1.0).describe("Blades on the tile"), P::num("height", 0.08, 0.01, 0.4).unit("m").describe("Blade height (m)"), P::color("color", [0.12, 0.27, 0.05]).describe("Grass colour")],
        |v: &Values, s: u32| {
            let size = v.f("size");
            let soil = tile(size, 0.02, |u, w| 0.003 * noise::fbm2(u * 20.0, w * 20.0, s, 3, 2.0, 0.5));
            let mut blades = geo::Mesh::default();
            for (i, p) in sh::scatter_rect(v.f("density") as u32, [size * 0.49; 2], s).into_iter().enumerate() {
                let i = i as u32;
                let h = v.f("height") * sh::range(s, i, 1, 0.6, 1.2);
                let b = sh::blade(0.006 * size.max(0.5), h, [0.0, h * sh::range(s, i, 2, 0.1, 0.5)], 2);
                geo::merge(&mut blades, &sh::place(&b, 1.0, sh::rnd(s, i, 3) * sh::TAU, 0.0, [p[0], 0.02, p[1]]));
            }
            Output::Parts(vec![Part::new(soil, [0.14, 0.09, 0.05]).material("ground"), Part::new(blades, v.color("color"))])
        });

    basic!(_r, "textures-fabric", "textures", "fabric", "Fabric", "fabric",
        [SIZE, P::num("threads", 24.0, 4.0, 80.0).step(1.0).describe("Threads across the tile"), P::color("color", [0.12, 0.2, 0.45]).describe("Cloth colour")],
        |v: &Values, _s: u32| {
            let n = v.f("threads").max(1.0);
            Output::Mesh(tile(v.f("size"), 0.01, |u, w| {
                // Plain weave: warp over weft, swapping every thread.
                let (a, b) = ((u * n * sh::PI).sin(), (w * n * sh::PI).sin());
                let over = if ((u * n).floor() + (w * n).floor()) as i32 % 2 == 0 { a.abs() } else { b.abs() };
                0.002 * over
            }))
        });

    basic!(_r, "textures-leather", "textures", "leather", "Leather", "leather",
        [SIZE, P::num("grain", 30.0, 5.0, 100.0).describe("Pebbled bumps across the tile"), P::color("color", [0.3, 0.14, 0.06]).describe("Leather colour")],
        |v: &Values, s: u32| {
            let n = v.f("grain");
            Output::Mesh(tile(v.f("size"), 0.01, |u, w| {
                let (f1, _) = noise::cells2(u * n, w * n, s);
                0.0015 * (1.0 - f1).powi(2) + 0.0015 * noise::fbm2(u * 4.0, w * 4.0, s, 3, 2.0, 0.5)
            }))
        });

    basic!(_r, "textures-plastic", "textures", "plastic", "Plastic", "glass",
        [SIZE, P::num("ribs", 6.0, 0.0, 20.0).step(1.0).describe("Moulded ribs across the tile"), P::color("color", [0.75, 0.12, 0.08]).describe("Plastic colour")],
        |v: &Values, _s: u32| {
            let n = v.f("ribs");
            Output::Mesh(tile(v.f("size"), 0.02, |u, w| {
                // A smooth panel, rounded at its edge, with moulded ribs.
                let edge = u.min(1.0 - u).min(w.min(1.0 - w));
                let round = -0.006 * (1.0 - (edge / 0.04).min(1.0)).powi(2);
                let rib = if n > 0.0 && (0.2..0.8).contains(&w) { 0.004 * (((u * n).fract() - 0.5).abs() < 0.08) as i32 as f32 } else { 0.0 };
                round + rib
            }))
        });

    basic!(_r, "textures-ice", "textures", "ice", "Ice", "glass",
        [SIZE, P::num("cracks", 5.0, 1.0, 20.0).describe("Cracked plates across the tile"), P::color("color", [0.72, 0.86, 0.94]).describe("Ice colour")],
        |v: &Values, s: u32| {
            let n = v.f("cracks");
            let mut m = tile(v.f("size"), 0.05, |u, w| {
                let (f1, f2) = noise::cells2(u * n, w * n, s);
                let crack = if f2 - f1 < 0.04 { -0.004 } else { 0.0 };
                crack + 0.004 * noise::hash2((u * n) as i32, (w * n) as i32, s)
            });
            sh::flat_shade(&mut m);
            Output::Parts(vec![Part::new(m, v.color("color")).alpha(0.85).material("glass")])
        });

    basic!(_r, "textures-snow", "textures", "snow", "Snow", "ground",
        [SIZE, P::num("depth", 0.06, 0.01, 0.3).unit("m").describe("Snow depth (m)"), P::num("drifts", 0.02, 0.0, 0.1).unit("m").describe("Height of the soft drifts (m)"), P::color("color", [0.92, 0.93, 0.96]).describe("Snow colour")],
        |v: &Values, s: u32| {
            let (d, dr) = (v.f("depth"), v.f("drifts"));
            Output::Mesh(tile(v.f("size"), d, |u, w| dr * (0.5 + 0.5 * noise::fbm2(u * 3.0, w * 3.0, s, 3, 2.0, 0.45)) + 0.0008 * noise::value2(u * 150.0, w * 150.0, s)))
        });

    basic!(_r, "textures-surface-damage-wear-and-noise", "textures", "surface-damage-wear-and-noise", "Surface damage, wear and noise", "stone",
        [SIZE, P::num("dents", 8.0, 0.0, 40.0).step(1.0).describe("Dents"), P::num("scratches", 12.0, 0.0, 60.0).step(1.0).describe("Scratches"), P::color("color", [0.45, 0.44, 0.42]).describe("Surface colour")],
        |v: &Values, s: u32| {
            let (nd, ns) = (v.f("dents") as u32, v.f("scratches") as u32);
            let dents: Vec<[f32; 3]> = (0..nd).map(|i| [sh::rnd(s, i, 1), sh::rnd(s, i, 2), sh::range(s, i, 3, 0.02, 0.07)]).collect();
            let scr: Vec<[f32; 4]> = (0..ns).map(|i| [sh::rnd(s, i, 4), sh::rnd(s, i, 5), sh::range(s, i, 6, 0.0, sh::PI), sh::range(s, i, 7, 0.1, 0.4)]).collect();
            Output::Mesh(tile(v.f("size"), 0.03, |u, w| {
                let mut h = 0.0008 * noise::fbm2(u * 40.0, w * 40.0, s, 3, 2.0, 0.5);
                for d in &dents {
                    let r2 = ((u - d[0]).powi(2) + (w - d[1]).powi(2)) / (d[2] * d[2]);
                    h -= 0.006 * (-r2 * 2.0).exp();
                }
                for c in &scr {
                    let (dx, dz) = (c[2].cos(), c[2].sin());
                    let (pu, pw) = (u - c[0], w - c[1]);
                    let along = pu * dx + pw * dz;
                    let across = (-pu * dz + pw * dx).abs();
                    if along.abs() < c[3] * 0.5 && across < 0.003 {
                        h -= 0.002;
                    }
                }
                h
            }))
        });
}
