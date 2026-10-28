//! Measuring a model against a target mesh (docs/model_research/plan-characters.md
//! section 8): `evani-model compare`.
//!
//! 1. **Load.** [`load_glb`] reads any glTF 2.0 binary: the scene's node tree
//!    with TRS or matrix transforms accumulated through parents, every
//!    triangle primitive (modes 4, 5 and 6; indices u8, u16, u32 or none),
//!    POSITION of any component type (float, or integers with or without
//!    `normalized`, which covers KHR_mesh_quantization) and COLOR_0 when
//!    present. A node with a `skin` uses no node transform (glTF: skinned
//!    vertices are placed by the joints; without skinning we keep the bind
//!    pose as stored). Written from the glTF 2.0 specification.
//! 2. **Align, never rescale.** Each mesh is moved so its lowest point is at
//!    y = 0 and its box is centred in x and z. The height ratio is reported.
//! 3. **Views.** front (camera on +z), side (on +x), back (on -z) and
//!    three-quarter (preview.rs's), orthographic, one frame per view shared
//!    by both models (the union of both boxes plus a 5% margin each side),
//!    N x N pixels. With an orthographic camera the back silhouette is the
//!    front one mirrored, so its IoU equals the front's (the sheet says so);
//!    it stays in the mean as the spec asks, weighting the front twice, and
//!    its shaded panels show the back's surface. Masks follow preview.rs's rule (a pixel is inside when at
//!    least two of its four samples are covered); IoU per view and the mean,
//!    plus `iou_hem_band`: front IoU over the rows between 30% and 45% of the
//!    target's height (a coat skirt that wraps the legs instead of hanging as
//!    one panel shows there).
//! 4. **Region measures** from the masks, the same rules for both models, in
//!    metres. "Width of a row" is counted in whole pixels; with the mask rule
//!    each edge can gain up to three quarters of a pixel, so a width is good
//!    to about 1.5 pixels (pixel = frame size / N, about 4 mm at 512 for a
//!    1.9 m figure); heights of steps are row boundaries, good to a pixel. The rules are deliberately
//!    simple and approximate: they find the named features on a standing
//!    figure in a T or A pose and can pick the wrong row on other poses.
//!    - `height`: the aligned box height (exact, not from the mask).
//!    - `arm_span`: front mask, the widest row (leftmost to rightmost pixel).
//!    - `hat_brim_width`: the widest row within the top 12% of the height,
//!      reported only when it is more than 1.15 times the head width (else
//!      not found: no hat, or the top band is the head itself). Without a hat
//!      the band assumes the head is at least 12% of the height; a smaller
//!      head lets the band reach the shoulders and spoils head width too.
//!    - `head_width`: the narrowest central span (see the coat hem) between
//!      that widest row and 25% of the height down from the top. The central
//!      run of a row is the run of covered pixels holding the centre column,
//!      else the run nearest it.
//!    - `shoulder_width`: the central runs of the rows below the neck (the
//!      narrowest central span between the top band's widest row and half the
//!      height, so big-headed figures work too), down to half the height or
//!      the first row whose central run is the whole row and passes 80% of
//!      the arm span (T-pose arms joining the body). Among the rows at least 1.5 times the neck's width, the one
//!      whose width grows least over the next two rows (the shoulder plateau
//!      between the neck slope and the arms; the first on ties). On a T-pose
//!      with a cape-like yoke this is the yoke's width; with no head row
//!      found there is no shoulder width either.
//!    - `coat_hem_height`: scanning up from 25% to 55% of the height, the
//!      central span (the central run, or when the centre column is empty
//!      between the legs, from the nearest run on the left to the nearest run
//!      on the right) is compared with the span three rows higher; the
//!      biggest outward jump of at least max(3 pixels, 6%) is the hem, placed
//!      at the sharpest single-row step inside that window. Else not found.
//!    - `boot_top_height`: the same scan of the single-leg width (the widest
//!      run inside the central span) from 15% to 35% of the height, stopping
//!      below the hem when one was found, taking the biggest change in either
//!      direction (boots can be wider or narrower than the trouser leg).
//!    - `side_depth`: side mask, the widest row.
//! 5. **Sheet.** One row per view: target (shaded grey), ours (shaded, COLOR_0
//!    vertex colours when present, else grey) and the overlay (grey both, red
//!    target only, blue ours only), with labels and the numbers. Flat face
//!    shading, two-sided, so loose normals or winding in a target don't
//!    matter. Bytes are identical for any thread count.
//!
//! Nothing of the target is kept beyond the run.

use crate::intent::Diag;
use crate::mesh::Mesh;
use crate::png::Grey;
use crate::preview::{self, Camera, Proj, Raster};
use crate::vec::*;
use serde_json::{Map, Value, json};

/// A loaded or compiled model: a world-space mesh and optional vertex colours
/// (linear RGB, 0 to 1).
#[derive(Clone, Debug, Default)]
pub struct Shape {
    pub mesh: Mesh,
    pub colors: Option<Vec<V3>>,
}

impl Shape {
    pub fn from_mesh(mesh: Mesh) -> Shape {
        Shape { mesh, colors: None }
    }
}

fn bad(code: &'static str, msg: impl Into<String>, fix: impl Into<String>) -> Vec<Diag> {
    vec![Diag::err(code, 0, msg, fix)]
}

const FIX_GLB: &str = "give a glTF 2.0 binary (.glb) with embedded buffers, e.g. one exported from Blender or Meshy";

// ---------------------------------------------------------------- loader

/// 4 x 4 matrix, column-major as in glTF.
type M4 = [f64; 16];
const M4ID: M4 = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

fn m4_mul(a: &M4, b: &M4) -> M4 {
    let mut o = [0.0; 16];
    for c in 0..4 {
        for r in 0..4 {
            let mut s = 0.0;
            for k in 0..4 {
                s += a[k * 4 + r] * b[c * 4 + k];
            }
            o[c * 4 + r] = s;
        }
    }
    o
}

fn m4_point(m: &M4, p: V3) -> V3 {
    [
        m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
        m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
        m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
    ]
}

fn m4_det3(m: &M4) -> f64 {
    m[0] * (m[5] * m[10] - m[9] * m[6]) - m[4] * (m[1] * m[10] - m[9] * m[2]) + m[8] * (m[1] * m[6] - m[5] * m[2])
}

fn nums(v: &Value, n: usize, what: &str, node: usize) -> Result<Vec<f64>, Vec<Diag>> {
    let a = v.as_array().filter(|a| a.len() == n).ok_or_else(|| bad("glb_read", format!("node {node}: `{what}` is not {n} numbers"), FIX_GLB))?;
    a.iter().map(|x| x.as_f64().ok_or_else(|| bad("glb_read", format!("node {node}: `{what}` holds a non-number"), FIX_GLB))).collect()
}

fn local_matrix(n: &Value, idx: usize) -> Result<M4, Vec<Diag>> {
    if !n["matrix"].is_null() {
        let m = nums(&n["matrix"], 16, "matrix", idx)?;
        let mut o = [0.0; 16];
        o.copy_from_slice(&m);
        return Ok(o);
    }
    let t = if n["translation"].is_null() { vec![0.0; 3] } else { nums(&n["translation"], 3, "translation", idx)? };
    let q = if n["rotation"].is_null() { vec![0.0, 0.0, 0.0, 1.0] } else { nums(&n["rotation"], 4, "rotation", idx)? };
    let s = if n["scale"].is_null() { vec![1.0; 3] } else { nums(&n["scale"], 3, "scale", idx)? };
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    // r[row][col] of the rotation.
    let r = [
        [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * w), 2.0 * (x * z + y * w)],
        [2.0 * (x * y + z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * w)],
        [2.0 * (x * z - y * w), 2.0 * (y * z + x * w), 1.0 - 2.0 * (x * x + y * y)],
    ];
    let mut o = M4ID;
    for c in 0..3 {
        for rr in 0..3 {
            o[c * 4 + rr] = r[rr][c] * s[c];
        }
    }
    o[12] = t[0];
    o[13] = t[1];
    o[14] = t[2];
    Ok(o)
}

fn le_u32(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes([*b.get(o)?, *b.get(o + 1)?, *b.get(o + 2)?, *b.get(o + 3)?]))
}

struct Doc<'a> {
    js: Value,
    bin: &'a [u8],
}

impl Doc<'_> {
    /// An accessor's values as f64 (normalised integers mapped to their
    /// float range) and its component count.
    fn accessor(&self, i: usize) -> Result<(Vec<f64>, usize), Vec<Diag>> {
        let e = |m: String| bad("glb_read", format!("accessor {i}: {m}"), FIX_GLB);
        let a = &self.js["accessors"][i];
        if a.is_null() {
            return Err(e("missing".into()));
        }
        if !a["sparse"].is_null() {
            return Err(bad("glb_sparse", format!("accessor {i} is sparse, which the compare loader does not read"), "export the target without sparse accessors (Blender: plain glTF binary export)"));
        }
        let count = a["count"].as_u64().ok_or_else(|| e("no count".into()))? as usize;
        let comps = match a["type"].as_str() {
            Some("SCALAR") => 1,
            Some("VEC2") => 2,
            Some("VEC3") => 3,
            Some("VEC4") => 4,
            _ => return Err(e("type is not SCALAR or VEC2 to VEC4".into())),
        };
        let ct = a["componentType"].as_u64().ok_or_else(|| e("no componentType".into()))?;
        let size = match ct {
            5120 | 5121 => 1,
            5122 | 5123 => 2,
            5125 | 5126 => 4,
            _ => return Err(e(format!("component type {ct} is not a glTF type"))),
        };
        let Some(vi) = a["bufferView"].as_u64() else {
            // No view: all zeros (glTF).
            return Ok((vec![0.0; count * comps], comps));
        };
        let v = &self.js["bufferViews"][vi as usize];
        let buffer = v["buffer"].as_u64().unwrap_or(0);
        if buffer != 0 || self.js["buffers"][0]["uri"].is_string() {
            return Err(bad("glb_external", format!("accessor {i} reads an external buffer"), "give a .glb whose geometry is in its own BIN chunk (no separate .bin file)"));
        }
        let off = v["byteOffset"].as_u64().unwrap_or(0) as usize + a["byteOffset"].as_u64().unwrap_or(0) as usize;
        let view_end = v["byteOffset"].as_u64().unwrap_or(0) as usize + v["byteLength"].as_u64().ok_or_else(|| e("view has no byteLength".into()))? as usize;
        let stride = v["byteStride"].as_u64().map(|s| s as usize).filter(|s| *s > 0).unwrap_or(size * comps);
        let norm = a["normalized"].as_bool().unwrap_or(false);
        if count > 0 && (off + (count - 1) * stride + size * comps > view_end.min(self.bin.len())) {
            return Err(e("reads past its buffer view or the BIN chunk".into()));
        }
        let b = self.bin;
        let mut out = Vec::with_capacity(count * comps);
        for k in 0..count {
            for c in 0..comps {
                let o = off + k * stride + c * size;
                let x = match ct {
                    5120 => {
                        let v = b[o] as i8 as f64;
                        if norm { (v / 127.0).max(-1.0) } else { v }
                    }
                    5121 => {
                        let v = b[o] as f64;
                        if norm { v / 255.0 } else { v }
                    }
                    5122 => {
                        let v = i16::from_le_bytes([b[o], b[o + 1]]) as f64;
                        if norm { (v / 32767.0).max(-1.0) } else { v }
                    }
                    5123 => {
                        let v = u16::from_le_bytes([b[o], b[o + 1]]) as f64;
                        if norm { v / 65535.0 } else { v }
                    }
                    5125 => {
                        let v = u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) as f64;
                        if norm { v / 4294967295.0 } else { v }
                    }
                    _ => f32::from_bits(u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])) as f64,
                };
                out.push(x);
            }
        }
        Ok((out, comps))
    }

    fn mesh_into(&self, mi: usize, m: &M4, out: &mut Mesh, colors: &mut Vec<Option<V3>>) -> Result<(), Vec<Diag>> {
        let flip = m4_det3(m) < 0.0;
        let prims = self.js["meshes"][mi]["primitives"].as_array().ok_or_else(|| bad("glb_read", format!("mesh {mi} has no primitives"), FIX_GLB))?;
        for (pi, p) in prims.iter().enumerate() {
            let mode = p["mode"].as_u64().unwrap_or(4);
            if !matches!(mode, 4..=6) {
                continue; // points and lines have no area
            }
            let pa = p["attributes"]["POSITION"].as_u64().ok_or_else(|| bad("glb_read", format!("mesh {mi} primitive {pi} has no POSITION"), FIX_GLB))? as usize;
            let (pos, comps) = self.accessor(pa)?;
            if comps != 3 {
                return Err(bad("glb_read", format!("mesh {mi} primitive {pi}: POSITION is not VEC3"), FIX_GLB));
            }
            let nv = pos.len() / 3;
            let idx: Vec<u32> = match p["indices"].as_u64() {
                Some(ia) => {
                    let (ix, _) = self.accessor(ia as usize)?;
                    let mut v = Vec::with_capacity(ix.len());
                    for x in ix {
                        if x < 0.0 || x >= nv as f64 {
                            return Err(bad("glb_read", format!("mesh {mi} primitive {pi}: index {x} out of range ({nv} vertices)"), FIX_GLB));
                        }
                        v.push(x as u32);
                    }
                    v
                }
                None => (0..nv as u32).collect(),
            };
            let mut tris: Vec<[u32; 3]> = Vec::new();
            match mode {
                4 => tris.extend(idx.as_chunks::<3>().0.iter().copied()),
                5 => {
                    for k in 2..idx.len() {
                        tris.push(if k % 2 == 0 { [idx[k - 2], idx[k - 1], idx[k]] } else { [idx[k - 1], idx[k - 2], idx[k]] });
                    }
                }
                _ => {
                    for k in 2..idx.len() {
                        tris.push([idx[0], idx[k - 1], idx[k]]);
                    }
                }
            }
            let col: Option<Vec<V3>> = match p["attributes"]["COLOR_0"].as_u64() {
                Some(ca) => {
                    let (c, cc) = self.accessor(ca as usize)?;
                    if (cc == 3 || cc == 4) && c.len() / cc == nv { Some((0..nv).map(|k| [c[k * cc], c[k * cc + 1], c[k * cc + 2]]).collect()) } else { None }
                }
                None => None,
            };
            let base = out.pos.len() as u32;
            for k in 0..nv {
                out.pos.push(m4_point(m, [pos[k * 3], pos[k * 3 + 1], pos[k * 3 + 2]]));
                colors.push(col.as_ref().map(|c| c[k]));
            }
            for t in tris {
                out.tris.push(if flip { [t[0] + base, t[2] + base, t[1] + base] } else { [t[0] + base, t[1] + base, t[2] + base] });
            }
        }
        Ok(())
    }

    fn walk(&self, node: usize, parent: &M4, depth: usize, out: &mut Mesh, colors: &mut Vec<Option<V3>>) -> Result<(), Vec<Diag>> {
        if depth > 256 {
            return Err(bad("glb_read", "the node tree is deeper than 256 levels or has a cycle", FIX_GLB));
        }
        let n = &self.js["nodes"][node];
        if n.is_null() {
            return Err(bad("glb_read", format!("node {node} does not exist"), FIX_GLB));
        }
        let world = m4_mul(parent, &local_matrix(n, node)?);
        if let Some(mi) = n["mesh"].as_u64() {
            // A skinned mesh ignores its node's transform (glTF 2.0).
            let m = if n["skin"].is_null() { world } else { M4ID };
            self.mesh_into(mi as usize, &m, out, colors)?;
        }
        for c in n["children"].as_array().into_iter().flatten() {
            let ci = c.as_u64().ok_or_else(|| bad("glb_read", format!("node {node}: a child index is not a number"), FIX_GLB))?;
            self.walk(ci as usize, &world, depth + 1, out, colors)?;
        }
        Ok(())
    }
}

/// Area-weighted vertex normals.
fn vertex_normals(m: &Mesh) -> Vec<V3> {
    let mut n = vec![[0.0; 3]; m.pos.len()];
    for t in &m.tris {
        let (a, b, c) = (m.pos[t[0] as usize], m.pos[t[1] as usize], m.pos[t[2] as usize]);
        let f = cross(sub(b, a), sub(c, a));
        for &v in t {
            n[v as usize] = add(n[v as usize], f);
        }
    }
    n.into_iter().map(|v| norm_or(v, [0.0, 1.0, 0.0])).collect()
}

/// Read every triangle of a glTF 2.0 binary's scene as one world-space mesh.
pub fn load_glb(glb: &[u8]) -> Result<Shape, Vec<Diag>> {
    if glb.get(0..4) != Some(b"glTF") {
        return Err(bad("glb_read", "not a glTF binary (no `glTF` magic)", FIX_GLB));
    }
    if le_u32(glb, 4) != Some(2) {
        return Err(bad("glb_read", "not glTF version 2", FIX_GLB));
    }
    let total = (le_u32(glb, 8).unwrap_or(0) as usize).min(glb.len());
    let (mut js, mut bin): (Option<Value>, &[u8]) = (None, &[]);
    let mut o = 12;
    while o + 8 <= total {
        let l = le_u32(glb, o).unwrap_or(0) as usize;
        let ty = &glb[o + 4..o + 8];
        let data = glb.get(o + 8..o + 8 + l).ok_or_else(|| bad("glb_read", "a chunk runs past the end of the file", "the file is cut short; export it again"))?;
        if ty == b"JSON" && js.is_none() {
            js = Some(serde_json::from_slice(data).map_err(|e| bad("glb_read", format!("JSON chunk: {e}"), FIX_GLB))?);
        } else if ty == b"BIN\0" && bin.is_empty() {
            bin = data;
        }
        o += 8 + l;
    }
    let Some(js) = js else { return Err(bad("glb_read", "no JSON chunk", FIX_GLB)) };
    let doc = Doc { js, bin };
    let roots: Vec<usize> = {
        let scene = doc.js["scene"].as_u64().unwrap_or(0) as usize;
        match doc.js["scenes"][scene]["nodes"].as_array() {
            Some(a) => a.iter().filter_map(Value::as_u64).map(|x| x as usize).collect(),
            None => {
                // No scene: every node that is nobody's child, in order.
                let nodes = doc.js["nodes"].as_array().map(Vec::len).unwrap_or(0);
                let mut child = vec![false; nodes];
                for n in doc.js["nodes"].as_array().into_iter().flatten() {
                    for c in n["children"].as_array().into_iter().flatten().filter_map(Value::as_u64) {
                        if let Some(f) = child.get_mut(c as usize) {
                            *f = true;
                        }
                    }
                }
                (0..nodes).filter(|i| !child[*i]).collect()
            }
        }
    };
    let mut mesh = Mesh::default();
    let mut cols: Vec<Option<V3>> = Vec::new();
    for r in roots {
        doc.walk(r, &M4ID, 0, &mut mesh, &mut cols)?;
    }
    if mesh.tris.is_empty() {
        return Err(bad("no_triangles", "the file's scene has no triangles", "give a .glb with at least one triangle mesh in its default scene"));
    }
    mesh.nrm = vertex_normals(&mesh);
    mesh.tri_part = vec![0; mesh.tris.len()];
    let colors = if cols.iter().any(Option::is_some) { Some(cols.into_iter().map(|c| c.unwrap_or([0.6, 0.6, 0.6])).collect()) } else { None };
    Ok(Shape { mesh, colors })
}

// ---------------------------------------------------------------- views

pub const VIEWS: [&str; 4] = ["front", "side", "back", "three_quarter"];

/// Lowest point to y = 0, box centred in x and z. Never rescales.
pub fn align(m: &mut Mesh) {
    let b = m.bounds();
    if b.is_empty() {
        return;
    }
    let c = b.centre();
    let shift = [-c[0], -b.min[1], -c[2]];
    for p in &mut m.pos {
        *p = add(*p, shift);
    }
}

/// preview.rs's named cameras plus `back` (camera on -z looking +z).
pub fn view_camera(view: &str, centre: V3, size: f64) -> Camera {
    if view == "back" {
        let fwd = [0.0, 0.0, 1.0];
        return Camera { eye: sub(centre, mul(fwd, size * 4.0)), fwd, up: [0.0, 1.0, 0.0], proj: Proj::Ortho { size }, centre };
    }
    Camera::named(view, centre, size)
}

fn basis(cam: &Camera) -> (V3, V3, V3) {
    let f = norm_or(cam.fwd, [0.0, 0.0, -1.0]);
    let r = norm_or(cross(f, cam.up), [1.0, 0.0, 0.0]);
    (r, cross(r, f), f)
}

/// The shared orthographic frame of one view: the box's projection plus a 5%
/// margin each side.
fn frame_size(view: &str, b: &Aabb) -> f64 {
    let cam = view_camera(view, b.centre(), 1.0);
    let mut e: f64 = 0.0;
    for k in 0..8 {
        let p = [if k & 1 == 0 { b.min[0] } else { b.max[0] }, if k & 2 == 0 { b.min[1] } else { b.max[1] }, if k & 4 == 0 { b.min[2] } else { b.max[2] }];
        let (u, v, _) = cam.project(p);
        e = e.max((u - 0.5).abs()).max((v - 0.5).abs());
    }
    (e * 2.0 * 1.1).max(1e-6)
}

/// preview.rs's mask rule on an existing raster.
fn mask_of(r: &Raster) -> Grey {
    let mut g = Grey::new(r.w, r.h, 0);
    for i in 0..r.w * r.h {
        if (0..4).filter(|k| r.tri[i * 4 + k] != u32::MAX).count() >= 2 {
            g.px[i] = 255;
        }
    }
    g
}

fn face_normals(m: &Mesh) -> Vec<V3> {
    m.tris
        .iter()
        .map(|t| {
            let (a, b, c) = (m.pos[t[0] as usize], m.pos[t[1] as usize], m.pos[t[2] as usize]);
            norm_or(cross(sub(b, a), sub(c, a)), [0.0, 0.0, 1.0])
        })
        .collect()
}

/// Shaded RGB panel: flat two-sided shading, white page, dark silhouette edge.
fn shade(r: &Raster, s: &Shape, fnrm: &[V3], cam: &Camera, mask: &Grey) -> Vec<u8> {
    let (w, h) = (r.w, r.h);
    let (rt, up, f) = basis(cam);
    let light = norm_or(add(add(mul(rt, -0.5), mul(up, 0.7)), mul(f, -0.5)), [0.0, 1.0, 0.0]);
    let mut px = vec![255u8; w * h * 3];
    for i in 0..w * h {
        let mut acc = [0.0; 3];
        for k in 0..4 {
            let j = i * 4 + k;
            let t = r.tri[j];
            if t == u32::MAX {
                acc = add(acc, [1.0; 3]);
                continue;
            }
            let tr = s.mesh.tris[t as usize];
            let base = match &s.colors {
                Some(c) => {
                    let b = r.bary[j];
                    let lin = add(add(mul(c[tr[0] as usize], b[0]), mul(c[tr[1] as usize], b[1])), mul(c[tr[2] as usize], b[2]));
                    // Linear to display with gamma 2 (sqrt is exact in IEEE).
                    [lin[0].clamp(0.0, 1.0).sqrt(), lin[1].clamp(0.0, 1.0).sqrt(), lin[2].clamp(0.0, 1.0).sqrt()]
                }
                None => [0.8, 0.8, 0.8],
            };
            let sh = 0.3 + 0.7 * dot(fnrm[t as usize], light).abs();
            acc = add(acc, mul(base, sh));
        }
        for c in 0..3 {
            px[i * 3 + c] = ((acc[c] / 4.0) * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
    for y in 0..h {
        for x in 0..w {
            if mask.get(x, y) == 0 {
                continue;
            }
            let edge = x == 0 || y == 0 || x + 1 == w || y + 1 == h || mask.get(x - 1, y) == 0 || mask.get(x + 1, y) == 0 || mask.get(x, y - 1) == 0 || mask.get(x, y + 1) == 0;
            if edge {
                px[(y * w + x) * 3..(y * w + x) * 3 + 3].copy_from_slice(&[45, 45, 45]);
            }
        }
    }
    px
}

const BOTH: [u8; 3] = [150, 150, 150];
const TARGET_ONLY: [u8; 3] = [225, 95, 80];
const OURS_ONLY: [u8; 3] = [60, 115, 225];

fn overlay(target: &Grey, ours: &Grey) -> Vec<u8> {
    let mut px = vec![255u8; target.w * target.h * 3];
    for i in 0..target.px.len() {
        let c = match (target.px[i] > 127, ours.px[i] > 127) {
            (true, true) => BOTH,
            (true, false) => TARGET_ONLY,
            (false, true) => OURS_ONLY,
            (false, false) => continue,
        };
        px[i * 3..i * 3 + 3].copy_from_slice(&c);
    }
    px
}

// ---------------------------------------------------------------- measures

/// A view's frame: maps mask rows and columns to metres.
#[derive(Clone, Copy, Debug)]
struct Frame {
    centre: V3,
    size: f64,
    n: usize,
}

impl Frame {
    fn pix(&self) -> f64 {
        self.size / self.n as f64
    }
    fn top(&self) -> f64 {
        self.centre[1] + self.size * 0.5
    }
    /// The row holding height `y` (clamped to the image).
    fn row_of(&self, y: f64) -> usize {
        (((self.top() - y) / self.pix()).floor().max(0.0) as usize).min(self.n - 1)
    }
    /// Height of the boundary between row `i - 1` (above) and row `i`.
    fn boundary_above(&self, i: usize) -> f64 {
        self.top() - i as f64 * self.pix()
    }
    /// Column holding x = 0 (both models are centred there).
    fn centre_col(&self) -> usize {
        (((0.0 - self.centre[0]) / self.size + 0.5) * self.n as f64).floor().clamp(0.0, (self.n - 1) as f64) as usize
    }
}

/// Runs of covered pixels in a row: (start, end exclusive).
fn runs(g: &Grey, y: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut x = 0;
    while x < g.w {
        if g.get(x, y) > 127 {
            let s = x;
            while x < g.w && g.get(x, y) > 127 {
                x += 1;
            }
            out.push((s, x));
        } else {
            x += 1;
        }
    }
    out
}

fn extent(rs: &[(usize, usize)]) -> usize {
    match (rs.first(), rs.last()) {
        (Some(a), Some(b)) => b.1 - a.0,
        _ => 0,
    }
}

/// The run holding column `cx`, else the nearest run.
fn central_run(rs: &[(usize, usize)], cx: usize) -> Option<(usize, usize)> {
    let d = |r: &(usize, usize)| if cx < r.0 { r.0 - cx } else { (cx + 1).saturating_sub(r.1) };
    rs.iter().copied().min_by_key(d)
}

/// The central run, or (centre empty) nearest run left to nearest run right.
fn central_span(rs: &[(usize, usize)], cx: usize) -> Option<(usize, usize)> {
    if let Some(r) = rs.iter().find(|r| r.0 <= cx && cx < r.1) {
        return Some(*r);
    }
    let left = rs.iter().rev().find(|r| r.1 <= cx);
    let right = rs.iter().find(|r| r.0 > cx);
    match (left, right) {
        (Some(l), Some(r)) => Some((l.0, r.1)),
        (Some(l), None) => Some(*l),
        (None, Some(r)) => Some(*r),
        (None, None) => None,
    }
}

/// Widest single run inside the central span (one leg under a coat).
fn leg_width(rs: &[(usize, usize)], cx: usize) -> usize {
    let Some(sp) = central_span(rs, cx) else { return 0 };
    rs.iter().filter(|r| r.0 >= sp.0 && r.1 <= sp.1).map(|r| r.1 - r.0).max().unwrap_or(0)
}

/// Scan rows upward from height `lo` to `hi` for the biggest step in
/// `width(row)` over three rows (outward only, or either way), at least
/// max(3 pixels, 6%); the step's height is the sharpest single-row change in
/// that window.
fn step_height(g: &Grey, f: &Frame, lo: f64, hi: f64, outward_only: bool, width: &dyn Fn(&[(usize, usize)]) -> usize) -> Option<f64> {
    let (bottom, top) = (f.row_of(lo), f.row_of(hi));
    if bottom < top + 3 {
        return None;
    }
    let w: Vec<i64> = (0..g.h).map(|y| if y >= top && y <= bottom { width(&runs(g, y)) as i64 } else { 0 }).collect();
    let score = |d: i64| if outward_only { d } else { d.abs() };
    let mut best: Option<(usize, i64)> = None;
    let mut i = bottom;
    while i >= top + 3 {
        let d = w[i - 3] - w[i];
        let need = 3.max((w[i] as f64 * 0.06).ceil() as i64);
        if score(d) >= need && best.is_none_or(|b| score(d) > b.1) {
            best = Some((i, score(d)));
        }
        i -= 1;
    }
    let (i, _) = best?;
    // Sharpest single-row step between rows i and i - 3.
    let mut k_best = i;
    let mut s_best = -1;
    for k in (i - 2..=i).rev() {
        let s = score(w[k - 1] - w[k]);
        if s > s_best {
            s_best = s;
            k_best = k;
        }
    }
    Some(f.boundary_above(k_best))
}

pub const MEASURES: [&str; 8] = ["height", "arm_span", "hat_brim_width", "head_width", "shoulder_width", "coat_hem_height", "boot_top_height", "side_depth"];

/// The region measures of one aligned model (see the module docs), in the
/// order of [`MEASURES`].
fn measure(front: &Grey, side: &Grey, ff: &Frame, sf: &Frame, height: f64) -> Vec<Option<f64>> {
    let px = ff.pix();
    let cx = ff.centre_col();
    let rows = |a: f64, b: f64| ff.row_of(b)..=ff.row_of(a); // heights a < b, top row first
    let row_runs: Vec<Vec<(usize, usize)>> = (0..front.h).map(|y| runs(front, y)).collect();
    let arm = row_runs.iter().map(|r| extent(r)).max().unwrap_or(0);
    let mut brim: Option<(usize, usize)> = None;
    for y in rows(height * 0.88, height) {
        let e = extent(&row_runs[y]);
        if e > 0 && brim.is_none_or(|b| e > b.1) {
            brim = Some((y, e));
        }
    }
    // Narrowest central run below the top band's widest row, down to `to`
    // (the first such row from the top).
    let narrowest = |to: f64| -> Option<(usize, usize)> {
        let (by, _) = brim?;
        let mut best: Option<(usize, usize)> = None;
        for y in by + 1..=ff.row_of(to) {
            // The central span, so that two legs never count as a neck.
            if let Some(r) = central_span(&row_runs[y], cx)
                && best.is_none_or(|b| r.1 - r.0 < b.1)
            {
                best = Some((y, r.1 - r.0));
            }
        }
        best
    };
    let head_w = narrowest(height * 0.75).map(|h| h.1);
    // A brim only when the top band is clearly wider than the head below it.
    let brim_w = brim.map(|b| b.1).filter(|b| head_w.is_some_and(|h| *b as f64 > 1.15 * h as f64));
    // The neck (shoulder search start) may sit low on big-headed figures.
    let head = narrowest(height * 0.5);
    // Down from the neck to half the height, stopping where the central run
    // is the whole row and passes 80% of the arm span (T-pose arms joined).
    let arm_cap = arm as f64 * 0.8;
    let shoulder = head.and_then(|(ny, nw)| {
        // Central runs of consecutive rows below the neck, up to the arms.
        let mut w: Vec<usize> = Vec::new();
        for y in ny + 1..=ff.row_of(height * 0.5) {
            match central_run(&row_runs[y], cx) {
                // Arms joined: the central run is the whole row and wide.
                Some(r) if (r.1 - r.0) as f64 <= arm_cap || r.1 - r.0 + 2 < extent(&row_runs[y]) => w.push(r.1 - r.0),
                _ => break,
            }
        }
        // The plateau: the row (once 1.5 x the neck wide) whose width grows
        // least over the next two rows; the first on ties.
        let mut best: Option<(i64, usize)> = None;
        for i in 0..w.len().saturating_sub(2) {
            if (w[i] as f64) < 1.5 * nw as f64 {
                continue;
            }
            let grow = w[i + 2] as i64 - w[i] as i64;
            if best.is_none_or(|b| grow < b.0) {
                best = Some((grow, w[i]));
            }
        }
        best.map(|b| b.1)
    });
    let span = |rs: &[(usize, usize)]| central_span(rs, cx).map(|s| s.1 - s.0).unwrap_or(0);
    let hem = step_height(front, ff, height * 0.25, height * 0.55, true, &span);
    let boot_hi = match hem {
        Some(hh) => (height * 0.35).min(hh - px),
        None => height * 0.35,
    };
    let leg = |rs: &[(usize, usize)]| leg_width(rs, cx);
    let boot = step_height(front, ff, height * 0.15, boot_hi, false, &leg);
    let depth = (0..side.h).map(|y| extent(&runs(side, y))).max().unwrap_or(0);
    let m = |p: usize| Some(p as f64 * px);
    vec![
        Some(height),
        m(arm),
        brim_w.map(|b| b as f64 * px),
        head_w.map(|p| p as f64 * px),
        shoulder.map(|p| p as f64 * px),
        hem,
        boot,
        Some(depth as f64 * sf.pix()),
    ]
}

// ---------------------------------------------------------------- compare

/// The result of one comparison.
#[derive(Clone, Debug)]
pub struct Comparison {
    /// IoU per view, in the order of [`VIEWS`].
    pub iou: [f64; 4],
    pub mean: f64,
    /// Front IoU over the rows between 30% and 45% of the target's height.
    pub iou_hem_band: f64,
    pub height_ours: f64,
    pub height_target: f64,
    /// (name, ours, target) in the order of [`MEASURES`].
    pub measures: Vec<(&'static str, Option<f64>, Option<f64>)>,
    pub sheet_w: usize,
    pub sheet_h: usize,
    /// RGB, 3 bytes per pixel.
    pub sheet: Vec<u8>,
}

fn r4(x: f64) -> f64 {
    let v = (x * 1e4).round() / 1e4;
    if v == 0.0 { 0.0 } else { v }
}
fn r1(x: f64) -> f64 {
    let v = (x * 10.0).round() / 10.0;
    if v == 0.0 { 0.0 } else { v }
}

impl Comparison {
    pub fn height_ratio(&self) -> f64 {
        if self.height_target > 0.0 { self.height_ours / self.height_target } else { 0.0 }
    }
    /// The numbers of the reply (without the sheet path, tier, counts, time).
    pub fn to_json(&self) -> Value {
        let mut iou = Map::new();
        for (v, x) in VIEWS.iter().zip(self.iou) {
            iou.insert((*v).into(), json!(r4(x)));
        }
        iou.insert("mean".into(), json!(r4(self.mean)));
        let mut ms = Map::new();
        for (name, o, t) in &self.measures {
            let v = match (o, t) {
                (Some(o), Some(t)) => json!({ "ours": r4(*o), "target": r4(*t), "diff_mm": r1((o - t) * 1000.0), "pct": if *t != 0.0 { json!(r1((o - t) / t * 100.0)) } else { Value::Null } }),
                _ => json!({ "ours": o.map(r4), "target": t.map(r4), "diff_mm": Value::Null, "pct": Value::Null, "note": "not found by the rule on one or both" }),
            };
            ms.insert((*name).into(), v);
        }
        json!({ "iou": Value::Object(iou), "iou_hem_band": r4(self.iou_hem_band), "height_ratio": r4(self.height_ratio()), "measures": Value::Object(ms) })
    }
}

/// IoU over the rows `rows` only.
fn iou_rows(a: &Grey, b: &Grey, rows: std::ops::RangeInclusive<usize>) -> f64 {
    let (mut i, mut u) = (0usize, 0usize);
    for y in rows {
        for x in 0..a.w.min(b.w) {
            let (p, q) = (a.get(x, y) > 127, b.get(x, y) > 127);
            i += usize::from(p && q);
            u += usize::from(p || q);
        }
    }
    if u == 0 { 1.0 } else { i as f64 / u as f64 }
}

fn put_text(px: &mut [u8], w: usize, x: usize, y: usize, s: usize, col: [u8; 3], t: &str) {
    let tw = t.chars().count() * 6 * s;
    let mut g = Grey::new(tw.max(1), 7 * s, 0);
    preview::text(&mut g, 0, 0, s, 255, t);
    let h = px.len() / 3 / w.max(1);
    for gy in 0..g.h {
        for gx in 0..g.w {
            let (sx, sy) = (x + gx, y + gy);
            if g.get(gx, gy) != 0 && sx < w && sy < h {
                px[(sy * w + sx) * 3..(sy * w + sx) * 3 + 3].copy_from_slice(&col);
            }
        }
    }
}

fn blit(px: &mut [u8], w: usize, panel: &[u8], pw: usize, ph: usize, x: usize, y: usize) {
    for j in 0..ph {
        let d = ((y + j) * w + x) * 3;
        px[d..d + pw * 3].copy_from_slice(&panel[j * pw * 3..(j + 1) * pw * 3]);
    }
}

fn fmt_m(v: Option<f64>) -> String {
    v.map(|x| format!("{x:.3} M")).unwrap_or_else(|| "N/A".into())
}

/// Compare `ours` with `target` at `n` x `n` pixels per view.
pub fn compare(ours: &Shape, target: &Shape, n: usize, threads: usize) -> Comparison {
    let n = n.clamp(64, 2048);
    let (mut o, mut t) = (ours.clone(), target.clone());
    align(&mut o.mesh);
    align(&mut t.mesh);
    let (bo, bt) = (o.mesh.bounds(), t.mesh.bounds());
    let (ho, ht) = (bo.size()[1].max(0.0), bt.size()[1].max(0.0));
    let ub = bo.union(&bt);
    let centre = ub.centre();
    let (fo, ft) = (face_normals(&o.mesh), face_normals(&t.mesh));
    let mut iou = [0.0; 4];
    let mut panels: Vec<[Vec<u8>; 3]> = Vec::new();
    let mut masks: Vec<(Grey, Grey, Frame)> = Vec::new();
    for (vi, view) in VIEWS.iter().enumerate() {
        let size = frame_size(view, &ub);
        let cam = view_camera(view, centre, size);
        let rt = preview::rasterize(&t.mesh, &cam, n, n, threads);
        let ro = preview::rasterize(&o.mesh, &cam, n, n, threads);
        let (mt, mo) = (mask_of(&rt), mask_of(&ro));
        iou[vi] = preview::iou(&mt, &mo);
        panels.push([shade(&rt, &t, &ft, &cam, &mt), shade(&ro, &o, &fo, &cam, &mo), overlay(&mt, &mo)]);
        masks.push((mt, mo, Frame { centre, size, n }));
    }
    let mean = iou.iter().sum::<f64>() / 4.0;
    let (ff, sf) = (masks[0].2, masks[1].2);
    let band = ff.row_of(ht * 0.45)..=ff.row_of(ht * 0.30);
    let iou_hem_band = iou_rows(&masks[0].0, &masks[0].1, band);
    let mt = measure(&masks[0].0, &masks[1].0, &ff, &sf, ht);
    let mo = measure(&masks[0].1, &masks[1].1, &ff, &sf, ho);
    let measures: Vec<(&'static str, Option<f64>, Option<f64>)> = MEASURES.iter().enumerate().map(|(i, name)| (*name, mo[i], mt[i])).collect();
    // Sheet: header, one row per view (target, ours, overlay), footer.
    let head = 48;
    let foot = 32 + 18 * (measures.len() + 1);
    let (w, h) = (n * 3, head + n * 4 + foot);
    let mut px = vec![255u8; w * h * 3];
    put_text(&mut px, w, 8, 6, 2, [20, 20, 20], "EVANI.MODEL COMPARE    TARGET    OURS    OVERLAY:");
    let lx = 8 + 50 * 12;
    put_text(&mut px, w, lx, 6, 2, [110, 110, 110], "BOTH");
    put_text(&mut px, w, lx + 6 * 12, 6, 2, TARGET_ONLY, "TARGET ONLY");
    put_text(&mut px, w, lx + 18 * 12, 6, 2, OURS_ONLY, "OURS ONLY");
    put_text(&mut px, w, 8, 26, 2, [90, 90, 90], "ALIGNED ON THE GROUND AND CENTRED, NEVER RESCALED. ONE ORTHOGRAPHIC FRAME PER VIEW SHARED BY BOTH.");
    for (vi, view) in VIEWS.iter().enumerate() {
        let y0 = head + vi * n;
        for (pi, p) in panels[vi].iter().enumerate() {
            blit(&mut px, w, p, n, n, pi * n, y0);
        }
        for k in 0..w {
            px[(y0 * w + k) * 3..(y0 * w + k) * 3 + 3].copy_from_slice(&[170, 170, 170]);
        }
        for x in [n, 2 * n] {
            for j in 0..n {
                px[((y0 + j) * w + x) * 3..((y0 + j) * w + x) * 3 + 3].copy_from_slice(&[170, 170, 170]);
            }
        }
        let name = view.replace('_', " ").to_uppercase();
        put_text(&mut px, w, 8, y0 + 8, 2, [20, 20, 20], &format!("{name}  TARGET"));
        put_text(&mut px, w, n + 8, y0 + 8, 2, [20, 20, 20], &format!("{name}  OURS"));
        put_text(&mut px, w, 2 * n + 8, y0 + 8, 2, [20, 20, 20], &if *view == "back" { format!("IOU {:.4} (FRONT, MIRRORED)", iou[vi]) } else { format!("IOU {:.4}", iou[vi]) });
    }
    let fy = head + 4 * n + 8;
    for k in 0..w {
        px[((head + 4 * n) * w + k) * 3..((head + 4 * n) * w + k) * 3 + 3].copy_from_slice(&[170, 170, 170]);
    }
    let ratio = if ht > 0.0 { ho / ht } else { 0.0 };
    put_text(&mut px, w, 8, fy, 2, [20, 20, 20], &format!("MEAN IOU {mean:.4}   HEM BAND IOU {iou_hem_band:.4}   HEIGHT RATIO OURS/TARGET {ratio:.4}"));
    for (i, (name, o, t)) in measures.iter().enumerate() {
        let diff = match (o, t) {
            (Some(o), Some(t)) if *t != 0.0 => format!("{:+.0} MM ({:+.1}%)", (o - t) * 1000.0, (o - t) / t * 100.0),
            _ => "N/A (NOT FOUND BY THE RULE)".into(),
        };
        let line = format!("{:<16} OURS {:>7}   TARGET {:>7}   DIFF {diff}", name.replace('_', " ").to_uppercase(), fmt_m(*o), fmt_m(*t));
        put_text(&mut px, w, 8, fy + 18 * (i + 1), 2, [20, 20, 20], &line);
    }
    Comparison { iou, mean, iou_hem_band, height_ours: ho, height_target: ht, measures, sheet_w: w, sheet_h: h, sheet: px }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Axis-aligned boxes as one triangle soup (12 triangles each).
    fn boxes(list: &[(V3, V3)]) -> Mesh {
        let mut m = Mesh::default();
        for (lo, hi) in list {
            let base = m.pos.len() as u32;
            for k in 0..8 {
                m.pos.push([if k & 1 == 0 { lo[0] } else { hi[0] }, if k & 2 == 0 { lo[1] } else { hi[1] }, if k & 4 == 0 { lo[2] } else { hi[2] }]);
            }
            for f in [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]] {
                m.tris.push([base + f[0], base + f[1], base + f[2]]);
                m.tris.push([base + f[0], base + f[2], base + f[3]]);
            }
        }
        m.nrm = vertex_normals(&m);
        m.tri_part = vec![0; m.tris.len()];
        m
    }

    /// A blocky standing figure with known measures (1.8 m tall).
    fn figure() -> Mesh {
        boxes(&[
            ([-0.23, 0.0, -0.1], [-0.02, 0.35, 0.15]),   // boots
            ([0.02, 0.0, -0.1], [0.23, 0.35, 0.15]),     //
            ([-0.2, 0.0, -0.08], [-0.05, 0.7, 0.08]),    // legs
            ([0.05, 0.0, -0.08], [0.2, 0.7, 0.08]),      //
            ([-0.3, 0.6, -0.15], [0.3, 1.4, 0.15]),      // coat
            ([-0.75, 1.2, -0.05], [-0.3, 1.3, 0.05]),    // arms
            ([0.3, 1.2, -0.05], [0.75, 1.3, 0.05]),      //
            ([-0.1, 1.4, -0.1], [0.1, 1.62, 0.1]),       // head
            ([-0.25, 1.62, -0.25], [0.25, 1.66, 0.25]),  // brim
            ([-0.12, 1.66, -0.12], [0.12, 1.8, 0.12]),   // crown
        ])
    }

    fn scaled(m: &Mesh, s: V3) -> Mesh {
        let mut o = m.clone();
        for p in &mut o.pos {
            *p = hadamard(*p, s);
        }
        o
    }

    #[test]
    fn identical_inputs_score_one_and_wrong_ones_lower() {
        let f = Shape::from_mesh(figure());
        let c = compare(&f, &f, 256, 2);
        for x in c.iou {
            assert_eq!(x, 1.0);
        }
        assert_eq!(c.mean, 1.0);
        assert_eq!(c.iou_hem_band, 1.0);
        assert_eq!(c.height_ratio(), 1.0);
        // A whole-model shift is undone by the alignment.
        let mut moved = figure();
        for p in &mut moved.pos {
            *p = add(*p, [0.4, 0.3, -0.2]);
        }
        assert_eq!(compare(&Shape::from_mesh(moved), &f, 256, 2).mean, 1.0);
        let taller = compare(&Shape::from_mesh(scaled(&figure(), [1.0, 1.1, 1.0])), &f, 256, 2);
        let wider = compare(&Shape::from_mesh(scaled(&figure(), [1.1, 1.0, 1.0])), &f, 256, 2);
        println!("drop taller {:.4} wider {:.4}", 1.0 - taller.mean, 1.0 - wider.mean);
        assert!(1.0 - taller.mean >= 0.05, "taller drop {}", 1.0 - taller.mean);
        assert!(1.0 - wider.mean >= 0.05, "wider drop {}", 1.0 - wider.mean);
        assert!((taller.height_ratio() - 1.1).abs() < 1e-9);
    }

    #[test]
    fn alignment_puts_the_lowest_point_on_the_ground() {
        let mut m = figure();
        for p in &mut m.pos {
            *p = add(*p, [1.0, -2.5, 3.0]);
        }
        align(&mut m);
        let b = m.bounds();
        assert_eq!(b.min[1], 0.0);
        assert!(b.centre()[0].abs() < 1e-12 && b.centre()[2].abs() < 1e-12);
        assert!((b.size()[1] - 1.8).abs() < 1e-12);
    }

    #[test]
    fn measures_of_a_known_figure_within_a_pixel() {
        let f = Shape::from_mesh(figure());
        let c = compare(&f, &f, 512, 2);
        // Frame: 1.8 m tall, 1.5 m wide, plus margins: 1.98 m over 512 px.
        let px = 1.98 / 512.0;
        let truth = [1.8, 1.5, 0.5, 0.2, 0.6, 0.6, 0.35, 0.5];
        for ((name, o, t), want) in c.measures.iter().zip(truth) {
            let got = o.unwrap_or_else(|| panic!("{name} not found"));
            assert_eq!(*o, *t);
            assert!((got - want).abs() <= px * 1.5, "{name}: {got} vs {want} (pixel {px})");
        }
    }

    #[test]
    fn arms_down_figure_without_a_hat() {
        // Arms hang 2 cm from the torso; no hat.
        let m = boxes(&[
            ([-0.18, 0.0, -0.08], [-0.03, 0.9, 0.08]),  // legs
            ([0.03, 0.0, -0.08], [0.18, 0.9, 0.08]),    //
            ([-0.2, 0.9, -0.12], [0.2, 1.5, 0.12]),     // torso
            ([-0.3, 0.85, -0.05], [-0.22, 1.45, 0.05]), // arms
            ([0.22, 0.85, -0.05], [0.3, 1.45, 0.05]),   //
            ([-0.08, 1.5, -0.09], [0.08, 1.75, 0.09]),  // head (14% of the height)
        ]);
        let s = Shape::from_mesh(m);
        let c = compare(&s, &s, 512, 2);
        let px = 1.925 / 512.0;
        let get = |n: &str| c.measures.iter().find(|m| m.0 == n).and_then(|m| m.1);
        assert_eq!(get("hat_brim_width"), None);
        for (n, want) in [("arm_span", 0.6), ("head_width", 0.16), ("shoulder_width", 0.4)] {
            let got = get(n).unwrap_or_else(|| panic!("{n} not found"));
            assert!((got - want).abs() <= px * 1.5, "{n}: {got} vs {want}");
        }
    }

    #[test]
    fn sheet_bytes_identical_for_any_thread_count() {
        let mut col = Shape::from_mesh(figure());
        col.colors = Some(col.mesh.pos.iter().map(|p| [0.2 + p[1] * 0.3, 0.4, 0.6]).collect());
        let t = Shape::from_mesh(scaled(&figure(), [1.05, 0.95, 1.0]));
        let a = compare(&col, &t, 128, 1);
        let b = compare(&col, &t, 128, 4);
        assert_eq!(a.sheet, b.sheet);
        assert_eq!(a.sheet.len(), a.sheet_w * a.sheet_h * 3);
        assert_eq!(crate::png::write_rgb(a.sheet_w, a.sheet_h, &a.sheet), crate::png::write_rgb(b.sheet_w, b.sheet_h, &b.sheet));
    }

    #[test]
    fn loader_reads_our_compiled_tier() {
        let j = crate::intent::parse_text(include_str!("../../../tests/model_bench/vehicles/kart_01_pip.evmodel")).unwrap();
        let tiers = crate::compile::compile(&j, Some(&["low".to_string()]), 2).unwrap();
        let tm = &tiers[0];
        let col = crate::collide::build_collision(&tm.model);
        let glb = crate::gltf::write_tier(tm, &col, None, &Map::new());
        let s = load_glb(&glb).unwrap();
        let world = tm.world_mesh();
        assert_eq!(s.mesh.pos.len(), world.pos.len());
        assert_eq!(s.mesh.tris, world.tris);
        let step = 3.2 / 65535.0;
        for (x, y) in s.mesh.pos.iter().zip(&world.pos) {
            assert!(dist(*x, *y) < step, "{x:?} vs {y:?}");
        }
        assert!(s.colors.is_none());
    }

    #[test]
    fn loader_applies_node_transforms_and_skips_skinned_nodes() {
        // A plain float file of one box, then wrapped in a parent with a
        // rotation and a child with a matrix.
        let m = boxes(&[([0.0, 0.0, 0.0], [1.0, 2.0, 3.0])]);
        let glb = crate::gltf::write_plain(&[("b".to_string(), &m)], &json!({}));
        let s = load_glb(&glb).unwrap();
        assert_eq!(s.mesh.tris.len(), 12);
        // Rebuild the JSON with a node tree.
        let jl = le_u32(&glb, 12).unwrap() as usize;
        let mut js: Value = serde_json::from_slice(&glb[20..20 + jl]).unwrap();
        let bin = glb[20 + jl + 8..].to_vec();
        js["nodes"] = json!([
            { "children": [1, 2], "rotation": [0.0, std::f64::consts::FRAC_1_SQRT_2, 0.0, std::f64::consts::FRAC_1_SQRT_2], "translation": [10.0, 0.0, 0.0] },
            { "mesh": 0, "matrix": [2.0,0.0,0.0,0.0, 0.0,1.0,0.0,0.0, 0.0,0.0,1.0,0.0, 0.0,5.0,0.0,1.0] },
            { "mesh": 0, "skin": 0, "translation": [100.0, 0.0, 0.0] }
        ]);
        js["scenes"] = json!([{ "nodes": [0] }]);
        let mut text = serde_json::to_vec(&js).unwrap();
        while !text.len().is_multiple_of(4) {
            text.push(b' ');
        }
        let mut out = Vec::new();
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&((12 + 8 + text.len() + 8 + bin.len()) as u32).to_le_bytes());
        out.extend_from_slice(&(text.len() as u32).to_le_bytes());
        out.extend_from_slice(b"JSON");
        out.extend_from_slice(&text);
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&bin);
        let s = load_glb(&out).unwrap();
        assert_eq!(s.mesh.tris.len(), 24);
        // Node 1: x scaled by 2, lifted 5, then turned 90 degrees about y
        // (x -> -z) and moved 10 along x.
        let b1 = Mesh { pos: s.mesh.pos[..8].to_vec(), ..Default::default() }.bounds();
        assert!(dist(b1.min, [10.0, 5.0, -2.0]) < 1e-9, "{:?}", b1);
        assert!(dist(b1.max, [13.0, 7.0, 0.0]) < 1e-9, "{:?}", b1);
        // Node 2 is skinned: bind pose as stored.
        let b2 = Mesh { pos: s.mesh.pos[8..].to_vec(), ..Default::default() }.bounds();
        assert!(dist(b2.min, [0.0; 3]) < 1e-9 && dist(b2.max, [1.0, 2.0, 3.0]) < 1e-9, "{:?}", b2);
        assert!(load_glb(b"nope").is_err());
    }

    /// The Meshy master is outside git (art-source); run with --ignored.
    #[test]
    #[ignore]
    fn reads_the_meshy_master() {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../art-source/meshy/frontier-marshal/frontier-marshal.master.glb");
        let s = load_glb(&std::fs::read(p).unwrap()).unwrap();
        let b = s.mesh.bounds();
        println!("tris {} box {:?}", s.mesh.tris.len(), b);
        assert!(s.mesh.tris.len() > 15_000 && s.mesh.tris.len() < 25_000);
        assert!(b.size()[1] > b.size()[0] * 0.8);
    }
}
