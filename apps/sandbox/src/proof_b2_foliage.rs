//! The batch 2 foliage proof set (`?set=b2-foliage`, feature `recipes-proof`;
//! package B2-P1, dev only, never in evani.toml): every foliage variant on the
//! shape it is made for, all built in code (no asset, no stream manifest):
//!
//! - back row: five leaf cards (oak, autumn maple, magnolia, dead brown, fern),
//!   quads whose base texture's alpha is a leaf shape (a 64 x 64 mask made
//!   here), so the model pass's alpha-mask discard cuts them; tilted back 40
//!   degrees, so snow can settle on them;
//! - second row: three needle sprays (pine, spruce, juniper) on a spray-shaped
//!   mask, wheat on a grass-tuft mask, thatch (straw) on a cube;
//! - front rows: nine ground tiles with a gentle bump (uv in metres, so moss
//!   has its uv; turf and litter are world space and need none): lawn,
//!   prairie, bunch grass, clover, frosted grass, burnt grass, moss, leaf
//!   litter and pine needles.
//!
//! B2-P7b adds `?set=b2-plants` (grid 16): the 16 foliage rows whose numbers were
//! set afterwards (ten leaf cards, three needle sprays, three grass grounds),
//! on the same cards and tiles.
//!
//! The page address adds to batch 1's: `?sun=back` puts the sun behind the
//! row (the back-light), `?wet=<0-1>` and `?snow=<0-1>` set the global rain
//! and snow amounts, `?yaw=180` shows the cards' undersides.

use evani_material_recipes::Block;
use serde_json::json;

/// The set's catalogue rows, their blocks (a test holds them equal to the
/// catalogue's), the fallback roughness and metalness.
pub const SET: [(&str, &str, f32, f32); 19] = [
    ("leaf.oak", "1400034d3f6a2a5e8a34a8c86059598c4d0d09004d00002e0000000000000000", 0.6, 0.0),
    ("leaf.maple.autumn", "1400034dc8401ae89a20ffb040994080263306003300007e0000000000000000", 0.6, 0.0),
    ("leaf.magnolia", "1400034d1e3a1a2e50248a5a3033cc59d9000800e600002b0000000000000000", 0.3, 0.0),
    ("leaf.dead_brown", "1400024d6a44248a5a309a6a3a80e6660dff08004d0000e80000000000000000", 0.7, 0.0),
    ("leaf.fern", "1400034d3a6a285a8a30a0c8504d66b31a0d2400330000280000000000000000", 0.6, 0.0),
    ("needle.pine", "1401030d1e3a1e2e4a227a9a404d994d590d3100000000020000000000000000", 0.6, 0.0),
    ("needle.spruce", "1401030d2a4a444a6a647a9a8040b3664d005800000000ce0000000000000000", 0.6, 0.0),
    ("needle.juniper", "1401070d5a6e5a3a4a8a8a9a7033b34d261a4b80000000170000000000000000", 0.6, 0.0),
    ("crop.wheat.ripe", "1402050fc09040e0b860f0d080668000404d1cd90000002e0000000000000000", 0.7, 0.0),
    ("roof.thatch", "1403005da8803ac8a050d8b87080cc0033333100000000010000000000000000", 0.8, 0.0),
    ("ground.grass.lawn", "1404058f3a6a245a8a304a3a24594d001a0d4b00000000960000000000000000", 0.9, 0.0),
    ("ground.grass.prairie", "1404058f9a8040c0a0585a4a3080400026662b1a000000880000000000000000", 0.9, 0.0),
    ("ground.grass.bunch", "1404058f5a7a308a9a486a5438664d001a4d3199000000620000000000000000", 0.9, 0.0),
    ("ground.clover", "1404058f3a6a2a5a8a3a3a30204dd90033002400000000e50000000000000000", 0.9, 0.0),
    ("ground.grass.frosted", "1404058f4a6a407a9a704a40304d4d001a334400000000670000006600000000", 0.9, 0.0),
    ("ground.grass.burnt", "1404048f1a16123a34307a74708040000000314d000000470000000000000000", 0.9, 0.0),
    ("ground.moss.carpet", "1405038f3a6a1e7aa02aa0c8406699990d006400000000d30000000000000000", 0.9, 0.0),
    ("ground.leaf_litter", "1406008fa0401ad08a283a2a1a8099001a4d0f00000000190000000000000000", 0.9, 0.0),
    ("ground.pine_needles", "1406008f8a4a22a8642e3a2818661f001a332400000000ad0000000000000000", 0.9, 0.0),
];

/// The plants set's rows (B2-P7b), generated from the catalogue; a test holds
/// every block equal to it.
pub const SET_PLANTS: [(&str, &str, f32, f32); 16] = [
// BEGIN PLANTS
    ("leaf.maple.green", "1400034d3a7a245aa030b0d868594d8c400d06003300009f0000000000000000", 0.6, 0.0),
    ("leaf.birch", "1400034d5a8a307aa83cc8d86066405933140d00260000150000000000000000", 0.6, 0.0),
    ("leaf.cottonwood", "1400034d4a7a2a6a9a3ac0d0704d4d80990d0800d900001f0000000000000000", 0.6, 0.0),
    ("leaf.willow", "1400034d6a8a589ab088d0e0b04d334d33000f00b30000b70000000000000000", 0.6, 0.0),
    ("leaf.elm", "1400034d3a6a28507c30a0c0604d66a61a1a0900330000b70000000000000000", 0.6, 0.0),
    ("leaf.palm", "1400034d2e6a2a4a8a34a0d0604d4d7366002400330000090000000000000000", 0.6, 0.0),
    ("leaf.banana", "1400034d4a8a2a6aaa34c0e07066409980400300260000cc0000000000000000", 0.6, 0.0),
    ("leaf.ivy", "1400034d1e4a242e60307aa050409980cc000a00990000690000000000000000", 0.6, 0.0),
    ("leaf.vine.grape", "1400034d4a7a2c6a9a3ab8d8684d4d99261a0500590000280000000000000000", 0.6, 0.0),
    ("leaf.jungle", "1400034d1e5a282e7a3490d0604d8080bf0004004d0000d80000000000000000", 0.6, 0.0),
    ("needle.fir", "1401030d1e4a2a2e603a80a86040805933003e003300009d0000000000000000", 0.6, 0.0),
    ("needle.cedar", "1401030d2a5a383a704888b0704da633260071000000001f0000000000000000", 0.6, 0.0),
    ("needle.dead", "1401020d8a4a22a86a306a4a2866cc1a0dff37000000009a0000000000000000", 0.6, 0.0),
    ("ground.grass.marsh", "1404058f3a7a305a9a404a4430663300591a1a260000006f0000000000000000", 0.9, 0.0),
    ("ground.grass.alpine", "1404058f3a72305a9a3e6a5a40804d001a005e0d0000005f0000000000000000", 0.9, 0.0),
    ("ground.grass.dead", "1404048fb0a070d0c0906a5a406659000db33140000000b20000000000000000", 0.9, 0.0),
// END PLANTS
];

/// How many [`SET_PLANTS`] shapes each grid row holds: two rows of leaves, the
/// needle sprays, the grass grounds.
pub const ROWS_PLANTS: [usize; 4] = [5, 5, 3, 3];

/// The `?set=b2-plants` grid number.
pub const GRID_PLANTS: u8 = 16;

/// Whether grid `g` is one of this module's sets.
pub fn is_grid(g: u8) -> bool {
    g == GRID || g == GRID_PLANTS
}

/// How many [`SET`] shapes each grid row holds, back to front: leaves,
/// needles with wheat and thatch, two rows of ground.
pub const ROWS: [usize; 4] = [5, 5, 5, 4];

/// The `?set=b2-foliage` grid number (12 is `?set=p12`, 13 is `?set=b2-options`).
pub const GRID: u8 = 14;

/// The shape a row rides on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Leaf,
    Spray,
    Tuft,
    Cube,
    Tile,
}

/// The shape of row `id`.
pub fn shape_of(id: &str) -> Shape {
    match id.split('.').next() {
        Some("leaf") => Shape::Leaf,
        Some("needle") => Shape::Spray,
        Some("crop") => Shape::Tuft,
        Some("roof") => Shape::Cube,
        _ => Shape::Tile,
    }
}

/// The mask textures are this many texels across.
const MASK: usize = 64;

/// A leaf (tip at the top, v = 0; a short stalk at the bottom), a needle
/// spray (a feather-shaped envelope round a twig) or a grass tuft (tapering
/// blades fanning from the root), as alpha at texel centre (u, v).
fn mask(shape: Shape, u: f32, v: f32) -> bool {
    let x = (u - 0.5).abs();
    match shape {
        Shape::Leaf => {
            let s = (1.0 - v - 0.08) / 0.9;
            ((0.0..=1.0).contains(&s) && x < 0.44 * (s * core::f32::consts::PI).sin().powf(0.75) * (1.0 - 0.25 * s)) || (v > 0.9 && x < 0.02)
        }
        Shape::Spray => {
            // A frond: pointed at both ends, needle tips with gaps at the edge.
            let s = 1.0 - v;
            let w = 0.47 * (s.min(0.95) / 0.95 * core::f32::consts::PI).sin().powf(0.7);
            let tip = ((v + x * 0.7) * 14.0).fract() < 0.55;
            (x < w && (x < w * 0.6 || tip)) || (x < 0.015 && s < 0.97)
        }
        _ => {
            let h = 1.0 - v;
            let lean = (u - 0.5) * h * 0.5;
            let w = (u - lean) * 7.0;
            ((w - w.floor() - 0.5).abs() < 0.42 * (1.0 - h) + 0.03) && h < 0.95 - 0.25 * (w.floor() * 1.7).sin().abs()
        }
    }
}

/// CRC-32 (IEEE) of the PNG chunk type and data.
fn crc32(bytes: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in bytes {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 == 1 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

/// A white RGBA PNG whose alpha is the shape's mask (stored, uncompressed
/// deflate: the proof is a dev build, 16 KB per mask).
pub fn mask_png(shape: Shape) -> Vec<u8> {
    let mut raw = Vec::with_capacity(MASK * (MASK * 4 + 1));
    for y in 0..MASK {
        raw.push(0);
        for x in 0..MASK {
            let on = mask(shape, (x as f32 + 0.5) / MASK as f32, (y as f32 + 0.5) / MASK as f32);
            raw.extend_from_slice(&[255, 255, 255, if on { 255 } else { 0 }]);
        }
    }
    let mut z = vec![0x78, 0x01];
    let chunks: Vec<&[u8]> = raw.chunks(65_535).collect();
    for (k, part) in chunks.iter().enumerate() {
        let n = u16::try_from(part.len()).unwrap_or(0);
        z.push(u8::from(k + 1 == chunks.len()));
        z.extend_from_slice(&n.to_le_bytes());
        z.extend_from_slice(&(!n).to_le_bytes());
        z.extend_from_slice(part);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + u32::from(x)) % 65_521;
        b = (b + a) % 65_521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let side = u32::try_from(MASK).unwrap_or(0).to_be_bytes();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&side);
    ihdr.extend_from_slice(&side);
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    for (ty, data) in [(b"IHDR", ihdr), (b"IDAT", z), (b"IEND", Vec::new())] {
        png.extend_from_slice(&u32::try_from(data.len()).unwrap_or(0).to_be_bytes());
        let mut body = ty.to_vec();
        body.extend_from_slice(&data);
        png.extend_from_slice(&body);
        png.extend_from_slice(&crc32(&body).to_be_bytes());
    }
    png
}

/// A mesh: position, normal, uv, tangent per vertex, and triangles.
#[derive(Default)]
struct Mesh {
    pos: Vec<[f32; 3]>,
    nor: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    idx: Vec<u16>,
}

impl Mesh {
    /// The mesh as a glb with one material named `name`; with `png`, its
    /// base colour texture, alpha-masked and double-sided (a card).
    fn glb(&self, name: &str, png: Option<&[u8]>) -> Vec<u8> {
        let f3 = |v: &[[f32; 3]]| v.iter().flatten().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
        let uv: Vec<u8> = self.uv.iter().flatten().flat_map(|x| x.to_le_bytes()).collect();
        let tan: Vec<u8> = self.nor.iter().flat_map(|_| [1.0f32, 0.0, 0.0, 1.0]).flat_map(f32::to_le_bytes).collect();
        let idx: Vec<u8> = self.idx.iter().flat_map(|i| i.to_le_bytes()).collect();
        let mut bin = Vec::new();
        let mut views = Vec::new();
        let parts: Vec<Vec<u8>> = [f3(&self.pos), f3(&self.nor), uv, tan, idx].into_iter().chain(png.map(<[u8]>::to_vec)).collect();
        for part in &parts {
            while !bin.len().is_multiple_of(4) {
                bin.push(0);
            }
            views.push(json!({"buffer": 0, "byteOffset": bin.len(), "byteLength": part.len()}));
            bin.extend_from_slice(part);
        }
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        let n = self.pos.len();
        let acc = |view: usize, ty: &str, count: usize, comp: u32| json!({"bufferView": view, "componentType": comp, "count": count, "type": ty});
        let lo = self.pos.iter().fold([f32::MAX; 3], |a, p| [a[0].min(p[0]), a[1].min(p[1]), a[2].min(p[2])]);
        let hi = self.pos.iter().fold([f32::MIN; 3], |a, p| [a[0].max(p[0]), a[1].max(p[1]), a[2].max(p[2])]);
        let mut p = acc(0, "VEC3", n, 5126);
        p["min"] = json!(lo);
        p["max"] = json!(hi);
        let mut doc = json!({
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
        if png.is_some() {
            doc["images"] = json!([{"bufferView": 5, "mimeType": "image/png"}]);
            doc["textures"] = json!([{"source": 0}]);
            doc["materials"][0] = json!({"name": name, "pbrMetallicRoughness": {"baseColorTexture": {"index": 0}}, "alphaMode": "MASK", "alphaCutoff": 0.5, "doubleSided": true});
        }
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

/// A card 1 wide and 1.2 tall, standing on y = 0 and leaning back 40 degrees
/// (its face towards +z and up), uv u across and v from the top (0) down,
/// with `shape`'s mask.
pub fn card_glb(shape: Shape) -> Vec<u8> {
    let (s, c) = 40f32.to_radians().sin_cos();
    let mut m = Mesh::default();
    for (x, y) in [(-0.5f32, 0.0f32), (0.5, 0.0), (0.5, 1.2), (-0.5, 1.2)] {
        m.pos.push([x, y * c, -y * s]);
        m.nor.push([0.0, s, c]);
        m.uv.push([x + 0.5, 1.0 - y / 1.2]);
    }
    m.idx.extend([0u16, 1, 2, 0, 2, 3]);
    m.glb("card", Some(&mask_png(shape)))
}

/// A ground tile 1.2 m square with a gentle bump (so slopes and their
/// normals show), lifted clear of the floor; uv in metres.
pub fn tile_glb() -> Vec<u8> {
    const N: usize = 16;
    const S: f32 = 1.2;
    let h = |x: f32, z: f32| 0.04 + 0.12 * (1.0 - (x * x + z * z) / (0.36 * 2.0)).max(0.0).powi(2);
    let mut m = Mesh::default();
    for j in 0..=N {
        for i in 0..=N {
            let (x, z) = ((i as f32 / N as f32 - 0.5) * S, (j as f32 / N as f32 - 0.5) * S);
            let e = 0.01;
            let (dx, dz) = ((h(x + e, z) - h(x - e, z)) / (2.0 * e), (h(x, z + e) - h(x, z - e)) / (2.0 * e));
            let l = (dx * dx + dz * dz + 1.0).sqrt();
            m.pos.push([x, h(x, z), z]);
            m.nor.push([-dx / l, 1.0 / l, -dz / l]);
            m.uv.push([x + S * 0.5, z + S * 0.5]);
        }
    }
    for j in 0..N {
        for i in 0..N {
            let a = u16::try_from(j * (N + 1) + i).unwrap_or(0);
            let b = a + u16::try_from(N + 1).unwrap_or(0);
            m.idx.extend([a, b, a + 1, a + 1, b, b + 1]);
        }
    }
    m.glb("tile", None)
}

/// Every row of grid `grid` as (id, glb with its block).
pub fn glbs(grid: u8) -> Vec<(String, Vec<u8>)> {
    let (leaf, spray, tuft, cube, tile) = (card_glb(Shape::Leaf), card_glb(Shape::Spray), card_glb(Shape::Tuft), crate::proof_recipes::cube_glb(), tile_glb());
    let set: &[(&str, &str, f32, f32)] = if grid == GRID_PLANTS { &SET_PLANTS } else { &SET };
    set.iter()
        .filter_map(|&(id, hex, rough, metal)| {
            let shape = match shape_of(id) {
                Shape::Leaf => &leaf,
                Shape::Spray => &spray,
                Shape::Tuft => &tuft,
                Shape::Cube => &cube,
                Shape::Tile => &tile,
            };
            let b = Block::from_hex(hex).ok()?;
            crate::proof_recipes::rewrite(shape, id, &b, rough, metal).map(|g| (id.to_string(), g))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    /// Every block is its catalogue row's, every row is ready with numbers,
    /// every foliage variant is on the grid, and the rows add up.
    #[test]
    fn the_set_is_the_catalogue_rows() {
        use evani_material_recipes::catalog::Catalog;
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../catalog/materials");
        let cat = Catalog::load_dir(&dir).unwrap();
        let mut variants = std::collections::BTreeSet::new();
        for &(id, hex, _, _) in &SET {
            let row = cat.row(id).unwrap_or_else(|| panic!("{id} is a catalogue row"));
            assert_eq!(hex, cat.encode(row).unwrap().to_hex(), "{id}");
            assert_eq!(row.status, "ready", "{id}: the recipe draws it");
            assert_eq!(cat.recipe_for(row).unwrap().code, 20, "{id}: foliage");
            assert!(row.params.contains_key("numbers"), "{id}: has numbers");
            variants.insert(row.params["variant"].as_str().unwrap_or("").to_string());
        }
        assert_eq!(variants.len(), 7, "all seven foliage variants are on the grid");
        assert_eq!(ROWS.iter().sum::<usize>(), SET.len());
        assert_eq!(glbs(GRID).len(), SET.len());
    }

    /// The plants set: every block is its catalogue row's, foliage, ready,
    /// none repeats the first set, and the rows add up (10 leaves, 3 needles,
    /// 3 grass).
    #[test]
    fn the_plants_set_is_the_catalogue_rows() {
        use evani_material_recipes::catalog::Catalog;
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../catalog/materials");
        let cat = Catalog::load_dir(&dir).unwrap();
        for &(id, hex, _, _) in &SET_PLANTS {
            let row = cat.row(id).unwrap_or_else(|| panic!("{id} is a catalogue row"));
            assert_eq!(hex, cat.encode(row).unwrap().to_hex(), "{id}");
            assert_eq!(row.status, "ready", "{id}");
            assert_eq!(cat.recipe_for(row).unwrap().code, 20, "{id}: foliage");
            assert!(!SET.iter().any(|r| r.0 == id), "{id} is in one set only");
        }
        assert_eq!(ROWS_PLANTS.iter().sum::<usize>(), SET_PLANTS.len());
        let shapes: Vec<Shape> = SET_PLANTS.iter().map(|r| shape_of(r.0)).collect();
        assert_eq!(shapes.iter().filter(|s| **s == Shape::Leaf).count(), 10);
        assert_eq!(shapes.iter().filter(|s| **s == Shape::Spray).count(), 3);
        assert_eq!(shapes.iter().filter(|s| **s == Shape::Tile).count(), 3);
        assert_eq!(glbs(GRID_PLANTS).len(), SET_PLANTS.len());
    }

    /// The cards load alpha-masked and double-sided with their mask embedded,
    /// the block reaches the material, and the masks cut a real shape.
    #[test]
    fn the_cards_are_masked_and_carry_their_block() {
        for (k, shape) in [Shape::Leaf, Shape::Spray, Shape::Tuft].into_iter().enumerate() {
            let (id, hex, rough, metal) = SET[k * 5];
            let b = Block::from_hex(hex).unwrap();
            let glb = crate::proof_recipes::rewrite(&card_glb(shape), id, &b, rough, metal).unwrap();
            let g = evani_gltf::Gltf::from_glb(&glb).unwrap();
            let p = evani_render::model::prepare(&g).unwrap();
            assert_eq!(p.materials.len(), 1);
            assert_eq!(p.materials[0].recipe, b.to_words(), "{id}");
            assert_eq!(p.materials[0].words[8], 1.0, "{id}: alpha mask");
            assert_eq!(p.materials[0].words[9], 1.0, "{id}: double-sided");
            assert!(p.textures.first().and_then(|t| t.image.as_ref()).is_some_and(|png| png.starts_with(b"\x89PNG")), "{id}: the mask is embedded");
            let on = (0..MASK * MASK).filter(|i| mask(shape, (i % MASK) as f32 / MASK as f32, (i / MASK) as f32 / MASK as f32)).count();
            assert!(on > MASK * MASK / 8 && on < MASK * MASK * 3 / 4, "{shape:?}: the mask cuts a shape ({on} texels)");
        }
        let g = evani_gltf::Gltf::from_glb(&tile_glb()).unwrap();
        assert_eq!(evani_render::model::prepare(&g).unwrap().materials.len(), 1);
    }

    /// The stored PNG is well formed: signature, IHDR first, every CRC right.
    #[test]
    fn the_mask_png_is_well_formed() {
        let png = mask_png(Shape::Leaf);
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        let mut at = 8;
        let mut types = Vec::new();
        while at < png.len() {
            let len = u32::from_be_bytes(png[at..at + 4].try_into().unwrap()) as usize;
            let body = &png[at + 4..at + 8 + len];
            let crc = u32::from_be_bytes(png[at + 8 + len..at + 12 + len].try_into().unwrap());
            assert_eq!(crc32(body), crc);
            types.push(String::from_utf8_lossy(&body[..4]).to_string());
            at += 12 + len;
        }
        assert_eq!(types, ["IHDR", "IDAT", "IEND"]);
    }
}
