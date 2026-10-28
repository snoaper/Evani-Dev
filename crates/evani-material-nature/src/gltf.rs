//! Nature surfaces from a glTF model: which materials are water, glass or
//! thin, and where their primitives sit.
//!
//! A material is picked, first match wins, by:
//! 1. its own `extras.evani_kind` (any [`Kind::from_name`] name);
//! 2. the root `extras.evani.nature` map (material name to kind name), which
//!    `cargo xtask convert` writes so the tags ride through every tier file;
//! 3. its glTF extensions: `KHR_materials_transmission` with an index of
//!    refraction (`KHR_materials_ior`) near 1.33 is water, any other
//!    transmission is glass (a pane when thin, else a bottle), and
//!    `KHR_materials_diffuse_transmission` is a thin surface (a leaf or paper
//!    patch when flat, else a blob).
//!
//! Each picked primitive's box is its `POSITION` accessor's `min`/`max`
//! taken through its node's world matrix (the surface turns only about +y,
//! so a tilted node's box is an approximation). Water's centre sits at the
//! top of its box (the water level).
//!
//! The model is still drawn by the engine: a game hides its water and glass
//! parts from the solid draw with
//! `ModelInstance.hide |= models.node_bit(id, name)` for each name in
//! [`GltfNature::hide`], and places [`GltfNature::surfaces`] with
//! [`Surface::placed`] where the instance stands. Keep water and glass in
//! nodes of their own: hiding is per node.

use serde_json::Value;

use crate::surface::{Kind, Surface};

/// What a model holds for the nature pass.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GltfNature {
    /// The surfaces, in the model's own space.
    pub surfaces: Vec<Surface>,
    /// Names of the nodes holding water or glass (sorted, no repeats).
    pub hide: Vec<String>,
}

/// Read a `.glb` (or a `.gltf` JSON text): only its JSON chunk is used.
pub fn from_glb(bytes: &[u8]) -> Result<GltfNature, String> {
    let json = json_chunk(bytes)?;
    let v: Value = serde_json::from_slice(json).map_err(|e| format!("glTF JSON: {e}"))?;
    Ok(from_json(&v))
}

/// The JSON chunk of a `.glb`, or the whole input if it is a JSON text.
pub fn json_chunk(bytes: &[u8]) -> Result<&[u8], String> {
    if bytes.first().is_some_and(|b| *b == b'{') {
        return Ok(bytes);
    }
    let word = |at: usize| -> Option<u32> {
        bytes
            .get(at..at + 4)
            .and_then(|s| s.try_into().ok())
            .map(u32::from_le_bytes)
    };
    if word(0) != Some(0x4654_6C67) {
        return Err("not a glb (no `glTF` magic)".into());
    }
    let len = word(12).ok_or("glb: cut short before the JSON chunk")? as usize;
    if word(16) != Some(0x4E4F_534A) {
        return Err("glb: the first chunk is not JSON".into());
    }
    bytes
        .get(20..20 + len)
        .ok_or_else(|| "glb: the JSON chunk runs past the end".into())
}

enum Pick {
    Exact(Kind),
    Water,
    Glass,
    Thin,
}

fn f64_at(v: &Value, path: &str) -> Option<f64> {
    v.pointer(path).and_then(Value::as_f64)
}

/// What material `index` is, if it is anything for the nature pass (the
/// shape-dependent kinds as their default: glass a pane, thin a blob).
pub fn material_kind(json: &Value, index: usize) -> Option<Kind> {
    pick(json, index).map(|p| match p {
        Pick::Exact(k) => k,
        Pick::Water => Kind::WaterRect,
        Pick::Glass => Kind::GlassPane,
        Pick::Thin => Kind::ThinBlob,
    })
}

fn pick(json: &Value, index: usize) -> Option<Pick> {
    let mat = json.get("materials")?.get(index)?;
    if let Some(k) = mat.pointer("/extras/evani_kind").and_then(Value::as_str).and_then(Kind::from_name) {
        return Some(Pick::Exact(k));
    }
    if let Some(k) = mat
        .get("name")
        .and_then(Value::as_str)
        .and_then(|n| json.pointer("/extras/evani/nature")?.get(n))
        .and_then(Value::as_str)
        .and_then(Kind::from_name)
    {
        return Some(Pick::Exact(k));
    }
    let ext = mat.get("extensions");
    let transmission = ext.and_then(|e| f64_at(e, "/KHR_materials_transmission/transmissionFactor"));
    if ext.is_some_and(|e| e.get("KHR_materials_transmission").is_some()) && transmission.unwrap_or(0.0) > 0.0 {
        let ior = ext.and_then(|e| f64_at(e, "/KHR_materials_ior/ior")).unwrap_or(1.5);
        return Some(if (1.30..=1.36).contains(&ior) {
            Pick::Water
        } else {
            Pick::Glass
        });
    }
    let diffuse = ext.and_then(|e| f64_at(e, "/KHR_materials_diffuse_transmission/diffuseTransmissionFactor"));
    if diffuse.unwrap_or(0.0) > 0.0 {
        return Some(Pick::Thin);
    }
    None
}

type Mat4 = [f32; 16];

const IDENTITY: Mat4 = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

fn mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut o = [0.0; 16];
    for c in 0..4 {
        for r in 0..4 {
            o[c * 4 + r] = (0..4).map(|k| a[k * 4 + r] * b[c * 4 + k]).sum();
        }
    }
    o
}

fn floats<const N: usize>(v: Option<&Value>) -> Option<[f32; N]> {
    let a = v?.as_array()?;
    if a.len() != N {
        return None;
    }
    let mut o = [0.0f32; N];
    for (i, x) in a.iter().enumerate() {
        o[i] = x.as_f64()? as f32;
    }
    Some(o)
}

/// A node's local matrix (`matrix`, else translation * rotation * scale).
fn local(node: &Value) -> Mat4 {
    if let Some(m) = floats::<16>(node.get("matrix")) {
        return m;
    }
    let t = floats::<3>(node.get("translation")).unwrap_or([0.0; 3]);
    let [x, y, z, w] = floats::<4>(node.get("rotation")).unwrap_or([0.0, 0.0, 0.0, 1.0]);
    let s = floats::<3>(node.get("scale")).unwrap_or([1.0; 3]);
    let r = [
        [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y + z * w), 2.0 * (x * z - y * w)],
        [2.0 * (x * y - z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z + x * w)],
        [2.0 * (x * z + y * w), 2.0 * (y * z - x * w), 1.0 - 2.0 * (x * x + y * y)],
    ];
    let mut m = IDENTITY;
    for c in 0..3 {
        for k in 0..3 {
            m[c * 4 + k] = r[c][k] * s[c];
        }
    }
    m[12] = t[0];
    m[13] = t[1];
    m[14] = t[2];
    m
}

/// Every node with its world matrix, walking the default scene (or every
/// root when there is no scene list).
fn world_nodes(json: &Value) -> Vec<(usize, Mat4)> {
    let nodes = json.get("nodes").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
    let kids = |n: &Value| -> Vec<usize> {
        n.get("children")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|c| c.as_u64().map(|c| c as usize))
            .collect()
    };
    let scene = json.get("scene").and_then(Value::as_u64).unwrap_or(0) as usize;
    let roots: Vec<usize> = match json.pointer(&format!("/scenes/{scene}/nodes")).and_then(Value::as_array) {
        Some(r) => r.iter().filter_map(|c| c.as_u64().map(|c| c as usize)).collect(),
        None => {
            let mut child = vec![false; nodes.len()];
            for n in nodes {
                for c in kids(n) {
                    if let Some(f) = child.get_mut(c) {
                        *f = true;
                    }
                }
            }
            (0..nodes.len()).filter(|&i| !child[i]).collect()
        }
    };
    let mut seen = vec![false; nodes.len()];
    let mut out = Vec::new();
    let mut stack: Vec<(usize, Mat4)> = roots.into_iter().rev().map(|r| (r, IDENTITY)).collect();
    while let Some((i, parent)) = stack.pop() {
        let Some(node) = nodes.get(i) else { continue };
        if std::mem::replace(&mut seen[i], true) {
            continue;
        }
        let m = mul(&parent, &local(node));
        for c in kids(node).into_iter().rev() {
            stack.push((c, m));
        }
        out.push((i, m));
    }
    out
}

/// What a model holds for the nature pass, from its glTF JSON.
pub fn from_json(json: &Value) -> GltfNature {
    let mut out = GltfNature::default();
    let nodes = json.get("nodes").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
    for (ni, m) in world_nodes(json) {
        let Some(node) = nodes.get(ni) else { continue };
        let Some(mesh) = node.get("mesh").and_then(Value::as_u64).and_then(|i| json.pointer(&format!("/meshes/{i}"))) else {
            continue;
        };
        let prims = mesh.get("primitives").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
        let mut hide = false;
        for prim in prims {
            let Some(p) = prim.get("material").and_then(Value::as_u64).and_then(|i| pick(json, i as usize)) else {
                continue;
            };
            let Some(acc) = prim
                .pointer("/attributes/POSITION")
                .and_then(Value::as_u64)
                .and_then(|i| json.pointer(&format!("/accessors/{i}")))
            else {
                continue;
            };
            let (Some(lo), Some(hi)) = (floats::<3>(acc.get("min")), floats::<3>(acc.get("max"))) else {
                continue;
            };
            let Some(s) = surface(p, &m, lo, hi) else { continue };
            hide |= s.kind.see_through();
            out.surfaces.push(s);
        }
        if hide
            && let Some(name) = node.get("name").and_then(Value::as_str).filter(|n| !n.is_empty())
            && let Err(at) = out.hide.binary_search_by(|x| x.as_str().cmp(name))
        {
            out.hide.insert(at, name.to_owned());
        }
    }
    out
}

fn surface(p: Pick, m: &Mat4, lo: [f32; 3], hi: [f32; 3]) -> Option<Surface> {
    let c = [0, 1, 2].map(|k| (lo[k] + hi[k]) * 0.5);
    let col = |k: usize| [m[k * 4], m[k * 4 + 1], m[k * 4 + 2]];
    let len = |v: [f32; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let centre = [0, 1, 2].map(|r| m[r] * c[0] + m[4 + r] * c[1] + m[8 + r] * c[2] + m[12 + r]);
    let half = [0, 1, 2].map(|k| (hi[k] - lo[k]).abs() * 0.5 * len(col(k)));
    let c0 = col(0);
    let yaw = (-c0[2]).atan2(c0[0]);
    let thinnest = half.iter().copied().fold(f32::INFINITY, f32::min);
    let widest = half.iter().copied().fold(0.0, f32::max);
    let flat = thinnest < 0.25 * widest;
    let kind = match p {
        Pick::Exact(k) => k,
        Pick::Water => Kind::WaterRect,
        Pick::Glass if flat => Kind::GlassPane,
        Pick::Glass => Kind::GlassBottle,
        Pick::Thin if flat => Kind::ThinPatch,
        Pick::Thin => Kind::ThinBlob,
    };
    let (centre, half) = match kind {
        Kind::WaterEllipse | Kind::WaterRect => ([centre[0], centre[1] + half[1], centre[2]], half),
        Kind::GlassBottle => {
            let r = half[0].max(half[2]);
            (centre, [r, (half[1] - r).max(0.0), r])
        }
        _ => (centre, half),
    };
    let s = Surface::new(kind, centre, half).with_yaw(yaw);
    let ok = s.centre.iter().chain(&s.half).all(|v| v.is_finite()) && s.yaw.is_finite();
    ok.then_some(s)
}
