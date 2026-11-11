//! The spectral sea (Cargo feature `spectral-sea`, `nature = ["spectral-sea"]`):
//! tall open-sea waves from a wind-and-swell spectrum, ray-marched so crests
//! stand up against the sky, with a roughness-aware sun and sky, far sun
//! glitter, a turquoise crest glow, per-channel absorption, whitecaps that
//! linger and erode into lace, a straight beach with breakers, swash and a
//! wet line, caustics on the bed and spray mist over breaking lips.
//!
//! It is its own pass (`nature.sea`, before the nature pass) drawing the
//! [`Sea`] a game puts in the [`SeaSurfaces`] resource; the nature pass and
//! its shaders are untouched, so water drawn as [`crate::Kind::WaterRect`] or
//! [`crate::Kind::WaterEllipse`] looks exactly as before, and a game that
//! doesn't list the part compiles none of this.
//!
//! Every setting is a live param (`material.nature.water.*`) read into the
//! draw's uniforms each frame: a change shows on the next frame, nothing is
//! baked or recompiled (the wave table is fixed, in normalised units:
//! [`spectrum`]). Quality ([`level`]): Ultra, High, Medium, Low from the
//! device tier or `material.nature.water.sea_quality`.

pub mod field;
pub mod spectrum;

#[cfg(test)]
mod tests;

use core::cell::Cell;
use std::rc::Rc;

use evani_core::camera::CameraPose;
use evani_core::params::{BakeTarget, ParamDef, ParamError, ParamId, ParamRegistry, ParamValue};
use evani_render::depth::inverse_distance;
use evani_render::post::{Blend, EffectShader, MeshShader, PostEffect, PostRecorder, UniformFrame, draw_effect, draw_effect_mesh, put_words};
use evani_render::shaders::{FAR, NEAR};

use crate::effect::norm;

/// The pass and its screen copy.
pub const PASS: &str = "nature.sea";
pub const PASSES: &[&str] = &[PASS];
pub const SCENE: &str = "nature.sea.scene";

/// The sea's quality levels (Chris: "Ultra / High / Medium / Low").
pub const LEVELS: [&str; 4] = ["Low", "Medium", "High", "Ultra"];

// The classic sea (`sea_vs.wgsl` then `sea_common.wgsl` are the one body
// it always had, split so the field sea can share everything after the
// vertex stage).
const SEA_WGSL: &str = concat!(
    include_str!("../../shaders/sea.wgsl"),
    "\n",
    include_str!("../../shaders/sea_vs.wgsl"),
    include_str!("../../shaders/sea_common.wgsl"),
    "\n",
    include_str!("../../shaders/sea_table.wgsl")
);
/// The field sea's wave textures, its mesh and its full-screen fill
/// (src/sea/field.rs).
const WAVES_WGSL: &str = concat!(
    include_str!("../../shaders/sea_waves.wgsl"),
    "\n",
    include_str!("../../shaders/sea_field_bands.wgsl"),
    "\n",
    include_str!("../../shaders/sea_field_table.wgsl")
);
// (Dev builds, `capture`, also get the foam view: sea_field_debug.wgsl; ship
// builds its no-op twin, so the view never reaches a game's download.)
#[cfg(feature = "capture")]
const FIELD_WGSL: &str = concat!(
    include_str!("../../shaders/sea_field.wgsl"),
    "\n",
    include_str!("../../shaders/sea_common.wgsl"),
    "\n",
    include_str!("../../shaders/sea_field_bands.wgsl"),
    "\n",
    include_str!("../../shaders/sea_field_break.wgsl"),
    "\n",
    include_str!("../../shaders/sea_field_debug.wgsl"),
    "\n",
    include_str!("../../shaders/sea_table.wgsl")
);
#[cfg(not(feature = "capture"))]
const FIELD_WGSL: &str = concat!(
    include_str!("../../shaders/sea_field.wgsl"),
    "\n",
    include_str!("../../shaders/sea_common.wgsl"),
    "\n",
    include_str!("../../shaders/sea_field_bands.wgsl"),
    "\n",
    include_str!("../../shaders/sea_field_break.wgsl"),
    "\n",
    include_str!("../../shaders/sea_field_debug_off.wgsl"),
    "\n",
    include_str!("../../shaders/sea_table.wgsl")
);
const SPRAY_WGSL: &str = concat!(
    include_str!("../../shaders/sea_spray.wgsl"),
    "\n",
    include_str!("../../shaders/sea_common.wgsl"),
    "\n",
    include_str!("../../shaders/sea_field_bands.wgsl"),
    "\n",
    include_str!("../../shaders/sea_field_break.wgsl"),
    "\n",
    include_str!("../../shaders/sea_table.wgsl")
);
const FILL_WGSL: &str = concat!(
    include_str!("../../shaders/sea_fill.wgsl"),
    "\n",
    include_str!("../../shaders/sea_vs.wgsl"),
    include_str!("../../shaders/sea_common.wgsl"),
    "\n",
    include_str!("../../shaders/sea_table.wgsl")
);
/// The generated component table (`spectrum::sea_table_wgsl`).
pub const TABLE_WGSL: &str = include_str!("../../shaders/sea_table.wgsl");

pub const COPY: usize = 0;
pub const FULL: usize = 1;
pub const QUICK: usize = 2;
/// The field sea's draws (src/sea/field.rs).
pub const WAVES: usize = 3;
pub const MESH: usize = 4;
pub const FILL: usize = 5;
pub const SPRAY: usize = 6;

const fn sea_shader(name: &'static str, source: &'static str, blend: Blend, input: bool) -> EffectShader {
    EffectShader { name, pass: PASS, source, slots: &[], blend, uniform_words: 64, input, depth: true }
}

/// Every level draws the same three: the copy, the quick stand-in (a flat
/// sea, ready almost at once: water shows while the full sea's pipeline
/// still builds), then the full sea over every pixel (Low takes its flat
/// path inside it), so the sea's WGSL ships once. The field sea (opted in,
/// src/sea/field.rs) draws the copy and the stand-in, then its wave
/// textures, its mesh and, under the water or over a beach, its fill, and
/// with breaking crests on their spray.
pub static SHADERS: [EffectShader; 7] = [
    EffectShader {
        name: "nature_sea_copy",
        pass: PASS,
        source: include_str!("../../shaders/sea_copy.wgsl"),
        slots: &[],
        blend: Blend::Replace,
        uniform_words: 4,
        input: true,
        depth: false,
    },
    sea_shader("nature_sea", SEA_WGSL, Blend::Replace, true),
    sea_shader("nature_sea_quick", include_str!("../../shaders/sea_quick.wgsl"), Blend::Replace, false),
    EffectShader {
        name: "nature_sea_waves",
        pass: PASS,
        source: WAVES_WGSL,
        slots: &[],
        blend: Blend::Replace,
        uniform_words: 64,
        input: false,
        depth: false,
    },
    sea_shader("nature_sea_field", FIELD_WGSL, Blend::Replace, true),
    sea_shader("nature_sea_fill", FILL_WGSL, Blend::Replace, true),
    sea_shader("nature_sea_spray", SPRAY_WGSL, Blend::Additive, true),
];

/// One open sea: its water level, centre (x, z) and half size (x, z). It
/// covers what it spans; a beach (`material.nature.water.beach_*`) can rise
/// out of it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sea {
    pub level: f32,
    pub centre: [f32; 2],
    pub half: [f32; 2],
}

impl Sea {
    /// An open sea at `level` reaching `half` metres each way from `centre`.
    pub fn new(level: f32, centre: [f32; 2], half: f32) -> Self {
        Sea { level, centre, half: [half, half] }
    }

    fn valid(&self) -> bool {
        self.level.is_finite() && self.centre.iter().chain(&self.half).all(|v| v.is_finite()) && self.half.iter().all(|v| *v > 0.0)
    }
}

pub(crate) struct State {
    pub(crate) sea: Cell<Option<Sea>>,
    pub(crate) camera: Cell<CameraPose>,
    pub(crate) time: Cell<f64>,
    pub(crate) sun: Cell<[f32; 3]>,
    pub(crate) sun_col: Cell<[f32; 3]>,
    pub(crate) sky_tint: Cell<f32>,
    /// 0 Low .. 3 Ultra; -1 off.
    pub(crate) level: Cell<i32>,
    #[cfg(feature = "capture")]
    pub(crate) jitter: Cell<[f32; 2]>,
}

/// The sea to draw (a world resource after the module's build; clones
/// share it). A game sets it once.
#[derive(Clone)]
pub struct SeaSurfaces(pub(crate) Rc<State>);

impl Default for SeaSurfaces {
    fn default() -> Self {
        Self(Rc::new(State {
            sea: Cell::new(None),
            camera: Cell::new(CameraPose::default()),
            time: Cell::new(0.0),
            sun: Cell::new([0.0, 1.0, 0.0]),
            sun_col: Cell::new([1.0; 3]),
            sky_tint: Cell::new(crate::pack_sky_tint([1.0; 3])),
            level: Cell::new(-1),
            #[cfg(feature = "capture")]
            jitter: Cell::new([0.0; 2]),
        }))
    }
}

impl SeaSurfaces {
    /// Show `sea` (one with a non-finite or empty size is ignored).
    pub fn set(&self, sea: Sea) {
        if sea.valid() {
            self.0.sea.set(Some(sea));
        }
    }

    pub fn clear(&self) {
        self.0.sea.set(None);
    }

    pub fn get(&self) -> Option<Sea> {
        self.0.sea.get()
    }

    /// The level drawn this frame (0 Low .. 3 Ultra), `None` when off.
    pub fn level(&self) -> Option<usize> {
        usize::try_from(self.0.level.get()).ok()
    }

    /// How high the sea stands over its level at `p` (x, z) now, counting
    /// only waves longer than about `smooth` metres (the CPU mirror of the
    /// drawn surface): a deck camera rides it so it never dips under a
    /// passing crest. 0 on Low (a flat level); `None` without a sea or with
    /// the part's params missing.
    pub fn height_at(&self, reg: &ParamRegistry, p: [f32; 2], smooth: f32) -> Option<f32> {
        let sea = self.get()?;
        let level = self.level()?;
        if level == 0 {
            return Some(0.0);
        }
        let s = SeaParams::find(reg)?.settings(reg);
        let st = &self.0;
        let fr = FrameIn {
            camera: st.camera.get(),
            size: [1280.0, 720.0],
            time: st.time.get(),
            sun: st.sun.get(),
            sun_col: st.sun_col.get(),
            sky_tint: st.sky_tint.get(),
            sea,
            level,
            inv: [0.0, 1.0],
            jitter: [0.0; 2],
        };
        if field::field_on(&s) {
            return Some(field::Field::new(&s, &fr).height_at(p, smooth));
        }
        Some(Mirror::new(&uniforms(&s, &fr)).height(p, (smooth / 8.0).max(1e-3)))
    }
}

/// The quality level for a device tier: Lite and Low show as Low, Medium
/// and VR as Medium, High as High, Ultra as Ultra.
pub fn level_for_tier(id: &str) -> usize {
    match id {
        "lite" | "low" => 0,
        "high" => 2,
        "ultra" => 3,
        _ => 1,
    }
}

/// The level from `material.nature.water.sea_quality` (-1: by tier).
pub fn level(quality: f32, tier: &str) -> usize {
    let q = quality.round() as i32;
    if q < 0 { level_for_tier(tier) } else { q.min(3) as usize }
}

/// The lowest level each setting shows on (the studio's labels; one line
/// per param the sea reads). Unlisted: every level.
pub const SHOWS_FROM: &[(&str, usize)] = &[
    ("material.nature.water.wave_height", 1),
    ("material.nature.water.sea_depth", 1),
    ("material.nature.water.breaker_height", 1),
    ("material.nature.water.breaker_length", 1),
    ("material.nature.water.refraction", 1),
    ("material.nature.water.mid_waves", 1),
    ("material.nature.water.wave_field", 1),
    ("material.nature.water.foam_streaks", 2),
    ("material.nature.water.spindrift", 2),
    ("material.nature.water.breaking", 1),
    ("material.nature.water.foam_linger", 2),
    ("material.nature.water.glitter", 2),
    ("material.nature.water.caustics", 2),
    ("material.nature.water.spray", 3),
];

/// "Ultra only", "High and Ultra", "Medium and up" or "every level".
pub fn shows_on(param: &str) -> &'static str {
    match SHOWS_FROM.iter().find(|(n, _)| *n == param).map_or(0, |(_, l)| *l) {
        3 => "Ultra only",
        2 => "High and Ultra",
        1 => "Medium and up",
        _ => "every level",
    }
}

#[derive(Debug, Clone, Copy)]
struct Def {
    name: &'static str,
    d: f32,
    min: f32,
    max: f32,
    step: f32,
    unit: &'static str,
    desc: &'static str,
}

const fn f(name: &'static str, d: f32, min: f32, max: f32, step: f32, unit: &'static str, desc: &'static str) -> Def {
    Def { name, d, min, max, step, unit, desc }
}

/// The sea's own params (under `material.nature.water.`).
static DEFS: [Def; 22] = [
    f("sea_quality", -1.0, -1.0, 3.0, 1.0, "", "Spectral sea quality: -1 by the device tier, else 0 Low (flat, the waves' light only), 1 Medium (wave heights), 2 High (glitter, caustics, lingering foam), 3 Ultra (spray, more waves)"),
    f("wind_speed", 9.0, 1.0, 30.0, 0.1, "m/s", "Spectral sea: wind speed. Stronger wind makes taller, longer, steeper waves (a JONSWAP sea)"),
    f("wind_direction", 20.0, 0.0, 360.0, 1.0, "deg", "Spectral sea: heading the wind blows the waves towards (0 = +x, 90 = +z)"),
    f("fetch", 120.0, 1.0, 1000.0, 1.0, "km", "Spectral sea: open water the wind has blown over. Longer fetch, bigger and longer waves"),
    f("wave_height", 1.0, 0.0, 5.0, 0.01, "x", "Spectral sea: wave height against what the wind gives (1 = as the wind makes them; about 3 x waveScale matches Ultra-Water at the same wind, wave length by wave length, measured from its own water surface)"),
    f("sea_swell", 0.7, 0.0, 3.0, 0.01, "x", "Spectral sea: size of the long swell from a far storm (0 = none)"),
    f("sea_swell_direction", 345.0, 0.0, 360.0, 1.0, "deg", "Spectral sea: heading the swell travels towards"),
    f("sea_depth", 200.0, 0.5, 1000.0, 0.5, "m", "Spectral sea: depth of the bed. Waves shrink over shallow water, so a trough never shows the bed"),
    f("foam_linger", 1.4, 0.0, 6.0, 0.05, "s", "Spectral sea: how long whitecap foam stays after a crest breaks"),
    f("glitter", 0.6, 0.0, 1.0, 0.01, "", "Spectral sea: far-off sun glitter, the sun path broken into sparkles"),
    f("caustics", 1.0, 0.0, 2.0, 0.01, "x", "Spectral sea: rippling light on the bed under shallow water"),
    f("spray", 1.0, 0.0, 2.0, 0.01, "x", "Spectral sea: spray mist blown off the breaking lips at the beach"),
    f("beach_slope", 0.0, 0.0, 0.3, 0.005, "", "Spectral sea: the beach's rise per metre (0 = no beach, open sea)"),
    f("beach_distance", 60.0, -2000.0, 2000.0, 0.5, "m", "Spectral sea: from the sea's centre to the waterline, towards the land"),
    f("beach_direction", 270.0, 0.0, 360.0, 1.0, "deg", "Spectral sea: heading from the sea towards the land"),
    f("breaker_height", 0.8, 0.0, 3.0, 0.01, "m", "Spectral sea: height of the waves breaking on the beach"),
    f("breaker_length", 30.0, 5.0, 120.0, 0.5, "m", "Spectral sea: length between the breakers"),
    f("mid_waves", 0.0, 0.0, 2.5, 0.02, "x", "Spectral sea: the wind sea's mid-size waves (a third of the peak's length down to a few metres) raised or lowered against what the spectrum gives (0 or 1 = as the spectrum gives them); any value above 0 draws the field sea (wave textures)"),
    f("foam_streaks", 0.0, 0.0, 1.0, 0.02, "", "Spectral sea: foam builds up where crests keep breaking and ages into long streaks blown along the wind, as in a gale (0 = off; on, the field sea draws)"),
    f("spindrift", 0.0, 0.0, 2.5, 0.02, "x", "Spectral sea: spray torn off the breaking crests out at sea and blown downwind (0 = off; on, the field sea draws)"),
    f("breaking", 0.0, 0.0, 1.5, 0.01, "x", "Spectral sea: some waves (more as the wind rises) steepen, fold over at the lip and collapse with a splash, leaving a patch of foam that fades (0 = none; on, the field sea draws; High and Ultra fold and splash in full, Medium leans and shows the foam and a thinner splash, Low has none)"),
    f("wave_field", 0.0, 0.0, 1.0, 1.0, "", "Spectral sea: draw the waves from wave textures made each frame on a moving mesh (Ultra-Water's way: crests squeezed sharp, and a tall storm costs no more to draw than a calm sea); 0 = the classic per-pixel waves"),
];

/// The foam view's param (registered in dev builds only, `capture`).
pub const FOAM_DEBUG: &str = "material.nature.water.foam_debug";

/// Names of the shared water params the sea reads (from the nature table).
const SHARED: [&str; 17] = [
    "material.nature.water.deep_color",
    "material.nature.water.clarity",
    "material.nature.water.scatter_color",
    "material.nature.water.sss_color",
    "material.nature.water.sss_strength",
    "material.nature.water.reflection",
    "material.nature.water.roughness",
    "material.nature.water.glint",
    "material.nature.water.refraction",
    "material.nature.water.whitecap_amount",
    "material.nature.water.whitecap_threshold",
    "material.nature.water.choppiness",
    "material.nature.water.foam",
    "material.nature.water.wave_speed",
    "material.nature.sky.horizon",
    "material.nature.sky.zenith",
    "material.nature.enabled",
];

/// The sea's own params (registered only with the part), by name.
pub fn own_param_names() -> Vec<String> {
    // (With the foam view's dev-only switch: the sea's own, not the lake's.)
    let mut v: Vec<String> = DEFS.iter().map(|d| format!("material.nature.water.{}", d.name)).collect();
    v.push(FOAM_DEBUG.to_string());
    v
}

/// Every param the sea reads, by name (its own then the shared ones).
pub fn param_names() -> Vec<String> {
    let mut v: Vec<String> = DEFS.iter().map(|d| format!("material.nature.water.{}", d.name)).collect();
    v.extend(SHARED.iter().map(|s| s.to_string()));
    v
}

/// The registered handles.
#[derive(Debug, Clone)]
pub struct SeaParams {
    own: Vec<ParamId>,
    shared: Vec<Option<ParamId>>,
}

impl SeaParams {
    /// Register the sea's params (the nature params must be registered).
    pub fn register(reg: &mut ParamRegistry) -> Result<Self, ParamError> {
        let mut own = Vec::with_capacity(DEFS.len());
        for d in DEFS.iter() {
            let name = format!("material.nature.water.{}", d.name);
            let def = ParamDef::f32(&name, d.d).range(d.min, d.max).step(d.step).unit(d.unit).describe(d.desc).owner("material.nature");
            let bake = if d.name == "sea_quality" { BakeTarget::None } else { BakeTarget::Look };
            own.push(reg.register(def.bake(bake))?);
        }
        // The foam view (dev builds only: the live tools turn it on; ship
        // builds have neither the param nor the view's shader code).
        #[cfg(feature = "capture")]
        reg.register(
            ParamDef::f32(FOAM_DEBUG, 0.0)
                .range(0.0, 1.0)
                .step(1.0)
                .describe("Spectral sea (dev builds): paint the field sea's foam by its cause: red new from a crest breaking or the shore, blue carried from a break a moment ago, white fading, yellow none (there should be none); the water in grey")
                .owner("material.nature")
                .bake(BakeTarget::None),
        )?;
        let shared = SHARED.iter().map(|n| reg.id(n)).collect();
        Ok(SeaParams { own, shared })
    }

    /// The handles of params already registered (by
    /// [`SeaParams::register`]), `None` if the part's params are missing.
    pub fn find(reg: &ParamRegistry) -> Option<Self> {
        let own = DEFS.iter().map(|d| reg.id(&format!("material.nature.water.{}", d.name))).collect::<Option<Vec<_>>>()?;
        Some(SeaParams { own, shared: SHARED.iter().map(|n| reg.id(n)).collect() })
    }

    fn own(&self, reg: &ParamRegistry, i: usize) -> f32 {
        match self.own.get(i).and_then(|id| reg.value(*id)) {
            Some(ParamValue::F32(x)) => x,
            _ => DEFS.get(i).map_or(0.0, |d| d.d),
        }
    }

    fn shared_f(&self, reg: &ParamRegistry, i: usize, d: f32) -> f32 {
        match self.shared.get(i).copied().flatten().and_then(|id| reg.value(id)) {
            Some(ParamValue::F32(x)) => x,
            _ => d,
        }
    }

    fn shared_c(&self, reg: &ParamRegistry, i: usize, d: [f32; 3]) -> [f32; 3] {
        match self.shared.get(i).copied().flatten().and_then(|id| reg.value(id)) {
            Some(ParamValue::Color(c) | ParamValue::Vec3(c)) => c,
            _ => d,
        }
    }

    /// `material.nature.enabled` (true when absent).
    pub fn enabled(&self, reg: &ParamRegistry) -> bool {
        !matches!(self.shared.get(16).copied().flatten().and_then(|id| reg.value(id)), Some(ParamValue::Bool(false)))
    }

    /// `sea_quality`.
    pub fn quality(&self, reg: &ParamRegistry) -> f32 {
        self.own(reg, 0)
    }

    /// The settings as plain numbers.
    pub fn settings(&self, reg: &ParamRegistry) -> Settings {
        let o = |i| self.own(reg, i);
        Settings {
            wind_speed: o(1),
            wind_direction: o(2),
            fetch_km: o(3),
            wave_height: o(4),
            swell: o(5),
            swell_direction: o(6),
            depth: o(7),
            foam_linger: o(8),
            glitter: o(9),
            caustics: o(10),
            spray: o(11),
            beach_slope: o(12),
            beach_distance: o(13),
            beach_direction: o(14),
            breaker_height: o(15),
            breaker_length: o(16),
            mid_waves: o(17),
            foam_streaks: o(18),
            spindrift: o(19),
            breaking: o(20),
            wave_field: o(21),
            foam_debug: match reg.id(FOAM_DEBUG).and_then(|id| reg.value(id)) {
                Some(ParamValue::F32(x)) => x,
                _ => 0.0,
            },
            deep: self.shared_c(reg, 0, [0.45, 0.74, 0.78]),
            clarity: self.shared_f(reg, 1, 2.5),
            scatter: self.shared_c(reg, 2, [0.08, 0.30, 0.34]),
            sss: self.shared_c(reg, 3, [0.10, 0.55, 0.42]),
            sss_strength: self.shared_f(reg, 4, 0.0),
            reflection: self.shared_f(reg, 5, 1.0),
            roughness: self.shared_f(reg, 6, 0.08),
            glint: self.shared_f(reg, 7, 1.0),
            refraction: self.shared_f(reg, 8, 1.0),
            whitecaps: self.shared_f(reg, 9, 0.0),
            whitecap_threshold: self.shared_f(reg, 10, 0.5),
            choppiness: self.shared_f(reg, 11, 0.0),
            shore_foam: self.shared_f(reg, 12, 0.8),
            speed: self.shared_f(reg, 13, 1.0),
            horizon: self.shared_c(reg, 14, [0.62, 0.72, 0.82]),
            zenith: self.shared_c(reg, 15, [0.24, 0.42, 0.72]),
        }
    }
}

/// The sea's settings for one frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    pub wind_speed: f32,
    pub wind_direction: f32,
    pub fetch_km: f32,
    pub wave_height: f32,
    pub swell: f32,
    pub swell_direction: f32,
    pub depth: f32,
    pub foam_linger: f32,
    pub glitter: f32,
    pub caustics: f32,
    pub spray: f32,
    pub beach_slope: f32,
    pub beach_distance: f32,
    pub beach_direction: f32,
    pub breaker_height: f32,
    pub breaker_length: f32,
    /// The field sea (src/sea/field.rs): the mid waves' gain, the gale's
    /// foam streaks, spindrift, and the switch itself.
    pub mid_waves: f32,
    pub foam_streaks: f32,
    pub spindrift: f32,
    pub breaking: f32,
    pub wave_field: f32,
    /// The foam view (dev builds; 0 = off).
    pub foam_debug: f32,
    pub deep: [f32; 3],
    pub clarity: f32,
    pub scatter: [f32; 3],
    pub sss: [f32; 3],
    pub sss_strength: f32,
    pub reflection: f32,
    pub roughness: f32,
    pub glint: f32,
    pub refraction: f32,
    pub whitecaps: f32,
    pub whitecap_threshold: f32,
    pub choppiness: f32,
    pub shore_foam: f32,
    pub speed: f32,
    pub horizon: [f32; 3],
    pub zenith: [f32; 3],
}

impl Default for Settings {
    fn default() -> Self {
        let mut reg = ParamRegistry::new();
        let _ = crate::NatureParams::register(&mut reg);
        match SeaParams::register(&mut reg) {
            Ok(p) => p.settings(&reg),
            Err(_) => unreachable_settings(),
        }
    }
}

fn unreachable_settings() -> Settings {
    Settings {
        wind_speed: 9.0,
        wind_direction: 20.0,
        fetch_km: 120.0,
        wave_height: 1.0,
        swell: 0.7,
        swell_direction: 345.0,
        depth: 200.0,
        foam_linger: 1.4,
        glitter: 0.6,
        caustics: 1.0,
        spray: 1.0,
        beach_slope: 0.0,
        beach_distance: 60.0,
        beach_direction: 270.0,
        breaker_height: 0.8,
        breaker_length: 30.0,
        mid_waves: 0.0,
        foam_streaks: 0.0,
        spindrift: 0.0,
        breaking: 0.0,
        wave_field: 0.0,
        foam_debug: 0.0,
        deep: [0.45, 0.74, 0.78],
        clarity: 2.5,
        scatter: [0.08, 0.30, 0.34],
        sss: [0.10, 0.55, 0.42],
        sss_strength: 0.0,
        reflection: 1.0,
        roughness: 0.08,
        glint: 1.0,
        refraction: 1.0,
        whitecaps: 0.0,
        whitecap_threshold: 0.5,
        choppiness: 0.0,
        shore_foam: 0.8,
        speed: 1.0,
        horizon: [0.62, 0.72, 0.82],
        zenith: [0.24, 0.42, 0.72],
    }
}

/// Modified Bessel function I0 (series).
fn bessel_i0(x: f64) -> f64 {
    let mut sum = 1.0;
    let mut term = 1.0;
    let q = x * x * 0.25;
    for k in 1..40 {
        term *= q / f64::from(k * k);
        sum += term;
        if term < 1e-12 * sum {
            break;
        }
    }
    sum
}

/// The crest profile exp(s (sin - 1)) for choppiness `c` (0..1): (s, mean,
/// scale to a sine's variance). The squeeze (0.9 x choppiness) does most
/// of the sharpening; the profile adds a little and flattens troughs.
pub fn profile(choppiness: f32) -> [f32; 3] {
    let s = f64::from((choppiness * 1.2).clamp(0.01, 1.2));
    let m = (-s).exp() * bessel_i0(s);
    let var = (-2.0 * s).exp() * (bessel_i0(2.0 * s) - bessel_i0(s) * bessel_i0(s));
    let k = 1.0 / (2.0 * var.max(1e-12)).sqrt();
    [s as f32, m as f32, k as f32]
}

/// A colour as one float: three sqrt-encoded 8-bit channels (the shader's
/// `unpack_col`).
pub fn pack_col(c: [f32; 3]) -> f32 {
    let q = |v: f32| (v.clamp(0.0, 1.0).sqrt() * 255.0).round() as u32;
    (q(c[0]) + 256 * q(c[1]) + 65536 * q(c[2])) as f32
}

/// The table the shader reads, parsed from the WGSL text (no generation at
/// run time: nothing is baked when the sea starts).
pub fn shader_table() -> Vec<spectrum::Comp> {
    let mut comps = Vec::new();
    let mut phases = Vec::new();
    let mut part = 0;
    for line in TABLE_WGSL.lines() {
        let l = line.trim();
        if l.starts_with("const SEA_C") {
            part = 1;
            continue;
        }
        if l.starts_with("const SEA_P") {
            part = 2;
            continue;
        }
        if l.starts_with(");") {
            part = 0;
            continue;
        }
        let nums = |s: &str| -> Vec<f32> { s.split(',').filter_map(|x| x.trim().trim_end_matches(')').parse::<f32>().ok()).collect() };
        match part {
            1 => {
                if let Some(inner) = l.strip_prefix("vec4<f32>(") {
                    let v = nums(inner);
                    if v.len() == 4 {
                        comps.push(spectrum::Comp { xx: v[0], cos: v[1], sin: v[2], amp: v[3], phase: 0.0 });
                    }
                }
            }
            2 => phases.extend(nums(l)),
            _ => {}
        }
    }
    for (c, p) in comps.iter_mut().zip(phases) {
        c.phase = p;
    }
    comps
}

thread_local! {
    static TABLE: Vec<spectrum::Comp> = shader_table();
}

/// The resolved slope variance the components carry (both axes), for the
/// unresolved roughness: the Cox-Munk mean square slope for the wind less
/// this.
fn resolved_variance(level: usize, kp: [f32; 2], amp: [f32; 2]) -> f32 {
    let level = level.min(3);
    let (b, (w, sw)) = (spectrum::GEO_BASE[level], spectrum::GEO[level]);
    let ids = (b..b + w + sw).chain(spectrum::DET_BASE..spectrum::DET_BASE + spectrum::DET_N[level]);
    TABLE.with(|t| {
        ids.filter_map(|i| t.get(i).map(|c| (i, c)))
            .map(|(i, c)| {
                let s = usize::from(spectrum::is_swell(i));
                let k = kp[s] * c.xx;
                let a = amp[s] * c.amp * (-0.0016 * k * k).exp();
                a * a * k * k * 0.5
            })
            .sum()
    })
}

/// What a frame's uniforms are made of besides the settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameIn {
    pub camera: CameraPose,
    /// Pixels of the target.
    pub size: [f32; 2],
    /// Sim seconds.
    pub time: f64,
    pub sun: [f32; 3],
    pub sun_col: [f32; 3],
    pub sky_tint: f32,
    pub sea: Sea,
    /// 0 Low .. 3 Ultra.
    pub level: usize,
    /// 1 / view distance = inv0 + inv1 * depth.
    pub inv: [f32; 2],
    /// A capture's projection jitter (pixels).
    pub jitter: [f32; 2],
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// The 64 uniform words of a sea draw (the shader's `Sea` struct): the
/// classic sea's, or with the field sea on ([`field::field_on`]) the field
/// sea's ([`field::Field::new`]: these plus its own few).
pub fn uniforms(s: &Settings, fr: &FrameIn) -> [f32; 64] {
    if field::field_on(s) {
        return field::Field::new(s, fr).words;
    }
    classic_uniforms(s, fr)
}

/// The classic sea's 64 uniform words.
pub fn classic_uniforms(s: &Settings, fr: &FrameIn) -> [f32; 64] {
    let mut o = [0.0f32; 64];
    let cam = fr.camera;
    let fwd = norm([cam.target[0] - cam.eye[0], cam.target[1] - cam.eye[1], cam.target[2] - cam.eye[2]]);
    let right = norm(cross(fwd, cam.up));
    let up = cross(right, fwd);
    let tan = (cam.fov_y_deg.to_radians() * 0.5).tan();
    let aspect = fr.size[0] / fr.size[1].max(1.0);
    let kx = 2.0 * fr.jitter[0] / fr.size[0].max(1.0);
    let ky = -2.0 * fr.jitter[1] / fr.size[1].max(1.0);
    let fwd = [0, 1, 2].map(|i| fwd[i] - kx * right[i] * tan * aspect - ky * up[i] * tan);
    let t = fr.time * f64::from(s.speed);
    let tl = (t / spectrum::LOOP_S).rem_euclid(1.0) as f32;
    let (kw, aw) = spectrum::scales(s.wind_speed, s.fetch_km * 1000.0);
    let (ks, as_) = spectrum::scales(spectrum::SWELL_WIND, spectrum::SWELL_FETCH);
    let aw = aw * s.wave_height;
    let as_ = as_ * s.swell.max(0.0).sqrt() * s.wave_height;
    let pr = profile(s.choppiness);
    let v_unres = (0.003 + 0.00512 * s.wind_speed - resolved_variance(fr.level, [kw, ks], [aw, as_])).max(0.0);
    let breakers = 8.0 * s.breaker_length.round().clamp(5.0, 200.0) + s.breaker_height.clamp(0.0, 7.99);
    let rows: [[f32; 4]; 16] = [
        [cam.eye[0], cam.eye[1], cam.eye[2], fr.inv[0]],
        [fwd[0], fwd[1], fwd[2], fr.inv[1]],
        [right[0] * tan * aspect, right[1] * tan * aspect, right[2] * tan * aspect, tl],
        [up[0] * tan, up[1] * tan, up[2] * tan, t as f32],
        [fr.sun[0], fr.sun[1], fr.sun[2], 2.0 * tan / fr.size[1].max(1.0)],
        [fr.sun_col[0], fr.sun_col[1], fr.sun_col[2], fr.sky_tint],
        [fr.sea.centre[0], fr.sea.level, fr.sea.centre[1], fr.level.min(3) as f32],
        [fr.sea.half[0], fr.sea.half[1], v_unres, s.spray],
        [kw, aw, s.wind_direction.to_radians(), ks],
        [as_, s.swell_direction.to_radians(), pr[0], pr[1]],
        [pr[2], s.depth.max(0.5), s.choppiness, s.clarity],
        [pack_col(s.horizon), pack_col(s.zenith), pack_col(s.deep), pack_col(s.scatter)],
        [pack_col(s.sss), s.sss_strength, s.reflection, s.roughness],
        [s.glint, s.glitter, s.refraction, s.caustics],
        [s.whitecaps, s.whitecap_threshold, s.foam_linger, s.shore_foam],
        [s.beach_slope.max(0.0), s.beach_distance, s.beach_direction.to_radians(), breakers],
    ];
    for (i, r) in rows.iter().enumerate() {
        o[i * 4..i * 4 + 4].copy_from_slice(r);
    }
    o
}

/// The sea's post effect (shares its state with the [`SeaSurfaces`]
/// resource).
pub struct SeaEffect {
    pub(crate) sea: SeaSurfaces,
    pub(crate) params: SeaParams,
}

impl PostEffect for SeaEffect {
    fn name(&self) -> &'static str {
        "nature.sea"
    }

    fn pass_names(&self) -> &'static [&'static str] {
        PASSES
    }

    fn shaders(&self) -> &'static [EffectShader] {
        &SHADERS
    }

    fn record(&self, pass: &str, reg: &ParamRegistry, rec: &mut dyn PostRecorder) {
        if pass != PASS || self.sea.get().is_none() {
            return;
        }
        let Some(level) = self.sea.level() else { return };
        let s = self.params.settings(reg);
        draw_effect(self, rec, reg, COPY, Some("hdr"), SCENE, 0);
        draw_effect(self, rec, reg, QUICK, None, "hdr", 0);
        if !field::field_on(&s) {
            draw_effect(self, rec, reg, FULL, Some(SCENE), "hdr", 0);
            return;
        }
        // The field sea: under the water or over a beach its fill first,
        // then this frame's wave textures and the mesh over them.
        if s.beach_slope > 0.0 || self.field_frame(&s, rec.target_size("hdr")).is_some_and(|f| f.eye_under) {
            draw_effect(self, rec, reg, FILL, Some(SCENE), "hdr", 0);
        }
        draw_effect(self, rec, reg, WAVES, None, field::FIELD, 0);
        draw_effect_mesh(self, rec, reg, MESH, SCENE, field::FIELD, "hdr", field::ZBUF, true, field::vertices(level));
        if field::brk_chance(&s) > 0.0 && field::spray_vertices(level) > 0 {
            draw_effect_mesh(self, rec, reg, SPRAY, SCENE, field::FIELD, "hdr", field::ZBUF, false, field::spray_vertices(level));
        }
    }

    fn mesh_shader(&self, shader: usize) -> Option<MeshShader> {
        match shader {
            MESH => Some(MeshShader { field: field::FIELD, depth_write: true }),
            SPRAY => Some(MeshShader { field: field::FIELD, depth_write: false }),
            _ => None,
        }
    }

    fn write_uniforms(&self, shader: &str, reg: &ParamRegistry, frame: &UniformFrame, out: &mut [f32]) -> usize {
        let field = match shader {
            "nature_sea_copy" => return put_words(out, &[0.0; 4]),
            "nature_sea" | "nature_sea_quick" => false,
            "nature_sea_waves" | "nature_sea_field" | "nature_sea_fill" | "nature_sea_spray" => true,
            _ => return 0,
        };
        if field {
            // (The wave textures' own target is the small atlas: the camera
            // words keep the screen's shape.)
            let size = if shader == "nature_sea_waves" { [1280.0, 720.0] } else { frame.target_size };
            let s = self.params.settings(reg);
            return match self.field_frame(&s, size) {
                Some(f) => put_words(out, &f.words),
                None => 0,
            };
        }
        let (Some(sea), Some(level)) = (self.sea.get(), self.sea.level()) else { return 0 };
        let st = &self.sea.0;
        let [inv0, inv1] = inverse_distance(NEAR, FAR);
        #[cfg(feature = "camera-range")]
        let [inv0, inv1] = {
            let _ = [inv0, inv1];
            let (n, f) = evani_render::camera_range::current();
            inverse_distance(n, f)
        };
        #[cfg(feature = "capture")]
        let jitter = st.jitter.get();
        #[cfg(not(feature = "capture"))]
        let jitter = [0.0; 2];
        let fr = FrameIn {
            camera: st.camera.get(),
            size: frame.target_size,
            time: st.time.get(),
            sun: st.sun.get(),
            sun_col: st.sun_col.get(),
            sky_tint: st.sky_tint.get(),
            sea,
            level,
            inv: [inv0, inv1],
            jitter,
        };
        put_words(out, &uniforms(&self.params.settings(reg), &fr))
    }
}

impl SeaEffect {
    /// This frame's field sea for a target of `size` pixels.
    fn field_frame(&self, s: &Settings, size: [f32; 2]) -> Option<field::Field> {
        let (Some(sea), Some(level)) = (self.sea.get(), self.sea.level()) else { return None };
        let st = &self.sea.0;
        let [inv0, inv1] = inverse_distance(NEAR, FAR);
        #[cfg(feature = "camera-range")]
        let [inv0, inv1] = {
            let _ = [inv0, inv1];
            let (n, f) = evani_render::camera_range::current();
            inverse_distance(n, f)
        };
        #[cfg(feature = "capture")]
        let jitter = st.jitter.get();
        #[cfg(not(feature = "capture"))]
        let jitter = [0.0; 2];
        let fr = FrameIn {
            camera: st.camera.get(),
            size,
            time: st.time.get(),
            sun: st.sun.get(),
            sun_col: st.sun_col.get(),
            sky_tint: st.sky_tint.get(),
            sea,
            level,
            inv: [inv0, inv1],
            jitter,
        };
        Some(field::Field::new(s, &fr))
    }
}

// ---- the CPU mirror (tests and tools): the march's height field ------------------

/// The shader's height field and march from a draw's uniform words, in
/// f32 like the GPU (its sines and exponentials may differ in the last
/// bits; the guarantees the tests check don't depend on them).
#[derive(Debug, Clone)]
pub struct Mirror {
    u: [f32; 64],
    gw: Vec<[f32; 4]>,
    gk: Vec<f32>,
    pub h_hi: f32,
    pub h_lo: f32,
    amp_lo: f32,
    slope_k: f32,
    lambda: f32,
    /// The longest waves (indices into `gw`), the envelope's margins and
    /// its slope bound.
    big: Vec<usize>,
    env_m: f32,
    env_b: f32,
    env_l: f32,
}

const W0: f32 = core::f32::consts::TAU / 300.0;
const TAU: f32 = core::f32::consts::TAU;
/// The surf zone's seaward edge (m from the waterline), as the shader's.
const SURF_OUT: f32 = 220.0;

/// Per level: (wind, swell) components the march reads, march steps,
/// refinements (as the shader's `lv`; Low does not march).
pub fn march_budget(level: usize) -> (usize, usize, usize, usize, usize) {
    let (w, s) = spectrum::GEO[level.min(3)];
    match level {
        3 => (w, s, 48, 20, 8),
        2 => (w, s, 40, 16, 7),
        1 => (w, s, 32, 16, 6),
        _ => (w, s, 0, 0, 0),
    }
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Mirror {
    pub fn new(u: &[f32; 64]) -> Self {
        let level = ((u[27].max(0.0) + 0.5) as u32 & 3) as usize;
        let (gw_n, gs_n, ..) = march_budget(level);
        let t = TABLE.with(Clone::clone);
        let (rw, rs) = ((u[34].cos(), u[34].sin()), (u[37].cos(), u[37].sin()));
        let mut m = Mirror { u: *u, gw: Vec::new(), gk: Vec::new(), big: Vec::new(), h_hi: 0.0, h_lo: 0.0, amp_lo: 0.0, slope_k: 0.0, lambda: 0.9 * u[42], env_m: 0.0, env_b: 0.0, env_l: 0.0 };
        let mut amp = 0.0;
        let mut slope = 0.0;
        let (mut big_amp, mut big_slope, mut rest) = (0.0f32, 0.0f32, 0.0f32);
        let base = spectrum::GEO_BASE[level];
        for i in 0..gw_n + gs_n {
            let idx = base + i;
            let c = t[idx];
            let swell = i >= gw_n;
            let (kp, a0, r) = if swell { (u[35], u[36], rs) } else { (u[32], u[33], rw) };
            let k = kp * c.xx;
            let d = [r.0 * c.cos - r.1 * c.sin, r.1 * c.cos + r.0 * c.sin];
            let n = ((9.81 * k).sqrt() / W0).round();
            let turns = (n * u[11]).fract();
            let a = a0 * c.amp * (-0.0016 * k * k).exp();
            m.gw.push([d[0] * k, d[1] * k, a, TAU * (c.phase - turns)]);
            m.gk.push(k);
            amp += a;
            slope += a * k;
            if (!swell && i < 3) || (swell && i < gw_n + 2) {
                m.big.push(m.gw.len() - 1);
                big_amp += a;
                big_slope += a * k;
            } else {
                rest += a;
            }
        }
        let (s, mean, nrm) = (u[38], u[39], u[40]);
        let pmax = (1.0 - mean) * nrm;
        let pmin = (mean - (-2.0 * s).exp()) * nrm;
        let br = m.breaker_bound();
        m.amp_lo = amp * pmin;
        m.h_hi = amp * pmax + br[0];
        m.h_lo = m.amp_lo + br[1];
        let s = s.max(1e-3);
        let y = (-1.0 + (1.0 + 4.0 * s * s).sqrt()) / (2.0 * s);
        m.lambda = m.lambda.min(0.6 / big_slope.max(1e-6));
        let squeeze = 1.0 / (1.0 - m.lambda * big_slope).max(0.2);
        let pslope = s * nrm * (1.0 - y * y).max(0.0).sqrt() * (s * (y - 1.0)).exp();
        m.slope_k = pslope * squeeze;
        m.env_m = rest * pmax + big_slope * pslope * m.lambda * big_amp;
        m.env_b = br[0];
        let dq = if u[60] > 0.0 { 0.45 * u[60] / m.amp_lo.max(1e-4) } else { 0.0 };
        m.env_l = big_slope * pslope + dq * (big_amp * pmax + m.env_m);
        let _ = slope;
        m
    }

    /// The shader's `envelope`: no wave stands higher at p.
    pub fn envelope(&self, p: [f32; 2], fp: f32) -> f32 {
        let q = self.sea_q(self.depth_at(p));
        let mut h = 0.0;
        for &i in &self.big {
            let (c, k) = (self.gw[i], self.gk[i]);
            h += c[2] * (1.0 - smoothstep(0.45, 1.1, k * fp)) * self.prof((c[0] * p[0] + c[1] * p[1] + c[3]).sin());
        }
        q * (h + self.env_m).max(0.0) + if self.in_surf(p) { self.env_b } else { 0.0 }
    }

    /// The shader's `in_surf`: p lies over the surf zone.
    fn in_surf(&self, p: [f32; 2]) -> bool {
        let s = self.shore_s(p);
        self.breakers_on() && (0.0..=SURF_OUT).contains(&s)
    }

    /// The shader's `surf_span`: the ray's stretch over the surf zone.
    fn surf_span(&self, ro: [f32; 3], rd: [f32; 3]) -> (f32, f32) {
        if !self.breakers_on() {
            return (1.0, -1.0);
        }
        let s0 = self.shore_s([ro[0], ro[2]]);
        let l = self.land();
        let ds = -(rd[0] * l[0] + rd[2] * l[1]);
        if ds.abs() < 1e-6 {
            return if (0.0..=SURF_OUT).contains(&s0) { (-1.0e9, 1.0e9) } else { (1.0, -1.0) };
        }
        let (a, b) = (-s0 / ds, (SURF_OUT - s0) / ds);
        (a.min(b), a.max(b))
    }

    fn level(&self) -> usize {
        ((self.u[27].max(0.0) + 0.5) as u32 & 3) as usize
    }

    fn breaker_len(&self) -> f32 {
        (self.u[63] / 8.0).floor()
    }

    fn breaker_h(&self) -> f32 {
        self.u[63] - 8.0 * (self.u[63] / 8.0).floor()
    }

    fn breakers_on(&self) -> bool {
        self.u[60] > 0.0 && self.breaker_h() > 0.0 && self.breaker_len() > 0.0
    }

    fn breaker_bound(&self) -> [f32; 3] {
        if !self.breakers_on() {
            return [0.0; 3];
        }
        let a = (self.breaker_h() * 2.5).min(0.42 * self.u[41]);
        let trains = if self.level() >= 2 { 2.0 } else { 1.0 };
        let k = TAU / (0.61 * self.breaker_len());
        [1.22 * a * trains, 0.78 * a * trains, a * trains * k * 1.5]
    }

    fn land(&self) -> [f32; 2] {
        [self.u[62].cos(), self.u[62].sin()]
    }

    /// Metres seaward of the waterline.
    pub fn shore_s(&self, p: [f32; 2]) -> f32 {
        if self.u[60] <= 0.0 {
            return 1.0e7;
        }
        let l = self.land();
        self.u[61] - ((p[0] - self.u[24]) * l[0] + (p[1] - self.u[26]) * l[1])
    }

    /// Water depth under p.
    pub fn depth_at(&self, p: [f32; 2]) -> f32 {
        if self.u[60] <= 0.0 {
            return self.u[41];
        }
        self.u[41].min(self.shore_s(p) * self.u[60])
    }

    fn sea_q(&self, d: f32) -> f32 {
        (0.45 * d / self.amp_lo.max(1e-4)).clamp(0.0, 1.0)
    }

    fn prof(&self, sn: f32) -> f32 {
        ((self.u[38] * (sn - 1.0)).exp() - self.u[39]) * self.u[40]
    }

    fn travel(&self, s: f32) -> f32 {
        let m = self.u[60].max(1e-4);
        let (sa, sb) = (0.3 / m, 14.0 / m);
        let ss = s.max(0.0);
        ss.min(sa) / (0.3f32 * 9.81).sqrt() + 2.0 * (ss.clamp(sa, sb).sqrt() - sa.sqrt()) / (9.81 * m).sqrt() + (ss - sb).max(0.0) / (14.0f32 * 9.81).sqrt()
    }

    /// The breakers' height at p (the shader's `breakers(.., false).h`,
    /// without the along-shore noise: the noise only shifts phases and
    /// scales amplitudes inside the same bounds, so the worst case is
    /// covered by taking it at its extremes; here the group is 1).
    fn breaker_height_at(&self, p: [f32; 2], group: f32) -> f32 {
        if !self.breakers_on() {
            return 0.0;
        }
        let s = self.shore_s(p);
        if !(0.0..=SURF_OUT).contains(&s) {
            return 0.0;
        }
        let d = s * self.u[60];
        let env = smoothstep(200.0, 80.0, s) * smoothstep(0.0, 0.35, d);
        let shoal = (d.max(0.05) / 14.0).powf(-0.25).clamp(0.3, 2.5);
        let tau = self.travel(s);
        let trains = if self.level() >= 2 { 2 } else { 1 };
        let mut h = 0.0;
        let mut room = 0.42 * d;
        for j in 0..trains {
            let lam = self.breaker_len() * if j == 1 { 0.61 } else { 1.0 };
            let w = ((9.81 * TAU / lam).sqrt() / W0).round().max(1.0) * W0;
            let a = (self.breaker_h() * env * shoal * group * if j == 1 { 0.55 } else { 1.0 }).min(room);
            room -= a.max(0.0);
            if a <= 1e-4 {
                continue;
            }
            let ph = w * tau + TAU * ((w / W0).round() * self.u[11]).fract() + 2.1 * j as f32;
            let c = 0.5 + 0.5 * ph.cos();
            h += a * (2.0 * c.max(1e-4).powf(1.6) - 0.78);
        }
        h
    }

    /// The sea's height above its level at p for a footprint fp (m).
    pub fn height(&self, p: [f32; 2], fp: f32) -> f32 {
        let q = self.sea_q(self.depth_at(p));
        // Undo the squeeze (one fixed-point step), as the shader does.
        let mut p0 = p;
        if self.lambda > 0.0 {
            let mut d = [0.0f32; 2];
            for &i in &self.big {
                let (c, k) = (self.gw[i], self.gk[i]);
                let w = 1.0 - smoothstep(0.45, 1.1, k * fp);
                let s = c[2] * w * (c[0] * p[0] + c[1] * p[1] + c[3]).cos() / k.max(1e-6);
                d[0] += c[0] * s;
                d[1] += c[1] * s;
            }
            p0 = [p[0] - self.lambda * q * d[0], p[1] - self.lambda * q * d[1]];
        }
        let mut h = 0.0;
        for (c, k) in self.gw.iter().zip(&self.gk) {
            let w = 1.0 - smoothstep(0.45, 1.1, k * fp);
            if w <= 0.0 {
                continue;
            }
            let th = c[0] * p0[0] + c[1] * p0[1] + c[3];
            h += c[2] * w * self.prof(th.sin());
        }
        h * q + self.breaker_height_at(p, 1.0)
    }

    /// The lowest the sea can reach under p (every component in its
    /// trough, the breakers too), for the bed test.
    pub fn trough_bound(&self, p: [f32; 2]) -> f32 {
        let d = self.depth_at(p);
        let q = self.sea_q(d);
        let br = if self.in_surf(p) {
            let trains = if self.level() >= 2 { 1.55 } else { 1.0 };
            0.78 * (self.breaker_h() * 2.5 * trains).min(0.42 * d.max(0.0))
        } else {
            0.0
        };
        -(self.amp_lo * q + br)
    }

    /// The shader's `slope_bound`: the slope bound of what footprint fp keeps.
    pub fn slope_bound(&self, fp: f32) -> f32 {
        let s: f32 = self.gw.iter().zip(&self.gk).map(|(c, k)| c[2] * k * (1.0 - smoothstep(0.45, 1.1, k * fp))).sum();
        s * self.slope_k
    }

    fn foot(&self, t: f32) -> f32 {
        (t * self.u[19]).max(1e-4)
    }

    fn above(&self, ro: [f32; 3], rd: [f32; 3], t: f32) -> f32 {
        let q = [ro[0] + rd[0] * t, ro[1] + rd[1] * t, ro[2] + rd[2] * t];
        q[1] - self.u[25] - self.height([q[0], q[2]], self.foot(t))
    }

    /// The shader's `march`: the first t (m along unit `rd`) where the ray
    /// meets the sea before `tmax`, or -1.
    pub fn march(&self, ro: [f32; 3], rd: [f32; 3], tmax: f32) -> f32 {
        self.march_bracket(ro, rd, tmax).0
    }

    /// [`Mirror::march`] with the final bracket `[ta, tb]` (above the waves
    /// at `ta`, at or under them at `tb`: a crossing lies between).
    pub fn march_bracket(&self, ro: [f32; 3], rd: [f32; 3], tmax: f32) -> (f32, f32, f32) {
        let (.., coarse, steps, bisect) = march_budget(self.level());
        let top = self.u[25] + self.h_hi + 0.02;
        let bot = self.u[25] - self.h_lo - 0.02;
        let mut t0 = 0.0;
        if ro[1] > top {
            if rd[1] >= 0.0 {
                return (-1.0, 0.0, 0.0);
            }
            t0 = (top - ro[1]) / rd[1];
        }
        let mut t1 = tmax;
        if rd[1] < 0.0 {
            t1 = t1.min((bot - ro[1]) / rd[1]);
        } else if rd[1] > 0.0 {
            t1 = t1.min((top - ro[1]) / rd[1]);
        }
        t1 = t1.min(6.0e4);
        if t1 <= t0 {
            return (-1.0, 0.0, 0.0);
        }
        let lat = (rd[0] * rd[0] + rd[2] * rd[2]).sqrt();
        // Stage 1: to the envelope.
        let fe = |t: f32| {
            let q = [ro[0] + rd[0] * t, ro[1] + rd[1] * t, ro[2] + rd[2] * t];
            q[1] - self.u[25] - self.envelope([q[0], q[2]], self.foot(t))
        };
        let span = self.surf_span(ro, rd);
        let (mut ta, mut tlast) = (t0, t0);
        for _ in 0..coarse {
            let e = fe(ta);
            if e <= 0.0 {
                ta = tlast;
                break;
            }
            if e < 0.03 + 2.0 * self.foot(ta) {
                break;
            }
            tlast = ta;
            let mut nt = ta + (e / (rd[1].abs() + self.env_l * lat).max(0.02)).max(0.002 * ta + 0.01);
            if ta < span.0 && nt > span.0 {
                nt = span.0;
            }
            ta = nt;
            if ta >= t1 {
                if fe(t1) > 0.0 {
                    return (-1.0, 0.0, 0.0);
                }
                ta = tlast;
                break;
            }
        }
        if fe(ta) <= 0.0 {
            ta = tlast;
        }
        let mut fa = self.above(ro, rd, ta);
        if fa <= 0.0 {
            return if ta > 0.0 { (ta, ta, ta) } else { (-1.0, 0.0, 0.0) };
        }
        let sb = self.slope_bound(self.foot(ta));
        let sbb = self.breaker_bound()[2];
        let (mut tb, mut fb) = (ta, fa);
        let mut found = false;
        let (mut tp, mut fpv) = (ta, fa);
        for _ in 0..steps {
            if fa < 0.004 + 1.5 * self.foot(ta) {
                return (ta, ta, ta);
            }
            let surf = ta >= span.0 - (0.01 + 1e-4 * ta) && ta <= span.1;
            let safe = fa / (rd[1].abs() + (sb + if surf { sbb } else { 0.0 }) * lat).max(0.02);
            let mut step = safe;
            let closing = fpv - fa;
            if closing > 0.0 {
                let guess = fa * (ta - tp) / closing * 1.05;
                // The shader's `reach_cap`.
                step = guess.clamp(safe, safe * 8.0 + 0.004 * ta);
            }
            step = step.max(0.004 * ta + 0.02);
            if !surf && ta < span.0 {
                step = step.min(span.0 - ta);
            }
            tp = ta;
            fpv = fa;
            tb = (ta + step).min(t1);
            fb = self.above(ro, rd, tb);
            if fb <= 0.0 {
                found = true;
                break;
            }
            if tb >= t1 {
                return (-1.0, 0.0, 0.0);
            }
            ta = tb;
            fa = fb;
        }
        if !found {
            tb = t1;
            fb = self.above(ro, rd, tb);
            if fb > 0.0 {
                return (-1.0, 0.0, 0.0);
            }
        }
        // Illinois regula falsi: the secant, with the end kept twice in a
        // row given half its weight, so the bracket keeps shrinking.
        let (mut wa, mut wb, mut side) = (fa, fb, 0);
        let n = bisect + if found { 0 } else { 8 };
        for i in 0..n {
            let f = if i % 2 == 1 || i >= bisect { 0.5 } else { (wa / (wa - wb).max(1e-6)).clamp(0.05, 0.95) };
            let tm = ta + (tb - ta) * f;
            let fm = self.above(ro, rd, tm);
            if fm > 0.0 {
                ta = tm;
                fa = fm;
                wa = fm;
                if side == 1 {
                    wb *= 0.5;
                }
                side = 1;
            } else {
                tb = tm;
                fb = fm;
                wb = fm;
                if side == -1 {
                    wa *= 0.5;
                }
                side = -1;
            }
        }
        (ta + (tb - ta) * (fa / (fa - fb).max(1e-6)).clamp(0.0, 1.0), ta, tb)
    }

    /// The shader's `eye_under`: the eye is inside a wave (the pass then
    /// draws the view from under the water, never a plane).
    pub fn eye_under(&self, eye: [f32; 3]) -> bool {
        self.level() >= 1 && eye[1] < self.u[25] + self.height([eye[0], eye[2]], 0.05)
    }

    /// How far above the sea the point at t on the ray is (0 on the waves).
    pub fn gap(&self, ro: [f32; 3], rd: [f32; 3], t: f32) -> f32 {
        self.above(ro, rd, t)
    }
}
