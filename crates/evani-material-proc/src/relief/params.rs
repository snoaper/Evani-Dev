//! The relief's live params, `material.relief.*` (owner `material-proc`):
//!
//! * `height_strength`, `bake_size`
//! * layers: `wetness`, `puddles`, `puddle_scale`, `moss`, `moss_color`,
//!   `moss_scale`
//! * `brick.length`, `.height`, `.mortar`, `.bevel`, `.variation`, `.color`
//! * `cobble.size`, `.gap`, `.variation`, `.color`
//! * `tiles.size`, `.grout`, `.gloss`, `.variation`, `.color`
//! * `wood.plank_width`, `.plank_length`, `.rings`, `.variation`, `.color`
//!
//! Colours, strength and layers only rewrite a few records; a pattern
//! param (or `bake_size`) rebakes that pattern (a few milliseconds a
//! surface in a release build).

use evani_core::params::{F32Param, I32Param, ParamDef, ParamError, ParamRegistry, Vec3Param};

use super::pattern::{Brick, Cobble, Planks, Tiles};
use super::{Layers, ReliefConfig};
use crate::params::OWNER;

/// The params' prefix.
pub const PREFIX: &str = "material.relief";

/// Every relief param handle.
#[derive(Debug, Clone)]
pub struct ReliefParams {
    height_strength: F32Param,
    bake_size: I32Param,
    wetness: F32Param,
    puddles: F32Param,
    puddle_scale: F32Param,
    moss: F32Param,
    moss_color: Vec3Param,
    moss_scale: F32Param,
    /// length, height, mortar, bevel, variation
    brick: [F32Param; 5],
    /// size, gap, variation
    cobble: [F32Param; 3],
    /// size, grout, gloss, variation
    tiles: [F32Param; 4],
    /// plank_width, plank_length, rings, variation
    wood: [F32Param; 4],
    /// brick, cobble, tiles, wood
    colors: [Vec3Param; 4],
}

fn f(
    reg: &mut ParamRegistry,
    name: &str,
    v: f32,
    range: (f32, f32),
    step: f32,
    unit: &str,
    what: &str,
) -> Result<F32Param, ParamError> {
    reg.register_f32(
        ParamDef::f32(&format!("{PREFIX}.{name}"), v)
            .range(range.0, range.1)
            .step(step)
            .unit(unit)
            .describe(what)
            .owner(OWNER),
    )
}

fn col(reg: &mut ParamRegistry, name: &str, v: [f32; 3], what: &str) -> Result<Vec3Param, ParamError> {
    reg.register_vec3(
        ParamDef::color(&format!("{PREFIX}.{name}"), v)
            .describe(what)
            .owner(OWNER),
    )
}

impl ReliefParams {
    /// Register them with `cfg`'s values as defaults.
    pub fn register(reg: &mut ParamRegistry, cfg: &ReliefConfig) -> Result<Self, ParamError> {
        let (b, c, t, w, l) = (cfg.brick, cfg.cobble, cfg.tiles, cfg.planks, cfg.layers);
        let var = (0.0, 2.0);
        Ok(ReliefParams {
            height_strength: f(
                reg,
                "height_strength",
                cfg.height_strength,
                (0.0, 3.0),
                0.05,
                "",
                "relief: how strongly the baked height bends the light (0 flat)",
            )?,
            bake_size: reg.register_i32(
                ParamDef::i32(&format!("{PREFIX}.bake_size"), cfg.bake_size as i32)
                    .range(16.0, 256.0)
                    .step(16.0)
                    .unit("texels")
                    .describe("relief: texels a side of each baked tile (a power of two; low tier caps at 128)")
                    .owner(OWNER),
            )?,
            wetness: f(reg, "wetness", l.wetness, (0.0, 1.0), 0.05, "", "relief: wet surfaces (darker, glossy, water in the joints)")?,
            puddles: f(reg, "puddles", l.puddles, (0.0, 1.0), 0.05, "", "relief: puddles in the low spots of upward faces")?,
            puddle_scale: f(reg, "puddle_scale", l.puddle_scale, (0.3, 10.0), 0.1, "m", "relief: size of the puddle pattern")?,
            moss: f(reg, "moss", l.moss, (0.0, 1.0), 0.05, "", "relief: moss in the joints and on top faces")?,
            moss_color: col(reg, "moss_color", l.moss_color, "relief: moss colour (linear)")?,
            moss_scale: f(reg, "moss_scale", l.moss_scale, (0.05, 5.0), 0.05, "m", "relief: size of the moss patches")?,
            brick: [
                f(reg, "brick.length", b.length, (0.08, 0.6), 0.005, "m", "brick: length")?,
                f(reg, "brick.height", b.height, (0.03, 0.3), 0.005, "m", "brick: height")?,
                f(reg, "brick.mortar", b.mortar, (0.0, 0.04), 0.001, "m", "brick: mortar joint width")?,
                f(reg, "brick.bevel", b.bevel, (0.0005, 0.03), 0.0005, "m", "brick: rounded arris width")?,
                f(reg, "brick.variation", b.variation, var, 0.05, "", "brick: per-brick tint and height variation")?,
            ],
            cobble: [
                f(reg, "cobble.size", c.size, (0.04, 0.6), 0.005, "m", "cobble: mean stone size")?,
                f(reg, "cobble.gap", c.gap, (0.0, 0.06), 0.001, "m", "cobble: joint between stones")?,
                f(reg, "cobble.variation", c.variation, var, 0.05, "", "cobble: per-stone tint and height variation")?,
            ],
            tiles: [
                f(reg, "tiles.size", t.size, (0.05, 1.0), 0.01, "m", "tiles: tile size")?,
                f(reg, "tiles.grout", t.grout, (0.0, 0.03), 0.0005, "m", "tiles: grout width")?,
                f(reg, "tiles.gloss", t.gloss, (0.0, 1.0), 0.05, "", "tiles: glaze gloss (0 matte, 1 mirror)")?,
                f(reg, "tiles.variation", t.variation, var, 0.05, "", "tiles: per-tile tint and tilt")?,
            ],
            wood: [
                f(reg, "wood.plank_width", w.width, (0.05, 0.5), 0.005, "m", "wood: plank width")?,
                f(reg, "wood.plank_length", w.length, (0.3, 4.0), 0.05, "m", "wood: plank length")?,
                f(reg, "wood.rings", w.rings, (5.0, 200.0), 1.0, "1/m", "wood: growth rings per metre")?,
                f(reg, "wood.variation", w.variation, var, 0.05, "", "wood: per-plank tint and cupping")?,
            ],
            colors: [
                col(reg, "brick.color", cfg.colors[0], "brick: base colour (linear)")?,
                col(reg, "cobble.color", cfg.colors[1], "cobble: base colour (linear)")?,
                col(reg, "tiles.color", cfg.colors[2], "tiles: glaze colour (linear)")?,
                col(reg, "wood.color", cfg.colors[3], "wood: base colour (linear)")?,
            ],
        })
    }

    /// The config the params hold (`free_surface` from `base`).
    pub fn read(&self, reg: &ParamRegistry, base: &ReliefConfig) -> ReliefConfig {
        let g = |h: F32Param| reg.f32(h);
        ReliefConfig {
            brick: Brick {
                length: g(self.brick[0]),
                height: g(self.brick[1]),
                mortar: g(self.brick[2]),
                bevel: g(self.brick[3]),
                variation: g(self.brick[4]),
            },
            cobble: Cobble {
                size: g(self.cobble[0]),
                gap: g(self.cobble[1]),
                variation: g(self.cobble[2]),
            },
            tiles: Tiles {
                size: g(self.tiles[0]),
                grout: g(self.tiles[1]),
                gloss: g(self.tiles[2]),
                variation: g(self.tiles[3]),
            },
            planks: Planks {
                width: g(self.wood[0]),
                length: g(self.wood[1]),
                rings: g(self.wood[2]),
                variation: g(self.wood[3]),
            },
            colors: self.colors.map(|h| reg.vec3(h)),
            height_strength: g(self.height_strength),
            layers: Layers {
                wetness: g(self.wetness),
                puddles: g(self.puddles),
                puddle_scale: g(self.puddle_scale),
                moss: g(self.moss),
                moss_color: reg.vec3(self.moss_color),
                moss_scale: g(self.moss_scale),
            },
            bake_size: reg.i32(self.bake_size).clamp(16, 256) as u32,
            free_surface: base.free_surface,
            debug: base.debug,
        }
    }
}
