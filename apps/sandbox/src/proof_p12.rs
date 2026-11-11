//! The P12 proof set (`?set=p12`, feature `recipes-family-proof`; dev only,
//! never in evani.toml): the optional batch-11 rows that are drawn, grouped
//! as vehicles (two rows), fantasy (gems and stones, then magic) and sci-fi.
//!
//! Every shape gets its material through the row-to-glTF adapter
//! (`evani_material_recipes::gltf`), the way a game's models will: the 20 rows
//! P5 to P10a already numbered and the 14 numbered in P12 (the same table the
//! catalogue holds, so a row's numbers are drawn here, not copied).

use evani_material_recipes::catalog::Catalog;
use evani_material_recipes::gltf;

/// The grid, back row first: vehicle paints and bright metal, vehicle parts,
/// gems and stones, magic, sci-fi.
pub const ROWS: [&[&str]; 5] = [
    &[
        "car.paint.solid_red",
        "car.paint.metallic_silver",
        "car.paint.metallic_blue",
        "car.paint.pearl_white",
        "car.paint.colour_shift",
        "car.paint.matte_black",
        "car.paint.primer",
        "car.steel.wheel",
        "car.carbon",
    ],
    &[
        "car.alloy.wheel",
        "car.engine.block",
        "car.iron.wheel_rim",
        "car.tyre.tread",
        "car.dashboard",
        "car.wood.wagon",
        "car.canvas.cover",
        "car.leather.seat",
    ],
    &[
        "gem.ruby",
        "gem.sapphire",
        "gem.emerald",
        "gem.diamond",
        "gem.amethyst",
        "gem.quartz_clear",
        "gem.amber",
        "gem.jade",
        "gem.turquoise",
        "gem.pearl",
    ],
    &["magic.rune", "magic.portal", "magic.cursed", "magic.holy"],
    &["sf.holo", "sf.energy_core", "sf.chitin"],
];

/// Flat cards (the portal and the cursed smoke), cubes (flat faces for the
/// planks, the dashboard and the carbon twill), the trunk (the tread) and
/// cones (the pointed crystals); the canvas rides on the curtain; the rest on
/// uv balls.
const CARDS: [&str; 2] = ["magic.portal", "magic.cursed"];
const CUBES: [&str; 3] = ["car.wood.wagon", "car.dashboard", "car.carbon"];
const POINTS: [&str; 2] = ["gem.amethyst", "gem.quartz_clear"];

fn err(e: impl core::fmt::Display) -> String {
    e.to_string()
}

/// Every shape of the set in grid order: its id and glb.
pub fn glbs(curtain: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let cat = Catalog::bundled().map_err(err)?;
    let (card, cube, trunk, horn, ball) = (
        crate::proof_recipes::card_glb(),
        crate::proof_recipes::cube_glb(),
        crate::proof_recipes::trunk_glb(),
        crate::proof_recipes::horn_glb(),
        crate::proof_recipes::sphere_glb(),
    );
    let mut out = Vec::new();
    for id in ROWS.iter().flat_map(|r| r.iter().copied()) {
        let row = cat.row(id).ok_or_else(|| format!("no row {id}"))?;
        let shape: &[u8] = if CARDS.contains(&id) {
            &card
        } else if CUBES.contains(&id) {
            &cube
        } else if id == "car.tyre.tread" {
            &trunk
        } else if POINTS.contains(&id) {
            &horn
        } else if id == "car.canvas.cover" {
            curtain
        } else {
            &ball
        };
        let m = gltf::gltf_material(&cat, row).map_err(err)?;
        out.push((id.to_string(), gltf::glb_with_material(shape, None, &m)?));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    /// Drawn in P11b (the nature family's rows), shown in `?set=p11b`, not here.
    const SHOWN_BY_P11B: [&str; 2] = ["car.windscreen", "magic.slime"];

    /// The set is exactly the optional rows that are ready (a row drawn is in
    /// it, a row in it is drawn), the grid rows add up, and every shape builds.
    #[test]
    fn the_p12_set_is_the_ready_optional_rows() {
        let cat = Catalog::bundled().unwrap();
        let shown: Vec<&str> = ROWS.iter().flat_map(|r| r.iter().copied()).collect();
        let ready: Vec<&str> = cat.rows.iter().filter(|r| r.batch == 11 && r.status == "ready" && !SHOWN_BY_P11B.contains(&r.id.as_str())).map(|r| r.id.as_str()).collect();
        let mut a = shown.clone();
        let mut b = ready;
        a.sort_unstable();
        b.sort_unstable();
        assert_eq!(a, b, "the proof set and the ready batch-11 rows are the same rows");
        assert_eq!(ROWS.iter().map(|r| r.len()).collect::<Vec<_>>(), crate::proof_recipes::P12_ROWS);
        let curtain = include_bytes!("../assets/models/layer-velvet.glb");
        let all = glbs(curtain).unwrap();
        assert_eq!(all.len(), shown.len());
        // What the engine reads at load: recipe rows carry their block; the
        // canvas cover is cloth, which since batch 3 is a recipe (its block)
        // composed with the layers family (its sheen and fuzz).
        let prep = |glb: &[u8]| evani_render::model::prepare(&evani_gltf::Gltf::from_glb(glb).unwrap()).unwrap();
        let red = prep(&all[0].1);
        assert_eq!(red.materials[0].recipe, cat.encode(cat.row("car.paint.solid_red").unwrap()).unwrap().to_words());
        let alloy = prep(&all[9].1);
        assert_eq!(alloy.materials[0].recipe, cat.encode(cat.row("car.alloy.wheel").unwrap()).unwrap().to_words());
        let canvas = prep(&all[9 + 6].1);
        assert_eq!(canvas.materials[0].recipe, cat.encode(cat.row("car.canvas.cover").unwrap()).unwrap().to_words(), "canvas cover: the cloth block");
        assert!(canvas.materials[0].layers[0] > 0.0, "canvas cover: the layers family's sheen");
    }
}
