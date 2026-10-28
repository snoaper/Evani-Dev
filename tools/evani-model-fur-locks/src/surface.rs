//! The generator's input: a world-space triangle mesh where every triangle
//! knows its solid, its colour region and its owning part.

use evani_model::build::Model;
use evani_model::compile::SolidMesh;
use evani_model::mesh::Mesh;
use evani_model::vec::{V3, add};

/// A triangle mesh in world space, labelled per triangle. `regions` and
/// `solids` are name tables; `tri_region` and `tri_solid` index them. The
/// empty region name means "no region" (treated as gear).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FurSurface {
    pub pos: Vec<V3>,
    /// Unit vertex normals (outward). Empty: face normals are used.
    pub nrm: Vec<V3>,
    pub tris: Vec<[u32; 3]>,
    pub tri_region: Vec<u32>,
    pub tri_solid: Vec<u32>,
    /// Owning part per triangle, copied to the locks grown from it (the
    /// lock solid's `tri_part`, so colours, bones and the bake follow).
    pub tri_part: Vec<u32>,
    pub regions: Vec<String>,
    pub solids: Vec<String>,
}

fn intern(list: &mut Vec<String>, name: &str) -> u32 {
    match list.iter().position(|n| n == name) {
        Some(i) => i as u32,
        None => {
            list.push(name.to_string());
            (list.len() - 1) as u32
        }
    }
}

impl FurSurface {
    pub fn new() -> Self {
        FurSurface::default()
    }

    /// Add one mesh placed at `t` (and mirrored across x = 0 first when
    /// `mirror`, as a mirrored solid instance is), every triangle in solid
    /// `solid` and in the region `region_of(part)` names for its part.
    pub fn add_mesh(&mut self, mesh: &Mesh, solid: &str, t: V3, mirror: bool, region_of: &dyn Fn(u32) -> String) {
        let base = self.pos.len() as u32;
        let sx = if mirror { -1.0 } else { 1.0 };
        self.pos.extend(mesh.pos.iter().map(|p| add([p[0] * sx, p[1], p[2]], t)));
        let has_nrm = mesh.nrm.len() == mesh.pos.len();
        if has_nrm && self.nrm.len() == base as usize {
            self.nrm.extend(mesh.nrm.iter().map(|n| [n[0] * sx, n[1], n[2]]));
        } else {
            // One mesh without normals: fall back to face normals everywhere.
            self.nrm.clear();
        }
        let si = intern(&mut self.solids, solid);
        for (ti, tri) in mesh.tris.iter().enumerate() {
            let tri = [tri[0] + base, tri[1] + base, tri[2] + base];
            self.tris.push(if mirror { [tri[0], tri[2], tri[1]] } else { tri });
            let part = mesh.tri_part.get(ti).copied().unwrap_or(0);
            let ri = intern(&mut self.regions, &region_of(part));
            self.tri_region.push(ri);
            self.tri_solid.push(si);
            self.tri_part.push(part);
        }
    }

    /// Every instance of every meshed solid of `model`, regions from the
    /// parts' colour regions. The usual input: the solids a tier meshed
    /// (`TierMesh::solids` from `evani_model::compile`).
    pub fn from_solids(model: &Model, solids: &[SolidMesh]) -> Self {
        let mut s = FurSurface::new();
        let region_of = |part: u32| model.parts.get(part as usize).and_then(|p| p.region.clone()).unwrap_or_default();
        for sm in solids {
            for inst in &sm.instances {
                s.add_mesh(&sm.mesh, &inst.solid, inst.t, inst.mirror, &region_of);
            }
        }
        s
    }

    /// The region name of triangle `t`.
    pub fn region_of(&self, t: usize) -> &str {
        self.tri_region.get(t).and_then(|&r| self.regions.get(r as usize)).map_or("", String::as_str)
    }

    /// The solid name of triangle `t`.
    pub fn solid_of(&self, t: usize) -> &str {
        self.tri_solid.get(t).and_then(|&r| self.solids.get(r as usize)).map_or("", String::as_str)
    }
}
