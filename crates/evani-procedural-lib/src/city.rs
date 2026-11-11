//! Category 14, City and public infrastructure: basic generators (batch B).
//! Street furniture from tapered posts, arms swept along curves, lathed
//! bollards and hydrants, sign plates extruded from their outline, glazed
//! frames, grates and covers from bars and rings, and a W-beam rail swept
//! along a curve. Inspired by three.js (MIT) CylinderGeometry,
//! LatheGeometry, ExtrudeGeometry and TubeGeometry; written fresh here.

use evani_procedural::Registry;

#[allow(unused_imports, dead_code)]
pub fn register(_r: &mut Registry) {
    use evani_procedural::{Output, ParamSpec as P, Part, Values, build as b, geo, shapes as sh};
    const STEEL: [f32; 3] = [0.25, 0.26, 0.27];
    const DARK: [f32; 3] = [0.06, 0.06, 0.065];
    const LAMP: [f32; 3] = [1.0, 0.85, 0.55];

    fn moved(mut m: geo::Mesh, d: [f32; 3]) -> geo::Mesh {
        geo::transform(&mut m, [1.0; 3], d);
        m
    }

    /// A tapered round pole from the ground to `h`.
    fn pole(r0: f32, r1: f32, h: f32) -> geo::Mesh {
        sh::frustum(r0, r1, h, 14)
    }

    basic!(_r, "city-electricity-poles", "city", "electricity-poles", "Electricity poles", "ground",
        [P::num("height", 9.0, 5.0, 16.0).unit("m").describe("Height (m)"), P::num("arms", 1.0, 1.0, 3.0).step(1.0).describe("Cross-arms"), P::num("sag", 0.5, 0.0, 2.0).unit("m").describe("Wire sag (m)"), P::color("color", [0.3, 0.22, 0.14]).describe("Pole colour")],
        |v: &Values, _s: u32| {
            let (h, n, sag) = (v.f("height"), v.f("arms").round() as u32, v.f("sag"));
            let mut wood = pole(0.16, 0.11, h);
            let (mut insul, mut wires) = (geo::Mesh::default(), geo::Mesh::default());
            for i in 0..n {
                let y = h - 0.4 - i as f32 * 0.9;
                geo::merge(&mut wood, &b::cbox([0.1, 0.1, 2.0], [0.0, y, 0.0]));
                for z in [-0.85f32, -0.3, 0.3, 0.85] {
                    let ins = b::revolve(&[[0.0, 0.0], [0.05, 0.0], [0.07, 0.05], [0.04, 0.1], [0.07, 0.15], [0.03, 0.22], [0.0, 0.22]], 10);
                    geo::merge(&mut insul, &moved(ins, [0.0, y + 0.05, z]));
                    // Wires running off both ways along x, sagging between poles.
                    let top = [0.0, y + 0.25, z];
                    for dir in [-1.0f32, 1.0] {
                        // A short span each way, cut where the next pole would stand.
                        let far = [dir * 3.5, y + 0.25, z];
                        let path = b::catenary(top, far, sag, 16);
                        geo::merge(&mut wires, &b::sweep(&path, &b::circle(0.012, 5), true));
                    }
                }
            }
            geo::merge(&mut wood, &b::bar([0.0, h - 1.6, 0.0], [0.0, h - 0.45, 0.7], 0.06, 0.06));
            Output::Parts(vec![Part::new(wood, v.color("color")), Part::new(insul, [0.55, 0.62, 0.6]).material("glass"), Part::new(wires, DARK)])
        });

    basic!(_r, "city-streetlights", "city", "streetlights", "Streetlights", "metal",
        [P::num("height", 8.0, 3.0, 14.0).unit("m").describe("Height (m)"), P::num("reach", 1.6, 0.3, 4.0).unit("m").describe("Arm reach (m)"), P::color("color", [0.35, 0.37, 0.38]).describe("Pole colour"), P::color("light", LAMP).describe("Lamp colour")],
        |v: &Values, _s: u32| {
            let (h, reach) = (v.f("height"), v.f("reach"));
            let mut steel = pole(0.12, 0.06, h);
            geo::merge(&mut steel, &b::revolve(&[[0.0, 0.0], [0.2, 0.0], [0.2, 0.5], [0.12, 0.6], [0.0, 0.6]], 16));
            // The arm curves out from the top; the lamp head hangs at its end.
            let arm: Vec<[f32; 3]> = (0..=12).map(|i| {
                let t = i as f32 / 12.0;
                let a = t * sh::PI * 0.5;
                [reach * a.sin(), h - 0.3 + 0.5 * (1.0 - (1.0 - t).powi(2)) - 0.0, 0.0]
            }).collect();
            geo::merge(&mut steel, &b::sweep(&arm, &b::circle(0.045, 8), true));
            let end = arm[arm.len() - 1];
            geo::merge(&mut steel, &b::rounded_box([0.6, 0.12, 0.28], 0.04, 3, [end[0] + 0.2, end[1] - 0.08, 0.0]));
            let lamp = b::cbox([0.5, 0.02, 0.22], [end[0] + 0.2, end[1] - 0.09, 0.0]);
            Output::Parts(vec![Part::new(steel, v.color("color")), Part::new(lamp, v.color("light")).material("glow")])
        });

    basic!(_r, "city-traffic-lights", "city", "traffic-lights", "Traffic lights", "metal",
        [P::num("height", 3.4, 2.4, 6.0).unit("m").describe("Pole height (m)"), P::num("lit", 0.0, 0.0, 2.0).step(1.0).describe("Lit lamp (0 red, 1 amber, 2 green)"), P::color("color", [0.08, 0.08, 0.09]).describe("Housing colour")],
        |v: &Values, _s: u32| {
            let (h, lit) = (v.f("height"), v.f("lit").round() as usize);
            let mut body = pole(0.07, 0.06, h - 0.9);
            let top = h - 0.9;
            geo::merge(&mut body, &b::rounded_box([0.32, 0.95, 0.24], 0.04, 3, [0.0, top, 0.0]));
            let cols = [[0.9, 0.05, 0.02], [0.95, 0.5, 0.02], [0.05, 0.85, 0.25]];
            let (mut on, mut off) = (geo::Mesh::default(), geo::Mesh::default());
            for (k, _) in cols.iter().enumerate() {
                let y = top + 0.78 - k as f32 * 0.3;
                let lens = b::rod([0.0, y, 0.12], [0.0, y, 0.135], 0.1, 16);
                // A thin hood over the top of each lamp.
                let mut hood: Vec<[f32; 2]> = (0..=8).map(|i| {
                    let a = sh::PI * i as f32 / 8.0;
                    [0.125 * a.cos(), 0.125 * a.sin()]
                }).collect();
                hood.extend((0..=8).rev().map(|i| {
                    let a = sh::PI * i as f32 / 8.0;
                    [0.115 * a.cos(), 0.115 * a.sin()]
                }));
                geo::merge(&mut body, &b::sweep(&[[0.0, y, 0.12], [0.0, y, 0.3]], &hood, true));
                if k == lit {
                    geo::merge(&mut on, &lens);
                } else {
                    geo::merge(&mut off, &lens);
                }
            }
            let glow = cols[lit.min(2)];
            Output::Parts(vec![Part::new(body, v.color("color")), Part::new(on, glow).material("glow"), Part::new(off, [0.08, 0.07, 0.06]).material("glass")])
        });

    basic!(_r, "city-road-signs", "city", "road-signs", "Road signs", "metal",
        [P::num("height", 2.4, 1.2, 4.0).unit("m").describe("Height (m)"), P::num("shape", 0.0, 0.0, 3.0).step(1.0).describe("Plate (0 round, 1 triangle, 2 octagon, 3 rectangle)"), P::num("size", 0.6, 0.3, 1.5).unit("m").describe("Plate size (m)"), P::color("color", [0.75, 0.05, 0.04]).describe("Sign colour")],
        |v: &Values, _s: u32| {
            let (h, shape, k) = (v.f("height"), v.f("shape").round() as u32, v.f("size"));
            let post = b::rod([0.0, 0.0, 0.0], [0.0, h, 0.0], 0.03, 10);
            let r = k * 0.5;
            let (outer, inner) = match shape {
                1 => (b::star(3, r * 1.15, 1.0), b::star(3, r * 0.8, 1.0)),
                2 => (b::star(8, r, 1.0), b::star(8, r * 0.88, 1.0)),
                3 => (b::rect(k * 1.3, k * 0.9), b::rect(k * 1.2, k * 0.8)),
                _ => (b::circle(r, 32), b::circle(r * 0.8, 32)),
            };
            let cy = h - r;
            let rot = |mut m: geo::Mesh| {
                // A warning triangle points up; the octagon sits flat.
                match shape {
                    1 => sh::rotate(&mut m, [0.0, 0.0, 1.0], sh::PI * 0.5),
                    2 => sh::rotate(&mut m, [0.0, 0.0, 1.0], sh::PI / 8.0),
                    _ => {}
                }
                moved(m, [0.0, cy, 0.07])
            };
            let rim = rot(b::extrude(&outer, 0.02));
            let face = rot(moved(b::extrude(&inner, 0.006), [0.0, 0.0, 0.012]));
            let (rim_c, face_c) = if shape == 2 { ([0.9, 0.9, 0.88], v.color("color")) } else { (v.color("color"), [0.9, 0.9, 0.88]) };
            Output::Parts(vec![Part::new(post, STEEL).material("metal"), Part::new(rim, rim_c), Part::new(face, face_c)])
        });

    basic!(_r, "city-bus-shelters", "city", "bus-shelters", "Bus shelters", "metal",
        [P::num("length", 4.0, 2.0, 8.0).unit("m").describe("Length (m)"), P::num("height", 2.5, 2.1, 3.2).unit("m").describe("Height (m)"), P::color("color", [0.12, 0.2, 0.28]).describe("Frame colour")],
        |v: &Values, _s: u32| {
            let (l, h) = (v.f("length"), v.f("height"));
            let d = 1.5;
            let mut frame = geo::Mesh::default();
            let bays = (l / 1.3).round().max(2.0) as u32;
            for i in 0..=bays {
                let x = (i as f32 / bays as f32 - 0.5) * l;
                geo::merge(&mut frame, &b::post(x, -d * 0.5, 0.08, h));
            }
            for z in [-d * 0.5, d * 0.5] {
                for x in [-l * 0.5, l * 0.5] {
                    geo::merge(&mut frame, &b::post(x, z, 0.08, h));
                }
            }
            let mut roof = b::extrude_x(&[[-d * 0.5 - 0.2, h], [d * 0.5 + 0.3, h + 0.2], [d * 0.5 + 0.3, h + 0.28], [-d * 0.5 - 0.2, h + 0.1]], l + 0.3);
            geo::merge(&mut roof, &b::cbox([l - 0.4, 0.06, 0.35], [0.0, 0.45, -d * 0.5 + 0.25]));
            let mut glass = b::cbox([l - 0.1, h - 0.3, 0.012], [0.0, 0.15 + (h - 0.3) * 0.5, -d * 0.5]);
            for x in [-l * 0.5, l * 0.5] {
                geo::merge(&mut glass, &b::cbox([0.012, h - 0.3, d - 0.1], [x, 0.15 + (h - 0.3) * 0.5, 0.0]));
            }
            Output::Parts(vec![Part::new(frame, v.color("color")), Part::new(roof, [0.3, 0.3, 0.32]), Part::new(glass, [0.5, 0.6, 0.62]).alpha(0.35).material("glass")])
        });

    basic!(_r, "city-bollards", "city", "bollards", "Bollards", "metal",
        [P::num("count", 5.0, 1.0, 16.0).step(1.0).describe("Bollards in the row"), P::num("height", 0.9, 0.4, 1.4).unit("m").describe("Height (m)"), P::color("color", [0.08, 0.08, 0.09]).describe("Bollard colour")],
        |v: &Values, _s: u32| {
            let (n, h) = (v.f("count").round() as u32, v.f("height"));
            let r = 0.1;
            let one = b::revolve(&[[0.0, 0.0], [r * 1.25, 0.0], [r * 1.25, 0.08], [r, 0.12], [r, h * 0.88], [r * 1.15, h * 0.9], [r * 1.05, h * 0.95], [r * 0.6, h], [0.0, h]], 20);
            let band = b::revolve(&[[r + 0.003, h * 0.75], [r + 0.003, h * 0.8]], 20);
            let (mut m, mut bands) = (geo::Mesh::default(), geo::Mesh::default());
            for i in 0..n {
                let x = (i as f32 - (n - 1) as f32 * 0.5) * 1.4;
                geo::merge(&mut m, &moved(one.clone(), [x, 0.0, 0.0]));
                geo::merge(&mut bands, &moved(band.clone(), [x, 0.0, 0.0]));
            }
            Output::Parts(vec![Part::new(m, v.color("color")), Part::new(bands, [0.85, 0.8, 0.6])])
        });

    basic!(_r, "city-fire-hydrants", "city", "fire-hydrants", "Fire hydrants", "metal",
        [P::num("height", 0.8, 0.4, 1.2).unit("m").describe("Height (m)"), P::num("outlets", 2.0, 1.0, 3.0).step(1.0).describe("Hose outlets"), P::color("color", [0.6, 0.05, 0.03]).describe("Hydrant colour")],
        |v: &Values, _s: u32| {
            let (h, n) = (v.f("height"), v.f("outlets").round() as u32);
            let r = h * 0.15;
            // A flanged foot, the barrel, a bonnet dome and its nut.
            let mut m = b::revolve(&[[0.0, 0.0], [r * 1.5, 0.0], [r * 1.5, h * 0.06], [r, h * 0.1], [r, h * 0.7], [r * 1.25, h * 0.72], [r * 1.25, h * 0.76], [r * 1.05, h * 0.78], [r * 0.9, h * 0.88], [r * 0.4, h * 0.94], [0.0, h * 0.95]], 24);
            let nut_fix = b::prism(&b::star(5, r * 0.3, 1.0), h * 0.94, h * 0.08);
            geo::merge(&mut m, &nut_fix);
            let mut caps = geo::Mesh::default();
            for i in 0..n {
                let a = [0.0f32, sh::PI, sh::PI * 0.5][i as usize];
                let (c, s) = (a.cos(), a.sin());
                let y = h * 0.5;
                geo::merge(&mut m, &b::rod([c * r * 0.8, y, s * r * 0.8], [c * r * 1.6, y, s * r * 1.6], r * 0.35, 14));
                geo::merge(&mut caps, &b::rod([c * r * 1.6, y, s * r * 1.6], [c * r * 1.75, y, s * r * 1.75], r * 0.42, 6));
            }
            Output::Parts(vec![Part::new(m, v.color("color")), Part::new(caps, [0.75, 0.7, 0.55]).material("metal")])
        });

    basic!(_r, "city-parking-meters", "city", "parking-meters", "Parking meters", "metal",
        [P::num("height", 1.4, 1.0, 1.8).unit("m").describe("Height (m)"), P::num("heads", 1.0, 1.0, 2.0).step(1.0).describe("Meter heads"), P::color("color", [0.2, 0.22, 0.24]).describe("Meter colour")],
        |v: &Values, _s: u32| {
            let (h, n) = (v.f("height"), v.f("heads").round() as u32);
            let mut m = b::rod([0.0, 0.0, 0.0], [0.0, h - 0.3, 0.0], 0.035, 10);
            let mut screens = geo::Mesh::default();
            let heads: &[f32] = if n == 1 { &[0.0] } else { &[-0.14, 0.14] };
            for &x in heads {
                geo::merge(&mut m, &b::rounded_box([0.24, 0.36, 0.16], 0.07, 4, [x, h - 0.35, 0.0]));
                geo::merge(&mut screens, &b::cbox([0.14, 0.08, 0.01], [x, h - 0.1, 0.081]));
                geo::merge(&mut m, &b::cbox([0.04, 0.012, 0.01], [x, h - 0.24, 0.081]));
            }
            if n > 1 {
                geo::merge(&mut m, &b::cbox([0.32, 0.04, 0.08], [0.0, h - 0.36, 0.0]));
            }
            Output::Parts(vec![Part::new(m, v.color("color")), Part::new(screens, [0.3, 0.6, 0.5]).material("glow")])
        });

    basic!(_r, "city-telephone-booths", "city", "telephone-booths", "Telephone booths", "ground",
        [P::num("height", 2.5, 2.0, 3.0).unit("m").describe("Height (m)"), P::num("panes", 6.0, 3.0, 10.0).step(1.0).describe("Glass panes up each side"), P::color("color", [0.55, 0.03, 0.02]).describe("Booth colour")],
        |v: &Values, _s: u32| {
            let (h, n) = (v.f("height"), v.f("panes").round() as u32);
            let w = 0.9;
            let mut body = sh::block([w + 0.1, 0.12, w + 0.1], [0.0; 3]);
            for (x, z) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                geo::merge(&mut body, &b::post(x * w * 0.5, z * w * 0.5, 0.1, h));
            }
            // A sign band, then a vaulted roof across the top.
            geo::merge(&mut body, &sh::block([w + 0.06, 0.22, w + 0.06], [0.0, h - 0.22, 0.0]));
            let mut arch = vec![[-w * 0.55, 0.0], [w * 0.55, 0.0]];
            for i in 1..12 {
                let a = sh::PI * i as f32 / 12.0;
                arch.push([w * 0.55 * a.cos(), 0.18 * a.sin()]);
            }
            geo::merge(&mut body, &moved(b::extrude(&arch, w + 0.1), [0.0, h, 0.0]));
            let mut bars = geo::Mesh::default();
            let mut glass = geo::Mesh::default();
            let gh = h - 0.6;
            for side in 0..4 {
                let mut face = b::cbox([w - 0.1, gh, 0.01], [0.0, 0.3 + gh * 0.5, w * 0.5]);
                let mut fb = geo::Mesh::default();
                for j in 1..n {
                    geo::merge(&mut fb, &b::cbox([w - 0.1, 0.03, 0.04], [0.0, 0.3 + gh * j as f32 / n as f32, w * 0.5]));
                }
                for x in [-w * 0.17, w * 0.17] {
                    geo::merge(&mut fb, &b::cbox([0.03, gh, 0.04], [x, 0.3 + gh * 0.5, w * 0.5]));
                }
                sh::rotate(&mut face, [0.0, 1.0, 0.0], side as f32 * sh::PI * 0.5);
                sh::rotate(&mut fb, [0.0, 1.0, 0.0], side as f32 * sh::PI * 0.5);
                geo::merge(&mut glass, &face);
                geo::merge(&mut bars, &fb);
            }
            geo::merge(&mut body, &bars);
            let sign = b::cbox([w * 0.7, 0.12, 0.01], [0.0, h - 0.11, w * 0.5 + 0.035]);
            Output::Parts(vec![Part::new(body, v.color("color")), Part::new(glass, [0.6, 0.7, 0.72]).alpha(0.35).material("glass"), Part::new(sign, [0.95, 0.92, 0.8]).material("glow")])
        });

    basic!(_r, "city-drainage-grates", "city", "drainage-grates", "Drainage grates", "metal",
        [P::num("length", 0.6, 0.3, 1.5).unit("m").describe("Length (m)"), P::num("bars", 9.0, 3.0, 30.0).step(1.0).describe("Bars across"), P::color("color", [0.1, 0.1, 0.1]).describe("Iron colour")],
        |v: &Values, _s: u32| {
            let (l, n) = (v.f("length"), v.f("bars").round() as u32);
            let w = 0.4;
            // A kerb-side slab with the grate frame set into it.
            let mut slab = sh::block([l + 1.0, 0.08, w + 0.8], [0.0, 0.0, 0.0]);
            geo::merge(&mut slab, &sh::block([l + 1.0, 0.15, 0.2], [0.0, 0.0, -(w + 0.8) * 0.5 - 0.1]));
            let mut iron = geo::Mesh::default();
            for (sx, sz) in [(l, 0.04f32), (0.04, w)] {
                for k in [-1.0f32, 1.0] {
                    let at = if sx > 0.1 { [0.0, 0.08, k * (w * 0.5 - 0.02)] } else { [k * (l * 0.5 - 0.02), 0.08, 0.0] };
                    geo::merge(&mut iron, &sh::block([sx, 0.012, sz], at));
                }
            }
            for i in 0..n {
                let x = ((i as f32 + 0.5) / n as f32 - 0.5) * (l - 0.08);
                geo::merge(&mut iron, &sh::block([(l - 0.08) / n as f32 * 0.55, 0.01, w - 0.06], [x, 0.08, 0.0]));
            }
            let hole = sh::block([l - 0.06, 0.002, w - 0.06], [0.0, 0.08, 0.0]);
            Output::Parts(vec![Part::new(iron, v.color("color")), Part::new(slab, [0.42, 0.41, 0.38]).material("stone"), Part::new(hole, [0.01, 0.01, 0.01])])
        });

    basic!(_r, "city-manhole-covers", "city", "manhole-covers", "Manhole covers", "metal",
        [P::num("radius", 0.35, 0.2, 0.8).unit("m").describe("Radius (m)"), P::num("rings", 4.0, 1.0, 10.0).step(1.0).describe("Raised rings on the lid"), P::color("color", [0.12, 0.11, 0.1]).describe("Iron colour")],
        |v: &Values, _s: u32| {
            let (r, n) = (v.f("radius"), v.f("rings").round() as u32);
            let t = 0.012;
            let mut iron = b::revolve(&[[0.0, 0.0], [r, 0.0], [r, t], [0.0, t]], 40);
            // A frame ring, concentric raised rings and a cross of ribs.
            geo::merge(&mut iron, &b::revolve(&[[r + 0.005, 0.0], [r + 0.08, 0.0], [r + 0.08, t + 0.004], [r + 0.005, t + 0.004], [r + 0.005, 0.0]], 40));
            for i in 1..=n {
                let rr = r * i as f32 / (n + 1) as f32;
                geo::merge(&mut iron, &b::revolve(&[[rr - 0.006, t], [rr + 0.006, t], [rr + 0.006, t + 0.006], [rr - 0.006, t + 0.006], [rr - 0.006, t]], 40));
            }
            for a in [0.0, sh::PI * 0.5] {
                let (c, s) = (a.cos() * r * 0.92, a.sin() * r * 0.92);
                geo::merge(&mut iron, &b::bar([-c, t + 0.003, -s], [c, t + 0.003, s], 0.012, 0.006));
            }
            let road = sh::block([r * 3.2, 0.006, r * 3.2], [0.0; 3]);
            let mut lifted = iron;
            geo::transform(&mut lifted, [1.0; 3], [0.0, 0.004, 0.0]);
            Output::Parts(vec![Part::new(lifted, v.color("color")), Part::new(road, [0.06, 0.06, 0.065]).material("stone")])
        });

    basic!(_r, "city-power-transformers", "city", "power-transformers", "Power transformers", "metal",
        [P::num("size", 1.6, 0.8, 4.0).unit("m").describe("Tank size (m)"), P::num("fins", 10.0, 0.0, 24.0).step(1.0).describe("Radiator fins each side"), P::color("color", [0.36, 0.42, 0.38]).describe("Tank colour")],
        |v: &Values, _s: u32| {
            let (k, n) = (v.f("size"), v.f("fins").round() as u32);
            let mut tank = sh::block([k, k * 1.1, k * 0.7], [0.0, 0.2, 0.0]);
            geo::merge(&mut tank, &sh::block([k * 1.05, 0.06, k * 0.75], [0.0, 0.2 + k * 1.1, 0.0]));
            for i in 0..n {
                let x = ((i as f32 + 0.5) / n as f32 - 0.5) * k * 0.9;
                for z in [-1.0f32, 1.0] {
                    geo::merge(&mut tank, &b::cbox([0.025, k * 0.85, k * 0.22], [x, 0.2 + k * 0.55, z * (k * 0.35 + k * 0.11)]));
                }
            }
            let plinth = sh::block([k * 1.5, 0.2, k * 1.3], [0.0; 3]);
            // Three bushings on top: stacked porcelain sheds.
            let mut bush = geo::Mesh::default();
            for i in 0..3 {
                let x = (i as f32 - 1.0) * k * 0.3;
                let mut prof = vec![[0.0, 0.0]];
                for j in 0..5 {
                    let y = j as f32 * 0.09;
                    prof.extend([[0.05, y], [0.1, y + 0.03], [0.05, y + 0.06]]);
                }
                prof.push([0.03, 0.5]);
                prof.push([0.0, 0.5]);
                geo::merge(&mut bush, &moved(b::revolve(&prof, 12), [x, 0.26 + k * 1.1, 0.0]));
            }
            Output::Parts(vec![Part::new(tank, v.color("color")), Part::new(plinth, [0.5, 0.49, 0.46]).material("stone"), Part::new(bush, [0.45, 0.3, 0.22]).material("glass")])
        });

    basic!(_r, "city-utility-cabinets", "city", "utility-cabinets", "Utility cabinets", "metal",
        [P::num("width", 1.0, 0.4, 2.0).unit("m").describe("Width (m)"), P::num("height", 1.3, 0.6, 2.0).unit("m").describe("Height (m)"), P::color("color", [0.18, 0.3, 0.2]).describe("Cabinet colour")],
        |v: &Values, _s: u32| {
            let (w, h) = (v.f("width"), v.f("height"));
            let d = 0.4;
            let mut m = sh::block([w, h, d], [0.0, 0.12, 0.0]);
            geo::merge(&mut m, &sh::block([w + 0.08, 0.04, d + 0.08], [0.0, 0.12 + h, 0.0]));
            // Two doors with a gap between and vent slots near the top.
            let mut dark = b::cbox([0.006, h - 0.1, 0.01], [0.0, 0.12 + h * 0.5, d * 0.5 + 0.003]);
            for x in [-w * 0.25, w * 0.25] {
                for j in 0..5 {
                    geo::merge(&mut dark, &b::cbox([w * 0.3, 0.015, 0.01], [x, 0.12 + h - 0.15 - j as f32 * 0.04, d * 0.5 + 0.003]));
                }
                geo::merge(&mut dark, &b::cbox([0.03, 0.08, 0.02], [x - x.signum() * (w * 0.25 - 0.06), 0.12 + h * 0.5, d * 0.5 + 0.01]));
            }
            let plinth = sh::block([w + 0.1, 0.12, d + 0.1], [0.0; 3]);
            Output::Parts(vec![Part::new(m, v.color("color")), Part::new(dark, [0.04, 0.05, 0.04]), Part::new(plinth, [0.5, 0.49, 0.46]).material("stone")])
        });

    basic!(_r, "city-billboards", "city", "billboards", "Billboards", "metal",
        [P::num("width", 8.0, 3.0, 16.0).unit("m").describe("Board width (m)"), P::num("height", 6.0, 3.0, 14.0).unit("m").describe("Height to the board's foot (m)"), P::color("color", [0.12, 0.35, 0.6]).describe("Poster colour")],
        |v: &Values, s: u32| {
            let (w, h) = (v.f("width"), v.f("height"));
            let bh = w * 0.4;
            let mut steel = b::rod([0.0, 0.0, 0.0], [0.0, h, 0.0], 0.3, 16);
            geo::merge(&mut steel, &b::cbox([w + 0.3, bh + 0.3, 0.25], [0.0, h + bh * 0.5, -0.15]));
            // A catwalk with a rail and three lamps on arms.
            geo::merge(&mut steel, &b::cbox([w, 0.06, 0.8], [0.0, h - 0.05, 0.4]));
            geo::merge(&mut steel, &b::bar([-w * 0.5, h + 0.9, 0.78], [w * 0.5, h + 0.9, 0.78], 0.04, 0.04));
            let mut lamps = geo::Mesh::default();
            for i in 0..3 {
                let x = (i as f32 - 1.0) * w * 0.33;
                geo::merge(&mut steel, &b::bar([x, h + bh + 0.1, -0.05], [x, h + bh + 0.3, 1.0], 0.05, 0.05));
                geo::merge(&mut lamps, &b::cbox([0.4, 0.12, 0.25], [x, h + bh + 0.32, 1.05]));
            }
            // The poster: a field with a seeded band and a block of "text".
            let poster = b::cbox([w, bh, 0.02], [0.0, h + bh * 0.5, 0.0]);
            let mut art = b::cbox([w, bh * sh::range(s, 1, 1, 0.2, 0.4), 0.02], [0.0, h + bh * 0.25, 0.01]);
            geo::merge(&mut art, &b::cbox([w * 0.45, bh * 0.12, 0.02], [-w * 0.2, h + bh * 0.75, 0.012]));
            Output::Parts(vec![Part::new(poster, v.color("color")), Part::new(art, [0.92, 0.88, 0.75]), Part::new(steel, STEEL), Part::new(lamps, LAMP).material("glow")])
        });

    basic!(_r, "city-crash-barriers", "city", "crash-barriers", "Crash barriers", "metal",
        [P::num("length", 12.0, 2.0, 40.0).unit("m").describe("Length (m)"), P::num("curve", 1.5, 0.0, 8.0).unit("m").describe("How far it bends (m)"), P::num("posts", 2.0, 1.0, 4.0).unit("m").describe("Post spacing (m)"), P::color("color", [0.55, 0.57, 0.58]).describe("Steel colour")],
        |v: &Values, _s: u32| {
            let (l, c, p) = (v.f("length"), v.f("curve"), v.f("posts"));
            let path = b::s_path(l, c, 0.55, 40);
            // A W-beam: two ridges and a valley, pressed from thin sheet.
            let t = 0.006;
            let wave = |y: f32| 0.04 * (y / 0.31 * sh::TAU).cos();
            let n = 12;
            let mut sec: Vec<[f32; 2]> = (0..=n).map(|i| {
                let y = (i as f32 / n as f32 - 0.5) * 0.31;
                [0.15 + wave(y), y]
            }).collect();
            sec.extend((0..=n).rev().map(|i| {
                let y = (i as f32 / n as f32 - 0.5) * 0.31;
                [0.15 + wave(y) - t, y]
            }));
            let rail = b::sweep(&path, &sec, true);
            let mut posts = geo::Mesh::default();
            let k = (b::path_len(&path) / p).floor() as u32;
            for i in 0..=k {
                let (pt, yaw, right) = b::along(&path, i as f32 / k.max(1) as f32);
                let at = [pt[0] + right[0] * 0.03, 0.0, pt[2] + right[2] * 0.03];
                let post = b::sweep(&[[0.0, 0.0, 0.0], [0.0, 0.75, 0.0]], &[[-0.05, -0.04], [0.05, -0.04], [0.05, -0.035], [-0.045, -0.035], [-0.045, 0.035], [0.05, 0.035], [0.05, 0.04], [-0.05, 0.04]], true);
                geo::merge(&mut posts, &sh::place(&post, 1.0, yaw, 0.0, at));
            }
            Output::Parts(vec![Part::new(rail, v.color("color")).material("metal"), Part::new(posts, [0.4, 0.42, 0.43]).material("metal")])
        });
}
