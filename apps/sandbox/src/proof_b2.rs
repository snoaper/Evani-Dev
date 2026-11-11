//! The batch 2 options proof (`?set=b2-options`, feature `recipes-proof`, dev
//! only, never in evani.toml). The 23 catalogue rows the new options of ground,
//! stone, bark and ice unlock, in three rows back to front: ground, snow and
//! ice on cubes (a real uv square, a flat top for the cover, cracks and rows),
//! stone and obsidian on uv spheres, bark on trunks. Larger than the other
//! sets' shapes (scale 1.0, not 0.5), so a crack or a furrow reads at its real
//! size. `?row=<n>` (0 to 2) frames one row; `?q=` and `?yaw=` as in the other
//! sets. A test holds every block equal to the catalogue row.
//!
//! B2-P7a adds `?set=b2-land` (grid 15): the 37 land rows whose numbers were
//! set afterwards, in six rows back to front: soil and mud, sand, gravel and
//! frozen ground on cubes, rock on spheres (two rows), the four plain barks
//! on trunks (`?row=0` to `5`). Same shapes, scale and options.

use evani_core::camera::CameraPose;

/// `?set=b2-options` is grid 12 in `proof_recipes`.
pub const GRID: u8 = 13;

/// `?set=b2-land` is grid 15.
pub const GRID_LAND: u8 = 15;

/// The shapes' scale (the other sets draw at 0.5).
pub const SCALE: f32 = 1.0;

/// How many shapes each row holds, back to front: ground and snow, rock,
/// bark.
pub const ROWS: [usize; 3] = [8, 8, 7];

/// The land set's rows: three of ground, two of rock, one of bark.
pub const ROWS_LAND: [usize; 6] = [6, 6, 6, 8, 7, 4];

/// Whether grid `g` is one of this module's sets.
pub fn is_grid(g: u8) -> bool {
    g == GRID || g == GRID_LAND
}

/// The rows of grid `g`.
pub fn rows(g: u8) -> &'static [usize] {
    if g == GRID_LAND { &ROWS_LAND } else { &ROWS }
}

/// The catalogue rows of grid `g`.
pub fn table(g: u8) -> &'static [(&'static str, &'static str, f32, f32)] {
    if g == GRID_LAND { &B2L } else { &B2O }
}

/// The grid: (catalogue id, its block as the catalogue encodes it, fallback
/// roughness, fallback metalness).
pub const B2O: [(&str, &str, f32, f32); 23] = [
    ("ground.soil.cracked_dry", "090004c9b8a8888a7a62a89878e6080000240026150000e80000000000000000", 0.9, 0.0),
    ("ground.soil.alkali", "090006c97a766e9a968ef0eee6d908000024cc1a1f0000290000000000000000", 0.9, 0.0),
    ("ground.soil.ploughed", "090008c93a2c206a60545a4a38f20d1a00240099002f00190000000000000000", 0.9, 0.0),
    ("ground.soil.wagon_ruts", "090408c96a54388a7a624a5a30e6084d0024004d000a00de0000000000000000", 0.9, 0.0),
    ("ground.soil.trail", "090408c97a64448a7a625a7a34e608330024004d002400e10000000000000000", 0.9, 0.0),
    ("ground.soil.forest_floor", "090010c93a2e226a62547a5a2cf2310d002400800000e6b70000000000000000", 0.9, 0.0),
    ("ground.corral_dirt", "090010c95a46327a6a58c8a850f23b1a002400cc000080b50000000000000000", 0.9, 0.0),
    ("ground.snow.crust", "1b020e86e8ecf4b0bcccf8fbff1a0d24804dcc00000000b20000000000000000", 0.3, 0.0),
    ("rock.limestone", "060401ddb4b2a898968cd8d6ccd9002f4c0000004d0000840000000000000000", 0.8, 0.0),
    ("rock.travertine", "060400ddd8c8a8c0ae88ece0c4b30044329900007300001a0000000000000000", 0.8, 0.0),
    ("rock.pumice", "060400ddb8b6b09a9892d8d6d0f2006d65000000e60000d70000000000000000", 0.8, 0.0),
    ("rock.lava.cooled", "060500dd2a2826403c386a5848cc00243f000000000000960000000000000000", 0.8, 0.0),
    ("rock.conglomerate", "060601dd8a6e586a5a4eb09a7ed9002a4c0000008c0000ea0000000000000000", 0.8, 0.0),
    ("rock.cave_wall", "060700dd4a443c5c564a8a807080001a32000000000000789900000000000000", 0.8, 0.0),
    ("rock.stalactite", "060700ddc8a880b88a5ce8d4b073002432000000000000a08000000000000000", 0.8, 0.0),
    ("rock.obsidian", "190301dd08080c3a3a440000000040230d000000000000bb0000000000000000", 0.1, 0.0),
    ("bark.redwood", "050401cd7a3c283c1c148a5238f299442f92001a000000e70000000000000000", 0.9, 0.0),
    ("bark.cypress", "050401cd7a6a5c443a328a7a6ae6802f2f6d0033000000360000000000000000", 0.9, 0.0),
    ("bark.vine_woody", "050401cd5a4a382e241a7a6a4ae6cc1a2f4900e6000000240000000000000000", 0.9, 0.0),
    ("bark.palm", "050501cd8a7a664a3e30a89a80d9b3582f240000000000d70000000000000000", 0.9, 0.0),
    ("bark.cherry", "050600cd6a2e222a140e8a46364d332f2f240000000000e60000000000000000", 0.4, 0.0),
    ("bark.aspen", "050202cdd8d4c08a86702a2a24991a2f2f2400000000001e0000000000000000", 0.9, 0.0),
    ("bark.burnt", "050105cd1c1a180a0908ff6a20e6e6241a2400000000006e0000000000000000", 0.9, 0.0),
];

/// The land set's rows (B2-P7a): catalogue id, block, fallback roughness and
/// metalness. Generated from the catalogue; a test holds every block equal.
pub const B2L: [(&str, &str, f32, f32); 37] = [
// BEGIN B2L
    ("ground.soil.clay_red", "090000c9a8502e8a7a6ac47a50cc080d00240059000000360000000000000000", 0.8, 0.0),
    ("ground.soil.clay_grey", "090000c96a70788a8e929aa0a473080500240033000000d80000000000000000", 0.45, 0.0),
    ("ground.soil.peat", "090000c92a1e164a3a2c5a4630f21214002400d9000000590000000000000000", 0.95, 0.0),
    ("ground.soil.ash_volcanic", "090002c91c1c1e3a3a3e58585cf203590024404d0000006b0000000000000000", 0.95, 0.0),
    ("ground.mud.riverbank", "090300c96a5a468a8070a8987859080a80440080000000e40000000000000000", 0.35, 0.0),
    ("ground.mud.tidal", "090300c94a4a487a7a748a8a84260805596300b3000000ec0000000000000000", 0.15, 0.0),
    ("ground.sand.desert", "09010289b4723ac8884ae0a860f20300d91f4d00000000e50000000000000000", 0.95, 0.0),
    ("ground.sand.white", "09010289c8c0a8dcd6c4f4f0e4f203054d396600000000110000000000000000", 0.95, 0.0),
    ("ground.sand.black", "090102891a1a1e2e2e344a4a52e603084d39e600000000b70000000000000000", 0.9, 0.0),
    ("ground.sand.wet", "090102898a7248a89264c8b48459030a402f4d00000000d0cc00000000000000", 0.35, 0.0),
    ("ground.sand.river", "090101896a5c488a8272b0a080f20859262a0000000000560000000000000000", 0.95, 0.0),
    ("ground.gravel.pea", "0902014d7a6a56a09484c8b896e605f200240000000000d70000000000000000", 0.9, 0.0),
    ("ground.gravel.shingle", "0902014d8a8680c8c4bce8e6e0cc15f200240000000000eb0000000000000000", 0.8, 0.0),
    ("ground.gravel.scree", "0905014d4a463e7a766ea09a8ee62ce600240000000000c30000000000000000", 0.9, 0.0),
    ("ground.gravel.railway", "0905014d2a28264a48466a6662e60ff200240000000000020000000000000000", 0.9, 0.0),
    ("ground.gravel.cinders", "0905014d1a16143a34305a4a40f212d900240099000000e30000000000000000", 0.95, 0.0),
    ("ground.gravel.creek", "0902014d4a4a447a766aa8a090991cf200240000000000c5b300000000000000", 0.6, 0.0),
    ("ground.permafrost", "090002865a5a5c8a8c90d8e4eccc0d400024d94d000000650000000000000000", 0.8, 0.0),
    ("rock.granite.pink", "060003ddb88a823a3236e8d8cc99001a7f008000000000d60000000000000000", 0.6, 0.0),
    ("rock.sandstone.buff", "060201ddd8c4a0b8a47eece0c4e60039bf990000000000600000000000000000", 0.9, 0.0),
    ("rock.sandstone.striped", "060201ddc8502ee8a868f0dcb4e6002abfff0000000000340000000000000000", 0.9, 0.0),
    ("rock.chalk", "060000ddece8e0d4d0c4f8f6f0f2001a72000000000000e90000000000000000", 0.95, 0.0),
    ("rock.slate", "060203dd4a4e56363a426a707a8c006398cc2600000000eb0000000000000000", 0.55, 0.0),
    ("rock.shale", "060201dd4a464034302c625a50e6008d7fe60000000000d70000000000000000", 0.9, 0.0),
    ("rock.marble.white", "060106ddecebe6d0cec88a8a904059247f004099000000640000000000000000", 0.25, 0.0),
    ("rock.marble.green", "060106dd1e3a2c14281ee8ecdc40732f7f00334d0000008a0000000000000000", 0.25, 0.0),
    ("rock.quartzite", "060003dde4dcd2d4a8a4f4f0ea99001ae500cc00000000ef0000000000000000", 0.6, 0.0),
    ("rock.flint", "060102dd2a2c301a1c20d8d4c84d4044ff008000000000650000000000000000", 0.3, 0.0),
    ("rock.mossy_boulder", "060001dd7a7a745a5a549a9a90e6001a4c000000000000f10000000000009900", 0.9, 0.0),
    ("rock.ore.iron", "060303dd4a2a22241a18a0482cb3001a650066000000009a0000000000000000", 0.7, 0.0),
    ("rock.ore.copper", "060101dd7a7a785e6a6a3aa890bf802465000000000000590000000000000000", 0.75, 0.0),
    ("rock.ore.gold_vein", "060303dde8e4dcc8c4b8e0b04080001a5800b300000000250000000000000000", 0.5, 0.0),
    ("rock.ore.coal", "060203dd1a1a1c0c0c0e3a3a3e4d0058bf998000000000130000000000000000", 0.3, 0.0),
    ("bark.beech", "050200cd6e6a6268645c7a7a6ab3261a2f2400000000000a0000000000000000", 0.7, 0.0),
    ("bark.cottonwood", "050001cd7a74682a26209a9484e6d91a2fb60000000000890000000000000000", 0.9, 0.0),
    ("bark.mangrove", "050001cd5a4e442a221c7a6c58b3992a2f6d0000000000e40000000000000000", 0.7, 0.0),
    ("bark.dead_snag", "050401cda09c923e3a34b8b4aae64d582fff0005000000e70000000000000000", 0.9, 0.0),
// END B2L
];

/// The shape a row rides on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// Ground, snow and ice: a cube.
    Cube,
    /// Rock: a uv sphere.
    Sphere,
    /// Bark: a trunk.
    Trunk,
}

/// The shape row `id` rides on.
pub fn kind(id: &str) -> Kind {
    if id.starts_with("bark.") {
        Kind::Trunk
    } else if id.starts_with("rock.") {
        Kind::Sphere
    } else {
        Kind::Cube
    }
}

/// Where shape `k` stands: rows back to front, 1.5 m apart, 1.12 m between
/// shapes, each row centred on x = 0.
pub fn pos(g: u8, k: usize) -> [f32; 3] {
    let rows = rows(g);
    let (mut row, mut i) = (0, k);
    while row < rows.len() - 1 && i >= rows[row] {
        i -= rows[row];
        row += 1;
    }
    let n = rows[row] as f32;
    [(i as f32 - (n - 1.0) * 0.5) * 1.12, 0.0, crate::proof_recipes::Z - 1.5 + 1.5 * row as f32]
}

/// The camera on the whole grid, from above and in front.
pub fn camera(g: u8) -> CameraPose {
    let z = crate::proof_recipes::Z;
    if g == GRID_LAND {
        CameraPose { eye: [0.0, 11.5, z + 15.0], target: [0.0, 0.6, z + 2.4], up: [0.0, 1.0, 0.0], fov_y_deg: 42.0 }
    } else {
        CameraPose { eye: [0.0, 6.4, z + 8.8], target: [0.0, 1.3, z - 0.2], up: [0.0, 1.0, 0.0], fov_y_deg: 42.0 }
    }
}

/// The camera close on row `row`, framing all its shapes.
pub fn row_camera(g: u8, row: usize) -> CameraPose {
    let z = crate::proof_recipes::Z - 1.5 + 1.5 * row.min(rows(g).len() - 1) as f32;
    CameraPose { eye: [0.0, 4.8, z + 7.4], target: [0.0, 1.0, z], up: [0.0, 1.0, 0.0], fov_y_deg: 42.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every block is the catalogue's for its row, each row is drawn by the
    /// recipes (ready) and the rows add up; options: ground, rock and bark.
    /// Land: ground (cubes), rock (spheres) and bark (trunks), in that order.
    #[test]
    fn the_blocks_are_the_catalogue_rows() {
        use evani_material_recipes::catalog::Catalog;
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../catalog/materials");
        let cat = Catalog::load_dir(&dir).unwrap();
        for g in [GRID, GRID_LAND] {
            let (rows, table) = (rows(g), table(g));
            assert_eq!(rows.iter().sum::<usize>(), table.len());
            for &(id, hex, _, _) in table {
                let row = cat.row(id).unwrap_or_else(|| panic!("{id} is a catalogue row"));
                assert_eq!(hex, cat.encode(row).unwrap().to_hex(), "{id}");
                assert_eq!(row.status, "ready", "{id}: the recipes draw it");
                assert!(row.params.contains_key("numbers"), "{id}: has numbers");
            }
            let kinds: Vec<Kind> = table.iter().map(|r| kind(r.0)).collect();
            let cubes = kinds.iter().take_while(|k| **k == Kind::Cube).count();
            let balls = kinds.iter().skip(cubes).take_while(|k| **k == Kind::Sphere).count();
            assert!(kinds.iter().skip(cubes + balls).all(|k| *k == Kind::Trunk), "cubes, then spheres, then trunks");
            if g == GRID {
                assert_eq!((cubes, balls), (ROWS[0], ROWS[1]));
            } else {
                assert_eq!((cubes, balls), (18, 15));
            }
            assert!(pos(g, 0)[2] < pos(g, rows[0])[2] && pos(g, rows[0])[2] < pos(g, rows[0] + rows[1])[2], "rows back to front");
        }
        // The two sets show different rows.
        for &(id, ..) in &B2L {
            assert!(!B2O.iter().any(|r| r.0 == id), "{id} is in one set only");
        }
    }
}
