//! Category 6, Water and coastal assets: basic generators (batch A). Oceans, lakes,
//! ponds, rivers and the storm sea are the nature pass, wrapped as it is
//! (crate::wrapped). These are CPU sheets of water (see-through, glassy)
//! over shaped ground, with foam and ice as lumps. Inspired by
//! threejs-water's and three.js (MIT) Water's idea (a flat sheet whose
//! surface is moved by summed waves); written fresh here.

use evani_procedural::Registry;

#[allow(unused_imports, dead_code)]
pub fn register(_r: &mut Registry) {
    use evani_procedural::{Output, ParamSpec as P, Part, Values, geo, noise, shapes as sh};
    const WATER: [f32; 3] = [0.05, 0.2, 0.26];
    const BANK: [f32; 3] = [0.2, 0.22, 0.1];
    const FOAM: [f32; 3] = [0.85, 0.88, 0.9];

    /// See-through water.
    fn water(m: geo::Mesh, c: [f32; 3]) -> Part {
        Part::new(m, c).alpha(0.8).material("glass")
    }

    /// A channel `len` long along x, meandering by `meander` (m), `w` wide
    /// and `d` deep in a square of ground: the banks and the water in it.
    fn channel(len: f32, w: f32, d: f32, meander: f32, s: u32) -> (geo::Mesh, geo::Mesh, impl Fn(f32) -> f32) {
        let mid = move |x: f32| meander * (noise::value2(x * 0.12, 5.0, s) - 0.5) * 2.0;
        let mut banks = geo::height_grid([len, len * 0.7], 96, |x, z| {
            let k = ((z - mid(x)) / (w * 0.5)).abs();
            d * k.clamp(0.0, 1.6).powf(1.6) / 1.6f32.powf(1.6) * 1.6 + 0.06 * noise::fbm2(x, z, s, 2, 2.0, 0.5)
        });
        sh::to_ground(&mut banks);
        let mut sheet = geo::height_grid([len, w * 1.3], 48, |_, _| d * 0.55);
        for vx in &mut sheet.vertices {
            vx[2] += mid(vx[0]);
        }
        (banks, sheet, mid)
    }

    basic!(_r, "water-streams", "water", "streams", "Streams", "glass",
        [P::num("length", 12.0, 2.0, 100.0).unit("m").describe("Stream length (m)"), P::num("width", 1.2, 0.2, 10.0).unit("m").describe("Stream width (m)"), P::color("color", WATER).describe("Water colour"), P::color("bank", BANK).describe("Bank colour")],
        |v: &Values, s: u32| {
            let (banks, sheet, _) = channel(v.f("length"), v.f("width"), 0.35, v.f("width") * 1.2, s);
            Output::Parts(vec![Part::new(banks, v.color("bank")).material("ground"), water(sheet, v.color("color"))])
        });

    basic!(_r, "water-waterfalls", "water", "waterfalls", "Waterfalls", "glass",
        [P::num("height", 4.0, 0.5, 60.0).unit("m").describe("Drop height (m)"), P::num("width", 2.0, 0.2, 30.0).unit("m").describe("Fall width (m)"), P::color("color", [0.5, 0.65, 0.72]).describe("Falling water colour"), P::color("rock", [0.28, 0.26, 0.24]).describe("Rock colour")],
        |v: &Values, s: u32| {
            let (h, w) = (v.f("height"), v.f("width"));
            // A cliff with the lip at x = 0, the pool below it.
            let mut rock = sh::rock([w * 1.2, h * 1.05, w * 2.0], s, 0.35, true);
            geo::transform(&mut rock, [1.0; 3], [-w * 0.6, 0.0, 0.0]);
            let (lo, hi) = geo::bounds(&rock);
            let lip = [hi[0] * 0.9, hi[1] * 0.95, (lo[2] + hi[2]) * 0.5];
            // The falling sheet: out from the lip, curving down (x = a fall's parabola).
            let mut fall = geo::Mesh::default();
            let mut b = geo::MeshBuilder::new();
            let (nu, nw) = (12u32, 6u32);
            for i in 0..=nu {
                let t = i as f32 / nu as f32;
                for j in 0..=nw {
                    let z = lip[2] + (j as f32 / nw as f32 - 0.5) * w;
                    let x = lip[0] + 0.35 * h.sqrt() * t.sqrt() + 0.05 * noise::value2(z * 3.0, t * 4.0, s);
                    b.vertex([x, lip[1] * (1.0 - t), z], [1.0, 0.3, 0.0]);
                }
            }
            let row = nw + 1;
            for i in 0..nu {
                for j in 0..nw {
                    let a = i * row + j;
                    b.quad(a, a + 1, a + row + 1, a + row);
                }
            }
            geo::merge(&mut fall, &b.finish());
            sh::fix_winding(&mut fall);
            sh::double_sided(&mut fall);
            let mut pool = sh::disc(w * 1.1, 0.05, 20, true);
            geo::transform(&mut pool, [1.0, 1.0, 0.7], [lip[0] + w * 0.6, 0.0, lip[2]]);
            let foam: Vec<[f32; 3]> = (0..14u32).map(|i| [lip[0] + 0.35 * h.sqrt() + sh::range(s, i, 1, -0.3, 0.5) * w * 0.5, 0.06, lip[2] + sh::range(s, i, 2, -0.5, 0.5) * w]).collect();
            let rads: Vec<f32> = (0..14u32).map(|i| w * 0.12 * sh::range(s, i, 3, 0.6, 1.2)).collect();
            let puffs = sh::puffs(&foam, &rads, s);
            Output::Parts(vec![Part::new(rock, v.color("rock")).material("stone"), Part::new(fall, v.color("color")).alpha(0.75).material("glass"), water(pool, WATER), Part::new(puffs, FOAM)])
        });

    basic!(_r, "water-rapids", "water", "rapids", "Rapids", "glass",
        [P::num("length", 10.0, 2.0, 80.0).unit("m").describe("Length of the run (m)"), P::num("roughness", 0.15, 0.0, 0.6).unit("m").describe("Height of the standing waves (m)"), P::color("color", [0.1, 0.25, 0.28]).describe("Water colour")],
        |v: &Values, s: u32| {
            let (l, rough) = (v.f("length"), v.f("roughness"));
            let (banks, _, mid) = channel(l, l * 0.35, 0.6, l * 0.05, s);
            let surf = geo::height_grid([l, l * 0.42], 64, |x, z| 0.33 + rough * (noise::fbm2(x * 0.9, z * 1.6, s, 3, 2.0, 0.5) * 0.5 + 0.5));
            let mut surf = surf;
            for vx in &mut surf.vertices {
                vx[2] += mid(vx[0]);
            }
            let (mut rocks, mut foam) = (geo::Mesh::default(), geo::Mesh::default());
            for (i, p) in sh::scatter_rect(9, [l * 0.42, l * 0.1], s).into_iter().enumerate() {
                let i = i as u32;
                let k = sh::range(s, i, 1, 0.3, 0.7);
                let z = p[1] + mid(p[0]);
                geo::merge(&mut rocks, &sh::place(&sh::rock([k, k * 0.9, k * 0.8], s.wrapping_add(i), 0.3, false), 1.0, 0.0, 0.0, [p[0], 0.05, z]));
                geo::merge(&mut foam, &sh::blob([k * 0.9, 0.06, k * 0.5], [p[0] + k * 0.6, 0.38 + rough, z], 0.8, s.wrapping_add(i * 3), 1));
            }
            Output::Parts(vec![Part::new(banks, BANK).material("ground"), Part::new(rocks, [0.3, 0.29, 0.27]).material("stone"), water(surf, v.color("color")), Part::new(foam, FOAM)])
        });

    basic!(_r, "water-water-currents", "water", "water-currents", "Water currents", "glass",
        [P::num("size", 12.0, 2.0, 100.0).unit("m").describe("Area width (m)"), P::num("swirl", 1.0, 0.0, 3.0).describe("How much the flow turns"), P::color("color", WATER).describe("Water colour"), P::color("lines", [0.5, 0.7, 0.75]).describe("Flow line colour")],
        |v: &Values, s: u32| {
            let (size, sw) = (v.f("size"), v.f("swirl"));
            let half = size * 0.5;
            let sheet = sh::water_sheet([size; 2], 0.1, 24, |_, _| 0.0);
            // Stream lines traced through a seeded swirling flow.
            let mut lines = geo::Mesh::default();
            for i in 0..14u32 {
                let mut p = [-half, 0.13, sh::range(s, i, 1, -0.9, 0.9) * half];
                let mut pts = vec![p];
                for _ in 0..30 {
                    let a = sw * (noise::value2(p[0] * 0.12, p[2] * 0.12, s) - 0.5) * 2.0;
                    p = [p[0] + a.cos() * size * 0.035, 0.13, p[2] + a.sin() * size * 0.035];
                    if p[0] > half || p[2].abs() > half {
                        break;
                    }
                    pts.push(p);
                }
                if pts.len() > 2 {
                    geo::merge(&mut lines, &sh::tube(&pts, |t| size * 0.004 * (1.0 - 0.6 * t), 4));
                }
            }
            Output::Parts(vec![water(sheet, v.color("color")), Part::new(lines, v.color("lines"))])
        });

    // Its surface flow, for Evani.commons' current-channel test scene
    // (feature `behaviour`, off in every game): fastest mid-channel and
    // slowing towards the banks, a slight meander (`swirl`), the stream
    // parting round each rock and a slow eddy turning in its lee.
    #[cfg(all(feature = "water-water-currents", feature = "behaviour"))]
    {
        fn flow(v: &Values, seed: u64, c: &evani_procedural::behaviour::Channel, p: [f32; 3], t: f32) -> [f32; 2] {
            let s = (seed as u32).wrapping_mul(0x85eb_ca6b) ^ 0x2c1b;
            let hw = c.half_width.max(0.1);
            let zr = (p[2] / hw).clamp(-1.0, 1.0);
            let prof = (1.0 - zr * zr).max(0.0);
            let umax = 1.0;
            let mut u = [umax * prof, 0.05 * v.f("swirl") * prof * (0.6 * p[0] - 0.9 * t + sh::rnd(s, 0, 1) * sh::TAU).sin()];
            for &([cx, cz], r) in &c.rocks {
                let (dx, dz) = (p[0] - cx, p[2] - cz);
                let r2 = dx * dx + dz * dz;
                if r2 < r * r {
                    return [0.0, 0.0];
                }
                // Potential flow round a cylinder in the local stream.
                let k = r * r / (r2 * r2);
                let us = u[0];
                u = [us * (1.0 - k * (dx * dx - dz * dz)), u[1] - us * 2.0 * k * dx * dz];
                // The lee eddy: a flat-topped spot behind the rock where
                // the stream stops and the water turns (period ~4.5 s).
                let a = 2.0 * r;
                let (ex, ez) = (dx - (r + 0.9 * a), dz);
                let q = (ex * ex + ez * ez) / (a * a);
                let w = (-q * q).exp();
                let om = sh::TAU / 4.5;
                u = [u[0] * (1.0 - w) - om * ez * w, u[1] * (1.0 - w) + om * ex * w];
            }
            u
        }
        _r.behaviours_mut().add_flow("water-water-currents", flow);
    }

    basic!(_r, "water-wave-foam", "water", "wave-foam", "Wave foam", "glass",
        [P::num("size", 8.0, 1.0, 60.0).unit("m").describe("Area width (m)"), P::num("bands", 4.0, 1.0, 12.0).step(1.0).describe("Bands of foam"), P::color("color", FOAM).describe("Foam colour"), P::color("water", WATER).describe("Water colour")],
        |v: &Values, s: u32| {
            let (size, n) = (v.f("size"), v.f("bands") as u32);
            let half = size * 0.5;
            let sheet = sh::water_sheet([size; 2], 0.1, 32, |x, z| 0.06 * (x * 1.3 + 0.4 * z).sin());
            let mut foam = geo::Mesh::default();
            for b in 0..n {
                let x0 = -half + (b as f32 + 0.5) * size / n as f32;
                for i in 0..24u32 {
                    let z = -half + (i as f32 + sh::rnd(s, b * 50 + i, 1)) * size / 24.0;
                    let x = x0 + 0.5 * (z * 0.7 + b as f32).sin();
                    let k = size * 0.025 * sh::range(s, b * 50 + i, 2, 0.5, 1.4);
                    geo::merge(&mut foam, &sh::blob([k * 1.4, k * 0.2, k], [x, 0.16, z], 0.9, s.wrapping_add(b * 50 + i), 1));
                }
            }
            Output::Parts(vec![water(sheet, v.color("water")), Part::new(foam, v.color("color"))])
        });

    // The rivers (Asset Studio column 20): their own file, crate::river.
    #[cfg(feature = "water-rivers")]
    _r.add(Box::new(evani_procedural::FnAsset {
        info: evani_procedural::AssetInfo::cpu("water", "rivers", "Rivers").material("ground"),
        params: &crate::river::PARAMS,
        make: crate::river::generate,
    }));

    // The shoreline (Asset Studio column 10): its own file, crate::shore.
    #[cfg(feature = "water-shorelines")]
    _r.add(Box::new(evani_procedural::FnAsset {
        info: evani_procedural::AssetInfo::cpu("water", "shorelines", "Shorelines").material("ground"),
        params: &crate::shore::PARAMS,
        make: crate::shore::generate,
    }));

    basic!(_r, "water-tidal-pools", "water", "tidal-pools", "Tidal pools", "glass",
        [P::num("size", 6.0, 1.0, 40.0).unit("m").describe("Area width (m)"), P::num("depth", 0.3, 0.05, 2.0).unit("m").describe("Pool depth (m)"), P::color("color", [0.08, 0.28, 0.3]).describe("Water colour"), P::color("rock", [0.27, 0.25, 0.23]).describe("Rock colour")],
        |v: &Values, s: u32| {
            let (size, d) = (v.f("size"), v.f("depth"));
            let mut rock = geo::height_grid([size; 2], 96, |x, z| {
                let (f1, _) = noise::cells2(x * 0.6, z * 0.6, s);
                d * 2.0 * (f1 * 1.6).min(1.0) + 0.05 * noise::fbm2(x * 2.0, z * 2.0, s, 3, 2.0, 0.5)
            });
            sh::to_ground(&mut rock);
            let sheet = sh::water_sheet([size; 2], d * 0.9, 24, |_, _| 0.0);
            Output::Parts(vec![Part::new(rock, v.color("rock")).material("stone"), water(sheet, v.color("color"))])
        });

    basic!(_r, "water-puddles", "water", "puddles", "Puddles", "glass",
        [P::num("count", 5.0, 1.0, 30.0).step(1.0).describe("Puddles"), P::num("size", 0.8, 0.1, 5.0).unit("m").describe("Typical puddle width (m)"), P::color("color", [0.12, 0.16, 0.18]).describe("Water colour"), P::color("ground", [0.2, 0.15, 0.1]).describe("Ground colour")],
        |v: &Values, s: u32| {
            let k = v.f("size");
            let n = v.f("count") as u32;
            let area = k * (n as f32).sqrt() * 1.6;
            let ground = geo::height_grid([area * 2.0; 2], 48, |x, z| 0.03 * (noise::fbm2(x, z, s, 3, 2.0, 0.5) * 0.5 + 0.5));
            let mut pools = geo::Mesh::default();
            for (i, p) in sh::scatter_disc(n, area * 0.8, s).into_iter().enumerate() {
                let mut d = sh::disc(k * 0.5, 0.035, 20, true);
                // An uneven outline.
                for vx in &mut d.vertices {
                    let a = vx[2].atan2(vx[0]);
                    let r = 1.0 + 0.3 * (noise::value2(a.cos() * 2.0 + 3.0, a.sin() * 2.0, s.wrapping_add(i as u32)) - 0.5) * 2.0;
                    vx[0] *= r;
                    vx[2] *= r * 0.75;
                }
                geo::merge(&mut pools, &sh::place(&d, 1.0, sh::rnd(s, i as u32, 2) * sh::TAU, 0.0, [p[0], 0.0, p[1]]));
            }
            Output::Parts(vec![Part::new(ground, v.color("ground")).material("ground"), Part::new(pools, v.color("color")).material("glass")])
        });

    basic!(_r, "water-marshes", "water", "marshes", "Marshes", "glass",
        [P::num("size", 12.0, 2.0, 100.0).unit("m").describe("Area width (m)"), P::num("reeds", 160.0, 10.0, 800.0).step(1.0).describe("Reeds"), P::color("color", [0.1, 0.14, 0.08]).describe("Water colour"), P::color("plants", [0.2, 0.28, 0.08]).describe("Reed colour")],
        |v: &Values, s: u32| {
            let size = v.f("size");
            let half = size * 0.5;
            let mut ground = geo::height_grid([size; 2], 64, |x, z| 0.25 * (noise::fbm2(x * 0.25, z * 0.25, s, 3, 2.0, 0.5) * 0.5 + 0.5));
            sh::to_ground(&mut ground);
            let sheet = sh::water_sheet([size; 2], 0.11, 16, |_, _| 0.0);
            let mut reeds = geo::Mesh::default();
            for (i, p) in sh::scatter_rect(v.f("reeds") as u32, [half * 0.9; 2], s).into_iter().enumerate() {
                let i = i as u32;
                let h = sh::range(s, i, 1, 0.6, 1.4);
                let b = sh::blade(0.03, h, [sh::range(s, i, 2, -0.1, 0.1), sh::range(s, i, 3, -0.1, 0.1)], 3);
                geo::merge(&mut reeds, &sh::place(&b, 1.0, sh::rnd(s, i, 4) * sh::TAU, 0.0, [p[0], 0.0, p[1]]));
            }
            Output::Parts(vec![Part::new(ground, [0.16, 0.14, 0.08]).material("ground"), water(sheet, v.color("color")), Part::new(reeds, v.color("plants")).material("grass")])
        });

    basic!(_r, "water-floating-ice", "water", "floating-ice", "Floating ice", "glass",
        [P::num("size", 10.0, 2.0, 100.0).unit("m").describe("Area width (m)"), P::num("floes", 9.0, 1.0, 40.0).step(1.0).describe("Ice floes"), P::color("color", [0.78, 0.88, 0.94]).describe("Ice colour"), P::color("water", [0.03, 0.14, 0.2]).describe("Water colour")],
        |v: &Values, s: u32| {
            let size = v.f("size");
            let sheet = sh::water_sheet([size; 2], 0.1, 16, |_, _| 0.0);
            let mut ice = geo::Mesh::default();
            for (i, p) in sh::scatter_disc(v.f("floes") as u32, size * 0.4, s).into_iter().enumerate() {
                let k = size * 0.12 * sh::range(s, i as u32, 1, 0.5, 1.3);
                let f = sh::rock([k, k * 0.12, k * 0.8], s.wrapping_add(i as u32 * 7), 0.5, true);
                geo::merge(&mut ice, &sh::place(&f, 1.0, sh::rnd(s, i as u32, 2) * sh::TAU, 0.0, [p[0], 0.06, p[1]]));
            }
            Output::Parts(vec![water(sheet, v.color("water")), Part::new(ice, v.color("color")).alpha(0.95).material("glass")])
        });
}
