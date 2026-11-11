//! The material recipes proof (feature `recipes-proof`; it turns on
//! `lit-proof` and evani-render's `material-recipes`): a row of five model
//! shapes (a curtain and four balls) whose glTF materials carry catalogue
//! blocks in `extras.evani_recipe`, as `cargo xtask convert --materials`
//! will write them, with the plain PBR factors beside them. The renderer
//! reads each block into its binding 12 and draws the patched lit stage.
//!
//! Batch 1 P2b compiles no recipe yet (P4 onwards add them), so each
//! material draws through the recipe stage's identity: its plain colour,
//! roughness and metalness. The scene proves the wiring (load, bind, patch,
//! params, no errors); the pictures change as recipe packages land.
//!
//! P3 turns on all eight condition layers. The row's materials carry a few
//! of their own (a material variant: the catalogue block with other default
//! amounts): wet and mud on the leather, dust on the walnut, wear and oxide
//! on the brass, snow on the marble, the ruby clean. In front, a second row
//! of nine plain test balls: one layer each (wet, mud, dust, snow, wear,
//! oxide, growth, blood), then all eight at once. `?q=low` or `?q=high` in
//! the page address sets `material.quality.conditions` for captures.
//!
//! P4 compiles the first recipes (metal, wood, stone, ceramic), so the walnut,
//! the brass button and the marble now draw their recipes, with the numbers
//! their catalogue rows carry. `?set=p4` in the page address swaps the scene
//! for [`P4`]: every catalogue row P4 gave numbers to, one row of balls per
//! recipe (metal, wood, stone, ceramic). `?q=` then sets
//! `material.quality.detail` too, so a capture shows each recipe's lite, low
//! or high path.
//!
//! P5 adds bark, masonry, plaster and ground. `?set=p5` shows [`P5`]: the
//! 18 catalogue rows P5 gave numbers to (bark, masonry, plaster, ground), on
//! balls (bark on trunks); `?set=p5&shape=cube` puts them all on cubes (flat
//! faces and edges, for seams); `?row=<n>` is a close camera on one grid row,
//! drawn alone.
//!
//! P6 adds leather, fur, hide and keratin. `?set=p6` shows [`P6`]: the 30
//! catalogue rows P6 gave numbers to (12 fur coats and 4 pattern coats, leather,
//! hide, keratin), on balls with a real uv map (a latitude-longitude sphere whose
//! seam faces away from the camera) and horns (a cone, for keratin);
//! `?set=p6&shape=cube` puts them all on cubes. `?q=` then sets the detail,
//! fur and skin groups too. The `?set=p4` tile ball is a cube, for its uv.
//!
//! P7 adds feather and scales. `?set=p7` shows [`P7`]: the 33 catalogue rows P7
//! gave numbers to (fish, snakes, lizards, crocodiles and 13 kinds of plumage),
//! on uv balls; snakes and snakeskin leather ride on a trunk (a body, uv round
//! and along it); `?set=p7&shape=cube` puts them all on cubes.
//!
//! P8 adds gem and ice. `?set=p8` shows [`P8`]: the 21 catalogue rows P8 gave
//! numbers to (ten gems and crystals, then snow and ice), on uv balls; the pointed
//! crystals and the icicle on a cone, the ice cube on a cube (`&shape=cube` puts
//! them all on cubes).
//!
//! P9 adds paint's extras, plastic, organic, fire and glow. `?set=p9` shows [`P9`]:
//! the 51 catalogue rows P9 gave numbers to, in six rows: fire, glow, two rows of
//! paint, organic and plastic, from a camera further back. Fire cards, smoke,
//! steam and the screen ride on a flat card, the tyre and sole on a trunk, the
//! carbon on cubes, the rest on uv balls (`&shape=cube` puts them all on cubes).
//!
//! P10a: `?set=p10a` (built with `recipes-family-proof`) shows the row-to-glTF
//! adapter on real models: four character heads whose materials only name
//! skin, eye and hair rows, the eight cloth rows on the curtain, and eight paint
//! rows with paint's clear coat drawn from the block (one with the coat off);
//! see `proof_p10a.rs`.
//!
//! P10b: `?set=p10b` (built with `recipes-family-proof`, so the layers family
//! draws the velvet and the steel's streak) draws a *converted model*: one
//! file (`tests/fixtures/models/p10b.<tier>.glb`) made by `cargo xtask convert
//! p10b-source.glb --materials p10b.map.json`, whose eight spheres carry
//! oak, brushed steel, red lacquer, granite, a bay horse coat, saddle leather,
//! a ruby and red velvet as `extras.evani_material` / `evani_recipe` written by
//! the converter, not by the proof. `&file=low` draws the low-tier file.
//!
//! P12: `?set=p12` (the same feature) shows the 34 drawn optional rows through the
//! adapter, grouped vehicles (two rows), gems and stones, magic, sci-fi;
//! see `proof_p12.rs`.
//! Never in evani.toml, so the default and ship builds are unchanged.
//!
//! Batch 3 B3-P1: `?set=b3-patterns` shows [`B3`], the rows the shared pattern
//! layouts unlock, on cubes (flat faces show a layout best): plain running
//! bond (the bonds moved into the layouts, unchanged), herringbone and basket
//! brick and the chequered marble floor; the plank deck, two floors and a
//! crate with nail heads; riveted iron, a riveted painted girder and two
//! painted tiles. `?row=`, `?q=` and `?yaw=` as for the other grids.
//! Batch 2, B2-P1: `?set=b2-foliage` shows the foliage recipe's 19 rows (all
//! seven variants) on leaf, needle and grass cards and ground tiles; see
//! `proof_b2_foliage.rs` (`?sun=back`, `?wet=`, `?snow=`).

use evani_core::camera::CameraPose;
use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::overlay::ErrorLog;
use evani_core::plugin::LookData;
use evani_material_recipes::{Block, Fields};
use evani_render::model::{ModelId, ModelInstance, ModelState, Models};
use evani_render::{MeshId, MeshInstance, RenderWorld};
use serde_json::{Value, json};

#[cfg(feature = "recipes-family-proof")]
#[path = "proof_p12.rs"]
mod proof_p12;
#[cfg(feature = "recipes-family-proof")]
#[path = "proof_p11a.rs"]
mod proof_p11a;
#[cfg(feature = "recipes-family-proof")]
#[path = "proof_b5_fur.rs"]
mod proof_b5_fur;
#[cfg(feature = "recipes-nature-proof")]
#[path = "proof_p11b.rs"]
mod proof_p11b;
#[path = "proof_b2_foliage.rs"]
pub mod b2_foliage;
#[path = "proof_b3_glass.rs"]
pub mod b3_glass;

const CURTAIN: &[u8] = include_bytes!("../assets/models/layer-velvet.glb");
const BALL: &[u8] = include_bytes!("../assets/models/layer-coat.glb");
const BALL2: &[u8] = include_bytes!("../assets/models/layer-bubble.glb");
/// The P10b converted model, high and low tier files (`xtask convert --materials`).
const P10B_HIGH: &[u8] = include_bytes!("../../../tests/fixtures/models/p10b.high.glb");
const P10B_LOW: &[u8] = include_bytes!("../../../tests/fixtures/models/p10b.low.glb");

/// One shape of the row: catalogue id, its block (64 hex characters, what
/// the catalogue encodes for the row; a test holds them equal), the fallback
/// roughness and metalness, which shape it rides on, and the condition
/// amounts this proof's variant gives it (block bytes 24-31, u8 unorm, in
/// wet mud dust snow wear oxide growth blood order).
pub struct Item {
    pub id: &'static str,
    pub hex: &'static str,
    pub rough: f32,
    pub metal: f32,
    pub shape: &'static [u8],
    pub cond: [u8; 8],
}

/// The row, left to right.
pub const ITEMS: [Item; 5] = [
    Item {
        id: "leather.saddle_tan",
        hex: "0b00019fa8743e6a4220000000734c664d001a000000002f0000000000000000",
        rough: 0.55,
        metal: 0.0,
        shape: CURTAIN,
        cond: [150, 120, 0, 0, 0, 0, 0, 0],
    },
    Item {
        id: "wood.walnut.stock",
        hex: "040000977a52343e26162a180e592cb3990d28000d0000890000000000000000",
        rough: 0.35,
        metal: 0.0,
        shape: BALL,
        cond: [0, 0, 190, 0, 0, 0, 0, 0],
    },
    Item {
        id: "fit.button.brass",
        hex: "01000737cca452f0dc944a6a5040ff007f1a80b3630000450000000000000000",
        rough: 0.25,
        metal: 1.0,
        shape: BALL2,
        cond: [0, 0, 0, 0, 200, 150, 0, 0],
    },
    Item {
        id: "stonework.marble_floor",
        hex: "06010cddeceae4d2cec62c2c3026730a4c000066001bff170000000000000000",
        rough: 0.15,
        metal: 0.0,
        shape: BALL,
        cond: [0, 0, 0, 215, 0, 0, 0, 0],
    },
    Item {
        id: "gem.ruby",
        hex: "1900015cc0102ae07080ff20404de63e14e61a00000000c60000000000000000",
        rough: 0.1,
        metal: 0.0,
        shape: BALL2,
        cond: [0; 8],
    },
];

impl Item {
    /// The catalogue block with this proof's condition amounts (a variant:
    /// same material, other default amounts), or `None` for bad hex.
    pub fn block(&self) -> Option<Block> {
        let mut b = Block::from_hex(self.hex).ok()?;
        b.0[24..32].copy_from_slice(&self.cond);
        Some(b)
    }
}

/// The plain test material of the front row: recipe 3 (plastic, not
/// compiled, so it draws its plain colour), a mid blue-grey (unlike dust,
/// snow or mud), rough 0.5. Ball `k` (0..8) lists layer `k` alone at amount
/// 0.8 (mud 0.45, so its rising edge sits on the small ball); ball 8 lists
/// all eight at 0.6. Not a catalogue row: a fixture for the layers.
pub fn test_block(k: usize) -> Block {
    let (mask, amount) = match k {
        1 => (2u8, 115u8),
        0..8 => (1u8 << k, 204),
        _ => (0xff, 153),
    };
    let mut cond_amounts = [0u8; 8];
    for (i, a) in cond_amounts.iter_mut().enumerate() {
        if (mask >> i) & 1 == 1 {
            *a = amount;
        }
    }
    Block::pack(&Fields {
        recipe: 3,
        variant: 0,
        extras: 0,
        cond_mask: mask,
        colours: [[120, 124, 132], [0; 3], [0; 3]],
        numbers: [0; 10],
        seed: 17 + k as u8,
        cond_amounts,
    })
}

/// How many test balls the front row has.
pub const TESTS: usize = 9;
/// Where the front row stands (z).
pub const TEST_Z: f32 = Z + 1.7;
/// The test balls' scale.
pub const TEST_SCALE: f32 = 0.5;

/// The x of test ball `k`.
pub fn test_x(k: usize) -> f32 {
    -2.4 + 0.6 * k as f32
}

/// The `?set=p4` grid: (catalogue id, its block as the catalogue encodes it,
/// fallback roughness, fallback metalness), one row per recipe in [`P4_ROWS`]
/// order. A test holds every block equal to the catalogue.
pub const P4: [(&str, &str, f32, f32); 23] = [
    ("metal.steel.blued", "010005bf1e26386a7a985a3a2847ff004c2680002400000f0000000000000000", 0.28, 1.0),
    ("metal.steel.brushed", "010105bf9ea2a8d2d5da6a4a3052ffcc65994d00240000be0000000000000000", 0.32, 1.0),
    ("metal.steel.forged", "010205bf34343a74747a5a32208cff0025809900240000300000000000000000", 0.55, 1.0),
    ("metal.iron.cast", "010305bf46464a76767a6a3a20b3e60098cc3300240000510000000000000000", 0.7, 0.9),
    ("roof.tin_corrugated", "0103015daab0b4d2d6d87a4a2a73e6001f803300240000610000000000000000", 0.45, 0.9),
    ("metal.brass.polished", "010005bfdcb860f4e0a04a6a501fff007f144d00240000530000000000000000", 0.12, 1.0),
    ("metal.copper.polished", "010005bfe6946cf8c4a43f8a701fff007f144d00240000bb0000000000000000", 0.12, 1.0),
    ("metal.silver.polished", "010005bfdcdce0f6f6f83a343014ff007f0f4d00240000170000000000000000", 0.08, 1.0),
    ("metal.gold.polished", "010005bff4c45afce6a48a6a301aff007f0f4d00240000020000000000000000", 0.1, 1.0),
    ("fit.button.brass", "01000737cca452f0dc944a6a5040ff007f1a80b3630000450000000000000000", 0.25, 1.0),
    ("wood.walnut.stock", "040000977a52343e26162a180e592cb3990d28000d0000890000000000000000", 0.35, 0.0),
    ("timber.oak", "040101d5c89a5e9a6e3e5a3a20994b3373082800cc0000790000000000000000", 0.6, 0.0),
    ("timber.pine", "040001d5e2c48ab8864a6a3e1ea61759999928000000004a0000000000000000", 0.65, 0.0),
    ("wood.plank.deck", "040307dfb084587a52324a2e1ab3214d802618cc0d0000c10000000000000000", 0.7, 0.0),
    ("wood.plank.weathered", "040303dfaaa498706c644a4640d91c66b33323b3000000150000000000000000", 0.85, 0.0),
    ("rock.granite.grey", "060003dd9c9c9a5c5c5ee8e6e099001a980099000000005d0000000000000000", 0.6, 0.0),
    ("rock.basalt", "060003dd4242442e2e305a5a5cbf001aff001a00000000e70000000000000000", 0.75, 0.0),
    ("rock.sandstone.red", "060201ddb0583ad8a078e0c8a8e60044bfd90000000000600000000000000000", 0.9, 0.0),
    ("stonework.marble_floor", "06010cddeceae4d2cec62c2c3026730a4c000066001bff170000000000000000", 0.15, 0.0),
    ("tile.glazed.white", "170502d59a9894f2f2ee3a3a3ae699001c054000000000330000000000000000", 0.09, 0.0),
    ("pot.terracotta", "17030095c26c42c26c425a30201a00001c260000000000410000000000000000", 0.45, 0.0),
    ("pot.porcelain.white", "17020295f4f2eefbfbf88a8a90f280001c009900000000980000000000000000", 0.06, 0.0),
    ("pot.enamel.white", "170402951c1c1ef4f4f01c1c1ed9b373170d1a00000000490000000000000000", 0.11, 0.0),
];

/// The `?set=p5` grid, as [`P4`]: bark, masonry, plaster, ground (recipe code order).
pub const P5: [(&str, &str, f32, f32); 18] = [
    ("bark.oak", "050001cd5a52481e1a168a8a70e6cc1f2fb60000000000600000000000000000", 0.9, 0.0),
    ("bark.pine", "050101cd8a4e302a1a12a86a48e6b3241a491a00000000a10000000000000000", 0.9, 0.0),
    ("bark.birch", "050200cde8e4da1a18149a968a99331a2f497300000000830000000000000000", 0.6, 0.0),
    ("brick.red.common", "070003dda85a42b8b2a48a4a382f0c2a80d99999000000d60000000000000000", 0.85, 0.0),
    ("stonework.cobble", "070203dd7a78724a46408a867c181d604dcc80b3000000f20000000000000000", 0.8, 0.0),
    ("stonework.ashlar", "070403ddb4b0a68a867ca09a8e7942151abf4d4d000000c40000000000000000", 0.75, 0.0),
    ("stonework.setts", "070303dd7a7a7e5a5a5c6a6a7010152a4dcc8099000000140000000000000000", 0.8, 0.0),
    ("stonework.flagstone", "070503dd9a94885a564e8a8478363a3566cc8080000000520000000000000000", 0.8, 0.0),
    ("plaster.white", "080002dde6e2d88a8478000000e6591a00101a00000000eb0000000000000000", 0.9, 0.0),
    ("concrete.smooth", "080203dda8a8a45a5a58000000cc3310260a8000000000ee0000000000000000", 0.8, 0.0),
    ("plaster.stucco", "080103ddd8cdb88a7e6a000000f2802f1a104d00000000270000000000000000", 0.95, 0.0),
    ("plaster.lime_wash", "080301ddece8dca09a88000000f2cc240d0a00000000008f0000000000000000", 0.95, 0.0),
    ("ground.soil.loam", "090001c93a2a1c6a665cc8b48af20d1f002400b3000000600000000000000000", 0.95, 0.0),
    ("ground.soil.packed", "090001c98a6e4c847f76c8b48af2084000240026000000770000000000000000", 0.95, 0.0),
    ("ground.mud.swamp", "090300c92a22186a665c8a7a5a400808002400cc000000240000000000000000", 0.25, 0.0),
    ("ground.sand.beach", "090102896a5a40b8a888e4cc94f2030db32f80000000001f0000000000000000", 0.95, 0.0),
    ("ground.gravel.road", "0905014d5a50469a968eb4a890e60de600240000000000b90000000000000000", 0.9, 0.0),
    ("asphalt.road", "090402dd2a2a2c6a6a6c5a5a5ed90280002433000000008c0000000000000000", 0.85, 0.0),
];

/// The `?set=b3-patterns` grid (B3-P1), as [`P4`]: masonry and stone, wood,
/// then metal, paint and ceramic; rows 4 to 10 (B3 packages after the
/// layouts): tiles and roofs, painted and glazed brick, boards and logs,
/// floors and panels, pattern metal, and plaster; row 12 the rows that needed
/// only numbers or a small variant (terrazzo, tar paper and linoleum sheets,
/// a log in its bark, two mirrors); row 13 the thin and cloth recipe stages
/// (three wallpapers, two carpets, a woven rug and batch 1's denim twill).
/// Every row is drawn on a cube.
pub const B3: [(&str, &str, f32, f32); 65] = [
    ("brick.red.common", "070003dda85a42b8b2a48a4a382f0c2a80d99999000000d60000000000000000", 0.85, 0.0),
    ("brick.herringbone", "070603dda85a42b8b2a48a4a382812154dd98099000000670000000000000000", 0.85, 0.0),
    ("brick.basket", "070703dd9a5a40a8a2947a4a342812154dd98099000000360000000000000000", 0.85, 0.0),
    ("stonework.marble_floor", "06010cddeceae4d2cec62c2c3026730a4c000066001bff170000000000000000", 0.15, 0.0),
    ("wood.plank.deck", "040307dfb084587a52324a2e1ab3214d802618cc0d0000c10000000000000000", 0.7, 0.0),
    ("wood.floor.oak", "040306dfd8a868a870385a3a204d2c59730d13594d00007f0000000000000000", 0.3, 0.0),
    ("wood.floor.pine", "040307dfe0c088b088485a38188c1c6699731e66000000c80000000000000000", 0.55, 0.0),
    ("wood.crate", "040307dfd8b880a080506a4a28d91780804d36e6000000090000000000000000", 0.85, 0.0),
    ("build.iron.rivet_plate", "01000d3d5a5a5e9a9aa07a44268ce600254d4d00240d00bc0000000000000000", 0.6, 0.9),
    ("build.steel.girder", "0200103d8a3a243a3a3ad8d8e0991a66002a00803d1b00250000000000000000", 0.6, 0.0),
    ("tile.blue_painted", "170507d5ead8c0f4f2ec2a4a9ad980264b001a000000008b0000000000000000", 0.1, 0.0),
    ("tile.mexican", "170506d5d8a030f2eee21e4aa8cc80001c0026000000008e0000000000000000", 0.12, 0.0),
    ("tile.subway", "170602d5a8a8a2f4f4f03a3a3ae680001c0026006d0000f40000000000000000", 0.13, 0.0),
    ("tile.hex", "170702d59a9a96f4f4f03a3a3ad966001c001a24000000aa0000000000000000", 0.17, 0.0),
    ("tile.encaustic", "170504d54a4a52e0d4bc9a3a281f1a001c0a1a000000006a0000000000000000", 0.75, 0.0),
    ("stonework.mosaic", "170802dd8e8a822a6a9ae0b050bf66001c001a00000000930000000000000000", 0.25, 0.0),
    ("roof.tile.terracotta", "1709005db8582cc8683a5a2a183333001c33990004e600e00000000000000000", 0.69, 0.0),
    ("roof.tile.flat_clay", "1709005da8503ab860444a24183333001c269900490000140000000000000000", 0.69, 0.0),
    ("roof.tile.slate", "070a035d5c66703a424a6a747e3647151ab38080000000d40000000000000000", 0.7, 0.0),
    ("wood.shingle", "040703dfa894786a5a464a3c2ce62166991a13b3000000a40000000000000000", 0.9, 0.0),
    ("brick.white_painted", "070005dda85a42b8b2a4eceae22c092a59e64d800000007f0000000000000000", 0.9, 0.0),
    ("brick.adobe", "070013ddb98a5ca98456d8b8605e1d6080f26699730000df0000000000000000", 0.95, 0.0),
    ("stonework.rubble", "070803dd8a867ca39d8d6c685e23189580e699cc000000210000000000000000", 0.9, 0.0),
    ("stonework.limestone", "070413dddcd0b4b4ac989a8a6a6b3a1526d94d4d400000af0000000000000000", 0.85, 0.0),
    ("brick.glazed", "07000bdde8dcb8cfc8b4d8c8982c091000cc264d000000c90000000000000000", 0.8, 0.0),
    ("wood.clapboard", "04040bdfc9a8748a6a40e4dcc8cc214d801a1399002e00b40000000000000000", 0.8, 0.0),
    ("wood.board_batten", "040503dfb894647a5a384a3020d91c668c401bb3000000e90000000000000000", 0.85, 0.0),
    ("wood.log.peeled", "040603dfdcae64a874306a5a48a6174d661a28cc1a0000b90000000000000000", 0.65, 0.0),
    ("wood.parquet", "040802dfdcae6ca870386a40204d2759800d03804000006a0000000000000000", 0.3, 0.0),
    ("wood.panelling", "040903df7e4e2e4e2e1a3a20104d21598c0013801a0000440000000000000000", 0.3, 0.0),
    ("wood.bamboo", "040a02dfcdbc647c8c3c5c6a2a594b1a660013cc000000ce0000000000000000", 0.35, 0.0),
    ("wall.cork", "040b00d5cc9e668a5c326a4020e6644dcc00284d000000900000000000000000", 0.9, 0.0),
    ("wood.beam.oak", "040111df7e5e3e4a32202a1a10d9174d991a284d4d00001e0000000000000000", 0.85, 0.0),
    ("wood.plank.painted", "04030bdfb89c707a6246e2dccdcc1c66992623b3008000350000000000000000", 0.8, 0.0),
    ("build.diamond_plate", "0104053d9a9ca2c8cad06a4a3066ff004c4d4d00249400a60000000000000000", 0.4, 1.0),
    ("wall.tin_ceiling", "010505d5e8e4daf6f2eaa8a09080260032334d00242800f40000000000000000", 0.5, 0.15),
    ("build.copper.roof", "0100113db8683adc8c5c3a9a7a59ff00654d6600244300f20000000000000000", 0.35, 1.0),
    ("roof.tin_standing", "0100115d8a3a2cb0604c5a2a208059003f4d4d00245e00c30000000000000000", 0.5, 0.35),
    ("plaster.cracked", "080007ddd8d0c06a60547a5a38e64d15cc1a1a4d000000610000000000000000", 0.9, 0.0),
    ("plaster.pebbledash", "08010addcfc7b68a8478000000f2331a0010d900330000a20000000000000000", 0.95, 0.0),
    ("concrete.board", "080210dd9c9c985a5a58000000e626150d102600000000500000000000000000", 0.9, 0.0),
    ("concrete.polished", "080402dda8a8a65a5a58000000381a1a0d101a00000000b40000000000000000", 0.22, 0.0),
    ("plaster.mud_daub", "080007dd9c7a565e46308c6c3cf2b31073151a80000000b90000000000000000", 0.95, 0.0),
    ("stonework.terrazzo", "060800ddd8d2c49a7a623a3a4033991a25000000000000f80000000000000000", 0.2, 0.0),
    ("roof.tar_paper", "0304025d2a2a2c6a6a68000000e60099cc2d0033300000fb0000000000000000", 0.9, 0.0),
    ("wall.linoleum", "030400d5a87e54d8c4a00000006600004c2d00b3000000a40000000000000000", 0.4, 0.0),
    ("wood.log.bark", "050001df5e4c3a241a128a7a60e6bf2a22920000000000e80000000000000000", 0.9, 0.0),
    ("glass.mirror", "0100001de6e8ecf6f6f81a1a1c05ff007f0000002400009c0000000000000000", 0.02, 1.0),
    ("glass.mirror_old", "0100201dc8c4b4e0dcd02a26200fff004c000000240080ea0000000000000000", 0.06, 1.0),
    ("wall.paper.stripe", "150501d5e8dcc06a8a6ac8c0b04dcc2900260000000000580000000000000000", 0.8, 0.0),
    ("wall.paper.floral", "150502d5eadcc4a84a4ac8c0b04dbf1900800000000000370000000000000000", 0.8, 0.0),
    ("wall.paper.peeling", "150505d5d8c8a06a7a9ab8b0a04d9921996600000000007d0000000000000000", 0.8, 0.0),
    ("wall.carpet.wool", "0a0307d59a7a5ac8b8a08a3a2a664db380f200000000000d0000000000000000", 0.95, 0.0),
    ("wall.carpet.patterned", "0a030fd56a1a20c89aa0d8b060664db380f210d9000000ab0000000000000000", 0.95, 0.0),
    ("wall.rug.woven", "0a050fd5a8402ad8c0a0e8dcc07f1acc4df208e6000000680000000000000000", 0.95, 0.0),
    ("cloth.denim.indigo", "0a01079f2a3e668a9ab88a3a2a4c339933d90000000000ac0000000000000000", 0.85, 0.0),
    ("build.grate", "0106013d7c7e80a8aaac7a44268cd9004c594d0024c900f20000000000000000", 0.55, 0.85),
    ("build.chain_link", "0107013d9aa0a4c8ccd07a503073e6006540660024ff00dd0000000000000000", 0.45, 0.9),
    ("build.mesh", "0108013d8c9296b8bcc06a4a2c80d90065334d0024ff00aa0000000000000000", 0.5, 0.85),
    ("wood.lattice", "040c01dfc8a0708a60385a3820bf4b4d730d00330000c66c0000000000000000", 0.75, 0.0),
    ("cloth.lace.white", "15070097f2ede4fff6e6c8c0b00dcc5a00000000000000630000000000000000", 0.8, 0.0),
    ("wall.paper.damask", "150502d5d8cdb4b8a47ec8c0b01acc0c001a0000000000c20000000000000000", 0.8, 0.0),
    ("paper.newsprint", "15060095cfc8b42a28268a847814d92100000000000000d10000000000000000", 0.85, 0.0),
    ("roof.sod", "1404055d4a6a2a7a8a3a4a3a248059000d403e40000000db0000000000000000", 0.9, 0.0),
    ("asphalt.cracked", "090406dd3030326a6a6c1a1a1cd9027300242600080000910000000000000000", 0.85, 0.0),
];

/// The rows of the [`B3`] grid.
pub const B3_ROWS: [usize; 15] = [4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 3, 6, 7, 4, 5];

/// The P6 rows drawn on a horn (a cone with rings along it) rather than a ball.
const HORNS: [&str; 5] = ["horn.cattle", "antler.bone", "grip.stag", "teeth.healthy", "bone.fresh"];

/// The `?set=p6` grid, as [`P4`]: fur coats (two rows), then leather and hide,
/// then keratin.
pub const P6: [(&str, &str, f32, f32); 30] = [
    ("horse.coat.bay", "0c00028f521f107c38201a14101ae6cc1acb004000d900dd0000000000000000", 0.8, 0.0),
    ("horse.coat.grey_dapple", "0c05028f5a5c608a8c90c4c6c81ae68080448c400d4d005d0000000000000000", 0.8, 0.0),
    ("horse.coat.palomino", "0c00028fb08030e0b868efe2c41ae699992499400d8000640000000000000000", 0.8, 0.0),
    ("cattle.coat.black", "0c00028f14121026221e3a343014ff594d243340050000b20000000000000000", 0.8, 0.0),
    ("dog.coat.tan", "0c00028f9a6a3ac89a60e8d4b014e666802480400d4d00870000000000000000", 0.8, 0.0),
    ("cat.coat.tabby_brown", "0c01028f6a4a2a9a72401c120a1fe666802fd9001a0000870000000000000000", 0.8, 0.0),
    ("sheep.wool.white", "0c09028fbcae92ece4ccfff8e8ccff1ab32f6640e600402c0000000000000000", 0.8, 0.0),
    ("trim.fur.sheepskin", "0c00028fc0b296efe6ccfff8eab3ff1ab32f0040d90000da0000000000000000", 0.8, 0.0),
    ("bear.coat.brown", "0c00028f2a18105a3a248a6a4ae6ff4d661a0040800000aa0000000000000000", 0.8, 0.0),
    ("wolf.coat.grey", "0c00028f3a38348a867cb49a7480ff4d662480404d99006f0000000000000000", 0.8, 0.0),
    ("fox.coat.red", "0c00028fb0480ee8842c14101080ff809924004033e6008a0000000000000000", 0.8, 0.0),
    ("tiger.coat", "0c01028fc8661ce88c38120c081aff66802ae6400d0000a10000000000000000", 0.8, 0.0),
    ("leopard.coat", "0c04028fc89a48e8c0701a120a1aff738044d9400d00007b0000000000000000", 0.8, 0.0),
    ("cheetah.coat", "0c02028fd0a860ecd08c1a140c14ff668058e6400d0000d20000000000000000", 0.8, 0.0),
    ("giraffe.coat", "0c0a028fe8d4a0f0e0b870441a0dff4d801ae640050080f00000000000000000", 0.8, 0.0),
    ("leather.saddle_tan", "0b00019fa8743e6a4220000000734c664d001a000000002f0000000000000000", 0.55, 0.0),
    ("leather.suede", "0b01039f8a6a4a5a4430000000f2724d26cc1a00000000080000000000000000", 0.55, 0.0),
    ("leather.rawhide", "0b02019fc8b0788a7448000000b33f4d800000000000002e0000000000000000", 0.55, 0.0),
    ("leather.tooled", "0b03019f9a6a3a4a2a140000008c4c4d330033e6000000330000000000000000", 0.55, 0.0),
    ("frog.green", "0f0303873e7a261a3a12b4a85a3339d94d4dcc000000008a0000000000000000", 0.4, 0.0),
    ("elephant.hide", "0f0101877a76704a4640a07a6ae62f000dd900000000009e0000000000000000", 0.4, 0.0),
    ("toad.warty", "0f0201876a5a3a3a2e1c9a7a5ab3441a1ab38000000000a60000000000000000", 0.4, 0.0),
    ("horn.cattle", "13000187e8dcc02a221800000040662c4d000000000000890000000000000000", 0.35, 0.0),
    ("antler.bone", "13020087b09a746a52380000000d80218c000000000000bf0000000000000000", 0.35, 0.0),
    ("grip.stag", "13020097a892704a382200000000991780000000000000be0000000000000000", 0.35, 0.0),
    ("teeth.healthy", "13030182e6dcc4b8a888000000801f64400000000000002a0000000000000000", 0.35, 0.0),
    ("bone.fresh", "13010087e4d6c0c8aa900000000d334b99000000000000c50000000000000000", 0.35, 0.0),
    ("beetle.black", "130502050c0c102a2a340000000000311f4d00000000007b0000000000000000", 0.35, 0.0),
    ("beetle.green", "130502053a6a20a89030000000000031268000000000000b0000000000000000", 0.35, 0.0),
    ("clam.shell", "13040381e8dcc8a08a6a00000026b32759330000000000f00000000000000000", 0.35, 0.0),
];

/// The `?set=p9` grid, as [`P4`]: fire (flames, embers, lava, smoke, steam),
/// glow (lamps, neon, screen, rune, hologram, energy), paint (solid, matte,
/// metallic, pearl, colour-shift, carbon), organic (bread, meat, cheese, fruit,
/// mushrooms) and plastic (gloss, rubber, foam, tread).
pub const P9: [(&str, &str, f32, f32); 51] = [
    ("fire.flame.wood", "1c000100fff0a0ff6a102a2420666666804d4d00000000130000000000000000", 0.5, 0.0),
    ("fire.flame.candle", "1c000100fff4c0ffa0201a14104d3326991f0000000000df0000000000000000", 0.5, 0.0),
    ("fire.flame.gas", "1c000100c0d8ff3060ff10101860801a59800000000000d30000000000000000", 0.5, 0.0),
    ("fire.flame.torch", "1c000100ffe090ff50083a24188059808c5c8000000000fc0000000000000000", 0.5, 0.0),
    ("fire.embers", "1c010100ffd060ff4a081a1412734080661a8000000000e50000000000000000", 0.5, 0.0),
    ("fire.lava", "1c020100ffe080ff5a081c1010801a4d801400000000004b0000000000000000", 0.5, 0.0),
    ("fire.smoke", "1c030100c8c8c8ff8a40403c3826264d99330000000000d60000000000000000", 0.5, 0.0),
    ("fire.steam", "1c040100f0f4f8a0b0c0c0c8d01a2633994d0000000000630000000000000000", 0.5, 0.0),
    ("glow.lamp.warm", "1d000104ffb060ffffff000000400d0d001a0000000000170000000000000000", 0.5, 0.0),
    ("glow.bulb.white", "1d000004fff4d0ffffff000000801a00001a0000000000720000000000000000", 0.5, 0.0),
    ("glow.neon.red", "1d010104ff1020ff808000000060990f001a0000000000030000000000000000", 0.5, 0.0),
    ("glow.neon.blue", "1d0101042080ffa0e0ff00000060990f001a0000000000d80000000000000000", 0.5, 0.0),
    ("glow.screen", "1d020204b8d8ffffffff000000301a004d1a00000000009a0000000000000000", 0.5, 0.0),
    ("magic.rune", "1d0301044090ffffffff000000501f80001a00000000002e0000000000000000", 0.5, 0.0),
    ("sf.holo", "1d0403b440c0ffc0f0ff000000503333392659cc000000720000000000000000", 0.5, 0.0),
    ("sf.energy_core", "1d0501b440ffffffffff00000060264d001a0000000000380000000000000000", 0.5, 0.0),
    ("finish.lacquer.black", "020001d50a0a0c202024ffffff0fff08002a00803d0000480000000000000000", 0.3, 0.0),
    ("finish.lacquer.red", "020001d59a0c14300408ffffff14ff08002a00803d0000280000000000000000", 0.3, 0.0),
    ("finish.painted.cream", "020000d5e8dcb8c8b890ffffff8c004d002a00803d0000850000000000000000", 0.5, 0.0),
    ("finish.milk_paint", "020400d55a7ea03a5a78ffffffe60080002a00803d0000cc0000000000000000", 0.5, 0.0),
    ("car.paint.solid_red", "0200013fb01820300408ffffff4de60d002a00803d0000960000000000000000", 0.3, 0.0),
    ("car.paint.metallic_silver", "0201033fa8aab03a3a3ee8e8f04de60d992a00803d0000ea0000000000000000", 0.3, 0.0),
    ("car.paint.metallic_blue", "0201033f102a6a0a1020a0c0ff4de60d802a00803d00007f0000000000000000", 0.3, 0.0),
    ("car.paint.pearl_white", "0202073ff0eee8d8d4ccffffff4de60d1a2a80803d00005b0000000000000000", 0.3, 0.0),
    ("car.paint.colour_shift", "0203073f5a209020a060e0d0ff4de60d332ab34d3d0000e40000000000000000", 0.3, 0.0),
    ("car.paint.matte_black", "0204003f101012202024ffffffd90080002a00803d0000ec0000000000000000", 0.5, 0.0),
    ("car.paint.primer", "0204003f6a6c70505256ffffffcc0080002a00803d0000d00000000000000000", 0.5, 0.0),
    ("finish.soap_film", "020305d5f0f0ff303040ffffff0dff05002aff993d0000660000000000000000", 0.2, 0.0),
    ("car.carbon", "0205091f0a0a0c2a2a30ffffff33ff08002a00805d0000dc0000000000000000", 0.3, 0.0),
    ("armour.carbon", "020509b70a0a0c2a2a30ffffff40ff0d002a00803d0000440000000000000000", 0.3, 0.0),
    ("wall.paint.gloss", "020001d5d8d4c8b0aca0ffffff26ff1a002a00803d0000730000000000000000", 0.3, 0.0),
    ("food.bread.crust", "16010085b8782c7a481800000000004d4bd96600000000a40000000000000000", 0.6, 0.0),
    ("food.bread.crumb", "16010185f0e0b8b898600000000066d964e60000000000600000000000000000", 0.6, 0.0),
    ("food.meat.raw", "16000185b02a30f0dcd00000008c8000316699000000003a0000000000000000", 0.4, 0.0),
    ("food.meat.cooked", "160001856a3a20a06a400000001a1a337eb34d00000000a90000000000000000", 0.5, 0.0),
    ("food.cheese.holes", "16010185e8c050c098280000001a334d12800000000000d10000000000000000", 0.5, 0.0),
    ("food.potato", "160200859a7a505a42280000000000334bcc8000000000320000000000000000", 0.6, 0.0),
    ("food.carrot", "16020185e8781ab04a080000001a1a1a64804d00000000520000000000000000", 0.5, 0.0),
    ("fungus.cap_red", "1604010dc01818f4f0e00000004d1a800d8000000000002d0000000000000000", 0.5, 0.0),
    ("fungus.cap_brown", "1604010d6a4428a88a68000000261a000d8c3300000000920000000000000000", 0.5, 0.0),
    ("food.beans", "160301856a3418a0602c000000cc333331330000000000dc0000000000000000", 0.3, 0.0),
    ("plastic.glossy.red", "03000195c81e1ee0e0e00000001f33004c2d0000000000a30000000000000000", 0.15, 0.0),
    ("plastic.matte.grey", "030002957a7c80b8b8b8000000a6004d722d00000000003d0000000000000000", 0.6, 0.0),
    ("plastic.rubber.black", "03010095141414303030000000cc00004c2d0000000000800000000000000000", 0.8, 0.0),
    ("plastic.rubber.red", "03010095a82424d05050000000bf00004c2d0000000000c00000000000000000", 0.7, 0.0),
    ("plastic.rubber_duck", "03000195f0c818ffffff0000001a66004c2d0000000000970000000000000000", 0.15, 0.0),
    ("plastic.bakelite", "030002953a22147a5030000000400099322d0000000000560000000000000000", 0.25, 0.0),
    ("plastic.toy_brick", "030001951a4fd0ffffff0000001426004c2d00000000005b0000000000000000", 0.1, 0.0),
    ("plastic.foam", "03020095e8e0c8b8ac90000000e60000652d0000000000960000000000000000", 0.9, 0.0),
    ("fit.rubber_sole", "03010414181818383838000000d900004c24cc00000000bc0000000000000000", 0.8, 0.0),
    ("car.tyre.tread", "0303041f1212122a2a2a000000e600004c2dff00000000710000000000000000", 0.9, 0.0),
];

/// How many [`P9`] shapes each grid row holds, back to front: fire, glow, paint
/// (two rows), organic, plastic.
pub const P9_ROWS: [usize; 6] = [8, 8, 8, 7, 10, 10];

/// The [`P9`] rows that ride on a flat card (flames, smoke, steam, the screen),
/// a trunk (tread) or a cube (carbon twill).
const CARDS: [&str; 7] = ["fire.flame.wood", "fire.flame.candle", "fire.flame.gas", "fire.flame.torch", "fire.smoke", "fire.steam", "glow.screen"];
const TREADS: [&str; 2] = ["fit.rubber_sole", "car.tyre.tread"];
const WEAVES: [&str; 2] = ["car.carbon", "armour.carbon"];

/// The `?set=p8` grid, as [`P4`]: ten gems and crystals (cut stones, then raw
/// crystals, amber, jade and salt), then snow and ice (snow, lake and glacier ice,
/// then the cube, icicle, frost, sea ice and snowball).
pub const P8: [(&str, &str, f32, f32); 21] = [
    ("gem.ruby", "1900015cc0102ae07080ff20404de63e14e61a00000000c60000000000000000", 0.1, 0.0),
    ("jewel.garnet", "190001148a1020c06070d0204040cc460dd90d00000000a70000000000000000", 0.1, 0.0),
    ("gem.sapphire", "1900015c1a3ab08090e03060ff40cc3e0de61400000000c80000000000000000", 0.1, 0.0),
    ("gem.emerald", "1901015c10803a60c08020d06026991a73bf0d00000000660000000000000000", 0.1, 0.0),
    ("gem.diamond", "1900015cf4f6ffffffffe0f0ffffff4f00ff0000000000c60000000000000000", 0.05, 0.0),
    ("gem.amethyst", "1903015c8a40c0c8a0e8a060e033992333bf1a00000000e90000000000000000", 0.1, 0.0),
    ("gem.quartz_clear", "1903015ce8f0f4ffffffe0e8f066b31a1ae60000000000570000000000000000", 0.1, 0.0),
    ("gem.amber", "1904025ce0901cf0c060ffb030004d0080b31a00000000690000000000000000", 0.2, 0.0),
    ("gem.jade", "1902005c60a07090c8a040a060001a004d5900000000009c0000000000000000", 0.3, 0.0),
    ("rock.salt", "190301ddf0d8d8fffffff0b0b01ab32c4d8000000000000a0000000000000000", 0.3, 0.0),
    ("ground.snow.fresh", "1b020686f4f8ffc8dcf0ffffff000063e680ff00000000190000000000000000", 0.5, 0.0),
    ("ground.snow.packed", "1b020786e0e4eca8b4c4f0f4f81a33394d4d9900000000db0000000000000000", 0.5, 0.0),
    ("ground.snow.slush", "1b000686a8b0b470808cd0d8dc4d00241a664d00000000fc0000000000000000", 0.3, 0.0),
    ("ground.snow.dirty", "1b020786b0a898786858d8d0c01a1a4e33339900000000b50000000000000000", 0.6, 0.0),
    ("ground.ice.lake", "1b0007863a6a880a2a48d8ecf8e699101a330000000000100000000000000000", 0.1, 0.0),
    ("ground.ice.glacier", "1b0107866ab0e01a60b0d0ecffcc4d154d990d00000000200000000000000000", 0.15, 0.0),
    ("ice.cube", "1b00078cb8dcf05a9ac8e8f6ffbf802f664d1a00000000570000000000000000", 0.1, 0.0),
    ("ice.icicle", "1b03078cc8e4f470a8d0f0faffe60d1a4d4d0000000000ed0000000000000000", 0.1, 0.0),
    ("ice.frost", "1b02068cf0f6ffa0c0e0ffffff33007d9933ff00000000680000000000000000", 0.5, 0.0),
    ("ice.sea_ice", "1b01078ce8f2f860a0d0f4fbff59800a33803300000000210000000000000000", 0.3, 0.0),
    ("ice.snowball", "1b02068cf4f6fac0d0e4ffffff0000978066cc00000000f40000000000000000", 0.6, 0.0),
];

/// How many [`P8`] shapes each grid row holds, back to front: cut gems, crystals
/// and soft stones, snow and ground ice, props.
pub const P8_ROWS: [usize; 4] = [5, 5, 6, 5];

/// The [`P8`] rows that ride on a cone: the pointed crystals and the icicle.
const POINTS: [&str; 3] = ["gem.amethyst", "gem.quartz_clear", "ice.icicle"];

/// The `?set=p7` grid, as [`P4`]: fish and snakes, lizards and crocodiles, then
/// plumage (plain and iridescent, then barred and speckled).
pub const P7: [(&str, &str, f32, f32); 33] = [
    ("trout.skin", "0e000381a8b0a81a1a18e08a90161a66cbcc8040000000ac0000000000000000", 0.4, 0.0),
    ("salmon.skin", "0e000381b8c4c81a2a346ac0d01c1a66cb599940000000a20000000000000000", 0.4, 0.0),
    ("carp.skin", "0e000381c89a304a3410f0d0605733b31a00664d0000005e0000000000000000", 0.4, 0.0),
    ("sardine.skin", "0e000381c0ccd05a6a7480e0d0131a4dcb26e6330000005e0000000000000000", 0.4, 0.0),
    ("koi.skin", "0e000381ece8e0e06a18ffffff361a8039f23340000000e50000000000000000", 0.4, 0.0),
    ("snake.rattle", "0e000187b89a6a2a1c108a7a5016339915e600800000008d0000000000000000", 0.4, 0.0),
    ("snake.python", "0e000387b898684a3018c0a070101a6639d92659000000990000000000000000", 0.4, 0.0),
    ("snake.black", "0e0003870c0c0e2a2a2e6a6aa0101a4d1a004d33000000960000000000000000", 0.4, 0.0),
    ("snake.green", "0e0003873aa0301e6a20a0e060101a4d1a002640000000c60000000000000000", 0.4, 0.0),
    ("leather.snake", "0e00019fa090703a2a18c0b0900c80806db30066000000480000000000000000", 0.5, 0.0),
    ("iguana.skin", "0e0101874a9a3a1e4a2090c8700ccc6663990073000000d10000000000000000", 0.4, 0.0),
    ("chameleon.skin", "0e01038758b030208a8ae0d04009e64d6db38059000000c40000000000000000", 0.4, 0.0),
    ("gecko.skin", "0e030187d8b8403a2a10f0d870063333cbcc00990000007f0000000000000000", 0.4, 0.0),
    ("tortoise.skin", "0e0301877a6c583a3024a090703d4dcc244d00b30000009d0000000000000000", 0.4, 0.0),
    ("alligator.belly", "0e020187d8ccb0a89878ffffff3db3800a260080000000fe0000000000000000", 0.4, 0.0),
    ("sturgeon.skin", "0e0201817a80782a2e2ca0b0b09799cc2f8000800000000b0000000000000000", 0.4, 0.0),
    ("alligator.back", "0e0301872a34200e140a5a6a403d1ae6396600990000006d0000000000000000", 0.4, 0.0),
    ("crocodile.skin", "0e0301876a70502a30208a90703033cc6db3008c000000f60000000000000000", 0.4, 0.0),
    ("beaded_lizard.skin", "0e0301871a1410e07a18f0a0401600cc39e60080000000240000000000000000", 0.4, 0.0),
    ("horned_lizard.skin", "0e030187c0a0704a3420e0c090164d99cbcc0099000000b10000000000000000", 0.4, 0.0),
    ("crow.feather", "0d00038f0e0e122a2a346a40a01e004899596600000000770000000000000000", 0.6, 0.0),
    ("eagle.feather.brown", "0d00038f4a3220b08040d0a05027593b1aa66600000000300000000000000000", 0.6, 0.0),
    ("pigeon.feather", "0d00038f7a7e883a3e4840b0801a3348b3804d00000000ca0000000000000000", 0.6, 0.0),
    ("hummingbird.feather", "0d00038f2a8a4ab0205030e0a009007ce64d4d00000000ca0000000000000000", 0.6, 0.0),
    ("swan.feather", "0d00028ff4f2eed8d4ccffffff271a2e00cc8000000000f10000000000000000", 0.6, 0.0),
    ("peacock.feather", "0d03038f2a6a4a1a1a6a2aa8c849cc6299666600000000e40000000000000000", 0.6, 0.0),
    ("bluejay.feather", "0d01038f3a70d010182860b0ff1acc4e4d804d00000000220000000000000000", 0.6, 0.0),
    ("owl.feather.barred", "0d01028f6a5038e8d8b8c8b0901eb35500d94d00000000e60000000000000000", 0.6, 0.0),
    ("dove.feather", "0d02028fb8a8901a1612d8c8b01e8c3b00b34d00000000980000000000000000", 0.6, 0.0),
    ("quail.feather", "0d00028f7a6a58f0e8d8a0907016b33b00b34d00000000a60000000000000000", 0.6, 0.0),
    ("turkey.feather", "0d01038f6a4a201a120830a07027994180804d00000000020000000000000000", 0.6, 0.0),
    ("woodpecker.feather", "0d01028f141414f0f0e86060601ee65500994d000000001f0000000000000000", 0.6, 0.0),
    ("sparrow.feather", "0d02028f7a5a3a2a1c10a0806016994800bf4d00000000a60000000000000000", 0.6, 0.0),
];

/// The P10b set is one converted model (eight spheres in a row).
pub const P10B_ROWS: [usize; 1] = [1];

/// The `?set=p10b` camera: close on the one row of eight spheres.
pub const P10B_CAMERA: CameraPose = CameraPose {
    eye: [0.0, 1.3, Z + 2.6],
    target: [0.0, 0.2, Z - 1.0],
    up: [0.0, 1.0, 0.0],
    fov_y_deg: 42.0,
};

/// How many P12 shapes each grid row holds, back to front: vehicle paints,
/// vehicle parts, gems and stones, magic, sci-fi (`proof_p12.rs`).
pub const P12_ROWS: [usize; 5] = [9, 8, 10, 4, 3];

/// How many P11a shapes each grid row holds, back to front: six heads, the
/// horse before and after and the planned cloth rows (`proof_p11a.rs`).
pub const P11A_ROWS: [usize; 2] = [6, 5];

/// `?set=p11a`'s grid number. It was 13 on batch 1's line; batch 2 had taken
/// 13 to 16 (b2-options, b2-foliage, b2-land, b2-plants), so the join moved
/// p11a to 17.
pub const P11A_GRID: u8 = 17;
/// The `?set=b3-patterns` grid number (batch 3, B3-P1).
pub const B3_GRID: u8 = 18;

/// `?set=b5-fur`'s grid number: batch 5's new fur coats (`proof_b5_fur.rs`),
/// two rows of eight on the ball.
pub const B5_FUR_GRID: u8 = 18;

/// How many P10a shapes each grid row holds, back to front: character heads,
/// cloth on the curtain, paint on the ball (`proof_p10a.rs`).
pub const P10A_ROWS: [usize; 3] = [4, 8, 8];

/// How many [`P7`] shapes each grid row holds, back to front: fish and snakes,
/// lizards and crocodiles, plain and iridescent plumage, barred and speckled.
pub const P7_ROWS: [usize; 4] = [10, 10, 7, 6];

/// How many [`P6`] shapes each grid row holds, back to front: fur coats, more
/// coats and pattern coats, leather and hide, keratin.
pub const P6_ROWS: [usize; 4] = [6, 9, 7, 8];

/// How many [`P5`] shapes each grid row holds, back to front: bark, masonry, plaster,
/// ground.
pub const P5_ROWS: [usize; 4] = [3, 5, 4, 6];

/// How many [`P4`] balls each grid row holds, back to front: metal, wood,
/// stone, ceramic.
pub const P4_ROWS: [usize; 4] = [10, 5, 4, 4];

/// Where shape `k` of a grid with these rows stands: rows back to front, each
/// centred on x = 0.
pub fn grid_pos(rows: &[usize], k: usize) -> [f32; 3] {
    let (mut row, mut i) = (0, k);
    while row < rows.len() - 1 && i >= rows[row] {
        i -= rows[row];
        row += 1;
    }
    let n = rows[row] as f32;
    [(i as f32 - (n - 1.0) * 0.5) * 0.62, 0.0, Z - 1.0 + 0.8 * row as f32]
}

/// Where [`P4`] ball `k` stands.
#[cfg(test)]
pub fn p4_pos(k: usize) -> [f32; 3] {
    grid_pos(&P4_ROWS, k)
}

/// The grid row shape `k` of a grid with these rows is in.
fn row_of(rows: &[usize], k: usize) -> usize {
    let (mut row, mut i) = (0, k);
    while row < rows.len() - 1 && i >= rows[row] {
        i -= rows[row];
        row += 1;
    }
    row
}

/// The rows of grid `set` (4 to 9).
fn rows_of(set: u8) -> &'static [usize] {
    match set {
        B3_GRID => &B3_ROWS,
        16 => &b2_foliage::ROWS_PLANTS,
        15 => &crate::proof_b2::ROWS_LAND,
        14 => &b2_foliage::ROWS,
        13 => &crate::proof_b2::ROWS,
        11 => &P10B_ROWS,
        P11A_GRID => &P11A_ROWS,
        B5_FUR_GRID => &[8, 8],
        12 => &P12_ROWS,
        10 => &P10A_ROWS,
        9 => &P9_ROWS,
        8 => &P8_ROWS,
        7 => &P7_ROWS,
        5 => &P5_ROWS,
        6 => &P6_ROWS,
        _ => &P4_ROWS,
    }
}

/// One vertex list of a proof shape: position, normal, uv and tangent (w = 1)
/// per vertex, and the triangle indices.
#[derive(Default)]
struct Mesh {
    pos: Vec<[f32; 3]>,
    nor: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    tan: Vec<[f32; 3]>,
    idx: Vec<u16>,
}

impl Mesh {
    fn vertex(&mut self, p: [f32; 3], n: [f32; 3], uv: [f32; 2], t: [f32; 3]) {
        self.pos.push(p);
        self.nor.push(n);
        self.uv.push(uv);
        self.tan.push(t);
    }

    /// The mesh as a glb with one material (named `name`, no textures).
    fn glb(&self, name: &str) -> Vec<u8> {
        let flat = |v: &[[f32; 3]]| v.iter().flatten().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
        let pos = flat(&self.pos);
        let nor = flat(&self.nor);
        let uv: Vec<u8> = self.uv.iter().flatten().flat_map(|x| x.to_le_bytes()).collect();
        let tan: Vec<u8> = self.tan.iter().flat_map(|t| [t[0], t[1], t[2], 1.0]).flat_map(f32::to_le_bytes).collect();
        let idx: Vec<u8> = self.idx.iter().flat_map(|i| i.to_le_bytes()).collect();
        let mut bin = Vec::new();
        let mut views = Vec::new();
        for part in [&pos, &nor, &uv, &tan, &idx] {
            views.push(json!({"buffer": 0, "byteOffset": bin.len(), "byteLength": part.len()}));
            bin.extend_from_slice(part);
        }
        let n = self.pos.len();
        let acc = |view: usize, ty: &str, count: usize, comp: u32| json!({"bufferView": view, "componentType": comp, "count": count, "type": ty});
        let lo = self.pos.iter().fold([f32::MAX; 3], |a, p| [a[0].min(p[0]), a[1].min(p[1]), a[2].min(p[2])]);
        let hi = self.pos.iter().fold([f32::MIN; 3], |a, p| [a[0].max(p[0]), a[1].max(p[1]), a[2].max(p[2])]);
        let mut p = acc(0, "VEC3", n, 5126);
        p["min"] = json!(lo);
        p["max"] = json!(hi);
        let doc = json!({
            "asset": {"version": "2.0"},
            "scene": 0,
            "scenes": [{"nodes": [0]}],
            "nodes": [{"mesh": 0, "name": name}],
            "meshes": [{"primitives": [{"attributes": {"POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2, "TANGENT": 3}, "indices": 4, "material": 0}]}],
            "materials": [{"name": name}],
            "accessors": [p, acc(1, "VEC3", n, 5126), acc(2, "VEC2", n, 5126), acc(3, "VEC4", n, 5126), acc(4, "SCALAR", self.idx.len(), 5123)],
            "bufferViews": views,
            "buffers": [{"byteLength": bin.len()}],
        });
        let mut js = serde_json::to_vec(&doc).unwrap_or_default();
        while !js.len().is_multiple_of(4) {
            js.push(b' ');
        }
        let total = 12 + 8 + js.len() + 8 + bin.len();
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&u32::try_from(total).unwrap_or(0).to_le_bytes());
        out.extend_from_slice(&u32::try_from(js.len()).unwrap_or(0).to_le_bytes());
        out.extend_from_slice(b"JSON");
        out.extend_from_slice(&js);
        out.extend_from_slice(&u32::try_from(bin.len()).unwrap_or(0).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&bin);
        out
    }
}

/// A cube 0.84 across standing on y = 0: six flat faces with their own uvs
/// and tangents, one material. The proof's seam check.
pub fn cube_glb() -> Vec<u8> {
    const H: f32 = 0.42;
    // (normal, u axis, v axis) with u x v = normal, so the winding faces out.
    const FACES: [([f32; 3], [f32; 3], [f32; 3]); 6] = [
        ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
        ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
        ([0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
        ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
    ];
    let mut m = Mesh::default();
    for (f, (n, t, b)) in FACES.iter().enumerate() {
        for (su, sv) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            let c = |k: usize| n[k] * H + (su * t[k] + sv * b[k]) * H + if k == 1 { H } else { 0.0 };
            m.vertex([c(0), c(1), c(2)], *n, [(su + 1.0) * 0.5, (sv + 1.0) * 0.5], *t);
        }
        let o = u16::try_from(f * 4).unwrap_or(0);
        m.idx.extend([0u16, 1, 2, 0, 2, 3].map(|i| o + i));
    }
    m.glb("cube")
}

/// A flat card 1 wide and 1.2 tall standing on y = 0 facing +z, uv u across and
/// v from the top (0) down (1). For flames, smoke, steam and screens.
pub fn card_glb() -> Vec<u8> {
    let mut m = Mesh::default();
    for (x, y) in [(-0.5f32, 0.0f32), (0.5, 0.0), (0.5, 1.2), (-0.5, 1.2)] {
        m.vertex([x, y, 0.0], [0.0, 0.0, 1.0], [x + 0.5, 1.0 - y / 1.2], [1.0, 0.0, 0.0]);
    }
    m.idx.extend([0u16, 1, 2, 0, 2, 3]);
    m.glb("card")
}

/// A ball 1 across (like the proof balls, which the grid draws at half size) standing on y = 0 with a real uv map: u once round it
/// (0 to 1 from the back, so the seam faces away from the camera), v from the
/// top pole (0) to the bottom (1); 32 segments by 16 rings. For fur, leather,
/// hide and keratin, whose patterns live in uv space.
pub fn sphere_glb() -> Vec<u8> {
    const SEG: usize = 32;
    const RING: usize = 16;
    const R: f32 = 0.5;
    let mut m = Mesh::default();
    for j in 0..=RING {
        let v = j as f32 / RING as f32;
        let (sp, cp) = (v * core::f32::consts::PI).sin_cos();
        for k in 0..=SEG {
            let u = k as f32 / SEG as f32;
            // u = 0 at the back (-z), running round to +x.
            let (s, c) = (u * core::f32::consts::TAU).sin_cos();
            let n = [s * sp, cp, -c * sp];
            m.vertex([n[0] * R, n[1] * R + R, n[2] * R], n, [u, v], [c, 0.0, s]);
        }
    }
    for j in 0..RING {
        for k in 0..SEG {
            let a = u16::try_from(j * (SEG + 1) + k).unwrap_or(0);
            let b = a + u16::try_from(SEG + 1).unwrap_or(0);
            m.idx.extend([a, a + 1, b, a + 1, b + 1, b]);
        }
    }
    m.glb("ball")
}

/// A horn: a cone 1.2 tall, 0.4 across at the base, standing on y = 0, u
/// once round it, v from the base (0) to the tip (1). For keratin.
pub fn horn_glb() -> Vec<u8> {
    const SEG: usize = 32;
    const R: f32 = 0.2;
    const HT: f32 = 1.2;
    let mut m = Mesh::default();
    // The side leans in by R over HT: the normal tilts up by that slope.
    let lean = R / HT;
    for k in 0..=SEG {
        let a = k as f32 / SEG as f32 * core::f32::consts::TAU;
        let (s, c) = a.sin_cos();
        let n = {
            let l = (1.0 + lean * lean).sqrt();
            [c / l, lean / l, s / l]
        };
        for (y, r, v) in [(0.0, R, 0.0), (HT, 0.012, 1.0)] {
            m.vertex([r * c, y, r * s], n, [k as f32 / SEG as f32, v], [-s, 0.0, c]);
        }
    }
    for k in 0..SEG {
        let o = u16::try_from(k * 2).unwrap_or(0);
        m.idx.extend([0u16, 1, 2, 1, 3, 2].map(|i| o + i));
    }
    m.glb("horn")
}

/// A trunk: 0.84 tall, 0.26 across, standing on y = 0, uv u once round it
/// (the first and last columns share a place, not a vertex) and v along it,
/// with a flat top. For bark.
pub fn trunk_glb() -> Vec<u8> {
    const SEG: usize = 32;
    const R: f32 = 0.13;
    const HT: f32 = 0.84;
    let mut m = Mesh::default();
    for k in 0..=SEG {
        let a = k as f32 / SEG as f32 * core::f32::consts::TAU;
        let (s, c) = a.sin_cos();
        // u runs with the angle, so the tangent is d(position)/du: (-sin, 0, cos) turned the right way.
        let t = [-s, 0.0, c];
        for (y, v) in [(0.0, 0.0), (HT, 1.0)] {
            m.vertex([R * c, y, R * s], [c, 0.0, s], [k as f32 / SEG as f32, v], t);
        }
    }
    for k in 0..SEG {
        let o = u16::try_from(k * 2).unwrap_or(0);
        m.idx.extend([0u16, 1, 2, 1, 3, 2].map(|i| o + i));
    }
    let top = u16::try_from(m.pos.len()).unwrap_or(0);
    m.vertex([0.0, HT, 0.0], [0.0, 1.0, 0.0], [0.5, 0.5], [1.0, 0.0, 0.0]);
    for k in 0..=SEG {
        let a = k as f32 / SEG as f32 * core::f32::consts::TAU;
        let (s, c) = a.sin_cos();
        m.vertex([R * c, HT, R * s], [0.0, 1.0, 0.0], [0.5 + c * 0.5, 0.5 + s * 0.5], [1.0, 0.0, 0.0]);
    }
    for k in 0..SEG {
        let o = top + 1 + u16::try_from(k).unwrap_or(0);
        m.idx.extend([top, o + 1, o]);
    }
    m.glb("trunk")
}

/// The `?set=p4` camera: the grid from above and in front, in the lower part
/// of the frame (the debug overlay covers the top).
pub const P4_CAMERA: CameraPose = CameraPose {
    eye: [0.0, 3.3, Z + 4.4],
    target: [0.0, 0.9, Z + 0.1],
    up: [0.0, 1.0, 0.0],
    fov_y_deg: 42.0,
};

/// The `?set=p9` camera: further back and higher, for six rows.
pub const P9_CAMERA: CameraPose = CameraPose {
    eye: [0.0, 4.8, Z + 7.4],
    target: [0.0, 0.6, Z + 0.9],
    up: [0.0, 1.0, 0.0],
    fov_y_deg: 42.0,
};

/// Where the row stands: beyond the grid, away from the other proofs.
pub const Z: f32 = -40.0;
/// The x of each shape.
pub const XS: [f32; 5] = [-2.6, -1.3, 0.0, 1.3, 2.6];
/// The low sun over the camera's right shoulder.
pub const SUN: [f32; 3] = [0.6, 0.55, 0.6];
/// The camera: the whole row across a 16:9 frame.
pub const CAMERA: CameraPose = CameraPose {
    eye: [0.0, 2.0, Z + 6.2],
    target: [0.0, 0.35, Z + 0.7],
    up: [0.0, 1.0, 0.0],
    fov_y_deg: 42.0,
};

/// The proof's state.
#[derive(Default)]
pub struct ProofRecipes {
    pub models: Vec<ModelId>,
    pub drawn: usize,
    /// The grid drawn: 0 the row, 4 the `?set=p4` grid, 5 the `?set=p5` grid.
    pub grid: u8,
    /// `?shape=cube`: the grid's shapes are cubes.
    pub cube: bool,
    /// The grid row a close camera frames (`?row=<n>`).
    pub row: Option<usize>,
    /// `?yaw=<degrees>`: every grid shape turned about the vertical (to bring a seam or a side to the camera).
    pub yaw: Option<f32>,
    /// `?far=<k>`: a row's close camera k times as far back (B3-P2: the
    /// cut-outs and fine patterns judged at a distance).
    pub far: Option<f32>,
    frames: u32,
    logged: bool,
}

/// The `Stage::Frame` system; registered after the scene's, so its camera
/// wins.
pub fn system() -> SystemDesc {
    SystemDesc::new("sandbox.proof_recipes", Stage::Frame, draw)
}

/// sRGB8 to linear.
fn linear(c: u8) -> f32 {
    let s = f32::from(c) / 255.0;
    if s <= 0.04045 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}

/// `glb` with its materials carrying `item`'s block (with the proof's
/// condition amounts) and fallback factors (its other extras dropped), or
/// `None` if the file doesn't parse.
pub fn with_block(glb: &[u8], item: &Item) -> Option<Vec<u8>> {
    rewrite(glb, item.id, &item.block()?, item.rough, item.metal)
}

/// `glb` with its materials carrying `block` (as `evani_material` `id`) and
/// the fallback factors.
pub fn rewrite(glb: &[u8], id: &str, block: &Block, rough: f32, metal: f32) -> Option<Vec<u8>> {
    let (json_bytes, bin) = evani_gltf::split_glb(glb).ok()?;
    let mut doc: Value = serde_json::from_slice(json_bytes).ok()?;
    let [r, g, b] = block.unpack().colours[0];
    for m in doc.get_mut("materials")?.as_array_mut()? {
        let m = m.as_object_mut()?;
        m.insert(
            "extras".into(),
            json!({"evani_material": id, "evani_recipe": block.to_hex()}),
        );
        // Merged, so a textured shape keeps its maps (B2-P1's alpha-masked cards).
        let pbr = m.entry("pbrMetallicRoughness").or_insert_with(|| json!({}));
        if let Some(pbr) = pbr.as_object_mut() {
            pbr.insert("baseColorFactor".into(), json!([linear(r), linear(g), linear(b), 1.0]));
            pbr.insert("roughnessFactor".into(), json!(rough));
            pbr.insert("metallicFactor".into(), json!(metal));
        }
        if evani_material_recipes::cut::is_cut(block) {
            // A cut-out row (B3-P2), as the catalogue writes it: masked and
            // seen from both sides.
            m.insert("alphaMode".into(), json!("MASK"));
            m.insert("alphaCutoff".into(), json!(0.5));
            m.insert("doubleSided".into(), json!(true));
        }
        if evani_material_recipes::blend::mode(block) != evani_material_recipes::blend::OPAQUE {
            // A see-through row (B3-P3), as the catalogue writes it: blended;
            // panes and cards from both sides.
            m.insert("alphaMode".into(), json!("BLEND"));
            let f = block.unpack();
            if (f.recipe == 24 && !matches!(f.variant, 1 | 2)) || f.recipe == 28 {
                m.insert("doubleSided".into(), json!(true));
            }
        }
    }
    let mut js = serde_json::to_vec(&doc).ok()?;
    while js.len() % 4 != 0 {
        js.push(b' ');
    }
    let mut bin = bin.map(<[u8]>::to_vec).unwrap_or_default();
    while bin.len() % 4 != 0 {
        bin.push(0);
    }
    let total = 12 + 8 + js.len() + if bin.is_empty() { 0 } else { 8 + bin.len() };
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&u32::try_from(total).ok()?.to_le_bytes());
    out.extend_from_slice(&u32::try_from(js.len()).ok()?.to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&js);
    if !bin.is_empty() {
        out.extend_from_slice(&u32::try_from(bin.len()).ok()?.to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&bin);
    }
    Some(out)
}

/// The value of `key=` in the page address (wasm only).
fn url_value(key: &str) -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        let url = web_sys::window()?.document()?.url().ok()?;
        let (_, query) = url.split_once('?')?;
        query
            .split('&')
            .find_map(|kv| kv.strip_prefix(key)?.strip_prefix('=').map(str::to_string))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = key;
        None
    }
}

/// `?q=lite|low|medium|high|ultra` in the page address: that
/// `material.quality.*` option (wasm only).
fn quality_from_url() -> Option<u32> {
    let q = url_value("q")?;
    evani_material_recipes::params::LEVELS
        .iter()
        .position(|l| *l == q)
        .and_then(|i| u32::try_from(i).ok())
}

/// `?row=<n>` in the page address (with `?set=p4` to `p9`): a close camera on
/// grid row `n` (back to front; the grid has as many rows as its `_ROWS` list) that draws that row alone, for judging
/// detail.
fn row_from_url() -> Option<usize> {
    url_value("row")?.parse().ok().filter(|&r: &usize| r < rows_of(set_from_url()).len())
}

/// The camera close on row `row` of a grid with these rows, framing all its
/// shapes.
pub fn grid_row_camera(rows: &[usize], row: usize) -> CameraPose {
    let first: usize = rows.iter().take(row).sum();
    let z = grid_pos(rows, first)[2];
    let half = (rows[row] as f32 - 1.0) * 0.31 + 0.35;
    let d = half / 0.62;
    CameraPose {
        eye: [0.0, 0.25 + d * 0.35, z + d],
        target: [0.0, 0.22, z],
        up: [0.0, 1.0, 0.0],
        fov_y_deg: 42.0,
    }
}

/// `?set=p4` to `p9` in the page address: draw that grid instead of the
/// row (0 when none).
fn set_from_url() -> u8 {
    match url_value("set").as_deref() {
        Some("p4") => 4,
        Some("p5") => 5,
        Some("p6") => 6,
        Some("p7") => 7,
        Some("p8") => 8,
        Some("p9") => 9,
        Some("p10a") => 10,
        Some("p10b") => 11,
        Some("b3-patterns") => B3_GRID,
        Some("p12") => 12,
        Some("p11a") => P11A_GRID,
        Some("b5-fur") => B5_FUR_GRID,
        Some("b2-options") => crate::proof_b2::GRID,
        Some("b2-foliage") => b2_foliage::GRID,
        Some("b2-land") => crate::proof_b2::GRID_LAND,
        Some("b2-plants") => b2_foliage::GRID_PLANTS,
        _ => 0,
    }
}

fn draw(world: &mut World) {
    if url_value("set").as_deref() == Some("b3-glass") {
        b3_glass::frame(world);
        return;
    }
    #[cfg(feature = "recipes-nature-proof")]
    if url_value("set").as_deref() == Some("p11b") {
        proof_p11b::frame(world);
        return;
    }
    let Some(mut p) = world
        .try_res_mut::<ProofRecipes>()
        .map(|mut p| core::mem::take(&mut *p))
    else {
        return;
    };
    if p.frames == 0 {
        let mut reg = world.res_mut::<evani_core::params::ParamRegistry>();
        let _ = reg.set_by_name(
            "render.light.direction",
            evani_core::params::ParamValue::Vec3(SUN),
        );
        // B2-P1: the sun behind the row (back-light), and global rain and snow.
        if url_value("sun").is_some_and(|s| s == "back") {
            let _ = reg.set_by_name("render.light.direction", evani_core::params::ParamValue::Vec3([-0.25, 0.5, -0.83]));
        }
        for layer in ["wet", "snow"] {
            if let Some(a) = url_value(layer).and_then(|a| a.parse::<f32>().ok()) {
                let _ = reg.set_by_name(&format!("material.cond.{layer}.amount"), evani_core::params::ParamValue::F32(a.clamp(0.0, 1.0)));
            }
        }
        p.grid = set_from_url();
        p.cube = url_value("shape").is_some_and(|s| s == "cube");
        p.row = row_from_url();
        p.yaw = url_value("yaw").and_then(|y| y.parse().ok());
        p.far = url_value("far").and_then(|y| y.parse().ok()).filter(|k: &f32| *k > 0.0);
    }
    // The groups the families mirror (skin, detail, water, glass, foliage) only
    // follow a change made after start-up, so `?q=` is applied on frame 4.
    if p.frames == 4 {
        let mut reg = world.res_mut::<evani_core::params::ParamRegistry>();
        if let Some(q) = quality_from_url() {
            for name in ["material.quality.conditions", "material.quality.detail", "material.quality.fur", "material.quality.skin", "material.quality.glass", "material.quality.reflections", "material.quality.foliage"] {
                let _ = reg.set_by_name(name, evani_core::params::ParamValue::Enum(q));
            }
        }
    }
    p.frames = p.frames.saturating_add(1);
    if let Some(mut models) = world.try_res_mut::<Models>() {
        if p.models.is_empty() && p.grid == 10 {
            #[cfg(feature = "recipes-family-proof")]
            let built = crate::proof_p10a::glbs(CURTAIN, BALL);
            #[cfg(not(feature = "recipes-family-proof"))]
            let built: Result<Vec<(String, Vec<u8>)>, String> = Err("?set=p10a needs the recipes-family-proof feature".into());
            match built {
                Ok(all) => {
                    for (id, glb) in all {
                        p.models.push(models.add_glb(&format!("models/p10a-{id}.glb"), &glb));
                    }
                }
                Err(e) if !p.logged => {
                    p.logged = true;
                    world.res_mut::<ErrorLog>().error(format!("sandbox: recipes proof: {e}"));
                }
                Err(_) => {}
            }
        }
        if p.models.is_empty() && p.grid == 11 {
            let file = if url_value("file").is_some_and(|f| f == "low") { P10B_LOW } else { P10B_HIGH };
            p.models.push(models.add_glb("models/p10b.glb", file));
        }
        if p.models.is_empty() && p.grid == 12 {
            #[cfg(feature = "recipes-family-proof")]
            let built = proof_p12::glbs(CURTAIN);
            #[cfg(not(feature = "recipes-family-proof"))]
            let built: Result<Vec<(String, Vec<u8>)>, String> = Err("?set=p12 needs the recipes-family-proof feature".into());
            match built {
                Ok(all) => {
                    for (id, glb) in all {
                        p.models.push(models.add_glb(&format!("models/p12-{id}.glb"), &glb));
                    }
                }
                Err(e) if !p.logged => {
                    p.logged = true;
                    world.res_mut::<ErrorLog>().error(format!("sandbox: recipes proof: {e}"));
                }
                Err(_) => {}
            }
        }
        if p.models.is_empty() && p.grid == P11A_GRID {
            #[cfg(feature = "recipes-family-proof")]
            let built = proof_p11a::glbs(CURTAIN, BALL);
            #[cfg(not(feature = "recipes-family-proof"))]
            let built: Result<Vec<(String, Vec<u8>)>, String> = Err("?set=p11a needs the recipes-family-proof feature".into());
            match built {
                Ok(all) => {
                    for (id, glb) in all {
                        p.models.push(models.add_glb(&format!("models/p11a-{id}.glb"), &glb));
                    }
                }
                Err(e) if !p.logged => {
                    p.logged = true;
                    world.res_mut::<ErrorLog>().error(format!("sandbox: recipes proof: {e}"));
                }
                Err(_) => {}
            }
        }
        if p.models.is_empty() && p.grid == B5_FUR_GRID {
            #[cfg(feature = "recipes-family-proof")]
            let built = proof_b5_fur::glbs(BALL);
            #[cfg(not(feature = "recipes-family-proof"))]
            let built: Result<Vec<(String, Vec<u8>)>, String> = Err("?set=b5-fur needs the recipes-family-proof feature".into());
            match built {
                Ok(all) => {
                    for (id, glb) in all {
                        p.models.push(models.add_glb(&format!("models/b5-fur-{id}.glb"), &glb));
                    }
                }
                Err(e) if !p.logged => {
                    p.logged = true;
                    world.res_mut::<ErrorLog>().error(format!("sandbox: recipes proof: {e}"));
                }
                Err(_) => {}
            }
        }
        if p.models.is_empty() && b2_foliage::is_grid(p.grid) {
            for (id, glb) in b2_foliage::glbs(p.grid) {
                p.models.push(models.add_glb(&format!("models/recipe-{id}.glb"), &glb));
            }
        }
        if p.models.is_empty() && p.grid != 0 && p.grid != 10 && p.grid != 11 && p.grid != 12 && p.grid != P11A_GRID && p.grid != B5_FUR_GRID && !b2_foliage::is_grid(p.grid) {
            let set: &[(&str, &str, f32, f32)] = match p.grid {
                B3_GRID => &B3,
                13 | 15 => crate::proof_b2::table(p.grid),
                5 => &P5,
                6 => &P6,
                7 => &P7,
                8 => &P8,
                9 => &P9,
                _ => &P4,
            };
            let (cube, trunk, sphere, horn, card) = (cube_glb(), trunk_glb(), sphere_glb(), horn_glb(), card_glb());
            for &(id, hex, rough, metal) in set {
                // Bark rides on a trunk (uv round and along it), keratin on a
                // horn (rings along it), tile on a cube (a real uv square); fur,
                // leather and hide on a uv sphere; the rest on balls.
                let shape: &[u8] = if p.cube || p.grid == B3_GRID {
                    &cube
                } else if crate::proof_b2::is_grid(p.grid) {
                    match crate::proof_b2::kind(id) {
                        crate::proof_b2::Kind::Cube => &cube,
                        crate::proof_b2::Kind::Sphere => &sphere,
                        crate::proof_b2::Kind::Trunk => &trunk,
                    }
                } else if id.starts_with("bark.") || (p.grid == 7 && (id.starts_with("snake.") || id == "leather.snake")) {
                    &trunk
                } else if p.grid == 9 && CARDS.contains(&id) {
                    &card
                } else if p.grid == 9 && TREADS.contains(&id) {
                    &trunk
                } else if (p.grid == 9 && WEAVES.contains(&id)) || (p.grid == 8 && id == "ice.cube") {
                    &cube
                } else if p.grid >= 6 {
                    if (p.grid == 6 && HORNS.contains(&id)) || (p.grid == 8 && POINTS.contains(&id)) { &horn } else { &sphere }
                } else if id.starts_with("tile.") {
                    &cube
                } else {
                    BALL
                };
                if let Some(glb) = Block::from_hex(hex)
                    .ok()
                    .and_then(|b| rewrite(shape, id, &b, rough, metal))
                {
                    p.models
                        .push(models.add_glb(&format!("models/recipe-{id}.glb"), &glb));
                }
            }
        }
        if p.models.is_empty() {
            for item in &ITEMS {
                match with_block(item.shape, item) {
                    Some(glb) => p
                        .models
                        .push(models.add_glb(&format!("models/recipe-{}.glb", item.id), &glb)),
                    None if !p.logged => {
                        p.logged = true;
                        world.res_mut::<ErrorLog>().error(format!(
                            "sandbox: recipes proof: {} did not rewrite",
                            item.id
                        ));
                    }
                    None => {}
                }
            }
            for k in 0..TESTS {
                let id = format!("test.cond.{k}");
                if let Some(glb) = rewrite(BALL, &id, &test_block(k), 0.5, 0.0) {
                    p.models
                        .push(models.add_glb(&format!("models/recipe-{id}.glb"), &glb));
                }
            }
        }
        p.drawn = 0;
        for (i, &id) in p.models.iter().enumerate() {
            if let (Some(ModelState::Failed(e)), false) = (models.state(id), p.logged) {
                p.logged = true;
                world
                    .res_mut::<ErrorLog>()
                    .error(format!("sandbox: recipes proof: {e}"));
            }
            if matches!(models.state(id), Some(ModelState::Ready)) {
                p.drawn += 1;
            }
            if p.grid != 0 {
                if p.row.is_some_and(|r| r != row_of(rows_of(p.grid), i)) {
                    // A close-up shows its row alone, so nothing stands in front.
                    continue;
                }
                let (at, size) = if crate::proof_b2::is_grid(p.grid) { (crate::proof_b2::pos(p.grid, i), crate::proof_b2::SCALE) } else { (grid_pos(rows_of(p.grid), i), TEST_SCALE) };
                let mut m = ModelInstance::new(id, at).scale([size; 3]);
                if let Some(y) = p.yaw {
                    let (s, c) = (y.to_radians() * 0.5).sin_cos();
                    m = m.rotation([0.0, s, 0.0, c]);
                } else if p.cube || p.grid == B3_GRID {
                    // A turn about the vertical, so the camera sees two sides and the top.
                    m = m.rotation([0.0, 0.2955, 0.0, 0.9553]);
                }
                models.draw(m);
                continue;
            }
            match XS.get(i) {
                Some(&x) => models.draw(ModelInstance::new(id, [x, 0.0, Z])),
                None => {
                    let k = i - ITEMS.len();
                    models.draw(
                        ModelInstance::new(id, [test_x(k), 0.0, TEST_Z])
                            .scale([TEST_SCALE; 3]),
                    );
                }
            }
        }
    }
    let ground = world
        .try_res::<LookData>()
        .map_or(0, |l| RenderWorld::material(&l, "ground"));
    if let Some(mut rw) = world.try_res_mut::<RenderWorld>() {
        // Take over the frame: the proof's camera, nothing the kit drew.
        rw.camera = match (p.grid, p.row) {
            (0, _) => CAMERA,
            (9 | 12, None) => P9_CAMERA,
            (11, None) => P10B_CAMERA,
            (13 | 15, None) => crate::proof_b2::camera(p.grid),
            (13 | 15, Some(r)) => crate::proof_b2::row_camera(p.grid, r),
            (g, Some(r)) => {
                let mut c = grid_row_camera(rows_of(g), r);
                if let Some(k) = p.far {
                    c.eye = core::array::from_fn(|i| c.target[i] + (c.eye[i] - c.target[i]) * k);
                }
                c
            }
            (_, None) => P4_CAMERA,
        };
        rw.sprites.clear();
        rw.meshes.clear();
        rw.blobs.clear();
        // The batch 3 grid's back rows (the cut-outs) stand beyond the usual
        // floor: a deeper one catches their shadows.
        let deep = p.grid == B3_GRID;
        let mut plane = MeshInstance::new(MeshId::PLANE, [0.0, 0.0, if deep { Z + 5.0 } else { Z }]);
        plane.scale = if deep { [16.0, 1.0, 24.0] } else { [14.0, 1.0, 8.0] };
        plane.material = ground;
        rw.draw_mesh(plane);
    }
    if p.frames % 10 == 0 {
        let reg = world.res::<evani_core::params::ParamRegistry>();
        let q = reg
            .id("material.quality.conditions")
            .and_then(|id| reg.value(id));
        let title = format!(
            "rc q={q:?} stage={} frames={} drawn={}",
            match evani_render::model::recipes::stage_state() {
                Some(true) => "patched",
                Some(false) => "fallback",
                None => "pending",
            },
            p.frames,
            p.drawn
        );
        #[cfg(target_arch = "wasm32")]
        if let Some(d) = web_sys::window().and_then(|w| w.document()) {
            d.set_title(&title);
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = title;
    }
    if let Some(mut r) = world.try_res_mut::<ProofRecipes>() {
        *r = p;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::params::ParamRegistry;

    /// The P10b model is what `cargo xtask convert --materials` wrote: in both
    /// tier files all eight materials name their map row, and each recipe row
    /// carries exactly the block the catalogue encodes for it (a family row,
    /// cloth, carries its layers data instead).
    #[test]
    fn the_p10b_model_carries_the_catalogue_rows() {
        use evani_material_recipes::catalog::Catalog;
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../catalog/materials");
        let cat = Catalog::load_dir(&dir).unwrap();
        let map: serde_json::Map<String, Value> = serde_json::from_str(include_str!("../../../tests/fixtures/models/p10b.map.json")).unwrap();
        assert_eq!(map.len(), 8);
        for (tier, glb) in [("high", P10B_HIGH), ("low", P10B_LOW)] {
            let (js, _) = evani_gltf::split_glb(glb).unwrap();
            let doc: Value = serde_json::from_slice(js).unwrap();
            let mats = doc["materials"].as_array().unwrap();
            assert_eq!(mats.len(), 8, "{tier}");
            for m in mats {
                let name = m["name"].as_str().unwrap();
                let id = map[name].as_str().unwrap();
                let row = cat.row(id).unwrap();
                assert_eq!(m["extras"]["evani_material"], json!(id), "{tier}: {name}");
                if row.recipe == "cloth" {
                    assert!(m["extras"].get("evani_recipe").is_none(), "{tier}: cloth is the layers family");
                    assert!(m["extras"]["evani_layers"]["sheen"].is_object(), "{tier}: {name}");
                } else {
                    assert_eq!(m["extras"]["evani_recipe"], json!(cat.encode(row).unwrap().to_hex()), "{tier}: {name} is the {id} block");
                }
            }
        }
    }

    /// Every block is what the catalogue encodes for its row.
    #[test]
    fn the_blocks_are_the_catalogue_rows() {
        use evani_material_recipes::catalog::Catalog;
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../catalog/materials");
        let cat = Catalog::load_dir(&dir).unwrap();
        for item in &ITEMS {
            let row = cat
                .row(item.id)
                .unwrap_or_else(|| panic!("{} is a catalogue row", item.id));
            let want = cat.encode(row).unwrap().to_hex();
            assert_eq!(item.hex, want, "{}", item.id);
        }
        // The P4 grid: every block is its row's, each row has numbers, the
        // rows come in recipe order and the grid's rows add up.
        let mut last = 0u8;
        for &(id, hex, _, _) in &P4 {
            let row = cat.row(id).unwrap_or_else(|| panic!("{id} is a catalogue row"));
            assert_eq!(hex, cat.encode(row).unwrap().to_hex(), "{id}");
            assert!(row.params.contains_key("numbers"), "{id}: has numbers");
            let code = cat.recipe_for(row).unwrap().code;
            assert!(code >= last, "{id}: grid rows in recipe order");
            last = code;
        }
        // The P5 grid, the same way (rows in recipe order within the grid).
        let mut last = 0u8;
        for &(id, hex, _, _) in &P5 {
            let row = cat.row(id).unwrap_or_else(|| panic!("{id} is a catalogue row"));
            assert_eq!(hex, cat.encode(row).unwrap().to_hex(), "{id}");
            assert!(row.params.contains_key("numbers"), "{id}: has numbers");
            assert_eq!(row.status, "ready", "{id}: the recipes draw it");
            let code = cat.recipe_for(row).unwrap().code;
            assert!(code >= last, "{id}: grid rows in recipe order");
            last = code;
        }
        // The P6 grid: every block is its row's, each row has numbers and the recipes draw it
        // (rows are grouped by family, not by recipe code); every P6 recipe is in it.
        let mut codes = std::collections::BTreeSet::new();
        for &(id, hex, _, _) in &P6 {
            let row = cat.row(id).unwrap_or_else(|| panic!("{id} is a catalogue row"));
            assert_eq!(hex, cat.encode(row).unwrap().to_hex(), "{id}");
            assert!(row.params.contains_key("numbers"), "{id}: has numbers");
            assert_eq!(row.status, "ready", "{id}: the recipes draw it");
            codes.insert(cat.recipe_for(row).unwrap().code);
        }
        assert_eq!(codes.into_iter().collect::<Vec<_>>(), [11, 12, 15, 19]);
        // The P7 grid, the same way: scales and feather, every variant of each shown.
        let mut codes = std::collections::BTreeSet::new();
        let mut variants = std::collections::BTreeSet::new();
        for &(id, hex, _, _) in &P7 {
            let row = cat.row(id).unwrap_or_else(|| panic!("{id} is a catalogue row"));
            assert_eq!(hex, cat.encode(row).unwrap().to_hex(), "{id}");
            assert!(row.params.contains_key("numbers"), "{id}: has numbers");
            assert_eq!(row.status, "ready", "{id}: the recipes draw it");
            let code = cat.recipe_for(row).unwrap().code;
            codes.insert(code);
            variants.insert((code, row.params["variant"].as_str().unwrap_or("").to_string()));
        }
        assert_eq!(codes.into_iter().collect::<Vec<_>>(), [13, 14]);
        assert_eq!(variants.len(), 8, "all four variants of both recipes are on the grid");
        // The P8 grid: gem and ice, every variant of each shown, the recipes draw every row.
        let mut codes = std::collections::BTreeSet::new();
        let mut variants = std::collections::BTreeSet::new();
        for &(id, hex, _, _) in &P8 {
            let row = cat.row(id).unwrap_or_else(|| panic!("{id} is a catalogue row"));
            assert_eq!(hex, cat.encode(row).unwrap().to_hex(), "{id}");
            assert!(row.params.contains_key("numbers"), "{id}: has numbers");
            assert_eq!(row.status, "ready", "{id}: the recipes draw it");
            let code = cat.recipe_for(row).unwrap().code;
            codes.insert(code);
            variants.insert((code, row.params["variant"].as_str().unwrap_or("").to_string()));
        }
        assert_eq!(codes.into_iter().collect::<Vec<_>>(), [25, 27]);
        assert_eq!(variants.len(), 9, "all five gem variants and all four ice variants are on the grid");
        // The P9 grid: every row has numbers and is drawn by its recipe; every P9 recipe
        // and every variant of it is on the grid; the shapes' ids exist.
        let mut codes = std::collections::BTreeSet::new();
        let mut variants = std::collections::BTreeSet::new();
        for &(id, hex, _, _) in &P9 {
            let row = cat.row(id).unwrap_or_else(|| panic!("{id} is a catalogue row"));
            assert_eq!(hex, cat.encode(row).unwrap().to_hex(), "{id}");
            assert!(row.params.contains_key("numbers"), "{id}: has numbers");
            assert_eq!(row.status, "ready", "{id}: the recipes draw it");
            let code = cat.recipe_for(row).unwrap().code;
            codes.insert(code);
            variants.insert((code, row.params["variant"].as_str().unwrap_or("").to_string()));
        }
        assert_eq!(codes.into_iter().collect::<Vec<_>>(), [2, 3, 22, 28, 29]);
        assert_eq!(variants.len(), 26, "every variant of the five recipes is on the grid");
        assert_eq!(P9_ROWS.iter().sum::<usize>(), P9.len());
        assert!([CARDS.as_slice(), TREADS.as_slice(), WEAVES.as_slice()].concat().iter().all(|h| P9.iter().any(|r| r.0 == *h)), "every card, tread and weave is a P9 row");
        assert_eq!(P8_ROWS.iter().sum::<usize>(), P8.len());
        assert!(POINTS.iter().all(|h| P8.iter().any(|r| r.0 == *h)), "every cone is a P8 row");
        assert_eq!(P7_ROWS.iter().sum::<usize>(), P7.len());
        assert_eq!(P6_ROWS.iter().sum::<usize>(), P6.len());
        assert!(HORNS.iter().all(|h| P6.iter().any(|r| r.0 == *h)), "every horn is a P6 row");
        assert_eq!(P5_ROWS.iter().sum::<usize>(), P5.len());
        assert_eq!(P6_ROWS.len(), P7_ROWS.len());
        assert_eq!(P7_ROWS.len(), P8_ROWS.len());
        assert_eq!(P4_ROWS.iter().sum::<usize>(), P4.len());
        // B3-P1: every row of the b3-patterns grid is its catalogue block, has
        // numbers and is drawn; every recipe that draws a layout is on it,
        // and every layout row but the reference brick uses one.
        let mut codes = std::collections::BTreeSet::new();
        for &(id, hex, _, _) in &B3 {
            let row = cat.row(id).unwrap_or_else(|| panic!("{id} is a catalogue row"));
            assert_eq!(hex, cat.encode(row).unwrap().to_hex(), "{id}");
            assert!(row.params.contains_key("numbers"), "{id}: has numbers");
            assert_eq!(row.status, "ready", "{id}: the recipes draw it");
            codes.insert(cat.recipe_for(row).unwrap().code);
            let b = Block::from_hex(hex).unwrap().unpack();
            // The first 12 rows are the layouts package's; the rest are the
            // later recipe packages' (their own variants and extras).
            if B3.iter().position(|r| r.0 == id).is_some_and(|k| k < 12) {
                let opted = b.variant >= 6 || (b.recipe != 7 && b.extras & !0b11 != 0);
                assert_eq!(opted, id != "brick.red.common", "{id}: uses a layout");
            }
        }
        assert_eq!(codes.into_iter().collect::<Vec<_>>(), [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 20, 21, 23]);
        assert_eq!(B3_ROWS.iter().sum::<usize>(), B3.len());
        assert_eq!(p4_pos(0)[2], p4_pos(9)[2], "the metal row");
        assert!(p4_pos(10)[2] > p4_pos(9)[2], "wood starts the next row");
    }

    /// Every P4 recipe is compiled in, and its live switches are registered
    /// under `material.recipe.<recipe>.<extra>` (default on); switching one
    /// off sets its deny bit in the words the GPU gets, and the detail
    /// quality reaches the packed levels.
    #[test]
    fn the_p4_recipe_switches_are_registered_and_work() {
        use evani_core::params::ParamValue;
        use evani_material_recipes::params::{RecipeParams, frame_words};
        use evani_material_recipes::recipes::BUILT;
        use evani_material_recipes::shader::{COMPILED, extra_base};
        assert_eq!(COMPILED.len(), BUILT.len(), "recipes-proof turns on every built recipe");
        let mut app = crate::build_app();
        app.run_headless(2, 1000.0 / 60.0);
        let p = (*app.world.res::<RecipeParams>()).clone();
        let mut reg = app.world.res_mut::<ParamRegistry>();
        for s in BUILT {
            for (k, extra) in s.extras.iter().enumerate() {
                let n = format!("material.recipe.{}.{extra}", s.name);
                assert_eq!(reg.id(&n).and_then(|id| reg.value(id)), Some(ParamValue::Bool(true)), "{n}");
                let bit = extra_base(s.code).unwrap() + k as u32;
                let denied = |reg: &ParamRegistry| (frame_words(reg, &p, "high", 0.0)[12 + (bit >> 5) as usize] >> (bit & 31)) & 1;
                assert_eq!(denied(&reg), 0, "{n}: allowed");
                reg.set_by_name(&n, ParamValue::Bool(false)).unwrap();
                assert_eq!(denied(&reg), 1, "{n}: denied on the GPU");
                reg.set_by_name(&n, ParamValue::Bool(true)).unwrap();
            }
        }
        reg.set_by_name("material.quality.detail", ParamValue::Enum(1)).unwrap();
        assert_eq!(frame_words(&reg, &p, "high", 0.0)[8] & 15, 0, "detail at lite");
        reg.set_by_name("material.quality.detail", ParamValue::Enum(0)).unwrap();
        assert_eq!(frame_words(&reg, &p, "low", 0.0)[8] & 15, 1, "auto follows the tier");
        // P10a: paint's clear coat strength rides in word 11 as strength - 1,
        // so its default (and an all-zero uniform) is the authored coat.
        let coat = evani_material_recipes::params::COAT_STRENGTH;
        assert_eq!(reg.id(coat).and_then(|id| reg.value(id)), Some(ParamValue::F32(1.0)), "{coat}");
        assert_eq!(frame_words(&reg, &p, "high", 0.0)[11], 0);
        reg.set_by_name(coat, ParamValue::F32(0.0)).unwrap();
        assert_eq!(f32::from_bits(frame_words(&reg, &p, "high", 0.0)[11]), -1.0, "coat off on the GPU");
    }

    /// The cube, trunk, uv sphere and horn parse, have one material, and the recipe block
    /// reaches it.
    #[test]
    fn the_cube_and_the_trunk_load_with_a_block() {
        let (id, hex, rough, metal) = P5[0];
        let b = Block::from_hex(hex).unwrap();
        for shape in [cube_glb(), trunk_glb(), sphere_glb(), horn_glb()] {
            let glb = rewrite(&shape, id, &b, rough, metal).expect("shape rewrites");
            let g = evani_gltf::Gltf::from_glb(&glb).unwrap();
            let p = evani_render::model::prepare(&g).unwrap();
            assert_eq!(p.materials.len(), 1);
            assert_eq!(p.materials[0].recipe, b.to_words());
        }
    }

    /// Each rewritten shape parses, and the renderer reads the block into
    /// every material's binding 12 words.
    #[test]
    fn the_shapes_carry_their_blocks() {
        for item in &ITEMS {
            let glb = with_block(item.shape, item).unwrap_or_else(|| panic!("{}", item.id));
            let g = evani_gltf::Gltf::from_glb(&glb).unwrap();
            let p = evani_render::model::prepare(&g).unwrap();
            let block = item.block().unwrap();
            assert_eq!(
                &block.0[..24],
                &Block::from_hex(item.hex).unwrap().0[..24],
                "{}: only the amounts differ from the catalogue",
                item.id
            );
            let want = block.to_words();
            assert!(!p.materials.is_empty());
            for m in &p.materials {
                assert_eq!(m.recipe, want, "{}", item.id);
                assert_ne!(m.recipe[0] & 0xff, 0, "{}: a recipe code", item.id);
            }
            let f = block.unpack();
            for (k, &a) in item.cond.iter().enumerate() {
                assert!(
                    a == 0 || (f.cond_mask >> k) & 1 == 1,
                    "{}: layer {k} is one the row lists",
                    item.id
                );
            }
        }
        for k in 0..TESTS {
            let f = test_block(k).unpack();
            assert_eq!(f.cond_mask.count_ones(), if k < 8 { 1 } else { 8 });
            assert!(f.cond_amounts.iter().any(|&a| a > 0));
        }
    }

    /// Every condition layer is compiled in and its live switches are
    /// registered (on, amount and each look number); switching a layer off
    /// or lowering the conditions quality changes the mask the GPU gets.
    #[test]
    fn the_condition_switches_are_registered_and_work() {
        use evani_core::params::ParamValue;
        use evani_material_recipes::conditions::{self, LOOKS, apply};
        use evani_material_recipes::params::{RecipeParams, mask_rule};
        use evani_material_recipes::shader::{LAYER_NAMES, LAYERS};
        assert_eq!(LAYERS.len(), 8, "recipes-proof turns on every cond-<layer>");
        let mut app = crate::build_app();
        app.run_headless(2, 1000.0 / 60.0);
        let p = (*app.world.res::<RecipeParams>()).clone();
        let mut reg = app.world.res_mut::<ParamRegistry>();
        assert!(reg.id(conditions::ENABLED).is_some());
        for (bit, layer) in LAYER_NAMES.iter().enumerate() {
            for what in ["on", "amount"] {
                let n = format!("material.cond.{layer}.{what}");
                assert!(reg.id(&n).is_some(), "{n} is registered");
            }
            for lk in LOOKS.iter().filter(|l| usize::from(l.bit) == bit) {
                let n = conditions::name(lk.bit, lk.name);
                assert!(reg.id(&n).is_some(), "{n} is registered");
            }
        }
        // The leather lists wet and mud (own amounts .59 and .82).
        let w = ITEMS[0].block().unwrap().to_words();
        let mask = |reg: &ParamRegistry| apply(&w, &mask_rule(reg, &p, "high"))[0] >> 24;
        assert_eq!(mask(&reg), 0b11, "wet and mud");
        reg.set_by_name("material.cond.mud.on", ParamValue::Bool(false)).unwrap();
        assert_eq!(mask(&reg), 0b01, "mud switched off");
        reg.set_by_name("material.cond.mud.on", ParamValue::Bool(true)).unwrap();
        reg.set_by_name("material.quality.conditions", ParamValue::Enum(1)).unwrap();
        assert_eq!(mask(&reg), 0b01, "lite: one layer, and mud is not a lite one");
        reg.set_by_name("material.quality.conditions", ParamValue::Enum(0)).unwrap();
        reg.set_by_name(conditions::ENABLED, ParamValue::Bool(false)).unwrap();
        assert_eq!(mask(&reg), 0, "all conditions off");
        reg.set_by_name(conditions::ENABLED, ParamValue::Bool(true)).unwrap();
        // A global amount reaches a layer the material lists at 0.
        let wood = ITEMS[1].block().unwrap().to_words();
        reg.set_by_name("material.cond.wet.amount", ParamValue::F32(0.8)).unwrap();
        assert_eq!(apply(&wood, &mask_rule(&reg, &p, "high"))[0] >> 24, 0b101, "rain wets the walnut too");
    }

    /// The scene loads all five shapes, draws them under its camera, logs no
    /// error, and the recipe switches are registered.
    #[test]
    fn the_recipes_scene_draws_its_row() {
        let mut app = crate::build_app();
        app.run_headless(8, 1000.0 / 60.0);
        assert_eq!(app.world.res::<ErrorLog>().error_count(), 0);
        let p = app.world.res::<ProofRecipes>();
        assert_eq!(p.models.len(), ITEMS.len() + TESTS);
        assert_eq!(
            p.drawn,
            ITEMS.len() + TESTS,
            "all five shapes and nine test balls loaded and were drawn"
        );
        assert_eq!(app.world.res::<RenderWorld>().camera.eye, CAMERA.eye);
        let reg = app.world.res::<ParamRegistry>();
        assert!(reg.id(evani_material_recipes::params::ENABLED).is_some());
        for name in evani_material_recipes::params::QUALITY {
            assert!(reg.id(name).is_some(), "{name} is registered");
        }
    }
}
