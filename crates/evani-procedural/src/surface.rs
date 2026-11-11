//! Realism surfaces: a catalogue material on a procedural part.
//!
//! A [`Surface`] is one row of the material catalogue
//! (catalog/materials/materials.json) as the engine carries it: its 32-byte
//! block (recipe, colours, numbers, seed, condition amounts) and its plain
//! PBR factors, plus how the part's surface is laid out in uv ([`Uv`]) and
//! how many metres one uv unit spans. A part that has one
//! ([`crate::Part::surface`]) is drawn by the procedural module through the
//! engine's textured-model path (feature `surface`): the module writes the
//! asset's surfaced parts into one in-memory `.glb` ([`glb`]) whose materials
//! carry the block (`extras.evani_recipe`), so the recipe stage paints the
//! concrete's pores, the oak's grain or the soil's pebbles on the GPU. No
//! texture or mesh is downloaded: the block is 32 bytes in the code.
//!
//! Without the `surface` feature (or in a game that lists no recipes) the
//! same part draws as before, a flat colour ([`Surface::base_rgb`]) through
//! the forward pass, so nothing breaks; the recipe stage itself comes with
//! the game's `[materials] recipes` (docs/PROCEDURAL.md "Realism").
//!
//! The presets below are catalogue rows encoded once by the catalogue's own
//! encoder; evani-procedural-lib's test `surface_presets_match_the_catalogue`
//! checks every one against the catalogue, so they can never drift from it.

use evani_render::mesh::Mesh;

/// Bytes in a block.
pub const BLOCK_BYTES: usize = 32;

/// How a part's uv is laid out (all in metres over [`Surface::scale`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Uv {
    /// Each triangle projected along its face's main axis (a box or
    /// tri-planar map): boxes, blocks, terrain (flat ground from above,
    /// cliffs from the side).
    Box,
    /// Wrapped round the y axis (u: arc length, v: height); faces that look
    /// up or down are projected from above: cylinders, cones, posts, trunks.
    Cylinder,
    /// Wrapped round the y axis and from pole to pole (arc lengths on the
    /// sphere through the part's centre): spheres, capsules, boulders.
    Sphere,
}

/// A catalogue material on a part.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Surface {
    /// The row's 32-byte block (layout: evani-material-recipes `block`).
    pub block: [u8; BLOCK_BYTES],
    /// The plain PBR factors drawn when no recipe stage is compiled in.
    pub roughness: f32,
    pub metallic: f32,
    pub uv: Uv,
    /// Metres per uv unit (the recipe lays its pattern in uv units).
    pub scale: f32,
    /// A card (leaves): drawn from both sides and alpha-masked (glTF
    /// `doubleSided`, alphaMode MASK), so a cut-out recipe (a game listing
    /// `"cutout"` in `[materials] recipes`) cuts its shape from the block.
    pub card: bool,
    /// See-through (glass): written with glTF alphaMode BLEND, so a game
    /// with the see-through pass (evani-render `material-recipes-blend`)
    /// draws it over what is behind (the glass recipe's own alpha); without
    /// that pass it draws as an opaque tinted gloss. Off for every preset
    /// (Asset Studio column 13, the office building's glazing).
    pub blend: bool,
}

const fn nib(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => 0,
    }
}

/// sRGB8 to linear.
pub fn srgb_to_linear(c: u8) -> f32 {
    let s = c as f32 / 255.0;
    if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
}

/// Linear to sRGB8.
pub fn linear_to_srgb(l: f32) -> u8 {
    let l = l.clamp(0.0, 1.0);
    let s = if l <= 0.003_130_8 { l * 12.92 } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0).round() as u8
}

/// The condition layers, in block order (bit and amount byte).
pub const CONDITIONS: [&str; 8] = ["wet", "mud", "dust", "snow", "wear", "oxide", "growth", "blood"];

impl Surface {
    /// A catalogue row's block (64 hex characters) and PBR factors, laid out
    /// by box projection at 2 m per uv unit (the material viewer's scale).
    pub const fn hex(hex: &str, roughness: f32, metallic: f32) -> Self {
        let h = hex.as_bytes();
        let mut block = [0u8; BLOCK_BYTES];
        let mut i = 0;
        while i < BLOCK_BYTES && 2 * i + 1 < h.len() {
            block[i] = (nib(h[2 * i]) << 4) | nib(h[2 * i + 1]);
            i += 1;
        }
        Self { block, roughness, metallic, uv: Uv::Box, scale: 2.0, card: false, blend: false }
    }

    /// Draw it as cards: both sides, alpha-masked ([`Surface::card`]).
    pub const fn cards(mut self) -> Self {
        self.card = true;
        self
    }

    /// Draw it see-through ([`Surface::blend`]).
    pub const fn see_through(mut self) -> Self {
        self.blend = true;
        self
    }

    pub const fn uv(mut self, uv: Uv) -> Self {
        self.uv = uv;
        self
    }

    /// Metres per uv unit: smaller is a finer pattern.
    pub const fn scale(mut self, metres: f32) -> Self {
        self.scale = metres;
        self
    }

    /// Colour `k` (0 = A, 1 = B, 2 = C) in linear.
    pub fn colour(&self, k: usize) -> [f32; 3] {
        let o = 4 + 3 * k.min(2);
        [srgb_to_linear(self.block[o]), srgb_to_linear(self.block[o + 1]), srgb_to_linear(self.block[o + 2])]
    }

    /// Colour A in linear: the part's colour where no recipe stage draws.
    pub fn base_rgb(&self) -> [f32; 3] {
        self.colour(0)
    }

    /// Move the material's colours so colour A becomes `rgb` (linear); B and
    /// C keep their relation to A (the grain stays darker than the wood,
    /// the aggregate lighter than the cement), so a colour param recolours
    /// the material without flattening it.
    pub fn tint(mut self, rgb: [f32; 3]) -> Self {
        let a = self.colour(0);
        let ratio: [f32; 3] = std::array::from_fn(|c| if a[c] > 1e-4 { rgb[c] / a[c] } else { 1.0 });
        for k in 0..3 {
            let col = self.colour(k);
            for c in 0..3 {
                self.block[4 + 3 * k + c] = linear_to_srgb(if k == 0 { rgb[c] } else { col[c] * ratio[c] });
            }
        }
        self
    }

    /// Set colour `k` (0 = A, 1 = B, 2 = C) to `rgb` (linear).
    pub fn set_colour(mut self, k: usize, rgb: [f32; 3]) -> Self {
        for (c, v) in rgb.iter().enumerate() {
            self.block[4 + 3 * k.min(2) + c] = linear_to_srgb(*v);
        }
        self
    }

    /// Recipe number `i` (0..10) as the block's unorm byte (0..1 over the
    /// number's own range).
    pub fn number(mut self, i: usize, unorm: f32) -> Self {
        self.block[13 + i.min(9)] = (unorm.clamp(0.0, 1.0) * 255.0).round() as u8;
        self
    }

    /// The recipe's variant (its pattern kind: flat-sawn or quarter-sawn
    /// wood, soil or gravel ground...; the catalogue's recipes.json order).
    pub fn variant(mut self, v: u8) -> Self {
        self.block[1] = v;
        self
    }

    /// The recipe extras this material uses (a bit mask in the recipe's
    /// extras order: grain relief, plank gaps... for wood; parallax, glints,
    /// translucency, tiles for stone).
    pub fn extras(mut self, mask: u8) -> Self {
        self.block[2] = mask;
        self
    }

    /// The pattern offset: a different seed moves the recipe's pattern (no
    /// two seeded assets share their grain or their stones).
    pub fn seed(mut self, seed: u32) -> Self {
        self.block[23] = (crate::noise::hash(seed) & 0xff) as u8;
        self
    }

    /// The seed the recipe stage reads (`b.seed`: the block's byte 23), for
    /// geometry that must line up with the material ([`crate::noise::gpu`]).
    pub fn shader_seed(&self) -> u32 {
        self.block[23] as u32
    }

    /// Recipe number `i` as the shader reads it (the stored byte over 255).
    pub fn number_of(&self, i: usize) -> f32 {
        self.block[13 + i.min(9)] as f32 / 255.0
    }

    /// Turn a condition layer on with this default amount (0..1): `wear`,
    /// `dust`, `mud`, `wet`, `snow`, `oxide`, `growth`, `blood`.
    pub fn condition(mut self, layer: &str, amount: f32) -> Self {
        if let Some(i) = CONDITIONS.iter().position(|c| *c == layer) {
            if amount > 0.0 {
                self.block[3] |= 1 << i;
            } else {
                self.block[3] &= !(1 << i);
            }
            self.block[24 + i] = (amount.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
        self
    }

    /// The block as 64 lower-case hex characters (`extras.evani_recipe`).
    pub fn block_hex(&self) -> String {
        const D: &[u8; 16] = b"0123456789abcdef";
        let mut s = String::with_capacity(BLOCK_BYTES * 2);
        for b in self.block {
            s.push(D[(b >> 4) as usize] as char);
            s.push(D[(b & 15) as usize] as char);
        }
        s
    }
}

// ---- presets: catalogue rows (checked against the catalogue by a test) -----

/// Catalogue presets for realism parts, by catalogue id. Each is a `const`,
/// so only the ones an asset names reach its download.
pub mod presets {
    use super::Surface;

    /// `concrete.smooth`
    pub const CONCRETE_SMOOTH: Surface = Surface::hex("080203dda8a8a45a5a58000000cc3310260a8000000000ee0000000000000000", 0.8, 0.0);
    /// `concrete.polished`
    pub const CONCRETE_POLISHED: Surface = Surface::hex("080402dda8a8a65a5a58000000381a1a0d101a00000000b40000000000000000", 0.2196, 0.0);
    /// `timber.oak`
    pub const TIMBER_OAK: Surface = Surface::hex("040101d5c89a5e9a6e3e5a3a20994b3373082800cc0000790000000000000000", 0.6, 0.0);
    /// `wood.floor.oak`
    pub const WOOD_FLOOR_OAK: Surface = Surface::hex("040306dfd8a868a870385a3a204d2c59730d13594d00007f0000000000000000", 0.302, 0.0);
    /// `wood.beam.oak`
    pub const WOOD_BEAM_OAK: Surface = Surface::hex("040111df7e5e3e4a32202a1a10d9174d991a284d4d00001e0000000000000000", 0.851, 0.0);
    /// `rock.granite.grey`
    pub const ROCK_GRANITE_GREY: Surface = Surface::hex("060003dd9c9c9a5c5c5ee8e6e099001a980099000000005d0000000000000000", 0.6, 0.0);
    /// `rock.limestone`
    pub const ROCK_LIMESTONE: Surface = Surface::hex("060401ddb4b2a898968cd8d6ccd9002f4c0000004d0000840000000000000000", 0.851, 0.0);
    /// `rock.basalt`
    pub const ROCK_BASALT: Surface = Surface::hex("060003dd4242442e2e305a5a5cbf001aff001a00000000e70000000000000000", 0.749, 0.0);
    /// `rock.sandstone.buff`
    pub const ROCK_SANDSTONE_BUFF: Surface = Surface::hex("060201ddd8c4a0b8a47eece0c4e60039bf990000000000600000000000000000", 0.902, 0.0);
    /// `rock.slate`
    pub const ROCK_SLATE: Surface = Surface::hex("060203dd4a4e56363a426a707a8c006398cc2600000000eb0000000000000000", 0.549, 0.0);
    /// `rock.cave_wall`
    pub const ROCK_CAVE_WALL: Surface = Surface::hex("060700dd4a443c5c564a8a807080001a32000000000000789900000000000000", 0.502, 0.0);
    /// `rock.mossy_boulder`
    pub const ROCK_MOSSY_BOULDER: Surface = Surface::hex("060001dd7a7a745a5a549a9a90e6001a4c000000000000f10000000000009900", 0.902, 0.0);
    /// `ground.soil.loam`
    pub const GROUND_SOIL_LOAM: Surface = Surface::hex("090001c93a2a1c6a665cc8b48af20d1f002400b3000000600000000000000000", 0.949, 0.0);
    /// `ground.soil.packed`
    pub const GROUND_SOIL_PACKED: Surface = Surface::hex("090001c98a6e4c847f76c8b48af2084000240026000000770000000000000000", 0.949, 0.0);
    /// `ground.soil.forest_floor`
    pub const GROUND_SOIL_FOREST_FLOOR: Surface = Surface::hex("090010c93a2e226a62547a5a2cf2310d002400800000e6b70000000000000000", 0.949, 0.0);
    /// `ground.soil.cracked_dry`
    pub const GROUND_SOIL_CRACKED_DRY: Surface = Surface::hex("090004c9b8a8888a7a62a89878e6080000240026150000e80000000000000000", 0.902, 0.0);
    /// `ground.sand.desert`
    pub const GROUND_SAND_DESERT: Surface = Surface::hex("09010289b4723ac8884ae0a860f20300d91f4d00000000e50000000000000000", 0.949, 0.0);
    /// `ground.sand.beach`
    pub const GROUND_SAND_BEACH: Surface = Surface::hex("090102896a5a40b8a888e4cc94f2030db32f80000000001f0000000000000000", 0.949, 0.0);
    /// `ground.sand.wet`
    pub const GROUND_SAND_WET: Surface = Surface::hex("090102898a7248a89264c8b48459030a402f4d00000000d0cc00000000000000", 0.349, 0.0);
    /// `ground.gravel.scree`
    pub const GROUND_GRAVEL_SCREE: Surface = Surface::hex("0905014d4a463e7a766ea09a8ee62ce600240000000000c30000000000000000", 0.902, 0.0);
    /// `ground.gravel.creek`
    pub const GROUND_GRAVEL_CREEK: Surface = Surface::hex("0902014d4a4a447a766aa8a090991cf200240000000000c5b300000000000000", 0.6, 0.0);
    /// `ground.grass.meadow`
    pub const GROUND_GRASS_MEADOW: Surface = Surface::hex("1402058f6a9a3aa8a850d8c870b31a002626244d0000002d0000000000000000", 0.5, 0.0);
    /// `ground.grass.lawn`
    pub const GROUND_GRASS_LAWN: Surface = Surface::hex("1404058f2f5c1c4e7e263a2e1c4d99001a0d1a00cc0000960000000000000000", 0.5, 0.0);
    /// `ground.grass.alpine`
    pub const GROUND_GRASS_ALPINE: Surface = Surface::hex("1404058f3a72305a9a3e6a5a40804d001a005e0d0000005f0000000000000000", 0.5, 0.0);
    /// `ground.moss.carpet`
    pub const GROUND_MOSS_CARPET: Surface = Surface::hex("1405038f3a6a1e7aa02aa0c8406699990d006400000000d30000000000000000", 0.5, 0.0);
    /// `ground.snow.packed`
    pub const GROUND_SNOW_PACKED: Surface = Surface::hex("1b020786e0e4eca8b4c4f0f4f81a33394d4d9900000000db0000000000000000", 0.5, 0.0);
    /// `ground.snow.fresh`
    pub const GROUND_SNOW_FRESH: Surface = Surface::hex("1b020686f4f8ffc8dcf0ffffff000063e680ff00000000190000000000000000", 0.5, 0.0);
    /// `plaster.white`
    pub const PLASTER_WHITE: Surface = Surface::hex("080002dde6e2d88a8478000000e6591a00101a00000000eb0000000000000000", 0.902, 0.0);
    /// `metal.steel.brushed`
    pub const METAL_STEEL_BRUSHED: Surface = Surface::hex("010105bf9ea2a8d2d5da6a4a3052ffcc65994d00240000be0000000000000000", 0.3216, 1.0);
    /// `metal.steel.polished`
    pub const METAL_STEEL_POLISHED: Surface = Surface::hex("010105bfcfcfccf2f4f66a4a3047ff80cc594d00240000290000000000000000", 0.2784, 1.0);
    /// `wall.paint.matte`
    pub const WALL_PAINT_MATTE: Surface = Surface::hex("080002d5d6d0c28a8478000000f21a1a00100d00000000240000000000000000", 0.949, 0.0);
    /// `finish.painted.cream`
    pub const FINISH_PAINTED_CREAM: Surface = Surface::hex("020000d5e8dcb8c8b890ffffff8c004d002a00803d0000850000000000000000", 0.549, 0.0);
    /// `rock.chalk`
    pub const ROCK_CHALK: Surface = Surface::hex("060000ddece8e0d4d0c4f8f6f0f2001a72000000000000e90000000000000000", 0.949, 0.0);
    /// `rock.shale`
    pub const ROCK_SHALE: Surface = Surface::hex("060201dd4a464034302c625a50e6008d7fe60000000000d70000000000000000", 0.902, 0.0);
    /// `rock.sandstone.red`
    pub const ROCK_SANDSTONE_RED: Surface = Surface::hex("060201ddb0583ad8a078e0c8a8e60044bfd90000000000600000000000000000", 0.902, 0.0);
    /// `ground.soil.clay_red`
    pub const GROUND_SOIL_CLAY_RED: Surface = Surface::hex("090000c9a8502e8a7a6ac47a50cc080d00240059000000360000000000000000", 0.8, 0.0);
    /// `ground.soil.ash_volcanic`
    pub const GROUND_SOIL_ASH_VOLCANIC: Surface = Surface::hex("090002c91c1c1e3a3a3e58585cf203590024404d0000006b0000000000000000", 0.949, 0.0);
    /// `bark.oak`
    pub const BARK_OAK: Surface = Surface::hex("050001cd5a52481e1a168a8a70e6cc1f2fb60000000000600000000000000000", 0.902, 0.0);
    /// `glass.window.clear`
    pub const GLASS_WINDOW_CLEAR: Surface = Surface::hex("1800011de4f0f83a3c400000001a000d218000000000004c0000000000000000", 0.05, 0.0);
    /// `metal.aluminium`
    pub const METAL_ALUMINIUM: Surface = Surface::hex("010005bfe8eaecf6f8fa9aa0a480ff0098404d00240000770000000000000000", 0.502, 1.0);
    /// `glow.bulb.white`
    pub const GLOW_BULB_WHITE: Surface = Surface::hex("1d000004fff4d0ffffff000000801a00001a0000000000720000000000000000", 0.5, 0.0);
    /// `glow.lamp.warm`
    pub const GLOW_LAMP_WARM: Surface = Surface::hex("1d000104ffb060ffffff000000400d0d001a0000000000170000000000000000", 0.5, 0.0);
    /// `brick.red.common` (Asset Studio column 37, the townhouses' brick)
    pub const BRICK_RED_COMMON: Surface = Surface::hex("070003dda85a42b8b2a48a4a382f0c2a80d99999000000d60000000000000000", 0.851, 0.0);
    /// `leaf.oak`
    /// Jersey knit (a heather of two greys): the NPC line-up's cotton.
    pub const CLOTH_KNIT_JERSEY: Surface = Surface::hex("0a02079f686a74a8aab48a3a2a2c26cc26e60000000000db0000000000000000", 0.902, 0.0);
    pub const LEAF_OAK: Surface =Surface::hex("1400034d3f6a2a5e8a34a8c86059598c4d0d09004d00002e0000000000000000", 0.5, 0.0);
}

// ---- uv layout --------------------------------------------------------------

/// A part's mesh with its uvs and tangents, vertices split where the layout
/// needs a seam (box faces of different axes, the wrap of a round part).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UvMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub tangents: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// Lay out `m`'s uv as `s` says. Deterministic; every triangle keeps its
/// winding and every vertex its normal.
pub fn layout(m: &Mesh, s: &Surface) -> UvMesh {
    layout_given(m, s, &[])
}

/// [`layout`], or, when `given` holds one uv per vertex (in metres, as the
/// part's maker laid them: a leaf card's corners, a branch's bark along its
/// length), those over [`Surface::scale`], with no seam split.
pub fn layout_given(m: &Mesh, s: &Surface, given: &[[f32; 2]]) -> UvMesh {
    use std::collections::HashMap;
    if !given.is_empty() && given.len() == m.vertices.len() {
        let k = 1.0 / s.scale.max(1e-3);
        let mut out = UvMesh {
            positions: m.vertices.iter().map(|v| [v[0], v[1], v[2]]).collect(),
            normals: m.vertices.iter().map(|v| [v[3], v[4], v[5]]).collect(),
            uvs: given.iter().map(|u| [u[0] * k, u[1] * k]).collect(),
            tangents: Vec::new(),
            indices: m.indices.clone(),
        };
        out.tangents = tangents(&out);
        return out;
    }
    let k = 1.0 / s.scale.max(1e-3);
    let (lo, hi) = if m.vertices.is_empty() { ([0.0; 3], [0.0; 3]) } else { crate::geo::bounds(m) };
    let c = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5];
    // Round parts: the arc-length radius (the widest horizontal reach).
    let r_wrap = m.vertices.iter().map(|v| ((v[0] - c[0]).powi(2) + (v[2] - c[2]).powi(2)).sqrt()).fold(0.0f32, f32::max).max(1e-3);
    let r_ball = m.vertices.iter().map(|v| ((v[0] - c[0]).powi(2) + (v[1] - c[1]).powi(2) + (v[2] - c[2]).powi(2)).sqrt()).fold(0.0f32, f32::max).max(1e-3);
    let tau = std::f32::consts::TAU;
    let box_uv = |p: [f32; 3], axis: usize, sign: f32| -> [f32; 2] {
        match axis {
            1 => [p[0] * k, -p[2] * k * sign],
            0 => [-p[2] * k * sign, -p[1] * k],
            _ => [p[0] * k * sign, -p[1] * k],
        }
    };
    let mut out = UvMesh::default();
    // (vertex, key) -> new index; the key names the projection (box axis and
    // sign, or the wrap's period shift), so a seam splits a vertex.
    let mut seen: HashMap<(u32, i32), u32> = HashMap::new();
    for tri in m.indices.as_chunks::<3>().0 {
        let p: [[f32; 3]; 3] = std::array::from_fn(|i| {
            let v = m.vertices[tri[i] as usize];
            [v[0], v[1], v[2]]
        });
        let fnorm = cross(sub(p[1], p[0]), sub(p[2], p[0]));
        let a = fnorm.map(f32::abs);
        let axis = if a[1] >= a[0] && a[1] >= a[2] { 1 } else if a[0] >= a[2] { 0 } else { 2 };
        let sign = if fnorm[axis] >= 0.0 { 1.0 } else { -1.0 };
        let flat_cap = a[1] > 0.7 * (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
        // Per corner uv and the seam key.
        let mut uv = [[0.0f32; 2]; 3];
        let mut key = [0i32; 3];
        match s.uv {
            Uv::Box => {
                for i in 0..3 {
                    uv[i] = box_uv(p[i], axis, sign);
                    key[i] = (axis as i32 + 1) * if sign > 0.0 { 1 } else { -1 };
                }
            }
            Uv::Cylinder | Uv::Sphere if s.uv == Uv::Cylinder && flat_cap => {
                for i in 0..3 {
                    uv[i] = box_uv(p[i], 1, sign);
                    key[i] = if sign > 0.0 { 100 } else { -100 };
                }
            }
            Uv::Cylinder | Uv::Sphere => {
                let ang = |q: [f32; 3]| (q[2] - c[2]).atan2(q[0] - c[0]) / tau + 0.5;
                let mut u: [f32; 3] = std::array::from_fn(|i| ang(p[i]));
                // A triangle across the wrap: lift the low corners a period.
                let (umin, umax) = (u.iter().cloned().fold(f32::MAX, f32::min), u.iter().cloned().fold(f32::MIN, f32::max));
                let mut shift = [0i32; 3];
                if umax - umin > 0.5 {
                    for i in 0..3 {
                        if u[i] < 0.5 {
                            u[i] += 1.0;
                            shift[i] = 1;
                        }
                    }
                }
                for i in 0..3 {
                    let (uu, vv) = if s.uv == Uv::Cylinder {
                        (u[i] * tau * r_wrap * k, -p[i][1] * k)
                    } else {
                        let d = sub(p[i], c);
                        let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-6);
                        (u[i] * tau * r_ball * k, (d[1] / l).clamp(-1.0, 1.0).acos() * r_ball * k)
                    };
                    uv[i] = [uu, vv];
                    key[i] = 1000 + shift[i];
                }
            }
        }
        for i in 0..3 {
            let vi = tri[i];
            let at = *seen.entry((vi, key[i])).or_insert_with(|| {
                let v = m.vertices[vi as usize];
                out.positions.push([v[0], v[1], v[2]]);
                out.normals.push([v[3], v[4], v[5]]);
                out.uvs.push(uv[i]);
                (out.positions.len() - 1) as u32
            });
            out.indices.push(at);
        }
    }
    out.tangents = tangents(&out);
    out
}

/// Tangents along u from each triangle's uv slope, summed per vertex and
/// made perpendicular to the normal (w = 1).
fn tangents(m: &UvMesh) -> Vec<[f32; 4]> {
    let mut t = vec![[0.0f32; 3]; m.positions.len()];
    for tri in m.indices.as_chunks::<3>().0 {
        let [a, b, c] = [tri[0] as usize, tri[1] as usize, tri[2] as usize];
        let (e1, e2) = (sub(m.positions[b], m.positions[a]), sub(m.positions[c], m.positions[a]));
        let (du1, dv1) = (m.uvs[b][0] - m.uvs[a][0], m.uvs[b][1] - m.uvs[a][1]);
        let (du2, dv2) = (m.uvs[c][0] - m.uvs[a][0], m.uvs[c][1] - m.uvs[a][1]);
        let det = du1 * dv2 - du2 * dv1;
        if det.abs() < 1e-12 {
            continue;
        }
        let dir: [f32; 3] = std::array::from_fn(|k| (e1[k] * dv2 - e2[k] * dv1) / det);
        for i in [a, b, c] {
            for k in 0..3 {
                t[i][k] += dir[k];
            }
        }
    }
    t.iter()
        .zip(&m.normals)
        .map(|(t, n)| {
            let d = t[0] * n[0] + t[1] * n[1] + t[2] * n[2];
            let f = [t[0] - n[0] * d, t[1] - n[1] * d, t[2] - n[2] * d];
            let l = (f[0] * f[0] + f[1] * f[1] + f[2] * f[2]).sqrt();
            if l > 1e-8 {
                [f[0] / l, f[1] / l, f[2] / l, 1.0]
            } else {
                let g = if n[1].abs() < 0.9 { cross([0.0, 1.0, 0.0], *n) } else { [1.0, 0.0, 0.0] };
                let gl = (g[0] * g[0] + g[1] * g[1] + g[2] * g[2]).sqrt().max(1e-8);
                [g[0] / gl, g[1] / gl, g[2] / gl, 1.0]
            }
        })
        .collect()
}

// ---- the in-memory model ----------------------------------------------------

fn f32s(out: &mut Vec<u8>, xs: impl Iterator<Item = f32>) {
    for x in xs {
        out.extend_from_slice(&(if x.is_finite() { x } else { 0.0 }).to_le_bytes());
    }
}

fn num(x: f32) -> String {
    let x = if x.is_finite() { x } else { 0.0 };
    format!("{x}")
}

/// A `.glb` of the given surfaced meshes: one node, one mesh, one primitive
/// and one material per part (positions, normals, uvs, tangents, u32
/// indices; the material's PBR factors, colour A as its base colour and the
/// block in `extras.evani_recipe`). `None` when no part has a triangle.
pub fn glb(parts: &[(&Mesh, &Surface)]) -> Option<Vec<u8>> {
    let given: Vec<(&Mesh, &Surface, &[[f32; 2]])> = parts.iter().map(|(m, s)| (*m, *s, &[][..])).collect();
    glb_given(&given)
}

/// [`glb`] with each part's own uvs where it has them ([`layout_given`]);
/// a card surface ([`Surface::card`]) is written double-sided and
/// alpha-masked.
pub fn glb_given(parts: &[(&Mesh, &Surface, &[[f32; 2]])]) -> Option<Vec<u8>> {
    let mut bin: Vec<u8> = Vec::new();
    let (mut views, mut accessors, mut prims, mut mats) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (mesh, s, given) in parts {
        let m = layout_given(mesh, s, given);
        let n = m.positions.len();
        if n == 0 || m.indices.is_empty() {
            continue;
        }
        let at = accessors.len();
        let mut view = |bytes: Vec<u8>, bin: &mut Vec<u8>| {
            views.push(format!(r#"{{"buffer":0,"byteOffset":{},"byteLength":{}}}"#, bin.len(), bytes.len()));
            bin.extend_from_slice(&bytes);
            views.len() - 1
        };
        let (lo, hi) = m.positions.iter().fold(([f32::MAX; 3], [f32::MIN; 3]), |(l, h), p| (std::array::from_fn(|k| l[k].min(p[k])), std::array::from_fn(|k| h[k].max(p[k]))));
        let mut b = Vec::with_capacity(n * 12);
        f32s(&mut b, m.positions.iter().flatten().copied());
        let v = view(b, &mut bin);
        accessors.push(format!(
            r#"{{"bufferView":{v},"componentType":5126,"count":{n},"type":"VEC3","min":[{},{},{}],"max":[{},{},{}]}}"#,
            num(lo[0]),
            num(lo[1]),
            num(lo[2]),
            num(hi[0]),
            num(hi[1]),
            num(hi[2])
        ));
        let mut b = Vec::with_capacity(n * 12);
        f32s(&mut b, m.normals.iter().flatten().copied());
        let v = view(b, &mut bin);
        accessors.push(format!(r#"{{"bufferView":{v},"componentType":5126,"count":{n},"type":"VEC3"}}"#));
        let mut b = Vec::with_capacity(n * 8);
        f32s(&mut b, m.uvs.iter().flatten().copied());
        let v = view(b, &mut bin);
        accessors.push(format!(r#"{{"bufferView":{v},"componentType":5126,"count":{n},"type":"VEC2"}}"#));
        let mut b = Vec::with_capacity(n * 16);
        f32s(&mut b, m.tangents.iter().flatten().copied());
        let v = view(b, &mut bin);
        accessors.push(format!(r#"{{"bufferView":{v},"componentType":5126,"count":{n},"type":"VEC4"}}"#));
        let b: Vec<u8> = m.indices.iter().flat_map(|i| i.to_le_bytes()).collect();
        let v = view(b, &mut bin);
        accessors.push(format!(r#"{{"bufferView":{v},"componentType":5125,"count":{},"type":"SCALAR"}}"#, m.indices.len()));
        let k = mats.len();
        prims.push(format!(r#"{{"attributes":{{"POSITION":{at},"NORMAL":{},"TEXCOORD_0":{},"TANGENT":{}}},"indices":{},"material":{k},"mode":4}}"#, at + 1, at + 2, at + 3, at + 4));
        let a = s.base_rgb();
        let card = if s.card {
            r#","alphaMode":"MASK","doubleSided":true"#
        } else if s.blend {
            r#","alphaMode":"BLEND""#
        } else {
            ""
        };
        mats.push(format!(
            r#"{{"name":"p{k}","pbrMetallicRoughness":{{"baseColorFactor":[{},{},{},1],"metallicFactor":{},"roughnessFactor":{}}}{card},"extras":{{"evani_recipe":"{}"}}}}"#,
            num(a[0]),
            num(a[1]),
            num(a[2]),
            num(s.metallic),
            num(s.roughness),
            s.block_hex()
        ));
    }
    if prims.is_empty() {
        return None;
    }
    let json = format!(
        r#"{{"asset":{{"version":"2.0","generator":"Evani.Dev procedural"}},"scene":0,"scenes":[{{"nodes":[0]}}],"nodes":[{{"mesh":0,"name":"procedural"}}],"meshes":[{{"primitives":[{}]}}],"materials":[{}],"accessors":[{}],"bufferViews":[{}],"buffers":[{{"byteLength":{}}}]}}"#,
        prims.join(","),
        mats.join(","),
        accessors.join(","),
        views.join(","),
        bin.len()
    );
    let mut j = json.into_bytes();
    while !j.len().is_multiple_of(4) {
        j.push(b' ');
    }
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let total = 12 + 8 + j.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(j.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&j);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    Some(out)
}

// ---- finishes: one slider over a few everyday materials ---------------------

/// The everyday finishes a made object can take (`finish` params): cast
/// concrete, polished concrete, plaster, granite, oak, brushed steel and
/// sandstone (new finishes go on the end, so a saved index keeps its
/// meaning).
pub const FINISHES: [(&str, Surface); 7] = [
    ("cast concrete", presets::CONCRETE_SMOOTH),
    ("polished concrete", presets::CONCRETE_POLISHED),
    ("plaster", presets::PLASTER_WHITE),
    ("granite", presets::ROCK_GRANITE_GREY),
    ("oak", presets::TIMBER_OAK),
    ("brushed steel", presets::METAL_STEEL_BRUSHED),
    ("sandstone", presets::ROCK_SANDSTONE_BUFF),
];

/// A `finish` param's description (the slider's index).
pub const FINISH_HELP: &str = "Finish: 0 cast concrete, 1 polished concrete, 2 plaster, 3 granite, 4 oak, 5 brushed steel, 6 sandstone";

/// The finish at slider value `x` (rounded, clamped).
pub fn finish(x: f32) -> Surface {
    FINISHES[(x.round().max(0.0) as usize).min(FINISHES.len() - 1)].1
}

/// The finish at slider value `x`, coloured by a colour param. An asset
/// gives its own look for its default finish (`default_finish`, made by
/// `look`, e.g. a darker, crack-free concrete) and its colour param's
/// default (`default`): at the default colour the default finish draws as
/// the asset tuned it and any other finish keeps its catalogue colours; a
/// changed colour recolours whichever finish is on.
pub fn finish_tinted(x: f32, colour: [f32; 3], default: [f32; 3], default_finish: f32, look: impl Fn(Surface) -> Surface) -> Surface {
    let at_default = colour.iter().zip(default).all(|(a, b)| (a - b).abs() < 0.01);
    let is_default = (x.round() - default_finish.round()).abs() < 0.5;
    let s = if is_default { look(finish(x)) } else { finish(x) };
    if at_default { s } else { s.tint(colour) }
}

/// Cast concrete as a made block shows it: mid grey, no map cracks, fine
/// dark pores rather than light aggregate (the catalogue row's numbers 3
/// and 5: crack amount and aggregate).
pub fn cast_concrete(s: Surface) -> Surface {
    // Round 9 (Chris's concrete spec): the layered concrete surface
    // (plaster variant 5, rc_plaster_concrete.wgsl) as "standard structural,
    // slightly aged".
    concrete_preset(s, 1)
}

/// The concrete presets (round 9), in [`concrete_preset`]'s order: the live
/// `concrete` param's names.
pub const CONCRETE_PRESETS: [&str; 5] = ["fresh poured", "standard structural, slightly aged", "aged, exposed", "rough industrial", "polished"];

/// A `concrete` param's description.
pub const CONCRETE_HELP: &str = "Concrete: 0 fresh poured, 1 standard structural slightly aged, 2 aged exposed, 3 rough industrial, 4 polished";

/// Concrete preset `k` (0..4, [`CONCRETE_PRESETS`]) on `s`: the layered
/// concrete surface's numbers (rc_plaster_concrete.wgsl: roughness 0, macro tone 1,
/// aggregate showing 2, pores 5, weathering 6, large blowholes 7) and its
/// warm-neutral grey. No cracks in the material (they are geometry).
pub fn concrete_preset(s: Surface, k: usize) -> Surface {
    // (colour, roughness, macro, aggregate, pores, weathering, blowholes)
    const P: [([f32; 3], [f32; 6]); 5] = [
        ([0.38, 0.37, 0.35], [0.86, 0.35, 0.05, 0.4, 0.0, 0.1]),
        ([0.34, 0.33, 0.31], [0.89, 0.6, 0.15, 0.8, 0.3, 0.2]),
        ([0.3, 0.29, 0.265], [0.91, 0.75, 0.45, 0.65, 0.7, 0.25]),
        ([0.31, 0.305, 0.29], [0.94, 0.65, 0.6, 0.8, 0.5, 0.4]),
        ([0.3, 0.3, 0.29], [0.45, 0.5, 0.5, 0.15, 0.1, 0.05]),
    ];
    let (c, n) = P[k.min(4)];
    s.tint(c).variant(5).extras(0).number(0, n[0]).number(1, n[1]).number(2, n[2]).number(3, 0.0).number(5, n[3]).number(6, n[4]).number(7, n[5]).condition("dust", 0.0)
}

/// The diagnostic view of a concrete surface (0 off, then base colour,
/// normal, roughness, porosity, edge wear and cavity, height).
pub fn concrete_view(s: Surface, view: f32) -> Surface {
    s.number(3, (view.round().clamp(0.0, 6.0)) / 6.0)
}

/// Cast concrete's albedo (linear): a real concrete's grey. Round 8: rounds
/// 4 to 7 darkened every albedo about threefold to make up for the old
/// light (the film rig); the studio light and the neutral tone curve show
/// true albedos, so they are back near the catalogue's (0.39 for concrete).
pub const CAST_CONCRETE: [f32; 3] = [0.34, 0.33, 0.305];
/// Buff sandstone as a dressed block shows it (linear).
pub const SANDSTONE: [f32; 3] = [0.55, 0.43, 0.27];
/// Solid light oak (linear): a warm honey, as the reference oak (the oak's
/// live session: more orange, measured against its lit and shaded faces).
pub const OAK: [f32; 3] = [0.47, 0.245, 0.088];

/// Light oak as a solid block shows it: solid wood (variant 15, round 8: the
/// rings of a log's volume, so the faces, bevels and ends show one piece of
/// wood with no seam). The oak's live session (rc_wood_solid): rings about
/// 11 mm apart (number 1: about 91 a metre), each year its own width, round a pith in the
/// piece's lower middle (numbers 5 and 9: y 0.104 m, z -0.096 m from its
/// base centre, as the reference block's end grain), wandering (2), each opening
/// with a dark band of pores (3) a little sunk (6), pin knots (4), rays (7),
/// long tone streaks (8) and an oiled finish (0).
pub fn solid_oak(s: Surface) -> Surface {
    s.tint(OAK)
        .variant(15)
        .extras(0)
        .number(0, 0.5)
        .number(1, 0.22)
        .number(2, 0.45)
        .number(3, 0.32)
        .number(4, 0.6)
        .number(5, 0.552)
        .number(6, 0.5)
        .number(7, 0.25)
        .number(8, 0.6)
        .number(9, 0.452)
}

/// The same oak where its end grain shows: the same solid wood (variant 16
/// draws as 15; the volume shows the end grain itself), so a part split at
/// the ends meets the rest with no seam.
pub fn oak_end_grain(s: Surface) -> Surface {
    solid_oak(s).variant(16)
}

/// `s` recoloured by a colour param unless the param is at its default.
pub fn tinted_unless_default(s: Surface, colour: [f32; 3], default: [f32; 3]) -> Surface {
    if colour.iter().zip(default).all(|(a, b)| (a - b).abs() < 0.01) { s } else { s.tint(colour) }
}
