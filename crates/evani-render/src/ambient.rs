//! Sky ambient (S5-R5): features `ibl` and `gtao`, which both turn on the
//! internal `ambient`. Pure data: the params, the procedural sky, its SH9
//! irradiance and the ambient uniform. The GPU side is `gpu/ambient.rs`
//! (frame layout, shader patch, sky cube), `gpu/gtao.rs` (ambient
//! occlusion) and `gpu/frame/ambient.rs` (the frame glue).
//!
//! The sky. There was no sky module before this: the background is
//! `render.clear_color` and every lit surface got a flat `render.ambient`.
//! The sky here is a direction function: a tint that runs from
//! `render.sky.horizon` up to `render.sky.zenith` (by the square root of the
//! height above the horizon) and down to `render.sky.ground`, scaled per
//! channel so its mean over the sphere is exactly `render.ambient` (only the
//! tints' shape matters; the level stays `render.ambient`, so a game keeps
//! its tuned brightness), plus a soft sun glow `render.sky.sun_glow` x the
//! light colour x `max(d . L, 0)^32`. One source for every tier: the SH9
//! diffuse, the analytic reflection and the cube all evaluate it.
//!
//! SH9. Standard real spherical harmonics on world xyz, bands 0 to 2. The
//! tint is projected numerically on a 16 x 32 grid (weights renormalised to
//! 4 pi), the sun glow analytically (a zonal lobe: `2 pi int t^32 P_l`), and
//! only when an input changes. Each coefficient is premultiplied by its
//! basis constant and the clamped-cosine convolution over pi (1, 2/3, 1/4),
//! so the shader evaluates a plain polynomial and a flat sky gives exactly
//! `render.ambient`.

// The frame code that writes the uniform is wasm only (`gpu`).
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

#[cfg(feature = "ibl")]
use evani_core::params::Vec3Param;
use evani_core::params::{F32Param, ParamDef, ParamError, ParamRegistry};
use evani_core::plugin::PluginContext;

#[cfg(feature = "ibl")]
use crate::shaders::Lighting;

/// Floats in the ambient uniform: `sh[9]`, zenith (w: F0), horizon (w:
/// roughness), ground (w: cube mips, 0 = analytic), sun direction (w: cube
/// size), sun glow colour (w: sky-light strength), AO (on, strength, 1/w,
/// 1/h of the frame).
pub const WORDS: usize = 60;
/// Where the AO vec4 starts.
pub const AO_AT: usize = 56;
/// Mip levels of the prefiltered sky cube (roughness 0, 0.25 ... 1).
#[cfg(feature = "ibl")]
pub const CUBE_MIPS: u32 = 5;

/// The ambient params' handles (render module build).
pub struct AmbientParams {
    #[cfg(feature = "ibl")]
    zenith: Vec3Param,
    #[cfg(feature = "ibl")]
    horizon: Vec3Param,
    #[cfg(feature = "ibl")]
    ground: Vec3Param,
    #[cfg(feature = "ibl")]
    sun_glow: F32Param,
    #[cfg(feature = "ibl")]
    strength: F32Param,
    #[cfg(feature = "ibl")]
    specular: F32Param,
    #[cfg(feature = "ibl")]
    roughness: F32Param,
    #[cfg(feature = "gtao")]
    ao_strength: F32Param,
    #[cfg(feature = "gtao")]
    ao_radius: F32Param,
}

/// Register the sky, reflection and AO params.
pub(crate) fn register(ctx: &mut PluginContext) -> Result<(), ParamError> {
    #[cfg(feature = "ibl")]
    let sky = |ctx: &mut PluginContext, name: &str, v: f32, what: &str| {
        ctx.param_vec3(ParamDef::color(name, [v; 3]).range(0.0, 4.0).describe(what).gpu())
    };
    let params = AmbientParams {
        #[cfg(feature = "ibl")]
        zenith: sky(ctx, "render.sky.zenith", 1.0, "Sky tint straight up (its shape only: render.ambient sets the level)")?,
        #[cfg(feature = "ibl")]
        horizon: sky(ctx, "render.sky.horizon", 0.77, "Sky tint at the horizon (its shape only: render.ambient sets the level)")?,
        #[cfg(feature = "ibl")]
        ground: sky(ctx, "render.sky.ground", 0.38, "Light bounced up from the ground (its shape only: render.ambient sets the level)")?,
        #[cfg(feature = "ibl")]
        sun_glow: ctx.param_f32(
            ParamDef::f32("render.sky.sun_glow", 0.25)
                .range(0.0, 2.0)
                .step(0.01)
                .describe("Glow round the sun in the sky light and reflections, as a share of the sun's light")
                .gpu(),
        )?,
        #[cfg(feature = "ibl")]
        strength: ctx.param_f32(
            ParamDef::f32("render.ibl.strength", 1.0)
                .range(0.0, 1.0)
                .step(0.01)
                .describe("Sky lighting: 0 = flat render.ambient and no sky reflections, 1 = full")
                .gpu(),
        )?,
        #[cfg(feature = "ibl")]
        specular: ctx.param_f32(
            ParamDef::f32("render.ibl.specular", 0.04)
                .range(0.0, 0.2)
                .step(0.005)
                .describe("Sky reflection head-on (F0; 0.04 for most non-metals, more at grazing angles)")
                .gpu(),
        )?,
        #[cfg(feature = "ibl")]
        roughness: ctx.param_f32(
            ParamDef::f32("render.ibl.roughness", 0.7)
                .range(0.05, 1.0)
                .step(0.01)
                .describe("How blurred sky reflections are (0 mirror, 1 matte)")
                .gpu(),
        )?,
        #[cfg(feature = "gtao")]
        ao_strength: ctx.param_f32(
            ParamDef::f32("render.ao.strength", 1.0)
                .range(0.0, 1.0)
                .step(0.01)
                .describe("Ambient occlusion: how much creases and contacts darken the ambient light (never direct light)")
                .gpu(),
        )?,
        #[cfg(feature = "gtao")]
        ao_radius: ctx.param_f32(
            ParamDef::f32("render.ao.radius", 1.0)
                .range(0.1, 4.0)
                .step(0.05)
                .unit("m")
                .describe("Ambient occlusion reach in metres")
                .gpu(),
        )?,
    };
    ctx.resource(params);
    Ok(())
}

impl AmbientParams {
    /// `render.ao.strength` and `render.ao.radius`.
    #[cfg(feature = "gtao")]
    pub fn ao(&self, reg: &ParamRegistry) -> (f32, f32) {
        (reg.f32(self.ao_strength), reg.f32(self.ao_radius))
    }
}

/// The sky's inputs this frame and its cached SH9 (recomputed only when an
/// input changes; `generation` counts the changes, for the sky cube).
#[cfg(feature = "ibl")]
#[derive(Default)]
pub struct Sky {
    key: [f32; 18],
    sh: [[f32; 3]; 9],
    tints: [[f32; 3]; 3],
    pub generation: u32,
}

/// Basis constants of the nine real SH functions (the polynomials are 1, y,
/// z, x, xy, yz, 3z^2 - 1, xz, x^2 - y^2).
#[cfg(feature = "ibl")]
const SH_C: [f32; 9] = [0.282_095, 0.488_603, 0.488_603, 0.488_603, 1.092_548, 1.092_548, 0.315_392, 1.092_548, 0.546_274];
/// Band of each coefficient.
#[cfg(feature = "ibl")]
const SH_BAND: [usize; 9] = [0, 1, 1, 1, 2, 2, 2, 2, 2];
/// Clamped-cosine convolution over pi per band (A_l / pi).
#[cfg(feature = "ibl")]
const SH_K: [f32; 3] = [1.0, 2.0 / 3.0, 0.25];

#[cfg(feature = "ibl")]
fn sh_poly(d: [f32; 3]) -> [f32; 9] {
    let [x, y, z] = d;
    [1.0, y, z, x, x * y, y * z, 3.0 * z * z - 1.0, x * z, x * x - y * y]
}

#[cfg(feature = "ibl")]
fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// `acc += c * f`.
#[cfg(feature = "ibl")]
fn add(acc: &mut [f32; 3], c: [f32; 3], f: f32) {
    for (a, v) in acc.iter_mut().zip(c) {
        *a += v * f;
    }
}

/// The sky tint towards a direction whose height (y) is `up`.
#[cfg(feature = "ibl")]
fn tint(t: &[[f32; 3]; 3], up: f32) -> [f32; 3] {
    let k = up.abs().min(1.0).sqrt();
    if up >= 0.0 { mix3(t[1], t[0], k) } else { mix3(t[1], t[2], k) }
}

#[cfg(feature = "ibl")]
impl Sky {
    /// Bring the SH up to date with this frame's light and sky params.
    /// Returns true when it changed.
    pub fn update(&mut self, reg: &ParamRegistry, p: &AmbientParams, l: &Lighting) -> bool {
        let d = crate::math::normalize(l.light_dir);
        let glow = reg.f32(p.sun_glow) * l.intensity;
        let sun = [l.light_color[0] * glow, l.light_color[1] * glow, l.light_color[2] * glow];
        let raw = [reg.vec3(p.zenith), reg.vec3(p.horizon), reg.vec3(p.ground)];
        let mut key = [0.0; 18];
        for (dst, v) in key.as_chunks_mut::<3>().0.iter_mut().zip([l.ambient, raw[0], raw[1], raw[2], d, sun]) {
            *dst = v;
        }
        if self.generation != 0 && key == self.key {
            return false;
        }
        self.key = key;
        // Grid over the sphere (cell centres), solid-angle weights.
        const NT: usize = 16;
        const NP: usize = 32;
        let (dt, dp) = (std::f32::consts::PI / NT as f32, std::f32::consts::TAU / NP as f32);
        let cell = |i: usize, j: usize| {
            let (st, ct) = ((i as f32 + 0.5) * dt).sin_cos();
            let (sp, cp) = ((j as f32 + 0.5) * dp).sin_cos();
            ([st * cp, ct, st * sp], st)
        };
        let (mut mean, mut wsum) = ([0.0f32; 3], 0.0f32);
        for i in 0..NT {
            for j in 0..NP {
                let (dir, w) = cell(i, j);
                add(&mut mean, tint(&raw, dir[1]), w);
                wsum += w;
            }
        }
        let scale = 4.0 * std::f32::consts::PI / wsum.max(1e-6);
        // Normalised tints: the sky's mean is render.ambient.
        for (t, r) in self.tints.iter_mut().zip(raw) {
            for (((t, r), m), a) in t.iter_mut().zip(r).zip(mean).zip(l.ambient) {
                let m = m / wsum.max(1e-6);
                *t = if m > 1e-6 { a * r / m } else { a };
            }
        }
        let mut sh = [[0.0f32; 3]; 9];
        for i in 0..NT {
            for j in 0..NP {
                let (dir, w) = cell(i, j);
                let c = tint(&self.tints, dir[1]);
                for (s, (y, kc)) in sh.iter_mut().zip(sh_poly(dir).into_iter().zip(SH_C)) {
                    add(s, c, y * kc * w * scale);
                }
            }
        }
        // The sun glow max(t, 0)^32 around d, projected exactly:
        // L_lm = 2 pi int_0^1 t^32 P_l(t) dt * Y_lm(d).
        let g = [
            std::f32::consts::TAU / 33.0,
            std::f32::consts::TAU / 34.0,
            std::f32::consts::PI * (3.0 / 35.0 - 1.0 / 33.0),
        ];
        for (s, ((y, kc), band)) in sh.iter_mut().zip(sh_poly(d).into_iter().zip(SH_C).zip(SH_BAND)) {
            add(s, sun, g[band] * y * kc);
        }
        // Premultiply basis constant and cosine convolution for the shader.
        for (s, (kc, band)) in sh.iter_mut().zip(SH_C.into_iter().zip(SH_BAND)) {
            let f = kc * SH_K[band];
            for v in s.iter_mut() {
                *v *= f;
            }
        }
        self.sh = sh;
        self.generation = self.generation.wrapping_add(1).max(1);
        true
    }

    /// Write the sky part of the uniform (everything but the AO vec4).
    /// `cube` is the sky cube's face size (0 = the analytic sky).
    pub fn write(&self, out: &mut [f32; WORDS], reg: &ParamRegistry, p: &AmbientParams, cube: u32) {
        for (dst, s) in out.as_chunks_mut::<4>().0.iter_mut().zip(self.sh) {
            *dst = [s[0], s[1], s[2], 0.0];
        }
        let mips = if cube > 0 { CUBE_MIPS as f32 } else { 0.0 };
        let [kx, ky, kz] = crate::math::normalize([self.key[12], self.key[13], self.key[14]]);
        let tail = [
            [self.tints[0][0], self.tints[0][1], self.tints[0][2], reg.f32(p.specular)],
            [self.tints[1][0], self.tints[1][1], self.tints[1][2], reg.f32(p.roughness)],
            [self.tints[2][0], self.tints[2][1], self.tints[2][2], mips],
            [kx, ky, kz, cube as f32],
            [self.key[15], self.key[16], self.key[17], reg.f32(p.strength)],
        ];
        for (dst, v) in out[36..AO_AT].as_chunks_mut::<4>().0.iter_mut().zip(tail) {
            *dst = v;
        }
    }
}
