//! A model tier at rest, before any region rules: every triangle primitive
//! merged in model space at the bind pose (a skinned mesh skinned with its
//! own bind matrices, `world(joint) x inverse bind`, the way the engine's
//! renderer binds it), with each vertex's glTF material, its body chart
//! (`_EVANI_REGION`) and the file's anchors and joints.
//!
//! The geometry stage's tests and `cargo xtask style-bake` read the horse
//! through it, and [`crate::proxy`] builds the mane and tail from its
//! anchors. WP2's `source::source_from_glb` (the region rules, as data) can
//! build on it; this module maps no regions itself.

use evani_gltf::{Gltf, IDENTITY, mul, transform_point};

use crate::math::{V3, normalize};

/// One model tier at its rest pose, in file units.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RestMesh {
    pub positions: Vec<V3>,
    pub normals: Vec<V3>,
    pub indices: Vec<u32>,
    /// Per vertex: the glTF material index (255: none).
    pub material: Vec<u8>,
    /// Per vertex: the body chart, four bytes (zeros: no chart).
    pub chart: Vec<[u8; 4]>,
    /// The file's material names, by index.
    pub material_names: Vec<String>,
    /// `anchor:<name>` nodes: name and rest position.
    pub anchors: Vec<(String, V3)>,
    /// Every named node: name and rest position (a joint's head).
    pub joints: Vec<(String, V3)>,
}

impl RestMesh {
    pub fn anchor(&self, name: &str) -> Option<V3> {
        self.anchors.iter().find(|(n, _)| n == name).map(|(_, p)| *p)
    }

    pub fn joint(&self, name: &str) -> Option<V3> {
        self.joints.iter().find(|(n, _)| n == name).map(|(_, p)| *p)
    }

    /// The vertex's material name ("" when it has none).
    pub fn material_name(&self, vertex: usize) -> &str {
        self.material.get(vertex).and_then(|&m| self.material_names.get(usize::from(m))).map_or("", String::as_str)
    }
}

fn err(e: evani_gltf::Error) -> String {
    format!("model: {e}")
}

/// The nodes under every detail level but the first (`extras.evani.lods`):
/// a tier `cargo xtask convert` made holds its levels side by side, and
/// reading them all would lay two horses on top of each other.
fn later_lod_nodes(g: &Gltf) -> Vec<bool> {
    let mut skip = vec![false; g.nodes.len()];
    let roots = g.evani.as_ref().map(|e| e.lods.iter().skip(1).filter_map(|l| l.node_index).collect::<Vec<_>>()).unwrap_or_default();
    let mut stack = roots;
    while let Some(i) = stack.pop() {
        if let Some(s) = skip.get_mut(i).filter(|s| !**s) {
            *s = true;
            stack.extend(g.nodes[i].children.iter().copied());
        }
    }
    skip
}

/// Read a `.glb` tier at rest (its first detail level only).
pub fn rest_from_glb(glb: &[u8]) -> Result<RestMesh, String> {
    let g = Gltf::from_glb(glb).map_err(err)?;
    let world = g.world_matrices();
    let skip = later_lod_nodes(&g);
    let mut out = RestMesh { material_names: g.materials.iter().map(|m| m.name.clone()).collect(), ..RestMesh::default() };
    for (ni, node) in g.nodes.iter().enumerate() {
        if skip[ni] {
            continue;
        }
        if !node.name.is_empty() {
            let w = world.get(ni).unwrap_or(&IDENTITY);
            out.joints.push((node.name.clone(), [w[12], w[13], w[14]]));
        }
        let Some(mi) = node.mesh else { continue };
        let mesh = g.meshes.get(mi).ok_or("model: a node names a mesh past the list")?;
        // A skinned mesh is placed by its joints, never by its node.
        let bind: Option<Vec<[f32; 16]>> = match node.skin.and_then(|s| g.skins.get(s)) {
            Some(s) => {
                let ibm = match s.inverse_bind_matrices {
                    Some(a) => g.read_f32(a).map_err(err)?,
                    None => IDENTITY.repeat(s.joints.len()),
                };
                if ibm.len() != s.joints.len() * 16 {
                    return Err("model: not one inverse bind matrix per joint".into());
                }
                Some(s.joints.iter().zip(ibm.as_chunks::<16>().0).map(|(&j, b)| mul(world.get(j).unwrap_or(&IDENTITY), b)).collect())
            }
            None => None,
        };
        let node_world = *world.get(ni).unwrap_or(&IDENTITY);
        for p in &mesh.primitives {
            if p.mode != 4 {
                continue;
            }
            let pos_a = p.attribute("POSITION").ok_or("model: a primitive without POSITION")?;
            let pos = g.read_f32(pos_a).map_err(err)?;
            let n = pos.len() / 3;
            let nor = match p.attribute("NORMAL") {
                Some(a) => g.read_f32(a).map_err(err)?,
                None => vec![0.0; n * 3],
            };
            let chart = match p.attribute("_EVANI_REGION") {
                Some(a) => g.read_f32(a).map_err(err)?,
                None => vec![0.0; n * 4],
            };
            if nor.len() != n * 3 || chart.len() != n * 4 {
                return Err("model: attribute counts don't match POSITION".into());
            }
            let skin = match (&bind, p.attribute("JOINTS_0"), p.attribute("WEIGHTS_0")) {
                (Some(b), Some(ja), Some(wa)) => {
                    let j = g.read_u32(ja).map_err(err)?;
                    let w = g.read_f32(wa).map_err(err)?;
                    if j.len() != n * 4 || w.len() != n * 4 || j.iter().any(|&x| x as usize >= b.len()) {
                        return Err("model: bad JOINTS_0 or WEIGHTS_0".into());
                    }
                    Some((j, w))
                }
                _ => None,
            };
            let base = out.positions.len() as u32;
            for v in 0..n {
                let p0 = [pos[v * 3], pos[v * 3 + 1], pos[v * 3 + 2]];
                let n0 = [nor[v * 3], nor[v * 3 + 1], nor[v * 3 + 2]];
                let m = match (&skin, &bind) {
                    (Some((j, w)), Some(b)) => {
                        let mut m = [0.0f32; 16];
                        let sum: f32 = (0..4).map(|k| w[v * 4 + k].max(0.0)).sum();
                        for k in 0..4 {
                            let wk = if sum > 0.0 { w[v * 4 + k].max(0.0) / sum } else if k == 0 { 1.0 } else { 0.0 };
                            if wk == 0.0 {
                                continue;
                            }
                            let bm = &b[j[v * 4 + k] as usize];
                            for (mi, bi) in m.iter_mut().zip(bm) {
                                *mi += wk * bi;
                            }
                        }
                        m
                    }
                    _ => node_world,
                };
                out.positions.push(transform_point(&m, p0));
                let tn = [m[0] * n0[0] + m[4] * n0[1] + m[8] * n0[2], m[1] * n0[0] + m[5] * n0[1] + m[9] * n0[2], m[2] * n0[0] + m[6] * n0[1] + m[10] * n0[2]];
                out.normals.push(normalize(tn, [0.0, 1.0, 0.0]));
                out.material.push(p.material.map_or(255, |m| m.min(254) as u8));
                out.chart.push([0, 1, 2, 3].map(|k| (chart[v * 4 + k].clamp(0.0, 1.0) * 255.0).round() as u8));
            }
            match p.indices {
                Some(ia) => {
                    let idx = g.read_u32(ia).map_err(err)?;
                    if idx.iter().any(|&i| i as usize >= n) || !idx.len().is_multiple_of(3) {
                        return Err("model: indices out of range".into());
                    }
                    out.indices.extend(idx.iter().map(|&i| base + i));
                }
                None => out.indices.extend((0..(n as u32 / 3 * 3)).map(|i| base + i)),
            }
        }
    }
    for a in &g.anchors {
        out.anchors.push((a.name.clone(), [a.world[12], a.world[13], a.world[14]]));
    }
    if out.positions.is_empty() || out.indices.is_empty() {
        return Err("model: no triangles".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `.glb` of one triangle drawn by two nodes, the second moved 5 along
    /// x; `lods` names them as detail levels the way `cargo xtask convert`
    /// writes them.
    fn two_level_glb(lods: bool) -> Vec<u8> {
        let extras = if lods { r#","extras":{"evani":{"format":1,"lods":[{"node":"horse_lod0","min_screen":0.25},{"node":"horse_lod1","min_screen":0.0}]}}"# } else { "" };
        let json = format!(
            r#"{{"asset":{{"version":"2.0"}},"buffers":[{{"byteLength":36}}],"bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":36}}],"accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}}],"meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}}}}]}}],"nodes":[{{"name":"horse","children":[1,2]}},{{"name":"horse_lod0","mesh":0}},{{"name":"horse_lod1","mesh":0,"translation":[5,0,0]}}],"scenes":[{{"nodes":[0]}}],"scene":0{extras}}}"#
        );
        let mut json = json.into_bytes();
        while !json.len().is_multiple_of(4) {
            json.push(b' ');
        }
        let bin: Vec<u8> = [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0].iter().flat_map(|f| f.to_le_bytes()).collect();
        let total = 12 + 8 + json.len() + 8 + bin.len();
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&(json.len() as u32).to_le_bytes());
        out.extend_from_slice(b"JSON");
        out.extend_from_slice(&json);
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&bin);
        out
    }

    /// A converted tier keeps its detail levels side by side: only the first
    /// is read (never two horses on top of each other); a file without
    /// levels reads every node.
    #[test]
    fn only_the_first_detail_level_is_read() {
        let one = rest_from_glb(&two_level_glb(true)).unwrap();
        assert_eq!(one.positions.len(), 3);
        assert!(one.positions.iter().all(|p| p[0] <= 1.0), "{:?}", one.positions);
        assert!(one.joint("horse_lod1").is_none());
        let both = rest_from_glb(&two_level_glb(false)).unwrap();
        assert_eq!(both.positions.len(), 6);
    }
}
