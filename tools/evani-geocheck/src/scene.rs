//! A drawn scene as the checks see it: the instances a kit pushed into
//! `RenderWorld`, their world shapes, which of them are static, and how they
//! group into objects (a tree is a trunk and a canopy, a kart is a body,
//! wheels and a driver).
//!
//! Grouping rule (generic, no kit help needed): kits draw an object's parts
//! close together in draw order, touching. Touching parts within the last
//! [`Config::window`] draws whose union stays under [`Config::max_object`]
//! form one object (details in [`Scene::build`]). Instances
//! at least [`Config::structure`] across (ground, road slabs, mountains)
//! stand alone as structures; very thin ones, and everything the caller says
//! was drawn with `draw_decal` ([`Scene::build_with`]), are decals (paint
//! lines). Instances whose mesh has no registered shape are listed in
//! [`Scene::skipped`], never dropped silently.

use std::collections::HashMap;

use evani_render::mesh::MeshId;
use evani_render::world::MeshInstance;

use crate::geom::*;
use crate::shape::{Shape, Shapes};

/// Tolerances of every check (metres unless noted). A kit can override any
/// of them in its `geocheck.toml` `[thresholds]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// Instances at least this wide are structures (terrain, road, walls).
    pub structure: f64,
    /// Largest object a run of touching parts may form.
    pub max_object: f64,
    /// Parts closer than this count as touching (grouping).
    pub touch: f64,
    /// How many previous parts of a run a new part may touch.
    pub window: usize,
    /// Thinner than this (and flat): a decal.
    pub decal: f64,
    /// Overlap volume that is an issue (m^3).
    pub overlap_volume: f64,
    /// Sinking into a structure up to this depth is contact, not overlap.
    pub sink: f64,
    /// Gap under an object that makes it float.
    pub float_gap: f64,
    /// A decal (drawn with `draw_decal`) further than this from every
    /// surface (above, below or buried) is off its surface.
    pub decal_gap: f64,
    /// Coplanar faces closer than this count as one surface in the render
    /// flicker test (the geometry test works from the depth maths,
    /// `zfight.rs`, with no gap limit).
    pub zfight_gap: f64,
    /// Faces whose normals are within this angle (degrees; parallel or
    /// opposite) are tested for z-fighting.
    pub zfight_angle: f64,
    /// Smallest flickering patch on screen that is an issue (pixels).
    pub zfight_px: f64,
    /// Farthest view distance the game draws surfaces at (m): z-fighting is
    /// looked for out to here (or the far plane, if nearer).
    pub view_distance: f64,
    /// Moving objects: penetration depth and volume that are an issue.
    pub penetration: f64,
    pub penetration_volume: f64,
    /// Collision mismatch: how far the collision may sit from the visible wall.
    pub collision_tol: f64,
    /// Fraction of frames an instance must appear in, unchanged, to be static.
    pub static_frac: f64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            structure: 10.0,
            max_object: 12.0,
            touch: 0.15,
            window: 12,
            decal: 0.05,
            overlap_volume: 0.002,
            sink: 0.1,
            float_gap: 0.03,
            decal_gap: 0.03,
            zfight_gap: 0.05,
            zfight_angle: 10.0,
            zfight_px: 4.0,
            view_distance: 150.0,
            penetration: 0.2,
            penetration_volume: 0.02,
            collision_tol: 0.6,
            static_frac: 0.8,
        }
    }
}

impl Config {
    /// Override fields from a JSON object (`{"sink": 0.1, ...}`).
    pub fn apply_json(&mut self, v: &serde_json::Value) -> Result<(), String> {
        let Some(o) = v.as_object() else {
            return Err("thresholds must be a table".into());
        };
        for (k, val) in o {
            let x = val.as_f64().ok_or_else(|| format!("threshold {k} must be a number"))?;
            let slot: &mut f64 = match k.as_str() {
                "structure" => &mut self.structure,
                "max_object" => &mut self.max_object,
                "touch" => &mut self.touch,
                "decal" => &mut self.decal,
                "overlap_volume" => &mut self.overlap_volume,
                "sink" => &mut self.sink,
                "float_gap" => &mut self.float_gap,
                "decal_gap" => &mut self.decal_gap,
                "zfight_gap" => &mut self.zfight_gap,
                "zfight_angle" => &mut self.zfight_angle,
                "zfight_px" => &mut self.zfight_px,
                "view_distance" => &mut self.view_distance,
                "penetration" => &mut self.penetration,
                "penetration_volume" => &mut self.penetration_volume,
                "collision_tol" => &mut self.collision_tol,
                "static_frac" => &mut self.static_frac,
                "window" => {
                    self.window = x.max(1.0) as usize;
                    continue;
                }
                _ => return Err(format!("unknown threshold {k}")),
            };
            *slot = x;
        }
        Ok(())
    }
}

/// What an instance is to the checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A part of a prop or character.
    Part,
    /// Terrain, road, big walls: at least [`Config::structure`] across.
    Structure,
    /// Paint lines, decals, shadows: anything drawn with `draw_decal`
    /// whatever its thickness, and thin flat geometry (5 cm or less).
    Decal,
}

/// One drawn instance.
#[derive(Debug, Clone)]
pub struct Item {
    /// Index in the frame's `RenderWorld::meshes`.
    pub index: usize,
    pub inst: MeshInstance,
    pub shape: Shape,
    pub kind: Kind,
    /// Class name for the report ("trunk (cylinder)").
    pub label: String,
    pub object: usize,
    /// Drawn with `RenderWorld::draw_decal` (depth offset, meant to lie on a
    /// surface): never z-fights the surface under it, but must lie on one.
    /// Thin geometry drawn as a plain mesh is `Kind::Decal` without this.
    pub decal: bool,
}

/// A group of items that form one thing.
#[derive(Debug, Clone)]
pub struct Object {
    pub items: Vec<usize>,
    pub aabb: Aabb,
    pub kind: Kind,
    pub label: String,
}

/// Uniform grid over XZ for broad-phase queries.
#[derive(Debug, Default, Clone)]
pub struct Grid {
    cell: f64,
    map: HashMap<(i64, i64), Vec<usize>>,
}

impl Grid {
    pub fn new(cell: f64) -> Self {
        Self {
            cell: cell.max(0.5),
            map: HashMap::new(),
        }
    }
    fn range(&self, b: &Aabb) -> (i64, i64, i64, i64) {
        let f = |x: f64| (x / self.cell).floor() as i64;
        (f(b.min[0]), f(b.max[0]), f(b.min[2]), f(b.max[2]))
    }
    pub fn insert(&mut self, id: usize, b: &Aabb) {
        if b.is_empty() {
            return;
        }
        let (x0, x1, z0, z1) = self.range(b);
        // Cap huge boxes (a 10 km ground) to a sane number of cells.
        if (x1 - x0 + 1) * (z1 - z0 + 1) > 1_000_000 {
            self.map.entry((i64::MIN, i64::MIN)).or_default().push(id);
            return;
        }
        for x in x0..=x1 {
            for z in z0..=z1 {
                self.map.entry((x, z)).or_default().push(id);
            }
        }
    }
    /// Ids whose cells touch `b` (sorted, unique).
    pub fn query(&self, b: &Aabb) -> Vec<usize> {
        let mut out = Vec::new();
        if let Some(v) = self.map.get(&(i64::MIN, i64::MIN)) {
            out.extend_from_slice(v);
        }
        let (x0, x1, z0, z1) = self.range(b);
        if (x1 - x0 + 1) * (z1 - z0 + 1) > 4_000_000 {
            for v in self.map.values() {
                out.extend_from_slice(v);
            }
        } else {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    if let Some(v) = self.map.get(&(x, z)) {
                        out.extend_from_slice(v);
                    }
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }
}

/// The analysed scene.
#[derive(Debug, Clone)]
pub struct Scene {
    pub items: Vec<Item>,
    pub objects: Vec<Object>,
    pub grid: Grid,
    pub bounds: Aabb,
    pub cfg: Config,
    /// Instances left out because their mesh is unknown (not built in and not
    /// registered with [`Shapes`]): the mesh and how many instances. Never
    /// dropped silently: the runner reports them.
    pub skipped: Vec<(MeshId, usize)>,
}

/// Mesh kind name for labels.
pub fn mesh_name(id: MeshId) -> String {
    match id {
        MeshId::CUBE => "cube".into(),
        MeshId::SPHERE => "sphere".into(),
        MeshId::PLANE => "plane".into(),
        MeshId::BEVEL_CUBE => "bevel cube".into(),
        MeshId::CYLINDER => "cylinder".into(),
        MeshId(n) => format!("mesh {n}"),
    }
}

/// Default class label: the look material's name and the mesh.
pub fn default_label(inst: &MeshInstance, materials: &[String]) -> String {
    let mat = materials
        .get(inst.material as usize)
        .cloned()
        .unwrap_or_else(|| format!("material {}", inst.material));
    format!("{mat} ({})", mesh_name(inst.mesh))
}

impl Scene {
    /// Build from one frame's instances (all of them, in draw order).
    pub fn build(
        instances: &[(usize, MeshInstance)],
        shapes: &mut Shapes,
        label: &dyn Fn(&MeshInstance) -> String,
        cfg: Config,
    ) -> Scene {
        Scene::build_with(instances, shapes, label, cfg, &|_| false)
    }

    /// [`Scene::build`] where `is_decal(index)` says which instances were
    /// drawn with `draw_decal` (by their draw slot): those are decals
    /// whatever their thickness.
    pub fn build_with(
        instances: &[(usize, MeshInstance)],
        shapes: &mut Shapes,
        label: &dyn Fn(&MeshInstance) -> String,
        cfg: Config,
        is_decal: &dyn Fn(usize) -> bool,
    ) -> Scene {
        let mut items = Vec::with_capacity(instances.len());
        let mut skipped: Vec<(MeshId, usize)> = Vec::new();
        for (index, inst) in instances {
            let Some(shape) = shapes.shape(inst) else {
                match skipped.iter_mut().find(|s| s.0 == inst.mesh) {
                    Some(s) => s.1 += 1,
                    None => skipped.push((inst.mesh, 1)),
                }
                continue;
            };
            if shape.aabb.is_empty() {
                continue;
            }
            let decal = is_decal(*index);
            let kind = if decal {
                Kind::Decal
            } else if shape.aabb.max_extent() >= cfg.structure {
                Kind::Structure
            } else if shape.aabb.min_extent() <= cfg.decal && shape.aabb.max_extent() >= 3.0 * shape.aabb.min_extent().max(1e-3) {
                Kind::Decal
            } else {
                Kind::Part
            };
            items.push(Item {
                index: *index,
                inst: *inst,
                shape,
                kind,
                label: label(inst),
                object: 0,
                decal,
            });
        }
        // Union-find over the last `window` items in draw order: a part joins
        // a touching earlier part's group (the most recent first) when the
        // merged box stays under `max_object`. Seam-like pieces (same
        // cross-section, laid end to end: kerbs, rails, fence rails) never
        // join each other directly, only through a part that holds them (a
        // post). Two copies of one part (same mesh and size: two crates side
        // by side) are two props, so they never join directly either. Two
        // groups that already have several parts merge only when
        // the new part continues a run of the other group (a fence rail
        // touching the previous rail and post): two finished trees whose
        // canopies touch stay two objects, so their overlap is still checked.
        let n = items.len();
        let mut parent: Vec<usize> = (0..n).collect();
        let mut size = vec![1usize; n];
        let mut boxes: Vec<Aabb> = items.iter().map(|it| it.shape.aabb).collect();
        fn find(p: &mut [usize], mut x: usize) -> usize {
            while p[x] != x {
                p[x] = p[p[x]];
                x = p[x];
            }
            x
        }
        for i in 0..n {
            if items[i].kind != Kind::Part {
                continue;
            }
            let near: Vec<usize> = (i.saturating_sub(cfg.window)..i)
                .rev()
                .filter(|&j| items[j].kind == Kind::Part && items[j].shape.aabb.overlaps(&items[i].shape.aabb, cfg.touch))
                .collect();
            let runs: Vec<usize> = near.iter().copied().filter(|&j| seam_like(&items[i].inst, &items[j].inst)).collect();
            for &j in &near {
                if runs.contains(&j) || copies(&items[i].inst, &items[j].inst) {
                    continue;
                }
                let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                if ri == rj {
                    continue;
                }
                if size[ri] > 1 && size[rj] > 1 && !runs.iter().any(|&k| find(&mut parent, k) == rj) {
                    continue;
                }
                let union = boxes[ri].union(&boxes[rj]);
                if union.max_extent() > cfg.max_object {
                    continue;
                }
                // Keep the earlier item as the root so objects stay in draw order.
                let (root, child) = if rj < ri { (rj, ri) } else { (ri, rj) };
                parent[child] = root;
                size[root] += size[child];
                boxes[root] = union;
            }
        }
        let mut objects: Vec<Object> = Vec::new();
        let mut object_of: HashMap<usize, usize> = HashMap::new();
        for i in 0..n {
            let r = find(&mut parent, i);
            let o = *object_of.entry(r).or_insert_with(|| {
                objects.push(Object {
                    items: Vec::new(),
                    aabb: boxes[r],
                    kind: items[r].kind,
                    label: String::new(),
                });
                objects.len() - 1
            });
            objects[o].items.push(i);
            items[i].object = o;
        }
        for o in &mut objects {
            o.label = object_label(&items, &o.items);
        }
        let mut bounds = Aabb::EMPTY;
        let mut grid = Grid::new(8.0);
        for (i, it) in items.iter().enumerate() {
            grid.insert(i, &it.shape.aabb);
            bounds = bounds.union(&it.shape.aabb);
        }
        Scene {
            items,
            objects,
            grid,
            bounds,
            cfg,
            skipped,
        }
    }

    /// First hit of a ray against solid or surface items (`skip` returns true
    /// for items to ignore): (distance, item).
    pub fn ray(&self, o: V3, dir: V3, tmax: f64, skip: &dyn Fn(usize) -> bool) -> Option<(f64, usize)> {
        self.ray_with(o, dir, tmax, skip, false)
    }

    /// Like [`Scene::ray`] but against surfaces only: a ray that starts
    /// inside a solid finds the wall it leaves through (distance to the
    /// nearest face), not 0.
    pub fn ray_surface(&self, o: V3, dir: V3, tmax: f64, skip: &dyn Fn(usize) -> bool) -> Option<(f64, usize)> {
        self.ray_with(o, dir, tmax, skip, true)
    }

    fn ray_with(&self, o: V3, dir: V3, tmax: f64, skip: &dyn Fn(usize) -> bool, surface: bool) -> Option<(f64, usize)> {
        let end = add(o, mul(dir, tmax));
        let mut b = Aabb::from_points(&[o, end]);
        b = b.grow(1e-6);
        let mut best: Option<(f64, usize)> = None;
        for id in self.grid.query(&b) {
            if skip(id) {
                continue;
            }
            let it = &self.items[id];
            if !it.shape.aabb.overlaps(&b, 1e-6) {
                continue;
            }
            let max = best.map_or(tmax, |x| x.0);
            let hit = if surface { it.shape.ray_surface(o, dir, max) } else { it.shape.ray(o, dir, max) };
            if let Some(t) = hit
                && best.is_none_or(|x| t < x.0)
            {
                best = Some((t, id));
            }
        }
        best
    }

    /// Solid items containing `p` (deeper than `margin`).
    pub fn inside(&self, p: V3, margin: f64, skip: &dyn Fn(usize) -> bool) -> Option<usize> {
        let b = Aabb { min: p, max: p };
        self.grid
            .query(&b)
            .into_iter()
            .find(|&id| !skip(id) && self.items[id].shape.contains(p, margin))
    }

    /// The ground height under the world (lowest structure top, or the
    /// lowest point of anything).
    pub fn ground_y(&self) -> f64 {
        let s = self
            .items
            .iter()
            .filter(|i| i.kind == Kind::Structure)
            .map(|i| i.shape.aabb.max[1])
            .fold(f64::INFINITY, f64::min);
        if s.is_finite() { s } else { self.bounds.min[1] }
    }

    /// Where the ground is: the XZ box of the structures whose top is at
    /// the ground height (within 0.5 m), at that height. Sky seen past it is
    /// the world's edge (a separate check), not a hole. The whole world box
    /// when there are no structures.
    pub fn ground_footprint(&self) -> Aabb {
        let gy = self.ground_y();
        let mut out: Option<Aabb> = None;
        for i in self.items.iter().filter(|i| i.kind == Kind::Structure && (i.shape.aabb.max[1] - gy).abs() <= 0.5) {
            let b = i.shape.aabb;
            out = Some(match out {
                None => b,
                Some(o) => Aabb {
                    min: [o.min[0].min(b.min[0]), o.min[1].min(b.min[1]), o.min[2].min(b.min[2])],
                    max: [o.max[0].max(b.max[0]), o.max[1].max(b.max[1]), o.max[2].max(b.max[2])],
                },
            });
        }
        out.unwrap_or(self.bounds)
    }
}

/// Two pieces of one run laid end to end (kerb segments, rails, fence
/// rails): the same mesh, the same cross-section (two scale components equal
/// within 3%), long along the third axis (at least twice the larger of the
/// other two) and the long axes nearly parallel.
pub fn seam_like(a: &MeshInstance, b: &MeshInstance) -> bool {
    if a.mesh != b.mesh {
        return false;
    }
    let close = |x: f32, y: f32| (x - y).abs() <= 0.03 * x.abs().max(y.abs()).max(1e-6);
    for long in 0..3 {
        let (u, v) = ((long + 1) % 3, (long + 2) % 3);
        if !close(a.scale[u], b.scale[u]) || !close(a.scale[v], b.scale[v]) {
            continue;
        }
        let side = a.scale[u].abs().max(a.scale[v].abs()).max(b.scale[u].abs()).max(b.scale[v].abs());
        if a.scale[long].abs() < 2.0 * side || b.scale[long].abs() < 2.0 * side {
            continue;
        }
        let mut axis = [0.0; 3];
        axis[long] = 1.0;
        let da = rotate(quat(a.rotation), axis);
        let db = rotate(quat(b.rotation), axis);
        if dot(da, db).abs() >= 0.9 {
            return true;
        }
    }
    false
}

/// Two copies of one part: the same mesh at the same size (within 1%).
pub fn copies(a: &MeshInstance, b: &MeshInstance) -> bool {
    a.mesh == b.mesh && (0..3).all(|k| (a.scale[k] - b.scale[k]).abs() <= 0.01 * a.scale[k].abs().max(b.scale[k].abs()).max(1e-6))
}

/// "trunk (cylinder) + tree (sphere)": the distinct part labels, most used first.
pub fn object_label(items: &[Item], ids: &[usize]) -> String {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for &i in ids {
        match counts.iter_mut().find(|c| c.0 == items[i].label) {
            Some(c) => c.1 += 1,
            None => counts.push((items[i].label.clone(), 1)),
        }
    }
    counts.sort_by_key(|c| std::cmp::Reverse(c.1));
    let names: Vec<String> = counts.iter().take(3).map(|c| c.0.clone()).collect();
    let mut s = names.join(" + ");
    if counts.len() > 3 {
        s.push_str(" + ...");
    }
    s
}

/// Instance identity for the static test: mesh, transform and material
/// (colour may animate on a static object).
pub fn instance_key(m: &MeshInstance) -> [i64; 12] {
    let q = |x: f32, s: f32| (f64::from(x) * f64::from(s)).round() as i64;
    [
        i64::from(m.mesh.0),
        q(m.position[0], 1e3),
        q(m.position[1], 1e3),
        q(m.position[2], 1e3),
        q(m.rotation[0], 1e4),
        q(m.rotation[1], 1e4),
        q(m.rotation[2], 1e4),
        q(m.rotation[3], 1e4),
        q(m.scale[0], 1e3),
        q(m.scale[1], 1e3),
        q(m.scale[2], 1e3),
        i64::from(m.material),
    ]
}

/// Split frames into static instances (present, unchanged, in at least
/// `frac` of the frames; taken from the first frame, in draw order) and a
/// per-frame mask of which instances are dynamic.
pub fn split_static(frames: &[Vec<MeshInstance>], frac: f64) -> (Vec<(usize, MeshInstance)>, Vec<Vec<bool>>) {
    let mut seen: HashMap<[i64; 12], usize> = HashMap::new();
    for f in frames {
        let mut keys: Vec<[i64; 12]> = f.iter().map(instance_key).collect();
        keys.sort_unstable();
        keys.dedup();
        for k in keys {
            *seen.entry(k).or_default() += 1;
        }
    }
    let need = ((frames.len() as f64 * frac).ceil() as usize).max(1);
    let is_static = |m: &MeshInstance| seen.get(&instance_key(m)).copied().unwrap_or(0) >= need;
    let statics = frames
        .first()
        .map(|f| f.iter().enumerate().filter(|(_, m)| is_static(m)).map(|(i, m)| (i, *m)).collect())
        .unwrap_or_default();
    let dynamic = frames.iter().map(|f| f.iter().map(|m| !is_static(m)).collect()).collect();
    (statics, dynamic)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn inst(mesh: MeshId, p: [f32; 3], s: [f32; 3]) -> MeshInstance {
        let mut m = MeshInstance::new(mesh, p);
        m.scale = s;
        m
    }

    #[test]
    fn groups_runs_of_touching_parts() {
        let ground = inst(MeshId::CUBE, [0.0, -0.5, 0.0], [100.0, 1.0, 100.0]);
        let trunk = inst(MeshId::CYLINDER, [0.0, 1.0, 0.0], [0.4, 2.0, 0.4]);
        let canopy = inst(MeshId::SPHERE, [0.0, 2.5, 0.0], [2.0, 2.0, 2.0]);
        let trunk2 = inst(MeshId::CYLINDER, [1.5, 1.0, 0.0], [0.4, 2.0, 0.4]);
        let canopy2 = inst(MeshId::SPHERE, [1.5, 2.5, 0.0], [2.0, 2.0, 2.0]);
        let line = inst(MeshId::CUBE, [5.0, 0.01, 0.0], [0.2, 0.02, 4.0]);
        let list: Vec<(usize, MeshInstance)> = [ground, trunk, canopy, trunk2, canopy2, line].into_iter().enumerate().collect();
        let mut sh = Shapes::new();
        let s = Scene::build(&list, &mut sh, &|m| mesh_name(m.mesh), Config::default());
        assert_eq!(s.items[0].kind, Kind::Structure);
        assert_eq!(s.items[5].kind, Kind::Decal);
        // ground | trunk+canopy | trunk2+canopy2 | line: trunk2 doesn't
        // touch tree 1, so it starts a new run (and the canopies' overlap
        // stays visible to the overlap check).
        let objs: Vec<Vec<usize>> = s.objects.iter().map(|o| o.items.clone()).collect();
        assert_eq!(objs, vec![vec![0], vec![1, 2], vec![3, 4], vec![5]]);
        // A ray down from above the trunk hits the canopy first.
        let (t, id) = s.ray([0.0, 10.0, 0.0], [0.0, -1.0, 0.0], 100.0, &|_| false).unwrap();
        assert_eq!(id, 2);
        assert!((t - 6.5).abs() < 1e-6);
        assert!((s.ground_y() - 0.0).abs() < 1e-6);
    }

    #[test]
    fn a_fence_is_one_object_but_kerb_segments_are_not() {
        // Post, two rails to the next post, post, ...: rails overlap each
        // other at the posts (seam-like, never joined directly) but each
        // touches a post, so the whole fence is one object.
        let mut list = Vec::new();
        for j in 0..7 {
            let z = j as f32 * 1.1;
            list.push(inst(MeshId::BEVEL_CUBE, [0.0, 0.55, z], [0.2, 1.1, 0.2]));
            if j > 0 {
                for y in [0.35f32, 0.75] {
                    list.push(inst(MeshId::CUBE, [0.0, y, z - 0.55], [0.1, 0.1, 1.15]));
                }
            }
        }
        // Then two kerb segments laid end to end, 5 cm overlapping.
        list.push(inst(MeshId::CUBE, [20.0, 0.1, 0.0], [0.9, 0.2, 4.05]));
        list.push(inst(MeshId::CUBE, [20.0, 0.1, 4.0], [0.9, 0.2, 4.05]));
        let l: Vec<(usize, MeshInstance)> = list.into_iter().enumerate().collect();
        let s = Scene::build(&l, &mut Shapes::new(), &|m| mesh_name(m.mesh), Config::default());
        let n = l.len();
        assert_eq!(s.objects.len(), 3, "{:?}", s.objects.iter().map(|o| o.items.clone()).collect::<Vec<_>>());
        assert_eq!(s.objects[0].items, (0..n - 2).collect::<Vec<_>>());
        assert_ne!(s.items[n - 2].object, s.items[n - 1].object);
    }

    #[test]
    fn seam_like_needs_same_cross_section_long_and_parallel() {
        let a = inst(MeshId::CUBE, [0.0; 3], [0.9, 0.2, 4.0]);
        let b = inst(MeshId::CUBE, [0.0, 0.0, 4.0], [0.9, 0.2, 4.4]);
        assert!(seam_like(&a, &b));
        // A different cross-section, a short piece, a crossing piece, another mesh.
        assert!(!seam_like(&a, &inst(MeshId::CUBE, [0.0; 3], [0.5, 0.2, 4.0])));
        assert!(!seam_like(&a, &inst(MeshId::CUBE, [0.0; 3], [0.9, 0.2, 1.2])));
        let mut turned = b;
        turned.rotation = [0.0, std::f32::consts::FRAC_1_SQRT_2, 0.0, std::f32::consts::FRAC_1_SQRT_2];
        assert!(!seam_like(&a, &turned));
        assert!(!seam_like(&a, &inst(MeshId::BEVEL_CUBE, [0.0; 3], [0.9, 0.2, 4.0])));
        // Trees and gumdrops are never seam-like.
        let ball = inst(MeshId::SPHERE, [0.0; 3], [1.0; 3]);
        assert!(!seam_like(&ball, &ball));
    }

    #[test]
    fn far_apart_props_are_separate_objects() {
        let a = inst(MeshId::CUBE, [0.0, 0.5, 0.0], [1.0, 1.0, 1.0]);
        let b = inst(MeshId::CUBE, [5.0, 0.5, 0.0], [1.0, 1.0, 1.0]);
        let mut sh = Shapes::new();
        let s = Scene::build(&[(0, a), (1, b)], &mut sh, &|m| mesh_name(m.mesh), Config::default());
        assert_eq!(s.objects.len(), 2);
        // Two copies of a crate drawn back to back, overlapping: two props.
        let c = inst(MeshId::CUBE, [0.6, 0.5, 0.0], [1.0, 1.0, 1.0]);
        let s = Scene::build(&[(0, a), (1, c)], &mut sh, &|m| mesh_name(m.mesh), Config::default());
        assert_eq!(s.objects.len(), 2);
    }

    #[test]
    fn ground_footprint_is_the_ground_not_the_mountains() {
        // A 100 x 100 m meadow at y = 0, a mountain far outside it.
        let ground = inst(MeshId::CUBE, [0.0, -0.5, 0.0], [100.0, 1.0, 100.0]);
        let hill = inst(MeshId::SPHERE, [300.0, 0.0, 0.0], [60.0, 80.0, 60.0]);
        let mut sh = Shapes::new();
        let s = Scene::build(&[(0, ground), (1, hill)], &mut sh, &|m| mesh_name(m.mesh), Config::default());
        assert!((s.ground_y() - 0.0).abs() < 1e-6);
        let fp = s.ground_footprint();
        assert!((fp.min[0] + 50.0).abs() < 1e-6 && (fp.max[0] - 50.0).abs() < 1e-6, "{fp:?}");
        assert!(s.bounds.max[0] > 300.0);
    }

    #[test]
    fn statics_are_the_unchanged_instances() {
        let a = inst(MeshId::CUBE, [0.0; 3], [1.0; 3]);
        let mut frames = Vec::new();
        for i in 0..10 {
            let mover = inst(MeshId::SPHERE, [i as f32, 0.0, 0.0], [1.0; 3]);
            frames.push(vec![a, mover]);
        }
        let (st, dynamic) = split_static(&frames, 0.8);
        assert_eq!(st.len(), 1);
        assert_eq!(st[0].0, 0);
        assert!(dynamic.iter().all(|d| d == &vec![false, true]));
    }

    #[test]
    fn thresholds_from_json() {
        let mut c = Config::default();
        c.apply_json(&serde_json::json!({"sink": 0.2, "window": 4})).unwrap();
        assert_eq!((c.sink, c.window), (0.2, 4));
        assert!(c.apply_json(&serde_json::json!({"nope": 1})).is_err());
    }
}
