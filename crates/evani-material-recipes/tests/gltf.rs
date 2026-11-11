//! Row to glTF (P10a): every catalogue row writes a material, and each family's
//! own parser reads the written JSON back to the intended record (layers
//! `Layers::from_extras` / `from_khr`, character `record::apply`, nature
//! `material_kind`), a recipe row's block round-trips through hex, and a real
//! `.glb` whose materials only name catalogue rows reaches the right blocks
//! through `apply_catalog` and the engine's own glTF reader.
//!
//! `cargo test -p evani-material-recipes --features gltf,data --test gltf`
#![cfg(all(feature = "gltf", feature = "data"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use evani_material_character::record::{self, Kind as CharKind, KIND_WORD, UNSET, WORDS};
use evani_material_layers::parse::Layers;
use evani_material_nature::Kind as NatureKind;
use evani_material_nature::gltf::material_kind;
use evani_material_recipes::block::Block;
use evani_material_recipes::catalog::{Catalog, Row};
use evani_material_recipes::gltf::{self, has_block, nature_kind, row_values};
use serde_json::{Value, json};

fn cat() -> Catalog {
    Catalog::bundled().unwrap()
}

fn rows<'a>(c: &'a Catalog, recipe: &str) -> Vec<&'a Row> {
    c.rows.iter().filter(|r| r.recipe == recipe).collect()
}

fn close(a: f32, b: f64) -> bool {
    (f64::from(a) - b).abs() < 1e-3
}

fn close3(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3)
}

/// One material in a minimal glTF document (for the nature parser).
fn doc_of(m: &Value) -> Value {
    json!({"materials": [m]})
}

#[test]
fn every_row_writes_a_material_and_recipe_blocks_round_trip() {
    let c = cat();
    let mut blocks = 0;
    for row in &c.rows {
        let m = gltf::gltf_material(&c, row).unwrap();
        assert_eq!(m["name"], json!(row.id));
        assert_eq!(m["extras"]["evani_material"], json!(row.id), "{}", row.id);
        let pbr = &m["pbrMetallicRoughness"];
        let base = pbr["baseColorFactor"].as_array().unwrap();
        assert_eq!(base.len(), 4);
        assert!(base.iter().all(|x| (0.0..=1.0).contains(&x.as_f64().unwrap())), "{}", row.id);
        assert!((0.0..=1.0).contains(&pbr["roughnessFactor"].as_f64().unwrap()));
        let recipe = c.recipe_for(row).unwrap();
        match m["extras"].get("evani_recipe") {
            Some(hex) => {
                assert!(has_block(recipe), "{}: only recipe-stage rows carry a block", row.id);
                let b = Block::from_hex(hex.as_str().unwrap()).unwrap();
                assert_eq!(b, c.encode(row).unwrap(), "{}: the block round-trips", row.id);
                blocks += 1;
            }
            None => assert!(!has_block(recipe), "{}: a recipe-stage row must carry its block", row.id),
        }
        // Paint's coat is the recipe stage's, never a second layered coat.
        if row.recipe == "paint" {
            assert!(m.pointer("/extensions/KHR_materials_clearcoat").is_none(), "{}", row.id);
            assert!(Layers::from_extras(&m["extras"]).is_none(), "{}: no layers record for paint", row.id);
        }
    }
    assert!(blocks >= 700, "about two thirds of the rows are recipe-stage rows ({blocks})");
}

/// B4-P3b: cloth is the recipe stage alone (weaves, patterns, sheen and
/// fuzz), so its material carries the block and no layers data at all: a
/// build with the layers family must not draw a second sheen over it.
#[test]
fn cloth_is_the_recipe_stage_alone() {
    let c = cat();
    let cloth = rows(&c, "cloth");
    assert!(cloth.len() >= 38, "every cloth row ({})", cloth.len());
    for row in cloth {
        let v = row_values(&c, row).unwrap();
        let m = gltf::gltf_material(&c, row).unwrap();
        assert!(Layers::from_extras(&m["extras"]).is_none(), "{}: no evani_layers or evani_cloth", row.id);
        assert!(m.pointer("/extensions/KHR_materials_sheen").is_none(), "{}: no layered sheen", row.id);
        assert_eq!(m["extras"]["evani_recipe"], json!(v.block.to_hex()), "{}: cloth is the recipe stage's", row.id);
        assert!(close(v.num("rough", 0.0), m["pbrMetallicRoughness"]["roughnessFactor"].as_f64().unwrap()));
    }
}

#[test]
fn brushed_metal_writes_anisotropy_for_the_layers_family() {
    let c = cat();
    let mut row = rows(&c, "metal")[0].clone();
    row.params.insert("numbers".into(), json!({"aniso": 0.6, "rough": 0.3}));
    row.params.insert("variant".into(), json!("brushed"));
    let m = gltf::gltf_material(&c, &row).unwrap();
    let l = Layers::from_extras(&m["extras"]).unwrap();
    let k = Layers::from_khr(&m["extensions"]).unwrap();
    assert!((l.anisotropy - 0.6).abs() < 0.01 && (k.anisotropy - l.anisotropy).abs() < 1e-4);
    assert!(m["extras"]["evani_recipe"].is_string(), "the recipe still draws the grain");
    // No anisotropy: no layers data at all.
    let plain = gltf::gltf_material(&c, rows(&c, "metal")[0]).unwrap();
    if row_values(&c, rows(&c, "metal")[0]).unwrap().num("aniso", 0.0) == 0.0 {
        assert!(Layers::from_extras(&plain["extras"]).is_none());
        assert!(plain.get("extensions").is_none());
    }
}

#[test]
fn skin_hair_and_eye_read_back_through_the_character_record() {
    let c = cat();
    for (recipe, kind) in [("skin", CharKind::Skin), ("hair", CharKind::Hair), ("eye", CharKind::Eye)] {
        let list = rows(&c, recipe);
        assert!(!list.is_empty());
        for row in list {
            let v = row_values(&c, row).unwrap();
            let m = gltf::gltf_material(&c, row).unwrap();
            let mut w = [0.0f32; WORDS];
            assert_eq!(record::apply(&m["extras"], &mut w), kind, "{}", row.id);
            assert_eq!(w[KIND_WORD], kind.code());
            assert!(m["extras"].get("evani_recipe").is_none(), "{}: one path per material", row.id);
            match kind {
                CharKind::Skin => {
                    assert!(close3([w[4], w[5], w[6]], v.colours[1]), "{}: scatter = undertone", row.id);
                    assert_eq!(w[7], UNSET, "pore strength stays the live param");
                    assert!((w[8] - 2.0 * v.num("wrinkles", 0.0)).abs() < 1e-3);
                    assert!((w[9] - v.num("pore_scale", 0.0)).abs() < 1e-2);
                    assert!((w[10] - v.num("freckles", 0.0)).abs() < 1e-3, "{}: spots = freckles", row.id);
                }
                CharKind::Hair => {
                    assert!((w[7] - (2.0 * v.num("shift", 0.0) - 1.0)).abs() < 1e-3, "{}", row.id);
                    assert!(w[4] >= v.colours[0][0] - 1e-3, "the highlight is lighter than the hair");
                }
                CharKind::Eye => {
                    assert!(close3([w[4], w[5], w[6]], v.colours[0]), "{}: iris colour", row.id);
                    assert!((w[11] - v.num("pupil", 0.0).clamp(0.05, 0.9)).abs() < 1e-3);
                    assert_eq!(&w[8..11], &[UNSET; 3], "the sclera stays the live param");
                    assert!(close3([w[12], w[13], w[15]], v.colours[1]), "{}: the ring round the pupil is colour 2", row.id);
                    assert_eq!(w[KIND_WORD], 2.0, "the ring words sit round the kind code");
                }
                CharKind::Plain => unreachable!(),
            }
        }
    }
}

#[test]
fn nature_rows_read_back_through_the_nature_parser() {
    let c = cat();
    for recipe in ["foliage", "thin", "glass", "water"] {
        let list = rows(&c, recipe);
        assert!(!list.is_empty());
        for row in list {
            let v = row_values(&c, row).unwrap();
            let m = gltf::gltf_material(&c, row).unwrap();
            // Thin carries its block (its recipe stage draws wallpaper, batch
            // 3), and so do foliage (in the recipe stage since B2-P1) and
            // glass (B3-P3); a see-through glass row is blended.
            assert_eq!(m["extras"].get("evani_recipe").is_some(), recipe != "water", "{}", row.id);
            assert_eq!(m.get("alphaMode").and_then(|a| a.as_str()) == Some("BLEND"), recipe == "glass", "{}", row.id);
            if matches!(v.variant, "wallpaper" | "newsprint" | "lace") {
                // A wall, not a light-through sheet: no nature kind at all.
                assert!(m["extras"].get("evani_kind").is_none() && m.get("extensions").is_none(), "{}", row.id);
                continue;
            }
            let want = NatureKind::from_name(nature_kind(recipe, v.variant)).expect("a name the nature pass reads");
            assert_eq!(material_kind(&doc_of(&m), 0), Some(want), "{}", row.id);
            // Without the tag, the extensions alone still pick the right class.
            let mut bare = m.clone();
            bare["extras"].as_object_mut().unwrap().remove("evani_kind");
            let k = material_kind(&doc_of(&bare), 0);
            let ok = match recipe {
                "water" => k == Some(NatureKind::WaterRect),
                "glass" => k == Some(NatureKind::GlassPane),
                _ => k == Some(NatureKind::ThinBlob),
            };
            assert!(ok, "{}: the extension fallback picks {k:?}", row.id);
            // Foliage's block reads back to the row (B2-P1).
            if recipe == "foliage" {
                assert_eq!(m["extras"]["evani_recipe"], json!(c.encode(row).unwrap().to_hex()), "{}", row.id);
                assert_eq!(evani_material_recipes::Block::from_hex(m["extras"]["evani_recipe"].as_str().unwrap()).unwrap(), c.encode(row).unwrap());
            }
        }
    }
}

#[test]
fn apply_keeps_textures_and_drops_stale_family_data() {
    let c = cat();
    let skin = gltf::gltf_material(&c, rows(&c, "skin")[0]).unwrap();
    let mut m = json!({"name": "body", "pbrMetallicRoughness": {"baseColorTexture": {"index": 0}, "metallicFactor": 1.0}, "normalTexture": {"index": 1}, "alphaMode": "MASK", "extras": {"artist": "kept"}, "extensions": {"KHR_texture_transform": {}}});
    gltf::apply(&mut m, &skin);
    assert_eq!(m["name"], json!("body"), "the model's own name stays");
    assert_eq!(m["pbrMetallicRoughness"]["baseColorTexture"], json!({"index": 0}));
    assert_eq!(m["normalTexture"], json!({"index": 1}));
    assert_eq!(m["alphaMode"], json!("MASK"));
    assert_eq!(m["extras"]["artist"], json!("kept"));
    assert!(m["extensions"]["KHR_texture_transform"].is_object());
    assert_eq!(m["extras"]["evani_kind"], json!("skin"));
    assert_eq!(m["pbrMetallicRoughness"]["metallicFactor"], json!(0.0));
    // The same material renamed to a wood row: no skin data is left.
    let wood = gltf::gltf_material(&c, c.row("wood.walnut.stock").unwrap()).unwrap();
    gltf::apply(&mut m, &wood);
    assert!(m["extras"].get("evani_kind").is_none() && m["extras"].get("evani_skin").is_none());
    assert_eq!(m["extras"]["evani_material"], json!("wood.walnut.stock"));
    assert!(m["extras"]["evani_recipe"].is_string());
    // Velvet onto a glass material: the transmission extensions go.
    let mut g = gltf::gltf_material(&c, rows(&c, "glass")[0]).unwrap();
    gltf::apply(&mut g, &gltf::gltf_material(&c, rows(&c, "cloth")[0]).unwrap());
    assert!(g.pointer("/extensions/KHR_materials_transmission").is_none());
}

#[test]
fn unknown_ids_are_an_error_and_change_nothing() {
    let c = cat();
    let mut doc = json!({"materials": [{"extras": {"evani_material": "wood.walnut.stock"}}, {"extras": {"evani_material": "no.such.row"}}]});
    let before = doc.clone();
    let e = gltf::apply_catalog(&mut doc, &c).unwrap_err();
    assert!(e.to_string().contains("no.such.row"), "{e}");
    assert_eq!(doc, before);
}

/// A real model (the sandbox's coated ball and the character head), its
/// materials naming catalogue rows only, filled by `apply_catalog`, written
/// as a .glb and read back with evani-gltf (the engine's model reader):
/// every material reaches its row's block or family record.
#[test]
fn a_real_glb_reaches_the_right_blocks_by_name() {
    let c = cat();
    let ball = include_bytes!("../../../apps/sandbox/assets/models/layer-coat.glb");
    let (mut doc, bin) = gltf::glb_parts(ball).unwrap();
    for m in doc["materials"].as_array_mut().unwrap() {
        m["extras"] = json!({"evani_material": "wood.walnut.stock"});
    }
    let done = gltf::apply_catalog(&mut doc, &c).unwrap();
    assert!(!done.is_empty());
    let glb = gltf::glb_build(&doc, bin.as_deref()).unwrap();
    let model = evani_gltf::Gltf::from_glb(&glb).unwrap();
    let want = c.encode(c.row("wood.walnut.stock").unwrap()).unwrap();
    for m in &model.materials {
        let hex = m.extras.get("evani_recipe").and_then(Value::as_str).unwrap();
        assert_eq!(Block::from_hex(hex).unwrap(), want);
        assert_eq!(m.extras["evani_material"], json!("wood.walnut.stock"));
    }

    let head = include_bytes!("../../../tests/fixtures/models/char-head.glb");
    let names = [("skin", "skin.tone.05.neutral"), ("eye", "eye.brown_dark"), ("hair", "hair.ginger"), ("hair_base", "finish.lacquer.black")];
    let (mut doc, bin) = gltf::glb_parts(head).unwrap();
    for m in doc["materials"].as_array_mut().unwrap() {
        let name = m["name"].as_str().unwrap_or_default().to_string();
        if let Some((_, id)) = names.iter().find(|(n, _)| *n == name) {
            m["extras"]["evani_material"] = json!(id);
        }
    }
    gltf::apply_catalog(&mut doc, &c).unwrap();
    let model = evani_gltf::Gltf::from_glb(&gltf::glb_build(&doc, bin.as_deref()).unwrap()).unwrap();
    assert_eq!(model.materials.len(), 5);
    for m in &model.materials {
        let mut w = [0.0f32; WORDS];
        let kind = record::apply(&m.extras, &mut w);
        match m.name.as_str() {
            "skin" => assert_eq!(kind, CharKind::Skin),
            "eye" => assert_eq!(kind, CharKind::Eye),
            "hair" => assert_eq!(kind, CharKind::Hair),
            "fur" => assert_eq!(kind, CharKind::Hair, "an untouched material keeps its own tags"),
            "hair_base" => {
                assert_eq!(kind, CharKind::Plain);
                let hex = m.extras.get("evani_recipe").and_then(Value::as_str);
                assert!(hex.is_some(), "a paint row on a head part carries its block");
            }
            other => panic!("unexpected material {other}"),
        }
    }
    let used = doc["extensionsUsed"].as_array().map_or(0, Vec::len);
    let carried = doc["materials"].as_array().unwrap().iter().filter(|m| m.get("extensions").is_some()).count();
    assert!(carried == 0 || used > 0, "extensions in use are listed");
}

#[test]
fn glb_parts_and_build_round_trip() {
    let ball = include_bytes!("../../../apps/sandbox/assets/models/layer-coat.glb");
    let (doc, bin) = gltf::glb_parts(ball).unwrap();
    let again = gltf::glb_build(&doc, bin.as_deref()).unwrap();
    let (doc2, bin2) = gltf::glb_parts(&again).unwrap();
    assert_eq!(doc, doc2);
    assert_eq!(bin, bin2);
    assert!(gltf::glb_parts(b"nope").is_err());
    let one = gltf::glb_with_material(ball, Some(0), &json!({"extras": {"evani_material": "x"}})).unwrap();
    assert_eq!(gltf::glb_parts(&one).unwrap().0["materials"][0]["extras"]["evani_material"], json!("x"));
}

/// P11b: every nature row sets only params the nature module registers, to
/// values inside their ranges (set, then read back unchanged), and each family
/// sets what it should: water its colour and waves, glass its tint, foliage a
/// glow colour, thin a light-through colour.
#[cfg(feature = "engine")]
#[test]
fn nature_rows_set_registered_params_inside_their_ranges() {
    use evani_core::params::{ParamRegistry, ParamValue};
    use evani_material_nature::params::NatureParams;
    use gltf::NatureValue;
    let c = cat();
    let mut reg = ParamRegistry::new();
    NatureParams::register(&mut reg).unwrap();
    let mut seen = 0;
    for row in c.rows.iter().filter(|r| ["foliage", "thin", "glass", "water"].contains(&r.recipe.as_str())) {
        let list = gltf::nature_params(&c, row).unwrap();
        assert!(!list.is_empty(), "{}: a nature row sets some params", row.id);
        for (name, v) in &list {
            let id = reg.id(name).unwrap_or_else(|| panic!("{}: the nature module registers no {name}", row.id));
            let want = match *v {
                NatureValue::F32(x) => ParamValue::F32(x),
                NatureValue::Colour(k) => ParamValue::Color(k),
            };
            reg.set_by_name(name, want).unwrap_or_else(|e| panic!("{}: {name}: {e}", row.id));
            assert_eq!(reg.value(id), Some(want), "{}: {name} is inside its range", row.id);
        }
        let has = |p: &str| list.iter().any(|(n, _)| n.starts_with(p));
        match row.recipe.as_str() {
            "water" => assert!(has("material.nature.water.deep_color") && has("material.nature.water.clarity") && has("material.nature.water.wave_strength"), "{}", row.id),
            "glass" => assert!(has("material.nature.glass.tint") && has("material.nature.glass.thickness"), "{}", row.id),
            _ => assert!(has("material.nature.foliage.color") || has("material.nature.grass.color") || has("material.nature.thin.color"), "{}", row.id),
        }
        seen += 1;
    }
    assert!(seen >= 40, "{seen} nature rows");
    let non_nature = c.rows.iter().find(|r| r.recipe == "wood").unwrap();
    assert!(gltf::nature_params(&c, non_nature).unwrap().is_empty(), "other families set none");
}

/// P11b: the water, glass and foliage quality groups drive the one nature
/// quality param, over its whole range.
#[cfg(feature = "engine")]
#[test]
fn the_water_glass_and_foliage_groups_drive_the_nature_quality_param() {
    use evani_core::params::{ParamRegistry, ParamValue};
    use evani_material_nature::params::{NatureParams, QUALITY};
    use evani_material_recipes::params::{MIRROR, MirrorValue, QUALITY as GROUPS};
    let mut reg = ParamRegistry::new();
    NatureParams::register(&mut reg).unwrap();
    for group in ["water", "glass", "foliage"] {
        let index = GROUPS.iter().position(|g| *g == format!("material.quality.{group}")).unwrap();
        let m = MIRROR.iter().find(|m| m.group == index).unwrap_or_else(|| panic!("{group} mirrors nothing"));
        assert_eq!(m.name, QUALITY, "{group}");
        let MirrorValue::F32(table) = m.value else { panic!("{group}: an f32 table") };
        assert_eq!(table, [-1.0, 0.0, 1.0, 2.0, 3.0, 3.0], "auto by tier, then lite to ultra");
        for v in table {
            assert!(reg.set_by_name(m.name, ParamValue::F32(v)).is_ok(), "{group}: {v}");
        }
    }
}
