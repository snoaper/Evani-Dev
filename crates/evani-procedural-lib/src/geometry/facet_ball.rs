//! The faceted concrete ball (`geometry-icosphere`; the live faceted-ball
//! session, 2026-11-04, against its reference best.png): a geodesic ball of
//! cast concrete panels (`detail` 1: the 80 triangles of a frequency-2
//! geodesic). Every panel is a shallow three-sided pyramid: its centre raised
//! along the panel's normal by `relief` (seeded a little per panel), three
//! flat faces meeting in crisp creases from the centre to the corners, so a
//! panel shows a lighter and a darker face and the ball a star and zigzag
//! pattern of light. Neighbouring panels meet in a hairline joint, a narrow
//! V groove whose walls are drawn in a darker, dull concrete (the gap's
//! shadow), so the joints read as thin dark seams. Panels fall in three
//! seeded groups a few per cent lighter or darker, each with its own stain
//! pattern, as panels cast one by one. Round 4 (its review: joints read as
//! drawn black lines, every apex alike): each panel's rim a narrow chamfer
//! (twice the joint's half width, seeded per panel) that catches the light
//! between face and joint, the joint walls a mid grey, the apexes' rise and
//! place more varied. Round 6 (round 5's review: the 2 mm rolled rims read
//! as tan piping): a 0.6 mm chamfer in the panel's own tone, the groove's
//! floor turned in so each joint shades to a fine dark hairline, the apexes
//! twice as varied. All real geometry; the concrete is the plaster
//! recipe's cast concrete at a sculpture's scale (variant 6), drawn as one
//! cast piece over each panel's faces (the ball's centre rides in colour C's
//! blue). Only this asset uses it.

use std::collections::HashMap;

use evani_procedural::geo::{Mesh, normalize};
use evani_procedural::surface::Surface;
use evani_procedural::{Part, Values, noise, shapes as sh};

/// The colour param's default: best.png's neutral, cool-leaning grey cast
/// concrete (linear; the warmth comes from the low sun).
pub const COLOR: [f32; 3] = [0.29, 0.285, 0.272];
/// The pyramids' rise (share of the radius) by default: about 6% of a
/// panel's edge, shallow enough that the panels' own edges stay ridges
/// (round 4: 3% left best.png's light and dark faces too alike).
pub const RELIEF: f32 = 0.038;
/// The plaster variant the ball's concrete draws with: the cast concrete
/// (5) at a finer pattern scale (rc_plaster.wgsl; nothing else uses it).
const PANEL_VARIANT: u8 = 6;
/// The panels' tone groups: each a little lighter or darker than the colour
/// (round 5: within 4%, so no single panel pops out).
const GROUPS: [f32; 3] = [0.96, 1.0, 1.04];
/// The concrete's own layers, live: each below 0 keeps the preset's number,
/// 0..1 replaces it (rc_plaster_concrete.wgsl's slot).
const LAYERS: [(&str, usize); 6] = [("rough", 0), ("mottle", 1), ("aggregate", 2), ("pores", 5), ("weathering", 6), ("blowholes", 7)];

/// The ball's finish (`finish`, `color`; the concrete by default, at the
/// `pattern` fineness), `k` times as light.
pub fn surface(v: &Values, s: u32, k: f32) -> Surface {
    let mut sf = evani_procedural::surface::finish_tinted(v.f("finish"), v.color("color"), COLOR, 1.0, |x| concrete(v, x)).seed(s).scale(1.0);
    if (k - 1.0).abs() > 1e-3 {
        let a = sf.colour(0);
        sf = sf.tint([a[0] * k, a[1] * k, a[2] * k]);
    }
    // The plaster recipe (code 8) in the ball's variant: its own concrete.
    if sf.block[0] == 8 && sf.block[1] == PANEL_VARIANT {
        // The pattern scale and the panel mottle ride in colour C's red and
        // green (unused by the concrete), set after any tint so a colour
        // change never moves them.
        // Blue 0: the planes follow each face (a preview form); `parts`
        // sets the ball's centre there.
        let p = ((v.f("pattern").clamp(1.0, 8.0) - 1.0) / 7.0).clamp(0.0, 1.0);
        return sf.set_colour(2, [p, v.f("patches").clamp(0.0, 1.0), 0.0]);
    }
    sf
}

/// The ball's concrete on `x`: the `concrete` preset, its live layers, the
/// grain's strength (slot 4's upper half), the stains and streaks (slots 8
/// and 9) and the diagnostic view, drawn as the ball's own variant.
pub fn concrete(v: &Values, x: Surface) -> Surface {
    let preset = v.f("concrete").round().clamp(0.0, 4.0) as usize;
    let mut c = evani_procedural::surface::concrete_preset(x, preset).tint(COLOR).variant(PANEL_VARIANT);
    for (name, slot) in LAYERS {
        let n = v.f(name);
        if n >= 0.0 {
            c = c.number(slot, n);
        }
    }
    let grain = v.f("grain").clamp(0.0, 4.0);
    if (grain - 1.0).abs() > 0.01 {
        c = c.number(4, 0.5 + grain / 8.0);
    }
    c = c.number(8, v.f("stains").clamp(0.0, 1.0)).number(9, v.f("streaks").clamp(0.0, 1.0));
    evani_procedural::surface::concrete_view(c, v.f("view"))
}

/// The rims' tone next to their panel's: the face's concrete, a shade
/// darker (round 6; round 5's warmer, darker tint read as tan piping
/// round every panel).
const RIM_TONE: f32 = 0.97;

/// The ball's parts: the panels in their tone groups, their rims and the
/// joints.
pub fn parts(v: &Values, r: f32, s: u32) -> Vec<Part> {
    let (groups, rims, joints, centre) = meshes(v, r, s);
    // The ball's centre (its height over the ground, a tenth of a metre a
    // unit) in colour C's blue: the concrete's planes follow the direction
    // from it, so a panel's faces share one cast surface.
    let centred = |sf: Surface| {
        if sf.block[0] == 8 && sf.block[1] == PANEL_VARIANT {
            let c = sf.colour(2);
            sf.set_colour(2, [c[0], c[1], (centre / 10.0).clamp(0.001, 1.0)])
        } else {
            sf
        }
    };
    let mut out: Vec<Part> = groups
        .into_iter()
        .enumerate()
        .filter(|(_, m)| !m.indices.is_empty())
        .map(|(g, m)| Part::surfaced(m, centred(surface(v, s ^ (g as u32).wrapping_mul(0x9e37_79b9), GROUPS[g]))))
        .collect();
    if !rims.indices.is_empty() {
        // The panel's concrete a shade darker (`surface` puts colour C, the
        // concrete's pattern data, back after its tint).
        out.push(Part::surfaced(rims, centred(surface(v, s ^ 0x52, RIM_TONE))));
    }
    if !joints.indices.is_empty() {
        // The joint walls: the gap's shadowed concrete, dull (a 1 mm gap
        // gets little light; at `seam_tone` 0.5 a mid grey, not a black line).
        out.push(Part::surfaced(joints, centred(surface(v, s ^ 0x51, v.f("seam_tone").clamp(0.05, 1.0))).number(0, 0.97).number(8, 0.0)));
    }
    out
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Key {
    /// An icosahedron corner.
    Corner(u32),
    /// A point on an icosahedron edge (corner ids low, high; its step from the low one).
    Edge(u32, u32, u32),
    /// A point inside icosahedron face `.0` (grid steps `.1`, `.2`).
    Face(u32, u32, u32),
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn mul(a: [f32; 3], k: f32) -> [f32; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn len(a: [f32; 3]) -> f32 {
    dot(a, a).sqrt()
}

/// The geodesic's corners (unit directions) and its panels (corner
/// indices), at frequency `f`: every icosahedron face split into f x f
/// triangles.
fn geodesic(f: u32) -> (Vec<[f32; 3]>, Vec<[u32; 3]>) {
    let ico = sh::icosphere(0);
    let dir = |i: u32| {
        let x = ico.vertices[i as usize];
        normalize([x[0], x[1], x[2]])
    };
    let mut points: Vec<[f32; 3]> = Vec::new();
    let mut at: HashMap<Key, u32> = HashMap::new();
    let mut panels = Vec::new();
    for (fi, t) in ico.indices.as_chunks::<3>().0.iter().enumerate() {
        let (ia, ib, ic) = (t[0], t[1], t[2]);
        let (a, b, c) = (dir(ia), dir(ib), dir(ic));
        // Grid point (i, j): barycentric (f - i - j, i, j) over (a, b, c).
        let mut id = |i: u32, j: u32| -> u32 {
            let key = if i == 0 && j == 0 {
                Key::Corner(ia)
            } else if i == f && j == 0 {
                Key::Corner(ib)
            } else if i == 0 && j == f {
                Key::Corner(ic)
            } else if j == 0 {
                if ia < ib { Key::Edge(ia, ib, i) } else { Key::Edge(ib, ia, f - i) }
            } else if i == 0 {
                if ia < ic { Key::Edge(ia, ic, j) } else { Key::Edge(ic, ia, f - j) }
            } else if i + j == f {
                if ib < ic { Key::Edge(ib, ic, j) } else { Key::Edge(ic, ib, f - j) }
            } else {
                Key::Face(fi as u32, i, j)
            };
            *at.entry(key).or_insert_with(|| {
                let w = (f - i - j) as f32;
                points.push(normalize(add(add(mul(a, w), mul(b, i as f32)), mul(c, j as f32))));
                (points.len() - 1) as u32
            })
        };
        for i in 0..f {
            for j in 0..f - i {
                panels.push([id(i, j), id(i + 1, j), id(i, j + 1)]);
                if i + j + 1 < f {
                    panels.push([id(i + 1, j), id(i + 1, j + 1), id(i, j + 1)]);
                }
            }
        }
    }
    (points, panels)
}

/// The panels' rim chamfer, in joint half-widths (`seam`): its width across
/// the face and its drop towards the joint (seeded 0.7 to 1.3 times per
/// panel). Round 6: at the default seam about 0.6 mm wide and 0.25 mm deep,
/// a third of round 5's, whose 2 mm rolled rims caught the sun as a raised
/// bead on both sides of every joint (piping); now the joint reads as a thin
/// dark hairline groove, as best.png's.
const BEVEL: (f32, f32) = (1.2, 0.5);
/// The pieces each rim chamfer is drawn in along its edge (its width
/// wanders between them).
const SEG: usize = 4;
/// How far a face's shading normals follow the ball's curve (round 10).
const DOME: f32 = 0.45;

/// The panels' faces (three tone groups), their rims, the joint walls and
/// the centre's height of the ball of radius `r` on the ground (see the
/// module).
pub fn meshes(v: &Values, r: f32, s: u32) -> ([Mesh; 3], Mesh, Mesh, f32) {
    // `detail` 0: the icosahedron's 20 panels; 1: the reference's 80; 2: 180...
    let f = v.f("detail").round().clamp(0.0, 4.0) as u32 + 1;
    let (dirs, panels) = geodesic(f);
    let size = (r / 0.5).sqrt();
    let rise = v.f("relief").clamp(0.0, 0.2) * r;
    let w = v.f("seam").clamp(0.0, 20.0) * 0.001 * size;
    let d = v.f("seam_depth").clamp(0.0, 10.0) * 0.001 * size;
    // The corners, every one a little off true (hand-finished; shared
    // corners move together, so panels stay closed).
    let k = 1.0 / (r * 0.6).max(1e-3);
    let pts: Vec<[f32; 3]> = dirs
        .iter()
        .map(|u| {
            let p = mul(*u, r);
            let q = mul(p, k);
            let amt = r * 0.008;
            add(p, [amt * noise::fbm3p(q, s, 3, 0.5), amt * noise::fbm3p([q[0] + 31.7, q[1], q[2]], s ^ 0x9e37, 3, 0.5), amt * noise::fbm3p([q[0], q[1] + 17.3, q[2]], s ^ 0x7f4a, 3, 0.5)])
        })
        .collect();
    let mut groups: [Mesh; 3] = [Mesh::default(), Mesh::default(), Mesh::default()];
    let mut rims = Mesh::default();
    let mut joints = Mesh::default();
    let push = |m: &mut Mesh, p: [f32; 3], n: [f32; 3]| -> u32 {
        m.vertices.push([p[0], p[1], p[2], n[0], n[1], n[2]]);
        (m.vertices.len() - 1) as u32
    };
    for (pi, t) in panels.iter().enumerate() {
        let mut c = [pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]];
        let mut ci = *t;
        let g = mul(add(add(c[0], c[1]), c[2]), 1.0 / 3.0);
        let mut n = normalize(cross(sub(c[1], c[0]), sub(c[2], c[0])));
        if dot(n, g) < 0.0 {
            c.swap(1, 2);
            ci.swap(1, 2);
            n = mul(n, -1.0);
        }
        // The panel's face: inset by the joint's half width all round
        // (scaled about the incentre, so every edge moves in by `w`).
        let e = [len(sub(c[2], c[1])), len(sub(c[0], c[2])), len(sub(c[1], c[0]))];
        let per = e[0] + e[1] + e[2];
        let inc = mul(add(add(mul(c[0], e[0]), mul(c[1], e[1])), mul(c[2], e[2])), 1.0 / per);
        let area = len(cross(sub(c[1], c[0]), sub(c[2], c[0]))) * 0.5;
        let rho = 2.0 * area / per;
        let shrink = (1.0 - w / rho.max(1e-6)).clamp(0.5, 1.0);
        let q: [[f32; 3]; 3] = std::array::from_fn(|i| add(inc, mul(sub(c[i], inc), shrink)));
        // The pyramid: its centre (the inset face's incentre, off by up to a
        // fifth of the panel's inradius) raised along the panel's normal by
        // 0.65 to 1.35 of the rise; three flat faces, crisp creases. Round 6
        // (its review: every apex alike): twice round 5's spread, so each
        // panel's three faces catch the light their own way (round 7: the
        // offset up to 0.3 of the inradius, so it shows in the normals).
        let e1 = normalize(sub(c[0], inc));
        let e2 = cross(n, e1);
        let jit = add(mul(e1, (sh::rnd(s, pi as u32, 3) - 0.5) * 0.6 * rho), mul(e2, (sh::rnd(s, pi as u32, 4) - 0.5) * 0.6 * rho));
        let apex = add(add(inc, jit), mul(n, rise * (0.65 + 0.7 * sh::rnd(s, pi as u32, 2))));
        // The rim chamfer: the faces start a chamfer's width in from the
        // panel's edge (on their creases, so they stay the same planes), the
        // edge itself drops a little towards the joint; one flat strip per
        // edge between them (its two long sides parallel: planar).
        let bk = 0.7 + 0.6 * sh::rnd(s, pi as u32, 5);
        let (bw, bd) = (BEVEL.0 * w * bk, BEVEL.1 * w * bk);
        let t = if w > 0.0 { (bw / rho.max(1e-6)).clamp(0.0, 0.3) } else { 0.0 };
        let inner: [[f32; 3]; 3] = std::array::from_fn(|i| add(q[i], mul(sub(apex, q[i]), t)));
        let q0 = q;
        let q: [[f32; 3]; 3] = if t > 0.0 { std::array::from_fn(|i| sub(q[i], mul(n, bd))) } else { q };
        let grp = ((sh::rnd(s, pi as u32, 7) * 3.0) as usize).min(2);
        let m = &mut groups[grp];
        // Round 5 (its review: the rim read as a uniform pale double line,
        // like piping): each edge's chamfer in SEG pieces, its width
        // wandering 0.7 to 1.3 times along the edge (seeded; the ends stay on
        // the creases, so a panel's faces still meet there), its normals
        // rolling from the face's into the joint (a worn, rounded arris, no
        // crisp line on either side). The faces stay flat: every inner point
        // lies on its face's plane.
        // Round 7 (round 6's review: every joint an even-weight drawn line):
        // the chamfer's outer edge, which is the joint's lip, moves in and
        // out along the edge (seeded, -0.4 to +0.5 of the joint's half
        // width; the ends stay put), so each groove widens and narrows, and
        // darkens and fades, along its length.
        let segs = if t > 0.0 { SEG } else { 1 };
        let mut lips: [Vec<[f32; 3]>; 3] = Default::default();
        for i in 0..3 {
            let j = (i + 1) % 3;
            let (a, b) = (inner[i], inner[j]);
            let fnrm = normalize(cross(sub(b, a), sub(apex, a)));
            let inw = normalize(cross(n, sub(q[j], q[i])));
            let inw = if dot(inw, sub(inc, q[i])) < 0.0 { mul(inw, -1.0) } else { inw };
            let rows: Vec<([f32; 3], [f32; 3])> = (0..=segs)
                .map(|k| {
                    let u = k as f32 / segs as f32;
                    let top = add(q0[i], mul(sub(q0[j], q0[i]), u));
                    let r = sh::rnd(s, (pi * 3 + i) as u32, 10 + k as u32);
                    let (tk, sk) = if k == 0 || k == segs { (t, 0.0) } else { (t * (0.7 + 0.6 * r), (sh::rnd(s, (pi * 3 + i) as u32, 20 + k as u32) - 0.45) * 0.9 * w) };
                    (add(top, mul(sub(apex, top), tk)), add(add(q[i], mul(sub(q[j], q[i]), u)), mul(inw, sk)))
                })
                .collect();
            lips[i] = rows.iter().map(|r| r.1).collect();
            // Round 10 (round 9's review: machine-perfect faces, no satin on
            // the lit side): each face a touch domed in its shading only
            // (its points stay on its plane): the normals lean by DOME of
            // how far the ball's own curve turns from the face's middle
            // (about 5 degrees at a big face's corner), so the light and the
            // sheen run across a face instead of filling it evenly.
            let mid = normalize(mul(add(add(apex, a), b), 1.0 / 3.0));
            let bent = |p: [f32; 3]| normalize(add(fnrm, mul(sub(normalize(p), mid), DOME)));
            let i2 = push(m, apex, bent(apex));
            let fan: Vec<u32> = rows.iter().map(|r| push(m, r.0, bent(r.0))).collect();
            for k in 0..segs {
                m.indices.extend_from_slice(&[fan[k], fan[k + 1], i2]);
            }
            if t > 0.0 {
                // The rim, its own part (its own tone): the face's inner
                // edge again (the face's normal, so the shading runs on),
                // then the dropped outer edge with the chamfer's own normal
                // (round 6: round 5 rolled it twice as far, past the
                // chamfer, so every rim caught the light like a bead).
                let bn = normalize(cross(sub(q[j], q[i]), sub(a, q[i])));
                let top: Vec<u32> = rows.iter().map(|r| push(&mut rims, r.0, bent(r.0))).collect();
                let lip: Vec<u32> = rows.iter().map(|r| push(&mut rims, r.1, bn)).collect();
                for k in 0..segs {
                    rims.indices.extend_from_slice(&[lip[k], lip[k + 1], top[k + 1], lip[k], top[k + 1], top[k]]);
                }
            }
        }
        // The joint: from the panel's edge down into the groove (the corners
        // pressed towards the centre by the joint's depth), one wall per
        // edge in the lip's pieces, met by the neighbour's wall at the
        // groove's floor (its points worked out from the lower-numbered
        // corner, so both panels' are the same to the bit). Round
        // 6: the walls shaded with their panel's normal (a 1 mm slot's walls
        // see the light as the face does, less what the slot hides:
        // `seam_tone`), so a joint is a fine line always darker than its
        // panel; walls lit by their own sideways normal caught the low sun
        // as pale lines on the side away from it.
        if w > 0.0 && d > 0.0 {
            let bottom: [[f32; 3]; 3] = std::array::from_fn(|i| sub(c[i], mul(normalize(c[i]), d)));
            for i in 0..3 {
                let j = (i + 1) % 3;
                let (b0, b1) = (bottom[i], bottom[j]);
                // The wall faces out of the panel (towards the gap).
                let wn = normalize(cross(sub(b1, b0), sub(q[i], b0)));
                let mid = mul(add(b0, b1), 0.5);
                let flip = dot(wn, sub(mid, inc)) < 0.0;
                let floor = |u: f32| if ci[i] < ci[j] { add(b0, mul(sub(b1, b0), u)) } else { add(b1, mul(sub(b0, b1), 1.0 - u)) };
                let lip = &lips[i];
                for k in 0..lip.len() - 1 {
                    let (u0, u1) = (k as f32 / (lip.len() - 1) as f32, (k + 1) as f32 / (lip.len() - 1) as f32);
                    let a0 = push(&mut joints, floor(u0), n);
                    let a1 = push(&mut joints, floor(u1), n);
                    let a2 = push(&mut joints, lip[k + 1], n);
                    let a3 = push(&mut joints, lip[k], n);
                    if flip {
                        joints.indices.extend_from_slice(&[a0, a2, a1, a0, a3, a2]);
                    } else {
                        joints.indices.extend_from_slice(&[a0, a1, a2, a0, a2, a3]);
                    }
                }
            }
        }
    }
    // On the ground: the lowest point at y = 0, the ball's centre above it.
    let lo = groups.iter().chain([&rims, &joints]).flat_map(|m| m.vertices.iter().map(|x| x[1])).fold(f32::MAX, f32::min);
    for m in groups.iter_mut().chain([&mut rims, &mut joints]) {
        for x in &mut m.vertices {
            x[1] -= lo;
        }
    }
    (groups, rims, joints, -lo)
}

#[cfg(test)]
mod tests {
    use evani_procedural::{Value, Values};

    fn ball(d: f32, set: &[(&str, f32)]) -> ([evani_procedural::geo::Mesh; 3], evani_procedural::geo::Mesh, evani_procedural::geo::Mesh) {
        let reg = crate::registry();
        let Some(a) = reg.get("geometry-icosphere") else { panic!("the faceted ball is built") };
        let specs = evani_procedural::Registry::specs(a);
        let mut v = Values::defaults(&specs).with_density(d);
        for (k, x) in set {
            v.set(k, Value::F32(*x));
        }
        let (g, r, j, _) = super::meshes(&v, 0.5, 1);
        (g, r, j)
    }

    /// The reference's density: 80 panels, each a three-sided pyramid of
    /// flat faces (each face a fan of `SEG` pieces on its plane, crisp
    /// creases; round 10: their shading normals a little domed) and a chamfered rim of `SEG` pieces along each
    /// edge whose normals roll into the joint (a worn arris), the joints
    /// present; on the ground, about a metre across, light.
    #[test]
    fn faceted_ball_has_80_pyramid_panels_on_the_ground() {
        use super::{SEG, cross, dot, normalize, sub};
        let (groups, rims, joints) = ball(1.0, &[]);
        let faces: usize = groups.iter().map(|m| m.indices.len() / 3).sum();
        assert_eq!(faces, 80 * 3 * SEG, "three faces of SEG pieces a panel");
        assert_eq!(rims.indices.len() / 3, 80 * 3 * 2 * SEG, "three rims of SEG two-triangle pieces a panel");
        let rolling = rims.indices.as_chunks::<3>().0.iter().filter(|t| {
            let n: Vec<&[f32; 6]> = t.iter().map(|&i| &rims.vertices[i as usize]).collect();
            n.iter().any(|v| (v[3] - n[0][3]).abs() + (v[4] - n[0][4]).abs() + (v[5] - n[0][5]).abs() > 1e-3)
        });
        assert_eq!(rolling.count(), 80 * 3 * 2 * SEG, "every rim piece rolls its normal into the joint");
        // Round 10: the faces' shading normals lean a few degrees with the
        // ball's curve (domed light), the faces themselves stay flat, and
        // no normal leaves its face's side.
        let mut flat = 0;
        for m in &groups {
            for t in m.indices.as_chunks::<3>().0 {
                let x: Vec<[f32; 6]> = t.iter().map(|&i| m.vertices[i as usize]).collect();
                let p = |k: usize| [x[k][0], x[k][1], x[k][2]];
                let g = normalize(cross(sub(p(1), p(0)), sub(p(2), p(0))));
                assert!(x.iter().all(|v| dot(g, [v[3], v[4], v[5]]) > 0.98), "a face piece's normals within about 11 degrees of its plane's");
                if x.iter().any(|v| (v[3] - x[0][3]).abs() + (v[4] - x[0][4]).abs() + (v[5] - x[0][5]).abs() > 1e-4) {
                    flat += 1;
                }
            }
        }
        assert_eq!(flat, 80 * 3 * SEG, "every face piece flat, its light domed");
        assert_eq!(joints.indices.len() / 3, 80 * 6 * SEG, "three walls a panel, each SEG two-triangle pieces");
        let all = groups.iter().chain([&rims, &joints]);
        let lo = all.clone().flat_map(|m| m.vertices.iter().map(|x| x[1])).fold(f32::MAX, f32::min);
        let hi = all.clone().flat_map(|m| m.vertices.iter().map(|x| x[1])).fold(f32::MIN, f32::max);
        assert!(lo.abs() < 1e-6, "on the ground: {lo}");
        assert!((hi - 1.0).abs() < 0.06, "about a metre high: {hi}");
        assert!(all.flat_map(|m| m.vertices.iter()).all(|v| v.iter().all(|x| x.is_finite())));
    }

    /// Joint walls meet: every wall's floor edge is shared, point for point,
    /// with the neighbouring panel's wall (no gap at the groove's floor).
    #[test]
    fn faceted_ball_joints_close_at_the_floor() {
        let (_, _, joints) = ball(1.0, &[]);
        let key = |p: &[f32; 6]| ((p[0] * 1e4).round() as i64, (p[1] * 1e4).round() as i64, (p[2] * 1e4).round() as i64);
        let mut floors = std::collections::HashMap::<((i64, i64, i64), (i64, i64, i64)), u32>::new();
        // Each wall is four vertices: its floor's two ends, then its top.
        for wall in joints.vertices.as_chunks::<4>().0 {
            let (f0, f1) = (key(&wall[0]), key(&wall[1]));
            let e = if f0 < f1 { (f0, f1) } else { (f1, f0) };
            *floors.entry(e).or_default() += 1;
        }
        assert!(floors.values().all(|&n| n == 2), "every groove floor shared by two walls");
    }

    /// The ball's centre is reported where it is: about a radius up.
    #[test]
    fn faceted_ball_reports_its_centre() {
        let reg = crate::registry();
        let Some(a) = reg.get("geometry-icosphere") else { panic!("the faceted ball is built") };
        let v = Values::defaults(&evani_procedural::Registry::specs(a));
        let (_, _, _, c) = super::meshes(&v, 0.5, 1);
        assert!((c - 0.5).abs() < 0.03, "centre {c}");
    }

    /// Without joints there is no rim either: the faces reach the panel's edge.
    #[test]
    fn faceted_ball_without_joints_has_plain_pyramids() {
        let (groups, rims, joints) = ball(1.0, &[("seam", 0.0)]);
        assert_eq!(groups.iter().map(|m| m.indices.len() / 3).sum::<usize>(), 80 * 3, "one triangle a face");
        assert!(rims.indices.is_empty() && joints.indices.is_empty());
    }

    /// The same seed makes the same ball.
    #[test]
    fn faceted_ball_is_deterministic() {
        let (a, ra, ja) = ball(1.0, &[]);
        let (b, rb, jb) = ball(1.0, &[]);
        assert!(ra.vertices.iter().zip(&rb.vertices).all(|(p, q)| p == q));
        for (x, y) in a.iter().zip(&b) {
            assert_eq!(x.indices, y.indices);
            assert!(x.vertices.iter().zip(&y.vertices).all(|(p, q)| p == q));
        }
        assert!(ja.vertices.iter().zip(&jb.vertices).all(|(p, q)| p == q));
    }
}
