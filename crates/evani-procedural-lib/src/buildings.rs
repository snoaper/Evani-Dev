//! Category 9, Buildings and structures: basic generators (batch B). Each is
//! a few parts composed from boxes, extruded profiles (gables, gambrels,
//! sawtooth roofs, barrel vaults, arched windows) and revolved towers, with
//! its windows as their own glass part. The front faces +z. Inspired by
//! three.js (MIT) BoxGeometry, ExtrudeGeometry and CylinderGeometry, and by
//! the usual procedural-building idea of a footprint split into floors and
//! bays; written fresh here.

use evani_procedural::Registry;

#[allow(unused_imports, dead_code)]
pub fn register(_r: &mut Registry) {
    use evani_procedural::{Output, ParamSpec as P, Part, Values, build as b, geo, shapes as sh};
    const GLASS: [f32; 3] = [0.05, 0.07, 0.09];
    const SLATE: [f32; 3] = [0.14, 0.14, 0.16];
    const TILE: [f32; 3] = [0.36, 0.14, 0.08];
    const DOOR: [f32; 3] = [0.17, 0.1, 0.06];
    const TRIM: [f32; 3] = [0.78, 0.76, 0.72];
    const BRICK: [f32; 3] = [0.42, 0.18, 0.11];

    fn glass(m: geo::Mesh) -> Part {
        Part::new(m, GLASS).material("glass")
    }

    /// `m` moved by `d`.
    fn moved(mut m: geo::Mesh, d: [f32; 3]) -> geo::Mesh {
        geo::transform(&mut m, [1.0; 3], d);
        m
    }

    /// A gable roof whose ridge runs along z (the gable faces the front).
    fn gable_z(w: f32, d: f32, h: f32, y: f32, o: f32) -> geo::Mesh {
        let mut m = b::gable(d, w, h, y, o);
        sh::rotate(&mut m, [0.0, 1.0, 0.0], sh::PI * 0.5);
        m
    }

    /// Merlons (crenellations) along a wall top from `a` to `c` (x, z) at
    /// height `y`, `t` thick.
    fn merlons(a: [f32; 2], c: [f32; 2], y: f32, t: f32) -> geo::Mesh {
        let (dx, dz) = (c[0] - a[0], c[1] - a[1]);
        let len = (dx * dx + dz * dz).sqrt();
        let n = (len / 1.2).floor().max(1.0) as u32;
        let mut m = geo::Mesh::default();
        for i in 0..n {
            let t0 = (i as f32 + 0.25) / n as f32;
            let at = [a[0] + dx * t0, y, a[1] + dz * t0];
            let one = sh::place(&sh::block([len / n as f32 * 0.5, 0.8, t], [0.0; 3]), 1.0, (-dz).atan2(dx), 0.0, [0.0; 3]);
            geo::merge(&mut m, &moved(one, at));
        }
        m
    }

    /// Sawtooth roof bays along x from `x0`, each `bay` wide and `d` deep,
    /// at height `y`: the solid teeth and their glazed north lights.
    fn sawtooth(x0: f32, bays: u32, bay: f32, d: f32, y: f32) -> (geo::Mesh, geo::Mesh) {
        let (mut roof, mut lights) = (geo::Mesh::default(), geo::Mesh::default());
        let h = bay * 0.45;
        for i in 0..bays {
            let x = x0 + i as f32 * bay;
            geo::merge(&mut roof, &moved(b::extrude(&[[x, 0.0], [x + bay, 0.0], [x + bay, h * 0.25], [x + bay * 0.08, h]], d), [0.0, y, 0.0]));
            geo::merge(&mut lights, &b::cbox([0.08, h * 0.7, d - 0.4], [x + bay * 0.06, y + h * 0.55, 0.0]));
        }
        (roof, lights)
    }

    basic!(_r, "buildings-log-cabins", "buildings", "log-cabins", "Log cabins", "ground",
        [P::num("width", 6.0, 3.0, 14.0).unit("m").describe("Width (m)"), P::num("depth", 5.0, 3.0, 12.0).unit("m").describe("Depth (m)"), P::num("logs", 10.0, 6.0, 18.0).step(1.0).describe("Log courses"), P::color("color", [0.33, 0.2, 0.11]).describe("Log colour")],
        |v: &Values, s: u32| {
            let (w, d, n) = (v.f("width"), v.f("depth"), v.f("logs").round() as u32);
            let r = 0.15;
            let mut logs = geo::Mesh::default();
            for i in 0..n {
                let y = r + i as f32 * r * 1.8;
                let j = sh::range(s, i, 1, -0.05, 0.05);
                for z in [-d * 0.5, d * 0.5] {
                    geo::merge(&mut logs, &b::rod([-w * 0.5 - 0.35 + j, y, z], [w * 0.5 + 0.35 + j, y, z], r, 10));
                }
                for x in [-w * 0.5, w * 0.5] {
                    geo::merge(&mut logs, &b::rod([x, y + r * 0.9, -d * 0.5 - 0.35 - j], [x, y + r * 0.9, d * 0.5 + 0.35 - j], r, 10));
                }
            }
            let top = n as f32 * r * 1.8 + r;
            let roof = b::gable(w, d, d * 0.4, top, 0.5);
            let door = b::cbox([1.0, 2.0, 0.1], [w * 0.15, 1.0, d * 0.5 + r]);
            let panes = [b::cbox([0.9, 0.8, 0.1], [-w * 0.25, 1.5, d * 0.5 + r]), b::cbox([0.1, 0.8, 0.9], [w * 0.5 + r, 1.5, 0.0])];
            let mut win = geo::Mesh::default();
            panes.iter().for_each(|p| geo::merge(&mut win, p));
            Output::Parts(vec![Part::new(logs, v.color("color")), Part::new(roof, [0.2, 0.15, 0.1]), Part::new(door, DOOR), glass(win)])
        });

    // The townhouses (Asset Studio column 37): their own file,
    // crate::townhouse (a terrace of brick, stone and stucco houses under
    // one slate roof, sash windows, fanlit doors up stone steps).
    #[cfg(feature = "buildings-townhouses")]
    _r.add(Box::new(evani_procedural::FnAsset {
        info: evani_procedural::AssetInfo::cpu("buildings", "townhouses", "Townhouses").material("stone"),
        params: &crate::townhouse::PARAMS,
        make: crate::townhouse::generate,
    }));

    basic!(_r, "buildings-farmhouses", "buildings", "farmhouses", "Farmhouses", "stone",
        [P::num("width", 10.0, 6.0, 20.0).unit("m").describe("Width (m)"), P::num("floors", 2.0, 1.0, 3.0).step(1.0).describe("Floors"), P::color("color", [0.78, 0.75, 0.68]).describe("Wall colour")],
        |v: &Values, _s: u32| {
            let (w, f) = (v.f("width"), v.f("floors").round() as u32);
            let (d, h) = (7.0, f as f32 * 2.8);
            let walls = sh::block([w, h, d], [0.0; 3]);
            let mut roof = b::gable(w, d, 2.8, h, 0.4);
            geo::merge(&mut roof, &sh::block([0.9, 2.2, 0.9], [w * 0.35, h + 1.4, 0.0]));
            let win = b::windows(w, d, 0.0, f, 2.8, 4, [0.9, 1.3]);
            let mut wood = b::cbox([1.0, 2.1, 0.1], [0.0, 1.05, d * 0.5 + 0.02]);
            // A porch: a deck, four posts and a lean-to roof.
            geo::merge(&mut wood, &sh::block([w * 0.6, 0.25, 2.2], [0.0, 0.0, d * 0.5 + 1.1]));
            for x in [-w * 0.28, -w * 0.1, w * 0.1, w * 0.28] {
                geo::merge(&mut wood, &b::post(x, d * 0.5 + 2.0, 0.15, 2.6));
            }
            // extrude_x turns the profile's x into -z: the porch is in front.
            let porch = b::extrude_x(&[[-d * 0.5, 2.9], [-d * 0.5 - 2.4, 2.5], [-d * 0.5 - 2.4, 2.6], [-d * 0.5, 3.0]], w * 0.64);
            geo::merge(&mut roof, &porch);
            Output::Parts(vec![Part::new(walls, v.color("color")), Part::new(roof, SLATE), glass(win), Part::new(wood, [0.3, 0.27, 0.24]).material("ground")])
        });

    basic!(_r, "buildings-barns", "buildings", "barns", "Barns", "ground",
        [P::num("length", 16.0, 8.0, 40.0).unit("m").describe("Length (m)"), P::num("width", 10.0, 6.0, 20.0).unit("m").describe("Width (m)"), P::color("color", [0.42, 0.09, 0.06]).describe("Barn colour")],
        |v: &Values, _s: u32| {
            let (l, w) = (v.f("length"), v.f("width"));
            let (hw, wall) = (w * 0.5, w * 0.42);
            // A gambrel: steep lower slopes, shallow upper ones; the gable
            // faces the front (+z).
            let g = [[-hw, 0.0], [hw, 0.0], [hw, wall], [hw * 0.6, wall + w * 0.32], [0.0, wall + w * 0.45], [-hw * 0.6, wall + w * 0.32], [-hw, wall]];
            let body = b::extrude(&g, l);
            let o = 0.25;
            let band = [[hw + o, wall - o], [hw * 0.6 + o * 0.6, wall + w * 0.32 + o * 0.5], [0.0, wall + w * 0.45 + o], [-hw * 0.6 - o * 0.6, wall + w * 0.32 + o * 0.5], [-hw - o, wall - o], [-hw, wall], [-hw * 0.6, wall + w * 0.32], [0.0, wall + w * 0.45], [hw * 0.6, wall + w * 0.32], [hw, wall]];
            let roof = b::extrude(&band, l + 0.6);
            let z = l * 0.5;
            let mut trim = geo::Mesh::default();
            let (dw, dh) = (w * 0.4, wall * 0.85);
            for (a, c) in [([-dw * 0.5, 0.0], [-dw * 0.5, dh]), ([dw * 0.5, 0.0], [dw * 0.5, dh]), ([-dw * 0.5, dh], [dw * 0.5, dh]), ([-dw * 0.5, 0.0], [dw * 0.5, dh]), ([dw * 0.5, 0.0], [-dw * 0.5, dh])] {
                geo::merge(&mut trim, &b::bar([a[0], a[1], z + 0.08], [c[0], c[1], z + 0.08], 0.2, 0.1));
            }
            let door = b::cbox([dw, dh, 0.08], [0.0, dh * 0.5, z + 0.02]);
            Output::Parts(vec![Part::new(body, v.color("color")), Part::new(roof, [0.22, 0.22, 0.23]), Part::new(door, [0.3, 0.06, 0.04]), Part::new(trim, TRIM)])
        });

    basic!(_r, "buildings-warehouses", "buildings", "warehouses", "Warehouses", "ground",
        [P::num("length", 30.0, 12.0, 80.0).unit("m").describe("Length (m)"), P::num("width", 18.0, 8.0, 40.0).unit("m").describe("Width (m)"), P::num("height", 7.0, 4.0, 14.0).unit("m").describe("Eaves height (m)"), P::color("color", [0.5, 0.53, 0.55]).describe("Cladding colour")],
        |v: &Values, _s: u32| {
            let (l, w, h) = (v.f("length"), v.f("width"), v.f("height"));
            let mut walls = sh::block([l, h, w], [0.0; 3]);
            // Corrugated cladding: a rib every 0.6 m round the walls.
            let ribs = (l / 0.6) as u32;
            for i in 0..=ribs {
                let x = (i as f32 / ribs as f32 - 0.5) * l;
                for z in [-w * 0.5, w * 0.5] {
                    geo::merge(&mut walls, &b::cbox([0.08, h, 0.06], [x, h * 0.5, z]));
                }
            }
            let roof = b::gable(l, w, 1.6, h, 0.3);
            let mut doors = geo::Mesh::default();
            for k in 0..3 {
                let x = (k as f32 - 1.0) * l * 0.28;
                for j in 0..12 {
                    geo::merge(&mut doors, &b::cbox([4.4, 0.36, 0.1], [x, 0.2 + j as f32 * 0.4, w * 0.5 + 0.06]));
                }
            }
            Output::Parts(vec![Part::new(walls, v.color("color")), Part::new(roof, [0.3, 0.32, 0.33]), Part::new(doors, [0.62, 0.6, 0.5])])
        });

    basic!(_r, "buildings-workshops", "buildings", "workshops", "Workshops", "stone",
        [P::num("bays", 4.0, 1.0, 12.0).step(1.0).describe("Roof bays"), P::num("depth", 12.0, 6.0, 30.0).unit("m").describe("Depth (m)"), P::color("color", BRICK).describe("Brick colour")],
        |v: &Values, _s: u32| {
            let (n, d) = (v.f("bays").round() as u32, v.f("depth"));
            let (bay, h) = (5.0f32, 4.0f32);
            let l = n as f32 * bay;
            let walls = sh::block([l, h, d], [0.0; 3]);
            let (roof, mut lights) = sawtooth(-l * 0.5, n, bay, d, h);
            geo::merge(&mut lights, &b::windows(l, d, 0.0, 1, h, n * 2, [1.6, 1.8]));
            let door = b::cbox([3.0, 3.2, 0.1], [l * 0.5 - 2.5, 1.6, d * 0.5 + 0.05]);
            Output::Parts(vec![Part::new(walls, v.color("color")), Part::new(roof, SLATE), glass(lights), Part::new(door, [0.18, 0.28, 0.32])])
        });

    basic!(_r, "buildings-shops", "buildings", "shops", "Shops", "stone",
        [P::num("width", 8.0, 4.0, 20.0).unit("m").describe("Frontage (m)"), P::num("floors", 2.0, 1.0, 5.0).step(1.0).describe("Floors"), P::color("color", [0.7, 0.66, 0.58]).describe("Wall colour"), P::color("awning", [0.12, 0.3, 0.2]).describe("Awning colour")],
        |v: &Values, _s: u32| {
            let (w, f) = (v.f("width"), v.f("floors").round() as u32);
            let (d, fh) = (10.0, 3.2);
            let h = f as f32 * fh;
            let mut walls = sh::block([w, h, d], [0.0; 3]);
            geo::merge(&mut walls, &sh::block([w + 0.3, 0.5, d + 0.3], [0.0, h, 0.0]));
            let z = d * 0.5;
            let mut win = b::cbox([w * 0.62, 2.2, 0.08], [-w * 0.12, 1.5, z + 0.02]);
            geo::merge(&mut win, &b::cbox([1.1, 2.4, 0.08], [w * 0.36, 1.2, z + 0.02]));
            if f > 1 {
                geo::merge(&mut win, &b::windows(w, d, fh, f - 1, fh, 3, [1.0, 1.5]));
            }
            let sign = b::cbox([w * 0.9, 0.6, 0.12], [0.0, 2.95, z + 0.06]);
            // A sloping awning in stripes.
            let (mut aw, mut stripes) = (geo::Mesh::default(), geo::Mesh::default());
            let k = (w * 0.9 / 0.5) as u32;
            for i in 0..k {
                let x = (i as f32 + 0.5) / k as f32 * w * 0.9 - w * 0.45;
                let mut slat = b::cbox([w * 0.9 / k as f32, 0.04, 1.6], [0.0; 3]);
                sh::rotate(&mut slat, [1.0, 0.0, 0.0], 0.35);
                let slat = moved(slat, [x, 2.4, z + 0.75]);
                geo::merge(if i % 2 == 0 { &mut aw } else { &mut stripes }, &slat);
            }
            Output::Parts(vec![Part::new(walls, v.color("color")), glass(win), Part::new(sign, [0.08, 0.08, 0.1]), Part::new(aw, v.color("awning")).material("ground"), Part::new(stripes, TRIM).material("ground")])
        });

    // The office building (Asset Studio column 13): its own file,
    // crate::office (stone-clad wings, a glass tower, real interiors).
    #[cfg(feature = "buildings-office-buildings")]
    _r.add(Box::new(evani_procedural::FnAsset {
        info: evani_procedural::AssetInfo::cpu("buildings", "office-buildings", "Office buildings").material("stone"),
        params: &crate::office::PARAMS,
        make: crate::office::generate,
    }));

    basic!(_r, "buildings-apartment-buildings", "buildings", "apartment-buildings", "Apartment buildings", "stone",
        [P::num("floors", 6.0, 2.0, 20.0).step(1.0).describe("Floors"), P::num("width", 24.0, 8.0, 60.0).unit("m").describe("Width (m)"), P::color("color", [0.72, 0.62, 0.5]).describe("Wall colour")],
        |v: &Values, _s: u32| {
            let (f, w) = (v.f("floors").round() as u32, v.f("width"));
            let (d, fh) = (12.0, 3.0);
            let h = f as f32 * fh;
            let mut walls = sh::block([w, h, d], [0.0; 3]);
            geo::merge(&mut walls, &sh::block([w + 0.4, 0.6, d + 0.4], [0.0, h, 0.0]));
            let cols = (w / 4.0).round().max(1.0) as u32;
            let win = b::windows(w, d, 0.0, f, fh, cols, [1.4, 1.6]);
            // A balcony on every front bay above the ground floor.
            let mut balc = geo::Mesh::default();
            for i in 1..f {
                for k in 0..cols {
                    let x = ((k as f32 + 0.5) / cols as f32 - 0.5) * w;
                    let y = i as f32 * fh;
                    geo::merge(&mut balc, &sh::block([2.4, 0.15, 1.3], [x, y, d * 0.5 + 0.65]));
                    geo::merge(&mut balc, &sh::block([2.4, 0.9, 0.06], [x, y + 0.15, d * 0.5 + 1.27]));
                    for sx in [-1.17f32, 1.17] {
                        geo::merge(&mut balc, &sh::block([0.06, 0.9, 1.3], [x + sx, y + 0.15, d * 0.5 + 0.65]));
                    }
                }
            }
            Output::Parts(vec![Part::new(walls, v.color("color")), glass(win), Part::new(balc, [0.85, 0.85, 0.83])])
        });

    basic!(_r, "buildings-high-rise-towers", "buildings", "high-rise-towers", "High-rise towers", "glass",
        [P::num("height", 120.0, 40.0, 400.0).unit("m").describe("Height (m)"), P::num("width", 24.0, 10.0, 60.0).unit("m").describe("Base width (m)"), P::num("tiers", 3.0, 1.0, 6.0).step(1.0).describe("Setback tiers"), P::color("color", [0.12, 0.16, 0.2]).describe("Glass colour")],
        |v: &Values, _s: u32| {
            let (h, w, n) = (v.f("height"), v.f("width"), v.f("tiers").round() as u32);
            let (mut glass_m, mut frame) = (geo::Mesh::default(), geo::Mesh::default());
            let mut y = 0.0;
            for t in 0..n {
                let tw = w * (1.0 - 0.18 * t as f32);
                let th = h / n as f32 * (1.0 - 0.1 * t as f32) + if t + 1 == n { h * 0.1 * (n as f32 - 1.0) * 0.5 } else { 0.0 };
                geo::merge(&mut glass_m, &sh::block([tw, th, tw], [0.0, y, 0.0]));
                // Floor bands every 4 m and corner fins.
                let floors = (th / 4.0) as u32;
                for k in 0..=floors {
                    geo::merge(&mut frame, &sh::block([tw + 0.2, 0.35, tw + 0.2], [0.0, y + k as f32 * th / floors.max(1) as f32 - 0.175, 0.0]));
                }
                for (sx, sz) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                    geo::merge(&mut frame, &sh::block([0.6, th, 0.6], [sx * tw * 0.5, y, sz * tw * 0.5]));
                }
                y += th;
            }
            geo::merge(&mut frame, &b::rod([0.0, y, 0.0], [0.0, y + h * 0.12, 0.0], 0.4, 8));
            Output::Parts(vec![Part::new(glass_m, v.color("color")).material("glass"), Part::new(frame, [0.6, 0.62, 0.64]).material("stone")])
        });

    basic!(_r, "buildings-aircraft-hangars", "buildings", "aircraft-hangars", "Aircraft hangars", "ground",
        [P::num("span", 40.0, 15.0, 100.0).unit("m").describe("Span (m)"), P::num("length", 50.0, 15.0, 120.0).unit("m").describe("Length (m)"), P::color("color", [0.52, 0.55, 0.56]).describe("Shell colour")],
        |v: &Values, _s: u32| {
            let (s, l) = (v.f("span"), v.f("length"));
            let hgt = s * 0.36;
            let mut arch = vec![[-s * 0.5, 0.0], [s * 0.5, 0.0]];
            for i in 1..24 {
                let a = sh::PI * i as f32 / 24.0;
                arch.push([s * 0.5 * a.cos(), hgt * a.sin()]);
            }
            let shell = b::extrude(&arch, l);
            // Ribs over the vault and the big front doors.
            let mut ribs = geo::Mesh::default();
            for k in 0..=8 {
                let z = (k as f32 / 8.0 - 0.5) * l;
                let path: Vec<[f32; 3]> = (0..=24).map(|i| {
                    let a = sh::PI * i as f32 / 24.0;
                    [s * 0.5 * a.cos() * 1.005, hgt * a.sin() * 1.005, z]
                }).collect();
                geo::merge(&mut ribs, &b::sweep(&path, &b::rect(0.4, 0.3), true));
            }
            let mut doors = geo::Mesh::default();
            let (dw, dh) = (s * 0.8, hgt * 0.7);
            for i in 0..6 {
                let x = ((i as f32 + 0.5) / 6.0 - 0.5) * dw;
                geo::merge(&mut doors, &b::cbox([dw / 6.0 - 0.15, dh, 0.2], [x, dh * 0.5, l * 0.5 + 0.1]));
            }
            Output::Parts(vec![Part::new(shell, v.color("color")), Part::new(ribs, [0.4, 0.42, 0.43]), Part::new(doors, [0.32, 0.36, 0.38])])
        });

    basic!(_r, "buildings-factories", "buildings", "factories", "Factories", "stone",
        [P::num("length", 30.0, 15.0, 80.0).unit("m").describe("Shed length (m)"), P::num("chimney", 25.0, 0.0, 60.0).unit("m").describe("Chimney height (m)"), P::color("color", BRICK).describe("Brick colour")],
        |v: &Values, _s: u32| {
            let (l, c) = (v.f("length"), v.f("chimney"));
            let (d, h) = (20.0, 7.0);
            let n = (l / 6.0).round().max(1.0) as u32;
            let bay = l / n as f32;
            let mut walls = sh::block([l, h, d], [0.0; 3]);
            geo::merge(&mut walls, &sh::block([10.0, 9.0, 8.0], [l * 0.5 + 5.0, 0.0, d * 0.5 - 4.0]));
            if c > 1.0 {
                let base = sh::block([4.0, 3.0, 4.0], [0.0; 3]);
                geo::merge(&mut walls, &moved(base, [-l * 0.5 - 4.0, 0.0, -d * 0.5 + 3.0]));
                let stack = moved(sh::frustum(1.6, 1.0, c, 16), [-l * 0.5 - 4.0, 0.0, -d * 0.5 + 3.0]);
                geo::merge(&mut walls, &stack);
            }
            let (roof, mut lights) = sawtooth(-l * 0.5, n, bay, d, h);
            geo::merge(&mut lights, &moved(b::windows(10.0, 8.0, 0.0, 3, 3.0, 4, [1.2, 1.4]), [l * 0.5 + 5.0, 0.0, d * 0.5 - 4.0]));
            let door = b::cbox([5.0, 5.0, 0.1], [-l * 0.2, 2.5, d * 0.5 + 0.05]);
            Output::Parts(vec![Part::new(walls, v.color("color")), Part::new(roof, SLATE), glass(lights), Part::new(door, [0.25, 0.3, 0.32])])
        });

    basic!(_r, "buildings-churches", "buildings", "churches", "Churches", "stone",
        [P::num("length", 18.0, 10.0, 50.0).unit("m").describe("Nave length (m)"), P::num("tower", 22.0, 10.0, 60.0).unit("m").describe("Spire top height (m)"), P::color("color", [0.6, 0.57, 0.5]).describe("Stone colour")],
        |v: &Values, _s: u32| {
            let (l, t) = (v.f("length"), v.f("tower"));
            let (w, h) = (9.0, 7.0);
            let mut walls = sh::block([w, h, l], [0.0; 3]);
            let mut roof = gable_z(w, l, w * 0.7, h, 0.4);
            // The tower at the front, its spire a tall pyramid.
            let tw = 4.5;
            let th = (t * 0.55).max(h + 3.0);
            let tz = l * 0.5 + tw * 0.5 - 0.3;
            geo::merge(&mut walls, &sh::block([tw, th, tw], [0.0, 0.0, tz]));
            geo::merge(&mut roof, &moved(b::hip(tw, tw, (t - th).max(2.0), th, 0.2), [0.0, 0.0, tz]));
            // Tall arched windows down the nave and a pointed door.
            let arch = |w: f32, hh: f32| {
                let mut p = vec![[-w * 0.5, 0.0], [w * 0.5, 0.0]];
                for i in 0..=8 {
                    let a = sh::PI * i as f32 / 8.0;
                    p.push([w * 0.5 * a.cos(), hh + w * 0.5 * a.sin()]);
                }
                p.dedup();
                p
            };
            let mut win = geo::Mesh::default();
            let n = (l / 4.0) as u32;
            for i in 0..n {
                let z = ((i as f32 + 0.5) / n as f32 - 0.5) * l;
                for x in [-w * 0.5, w * 0.5] {
                    let mut a = b::extrude(&arch(1.2, 2.6), 0.1);
                    sh::rotate(&mut a, [0.0, 1.0, 0.0], sh::PI * 0.5);
                    geo::merge(&mut win, &moved(a, [x, 2.0, z]));
                }
            }
            geo::merge(&mut win, &moved(b::extrude(&arch(1.2, th * 0.55), 0.1), [0.0, th * 0.2, tz + tw * 0.5]));
            let door = moved(b::extrude(&arch(1.8, 2.2), 0.12), [0.0, 0.0, tz + tw * 0.5]);
            Output::Parts(vec![Part::new(walls, v.color("color")), Part::new(roof, SLATE), glass(win), Part::new(door, DOOR)])
        });

    basic!(_r, "buildings-schools", "buildings", "schools", "Schools", "stone",
        [P::num("length", 36.0, 16.0, 80.0).unit("m").describe("Main wing length (m)"), P::num("floors", 2.0, 1.0, 4.0).step(1.0).describe("Floors"), P::color("color", [0.55, 0.3, 0.2]).describe("Brick colour")],
        |v: &Values, _s: u32| {
            let (l, f) = (v.f("length"), v.f("floors").round() as u32);
            let (d, fh) = (12.0, 3.6);
            let h = f as f32 * fh;
            // An L: the main wing along x, a side wing back along -z.
            let plan = [[-l * 0.5, -d * 0.5], [l * 0.5, -d * 0.5], [l * 0.5, d * 0.5], [-l * 0.5, d * 0.5]];
            let mut walls = b::prism(&plan, 0.0, h);
            geo::merge(&mut walls, &sh::block([d, h, l * 0.6], [-l * 0.5 + d * 0.5, 0.0, -d * 0.5 - l * 0.3]));
            let mut trim = sh::block([l + 0.4, 0.5, d + 0.4], [0.0, h, 0.0]);
            geo::merge(&mut trim, &sh::block([d + 0.4, 0.5, l * 0.6], [-l * 0.5 + d * 0.5, h, -d * 0.5 - l * 0.3]));
            // Long window bands, floor by floor.
            let mut win = geo::Mesh::default();
            for i in 0..f {
                let y = i as f32 * fh + 1.0;
                for z in [-d * 0.5, d * 0.5] {
                    geo::merge(&mut win, &b::cbox([l - 3.0, 1.6, 0.08], [0.0, y + 0.8, z]));
                }
                geo::merge(&mut win, &b::cbox([0.08, 1.6, l * 0.5], [-l * 0.5 + d, y + 0.8, -d * 0.5 - l * 0.3]));
            }
            // An entrance canopy on posts and a flagpole.
            geo::merge(&mut trim, &sh::block([6.0, 0.3, 3.0], [l * 0.25, 3.2, d * 0.5 + 1.5]));
            for x in [l * 0.25 - 2.8, l * 0.25 + 2.8] {
                geo::merge(&mut trim, &b::post(x, d * 0.5 + 2.8, 0.2, 3.2));
            }
            geo::merge(&mut trim, &b::rod([l * 0.5 + 3.0, 0.0, d * 0.5 + 3.0], [l * 0.5 + 3.0, h + 5.0, d * 0.5 + 3.0], 0.08, 8));
            Output::Parts(vec![Part::new(walls, v.color("color")), Part::new(trim, [0.75, 0.74, 0.7]), glass(win)])
        });

    basic!(_r, "buildings-castles", "buildings", "castles", "Castles", "stone",
        [P::num("size", 30.0, 15.0, 80.0).unit("m").describe("Side of the walls (m)"), P::num("wall", 8.0, 4.0, 20.0).unit("m").describe("Wall height (m)"), P::color("color", [0.5, 0.48, 0.44]).describe("Stone colour")],
        |v: &Values, _s: u32| {
            let (s, h) = (v.f("size"), v.f("wall"));
            let hs = s * 0.5;
            let t = 2.0;
            let mut stone = geo::Mesh::default();
            let corners = [[-hs, -hs], [hs, -hs], [hs, hs], [-hs, hs]];
            for i in 0..4 {
                let (a, c) = (corners[i], corners[(i + 1) % 4]);
                let mid = [(a[0] + c[0]) * 0.5, (a[1] + c[1]) * 0.5];
                let size = if a[0] == c[0] { [t, h, s] } else { [s, h, t] };
                geo::merge(&mut stone, &sh::block(size, [mid[0], 0.0, mid[1]]));
                // The parapet's merlons, on the wall's outer edge.
                let out = |p: [f32; 2]| [p[0] * (1.0 + t * 0.4 / hs), p[1] * (1.0 + t * 0.4 / hs)];
                geo::merge(&mut stone, &merlons(out(a), out(c), h, 0.6));
            }
            // Round corner towers with conical roofs, a keep in the middle.
            let mut roofs = geo::Mesh::default();
            let (tr, th) = (s * 0.1 + 1.0, h + 4.0);
            for c in corners {
                geo::merge(&mut stone, &moved(sh::frustum(tr * 1.08, tr, th, 20), [c[0], 0.0, c[1]]));
                geo::merge(&mut roofs, &moved(sh::frustum(tr * 1.2, 0.0, tr * 1.6, 20), [c[0], th, c[1]]));
            }
            let k = s * 0.32;
            geo::merge(&mut stone, &sh::block([k, h * 2.2, k], [0.0; 3]));
            for (a, c) in [([-k * 0.5, -k * 0.5], [k * 0.5, -k * 0.5]), ([k * 0.5, -k * 0.5], [k * 0.5, k * 0.5]), ([k * 0.5, k * 0.5], [-k * 0.5, k * 0.5]), ([-k * 0.5, k * 0.5], [-k * 0.5, -k * 0.5])] {
                geo::merge(&mut stone, &merlons(a, c, h * 2.2, 0.6));
            }
            let gate = b::extrude(&[[-2.0, 0.0], [2.0, 0.0], [2.0, 3.5], [0.0, 5.0], [-2.0, 3.5]], 0.2);
            let gate = moved(gate, [0.0, 0.0, hs + t * 0.5 + 0.05]);
            Output::Parts(vec![Part::new(stone, v.color("color")), Part::new(roofs, SLATE), Part::new(gate, DOOR)])
        });
}
