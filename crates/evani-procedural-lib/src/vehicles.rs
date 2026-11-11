//! Category 15, Ground vehicles: basic generators (batch C). Bodies are
//! side silhouettes extruded across the width, glasshouses a narrower
//! extrusion in glass, wheels the base's tyre-and-rim, frames rods between
//! points, tracks a belt swept round a stadium loop. Front at +x. Inspired
//! by three.js (MIT) ExtrudeGeometry, CylinderGeometry and TubeGeometry,
//! and the low-poly car built from a side outline; written fresh here.

use evani_procedural::Registry;

#[allow(unused_imports, dead_code)]
pub fn register(_r: &mut Registry) {
    use evani_procedural::{Output, ParamSpec as P, Part, Values, build as b, craft as c, geo, shapes as sh};
    type Mesh = geo::Mesh;
    const RUBBER: [f32; 3] = [0.03, 0.03, 0.035];
    const RIM: [f32; 3] = [0.45, 0.46, 0.48];
    const DARK: [f32; 3] = [0.06, 0.06, 0.07];
    const GLASS: [f32; 3] = [0.12, 0.16, 0.2];
    const HEAD: [f32; 3] = [1.0, 0.92, 0.7];
    const TAIL: [f32; 3] = [0.8, 0.05, 0.03];

    fn moved(mut m: Mesh, d: [f32; 3]) -> Mesh {
        geo::transform(&mut m, [1.0; 3], d);
        m
    }

    /// Wheels at each x, both sides at ±`track`: (tyres, rims).
    fn wheelset(xs: &[f32], track: f32, r: f32, w: f32, spokes: u32) -> (Mesh, Mesh) {
        let (mut t, mut m) = (Mesh::default(), Mesh::default());
        for &x in xs {
            for z in [-track, track] {
                let (a, bb) = c::wheel([x, r, z], r, w, spokes);
                geo::merge(&mut t, &a);
                geo::merge(&mut m, &bb);
            }
        }
        (t, m)
    }

    /// Lamps on the nose (x = `front`) and tail (x = `back`) at height `y`,
    /// ±`z` across: (head lamps, tail lamps).
    fn lamps(front: f32, back: f32, y: f32, z: f32, size: [f32; 2]) -> (Mesh, Mesh) {
        let (mut h, mut t) = (Mesh::default(), Mesh::default());
        for s in [-z, z] {
            geo::merge(&mut h, &b::cbox([0.04, size[1], size[0]], [front, y, s]));
            geo::merge(&mut t, &b::cbox([0.04, size[1], size[0]], [back, y, s]));
        }
        (h, t)
    }

    /// A car-like body: `len` by `wid`, its floor at `low`, the nose top at
    /// `nose`, the belt line at `belt`, the tail top at `tail`, the
    /// glasshouse at x `g` = [front base, front top, rear top, rear base]
    /// up to `roof`; wheels of radius `r` at the two axle xs.
    struct Car {
        len: f32,
        wid: f32,
        low: f32,
        nose: f32,
        belt: f32,
        tail: f32,
        roof: f32,
        g: [f32; 4],
        r: f32,
        axles: [f32; 2],
    }

    fn car(k: &Car, paint: [f32; 3]) -> Vec<Part> {
        let (l, lo) = (k.len * 0.5, k.low);
        let body = [[-l, lo + 0.12], [-l + 0.12, lo], [l - 0.15, lo], [l, lo + 0.15], [l, k.nose - 0.08], [l - 0.2, k.nose], [k.g[0], k.belt], [k.g[3], k.belt], [-l, k.tail]];
        let body = b::extrude(&body, k.wid);
        let glass = b::extrude(&[[k.g[0], k.belt - 0.02], [k.g[1], k.roof], [k.g[2], k.roof], [k.g[3], k.belt - 0.02]], k.wid * 0.86);
        let roof = b::extrude(&[[k.g[1] + 0.02, k.roof - 0.03], [k.g[2] - 0.02, k.roof - 0.03], [k.g[2], k.roof + 0.02], [k.g[1], k.roof + 0.02]], k.wid * 0.88);
        let mut paint_m = body;
        geo::merge(&mut paint_m, &roof);
        let (tyres, rims) = wheelset(&k.axles, k.wid * 0.5 - k.r * 0.3, k.r, k.r * 0.65, 0);
        let (hl, tl) = lamps(l + 0.005, -l - 0.005, (lo + k.nose) * 0.5 + 0.05, k.wid * 0.33, [0.3, 0.1]);
        let mut trim = b::cbox([0.08, 0.1, k.wid + 0.02], [l, lo + 0.1, 0.0]);
        geo::merge(&mut trim, &b::cbox([0.08, 0.1, k.wid + 0.02], [-l, lo + 0.1, 0.0]));
        vec![
            Part::new(paint_m, paint).material("accent"),
            Part::new(glass, GLASS).material("glass"),
            Part::new(tyres, RUBBER).material("ground"),
            Part::new(rims, RIM).material("metal"),
            Part::new(trim, DARK).material("ground"),
            Part::new(hl, HEAD).material("glow"),
            Part::new(tl, TAIL).material("glow"),
        ]
    }

    basic!(_r, "vehicles-sedans", "vehicles", "sedans", "Sedans", "accent",
        [P::num("length", 4.6, 3.6, 6.0).unit("m").describe("Length (m)"), P::num("roof", 1.45, 1.2, 1.8).unit("m").describe("Roof height (m)"), P::num("wheel", 0.33, 0.26, 0.5).unit("m").describe("Wheel radius (m)"), P::color("color", [0.45, 0.04, 0.04]).describe("Paint colour")],
        |v: &Values, _s: u32| {
            let (l, h, r) = (v.f("length"), v.f("roof"), v.f("wheel"));
            let low = r * 0.75;
            Output::Parts(car(&Car { len: l, wid: 1.8, low, nose: low + 0.42, belt: low + 0.58, tail: low + 0.55, roof: h, g: [l * 0.2, l * 0.02, -l * 0.2, -l * 0.33], r, axles: [l * 0.32, -l * 0.3] }, v.color("color")))
        });

    basic!(_r, "vehicles-hatchbacks", "vehicles", "hatchbacks", "Hatchbacks", "accent",
        [P::num("length", 3.9, 3.2, 4.6).unit("m").describe("Length (m)"), P::num("roof", 1.48, 1.3, 1.75).unit("m").describe("Roof height (m)"), P::num("rake", 0.15, 0.0, 0.5).unit("m").describe("Rear window lean (m)"), P::color("color", [0.05, 0.2, 0.45]).describe("Paint colour")],
        |v: &Values, _s: u32| {
            let (l, h, rake) = (v.f("length"), v.f("roof"), v.f("rake"));
            let r = 0.3;
            let low = r * 0.75;
            Output::Parts(car(&Car { len: l, wid: 1.75, low, nose: low + 0.45, belt: low + 0.6, tail: low + 0.6, roof: h, g: [l * 0.22, l * 0.03, -l * 0.42 + rake, -l * 0.5 + 0.02], r, axles: [l * 0.32, -l * 0.32] }, v.color("color")))
        });

    basic!(_r, "vehicles-pickup-trucks", "vehicles", "pickup-trucks", "Pickup trucks", "accent",
        [P::num("bed", 1.8, 1.2, 2.6).unit("m").describe("Bed length (m)"), P::num("lift", 0.0, 0.0, 0.4).unit("m").describe("Suspension lift (m)"), P::color("color", [0.12, 0.13, 0.14]).describe("Paint colour")],
        |v: &Values, _s: u32| {
            let (bed, lift) = (v.f("bed"), v.f("lift"));
            let r = 0.4;
            let low = r * 0.8 + lift;
            let cab = 2.6;
            let l = cab + bed;
            let x0 = l * 0.5;
            let rear = x0 - cab;
            // The cab is a short car body; behind it the bed's floor and walls.
            let mut ps = car(&Car { len: l, wid: 1.95, low, nose: low + 0.62, belt: low + 0.72, tail: low + 0.3, roof: low + 1.25, g: [x0 - 1.25, x0 - 1.65, rear + 0.1, rear], r, axles: [x0 - 0.85, -l * 0.5 + bed * 0.45] }, v.color("color"));
            let mut walls = Mesh::default();
            for z in [-0.93, 0.93] {
                geo::merge(&mut walls, &b::cbox([bed, 0.42, 0.08], [-l * 0.5 + bed * 0.5, low + 0.51, z]));
            }
            geo::merge(&mut walls, &b::cbox([0.08, 0.42, 1.94], [-l * 0.5 + 0.04, low + 0.51, 0.0]));
            geo::merge(&mut walls, &b::cbox([0.08, 0.5, 1.94], [rear - 0.04, low + 0.55, 0.0]));
            ps.push(Part::new(walls, v.color("color")).material("accent"));
            Output::Parts(ps)
        });

    basic!(_r, "vehicles-heavy-trucks", "vehicles", "heavy-trucks", "Heavy trucks", "accent",
        [P::num("length", 7.0, 4.0, 12.0).unit("m").describe("Box length (m)"), P::num("axles", 2.0, 1.0, 4.0).step(1.0).describe("Rear axles"), P::color("color", [0.6, 0.06, 0.04]).describe("Cab colour"), P::color("box", [0.75, 0.74, 0.7]).describe("Box colour")],
        |v: &Values, _s: u32| {
            let (bl, n) = (v.f("length"), v.f("axles").round() as u32);
            let r = 0.5;
            let (w, frame_y) = (2.45, r + 0.25);
            // A cab-over cab, the chassis rails and a box body behind.
            let cab_x = bl * 0.5 + 1.2;
            let mut cab = b::rounded_box([2.2, 2.3, w], 0.12, 4, [cab_x, frame_y, 0.0]);
            geo::merge(&mut cab, &b::cbox([0.3, 0.5, w * 0.9], [cab_x + 1.15, frame_y + 0.25, 0.0]));
            let glass = b::cbox([0.06, 0.8, w * 0.85], [cab_x + 1.11, frame_y + 1.55, 0.0]);
            let boxm = b::rounded_box([bl, 2.6, w + 0.05], 0.05, 2, [0.0, frame_y + 0.3, 0.0]);
            let mut chassis = b::cbox([bl + 2.6, 0.3, 1.0], [0.6, frame_y + 0.15, 0.0]);
            let mut xs = vec![cab_x + 0.3];
            let pitch = 1.35f32.min((bl - 1.2) / n as f32);
            for i in 0..n {
                xs.push(-bl * 0.5 + 1.0 + i as f32 * pitch);
            }
            let (tyres, rims) = wheelset(&xs, w * 0.5 - 0.3, r, 0.38, 0);
            for &x in &xs[1..] {
                geo::merge(&mut chassis, &b::rod([x, r, -w * 0.4], [x, r, w * 0.4], 0.08, 8));
            }
            let (hl, tl) = lamps(cab_x + 1.31, -bl * 0.5 - 0.01, frame_y + 0.25, w * 0.38, [0.3, 0.15]);
            Output::Parts(vec![
                Part::new(cab, v.color("color")).material("accent"),
                Part::new(boxm, v.color("box")).material("ground"),
                Part::new(glass, GLASS).material("glass"),
                Part::new(chassis, DARK),
                Part::new(tyres, RUBBER).material("ground"),
                Part::new(rims, RIM).material("metal"),
                Part::new(hl, HEAD).material("glow"),
                Part::new(tl, TAIL).material("glow"),
            ])
        });

    basic!(_r, "vehicles-delivery-vans", "vehicles", "delivery-vans", "Delivery vans", "accent",
        [P::num("length", 5.4, 4.4, 7.0).unit("m").describe("Length (m)"), P::num("roof", 2.4, 1.9, 2.9).unit("m").describe("Roof height (m)"), P::color("color", [0.75, 0.74, 0.7]).describe("Paint colour")],
        |v: &Values, _s: u32| {
            let (l, h) = (v.f("length"), v.f("roof"));
            let r = 0.36;
            let low = r * 0.8;
            // A tall box with a short bonnet: the glass only at the front.
            let mut ps = car(&Car { len: l, wid: 2.0, low, nose: low + 0.62, belt: low + 0.95, tail: h - 0.05, roof: h, g: [l * 0.5 - 0.75, l * 0.5 - 1.4, l * 0.5 - 1.55, l * 0.5 - 1.6], r, axles: [l * 0.5 - 0.9, -l * 0.5 + 1.0] }, v.color("color"));
            let panel = b::cbox([l - 1.6, h - low - 0.45, 2.0], [-0.8, (low + 0.45 + h) * 0.5, 0.0]);
            ps.push(Part::new(panel, v.color("color")).material("accent"));
            Output::Parts(ps)
        });

    basic!(_r, "vehicles-buses", "vehicles", "buses", "Buses", "accent",
        [P::num("length", 12.0, 8.0, 18.0).unit("m").describe("Length (m)"), P::num("decks", 1.0, 1.0, 2.0).step(1.0).describe("Decks"), P::color("color", [0.7, 0.08, 0.05]).describe("Paint colour")],
        |v: &Values, _s: u32| {
            let (l, d) = (v.f("length"), v.f("decks").round() as u32);
            let (w, r, low) = (2.55, 0.5, 0.35);
            let deck = 1.75;
            let h = 0.9 + deck * d as f32;
            let body = b::rounded_box([l, h, w], 0.15, 3, [0.0, low, 0.0]);
            // A window band per deck along both sides, the windscreen, doors.
            let mut glass = Mesh::default();
            for k in 0..d {
                let y = low + 0.95 + k as f32 * deck + 0.55;
                for z in [-w * 0.5 - 0.01, w * 0.5 + 0.01] {
                    geo::merge(&mut glass, &b::cbox([l - 1.2, 0.95, 0.03], [-0.3, y, z]));
                }
            }
            geo::merge(&mut glass, &b::cbox([0.04, 1.3, w * 0.9], [l * 0.5 + 0.01, low + 1.55, 0.0]));
            let mut doors = Mesh::default();
            for x in [l * 0.5 - 0.9, 0.0] {
                geo::merge(&mut doors, &b::cbox([1.1, 1.9, 0.04], [x, low + 1.0, w * 0.5 + 0.02]));
            }
            let (tyres, rims) = wheelset(&[l * 0.5 - 2.4, -l * 0.5 + 3.0], w * 0.5 - 0.3, r, 0.34, 0);
            let (hl, tl) = lamps(l * 0.5 + 0.02, -l * 0.5 - 0.02, low + 0.4, w * 0.38, [0.3, 0.15]);
            Output::Parts(vec![
                Part::new(body, v.color("color")).material("accent"),
                Part::new(glass, GLASS).material("glass"),
                Part::new(doors, DARK),
                Part::new(tyres, RUBBER).material("ground"),
                Part::new(rims, RIM).material("metal"),
                Part::new(hl, HEAD).material("glow"),
                Part::new(tl, TAIL).material("glow"),
            ])
        });

    basic!(_r, "vehicles-motorcycles", "vehicles", "motorcycles", "Motorcycles", "accent",
        [P::num("wheelbase", 1.45, 1.2, 1.8).unit("m").describe("Wheelbase (m)"), P::num("wheel", 0.32, 0.25, 0.4).unit("m").describe("Wheel radius (m)"), P::num("rake", 25.0, 18.0, 40.0).unit("deg").describe("Fork rake (degrees)"), P::color("color", [0.05, 0.25, 0.6]).describe("Tank colour")],
        |v: &Values, _s: u32| {
            let (wb, r, rake) = (v.f("wheelbase"), v.f("wheel"), v.f("rake").to_radians());
            let (fx, rx) = (wb * 0.5, -wb * 0.5);
            let (t1, r1) = c::wheel([fx, r, 0.0], r, 0.12, 0);
            let (t2, r2) = c::wheel([rx, r, 0.0], r, 0.15, 0);
            let mut tyres = t1;
            geo::merge(&mut tyres, &t2);
            let mut rims = r1;
            geo::merge(&mut rims, &r2);
            // The fork leans back by the rake from the front hub to the head.
            let head = [fx - (r + 0.45) * rake.sin(), r + (r + 0.45) * rake.cos(), 0.0];
            let mut frame = Mesh::default();
            for z in [-0.07, 0.07] {
                geo::merge(&mut frame, &b::rod([fx, r, z], [head[0], head[1], z], 0.025, 8));
                geo::merge(&mut frame, &b::rod([rx, r, z], [rx + 0.55, r + 0.35, z], 0.022, 8));
            }
            let seat_end = [rx + 0.2, r + 0.55, 0.0];
            geo::merge(&mut frame, &b::rod(head, [0.0, r + 0.2, 0.0], 0.03, 8));
            geo::merge(&mut frame, &b::rod([0.0, r + 0.2, 0.0], seat_end, 0.025, 8));
            geo::merge(&mut frame, &b::rod([head[0] - 0.05, head[1] + 0.1, -0.35], [head[0] - 0.05, head[1] + 0.1, 0.35], 0.016, 8));
            let mut engine = b::rounded_box([0.42, 0.36, 0.32], 0.05, 2, [0.05, r - 0.08, 0.0]);
            geo::merge(&mut engine, &b::rod([0.0, r - 0.05, 0.14], [rx - 0.05, r + 0.05, 0.18], 0.04, 10));
            let tank = c::ball([0.3, 0.13, 0.15], [head[0] - 0.3, head[1] - 0.05, 0.0]);
            let seat = b::rounded_box([0.6, 0.1, 0.26], 0.04, 2, [rx + 0.45, r + 0.52, 0.0]);
            let lamp = c::ball([0.05, 0.08, 0.08], [head[0] + 0.08, head[1] - 0.05, 0.0]);
            Output::Parts(vec![
                Part::new(tank, v.color("color")).material("accent"),
                Part::new(frame, [0.15, 0.15, 0.16]).material("metal"),
                Part::new(engine, [0.3, 0.3, 0.32]).material("metal"),
                Part::new(seat, DARK).material("ground"),
                Part::new(tyres, RUBBER).material("ground"),
                Part::new(rims, RIM).material("metal"),
                Part::new(lamp, HEAD).material("glow"),
            ])
        });

    basic!(_r, "vehicles-bicycles", "vehicles", "bicycles", "Bicycles", "accent",
        [P::num("frame", 0.54, 0.4, 0.65).unit("m").describe("Frame size (m)"), P::num("wheel", 0.34, 0.25, 0.37).unit("m").describe("Wheel radius (m)"), P::num("spokes", 16.0, 6.0, 32.0).step(1.0).describe("Spokes"), P::color("color", [0.0, 0.42, 0.3]).describe("Frame colour")],
        |v: &Values, _s: u32| {
            let (f, r, n) = (v.f("frame"), v.f("wheel"), v.f("spokes").round() as u32);
            let wb = r * 3.0;
            let (fh, rh) = ([wb * 0.5, r, 0.0], [-wb * 0.5, r, 0.0]);
            let (t1, r1) = c::wheel(fh, r, 0.035, n);
            let (t2, r2) = c::wheel(rh, r, 0.035, n);
            let mut tyres = t1;
            geo::merge(&mut tyres, &t2);
            let mut rims = r1;
            geo::merge(&mut rims, &r2);
            // The diamond: seat tube, top tube, down tube, stays and fork.
            let bb = [0.0, r * 0.85, 0.0];
            let seat = [-f * 0.3, bb[1] + f, 0.0];
            let head = [wb * 0.5 - r * 0.35, bb[1] + f * 0.92, 0.0];
            let head_lo = [head[0] + 0.04, head[1] - 0.15, 0.0];
            let mut frame = Mesh::default();
            for (a, z) in [(bb, seat), (seat, head), (bb, head_lo), (head_lo, head)] {
                geo::merge(&mut frame, &b::rod(a, z, 0.018, 8));
            }
            for zz in [-0.05, 0.05] {
                geo::merge(&mut frame, &b::rod([rh[0], r, zz], [bb[0], bb[1], 0.0], 0.01, 6));
                geo::merge(&mut frame, &b::rod([rh[0], r, zz], [seat[0], seat[1], 0.0], 0.01, 6));
                geo::merge(&mut frame, &b::rod([fh[0], r, zz], head_lo, 0.012, 6));
            }
            let mut parts = b::rod(seat, [seat[0] - 0.03, seat[1] + 0.12, 0.0], 0.012, 6);
            geo::merge(&mut parts, &b::rounded_box([0.26, 0.05, 0.12], 0.02, 2, [seat[0] - 0.05, seat[1] + 0.12, 0.0]));
            geo::merge(&mut parts, &b::rod(head, [head[0] - 0.05, head[1] + 0.12, 0.0], 0.014, 6));
            geo::merge(&mut parts, &b::rod([head[0] - 0.05, head[1] + 0.12, -0.25], [head[0] - 0.05, head[1] + 0.12, 0.25], 0.012, 6));
            geo::merge(&mut parts, &b::rod([0.0, bb[1], -0.06], [0.0, bb[1], 0.06], 0.1, 16));
            Output::Parts(vec![
                Part::new(frame, v.color("color")).material("accent"),
                Part::new(parts, DARK),
                Part::new(tyres, RUBBER).material("ground"),
                Part::new(rims, RIM).material("metal"),
            ])
        });

    basic!(_r, "vehicles-quad-bikes", "vehicles", "quad-bikes", "Quad bikes", "accent",
        [P::num("size", 1.0, 0.6, 1.4).describe("Size"), P::num("wheel", 0.3, 0.22, 0.42).unit("m").describe("Wheel radius (m)"), P::color("color", [0.7, 0.32, 0.02]).describe("Body colour")],
        |v: &Values, _s: u32| {
            let (k, r) = (v.f("size"), v.f("wheel"));
            let (tyres, rims) = wheelset(&[0.6, -0.6], 0.5, r, 0.26, 0);
            let mut body = b::rounded_box([1.4, 0.32, 0.75], 0.12, 3, [0.0, r + 0.12, 0.0]);
            // Fenders over each axle, racks front and back.
            for x in [0.6f32, -0.6] {
                geo::merge(&mut body, &b::rounded_box([0.7, 0.1, 1.2], 0.04, 2, [x, r * 2.0 + 0.04, 0.0]));
            }
            let seat = b::rounded_box([0.6, 0.12, 0.36], 0.05, 2, [-0.15, r + 0.44, 0.0]);
            let mut steel = b::rod([0.38, r + 0.45, 0.0], [0.28, r + 0.8, 0.0], 0.025, 8);
            geo::merge(&mut steel, &b::rod([0.28, r + 0.8, -0.38], [0.28, r + 0.8, 0.38], 0.018, 8));
            for x in [0.82f32, -0.82] {
                geo::merge(&mut steel, &b::cbox([0.25, 0.03, 0.8], [x, r * 2.0 + 0.16, 0.0]));
            }
            let mut ps = vec![
                Part::new(body, v.color("color")).material("accent"),
                Part::new(seat, DARK).material("ground"),
                Part::new(steel, [0.15, 0.15, 0.16]).material("metal"),
                Part::new(tyres, RUBBER).material("ground"),
                Part::new(rims, RIM).material("metal"),
            ];
            c::scale_parts(&mut ps, k);
            Output::Parts(ps)
        });

    basic!(_r, "vehicles-tractors", "vehicles", "tractors", "Tractors", "accent",
        [P::num("rear", 0.8, 0.55, 1.1).unit("m").describe("Rear wheel radius (m)"), P::num("cab", 1.6, 1.2, 2.0).unit("m").describe("Cab height above the deck (m)"), P::color("color", [0.12, 0.38, 0.08]).describe("Body colour")],
        |v: &Values, _s: u32| {
            let (rr, ch) = (v.f("rear"), v.f("cab"));
            let fr = rr * 0.55;
            let (t1, r1) = wheelset(&[-0.9], 0.85, rr, 0.5, 0);
            let (t2, r2) = wheelset(&[1.5], 0.75, fr, 0.32, 0);
            let mut tyres = t1;
            geo::merge(&mut tyres, &t2);
            let mut rims = r1;
            geo::merge(&mut rims, &r2);
            // A long bonnet over the front axle, the cab over the rear.
            let deck = rr + 0.15;
            let mut body = b::rounded_box([1.9, 0.75, 0.85], 0.08, 2, [0.85, fr + 0.15, 0.0]);
            geo::merge(&mut body, &b::cbox([1.3, 0.35, 1.3], [-0.85, deck - 0.17, 0.0]));
            geo::merge(&mut body, &b::cbox([1.3, 0.08, 1.4], [-0.85, deck + ch, 0.0]));
            let mut frame = Mesh::default();
            for (x, z) in [(-0.25, 0.62), (-0.25, -0.62), (-1.45, 0.62), (-1.45, -0.62)] {
                geo::merge(&mut frame, &b::post(x, z, 0.06, deck + ch));
            }
            let glass = b::cbox([1.15, ch - 0.1, 1.2], [-0.85, deck + ch * 0.5, 0.0]);
            geo::merge(&mut frame, &b::rod([1.3, fr + 0.9, 0.25], [1.3, fr + 1.7, 0.25], 0.05, 10));
            let mut grille = b::cbox([0.04, 0.6, 0.7], [1.81, fr + 0.5, 0.0]);
            geo::merge(&mut grille, &b::cbox([0.04, 0.12, 0.12], [1.81, fr + 0.88, 0.3]));
            Output::Parts(vec![
                Part::new(body, v.color("color")).material("accent"),
                Part::new(glass, GLASS).alpha(0.6).material("glass"),
                Part::new(frame, DARK).material("metal"),
                Part::new(grille, DARK),
                Part::new(tyres, RUBBER).material("ground"),
                Part::new(rims, [0.6, 0.5, 0.05]).material("metal"),
            ])
        });

    basic!(_r, "vehicles-trailers", "vehicles", "trailers", "Trailers", "accent",
        [P::num("length", 3.0, 1.5, 8.0).unit("m").describe("Bed length (m)"), P::num("sides", 0.4, 0.0, 1.5).unit("m").describe("Side wall height (m)"), P::num("axles", 1.0, 1.0, 3.0).step(1.0).describe("Axles"), P::color("color", [0.3, 0.32, 0.34]).describe("Colour")],
        |v: &Values, _s: u32| {
            let (l, sw, n) = (v.f("length"), v.f("sides"), v.f("axles").round() as u32);
            let (w, r) = (1.6, 0.3);
            let deck = r * 2.0 + 0.1;
            let mut bed = b::cbox([l, 0.1, w], [0.0, deck, 0.0]);
            if sw > 0.01 {
                for z in [-w * 0.5, w * 0.5] {
                    geo::merge(&mut bed, &b::cbox([l, sw, 0.04], [0.0, deck + sw * 0.5, z]));
                }
                for x in [-l * 0.5, l * 0.5] {
                    geo::merge(&mut bed, &b::cbox([0.04, sw, w], [x, deck + sw * 0.5, 0.0]));
                }
            }
            // An A-frame drawbar to the hitch and a jockey wheel.
            let mut steel = Mesh::default();
            for z in [-0.6, 0.6] {
                geo::merge(&mut steel, &b::bar([l * 0.5, deck - 0.08, z], [l * 0.5 + 1.2, deck - 0.1, 0.0], 0.08, 0.08));
            }
            geo::merge(&mut steel, &b::rod([l * 0.5 + 1.15, deck - 0.1, 0.0], [l * 0.5 + 1.15, 0.12, 0.0], 0.03, 8));
            let xs: Vec<f32> = (0..n).map(|i| (i as f32 - (n - 1) as f32 * 0.5) * 0.75 - l * 0.05).collect();
            let (mut tyres, rims) = wheelset(&xs, w * 0.5 + 0.12, r, 0.18, 0);
            geo::merge(&mut tyres, &c::wheel([l * 0.5 + 1.15, 0.1, 0.0], 0.1, 0.06, 0).0);
            for &x in &xs {
                geo::merge(&mut steel, &b::rod([x, r, -w * 0.5 - 0.1], [x, r, w * 0.5 + 0.1], 0.04, 8));
            }
            let (_, tl) = lamps(l * 0.5, -l * 0.5 - 0.01, deck - 0.02, w * 0.4, [0.2, 0.08]);
            Output::Parts(vec![
                Part::new(bed, v.color("color")).material("accent"),
                Part::new(steel, DARK).material("metal"),
                Part::new(tyres, RUBBER).material("ground"),
                Part::new(rims, RIM).material("metal"),
                Part::new(tl, TAIL).material("glow"),
            ])
        });

    basic!(_r, "vehicles-train-carriages", "vehicles", "train-carriages", "Train carriages", "accent",
        [P::num("length", 20.0, 10.0, 26.0).unit("m").describe("Length (m)"), P::num("windows", 10.0, 4.0, 16.0).step(1.0).describe("Windows a side"), P::color("color", [0.06, 0.24, 0.12]).describe("Body colour")],
        |v: &Values, _s: u32| {
            let (l, n) = (v.f("length"), v.f("windows").round() as u32);
            let (w, low, h) = (2.8, 1.0, 2.9);
            // A boxy section with a rounded roof, lofted along the length.
            let body = c::loft_x(-l * 0.5, l * 0.5, 2, |_| low + h * 0.5, |_| c::squircle(w * 0.5, h * 0.5, 5.0, 32));
            let mut glass = Mesh::default();
            let pitch = (l - 3.0) / n as f32;
            for i in 0..n {
                let x = -l * 0.5 + 1.5 + (i as f32 + 0.5) * pitch;
                for z in [-w * 0.5 - 0.005, w * 0.5 + 0.005] {
                    geo::merge(&mut glass, &b::cbox([pitch * 0.7, 0.85, 0.03], [x, low + h * 0.62, z]));
                }
            }
            let mut doors = Mesh::default();
            for x in [-l * 0.5 + 0.8, l * 0.5 - 0.8] {
                for z in [-w * 0.5 - 0.01, w * 0.5 + 0.01] {
                    geo::merge(&mut doors, &b::cbox([0.9, 2.0, 0.03], [x, low + 1.1, z]));
                }
            }
            // Two bogies of two wheelsets each, on a short length of track.
            let mut bogie = Mesh::default();
            let mut wheels = Mesh::default();
            for bx in [-l * 0.5 + 3.0, l * 0.5 - 3.0] {
                geo::merge(&mut bogie, &b::cbox([2.8, 0.35, w * 0.7], [bx, 0.75, 0.0]));
                for x in [bx - 1.0, bx + 1.0] {
                    for z in [-0.72f32, 0.72] {
                        geo::merge(&mut wheels, &b::rod([x, 0.45, z - 0.06 * z.signum()], [x, 0.45, z + 0.04 * z.signum()], 0.45, 20));
                    }
                    geo::merge(&mut bogie, &b::rod([x, 0.45, -0.8], [x, 0.45, 0.8], 0.07, 8));
                }
            }
            let mut rails = Mesh::default();
            for z in [-0.72, 0.72] {
                geo::merge(&mut rails, &b::cbox([l + 2.0, 0.15, 0.08], [0.0, 0.075, z]));
            }
            let mut wheels_rails = wheels;
            geo::merge(&mut wheels_rails, &rails);
            Output::Parts(vec![
                Part::new(body, v.color("color")).material("accent"),
                Part::new(glass, GLASS).material("glass"),
                Part::new(doors, DARK),
                Part::new(bogie, [0.1, 0.1, 0.1]),
                Part::new(wheels_rails, [0.35, 0.35, 0.37]).material("metal"),
            ])
        });

    basic!(_r, "vehicles-tanks", "vehicles", "tanks", "Tanks", "ground",
        [P::num("barrel", 4.5, 2.0, 7.0).unit("m").describe("Gun length (m)"), P::num("turn", 20.0, -180.0, 180.0).unit("deg").describe("Turret turn (degrees)"), P::num("lift", 0.0, -5.0, 20.0).unit("deg").describe("Gun elevation (degrees)"), P::color("color", [0.2, 0.24, 0.12]).describe("Paint colour")],
        |v: &Values, _s: u32| {
            let (gl, turn, lift) = (v.f("barrel"), v.f("turn").to_radians(), v.f("lift").to_radians());
            let (l, w) = (7.0, 3.6);
            // The hull: a side outline with a sloped glacis.
            let hull = b::extrude(&[[-l * 0.5, 0.5], [l * 0.5 - 0.8, 0.45], [l * 0.5, 1.0], [l * 0.5 - 1.6, 1.75], [-l * 0.5, 1.8]], w - 1.2);
            let mut deck = b::cbox([l - 0.3, 0.12, w], [-0.1, 1.55, 0.0]);
            // Tracks: a belt swept round a stadium loop each side; road wheels.
            let mut tracks = Mesh::default();
            let mut wheels = Mesh::default();
            for z in [-(w * 0.5 - 0.3), w * 0.5 - 0.3] {
                let (ra, x0, x1) = (0.45f32, -l * 0.5 + 0.45, l * 0.5 - 0.45);
                let mut path: Vec<[f32; 3]> = Vec::new();
                for k in 0..=11 {
                    let a = -sh::PI * 0.5 + k as f32 / 11.0 * sh::PI;
                    path.push([x1 + ra * a.cos(), ra + ra * a.sin() + 0.05, z]);
                }
                for k in 0..=11 {
                    let a = sh::PI * 0.5 + k as f32 / 11.0 * sh::PI;
                    path.push([x0 + ra * a.cos(), ra + ra * a.sin() + 0.05, z]);
                }
                path.push(path[0]);
                geo::merge(&mut tracks, &b::sweep(&path, &b::rect(0.55, 0.08), false));
                for i in 0..6 {
                    let x = x0 + (x1 - x0) * i as f32 / 5.0;
                    geo::merge(&mut wheels, &b::rod([x, 0.5, z - 0.22], [x, 0.5, z + 0.22], 0.36, 16));
                }
            }
            // The turret and gun, turned about its ring.
            let mut turret = b::extrude(&[[-1.6, 0.0], [1.0, 0.0], [1.5, 0.3], [1.2, 0.75], [-1.4, 0.8]], 2.4);
            let (ca, sa) = (lift.cos(), lift.sin());
            let mut gun = b::rod([1.2, 0.4, 0.0], [1.2 + gl * ca, 0.4 + gl * sa, 0.0], 0.09, 12);
            geo::merge(&mut gun, &b::rod([1.2 + gl * 0.75 * ca, 0.4 + gl * 0.75 * sa, 0.0], [1.2 + gl * 0.85 * ca, 0.4 + gl * 0.85 * sa, 0.0], 0.15, 12));
            geo::merge(&mut turret, &b::rod([-0.4, 0.8, 0.6], [-0.4, 0.95, 0.6], 0.35, 16));
            let ty = 1.61;
            let turret = sh::place(&turret, 1.0, turn, 0.0, [0.3, ty, 0.0]);
            let gun = sh::place(&gun, 1.0, turn, 0.0, [0.3, ty, 0.0]);
            geo::merge(&mut deck, &turret);
            Output::Parts(vec![
                Part::new(hull, v.color("color")),
                Part::new(deck, v.color("color")),
                Part::new(gun, [0.15, 0.17, 0.1]).material("metal"),
                Part::new(tracks, [0.08, 0.08, 0.08]).material("metal"),
                Part::new(wheels, [0.18, 0.2, 0.12]),
            ])
        });

    basic!(_r, "vehicles-off-road-buggies", "vehicles", "off-road-buggies", "Off-road buggies", "accent",
        [P::num("wheel", 0.42, 0.3, 0.6).unit("m").describe("Wheel radius (m)"), P::num("ride", 0.35, 0.15, 0.7).unit("m").describe("Ride height (m)"), P::color("color", [0.85, 0.5, 0.0]).describe("Frame colour")],
        |v: &Values, _s: u32| {
            let (r, ride) = (v.f("wheel"), v.f("ride"));
            let (l, w) = (2.8, 1.5);
            let (tyres, rims) = wheelset(&[l * 0.38, -l * 0.38], w * 0.5 + 0.15, r, 0.32, 0);
            let y0 = r + ride * 0.5;
            // A roll cage of tubes over a floor pan; the engine at the back.
            let mut cage = Mesh::default();
            let pts = |x: f32, y: f32, z: f32| [x, y0 + y, z];
            for z in [-0.55f32, 0.55] {
                for (a, c2) in [((1.2, 0.0), (0.3, 0.9)), ((0.3, 0.9), (-0.6, 1.0)), ((-0.6, 1.0), (-1.0, 0.1)), ((1.2, 0.0), (-1.2, 0.0)), ((-0.6, 1.0), (-0.6, 0.0))] {
                    geo::merge(&mut cage, &b::rod(pts(a.0, a.1, z), pts(c2.0, c2.1, z * 0.9), 0.03, 8));
                }
            }
            for (x, y) in [(0.3, 0.9), (-0.6, 1.0), (1.2, 0.0), (-1.2, 0.0)] {
                geo::merge(&mut cage, &b::rod(pts(x, y, -0.55), pts(x, y, 0.55), 0.03, 8));
            }
            let mut pan = b::cbox([2.4, 0.06, 1.1], [0.0, y0, 0.0]);
            geo::merge(&mut pan, &b::rounded_box([0.6, 0.45, 0.42], 0.06, 2, [-0.15, y0 + 0.03, 0.27]));
            geo::merge(&mut pan, &b::rounded_box([0.6, 0.45, 0.42], 0.06, 2, [-0.15, y0 + 0.03, -0.27]));
            let mut engine = b::rounded_box([0.6, 0.5, 0.8], 0.05, 2, [-1.0, y0 + 0.03, 0.0]);
            for z in [-0.5f32, 0.5] {
                geo::merge(&mut engine, &b::bar([0.9, y0, z * 1.3], [l * 0.38, r, z * 2.0], 0.06, 0.06));
                geo::merge(&mut engine, &b::bar([-0.9, y0, z * 1.3], [-l * 0.38, r, z * 2.0], 0.06, 0.06));
            }
            Output::Parts(vec![
                Part::new(cage, v.color("color")).material("accent"),
                Part::new(pan, DARK),
                Part::new(engine, [0.28, 0.28, 0.3]).material("metal"),
                Part::new(tyres, RUBBER).material("ground"),
                Part::new(rims, RIM).material("metal"),
            ])
        });

    basic!(_r, "vehicles-emergency-vehicles", "vehicles", "emergency-vehicles", "Emergency vehicles", "accent",
        [P::num("kind", 0.0, 0.0, 2.0).step(1.0).describe("Kind: 0 police car, 1 ambulance, 2 fire engine"), P::num("length", 1.0, 0.85, 1.25).describe("Length (share of the usual)"), P::color("color", [0.03, 0.12, 0.55]).describe("Stripe colour")],
        |v: &Values, _s: u32| {
            let (kind, k) = (v.f("kind").round() as u32, v.f("length"));
            let r = if kind == 2 { 0.5 } else { 0.34 };
            let low = r * 0.78;
            let (white, red) = ([0.78, 0.78, 0.76], [0.6, 0.04, 0.03]);
            let (l, w, top, mut ps) = match kind {
                0 => {
                    let l = 4.9 * k;
                    (l, 1.85, 1.45, car(&Car { len: l, wid: 1.85, low, nose: low + 0.42, belt: low + 0.58, tail: low + 0.55, roof: 1.45, g: [l * 0.2, l * 0.02, -l * 0.2, -l * 0.33], r, axles: [l * 0.32, -l * 0.3] }, white))
                }
                1 => {
                    let l = 6.0 * k;
                    let mut ps = car(&Car { len: l, wid: 2.1, low, nose: low + 0.62, belt: low + 0.95, tail: 2.6, roof: 2.65, g: [l * 0.5 - 0.75, l * 0.5 - 1.4, l * 0.5 - 1.55, l * 0.5 - 1.6], r, axles: [l * 0.5 - 0.9, -l * 0.5 + 1.1] }, white);
                    ps.push(Part::new(b::cbox([l - 1.6, 2.65 - low - 0.45, 2.1], [-0.8, (low + 0.45 + 2.65) * 0.5, 0.0]), white).material("accent"));
                    (l, 2.1, 2.65, ps)
                }
                _ => {
                    let l = 9.0 * k;
                    let body = b::rounded_box([l, 2.3, 2.5], 0.1, 2, [0.0, r + 0.3, 0.0]);
                    let glass = b::cbox([0.06, 0.8, 2.2], [l * 0.5 + 0.01, r + 1.9, 0.0]);
                    let (tyres, rims) = wheelset(&[l * 0.5 - 1.4, -l * 0.5 + 1.8, -l * 0.5 + 3.0], 0.95, r, 0.38, 0);
                    // A ladder along the roof.
                    let mut ladder = Mesh::default();
                    for z in [-0.35, 0.35] {
                        geo::merge(&mut ladder, &b::rod([-l * 0.5, r + 2.75, z], [l * 0.5 - 1.5, r + 2.75, z], 0.04, 8));
                    }
                    let rungs = ((l - 1.5) / 0.35) as u32;
                    for i in 0..rungs {
                        let x = -l * 0.5 + i as f32 * 0.35;
                        geo::merge(&mut ladder, &b::rod([x, r + 2.75, -0.35], [x, r + 2.75, 0.35], 0.02, 6));
                    }
                    (l, 2.5, r + 2.6, vec![Part::new(body, red).material("accent"), Part::new(glass, GLASS).material("glass"), Part::new(tyres, RUBBER).material("ground"), Part::new(rims, RIM).material("metal"), Part::new(ladder, [0.6, 0.6, 0.62]).material("metal")])
                }
            };
            // A stripe down each side and a light bar on the roof.
            let mut stripe = Mesh::default();
            for z in [-w * 0.5 - 0.01, w * 0.5 + 0.01] {
                geo::merge(&mut stripe, &b::cbox([l * 0.9, 0.16, 0.02], [0.0, low + 0.42, z]));
            }
            ps.push(Part::new(stripe, v.color("color")).material("accent"));
            let bar_x = if kind == 0 { -l * 0.08 } else { l * 0.5 - 1.5 };
            ps.push(Part::new(b::cbox([0.25, 0.12, w * 0.32], [bar_x, top + 0.08, w * 0.17]), [0.1, 0.25, 1.0]).material("glow"));
            ps.push(Part::new(b::cbox([0.25, 0.12, w * 0.32], [bar_x, top + 0.08, -w * 0.17]), [1.0, 0.08, 0.05]).material("glow"));
            Output::Parts(ps)
        });
}
