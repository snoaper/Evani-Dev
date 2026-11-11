//! The P10a proof set (`?set=p10a`, feature `recipes-family-proof`: the
//! recipes proof plus the layers and character families and the row-to-glTF
//! adapter with the bundled catalogue; dev only, never in evani.toml).
//!
//! Every shape is a real model whose materials reach the catalogue through
//! the adapter (`evani_material_recipes::gltf`), as a game's models will:
//!
//! - back row: the character head fixture (`tests/fixtures/models/char-head.glb`)
//!   four times; its `skin`, `eye` and `hair` materials only *name* a skin
//!   tone, `eye.brown_dark` and a hair row in `extras.evani_material`, and
//!   `apply_catalog` fills them in (the character family draws them);
//! - middle row: the velvet curtain fixture with the eight cloth rows (the
//!   layers family's sheen, weave and fuzz);
//! - front row: the coated ball with eight paint rows, drawn by the recipe
//!   stage with paint's clear coat from the block (P10a); the third is the
//!   same red with its `clearcoat` extra off, beside the coated one.

use evani_material_recipes::catalog::Catalog;
use evani_material_recipes::gltf;
use evani_material_recipes::Block;
use serde_json::{Value, json};

/// The head fixture (1.1 MB, compiled in only with `recipes-family-proof`).
const HEAD: &[u8] = include_bytes!("../../../tests/fixtures/models/char-head.glb");

/// Back row: (skin row, hair row) per head; every head has [`EYE`].
pub const HEADS: [(&str, &str); 4] = [
    ("skin.tone.02.warm", "hair.blond"),
    ("skin.tone.05.neutral", "hair.black"),
    ("skin.tone.08.cool", "hair.ginger"),
    ("skin.tone.10.warm", "hair.grey"),
];
/// The eye row every head uses.
pub const EYE: &str = "eye.brown_dark";
/// Middle row: the cloth rows, on the curtain.
pub const CLOTH: [&str; 8] = [
    "cloth.cotton.white",
    "cloth.burlap",
    "cloth.canvas.natural",
    "cloth.denim.indigo",
    "cloth.knit.chunky",
    "cloth.velvet.red",
    "cloth.silk.plain",
    "tool.rope.hemp",
];
/// Front row: paint rows on the ball; `!nocoat` is the row with its
/// `clearcoat` extra off.
pub const PAINT: [&str; 8] = [
    "finish.clearcoat",
    "car.paint.solid_red",
    "car.paint.solid_red!nocoat",
    "car.paint.metallic_blue",
    "car.paint.pearl_white",
    "car.paint.colour_shift",
    "finish.lacquer.black",
    "car.paint.matte_black",
];

/// The head's own scale and lift so it stands on the ground about as big as
/// a proof ball (the fixture is life size, centred near the nose).
const HEAD_SCALE: f32 = 4.0;
const HEAD_LIFT: f32 = 0.68;

fn err(e: impl core::fmt::Display) -> String {
    e.to_string()
}

/// A head whose materials name the rows, filled in by `apply_catalog`.
pub fn head_glb(cat: &Catalog, skin: &str, hair: &str) -> Result<Vec<u8>, String> {
    let (mut doc, bin) = gltf::glb_parts(HEAD)?;
    for m in doc.get_mut("materials").and_then(Value::as_array_mut).ok_or("head: no materials")? {
        let id = match m.get("name").and_then(Value::as_str) {
            Some("skin") => skin,
            Some("eye") => EYE,
            Some("hair") => hair,
            _ => continue,
        };
        m["extras"] = json!({ "evani_material": id });
    }
    gltf::apply_catalog(&mut doc, cat).map_err(err)?;
    let roots: Vec<usize> = doc
        .pointer("/scenes/0/nodes")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|n| n.as_u64().map(|n| n as usize)).collect())
        .unwrap_or_default();
    for r in roots {
        if let Some(n) = doc.pointer_mut(&format!("/nodes/{r}")).and_then(Value::as_object_mut) {
            n.insert("scale".into(), json!([HEAD_SCALE, HEAD_SCALE, HEAD_SCALE]));
            n.insert("translation".into(), json!([0.0, HEAD_LIFT, 0.0]));
        }
    }
    gltf::glb_build(&doc, bin.as_deref())
}

/// The material written for a [`PAINT`] entry (`!nocoat`: the clearcoat
/// extra cleared in its block).
pub fn paint_material(cat: &Catalog, entry: &str) -> Result<Value, String> {
    let (id, nocoat) = entry.strip_suffix("!nocoat").map_or((entry, false), |id| (id, true));
    let row = cat.row(id).ok_or_else(|| format!("no row {id}"))?;
    let mut m = gltf::gltf_material(cat, row).map_err(err)?;
    if nocoat {
        let mut f = cat.encode(row).map_err(err)?.unpack();
        f.extras &= !1;
        m["extras"]["evani_recipe"] = json!(Block::pack(&f).to_hex());
    }
    Ok(m)
}

/// Every shape of the set, in grid order (back row first): its id and glb.
pub fn glbs(curtain: &[u8], ball: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let cat = Catalog::bundled().map_err(err)?;
    let mut out = Vec::new();
    for (skin, hair) in HEADS {
        out.push((format!("head-{skin}"), head_glb(&cat, skin, hair)?));
    }
    for id in CLOTH {
        let row = cat.row(id).ok_or_else(|| format!("no row {id}"))?;
        let m = gltf::gltf_material(&cat, row).map_err(err)?;
        out.push((id.to_string(), gltf::glb_with_material(curtain, None, &m)?));
    }
    for entry in PAINT {
        out.push((entry.replace('!', "-"), gltf::glb_with_material(ball, None, &paint_material(&cat, entry)?)?));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    /// Every shape builds, the grid rows add up, and each head's materials
    /// reach their rows (read back with the engine's glTF reader).
    #[test]
    fn the_p10a_set_builds_through_the_adapter() {
        let cat = Catalog::bundled().unwrap();
        let curtain = include_bytes!("../assets/models/layer-velvet.glb");
        let ball = include_bytes!("../assets/models/layer-coat.glb");
        let all = glbs(curtain, ball).unwrap();
        assert_eq!(all.len(), crate::proof_recipes::P10A_ROWS.iter().sum::<usize>());
        // Shown here and judged: ready. Rope (a twist is geometry) is shown
        // but stays planned (DECISIONS P10a); denim waited for its twill,
        // which batch 3's cloth recipe draws, so it is ready; the eye got its
        // own inner ring in P11a and is ready.
        let planned = ["tool.rope.hemp"];
        for id in HEADS.iter().flat_map(|(s, h)| [*s, *h]).chain([EYE]).chain(CLOTH) {
            let want = if planned.contains(&id) { "planned" } else { "ready" };
            assert_eq!(cat.row(id).unwrap().status, want, "{id}");
        }
        let head = evani_gltf::Gltf::from_glb(&all[0].1).unwrap();
        let ids: Vec<&str> = head.materials.iter().filter_map(|m| m.extras.get("evani_material").and_then(Value::as_str)).collect();
        assert_eq!(ids, ["skin.tone.02.warm", "eye.brown_dark", "hair.blond"]);
        let red = paint_material(&cat, "car.paint.solid_red").unwrap();
        let bare = paint_material(&cat, "car.paint.solid_red!nocoat").unwrap();
        let bit = |m: &Value| Block::from_hex(m["extras"]["evani_recipe"].as_str().unwrap()).unwrap().unpack().extras & 1;
        assert_eq!((bit(&red), bit(&bare)), (1, 0), "only the coat switch differs");
        // What the engine reads at load: the head's skin is the character
        // family's (no block), the curtain's cloth the layers family's sheen
        // over its batch 3 cloth block, the paint ball's block the recipe
        // stage's.
        let prep = |glb: &[u8]| evani_render::model::prepare(&evani_gltf::Gltf::from_glb(glb).unwrap()).unwrap();
        let head = prep(&all[0].1);
        let words = |i: usize| head.materials[i].surface.words;
        assert_eq!((words(0)[14], words(1)[14], words(2)[14]), (1.0, 2.0, 3.0), "skin, eye, hair kinds");
        assert!(head.materials.iter().take(3).all(|m| m.recipe == [0; 8]), "one path per material");
        let velvet = prep(&all[4 + 5].1);
        assert!(velvet.materials[0].layers[0] > 0.1, "velvet sheen from the layers family");
        assert_eq!(velvet.materials[0].recipe, cat.encode(cat.row(CLOTH[5]).unwrap()).unwrap().to_words(), "velvet: its cloth block");
        let coat = prep(&all[4 + 8 + 1].1);
        let want = cat.encode(cat.row("car.paint.solid_red").unwrap()).unwrap().to_words();
        assert_eq!(coat.materials[0].recipe, want);
    }
}
