//! The recipe stage's live switches (batch 1 build plan 2.5 and 2.6, package
//! P2b; feature `engine`): `material.recipe.enabled`, the eight
//! `material.quality.<group>` levels, and one `material.recipe.<recipe>.<extra>`
//! allow switch per extra of each compiled recipe. They all land in the shared
//! per-frame uniform (binding 13, [`frame_words`]), so a slider never rebuilds a
//! pipeline.
//!
//! Tiers carry no new field (DECISIONS "Lean data"): the tier table is here, in
//! code ([`level_for_tier`]), like `material.layers.quality` and
//! `material.character.quality`. A group a family owns also drives that
//! family's own quality param by name ([`MIRROR`], [`mirror`]), so neither
//! crate depends on the other; a name that isn't registered is skipped.

use evani_core::params::{BoolParam, EnumParam, ParamDef, ParamError, ParamRegistry, ParamValue};
use evani_core::ecs::{Stage, SystemDesc};
use evani_core::plugin::PluginContext;

use crate::conditions::{CondParams, MaskRule};
use crate::shader::{COMPILED, FRAME_BYTES, GROUPS, extra_base, extra_name, stage_recipe};

/// `material.recipe.enabled`: the recipe stage on or off (off draws every
/// block material through the plain lit pass).
pub const ENABLED: &str = "material.recipe.enabled";

/// `material.recipe.paint.coat_strength` (P10a, registered only with
/// `recipe-paint`): times every paint block's coat number (0 no coat, 1 as
/// the catalogue has it, 2 double); frame word 11 carries it minus 1, so an
/// all-zero uniform draws the coat as authored.
pub const COAT_STRENGTH: &str = "material.recipe.paint.coat_strength";

/// `material.recipe.cloth.sheen_strength` (B4-P3b, registered only with
/// `recipe-cloth`): times every cloth block's sheen (0 none, 1 as the
/// catalogue has it, 2 double), as `material.layers.sheen_strength` did for
/// cloth before. Bits 24 to 31 of frame word 10 carry it in hundredths,
/// offset so an all-zero uniform draws the sheen as authored
/// ([`cloth_sheen_bits`]).
pub const CLOTH_SHEEN: &str = "material.recipe.cloth.sheen_strength";

/// `material.recipe.cut.enabled` (B3-P2, registered only with `cutout`): the
/// cut-out's holes on or off (off draws grates, fences and lattice solid, in
/// their shadows too); off bit 4 of frame word 10.
pub const CUT_ENABLED: &str = "material.recipe.cut.enabled";

/// `material.recipe.cut.min_px` (B3-P2, with `cutout`): every wire, bar and
/// slat is drawn at least this many pixels wide (0, the default, draws each
/// at its true width, so a far fence fades to its real open share); bits 8
/// to 15 of frame word 10 carry it in steps of 1/85 px (word 31 is the style
/// step's).
pub const CUT_MIN_PX: &str = "material.recipe.cut.min_px";

/// `material.recipe.blend.enabled` (B3-P3, registered only with `blend`):
/// glass, smoke, steam, fire and holograms drawn see-through (off draws them
/// opaque, the light version: a tinted gloss, a glowing card). Read on the
/// CPU by evani-render, which picks the pipeline.
pub const BLEND_ENABLED: &str = "material.recipe.blend.enabled";

/// `material.recipe.blend.sort` (B3-P3, with `blend`): see-through draws far
/// to near (off: in model order, to see what the sort buys).
pub const BLEND_SORT: &str = "material.recipe.blend.sort";

/// `material.recipe.blend.refraction` (B3-P3, with `refract`): how far
/// frosted, wavy and block glass bend what is behind (0 skips the copy and
/// draws them as clear over; 1 as authored); bits 16 to 23 of frame word 10,
/// in percent.
pub const BLEND_REFRACTION: &str = "material.recipe.blend.refraction";

/// The eight quality params, in [`GROUPS`] order.
pub const QUALITY: [&str; 8] = [
    "material.quality.detail",
    "material.quality.fur",
    "material.quality.water",
    "material.quality.glass",
    "material.quality.skin",
    "material.quality.foliage",
    "material.quality.conditions",
    "material.quality.reflections",
];

/// The options of every `material.quality.<group>` param.
pub const LEVELS: [&str; 6] = ["auto", "lite", "low", "medium", "high", "ultra"];

/// How a quality group drives a family's own param.
#[derive(Debug, Clone, Copy)]
pub enum MirrorValue {
    /// An f32 param: the value for auto, lite, low, medium, high, ultra.
    F32([f32; 6]),
    /// An enum param: the option index for auto, lite, low, medium, high, ultra.
    Enum([u32; 6]),
}

/// One mirror row: a [`GROUPS`] index, the family param's name, its values.
#[derive(Debug, Clone, Copy)]
pub struct Mirror {
    pub group: usize,
    pub name: &'static str,
    pub value: MirrorValue,
}

/// The families' own quality params. `skin` sets
/// `material.character.quality` (-1 by tier, 0 Lite .. 3 High; Ultra is its
/// High); `detail` sets `material.layers.quality` (auto, cheap, full: Lite and
/// Low cheap). The nature family has one quality param for its single pass,
/// `material.nature.quality` (-1 by tier, 0 Lite .. 3 High; Ultra is its High),
/// which the water, glass and foliage groups all drive: the group changed
/// last sets it (the pass has one mode, so there is no per-group level).
pub const MIRROR: [Mirror; 5] = [
    Mirror {
        group: 4,
        name: "material.character.quality",
        value: MirrorValue::F32([-1.0, 0.0, 1.0, 2.0, 3.0, 3.0]),
    },
    Mirror {
        group: 0,
        name: "material.layers.quality",
        value: MirrorValue::Enum([0, 1, 1, 2, 2, 2]),
    },
    Mirror {
        group: 2,
        name: "material.nature.quality",
        value: MirrorValue::F32([-1.0, 0.0, 1.0, 2.0, 3.0, 3.0]),
    },
    Mirror {
        group: 3,
        name: "material.nature.quality",
        value: MirrorValue::F32([-1.0, 0.0, 1.0, 2.0, 3.0, 3.0]),
    },
    Mirror {
        group: 5,
        name: "material.nature.quality",
        value: MirrorValue::F32([-1.0, 0.0, 1.0, 2.0, 3.0, 3.0]),
    },
];

/// The parameter handles, kept as a world resource by [`register`].
#[derive(Debug, Clone)]
pub struct RecipeParams {
    pub enabled: BoolParam,
    pub quality: [EnumParam; 8],
    /// The compiled extras' allow switches: (global extras bit, handle).
    pub extras: Vec<(u32, BoolParam)>,
    /// The compiled condition layers' switches (`material.cond.*`).
    pub cond: CondParams,
    /// Paint's clear coat strength ([`COAT_STRENGTH`]), with `recipe-paint`.
    #[cfg(feature = "recipe-paint")]
    pub coat: evani_core::params::F32Param,
    /// Cloth's sheen strength ([`CLOTH_SHEEN`]), with `recipe-cloth`.
    #[cfg(feature = "recipe-cloth")]
    pub cloth_sheen: evani_core::params::F32Param,
    /// The cut-out's switches ([`CUT_ENABLED`], [`CUT_MIN_PX`]), with `cutout`.
    #[cfg(feature = "cutout")]
    pub cut: (BoolParam, evani_core::params::F32Param),
    /// The see-through pass's switches ([`BLEND_ENABLED`], [`BLEND_SORT`]), with `blend`.
    #[cfg(feature = "blend")]
    pub blend: (BoolParam, BoolParam),
    /// [`BLEND_REFRACTION`], with `refract`.
    #[cfg(feature = "refract")]
    pub refraction: evani_core::params::F32Param,
}

/// The level (0 lite .. 4 ultra) a tier draws: lite, low, medium, high and
/// ultra are their own level, vr is medium (stereo frame cost), anything
/// else medium.
pub fn level_for_tier(tier: &str) -> u32 {
    match tier {
        "lite" => 0,
        "low" => 1,
        "high" => 3,
        "ultra" => 4,
        _ => 2,
    }
}

/// The level of a `material.quality.*` option (0 auto follows the tier).
pub fn level(option: u32, tier: &str) -> u32 {
    match option {
        1..=5 => option - 1,
        _ => level_for_tier(tier),
    }
}

/// The 32 bits of `misc.x`: each group's level in its 4 bits.
pub fn pack_levels(levels: [u32; 8]) -> u32 {
    levels
        .iter()
        .enumerate()
        .fold(0, |acc, (g, l)| acc | ((l & 15) << (g * 4)))
}

/// Register the switches for what is compiled in and keep their handles.
pub fn register(ctx: &mut PluginContext) -> Result<(), ParamError> {
    let enabled = ctx.param_bool(
        ParamDef::bool(ENABLED, true)
            .describe("Catalogue material recipes (off: plain PBR)")
            .gpu(),
    )?;
    let mut quality = Vec::with_capacity(QUALITY.len());
    for (name, group) in QUALITY.iter().zip(GROUPS) {
        let what = format!("{group} material quality (auto: by tier)");
        quality.push(
            ctx.param_enum(
                ParamDef::enumeration(name, &LEVELS, 0)
                    .describe(&what)
                    .gpu(),
            )?,
        );
    }
    let quality: [EnumParam; 8] = quality
        .try_into()
        .map_err(|_| ParamError::BadName("material.quality.*: eight groups".into()))?;
    let mut extras = Vec::new();
    for r in COMPILED {
        let (Some(row), Some(base)) = (stage_recipe(r.code), extra_base(r.code)) else {
            continue;
        };
        for k in 0..u32::from(row.extras) {
            let Some(name) = extra_name(base + k) else {
                continue;
            };
            let what = format!("Allow this {} extra", row.name);
            extras.push((
                base + k,
                ctx.param_bool(ParamDef::bool(&name, true).describe(&what).gpu())?,
            ));
        }
    }
    #[cfg(feature = "conditions")]
    let cond = crate::conditions::register(ctx)?;
    #[cfg(not(feature = "conditions"))]
    let cond = CondParams::default();
    #[cfg(feature = "recipe-paint")]
    let coat = ctx.param_f32(
        ParamDef::f32(COAT_STRENGTH, 1.0)
            .range(0.0, 2.0)
            .step(0.01)
            .unit("x")
            .describe("Clear coat on every catalogue paint, times its own coat (0 = off, 1 = as authored)")
            .gpu(),
    )?;
    #[cfg(feature = "recipe-cloth")]
    let cloth_sheen = ctx.param_f32(
        ParamDef::f32(CLOTH_SHEEN, 1.0)
            .range(0.0, 2.0)
            .step(0.01)
            .unit("x")
            .describe("Sheen on every catalogue cloth, times its own sheen (0 = off, 1 = as authored)")
            .gpu(),
    )?;
    #[cfg(feature = "cutout")]
    let cut = (
        ctx.param_bool(ParamDef::bool(CUT_ENABLED, true).describe("Cut-out holes in grates, fences and lattice (off: solid)").gpu())?,
        ctx.param_f32(
            ParamDef::f32(CUT_MIN_PX, 0.0)
                .range(0.0, 3.0)
                .step(0.05)
                .unit("px")
                .describe("Cut-out wire drawn at least this wide (0 = true width; raise it to keep far fences readable)")
                .gpu(),
        )?,
    );
    #[cfg(feature = "blend")]
    let blend = (
        ctx.param_bool(ParamDef::bool(BLEND_ENABLED, true).describe("See-through glass, smoke, steam, fire and holograms (off: drawn opaque)"))?,
        ctx.param_bool(ParamDef::bool(BLEND_SORT, true).describe("See-through draws sorted far to near"))?,
    );
    #[cfg(feature = "refract")]
    let refraction = ctx.param_f32(
        ParamDef::f32(BLEND_REFRACTION, 1.0)
            .range(0.0, 2.0)
            .step(0.01)
            .unit("x")
            .describe("How far frosted, wavy and block glass bend what is behind (0 = no copy, drawn as clear glass)")
            .gpu(),
    )?;
    ctx.resource(RecipeParams {
        enabled,
        quality,
        extras,
        cond,
        #[cfg(feature = "recipe-paint")]
        coat,
        #[cfg(feature = "recipe-cloth")]
        cloth_sheen,
        #[cfg(feature = "cutout")]
        cut,
        #[cfg(feature = "blend")]
        blend,
        #[cfg(feature = "refract")]
        refraction,
    });
    // The family mirror writes params, so it runs as its own Update system:
    // the renderer only ever holds a shared borrow of the registry.
    let mut seen: Option<u64> = None;
    ctx.system(SystemDesc::new("material.recipe.mirror", Stage::Update, move |world| {
        let Some(q) = world.try_res::<RecipeParams>().map(|p| p.quality) else {
            return;
        };
        let Some(mut reg) = world.try_res_mut::<ParamRegistry>() else {
            return;
        };
        seen = Some(match seen {
            None => reg.revision(),
            Some(since) => mirror(&mut reg, &q, since),
        });
    }));
    Ok(())
}

/// The words of the shared per-frame uniform ([`FRAME_BYTES`]): the eight
/// global condition amounts (zero for a layer not compiled in), the packed
/// levels, sim time, the off bits, the extras deny bits and the condition
/// layers' look numbers.
pub fn frame_words(
    reg: &ParamRegistry,
    p: &RecipeParams,
    tier: &str,
    sim_seconds: f64,
) -> [u32; FRAME_BYTES / 4] {
    let levels = core::array::from_fn(|g| level(reg.enum_index(p.quality[g]), tier));
    let mut w = default_frame(tier, sim_seconds);
    w[8] = pack_levels(levels);
    w[10] = u32::from(!reg.bool(p.enabled));
    for &(bit, h) in &p.extras {
        if !reg.bool(h)
            && let Some(word) = w.get_mut(12 + (bit >> 5) as usize)
        {
            *word |= 1 << (bit & 31);
        }
    }
    #[cfg(feature = "conditions")]
    crate::conditions::fill_frame(reg, &p.cond, &mut w);
    #[cfg(feature = "recipe-paint")]
    {
        w[11] = (reg.f32(p.coat) - 1.0).to_bits();
    }
    #[cfg(feature = "cutout")]
    {
        if !reg.bool(p.cut.0) {
            w[10] |= 4;
        }
        w[10] |= ((reg.f32(p.cut.1).clamp(0.0, 3.0) * 85.0).round() as u32) << 8;
    }
    #[cfg(feature = "refract")]
    {
        w[10] |= refraction_bits(reg.f32(p.refraction));
    }
    #[cfg(feature = "recipe-cloth")]
    {
        w[10] |= cloth_sheen_bits(reg.f32(p.cloth_sheen));
    }
    w
}

/// Cloth's sheen strength in frame word 10's bits 24 to 31: hundredths plus
/// 156, wrapped, so strength 1 is 0 and the shader reads
/// `((bits + 100) & 255) / 100` (0 to 2.55).
#[cfg(feature = "recipe-cloth")]
pub fn cloth_sheen_bits(strength: f32) -> u32 {
    ((((strength.clamp(0.0, 2.0) * 100.0).round() as u32) + 156) & 255) << 24
}

/// The refraction strength's bits in frame word 10 (16 to 23, percent).
#[cfg(feature = "refract")]
pub fn refraction_bits(strength: f32) -> u32 {
    ((strength.clamp(0.0, 2.0) * 100.0).round() as u32) << 16
}

/// The level (0 lite .. 4 ultra) of `material.quality.conditions` on `tier`.
pub fn conditions_level(reg: &ParamRegistry, p: &RecipeParams, tier: &str) -> u32 {
    p.quality.get(6).map_or_else(|| level_for_tier(tier), |&h| level(reg.enum_index(h), tier))
}

/// What decides each material's effective condition mask this frame
/// ([`crate::conditions::apply`]).
pub fn mask_rule(reg: &ParamRegistry, p: &RecipeParams, tier: &str) -> MaskRule {
    MaskRule::from_params(reg, &p.cond, conditions_level(reg, p, tier))
}

/// The frame words with every switch at its default on `tier` (no registry).
pub fn default_frame(tier: &str, sim_seconds: f64) -> [u32; FRAME_BYTES / 4] {
    let mut w = [0u32; FRAME_BYTES / 4];
    w[8] = pack_levels([level_for_tier(tier); 8]);
    w[9] = (sim_seconds as f32).to_bits();
    #[cfg(feature = "refract")]
    {
        w[10] = refraction_bits(1.0);
    }
    #[cfg(feature = "conditions")]
    crate::conditions::default_looks(&mut w);
    w
}

/// Drive the families' quality params from the groups that changed after
/// registry revision `since`; returns the revision to pass next time.
pub fn mirror(reg: &mut ParamRegistry, quality: &[EnumParam; 8], since: u64) -> u64 {
    for m in &MIRROR {
        let Some(&h) = quality.get(m.group) else {
            continue;
        };
        if !reg.changed_since(h.id(), since) || reg.id(m.name).is_none() {
            continue;
        }
        let i = (reg.enum_index(h) as usize).min(LEVELS.len() - 1);
        let v = match m.value {
            MirrorValue::F32(t) => ParamValue::F32(t[i]),
            MirrorValue::Enum(t) => ParamValue::Enum(t[i]),
        };
        let _ = reg.set_by_name(m.name, v);
    }
    reg.revision()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn tiers_map_to_levels() {
        assert_eq!(
            ["lite", "low", "medium", "high", "ultra", "vr", "?"].map(level_for_tier),
            [0, 1, 2, 3, 4, 2, 2]
        );
        assert_eq!(level(0, "high"), 3);
        assert_eq!(level(1, "ultra"), 0);
        assert_eq!(level(5, "lite"), 4);
    }

    #[test]
    fn levels_pack_in_group_order() {
        assert_eq!(pack_levels([0, 1, 2, 3, 4, 0, 0, 4]), 0x4004_3210);
        let w = default_frame("high", 2.5);
        assert_eq!(w[8], 0x3333_3333);
        assert_eq!(f32::from_bits(w[9]), 2.5);
        // Bits 16-23 of word 10 carry the refraction strength (B3-P3).
        assert!(
            w[10] & 0xff00_ffff == 0 && w[11..16].iter().all(|&x| x == 0),
            "nothing off, nothing denied"
        );
        #[cfg(feature = "refract")]
        assert_eq!(w[10] >> 16, 100, "refraction as authored");
        #[cfg(feature = "conditions")]
        assert_eq!(f32::from_bits(w[16]), 0.5, "look numbers at their defaults");
        assert_eq!(w.len() * 4, FRAME_BYTES);
    }

    /// B4-P3b: the cloth sheen strength round-trips through the shader's
    /// decode, and the default (1) is all zero bits, so an untouched uniform
    /// draws the sheen as authored.
    #[cfg(feature = "recipe-cloth")]
    #[test]
    fn cloth_sheen_strength_packs_into_word_ten() {
        let decode = |w: u32| f32::from(u8::try_from(((w >> 24) + 100) & 255).unwrap_or(0)) * 0.01;
        assert_eq!(cloth_sheen_bits(1.0), 0);
        for s in [0.0f32, 0.25, 0.5, 1.0, 1.37, 2.0] {
            assert!((decode(cloth_sheen_bits(s)) - s).abs() < 0.006, "{s}");
        }
        assert_eq!(decode(cloth_sheen_bits(9.0)), 2.0, "clamped");
        assert_eq!(cloth_sheen_bits(0.5) & 0x00ff_ffff, 0, "the other bits of word 10 stay free");
    }
}
