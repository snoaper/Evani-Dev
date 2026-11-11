//! Test shapes for the material viewer, and a tiny glTF 2.0 binary (.glb)
//! writer so each shape can go through the engine's normal model path.
//!
//! Everything here is plain Rust with no outside crates, and fully
//! deterministic: the same call always gives the same bytes.
//!
//! Conventions: Y up, right-handed, counter-clockwise front faces (as glTF
//! expects), outward unit normals, centred on the origin. Flat faces are cut
//! into a fine grid so the bump noise shows on them too.

use std::f32::consts::TAU;

/// One of the viewer's test shapes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Shape {
    Sphere,
    Monkey,
    Cube,
    LongBox,
    Prism,
    Cylinder,
    Ring,
    Teapot,
}

impl Shape {
    /// Every shape, in menu order (sphere first).
    pub const ALL: [Shape; 8] = [
        Shape::Sphere,
        Shape::Monkey,
        Shape::Cube,
        Shape::LongBox,
        Shape::Prism,
        Shape::Cylinder,
        Shape::Ring,
        Shape::Teapot,
    ];

    /// The stable id, as used in saved settings and file names.
    pub fn id(self) -> &'static str {
        match self {
            Shape::Sphere => "sphere",
            Shape::Monkey => "monkey",
            Shape::Cube => "cube",
            Shape::LongBox => "long_box",
            Shape::Prism => "prism",
            Shape::Cylinder => "cylinder",
            Shape::Ring => "ring",
            Shape::Teapot => "teapot",
        }
    }

    /// The name shown to people.
    pub fn label(self) -> &'static str {
        match self {
            Shape::Sphere => "Sphere",
            Shape::Monkey => "Monkey head",
            Shape::Cube => "Big cube",
            Shape::LongBox => "Long box",
            Shape::Prism => "Triangle prism",
            Shape::Cylinder => "Cylinder",
            Shape::Ring => "Ring",
            Shape::Teapot => "Teapot",
        }
    }

    /// The shape with this id, if there is one.
    pub fn from_id(id: &str) -> Option<Shape> {
        Shape::ALL.into_iter().find(|s| s.id() == id)
    }
}

/// A triangle mesh: one position and one normal per vertex, three indices
/// per triangle.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

/// Builds the generated shapes.
///
/// `Shape::Monkey` and `Shape::Teapot` return None: they are streamed model
/// files, not generated (sources in CREDITS.md).
pub fn build(shape: Shape) -> Option<Mesh> {
    let mut b = Builder::default();
    match shape {
        Shape::Sphere => b.sphere(1.0, 96, 48),
        Shape::Cube => b.cuboid([1.45; 3], [24; 3]),
        Shape::LongBox => b.cuboid([2.0, 0.55, 0.55], [32, 12, 12]),
        Shape::Prism => b.prism(0.85, 1.4, 24),
        Shape::Cylinder => b.cylinder(0.75, 1.5, 96, 24, 16),
        Shape::Ring => b.torus(0.72, 0.28, 96, 48),
        Shape::Monkey | Shape::Teapot => return None,
    }
    Some(b.mesh)
}

/// The selection marker: a thin ring of radius 1 lying flat round Y (a tube
/// of radius 0.028), drawn glowing on the floor round the selected shape
/// (critic round 2: the filled disc read as an offset pale blob).
#[cfg(feature = "material-styles")]
pub fn selection_ring() -> Mesh {
    let mut b = Builder::default();
    b.torus(1.0, 0.028, 128, 12);
    b.mesh
}

/// A flat floor tile (a thin slab, about radius 1), for the catalogue's
/// ground, masonry and plaster materials, which are seen from above.
#[cfg(feature = "material-catalogue")]
pub fn tile() -> Mesh {
    let mut b = Builder::default();
    b.cuboid([1.36, 0.1, 1.36], [32, 2, 32]);
    b.mesh
}

/// A standing card (a thin upright slab facing +z), for the catalogue's fire
/// and glow cards.
#[cfg(feature = "material-catalogue")]
pub fn card() -> Mesh {
    let mut b = Builder::default();
    b.cuboid([1.1, 1.5, 0.03], [16, 22, 1]);
    b.mesh
}

/// A terrain tile with a slope (B2-P8): a 2.8 m square patch that tilts up
/// towards the left and carries a soft hump, so turf, litter, moss and soil
/// show how they run over a bank (about radius 1.4). Normals are the surface's
/// own, not a face's. Round 5: the rise fades to nothing at the border, so
/// the whole edge lies level on the floor (it used to touch at one edge only
/// and hover, its shadow showing underneath).
#[cfg(feature = "material-catalogue")]
pub fn slope() -> Mesh {
    const N: usize = 48;
    const S: f32 = SLOPE_HALF;
    let h = slope_h;
    let grad = |x: f32, z: f32| {
        let e = 1e-3;
        ((h(x + e, z) - h(x - e, z)) / (2.0 * e), (h(x, z + e) - h(x, z - e)) / (2.0 * e))
    };
    let mut m = Mesh::default();
    for j in 0..=N {
        for i in 0..=N {
            let (x, z) = ((i as f32 / N as f32 - 0.5) * 2.0 * S, (j as f32 / N as f32 - 0.5) * 2.0 * S);
            let (dx, dz) = grad(x, z);
            m.positions.push([x, h(x, z) - 0.15, z]);
            m.normals.push(normalize([-dx, 1.0, -dz], [0.0, 1.0, 0.0]));
        }
    }
    for j in 0..N {
        for i in 0..N {
            let a = (j * (N + 1) + i) as u32;
            let b = a + (N + 1) as u32;
            m.indices.extend([a, b, a + 1, a + 1, b, b + 1]);
        }
    }
    m
}

/// The slope's half width (m).
#[cfg(feature = "material-catalogue")]
const SLOPE_HALF: f32 = 1.4;

/// The slope's surface height over its base at (x, z) (its mesh sits 0.15
/// lower).
#[cfg(feature = "material-catalogue")]
fn slope_h(x: f32, z: f32) -> f32 {
    let s = SLOPE_HALF;
    let w = (1.0 - (x / s).powi(4)) * (1.0 - (z / s).powi(4));
    (0.1 * (x / s + 1.0) + 0.16 * (-(x * x + z * z) * 1.1).exp()) * w
}

/// The lawn row's own shape (material-view critic rounds 8-11: on the bare
/// slope the lawn read as a flat green card, no blade standing up anywhere):
/// the slope with short blades standing on it, 6 to 11 cm, each a thin closed
/// three-sided spike (flat-shaded, so each blade catches the light on one
/// side) leaning a little along the mowing stripes, its foot sunk 4 mm into
/// the turf. They break the top and the outline, so the patch reads as turf
/// at any size; the turf material draws them (its standing-blade march runs
/// only on faces turned up, so the blades take its painted colour). Placed by
/// an integer hash: the same on every run.
#[cfg(feature = "material-catalogue")]
pub fn lawn() -> Mesh {
    const BLADES: u32 = 7500;
    const RIM: u32 = 800;
    let mut m = slope();
    let hash = |i: u32, k: u32| -> f32 {
        let mut x = i.wrapping_mul(0x9E37_79B1) ^ k.wrapping_mul(0x85EB_CA77);
        x ^= x >> 15;
        x = x.wrapping_mul(0x2C1B_3C6D);
        x ^= x >> 12;
        x = x.wrapping_mul(0x297A_2D39);
        x ^= x >> 15;
        (x >> 8) as f32 / 16_777_216.0
    };
    let edge = SLOPE_HALF - 0.02;
    for i in 0..BLADES + RIM {
        let (x, z) = if i < BLADES {
            ((hash(i, 1) * 2.0 - 1.0) * edge, (hash(i, 2) * 2.0 - 1.0) * edge)
        } else {
            // A rim of blades on all four sides, up to 7 cm outside the bank,
            // so its foot line is not a ruled edge (critic round 11: straight
            // edges, seen from the front the inner blades only break the top).
            let along = (hash(i, 1) * 2.0 - 1.0) * (SLOPE_HALF + 0.05);
            let out = SLOPE_HALF + 0.07 - 0.12 * hash(i, 2);
            match i % 4 {
                0 => (along, out),
                1 => (along, -out),
                2 => (out, along),
                _ => (-out, along),
            }
        };
        // Past the bank's edge the blade stands on the floor under it.
        let inside = x.abs() < SLOPE_HALF && z.abs() < SLOPE_HALF;
        let y = if inside { slope_h(x, z) } else { 0.0 } - 0.15 - 0.004;
        // 4.5 to 8 cm: a mown lawn (critic round 12: long for a short lawn).
        let height = 0.045 + 0.035 * hash(i, 3);
        // Mowing stripes 0.5 m wide along z: the blades lean one way or the
        // other, each a little off.
        let stripe = if (x / 0.5).floor() as i32 % 2 == 0 { 1.0 } else { -1.0 };
        let lean_a = (hash(i, 4) - 0.5) * 1.4;
        let lean = 0.25 * height * (0.4 + 0.6 * hash(i, 5));
        let (lx, lz) = (lean_a.sin() * lean, stripe * lean_a.cos() * lean);
        let turn = hash(i, 6) * core::f32::consts::TAU;
        let (c, s) = (turn.cos(), turn.sin());
        // A flat blade's foot: wide one way, thin the other.
        let foot = [[0.011, 0.0], [-0.011, 0.0], [0.0, 0.005]].map(|[a, b]| [x + a * c - b * s, y, z + a * s + b * c]);
        let tip = [x + lx, y + height, z + lz];
        let faces = [[foot[0], foot[1], tip], [foot[1], foot[2], tip], [foot[2], foot[0], tip], [foot[0], foot[2], foot[1]]];
        let centre = [x + lx * 0.25, y + height * 0.25, z + lz * 0.25];
        for f in faces {
            let e1 = [f[1][0] - f[0][0], f[1][1] - f[0][1], f[1][2] - f[0][2]];
            let e2 = [f[2][0] - f[0][0], f[2][1] - f[0][1], f[2][2] - f[0][2]];
            let mut n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
            let mid = [(f[0][0] + f[1][0] + f[2][0]) / 3.0, (f[0][1] + f[1][1] + f[2][1]) / 3.0, (f[0][2] + f[1][2] + f[2][2]) / 3.0];
            // Wound outward from the spike's middle.
            let out = (mid[0] - centre[0]) * n[0] + (mid[1] - centre[1]) * n[1] + (mid[2] - centre[2]) * n[2];
            let f = if out < 0.0 {
                n = [-n[0], -n[1], -n[2]];
                [f[0], f[2], f[1]]
            } else {
                f
            };
            let n = normalize(n, [0.0, 1.0, 0.0]);
            let base = m.positions.len() as u32;
            for p in f {
                m.positions.push(p);
                m.normals.push(n);
            }
            m.indices.extend([base, base + 1, base + 2]);
        }
    }
    m
}

/// A flat card facing +z, 1.1 wide and 1.5 tall (about radius 1), for leaves,
/// needle sprays and grass tufts: it is cut to the leaf's outline by an alpha
/// mask texture, so it is drawn double sided (B2-P8).
#[cfg(feature = "material-catalogue")]
pub fn leaf_card() -> Mesh {
    let mut m = Mesh::default();
    for (x, y) in [(-0.55f32, -0.75f32), (0.55, -0.75), (0.55, 0.75), (-0.55, 0.75)] {
        m.positions.push([x, y, 0.0]);
        m.normals.push([0.0, 0.0, 1.0]);
    }
    m.indices.extend([0u32, 1, 2, 0, 2, 3]);
    m
}

/// A rose petal card (critic round 1): the leaf card's box (1.1 wide, 1.5
/// tall, facing +z), cupped across (the sides rise towards the viewer) and
/// its top rolled back a little, so the petal mask (`masks.rs`) cuts a petal
/// that curves like one. Drawn double sided; normals follow the curve.
#[cfg(feature = "material-catalogue")]
pub fn petal_card() -> Mesh {
    const NX: usize = 8;
    const NY: usize = 10;
    let mut m = Mesh::default();
    for j in 0..=NY {
        let y = -0.75 + 1.5 * j as f32 / NY as f32;
        for i in 0..=NX {
            let x = -0.55 + 1.1 * i as f32 / NX as f32;
            let across = x / 0.55;
            let top = ((y - 0.3) / 0.45).max(0.0);
            m.positions.push([x, y, 0.22 * across * across - 0.12 * top * top]);
        }
    }
    for j in 0..NY {
        for i in 0..NX {
            let a = (j * (NX + 1) + i) as u32;
            let b = a + (NX + 1) as u32;
            m.indices.extend([a, a + 1, b + 1, a, b + 1, b]);
        }
    }
    m.normals = face_normals(&m, [0.0, 0.0, 1.0]);
    m
}

/// A leaf blade made of its outline (no mask): 1 long along +x (base at
/// x = -0.5, tip at 0.5), the leaf card's outline across (at most 0.5 wide),
/// its top facing +z with its two halves folded up by `fold` at the widest
/// edges (less where it narrows),
/// and an underside that meets the top at the outline and sinks to `thick`
/// below it along the midrib: a closed thin solid (every edge shared by two
/// triangles), so it needs no double-sided drawing and has no open edge.
/// For a scene leaf drawn as a plain model (the garden's leaf on a stem).
#[cfg(feature = "models")]
pub fn leaf_blade(fold: f32, thick: f32) -> Mesh {
    const N: usize = 12;
    let half = |s: f32| 0.25 * (s * core::f32::consts::PI).sin().powf(0.75) * (1.0 - 0.25 * s);
    let mut m = Mesh::default();
    // The base point, N - 1 rows of (lower edge, top midrib, upper edge,
    // underside midrib), the tip.
    m.positions.push([-0.5, 0.0, 0.0]);
    for k in 1..N {
        let s = k as f32 / N as f32;
        let (x, w) = (s - 0.5, half(s));
        // The midrib is thickest in the middle of the leaf.
        let t = thick * (s * core::f32::consts::PI).sin();
        // The edges rise with the width, so the narrow ends stay gentle.
        let z = fold * w / 0.25;
        m.positions.extend([[x, -w, z], [x, 0.0, 0.0], [x, w, z], [x, 0.0, -t]]);
    }
    m.positions.push([0.5, 0.0, 0.0]);
    let tip = (m.positions.len() - 1) as u32;
    // A corner: the base, the tip, or row k's lower edge, midrib or upper
    // edge. The midrib is the top's on the top and the underside's below.
    #[derive(Clone, Copy)]
    enum C {
        Base,
        Tip,
        Lo(usize),
        Mid(usize),
        Hi(usize),
    }
    let at = |c: C, under: bool| -> u32 {
        let row = |k: usize| 1 + 4 * (k as u32 - 1);
        match c {
            C::Base => 0,
            C::Tip => tip,
            C::Lo(k) => row(k),
            C::Mid(k) => row(k) + if under { 3 } else { 1 },
            C::Hi(k) => row(k) + 2,
        }
    };
    // The top's triangles, counter-clockwise seen from +z.
    let mut top = vec![[C::Base, C::Lo(1), C::Mid(1)], [C::Base, C::Mid(1), C::Hi(1)]];
    for a in 1..N - 1 {
        let b = a + 1;
        top.extend([[C::Lo(a), C::Lo(b), C::Mid(b)], [C::Lo(a), C::Mid(b), C::Mid(a)], [C::Mid(a), C::Mid(b), C::Hi(b)], [C::Mid(a), C::Hi(b), C::Hi(a)]]);
    }
    top.extend([[C::Lo(N - 1), C::Tip, C::Mid(N - 1)], [C::Mid(N - 1), C::Tip, C::Hi(N - 1)]]);
    for [p, q, r] in top {
        m.indices.extend([at(p, false), at(q, false), at(r, false)]);
        // The underside: the same corners, wound the other way.
        m.indices.extend([at(p, true), at(r, true), at(q, true)]);
    }
    m.normals = face_normals(&m, [0.0, 0.0, 1.0]);
    m
}

/// Per-vertex normals: the area-weighted sum of the triangles' normals at
/// each vertex (`fallback` where they cancel).
#[cfg(any(feature = "models", feature = "material-catalogue"))]
fn face_normals(m: &Mesh, fallback: [f32; 3]) -> Vec<[f32; 3]> {
    let mut n = vec![[0.0f32; 3]; m.positions.len()];
    for t in m.indices.as_chunks::<3>().0 {
        let [a, b, c] = [t[0] as usize, t[1] as usize, t[2] as usize];
        let f = cross(sub(m.positions[b], m.positions[a]), sub(m.positions[c], m.positions[a]));
        for v in [a, b, c] {
            n[v] = add(n[v], f);
        }
    }
    n.into_iter().map(|v| normalize(v, fallback)).collect()
}

/// A rock (B2-P8): a sphere pushed in and out by a few smooth waves, so the
/// outline is lumpy and the stone's relief reads on a surface that isn't a
/// ball. Normals follow the displaced surface (no seam).
#[cfg(feature = "material-catalogue")]
pub fn rock() -> Mesh {
    let wave = |p: [f32; 3]| 0.07 * (3.1 * p[0] + 1.3).sin() * (2.7 * p[1] + 0.4).cos() + 0.05 * (4.3 * p[2] - 0.7).sin() * (3.7 * p[0] + 2.1).sin() + 0.04 * (5.9 * p[1] + 5.0 * p[2]).cos();
    let mut m = build(Shape::Sphere).unwrap_or_default();
    for (pos, nor) in m.positions.iter_mut().zip(m.normals.iter_mut()) {
        let u = normalize(*pos, [0.0, 1.0, 0.0]);
        // The wave's slope along the surface, by central differences.
        let e = 0.01;
        let g = [0, 1, 2].map(|k| {
            let (mut a, mut b) = (u, u);
            a[k] += e;
            b[k] -= e;
            (wave(a) - wave(b)) / (2.0 * e)
        });
        let g = sub(g, scale(u, dot(g, u)));
        let f = wave(u);
        *pos = scale(u, 1.0 + f);
        *nor = normalize(sub(u, scale(g, 1.0 / (1.0 + f))), u);
    }
    m
}

/// A cluster of quartz points (material-view round 4): three six-sided
/// prisms, each ending in a six-faced point, the tall one upright and two
/// shorter ones leaning out from its foot, flat-shaded so every face reads
/// as a cut facet. Bottom at y = -0.8, about radius 1.
#[cfg(feature = "material-catalogue")]
pub fn crystal() -> Mesh {
    let mut m = Mesh::default();
    // (radius, prism height, point height, lean (radians), lean direction,
    // turn about its own axis)
    let points = [(0.36f32, 0.95f32, 0.5f32, 0.0f32, 0.0f32, 0.2f32), (0.27, 0.6, 0.36, 0.55, 2.4, 0.6), (0.24, 0.45, 0.32, 0.65, -0.7, 0.0)];
    for (r, hb, ht, lean, dir, turn) in points {
        let place = |p: [f32; 3]| {
            // Lean about the horizontal axis at right angles to `dir`, from
            // the foot, then set the foot on the cluster's base.
            let (s, c) = lean.sin_cos();
            let (ds, dc) = dir.sin_cos();
            let along = p[0] * dc + p[2] * ds;
            let across = -p[0] * ds + p[2] * dc;
            let (al, y) = (along * c + p[1] * s, -along * s + p[1] * c);
            [al * dc - across * ds, y - 0.8, al * ds + across * dc]
        };
        let ring = |y: f32| -> Vec<[f32; 3]> { (0..6).map(|k| (k as f32 * core::f32::consts::FRAC_PI_3 + turn).sin_cos()).map(|(s, c)| place([r * c, y, r * s])).collect() };
        let (lo, hi, apex) = (ring(0.0), ring(hb), place([0.0, hb + ht, 0.0]));
        let mut face = |pts: &[[f32; 3]]| {
            let at = m.positions.len() as u32;
            let n = normalize(cross(sub(pts[1], pts[0]), sub(pts[2], pts[0])), [0.0, 1.0, 0.0]);
            for p in pts {
                m.positions.push(*p);
                m.normals.push(n);
            }
            for k in 1..pts.len() as u32 - 1 {
                m.indices.extend([at, at + k, at + k + 1]);
            }
        };
        // Wound counter-clockwise seen from outside (the ring turns from +x
        // to +z, so a side goes down its near edge and up its far one).
        for k in 0..6 {
            let j = (k + 1) % 6;
            face(&[lo[k], hi[k], hi[j], lo[j]]);
            face(&[hi[k], apex, hi[j]]);
        }
        face(&lo);
    }
    m
}

/// A slime blob (material-view round 4): a sagging mound, wider at its foot
/// and flat where it rests, with a slow wobble over it and three drips
/// running down its sides. Bottom at y = -0.62, about radius 1. Normals
/// follow the surface.
#[cfg(feature = "material-catalogue")]
pub fn blob() -> Mesh {
    let mut m = build(Shape::Sphere).unwrap_or_default();
    for p in m.positions.iter_mut() {
        let u = normalize(*p, [0.0, 1.0, 0.0]);
        let a = u[2].atan2(u[0]);
        let drips: f32 = [0.4f32, 2.5, 4.4].iter().map(|&d| (-((a - d + core::f32::consts::PI).rem_euclid(core::f32::consts::TAU) - core::f32::consts::PI).powi(2) / 0.05).exp()).sum();
        let side = (1.0 - u[1].abs()).max(0.0);
        let r = 1.0 + 0.05 * (3.0 * a + 2.0 * u[1]).sin() + 0.04 * (5.0 * a - 3.0 * u[1]).cos() + 0.12 * drips * side * (0.5 - 0.5 * u[1]);
        let low = (-u[1]).max(0.0);
        let spread = 1.0 + 0.32 * low * low;
        *p = [u[0] * r * spread, (u[1] * 0.68 * r).max(-0.62 + 0.02 * low), u[2] * r * spread];
    }
    m.normals = face_normals(&m, [0.0, 1.0, 0.0]);
    m
}

/// A unit sphere's point `u` pushed out to a superellipsoid: half sizes `r`,
/// each axis rounded by its exponent `e` (1 a ball, towards 0 a box), then
/// tilted back by `tilt` radians about x (its +z face turns up towards a
/// camera looking down at it).
#[cfg(feature = "material-catalogue")]
fn squircle(u: [f32; 3], r: [f32; 3], e: [f32; 3], tilt: f32) -> [f32; 3] {
    let p: [f32; 3] = std::array::from_fn(|k| u[k].signum() * u[k].abs().powf(e[k]) * r[k]);
    let (s, c) = tilt.sin_cos();
    [p[0], p[1] * c + p[2] * s, -p[1] * s + p[2] * c]
}

/// A cut of meat or fish (material-view critic round 6: on a ball the grain
/// met at the poles and read as a striped beach ball): a slab with rounded
/// edges, its broad face to the viewer and leant back a little, so the grain
/// runs straight across it. The viewer maps its uvs flat by facing.
#[cfg(feature = "material-catalogue")]
pub fn slab() -> Mesh {
    let mut m = build(Shape::Sphere).unwrap_or_default();
    for p in m.positions.iter_mut() {
        *p = squircle(normalize(*p, [0.0, 1.0, 0.0]), [1.0, 0.68, 0.24], [0.45, 0.45, 0.6], 0.3);
    }
    m.normals = face_normals(&m, [0.0, 0.0, 1.0]);
    m
}

/// A pie (critic round 6: a pie with no crust): a round, flat-topped
/// pastry with a rounded rim, tilted so its top faces a viewer looking down
/// at it a little, where the lattice reads. The viewer maps its uvs flat by
/// facing.
#[cfg(feature = "material-catalogue")]
pub fn pie() -> Mesh {
    let mut m = build(Shape::Sphere).unwrap_or_default();
    for p in m.positions.iter_mut() {
        // Flat top and bottom, a rounded rim; the top turned to face +z and
        // up (about 57 degrees).
        *p = squircle(normalize(*p, [0.0, 1.0, 0.0]), [1.0, 0.3, 1.0], [0.8, 0.35, 0.8], -1.0);
    }
    m.normals = face_normals(&m, [0.0, 0.0, 1.0]);
    m
}

/// A rose in bloom (material-view round 4): 17 cupped petal cards (the
/// petal card's grid, each cut by the petal mask through its own uvs) in
/// four rings round the axis, a tight bud of three at the centre opening to
/// five broad outer petals laid back, each petal turned by the golden angle
/// from the last. Returns the mesh and its uvs (each petal's own card
/// space, as the mask wants). Double sided; normals follow the cups.
#[cfg(feature = "material-catalogue")]
pub fn rose() -> (Mesh, Vec<[f32; 2]>) {
    const NX: usize = 8;
    const NY: usize = 10;
    let mut m = Mesh::default();
    let mut uvs = Vec::new();
    // (petals, scale, lean back from upright (radians), cup, distance from
    // the axis, height of the petal's foot)
    let rings = [(3usize, 0.46f32, 0.06f32, 0.46f32, 0.03f32, 0.08f32), (4, 0.6, 0.28, 0.38, 0.07, 0.03), (5, 0.74, 0.52, 0.3, 0.11, -0.03), (5, 0.86, 0.82, 0.24, 0.15, -0.09)];
    let mut turn = 0.0f32;
    for (count, s, lean, cup, out, foot) in rings {
        for _ in 0..count {
            turn += 2.399_963;
            let (ys, yc) = turn.sin_cos();
            let (ls, lc) = lean.sin_cos();
            let at = m.positions.len() as u32;
            for j in 0..=NY {
                let y = -0.75 + 1.5 * j as f32 / NY as f32;
                for i in 0..=NX {
                    let x = -0.55 + 1.1 * i as f32 / NX as f32;
                    uvs.push([x / 1.1 + 0.5, 0.5 - y / 1.5]);
                    // The card from its foot: cupped across towards the
                    // axis (+z), its rim rolled back outwards.
                    let across = x / 0.55;
                    let up = (y + 0.75) * s;
                    let top = ((y - 0.25) / 0.5).max(0.0);
                    let z = s * (cup * across * across - 0.16 * top * top);
                    // Lean back (the top away from the axis), then turn.
                    let (py, pz) = (up * lc + z * ls, -up * ls + z * lc);
                    let (px, pz) = (x * s, pz - out);
                    m.positions.push([px * yc - pz * ys, py + foot - 0.35, px * ys + pz * yc]);
                }
            }
            for j in 0..NY {
                for i in 0..NX {
                    let a = at + (j * (NX + 1) + i) as u32;
                    let b = a + (NX + 1) as u32;
                    m.indices.extend([a, a + 1, b + 1, a, b + 1, b]);
                }
            }
        }
    }
    m.normals = face_normals(&m, [0.0, 1.0, 0.0]);
    (m, uvs)
}

/// How finely the bump noise varies over the surface (bumps per unit).
const BUMP_FREQUENCY: f32 = 6.0;
/// How far a unit of noise slope tilts the normal at strength 1.
const BUMP_TILT: f32 = 0.06;

/// Tilts the normals by a deterministic 3D noise field (the slope of smooth
/// value noise at position times about 6). Strength 0.0 returns the normals
/// unchanged (bit-identical); 1.0 is a strong, clearly visible bumpy look.
/// Normals stay unit length; positions and indices are not touched.
pub fn bumped(mesh: &Mesh, strength: f32) -> Mesh {
    let mut out = mesh.clone();
    if strength == 0.0 || !strength.is_finite() {
        return out;
    }
    for (n, p) in out.normals.iter_mut().zip(&mesh.positions) {
        let g = noise_slope(*p);
        // Keep only the part of the slope along the surface.
        let g = sub(g, scale(*n, dot(g, *n)));
        *n = normalize(sub(*n, scale(g, strength * BUMP_TILT)), *n);
    }
    out
}

/// The relief a style material's bump draws (critic round 3: grass, fabric
/// and leather all read as smooth balls under one soft noise). Each is a
/// real displacement with its own pattern, the normals following it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Relief {
    /// The soft all-purpose noise of [`bumped`] (stone, plaster...).
    Noise,
    /// Upright blade streaks (grass, hay).
    Blades,
    /// Threads crossing over and under (fabric, cloth).
    Weave,
    /// Pebbled grain cut by fine creases (leather, hide).
    Grain,
}

impl Relief {
    /// The relief a style material draws, by its id.
    pub fn of(material: &str) -> Self {
        match material {
            "grass" | "hay" | "moss" => Self::Blades,
            "fabric" | "cloth" | "cotton" | "wool" => Self::Weave,
            "leather" | "hide" => Self::Grain,
            _ => Self::Noise,
        }
    }
}

/// The relief's height (about -1..1) at a point of a radius-1 shape.
fn relief_field(kind: Relief, p: [f32; 3]) -> f32 {
    match kind {
        Relief::Noise => bump_field(scale(p, BUMP_FREQUENCY)),
        Relief::Blades => {
            // Narrow upright ridges: noise stretched along y, folded sharp.
            let q = [p[0] * 16.0, p[1] * 3.0, p[2] * 16.0];
            let ridge = 1.0 - (2.0 * value_noise(q) - 1.0).abs();
            ridge * 0.8 + 0.4 * value_noise([q[0] * 2.1 + 5.3, q[1] * 1.7, q[2] * 2.1 + 1.9]) - 0.6
        }
        Relief::Weave => {
            // Warp and weft round the shape: each thread rises over one
            // crossing and dips under the next.
            let lon = p[2].atan2(p[0]);
            let lat = p[1].clamp(-1.0, 1.0).asin();
            let (a, b) = (lon * 22.0, lat * 22.0);
            let warp = a.sin().abs() * (b * 0.5).cos();
            let weft = b.sin().abs() * (a * 0.5 + core::f32::consts::FRAC_PI_2).cos();
            // Fades out at the poles, where the threads' rings bunch up.
            let r2 = p[0] * p[0] + p[2] * p[2];
            (warp + weft) * r2 * r2
        }
        Relief::Grain => {
            let pebble = value_noise(scale(p, 22.0));
            let crease = 1.0 - (2.0 * value_noise([p[0] * 7.0 + 3.1, p[1] * 7.0 + 8.2, p[2] * 7.0 + 0.7]) - 1.0).abs();
            pebble - 0.7 * crease.powi(8)
        }
    }
}

/// How deep each relief cuts (radius-1 units at strength 1) and how hard its
/// slope tilts the normals.
fn relief_depth(kind: Relief) -> (f32, f32) {
    match kind {
        Relief::Noise => (0.0, BUMP_TILT),
        // Blades and threads only tilt the light: displaced, their fine
        // ridges overlapped at grazing angles and flickered (geocheck).
        Relief::Blades => (0.0, 0.018),
        Relief::Weave => (0.0, 0.02),
        Relief::Grain => (0.006, 0.026),
    }
}

/// `mesh` with relief `kind` at `strength`: [`bumped`] for the plain noise,
/// else displaced along the normals with the normals tilted to match.
pub fn relief(mesh: &Mesh, kind: Relief, strength: f32) -> Mesh {
    if kind == Relief::Noise || strength == 0.0 || !strength.is_finite() {
        return bumped(mesh, strength);
    }
    let (depth, tilt) = relief_depth(kind);
    let mut out = mesh.clone();
    const H: f32 = 1e-3;
    for ((pos, nor), (p, n)) in out.positions.iter_mut().zip(out.normals.iter_mut()).zip(mesh.positions.iter().zip(&mesh.normals)) {
        let mut g = [0.0; 3];
        for (k, gk) in g.iter_mut().enumerate() {
            let (mut a, mut b) = (*p, *p);
            a[k] += H;
            b[k] -= H;
            *gk = (relief_field(kind, a) - relief_field(kind, b)) / (2.0 * H);
        }
        let g = sub(g, scale(*n, dot(g, *n)));
        // Tilt at most 35 degrees: a steep crease must not turn a normal
        // past its triangle (geocheck: wound against its normals).
        let t = scale(g, strength * tilt);
        let tl = dot(t, t).sqrt();
        let t = if tl > 0.7 { scale(t, 0.7 / tl) } else { t };
        *pos = add(*p, scale(*n, relief_field(kind, *p) * depth * strength));
        *nor = normalize(sub(*n, t), *n);
    }
    // Where the relief would fold a small triangle over its neighbours (near
    // a sphere's poles, on a coarse shape), its corners keep their plain
    // place and normal: no triangle faces against the surface round it
    // (geocheck compares each winding with area-weighted vertex normals).
    for _ in 0..64 {
        let mut undo = Vec::new();
        let geo = area_normals(&out);
        for tri in out.indices.chunks(3) {
            let [a, b, c] = [tri[0], tri[1], tri[2]].map(|i| i as usize);
            let face = cross(sub(out.positions[b], out.positions[a]), sub(out.positions[c], out.positions[a]));
            let n = add(add(geo[a], geo[b]), geo[c]);
            let tilted = add(add(out.normals[a], out.normals[b]), out.normals[c]);
            if dot(face, face) > 1e-24 && (dot(face, n) <= 0.0 || dot(face, tilted) <= 0.0) {
                undo.extend([a, b, c]);
            }
        }
        if undo.is_empty() {
            break;
        }
        for v in undo {
            out.positions[v] = mesh.positions[v];
            out.normals[v] = mesh.normals[v];
        }
    }
    out
}

/// Area-weighted vertex normals from the triangles alone (as geocheck
/// derives them from a model's positions).
pub(crate) fn area_normals(m: &Mesh) -> Vec<[f32; 3]> {
    let mut n = vec![[0.0f32; 3]; m.positions.len()];
    for tri in m.indices.chunks(3) {
        let [a, b, c] = [tri[0], tri[1], tri[2]].map(|i| i as usize);
        let f = cross(sub(m.positions[b], m.positions[a]), sub(m.positions[c], m.positions[a]));
        for v in [a, b, c] {
            n[v] = add(n[v], f);
        }
    }
    n.into_iter().map(|v| normalize(v, [0.0, 1.0, 0.0])).collect()
}

/// The sphere a patterned relief is drawn on: fine enough for blades and
/// threads (the plain sphere's 96 x 48 would smear them).
pub fn relief_sphere() -> Mesh {
    let mut b = Builder::default();
    b.sphere(1.0, 288, 144);
    b.mesh
}

/// A plain metal/rough material for [`glb`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pbr {
    pub base: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
    pub emissive: [f32; 3],
}

/// A complete .glb: one scene, one node, one mesh, one primitive (POSITION,
/// NORMAL, u32 indices) and one material with baseColorFactor,
/// metallicFactor, roughnessFactor and emissiveFactor. Values are clamped to
/// valid glTF ranges (0 to 1; emissive too, as the engine's reader has no
/// emissive-strength extension). A base alpha below 1 turns on blending.
///
/// The engine's reader needs only POSITION on a triangle primitive; NORMAL is
/// written so the shapes keep their own (flat or bumped) normals. No UVs or
/// tangents: these materials use no textures.
pub fn glb(mesh: &Mesh, mat: &Pbr) -> Vec<u8> {
    glb_parts(&[(mesh, mat, false)])
}

/// A .glb of several parts in one mesh: one primitive and one material per
/// part, in order (a style's light bands, its outline...). The flag draws
/// both faces of that part (`doubleSided`); single-sided parts lose their
/// back faces in the engine's model shaders, which is what an inside-out
/// outline shell needs. Empty parts keep their material but get no
/// primitive. One part is exactly [`glb`]'s file.
pub fn glb_parts(parts: &[(&Mesh, &Pbr, bool)]) -> Vec<u8> {
    let mut bin = Vec::new();
    let mut views: Vec<(usize, usize, u32)> = Vec::new();
    let mut accessors: Vec<String> = Vec::new();
    let mut primitives: Vec<String> = Vec::new();
    let mut materials: Vec<String> = Vec::new();
    for (part, (mesh, mat, double_sided)) in parts.iter().enumerate() {
        materials.push(material_json(mat, *double_sided));
        let n = mesh.positions.len();
        if n == 0 {
            continue;
        }
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        let start = bin.len();
        for p in &mesh.positions {
            for k in 0..3 {
                let v = finite(p[k]);
                lo[k] = lo[k].min(v);
                hi[k] = hi[k].max(v);
                bin.extend_from_slice(&v.to_le_bytes());
            }
        }
        views.push((start, bin.len() - start, 34962));
        let mut attributes = format!(r#""POSITION":{}"#, accessors.len());
        accessors.push(format!(
            r#"{{"bufferView":{},"componentType":5126,"count":{n},"type":"VEC3","min":[{},{},{}],"max":[{},{},{}]}}"#,
            views.len() - 1,
            num(lo[0]),
            num(lo[1]),
            num(lo[2]),
            num(hi[0]),
            num(hi[1]),
            num(hi[2]),
        ));
        if mesh.normals.len() == n {
            let start = bin.len();
            for v in mesh.normals.iter().flatten() {
                bin.extend_from_slice(&finite(*v).to_le_bytes());
            }
            views.push((start, bin.len() - start, 34962));
            attributes.push_str(&format!(r#","NORMAL":{}"#, accessors.len()));
            accessors.push(format!(r#"{{"bufferView":{},"componentType":5126,"count":{n},"type":"VEC3"}}"#, views.len() - 1));
        }
        let mut indices = String::new();
        if !mesh.indices.is_empty() {
            let start = bin.len();
            for i in &mesh.indices {
                bin.extend_from_slice(&i.to_le_bytes());
            }
            views.push((start, bin.len() - start, 34963));
            indices = format!(r#","indices":{}"#, accessors.len());
            accessors.push(format!(
                r#"{{"bufferView":{},"componentType":5125,"count":{},"type":"SCALAR"}}"#,
                views.len() - 1,
                mesh.indices.len()
            ));
        }
        primitives.push(format!(r#"{{"attributes":{{{attributes}}}{indices},"material":{part},"mode":4}}"#));
    }
    pad(&mut bin, 0);

    let mut json = String::from(
        r#"{"asset":{"version":"2.0","generator":"Evani.Dev material-view"},"scene":0,"scenes":[{"nodes":[0]}],"#,
    );
    let materials = if materials.is_empty() { String::new() } else { format!(r#","materials":[{}]"#, materials.join(",")) };
    if primitives.is_empty() {
        json.push_str(&format!(r#""nodes":[{{"name":"shape"}}]{materials}}}"#));
    } else {
        let views: Vec<String> = views
            .iter()
            .map(|(o, l, t)| {
                format!(r#"{{"buffer":0,"byteOffset":{o},"byteLength":{l},"target":{t}}}"#)
            })
            .collect();
        json.push_str(&format!(
            r#""nodes":[{{"name":"shape","mesh":0}}],"meshes":[{{"name":"shape","primitives":[{}]}}]{materials},"accessors":[{}],"bufferViews":[{}],"buffers":[{{"byteLength":{}}}]}}"#,
            primitives.join(","),
            accessors.join(","),
            views.join(","),
            bin.len(),
        ));
    }
    let mut json = json.into_bytes();
    pad(&mut json, b' ');

    let with_bin = !primitives.is_empty();
    let total = 12 + 8 + json.len() + if with_bin { 8 + bin.len() } else { 0 };
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x4E4F_534Au32.to_le_bytes());
    out.extend_from_slice(&json);
    if with_bin {
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(&0x004E_4942u32.to_le_bytes());
        out.extend_from_slice(&bin);
    }
    out
}

/// One glTF material object (factors clamped; `doubleSided` only when set).
pub(crate) fn material_json(mat: &Pbr, double_sided: bool) -> String {
    let base = mat.base.map(unit);
    let emissive = mat.emissive.map(unit);
    let alpha = if base[3] < 1.0 { r#","alphaMode":"BLEND""# } else { "" };
    let sides = if double_sided { r#","doubleSided":true"# } else { "" };
    format!(
        r#"{{"name":"material","pbrMetallicRoughness":{{"baseColorFactor":[{},{},{},{}],"metallicFactor":{},"roughnessFactor":{}}},"emissiveFactor":[{},{},{}]{alpha}{sides}}}"#,
        num(base[0]),
        num(base[1]),
        num(base[2]),
        num(base[3]),
        num(unit(mat.metallic)),
        num(unit(mat.roughness)),
        num(emissive[0]),
        num(emissive[1]),
        num(emissive[2]),
    )
}

// ---------------------------------------------------------------- builder

#[derive(Default)]
struct Builder {
    mesh: Mesh,
}

impl Builder {
    fn vertex(&mut self, p: [f32; 3], n: [f32; 3]) -> u32 {
        self.mesh.positions.push(p);
        self.mesh.normals.push(n);
        (self.mesh.positions.len() - 1) as u32
    }

    /// Adds one triangle, wound so its face points the way its vertex
    /// normals do. Triangles with no area (at a pole) are left out.
    fn tri(&mut self, a: u32, b: u32, c: u32) {
        let m = &self.mesh;
        let (pa, pb, pc) = (
            m.positions[a as usize],
            m.positions[b as usize],
            m.positions[c as usize],
        );
        let face = cross(sub(pb, pa), sub(pc, pa));
        if dot(face, face) < 1e-14 {
            return;
        }
        let along = add(
            add(m.normals[a as usize], m.normals[b as usize]),
            m.normals[c as usize],
        );
        if dot(face, along) >= 0.0 {
            self.mesh.indices.extend([a, b, c]);
        } else {
            self.mesh.indices.extend([a, c, b]);
        }
    }

    /// A `nu` by `nv` grid of cells; `f(u, v)` gives position and normal
    /// for u and v in 0..=1.
    fn grid(&mut self, nu: u32, nv: u32, f: impl Fn(f32, f32) -> ([f32; 3], [f32; 3])) {
        let base = self.mesh.positions.len() as u32;
        for j in 0..=nv {
            for i in 0..=nu {
                let (p, n) = f(i as f32 / nu as f32, j as f32 / nv as f32);
                self.vertex(p, n);
            }
        }
        let row = nu + 1;
        for j in 0..nv {
            for i in 0..nu {
                let a = base + j * row + i;
                let (b, c) = (a + 1, a + row);
                self.tri(a, b, c + 1);
                self.tri(a, c + 1, c);
            }
        }
    }

    /// A flat triangle a, b, c cut into `n` steps per edge.
    fn tri_grid(&mut self, a: [f32; 3], b: [f32; 3], c: [f32; 3], normal: [f32; 3], n: u32) {
        let (ab, ac) = (sub(b, a), sub(c, a));
        let mut rows = Vec::with_capacity(n as usize + 1);
        for i in 0..=n {
            rows.push(self.mesh.positions.len() as u32);
            for j in 0..=(n - i) {
                let p = add(
                    a,
                    add(
                        scale(ab, i as f32 / n as f32),
                        scale(ac, j as f32 / n as f32),
                    ),
                );
                self.vertex(p, normal);
            }
        }
        for i in 0..n {
            for j in 0..(n - i) {
                let (r0, r1) = (rows[i as usize], rows[i as usize + 1]);
                self.tri(r0 + j, r1 + j, r0 + j + 1);
                if j + 1 < n - i {
                    self.tri(r1 + j, r1 + j + 1, r0 + j + 1);
                }
            }
        }
    }

    fn sphere(&mut self, r: f32, around: u32, rings: u32) {
        self.grid(around, rings, |u, v| {
            let (t, f) = (u * TAU, v * std::f32::consts::PI);
            let n = [f.sin() * t.cos(), f.cos(), f.sin() * t.sin()];
            (scale(n, r), n)
        });
    }

    /// A box of the given full size, each face cut into cells.
    fn cuboid(&mut self, size: [f32; 3], cells: [u32; 3]) {
        for k in 0..3 {
            let (a, b) = ((k + 1) % 3, (k + 2) % 3);
            for sign in [1.0f32, -1.0] {
                let mut normal = [0.0; 3];
                normal[k] = sign;
                self.grid(cells[a], cells[b], |u, v| {
                    let mut p = [0.0; 3];
                    p[k] = sign * size[k] * 0.5;
                    p[a] = (u - 0.5) * size[a];
                    p[b] = (v - 0.5) * size[b];
                    (p, normal)
                });
            }
        }
    }

    /// An equilateral triangle (corner radius `r`, point up) pushed `len`
    /// along Z; flat-shaded.
    fn prism(&mut self, r: f32, len: f32, cells: u32) {
        let corner = |deg: f32| {
            let a = deg.to_radians();
            [r * a.cos(), r * a.sin(), 0.0]
        };
        let tri = [corner(90.0), corner(210.0), corner(330.0)];
        let h = len * 0.5;
        for e in 0..3 {
            let (a, b) = (tri[e], tri[(e + 1) % 3]);
            let normal = normalize(scale(add(a, b), 0.5), [0.0, 1.0, 0.0]);
            self.grid(cells, cells, |u, v| {
                (
                    add(add(a, scale(sub(b, a), u)), [0.0, 0.0, (v - 0.5) * len]),
                    normal,
                )
            });
        }
        for z in [h, -h] {
            let at = |p: [f32; 3]| [p[0], p[1], z];
            self.tri_grid(
                at(tri[0]),
                at(tri[1]),
                at(tri[2]),
                [0.0, 0.0, z.signum()],
                cells,
            );
        }
    }

    /// Smooth side, flat caps.
    fn cylinder(&mut self, r: f32, height: f32, around: u32, rows: u32, cap_rings: u32) {
        let h = height * 0.5;
        self.grid(around, rows, |u, v| {
            let t = u * TAU;
            let n = [t.cos(), 0.0, t.sin()];
            ([r * n[0], -h + v * height, r * n[2]], n)
        });
        for y in [h, -h] {
            self.grid(around, cap_rings, |u, v| {
                let t = u * TAU;
                (
                    [v * r * t.cos(), y, v * r * t.sin()],
                    [0.0, y.signum(), 0.0],
                )
            });
        }
    }

    /// A ring lying flat around Y.
    fn torus(&mut self, major: f32, minor: f32, around: u32, tube: u32) {
        self.grid(around, tube, |u, v| {
            let (t, f) = (u * TAU, v * TAU);
            let n = [f.cos() * t.cos(), f.sin(), f.cos() * t.sin()];
            (
                [
                    major * t.cos() + minor * n[0],
                    minor * n[1],
                    major * t.sin() + minor * n[2],
                ],
                n,
            )
        });
    }
}

// ------------------------------------------------------------------ noise

/// A repeatable pseudo-random value in -1..=1 for one lattice point.
fn hash(x: i32, y: i32, z: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8DA6_B343)
        ^ (y as u32).wrapping_mul(0xD816_3841)
        ^ (z as u32).wrapping_mul(0xCB1A_B31F);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7FEB_352D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846C_A68B);
    h ^= h >> 16;
    (h >> 8) as f32 / 16_777_215.0 * 2.0 - 1.0
}

/// Smooth value noise (quintic fade between lattice values).
pub(crate) fn value_noise(p: [f32; 3]) -> f32 {
    let i = p.map(f32::floor);
    let f = [p[0] - i[0], p[1] - i[1], p[2] - i[2]];
    let w = f.map(|t| t * t * t * (t * (t * 6.0 - 15.0) + 10.0));
    let (x, y, z) = (i[0] as i32, i[1] as i32, i[2] as i32);
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let mut layer = [0.0f32; 2];
    for (dz, l) in layer.iter_mut().enumerate() {
        let zz = z + dz as i32;
        let near = lerp(hash(x, y, zz), hash(x + 1, y, zz), w[0]);
        let far = lerp(hash(x, y + 1, zz), hash(x + 1, y + 1, zz), w[0]);
        *l = lerp(near, far, w[1]);
    }
    lerp(layer[0], layer[1], w[2])
}

/// Two octaves of value noise.
fn bump_field(q: [f32; 3]) -> f32 {
    value_noise(q) + 0.5 * value_noise([q[0] * 2.03 + 17.1, q[1] * 2.03 + 3.7, q[2] * 2.03 + 9.3])
}

/// The slope of the bump field at a world position (per world unit).
fn noise_slope(p: [f32; 3]) -> [f32; 3] {
    const H: f32 = 2e-3;
    let q = scale(p, BUMP_FREQUENCY);
    let mut g = [0.0; 3];
    for (k, gk) in g.iter_mut().enumerate() {
        let (mut a, mut b) = (q, q);
        a[k] += H;
        b[k] -= H;
        *gk = (bump_field(a) - bump_field(b)) / (2.0 * H) * BUMP_FREQUENCY;
    }
    g
}

// ----------------------------------------------------------------- vectors

pub(crate) fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub(crate) fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub(crate) fn scale(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

pub(crate) fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub(crate) fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// `a` at unit length, or `fallback` if `a` has no length.
pub(crate) fn normalize(a: [f32; 3], fallback: [f32; 3]) -> [f32; 3] {
    let l = dot(a, a).sqrt();
    if l > 1e-12 && l.is_finite() {
        scale(a, 1.0 / l)
    } else {
        fallback
    }
}

// -------------------------------------------------------------------- glb

/// A finite value (anything else becomes 0).
fn finite(x: f32) -> f32 {
    if x.is_finite() { x } else { 0.0 }
}

/// Clamped to 0..=1 (not-a-number becomes 0).
fn unit(x: f32) -> f32 {
    finite(x).clamp(0.0, 1.0)
}

/// A JSON number (Rust's `Display` for f32 never uses exponents).
fn num(x: f32) -> String {
    format!("{}", finite(x))
}

/// Pads to a multiple of 4 bytes.
fn pad(v: &mut Vec<u8>, with: u8) {
    while !v.len().is_multiple_of(4) {
        v.push(with);
    }
}

/// The studio's material ball (studio/balls.rs): a radius 1 sphere, lighter
/// than the test shape (a 128 px picture shows no more), so its build and its
/// sun cones stay cheap. Declared before the tests: no line above them moves.
#[cfg(feature = "material-studio")]
pub fn ball() -> Mesh {
    let mut b = Builder::default();
    b.sphere(1.0, 64, 32);
    b.mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generated() -> Vec<(Shape, Mesh)> {
        Shape::ALL
            .into_iter()
            .filter_map(|s| build(s).map(|m| (s, m)))
            .collect()
    }

    fn len(a: [f32; 3]) -> f32 {
        dot(a, a).sqrt()
    }

    /// Every patterned relief keeps each triangle facing the way its
    /// vertex normals do (geocheck: "wound against their normals"), on the
    /// relief sphere and on the plain shapes.
    #[test]
    fn reliefs_keep_triangles_facing_out() {
        let mut meshes = generated();
        meshes.push((Shape::Sphere, relief_sphere()));
        for (kind, strength) in [Relief::Blades, Relief::Weave, Relief::Grain].into_iter().flat_map(|k| [0.25, 0.4, 0.45, 0.8, 1.0].map(|s| (k, s))) {
            for (s, m) in &meshes {
                let r = relief(m, kind, strength);
                let geo = area_normals(&r);
                let mut bad = Vec::new();
                for (t, tri) in r.indices.chunks(3).enumerate() {
                    let [a, b, c] = [tri[0], tri[1], tri[2]].map(|i| i as usize);
                    let face = cross(sub(r.positions[b], r.positions[a]), sub(r.positions[c], r.positions[a]));
                    if len(face) < 1e-12 {
                        continue;
                    }
                    let n = add(add(r.normals[a], r.normals[b]), r.normals[c]);
                    let g = add(add(geo[a], geo[b]), geo[c]);
                    if dot(face, n) <= 0.0 || dot(face, g) <= 0.0 {
                        bad.push((t, r.positions[a]));
                    }
                }
                let plain = bad.iter().filter(|(t, _)| { let tri = &r.indices[t * 3..t * 3 + 3]; tri.iter().all(|&i| r.positions[i as usize] == m.positions[i as usize]) }).count();
                assert!(bad.is_empty(), "{kind:?} {strength} on {s:?}: {} inverted ({plain} with every corner plain), first {:?}", bad.len(), bad.first());
            }
        }
    }

    #[test]
    fn ids_round_trip() {
        assert_eq!(Shape::ALL[0], Shape::Sphere);
        for s in Shape::ALL {
            assert_eq!(Shape::from_id(s.id()), Some(s));
            assert!(!s.label().is_empty());
        }
        assert_eq!(Shape::from_id("nope"), None);
        assert!(build(Shape::Monkey).is_none());
    }

    #[test]
    fn meshes_are_well_formed() {
        let all = generated();
        assert_eq!(all.len(), 6, "every generated shape builds");
        for (s, m) in &all {
            let n = m.positions.len();
            assert_eq!(m.normals.len(), n, "{s:?}");
            assert_eq!(m.indices.len() % 3, 0, "{s:?}");
            assert!(
                m.indices.iter().all(|&i| (i as usize) < n),
                "{s:?} index in range"
            );
            let tris = m.indices.len() / 3;
            assert!(
                (2_000..=40_000).contains(&tris),
                "{s:?} has {tris} triangles"
            );
            for (p, nn) in m.positions.iter().zip(&m.normals) {
                assert!(p.iter().chain(nn).all(|x| x.is_finite()), "{s:?} finite");
                assert!((len(*nn) - 1.0).abs() < 1e-4, "{s:?} unit normal");
                // Fits a row slot: every coordinate within 1.05; the big
                // cube's corners reach 1.26 from the centre.
                assert!(
                    p.iter().all(|x| x.abs() <= 1.05),
                    "{s:?} fits its slot: {p:?}"
                );
                assert!(len(*p) <= 1.26, "{s:?} radius {}", len(*p));
            }
        }
    }

    #[test]
    fn faces_point_out() {
        for (s, m) in generated() {
            // Every triangle is counter-clockwise seen from where its normals point.
            for t in m.indices.as_chunks::<3>().0 {
                let [a, b, c] = t.map(|i| i as usize);
                let face = cross(
                    sub(m.positions[b], m.positions[a]),
                    sub(m.positions[c], m.positions[a]),
                );
                let along = add(add(m.normals[a], m.normals[b]), m.normals[c]);
                assert!(dot(face, along) > 0.0, "{s:?} winding");
            }
            // Normals point away from the middle.
            let mut sum = 0.0;
            for (p, n) in m.positions.iter().zip(&m.normals) {
                let from = if s == Shape::Ring {
                    sub(
                        *p,
                        scale(normalize([p[0], 0.0, p[2]], [1.0, 0.0, 0.0]), 0.72),
                    )
                } else {
                    *p
                };
                let d = dot(*n, from);
                assert!(d > -1e-5, "{s:?} inward normal at {p:?}");
                sum += d;
            }
            assert!(
                sum / m.positions.len() as f32 > 0.1,
                "{s:?} outward on average"
            );
        }
    }

    #[test]
    fn bump_strength_zero_is_identity_and_one_tilts() {
        for (s, m) in generated() {
            assert_eq!(bumped(&m, 0.0), m, "{s:?}");
            let b = bumped(&m, 1.0);
            assert_eq!(b.positions, m.positions);
            assert_eq!(b.indices, m.indices);
            let mut moved = 0.0;
            for (nb, n) in b.normals.iter().zip(&m.normals) {
                assert!((len(*nb) - 1.0).abs() < 1e-4, "{s:?} bumped unit normal");
                assert!(dot(*nb, *n) > 0.0, "{s:?} bump flips a normal");
                moved += len(sub(*nb, *n));
            }
            let mean = moved / m.normals.len() as f32;
            assert!(mean > 0.15, "{s:?} bump too weak: {mean}");
            assert_eq!(bumped(&m, 1.0), b, "{s:?} deterministic");
        }
    }

    fn u32_at(b: &[u8], at: usize) -> u32 {
        u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
    }

    /// A small structural JSON check: braces and brackets balance outside
    /// strings, and no non-numbers slipped in.
    fn json_balanced(s: &str) -> bool {
        let mut stack = Vec::new();
        let mut in_str = false;
        let mut esc = false;
        for c in s.chars() {
            if in_str {
                match (esc, c) {
                    (true, _) => esc = false,
                    (false, '\\') => esc = true,
                    (false, '"') => in_str = false,
                    _ => {}
                }
                continue;
            }
            match c {
                '"' => in_str = true,
                '{' | '[' => stack.push(c),
                '}' if stack.pop() != Some('{') => return false,
                ']' if stack.pop() != Some('[') => return false,
                _ => {}
            }
        }
        stack.is_empty() && !in_str && !s.contains("inf") && !s.contains("NaN")
    }

    #[test]
    fn glb_layout_is_valid() {
        let mat = Pbr {
            base: [0.8, 0.2, 0.1, 1.0],
            metallic: 2.0,
            roughness: -1.0,
            emissive: [0.0, 5.0, f32::NAN],
        };
        for (s, m) in generated() {
            let g = glb(&m, &mat);
            assert_eq!(&g[0..4], b"glTF");
            assert_eq!(u32_at(&g, 4), 2);
            assert_eq!(u32_at(&g, 8) as usize, g.len());
            let jl = u32_at(&g, 12) as usize;
            assert_eq!(u32_at(&g, 16), 0x4E4F_534A);
            assert_eq!(jl % 4, 0);
            let json = std::str::from_utf8(&g[20..20 + jl]).expect("utf8 json");
            let bl = u32_at(&g, 20 + jl) as usize;
            assert_eq!(u32_at(&g, 24 + jl), 0x004E_4942);
            assert_eq!(bl % 4, 0);
            assert_eq!(28 + jl + bl, g.len());
            assert_eq!(bl, m.positions.len() * 24 + m.indices.len() * 4, "{s:?}");
            assert!(json.trim_end().starts_with('{') && json.trim_end().ends_with('}'));
            assert!(json_balanced(json), "{s:?}: {json}");
            for key in [
                r#""version":"2.0""#,
                r#""POSITION":0"#,
                r#""NORMAL":1"#,
                r#""indices":2"#,
                r#""pbrMetallicRoughness""#,
                r#""baseColorFactor":[0.8,0.2,0.1,1]"#,
                r#""metallicFactor":1"#,
                r#""roughnessFactor":0"#,
                r#""emissiveFactor":[0,1,0]"#,
                r#""min":["#,
                r#""max":["#,
                r#""target":34962"#,
                r#""target":34963"#,
                r#""componentType":5125"#,
                r#""mode":4"#,
            ] {
                assert!(json.contains(key), "{s:?} missing {key}: {json}");
            }
            assert!(!json.contains("alphaMode"));
            assert_eq!(glb(&m, &mat), g, "{s:?} deterministic");
        }
        let see_through = Pbr {
            base: [1.0, 1.0, 1.0, 0.5],
            metallic: 0.0,
            roughness: 0.5,
            emissive: [0.0; 3],
        };
        let g = glb(&generated()[0].1, &see_through);
        let jl = u32_at(&g, 12) as usize;
        assert!(
            std::str::from_utf8(&g[20..20 + jl])
                .is_ok_and(|j| j.contains(r#""alphaMode":"BLEND""#))
        );
    }

    #[test]
    fn empty_mesh_still_makes_a_valid_file() {
        let mat = Pbr {
            base: [1.0; 4],
            metallic: 0.0,
            roughness: 1.0,
            emissive: [0.0; 3],
        };
        let g = glb(&Mesh::default(), &mat);
        assert_eq!(&g[0..4], b"glTF");
        assert_eq!(u32_at(&g, 8) as usize, g.len());
        let jl = u32_at(&g, 12) as usize;
        assert_eq!(20 + jl, g.len());
        assert!(std::str::from_utf8(&g[20..20 + jl]).is_ok_and(json_balanced));
    }

    #[test]
    fn builds_are_deterministic() {
        for s in Shape::ALL {
            assert_eq!(build(s), build(s));
        }
    }

    #[test]
    fn triangle_counts() {
        for (s, m) in generated() {
            println!(
                "{:<10} {:>6} triangles {:>6} vertices",
                s.id(),
                m.indices.len() / 3,
                m.positions.len()
            );
        }
    }
}
