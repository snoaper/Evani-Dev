//! The live material params: per surface `material.<surface>.color` (a
//! colour whose default is the surface's authored base colour; the shader
//! multiplies the painted albedo by colour / default, so the default paints
//! exactly as authored), and for the world surfaces `.wear` (edge chips and
//! paint wear), `.grime` (dirt up the base of walls) and `.streaks` (run-off
//! from the top), each a multiplier with default 1. Two global ones:
//! `material.ao.strength` (the baked AO's exponent) and
//! `material.ao.warmth` (how much shaded corners warm towards bounce light).
//!
//! Roughness is not here on purpose: the forward mesh pass has no
//! per-material roughness (only the global `render.ibl.roughness`), so a
//! param would change nothing. See docs/CHIMPDUTY_INTAKE.md.
//!
//! [`Tuning`] is the values; [`MaterialParams`] the registry handles that
//! read them; [`Tuning::records`] the `kit_data` records the shader reads
//! (see [`crate::layout`]).

use evani_core::params::{F32Param, ParamDef, ParamError, ParamRegistry, Vec3Param};

use crate::scene::surface;

/// The params' owner.
pub const OWNER: &str = "material-proc";

/// One surface's param name, authored base colour (linear; the colour
/// param's default) and whether it takes wear, grime and streaks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceInfo {
    pub id: u32,
    pub name: &'static str,
    pub color: [f32; 3],
    pub weathers: bool,
}

const fn info(id: u32, name: &'static str, color: [f32; 3], weathers: bool) -> SurfaceInfo {
    SurfaceInfo {
        id,
        name,
        color,
        weathers,
    }
}

/// Every surface with params (all but the region-styled `WALL` and `FLOOR`),
/// with the base colour its painter starts from.
pub const SURFACES: [SurfaceInfo; 17] = [
    info(surface::PLASTER, "plaster", [0.58, 0.46, 0.28], true),
    info(surface::SANDSTONE, "sandstone", [0.55, 0.35, 0.16], true),
    info(surface::CONCRETE, "concrete", [0.35, 0.32, 0.28], true),
    info(surface::CEILING, "ceiling", [0.23, 0.21, 0.18], true),
    info(surface::WOOD_CRATE, "wood_crate", [0.3, 0.14, 0.045], true),
    info(
        surface::METAL_CRATE,
        "metal_crate",
        [0.075, 0.11, 0.05],
        true,
    ),
    info(surface::STONE_STEP, "stone_step", [0.47, 0.41, 0.32], true),
    info(surface::PLATFORM, "platform", [0.5, 0.38, 0.22], true),
    info(surface::RAMP, "ramp", [0.3, 0.29, 0.27], true),
    info(surface::PAVING, "paving", [0.36, 0.3, 0.22], true),
    info(
        surface::BRICK_PAVING,
        "brick_paving",
        [0.25, 0.13, 0.08],
        true,
    ),
    info(surface::TRIM_STONE, "trim_stone", [0.62, 0.56, 0.46], true),
    info(surface::TRIM_WOOD, "trim_wood", [0.12, 0.07, 0.035], true),
    info(surface::SIGN_A, "sign_a", [0.035, 0.033, 0.03], false),
    info(surface::SIGN_B, "sign_b", [0.035, 0.033, 0.03], false),
    info(surface::ARROW, "arrow", [0.035, 0.033, 0.03], false),
    info(surface::GROUND, "ground", [0.51, 0.39, 0.22], true),
];

/// The wear, grime and streak multipliers' range.
pub const WEATHER_RANGE: (f32, f32) = (0.0, 3.0);
/// `material.ao.strength`: default and range (the exponent on the baked AO).
pub const AO_STRENGTH: (f32, f32, f32) = (1.2, 0.0, 4.0);
/// `material.ao.warmth`: default and range.
pub const AO_WARMTH: (f32, f32, f32) = (1.0, 0.0, 2.0);

/// The surface's info (None for `WALL`, `FLOOR` and unknown ids).
pub fn surface_info(id: u32) -> Option<&'static SurfaceInfo> {
    SURFACES.iter().find(|s| s.id == id)
}

/// The surface's param prefix (`material.<name>`).
pub fn prefix(s: &SurfaceInfo) -> String {
    format!("material.{}", s.name)
}

/// One surface's tuning.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceTune {
    /// Linear RGB (0..1).
    pub color: [f32; 3],
    pub wear: f32,
    pub grime: f32,
    pub streaks: f32,
}

/// Every surface's tuning, indexed by surface id, and the AO's.
#[derive(Debug, Clone, PartialEq)]
pub struct Tuning {
    pub surfaces: [SurfaceTune; surface::COUNT as usize],
    pub ao_strength: f32,
    pub ao_warmth: f32,
}

impl Default for Tuning {
    /// The authored look: every multiplier 1.
    fn default() -> Self {
        let mut surfaces = [SurfaceTune {
            color: [1.0; 3],
            wear: 1.0,
            grime: 1.0,
            streaks: 1.0,
        }; surface::COUNT as usize];
        for s in SURFACES {
            surfaces[s.id as usize].color = s.color;
        }
        Tuning {
            surfaces,
            ao_strength: AO_STRENGTH.0,
            ao_warmth: AO_WARMTH.0,
        }
    }
}

impl Tuning {
    /// The header record (`kit_data[7]`) for tune records from `base`.
    pub fn header(&self, base: usize) -> [f32; 4] {
        [
            base as f32,
            surface::COUNT as f32,
            self.ao_strength,
            self.ao_warmth,
        ]
    }

    /// Two records per surface id: `[colour / default rgb, wear]`,
    /// `[grime, streaks, 0, 0]`. A surface without a colour default (`WALL`,
    /// `FLOOR`) multiplies by 1.
    pub fn records(&self) -> Vec<[f32; 4]> {
        let mut out = Vec::with_capacity(2 * surface::COUNT as usize);
        for (id, t) in self.surfaces.iter().enumerate() {
            let ratio = match surface_info(id as u32) {
                Some(s) => std::array::from_fn(|i| t.color[i] / s.color[i].max(1e-4)),
                None => [1.0; 3],
            };
            out.push([ratio[0], ratio[1], ratio[2], t.wear]);
            out.push([t.grime, t.streaks, 0.0, 0.0]);
        }
        out
    }
}

/// The registry handles of every material param.
#[derive(Debug, Clone)]
pub struct MaterialParams {
    /// `(surface id, colour, [wear, grime, streaks])` per [`SURFACES`] row.
    surfaces: Vec<(u32, Vec3Param, Option<[F32Param; 3]>)>,
    ao_strength: F32Param,
    ao_warmth: F32Param,
}

impl MaterialParams {
    /// Register them all (owner `material-proc`).
    pub fn register(reg: &mut ParamRegistry) -> Result<Self, ParamError> {
        let mut surfaces = Vec::with_capacity(SURFACES.len());
        for s in SURFACES {
            let p = prefix(&s);
            let color = reg.register_vec3(
                ParamDef::color(&format!("{p}.color"), s.color)
                    .describe(&format!(
                        "{} base colour (linear); the procedural pattern keeps its detail",
                        s.name
                    ))
                    .owner(OWNER),
            )?;
            let weather = if s.weathers {
                let mut h = [None; 3];
                for (k, (what, describe)) in [
                    ("wear", "edge chips and worn paint (multiplier)"),
                    ("grime", "dirt up the base of vertical faces (multiplier)"),
                    (
                        "streaks",
                        "run-off streaks from the top of vertical faces (multiplier)",
                    ),
                ]
                .into_iter()
                .enumerate()
                {
                    h[k] = Some(
                        reg.register_f32(
                            ParamDef::f32(&format!("{p}.{what}"), 1.0)
                                .range(WEATHER_RANGE.0, WEATHER_RANGE.1)
                                .step(0.05)
                                .describe(&format!("{}: {describe}", s.name))
                                .owner(OWNER),
                        )?,
                    );
                }
                match h {
                    [Some(a), Some(b), Some(c)] => Some([a, b, c]),
                    _ => None,
                }
            } else {
                None
            };
            surfaces.push((s.id, color, weather));
        }
        let ao_strength = reg.register_f32(
            ParamDef::f32("material.ao.strength", AO_STRENGTH.0)
                .range(AO_STRENGTH.1, AO_STRENGTH.2)
                .step(0.05)
                .describe("How strongly the baked ambient occlusion darkens corners (its exponent; 0 = off)")
                .owner(OWNER),
        )?;
        let ao_warmth = reg.register_f32(
            ParamDef::f32("material.ao.warmth", AO_WARMTH.0)
                .range(AO_WARMTH.1, AO_WARMTH.2)
                .step(0.05)
                .describe("How much shaded corners warm towards bounce light (0 = neutral)")
                .owner(OWNER),
        )?;
        Ok(MaterialParams {
            surfaces,
            ao_strength,
            ao_warmth,
        })
    }

    /// The current values.
    pub fn read(&self, reg: &ParamRegistry) -> Tuning {
        let mut t = Tuning::default();
        for (id, color, weather) in &self.surfaces {
            let s = &mut t.surfaces[*id as usize];
            s.color = reg.vec3(*color);
            if let Some([w, g, st]) = weather {
                s.wear = reg.f32(*w);
                s.grime = reg.f32(*g);
                s.streaks = reg.f32(*st);
            }
        }
        t.ao_strength = reg.f32(self.ao_strength);
        t.ao_warmth = reg.f32(self.ao_warmth);
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::params::ParamValue;

    #[test]
    fn every_param_registers_with_its_default_inside_its_range() {
        let mut reg = ParamRegistry::default();
        let h = MaterialParams::register(&mut reg).unwrap();
        // 14 weathering surfaces x 4, 3 stencils x 1, 2 AO.
        assert_eq!(reg.len(), 14 * 4 + 3 + 2);
        for d in reg.defs() {
            assert!(d.name.starts_with("material."), "{}", d.name);
            assert_eq!(d.owner, OWNER);
            let v = match d.default {
                ParamValue::F32(v) => vec![v],
                ParamValue::Color(c) => c.to_vec(),
                other => panic!("{}: {other:?}", d.name),
            };
            assert!(
                v.iter().all(|x| (d.min..=d.max).contains(x)),
                "{}: {v:?} not in {}..{}",
                d.name,
                d.min,
                d.max
            );
        }
        // The defaults read back as the authored look, which packs to ones.
        let t = h.read(&reg);
        assert_eq!(t, Tuning::default());
        for r in t.records() {
            assert!(
                r.iter().all(|v| (*v - 1.0).abs() < 1e-6 || *v == 0.0),
                "{r:?}"
            );
        }
    }

    #[test]
    fn values_past_the_range_are_clamped_and_reach_the_records() {
        let mut reg = ParamRegistry::default();
        let h = MaterialParams::register(&mut reg).unwrap();
        let wear = reg.id("material.plaster.wear").unwrap();
        reg.set(wear, ParamValue::F32(9.0)).unwrap();
        let ao = reg.id("material.ao.strength").unwrap();
        reg.set(ao, ParamValue::F32(-1.0)).unwrap();
        let col = reg.id("material.plaster.color").unwrap();
        reg.set(col, ParamValue::Color([0.29, 0.92, 2.0])).unwrap();
        let t = h.read(&reg);
        assert_eq!(t.surfaces[surface::PLASTER as usize].wear, WEATHER_RANGE.1);
        assert_eq!(t.ao_strength, AO_STRENGTH.1);
        let r = t.records();
        let p = 2 * surface::PLASTER as usize;
        assert!(
            (r[p][0] - 0.5).abs() < 1e-5 && (r[p][1] - 2.0).abs() < 1e-5,
            "{:?}",
            r[p]
        );
        // Blue clamps to 1, so 1 / 0.28.
        assert!((r[p][2] - 1.0 / 0.28).abs() < 1e-4, "{:?}", r[p]);
        assert_eq!(r[p][3], 3.0);
        // Other surfaces untouched.
        let q = 2 * surface::SANDSTONE as usize;
        assert!(r[q].iter().all(|v| (*v - 1.0).abs() < 1e-6));
    }

    #[test]
    fn names_are_unique_and_cover_every_painted_surface() {
        let mut ids: Vec<u32> = SURFACES.iter().map(|s| s.id).collect();
        ids.sort_unstable();
        assert_eq!(ids, (2..surface::COUNT).collect::<Vec<_>>());
        let mut names: Vec<&str> = SURFACES.iter().map(|s| s.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), SURFACES.len());
    }
}
