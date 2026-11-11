//! Category 13, Industrial and factory assets: basic generators (batch B).
//! Steel frames of I-sections and bars, rollers and drums as rods, lathed
//! tanks, silos, valves and motors, gears from their toothed outline, pipe
//! runs swept along seeded routes, and a robot arm posed by its joint
//! angles. Inspired by three.js (MIT) ExtrudeGeometry, LatheGeometry,
//! CylinderGeometry and TubeGeometry; written fresh here.

use evani_procedural::Registry;

#[allow(unused_imports, dead_code)]
pub fn register(_r: &mut Registry) {
    use evani_procedural::{Output, ParamSpec as P, Part, Values, build as b, geo, shapes as sh};
    const STEEL: [f32; 3] = [0.22, 0.23, 0.24];
    const YELLOW: [f32; 3] = [0.62, 0.42, 0.03];
    const RUBBER: [f32; 3] = [0.03, 0.03, 0.03];
    const GREEN: [f32; 3] = [0.1, 0.24, 0.16];

    fn moved(mut m: geo::Mesh, d: [f32; 3]) -> geo::Mesh {
        geo::transform(&mut m, [1.0; 3], d);
        m
    }

    /// An I-section `h` deep, `w` wide, flanges `t` thick.
    fn i_profile(h: f32, w: f32, t: f32) -> Vec<[f32; 2]> {
        let (a, c, web) = (w * 0.5, h * 0.5, t * 0.5);
        vec![[-a, -c], [a, -c], [a, -c + t], [web, -c + t], [web, c - t], [a, c - t], [a, c], [-a, c], [-a, c - t], [-web, c - t], [-web, -c + t], [-a, -c + t]]
    }

    /// An I-beam from `a` to `c`.
    fn beam(a: [f32; 3], c: [f32; 3], h: f32) -> geo::Mesh {
        b::sweep(&[a, c], &i_profile(h, h * 0.55, h * 0.08), true)
    }

    /// A conveyor frame `l` long at belt height `h`: side rails and legs.
    fn conveyor_frame(l: f32, w: f32, h: f32) -> geo::Mesh {
        let mut m = geo::Mesh::default();
        for z in [-w * 0.5 - 0.03, w * 0.5 + 0.03] {
            geo::merge(&mut m, &b::sweep(&[[-l * 0.5, h - 0.06, z], [l * 0.5, h - 0.06, z]], &b::rect(0.03, 0.16), true));
        }
        let n = (l / 1.5).ceil() as u32;
        for i in 0..=n {
            let x = (i as f32 / n as f32 - 0.5) * (l - 0.1);
            for z in [-w * 0.5 - 0.03, w * 0.5 + 0.03] {
                geo::merge(&mut m, &b::post(x, z, 0.05, h - 0.14));
            }
            geo::merge(&mut m, &b::cbox([0.04, 0.04, w + 0.06], [x, 0.25, 0.0]));
        }
        m
    }

    /// A hook: a shank and a curled bend.
    fn hook(at: [f32; 3], s: f32) -> geo::Mesh {
        let mut m = b::rod([at[0], at[1], at[2]], [at[0], at[1] - s * 0.5, at[2]], s * 0.06, 8);
        let curl: Vec<[f32; 3]> = (0..=12).map(|k| {
            let a = sh::PI * 1.5 * k as f32 / 12.0 - sh::PI;
            [at[0] + s * 0.22 * a.cos() + s * 0.22, at[1] - s * 0.72 + s * 0.22 * a.sin(), at[2]]
        }).collect();
        geo::merge(&mut m, &b::sweep(&curl, &b::circle(s * 0.06, 8), true));
        m
    }

    basic!(_r, "industrial-conveyor-belts", "industrial", "conveyor-belts", "Conveyor belts", "ground",
        [P::num("length", 6.0, 1.5, 20.0).unit("m").describe("Length (m)"), P::num("height", 0.9, 0.4, 1.6).unit("m").describe("Belt height (m)"), P::color("color", RUBBER).describe("Belt colour"), P::color("frame", [0.16, 0.3, 0.45]).describe("Frame colour")],
        |v: &Values, _s: u32| {
            let (l, h) = (v.f("length"), v.f("height"));
            let w = 0.6;
            let frame = conveyor_frame(l, w, h);
            // The belt wraps two end drums.
            let mut belt = sh::block([l, 0.015, w], [0.0, h, 0.0]);
            geo::merge(&mut belt, &sh::block([l, 0.015, w], [0.0, h - 0.13, 0.0]));
            let mut drums = geo::Mesh::default();
            for x in [-l * 0.5, l * 0.5] {
                geo::merge(&mut belt, &b::rod([x, h - 0.0575, -w * 0.5], [x, h - 0.0575, w * 0.5], 0.072, 16));
                geo::merge(&mut drums, &b::rod([x, h - 0.0575, -w * 0.5 - 0.06], [x, h - 0.0575, w * 0.5 + 0.06], 0.02, 8));
            }
            let mut f = frame;
            geo::merge(&mut f, &drums);
            Output::Parts(vec![Part::new(belt, v.color("color")).material("ground"), Part::new(f, v.color("frame"))])
        });

    basic!(_r, "industrial-roller-conveyors", "industrial", "roller-conveyors", "Roller conveyors", "ground",
        [P::num("length", 4.0, 1.0, 16.0).unit("m").describe("Length (m)"), P::num("pitch", 0.12, 0.06, 0.4).unit("m").describe("Roller spacing (m)"), P::color("color", [0.5, 0.52, 0.55]).describe("Roller colour")],
        |v: &Values, _s: u32| {
            let (l, p) = (v.f("length"), v.f("pitch"));
            let (w, h) = (0.6, 0.75);
            let frame = conveyor_frame(l, w, h);
            let mut rollers = geo::Mesh::default();
            let n = (l / p).floor() as u32;
            for i in 0..n {
                let x = (i as f32 + 0.5) * p - n as f32 * p * 0.5;
                geo::merge(&mut rollers, &b::rod([x, h - 0.02, -w * 0.5], [x, h - 0.02, w * 0.5], 0.025, 12));
            }
            Output::Parts(vec![Part::new(rollers, v.color("color")).material("metal"), Part::new(frame, YELLOW)])
        });

    basic!(_r, "industrial-overhead-cranes", "industrial", "overhead-cranes", "Overhead cranes", "ground",
        [P::num("span", 10.0, 4.0, 30.0).unit("m").describe("Span between the runways (m)"), P::num("height", 7.0, 3.0, 15.0).unit("m").describe("Runway height (m)"), P::num("hook", 0.5, 0.0, 1.0).describe("Hook drop (share of the height)"), P::color("color", YELLOW).describe("Crane colour")],
        |v: &Values, _s: u32| {
            let (s, h, drop) = (v.f("span"), v.f("height"), v.f("hook"));
            let run = s * 0.9;
            let mut steel = geo::Mesh::default();
            // Two runway beams on columns.
            for z in [-s * 0.5, s * 0.5] {
                for x in [-run * 0.5, 0.0, run * 0.5] {
                    geo::merge(&mut steel, &beam([x, 0.0, z], [x, h, z], 0.35));
                }
                geo::merge(&mut steel, &beam([-run * 0.5 - 0.3, h + 0.2, z], [run * 0.5 + 0.3, h + 0.2, z], 0.4));
            }
            // The bridge girder across, its end trucks and the trolley.
            let mut crane = beam([0.0, h + 0.75, -s * 0.5 - 0.2], [0.0, h + 0.75, s * 0.5 + 0.2], 0.7);
            for z in [-s * 0.5, s * 0.5] {
                geo::merge(&mut crane, &b::cbox([1.6, 0.4, 0.4], [0.0, h + 0.55, z]));
            }
            geo::merge(&mut crane, &b::cbox([1.0, 0.6, 1.2], [0.0, h + 1.4, s * 0.15]));
            let y = h + 1.1 - (h - 0.8) * drop;
            let mut rope = b::rod([0.0, h + 1.1, s * 0.15], [0.0, y, s * 0.15], 0.02, 6);
            geo::merge(&mut rope, &b::cbox([0.3, 0.35, 0.25], [0.0, y - 0.1, s * 0.15]));
            geo::merge(&mut rope, &hook([0.0, y - 0.28, s * 0.15], 0.5));
            Output::Parts(vec![Part::new(crane, v.color("color")), Part::new(steel, STEEL), Part::new(rope, [0.1, 0.1, 0.1]).material("metal")])
        });

    basic!(_r, "industrial-hoists", "industrial", "hoists", "Hoists", "ground",
        [P::num("chain", 1.5, 0.3, 4.0).unit("m").describe("Chain drop (m)"), P::num("size", 0.35, 0.15, 0.8).unit("m").describe("Hoist body size (m)"), P::color("color", [0.6, 0.12, 0.06]).describe("Body colour")],
        |v: &Values, _s: u32| {
            let (c, k) = (v.f("chain"), v.f("size"));
            let top = c + k * 2.0 + 0.6;
            let mut steel = beam([-1.2, top, 0.0], [1.2, top, 0.0], 0.25);
            for x in [-1.1, 1.1] {
                geo::merge(&mut steel, &b::post(x, 0.0, 0.12, top - 0.125));
            }
            // The body hangs from a trolley; the chain is a column of links.
            let mut body = b::cbox([k * 0.8, k * 0.2, 0.15], [0.0, top - 0.2, 0.0]);
            geo::merge(&mut body, &moved(b::revolve(&[[0.0, 0.0], [k * 0.5, 0.0], [k * 0.5, k * 0.9], [0.0, k * 0.9]], 20), [0.0, top - 0.3 - k * 0.9, 0.0]));
            let mut chain = geo::Mesh::default();
            let link = 0.05;
            let n = (c / link) as u32;
            let y0 = top - 0.3 - k * 0.9;
            for i in 0..n {
                let ring: Vec<[f32; 3]> = (0..=10).map(|j| {
                    let a = j as f32 / 10.0 * sh::TAU;
                    [0.0, y0 - i as f32 * link - link * 0.6 * (0.5 + 0.5 * a.cos()), 0.015 * a.sin()]
                }).collect();
                let mut l = b::sweep(&ring, &b::circle(0.005, 6), false);
                if i % 2 == 1 {
                    sh::rotate(&mut l, [0.0, 1.0, 0.0], sh::PI * 0.5);
                }
                geo::merge(&mut chain, &l);
            }
            geo::merge(&mut chain, &hook([0.0, y0 - c - 0.02, 0.0], 0.3));
            Output::Parts(vec![Part::new(body, v.color("color")), Part::new(steel, STEEL), Part::new(chain, [0.3, 0.3, 0.3]).material("metal")])
        });

    basic!(_r, "industrial-gantry-cranes", "industrial", "gantry-cranes", "Gantry cranes", "ground",
        [P::num("span", 12.0, 4.0, 40.0).unit("m").describe("Span (m)"), P::num("height", 9.0, 3.0, 25.0).unit("m").describe("Height (m)"), P::color("color", [0.62, 0.2, 0.05]).describe("Crane colour")],
        |v: &Values, _s: u32| {
            let (s, h) = (v.f("span"), v.f("height"));
            let mut crane = geo::Mesh::default();
            // A-frame legs at each end on bogies running on rails.
            let foot = h * 0.22;
            for x in [-s * 0.5, s * 0.5] {
                for z in [-foot, foot] {
                    geo::merge(&mut crane, &b::bar([x, 0.6, z], [x, h, 0.0], 0.4, 0.4));
                }
                geo::merge(&mut crane, &b::bar([x, h * 0.3, -foot * 0.7], [x, h * 0.3, foot * 0.7], 0.25, 0.25));
                geo::merge(&mut crane, &b::cbox([0.8, 0.5, foot * 2.0 + 1.0], [x, 0.45, 0.0]));
            }
            for z in [-0.5, 0.5] {
                geo::merge(&mut crane, &b::cbox([s + 4.0, 0.9, 0.35], [0.0, h + 0.45, z]));
            }
            let trolley = b::cbox([1.6, 1.0, 1.6], [s * 0.15, h + 1.4, 0.0]);
            let mut rails = geo::Mesh::default();
            for x in [-s * 0.5, s * 0.5] {
                for zz in [-0.1, 0.1] {
                    geo::merge(&mut rails, &b::cbox([0.08, 0.12, foot * 2.0 + 6.0], [x + zz, 0.06, 0.0]));
                }
            }
            let mut hook_m = b::rod([s * 0.15, h + 0.9, 0.0], [s * 0.15, h * 0.4, 0.0], 0.03, 6);
            geo::merge(&mut hook_m, &hook([s * 0.15, h * 0.4, 0.0], 0.7));
            geo::merge(&mut crane, &trolley);
            Output::Parts(vec![Part::new(crane, v.color("color")), Part::new(rails, STEEL).material("metal"), Part::new(hook_m, [0.1, 0.1, 0.1])])
        });

    basic!(_r, "industrial-robotic-arms", "industrial", "robotic-arms", "Robotic arms", "accent",
        [P::num("shoulder", 35.0, -10.0, 90.0).unit("deg").describe("Shoulder angle up from level (degrees)"), P::num("elbow", -60.0, -150.0, 0.0).unit("deg").describe("Elbow bend (degrees)"), P::num("turn", 30.0, -180.0, 180.0).unit("deg").describe("Base turn (degrees)"), P::color("color", [0.85, 0.36, 0.04]).describe("Arm colour")],
        |v: &Values, _s: u32| {
            let (sh_a, el, turn) = (v.f("shoulder").to_radians(), v.f("elbow").to_radians(), v.f("turn").to_radians());
            let base = b::revolve(&[[0.0, 0.0], [0.45, 0.0], [0.45, 0.15], [0.32, 0.25], [0.0, 0.25]], 28);
            // The arm in its own plane (x up), turned about y at the end.
            let mut arm = moved(b::revolve(&[[0.0, 0.0], [0.28, 0.0], [0.28, 0.45], [0.0, 0.45]], 24), [0.0, 0.25, 0.0]);
            let shoulder = [0.0, 0.85, 0.0];
            let (l1, l2) = (1.2, 1.0);
            let elbow = [shoulder[0] + l1 * sh_a.cos(), shoulder[1] + l1 * sh_a.sin(), 0.0];
            let a2 = sh_a + el;
            let wrist = [elbow[0] + l2 * a2.cos(), elbow[1] + l2 * a2.sin(), 0.0];
            geo::merge(&mut arm, &b::bar(shoulder, elbow, 0.22, 0.26));
            geo::merge(&mut arm, &b::bar(elbow, wrist, 0.16, 0.18));
            let mut joints = geo::Mesh::default();
            for (p, r) in [(shoulder, 0.2f32), (elbow, 0.15), (wrist, 0.1)] {
                geo::merge(&mut joints, &b::rod([p[0], p[1], -r - 0.05], [p[0], p[1], r + 0.05], r, 16));
            }
            // A two-finger gripper past the wrist.
            let dir = [a2.cos(), a2.sin(), 0.0];
            let tip = [wrist[0] + dir[0] * 0.15, wrist[1] + dir[1] * 0.15, 0.0];
            geo::merge(&mut joints, &b::bar(wrist, tip, 0.12, 0.12));
            for z in [-0.05f32, 0.05] {
                geo::merge(&mut joints, &b::bar([tip[0], tip[1], z], [tip[0] + dir[0] * 0.15, tip[1] + dir[1] * 0.15, z], 0.03, 0.04));
            }
            let spin = |m: &geo::Mesh| sh::place(m, 1.0, turn, 0.0, [0.0; 3]);
            Output::Parts(vec![Part::new(spin(&arm), v.color("color")), Part::new(spin(&joints), [0.15, 0.15, 0.16]), Part::new(base, [0.2, 0.2, 0.22])])
        });

    basic!(_r, "industrial-pumps", "industrial", "pumps", "Pumps", "ground",
        [P::num("size", 0.5, 0.2, 2.0).unit("m").describe("Casing size (m)"), P::num("pipe", 0.12, 0.04, 0.4).unit("m").describe("Pipe radius (m)"), P::color("color", [0.12, 0.25, 0.5]).describe("Pump colour")],
        |v: &Values, _s: u32| {
            let (k, pr) = (v.f("size"), v.f("pipe").min(v.f("size") * 0.4));
            let mut skid = sh::block([k * 4.0, 0.08, k * 1.6], [0.0; 3]);
            geo::merge(&mut skid, &sh::block([k * 4.0, 0.08, 0.1], [0.0, 0.0, 0.0]));
            // The volute: a squat drum on its side, the motor in line.
            let cy = 0.08 + k * 0.6;
            let mut pump = b::rod([-k * 0.25, cy, 0.0], [k * 0.25, cy, 0.0], k * 0.55, 24);
            geo::merge(&mut pump, &b::cbox([k * 0.4, cy - 0.08, k * 0.5], [0.0, 0.08 + (cy - 0.08) * 0.5, 0.0]));
            geo::merge(&mut pump, &b::rod([k * 0.25, cy, 0.0], [k * 2.0, cy, 0.0], k * 0.38, 24));
            geo::merge(&mut pump, &b::cbox([k * 1.4, cy - 0.08, k * 0.5], [k * 1.1, 0.08 + (cy - 0.08) * 0.5, 0.0]));
            // Suction in at the eye, discharge up out of the top.
            let mut pipes = b::rod([-k * 0.25, cy, 0.0], [-k * 2.0, cy, 0.0], pr, 16);
            geo::merge(&mut pipes, &b::rod([0.0, cy + k * 0.5, 0.0], [0.0, cy + k * 1.6, 0.0], pr, 16));
            for (a, c) in [([-k * 0.3, cy, 0.0], [-k * 0.36, cy, 0.0]), ([-k * 1.9, cy, 0.0], [-k * 1.96, cy, 0.0]), ([0.0, cy + k * 1.5, 0.0], [0.0, cy + k * 1.56, 0.0])] {
                geo::merge(&mut pipes, &b::rod(a, c, pr * 1.6, 16));
            }
            Output::Parts(vec![Part::new(pump, v.color("color")), Part::new(pipes, STEEL).material("metal"), Part::new(skid, [0.15, 0.15, 0.15])])
        });

    basic!(_r, "industrial-electric-motors", "industrial", "electric-motors", "Electric motors", "ground",
        [P::num("length", 0.6, 0.2, 2.0).unit("m").describe("Body length (m)"), P::num("radius", 0.2, 0.08, 0.6).unit("m").describe("Body radius (m)"), P::num("fins", 16.0, 0.0, 40.0).step(1.0).describe("Cooling fins"), P::color("color", [0.2, 0.32, 0.45]).describe("Motor colour")],
        |v: &Values, _s: u32| {
            let (l, r, n) = (v.f("length"), v.f("radius"), v.f("fins").round() as u32);
            let cy = r * 1.2;
            let mut body = b::rod([-l * 0.5, cy, 0.0], [l * 0.5, cy, 0.0], r, 28);
            for x in [-l * 0.5 - r * 0.15, l * 0.5] {
                geo::merge(&mut body, &b::rod([x, cy, 0.0], [x + r * 0.15, cy, 0.0], r * 0.92, 28));
            }
            for i in 0..n {
                let a = i as f32 / n as f32 * sh::TAU;
                if a.sin() < -0.6 {
                    continue;
                }
                let mut fin = b::cbox([l * 0.9, r * 0.14, 0.012], [0.0, r * 1.05, 0.0]);
                sh::rotate(&mut fin, [1.0, 0.0, 0.0], a);
                geo::merge(&mut body, &moved(fin, [0.0, cy, 0.0]));
            }
            // Feet, a terminal box on top, the shaft out of one end.
            for x in [-l * 0.3, l * 0.3] {
                geo::merge(&mut body, &b::cbox([r * 0.5, cy * 0.5, r * 2.0], [x, cy * 0.25, 0.0]));
            }
            geo::merge(&mut body, &b::cbox([r * 0.8, r * 0.4, r * 0.8], [0.0, cy + r * 1.1, 0.0]));
            let shaft = b::rod([l * 0.5 + r * 0.15, cy, 0.0], [l * 0.5 + r * 0.8, cy, 0.0], r * 0.15, 12);
            Output::Parts(vec![Part::new(body, v.color("color")), Part::new(shaft, [0.55, 0.55, 0.58]).material("metal")])
        });

    basic!(_r, "industrial-generators", "industrial", "generators", "Generators", "ground",
        [P::num("length", 3.0, 1.0, 8.0).unit("m").describe("Length (m)"), P::num("louvres", 12.0, 0.0, 30.0).step(1.0).describe("Louvres on each side"), P::color("color", [0.75, 0.6, 0.1]).describe("Canopy colour")],
        |v: &Values, _s: u32| {
            let (l, n) = (v.f("length"), v.f("louvres").round() as u32);
            let (w, h) = (l * 0.4 + 0.4, l * 0.4 + 0.5);
            let skid = sh::block([l + 0.2, 0.15, w + 0.1], [0.0; 3]);
            let mut canopy = sh::block([l, h, w], [0.0, 0.15, 0.0]);
            let mut dark = geo::Mesh::default();
            // Louvres along each side, a radiator grille at one end, doors.
            for i in 0..n {
                let x = ((i as f32 + 0.5) / n as f32 - 0.5) * l * 0.5 - l * 0.2;
                for z in [-w * 0.5 - 0.01, w * 0.5 + 0.01] {
                    let louvre = b::cbox([l * 0.5 / n as f32 * 0.7, h * 0.5, 0.02], [0.0; 3]);
                    geo::merge(&mut dark, &moved(louvre, [x, 0.15 + h * 0.5, z]));
                }
            }
            geo::merge(&mut dark, &b::cbox([0.02, h * 0.7, w * 0.8], [l * 0.5 + 0.01, 0.15 + h * 0.5, 0.0]));
            for x in [l * 0.2, l * 0.35] {
                geo::merge(&mut canopy, &b::cbox([l * 0.13, h * 0.8, 0.015], [x, 0.15 + h * 0.48, w * 0.5 + 0.008]));
            }
            let stack = b::rod([-l * 0.3, 0.15 + h, 0.0], [-l * 0.3, 0.15 + h + 0.7, 0.0], 0.08, 12);
            geo::merge(&mut dark, &stack);
            Output::Parts(vec![Part::new(canopy, v.color("color")), Part::new(dark, [0.08, 0.08, 0.08]), Part::new(skid, STEEL)])
        });

    basic!(_r, "industrial-gearboxes", "industrial", "gearboxes", "Gearboxes", "ground",
        [P::num("teeth", 24.0, 8.0, 60.0).step(1.0).describe("Teeth on the big gear"), P::num("size", 0.6, 0.2, 2.0).unit("m").describe("Housing size (m)"), P::color("color", [0.25, 0.3, 0.28]).describe("Housing colour")],
        |v: &Values, _s: u32| {
            let (n, k) = (v.f("teeth").round() as u32, v.f("size"));
            let mut housing = b::rounded_box([k * 1.6, k, k * 0.7], k * 0.06, 4, [0.0; 3]);
            for x in [-k * 0.55, k * 0.55] {
                geo::merge(&mut housing, &b::cbox([0.04, k * 0.8, k * 0.75], [x, k * 0.5, 0.0]));
            }
            // An open side shows the big gear meshing with a pinion.
            let big_r = k * 0.38;
            let small_n = (n / 2).max(6);
            let small_r = big_r * small_n as f32 / n as f32;
            let depth = (big_r * 0.12).max(0.01);
            let mut gears = moved(b::extrude(&b::gear(n, big_r, depth), k * 0.1), [-k * 0.25, k * 0.5, k * 0.4]);
            let mut pin = b::extrude(&b::gear(small_n, small_r, depth), k * 0.1);
            sh::rotate(&mut pin, [0.0, 0.0, 1.0], sh::PI / small_n as f32);
            geo::merge(&mut gears, &moved(pin, [-k * 0.25 + big_r + small_r - depth * 0.6, k * 0.5, k * 0.4]));
            let mut shafts = b::rod([-k * 0.25, k * 0.5, -k * 0.6], [-k * 0.25, k * 0.5, k * 0.5], k * 0.06, 12);
            let px = -k * 0.25 + big_r + small_r - depth * 0.6;
            geo::merge(&mut shafts, &b::rod([px, k * 0.5, -k * 0.6], [px, k * 0.5, k * 0.5], k * 0.04, 12));
            Output::Parts(vec![Part::new(housing, v.color("color")), Part::new(gears, [0.5, 0.48, 0.42]).material("metal"), Part::new(shafts, [0.35, 0.35, 0.36]).material("metal")])
        });

    basic!(_r, "industrial-pipe-networks", "industrial", "pipe-networks", "Pipe networks", "ground",
        [P::num("runs", 4.0, 1.0, 10.0).step(1.0).describe("Pipe runs"), P::num("length", 6.0, 2.0, 20.0).unit("m").describe("Length of the rack (m)"), P::num("radius", 0.1, 0.03, 0.4).unit("m").describe("Pipe radius (m)"), P::color("color", [0.5, 0.42, 0.22]).describe("Pipe colour")],
        |v: &Values, s: u32| {
            let (n, l, r) = (v.f("runs").round() as u32, v.f("length"), v.f("radius"));
            let h = 2.5;
            // A rack of portal frames; each run follows it and drops at a seeded end.
            let mut rack = geo::Mesh::default();
            let frames = (l / 3.0).ceil() as u32;
            let width = n as f32 * r * 3.0 + 0.4;
            for i in 0..=frames {
                let x = (i as f32 / frames as f32 - 0.5) * l;
                for z in [-width * 0.5, width * 0.5] {
                    geo::merge(&mut rack, &b::post(x, z, 0.15, h));
                }
                geo::merge(&mut rack, &b::cbox([0.15, 0.15, width + 0.15], [x, h - 0.075, 0.0]));
            }
            let (mut pipes, mut flanges) = (geo::Mesh::default(), geo::Mesh::default());
            for k in 0..n {
                let z = ((k as f32 + 0.5) / n as f32 - 0.5) * (width - 0.4);
                let y = h + r;
                let end = if sh::rnd(s, k, 1) > 0.5 { 1.0 } else { -1.0 };
                let bend = r * 3.0;
                let x_end = end * (l * 0.5 + sh::range(s, k, 2, 0.2, 1.0));
                // Along the rack, a rounded elbow, then down to a seeded height.
                let mut path = vec![[-x_end, y, z], [x_end - end * bend, y, z]];
                for j in 1..=6 {
                    let a = j as f32 / 6.0 * sh::PI * 0.5;
                    path.push([x_end - end * bend + end * bend * a.sin(), y - bend + bend * a.cos(), z]);
                }
                let foot = sh::range(s, k, 3, 0.3, 1.2);
                path.push([x_end, foot, z]);
                geo::merge(&mut pipes, &b::sweep(&path, &b::circle(r, 14), true));
                geo::merge(&mut flanges, &b::rod([x_end, foot - 0.03, z], [x_end, foot + 0.03, z], r * 1.6, 14));
                geo::merge(&mut flanges, &b::rod([-x_end - end * 0.03, y, z], [-x_end + end * 0.03, y, z], r * 1.6, 14));
            }
            Output::Parts(vec![Part::new(pipes, v.color("color")), Part::new(flanges, STEEL).material("metal"), Part::new(rack, [0.32, 0.33, 0.35])])
        });

    basic!(_r, "industrial-valves", "industrial", "valves", "Valves", "ground",
        [P::num("size", 0.3, 0.1, 1.2).unit("m").describe("Pipe bore radius (m)"), P::num("spokes", 4.0, 3.0, 8.0).step(1.0).describe("Handwheel spokes"), P::color("color", [0.1, 0.1, 0.11]).describe("Body colour"), P::color("wheel", [0.6, 0.08, 0.05]).describe("Handwheel colour")],
        |v: &Values, _s: u32| {
            let (k, n) = (v.f("size"), v.f("spokes").round() as u32);
            let cy = k * 2.2;
            // A gate valve: a bulbous body between two bolted flanges, a bonnet, a stem.
            let mut body = b::rod([-k * 2.2, cy, 0.0], [k * 2.2, cy, 0.0], k, 20);
            for x in [-k * 1.2, k * 1.2] {
                geo::merge(&mut body, &b::rod([x - k * 0.15, cy, 0.0], [x + k * 0.15, cy, 0.0], k * 1.6, 24));
                for i in 0..8 {
                    let a = i as f32 / 8.0 * sh::TAU;
                    let (yy, zz) = (cy + k * 1.4 * a.cos(), k * 1.4 * a.sin());
                    geo::merge(&mut body, &b::rod([x - k * 0.22, yy, zz], [x + k * 0.22, yy, zz], k * 0.08, 6));
                }
            }
            geo::merge(&mut body, &sh::blob([k, k * 1.1, k], [0.0, cy, 0.0], 0.0, 1, 2));
            geo::merge(&mut body, &b::revolve(&[[0.0, cy], [k * 0.7, cy], [k * 0.5, cy + k * 2.2], [0.0, cy + k * 2.2]], 16));
            geo::merge(&mut body, &b::rod([0.0, cy + k * 2.2, 0.0], [0.0, cy + k * 3.0, 0.0], k * 0.1, 8));
            for x in [-k * 2.0, k * 2.0] {
                geo::merge(&mut body, &b::cbox([k * 0.4, cy - k, k * 0.8], [x, (cy - k) * 0.5, 0.0]));
            }
            let wy = cy + k * 3.0;
            let rim: Vec<[f32; 2]> = (0..=12).map(|i| {
                let a = -sh::PI * 0.5 + i as f32 / 12.0 * sh::TAU;
                [k * 1.2 + k * 0.08 * a.cos(), wy + k * 0.08 * a.sin()]
            }).collect();
            let mut wheel = b::revolve(&rim, 32);
            for i in 0..n {
                let a = i as f32 / n as f32 * sh::TAU;
                geo::merge(&mut wheel, &b::rod([0.0, wy, 0.0], [k * 1.2 * a.cos(), wy, k * 1.2 * a.sin()], k * 0.05, 6));
            }
            Output::Parts(vec![Part::new(body, v.color("color")), Part::new(wheel, v.color("wheel"))])
        });

    basic!(_r, "industrial-storage-tanks", "industrial", "storage-tanks", "Storage tanks", "ground",
        [P::num("radius", 3.0, 0.5, 15.0).unit("m").describe("Radius (m)"), P::num("height", 6.0, 1.0, 25.0).unit("m").describe("Shell height (m)"), P::color("color", [0.12, 0.21, 0.14]).describe("Tank colour")],
        |v: &Values, _s: u32| {
            let (r, h) = (v.f("radius"), v.f("height"));
            // A shell on a ring plinth with a low domed roof.
            let mut prof = vec![[0.0, 0.3], [r, 0.3], [r, h]];
            for i in 1..=8 {
                let a = i as f32 / 8.0 * sh::PI * 0.5;
                prof.push([r * a.cos(), h + r * 0.18 * a.sin()]);
            }
            let tank = b::revolve(&prof, 48);
            let mut steel = b::revolve(&[[0.0, 0.0], [r + 0.3, 0.0], [r + 0.3, 0.3], [0.0, 0.3]], 48);
            // A caged ladder up the side and a handrail round the top.
            let (lx, lz) = (r + 0.25, 0.0);
            for z in [lz - 0.22, lz + 0.22] {
                geo::merge(&mut steel, &b::bar([lx, 0.3, z], [lx, h + 1.0, z], 0.04, 0.06));
            }
            let rungs = (h / 0.3) as u32;
            for i in 1..rungs {
                geo::merge(&mut steel, &b::rod([lx, 0.3 + i as f32 * 0.3, lz - 0.22], [lx, 0.3 + i as f32 * 0.3, lz + 0.22], 0.012, 6));
            }
            for i in 0..12 {
                let a = (i as f32 / 12.0 - 0.5) * 1.2;
                geo::merge(&mut steel, &b::rod([r * a.cos(), h, r * a.sin()], [r * a.cos(), h + 1.0, r * a.sin()], 0.025, 6));
            }
            let arc: Vec<[f32; 3]> = (0..=24).map(|i| {
                let a = (i as f32 / 24.0 - 0.5) * 1.2;
                [r * a.cos(), h + 1.0, r * a.sin()]
            }).collect();
            geo::merge(&mut steel, &b::sweep(&arc, &b::circle(0.03, 6), true));
            Output::Parts(vec![Part::new(tank, v.color("color")), Part::new(steel, [0.14, 0.14, 0.15]).material("metal")])
        });

    basic!(_r, "industrial-silos", "industrial", "silos", "Silos", "ground",
        [P::num("height", 14.0, 4.0, 40.0).unit("m").describe("Shell height (m)"), P::num("radius", 3.0, 1.0, 10.0).unit("m").describe("Radius (m)"), P::color("color", [0.17, 0.18, 0.19]).describe("Shell colour")],
        |v: &Values, _s: u32| {
            let (h, r) = (v.f("height"), v.f("radius"));
            // A corrugated shell: the profile ripples every 0.4 m.
            let n = (h / 0.4) as u32;
            let mut prof = vec![[0.0, 0.0], [r, 0.0]];
            for i in 1..=n * 2 {
                let y = i as f32 / (n * 2) as f32 * h;
                prof.push([r + if i % 2 == 1 { 0.04 } else { 0.0 }, y]);
            }
            prof.push([r + 0.15, h]);
            prof.push([0.0, h + r * 0.55]);
            let shell = b::revolve(&prof, 40);
            let mut steel = moved(sh::frustum(0.5, 0.35, 0.6, 12), [0.0, h + r * 0.5, 0.0]);
            for z in [-0.22, 0.22] {
                geo::merge(&mut steel, &b::bar([r + 0.25, 0.0, z], [r + 0.25, h + 0.3, z], 0.04, 0.06));
            }
            for i in 1..(h / 0.3) as u32 {
                let y = i as f32 * 0.3;
                geo::merge(&mut steel, &b::rod([r + 0.25, y, -0.22], [r + 0.25, y, 0.22], 0.012, 6));
            }
            geo::merge(&mut steel, &b::revolve(&[[r * 0.9, 0.0], [r + 0.3, 0.0], [r + 0.3, 0.25], [r * 0.9, 0.25], [r * 0.9, 0.0]], 40));
            Output::Parts(vec![Part::new(shell, v.color("color")).material("ground"), Part::new(steel, [0.32, 0.07, 0.04])])
        });

    basic!(_r, "industrial-scaffolding", "industrial", "scaffolding", "Scaffolding", "ground",
        [P::num("bays", 3.0, 1.0, 10.0).step(1.0).describe("Bays along"), P::num("lifts", 3.0, 1.0, 10.0).step(1.0).describe("Levels up"), P::color("color", [0.45, 0.46, 0.48]).describe("Tube colour")],
        |v: &Values, s: u32| {
            let (n, m) = (v.f("bays").round() as u32, v.f("lifts").round() as u32);
            let (bay, lift, d) = (2.0, 2.0, 1.2);
            let l = n as f32 * bay;
            let r = 0.024;
            let mut tubes = geo::Mesh::default();
            let mut boards = geo::Mesh::default();
            for i in 0..=n {
                let x = i as f32 * bay - l * 0.5;
                for z in [0.0, d] {
                    geo::merge(&mut tubes, &b::rod([x, 0.0, z], [x, m as f32 * lift + 1.0, z], r, 8));
                }
            }
            for j in 1..=m {
                let y = j as f32 * lift;
                for z in [0.0, d] {
                    geo::merge(&mut tubes, &b::rod([-l * 0.5 - 0.1, y, z], [l * 0.5 + 0.1, y, z], r, 8));
                    geo::merge(&mut tubes, &b::rod([-l * 0.5, y + 1.0, z], [l * 0.5, y + 1.0, z], r, 8));
                }
                for i in 0..=n {
                    let x = i as f32 * bay - l * 0.5;
                    geo::merge(&mut tubes, &b::rod([x, y, -0.1], [x, y, d + 0.1], r, 8));
                }
                for i in 0..n {
                    let x0 = i as f32 * bay - l * 0.5;
                    for k in 0..5 {
                        let z = 0.12 + k as f32 * 0.24;
                        geo::merge(&mut boards, &b::cbox([bay - 0.04, 0.04, 0.22], [x0 + bay * 0.5, y + r + 0.02 + sh::range(s, i * 7 + j * 31 + k, 1, 0.0, 0.004), z]));
                    }
                }
            }
            // A diagonal brace up the outer face.
            for i in 0..n.min(m) {
                let (x0, y0) = (i as f32 * bay - l * 0.5, i as f32 * lift);
                geo::merge(&mut tubes, &b::rod([x0, y0 + 0.1, -0.05], [x0 + bay, y0 + lift, -0.05], r, 8));
            }
            Output::Parts(vec![Part::new(tubes, v.color("color")).material("metal"), Part::new(boards, [0.5, 0.4, 0.25]).material("ground")])
        });
}
