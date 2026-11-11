//! The Asset & Style Studio's screen passes (feature `studio-passes`, WP5 of
//! docs/studio/WORK_PACKAGES.md): Pixel and Pixel HD, paper (Watercolour,
//! Ink, Gothic, Storybook), tilt-shift (Diorama) and silhouette, as real
//! passes over the finished image instead of stand-in geometry.
//!
//! One pair of graph passes after the tonemap, the way the grade does it (a
//! pass cannot read the target it writes): `studio.apply` reads `ldr` and the
//! scene depth and draws the chosen mode into the transient `studio.tmp`;
//! `studio.back` copies it home. The mode is the param
//! `render.studio_pass.mode` (its option names are `evani_studio`'s
//! `Pass::name()`s); with `none` both passes record no draw, so switching a
//! pass on or off rebuilds nothing and costs nothing while off. Every number
//! is a registered param (`render.studio_pass.*`), so the studio and
//! evani-live tune them alike.
//!
//! Pixel's grid follows the camera's remainder below one art pixel
//! ([`StudioPassState::set_offset`]): the app snaps the camera to whole art
//! pixels and hands the rest here, so pixels never crawl.
//!
//! Games without the feature compile none of this: no params, no passes, no
//! WGSL in their download.

use core::cell::Cell;
use std::rc::Rc;

use evani_core::params::{EnumParam, F32Param, I32Param, ParamDef, ParamRegistry, Vec3Param};
use evani_core::plugin::{PluginContext, PluginError};

use crate::depth::inverse_distance;
use crate::graph::{PassDesc, RenderGraph, TargetDesc, TargetKind};
use crate::post::{Blend, EffectShader, PostEffect, PostEffects, PostRecorder, UniformFrame, draw_effect, put_words};
use crate::shaders::{FAR, NEAR};

/// The modes, in the order of `render.studio_pass.mode`'s options. The
/// first five are evani-studio's `Pass::name()`s (the app test holds them
/// equal); `dither` is the screen-space form of a surface stage's `dither`
/// (the engraving style), which the app turns on from the surface.
pub const MODES: [&str; 6] = ["none", "pixel", "paper", "tiltshift", "silhouette", "dither"];

/// The two graph passes.
pub const PASSES: &[&str] = &["studio.apply", "studio.back"];

/// The transient target `studio.apply` draws into.
pub const TMP: &str = "studio.tmp";

const fn shader(name: &'static str, pass: &'static str, source: &'static str, words: usize, depth: bool) -> EffectShader {
    EffectShader { name, pass, source, slots: &[], blend: Blend::Replace, uniform_words: words, input: true, depth }
}

/// Shader `i + 1` is mode `MODES[i + 1]`'s; the last one is the copy home.
pub static SHADERS: [EffectShader; 6] = [
    shader("studio_pixel", "studio.apply", include_str!("../shaders/studio_pixel.wgsl"), 16, true),
    shader("studio_paper", "studio.apply", include_str!("../shaders/studio_paper.wgsl"), 12, true),
    shader("studio_tiltshift", "studio.apply", include_str!("../shaders/studio_tiltshift.wgsl"), 8, true),
    shader("studio_silhouette", "studio.apply", include_str!("../shaders/studio_silhouette.wgsl"), 16, true),
    shader("studio_dither", "studio.apply", include_str!("../shaders/studio_dither.wgsl"), 12, true),
    shader("studio_copy", "studio.back", include_str!("../shaders/studio_copy.wgsl"), 4, false),
];
const COPY: usize = 5;

/// Defaults (the Classic Pixel style's numbers; the studio writes each
/// style's own over them).
pub mod defaults {
    pub const PX: f32 = 4.0;
    pub const LEVELS: i32 = 5;
    pub const OUTLINE: f32 = 1.0;
    pub const DITHER: f32 = 0.35;
    pub const GRAIN: f32 = 0.4;
    pub const EDGE: f32 = 0.5;
    pub const BLEED: f32 = 0.3;
    pub const FOCUS: f32 = 0.5;
    pub const BLUR: f32 = 0.6;
    pub const BAND: f32 = 0.12;
    pub const FOG: f32 = 0.55;
    pub const INK: [f32; 3] = [0.07, 0.05, 0.09];
    pub const PAPER: [f32; 3] = [0.94, 0.9, 0.81];
    pub const FOG_TOP: [f32; 3] = [0.62, 0.68, 0.74];
    pub const FOG_LOW: [f32; 3] = [0.86, 0.8, 0.7];
    pub const SHAPE: [f32; 3] = [0.035, 0.035, 0.05];
    pub const PERIOD: f32 = 4.0;
    pub const LIFT: f32 = 1.3;
    pub const GRADE: f32 = 1.0;
}

/// Every param name, for tests and tools.
pub const PARAMS: [&str; 19] = [
    "render.studio_pass.mode",
    "render.studio_pass.px",
    "render.studio_pass.levels",
    "render.studio_pass.outline",
    "render.studio_pass.dither",
    "render.studio_pass.ink",
    "render.studio_pass.grain",
    "render.studio_pass.edge",
    "render.studio_pass.bleed",
    "render.studio_pass.paper",
    "render.studio_pass.focus",
    "render.studio_pass.blur",
    "render.studio_pass.band",
    "render.studio_pass.fog",
    "render.studio_pass.fog_top",
    "render.studio_pass.shape",
    "render.studio_pass.period",
    "render.studio_pass.lift",
    "render.studio_pass.grade",
];

#[derive(Debug, Clone, Copy)]
struct Handles {
    mode: EnumParam,
    px: F32Param,
    levels: I32Param,
    outline: F32Param,
    dither: F32Param,
    ink: Vec3Param,
    grain: F32Param,
    edge: F32Param,
    bleed: F32Param,
    paper: Vec3Param,
    focus: F32Param,
    blur: F32Param,
    band: F32Param,
    fog: F32Param,
    fog_top: Vec3Param,
    shape: Vec3Param,
    period: F32Param,
    lift: F32Param,
    grade: F32Param,
}

/// The effect the renderer runs.
#[derive(Debug)]
pub struct StudioPasses {
    params: Handles,
    /// Pixel grid offset in screen pixels (the camera's sub-art-pixel rest).
    offset: Cell<[f32; 2]>,
}

/// World resource: the app's handle on the running effect.
#[derive(Debug, Clone)]
pub struct StudioPassState(pub Rc<StudioPasses>);

impl StudioPassState {
    /// The pixel grid's offset in screen pixels, each in `0..px`.
    pub fn set_offset(&self, offset: [f32; 2]) {
        self.0.offset.set(offset);
    }

    pub fn offset(&self) -> [f32; 2] {
        self.0.offset.get()
    }
}

/// The art pixel in screen pixels: `px` per 720 lines of view, scaled by
/// whole steps on taller views (a 1440-line screen draws each art pixel
/// twice as wide), so the grid keeps its look from a laptop to a wide
/// screen and stays whole pixels.
pub fn art_pixel(px: f32, view_h: f32) -> f32 {
    px.round().max(1.0) * (view_h / 720.0).round().max(1.0)
}

impl StudioPasses {
    /// The current mode's index in [`MODES`].
    pub fn mode(&self, reg: &ParamRegistry) -> usize {
        reg.enum_index(self.params.mode) as usize
    }
}

fn f(name: &str, d: f32, min: f32, max: f32, step: f32, desc: &str) -> ParamDef {
    ParamDef::f32(name, d).range(min, max).step(step).describe(desc).owner("render.studio_pass").gpu()
}

fn colour(name: &str, d: [f32; 3], desc: &str) -> ParamDef {
    ParamDef::color(name, d).unit("display rgb").describe(desc).owner("render.studio_pass").gpu()
}

/// Register the params, the effect and its two passes (render module build).
pub fn register(ctx: &mut PluginContext) -> Result<(), PluginError> {
    use defaults as d;
    let mode = ctx.param_enum(
        ParamDef::enumeration("render.studio_pass.mode", &MODES, 0)
            .describe("The studio's screen pass: none, pixel (Pixel, Pixel HD), paper (Watercolour, Ink, Gothic, Storybook), tiltshift (Diorama), silhouette, or dither (the engraving style's surface).")
            .owner("render.studio_pass"),
    )?;
    let params = Handles {
        mode,
        px: ctx.param_f32(f("render.studio_pass.px", d::PX, 1.0, 12.0, 1.0, "Pixel: screen pixels per art pixel at a 720-line view (whole numbers; scaled by whole steps on taller views).").unit("px"))?,
        levels: ctx.param_i32(
            ParamDef::i32("render.studio_pass.levels", d::LEVELS)
                .range(0.0, 16.0)
                .step(1.0)
                .describe("Pixel: palette steps (luma gets 2n - 1, each chroma axis 2n; 0 = no palette cut).")
                .owner("render.studio_pass")
                .gpu(),
        )?,
        outline: ctx.param_f32(f("render.studio_pass.outline", d::OUTLINE, 0.0, 1.0, 0.05, "Pixel: outline strength on depth edges (silhouette in ink, inner edges a darker shade)."))?,
        dither: ctx.param_f32(f("render.studio_pass.dither", d::DITHER, 0.0, 1.0, 0.05, "Pixel: ordered dither between palette steps."))?,
        ink: ctx.param_vec3(colour("render.studio_pass.ink", d::INK, "Pixel: the silhouette line's colour."))?,
        grain: ctx.param_f32(f("render.studio_pass.grain", d::GRAIN, 0.0, 1.0, 0.05, "Paper: grain strength."))?,
        edge: ctx.param_f32(f("render.studio_pass.edge", d::EDGE, 0.0, 1.0, 0.05, "Paper: pigment pooling at the edges of washes and shapes."))?,
        bleed: ctx.param_f32(f("render.studio_pass.bleed", d::BLEED, 0.0, 1.0, 0.05, "Paper: how far washes run past their edges, and the line's waver."))?,
        paper: ctx.param_vec3(colour("render.studio_pass.paper", d::PAPER, "Paper: the sheet's colour (shows where the sky was)."))?,
        focus: ctx.param_f32(f("render.studio_pass.focus", d::FOCUS, 0.0, 1.0, 0.01, "Miniature: the sharp band's height in the view (0 = top, 1 = bottom)."))?,
        blur: ctx.param_f32(f("render.studio_pass.blur", d::BLUR, 0.0, 1.0, 0.05, "Miniature: blur outside the band and the depth in focus."))?,
        band: ctx.param_f32(f("render.studio_pass.band", d::BAND, 0.0, 0.5, 0.01, "Miniature: half the sharp band's height (0..1 of the view)."))?,
        fog: ctx.param_f32(f("render.studio_pass.fog", d::FOG, 0.0, 1.0, 0.05, "Silhouette: how far shapes behind the subject fade into the fog."))?,
        fog_top: ctx.param_vec3(colour("render.studio_pass.fog_top", d::FOG_TOP, "Silhouette: the fog's colour at the top of the view."))?,
        shape: ctx.param_vec3(colour("render.studio_pass.shape", d::SHAPE, "Silhouette: the cut-outs' colour."))?,
        period: ctx.param_f32(f("render.studio_pass.period", d::PERIOD, 2.0, 12.0, 0.5, "Engraving: the hatch lines' spacing in pixels.").unit("px"))?,
        lift: ctx.param_f32(f("render.studio_pass.lift", d::LIFT, 0.5, 3.0, 0.05, "Engraving: tone lift before the ink is laid (higher = lighter, less ink)."))?,
        grade: ctx.param_f32(f("render.studio_pass.grade", d::GRADE, 0.0, 1.0, 0.05, "Miniature: the toy model's extra saturation and contrast (1); 0 is a plain photographic depth of field with a 1-step dither (the procedural library's realism view)."))?,
    };
    let fx = Rc::new(StudioPasses { params, offset: Cell::new([0.0; 2]) });
    match ctx.world.try_res_mut::<PostEffects>() {
        Some(mut pe) => pe.effects.push(fx.clone()),
        None => return Err(PluginError("studio passes: the render module's PostEffects is missing".into())),
    }
    ctx.resource(StudioPassState(fx));
    let mut g = ctx.world.res_mut::<RenderGraph>();
    let err = |e: crate::graph::GraphError| PluginError(format!("studio passes: {e}"));
    g.add_target(TargetDesc::new(TMP, TargetKind::Ldr).transient()).map_err(err)?;
    // Orders 140 and 141: after the tonemap (100), before the grade (150),
    // the vignette (200) and the HUD, so the HUD stays sharp.
    g.add_pass(PassDesc::new("studio.apply", "render.studio_pass").reads(&["ldr", "depth"]).writes(&[TMP]).order(140)).map_err(err)?;
    // Reads `ldr` too: an in-place writer (see the grade's `grade.back`).
    g.add_pass(PassDesc::new("studio.back", "render.studio_pass").reads(&[TMP, "ldr"]).writes(&["ldr"]).order(141)).map_err(err)?;
    Ok(())
}

impl PostEffect for StudioPasses {
    fn name(&self) -> &'static str {
        "studio_passes"
    }

    fn pass_names(&self) -> &'static [&'static str] {
        PASSES
    }

    fn shaders(&self) -> &'static [EffectShader] {
        &SHADERS
    }

    /// `studio.apply`: the mode's shader, ldr -> studio.tmp; `studio.back`:
    /// the copy home. Nothing at all in mode `none`.
    fn record(&self, pass: &str, reg: &ParamRegistry, rec: &mut dyn PostRecorder) {
        let mode = self.mode(reg);
        if mode == 0 || mode >= MODES.len() {
            return;
        }
        match pass {
            "studio.apply" => draw_effect(self, rec, reg, mode - 1, Some("ldr"), TMP, 0),
            "studio.back" => draw_effect(self, rec, reg, COPY, Some(TMP), "ldr", 0),
            _ => {}
        }
    }

    fn write_uniforms(&self, shader: &str, reg: &ParamRegistry, frame: &UniformFrame, out: &mut [f32]) -> usize {
        let p = &self.params;
        let [w, h] = frame.target_size;
        let [i0, i1] = inverse_distance(NEAR, FAR);
        #[cfg(feature = "camera-range")]
        let [i0, i1] = {
            let _ = [i0, i1];
            let (n, f) = crate::camera_range::current();
            inverse_distance(n, f)
        };
        match shader {
            "studio_pixel" => {
                let ink = reg.vec3(p.ink);
                let [ox, oy] = self.offset.get();
                put_words(
                    out,
                    &[
                        w,
                        h,
                        i0,
                        i1,
                        art_pixel(reg.f32(p.px), h),
                        reg.i32(p.levels).max(0) as f32,
                        reg.f32(p.outline),
                        reg.f32(p.dither),
                        ox,
                        oy,
                        0.0,
                        0.0,
                        ink[0],
                        ink[1],
                        ink[2],
                        0.0,
                    ],
                )
            }
            "studio_paper" => {
                let c = reg.vec3(p.paper);
                put_words(out, &[w, h, i0, i1, reg.f32(p.grain), reg.f32(p.edge), reg.f32(p.bleed), 0.0, c[0], c[1], c[2], 0.0])
            }
            "studio_tiltshift" => put_words(out, &[w, h, i0, i1, reg.f32(p.focus), reg.f32(p.blur), reg.f32(p.band), reg.f32(p.grade)]),
            "studio_silhouette" => {
                let (t, s) = (reg.vec3(p.fog_top), reg.vec3(p.shape));
                let l = defaults::FOG_LOW;
                put_words(out, &[w, h, i0, i1, t[0], t[1], t[2], reg.f32(p.fog), l[0], l[1], l[2], 0.0, s[0], s[1], s[2], 0.0])
            }
            "studio_dither" => {
                let (i, q) = (reg.vec3(p.ink), reg.vec3(p.paper));
                put_words(out, &[w, h, i0, i1, i[0], i[1], i[2], reg.f32(p.period), q[0], q[1], q[2], reg.f32(p.lift)])
            }
            "studio_copy" => put_words(out, &[w, h, 0.0, 0.0]),
            _ => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RenderModule;
    use evani_core::App;
    use evani_core::params::ParamValue;

    fn app() -> App {
        let mut a = App::new("t").with_module(RenderModule::new());
        a.run_headless(1, 16.0);
        a
    }

    #[derive(Default)]
    struct Rec(Vec<(usize, Option<String>, String, Vec<f32>)>);
    impl PostRecorder for Rec {
        fn target_size(&self, _: &str) -> [f32; 2] {
            [640.0, 360.0]
        }
        fn draw(&mut self, s: usize, src: Option<&str>, dst: &str, u: &[f32]) {
            self.0.push((s, src.map(String::from), dst.to_string(), u.to_vec()));
        }
    }

    fn record(a: &App) -> Rec {
        let reg = a.world.res::<ParamRegistry>();
        let pe = a.world.res::<PostEffects>();
        let mut rec = Rec::default();
        for pass in PASSES {
            let (_, fx) = pe.for_pass(pass).unwrap();
            fx.record(pass, &reg, &mut rec);
        }
        rec
    }

    fn set(a: &mut App, name: &str, v: ParamValue) {
        a.world.res_mut::<ParamRegistry>().set_by_name(name, v).unwrap();
    }

    #[test]
    fn the_passes_read_depth_and_write_ldr_back() {
        let a = app();
        let g = a.world.res::<RenderGraph>();
        let apply = g.pass("studio.apply").unwrap();
        assert!(apply.inputs.contains(&"depth") && apply.inputs.contains(&"ldr"));
        assert_eq!(apply.outputs, vec![TMP]);
        let back = g.pass("studio.back").unwrap();
        assert_eq!(back.outputs, vec!["ldr"]);
        let order: Vec<&str> = g.ordered().map(|p| p.name).collect();
        let at = |n: &str| order.iter().position(|p| *p == n).unwrap();
        assert!(at("tonemap") < at("studio.apply") && at("studio.apply") < at("studio.back") && at("studio.back") < at("present"));
        for p in PARAMS {
            assert!(a.world.res::<ParamRegistry>().id(p).is_some(), "{p} is registered");
        }
    }

    #[test]
    fn none_draws_nothing_and_every_mode_draws_its_shader_then_copies_home() {
        let mut a = app();
        assert!(record(&a).0.is_empty(), "mode none records no draw");
        for (m, name) in MODES.iter().enumerate().skip(1) {
            set(&mut a, "render.studio_pass.mode", ParamValue::Enum(m as u32));
            let r = record(&a).0;
            assert_eq!(r.len(), 2, "{name}");
            assert_eq!((r[0].0, r[0].1.as_deref(), r[0].2.as_str()), (m - 1, Some("ldr"), TMP), "{name}");
            assert_eq!((r[1].0, r[1].1.as_deref(), r[1].2.as_str()), (COPY, Some(TMP), "ldr"), "{name}");
            assert_eq!(r[0].3.len(), SHADERS[m - 1].uniform_words, "{name} fills its whole uniform");
            assert!(SHADERS[m - 1].name.ends_with(name), "shader {} is mode {name}'s", SHADERS[m - 1].name);
        }
    }

    #[test]
    fn uniforms_carry_the_params_and_the_grid_offset() {
        let mut a = app();
        set(&mut a, "render.studio_pass.mode", ParamValue::Enum(1));
        set(&mut a, "render.studio_pass.px", ParamValue::F32(2.6));
        set(&mut a, "render.studio_pass.levels", ParamValue::I32(0));
        a.world.res::<StudioPassState>().set_offset([1.5, 0.25]);
        let r = record(&a).0;
        let u = &r[0].3;
        assert_eq!(&u[..2], &[640.0, 360.0]);
        assert_eq!(&u[2..4], &inverse_distance(NEAR, FAR));
        assert_eq!((u[4], u[5]), (3.0, 0.0), "px rounds to whole pixels; levels 0 = no palette");
        assert_eq!((art_pixel(4.0, 720.0), art_pixel(4.0, 1440.0), art_pixel(3.0, 1000.0), art_pixel(0.2, 300.0)), (4.0, 8.0, 3.0, 1.0));
        assert_eq!(&u[8..10], &[1.5, 0.25]);
        set(&mut a, "render.studio_pass.mode", ParamValue::Enum(2));
        set(&mut a, "render.studio_pass.grain", ParamValue::F32(0.7));
        let r = record(&a).0;
        assert_eq!(r[0].3[4], 0.7);
    }

    /// Each shader is a small module (the material shaders' 60 KB cap is far
    /// above; these must stay tiny so a studio build adds next to nothing),
    /// carries its `evani.shader.<name>` marker and declares the bindings the
    /// renderer's depth-reading layout gives it.
    #[test]
    fn shaders_are_small_marked_and_bound_as_the_renderer_binds_them() {
        let mut total = 0;
        for s in &SHADERS {
            assert!(s.source.len() < 8 * 1024, "{} is {} bytes", s.name, s.source.len());
            total += s.source.len();
            assert!(s.source.starts_with(&format!("// evani.shader.{}\n", s.name)), "{}", s.name);
            for b in ["@binding(0) var src: texture_2d<f32>", "@binding(1) var samp: sampler", "@binding(2) var<uniform> u:"] {
                assert!(s.source.contains(b), "{} binds {b}", s.name);
            }
            assert_eq!(s.source.contains("@binding(3) var depth_tex: texture_depth_2d"), s.depth, "{}", s.name);
            assert!(s.uniform_words % 4 == 0 && s.uniform_words <= crate::post::MAX_UNIFORM_WORDS);
        }
        assert!(total < 20 * 1024, "all studio pass WGSL together is {total} bytes");
    }
}
