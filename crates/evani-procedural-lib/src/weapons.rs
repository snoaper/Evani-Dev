//! Category 21, Weapons, tools and combat equipment: basic generators
//! (batch C). Blades are lens sections lofted to a point, flat parts
//! (knives, axe heads, stocks) side outlines extruded thin, barrels rods and
//! turned profiles, hafts and bows tubes along curves. Shown in side view
//! (length along x, the side facing +z), resting on the ground. Inspired by
//! three.js (MIT) ExtrudeGeometry, LatheGeometry and TubeGeometry; written
//! fresh here.

use evani_procedural::Registry;

#[allow(unused_imports, dead_code)]
pub fn register(_r: &mut Registry) {
    use evani_procedural::{Output, ParamSpec as P, Part, Values, build as b, craft as c, geo, shapes as sh};
    type Mesh = geo::Mesh;
    const STEEL: [f32; 3] = [0.55, 0.56, 0.58];
    const GUN: [f32; 3] = [0.08, 0.08, 0.09];
    const WOOD: [f32; 3] = [0.38, 0.22, 0.1];
    const DARK: [f32; 3] = [0.05, 0.05, 0.05];

    fn moved(mut m: Mesh, d: [f32; 3]) -> Mesh {
        geo::transform(&mut m, [1.0; 3], d);
        m
    }

    /// A blade from `x0` along +x for `len`: a lens section `w` wide (up)
    /// and `t` thick, narrowing to a point over its last `tip` share.
    fn blade(x0: f32, y: f32, len: f32, w: f32, t: f32, tip: f32) -> Mesh {
        let st: Vec<c::Station> = (0..12).map(|i| {
            let u = i as f32 / 11.0;
            let k = if u > 1.0 - tip { ((1.0 - u) / tip).max(0.0).powf(0.8) } else { 1.0 - 0.15 * u };
            c::Station { at: [x0 + len * u, y, 0.0], u: [0.0, 1.0, 0.0], v: [0.0, 0.0, 1.0], profile: c::squircle((w * 0.5 * k).max(1e-4), (t * 0.5 * k.max(0.15)).max(1e-4), 1.2, 12) }
        }).collect();
        c::loft(&st, true, true)
    }

    /// A side outline `poly` (x, y) extruded `t` thick across z.
    fn flat(poly: &[[f32; 2]], t: f32) -> Mesh {
        b::extrude(poly, t)
    }

    basic!(_r, "weapons-swords", "weapons", "swords", "Swords", "metal",
        [P::num("length", 0.8, 0.4, 1.3).unit("m").describe("Blade length (m)"), P::num("width", 0.05, 0.025, 0.1).unit("m").describe("Blade width (m)"), P::num("guard", 0.2, 0.08, 0.35).unit("m").describe("Crossguard width (m)"), P::color("color", STEEL).describe("Blade colour"), P::color("grip", [0.25, 0.12, 0.06]).describe("Grip colour")],
        |v: &Values, _s: u32| {
            let (l, w, g) = (v.f("length"), v.f("width"), v.f("guard"));
            let y = g * 0.5;
            let bl = blade(0.0, y, l, w, w * 0.18, 0.18);
            let guard_m = b::rounded_box([0.025, g, 0.03], 0.008, 2, [-0.012, 0.0, 0.0]);
            let grip = c::limb([-0.03, y, 0.0], [-0.2, y, 0.0], 0.016, 0.015, 10);
            let pommel = c::ball([0.025, 0.025, 0.02], [-0.22, y, 0.0]);
            let mut metal = pommel;
            geo::merge(&mut metal, &guard_m);
            c::stand(vec![Part::new(bl, v.color("color")).material("metal"), Part::new(metal, [0.6, 0.5, 0.25]).material("metal"), Part::new(grip, v.color("grip")).material("ground")])
        });

    basic!(_r, "weapons-knives", "weapons", "knives", "Knives", "metal",
        [P::num("length", 0.15, 0.06, 0.35).unit("m").describe("Blade length (m)"), P::num("belly", 0.5, 0.0, 1.0).describe("Edge curve up to the tip"), P::color("color", [0.12, 0.1, 0.08]).describe("Handle colour")],
        |v: &Values, _s: u32| {
            let (l, belly) = (v.f("length"), v.f("belly"));
            let w = l * 0.2;
            // A straight spine, the edge curving up to the point.
            let mut poly = vec![[0.0, 0.0], [0.0, w], [l * 0.8, w], [l, w * 0.75]];
            for i in 1..8 {
                let t = i as f32 / 8.0;
                poly.push([l * (1.0 - t), w * 0.75 * (1.0 - t).powf(1.0 + belly * 2.0)]);
            }
            let bl = flat(&poly, l * 0.025);
            let handle = b::rounded_box([l * 0.75, w * 0.85, l * 0.08], w * 0.25, 3, [-l * 0.38, w * 0.05, 0.0]);
            let bolster = b::cbox([l * 0.04, w * 1.05, l * 0.09], [-l * 0.01, w * 0.5, 0.0]);
            c::stand(vec![Part::new(bl, STEEL).material("metal"), Part::new(handle, v.color("color")).material("ground"), Part::new(bolster, [0.5, 0.45, 0.35]).material("metal")])
        });

    basic!(_r, "weapons-axes", "weapons", "axes", "Axes", "ground",
        [P::num("haft", 0.75, 0.35, 1.2).unit("m").describe("Haft length (m)"), P::num("head", 0.18, 0.1, 0.32).unit("m").describe("Head size (m)"), P::num("bits", 1.0, 1.0, 2.0).step(1.0).describe("Bits (2 a double-bit axe)"), P::color("color", WOOD).describe("Haft colour")],
        |v: &Values, _s: u32| {
            let (l, hs, n) = (v.f("haft"), v.f("head"), v.f("bits").round() as u32);
            // A gently curved haft along x; the head at its +x end, bit up.
            let path: Vec<[f32; 3]> = (0..8).map(|i| {
                let t = i as f32 / 7.0;
                [l * t, 0.03 + 0.03 * (t * sh::PI).sin(), 0.0]
            }).collect();
            let haft = sh::tube(&path, |t| 0.016 + 0.004 * t, 10);
            let mut head = flat(&[[-hs * 0.12, -hs * 0.1], [hs * 0.12, -hs * 0.1], [hs * 0.18, hs * 0.2], [hs * 0.35, hs * 0.75], [-hs * 0.3, hs * 0.85], [-hs * 0.15, hs * 0.2]], hs * 0.12);
            if n >= 2 {
                let mut other = head.clone();
                sh::rotate(&mut other, [0.0, 0.0, 1.0], sh::PI);
                geo::merge(&mut head, &other);
            } else {
                geo::merge(&mut head, &b::cbox([hs * 0.22, hs * 0.25, hs * 0.16], [0.0, -hs * 0.18, 0.0]));
            }
            let head = moved(head, [l * 0.96, 0.06, 0.0]);
            c::stand(vec![Part::new(haft, v.color("color")).material("ground"), Part::new(head, STEEL).material("metal")])
        });

    basic!(_r, "weapons-spears", "weapons", "spears", "Spears", "ground",
        [P::num("length", 2.2, 1.2, 3.5).unit("m").describe("Length (m)"), P::num("head", 0.3, 0.12, 0.6).unit("m").describe("Head length (m)"), P::color("color", WOOD).describe("Shaft colour")],
        |v: &Values, _s: u32| {
            let (l, hl) = (v.f("length"), v.f("head"));
            let y = 0.03;
            let shaft = b::rod([0.0, y, 0.0], [l - hl, y, 0.0], 0.016, 10);
            // A leaf-shaped head on a socket.
            let st: Vec<c::Station> = (0..12).map(|i| {
                let u = i as f32 / 11.0;
                let k = (u * sh::PI).sin().powf(0.7) * (1.0 - u * 0.4);
                c::Station { at: [l - hl + hl * u, y, 0.0], u: [0.0, 1.0, 0.0], v: [0.0, 0.0, 1.0], profile: c::squircle((hl * 0.18 * k).max(0.012 * (1.0 - u)).max(1e-4), (hl * 0.03 * k).max(0.01 * (1.0 - u)).max(1e-4), 1.2, 12) }
            }).collect();
            let mut head = c::loft(&st, true, true);
            geo::merge(&mut head, &c::limb([l - hl - 0.08, y, 0.0], [l - hl + 0.02, y, 0.0], 0.02, 0.016, 10));
            let butt = c::cone([0.0, y, 0.0], [-0.06, y, 0.0], 0.018, 10);
            geo::merge(&mut head, &butt);
            c::stand(vec![Part::new(shaft, v.color("color")).material("ground"), Part::new(head, STEEL).material("metal")])
        });

    basic!(_r, "weapons-bows", "weapons", "bows", "Bows", "ground",
        [P::num("length", 1.6, 0.9, 2.0).unit("m").describe("Length tip to tip (m)"), P::num("draw", 0.0, 0.0, 0.7).unit("m").describe("String drawn back (m)"), P::num("bend", 0.15, 0.05, 0.3).describe("Limb bend at rest"), P::color("color", [0.45, 0.28, 0.12]).describe("Wood colour")],
        |v: &Values, _s: u32| {
            let (l, dr, bend) = (v.f("length"), v.f("draw"), v.f("bend"));
            // Upright, the limbs curving forward (+x); drawn, they bend further.
            let k = bend + dr * 0.25;
            let half = l * 0.5;
            let arc: Vec<[f32; 3]> = (0..=16).map(|i| {
                let t = i as f32 / 16.0 * 2.0 - 1.0;
                [l * k * (1.0 - t * t), half + half * t * (1.0 - k * 0.3), 0.0]
            }).collect();
            let limbs = sh::tube(&arc, |t| 0.018 * (1.0 - 0.55 * (2.0 * t - 1.0).abs()), 8);
            let (top, bot) = (arc[16], arc[0]);
            let mut string = b::rod(top, [-dr, half, 0.0], 0.003, 5);
            geo::merge(&mut string, &b::rod([-dr, half, 0.0], bot, 0.003, 5));
            let grip = c::limb([l * k, half - 0.06, 0.0], [l * k, half + 0.06, 0.0], 0.024, 0.024, 10);
            c::stand(vec![Part::new(limbs, v.color("color")).material("ground"), Part::new(string, [0.85, 0.82, 0.72]).material("ground"), Part::new(grip, [0.15, 0.08, 0.04]).material("ground")])
        });

    basic!(_r, "weapons-arrows", "weapons", "arrows", "Arrows", "ground",
        [P::num("length", 0.75, 0.4, 1.0).unit("m").describe("Length (m)"), P::num("vanes", 0.1, 0.04, 0.18).unit("m").describe("Fletching length (m)"), P::color("color", [0.6, 0.45, 0.25]).describe("Shaft colour"), P::color("fletch", [0.65, 0.08, 0.06]).describe("Fletching colour")],
        |v: &Values, _s: u32| {
            let (l, vl) = (v.f("length"), v.f("vanes"));
            let y = 0.025;
            let shaft = b::rod([0.0, y, 0.0], [l, y, 0.0], 0.004, 8);
            // A broadhead, three vanes round the back and a nock.
            let head = flat(&[[l, y - 0.015], [l + 0.07, y], [l, y + 0.015], [l - 0.01, y]], 0.003);
            let mut vanes = Mesh::default();
            for k in 0..3 {
                let a = k as f32 / 3.0 * sh::TAU;
                let mut f = flat(&[[0.02, 0.0], [0.02 + vl, 0.0], [0.04 + vl, 0.004], [0.04, 0.018]], 0.0015);
                sh::rotate(&mut f, [1.0, 0.0, 0.0], a);
                geo::merge(&mut vanes, &moved(f, [0.0, y, 0.0]));
            }
            let mut tips = head;
            geo::merge(&mut tips, &b::rod([-0.012, y, 0.0], [0.0, y, 0.0], 0.005, 8));
            c::stand(vec![Part::new(shaft, v.color("color")).material("ground"), Part::new(tips, STEEL).material("metal"), Part::new(vanes, v.color("fletch")).material("ground")])
        });

    basic!(_r, "weapons-shields", "weapons", "shields", "Shields", "ground",
        [P::num("size", 0.8, 0.4, 1.2).unit("m").describe("Height (m)"), P::num("shape", 0.0, 0.0, 2.0).step(1.0).describe("Shape: 0 round, 1 heater, 2 kite"), P::num("boss", 1.0, 0.0, 2.0).describe("Boss size"), P::color("color", [0.5, 0.08, 0.06]).describe("Face colour"), P::color("rim", STEEL).describe("Rim colour")],
        |v: &Values, _s: u32| {
            let (s, shape, boss) = (v.f("size"), v.f("shape").round() as u32, v.f("boss"));
            // Standing on its rim, facing +z.
            let half = s * 0.5;
            let poly: Vec<[f32; 2]> = match shape {
                0 => b::circle(half, 40).into_iter().map(|p| [p[0], p[1] + half]).collect(),
                1 => {
                    let mut p = vec![[-half * 0.8, s], [half * 0.8, s]];
                    for i in 0..=12 {
                        let t = i as f32 / 12.0;
                        p.push([half * 0.8 * (1.0 - t).powf(0.6), s * 0.55 * (1.0 - t)]);
                    }
                    for i in 1..12 {
                        let t = 1.0 - i as f32 / 12.0;
                        p.push([-half * 0.8 * (1.0 - t).powf(0.6), s * 0.55 * (1.0 - t)]);
                    }
                    p
                }
                _ => {
                    let mut p = Vec::new();
                    for i in 0..=12 {
                        let a = i as f32 / 12.0 * sh::PI;
                        p.push([half * 0.6 * a.cos(), s * 0.8 + half * 0.4 * a.sin()]);
                    }
                    p.push([0.0, 0.0]);
                    p
                }
            };
            let face = flat(&poly, 0.03);
            let mut ring: Vec<[f32; 3]> = poly.iter().map(|p| [p[0], p[1], 0.0]).collect();
            if let Some(f) = ring.first().copied() {
                ring.push(f);
            }
            let rim = b::sweep(&ring, &b::rect(0.02, 0.045), false);
            let mut fittings = Mesh::default();
            if boss > 0.01 {
                let cy = if shape == 0 { half } else { s * 0.62 };
                geo::merge(&mut fittings, &c::ball([s * 0.09 * boss, s * 0.09 * boss, s * 0.05 * boss + 0.01], [0.0, cy, 0.015]));
            }
            geo::merge(&mut fittings, &rim);
            c::stand(vec![Part::new(face, v.color("color")).material("ground"), Part::new(fittings, v.color("rim")).material("metal")])
        });

    basic!(_r, "weapons-pistols", "weapons", "pistols", "Pistols", "metal",
        [P::num("length", 0.19, 0.14, 0.3).unit("m").describe("Slide length (m)"), P::num("grip", 15.0, 0.0, 30.0).unit("deg").describe("Grip rake (degrees)"), P::color("color", GUN).describe("Frame colour")],
        |v: &Values, _s: u32| {
            let (l, ga) = (v.f("length"), v.f("grip").to_radians());
            let y = 0.12;
            let slide = b::cbox([l, 0.032, 0.026], [l * 0.5, y + 0.016, 0.0]);
            let mut frame = b::cbox([l * 0.8, 0.02, 0.024], [l * 0.45, y - 0.01, 0.0]);
            // The grip raked back, a trigger guard loop, sights and a muzzle.
            let mut grip = b::rounded_box([0.03, 0.11, 0.03], 0.006, 2, [0.0, -0.11, 0.0]);
            sh::rotate(&mut grip, [0.0, 0.0, 1.0], ga);
            geo::merge(&mut frame, &moved(grip, [0.03, y, 0.0]));
            let guard: Vec<[f32; 3]> = (0..=12).map(|k| {
                let a = sh::PI + k as f32 / 12.0 * sh::PI;
                [0.075 + 0.025 * a.cos(), y - 0.02 + 0.025 * a.sin(), 0.0]
            }).collect();
            geo::merge(&mut frame, &b::sweep(&guard, &b::rect(0.004, 0.008), true));
            geo::merge(&mut frame, &b::cbox([0.004, 0.016, 0.004], [0.072, y - 0.025, 0.0]));
            let mut parts = b::cbox([0.006, 0.006, 0.006], [l * 0.95, y + 0.018, 0.0]);
            geo::merge(&mut parts, &b::cbox([0.006, 0.006, 0.016], [l * 0.05, y + 0.018, 0.0]));
            geo::merge(&mut parts, &b::rod([l, y + 0.004, 0.0], [l + 0.004, y + 0.004, 0.0], 0.006, 10));
            c::stand(vec![Part::new(slide, [0.12, 0.12, 0.13]).material("metal"), Part::new(frame, v.color("color")).material("ground"), Part::new(parts, DARK).material("metal")])
        });

    basic!(_r, "weapons-rifles", "weapons", "rifles", "Rifles", "ground",
        [P::num("barrel", 0.6, 0.35, 0.9).unit("m").describe("Barrel length (m)"), P::num("scope", 1.0, 0.0, 1.0).step(1.0).describe("A scope (1) or iron sights (0)"), P::color("color", WOOD).describe("Stock colour")],
        |v: &Values, _s: u32| {
            let (bl, scope) = (v.f("barrel"), v.f("scope") > 0.5);
            let y = 0.12;
            // A wooden stock and forend in side outline, a receiver, barrel, magazine.
            let stock = flat(&[[-0.42, y - 0.09], [-0.12, y - 0.03], [0.0, y - 0.035], [0.32, y - 0.02], [0.33, y + 0.005], [-0.05, y + 0.005], [-0.42, y + 0.02]], 0.04);
            let mut metal = b::cbox([0.22, 0.04, 0.034], [0.05, y + 0.02, 0.0]);
            geo::merge(&mut metal, &b::rod([0.15, y + 0.025, 0.0], [0.15 + bl, y + 0.025, 0.0], 0.009, 10));
            geo::merge(&mut metal, &b::cbox([0.04, 0.07, 0.022], [0.05, y - 0.04, 0.0]));
            geo::merge(&mut metal, &b::rod([0.12, y + 0.03, 0.02], [0.08, y + 0.04, 0.035], 0.005, 6));
            if scope {
                geo::merge(&mut metal, &b::rod([-0.04, y + 0.08, 0.0], [0.2, y + 0.08, 0.0], 0.014, 14));
                for x in [-0.06f32, 0.22] {
                    geo::merge(&mut metal, &b::rod([x - 0.03, y + 0.08, 0.0], [x + 0.03, y + 0.08, 0.0], 0.02, 14));
                }
                for x in [0.0f32, 0.15] {
                    geo::merge(&mut metal, &b::cbox([0.015, 0.04, 0.016], [x, y + 0.055, 0.0]));
                }
            } else {
                geo::merge(&mut metal, &b::cbox([0.006, 0.018, 0.004], [0.14 + bl, y + 0.04, 0.0]));
            }
            c::stand(vec![Part::new(stock, v.color("color")).material("ground"), Part::new(metal, GUN).material("metal")])
        });

    basic!(_r, "weapons-shotguns", "weapons", "shotguns", "Shotguns", "ground",
        [P::num("barrel", 0.7, 0.35, 0.8).unit("m").describe("Barrel length (m)"), P::num("barrels", 2.0, 1.0, 2.0).step(1.0).describe("Barrels side by side"), P::color("color", [0.42, 0.24, 0.1]).describe("Wood colour")],
        |v: &Values, _s: u32| {
            let (bl, n) = (v.f("barrel"), v.f("barrels").round() as u32);
            let y = 0.12;
            let mut wood = flat(&[[-0.4, y - 0.1], [-0.08, y - 0.03], [0.02, y - 0.03], [0.02, y + 0.01], [-0.06, y + 0.01], [-0.4, y + 0.02]], 0.042);
            geo::merge(&mut wood, &b::rounded_box([0.26, 0.035, 0.045], 0.01, 2, [0.2, y - 0.025, 0.0]));
            let mut metal = b::cbox([0.12, 0.045, 0.042], [0.06, y - 0.005, 0.0]);
            for k in 0..n {
                let z = if n == 1 { 0.0 } else if k == 0 { -0.011 } else { 0.011 };
                geo::merge(&mut metal, &b::rod([0.12, y + 0.015, z], [0.12 + bl, y + 0.015, z], 0.011, 12));
            }
            geo::merge(&mut metal, &b::rod([0.02, y - 0.02, 0.0], [0.015, y - 0.045, 0.0], 0.003, 5));
            c::stand(vec![Part::new(wood, v.color("color")).material("ground"), Part::new(metal, [0.12, 0.12, 0.14]).material("metal")])
        });

    basic!(_r, "weapons-machine-guns", "weapons", "machine-guns", "Machine guns", "metal",
        [P::num("barrel", 0.6, 0.35, 1.0).unit("m").describe("Barrel length (m)"), P::num("spread", 30.0, 0.0, 60.0).unit("deg").describe("Bipod leg spread (degrees)"), P::color("color", [0.18, 0.2, 0.14]).describe("Body colour")],
        |v: &Values, _s: u32| {
            let (bl, sp) = (v.f("barrel"), v.f("spread").to_radians());
            let y = 0.3;
            // Receiver and stock, a jacketed barrel, a bipod, a box and belt.
            let mut body = b::cbox([0.35, 0.08, 0.06], [0.0, y, 0.0]);
            geo::merge(&mut body, &flat(&[[-0.18, y - 0.03], [-0.45, y - 0.06], [-0.45, y + 0.04], [-0.18, y + 0.03]], 0.04));
            geo::merge(&mut body, &b::rod([0.17, y, 0.0], [0.17 + bl * 0.6, y, 0.0], 0.03, 14));
            let mut steel = b::rod([0.17, y, 0.0], [0.17 + bl, y, 0.0], 0.012, 10);
            for i in 0..6 {
                let x = 0.2 + i as f32 * bl * 0.1;
                geo::merge(&mut steel, &b::rod([x, y, 0.0], [x + 0.01, y, 0.0], 0.034, 14));
            }
            let pivot = [0.17 + bl * 0.55, y - 0.03, 0.0];
            for s in [-1.0f32, 1.0] {
                let foot = [pivot[0] + 0.05, 0.0, s * y * sp.sin()];
                geo::merge(&mut steel, &b::rod(pivot, foot, 0.008, 6));
            }
            let mut ammo = b::cbox([0.12, 0.1, 0.08], [0.0, y - 0.09, -0.08]);
            let belt: Vec<[f32; 3]> = (0..8).map(|i| {
                let t = i as f32 / 7.0;
                [0.0, y - 0.04 + 0.04 * t, -0.08 + 0.08 * t]
            }).collect();
            geo::merge(&mut ammo, &b::sweep(&belt, &b::rect(0.05, 0.012), true));
            geo::merge(&mut body, &b::cbox([0.03, 0.08, 0.025], [-0.05, y - 0.07, 0.0]));
            c::stand(vec![Part::new(body, v.color("color")).material("ground"), Part::new(steel, GUN).material("metal"), Part::new(ammo, [0.55, 0.42, 0.15]).material("metal")])
        });

    basic!(_r, "weapons-cannons", "weapons", "cannons", "Cannons", "ground",
        [P::num("barrel", 2.0, 1.0, 3.5).unit("m").describe("Barrel length (m)"), P::num("lift", 8.0, -5.0, 30.0).unit("deg").describe("Elevation (degrees)"), P::num("wheel", 0.6, 0.35, 0.9).unit("m").describe("Wheel radius (m)"), P::color("color", [0.18, 0.18, 0.17]).describe("Barrel colour")],
        |v: &Values, _s: u32| {
            let (bl, el, wr) = (v.f("barrel"), v.f("lift").to_radians(), v.f("wheel"));
            // A turned barrel with reinforcing rings and a cascabel, on a carriage.
            let r = bl * 0.08;
            let mut prof = vec![[0.0, -bl * 0.32]];
            prof.push([r * 0.5, -bl * 0.32]);
            prof.push([r * 1.2, -bl * 0.27]);
            prof.push([r * 1.25, -bl * 0.05]);
            prof.push([r * 1.05, 0.0]);
            prof.push([r * 0.75, bl * 0.62]);
            prof.push([r * 0.95, bl * 0.64]);
            prof.push([r * 0.95, bl * 0.68]);
            prof.push([r * 0.45, bl * 0.68]);
            prof.push([r * 0.45, bl * 0.5]);
            prof.push([0.0, bl * 0.5]);
            let mut barrel = b::revolve(&prof, 28);
            geo::merge(&mut barrel, &c::ball([r * 0.4; 3], [0.0, -bl * 0.36, 0.0]));
            for yy in [bl * 0.1, bl * 0.35] {
                geo::merge(&mut barrel, &b::rod([0.0, yy, 0.0], [0.0, yy + bl * 0.03, 0.0], r * 1.12, 28));
            }
            // Lay it along +x, raised by the elevation, the trunnions at the axle.
            sh::rotate(&mut barrel, [0.0, 0.0, 1.0], -sh::PI * 0.5 + el);
            let ax = [0.0, wr, 0.0];
            let barrel = moved(barrel, [ax[0], ax[1] + r * 1.4, 0.0]);
            let mut wood = b::bar([0.1, wr + r * 0.6, 0.0], [-bl * 0.75, 0.05, 0.0], r * 2.6, r * 1.2);
            geo::merge(&mut wood, &b::rod([0.0, wr, -wr * 0.75], [0.0, wr, wr * 0.75], r * 0.5, 10));
            let (mut tyres, mut hubs) = (Mesh::default(), Mesh::default());
            for z in [-wr * 0.72, wr * 0.72] {
                let (t, h) = c::wheel([0.0, wr, z], wr, wr * 0.12, 12);
                geo::merge(&mut tyres, &t);
                geo::merge(&mut hubs, &h);
            }
            geo::merge(&mut wood, &hubs);
            Output::Parts(vec![Part::new(barrel, v.color("color")).material("metal"), Part::new(wood, [0.36, 0.22, 0.1]).material("ground"), Part::new(tyres, [0.15, 0.15, 0.15]).material("metal")])
        });

    basic!(_r, "weapons-turrets", "weapons", "turrets", "Turrets", "metal",
        [P::num("turn", 30.0, -180.0, 180.0).unit("deg").describe("Turn (degrees)"), P::num("lift", 10.0, -10.0, 60.0).unit("deg").describe("Elevation (degrees)"), P::num("barrels", 2.0, 1.0, 4.0).step(1.0).describe("Barrels"), P::color("color", [0.3, 0.33, 0.3]).describe("Housing colour")],
        |v: &Values, _s: u32| {
            let (turn, el, n) = (v.f("turn").to_radians(), v.f("lift").to_radians(), v.f("barrels").round() as u32);
            // A turned pedestal; the housing and guns turn about it and raise.
            let base = b::revolve(&[[0.0, 0.0], [0.9, 0.0], [0.9, 0.15], [0.55, 0.3], [0.5, 1.2], [0.7, 1.3], [0.0, 1.3]], 32);
            let house = b::rounded_box([1.4, 0.7, 1.2], 0.12, 3, [0.0, 1.3, 0.0]);
            let (ce, se) = (el.cos(), el.sin());
            let mut guns = Mesh::default();
            for k in 0..n {
                let z = (k as f32 - (n - 1) as f32 * 0.5) * 0.22;
                let a = [0.65, 1.65, z];
                geo::merge(&mut guns, &b::rod(a, [a[0] + 1.6 * ce, a[1] + 1.6 * se, z], 0.05, 12));
                geo::merge(&mut guns, &b::rod([a[0] + 1.45 * ce, a[1] + 1.45 * se, z], [a[0] + 1.62 * ce, a[1] + 1.62 * se, z], 0.075, 12));
            }
            let house = sh::place(&house, 1.0, turn, 0.0, [0.0; 3]);
            let guns = sh::place(&guns, 1.0, turn, 0.0, [0.0; 3]);
            Output::Parts(vec![Part::new(base, [0.22, 0.23, 0.24]).material("metal"), Part::new(house, v.color("color")).material("ground"), Part::new(guns, GUN).material("metal")])
        });

    basic!(_r, "weapons-pickaxes", "weapons", "pickaxes", "Pickaxes", "ground",
        [P::num("handle", 0.9, 0.4, 1.1).unit("m").describe("Handle length (m)"), P::num("span", 0.55, 0.3, 0.8).unit("m").describe("Head span (m)"), P::color("color", WOOD).describe("Handle colour")],
        |v: &Values, _s: u32| {
            let (l, sp) = (v.f("handle"), v.f("span"));
            let handle = b::rod([0.0, 0.03, 0.0], [l, 0.03, 0.0], 0.018, 10);
            // A head curving down at both ends to points, across the handle's end.
            let arc: Vec<[f32; 3]> = (0..=14).map(|i| {
                let t = i as f32 / 14.0 * 2.0 - 1.0;
                [l - 0.02 - sp * 0.12 * t * t, 0.03 + sp * 0.5 * t, 0.0]
            }).collect();
            let mut head = sh::tube(&arc, |t| 0.025 * (1.0 - (2.0 * t - 1.0).abs().powf(1.5)).max(0.04), 10);
            geo::merge(&mut head, &b::rod([l - 0.05, 0.03, 0.0], [l + 0.02, 0.03, 0.0], 0.03, 12));
            c::stand(vec![Part::new(handle, v.color("color")).material("ground"), Part::new(head, [0.3, 0.3, 0.32]).material("metal")])
        });

    basic!(_r, "weapons-shovels", "weapons", "shovels", "Shovels", "ground",
        [P::num("length", 1.1, 0.6, 1.5).unit("m").describe("Shaft length (m)"), P::num("width", 0.22, 0.14, 0.32).unit("m").describe("Blade width (m)"), P::color("color", WOOD).describe("Shaft colour")],
        |v: &Values, _s: u32| {
            let (l, w) = (v.f("length"), v.f("width"));
            let y = 0.03;
            let mut shaft = b::rod([0.0, y, 0.0], [l, y, 0.0], 0.017, 10);
            // A D-handle at the back, a dished blade with a pointed tip at the front.
            let grip: Vec<[f32; 3]> = (0..=12).map(|k| {
                let a = sh::PI * 0.5 + k as f32 / 12.0 * sh::PI;
                [-0.06 + 0.08 * a.cos(), y, 0.06 * a.sin()]
            }).collect();
            geo::merge(&mut shaft, &b::sweep(&grip, &b::circle(0.012, 8), true));
            geo::merge(&mut shaft, &b::rod([-0.06, y, -0.06], [-0.06, y, 0.06], 0.014, 8));
            let st: Vec<c::Station> = (0..10).map(|i| {
                let t = i as f32 / 9.0;
                let half = w * 0.5 * if t > 0.7 { 1.0 - ((t - 0.7) / 0.3).powi(2) * 0.8 } else { 0.85 + 0.15 * t / 0.7 };
                let prof: Vec<[f32; 2]> = (0..=10).map(|k| {
                    let u = k as f32 / 10.0 * 2.0 - 1.0;
                    [u * half, 0.03 * w * (1.0 - u * u) * 4.0]
                }).chain((0..=10).rev().map(|k| {
                    let u = k as f32 / 10.0 * 2.0 - 1.0;
                    [u * half, 0.03 * w * (1.0 - u * u) * 4.0 + 0.004]
                })).collect();
                c::Station { at: [l + w * 1.3 * t, y, 0.0], u: [0.0, 0.0, 1.0], v: [0.0, 1.0, 0.0], profile: prof }
            }).collect();
            let mut blade_m = c::loft(&st, true, true);
            geo::merge(&mut blade_m, &c::limb([l - 0.08, y, 0.0], [l + 0.03, y, 0.0], 0.02, 0.024, 10));
            c::stand(vec![Part::new(shaft, v.color("color")).material("ground"), Part::new(blade_m, [0.32, 0.33, 0.35]).material("metal")])
        });
}
