//! The geometry checks on drawn scenes (render-based ones are in `image`).
//!
//! Each returns issues with a location, a measure and a suggested fix.

use std::collections::HashMap;

use evani_render::mesh::MeshId;
use evani_render::world::MeshInstance;

use crate::geom::*;
use crate::image::{Camera, NEAR};
use crate::meshcheck;
use crate::report::{Check, Issue};
use crate::scene::{Config, Kind, Scene, mesh_name, seam_like};
use crate::shape::Shapes;

fn obj_label(s: &Scene, item: usize) -> String {
    s.objects[s.items[item].object].label.clone()
}

/// Volume of the intersection of two boxes (an upper bound of any overlap).
fn box_overlap_volume(a: &Aabb, b: &Aabb) -> f64 {
    let i = a.intersect(b);
    if i.is_empty() {
        return 0.0;
    }
    let e = i.extent();
    e[0].max(0.0) * e[1].max(0.0) * e[2].max(0.0)
}

// ---------------------------------------------------------------- overlaps

/// Static overlaps: pairs of solid items of different objects (or two
/// pieces of one run, see [`seam_like`]) whose shared volume is above
/// `overlap_volume`, unless the shared part is buried in a third solid. Sinking a part into a structure (a
/// post into the ground) up to `sink` deep is contact; decals are skipped.
pub fn overlaps(s: &Scene) -> Vec<Issue> {
    let c = &s.cfg;
    let mut out = Vec::new();
    for i in 0..s.items.len() {
        let a = &s.items[i];
        if a.kind == Kind::Decal || !a.shape.solid {
            continue;
        }
        for j in s.grid.query(&a.shape.aabb) {
            if j <= i {
                continue;
            }
            let b = &s.items[j];
            if b.kind == Kind::Decal || !b.shape.solid {
                continue;
            }
            // Parts of one object overlap by design, except two pieces of a
            // run (kerbs, rails) overlapping where they meet.
            if a.object == b.object && !(a.kind == Kind::Part && b.kind == Kind::Part && seam_like(&a.inst, &b.inst)) {
                continue;
            }
            if box_overlap_volume(&a.shape.aabb, &b.shape.aabb) <= c.overlap_volume {
                continue;
            }
            let Some((vol, bb)) = a.shape.overlap(&b.shape) else { continue };
            let depth = bb.min_extent();
            if vol <= c.overlap_volume {
                continue;
            }
            let structures = (a.kind == Kind::Structure) as u8 + (b.kind == Kind::Structure) as u8;
            if structures == 1 && depth <= c.sink {
                continue;
            }
            // Hidden inside a third solid (two fence rails meeting inside
            // their post): nobody can see it.
            let ctr = bb.center();
            let skip = |id: usize| id == i || id == j || s.items[id].kind == Kind::Decal;
            let buried = [-1.0, 1.0]
                .iter()
                .flat_map(|&x| [-1.0, 1.0].map(move |y| (x, y)))
                .flat_map(|(x, y)| [-1.0, 1.0].map(move |z| [x, y, z]))
                .map(|k: [f64; 3]| {
                    let e = bb.extent();
                    [ctr[0] + 0.4 * k[0] * e[0], ctr[1] + 0.4 * k[1] * e[1], ctr[2] + 0.4 * k[2] * e[2]]
                })
                .chain(std::iter::once(ctr))
                .all(|p| s.inside(p, 0.002, &skip).is_some());
            if buried {
                continue;
            }
            let seam = structures == 0 && seam_like(&a.inst, &b.inst);
            let (kind, fix) = match structures {
                0 if seam => (
                    "seam overlap",
                    "The run's pieces overlap where they meet: build it as one continuous mesh (evani_render::spline::sweep), or trim the pieces to meet edge to edge.",
                ),
                2 => (
                    "structure overlap",
                    "Build the run as one continuous mesh (evani_render::spline::sweep) instead of overlapping pieces, or trim the pieces to meet edge to edge.",
                ),
                1 => (
                    "sunk into structure",
                    "Place it with evani_render::place::snap_to_ground so it rests on the surface; tag it if it is meant to be buried (a rock).",
                ),
                _ => (
                    "prop overlap",
                    "Place props with evani_render::place::Footprints so they can't overlap, or move one apart.",
                ),
            };
            let mut is = Issue::new(Check::Overlap, kind, bb.center());
            is.objects = vec![obj_label(s, i), obj_label(s, j)];
            is.items = vec![a.index, b.index];
            is.aabb = Some(bb);
            is.measure = vol;
            is.unit = "m3".into();
            is.detail = format!("{} and {} share {vol:.3} m3, {depth:.2} m deep", a.label, b.label);
            is.fix = fix.into();
            out.push(is);
        }
    }
    out
}

// ---------------------------------------------------------------- floating

/// Floating objects: rays straight down from the base of each prop and
/// decal (structures are the ground). Grounded when any ray meets something
/// within `float_gap`; over nothing when no ray meets anything.
pub fn floating(s: &Scene) -> Vec<Issue> {
    let c = &s.cfg;
    const LIFT: f64 = 0.02;
    let mut out = Vec::new();
    for (oi, o) in s.objects.iter().enumerate() {
        // Decals have their own test (they are meant to lie on a surface, and
        // are judged by that, below).
        if o.kind == Kind::Structure || o.items.iter().all(|&i| s.items[i].decal) {
            continue;
        }
        let base = o.aabb.min[1];
        let mut samples = Vec::new();
        for &it in &o.items {
            let b = s.items[it].shape.aabb;
            if b.min[1] > base + 0.05 {
                continue;
            }
            let ctr = b.center();
            let e = b.extent();
            samples.push([ctr[0], ctr[2]]);
            for (dx, dz) in [(-0.3, -0.3), (0.3, -0.3), (-0.3, 0.3), (0.3, 0.3)] {
                samples.push([ctr[0] + dx * e[0], ctr[2] + dz * e[2]]);
            }
        }
        let own = |id: usize| s.items[id].object == oi;
        let mut best = f64::INFINITY;
        for p in &samples {
            let origin = [p[0], base + LIFT, p[1]];
            if let Some((t, _)) = s.ray(origin, [0.0, -1.0, 0.0], 1e4, &own) {
                best = best.min(t - LIFT);
            }
            if best <= c.float_gap {
                break;
            }
        }
        if best <= c.float_gap {
            continue;
        }
        let at = [o.aabb.center()[0], base, o.aabb.center()[2]];
        let mut is = if best.is_finite() {
            let mut is = Issue::new(Check::Floating, "floating", at);
            is.measure = best;
            is.unit = "m".into();
            is.detail = format!("its base is {best:.2} m above what is under it");
            is.fix = "Place it with evani_render::place::snap_to_ground (raycast placement); tag it if it flies on purpose.".into();
            is
        } else {
            let mut is = Issue::new(Check::Floating, "over nothing", at);
            is.measure = 0.0;
            is.unit = "m".into();
            is.detail = "nothing below it at all".into();
            is.fix = "Move it onto the world or extend the ground under it.".into();
            is
        };
        is.objects = vec![o.label.clone()];
        is.items = o.items.iter().map(|&i| s.items[i].index).collect();
        is.aabb = Some(o.aabb);
        out.push(is);
    }
    out.extend(decals_off_surface(s));
    out
}

/// Decals (drawn with `RenderWorld::draw_decal`) that don't lie on a surface.
/// A decal is a thin mark: it may sit on its surface, a little proud of it or
/// sunk into it, and the depth offset makes it show. From several points of
/// its mid-plane, rays go along its thin axis both ways; the nearest surface
/// (other than another decal) must be within `decal_gap` of the decal's faces
/// for at least one point, and the decal must not be buried more than its own
/// thickness inside a solid. Otherwise it floats above the surface, hangs
/// below it, or is hidden: "decal off surface".
pub fn decals_off_surface(s: &Scene) -> Vec<Issue> {
    let c = &s.cfg;
    let skip = |id: usize| s.items[id].decal;
    let mut out = Vec::new();
    for it in s.items.iter().filter(|i| i.decal) {
        let sh = &it.shape;
        // The thin axis: the normal of the decal's largest face; the
        // mid-plane lies halfway between its extreme faces along it.
        let Some(big) = sh.faces.iter().max_by(|a, b| a.area().total_cmp(&b.area())) else { continue };
        let n = big.n;
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for f in &sh.faces {
            for p in &f.pts {
                let h = dot(n, *p);
                lo = lo.min(h);
                hi = hi.max(h);
            }
        }
        let half = 0.5 * (hi - lo);
        let mid = 0.5 * (hi + lo);
        let ctr = big.centroid();
        let mut samples = vec![ctr];
        for p in big.pts.iter().take(8) {
            samples.push(lerp(ctr, *p, 0.6));
        }
        let mut best = f64::INFINITY; // smallest gap between the decal's face and a surface
        let mut buried = false;
        for p in &samples {
            let o = add(*p, mul(n, mid - dot(n, *p)));
            for dir in [n, mul(n, -1.0)] {
                if let Some((t, _)) = s.ray_surface(o, dir, 1e4, &skip) {
                    best = best.min((t - half).max(0.0));
                }
            }
            buried |= s.inside(o, half + c.decal_gap, &skip).is_some();
        }
        if best <= c.decal_gap && !buried {
            continue;
        }
        let at = it.shape.aabb.center();
        let (kind, detail, measure) = if buried {
            ("decal off surface", format!("{} is buried more than {:.0} cm inside a solid: it can't be seen", it.label, (half + c.decal_gap) * 100.0), half + c.decal_gap)
        } else if best.is_finite() {
            ("decal off surface", format!("{} is {best:.2} m from the nearest surface (a decal must lie on one, within {:.0} cm)", it.label, c.decal_gap * 100.0), best)
        } else {
            ("decal off surface", format!("{} has no surface within reach: it lies over nothing", it.label), 0.0)
        };
        let mut is = Issue::new(Check::Floating, kind, at);
        is.objects = vec![it.label.clone()];
        is.items = vec![it.index];
        is.aabb = Some(it.shape.aabb);
        is.measure = measure;
        is.unit = "m".into();
        is.detail = detail;
        is.fix = "Put the decal on the surface it marks (its height from evani_render::place::ground_height, or the surface's top), or draw it as part of that surface; tag it if it is meant to hover.".into();
        out.push(is);
    }
    out
}

// ---------------------------------------------------------------- meshes

/// Mesh validation of every mesh a scene uses (`used`: mesh and an example
/// position). `intended_open` says which meshes are open on purpose
/// (PLANE, road ribbons).
pub fn meshes(shapes: &Shapes, used: &[(MeshId, V3)], intended_open: &dyn Fn(MeshId) -> bool) -> Vec<Issue> {
    let mut out = Vec::new();
    for (id, at) in used {
        let Some(m) = shapes.mesh(*id) else { continue };
        let r = meshcheck::validate(&m, intended_open(*id));
        for p in r.problems() {
            let mut is = Issue::new(Check::Mesh, p.split_once(' ').map_or(p.as_str(), |x| x.1), *at);
            is.objects = vec![format!("{} mesh", mesh_name(*id))];
            is.measure = p.split(' ').next().and_then(|n| n.parse().ok()).unwrap_or(1.0);
            is.unit = "triangles/edges".into();
            is.detail = format!("{} of {} triangles: {p}", mesh_name(*id), r.triangles);
            is.fix = "Fix the mesh generator, importer or converter: drop zero-area and duplicate triangles, weld and close the surface, wind every triangle outward (counter-clockwise from outside).".into();
            out.push(is);
        }
    }
    out
}

/// Distinct meshes of an instance list with an example position.
pub fn used_meshes(list: &[MeshInstance]) -> Vec<(MeshId, V3)> {
    let mut seen: Vec<(MeshId, V3)> = Vec::new();
    for m in list {
        if !seen.iter().any(|s| s.0 == m.mesh) {
            seen.push((m.mesh, v3(m.position)));
        }
    }
    seen
}

// ---------------------------------------------------------------- camera

/// The near-plane corners, edge midpoints and centre, plus the eye.
pub fn near_points(cam: &Camera) -> Vec<V3> {
    let (f, s, u) = cam.basis();
    let hh = NEAR * (cam.fov_y_deg.to_radians() * 0.5).tan();
    let hw = hh * cam.aspect;
    let c = add(cam.eye, mul(f, NEAR));
    let mut p = vec![cam.eye];
    for a in [-1.0, 0.0, 1.0] {
        for b in [-1.0, 0.0, 1.0] {
            p.push(add(c, add(mul(s, a * hw), mul(u, b * hh))));
        }
    }
    p
}

/// Camera clipping: frames (time, camera, scenes to test: the statics and
/// that frame's movers) where the eye or the near plane is inside solid
/// geometry. Consecutive frames in the same object are one issue.
pub fn camera_clip(frames: &[(f64, Camera, Vec<&Scene>)]) -> Vec<Issue> {
    let mut out: Vec<Issue> = Vec::new();
    let mut last: Option<(String, f64)> = None;
    for (t, cam, scenes) in frames {
        let pts = near_points(cam);
        let mut hit = None;
        'find: for sc in scenes {
            for p in &pts {
                if let Some(id) = sc.inside(*p, 0.0, &|i| sc.items[i].kind == Kind::Decal) {
                    hit = Some((obj_label(sc, id), sc.items[id].index, *p));
                    break 'find;
                }
            }
        }
        let Some((label, index, p)) = hit else {
            last = None;
            continue;
        };
        if let (Some((l, _)), Some(prev)) = (&last, out.last_mut())
            && *l == label
        {
            prev.measure += 1.0;
            prev.detail = format!("the near plane is inside it for {} sampled frames from t={:.2}s", prev.measure, prev.time.unwrap_or(0.0));
            last = Some((label, *t));
            continue;
        }
        let mut is = Issue::new(Check::Camera, "near plane inside geometry", p);
        is.objects = vec![label.clone()];
        is.items = vec![index];
        is.aabb = Some(Aabb::from_points(&[p]).grow(0.5));
        is.measure = 1.0;
        is.unit = "frames".into();
        is.time = Some(*t);
        is.detail = "the near plane is inside it for 1 sampled frame".into();
        is.fix = "Add camera collision: sweep from the target to the eye and stop before hits (or fade the blocker); keep the eye out of solid props.".into();
        out.push(is);
        last = Some((label, *t));
    }
    out
}

// ---------------------------------------------------------------- moving

/// One sampled frame of a play run.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub time: f64,
    pub meshes: Vec<MeshInstance>,
    /// Index in `meshes` where the decals (`RenderWorld::decals`) start.
    pub decal_from: usize,
    pub camera: Camera,
}

/// What a drawn part is, for the character-in-vehicle test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Other,
    Character,
    Vehicle,
}

struct Track {
    centroid: V3,
    label: String,
    seen: usize,
}

/// Moving clipping over sampled frames. `dynamic[f][i]`: instance `i` of
/// frame `f` moves (from `scene::split_static`). Finds deep penetrations
/// into static geometry, tunnelling (a mover's centre crossing a solid
/// between samples), leaving the world, and characters clipping through
/// their vehicles. Each (mover, kind, other) reports its worst frame once.
pub fn moving(
    stat: &Scene,
    frames: &[Snapshot],
    dynamic: &[Vec<bool>],
    shapes: &mut Shapes,
    label: &dyn Fn(&MeshInstance) -> String,
    role: &dyn Fn(&MeshInstance) -> Role,
) -> Vec<Issue> {
    let c: Config = stat.cfg.clone();
    let mut tracks: Vec<Track> = Vec::new();
    let mut found: HashMap<(usize, &'static str, String), (Issue, usize)> = HashMap::new();
    let world = stat.bounds.grow(2.0);
    let floor = stat.bounds.min[1] - 1.0;
    let mut record = |key: (usize, &'static str, String), is: Issue| {
        let e = found.entry(key).or_insert((is.clone(), 0));
        e.1 += 1;
        if is.measure > e.0.measure {
            e.0 = is;
        }
    };
    for (fi, f) in frames.iter().enumerate() {
        let list: Vec<(usize, MeshInstance)> = f
            .meshes
            .iter()
            .enumerate()
            .filter(|(i, _)| dynamic.get(fi).and_then(|d| d.get(*i)).copied().unwrap_or(true))
            .map(|(i, m)| (i, *m))
            .collect();
        let dy = Scene::build_with(&list, shapes, label, c.clone(), &|i| i >= f.decal_from);
        // Match objects to tracks (greedy nearest centroid).
        let mut used = vec![false; tracks.len()];
        let mut obj_track = vec![usize::MAX; dy.objects.len()];
        for (oi, o) in dy.objects.iter().enumerate() {
            if o.kind == Kind::Decal {
                continue;
            }
            let ctr = o.aabb.center();
            let best = tracks
                .iter()
                .enumerate()
                .filter(|(ti, t)| !used[*ti] && t.label == o.label)
                .map(|(ti, t)| (ti, dist(t.centroid, ctr)))
                .filter(|x| x.1 < 8.0)
                .min_by(|a, b| a.1.total_cmp(&b.1));
            let ti = match best {
                Some((ti, _)) => {
                    used[ti] = true;
                    ti
                }
                None => {
                    tracks.push(Track {
                        centroid: ctr,
                        label: o.label.clone(),
                        seen: 0,
                    });
                    used.push(true);
                    tracks.len() - 1
                }
            };
            obj_track[oi] = ti;
            let prev = tracks[ti].centroid;
            let had_prev = tracks[ti].seen > 0;
            tracks[ti].centroid = ctr;
            tracks[ti].seen += 1;
            // Leaving the world.
            if !world.contains([ctr[0], world.center()[1], ctr[2]], 0.0) || ctr[1] < floor {
                let mut is = Issue::new(Check::Moving, "left the world", ctr);
                is.objects = vec![o.label.clone()];
                is.measure = 1.0;
                is.unit = "".into();
                is.time = Some(f.time);
                is.detail = "its centre is outside the world's bounds".into();
                is.fix = "Clamp movers to the world (or respawn them when they fall below the ground).".into();
                record((ti, "left", String::new()), is);
            }
            // Tunnelling between samples.
            if had_prev {
                let d = sub(ctr, prev);
                let l = len(d);
                let solid_static = |id: usize| stat.items[id].kind == Kind::Decal || !stat.items[id].shape.solid;
                if l > 1e-3
                    && stat.inside(prev, 0.0, &solid_static).is_none()
                    && stat.inside(ctr, 0.0, &solid_static).is_none()
                    && let Some((t, id)) = stat.ray(prev, norm(d), l, &solid_static)
                    && t > 1e-6
                {
                    let at = add(prev, mul(norm(d), t));
                    let mut is = Issue::new(Check::Moving, "tunnelling", at);
                    is.objects = vec![o.label.clone(), obj_label(stat, id)];
                    is.items = vec![stat.items[id].index];
                    is.aabb = Some(Aabb::from_points(&[prev, ctr]).grow(0.5));
                    is.measure = l;
                    is.unit = "m step".into();
                    is.time = Some(f.time);
                    is.detail = format!("moved {l:.2} m in one sample, through {}", stat.items[id].label);
                    is.fix = "Sub-step the movement or sweep the collider (continuous collision) so a mover can't cross a wall between frames.".into();
                    record((ti, "tunnel", obj_label(stat, id)), is);
                }
            }
            // Deep penetration into static geometry.
            for &it in &o.items {
                let a = &dy.items[it];
                if !a.shape.solid {
                    continue;
                }
                for j in stat.grid.query(&a.shape.aabb) {
                    let b = &stat.items[j];
                    if b.kind == Kind::Decal || !b.shape.solid {
                        continue;
                    }
                    if box_overlap_volume(&a.shape.aabb, &b.shape.aabb) <= c.penetration_volume.min(1e-4) {
                        continue;
                    }
                    let Some((vol, bb)) = a.shape.overlap(&b.shape) else { continue };
                    let depth = bb.min_extent();
                    if depth <= c.penetration && vol <= c.penetration_volume {
                        continue;
                    }
                    let mut is = Issue::new(Check::Moving, "penetration", bb.center());
                    is.objects = vec![o.label.clone(), obj_label(stat, j)];
                    is.items = vec![b.index];
                    is.aabb = Some(bb);
                    is.measure = depth;
                    is.unit = "m deep".into();
                    is.time = Some(f.time);
                    is.detail = format!("{} goes {depth:.2} m ({vol:.3} m3) into {}", a.label, b.label);
                    is.fix = "Resolve collisions against this object (push the mover out) or stop it at the surface.".into();
                    record((ti, "pen", obj_label(stat, j)), is);
                }
            }
        }
        // Characters clipping through vehicles (any pair this frame).
        for i in 0..dy.items.len() {
            let a = &dy.items[i];
            if role(&a.inst) != Role::Character || !a.shape.solid {
                continue;
            }
            for j in dy.grid.query(&a.shape.aabb) {
                let b = &dy.items[j];
                if role(&b.inst) != Role::Vehicle || !b.shape.solid {
                    continue;
                }
                let Some((vol, bb)) = a.shape.overlap(&b.shape) else { continue };
                if vol <= c.penetration_volume {
                    continue;
                }
                let ti = obj_track.get(a.object).copied().unwrap_or(usize::MAX);
                let mut is = Issue::new(Check::Moving, "character in vehicle", bb.center());
                is.objects = vec![a.label.clone(), b.label.clone()];
                is.aabb = Some(bb);
                is.measure = vol;
                is.unit = "m3".into();
                is.time = Some(f.time);
                is.detail = format!("{} passes {vol:.3} m3 into {}", a.label, b.label);
                is.fix = "Seat the character inside the vehicle's cockpit: lower or shrink it, or open the body around it.".into();
                record((ti, "charveh", b.label.clone()), is);
            }
        }
    }
    let mut out: Vec<Issue> = found
        .into_values()
        .map(|(mut is, n)| {
            if n > 1 {
                is.detail.push_str(&format!(" (worst of {n} sampled frames)"));
            }
            is
        })
        .collect();
    out.sort_by(|a, b| a.kind.cmp(&b.kind).then(a.time.unwrap_or(0.0).total_cmp(&b.time.unwrap_or(0.0))));
    out
}

// ---------------------------------------------------------------- collision

/// A sideways probe from a mover's path towards where a wall should be.
#[derive(Debug, Clone)]
pub struct WallProbe {
    pub origin: V3,
    /// Unit direction.
    pub dir: V3,
    /// How far to look.
    pub reach: f64,
    /// Distance from `origin` at which the game's collision stops the
    /// mover's visible surface (`None`: nothing stops it within `reach`).
    pub collision: Option<f64>,
    /// Groups probes into runs ("left", "right").
    pub side: String,
    /// Distance along the path (m).
    pub station: f64,
}

/// A sample of a bot run along walls.
#[derive(Debug, Clone)]
pub struct BotSample {
    pub time: f64,
    pub pos: V3,
    /// The mover's visible half-width.
    pub radius: f64,
    /// The game's collision stopped or deflected it this step.
    pub blocked: bool,
}

/// Collision mismatch: the game's collision against the visible geometry.
/// "missing wall": something solid is visible but collision lets the mover
/// into or through it; "invisible wall": collision stops the mover where
/// nothing is visible. Consecutive probes of the same side and kind merge.
pub fn collision(s: &Scene, probes: &[WallProbe], bot: &[BotSample]) -> Vec<Issue> {
    let tol = s.cfg.collision_tol;
    let skip = |id: usize| s.items[id].kind == Kind::Decal;
    let mut out: Vec<Issue> = Vec::new();
    // (side, kind, first station, last station) of the open run in `out`.
    let mut open: Option<(String, &'static str, f64, f64)> = None;
    let spacing = probes
        .windows(2)
        .map(|w| (w[1].station - w[0].station).abs())
        .filter(|d| *d > 0.0)
        .fold(f64::INFINITY, f64::min);
    let join = if spacing.is_finite() { spacing * 2.5 } else { 5.0 };
    let mut sorted: Vec<&WallProbe> = probes.iter().collect();
    sorted.sort_by(|a, b| a.side.cmp(&b.side).then(a.station.total_cmp(&b.station)));
    for p in sorted {
        let vis = s.ray(p.origin, p.dir, p.reach, &skip).map(|h| h.0);
        let verdict: Option<(&'static str, f64, V3)> = match (vis, p.collision) {
            (Some(v), None) => Some(("missing wall", p.reach - v, add(p.origin, mul(p.dir, v)))),
            (Some(v), Some(c)) if c > v + tol => Some(("mover enters the wall", c - v, add(p.origin, mul(p.dir, v)))),
            (Some(v), Some(c)) if c < v - tol => Some(("invisible wall", v - c, add(p.origin, mul(p.dir, c)))),
            (None, Some(c)) => Some(("invisible wall", p.reach - c, add(p.origin, mul(p.dir, c)))),
            _ => None,
        };
        let Some((kind, m, at)) = verdict else {
            open = None;
            continue;
        };
        if let (Some((side, k, first, last)), Some(prev)) = (&open, out.last_mut())
            && *side == p.side
            && *k == kind
            && (p.station - last).abs() <= join
        {
            let first = *first;
            prev.aabb = Some(prev.aabb.unwrap_or(Aabb::EMPTY).union(&Aabb::from_points(&[at]).grow(0.5)));
            prev.detail = format!("{} side, from {first:.0} m to {:.0} m along the path", p.side, p.station);
            if m > prev.measure {
                prev.measure = m;
            }
            open = Some((p.side.clone(), kind, first, p.station));
            continue;
        }
        let mut is = Issue::new(Check::Collision, kind, at);
        is.objects = vec![match vis.and_then(|_| s.ray(p.origin, p.dir, p.reach, &skip)) {
            Some((_, id)) => obj_label(s, id),
            None => "(nothing drawn)".into(),
        }];
        is.aabb = Some(Aabb::from_points(&[at]).grow(0.5));
        is.measure = m;
        is.unit = "m".into();
        is.detail = format!("{} side, at {:.0} m along the path", p.side, p.station);
        is.fix = match kind {
            "missing wall" => "Add collision for the visible wall: movers pass through what the player sees.".into(),
            "mover enters the wall" => format!(
                "Move the collision boundary in by {m:.2} m (or the wall out) so the mover stops at the visible surface."
            ),
            _ => "Draw a wall where the collision stops movers, or move the collision out to the visible edge.".into(),
        };
        out.push(is);
        open = Some((p.side.clone(), kind, p.station, p.station));
    }
    // Bot samples: inside something solid (not the ground it drives on), or
    // blocked with nothing visible around it.
    let mut last_kind: Option<&'static str> = None;
    for b in bot {
        let inside = s.inside(b.pos, 0.0, &|id| s.items[id].kind == Kind::Decal);
        let verdict = if let Some(id) = inside {
            Some(("bot inside a wall", obj_label(s, id)))
        } else if b.blocked {
            let near = (0..8).any(|k| {
                let a = f64::from(k) * std::f64::consts::FRAC_PI_4;
                s.ray(b.pos, [a.cos(), 0.0, a.sin()], b.radius + tol, &skip).is_some()
            });
            (!near).then(|| ("bot blocked by nothing", "(nothing drawn)".to_string()))
        } else {
            None
        };
        let Some((kind, label)) = verdict else {
            last_kind = None;
            continue;
        };
        if last_kind == Some(kind)
            && let Some(prev) = out.last_mut()
        {
            prev.measure += 1.0;
            continue;
        }
        let mut is = Issue::new(Check::Collision, kind, b.pos);
        is.objects = vec![label];
        is.aabb = Some(Aabb::from_points(&[b.pos]).grow(b.radius + 0.5));
        is.measure = 1.0;
        is.unit = "samples".into();
        is.time = Some(b.time);
        is.detail = format!("wall-hugging bot at t={:.2}s", b.time);
        is.fix = if kind == "bot inside a wall" {
            "Add collision for this wall: the bot drove into it.".into()
        } else {
            "Draw the wall that stops the bot here, or remove the invisible collision.".into()
        };
        out.push(is);
        last_kind = Some(kind);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::mesh_name;

    fn inst(mesh: MeshId, p: [f32; 3], s: [f32; 3]) -> MeshInstance {
        let mut m = MeshInstance::new(mesh, p);
        m.scale = s;
        m
    }

    fn scene(list: &[MeshInstance]) -> Scene {
        let l: Vec<(usize, MeshInstance)> = list.iter().copied().enumerate().collect();
        Scene::build(&l, &mut Shapes::new(), &|m| mesh_name(m.mesh), Config::default())
    }

    fn ground() -> MeshInstance {
        inst(MeshId::CUBE, [0.0, -0.5, 0.0], [100.0, 1.0, 100.0])
    }

    #[test]
    fn overlap_finds_props_but_allows_shallow_sinking() {
        let a = inst(MeshId::CUBE, [0.0, 0.45, 0.0], [1.0, 1.0, 1.0]); // sunk 5 cm: fine
        // A window's worth of spacers in draw order keeps b and c in
        // separate objects (drawn back to back, touching, they would group).
        let gap = |x: f32| inst(MeshId::SPHERE, [x, 0.5, 20.0], [0.2, 0.2, 0.2]);
        let b = inst(MeshId::CUBE, [5.0, 0.5, 0.0], [1.0, 1.0, 1.0]);
        let c = inst(MeshId::CUBE, [5.6, 0.5, 0.0], [1.0, 1.0, 1.0]); // 0.4 m into b
        let deep = inst(MeshId::CUBE, [-5.0, 0.0, 0.0], [1.0, 1.0, 1.0]); // half buried
        let mut list = vec![ground(), a, gap(0.0), b];
        list.extend((0..Config::default().window).map(|k| gap(10.0 + k as f32)));
        list.extend([c, gap(40.0), deep]);
        let s = scene(&list);
        let o = overlaps(&s);
        let kinds: Vec<&str> = o.iter().map(|i| i.kind.as_str()).collect();
        assert!(kinds.contains(&"prop overlap"), "{kinds:?}");
        assert!(kinds.contains(&"sunk into structure"));
        assert_eq!(o.len(), 2, "{o:?}");
        let p = o.iter().find(|i| i.kind == "prop overlap").unwrap();
        assert!((p.measure - 0.4).abs() < 1e-5, "{}", p.measure);
    }

    #[test]
    fn run_pieces_overlapping_at_the_seam_are_seam_overlaps() {
        // Two kerb segments overlapping 0.3 m end to end, sitting on the ground.
        let k1 = inst(MeshId::CUBE, [0.0, 0.1, 0.0], [0.9, 0.2, 4.3]);
        let k2 = inst(MeshId::CUBE, [0.0, 0.1, 4.0], [0.9, 0.2, 4.3]);
        let o = overlaps(&scene(&[ground(), k1, k2]));
        assert_eq!(o.len(), 1, "{o:?}");
        assert_eq!(o[0].kind, "seam overlap");
        assert!(o[0].fix.contains("spline::sweep"));
        assert!((o[0].measure - 0.9 * 0.2 * 0.3).abs() < 1e-5, "{}", o[0].measure);
        // Two fence rails overlapping 0.15 m inside the post between them: hidden.
        let post = inst(MeshId::BEVEL_CUBE, [5.0, 0.6, 2.0], [0.4, 1.2, 0.4]);
        let r1 = inst(MeshId::CUBE, [5.0, 0.6, 1.075], [0.15, 0.15, 2.0]);
        let r2 = inst(MeshId::CUBE, [5.0, 0.6, 2.925], [0.15, 0.15, 2.0]);
        let o = overlaps(&scene(&[ground(), post, r1, r2]));
        assert!(o.is_empty(), "{o:?}");
    }

    #[test]
    fn floating_and_over_nothing() {
        let on = inst(MeshId::CUBE, [0.0, 0.5, 0.0], [1.0, 1.0, 1.0]);
        let up = inst(MeshId::CUBE, [10.0, 1.0, 0.0], [1.0, 1.0, 1.0]); // 0.5 m up
        let out = inst(MeshId::CUBE, [200.0, 0.5, 0.0], [1.0, 1.0, 1.0]);
        let s = scene(&[ground(), on, up, out]);
        let f = floating(&s);
        assert_eq!(f.len(), 2, "{f:?}");
        let fl = f.iter().find(|i| i.kind == "floating").unwrap();
        assert!((fl.measure - 0.5).abs() < 1e-6);
        assert!(f.iter().any(|i| i.kind == "over nothing"));
    }

    #[test]
    fn mesh_check_allows_plane_borders_only_when_intended() {
        let sh = Shapes::new();
        let used = vec![(MeshId::PLANE, [0.0; 3]), (MeshId::CUBE, [0.0; 3])];
        assert!(meshes(&sh, &used, &|id| id == MeshId::PLANE).is_empty());
        assert!(!meshes(&sh, &[(MeshId::PLANE, [0.0; 3])], &|_| false).is_empty());
    }

    #[test]
    fn camera_inside_a_wall_is_flagged_once_per_run() {
        let wall = inst(MeshId::CUBE, [0.0, 1.0, 0.0], [4.0, 2.0, 0.5]);
        let s = scene(&[ground(), wall]);
        let cam = |z: f64| Camera {
            eye: [0.0, 1.0, z],
            target: [0.0, 1.0, z - 5.0],
            up: [0.0, 1.0, 0.0],
            fov_y_deg: 60.0,
            aspect: 1.6,
        };
        let frames = vec![
            (0.0, cam(3.0), vec![&s]),
            (0.1, cam(0.1), vec![&s]),
            (0.2, cam(0.0), vec![&s]),
            (0.3, cam(-3.0), vec![&s]),
        ];
        let c = camera_clip(&frames);
        assert_eq!(c.len(), 1, "{c:?}");
        assert_eq!(c[0].measure, 2.0);
    }

    #[test]
    fn moving_finds_tunnelling_penetration_leaving_and_driver_clipping() {
        let wall = inst(MeshId::CUBE, [0.0, 1.0, 0.0], [10.0, 2.0, 0.3]);
        let s = scene(&[ground(), wall]);
        let mut frames = Vec::new();
        let cam = Camera {
            eye: [0.0, 5.0, 10.0],
            target: [0.0; 3],
            up: [0.0, 1.0, 0.0],
            fov_y_deg: 60.0,
            aspect: 1.6,
        };
        // A box moving -z through the wall in one big step, then deep in it.
        for (k, z) in [3.0f32, 2.0, -2.0, 0.0, 300.0].iter().enumerate() {
            let mut body = inst(MeshId::CUBE, [0.0, 0.6, *z], [1.0, 1.0, 1.0]);
            body.material = 1;
            let mut driver = inst(MeshId::SPHERE, [0.0, 0.8, *z], [0.5, 0.5, 0.5]);
            driver.material = 2;
            frames.push(Snapshot {
                time: k as f64,
                meshes: vec![ground(), wall, body, driver],
                decal_from: usize::MAX,
                camera: cam,
            });
        }
        let lists: Vec<Vec<MeshInstance>> = frames.iter().map(|f| f.meshes.clone()).collect();
        let (_, dynamic) = crate::scene::split_static(&lists, 0.8);
        let role = |m: &MeshInstance| match m.material {
            1 => Role::Vehicle,
            2 => Role::Character,
            _ => Role::Other,
        };
        let mut sh = Shapes::new();
        let m = moving(&s, &frames, &dynamic, &mut sh, &|m| format!("m{}", m.material), &role);
        let kinds: Vec<&str> = m.iter().map(|i| i.kind.as_str()).collect();
        for k in ["tunnelling", "penetration", "character in vehicle"] {
            assert!(kinds.contains(&k), "{k} missing in {kinds:?}");
        }
        // The jump to z = 300 is a new track (too far to match) outside the world.
        assert!(kinds.contains(&"left the world"), "{kinds:?}");
    }

    #[test]
    fn collision_probes_find_missing_and_invisible_walls() {
        // A rail on the right from z=0..-20 only.
        let rail = inst(MeshId::CUBE, [5.0, 0.5, -10.0], [0.5, 1.0, 20.0]);
        let s = scene(&[ground(), rail]);
        let mut probes = Vec::new();
        for k in 0..20 {
            let z = -2.0 * k as f64;
            // Collision: the game stops at 3 m on the right everywhere.
            probes.push(WallProbe {
                origin: [0.0, 0.5, z],
                dir: [1.0, 0.0, 0.0],
                reach: 10.0,
                collision: Some(4.75),
                side: "right".into(),
                station: -z,
            });
        }
        let c = collision(&s, &probes, &[]);
        // Where the rail is (z 0..-20) collision matches; beyond it the
        // collision stops movers at nothing: one merged run.
        assert_eq!(c.len(), 1, "{c:?}");
        assert_eq!(c[0].kind, "invisible wall");
        // No collision at all next to the rail: missing wall.
        for p in &mut probes {
            p.collision = None;
        }
        let c = collision(&s, &probes, &[]);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].kind, "missing wall");
        // A bot inside the rail.
        let bot = vec![BotSample {
            time: 1.0,
            pos: [5.0, 0.5, -5.0],
            radius: 0.8,
            blocked: false,
        }];
        assert_eq!(collision(&s, &[], &bot)[0].kind, "bot inside a wall");
    }

    // ---- custom meshes and decals

    use evani_render::mesh::{Mesh, MeshLibrary};
    use evani_render::spline::{Path, Profile, sweep};

    /// A swept mesh along `points` (world space: drawn with an identity instance).
    fn swept(points: Vec<[f32; 3]>, closed: bool, profile: &Profile) -> Mesh {
        let path = Path::new(points, closed);
        sweep(&path.frames(2.0), profile, closed)
    }

    /// Register `m` in a fresh library and shapes; returns its id.
    fn register(shapes: &mut Shapes, m: Mesh) -> MeshId {
        let mut lib = MeshLibrary::default();
        let id = lib.add(m).expect("room in the library");
        shapes.add_library(&lib);
        id
    }

    /// A scene whose instances from `decal_from` on are decals.
    fn scene_of(list: &[MeshInstance], shapes: &mut Shapes, decal_from: usize) -> Scene {
        let l: Vec<(usize, MeshInstance)> = list.iter().copied().enumerate().collect();
        Scene::build_with(&l, shapes, &|m| mesh_name(m.mesh), Config::default(), &|i| i >= decal_from)
    }

    fn at_origin(mesh: MeshId) -> MeshInstance {
        MeshInstance::new(mesh, [0.0; 3])
    }

    #[test]
    fn a_custom_swept_slab_is_registered_and_overlaps_a_cube() {
        let mut sh = Shapes::new();
        let road = register(&mut sh, swept(vec![[0.0, 0.0, 0.0], [20.0, 0.0, 0.0]], false, &Profile::slab(3.0, 0.3)));
        // The road is 20 x 6 x 0.3 m = 36 m3 (a straight one is a single convex box).
        let shape = sh.shape(&at_origin(road)).expect("the registered mesh has a shape");
        assert!((shape.volume() - 36.0).abs() < 1e-3, "{}", shape.volume());
        // A 1 m cube at the road's middle, half sunk (y -0.5..0.5): 0.3 m of it is in the slab.
        let cube = inst(MeshId::CUBE, [10.0, 0.0, 0.0], [1.0, 1.0, 1.0]);
        let cube_shape = sh.shape(&cube).unwrap();
        let (v, _) = shape.overlap(&cube_shape).expect("the cube is inside the slab");
        assert!((v - 0.3).abs() < 1e-4, "{v}");
        let s = scene_of(&[at_origin(road), cube], &mut sh, usize::MAX);
        assert!(s.skipped.is_empty());
        assert_eq!(s.items.len(), 2, "both instances are in the scene");
        let o = overlaps(&s);
        assert!(!o.is_empty(), "the cube in the road is found");
        // A bent road (a closed loop: concave, split into convex segments) gives the same answer.
        let arc: Vec<[f32; 3]> = (0..12)
            .map(|i| {
                let a = i as f32 / 12.0 * core::f32::consts::TAU;
                [40.0 * a.cos(), 0.0, 40.0 * a.sin()]
            })
            .collect();
        let mut sh2 = Shapes::new();
        let ring = register(&mut sh2, swept(arc, true, &Profile::slab(7.0, 0.3)));
        let ring_shape = sh2.shape(&at_origin(ring)).unwrap();
        assert!(!ring_shape.pieces.is_empty());
        let on_ring = sh2.shape(&inst(MeshId::CUBE, [40.0, 0.0, 0.0], [1.0, 1.0, 1.0])).unwrap();
        let (v, _) = ring_shape.overlap(&on_ring).expect("the cube is inside the loop road");
        assert!((v - 0.3).abs() < 1e-3, "{v}");
        // A cube beside the road is not.
        let beside = inst(MeshId::CUBE, [10.0, 0.5, 8.0], [1.0, 1.0, 1.0]);
        assert!(overlaps(&scene_of(&[at_origin(road), beside], &mut sh, usize::MAX)).is_empty());
    }

    #[test]
    fn the_segments_of_one_sweep_do_not_overlap_each_other() {
        let mut sh = Shapes::new();
        let arc: Vec<[f32; 3]> = (0..16)
            .map(|i| {
                let a = i as f32 / 16.0 * core::f32::consts::TAU;
                [40.0 * a.cos(), 0.0, 40.0 * a.sin()]
            })
            .collect();
        let road = register(&mut sh, swept(arc.clone(), true, &Profile::slab(7.0, 0.3)));
        let wall = {
            let mut lib = MeshLibrary::default();
            lib.add(swept(arc, true, &Profile::wall(8.0, 0.5, 1.0))).unwrap();
            lib
        };
        // Same library layout is fine here: the wall lives in its own scene.
        let mut sh2 = Shapes::new();
        sh2.add_library(&wall);
        let wall_id = MeshId(evani_render::mesh::BUILTIN_MESH_COUNT as u16);
        for (shapes, id) in [(&mut sh, road), (&mut sh2, wall_id)] {
            let s = scene_of(&[at_origin(id)], shapes, usize::MAX);
            assert_eq!(s.items.len(), 1);
            assert!(s.items[0].shape.pieces.len() > 4, "{} pieces", s.items[0].shape.pieces.len());
            let sum: f64 = s.items[0]
                .shape
                .pieces
                .iter()
                .map(|p| crate::shape::polys_volume(p.faces.iter().map(|f| f.pts.as_slice())))
                .sum();
            assert!((sum - s.items[0].shape.volume()).abs() < 1e-6);
            assert!(overlaps(&s).is_empty(), "touching segments of one mesh are not an overlap");
        }
    }

    #[test]
    fn decals_on_a_road_do_not_fight_it_but_floating_ones_are_flagged() {
        let mut road = inst(MeshId::CUBE, [0.0, -0.1, 0.0], [10.0, 0.2, 40.0]);
        road.color = [0.3, 0.3, 0.3, 1.0];
        let mut line = inst(MeshId::CUBE, [0.0, -0.05, 0.0], [0.3, 0.1, 6.0]); // top flush with the road
        line.color = [1.0, 1.0, 1.0, 1.0];
        let mut sh = Shapes::new();
        // Drawn as a decal it is on the surface (and does not fight it:
        // zfight.rs tests).
        let s = scene_of(&[road, line], &mut sh, 1);
        assert_eq!(s.items[1].kind, Kind::Decal);
        assert!(s.items[1].decal);
        assert!(overlaps(&s).is_empty());
        assert!(floating(&s).is_empty(), "{:?}", floating(&s));
        // Sunk 2 cm into the road, or 1 cm proud of it: still on it.
        for dy in [-0.02f32, 0.01] {
            let mut d = line;
            d.position[1] += dy;
            let f = floating(&scene_of(&[road, d], &mut sh, 1));
            assert!(f.is_empty(), "dy {dy}: {f:?}");
        }
        // A flat plane decal lying on the road is on it too.
        let mut paint = inst(MeshId::PLANE, [0.0, 0.0, 5.0], [2.0, 1.0, 2.0]);
        paint.color = [1.0, 0.0, 0.0, 1.0];
        let s = scene_of(&[road, paint], &mut sh, 1);
        assert!(floating(&s).is_empty(), "{:?}", floating(&s));
        // Lifted so its underside is 10 cm above the road: "decal off surface".
        let mut up = line;
        up.position[1] += 0.20;
        let f = floating(&scene_of(&[road, up], &mut sh, 1));
        assert_eq!(f.len(), 1, "{f:?}");
        assert_eq!(f[0].kind, "decal off surface");
        assert_eq!(f[0].check, Check::Floating);
        assert!((f[0].measure - 0.10).abs() < 0.01, "{}", f[0].measure);
        assert_eq!(f[0].items, vec![1]);
        // Far below the surface: buried, can't be seen.
        let block = inst(MeshId::CUBE, [0.0, -1.0, 0.0], [10.0, 2.0, 40.0]);
        let mut down = line;
        down.position[1] = -0.5;
        let f = floating(&scene_of(&[block, down], &mut sh, 1));
        assert!(f.iter().any(|i| i.kind == "decal off surface"), "{f:?}");
        // Over nothing at all.
        let lone = inst(MeshId::PLANE, [100.0, 0.0, 0.0], [2.0, 1.0, 2.0]);
        let f = floating(&scene_of(&[road, lone], &mut sh, 1));
        assert!(f.iter().any(|i| i.kind == "decal off surface"), "{f:?}");
    }

    #[test]
    fn custom_meshes_are_validated_and_open_ones_need_allowing() {
        let mut sh = Shapes::new();
        let closed = register(&mut sh, swept(vec![[0.0, 0.0, 0.0], [20.0, 0.0, 0.0]], false, &Profile::slab(3.0, 0.3)));
        // An open ribbon: the slab without its end caps and underside.
        let ribbon = Mesh {
            vertices: vec![
                [0.0, 0.0, -1.0, 0.0, 1.0, 0.0],
                [10.0, 0.0, -1.0, 0.0, 1.0, 0.0],
                [10.0, 0.0, 1.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0, 0.0, 1.0, 0.0],
            ],
            indices: vec![0, 2, 1, 0, 3, 2],
        };
        let mut lib = MeshLibrary::default();
        let a = lib.add(sh.mesh(closed).unwrap()).unwrap();
        let b = lib.add(ribbon).unwrap();
        sh.add_library(&lib);
        let used = vec![(a, [0.0; 3]), (b, [0.0; 3])];
        let issues = meshes(&sh, &used, &|id| sh.open_intended(id));
        assert!(issues.iter().all(|i| i.objects[0].contains(&mesh_name(b))), "the closed sweep is clean: {issues:?}");
        assert!(!issues.is_empty(), "the open ribbon is reported");
        sh.allow_open(b);
        assert!(meshes(&sh, &used, &|id| sh.open_intended(id)).is_empty());
    }

    #[test]
    fn instances_of_unknown_meshes_are_listed_not_dropped_silently() {
        let mut sh = Shapes::new();
        let ghost = MeshInstance::new(MeshId(9), [0.0; 3]);
        let s = scene_of(&[ground(), ghost, ghost], &mut sh, usize::MAX);
        assert_eq!(s.skipped, vec![(MeshId(9), 2)]);
        assert_eq!(s.items.len(), 1);
    }
}
