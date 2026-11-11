//! Category 22, Agriculture, crops and food: basic generators (batch A). Wheat is
//! evani-grass, wrapped as it is (crate::wrapped). Soil ridges and rows of
//! plants, lathed and lumped produce, straw shapes, a frame-and-glass
//! greenhouse. Inspired by three.js (MIT) LatheGeometry, CylinderGeometry,
//! ConeGeometry and SphereGeometry, and SeedThree's plant idea (a stalk
//! with leaves); written fresh here.

use evani_procedural::Registry;

#[allow(unused_imports, dead_code)]
pub fn register(_r: &mut Registry) {
    use evani_procedural::{Output, ParamSpec as P, Part, Values, geo, noise, shapes as sh};
    const SOIL: [f32; 3] = [0.16, 0.1, 0.06];
    const LEAF: [f32; 3] = [0.12, 0.26, 0.06];
    const STRAW: [f32; 3] = [0.62, 0.48, 0.2];

    /// Soil with `rows` ridges along x, `l` long and `w` wide.
    fn ridges(l: f32, w: f32, rows: u32, s: u32) -> geo::Mesh {
        let n = rows.max(1) as f32;
        geo::height_grid([l, w], 64, |x, z| {
            let t = (z / w + 0.5) * n;
            0.12 * (0.5 + 0.5 * (t * sh::TAU).cos()).powf(0.7) + 0.02 * noise::fbm2(x, z, s, 2, 2.0, 0.5)
        })
    }

    /// The x, z of `per_row` plants on each of `rows` ridges.
    fn on_rows(l: f32, w: f32, rows: u32, per_row: u32, s: u32) -> Vec<[f32; 2]> {
        let mut out = Vec::new();
        for r in 0..rows.max(1) {
            let z = (r as f32 / rows.max(1) as f32 - 0.5) * w;
            for i in 0..per_row.max(1) {
                let x = ((i as f32 + 0.5) / per_row.max(1) as f32 - 0.5) * l + sh::range(s, r * 100 + i, 1, -0.05, 0.05);
                out.push([x, z]);
            }
        }
        out
    }

    /// A ribbed ball (pumpkin, melon): `ribs` grooves round y.
    fn ribbed(r: f32, ribs: f32, at: [f32; 3]) -> geo::Mesh {
        let mut m = sh::icosphere(3);
        for v in &mut m.vertices {
            let a = v[2].atan2(v[0]);
            let k = 1.0 - 0.06 * (a * ribs).cos().abs();
            *v = [at[0] + v[0] * r * k, at[1] + v[1] * r * 0.8 * k, at[2] + v[2] * r * k, 0.0, 1.0, 0.0];
        }
        geo::smooth_normals(&mut m);
        m
    }

    basic!(_r, "agriculture-crop-rows", "agriculture", "crop-rows", "Crop rows", "ground",
        [P::num("length", 8.0, 1.0, 60.0).unit("m").describe("Row length (m)"), P::num("rows", 6.0, 1.0, 30.0).step(1.0).describe("Rows"), P::color("color", LEAF).describe("Crop colour"), P::color("soil", SOIL).describe("Soil colour")],
        |v: &Values, s: u32| {
            let (l, n) = (v.f("length"), v.f("rows") as u32);
            let w = n as f32 * 0.75;
            let soil = ridges(l, w, n, s);
            let mut crops = geo::Mesh::default();
            for (i, p) in on_rows(l, w, n, (l / 0.35) as u32, s).into_iter().enumerate() {
                let k = sh::range(s, i as u32, 2, 0.08, 0.14);
                geo::merge(&mut crops, &sh::blob([k, k * 0.8, k], [p[0], 0.12 + k * 0.6, p[1]], 0.7, s.wrapping_add(i as u32), 1));
            }
            Output::Parts(vec![Part::new(soil, v.color("soil")), Part::new(crops, v.color("color")).material("grass")])
        });

    basic!(_r, "agriculture-corn-plants", "agriculture", "corn-plants", "Corn plants", "grass",
        [P::num("height", 2.2, 0.5, 4.0).unit("m").describe("Plant height (m)"), P::num("plants", 9.0, 1.0, 60.0).step(1.0).describe("Plants"), P::color("color", [0.16, 0.3, 0.07]).describe("Leaf colour"), P::color("cob", [0.8, 0.62, 0.15]).describe("Cob colour")],
        |v: &Values, s: u32| {
            let (h, n) = (v.f("height"), v.f("plants") as u32);
            let (mut plants, mut cobs) = (geo::Mesh::default(), geo::Mesh::default());
            let side = (n as f32).sqrt().ceil().max(1.0);
            for i in 0..n {
                let at = [((i as f32 % side) - side * 0.5 + 0.5) * 0.75, 0.0, ((i as f32 / side).floor() - side * 0.5 + 0.5) * 0.75];
                let t = h * sh::range(s, i, 1, 0.85, 1.05);
                geo::merge(&mut plants, &sh::place(&sh::frustum(0.022, 0.01, t, 6), 1.0, 0.0, 0.0, at));
                // Long leaves arching off the stalk, a tassel on top.
                for j in 0..8u32 {
                    let y = t * (0.15 + 0.09 * j as f32);
                    let mut lf = sh::blade(0.07, t * 0.45, [0.0, t * 0.3], 4);
                    sh::rotate(&mut lf, [1.0, 0.0, 0.0], 0.9);
                    geo::merge(&mut plants, &sh::place(&lf, 1.0, j as f32 * 2.4 + sh::rnd(s, i, 2), 0.0, [at[0], y, at[2]]));
                }
                geo::merge(&mut plants, &sh::place(&sh::blade(0.03, t * 0.12, [0.02, 0.0], 2), 1.0, 0.0, 0.0, [at[0], t, at[2]]));
                let a = sh::rnd(s, i, 3) * sh::TAU;
                geo::merge(&mut cobs, &sh::blob([0.035, 0.11, 0.035], [at[0] + a.cos() * 0.05, t * 0.55, at[2] + a.sin() * 0.05], 0.05, s, 1));
            }
            Output::Parts(vec![Part::new(plants, v.color("color")), Part::new(cobs, v.color("cob"))])
        });

    basic!(_r, "agriculture-potato-plants", "agriculture", "potato-plants", "Potato plants", "grass",
        [P::num("length", 4.0, 1.0, 40.0).unit("m").describe("Row length (m)"), P::num("rows", 3.0, 1.0, 20.0).step(1.0).describe("Rows"), P::color("color", [0.14, 0.3, 0.08]).describe("Leaf colour"), P::color("potato", [0.55, 0.4, 0.22]).describe("Potato colour")],
        |v: &Values, s: u32| {
            let (l, n) = (v.f("length"), v.f("rows") as u32);
            let w = n as f32 * 0.8;
            let soil = ridges(l, w, n, s);
            let (mut leaves, mut tubers) = (geo::Mesh::default(), geo::Mesh::default());
            for (i, p) in on_rows(l, w, n, (l / 0.5) as u32, s).into_iter().enumerate() {
                let i = i as u32;
                geo::merge(&mut leaves, &sh::crown([0.22, 0.16, 0.22], [p[0], 0.28, p[1]], 4, s.wrapping_add(i)));
                if i.is_multiple_of(3) {
                    geo::merge(&mut tubers, &sh::blob([0.05, 0.035, 0.04], [p[0] + 0.2, 0.12, p[1] + 0.08], 0.3, s.wrapping_add(i * 7), 1));
                }
            }
            Output::Parts(vec![Part::new(soil, SOIL).material("ground"), Part::new(leaves, v.color("color")), Part::new(tubers, v.color("potato")).material("ground")])
        });

    basic!(_r, "agriculture-pumpkin-plants", "agriculture", "pumpkin-plants", "Pumpkin plants", "grass",
        [P::num("size", 0.35, 0.1, 1.0).unit("m").describe("Pumpkin radius (m)"), P::num("pumpkins", 4.0, 1.0, 20.0).step(1.0).describe("Pumpkins"), P::color("color", [0.8, 0.35, 0.05]).describe("Pumpkin colour"), P::color("leaves", LEAF).describe("Leaf and vine colour")],
        |v: &Values, s: u32| {
            let (r, n) = (v.f("size"), v.f("pumpkins") as u32);
            let (mut fruit, mut vine) = (geo::Mesh::default(), geo::Mesh::default());
            let mut last = [0.0f32, r * 0.2, 0.0];
            for (i, p) in sh::scatter_disc(n, r * 2.5 * (n as f32).sqrt(), s).into_iter().enumerate() {
                let k = r * sh::range(s, i as u32, 1, 0.7, 1.1);
                geo::merge(&mut fruit, &ribbed(k, 5.0, [p[0], k * 0.8, p[1]]));
                // The vine wanders from pumpkin to pumpkin, leaves along it.
                let here = [p[0], 0.03, p[1]];
                let pts: Vec<[f32; 3]> = (0..=8).map(|j| {
                    let t = j as f32 / 8.0;
                    [last[0] + (here[0] - last[0]) * t + 0.2 * (t * 6.0).sin() * r, 0.03, last[2] + (here[2] - last[2]) * t]
                }).collect();
                geo::merge(&mut vine, &sh::tube(&pts, |_| 0.012, 4));
                for (j, q) in pts.iter().enumerate().step_by(3) {
                    let mut lf = sh::leaf(r * 0.8, r * 0.7);
                    sh::rotate(&mut lf, [0.0, 0.0, 1.0], 0.3);
                    geo::merge(&mut vine, &sh::place(&lf, 1.0, j as f32 * 1.7 + i as f32, 0.0, [q[0], 0.05, q[2]]));
                }
                last = here;
            }
            Output::Parts(vec![Part::new(fruit, v.color("color")).material("ground"), Part::new(vine, v.color("leaves"))])
        });

    basic!(_r, "agriculture-grape-vines", "agriculture", "grape-vines", "Grape vines", "grass",
        [P::num("length", 4.0, 1.0, 30.0).unit("m").describe("Trellis length (m)"), P::num("height", 1.6, 0.5, 3.0).unit("m").describe("Trellis height (m)"), P::color("color", [0.28, 0.08, 0.3]).describe("Grape colour"), P::color("leaves", [0.15, 0.3, 0.08]).describe("Leaf colour")],
        |v: &Values, s: u32| {
            let (l, h) = (v.f("length"), v.f("height"));
            let mut wood = geo::Mesh::default();
            let posts = (l / 1.5).ceil().max(1.0) as u32 + 1;
            for i in 0..posts {
                let x = (i as f32 / (posts - 1).max(1) as f32 - 0.5) * l;
                geo::merge(&mut wood, &sh::block([0.08, h, 0.08], [x, 0.0, 0.0]));
            }
            for y in [h * 0.55, h * 0.95] {
                geo::merge(&mut wood, &sh::tube(&[[-l * 0.5, y, 0.0], [l * 0.5, y, 0.0]], |_| 0.004, 4));
            }
            let (mut leaves, mut grapes) = (geo::Mesh::default(), geo::Mesh::default());
            for i in 0..(l * 8.0) as u32 {
                let x = sh::range(s, i, 1, -0.5, 0.5) * l;
                let y = h * sh::range(s, i, 2, 0.5, 1.05);
                let mut lf = sh::leaf(0.14, 0.12);
                sh::rotate(&mut lf, [1.0, 0.0, 0.0], sh::range(s, i, 3, 1.2, 1.9));
                geo::merge(&mut leaves, &sh::place(&lf, 1.0, sh::range(s, i, 4, -0.6, 0.6), 0.0, [x, y, 0.03]));
                if i % 4 == 0 {
                    // A bunch: balls in a downward cone.
                    for j in 0..14u32 {
                        let t = j as f32 / 14.0;
                        let a = j as f32 * 2.4;
                        let rr = 0.05 * (1.0 - t);
                        geo::merge(&mut grapes, &sh::blob([0.014; 3], [x + a.cos() * rr, h * 0.5 - t * 0.16, 0.06 + a.sin() * rr], 0.0, s, 1));
                    }
                }
            }
            Output::Parts(vec![Part::new(wood, [0.3, 0.22, 0.14]).material("ground"), Part::new(leaves, v.color("leaves")), Part::new(grapes, v.color("color")).material("ground")])
        });

    basic!(_r, "agriculture-orchard-trees", "agriculture", "orchard-trees", "Orchard trees", "grass",
        [P::num("spacing", 4.0, 1.5, 15.0).unit("m").describe("Distance between trees (m)"), P::num("rows", 3.0, 1.0, 10.0).step(1.0).describe("Rows and columns"), P::color("color", [0.1, 0.25, 0.05]).describe("Leaf colour"), P::color("fruit", [0.65, 0.08, 0.04]).describe("Fruit colour")],
        |v: &Values, s: u32| {
            let (sp, n) = (v.f("spacing"), v.f("rows") as u32);
            let (mut wood, mut leaves, mut fruit) = (geo::Mesh::default(), geo::Mesh::default(), geo::Mesh::default());
            for i in 0..n * n {
                let at = [((i % n) as f32 - (n as f32 - 1.0) * 0.5) * sp, 0.0, ((i / n) as f32 - (n as f32 - 1.0) * 0.5) * sp];
                let h = sp * 0.75 * sh::range(s, i, 1, 0.85, 1.05);
                let (t, _) = sh::trunk(h * 0.45, h * 0.04, h * 0.03, h * 0.04, s.wrapping_add(i), 7);
                geo::merge(&mut wood, &sh::place(&t, 1.0, 0.0, 0.0, at));
                let c = sh::crown([h * 0.32, h * 0.28, h * 0.32], [at[0], h * 0.68, at[2]], 5, s.wrapping_add(i * 3));
                for j in 0..12u32 {
                    let Some(p) = c.vertices.get((sh::rnd(s, i * 20 + j, 2) * c.vertices.len() as f32) as usize % c.vertices.len().max(1)) else { continue };
                    geo::merge(&mut fruit, &sh::blob([h * 0.025; 3], [p[0], p[1], p[2]], 0.0, s, 1));
                }
                geo::merge(&mut leaves, &c);
            }
            Output::Parts(vec![Part::new(wood, [0.22, 0.14, 0.08]).material("ground"), Part::new(leaves, v.color("color")), Part::new(fruit, v.color("fruit")).material("ground")])
        });

    basic!(_r, "agriculture-irrigation-channels", "agriculture", "irrigation-channels", "Irrigation channels", "ground",
        [P::num("length", 10.0, 2.0, 80.0).unit("m").describe("Channel length (m)"), P::num("width", 0.8, 0.2, 5.0).unit("m").describe("Channel width (m)"), P::color("color", [0.08, 0.2, 0.24]).describe("Water colour"), P::color("soil", [0.24, 0.17, 0.1]).describe("Bank colour")],
        |v: &Values, s: u32| {
            let (l, w) = (v.f("length"), v.f("width"));
            let mut ground = geo::height_grid([l, l * 0.5], 96, |x, z| {
                let k = (z.abs() / (w * 0.5)).min(1.6);
                0.4 * (k / 1.6).powi(2).min(1.0) + 0.02 * noise::fbm2(x, z, s, 2, 2.0, 0.5)
            });
            sh::to_ground(&mut ground);
            let water = sh::water_sheet([l, w * 1.1], 0.2, 16, |_, _| 0.0);
            // A small wooden sluice gate across it.
            let gate = sh::block([0.08, 0.6, w * 1.6], [l * 0.2, 0.0, 0.0]);
            Output::Parts(vec![Part::new(ground, v.color("soil")), Part::new(water, v.color("color")).alpha(0.8).material("glass"), Part::new(gate, [0.3, 0.2, 0.12])])
        });

    basic!(_r, "agriculture-greenhouses", "agriculture", "greenhouses", "Greenhouses", "glass",
        [P::num("length", 6.0, 2.0, 40.0).unit("m").describe("Greenhouse length (m)"), P::num("width", 3.0, 1.0, 12.0).unit("m").describe("Greenhouse width (m)"), P::color("color", [0.85, 0.95, 0.92]).describe("Glass tint"), P::color("frame", [0.85, 0.85, 0.83]).describe("Frame colour")],
        |v: &Values, _s: u32| {
            let (l, w) = (v.f("length"), v.f("width"));
            let (wall, ridge) = (w * 0.6, w * 0.95);
            let (mut frame, mut glass) = (geo::Mesh::default(), geo::Mesh::default());
            let bays = (l / 1.2).ceil().max(1.0) as u32;
            let bar = |a: [f32; 3], b: [f32; 3]| sh::tube(&[a, b], |_| 0.025, 4);
            for i in 0..=bays {
                let x = (i as f32 / bays as f32 - 0.5) * l;
                for side in [-1.0f32, 1.0] {
                    geo::merge(&mut frame, &bar([x, 0.0, side * w * 0.5], [x, wall, side * w * 0.5]));
                    geo::merge(&mut frame, &bar([x, wall, side * w * 0.5], [x, ridge, 0.0]));
                }
            }
            for (y, z) in [(wall, w * 0.5), (wall, -w * 0.5), (ridge, 0.0)] {
                geo::merge(&mut frame, &bar([-l * 0.5, y, z], [l * 0.5, y, z]));
            }
            // Panes: walls and the two roof slopes, thin slabs.
            for side in [-1.0f32, 1.0] {
                geo::merge(&mut glass, &sh::block([l, wall, 0.01], [0.0, 0.0, side * w * 0.5]));
                let mut roof = sh::block([l, 0.01, (w * 0.5f32).hypot(ridge - wall)], [0.0, 0.0, 0.0]);
                sh::rotate(&mut roof, [1.0, 0.0, 0.0], -side * ((ridge - wall) / (w * 0.5)).atan());
                geo::merge(&mut glass, &sh::place(&roof, 1.0, 0.0, 0.0, [0.0, (wall + ridge) * 0.5, side * w * 0.25]));
            }
            for end in [-1.0f32, 1.0] {
                geo::merge(&mut glass, &sh::block([0.01, wall, w], [end * l * 0.5, 0.0, 0.0]));
            }
            Output::Parts(vec![Part::new(frame, v.color("frame")).material("metal"), Part::new(glass, v.color("color")).alpha(0.5).material("glass")])
        });

    basic!(_r, "agriculture-hay-bales", "agriculture", "hay-bales", "Hay bales", "ground",
        [P::num("radius", 0.7, 0.2, 2.0).unit("m").describe("Bale radius (m)"), P::num("bales", 3.0, 1.0, 20.0).step(1.0).describe("Bales"), P::color("color", STRAW).describe("Straw colour")],
        |v: &Values, s: u32| {
            let (r, n) = (v.f("radius"), v.f("bales") as u32);
            let mut m = geo::Mesh::default();
            for i in 0..n {
                // A round bale lying on its side, slightly soft-edged.
                let mut b = geo::lathe(&[[0.0, 0.0], [r * 0.92, 0.0], [r, r * 0.08], [r, r * 1.32], [r * 0.92, r * 1.4], [0.0, r * 1.4]], 20);
                sh::fix_winding(&mut b);
                sh::rotate(&mut b, [0.0, 0.0, 1.0], sh::PI * 0.5);
                let at = [(i as f32 - (n as f32 - 1.0) * 0.5) * r * 2.6 + r * 0.7, r, sh::range(s, i, 1, -0.3, 0.3) * r];
                geo::merge(&mut m, &sh::place(&b, 1.0, sh::range(s, i, 2, -0.3, 0.3), 0.0, at));
            }
            Output::Mesh(m)
        });

    basic!(_r, "agriculture-haystacks", "agriculture", "haystacks", "Haystacks", "ground",
        [P::num("height", 2.5, 0.5, 8.0).unit("m").describe("Stack height (m)"), P::num("width", 2.2, 0.5, 8.0).unit("m").describe("Stack width (m)"), P::color("color", STRAW).describe("Hay colour")],
        |v: &Values, s: u32| {
            let (h, w) = (v.f("height"), v.f("width"));
            let prof: Vec<[f32; 2]> = (0..=10).map(|j| {
                let t = j as f32 / 10.0;
                [w * 0.5 * (1.0 - t * t).max(0.0).sqrt() * (1.0 - 0.25 * t) + 0.001, t * h]
            }).collect();
            let mut m = geo::lathe(&prof, 24);
            for vx in &mut m.vertices {
                let k = 1.0 + 0.06 * noise::fbm3([vx[0] * 2.0, vx[1] * 2.0, vx[2] * 2.0], s, 3);
                vx[0] *= k;
                vx[2] *= k;
            }
            geo::smooth_normals(&mut m);
            sh::fix_winding(&mut m);
            Output::Mesh(m)
        });

    basic!(_r, "agriculture-grain-piles", "agriculture", "grain-piles", "Grain piles", "ground",
        [P::num("radius", 1.2, 0.2, 6.0).unit("m").describe("Pile radius (m)"), P::num("slope", 0.6, 0.2, 1.0).describe("Steepness (grain settles at about 0.6)"), P::color("color", [0.66, 0.5, 0.22]).describe("Grain colour")],
        |v: &Values, s: u32| {
            let (r, sl) = (v.f("radius"), v.f("slope"));
            let mut m = geo::height_grid([r * 2.2; 2], 64, |x, z| {
                let d = (x * x + z * z).sqrt();
                let cone = (r - d).max(0.0) * sl;
                // A rounded top and grainy surface.
                let top = r * sl * 0.85;
                cone.min(top + (cone - top).max(0.0) * 0.3) + 0.01 * noise::value2(x * 40.0, z * 40.0, s)
            });
            sh::to_ground(&mut m);
            Output::Mesh(m)
        });

    basic!(_r, "agriculture-fruit", "agriculture", "fruit", "Fruit", "ground",
        [P::num("size", 1.0, 0.3, 3.0).describe("Scale (1 = real size)"), P::color("apple", [0.6, 0.06, 0.04]).describe("Apple colour"), P::color("pear", [0.55, 0.55, 0.12]).describe("Pear colour"), P::color("orange", [0.85, 0.35, 0.02]).describe("Orange colour"), P::color("banana", [0.85, 0.7, 0.12]).describe("Banana colour")],
        |v: &Values, s: u32| {
            let k = v.f("size");
            // An apple: a ball dimpled at the top and bottom.
            let mut apple = sh::icosphere(3);
            for vx in &mut apple.vertices {
                let dimple = 1.0 - 0.18 * vx[1].abs().powi(6);
                *vx = [vx[0] * 0.04 * k, (vx[1] * 0.85 * dimple + 0.85) * 0.04 * k, vx[2] * 0.04 * k, 0.0, 1.0, 0.0];
            }
            geo::smooth_normals(&mut apple);
            let apple = sh::place(&apple, 1.0, 0.0, 0.0, [-0.12 * k, 0.0, 0.0]);
            let pear = geo::lathe(&(0..=12).map(|j| { let t = j as f32 / 12.0; [0.045 * k * ((t * sh::PI).sin() * (1.0 - 0.45 * t) + 0.02), t * 0.11 * k] }).collect::<Vec<_>>(), 16);
            let pear = sh::place(&pear, 1.0, 0.0, 0.0, [0.0, 0.0, 0.05 * k]);
            let orange = sh::blob([0.04 * k; 3], [0.12 * k, 0.04 * k, 0.0], 0.03, s, 3);
            let curve: Vec<[f32; 3]> = (0..=10).map(|j| { let t = j as f32 / 10.0; let a = (t - 0.5) * 1.6; [a.sin() * 0.1 * k, 0.018 * k + (1.0 - a.cos()) * 0.1 * k, -0.1 * k] }).collect();
            let banana = sh::tube(&curve, |t| 0.018 * k * (t * sh::PI).sin().max(0.15), 6);
            Output::Parts(vec![Part::new(apple, v.color("apple")), Part::new(pear, v.color("pear")), Part::new(orange, v.color("orange")), Part::new(banana, v.color("banana"))])
        });

    basic!(_r, "agriculture-vegetables", "agriculture", "vegetables", "Vegetables", "ground",
        [P::num("size", 1.0, 0.3, 3.0).describe("Scale (1 = real size)"), P::color("carrot", [0.85, 0.3, 0.03]).describe("Carrot colour"), P::color("tomato", [0.7, 0.05, 0.03]).describe("Tomato colour"), P::color("cabbage", [0.3, 0.5, 0.18]).describe("Cabbage colour"), P::color("aubergine", [0.18, 0.05, 0.2]).describe("Aubergine colour")],
        |v: &Values, s: u32| {
            let k = v.f("size");
            let mut carrot = sh::frustum(0.018 * k, 0.0, 0.16 * k, 10);
            sh::rotate(&mut carrot, [0.0, 0.0, 1.0], sh::PI * 0.5);
            let carrot = sh::place(&carrot, 1.0, 0.3, 0.0, [-0.1 * k, 0.02 * k, 0.05 * k]);
            let mut tops = geo::Mesh::default();
            for j in 0..4u32 {
                let mut b = sh::blade(0.008 * k, 0.08 * k, [0.02 * k, 0.0], 3);
                sh::rotate(&mut b, [0.0, 0.0, 1.0], 1.0 + j as f32 * 0.2);
                geo::merge(&mut tops, &sh::place(&b, 1.0, 0.3, 0.0, [-0.1 * k, 0.02 * k, 0.05 * k]));
            }
            let tomato = sh::blob([0.035 * k, 0.03 * k, 0.035 * k], [0.08 * k, 0.03 * k, 0.07 * k], 0.05, s, 3);
            let cabbage = sh::blob([0.08 * k, 0.07 * k, 0.08 * k], [0.0, 0.07 * k, -0.08 * k], 0.25, s ^ 3, 2);
            let mut aub = geo::lathe(&(0..=12).map(|j| { let t = j as f32 / 12.0; [0.035 * k * ((t * sh::PI).sin() * (0.55 + 0.45 * (1.0 - t)) + 0.02), t * 0.17 * k] }).collect::<Vec<_>>(), 14);
            sh::rotate(&mut aub, [1.0, 0.0, 0.0], sh::PI * 0.5);
            let aub = sh::place(&aub, 1.0, -0.4, 0.0, [0.12 * k, 0.04 * k, -0.06 * k]);
            Output::Parts(vec![Part::new(carrot, v.color("carrot")), Part::new(tops, [0.15, 0.35, 0.08]).material("grass"), Part::new(tomato, v.color("tomato")), Part::new(cabbage, v.color("cabbage")).material("grass"), Part::new(aub, v.color("aubergine"))])
        });

    basic!(_r, "agriculture-bread-loaves", "agriculture", "bread-loaves", "Bread loaves", "ground",
        [P::num("size", 1.0, 0.3, 3.0).describe("Scale (1 = real size)"), P::num("scores", 3.0, 0.0, 8.0).step(1.0).describe("Cuts across the loaf"), P::color("color", [0.55, 0.32, 0.12]).describe("Crust colour")],
        |v: &Values, s: u32| {
            let (k, cuts) = (v.f("size"), v.f("scores"));
            // A loaf: a squashed lump scored across the top.
            let mut loaf = sh::blob([0.15 * k, 0.07 * k, 0.08 * k], [0.0, 0.0, 0.0], 0.08, s, 3);
            for vx in &mut loaf.vertices {
                if vx[1] > 0.0 && cuts > 0.0 {
                    let t = (vx[0] / (0.3 * k) + 0.5) * (cuts + 1.0);
                    let groove = (1.0 - ((t.fract() - 0.5).abs() * 8.0).min(1.0)) * if t > 0.5 && t < cuts + 0.5 { 1.0 } else { 0.0 };
                    vx[1] -= 0.012 * k * groove;
                }
                vx[1] = vx[1].max(-0.02 * k);
            }
            geo::smooth_normals(&mut loaf);
            let loaf = sh::place(&loaf, 1.0, 0.0, 0.0, [0.0, 0.02 * k, 0.0]);
            let boule = sh::blob([0.09 * k, 0.065 * k, 0.09 * k], [0.25 * k, 0.04 * k, 0.05 * k], 0.06, s ^ 5, 3);
            let pts: Vec<[f32; 3]> = (0..=10).map(|j| [-0.3 * k + j as f32 * 0.06 * k, 0.03 * k, -0.17 * k]).collect();
            let baguette = sh::tube(&pts, |t| 0.03 * k * ((t * sh::PI).sin().powf(0.3)).max(0.2), 10);
            let mut m = loaf;
            geo::merge(&mut m, &boule);
            geo::merge(&mut m, &baguette);
            Output::Mesh(m)
        });
}
