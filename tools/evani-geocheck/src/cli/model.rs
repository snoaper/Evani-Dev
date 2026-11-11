//! `evani-geocheck --model <folder|file.glb>`: the geometry checks on one
//! model's tier files, outside any game (Evani.Model construct models,
//! converted models before a game takes them).
//!
//! - Every `<id>.<tier>.glb` beside the given file (or in the folder; the
//!   converter's `master` copy is skipped) is read through the engine's own
//!   reader (evani-gltf: meshopt, quantisation and node transforms as a game
//!   reads them).
//! - Each mesh primitive is split into its pieces: triangles joined by a
//!   shared edge (positions welded) form one piece, so the planks merged
//!   into one draw call are checked one by one.
//! - The static checks run per tier on a scene of those pieces standing on
//!   a ground plane at the model's lowest point: overlaps between any two
//!   pieces (each piece its own object; convex pieces are measured exactly,
//!   concave ones are surfaces only and named in the notes), floating per
//!   output node (a node rests on the ground or on another node), mesh
//!   validation of every piece, and z-fighting seen from an orbit of
//!   cameras round the model at two distances and two heights. Moving
//!   clipping, camera clipping, collision and the live captures (gaps,
//!   flicker) need a running game and are reported as not run.
//! - Tags: `geocheck.toml` beside the model files (`[[tag]]` as in a game).
//! - Pictures: `views/<tier>.png` and `views/tiers.png` (the tier files as
//!   read, in their piece colours) for review, and `views/closeup.png` (the
//!   top two tiers where they differ most, enlarged twice).
//! - Tier gain: one note per adjacent tier pair, the share of the model's
//!   pixels the higher tier changes at its design view (WARN under
//!   `TIER_GAIN_MIN`; `cargo xtask model` warns with the parts and a fix).

use std::path::{Path, PathBuf};

use evani_core::camera::CameraPose;
use evani_geocheck::checks;
use evani_geocheck::image::Camera;
use evani_geocheck::report::{Check, Report};
use evani_geocheck::scene::{Config, Kind, Object, Scene, object_label};
use evani_geocheck::shape::Shapes;
use evani_geocheck::tags::{self, Tag};
use evani_geocheck::zfight::{self, DepthSetup, Viewer};
use evani_render::mesh::{Mesh, MeshId};
use evani_render::world::MeshInstance;
use serde_json::Value;

const TIERS: [&str; 6] = ["lite", "low", "medium", "high", "ultra", "vr"];
/// First mesh id used for pieces (above the engine's built-ins and library).
const FIRST_ID: u16 = 1000;

/// One piece of a tier file, in model space.
pub struct Piece {
    pub node: String,
    pub pos: Vec<[f64; 3]>,
    pub nrm: Vec<[f64; 3]>,
    pub tris: Vec<[u32; 3]>,
    /// Linear RGB (COLOR_0 times the material's base colour factor).
    pub colour: [f64; 3],
}

/// One tier file as read.
pub struct Tier {
    pub tier: String,
    pub bytes: usize,
    pub pieces: Vec<Piece>,
}

/// The model id and its tier files: `<folder>/<id>.<tier>.glb`.
pub fn tier_files(arg: &Path) -> Result<(String, Vec<(String, PathBuf)>), String> {
    let (dir, want) = if arg.is_dir() {
        (arg.to_path_buf(), None)
    } else {
        let name = arg.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let stem = name.strip_suffix(".glb").ok_or_else(|| format!("{}: not a .glb file or a folder", arg.display()))?;
        let id = stem.rsplit_once('.').filter(|(_, t)| TIERS.contains(t) || *t == "master").map_or(stem, |x| x.0);
        (arg.parent().map_or_else(|| PathBuf::from("."), Path::to_path_buf), Some(id.to_string()))
    };
    let mut found: Vec<(String, String, PathBuf)> = Vec::new();
    let entries = std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for e in entries.filter_map(Result::ok) {
        let p = e.path();
        let Some(name) = p.file_name().and_then(|n| n.to_str()).map(str::to_owned) else { continue };
        let Some(stem) = name.strip_suffix(".glb") else { continue };
        let Some((id, tier)) = stem.rsplit_once('.') else { continue };
        if TIERS.contains(&tier) && want.as_deref().is_none_or(|w| w == id) {
            found.push((id.to_string(), tier.to_string(), p));
        }
    }
    let mut ids: Vec<&str> = found.iter().map(|f| f.0.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    let id = match ids.as_slice() {
        [one] => (*one).to_string(),
        [] => return Err(format!("no <id>.<tier>.glb files in {} (tiers {})", dir.display(), TIERS.join(", "))),
        many => return Err(format!("{} holds several models ({}): give one file, e.g. {}/{}.low.glb", dir.display(), many.join(", "), dir.display(), many[0])),
    };
    let mut files: Vec<(String, PathBuf)> = found.into_iter().filter(|f| f.0 == id).map(|f| (f.1, f.2)).collect();
    files.sort_by_key(|f| TIERS.iter().position(|t| *t == f.0));
    Ok((id, files))
}

fn mat3(m: &[f32; 16], v: [f32; 3]) -> [f64; 3] {
    [
        f64::from(m[0] * v[0] + m[4] * v[1] + m[8] * v[2]),
        f64::from(m[1] * v[0] + m[5] * v[1] + m[9] * v[2]),
        f64::from(m[2] * v[0] + m[6] * v[1] + m[10] * v[2]),
    ]
}

fn det3(m: &[f32; 16]) -> f64 {
    let a = |i: usize| f64::from(m[i]);
    a(0) * (a(5) * a(10) - a(9) * a(6)) - a(4) * (a(1) * a(10) - a(9) * a(2)) + a(8) * (a(1) * a(6) - a(5) * a(2))
}

fn norm(v: [f64; 3]) -> [f64; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 1e-300 { [v[0] / l, v[1] / l, v[2] / l] } else { [0.0, 1.0, 0.0] }
}

/// Weld key: positions to 10 micrometres.
fn key(p: [f64; 3]) -> [i64; 3] {
    [(p[0] * 1e5).round() as i64, (p[1] * 1e5).round() as i64, (p[2] * 1e5).round() as i64]
}

fn find(p: &mut [usize], mut x: usize) -> usize {
    while p[x] != x {
        p[x] = p[p[x]];
        x = p[x];
    }
    x
}

/// Split a triangle list into pieces: triangles joined across a welded edge
/// that exactly two triangles use. Two pieces meeting edge to edge (a butt
/// joint at a box corner) share an edge used by four triangles, so they stay
/// two pieces; a piece that is itself non-manifold stays one piece through
/// its other edges, so mesh validation still sees the fault.
pub fn split_pieces(pos: &[[f64; 3]], tris: &[[u32; 3]]) -> Vec<Vec<usize>> {
    let mut weld: std::collections::BTreeMap<[i64; 3], usize> = std::collections::BTreeMap::new();
    let ids: Vec<usize> = pos.iter().map(|p| {
        let n = weld.len();
        *weld.entry(key(*p)).or_insert(n)
    }).collect();
    let mut edges: std::collections::BTreeMap<(usize, usize), Vec<usize>> = std::collections::BTreeMap::new();
    for (ti, t) in tris.iter().enumerate() {
        for k in 0..3 {
            let (a, b) = (ids[t[k] as usize], ids[t[(k + 1) % 3] as usize]);
            if a != b {
                edges.entry((a.min(b), a.max(b))).or_default().push(ti);
            }
        }
    }
    let mut parent: Vec<usize> = (0..tris.len()).collect();
    for users in edges.values() {
        if let [x, y] = users.as_slice() {
            let (ra, rb) = (find(&mut parent, *x), find(&mut parent, *y));
            if ra != rb {
                parent[ra.max(rb)] = ra.min(rb);
            }
        }
    }
    let mut groups: std::collections::BTreeMap<usize, Vec<usize>> = std::collections::BTreeMap::new();
    for ti in 0..tris.len() {
        let r = find(&mut parent, ti);
        groups.entry(r).or_default().push(ti);
    }
    groups.into_values().collect()
}

/// Read one tier file into pieces (model space).
pub fn load_tier(tier: &str, file: &Path) -> Result<Tier, String> {
    let bytes = std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let g = evani_gltf::Gltf::from_glb(&bytes).map_err(|e| format!("{}: {}", file.display(), e.0))?;
    let world = g.world_matrices();
    let mut pieces = Vec::new();
    for (ni, node) in g.nodes.iter().enumerate() {
        let Some(mi) = node.mesh else { continue };
        let Some(mesh) = g.meshes.get(mi) else { continue };
        let m = &world[ni];
        let flip = det3(m) < 0.0;
        // The packer may hang a mesh on an unnamed child of its named node:
        // the piece belongs to the nearest named node above it.
        let mut owner = ni;
        while g.nodes[owner].name.is_empty() {
            match g.parent(owner) {
                Some(p) => owner = p,
                None => break,
            }
        }
        let node_name = g.nodes[owner].name.clone();
        for prim in mesh.primitives.iter().filter(|p| p.mode == 4) {
            let rd = |a: Option<usize>| -> Result<Vec<f32>, String> { a.map_or(Ok(Vec::new()), |i| g.read_f32(i).map_err(|e| format!("{}: {}", file.display(), e.0))) };
            let p = rd(prim.attribute("POSITION"))?;
            let n = rd(prim.attribute("NORMAL"))?;
            let c = rd(prim.attribute("COLOR_0"))?;
            let ccomp = prim.attribute("COLOR_0").and_then(|a| g.accessors.get(a)).map_or(4, |a| a.components);
            let idx: Vec<u32> = match prim.indices {
                Some(i) => g.read_u32(i).map_err(|e| format!("{}: {}", file.display(), e.0))?,
                None => (0..(p.len() / 3) as u32).collect(),
            };
            let pos: Vec<[f64; 3]> = p.as_chunks::<3>().0.iter().map(|v| {
                let w = evani_gltf::transform_point(m, [v[0], v[1], v[2]]);
                [f64::from(w[0]), f64::from(w[1]), f64::from(w[2])]
            }).collect();
            let nrm: Vec<[f64; 3]> = n.as_chunks::<3>().0.iter().map(|v| norm(mat3(m, [v[0], v[1], v[2]]))).collect();
            let tris: Vec<[u32; 3]> = idx.as_chunks::<3>().0.iter().map(|t| if flip { [t[0], t[2], t[1]] } else { [t[0], t[1], t[2]] }).collect();
            let factor = prim.material.and_then(|k| g.materials.get(k)).map_or([1.0; 4], |mt| mt.base_color_factor);
            for group in split_pieces(&pos, &tris) {
                let mut remap: std::collections::BTreeMap<u32, u32> = std::collections::BTreeMap::new();
                let mut pc = Piece { node: node_name.clone(), pos: Vec::new(), nrm: Vec::new(), tris: Vec::new(), colour: [0.7; 3] };
                for &ti in &group {
                    let mut t = [0u32; 3];
                    for (k, v) in tris[ti].iter().enumerate() {
                        let next = remap.len() as u32;
                        t[k] = *remap.entry(*v).or_insert_with(|| {
                            pc.pos.push(pos[*v as usize]);
                            pc.nrm.push(nrm.get(*v as usize).copied().unwrap_or([0.0, 1.0, 0.0]));
                            next
                        });
                    }
                    pc.tris.push(t);
                }
                if let Some(&first) = remap.keys().next() {
                    let i = first as usize * ccomp;
                    for (k, (slot, f)) in pc.colour.iter_mut().zip(factor).enumerate() {
                        *slot = f64::from(c.get(i + k).copied().unwrap_or(1.0) * f);
                    }
                }
                pieces.push(pc);
            }
        }
    }
    Ok(Tier { tier: tier.to_string(), bytes: bytes.len(), pieces })
}

fn bounds(pieces: &[Piece]) -> ([f64; 3], [f64; 3]) {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in pieces {
        for q in &p.pos {
            for k in 0..3 {
                lo[k] = lo[k].min(q[k]);
                hi[k] = hi[k].max(q[k]);
            }
        }
    }
    (lo, hi)
}

/// Nodes that touch nothing: no vertex of any of their pieces lies within
/// `gap` of another node's piece (its box) or of the ground. Down-rays
/// can't judge hardware hung on a wall (a strap, a handle), so a model is
/// checked as one object on the ground plus this attachment test.
fn detached(t: &Tier, gap: f64, ground_y: f64) -> Vec<evani_geocheck::report::Issue> {
    let boxes: Vec<([f64; 3], [f64; 3])> = t.pieces.iter().map(|p| bounds(std::slice::from_ref(p))).collect();
    let mut names: Vec<&str> = t.pieces.iter().map(|p| p.node.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    let mut out = Vec::new();
    if names.len() < 2 {
        return out;
    }
    for n in names {
        let mine: Vec<usize> = (0..t.pieces.len()).filter(|&i| t.pieces[i].node == n).collect();
        let near = |p: [f64; 3], b: &([f64; 3], [f64; 3])| (0..3).all(|k| p[k] >= b.0[k] - gap && p[k] <= b.1[k] + gap);
        let touches = mine.iter().any(|&i| {
            t.pieces[i].pos.iter().any(|p| p[1] <= ground_y + gap || boxes.iter().enumerate().any(|(j, b)| t.pieces[j].node != n && near(*p, b)))
        });
        if !touches {
            let (lo, hi) = bounds(&mine.iter().map(|&i| Piece { node: String::new(), pos: t.pieces[i].pos.clone(), nrm: Vec::new(), tris: Vec::new(), colour: [0.0; 3] }).collect::<Vec<_>>());
            let mut is = evani_geocheck::report::Issue::new(Check::Floating, "detached node", [(lo[0] + hi[0]) * 0.5, lo[1], (lo[2] + hi[2]) * 0.5]);
            is.objects = vec![n.to_string()];
            is.detail = format!("node {n} touches no other node and not the ground (nothing within {gap} m)");
            is.fix = "Move the part onto what holds it (the recipe's coordinates or the band it is laid on), or make it part of that node.".into();
            out.push(is);
        }
    }
    out
}

fn pose(eye: [f64; 3], target: [f64; 3]) -> CameraPose {
    CameraPose { eye: eye.map(|x| x as f32), target: target.map(|x| x as f32), up: [0.0, 1.0, 0.0], fov_y_deg: 50.0 }
}

/// The checks on one tier; issues carry the tier in their detail.
fn check_tier(t: &Tier, cfg: &Config, report: &mut Report) {
    let (lo, hi) = bounds(&t.pieces);
    let centre = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5];
    let radius = 0.5 * ((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2) + (hi[2] - lo[2]).powi(2)).sqrt().max(1e-3);
    let mut shapes = Shapes::new();
    let mut list: Vec<(usize, MeshInstance)> = Vec::new();
    let mut labels: Vec<String> = Vec::new();
    for (k, p) in t.pieces.iter().enumerate() {
        let id = MeshId(FIRST_ID + k as u16);
        let vertices = p.pos.iter().zip(&p.nrm).map(|(a, n)| [a[0] as f32, a[1] as f32, a[2] as f32, n[0] as f32, n[1] as f32, n[2] as f32]).collect();
        shapes.add_mesh(id, Mesh { vertices, indices: p.tris.iter().flatten().copied().collect() });
        list.push((k, MeshInstance::new(id, [0.0; 3])));
        labels.push(format!("{} piece {} ({} triangles)", p.node, k, p.tris.len()));
    }
    // The ground: a plane at the lowest point, wide enough to be a structure.
    let ground_at = t.pieces.len();
    let mut ground = MeshInstance::new(MeshId::PLANE, [centre[0] as f32, lo[1] as f32, centre[2] as f32]);
    let wide = (radius * 8.0).max(cfg.structure * 2.0) as f32;
    ground.scale = [wide, 1.0, wide];
    list.push((ground_at, ground));
    let label = |m: &MeshInstance| {
        if m.mesh == MeshId::PLANE { "ground (plane)".to_string() } else { labels.get(usize::from(m.mesh.0 - FIRST_ID)).cloned().unwrap_or_default() }
    };
    let mut scene = Scene::build(&list, &mut shapes, &label, cfg.clone());
    // Pieces are parts whatever their thickness (a 2 cm plank is not a decal
    // here); each piece is its own object, so every pair is checked.
    let nodes: Vec<String> = scene.items.iter().map(|it| if it.index == ground_at { "ground".to_string() } else { t.pieces[it.index].node.clone() }).collect();
    for it in &mut scene.items {
        if it.index != ground_at {
            it.kind = Kind::Part;
        }
    }
    let per = |s: &mut Scene, key: &dyn Fn(usize) -> String| {
        let mut keys: Vec<String> = Vec::new();
        let mut objects: Vec<Object> = Vec::new();
        for i in 0..s.items.len() {
            let k = key(i);
            let o = match keys.iter().position(|x| *x == k) {
                Some(o) => o,
                None => {
                    keys.push(k);
                    objects.push(Object { items: Vec::new(), aabb: s.items[i].shape.aabb, kind: s.items[i].kind, label: String::new() });
                    objects.len() - 1
                }
            };
            objects[o].items.push(i);
            objects[o].aabb = objects[o].aabb.union(&s.items[i].shape.aabb);
            s.items[i].object = o;
        }
        for o in &mut objects {
            o.label = object_label(&s.items, &o.items);
        }
        s.objects = objects;
    };
    let mut whole = scene.clone();
    per(&mut scene, &|i| format!("item {i}"));
    per(&mut whole, &|i| if i == ground_at { "ground".to_string() } else { "model".to_string() });
    let tag = |mut v: Vec<evani_geocheck::report::Issue>| {
        for i in &mut v {
            i.detail = format!("[{}] {}", t.tier, i.detail);
        }
        v
    };
    report.issues.extend(tag(checks::overlaps(&scene)));
    report.issues.extend(tag(checks::floating(&whole)));
    report.issues.extend(tag(detached(t, cfg.float_gap, lo[1])));
    let insts: Vec<MeshInstance> = list.iter().map(|x| x.1).collect();
    let used = checks::used_meshes(&insts);
    report.issues.extend(tag(checks::meshes(&shapes, &used, &|id| shapes.open_intended(id))));
    let mut viewers = Vec::new();
    for (dist, h) in [(2.6, 0.35), (2.6, 1.2), (1.6, 0.5)] {
        for k in 0..12 {
            let a = core::f64::consts::TAU * (k as f64 + 0.5) / 12.0;
            let eye = [centre[0] + radius * dist * a.cos(), centre[1] + radius * dist * h, centre[2] + radius * dist * a.sin()];
            viewers.push(Viewer::new(Camera::from_pose(&pose(eye, centre), 16.0 / 9.0), 720.0, "orbit", None));
        }
    }
    report.issues.extend(tag(zfight::zfight(&scene, DepthSetup::engine(), &viewers)));
    let surface_only: Vec<MeshId> = used.iter().map(|u| u.0).collect();
    let concave = shapes.notes(&surface_only).len();
    report.notes.push(format!(
        "{}: {} pieces in {} nodes, {} triangles, {} bytes, {:.2} x {:.2} x {:.2} m; {} concave piece(s) are surfaces only for the overlap volume (curved boards, ropes), the rest are measured exactly; {} cameras.",
        t.tier,
        t.pieces.len(),
        { let mut n = nodes.clone(); n.sort(); n.dedup(); n.len() - 1 },
        t.pieces.iter().map(|p| p.tris.len()).sum::<usize>(),
        t.bytes,
        hi[0] - lo[0],
        hi[1] - lo[1],
        hi[2] - lo[2],
        concave,
        viewers.len()
    ));
}

/// A tier's pieces as one picture mesh and its piece colours.
fn tier_mesh(t: &Tier) -> (evani_model::mesh::Mesh, Vec<[u8; 4]>) {
    let mut mesh = evani_model::mesh::Mesh::default();
    let mut cols = Vec::new();
    for (k, p) in t.pieces.iter().enumerate() {
        let base = mesh.pos.len() as u32;
        mesh.pos.extend_from_slice(&p.pos);
        mesh.nrm.extend_from_slice(&p.nrm);
        mesh.tris.extend(p.tris.iter().map(|x| [x[0] + base, x[1] + base, x[2] + base]));
        mesh.tri_part.extend(std::iter::repeat_n(k as u32, p.tris.len()));
        let c = |k: usize| (p.colour[k] * 255.0).round().clamp(0.0, 255.0) as u8;
        cols.push([c(0), c(1), c(2), 255]);
    }
    (mesh, cols)
}

/// Tier gain on the files as read (the measure `cargo xtask model` warns
/// on, evani_model::construct_out::TierGain): each tier against the one
/// below at its design view, one note line per pair.
fn tier_gain_notes(tiers: &[Tier]) -> Vec<String> {
    use evani_model::construct_out::{TIER_GAIN_MIN, design_distance, design_px, mesh_change};
    let threads = evani_model::rng::default_threads().min(4);
    tiers
        .windows(2)
        .map(|w| {
            let (lo, hi) = (&w[0], &w[1]);
            let (ml, cl) = tier_mesh(lo);
            let (mh, ch) = tier_mesh(hi);
            let px = design_px(&hi.tier);
            let (changed, covered, extent) = mesh_change((&ml, &cl), (&mh, &ch), px, threads);
            let share = changed as f64 / covered.max(1) as f64;
            let extra = mh.tris.len() as i64 - ml.tris.len() as i64;
            let bytes = hi.bytes as i64 - lo.bytes as i64;
            let wasted = (extra > 0 || bytes > 1024) && share < TIER_GAIN_MIN;
            format!(
                "Tier gain {} -> {}: {:+} triangles, {:+} bytes, {:.1}% of the model's pixels changed at {} px across (about {:.1} m){}.",
                lo.tier,
                hi.tier,
                extra,
                bytes,
                share * 100.0,
                px,
                design_distance(extent, px),
                if wasted { format!(": WARN under {:.0}%, the {} tier's extra detail does not show (cargo xtask model names the parts)", TIER_GAIN_MIN * 100.0, hi.tier) } else { String::new() }
            )
        })
        .collect()
}

/// Pictures of every tier as read: `views/<tier>.png` and a captioned
/// `views/tiers.png`.
fn pictures(tiers: &[Tier], out: &Path) -> Result<Vec<PathBuf>, String> {
    let views = out.join("views");
    std::fs::create_dir_all(&views).map_err(|e| format!("{}: {e}", views.display()))?;
    let size = 512;
    let threads = evani_model::rng::default_threads().min(4);
    let mut written = Vec::new();
    let mut sheet_parts = Vec::new();
    for t in tiers {
        let (mesh, cols) = tier_mesh(t);
        let px = evani_model::construct_out::picture_mesh(&mesh, &cols, size, "three_quarter", threads);
        let f = views.join(format!("{}.png", t.tier));
        std::fs::write(&f, evani_model::png::write_rgb(size, size, &px)).map_err(|e| format!("{}: {e}", f.display()))?;
        written.push(f);
        sheet_parts.push((px, format!("{} {}T {:.1}KB", t.tier.to_uppercase(), mesh.tris.len(), t.bytes as f64 / 1024.0)));
    }
    let n = sheet_parts.len().max(1);
    let (w, h) = (size * n, size + 28);
    let mut sheet = vec![255u8; w * h * 3];
    for (k, (px, cap)) in sheet_parts.iter().enumerate() {
        for y in 0..size {
            let dst = ((y + 28) * w + k * size) * 3;
            sheet[dst..dst + size * 3].copy_from_slice(&px[y * size * 3..(y + 1) * size * 3]);
        }
        let mut g = evani_model::png::Grey::new(size, 28, 255);
        evani_model::preview::text(&mut g, 8, 6, 2, 30, cap);
        for y in 0..28 {
            for x in 0..size {
                let v = g.get(x, y);
                let i = (y * w + k * size + x) * 3;
                sheet[i..i + 3].copy_from_slice(&[v, v, v]);
            }
        }
    }
    let f = views.join("tiers.png");
    std::fs::write(&f, evani_model::png::write_rgb(w, h, &sheet)).map_err(|e| format!("{}: {e}", f.display()))?;
    written.push(f);
    // The top two tiers side by side where they differ most, at the higher
    // one's design view, enlarged twice: `views/closeup.png`.
    if let [.., lo, hi] = tiers {
        let ((ml, cl), (mh, ch)) = (tier_mesh(lo), tier_mesh(hi));
        let cap = |t: &Tier, n: usize| format!("{} {}T {:.1}KB", t.tier.to_uppercase(), n, t.bytes as f64 / 1024.0);
        let png = evani_model::construct_out::closeup_mesh(
            (&ml, &cl, &cap(lo, ml.tris.len())),
            (&mh, &ch, &cap(hi, mh.tris.len())),
            evani_model::construct_out::design_px(&hi.tier),
            256,
            2,
            threads,
        );
        let f = views.join("closeup.png");
        std::fs::write(&f, png).map_err(|e| format!("{}: {e}", f.display()))?;
        written.push(f);
    }
    Ok(written)
}

/// The report for one model; returns it, its folder and the tags.
pub fn run(root: &Path, arg: &Path, render: bool) -> Result<(Report, PathBuf, Vec<Tag>), String> {
    let (id, files) = tier_files(arg)?;
    let app = format!("model-{id}");
    let out = super::render::out_dir(root, &app);
    std::fs::create_dir_all(&out).map_err(|e| format!("create {}: {e}", out.display()))?;
    let mut report = Report { app: app.clone(), ..Report::default() };
    let cfg = Config::default();
    let dir = files.first().and_then(|f| f.1.parent()).map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let mut tag_list = Vec::new();
    let tf = dir.join("geocheck.toml");
    if let Ok(text) = std::fs::read_to_string(&tf) {
        let v: Value = toml::from_str(&text).map_err(|e| format!("{}: {e}", tf.display()))?;
        if let Some(t) = v.get("tag") {
            tag_list = tags::parse(t).map_err(|e| format!("{}: {e}", tf.display()))?;
        }
    }
    report.notes.push(format!("Model {id}: tier files {} in {}.", files.iter().map(|f| f.0.as_str()).collect::<Vec<_>>().join(", "), dir.display()));
    let mut tiers = Vec::new();
    for (tier, file) in &files {
        eprintln!("geocheck: model {id} tier {tier}");
        let t = load_tier(tier, file)?;
        check_tier(&t, &cfg, &mut report);
        tiers.push(t);
    }
    report.ran.push((Check::Overlap, format!("every pair of pieces (each tier's primitives split into pieces joined by shared edges), exact convex intersection volume above {} m^3; the ground plane at the model's lowest point, sinking into it up to {} m is contact.", cfg.overlap_volume, cfg.sink)));
    report.ran.push((Check::Floating, format!("the model as one object: rays down from its base must meet the ground within {0} m; and every output node must touch another node or the ground (a vertex within {0} m of another node's piece box).", cfg.float_gap)));
    report.ran.push((Check::Mesh, "every piece validated (degenerate, duplicate, open, non-manifold, flipped, inside-out, self-intersecting).".into()));
    report.ran.push((Check::ZFight, "faces of different pieces parallel within the threshold that the engine's depth buffer can't separate, from 36 orbit cameras per tier (12 azimuths at 2.6 radii low and high, and at 1.6 radii).".into()));
    for (c, why) in [
        (Check::Moving, "not applicable: one model, nothing moves."),
        (Check::Camera, "not applicable: no game camera."),
        (Check::Collision, "not applicable: models are render-only; collision comes from kit data."),
        (Check::Gap, "not run: gaps and flicker need captures in a running game (run geocheck on the game that draws this model)."),
    ] {
        report.ran.push((c, why.into()));
    }
    if render {
        report.notes.extend(tier_gain_notes(&tiers));
        let shots = pictures(&tiers, &out)?;
        report.notes.push(format!("Pictures: {}.", shots.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")));
    }
    Ok((report, out, tag_list))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pieces_split_by_shared_edges_not_corners() {
        // Two unit squares sharing only a corner, and a third sharing an edge
        // with the first: two pieces.
        let pos = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0], [2.0, 1.0, 0.0], [2.0, 2.0, 0.0], [0.0, -1.0, 0.0], [1.0, -1.0, 0.0]];
        let tris = vec![[0, 1, 2], [0, 2, 3], [4, 5, 6], [7, 8, 1], [7, 1, 0]];
        let p = split_pieces(&pos, &tris);
        assert_eq!(p, vec![vec![0, 1, 3, 4], vec![2]]);
        // Four triangles on one edge (two pieces meeting edge to edge): none joins.
        let pos = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, -1.0]];
        let tris = vec![[0, 1, 2], [1, 0, 3], [0, 1, 4], [1, 0, 5]];
        assert_eq!(split_pieces(&pos, &tris).len(), 4);
    }
}
