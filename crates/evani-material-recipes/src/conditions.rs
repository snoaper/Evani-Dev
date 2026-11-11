//! The condition layers' live switches and CPU maths (batch 1 build plan 2.4
//! and 2.6, package P3; feature `engine`).
//!
//! Registered only for the layers compiled in ([`crate::shader::LAYERS`],
//! features `cond-<layer>`):
//!
//! - `material.cond.enabled`: every layer on or off;
//! - per layer `material.cond.<layer>.on` (the layer on or off, like a game's
//!   settings menu), `material.cond.<layer>.amount` (the global amount: rain
//!   is a global wet of 0.8) and its look numbers ([`LOOKS`], e.g.
//!   `material.cond.wet.puddles`, `material.cond.mud.color`).
//!
//! The amounts and look numbers land in the shared per-frame uniform
//! (binding 13, [`fill_frame`]). The on switches, the quality limit
//! (`material.quality.conditions`: lite 1 layer, low 2, medium 4, high and
//! ultra 8) and the light version (lite keeps only wet, dust and snow, as a
//! tint) are applied on the CPU: [`effective_mask`] rewrites each material's
//! condition mask to its strongest allowed layers, so the shader never sorts
//! and a layer that is off costs nothing on the GPU.

use evani_core::params::{BoolParam, F32Param, ParamDef, ParamError, ParamRegistry, Vec3Param};
use evani_core::plugin::PluginContext;

use crate::shader::{LAYERS, LAYER_NAMES};

/// Whether any condition layer is compiled in (feature `conditions`, on with
/// any `cond-<layer>`). The GPU side tests it, so without a layer the mask
/// rewrite is unreachable and never reaches the download.
pub const ACTIVE: bool = cfg!(feature = "conditions");

/// `material.cond.enabled`: all condition layers on or off.
pub const ENABLED: &str = "material.cond.enabled";

/// Bits of the layers the light version keeps (wet, dust, snow).
pub const LITE_LAYERS: u8 = 0b0000_1101;

/// How many layers a material draws at each conditions quality level
/// (lite, low, medium, high, ultra).
pub const LIMITS: [u32; 5] = [1, 2, 4, 8, 8];

/// What a look number is in the frame uniform.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LookKind {
    /// A plain number: (default, min, max).
    Num(f32, f32, f32),
    /// An angle in degrees (default, min, max), sent as its cosine.
    Angle(f32, f32, f32),
    /// A linear colour, sent as sRGB8 in one word.
    Colour([f32; 3]),
}

/// One look number of a layer: `material.cond.<layer>.<name>`.
#[derive(Debug, Clone, Copy)]
pub struct Look {
    /// The layer's bit (0 wet .. 7 blood).
    pub bit: u8,
    /// The name after the layer.
    pub name: &'static str,
    /// Its word in the frame uniform (core.wgsl `RecFrame`: 16-19 `ka`,
    /// 20-23 `kb`, 24-27 `kc`, 28-31 `kd`).
    pub word: usize,
    pub kind: LookKind,
    pub unit: &'static str,
    pub what: &'static str,
}

const fn look(bit: u8, name: &'static str, word: usize, kind: LookKind, unit: &'static str, what: &'static str) -> Look {
    Look { bit, name, word, kind, unit, what }
}

/// Every layer's look numbers, in frame word order within each layer.
pub const LOOKS: [Look; 15] = [
    look(0, "puddles", 16, LookKind::Num(0.5, 0.0, 1.0), "", "How much flat ground holds puddles"),
    look(0, "puddle_scale", 17, LookKind::Num(0.6, 0.05, 4.0), "1/m", "Puddle patches per metre"),
    look(1, "ground", 18, LookKind::Num(0.0, -100.0, 100.0), "m", "World height mud rises from"),
    look(1, "height", 19, LookKind::Num(0.6, 0.05, 3.0), "m", "How high mud reaches at full amount"),
    look(1, "color", 28, LookKind::Colour([0.09, 0.055, 0.03]), "", "Mud colour (linear)"),
    look(2, "cavity", 20, LookKind::Num(0.6, 0.0, 1.0), "", "How much dust gathers in cavities"),
    look(2, "color", 29, LookKind::Colour([0.36, 0.3, 0.22]), "", "Dust colour (linear)"),
    look(3, "angle", 21, LookKind::Angle(50.0, 0.0, 90.0), "deg", "Steepest slope snow holds on"),
    look(3, "frost", 22, LookKind::Num(0.3, 0.0, 1.0), "", "Frost on what the snow leaves bare"),
    look(4, "edge", 23, LookKind::Num(0.12, 0.0, 1.0), "m", "How far wear reaches round edges"),
    look(4, "scratches", 24, LookKind::Num(0.4, 0.0, 1.0), "", "How many scratch lines"),
    look(5, "strength", 25, LookKind::Num(0.7, 0.0, 1.0), "", "How fast oxide spreads with the amount"),
    look(6, "color", 30, LookKind::Colour([0.05, 0.11, 0.02]), "", "Moss colour (linear)"),
    look(6, "scale", 26, LookKind::Num(3.0, 0.2, 20.0), "1/m", "Moss patches per metre"),
    look(7, "age", 27, LookKind::Num(0.0, 0.0, 1.0), "", "Blood age: fresh red to dry brown"),
];

/// A registered look number.
#[derive(Debug, Clone, Copy)]
pub enum LookParam {
    Num(F32Param),
    Colour(Vec3Param),
}

/// One compiled layer's handles.
#[derive(Debug, Clone)]
pub struct LayerParams {
    pub bit: u8,
    pub on: BoolParam,
    pub amount: F32Param,
    /// (index into [`LOOKS`], handle).
    pub looks: Vec<(usize, LookParam)>,
}

/// The condition switches, kept inside the recipe params resource.
/// `enabled` is `None` when no layer is compiled in (nothing registered).
#[derive(Debug, Clone, Default)]
pub struct CondParams {
    pub enabled: Option<BoolParam>,
    pub layers: Vec<LayerParams>,
}

/// `material.cond.<layer>.<what>`.
pub fn name(bit: u8, what: &str) -> String {
    let layer = LAYER_NAMES.get(usize::from(bit)).copied().unwrap_or("?");
    format!("material.cond.{layer}.{what}")
}

/// Register the switches of the compiled layers.
pub fn register(ctx: &mut PluginContext) -> Result<CondParams, ParamError> {
    if LAYERS.is_empty() {
        return Ok(CondParams::default());
    }
    let enabled = ctx.param_bool(ParamDef::bool(ENABLED, true).describe("Condition layers (wet, mud, snow...) on").gpu())?;
    let mut layers = Vec::with_capacity(LAYERS.len());
    for l in LAYERS {
        let layer = LAYER_NAMES.get(usize::from(l.bit)).copied().unwrap_or("?");
        let on = ctx.param_bool(ParamDef::bool(&name(l.bit, "on"), true).describe(&format!("The {layer} layer on")).gpu())?;
        let amount = ctx.param_f32(
            ParamDef::f32(&name(l.bit, "amount"), 0.0)
                .range(0.0, 1.0)
                .describe(&format!("Global {layer} amount (a material's own amount wins if larger)"))
                .gpu(),
        )?;
        let mut looks = Vec::new();
        for (k, lk) in LOOKS.iter().enumerate().filter(|(_, lk)| lk.bit == l.bit) {
            let n = name(l.bit, lk.name);
            let p = match lk.kind {
                LookKind::Num(d, lo, hi) | LookKind::Angle(d, lo, hi) => LookParam::Num(
                    ctx.param_f32(ParamDef::f32(&n, d).range(lo, hi).unit(lk.unit).describe(lk.what).gpu())?,
                ),
                LookKind::Colour(c) => LookParam::Colour(ctx.param_vec3(ParamDef::color(&n, c).describe(lk.what).gpu())?),
            };
            looks.push((k, p));
        }
        layers.push(LayerParams { bit: l.bit, on, amount, looks });
    }
    Ok(CondParams { enabled: Some(enabled), layers })
}

/// Linear to sRGB8.
fn srgb8(l: f32) -> u32 {
    let l = l.clamp(0.0, 1.0);
    let s = if l <= 0.003_130_8 { l * 12.92 } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0 + 0.5) as u32
}

/// A linear colour as the sRGB8 word the shader unpacks (`unpack4x8unorm`).
pub fn pack_colour(c: [f32; 3]) -> u32 {
    srgb8(c[0]) | (srgb8(c[1]) << 8) | (srgb8(c[2]) << 16)
}

/// The frame word of a look number's value.
fn look_word(kind: LookKind, v: [f32; 3]) -> u32 {
    match kind {
        LookKind::Num(..) => v[0].to_bits(),
        LookKind::Angle(..) => v[0].to_radians().cos().to_bits(),
        LookKind::Colour(_) => pack_colour(v),
    }
}

/// Every look number at its default (words 16..32 of the frame uniform).
pub fn default_looks(w: &mut [u32]) {
    for lk in &LOOKS {
        let v = match lk.kind {
            LookKind::Num(d, ..) | LookKind::Angle(d, ..) => [d; 3],
            LookKind::Colour(c) => c,
        };
        if let Some(x) = w.get_mut(lk.word) {
            *x = look_word(lk.kind, v);
        }
    }
}

/// The global amounts (words 0..8), the conditions-off bit (word 10, bit 2)
/// and the look numbers of the compiled layers.
pub fn fill_frame(reg: &ParamRegistry, c: &CondParams, w: &mut [u32]) {
    if let (Some(h), Some(x)) = (c.enabled, w.get_mut(10))
        && !reg.bool(h)
    {
        *x |= 2;
    }
    for l in &c.layers {
        if let Some(x) = w.get_mut(usize::from(l.bit)) {
            *x = reg.f32(l.amount).clamp(0.0, 1.0).to_bits();
        }
        for &(k, p) in &l.looks {
            let Some(lk) = LOOKS.get(k) else {
                continue;
            };
            let v = match p {
                LookParam::Num(h) => [reg.f32(h); 3],
                LookParam::Colour(h) => reg.vec3(h),
            };
            if let Some(x) = w.get_mut(lk.word) {
                *x = look_word(lk.kind, v);
            }
        }
    }
}

/// What decides each material's effective condition mask this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MaskRule {
    /// The global amounts, by bit.
    pub global: [f32; 8],
    /// The layers allowed at all: compiled, switched on, and (lite) the light
    /// version's three.
    pub allow: u8,
    /// How many layers a material may draw.
    pub limit: u32,
}

impl MaskRule {
    /// The rule for the layers in `compiled` (bits), switched on as `on`, at
    /// conditions quality `level` (0 lite .. 4 ultra).
    pub fn new(global: [f32; 8], compiled: u8, on: u8, level: u32) -> Self {
        let lite = level == 0;
        let allow = compiled & on & if lite { LITE_LAYERS } else { 0xff };
        let limit = LIMITS.get(level as usize).copied().unwrap_or(8);
        Self { global, allow, limit }
    }

    /// The rule from the registry: compiled layers, their on switches and
    /// global amounts, at `level`. With the layers all off, nothing is allowed.
    pub fn from_params(reg: &ParamRegistry, c: &CondParams, level: u32) -> Self {
        let mut global = [0.0; 8];
        let mut compiled = 0u8;
        let mut on = 0u8;
        for l in &c.layers {
            let bit = l.bit & 7;
            compiled |= 1 << bit;
            if reg.bool(l.on) {
                on |= 1 << bit;
            }
            if let Some(g) = global.get_mut(usize::from(bit)) {
                *g = reg.f32(l.amount).clamp(0.0, 1.0);
            }
        }
        if c.enabled.is_some_and(|h| !reg.bool(h)) {
            on = 0;
        }
        Self::new(global, compiled, on, level)
    }
}

/// A layer's final amount: the larger of the material's own (`own`, u8 unorm)
/// and the global one (the shader's `rc_amount`, before the mask).
pub fn amount(own: u8, global: f32) -> f32 {
    (f32::from(own) / 255.0).max(global.clamp(0.0, 1.0))
}

/// The condition mask a material draws: of the layers in its own `mask` that
/// the rule allows and whose amount is above zero, the `limit` strongest
/// (ties go to the lower bit). `own` is the block's eight default amounts.
pub fn effective_mask(mask: u8, own: [u8; 8], r: &MaskRule) -> u8 {
    let mut cand = [(0.0f32, 0u8); 8];
    let mut n = 0;
    for bit in 0..8u8 {
        let k = usize::from(bit);
        let a = amount(own[k], r.global[k]);
        if ((mask & r.allow) >> bit) & 1 == 1
            && a > 0.0
            && let Some(slot) = cand.get_mut(n)
        {
            *slot = (a, bit);
            n += 1;
        }
    }
    let cand = &mut cand[..n];
    // Strongest first; equal amounts keep bit order (a stable sort).
    cand.sort_by(|x, y| y.0.total_cmp(&x.0));
    cand.iter().take(r.limit as usize).fold(0, |m, &(_, bit)| m | (1 << bit))
}

/// A block's eight words with its condition mask (byte 3) replaced by
/// [`effective_mask`].
pub fn apply(words: &[u32; 8], r: &MaskRule) -> [u32; 8] {
    let mut w = *words;
    let mask = (w[0] >> 24) as u8;
    let own = core::array::from_fn(|k| (w[6 + k / 4] >> (8 * (k % 4))) as u8);
    w[0] = (w[0] & 0x00ff_ffff) | (u32::from(effective_mask(mask, own, r)) << 24);
    w
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    const ALL: u8 = 0xff;

    #[test]
    fn the_amount_is_the_larger_of_own_and_global() {
        assert_eq!(amount(0, 0.8), 0.8);
        assert_eq!(amount(255, 0.2), 1.0);
        assert!((amount(51, 0.1) - 0.2).abs() < 1e-6);
        assert_eq!(amount(0, 2.0), 1.0, "global clamps");
    }

    #[test]
    fn a_layer_not_in_the_mask_stays_off() {
        let r = MaskRule::new([1.0; 8], ALL, ALL, 4);
        // Global amounts are all 1, but only wet and snow are listed.
        assert_eq!(effective_mask(0b1001, [0; 8], &r), 0b1001);
        assert_eq!(effective_mask(0, [255; 8], &r), 0);
        // Not compiled, or switched off: off whatever the amounts.
        let r = MaskRule::new([1.0; 8], 0b0000_0001, ALL, 4);
        assert_eq!(effective_mask(0b1001, [0; 8], &r), 0b0001);
        let r = MaskRule::new([1.0; 8], ALL, !0b1000, 4);
        assert_eq!(effective_mask(0b1001, [0; 8], &r), 0b0001);
        // Listed but no amount anywhere: nothing to draw.
        let r = MaskRule::new([0.0; 8], ALL, ALL, 4);
        assert_eq!(effective_mask(0b1001, [0; 8], &r), 0);
    }

    #[test]
    fn the_strongest_n_layers_win() {
        // Own amounts: wet .2, mud .9, dust .5, snow .7, wear .1, oxide .6,
        // growth .3, blood .4; all eight listed.
        let own = [51, 230, 128, 179, 26, 153, 77, 102];
        let mask = |level| effective_mask(ALL, own, &MaskRule::new([0.0; 8], ALL, ALL, level));
        assert_eq!(mask(4), 0xff, "ultra: 8");
        assert_eq!(mask(3), 0xff, "high: 8");
        assert_eq!(mask(2), 0b0010_1110, "medium: mud, snow, oxide, dust");
        assert_eq!(mask(1), 0b0000_1010, "low: mud, snow");
        // Lite: one layer, and only of wet, dust and snow (mud is not one).
        assert_eq!(mask(0), 0b0000_1000, "lite: snow");
        // A global amount lifts a layer above the rest; ties keep bit order.
        let r = MaskRule::new([1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0], ALL, ALL, 1);
        assert_eq!(effective_mask(ALL, own, &r), 0b1000_0001);
        assert_eq!(LIMITS, [1, 2, 4, 8, 8]);
    }

    #[test]
    fn lite_keeps_only_the_tint_layers() {
        let r = MaskRule::new([1.0; 8], ALL, ALL, 0);
        assert_eq!(r.allow, LITE_LAYERS);
        assert_eq!(r.limit, 1);
        assert_eq!(effective_mask(0b1111_0010, [255; 8], &r), 0, "mud, wear, oxide, growth, blood: off");
        assert_eq!(effective_mask(0b0000_0100, [255; 8], &r), 0b0100, "dust stays");
        // The shader's lite branch of wet, dust and snow is colour only.
        for (src, bit) in [(0usize, 0u8), (2, 2), (3, 3)] {
            let w = crate::shader::ALL_LAYERS[src].wgsl.replace(' ', "");
            assert_eq!(crate::shader::ALL_LAYERS[src].bit, bit);
            let lite = &w[..w.find("returnr;").expect("a lite return")];
            assert!(lite.contains("==0u){"), "{bit}: a lite branch");
            assert!(lite.contains("r.base="), "{bit}: a tint");
            assert!(!lite.contains("r.rough") && !lite.contains("r.n=") && !lite.contains("r.metal"), "{bit}: colour only");
        }
    }

    #[test]
    fn apply_rewrites_only_the_mask_byte() {
        let mut w = [0x9f03_000b, 1, 2, 3, 4, 5, 0, 0];
        w[6] = 0x0000_cc99; // own wet .6, mud .8
        let r = MaskRule::new([0.0; 8], ALL, ALL, 1);
        let out = apply(&w, &r);
        assert_eq!(out[0], 0x0303_000b, "wet and mud kept, the rest of word 0 as it was");
        assert_eq!(&out[1..], &w[1..]);
    }

    #[test]
    fn looks_fill_the_reserved_words_once() {
        let mut seen = [false; 32];
        for lk in &LOOKS {
            assert!((16..31).contains(&lk.word), "{}", lk.name);
            assert!(!seen[lk.word], "{} shares word {}", lk.name, lk.word);
            seen[lk.word] = true;
        }
        let mut w = [0u32; 32];
        default_looks(&mut w);
        assert_eq!(f32::from_bits(w[16]), 0.5);
        assert!((f32::from_bits(w[21]) - 50f32.to_radians().cos()).abs() < 1e-6, "the angle goes as its cosine");
        assert_eq!(pack_colour([1.0, 0.0, 0.214]), 0x007f_00ff, "sRGB8, red in the low byte");
        assert_eq!(w[31], 0, "kd.w stays reserved");
    }
}
