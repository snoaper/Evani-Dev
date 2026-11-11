//! A built model at one render tier: pieces in named groups (glTF nodes),
//! instances of prototype pieces written once, anchors and stages.
//!
//! Every piece keeps its own mesh until output, so the checks see each one
//! (closed, outward, not degenerate: check 1); [`Model::output`] then merges
//! a group's pieces into one mesh per material, which is what the writer
//! stores. A plane split ([`Model::split`]) moves whole pieces into two
//! named halves by their centres; [`check_split`] proves the halves together
//! are the whole (check 4). Cutting a piece that crosses the plane, with a
//! cap, is the fracture package's (MG8).

use crate::mesh::Mesh;
use crate::vec::*;

/// One exact piece.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    /// Stable path id (`lid/plank_3`): its seeds and its check reports use it.
    pub id: String,
    /// Output node (several pieces share one).
    pub group: String,
    /// Catalogue material name (`wood.oak_weathered`).
    pub material: String,
    /// Linear RGBA (the tone variation already applied).
    pub colour: [u8; 4],
    /// Per-piece variation value in [0, 1) (TEXCOORD_1.x), for looks that
    /// vary grain per piece.
    pub var: f64,
    /// Stages the piece is drawn in; empty: every stage.
    pub stages: Vec<String>,
    pub mesh: Mesh,
}

/// A prototype written once and placed by many [`Instance`]s.
#[derive(Clone, Debug, PartialEq)]
pub struct Proto {
    pub name: String,
    /// Pieces in the prototype's own frame (their `group` is ignored).
    pub pieces: Vec<Piece>,
}

/// A placed prototype: rotation, then translation; `mirror` negates x first.
#[derive(Clone, Debug, PartialEq)]
pub struct Instance {
    /// Node name (`studs_3`).
    pub name: String,
    pub proto: String,
    /// Parent group node.
    pub group: String,
    pub q: Quat,
    pub t: V3,
    pub mirror: bool,
}

/// A gameplay anchor (`anchor:<name>` node); `q` makes it a socket.
#[derive(Clone, Debug, PartialEq)]
pub struct Anchor {
    pub name: String,
    pub p: V3,
    pub q: Option<Quat>,
    pub group: String,
}

/// The model at one tier.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Model {
    pub id: String,
    pub seed: u64,
    pub tier: String,
    pub pieces: Vec<Piece>,
    pub protos: Vec<Proto>,
    pub instances: Vec<Instance>,
    pub anchors: Vec<Anchor>,
    /// Stage names in order (empty: the model has one look).
    pub stages: Vec<String>,
    /// Parts left out at this tier, with the reason.
    pub dropped: Vec<(String, String)>,
    /// Parameter values used (name, value), in name order.
    pub params: Vec<(String, f64)>,
    /// The recipe's content hash (stale-output check).
    pub recipe_hash: String,
}

/// One mesh of the output: a group's pieces of one material and stage set.
#[derive(Clone, Debug, PartialEq)]
pub struct OutMesh {
    pub material: String,
    pub mesh: Mesh,
    /// Linear RGBA per vertex.
    pub colour: Vec<[u8; 4]>,
    /// Per-vertex piece variation (TEXCOORD_1.x).
    pub var: Vec<f64>,
    /// Pieces merged into it (ids), in order.
    pub pieces: Vec<String>,
}

/// One output node: a group (or a prototype) and its meshes.
#[derive(Clone, Debug, PartialEq)]
pub struct OutNode {
    pub name: String,
    pub stages: Vec<String>,
    pub meshes: Vec<OutMesh>,
}

impl Piece {
    pub fn centre(&self) -> V3 {
        self.mesh.bounds().centre()
    }
}

fn merge(pieces: &[&Piece]) -> Vec<OutMesh> {
    let mut mats: Vec<&str> = pieces.iter().map(|p| p.material.as_str()).collect();
    mats.sort_unstable();
    mats.dedup();
    mats.iter()
        .map(|m| {
            let mut o = OutMesh { material: (*m).to_string(), mesh: Mesh::default(), colour: Vec::new(), var: Vec::new(), pieces: Vec::new() };
            for p in pieces.iter().filter(|p| p.material == *m) {
                o.mesh.append(&p.mesh);
                o.colour.extend(std::iter::repeat_n(p.colour, p.mesh.pos.len()));
                o.var.extend(std::iter::repeat_n(p.var, p.mesh.pos.len()));
                o.pieces.push(p.id.clone());
            }
            o
        })
        .collect()
}

impl Model {
    pub fn triangles(&self) -> usize {
        let own: usize = self.pieces.iter().map(|p| p.mesh.tris.len()).sum();
        let inst: usize = self.instances.iter().map(|i| self.proto(&i.proto).map_or(0, |p| p.pieces.iter().map(|x| x.mesh.tris.len()).sum())).sum();
        own + inst
    }
    pub fn proto(&self, name: &str) -> Option<&Proto> {
        self.protos.iter().find(|p| p.name == name)
    }
    /// Group nodes in first-use order, each split by stage set, with their
    /// merged meshes; then the prototypes (meshes written once).
    pub fn output(&self) -> (Vec<OutNode>, Vec<OutNode>) {
        let mut keys: Vec<(String, Vec<String>)> = Vec::new();
        for p in &self.pieces {
            let k = (p.group.clone(), p.stages.clone());
            if !keys.contains(&k) {
                keys.push(k);
            }
        }
        for i in &self.instances {
            let k = (i.group.clone(), Vec::new());
            if !keys.iter().any(|x| x.0 == k.0) {
                keys.push(k);
            }
        }
        let nodes = keys
            .iter()
            .map(|(g, st)| {
                let ps: Vec<&Piece> = self.pieces.iter().filter(|p| &p.group == g && &p.stages == st).collect();
                let name = if st.is_empty() { g.clone() } else { format!("{g}~{}", st.join("+")) };
                OutNode { name, stages: st.clone(), meshes: merge(&ps) }
            })
            .collect();
        let protos = self.protos.iter().map(|p| OutNode { name: p.name.clone(), stages: Vec::new(), meshes: merge(&p.pieces.iter().collect::<Vec<_>>()) }).collect();
        (nodes, protos)
    }
    /// Every piece in model space, instances placed (for checks and pictures).
    pub fn world_pieces(&self) -> Vec<Piece> {
        let mut out = self.pieces.clone();
        for i in &self.instances {
            if let Some(p) = self.proto(&i.proto) {
                for (k, pc) in p.pieces.iter().enumerate() {
                    let mut w = pc.clone();
                    w.id = format!("{}/{}#{k}", i.group, i.name);
                    w.group = i.group.clone();
                    w.mesh = pc.mesh.transformed(i.q, i.t, i.mirror);
                    out.push(w);
                }
            }
        }
        out
    }
    /// Move every piece of `groups` into `<group>_<names.0>` (centre on the
    /// positive side of `n . p = d`) or `<group>_<names.1>`.
    pub fn split(&mut self, groups: &[&str], n: V3, d: f64, names: (&str, &str)) {
        for p in &mut self.pieces {
            if groups.contains(&p.group.as_str()) {
                let side = if dot(n, p.centre()) >= d { names.0 } else { names.1 };
                p.group = format!("{}_{side}", p.group);
            }
        }
    }
}

/// Check 4 (MG1 form): after a split, each source piece is in exactly one
/// half with its mesh unchanged, and the halves' triangles and volume add up
/// to the whole. Returns the problems (empty when sound). Pieces that cross
/// the plane ([`crossing`]) go whole by their centre until MG8 cuts them.
pub fn check_split(before: &Model, after: &Model, groups: &[&str], names: (&str, &str)) -> Vec<String> {
    let mut bad = Vec::new();
    let half = |g: &str, s: &str, side: &str| g.len() == s.len() + side.len() + 1 && g.starts_with(s) && g.ends_with(side) && g.as_bytes()[s.len()] == b'_';
    let in_half = |g: &str, s: &str| half(g, s, names.0) || half(g, s, names.1);
    for p in before.pieces.iter().filter(|p| groups.contains(&p.group.as_str())) {
        let copies: Vec<&Piece> = after.pieces.iter().filter(|q| q.id == p.id).collect();
        match copies.as_slice() {
            [q] => {
                if !in_half(&q.group, &p.group) {
                    bad.push(format!("{}: in {}, not in either half of {}", p.id, q.group, p.group));
                }
                if q.mesh != p.mesh {
                    bad.push(format!("{}: its mesh changed in the split", p.id));
                }
            }
            [] => bad.push(format!("{}: lost in the split", p.id)),
            _ => bad.push(format!("{}: in {} halves", p.id, copies.len())),
        }
    }
    let sum = |m: &Model, f: &dyn Fn(&str) -> bool| -> (usize, f64) {
        m.pieces.iter().filter(|p| f(&p.group)).fold((0, 0.0), |a, p| (a.0 + p.mesh.tris.len(), a.1 + p.mesh.volume()))
    };
    let whole = sum(before, &|g| groups.contains(&g));
    let halves = sum(after, &|g| groups.iter().any(|s| in_half(g, s)));
    if whole.0 != halves.0 || (whole.1 - halves.1).abs() > 1e-9 * whole.1.abs().max(1.0) {
        bad.push(format!("halves {} triangles / {:.6} m3, whole {} / {:.6}", halves.0, halves.1, whole.0, whole.1));
    }
    bad
}

/// How many pieces of `m` cross the plane `n . p = d` (their box straddles it).
pub fn crossing(m: &Model, n: V3, d: f64) -> usize {
    m.pieces
        .iter()
        .filter(|p| {
            let (lo, hi) = p.mesh.pos.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |a, q| {
                let s = dot(n, *q) - d;
                (a.0.min(s), a.1.max(s))
            });
            lo < -1e-9 && hi > 1e-9
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::block;

    fn piece(id: &str, group: &str, mat: &str, c: V3) -> Piece {
        Piece { id: id.into(), group: group.into(), material: mat.into(), colour: [200, 150, 100, 255], var: 0.5, stages: Vec::new(), mesh: block(c, QID, [0.1, 0.1, 0.1]) }
    }

    #[test]
    fn output_merges_per_group_and_material_and_split_partitions() {
        let mut m = Model { id: "t".into(), ..Model::default() };
        m.pieces.push(piece("a/0", "a", "wood", [-1.0, 0.0, 0.0]));
        m.pieces.push(piece("a/1", "a", "iron", [0.0, 0.0, 0.0]));
        m.pieces.push(piece("a/2", "a", "wood", [1.0, 0.0, 0.0]));
        let mut dmg = piece("b/0", "b", "wood", [0.0, 1.0, 0.0]);
        dmg.stages = vec!["dmg0".into()];
        m.pieces.push(dmg);
        m.protos.push(Proto { name: "stud".into(), pieces: vec![piece("stud/0", "", "iron", [0.0; 3])] });
        m.instances.push(Instance { name: "stud_0".into(), proto: "stud".into(), group: "a".into(), q: QID, t: [0.0, 2.0, 0.0], mirror: false });
        let (nodes, protos) = m.output();
        assert_eq!(nodes.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["a", "b~dmg0"]);
        assert_eq!(nodes[0].meshes.iter().map(|o| (o.material.as_str(), o.pieces.len())).collect::<Vec<_>>(), [("iron", 1), ("wood", 2)]);
        assert_eq!(protos.len(), 1);
        assert_eq!(m.triangles(), 5 * 12);
        assert_eq!(m.world_pieces().len(), 5);
        let before = m.clone();
        assert_eq!(crossing(&m, [1.0, 0.0, 0.0], 0.0), 2);
        m.split(&["a"], [1.0, 0.0, 0.0], 0.0, ("fore", "aft"));
        let groups: Vec<&str> = m.pieces.iter().map(|p| p.group.as_str()).collect();
        assert_eq!(groups, ["a_aft", "a_fore", "a_fore", "b"]);
        let bad = check_split(&before, &m, &["a"], ("fore", "aft"));
        assert!(bad.is_empty(), "{bad:?}");
        // A piece dropped from a half is caught.
        let mut lost = m.clone();
        lost.pieces.remove(0);
        assert!(!check_split(&before, &lost, &["a"], ("fore", "aft")).is_empty());
    }
}
