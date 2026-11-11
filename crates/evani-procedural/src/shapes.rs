//! Shared shapes for the library's basic generators (batch A, "1 out of
//! 10"): an icosphere, seeded rocks and blobs, frustums and discs, tubes
//! along a path, blades and leaves, leafy crowns, trunks and branches,
//! scatter patterns, puffs and streaks for weather and effects, and
//! placing copies. Each is plain maths over [`crate::geo`] and
//! [`crate::noise`]: no tables, no third-party code. Ideas from three.js
//! (MIT): IcosahedronGeometry's subdivided icosahedron, TubeGeometry's
//! rings along a path with parallel-transported frames, LatheGeometry's
//! profile turned round an axis; written fresh here.

use crate::geo::{self, Mesh, MeshBuilder, normalize};
use crate::noise;

pub const TAU: f32 = std::f32::consts::TAU;
pub const PI: f32 = std::f32::consts::PI;

/// A seeded number in [0, 1) for item `i`, channel `k`.
pub fn rnd(seed: u32, i: u32, k: u32) -> f32 {
    noise::hash2(i as i32, k as i32, seed)
}

/// A seeded number in [a, b) for item `i`, channel `k`.
pub fn range(seed: u32, i: u32, k: u32, a: f32, b: f32) -> f32 {
    a + (b - a) * rnd(seed, i, k)
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale(a: [f32; 3], k: f32) -> [f32; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// Make every triangle wind counter-clockwise seen from the side its
/// vertex normals point to (build the vertices with the normals you mean,
/// then call this; the renderer culls the other side).
pub fn fix_winding(m: &mut Mesh) {
    let v = &m.vertices;
    for t in m.indices.as_chunks_mut::<3>().0 {
        let (Some(a), Some(b), Some(c)) = (v.get(t[0] as usize), v.get(t[1] as usize), v.get(t[2] as usize)) else { continue };
        let p = |x: &[f32; 6]| [x[0], x[1], x[2]];
        let n = cross(sub(p(b), p(a)), sub(p(c), p(a)));
        let want = [a[3] + b[3] + c[3], a[4] + b[4] + c[4], a[5] + b[5] + c[5]];
        if dot(n, want) < 0.0 {
            t.swap(1, 2);
        }
    }
}

/// Every triangle its own three vertices with the face's normal: flat
/// facets (cut stone, crystals, ice).
pub fn flat_shade(m: &mut Mesh) {
    let mut b = MeshBuilder::new();
    for t in m.indices.as_chunks::<3>().0 {
        let (Some(a), Some(bb), Some(c)) = (m.vertices.get(t[0] as usize), m.vertices.get(t[1] as usize), m.vertices.get(t[2] as usize)) else { continue };
        let p = |x: &[f32; 6]| [x[0], x[1], x[2]];
        let n = normalize(cross(sub(p(bb), p(a)), sub(p(c), p(a))));
        let i = b.vertex(p(a), n);
        b.vertex(p(bb), n);
        b.vertex(p(c), n);
        b.tri(i, i + 1, i + 2);
    }
    *m = b.finish();
}

/// Show from both sides: a copy of every triangle facing the other way.
pub fn double_sided(m: &mut Mesh) {
    let n = m.vertices.len() as u32;
    let back: Vec<[f32; 6]> = m.vertices.iter().map(|v| [v[0], v[1], v[2], -v[3], -v[4], -v[5]]).collect();
    m.vertices.extend(back);
    let idx: Vec<u32> = m.indices.as_chunks::<3>().0.iter().flat_map(|t| [t[0] + n, t[2] + n, t[1] + n]).collect();
    m.indices.extend(idx);
}

/// Move the mesh up or down so its lowest point is at y = 0.
pub fn to_ground(m: &mut Mesh) {
    let (lo, _) = geo::bounds(m);
    for v in &mut m.vertices {
        v[1] -= lo[1];
    }
}

/// Rotate about `axis` through the origin by `angle` (radians).
pub fn rotate(m: &mut Mesh, axis: [f32; 3], angle: f32) {
    let k = normalize(axis);
    let (s, c) = angle.sin_cos();
    let rot = |p: [f32; 3]| add(add(scale(p, c), scale(cross(k, p), s)), scale(k, dot(k, p) * (1.0 - c)));
    for v in &mut m.vertices {
        let p = rot([v[0], v[1], v[2]]);
        let n = rot([v[3], v[4], v[5]]);
        *v = [p[0], p[1], p[2], n[0], n[1], n[2]];
    }
}

/// A placed copy of `m`: scaled by `s`, tilted by `tilt` about x, turned by
/// `yaw` about y, then moved to `at`.
pub fn place(m: &Mesh, s: f32, yaw: f32, tilt: f32, at: [f32; 3]) -> Mesh {
    let mut c = m.clone();
    geo::transform(&mut c, [s; 3], [0.0; 3]);
    if tilt != 0.0 {
        rotate(&mut c, [1.0, 0.0, 0.0], tilt);
    }
    if yaw != 0.0 {
        rotate(&mut c, [0.0, 1.0, 0.0], yaw);
    }
    geo::transform(&mut c, [1.0; 3], at);
    c
}

/// An icosphere of radius 1 round the origin, `sub` times subdivided
/// (0..4), smooth normals.
pub fn icosphere(sub: u32) -> Mesh {
    let t = (1.0 + 5.0f32.sqrt()) * 0.5;
    let mut p: Vec<[f32; 3]> = [
        [-1.0, t, 0.0], [1.0, t, 0.0], [-1.0, -t, 0.0], [1.0, -t, 0.0],
        [0.0, -1.0, t], [0.0, 1.0, t], [0.0, -1.0, -t], [0.0, 1.0, -t],
        [t, 0.0, -1.0], [t, 0.0, 1.0], [-t, 0.0, -1.0], [-t, 0.0, 1.0],
    ]
    .iter()
    .map(|v| normalize(*v))
    .collect();
    let mut f: Vec<[u32; 3]> = vec![
        [0, 11, 5], [0, 5, 1], [0, 1, 7], [0, 7, 10], [0, 10, 11], [1, 5, 9], [5, 11, 4], [11, 10, 2], [10, 7, 6], [7, 1, 8],
        [3, 9, 4], [3, 4, 2], [3, 2, 6], [3, 6, 8], [3, 8, 9], [4, 9, 5], [2, 4, 11], [6, 2, 10], [8, 6, 7], [9, 8, 1],
    ];
    for _ in 0..sub.min(4) {
        let mut mid = std::collections::HashMap::new();
        let mut half = |a: u32, b: u32, p: &mut Vec<[f32; 3]>| -> u32 {
            let key = (a.min(b), a.max(b));
            *mid.entry(key).or_insert_with(|| {
                let (pa, pb) = (p[a as usize], p[b as usize]);
                p.push(normalize(scale(add(pa, pb), 0.5)));
                (p.len() - 1) as u32
            })
        };
        let mut next = Vec::with_capacity(f.len() * 4);
        for [a, b, c] in f {
            let (ab, bc, ca) = (half(a, b, &mut p), half(b, c, &mut p), half(c, a, &mut p));
            next.extend([[a, ab, ca], [b, bc, ab], [c, ca, bc], [ab, bc, ca]]);
        }
        f = next;
    }
    let mut m = Mesh { vertices: p.iter().map(|v| [v[0], v[1], v[2], v[0], v[1], v[2]]).collect(), indices: f.into_iter().flatten().collect() };
    fix_winding(&mut m);
    m
}

/// A lumpy ball: an icosphere of half sizes `half` round `at`, pushed in
/// and out by seeded noise (`lump` 0 round .. 1 very lumpy).
pub fn blob(half: [f32; 3], at: [f32; 3], lump: f32, seed: u32, sub: u32) -> Mesh {
    let mut m = icosphere(sub);
    for v in &mut m.vertices {
        let d = [v[0], v[1], v[2]];
        let k = 1.0 + lump * 0.5 * noise::fbm3(scale(d, 1.6), seed, 3);
        *v = [at[0] + d[0] * k * half[0], at[1] + d[1] * k * half[1], at[2] + d[2] * k * half[2], 0.0, 1.0, 0.0];
    }
    geo::smooth_normals(&mut m);
    m
}

/// A rock `size` (x, y, z extent, m) on the ground: an icosphere pushed by
/// seeded noise (`rough` 0 smooth .. 1 craggy), flattened underneath;
/// `facets` cuts it into flat faces (sharp stone) instead of smooth.
pub fn rock(size: [f32; 3], seed: u32, rough: f32, facets: bool) -> Mesh {
    let mut m = icosphere(if facets { 1 } else { 2 });
    for v in &mut m.vertices {
        let d = [v[0], v[1], v[2]];
        let n = noise::fbm3(scale(d, 1.4), seed, 4);
        let k = 1.0 + rough * 0.42 * n;
        let mut p = scale(d, k);
        // A flatter underside, as a rock settles.
        if p[1] < -0.3 {
            p[1] = -0.3 + (p[1] + 0.3) * 0.25;
        }
        *v = [p[0] * size[0] * 0.5, p[1] * size[1] * 0.5, p[2] * size[2] * 0.5, 0.0, 1.0, 0.0];
    }
    if facets {
        flat_shade(&mut m);
    } else {
        geo::smooth_normals(&mut m);
    }
    to_ground(&mut m);
    m
}

/// A flat disc of radius `r` at height `y`, facing up (`up`) or down.
pub fn disc(r: f32, y: f32, sides: u32, up: bool) -> Mesh {
    let s = sides.clamp(3, 256);
    let n = if up { [0.0, 1.0, 0.0] } else { [0.0, -1.0, 0.0] };
    let mut b = MeshBuilder::new();
    let c = b.vertex([0.0, y, 0.0], n);
    for i in 0..s {
        let a = i as f32 / s as f32 * TAU;
        b.vertex([r * a.cos(), y, -r * a.sin()], n);
    }
    for i in 0..s {
        b.tri(c, c + 1 + i, c + 1 + (i + 1) % s);
    }
    let mut m = b.finish();
    fix_winding(&mut m);
    m
}

/// A frustum from y = 0 (radius `r0`) to y = `h` (radius `r1`; 0 makes a
/// cone), `sides` round, capped.
pub fn frustum(r0: f32, r1: f32, h: f32, sides: u32) -> Mesh {
    let mut m = geo::lathe(&[[r0, 0.0], [r1, h]], sides);
    // The side's normals lean with the slope.
    let slope = (r0 - r1) / h.max(1e-4);
    for v in &mut m.vertices {
        let r = (v[0] * v[0] + v[2] * v[2]).sqrt().max(1e-6);
        let n = normalize([v[0] / r, slope, v[2] / r]);
        v[3] = n[0];
        v[4] = n[1];
        v[5] = n[2];
    }
    fix_winding(&mut m);
    geo::merge(&mut m, &disc(r0, 0.0, sides, false));
    if r1 > 1e-4 {
        geo::merge(&mut m, &disc(r1, h, sides, true));
    }
    m
}

/// A box `size` with its bottom centre at `at`.
pub fn block(size: [f32; 3], at: [f32; 3]) -> Mesh {
    let mut m = geo::segmented_box(size, 1);
    geo::transform(&mut m, [1.0; 3], at);
    m
}

/// A tube along `path` with radius `radius(t)` (t 0 at the start, 1 at
/// the end), `sides` round; the start is capped, a zero end radius closes
/// to a point. Frames are carried along the path so it never twists.
pub fn tube(path: &[[f32; 3]], radius: impl Fn(f32) -> f32, sides: u32) -> Mesh {
    let s = sides.clamp(3, 64);
    let n = path.len();
    if n < 2 {
        return Mesh::default();
    }
    let tangent = |i: usize| normalize(sub(path[(i + 1).min(n - 1)], path[i.saturating_sub(1)]));
    let t0 = tangent(0);
    let mut nrm = normalize(cross(t0, if t0[1].abs() < 0.9 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] }));
    let mut b = MeshBuilder::new();
    for (i, p) in path.iter().enumerate() {
        let t = tangent(i);
        nrm = normalize(sub(nrm, scale(t, dot(nrm, t))));
        let bin = cross(t, nrm);
        let r = radius(i as f32 / (n - 1) as f32).max(0.0);
        for k in 0..=s {
            let a = k as f32 / s as f32 * TAU;
            let d = add(scale(nrm, a.cos()), scale(bin, a.sin()));
            b.vertex(add(*p, scale(d, r)), d);
        }
    }
    let row = s + 1;
    for i in 0..(n - 1) as u32 {
        for k in 0..s {
            let a = i * row + k;
            b.quad(a, a + 1, a + row + 1, a + row);
        }
    }
    let mut m = b.finish();
    fix_winding(&mut m);
    // Cap the start.
    let r = radius(0.0);
    if r > 1e-5 {
        let mut cap = MeshBuilder::new();
        let back = scale(t0, -1.0);
        let c = cap.vertex(path[0], back);
        for v in m.vertices.iter().take(s as usize) {
            cap.vertex([v[0], v[1], v[2]], back);
        }
        for k in 0..s {
            cap.tri(c, c + 1 + k, c + 1 + (k + 1) % s);
        }
        let mut cm = cap.finish();
        fix_winding(&mut cm);
        geo::merge(&mut m, &cm);
    }
    m
}

/// A path of `n` points from `from`, `len` long, heading `dir`, curving by
/// `curve` (a bend added per unit length, m) and wobbling by seeded noise.
pub fn path(from: [f32; 3], dir: [f32; 3], len: f32, n: u32, curve: [f32; 3], wobble: f32, seed: u32) -> Vec<[f32; 3]> {
    let d = normalize(dir);
    let n = n.max(2);
    (0..n)
        .map(|i| {
            let t = i as f32 / (n - 1) as f32;
            let w = |k: u32| (noise::value2(t * 3.0, k as f32 * 7.1, seed) - 0.5) * 2.0 * wobble * t;
            let along = scale(d, t * len);
            let bend = scale(curve, t * t * len);
            add(add(from, add(along, bend)), [w(1), w(2) * 0.5, w(3)])
        })
        .collect()
}

/// A tapered blade rising from the origin: `w` wide at the base, `h` tall,
/// its tip moved by `bend` (x, z at the tip, m), `segs` segments, shown
/// from both sides (grass, reeds, flames, fronds).
pub fn blade(w: f32, h: f32, bend: [f32; 2], segs: u32) -> Mesh {
    let n = segs.clamp(1, 16);
    let mut b = MeshBuilder::new();
    for i in 0..=n {
        let t = i as f32 / n as f32;
        let (cx, cz) = (bend[0] * t * t, bend[1] * t * t);
        // Across (1, 0, 0) by along (dx, h, dz): the blade's face.
        let dz = 2.0 * bend[1] * t;
        let nrm = normalize([0.0, -dz, h.max(1e-4)]);
        let half = 0.5 * w * (1.0 - t * 0.92);
        b.vertex([cx - half, t * h, cz], nrm);
        b.vertex([cx + half, t * h, cz], nrm);
    }
    for i in 0..n {
        let a = i * 2;
        b.quad(a, a + 1, a + 3, a + 2);
    }
    let mut m = b.finish();
    fix_winding(&mut m);
    double_sided(&mut m);
    foliage_normals(&mut m);
    m
}

/// Bend every normal towards the sky (the usual foliage shading): a thin
/// card seen edge-on then shades like the ground it grows from, lit from
/// above, instead of catching the look's rim light as a pale sliver.
pub fn foliage_normals(m: &mut Mesh) {
    for v in &mut m.vertices {
        let n = normalize([v[3], v[4] + 1.6, v[5]]);
        v[3] = n[0];
        v[4] = n[1];
        v[5] = n[2];
    }
}

/// A leaf: a flat diamond `len` long and `w` wide along +x from the
/// origin, facing up, shown from both sides.
pub fn leaf(len: f32, w: f32) -> Mesh {
    let mut b = MeshBuilder::new();
    let up = [0.0, 1.0, 0.0];
    let a = b.vertex([0.0, 0.0, 0.0], up);
    b.vertex([len * 0.45, 0.0, w * 0.5], up);
    b.vertex([len, 0.0, 0.0], up);
    b.vertex([len * 0.45, 0.0, -w * 0.5], up);
    b.quad(a, a + 1, a + 2, a + 3);
    let mut m = b.finish();
    fix_winding(&mut m);
    double_sided(&mut m);
    foliage_normals(&mut m);
    m
}

/// A leafy crown: `lumps` seeded lumpy balls inside an ellipsoid of half
/// sizes `half` round `at`.
pub fn crown(half: [f32; 3], at: [f32; 3], lumps: u32, seed: u32) -> Mesh {
    let mut m = Mesh::default();
    let n = lumps.max(1);
    for i in 0..n {
        let (a, e, r) = (rnd(seed, i, 1) * TAU, rnd(seed, i, 2) * 2.0 - 1.0, rnd(seed, i, 3).sqrt() * 0.6);
        let c = [at[0] + half[0] * r * a.cos() * (1.0 - e * e).sqrt(), at[1] + half[1] * r * e, at[2] + half[2] * r * a.sin() * (1.0 - e * e).sqrt()];
        let k = range(seed, i, 4, 0.45, 0.7);
        geo::merge(&mut m, &blob([half[0] * k, half[1] * k, half[2] * k], c, 0.6, seed.wrapping_add(i * 31), 1));
    }
    m
}

/// `n` seeded points spread over a disc of radius `r` (x, z): a jittered
/// sunflower, so they neither clump nor line up.
pub fn scatter_disc(n: u32, r: f32, seed: u32) -> Vec<[f32; 2]> {
    (0..n)
        .map(|i| {
            let rr = r * ((i as f32 + 0.5) / n.max(1) as f32).sqrt() * range(seed, i, 7, 0.85, 1.0);
            let a = i as f32 * 2.399_963 + range(seed, i, 8, -0.4, 0.4);
            [rr * a.cos(), rr * a.sin()]
        })
        .collect()
}

/// `n` seeded points over a rectangle of half sizes `half` (x, z): a
/// jittered grid.
pub fn scatter_rect(n: u32, half: [f32; 2], seed: u32) -> Vec<[f32; 2]> {
    let n = n.max(1);
    let aspect = (half[0] / half[1].max(1e-4)).max(1e-4);
    let cols = ((n as f32 * aspect).sqrt().ceil() as u32).max(1);
    let rows = n.div_ceil(cols);
    (0..n)
        .map(|i| {
            let (cx, cz) = ((i % cols) as f32, (i / cols) as f32);
            let x = ((cx + rnd(seed, i, 5)) / cols as f32 * 2.0 - 1.0) * half[0];
            let z = ((cz + rnd(seed, i, 6)) / rows as f32 * 2.0 - 1.0) * half[1];
            [x, z]
        })
        .collect()
}

/// A trunk: a tapered tube `height` tall from `r0` to `r1`, leaning by
/// seeded noise (`lean` m at the top). Returns the mesh and its path.
pub fn trunk(height: f32, r0: f32, r1: f32, lean: f32, seed: u32, sides: u32) -> (Mesh, Vec<[f32; 3]>) {
    let p = path([0.0, -0.05 * r0.max(0.01), 0.0], [0.0, 1.0, 0.0], height, 8, [0.0; 3], lean, seed);
    let m = tube(&p, |t| r0 + (r1 - r0) * t, sides);
    (m, p)
}

/// `count` branches off `trunk`'s upper part (from `from` 0..1 up it),
/// each `len` long, rising at `rise` (0 flat, 1 straight up), radius `r`
/// at the base. Returns the wood and the branch tips.
pub fn branches(trunk: &[[f32; 3]], count: u32, from: f32, len: f32, rise: f32, r: f32, seed: u32) -> (Mesh, Vec<[f32; 3]>) {
    let mut m = Mesh::default();
    let mut tips = Vec::new();
    if trunk.len() < 2 {
        return (m, tips);
    }
    for i in 0..count {
        let t = from + (1.0 - from) * (i as f32 + rnd(seed, i, 1)) / count.max(1) as f32;
        let f = t * (trunk.len() - 1) as f32;
        let (j, k) = (f.floor() as usize, f.fract());
        let a = trunk[j.min(trunk.len() - 1)];
        let b = trunk[(j + 1).min(trunk.len() - 1)];
        let at = add(a, scale(sub(b, a), k));
        let yaw = i as f32 * 2.399_963 + rnd(seed, i, 2) * 0.8;
        let l = len * range(seed, i, 3, 0.7, 1.1) * (1.0 - 0.45 * t);
        let dir = [yaw.cos() * (1.0 - rise), rise, yaw.sin() * (1.0 - rise)];
        let p = path(at, dir, l, 5, [0.0, -0.12, 0.0], l * 0.08, seed.wrapping_add(i * 7));
        if let Some(tip) = p.last() {
            tips.push(*tip);
        }
        geo::merge(&mut m, &tube(&p, |s| r * (1.0 - 0.8 * s), 6));
    }
    (m, tips)
}

/// Small lumpy puffs (smoke, cloud, dust, foam): one per point, radius
/// `r[i]`.
pub fn puffs(points: &[[f32; 3]], r: &[f32], seed: u32) -> Mesh {
    let mut m = Mesh::default();
    for (i, (p, rr)) in points.iter().zip(r).enumerate() {
        geo::merge(&mut m, &blob([*rr; 3], *p, 0.5, seed.wrapping_add(i as u32 * 13), 1));
    }
    m
}

/// Thin streaks (rain, sparks, wind lines): a narrow box from each point
/// along `dir`, `len` long and `w` thick.
pub fn streaks(points: &[[f32; 3]], dir: [f32; 3], len: f32, w: f32) -> Mesh {
    let d = normalize(dir);
    let mut m = Mesh::default();
    let base = geo::segmented_box([w, len, w], 1);
    let axis = cross([0.0, 1.0, 0.0], d);
    let angle = dot([0.0, 1.0, 0.0], d).clamp(-1.0, 1.0).acos();
    for p in points {
        let mut s = base.clone();
        if axis.iter().any(|x| x.abs() > 1e-5) {
            rotate(&mut s, axis, angle);
        } else if d[1] < 0.0 {
            rotate(&mut s, [1.0, 0.0, 0.0], PI);
        }
        geo::transform(&mut s, [1.0; 3], *p);
        geo::merge(&mut m, &s);
    }
    m
}

/// Ripple the top of a height function: `amp` high waves `wave` m apart
/// along `dir` (radians), bent by noise (sand dunes, rippled sand).
pub fn ripples(x: f32, z: f32, dir: f32, wave: f32, amp: f32, seed: u32) -> f32 {
    let (s, c) = dir.sin_cos();
    let u = (x * c + z * s) / wave.max(1e-3) + noise::value2(x * 0.15, z * 0.15, seed) * 2.0;
    // Gentle on the windward side, steep on the lee.
    let f = u.fract();
    amp * if f < 0.75 { f / 0.75 } else { (1.0 - f) / 0.25 }
}

/// A rocky vault: a half pipe `len` long along z, standing on y = 0, inner
/// radius `r_in`, outer `r_out`, made lumpy by seeded noise (`lump` 0..1):
/// a cave mouth or, short, a stone arch.
pub fn vault(len: f32, r_in: f32, r_out: f32, lump: f32, seed: u32) -> Mesh {
    let (nu, na) = (14u32, 18u32);
    let r_at = |outer: bool, i: u32, j: u32| {
        let (u, a) = (i as f32 / nu as f32, j as f32 / na as f32 * PI);
        let base = if outer { r_out } else { r_in };
        let n = noise::fbm3([u * len * 0.6, a * 1.3, if outer { 0.0 } else { 9.0 }], seed, 3);
        (base * (1.0 + lump * 0.22 * n), u, a)
    };
    let pos = |r: f32, u: f32, a: f32| [a.cos() * r, a.sin() * r, (u - 0.5) * len];
    let mut b = MeshBuilder::new();
    for outer in [true, false] {
        let start = b.mesh.vertices.len() as u32;
        for i in 0..=nu {
            for j in 0..=na {
                let (r, u, a) = r_at(outer, i, j);
                let d = [a.cos(), a.sin(), 0.0];
                b.vertex(pos(r, u, a), if outer { d } else { scale(d, -1.0) });
            }
        }
        let row = na + 1;
        for i in 0..nu {
            for j in 0..na {
                let a = start + i * row + j;
                b.quad(a, a + 1, a + row + 1, a + row);
            }
        }
    }
    // The two ends (z = -len/2 and +len/2) and the two feet (y = 0).
    for (i, nz) in [(0u32, -1.0f32), (nu, 1.0)] {
        for j in 0..na {
            let (ro0, u, a0) = r_at(true, i, j);
            let (ro1, _, a1) = r_at(true, i, j + 1);
            let (ri0, _, _) = r_at(false, i, j);
            let (ri1, _, _) = r_at(false, i, j + 1);
            let n = [0.0, 0.0, nz];
            let v0 = b.vertex(pos(ro0, u, a0), n);
            b.vertex(pos(ro1, u, a1), n);
            b.vertex(pos(ri1, u, a1), n);
            b.vertex(pos(ri0, u, a0), n);
            b.quad(v0, v0 + 1, v0 + 2, v0 + 3);
        }
    }
    for j in [0u32, na] {
        for i in 0..nu {
            let (ro0, u0, a) = r_at(true, i, j);
            let (ro1, u1, _) = r_at(true, i + 1, j);
            let (ri0, _, _) = r_at(false, i, j);
            let (ri1, _, _) = r_at(false, i + 1, j);
            let n = [0.0, -1.0, 0.0];
            let v0 = b.vertex(pos(ro0, u0, a), n);
            b.vertex(pos(ro1, u1, a), n);
            b.vertex(pos(ri1, u1, a), n);
            b.vertex(pos(ri0, u0, a), n);
            b.quad(v0, v0 + 1, v0 + 2, v0 + 3);
        }
    }
    let mut m = b.finish();
    fix_winding(&mut m);
    for v in &mut m.vertices {
        v[1] = v[1].max(0.0);
    }
    m
}

/// A flat water sheet `size` (x, z) at height `y`, `cells` a side, its
/// surface moved by `wave(x, z)` (m).
pub fn water_sheet(size: [f32; 2], y: f32, cells: u32, wave: impl Fn(f32, f32) -> f32) -> Mesh {
    let mut m = geo::height_grid(size, cells, |x, z| y + wave(x, z));
    // A sheet may dip under y = 0 at a wave trough: keep it on the ground.
    for v in &mut m.vertices {
        v[1] = v[1].max(0.0);
    }
    m
}
