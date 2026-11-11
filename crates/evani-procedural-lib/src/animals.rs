//! Category 20, Animals and creatures: basic generators (batch C). The
//! four-legged ones share the base's template (`craft::Quad`: a lofted
//! barrel, jointed legs, neck, head, snout, ears and tail, its proportions
//! the params of each species), with their own extras: manes, horns,
//! antlers, wool, humps, patches, wings. Birds, fish, sharks, whales and
//! insects are lofted or ball bodies with the base's fin and wing panels.
//! Head at +x. Inspired by three.js (MIT) capsule and LatheGeometry, and
//! the "body proportions as params" idea of creature makers; written fresh
//! here.

use evani_procedural::Registry;

#[allow(unused_imports, dead_code)]
pub fn register(_r: &mut Registry) {
    use evani_procedural::{Output, ParamSpec as P, Part, Values, build as b, craft as c, geo, shapes as sh};
    type Mesh = geo::Mesh;
    const DARK: [f32; 3] = [0.03, 0.03, 0.03];
    const HOOF: [f32; 3] = [0.08, 0.07, 0.06];
    const HORN: [f32; 3] = [0.6, 0.55, 0.45];

    fn moved(mut m: Mesh, d: [f32; 3]) -> Mesh {
        geo::transform(&mut m, [1.0; 3], d);
        m
    }

    /// The template's body in `coat`, with `extra` parts, scaled by `k`.
    fn animal(q: &c::Quad, coat: [f32; 3], feet: [f32; 3], extra: Vec<Part>, k: f32) -> Output {
        let body = c::quadruped(q);
        let mut ps = vec![Part::new(body.skin, coat).material("ground"), Part::new(body.feet, feet).material("ground"), Part::new(body.eyes, DARK).material("glass"), Part::new(body.nose, DARK).material("ground")];
        ps.extend(extra);
        c::scale_parts(&mut ps, k);
        c::stand(ps)
    }

    /// A tail of hair from `at`, `len` long, hanging down and back.
    fn hair_tail(at: [f32; 3], len: f32, r: f32, lift: f32) -> Mesh {
        let path: Vec<[f32; 3]> = (0..8).map(|i| {
            let t = i as f32 / 7.0;
            [at[0] - len * 0.35 * t - len * 0.1 * lift * t, at[1] - len * t * (1.0 - lift) + len * lift * 0.4 * t, at[2]]
        }).collect();
        sh::tube(&path, |t| r * (0.6 + 0.6 * t.min(0.7)) * (1.0 - (t - 0.85).max(0.0) * 5.0), 10)
    }

    /// Seeded blobs over the barrel of `q` (patches, wool): `n` of radius `r`.
    fn on_barrel(q: &c::Quad, n: u32, r: f32, sink: f32, seed: u32) -> Vec<[f32; 3]> {
        let cy = q.leg + q.girth * 0.75;
        (0..n).map(|i| {
            let t = sh::range(seed, i, 1, 0.12, 0.88);
            let a = sh::range(seed, i, 2, -0.35, 1.0) * sh::PI;
            let u = (2.0 * t - 1.0).abs();
            let k = (1.0 - u.powi(3)).max(0.0).cbrt();
            [-q.len * 0.5 + q.len * t, cy + q.girth * k * a.sin() * (1.0 - sink * r / q.girth), q.girth * 0.82 * k * a.cos() * (1.0 - sink * r / q.girth)]
        }).collect()
    }

    basic!(_r, "animals-horses", "animals", "horses", "Horses", "ground",
        [P::num("size", 1.0, 0.6, 1.2).describe("Size (1 a 1.6 m horse)"), P::num("legs", 1.0, 0.8, 1.2).describe("Leg length"), P::num("neck", 0.85, 0.6, 1.1).unit("m").describe("Neck length (m)"), P::color("color", [0.32, 0.17, 0.08]).describe("Coat colour"), P::color("mane", [0.06, 0.04, 0.03]).describe("Mane and tail colour")],
        |v: &Values, _s: u32| {
            let q = c::Quad { len: 1.9, girth: 0.36, leg: 1.0 * v.f("legs"), leg_r: 0.055, neck: v.f("neck"), neck_up: 0.95, head: 0.6, snout: 0.55, tail: 0.0, tail_up: 0.0, ears: 0.12 };
            let body = c::quadruped(&q);
            // A mane down the top of the neck, a long hair tail.
            let [a, z] = body.neck_line;
            let mut hair = b::sweep(&[c::lerp3(a, z, -0.05), c::lerp3(a, z, 0.5), z], &b::rect(0.05, 0.14), true);
            geo::merge(&mut hair, &hair_tail(body.rump, 0.85, 0.07, 0.1));
            animal(&q, v.color("color"), HOOF, vec![Part::new(hair, v.color("mane")).material("ground")], v.f("size"))
        });

    basic!(_r, "animals-dogs", "animals", "dogs", "Dogs", "ground",
        [P::num("size", 1.0, 0.4, 1.4).describe("Size (1 a 0.6 m dog)"), P::num("legs", 1.0, 0.6, 1.3).describe("Leg length"), P::num("ears", 0.09, 0.02, 0.16).unit("m").describe("Ear length (m)"), P::color("color", [0.55, 0.36, 0.16]).describe("Coat colour")],
        |v: &Values, _s: u32| {
            let q = c::Quad { len: 0.75, girth: 0.17, leg: 0.42 * v.f("legs"), leg_r: 0.032, neck: 0.25, neck_up: 0.8, head: 0.26, snout: 0.6, tail: 0.35, tail_up: 0.6, ears: v.f("ears") };
            animal(&q, v.color("color"), [0.2, 0.14, 0.08], vec![], v.f("size"))
        });

    basic!(_r, "animals-cats", "animals", "cats", "Cats", "ground",
        [P::num("size", 1.0, 0.6, 1.6).describe("Size (1 a house cat)"), P::num("tail", 0.42, 0.1, 0.6).unit("m").describe("Tail length (m)"), P::num("legs", 1.0, 0.7, 1.3).describe("Leg length"), P::color("color", [0.45, 0.42, 0.4]).describe("Coat colour")],
        |v: &Values, _s: u32| {
            let q = c::Quad { len: 0.45, girth: 0.1, leg: 0.22 * v.f("legs"), leg_r: 0.018, neck: 0.1, neck_up: 0.9, head: 0.13, snout: 0.15, tail: v.f("tail"), tail_up: 0.95, ears: 0.05 };
            animal(&q, v.color("color"), [0.3, 0.28, 0.27], vec![], v.f("size"))
        });

    basic!(_r, "animals-cattle", "animals", "cattle", "Cattle", "ground",
        [P::num("size", 1.0, 0.7, 1.3).describe("Size (1 a 1.4 m cow)"), P::num("horns", 0.2, 0.0, 0.6).unit("m").describe("Horn length (m)"), P::num("patches", 8.0, 0.0, 20.0).step(1.0).describe("Patches"), P::color("color", [0.75, 0.74, 0.7]).describe("Coat colour"), P::color("patch", [0.05, 0.05, 0.05]).describe("Patch colour")],
        |v: &Values, s: u32| {
            let (hl, np) = (v.f("horns"), v.f("patches").round() as u32);
            let q = c::Quad { len: 2.0, girth: 0.48, leg: 0.72, leg_r: 0.065, neck: 0.45, neck_up: 0.25, head: 0.55, snout: 0.35, tail: 0.8, tail_up: -0.3, ears: 0.1 };
            let body = c::quadruped(&q);
            let mut horns = Mesh::default();
            if hl > 0.01 {
                for side in [-1.0f32, 1.0] {
                    let base = [body.crown[0] - 0.05, body.crown[1], side * 0.12];
                    let path: Vec<[f32; 3]> = (0..6).map(|i| {
                        let t = i as f32 / 5.0;
                        [base[0] + hl * 0.2 * t, base[1] + hl * 0.5 * t * t, base[2] + side * hl * t]
                    }).collect();
                    geo::merge(&mut horns, &sh::tube(&path, |t| 0.035 * (1.0 - t * 0.85), 8));
                }
            }
            let mut patches = Mesh::default();
            for (i, p) in on_barrel(&q, np, 0.2, 0.4, s).into_iter().enumerate() {
                let r = sh::range(s, i as u32, 3, 0.12, 0.24);
                geo::merge(&mut patches, &c::ball([r * 1.3, r, r * 0.5 + 0.05], p));
            }
            let udder = c::ball([0.14, 0.1, 0.12], [-q.len * 0.25, q.leg + 0.02, 0.0]);
            animal(&q, v.color("color"), HOOF, vec![Part::new(horns, HORN).material("ground"), Part::new(patches, v.color("patch")).material("ground"), Part::new(udder, [0.75, 0.5, 0.48]).material("ground")], v.f("size"))
        });

    basic!(_r, "animals-pigs", "animals", "pigs", "Pigs", "ground",
        [P::num("size", 1.0, 0.4, 1.4).describe("Size (1 a 1.2 m pig)"), P::num("girth", 1.0, 0.8, 1.35).describe("Girth"), P::num("legs", 1.0, 0.7, 1.4).describe("Leg length"), P::color("color", [0.85, 0.55, 0.5]).describe("Skin colour")],
        |v: &Values, _s: u32| {
            let q = c::Quad { len: 1.2, girth: 0.36 * v.f("girth"), leg: 0.3 * v.f("legs"), leg_r: 0.05, neck: 0.12, neck_up: 0.15, head: 0.38, snout: 0.45, tail: 0.15, tail_up: 0.8, ears: 0.12 };
            let body = c::quadruped(&q);
            // A flat snout disc at the nose's end.
            let n = c::lerp3(body.head_at, [body.head_at[0] + body.head_dir[0], body.head_at[1] + body.head_dir[1], 0.0], q.head * (0.3 + q.snout * 0.55));
            let disc = b::rod(n, c::lerp3(n, [n[0] + body.head_dir[0], n[1] + body.head_dir[1], 0.0], 0.04), q.head * 0.15, 16);
            animal(&q, v.color("color"), [0.4, 0.28, 0.25], vec![Part::new(disc, [0.75, 0.42, 0.4]).material("ground")], v.f("size"))
        });

    basic!(_r, "animals-sheep", "animals", "sheep", "Sheep", "ground",
        [P::num("size", 1.0, 0.6, 1.3).describe("Size"), P::num("wool", 0.1, 0.03, 0.18).unit("m").describe("Wool depth (m)"), P::num("legs", 1.0, 0.75, 1.3).describe("Leg length"), P::color("color", [0.82, 0.8, 0.72]).describe("Wool colour"), P::color("face", [0.1, 0.09, 0.08]).describe("Face and leg colour")],
        |v: &Values, s: u32| {
            let wd = v.f("wool");
            let q = c::Quad { len: 1.05, girth: 0.3, leg: 0.45 * v.f("legs"), leg_r: 0.03, neck: 0.25, neck_up: 0.6, head: 0.28, snout: 0.45, tail: 0.1, tail_up: 0.2, ears: 0.08 };
            // Wool: seeded puffs over the barrel and up the neck.
            let pts = on_barrel(&q, 70, wd, 0.0, s);
            let rs: Vec<f32> = (0..pts.len()).map(|i| wd * sh::range(s, i as u32, 5, 1.1, 1.6)).collect();
            let wool = sh::puffs(&pts, &rs, s);
            let body = c::quadruped(&q);
            let mut ps = vec![Part::new(body.skin, v.color("face")).material("ground"), Part::new(body.feet, DARK).material("ground"), Part::new(body.eyes, DARK).material("glass"), Part::new(wool, v.color("color")).material("ground")];
            c::scale_parts(&mut ps, v.f("size"));
            c::stand(ps)
        });

    basic!(_r, "animals-deer", "animals", "deer", "Deer", "ground",
        [P::num("size", 1.0, 0.6, 1.4).describe("Size"), P::num("tines", 4.0, 0.0, 7.0).step(1.0).describe("Antler tines a side (0 a doe)"), P::num("legs", 1.0, 0.8, 1.2).describe("Leg length"), P::color("color", [0.45, 0.28, 0.14]).describe("Coat colour")],
        |v: &Values, _s: u32| {
            let n = v.f("tines").round() as u32;
            let q = c::Quad { len: 1.3, girth: 0.24, leg: 0.85 * v.f("legs"), leg_r: 0.03, neck: 0.55, neck_up: 1.0, head: 0.34, snout: 0.55, tail: 0.12, tail_up: 0.6, ears: 0.14 };
            let body = c::quadruped(&q);
            // Antlers: a beam each side sweeping up and back, tines off it.
            let mut ant = Mesh::default();
            if n > 0 {
                let len = 0.35 + 0.06 * n as f32;
                for side in [-1.0f32, 1.0] {
                    let base = [body.crown[0] - 0.03, body.crown[1], side * 0.06];
                    let beam: Vec<[f32; 3]> = (0..8).map(|i| {
                        let t = i as f32 / 7.0;
                        [base[0] - len * 0.35 * t + len * 0.2 * t * t, base[1] + len * t, base[2] + side * len * 0.45 * (t * 1.6).sin().max(0.0)]
                    }).collect();
                    geo::merge(&mut ant, &sh::tube(&beam, |t| 0.022 * (1.0 - t * 0.6), 7));
                    for k in 0..n {
                        let (p, _, _) = b::along(&beam, 0.2 + 0.75 * k as f32 / n as f32);
                        let tip = [p[0] + len * 0.22, p[1] + len * 0.25, p[2] - side * 0.02];
                        geo::merge(&mut ant, &c::cone(p, tip, 0.014, 6));
                    }
                }
            }
            let tail_white = c::ball([0.05, 0.07, 0.05], [-q.len * 0.5 + 0.02, q.leg + q.girth * 1.2, 0.0]);
            animal(&q, v.color("color"), HOOF, vec![Part::new(ant, [0.55, 0.45, 0.32]).material("ground"), Part::new(tail_white, [0.8, 0.78, 0.72]).material("ground")], v.f("size"))
        });

    basic!(_r, "animals-wolves", "animals", "wolves", "Wolves", "ground",
        [P::num("size", 1.0, 0.7, 1.3).describe("Size"), P::num("legs", 1.0, 0.8, 1.2).describe("Leg length"), P::num("tail", 0.45, 0.2, 0.6).unit("m").describe("Tail length (m)"), P::color("color", [0.38, 0.36, 0.33]).describe("Coat colour")],
        |v: &Values, _s: u32| {
            let q = c::Quad { len: 1.1, girth: 0.21, leg: 0.6 * v.f("legs"), leg_r: 0.035, neck: 0.32, neck_up: 0.55, head: 0.32, snout: 0.7, tail: 0.0, tail_up: 0.0, ears: 0.1 };
            let body = c::quadruped(&q);
            // A bushy tail hanging low, a ruff at the throat.
            let mut fur = hair_tail(body.rump, v.f("tail"), 0.06, 0.2);
            geo::merge(&mut fur, &c::ball([0.16, 0.2, 0.18], [q.len * 0.5 - 0.05, q.leg + q.girth * 0.9, 0.0]));
            animal(&q, v.color("color"), [0.15, 0.14, 0.13], vec![Part::new(fur, v.color("color")).material("ground")], v.f("size"))
        });

    basic!(_r, "animals-bears", "animals", "bears", "Bears", "ground",
        [P::num("size", 1.0, 0.6, 1.5).describe("Size"), P::num("hump", 1.0, 0.0, 2.0).describe("Shoulder hump"), P::num("legs", 1.0, 0.8, 1.2).describe("Leg length"), P::color("color", [0.24, 0.14, 0.07]).describe("Fur colour")],
        |v: &Values, _s: u32| {
            let q = c::Quad { len: 1.5, girth: 0.48, leg: 0.5 * v.f("legs"), leg_r: 0.1, neck: 0.3, neck_up: 0.3, head: 0.45, snout: 0.35, tail: 0.0, tail_up: 0.0, ears: 0.07 };
            let body = c::quadruped(&q);
            let hump = c::ball([0.35, 0.16 * v.f("hump") + 0.02, 0.3], [body.withers[0] - 0.1, body.withers[1] - 0.08, 0.0]);
            animal(&q, v.color("color"), DARK, vec![Part::new(hump, v.color("color")).material("ground")], v.f("size"))
        });

    basic!(_r, "animals-birds", "animals", "birds", "Birds", "ground",
        [P::num("size", 0.3, 0.1, 1.2).unit("m").describe("Body length (m)"), P::num("span", 2.2, 1.4, 3.5).describe("Wingspan (share of the body)"), P::num("spread", 0.3, 0.0, 1.0).describe("Wings spread (0 folded)"), P::color("color", [0.3, 0.33, 0.38]).describe("Plumage colour"), P::color("beak", [0.85, 0.6, 0.05]).describe("Beak and leg colour")],
        |v: &Values, _s: u32| {
            let (l, span, sp) = (v.f("size"), v.f("span"), v.f("spread"));
            let leg = l * 0.35;
            let cy = leg + l * 0.18;
            let mut body = c::ball([l * 0.4, l * 0.2, l * 0.18], [0.0, cy, 0.0]);
            let head = [l * 0.38, cy + l * 0.2, 0.0];
            geo::merge(&mut body, &c::ball([l * 0.13; 3], head));
            geo::merge(&mut body, &c::limb([l * 0.2, cy + l * 0.05, 0.0], head, l * 0.13, l * 0.1, 10));
            // Wings swing from folded along the back to spread wide.
            let half = l * span * 0.5;
            for side in [-1.0f32, 1.0] {
                let dir = [-(1.0 - sp) * 0.95, 0.15 + 0.2 * sp, side * (0.3 + 0.7 * sp)];
                geo::merge(&mut body, &c::panel([l * 0.12, cy + l * 0.12, side * l * 0.12], dir, half * (0.55 + 0.45 * sp), l * 0.35, l * 0.15, l * 0.1 * sp, 0.08));
            }
            geo::merge(&mut body, &c::panels([-l * 0.3, cy + l * 0.04, 0.0], [-0.3, 0.05, 1.0], l * 0.1, [l * 0.3, l * 0.36], 0.0, 0.05));
            let mut beak = c::cone([head[0] + l * 0.09, head[1], 0.0], [head[0] + l * 0.26, head[1] - l * 0.03, 0.0], l * 0.045, 8);
            for z in [-l * 0.06f32, l * 0.06] {
                geo::merge(&mut beak, &b::rod([0.0, cy - l * 0.1, z], [l * 0.02, 0.0, z], l * 0.012, 6));
                geo::merge(&mut beak, &b::rod([l * 0.02, 0.005, z], [l * 0.12, 0.005, z], l * 0.008, 5));
            }
            let mut eyes = Mesh::default();
            for z in [-l * 0.1f32, l * 0.1] {
                geo::merge(&mut eyes, &c::ball([l * 0.025; 3], [head[0] + l * 0.06, head[1] + l * 0.03, z]));
            }
            c::stand(vec![Part::new(body, v.color("color")).material("ground"), Part::new(beak, v.color("beak")).material("ground"), Part::new(eyes, DARK).material("glass")])
        });

    basic!(_r, "animals-fish", "animals", "fish", "Fish", "ground",
        [P::num("length", 0.4, 0.1, 1.5).unit("m").describe("Length (m)"), P::num("depth", 0.32, 0.15, 0.6).describe("Body depth (share of the length)"), P::num("fins", 1.0, 0.5, 2.0).describe("Fin size"), P::color("color", [0.45, 0.5, 0.42]).describe("Body colour"), P::color("fin", [0.6, 0.35, 0.15]).describe("Fin colour")],
        |v: &Values, _s: u32| {
            let (l, dp, f) = (v.f("length"), v.f("depth"), v.f("fins"));
            let y = l * dp * 0.6;
            // A body flattened side to side, a forked tail and fins.
            let body = c::loft_x(-l * 0.45, l * 0.5, 20, |_| y, |t| {
                let k = c::streamline(t, 0.25, 0.65, 0.12);
                c::ellipse(l * dp * 0.22 * k, l * dp * 0.5 * k, 18)
            });
            let tx = -l * 0.42;
            let mut fins = Mesh::default();
            for up in [1.0f32, -1.0] {
                geo::merge(&mut fins, &c::panel([tx, y, 0.0], [-0.6, up * 0.8, 0.0001], l * 0.22 * f, l * 0.08, l * 0.06, 0.0, 0.08));
            }
            geo::merge(&mut fins, &c::panel([l * 0.05, y + l * dp * 0.45, 0.0], [-0.15, 1.0, 0.0], l * 0.12 * f, l * 0.25, l * 0.1, l * 0.08, 0.06));
            geo::merge(&mut fins, &c::panels([l * 0.22, y - l * dp * 0.18, l * dp * 0.18], [-0.5, -0.3, 1.0], l * 0.1 * f, [l * 0.06, l * 0.03], 0.0, 0.08));
            let mut eyes = Mesh::default();
            for z in [-1.0f32, 1.0] {
                geo::merge(&mut eyes, &c::ball([l * 0.03, l * 0.03, l * 0.012], [l * 0.36, y + l * dp * 0.12, z * l * dp * 0.13]));
            }
            c::stand(vec![Part::new(body, v.color("color")).material("ground"), Part::new(fins, v.color("fin")).material("ground"), Part::new(eyes, DARK).material("glass")])
        });

    basic!(_r, "animals-sharks", "animals", "sharks", "Sharks", "ground",
        [P::num("length", 3.5, 1.2, 8.0).unit("m").describe("Length (m)"), P::num("dorsal", 1.0, 0.5, 1.8).describe("Dorsal fin size"), P::color("color", [0.32, 0.36, 0.4]).describe("Back colour")],
        |v: &Values, _s: u32| {
            let (l, df) = (v.f("length"), v.f("dorsal"));
            let r = l * 0.1;
            let y = r * 1.1;
            // A torpedo body with a pointed snout, a tall dorsal, a lopsided tail.
            let body = c::loft_x(-l * 0.5, l * 0.5, 22, |t| y + r * 0.1 * (1.0 - t), |t| {
                let k = if t > 0.8 { ((1.0 - t) / 0.2).powf(0.6) } else { c::streamline(t, 0.0, 0.65, 0.08) };
                c::ellipse(r * 0.85 * k, r * k, 18)
            });
            let mut fins = c::panel([l * 0.05, y + r * 0.8, 0.0], [-0.35, 1.0, 0.0], r * 1.5 * df, l * 0.16, l * 0.04, l * 0.05, 0.08);
            geo::merge(&mut fins, &c::panel([-l * 0.46, y, 0.0], [-0.55, 0.85, 0.0001], l * 0.25, l * 0.08, l * 0.04, 0.0, 0.08));
            geo::merge(&mut fins, &c::panel([-l * 0.46, y, 0.0], [-0.5, -0.85, 0.0001], l * 0.12, l * 0.06, l * 0.03, 0.0, 0.08));
            geo::merge(&mut fins, &c::panels([l * 0.18, y - r * 0.6, r * 0.6], [-0.4, -0.35, 1.0], r * 1.8, [l * 0.12, l * 0.04], l * 0.03, 0.08));
            let mut gills = Mesh::default();
            for i in 0..5 {
                for z in [-1.0f32, 1.0] {
                    geo::merge(&mut gills, &b::cbox([l * 0.006, r * 0.6, r * 0.05], [l * 0.25 - i as f32 * l * 0.02, y, z * r * 0.85]));
                }
            }
            let mut eyes = Mesh::default();
            for z in [-1.0f32, 1.0] {
                geo::merge(&mut eyes, &c::ball([r * 0.07; 3], [l * 0.37, y + r * 0.2, z * r * 0.52]));
            }
            geo::merge(&mut gills, &eyes);
            let belly = c::loft_x(-l * 0.3, l * 0.42, 10, |_| y - r * 0.35, |t| c::ellipse(r * 0.8 * c::streamline(t, 0.3, 0.4, 0.3), r * 0.62 * c::streamline(t, 0.3, 0.4, 0.3), 16));
            c::stand(vec![Part::new(body, v.color("color")).material("ground"), Part::new(belly, [0.78, 0.78, 0.76]).material("ground"), Part::new(fins, v.color("color")).material("ground"), Part::new(gills, DARK).material("ground")])
        });

    basic!(_r, "animals-whales", "animals", "whales", "Whales", "ground",
        [P::num("length", 14.0, 5.0, 28.0).unit("m").describe("Length (m)"), P::num("girth", 1.0, 0.7, 1.4).describe("Girth"), P::num("flukes", 1.0, 0.6, 1.6).describe("Fluke span"), P::color("color", [0.14, 0.16, 0.19]).describe("Skin colour")],
        |v: &Values, _s: u32| {
            let (l, g, fk) = (v.f("length"), v.f("girth"), v.f("flukes"));
            let r = l * 0.1 * g;
            let y = r;
            // A massive rounded head tapering to the tail stock, flukes level.
            let body = c::loft_x(-l * 0.5, l * 0.5, 24, |_| y, |t| {
                let k = if t > 0.9 { (1.0 - ((t - 0.9) / 0.1).powi(2)).max(0.0).sqrt() } else { 0.12 + 0.88 * ((t / 0.9).powf(0.8) * sh::PI * 0.5).sin() };
                c::ellipse(r * 0.95 * k, r * k, 20)
            });
            let mut fins = c::panels([-l * 0.48, y, 0.0], [-0.3, 0.0, 1.0], l * 0.13 * fk, [l * 0.07, l * 0.03], l * 0.03, 0.08);
            geo::merge(&mut fins, &c::panels([l * 0.18, y - r * 0.5, r * 0.8], [-0.4, -0.4, 1.0], l * 0.12, [l * 0.06, l * 0.02], l * 0.03, 0.1));
            geo::merge(&mut fins, &c::panel([-l * 0.22, y + r * 0.8, 0.0], [-0.6, 0.8, 0.0], r * 0.35, l * 0.05, l * 0.01, l * 0.02, 0.1));
            let mut eyes = Mesh::default();
            for z in [-1.0f32, 1.0] {
                geo::merge(&mut eyes, &c::ball([r * 0.05; 3], [l * 0.3, y - r * 0.25, z * r * 0.88]));
            }
            let belly = c::loft_x(-l * 0.1, l * 0.44, 10, |_| y - r * 0.38, |t| c::ellipse(r * 0.82 * c::streamline(t, 0.3, 0.3, 0.3), r * 0.62 * c::streamline(t, 0.3, 0.3, 0.3), 16));
            c::stand(vec![Part::new(body, v.color("color")).material("ground"), Part::new(belly, [0.6, 0.6, 0.58]).material("ground"), Part::new(fins, v.color("color")).material("ground"), Part::new(eyes, DARK).material("glass")])
        });

    basic!(_r, "animals-insects", "animals", "insects", "Insects", "ground",
        [P::num("size", 0.03, 0.005, 0.12).unit("m").describe("Body length (m)"), P::num("legs", 1.0, 0.5, 2.0).describe("Leg length"), P::num("wings", 1.0, 0.0, 1.6).describe("Wing size (0 none)"), P::color("color", [0.12, 0.2, 0.1]).describe("Shell colour")],
        |v: &Values, _s: u32| {
            let (l, lg, wg) = (v.f("size"), v.f("legs"), v.f("wings"));
            let y = l * 0.35 * lg;
            // Head, thorax and abdomen; six jointed legs; antennae; wings.
            let mut body = c::ball([l * 0.12, l * 0.1, l * 0.1], [l * 0.36, y + l * 0.02, 0.0]);
            geo::merge(&mut body, &c::ball([l * 0.15, l * 0.12, l * 0.12], [l * 0.12, y, 0.0]));
            geo::merge(&mut body, &c::ball([l * 0.32, l * 0.15, l * 0.17], [-l * 0.25, y, 0.0]));
            let mut legs = Mesh::default();
            for k in 0..3 {
                let x = l * (0.2 - 0.1 * k as f32);
                for z in [-1.0f32, 1.0] {
                    let hip = [x, y - l * 0.03, z * l * 0.08];
                    let knee = [x + l * (0.12 - 0.12 * k as f32), y + l * 0.15 * lg, z * l * 0.35 * lg];
                    let foot = [x + l * (0.2 - 0.2 * k as f32) * lg, 0.0, z * l * 0.55 * lg];
                    geo::merge(&mut legs, &c::chain(&[(hip, l * 0.02), (knee, l * 0.016), (foot, l * 0.01)], 6));
                }
            }
            for z in [-1.0f32, 1.0] {
                let path: Vec<[f32; 3]> = (0..6).map(|i| {
                    let t = i as f32 / 5.0;
                    [l * 0.45 + l * 0.3 * t, y + l * 0.06 + l * 0.25 * t, z * (l * 0.04 + l * 0.15 * t)]
                }).collect();
                geo::merge(&mut legs, &sh::tube(&path, |_| l * 0.008, 5));
            }
            let mut wings = Mesh::default();
            if wg > 0.01 {
                for z in [-1.0f32, 1.0] {
                    geo::merge(&mut wings, &c::panel([l * 0.12, y + l * 0.12, z * l * 0.05], [-0.7, 0.15, z * 0.7], l * 0.55 * wg, l * 0.2 * wg, l * 0.14 * wg, 0.0, 0.03));
                }
            }
            c::stand(vec![Part::new(body, v.color("color")).material("accent"), Part::new(legs, DARK).material("ground"), Part::new(wings, [0.8, 0.85, 0.9]).alpha(0.45).material("glass")])
        });

    basic!(_r, "animals-fantasy-creatures", "animals", "fantasy-creatures", "Fantasy creatures", "ground",
        [P::num("size", 1.0, 0.5, 3.0).describe("Size (1 a 4 m dragon)"), P::num("wings", 1.0, 0.0, 1.6).describe("Wingspan (0 none)"), P::num("horns", 0.35, 0.0, 0.8).unit("m").describe("Horn length (m)"), P::color("color", [0.18, 0.3, 0.12]).describe("Scale colour"), P::color("wing", [0.4, 0.12, 0.08]).describe("Wing membrane colour")],
        |v: &Values, _s: u32| {
            let (wg, hl) = (v.f("wings"), v.f("horns"));
            // A dragon on the four-legged template: a long neck and tail, horns,
            // a row of spines and two bat wings of finger bones and membrane.
            let q = c::Quad { len: 2.2, girth: 0.42, leg: 0.75, leg_r: 0.09, neck: 1.1, neck_up: 0.75, head: 0.6, snout: 0.7, tail: 2.2, tail_up: 0.35, ears: 0.0 };
            let body = c::quadruped(&q);
            let mut horns = Mesh::default();
            let mut spines = Mesh::default();
            if hl > 0.01 {
                for side in [-1.0f32, 1.0] {
                    let base = [body.crown[0] - 0.05, body.crown[1], side * 0.12];
                    geo::merge(&mut horns, &c::cone(base, [base[0] - hl * 0.8, base[1] + hl * 0.55, base[2] + side * hl * 0.15], 0.05, 8));
                }
            }
            for i in 0..9 {
                let t = i as f32 / 8.0;
                let p = [body.withers[0] - (body.withers[0] - body.rump[0]) * t + 0.3, body.withers[1] - 0.04, 0.0];
                geo::merge(&mut spines, &c::cone(p, [p[0] - 0.1, p[1] + 0.2, 0.0], 0.06, 6));
            }
            let (mut bones, mut membrane) = (Mesh::default(), Mesh::default());
            if wg > 0.01 {
                let span = 2.6 * wg;
                for side in [-1.0f32, 1.0] {
                    let sh_ = [body.withers[0] - 0.1, body.withers[1] - 0.05, side * q.girth * 0.6];
                    let wrist = [sh_[0] + 0.2, sh_[1] + span * 0.35, sh_[2] + side * span * 0.45];
                    geo::merge(&mut bones, &c::limb(sh_, wrist, 0.06, 0.04, 8));
                    let tips: Vec<[f32; 3]> = (0..4).map(|k| {
                        let a = k as f32 / 3.0;
                        [wrist[0] - span * (0.1 + 0.55 * a), wrist[1] + span * (0.15 - 0.45 * a), wrist[2] + side * span * (0.5 - 0.25 * a)]
                    }).collect();
                    let mut prev = sh_;
                    for tip in tips.iter().chain([[sh_[0] - 0.9, sh_[1] - 0.1, sh_[2]]].iter()) {
                        geo::merge(&mut bones, &c::limb(wrist, *tip, 0.03, 0.012, 6));
                        let mut bl = geo::MeshBuilder::new();
                        for p in [wrist, prev, *tip] {
                            bl.vertex(p, [0.0, 1.0, 0.0]);
                        }
                        bl.tri(0, 1, 2);
                        let mut m = bl.finish();
                        geo::smooth_normals(&mut m);
                        sh::double_sided(&mut m);
                        geo::merge(&mut membrane, &m);
                        prev = *tip;
                    }
                }
            }
            geo::merge(&mut bones, &spines);
            animal(&q, v.color("color"), [0.1, 0.1, 0.08], vec![Part::new(horns, HORN).material("ground"), Part::new(bones, v.color("color")).material("ground"), Part::new(membrane, v.color("wing")).material("ground")], v.f("size"))
        });
}
