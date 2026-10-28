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
//! `sky.cloud_elevation`, `sky.brightness`, `sky.sun_disc`; and the sun:
//! `sky.follow_light` (default on: the dome's sun is wherever
//! `render.light.direction` points, so the disc sits where the shadows come
//! from), else `sky.sun_elevation` and `sky.sun_azimuth` (degrees).

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
                0.0,
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
        std::array::from_fn(|i| {
            let ratio = (beta_r[i] * r_phase + beta_m[i] * m_phase) / (beta_r[i] + beta_m[i]);
            let lin = (sun_e * ratio * (1.0 - fex[i])).powf(1.5);
            let lin = lin * (1.0 + ((sun_e * ratio * fex[i]).sqrt() - 1.0) * fade);
            let l0 = 0.1 * fex[i];
            ((lin + l0) * 0.04 + NIGHT[i]) * self.brightness
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
    follow_light: BoolParam,
    sun_elevation: F32Param,
    sun_azimuth: F32Param,
}

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
        Ok(SkyHandles {
            f,
            sun_disc,
            follow_light,
            sun_elevation,
            sun_azimuth,
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
    }

    #[test]
    fn sun_direction_is_unit_and_up() {
        let s = sun_direction(20.0, 56.0);
        assert!(((s[0] * s[0] + s[1] * s[1] + s[2] * s[2]) - 1.0).abs() < 1e-5);
        assert!((s[1] - 20f32.to_radians().sin()).abs() < 1e-6);
    }
}
