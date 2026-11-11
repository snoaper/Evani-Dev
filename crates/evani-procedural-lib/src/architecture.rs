//! Category 10, Architectural components: basic generators (batch B). Single
//! building parts from boxes laid in courses (bricks, boards, tiles),
//! profiles swept along their run (I-beams, gutters, rails) or extruded
//! (pickets, steps), and a lathe for column bases and capitals; doors and
//! gates swing on their hinge. The front faces +z. Inspired by three.js
//! (MIT) BoxGeometry, ExtrudeGeometry, LatheGeometry and TubeGeometry;
//! written fresh here.

use evani_procedural::Registry;

#[allow(unused_imports, dead_code)]
pub fn register(_r: &mut Registry) {
    use evani_procedural::{Output, ParamSpec as P, Part, Values, build as b, geo, shapes as sh};
    const WOOD: [f32; 3] = [0.38, 0.25, 0.14];
    const STEEL: [f32; 3] = [0.24, 0.24, 0.25];
    const CONCRETE: [f32; 3] = [0.52, 0.51, 0.48];
    const GLASS: [f32; 3] = [0.12, 0.16, 0.2];

    fn moved(mut m: geo::Mesh, d: [f32; 3]) -> geo::Mesh {
        geo::transform(&mut m, [1.0; 3], d);
        m
    }

    /// `m` swung by `angle` about a vertical hinge at x = `hx`, z = `hz`.
    fn swung(mut m: geo::Mesh, hx: f32, hz: f32, angle: f32) -> geo::Mesh {
        geo::transform(&mut m, [1.0; 3], [-hx, 0.0, -hz]);
        sh::rotate(&mut m, [0.0, 1.0, 0.0], angle);
        moved(m, [hx, 0.0, hz])
    }

    /// An I-section `h` deep and `w` wide (flanges `t` thick).
    fn i_profile(h: f32, w: f32, t: f32) -> Vec<[f32; 2]> {
        let (a, c, web) = (w * 0.5, h * 0.5, t * 0.5);
        vec![[-a, -c], [a, -c], [a, -c + t], [web, -c + t], [web, c - t], [a, c - t], [a, c], [-a, c], [-a, c - t], [-web, c - t], [-web, -c + t], [-a, -c + t]]
    }

    basic!(_r, "architecture-walls", "architecture", "walls", "Walls", "stone",
        [P::num("length", 4.0, 1.0, 12.0).unit("m").describe("Length (m)"), P::num("height", 2.4, 0.5, 6.0).unit("m").describe("Height (m)"), P::num("thickness", 0.22, 0.1, 0.6).unit("m").describe("Thickness (m)"), P::color("color", [0.45, 0.19, 0.12]).describe("Brick colour")],
        |v: &Values, s: u32| {
            let (l, h, t) = (v.f("length"), v.f("height"), v.f("thickness"));
            let core = sh::block([l - 0.02, h - 0.01, t - 0.02], [0.0; 3]);
            let (bl, bh) = (0.215, 0.065);
            let (rows, cols) = ((h / (bh + 0.01)).floor() as u32, (l / (bl + 0.01)).ceil() as u32 + 1);
            let mut bricks = geo::Mesh::default();
            for j in 0..rows {
                let off = if j % 2 == 0 { 0.0 } else { (bl + 0.01) * 0.5 };
                for i in 0..cols {
                    let x0 = -l * 0.5 + i as f32 * (bl + 0.01) - off;
                    let (a, c) = (x0.max(-l * 0.5), (x0 + bl).min(l * 0.5));
                    if c - a < 0.02 {
                        continue;
                    }
                    let k = j * 131 + i;
                    let depth = t + sh::range(s, k, 1, -0.006, 0.004);
                    geo::merge(&mut bricks, &sh::block([c - a, bh, depth], [(a + c) * 0.5, j as f32 * (bh + 0.01), sh::range(s, k, 2, -0.002, 0.002)]));
                }
            }
            Output::Parts(vec![Part::new(bricks, v.color("color")), Part::new(core, [0.55, 0.53, 0.5])])
        });

    basic!(_r, "architecture-floors", "architecture", "floors", "Floors", "ground",
        [P::num("width", 4.0, 1.0, 12.0).unit("m").describe("Width (m)"), P::num("depth", 3.0, 1.0, 12.0).unit("m").describe("Depth (m)"), P::num("board", 0.14, 0.05, 0.4).unit("m").describe("Board width (m)"), P::color("color", [0.5, 0.33, 0.18]).describe("Board colour")],
        |v: &Values, s: u32| {
            let (w, d, bw) = (v.f("width"), v.f("depth"), v.f("board"));
            let mut joists = geo::Mesh::default();
            let nj = (w / 0.4).floor() as u32 + 1;
            for i in 0..nj {
                let x = (i as f32 / (nj - 1).max(1) as f32 - 0.5) * (w - 0.05);
                geo::merge(&mut joists, &sh::block([0.05, 0.15, d], [x, 0.0, 0.0]));
            }
            // Boards along x in rows, each row broken at seeded joints.
            let mut boards = geo::Mesh::default();
            let rows = (d / bw).floor().max(1.0) as u32;
            for j in 0..rows {
                let z = ((j as f32 + 0.5) / rows as f32 - 0.5) * d;
                let cut = sh::range(s, j, 1, 0.25, 0.75) * w - w * 0.5;
                for (a, c) in [(-w * 0.5, cut), (cut, w * 0.5)] {
                    geo::merge(&mut boards, &sh::block([c - a - 0.004, 0.022 + sh::range(s, j, 2, 0.0, 0.002), d / rows as f32 - 0.004], [(a + c) * 0.5, 0.15, z]));
                }
            }
            Output::Parts(vec![Part::new(boards, v.color("color")), Part::new(joists, [0.42, 0.33, 0.22])])
        });

    basic!(_r, "architecture-roofs", "architecture", "roofs", "Roofs", "stone",
        [P::num("width", 6.0, 2.0, 16.0).unit("m").describe("Width along the ridge (m)"), P::num("depth", 5.0, 2.0, 14.0).unit("m").describe("Depth, eave to eave (m)"), P::num("pitch", 35.0, 10.0, 60.0).unit("deg").describe("Roof pitch (degrees)"), P::color("color", [0.38, 0.15, 0.09]).describe("Tile colour")],
        |v: &Values, _s: u32| {
            let (w, d, p) = (v.f("width"), v.f("depth"), v.f("pitch").to_radians());
            let base = 0.3;
            let h = d * 0.5 * p.tan();
            let deck = b::gable(w, d, h, base, 0.0);
            // Overlapping tile courses up each slope, a ridge cap on top.
            let mut tiles = geo::Mesh::default();
            let slope = d * 0.5 / p.cos();
            let rows = (slope / 0.3).ceil() as u32;
            for side in [-1.0f32, 1.0] {
                for i in 0..rows {
                    let t = (i as f32 + 0.5) / rows as f32;
                    // Each course tipped a little steeper than the roof, so
                    // its lower edge stands proud of the course below.
                    let mut row = b::cbox([w + 0.2, 0.035, slope / rows as f32 + 0.1], [0.0; 3]);
                    sh::rotate(&mut row, [1.0, 0.0, 0.0], side * (p + 0.14));
                    let (z, y) = (side * d * 0.5 * (1.0 - t), base + h * t + 0.05);
                    geo::merge(&mut tiles, &moved(row, [0.0, y, z]));
                }
            }
            geo::merge(&mut tiles, &b::rod([-w * 0.5 - 0.1, base + h + 0.03, 0.0], [w * 0.5 + 0.1, base + h + 0.03, 0.0], 0.09, 10));
            let plate = sh::block([w, base, d], [0.0; 3]);
            Output::Parts(vec![Part::new(tiles, v.color("color")), Part::new(deck, [0.2, 0.15, 0.1]), Part::new(plate, [0.75, 0.73, 0.68])])
        });

    basic!(_r, "architecture-foundations", "architecture", "foundations", "Foundations", "stone",
        [P::num("width", 8.0, 2.0, 30.0).unit("m").describe("Width (m)"), P::num("depth", 6.0, 2.0, 30.0).unit("m").describe("Depth (m)"), P::num("height", 0.6, 0.2, 2.0).unit("m").describe("Wall height above the footing (m)"), P::color("color", CONCRETE).describe("Concrete colour")],
        |v: &Values, _s: u32| {
            let (w, d, h) = (v.f("width"), v.f("depth"), v.f("height"));
            let mut conc = geo::Mesh::default();
            let mut rebar = geo::Mesh::default();
            // A wide footing, a narrower wall on it, round the plan; a slab inside.
            for (len, at, along_x) in [(w, [0.0, -d * 0.5], true), (w, [0.0, d * 0.5], true), (d, [-w * 0.5, 0.0], false), (d, [w * 0.5, 0.0], false)] {
                let (fw, ww) = (0.6, 0.25);
                let (foot, wall) = if along_x { ([len + fw, 0.25, fw], [len + ww, h, ww]) } else { ([fw, 0.25, len + fw], [ww, h, len + ww]) };
                geo::merge(&mut conc, &sh::block(foot, [at[0], 0.0, at[1]]));
                geo::merge(&mut conc, &sh::block(wall, [at[0], 0.25, at[1]]));
                let n = (len / 1.0) as u32;
                for i in 0..=n {
                    let t = (i as f32 / n.max(1) as f32 - 0.5) * len;
                    let (x, z) = if along_x { (at[0] + t, at[1]) } else { (at[0], at[1] + t) };
                    geo::merge(&mut rebar, &b::rod([x, 0.25 + h - 0.05, z], [x, 0.25 + h + 0.45, z], 0.008, 6));
                }
            }
            geo::merge(&mut conc, &sh::block([w - 0.25, 0.12, d - 0.25], [0.0, 0.25 + h - 0.12, 0.0]));
            Output::Parts(vec![Part::new(conc, v.color("color")), Part::new(rebar, [0.33, 0.2, 0.12])])
        });

    basic!(_r, "architecture-doors", "architecture", "doors", "Doors", "ground",
        [P::num("width", 0.9, 0.6, 2.0).unit("m").describe("Leaf width (m)"), P::num("height", 2.1, 1.6, 3.0).unit("m").describe("Leaf height (m)"), P::num("open", 25.0, 0.0, 100.0).unit("deg").describe("How far it stands open (degrees)"), P::color("color", [0.18, 0.28, 0.24]).describe("Door colour")],
        |v: &Values, _s: u32| {
            let (w, h, open) = (v.f("width"), v.f("height"), v.f("open").to_radians());
            let t = 0.045;
            let mut frame = geo::Mesh::default();
            for x in [-w * 0.5 - 0.04, w * 0.5 + 0.04] {
                geo::merge(&mut frame, &sh::block([0.08, h + 0.08, 0.14], [x, 0.0, 0.0]));
            }
            geo::merge(&mut frame, &sh::block([w + 0.16, 0.08, 0.14], [0.0, h, 0.0]));
            // The leaf with four raised panels, a knob, swung on its hinge.
            let mut leaf = sh::block([w, h, t], [0.0; 3]);
            for (px, py) in [(-0.22f32, 0.25f32), (0.22, 0.25), (-0.22, 0.68), (0.22, 0.68)] {
                let (pw, ph) = (w * 0.36, h * 0.33);
                for z in [t * 0.5, -t * 0.5] {
                    geo::merge(&mut leaf, &b::cbox([pw, ph, 0.012], [px * w * 1.1, py * h, z]));
                }
            }
            let mut knob = sh::icosphere(2);
            geo::transform(&mut knob, [0.035; 3], [w * 0.4, h * 0.47, t * 0.5 + 0.05]);
            let mut k2 = b::rod([w * 0.4, h * 0.47, -t * 0.5 - 0.05], [w * 0.4, h * 0.47, t * 0.5 + 0.05], 0.008, 8);
            geo::merge(&mut k2, &knob);
            let leaf = swung(leaf, -w * 0.5, 0.0, open);
            let knob = swung(k2, -w * 0.5, 0.0, open);
            Output::Parts(vec![Part::new(leaf, v.color("color")), Part::new(frame, [0.8, 0.79, 0.75]), Part::new(knob, [0.55, 0.45, 0.2]).material("metal")])
        });

    basic!(_r, "architecture-windows", "architecture", "windows", "Windows", "ground",
        [P::num("width", 1.2, 0.4, 3.0).unit("m").describe("Width (m)"), P::num("height", 1.4, 0.4, 3.0).unit("m").describe("Height (m)"), P::num("panes", 2.0, 1.0, 6.0).step(1.0).describe("Panes across (and up, by the shape)"), P::color("color", [0.85, 0.85, 0.82]).describe("Frame colour")],
        |v: &Values, _s: u32| {
            let (w, h, n) = (v.f("width"), v.f("height"), v.f("panes").round() as u32);
            let y0 = 0.08;
            let mut frame = sh::block([w + 0.2, 0.05, 0.2], [0.0, 0.0, 0.03]);
            for x in [-w * 0.5, w * 0.5] {
                geo::merge(&mut frame, &sh::block([0.07, h, 0.1], [x, y0, 0.0]));
            }
            for y in [y0, y0 + h - 0.07] {
                geo::merge(&mut frame, &sh::block([w, 0.07, 0.1], [0.0, y, 0.0]));
            }
            // Glazing bars: `panes` across, rows to keep panes near square.
            let rows = ((h / w) * n as f32).round().max(1.0) as u32;
            for i in 1..n {
                let x = (i as f32 / n as f32 - 0.5) * w;
                geo::merge(&mut frame, &sh::block([0.025, h, 0.05], [x, y0, 0.0]));
            }
            for j in 1..rows {
                let y = y0 + j as f32 / rows as f32 * h;
                geo::merge(&mut frame, &b::cbox([w, 0.025, 0.05], [0.0, y, 0.0]));
            }
            let glass = sh::block([w, h, 0.01], [0.0, y0, 0.0]);
            Output::Parts(vec![Part::new(frame, v.color("color")), Part::new(glass, GLASS).alpha(0.7).material("glass")])
        });

    basic!(_r, "architecture-staircases", "architecture", "staircases", "Staircases", "ground",
        [P::num("steps", 12.0, 3.0, 30.0).step(1.0).describe("Steps"), P::num("width", 1.0, 0.6, 3.0).unit("m").describe("Width (m)"), P::num("rise", 0.18, 0.12, 0.24).unit("m").describe("Rise per step (m)"), P::color("color", WOOD).describe("Wood colour")],
        |v: &Values, _s: u32| {
            let (n, w, rise) = (v.f("steps").round() as u32, v.f("width"), v.f("rise"));
            let run = 0.27;
            let mut steps = geo::Mesh::default();
            // Treads and risers, not a solid block.
            for i in 0..n {
                let (z, y) = ((i as f32 + 0.5) * run - n as f32 * run * 0.5, (i + 1) as f32 * rise);
                geo::merge(&mut steps, &sh::block([w, 0.04, run + 0.03], [0.0, y - 0.04, z]));
                geo::merge(&mut steps, &sh::block([w, rise - 0.04, 0.02], [0.0, y - rise, z - run * 0.5 + 0.01]));
            }
            let (len, top) = (n as f32 * run, n as f32 * rise);
            let mut rails = geo::Mesh::default();
            for x in [-w * 0.5 - 0.03, w * 0.5 + 0.03] {
                geo::merge(&mut rails, &b::bar([x, 0.0, -len * 0.5], [x, top, len * 0.5], 0.05, 0.25));
            }
            // A handrail on balusters, one per step, on the right.
            let x = w * 0.5 + 0.03;
            for i in 0..n {
                let (z, y) = ((i as f32 + 0.5) * run - len * 0.5, (i + 1) as f32 * rise);
                geo::merge(&mut rails, &b::bar([x, y, z], [x, y + 0.85, z], 0.03, 0.03));
            }
            geo::merge(&mut rails, &b::bar([x, rise + 0.9, -len * 0.5 + run * 0.5], [x, top + 0.9, len * 0.5 - run * 0.5], 0.06, 0.05));
            // It climbs away from the front, so the treads face the viewer.
            let turn = |m: &geo::Mesh| sh::place(m, 1.0, sh::PI, 0.0, [0.0; 3]);
            Output::Parts(vec![Part::new(turn(&steps), v.color("color")), Part::new(turn(&rails), [0.3, 0.2, 0.12])])
        });

    basic!(_r, "architecture-balconies", "architecture", "balconies", "Balconies", "stone",
        [P::num("width", 3.0, 1.0, 8.0).unit("m").describe("Width (m)"), P::num("depth", 1.4, 0.6, 3.0).unit("m").describe("Depth (m)"), P::num("height", 3.0, 1.0, 10.0).unit("m").describe("Floor height above the ground (m)"), P::color("color", [0.15, 0.15, 0.16]).describe("Railing colour")],
        |v: &Values, _s: u32| {
            let (w, d, h) = (v.f("width"), v.f("depth"), v.f("height"));
            let mut wall = sh::block([w + 2.0, h + 3.0, 0.3], [0.0, 0.0, -0.15]);
            geo::merge(&mut wall, &sh::block([w, 0.18, d], [0.0, h - 0.18, d * 0.5]));
            // Brackets under the slab (one each side).
            let mut br = geo::Mesh::default();
            for x in [-w * 0.4, w * 0.4] {
                let m = b::extrude_x(&[[0.0, h - 0.18], [0.0, h - 0.18 - d * 0.7], [-d * 0.8, h - 0.18]], 0.12);
                geo::merge(&mut br, &moved(m, [x, 0.0, 0.0]));
            }
            let mut rail = geo::Mesh::default();
            let top = h + 1.0;
            let corners = [[-w * 0.5 + 0.03, 0.02], [-w * 0.5 + 0.03, d - 0.03], [w * 0.5 - 0.03, d - 0.03], [w * 0.5 - 0.03, 0.02]];
            for k in 0..3 {
                let (a, c) = (corners[k], corners[k + 1]);
                geo::merge(&mut rail, &b::bar([a[0], top, a[1]], [c[0], top, c[1]], 0.05, 0.05));
                let len = ((c[0] - a[0]).powi(2) + (c[1] - a[1]).powi(2)).sqrt();
                let n = (len / 0.12) as u32;
                for i in 0..=n {
                    let t = i as f32 / n.max(1) as f32;
                    let (x, z) = (a[0] + (c[0] - a[0]) * t, a[1] + (c[1] - a[1]) * t);
                    geo::merge(&mut rail, &b::bar([x, h, z], [x, top, z], 0.02, 0.02));
                }
            }
            let door = b::cbox([0.9, 2.1, 0.06], [0.0, h + 1.05, 0.01]);
            let mut m = wall;
            geo::merge(&mut m, &br);
            Output::Parts(vec![Part::new(m, [0.7, 0.66, 0.6]), Part::new(rail, v.color("color")).material("metal"), Part::new(door, GLASS).material("glass")])
        });

    basic!(_r, "architecture-porches", "architecture", "porches", "Porches", "ground",
        [P::num("width", 4.0, 1.5, 10.0).unit("m").describe("Width (m)"), P::num("depth", 2.5, 1.0, 5.0).unit("m").describe("Depth (m)"), P::num("height", 0.6, 0.2, 1.5).unit("m").describe("Deck height (m)"), P::color("color", [0.82, 0.8, 0.75]).describe("Paint colour")],
        |v: &Values, _s: u32| {
            let (w, d, h) = (v.f("width"), v.f("depth"), v.f("height"));
            let mut wall = sh::block([w + 1.0, h + 3.2, 0.25], [0.0, 0.0, -0.125]);
            geo::merge(&mut wall, &b::cbox([1.0, 2.1, 0.06], [0.0, h + 1.05, 0.02]));
            let mut deck = sh::block([w, h, d], [0.0, 0.0, d * 0.5]);
            let n = ((h / 0.18).round() as u32).max(1);
            let st = b::stairs(n, 1.4, h / n as f32, 0.28);
            geo::merge(&mut deck, &moved(swung(st, 0.0, 0.0, sh::PI), [0.0, 0.0, d + n as f32 * 0.28]));
            let mut paint = geo::Mesh::default();
            let top = h + 2.6;
            for x in [-w * 0.5 + 0.1, w * 0.5 - 0.1] {
                geo::merge(&mut paint, &b::post(x, d - 0.1, 0.14, top));
                geo::merge(&mut paint, &b::bar([x, h + 0.9, 0.05], [x, h + 0.9, d - 0.1], 0.06, 0.06));
            }
            // A shed roof falling away from the wall.
            let roof = b::extrude_x(&[[0.0, top + 0.5], [-d - 0.3, top - 0.05], [-d - 0.3, top + 0.05], [0.0, top + 0.6]], w + 0.4);
            Output::Parts(vec![Part::new(deck, [0.35, 0.25, 0.17]), Part::new(paint, v.color("color")), Part::new(roof, [0.18, 0.18, 0.2]), Part::new(wall, [0.62, 0.55, 0.45])])
        });

    basic!(_r, "architecture-chimneys", "architecture", "chimneys", "Chimneys", "stone",
        [P::num("height", 2.0, 0.8, 6.0).unit("m").describe("Stack height above the roof (m)"), P::num("pots", 2.0, 1.0, 4.0).step(1.0).describe("Chimney pots"), P::color("color", [0.42, 0.2, 0.13]).describe("Brick colour")],
        |v: &Values, _s: u32| {
            let (h, n) = (v.f("height"), v.f("pots").round() as u32);
            let roof = b::gable(3.0, 3.0, 1.0, 0.0, 0.0);
            let w = 0.35 + 0.3 * n as f32;
            let mut stack = sh::block([w, h + 0.6, 0.6], [0.0, 0.3, 0.0]);
            // Corbelled courses near the top.
            geo::merge(&mut stack, &sh::block([w + 0.12, 0.1, 0.72], [0.0, h + 0.5, 0.0]));
            geo::merge(&mut stack, &sh::block([w + 0.2, 0.12, 0.8], [0.0, h + 0.6, 0.0]));
            let mut pots = geo::Mesh::default();
            for i in 0..n {
                let x = ((i as f32 + 0.5) / n as f32 - 0.5) * (w - 0.15);
                let pot = b::revolve(&[[0.0, 0.0], [0.13, 0.0], [0.11, 0.35], [0.13, 0.4], [0.13, 0.45], [0.09, 0.45], [0.09, 0.3]], 14);
                geo::merge(&mut pots, &moved(pot, [x, h + 0.72, 0.0]));
            }
            Output::Parts(vec![Part::new(stack, v.color("color")), Part::new(roof, [0.15, 0.15, 0.17]), Part::new(pots, [0.55, 0.3, 0.18])])
        });

    basic!(_r, "architecture-columns", "architecture", "columns", "Columns", "stone",
        [P::num("height", 4.0, 1.0, 12.0).unit("m").describe("Height (m)"), P::num("radius", 0.3, 0.1, 1.2).unit("m").describe("Shaft radius (m)"), P::num("flutes", 20.0, 0.0, 32.0).step(1.0).describe("Flutes round the shaft (0: smooth)"), P::color("color", [0.82, 0.8, 0.74]).describe("Stone colour")],
        |v: &Values, _s: u32| {
            let (h, r, f) = (v.f("height"), v.f("radius"), v.f("flutes").round() as u32);
            let (pb, cap) = (r * 0.5, r * 0.6);
            // Plinth, a lathed base, the shaft, a lathed capital, an abacus.
            let mut m = sh::block([r * 2.8, pb * 0.5, r * 2.8], [0.0; 3]);
            geo::merge(&mut m, &b::revolve(&[[0.0, pb * 0.5], [r * 1.3, pb * 0.5], [r * 1.35, pb * 0.75], [r * 1.15, pb], [r * 1.05, pb], [0.0, pb]], 32));
            let shaft_h = h - pb - cap;
            let plan = if f >= 4 {
                (0..f * 6).map(|i| {
                    let a = i as f32 / (f * 6) as f32 * sh::TAU;
                    let dip = (a * f as f32 * 0.5).sin().abs();
                    let k = r * (1.0 - 0.06 * (1.0 - dip));
                    [k * a.cos(), k * a.sin()]
                }).collect()
            } else {
                b::circle(r, 32)
            };
            geo::merge(&mut m, &b::prism(&plan, pb, shaft_h));
            let y = pb + shaft_h;
            geo::merge(&mut m, &b::revolve(&[[0.0, y], [r * 1.02, y], [r * 1.25, y + cap * 0.5], [r * 1.35, y + cap * 0.62], [0.0, y + cap * 0.62]], 32));
            geo::merge(&mut m, &sh::block([r * 2.9, cap * 0.38, r * 2.9], [0.0, y + cap * 0.62, 0.0]));
            Output::Mesh(m)
        });

    basic!(_r, "architecture-structural-beams", "architecture", "structural-beams", "Structural beams", "metal",
        [P::num("span", 6.0, 2.0, 20.0).unit("m").describe("Span between the columns (m)"), P::num("height", 3.5, 2.0, 10.0).unit("m").describe("Column height (m)"), P::num("depth", 0.3, 0.12, 1.0).unit("m").describe("Beam depth (m)"), P::color("color", STEEL).describe("Steel colour")],
        |v: &Values, _s: u32| {
            let (s, h, dp) = (v.f("span"), v.f("height"), v.f("depth"));
            let mut m = geo::Mesh::default();
            for x in [-s * 0.5, s * 0.5] {
                geo::merge(&mut m, &b::sweep(&[[x, 0.02, 0.0], [x, h, 0.0]], &i_profile(0.25, 0.25, 0.02), true));
                geo::merge(&mut m, &sh::block([0.4, 0.02, 0.4], [x, 0.0, 0.0]));
                // A cleat plate where the beam meets the column.
                geo::merge(&mut m, &b::cbox([0.02, dp, 0.2], [x - x.signum() * 0.135, h + dp * 0.5, 0.0]));
            }
            geo::merge(&mut m, &b::sweep(&[[-s * 0.5 - 0.125, h + dp * 0.5, 0.0], [s * 0.5 + 0.125, h + dp * 0.5, 0.0]], &i_profile(dp, dp * 0.55, dp * 0.07), true));
            Output::Mesh(m)
        });

    basic!(_r, "architecture-gutters", "architecture", "gutters", "Gutters", "metal",
        [P::num("length", 4.0, 1.0, 12.0).unit("m").describe("Length (m)"), P::num("radius", 0.06, 0.03, 0.15).unit("m").describe("Gutter radius (m)"), P::num("height", 3.0, 1.0, 8.0).unit("m").describe("Eaves height (m)"), P::color("color", [0.12, 0.12, 0.13]).describe("Gutter colour")],
        |v: &Values, _s: u32| {
            let (l, r, h) = (v.f("length"), v.f("radius"), v.f("height"));
            let t = 0.004;
            // A half-round section, open on top, swept along the eaves.
            let mut sec: Vec<[f32; 2]> = (0..=10).map(|i| {
                let a = sh::PI + sh::PI * i as f32 / 10.0;
                [(r + t) * a.cos(), (r + t) * a.sin()]
            }).collect();
            sec.extend((0..=10).rev().map(|i| {
                let a = sh::PI + sh::PI * i as f32 / 10.0;
                [r * a.cos(), r * a.sin()]
            }));
            let mut m = b::sweep(&[[-l * 0.5, h, r + 0.02], [l * 0.5, h, r + 0.02]], &sec, true);
            // Brackets, a downpipe with an outlet, and the shoe at its foot.
            let n = (l / 0.9) as u32;
            for i in 0..=n {
                let x = (i as f32 / n.max(1) as f32 - 0.5) * (l - 0.1);
                geo::merge(&mut m, &b::sweep(&[[x, h, 0.0], [x, h, r * 2.0 + 0.04]], &b::rect(0.03, 0.006), true));
            }
            let px = l * 0.5 - 0.15;
            let pr = r * 0.55;
            geo::merge(&mut m, &b::rod([px, h - r, r + 0.02], [px, h - r - 0.25, 0.08], pr, 12));
            geo::merge(&mut m, &b::rod([px, h - r - 0.25, 0.08], [px, 0.18, 0.08], pr, 12));
            geo::merge(&mut m, &b::rod([px, 0.18, 0.08], [px, 0.05, 0.25], pr, 12));
            let fascia = sh::block([l + 0.2, 0.22, 0.03], [0.0, h - 0.1, -0.015]);
            let mut wall = sh::block([l + 0.6, h - 0.1, 0.25], [0.0, 0.0, -0.13]);
            geo::merge(&mut wall, &fascia);
            Output::Parts(vec![Part::new(m, v.color("color")), Part::new(wall, [0.72, 0.68, 0.6]).material("stone")])
        });

    basic!(_r, "architecture-fences", "architecture", "fences", "Fences", "ground",
        [P::num("length", 6.0, 1.0, 30.0).unit("m").describe("Length (m)"), P::num("height", 1.0, 0.4, 2.0).unit("m").describe("Picket height (m)"), P::num("gap", 0.05, 0.01, 0.2).unit("m").describe("Gap between pickets (m)"), P::color("color", [0.86, 0.85, 0.8]).describe("Paint colour")],
        |v: &Values, s: u32| {
            let (l, h, g) = (v.f("length"), v.f("height"), v.f("gap"));
            let pw = 0.08;
            let mut m = geo::Mesh::default();
            let picket = b::extrude(&[[-pw * 0.5, 0.0], [pw * 0.5, 0.0], [pw * 0.5, h - pw * 0.6], [0.0, h], [-pw * 0.5, h - pw * 0.6]], 0.02);
            let n = (l / (pw + g)).floor() as u32;
            for i in 0..n {
                let x = (i as f32 + 0.5) * (pw + g) - l * 0.5;
                let lift = sh::range(s, i, 1, 0.03, 0.05);
                geo::merge(&mut m, &moved(picket.clone(), [x, lift, 0.04]));
            }
            for y in [h * 0.25, h * 0.7] {
                geo::merge(&mut m, &b::cbox([l, 0.08, 0.03], [0.0, y, 0.015]));
            }
            let posts = (l / 2.0).ceil() as u32;
            for i in 0..=posts {
                let x = (i as f32 / posts as f32 - 0.5) * l;
                geo::merge(&mut m, &b::post(x, -0.04, 0.09, h * 0.9));
            }
            Output::Mesh(m)
        });

    basic!(_r, "architecture-gates", "architecture", "gates", "Gates", "ground",
        [P::num("width", 3.0, 1.0, 5.0).unit("m").describe("Gate width (m)"), P::num("height", 1.2, 0.8, 2.0).unit("m").describe("Height (m)"), P::num("open", 30.0, 0.0, 110.0).unit("deg").describe("How far it stands open (degrees)"), P::color("color", [0.5, 0.4, 0.28]).describe("Wood colour")],
        |v: &Values, _s: u32| {
            let (w, h, open) = (v.f("width"), v.f("height"), v.f("open").to_radians());
            let y0 = 0.1;
            // A five-bar gate: stiles, five rails and a diagonal brace.
            let mut gate = geo::Mesh::default();
            for x in [0.0, w] {
                geo::merge(&mut gate, &b::bar([x, y0, 0.0], [x, y0 + h, 0.0], 0.09, 0.07));
            }
            for i in 0..5 {
                let y = y0 + 0.05 + i as f32 / 4.0 * (h - 0.1);
                geo::merge(&mut gate, &b::bar([0.0, y, 0.0], [w, y, 0.0], 0.1, 0.04));
            }
            geo::merge(&mut gate, &b::bar([0.05, y0 + 0.05, 0.035], [w - 0.05, y0 + h - 0.05, 0.035], 0.1, 0.03));
            let gate = swung(moved(gate, [-w * 0.5, 0.0, 0.0]), -w * 0.5, 0.0, open);
            let mut posts = geo::Mesh::default();
            for x in [-w * 0.5 - 0.12, w * 0.5 + 0.12] {
                geo::merge(&mut posts, &b::post(x, 0.0, 0.18, h + 0.35));
            }
            Output::Parts(vec![Part::new(gate, v.color("color")), Part::new(posts, [0.32, 0.25, 0.17])])
        });
}
