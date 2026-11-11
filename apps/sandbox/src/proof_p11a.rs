//! The P11a proof set (`?set=p11a`, feature `recipes-family-proof`; dev only,
//! never in evani.toml): the people and cloth rows P11a numbered, through the
//! row-to-glTF adapter like a game's models.
//!
//! - back row: six character heads (the head fixture, whose skin, eye and hair
//!   materials only name rows): four skin tones, the freckled trait and the
//!   elder trait; every head has `eye.brown_dark`, whose inner ring is its own
//!   colour now;
//! - front row: the bay horse coat with its new dapple and countershading,
//!   the same row with its old numbers beside it (before and after), then
//!   three cloth rows that stay planned and are shown so the reason can be
//!   judged: denim, the striped wool blanket and hemp rope.

use evani_material_recipes::catalog::Catalog;
use evani_material_recipes::gltf;
use serde_json::Value;

/// Back row: (skin row, hair row) per head.
pub const HEADS: [(&str, &str); 6] = [
    ("skin.tone.02.warm", "hair.blond"),
    ("skin.tone.05.neutral", "hair.black"),
    ("skin.tone.08.cool", "hair.ginger"),
    ("skin.tone.10.warm", "hair.grey"),
    ("skin.trait.freckled", "hair.ginger"),
    ("skin.trait.elder", "hair.grey"),
];
/// Front row: the horse rows and the planned cloth rows.
pub const FRONT: [&str; 5] = ["horse.coat.bay", "horse.coat.bay!before", "cloth.denim.indigo", "cloth.wool.blanket", "tool.rope.hemp"];

/// The bay horse's numbers before P11a (plain solid coat), for the before side.
const BAY_BEFORE: &str = r#"{"variant":"solid","colours":["5a2a1a","8a4a2c","1a1410"],"numbers":{"length":0.1,"density":0.9,"sheen":0.55,"sheen_shift":0.4,"pattern_scale":8,"pattern_contrast":0.5,"flow_angle":90,"clump":0.05,"points":0.85},"extras":["pattern"]}"#;

fn err(e: impl core::fmt::Display) -> String {
    e.to_string()
}

/// The material for a [`FRONT`] entry (`!before`: the row with its old numbers).
pub fn front_material(cat: &Catalog, entry: &str) -> Result<Value, String> {
    let (id, before) = entry.strip_suffix("!before").map_or((entry, false), |id| (id, true));
    let mut row = cat.row(id).ok_or_else(|| format!("no row {id}"))?.clone();
    if before {
        let old: Value = serde_json::from_str(BAY_BEFORE).map_err(err)?;
        row.params = old.as_object().cloned().ok_or("params")?;
    }
    gltf::gltf_material(cat, &row).map_err(err)
}

/// Every shape of the set in grid order (back row first): its id and glb.
/// `curtain` carries the cloth, `ball` the horse.
pub fn glbs(curtain: &[u8], ball: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let cat = Catalog::bundled().map_err(err)?;
    let mut out = Vec::new();
    for (skin, hair) in HEADS {
        out.push((format!("head-{skin}"), crate::proof_p10a::head_glb(&cat, skin, hair)?));
    }
    for entry in FRONT {
        let m = front_material(&cat, entry)?;
        let shape = if entry.starts_with("horse") { ball } else { curtain };
        out.push((entry.replace('!', "-"), gltf::glb_with_material(shape, None, &m)?));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    /// Every shape builds, the grid rows add up, the new people rows are
    /// drawn, the horse's two sides differ and the cloth rows are the ones
    /// that stay planned.
    #[test]
    fn the_p11a_set_builds_through_the_adapter() {
        let cat = Catalog::bundled().unwrap();
        let curtain = include_bytes!("../assets/models/layer-velvet.glb");
        let ball = include_bytes!("../assets/models/layer-coat.glb");
        let all = glbs(curtain, ball).unwrap();
        assert_eq!(all.len(), crate::proof_recipes::P11A_ROWS.iter().sum::<usize>());
        for id in HEADS.iter().flat_map(|(s, h)| [*s, *h]).chain([crate::proof_p10a::EYE, "horse.coat.bay"]) {
            assert_eq!(cat.row(id).unwrap().status, "ready", "{id}");
        }
        let after = front_material(&cat, "horse.coat.bay").unwrap();
        let before = front_material(&cat, "horse.coat.bay!before").unwrap();
        assert_ne!(after["extras"]["evani_recipe"], before["extras"]["evani_recipe"], "the horse's two sides differ");
        // Denim is ready since batch 3's cloth recipe draws its twill.
        assert_eq!(cat.row("cloth.denim.indigo").unwrap().status, "ready");
        for id in ["cloth.wool.blanket", "tool.rope.hemp"] {
            assert_eq!(cat.row(id).unwrap().status, "planned", "{id}");
        }
        // The freckled head's skin names its spots; the eye its ring.
        let prep = |glb: &[u8]| evani_render::model::prepare(&evani_gltf::Gltf::from_glb(glb).unwrap()).unwrap();
        let freckled = prep(&all[4].1);
        let words = freckled.materials[0].surface.words;
        assert!(words[10] > 0.5, "freckles on the skin: {}", words[10]);
        let eye = freckled.materials[1].surface.words;
        assert!(eye[12] > 0.0 && eye[15] > 0.0 && eye[14] == 2.0, "the eye's ring colour: {eye:?}");
    }
}
