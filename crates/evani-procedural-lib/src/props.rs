//! Category 12, Environmental props and clutter: basic generators (batch B).
//! Lathed vessels (barrels, bottles, cans, pots, baskets, lanterns),
//! framed boxes (crates, pallets, books), lumpy blobs (sacks, bags),
//! scattered flat pieces (papers, shards) and heaped rubble, each placed
//! and nudged by the seed. Inspired by three.js (MIT) LatheGeometry,
//! BoxGeometry and IcosahedronGeometry; written fresh here.

use evani_procedural::Registry;

#[allow(unused_imports, dead_code)]
pub fn register(_r: &mut Registry) {
    use evani_procedural::{Output, ParamSpec as P, Part, Values, build as b, geo, shapes as sh};
    const IRON: [f32; 3] = [0.12, 0.12, 0.13];

    fn moved(mut m: geo::Mesh, d: [f32; 3]) -> geo::Mesh {
        geo::transform(&mut m, [1.0; 3], d);
        m
    }

    /// A crate `s` a side: a plank box with a frame of battens on its edges.
    fn crate_box(s: f32) -> geo::Mesh {
        let t = s * 0.07;
        let mut m = sh::block([s - t, s - t, s - t], [0.0, t * 0.5, 0.0]);
        let h = s * 0.5;
        for a in [-1.0f32, 1.0] {
            for c in [-1.0f32, 1.0] {
                geo::merge(&mut m, &b::cbox([s, t, t], [0.0, h + a * (h - t * 0.5), c * (h - t * 0.5)]));
                geo::merge(&mut m, &b::cbox([t, s, t], [a * (h - t * 0.5), h, c * (h - t * 0.5)]));
                geo::merge(&mut m, &b::cbox([t, t, s], [a * (h - t * 0.5), h + c * (h - t * 0.5), 0.0]));
            }
        }
        // A diagonal brace on each side face.
        for (n, axis) in [(1.0f32, 0usize), (-1.0, 0), (1.0, 2), (-1.0, 2)] {
            let (a, c) = if axis == 0 { ([n * h, t, -h + t], [n * h, s - t, h - t]) } else { ([-h + t, t, n * h], [h - t, s - t, n * h]) };
            geo::merge(&mut m, &b::bar(a, c, t, t * 0.6));
        }
        m
    }

    /// A tied sack or rubbish bag: a squashed lumpy ball and a gathered neck.
    fn sack(s: f32, full: f32, seed: u32) -> geo::Mesh {
        let h = s * (0.6 + 0.5 * full);
        let mut m = sh::blob([s * 0.42, h * 0.5, s * 0.36], [0.0, h * 0.5, 0.0], 0.35, seed, 2);
        sh::to_ground(&mut m);
        let top = geo::bounds(&m).1[1];
        geo::merge(&mut m, &moved(sh::frustum(s * 0.1, s * 0.03, s * 0.12, 10), [0.0, top - s * 0.04, 0.0]));
        geo::merge(&mut m, &moved(sh::frustum(s * 0.03, s * 0.08, s * 0.06, 10), [0.0, top + s * 0.07, 0.0]));
        m
    }

    basic!(_r, "props-wooden-crates", "props", "wooden-crates", "Wooden crates", "ground",
        [P::num("size", 0.6, 0.2, 1.5).unit("m").describe("Crate size (m)"), P::num("count", 3.0, 1.0, 8.0).step(1.0).describe("Crates in the stack"), P::color("color", [0.48, 0.34, 0.2]).describe("Wood colour")],
        |v: &Values, s: u32| {
            let (k, n) = (v.f("size"), v.f("count").round() as u32);
            let one = crate_box(k);
            let mut m = geo::Mesh::default();
            // Two on the ground per row, the rest stacked, each turned a little.
            for i in 0..n {
                let (layer, slot) = (i / 2, i % 2);
                let x = if layer == 0 { (slot as f32 - 0.5) * k * 1.05 } else { (slot as f32 - 0.5) * k * 0.55 * (1.0 - layer as f32 * 0.3) };
                let yaw = sh::range(s, i, 1, -0.2, 0.2);
                geo::merge(&mut m, &sh::place(&one, 1.0, yaw, 0.0, [x, layer as f32 * k, sh::range(s, i, 2, -0.05, 0.05) * k]));
            }
            Output::Mesh(m)
        });

    basic!(_r, "props-barrels", "props", "barrels", "Barrels", "ground",
        [P::num("height", 0.9, 0.3, 1.6).unit("m").describe("Height (m)"), P::num("bulge", 0.12, 0.0, 0.3).describe("Bulge of the staves"), P::color("color", [0.4, 0.24, 0.12]).describe("Stave colour")],
        |v: &Values, _s: u32| {
            let (h, bg) = (v.f("height"), v.f("bulge"));
            let r = h * 0.32;
            let prof: Vec<[f32; 2]> = (0..=12).map(|i| {
                let t = i as f32 / 12.0;
                [r * (1.0 + bg * (t * sh::PI).sin()), t * h]
            }).collect();
            let mut wood = b::revolve(&[[0.0, 0.0], [r, 0.0]], 24);
            geo::merge(&mut wood, &b::revolve(&prof, 24));
            geo::merge(&mut wood, &b::revolve(&[[r, h], [r * 0.95, h], [r * 0.95, h - 0.03], [0.0, h - 0.03]], 24));
            // Iron hoops round the barrel at four heights.
            let mut hoops = geo::Mesh::default();
            for t in [0.08f32, 0.3, 0.7, 0.92] {
                let rr = r * (1.0 + bg * (t * sh::PI).sin()) + 0.004;
                geo::merge(&mut hoops, &b::revolve(&[[rr - 0.006, t * h - 0.02], [rr, t * h - 0.02], [rr, t * h + 0.02], [rr - 0.006, t * h + 0.02]], 24));
            }
            Output::Parts(vec![Part::new(wood, v.color("color")), Part::new(hoops, IRON).material("metal")])
        });

    basic!(_r, "props-sacks", "props", "sacks", "Sacks", "ground",
        [P::num("size", 0.6, 0.2, 1.2).unit("m").describe("Sack size (m)"), P::num("fullness", 0.7, 0.0, 1.0).describe("How full"), P::num("count", 3.0, 1.0, 8.0).step(1.0).describe("Sacks"), P::color("color", [0.5, 0.42, 0.28]).describe("Hessian colour")],
        |v: &Values, s: u32| {
            let (k, f, n) = (v.f("size"), v.f("fullness"), v.f("count").round() as u32);
            let mut m = geo::Mesh::default();
            for (i, p) in sh::scatter_disc(n, k * 0.45 * (n as f32).sqrt(), s).into_iter().enumerate() {
                let one = sack(k * sh::range(s, i as u32, 1, 0.85, 1.1), f, s.wrapping_add(i as u32 * 7));
                geo::merge(&mut m, &sh::place(&one, 1.0, sh::rnd(s, i as u32, 2) * sh::TAU, 0.0, [p[0], 0.0, p[1]]));
            }
            Output::Mesh(m)
        });

    basic!(_r, "props-baskets", "props", "baskets", "Baskets", "ground",
        [P::num("radius", 0.22, 0.08, 0.6).unit("m").describe("Radius (m)"), P::num("height", 0.25, 0.08, 0.6).unit("m").describe("Height (m)"), P::color("color", [0.5, 0.36, 0.2]).describe("Wicker colour")],
        |v: &Values, _s: u32| {
            let (r, h) = (v.f("radius"), v.f("height"));
            let t = 0.012;
            let mut m = b::revolve(&[[0.0, 0.0], [r * 0.75, 0.0], [r, h], [r - t, h], [r * 0.75 - t, t], [0.0, t]], 28);
            // Woven rows as rings round the outside, a rim and a hoop handle.
            let rows = (h / 0.03) as u32;
            for i in 0..rows {
                let y = (i as f32 + 0.5) / rows as f32 * h;
                let rr = r * 0.75 + (r * 0.25) * y / h;
                let ring: Vec<[f32; 2]> = (0..=8).map(|k| {
                    let a = -sh::PI * 0.5 + k as f32 / 8.0 * sh::TAU;
                    [rr + 0.006 * a.cos(), y + 0.008 * a.sin()]
                }).collect();
                geo::merge(&mut m, &b::revolve(&ring, 28));
            }
            let handle: Vec<[f32; 3]> = (0..=16).map(|k| {
                let a = sh::PI * k as f32 / 16.0;
                [r * 0.95 * a.cos(), h + r * 0.8 * a.sin(), 0.0]
            }).collect();
            geo::merge(&mut m, &b::sweep(&handle, &b::circle(0.012, 8), true));
            Output::Mesh(m)
        });

    basic!(_r, "props-bottles", "props", "bottles", "Bottles", "glass",
        [P::num("height", 0.3, 0.1, 0.6).unit("m").describe("Height (m)"), P::num("count", 4.0, 1.0, 12.0).step(1.0).describe("Bottles"), P::color("color", [0.08, 0.25, 0.1]).describe("Glass colour")],
        |v: &Values, s: u32| {
            let (h, n) = (v.f("height"), v.f("count").round() as u32);
            let mut glass = geo::Mesh::default();
            let mut caps = geo::Mesh::default();
            for (i, p) in sh::scatter_disc(n, h * 0.35 * (n as f32).sqrt(), s).into_iter().enumerate() {
                let k = h * sh::range(s, i as u32, 1, 0.8, 1.1);
                let r = k * 0.13;
                // Body, shoulder and neck in one profile.
                let prof = [[0.0, 0.0], [r, 0.0], [r, k * 0.6], [r * 0.85, k * 0.7], [r * 0.32, k * 0.8], [r * 0.3, k * 0.97], [0.0, k * 0.97]];
                geo::merge(&mut glass, &moved(b::revolve(&prof, 18), [p[0], 0.0, p[1]]));
                geo::merge(&mut caps, &moved(sh::frustum(r * 0.34, r * 0.34, k * 0.05, 12), [p[0], k * 0.96, p[1]]));
            }
            Output::Parts(vec![Part::new(glass, v.color("color")).alpha(0.85).material("glass"), Part::new(caps, [0.5, 0.42, 0.2]).material("metal")])
        });

    basic!(_r, "props-metal-cans", "props", "metal-cans", "Metal cans", "metal",
        [P::num("height", 0.12, 0.05, 0.5).unit("m").describe("Can height (m)"), P::num("count", 6.0, 1.0, 20.0).step(1.0).describe("Cans"), P::color("color", [0.45, 0.08, 0.06]).describe("Label colour")],
        |v: &Values, s: u32| {
            let (h, n) = (v.f("height"), v.f("count").round() as u32);
            let r = h * 0.33;
            let (mut tins, mut labels) = (geo::Mesh::default(), geo::Mesh::default());
            // A pyramid stack: rows of fewer cans going up; leftovers knocked over.
            let mut left = n;
            let mut row = ((n as f32 * 2.0).sqrt().floor() as u32).max(1);
            let mut y = 0.0;
            let mut k = 0u32;
            while left > 0 && row > 0 {
                let here = row.min(left);
                for i in 0..here {
                    let x = (i as f32 - (here - 1) as f32 * 0.5) * r * 2.05;
                    let at = [x, y, sh::range(s, k, 1, -0.01, 0.01)];
                    geo::merge(&mut tins, &moved(b::revolve(&[[0.0, 0.0], [r * 0.93, 0.0], [r, 0.01], [r, h - 0.01], [r * 0.93, h], [0.0, h]], 20), at));
                    geo::merge(&mut labels, &moved(b::revolve(&[[r + 0.001, h * 0.15], [r + 0.001, h * 0.85]], 20), at));
                    k += 1;
                }
                left -= here;
                row -= 1;
                y += h;
            }
            Output::Parts(vec![Part::new(tins, [0.55, 0.56, 0.58]).material("metal"), Part::new(labels, v.color("color"))])
        });

    basic!(_r, "props-broken-glass", "props", "broken-glass", "Broken glass", "glass",
        [P::num("count", 30.0, 3.0, 200.0).step(1.0).describe("Shards"), P::num("spread", 0.6, 0.1, 3.0).unit("m").describe("Radius they lie in (m)"), P::num("size", 0.06, 0.01, 0.3).unit("m").describe("Typical shard size (m)"), P::color("color", [0.55, 0.65, 0.65]).describe("Glass colour")],
        |v: &Values, s: u32| {
            let (n, sp, k) = (v.f("count").round() as u32, v.f("spread"), v.f("size"));
            let mut m = geo::Mesh::default();
            for (i, p) in sh::scatter_disc(n, sp, s).into_iter().enumerate() {
                let i = i as u32;
                // A thin seeded triangle, slightly tilted where it fell.
                let tri: Vec<[f32; 2]> = (0..3).map(|j| {
                    let a = j as f32 / 3.0 * sh::TAU + sh::range(s, i * 3 + j, 1, -0.6, 0.6);
                    let r = k * sh::range(s, i * 3 + j, 2, 0.4, 1.0);
                    [r * a.cos(), r * a.sin()]
                }).collect();
                let shard = b::prism(&tri, 0.0, 0.004);
                geo::merge(&mut m, &sh::place(&shard, 1.0, sh::rnd(s, i, 3) * sh::TAU, sh::range(s, i, 4, -0.15, 0.15), [p[0], 0.004 + 0.003 * (i % 3) as f32, p[1]]));
            }
            Output::Parts(vec![Part::new(m, v.color("color")).alpha(0.6).material("glass")])
        });

    basic!(_r, "props-scattered-papers", "props", "scattered-papers", "Scattered papers", "ground",
        [P::num("count", 12.0, 1.0, 60.0).step(1.0).describe("Sheets"), P::num("spread", 0.8, 0.1, 4.0).unit("m").describe("Radius they lie in (m)"), P::num("curl", 0.02, 0.0, 0.08).unit("m").describe("How far the sheets curl up (m)"), P::color("color", [0.85, 0.84, 0.8]).describe("Paper colour")],
        |v: &Values, s: u32| {
            let (n, sp, c) = (v.f("count").round() as u32, v.f("spread"), v.f("curl"));
            let mut m = geo::Mesh::default();
            for (i, p) in sh::scatter_disc(n, sp, s).into_iter().enumerate() {
                let i = i as u32;
                // An A4 sheet, its corners lifted by the curl.
                let mut sheet = geo::height_grid([0.21, 0.297], 6, |x, z| c * ((x / 0.105).powi(2) * 0.5 + (z / 0.15).powi(4) * 0.5) * sh::range(s, i, 5, 0.3, 1.0));
                sh::double_sided(&mut sheet);
                let y = 0.003 + 0.002 * i as f32;
                geo::merge(&mut m, &sh::place(&sheet, 1.0, sh::rnd(s, i, 3) * sh::TAU, 0.0, [p[0], y, p[1]]));
            }
            Output::Mesh(m)
        });

    basic!(_r, "props-books", "props", "books", "Books", "ground",
        [P::num("count", 8.0, 1.0, 30.0).step(1.0).describe("Books"), P::num("height", 0.24, 0.12, 0.4).unit("m").describe("Typical height (m)"), P::color("color", [0.3, 0.1, 0.08]).describe("Cover colour")],
        |v: &Values, s: u32| {
            let (n, h) = (v.f("count").round() as u32, v.f("height"));
            let (mut covers, mut pages) = (geo::Mesh::default(), geo::Mesh::default());
            let mut x = 0.0;
            let mut pile = 0.0;
            // A row standing on end, the last few lying in a little pile.
            let stand = n - n / 4;
            for i in 0..n {
                let (bh, bt) = (h * sh::range(s, i, 1, 0.8, 1.1), sh::range(s, i, 2, 0.02, 0.05));
                let bd = bh * 0.7;
                if i < stand {
                    geo::merge(&mut covers, &sh::block([bt, bh, bd], [x + bt * 0.5, 0.0, 0.0]));
                    geo::merge(&mut pages, &sh::block([bt - 0.004, bh - 0.008, bd - 0.006], [x + bt * 0.5, 0.004, 0.004]));
                    x += bt + 0.002;
                } else {
                    let yaw = sh::range(s, i, 3, -0.3, 0.3);
                    let at = [x + h * 0.7, pile, 0.0];
                    geo::merge(&mut covers, &sh::place(&sh::block([bh, bt, bd], [0.0; 3]), 1.0, yaw, 0.0, at));
                    geo::merge(&mut pages, &sh::place(&sh::block([bh - 0.008, bt - 0.004, bd - 0.006], [0.004, 0.002, 0.0]), 1.0, yaw, 0.0, at));
                    pile += bt;
                }
            }
            Output::Parts(vec![Part::new(covers, v.color("color")), Part::new(pages, [0.85, 0.82, 0.74])])
        });

    basic!(_r, "props-signs", "props", "signs", "Signs", "ground",
        [P::num("height", 2.2, 1.0, 4.0).unit("m").describe("Post height (m)"), P::num("boards", 3.0, 1.0, 5.0).step(1.0).describe("Arrow boards"), P::color("color", [0.42, 0.3, 0.18]).describe("Wood colour")],
        |v: &Values, s: u32| {
            let (h, n) = (v.f("height"), v.f("boards").round() as u32);
            let mut post = b::post(0.0, 0.0, 0.1, h);
            geo::merge(&mut post, &moved(b::hip(0.12, 0.12, 0.08, 0.0, 0.0), [0.0, h, 0.0]));
            let mut boards = geo::Mesh::default();
            for i in 0..n {
                let (l, bh) = (sh::range(s, i, 1, 0.6, 0.9), 0.16);
                // An arrow board pointing its own seeded way.
                let arrow = b::extrude(&[[0.06, -bh * 0.5], [l - 0.12, -bh * 0.5], [l, 0.0], [l - 0.12, bh * 0.5], [0.06, bh * 0.5]], 0.03);
                let yaw = sh::range(s, i, 2, 0.0, sh::TAU);
                geo::merge(&mut boards, &sh::place(&arrow, 1.0, yaw, 0.0, [0.0, h - 0.15 - i as f32 * 0.22, 0.0]));
            }
            Output::Parts(vec![Part::new(post, v.color("color")), Part::new(boards, [0.7, 0.62, 0.48])])
        });

    basic!(_r, "props-lanterns", "props", "lanterns", "Lanterns", "metal",
        [P::num("height", 0.4, 0.15, 1.2).unit("m").describe("Height (m)"), P::num("bars", 4.0, 3.0, 8.0).step(1.0).describe("Cage bars"), P::color("color", [0.1, 0.1, 0.1]).describe("Iron colour"), P::color("glow", [1.0, 0.65, 0.25]).describe("Flame colour")],
        |v: &Values, _s: u32| {
            let (h, n) = (v.f("height"), v.f("bars").round() as u32);
            let r = h * 0.22;
            let mut iron = b::revolve(&[[0.0, 0.0], [r * 1.1, 0.0], [r * 1.1, h * 0.08], [0.0, h * 0.08]], 16);
            geo::merge(&mut iron, &moved(sh::frustum(r * 1.15, r * 0.25, h * 0.2, 16), [0.0, h * 0.72, 0.0]));
            for i in 0..n {
                let a = i as f32 / n as f32 * sh::TAU;
                geo::merge(&mut iron, &b::rod([r * a.cos(), h * 0.08, r * a.sin()], [r * a.cos(), h * 0.72, r * a.sin()], h * 0.012, 6));
            }
            let ring: Vec<[f32; 3]> = (0..=12).map(|k| {
                let a = sh::PI * k as f32 / 12.0;
                [r * 0.4 * a.cos(), h * 0.92 + r * 0.4 * a.sin(), 0.0]
            }).collect();
            geo::merge(&mut iron, &b::sweep(&ring, &b::circle(h * 0.012, 6), true));
            let glass = moved(sh::frustum(r * 0.92, r * 0.92, h * 0.62, 16), [0.0, h * 0.09, 0.0]);
            let flame = moved(b::revolve(&[[0.0, 0.0], [r * 0.18, h * 0.06], [0.0, h * 0.22]], 10), [0.0, h * 0.2, 0.0]);
            Output::Parts(vec![Part::new(iron, v.color("color")), Part::new(glass, [0.9, 0.85, 0.7]).alpha(0.35).material("glass"), Part::new(flame, v.color("glow")).material("glow")])
        });

    basic!(_r, "props-cooking-pots", "props", "cooking-pots", "Cooking pots", "metal",
        [P::num("radius", 0.18, 0.06, 0.6).unit("m").describe("Radius (m)"), P::num("height", 0.18, 0.05, 0.6).unit("m").describe("Height (m)"), P::color("color", [0.12, 0.12, 0.12]).describe("Pot colour")],
        |v: &Values, _s: u32| {
            let (r, h) = (v.f("radius"), v.f("height"));
            let t = r * 0.04;
            // A walled pot (open), a rolled rim, two loop handles, a lid on top.
            let mut m = b::revolve(&[[0.0, 0.0], [r * 0.9, 0.0], [r, h * 0.12], [r, h], [r + t, h], [r + t, h + t], [r - t, h + t], [r - t, h * 0.15], [r * 0.9 - t, t], [0.0, t]], 32);
            for side in [-1.0f32, 1.0] {
                let loop_path: Vec<[f32; 3]> = (0..=10).map(|k| {
                    let a = sh::PI * k as f32 / 10.0 - sh::PI * 0.5;
                    [side * (r + r * 0.12 * a.cos()), h * 0.8 + r * 0.1 * a.sin(), 0.0]
                }).collect();
                geo::merge(&mut m, &b::sweep(&loop_path, &b::circle(t * 0.8, 8), true));
            }
            let mut lid = b::revolve(&[[0.0, h + t], [r + t * 0.5, h + t], [r * 0.9, h + t + r * 0.12], [0.0, h + t + r * 0.16]], 32);
            geo::merge(&mut lid, &b::revolve(&[[0.0, h + t + r * 0.15], [r * 0.12, h + t + r * 0.15], [r * 0.12, h + t + r * 0.25], [0.0, h + t + r * 0.25]], 12));
            Output::Parts(vec![Part::new(m, v.color("color")).material("metal"), Part::new(lid, [0.16, 0.16, 0.17]).material("metal")])
        });

    basic!(_r, "props-rubble-piles", "props", "rubble-piles", "Rubble piles", "stone",
        [P::num("size", 1.5, 0.4, 6.0).unit("m").describe("Pile width (m)"), P::num("pieces", 60.0, 10.0, 300.0).step(1.0).describe("Pieces"), P::color("color", [0.42, 0.4, 0.37]).describe("Rubble colour")],
        |v: &Values, s: u32| {
            let (k, n) = (v.f("size"), v.f("pieces").round() as u32);
            let (mut stone, mut brick) = (geo::Mesh::default(), geo::Mesh::default());
            // Pieces heaped in a cone: nearer the middle, higher up.
            for i in 0..n {
                let a = sh::rnd(s, i, 1) * sh::TAU;
                let d = sh::rnd(s, i, 2).sqrt() * k * 0.5;
                let y = (1.0 - d / (k * 0.5)) * k * 0.3 * sh::range(s, i, 3, 0.6, 1.0);
                let at = [d * a.cos(), y, d * a.sin()];
                let q = k * sh::range(s, i, 4, 0.05, 0.12);
                if i % 3 == 0 {
                    let br = b::cbox([0.215, 0.065, 0.1], [0.0; 3]);
                    geo::merge(&mut brick, &sh::place(&br, q / 0.12, sh::rnd(s, i, 5) * sh::TAU, sh::range(s, i, 6, -0.6, 0.6), at));
                } else {
                    let r = sh::rock([q, q * 0.6, q * 0.8], s.wrapping_add(i % 11), 0.7, true);
                    geo::merge(&mut stone, &sh::place(&r, 1.0, sh::rnd(s, i, 5) * sh::TAU, 0.0, at));
                }
            }
            Output::Parts(vec![Part::new(stone, v.color("color")), Part::new(brick, [0.4, 0.17, 0.1])])
        });

    basic!(_r, "props-rubbish-bags", "props", "rubbish-bags", "Rubbish bags", "ground",
        [P::num("count", 4.0, 1.0, 12.0).step(1.0).describe("Bags"), P::num("size", 0.6, 0.3, 1.0).unit("m").describe("Bag size (m)"), P::color("color", [0.03, 0.03, 0.035]).describe("Bag colour")],
        |v: &Values, s: u32| {
            let (n, k) = (v.f("count").round() as u32, v.f("size"));
            let mut m = geo::Mesh::default();
            for (i, p) in sh::scatter_disc(n, k * 0.4 * (n as f32).sqrt(), s).into_iter().enumerate() {
                let one = sack(k * sh::range(s, i as u32, 1, 0.8, 1.1), 0.9, s.wrapping_add(i as u32 * 13));
                geo::merge(&mut m, &sh::place(&one, 1.0, sh::rnd(s, i as u32, 2) * sh::TAU, sh::range(s, i as u32, 3, -0.2, 0.2), [p[0], 0.0, p[1]]));
            }
            Output::Mesh(m)
        });

    basic!(_r, "props-wooden-pallets", "props", "wooden-pallets", "Wooden pallets", "ground",
        [P::num("stack", 3.0, 1.0, 10.0).step(1.0).describe("Pallets in the stack"), P::num("width", 1.2, 0.8, 1.4).unit("m").describe("Length (m)"), P::color("color", [0.6, 0.48, 0.32]).describe("Wood colour")],
        |v: &Values, s: u32| {
            let (n, w) = (v.f("stack").round() as u32, v.f("width"));
            let d = 0.8;
            // One pallet: bottom boards, nine blocks, stringer boards, a deck.
            let mut one = geo::Mesh::default();
            for z in [-d * 0.5 + 0.05, 0.0, d * 0.5 - 0.05] {
                geo::merge(&mut one, &b::cbox([w, 0.022, 0.1], [0.0, 0.011, z]));
                for x in [-w * 0.5 + 0.05, 0.0, w * 0.5 - 0.05] {
                    geo::merge(&mut one, &b::cbox([0.1, 0.078, 0.1], [x, 0.061, z]));
                }
            }
            for x in [-w * 0.5 + 0.05, 0.0, w * 0.5 - 0.05] {
                geo::merge(&mut one, &b::cbox([0.1, 0.022, d], [x, 0.111, 0.0]));
            }
            for i in 0..7 {
                let x = (i as f32 / 6.0 - 0.5) * (w - 0.1);
                geo::merge(&mut one, &b::cbox([0.1, 0.022, d], [x, 0.133, 0.0]));
            }
            let ph = 0.144;
            let mut m = geo::Mesh::default();
            for i in 0..n {
                geo::merge(&mut m, &sh::place(&one, 1.0, sh::range(s, i, 1, -0.06, 0.06), 0.0, [sh::range(s, i, 2, -0.03, 0.03), i as f32 * ph, 0.0]));
            }
            Output::Mesh(m)
        });
}
