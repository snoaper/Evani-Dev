//! The sky dome: a Preetham daylight sky with drifting cumulus, drawn as
//! one large unlit sphere round the camera whose surface code (the `SKY`
//! slot of `shaders/material.wgsl`) colours each pixel from its view
//! direction. Ported from three.js `examples/jsm/objects/Sky.js` (MIT,
//! Copyright © 2010-2026 three.js authors), after A. J. Preetham, P.
//! Shirley and B. Smits, "A Practical Analytic Model for Daylight"
//! (SIGGRAPH 1999).
//!
//! The view direction needs no camera data: the dome's vertex normals are
//! its positions over its radius, and the forward pass interpolates them
//! linearly, so the normal the surface code gets is exactly the direction
//! from the dome's centre (the eye) to the pixel.
//!
//! [`SkyParams::radiance`] is the same sky (without clouds) on the CPU, so a
//! look can match its ambient tints and its haze colour to the dome.
//!
//! Live params ([`SkyHandles`]): `sky.turbidity`, `sky.rayleigh`,
//! `sky.mie_coefficient`, `sky.mie_directional_g`, `sky.cloud_scale`,
//! `sky.cloud_speed`, `sky.cloud_coverage`, `sky.cloud_density`,
//! `sky.cloud_elevation`, `sky.brightness`, `sky.sun_disc`, `sky.visible`
//! (draw the dome at all) and `sky.open_below` (leave the dome open under
//! the horizon, so a sea drawn over the depth afterwards, such as
//! evani-material-nature's open ocean, reaches the skyline instead of being
//! cut off where the view meets the dome); and the sun:
//! `sky.follow_light` (default on: the dome's sun is wherever
//! `render.light.direction` points, so the disc sits where the shadows come
//! from), else `sky.sun_elevation` and `sky.sun_azimuth` (degrees).
//!
//! Feature `sky-horizon` adds [`CloudHorizon`] (`sky.cloud_horizon_low`,
//! `_high`, `_curve` and `_haze`): where the clouds fade in over the
//! horizon, how much the layer curves away and how far the haze hides it,
//! for cameras that look mostly at the skyline. Its defaults draw the same
//! clouds as without the feature.

/// The look material the dome is drawn through (unlit, white).
pub const SKY_MATERIAL: &str = "sky";
/// The dome's radius (m): far inside the camera's 1000 m far plane, far
/// outside the sun shadow map's depth range (it casts nothing).
pub const RADIUS: f32 = 500.0;
/// The first `kit_data` record of the sky's parameters (four records).
pub const RECORD: usize = 3;
/// How often the cloud drift time is re-sent (Hz). Every send uploads the
/// whole `kit_data` buffer, and the drift is far slower than a pixel per
/// quarter second.
pub const TIME_HZ: f64 = 4.0;

/// The sky's look (three.js Sky.js parameters, plus a brightness).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkyParams {
    /// Haze: 2 is a clear blue sky, 10 a milky one.
    pub turbidity: f32,
    /// Molecular (blue) scattering.
    pub rayleigh: f32,
    /// Aerosol scattering and how forward it leans (the glow round the sun).
    pub mie_coefficient: f32,
    pub mie_directional_g: f32,
    /// Cloud size (bigger = smaller clouds), drift speed, how much of the sky
    /// they cover (0..1) and how solid they are (0..1).
    pub cloud_scale: f32,
    pub cloud_speed: f32,
    pub cloud_coverage: f32,
    pub cloud_density: f32,
    /// How fast the curved cloud layer recedes towards the horizon (0..1;
    /// higher reads as lower, closer clouds that shrink sooner).
    pub cloud_elevation: f32,
    /// Multiplies the whole sky (three.js leaves this to its exposure).
    pub brightness: f32,
    /// Draw the sun disc.
    pub sun_disc: bool,
    /// Draw the dome at all (off: nothing is drawn, the records stay).
    pub visible: bool,
    /// Leave the dome open below the horizon: those fragments are dropped,
    /// so the depth there stays at the far plane and a sea drawn later
    /// treats it as open sky out to the horizon.
    pub open_below: bool,
    /// The clouds near the horizon (feature `sky-horizon`).
    #[cfg(feature = "sky-horizon")]
    pub horizon: CloudHorizon,
}

/// How the clouds meet the horizon (feature `sky-horizon`). Heights are on
/// the dome: the sine of the elevation (0 the horizon, 1 the zenith).
#[cfg(feature = "sky-horizon")]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CloudHorizon {
    /// Where the clouds start to show over the horizon, and where they are
    /// full (the puff detail follows the same band, a little higher).
    pub fade_low: f32,
    pub fade_high: f32,
    /// Scales the cloud layer's curve: below 1 the layer bends away less,
    /// so clouds low in the sky stay tall heaps instead of thin streaks.
    pub curve: f32,
    /// How far the air hides the clouds: 1 as the sky's own extinction
    /// (far clouds melt into the horizon glow), lower keeps them distinct.
    pub haze: f32,
    /// The clouds' own look (record [`crate::layout::SKY_CLOUD_LOOK`]): how
    /// strongly each small lobe inside a cloud shades like a ball (0.3;
    /// lower: broad, photographic shading instead of a scaly texture), how
    /// wide the soft band at a cloud's outline is (0.16; lower: crisper
    /// edges), how deeply the lobes bite into the outline (1: cauliflower
    /// edges as drawn; higher: deeper) and the clouds' brightness (1).
    pub lobe_shade: f32,
    pub edge: f32,
    pub bite: f32,
    pub gain: f32,
    /// The clear sky's tone (record [`crate::layout::SKY_TONE`]): a colour
    /// multiplier on the scattered light (1, 1, 1: as the model gives it)
    /// and how much the sky deepens towards the zenith (0: none; 0.4: the
    /// zenith at 60 percent, as a clear photographed sky's deep top).
    pub tint: [f32; 3],
    pub deep: f32,
    /// How the clouds are lit (record [`crate::layout::SKY_CLOUD_LIGHT`]):
    /// how far the lit side leans along the view, away from the eye (0.8;
    /// below 0 it leans towards the eye, which puts the light on the
    /// crowns, the upper side on screen, of clouds seen from the ground),
    /// and how strongly each heap shades like a ball (0.4).
    pub up_bias: f32,
    pub body_shade: f32,
    /// The clouds as a real volume (off: the flat clouds above).
    pub volume: CloudVolume,
}

/// The clouds as a real volume (feature `sky-horizon`; `on` false by
/// default, so the dome draws its flat clouds as before): fair-weather
/// cumulus in a layer `base` km up and `depth` km thick, marched along each
/// view ray (`shaders/sky_cumulus.wgsl`). Each heap's footprint comes from
/// a seeded field over the layer (separate heaps about a third to half of
/// `size` km across, clusters and clear stretches, `seed` picks the
/// arrangement; `sky.cloud_coverage` sets how many); its height rises from
/// a thin rim to a rounded crown, billows heap the crown and fray the rims,
/// the base stays flat. Lit by the sun through the cloud (multiple
/// scattering), the sky from above and the ground from below; the clouds'
/// haze (`sky.cloud_horizon_haze`) hides far ones into the horizon. A faint
/// high cirrus layer (`cirrus`) lies behind them. Coverage, density, speed
/// and the clouds' gain are the dome's own params.
#[cfg(feature = "sky-horizon")]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CloudVolume {
    pub on: bool,
    pub seed: f32,
    /// The layer's base and depth (km), the heaps' field size (km).
    pub base: f32,
    pub depth: f32,
    pub size: f32,
    /// Faint cirrus streaks far up (0 none, 1 most).
    pub cirrus: f32,
    /// How high the billows heap (1.4), how soft the surface is (0.05,
    /// a share of the depth), how dark the base is under its sky light
    /// (0.2: a fifth of the crown's) and how much the sunlit outer skin
    /// dims (0.25).
    pub billow: f32,
    pub soft: f32,
    pub base_shade: f32,
    pub powder: f32,
}

#[cfg(feature = "sky-horizon")]
impl Default for CloudVolume {
    fn default() -> Self {
        CloudVolume {
            on: false,
            seed: 0.0,
            base: 1.4,
            depth: 0.9,
            size: 3.2,
            cirrus: 0.0,
            billow: 1.4,
            soft: 0.05,
            base_shade: 0.2,
            powder: 0.25,
        }
    }
}

#[cfg(feature = "sky-horizon")]
impl CloudVolume {
    /// The layer record ([`crate::layout::SKY_CUMULUS`]).
    pub fn record(&self) -> [f32; 4] {
        [
            self.base.max(0.1),
            self.depth.max(0.05),
            self.size.max(0.1),
            self.cirrus.clamp(0.0, 1.0),
        ]
    }

    /// The look record ([`crate::layout::SKY_CUMULUS_LOOK`]).
    pub fn look_record(&self) -> [f32; 4] {
        [
            self.billow.max(0.0),
            self.soft.max(0.01),
            self.base_shade.clamp(0.0, 1.0),
            self.powder.clamp(0.0, 1.0),
        ]
    }
}

#[cfg(feature = "sky-horizon")]
impl Default for CloudHorizon {
    /// The clouds as the dome draws them without the feature.
    fn default() -> Self {
        CloudHorizon {
            fade_low: 0.03,
            fade_high: 0.27,
            curve: 1.0,
            haze: 1.0,
            lobe_shade: 0.3,
            edge: 0.16,
            bite: 1.0,
            gain: 1.0,
            tint: [1.0; 3],
            deep: 0.0,
            up_bias: 0.8,
            body_shade: 0.4,
            volume: CloudVolume::default(),
        }
    }
}

#[cfg(feature = "sky-horizon")]
impl CloudHorizon {
    /// The `kit_data` record ([`crate::layout::SKY_HORIZON`]): the fade band
    /// (its top kept above its bottom), the curve and the haze.
    pub fn record(&self) -> [f32; 4] {
        let low = self.fade_low.max(0.0);
        [
            low,
            self.fade_high.max(low + 0.001),
            self.curve.max(0.0),
            self.haze.max(0.0),
        ]
    }

    /// The clouds' look record ([`crate::layout::SKY_CLOUD_LOOK`]).
    pub fn look_record(&self) -> [f32; 4] {
        [
            self.lobe_shade.max(0.0),
            self.edge.max(0.005),
            self.bite.max(0.0),
            self.gain.max(0.0),
        ]
    }

    /// The clear sky's tone record ([`crate::layout::SKY_TONE`]).
    pub fn tone_record(&self) -> [f32; 4] {
        [
            self.tint[0].max(0.0),
            self.tint[1].max(0.0),
            self.tint[2].max(0.0),
            self.deep.clamp(0.0, 0.95),
        ]
    }

    /// The clouds' light record ([`crate::layout::SKY_CLOUD_LIGHT`]), with
    /// the cloud volume's switch and seed.
    pub fn light_record(&self) -> [f32; 4] {
        [
            self.up_bias,
            self.body_shade.max(0.0),
            f32::from(u8::from(self.volume.on)),
            self.volume.seed,
        ]
    }

    /// The tone's factor on the scattered light looking along a direction
    /// `y` up (the sine of the elevation), per channel.
    pub fn tone(&self, y: f32) -> [f32; 3] {
        let r = self.tone_record();
        let y = y.max(0.0);
        let k = 1.0 - r[3] * y * y;
        [r[0] * k, r[1] * k, r[2] * k]
    }

    /// How much cloud shows at height `y` (the shader's horizon fade, 0..1).
    pub fn fade(&self, y: f32) -> f32 {
        let r = self.record();
        smoothstep(r[0], r[1], y)
    }

    /// How much puff detail shows at height `y` (0..1).
    pub fn detail(&self, y: f32) -> f32 {
        let r = self.record();
        smoothstep(r[0] + 0.07, r[1] + 0.13, y)
    }
}

#[cfg(feature = "sky-horizon")]
fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Default for SkyParams {
    /// A clear, deep blue afternoon sky with fair-weather clouds.
    fn default() -> Self {
        SkyParams {
            turbidity: 3.0,
            rayleigh: 1.0,
            mie_coefficient: 0.005,
            mie_directional_g: 0.8,
            cloud_scale: 0.0012,
            cloud_speed: 0.00002,
            cloud_coverage: 0.4,
            cloud_density: 0.5,
            cloud_elevation: 0.5,
            brightness: 1.25,
            sun_disc: true,
            visible: true,
            open_below: false,
            #[cfg(feature = "sky-horizon")]
            horizon: CloudHorizon::default(),
        }
    }
}

impl SkyParams {
    /// The four `kit_data` records from [`RECORD`]: the sun direction
    /// (normalised; towards the sun) and the drift time in seconds, then the
    /// parameters.
    pub fn records(&self, sun: [f32; 3], time: f32) -> [[f32; 4]; 4] {
        let s = norm(sun);
        [
            [s[0], s[1], s[2], time],
            [
                self.turbidity,
                self.rayleigh,
                self.mie_coefficient,
                self.mie_directional_g,
            ],
            [
                self.cloud_scale,
                self.cloud_speed,
                self.cloud_coverage,
                self.cloud_density,
            ],
            [
                self.cloud_elevation,
                self.brightness,
                f32::from(u8::from(self.sun_disc)),
                f32::from(u8::from(self.open_below)),
            ],
        ]
    }

    /// The clear sky's colour (linear HDR, as the dome draws it before
    /// exposure) looking along `dir` with the sun towards `sun`: the shader's
    /// sky without its clouds and sun disc.
    pub fn radiance(&self, sun: [f32; 3], dir: [f32; 3]) -> [f32; 3] {
        let (s, d) = (norm(sun), norm(dir));
        // Sun intensity by its height (the "earth shadow" cut-off).
        let zc = s[1].clamp(-1.0, 1.0);
        let sun_e = EE * (1.0 - (-(CUTOFF - zc.acos()) / STEEPNESS).exp()).max(0.0);
        let beta_r = TOTAL_RAYLEIGH.map(|v| v * self.rayleigh);
        let c = 0.2 * self.turbidity * 10e-18;
        let beta_m = MIE_CONST.map(|v| 0.434 * c * v * self.mie_coefficient);
        // Optical length (zenith angle capped at 90 degrees).
        let za = d[1].max(0.0).acos();
        let inv = 1.0 / (za.cos() + 0.15 * (93.885 - za.to_degrees()).powf(-1.253));
        let (sr, sm) = (RAYLEIGH_ZENITH * inv, MIE_ZENITH * inv);
        let fex: [f32; 3] = std::array::from_fn(|i| (-(beta_r[i] * sr + beta_m[i] * sm)).exp());
        let cos_t = d[0] * s[0] + d[1] * s[1] + d[2] * s[2];
        let r_phase = rayleigh_phase(cos_t * 0.5 + 0.5);
        let m_phase = hg_phase(cos_t, self.mie_directional_g);
        let fade = (1.0 - s[1]).powi(5).clamp(0.0, 1.0);
        #[cfg(feature = "sky-horizon")]
        let tone = self.horizon.tone(d[1]);
        #[cfg(not(feature = "sky-horizon"))]
        let tone = [1.0f32; 3];
        std::array::from_fn(|i| {
            let ratio = (beta_r[i] * r_phase + beta_m[i] * m_phase) / (beta_r[i] + beta_m[i]);
            let lin = (sun_e * ratio * (1.0 - fex[i])).powf(1.5);
            let lin = lin * (1.0 + ((sun_e * ratio * fex[i]).sqrt() - 1.0) * fade);
            let l0 = 0.1 * fex[i];
            ((lin + l0) * 0.04 * tone[i] + NIGHT[i]) * self.brightness
        })
    }

    /// The clear sky's mean colour over `n` directions round the horizon at
    /// height `y` (the sine of the elevation).
    pub fn ring(&self, sun: [f32; 3], y: f32, n: usize) -> [f32; 3] {
        let r = (1.0 - y * y).max(0.0).sqrt();
        let mut sum = [0.0f32; 3];
        for k in 0..n.max(1) {
            let a = k as f32 / n.max(1) as f32 * std::f32::consts::TAU;
            let c = self.radiance(sun, [r * a.cos(), y, r * a.sin()]);
            for (s, v) in sum.iter_mut().zip(c) {
                *s += v;
            }
        }
        sum.map(|v| v / n.max(1) as f32)
    }
}

const EE: f32 = 1000.0;
const CUTOFF: f32 = 1.611_073_2;
const STEEPNESS: f32 = 1.5;
const TOTAL_RAYLEIGH: [f32; 3] = [5.804_543e-6, 1.356_291_1e-5, 3.026_590_2e-5];
const MIE_CONST: [f32; 3] = [1.839_991_9e14, 2.779_802_4e14, 4.079_048e14];
const RAYLEIGH_ZENITH: f32 = 8.4e3;
const MIE_ZENITH: f32 = 1.25e3;
const NIGHT: [f32; 3] = [0.0, 0.0003, 0.00075];

fn rayleigh_phase(c: f32) -> f32 {
    0.059_683_1 * (1.0 + c * c)
}

fn hg_phase(c: f32, g: f32) -> f32 {
    let g2 = g * g;
    0.079_577_47 * (1.0 - g2) / (1.0 - 2.0 * g * c + g2).powf(1.5)
}

fn norm(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 1e-6 {
        v.map(|c| c / l)
    } else {
        [0.0, 1.0, 0.0]
    }
}

/// The direction towards the sun at `elevation` degrees above the horizon
/// and `azimuth` degrees from +z towards +x.
pub fn sun_direction(elevation: f32, azimuth: f32) -> [f32; 3] {
    let (e, a) = (elevation.to_radians(), azimuth.to_radians());
    [e.cos() * a.sin(), e.sin(), e.cos() * a.cos()]
}

use evani_core::params::{BoolParam, F32Param, ParamDef, ParamError, ParamRegistry};
#[cfg(feature = "sky-horizon")]
use evani_core::params::Vec3Param;

/// The sun the dome uses while `sky.follow_light` is off: elevation and
/// azimuth (degrees).
pub const SUN_DEFAULT: (f32, f32) = (35.0, 56.0);

/// `(suffix, min, max, step, description)` per float param, in
/// [`SkyParams::floats`] order.
const FLOATS: [(&str, f32, f32, f32, &str); 10] = [
    (
        "turbidity",
        1.0,
        20.0,
        0.1,
        "Haze: 2 is a clear blue sky, 10 a milky one",
    ),
    ("rayleigh", 0.0, 4.0, 0.05, "Molecular (blue) scattering"),
    (
        "mie_coefficient",
        0.0,
        0.1,
        0.001,
        "Aerosol scattering (the glow round the sun)",
    ),
    (
        "mie_directional_g",
        0.0,
        0.999,
        0.01,
        "How forward the aerosol glow leans",
    ),
    (
        "cloud_scale",
        0.0002,
        0.01,
        0.0001,
        "Cloud size (bigger = smaller clouds)",
    ),
    ("cloud_speed", 0.0, 0.001, 0.00001, "Cloud drift speed"),
    (
        "cloud_coverage",
        0.0,
        1.0,
        0.01,
        "How much of the sky the clouds cover",
    ),
    ("cloud_density", 0.0, 1.0, 0.01, "How solid the clouds are"),
    (
        "cloud_elevation",
        0.0,
        1.0,
        0.01,
        "How fast the cloud layer recedes towards the horizon",
    ),
    ("brightness", 0.0, 4.0, 0.01, "Multiplies the whole sky"),
];

impl SkyParams {
    /// The float params' values, in [`FLOATS`] order.
    pub fn floats(&self) -> [f32; 10] {
        [
            self.turbidity,
            self.rayleigh,
            self.mie_coefficient,
            self.mie_directional_g,
            self.cloud_scale,
            self.cloud_speed,
            self.cloud_coverage,
            self.cloud_density,
            self.cloud_elevation,
            self.brightness,
        ]
    }

    /// This sky with the float params set from `v` ([`Self::floats`] order).
    pub fn with_floats(mut self, v: [f32; 10]) -> Self {
        [
            self.turbidity,
            self.rayleigh,
            self.mie_coefficient,
            self.mie_directional_g,
            self.cloud_scale,
            self.cloud_speed,
            self.cloud_coverage,
            self.cloud_density,
            self.cloud_elevation,
            self.brightness,
        ] = v;
        self
    }
}

/// The registry handles of the sky params.
#[derive(Debug, Clone, Copy)]
pub struct SkyHandles {
    f: [F32Param; 10],
    sun_disc: BoolParam,
    visible: BoolParam,
    open_below: BoolParam,
    follow_light: BoolParam,
    sun_elevation: F32Param,
    sun_azimuth: F32Param,
    #[cfg(feature = "sky-horizon")]
    horizon: [F32Param; 11],
    #[cfg(feature = "sky-horizon")]
    tint: Vec3Param,
    #[cfg(feature = "sky-horizon")]
    volume_on: BoolParam,
    #[cfg(feature = "sky-horizon")]
    volume: [F32Param; 9],
}

/// `(suffix, min, max, step, description)` per cloud volume param, in
/// [`CloudVolume`] field order after `on` (feature `sky-horizon`).
#[cfg(feature = "sky-horizon")]
const VOLUME: [(&str, f32, f32, f32, &str); 9] = [
    ("cloud_seed", 0.0, 1000.0, 0.01, "Which arrangement of cloud heaps the volume draws (any number)"),
    ("cloud_base", 0.2, 5.0, 0.05, "The cloud volume's flat base (km up)"),
    ("cloud_depth", 0.1, 4.0, 0.05, "How tall the volume's tallest heaps are (km)"),
    ("cloud_size", 0.3, 20.0, 0.1, "The volume's heap field size (km; heaps a third to half of it across)"),
    ("cirrus", 0.0, 1.0, 0.01, "Faint cirrus streaks far up, behind the volume's heaps (0: none)"),
    ("cloud_billow", 0.0, 3.0, 0.01, "How high the volume's billows heap on the crowns (1.4)"),
    ("cloud_soft", 0.01, 0.5, 0.005, "How soft the volume's cloud surface is (0.05)"),
    ("cloud_base_shade", 0.0, 1.0, 0.01, "How much sky light reaches the volume's flat bases (0.2)"),
    ("cloud_powder", 0.0, 1.0, 0.01, "How much the sunlit outer skin of the volume's heaps dims (0.25)"),
];

/// `(suffix, min, max, step, description)` per horizon param, in
/// [`CloudHorizon::record`] order (feature `sky-horizon`).
#[cfg(feature = "sky-horizon")]
const HORIZON: [(&str, f32, f32, f32, &str); 11] = [
    (
        "cloud_horizon_low",
        0.0,
        0.5,
        0.005,
        "Where the clouds start to show over the horizon (sine of the elevation)",
    ),
    (
        "cloud_horizon_high",
        0.0,
        0.8,
        0.005,
        "Where the clouds are full over the horizon (sine of the elevation)",
    ),
    (
        "cloud_horizon_curve",
        0.0,
        2.0,
        0.01,
        "How much the cloud layer bends away near the horizon (below 1: taller low clouds)",
    ),
    (
        "cloud_horizon_haze",
        0.0,
        2.0,
        0.01,
        "How far the air hides the clouds near the horizon (1: as the sky's own haze)",
    ),
    (
        "cloud_lobe_shade",
        0.0,
        1.0,
        0.01,
        "How strongly each small lobe inside a cloud shades like a ball (0.3; lower: broad, photographic shading)",
    ),
    (
        "cloud_edge",
        0.005,
        0.5,
        0.005,
        "How wide the soft band at a cloud's outline is (0.16; lower: crisper edges)",
    ),
    (
        "cloud_bite",
        0.0,
        3.0,
        0.01,
        "How deeply the lobes bite into a cloud's outline (1; higher: deeper cauliflower edges)",
    ),
    (
        "cloud_gain",
        0.0,
        3.0,
        0.01,
        "The clouds' brightness (1)",
    ),
    (
        "zenith_deep",
        0.0,
        0.95,
        0.01,
        "How much the clear sky deepens towards the zenith (0: as the model gives it)",
    ),
    (
        "cloud_up_bias",
        -2.0,
        2.0,
        0.01,
        "Where the clouds' lit side leans along the view (0.8: away from the eye; below 0: the crowns, as clouds seen from the ground)",
    ),
    (
        "cloud_body_shade",
        0.0,
        2.0,
        0.01,
        "How strongly each cloud heap shades like a ball (0.4)",
    ),
];

impl SkyHandles {
    /// Register the sky params, starting at `start` (owner `material-proc`).
    pub fn register(reg: &mut ParamRegistry, start: &SkyParams) -> Result<Self, ParamError> {
        let owner = crate::params::OWNER;
        let v = start.floats();
        let mut f = Vec::with_capacity(FLOATS.len());
        for (i, (suffix, min, max, step, describe)) in FLOATS.into_iter().enumerate() {
            f.push(
                reg.register_f32(
                    ParamDef::f32(&format!("sky.{suffix}"), v[i])
                        .range(min, max)
                        .step(step)
                        .describe(describe)
                        .owner(owner),
                )?,
            );
        }
        let f: [F32Param; 10] = f
            .try_into()
            .map_err(|_| ParamError::Unknown("sky".into()))?;
        let sun_disc = reg.register_bool(
            ParamDef::bool("sky.sun_disc", start.sun_disc)
                .describe("Draw the sun disc")
                .owner(owner),
        )?;
        let visible = reg.register_bool(
            ParamDef::bool("sky.visible", start.visible)
                .describe("Draw the sky dome")
                .owner(owner),
        )?;
        let open_below = reg.register_bool(
            ParamDef::bool("sky.open_below", start.open_below)
                .describe("Leave the dome open below the horizon (for a sea drawn over the depth afterwards)")
                .owner(owner),
        )?;
        let follow_light = reg.register_bool(
            ParamDef::bool("sky.follow_light", true)
                .describe("The dome's sun follows render.light.direction (off: sky.sun_elevation and sky.sun_azimuth)")
                .owner(owner),
        )?;
        let sun_elevation = reg.register_f32(
            ParamDef::f32("sky.sun_elevation", SUN_DEFAULT.0)
                .range(-20.0, 90.0)
                .step(0.5)
                .unit("deg")
                .describe("The dome's sun height while it doesn't follow the light")
                .owner(owner),
        )?;
        let sun_azimuth = reg.register_f32(
            ParamDef::f32("sky.sun_azimuth", SUN_DEFAULT.1)
                .range(-180.0, 180.0)
                .step(1.0)
                .unit("deg")
                .describe(
                    "The dome's sun bearing (from +z towards +x) while it doesn't follow the light",
                )
                .owner(owner),
        )?;
        #[cfg(feature = "sky-horizon")]
        let horizon = {
            let h = start.horizon;
            let v = [
                h.fade_low,
                h.fade_high,
                h.curve,
                h.haze,
                h.lobe_shade,
                h.edge,
                h.bite,
                h.gain,
                h.deep,
                h.up_bias,
                h.body_shade,
            ];
            let mut out = Vec::with_capacity(HORIZON.len());
            for (i, (suffix, min, max, step, describe)) in HORIZON.into_iter().enumerate() {
                out.push(
                    reg.register_f32(
                        ParamDef::f32(&format!("sky.{suffix}"), v[i])
                            .range(min, max)
                            .step(step)
                            .describe(describe)
                            .owner(owner),
                    )?,
                );
            }
            let out: [F32Param; 11] = out
                .try_into()
                .map_err(|_| ParamError::Unknown("sky.cloud_horizon".into()))?;
            out
        };
        #[cfg(feature = "sky-horizon")]
        let tint = reg.register_vec3(
            ParamDef::color("sky.tint", start.horizon.tint)
                .range(0.0, 2.0)
                .describe("A colour multiplier on the clear sky's scattered light (1, 1, 1: as the model gives it)")
                .owner(owner),
        )?;
        #[cfg(feature = "sky-horizon")]
        let volume_on = reg.register_bool(
            ParamDef::bool("sky.cloud_volume", start.horizon.volume.on)
                .describe("Draw the clouds as a real volume (cumulus heaps lit through themselves) instead of the flat layer")
                .owner(owner),
        )?;
        #[cfg(feature = "sky-horizon")]
        let volume = {
            let c = start.horizon.volume;
            let v = [c.seed, c.base, c.depth, c.size, c.cirrus, c.billow, c.soft, c.base_shade, c.powder];
            let mut out = Vec::with_capacity(VOLUME.len());
            for (i, (suffix, min, max, step, describe)) in VOLUME.into_iter().enumerate() {
                out.push(
                    reg.register_f32(
                        ParamDef::f32(&format!("sky.{suffix}"), v[i])
                            .range(min, max)
                            .step(step)
                            .describe(describe)
                            .owner(owner),
                    )?,
                );
            }
            let out: [F32Param; 9] = out
                .try_into()
                .map_err(|_| ParamError::Unknown("sky.cloud_volume".into()))?;
            out
        };
        Ok(SkyHandles {
            f,
            sun_disc,
            visible,
            open_below,
            follow_light,
            sun_elevation,
            sun_azimuth,
            #[cfg(feature = "sky-horizon")]
            horizon,
            #[cfg(feature = "sky-horizon")]
            tint,
            #[cfg(feature = "sky-horizon")]
            volume_on,
            #[cfg(feature = "sky-horizon")]
            volume,
        })
    }

    /// The current sky.
    pub fn read(&self, reg: &ParamRegistry) -> SkyParams {
        let mut v = [0.0; 10];
        for (o, h) in v.iter_mut().zip(self.f) {
            *o = reg.f32(h);
        }
        let mut s = SkyParams::default().with_floats(v);
        s.sun_disc = reg.bool(self.sun_disc);
        s.visible = reg.bool(self.visible);
        s.open_below = reg.bool(self.open_below);
        #[cfg(feature = "sky-horizon")]
        {
            let [
                low,
                high,
                curve,
                haze,
                lobe_shade,
                edge,
                bite,
                gain,
                deep,
                up_bias,
                body_shade,
            ] = self.horizon.map(|h| reg.f32(h));
            s.horizon = CloudHorizon {
                fade_low: low,
                fade_high: high,
                curve,
                haze,
                lobe_shade,
                edge,
                bite,
                gain,
                tint: reg.vec3(self.tint),
                deep,
                up_bias,
                body_shade,
                volume: {
                    let [seed, base, depth, size, cirrus, billow, soft, base_shade, powder] =
                        self.volume.map(|h| reg.f32(h));
                    CloudVolume {
                        on: reg.bool(self.volume_on),
                        seed,
                        base,
                        depth,
                        size,
                        cirrus,
                        billow,
                        soft,
                        base_shade,
                        powder,
                    }
                },
            };
        }
        s
    }

    /// The dome's sun: `light` (the scene light, towards the sun) while
    /// `sky.follow_light` is on, else the elevation and azimuth params.
    pub fn sun(&self, reg: &ParamRegistry, light: [f32; 3]) -> [f32; 3] {
        if reg.bool(self.follow_light) {
            light
        } else {
            sun_direction(reg.f32(self.sun_elevation), reg.f32(self.sun_azimuth))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sky at known sun angles: a blue midday zenith, a warm low sun
    /// towards the sun, a dark sky after sunset, pinned values at 30 degrees.
    #[test]
    fn sky_colour_at_known_sun_angles() {
        let p = SkyParams::default();
        let lum = |c: [f32; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        let noon = sun_direction(60.0, 0.0);
        let z = p.radiance(noon, [0.0, 1.0, 0.0]);
        assert!(z[2] > 1.5 * z[0], "noon zenith {z:?}");
        let low = sun_direction(2.0, 0.0);
        let towards = p.radiance(low, [0.0, 0.05, 1.0]);
        let away = p.radiance(low, [0.0, 0.05, -1.0]);
        assert!(towards[0] > towards[2], "towards the low sun {towards:?}");
        assert!(
            away[2] / away[0] > towards[2] / towards[0],
            "away {away:?} towards {towards:?}"
        );
        let night = p.ring(sun_direction(-8.0, 0.0), 0.3, 16);
        assert!(
            lum(night) < 0.05 * lum(p.ring(noon, 0.3, 16)),
            "night {night:?}"
        );
        // Pinned (regression): the zenith with the sun 30 degrees up.
        let z30 = p.radiance(sun_direction(30.0, 0.0), [0.0, 1.0, 0.0]);
        for (got, want) in z30.iter().zip(Z30) {
            assert!(
                (got - want).abs() <= 0.01 * want.abs() + 1e-6,
                "zenith at 30 degrees {z30:?}, want {Z30:?}"
            );
        }
    }

    /// The default sky's zenith colour with the sun 30 degrees up.
    const Z30: [f32; 3] = [0.079_573_63, 0.273_598_3, 0.842_138_65];

    #[test]
    fn params_register_in_range_and_follow_the_light() {
        let mut reg = ParamRegistry::default();
        let h = SkyHandles::register(&mut reg, &SkyParams::default()).unwrap();
        assert_eq!(h.read(&reg), SkyParams::default());
        assert!(reg.defs().iter().all(|d| d.name.starts_with("sky.")));
        let light = [0.0, 1.0, 0.0];
        assert_eq!(h.sun(&reg, light), light);
        let follow = reg.id("sky.follow_light").unwrap();
        reg.set(follow, evani_core::params::ParamValue::Bool(false))
            .unwrap();
        let s = h.sun(&reg, light);
        assert!(
            (s[1] - SUN_DEFAULT.0.to_radians().sin()).abs() < 1e-6,
            "{s:?}"
        );
    }

    #[test]
    fn zenith_is_blue_and_the_horizon_paler_and_brighter() {
        let p = SkyParams::default();
        let sun = sun_direction(20.0, 56.0);
        let z = p.radiance(sun, [0.0, 1.0, 0.0]);
        let h = p.ring(sun, 0.02, 32);
        assert!(
            z.iter().chain(&h).all(|v| v.is_finite() && *v > 0.0),
            "{z:?} {h:?}"
        );
        assert!(z[2] > z[1] && z[1] > z[0], "zenith {z:?}");
        // Paler: less blue over red.
        assert!(h[2] / h[0] < z[2] / z[0], "horizon {h:?} zenith {z:?}");
        assert!(h.iter().sum::<f32>() > z.iter().sum::<f32>());
    }

    #[test]
    fn records_carry_the_normalised_sun_and_the_params() {
        let p = SkyParams::default();
        let r = p.records([0.0, 2.0, 0.0], 7.5);
        assert_eq!(r[0], [0.0, 1.0, 0.0, 7.5]);
        assert_eq!(r[1][0], p.turbidity);
        assert_eq!(r[3][1], p.brightness);
        assert_eq!(r[3][2], 1.0);
        assert_eq!(r[3][3], 0.0, "closed below by default");
        let open = SkyParams {
            open_below: true,
            ..p
        };
        assert_eq!(open.records([0.0, 1.0, 0.0], 0.0)[3][3], 1.0);
    }

    /// `sky.visible` and `sky.open_below` read back from the registry.
    #[test]
    fn visible_and_open_below_are_live_params() {
        let mut reg = ParamRegistry::default();
        let h = SkyHandles::register(&mut reg, &SkyParams::default()).unwrap();
        let set = |reg: &mut ParamRegistry, n: &str, v: bool| {
            let id = reg.id(n).unwrap();
            reg.set(id, evani_core::params::ParamValue::Bool(v))
                .unwrap();
        };
        set(&mut reg, "sky.visible", false);
        set(&mut reg, "sky.open_below", true);
        let s = h.read(&reg);
        assert!(!s.visible && s.open_below, "{s:?}");
    }

    /// Feature `sky-horizon`: the defaults are the clouds without the
    /// feature (the shader's fixed 0.03..0.27 fade, 0.1..0.4 detail), a lower
    /// band shows clouds a few degrees over the skyline, the record keeps the
    /// band ordered, and the four params read back from the registry.
    #[cfg(feature = "sky-horizon")]
    #[test]
    fn cloud_horizon_defaults_match_and_low_bands_show_clouds() {
        let d = CloudHorizon::default();
        assert_eq!(d.record(), [0.03, 0.27, 1.0, 1.0]);
        assert_eq!(SkyParams::default().horizon, d);
        assert_eq!(d.fade(0.03), 0.0);
        assert_eq!(d.fade(0.27), 1.0);
        assert!(d.detail(0.1).abs() < 1e-6 && (d.detail(0.4) - 1.0).abs() < 1e-6);
        // 3 degrees up: nearly nothing by default, most of a cloud low down.
        let y3 = 3f32.to_radians().sin();
        assert!(d.fade(y3) < 0.05, "{}", d.fade(y3));
        let low = CloudHorizon {
            fade_low: 0.0,
            fade_high: 0.08,
            curve: 0.5,
            haze: 0.4,
            ..d
        };
        assert!(low.fade(y3) > 0.6, "{}", low.fade(y3));
        // A band set upside down still has its top above its bottom.
        let flipped = CloudHorizon {
            fade_low: 0.2,
            fade_high: 0.1,
            ..d
        };
        let r = flipped.record();
        assert!(r[1] > r[0], "{r:?}");
        let mut reg = ParamRegistry::default();
        let h = SkyHandles::register(&mut reg, &SkyParams::default()).unwrap();
        for (n, v) in [
            ("sky.cloud_horizon_low", 0.0),
            ("sky.cloud_horizon_high", 0.08),
            ("sky.cloud_horizon_curve", 0.5),
            ("sky.cloud_horizon_haze", 0.4),
        ] {
            let id = reg.id(n).unwrap();
            reg.set(id, evani_core::params::ParamValue::F32(v)).unwrap();
        }
        assert_eq!(h.read(&reg).horizon, low);
    }

    #[test]
    fn sun_direction_is_unit_and_up() {
        let s = sun_direction(20.0, 56.0);
        assert!(((s[0] * s[0] + s[1] * s[1] + s[2] * s[2]) - 1.0).abs() < 1e-5);
        assert!((s[1] - 20f32.to_radians().sin()).abs() < 1e-6);
    }
}
