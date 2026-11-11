//! Category 19, Clothing and character equipment: basic generators (batch
//! C). Garments are the shared body template's pieces (`craft::torso`,
//! `arm`, `leg`, `foot`) grown a little past the skin and cut to length,
//! skirts and hems turned profiles round the body, shown on a grey
//! mannequin; footwear, hats, belts and bags are their own lofts and
//! sweeps. Faces +z. Inspired by three.js (MIT) LatheGeometry,
//! TubeGeometry and ParametricGeometry, and the garment-as-offset-body idea
//! of character creators; written fresh here.

use evani_procedural::Registry;

#[allow(unused_imports, dead_code)]
pub fn register(_r: &mut Registry) {
    use evani_procedural::{Output, ParamSpec as P, Part, Values, build as b, craft as c, geo, shapes as sh};
    use c::joint::*;
    type Mesh = geo::Mesh;
    const MANNEQUIN: [f32; 3] = [0.5, 0.51, 0.53];
    const DARK: [f32; 3] = [0.05, 0.05, 0.05];
    const METAL: [f32; 3] = [0.6, 0.58, 0.52];

    fn moved(mut m: Mesh, d: [f32; 3]) -> Mesh {
        geo::transform(&mut m, [1.0; 3], d);
        m
    }

    fn human() -> c::Human {
        c::Human::default()
    }

    /// The mannequin: the whole body in grey.
    fn mannequin(h: &c::Human) -> Part {
        Part::new(c::body_mesh(&c::body(h, 0.0)), MANNEQUIN).material("ground")
    }

    /// A skirt-like shell from the waist at height `y0` (radius `r0`) down
    /// to `y1` flaring to `r1`, `pleats` folds round it, oval like the body.
    fn flare(y0: f32, r0: f32, y1: f32, r1: f32, pleats: u32) -> Mesh {
        let n = 10;
        let prof: Vec<[f32; 2]> = (0..=n).map(|i| {
            let t = i as f32 / n as f32;
            [r1 + (r0 - r1) * t.powf(1.4), y1 + (y0 - y1) * t]
        }).collect();
        let mut m = b::revolve(&prof, 48);
        if pleats > 0 {
            for v in &mut m.vertices {
                let a = v[2].atan2(v[0]);
                let depth = ((y0 - v[1]) / (y0 - y1).max(1e-3)).clamp(0.0, 1.0);
                let k = 1.0 + 0.05 * depth * (a * pleats as f32).sin();
                v[0] *= k;
                v[2] *= k;
            }
            geo::smooth_normals(&mut m);
        }
        geo::transform(&mut m, [1.0, 1.0, 0.72], [0.0; 3]);
        sh::double_sided(&mut m);
        m
    }

    /// Small buttons down the front from `y0` to `y1` at `z`.
    fn buttons(n: u32, y0: f32, y1: f32, z: f32, x: f32) -> Mesh {
        let mut m = Mesh::default();
        for i in 0..n {
            let y = y0 + (y1 - y0) * i as f32 / (n.max(2) - 1) as f32;
            geo::merge(&mut m, &c::ball([0.008, 0.008, 0.004], [x, y, z]));
        }
        m
    }

    /// The front of the trunk at height `y` (its depth, for a garment `g` thick).
    fn front(h: &c::Human, y: f32, g: f32) -> f32 {
        let j = h.frame();
        let t = ((y - (j[PELVIS][1] - h.height * 0.06)) / (j[SHOULDER_L][1] - j[PELVIS][1] + h.height * 0.08)).clamp(0.0, 1.0);
        (0.075 + 0.01 * (t * sh::PI).sin()) * h.height + g + 0.003
    }

    /// A shoe (or a boot's foot) along +z from the heel, `len` long: (upper, sole).
    fn shoe(len: f32, w: f32, toe: f32, heel: f32) -> (Mesh, Mesh) {
        let st: Vec<c::Station> = (0..9).map(|i| {
            let t = i as f32 / 8.0;
            let half = len * w * (0.15 + 0.08 * (t * 1.3).min(1.0)) * if t > 0.85 { 1.0 - (t - 0.85) * (3.0 + toe * 2.0) } else { 1.0 };
            let hh = len * (0.33 - 0.22 * t);
            let lift = heel * (1.0 - t * 1.6).max(0.0);
            c::Station { at: [0.0, lift + len * 0.03 + hh * 0.5, len * t * (0.95 + toe * 0.2)], u: [1.0, 0.0, 0.0], v: [0.0, 1.0, 0.0], profile: c::squircle(half.max(0.004), hh * 0.5, 3.0, 18) }
        }).collect();
        let upper = c::loft(&st, true, true);
        let l2 = len * (0.95 + toe * 0.2);
        let mut sole = b::rounded_box([len * w * 0.4, len * 0.035, l2 * 0.62], 0.01, 2, [0.0, 0.0, l2 * 0.62]);
        geo::merge(&mut sole, &b::cbox([len * w * 0.3, heel + len * 0.035, len * 0.25], [0.0, (heel + len * 0.035) * 0.5, len * 0.13]));
        (upper, sole)
    }

    basic!(_r, "clothing-shirts", "clothing", "shirts", "Shirts", "ground",
        [P::num("sleeve", 0.35, 0.1, 1.0).describe("Sleeve length (share of the arm)"), P::num("fit", 0.008, 0.003, 0.03).unit("m").describe("Looseness (m)"), P::num("length", 0.15, 0.0, 0.35).describe("Hem height (share of the trunk)"), P::color("color", [0.2, 0.32, 0.55]).describe("Fabric colour")],
        |v: &Values, _s: u32| {
            let (sl, g, hem) = (v.f("sleeve"), v.f("fit"), v.f("length"));
            let h = human();
            let j = h.frame();
            let mut shirt = c::torso(&h, hem, 0.99, g);
            for s in [1.0f32, -1.0] {
                geo::merge(&mut shirt, &c::arm(&h, s, sl, g));
            }
            // A collar ring round the neck and buttons down the front.
            let ring: Vec<[f32; 3]> = (0..=20).map(|k| {
                let a = k as f32 / 20.0 * sh::TAU;
                [h.height * 0.045 * a.cos(), j[NECK][1] - h.height * 0.025, h.height * 0.04 * a.sin()]
            }).collect();
            geo::merge(&mut shirt, &b::sweep(&ring, &b::rect(0.012, 0.035), false));
            let bt = buttons(6, j[PELVIS][1] + h.height * 0.04, j[CHEST][1] + h.height * 0.07, front(&h, j[CHEST][1], g), 0.0);
            Output::Parts(vec![mannequin(&h), Part::new(shirt, v.color("color")).material("ground"), Part::new(bt, [0.85, 0.85, 0.8]).material("ground")])
        });

    basic!(_r, "clothing-jackets", "clothing", "jackets", "Jackets", "ground",
        [P::num("length", 0.05, 0.0, 0.3).describe("Hem height (share of the trunk)"), P::num("puff", 0.018, 0.008, 0.045).unit("m").describe("Padding (m)"), P::num("sleeve", 1.0, 0.5, 1.0).describe("Sleeve length (share of the arm)"), P::color("color", [0.12, 0.14, 0.12]).describe("Fabric colour")],
        |v: &Values, _s: u32| {
            let (hem, g, sl) = (v.f("length"), v.f("puff"), v.f("sleeve"));
            let h = human();
            let j = h.frame();
            let mut jk = c::torso(&h, hem, 1.0, g);
            for s in [1.0f32, -1.0] {
                geo::merge(&mut jk, &c::arm(&h, s, sl, g * 0.8));
            }
            // A stand-up collar, a zip down the front, two pockets.
            let ring: Vec<[f32; 3]> = (0..=20).map(|k| {
                let a = k as f32 / 20.0 * sh::TAU;
                [h.height * 0.05 * a.cos(), j[NECK][1] - h.height * 0.02, h.height * 0.045 * a.sin()]
            }).collect();
            geo::merge(&mut jk, &b::sweep(&ring, &b::rect(0.015, 0.06), false));
            let (y0, y1) = (j[PELVIS][1] - h.height * 0.06 + hem * h.height * 0.35, j[NECK][1] - h.height * 0.04);
            let mut trim = b::cbox([0.012, y1 - y0, 0.008], [0.0, (y0 + y1) * 0.5, front(&h, (y0 + y1) * 0.5, g)]);
            for s in [-1.0f32, 1.0] {
                let y = y0 + h.height * 0.07;
                geo::merge(&mut trim, &b::cbox([0.1, 0.015, 0.01], [s * 0.09, y + 0.05, front(&h, y, g) - 0.01]));
            }
            Output::Parts(vec![mannequin(&h), Part::new(jk, v.color("color")).material("ground"), Part::new(trim, METAL).material("metal")])
        });

    basic!(_r, "clothing-coats", "clothing", "coats", "Coats", "ground",
        [P::num("length", 0.55, 0.2, 0.95).describe("Length below the waist (share of the leg)"), P::num("flare", 1.15, 1.0, 1.6).describe("Flare of the skirt"), P::color("color", [0.32, 0.22, 0.14]).describe("Fabric colour")],
        |v: &Values, _s: u32| {
            let (len, fl) = (v.f("length"), v.f("flare"));
            let h = human();
            let j = h.frame();
            let g = 0.02;
            let mut coat = c::torso(&h, 0.2, 1.0, g);
            for s in [1.0f32, -1.0] {
                geo::merge(&mut coat, &c::arm(&h, s, 1.0, g * 0.8));
            }
            // The skirt of the coat from the waist down past the hips and knees.
            let waist = j[PELVIS][1] + h.height * 0.05;
            let r0 = h.height * 0.12;
            let y1 = waist - (waist - 0.05) * len;
            geo::merge(&mut coat, &flare(waist, r0, y1, r0 * fl + (waist - y1) * 0.08, 0));
            // A lapel collar and two rows of buttons.
            for s in [-1.0f32, 1.0] {
                geo::merge(&mut coat, &b::bar([s * 0.03, j[NECK][1] - 0.02, front(&h, j[CHEST][1], g) - 0.02], [s * 0.1, j[CHEST][1] - 0.04, front(&h, j[CHEST][1], g)], 0.05, 0.012));
            }
            let mut bt = Mesh::default();
            for s in [-1.0f32, 1.0] {
                geo::merge(&mut bt, &buttons(3, waist + 0.02, j[CHEST][1] - 0.02, front(&h, j[CHEST][1] - 0.1, g), s * 0.05));
            }
            Output::Parts(vec![mannequin(&h), Part::new(coat, v.color("color")).material("ground"), Part::new(bt, DARK).material("ground")])
        });

    basic!(_r, "clothing-trousers", "clothing", "trousers", "Trousers", "ground",
        [P::num("length", 1.0, 0.55, 1.0).describe("Leg length (share of the leg)"), P::num("width", 0.012, 0.004, 0.04).unit("m").describe("Looseness (m)"), P::color("color", [0.12, 0.15, 0.25]).describe("Fabric colour")],
        |v: &Values, _s: u32| {
            let (len, g) = (v.f("length"), v.f("width"));
            let h = human();
            let j = h.frame();
            let mut tr = c::torso(&h, 0.0, 0.3, g);
            for s in [1.0f32, -1.0] {
                geo::merge(&mut tr, &c::leg(&h, s, len, g));
            }
            // A belt band at the waist with loops.
            let y = j[PELVIS][1] + h.height * 0.04;
            let ring: Vec<[f32; 3]> = (0..=24).map(|k| {
                let a = k as f32 / 24.0 * sh::TAU;
                [(h.height * 0.1 + g + 0.004) * a.cos(), y, (h.height * 0.08 + g + 0.004) * a.sin()]
            }).collect();
            let belt = b::sweep(&ring, &b::rect(0.008, 0.035), false);
            Output::Parts(vec![mannequin(&h), Part::new(tr, v.color("color")).material("ground"), Part::new(belt, [0.2, 0.12, 0.06]).material("ground")])
        });

    basic!(_r, "clothing-shorts", "clothing", "shorts", "Shorts", "ground",
        [P::num("length", 0.35, 0.12, 0.7).describe("Leg length (share of the leg)"), P::num("width", 0.02, 0.005, 0.05).unit("m").describe("Looseness (m)"), P::color("color", [0.6, 0.45, 0.22]).describe("Fabric colour")],
        |v: &Values, _s: u32| {
            let (len, g) = (v.f("length"), v.f("width"));
            let h = human();
            let mut sh_m = c::torso(&h, 0.0, 0.3, g * 0.6);
            for s in [1.0f32, -1.0] {
                geo::merge(&mut sh_m, &c::leg(&h, s, len, g));
            }
            Output::Parts(vec![mannequin(&h), Part::new(sh_m, v.color("color")).material("ground")])
        });

    basic!(_r, "clothing-skirts", "clothing", "skirts", "Skirts", "ground",
        [P::num("length", 0.5, 0.2, 0.95).describe("Length (share of the leg)"), P::num("flare", 1.3, 1.0, 2.0).describe("Flare at the hem"), P::num("pleats", 0.0, 0.0, 24.0).step(1.0).describe("Pleats round it"), P::color("color", [0.45, 0.08, 0.12]).describe("Fabric colour")],
        |v: &Values, _s: u32| {
            let (len, fl, n) = (v.f("length"), v.f("flare"), v.f("pleats").round() as u32);
            let h = human();
            let j = h.frame();
            let waist = j[PELVIS][1] + h.height * 0.06;
            let r0 = h.height * 0.105;
            let y1 = waist - (waist - 0.05) * len;
            let mut skirt = flare(waist, r0, y1, r0 * fl + (waist - y1) * 0.1, n);
            geo::merge(&mut skirt, &c::torso(&h, 0.18, 0.32, 0.006));
            Output::Parts(vec![mannequin(&h), Part::new(skirt, v.color("color")).material("ground")])
        });

    basic!(_r, "clothing-dresses", "clothing", "dresses", "Dresses", "ground",
        [P::num("length", 0.7, 0.3, 0.97).describe("Length (share of the leg)"), P::num("flare", 1.4, 1.0, 2.2).describe("Flare at the hem"), P::color("color", [0.1, 0.32, 0.3]).describe("Fabric colour")],
        |v: &Values, _s: u32| {
            let (len, fl) = (v.f("length"), v.f("flare"));
            let h = human();
            let j = h.frame();
            // A bodice from the waist to under the arms, straps, a skirt.
            let mut dress = c::torso(&h, 0.3, 0.88, 0.006);
            for s in [-1.0f32, 1.0] {
                let p = [s * h.height * 0.06, j[SHOULDER_L][1] + 0.01, 0.0];
                geo::merge(&mut dress, &b::sweep(&[[p[0], j[CHEST][1] + 0.02, front(&h, j[CHEST][1], 0.006) - 0.01], [p[0], p[1] + 0.02, 0.0], [p[0], j[CHEST][1] + 0.02, -front(&h, j[CHEST][1], 0.006) + 0.01]], &b::rect(0.02, 0.004), true));
            }
            let waist = j[PELVIS][1] + h.height * 0.08;
            let r0 = h.height * 0.1;
            let y1 = waist - (waist - 0.05) * len;
            geo::merge(&mut dress, &flare(waist, r0, y1, r0 * fl + (waist - y1) * 0.1, 0));
            Output::Parts(vec![mannequin(&h), Part::new(dress, v.color("color")).material("ground")])
        });

    basic!(_r, "clothing-boots", "clothing", "boots", "Boots", "ground",
        [P::num("shaft", 0.25, 0.08, 0.5).unit("m").describe("Shaft height (m)"), P::num("heel", 0.03, 0.0, 0.08).unit("m").describe("Heel height (m)"), P::num("toe", 0.2, 0.0, 1.0).describe("Toe point"), P::color("color", [0.25, 0.14, 0.07]).describe("Leather colour")],
        |v: &Values, _s: u32| {
            let (shaft, heel, toe) = (v.f("shaft"), v.f("heel"), v.f("toe"));
            let len = 0.28;
            let (mut upper, mut sole) = (Mesh::default(), Mesh::default());
            for x in [-0.08f32, 0.08] {
                let (u, s) = shoe(len, 1.0, toe, heel);
                geo::merge(&mut upper, &moved(u, [x, 0.0, 0.0]));
                geo::merge(&mut sole, &moved(s, [x, 0.0, 0.0]));
                // The shaft up the shin, a little wider at the top.
                geo::merge(&mut upper, &c::limb([x, heel + len * 0.18, len * 0.16], [x, heel + len * 0.18 + shaft, len * 0.13], len * 0.16, len * 0.18, 16));
            }
            Output::Parts(vec![Part::new(upper, v.color("color")).material("ground"), Part::new(sole, [0.06, 0.05, 0.05]).material("ground")])
        });

    basic!(_r, "clothing-shoes", "clothing", "shoes", "Shoes", "ground",
        [P::num("heel", 0.015, 0.0, 0.1).unit("m").describe("Heel height (m)"), P::num("toe", 0.3, 0.0, 1.0).describe("Toe point"), P::num("size", 0.27, 0.2, 0.32).unit("m").describe("Length (m)"), P::color("color", [0.08, 0.08, 0.09]).describe("Upper colour"), P::color("sole", [0.7, 0.68, 0.62]).describe("Sole colour")],
        |v: &Values, _s: u32| {
            let (heel, toe, len) = (v.f("heel"), v.f("toe"), v.f("size"));
            let (mut upper, mut sole, mut laces) = (Mesh::default(), Mesh::default(), Mesh::default());
            for x in [-0.075f32, 0.075] {
                let (u, s) = shoe(len, 0.95, toe, heel);
                // Cut low: the upper only up to the ankle.
                let mut u = u;
                for vt in &mut u.vertices {
                    vt[1] = vt[1].min(heel * 0.6 + len * 0.24);
                }
                geo::smooth_normals(&mut u);
                geo::merge(&mut upper, &moved(u, [x, 0.0, 0.0]));
                geo::merge(&mut sole, &moved(s, [x, 0.0, 0.0]));
                for k in 0..4 {
                    let z = len * (0.35 + 0.08 * k as f32);
                    let y = heel * (1.0 - z / len * 1.6).max(0.0) + len * 0.03 + len * (0.33 - 0.22 * z / len) + 0.003;
                    geo::merge(&mut laces, &b::cbox([len * 0.12, 0.006, 0.008], [x, y.min(heel * 0.6 + len * 0.24) - 0.002, z]));
                }
            }
            Output::Parts(vec![Part::new(upper, v.color("color")).material("ground"), Part::new(sole, v.color("sole")).material("ground"), Part::new(laces, [0.8, 0.8, 0.78]).material("ground")])
        });

    basic!(_r, "clothing-hats", "clothing", "hats", "Hats", "ground",
        [P::num("brim", 0.06, 0.0, 0.2).unit("m").describe("Brim width (m)"), P::num("crown", 0.12, 0.03, 0.25).unit("m").describe("Crown height (m)"), P::num("taper", 0.85, 0.5, 1.1).describe("Crown taper (top over base)"), P::color("color", [0.18, 0.15, 0.12]).describe("Felt colour"), P::color("band", [0.5, 0.1, 0.06]).describe("Band colour")],
        |v: &Values, _s: u32| {
            let (br, cr, tp) = (v.f("brim"), v.f("crown"), v.f("taper"));
            let r = 0.095;
            // On a grey head stand: the crown turned up from the brim, a band.
            let stand = {
                let mut m = c::ball([0.08, 0.11, 0.095], [0.0, 0.2, 0.0]);
                geo::merge(&mut m, &c::limb([0.0, 0.0, 0.0], [0.0, 0.12, 0.0], 0.045, 0.04, 12));
                geo::merge(&mut m, &sh::frustum(0.1, 0.06, 0.03, 24));
                m
            };
            let base_y = 0.26;
            // Walked up the outside and back down the inside.
            let mut prof = vec![[0.0, base_y + cr + 0.004], [r * tp, base_y + cr], [r, base_y], [r + br, base_y - br * 0.15], [r + br, base_y - br * 0.15 - 0.006], [r * 0.98, base_y - 0.006], [r * 0.98 * tp, base_y + cr - 0.006], [0.0, base_y + cr - 0.002]];
            prof.reverse();
            let mut hat = b::revolve(&prof, 40);
            geo::transform(&mut hat, [1.0, 1.0, 1.12], [0.0; 3]);
            let mut band = b::revolve(&[[r * 1.004, base_y + 0.002], [r * (1.0 + (tp - 1.0) * 0.25) * 1.004, base_y + cr * 0.25], [r * 0.99, base_y + cr * 0.25], [r * 0.99, base_y + 0.002]], 40);
            geo::transform(&mut band, [1.0, 1.0, 1.12], [0.0; 3]);
            Output::Parts(vec![Part::new(stand, MANNEQUIN).material("ground"), Part::new(hat, v.color("color")).material("ground"), Part::new(band, v.color("band")).material("ground")])
        });

    basic!(_r, "clothing-gloves", "clothing", "gloves", "Gloves", "ground",
        [P::num("cuff", 0.06, 0.02, 0.2).unit("m").describe("Cuff length (m)"), P::num("fingers", 1.0, 0.0, 1.0).describe("Finger length (0 a mitten)"), P::color("color", [0.3, 0.18, 0.08]).describe("Leather colour")],
        |v: &Values, _s: u32| {
            let (cuff, fing) = (v.f("cuff"), v.f("fingers"));
            let mut glove = Mesh::default();
            // A pair, upright: a cuff, the padded palm, fingers (or a mitten) and a thumb.
            for x in [-0.07f32, 0.07] {
                let mut m = c::limb([0.0, 0.0, 0.0], [0.0, cuff, 0.0], 0.04, 0.035, 14);
                geo::transform(&mut m, [1.2, 1.0, 0.85], [0.0; 3]);
                geo::merge(&mut m, &b::rounded_box([0.095, 0.1, 0.038], 0.016, 3, [0.0, cuff, 0.0]));
                if fing > 0.05 {
                    for (i, l) in [0.07f32, 0.08, 0.076, 0.06].into_iter().enumerate() {
                        let fx = (i as f32 - 1.5) * 0.023;
                        geo::merge(&mut m, &c::limb([fx, cuff + 0.1, 0.0], [fx, cuff + 0.1 + l * fing, 0.0], 0.011, 0.01, 10));
                    }
                } else {
                    geo::merge(&mut m, &c::ball([0.047, 0.05, 0.02], [0.0, cuff + 0.11, 0.0]));
                }
                let s = if x < 0.0 { -1.0 } else { 1.0 };
                geo::merge(&mut m, &c::limb([s * 0.04, cuff + 0.03, 0.01], [s * 0.075, cuff + 0.09, 0.02], 0.014, 0.012, 10));
                geo::merge(&mut glove, &moved(m, [x, 0.0, 0.0]));
            }
            Output::Parts(vec![Part::new(glove, v.color("color")).material("ground")])
        });

    basic!(_r, "clothing-scarves", "clothing", "scarves", "Scarves", "ground",
        [P::num("length", 0.5, 0.1, 0.9).unit("m").describe("Length of the hanging ends (m)"), P::num("width", 0.18, 0.08, 0.3).unit("m").describe("Width (m)"), P::color("color", [0.55, 0.06, 0.06]).describe("Wool colour"), P::color("stripe", [0.85, 0.8, 0.65]).describe("Stripe colour")],
        |v: &Values, _s: u32| {
            let (len, w) = (v.f("length"), v.f("width"));
            let h = human();
            let j = h.frame();
            // Wrapped once round the neck, the two ends hanging down the front.
            let ny = j[NECK][1] - 0.03;
            let ring: Vec<[f32; 3]> = (0..=28).map(|k| {
                let a = k as f32 / 28.0 * sh::TAU;
                [0.075 * a.cos(), ny, 0.07 * a.sin()]
            }).collect();
            let mut wool = b::sweep(&ring, &b::rect(0.03, w * 0.5), false);
            let mut stripes = Mesh::default();
            for (k, s) in [-1.0f32, 1.0].into_iter().enumerate() {
                let x = s * 0.05;
                let z0 = front(&h, ny - 0.1, 0.0) + 0.02;
                let path: Vec<[f32; 3]> = (0..8).map(|i| {
                    let t = i as f32 / 7.0;
                    [x + s * 0.02 * t, ny - 0.02 - (len + 0.05) * t, z0 + 0.03 * (1.0 - t) + k as f32 * 0.012]
                }).collect();
                geo::merge(&mut wool, &b::sweep(&path, &b::rect(w * 0.5, 0.012), true));
                for q in [0.75f32, 0.85, 0.95] {
                    let (p, _, _) = b::along(&path, q);
                    geo::merge(&mut stripes, &b::cbox([w * 0.52, len * 0.025 + 0.005, 0.016], p));
                }
            }
            let bust = {
                let mut m = c::torso(&h, 0.55, 1.0, 0.0);
                geo::merge(&mut m, &c::body(&h, 0.0).head);
                geo::merge(&mut m, &c::body(&h, 0.0).neck);
                m
            };
            c::stand(vec![Part::new(bust, MANNEQUIN).material("ground"), Part::new(wool, v.color("color")).material("ground"), Part::new(stripes, v.color("stripe")).material("ground")])
        });

    basic!(_r, "clothing-belts", "clothing", "belts", "Belts", "ground",
        [P::num("waist", 0.85, 0.6, 1.2).unit("m").describe("Waist (m round)"), P::num("width", 0.035, 0.02, 0.06).unit("m").describe("Strap width (m)"), P::color("color", [0.22, 0.12, 0.06]).describe("Leather colour"), P::color("buckle", METAL).describe("Buckle colour")],
        |v: &Values, _s: u32| {
            let (round, w) = (v.f("waist"), v.f("width"));
            // Buckled as worn: a strap round an oval, a buckle at the front, the tip through a keeper.
            let (rx, rz) = (round / sh::TAU * 1.15, round / sh::TAU * 0.85);
            let ring: Vec<[f32; 3]> = (0..=40).map(|k| {
                let a = k as f32 / 40.0 * sh::TAU;
                [rx * a.sin(), w * 0.5, rz * a.cos()]
            }).collect();
            let mut strap = b::sweep(&ring, &b::rect(0.004, w), false);
            let tip: Vec<[f32; 3]> = (0..6).map(|k| {
                let a = 0.12 + k as f32 * 0.06;
                [rx * a.sin(), w * 0.5, rz * a.cos() + 0.006]
            }).collect();
            geo::merge(&mut strap, &b::sweep(&tip, &b::rect(0.004, w * 0.96), true));
            let frame: Vec<[f32; 3]> = vec![[-w * 0.6, 0.0, rz + 0.008], [w * 0.6, 0.0, rz + 0.008], [w * 0.6, w * 1.1, rz + 0.008], [-w * 0.6, w * 1.1, rz + 0.008], [-w * 0.6, 0.0, rz + 0.008]];
            let mut buckle = b::sweep(&frame, &b::rect(0.005, 0.005), false);
            geo::merge(&mut buckle, &b::bar([0.0, w * 0.55, rz + 0.008], [w * 0.6, w * 0.55, rz + 0.008], 0.004, 0.004));
            let buckle = moved(buckle, [0.0, -w * 0.05, 0.0]);
            Output::Parts(vec![Part::new(strap, v.color("color")).material("ground"), Part::new(buckle, v.color("buckle")).material("metal")])
        });

    basic!(_r, "clothing-backpacks", "clothing", "backpacks", "Backpacks", "ground",
        [P::num("size", 1.0, 0.6, 1.6).describe("Size (1 a day pack)"), P::num("pocket", 0.6, 0.0, 1.0).describe("Front pocket size"), P::color("color", [0.12, 0.28, 0.2]).describe("Fabric colour")],
        |v: &Values, _s: u32| {
            let (k, pk) = (v.f("size"), v.f("pocket"));
            let (w, hgt, d) = (0.3, 0.45, 0.18);
            // The bag, its top flap, a front pocket, two shoulder straps at the back.
            let mut bag = b::rounded_box([w, hgt, d], 0.06, 4, [0.0, 0.0, 0.0]);
            geo::merge(&mut bag, &b::rounded_box([w * 0.95, 0.06, d * 1.05], 0.025, 2, [0.0, hgt - 0.04, 0.01]));
            if pk > 0.05 {
                geo::merge(&mut bag, &b::rounded_box([w * 0.75 * (0.6 + 0.4 * pk), hgt * 0.4 * (0.5 + 0.5 * pk), d * 0.35 * pk + 0.02], 0.03, 3, [0.0, 0.04, d * 0.5]));
            }
            let mut straps = Mesh::default();
            for s in [-1.0f32, 1.0] {
                let path: Vec<[f32; 3]> = (0..10).map(|i| {
                    let t = i as f32 / 9.0;
                    [s * w * 0.28, hgt * (0.92 - 0.82 * t), -d * 0.5 - 0.06 * (t * sh::PI).sin()]
                }).collect();
                geo::merge(&mut straps, &b::sweep(&path, &b::rect(0.05, 0.012), true));
            }
            let mut trim = b::rod([-w * 0.3, hgt * 0.55, d * 0.51], [w * 0.3, hgt * 0.55, d * 0.51], 0.004, 6);
            geo::merge(&mut trim, &b::rod([0.0, hgt, -0.02], [0.0, hgt + 0.03, -0.02], 0.008, 6));
            let mut ps = vec![Part::new(bag, v.color("color")).material("ground"), Part::new(straps, [0.08, 0.08, 0.09]).material("ground"), Part::new(trim, DARK).material("metal")];
            c::scale_parts(&mut ps, k);
            Output::Parts(ps)
        });

    basic!(_r, "clothing-armour-and-capes", "clothing", "armour-and-capes", "Armour and capes", "ground",
        [P::num("cape", 0.9, 0.0, 1.3).unit("m").describe("Cape length (m)"), P::num("plate", 1.0, 0.5, 1.6).describe("Pauldron size"), P::color("color", [0.45, 0.06, 0.06]).describe("Cape colour"), P::color("metal", [0.24, 0.25, 0.27]).describe("Armour colour")],
        |v: &Values, s: u32| {
            let (cl, pl) = (v.f("cape"), v.f("plate"));
            let h = human();
            let j = h.frame();
            // A breastplate over the trunk, pauldrons on the shoulders, greaves on the shins.
            let mut armour = c::torso(&h, 0.25, 0.95, 0.025);
            for sd in [1.0f32, -1.0] {
                let sp = if sd > 0.0 { j[SHOULDER_L] } else { j[SHOULDER_R] };
                let cap = c::ball([0.075 * pl, 0.06 * pl, 0.08 * pl], [sp[0] * 1.08, sp[1] + 0.02, 0.0]);
                geo::merge(&mut armour, &cap);
                let (kn, an) = if sd > 0.0 { (j[KNEE_L], j[ANKLE_L]) } else { (j[KNEE_R], j[ANKLE_R]) };
                geo::merge(&mut armour, &c::limb(c::lerp3(kn, an, 0.05), c::lerp3(kn, an, 0.85), h.height * 0.04, h.height * 0.032, 14));
            }
            // A cape from the shoulders down the back, its folds seeded.
            let mut cape = Mesh::default();
            if cl > 0.01 {
                let top = j[SHOULDER_L][1] + 0.02;
                let (cols, rows) = (14u32, 10u32);
                let mut bl = geo::MeshBuilder::new();
                for r in 0..=rows {
                    let t = r as f32 / rows as f32;
                    for cc in 0..=cols {
                        let u = cc as f32 / cols as f32 - 0.5;
                        let half = 0.24 + 0.3 * t;
                        let fold = 0.025 * t * (u * 18.0 + sh::range(s, 1, 1, 0.0, 6.0)).sin();
                        bl.vertex([u * 2.0 * half, top - cl * t, -0.12 - 0.08 * t * t - fold - 0.06 * (1.0 - (2.0 * u).powi(2)) * (1.0 - t)], [0.0, 0.0, -1.0]);
                    }
                }
                for r in 0..rows {
                    for cc in 0..cols {
                        let a = r * (cols + 1) + cc;
                        bl.quad(a, a + 1, a + cols + 2, a + cols + 1);
                    }
                }
                cape = bl.finish();
                geo::smooth_normals(&mut cape);
                sh::double_sided(&mut cape);
            }
            Output::Parts(vec![mannequin(&h), Part::new(armour, v.color("metal")).material("metal"), Part::new(cape, v.color("color")).material("ground")])
        });
}
