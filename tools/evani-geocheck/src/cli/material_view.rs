//! The material viewer adapter: the viewer's scenes are drawn by its app
//! crate (a binary), so the app writes them out itself: its `geocheck_dump`
//! test stages every target-pack moment natively (as `capture.frame
//! {moment}` does) and writes `dump.json`: per scene the mesh draw list, the
//! decals, each model instance with its highest-detail triangles, the
//! moment's camera and four views round the scene's home orbit.
//!
//! - Every scene is checked on its own (the scenes all stand at the origin
//!   and are never drawn together): its draw list is its static world, the
//!   models become custom meshes of the checker's library.
//! - Nothing moves while a moment is held (the turntable is off), so there
//!   are no movers; collision does not apply (a viewer has no player).

use std::path::Path;
use std::process::Command;

use evani_core::camera::CameraPose;
use evani_render::mesh::{Mesh, MeshId, MeshLibrary};
use evani_render::world::MeshInstance;
use serde_json::Value;

/// One scene of the dump.
pub struct SceneDump {
    /// The moment id (`styles`, `ball`, ...).
    pub id: String,
    pub camera: CameraPose,
    pub views: Vec<CameraPose>,
    /// Meshes, then decals from `decal_from`, then the models.
    pub instances: Vec<MeshInstance>,
    pub decal_from: usize,
    /// Sky dome instances left out of the solid world.
    pub sky_left_out: usize,
}

/// The index of a look material by name.
fn materials_index(v: &Value, name: &str) -> Option<u32> {
    v["materials"].as_array()?.iter().position(|m| m.as_str() == Some(name)).map(|i| i as u32)
}

/// The dump read back.
pub struct Dump {
    pub scenes: Vec<SceneDump>,
    pub materials: Vec<String>,
    pub library: MeshLibrary,
    /// Custom mesh ids and the model each was read from.
    pub models: Vec<(MeshId, String)>,
}

pub const ASPECT: f64 = 16.0 / 9.0;
pub const VIEWPORT_H: f64 = 1080.0;

fn f3(v: &Value) -> [f32; 3] {
    let a = |i: usize| v.get(i).and_then(Value::as_f64).unwrap_or(0.0) as f32;
    [a(0), a(1), a(2)]
}

fn f4(v: &Value) -> [f32; 4] {
    let a = |i: usize| v.get(i).and_then(Value::as_f64).unwrap_or(0.0) as f32;
    [a(0), a(1), a(2), a(3)]
}

fn pose(v: &Value) -> CameraPose {
    CameraPose { eye: f3(&v["eye"]), target: f3(&v["target"]), up: f3(&v["up"]), fov_y_deg: v["fov_y_deg"].as_f64().unwrap_or(50.0) as f32 }
}

/// Area-weighted vertex normals (the checker reads positions; mesh
/// validation compares the winding with these).
fn mesh_of(pos: &[f32], idx: &[u32]) -> Mesh {
    let n = pos.len() / 3;
    let mut nor = vec![[0.0f32; 3]; n];
    for t in idx.as_chunks::<3>().0 {
        let p = |i: u32| {
            let i = i as usize * 3;
            [pos[i], pos[i + 1], pos[i + 2]]
        };
        let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
        let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let f = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
        for &v in t {
            for k in 0..3 {
                nor[v as usize][k] += f[k];
            }
        }
    }
    let vertices = (0..n)
        .map(|i| {
            let l = (nor[i][0] * nor[i][0] + nor[i][1] * nor[i][1] + nor[i][2] * nor[i][2]).sqrt().max(1e-12);
            [pos[i * 3], pos[i * 3 + 1], pos[i * 3 + 2], nor[i][0] / l, nor[i][1] / l, nor[i][2] / l]
        })
        .collect();
    Mesh { vertices, indices: idx.to_vec() }
}

/// Run the app's dump test and read the dump.
pub fn record(root: &Path, out: &Path) -> Result<Dump, String> {
    let dir = out.join("native");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    eprintln!("geocheck: staging every material-view scene natively (cargo test -p evani-material-view geocheck_dump)");
    let status = Command::new(env!("CARGO"))
        .args(["test", "-q", "-p", "evani-material-view", "geocheck_dump", "--", "--exact", "tests::geocheck_dump"])
        .env("EVANI_GEOCHECK_DUMP", &dir)
        .current_dir(root)
        .status()
        .map_err(|e| format!("cargo test: {e}"))?;
    if !status.success() {
        return Err(format!("the material-view dump test failed ({status})"));
    }
    let text = std::fs::read_to_string(dir.join("dump.json")).map_err(|e| format!("the dump test wrote no dump.json: {e}"))?;
    parse(&text)
}

/// Read `dump.json`.
pub fn parse(text: &str) -> Result<Dump, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("dump.json: {e}"))?;
    let materials = v["materials"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect();
    let mut library = MeshLibrary::default();
    // Each model shape is one custom mesh per part (mesh node), so a part is
    // validated on its own and the parts of one model form one object.
    let mut ids: Vec<(String, Vec<MeshId>)> = Vec::new();
    let mut models = Vec::new();
    if let Some(shapes) = v["shapes"].as_object() {
        for (key, s) in shapes {
            let name = key.split('#').next().unwrap_or(key).rsplit('/').next().unwrap_or(key).trim_end_matches(".glb").to_string();
            let name = if name.is_empty() { key.clone() } else { name };
            let mut part_ids = Vec::new();
            for (k, part) in s["parts"].as_array().into_iter().flatten().enumerate() {
                let pos: Vec<f32> = part["positions"].as_array().into_iter().flatten().filter_map(Value::as_f64).map(|x| x as f32).collect();
                let idx: Vec<u32> = part["indices"].as_array().into_iter().flatten().filter_map(Value::as_u64).map(|x| x as u32).collect();
                if pos.is_empty() || idx.is_empty() {
                    continue;
                }
                let id = library.add(mesh_of(&pos, &idx)).ok_or("the checker's mesh library is full")?;
                part_ids.push(id);
                let mat = part["material"].as_str().unwrap_or("");
                models.push((id, if mat.is_empty() { format!("{name} part {k}") } else { format!("{name} part {k} ({mat})") }));
            }
            ids.push((key.clone(), part_ids));
        }
    }
    let sky = materials_index(&v, "sky");
    let mut scenes = Vec::new();
    for s in v["scenes"].as_array().into_iter().flatten() {
        let mut instances: Vec<MeshInstance> = s["meshes"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|m| {
                let mut i = MeshInstance::new(MeshId(m[0].as_u64().unwrap_or(0) as u16), f3(&m[1]));
                i.rotation = f4(&m[2]);
                i.scale = f3(&m[3]);
                i.material = m[4].as_u64().unwrap_or(0) as u32;
                i.color = f4(&m[5]);
                i
            })
            .collect();
        let decal_from = s["decal_from"].as_u64().unwrap_or(instances.len() as u64) as usize;
        let (meshes, decals) = instances.split_at(decal_from.min(instances.len()));
        let (meshes, decals) = (meshes.to_vec(), decals.to_vec());
        // The procedural sky dome (the look's `sky` material) is drawn as a
        // sphere round the camera: the backdrop, not world geometry.
        let before = meshes.len();
        let mut list: Vec<MeshInstance> = meshes.into_iter().filter(|m| Some(m.material) != sky).collect();
        let sky_left_out = before - list.len();
        for m in s["models"].as_array().into_iter().flatten() {
            let Some(parts) = ids.iter().find(|(k, _)| Some(k.as_str()) == m["shape"].as_str()).map(|(_, p)| p.clone()) else { continue };
            for id in parts {
                let mut i = MeshInstance::new(id, f3(&m["position"]));
                i.rotation = f4(&m["rotation"]);
                i.scale = f3(&m["scale"]);
                list.push(i);
            }
        }
        // Decals last, so `decal_from` still marks them.
        let decal_from = list.len();
        list.extend(decals);
        instances = list;
        scenes.push(SceneDump {
            id: s["id"].as_str().unwrap_or("?").to_string(),
            camera: pose(&s["camera"]),
            views: s["views"].as_array().into_iter().flatten().map(pose).collect(),
            instances,
            decal_from,
            sky_left_out,
        });
    }
    if scenes.is_empty() {
        return Err("dump.json has no scenes".into());
    }
    Ok(Dump { scenes, materials, library, models })
}

/// The label of an instance: the model's file name for a model, else the
/// look material and the built-in mesh.
pub fn label(m: &MeshInstance, dump: &Dump, scene: &str) -> String {
    let base = match dump.models.iter().find(|(id, _)| *id == m.mesh) {
        Some((_, name)) => format!("model {name}"),
        None => evani_geocheck::scene::default_label(m, &dump.materials),
    };
    format!("{scene}/{base}")
}

/// The capture plan step that opens a scene in the live game: its moment
/// (the clock is let go again afterwards, the scene stays).
pub fn open_step(id: &str) -> Value {
    serde_json::json!({
        "name": format!("open-{id}"), "cmd": "capture.frame",
        "params": { "width": 64, "moment": id },
        "note": format!("Geometry check: open the {id} scene"), "timeout_ms": 60000
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A dump with one scene, a floor and one model instance reads back
    /// with the model as a custom mesh after the meshes and the decal last.
    #[test]
    fn a_dump_reads_back() {
        let text = r#"{"materials": ["ground"], "shapes": {"models/ball.glb#3": {"parts": [{"positions": [0,0,0, 1,0,0, 0,1,0], "indices": [0,1,2]}]}},
          "scenes": [{"id": "ball", "camera": {"eye": [0,1,5], "target": [0,0,0], "up": [0,1,0], "fov_y_deg": 50},
            "views": [{"eye": [5,1,0], "target": [0,0,0], "up": [0,1,0], "fov_y_deg": 50}],
            "meshes": [[2, [0,0,0], [0,0,0,1], [10,1,10], 0, [1,1,1,1]], [2, [0,0.01,0], [0,0,0,1], [1,1,1], 0, [1,1,1,1]]],
            "decal_from": 1, "models": [{"shape": "models/ball.glb#3", "position": [0,0.5,0], "rotation": [0,0,0,1], "scale": [1,1,1]}]}]}"#;
        let d = parse(text).unwrap();
        assert_eq!(d.scenes.len(), 1);
        let s = &d.scenes[0];
        assert_eq!(s.instances.len(), 3);
        assert_eq!(s.decal_from, 2);
        assert_eq!(s.instances[1].mesh, d.models[0].0);
        assert_eq!(d.models[0].1, "ball part 0");
        assert_eq!(label(&s.instances[1], &d, "ball"), "ball/model ball part 0");
        assert_eq!(s.views.len(), 1);
    }
}
