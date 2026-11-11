//! Category 16, Aircraft and spacecraft: basic generators (batch C).
//! Fuselages are round bodies lofted along x, wings, tails and fins the
//! base's airfoil panels, propellers and rotors thin blades turned round
//! their hub, balloons and rockets turned profiles. Nose at +x; each
//! stands on its wheels, skids, floats or base. Inspired by three.js (MIT)
//! LatheGeometry, ExtrudeGeometry and TubeGeometry, and the "fuselage,
//! wing, tail" part breakdown of low-poly aircraft; written fresh here.

use evani_procedural::Registry;

#[allow(unused_imports, dead_code)]
pub fn register(_r: &mut Registry) {
    use evani_procedural::{Output, ParamSpec as P, Part, Values, build as b, craft as c, geo, shapes as sh};
    type Mesh = geo::Mesh;
    const RUBBER: [f32; 3] = [0.03, 0.03, 0.035];
    const DARK: [f32; 3] = [0.06, 0.06, 0.07];
    const GLASS: [f32; 3] = [0.1, 0.14, 0.2];
    const STEEL: [f32; 3] = [0.35, 0.36, 0.38];
    const WHITE: [f32; 3] = [0.78, 0.79, 0.8];

    fn moved(mut m: Mesh, d: [f32; 3]) -> Mesh {
        geo::transform(&mut m, [1.0; 3], d);
        m
    }

    /// A round fuselage `len` long of radius `r` at height `y`, its nose
    /// rounding over `nose` and its tail closing to `tail` over `back`.
    fn fuselage(len: f32, r: f32, y: f32, nose: f32, back: f32, tail: f32) -> Mesh {
        c::spindle(-len * 0.5, len * 0.5, y, 1.0, |t| r * c::streamline(t, nose, back, tail))
    }

    /// Both wings from the centre line: half-span `half`, root and tip
    /// chords, the tip swept back by `sweep` (m), raised by `dih` (radians).
    fn wings(x: f32, y: f32, half: f32, chords: [f32; 2], sweep: f32, dih: f32) -> Mesh {
        c::panels([x, y, 0.0], [0.0, dih.sin(), dih.cos()], half, chords, sweep, 0.12)
    }

    /// A fin standing up from `(x, y)`.
    fn fin(x: f32, y: f32, h: f32, chords: [f32; 2], sweep: f32) -> Mesh {
        c::panel([x, y, 0.0], [0.0, 1.0, 0.0], h, chords[0], chords[1], sweep, 0.1)
    }

    /// A propeller facing +x at `at`: `n` blades of radius `r` and a spinner.
    fn prop(at: [f32; 3], r: f32, n: u32) -> Mesh {
        let mut m = c::cone([at[0] - r * 0.1, at[1], at[2]], [at[0] + r * 0.3, at[1], at[2]], r * 0.16, 12);
        for i in 0..n {
            let mut blade = b::cbox([r * 0.03, r, r * 0.16], [0.0, r * 0.5, 0.0]);
            sh::rotate(&mut blade, [0.0, 1.0, 0.0], 0.25);
            sh::rotate(&mut blade, [1.0, 0.0, 0.0], i as f32 / n as f32 * sh::TAU + 0.4);
            geo::merge(&mut m, &moved(blade, at));
        }
        m
    }

    /// A rotor turning about y at `at`: `n` blades of radius `r`, a hub.
    fn rotor(at: [f32; 3], r: f32, n: u32, chord: f32) -> Mesh {
        let mut m = b::rod([at[0], at[1] - chord * 0.6, at[2]], [at[0], at[1] + chord * 0.4, at[2]], chord * 0.5, 12);
        for i in 0..n {
            let blade = b::cbox([r, chord * 0.12, chord], [r * 0.5, 0.0, 0.0]);
            geo::merge(&mut m, &sh::place(&blade, 1.0, i as f32 / n as f32 * sh::TAU + 0.3, 0.0, at));
        }
        m
    }

    /// A gear leg at (x, z) from height `h` to a wheel of radius `r`: (strut, tyre).
    fn leg(x: f32, z: f32, h: f32, r: f32) -> (Mesh, Mesh) {
        let strut = b::rod([x, h, z], [x, r, z], r * 0.18, 8);
        let (t, hub) = c::wheel([x, r, z], r, r * 0.6, 0);
        let mut s = strut;
        geo::merge(&mut s, &hub);
        (s, t)
    }

    /// A row of windows on both sides of a fuselage of radius `r` at height
    /// `y`, `n` of them from `x0` to `x1`.
    fn portholes(x0: f32, x1: f32, n: u32, y: f32, r: f32, size: f32) -> Mesh {
        let mut m = Mesh::default();
        for i in 0..n {
            let x = x0 + (x1 - x0) * (i as f32 + 0.5) / n as f32;
            for z in [-r * 0.99, r * 0.99] {
                geo::merge(&mut m, &b::cbox([size, size * 1.3, r * 0.08], [x, y + r * 0.2, z]));
            }
        }
        m
    }

    basic!(_r, "aircraft-biplanes", "aircraft", "biplanes", "Biplanes", "accent",
        [P::num("span", 8.0, 6.0, 11.0).unit("m").describe("Wing span (m)"), P::num("gap", 1.4, 1.0, 2.0).unit("m").describe("Gap between the wings (m)"), P::color("color", [0.55, 0.06, 0.04]).describe("Fuselage colour"), P::color("wing", [0.75, 0.68, 0.45]).describe("Wing colour")],
        |v: &Values, _s: u32| {
            let (span, gap) = (v.f("span"), v.f("gap"));
            let (l, r, y) = (6.5, 0.5, 1.6);
            let body = fuselage(l, r, y, 0.12, 0.5, 0.15);
            let mut wing = wings(1.4, y - 0.4, span * 0.5, [1.5, 1.3], 0.1, 0.03);
            geo::merge(&mut wing, &wings(1.7, y - 0.4 + gap, span * 0.5, [1.5, 1.3], 0.1, 0.02));
            geo::merge(&mut wing, &wings(-2.7, y + 0.05, 1.4, [0.8, 0.5], 0.2, 0.0));
            geo::merge(&mut wing, &fin(-2.6, y + 0.2, 1.0, [1.0, 0.5], 0.5));
            // Struts between the wings and from the fuselage.
            let mut struts = Mesh::default();
            for z in [-span * 0.36, span * 0.36, -0.5, 0.5] {
                geo::merge(&mut struts, &b::rod([1.0, y - 0.4, z], [1.3, y - 0.4 + gap, z], 0.035, 6));
                geo::merge(&mut struts, &b::rod([1.8, y - 0.4, z], [2.1, y - 0.4 + gap, z], 0.035, 6));
            }
            let (s1, t1) = leg(1.6, 0.8, y - 0.3, 0.35);
            let (s2, t2) = leg(1.6, -0.8, y - 0.3, 0.35);
            geo::merge(&mut struts, &s1);
            geo::merge(&mut struts, &s2);
            let mut tyres = t1;
            geo::merge(&mut tyres, &t2);
            geo::merge(&mut struts, &b::rod([-2.9, y - 0.3, 0.0], [-3.0, 0.1, 0.0], 0.04, 6));
            let p = prop([l * 0.5 + 0.1, y, 0.0], 1.1, 2);
            Output::Parts(vec![
                Part::new(body, v.color("color")).material("accent"),
                Part::new(wing, v.color("wing")).material("ground"),
                Part::new(struts, DARK),
                Part::new(tyres, RUBBER).material("ground"),
                Part::new(p, [0.3, 0.2, 0.1]),
            ])
        });

    basic!(_r, "aircraft-light-aircraft", "aircraft", "light-aircraft", "Light aircraft", "accent",
        [P::num("span", 11.0, 8.0, 13.0).unit("m").describe("Wing span (m)"), P::num("length", 8.0, 6.5, 10.0).unit("m").describe("Length (m)"), P::color("color", [0.78, 0.79, 0.8]).describe("Body colour"), P::color("trim", [0.05, 0.15, 0.5]).describe("Trim colour")],
        |v: &Values, _s: u32| {
            let (span, l) = (v.f("span"), v.f("length"));
            let (r, y) = (0.6, 1.3);
            // A high wing on a cabin; tricycle gear.
            let mut body = fuselage(l, r, y, 0.15, 0.55, 0.12);
            geo::merge(&mut body, &wings(l * 0.1, y + r * 0.95, span * 0.5, [1.5, 1.1], 0.05, 0.03));
            geo::merge(&mut body, &wings(-l * 0.42, y + 0.1, 1.7, [0.9, 0.6], 0.15, 0.0));
            let tail = fin(-l * 0.4, y + 0.3, 1.3, [1.3, 0.7], 0.6);
            let glass = c::ball([1.0, 0.45, r * 0.95], [l * 0.14, y + 0.3, 0.0]);
            let mut struts = Mesh::default();
            for z in [-1.0f32, 1.0] {
                geo::merge(&mut struts, &b::rod([l * 0.05, y - r * 0.5, z * r * 0.8], [l * 0.08, y + r * 0.95, z * 2.0], 0.04, 6));
            }
            let mut tyres = Mesh::default();
            for (x, z) in [(l * 0.32, 0.0), (-l * 0.02, 1.1), (-l * 0.02, -1.1)] {
                let (s, t) = leg(x, z, y - r * 0.5, 0.25);
                geo::merge(&mut struts, &s);
                geo::merge(&mut tyres, &t);
            }
            let p = prop([l * 0.5 + 0.1, y, 0.0], 0.9, 2);
            Output::Parts(vec![
                Part::new(body, v.color("color")).material("accent"),
                Part::new(tail, v.color("trim")).material("accent"),
                Part::new(glass, GLASS).material("glass"),
                Part::new(struts, STEEL).material("metal"),
                Part::new(tyres, RUBBER).material("ground"),
                Part::new(p, DARK),
            ])
        });

    basic!(_r, "aircraft-propeller-passenger-aircraft", "aircraft", "propeller-passenger-aircraft", "Propeller passenger aircraft", "accent",
        [P::num("length", 24.0, 16.0, 32.0).unit("m").describe("Length (m)"), P::num("span", 26.0, 18.0, 32.0).unit("m").describe("Wing span (m)"), P::color("color", [0.78, 0.79, 0.8]).describe("Body colour"), P::color("trim", [0.6, 0.06, 0.05]).describe("Tail colour")],
        |v: &Values, _s: u32| {
            let (l, span) = (v.f("length"), v.f("span"));
            let (r, y) = (1.35, 2.0);
            let mut body = fuselage(l, r, y, 0.1, 0.3, 0.15);
            geo::merge(&mut body, &wings(l * 0.05, y + r * 0.9, span * 0.5, [2.6, 1.4], 0.4, 0.02));
            // Two nacelles under the wing, each with a six-blade prop.
            let mut props = Mesh::default();
            for z in [-span * 0.17, span * 0.17] {
                geo::merge(&mut body, &moved(c::spindle(-2.0, 2.2, 0.0, 1.0, |t| 0.6 * c::streamline(t, 0.2, 0.4, 0.3)), [l * 0.05, y + r * 0.7, z]));
                geo::merge(&mut props, &prop([l * 0.05 + 2.3, y + r * 0.7, z], 1.9, 6));
            }
            // A T-tail.
            let mut tail = fin(-l * 0.42, y + r * 0.5, 4.2, [4.0, 2.2], 2.5);
            geo::merge(&mut tail, &wings(-l * 0.42 - 2.4, y + r * 0.5 + 4.2, 4.0, [2.0, 1.1], 0.6, 0.0));
            let win = portholes(-l * 0.32, l * 0.32, (l / 0.9) as u32, y, r, 0.3);
            let mut gear = Mesh::default();
            let mut tyres = Mesh::default();
            for (x, z) in [(l * 0.38, 0.0), (-l * 0.02, 1.4), (-l * 0.02, -1.4)] {
                let (s, t) = leg(x, z, y - r * 0.6, 0.45);
                geo::merge(&mut gear, &s);
                geo::merge(&mut tyres, &t);
            }
            Output::Parts(vec![
                Part::new(body, v.color("color")).material("accent"),
                Part::new(tail, v.color("trim")).material("accent"),
                Part::new(win, DARK),
                Part::new(props, DARK),
                Part::new(gear, STEEL).material("metal"),
                Part::new(tyres, RUBBER).material("ground"),
            ])
        });

    basic!(_r, "aircraft-commercial-jetliners", "aircraft", "commercial-jetliners", "Commercial jetliners", "accent",
        [P::num("length", 40.0, 30.0, 70.0).unit("m").describe("Length (m)"), P::num("span", 36.0, 28.0, 65.0).unit("m").describe("Wing span (m)"), P::num("engines", 2.0, 2.0, 4.0).step(2.0).describe("Engines"), P::color("color", [0.8, 0.8, 0.82]).describe("Body colour")],
        |v: &Values, _s: u32| {
            let (l, span, n) = (v.f("length"), v.f("span"), v.f("engines").round() as u32);
            let r = l * 0.05;
            let y = r + 2.0;
            let mut body = fuselage(l, r, y, 0.08, 0.28, 0.12);
            // A low wing swept back; the tail and fin.
            geo::merge(&mut body, &wings(l * 0.06, y - r * 0.6, span * 0.5, [l * 0.18, l * 0.05], span * 0.28, 0.1));
            geo::merge(&mut body, &wings(-l * 0.4, y + r * 0.2, span * 0.18, [l * 0.08, l * 0.03], span * 0.12, 0.08));
            let tail = fin(-l * 0.38, y + r * 0.6, l * 0.2, [l * 0.12, l * 0.04], l * 0.1);
            // Engine pods hung under the wing.
            let mut pods = Mesh::default();
            let spots: Vec<f32> = if n >= 4 { vec![0.33, 0.62] } else { vec![0.36] };
            for k in spots {
                for side in [-1.0f32, 1.0] {
                    let z = side * span * 0.5 * k;
                    let x = l * 0.06 - span * 0.28 * k + l * 0.06;
                    let yy = y - r * 0.6 + span * 0.5 * k * 0.1 - r * 0.55;
                    geo::merge(&mut pods, &moved(c::spindle(-l * 0.05, l * 0.05, 0.0, 1.0, |t| r * 0.55 * (0.85 + 0.15 * (t * sh::PI).sin())), [x, yy, z]));
                }
            }
            let win = portholes(-l * 0.36, l * 0.38, (l / 1.0) as u32, y, r, 0.35);
            let mut gear = Mesh::default();
            let mut tyres = Mesh::default();
            for (x, z) in [(l * 0.4, 0.0), (0.0, r * 1.2), (0.0, -r * 1.2)] {
                let (s, t) = leg(x, z, y - r * 0.7, 0.6);
                geo::merge(&mut gear, &s);
                geo::merge(&mut tyres, &t);
            }
            Output::Parts(vec![
                Part::new(body, v.color("color")).material("accent"),
                Part::new(tail, [0.05, 0.15, 0.45]).material("accent"),
                Part::new(pods, [0.55, 0.56, 0.58]).material("metal"),
                Part::new(win, DARK),
                Part::new(gear, STEEL).material("metal"),
                Part::new(tyres, RUBBER).material("ground"),
            ])
        });

    basic!(_r, "aircraft-cargo-aircraft", "aircraft", "cargo-aircraft", "Cargo aircraft", "ground",
        [P::num("length", 35.0, 25.0, 50.0).unit("m").describe("Length (m)"), P::num("span", 40.0, 30.0, 52.0).unit("m").describe("Wing span (m)"), P::color("color", [0.32, 0.36, 0.3]).describe("Body colour")],
        |v: &Values, _s: u32| {
            let (l, span) = (v.f("length"), v.f("span"));
            let r = l * 0.065;
            let y = r + 1.2;
            // A wide body whose tail sweeps up to a loading ramp; high wing, T-tail.
            let mut body = c::loft_x(-l * 0.5, l * 0.5, 22, |t| y + if t < 0.3 { r * 0.9 * (1.0 - t / 0.3).powi(2) } else { 0.0 }, |t| {
                let k = r * c::streamline(t, 0.1, 0.32, 0.12);
                c::squircle(k, k, 2.4, 20)
            });
            geo::merge(&mut body, &wings(l * 0.04, y + r * 0.95, span * 0.5, [l * 0.14, l * 0.06], span * 0.08, -0.03));
            geo::merge(&mut body, &fin(-l * 0.42, y + r * 1.2, l * 0.2, [l * 0.13, l * 0.07], l * 0.1));
            geo::merge(&mut body, &wings(-l * 0.44 - l * 0.08, y + r * 1.2 + l * 0.2, span * 0.16, [l * 0.07, l * 0.04], l * 0.03, 0.0));
            let mut props = Mesh::default();
            for k in [0.22f32, 0.42] {
                for side in [-1.0f32, 1.0] {
                    let z = side * span * 0.5 * k;
                    let x = l * 0.04 - span * 0.08 * k + 1.5;
                    geo::merge(&mut body, &moved(c::spindle(-3.0, 2.5, 0.0, 1.0, |t| 0.8 * c::streamline(t, 0.2, 0.4, 0.3)), [x, y + r * 0.75, z]));
                    geo::merge(&mut props, &prop([x + 2.6, y + r * 0.75, z], 2.2, 4));
                }
            }
            let mut gear = Mesh::default();
            let mut tyres = Mesh::default();
            for (x, z) in [(l * 0.4, 0.0), (0.0, r * 0.9), (0.0, -r * 0.9), (-3.0, r * 0.9), (-3.0, -r * 0.9)] {
                let (s, t) = leg(x, z, y - r * 0.7, 0.6);
                geo::merge(&mut gear, &s);
                geo::merge(&mut tyres, &t);
            }
            let glass = c::ball([1.2, 0.5, r * 0.7], [l * 0.46, y + r * 0.45, 0.0]);
            Output::Parts(vec![
                Part::new(body, v.color("color")),
                Part::new(props, DARK),
                Part::new(glass, GLASS).material("glass"),
                Part::new(gear, STEEL).material("metal"),
                Part::new(tyres, RUBBER).material("ground"),
            ])
        });

    basic!(_r, "aircraft-fighter-jets", "aircraft", "fighter-jets", "Fighter jets", "metal",
        [P::num("length", 16.0, 12.0, 22.0).unit("m").describe("Length (m)"), P::num("sweep", 40.0, 20.0, 65.0).unit("deg").describe("Wing sweep (degrees)"), P::num("fins", 2.0, 1.0, 2.0).step(1.0).describe("Tail fins"), P::color("color", [0.32, 0.35, 0.38]).describe("Paint colour")],
        |v: &Values, _s: u32| {
            let (l, sw, n) = (v.f("length"), v.f("sweep").to_radians(), v.f("fins").round() as u32);
            let (r, y) = (l * 0.055, 1.9);
            // A pointed body with intakes either side; a swept wing near the back.
            let mut body = c::spindle(-l * 0.5, l * 0.5, y, 0.8, |t| r * if t > 0.6 { ((1.0 - t) / 0.4).powf(0.7) } else { 0.75 + 0.25 * (t / 0.6).min(1.0) });
            for z in [-r * 0.9, r * 0.9] {
                geo::merge(&mut body, &b::cbox([l * 0.35, r * 1.1, r * 0.7], [-l * 0.05, y - r * 0.15, z]));
            }
            let half = l * 0.36;
            geo::merge(&mut body, &wings(l * 0.1, y - r * 0.2, half, [l * 0.38, l * 0.06], half * sw.tan(), -0.03));
            geo::merge(&mut body, &wings(-l * 0.36, y - r * 0.1, l * 0.17, [l * 0.14, l * 0.05], l * 0.08, -0.05));
            let mut fins = Mesh::default();
            for k in 0..n {
                let z = if n == 1 { 0.0 } else if k == 0 { -r * 0.9 } else { r * 0.9 };
                let mut f = fin(-l * 0.32, 0.0, l * 0.2, [l * 0.15, l * 0.05], l * 0.1);
                if n == 2 {
                    sh::rotate(&mut f, [1.0, 0.0, 0.0], if k == 0 { 0.25 } else { -0.25 });
                }
                geo::merge(&mut fins, &moved(f, [0.0, y + r * 0.4, z]));
            }
            let glass = c::ball([l * 0.1, r * 0.6, r * 0.55], [l * 0.24, y + r * 0.6, 0.0]);
            let nozzle = b::rod([-l * 0.5 - 0.3, y, 0.0], [-l * 0.47, y, 0.0], r * 0.6, 16);
            let mut gear = Mesh::default();
            let mut tyres = Mesh::default();
            for (x, z) in [(l * 0.25, 0.0), (-l * 0.1, r * 1.3), (-l * 0.1, -r * 1.3)] {
                let (s, t) = leg(x, z, y - r * 0.6, 0.35);
                geo::merge(&mut gear, &s);
                geo::merge(&mut tyres, &t);
            }
            Output::Parts(vec![
                Part::new(body, v.color("color")).material("ground"),
                Part::new(fins, v.color("color")).material("ground"),
                Part::new(glass, [0.35, 0.28, 0.08]).material("glass"),
                Part::new(nozzle, [0.12, 0.12, 0.12]).material("metal"),
                Part::new(gear, STEEL).material("metal"),
                Part::new(tyres, RUBBER).material("ground"),
            ])
        });

    basic!(_r, "aircraft-ground-attack-aircraft", "aircraft", "ground-attack-aircraft", "Ground-attack aircraft", "ground",
        [P::num("span", 17.5, 13.0, 22.0).unit("m").describe("Wing span (m)"), P::num("length", 16.0, 12.0, 20.0).unit("m").describe("Length (m)"), P::color("color", [0.38, 0.4, 0.4]).describe("Paint colour")],
        |v: &Values, _s: u32| {
            let (span, l) = (v.f("span"), v.f("length"));
            let (r, y) = (0.75, 2.0);
            // A straight low wing, two engine pods high on the rear body and an H tail.
            let mut body = fuselage(l, r, y, 0.12, 0.4, 0.2);
            geo::merge(&mut body, &wings(l * 0.05, y - r * 0.5, span * 0.5, [3.2, 1.8], 0.4, 0.05));
            geo::merge(&mut body, &wings(-l * 0.45, y + 0.2, 2.8, [1.8, 1.4], 0.1, 0.0));
            let mut pods = Mesh::default();
            for z in [-1.2f32, 1.2] {
                geo::merge(&mut pods, &moved(c::spindle(-1.8, 1.8, 0.0, 1.0, |t| 0.6 * (0.85 + 0.15 * (t * sh::PI).sin())), [-l * 0.2, y + r + 0.4, z]));
                geo::merge(&mut body, &b::cbox([1.0, 0.5, 0.3], [-l * 0.2, y + r * 0.6, z * 0.7]));
                geo::merge(&mut body, &moved(fin(0.0, 0.0, 2.6, [1.6, 1.2], 0.2), [-l * 0.45 + 0.6, y + 0.2, z * 2.3]));
            }
            let glass = c::ball([1.0, 0.5, 0.55], [l * 0.28, y + r * 0.75, 0.0]);
            let gun = b::rod([l * 0.5 - 0.2, y - 0.2, 0.0], [l * 0.5 + 1.0, y - 0.2, 0.0], 0.08, 10);
            let mut gear = Mesh::default();
            let mut tyres = Mesh::default();
            for (x, z) in [(l * 0.35, 0.0), (l * 0.05, 2.0), (l * 0.05, -2.0)] {
                let (s, t) = leg(x, z, y - r * 0.6, 0.45);
                geo::merge(&mut gear, &s);
                geo::merge(&mut tyres, &t);
            }
            geo::merge(&mut gear, &gun);
            Output::Parts(vec![
                Part::new(body, v.color("color")),
                Part::new(pods, [0.3, 0.32, 0.33]).material("metal"),
                Part::new(glass, GLASS).material("glass"),
                Part::new(gear, STEEL).material("metal"),
                Part::new(tyres, RUBBER).material("ground"),
            ])
        });

    basic!(_r, "aircraft-helicopters", "aircraft", "helicopters", "Helicopters", "accent",
        [P::num("blades", 4.0, 2.0, 7.0).step(1.0).describe("Main rotor blades"), P::num("rotor", 5.5, 3.5, 9.0).unit("m").describe("Rotor radius (m)"), P::num("size", 1.0, 0.7, 1.5).describe("Size"), P::color("color", [0.6, 0.06, 0.04]).describe("Body colour")],
        |v: &Values, _s: u32| {
            let (n, rr, k) = (v.f("blades").round() as u32, v.f("rotor"), v.f("size"));
            let y = 1.6;
            // A rounded cabin, a tail boom with a fin and a tail rotor, skids.
            let mut body = c::ball([2.2, 1.1, 1.0], [0.6, y, 0.0]);
            geo::merge(&mut body, &c::limb([-0.8, y + 0.3, 0.0], [-6.2, y + 0.6, 0.0], 0.45, 0.16, 12));
            geo::merge(&mut body, &fin(-5.9, y + 0.6, 1.4, [0.9, 0.5], 0.5));
            geo::merge(&mut body, &b::rounded_box([2.0, 0.6, 1.2], 0.2, 2, [-0.2, y + 0.75, 0.0]));
            let glass = c::ball([1.1, 0.75, 0.85], [1.7, y + 0.2, 0.0]);
            let mut rotors = rotor([0.0, y + 1.65, 0.0], rr * k.recip(), n, 0.45);
            geo::merge(&mut rotors, &b::rod([0.0, y + 1.3, 0.0], [0.0, y + 1.6, 0.0], 0.12, 10));
            let mut tail_rotor = b::cbox([1.6, 0.18, 0.05], [0.0, 0.0, 0.0]);
            let mut t2 = tail_rotor.clone();
            sh::rotate(&mut t2, [0.0, 0.0, 1.0], sh::PI * 0.5);
            geo::merge(&mut tail_rotor, &t2);
            geo::merge(&mut rotors, &moved(tail_rotor, [-6.1, y + 1.0, 0.35]));
            let mut skids = Mesh::default();
            for z in [-1.0f32, 1.0] {
                geo::merge(&mut skids, &b::rod([-1.3, 0.06, z], [2.4, 0.06, z], 0.06, 8));
                geo::merge(&mut skids, &b::rod([2.4, 0.06, z], [2.7, 0.3, z], 0.06, 8));
                for x in [-0.6, 1.4] {
                    geo::merge(&mut skids, &b::rod([x, 0.06, z], [x, y - 0.7, z * 0.6], 0.05, 8));
                }
            }
            let mut ps = vec![
                Part::new(body, v.color("color")).material("accent"),
                Part::new(glass, GLASS).material("glass"),
                Part::new(rotors, DARK),
                Part::new(skids, STEEL).material("metal"),
            ];
            c::scale_parts(&mut ps, k);
            Output::Parts(ps)
        });

    basic!(_r, "aircraft-quadcopter-drones", "aircraft", "quadcopter-drones", "Quadcopter drones", "accent",
        [P::num("arm", 0.25, 0.12, 0.6).unit("m").describe("Arm length (m)"), P::num("prop", 0.12, 0.05, 0.25).unit("m").describe("Propeller radius (m)"), P::color("color", [0.08, 0.08, 0.09]).describe("Body colour"), P::color("accent", [0.85, 0.3, 0.02]).describe("Motor colour")],
        |v: &Values, _s: u32| {
            let (a, pr) = (v.f("arm"), v.f("prop"));
            let y = 0.12;
            let mut body = b::rounded_box([0.18, 0.06, 0.12], 0.02, 2, [0.0, y, 0.0]);
            geo::merge(&mut body, &c::ball([0.05, 0.03, 0.05], [0.07, y - 0.005, 0.0]));
            let mut motors = Mesh::default();
            let mut props = Mesh::default();
            for i in 0..4 {
                let ang = sh::PI * 0.25 + i as f32 * sh::PI * 0.5;
                let tip = [a * ang.cos(), y + 0.03, a * ang.sin()];
                geo::merge(&mut body, &b::rod([0.0, y + 0.03, 0.0], tip, 0.012, 8));
                geo::merge(&mut motors, &b::rod([tip[0], tip[1] - 0.01, tip[2]], [tip[0], tip[1] + 0.04, tip[2]], 0.022, 12));
                let p = b::cbox([pr * 2.0, 0.004, pr * 0.22], [0.0; 3]);
                geo::merge(&mut props, &sh::place(&p, 1.0, ang * 1.7, 0.0, [tip[0], tip[1] + 0.045, tip[2]]));
                geo::merge(&mut body, &b::rod([tip[0] * 0.5, y, tip[2] * 0.5], [tip[0] * 0.55, 0.0, tip[2] * 0.55], 0.006, 6));
            }
            Output::Parts(vec![
                Part::new(body, v.color("color")).material("ground"),
                Part::new(motors, v.color("accent")).material("metal"),
                Part::new(props, [0.2, 0.2, 0.22]).alpha(0.8),
            ])
        });

    basic!(_r, "aircraft-fixed-wing-drones", "aircraft", "fixed-wing-drones", "Fixed-wing drones", "ground",
        [P::num("span", 4.0, 1.5, 15.0).unit("m").describe("Wing span (m)"), P::num("booms", 1.0, 0.0, 1.0).step(1.0).describe("Twin tail booms (1) or a single tail (0)"), P::color("color", [0.6, 0.62, 0.62]).describe("Colour")],
        |v: &Values, _s: u32| {
            let (span, twin) = (v.f("span"), v.f("booms") > 0.5);
            let k = span / 4.0;
            let (l, r, y) = (1.6 * k, 0.14 * k, 0.45 * k);
            // A slim pod, a long straight wing, a pusher prop at the back.
            let mut body = fuselage(l, r, y, 0.25, 0.25, 0.3);
            geo::merge(&mut body, &wings(0.0, y + r * 0.5, span * 0.5, [0.35 * k, 0.2 * k], 0.05 * k, 0.04));
            if twin {
                for z in [-0.5 * k, 0.5 * k] {
                    geo::merge(&mut body, &b::rod([0.0, y + r * 0.4, z], [-1.6 * k, y + r * 0.4, z], 0.025 * k, 8));
                    geo::merge(&mut body, &moved(fin(0.0, 0.0, 0.3 * k, [0.25 * k, 0.15 * k], 0.08 * k), [-1.45 * k, y + r * 0.4, z]));
                }
                geo::merge(&mut body, &b::cbox([0.2 * k, 0.02 * k, 1.0 * k], [-1.55 * k, y + r * 0.4 + 0.3 * k, 0.0]));
            } else {
                geo::merge(&mut body, &b::rod([-l * 0.4, y, 0.0], [-1.7 * k, y + r * 0.2, 0.0], 0.04 * k, 8));
                geo::merge(&mut body, &wings(-1.55 * k, y + r * 0.2, 0.5 * k, [0.25 * k, 0.15 * k], 0.06 * k, 0.6));
            }
            let mut p = prop([0.0, 0.0, 0.0], 0.3 * k, 2);
            p = c::mirror_x(&p);
            let p = moved(p, [-l * 0.5 - 0.05 * k, y, 0.0]);
            let mut gear = Mesh::default();
            for (x, z) in [(l * 0.3, 0.0), (-0.1 * k, 0.25 * k), (-0.1 * k, -0.25 * k)] {
                geo::merge(&mut gear, &b::rod([x, y - r * 0.5, z * 0.5], [x, 0.04 * k, z], 0.01 * k, 6));
                geo::merge(&mut gear, &c::wheel([x, 0.04 * k, z], 0.04 * k, 0.025 * k, 0).0);
            }
            Output::Parts(vec![Part::new(body, v.color("color")), Part::new(p, DARK), Part::new(gear, DARK).material("metal")])
        });

    basic!(_r, "aircraft-gliders", "aircraft", "gliders", "Gliders", "accent",
        [P::num("span", 18.0, 12.0, 26.0).unit("m").describe("Wing span (m)"), P::num("length", 7.0, 5.5, 9.0).unit("m").describe("Length (m)"), P::color("color", [0.85, 0.86, 0.86]).describe("Colour")],
        |v: &Values, _s: u32| {
            let (span, l) = (v.f("span"), v.f("length"));
            let y = 0.65;
            // A slender pod tapering to a thin boom, very long thin wings, a T-tail.
            let mut body = c::spindle(-l * 0.5, l * 0.5, y, 1.0, |t| 0.38 * if t > 0.55 { c::streamline(t, 0.3, 0.0, 1.0) } else { 0.25 + 0.75 * (t / 0.55).powi(2) });
            geo::merge(&mut body, &wings(l * 0.12, y + 0.28, span * 0.5, [0.95, 0.4], 0.25, 0.05));
            geo::merge(&mut body, &fin(-l * 0.44, y + 0.1, 1.1, [0.9, 0.55], 0.45));
            geo::merge(&mut body, &wings(-l * 0.44 - 0.45, y + 1.2, 1.3, [0.5, 0.3], 0.08, 0.0));
            let glass = c::ball([0.9, 0.3, 0.3], [l * 0.28, y + 0.25, 0.0]);
            let (s, t) = leg(l * 0.08, 0.0, y - 0.25, 0.18);
            Output::Parts(vec![Part::new(body, v.color("color")).material("accent"), Part::new(glass, GLASS).material("glass"), Part::new(s, DARK), Part::new(t, RUBBER).material("ground")])
        });

    basic!(_r, "aircraft-seaplanes", "aircraft", "seaplanes", "Seaplanes", "accent",
        [P::num("span", 12.0, 9.0, 15.0).unit("m").describe("Wing span (m)"), P::num("floats", 6.5, 4.5, 8.5).unit("m").describe("Float length (m)"), P::color("color", [0.85, 0.65, 0.05]).describe("Body colour"), P::color("float", [0.78, 0.78, 0.78]).describe("Float colour")],
        |v: &Values, _s: u32| {
            let (span, fl) = (v.f("span"), v.f("floats"));
            let (l, r, y) = (8.5, 0.62, 2.2);
            // A high-wing aircraft on two floats, each a small closed hull.
            let mut body = fuselage(l, r, y, 0.15, 0.55, 0.12);
            geo::merge(&mut body, &wings(l * 0.1, y + r * 0.95, span * 0.5, [1.6, 1.1], 0.05, 0.03));
            geo::merge(&mut body, &wings(-l * 0.42, y + 0.1, 1.8, [0.9, 0.6], 0.15, 0.0));
            geo::merge(&mut body, &fin(-l * 0.4, y + 0.3, 1.4, [1.3, 0.7], 0.6));
            let glass = c::ball([1.0, 0.45, r * 0.95], [l * 0.14, y + 0.3, 0.0]);
            let mut floats = Mesh::default();
            let mut struts = Mesh::default();
            for z in [-1.3f32, 1.3] {
                let hull = c::hull(fl, 0.6, 0.5, 2.0, false, |t| [if t > 0.75 { 1.0 - ((t - 0.75) / 0.25).powi(2) * 0.9 } else { 0.8 + 0.2 * (t / 0.75) }, if t > 0.8 { (t - 0.8) * 1.2 } else { 0.0 }, 0.0]);
                geo::merge(&mut floats, &moved(hull, [l * 0.08, 0.0, z]));
                for x in [l * 0.08 + fl * 0.2, l * 0.08 - fl * 0.15] {
                    geo::merge(&mut struts, &b::rod([x, 0.45, z], [x * 0.8, y - r * 0.6, z * 0.35], 0.04, 6));
                }
            }
            let p = prop([l * 0.5 + 0.1, y, 0.0], 1.0, 3);
            Output::Parts(vec![
                Part::new(body, v.color("color")).material("accent"),
                Part::new(floats, v.color("float")).material("accent"),
                Part::new(glass, GLASS).material("glass"),
                Part::new(struts, STEEL).material("metal"),
                Part::new(p, DARK),
            ])
        });

    basic!(_r, "aircraft-hot-air-balloons", "aircraft", "hot-air-balloons", "Hot-air balloons", "ground",
        [P::num("size", 9.0, 5.0, 16.0).unit("m").describe("Envelope radius (m)"), P::num("gores", 12.0, 6.0, 24.0).step(2.0).describe("Panels round the envelope"), P::color("color", [0.7, 0.08, 0.05]).describe("Panel colour"), P::color("second", [0.85, 0.65, 0.05]).describe("Second panel colour")],
        |v: &Values, _s: u32| {
            let (rr, n) = (v.f("size"), (v.f("gores").round() as u32).max(2));
            // A teardrop envelope turned round its axis, its panels in two colours.
            let base = 3.0 + rr * 1.05;
            let prof: Vec<[f32; 2]> = (0..=24).map(|i| {
                let t = i as f32 / 24.0;
                let a = -sh::PI * 0.5 + t * sh::PI;
                let k = if a < 0.0 { 0.18 + 0.82 * (1.0 - (a / (sh::PI * 0.5)).powi(2)).max(0.0).sqrt() } else { a.cos() };
                [rr * k.max(0.0), base + rr * 0.6 + rr * a.sin() * if a < 0.0 { 1.4 } else { 1.0 }]
            }).collect();
            let (mut ga, mut gb) = (Mesh::default(), Mesh::default());
            let steps = 4;
            for g in 0..n {
                let mut bl = geo::MeshBuilder::new();
                for p in &prof {
                    for s in 0..=steps {
                        let a = (g as f32 + s as f32 / steps as f32) / n as f32 * sh::TAU;
                        bl.vertex([p[0] * a.cos(), p[1], -p[0] * a.sin()], [a.cos(), 0.0, -a.sin()]);
                    }
                }
                let row = steps + 1;
                for j in 0..(prof.len() - 1) as u32 {
                    for s in 0..steps {
                        let i0 = j * row + s;
                        bl.quad(i0, i0 + 1, i0 + row + 1, i0 + row);
                    }
                }
                let mut m = bl.finish();
                geo::smooth_normals(&mut m);
                // Outward: the smooth normal from the axis.
                for vtx in &mut m.vertices {
                    let rad = (vtx[0] * vtx[0] + vtx[2] * vtx[2]).sqrt().max(1e-4);
                    if vtx[3] * vtx[0] / rad + vtx[5] * vtx[2] / rad < 0.0 {
                        vtx[3] = -vtx[3];
                        vtx[4] = -vtx[4];
                        vtx[5] = -vtx[5];
                    }
                }
                sh::fix_winding(&mut m);
                geo::merge(if g % 2 == 0 { &mut ga } else { &mut gb }, &m);
            }
            // The basket on ropes under the mouth, and the burner.
            let mouth = prof.first().map_or(1.0, |p| p[0]);
            let basket = b::rounded_box([1.4, 1.1, 1.4], 0.08, 2, [0.0, 0.0, 0.0]);
            let mut ropes = Mesh::default();
            for i in 0..4 {
                let a = sh::PI * 0.25 + i as f32 * sh::PI * 0.5;
                geo::merge(&mut ropes, &b::rod([0.65 * a.cos(), 1.1, 0.65 * a.sin()], [mouth * a.cos(), prof[0][1], mouth * a.sin()], 0.02, 5));
            }
            geo::merge(&mut ropes, &b::rod([0.0, 1.7, 0.0], [0.0, 2.1, 0.0], 0.18, 12));
            Output::Parts(vec![
                Part::new(ga, v.color("color")),
                Part::new(gb, v.color("second")),
                Part::new(basket, [0.45, 0.32, 0.16]),
                Part::new(ropes, DARK).material("metal"),
            ])
        });

    basic!(_r, "aircraft-rockets", "aircraft", "rockets", "Rockets", "accent",
        [P::num("height", 40.0, 10.0, 110.0).unit("m").describe("Height (m)"), P::num("stages", 2.0, 1.0, 3.0).step(1.0).describe("Stages"), P::num("fins", 4.0, 0.0, 6.0).step(1.0).describe("Fins"), P::color("color", [0.82, 0.82, 0.8]).describe("Body colour")],
        |v: &Values, _s: u32| {
            let (h, n, nf) = (v.f("height"), v.f("stages").round() as u32, v.f("fins").round() as u32);
            let r = h * 0.045;
            let foot = h * 0.04;
            // Stages stepping in, an interstage band at each joint, a nose cone.
            let body_h = h * 0.82;
            let mut prof = vec![[0.0, foot], [r, foot]];
            let mut bands = Mesh::default();
            for k in 0..n {
                let rk = r * (1.0 - k as f32 * 0.15);
                let y0 = foot + body_h * k as f32 / n as f32;
                let y1 = foot + body_h * (k + 1) as f32 / n as f32;
                prof.push([rk, y0]);
                prof.push([rk, y1]);
                if k + 1 < n {
                    geo::merge(&mut bands, &b::rod([0.0, y1 - h * 0.006, 0.0], [0.0, y1 + h * 0.006, 0.0], rk * 1.02, 32));
                }
            }
            let top_r = r * (1.0 - (n - 1) as f32 * 0.15);
            for i in 1..=10 {
                let t = i as f32 / 10.0;
                prof.push([top_r * (1.0 - t * t).sqrt().max(0.0) * (1.0 - t * 0.2), foot + body_h + (h - foot - body_h) * t]);
            }
            prof.push([0.0, h]);
            let body = b::revolve(&prof, 32);
            let bell = b::revolve(&[[r * 0.3, foot], [r * 0.75, 0.0], [r * 0.7, 0.0], [r * 0.25, foot]], 24);
            let mut fins = Mesh::default();
            for i in 0..nf {
                let a = i as f32 / nf as f32 * sh::TAU;
                let mut f = c::panel([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], r * 1.6, h * 0.14, h * 0.05, h * 0.06, 0.08);
                sh::rotate(&mut f, [0.0, 0.0, 1.0], sh::PI * 0.5);
                geo::merge(&mut fins, &sh::place(&f, 1.0, a, 0.0, [r * 0.9 * a.sin(), foot + h * 0.12, r * 0.9 * a.cos()]));
            }
            Output::Parts(vec![
                Part::new(body, v.color("color")).material("accent"),
                Part::new(bands, [0.08, 0.08, 0.09]),
                Part::new(fins, [0.08, 0.08, 0.09]),
                Part::new(bell, [0.25, 0.22, 0.2]).material("metal"),
            ])
        });

    basic!(_r, "aircraft-space-shuttles", "aircraft", "space-shuttles", "Space shuttles", "accent",
        [P::num("length", 37.0, 28.0, 45.0).unit("m").describe("Orbiter length (m)"), P::num("stack", 1.0, 0.0, 1.0).step(1.0).describe("On its tank and boosters (1) or alone (0)"), P::color("color", [0.62, 0.3, 0.08]).describe("Tank colour")],
        |v: &Values, _s: u32| {
            let (l, stack) = (v.f("length"), v.f("stack") > 0.5);
            let r = l * 0.075;
            // The orbiter built nose +x: a body, a double-delta wing, a tall fin.
            let mut orb = c::loft_x(-l * 0.5, l * 0.5, 18, |_| 0.0, |t| {
                let k = r * c::streamline(t, 0.15, 0.05, 0.85);
                c::squircle(k, k, 3.0, 20)
            });
            geo::merge(&mut orb, &c::panels([l * 0.05, -r * 0.6, 0.0], [0.0, 0.0, 1.0], l * 0.32, [l * 0.55, l * 0.08], l * 0.35, 0.06));
            geo::merge(&mut orb, &fin(-l * 0.32, r * 0.8, l * 0.22, [l * 0.22, l * 0.08], l * 0.15));
            let mut belly = c::panels([l * 0.05, -r * 0.68, 0.0], [0.0, 0.0, 1.0], l * 0.3, [l * 0.52, l * 0.07], l * 0.34, 0.03);
            geo::merge(&mut belly, &b::rod([-l * 0.5 - 1.0, 0.0, 0.0], [-l * 0.5 + 0.2, 0.0, 0.0], r * 0.4, 12));
            let glass = c::ball([l * 0.06, r * 0.35, r * 0.6], [l * 0.36, r * 0.65, 0.0]);
            if !stack {
                // Alone on its gear, nose +x.
                let mut gear = Mesh::default();
                let mut tyres = Mesh::default();
                for (x, z) in [(l * 0.35, 0.0), (-l * 0.05, r * 1.3), (-l * 0.05, -r * 1.3)] {
                    let (s, t) = leg(x, z, -r * 0.6, 0.6);
                    geo::merge(&mut gear, &moved(s, [0.0, -(r + 2.0), 0.0]));
                    geo::merge(&mut tyres, &moved(t, [0.0, -(r + 2.0), 0.0]));
                }
                return c::stand(vec![
                    Part::new(orb, WHITE).material("accent"),
                    Part::new(belly, [0.06, 0.06, 0.07]),
                    Part::new(glass, GLASS).material("glass"),
                    Part::new(gear, STEEL).material("metal"),
                    Part::new(tyres, RUBBER).material("ground"),
                ]);
            }
            // Upright on the big tank, a booster either side.
            let up = |m: &Mesh| {
                let mut m = m.clone();
                sh::rotate(&mut m, [0.0, 0.0, 1.0], sh::PI * 0.5);
                sh::rotate(&mut m, [0.0, 1.0, 0.0], sh::PI * 0.5);
                moved(m, [0.0, l * 0.5 + 4.0, l * 0.115 + r + 0.5])
            };
            let (th, tr) = (l * 1.3, l * 0.115);
            let mut tp = vec![[0.0, 2.0], [tr, 2.0], [tr, 2.0 + th * 0.8]];
            for i in 1..=8 {
                let t = i as f32 / 8.0;
                tp.push([tr * (1.0 - t * t), 2.0 + th * 0.8 + th * 0.2 * t]);
            }
            let tank = b::revolve(&tp, 32);
            let mut boosters = Mesh::default();
            for z in [-1.0f32, 1.0] {
                let mut bp = vec![[0.0, 0.0], [tr * 0.4, 0.0], [tr * 0.42, 2.5], [tr * 0.42, th * 0.85]];
                for i in 1..=6 {
                    let t = i as f32 / 6.0;
                    bp.push([tr * 0.42 * (1.0 - t * t), th * 0.85 + th * 0.08 * t]);
                }
                geo::merge(&mut boosters, &moved(b::revolve(&bp, 24), [z * tr * 1.45, 0.0, 0.0]));
            }
            let orb_up = up(&orb);
            Output::Parts(vec![
                Part::new(tank, v.color("color")),
                Part::new(boosters, WHITE).material("accent"),
                Part::new(orb_up, WHITE).material("accent"),
                Part::new(up(&belly), [0.06, 0.06, 0.07]),
                Part::new(up(&glass), GLASS).material("glass"),
            ])
        });
}
