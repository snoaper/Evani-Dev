//! The style detail step (feature `style`; dev viewers only, no game lists
//! it): keeps a recipe material's own pattern (grain, stripes, scales, rust)
//! inside a non-realistic art style's flat colours, simplified the style's
//! way. See `shaders/style.wgsl` for what the shader does.
//!
//! The hook is small on purpose. With the feature on, [`crate::shader`]
//! rewrites two lines of the stripped core (the last line of `mat_surface`
//! and of `mat_extra`) to call the step and appends its WGSL; the step reads
//! one word of the per-frame uniform, `kd.w` ([`STYLE_WORD`], reserved in the
//! plain stage), so no binding, buffer size or layout changes. evani-render's
//! `material-recipe-style` writes that word from the [`RecipeStyle`] world
//! resource each frame. Without the feature none of this is compiled and the
//! stage's text is byte for byte the plain one.

use crate::shader::{wgsl_strip, wgsl_stripped, utf8};

/// The style step's WGSL, comments stripped at compile time.
pub const STYLE_WGSL: &str = utf8(&STYLE_BYTES);
const STYLE_SRC: &[u8] = include_str!("../shaders/style.wgsl").as_bytes();
const STYLE_BYTES: [u8; wgsl_strip(STYLE_SRC, &mut [])] = wgsl_stripped(STYLE_SRC);

/// The per-frame uniform word the style lives in (`kd.w`).
pub const STYLE_WORD: usize = 31;

/// The two core lines the hook rewrites (source text; matched stripped).
pub const HOOKS: [(&str, &str); 2] = [
    (
        "return mat_conditions(b, f, i, rc_dispatch(b, f, i, o));",
        "return rc_style_surface(b, f, i, o);",
    ),
    (
        "return rc_post(c0 + c, o, g, v, l, sun, vis);",
        "return rc_style_post(c0, rc_post(c0 + c, o, g, v, l, sun, vis));",
    ),
];

/// `s` as the tight strip leaves it (how the core is stored).
pub fn tight(s: &str) -> String {
    let n = wgsl_strip(s.as_bytes(), &mut []);
    let mut out = vec![0u8; n];
    wgsl_strip(s.as_bytes(), &mut out);
    String::from_utf8(out).unwrap_or_default()
}

/// The stripped core with the two hooks in, or `None` if a line is missing
/// (the caller keeps the plain core: no style, never a broken stage).
pub fn hooked(core: &str) -> Option<String> {
    let mut s = core.to_string();
    for (find, with) in HOOKS {
        let (find, with) = (tight(find), tight(with));
        let at = s.find(&find)?;
        s.replace_range(at..at + find.len(), &with);
    }
    Some(s)
}

/// How the style step simplifies the pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// The plain recipe stage.
    #[default]
    Off,
    /// The pattern's brightness (and some of its hue) times the flat colour:
    /// toon and cel bands, pixel and voxel cubes, the lit styles.
    Tone,
    /// The pattern's dark parts set the ink density on paper.
    Dither,
    /// The tone, plus a glow along the pattern's edges.
    Neon,
}

/// One style's settings for the step.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Style {
    pub mode: Mode,
    /// Posterize step in stops (0 = smooth; up to 1.75, in quarters).
    pub step: f32,
    /// 0 = no pattern, 1 = the pattern as it is, up to 2.
    pub strength: f32,
    /// How much of the pattern's own hue is kept (0 = brightness only).
    pub hue: f32,
    /// Cell size in metres (0 = none; up to about 26 m).
    pub cell: f32,
}

/// The largest cell the word holds (metres).
pub const CELL_MAX: f32 = ((1u32 << 18) - 1) as f32 / 10_000.0;

impl Style {
    /// The uniform word (0 when off or with no strength).
    pub fn pack(&self) -> u32 {
        let mode = match self.mode {
            Mode::Off => return 0,
            Mode::Tone => 1u32,
            Mode::Dither => 2,
            Mode::Neon => 3,
        };
        let fin = |x: f32| if x.is_finite() { x } else { 0.0 };
        let step = (fin(self.step) * 4.0).round().clamp(0.0, 7.0) as u32;
        let strength = (fin(self.strength) * 63.5).round().clamp(0.0, 127.0) as u32;
        let hue = (fin(self.hue) * 3.0).round().clamp(0.0, 3.0) as u32;
        let cell = (fin(self.cell) * 10_000.0).round().clamp(0.0, ((1u32 << 18) - 1) as f32) as u32;
        mode | step << 2 | strength << 5 | hue << 12 | cell << 14
    }

    /// The settings a word holds (the shader's reading of it).
    pub fn unpack(w: u32) -> Self {
        let mode = match w & 3 {
            0 => Mode::Off,
            1 => Mode::Tone,
            2 => Mode::Dither,
            _ => Mode::Neon,
        };
        Self {
            mode,
            step: ((w >> 2) & 7) as f32 * 0.25,
            strength: ((w >> 5) & 127) as f32 / 63.5,
            hue: ((w >> 12) & 3) as f32 / 3.0,
            cell: (w >> 14) as f32 / 10_000.0,
        }
    }

    /// The cell as the word stores it.
    pub fn stored_cell(&self) -> f32 {
        Self::unpack(Style { mode: Mode::Tone, ..*self }.pack()).cell
    }

    /// The centre of the shader's cell holding `x` (one axis): a model whose
    /// cubes are centred on multiples of the cell size (in its own space) is
    /// placed here, so each cube is one of the shader's cells.
    pub fn cell_centre(&self, x: f32) -> f32 {
        let c = self.stored_cell();
        if c <= 0.0 || !x.is_finite() { x } else { ((x / c).floor() + 0.5) * c }
    }
}

/// The style word the renderer writes into the recipe uniform each frame (a
/// world resource; absent or 0 = the plain stage).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RecipeStyle(pub u32);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shader::CORE_STRIPPED;

    #[test]
    fn both_hooks_land_once() {
        let h = hooked(CORE_STRIPPED).expect("both core lines found");
        assert_eq!(h.matches("rc_style_surface(").count(), 1);
        assert_eq!(h.matches("rc_style_post(").count(), 1);
        assert!(!h.contains(&tight(HOOKS[0].0)), "the plain mat_surface tail is gone");
    }

    #[test]
    fn a_word_round_trips() {
        let s = Style { mode: Mode::Neon, step: 0.75, strength: 1.2, hue: 2.0 / 3.0, cell: 0.0725 };
        let w = s.pack();
        let back = Style::unpack(w);
        assert_eq!(back.mode, Mode::Neon);
        assert!((back.step - 0.75).abs() < 1e-6);
        assert!((back.strength - 1.2).abs() < 0.01);
        assert!((back.hue - 2.0 / 3.0).abs() < 1e-6);
        assert!((back.cell - 0.0725).abs() < 1e-4);
        assert_eq!(Style::unpack(back.pack()).pack(), w, "stable");
        assert_eq!(Style { mode: Mode::Off, strength: 1.0, ..Style::default() }.pack(), 0);
        let big = Style { mode: Mode::Tone, cell: 1e9, strength: f32::NAN, ..Style::default() };
        assert!((Style::unpack(big.pack()).cell - CELL_MAX).abs() < 1e-3);
        assert_eq!(Style::unpack(big.pack()).strength, 0.0);
    }

    /// The recipe runs from one call site and never in a loop: browser shader
    /// compilers inline the whole dispatch at each call, and a second call
    /// plus a 3-pass loop took the pipeline build from 32 s to 374 s.
    #[test]
    fn the_recipe_is_called_once_and_never_looped() {
        assert_eq!(STYLE_WGSL.matches("rc_dispatch(").count(), 1);
        assert_eq!(STYLE_WGSL.matches("mat_conditions(").count(), 1);
        for word in ["for(", "for (", "loop{", "loop {", "while"] {
            assert!(!STYLE_WGSL.contains(word), "no `{word}` in the style step");
        }
        let h = hooked(crate::shader::CORE_STRIPPED).expect("hooked");
        assert_eq!(h.matches("rc_dispatch(").count(), 0, "the core's own call moved into the step");
    }

    #[test]
    fn the_step_is_stripped() {
        assert!(STYLE_WGSL.starts_with("// evani.shader.recipes.style"));
        assert!(!STYLE_WGSL.contains("reserved in the plain stage"), "comments stripped");
        assert!(STYLE_WGSL.contains("fn rc_style_surface("));
    }
}
