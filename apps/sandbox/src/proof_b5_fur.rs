//! The B5-P1 coat page (`?set=b5-fur`, feature `recipes-family-proof`; dev
//! only, never in evani.toml): the fur rows that use batch 5's new variants,
//! on the ball like `?set=p6`, through the row-to-glTF adapter like a game's
//! models.
//!
//! - back row: five roans (steel grey, red, blue, bay, rabicano), the merle
//!   dog and the giraffe;
//! - front row: tortoiseshell, calico, the speckled longhorn, four sheep
//!   fleeces and the curly dog.

use evani_material_recipes::catalog::Catalog;
use evani_material_recipes::gltf;

/// Every shape of the page in grid order (back row first).
pub const IDS: [&str; 16] = [
    "horse.coat.grey_steel",
    "horse.coat.red_roan",
    "horse.coat.blue_roan",
    "horse.coat.bay_roan",
    "horse.coat.rabicano",
    "dog.coat.merle",
    "giraffe.coat",
    "sheep.wool.white",
    "cat.coat.tortoiseshell",
    "cat.coat.calico",
    "cattle.coat.speckled",
    "sheep.wool.black",
    "sheep.wool.brown",
    "sheep.wool.grey",
    "sheep.skin.shorn",
    "dog.coat.curly_brown",
];

/// How many shapes each grid row holds, back to front.
pub const ROWS: [usize; 2] = [8, 8];

/// The page's grid number (`?set=b5-fur`).
pub const GRID: u8 = 18;

fn err(e: impl core::fmt::Display) -> String {
    e.to_string()
}

/// Every shape of the page: its id and glb. `ball` is the uv sphere the fur
/// rows ride on.
pub fn glbs(ball: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let cat = Catalog::bundled().map_err(err)?;
    let mut out = Vec::new();
    for id in IDS {
        let row = cat.row(id).ok_or_else(|| format!("no row {id}"))?;
        let m = gltf::gltf_material(&cat, row).map_err(err)?;
        out.push((id.to_string(), gltf::glb_with_material(ball, None, &m).map_err(err)?));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    /// Every row is drawn (ready, fur), the grid rows add up, every shape
    /// builds, and no two coats share a block.
    #[test]
    fn the_coat_page_builds_through_the_adapter() {
        let cat = Catalog::bundled().unwrap();
        assert_eq!(ROWS.iter().sum::<usize>(), IDS.len());
        for id in IDS {
            let row = cat.row(id).unwrap();
            assert_eq!(row.status, "ready", "{id}");
            assert_eq!(row.recipe, "fur", "{id}");
        }
        let ball = include_bytes!("../assets/models/layer-coat.glb");
        let all = glbs(ball).unwrap();
        assert_eq!(all.len(), IDS.len());
        let mut hexes: Vec<String> = IDS.iter().map(|id| cat.encode(cat.row(id).unwrap()).unwrap().to_hex()).collect();
        hexes.sort();
        hexes.dedup();
        assert_eq!(hexes.len(), IDS.len(), "every coat has its own block");
    }
}
