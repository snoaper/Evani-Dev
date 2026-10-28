//! The photoreal look's colour-grade slot: the second stage of its
//! `tonemap` slot body, run on the display-range colour the filmic stage
//! ([`crate::tonemap`]) produced.
//!
//! The renderer's tonemap pass calls only `slot_exposure` and
//! `slot_tonemap`, and a new engine slot marker would mean editing the
//! renderer's pass shaders, so the grade is a slot *of this look*: a named
//! stage spliced into the tonemap body between [`GRADE_BEGIN`] and
//! [`GRADE_END`]. It works on `var c: vec3<f32>` (display-referred linear,
//! 0..1) and must leave the result in `c`.
//!
//! Three sources, chosen by `look.photoreal.grade.source`:
//! * `look` (default): the fixed photographic grade built from the
//!   `look.photoreal.grade.*` params ([`GradeSettings::wgsl`]): contrast
//!   around mid grey, desaturation, a cool-shadow / warm-highlight split
//!   tone and a slight black lift.
//! * `custom`: the body a game handed to
//!   [`crate::PhotorealLook::with_grade_body`].
//! * `none`: no grade stage. **This is the hook for S5-FX1**: the catalog's
//!   `grade` effect (a swappable 32³ LUT) is built later in `evani-fx` as
//!   its own pass after the tonemap; a game that turns it on and wants the
//!   LUT alone sets the source to `none` (or keeps `look` to stack the LUT
//!   on top of the look's grade). Nothing here depends on that effect.

/// First line of the grade stage inside the composed tonemap body.
pub const GRADE_BEGIN: &str = "    // @photoreal-grade begin";
/// Last line of the grade stage inside the composed tonemap body.
pub const GRADE_END: &str = "    // @photoreal-grade end";

/// Name of the grade hook (what S5-FX1's `grade` effect plugs in next to).
pub const GRADE_HOOK: &str = "photoreal.grade";

/// Where the grade stage comes from (`look.photoreal.grade.source`).
pub const GRADE_SOURCES: [&str; 3] = ["look", "custom", "none"];

/// The fixed photographic grade's settings (one per `look.photoreal.grade.*`
/// param).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GradeSettings {
    /// Contrast around mid grey (1 = unchanged).
    pub contrast: f32,
    /// Saturation (1 = unchanged; documentary realism sits under 1).
    pub saturation: f32,
    /// Black lift towards a matte print (0 = none).
    pub lift: f32,
    /// How far shadows and highlights lean towards their tints (0 = none).
    pub split: f32,
    /// Shadow tint (linear RGB; normalised to keep luminance).
    pub shadow_tint: [f32; 3],
    /// Highlight tint (linear RGB; normalised to keep luminance).
    pub highlight_tint: [f32; 3],
}

impl Default for GradeSettings {
    /// Gritty documentary: a little more contrast, colour pulled back to
    /// about three quarters, cool steel-blue shadows, warm dusty highlights,
    /// blacks lifted just off zero.
    fn default() -> Self {
        Self {
            contrast: 1.08,
            saturation: 0.76,
            lift: 0.012,
            split: 0.22,
            shadow_tint: [0.82, 0.95, 1.0],
            highlight_tint: [1.0, 0.93, 0.80],
        }
    }
}

/// Format a value as a WGSL float literal (always with a decimal point).
pub(crate) fn lit(v: f32) -> String {
    let v = if v.is_finite() { v } else { 0.0 };
    format!("{v:.5}")
}

pub(crate) fn lit3(v: [f32; 3]) -> String {
    format!("vec3<f32>({}, {}, {})", lit(v[0]), lit(v[1]), lit(v[2]))
}

impl GradeSettings {
    /// The grade stage as WGSL statements on `c` (no markers).
    pub fn wgsl(&self) -> String {
        let contrast = self.contrast.clamp(0.25, 4.0);
        let saturation = self.saturation.clamp(0.0, 2.0);
        let lift = self.lift.clamp(0.0, 0.5);
        let split = self.split.clamp(0.0, 1.0);
        let tint = |t: [f32; 3]| t.map(|x| x.clamp(0.0, 4.0));
        format!(
            "    let grade_w = vec3<f32>(0.2126, 0.7152, 0.0722);\n\
             \x20   // contrast around mid grey, in log space so blacks stay black\n\
             \x20   c = 0.18 * pow(max(c, vec3<f32>(0.00001)) / 0.18, vec3<f32>({contrast}));\n\
             \x20   // pull the colour back\n\
             \x20   let grade_l = dot(c, grade_w);\n\
             \x20   c = mix(vec3<f32>(grade_l), c, {saturation});\n\
             \x20   // split tone: cool shadows, warm highlights, luminance kept\n\
             \x20   let grade_lo = {lo};\n\
             \x20   let grade_hi = {hi};\n\
             \x20   let grade_t = mix(grade_lo / max(dot(grade_lo, grade_w), 0.0001), grade_hi / max(dot(grade_hi, grade_w), 0.0001), smoothstep(0.05, 0.6, grade_l));\n\
             \x20   c = c * mix(vec3<f32>(1.0), grade_t, {split});\n\
             \x20   // matte black lift\n\
             \x20   c = c + {lift} * (vec3<f32>(1.0) - c);",
            contrast = lit(contrast),
            saturation = lit(saturation),
            lo = lit3(tint(self.shadow_tint)),
            hi = lit3(tint(self.highlight_tint)),
            split = lit(split),
            lift = lit(lift),
        )
    }
}

/// The grade stage to splice: the chosen source, wrapped in the markers.
/// `None` for source `none` (or `custom` with no body given).
pub fn stage(source: u32, settings: &GradeSettings, custom: Option<&str>) -> Option<String> {
    let body = match source {
        0 => settings.wgsl(),
        1 => custom?.to_string(),
        _ => return None,
    };
    Some(format!("{GRADE_BEGIN}\n{body}\n{GRADE_END}"))
}
