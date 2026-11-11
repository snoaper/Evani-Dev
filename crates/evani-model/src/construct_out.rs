//! Output for construct models (the exact-piece back end, crate
//! evani-construct; docs/MODELGEN_PLAN.md 3.6): one `.glb` per render tier
//! in Evani model format 1 (docs/MODEL_FORMAT.md), its report, and review
//! pictures.
//!
//! Layout of a tier file:
//! - the root node (named after the model) holds one **group node** per
//!   recipe node and stage set (`body`, `lid~intact`), named so a game hides
//!   and shows them by name (docs/CONTENT.md), with `extras.evani.kind:
//!   "group"` and its `stages`;
//! - each group node has one **solid** child (`<group>.mesh`) whose mesh has
//!   one primitive per catalogue material, and one solid child per placed
//!   **instance** (`studs_3`), which reuses its prototype's mesh (written
//!   once) with a rotation;
//! - `anchor:<name>` nodes (with a rotation for sockets), and the root
//!   `extras.evani` (format 1, `parts`, `dropped` (part names), `triangles`, `budget`,
//!   `sockets`, and a `construct` block: recipe hash, parameters, stages).
//!
//! Attributes (KHR_mesh_quantization, as the field writer): POSITION u16 over
//! the solid's box (undone by the node's scale and translation), NORMAL i8
//! (Low, Medium) or i16 (High and up), TEXCOORD_0 the grain in metres as u16
//! normalised over the primitive's range (`extras.evani.uv_min`, `uv_m` give
//! it back in metres), TEXCOORD_1.x the piece's variation value, COLOR_0 the
//! piece's tone. Materials carry the brightest tone of their pieces as
//! `baseColorFactor` and COLOR_0 the rest of the way down, so a glTF viewer
//! (colour x factor) shows each piece's exact tone, and a reader that uses
//! only the factor (the engine today) shows the material's colour.

use crate::png;
use crate::preview::{Camera, rasterize};
use crate::vec::*;
use evani_construct::model::{Model, OutMesh};
use serde_json::{Map, Value, json};

fn r6(x: f64) -> f64 {
    crate::gltf::r6(x)
}
fn v3(a: V3) -> Value {
    json!([r6(a[0]), r6(a[1]), r6(a[2])])
}
fn q4(q: Quat) -> Value {
    json!([r6(q[0]), r6(q[1]), r6(q[2]), r6(q[3])])
}

const ARRAY: u32 = 34962;
const ELEMENT: u32 = 34963;

#[derive(Default)]
struct Bin {
    data: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}

impl Bin {
    fn view(&mut self, bytes: &[u8], stride: Option<usize>, target: u32) -> usize {
        while !self.data.len().is_multiple_of(4) {
            self.data.push(0);
        }
        let mut v = json!({ "buffer": 0, "byteOffset": self.data.len(), "byteLength": bytes.len(), "target": target });
        if let Some(s) = stride {
            v["byteStride"] = json!(s);
        }
        self.data.extend_from_slice(bytes);
        self.views.push(v);
        self.views.len() - 1
    }
    fn accessor(&mut self, view: usize, ctype: u32, normalized: bool, count: usize, ty: &str, minmax: Option<(Value, Value)>) -> usize {
        let mut a = json!({ "bufferView": view, "componentType": ctype, "count": count, "type": ty });
        if normalized {
            a["normalized"] = json!(true);
        }
        if let Some((lo, hi)) = minmax {
            a["min"] = lo;
            a["max"] = hi;
        }
        self.accessors.push(a);
        self.accessors.len() - 1
    }
}

/// A solid written once: its glTF mesh index and its quantisation box.
struct Written {
    mesh: usize,
    min: V3,
    step: f64,
}

/// Write one node's meshes (one primitive per material) as one glTF mesh.
fn write_mesh(bin: &mut Bin, meshes: &mut Vec<Value>, mats: &[(String, [u8; 4])], name: &str, parts: &[OutMesh], hi_normals: bool) -> Option<Written> {
    let mut b = Aabb::EMPTY;
    for o in parts {
        for p in &o.mesh.pos {
            b.add_point(*p);
        }
    }
    if b.is_empty() {
        return None;
    }
    let size = b.size();
    let step = (size[0].max(size[1]).max(size[2]) / 65535.0).max(1e-9);
    let mut prims = Vec::new();
    for o in parts {
        let m = &o.mesh;
        if m.tris.is_empty() {
            continue;
        }
        let mut pb = Vec::with_capacity(m.pos.len() * 8);
        let (mut qmin, mut qmax) = ([u16::MAX; 3], [0u16; 3]);
        for p in &m.pos {
            for k in 0..3 {
                let q = ((p[k] - b.min[k]) / step).round().clamp(0.0, 65535.0) as u16;
                qmin[k] = qmin[k].min(q);
                qmax[k] = qmax[k].max(q);
                pb.extend_from_slice(&q.to_le_bytes());
            }
            pb.extend_from_slice(&[0, 0]);
        }
        let pv = bin.view(&pb, Some(8), ARRAY);
        let pa = bin.accessor(pv, 5123, false, m.pos.len(), "VEC3", Some((json!(qmin), json!(qmax))));
        let mut nb = Vec::new();
        let na = if hi_normals {
            for n in &m.nrm {
                for k in 0..3 {
                    nb.extend_from_slice(&((n[k] * 32767.0).round().clamp(-32767.0, 32767.0) as i16).to_le_bytes());
                }
                nb.extend_from_slice(&[0, 0]);
            }
            let v = bin.view(&nb, Some(8), ARRAY);
            bin.accessor(v, 5122, true, m.nrm.len(), "VEC3", None)
        } else {
            for n in &m.nrm {
                for k in 0..3 {
                    nb.push(((n[k] * 127.0).round().clamp(-127.0, 127.0) as i8) as u8);
                }
                nb.push(0);
            }
            let v = bin.view(&nb, Some(4), ARRAY);
            bin.accessor(v, 5120, true, m.nrm.len(), "VEC3", None)
        };
        // Grain UVs: metres, normalised over the primitive's range.
        let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for uv in &m.uv {
            for k in 0..2 {
                lo[k] = lo[k].min(uv[k]);
                hi[k] = hi[k].max(uv[k]);
            }
        }
        let span = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1e-6);
        let mut ub = Vec::with_capacity(m.uv.len() * 4);
        for uv in &m.uv {
            for k in 0..2 {
                ub.extend_from_slice(&((((uv[k] - lo[k]) / span).clamp(0.0, 1.0) * 65535.0).round() as u16).to_le_bytes());
            }
        }
        let uvv = bin.view(&ub, Some(4), ARRAY);
        let uva = bin.accessor(uvv, 5123, true, m.uv.len(), "VEC2", None);
        // Variation value per piece.
        let mut vb = Vec::with_capacity(o.var.len() * 4);
        for v in &o.var {
            vb.extend_from_slice(&((v.clamp(0.0, 1.0) * 65535.0).round() as u16).to_le_bytes());
            vb.extend_from_slice(&[0, 0]);
        }
        let vv = bin.view(&vb, Some(4), ARRAY);
        let va = bin.accessor(vv, 5123, true, o.var.len(), "VEC2", None);
        // Tone relative to the material's factor (the brightest piece).
        let mi = mats.iter().position(|x| x.0 == o.material).unwrap_or(0);
        let top = mats.get(mi).map_or([255; 4], |x| x.1);
        let mut cb = Vec::with_capacity(o.colour.len() * 4);
        for c in &o.colour {
            for k in 0..3 {
                let t = if top[k] == 0 { 255 } else { ((f64::from(c[k]) / f64::from(top[k])) * 255.0).round().clamp(0.0, 255.0) as u8 };
                cb.push(t);
            }
            cb.push(255);
        }
        let cv = bin.view(&cb, Some(4), ARRAY);
        let ca = bin.accessor(cv, 5121, true, o.colour.len(), "VEC4", None);
        let mut ib = Vec::new();
        let ctype = if m.pos.len() < 65536 {
            for t in &m.tris {
                for &v in t {
                    ib.extend_from_slice(&(v as u16).to_le_bytes());
                }
            }
            5123
        } else {
            for t in &m.tris {
                for &v in t {
                    ib.extend_from_slice(&v.to_le_bytes());
                }
            }
            5125
        };
        let iv = bin.view(&ib, None, ELEMENT);
        let ia = bin.accessor(iv, ctype, false, m.tris.len() * 3, "SCALAR", None);
        prims.push(json!({
            "attributes": { "POSITION": pa, "NORMAL": na, "TEXCOORD_0": uva, "TEXCOORD_1": va, "COLOR_0": ca },
            "indices": ia, "material": mi, "mode": 4,
            "extras": { "evani": { "uv_min": [r6(lo[0]), r6(lo[1])], "uv_m": r6(span), "pieces": o.pieces.len() } }
        }));
    }
    if prims.is_empty() {
        return None;
    }
    meshes.push(json!({ "name": name, "primitives": prims }));
    Some(Written { mesh: meshes.len() - 1, min: b.min, step })
}

/// The materials a model uses, in name order, each with its brightest
/// piece tone (per channel).
fn materials(m: &Model) -> Vec<(String, [u8; 4])> {
    let mut out: Vec<(String, [u8; 4])> = Vec::new();
    for p in m.pieces.iter().chain(m.protos.iter().flat_map(|x| x.pieces.iter())) {
        match out.iter_mut().find(|x| x.0 == p.material) {
            Some(x) => {
                for k in 0..4 {
                    x.1[k] = x.1[k].max(p.colour[k]);
                }
            }
            None => out.push((p.material.clone(), p.colour)),
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// One tier of a construct model as a `.glb`. `budget` is the tier's
/// triangle budget (0: none); `meta` adds keys to the root `extras.evani`.
pub fn write(m: &Model, budget: usize, meta: &Map<String, Value>) -> Vec<u8> {
    let hi_normals = matches!(m.tier.as_str(), "high" | "ultra" | "vr");
    let mats = materials(m);
    let mut bin = Bin::default();
    let mut meshes = Vec::new();
    let mut nodes: Vec<Value> = vec![Value::Null];
    let mut root_children = Vec::new();
    let (groups, protos) = m.output();
    // Prototype meshes, written once.
    let mut proto_mesh: Vec<(String, Written)> = Vec::new();
    for p in &protos {
        if let Some(w) = write_mesh(&mut bin, &mut meshes, &mats, &p.name, &p.meshes, hi_normals) {
            proto_mesh.push((p.name.clone(), w));
        }
    }
    let mut parts = Vec::new();
    for g in &groups {
        let gi = nodes.len();
        nodes.push(Value::Null);
        let mut children = Vec::new();
        if let Some(w) = write_mesh(&mut bin, &mut meshes, &mats, &g.name, &g.meshes, hi_normals) {
            nodes.push(json!({
                "name": format!("{}.mesh", g.name), "mesh": w.mesh,
                "translation": v3(w.min), "scale": [w.step, w.step, w.step],
                "extras": { "evani": { "kind": "solid", "origin": [0.0, 0.0, 0.0], "mirrored": false } }
            }));
            children.push(nodes.len() - 1);
        }
        let base_group = g.name.split('~').next().unwrap_or(&g.name);
        if g.stages.is_empty() {
            for inst in m.instances.iter().filter(|i| i.group == base_group) {
                let Some((_, w)) = proto_mesh.iter().find(|x| x.0 == inst.proto) else { continue };
                let sx = if inst.mirror { -1.0 } else { 1.0 };
                let t = add(inst.t, qrot(inst.q, [w.min[0] * sx, w.min[1], w.min[2]]));
                nodes.push(json!({
                    "name": inst.name, "mesh": w.mesh,
                    "translation": v3(t), "rotation": q4(inst.q), "scale": [w.step * sx, w.step, w.step],
                    "extras": { "evani": { "kind": "solid", "origin": v3(inst.t), "mirrored": inst.mirror, "proto": inst.proto } }
                }));
                children.push(nodes.len() - 1);
            }
        }
        let tris: usize = g.meshes.iter().map(|o| o.mesh.tris.len()).sum();
        nodes[gi] = json!({ "name": g.name, "children": children, "extras": { "evani": { "kind": "group", "stages": g.stages, "triangles": tris } } });
        root_children.push(gi);
        parts.push(json!({ "name": g.name, "role": "construct", "solid": format!("{}.mesh", g.name), "group": Value::Null, "generator": "construct", "dropped": false }));
    }
    // Parts left out at this tier are named once, in `dropped` (names only:
    // every lower tier would otherwise carry each higher tier's parts twice,
    // with reasons; the reasons are in `<model>.evmodel.json`).
    let mut sockets = Map::new();
    for a in &m.anchors {
        let mut node = json!({ "name": format!("anchor:{}", a.name), "translation": v3(a.p), "extras": { "evani": { "kind": "anchor", "normal": [0.0, 1.0, 0.0], "part": a.group } } });
        if let Some(q) = a.q {
            node["rotation"] = q4(q);
            node["extras"]["evani"]["q"] = q4(q);
            let mut s = json!({ "p": v3(a.p), "q": q4(q) });
            if !a.group.is_empty() {
                s["part"] = json!(a.group);
            }
            sockets.insert(a.name.clone(), s);
        }
        nodes.push(node);
        root_children.push(nodes.len() - 1);
    }
    nodes[0] = json!({ "name": m.id, "children": root_children });
    let gmats: Vec<Value> = mats
        .iter()
        .map(|(name, c)| {
            let metal = name.starts_with("metal.");
            json!({
                "name": name,
                "pbrMetallicRoughness": {
                    "baseColorFactor": [r6(f64::from(c[0]) / 255.0), r6(f64::from(c[1]) / 255.0), r6(f64::from(c[2]) / 255.0), 1.0],
                    "metallicFactor": if metal { 0.6 } else { 0.0 },
                    "roughnessFactor": if metal { 0.45 } else { 0.85 }
                },
                "extras": { "evani": { "material": name } }
            })
        })
        .collect();
    let mut evani = Map::new();
    evani.insert("format".into(), json!(1));
    evani.insert("class".into(), json!(m.id));
    evani.insert("style".into(), json!("construct"));
    evani.insert("tier".into(), json!(m.tier));
    evani.insert("seed".into(), json!(m.seed));
    evani.insert("intent".into(), json!(m.recipe_hash));
    evani.insert("tool".into(), json!(format!("Evani.Model construct {}", env!("CARGO_PKG_VERSION"))));
    evani.insert("parts".into(), Value::Array(parts));
    evani.insert("dropped".into(), Value::Array(m.dropped.iter().map(|d| json!(d.0)).collect()));
    evani.insert("triangles".into(), json!(m.triangles()));
    evani.insert("budget".into(), json!(budget));
    evani.insert("cell_m".into(), json!(0.0));
    if !sockets.is_empty() {
        evani.insert("sockets".into(), Value::Object(sockets));
    }
    let params: Map<String, Value> = m.params.iter().map(|(k, v)| (k.clone(), json!(r6(*v)))).collect();
    evani.insert(
        "construct".into(),
        json!({ "recipe": m.recipe_hash, "params": params, "stages": m.stages, "pieces": m.pieces.len(), "instances": m.instances.len(), "prototypes": m.protos.len() }),
    );
    for (k, v) in meta {
        evani.insert(k.clone(), v.clone());
    }
    let mut doc = Map::new();
    doc.insert("asset".into(), json!({ "version": "2.0", "generator": format!("Evani.Model {} (construct)", env!("CARGO_PKG_VERSION")) }));
    doc.insert("extensionsUsed".into(), json!(["KHR_mesh_quantization"]));
    doc.insert("extensionsRequired".into(), json!(["KHR_mesh_quantization"]));
    doc.insert("scene".into(), json!(0));
    doc.insert("scenes".into(), json!([{ "nodes": [0] }]));
    doc.insert("nodes".into(), Value::Array(nodes));
    doc.insert("meshes".into(), Value::Array(meshes));
    doc.insert("materials".into(), Value::Array(gmats));
    while !bin.data.len().is_multiple_of(4) {
        bin.data.push(0);
    }
    doc.insert("buffers".into(), json!([{ "byteLength": bin.data.len() }]));
    doc.insert("bufferViews".into(), Value::Array(bin.views));
    doc.insert("accessors".into(), Value::Array(bin.accessors));
    doc.insert("extras".into(), json!({ "evani": Value::Object(evani) }));
    let mut js = serde_json::to_string(&Value::Object(doc)).unwrap_or_default().into_bytes();
    while !js.len().is_multiple_of(4) {
        js.push(b' ');
    }
    let total = 12 + 8 + js.len() + 8 + bin.data.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(js.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&js);
    out.extend_from_slice(&(bin.data.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin.data);
    out
}

/// The model's pieces as one field-back-end mesh (`tri_part` = piece
/// index) and each piece's colour, for pictures and the shared checks.
pub fn world_mesh(m: &Model) -> (crate::mesh::Mesh, Vec<[u8; 4]>) {
    let mut out = crate::mesh::Mesh { pos: Vec::new(), nrm: Vec::new(), tris: Vec::new(), tri_part: Vec::new() };
    let mut cols = Vec::new();
    for (i, p) in m.world_pieces().iter().enumerate() {
        let base = out.pos.len() as u32;
        out.pos.extend_from_slice(&p.mesh.pos);
        out.nrm.extend_from_slice(&p.mesh.nrm);
        out.tris.extend(p.mesh.tris.iter().map(|t| [t[0] + base, t[1] + base, t[2] + base]));
        out.tri_part.extend(std::iter::repeat_n(i as u32, p.mesh.tris.len()));
        cols.push(p.colour);
    }
    (out, cols)
}

fn linear_to_srgb(c: f64) -> f64 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.0031308 { c * 12.92 } else { 1.055 * crate::dmath::pow(c, 1.0 / 2.4) - 0.055 }
}

/// A colour picture of the model (three-quarter view, `size` pixels square,
/// piece tones lit from the upper left, dark outlines), as RGB bytes.
pub fn picture(m: &Model, size: usize, view: &str, threads: usize) -> Vec<u8> {
    let (mesh, cols) = world_mesh(m);
    picture_mesh(&mesh, &cols, size, view, threads)
}

/// [`picture`] of any mesh whose `tri_part` indexes `cols` (one linear RGBA
/// per piece): geocheck draws a converted model's tier files with it.
pub fn picture_mesh(mesh: &crate::mesh::Mesh, cols: &[[u8; 4]], size: usize, view: &str, threads: usize) -> Vec<u8> {
    let b = mesh.bounds();
    if b.is_empty() {
        return vec![255u8; size * size * 3];
    }
    picture_framed(mesh, cols, size, view, b.centre(), len(b.size()).max(1e-6) * 1.1, threads)
}

/// [`picture_mesh`] with the framing given: `extent` metres across the
/// picture, centred on `centre` (two meshes drawn in the same frame line up
/// pixel for pixel).
pub fn picture_framed(mesh: &crate::mesh::Mesh, cols: &[[u8; 4]], size: usize, view: &str, centre: V3, extent: f64, threads: usize) -> Vec<u8> {
    let mut px = vec![255u8; size * size * 3];
    if mesh.tris.is_empty() {
        return px;
    }
    let cam = Camera::named(view, centre, extent);
    let r = rasterize(mesh, &cam, size, size, threads);
    let light = norm_or([-0.45, 0.8, 0.4], [0.0, 1.0, 0.0]);
    for y in 0..size {
        for x in 0..size {
            let mut acc = [0.0; 3];
            for k in 0..4 {
                let i = (y * size + x) * 4 + k;
                let t = r.tri[i];
                if t == u32::MAX {
                    for c in &mut acc {
                        *c += 1.0;
                    }
                    continue;
                }
                let tr = mesh.tris[t as usize];
                let bc = r.bary[i];
                let n = norm_or(add(add(mul(mesh.nrm[tr[0] as usize], bc[0]), mul(mesh.nrm[tr[1] as usize], bc[1])), mul(mesh.nrm[tr[2] as usize], bc[2])), [0.0, 1.0, 0.0]);
                let shade = 0.35 + 0.65 * dot(n, light).max(0.0);
                let col = cols.get(mesh.tri_part[t as usize] as usize).copied().unwrap_or([180; 4]);
                for c in 0..3 {
                    acc[c] += linear_to_srgb(f64::from(col[c]) / 255.0 * shade);
                }
            }
            for c in 0..3 {
                px[(y * size + x) * 3 + c] = ((acc[c] / 4.0) * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    // Outlines where a pixel's nearest piece differs from its neighbour's.
    let piece_at = |x: usize, y: usize| -> u32 {
        let i = (y * size + x) * 4;
        (0..4).map(|k| r.tri[i + k]).find(|t| *t != u32::MAX).map_or(u32::MAX, |t| mesh.tri_part[t as usize])
    };
    let mut edge = vec![false; size * size];
    for y in 0..size.saturating_sub(1) {
        for x in 0..size.saturating_sub(1) {
            let p = piece_at(x, y);
            if p != piece_at(x + 1, y) || p != piece_at(x, y + 1) {
                edge[y * size + x] = true;
            }
        }
    }
    for (i, e) in edge.iter().enumerate() {
        if *e {
            for c in 0..3 {
                px[i * 3 + c] = (f64::from(px[i * 3 + c]) * 0.35) as u8;
            }
        }
    }
    px
}

/// The tiers side by side with captions (triangles, bytes), as a PNG.
pub fn tier_sheet(tiers: &[(&Model, usize)], size: usize, threads: usize) -> Vec<u8> {
    let n = tiers.len().max(1);
    let (w, h) = (size * n, size + 24);
    let mut px = vec![255u8; w * h * 3];
    for (k, (m, bytes)) in tiers.iter().enumerate() {
        let pic = picture(m, size, "three_quarter", threads);
        for y in 0..size {
            let dst = ((y + 24) * w + k * size) * 3;
            px[dst..dst + size * 3].copy_from_slice(&pic[y * size * 3..(y + 1) * size * 3]);
        }
        let mut cap = png::Grey::new(size, 24, 255);
        let text = format!("{} {}T {}KB", m.tier.to_uppercase(), m.triangles(), bytes.div_ceil(1024));
        crate::preview::text(&mut cap, 6, 6, 2, 30, &text);
        for y in 0..24 {
            for x in 0..size {
                let v = cap.get(x, y);
                let i = (y * w + k * size + x) * 3;
                px[i] = v;
                px[i + 1] = v;
                px[i + 2] = v;
            }
        }
    }
    png::write_rgb(w, h, &px)
}

/// Tier gain (errors become tools: a sea chest's Ultra once cost 536 more
/// triangles than High and looked the same). Each tier has a design view:
/// the picture size, in pixels across, that the model fills when seen as
/// close as that tier is meant for. Picture framing as in [`picture`] (the
/// model's box diagonal plus 10% across the picture); on a 1440-pixel-tall
/// screen with a 50 degree view, a 1.3 m model fills 1024 pixels at about
/// 1.9 m (Ultra, a close-up), 512 at 3.8 m (High), 256 at 7.7 m (Medium)
/// and 128 at 15 m (Low).
pub fn design_px(tier: &str) -> usize {
    match tier {
        "lite" => 96,
        "low" => 128,
        "medium" => 256,
        "high" => 512,
        _ => 1024,
    }
}

/// The distance (m) at which a model of framed extent `extent_m` fills
/// `px` pixels on a 1440-pixel-tall screen with a 50 degree vertical view.
pub fn design_distance(extent_m: f64, px: usize) -> f64 {
    extent_m * 1440.0 / (2.0 * crate::dmath::tan(crate::dmath::deg(25.0)) * px.max(1) as f64)
}

/// The share of the model's pixels a costlier tier must change, at its own
/// design view, to count as visible. From the sea chest (docs/MODELGEN_PLAN.md,
/// "Tier gain"): an Ultra that only rounded its studs and rope changed
/// 0.03% (243 of 735,825 pixels); its real detail steps changed 8.1 to 8.3%.
/// 2% sits 60 times above the waste and 4 times below the weakest real step.
pub const TIER_GAIN_MIN: f64 = 0.02;
/// A pixel counts as changed when a colour channel moves by more than this
/// (of 255): above anti-aliasing noise, below any shading or outline change.
pub const TIER_GAIN_PIXEL: u8 = 24;
/// The views a tier pair is compared from: front three-quarter and its
/// opposite corner, so detail on the back counts too.
pub const TIER_GAIN_VIEWS: [&str; 2] = ["three_quarter", "three_quarter_back"];

/// One adjacent tier pair, compared at the higher tier's design view.
#[derive(Clone, Debug)]
pub struct TierGain {
    pub lower: String,
    pub higher: String,
    pub px: usize,
    /// Pixels changed and pixels covered by the model in either tier, summed
    /// over [`TIER_GAIN_VIEWS`].
    pub changed: usize,
    pub covered: usize,
    pub extra_triangles: i64,
    pub extra_bytes: i64,
    /// Framed extent (m) and the design distance it gives.
    pub extent_m: f64,
    pub distance_m: f64,
    /// Extra triangles per part, largest first (parts the higher tier adds or grows).
    pub by_part: Vec<(String, i64)>,
}

impl TierGain {
    pub fn share(&self) -> f64 {
        self.changed as f64 / self.covered.max(1) as f64
    }
    /// Costs more (triangles, or over 1 KB: the tier's own name and
    /// metadata differ by a few bytes) and shows too little.
    pub fn wasted(&self) -> bool {
        (self.extra_triangles > 0 || self.extra_bytes > 1024) && self.share() < TIER_GAIN_MIN
    }
    /// The warning, with its fix line.
    pub fn warning(&self) -> String {
        let parts: Vec<String> = self.by_part.iter().filter(|p| p.1 > 0).take(4).map(|(n, d)| format!("{n} +{d}")).collect();
        format!(
            "WARN tier gain: {} costs {} more triangles and {:.1} KB more than {} but changes only {:.1}% of the model's pixels at its design view ({} px across, about {:.1} m away; needs {:.0}%){}.\n  fix: make the extra detail visible at {:.1} m, or drop the {} tier so it falls back to {}.",
            self.higher,
            self.extra_triangles,
            self.extra_bytes as f64 / 1024.0,
            self.lower,
            self.share() * 100.0,
            self.px,
            self.distance_m,
            TIER_GAIN_MIN * 100.0,
            if parts.is_empty() { String::new() } else { format!("; the extra triangles: {}", parts.join(", ")) },
            self.distance_m,
            self.higher,
            self.lower
        )
    }
    pub fn to_json(&self) -> Value {
        json!({
            "lower": self.lower, "higher": self.higher, "design_px": self.px, "design_distance_m": r6(self.distance_m),
            "changed_px": self.changed, "covered_px": self.covered, "changed_share": r6(self.share()),
            "extra_triangles": self.extra_triangles, "extra_bytes": self.extra_bytes,
            "by_part": self.by_part.iter().map(|p| json!([p.0, p.1])).collect::<Vec<_>>(),
            "min_share": TIER_GAIN_MIN, "ok": !self.wasted()
        })
    }
}

/// Two pictures of the same size: (changed, covered) pixels.
pub fn pixel_change(a: &[u8], b: &[u8]) -> (usize, usize) {
    let mut changed = 0;
    let mut covered = 0;
    for (p, q) in a.as_chunks::<3>().0.iter().zip(b.as_chunks::<3>().0) {
        if *p != [255, 255, 255] || *q != [255, 255, 255] {
            covered += 1;
            if p.iter().zip(q).any(|(x, y)| x.abs_diff(*y) > TIER_GAIN_PIXEL) {
                changed += 1;
            }
        }
    }
    (changed, covered)
}

/// Both meshes drawn at `px` from each of [`TIER_GAIN_VIEWS`] in one shared
/// frame: (changed, covered, framed extent in m).
pub fn mesh_change(lo: (&crate::mesh::Mesh, &[[u8; 4]]), hi: (&crate::mesh::Mesh, &[[u8; 4]]), px: usize, threads: usize) -> (usize, usize, f64) {
    let mut b = lo.0.bounds();
    for p in &hi.0.pos {
        b.add_point(*p);
    }
    if b.is_empty() {
        return (0, 0, 0.0);
    }
    let extent = len(b.size()).max(1e-6) * 1.1;
    let (mut changed, mut covered) = (0, 0);
    for view in TIER_GAIN_VIEWS {
        let a = picture_framed(lo.0, lo.1, px, view, b.centre(), extent, threads);
        let c = picture_framed(hi.0, hi.1, px, view, b.centre(), extent, threads);
        let (ch, cv) = pixel_change(&a, &c);
        changed += ch;
        covered += cv;
    }
    (changed, covered, extent)
}

/// Triangles per part: pieces by their part name (the id before `/` or a
/// mirror's `.m`), instances by the part that placed them.
pub fn part_triangles(m: &Model) -> std::collections::BTreeMap<String, i64> {
    // `front/s0/p1.m` -> front; `nails_3.m#0` (a placed or baked copy) -> nails.
    let part = |id: &str| -> String {
        let n = id.split(['/', '#']).next().unwrap_or(id).trim_end_matches(".m");
        n.rsplit_once('_').filter(|(_, d)| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit())).map_or(n, |x| x.0).to_string()
    };
    let mut out = std::collections::BTreeMap::new();
    for p in &m.pieces {
        *out.entry(part(&p.id)).or_insert(0) += p.mesh.tris.len() as i64;
    }
    for i in &m.instances {
        let tris: usize = m.proto(&i.proto).map_or(0, |p| p.pieces.iter().map(|x| x.mesh.tris.len()).sum());
        *out.entry(part(&i.name)).or_insert(0) += tris as i64;
    }
    out
}

/// Every adjacent pair of `tiers` (lowest first, each with its file bytes).
pub fn tier_gains(tiers: &[(&Model, usize)], threads: usize) -> Vec<TierGain> {
    tiers
        .windows(2)
        .map(|w| {
            let ((lo, lb), (hi, hb)) = (w[0], w[1]);
            let px = design_px(&hi.tier);
            let (ml, cl) = world_mesh(lo);
            let (mh, ch) = world_mesh(hi);
            let (changed, covered, extent) = mesh_change((&ml, &cl), (&mh, &ch), px, threads);
            let (pl, ph) = (part_triangles(lo), part_triangles(hi));
            let mut by_part: Vec<(String, i64)> = ph.iter().map(|(k, v)| (k.clone(), v - pl.get(k).copied().unwrap_or(0))).filter(|p| p.1 != 0).collect();
            by_part.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            TierGain {
                lower: lo.tier.clone(),
                higher: hi.tier.clone(),
                px,
                changed,
                covered,
                extra_triangles: hi.triangles() as i64 - lo.triangles() as i64,
                extra_bytes: hb as i64 - lb as i64,
                extent_m: extent,
                distance_m: design_distance(extent, px),
                by_part,
            }
        })
        .collect()
}

/// The top two tiers drawn at the higher one's design view in one frame,
/// cropped to the same window (the `crop`-pixel square where they differ
/// most), enlarged `zoom` times and set side by side with captions, as a PNG.
pub fn closeup(lo: (&Model, usize), hi: (&Model, usize), crop: usize, zoom: usize, threads: usize) -> Vec<u8> {
    let (ml, cl) = world_mesh(lo.0);
    let (mh, ch) = world_mesh(hi.0);
    let cap = |m: &Model, bytes: usize| format!("{} {}T {:.1}KB", m.tier.to_uppercase(), m.triangles(), bytes as f64 / 1024.0);
    closeup_mesh((&ml, &cl, &cap(lo.0, lo.1)), (&mh, &ch, &cap(hi.0, hi.1)), design_px(&hi.0.tier), crop, zoom, threads)
}

/// [`closeup`] of any two meshes (geocheck draws converted tier files with
/// it), each with its caption, at `px` across.
pub fn closeup_mesh(lo: (&crate::mesh::Mesh, &[[u8; 4]], &str), hi: (&crate::mesh::Mesh, &[[u8; 4]], &str), px: usize, crop: usize, zoom: usize, threads: usize) -> Vec<u8> {
    let (ml, cl) = (lo.0, lo.1);
    let (mh, ch) = (hi.0, hi.1);
    let mut b = ml.bounds();
    for p in &mh.pos {
        b.add_point(*p);
    }
    let extent = len(b.size()).max(1e-6) * 1.1;
    let a = picture_framed(ml, cl, px, "three_quarter", b.centre(), extent, threads);
    let c = picture_framed(mh, ch, px, "three_quarter", b.centre(), extent, threads);
    let crop = crop.clamp(16, px);
    // Changed-pixel counts summed over a crop window, on a coarse grid.
    let diff: Vec<u32> = a.as_chunks::<3>().0.iter().zip(c.as_chunks::<3>().0).map(|(p, q)| u32::from(p.iter().zip(q).any(|(x, y)| x.abs_diff(*y) > TIER_GAIN_PIXEL))).collect();
    let step = (crop / 8).max(1);
    let (mut best, mut at) = (0u32, ((px - crop) / 2, (px - crop) / 2));
    let mut y0 = 0;
    while y0 + crop <= px {
        let mut x0 = 0;
        while x0 + crop <= px {
            let s: u32 = (y0..y0 + crop).step_by(2).map(|y| (x0..x0 + crop).step_by(2).map(|x| diff[y * px + x]).sum::<u32>()).sum();
            if s > best {
                best = s;
                at = (x0, y0);
            }
            x0 += step;
        }
        y0 += step;
    }
    let side = crop * zoom.max(1);
    let gap = 8;
    let cap_h = 28;
    let (w, h) = (side * 2 + gap, side + cap_h);
    let mut out = vec![255u8; w * h * 3];
    for (k, (pic, label)) in [(&a, lo.2), (&c, hi.2)].into_iter().enumerate() {
        let ox = k * (side + gap);
        for y in 0..side {
            for x in 0..side {
                let (sx, sy) = (at.0 + x / zoom.max(1), at.1 + y / zoom.max(1));
                let si = (sy * px + sx) * 3;
                let di = ((y + cap_h) * w + ox + x) * 3;
                out[di..di + 3].copy_from_slice(&pic[si..si + 3]);
            }
        }
        let mut g = png::Grey::new(side, cap_h, 255);
        crate::preview::text(&mut g, 8, 6, 2, 30, label);
        for y in 0..cap_h {
            for x in 0..side {
                let v = g.get(x, y);
                let i = (y * w + ox + x) * 3;
                out[i..i + 3].copy_from_slice(&[v, v, v]);
            }
        }
    }
    png::write_rgb(w, h, &out)
}

/// What one construct compile made and found.
#[derive(Clone, Debug, Default)]
pub struct Compiled {
    /// The report written as `<id>.evmodel.json` (also the command's reply).
    pub report: Value,
    /// Problems: failed checks, budgets exceeded, non-determinism.
    pub problems: Vec<String>,
}

/// Draw calls of a tier: one per primitive of each drawn node.
fn draw_calls(m: &Model) -> usize {
    let (groups, protos) = m.output();
    let own: usize = groups.iter().map(|g| g.meshes.iter().filter(|o| !o.mesh.tris.is_empty()).count()).sum();
    let inst: usize = m.instances.iter().map(|i| protos.iter().find(|p| p.name == i.proto).map_or(0, |p| p.meshes.len())).sum();
    own + inst
}

/// Build a recipe at every tier it lists (or `only`), run construct's
/// checks (1 and 4, budgets; tier gain as warnings), write `<id>.<tier>.glb` and
/// `<id>.evmodel.json` into `out`, and with `check` build every tier a
/// second time and compare the bytes. `sheet` also writes `<id>.tiers.png`
/// and `<id>.closeup.png` (the top two tiers where they differ most).
pub fn compile(recipe_text: &str, out: &std::path::Path, only: Option<&[String]>, overrides: &[(String, f64)], check: bool, sheet: bool, threads: usize) -> Result<Compiled, String> {
    let t0 = std::time::Instant::now();
    let recipe = evani_construct::recipe::Recipe::parse(recipe_text)?;
    let specs: Vec<&evani_construct::recipe::TierSpec> = recipe.tiers.iter().filter(|t| only.is_none_or(|o| o.iter().any(|x| x == &t.name))).collect();
    if specs.is_empty() {
        return Err(format!("recipe {}: none of the asked tiers are in it (it has {})", recipe.id, recipe.tiers.iter().map(|t| t.name.as_str()).collect::<Vec<_>>().join(", ")));
    }
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let mut problems = Vec::new();
    let mut tiers = Map::new();
    let mut built: Vec<(Model, usize)> = Vec::new();
    for spec in &specs {
        let tb = std::time::Instant::now();
        let m = recipe.build(&spec.name, overrides)?;
        let ms_build = tb.elapsed().as_secs_f64() * 1000.0;
        let mut checks = Vec::new();
        for i in evani_construct::check::pieces(&m) {
            checks.push(format!("check 1: {}: {}", i.piece, i.what));
        }
        if let Some((groups, _, _, names)) = recipe.split(&spec.name)? {
            let whole = evani_construct::recipe::Recipe::unsplit(&m, &groups, &names);
            let g: Vec<&str> = groups.iter().map(String::as_str).collect();
            for p in evani_construct::model::check_split(&whole, &m, &g, (&names.0, &names.1)) {
                checks.push(format!("check 4: {p}"));
            }
        }
        let tris = m.triangles();
        if spec.budget > 0 && tris > spec.budget {
            checks.push(format!("budget: {tris} triangles over the {} budget", spec.budget));
        }
        let tw = std::time::Instant::now();
        let bytes = write(&m, spec.budget, &Map::new());
        let ms_write = tw.elapsed().as_secs_f64() * 1000.0;
        if check {
            let again = write(&recipe.build(&spec.name, overrides)?, spec.budget, &Map::new());
            if again != bytes {
                checks.push("determinism: a second build wrote different bytes".to_string());
            }
        }
        let file = format!("{}.{}.glb", recipe.id, spec.name);
        std::fs::write(out.join(&file), &bytes).map_err(|e| format!("{file}: {e}"))?;
        for c in &checks {
            problems.push(format!("{}: {c}", spec.name));
        }
        tiers.insert(
            spec.name.clone(),
            json!({
                "file": file, "bytes": bytes.len(), "sha256": crate::sha256::hex(&crate::sha256::sha256(&bytes)),
                "triangles": tris, "budget": spec.budget, "draw_calls": draw_calls(&m),
                "pieces": m.pieces.len(), "instances": m.instances.len(), "anchors": m.anchors.len(),
                "dropped": m.dropped.iter().map(|d| json!({ "part": d.0, "why": d.1 })).collect::<Vec<_>>(),
                "build_ms": r6(ms_build), "write_ms": r6(ms_write),
                "checks": checks, "ok": checks.is_empty()
            }),
        );
        built.push((m, bytes.len()));
    }
    let refs: Vec<(&Model, usize)> = built.iter().map(|(m, b)| (m, *b)).collect();
    // Tier gain: each costlier tier must show at its own design view.
    let gains = tier_gains(&refs, threads);
    let warnings: Vec<String> = gains.iter().filter(|g| g.wasted()).map(TierGain::warning).collect();
    if sheet {
        let png = tier_sheet(&refs, 360, threads);
        std::fs::write(out.join(format!("{}.tiers.png", recipe.id)), png).map_err(|e| format!("tier sheet: {e}"))?;
        if let [.., lo, hi] = refs.as_slice() {
            let png = closeup(*lo, *hi, 256, 2, threads);
            std::fs::write(out.join(format!("{}.closeup.png", recipe.id)), png).map_err(|e| format!("close-up: {e}"))?;
        }
    }
    let params: Map<String, Value> = recipe.values(overrides)?.into_iter().map(|(k, v)| (k, json!(r6(v)))).collect();
    let report = json!({
        "construct": 1, "id": recipe.id, "tool": format!("Evani.Model construct {}", env!("CARGO_PKG_VERSION")),
        "recipe_hash": recipe.hash, "seed": recipe.seed, "params": params,
        "tiers": tiers, "checked_twice": check, "ok": problems.is_empty(), "problems": problems,
        "tier_gain": gains.iter().map(TierGain::to_json).collect::<Vec<_>>(), "warnings": warnings,
        "ms": r6(t0.elapsed().as_secs_f64() * 1000.0)
    });
    let text = serde_json::to_string_pretty(&report).unwrap_or_default();
    std::fs::write(out.join(format!("{}.evmodel.json", recipe.id)), text + "\n").map_err(|e| format!("report: {e}"))?;
    Ok(Compiled { report, problems })
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_construct::recipe::Recipe;

    const R: &str = r##"{
      "construct": 1, "id": "t-box", "seed": 5,
      "params": { "w": 0.8 },
      "tiers": { "low": { "budget": 300 }, "high": { "budget": 3000 } },
      "materials": { "wood": { "colour": "#8a6a48" }, "iron": { "colour": "#3a3a3c" } },
      "curves": {
        "top": { "kind": "polyline", "points": [["-w / 2", 0.5, 0], ["w / 2", 0.5, 0]] },
        "bottom": { "kind": "polyline", "points": [["-w / 2", 0, 0], ["w / 2", 0, 0]] },
        "rail": { "kind": "polyline", "points": [[0.05, 0.25, 0.03], ["w / 2", 0.25, 0.03]] }
      },
      "surfaces": { "front": { "kind": "ruled", "a": "top", "b": "bottom" } },
      "protos": { "stud": { "parts": [ { "op": "lathe", "profile": [[0, 0], [0.012, 0], [0, 0.008]], "segments": 8, "material": "iron" } ] } },
      "parts": [
        { "name": "front", "node": "body", "op": "boards", "on": "front", "count": 3, "piece": { "thickness": 0.02, "gap": 0.004 }, "material": "wood", "vary": { "tone": 0.1 } },
        { "name": "studs", "node": "body", "op": "along", "curve": "rail", "count": 3, "inset": 0.05, "proto": "stud", "anchor": "stud_{i}", "mirror": true }
      ],
      "anchors": { "lid": { "at": [0, 0.5, 0], "q": [0, 0.7071, 0, 0.7071] } }
    }"##;

    #[test]
    fn construct_tiers_write_read_back_and_are_byte_identical() {
        let r = Recipe::parse(R).unwrap();
        let m = r.build("high", &[]).unwrap();
        let a = write(&m, 3000, &Map::new());
        assert_eq!(a, write(&r.build("high", &[]).unwrap(), 3000, &Map::new()));
        assert_eq!(a.len() % 4, 0);
        let rb = crate::gltf::read(&a).unwrap();
        assert_eq!(rb.json["asset"]["generator"].as_str().unwrap().split(' ').next(), Some("Evani.Model"));
        assert_eq!(rb.json["extras"]["evani"]["tier"], "high");
        assert_eq!(rb.json["extras"]["evani"]["construct"]["recipe"], r.hash.as_str());
        assert_eq!(rb.json["materials"].as_array().unwrap().len(), 2);
        // One solid for the boards, three studs and their three mirror twins.
        assert_eq!(rb.instances.len(), 7, "{:?}", rb.instances.iter().map(|i| &i.0).collect::<Vec<_>>());
        assert_eq!(rb.json["meshes"].as_array().unwrap().len(), 2);
        // Every placed stud reads back closed and outward (construct's check 1,
        // which welds by position: pieces keep hard edges as split vertices).
        for (name, p, n, t) in rb.instances.iter().filter(|i| i.0.starts_with("studs_")) {
            let m = evani_construct::mesh::Mesh { pos: p.clone(), nrm: n.clone(), uv: vec![[0.0; 2]; p.len()], tris: t.clone() };
            let bad: Vec<String> = evani_construct::check::piece(&m).into_iter().filter(|s| !s.contains("normals")).collect();
            assert!(bad.is_empty(), "{name}: {bad:?}");
        }
        assert_eq!(rb.anchors.iter().filter(|a| a.0.starts_with("stud_")).count(), 6);
        let stud = rb.anchors.iter().find(|a| a.0 == "stud_0").unwrap();
        assert!((stud.1[0] - 0.10).abs() < 1e-6 && (stud.1[2] - 0.03).abs() < 1e-6, "{:?}", stud.1);
        assert!(rb.json["extras"]["evani"]["sockets"]["lid"]["q"].is_array());
        // A picture and a sheet come out as PNGs.
        let sheet = tier_sheet(&[(&r.build("low", &[]).unwrap(), 1000), (&m, a.len())], 96, 2);
        assert_eq!(&sheet[1..4], b"PNG");
    }
    /// A box with studs: `studs` the stud's sides per tier, `boards` the
    /// front's board count per tier, `extra` more parts (JSON) for high.
    fn gain_box(studs: &str, boards: &str, extra: &str) -> Vec<TierGain> {
        let text = format!(
            r##"{{ "construct": 1, "id": "t-gain", "seed": 3,
              "tiers": {{ "low": {{}}, "high": {{}} }},
              "materials": {{ "wood": {{ "colour": "#8a6a48" }}, "iron": {{ "colour": "#3a3a3c" }} }},
              "curves": {{
                "top": {{ "kind": "polyline", "points": [[-0.4, 0.5, 0], [0.4, 0.5, 0]] }},
                "bottom": {{ "kind": "polyline", "points": [[-0.4, 0, 0], [0.4, 0, 0]] }},
                "rail": {{ "kind": "polyline", "points": [[0.05, 0.25, 0.012], [0.4, 0.25, 0.012]] }}
              }},
              "surfaces": {{ "front": {{ "kind": "ruled", "a": "top", "b": "bottom" }} }},
              "protos": {{ "stud": {{ "parts": [ {{ "op": "lathe", "profile": [[0, 0], [0.02, 0], [0.012, 0.012], [0, 0.016]], "segments": {studs}, "material": "iron" }} ] }} }},
              "parts": [
                {{ "name": "front", "op": "boards", "on": "front", "count": {boards}, "piece": {{ "thickness": 0.02, "gap": 0.006 }}, "material": "wood" }},
                {{ "name": "studs", "op": "along", "curve": "rail", "count": 3, "inset": 0.05, "proto": "stud", "mirror": true }}{extra}
              ] }}"##
        );
        let r = Recipe::parse(&text).unwrap();
        let (lo, hi) = (r.build("low", &[]).unwrap(), r.build("high", &[]).unwrap());
        let (bl, bh) = (write(&lo, 0, &Map::new()).len(), write(&hi, 0, &Map::new()).len());
        tier_gains(&[(&lo, bl), (&hi, bh)], 2)
    }

    #[test]
    fn tier_gain_warns_on_detail_that_does_not_show_and_passes_real_detail() {
        // High only rounds its studs (10 to 12 sides): it costs triangles
        // and bytes, and the check names the tier, the cost and the part.
        let g = gain_box(r#"{ "low": 10, "high": 12 }"#, "3", "");
        assert_eq!(g.len(), 1);
        let w = &g[0];
        assert!(w.extra_triangles > 0 && w.extra_bytes > 0, "{w:?}");
        assert!(w.wasted(), "share {:.4}", w.share());
        assert_eq!(w.by_part.first().map(|p| p.0.as_str()), Some("studs"));
        let text = w.warning();
        assert!(text.starts_with("WARN tier gain: high costs") && text.contains("than low") && text.contains("fix: make the extra detail visible at"), "{text}");
        assert!(text.contains("drop the high tier so it falls back to low"), "{text}");
        // High splits the front into boards and adds a brass plate: it shows.
        let g = gain_box("10", r#"{ "low": 1, "high": 4 }"#, r#", { "name": "plate", "op": "block", "at": [0, 0.3, 0.006], "half": [0.08, 0.05, 0.006], "material": "iron", "tiers": ["high"] }"#);
        assert!(!g[0].wasted(), "share {:.4}", g[0].share());
        assert!(g[0].share() > TIER_GAIN_MIN, "{:.4}", g[0].share());
        assert!(g[0].to_json()["ok"].as_bool().unwrap());
        // Identical tiers cost nothing and are never called wasted.
        let g = gain_box("10", "3", "");
        assert_eq!((g[0].extra_triangles, g[0].changed), (0, 0));
        assert!(!g[0].wasted());
    }

    #[test]
    fn a_100k_triangle_model_builds_and_writes_in_under_a_second() {
        // docs/MODELGEN_PLAN.md MG1 budget: 100 strips of 50 rows by 4 columns.
        let big = r##"{ "construct": 1, "id": "t-big", "seed": 1,
          "materials": { "wood": { "colour": "#8a6a48" } },
          "surfaces": { "s": { "kind": "formula", "x": "u * 8", "y": "v * 4", "z": "sin(u * 180) * 0.6 + sin(v * 360) * 0.2" } },
          "parts": [ { "name": "boards", "op": "boards", "on": "s", "count": 100, "rows": 50, "cols": 4,
                       "piece": { "thickness": 0.01, "gap": 0.002 }, "material": "wood", "vary": { "tone": 0.1 } } ] }"##;
        let r = Recipe::parse(big).unwrap();
        let t0 = std::time::Instant::now();
        let m = r.build("high", &[]).unwrap();
        let built = t0.elapsed().as_secs_f64();
        let bytes = write(&m, 0, &Map::new());
        let total = t0.elapsed().as_secs_f64();
        assert!(m.triangles() >= 100_000, "{}", m.triangles());
        eprintln!("100k model: {} triangles, build {built:.3} s, build and write {total:.3} s, {} bytes", m.triangles(), bytes.len());
        assert!(total < 1.0, "build and write took {total:.3} s");
    }
}
