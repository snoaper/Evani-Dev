//! Category 4, Grass, plants and ground vegetation: basic generators (batch A).
//! Short and long grass are evani-grass, wrapped as it is (crate::wrapped).
//! Blades, leaves and lumpy crowns scattered by seed. Inspired by
//! threejs-grass and three.js (MIT) PlaneGeometry blades bent along their
//! height; written fresh here.

use evani_procedural::Registry;

#[allow(unused_imports, dead_code)]
pub fn register(_r: &mut Registry) {
    use evani_procedural::{Output, ParamSpec as P, Part, Values, geo, noise, shapes as sh};
    const LEAF: [f32; 3] = [0.1, 0.24, 0.05];

    basic!(_r, "vegetation-wildflowers", "vegetation", "wildflowers", "Wildflowers", "grass",
        [P::num("count", 40.0, 1.0, 300.0).step(1.0).describe("Flowers"), P::num("spread", 1.0, 0.2, 6.0).unit("m").describe("Patch radius (m)"), P::color("color", [0.75, 0.2, 0.35]).describe("Petal colour"), P::color("stem", LEAF).describe("Stem colour")],
        |v: &Values, s: u32| {
            let (mut stems, mut heads) = (geo::Mesh::default(), geo::Mesh::default());
            for (i, p) in sh::scatter_disc(v.f("count") as u32, v.f("spread"), s).into_iter().enumerate() {
                let i = i as u32;
                let h = sh::range(s, i, 1, 0.25, 0.5);
                let bend = [sh::range(s, i, 2, -0.06, 0.06), sh::range(s, i, 3, -0.06, 0.06)];
                geo::merge(&mut stems, &sh::place(&sh::blade(0.012, h, bend, 3), 1.0, sh::rnd(s, i, 4) * sh::TAU, 0.0, [p[0], 0.0, p[1]]));
                let top = [p[0] + bend[0], h, p[1] + bend[1]];
                for k in 0..5u32 {
                    let a = k as f32 / 5.0 * sh::TAU;
                    let mut pe = sh::leaf(0.035, 0.022);
                    sh::rotate(&mut pe, [0.0, 1.0, 0.0], a);
                    geo::merge(&mut heads, &sh::place(&pe, 1.0, 0.0, 0.0, top));
                }
            }
            Output::Parts(vec![Part::new(stems, v.color("stem")), Part::new(heads, v.color("color"))])
        });

    basic!(_r, "vegetation-flower-beds", "vegetation", "flower-beds", "Flower beds", "grass",
        [P::num("length", 2.4, 0.5, 20.0).unit("m").describe("Bed length (m)"), P::num("width", 1.0, 0.3, 6.0).unit("m").describe("Bed width (m)"), P::color("color", [0.8, 0.55, 0.1]).describe("Flower colour"), P::color("soil", [0.12, 0.07, 0.04]).describe("Soil colour")],
        |v: &Values, s: u32| {
            let (l, w) = (v.f("length"), v.f("width"));
            let soil = sh::block([l, 0.12, w], [0.0, 0.0, 0.0]);
            let (mut leaves, mut flowers) = (geo::Mesh::default(), geo::Mesh::default());
            let n = ((l * w) * 30.0) as u32;
            for (i, p) in sh::scatter_rect(n.max(4), [l * 0.45, w * 0.42], s).into_iter().enumerate() {
                let i = i as u32;
                let k = sh::range(s, i, 1, 0.07, 0.12);
                geo::merge(&mut leaves, &sh::blob([k, k * 0.7, k], [p[0], 0.12 + k * 0.5, p[1]], 0.6, s.wrapping_add(i), 1));
                geo::merge(&mut flowers, &sh::blob([k * 0.45; 3], [p[0], 0.12 + k * 1.3, p[1]], 0.3, s.wrapping_add(i * 3), 1));
            }
            Output::Parts(vec![Part::new(soil, v.color("soil")).material("ground"), Part::new(leaves, LEAF), Part::new(flowers, v.color("color"))])
        });

    basic!(_r, "vegetation-weeds", "vegetation", "weeds", "Weeds", "grass",
        [P::num("count", 6.0, 1.0, 40.0).step(1.0).describe("Weeds"), P::num("size", 0.25, 0.05, 1.0).unit("m").describe("Leaf length (m)"), P::color("color", [0.16, 0.3, 0.07]).describe("Leaf colour")],
        |v: &Values, s: u32| {
            let mut m = geo::Mesh::default();
            let k = v.f("size");
            for (i, p) in sh::scatter_disc(v.f("count") as u32, k * 3.0, s).into_iter().enumerate() {
                // A rosette of leaves lifted at their tips, and a seed stalk.
                for j in 0..7u32 {
                    let mut lf = sh::leaf(k * sh::range(s, i as u32 * 9 + j, 1, 0.7, 1.1), k * 0.35);
                    sh::rotate(&mut lf, [0.0, 0.0, 1.0], 0.35);
                    geo::merge(&mut m, &sh::place(&lf, 1.0, j as f32 / 7.0 * sh::TAU + sh::rnd(s, i as u32, 2), 0.0, [p[0], 0.01, p[1]]));
                }
                geo::merge(&mut m, &sh::place(&sh::blade(0.01, k * 1.6, [0.03, 0.02], 3), 1.0, 0.0, 0.0, [p[0], 0.0, p[1]]));
            }
            Output::Mesh(m)
        });

    basic!(_r, "vegetation-ferns", "vegetation", "ferns", "Ferns", "grass",
        [P::num("size", 0.8, 0.1, 3.0).unit("m").describe("Frond length (m)"), P::num("fronds", 10.0, 3.0, 30.0).step(1.0).describe("Fronds"), P::color("color", [0.1, 0.27, 0.05]).describe("Frond colour")],
        |v: &Values, s: u32| {
            let (l, n) = (v.f("size"), v.f("fronds") as u32);
            let mut m = geo::Mesh::default();
            for i in 0..n {
                // A frond arching out and down, with leaflets along it.
                let yaw = i as f32 / n as f32 * sh::TAU + sh::rnd(s, i, 1) * 0.4;
                let pts: Vec<[f32; 3]> = (0..=8).map(|j| {
                    let t = j as f32 / 8.0;
                    [t * l * 0.85, l * (0.55 * t - 0.5 * t * t), 0.0]
                }).collect();
                let mut fr = sh::tube(&pts, |t| 0.008 * (1.0 - t) + 0.002, 4);
                for (j, p) in pts.iter().enumerate().skip(1) {
                    let t = j as f32 / 8.0;
                    for side in [-1.0f32, 1.0] {
                        let mut lf = sh::leaf(l * 0.22 * (1.0 - t * 0.7), l * 0.06);
                        sh::rotate(&mut lf, [0.0, 1.0, 0.0], side * 1.3);
                        geo::merge(&mut fr, &sh::place(&lf, 1.0, 0.0, 0.0, *p));
                    }
                }
                geo::merge(&mut m, &sh::place(&fr, 1.0, yaw, 0.0, [0.0, 0.02, 0.0]));
            }
            Output::Mesh(m)
        });

    basic!(_r, "vegetation-bushes", "vegetation", "bushes", "Bushes", "grass",
        [P::num("size", 1.2, 0.2, 6.0).unit("m").describe("Bush width (m)"), P::num("lumps", 9.0, 2.0, 30.0).step(1.0).describe("Leafy lumps"), P::color("color", [0.09, 0.22, 0.05]).describe("Leaf colour")],
        |v: &Values, s: u32| {
            let k = v.f("size");
            let mut m = sh::crown([k * 0.5, k * 0.38, k * 0.5], [0.0, k * 0.35, 0.0], v.f("lumps") as u32, s);
            sh::to_ground(&mut m);
            Output::Mesh(m)
        });

    basic!(_r, "vegetation-shrubs", "vegetation", "shrubs", "Shrubs", "grass",
        [P::num("height", 1.6, 0.3, 6.0).unit("m").describe("Shrub height (m)"), P::num("stems", 5.0, 1.0, 16.0).step(1.0).describe("Woody stems"), P::color("color", [0.12, 0.25, 0.06]).describe("Leaf colour"), P::color("wood", [0.22, 0.14, 0.08]).describe("Stem colour")],
        |v: &Values, s: u32| {
            let (h, n) = (v.f("height"), v.f("stems") as u32);
            let (mut wood, mut leaves) = (geo::Mesh::default(), geo::Mesh::default());
            for i in 0..n {
                let a = i as f32 / n as f32 * sh::TAU + sh::rnd(s, i, 1);
                let dir = [a.cos() * 0.35, 1.0, a.sin() * 0.35];
                let p = sh::path([0.0; 3], dir, h * sh::range(s, i, 2, 0.7, 1.0), 6, [0.0; 3], 0.1, s.wrapping_add(i));
                geo::merge(&mut wood, &sh::tube(&p, |t| 0.025 * (1.0 - 0.7 * t), 5));
                if let Some(tip) = p.last() {
                    geo::merge(&mut leaves, &sh::crown([h * 0.22, h * 0.2, h * 0.22], *tip, 4, s.wrapping_add(i * 5)));
                }
            }
            Output::Parts(vec![Part::new(wood, v.color("wood")).material("ground"), Part::new(leaves, v.color("color"))])
        });

    basic!(_r, "vegetation-reeds", "vegetation", "reeds", "Reeds", "grass",
        [P::num("height", 1.8, 0.3, 4.0).unit("m").describe("Reed height (m)"), P::num("density", 70.0, 5.0, 400.0).step(1.0).describe("Reeds"), P::color("color", [0.22, 0.3, 0.1]).describe("Reed colour"), P::color("heads", [0.25, 0.14, 0.06]).describe("Seed head colour")],
        |v: &Values, s: u32| {
            let h = v.f("height");
            let (mut reeds, mut heads) = (geo::Mesh::default(), geo::Mesh::default());
            for (i, p) in sh::scatter_disc(v.f("density") as u32, h * 0.35, s).into_iter().enumerate() {
                let i = i as u32;
                let t = h * sh::range(s, i, 1, 0.6, 1.1);
                let bend = [sh::range(s, i, 2, -0.12, 0.12), sh::range(s, i, 3, -0.12, 0.12)];
                geo::merge(&mut reeds, &sh::place(&sh::blade(0.03, t, bend, 4), 1.0, sh::rnd(s, i, 4) * sh::TAU, 0.0, [p[0], 0.0, p[1]]));
                if i.is_multiple_of(3) {
                    geo::merge(&mut heads, &sh::blob([0.022, 0.09, 0.022], [p[0] + bend[0], t * 0.9, p[1] + bend[1]], 0.1, s, 1));
                }
            }
            Output::Parts(vec![Part::new(reeds, v.color("color")), Part::new(heads, v.color("heads"))])
        });

    basic!(_r, "vegetation-moss-patches", "vegetation", "moss-patches", "Moss patches", "grass",
        [P::num("spread", 1.0, 0.1, 8.0).unit("m").describe("Patch radius (m)"), P::num("patches", 12.0, 1.0, 60.0).step(1.0).describe("Patches"), P::color("color", [0.12, 0.25, 0.04]).describe("Moss colour")],
        |v: &Values, s: u32| {
            let r = v.f("spread");
            let mut m = geo::Mesh::default();
            for (i, p) in sh::scatter_disc(v.f("patches") as u32, r, s).into_iter().enumerate() {
                let k = r * 0.25 * sh::range(s, i as u32, 1, 0.5, 1.2);
                geo::merge(&mut m, &sh::blob([k, k * 0.12, k * 0.8], [p[0], 0.0, p[1]], 0.8, s.wrapping_add(i as u32 * 7), 2));
            }
            for vx in &mut m.vertices {
                vx[1] = vx[1].max(0.0);
            }
            Output::Mesh(m)
        });

    basic!(_r, "vegetation-vines", "vegetation", "vines", "Vines", "grass",
        [P::num("height", 2.0, 0.5, 8.0).unit("m").describe("Height they climb (m)"), P::num("turns", 3.0, 0.5, 10.0).describe("Turns round the post"), P::color("color", [0.1, 0.25, 0.05]).describe("Leaf colour"), P::color("post", [0.35, 0.26, 0.16]).describe("Post colour")],
        |v: &Values, s: u32| {
            let (h, turns) = (v.f("height"), v.f("turns"));
            let post = sh::frustum(0.05, 0.045, h * 1.05, 8);
            let pts: Vec<[f32; 3]> = (0..=48).map(|j| {
                let t = j as f32 / 48.0;
                let a = t * turns * sh::TAU;
                [a.cos() * 0.07, t * h, a.sin() * 0.07]
            }).collect();
            let vine = sh::tube(&pts, |_| 0.008, 4);
            let mut leaves = geo::Mesh::default();
            for (j, p) in pts.iter().enumerate().step_by(2) {
                let mut lf = sh::leaf(0.09, 0.06);
                sh::rotate(&mut lf, [0.0, 0.0, 1.0], -0.6);
                geo::merge(&mut leaves, &sh::place(&lf, 1.0, j as f32 * 0.9 + sh::rnd(s, j as u32, 1), 0.0, *p));
            }
            Output::Parts(vec![Part::new(post, v.color("post")).material("ground"), Part::new(vine, [0.12, 0.16, 0.05]), Part::new(leaves, v.color("color"))])
        });

    basic!(_r, "vegetation-ground-cover-plants", "vegetation", "ground-cover-plants", "Ground cover plants", "grass",
        [P::num("spread", 1.5, 0.2, 10.0).unit("m").describe("Patch radius (m)"), P::num("plants", 30.0, 2.0, 200.0).step(1.0).describe("Plants"), P::color("color", [0.11, 0.28, 0.07]).describe("Leaf colour")],
        |v: &Values, s: u32| {
            let mut m = geo::Mesh::default();
            for (i, p) in sh::scatter_disc(v.f("plants") as u32, v.f("spread"), s).into_iter().enumerate() {
                let i = i as u32;
                let k = sh::range(s, i, 1, 0.06, 0.12);
                for j in 0..5u32 {
                    let mut lf = sh::leaf(k, k * 0.6);
                    sh::rotate(&mut lf, [0.0, 0.0, 1.0], 0.25);
                    geo::merge(&mut m, &sh::place(&lf, 1.0, j as f32 * 1.256 + sh::rnd(s, i, 2) * 3.0, 0.0, [p[0], 0.01, p[1]]));
                }
            }
            Output::Mesh(m)
        });
}
