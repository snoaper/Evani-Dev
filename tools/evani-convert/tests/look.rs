//! STAGE3_PLAN S3-12, look matching: `--look toon` drops the maps the look
//! never samples, merges materials equal in what it reads, and the quality
//! gate still judges the result. Like convert.rs these run the pinned tools
//! (`cargo xtask doctor --fix`) and skip with a printed reason without them;
//! `EVANI_REQUIRE_TOOLS=1` turns a skip into a failure.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code: a failed step is a failed test

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use evani_convert::glb::Doc;
use evani_convert::{Options, Outcome, convert, look, tools::Tools};
use evani_gltf::Gltf;
use serde_json::{Value, json};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap().to_path_buf()
}

fn scratch(name: &str) -> PathBuf {
    let t = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| root().join("target"));
    let p = std::path::absolute(t).unwrap().join("evani-look-tests").join(name);
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

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

const MAP_PX: usize = 128;

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

fn map_png(seed: u8) -> Vec<u8> {
    let mut px = Vec::with_capacity(MAP_PX * MAP_PX * 3);
    for y in 0..MAP_PX {
        for x in 0..MAP_PX {
            let check = ((x / 16 + y / 16) % 2) as u8;
            px.extend_from_slice(&[(x as u8).wrapping_mul(2).wrapping_add(seed), (y as u8).wrapping_mul(2), check * 150 + seed / 2]);
        }
    }
    evani_model::png::write_rgb(MAP_PX, MAP_PX, &px)
}

/// A tangent-space normal map as a baker writes one: gentle bumps (unit
/// normals tilted at most about 23 degrees, smooth across texels) encoded
/// `n * 0.5 + 0.5`.
fn normal_png() -> Vec<u8> {
    let mut px = Vec::with_capacity(MAP_PX * MAP_PX * 3);
    let k = 2.0 * std::f32::consts::PI * 4.0 / MAP_PX as f32;
    for y in 0..MAP_PX {
        for x in 0..MAP_PX {
            let (dx, dy) = (0.3 * (k * x as f32).cos(), 0.3 * (k * y as f32).cos());
            let l = (dx * dx + dy * dy + 1.0).sqrt();
            px.extend([-dx / l, -dy / l, 1.0 / l].map(|c| ((c * 0.5 + 0.5) * 255.0).round() as u8));
        }
    }
    evani_model::png::write_rgb(MAP_PX, MAP_PX, &px)
}

/// A car-ish model for matching: a sphere "body" mesh of three primitives
/// whose materials differ only in what toon never reads (metal-rough maps and
/// factors, occlusion), a "trim" mesh in a genuinely different material
/// (emissive), named nodes and an anchor with extras. Five images: base,
/// normal, metal-rough A, occlusion, metal-rough B.
fn fixture(dir: &Path) -> PathBuf {
    let (rings, segs) = (12u32, 16u32);
    let (mut pos, mut nrm, mut uv, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::<u16>::new());
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
    let mut views = vec![
        json!({ "buffer": 0, "byteOffset": po, "byteLength": pl, "target": 34962 }),
        json!({ "buffer": 0, "byteOffset": no, "byteLength": nl, "target": 34962 }),
        json!({ "buffer": 0, "byteOffset": uo, "byteLength": ul, "target": 34962 }),
    ];
    // Three index ranges (thirds of the rings), each its own primitive.
    let per = (idx.len() / 3 / 6) * 6;
    let mut accessors = vec![
        json!({ "bufferView": 0, "componentType": 5126, "count": n_verts, "type": "VEC3", "min": [-0.5, -0.5, -0.5], "max": [0.5, 0.5, 0.5] }),
        json!({ "bufferView": 1, "componentType": 5126, "count": n_verts, "type": "VEC3" }),
        json!({ "bufferView": 2, "componentType": 5126, "count": n_verts, "type": "VEC2" }),
    ];
    let mut ranges = Vec::new();
    for k in 0..3 {
        let part = if k == 2 { &idx[2 * per..] } else { &idx[k * per..(k + 1) * per] };
        let off = bin.len();
        for i in part {
            bin.extend_from_slice(&i.to_le_bytes());
        }
        views.push(json!({ "buffer": 0, "byteOffset": off, "byteLength": part.len() * 2, "target": 34963 }));
        align4(&mut bin);
        accessors.push(json!({ "bufferView": views.len() - 1, "componentType": 5123, "count": part.len(), "type": "SCALAR" }));
        ranges.push(accessors.len() - 1);
    }
    let mut images = Vec::new();
    for k in 0..5u8 {
        let png = if k == 1 { normal_png() } else { map_png(k * 40) };
        let off = bin.len();
        bin.extend_from_slice(&png);
        views.push(json!({ "buffer": 0, "byteOffset": off, "byteLength": png.len() }));
        align4(&mut bin);
        images.push(json!({ "bufferView": views.len() - 1, "mimeType": "image/png" }));
    }
    let prim = |ix: usize, material: usize| json!({ "attributes": { "POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2 }, "indices": ix, "material": material });
    // Textures: 0 base, 1 normal, 2 metal-rough A, 3 occlusion, 4 metal-rough B.
    let paint = |name: &str, extra: Value| {
        let mut m = json!({
            "name": name,
            "pbrMetallicRoughness": { "baseColorTexture": { "index": 0 } },
            "normalTexture": { "index": 1 },
            "doubleSided": true
        });
        for (k, v) in extra.as_object().unwrap() {
            if k == "pbr" {
                for (pk, pv) in v.as_object().unwrap() {
                    m["pbrMetallicRoughness"][pk] = pv.clone();
                }
            } else {
                m[k] = v.clone();
            }
        }
        m
    };
    let doc = json!({
        "asset": { "version": "2.0", "generator": "evani-convert look test fixture" },
        "extras": { "evani": { "format": 1, "class": "looky", "min_distance_m": 40.0 }, "author_note": "kept as is" },
        "scene": 0,
        "scenes": [{ "nodes": [0] }],
        "nodes": [
            { "name": "looky", "children": [1, 2, 3] },
            { "name": "body", "mesh": 0 },
            { "name": "trim", "mesh": 1, "translation": [0.0, 0.0, 1.0] },
            { "name": "anchor:seat", "translation": [0.0, 0.6, 0.0], "extras": { "seat_height": 0.6 } }
        ],
        "meshes": [
            { "name": "body", "primitives": [prim(ranges[0], 0), prim(ranges[1], 1), prim(ranges[2], 2)] },
            { "name": "trim", "primitives": [prim(ranges[0], 3)] }
        ],
        "materials": [
            paint("paint_a", json!({ "pbr": { "metallicRoughnessTexture": { "index": 2 }, "metallicFactor": 0.1, "roughnessFactor": 0.9 }, "occlusionTexture": { "index": 3 } })),
            paint("paint_b", json!({ "pbr": { "metallicRoughnessTexture": { "index": 4 }, "metallicFactor": 0.9, "roughnessFactor": 0.2 }, "occlusionTexture": { "index": 3, "strength": 0.5 } })),
            paint("paint_c", json!({ "pbr": { "metallicFactor": 0.5 } })),
            paint("trim_glow", json!({ "emissiveFactor": [0.4, 0.2, 0.0], "pbr": { "metallicRoughnessTexture": { "index": 2 } } }))
        ],
        "textures": [{ "source": 0 }, { "source": 1 }, { "source": 2 }, { "source": 3 }, { "source": 4 }],
        "images": images,
        "accessors": accessors,
        "bufferViews": views,
        "buffers": [{ "byteLength": bin.len() }]
    });
    let p = dir.join("looky.glb");
    std::fs::write(&p, Doc { json: doc, bin }.to_glb().unwrap()).unwrap();
    p
}

fn options(input: &Path, out: &Path, look: Option<&str>) -> Options {
    let mut o = Options::new(&root(), input);
    o.id = Some("looky".into());
    o.out = Some(out.to_path_buf());
    o.tiers = vec!["low".into()];
    o.look = look.map(str::to_owned);
    o
}

/// The fixture converted once without a look and once with toon.
fn both() -> &'static (Outcome, Outcome, PathBuf) {
    static ONE: OnceLock<(Outcome, Outcome, PathBuf)> = OnceLock::new();
    ONE.get_or_init(|| {
        let dir = scratch("looky");
        let input = fixture(&dir);
        let plain = convert(&options(&input, &dir.join("plain"), None)).unwrap();
        let toon = convert(&options(&input, &dir.join("toon"), Some("toon"))).unwrap();
        (plain, toon, input)
    })
}

fn read(o: &Outcome, file: &str) -> Gltf {
    Gltf::from_glb(&std::fs::read(o.out_dir.join(file)).unwrap()).unwrap_or_else(|e| panic!("{file}: the engine's reader refuses it: {}", e.0))
}

fn image_names(o: &Outcome) -> Vec<String> {
    o.report["tiers"][0]["images"].as_array().unwrap().iter().map(|i| i["name"].as_str().unwrap().to_owned()).collect()
}

// ------------------------------------------------------------------ tests

#[test]
fn toon_drops_unused_maps() {
    let Some(()) = tools_or_skip("toon_drops_unused_maps") else { return };
    let (plain, toon, _) = both();
    assert!(plain.errors.is_empty(), "{:?}", plain.errors);
    assert!(toon.errors.is_empty(), "{:?}", toon.errors);
    // Without a look every map ships (the converter's old behaviour).
    let p = image_names(plain);
    for n in ["base", "normal", "metal_rough", "occlusion"] {
        assert!(p.iter().any(|x| x == n), "plain: {n} in {p:?}");
    }
    // With toon: base and normal only; no metal-rough or occlusion image, set or file.
    let t = image_names(toon);
    assert_eq!(t, ["base", "normal"], "toon keeps what it samples");
    let g = read(toon, "looky.low.glb");
    assert_eq!(g.textures.len(), 2, "two textures left");
    for m in &g.materials {
        assert!(m.base_color_texture.is_some() && m.normal_texture.is_some(), "{}: the maps toon samples stay", m.name);
        assert!(m.metallic_roughness_texture.is_none() && m.occlusion_texture.is_none(), "{}: the maps toon never samples are gone", m.name);
    }
    for f in report_files(toon) {
        assert!(!f.contains("metal_rough") && !f.contains("occlusion"), "{f} should not ship under toon");
    }
    // The bytes saved are in the report, exactly.
    let lm = &toon.report["look_match"];
    assert_eq!(lm["has_table"], true);
    let slots: Vec<&str> = lm["dropped_slots"].as_array().unwrap().iter().map(|d| d["slot"].as_str().unwrap()).collect();
    for want in ["metallic_factor", "roughness_factor", "metallic_roughness_texture", "occlusion_texture"] {
        assert!(slots.contains(&want), "dropped {want}: {slots:?}");
    }
    let t0 = &lm["tiers"][0];
    assert!(t0["download_saved"]["astc_device"].as_i64().unwrap() > 0, "{t0}");
    assert!(t0["maps_not_shipped"]["astc_gpu_bytes"].as_u64().unwrap() > 0, "{t0}");
    let mut dropped: Vec<&str> = t0["maps_not_shipped"]["images"].as_array().unwrap().iter().map(|i| i.as_str().unwrap()).collect();
    dropped.sort_unstable();
    assert_eq!(dropped, ["metal_rough", "metal_rough_1", "occlusion"]);
    assert!(toon.out_dir.join("looky.report.md").is_file());
    let md = std::fs::read_to_string(toon.out_dir.join("looky.report.md")).unwrap();
    assert!(md.contains("Look matching (`toon`)"), "the markdown report has the section");
}

fn report_files(o: &Outcome) -> Vec<String> {
    o.report["tiers"][0]["files"].as_array().unwrap().iter().map(|f| f["file"].as_str().unwrap().to_owned()).collect()
}

#[test]
fn merged_materials_cut_draw_calls() {
    let Some(()) = tools_or_skip("merged_materials_cut_draw_calls") else { return };
    let (plain, toon, _) = both();
    let lm = &toon.report["look_match"];
    assert_eq!(lm["materials"]["before"], 4);
    assert_eq!(lm["materials"]["after"], 2, "the three paints are one material under toon; the glowing trim is its own");
    let g = read(toon, "looky.low.glb");
    assert_eq!(g.materials.len(), 2);
    // The report's own draw-call count, with and without the look.
    let (without, with) = (plain.report["tiers"][0]["draw_calls"].as_u64().unwrap(), toon.report["tiers"][0]["draw_calls"].as_u64().unwrap());
    assert_eq!(without, 4, "one draw call per primitive and material");
    assert!(with < without, "merged materials cut draw calls: {without} -> {with}");
    assert_eq!(with, 2, "body (one material) and trim");
    // The report measured the same thing itself.
    assert_eq!(lm["tiers"][0]["draw_calls"]["without"].as_u64().unwrap(), without);
    assert_eq!(lm["tiers"][0]["draw_calls"]["with"].as_u64().unwrap(), with);
    // The look-relevant difference is kept: the trim still glows, the paint does not.
    let glow: Vec<bool> = g.materials.iter().map(|m| m.emissive_factor.iter().any(|c| *c > 0.0)).collect();
    assert_eq!(glow.iter().filter(|x| **x).count(), 1);
}

#[test]
fn matching_keeps_names_anchors_and_extras() {
    let Some(()) = tools_or_skip("matching_keeps_names_anchors_and_extras") else { return };
    let (_, toon, input) = both();
    // The matched document: nodes, scenes and extras equal the input's, exactly.
    let doc = Doc::read_file(input).unwrap();
    let (m, rep) = look::apply(&doc, look::row("toon").unwrap(), None);
    assert!(rep.changed());
    for key in ["nodes", "scenes", "scene", "extras", "accessors", "bufferViews", "buffers"] {
        assert_eq!(m.json.get(key), doc.json.get(key), "{key} is untouched by matching");
    }
    assert_eq!(m.bin, doc.bin);
    // And the shipped file still has them.
    let g = read(toon, "looky.low.glb");
    assert!(g.node_by_name("body").is_some() && g.node_by_name("trim").is_some());
    let anchor = g.node_by_name("anchor:seat").expect("the anchor survives");
    assert!(g.nodes[anchor].extras.get("seat_height").is_some());
    assert_eq!(g.evani.as_ref().unwrap().class.as_deref(), Some("looky"));
    assert_eq!(g.extras.get("author_note").and_then(Value::as_str), Some("kept as is"));
}

#[test]
fn a_look_without_a_table_changes_nothing() {
    let Some(()) = tools_or_skip("a_look_without_a_table_changes_nothing") else { return };
    let dir = scratch("looky-basic");
    let input = fixture(&dir);
    let o = convert(&options(&input, &dir.join("out"), Some("basic"))).unwrap();
    assert!(o.errors.is_empty(), "{:?}", o.errors);
    assert_eq!(o.report["look_match"]["has_table"], false);
    let names = image_names(&o);
    assert!(names.iter().any(|n| n == "metal_rough") && names.iter().any(|n| n == "occlusion"), "{names:?}");
    let notes: Vec<&str> = o.report["notes"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert!(notes.iter().any(|n| n.contains("no table for this look")), "{notes:?}");
}

#[test]
fn snap_palette_moves_untextured_colours_only_when_asked() {
    let Some(()) = tools_or_skip("snap_palette_moves_untextured_colours_only_when_asked") else { return };
    // An untextured model: one material near the toon cherry swatch, one far from any.
    let dir = scratch("looky-snap");
    let mut doc = Doc::read_file(&fixture(&dir)).unwrap();
    let pal = look::palette("toon").unwrap();
    let near = [pal[0][0] * 0.97, pal[0][1] * 1.05, pal[0][2], 1.0];
    doc.json["materials"] = json!([
        { "name": "near", "pbrMetallicRoughness": { "baseColorFactor": near } },
        { "name": "far", "pbrMetallicRoughness": { "baseColorFactor": [0.02, 0.02, 0.02, 1.0] } }
    ]);
    doc.json["textures"] = json!([]);
    doc.json["images"] = json!([]);
    doc.json["meshes"][0]["primitives"] = json!([doc.json["meshes"][0]["primitives"][0].clone(), doc.json["meshes"][0]["primitives"][1].clone()]);
    doc.json["meshes"][0]["primitives"][0]["material"] = json!(0);
    doc.json["meshes"][0]["primitives"][1]["material"] = json!(1);
    doc.json["meshes"][1]["primitives"][0]["material"] = json!(1);
    let input = dir.join("flat.glb");
    std::fs::write(&input, doc.to_glb().unwrap()).unwrap();
    let run = |snap: bool, name: &str| {
        let mut o = options(&input, &dir.join(name), Some("toon"));
        o.snap_palette = snap;
        convert(&o).unwrap()
    };
    let (off, on) = (run(false, "off"), run(true, "on"));
    let colour = |o: &Outcome, i: usize| read(o, "looky.low.glb").materials[i].base_color_factor;
    let near_off = colour(&off, 0);
    assert!((near_off[0] - near[0]).abs() < 1e-3, "not asked: the colour stays ({near_off:?})");
    let near_on = colour(&on, 0);
    assert!((near_on[0] - pal[0][0]).abs() < 1e-3 && (near_on[1] - pal[0][1]).abs() < 1e-3, "snapped to cherry: {near_on:?}");
    let far_on = colour(&on, 1);
    assert!(far_on[0] < 0.05, "too far from any swatch: stays dark ({far_on:?})");
    let sp = &on.report["look_match"]["snap_palette"];
    assert_eq!(sp["snapped"].as_array().unwrap().len(), 1);
    assert_eq!(sp["skipped"].as_array().unwrap().len(), 1);
    assert_eq!(off.report["look_match"]["snap_palette"]["snapped"].as_array().unwrap().len(), 0);
}

/// `tests/fixtures/models/look-match.low.glb` is the matched fixture's tier
/// file, the one evani-render's `variant_works_on_a_look_matched_file` draws.
/// Rewrite it with `EVANI_REWRITE_FIXTURES=1 cargo test -p evani-convert --test look`.
#[test]
fn look_match_fixture_is_the_converter_output() {
    let Some(()) = tools_or_skip("look_match_fixture_is_the_converter_output") else { return };
    let (_, toon, _) = both();
    let made = std::fs::read(toon.out_dir.join("looky.low.glb")).unwrap();
    let path = root().join("tests/fixtures/models/look-match.low.glb");
    if std::env::var("EVANI_REWRITE_FIXTURES").is_ok_and(|v| v == "1") {
        std::fs::write(&path, &made).unwrap();
    }
    let kept = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e} (rewrite it with EVANI_REWRITE_FIXTURES=1)", path.display()));
    assert!(kept == made, "{} is not what `--look toon` makes of the fixture today: rewrite it with EVANI_REWRITE_FIXTURES=1", path.display());
}

/// The relief fixture converted, with a look or without: every relief
/// material's height map is a real texture whose image is the height picture,
/// its normal map is still its normal map, and its recipe is intact.
fn relief_survives(look: Option<&str>, name: &str) {
    let src = root().join("tests/fixtures/models/relief-tile.glb");
    let dir = scratch(name);
    let mut o = Options::new(&root(), &src);
    o.id = Some("tile".into());
    o.out = Some(dir.join("out"));
    o.tiers = vec!["low".into()];
    o.look = look.map(str::to_owned);
    let out = convert(&o).unwrap();
    assert!(out.errors.is_empty(), "{:?}", out.errors);
    let before = Doc::read_file(&src).unwrap();
    let after = Doc::read_file(&out.out_dir.join("tile.low.glb")).unwrap();
    let mut seen = 0;
    for (i, m) in after.arr("materials").iter().enumerate() {
        let Some(rel) = m.pointer("/extras/evani/relief") else { continue };
        let was = before.arr("materials").iter().find(|b| b["name"] == m["name"]).and_then(|b| b.pointer("/extras/evani/relief")).expect("the input had it");
        // The wall's columns have a micro pattern only: nothing to keep, nothing invented.
        if was.get("height").is_none() {
            assert!(rel.get("height").is_none(), "material {i}: a height map appeared: {rel}");
            assert_eq!(rel.get("micro"), was.get("micro"));
            continue;
        }
        seen += 1;
        let tex = rel.pointer("/height/index").and_then(Value::as_u64).unwrap_or_else(|| panic!("material {i}: height.index is gone: {rel}")) as usize;
        let uri_of = |t: usize| {
            let src = after.arr("textures")[t]["source"].as_u64().unwrap() as usize;
            after.arr("images")[src]["uri"].as_str().unwrap().to_owned()
        };
        assert!(uri_of(tex).contains("height"), "material {i}: height.index {tex} names {}", uri_of(tex));
        let n = m.pointer("/normalTexture/index").and_then(Value::as_u64).expect("the normal map stays") as usize;
        assert!(uri_of(n).contains("normal"), "material {i}: normal map is {}", uri_of(n));
        assert!(rel.pointer("/height/parked").is_none() && m.get("occlusionTexture").is_none(), "material {i}: the parking is undone");
        // The recipe is the input's, apart from the (renumbered) texture.
        assert_eq!(rel.get("micro"), was.get("micro"));
        assert_eq!(rel.pointer("/height/depth"), was.pointer("/height/depth"));
    }
    assert!(seen > 0, "the fixture has relief materials");
    let g = Gltf::from_glb(&std::fs::read(out.out_dir.join("tile.low.glb")).unwrap()).unwrap();
    assert!(g.materials.iter().any(|m| m.extras.get("evani").is_some()), "the engine's reader keeps the recipe");
}

#[test]
fn relief_maps_survive_toon() {
    let Some(()) = tools_or_skip("relief_maps_survive_toon") else { return };
    relief_survives(Some("toon"), "tile-toon");
}

#[test]
fn relief_maps_survive_without_a_look() {
    let Some(()) = tools_or_skip("relief_maps_survive_without_a_look") else { return };
    relief_survives(None, "tile-plain");
}

#[test]
fn gate_still_runs_after_matching() {
    let Some(()) = tools_or_skip("gate_still_runs_after_matching") else { return };
    // The gate needs the model-view page and a headless Chrome.
    if let Err(e) = evani_convert::runner::gate_dist(&root()) {
        if std::env::var("EVANI_REQUIRE_TOOLS").is_ok_and(|v| v == "1") {
            panic!("gate_still_runs_after_matching: EVANI_REQUIRE_TOOLS=1 but the gate page could not be built ({e})");
        }
        println!("SKIPPED gate_still_runs_after_matching: the gate page could not be built ({e})");
        return;
    }
    let dir = scratch("looky-gate");
    let input = fixture(&dir);
    let mut o = options(&input, &dir.join("out"), Some("toon"));
    o.gate = true;
    // Size the textures for the distance the gate looks from. Its views frame
    // the model to fill the picture (the close-up nearer still), while the
    // fixture's `min_distance_m` of 40 sizes Low's maps to 64 px from 128:
    // the fine checker then softens at the close-up and every rung fails
    // (best rung SSIMULACRA2 -26.1, so the bar stays 90), whatever the look
    // does. At 1 m the maps stay full size, the format floor is real and the
    // bar comes from it by the gate's own rule, as on any model (DECISIONS
    // "Look matching in the converter (S3-12)").
    o.min_distance = Some(1.0);
    let out = convert(&o).unwrap();
    let gate = &out.report["gate"];
    assert!(gate.is_object(), "the gate ran: {}", out.report["notes"]);
    // The gate scored the matched tier file against the master (the original, all maps).
    let tier = &gate["tiers"][0];
    assert_eq!(tier["tier"], "low", "{gate}");
    assert!(tier["final"]["views"].as_array().is_some_and(|v| !v.is_empty()), "the tier file's views were scored: {tier}");
    assert!(tier["final"]["pass"].as_bool().unwrap_or(false), "the matched file passes the gate: {tier}");
    let master = Doc::read_file(&out.out_dir.join("looky.master.glb")).unwrap();
    assert_eq!(master.arr("materials").len(), 4, "the master is the original");
    assert!(master.arr("images").len() == 5, "the master keeps every map");
    let g = read(&out, "looky.low.glb");
    assert_eq!(g.materials.len(), 2, "the shipped file is the matched one");
    assert!(out.errors.is_empty(), "the matched file passes the gate: {:?}", out.errors);
}

/// The gate page and a headless Chrome, or a printed skip (a failure under
/// `EVANI_REQUIRE_TOOLS=1`).
fn gate_or_skip(test: &str) -> Option<()> {
    tools_or_skip(test)?;
    match evani_convert::runner::gate_dist(&root()) {
        Ok(_) => Some(()),
        Err(e) if std::env::var("EVANI_REQUIRE_TOOLS").is_ok_and(|v| v == "1") => panic!("{test}: EVANI_REQUIRE_TOOLS=1 but the gate page could not be built ({e})"),
        Err(e) => {
            println!("SKIPPED {test}: the gate page could not be built ({e})");
            None
        }
    }
}

/// S7-5: a textured trial is scored on what each device family samples
/// (ASTC on phones, BC7 on desktops, the PNG/JPEG core elsewhere), from the
/// encoder the tier file's sets come from, and so is the finished file: the
/// S3-12 look fixture's Medium and Low files (toon, as the open S3-6 finding
/// measured them) pass the bar their trials passed, on every family, and
/// score as their trials did.
#[test]
fn trial_scores_the_device_encoding() {
    let Some(()) = gate_or_skip("trial_scores_the_device_encoding") else { return };
    let dir = scratch("looky-families");
    let input = fixture(&dir);
    let mut o = options(&input, &dir.join("out"), Some("toon"));
    o.tiers = vec!["medium".into(), "low".into()];
    o.gate = true;
    // Full-size maps at the gate's distance (see gate_still_runs_after_matching).
    o.min_distance = Some(1.0);
    let out = convert(&o).unwrap();
    let gate = &out.report["gate"];
    assert!(gate.is_object(), "the gate ran: {}", out.report["notes"]);
    let tiers = gate["tiers"].as_array().unwrap();
    assert_eq!(tiers.iter().map(|t| t["tier"].as_str().unwrap()).collect::<Vec<_>>(), ["medium", "low"], "{gate}");
    for t in tiers {
        let tier = t["tier"].as_str().unwrap();
        let trials = t["trials"].as_array().unwrap();
        assert!(!trials.is_empty(), "{tier}: the search tried settings: {t}");
        // The best rung (the floor) is scored on all three families.
        let floor = &trials[0]["families"];
        for f in evani_convert::gate::FAMILIES {
            assert!(floor[f]["ssimulacra2"].is_number(), "{tier}: the floor trial is scored on {f}: {t}");
        }
        // Every trial names the family it is judged on.
        for tr in trials {
            assert!(tr["family"].is_string(), "{tier}: a textured trial is judged on a texture family: {tr}");
        }
        let fin = &t["final"];
        for f in evani_convert::gate::FAMILIES {
            assert!(fin["families"][f]["ssimulacra2"].is_number(), "{tier}: the finished file is scored on {f}: {fin}");
        }
        let bar = t["ssimulacra2_bar"].as_f64().unwrap();
        assert!(fin["pass"].as_bool().unwrap(), "{tier}: the finished file passes the bar ({bar}) on every family: {t}");
        for f in evani_convert::gate::FAMILIES {
            let s = fin["families"][f]["ssimulacra2"].as_f64().unwrap();
            assert!(s >= bar, "{tier}: the finished file's {f} score {s} is under the bar {bar}: {t}");
        }
        // The chosen trial passed and the finished file scores as it did.
        let chosen = &t["chosen"];
        assert!(chosen.is_object(), "{tier}: a setting passed: {t}");
        assert!(trials.iter().any(|tr| tr["setting"] == *chosen && tr["pass"] == true), "{tier}: the chosen setting's trial passed: {t}");
        let drift = t["drift"].as_f64().unwrap();
        assert!(drift <= evani_convert::gate::DRIFT_MAX, "{tier}: the finished file drifts {drift} from its trial: {t}");
        assert_eq!(t["geometry_same"], true, "{tier}: {t}");
    }
    assert!(out.errors.is_empty(), "{:?}", out.errors);
}

/// The gate's BC7 decoder against the KTX tools' own: a BC7 set is
/// transcoded from UASTC, and `ktx extract --transcode rgba8` decodes that
/// UASTC file; the two pictures should be the same within a step or two.
#[test]
fn bc7_decode_matches_the_ktx_tools() {
    let Some(()) = tools_or_skip("bc7_decode_matches_the_ktx_tools") else { return };
    let tools = Tools::locate(&root()).unwrap();
    let dir = scratch("bc7-decode");
    let png = dir.join("map.png");
    std::fs::write(&png, map_png(40)).unwrap();
    let s = |p: &Path| p.to_string_lossy().into_owned();
    let mid = dir.join("map.uastc.ktx2");
    let bc7 = dir.join("map.bc7.ktx2");
    let raw = dir.join("map.raw");
    let a = evani_convert::texture::ktx_create_args("uastc", true, &s(&png), &s(&mid));
    tools.ktx(&a.iter().map(String::as_str).collect::<Vec<_>>(), "ktx create UASTC").unwrap();
    tools.ktx(&["transcode", "--target", "bc7", &s(&mid), &s(&bc7)], "ktx transcode BC7").unwrap();
    tools.ktx(&["extract", "--transcode", "rgba8", "--raw", &s(&mid), &s(&raw)], "ktx extract rgba8").unwrap();
    let (w, h, ours) = evani_convert::bc7::decode_ktx2_level0(&std::fs::read(&bc7).unwrap()).unwrap();
    let theirs = std::fs::read(&raw).unwrap();
    assert_eq!((w, h), (MAP_PX, MAP_PX));
    assert_eq!(ours.len(), theirs.len(), "level 0 sizes");
    let diffs: Vec<u32> = ours.iter().zip(&theirs).map(|(a, b)| u32::from(a.abs_diff(*b))).collect();
    let max = diffs.iter().copied().max().unwrap_or(0);
    let mean = diffs.iter().sum::<u32>() as f64 / diffs.len() as f64;
    println!("bc7 decode vs ktx rgba8: max {max}, mean {mean:.4}");
    assert!(max <= 4 && mean < 0.5, "the BC7 decode differs from the UASTC decode: max {max}, mean {mean}");
}
