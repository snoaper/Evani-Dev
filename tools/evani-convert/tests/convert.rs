//! STAGE3_PLAN S3-5's eight converter tests. They run the pinned external
//! programs (`cargo xtask doctor --fix` installs them into .evani/bin); a
//! test that needs them prints why and skips when they are missing. Every
//! converted file is read back with the engine's own reader (evani-gltf), so
//! the converter and the reader are checked against each other.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code: a failed step is a failed test

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use evani_convert::{Options, Outcome, convert, glb::Doc, report, texture, tools::Tools};
use evani_gltf::Gltf;
use serde_json::{Value, json};

fn root() -> PathBuf {
    // Two folders up from tools/evani-convert (not `canonicalize`, which adds
    // Windows' `\\?\` prefix to every path the tests print and compare).
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap().to_path_buf()
}

/// Scratch output (gitignored): the workspace's target folder.
fn scratch(name: &str) -> PathBuf {
    let t = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| root().join("target"));
    let p = std::path::absolute(t).unwrap().join("evani-convert-tests").join(name);
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// `None` (and a printed `SKIPPED` line) when the pinned tools are not
/// installed, so a checkout without `doctor --fix` still runs the rest. With
/// `EVANI_REQUIRE_TOOLS=1` a missing tool fails the test instead: a pass then
/// means the pipeline really ran on the pinned tools.
fn tools_or_skip(test: &str) -> Option<()> {
    match Tools::locate(&root()) {
        Ok(_) => Some(()),
        Err(e) if std::env::var("EVANI_REQUIRE_TOOLS").is_ok_and(|v| v == "1") => {
            panic!("{test}: EVANI_REQUIRE_TOOLS=1 but the converter's tools are missing ({e})")
        }
        Err(e) => {
            println!("SKIPPED {test}: the converter's tools are missing ({e})");
            None
        }
    }
}

// ---------------------------------------------------------------- fixture

/// Closest camera distance written into the fixture (metres).
const FIXTURE_MIN_DISTANCE: f64 = 40.0;
/// Every fixture map is this many pixels square.
const MAP_PX: usize = 256;

fn push_f32(bin: &mut Vec<u8>, v: &[f32]) -> (usize, usize) {
    let off = bin.len();
    for x in v {
        bin.extend_from_slice(&x.to_le_bytes());
    }
    (off, bin.len() - off)
}

fn align4(bin: &mut Vec<u8>) {
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
}

/// A map with a pattern (so the encoders have something real to do).
fn map_png(seed: u8) -> Vec<u8> {
    let mut px = Vec::with_capacity(MAP_PX * MAP_PX * 3);
    for y in 0..MAP_PX {
        for x in 0..MAP_PX {
            let check = ((x / 16 + y / 16) % 2) as u8;
            px.extend_from_slice(&[(x as u8).wrapping_add(seed), (y as u8).wrapping_mul(2), check * 150 + seed / 2]);
        }
    }
    evani_model::png::write_rgb(MAP_PX, MAP_PX, &px)
}

/// A small model with the things the converter must keep: one sphere mesh
/// used by two nodes, a second identical mesh (dedup folds it into the
/// first), an `anchor:seat` node with its own extras, root extras with an
/// author's key beside `evani`, and five material maps (base colour,
/// metal-rough, normal, occlusion, emissive). Built here, not checked in.
fn fixture(dir: &Path) -> PathBuf {
    let (rings, segs) = (12u32, 16u32);
    let (mut pos, mut nrm, mut uv, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for r in 0..=rings {
        let th = std::f32::consts::PI * r as f32 / rings as f32;
        for s in 0..=segs {
            let ph = 2.0 * std::f32::consts::PI * s as f32 / segs as f32;
            let n = [th.sin() * ph.cos(), th.cos(), th.sin() * ph.sin()];
            pos.extend(n.map(|c| c * 0.5));
            nrm.extend(n);
            uv.extend([s as f32 / segs as f32, r as f32 / rings as f32]);
        }
    }
    for r in 0..rings {
        for s in 0..segs {
            let a = (r * (segs + 1) + s) as u16;
            let b = a + segs as u16 + 1;
            idx.extend([a, b, a + 1, a + 1, b, b + 1]);
        }
    }
    let n_verts = pos.len() / 3;
    let mut bin = Vec::new();
    let (po, pl) = push_f32(&mut bin, &pos);
    let (no, nl) = push_f32(&mut bin, &nrm);
    let (uo, ul) = push_f32(&mut bin, &uv);
    let io = bin.len();
    for i in &idx {
        bin.extend_from_slice(&i.to_le_bytes());
    }
    let il = bin.len() - io;
    align4(&mut bin);
    let mut views = vec![
        json!({ "buffer": 0, "byteOffset": po, "byteLength": pl, "target": 34962 }),
        json!({ "buffer": 0, "byteOffset": no, "byteLength": nl, "target": 34962 }),
        json!({ "buffer": 0, "byteOffset": uo, "byteLength": ul, "target": 34962 }),
        json!({ "buffer": 0, "byteOffset": io, "byteLength": il, "target": 34963 }),
    ];
    let mut images = Vec::new();
    for k in 0..5u8 {
        let png = map_png(k * 40);
        let off = bin.len();
        bin.extend_from_slice(&png);
        views.push(json!({ "buffer": 0, "byteOffset": off, "byteLength": png.len() }));
        align4(&mut bin);
        images.push(json!({ "bufferView": views.len() - 1, "mimeType": "image/png" }));
    }
    let prim = json!({ "attributes": { "POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2 }, "indices": 3, "material": 0 });
    let doc = json!({
        "asset": { "version": "2.0", "generator": "evani-convert test fixture" },
        "extras": { "evani": { "format": 1, "class": "fixture", "min_distance_m": FIXTURE_MIN_DISTANCE }, "author_note": "kept as is" },
        "scene": 0,
        "scenes": [{ "nodes": [0] }],
        "nodes": [
            { "name": "fixture", "children": [1, 2, 3, 4] },
            { "name": "wheel_l", "mesh": 0, "translation": [-1.0, 0.0, 0.0] },
            { "name": "wheel_r", "mesh": 0, "translation": [1.0, 0.0, 0.0] },
            { "name": "hub", "mesh": 1, "translation": [0.0, 0.0, 1.0] },
            { "name": "anchor:seat", "translation": [0.0, 0.6, 0.0], "extras": { "seat_height": 0.6 } }
        ],
        "meshes": [{ "name": "ball", "primitives": [prim.clone()] }, { "name": "ball_copy", "primitives": [prim] }],
        "materials": [{
            "name": "painted",
            "pbrMetallicRoughness": { "baseColorTexture": { "index": 0 }, "metallicRoughnessTexture": { "index": 1 } },
            "normalTexture": { "index": 2 },
            "occlusionTexture": { "index": 3 },
            "emissiveTexture": { "index": 4 },
            "emissiveFactor": [1.0, 1.0, 1.0]
        }],
        "textures": [{ "source": 0 }, { "source": 1 }, { "source": 2 }, { "source": 3 }, { "source": 4 }],
        "images": images,
        "accessors": [
            { "bufferView": 0, "componentType": 5126, "count": n_verts, "type": "VEC3", "min": [-0.5, -0.5, -0.5], "max": [0.5, 0.5, 0.5] },
            { "bufferView": 1, "componentType": 5126, "count": n_verts, "type": "VEC3" },
            { "bufferView": 2, "componentType": 5126, "count": n_verts, "type": "VEC2" },
            { "bufferView": 3, "componentType": 5123, "count": idx.len(), "type": "SCALAR" }
        ],
        "bufferViews": views,
        "buffers": [{ "byteLength": bin.len() }]
    });
    let p = dir.join("fixture.glb");
    std::fs::write(&p, Doc { json: doc, bin }.to_glb().unwrap()).unwrap();
    p
}

fn fixture_options(input: &Path, out: &Path) -> Options {
    let mut o = Options::new(&root(), input);
    o.id = Some("fixture".into());
    o.out = Some(out.to_path_buf());
    o
}

/// The fixture converted once with the default tiers; shared by the tests
/// that only read the result (converting is the slow part).
fn converted() -> &'static Outcome {
    static ONE: OnceLock<Outcome> = OnceLock::new();
    ONE.get_or_init(|| {
        let dir = scratch("fixture");
        let input = fixture(&dir);
        convert(&fixture_options(&input, &dir.join("out"))).unwrap()
    })
}

fn read(out: &Path, file: &str) -> Gltf {
    Gltf::from_glb(&std::fs::read(out.join(file)).unwrap()).unwrap_or_else(|e| panic!("{file}: the engine's reader refuses it: {}", e.0))
}

fn tier_files(o: &Outcome) -> Vec<String> {
    o.report["tiers"].as_array().unwrap().iter().map(|t| t["file"].as_str().unwrap().to_owned()).collect()
}

// ------------------------------------------------------------------ tests

#[test]
fn report_lists_every_tier_and_file() {
    let Some(()) = tools_or_skip("report_lists_every_tier_and_file") else { return };
    let o = converted();
    assert!(o.errors.is_empty(), "problems: {:?}", o.errors);
    let r = &o.report;
    let tiers: Vec<&str> = r["tiers"].as_array().unwrap().iter().map(|t| t["tier"].as_str().unwrap()).collect();
    assert_eq!(tiers, evani_convert::DEFAULT_TIERS);
    // Every file in the folder (but the master, reports and manifest) is in
    // exactly one tier's list, with its raw, gzip and brotli sizes.
    let mut listed = std::collections::BTreeSet::new();
    for t in r["tiers"].as_array().unwrap() {
        assert!(t["bytes"]["raw"].as_u64().unwrap() > 0);
        for f in t["files"].as_array().unwrap() {
            let name = f["file"].as_str().unwrap();
            let raw = std::fs::metadata(o.out_dir.join(name)).unwrap().len();
            assert_eq!(f["raw"].as_u64(), Some(raw), "{name}");
            assert!(f["gzip"].as_u64().unwrap() > 0 && f["brotli"].as_u64().unwrap() > 0, "{name}");
            assert!(listed.insert(name.to_owned()), "{name} listed twice");
        }
        // Five maps: core image plus ASTC and BC7 per map, and the tier file.
        assert_eq!(t["files"].as_array().unwrap().len(), 1 + 5 * 3, "{}", t["tier"]);
        assert!(t["texture_memory"]["astc"].as_u64().unwrap() > 0);
        assert!(t["texture_memory"]["budget"].as_u64().is_some());
    }
    for f in report::list_files(&o.out_dir) {
        let f = f.to_string_lossy().replace('\\', "/");
        let side = ["fixture.master.glb", "fixture.report.json", "fixture.report.md", "fixture.manifest.json"];
        assert!(side.contains(&f.as_str()) || listed.contains(&f), "{f} is in the folder but not in the report");
    }
    for f in &listed {
        assert!(o.out_dir.join(f).is_file(), "{f} is in the report but not in the folder");
    }
    // The package against the 10 MB cap, the tool versions and the input hash.
    assert_eq!(r["package"]["cap"].as_u64(), Some(10 * 1024 * 1024));
    assert_eq!(r["package"]["over_cap"].as_bool(), Some(false));
    assert_eq!(r["tools"].as_array().unwrap().len(), 3);
    assert_eq!(r["inputs"][0]["sha256"].as_str().unwrap().len(), 64);
    // The manifest lists the same files and the engine's manifest reader reads it.
    let m = std::fs::read_to_string(o.out_dir.join("fixture.manifest.json")).unwrap();
    let man = evani_core::assets::AssetManifest::from_json(&m).unwrap();
    let _ = man;
    let mj: Value = serde_json::from_str(&m).unwrap();
    assert_eq!(mj["assets"].as_array().unwrap().len(), listed.len());
    let md = std::fs::read_to_string(o.out_dir.join("fixture.report.md")).unwrap();
    for t in evani_convert::DEFAULT_TIERS {
        assert!(md.contains(&format!("fixture.{t}.glb")), "report.md lacks {t}");
    }
}

#[test]
fn extras_and_anchor_nodes_survive() {
    let Some(()) = tools_or_skip("extras_and_anchor_nodes_survive") else { return };
    let o = converted();
    for file in tier_files(o) {
        let g = read(&o.out_dir, &file);
        let tier = file.split('.').nth(1).unwrap();
        let e = g.evani.as_ref().unwrap_or_else(|| panic!("{file}: no extras.evani"));
        assert_eq!(e.tier.as_deref(), Some(tier));
        assert_eq!(e.class.as_deref(), Some("fixture"));
        assert_eq!(e.raw["min_distance_m"].as_f64(), Some(FIXTURE_MIN_DISTANCE), "{file}: the input's evani keys are kept");
        let doc = Doc::read_file(&o.out_dir.join(&file)).unwrap();
        assert_eq!(doc.json.pointer("/extras/author_note").and_then(Value::as_str), Some("kept as is"), "{file}: root extras");
        // Levels of detail: named nodes `<id>_lodK`, each holding the model.
        assert_eq!(e.lods.len(), 2, "{file}");
        for (k, l) in e.lods.iter().enumerate() {
            assert_eq!(l.node, format!("fixture_lod{k}"));
            assert!(l.node_index.is_some(), "{file}: LOD node {} missing", l.node);
        }
        // The anchor and its extras: gameplay data, so once in the file
        // (level 0 only, the name stays unique), in the same place.
        assert!(g.anchors.iter().all(|a| a.name == "seat"), "{file}");
        assert_eq!(g.anchors.len(), 1, "{file}: one anchor, on level 0");
        for a in &g.anchors {
            let p = [a.world[12], a.world[13], a.world[14]];
            assert!((p[1] - 0.6).abs() < 1e-3 && p[0].abs() < 1e-3, "{file}: anchor moved to {p:?}");
            let n = &doc.json["nodes"][a.node];
            assert_eq!(n.pointer("/extras/seat_height").and_then(Value::as_f64), Some(0.6), "{file}: anchor extras");
        }
        // Names live in nodes, never meshes; duplicates are one mesh used twice.
        for name in ["wheel_l", "wheel_r", "hub"] {
            assert!(g.nodes.iter().any(|n| n.name == name), "{file}: node {name} lost");
        }
        let lod0 = g.nodes.iter().position(|n| n.name == "fixture_lod0").unwrap();
        let in_lod0 = |name: &str| {
            g.nodes.iter().enumerate().find(|(i, n)| n.name == name && {
                let mut p = Some(*i);
                while let Some(q) = p {
                    if q == lod0 {
                        break;
                    }
                    p = g.parent(q);
                }
                p == Some(lod0)
            })
        };
        assert!(in_lod0("anchor:seat").is_some(), "{file}: the anchor sits under level 0");
        // The mesh is on the named node or on gltfpack's unnamed child of it.
        let mesh_of = |i: usize| {
            g.nodes[i].mesh.or_else(|| {
                g.nodes.iter().enumerate().find(|(c, n)| n.name.is_empty() && n.mesh.is_some() && g.parent(*c) == Some(i)).and_then(|(_, n)| n.mesh)
            })
        };
        let meshes: Vec<usize> = ["wheel_l", "wheel_r", "hub"].iter().filter_map(|n| in_lod0(n)).filter_map(|(i, _)| mesh_of(i)).collect();
        assert_eq!(meshes.len(), 3, "{file}");
        assert!(meshes.iter().all(|m| *m == meshes[0]), "{file}: the three balls should share one mesh, got {meshes:?}");
        assert!(!doc.json.to_string().contains("EXT_mesh_gpu_instancing"), "{file}: no instancing extension");
    }
}

#[test]
fn converted_files_pass_the_khronos_validator() {
    let Some(()) = tools_or_skip("converted_files_pass_the_khronos_validator") else { return };
    let o = converted();
    let tools = Tools::locate(&root()).unwrap();
    let v = o.report["validator"].as_array().unwrap();
    assert_eq!(v.len(), 4);
    for e in v {
        assert_eq!(e["errors"].as_u64(), Some(0), "{e}");
        assert_eq!(e["geometry"]["checked"].as_bool(), Some(true), "{e}");
        assert_eq!(e["geometry"]["errors"].as_u64(), Some(0), "{e}");
    }
    // And once more here, straight from the validator, on the shipped files.
    for file in tier_files(o) {
        let (err, _, lines) = report::validate(&tools, &o.out_dir.join(&file)).unwrap();
        assert_eq!(err, 0, "{file}: {lines:?}");
    }
}

#[test]
fn realistic_maps_survive_without_look_matching() {
    let Some(()) = tools_or_skip("realistic_maps_survive_without_look_matching") else { return };
    let o = converted();
    for file in tier_files(o) {
        let g = read(&o.out_dir, &file);
        let m = &g.materials[0];
        assert!(m.base_color_texture.is_some(), "{file}: base colour");
        assert!(m.metallic_roughness_texture.is_some(), "{file}: metal-rough");
        assert!(m.normal_texture.is_some(), "{file}: normal");
        assert!(m.occlusion_texture.is_some(), "{file}: occlusion");
        assert!(m.emissive_texture.is_some(), "{file}: emissive");
        let e = g.evani.as_ref().unwrap();
        assert_eq!(e.textures.len(), g.textures.len(), "{file}: one texture set per texture");
        let names: std::collections::BTreeSet<&str> = e.textures.iter().map(|t| t.name.as_str()).collect();
        for n in ["base", "metal_rough", "normal", "occlusion", "emissive"] {
            assert!(names.contains(n), "{file}: no {n} set");
        }
        let tier = file.split('.').nth(1).unwrap();
        for t in &e.textures {
            // The core image: JPEG for opaque colour and data, PNG for the normal map.
            let img = g.textures[t.texture].source.unwrap();
            let uri = g.images[img].uri.clone().unwrap();
            assert_eq!(uri, format!("textures/{}.{tier}.{}", t.name, if t.name == "normal" { "png" } else { "jpg" }), "{file}");
            assert!(o.out_dir.join(&uri).is_file(), "{uri}");
            let sets: std::collections::BTreeMap<&str, &str> = t.sets.iter().map(|(f, u)| (f.as_str(), u.as_str())).collect();
            for (set, vk) in [("astc", [157u32, 158]), ("bc7", [145, 146])] {
                let u = sets.get(set).unwrap_or_else(|| panic!("{file}: {} has no {set} set", t.name));
                assert_eq!(*u, format!("textures/{}.{tier}.{set}.ktx2", t.name));
                let b = std::fs::read(o.out_dir.join(u)).unwrap();
                let f = texture::ktx2_vk_format(&b).unwrap();
                // sRGB for colour (base, emissive), linear for data.
                let want = if t.name == "base" || t.name == "emissive" { vk[1] } else { vk[0] };
                assert_eq!(f, want, "{u}: vkFormat");
                assert_eq!(texture::ktx2_check(&b), Ok(()), "{u}");
            }
        }
    }
}

#[test]
fn same_input_same_bytes() {
    let Some(()) = tools_or_skip("same_input_same_bytes") else { return };
    let first = converted();
    let dir = scratch("again");
    let input = fixture(&dir);
    let again = convert(&fixture_options(&input, &dir.join("out"))).unwrap();
    let a = report::list_files(&first.out_dir);
    let b = report::list_files(&again.out_dir);
    assert_eq!(a, b, "the same files");
    for f in &a {
        let name = f.to_string_lossy();
        let (x, y) = (std::fs::read(first.out_dir.join(f)).unwrap(), std::fs::read(again.out_dir.join(f)).unwrap());
        if name.ends_with(".report.json") || name.ends_with(".report.md") {
            // Reports name the input's path, which differs; the rest is equal.
            let strip = |t: &[u8]| String::from_utf8_lossy(t).replace(&s(&first.out_dir), "").replace(&s(&again.out_dir), "").replace("evani-convert-tests/fixture", "X").replace("evani-convert-tests/again", "X");
            assert_eq!(strip(&x), strip(&y), "{name}");
        } else {
            assert!(x == y, "{name} differs between two runs");
        }
    }
}

fn s(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

#[test]
fn missing_tool_names_the_fix() {
    // An engine root without .evani/bin: every tool is missing.
    let empty = scratch("no-tools");
    let e = Tools::locate(&empty).unwrap_err();
    for tool in ["gltfpack", "ktx", "gltf-transform"] {
        assert!(e.contains(tool), "{e}");
    }
    assert!(e.contains("fix: run `cargo xtask doctor --fix`"), "{e}");
    // The converter itself stops with the same message before doing anything.
    let input = fixture(&empty);
    let mut o = Options::new(&empty, &input);
    o.out = Some(empty.join("out"));
    let err = convert(&o).unwrap_err();
    assert!(err.contains("doctor --fix") && err.contains("gltfpack"), "{err}");
    assert!(!empty.join("out").exists(), "nothing written");
}

#[test]
fn texture_size_follows_screen_size() {
    let Some(()) = tools_or_skip("texture_size_follows_screen_size") else { return };
    let o = converted();
    let r = &o.report;
    let extent = 3.0; // the balls' box: x from -1.5 to 1.5
    let mut sizes = Vec::new();
    for t in r["tiers"].as_array().unwrap() {
        let tier = t["tier"].as_str().unwrap();
        let screen = texture::Screen::load(&root(), tier).unwrap();
        let want = texture::size_for(&screen, extent, Some(FIXTURE_MIN_DISTANCE));
        assert_eq!(t["texture_size"]["size"].as_u64(), Some(u64::from(want.size)), "{tier}");
        // The numbers come from assets/tiers: height x render scale x DPR cap.
        assert_eq!(t["screen"]["pixels"].as_f64(), Some(screen.pixels()), "{tier}");
        let px = (want.size as usize).min(MAP_PX) as u64;
        for im in t["images"].as_array().unwrap() {
            assert_eq!((im["w"].as_u64(), im["h"].as_u64()), (Some(px), Some(px)), "{tier} {}", im["name"]);
        }
        sizes.push(want.size);
    }
    // Low (1080 x 0.75 x 1.25) sees the model at 128 px, medium (x 1.5) at 256.
    assert_eq!(sizes[0], 128);
    assert_eq!(sizes[1], 256);
    // Closer means bigger, never past the tier's cap; farther shrinks to the floor.
    let low = texture::Screen::load(&root(), "low").unwrap();
    let near = texture::size_for(&low, extent, Some(2.0));
    let far = texture::size_for(&low, extent, Some(10_000.0));
    let unknown = texture::size_for(&low, extent, None);
    assert_eq!(near.size, near.cap);
    assert_eq!(far.size, far.floor);
    assert_eq!(unknown.size, unknown.cap);
    assert!(texture::size_for(&low, extent, Some(20.0)).size >= texture::size_for(&low, extent, Some(40.0)).size);
}

#[test]
fn evani_model_files_are_not_resimplified() {
    let Some(()) = tools_or_skip("evani_model_files_are_not_resimplified") else { return };
    let dir = scratch("evani-model");
    let src = root().join("tests").join("fixtures").join("models").join("kart.low.glb");
    let input = dir.join("kart.low.glb");
    std::fs::copy(&src, &input).unwrap();
    // Its normal maps are not checked in beside it: stand-ins of the listed size.
    let j = Doc::read_file(&input).unwrap().json;
    for m in j.pointer("/extras/evani/normal_maps/maps").and_then(Value::as_array).unwrap() {
        let size = m["size"].as_u64().unwrap() as usize;
        let px: Vec<u8> = (0..size * size).flat_map(|i| [128u8, 128, 255 - (i % 7) as u8]).collect();
        std::fs::write(dir.join(m["file"].as_str().unwrap()), evani_model::png::write_rgb(size, size, &px)).unwrap();
    }
    let mut opt = Options::new(&root(), &input);
    opt.out = Some(dir.join("out"));
    let o = convert(&opt).unwrap();
    assert!(o.errors.is_empty(), "problems: {:?}", o.errors);
    assert_eq!(o.report["evani_model_input"].as_bool(), Some(true));
    let tiers: Vec<&str> = o.report["tiers"].as_array().unwrap().iter().map(|t| t["tier"].as_str().unwrap()).collect();
    assert_eq!(tiers, ["low"], "only the tiers Evani.Model wrote");
    let g_in = Gltf::from_glb(&std::fs::read(&input).unwrap()).unwrap();
    let g_out = read(&o.out_dir, &format!("{}.low.glb", o.id));
    let tris = |g: &Gltf| -> usize {
        let mut n = 0;
        for node in &g.nodes {
            if let Some(m) = node.mesh {
                for p in &g.meshes[m].primitives {
                    n += p.indices.map_or(0, |i| g.accessors[i].count) / 3;
                }
            }
        }
        n
    };
    assert!(tris(&g_in) > 0);
    assert_eq!(tris(&g_out), tris(&g_in), "same triangles: never re-simplified");
    // gltfpack may move a mesh onto a new unnamed child (its dequantize
    // transform); the named nodes stay, in order, and each still carries its
    // part: the mesh is on the node itself or on a direct child.
    let names = |g: &Gltf| g.nodes.iter().filter(|n| !n.name.is_empty()).map(|n| n.name.clone()).collect::<Vec<_>>();
    assert_eq!(names(&g_out), names(&g_in), "same named nodes, same order");
    for n in g_in.nodes.iter().filter(|n| n.mesh.is_some()) {
        let i = g_out.nodes.iter().position(|o| o.name == n.name).unwrap();
        let has = g_out.nodes[i].mesh.is_some()
            || g_out.nodes.iter().enumerate().any(|(c, o)| o.mesh.is_some() && o.name.is_empty() && g_out.parent(c) == Some(i));
        assert!(has, "{}: its mesh moved away from the named node", n.name);
    }
    let doc = Doc::read_file(&o.out_dir.join(format!("{}.low.glb", o.id))).unwrap();
    assert!(doc.json.to_string().contains("EXT_meshopt_compression"), "compressed");
    // The input's Evani metadata is kept (parts, provenance), tier set.
    let (e_in, e_out) = (g_in.evani.unwrap(), g_out.evani.unwrap());
    assert_eq!(e_out.parts, e_in.parts);
    assert_eq!(e_out.tier.as_deref(), Some("low"));
}
