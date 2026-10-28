//! Post-effect interface between evani-render and optional effect crates
//! (evani-fx). An effect declares its WGSL files ([`EffectShader`]) and
//! records its draws through a [`PostRecorder`]; the renderer owns every GPU
//! object (pipelines, targets, uniform ring) and runs the draws when the
//! effect's graph pass comes up. Pure data here, so effects test natively
//! with a fake recorder.

use std::rc::Rc;

use evani_core::params::ParamRegistry;

/// How a draw's output is combined with its target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Blend {
    /// Overwrite the target.
    Replace,
    /// `dst + src` (bloom upsample and composite).
    Additive,
    /// `dst * src` (vignette).
    Multiply,
}

/// One WGSL file of an effect.
#[derive(Debug, Clone, Copy)]
pub struct EffectShader {
    /// Shader id, also the `// evani.shader.<name>` marker on its first line
    /// (xtask looks for it to prove a compiled-out effect left no shader).
    pub name: &'static str,
    /// Render-graph pass that runs it.
    pub pass: &'static str,
    /// WGSL source with `vs_main` / `fs_main`. Slot functions are spliced in
    /// at `//@slot <name>` marker lines (see [`crate::splice`]).
    pub source: &'static str,
    pub slots: &'static [&'static str],
    pub blend: Blend,
    /// Size of its uniform struct in 4-byte words (what
    /// [`PostEffect::write_uniforms`] fills). At most [`MAX_UNIFORM_WORDS`].
    pub uniform_words: usize,
    /// Samples a source texture: bind group layout A (texture, sampler,
    /// uniform). `false`: layout B (uniform only), e.g. the vignette.
    pub input: bool,
    /// Also reads the scene depth (`texture_depth_2d`, load it with
    /// `textureLoad`): bound after the other bindings, at binding 3 with an
    /// input and at binding 1 without one. The pass must list `depth` in
    /// its graph inputs so the forward pass keeps the depth target.
    pub depth: bool,
}

/// Largest uniform struct an effect shader may declare (one 256-byte slot
/// of the renderer's uniform ring).
pub const MAX_UNIFORM_WORDS: usize = 64;

/// Per-draw values the renderer knows and the params don't.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UniformFrame {
    /// Size in pixels of the texture the shader samples.
    pub source_size: [f32; 2],
    /// Size in pixels of the target it draws into.
    pub target_size: [f32; 2],
    /// Mip level for multi-level passes (bloom blur), else 0.
    pub mip: u32,
}

/// Copy `vals` into `out` if it fits; returns the words written.
pub fn put_words(out: &mut [f32], vals: &[f32]) -> usize {
    match out.get_mut(..vals.len()) {
        Some(dst) => {
            dst.copy_from_slice(vals);
            vals.len()
        }
        None => 0,
    }
}

/// What an effect's [`PostEffect::record`] draws through. Implemented by the
/// renderer (GPU) and by test fakes.
pub trait PostRecorder {
    /// Pixel size of a graph target this frame (`[0, 0]` if unknown).
    fn target_size(&self, target: &str) -> [f32; 2];
    /// One full-screen draw of `shaders()[shader]` sampling `src` (None for
    /// shaders without an input) into `dst`, with the shader's blend mode.
    /// `uniforms` fills the shader's uniform struct.
    fn draw(&mut self, shader: usize, src: Option<&str>, dst: &str, uniforms: &[f32]);
}

/// A post effect as the renderer sees it. evani-fx's `Effect` extends it.
pub trait PostEffect: 'static {
    fn name(&self) -> &'static str;
    /// Graph passes this effect records (toggled together).
    fn pass_names(&self) -> &'static [&'static str];
    fn shaders(&self) -> &'static [EffectShader];
    /// Fill the uniform struct of `shader` from the current param values.
    /// Returns the words written (0 for an unknown shader or a short `out`).
    /// No allocation: called per draw.
    fn write_uniforms(
        &self,
        shader: &str,
        reg: &ParamRegistry,
        frame: &UniformFrame,
        out: &mut [f32],
    ) -> usize;
    /// Record the draws of graph pass `pass`. No allocation.
    fn record(&self, pass: &str, reg: &ParamRegistry, rec: &mut dyn PostRecorder);
}

/// Helper for [`PostEffect::record`]: build the [`UniformFrame`] from the
/// recorder's target sizes, fill the uniforms and draw.
pub fn draw_effect<E: PostEffect + ?Sized>(
    effect: &E,
    rec: &mut dyn PostRecorder,
    reg: &ParamRegistry,
    shader: usize,
    src: Option<&str>,
    dst: &str,
    mip: u32,
) {
    let Some(sh) = effect.shaders().get(shader) else {
        return;
    };
    let frame = UniformFrame {
        source_size: src.map_or([0.0; 2], |s| rec.target_size(s)),
        target_size: rec.target_size(dst),
        mip,
    };
    let mut buf = [0.0f32; MAX_UNIFORM_WORDS];
    let n = effect.write_uniforms(sh.name, reg, &frame, &mut buf);
    rec.draw(shader, src, dst, &buf[..n]);
}

/// World resource (inserted by the render module): the post effects compiled
/// into this build, in registration order. evani-fx pushes its effects here.
#[derive(Default, Clone)]
pub struct PostEffects {
    pub effects: Vec<Rc<dyn PostEffect>>,
}

impl PostEffects {
    /// The effect that records `pass`, with its index.
    pub fn for_pass(&self, pass: &str) -> Option<(usize, &Rc<dyn PostEffect>)> {
        self.effects
            .iter()
            .enumerate()
            .find(|(_, e)| e.pass_names().contains(&pass))
    }
}

impl core::fmt::Debug for PostEffects {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list().entries(self.effects.iter().map(|e| e.name())).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static SH: [EffectShader; 1] = [EffectShader {
        name: "t",
        pass: "t",
        source: "",
        slots: &[],
        blend: Blend::Replace,
        uniform_words: 2,
        input: true,
        depth: false,
    }];

    struct E;
    impl PostEffect for E {
        fn name(&self) -> &'static str {
            "e"
        }
        fn pass_names(&self) -> &'static [&'static str] {
            &["t"]
        }
        fn shaders(&self) -> &'static [EffectShader] {
            &SH
        }
        fn write_uniforms(&self, _: &str, _: &ParamRegistry, f: &UniformFrame, out: &mut [f32]) -> usize {
            put_words(out, &[f.source_size[0], f.target_size[0]])
        }
        fn record(&self, _: &str, reg: &ParamRegistry, rec: &mut dyn PostRecorder) {
            draw_effect(self, rec, reg, 0, Some("a"), "b", 0);
        }
    }

    #[derive(Default)]
    struct Rec(Vec<(usize, Option<String>, String, Vec<f32>)>);
    impl PostRecorder for Rec {
        fn target_size(&self, t: &str) -> [f32; 2] {
            if t == "a" { [100.0, 50.0] } else { [50.0, 25.0] }
        }
        fn draw(&mut self, s: usize, src: Option<&str>, dst: &str, u: &[f32]) {
            self.0.push((s, src.map(String::from), dst.to_string(), u.to_vec()));
        }
    }

    #[test]
    fn put_words_checks_length() {
        let mut out = [0.0; 3];
        assert_eq!(put_words(&mut out, &[1.0, 2.0]), 2);
        assert_eq!(out, [1.0, 2.0, 0.0]);
        assert_eq!(put_words(&mut out, &[1.0; 4]), 0);
    }

    #[test]
    fn draw_effect_fills_uniform_frame() {
        let mut rec = Rec::default();
        let reg = ParamRegistry::new();
        E.record("t", &reg, &mut rec);
        assert_eq!(rec.0, vec![(0, Some("a".into()), "b".into(), vec![100.0, 50.0])]);
        let fx = PostEffects {
            effects: vec![Rc::new(E)],
        };
        assert_eq!(fx.for_pass("t").map(|(i, _)| i), Some(0));
        assert!(fx.for_pass("x").is_none());
    }
}
