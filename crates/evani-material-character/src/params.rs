//! The live params, the quality each tier gets, and the `char_params` slot
//! body they become.
//!
//! Every colour, strength and width is a registered param (owner
//! `material-character`): `material.eye.*`, `material.skin.*` and
//! `material.hair.*`, plus `material.character.quality` (-1: by tier). A
//! material's own values (see [`crate::record`]) win over the eye, skin and
//! hair colours and sizes; the params are what every other material of that
//! kind uses.
//!
//! Quality by tier (the tier files are compiled into every game, so the
//! table lives here rather than in a new tier field):
//!
//! | quality | tiers        | eye                         | skin                                   | hair |
//! |---------|--------------|-----------------------------|----------------------------------------|------|
//! | 0       | lite         | flat painted iris, wet lobes | wrapped diffuse, warm terminator      | strand highlights, coverage edges |
//! | 1       | low          | + cornea bulge and refraction | as 0                                 | as 0 |
//! | 2       | medium, vr   | as 1                        | + pre-integrated lookup by curvature, pores | as 0 |
//! | 3       | high, ultra  | as 1                        | + fine wrinkles                        | as 0 |
//!
//! The slot body is rewritten (one rebuild of the two model pipelines) when
//! a param or the tier changes, the way the stylizedpbr look updates its
//! slots.

use evani_core::ecs::{Stage, SystemDesc};
use evani_core::params::{F32Param, ParamDef, ParamRegistry, Vec3Param};
use evani_core::plugin::{PluginContext, PluginError};
use evani_core::slots::{ShaderSlots, SlotRebuilds};
use evani_core::tiers::Tiers;

use crate::lut::{self, FIT_ROWS, FitRow};
use crate::{OWNER, shader};

/// A number param: name, default, min, max, step, description.
type Num = (&'static str, f32, f32, f32, f32, &'static str);
/// A colour param: name, default (linear), description.
type Col = (&'static str, [f32; 3], &'static str);

/// Colours, in slot order (each is the rgb of one slot entry).
pub const COLORS: [Col; 6] = [
    ("material.eye.iris_color", [0.30, 0.15, 0.05], "Outer iris colour (linear); a material's evani_eye.iris_color wins"),
    ("material.eye.iris_inner_color", [0.62, 0.40, 0.13], "Iris colour round the pupil (the collarette)"),
    ("material.eye.sclera_color", [0.80, 0.76, 0.72], "White of the eye; a material's evani_eye.sclera_color wins"),
    ("material.skin.scatter_color", [1.0, 0.42, 0.26], "Colour of the light that bleeds past the shadow edge under the skin"),
    ("material.hair.primary_color", [1.0, 0.97, 0.92], "Tint of the sharp strand highlight (light colour)"),
    ("material.hair.secondary_color", [1.0, 0.85, 0.7], "Tint of the soft second highlight (times the hair colour)"),
];

/// Numbers, in slot order (four per slot entry, after the colours' w).
pub const NUMS: [Num; 27] = [
    // Slot 1 to 3 w: one each.
    ("material.eye.iris_size", 0.48, 0.1, 0.9, 0.01, "Iris radius as a share of the eyeball radius"),
    ("material.eye.pupil_size", 0.36, 0.05, 0.9, 0.01, "Pupil radius as a share of the iris"),
    ("material.eye.limbal_width", 0.16, 0.0, 0.6, 0.01, "Width of the dark ring round the iris, as a share of the iris"),
    // Slot 4.
    ("material.eye.cornea_bulge", 0.75, 0.0, 1.0, 0.01, "How far the clear cornea bulges over the iris (0 = flush)"),
    ("material.eye.ior", 1.376, 1.0, 1.6, 0.005, "Refractive index of the cornea (how far the iris shifts with the view)"),
    ("material.eye.iris_depth", 0.06, 0.0, 0.3, 0.005, "How deep the iris sits under the cornea, as a share of the eyeball radius"),
    ("material.eye.wet_roughness", 0.05, 0.02, 0.5, 0.005, "Roughness of the wet eye surface"),
    // Slot 5.
    ("material.eye.wet_lobe", 1.0, 0.0, 4.0, 0.05, "Strength of the sharp wet highlight"),
    ("material.eye.catchlight", 1.6, 0.0, 8.0, 0.05, "Brightness of the catchlight (times the sun)"),
    ("material.eye.catchlight_size", 0.02, 0.002, 0.2, 0.002, "Size of the catchlight"),
    ("material.eye.fibre_strength", 1.0, 0.0, 2.0, 0.05, "Contrast of the iris fibres and rings"),
    // Slot 6 w.
    ("material.skin.wrap", 0.45, 0.0, 1.0, 0.01, "How far light wraps past the shadow edge on Lite and Low"),
    // Slot 7.
    ("material.skin.warmth", 1.0, 0.0, 3.0, 0.05, "Strength of the warm band along the shadow edge"),
    ("material.skin.curvature_scale", 6.0, 0.0, 40.0, 0.5, "How strongly curvature softens the light (1 = true to size; more for small or stylised heads)"),
    ("material.skin.pore_strength", 0.35, 0.0, 2.0, 0.01, "Depth of the pores (Medium and up)"),
    ("material.skin.wrinkle_strength", 0.25, 0.0, 2.0, 0.01, "Depth of the fine wrinkles (High and Ultra)"),
    // Slot 8.
    ("material.skin.pore_scale", 220.0, 10.0, 4000.0, 5.0, "Pores per UV unit"),
    // 400 lines per UV unit is about 1.6 mm apart on a 0.23 m head; 45 drew
    // 14 mm stripes that read as banding, not skin.
    ("material.skin.wrinkle_scale", 400.0, 2.0, 1000.0, 1.0, "Wrinkle lines per UV unit"),
    ("material.skin.normal_blur", 0.6, 0.0, 1.0, 0.05, "How much the red light ignores small bumps (light scattered under them)"),
    ("material.skin.unused", 0.0, 0.0, 0.0, 0.0, ""),
    // Slot 9 and 10 w.
    ("material.hair.primary_strength", 0.35, 0.0, 4.0, 0.01, "Strength of the sharp strand highlight"),
    ("material.hair.secondary_strength", 0.6, 0.0, 4.0, 0.01, "Strength of the soft, coloured second highlight"),
    // Slot 11.
    ("material.hair.primary_shift", 0.1, -1.0, 1.0, 0.01, "Shift of the sharp highlight along the strand (towards the tip)"),
    ("material.hair.secondary_shift", -0.15, -1.0, 1.0, 0.01, "Shift of the second highlight (towards the root)"),
    ("material.hair.shift_jitter", 0.2, 0.0, 1.0, 0.01, "How much each lock's highlight shifts differently"),
    ("material.hair.primary_exponent", 80.0, 2.0, 512.0, 1.0, "Sharpness of the strand highlight"),
    // Slot 12 (four).
    ("material.hair.edge_softness", 0.45, 0.0, 1.0, 0.01, "Soft edge across each lock, as a share of its width"),
];

/// The rest of slot 12.
pub const HAIR_TAIL: [Num; 3] = [
    ("material.hair.tip_width", 0.25, 0.0, 1.0, 0.01, "Lock width at the tip, as a share of its root width"),
    ("material.hair.secondary_exponent", 20.0, 2.0, 512.0, 1.0, "Sharpness of the second highlight"),
    ("material.hair.root_dark", 0.45, 0.0, 1.0, 0.01, "How much darker the hair is at its roots (shadow inside the fur)"),
];

/// `material.character.quality`.
pub const QUALITY: Num = ("material.character.quality", -1.0, -1.0, 3.0, 1.0, "Character material quality: -1 by tier, else 0 (Lite) to 3 (High)");

/// `material.character.enabled`: 0 turns the character shading off (the
/// plain lit pass), for before/after looks.
pub const ENABLED: Num = ("material.character.enabled", 1.0, 0.0, 1.0, 1.0, "Character materials on (1) or off (0: the plain lit pass), for before and after looks");

/// The quality a tier gets.
pub fn tier_quality(id: &str) -> u32 {
    match id {
        "lite" => 0,
        "low" => 1,
        "medium" | "vr" => 2,
        "high" | "ultra" => 3,
        _ => 2,
    }
}

/// Handles of every param.
#[derive(Debug, Clone)]
pub struct Handles {
    colors: Vec<Vec3Param>,
    nums: Vec<F32Param>,
    quality: F32Param,
    enabled: F32Param,
}

/// The values the body is made from.
#[derive(Debug, Clone, PartialEq)]
pub struct Values {
    pub colors: [[f32; 3]; 6],
    /// [`NUMS`] then [`HAIR_TAIL`].
    pub nums: [f32; 30],
    pub quality: f32,
    pub enabled: f32,
}

impl Default for Values {
    fn default() -> Self {
        let mut v = Self { colors: [[0.0; 3]; 6], nums: [0.0; 30], quality: QUALITY.1, enabled: ENABLED.1 };
        for (o, c) in v.colors.iter_mut().zip(COLORS) {
            *o = c.1;
        }
        for (o, n) in v.nums.iter_mut().zip(NUMS.iter().chain(HAIR_TAIL.iter())) {
            *o = n.1;
        }
        v
    }
}

fn num_def(n: &Num) -> ParamDef {
    ParamDef::f32(n.0, n.1).range(n.2, n.3).step(n.4).describe(n.5).owner(OWNER)
}

impl Handles {
    /// Register them all (the unused padding entries are not params).
    pub fn register(reg: &mut ParamRegistry) -> Result<Self, PluginError> {
        let mut colors = Vec::new();
        for c in COLORS.iter().filter(|c| !c.2.is_empty()) {
            colors.push(reg.register_vec3(ParamDef::color(c.0, c.1).describe(c.2).owner(OWNER))?);
        }
        let mut nums = Vec::new();
        for n in NUMS.iter().chain(HAIR_TAIL.iter()).filter(|n| !n.5.is_empty()) {
            nums.push(reg.register_f32(num_def(n))?);
        }
        let quality = reg.register_f32(num_def(&QUALITY))?;
        let enabled = reg.register_f32(num_def(&ENABLED))?;
        Ok(Self { colors, nums, quality, enabled })
    }

    pub fn read(&self, reg: &ParamRegistry) -> Values {
        let mut v = Values::default();
        let mut c = self.colors.iter();
        for (o, d) in v.colors.iter_mut().zip(COLORS) {
            if !d.2.is_empty()
                && let Some(h) = c.next()
            {
                *o = reg.vec3(*h);
            }
        }
        let mut n = self.nums.iter();
        for (o, d) in v.nums.iter_mut().zip(NUMS.iter().chain(HAIR_TAIL.iter())) {
            if !d.5.is_empty()
                && let Some(h) = n.next()
            {
                *o = reg.f32(*h);
            }
        }
        v.quality = reg.f32(self.quality);
        v.enabled = reg.f32(self.enabled);
        v
    }
}

fn f(x: f32) -> String {
    let x = if x.is_finite() { x } else { 0.0 };
    // Enough digits for colours and exponents, always a float literal.
    let s = format!("{x:.5}");
    if s.contains('.') { s } else { format!("{s}.0") }
}

fn v4(a: f32, b: f32, c: f32, d: f32) -> String {
    format!("vec4<f32>({}, {}, {}, {})", f(a), f(b), f(c), f(d))
}

/// The entries of the slot, index 0 to 28.
pub fn entries(v: &Values, tier: u32, msaa: u32, fit: &[FitRow; FIT_ROWS]) -> Vec<String> {
    let q = if v.quality >= 0.0 { v.quality.round().clamp(0.0, 3.0) } else { tier.min(3) as f32 };
    let n = &v.nums;
    let c = &v.colors;
    let mut e = vec![
        v4(if v.enabled >= 0.5 { 1.0 } else { 0.0 }, q, msaa.max(1) as f32, 0.0),
        v4(c[0][0], c[0][1], c[0][2], n[0]),
        v4(c[1][0], c[1][1], c[1][2], n[1]),
        v4(c[2][0], c[2][1], c[2][2], n[2]),
        v4(n[3], n[4], n[5], n[6]),
        v4(n[7], n[8], n[9], n[10]),
        v4(c[3][0], c[3][1], c[3][2], n[11]),
        v4(n[12], n[13], n[14], n[15]),
        v4(n[16], n[17], n[18], n[19]),
        v4(c[4][0], c[4][1], c[4][2], n[20]),
        v4(c[5][0], c[5][1], c[5][2], n[21]),
        v4(n[22], n[23], n[24], n[25]),
        v4(n[26], n[27], n[28], n[29]),
    ];
    for r in fit {
        e.push(v4(r.width[0], r.width[1], r.width[2], 0.0));
    }
    for r in fit {
        e.push(v4(r.amp[0], r.amp[1], r.amp[2], 0.0));
    }
    e
}

/// The `char_params` slot body.
pub fn body(v: &Values, tier: u32, msaa: u32, fit: &[FitRow; FIT_ROWS]) -> String {
    let mut s = String::from("    switch i {\n");
    for (i, e) in entries(v, tier, msaa, fit).iter().enumerate() {
        s.push_str(&format!("        case {i}u: {{ return {e}; }}\n"));
    }
    s.push_str("        default: { return vec4<f32>(0.0); }\n    }");
    s
}

/// The tier's quality and MSAA samples, or Medium without MSAA when the
/// game has no tiers (tests, tools).
fn tier_now(world: &evani_core::ecs::World) -> (usize, u32, u32) {
    world.try_res::<Tiers>().map_or((usize::MAX, 2, 1), |t| {
        let a = t.active();
        (t.active_index(), tier_quality(&a.id), a.msaa)
    })
}

/// Declare the slot, register the params, set the first body and keep it
/// in step with the params and the tier.
pub fn register(ctx: &mut PluginContext) -> Result<(), PluginError> {
    ctx.world.res_mut::<ShaderSlots>().declare(shader::CHAR_PARAMS);
    let handles = Handles::register(&mut ctx.world.res_mut::<ParamRegistry>())?;
    let fit = lut::fitted();
    let (_, q, ms) = tier_now(ctx.world);
    let first = body(&handles.read(&ctx.world.res::<ParamRegistry>()), q, ms, &fit);
    ctx.world.res_mut::<ShaderSlots>().set_default(shader::CHAR_PARAMS.name, &first, OWNER).map_err(|e| PluginError(e.to_string()))?;
    let mut last = first;
    let mut seen = (u64::MAX, usize::MAX);
    ctx.system(SystemDesc::new("material.character.slots", Stage::Update, move |world| {
        let (tier, q, ms) = tier_now(world);
        let body = {
            let Some(params) = world.try_res::<ParamRegistry>() else {
                return;
            };
            if (params.revision(), tier) == seen {
                return;
            }
            seen = (params.revision(), tier);
            body(&handles.read(&params), q, ms, &fit)
        };
        if body == last {
            return;
        }
        let Some(mut slots) = world.try_res_mut::<ShaderSlots>() else {
            return;
        };
        let queued = world.try_res_mut::<SlotRebuilds>().map(|mut r| r.request_detached(&slots, shader::CHAR_PARAMS.name, &body, 0.0));
        match queued {
            Some(Ok(())) => last = body,
            // No renderer (tests, tools): the body is all there is.
            None => {
                if let Some(id) = slots.id(shader::CHAR_PARAMS.name)
                    && slots.set_body(id, &body).is_ok()
                {
                    last = body;
                }
            }
            Some(Err(_)) => {}
        }
    }));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fit() -> [FitRow; FIT_ROWS] {
        [FitRow { width: [0.1, 0.05, 0.02], amp: [1.0; 3], rms: [0.0; 3] }; FIT_ROWS]
    }

    #[test]
    fn names_follow_the_rules() {
        let all = COLORS.iter().map(|c| (c.0, c.2)).chain(NUMS.iter().chain(HAIR_TAIL.iter()).chain([&QUALITY, &ENABLED]).map(|n| (n.0, n.5)));
        let mut names = Vec::new();
        for (name, desc) in all.filter(|(_, d)| !d.is_empty()) {
            assert!(name.starts_with("material.") && name.split('.').count() == 3, "{name}");
            assert!(name.chars().all(|c| c.is_ascii_lowercase() || c == '.' || c == '_'), "{name}");
            assert!(!desc.is_empty());
            names.push(name);
        }
        let n = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), n, "no duplicates");
        for d in NUMS.iter().chain(HAIR_TAIL.iter()).filter(|d| !d.5.is_empty()) {
            assert!(d.2 <= d.1 && d.1 <= d.3, "{} default in range", d.0);
        }
    }

    #[test]
    fn tiers_map_to_quality() {
        let q: Vec<u32> = ["lite", "low", "medium", "high", "ultra", "vr", "custom"].iter().map(|t| tier_quality(t)).collect();
        assert_eq!(q, [0, 1, 2, 3, 3, 2, 2]);
    }

    #[test]
    fn body_has_every_entry_and_the_override_wins() {
        let v = Values::default();
        let b = body(&v, 1, 4, &fit());
        for i in 0..29 {
            assert!(b.contains(&format!("case {i}u:")), "case {i}");
        }
        assert!(b.contains("case 0u: { return vec4<f32>(1.00000, 1.00000, 4.00000, 0.00000); }"), "{b}");
        // Iris colour and size in entry 1.
        assert!(b.contains("case 1u: { return vec4<f32>(0.30000, 0.15000, 0.05000, 0.48000); }"));
        let mut forced = v.clone();
        forced.quality = 3.0;
        assert!(body(&forced, 0, 1, &fit()).contains("case 0u: { return vec4<f32>(1.00000, 3.00000, 1.00000"));
        assert!(evani_core::slots::check_body(&b).is_ok());
    }

    #[test]
    fn handles_register_and_read_back() {
        let mut reg = ParamRegistry::default();
        let h = Handles::register(&mut reg).unwrap();
        assert_eq!(h.read(&reg), Values::default());
        assert!(Handles::register(&mut reg).is_err(), "names are taken once");
    }
}
