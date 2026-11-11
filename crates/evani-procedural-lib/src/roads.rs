//! Category 8, Roads, paths and transport infrastructure: basic generators
//! (batch B). A cross-section swept along a seeded S-curve (dirt, asphalt,
//! ballast, kerbs, rails), units laid along the path in its frame
//! (cobbles, bricks, slabs, planks, sleepers), plans extruded flat
//! (crossings), revolved rings (roundabouts) and an arch swept over a span.
//! Inspired by three.js (MIT) ExtrudeGeometry and TubeGeometry (a shape
//! carried along a path in its frames) and LatheGeometry; written fresh
//! here.

use evani_procedural::Registry;

#[allow(unused_imports, dead_code)]
pub fn register(_r: &mut Registry) {
    use evani_procedural::{Output, ParamSpec as P, Part, Values, build as b, geo, shapes as sh};
    const ASPHALT: [f32; 3] = [0.06, 0.06, 0.065];
    const PAINT: [f32; 3] = [0.8, 0.8, 0.76];
    const KERB: [f32; 3] = [0.5, 0.49, 0.46];
    const WIDTH: P = P::num("width", 4.0, 1.0, 14.0).unit("m").describe("Road width (m)");
    const LENGTH: P = P::num("length", 12.0, 3.0, 60.0).unit("m").describe("Length (m)");
    const CURVE: P = P::num("curve", 2.0, 0.0, 10.0).unit("m").describe("How far the road bends (m)");

    /// The road's centre line: an S-curve along x.
    fn line(v: &Values) -> Vec<[f32; 3]> {
        b::s_path(v.f("length"), v.get("curve").map_or(0.0, |_| v.f("curve")), 0.0, 40)
    }

    /// A strip `w` wide across, from `x0` (right of the line) to `x0 + w`,
    /// from height `y0` to `y1`, along `path`.
    fn strip(path: &[[f32; 3]], x0: f32, w: f32, y0: f32, y1: f32) -> geo::Mesh {
        b::sweep(path, &[[x0, y0], [x0 + w, y0], [x0 + w, y1], [x0, y1]], true)
    }

    /// Units `size` (along, up, across) laid in rows along `path` over a
    /// `width`, every other row offset by half; `gap` between them; corners
    /// rounded by `round`; the seed nudges each.
    fn units(path: &[[f32; 3]], width: f32, size: [f32; 3], gap: f32, round: f32, seed: u32) -> geo::Mesh {
        let total = b::path_len(path);
        let rows = (total / (size[0] + gap)).floor().max(1.0) as u32;
        let cols = (width / (size[2] + gap)).floor().max(1.0) as u32;
        let unit = if round > 0.0 { b::rounded_box(size, round, 2, [0.0; 3]) } else { sh::block(size, [0.0; 3]) };
        let mut m = geo::Mesh::default();
        for i in 0..rows {
            let (p, yaw, right) = b::along(path, (i as f32 + 0.5) / rows as f32);
            let shift = if i % 2 == 1 { 0.5 } else { 0.0 };
            for j in 0..cols {
                let x = ((j as f32 + 0.5 + shift) / cols as f32 - 0.5) * width;
                if x.abs() + size[2] * 0.5 > width * 0.5 {
                    continue;
                }
                let k = i * 977 + j;
                let jit = sh::range(seed, k, 1, -0.06, 0.06);
                let at = [p[0] + right[0] * x, p[1] + sh::range(seed, k, 2, 0.0, size[1] * 0.25), p[2] + right[2] * x];
                geo::merge(&mut m, &sh::place(&unit, 1.0, yaw + jit, 0.0, at));
            }
        }
        m
    }

    /// Dashes `len` long every `every` m at `x` across, `y` up.
    fn dashes(path: &[[f32; 3]], x: f32, y: f32, len: f32, every: f32, w: f32) -> geo::Mesh {
        let total = b::path_len(path);
        let n = (total / every).floor().max(1.0) as u32;
        let mut m = geo::Mesh::default();
        for i in 0..n {
            let (p, yaw, right) = b::along(path, (i as f32 + 0.5) / n as f32);
            let at = [p[0] + right[0] * x, y, p[2] + right[2] * x];
            geo::merge(&mut m, &sh::place(&sh::block([len, 0.012, w], [0.0; 3]), 1.0, yaw, 0.0, at));
        }
        m
    }

    basic!(_r, "roads-dirt-roads", "roads", "dirt-roads", "Dirt roads", "ground",
        [P::num("width", 3.5, 1.0, 12.0).unit("m").describe("Road width (m)"), LENGTH, CURVE, P::color("color", [0.17, 0.11, 0.06]).describe("Earth colour")],
        |v: &Values, _s: u32| {
            let w = v.f("width") * 0.5;
            // A crowned track with two wheel ruts.
            let prof = [[-w - 0.3, 0.0], [w + 0.3, 0.0], [w, 0.06], [w * 0.6, 0.09], [w * 0.45, 0.04], [w * 0.3, 0.1], [-w * 0.3, 0.1], [-w * 0.45, 0.04], [-w * 0.6, 0.09], [-w, 0.06]];
            Output::Mesh(b::sweep(&line(v), &prof, true))
        });

    basic!(_r, "roads-gravel-roads", "roads", "gravel-roads", "Gravel roads", "stone",
        [P::num("width", 3.5, 1.0, 12.0).unit("m").describe("Road width (m)"), LENGTH, P::num("stones", 500.0, 20.0, 3000.0).step(10.0).describe("Loose stones on top"), P::color("color", [0.55, 0.52, 0.47]).describe("Gravel colour")],
        |v: &Values, s: u32| {
            let (w, path) = (v.f("width"), b::s_path(v.f("length"), 1.5, 0.0, 40));
            let bed = b::sweep(&path, &[[-w * 0.5 - 0.2, 0.0], [w * 0.5 + 0.2, 0.0], [w * 0.5, 0.07], [-w * 0.5, 0.07]], true);
            let mut stones = geo::Mesh::default();
            let n = v.f("stones").round().max(1.0) as u32;
            for i in 0..n {
                let (p, yaw, right) = b::along(&path, sh::rnd(s, i, 1));
                let x = sh::range(s, i, 2, -0.5, 0.5) * w;
                let k = sh::range(s, i, 3, 0.03, 0.08);
                let r = sh::rock([k, k * 0.6, k * 0.8], s.wrapping_add(i % 7), 0.5, true);
                geo::merge(&mut stones, &sh::place(&r, 1.0, yaw + i as f32, 0.0, [p[0] + right[0] * x, 0.05, p[2] + right[2] * x]));
            }
            Output::Parts(vec![Part::new(bed, [0.16, 0.14, 0.12]), Part::new(stones, v.color("color"))])
        });

    basic!(_r, "roads-cobblestone-roads", "roads", "cobblestone-roads", "Cobblestone roads", "stone",
        [WIDTH, P::num("length", 8.0, 2.0, 40.0).unit("m").describe("Length (m)"), P::num("cobble", 0.2, 0.08, 0.5).unit("m").describe("Cobble size (m)"), P::color("color", [0.38, 0.36, 0.34]).describe("Stone colour")],
        |v: &Values, s: u32| {
            let (w, c) = (v.f("width"), v.f("cobble"));
            let path = b::s_path(v.f("length"), 1.2, 0.0, 40);
            let bed = strip(&path, -w * 0.5, w, 0.0, 0.05);
            let cobbles = units(&path, w - 0.05, [c, c * 0.5, c * 0.85], c * 0.12, c * 0.22, s);
            let mut lift = cobbles;
            geo::transform(&mut lift, [1.0; 3], [0.0, 0.02, 0.0]);
            Output::Parts(vec![Part::new(bed, [0.16, 0.15, 0.14]), Part::new(lift, v.color("color"))])
        });

    basic!(_r, "roads-brick-roads", "roads", "brick-roads", "Brick roads", "stone",
        [WIDTH, P::num("length", 8.0, 2.0, 40.0).unit("m").describe("Length (m)"), CURVE, P::color("color", [0.45, 0.17, 0.1]).describe("Brick colour")],
        |v: &Values, s: u32| {
            let w = v.f("width");
            let path = b::s_path(v.f("length"), v.f("curve"), 0.0, 40);
            let bed = strip(&path, -w * 0.5, w, 0.0, 0.04);
            let bricks = units(&path, w - 0.04, [0.1, 0.06, 0.2], 0.012, 0.008, s);
            Output::Parts(vec![Part::new(bed, [0.3, 0.29, 0.26]), Part::new(bricks, v.color("color"))])
        });

    basic!(_r, "roads-asphalt-roads", "roads", "asphalt-roads", "Asphalt roads", "stone",
        [WIDTH, LENGTH, CURVE, P::color("color", ASPHALT).describe("Asphalt colour")],
        |v: &Values, _s: u32| {
            let w = v.f("width");
            let path = line(v);
            let road = strip(&path, -w * 0.5, w, 0.0, 0.06);
            let mut paint = dashes(&path, 0.0, 0.055, 2.0, 5.0, 0.12);
            for x in [-w * 0.5 + 0.25, w * 0.5 - 0.37] {
                geo::merge(&mut paint, &strip(&path, x, 0.12, 0.055, 0.066));
            }
            Output::Parts(vec![Part::new(road, v.color("color")), Part::new(paint, PAINT)])
        });

    basic!(_r, "roads-concrete-roads", "roads", "concrete-roads", "Concrete roads", "stone",
        [WIDTH, LENGTH, P::num("slab", 4.0, 1.0, 12.0).unit("m").describe("Slab length (m)"), P::color("color", [0.55, 0.54, 0.5]).describe("Concrete colour")],
        |v: &Values, _s: u32| {
            let w = v.f("width");
            let path = b::s_path(v.f("length"), 1.5, 0.0, 60);
            let total = b::path_len(&path);
            let n = (total / v.f("slab")).round().max(1.0) as usize;
            let mut slabs = geo::Mesh::default();
            for i in 0..n {
                // Each slab its own stretch of the path, 2 cm short at each end.
                let (a, c) = ((i as f32 + 0.02 / v.f("slab")) / n as f32, (i as f32 + 1.0 - 0.02 / v.f("slab")) / n as f32);
                let sub: Vec<[f32; 3]> = (0..=6).map(|k| b::along(&path, a + (c - a) * k as f32 / 6.0).0).collect();
                for (x0, ww) in [(-w * 0.5, w * 0.5 - 0.01), (0.01, w * 0.5 - 0.01)] {
                    geo::merge(&mut slabs, &strip(&sub, x0, ww, 0.0, 0.12));
                }
            }
            let joints = strip(&path, -w * 0.5, w, 0.0, 0.1);
            Output::Parts(vec![Part::new(slabs, v.color("color")), Part::new(joints, [0.12, 0.12, 0.11])])
        });

    basic!(_r, "roads-wooden-boardwalks", "roads", "wooden-boardwalks", "Wooden boardwalks", "ground",
        [P::num("width", 1.8, 0.6, 6.0).unit("m").describe("Walk width (m)"), P::num("length", 10.0, 2.0, 40.0).unit("m").describe("Length (m)"), P::num("height", 0.6, 0.1, 3.0).unit("m").describe("Deck height (m)"), P::color("color", [0.36, 0.24, 0.14]).describe("Wood colour")],
        |v: &Values, s: u32| {
            let (w, h) = (v.f("width"), v.f("height"));
            let path = b::s_path(v.f("length"), 1.6, h, 40);
            let total = b::path_len(&path);
            let mut m = geo::Mesh::default();
            // Stringers under the planks, planks across, posts down.
            for x in [-w * 0.4, w * 0.4 - 0.08] {
                geo::merge(&mut m, &strip(&path, x, 0.08, -0.2, -0.04));
            }
            let n = (total / 0.16).floor() as u32;
            for i in 0..n {
                let (p, yaw, _) = b::along(&path, (i as f32 + 0.5) / n as f32);
                let plank = sh::block([0.14, 0.04, w * sh::range(s, i, 1, 0.97, 1.0)], [0.0, -0.04, 0.0]);
                geo::merge(&mut m, &sh::place(&plank, 1.0, yaw, 0.0, p));
            }
            let posts = (total / 2.0).floor() as u32 + 1;
            for i in 0..posts {
                let (p, _, right) = b::along(&path, i as f32 / (posts - 1).max(1) as f32);
                for x in [-w * 0.45, w * 0.45] {
                    geo::merge(&mut m, &b::post(p[0] + right[0] * x, p[2] + right[2] * x, 0.1, h - 0.04));
                }
            }
            Output::Mesh(m)
        });

    basic!(_r, "roads-railway-tracks", "roads", "railway-tracks", "Railway tracks", "stone",
        [LENGTH, CURVE, P::num("spacing", 0.65, 0.4, 1.5).unit("m").describe("Sleeper spacing (m)"), P::color("color", [0.25, 0.17, 0.11]).describe("Sleeper colour")],
        |v: &Values, _s: u32| {
            let path = line(v);
            let ballast = b::sweep(&path, &[[-1.8, 0.0], [1.8, 0.0], [1.3, 0.3], [-1.3, 0.3]], true);
            let total = b::path_len(&path);
            let n = (total / v.f("spacing")).floor().max(1.0) as u32;
            let mut sleepers = geo::Mesh::default();
            for i in 0..n {
                let (p, yaw, _) = b::along(&path, (i as f32 + 0.5) / n as f32);
                geo::merge(&mut sleepers, &sh::place(&sh::block([0.25, 0.15, 2.5], [0.0; 3]), 1.0, yaw, 0.0, [p[0], 0.24, p[2]]));
            }
            // Rails: a flat-bottomed profile (foot, web, head) each side.
            let mut rails = geo::Mesh::default();
            for c in [-0.75f32, 0.75] {
                let prof = [[c - 0.07, 0.39], [c + 0.07, 0.39], [c + 0.07, 0.405], [c + 0.01, 0.41], [c + 0.01, 0.52], [c + 0.035, 0.53], [c + 0.035, 0.56], [c - 0.035, 0.56], [c - 0.035, 0.53], [c - 0.01, 0.52], [c - 0.01, 0.41], [c - 0.07, 0.405]];
                geo::merge(&mut rails, &b::sweep(&path, &prof, true));
            }
            Output::Parts(vec![Part::new(ballast, [0.33, 0.31, 0.29]).material("stone"), Part::new(sleepers, v.color("color")).material("ground"), Part::new(rails, [0.3, 0.29, 0.28]).material("metal")])
        });

    basic!(_r, "roads-tram-tracks", "roads", "tram-tracks", "Tram tracks", "stone",
        [LENGTH, CURVE, P::num("width", 5.0, 3.0, 12.0).unit("m").describe("Street width (m)"), P::color("color", [0.3, 0.29, 0.28]).describe("Street colour")],
        |v: &Values, _s: u32| {
            let w = v.f("width");
            let path = line(v);
            let street = strip(&path, -w * 0.5, w, 0.0, 0.1);
            let (mut rails, mut grooves) = (geo::Mesh::default(), geo::Mesh::default());
            for c in [-0.72f32, 0.72] {
                geo::merge(&mut rails, &strip(&path, c - 0.04, 0.05, 0.05, 0.106));
                geo::merge(&mut grooves, &strip(&path, c + 0.01, 0.03, 0.05, 0.104));
            }
            Output::Parts(vec![Part::new(street, v.color("color")), Part::new(rails, [0.42, 0.41, 0.4]).material("metal"), Part::new(grooves, [0.03, 0.03, 0.03])])
        });

    basic!(_r, "roads-road-kerbs", "roads", "road-kerbs", "Road kerbs", "stone",
        [LENGTH, P::num("height", 0.15, 0.05, 0.4).unit("m").describe("Kerb height above the road (m)"), CURVE, P::color("color", KERB).describe("Kerb colour")],
        |v: &Values, _s: u32| {
            let h = v.f("height");
            let path = line(v);
            let total = b::path_len(&path);
            let n = (total / 1.0).round().max(1.0) as usize;
            let mut kerb = geo::Mesh::default();
            // One-metre kerb stones, the road side chamfered.
            for i in 0..n {
                let (a, c) = ((i as f32 + 0.006) / n as f32, (i as f32 + 0.994) / n as f32);
                let sub: Vec<[f32; 3]> = (0..=4).map(|k| b::along(&path, a + (c - a) * k as f32 / 4.0).0).collect();
                geo::merge(&mut kerb, &b::sweep(&sub, &[[-0.15, 0.0], [0.0, 0.0], [0.0, 0.06 + h], [-0.12, 0.06 + h], [-0.15, 0.03 + h]], true));
            }
            let road = strip(&path, -1.6, 1.45, 0.0, 0.06);
            let walk = strip(&path, 0.0, 1.4, 0.0, 0.05 + h);
            Output::Parts(vec![Part::new(kerb, v.color("color")), Part::new(road, ASPHALT), Part::new(walk, [0.42, 0.4, 0.37])])
        });

    basic!(_r, "roads-pavements", "roads", "pavements", "Pavements", "stone",
        [P::num("width", 2.4, 1.0, 8.0).unit("m").describe("Pavement width (m)"), P::num("length", 8.0, 2.0, 40.0).unit("m").describe("Length (m)"), P::num("slab", 0.6, 0.2, 1.2).unit("m").describe("Paving slab size (m)"), P::color("color", [0.5, 0.48, 0.45]).describe("Slab colour")],
        |v: &Values, s: u32| {
            let (w, k) = (v.f("width"), v.f("slab"));
            let path = b::s_path(v.f("length"), 1.0, 0.0, 40);
            let bed = strip(&path, -w * 0.5, w, 0.0, 0.08);
            let mut slabs = units(&path, w, [k, 0.04, k], 0.008, 0.0, s);
            geo::transform(&mut slabs, [1.0; 3], [0.0, 0.06, 0.0]);
            let kerb = strip(&path, w * 0.5, 0.15, 0.0, 0.13);
            Output::Parts(vec![Part::new(bed, [0.2, 0.2, 0.19]), Part::new(slabs, v.color("color")), Part::new(kerb, KERB)])
        });

    basic!(_r, "roads-road-intersections", "roads", "road-intersections", "Road intersections", "stone",
        [P::num("arm", 8.0, 3.0, 30.0).unit("m").describe("Arm length from the centre (m)"), WIDTH, P::num("stripes", 6.0, 0.0, 12.0).step(1.0).describe("Zebra stripes on each arm"), P::color("color", ASPHALT).describe("Asphalt colour")],
        |v: &Values, _s: u32| {
            let (a, h) = (v.f("arm"), v.f("width") * 0.5);
            let plan = [[-a, -h], [-h, -h], [-h, -a], [h, -a], [h, -h], [a, -h], [a, h], [h, h], [h, a], [-h, a], [-h, h], [-a, h]];
            let road = b::prism(&plan, 0.0, 0.06);
            let mut paint = geo::Mesh::default();
            let n = v.f("stripes").round() as u32;
            for k in 0..4 {
                let mut arm = geo::Mesh::default();
                // Zebra stripes, a stop line and the centre dashes, on +x.
                for i in 0..n {
                    let z = ((i as f32 + 0.5) / n.max(1) as f32 - 0.5) * h * 1.8;
                    geo::merge(&mut arm, &b::cbox([2.4, 0.012, h * 0.9 / n.max(1) as f32], [h + 1.6, 0.06, z]));
                }
                geo::merge(&mut arm, &b::cbox([0.3, 0.012, h * 0.95], [h + 3.2, 0.06, -h * 0.5]));
                let mut x = h + 4.0;
                while x + 1.5 < a {
                    geo::merge(&mut arm, &b::cbox([1.5, 0.012, 0.12], [x + 0.75, 0.06, 0.0]));
                    x += 4.0;
                }
                sh::rotate(&mut arm, [0.0, 1.0, 0.0], k as f32 * sh::PI * 0.5);
                geo::merge(&mut paint, &arm);
            }
            Output::Parts(vec![Part::new(road, v.color("color")), Part::new(paint, PAINT)])
        });

    basic!(_r, "roads-roundabouts", "roads", "roundabouts", "Roundabouts", "stone",
        [P::num("radius", 9.0, 4.0, 30.0).unit("m").describe("Outer radius (m)"), P::num("arms", 4.0, 3.0, 6.0).step(1.0).describe("Roads joining"), P::num("lane", 4.5, 3.0, 9.0).unit("m").describe("Ring road width (m)"), P::color("color", ASPHALT).describe("Asphalt colour")],
        |v: &Values, _s: u32| {
            let (r, lane) = (v.f("radius"), v.f("lane").min(v.f("radius") - 1.0));
            let ri = r - lane;
            let mut road = b::revolve(&[[ri, 0.0], [r, 0.0], [r, 0.06], [ri, 0.06], [ri, 0.0]], 72);
            let n = v.f("arms").round().max(3.0) as u32;
            for i in 0..n {
                let mut arm = sh::block([10.0, 0.05, 7.0], [r + 4.6, 0.0, 0.0]);
                sh::rotate(&mut arm, [0.0, 1.0, 0.0], i as f32 / n as f32 * sh::TAU + 0.3);
                geo::merge(&mut road, &arm);
            }
            let kerb = b::revolve(&[[ri - 0.2, 0.0], [ri, 0.0], [ri, 0.2], [ri - 0.2, 0.2], [ri - 0.2, 0.0]], 72);
            let island = b::revolve(&[[0.0, 0.0], [ri - 0.2, 0.0], [ri - 0.2, 0.18], [ri * 0.5, 0.45], [0.0, 0.6]], 48);
            let mut paint = geo::Mesh::default();
            for i in 0..36 {
                let a = i as f32 / 36.0 * sh::TAU;
                let mut d = b::cbox([0.12, 0.012, 1.0], [ri + lane * 0.5, 0.06, 0.0]);
                sh::rotate(&mut d, [0.0, 1.0, 0.0], a);
                geo::merge(&mut paint, &d);
            }
            Output::Parts(vec![Part::new(road, v.color("color")), Part::new(kerb, KERB), Part::new(island, [0.07, 0.13, 0.04]).material("ground"), Part::new(paint, PAINT)])
        });

    basic!(_r, "roads-bridges", "roads", "bridges", "Bridges", "stone",
        [P::num("span", 14.0, 4.0, 60.0).unit("m").describe("Arch span (m)"), P::num("height", 4.0, 1.5, 20.0).unit("m").describe("Deck height (m)"), P::num("width", 4.0, 1.5, 12.0).unit("m").describe("Deck width (m)"), P::color("color", [0.46, 0.43, 0.38]).describe("Stone colour")],
        |v: &Values, _s: u32| {
            let (span, h, w) = (v.f("span"), v.f("height"), v.f("width"));
            let rise = h - 0.6;
            // The arch: a ring segment swept from bank to bank.
            let arc: Vec<[f32; 3]> = (0..=32).map(|i| {
                let t = i as f32 / 32.0;
                let x = (t - 0.5) * span;
                [x, rise * (1.0 - (2.0 * t - 1.0).powi(2)), 0.0]
            }).collect();
            let mut m = b::sweep(&arc, &b::rect(w, 0.5), true);
            // Deck, parapets, abutments and the columns between arch and deck.
            let len = span + 4.0;
            geo::merge(&mut m, &sh::block([len, 0.5, w], [0.0, h - 0.5, 0.0]));
            for z in [-w * 0.5 + 0.15, w * 0.5 - 0.15] {
                geo::merge(&mut m, &sh::block([len, 0.8, 0.3], [0.0, h, z]));
            }
            for x in [-span * 0.5 - 1.0, span * 0.5 + 1.0] {
                geo::merge(&mut m, &sh::block([2.0, h - 0.5, w], [x, 0.0, 0.0]));
            }
            for i in 1..8 {
                let x = (i as f32 / 8.0 - 0.5) * span;
                let y = rise * (1.0 - (2.0 * x / span).powi(2));
                if h - 0.5 - y > 0.2 {
                    geo::merge(&mut m, &sh::block([0.4, h - 0.5 - y, w * 0.9], [x, y, 0.0]));
                }
            }
            Output::Mesh(m)
        });

    basic!(_r, "roads-tunnels", "roads", "tunnels", "Tunnels", "stone",
        [P::num("width", 7.0, 3.0, 16.0).unit("m").describe("Opening width (m)"), P::num("length", 10.0, 4.0, 40.0).unit("m").describe("Tunnel length shown (m)"), P::num("hill", 6.0, 3.0, 20.0).unit("m").describe("Hill height over the road (m)"), P::color("color", [0.45, 0.43, 0.4]).describe("Portal stone colour")],
        |v: &Values, s: u32| {
            let (w, l, hill) = (v.f("width"), v.f("length"), v.f("hill"));
            let r = w * 0.5;
            // The portal: a wall with an arched opening cut from its foot.
            let (ow, oh) = (w + 3.0, r * 0.6 + r + 1.6);
            let mut plan = vec![[-ow * 0.5, 0.0], [-r, 0.0]];
            for i in 0..=16 {
                let a = sh::PI - sh::PI * i as f32 / 16.0;
                plan.push([r * a.cos(), r * 0.6 + r * a.sin()]);
            }
            plan.extend([[r, 0.0], [ow * 0.5, 0.0], [ow * 0.5, oh], [-ow * 0.5, oh]]);
            let mut portal = b::extrude(&plan, 1.0);
            geo::transform(&mut portal, [1.0; 3], [0.0, 0.0, l * 0.5]);
            // The lining: walls and a vault, inside the hill.
            let mut lining = b::sweep(&[[0.0, 0.0, l * 0.5], [0.0, 0.0, -l * 0.5]], &[[-r - 0.4, 0.0], [-r, 0.0], [-r, r * 0.6], [r, r * 0.6], [r, 0.0], [r + 0.4, 0.0], [r + 0.4, r * 0.6 + r + 0.4], [-r - 0.4, r * 0.6 + r + 0.4]], true);
            let mut vault = sh::vault(l, r, r + 0.4, 0.0, s);
            geo::transform(&mut vault, [1.0; 3], [0.0, r * 0.6, 0.0]);
            geo::merge(&mut lining, &vault);
            let road = sh::block([w - 0.6, 0.06, l + 4.0], [0.0, 0.0, 2.0]);
            let earth = sh::blob([ow * 1.1, hill + oh * 0.5, l * 0.55], [0.0, 0.0, -1.0], 0.4, s, 3);
            Output::Parts(vec![Part::new(portal, v.color("color")), Part::new(lining, [0.3, 0.29, 0.28]), Part::new(road, ASPHALT), Part::new(earth, [0.09, 0.12, 0.05]).material("ground")])
        });
}
