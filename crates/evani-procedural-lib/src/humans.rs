//! Category 18, Humans and character anatomy: basic generators (batch C).
//! One shared body template from the base (`craft::Human`: a joint frame
//! whose height, bulk, leg share and pose are params, and its torso, limb,
//! hand, foot and head pieces), a head shaped from an ellipsoid with its
//! features, the frame's bone list as a debug view, and the face's morph
//! targets as data blended live. Faces +z. Inspired by three.js (MIT)
//! SkinnedMesh/Bone, morph targets and capsule limbs: one base body moved
//! by proportion params; written fresh here.

use evani_procedural::Registry;

#[allow(unused_imports, dead_code)]
pub fn register(_r: &mut Registry) {
    use evani_procedural::{Output, ParamSpec as P, Part, Values, build as b, craft as c, geo, shapes as sh};
    type Mesh = geo::Mesh;
    const SKIN: [f32; 3] = [0.62, 0.42, 0.32];
    const WHITE: [f32; 3] = [0.85, 0.85, 0.82];
    const DARK: [f32; 3] = [0.04, 0.03, 0.03];
    const LIP: [f32; 3] = [0.5, 0.22, 0.2];
    const HAIR: [f32; 3] = [0.12, 0.07, 0.04];

    fn moved(mut m: Mesh, d: [f32; 3]) -> Mesh {
        geo::transform(&mut m, [1.0; 3], d);
        m
    }

    fn rot_x(v: [f32; 3], a: f32) -> [f32; 3] {
        let (s, co) = a.sin_cos();
        [v[0], v[1] * co - v[2] * s, v[1] * s + v[2] * co]
    }

    /// Only the triangles of `m` whose three corners pass `keep`.
    fn keep(m: &Mesh, keep: impl Fn([f32; 3]) -> bool) -> Mesh {
        let p = |i: u32| m.vertices.get(i as usize).map_or([0.0; 3], |v| [v[0], v[1], v[2]]);
        let idx: Vec<u32> = m.indices.as_chunks::<3>().0.iter().filter(|t| t.iter().all(|&i| keep(p(i)))).flatten().copied().collect();
        Mesh { vertices: m.vertices.clone(), indices: idx }
    }

    /// A head of half height `hs` round the origin: an ellipsoid `width`
    /// wide, the jaw narrowed by `jaw` (1 average) to a chin.
    fn skull(hs: f32, width: f32, jaw: f32) -> Mesh {
        let mut m = sh::icosphere(3);
        for v in &mut m.vertices {
            let (x, y, z) = (v[0], v[1], v[2]);
            let low = (-y - 0.1).max(0.0);
            let k = 1.0 - low * (0.55 - 0.25 * jaw);
            *v = [x * hs * 0.78 * width * k, y * hs * (1.0 + 0.08 * low), z * hs * 0.9 * (1.0 - 0.15 * low) + hs * 0.08 * low, 0.0, 1.0, 0.0];
        }
        geo::smooth_normals(&mut m);
        m
    }

    /// The face's features on a head of half height `hs`, `width` wide:
    /// (eye whites, irises, nose, ears, brows, lips).
    fn features(hs: f32, width: f32, nose: f32) -> [Mesh; 6] {
        let (mut white, mut iris, mut ears, mut brows) = (Mesh::default(), Mesh::default(), Mesh::default(), Mesh::default());
        for s in [-1.0f32, 1.0] {
            let e = [s * hs * 0.3 * width, hs * 0.12, hs * 0.72];
            geo::merge(&mut white, &c::ball([hs * 0.13; 3], e));
            geo::merge(&mut iris, &c::ball([hs * 0.07, hs * 0.07, hs * 0.03], [e[0], e[1], e[2] + hs * 0.115]));
            geo::merge(&mut ears, &c::ball([hs * 0.07, hs * 0.26, hs * 0.16], [s * hs * 0.78 * width, hs * 0.02, -hs * 0.05]));
            geo::merge(&mut brows, &b::bar([s * hs * 0.12, hs * 0.32, hs * 0.82], [s * hs * 0.48 * width, hs * 0.34, hs * 0.72], hs * 0.06, hs * 0.04));
        }
        let mut nose_m = c::limb([0.0, hs * 0.1, hs * 0.86], [0.0, -hs * 0.2, hs * (0.92 + nose)], hs * 0.07, hs * 0.11, 10);
        geo::merge(&mut nose_m, &c::ball([hs * 0.17, hs * 0.08, hs * 0.1], [0.0, -hs * 0.24, hs * (0.86 + nose * 0.8)]));
        let lips = c::limb([-hs * 0.24, -hs * 0.48, hs * 0.8], [hs * 0.24, -hs * 0.48, hs * 0.8], hs * 0.05, hs * 0.05, 10);
        [white, iris, nose_m, ears, brows, lips]
    }

    /// A bust under a head of half height `hs`: neck and shoulders to a flat base.
    fn bust(hs: f32) -> Mesh {
        let st = vec![
            c::Station::y(0.0, -hs * 3.2, 0.0, c::squircle(hs * 2.2, hs * 1.05, 2.6, 24)),
            c::Station::y(0.0, -hs * 2.4, 0.0, c::squircle(hs * 2.15, hs * 1.0, 2.6, 24)),
            c::Station::y(0.0, -hs * 1.85, 0.0, c::squircle(hs * 1.1, hs * 0.7, 2.4, 24)),
            c::Station::y(0.0, -hs * 1.5, 0.0, c::squircle(hs * 0.48, hs * 0.48, 2.0, 24)),
            c::Station::y(0.0, -hs * 0.5, -hs * 0.1, c::squircle(hs * 0.44, hs * 0.44, 2.0, 24)),
        ];
        c::loft(&st, true, true)
    }

    /// A finger from `base` heading `dir`, bending forward (+z) by `bend`
    /// at each joint: (finger, nail).
    fn finger(base: [f32; 3], dir: [f32; 3], len: f32, r: f32, bend: f32) -> (Mesh, Mesh) {
        let mut pts = vec![(base, r)];
        let (mut p, mut d) = (base, dir);
        for (k, share) in [0.45f32, 0.3, 0.25].into_iter().enumerate() {
            d = rot_x(d, bend * [0.8, 1.1, 0.7][k]);
            p = [p[0] + d[0] * len * share, p[1] + d[1] * len * share, p[2] + d[2] * len * share];
            pts.push((p, r * (0.92 - 0.1 * k as f32)));
        }
        let m = c::chain(&pts, 10);
        let n = pts.len();
        let (tip, prev) = (pts[n - 1].0, pts[n - 2].0);
        let nail_at = c::lerp3(prev, tip, 0.6);
        let out = rot_x([0.0, 0.0, -1.0], bend * 2.6);
        let nail = c::ball_along([len * 0.1, r * 0.25, r * 0.7], [nail_at[0] + out[0] * r * 0.75, nail_at[1] + out[1] * r * 0.75, nail_at[2] + out[2] * r * 0.75], [tip[0] - prev[0], tip[1] - prev[1], tip[2] - prev[2]]);
        (m, nail)
    }

    basic!(_r, "humans-base-human-bodies", "humans", "base-human-bodies", "Base human bodies", "ground",
        [P::num("height", 1.75, 1.45, 2.05).unit("m").describe("Height (m)"), P::num("bulk", 1.0, 0.7, 1.5).describe("Build (1 average)"), P::num("legs", 0.5, 0.44, 0.56).describe("Leg length (share of the height)"), P::color("color", SKIN).describe("Skin colour")],
        |v: &Values, _s: u32| {
            let h = c::Human { height: v.f("height"), bulk: v.f("bulk"), legs: v.f("legs"), ..c::Human::default() };
            let body = c::body_mesh(&c::body(&h, 0.0));
            let hs = h.height * 0.062;
            let j = h.frame();
            let head_c = [0.0, j[c::joint::HEAD][1] + hs * 0.15, j[c::joint::HEAD][2]];
            let mut eyes = Mesh::default();
            for s in [-1.0f32, 1.0] {
                geo::merge(&mut eyes, &c::ball([hs * 0.1; 3], [head_c[0] + s * hs * 0.3, head_c[1] + hs * 0.1, head_c[2] + hs * 0.82]));
            }
            Output::Parts(vec![Part::new(body, v.color("color")).material("ground"), Part::new(eyes, DARK)])
        });

    basic!(_r, "humans-procedural-heads", "humans", "procedural-heads", "Procedural heads", "ground",
        [P::num("width", 1.0, 0.82, 1.2).describe("Head width"), P::num("jaw", 1.0, 0.6, 1.5).describe("Jaw width"), P::num("nose", 0.12, 0.04, 0.3).describe("Nose length (share of the head)"), P::color("color", SKIN).describe("Skin colour"), P::color("eyes", [0.2, 0.35, 0.5]).describe("Eye colour")],
        |v: &Values, _s: u32| {
            let (w, jaw, nose) = (v.f("width"), v.f("jaw"), v.f("nose"));
            let hs = 0.12;
            let mut skin = skull(hs, w, jaw);
            let [white, iris, nose_m, ears, brows, lips] = features(hs, w, nose);
            geo::merge(&mut skin, &nose_m);
            geo::merge(&mut skin, &ears);
            geo::merge(&mut skin, &bust(hs));
            c::stand(vec![
                Part::new(skin, v.color("color")).material("ground"),
                Part::new(white, WHITE).material("ground"),
                Part::new(iris, v.color("eyes")).material("glass"),
                Part::new(brows, HAIR).material("ground"),
                Part::new(lips, LIP).material("ground"),
            ])
        });

    basic!(_r, "humans-facial-structures", "humans", "facial-structures", "Facial structures", "ground",
        [P::num("cheeks", 1.0, 0.0, 2.0).describe("Cheekbone prominence"), P::num("brow", 1.0, 0.0, 2.0).describe("Brow ridge"), P::num("chin", 1.0, 0.3, 2.0).describe("Chin prominence"), P::color("color", [0.58, 0.4, 0.3]).describe("Skin colour")],
        |v: &Values, _s: u32| {
            let (ch, br, chin) = (v.f("cheeks"), v.f("brow"), v.f("chin"));
            let hs = 0.12;
            // The head's planes and landmarks pushed out as params say.
            let mut skull_m = sh::icosphere(3);
            for vt in &mut skull_m.vertices {
                let (x, y, z) = (vt[0], vt[1], vt[2]);
                let bell = |a: f32, w: f32| (-(a / w) * (a / w)).exp();
                let low = (-y - 0.1).max(0.0);
                let k = 1.0 - low * 0.35;
                let front = z.max(0.0);
                let cheek = ch * 0.1 * bell(x.abs() - 0.55, 0.18) * bell(y + 0.05, 0.15) * front;
                let ridge = br * 0.08 * bell(y - 0.32, 0.08) * bell(x.abs() - 0.3, 0.3) * front;
                let ch_k = chin * 0.12 * bell(x, 0.2) * bell(y + 0.88, 0.14) * front;
                let r = 1.0 + cheek + ridge + ch_k;
                *vt = [x * hs * 0.78 * k * r, y * hs * (1.0 + 0.08 * low), z * hs * 0.9 * (1.0 - 0.15 * low) * r + hs * 0.08 * low, 0.0, 1.0, 0.0];
            }
            geo::smooth_normals(&mut skull_m);
            sh::flat_shade(&mut skull_m);
            let [white, iris, nose_m, ears, _, _] = features(hs, 1.0, 0.1);
            let mut skin = skull_m;
            geo::merge(&mut skin, &nose_m);
            geo::merge(&mut skin, &ears);
            geo::merge(&mut skin, &bust(hs));
            let mut eyes = white;
            geo::merge(&mut eyes, &iris);
            c::stand(vec![Part::new(skin, v.color("color")).material("ground"), Part::new(eyes, [0.3, 0.28, 0.26]).material("ground")])
        });

    basic!(_r, "humans-hairstyles", "humans", "hairstyles", "Hairstyles", "ground",
        [P::num("length", 0.15, 0.0, 0.5).unit("m").describe("Hair length (m)"), P::num("volume", 1.0, 0.5, 2.0).describe("Volume"), P::num("parting", 0.3, -1.0, 1.0).describe("Parting (left to right)"), P::color("color", HAIR).describe("Hair colour"), P::color("skin", SKIN).describe("Skin colour")],
        |v: &Values, s: u32| {
            let (len, vol, part) = (v.f("length"), v.f("volume"), v.f("parting"));
            let hs = 0.12;
            let mut skin = skull(hs, 1.0, 1.0);
            let [white, iris, nose_m, ears, brows, lips] = features(hs, 1.0, 0.12);
            geo::merge(&mut skin, &nose_m);
            geo::merge(&mut skin, &ears);
            geo::merge(&mut skin, &bust(hs));
            // A cap over the scalp, its rim at the hairline (low at the back).
            let mut cap = sh::icosphere(3);
            let g = 1.0 + 0.05 * vol;
            for vt in &mut cap.vertices {
                let (x, y, z) = (vt[0], vt[1], vt[2]);
                let line = if z > 0.0 { 0.45 - 0.2 * x.abs() } else { -0.25 - 0.3 * (-z) };
                let yy = y.max(line);
                let puff = 1.0 + 0.04 * vol * (1.0 - (x - part * 0.3).abs()).max(0.0);
                *vt = [x * hs * 0.8 * g, yy * hs * 1.02 * g * puff + hs * 0.02, z * hs * 0.93 * g, 0.0, 1.0, 0.0];
            }
            geo::smooth_normals(&mut cap);
            // Locks hanging from the back and sides, seeded.
            if len > 0.01 {
                for i in 0..48 {
                    let a = sh::PI * 0.15 + sh::range(s, i, 1, 0.0, sh::PI * 1.7);
                    let (x, z) = (a.cos() * hs * 0.8 * g, -a.sin().abs() * hs * 0.9 * g * if a.sin() > 0.0 { 1.0 } else { 0.2 } - hs * 0.05);
                    let top = [x, hs * 0.2, z];
                    let out = [x * 0.15, 0.0, z * 0.15];
                    let path: Vec<[f32; 3]> = (0..6).map(|k| {
                        let t = k as f32 / 5.0;
                        [top[0] + out[0] * t * vol, top[1] - (len + hs * 0.3) * t, top[2] + out[2] * t * vol]
                    }).collect();
                    geo::merge(&mut cap, &sh::tube(&path, |t| hs * 0.09 * vol * (1.0 - t * 0.6), 6));
                }
            }
            c::stand(vec![
                Part::new(skin, v.color("skin")).material("ground"),
                Part::new(cap, v.color("color")).material("ground"),
                Part::new(white, WHITE).material("ground"),
                Part::new(iris, [0.15, 0.1, 0.05]).material("glass"),
                Part::new(brows, v.color("color")).material("ground"),
                Part::new(lips, LIP).material("ground"),
            ])
        });

    basic!(_r, "humans-eyes", "humans", "eyes", "Eyes", "ground",
        [P::num("size", 0.012, 0.009, 0.016).unit("m").describe("Eyeball radius (m)"), P::num("iris", 0.45, 0.3, 0.6).describe("Iris size (share of the eye)"), P::num("open", 0.7, 0.0, 1.0).describe("Lids open"), P::color("color", [0.18, 0.38, 0.55]).describe("Iris colour")],
        |v: &Values, _s: u32| {
            let (r, ir, open) = (v.f("size"), v.f("iris"), v.f("open"));
            let c0 = [0.0, r * 1.1, 0.0];
            let white = c::ball([r; 3], c0);
            // The iris disc and pupil just in front, the cornea a clear bulge.
            let iris = c::ball([r * ir, r * ir, r * 0.1], [c0[0], c0[1], c0[2] + r * 0.93]);
            let pupil = c::ball([r * ir * 0.42, r * ir * 0.42, r * 0.06], [c0[0], c0[1], c0[2] + r * 1.0]);
            let cornea = c::ball([r * ir * 1.1, r * ir * 1.1, r * 0.3], [c0[0], c0[1], c0[2] + r * 0.9]);
            // Upper and lower lids: shells over the ball, parting as it opens.
            let shell = c::ball([r * 1.08; 3], c0);
            let gap = r * (0.05 + 0.95 * open);
            let mut lids = keep(&shell, |p| p[1] - c0[1] > gap * 0.75 || p[1] - c0[1] < -gap * 0.55 || p[2] < c0[2] - r * 0.2);
            sh::double_sided(&mut lids);
            Output::Parts(vec![
                Part::new(white, WHITE).material("ground"),
                Part::new(iris, v.color("color")).material("ground"),
                Part::new(pupil, DARK).material("ground"),
                Part::new(cornea, [0.9, 0.9, 0.95]).alpha(0.3).material("glass"),
                Part::new(lids, SKIN).material("ground"),
            ])
        });

    basic!(_r, "humans-noses", "humans", "noses", "Noses", "ground",
        [P::num("length", 0.05, 0.035, 0.075).unit("m").describe("Length (m)"), P::num("width", 1.0, 0.7, 1.5).describe("Width at the nostrils"), P::num("bridge", 0.5, 0.0, 1.0).describe("Bridge height"), P::color("color", SKIN).describe("Skin colour")],
        |v: &Values, _s: u32| {
            let (l, w, br) = (v.f("length"), v.f("width"), v.f("bridge"));
            let hs = 0.12;
            // A face mask (the head's front) with the nose lofted down from the bridge.
            let mut face = keep(&skull(hs, 1.0, 1.0), |p| p[2] > hs * 0.15);
            sh::double_sided(&mut face);
            let st: Vec<c::Station> = (0..8).map(|i| {
                let t = i as f32 / 7.0;
                let half = hs * (0.06 + 0.12 * w * t.powf(1.5));
                let proud = hs * 0.04 + (l * (0.25 + 0.75 * t)) * (0.6 + 0.4 * br * (1.0 - t)) ;
                let y = hs * 0.15 - l * 1.1 * t;
                c::Station { at: [0.0, y, hs * 0.8], u: [1.0, 0.0, 0.0], v: [0.0, 0.0, 1.0], profile: (0..12).map(|k| {
                    let a = k as f32 / 12.0 * sh::TAU;
                    [half * a.cos(), (a.sin() * 0.5 + 0.5) * proud]
                }).collect() }
            }).collect();
            let mut skin = c::loft(&st, true, true);
            let tip_y = hs * 0.15 - l * 1.1;
            for s in [-1.0f32, 1.0] {
                geo::merge(&mut skin, &c::ball([hs * 0.08 * w, hs * 0.07, hs * 0.07], [s * hs * 0.1 * w, tip_y + hs * 0.02, hs * 0.86 + l * 0.6]));
            }
            geo::merge(&mut skin, &face);
            let mut nostrils = Mesh::default();
            for s in [-1.0f32, 1.0] {
                geo::merge(&mut nostrils, &c::ball([hs * 0.04 * w, hs * 0.015, hs * 0.03], [s * hs * 0.07 * w, tip_y - hs * 0.045, hs * 0.86 + l * 0.65]));
            }
            c::stand(vec![Part::new(skin, v.color("color")).material("ground"), Part::new(nostrils, DARK)])
        });

    basic!(_r, "humans-ears", "humans", "ears", "Ears", "ground",
        [P::num("size", 0.065, 0.05, 0.08).unit("m").describe("Height (m)"), P::num("lobe", 1.0, 0.0, 2.0).describe("Lobe size"), P::num("out", 20.0, 0.0, 45.0).unit("deg").describe("Angle out from the head (degrees)"), P::color("color", SKIN).describe("Skin colour")],
        |v: &Values, _s: u32| {
            let (h, lobe, out) = (v.f("size"), v.f("lobe"), v.f("out").to_radians());
            // In the xy plane facing +z: a flat shell, the helix rim curling
            // round the top and back, the bowl (concha) and the lobe.
            let base = c::ball([h * 0.3, h * 0.48, h * 0.06], [0.0, h * 0.5, 0.0]);
            let helix: Vec<[f32; 3]> = (0..=16).map(|k| {
                let a = sh::PI * 0.75 - k as f32 / 16.0 * sh::PI * 1.45;
                [h * 0.3 * a.cos() + h * 0.02, h * 0.55 + h * 0.42 * a.sin(), h * 0.05]
            }).collect();
            let mut skin = sh::tube(&helix, |t| h * 0.055 * (1.0 - 0.4 * (2.0 * t - 1.0).abs()), 10);
            geo::merge(&mut skin, &base);
            geo::merge(&mut skin, &c::ball([h * 0.14 * (0.4 + 0.6 * lobe), h * 0.12 * (0.4 + 0.6 * lobe), h * 0.05], [h * 0.02, h * 0.08, 0.01]));
            geo::merge(&mut skin, &c::ball([h * 0.05, h * 0.08, h * 0.05], [-h * 0.2, h * 0.5, h * 0.05]));
            let bowl = c::ball([h * 0.12, h * 0.14, h * 0.03], [-h * 0.02, h * 0.42, h * 0.045]);
            let ps = vec![Part::new(skin, v.color("color")).material("ground"), Part::new(bowl, [0.4, 0.22, 0.18]).material("ground")];
            let mut ps2 = Vec::new();
            for p in ps {
                let m = sh::place(&p.mesh, 1.0, -out, 0.0, [0.0; 3]);
                ps2.push(Part { mesh: m, ..p });
            }
            c::stand(ps2)
        });

    basic!(_r, "humans-mouths-and-lips", "humans", "mouths-and-lips", "Mouths and lips", "ground",
        [P::num("smile", 0.3, -1.0, 1.0).describe("Smile (-1 a frown)"), P::num("fullness", 1.0, 0.5, 1.8).describe("Lip fullness"), P::num("open", 0.0, 0.0, 1.0).describe("Mouth open"), P::color("color", LIP).describe("Lip colour")],
        |v: &Values, _s: u32| {
            let (sm, full, op) = (v.f("smile"), v.f("fullness"), v.f("open"));
            let hs = 0.12;
            let mut face = keep(&skull(hs, 1.0, 1.0), |p| p[2] > hs * 0.1 && p[1] < hs * 0.05);
            sh::double_sided(&mut face);
            // Each lip a tube across the mouth, fuller in the middle; the
            // corners lift with the smile, the lower lip drops as it opens.
            let lip = |lower: bool| {
                let path: Vec<[f32; 3]> = (0..13).map(|k| {
                    let x = (k as f32 / 12.0 - 0.5) * hs * 0.6;
                    let u = x / (hs * 0.3);
                    let corner = sm * hs * 0.1 * u * u;
                    let y = -hs * 0.48 + corner + if lower { -hs * 0.05 - op * hs * 0.18 * (1.0 - u * u) } else { hs * 0.03 };
                    let z = hs * 0.86 * (1.0 - u * u * 0.18);
                    [x, y, z]
                }).collect();
                sh::tube(&path, |t| hs * 0.045 * full * (0.35 + 0.65 * (t * sh::PI).sin()), 10)
            };
            let mut lips = lip(false);
            geo::merge(&mut lips, &lip(true));
            let inside = c::ball([hs * 0.26, hs * (0.02 + 0.09 * op), hs * 0.06], [0.0, -hs * 0.48 - op * hs * 0.08, hs * 0.78]);
            let mut teeth = Mesh::default();
            if op > 0.05 {
                geo::merge(&mut teeth, &b::cbox([hs * 0.32, hs * 0.05, hs * 0.04], [0.0, -hs * 0.46, hs * 0.8]));
            }
            c::stand(vec![Part::new(face, SKIN).material("ground"), Part::new(lips, v.color("color")).material("ground"), Part::new(inside, [0.15, 0.03, 0.03]), Part::new(teeth, WHITE).material("ground")])
        });

    basic!(_r, "humans-hands", "humans", "hands", "Hands", "ground",
        [P::num("curl", 0.2, 0.0, 1.4).describe("Finger curl"), P::num("spread", 0.3, 0.0, 1.0).describe("Finger spread"), P::num("size", 1.0, 0.7, 1.3).describe("Size"), P::color("color", SKIN).describe("Skin colour")],
        |v: &Values, _s: u32| {
            let (curl, spread, k) = (v.f("curl"), v.f("spread"), v.f("size"));
            // Upright, the palm facing +z: wrist, palm, four fingers and a thumb.
            let mut skin = c::limb([0.0, 0.0, 0.0], [0.0, 0.05, 0.0], 0.026, 0.028, 12);
            geo::transform(&mut skin, [1.25, 1.0, 0.8], [0.0; 3]);
            geo::merge(&mut skin, &b::rounded_box([0.085, 0.095, 0.03], 0.013, 3, [0.0, 0.04, 0.0]));
            let mut nails = Mesh::default();
            let lens = [0.07, 0.08, 0.076, 0.06];
            for (i, l) in lens.into_iter().enumerate() {
                let x = (i as f32 - 1.5) * 0.021;
                let a = (i as f32 - 1.5) * spread * 0.18;
                let (f, n) = finger([x, 0.13, 0.0], [a.sin(), a.cos(), 0.0], l, 0.0095 - 0.0005 * (i as f32 - 1.5).abs(), curl);
                geo::merge(&mut skin, &f);
                geo::merge(&mut nails, &n);
            }
            let ta = 0.7 + spread * 0.4;
            let (t, tn) = finger([0.038, 0.055, 0.008], c::lerp3([ta.sin(), ta.cos(), 0.0], [0.0, 0.0, 1.0], 0.3), 0.07, 0.012, curl * 0.6);
            geo::merge(&mut skin, &t);
            geo::merge(&mut nails, &tn);
            let mut ps = vec![Part::new(skin, v.color("color")).material("ground"), Part::new(nails, [0.75, 0.55, 0.5]).material("ground")];
            c::scale_parts(&mut ps, k);
            c::stand(ps)
        });

    basic!(_r, "humans-feet", "humans", "feet", "Feet", "ground",
        [P::num("length", 0.26, 0.2, 0.32).unit("m").describe("Length (m)"), P::num("width", 1.0, 0.75, 1.3).describe("Width"), P::num("ankle", 0.08, 0.03, 0.18).unit("m").describe("Ankle shown above the foot (m)"), P::color("color", SKIN).describe("Skin colour")],
        |v: &Values, _s: u32| {
            let (l, w, an) = (v.f("length"), v.f("width"), v.f("ankle"));
            // Sections from heel to toes along +z; five toes at the front.
            let st: Vec<c::Station> = (0..9).map(|i| {
                let t = i as f32 / 8.0;
                let half_w = l * w * (0.13 + 0.07 * (t * 1.2).min(1.0)) * if t > 0.9 { 1.0 - (t - 0.9) * 3.0 } else { 1.0 };
                let h = l * (0.28 - 0.18 * t);
                c::Station { at: [0.0, h * 0.5, l * t * 0.85], u: [1.0, 0.0, 0.0], v: [0.0, 1.0, 0.0], profile: c::squircle(half_w, h * 0.5, 2.6, 18) }
            }).collect();
            let mut skin = c::loft(&st, true, true);
            geo::merge(&mut skin, &c::limb([0.0, l * 0.18, l * 0.12], [0.0, l * 0.22 + an, l * 0.1], l * 0.13, l * 0.12, 12));
            let mut nails = Mesh::default();
            for k in 0..5 {
                let x = (k as f32 - 1.6) * l * w * 0.068;
                let r = l * (0.05 - 0.006 * k as f32);
                let z = l * (0.88 - 0.02 * k as f32 * k as f32 * 0.5);
                geo::merge(&mut skin, &c::ball([r, r * 0.8, r * 1.2], [-x, r * 0.8, z]));
                geo::merge(&mut nails, &c::ball([r * 0.6, r * 0.2, r * 0.6], [-x, r * 1.5, z + r * 0.4]));
            }
            c::stand(vec![Part::new(skin, v.color("color")).material("ground"), Part::new(nails, [0.75, 0.55, 0.5]).material("ground")])
        });

    basic!(_r, "humans-fingers", "humans", "fingers", "Fingers", "ground",
        [P::num("length", 0.08, 0.05, 0.11).unit("m").describe("Length (m)"), P::num("bend", 0.3, 0.0, 1.5).describe("Bend at each joint"), P::num("thickness", 0.009, 0.006, 0.013).unit("m").describe("Thickness (m)"), P::color("color", SKIN).describe("Skin colour")],
        |v: &Values, _s: u32| {
            let (l, bend, r) = (v.f("length"), v.f("bend"), v.f("thickness"));
            // Upright from its knuckle, with crease rings at the joints.
            let (mut skin, nail) = finger([0.0, r, 0.0], [0.0, 1.0, 0.0], l, r, bend);
            geo::merge(&mut skin, &c::ball([r * 1.25, r * 1.0, r * 1.15], [0.0, r, 0.0]));
            let mut creases = Mesh::default();
            let (mut p, mut d) = ([0.0, r, 0.0], [0.0f32, 1.0, 0.0]);
            for (k, share) in [0.45f32, 0.3].into_iter().enumerate() {
                d = rot_x(d, bend * [0.8, 1.1][k]);
                p = [p[0] + d[0] * l * share, p[1] + d[1] * l * share, p[2] + d[2] * l * share];
                let d2 = rot_x(d, bend * [1.1, 0.7][k] * 0.5);
                let ring: Vec<[f32; 3]> = (0..=12).map(|i| {
                    let a = i as f32 / 12.0 * sh::TAU;
                    let side = [a.cos(), 0.0, a.sin()];
                    let s2 = rot_x(side, bend * 0.5);
                    [p[0] + s2[0] * r * 1.0, p[1] + s2[1] * r * 1.0 + d2[1] * 0.0, p[2] + s2[2] * r * 1.0]
                }).collect();
                geo::merge(&mut creases, &b::sweep(&ring, &b::circle(r * 0.08, 5), false));
            }
            c::stand(vec![Part::new(skin, v.color("color")).material("ground"), Part::new(nail, [0.78, 0.58, 0.52]).material("ground"), Part::new(creases, [0.38, 0.22, 0.17]).material("ground")])
        });

    basic!(_r, "humans-skeletal-rigs", "humans", "skeletal-rigs", "Skeletal rigs", "ground",
        [P::num("height", 1.75, 1.0, 2.2).unit("m").describe("Height (m)"), P::num("arms", 20.0, 0.0, 90.0).unit("deg").describe("Arms out (degrees)"), P::num("knees", 0.0, 0.0, 70.0).unit("deg").describe("Knee bend (degrees)"), P::color("color", [0.75, 0.62, 0.2]).describe("Bone colour")],
        |v: &Values, _s: u32| {
            let h = c::Human { height: v.f("height"), arms: v.f("arms").to_radians(), knees: v.f("knees").to_radians(), ..c::Human::default() };
            // The bone list (craft::HUMAN_BONES) drawn as the usual debug
            // shapes: an octahedron per bone, a ball per joint.
            let j = h.frame();
            let bones = c::bone_shapes(&j, h.height * 0.03);
            let mut joints = Mesh::default();
            for p in j {
                geo::merge(&mut joints, &c::ball([h.height * 0.012; 3], p));
            }
            let ghost = c::body_mesh(&c::body(&h, 0.0));
            Output::Parts(vec![Part::new(bones, v.color("color")).material("ground"), Part::new(joints, [0.1, 0.5, 0.9]).material("glow"), Part::new(ghost, [0.6, 0.62, 0.66]).alpha(0.18)])
        });

    basic!(_r, "humans-muscle-forms", "humans", "muscle-forms", "Muscle forms", "ground",
        [P::num("mass", 1.0, 0.3, 2.0).describe("Muscle mass"), P::num("height", 1.8, 1.5, 2.05).unit("m").describe("Height (m)"), P::num("definition", 0.6, 0.0, 1.0).describe("Definition"), P::color("color", [0.55, 0.16, 0.14]).describe("Muscle colour")],
        |v: &Values, _s: u32| {
            let (mass, ht, def) = (v.f("mass"), v.f("height"), v.f("definition"));
            let h = c::Human { height: ht, bulk: 0.9 + 0.15 * mass, ..c::Human::default() };
            let base = c::body_mesh(&c::body(&h, 0.0));
            let j = h.frame();
            use c::joint::*;
            let k = ht * 0.01 * mass;
            let mut mus = Mesh::default();
            // Each muscle a ball laid along its bone or on the trunk.
            let along = |a: [f32; 3], z: [f32; 3], t: f32, r: [f32; 3]| c::ball_along(r, c::lerp3(a, z, t), [z[0] - a[0], z[1] - a[1], z[2] - a[2]]);
            for s in [1.0f32, -1.0] {
                let (sh_, el, wr, hp, kn, an) = if s > 0.0 { (j[SHOULDER_L], j[ELBOW_L], j[WRIST_L], j[HIP_L], j[KNEE_L], j[ANKLE_L]) } else { (j[SHOULDER_R], j[ELBOW_R], j[WRIST_R], j[HIP_R], j[KNEE_R], j[ANKLE_R]) };
                geo::merge(&mut mus, &c::ball([k * 5.0, k * 4.0, k * 4.5], [sh_[0] * 1.05, sh_[1], 0.0]));
                geo::merge(&mut mus, &along(sh_, el, 0.5, [ht * 0.07, k * 3.2, k * 3.2]));
                geo::merge(&mut mus, &along(el, wr, 0.3, [ht * 0.06, k * 2.6, k * 2.6]));
                geo::merge(&mut mus, &along(hp, kn, 0.45, [ht * 0.11, k * 5.5, k * 5.5]));
                geo::merge(&mut mus, &along(kn, an, 0.3, [ht * 0.07, k * 4.0, k * 4.0]));
                geo::merge(&mut mus, &c::ball([k * 6.5, k * 4.5, k * 2.5], [s * ht * 0.06, j[CHEST][1] + ht * 0.02, ht * 0.06]));
            }
            // An abdominal grid, its relief by the definition.
            let mut abs = Mesh::default();
            for row in 0..3 {
                for s in [-1.0f32, 1.0] {
                    let y = j[CHEST][1] - ht * (0.07 + 0.045 * row as f32);
                    geo::merge(&mut abs, &c::ball([ht * 0.022, ht * 0.019, ht * 0.012 * (0.4 + def)], [s * ht * 0.025, y, ht * 0.07]));
                }
            }
            Output::Parts(vec![Part::new(base, [0.6, 0.44, 0.36]).material("ground"), Part::new(mus, v.color("color")).material("ground"), Part::new(abs, v.color("color")).material("ground")])
        });

    basic!(_r, "humans-facial-expression-morphs", "humans", "facial-expression-morphs", "Facial expression morphs", "ground",
        [P::num("smile", 0.5, -1.0, 1.0).describe("Smile (-1 a frown) weight"), P::num("open", 0.0, 0.0, 1.0).describe("Jaw open weight"), P::num("brows", 0.0, -1.0, 1.0).describe("Brows raised weight"), P::num("blink", 0.0, 0.0, 1.0).describe("Blink weight"), P::color("color", SKIN).describe("Skin colour")],
        |v: &Values, _s: u32| {
            let w = [v.f("smile"), v.f("open"), v.f("brows"), v.f("blink")];
            let hs = 0.12;
            // The morph targets (craft::face_morphs) blended on the head.
            let head = skull(hs, 1.0, 1.0);
            let targets = c::face_morphs(&head, [0.0; 3], hs);
            let mut skin = c::apply_morphs(&head, &targets, &w);
            let [white, iris, nose_m, ears, _, _] = features(hs, 1.0, 0.1);
            geo::merge(&mut skin, &nose_m);
            geo::merge(&mut skin, &ears);
            geo::merge(&mut skin, &bust(hs));
            // The features that ride on the targets: brows, lids, the mouth line.
            let mut brows = Mesh::default();
            for s in [-1.0f32, 1.0] {
                let y = hs * (0.33 + 0.1 * w[2]);
                geo::merge(&mut brows, &b::bar([s * hs * 0.12, y - 0.01 * w[2], hs * 0.86], [s * hs * 0.46, y + hs * 0.02 * w[2], hs * 0.74], hs * 0.06, hs * 0.04));
                let lid_y = hs * (0.27 - 0.15 * w[3]);
                geo::merge(&mut brows, &c::ball([hs * 0.15, (hs * 0.13 - (hs * 0.27 - lid_y)).max(hs * 0.02) * 0.6 + hs * 0.02, hs * 0.1], [s * hs * 0.3, lid_y - hs * 0.04, hs * 0.74]));
            }
            let path: Vec<[f32; 3]> = (0..13).map(|k| {
                let x = (k as f32 / 12.0 - 0.5) * hs * 0.56;
                let u = x / (hs * 0.28);
                [x, -hs * 0.48 + w[0] * hs * 0.12 * u * u - w[1] * hs * 0.05, hs * 0.88 * (1.0 - u * u * 0.15)]
            }).collect();
            let mut mouth = sh::tube(&path, |t| hs * 0.035 * (0.4 + 0.6 * (t * sh::PI).sin()), 8);
            geo::merge(&mut mouth, &c::ball([hs * 0.22, hs * (0.01 + 0.12 * w[1]), hs * 0.05], [0.0, -hs * 0.5 - w[1] * hs * 0.1, hs * 0.82]));
            let mut eyes = white;
            geo::merge(&mut eyes, &iris);
            c::stand(vec![
                Part::new(skin, v.color("color")).material("ground"),
                Part::new(eyes, [0.3, 0.3, 0.32]).material("ground"),
                Part::new(brows, HAIR).material("ground"),
                Part::new(mouth, [0.3, 0.08, 0.07]).material("ground"),
            ])
        });

    // The photoreal line-up (Asset Studio column 22): sculpted bodies of six
    // builds in grey tanks and shorts, its own module (crate::npc).
    basic!(_r, "humans-full-npc-body-variations", "humans", "full-npc-body-variations", "Full NPC body variations", "skin",
        [P::num("count", 6.0, 1.0, 9.0).step(1.0).describe("People in the line-up"), P::num("variety", 0.0, 0.0, 1.0).describe("How much they differ from the six reference builds"), P::num("spacing", 0.73, 0.4, 1.5).unit("m").describe("Distance between people (m)"), P::num("turn", 38.0, -90.0, 90.0).unit("deg").describe("How far each person turns to the right (degrees)"), P::num("detail", 1.0, 0.4, 1.2).describe("Mesh detail (1 the reference; lower for crowds)"), P::color("color", [0.19, 0.19, 0.2]).describe("Clothing colour")],
        |v: &Values, s: u32| crate::npc::lineup(v, s));
}
