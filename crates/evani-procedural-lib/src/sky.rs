//! Category 7, Sky, weather and atmosphere: basic generators (batch A). The sky
//! dome is evani-material-proc's sky, wrapped as it is (crate::wrapped).
//! These are CPU meshes over a patch of ground: puffs for clouds and mist,
//! streaks for rain and wind, flakes and balls for snow and hail, a jagged
//! tube for lightning, bands for rainbows and aurora. Faint layers use the
//! renderer's screen-door fade. Inspired by threejs-weathering's and
//! pmndrs/sky's ideas (particles in a volume, discs on the sky) and three.js
//! (MIT) TubeGeometry; written fresh here.

use evani_procedural::Registry;

#[allow(unused_imports, dead_code)]
pub fn register(_r: &mut Registry) {
    use evani_procedural::{Output, ParamSpec as P, Part, Values, geo, noise, shapes as sh};
    const AREA: P = P::num("area", 8.0, 1.0, 60.0).unit("m").describe("Width of the area it fills (m)");

    /// `n` seeded points in a box `half` (x, z) wide from y0 to y1.
    fn volume(n: u32, half: f32, y0: f32, y1: f32, s: u32) -> Vec<[f32; 3]> {
        sh::scatter_rect(n, [half; 2], s).into_iter().enumerate().map(|(i, p)| [p[0], sh::range(s, i as u32, 11, y0, y1), p[1]]).collect()
    }

    basic!(_r, "sky-sun-and-moon-discs", "sky", "sun-and-moon-discs", "Sun and moon discs", "glow",
        [P::num("size", 2.0, 0.2, 20.0).unit("m").describe("Sun disc radius (m)"), P::color("color", [1.0, 0.82, 0.45]).describe("Sun colour"), P::color("moon", [0.8, 0.8, 0.76]).describe("Moon colour")],
        |v: &Values, s: u32| {
            let r = v.f("size");
            // Upright discs facing +z (towards the viewer), sun and moon side by side.
            let up = |rad: f32, at: [f32; 3]| {
                let mut d = sh::disc(rad, 0.0, 32, true);
                sh::rotate(&mut d, [1.0, 0.0, 0.0], sh::PI * 0.5);
                sh::place(&d, 1.0, 0.0, 0.0, at)
            };
            let sun = up(r, [-r * 1.4, r * 2.5, 0.0]);
            let moon = up(r * 0.7, [r * 1.6, r * 2.5, 0.0]);
            let mut craters = geo::Mesh::default();
            for i in 0..6u32 {
                let (a, d) = (sh::range(s, i, 1, 0.0, sh::TAU), sh::range(s, i, 2, 0.1, 0.5) * r);
                geo::merge(&mut craters, &up(r * sh::range(s, i, 3, 0.06, 0.16), [r * 1.6 + a.cos() * d, r * 2.5 + a.sin() * d, 0.01]));
            }
            Output::Parts(vec![Part::new(sun, v.color("color")).material("glow"), Part::new(moon, v.color("moon")).material("ground"), Part::new(craters, [0.45, 0.45, 0.43]).material("ground")])
        });

    basic!(_r, "sky-cloud-layers", "sky", "cloud-layers", "Cloud layers", "ground",
        [P::num("area", 30.0, 4.0, 300.0).unit("m").describe("Width of the layer (m)"), P::num("height", 8.0, 1.0, 100.0).unit("m").describe("Height of the layer (m)"), P::num("cover", 0.6, 0.1, 1.0).describe("How much of the sky it covers"), P::color("color", [0.85, 0.86, 0.88]).describe("Cloud colour")],
        |v: &Values, s: u32| {
            let (a, h, c) = (v.f("area"), v.f("height"), v.f("cover"));
            let n = (60.0 * c) as u32 + 4;
            let pts: Vec<[f32; 3]> = sh::scatter_rect(n, [a * 0.5; 2], s).into_iter().enumerate().map(|(i, p)| [p[0], h + sh::range(s, i as u32, 1, -0.3, 0.3), p[1]]).collect();
            let r: Vec<f32> = (0..n).map(|i| a * 0.06 * sh::range(s, i, 2, 0.6, 1.3)).collect();
            let mut m = sh::puffs(&pts, &r, s);
            // Flat bottoms and thin layers.
            for vx in &mut m.vertices {
                vx[1] = h + (vx[1] - h).max(-a * 0.01) * 0.35;
            }
            geo::smooth_normals(&mut m);
            Output::Parts(vec![Part::new(m, v.color("color")).alpha(0.9)])
        });

    basic!(_r, "sky-volumetric-looking-clouds", "sky", "volumetric-looking-clouds", "Volumetric-looking clouds", "ground",
        [P::num("size", 8.0, 1.0, 100.0).unit("m").describe("Cloud width (m)"), P::num("height", 6.0, 0.0, 100.0).unit("m").describe("Height of its base (m)"), P::num("tower", 0.6, 0.15, 1.2).describe("How high its tallest heap rises over the base (times its width)"), P::color("color", [0.95, 0.947, 0.942]).describe("Cloud colour")],
        |v: &Values, s: u32| {
            // A cumulus (crate::clouds): heaps, the tallest to one side, on a
            // flat base. Games draw it as a ray-marched volume (the procedural
            // module hosts crate::clouds::CloudVolume); this mesh is its big
            // lobes, for bounds and for a host without the volume.
            let (w, h) = (v.f("size"), v.f("height"));
            let lobes = crate::clouds::cumulus(w, h, v.f("tower"), s).lobes;
            let (pts, r): (Vec<[f32; 3]>, Vec<f32>) = lobes.iter().filter(|l| l[3] >= 0.04 * w).map(|l| ([l[0], l[1], l[2]], l[3])).unzip();
            let mut m = sh::puffs(&pts, &r, s);
            for vx in &mut m.vertices {
                vx[1] = vx[1].max(h);
            }
            geo::smooth_normals(&mut m);
            Output::Mesh(m)
        });

    basic!(_r, "sky-distance-fog", "sky", "distance-fog", "Distance fog", "ground",
        [P::num("visibility", crate::fog::VISIBILITY, 5.0, 2000.0).unit("m").describe("How far you can see along the ground before things are nearly lost in the fog (m)"), P::num("start", crate::fog::START, 0.0, 200.0).unit("m").describe("The clear air round the viewer before the fog begins (m)"), P::num("height", crate::fog::HEIGHT, 0.5, 200.0).unit("m").describe("How deep the thicker ground layer is: the fog thins by e every this many metres up (m)"), P::color("color", crate::fog::COLOR).describe("Fog colour (high up; it is a little darker along the horizon)")],
        |v: &Values, _s: u32| {
            // The fog is a depth-based height fog (crate::fog): the procedural
            // module hosts crate::fog::DistanceFog, a full-screen pass that
            // fades everything by its distance and height. This mesh is only
            // the fog bank's bounds, for a host without the pass.
            Output::Mesh(crate::fog::bank(v.f("visibility"), v.f("start"), v.f("height")))
        });

    basic!(_r, "sky-ground-mist", "sky", "ground-mist", "Ground mist", "ground",
        [AREA, P::num("puffs", 40.0, 4.0, 200.0).step(1.0).describe("Wisps"), P::color("color", [0.82, 0.84, 0.86]).describe("Mist colour")],
        |v: &Values, s: u32| {
            let a = v.f("area");
            let n = v.f("puffs") as u32;
            let pts: Vec<[f32; 3]> = sh::scatter_rect(n, [a * 0.5; 2], s).into_iter().map(|p| [p[0], 0.15, p[1]]).collect();
            let r: Vec<f32> = (0..n).map(|i| a * 0.07 * sh::range(s, i, 1, 0.6, 1.3)).collect();
            let mut m = sh::puffs(&pts, &r, s);
            for vx in &mut m.vertices {
                vx[1] = (vx[1] * 0.25).max(0.0);
            }
            geo::smooth_normals(&mut m);
            Output::Parts(vec![Part::new(m, v.color("color")).alpha(0.5)])
        });

    basic!(_r, "sky-rain", "sky", "rain", "Rain", "ground",
        [AREA, P::num("drops", 400.0, 20.0, 3000.0).step(1.0).describe("Raindrops"), P::num("slant", 0.15, 0.0, 1.0).describe("How far the wind tilts them"), P::color("color", [0.22, 0.3, 0.4]).describe("Rain colour")],
        |v: &Values, s: u32| {
            let a = v.f("area");
            let pts = volume(v.f("drops") as u32, a * 0.5, 0.2, a * 0.6, s);
            let m = sh::streaks(&pts, [v.f("slant"), -1.0, 0.0], 0.4, 0.02);
            Output::Parts(vec![Part::new(m, v.color("color")).alpha(0.85)])
        });

    basic!(_r, "sky-snowfall", "sky", "snowfall", "Snowfall", "ground",
        [AREA, P::num("flakes", 500.0, 20.0, 3000.0).step(1.0).describe("Snowflakes"), P::color("color", [0.95, 0.96, 0.98]).describe("Snow colour")],
        |v: &Values, s: u32| {
            let a = v.f("area");
            let mut flake = sh::icosphere(0);
            sh::flat_shade(&mut flake);
            let mut m = geo::Mesh::default();
            for (i, p) in volume(v.f("flakes") as u32, a * 0.5, 0.05, a * 0.6, s).into_iter().enumerate() {
                geo::merge(&mut m, &sh::place(&flake, 0.05 * sh::range(s, i as u32, 2, 0.6, 1.4), 0.0, 0.0, p));
            }
            Output::Mesh(m)
        });

    basic!(_r, "sky-hail", "sky", "hail", "Hail", "ground",
        [AREA, P::num("stones", 250.0, 10.0, 2000.0).step(1.0).describe("Hailstones"), P::num("size", 0.06, 0.005, 0.15).unit("m").describe("Hailstone size (m)"), P::color("color", [0.55, 0.66, 0.78]).describe("Ice colour")],
        |v: &Values, s: u32| {
            let a = v.f("area");
            let ball = sh::icosphere(1);
            let mut m = geo::Mesh::default();
            let n = v.f("stones") as u32;
            for (i, p) in volume(n, a * 0.5, 0.0, a * 0.5, s).into_iter().enumerate() {
                // A third already lie on the ground.
                let y = if i % 3 == 0 { v.f("size") * 0.5 } else { p[1] };
                geo::merge(&mut m, &sh::place(&ball, v.f("size") * 0.5 * sh::range(s, i as u32, 2, 0.7, 1.3), 0.0, 0.0, [p[0], y, p[2]]));
            }
            Output::Parts(vec![Part::new(m, v.color("color")).material("glass")])
        });

    basic!(_r, "sky-wind-gust-effects", "sky", "wind-gust-effects", "Wind gust effects", "ground",
        [AREA, P::num("gusts", 10.0, 1.0, 50.0).step(1.0).describe("Gust lines"), P::color("color", [0.3, 0.42, 0.55]).describe("Line colour")],
        |v: &Values, s: u32| {
            let a = v.f("area");
            let mut m = geo::Mesh::default();
            for i in 0..v.f("gusts") as u32 {
                // A swoosh: a line along +x that curls up at its end.
                let (z, y) = (sh::range(s, i, 1, -0.5, 0.5) * a, sh::range(s, i, 2, 0.4, 2.5));
                let x0 = sh::range(s, i, 3, -0.5, 0.1) * a;
                let len = a * sh::range(s, i, 4, 0.25, 0.5);
                let curl = len * 0.12;
                let pts: Vec<[f32; 3]> = (0..=24).map(|j| {
                    let t = j as f32 / 24.0;
                    let k = ((t - 0.7) / 0.3).max(0.0) * sh::PI * 1.5;
                    [x0 + t * len + if k > 0.0 { curl * k.sin() - curl * k * 0.6 } else { 0.0 }, y + if k > 0.0 { curl * (1.0 - k.cos()) } else { 0.0 } + 0.1 * (t * 9.0).sin(), z]
                }).collect();
                geo::merge(&mut m, &sh::tube(&pts, |t| 0.04 * (1.0 - t) * (t * 6.0).min(1.0) + 0.004, 4));
            }
            Output::Parts(vec![Part::new(m, v.color("color")).alpha(0.7)])
        });

    // Its wind, for Evani.commons' wind-field test scene (feature
    // `behaviour`, off in every game): a light breeze along +x, wavering a
    // little in strength and direction, with gust fronts at uneven intervals
    // that start upwind of the field and sweep along it.
    #[cfg(all(feature = "sky-wind-gust-effects", feature = "behaviour"))]
    {
        fn wind(_v: &Values, seed: u64, p: [f32; 3], t: f32) -> [f32; 3] {
            let s = (seed as u32).wrapping_mul(0x9e37_79b9) ^ 0x51ed;
            let th = 0.12 * (0.05 * t + sh::rnd(s, 0, 9) * sh::TAU).sin();
            let d = [th.cos(), th.sin()];
            let x = p[0] * d[0] + p[2] * d[1];
            // Turbulence carried along with the wind (coherent over metres).
            let mut turb = 0.0;
            for (i, (f, k)) in [(0.45f32, 0.21f32), (0.8, 0.33), (1.3, 0.5)].into_iter().enumerate() {
                turb += (sh::TAU * f * t - k * x + sh::rnd(s, i as u32, 7) * sh::TAU).sin() / 3.0;
            }
            // Gust fronts: one starts every 2.5 to 11.5 s (mean 6 s, most
            // of them short, a few long lulls), 15 m upwind, moving at 6 m/s,
            // each a 3 s swell.
            let mut g = 0.0;
            let mut tk = 1.0 + 4.0 * sh::rnd(s, 0, 1);
            let mut k = 0u32;
            while tk < t && k < 100_000 {
                let tau = t - tk - (x + 15.0) / 6.0;
                if tau > 0.0 && tau < 3.0 {
                    g += (sh::PI * tau / 3.0).sin().powi(2) * sh::range(s, k, 3, 0.8, 1.2);
                }
                tk += 2.5 + 9.0 * sh::rnd(s, k, 2).powf(1.5);
                k += 1;
            }
            let speed = 3.0 * (1.0 + 0.25 * turb) + 3.6 * g;
            [d[0] * speed, 0.0, d[1] * speed]
        }
        _r.behaviours_mut().add_wind("sky-wind-gust-effects", wind);
    }

    basic!(_r, "sky-dust-storms", "sky", "dust-storms", "Dust storms", "ground",
        [P::num("area", 30.0, 4.0, 300.0).unit("m").describe("Width of the storm front (m)"), P::num("height", 8.0, 1.0, 100.0).unit("m").describe("Height of the wall of dust (m)"), P::color("color", [0.5, 0.36, 0.22]).describe("Dust colour")],
        |v: &Values, s: u32| {
            let (a, h) = (v.f("area"), v.f("height"));
            let mut pts = Vec::new();
            let mut r = Vec::new();
            for i in 0..60u32 {
                let x = sh::range(s, i, 1, -0.5, 0.5) * a;
                let y = h * sh::rnd(s, i, 2).powf(1.5);
                pts.push([x, y, sh::range(s, i, 3, -0.08, 0.08) * a - y * 0.25]);
                r.push(h * 0.22 * sh::range(s, i, 4, 0.7, 1.3) * (1.0 - 0.5 * y / h));
            }
            Output::Parts(vec![Part::new(sh::puffs(&pts, &r, s), v.color("color")).alpha(0.85)])
        });

    basic!(_r, "sky-sandstorms", "sky", "sandstorms", "Sandstorms", "ground",
        [AREA, P::num("grains", 500.0, 20.0, 3000.0).step(1.0).describe("Blown sand streaks"), P::color("color", [0.7, 0.55, 0.32]).describe("Sand colour")],
        |v: &Values, s: u32| {
            let a = v.f("area");
            let streaks = sh::streaks(&volume(v.f("grains") as u32, a * 0.5, 0.05, 2.0, s), [1.0, 0.08, 0.15], 0.4, 0.01);
            let low: Vec<[f32; 3]> = sh::scatter_rect(18, [a * 0.5; 2], s ^ 9).into_iter().map(|p| [p[0], 0.2, p[1]]).collect();
            let r: Vec<f32> = (0..18u32).map(|i| sh::range(s, i, 5, 0.5, 1.1)).collect();
            let mut dust = sh::puffs(&low, &r, s);
            for vx in &mut dust.vertices {
                vx[1] = (vx[1] * 0.4).max(0.0);
            }
            geo::smooth_normals(&mut dust);
            Output::Parts(vec![Part::new(streaks, v.color("color")).alpha(0.75), Part::new(dust, v.color("color")).alpha(0.55)])
        });

    basic!(_r, "sky-lightning", "sky", "lightning", "Lightning", "glow",
        [P::num("height", 12.0, 2.0, 100.0).unit("m").describe("Height of the cloud it falls from (m)"), P::num("forks", 3.0, 0.0, 10.0).step(1.0).describe("Forks off the main bolt"), P::color("color", [0.75, 0.8, 1.0]).describe("Bolt colour")],
        |v: &Values, s: u32| {
            let h = v.f("height");
            // A jagged path down, kinked at every step.
            let jag = |from: [f32; 3], steps: u32, drop: f32, seed: u32| -> Vec<[f32; 3]> {
                let mut p = from;
                let mut out = vec![p];
                for i in 0..steps {
                    p = [p[0] + sh::range(seed, i, 1, -0.5, 0.5) * drop * 0.6, p[1] - drop, p[2] + sh::range(seed, i, 2, -0.5, 0.5) * drop * 0.6];
                    out.push([p[0], p[1].max(0.0), p[2]]);
                }
                out
            };
            let main = jag([0.0, h, 0.0], 14, h / 14.0, s);
            let mut bolt = sh::tube(&main, |t| 0.06 * (1.0 - 0.5 * t), 4);
            for f in 0..v.f("forks") as u32 {
                let at = main[(2 + (sh::rnd(s, f, 3) * 8.0) as usize).min(main.len() - 1)];
                let fork = jag(at, 5, h / 18.0, s.wrapping_add(f * 77));
                geo::merge(&mut bolt, &sh::tube(&fork, |t| 0.03 * (1.0 - t), 4));
            }
            let pts: Vec<[f32; 3]> = (0..12u32).map(|i| [sh::range(s, i, 4, -1.0, 1.0) * h * 0.3, h + sh::range(s, i, 5, 0.0, 0.15) * h, sh::range(s, i, 6, -1.0, 1.0) * h * 0.15]).collect();
            let r: Vec<f32> = (0..12u32).map(|i| h * 0.1 * sh::range(s, i, 7, 0.7, 1.3)).collect();
            Output::Parts(vec![Part::new(bolt, v.color("color")).material("glow"), Part::new(sh::puffs(&pts, &r, s), [0.22, 0.23, 0.26]).material("ground")])
        });

    basic!(_r, "sky-rainbows", "sky", "rainbows", "Rainbows", "ground",
        [P::num("radius", 10.0, 1.0, 200.0).unit("m").describe("Radius of the arc (m)"), P::num("width", 1.0, 0.1, 20.0).unit("m").describe("Width of the band (m)")],
        |v: &Values, _s: u32| {
            let (r, w) = (v.f("radius"), v.f("width"));
            const BANDS: [[f32; 3]; 6] = [[0.8, 0.05, 0.04], [0.9, 0.35, 0.02], [0.9, 0.8, 0.05], [0.1, 0.6, 0.1], [0.05, 0.2, 0.8], [0.35, 0.08, 0.6]];
            let mut parts = Vec::new();
            for (k, c) in BANDS.iter().enumerate() {
                let rr = r - k as f32 * w / 6.0;
                // A flat ribbon of the arc, facing +z.
                let mut b = geo::MeshBuilder::new();
                for i in 0..=48u32 {
                    let a = i as f32 / 48.0 * sh::PI;
                    for q in [rr, rr - w / 6.0] {
                        b.vertex([a.cos() * q, a.sin() * q, 0.0], [0.0, 0.0, 1.0]);
                    }
                }
                for i in 0..48u32 {
                    let a = i * 2;
                    b.quad(a, a + 1, a + 3, a + 2);
                }
                let mut m = b.finish();
                sh::fix_winding(&mut m);
                sh::double_sided(&mut m);
                parts.push(Part::new(m, *c).alpha(0.6));
            }
            Output::Parts(parts)
        });

    basic!(_r, "sky-aurora-effects", "sky", "aurora-effects", "Aurora effects", "glow",
        [P::num("width", 30.0, 4.0, 300.0).unit("m").describe("Width of the curtain (m)"), P::num("height", 10.0, 1.0, 100.0).unit("m").describe("Height of the curtain's base (m)"), P::color("color", [0.1, 0.85, 0.4]).describe("Lower colour"), P::color("upper", [0.55, 0.15, 0.6]).describe("Upper fringe colour")],
        |v: &Values, s: u32| {
            let (w, h) = (v.f("width"), v.f("height"));
            // A curtain waving along x: two bands, the fringe above.
            let band = |y0: f32, y1: f32| {
                let mut b = geo::MeshBuilder::new();
                for i in 0..=60u32 {
                    let t = i as f32 / 60.0;
                    let x = (t - 0.5) * w;
                    let z = w * 0.08 * (t * 7.0 + noise::value2(t * 4.0, 1.0, s) * 3.0).sin();
                    let fold = 0.3 * h * noise::value2(t * 6.0, 2.0, s);
                    b.vertex([x, h + y0 + fold * 0.2, z], [0.0, 0.0, 1.0]);
                    b.vertex([x, h + y1 + fold, z + w * 0.01], [0.0, 0.0, 1.0]);
                }
                for i in 0..60u32 {
                    let a = i * 2;
                    b.quad(a, a + 2, a + 3, a + 1);
                }
                let mut m = b.finish();
                sh::fix_winding(&mut m);
                sh::double_sided(&mut m);
                m
            };
            Output::Parts(vec![Part::new(band(0.0, h * 0.35), v.color("color")).alpha(0.6).material("glow"), Part::new(band(h * 0.35, h * 0.55), v.color("upper")).alpha(0.4).material("glow")])
        });
}
