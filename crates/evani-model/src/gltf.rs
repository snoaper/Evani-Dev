//! glTF 2.0 output, written by us (docs/model_research/17): one `.glb` per
//! render tier with KHR_mesh_quantization, the part tree, gameplay anchors as
//! empty nodes, collision proxies under a `collision` node and the Evani
//! metadata in the root `extras.evani`. Bytes are deterministic: JSON keys
//! sorted, numbers rounded to fixed precision, buffers in solid order.
//!
//! Quantisation: POSITION as unsigned 16-bit integers (not normalised) over
//! the mesh's box with one uniform step, undone by the instance node's scale
//! and translation; NORMAL as normalised 8-bit (low, medium) or 16-bit (high,
//! ultra); TEXCOORD_0 as normalised unsigned 16-bit and TANGENT as normalised
//! 8-bit when charts exist; indices 16-bit under 65,536 vertices.
//! A reader for our own files backs the round-trip tests.

use crate::collide::Collision;
use crate::compile::TierMesh;
use crate::generator::Proxy;
use crate::vec::*;
use serde_json::{Map, Value, json};

/// Per-solid surface data (charts and tangents), when built.
#[derive(Clone, Debug, Default)]
pub struct Surface {
    pub uv: Vec<[f64; 2]>,
    pub tangent: Vec<[f64; 4]>,
}

/// Round to 6 decimals (micrometres for lengths): stable, compact JSON.
pub fn r6(x: f64) -> f64 {
    let v = (x * 1e6).round() / 1e6;
    if v == 0.0 { 0.0 } else { v }
}

fn q4(q: Quat) -> Value {
    json!([r6(q[0]), r6(q[1]), r6(q[2]), r6(q[3])])
}

/// A socket's anchor node carries its rotation (glTF `rotation`, and `q` in
/// the node's `extras.evani`); an anchor without one is left untouched.
fn socket_rotation(node: &mut Value, q: Option<Quat>) {
    if let Some(q) = q {
        node["rotation"] = q4(q);
        node["extras"]["evani"]["q"] = q4(q);
    }
}

/// `extras.evani.sockets.<name>`: position and rotation in the model frame.
fn socket_json(a: &crate::build::GameAnchor, q: Quat) -> Value {
    let mut j = json!({ "p": v3(a.p), "q": q4(q) });
    if !a.part.is_empty() {
        j["part"] = json!(a.part);
    }
    if let Some(jn) = &a.joint {
        j["joint"] = json!(jn);
    }
    j
}

fn v3(a: V3) -> Value {
    json!([r6(a[0]), r6(a[1]), r6(a[2])])
}

struct Bin {
    data: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}

impl Bin {
    fn align(&mut self) {
        while !self.data.len().is_multiple_of(4) {
            self.data.push(0);
        }
    }
    fn view(&mut self, bytes: &[u8], stride: Option<usize>, target: u32) -> usize {
        self.align();
        let off = self.data.len();
        self.data.extend_from_slice(bytes);
        let mut v = Map::new();
        v.insert("buffer".into(), json!(0));
        v.insert("byteOffset".into(), json!(off));
        v.insert("byteLength".into(), json!(bytes.len()));
        if let Some(s) = stride {
            v.insert("byteStride".into(), json!(s));
        }
        v.insert("target".into(), json!(target));
        self.views.push(Value::Object(v));
        self.views.len() - 1
    }
    /// A view for non-vertex data (inverse bind matrices): no target.
    fn view_untargeted(&mut self, bytes: &[u8]) -> usize {
        self.align();
        let off = self.data.len();
        self.data.extend_from_slice(bytes);
        self.views.push(json!({ "buffer": 0, "byteOffset": off, "byteLength": bytes.len() }));
        self.views.len() - 1
    }
    fn accessor(&mut self, view: usize, ctype: u32, normalized: bool, count: usize, ty: &str, min: Option<Value>, max: Option<Value>) -> usize {
        let mut a = Map::new();
        a.insert("bufferView".into(), json!(view));
        a.insert("componentType".into(), json!(ctype));
        if normalized {
            a.insert("normalized".into(), json!(true));
        }
        a.insert("count".into(), json!(count));
        a.insert("type".into(), json!(ty));
        if let Some(m) = min {
            a.insert("min".into(), m);
        }
        if let Some(m) = max {
            a.insert("max".into(), m);
        }
        self.accessors.push(Value::Object(a));
        self.accessors.len() - 1
    }
}

const ARRAY: u32 = 34962;
const ELEMENT: u32 = 34963;

fn proxy_json(p: &Proxy) -> (Value, V3, Quat) {
    match p {
        Proxy::Box { c, half, q } => (json!({ "shape": "box", "half": v3(*half) }), *c, *q),
        Proxy::Cylinder { c, q, r, half_h } => (json!({ "shape": "cylinder", "radius": r6(*r), "half_height": r6(*half_h), "axis": "y" }), *c, *q),
        Proxy::Capsule { a, b, r } => (json!({ "shape": "capsule", "a": v3(*a), "b": v3(*b), "radius": r6(*r) }), [0.0; 3], QID),
        Proxy::Sphere { c, r } => (json!({ "shape": "sphere", "radius": r6(*r) }), *c, QID),
        Proxy::Hull { pts } => (json!({ "shape": "hull", "points": pts.iter().map(|p| v3(*p)).collect::<Vec<_>>() }), [0.0; 3], QID),
    }
}

/// One sRGB byte to a linear byte (our own maths, deterministic).
pub fn srgb_to_linear_u8(c: u8) -> u8 {
    let c = c as f64 / 255.0;
    let l = if c <= 0.04045 { c / 12.92 } else { crate::dmath::pow((c + 0.055) / 1.055, 2.4) };
    (l * 255.0).round().clamp(0.0, 255.0) as u8
}

/// Per-vertex linear RGBA from the part regions: a vertex takes the region
/// of the lowest-index triangle using it. `None` for models without regions.
pub fn vertex_colours(m: &crate::mesh::Mesh, model: &crate::build::Model) -> Option<Vec<[u8; 4]>> {
    if model.region_colours.is_empty() {
        return None;
    }
    let lin: Vec<Option<[u8; 4]>> = model
        .parts
        .iter()
        .map(|p| p.region.as_ref().and_then(|r| model.region_colours.get(r)).map(|c| [srgb_to_linear_u8(c[0]), srgb_to_linear_u8(c[1]), srgb_to_linear_u8(c[2]), 255]))
        .collect();
    let mut out: Vec<Option<[u8; 4]>> = vec![None; m.pos.len()];
    for (ti, t) in m.tris.iter().enumerate() {
        let c = m.tri_part.get(ti).and_then(|&p| lin.get(p as usize).copied().flatten());
        for &v in t {
            if let Some(slot) = out.get_mut(v as usize)
                && slot.is_none()
            {
                *slot = c.or(Some([255; 4]));
            }
        }
    }
    Some(out.into_iter().map(|c| c.unwrap_or([255; 4])).collect())
}

/// A skinned solid: each instance baked into its own mesh in world space
/// (glTF ignores a skinned node's transform, so no quantisation by node
/// scale): FLOAT positions, JOINTS_0 (u8) and WEIGHTS_0 (float), and the
/// node in the scene list with no transform. A mirrored instance flips x,
/// the winding and the tangent handedness.
fn write_skinned_solid(
    bin: &mut Bin,
    meshes: &mut Vec<Value>,
    nodes: &mut Vec<Value>,
    scene_nodes: &mut Vec<usize>,
    tm: &TierMesh,
    s: &crate::compile::SolidMesh,
    sf: Option<&Surface>,
    sk: &crate::rig::Skeleton,
    hi_normals: bool,
) {
    let m = &s.mesh;
    let sf = sf.filter(|sf| sf.uv.len() == m.pos.len() && sf.tangent.len() == m.pos.len());
    for inst in &s.instances {
        let sx = if inst.mirror { -1.0 } else { 1.0 };
        let pos: Vec<V3> = m.pos.iter().map(|p| add([p[0] * sx, p[1], p[2]], inst.t)).collect();
        let tris: Vec<[u32; 3]> = m.tris.iter().map(|t| if inst.mirror { [t[0], t[2], t[1]] } else { *t }).collect();
        let mut attrs = Map::new();
        let mut pb = Vec::with_capacity(pos.len() * 12);
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for p in &pos {
            for k in 0..3 {
                let f = p[k] as f32;
                lo[k] = lo[k].min(f);
                hi[k] = hi[k].max(f);
                pb.extend_from_slice(&f.to_le_bytes());
            }
        }
        let pv = bin.view(&pb, Some(12), ARRAY);
        let mm = |a: [f32; 3]| json!([a[0] as f64, a[1] as f64, a[2] as f64]);
        attrs.insert("POSITION".into(), json!(bin.accessor(pv, 5126, false, pos.len(), "VEC3", Some(mm(lo)), Some(mm(hi)))));
        let mut nb = Vec::new();
        if hi_normals {
            for n in &m.nrm {
                for k in 0..3 {
                    let c = if k == 0 { n[k] * sx } else { n[k] };
                    nb.extend_from_slice(&((c * 32767.0).round().clamp(-32767.0, 32767.0) as i16).to_le_bytes());
                }
                nb.extend_from_slice(&[0, 0]);
            }
            let v = bin.view(&nb, Some(8), ARRAY);
            attrs.insert("NORMAL".into(), json!(bin.accessor(v, 5122, true, m.nrm.len(), "VEC3", None, None)));
        } else {
            for n in &m.nrm {
                for k in 0..3 {
                    let c = if k == 0 { n[k] * sx } else { n[k] };
                    nb.push(((c * 127.0).round().clamp(-127.0, 127.0) as i8) as u8);
                }
                nb.push(0);
            }
            let v = bin.view(&nb, Some(4), ARRAY);
            attrs.insert("NORMAL".into(), json!(bin.accessor(v, 5120, true, m.nrm.len(), "VEC3", None, None)));
        }
        if let Some(sf) = sf {
            let mut ub = Vec::new();
            for uv in &sf.uv {
                for k in 0..2 {
                    ub.extend_from_slice(&((uv[k].clamp(0.0, 1.0) * 65535.0).round() as u16).to_le_bytes());
                }
            }
            let uvv = bin.view(&ub, Some(4), ARRAY);
            attrs.insert("TEXCOORD_0".into(), json!(bin.accessor(uvv, 5123, true, sf.uv.len(), "VEC2", None, None)));
            let mut tb = Vec::new();
            for t in &sf.tangent {
                tb.push(((t[0] * sx * 127.0).round().clamp(-127.0, 127.0) as i8) as u8);
                tb.push(((t[1] * 127.0).round().clamp(-127.0, 127.0) as i8) as u8);
                tb.push(((t[2] * 127.0).round().clamp(-127.0, 127.0) as i8) as u8);
                tb.push((if t[3] * sx < 0.0 { -127i8 } else { 127i8 }) as u8);
            }
            let tv = bin.view(&tb, Some(4), ARRAY);
            attrs.insert("TANGENT".into(), json!(bin.accessor(tv, 5120, true, sf.tangent.len(), "VEC4", None, None)));
        }
        if let Some(cols) = vertex_colours(m, &tm.model) {
            let cb: Vec<u8> = cols.iter().flatten().copied().collect();
            let cv = bin.view(&cb, Some(4), ARRAY);
            attrs.insert("COLOR_0".into(), json!(bin.accessor(cv, 5121, true, cols.len(), "VEC4", None, None)));
        }
        let inf = crate::rig::weights(&tm.model, sk, &pos, &m.tris, &m.tri_part);
        let jb: Vec<u8> = inf.iter().flat_map(|i| i.0).collect();
        let jv = bin.view(&jb, Some(4), ARRAY);
        attrs.insert("JOINTS_0".into(), json!(bin.accessor(jv, 5121, false, inf.len(), "VEC4", None, None)));
        let mut wb = Vec::with_capacity(inf.len() * 16);
        for i in &inf {
            for w in i.1 {
                wb.extend_from_slice(&w.to_le_bytes());
            }
        }
        let wv = bin.view(&wb, Some(16), ARRAY);
        attrs.insert("WEIGHTS_0".into(), json!(bin.accessor(wv, 5126, false, inf.len(), "VEC4", None, None)));
        let mut ib = Vec::new();
        let ctype = if pos.len() < 65536 {
            for t in &tris {
                for &v in t {
                    ib.extend_from_slice(&(v as u16).to_le_bytes());
                }
            }
            5123
        } else {
            for t in &tris {
                for &v in t {
                    ib.extend_from_slice(&v.to_le_bytes());
                }
            }
            5125
        };
        let iv = bin.view(&ib, None, ELEMENT);
        let ia = bin.accessor(iv, ctype, false, tris.len() * 3, "SCALAR", None, None);
        meshes.push(json!({ "name": inst.solid, "primitives": [{ "attributes": Value::Object(attrs), "indices": ia, "mode": 4 }] }));
        nodes.push(json!({
            "name": inst.solid,
            "mesh": meshes.len() - 1,
            "skin": 0,
            "extras": { "evani": { "kind": "solid", "origin": v3(inst.t), "mirrored": inst.mirror } }
        }));
        scene_nodes.push(nodes.len() - 1);
    }
}

/// Write one tier as `.glb` bytes. `meta` is merged into `extras.evani`.
pub fn write_tier(tm: &TierMesh, col: &Collision, surfaces: Option<&[Surface]>, meta: &Map<String, Value>) -> Vec<u8> {
    let mut bin = Bin { data: Vec::new(), views: Vec::new(), accessors: Vec::new() };
    let hi_normals = matches!(tm.tier.name.as_str(), "high" | "ultra" | "vr");
    let mut meshes = Vec::new();
    let mut nodes: Vec<Value> = vec![Value::Null];
    let mut root_children = Vec::new();
    // Skinned meshes sit in the scene list beside node 0.
    let mut scene_nodes: Vec<usize> = vec![0];
    for (si, s) in tm.solids.iter().enumerate() {
        if let Some(sk) = &tm.model.skeleton {
            let sf = surfaces.and_then(|x| x.get(si));
            write_skinned_solid(&mut bin, &mut meshes, &mut nodes, &mut scene_nodes, tm, s, sf, sk, hi_normals);
            continue;
        }
        let m = &s.mesh;
        let b = m.bounds();
        let size = b.size();
        let step = (size[0].max(size[1]).max(size[2]) / 65535.0).max(1e-9);
        // Positions: u16 x3 padded to 8 bytes.
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
        let pa = bin.accessor(pv, 5123, false, m.pos.len(), "VEC3", Some(json!(qmin)), Some(json!(qmax)));
        // Normals.
        let mut nb = Vec::new();
        let (na, _) = if hi_normals {
            for n in &m.nrm {
                for k in 0..3 {
                    nb.extend_from_slice(&((n[k] * 32767.0).round().clamp(-32767.0, 32767.0) as i16).to_le_bytes());
                }
                nb.extend_from_slice(&[0, 0]);
            }
            let v = bin.view(&nb, Some(8), ARRAY);
            (bin.accessor(v, 5122, true, m.nrm.len(), "VEC3", None, None), 0)
        } else {
            for n in &m.nrm {
                for k in 0..3 {
                    nb.push(((n[k] * 127.0).round().clamp(-127.0, 127.0) as i8) as u8);
                }
                nb.push(0);
            }
            let v = bin.view(&nb, Some(4), ARRAY);
            (bin.accessor(v, 5120, true, m.nrm.len(), "VEC3", None, None), 0)
        };
        let mut attrs = Map::new();
        attrs.insert("POSITION".into(), json!(pa));
        attrs.insert("NORMAL".into(), json!(na));
        if let Some(sf) = surfaces.and_then(|s| s.get(si))
            && sf.uv.len() == m.pos.len()
            && sf.tangent.len() == m.pos.len()
        {
            let mut ub = Vec::new();
            for uv in &sf.uv {
                for k in 0..2 {
                    ub.extend_from_slice(&((uv[k].clamp(0.0, 1.0) * 65535.0).round() as u16).to_le_bytes());
                }
            }
            let uvv = bin.view(&ub, Some(4), ARRAY);
            attrs.insert("TEXCOORD_0".into(), json!(bin.accessor(uvv, 5123, true, sf.uv.len(), "VEC2", None, None)));
            let mut tb = Vec::new();
            for t in &sf.tangent {
                for k in 0..3 {
                    tb.push(((t[k] * 127.0).round().clamp(-127.0, 127.0) as i8) as u8);
                }
                tb.push((if t[3] < 0.0 { -127i8 } else { 127i8 }) as u8);
            }
            let tv = bin.view(&tb, Some(4), ARRAY);
            attrs.insert("TANGENT".into(), json!(bin.accessor(tv, 5120, true, sf.tangent.len(), "VEC4", None, None)));
        }
        // Region colours (linear, from the sRGB region colours).
        if let Some(cols) = vertex_colours(m, &tm.model) {
            let cb: Vec<u8> = cols.iter().flatten().copied().collect();
            let cv = bin.view(&cb, Some(4), ARRAY);
            attrs.insert("COLOR_0".into(), json!(bin.accessor(cv, 5121, true, cols.len(), "VEC4", None, None)));
        }
        // Indices.
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
        let ia = bin.accessor(iv, ctype, false, m.tris.len() * 3, "SCALAR", None, None);
        meshes.push(json!({ "name": s.solid, "primitives": [{ "attributes": Value::Object(attrs), "indices": ia, "mode": 4 }] }));
        for inst in &s.instances {
            let sx = if inst.mirror { -1.0 } else { 1.0 };
            let o = [b.min[0] * sx, b.min[1], b.min[2]];
            let node = json!({
                "name": inst.solid,
                "mesh": si,
                "translation": v3(add(inst.t, o)),
                "scale": [step * sx, step, step],
                "extras": { "evani": { "kind": "solid", "origin": v3(inst.t), "mirrored": inst.mirror } }
            });
            nodes.push(node);
            root_children.push(nodes.len() - 1);
        }
    }
    // Skeleton: joint nodes (translation-only rest pose) under node 0, with
    // the anchors that ride on joints as their children.
    let mut skin = None;
    if let Some(sk) = &tm.model.skeleton {
        let base = nodes.len();
        let jn = sk.joints.len();
        let on_joint: Vec<(usize, &crate::build::GameAnchor)> = tm.model.anchors.iter().filter_map(|a| a.joint.as_ref().and_then(|j| sk.index(j)).map(|j| (j, a))).collect();
        for (i, j) in sk.joints.iter().enumerate() {
            let parent_at = j.parent.map(|p| sk.joints[p].at).unwrap_or([0.0; 3]);
            let mut children: Vec<usize> = sk.children(i).into_iter().map(|c| base + c).collect();
            children.extend(on_joint.iter().enumerate().filter(|(_, (ji, _))| *ji == i).map(|(k, _)| base + jn + k));
            let mut node = json!({ "name": j.name, "translation": v3(sub(j.at, parent_at)), "extras": { "evani": { "kind": "joint", "rest": v3(j.at), "radius": r6(j.radius) } } });
            if !children.is_empty() {
                node["children"] = json!(children);
            }
            nodes.push(node);
            if j.parent.is_none() {
                root_children.push(base + i);
            }
        }
        for (ji, a) in &on_joint {
            let mut node = json!({
                "name": format!("anchor:{}", a.name),
                "translation": v3(sub(a.p, sk.joints[*ji].at)),
                "extras": { "evani": { "kind": "anchor", "normal": v3(a.n), "part": a.part, "joint": sk.joints[*ji].name, "rest": v3(a.p) } }
            });
            // Joints rest unrotated, so the model-frame q is also the local one.
            socket_rotation(&mut node, a.q);
            nodes.push(node);
        }
        // Inverse bind matrices: translation by minus the joint's rest position.
        let mut ib = Vec::with_capacity(jn * 64);
        for j in &sk.joints {
            let m: [f32; 16] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -j.at[0] as f32, -j.at[1] as f32, -j.at[2] as f32, 1.0];
            for x in m {
                ib.extend_from_slice(&x.to_le_bytes());
            }
        }
        let iv = bin.view_untargeted(&ib);
        let ia = bin.accessor(iv, 5126, false, jn, "MAT4", None, None);
        skin = Some(json!([{ "name": "skeleton", "inverseBindMatrices": ia, "joints": (base..base + jn).collect::<Vec<_>>(), "skeleton": base }]));
    }
    // Anchors.
    for a in &tm.model.anchors {
        if tm.model.skeleton.is_some() && a.joint.is_some() {
            continue;
        }
        let mut node = json!({ "name": format!("anchor:{}", a.name), "translation": v3(a.p), "extras": { "evani": { "kind": "anchor", "normal": v3(a.n), "part": a.part } } });
        socket_rotation(&mut node, a.q);
        nodes.push(node);
        root_children.push(nodes.len() - 1);
    }
    // Collision.
    let mut col_children = Vec::new();
    for p in &col.proxies {
        let (shape, c, q) = proxy_json(&p.proxy);
        nodes.push(json!({
            "name": format!("col:{}", p.name),
            "translation": v3(c),
            "rotation": [r6(q[0]), r6(q[1]), r6(q[2]), r6(q[3])],
            "extras": { "evani": { "kind": "collision", "solid": p.solid, "proxy": shape, "missed": r6(p.missed), "excess": r6(p.excess) } }
        }));
        col_children.push(nodes.len() - 1);
    }
    let mut col_node = json!({ "name": "collision", "children": col_children });
    if let Some(f) = &col.footprint {
        col_node["extras"] = json!({ "evani": { "kind": "footprint", "shape": "capsule", "centre_xz": [r6(f.centre[0]), r6(f.centre[1])], "radius": r6(f.radius), "half_segment": r6(f.half_segment) } });
    }
    nodes.push(col_node);
    root_children.push(nodes.len() - 1);
    // Root node and metadata.
    let parts: Vec<Value> = tm
        .model
        .parts
        .iter()
        .map(|p| {
            let mut j = json!({ "name": p.name, "role": p.role, "solid": p.solid, "group": p.group, "generator": p.kind, "dropped": tm.dropped.iter().any(|d| d.0 == p.name) });
            if let Some(r) = &p.region {
                j["region"] = json!(r);
            }
            j
        })
        .collect();
    let mut evani = Map::new();
    evani.insert("format".into(), json!(1));
    evani.insert("class".into(), json!(tm.model.class));
    evani.insert("style".into(), json!(tm.model.style));
    evani.insert("tier".into(), json!(tm.tier.name));
    evani.insert("seed".into(), json!(tm.model.seed));
    evani.insert("intent".into(), json!(crate::intent::content_hash(&tm.model.intent)));
    evani.insert("parts".into(), Value::Array(parts));
    evani.insert("dropped".into(), Value::Array(tm.dropped.iter().map(|d| json!({ "part": d.0, "why": d.1 })).collect()));
    evani.insert("triangles".into(), json!(tm.tris));
    evani.insert("budget".into(), json!(tm.tier.budget));
    evani.insert("cell_m".into(), json!(r6(tm.h)));
    if !tm.model.region_colours.is_empty() {
        let regs: Map<String, Value> = tm.model.region_colours.iter().map(|(k, c)| (k.clone(), json!(format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])))).collect();
        evani.insert("regions".into(), Value::Object(regs));
    }
    // Sockets (anchors with a rotation): `{p, q}` in the model frame, so a
    // tool that only reads the metadata places an attached model without
    // walking the node tree. Absent when no anchor has `q` (files as before).
    let sockets: Map<String, Value> = tm.model.anchors.iter().filter_map(|a| a.q.map(|q| (a.name.clone(), socket_json(a, q)))).collect();
    if !sockets.is_empty() {
        evani.insert("sockets".into(), Value::Object(sockets));
    }
    for (k, v) in meta {
        evani.insert(k.clone(), v.clone());
    }
    nodes[0] = json!({ "name": tm.model.class, "children": root_children });
    let mut doc = Map::new();
    doc.insert("asset".into(), json!({ "version": "2.0", "generator": format!("Evani.Model {} (M1)", env!("CARGO_PKG_VERSION")) }));
    doc.insert("extensionsUsed".into(), json!(["KHR_mesh_quantization"]));
    doc.insert("extensionsRequired".into(), json!(["KHR_mesh_quantization"]));
    doc.insert("scene".into(), json!(0));
    doc.insert("scenes".into(), json!([{ "nodes": scene_nodes }]));
    doc.insert("nodes".into(), Value::Array(nodes));
    doc.insert("meshes".into(), Value::Array(meshes));
    if let Some(s) = skin {
        doc.insert("skins".into(), s);
    }
    bin.align();
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

/// What our reader recovers from a `.glb`.
#[derive(Clone, Debug, Default)]
pub struct ReadBack {
    pub json: Value,
    /// Per instance node: name, world positions, normals, triangles.
    pub instances: Vec<(String, Vec<V3>, Vec<V3>, Vec<[u32; 3]>)>,
    pub anchors: Vec<(String, V3)>,
    /// Joint nodes: name and world rest position.
    pub joints: Vec<(String, V3)>,
    /// Per instance: joints and weights per vertex (empty when not skinned).
    pub influences: Vec<Vec<([u32; 4], [f64; 4])>>,
    /// Per instance: COLOR_0 per vertex as stored (u8 values, 0 to 255).
    pub colours: Vec<Vec<[f64; 4]>>,
}

fn rd_u32(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes([*b.get(o)?, *b.get(o + 1)?, *b.get(o + 2)?, *b.get(o + 3)?]))
}

/// Read a `.glb` we wrote (the subset of glTF our writer uses).
pub fn read(glb: &[u8]) -> Result<ReadBack, String> {
    if glb.get(0..4) != Some(b"glTF") || rd_u32(glb, 4) != Some(2) {
        return Err("not a glTF 2.0 binary".into());
    }
    if rd_u32(glb, 8) != Some(glb.len() as u32) {
        return Err("length mismatch".into());
    }
    let jl = rd_u32(glb, 12).ok_or("short")? as usize;
    if glb.get(16..20) != Some(b"JSON") {
        return Err("first chunk is not JSON".into());
    }
    let js: Value = serde_json::from_slice(glb.get(20..20 + jl).ok_or("short JSON")?).map_err(|e| e.to_string())?;
    let bo = 20 + jl;
    let bl = rd_u32(glb, bo).ok_or("no BIN chunk")? as usize;
    if glb.get(bo + 4..bo + 8) != Some(b"BIN\0") {
        return Err("second chunk is not BIN".into());
    }
    let bin = glb.get(bo + 8..bo + 8 + bl).ok_or("short BIN")?;
    let acc = |i: usize| -> Result<(Vec<f64>, usize, usize), String> {
        let a = &js["accessors"][i];
        let v = &js["bufferViews"][a["bufferView"].as_u64().ok_or("accessor view")? as usize];
        let off = v["byteOffset"].as_u64().unwrap_or(0) as usize;
        let count = a["count"].as_u64().ok_or("count")? as usize;
        let comps = match a["type"].as_str() {
            Some("SCALAR") => 1,
            Some("VEC2") => 2,
            Some("VEC3") => 3,
            Some("VEC4") => 4,
            _ => return Err("type".into()),
        };
        let ct = a["componentType"].as_u64().ok_or("ctype")?;
        let size = match ct {
            5120 | 5121 => 1,
            5122 | 5123 => 2,
            5125 | 5126 => 4,
            _ => return Err("ctype".into()),
        };
        let stride = v["byteStride"].as_u64().map(|s| s as usize).unwrap_or(size * comps);
        let norm = a["normalized"].as_bool().unwrap_or(false);
        let mut out = Vec::with_capacity(count * comps);
        for e in 0..count {
            for c in 0..comps {
                let o = off + e * stride + c * size;
                let x = match ct {
                    5120 => {
                        let v = *bin.get(o).ok_or("range")? as i8 as f64;
                        if norm { (v / 127.0).max(-1.0) } else { v }
                    }
                    5121 => *bin.get(o).ok_or("range")? as f64,
                    5122 => {
                        let v = i16::from_le_bytes([*bin.get(o).ok_or("range")?, *bin.get(o + 1).ok_or("range")?]) as f64;
                        if norm { (v / 32767.0).max(-1.0) } else { v }
                    }
                    5123 => {
                        let v = u16::from_le_bytes([*bin.get(o).ok_or("range")?, *bin.get(o + 1).ok_or("range")?]) as f64;
                        if norm { v / 65535.0 } else { v }
                    }
                    5125 => rd_u32(bin, o).ok_or("range")? as f64,
                    _ => f32::from_bits(rd_u32(bin, o).ok_or("range")?) as f64,
                };
                out.push(x);
            }
        }
        Ok((out, count, comps))
    };
    let mut rb = ReadBack { json: js.clone(), ..Default::default() };
    // World translations: our nodes carry translation-only transforms
    // below the root (joints, anchors on joints).
    let all_nodes: Vec<Value> = js["nodes"].as_array().cloned().unwrap_or_default();
    let local = |n: &Value| n["translation"].as_array().map(|a| [a[0].as_f64().unwrap_or(0.0), a[1].as_f64().unwrap_or(0.0), a[2].as_f64().unwrap_or(0.0)]).unwrap_or([0.0; 3]);
    let mut parent: Vec<Option<usize>> = vec![None; all_nodes.len()];
    for (i, n) in all_nodes.iter().enumerate() {
        for c in n["children"].as_array().into_iter().flatten().filter_map(Value::as_u64) {
            if let Some(slot) = parent.get_mut(c as usize) {
                *slot = Some(i);
            }
        }
    }
    let world = |mut i: usize| {
        let mut t = [0.0; 3];
        for _ in 0..all_nodes.len() {
            t = add(t, local(&all_nodes[i]));
            match parent[i] {
                Some(p) => i = p,
                None => break,
            }
        }
        t
    };
    for (ni, n) in all_nodes.iter().enumerate() {
        let name = n["name"].as_str().unwrap_or("").to_string();
        let t = world(ni);
        if let Some(a) = name.strip_prefix("anchor:") {
            rb.anchors.push((a.to_string(), t));
        }
        if n["extras"]["evani"]["kind"] == "joint" {
            rb.joints.push((name.clone(), t));
        }
        let Some(mi) = n["mesh"].as_u64() else { continue };
        let s = n["scale"].as_array().map(|a| [a[0].as_f64().unwrap_or(1.0), a[1].as_f64().unwrap_or(1.0), a[2].as_f64().unwrap_or(1.0)]).unwrap_or([1.0; 3]);
        let prim = &js["meshes"][mi as usize]["primitives"][0];
        let (p, pc, _) = acc(prim["attributes"]["POSITION"].as_u64().ok_or("POSITION")? as usize)?;
        let (nn, _, _) = acc(prim["attributes"]["NORMAL"].as_u64().ok_or("NORMAL")? as usize)?;
        let (ix, _, _) = acc(prim["indices"].as_u64().ok_or("indices")? as usize)?;
        let mirror = s[0] < 0.0;
        let pos: Vec<V3> = (0..pc).map(|i| add(hadamard([p[3 * i], p[3 * i + 1], p[3 * i + 2]], s), t)).collect();
        let nrm: Vec<V3> = (0..pc).map(|i| norm_or([nn[3 * i] * if mirror { -1.0 } else { 1.0 }, nn[3 * i + 1], nn[3 * i + 2]], [0.0, 1.0, 0.0])).collect();
        let tris: Vec<[u32; 3]> = ix.chunks(3).map(|c| if mirror { [c[0] as u32, c[2] as u32, c[1] as u32] } else { [c[0] as u32, c[1] as u32, c[2] as u32] }).collect();
        let mut inf = Vec::new();
        if let (Some(ja), Some(wa)) = (prim["attributes"]["JOINTS_0"].as_u64(), prim["attributes"]["WEIGHTS_0"].as_u64()) {
            let (jj, _, _) = acc(ja as usize)?;
            let (ww, _, _) = acc(wa as usize)?;
            inf = (0..pc).map(|i| ([jj[4 * i] as u32, jj[4 * i + 1] as u32, jj[4 * i + 2] as u32, jj[4 * i + 3] as u32], [ww[4 * i], ww[4 * i + 1], ww[4 * i + 2], ww[4 * i + 3]])).collect();
        }
        let mut cols = Vec::new();
        if let Some(ca) = prim["attributes"]["COLOR_0"].as_u64() {
            let (cc, _, _) = acc(ca as usize)?;
            cols = (0..pc).map(|i| [cc[4 * i], cc[4 * i + 1], cc[4 * i + 2], cc[4 * i + 3]]).collect();
        }
        rb.influences.push(inf);
        rb.colours.push(cols);
        rb.instances.push((name, pos, nrm, tris));
    }
    Ok(rb)
}

/// A plain float `.glb` of named meshes (reference meshes for the benchmark;
/// no quantisation, no metadata beyond `extras`).
pub fn write_plain(meshes: &[(String, &crate::mesh::Mesh)], extras: &Value) -> Vec<u8> {
    let mut bin = Bin { data: Vec::new(), views: Vec::new(), accessors: Vec::new() };
    let mut gmeshes = Vec::new();
    let mut nodes = Vec::new();
    for (i, (name, m)) in meshes.iter().enumerate() {
        let mut pb = Vec::new();
        for p in &m.pos {
            for k in 0..3 {
                pb.extend_from_slice(&(p[k] as f32).to_le_bytes());
            }
        }
        let pv = bin.view(&pb, None, ARRAY);
        let lo: Vec<f32> = (0..3).map(|k| m.pos.iter().map(|p| p[k] as f32).fold(f32::INFINITY, f32::min)).collect();
        let hi: Vec<f32> = (0..3).map(|k| m.pos.iter().map(|p| p[k] as f32).fold(f32::NEG_INFINITY, f32::max)).collect();
        let pa = bin.accessor(pv, 5126, false, m.pos.len(), "VEC3", Some(json!(lo)), Some(json!(hi)));
        let mut ib = Vec::new();
        for t in &m.tris {
            for &v in t {
                ib.extend_from_slice(&v.to_le_bytes());
            }
        }
        let iv = bin.view(&ib, None, ELEMENT);
        let ia = bin.accessor(iv, 5125, false, m.tris.len() * 3, "SCALAR", None, None);
        gmeshes.push(json!({ "name": name, "primitives": [{ "attributes": { "POSITION": pa }, "indices": ia, "mode": 4 }] }));
        nodes.push(json!({ "name": name, "mesh": i }));
    }
    bin.align();
    let doc = json!({
        "asset": { "version": "2.0", "generator": format!("Evani.Model {} reference", env!("CARGO_PKG_VERSION")) },
        "scene": 0,
        "scenes": [{ "nodes": (0..nodes.len()).collect::<Vec<_>>() }],
        "nodes": nodes,
        "meshes": gmeshes,
        "buffers": [{ "byteLength": bin.data.len() }],
        "bufferViews": bin.views,
        "accessors": bin.accessors,
        "extras": extras
    });
    let mut js = serde_json::to_string(&doc).unwrap_or_default().into_bytes();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kart_tier_round_trips_and_is_byte_identical() {
        let j = crate::intent::parse_text(include_str!("../../../tests/model_bench/vehicles/kart_01_pip.evmodel")).unwrap();
        let tiers = crate::compile::compile(&j, Some(&["low".to_string()]), 2).unwrap();
        let tm = &tiers[0];
        let col = crate::collide::build_collision(&tm.model);
        let a = write_tier(tm, &col, None, &Map::new());
        let b = write_tier(tm, &col, None, &Map::new());
        assert_eq!(a, b);
        assert_eq!(a.len() % 4, 0);
        let rb = read(&a).unwrap();
        assert_eq!(rb.instances.len(), 5);
        assert_eq!(rb.json["extensionsRequired"][0], "KHR_mesh_quantization");
        // Dequantised positions match the compiled mesh within half a step.
        let world = tm.world_mesh();
        let mut got = Vec::new();
        for (_, p, _, _) in &rb.instances {
            got.extend_from_slice(p);
        }
        assert_eq!(got.len(), world.pos.len());
        let step = 3.2 / 65535.0;
        for (x, y) in got.iter().zip(&world.pos) {
            assert!(dist(*x, *y) < step, "{x:?} vs {y:?}");
        }
        // Mirrored wheels read back closed and outward.
        for (name, p, _, t) in &rb.instances {
            let r = crate::meshcheck::check(p, t, &Default::default());
            assert!(r.ok(), "{name}: {:?}", r.summary());
        }
        let hub = rb.anchors.iter().find(|a| a.0 == "wheel_hub.fr").unwrap();
        assert!((hub.1[0] + 0.9).abs() < 1e-6);
    }
}
