//! Bloom (feature `bloom`): bright-pass into a half-resolution target, then a
//! dual-Kawase blur down and back up a chain of half-size mips, then an
//! additive composite onto the HDR image. Three graph passes, four WGSL files.
//!
//! The blur is frame-relative: `fx.bloom.mips` and `fx.bloom.spread` are the
//! chain at a 1080-pixel short side ([`REFERENCE_SHORT_SIDE`]); a smaller
//! frame (a phone, a small window, a lower render scale) runs fewer levels
//! with the remainder in the spread, so a halo covers the same share of the
//! picture at every size ([`blur_plan`]).

use evani_core::params::{BakeTarget, F32Param, I32Param, ParamDef, ParamRegistry, Vec3Param};
use evani_core::plugin::{PluginContext, PluginError};
use evani_render::graph::{GraphError, PassDesc, RenderGraph, TargetDesc, TargetKind};

use crate::{
    Blend, Effect, EffectInfo, EffectShader, PostEffect, PostRecorder, StyleClasses, UniformFrame, draw_effect,
    put_words,
};

pub static INFO: EffectInfo = EffectInfo {
    name: "bloom",
    cost: 3,
    value: 0.8,
    // Catalog works_with: 2d, pseudo-3d, 3d (2D sprites are drawn into the
    // HDR target before the effects, so they bloom too).
    styles: StyleClasses::ALL,
    vr_comfort_off: false,
    description: "Glow around bright areas (bright-pass + dual-Kawase blur over half-res mips)",
};

/// Most blur levels the graph declares targets for (`fx.bloom.mips` picks
/// how many are used at runtime).
pub const MAX_MIPS: usize = 7;

/// `bloom.mip1` (1/2 res) ... `bloom.mip7` (1/128 res).
pub const MIP_TARGETS: [&str; MAX_MIPS] = [
    "bloom.mip1",
    "bloom.mip2",
    "bloom.mip3",
    "bloom.mip4",
    "bloom.mip5",
    "bloom.mip6",
    "bloom.mip7",
];

pub const PASSES: &[&str] = &["bloom.bright", "bloom.blur", "bloom.composite"];

static SHADERS: [EffectShader; 4] = [
    EffectShader {
        name: "bloom_bright",
        pass: "bloom.bright",
        source: include_str!("../shaders/bloom_bright.wgsl"),
        slots: &[],
        blend: Blend::Replace,
        uniform_words: 8,
        input: true,
        depth: false,
    },
    EffectShader {
        name: "bloom_down",
        pass: "bloom.blur",
        source: include_str!("../shaders/bloom_down.wgsl"),
        slots: &[],
        blend: Blend::Replace,
        uniform_words: 4,
        input: true,
        depth: false,
    },
    EffectShader {
        name: "bloom_up",
        pass: "bloom.blur",
        source: include_str!("../shaders/bloom_up.wgsl"),
        slots: &["bloom_weight"],
        blend: Blend::Additive,
        uniform_words: 4,
        input: true,
        depth: false,
    },
    EffectShader {
        name: "bloom_composite",
        pass: "bloom.composite",
        source: include_str!("../shaders/bloom_composite.wgsl"),
        slots: &[],
        blend: Blend::Additive,
        uniform_words: 4,
        input: true,
        depth: false,
    },
];

#[derive(Debug, Clone, Copy)]
pub struct BloomParams {
    pub threshold: F32Param,
    pub knee: F32Param,
    pub clamp: F32Param,
    pub intensity: F32Param,
    pub tint: Vec3Param,
    pub spread: F32Param,
    pub mips: I32Param,
}

#[derive(Debug)]
pub struct Bloom {
    pub params: Option<BloomParams>,
    /// `fx.bloom.spread` multiplier for this frame's blur, set by `record`
    /// from [`blur_plan`] before it draws the chain (1 at the reference
    /// size). Kept here because the deep mips' rounded sizes no longer
    /// give the frame size back exactly.
    pub spread_scale: std::cell::Cell<f32>,
}

impl Default for Bloom {
    fn default() -> Self {
        Self {
            params: None,
            spread_scale: std::cell::Cell::new(1.0),
        }
    }
}

/// Short side (pixels) of the frame the bloom params are tuned at. On any
/// other frame size the halo keeps the size it has here relative to the
/// frame: a phone or a small window gets fewer, tighter blur levels, not
/// the same pixel radius over a smaller picture.
pub const REFERENCE_SHORT_SIDE: f32 = 1080.0;

/// The blur for a frame whose short side is `short` pixels, with
/// `fx.bloom.mips` = `mips`: how many downsample steps to run and what to
/// multiply `fx.bloom.spread` by. The halo radius is about
/// `spread * 2^steps` full-resolution pixels, so the steps follow
/// `log2(short / REFERENCE_SHORT_SIDE)` and the fraction left over goes
/// into the spread. At the reference size it is `(mips - 1, 1.0)`, exactly
/// the fixed chain.
pub fn blur_plan(mips: usize, short: f32) -> (usize, f32) {
    let steps = mips.clamp(1, MAX_MIPS) as f32 - 1.0;
    if !(short.is_finite() && short > 0.0) {
        return (steps as usize, 1.0);
    }
    let want = (steps + (short / REFERENCE_SHORT_SIDE).log2()).clamp(0.0, (MAX_MIPS - 1) as f32);
    // Round away float noise so the reference size lands on whole steps.
    let n = (want + 1e-4).floor();
    (n as usize, 2f32.powf((want - n).max(0.0)))
}

impl Bloom {
    /// Blur levels to use this frame (`fx.bloom.mips`, clamped to the graph).
    pub fn mips(&self, reg: &ParamRegistry) -> usize {
        self.params
            .map_or(5, |p| reg.i32(p.mips).clamp(1, MAX_MIPS as i32) as usize)
    }
}

fn f(name: &str, d: f32, min: f32, max: f32, unit: &str, desc: &str) -> ParamDef {
    ParamDef::f32(name, d)
        .range(min, max)
        .step(0.01)
        .unit(unit)
        .describe(desc)
        .owner("fx.bloom")
        .bake(BakeTarget::Look)
        .gpu()
}

impl Effect for Bloom {
    fn info(&self) -> &'static EffectInfo {
        &INFO
    }

    fn register(&mut self, ctx: &mut PluginContext) -> Result<(), PluginError> {
        self.params = Some(BloomParams {
            threshold: ctx.param_f32(f(
                "fx.bloom.threshold",
                1.0,
                0.0,
                10.0,
                "luminance",
                "Brightness above which pixels bloom.",
            ))?,
            knee: ctx.param_f32(f(
                "fx.bloom.knee",
                0.5,
                0.0,
                5.0,
                "luminance",
                "Softness of the threshold: how gradually bloom fades in.",
            ))?,
            clamp: ctx.param_f32(f(
                "fx.bloom.clamp",
                64.0,
                1.0,
                1000.0,
                "luminance",
                "Brightest value fed into the blur (stops single-pixel sparkles).",
            ))?,
            intensity: ctx.param_f32(f(
                "fx.bloom.intensity",
                0.6,
                0.0,
                4.0,
                "",
                "How strongly the blurred bright areas are added back.",
            ))?,
            tint: ctx.param_vec3(
                ParamDef::color("fx.bloom.tint", [1.0, 1.0, 1.0])
                    .unit("linear rgb")
                    .describe("Colour of the glow.")
                    .owner("fx.bloom")
                    .bake(BakeTarget::Look)
                    .gpu(),
            )?,
            spread: ctx.param_f32(f(
                "fx.bloom.spread",
                1.0,
                0.5,
                3.0,
                "texels",
                "Blur sample offset per level; larger is wider and softer.",
            ))?,
            mips: ctx.param_i32(
                ParamDef::i32("fx.bloom.mips", 5)
                    .range(2.0, MAX_MIPS as f32)
                    .unit("levels")
                    .describe("Blur levels.")
                    .owner("fx.bloom")
                    .bake(BakeTarget::Look),
            )?,
        });
        Ok(())
    }

    fn passes(&self, g: &mut RenderGraph) -> Result<(), GraphError> {
        let mut scale = 1.0;
        for name in MIP_TARGETS {
            scale *= 0.5;
            g.add_target(TargetDesc::new(name, TargetKind::HdrColor).scaled(scale).transient())?;
        }
        g.add_pass(
            PassDesc::new("bloom.bright", "fx.bloom")
                .reads(&["hdr"])
                .writes(&["bloom.mip1"])
                .order(50),
        )?;
        g.add_pass(
            PassDesc::new("bloom.blur", "fx.bloom")
                .reads(&["bloom.mip1"])
                .writes(&MIP_TARGETS)
                .uses_slots(&["bloom_weight"])
                .order(51),
        )?;
        g.add_pass(
            PassDesc::new("bloom.composite", "fx.bloom")
                .reads(&["hdr", "bloom.mip1"])
                .writes(&["hdr"])
                .order(52),
        )
    }
}

impl PostEffect for Bloom {
    fn name(&self) -> &'static str {
        INFO.name
    }

    /// `bloom.bright`: hdr -> mip1. `bloom.blur`: down mip1 -> ... -> mipN,
    /// then back up mipN -> ... -> mip1 (additive, each level weighted by the
    /// `bloom_weight` slot). `bloom.composite`: mip1 added onto hdr.
    fn record(&self, pass: &str, reg: &ParamRegistry, rec: &mut dyn PostRecorder) {
        match pass {
            "bloom.bright" => draw_effect(self, rec, reg, 0, Some("hdr"), MIP_TARGETS[0], 0),
            "bloom.blur" => {
                // Frame-relative: the steps follow the frame's short side
                // (mip1 is half of it), see [`blur_plan`].
                let half = rec.target_size(MIP_TARGETS[0]);
                let (steps, mult) = blur_plan(self.mips(reg), 2.0 * half[0].min(half[1]));
                self.spread_scale.set(mult);
                let n = steps + 1;
                // `mip` is the source level, 0 = half resolution (mip1).
                for i in 1..n {
                    draw_effect(self, rec, reg, 1, Some(MIP_TARGETS[i - 1]), MIP_TARGETS[i], i as u32 - 1);
                }
                for i in (1..n).rev() {
                    draw_effect(self, rec, reg, 2, Some(MIP_TARGETS[i]), MIP_TARGETS[i - 1], i as u32);
                }
            }
            "bloom.composite" => draw_effect(self, rec, reg, 3, Some(MIP_TARGETS[0]), "hdr", 0),
            _ => {}
        }
    }

    fn pass_names(&self) -> &'static [&'static str] {
        PASSES
    }

    fn shaders(&self) -> &'static [EffectShader] {
        &SHADERS
    }

    fn write_uniforms(
        &self,
        shader: &str,
        reg: &ParamRegistry,
        frame: &UniformFrame,
        out: &mut [f32],
    ) -> usize {
        let texel = [
            1.0 / frame.source_size[0].max(1.0),
            1.0 / frame.source_size[1].max(1.0),
        ];
        let v = |h: Option<F32Param>, d: f32| h.map_or(d, |h| reg.f32(h));
        let p = self.params;
        match shader {
            "bloom_bright" => put_words(
                out,
                &[
                    texel[0],
                    texel[1],
                    v(p.map(|p| p.threshold), 1.0),
                    v(p.map(|p| p.knee), 0.5),
                    v(p.map(|p| p.clamp), 64.0),
                    0.0,
                    0.0,
                    0.0,
                ],
            ),
            "bloom_down" | "bloom_up" => put_words(
                out,
                &[
                    texel[0],
                    texel[1],
                    v(p.map(|p| p.spread), 1.0) * self.spread_scale.get(),
                    f32::from_bits(frame.mip),
                ],
            ),
            "bloom_composite" => {
                let t = p.map_or([1.0; 3], |p| reg.vec3(p.tint));
                put_words(out, &[t[0], t[1], t[2], v(p.map(|p| p.intensity), 0.6)])
            }
            _ => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FxModule;
    use evani_core::App;
    use evani_render::RenderModule;

    fn app() -> App {
        let mut a = App::new("t")
            .with_module(RenderModule::new())
            .with_module(FxModule::new().with_effect(Bloom::default()));
        a.run_headless(1, 16.0);
        a
    }

    #[test]
    fn bloom_registers_live_api_names() {
        let a = app();
        let reg = a.world.res::<ParamRegistry>();
        for n in [
            "fx.bloom.enabled",
            "fx.bloom.value_weight",
            "fx.bloom.threshold",
            "fx.bloom.intensity",
            "fx.bloom.mips",
        ] {
            assert!(reg.id(n).is_some(), "{n}");
        }
        let names: Vec<_> = a.world.res::<RenderGraph>().ordered().map(|p| p.name).collect();
        // The forward pass is there only with the 3D path (feature `forward`).
        let expected: Vec<_> = [
            "forward",
            "sprites",
            "bloom.bright",
            "bloom.blur",
            "bloom.composite",
            "tonemap",
            "present",
        ]
        .into_iter()
        .filter(|&n| evani_render::FORWARD_PATH || n != "forward")
        .collect();
        assert_eq!(names, expected);
    }

    #[test]
    fn uniforms_follow_params() {
        let a = app();
        let reg = a.world.res::<ParamRegistry>();
        let bloom = Bloom {
            params: Some(BloomParams {
                threshold: reg.f32_handle("fx.bloom.threshold").unwrap(),
                knee: reg.f32_handle("fx.bloom.knee").unwrap(),
                clamp: reg.f32_handle("fx.bloom.clamp").unwrap(),
                intensity: reg.f32_handle("fx.bloom.intensity").unwrap(),
                tint: Vec3Param(reg.id("fx.bloom.tint").unwrap()),
                spread: reg.f32_handle("fx.bloom.spread").unwrap(),
                mips: I32Param(reg.id("fx.bloom.mips").unwrap()),
            }),
            ..Bloom::default()
        };
        let frame = UniformFrame {
            source_size: [200.0, 100.0],
            target_size: [100.0, 50.0],
            mip: 3,
        };
        let mut out = [0.0; 8];
        assert_eq!(bloom.write_uniforms("bloom_composite", &reg, &frame, &mut out), 4);
        assert_eq!(&out[..4], &[1.0, 1.0, 1.0, 0.6]);
        assert_eq!(bloom.write_uniforms("bloom_up", &reg, &frame, &mut out), 4);
        assert_eq!(out[0], 0.005);
        assert_eq!(out[3].to_bits(), 3);
        assert_eq!(bloom.mips(&reg), 5);
        assert_eq!(bloom.write_uniforms("nope", &reg, &frame, &mut out), 0);
    }

    /// Records draws as `(shader, src, dst, mip)`.
    #[derive(Default)]
    struct Rec(Vec<(usize, Option<String>, String, u32)>);
    impl PostRecorder for Rec {
        fn target_size(&self, t: &str) -> [f32; 2] {
            let level = MIP_TARGETS.iter().position(|m| *m == t).map_or(0, |i| i + 1);
            [1920.0 / (1 << level) as f32, 1080.0 / (1 << level) as f32]
        }
        fn draw(&mut self, shader: usize, src: Option<&str>, dst: &str, u: &[f32]) {
            let mip = if shader == 1 || shader == 2 { u[3].to_bits() } else { 0 };
            self.0.push((shader, src.map(String::from), dst.to_string(), mip));
        }
    }

    #[test]
    fn records_the_dual_filter_chain() {
        let a = app();
        let pe = a.world.res::<evani_render::PostEffects>();
        let (_, bloom) = pe.for_pass("bloom.blur").unwrap();
        let reg = a.world.res::<ParamRegistry>();
        let mut rec = Rec::default();
        for pass in PASSES {
            bloom.record(pass, &reg, &mut rec);
        }
        let d = |s: usize, src: &str, dst: &str, mip: u32| (s, Some(src.to_string()), dst.to_string(), mip);
        // fx.bloom.mips = 5: 4 downsamples, 4 upsamples.
        assert_eq!(
            rec.0,
            vec![
                d(0, "hdr", "bloom.mip1", 0),
                d(1, "bloom.mip1", "bloom.mip2", 0),
                d(1, "bloom.mip2", "bloom.mip3", 1),
                d(1, "bloom.mip3", "bloom.mip4", 2),
                d(1, "bloom.mip4", "bloom.mip5", 3),
                d(2, "bloom.mip5", "bloom.mip4", 4),
                d(2, "bloom.mip4", "bloom.mip3", 3),
                d(2, "bloom.mip3", "bloom.mip2", 2),
                d(2, "bloom.mip2", "bloom.mip1", 1),
                d(3, "bloom.mip1", "hdr", 0),
            ]
        );
    }

    /// Records the blur chain on a frame of `w` x `h`: (downsample count,
    /// spread uniform of the first blur draw).
    struct Sized(f32, f32, Vec<(usize, f32)>);
    impl PostRecorder for Sized {
        fn target_size(&self, t: &str) -> [f32; 2] {
            let level = MIP_TARGETS.iter().position(|m| *m == t).map_or(0, |i| i + 1);
            [(self.0 / (1 << level) as f32).floor(), (self.1 / (1 << level) as f32).floor()]
        }
        fn draw(&mut self, shader: usize, _: Option<&str>, _: &str, u: &[f32]) {
            self.2.push((shader, if shader == 1 || shader == 2 { u[2] } else { 0.0 }));
        }
    }

    fn chain(w: f32, h: f32) -> (usize, f32) {
        let a = app();
        let pe = a.world.res::<evani_render::PostEffects>();
        let (_, bloom) = pe.for_pass("bloom.blur").unwrap();
        let reg = a.world.res::<ParamRegistry>();
        let mut rec = Sized(w, h, Vec::new());
        bloom.record("bloom.blur", &reg, &mut rec);
        let downs = rec.2.iter().filter(|d| d.0 == 1).count();
        let spread = rec.2.first().map_or(1.0, |d| d.1);
        (downs, spread)
    }

    #[test]
    fn the_blur_plan_is_the_fixed_chain_at_the_reference_size() {
        for mips in 1..=MAX_MIPS {
            assert_eq!(blur_plan(mips, REFERENCE_SHORT_SIDE), (mips - 1, 1.0), "mips {mips}");
        }
        assert_eq!(blur_plan(5, 0.0), (4, 1.0));
        assert_eq!(blur_plan(5, f32::NAN), (4, 1.0));
        // Twice the size: one more level, capped by the graph's targets.
        assert_eq!(blur_plan(5, 2160.0), (5, 1.0));
        assert_eq!(blur_plan(MAX_MIPS, 4320.0), (MAX_MIPS - 1, 1.0));
        // Tiny frames: no blur levels, just the half-res bright pass.
        assert_eq!(blur_plan(3, 100.0).0, 0);
    }

    /// The halo radius (about spread * 2^steps pixels) over the frame's
    /// short side is the same at every window size.
    #[test]
    fn the_halo_holds_its_size_relative_to_the_frame() {
        let rel = |short: f32| {
            let (steps, mult) = blur_plan(5, short);
            mult * 2f32.powi(steps as i32) / short
        };
        let at_1080 = rel(1080.0);
        for short in [360.0, 390.0, 540.0, 720.0, 844.0, 1440.0, 2160.0] {
            let r = rel(short);
            assert!((r / at_1080 - 1.0).abs() < 1e-3, "short side {short}: {r} vs {at_1080}");
        }
        // Through the recorder: 1080p runs the param's chain unchanged; a
        // 640 x 360 page and a 390 x 844 phone run fewer, scaled levels.
        assert_eq!(chain(1920.0, 1080.0), (4, 1.0));
        let (downs, spread) = chain(640.0, 360.0);
        assert_eq!(downs, 2);
        assert!((spread - 2f32.powf(4.0 + (1.0f32 / 3.0).log2() - 2.0)).abs() < 1e-3, "{spread}");
        assert_eq!(chain(390.0, 844.0).0, chain(844.0, 390.0).0);
        assert!(chain(390.0, 844.0).0 < 4);
    }}
