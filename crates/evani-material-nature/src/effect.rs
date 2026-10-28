//! The `nature` render pass: evani-render's post-effect interface, so the
//! engine draws it after the forward pass without any engine change.
//!
//! * Lite and Low ([`Mode::Flat`]): a multiply and an additive draw per five
//!   surfaces: calm water (tint, body colour, Fresnel sky) and glass, no
//!   waves, foam or glow.
//! * Medium and VR ([`Mode::Split`]): a multiply draw (what is behind each
//!   surface: depth fade, glass tint, foam cover) and an additive draw (light
//!   on top: Fresnel sky, sun glint, foam, the back-lit glow), no copy.
//! * High and Ultra ([`Mode::Refract`], `material.nature.refraction` on):
//!   copy `hdr` into the transient `nature.scene`, then one replace draw
//!   that reads the copy bent by the waves and a bottle's curve.

use evani_core::params::ParamRegistry;
use evani_render::depth::inverse_distance;
use evani_render::post::{
    Blend, EffectShader, PostEffect, PostRecorder, UniformFrame, draw_effect, put_words,
};
use evani_render::shaders::{FAR, NEAR};

use crate::surface::NatureSurfaces;

/// How the pass draws this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Off,
    Flat,
    Split,
    Refract,
}

impl Mode {
    /// The mode for a tier (`refraction`: `material.nature.refraction`).
    pub fn for_tier(id: &str, refraction: bool) -> Mode {
        match id {
            "lite" | "low" => Mode::Flat,
            "high" | "ultra" if refraction => Mode::Refract,
            _ => Mode::Split,
        }
    }
}

/// Surfaces one draw meets.
pub const BATCH: usize = 5;
pub const PASS: &str = "nature";
pub const PASSES: &[&str] = &[PASS];
/// The screen copy High and Ultra read.
pub const SCENE: &str = "nature.scene";

const NATURE_WGSL: &str = concat!(
    include_str!("../shaders/nature.wgsl"),
    "\n",
    include_str!("../shaders/nature_common.wgsl")
);
const REFRACT_WGSL: &str = concat!(
    include_str!("../shaders/nature_refract.wgsl"),
    "\n",
    include_str!("../shaders/nature_common.wgsl")
);

pub const FLAT: usize = 0;
pub const MUL: usize = 1;
pub const ADD: usize = 2;
pub const COPY: usize = 3;
pub const REFRACT: usize = 4;
pub const FLAT_ADD: usize = 5;

const fn surf_shader(name: &'static str, source: &'static str, blend: Blend, input: bool) -> EffectShader {
    EffectShader {
        name,
        pass: PASS,
        source,
        slots: &["nature_mat"],
        blend,
        uniform_words: 64,
        input,
        depth: true,
    }
}

pub static SHADERS: [EffectShader; 6] = [
    surf_shader("nature_flat", NATURE_WGSL, Blend::Multiply, false),
    surf_shader("nature_mul", NATURE_WGSL, Blend::Multiply, false),
    surf_shader("nature_add", NATURE_WGSL, Blend::Additive, false),
    EffectShader {
        name: "nature_copy",
        pass: PASS,
        source: include_str!("../shaders/nature_copy.wgsl"),
        slots: &[],
        blend: Blend::Replace,
        uniform_words: 4,
        input: true,
        depth: false,
    },
    surf_shader("nature_refract", REFRACT_WGSL, Blend::Replace, true),
    surf_shader("nature_flat_add", NATURE_WGSL, Blend::Additive, false),
];

pub(crate) fn norm(v: [f32; 3]) -> [f32; 3] {
    evani_render::math::normalize(v)
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// The post effect (shares its state with the [`NatureSurfaces`] resource).
pub struct NatureEffect {
    pub(crate) surfaces: NatureSurfaces,
}

impl NatureEffect {
    fn batches(&self) -> usize {
        self.surfaces.drawn().div_ceil(BATCH)
    }
}

impl PostEffect for NatureEffect {
    fn name(&self) -> &'static str {
        "nature"
    }

    fn pass_names(&self) -> &'static [&'static str] {
        PASSES
    }

    fn shaders(&self) -> &'static [EffectShader] {
        &SHADERS
    }

    fn record(&self, pass: &str, reg: &ParamRegistry, rec: &mut dyn PostRecorder) {
        if pass != PASS {
            return;
        }
        for b in 0..self.batches() as u32 {
            match self.surfaces.mode() {
                Mode::Off => return,
                Mode::Flat => {
                    draw_effect(self, rec, reg, FLAT, None, "hdr", b);
                    draw_effect(self, rec, reg, FLAT_ADD, None, "hdr", b);
                }
                Mode::Split => {
                    draw_effect(self, rec, reg, MUL, None, "hdr", b);
                    draw_effect(self, rec, reg, ADD, None, "hdr", b);
                }
                Mode::Refract => {
                    draw_effect(self, rec, reg, COPY, Some("hdr"), SCENE, b);
                    draw_effect(self, rec, reg, REFRACT, Some(SCENE), "hdr", b);
                }
            }
        }
    }

    /// `frame.mip` is the batch: surfaces `5 * mip ..` of this frame's.
    fn write_uniforms(&self, shader: &str, _reg: &ParamRegistry, frame: &UniformFrame, out: &mut [f32]) -> usize {
        let mode = match shader {
            "nature_flat" => 0.0,
            "nature_mul" => 1.0,
            "nature_add" => 2.0,
            "nature_refract" => 3.0,
            "nature_flat_add" => 4.0,
            "nature_copy" => return put_words(out, &[0.0; 4]),
            _ => return 0,
        };
        if out.len() < 64 {
            return 0;
        }
        let st = &self.surfaces.0;
        let cam = st.camera.get();
        let fwd = norm([
            cam.target[0] - cam.eye[0],
            cam.target[1] - cam.eye[1],
            cam.target[2] - cam.eye[2],
        ]);
        let right = norm(cross(fwd, cam.up));
        let up = cross(right, fwd);
        let tan = (cam.fov_y_deg.to_radians() * 0.5).tan();
        let aspect = frame.target_size[0] / frame.target_size[1].max(1.0);
        let [inv0, inv1] = inverse_distance(NEAR, FAR);
        #[cfg(feature = "camera-range")]
        let [inv0, inv1] = {
            let _ = [inv0, inv1];
            let (n, f) = evani_render::camera_range::current();
            inverse_distance(n, f)
        };
        let sun = st.sun.get();
        let col = st.sun_col.get();
        let skip = frame.mip as usize * BATCH;
        let drawn = st.drawn.try_borrow();
        let list: &[crate::Surface] = drawn.as_deref().map_or(&[], |v| v.as_slice());
        let batch = list.get(skip..).unwrap_or(&[]);
        let n = batch.len().min(BATCH);
        let head = [
            [cam.eye[0], cam.eye[1], cam.eye[2], inv0],
            [fwd[0], fwd[1], fwd[2], inv1],
            [right[0] * tan * aspect, right[1] * tan * aspect, right[2] * tan * aspect, st.time.get()],
            [up[0] * tan, up[1] * tan, up[2] * tan, mode],
            [sun[0], sun[1], sun[2], n as f32],
            [col[0], col[1], col[2], st.sky_tint.get()],
        ];
        for (i, row) in head.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(row);
        }
        out[24..64].fill(0.0);
        for (i, s) in batch.iter().take(BATCH).enumerate() {
            out[24 + i * 8..32 + i * 8].copy_from_slice(&s.words());
        }
        64
    }
}
