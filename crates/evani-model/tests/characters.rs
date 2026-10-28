//! MC1: the frontier marshal, a code-built character in a T-pose
//! (plan-characters.md section 9). It compiles on all four tiers within
//! budget with clean meshes and every must passing, writes identical bytes
//! on 1 and 8 threads, and its glTF carries the skeleton, the anchors on
//! their joints, skin weights (1 to 4 per vertex, summing to 1, hands on
//! the hand joints) and per-vertex region colours.

use evani_model::api::{self, Workspace};
use evani_model::gltf;
use std::path::{Path, PathBuf};

fn marshal() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/model_bench/characters/frontier_marshal.evmodel")
}

fn scratch(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("evani-model-characters-{}-{tag}", std::process::id()))
}

#[test]
fn the_marshal_compiles_on_every_tier_within_budget_clean_and_passing_its_musts() {
    let root = scratch("tiers");
    let ws = Workspace::new(&root);
    let j = ws.read_intent(&marshal().to_string_lossy()).unwrap_or_else(|d| panic!("{}", api::error_json(&d)));
    let model = evani_model::build::build(&j, 0.0).unwrap_or_else(|d| panic!("{}", api::error_json(&d)));
    let tiers = evani_model::compile::compile(&j, None, 4).unwrap_or_else(|d| panic!("{}", api::error_json(&d)));
    assert_eq!(tiers.iter().map(|t| t.tier.name.as_str()).collect::<Vec<_>>(), ["low", "medium", "high", "ultra"]);
    let rs = api::musts(&model, &tiers, 4);
    for r in &rs {
        assert!(r.pass, "must failed: {}", r.to_json());
    }
    for t in &tiers {
        eprintln!("{}: {} tris of {}, cell {:.4} m, min feature {:.4} m, dropped {}", t.tier.name, t.tris, t.tier.budget, t.h, t.min_feature, t.dropped.len());
        assert!(t.tris <= t.tier.budget, "{} over budget: {}", t.tier.name, t.tris);
        for s in &t.solids {
            assert!(s.report.ok(), "{} {}: {:?}", t.tier.name, s.solid, s.report.summary());
            assert!(s.report.watertight() && s.report.manifold());
        }
    }
    // The fringe reads at ultra; lower tiers drop it (envelopes are MC2).
    let ultra = &tiers[3];
    assert!(ultra.dropped.iter().all(|d| !d.0.starts_with("fringe")), "{:?}", ultra.dropped);
    // The coat's thickness at its check points, within 30% of the target
    // and never a ray that ran out at the cap.
    let th = evani_model::build::garment_thickness(&model);
    assert_eq!(th.len(), 3, "{th:?}");
    for t in &th {
        assert!(t.pass && !t.capped, "{} {}: {:.4} m against {:.4} m ({t:?})", t.part, t.name, t.got, t.target);
    }
    let _ = std::fs::remove_dir_all(&root);
}

fn compile(threads: usize, tag: &str) -> (serde_json::Value, Vec<(String, Vec<u8>)>) {
    let root = scratch(tag);
    let out = root.join("out");
    let _ = std::fs::remove_dir_all(&root);
    let mut ws = Workspace::new(&root.join("ws"));
    ws.threads = threads;
    let only = ["low".to_string(), "ultra".to_string()];
    let r = match api::compile_model(&ws, &marshal().to_string_lossy(), Some(&only), Some(&out)) {
        Ok(r) => r,
        Err(d) => panic!("marshal compile on {threads} threads failed: {}", api::error_json(&d)),
    };
    let Ok(rd) = std::fs::read_dir(&out) else { panic!("no output folder {}", out.display()) };
    let mut files: Vec<(String, Vec<u8>)> = rd
        .flatten()
        .map(|e| {
            let p = e.path();
            let name = p.file_name().and_then(|x| x.to_str()).unwrap_or("?").to_string();
            (name, std::fs::read(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display())))
        })
        .collect();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let _ = std::fs::remove_dir_all(&root);
    (r, files)
}

#[test]
fn the_marshal_is_deterministic_and_its_gltf_is_skinned_coloured_and_anchored() {
    let (r1, f1) = compile(1, "t1");
    let (r8, f8) = compile(8, "t8");
    let names: Vec<&str> = f1.iter().map(|f| f.0.as_str()).collect();
    assert_eq!(names, f8.iter().map(|f| f.0.as_str()).collect::<Vec<_>>());
    assert_eq!(names.iter().filter(|n| n.ends_with(".glb")).count(), 2, "{names:?}");
    for ((n, a), (_, b)) in f1.iter().zip(&f8) {
        assert!(a == b, "{n} differs between 1 and 8 threads ({} and {} bytes)", a.len(), b.len());
    }
    assert_eq!(r1["hash"], r8["hash"]);
    // The thickness report is in the compile result and the metadata.
    let th = &r1["checks"]["garment_thickness"];
    assert_eq!(th["points"].as_array().map(Vec::len), Some(3), "{th}");
    assert_eq!(th["flagged"], 0, "{th}");
    // Normal maps agree across chart seams (the M1 bar for the kart).
    let nm = &r1["checks"]["normal_maps"];
    assert!(nm["seam_max_deg"].as_f64().is_some_and(|d| d < 2.0), "{nm}");
    let meta = f1.iter().find(|f| f.0.ends_with(".evmodel.json")).map(|f| String::from_utf8_lossy(&f.1).to_string()).unwrap_or_default();
    assert!(meta.contains("garment_thickness"));

    let joint_names = ["root", "hips", "spine", "chest", "neck", "head", "clavicle_l", "clavicle_r", "upperarm_l", "upperarm_r", "forearm_l", "forearm_r", "hand_l", "hand_r", "thigh_l", "thigh_r", "shin_l", "shin_r", "foot_l", "foot_r", "toe_l", "toe_r"];
    for (n, bytes) in f1.iter().filter(|f| f.0.ends_with(".glb")) {
        let rb = gltf::read(bytes).unwrap_or_else(|e| panic!("{n}: {e}"));
        // Joints with their rest positions; the left hand on +x.
        for jn in joint_names {
            assert!(rb.joints.iter().any(|j| j.0 == jn), "{n}: joint {jn} missing");
        }
        let hand_l = rb.joints.iter().find(|j| j.0 == "hand_l").map(|j| j.1).unwrap_or([0.0; 3]);
        assert!((hand_l[0] - 0.72).abs() < 1e-4 && (hand_l[1] - 1.45).abs() < 1e-4, "{n}: hand_l at {hand_l:?}");
        // Anchors come back in world space, riding on their joints.
        for a in ["hand_l", "hand_r", "seat", "head_look", "head_top", "foot_l", "foot_r"] {
            assert!(rb.anchors.iter().any(|x| x.0 == a), "{n}: anchor {a} missing");
        }
        let anchor = |a: &str| rb.anchors.iter().find(|x| x.0 == a).map(|x| x.1).unwrap_or([0.0; 3]);
        assert!((anchor("hand_l")[0] - 0.80).abs() < 1e-4 && (anchor("hand_r")[0] + 0.80).abs() < 1e-4, "{n}");
        assert!((anchor("head_top")[1] - 1.895).abs() < 1e-4, "{n}");
        // The skin's joint list, by name.
        let all_nodes = rb.json["nodes"].as_array().cloned().unwrap_or_default();
        let skin: Vec<String> = rb.json["skins"][0]["joints"].as_array().into_iter().flatten().filter_map(|v| v.as_u64()).map(|i| all_nodes[i as usize]["name"].as_str().unwrap_or("").to_string()).collect();
        assert_eq!(skin.len(), joint_names.len(), "{n}");
        assert!(!rb.instances.is_empty() && rb.influences.len() == rb.instances.len(), "{n}");
        let (mut hand_verts, mut hand_ok) = (0usize, 0usize);
        for (inst, inf) in rb.instances.iter().zip(&rb.influences) {
            assert_eq!(inf.len(), inst.1.len(), "{n} {}: one influence set per vertex", inst.0);
            for (v, (js, ws)) in inst.1.iter().zip(inf) {
                let used = ws.iter().filter(|w| **w > 0.0).count();
                assert!((1..=4).contains(&used), "{n}: {used} influences at {v:?}");
                let sum: f64 = ws.iter().sum();
                assert!((sum - 1.0).abs() < 1e-6, "{n}: weights sum {sum} at {v:?}");
                assert!(js.iter().all(|&j| (j as usize) < skin.len()));
                // Past the wrist (hand joints at |x| 0.72), mostly the hand.
                if v[0].abs() > 0.78 {
                    hand_verts += 1;
                    let side = if v[0] > 0.0 { "hand_l" } else { "hand_r" };
                    let w: f64 = js.iter().zip(ws).filter(|(j, _)| skin[**j as usize] == side).map(|(_, w)| *w).sum();
                    if w > 0.5 {
                        hand_ok += 1;
                    } else {
                        eprintln!("{n}: hand vertex {v:?} weights {:?}", js.iter().zip(ws).map(|(j, w)| (skin[*j as usize].clone(), *w)).collect::<Vec<_>>());
                    }
                }
            }
        }
        assert!(hand_verts > 20, "{n}: only {hand_verts} hand vertices");
        assert_eq!(hand_ok, hand_verts, "{n}: hand vertices not mostly on the hand joints");
        // Region colours on every vertex, several distinct ones.
        let mut distinct: Vec<[u64; 3]> = Vec::new();
        for (inst, cols) in rb.instances.iter().zip(&rb.colours) {
            assert_eq!(cols.len(), inst.1.len(), "{n} {}: COLOR_0 on every vertex", inst.0);
            for c in cols {
                let k = [c[0] as u64, c[1] as u64, c[2] as u64];
                if !distinct.contains(&k) {
                    distinct.push(k);
                }
            }
        }
        assert!(distinct.len() >= 8, "{n}: only {} distinct region colours", distinct.len());
        // The skin tone's sRGB #c69071, stored in linear values.
        let lin = [gltf::srgb_to_linear_u8(0xc6), gltf::srgb_to_linear_u8(0x90), gltf::srgb_to_linear_u8(0x71)];
        assert!(distinct.contains(&[lin[0] as u64, lin[1] as u64, lin[2] as u64]), "{n}: no skin colour");
    }
}

#[test]
fn srgb_bytes_convert_to_linear_bytes() {
    assert_eq!(gltf::srgb_to_linear_u8(0), 0);
    assert_eq!(gltf::srgb_to_linear_u8(255), 255);
    // 0.5 sRGB is about 0.214 linear; 10/255 is on the linear toe.
    assert_eq!(gltf::srgb_to_linear_u8(128), 55);
    assert_eq!(gltf::srgb_to_linear_u8(10), 1);
    let mut last = 0;
    for c in 0..=255u8 {
        let l = gltf::srgb_to_linear_u8(c);
        assert!(l >= last && l <= c, "{c} -> {l}");
        last = l;
    }
}
