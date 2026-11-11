//! Category 17, Boats and marine vessels: basic generators (batch C).
//! Hulls are the base's lofted hull (stations whose width, keel lift and
//! sheer follow the length; open boats a skin seen from both sides),
//! superstructures boxes with window rows, masts and spars rods, sails
//! thin double-sided panels. Bow at +x, keel at y = 0. Inspired by three.js
//! (MIT) ExtrudeGeometry and ParametricGeometry (a surface over two
//! parameters), and the hull-from-stations idea of naval lines plans;
//! written fresh here.

use evani_procedural::Registry;

#[allow(unused_imports, dead_code)]
pub fn register(_r: &mut Registry) {
    use evani_procedural::{Output, ParamSpec as P, Part, Values, build as b, craft as c, geo, shapes as sh};
    type Mesh = geo::Mesh;
    const DARK: [f32; 3] = [0.06, 0.06, 0.07];
    const GLASS: [f32; 3] = [0.1, 0.14, 0.2];
    const WOOD: [f32; 3] = [0.36, 0.22, 0.1];
    const WHITE: [f32; 3] = [0.8, 0.8, 0.78];
    const STEEL: [f32; 3] = [0.35, 0.36, 0.38];
    const SAIL: [f32; 3] = [0.82, 0.8, 0.72];

    fn moved(mut m: Mesh, d: [f32; 3]) -> Mesh {
        geo::transform(&mut m, [1.0; 3], d);
        m
    }

    /// A pointed bow and a transom stern of `stern` width share; the keel
    /// rises `rise` (m) at the bow, the sheer `sheer` (m).
    fn shape(stern: f32, rise: f32, sheer: f32) -> impl Fn(f32) -> [f32; 3] {
        move |t: f32| {
            let w = if t > 0.55 { (1.0 - ((t - 0.55) / 0.45).powi(2)).max(0.0).sqrt() } else { stern + (1.0 - stern) * (1.0 - (1.0 - t / 0.55).powi(2)).sqrt() };
            let keel = if t > 0.7 { rise * ((t - 0.7) / 0.3).powi(2) } else { 0.0 };
            [w, keel, sheer * ((t - 0.5) * 2.0).powi(2)]
        }
    }

    /// Pointed at both ends (canoes, kayaks).
    fn double_ended(rise: f32, sheer: f32) -> impl Fn(f32) -> [f32; 3] {
        move |t: f32| {
            let u = (2.0 * t - 1.0).abs();
            [(1.0 - u.powf(2.2)).max(0.0).sqrt(), rise * u.powi(4), sheer * u.powi(3)]
        }
    }

    /// A thin sail through three or four corners, seen from both sides.
    fn sail(pts: &[[f32; 3]]) -> Mesh {
        let mut bl = geo::MeshBuilder::new();
        let n = [0.0, 0.0, 1.0];
        for p in pts {
            bl.vertex(*p, n);
        }
        bl.tri(0, 1, 2);
        if pts.len() > 3 {
            bl.tri(0, 2, 3);
        }
        let mut m = bl.finish();
        geo::smooth_normals(&mut m);
        sh::double_sided(&mut m);
        m
    }

    /// A deckhouse `size` on the deck at `(x, y)`, with a window band on
    /// both sides and the front: (house, windows).
    fn house(size: [f32; 3], at: [f32; 2], windows: u32) -> (Mesh, Mesh) {
        let m = b::rounded_box(size, 0.05, 2, [at[0], at[1], 0.0]);
        let mut w = Mesh::default();
        let y = at[1] + size[1] * 0.62;
        for i in 0..windows {
            let x = at[0] + ((i as f32 + 0.5) / windows as f32 - 0.5) * size[0] * 0.85;
            for z in [-size[2] * 0.5 - 0.01, size[2] * 0.5 + 0.01] {
                geo::merge(&mut w, &b::cbox([size[0] * 0.8 / windows as f32, size[1] * 0.3, 0.03], [x, y, z]));
            }
        }
        geo::merge(&mut w, &b::cbox([0.03, size[1] * 0.3, size[2] * 0.8], [at[0] + size[0] * 0.5 + 0.01, y, 0.0]));
        (m, w)
    }

    basic!(_r, "boats-canoes", "boats", "canoes", "Canoes", "ground",
        [P::num("length", 4.8, 3.0, 6.5).unit("m").describe("Length (m)"), P::num("beam", 0.9, 0.6, 1.2).unit("m").describe("Beam (m)"), P::num("seats", 2.0, 1.0, 3.0).step(1.0).describe("Seats"), P::color("color", [0.55, 0.12, 0.04]).describe("Hull colour")],
        |v: &Values, _s: u32| {
            let (l, w, n) = (v.f("length"), v.f("beam"), v.f("seats").round() as u32);
            let d = 0.36;
            let hull = c::hull(l, w, d, 1.8, true, double_ended(0.12, 0.12));
            // Thwarts across and a paddle laid along the floor.
            let mut wood = Mesh::default();
            for i in 0..n {
                let x = ((i as f32 + 0.5) / n as f32 - 0.5) * l * 0.6;
                let half = w * 0.5 * (1.0 - (2.0 * x / l).abs().powf(2.2)).max(0.0).sqrt() * 0.95;
                geo::merge(&mut wood, &b::cbox([0.18, 0.03, half * 2.0], [x, d - 0.08, 0.0]));
            }
            geo::merge(&mut wood, &b::rod([-l * 0.25, 0.1, 0.12], [l * 0.1, 0.1, 0.12], 0.016, 8));
            geo::merge(&mut wood, &b::cbox([0.4, 0.015, 0.16], [l * 0.1 + 0.2, 0.1, 0.12]));
            Output::Parts(vec![Part::new(hull, v.color("color")).material("accent"), Part::new(wood, WOOD).material("ground")])
        });

    basic!(_r, "boats-kayaks", "boats", "kayaks", "Kayaks", "ground",
        [P::num("length", 4.2, 2.6, 5.5).unit("m").describe("Length (m)"), P::num("beam", 0.62, 0.5, 0.85).unit("m").describe("Beam (m)"), P::color("color", [0.85, 0.42, 0.0]).describe("Hull colour")],
        |v: &Values, _s: u32| {
            let (l, w) = (v.f("length"), v.f("beam"));
            // A closed deck with a cockpit rim; a twin-bladed paddle across it.
            let hull = c::hull(l, w, 0.32, 2.0, false, double_ended(0.08, 0.06));
            let mut dark = b::revolve(&[[0.0, 0.33], [0.3, 0.33], [0.3, 0.36], [0.26, 0.36], [0.26, 0.34], [0.0, 0.34]], 20);
            geo::transform(&mut dark, [1.4, 1.0, 0.9], [-0.15, 0.0, 0.0]);
            let mut paddle = b::rod([0.2, 0.42, -1.05], [0.2, 0.42, 1.05], 0.016, 8);
            for z in [-1.1f32, 1.1] {
                geo::merge(&mut paddle, &b::cbox([0.16, 0.01, 0.42], [0.2, 0.42, z]));
            }
            Output::Parts(vec![Part::new(hull, v.color("color")).material("accent"), Part::new(dark, DARK), Part::new(paddle, [0.1, 0.1, 0.12])])
        });

    basic!(_r, "boats-rowboats", "boats", "rowboats", "Rowboats", "ground",
        [P::num("length", 3.8, 2.5, 5.5).unit("m").describe("Length (m)"), P::num("oars", 20.0, -30.0, 60.0).unit("deg").describe("Oar sweep (degrees)"), P::color("color", [0.12, 0.3, 0.42]).describe("Hull colour")],
        |v: &Values, _s: u32| {
            let (l, oa) = (v.f("length"), v.f("oars").to_radians());
            let (w, d) = (l * 0.38, 0.55);
            let hull = c::hull(l, w, d, 1.6, true, shape(0.55, 0.2, 0.1));
            let mut wood = Mesh::default();
            for x in [-l * 0.3, 0.0, l * 0.22] {
                geo::merge(&mut wood, &b::cbox([0.22, 0.03, w * 0.9], [x, d - 0.12, 0.0]));
            }
            // Oars from rowlocks at the gunwales, swept by the angle.
            let mut oars = Mesh::default();
            for side in [-1.0f32, 1.0] {
                let lock = [0.0, d + 0.05, side * w * 0.48];
                let dir = [-oa.sin(), -0.25, side * oa.cos()];
                let tip = [lock[0] + dir[0] * 1.6, lock[1] + dir[1] * 1.6, lock[2] + dir[2] * 1.6];
                let inner = [lock[0] - dir[0] * 0.6, lock[1] - dir[1] * 0.6, lock[2] - dir[2] * 0.6];
                geo::merge(&mut oars, &b::rod(inner, tip, 0.02, 8));
                geo::merge(&mut oars, &b::bar(c::lerp3(lock, tip, 0.75), tip, 0.13, 0.015));
            }
            Output::Parts(vec![Part::new(hull, v.color("color")).material("accent"), Part::new(wood, WOOD).material("ground"), Part::new(oars, [0.5, 0.36, 0.2])])
        });

    basic!(_r, "boats-dinghies", "boats", "dinghies", "Dinghies", "ground",
        [P::num("length", 3.2, 2.2, 4.8).unit("m").describe("Length (m)"), P::num("tube", 0.22, 0.15, 0.32).unit("m").describe("Tube radius (m)"), P::color("color", [0.12, 0.13, 0.15]).describe("Tube colour")],
        |v: &Values, _s: u32| {
            let (l, r) = (v.f("length"), v.f("tube"));
            let w = l * 0.48;
            // An inflatable: a tube swept round a U plan closed by the transom.
            let mut plan: Vec<[f32; 3]> = vec![[-l * 0.5, r, -w * 0.5 + r]];
            for k in 0..=12 {
                let a = -sh::PI * 0.5 + k as f32 / 12.0 * sh::PI;
                plan.push([l * 0.5 - w * 0.5 + (w * 0.5 - r) * a.cos(), r + 0.05 * a.cos(), (w * 0.5 - r) * a.sin()]);
            }
            plan.push([-l * 0.5, r, w * 0.5 - r]);
            let tube = b::sweep(&plan, &b::circle(r, 14), true);
            let mut floor = b::cbox([l * 0.85, 0.05, w - r * 2.0], [-l * 0.02, 0.06, 0.0]);
            geo::merge(&mut floor, &b::cbox([0.06, r * 2.2, w - r * 1.6], [-l * 0.5 + 0.03, r * 1.1, 0.0]));
            // An outboard on the transom.
            let mut motor = b::rounded_box([0.32, 0.36, 0.26], 0.06, 2, [-l * 0.5 - 0.12, r * 2.0, 0.0]);
            geo::merge(&mut motor, &b::cbox([0.1, r * 2.0 + 0.2, 0.08], [-l * 0.5 - 0.12, r - 0.1, 0.0]));
            Output::Parts(vec![Part::new(tube, v.color("color")).material("ground"), Part::new(floor, [0.3, 0.3, 0.32]), Part::new(motor, DARK).material("accent")])
        });

    basic!(_r, "boats-fishing-boats", "boats", "fishing-boats", "Fishing boats", "ground",
        [P::num("length", 12.0, 7.0, 20.0).unit("m").describe("Length (m)"), P::num("cabin", 2.4, 1.6, 3.5).unit("m").describe("Wheelhouse height (m)"), P::color("color", [0.12, 0.25, 0.42]).describe("Hull colour"), P::color("house", [0.8, 0.8, 0.78]).describe("Wheelhouse colour")],
        |v: &Values, _s: u32| {
            let (l, ch) = (v.f("length"), v.f("cabin"));
            let (w, d) = (l * 0.33, l * 0.18);
            let hull = c::hull(l, w, d, 2.2, false, shape(0.7, d * 0.35, d * 0.25));
            let (hm, win) = house([l * 0.22, ch, w * 0.6], [l * 0.12, d], 3);
            // A mast with a boom over the working deck, a gantry at the stern.
            let mut rig = b::rod([l * 0.32, d, 0.0], [l * 0.32, d + ch * 2.6, 0.0], 0.08, 10);
            geo::merge(&mut rig, &b::rod([l * 0.32, d + ch * 0.9, 0.0], [-l * 0.2, d + ch * 1.6, 0.0], 0.05, 8));
            for z in [-w * 0.4, w * 0.4] {
                geo::merge(&mut rig, &b::rod([-l * 0.45, d, z], [-l * 0.45, d + ch * 1.4, z], 0.07, 8));
            }
            geo::merge(&mut rig, &b::rod([-l * 0.45, d + ch * 1.4, -w * 0.4], [-l * 0.45, d + ch * 1.4, w * 0.4], 0.07, 8));
            Output::Parts(vec![
                Part::new(hull, v.color("color")).material("accent"),
                Part::new(hm, v.color("house")).material("accent"),
                Part::new(win, GLASS).material("glass"),
                Part::new(rig, STEEL).material("metal"),
            ])
        });

    basic!(_r, "boats-sailing-boats", "boats", "sailing-boats", "Sailing boats", "ground",
        [P::num("length", 9.0, 5.0, 16.0).unit("m").describe("Length (m)"), P::num("mast", 1.4, 1.0, 2.0).describe("Mast height (share of the length)"), P::color("color", [0.8, 0.8, 0.78]).describe("Hull colour"), P::color("sail", SAIL).describe("Sail colour")],
        |v: &Values, _s: u32| {
            let (l, mh) = (v.f("length"), v.f("mast"));
            let (w, d) = (l * 0.32, l * 0.11);
            // A fin keel and rudder under a V hull (the keel stands on the ground).
            let keel_d = l * 0.18;
            let hull = moved(c::hull(l, w, d, 1.4, false, shape(0.65, d * 0.5, d * 0.2)), [0.0, keel_d, 0.0]);
            let mut fin = c::panel([l * 0.05, 0.0, 0.0], [0.0, 1.0, 0.0], keel_d + 0.1, l * 0.12, l * 0.18, -l * 0.04, 0.12);
            geo::merge(&mut fin, &c::ball([l * 0.1, l * 0.025, l * 0.03], [0.0, l * 0.025, 0.0]));
            let top = keel_d + d + l * mh;
            let mx = l * 0.1;
            let mut rig = b::rod([mx, keel_d + d, 0.0], [mx, top, 0.0], 0.06 * l / 9.0, 10);
            geo::merge(&mut rig, &b::rod([mx, keel_d + d + l * 0.1, 0.0], [-l * 0.38, keel_d + d + l * 0.1, 0.0], 0.05 * l / 9.0, 8));
            // Mainsail from mast to boom; a jib to the bow.
            let mut sails = sail(&[[mx - 0.05, keel_d + d + l * 0.12, 0.0], [-l * 0.37, keel_d + d + l * 0.12, 0.0], [mx - 0.05, top - 0.3, 0.0]]);
            geo::merge(&mut sails, &sail(&[[mx + 0.1, keel_d + d + l * 0.08, 0.0], [l * 0.48, keel_d + d + 0.1, 0.0], [mx + 0.1, top - l * 0.15, 0.0]]));
            let (cab, win) = house([l * 0.25, d * 0.5, w * 0.55], [-l * 0.08, keel_d + d], 3);
            geo::merge(&mut rig, &b::rod([l * 0.48, keel_d + d, 0.0], [mx, top, 0.0], 0.01, 4));
            Output::Parts(vec![
                Part::new(hull, v.color("color")).material("accent"),
                Part::new(cab, v.color("color")).material("accent"),
                Part::new(sails, v.color("sail")).material("ground"),
                Part::new(fin, DARK),
                Part::new(win, GLASS).material("glass"),
                Part::new(rig, STEEL).material("metal"),
            ])
        });

    basic!(_r, "boats-speedboats", "boats", "speedboats", "Speedboats", "ground",
        [P::num("length", 6.5, 4.5, 10.0).unit("m").describe("Length (m)"), P::num("vee", 1.2, 0.8, 2.0).describe("Hull fullness (1 a deep V, 2 round)"), P::color("color", [0.75, 0.05, 0.04]).describe("Hull colour")],
        |v: &Values, _s: u32| {
            let (l, p) = (v.f("length"), v.f("vee"));
            let (w, d) = (l * 0.36, l * 0.13);
            let hull = c::hull(l, w, d, p, false, shape(0.85, d * 0.4, d * 0.05));
            // A raked windscreen, seats, an outboard.
            let screen = b::extrude(&[[l * 0.12, d], [l * 0.05, d + 0.45], [l * 0.0, d + 0.45], [l * 0.08, d]], w * 0.8);
            let mut seats = Mesh::default();
            for x in [-l * 0.05, -l * 0.25] {
                geo::merge(&mut seats, &b::rounded_box([0.5, 0.35, w * 0.7], 0.08, 2, [x, d - 0.05, 0.0]));
            }
            let mut motor = b::rounded_box([0.5, 0.6, 0.4], 0.1, 2, [-l * 0.5 - 0.2, d - 0.1, 0.0]);
            geo::merge(&mut motor, &b::cbox([0.15, d * 0.9, 0.1], [-l * 0.5 - 0.2, d * 0.45, 0.0]));
            Output::Parts(vec![
                Part::new(hull, v.color("color")).material("accent"),
                Part::new(screen, GLASS).material("glass"),
                Part::new(seats, WHITE).material("ground"),
                Part::new(motor, DARK).material("accent"),
            ])
        });

    basic!(_r, "boats-yachts", "boats", "yachts", "Yachts", "ground",
        [P::num("length", 24.0, 14.0, 60.0).unit("m").describe("Length (m)"), P::num("decks", 2.0, 1.0, 4.0).step(1.0).describe("Decks above the hull"), P::color("color", [0.85, 0.85, 0.84]).describe("Hull colour")],
        |v: &Values, _s: u32| {
            let (l, n) = (v.f("length"), v.f("decks").round() as u32);
            let (w, d) = (l * 0.22, l * 0.11);
            let hull = c::hull(l, w, d, 2.0, false, shape(0.8, d * 0.45, d * 0.15));
            // Decks stepping back and in, each with a dark window band.
            let (mut sup, mut win) = (Mesh::default(), Mesh::default());
            let dh = l * 0.06;
            for k in 0..n {
                let s = 1.0 - k as f32 * 0.18;
                let (hm, wm) = house([l * 0.55 * s, dh, w * 0.82 * s], [-l * 0.08 - k as f32 * l * 0.04, d + k as f32 * dh], 1);
                geo::merge(&mut sup, &hm);
                geo::merge(&mut win, &wm);
            }
            geo::merge(&mut sup, &b::rod([-l * 0.12, d + n as f32 * dh, 0.0], [-l * 0.12, d + n as f32 * dh + l * 0.08, 0.0], l * 0.006, 8));
            Output::Parts(vec![Part::new(hull, v.color("color")).material("accent"), Part::new(sup, WHITE).material("accent"), Part::new(win, GLASS).material("glass")])
        });

    basic!(_r, "boats-ferries", "boats", "ferries", "Ferries", "ground",
        [P::num("length", 60.0, 30.0, 140.0).unit("m").describe("Length (m)"), P::num("decks", 3.0, 1.0, 6.0).step(1.0).describe("Passenger decks"), P::color("color", [0.1, 0.18, 0.4]).describe("Hull colour")],
        |v: &Values, _s: u32| {
            let (l, n) = (v.f("length"), v.f("decks").round() as u32);
            let (w, d) = (l * 0.2, l * 0.08);
            let hull = c::hull(l, w, d, 3.0, false, shape(0.9, d * 0.4, d * 0.05));
            let (mut sup, mut win) = (Mesh::default(), Mesh::default());
            let dh = 2.8;
            for k in 0..n {
                let (hm, wm) = house([l * 0.7, dh, w * 0.92], [-l * 0.02, d + k as f32 * dh], (l / 6.0) as u32);
                geo::merge(&mut sup, &hm);
                geo::merge(&mut win, &wm);
            }
            let top = d + n as f32 * dh;
            let funnel = b::rounded_box([l * 0.08, l * 0.06, w * 0.3], 0.3, 2, [-l * 0.18, top, 0.0]);
            let bridge = house([l * 0.12, dh, w], [l * 0.25, top], 4);
            geo::merge(&mut sup, &bridge.0);
            geo::merge(&mut win, &bridge.1);
            Output::Parts(vec![Part::new(hull, v.color("color")).material("accent"), Part::new(sup, WHITE).material("accent"), Part::new(win, GLASS).material("glass"), Part::new(funnel, [0.65, 0.08, 0.05]).material("accent")])
        });

    basic!(_r, "boats-cargo-ships", "boats", "cargo-ships", "Cargo ships", "ground",
        [P::num("length", 120.0, 60.0, 300.0).unit("m").describe("Length (m)"), P::num("stack", 3.0, 0.0, 7.0).step(1.0).describe("Containers stacked on deck"), P::color("color", [0.35, 0.08, 0.05]).describe("Hull colour")],
        |v: &Values, s: u32| {
            let (l, n) = (v.f("length"), v.f("stack").round() as u32);
            let (w, d) = (l * 0.15, l * 0.09);
            let hull = c::hull(l, w, d, 4.0, false, shape(0.85, d * 0.35, d * 0.06));
            // The bridge block aft; rows of seeded-colour containers forward.
            let (br, bw) = house([l * 0.08, l * 0.1, w * 0.95], [-l * 0.36, d], 4);
            let palette = [[0.55, 0.08, 0.05], [0.08, 0.22, 0.45], [0.12, 0.35, 0.15], [0.7, 0.45, 0.05], [0.4, 0.4, 0.42]];
            let mut boxes: Vec<Mesh> = vec![Mesh::default(); palette.len()];
            let (cl, cw, chh) = (12.2, 2.44, 2.6);
            let rows = ((l * 0.62) / (cl + 0.4)) as u32;
            let cols = ((w * 0.85) / cw) as u32;
            for i in 0..rows {
                for j in 0..cols.max(1) {
                    for k in 0..n {
                        let x = -l * 0.28 + (i as f32 + 0.5) * (cl + 0.4);
                        let z = (j as f32 + 0.5 - cols as f32 * 0.5) * cw;
                        let pick = (sh::rnd(s, i * 131 + j * 17 + k * 7, 1) * palette.len() as f32) as usize % palette.len();
                        geo::merge(&mut boxes[pick], &b::cbox([cl, chh - 0.05, cw - 0.06], [x, d + chh * (k as f32 + 0.5), z]));
                    }
                }
            }
            let mut ps = vec![Part::new(hull, v.color("color")).material("accent"), Part::new(br, WHITE).material("accent"), Part::new(bw, GLASS).material("glass")];
            for (m, col) in boxes.into_iter().zip(palette) {
                ps.push(Part::new(m, col).material("ground"));
            }
            Output::Parts(ps)
        });

    basic!(_r, "boats-tugboats", "boats", "tugboats", "Tugboats", "ground",
        [P::num("length", 25.0, 15.0, 40.0).unit("m").describe("Length (m)"), P::num("house", 1.0, 0.7, 1.4).describe("Wheelhouse size"), P::color("color", [0.12, 0.12, 0.13]).describe("Hull colour"), P::color("house_color", [0.72, 0.1, 0.05]).describe("Wheelhouse colour")],
        |v: &Values, _s: u32| {
            let (l, k) = (v.f("length"), v.f("house"));
            let (w, d) = (l * 0.38, l * 0.15);
            let hull = c::hull(l, w, d, 2.4, false, shape(0.8, d * 0.35, d * 0.25));
            // A tall wheelhouse forward, a funnel, a ring of tyre fenders.
            let (h1, w1) = house([l * 0.3 * k, l * 0.1 * k, w * 0.6], [l * 0.08, d], 2);
            let (h2, w2) = house([l * 0.16 * k, l * 0.09 * k, w * 0.5], [l * 0.12, d + l * 0.1 * k], 3);
            let mut sup = h1;
            geo::merge(&mut sup, &h2);
            let mut win = w1;
            geo::merge(&mut win, &w2);
            let funnel = b::rounded_box([l * 0.08, l * 0.12, w * 0.22], 0.2, 2, [-l * 0.12, d, 0.0]);
            let mut fenders = Mesh::default();
            for i in 0..10 {
                let t = (i as f32 + 0.5) / 10.0;
                let half = w * 0.5 * shape(0.8, 0.0, 0.0)(t)[0];
                for side in [-1.0f32, 1.0] {
                    let x = (t - 0.5) * l * 0.9;
                    let tyre = c::wheel([0.0, 0.0, 0.0], l * 0.022, l * 0.014, 0).0;
                    geo::merge(&mut fenders, &moved(tyre, [x, d * 0.75, side * (half + l * 0.012)]));
                }
            }
            Output::Parts(vec![
                Part::new(hull, v.color("color")).material("accent"),
                Part::new(sup, v.color("house_color")).material("accent"),
                Part::new(win, GLASS).material("glass"),
                Part::new(funnel, [0.1, 0.1, 0.1]),
                Part::new(fenders, [0.03, 0.03, 0.035]).material("ground"),
            ])
        });

    basic!(_r, "boats-pirate-ships", "boats", "pirate-ships", "Pirate ships", "ground",
        [P::num("length", 30.0, 18.0, 50.0).unit("m").describe("Length (m)"), P::num("masts", 3.0, 1.0, 3.0).step(1.0).describe("Masts"), P::color("color", [0.28, 0.16, 0.07]).describe("Hull colour"), P::color("sail", [0.78, 0.74, 0.62]).describe("Sail colour")],
        |v: &Values, _s: u32| {
            let (l, n) = (v.f("length"), v.f("masts").round() as u32);
            let (w, d) = (l * 0.28, l * 0.16);
            // A round hull with a high sterncastle and a bowsprit.
            let hull = c::hull(l, w, d, 1.8, false, |t| { let s = shape(0.7, l * 0.02, 0.0)(t); [s[0], s[1], if t < 0.25 { d * 0.45 * (1.0 - t / 0.25) } else { d * 0.25 * ((t - 0.25) / 0.75).powi(2) }] });
            let mut wood = b::cbox([l * 0.2, d * 0.35, w * 0.75], [-l * 0.38, d * 1.45 + d * 0.17, 0.0]);
            geo::merge(&mut wood, &b::rod([l * 0.45, d * 1.15, 0.0], [l * 0.75, d * 1.6, 0.0], l * 0.008, 8));
            let mut sails = Mesh::default();
            for i in 0..n {
                let x = if n == 1 { 0.0 } else { l * 0.3 - i as f32 * l * 0.55 / (n - 1) as f32 };
                let h = l * if i == 0 && n > 1 { 0.85 } else { 1.0 } * 0.9;
                geo::merge(&mut wood, &b::rod([x, d, 0.0], [x, d + h, 0.0], l * 0.01, 8));
                // Two square sails on yards.
                for (y0, y1, half) in [(d + h * 0.3, d + h * 0.6, w * 0.55), (d + h * 0.64, d + h * 0.9, w * 0.42)] {
                    geo::merge(&mut wood, &b::rod([x + 0.3, y1, -half], [x + 0.3, y1, half], l * 0.005, 6));
                    let belly = half * 0.15;
                    geo::merge(&mut sails, &sail(&[[x + 0.3 + belly, y0, -half * 0.95], [x + 0.3 + belly, y0, half * 0.95], [x + 0.3, y1, half], [x + 0.3, y1, -half]]));
                }
            }
            let mut ports = Mesh::default();
            for i in 0..8 {
                let x = -l * 0.3 + i as f32 * l * 0.08;
                let half = w * 0.5 * shape(0.7, 0.0, 0.0)((x / l) + 0.5)[0];
                for z in [-half - 0.02, half + 0.02] {
                    geo::merge(&mut ports, &b::cbox([0.8, 0.7, 0.06], [x, d * 0.72, z]));
                }
            }
            Output::Parts(vec![
                Part::new(hull, v.color("color")).material("ground"),
                Part::new(wood, [0.2, 0.12, 0.06]).material("ground"),
                Part::new(sails, v.color("sail")).material("ground"),
                Part::new(ports, DARK),
            ])
        });

    basic!(_r, "boats-submarines", "boats", "submarines", "Submarines", "metal",
        [P::num("length", 70.0, 30.0, 170.0).unit("m").describe("Length (m)"), P::num("sail", 1.0, 0.5, 1.8).describe("Sail (tower) size"), P::color("color", [0.1, 0.1, 0.11]).describe("Hull colour")],
        |v: &Values, _s: u32| {
            let (l, k) = (v.f("length"), v.f("sail"));
            let r = l * 0.055;
            // A cigar body on the ground, a sail with planes, cruciform tail, prop.
            let mut body = c::spindle(-l * 0.5, l * 0.5, r, 1.0, |t| r * c::streamline(t, 0.12, 0.35, 0.08));
            let (sl, sh_) = (l * 0.12 * k, r * 1.4 * k);
            geo::merge(&mut body, &c::panel([l * 0.18, r * 1.7, 0.0], [0.0, 1.0, 0.0], sh_, sl, sl * 0.85, sl * 0.05, 0.25));
            geo::merge(&mut body, &c::panels([l * 0.15, r * 1.7 + sh_ * 0.6, 0.0], [0.0, 0.0, 1.0], r * 0.9, [sl * 0.35, sl * 0.25], sl * 0.05, 0.12));
            geo::merge(&mut body, &c::panels([-l * 0.42, r, 0.0], [0.0, 0.0, 1.0], r * 0.9, [l * 0.06, l * 0.04], l * 0.01, 0.12));
            geo::merge(&mut body, &c::panel([-l * 0.42, r, 0.0], [0.0, 1.0, 0.0], r * 0.9, l * 0.06, l * 0.04, l * 0.01, 0.12));
            let mut prop = Mesh::default();
            for i in 0..7 {
                let a = i as f32 / 7.0 * sh::TAU;
                let mut blade = b::cbox([r * 0.05, r * 0.5, r * 0.18], [0.0, r * 0.25, 0.0]);
                sh::rotate(&mut blade, [0.0, 1.0, 0.0], 0.4);
                sh::rotate(&mut blade, [1.0, 0.0, 0.0], a);
                geo::merge(&mut prop, &moved(blade, [-l * 0.5 - r * 0.1, r, 0.0]));
            }
            Output::Parts(vec![Part::new(body, v.color("color")), Part::new(prop, [0.45, 0.35, 0.15]).material("metal")])
        });

    basic!(_r, "boats-rafts", "boats", "rafts", "Rafts", "ground",
        [P::num("logs", 7.0, 4.0, 14.0).step(1.0).describe("Logs"), P::num("length", 4.0, 2.0, 7.0).unit("m").describe("Length (m)"), P::num("mast", 1.0, 0.0, 1.0).step(1.0).describe("A mast and sail (1) or none (0)"), P::color("color", [0.38, 0.26, 0.13]).describe("Log colour")],
        |v: &Values, s: u32| {
            let (n, l, mast) = (v.f("logs").round() as u32, v.f("length"), v.f("mast") > 0.5);
            let r = 0.15;
            let mut logs = Mesh::default();
            for i in 0..n {
                let z = (i as f32 - (n - 1) as f32 * 0.5) * r * 2.0;
                let ll = l * sh::range(s, i, 1, 0.9, 1.0);
                let off = sh::range(s, i, 2, -0.1, 0.1);
                geo::merge(&mut logs, &b::rod([-ll * 0.5 + off, r, z], [ll * 0.5 + off, r, z], r * sh::range(s, i, 3, 0.85, 1.0), 10));
            }
            // Lashings across, and a mast with a square sail.
            let half = n as f32 * r;
            let mut rope = Mesh::default();
            for x in [-l * 0.35, 0.0, l * 0.35] {
                geo::merge(&mut rope, &b::rod([x, r * 2.0, -half], [x, r * 2.0, half], 0.025, 6));
            }
            let mut sailm = Mesh::default();
            if mast {
                geo::merge(&mut logs, &b::rod([0.0, r * 2.0, 0.0], [0.0, r * 2.0 + l * 0.75, 0.0], 0.05, 8));
                geo::merge(&mut logs, &b::rod([0.0, r * 2.0 + l * 0.72, -l * 0.3], [0.0, r * 2.0 + l * 0.72, l * 0.3], 0.03, 6));
                sailm = sail(&[[0.02, r * 2.0 + l * 0.3, -l * 0.28], [0.02, r * 2.0 + l * 0.3, l * 0.28], [0.02, r * 2.0 + l * 0.71, l * 0.29], [0.02, r * 2.0 + l * 0.71, -l * 0.29]]);
            }
            Output::Parts(vec![Part::new(logs, v.color("color")).material("ground"), Part::new(rope, [0.55, 0.46, 0.3]), Part::new(sailm, SAIL).material("ground")])
        });

    basic!(_r, "boats-landing-craft", "boats", "landing-craft", "Landing craft", "metal",
        [P::num("length", 15.0, 9.0, 25.0).unit("m").describe("Length (m)"), P::num("ramp", 0.0, 0.0, 70.0).unit("deg").describe("Ramp lowered (degrees)"), P::color("color", [0.28, 0.32, 0.3]).describe("Hull colour")],
        |v: &Values, _s: u32| {
            let (l, ra) = (v.f("length"), v.f("ramp").to_radians());
            let (w, d) = (l * 0.28, 2.2);
            // A flat-bottomed open box with high walls, a bow ramp hinged at the floor.
            let mut hull = b::cbox([l, 0.3, w], [0.0, 0.15, 0.0]);
            for z in [-w * 0.5 + 0.1, w * 0.5 - 0.1] {
                geo::merge(&mut hull, &b::cbox([l, d, 0.2], [0.0, d * 0.5, z]));
            }
            let mut house_m = b::cbox([l * 0.18, d * 1.6, w], [-l * 0.5 + l * 0.09, d * 0.8, 0.0]);
            geo::merge(&mut house_m, &b::cbox([l * 0.08, d * 0.7, w * 0.5], [-l * 0.5 + l * 0.08, d * 1.6 + d * 0.35, 0.0]));
            let rl = d;
            let hinge = [l * 0.5, 0.3, 0.0];
            let tip = [hinge[0] + rl * ra.sin(), hinge[1] + rl * ra.cos(), 0.0];
            let ramp = b::sweep(&[[hinge[0], hinge[1], w * 0.5 - 0.2], [hinge[0], hinge[1], -w * 0.5 + 0.2]], &[[0.0, 0.0], [tip[0] - hinge[0], tip[1] - hinge[1]], [tip[0] - hinge[0] + 0.15 * ra.cos(), tip[1] - hinge[1] - 0.15 * ra.sin()], [0.15, 0.0]], true);
            let win = b::cbox([0.05, d * 0.25, w * 0.4], [-l * 0.5 + l * 0.12 + 0.03, d * 1.9, 0.0]);
            Output::Parts(vec![Part::new(hull, v.color("color")), Part::new(house_m, v.color("color")), Part::new(ramp, [0.22, 0.25, 0.23]), Part::new(win, GLASS).material("glass")])
        });
}
