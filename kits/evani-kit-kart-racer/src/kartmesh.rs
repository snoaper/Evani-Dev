//! The candy karts' own meshes (see [`crate::candy`]): soft candy shapes the
//! engine's built-in boxes, cylinders and spheres can't make. Built once in
//! code and registered in the mesh library after the item box's; each fits
//! the built-ins' unit box (about -0.5..0.5) and is drawn with plain
//! instances, so every kart, player or rival, local or online, draws the
//! same shapes with no download. Draw only: nothing here is read by the race.
//!
//! * [`ROUND`]: a soft rounded box (flat faces, round edges): candy shells,
//!   gummies, seats, chocolate segments.
//! * [`DONUT`], [`RING`]: a fat and a thin torus round Y (donut and gummy
//!   ring tyres, steering wheels, goggles, chocolate rims).
//! * [`SWIRL`], [`PINWHEEL`]: lollipop and peppermint patterns, each a pair
//!   of thin decals just proud of the built-in cylinder's two caps; drawn
//!   with the cylinder's own transform, so a wheel's pattern rolls with it.
//! * [`SOFTSERVE`]: piped frosting, a ridged tube winding up a cone to a tip.
//! * [`ICING`]: a frosting cap with drips running down its sides.
//! * [`PLEAT`]: a pleated cupcake case. [`FAN`]: a sweet wrapper's twisted
//!   end. [`QUILT`]: a waffle cone (a cone with a raised diamond lattice).
//! * [`STRIPE`]: two helical bands just outside the built-in cylinder (a
//!   candy stick's stripes). [`STAR`], [`HEART`]: puffy emblems.
//!   [`SPRINKLE`]: a small sphere drawn stretched (sprinkles, sugar).
//!   [`BALL`]: a lighter sphere for candy bodies and scoops.

use evani_render::MeshId;
use evani_render::mesh::{Mesh, sphere};

use core::f32::consts::{PI, TAU};

/// Rounded box filling -0.5..0.5 on every axis, edge radius [`ROUND_EDGE`]
/// (16 x 10 grid, about 300 triangles).
pub const ROUND: usize = 0;
/// Torus round Y: outer radius 0.5, tube radius [`DONUT_TUBE`] (y within
/// +-0.18).
pub const DONUT: usize = 1;
/// Torus round Y: outer radius 0.5, tube radius [`RING_TUBE`] (y within
/// +-0.09).
pub const RING: usize = 2;
/// A lollipop swirl on both caps of the unit cylinder: a spiral band from
/// the middle to radius 0.49, [`DECAL_LIFT`] over each cap.
pub const SWIRL: usize = 3;
/// A peppermint on both caps of the unit cylinder: four curved wedges.
pub const PINWHEEL: usize = 4;
/// Piped frosting standing on y = 0: radius 0.5 at the foot, tip at y 0.98.
pub const SOFTSERVE: usize = 5;
/// A frosting cap: an ellipsoid top (radius 0.5 at y = 0, top at y 0.3) and
/// a skirt hanging below y = 0 with drips (longest to y -0.32). Open below.
pub const ICING: usize = 6;
/// A pleated case: 16 pleats, radius 0.4 at y = -0.5 flaring to 0.5 at
/// y = 0.5, closed both ends.
pub const PLEAT: usize = 7;
/// A wrapper's twisted end: a narrow neck at y = -0.5 flaring to a crinkled
/// mouth of radius 0.5 at y = 0.5.
pub const FAN: usize = 8;
/// Two helical bands at radius [`STRIPE_RADIUS`], one turn over
/// y -0.5..0.5 (ending where they started, so pieces stack seamlessly):
/// drawn over the unit cylinder they make a candy stick.
pub const STRIPE: usize = 9;
/// A puffy five-point star in the XZ plane, y -0.5..0.5 (the top bulges to
/// y 0.62 in the middle).
pub const STAR: usize = 10;
/// A puffy heart in the XZ plane, point towards -Z, y -0.5..0.5.
pub const HEART: usize = 11;
/// A small sphere (radius 0.5, 6 x 4): sprinkles, sugar, buttons.
pub const SPRINKLE: usize = 12;
/// A waffle cone: base radius 0.5 at y = -0.5, tip at y = 0.5, its side a
/// raised diamond lattice (flat facets), base closed.
pub const QUILT: usize = 13;
/// A sphere of radius 0.5 (16 x 12): candy bodies, scoops, cherries; lighter
/// than the built-in sphere, which the racers' heads keep.
pub const BALL: usize = 14;
/// How many meshes there are.
pub const COUNT: usize = BALL + 1;

/// Edge radius of [`ROUND`] (of its unit size).
pub const ROUND_EDGE: f32 = 0.22;
/// Tube radius of [`DONUT`].
pub const DONUT_TUBE: f32 = 0.18;
/// Tube radius of [`RING`].
pub const RING_TUBE: f32 = 0.09;
/// How far the cap decals ([`SWIRL`], [`PINWHEEL`]) stand off the caps.
pub const DECAL_LIFT: f32 = 0.025;
/// Radius of the [`STRIPE`] bands (the unit cylinder is 0.5; its 16 flat
/// sides come in to 0.49).
pub const STRIPE_RADIUS: f32 = 0.535;

/// What each mesh falls back to when the library is full (a built-in of
/// about the same size), or `None` for a pure decoration (left out).
pub const SUBSTITUTE: [Option<MeshId>; COUNT] = [
    Some(MeshId::BEVEL_CUBE),
    Some(MeshId::CYLINDER),
    Some(MeshId::CYLINDER),
    None,
    None,
    Some(MeshId::DOME),
    Some(MeshId::DOME),
    Some(MeshId::CYLINDER),
    Some(MeshId::CONE),
    None,
    Some(MeshId::BEVEL_CUBE),
    Some(MeshId::LOW_SPHERE),
    Some(MeshId::LOW_SPHERE),
    Some(MeshId::CONE),
    Some(MeshId::SPHERE),
];

/// Every mesh, in id order.
pub fn build() -> Vec<Mesh> {
    vec![
        round(),
        torus(0.5 - DONUT_TUBE, DONUT_TUBE, 16, 8),
        torus(0.5 - RING_TUBE, RING_TUBE, 16, 5),
        swirl(),
        pinwheel(),
        softserve(),
        icing(),
        pleat(),
        fan(),
        stripe(),
        star(),
        heart(),
        sphere(6, 4),
        quilt(),
        sphere(16, 12),
    ]
}

type V = [f32; 3];

fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: V, b: V) -> V {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn dot(a: V, b: V) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn norm(v: V) -> V {
    let l = dot(v, v).sqrt().max(1e-9);
    [v[0] / l, v[1] / l, v[2] / l]
}

/// One triangle of shared vertices `a, b, c`, wound so it faces the way
/// their normals point; a triangle with no area is left out.
fn tri(m: &mut Mesh, a: u32, b: u32, c: u32) {
    let p = |i: u32| {
        let v = m.vertices[i as usize];
        ([v[0], v[1], v[2]], [v[3], v[4], v[5]])
    };
    let ((pa, na), (pb, nb), (pc, nc)) = (p(a), p(b), p(c));
    let face = cross(sub(pb, pa), sub(pc, pa));
    if dot(face, face) < 1e-14 {
        return;
    }
    let n = [na[0] + nb[0] + nc[0], na[1] + nb[1] + nc[1], na[2] + nb[2] + nc[2]];
    if dot(face, n) >= 0.0 {
        m.indices.extend_from_slice(&[a, b, c]);
    } else {
        m.indices.extend_from_slice(&[a, c, b]);
    }
}

/// A smooth grid of `(rows + 1) x (cols + 1)` vertices from `f(i, j)`
/// (position, normal), two triangles per cell, each facing its normals.
fn grid(m: &mut Mesh, rows: u32, cols: u32, f: impl Fn(u32, u32) -> (V, V)) {
    let base = m.vertices.len() as u32;
    for i in 0..=rows {
        for j in 0..=cols {
            let (p, n) = f(i, j);
            let n = norm(n);
            m.vertices.push([p[0], p[1], p[2], n[0], n[1], n[2]]);
        }
    }
    let w = cols + 1;
    for i in 0..rows {
        for j in 0..cols {
            let a = base + i * w + j;
            let (b, c, d) = (a + 1, a + w, a + w + 1);
            tri(m, a, c, b);
            tri(m, b, c, d);
        }
    }
}

/// A flat triangle with its own vertices, facing away from `inside`.
fn flat(m: &mut Mesh, a: V, b: V, c: V, inside: V) {
    let face = cross(sub(b, a), sub(c, a));
    if dot(face, face) < 1e-14 {
        return;
    }
    let mid = [(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0, (a[2] + b[2] + c[2]) / 3.0];
    let out = dot(face, sub(mid, inside)) >= 0.0;
    let n = norm(if out { face } else { [-face[0], -face[1], -face[2]] });
    let base = m.vertices.len() as u32;
    for p in [a, b, c] {
        m.vertices.push([p[0], p[1], p[2], n[0], n[1], n[2]]);
    }
    if out {
        m.indices.extend_from_slice(&[base, base + 1, base + 2]);
    } else {
        m.indices.extend_from_slice(&[base, base + 2, base + 1]);
    }
}

/// A flat quad `a b c d` (in order round it), facing away from `inside`.
fn flat4(m: &mut Mesh, a: V, b: V, c: V, d: V, inside: V) {
    flat(m, a, b, c, inside);
    flat(m, a, c, d, inside);
}

/// Unit direction at angle `a` in the XZ plane (0 = +X, growing towards +Z).
fn dir(a: f32) -> (f32, f32) {
    let (s, c) = a.sin_cos();
    (c, s)
}

/// [`ROUND`]: a box of half size `0.5 - r` swept by a sphere of radius `r`.
/// Each vertex of a UV sphere's normal `n` lands on the corner block its
/// signs pick, pushed out by `r n`, so the faces between rows are flat.
fn round() -> Mesh {
    let r = ROUND_EDGE;
    let h = 0.5 - r;
    let sg = |x: f32| {
        if x > 1e-4 {
            1.0
        } else if x < -1e-4 {
            -1.0
        } else {
            0.0
        }
    };
    let mut m = Mesh::default();
    grid(&mut m, 10, 16, |i, j| {
        let th = i as f32 / 10.0 * PI;
        let ph = j as f32 / 16.0 * TAU;
        let n = [th.sin() * ph.cos(), th.cos(), th.sin() * ph.sin()];
        ([h * sg(n[0]) + r * n[0], h * sg(n[1]) + r * n[1], h * sg(n[2]) + r * n[2]], n)
    });
    m
}

/// A torus round Y: ring radius `big`, tube radius `tube`.
fn torus(big: f32, tube: f32, around: u32, sides: u32) -> Mesh {
    let mut m = Mesh::default();
    grid(&mut m, sides, around, |i, j| {
        let v = i as f32 / sides as f32 * TAU;
        let (x, z) = dir(j as f32 / around as f32 * TAU);
        let (sv, cv) = v.sin_cos();
        let r = big + tube * cv;
        ([r * x, tube * sv, r * z], [cv * x, sv, cv * z])
    });
    m
}

/// [`SWIRL`]: on each cap, a band winding out 2.25 turns from the middle,
/// widening as it goes, flat on the cap.
fn swirl() -> Mesh {
    let mut m = Mesh::default();
    for ny in [1.0f32, -1.0] {
        let y = ny * (0.5 + DECAL_LIFT);
        grid(&mut m, 44, 1, |i, j| {
            let t = i as f32 / 44.0;
            let r = 0.05 + 0.36 * t;
            let w = 0.025 + 0.055 * t;
            let rr = if j == 0 { (r - w).max(0.0) } else { r + w };
            let (x, z) = dir(2.25 * TAU * t);
            ([rr * x, y, rr * z], [0.0, ny, 0.0])
        });
    }
    m
}

/// [`PINWHEEL`]: on each cap, four wedges from the middle to radius 0.48,
/// each 45 degrees wide at the rim and curving as it goes out.
fn pinwheel() -> Mesh {
    let mut m = Mesh::default();
    for ny in [1.0f32, -1.0] {
        let y = ny * (0.5 + DECAL_LIFT);
        for k in 0..4 {
            let a0 = k as f32 * TAU / 4.0;
            grid(&mut m, 4, 3, |i, j| {
                let rho = 0.48 * i as f32 / 4.0;
                let a = a0 + 1.1 * rho / 0.48 + (TAU / 8.0) * j as f32 / 3.0;
                let (x, z) = dir(a);
                ([rho * x, y, rho * z], [0.0, ny, 0.0])
            });
        }
    }
    m
}

/// [`SOFTSERVE`]: a ridged tube (eight sides, every other one pulled in
/// like a piping nozzle's) winding 2.4 turns up a cone, then curling in to
/// a tip, round a core cone that hides the gaps between the turns.
fn softserve() -> Mesh {
    let mut m = Mesh::default();
    let turns = 2.4f32;
    let path = |t: f32| -> (V, f32) {
        if t <= 1.0 {
            let (x, z) = dir(turns * TAU * t);
            let r = 0.34 * (1.0 - 0.82 * t);
            ([r * x, 0.12 + 0.6 * t, r * z], 0.16 * (1.0 - 0.55 * t))
        } else {
            // The tip: from the last turn's end in to the middle and up.
            let s = (t - 1.0) / 0.2;
            let (x, z) = dir(turns * TAU);
            let r = 0.34 * 0.18 * (1.0 - s);
            ([r * x, 0.72 + 0.24 * s, r * z], 0.072 * (1.0 - s).max(0.0).powf(0.8))
        }
    };
    let steps = 40u32;
    let sides = 8u32;
    let t_of = |i: u32| i as f32 / steps as f32 * 1.2;
    grid(&mut m, steps, sides, |i, j| {
        let t = t_of(i);
        let (c, rad) = path(t);
        let (c2, _) = path((t + 0.01).min(1.2));
        let (c0, _) = path((t - 0.01).max(0.0));
        let tan = norm(sub(c2, c0));
        let out = norm([c[0], 0.0, c[2]]);
        let out = if dot(out, out) < 0.5 { [1.0, 0.0, 0.0] } else { out };
        let b = norm(cross(tan, out));
        let n = norm(cross(b, tan));
        let (s, co) = (j as f32 / sides as f32 * TAU).sin_cos();
        let ridge = if j % 2 == 0 { 1.0 } else { 0.8 };
        let d = [n[0] * co + b[0] * s, n[1] * co + b[1] * s, n[2] * co + b[2] * s];
        ([c[0] + d[0] * rad * ridge, c[1] + d[1] * rad * ridge, c[2] + d[2] * rad * ridge], d)
    });
    // The tube's open start, closed with a fan.
    let (c, _) = path(0.0);
    let start = m.vertices.len() as u32;
    let tan = norm(sub(path(0.01).0, c));
    m.vertices.push([c[0], c[1], c[2], -tan[0], -tan[1], -tan[2]]);
    for j in 0..sides {
        // The grid's first row is the tube's start ring.
        let v = m.vertices[j as usize];
        m.vertices.push([v[0], v[1], v[2], -tan[0], -tan[1], -tan[2]]);
    }
    for j in 0..sides {
        tri(&mut m, start, start + 1 + j, start + 1 + (j + 1) % sides);
    }
    // The core: a cone from radius 0.3 on the ground to a point at y 0.8.
    grid(&mut m, 1, 12, |i, j| {
        let (x, z) = dir(j as f32 / 12.0 * TAU);
        let n = [x * 0.8, 0.37, z * 0.8];
        if i == 0 { ([0.3 * x, 0.0, 0.3 * z], n) } else { ([0.0, 0.8, 0.0], n) }
    });
    m
}

/// [`ICING`]: an ellipsoid cap over a skirt whose lower edge runs down in
/// seven drips of different lengths.
fn icing() -> Mesh {
    let mut m = Mesh::default();
    let seg = 28u32;
    // The cap: a quarter ellipse (radius 0.5, height 0.3) swept round Y.
    grid(&mut m, 4, seg, |i, j| {
        let a = i as f32 / 4.0 * core::f32::consts::FRAC_PI_2;
        let (x, z) = dir(j as f32 / seg as f32 * TAU);
        let (sa, ca) = a.sin_cos();
        ([0.5 * sa * x, 0.3 * ca, 0.5 * sa * z], [sa * x / 0.25, ca / 0.09, sa * z / 0.25])
    });
    const DRIPS: [f32; 7] = [1.0, 0.55, 0.85, 0.45, 0.95, 0.65, 0.5];
    // Each drip a rounded tongue: a smooth bump, flat between drips.
    let drop = |a: f32| {
        let k = ((a / TAU * 7.0).round() as usize) % 7;
        let lobe = (0.5 + 0.5 * (a * 7.0).cos()).powi(4);
        0.05 + 0.27 * DRIPS[k] * lobe
    };
    let skirt = 56u32;
    grid(&mut m, 3, skirt, |i, j| {
        let a = j as f32 / skirt as f32 * TAU;
        let (x, z) = dir(a);
        // Rows at 0, 0.55, 0.88 and 1 of the drop: the tongues round off.
        let f = [0.0f32, 0.55, 0.88, 1.0][i as usize];
        let y = -drop(a) * f;
        let r = 0.5 + 0.014 * f;
        ([r * x, y, r * z], [x, -0.15 * f, z])
    });
    m
}

/// [`PLEAT`]: 32 flat facets (16 pleats, every other corner pulled in).
fn pleat() -> Mesh {
    let mut m = Mesh::default();
    let n = 32u32;
    let corner = |i: u32, y: f32| {
        let (x, z) = dir(i as f32 / n as f32 * TAU);
        let r = if y > 0.0 { 0.5 } else { 0.4 } * if i.is_multiple_of(2) { 1.0 } else { 0.88 };
        [r * x, y, r * z]
    };
    let mid = [0.0, 0.0, 0.0];
    for i in 0..n {
        let j = (i + 1) % n;
        flat4(&mut m, corner(i, -0.5), corner(j, -0.5), corner(j, 0.5), corner(i, 0.5), mid);
        flat(&mut m, [0.0, 0.5, 0.0], corner(i, 0.5), corner(j, 0.5), [0.0, 0.0, 0.0]);
        flat(&mut m, [0.0, -0.5, 0.0], corner(j, -0.5), corner(i, -0.5), [0.0, 0.0, 0.0]);
    }
    m
}

/// [`FAN`]: a narrow neck flaring through a middle ring to a mouth whose
/// edge goes in and out and up and down (crinkled cellophane).
fn fan() -> Mesh {
    let mut m = Mesh::default();
    let n = 14u32;
    let ring = |i: u32, k: usize| -> V {
        let (x, z) = dir(i as f32 / n as f32 * TAU);
        let odd = i % 2 == 1;
        match k {
            0 => [0.07 * x, -0.5, 0.07 * z],
            1 => [0.08 * x, -0.32, 0.08 * z],
            2 => {
                let r = if odd { 0.22 } else { 0.27 };
                [r * x, 0.05, r * z]
            }
            _ => {
                let (r, y) = if odd { (0.38, 0.4) } else { (0.5, 0.5) };
                [r * x, y, r * z]
            }
        }
    };
    for i in 0..n {
        let j = (i + 1) % n;
        for k in 0..3 {
            let inside = [0.0, ring(i, k)[1] - 0.2, 0.0];
            flat4(&mut m, ring(i, k), ring(j, k), ring(j, k + 1), ring(i, k + 1), inside);
        }
        flat(&mut m, [0.0, -0.5, 0.0], ring(j, 0), ring(i, 0), [0.0, 0.0, 0.0]);
        flat(&mut m, [0.0, 0.42, 0.0], ring(i, 3), ring(j, 3), [0.0, 0.0, 0.0]);
    }
    m
}

/// [`STRIPE`]: two bands half a turn apart, each 60 degrees wide, one
/// turn from y -0.5 to 0.5.
fn stripe() -> Mesh {
    let mut m = Mesh::default();
    for k in 0..2 {
        grid(&mut m, 16, 2, |i, j| {
            let t = i as f32 / 16.0;
            let a = k as f32 * PI + TAU * t + (60f32).to_radians() * j as f32 / 2.0;
            let (x, z) = dir(a);
            ([STRIPE_RADIUS * x, t - 0.5, STRIPE_RADIUS * z], [x, 0.0, z])
        });
    }
    m
}

/// A puffy flat shape: the outline `pts` (in the XZ plane, round the
/// origin) as a prism from y -0.5 to 0.5 whose top rises to y 0.62 in the
/// middle (flat facets).
fn puffy(pts: &[(f32, f32)]) -> Mesh {
    let mut m = Mesh::default();
    let n = pts.len();
    let at = |i: usize, y: f32| [pts[i % n].0, y, pts[i % n].1];
    for i in 0..n {
        let j = i + 1;
        flat4(&mut m, at(i, -0.5), at(j, -0.5), at(j, 0.5), at(i, 0.5), [0.0, 0.0, 0.0]);
        flat(&mut m, [0.0, 0.62, 0.0], at(i, 0.5), at(j, 0.5), [0.0, -0.5, 0.0]);
        flat(&mut m, [0.0, -0.5, 0.0], at(j, -0.5), at(i, -0.5), [0.0, 0.5, 0.0]);
    }
    m
}

/// [`STAR`]: five points, inner radius 0.21.
fn star() -> Mesh {
    let pts: Vec<(f32, f32)> = (0..10)
        .map(|i| {
            let r = if i % 2 == 0 { 0.5 } else { 0.21 };
            // The first point towards +Z.
            let (x, z) = dir(core::f32::consts::FRAC_PI_2 + i as f32 / 10.0 * TAU);
            (r * x, r * z)
        })
        .collect();
    puffy(&pts)
}

/// [`HEART`]: the classic heart curve, scaled into 0.5 and centred, its
/// point towards -Z.
fn heart() -> Mesh {
    let n = 22;
    let raw: Vec<(f32, f32)> = (0..n)
        .map(|i| {
            let t = i as f32 / n as f32 * TAU;
            let x = 16.0 * t.sin().powi(3);
            let z = 13.0 * t.cos() - 5.0 * (2.0 * t).cos() - 2.0 * (3.0 * t).cos() - (4.0 * t).cos();
            (x, z)
        })
        .collect();
    // z runs from -17 (the point) to about 12; centre it.
    let pts: Vec<(f32, f32)> = raw.iter().map(|&(x, z)| (x / 34.0, (z + 2.5) / 34.0)).collect();
    puffy(&pts)
}

/// [`QUILT`]: a cone (16 around, 8 rings) whose vertices are pushed in on a
/// checkerboard, so the facets between them make a raised diamond lattice.
fn quilt() -> Mesh {
    let mut m = Mesh::default();
    let (around, rings) = (16u32, 8u32);
    let p = |i: u32, j: u32| -> V {
        let t = j as f32 / rings as f32;
        let r = 0.5 * (1.0 - t);
        let inset = if (i + j) % 2 == 1 { 1.0 - 0.17 * (1.0 - t * 0.5) } else { 1.0 };
        let (x, z) = dir(i as f32 / around as f32 * TAU);
        [r * inset * x, t - 0.5, r * inset * z]
    };
    for j in 0..rings {
        let axis = [0.0, j as f32 / rings as f32 - 0.5, 0.0];
        for i in 0..around {
            let i2 = (i + 1) % around;
            let (a, b, c, d) = (p(i, j), p(i2, j), p(i2, j + 1), p(i, j + 1));
            // Split each cell along its raised diagonal: the ridges.
            if (i + j) % 2 == 0 {
                flat(&mut m, a, b, c, axis);
                flat(&mut m, a, c, d, axis);
            } else {
                flat(&mut m, a, b, d, axis);
                flat(&mut m, b, c, d, axis);
            }
        }
    }
    for i in 0..around {
        flat(&mut m, [0.0, -0.5, 0.0], p((i + 1) % around, 0), p(i, 0), [0.0, 0.0, 0.0]);
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every mesh is well formed, fits the unit box (with a small margin
    /// for the cap decals and stripes), and its triangles face the way its
    /// normals point.
    #[test]
    fn meshes_are_sound_and_fit_the_unit_box() {
        let all = build();
        assert_eq!(all.len(), COUNT);
        for (k, m) in all.iter().enumerate() {
            assert!(!m.indices.is_empty() && m.indices.len() % 3 == 0, "mesh {k}");
            assert!(m.indices.iter().all(|&i| (i as usize) < m.vertices.len()), "mesh {k} index out of range");
            for v in &m.vertices {
                assert!(v.iter().all(|c| c.is_finite()), "mesh {k} has a NaN");
                let l = (v[3] * v[3] + v[4] * v[4] + v[5] * v[5]).sqrt();
                assert!((l - 1.0).abs() < 1e-3, "mesh {k} normal length {l}");
                assert!(v[0].abs() <= 0.54 && v[2].abs() <= 0.54, "mesh {k} wide {v:?}");
                assert!(v[1] >= -0.54 && v[1] <= 1.0, "mesh {k} tall {v:?}");
            }
            for t in m.indices.as_chunks::<3>().0 {
                let p = |i: u32| {
                    let v = m.vertices[i as usize];
                    ([v[0], v[1], v[2]], [v[3], v[4], v[5]])
                };
                let ((a, na), (b, nb), (c, nc)) = (p(t[0]), p(t[1]), p(t[2]));
                let face = cross(sub(b, a), sub(c, a));
                let n = [na[0] + nb[0] + nc[0], na[1] + nb[1] + nc[1], na[2] + nb[2] + nc[2]];
                assert!(dot(face, n) >= 0.0, "mesh {k}: a triangle faces against its normals");
            }
        }
    }

    /// The round box's flat faces are flat: its top is at 0.5 everywhere
    /// inside the edge radius.
    #[test]
    fn the_round_box_fills_the_unit_box() {
        let m = round();
        let max = m.vertices.iter().fold([0.0f32; 3], |a, v| [a[0].max(v[0].abs()), a[1].max(v[1].abs()), a[2].max(v[2].abs())]);
        for c in max {
            assert!((c - 0.5).abs() < 1e-4, "{max:?}");
        }
    }

    /// The cap decals stay inside the cylinder's rim and just over its caps.
    #[test]
    fn cap_decals_sit_on_the_caps_inside_the_rim() {
        for m in [swirl(), pinwheel()] {
            for v in &m.vertices {
                assert!(v[0].hypot(v[2]) <= 0.495, "{v:?}");
                assert!((v[1].abs() - (0.5 + DECAL_LIFT)).abs() < 1e-5, "{v:?}");
            }
        }
    }
}
