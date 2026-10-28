//! Normals, tangents, UV charts and exact normal maps
//! (docs/model_research/16-normals-tangents-uvs.md), M1 version.
//!
//! Charts follow the part graph: triangles are grouped by owning part and
//! by the axis their face points along (six directions), then split into
//! edge-connected pieces, so seams fall on part boundaries and box-like
//! creases. Each chart is projected on its axis plane in real units (one
//! texel density per tier) with a right-handed frame, so no chart is
//! mirrored. Charts are packed by our own skyline packer, largest first.
//!
//! Tangents follow the MikkTSpace convention as described in its public
//! write-up (implemented here from that description): per-corner tangents
//! from the texture-coordinate derivatives, weighted by corner angle, made
//! perpendicular to the vertex normal, with the bitangent rebuilt from the
//! normal, the tangent and a stored handedness sign.
//!
//! The normal map has no rays: each texel's point on the mesh is moved onto
//! the field by bounded Newton steps and the field's exact gradient there is
//! expressed in the interpolated tangent frame. Texels outside the charts
//! are filled from their neighbours so filtering never reads the background.

use crate::mesh::Mesh;
use crate::rng::map_tiles;
use crate::sdf::Node;
use crate::vec::*;

#[derive(Clone, Debug)]
pub struct SurfaceOut {
    /// The mesh split along chart seams (same triangles, more vertices).
    pub mesh: Mesh,
    pub uv: Vec<[f64; 2]>,
    pub tangent: Vec<[f64; 4]>,
    pub size: usize,
    /// RGB, `size` x `size`, tangent-space normals (+x right, +y up the
    /// chart's v axis, +z out of the surface).
    pub normal_map: Vec<u8>,
    pub charts: usize,
    pub texels_per_m: f64,
    /// Largest angle between the world normals the two sides of a seam
    /// encode at the same point (texels decoded in their own frames,
    /// interpolated over each side's own texels), degrees.
    pub seam_max_deg: f64,
    /// The plain largest angle between the two sides (texel spacing included).
    pub seam_raw_deg: f64,
    /// Largest angle between a decoded texel and the field normal, degrees.
    pub bake_max_deg: f64,
    /// Texels claimed by two triangles of one chart (overlaps).
    pub overlaps: usize,
    /// Seam samples compared, and those skipped because the surface turns
    /// more than 10 degrees within a texel there (unresolved at this tier).
    pub seam_samples: usize,
    pub seam_unresolved: usize,
}

const PAD: usize = 3;

/// Axis key (0..6: +x, -x, +y, -y, +z, -z) and the chart plane (u, v) for it.
fn axis_key(n: V3) -> usize {
    let a = [n[0].abs(), n[1].abs(), n[2].abs()];
    let ax = if a[0] >= a[1] && a[0] >= a[2] {
        0
    } else if a[1] >= a[2] {
        1
    } else {
        2
    };
    ax * 2 + usize::from(n[ax] < 0.0)
}

/// Right-handed (u, v, n) frames: u x v = the axis direction.
fn project(key: usize, p: V3) -> [f64; 2] {
    match key {
        0 => [-p[2], p[1]],
        1 => [p[2], p[1]],
        2 => [p[0], -p[2]],
        3 => [p[0], p[2]],
        4 => [p[0], p[1]],
        _ => [-p[0], p[1]],
    }
}

/// Our own skyline packer: rectangles (w, h) in texels into a square of
/// side `size`; `None` when they do not fit.
pub fn pack(rects: &[(usize, usize)], size: usize) -> Option<Vec<(usize, usize)>> {
    let mut order: Vec<usize> = (0..rects.len()).collect();
    order.sort_by(|&a, &b| rects[b].1.cmp(&rects[a].1).then(rects[b].0.cmp(&rects[a].0)).then(a.cmp(&b)));
    let mut sky = vec![0usize; size];
    let mut out = vec![(0, 0); rects.len()];
    for i in order {
        let (w, h) = rects[i];
        if w > size || h > size {
            return None;
        }
        let mut best: Option<(usize, usize)> = None;
        for x in 0..=(size - w) {
            let y = sky[x..x + w].iter().copied().max().unwrap_or(0);
            if y + h <= size && best.is_none_or(|b| y < b.1) {
                best = Some((x, y));
            }
        }
        let (x, y) = best?;
        for s in &mut sky[x..x + w] {
            *s = y + h;
        }
        out[i] = (x, y);
    }
    Some(out)
}

/// Charts, tangents and a baked normal map for `mesh` (in the field's frame).
pub fn build(mesh: &Mesh, field: &Node, texels_per_m: f64, max_size: usize, threads: usize) -> SurfaceOut {
    let nt = mesh.tris.len();
    let fnorm: Vec<V3> = mesh
        .tris
        .iter()
        .map(|t| norm_or(cross(sub(mesh.pos[t[1] as usize], mesh.pos[t[0] as usize]), sub(mesh.pos[t[2] as usize], mesh.pos[t[0] as usize])), [0.0, 1.0, 0.0]))
        .collect();
    let key: Vec<(u32, usize)> = (0..nt).map(|i| (mesh.tri_part.get(i).copied().unwrap_or(0), axis_key(fnorm[i]))).collect();
    // Edge-connected components with the same key.
    let mut parent: Vec<usize> = (0..nt).collect();
    fn find(p: &mut [usize], mut x: usize) -> usize {
        while p[x] != x {
            p[x] = p[p[x]];
            x = p[x];
        }
        x
    }
    let mut edges: Vec<(u32, u32, usize)> = Vec::with_capacity(nt * 3);
    for (i, t) in mesh.tris.iter().enumerate() {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            edges.push((a.min(b), a.max(b), i));
        }
    }
    edges.sort_unstable();
    for w in edges.windows(2) {
        if w[0].0 == w[1].0 && w[0].1 == w[1].1 && key[w[0].2] == key[w[1].2] {
            let (a, b) = (find(&mut parent, w[0].2), find(&mut parent, w[1].2));
            if a != b {
                parent[a.max(b)] = a.min(b);
            }
        }
    }
    let roots: Vec<usize> = (0..nt).map(|i| find(&mut parent, i)).collect();
    let mut chart_ids: Vec<usize> = roots.clone();
    chart_ids.sort_unstable();
    chart_ids.dedup();
    let chart_of: Vec<usize> = roots.iter().map(|r| chart_ids.binary_search(r).unwrap_or(0)).collect();
    let nc = chart_ids.len();

    // Split vertices per chart: (chart, old vertex) -> new vertex.
    let mut pairs: Vec<(usize, u32)> = Vec::with_capacity(nt * 3);
    for (i, t) in mesh.tris.iter().enumerate() {
        for &v in t {
            pairs.push((chart_of[i], v));
        }
    }
    pairs.sort_unstable();
    pairs.dedup();
    let new_index = |c: usize, v: u32| pairs.binary_search(&(c, v)).unwrap_or(0) as u32;
    let mut out = Mesh { pos: Vec::with_capacity(pairs.len()), nrm: Vec::with_capacity(pairs.len()), tris: Vec::with_capacity(nt), tri_part: mesh.tri_part.clone() };
    let mut uvm: Vec<[f64; 2]> = Vec::with_capacity(pairs.len());
    let mut vchart = Vec::with_capacity(pairs.len());
    for &(c, v) in &pairs {
        out.pos.push(mesh.pos[v as usize]);
        out.nrm.push(mesh.nrm[v as usize]);
        let k = key[chart_ids[c]].1;
        uvm.push(project(k, mesh.pos[v as usize]));
        vchart.push(c);
    }
    for (i, t) in mesh.tris.iter().enumerate() {
        out.tris.push([new_index(chart_of[i], t[0]), new_index(chart_of[i], t[1]), new_index(chart_of[i], t[2])]);
    }
    // Chart boxes in metres.
    let mut cmin = vec![[f64::INFINITY; 2]; nc];
    let mut cmax = vec![[f64::NEG_INFINITY; 2]; nc];
    for (i, uv) in uvm.iter().enumerate() {
        let c = vchart[i];
        for k in 0..2 {
            cmin[c][k] = cmin[c][k].min(uv[k]);
            cmax[c][k] = cmax[c][k].max(uv[k]);
        }
    }
    // Pack at the requested density, lowering it until the charts fit.
    let mut density = texels_per_m;
    let mut size;
    let mut places = None;
    let area: f64 = (0..nc).map(|c| ((cmax[c][0] - cmin[c][0]) * density + 2.0 * PAD as f64) * ((cmax[c][1] - cmin[c][1]) * density + 2.0 * PAD as f64)).sum();
    size = 16usize;
    while ((size * size) as f64) < area * 1.4 && size < max_size {
        size *= 2;
    }
    for _ in 0..64 {
        let rects: Vec<(usize, usize)> = (0..nc).map(|c| (((cmax[c][0] - cmin[c][0]) * density).ceil() as usize + 2 * PAD, ((cmax[c][1] - cmin[c][1]) * density).ceil() as usize + 2 * PAD)).collect();
        places = pack(&rects, size);
        if places.is_some() {
            break;
        }
        if size < max_size {
            size *= 2;
        } else {
            density *= 0.9;
        }
    }
    if places.is_none() {
        // Padding alone overflows the cap: allow one size more.
        size *= 2;
        let rects: Vec<(usize, usize)> = (0..nc).map(|c| (((cmax[c][0] - cmin[c][0]) * density).ceil() as usize + 2 * PAD, ((cmax[c][1] - cmin[c][1]) * density).ceil() as usize + 2 * PAD)).collect();
        places = pack(&rects, size);
    }
    let places = places.unwrap_or_default();
    let s = size as f64;
    let uv: Vec<[f64; 2]> = uvm
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let c = vchart[i];
            let (ox, oy) = places[c];
            // v grows downward in the image; the chart's +v points up.
            let x = ox as f64 + PAD as f64 + (m[0] - cmin[c][0]) * density;
            let y = oy as f64 + PAD as f64 + (cmax[c][1] - m[1]) * density;
            [x / s, y / s]
        })
        .collect();

    let tangent = tangents(&out, &uv);

    // Bake.
    let frames: Vec<(V3, V3, V3)> = (0..out.pos.len())
        .map(|i| {
            let n = norm_or(out.nrm[i], [0.0, 1.0, 0.0]);
            let t = [tangent[i][0], tangent[i][1], tangent[i][2]];
            let b = mul(cross(n, t), tangent[i][3]);
            (t, b, n)
        })
        .collect();
    // Rasterise every triangle's texels: (texel, tri, barycentrics).
    let per_tri: Vec<Vec<(usize, [f64; 3], bool)>> = map_tiles(nt.div_ceil(512), threads, |tile| {
        let lo = tile * 512;
        let hi = (lo + 512).min(nt);
        let mut acc = Vec::new();
        for ti in lo..hi {
            let t = out.tris[ti];
            let p: Vec<[f64; 2]> = t.iter().map(|&v| [uv[v as usize][0] * s, uv[v as usize][1] * s]).collect();
            let x0 = (p.iter().map(|q| q[0]).fold(f64::INFINITY, f64::min).floor() - 1.0).max(0.0) as usize;
            let x1 = (p.iter().map(|q| q[0]).fold(f64::NEG_INFINITY, f64::max).ceil() as usize + 1).min(size);
            let y0 = (p.iter().map(|q| q[1]).fold(f64::INFINITY, f64::min).floor() - 1.0).max(0.0) as usize;
            let y1 = (p.iter().map(|q| q[1]).fold(f64::NEG_INFINITY, f64::max).ceil() as usize + 1).min(size);
            let area = (p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[1][1] - p[0][1]) * (p[2][0] - p[0][0]);
            if area.abs() < 1e-12 {
                acc.push(Vec::new());
                continue;
            }
            let mut v = Vec::new();
            for y in y0..y1 {
                for x in x0..x1 {
                    let c = [x as f64 + 0.5, y as f64 + 0.5];
                    let bary_at = |c: [f64; 2]| {
                        let e = |a: [f64; 2], b: [f64; 2]| ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])) / area;
                        [e(p[1], p[2]), e(p[2], p[0]), e(p[0], p[1])]
                    };
                    let b = bary_at(c);
                    if b.iter().all(|&x| x >= -1e-9) {
                        v.push((y * size + x, b, true));
                        continue;
                    }
                    // Texels the triangle touches: baked at its nearest point.
                    let mut best = (f64::INFINITY, c);
                    for k in 0..3 {
                        let (a, bb) = (p[k], p[(k + 1) % 3]);
                        let d = [bb[0] - a[0], bb[1] - a[1]];
                        let l2 = d[0] * d[0] + d[1] * d[1];
                        let t = if l2 > 0.0 { (((c[0] - a[0]) * d[0] + (c[1] - a[1]) * d[1]) / l2).clamp(0.0, 1.0) } else { 0.0 };
                        let q = [a[0] + d[0] * t, a[1] + d[1] * t];
                        let dd = (q[0] - c[0]).powi(2) + (q[1] - c[1]).powi(2);
                        if dd < best.0 {
                            best = (dd, q);
                        }
                    }
                    if best.0 <= 0.75 * 0.75 {
                        let b = bary_at(best.1).map(|x| x.max(0.0));
                        let sum = b[0] + b[1] + b[2];
                        v.push((y * size + x, b.map(|x| x / sum), false));
                    }
                }
            }
            acc.push(v);
        }
        acc
    })
    .into_iter()
    .flatten()
    .collect();
    let mut owner = vec![(u32::MAX, [0.0; 3]); size * size];
    let mut overlaps = 0;
    for (ti, texels) in per_tri.iter().enumerate() {
        for &(tx, b, interior) in texels {
            if !interior {
                continue;
            }
            if owner[tx].0 != u32::MAX && owner[tx].0 != ti as u32 {
                // Both strictly inside: a real fold of the chart, not a shared edge.
                let inner = |b: [f64; 3]| b.iter().all(|&x| x > 1e-6);
                if chart_of[owner[tx].0 as usize] == chart_of[ti] && inner(b) && inner(owner[tx].1) {
                    overlaps += 1;
                }
                continue;
            }
            owner[tx] = (ti as u32, b);
        }
    }
    // Then the touched texels nobody covers (first triangle wins).
    for (ti, texels) in per_tri.iter().enumerate() {
        for &(tx, b, interior) in texels {
            if !interior && owner[tx].0 == u32::MAX {
                owner[tx] = (ti as u32, b);
            }
        }
    }
    let encode = |n: V3, fr: (V3, V3, V3)| -> [u8; 3] {
        let ts = norm_or([dot(n, fr.0), dot(n, fr.1), dot(n, fr.2)], [0.0, 0.0, 1.0]);
        [((ts[0] * 0.5 + 0.5) * 255.0).round() as u8, ((ts[1] * 0.5 + 0.5) * 255.0).round() as u8, ((ts[2] * 0.5 + 0.5) * 255.0).round() as u8]
    };
    let frame_at = |ti: usize, b: [f64; 3]| -> (V3, V3, V3, V3) {
        let t = out.tris[ti];
        let mut acc = ([0.0; 3], [0.0; 3], [0.0; 3], [0.0; 3]);
        for k in 0..3 {
            let v = t[k] as usize;
            acc.0 = add(acc.0, mul(frames[v].0, b[k]));
            acc.1 = add(acc.1, mul(frames[v].1, b[k]));
            acc.2 = add(acc.2, mul(frames[v].2, b[k]));
            acc.3 = add(acc.3, mul(out.pos[v], b[k]));
        }
        let n = norm_or(acc.2, [0.0, 1.0, 0.0]);
        let tt = norm_or(sub(acc.0, mul(n, dot(n, acc.0))), [1.0, 0.0, 0.0]);
        let sign = tangent[t[0] as usize][3];
        (tt, mul(cross(n, tt), sign), n, acc.3)
    };
    let surface_normal = |p: V3| -> V3 {
        let mut x = p;
        for _ in 0..4 {
            let sm = field.eval(x);
            let g2 = dot(sm.g, sm.g);
            if g2 <= 1e-24 || sm.d.abs() < 1e-9 {
                break;
            }
            let step = (sm.d / g2.sqrt()).clamp(-0.02, 0.02);
            x = sub(x, mul(norm_or(sm.g, [0.0; 3]), step));
        }
        norm_or(field.eval(x).g, [0.0, 1.0, 0.0])
    };
    let rows: Vec<Vec<u8>> = map_tiles(size, threads, |y| {
        let mut row = vec![0u8; size * 3];
        for x in 0..size {
            let (ti, b) = owner[y * size + x];
            let px = if ti == u32::MAX {
                [128, 128, 255]
            } else {
                let (t, bb, n, p) = frame_at(ti as usize, b);
                encode(surface_normal(p), (t, bb, n))
            };
            row[x * 3..x * 3 + 3].copy_from_slice(&px);
        }
        row
    });
    let mut img: Vec<u8> = rows.into_iter().flatten().collect();
    // Fill the padding from covered neighbours (four passes).
    let mut covered: Vec<bool> = owner.iter().map(|o| o.0 != u32::MAX).collect();
    for _ in 0..PAD + 1 {
        let snap = img.clone();
        let cov = covered.clone();
        for y in 0..size {
            for x in 0..size {
                if cov[y * size + x] {
                    continue;
                }
                let mut sum = [0u32; 3];
                let mut n = 0;
                for (dx, dy) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1)] {
                    let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                    if nx < 0 || ny < 0 || nx >= size as i64 || ny >= size as i64 {
                        continue;
                    }
                    let j = ny as usize * size + nx as usize;
                    if cov[j] {
                        for k in 0..3 {
                            sum[k] += snap[j * 3 + k] as u32;
                        }
                        n += 1;
                    }
                }
                if n > 0 {
                    for k in 0..3 {
                        img[(y * size + x) * 3 + k] = (sum[k] + n / 2).checked_div(n).unwrap_or(0) as u8;
                    }
                    covered[y * size + x] = true;
                }
            }
        }
    }

    // Checks: decoded texels against the field, and both sides of seams.
    let decode = |px: &[u8], fr: (V3, V3, V3)| -> V3 {
        let ts = [px[0] as f64 / 255.0 * 2.0 - 1.0, px[1] as f64 / 255.0 * 2.0 - 1.0, px[2] as f64 / 255.0 * 2.0 - 1.0];
        norm_or(add(add(mul(fr.0, ts[0]), mul(fr.1, ts[1])), mul(fr.2, ts[2])), [0.0, 1.0, 0.0])
    };
    let angle = |a: V3, b: V3| crate::dmath::atan2(len(cross(a, b)), dot(a, b)).to_degrees();
    let mut bake_max: f64 = 0.0;
    for (i, o) in owner.iter().enumerate().step_by(7) {
        if o.0 == u32::MAX {
            continue;
        }
        let (t, bb, n, p) = frame_at(o.0 as usize, o.1);
        let got = decode(&img[i * 3..i * 3 + 3], (t, bb, n));
        bake_max = bake_max.max(angle(got, surface_normal(p)));
    }
    // The world normal a texel stands for (decoded in its owner's frame).
    let texel_world = |tx: usize| -> Option<V3> {
        let o = owner[tx];
        if o.0 == u32::MAX {
            return None;
        }
        let (tt, bb, n, _) = frame_at(o.0 as usize, o.1);
        Some(decode(&img[tx * 3..tx * 3 + 3], (tt, bb, n)))
    };
    // Bilinear over the side's own texels (same chart) around the point.
    let sample = |ti: usize, b: [f64; 3], edge_n: V3| -> (V3, f64) {
        let t = out.tris[ti];
        let u = [0, 1].map(|k| (0..3).map(|j| uv[t[j] as usize][k] * b[j]).sum::<f64>() * s - 0.5);
        let (x0, y0) = (u[0].floor(), u[1].floor());
        let (fx, fy) = (u[0] - x0, u[1] - y0);
        let mut acc = [0.0; 3];
        let mut turn: f64 = 0.0;
        for (dx, dy, w) in [(0.0, 0.0, (1.0 - fx) * (1.0 - fy)), (1.0, 0.0, fx * (1.0 - fy)), (0.0, 1.0, (1.0 - fx) * fy), (1.0, 1.0, fx * fy)] {
            let (x, y) = (x0 + dx, y0 + dy);
            if x < 0.0 || y < 0.0 || x >= s || y >= s {
                continue;
            }
            let tx = y as usize * size + x as usize;
            if owner[tx].0 != u32::MAX && chart_of[owner[tx].0 as usize] == chart_of[ti]
                && let Some(n) = texel_world(tx)
            {
                acc = add(acc, mul(n, w + 1e-9));
                let o = owner[tx];
                let (_, _, _, q) = frame_at(o.0 as usize, o.1);
                turn = turn.max(angle(surface_normal(q), edge_n));
            }
        }
        (norm_or(acc, frame_at(ti, b).2), turn)
    };
    let mut seam_max: f64 = 0.0;
    let (mut resolved, mut unresolved) = (0usize, 0usize);
    let mut seam_raw: f64 = 0.0;
    for w in edges.windows(2) {
        let (a, b) = (w[0].2, w[1].2);
        if w[0].0 != w[1].0 || w[0].1 != w[1].1 || chart_of[a] == chart_of[b] {
            continue;
        }
        for f in [0.25, 0.5, 0.75] {
            // Barycentrics of the edge point in each triangle.
            let bary = |ti: usize| -> [f64; 3] {
                let t = mesh.tris[ti];
                let mut out_b = [0.0; 3];
                for k in 0..3 {
                    if t[k] == w[0].0 {
                        out_b[k] = 1.0 - f;
                    } else if t[k] == w[0].1 {
                        out_b[k] = f;
                    }
                }
                out_b
            };
            // Only where the surface itself is smooth across the seam: a seam
            // on a crease (the true normals already differ) is placed there
            // by design and is not a defect.
            let inside_pt = |ti: usize, bb: [f64; 3]| {
                let t = out.tris[ti];
                let c = (0..3).fold([0.0; 3], |acc, k| add(acc, mul(out.pos[t[k] as usize], 1.0 / 3.0)));
                let p = (0..3).fold([0.0; 3], |acc, k| add(acc, mul(out.pos[t[k] as usize], bb[k])));
                lerp(p, c, 0.3)
            };
            let (na, nb) = (surface_normal(inside_pt(a, bary(a))), surface_normal(inside_pt(b, bary(b))));
            if angle(na, nb) > 15.0 || angle(fnorm[a], fnorm[b]) > 30.0 {
                continue;
            }
            // Resolved at texel scale: the true normal one texel into each
            // side stays within 10 degrees of the edge's.
            let p = lerp(mesh.pos[w[0].0 as usize], mesh.pos[w[0].1 as usize], f);
            let ne = surface_normal(p);
            let step = 1.5 / density;
            let into = |ti: usize| {
                let t = out.tris[ti];
                let c = (0..3).fold([0.0; 3], |acc, k| add(acc, mul(out.pos[t[k] as usize], 1.0 / 3.0)));
                let d = sub(c, p);
                add(p, mul(norm_or(d, [0.0; 3]), step.min(len(d))))
            };
            if angle(surface_normal(into(a)), ne) > 10.0 || angle(surface_normal(into(b)), ne) > 10.0 {
                unresolved += 1;
                continue;
            }
            resolved += 1;
            // The part of the mismatch the texel spacing does not explain:
            // each side's texels sit where the true normal already turned.
            let ((sa, ta), (sb, tb)) = (sample(a, bary(a), ne), sample(b, bary(b), ne));
            let raw = angle(sa, sb);
            seam_raw = seam_raw.max(raw);
            seam_max = seam_max.max(raw - ta - tb);
        }
    }
    SurfaceOut { mesh: out, uv, tangent, size, normal_map: img, charts: nc, texels_per_m: density, seam_max_deg: seam_max.max(0.0), seam_raw_deg: seam_raw, bake_max_deg: bake_max, overlaps, seam_samples: resolved, seam_unresolved: unresolved }
}

/// Per-vertex tangents in the MikkTSpace convention (see the module note).
pub fn tangents(m: &Mesh, uv: &[[f64; 2]]) -> Vec<[f64; 4]> {
    let n = m.pos.len();
    let mut tacc = vec![[0.0; 3]; n];
    let mut bacc = vec![[0.0; 3]; n];
    // Handedness votes from the face normals (constant over a planar chart).
    let mut hand = vec![0.0f64; n];
    for t in &m.tris {
        let (p0, p1, p2) = (m.pos[t[0] as usize], m.pos[t[1] as usize], m.pos[t[2] as usize]);
        let (w0, w1, w2) = (uv[t[0] as usize], uv[t[1] as usize], uv[t[2] as usize]);
        let (e1, e2) = (sub(p1, p0), sub(p2, p0));
        let (du1, dv1, du2, dv2) = (w1[0] - w0[0], w1[1] - w0[1], w2[0] - w0[0], w2[1] - w0[1]);
        let det = du1 * dv2 - du2 * dv1;
        if det.abs() < 1e-20 {
            continue;
        }
        let r = 1.0 / det;
        let tdir = mul(sub(mul(e1, dv2), mul(e2, dv1)), r);
        let bdir = mul(sub(mul(e2, du1), mul(e1, du2)), r);
        let face = cross(e1, e2);
        let vote = if dot(cross(tdir, bdir), face) < 0.0 { -1.0 } else { 1.0 };
        for k in 0..3 {
            hand[t[k] as usize] += vote;
            let v = t[k] as usize;
            let (a, b) = (sub(m.pos[t[(k + 1) % 3] as usize], m.pos[v]), sub(m.pos[t[(k + 2) % 3] as usize], m.pos[v]));
            let ang = crate::dmath::atan2(len(cross(a, b)), dot(a, b));
            let nv = norm_or(m.nrm[v], [0.0, 1.0, 0.0]);
            let tp = norm_or(sub(tdir, mul(nv, dot(nv, tdir))), [0.0; 3]);
            let bp = norm_or(sub(bdir, mul(nv, dot(nv, bdir))), [0.0; 3]);
            tacc[v] = add(tacc[v], mul(tp, ang));
            bacc[v] = add(bacc[v], mul(bp, ang));
        }
    }
    (0..n)
        .map(|v| {
            let nv = norm_or(m.nrm[v], [0.0, 1.0, 0.0]);
            let fallback = norm_or(cross(nv, if nv[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] }), [1.0, 0.0, 0.0]);
            let w = if hand[v] < 0.0 { -1.0 } else { 1.0 };
            let tp = sub(tacc[v], mul(nv, dot(nv, tacc[v])));
            // Where the normal leans along the chart's u axis the projected
            // tangent collapses: rebuild it from the bitangent (T = w B x N).
            let t = if len(tp) > 0.25 * len(tacc[v]) {
                norm_or(tp, fallback)
            } else {
                let bp = sub(bacc[v], mul(nv, dot(nv, bacc[v])));
                norm_or(mul(cross(bp, nv), w), fallback)
            };
            [t[0], t[1], t[2], w]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packer_places_without_overlap() {
        let rects = vec![(10, 20), (30, 5), (12, 12), (40, 40), (8, 3)];
        let p = pack(&rects, 64).unwrap();
        for i in 0..rects.len() {
            for j in i + 1..rects.len() {
                let (a, b) = (p[i], p[j]);
                let sep = a.0 + rects[i].0 <= b.0 || b.0 + rects[j].0 <= a.0 || a.1 + rects[i].1 <= b.1 || b.1 + rects[j].1 <= a.1;
                assert!(sep, "{i} and {j} overlap");
            }
        }
        assert!(pack(&[(100, 10)], 64).is_none());
    }

    #[test]
    fn rounded_box_bakes_a_valid_seamless_map() {
        let f = Node::RoundBox { half: [0.5, 0.3, 0.4], r: 0.1 }.label(0).prepare();
        let m = crate::mesh::extract(&f, f.bounds(), &crate::mesh::Options::new(0.05, 2));
        let s = build(&m, &f, 200.0, 512, 2);
        // Tangents: unit, perpendicular to the normal, handedness +-1.
        for (i, t) in s.tangent.iter().enumerate() {
            let n = norm_or(s.mesh.nrm[i], [0.0; 3]);
            assert!((len([t[0], t[1], t[2]]) - 1.0).abs() < 1e-9);
            assert!(dot(n, [t[0], t[1], t[2]]).abs() < 1e-9);
            assert!(t[3] == 1.0 || t[3] == -1.0);
        }
        assert!(s.uv.iter().all(|u| (0.0..=1.0).contains(&u[0]) && (0.0..=1.0).contains(&u[1])));
        assert_eq!(s.overlaps, 0);
        assert!(s.bake_max_deg < 3.0, "bake error {}", s.bake_max_deg);
        assert!(s.seam_max_deg < 2.0, "seam {} (raw {})", s.seam_max_deg, s.seam_raw_deg);
        assert!(s.seam_samples > 0);
        assert!(s.charts >= 6);
        // The split mesh is the same surface: same triangles, same area.
        assert_eq!(s.mesh.tris.len(), m.tris.len());
        // Identical for any thread count.
        let t = build(&m, &f, 200.0, 512, 4);
        assert_eq!(t.normal_map, s.normal_map);
        assert_eq!(t.uv, s.uv);
    }
}
