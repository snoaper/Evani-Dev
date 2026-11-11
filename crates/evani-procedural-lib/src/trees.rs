//! Category 5, Trees and forests: basic generators (batch A). A tapered, leaning
//! trunk, branches off its upper part and leafy crowns (or cone tiers,
//! fronds, hanging strands) at their tips. Inspired by SeedThree's and
//! threejs-trees' ideas (a trunk and recursive branches, foliage at the
//! tips) and three.js (MIT) TubeGeometry; written fresh here.

use evani_procedural::Registry;

#[allow(unused_imports, dead_code)]
pub fn register(_r: &mut Registry) {
    use evani_procedural::{Output, ParamSpec as P, Part, Values, geo, noise, shapes as sh};
    const BARK: [f32; 3] = [0.2, 0.13, 0.08];
    const LEAVES: [f32; 3] = [0.08, 0.2, 0.04];
    const HEIGHT: P = P::num("height", 7.0, 1.0, 40.0).unit("m").describe("Tree height (m)");

    /// A broadleaf tree: trunk, `n` branches and a crown at each tip.
    fn broadleaf(h: f32, n: u32, crown: f32, s: u32) -> (geo::Mesh, geo::Mesh) {
        let (mut wood, trunk) = sh::trunk(h * 0.65, h * 0.045, h * 0.022, h * 0.05, s, 9);
        let (b, tips) = sh::branches(&trunk, n, 0.45, h * 0.35, 0.55, h * 0.018, s);
        geo::merge(&mut wood, &b);
        let mut leaves = geo::Mesh::default();
        for (i, t) in tips.iter().enumerate() {
            geo::merge(&mut leaves, &sh::crown([h * crown; 3], *t, 4, s.wrapping_add(i as u32 * 37)));
        }
        if let Some(top) = trunk.last() {
            geo::merge(&mut leaves, &sh::crown([h * crown * 1.2; 3], *top, 5, s ^ 99));
        }
        (wood, leaves)
    }

    // Photoreal (the Asset Studio oak session): crate::oak.
    basic!(_r, "trees-oak-trees", "trees", "oak-trees", "Oak trees", "grass",
        [P::num("height", 13.0, 2.0, 40.0).unit("m").describe("Tree height (m)"),
         P::num("spread", 1.25, 0.5, 2.0).describe("Crown width over the height"),
         P::num("branches", 2.0, 0.0, 10.0).step(1.0).describe("Big limbs off the trunk below the fork (four leaders grow from the fork too)"),
         P::num("leaves", 1.0, 0.2, 2.0).describe("Leaf sprays per clump, over the usual"),
         P::num("gnarl", 0.6, 0.0, 1.0).describe("How crooked the limbs grow (0 straight)"),
         P::num("grove", 1.0, 1.0, 5.0).step(1.0).describe("Oaks in the group (the extra, smaller ones stand behind the first, seen from +z)"),
         P::color("color", crate::oak::LEAVES).describe("Leaf colour"), P::color("bark", crate::oak::BARK).describe("Bark colour")],
        crate::oak::make);

    basic!(_r, "trees-pine-trees", "trees", "pine-trees", "Pine trees", "grass",
        [P::num("height", 9.0, 1.0, 50.0).unit("m").describe("Tree height (m)"), P::num("tiers", 6.0, 2.0, 14.0).step(1.0).describe("Tiers of boughs"), P::color("color", [0.05, 0.16, 0.06]).describe("Needle colour"), P::color("bark", [0.18, 0.11, 0.07]).describe("Bark colour")],
        |v: &Values, s: u32| {
            let (h, n) = (v.f("height"), v.f("tiers") as u32);
            let (wood, _) = sh::trunk(h * 0.95, h * 0.035, h * 0.01, h * 0.01, s, 8);
            let mut needles = geo::Mesh::default();
            for i in 0..n {
                let t = i as f32 / n as f32;
                let (r, y) = (h * 0.22 * (1.0 - t * 0.85), h * (0.25 + 0.7 * t));
                let mut c = sh::frustum(r, 0.0, h * 0.24, 12);
                // Drooping, uneven bough edges.
                for vx in &mut c.vertices {
                    let a = vx[2].atan2(vx[0]);
                    vx[1] -= 0.08 * r * (1.0 + (a * 7.0 + i as f32).sin()) * ((vx[0] * vx[0] + vx[2] * vx[2]).sqrt() / r.max(1e-3));
                }
                geo::smooth_normals(&mut c);
                geo::merge(&mut needles, &sh::place(&c, 1.0, sh::rnd(s, i, 1) * sh::TAU, 0.0, [0.0, y, 0.0]));
            }
            Output::Parts(vec![Part::new(wood, v.color("bark")).material("ground"), Part::new(needles, v.color("color"))])
        });

    basic!(_r, "trees-birch-trees", "trees", "birch-trees", "Birch trees", "grass",
        [P::num("height", 9.0, 1.0, 30.0).unit("m").describe("Tree height (m)"), P::num("lean", 0.4, 0.0, 2.0).unit("m").describe("How far the top leans (m)"), P::color("color", [0.2, 0.32, 0.06]).describe("Leaf colour"), P::color("bark", [0.72, 0.7, 0.64]).describe("Bark colour")],
        |v: &Values, s: u32| {
            let h = v.f("height");
            let (mut wood, trunk) = sh::trunk(h * 0.8, h * 0.022, h * 0.01, v.f("lean"), s, 8);
            let (b, tips) = sh::branches(&trunk, 8, 0.5, h * 0.18, 0.75, h * 0.008, s);
            geo::merge(&mut wood, &b);
            let mut leaves = geo::Mesh::default();
            for (i, t) in tips.iter().enumerate() {
                geo::merge(&mut leaves, &sh::crown([h * 0.08, h * 0.1, h * 0.08], *t, 3, s.wrapping_add(i as u32 * 7)));
            }
            if let Some(top) = trunk.last() {
                geo::merge(&mut leaves, &sh::crown([h * 0.1, h * 0.16, h * 0.1], *top, 4, s ^ 5));
            }
            Output::Parts(vec![Part::new(wood, v.color("bark")).material("ground"), Part::new(leaves, v.color("color"))])
        });

    basic!(_r, "trees-palm-trees", "trees", "palm-trees", "Palm trees", "grass",
        [P::num("height", 7.0, 1.0, 30.0).unit("m").describe("Tree height (m)"), P::num("curve", 1.2, 0.0, 4.0).unit("m").describe("How far the trunk curves (m)"), P::color("color", [0.1, 0.25, 0.05]).describe("Frond colour"), P::color("bark", [0.32, 0.24, 0.15]).describe("Trunk colour")],
        |v: &Values, s: u32| {
            let h = v.f("height");
            let trunk = sh::path([0.0; 3], [0.0, 1.0, 0.0], h, 10, [v.f("curve") / h.max(0.1), 0.0, 0.0], 0.0, s);
            let wood = sh::tube(&trunk, |t| h * (0.03 - 0.012 * t), 8);
            let top = trunk.last().copied().unwrap_or([0.0, h, 0.0]);
            let mut fronds = geo::Mesh::default();
            for i in 0..10u32 {
                let yaw = i as f32 / 10.0 * sh::TAU + sh::rnd(s, i, 1) * 0.3;
                let len = h * sh::range(s, i, 2, 0.38, 0.5);
                // A wide blade built up the y axis bending to +z, laid over
                // (about x) so it reaches out along +z and droops.
                let mut f = sh::blade(len * 0.18, len, [0.0, len * 0.55], 6);
                sh::rotate(&mut f, [1.0, 0.0, 0.0], 1.35);
                geo::merge(&mut fronds, &sh::place(&f, 1.0, yaw, 0.0, top));
            }
            Output::Parts(vec![Part::new(wood, v.color("bark")).material("ground"), Part::new(fronds, v.color("color"))])
        });

    basic!(_r, "trees-willow-trees", "trees", "willow-trees", "Willow trees", "grass",
        [HEIGHT, P::num("strands", 60.0, 10.0, 200.0).step(1.0).describe("Hanging strands"), P::color("color", [0.2, 0.3, 0.07]).describe("Leaf colour"), P::color("bark", BARK).describe("Bark colour")],
        |v: &Values, s: u32| {
            let h = v.f("height");
            let (wood, mut leaves) = broadleaf(h, 6, 0.14, s);
            for (i, p) in sh::scatter_disc(v.f("strands") as u32, h * 0.38, s ^ 3).into_iter().enumerate() {
                let i = i as u32;
                let top = h * (0.85 - 0.25 * ((p[0] * p[0] + p[1] * p[1]).sqrt() / (h * 0.38)));
                let len = top * sh::range(s, i, 1, 0.45, 0.8);
                let mut b = sh::blade(h * 0.025, len, [0.0, 0.0], 4);
                sh::rotate(&mut b, [1.0, 0.0, 0.0], sh::PI);
                geo::merge(&mut leaves, &sh::place(&b, 1.0, sh::rnd(s, i, 2) * sh::TAU, 0.0, [p[0], top, p[1]]));
            }
            Output::Parts(vec![Part::new(wood, v.color("bark")).material("ground"), Part::new(leaves, v.color("color"))])
        });

    basic!(_r, "trees-fruit-trees", "trees", "fruit-trees", "Fruit trees", "grass",
        [P::num("height", 4.0, 1.0, 15.0).unit("m").describe("Tree height (m)"), P::num("fruit", 30.0, 0.0, 200.0).step(1.0).describe("Fruit on the tree"), P::color("color", [0.1, 0.24, 0.05]).describe("Leaf colour"), P::color("fruit_color", [0.6, 0.06, 0.04]).describe("Fruit colour")],
        |v: &Values, s: u32| {
            let h = v.f("height");
            let (wood, leaves) = broadleaf(h, 6, 0.18, s);
            let mut fruit = geo::Mesh::default();
            let n = leaves.vertices.len().max(1);
            for i in 0..v.f("fruit") as u32 {
                let Some(p) = leaves.vertices.get((sh::rnd(s, i, 1) * n as f32) as usize % n) else { continue };
                geo::merge(&mut fruit, &sh::blob([h * 0.018; 3], [p[0], p[1] - h * 0.01, p[2]], 0.1, s, 1));
            }
            Output::Parts(vec![Part::new(wood, BARK).material("ground"), Part::new(leaves, v.color("color")), Part::new(fruit, v.color("fruit_color"))])
        });

    basic!(_r, "trees-saplings", "trees", "saplings", "Saplings", "grass",
        [P::num("height", 1.2, 0.2, 4.0).unit("m").describe("Sapling height (m)"), P::color("color", [0.16, 0.32, 0.07]).describe("Leaf colour"), P::color("bark", [0.3, 0.2, 0.1]).describe("Stem colour")],
        |v: &Values, s: u32| {
            let h = v.f("height");
            let (mut wood, trunk) = sh::trunk(h, 0.012 * h.max(0.5), 0.004, h * 0.05, s, 6);
            let (b, tips) = sh::branches(&trunk, 4, 0.5, h * 0.25, 0.6, 0.005, s);
            geo::merge(&mut wood, &b);
            let mut leaves = geo::Mesh::default();
            for (i, t) in tips.iter().chain(trunk.last()).enumerate() {
                for j in 0..5u32 {
                    let mut lf = sh::leaf(h * 0.08, h * 0.04);
                    sh::rotate(&mut lf, [0.0, 0.0, 1.0], 0.4);
                    geo::merge(&mut leaves, &sh::place(&lf, 1.0, j as f32 * 1.256 + i as f32, 0.0, *t));
                }
            }
            Output::Parts(vec![Part::new(wood, v.color("bark")).material("ground"), Part::new(leaves, v.color("color"))])
        });

    basic!(_r, "trees-dead-trees", "trees", "dead-trees", "Dead trees", "ground",
        [HEIGHT, P::num("branches", 6.0, 1.0, 16.0).step(1.0).describe("Bare branches"), P::color("color", [0.28, 0.25, 0.22]).describe("Weathered wood colour")],
        |v: &Values, s: u32| {
            let h = v.f("height");
            let (mut wood, trunk) = sh::trunk(h * 0.75, h * 0.045, h * 0.012, h * 0.12, s, 8);
            let (b, tips) = sh::branches(&trunk, v.f("branches") as u32, 0.35, h * 0.35, 0.45, h * 0.015, s);
            geo::merge(&mut wood, &b);
            // Twigs off each branch tip.
            for (i, t) in tips.iter().enumerate() {
                for j in 0..2u32 {
                    let a = sh::range(s, i as u32 * 3 + j, 1, 0.0, sh::TAU);
                    let p = sh::path(*t, [a.cos(), 0.8, a.sin()], h * 0.1, 3, [0.0; 3], h * 0.02, s.wrapping_add(j));
                    geo::merge(&mut wood, &sh::tube(&p, |u| h * 0.005 * (1.0 - u), 4));
                }
            }
            Output::Mesh(wood)
        });

    basic!(_r, "trees-tree-stumps", "trees", "tree-stumps", "Tree stumps", "ground",
        [P::num("radius", 0.35, 0.05, 2.0).unit("m").describe("Stump radius (m)"), P::num("height", 0.45, 0.05, 2.0).unit("m").describe("Stump height (m)"), P::color("color", BARK).describe("Bark colour"), P::color("cut", [0.55, 0.4, 0.24]).describe("Cut wood colour")],
        |v: &Values, s: u32| {
            let (r, h) = (v.f("radius"), v.f("height"));
            let mut bark = geo::lathe(&[[r * 1.35, 0.0], [r * 1.08, h * 0.25], [r, h * 0.6], [r * 0.98, h]], 14);
            for i in 0..5u32 {
                let a = i as f32 / 5.0 * sh::TAU + sh::rnd(s, i, 1);
                let p = sh::path([a.cos() * r * 0.8, h * 0.3, a.sin() * r * 0.8], [a.cos(), -0.35, a.sin()], r * 1.6, 5, [0.0; 3], r * 0.1, s.wrapping_add(i));
                let p: Vec<[f32; 3]> = p.into_iter().map(|q| [q[0], q[1].max(0.0), q[2]]).collect();
                geo::merge(&mut bark, &sh::tube(&p, |t| r * 0.22 * (1.0 - t * 0.85), 6));
            }
            let top = sh::disc(r * 0.98, h, 14, true);
            Output::Parts(vec![Part::new(bark, v.color("color")), Part::new(top, v.color("cut"))])
        });

    basic!(_r, "trees-fallen-trees", "trees", "fallen-trees", "Fallen trees", "ground",
        [P::num("length", 6.0, 1.0, 30.0).unit("m").describe("Log length (m)"), P::num("radius", 0.3, 0.05, 1.5).unit("m").describe("Log radius (m)"), P::color("color", [0.22, 0.15, 0.09]).describe("Bark colour"), P::color("cut", [0.5, 0.36, 0.22]).describe("Broken end colour")],
        |v: &Values, s: u32| {
            let (l, r) = (v.f("length"), v.f("radius"));
            let p = sh::path([-l * 0.5, r, 0.0], [1.0, 0.0, 0.0], l, 8, [0.0; 3], r * 0.3, s);
            let p: Vec<[f32; 3]> = p.into_iter().map(|q| [q[0], r, q[2]]).collect();
            let mut bark = sh::tube(&p, |t| r * (1.0 - 0.3 * t), 10);
            // Stubs of broken branches.
            for i in 0..4u32 {
                let x = sh::range(s, i, 1, -0.3, 0.4) * l;
                let a = sh::range(s, i, 2, -1.2, 1.2);
                let q = sh::path([x, r, 0.0], [0.3, a.cos(), a.sin()], r * 2.0, 3, [0.0; 3], 0.0, s);
                geo::merge(&mut bark, &sh::tube(&q, |t| r * 0.25 * (1.0 - 0.6 * t), 5));
            }
            let mut end = sh::disc(r * 0.98, 0.0, 10, true);
            sh::rotate(&mut end, [0.0, 0.0, 1.0], sh::PI * 0.5);
            let end = sh::place(&end, 1.0, 0.0, 0.0, [-l * 0.5 + 0.001, r, p.first().map_or(0.0, |q| q[2])]);
            Output::Parts(vec![Part::new(bark, v.color("color")), Part::new(end, v.color("cut"))])
        });

    basic!(_r, "trees-exposed-root-systems", "trees", "exposed-root-systems", "Exposed root systems", "ground",
        [P::num("radius", 0.4, 0.1, 2.0).unit("m").describe("Trunk radius (m)"), P::num("roots", 9.0, 3.0, 24.0).step(1.0).describe("Roots"), P::color("color", [0.24, 0.16, 0.1]).describe("Root colour")],
        |v: &Values, s: u32| {
            let (r, n) = (v.f("radius"), v.f("roots") as u32);
            let mut m = sh::frustum(r * 1.1, r, r * 5.0, 12);
            for i in 0..n {
                let a = i as f32 / n as f32 * sh::TAU + sh::rnd(s, i, 1) * 0.4;
                let reach = r * sh::range(s, i, 2, 3.0, 5.0);
                // Arching out of the trunk's foot and down into the ground.
                let pts: Vec<[f32; 3]> = (0..=8).map(|j| {
                    let t = j as f32 / 8.0;
                    let d = r * 0.8 + t * reach;
                    [a.cos() * d, r * 1.2 * (1.0 - t) + r * 0.5 * (t * sh::PI).sin() * (1.0 - t), a.sin() * d]
                }).collect();
                geo::merge(&mut m, &sh::tube(&pts, |t| r * 0.28 * (1.0 - 0.85 * t), 6));
            }
            Output::Mesh(m)
        });

    basic!(_r, "trees-branch-structures", "trees", "branch-structures", "Branch structures", "ground",
        [P::num("length", 2.0, 0.3, 10.0).unit("m").describe("Main branch length (m)"), P::num("levels", 3.0, 1.0, 4.0).step(1.0).describe("Levels of forking"), P::color("color", [0.26, 0.18, 0.11]).describe("Wood colour")],
        |v: &Values, s: u32| {
            let (l, levels) = (v.f("length"), v.f("levels") as u32);
            let mut m = geo::Mesh::default();
            // Each level forks every tip in two, shorter and thinner.
            let mut tips: Vec<([f32; 3], [f32; 3], f32, f32)> = vec![([0.0; 3], [0.0, 1.0, 0.0], l, l * 0.04)];
            for lv in 0..levels {
                let mut next = Vec::new();
                for (k, (at, dir, len, r)) in tips.iter().enumerate() {
                    let p = sh::path(*at, *dir, *len, 5, [0.0; 3], len * 0.06, s.wrapping_add(lv * 97 + k as u32));
                    geo::merge(&mut m, &sh::tube(&p, |t| r * (1.0 - 0.35 * t), 6));
                    let end = p.last().copied().unwrap_or(*at);
                    for side in [-1.0f32, 1.0] {
                        let a = sh::range(s, lv * 31 + k as u32 * 2 + (side > 0.0) as u32, 1, 0.0, sh::TAU);
                        let spread = 0.6 * side;
                        let d = [dir[0] + spread * a.cos(), dir[1], dir[2] + spread * a.sin()];
                        next.push((end, d, len * 0.65, r * 0.62));
                    }
                }
                tips = next;
            }
            Output::Mesh(m)
        });

    basic!(_r, "trees-tree-canopies", "trees", "tree-canopies", "Tree canopies", "grass",
        [P::num("width", 10.0, 2.0, 60.0).unit("m").describe("Canopy width (m)"), P::num("height", 6.0, 1.0, 40.0).unit("m").describe("Canopy height over the ground (m)"), P::color("color", [0.07, 0.19, 0.04]).describe("Leaf colour"), P::color("bark", BARK).describe("Trunk colour")],
        |v: &Values, s: u32| {
            let (w, h) = (v.f("width"), v.f("height"));
            let leaves = sh::crown([w * 0.5, h * 0.18, w * 0.5], [0.0, h, 0.0], 16, s);
            let mut wood = geo::Mesh::default();
            for (i, p) in sh::scatter_disc(4, w * 0.25, s).into_iter().enumerate() {
                let (t, _) = sh::trunk(h, h * 0.03, h * 0.02, h * 0.04, s.wrapping_add(i as u32), 8);
                geo::merge(&mut wood, &sh::place(&t, 1.0, 0.0, 0.0, [p[0], 0.0, p[1]]));
            }
            Output::Parts(vec![Part::new(wood, v.color("bark")).material("ground"), Part::new(leaves, v.color("color"))])
        });

    basic!(_r, "trees-hedgerows", "trees", "hedgerows", "Hedgerows", "grass",
        [P::num("length", 8.0, 1.0, 60.0).unit("m").describe("Hedge length (m)"), P::num("height", 1.6, 0.4, 5.0).unit("m").describe("Hedge height (m)"), P::color("color", [0.08, 0.2, 0.04]).describe("Leaf colour")],
        |v: &Values, s: u32| {
            let (l, h) = (v.f("length"), v.f("height"));
            let n = (l / (h * 0.6)).ceil().max(1.0) as u32;
            let mut m = geo::Mesh::default();
            for i in 0..n {
                let x = -l * 0.5 + (i as f32 + 0.5) * l / n as f32;
                geo::merge(&mut m, &sh::crown([h * 0.5, h * 0.5, h * 0.4], [x, h * 0.5, 0.0], 4, s.wrapping_add(i * 13)));
            }
            sh::to_ground(&mut m);
            Output::Mesh(m)
        });

    basic!(_r, "trees-dense-forest-clusters", "trees", "dense-forest-clusters", "Dense forest clusters", "grass",
        [P::num("radius", 10.0, 2.0, 60.0).unit("m").describe("Radius of the cluster (m)"), P::num("trees", 24.0, 3.0, 80.0).step(1.0).describe("Trees"), P::color("color", [0.06, 0.17, 0.05]).describe("Foliage colour"), P::color("bark", BARK).describe("Bark colour")],
        |v: &Values, s: u32| {
            let r = v.f("radius");
            let (mut wood, mut leaves) = (geo::Mesh::default(), geo::Mesh::default());
            for (i, p) in sh::scatter_disc(v.f("trees") as u32, r, s).into_iter().enumerate() {
                let i = i as u32;
                // Taller in the middle; a mix of conifers and broadleaves.
                let h = (r * 0.9) * sh::range(s, i, 1, 0.55, 1.0) * (1.0 - 0.35 * (p[0] * p[0] + p[1] * p[1]).sqrt() / r);
                let (t, _) = sh::trunk(h * 0.7, h * 0.03, h * 0.015, 0.0, s.wrapping_add(i), 6);
                geo::merge(&mut wood, &sh::place(&t, 1.0, 0.0, 0.0, [p[0], 0.0, p[1]]));
                let crown = if i.is_multiple_of(2) {
                    sh::place(&sh::frustum(h * 0.2, 0.0, h * 0.75, 10), 1.0, 0.0, 0.0, [p[0], h * 0.25, p[1]])
                } else {
                    sh::crown([h * 0.2, h * 0.25, h * 0.2], [p[0], h * 0.7, p[1]], 3, s.wrapping_add(i * 3))
                };
                geo::merge(&mut leaves, &crown);
            }
            Output::Parts(vec![Part::new(wood, v.color("bark")).material("ground"), Part::new(leaves, v.color("color"))])
        });
}
