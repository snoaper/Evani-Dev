//! Category 11, Interior furniture: basic generators (batch B). Carcasses,
//! tops, legs, drawers and doors from boxes, cushions from bevelled boxes,
//! turned seats and rails from a lathe; tables and chairs are shared by the
//! dining set and the island. The front faces +z. Inspired by three.js
//! (MIT) BoxGeometry, RoundedBoxGeometry and LatheGeometry; written fresh
//! here.

use evani_procedural::Registry;

#[allow(unused_imports, dead_code)]
pub fn register(_r: &mut Registry) {
    use evani_procedural::{Output, ParamSpec as P, Part, Values, build as b, geo, shapes as sh};
    const WOOD: [f32; 3] = [0.34, 0.19, 0.09];
    const HANDLE: [f32; 3] = [0.3, 0.3, 0.3];
    const FABRIC: [f32; 3] = [0.25, 0.32, 0.4];
    const LINEN: [f32; 3] = [0.8, 0.79, 0.75];

    fn moved(mut m: geo::Mesh, d: [f32; 3]) -> geo::Mesh {
        geo::transform(&mut m, [1.0; 3], d);
        m
    }

    fn turned(m: geo::Mesh, yaw: f32) -> geo::Mesh {
        sh::place(&m, 1.0, yaw, 0.0, [0.0; 3])
    }

    /// Four legs `w` square, `h` tall, at the corners of a `x` by `z` frame.
    fn legs(x: f32, z: f32, w: f32, h: f32) -> geo::Mesh {
        let mut m = geo::Mesh::default();
        for (sx, sz) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            geo::merge(&mut m, &b::post(sx * (x * 0.5 - w * 0.5), sz * (z * 0.5 - w * 0.5), w, h));
        }
        m
    }

    /// A table `w` by `d`, `h` high: a top, an apron and four legs.
    fn table(w: f32, d: f32, h: f32) -> geo::Mesh {
        let t = 0.035;
        let mut m = sh::block([w, t, d], [0.0, h - t, 0.0]);
        geo::merge(&mut m, &sh::block([w - 0.14, 0.09, d - 0.14], [0.0, h - t - 0.09, 0.0]));
        geo::merge(&mut m, &legs(w - 0.08, d - 0.08, 0.06, h - t));
        m
    }

    /// A chair facing +z: seat at `seat` m, back up to `back` m.
    fn chair(seat: f32, back: f32) -> geo::Mesh {
        let (w, d) = (0.44, 0.42);
        let mut m = sh::block([w, 0.04, d], [0.0, seat - 0.04, 0.0]);
        geo::merge(&mut m, &legs(w, d, 0.04, seat - 0.04));
        // Back posts rising from the back legs, three slats between them.
        for x in [-w * 0.5 + 0.02, w * 0.5 - 0.02] {
            geo::merge(&mut m, &b::bar([x, seat, -d * 0.5 + 0.02], [x, back, -d * 0.5 - 0.02], 0.04, 0.04));
        }
        for i in 0..3 {
            let y = seat + (back - seat) * (0.4 + i as f32 * 0.22);
            geo::merge(&mut m, &b::cbox([w - 0.04, 0.06, 0.02], [0.0, y, -d * 0.5 - 0.01]));
        }
        m
    }

    /// A row of drawer fronts on the +z face of a carcass `w` wide at `z`,
    /// from `y0` to `y1`, `n` drawers, each with a handle.
    fn drawers(w: f32, z: f32, y0: f32, y1: f32, n: u32) -> (geo::Mesh, geo::Mesh) {
        let (mut fronts, mut handles) = (geo::Mesh::default(), geo::Mesh::default());
        let h = (y1 - y0) / n.max(1) as f32;
        for i in 0..n {
            let y = y0 + (i as f32 + 0.5) * h;
            geo::merge(&mut fronts, &b::cbox([w - 0.03, h - 0.02, 0.02], [0.0, y, z + 0.01]));
            geo::merge(&mut handles, &b::bar([-0.06, y + h * 0.15, z + 0.035], [0.06, y + h * 0.15, z + 0.035], 0.012, 0.012));
        }
        (fronts, handles)
    }

    basic!(_r, "furniture-tables", "furniture", "tables", "Tables", "ground",
        [P::num("width", 1.4, 0.4, 3.5).unit("m").describe("Width (m)"), P::num("depth", 0.8, 0.4, 1.6).unit("m").describe("Depth (m)"), P::num("height", 0.75, 0.4, 1.1).unit("m").describe("Height (m)"), P::color("color", WOOD).describe("Wood colour")],
        |v: &Values, _s: u32| Output::Mesh(table(v.f("width"), v.f("depth"), v.f("height"))));

    basic!(_r, "furniture-chairs", "furniture", "chairs", "Chairs", "ground",
        [P::num("seat", 0.45, 0.3, 0.7).unit("m").describe("Seat height (m)"), P::num("back", 0.95, 0.6, 1.4).unit("m").describe("Top of the back (m)"), P::color("color", WOOD).describe("Wood colour")],
        |v: &Values, _s: u32| Output::Mesh(chair(v.f("seat"), v.f("back").max(v.f("seat") + 0.2))));

    basic!(_r, "furniture-stools", "furniture", "stools", "Stools", "ground",
        [P::num("height", 0.65, 0.3, 1.0).unit("m").describe("Seat height (m)"), P::num("legs", 3.0, 3.0, 4.0).step(1.0).describe("Legs"), P::color("color", WOOD).describe("Wood colour")],
        |v: &Values, _s: u32| {
            let (h, n) = (v.f("height"), v.f("legs").round() as u32);
            let r = 0.17;
            let mut m = b::revolve(&[[0.0, h - 0.04], [r, h - 0.04], [r + 0.01, h - 0.01], [r - 0.01, h + 0.005], [0.0, h + 0.012]], 24);
            // Splayed legs and a rung ring.
            for i in 0..n {
                let a = i as f32 / n as f32 * sh::TAU + 0.3;
                let (c, s) = (a.cos(), a.sin());
                geo::merge(&mut m, &b::rod([c * r * 1.3, 0.0, s * r * 1.3], [c * r * 0.6, h - 0.04, s * r * 0.6], 0.016, 8));
            }
            let rr = r * (1.3 - 0.7 * 0.35) - 0.005;
            let ring: Vec<[f32; 2]> = (0..=12).map(|i| {
                let a = -sh::PI * 0.5 + i as f32 / 12.0 * sh::TAU;
                [rr + 0.01 * a.cos(), h * 0.35 + 0.01 * a.sin()]
            }).collect();
            geo::merge(&mut m, &b::revolve(&ring, 24));
            Output::Mesh(m)
        });

    basic!(_r, "furniture-sofas", "furniture", "sofas", "Sofas", "ground",
        [P::num("seats", 3.0, 1.0, 5.0).step(1.0).describe("Seats"), P::num("depth", 0.9, 0.7, 1.2).unit("m").describe("Depth (m)"), P::color("color", FABRIC).describe("Upholstery colour")],
        |v: &Values, s: u32| {
            let (n, d) = (v.f("seats").round() as u32, v.f("depth"));
            let sw = 0.62;
            let w = n as f32 * sw + 0.4;
            let mut body = b::rounded_box([w, 0.28, d], 0.04, 3, [0.0, 0.1, 0.0]);
            for x in [-w * 0.5 + 0.1, w * 0.5 - 0.1] {
                geo::merge(&mut body, &b::rounded_box([0.2, 0.38, d], 0.06, 3, [x, 0.38, 0.0]));
            }
            geo::merge(&mut body, &b::rounded_box([w - 0.4, 0.42, 0.2], 0.06, 3, [0.0, 0.38, -d * 0.5 + 0.1]));
            for i in 0..n {
                let x = (i as f32 + 0.5 - n as f32 * 0.5) * sw;
                let squash = sh::range(s, i, 1, 0.0, 0.015);
                geo::merge(&mut body, &b::rounded_box([sw - 0.02, 0.14 - squash, d - 0.24], 0.05, 3, [x, 0.38, 0.1]));
                geo::merge(&mut body, &b::rounded_box([sw - 0.04, 0.42, 0.16], 0.07, 3, [x, 0.52, -d * 0.5 + 0.26]));
            }
            let feet = legs(w - 0.1, d - 0.1, 0.05, 0.1);
            Output::Parts(vec![Part::new(body, v.color("color")), Part::new(feet, [0.12, 0.08, 0.05]).material("ground")])
        });

    basic!(_r, "furniture-benches", "furniture", "benches", "Benches", "ground",
        [P::num("length", 1.8, 0.8, 4.0).unit("m").describe("Length (m)"), P::num("slats", 4.0, 2.0, 8.0).step(1.0).describe("Seat slats"), P::color("color", [0.42, 0.27, 0.15]).describe("Slat colour"), P::color("frame", [0.08, 0.09, 0.08]).describe("Frame colour")],
        |v: &Values, _s: u32| {
            let (l, n) = (v.f("length"), v.f("slats").round() as u32);
            let mut slats = geo::Mesh::default();
            for i in 0..n {
                let z = 0.22 - i as f32 * 0.44 / (n - 1).max(1) as f32;
                geo::merge(&mut slats, &b::cbox([l, 0.03, 0.44 / n as f32 - 0.015], [0.0, 0.44, z]));
            }
            for i in 0..3 {
                let y = 0.58 + i as f32 * 0.13;
                geo::merge(&mut slats, &b::cbox([l, 0.09, 0.025], [0.0, y, -0.29 - i as f32 * 0.03]));
            }
            // Cast end frames: legs, a seat bearer and a raked back post.
            let mut frame = geo::Mesh::default();
            for x in [-l * 0.5 + 0.15, l * 0.5 - 0.15] {
                geo::merge(&mut frame, &b::bar([x, 0.0, 0.22], [x, 0.42, 0.18], 0.05, 0.05));
                geo::merge(&mut frame, &b::bar([x, 0.0, -0.22], [x, 0.42, -0.22], 0.05, 0.05));
                geo::merge(&mut frame, &b::bar([x, 0.41, 0.26], [x, 0.41, -0.26], 0.05, 0.05));
                geo::merge(&mut frame, &b::bar([x, 0.42, -0.24], [x, 0.9, -0.36], 0.05, 0.05));
            }
            Output::Parts(vec![Part::new(slats, v.color("color")), Part::new(frame, v.color("frame")).material("metal")])
        });

    basic!(_r, "furniture-beds", "furniture", "beds", "Beds", "ground",
        [P::num("width", 1.5, 0.9, 2.0).unit("m").describe("Width (m)"), P::num("length", 2.1, 1.8, 2.4).unit("m").describe("Length (m)"), P::color("color", WOOD).describe("Frame colour"), P::color("sheet", [0.5, 0.58, 0.65]).describe("Duvet colour")],
        |v: &Values, _s: u32| {
            let (w, l) = (v.f("width"), v.f("length"));
            let mut frame = sh::block([w + 0.08, 0.2, l], [0.0, 0.15, 0.0]);
            geo::merge(&mut frame, &legs(w + 0.08, l, 0.07, 0.15));
            geo::merge(&mut frame, &sh::block([w + 0.12, 1.0, 0.06], [0.0, 0.0, -l * 0.5 - 0.03]));
            geo::merge(&mut frame, &sh::block([w + 0.12, 0.55, 0.05], [0.0, 0.0, l * 0.5 + 0.025]));
            let mut linen = b::rounded_box([w - 0.02, 0.2, l - 0.04], 0.05, 4, [0.0, 0.35, 0.0]);
            for x in [-w * 0.24, w * 0.24] {
                geo::merge(&mut linen, &b::rounded_box([w * 0.42, 0.12, 0.38], 0.05, 4, [x, 0.55, -l * 0.5 + 0.28]));
            }
            if w < 1.2 {
                linen = b::rounded_box([w - 0.02, 0.2, l - 0.04], 0.05, 4, [0.0, 0.35, 0.0]);
                geo::merge(&mut linen, &b::rounded_box([w * 0.7, 0.12, 0.38], 0.05, 4, [0.0, 0.55, -l * 0.5 + 0.28]));
            }
            let duvet = b::rounded_box([w + 0.04, 0.1, l * 0.66], 0.04, 4, [0.0, 0.5, l * 0.16]);
            Output::Parts(vec![Part::new(frame, v.color("color")), Part::new(linen, LINEN).material("ground"), Part::new(duvet, v.color("sheet")).material("ground")])
        });

    basic!(_r, "furniture-cabinets", "furniture", "cabinets", "Cabinets", "ground",
        [P::num("width", 0.9, 0.4, 2.4).unit("m").describe("Width (m)"), P::num("height", 0.9, 0.4, 2.2).unit("m").describe("Height (m)"), P::color("color", [0.55, 0.5, 0.42]).describe("Cabinet colour")],
        |v: &Values, _s: u32| {
            let (w, h) = (v.f("width"), v.f("height"));
            let d = 0.45;
            let mut body = sh::block([w, h - 0.08, d], [0.0, 0.08, 0.0]);
            geo::merge(&mut body, &sh::block([w - 0.06, 0.08, d - 0.05], [0.0, 0.0, -0.02]));
            geo::merge(&mut body, &sh::block([w + 0.03, 0.03, d + 0.03], [0.0, h, 0.0]));
            let mut handles = geo::Mesh::default();
            // Two doors, each with a raised panel and a handle.
            for (k, x) in [(-1.0f32, -w * 0.25), (1.0, w * 0.25)] {
                geo::merge(&mut body, &b::cbox([w * 0.5 - 0.02, h - 0.12, 0.02], [x, 0.08 + (h - 0.08) * 0.5, d * 0.5 + 0.01]));
                geo::merge(&mut body, &b::cbox([w * 0.5 - 0.12, h - 0.25, 0.012], [x, 0.08 + (h - 0.08) * 0.5, d * 0.5 + 0.026]));
                let hx = x - k * (w * 0.25 - 0.05);
                geo::merge(&mut handles, &b::bar([hx, h * 0.55, d * 0.5 + 0.04], [hx, h * 0.55 + 0.12, d * 0.5 + 0.04], 0.012, 0.012));
            }
            Output::Parts(vec![Part::new(body, v.color("color")), Part::new(handles, HANDLE).material("metal")])
        });

    basic!(_r, "furniture-shelving-units", "furniture", "shelving-units", "Shelving units", "ground",
        [P::num("width", 1.0, 0.4, 3.0).unit("m").describe("Width (m)"), P::num("height", 1.8, 0.5, 2.6).unit("m").describe("Height (m)"), P::num("shelves", 5.0, 2.0, 10.0).step(1.0).describe("Shelves"), P::color("color", [0.6, 0.45, 0.3]).describe("Wood colour")],
        |v: &Values, _s: u32| {
            let (w, h, n) = (v.f("width"), v.f("height"), v.f("shelves").round() as u32);
            let (d, t) = (0.32, 0.022);
            let mut m = geo::Mesh::default();
            for x in [-w * 0.5 + t * 0.5, w * 0.5 - t * 0.5] {
                geo::merge(&mut m, &sh::block([t, h, d], [x, 0.0, 0.0]));
            }
            geo::merge(&mut m, &sh::block([w, h, 0.006], [0.0, 0.0, -d * 0.5 + 0.003]));
            for i in 0..n {
                let y = 0.06 + i as f32 * (h - 0.06 - t) / (n - 1).max(1) as f32;
                geo::merge(&mut m, &sh::block([w - 2.0 * t, t, d - 0.01], [0.0, y, 0.005]));
            }
            geo::merge(&mut m, &b::cbox([w - 2.0 * t, 0.06, 0.02], [0.0, 0.03, d * 0.5 - 0.02]));
            Output::Mesh(m)
        });

    basic!(_r, "furniture-wardrobes", "furniture", "wardrobes", "Wardrobes", "ground",
        [P::num("width", 1.2, 0.6, 2.4).unit("m").describe("Width (m)"), P::num("height", 2.0, 1.6, 2.5).unit("m").describe("Height (m)"), P::color("color", [0.42, 0.26, 0.14]).describe("Wood colour")],
        |v: &Values, _s: u32| {
            let (w, h) = (v.f("width"), v.f("height"));
            let d = 0.6;
            let mut body = sh::block([w, h - 0.1, d], [0.0, 0.1, 0.0]);
            geo::merge(&mut body, &sh::block([w - 0.06, 0.1, d - 0.06], [0.0, 0.0, 0.0]));
            // A stepped cornice, two doors with panels, a drawer below.
            geo::merge(&mut body, &sh::block([w + 0.06, 0.05, d + 0.06], [0.0, h, 0.0]));
            geo::merge(&mut body, &sh::block([w + 0.1, 0.04, d + 0.1], [0.0, h + 0.05, 0.0]));
            let mut handles = geo::Mesh::default();
            for (k, x) in [(-1.0f32, -w * 0.25), (1.0, w * 0.25)] {
                for (y0, y1) in [(0.4, h * 0.55), (h * 0.57, h - 0.05)] {
                    geo::merge(&mut body, &b::cbox([w * 0.5 - 0.12, y1 - y0 - 0.1, 0.014], [x, (y0 + y1) * 0.5, d * 0.5 + 0.007]));
                }
                let hx = x - k * (w * 0.25 - 0.06);
                geo::merge(&mut handles, &b::bar([hx, h * 0.5, d * 0.5 + 0.03], [hx, h * 0.5 + 0.18, d * 0.5 + 0.03], 0.014, 0.014));
            }
            let (fr, hd) = drawers(w, d * 0.5, 0.12, 0.38, 1);
            geo::merge(&mut body, &fr);
            geo::merge(&mut handles, &hd);
            Output::Parts(vec![Part::new(body, v.color("color")), Part::new(handles, HANDLE).material("metal")])
        });

    basic!(_r, "furniture-dressers", "furniture", "dressers", "Dressers", "ground",
        [P::num("width", 1.0, 0.5, 2.0).unit("m").describe("Width (m)"), P::num("drawers", 4.0, 2.0, 7.0).step(1.0).describe("Drawers"), P::color("color", [0.5, 0.3, 0.16]).describe("Wood colour")],
        |v: &Values, _s: u32| {
            let (w, n) = (v.f("width"), v.f("drawers").round() as u32);
            let (d, h) = (0.48, 0.18 * n as f32 + 0.18);
            let mut body = sh::block([w, h - 0.12, d], [0.0, 0.12, 0.0]);
            geo::merge(&mut body, &sh::block([w + 0.04, 0.03, d + 0.03], [0.0, h, 0.0]));
            geo::merge(&mut body, &legs(w - 0.02, d - 0.02, 0.05, 0.12));
            let (fr, hd) = drawers(w, d * 0.5, 0.14, h - 0.02, n);
            geo::merge(&mut body, &fr);
            Output::Parts(vec![Part::new(body, v.color("color")), Part::new(hd, [0.6, 0.48, 0.2]).material("metal")])
        });

    basic!(_r, "furniture-desks", "furniture", "desks", "Desks", "ground",
        [P::num("width", 1.4, 0.8, 2.4).unit("m").describe("Width (m)"), P::num("depth", 0.7, 0.5, 1.0).unit("m").describe("Depth (m)"), P::color("color", [0.52, 0.38, 0.24]).describe("Wood colour")],
        |v: &Values, _s: u32| {
            let (w, d) = (v.f("width"), v.f("depth"));
            let h = 0.75;
            let mut body = sh::block([w, 0.03, d], [0.0, h - 0.03, 0.0]);
            // A drawer pedestal on the right, two legs on the left, a modesty panel.
            let pw = 0.42;
            let px = w * 0.5 - pw * 0.5 - 0.02;
            geo::merge(&mut body, &sh::block([pw, h - 0.03, d - 0.06], [px, 0.0, 0.0]));
            for z in [-d * 0.5 + 0.05, d * 0.5 - 0.05] {
                geo::merge(&mut body, &b::post(-w * 0.5 + 0.05, z, 0.05, h - 0.03));
            }
            geo::merge(&mut body, &sh::block([w - pw - 0.1, 0.4, 0.02], [-pw * 0.5, h - 0.45, -d * 0.5 + 0.06]));
            let (fr, hd) = drawers(pw, (d - 0.06) * 0.5, 0.04, h - 0.06, 3);
            geo::merge(&mut body, &moved(fr, [px, 0.0, 0.0]));
            Output::Parts(vec![Part::new(body, v.color("color")), Part::new(moved(hd, [px, 0.0, 0.0]), HANDLE).material("metal")])
        });

    basic!(_r, "furniture-kitchen-counters", "furniture", "kitchen-counters", "Kitchen counters", "ground",
        [P::num("units", 4.0, 1.0, 8.0).step(1.0).describe("Base units (60 cm each)"), P::num("sink", 2.0, 0.0, 8.0).step(1.0).describe("Which unit has the sink (0: none)"), P::color("color", [0.75, 0.75, 0.72]).describe("Door colour"), P::color("top", [0.18, 0.18, 0.19]).describe("Worktop colour")],
        |v: &Values, _s: u32| {
            let (n, sink) = (v.f("units").round() as u32, v.f("sink").round() as u32);
            let (uw, d, h) = (0.6, 0.6, 0.9);
            let w = n as f32 * uw;
            let mut body = sh::block([w, h - 0.14, d - 0.02], [0.0, 0.1, -0.01]);
            let mut plinth = sh::block([w, 0.1, d - 0.08], [0.0, 0.0, -0.04]);
            let mut handles = geo::Mesh::default();
            for i in 0..n {
                let x = (i as f32 + 0.5) * uw - w * 0.5;
                geo::merge(&mut body, &b::cbox([uw - 0.006, h - 0.16, 0.018], [x, 0.1 + (h - 0.14) * 0.5, d * 0.5 - 0.01]));
                geo::merge(&mut handles, &b::bar([x - 0.1, h - 0.12, d * 0.5 + 0.02], [x + 0.1, h - 0.12, d * 0.5 + 0.02], 0.012, 0.012));
            }
            let top = sh::block([w + 0.02, 0.04, d + 0.02], [0.0, h - 0.04, 0.0]);
            if (1..=n).contains(&sink) {
                let x = (sink as f32 - 0.5) * uw - w * 0.5;
                // A steel bowl sunk into the top, and a swan-neck tap.
                geo::merge(&mut plinth, &sh::block([0.46, 0.006, 0.4], [x, h, 0.0]));
                geo::merge(&mut handles, &b::rod([x, h, -0.24], [x, h + 0.3, -0.24], 0.015, 8));
                geo::merge(&mut handles, &b::rod([x, h + 0.3, -0.24], [x, h + 0.3, -0.08], 0.012, 8));
            }
            Output::Parts(vec![Part::new(body, v.color("color")), Part::new(top, v.color("top")).material("stone"), Part::new(plinth, [0.2, 0.2, 0.2]), Part::new(handles, [0.5, 0.5, 0.52]).material("metal")])
        });

    basic!(_r, "furniture-dining-sets", "furniture", "dining-sets", "Dining sets", "ground",
        [P::num("chairs", 6.0, 2.0, 10.0).step(1.0).describe("Chairs round the table"), P::num("length", 1.8, 0.8, 3.5).unit("m").describe("Table length (m)"), P::color("color", WOOD).describe("Wood colour")],
        |v: &Values, s: u32| {
            let (n, l) = (v.f("chairs").round() as u32, v.f("length"));
            let d = 0.95;
            let mut m = table(l, d, 0.75);
            // Chairs down both sides (and one at each end when odd).
            let side = n / 2;
            let ends = n - side * 2;
            for k in 0..2u32 {
                let per = if k == 0 { side + ends % 2 } else { side };
                for i in 0..per {
                    let x = ((i as f32 + 0.5) / per as f32 - 0.5) * (l - 0.3);
                    let z = if k == 0 { d * 0.5 + 0.12 } else { -d * 0.5 - 0.12 };
                    let yaw = if k == 0 { sh::PI } else { 0.0 } + sh::range(s, i + k * 17, 1, -0.08, 0.08);
                    let off = sh::range(s, i + k * 17, 2, 0.0, 0.08) * if k == 0 { 1.0 } else { -1.0 };
                    geo::merge(&mut m, &moved(turned(chair(0.45, 0.95), yaw), [x, 0.0, z + off]));
                }
            }
            Output::Mesh(m)
        });

    basic!(_r, "furniture-kitchen-islands", "furniture", "kitchen-islands", "Kitchen islands", "ground",
        [P::num("length", 2.0, 1.0, 4.0).unit("m").describe("Length (m)"), P::num("stools", 3.0, 0.0, 6.0).step(1.0).describe("Stools along the overhang"), P::color("color", [0.18, 0.26, 0.3]).describe("Cabinet colour"), P::color("top", [0.82, 0.8, 0.76]).describe("Worktop colour")],
        |v: &Values, _s: u32| {
            let (l, n) = (v.f("length"), v.f("stools").round() as u32);
            let (d, h) = (0.9, 0.92);
            let mut body = sh::block([l, h - 0.04, d - 0.3], [0.0, 0.0, -0.15]);
            for i in 0..(l / 0.6).round() as u32 {
                let x = ((i as f32 + 0.5) / (l / 0.6).round() - 0.5) * l;
                geo::merge(&mut body, &b::cbox([0.59, h - 0.16, 0.018], [x, 0.1 + (h - 0.16) * 0.5, -0.459]));
            }
            let top = sh::block([l + 0.06, 0.04, d], [0.0, h - 0.04, 0.0]);
            let mut stools = geo::Mesh::default();
            for i in 0..n {
                let x = ((i as f32 + 0.5) / n as f32 - 0.5) * (l - 0.4);
                let st = b::revolve(&[[0.0, 0.62], [0.18, 0.62], [0.18, 0.66], [0.0, 0.66]], 20);
                geo::merge(&mut stools, &moved(st, [x, 0.0, d * 0.5 + 0.12]));
                geo::merge(&mut stools, &b::rod([x, 0.0, d * 0.5 + 0.12], [x, 0.62, d * 0.5 + 0.12], 0.025, 10));
                geo::merge(&mut stools, &moved(b::revolve(&[[0.0, 0.0], [0.2, 0.0], [0.2, 0.02], [0.0, 0.02]], 20), [x, 0.0, d * 0.5 + 0.12]));
            }
            Output::Parts(vec![Part::new(body, v.color("color")), Part::new(top, v.color("top")).material("stone"), Part::new(stools, [0.12, 0.12, 0.12]).material("ground")])
        });

    basic!(_r, "furniture-bunk-beds", "furniture", "bunk-beds", "Bunk beds", "ground",
        [P::num("length", 2.0, 1.8, 2.3).unit("m").describe("Length (m)"), P::num("height", 1.7, 1.4, 2.0).unit("m").describe("Post height (m)"), P::color("color", [0.55, 0.42, 0.28]).describe("Wood colour")],
        |v: &Values, _s: u32| {
            let (l, h) = (v.f("length"), v.f("height"));
            let w = 1.0;
            let mut frame = legs(l + 0.1, w + 0.1, 0.07, h);
            let mut linen = geo::Mesh::default();
            for y in [0.3, h * 0.62] {
                geo::merge(&mut frame, &sh::block([l + 0.04, 0.12, w + 0.04], [0.0, y - 0.12, 0.0]));
                geo::merge(&mut linen, &b::rounded_box([l - 0.02, 0.16, w - 0.04], 0.04, 4, [0.0, y, 0.0]));
                geo::merge(&mut linen, &b::rounded_box([0.36, 0.1, w * 0.6], 0.04, 4, [-l * 0.5 + 0.25, y + 0.16, 0.0]));
            }
            // A guard rail on the top bunk and a ladder at the foot.
            let top = h * 0.62;
            geo::merge(&mut frame, &b::bar([-l * 0.5, top + 0.3, w * 0.5 + 0.03], [l * 0.2, top + 0.3, w * 0.5 + 0.03], 0.05, 0.04));
            geo::merge(&mut frame, &b::bar([l * 0.2, top, w * 0.5 + 0.03], [l * 0.2, top + 0.3, w * 0.5 + 0.03], 0.04, 0.04));
            let lx = l * 0.5 - 0.35;
            for x in [lx - 0.2, lx + 0.2] {
                geo::merge(&mut frame, &b::bar([x, 0.0, w * 0.5 + 0.12], [x, top + 0.3, w * 0.5 + 0.06], 0.04, 0.04));
            }
            let rungs = ((top + 0.2) / 0.3) as u32;
            for i in 1..=rungs {
                let y = i as f32 * 0.3;
                let z = w * 0.5 + 0.12 - 0.06 * y / (top + 0.3);
                geo::merge(&mut frame, &b::rod([lx - 0.2, y, z], [lx + 0.2, y, z], 0.016, 8));
            }
            Output::Parts(vec![Part::new(frame, v.color("color")), Part::new(linen, [0.55, 0.6, 0.66]).material("ground")])
        });
}
