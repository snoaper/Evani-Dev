//! [`super::prepare`] in steps (model-read-spread, 2026-11-02): one
//! primitive a step, so the browser can spread a big streamed model over
//! frames ([`super::read`]). Stepped to the end it is exactly the one-go
//! prepare (that one runs these steps back to back).

use evani_gltf::{Error, Gltf};

use super::{Prepared, SHARED, material, part, sphere};

/// A prepare under way: the parts of nodes `..node` (and primitives
/// `..prim` of `node`) are in.
pub struct Steps {
    world: Vec<[f32; 16]>,
    hide: Vec<usize>,
    lod_nodes: Vec<Option<usize>>,
    mat_of: Vec<Option<usize>>,
    out: Prepared,
    node: usize,
    prim: usize,
}

/// The node's LOD level (or SHARED); `None`: hidden (a collision proxy or
/// under one), or the walk up found a cycle.
fn level_of(g: &Gltf, hide: &[usize], lod_nodes: &[Option<usize>], mut n: usize) -> Option<u8> {
    let mut lvl = SHARED;
    for _ in 0..=g.nodes.len() {
        if hide.contains(&n) {
            return None;
        }
        if lvl == SHARED && let Some(k) = lod_nodes.iter().position(|&l| l == Some(n)) {
            lvl = k as u8;
        }
        match g.parent(n) {
            Some(p) => n = p,
            None => return Some(lvl),
        }
    }
    None
}

impl Steps {
    pub fn new(g: &Gltf) -> Self {
        let lod_nodes: Vec<Option<usize>> = g.evani.as_ref().map(|e| e.lods.iter().take(usize::from(SHARED)).map(|l| l.node_index).collect()).unwrap_or_default();
        let levels = lod_nodes.len();
        let mut out = Prepared {
            parts: Vec::new(),
            materials: Vec::new(),
            textures: Vec::new(),
            triangles: 0,
            lods: match &g.evani {
                Some(e) if levels > 1 => e.lods.iter().take(levels).map(|l| l.min_screen).collect(),
                _ => Vec::new(),
            },
            lod_triangles: vec![0; levels],
            lod_parts: vec![0; levels],
            centre: [0.0; 3],
            radius: 0.0, #[cfg(feature = "skinning")] bind: Vec::new(),
            names: Vec::new(),
            anchors: g.anchors.iter().map(|a| (a.name.clone(), a.world)).collect(),
        };
        // A stable insertion sort by name, not the standard sort: a model has a
        // handful of anchors, and each standard sort costs kilobytes of wasm.
        for k in 1..out.anchors.len() {
            let mut j = k;
            while j > 0 && out.anchors[j].0 < out.anchors[j - 1].0 {
                out.anchors.swap(j - 1, j);
                j -= 1;
            }
        }
        Self { world: g.world_matrices(), hide: g.collision.into_iter().collect(), lod_nodes, mat_of: vec![None; g.materials.len() + 1], out, node: 0, prim: 0 }
    }

    /// One more triangle primitive in (nodes without one are passed over
    /// in the same step). True once every node is done.
    pub fn step(&mut self, g: &Gltf) -> Result<bool, Error> {
        while self.node < g.nodes.len() {
            let ni = self.node;
            let node = &g.nodes[ni];
            let mesh = node.mesh.and_then(|m| g.meshes.get(m).map(|x| (m, x)));
            let lvl = mesh.and_then(|_| level_of(g, &self.hide, &self.lod_nodes, ni)).filter(|_| node.skin.is_none());
            let (Some((mesh_i, mesh)), Some(lvl)) = (mesh, lvl) else {
                (self.node, self.prim) = (ni + 1, 0);
                continue;
            };
            let Some(p) = mesh.primitives.get(self.prim) else {
                (self.node, self.prim) = (ni + 1, 0);
                continue;
            };
            let pi = self.prim;
            self.prim += 1;
            let out = &mut self.out;
            let mi = p.material.filter(|&m| m < g.materials.len()).unwrap_or(g.materials.len());
            let mat = match self.mat_of[mi] {
                Some(i) => i,
                None => {
                    out.materials.push(material(g, g.materials.get(mi), &mut out.textures));
                    self.mat_of[mi] = Some(out.materials.len() - 1);
                    out.materials.len() - 1
                }
            };
            if let Some(mut pt) = part(g, p, self.world[ni], mat, [ni, mesh_i, pi])? {
                let levels = self.lod_nodes.len();
                let tris = pt.indices.len() as u32 / 3;
                pt.lod = lvl;
                if let Some(k) = Some(usize::from(lvl)).filter(|&k| k < levels) {
                    out.lod_triangles[k] = out.lod_triangles[k].saturating_add(tris);
                    out.lod_parts[k] += 1;
                }
                if lvl == SHARED || lvl == 0 {
                    out.triangles = out.triangles.saturating_add(tris);
                }
                out.parts.push(pt);
            }
            return Ok(false);
        }
        Ok(true)
    }

    /// Every node done (stepping on first if not): the bounds, the skinned
    /// parts and the node names, as the one-go prepare ends.
    pub fn finish(mut self, g: &Gltf) -> Result<Prepared, Error> {
        while !self.step(g)? {}
        let mut out = self.out;
        // The sun cones are not baked here: only the tiers without a shadow map
        // read them, so the GPU side bakes them over frames when one draws the
        // model ([`super::sun::Bake`]); a PC on Medium and up never pays for it.
        (out.centre, out.radius) = sphere(&out.parts); #[cfg(feature = "skinning")] { let (hide, lods) = (&self.hide, &self.lod_nodes); super::skin::add(g, &self.world, &|n| level_of(g, hide, lods, n), &mut self.mat_of, &mut out)?; }
        // Named nodes: every name on a part's way up to the root, sorted, the
        // first 32; a part's tags are the places of its own.
        let names_of = |p: &super::Part| core::iter::successors(Some(p.source[0]), |&k| g.parent(k)).take(g.nodes.len() + 1).filter_map(|k| g.nodes.get(k)).map(|n| n.name.as_str()).filter(|s| !s.is_empty()).collect::<Vec<_>>();
        // Sorted and unique by insertion: the same list as sort plus dedup,
        // without linking a standard sort for strings.
        for n in out.parts.iter().flat_map(names_of) {
            if let Err(i) = out.names.binary_search_by(|x| x.as_str().cmp(n)) {
                out.names.insert(i, n.to_owned());
            }
        }
        out.names.truncate(32);
        for p in &mut out.parts {
            p.tags = names_of(p).into_iter().filter_map(|n| out.names.binary_search_by(|x| x.as_str().cmp(n)).ok()).fold(0, |t, b| t | 1 << b);
        }
        Ok(out)
    }
}
