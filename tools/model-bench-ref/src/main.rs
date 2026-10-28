//! `model-bench-ref`: makes the Evani.Model benchmark references
//! (tests/model_bench/README.md, plan-m1.md task T2).
//!
//! For the 8 Sugarpeak karts it captures our own kart racer's kart at rest
//! (`Kart::new` + `draw_kart`, 34 instances of the renderer's built-in
//! meshes), splits off the seated driver (instances 15 to 21), applies each
//! rival's shape overrides from the manifest (decision D4, option B) and
//! writes, per item: `mesh.glb` (the instances as drawn), front, side and top
//! masks in the item's fixed frame and a chase mask from the kit's default
//! chase camera (8-bit greyscale PNG, 512 px, our own writer), and
//! `ref.json` (provenance with the SHA-256 of every file).
//!
//! The 22 non-kart references (primitive assemblies from the manifest's
//! descriptions) and the outer-surface point sets for Chamfer distance are
//! the rest of task T2 (M2 needs them first).
//!
//! Usage: `model-bench-ref [--out tests/model_bench/ref]`

use evani_kit_kart_racer::draw::{Mat, draw_kart};
use evani_kit_kart_racer::kart::Kart;
use evani_model::mesh::Mesh;
use evani_model::png;
use evani_model::preview::{self, Camera, Proj};
use evani_model::vec::*;
use evani_render::{MeshId, RenderWorld};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub const SIZE: usize = 512;

/// One drawn instance (kart frame: y up, nose +z, model's left +x).
#[derive(Clone, Debug)]
pub struct Inst {
    pub mesh: u16,
    pub pos: V3,
    pub rot: Quat,
    pub scale: V3,
    /// Part of the seated driver (instances 15 to 21 of the capture).
    pub driver: bool,
}

/// The kart at rest, as `draw_kart` draws it: 34 instances.
pub fn capture() -> Result<Vec<Inst>, String> {
    let kart = Kart::new(0, None, [0.0, 0.0, 0.0], 0.0);
    let at_rest = kart.speed == 0.0 && kart.steer_vis == 0.0 && kart.drift == 0.0 && kart.boost == 0.0 && kart.hop == 0.0 && kart.squash == 0.0 && kart.wheel_roll == 0.0 && kart.vy == 0.0;
    if !at_rest {
        return Err("Kart::new is no longer at rest; update the capture".into());
    }
    let mats = [Mat { index: 0, tint: [1.0; 3] }; 16];
    let mut rw = RenderWorld::default();
    draw_kart(&kart, ([1.0, 1.0, 1.0], None), [0.0, 0.0, 0.0], 0.0, 1.0, &mats, &mut rw);
    if rw.meshes.len() != 34 {
        return Err(format!("draw_kart drew {} instances, the manifest lists 34; update tests/model_bench/manifest.json", rw.meshes.len()));
    }
    Ok(rw
        .meshes
        .iter()
        .enumerate()
        .map(|(i, m)| Inst {
            mesh: m.mesh.0,
            pos: m.position.map(f64::from),
            rot: m.rotation.map(f64::from),
            scale: m.scale.map(f64::from),
            driver: (15..=21).contains(&i),
        })
        .collect())
}

fn pitch(a: f64) -> Quat {
    quat_axis([1.0, 0.0, 0.0], a)
}

/// A wheel as draw_kart draws one at rest: tyre, rim, hub bar.
fn wheel(x: f64, z: f64, r: f64, w: f64) -> [Inst; 3] {
    let tyre_q = quat_axis([0.0, 0.0, 1.0], core::f64::consts::FRAC_PI_2);
    let c = [x, r, z];
    [
        Inst { mesh: MeshId::CYLINDER.0, pos: c, rot: tyre_q, scale: [2.0 * r, w, 2.0 * r], driver: false },
        Inst { mesh: MeshId::CYLINDER.0, pos: c, rot: tyre_q, scale: [1.2 * r, w + 0.04, 1.2 * r], driver: false },
        Inst { mesh: MeshId::BEVEL_CUBE.0, pos: c, rot: QID, scale: [w + 0.09, 1.25 * r, 0.24 * r], driver: false },
    ]
}

/// The rival shape overrides of tests/model_bench/manifest.json, as data
/// edits on the captured instances (order: manifest draw_order).
pub fn apply_overrides(id: &str, k: &mut Vec<Inst>) {
    let sphere = MeshId::SPHERE.0;
    match id {
        "kart_02_mo" => {
            k[0].scale[1] = 0.42;
            k[0].pos[1] = 0.42;
            k[1].rot = pitch(0.1);
            k[1].scale[1] = 0.42;
            k[1].mesh = sphere;
            k[2].mesh = sphere;
            k[3].mesh = sphere;
        }
        "kart_03_taffy" => {
            k[0].pos[1] = 0.3;
            k[0].scale[2] = 2.6;
            k[1].pos[2] = 1.12;
            k[1].scale[2] = 1.15;
            k[1].rot = pitch(0.12);
            k[4].pos[2] = 1.62;
            k[10].scale[0] = 1.9;
            k[10].pos[1] = 1.3;
            k[7].scale[1] = 0.54;
            k[9].scale[1] = 0.54;
        }
        "kart_04_bramble" => {
            k[0].scale[0] = 1.45;
            k[0].scale[1] = 0.38;
            for (i, x) in [(2usize, 0.82f64), (3, -0.82)] {
                k[i].pos[0] = x;
                k[i].scale = [0.44, 0.48, 0.98];
            }
            k[10].scale[0] = 1.1;
            k[10].scale[2] = 0.3;
            k[4].scale[1] = 0.32;
            k[4].scale[2] = 0.36;
        }
        "kart_05_zuzu" => {
            k[1].scale[0] = 0.7;
            k[1].scale[2] = 1.0;
            k[1].rot = pitch(0.35);
            k[2].rot = quat_axis([0.0, 1.0, 0.0], -0.25);
            k[3].rot = quat_axis([0.0, 1.0, 0.0], 0.25);
            for i in [6, 8] {
                k[i].scale[1] = 0.7;
                k[i].rot = pitch(core::f64::consts::FRAC_PI_2 - 0.6);
            }
        }
        "kart_06_nib" => {
            for inst in k.iter_mut().take(15) {
                inst.pos[2] *= 0.85;
            }
            k[0].scale[0] = 1.15;
            k[10].scale[0] = 1.15;
            let fr = wheel(0.8 * 0.9, 0.81, 0.33, 0.32);
            let fl = wheel(-0.8 * 0.9, 0.81, 0.33, 0.32);
            let rr = wheel(0.84 * 0.9, -0.73, 0.35, 0.36);
            let rl = wheel(-0.84 * 0.9, -0.73, 0.35, 0.36);
            for (base, w) in [(22usize, fr), (25, fl), (28, rr), (31, rl)] {
                for (j, inst) in w.into_iter().enumerate() {
                    k[base + j] = inst;
                }
            }
        }
        "kart_07_clover" => {
            k[5].mesh = sphere;
            k[5].scale = [1.0, 0.6, 0.7];
            k[5].pos[1] = 0.68;
            for i in [2, 3] {
                k[i].mesh = sphere;
                k[i].scale = [0.4, 0.42, 0.8];
            }
            // Remove the wing and its struts (7, 9, 10), from the back.
            for i in [10usize, 9, 7] {
                k.remove(i);
            }
        }
        "kart_08_rumble" => {
            let rr = wheel(0.9, -0.86, 0.52, 0.56);
            let rl = wheel(-0.9, -0.86, 0.52, 0.56);
            for (base, w) in [(28usize, rr), (31, rl)] {
                for (j, inst) in w.into_iter().enumerate() {
                    k[base + j] = inst;
                }
            }
            k[5].scale = [1.15, 0.56, 0.6];
            k[5].pos[1] = 0.66;
            k[4].scale[1] = 0.3;
            let ex = k[6].clone();
            k[6].pos[0] = 0.18;
            k[8].pos[0] = -0.18;
            k[6].scale[1] = 0.36;
            k[8].scale[1] = 0.36;
            for x in [0.42, -0.42] {
                let mut e = ex.clone();
                e.pos[0] = x;
                e.scale[1] = 0.36;
                k.push(e);
            }
        }
        _ => {}
    }
}

/// Triangles of the instances (each a closed convex built-in mesh).
pub fn to_mesh(insts: &[Inst]) -> Result<Mesh, String> {
    let mut out = Mesh::default();
    for i in insts {
        let b = evani_render::mesh::builtin(MeshId(i.mesh)).ok_or(format!("no built-in mesh {}", i.mesh))?;
        let base = out.pos.len() as u32;
        for v in &b.vertices {
            let p = [v[0] as f64 * i.scale[0], v[1] as f64 * i.scale[1], v[2] as f64 * i.scale[2]];
            out.pos.push(add(qrot(i.rot, p), i.pos));
            out.nrm.push(qrot(i.rot, [v[3] as f64, v[4] as f64, v[5] as f64]));
        }
        for t in b.indices.chunks(3) {
            out.tris.push([base + t[0], base + t[1], base + t[2]]);
            out.tri_part.push(0);
        }
    }
    Ok(out)
}

/// The item's fixed views (manifest `frame_centre`): horizontally on the
/// reference's box centre, the ground 5% above the bottom edge.
pub fn cameras(b: &Aabb, frame: f64, chase: bool) -> Vec<(&'static str, Camera)> {
    let c = b.centre();
    let centre = [c[0], 0.45 * frame, c[2]];
    let mut v = vec![("front", Camera::named("front", centre, frame)), ("side", Camera::named("side", centre, frame))];
    let mut top = Camera::named("top", [c[0], 0.0, c[2]], frame);
    top.centre = [c[0], 0.0, c[2]];
    v.push(("top", top));
    if chase {
        // The kit's default chase pose: 7 m behind, 3 m up, looking at a
        // point 4 m ahead and 1 m up, 62 degree vertical field of view.
        let eye = [0.0, 3.0, -7.0];
        let target = [0.0, 1.0, 4.0];
        v.push(("chase", Camera { eye, fwd: sub(target, eye), up: [0.0, 1.0, 0.0], proj: Proj::Persp { fov_deg: 62.0 }, centre: target }));
    }
    v
}

fn git_commit(path: &str) -> String {
    std::process::Command::new("git")
        .args(["log", "-1", "--format=%H", "--", path])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// Write one item's references; returns its provenance record.
pub fn write_item(dir: &Path, id: &str, insts: &[Inst], frame: f64, chase: bool, overrides: &Value, commit: &str) -> Result<Value, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mesh = to_mesh(insts)?;
    let b = mesh.bounds();
    let mut files = serde_json::Map::new();
    let glb = evani_model::gltf::write_plain(&[(id.to_string(), &mesh)], &json!({ "evani_reference": id }));
    std::fs::write(dir.join("mesh.glb"), &glb).map_err(|e| e.to_string())?;
    files.insert("mesh.glb".into(), json!(evani_model::sha256::tag(&glb)));
    for (name, cam) in cameras(&b, frame, chase) {
        let m = preview::mask(&mesh, &cam, SIZE, 4);
        let bytes = png::write(&m);
        let f = format!("{name}.png");
        std::fs::write(dir.join(&f), &bytes).map_err(|e| e.to_string())?;
        files.insert(f, json!(evani_model::sha256::tag(&bytes)));
    }
    let s = b.size();
    let rec = json!({
        "id": id,
        "tool": format!("model-bench-ref {}", env!("CARGO_PKG_VERSION")),
        "kit_commit": commit,
        "instances": insts.len(),
        "overrides": overrides,
        "frame_m": frame,
        "frame_centre": [evani_model::gltf::r6(b.centre()[0]), evani_model::gltf::r6(0.45 * frame), evani_model::gltf::r6(b.centre()[2])],
        "size_m": { "length": evani_model::gltf::r6(s[2]), "width": evani_model::gltf::r6(s[0]), "height": evani_model::gltf::r6(s[1]) },
        "chase": if chase { json!({ "eye": [0.0, 3.0, -7.0], "target": [0.0, 1.0, 4.0], "fov_deg": 62.0, "source": "kit.kart_racer.cam_distance 7, cam_height 3, cam_look_ahead 4, cam_fov 62" }) } else { Value::Null },
        "files": Value::Object(files)
    });
    std::fs::write(dir.join("ref.json"), serde_json::to_string_pretty(&rec).map_err(|e| e.to_string())? + "\n").map_err(|e| e.to_string())?;
    Ok(rec)
}

/// The look's shader constants, read from its WGSL body, must equal our
/// ramp data (the drift test of note 21).
pub fn ramp_drift() -> Result<(), String> {
    let body = evani_look_toon::TOON_RAMP_BODY;
    let num = |key: &str| -> Option<f64> {
        let i = body.find(&format!("let {key} = "))? + key.len() + 7;
        body[i..].split(';').next()?.trim().parse().ok()
    };
    let r = preview::toon_ramp();
    let checks = [("band_low", r.band_low), ("band_high", r.band_high), ("softness", r.softness)];
    for (k, v) in checks {
        match num(k) {
            Some(x) if (x - v).abs() < 1e-12 => {}
            other => return Err(format!("toon ramp drift: the look's {k} is {other:?}, data/looks/toon.json has {v}")),
        }
    }
    let tail = format!("return {} + {} * low + {} * high;", r.base, r.mid, r.top);
    if !body.contains(&tail) {
        return Err(format!("toon ramp drift: the look's band values changed (expected `{tail}`)"));
    }
    Ok(())
}

pub fn run(out: &Path, manifest: &Value) -> Result<Vec<Value>, String> {
    ramp_drift()?;
    let base = capture()?;
    let commit = git_commit("kits/evani-kit-kart-racer/src/draw.rs");
    let mut recs = Vec::new();
    for it in manifest["items"].as_array().into_iter().flatten().filter(|i| i["class"] == "kart") {
        let id = it["id"].as_str().unwrap_or("");
        let mut k = base.clone();
        apply_overrides(id, &mut k);
        // The driver is not part of the kart reference.
        let driverless: Vec<Inst> = k.into_iter().filter(|x| !x.driver).collect();
        let frame = it["frame_m"].as_f64().unwrap_or(3.4);
        recs.push(write_item(&out.join(id), id, &driverless, frame, true, &it["reference"]["overrides"], &commit)?);
    }
    // The seated driver, for the karts' `seats` must.
    let driver: Vec<Inst> = base[15..=21].to_vec();
    recs.push(write_item(&out.join("kart_driver"), "kart_driver", &driver, 1.6, false, &json!("instances 15-21 of the base kart"), &commit)?);
    Ok(recs)
}


/// Silhouette IoU of a compiled intent against an item's reference masks
/// (same fixed frame and cameras), per view.
pub fn iou_against_refs(refs: &Path, id: &str, intent_src: &str) -> Result<Vec<(String, f64)>, String> {
    let rec: Value = serde_json::from_str(&std::fs::read_to_string(refs.join(id).join("ref.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let frame = rec["frame_m"].as_f64().unwrap_or(3.4);
    let c: Vec<f64> = rec["frame_centre"].as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
    let refbox = Aabb::centred([c[0], 0.5, c[2]], [0.1; 3]);
    let j = evani_model::intent::read(intent_src).map_err(|d| format!("{d:?}"))?;
    let tiers = evani_model::compile::compile(&j, Some(&["ultra".to_string()]), 4).map_err(|d| format!("{d:?}"))?;
    let mesh = tiers.last().ok_or("no tier")?.world_mesh();
    let mut out = Vec::new();
    for (name, cam) in cameras(&refbox, frame, true) {
        let r = png::read(&std::fs::read(refs.join(id).join(format!("{name}.png"))).map_err(|e| e.to_string())?)?;
        let m = preview::mask(&mesh, &cam, SIZE, 4);
        out.push((name.to_string(), preview::iou(&m, &r)));
    }
    Ok(out)
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = args.iter().position(|a| a == "--out").and_then(|i| args.get(i + 1)).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("tests/model_bench/ref"));
    let manifest: Value = match std::fs::read_to_string("tests/model_bench/manifest.json").map_err(|e| e.to_string()).and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string())) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("model-bench-ref: cannot read tests/model_bench/manifest.json ({e}); run from the workspace root");
            return std::process::ExitCode::FAILURE;
        }
    };
    if let Some(i) = args.iter().position(|a| a == "--iou") {
        let id = args.get(i + 1).cloned().unwrap_or_default();
        let file = format!("tests/model_bench/vehicles/{id}.evmodel");
        return match std::fs::read_to_string(&file).map_err(|e| e.to_string()).and_then(|src| iou_against_refs(&out, &id, &src)) {
            Ok(v) => {
                for (view, x) in v {
                    println!("{id} {view} IoU {x:.3}");
                }
                std::process::ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("model-bench-ref: {e}");
                std::process::ExitCode::FAILURE
            }
        };
    }
    match run(&out, &manifest) {
        Ok(recs) => {
            for r in recs {
                println!("{} {} instances, {:?}", r["id"].as_str().unwrap_or(""), r["instances"], r["size_m"]);
            }
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("model-bench-ref: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_kart_matches_the_manifest_size() {
        let k = capture().unwrap();
        let body: Vec<Inst> = k.iter().filter(|x| !x.driver).cloned().collect();
        let s = to_mesh(&body).unwrap().bounds().size();
        assert!((s[2] - 3.106).abs() < 0.01, "length {}", s[2]);
        assert!((s[0] - 2.21).abs() < 0.01, "width {}", s[0]);
        assert!((s[1] - 1.264).abs() < 0.01, "height {}", s[1]);
        let all = to_mesh(&k).unwrap().bounds().size();
        assert!((all[1] - 2.14).abs() < 0.01, "with driver {}", all[1]);
    }

    #[test]
    fn masks_are_symmetric_and_repeatable() {
        let k = capture().unwrap();
        let body: Vec<Inst> = k.iter().filter(|x| !x.driver).cloned().collect();
        let m = to_mesh(&body).unwrap();
        let cams = cameras(&m.bounds(), 3.4, true);
        let front = preview::mask(&m, &cams[0].1, SIZE, 4);
        let mut asym = 0;
        for y in 0..SIZE {
            for x in 0..SIZE / 2 {
                if front.get(x, y) != front.get(SIZE - 1 - x, y) {
                    asym += 1;
                }
            }
        }
        assert!(asym < SIZE, "{asym} asymmetric pixels");
        assert_eq!(png::write(&front), png::write(&preview::mask(&m, &cams[0].1, SIZE, 1)));
    }

    /// Task T8a's sanity check: the base kart intent, compiled at range
    /// centres without search, within IoU 0.85 of the captured kart per view.
    #[test]
    fn base_kart_intent_is_within_085_iou_of_the_reference() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/model_bench");
        let src = std::fs::read_to_string(root.join("vehicles/kart_01_pip.evmodel")).unwrap();
        for (view, x) in iou_against_refs(&root.join("ref"), "kart_01_pip", &src).unwrap() {
            assert!(x >= 0.85, "{view}: IoU {x:.3}");
        }
    }

    #[test]
    fn the_toon_ramp_has_not_drifted() {
        ramp_drift().unwrap();
    }
}
