//! The review dump (docs/KIT_PROCESS.md, "Kit cooperation"): the
//! `review_dump` test measures the kit's real draw output and, when
//! `EVANI_REVIEW_DUMP` is set, writes `$EVANI_REVIEW_DUMP/review.json` for
//! the review's colour and contact-shadow checks (schema in
//! `tools/xtask/src/kitchecks.rs`).
//!
//! * Colours: the field's kart colours from `data/karts.json` (linear, as
//!   the renderer gets them) in sRGB hex; the player's kart (Pip) and how it
//!   is marked besides its colour.
//! * Objects: every kart on the starting grid, every item box and every
//!   scenery prop, with its gap to the ground and whether it casts a contact
//!   shadow on every tier.
//!   - The ground is the drawn ground: the road and the grass shoulder and
//!     bank (the track meshes), meadow patches and the meadow, built by the kit's own
//!     [`draw::build_statics_with`]; the gap is a ray cast straight down
//!     from each vertex of the object's drawn meshes, and the object's gap
//!     is the smallest (negative: sunk).
//!   - A contact shadow on Medium and up is the shadow map: the object has
//!     a lit (not emissive) part. On Low (no shadow map) a kart needs a blob
//!     under it from the kit's blob list, an item box or a prop a dark disc
//!     under it in the Low draw (props: only those in the Low layout; Low
//!     has no grass tufts and two trees in three are left out).
//!   - Mountains, the hill ring and the clouds are terrain and sky, not
//!     objects standing on the ground: they are left out.
//!   - Item boxes hover on purpose and are tagged `intentional`; the
//!     grandstands reach down into the bank's slope on purpose (their steps
//!     go below the bank's foot so no gap shows) and are tagged too.

use std::collections::HashMap;

use evani_core::App;
use evani_render::mesh::builtin;
use evani_render::{MeshId, MeshInstance, RenderWorld};
use serde_json::{Value, json};

use crate::draw::{self, Mat, ROLES, role};
use crate::race::{Race, RaceView, Request};
use crate::scenery::{self, PropKind};
use crate::{KartRacerPlugin, ctx};

/// Grid cell for the ground lookup (m).
const CELL: f32 = 4.0;
/// Triangles wider than this go in the always-checked list (the meadow).
const BIG: f32 = 64.0;
/// The review's ground tolerance (kitchecks `GAP_3D_M`).
const GAP_OK: f64 = 0.05;
/// How the player's kart is marked besides its colour.
const MARKER: &str = "pink arrow with a white cap over the kart in split-screen and in the finish orbit (none in the one-player chase view, where the kart is the one you drive), plus a larger dot with a white ring on the minimap in every view";

/// Every role resolved to its own material slot, so a drawn instance's
/// material says what it is.
fn role_mats() -> [Mat; 16] {
    core::array::from_fn(|r| Mat { index: r as u32, tint: [1.0; 3] })
}

/// Role `material` is emissive (unlit in the toon look: casts no shadow).
fn emissive(material: u32) -> bool {
    ROLES.get(material as usize).is_some_and(|r| r.2)
}

fn rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let u = [q[0], q[1], q[2]];
    let w = q[3];
    let cross = |a: [f32; 3], b: [f32; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let t = cross(u, v).map(|c| 2.0 * c);
    let ut = cross(u, t);
    [v[0] + w * t[0] + ut[0], v[1] + w * t[1] + ut[1], v[2] + w * t[2] + ut[2]]
}

/// World-space triangles of an instance.
fn triangles(m: &MeshInstance, meshes: &mut HashMap<MeshId, evani_render::mesh::Mesh>) -> Vec<[[f32; 3]; 3]> {
    let mesh = meshes.entry(m.mesh).or_insert_with(|| {
        builtin(m.mesh)
            .or_else(|| (0..crate::propmesh::COUNT).find(|&k| crate::propmesh::id(k) == m.mesh).map(|k| crate::propmesh::build().swap_remove(k)))
            .expect("a built-in or prop mesh")
    });
    let world = |i: u32| {
        let v = mesh.vertices[i as usize];
        let r = rotate(m.rotation, [v[0] * m.scale[0], v[1] * m.scale[1], v[2] * m.scale[2]]);
        [r[0] + m.position[0], r[1] + m.position[1], r[2] + m.position[2]]
    };
    mesh.indices.as_chunks::<3>().0.iter().map(|t| [world(t[0]), world(t[1]), world(t[2])]).collect()
}

/// The drawn ground's upward faces, in a grid for lookups straight down.
struct Ground {
    tris: Vec<[[f32; 3]; 3]>,
    cells: HashMap<(i32, i32), Vec<usize>>,
    big: Vec<usize>,
}

impl Ground {
    fn new(statics: &draw::Statics) -> Self {
        // The track meshes under their placeholder ids, the rest built in.
        let mut meshes: HashMap<MeshId, evani_render::mesh::Mesh> =
            statics.meshes.iter().enumerate().map(|(k, m)| (draw::track_mesh_id(k), m.clone())).collect();
        let mut g = Ground { tris: Vec::new(), cells: HashMap::new(), big: Vec::new() };
        let ground_roles = [role::ROAD as u32, role::GRASS as u32];
        let ground_mesh = |id: MeshId| matches!(id, MeshId::CUBE | MeshId::PLANE) || meshes.contains_key(&id);
        let ground: Vec<MeshInstance> = statics.instances.iter().filter(|m| ground_roles.contains(&m.material) && ground_mesh(m.mesh)).copied().collect();
        for m in &ground {
            for t in triangles(m, &mut meshes) {
                let (a, b, c) = (t[0], t[1], t[2]);
                let ny = (b[2] - a[2]) * (c[0] - a[0]) - (b[0] - a[0]) * (c[2] - a[2]);
                let len = {
                    let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                    let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
                    let n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
                    (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt()
                };
                // Upward faces only (normal more up than sideways).
                if len <= 0.0 || ny / len < 0.5 {
                    continue;
                }
                let i = g.tris.len();
                g.tris.push(t);
                let lo = [a[0].min(b[0]).min(c[0]), a[2].min(b[2]).min(c[2])];
                let hi = [a[0].max(b[0]).max(c[0]), a[2].max(b[2]).max(c[2])];
                if hi[0] - lo[0] > BIG || hi[1] - lo[1] > BIG {
                    g.big.push(i);
                    continue;
                }
                for x in (lo[0] / CELL).floor() as i32..=(hi[0] / CELL).floor() as i32 {
                    for z in (lo[1] / CELL).floor() as i32..=(hi[1] / CELL).floor() as i32 {
                        g.cells.entry((x, z)).or_default().push(i);
                    }
                }
            }
        }
        g
    }

    /// Height of the highest ground face straight below or above (x, z).
    fn height(&self, x: f32, z: f32) -> Option<f32> {
        let key = ((x / CELL).floor() as i32, (z / CELL).floor() as i32);
        let near = self.cells.get(&key).map_or(&[][..], Vec::as_slice);
        near.iter()
            .chain(&self.big)
            .filter_map(|&i| {
                let [a, b, c] = self.tris[i];
                let d = (b[2] - c[2]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[2] - c[2]);
                if d.abs() < 1e-9 {
                    return None;
                }
                let l1 = ((b[2] - c[2]) * (x - c[0]) + (c[0] - b[0]) * (z - c[2])) / d;
                let l2 = ((c[2] - a[2]) * (x - c[0]) + (a[0] - c[0]) * (z - c[2])) / d;
                let l3 = 1.0 - l1 - l2;
                let e = -1e-4;
                (l1 >= e && l2 >= e && l3 >= e).then_some(l1 * a[1] + l2 * b[1] + l3 * c[1])
            })
            .reduce(f32::max)
    }

    /// Smallest clearance of the instances' vertices over the ground (m).
    fn gap(&self, parts: &[MeshInstance]) -> f64 {
        let mut meshes = HashMap::new();
        let mut best = f64::MAX;
        for m in parts {
            for t in triangles(m, &mut meshes) {
                for v in t {
                    if let Some(h) = self.height(v[0], v[2]) {
                        best = best.min(f64::from(v[1] - h));
                    }
                }
            }
        }
        best
    }
}

/// Horizontal box round the instances (min x, min z, max x, max z).
fn footprint(parts: &[MeshInstance]) -> [f32; 4] {
    let mut meshes = HashMap::new();
    let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    for m in parts {
        for t in triangles(m, &mut meshes) {
            for v in t {
                b = [b[0].min(v[0]), b[1].min(v[2]), b[2].max(v[0]), b[3].max(v[2])];
            }
        }
    }
    b
}

fn inside(fp: [f32; 4], p: [f32; 3], margin: f32) -> bool {
    p[0] >= fp[0] - margin && p[0] <= fp[2] + margin && p[2] >= fp[1] - margin && p[2] <= fp[3] + margin
}

fn srgb_hex(c: [f32; 3]) -> String {
    let ch = |v: f32| {
        let v = v.clamp(0.0, 1.0);
        let s = if v <= 0.003_130_8 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
        (s * 255.0).round() as u8
    };
    format!("#{:02x}{:02x}{:02x}", ch(c[0]), ch(c[1]), ch(c[2]))
}

fn object(name: String, gap: f64, contact_shadow: bool, intentional: Option<&str>) -> Value {
    let mut o = json!({ "name": name, "gap": (gap * 1000.0).round() / 1000.0, "contact_shadow": contact_shadow });
    if let Some(why) = intentional {
        o["intentional"] = json!(why);
    }
    o
}

/// Measure the kit's draw output and write the review dump.
#[test]
fn review_dump() {
    let mut a = App::new("review").with_kit(KartRacerPlugin);
    a.start();
    a.world.res_mut::<Race>().request(Request::Start { players: 1 });
    a.run_headless(10, 1000.0 / 60.0);
    let (_, c, _) = ctx(&a.world).expect("kit built");
    let verge = c.t.verge_width;
    let mats = role_mats();
    let r = RaceView::new(&a.world);
    let track = &r.track;

    let mut statics = draw::Statics::default();
    // Ground only (props are measured one by one below), with the meadow's
    // grass patches laid out clear of the full prop layout, as drawn.
    let full_layout = scenery::layout(track, verge, false);
    draw::build_statics_parts(track, &mats, verge, &[], &full_layout, true, &mut statics);
    let ground = Ground::new(&statics);
    let mut objects = Vec::new();

    // Colours of the field.
    let me = r.player_kart(0).expect("one local player");
    let spec = |k: usize| &r.roster[r.karts[k].spec];
    let player = spec(me);
    let others: Vec<Value> = (0..r.karts.len()).filter(|&k| k != me).map(|k| json!({ "name": spec(k).name, "hex": srgb_hex(spec(k).color) })).collect();
    let colours = json!({
        "player": { "name": player.name, "hex": srgb_hex(player.color), "marker": MARKER },
        "others": others,
    });

    // Karts on the grid; on Low each needs a blob under it.
    let mut low = RenderWorld::default();
    draw::push_blobs(&r, 1.0, &mut low);
    for (i, k) in r.karts.iter().enumerate() {
        let mut rw = RenderWorld::default();
        let s = &r.roster[k.spec];
        draw::draw_kart(k, (s.color, s.suit), k.pos, k.yaw + k.slip + k.spin_angle, c.t.max_speed, &mats, &mut rw);
        let lit = rw.meshes.iter().any(|m| !emissive(m.material));
        let blob = low.blobs.iter().any(|b| {
            let g = ground.height(b[0], b[2]).unwrap_or(b[1]);
            (b[0] - k.pos[0]).hypot(b[2] - k.pos[2]) < 0.6 && (b[1] - g).abs() < 0.1
        });
        objects.push(object(format!("kart{i} {}", s.name), ground.gap(&rw.meshes), lit && blob, None));
    }

    // Item boxes over a whole bob and turn (4 s): the lowest reach.
    let look = draw::ItemBoxLook::default();
    let ring = crate::roadmarks::mesh_rings(track, verge);
    for (i, b) in r.boxes.iter().enumerate() {
        let seat = draw::BoxSeat::new(track, &ring, b.pos);
        let (mut gap, mut lit, mut disc) = (f64::MAX, true, true);
        for f in 0..240 {
            let time = f as f32 / 60.0;
            let mut rw = RenderWorld::default();
            draw::draw_item_box(b, i, &look, seat.as_ref(), &mats, time, true, &mut rw);
            gap = gap.min(ground.gap(&rw.meshes));
            lit &= rw.meshes.iter().any(|m| !emissive(m.material));
            let mut rl = RenderWorld::default();
            draw::draw_item_box(b, i, &look, seat.as_ref(), &mats, time, false, &mut rl);
            let fp = footprint(&rw.meshes);
            // Low's road disc is a decal under the box.
            disc &= !rl.decals.is_empty() && rl.decals.iter().all(|m| inside(fp, m.position, 0.0));
        }
        objects.push(object(format!("item_box{i}"), gap, lit && disc, Some("hovering pickup reads from far away (spins and bobs over the road)")));
    }

    // Scenery props: the full layout (Medium and up) and the Low layout.
    let full = scenery::layout(track, verge, false);
    let thin = scenery::layout(track, verge, true);
    let terrain = |k: PropKind| matches!(k, PropKind::Mountain | PropKind::Hill | PropKind::Cloud);
    for t in thin.iter().filter(|t| !terrain(t.kind)) {
        assert!(full.iter().any(|p| p.kind == t.kind && p.pos == t.pos), "a Low prop not in the full layout: {t:?}");
    }
    let mut n: HashMap<PropKind, usize> = HashMap::new();
    for p in &full {
        // Terrain and sky, not objects on the ground.
        if terrain(p.kind) {
            continue;
        }
        let mut parts = Vec::new();
        p.instances(track, &mats, true, &mut parts);
        let lit = parts.iter().any(|m| !emissive(m.material));
        let disc = match thin.iter().find(|t| t.kind == p.kind && t.pos == p.pos) {
            None => true,
            Some(t) => {
                let (mut lit_low, mut with) = (Vec::new(), Vec::new());
                t.instances(track, &mats, true, &mut lit_low);
                t.instances(track, &mats, false, &mut with);
                let fp = footprint(&lit_low);
                let discs: Vec<_> = with.iter().filter(|m| !lit_low.contains(m)).collect();
                !discs.is_empty() && discs.iter().all(|m| inside(fp, m.position, 0.3))
            }
        };
        let k = n.entry(p.kind).or_default();
        let name = format!("{:?}{} at ({:.0}, {:.0})", p.kind, *k, p.pos[0], p.pos[2]);
        *k += 1;
        let why = (p.kind == PropKind::Grandstand).then_some("stepped stand on the bank's slope: its steps reach below the bank's foot on purpose, so no gap shows under them");
        objects.push(object(name, ground.gap(&parts), lit && disc, why));
    }

    let dump = json!({ "colours": colours, "objects": objects });
    if let Ok(dir) = std::env::var("EVANI_REVIEW_DUMP") {
        std::fs::create_dir_all(&dir).expect("dump folder");
        let path = std::path::Path::new(&dir).join("review.json");
        std::fs::write(&path, serde_json::to_string_pretty(&dump).expect("json")).expect("write review.json");
        eprintln!("review_dump: wrote {}", path.display());
    }

    // The kit's own bar: the review's.
    let bad: Vec<String> = dump["objects"]
        .as_array()
        .expect("objects")
        .iter()
        .filter(|o| {
            let gap = o["gap"].as_f64().unwrap_or(f64::MAX);
            !o["contact_shadow"].as_bool().unwrap_or(false) || (o.get("intentional").is_none() && gap.abs() > GAP_OK)
        })
        .map(|o| format!("{} gap {} shadow {}", o["name"], o["gap"], o["contact_shadow"]))
        .collect();
    eprintln!("review_dump: {} objects, {} not grounded", dump["objects"].as_array().map_or(0, Vec::len), bad.len());
    assert!(bad.is_empty(), "not grounded ({}): {}", bad.len(), bad.iter().take(30).cloned().collect::<Vec<_>>().join("; "));
}
