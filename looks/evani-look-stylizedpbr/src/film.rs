//! The film preset (feature `film`, S5-R5b): animated-film light on the
//! stylizedpbr look. A key/fill/kick rig that follows the camera (the sun is
//! the key), a sky/ground hemisphere, richer contrast in the tonemap, wrapped
//! skin with a warm terminator and wet eyes with a catchlight. Same params,
//! same slots and same shaders as the plain look; only values differ, all in
//! `presets/film.json`.
//!
//! The light reaches models through the engine's lit surface pass
//! (evani-render feature `look-surface`, shaders/model_lit.wgsl), which reads
//! `surface_params` 2 to 7 and 10 to 11 and the material kind from glTF
//! `extras.evani_surface.kind` ([`SurfaceKind`]). The engine's point lights
//! and ambient occlusion (`point-lights`, `gtao`) add to it as they are.
//! Forward meshes get the tonemap and hemisphere-free slot bodies only.
//!
//! [`FilmLook`] is the look with the preset; [`build_with`] is the one body
//! both share (the plain [`StylizedPbrLook`] build, told which preset).
//! Without the preset nothing here changes the plain look's bodies (tests).
//!
//! Adapted from ChimpDuty's stylizedpbr film preset (ChimpDuty, Chris
//! Pearson), fur keys left out.

use evani_core::effects::StyleClasses;
use evani_core::params::{F32Param, ParamDef, ParamRegistry};
use evani_core::plugin::{LookContext, LookPlugin, PluginError};
use serde_json::Value;

use crate::{BODY_TAG, NAME, PARAMS, ParamRow, Settings, StylizedPbrLook, lit};

/// The fill light's wrap (how far it creeps past its terminator).
pub const FILL_WRAP: f32 = 0.5;
/// Skin smoother than this reads wet (a mouth painted on the head).
pub const WET_ROUGHNESS: f32 = 0.35;
/// Size of the eyes' catchlight (1 - cos of its half angle, about 6 deg).
pub const CATCH_SIZE: f32 = 0.006;
/// The glTF material extras key the engine reads the kind from.
pub const SURFACE_KEY: &str = "evani_surface";

/// How the lit pass lights a material (glTF `extras.evani_surface.kind`).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceKind {
    /// Plain PBR.
    #[default]
    Plain,
    /// Wrapped skin with a warm terminator, wet where very smooth.
    Skin,
    /// Wet all over, with a catchlight.
    Eye,
}

impl SurfaceKind {
    /// `plain`, `skin` or `eye`.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "plain" => Some(Self::Plain),
            "skin" => Some(Self::Skin),
            "eye" => Some(Self::Eye),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Plain => "plain",
            Self::Skin => "skin",
            Self::Eye => "eye",
        }
    }

    /// The code the engine's lit shader reads (0, 1, 2).
    pub fn code(self) -> f32 {
        match self {
            Self::Plain => 0.0,
            Self::Skin => 1.0,
            Self::Eye => 2.0,
        }
    }

    /// The material extras that ask for this kind.
    pub fn extras(self) -> Value {
        serde_json::json!({ SURFACE_KEY: { "kind": self.name() } })
    }
}

/// The film params, registered as `look.stylizedpbr.<name>` only by the
/// film build. Their defaults are "off": the preset sets them.
pub const FILM_PARAMS: [ParamRow; 22] = [
    ("sky", 0.0, 0.0, 2.0, 0.01, "", "Film hemisphere: sky light level (sky and ground 0 = the plain tints times render.ambient)"),
    ("ground", 0.0, 0.0, 2.0, 0.01, "", "Film hemisphere: ground bounce level"),
    ("ground_warmth", 0.0, 0.0, 1.0, 0.01, "", "How warm the ground bounce is (0 white)"),
    ("contrast", 1.0, 0.5, 2.0, 0.01, "", "Tonemap contrast around mid grey (1 = the plain curve)"),
    ("kick", 0.0, 0.0, 2.0, 0.01, "", "Back kick light, as a share of the sun's brightness (0 = off)"),
    ("kick_wrap", 0.2, 0.0, 1.0, 0.01, "", "Kick light wrap past its terminator"),
    ("kick_yaw", 40.0, 0.0, 180.0, 1.0, "deg", "Kick azimuth: 0 straight behind the model, 180 from the camera"),
    ("kick_pitch", 35.0, -30.0, 89.0, 1.0, "deg", "Kick height"),
    ("kick_r", 1.0, 0.0, 2.0, 0.01, "", "Kick colour, red"),
    ("kick_g", 1.0, 0.0, 2.0, 0.01, "", "Kick colour, green"),
    ("kick_b", 1.0, 0.0, 2.0, 0.01, "", "Kick colour, blue"),
    ("fill", 0.0, 0.0, 2.0, 0.01, "", "Fill light, as a share of the sun's brightness (0 = off)"),
    ("fill_r", 1.0, 0.0, 2.0, 0.01, "", "Fill colour, red (also the film sky's colour)"),
    ("fill_g", 1.0, 0.0, 2.0, 0.01, "", "Fill colour, green"),
    ("fill_b", 1.0, 0.0, 2.0, 0.01, "", "Fill colour, blue"),
    ("fill_yaw", 120.0, 0.0, 180.0, 1.0, "deg", "Fill azimuth: 0 straight behind the model, 180 from the camera"),
    ("fill_pitch", 10.0, -30.0, 89.0, 1.0, "deg", "Fill height"),
    ("skin_wrap", 0.0, 0.0, 1.0, 0.01, "", "Skin: how far light wraps past the terminator (0 = off)"),
    ("skin_scatter", 0.0, 0.0, 1.0, 0.01, "", "Skin: red-shifted wrap and the warm terminator band"),
    ("eye_gloss", 0.0, 0.0, 0.5, 0.01, "", "Wet roughness of eyes and wet skin (0 = off)"),
    ("eye_catch", 0.0, 0.0, 4.0, 0.01, "", "Catchlight strength on wet surfaces (0 = off)"),
    ("hue_keep", PLAIN_HUE_KEEP, 0.0, 1.0, 0.01, "", "Tonemap: share of the hue-keeping luminance curve (0.3 = the plain curve; more keeps bright orange from sliding to yellow)"),
];

/// The plain tonemap's share of its luminance curve (`mix(pc, bl, 0.3)`).
pub const PLAIN_HUE_KEEP: f32 = 0.3;

/// Current values of [`FILM_PARAMS`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FilmSettings {
    pub sky: f32,
    pub ground: f32,
    pub ground_warmth: f32,
    pub contrast: f32,
    pub kick: f32,
    pub kick_wrap: f32,
    pub kick_yaw: f32,
    pub kick_pitch: f32,
    pub kick_color: [f32; 3],
    pub fill: f32,
    pub fill_color: [f32; 3],
    pub fill_yaw: f32,
    pub fill_pitch: f32,
    pub skin_wrap: f32,
    pub skin_scatter: f32,
    pub eye_gloss: f32,
    pub eye_catch: f32,
    pub hue_keep: f32,
}

impl Default for FilmSettings {
    fn default() -> Self {
        Self::from_values(FILM_PARAMS.map(|r| r.1))
    }
}

impl FilmSettings {
    pub fn from_values(v: [f32; 22]) -> Self {
        Self {
            sky: v[0],
            ground: v[1],
            ground_warmth: v[2],
            contrast: v[3],
            kick: v[4],
            kick_wrap: v[5],
            kick_yaw: v[6],
            kick_pitch: v[7],
            kick_color: [v[8], v[9], v[10]],
            fill: v[11],
            fill_color: [v[12], v[13], v[14]],
            fill_yaw: v[15],
            fill_pitch: v[16],
            skin_wrap: v[17],
            skin_scatter: v[18],
            eye_gloss: v[19],
            eye_catch: v[20],
            hue_keep: v[21],
        }
    }

    /// Rig light direction in the camera's frame as (back, right, up) parts,
    /// from azimuth (0 = straight behind the model, positive towards the
    /// camera's right, 180 = from the camera) and height, in degrees.
    pub fn rig_dir(yaw_deg: f32, pitch_deg: f32) -> [f32; 3] {
        let (a, e) = (yaw_deg.clamp(0.0, 180.0).to_radians(), pitch_deg.clamp(-30.0, 89.0).to_radians());
        [a.cos() * e.cos(), a.sin() * e.cos(), e.sin()]
    }

    /// The sky/ground cases (2 and 3). With `sky` and `ground` at 0 they are
    /// the plain look's tints that multiply the scene ambient; otherwise the
    /// final hemisphere light (w = 1): a sky in the fill colour, a warm
    /// ground bounce.
    pub fn hemisphere_cases(&self) -> String {
        if self.sky <= 0.0 && self.ground <= 0.0 {
            return "        case 2u: { return vec4<f32>(1.1, 1.08, 1.12, 0.0); }\n        case 3u: { return vec4<f32>(0.55, 0.48, 0.42, 0.0); }\n".into();
        }
        let sky = self.sky.max(0.0);
        let g = self.ground.max(0.0);
        let warm = self.ground_warmth.clamp(0.0, 1.0);
        let tint = |w: f32| 1.0 + (w - 1.0) * warm;
        format!(
            "        case 2u: {{ return vec4<f32>({}, {}, {}, 1.0); }}\n        case 3u: {{ return vec4<f32>({}, {}, {}, 1.0); }}\n",
            lit(self.fill_color[0].max(0.0) * sky),
            lit(self.fill_color[1].max(0.0) * sky),
            lit(self.fill_color[2].max(0.0) * sky),
            lit(g),
            lit(tint(0.82) * g),
            lit(tint(0.64) * g),
        )
    }

    /// The rig's cases (4 to 7): kick direction (camera frame) and strength,
    /// kick colour and wrap, fill direction and strength, fill colour and
    /// wrap. Left out (the slot answers 0) when both lights are off.
    pub fn rig_cases(&self) -> String {
        if self.kick <= 0.0 && self.fill <= 0.0 {
            return String::new();
        }
        let row = |case: u32, v: [f32; 3], w: f32| format!("        case {case}u: {{ return vec4<f32>({}, {}, {}, {}); }}\n", lit(v[0]), lit(v[1]), lit(v[2]), lit(w));
        let c = |v: [f32; 3]| v.map(|x| x.max(0.0));
        let mut s = row(4, Self::rig_dir(self.kick_yaw, self.kick_pitch), self.kick.max(0.0));
        s.push_str(&row(5, c(self.kick_color), self.kick_wrap.clamp(0.0, 1.0)));
        s.push_str(&row(6, Self::rig_dir(self.fill_yaw, self.fill_pitch), self.fill.max(0.0)));
        s.push_str(&row(7, c(self.fill_color), FILL_WRAP));
        s
    }

    /// The film materials' cases (10 and 11): skin (wrap, scatter, wet
    /// roughness limit) and wet surfaces (roughness, catchlight strength and
    /// size). Left out while all four strengths are 0.
    pub fn material_cases(&self) -> String {
        if self.skin_wrap <= 0.0 && self.skin_scatter <= 0.0 && self.eye_gloss <= 0.0 && self.eye_catch <= 0.0 {
            return String::new();
        }
        let row = |case: u32, v: [f32; 4]| format!("        case {case}u: {{ return vec4<f32>({}, {}, {}, {}); }}\n", lit(v[0]), lit(v[1]), lit(v[2]), lit(v[3]));
        let mut s = row(10, [self.skin_wrap.clamp(0.0, 1.0), self.skin_scatter.clamp(0.0, 1.0), WET_ROUGHNESS, 0.0]);
        s.push_str(&row(11, [self.eye_gloss.clamp(0.0, 0.5), self.eye_catch.clamp(0.0, 4.0), CATCH_SIZE, 0.0]));
        s
    }

    /// `surface_params` with the film cases: the plain look's 0 and 1, then
    /// the hemisphere, the rig and the materials.
    pub fn surface_params_body(&self, base: &Settings) -> String {
        format!(
            "    // {BODY_TAG}: surface constants (film)\n    switch i {{\n        case 0u: {{ return vec4<f32>({}, {}, 2.2, 0.0); }}\n        case 1u: {{ return vec4<f32>({}, {}, 0.0, 0.0); }}\n{}{}{}        default: {{ return vec4<f32>(0.0); }}\n    }}",
            lit(base.ao_strength.clamp(0.0, 1.0)),
            lit(base.env_spec.clamp(0.0, 1.0)),
            lit(base.detail_strength.clamp(0.0, 1.0)),
            lit(base.detail_scale.max(0.001)),
            self.hemisphere_cases(),
            self.rig_cases(),
            self.material_cases(),
        )
    }

    /// The plain filmic curve, with `hue_keep` of the hue-keeping luminance
    /// curve instead of the plain 0.3 (bright saturated colours, like the
    /// orange glow, stay orange instead of sliding to yellow channel by
    /// channel), then (contrast other than 1) pushed away from the curve's
    /// own mid grey (0.18 maps to 0.24) by a power: shadows deepen and
    /// lights open up, black and white stay where they are.
    pub fn tonemap_body(&self, base: &Settings) -> String {
        let plain = base.tonemap_body();
        let keep = self.hue_keep.clamp(0.0, 1.0);
        if (self.contrast - 1.0).abs() < 1e-4 && (keep - PLAIN_HUE_KEEP).abs() < 1e-4 {
            return plain;
        }
        plain.replace(
            PLAIN_TONEMAP_TAIL,
            &format!(
                "    let o = clamp(mix(pc, bl, {}), vec3<f32>(1e-6), vec3<f32>(1.0));\n    return clamp(0.24 * pow(o / 0.24, vec3<f32>({})), vec3<f32>(0.0), vec3<f32>(1.0));",
                lit(keep),
                lit(self.contrast.clamp(0.5, 2.0))
            ),
        )
    }
}

/// The last line of the plain tonemap body, where contrast joins.
const PLAIN_TONEMAP_TAIL: &str = "    return clamp(mix(pc, bl, 0.3), vec3<f32>(0.0), vec3<f32>(1.0));";

/// The look's slot bodies, with the film's when it is on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tuned {
    pub base: Settings,
    pub film: Option<FilmSettings>,
}

impl Tuned {
    /// Every slot body: the plain look's, with `surface_params` and
    /// `tonemap` from the film when it is on.
    pub fn bodies(&self) -> Vec<(&'static str, String)> {
        let mut v = self.base.bodies();
        if let Some(f) = &self.film {
            for (name, body) in &mut v {
                match *name {
                    "surface_params" => *body = f.surface_params_body(&self.base),
                    "tonemap" => *body = f.tonemap_body(&self.base),
                    _ => {}
                }
            }
        }
        v
    }
}

/// A named set of param values and engine `render.*` values.
#[derive(Debug, Clone, PartialEq)]
pub struct Preset {
    pub name: String,
    /// (param suffix, value); the rest keep their defaults.
    pub params: Vec<(String, f32)>,
    /// Engine values the look asks for (`render.*`, `fx.*`), over its own.
    pub post: Vec<(String, Value)>,
}

impl Preset {
    /// Reads a preset: `{"name", "params": {suffix: number}, "post": {name:
    /// value}}`. A param that is in neither table, or outside its range, is
    /// an error (a typo must not pass silently).
    pub fn parse(json: &str) -> Result<Preset, String> {
        let v: Value = serde_json::from_str(json).map_err(|e| format!("preset json: {e}"))?;
        let name = v.get("name").and_then(Value::as_str).ok_or("preset has no name")?.to_string();
        let mut params = Vec::new();
        if let Some(map) = v.get("params").and_then(Value::as_object) {
            for (k, x) in map {
                let row = PARAMS.iter().chain(FILM_PARAMS.iter()).find(|r| r.0 == k).ok_or_else(|| format!("preset {name}: no param {k}"))?;
                let f = x.as_f64().ok_or_else(|| format!("preset {name}: {k} is not a number"))? as f32;
                if !(row.2..=row.3).contains(&f) {
                    return Err(format!("preset {name}: {k} = {f} is outside {}..{}", row.2, row.3));
                }
                params.push((k.clone(), f));
            }
        }
        let post = v.get("post").and_then(Value::as_object).map(|m| m.iter().map(|(k, x)| (k.clone(), x.clone())).collect()).unwrap_or_default();
        Ok(Preset { name, params, post })
    }

    /// This preset's value for `suffix`, else `default`.
    pub fn value(&self, suffix: &str, default: f32) -> f32 {
        self.params.iter().find(|(k, _)| k == suffix).map_or(default, |(_, x)| *x)
    }

    /// The plain look's settings with this preset's values on top.
    pub fn settings(&self) -> Settings {
        let mut v = [0.0; 12];
        for (i, row) in PARAMS.iter().enumerate() {
            v[i] = self.value(row.0, row.1);
        }
        Settings::from_values(v)
    }

    /// The film settings with this preset's values on top.
    pub fn film_settings(&self) -> FilmSettings {
        FilmSettings::from_values(FILM_PARAMS.map(|r| self.value(r.0, r.1)))
    }
}

/// The film preset's data.
pub const FILM_PRESET_JSON: &str = include_str!("../presets/film.json");

/// A preset for [`build_with`].
#[derive(Debug, Clone, PartialEq)]
pub struct Film {
    pub preset: Preset,
}

impl Film {
    /// The film preset ([`FILM_PRESET_JSON`]).
    pub fn preset() -> Result<Film, String> {
        Preset::parse(FILM_PRESET_JSON).map(|preset| Film { preset })
    }
}

/// The preset the next plain build applies (set by [`build_with`]).
struct FilmRequest(Option<Film>);

/// The look's build with `film` (None: the plain look). The one body both
/// [`StylizedPbrLook`] and [`FilmLook`] run.
pub fn build_with(look: &mut LookContext, film: Option<Film>) -> Result<(), PluginError> {
    look.world.insert_resource(FilmRequest(film));
    StylizedPbrLook.build(look)
}

/// The preset [`build_with`] asked for, once.
pub fn take(look: &mut LookContext) -> Option<Film> {
    look.world.remove_resource::<FilmRequest>().and_then(|r| r.0)
}

/// A base param's start value: the preset's, else its default.
pub fn start(film: Option<&Film>, suffix: &str, default: f32) -> f32 {
    film.map_or(default, |f| f.preset.value(suffix, default))
}

/// Registers the film params (none without a preset), starting at the
/// preset's values.
pub fn register(look: &mut LookContext, film: Option<&Film>) -> Result<Vec<F32Param>, PluginError> {
    let Some(f) = film else {
        return Ok(Vec::new());
    };
    let mut handles = Vec::with_capacity(FILM_PARAMS.len());
    for (suffix, default, min, max, step, unit, description) in FILM_PARAMS {
        let def = ParamDef::f32(&format!("look.{NAME}.{suffix}"), f.preset.value(suffix, default)).range(min, max).step(step).unit(unit).describe(description);
        handles.push(look.param_f32(def)?);
    }
    Ok(handles)
}

/// The bodies a build starts from.
pub fn start_bodies(film: Option<&Film>) -> Vec<(&'static str, String)> {
    match film {
        Some(f) => Tuned { base: f.preset.settings(), film: Some(f.preset.film_settings()) }.bodies(),
        None => Settings::default().bodies(),
    }
}

/// The film settings from the registry (None without the film params).
pub fn read(params: &ParamRegistry, handles: &[F32Param]) -> Option<FilmSettings> {
    if handles.is_empty() {
        return None;
    }
    let mut v = FILM_PARAMS.map(|r| r.1);
    for (i, h) in handles.iter().enumerate().take(v.len()) {
        v[i] = params.f32(*h);
    }
    Some(FilmSettings::from_values(v))
}

/// The preset's engine values, after the look's own (later wins). An `fx.*`
/// value the game doesn't build (no bloom in `effects`) is left out, as the
/// plain look does, instead of logging "not in this build".
pub fn post(look: &mut LookContext, film: Option<&Film>) {
    for (name, value) in film.map_or(&[][..], |f| &f.preset.post[..]) {
        if name.starts_with("fx.") && !look.world.try_res::<ParamRegistry>().is_some_and(|r| r.id(name).is_some()) {
            continue;
        }
        look.post_param(name, value.clone());
    }
}

/// The stylizedpbr look with the film preset.
#[derive(Debug, Default, Clone, Copy)]
pub struct FilmLook;

impl LookPlugin for FilmLook {
    fn name(&self) -> &'static str {
        NAME
    }

    fn version(&self) -> Option<&'static str> {
        Some(env!("CARGO_PKG_VERSION"))
    }

    fn points(&self) -> u32 {
        3
    }

    fn styles(&self) -> StyleClasses {
        StyleClasses::THREE_D
    }

    fn build(&self, look: &mut LookContext) -> Result<(), PluginError> {
        build_with(look, Some(Film::preset().map_err(PluginError)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cases(body: &str) -> Vec<u32> {
        body.lines().filter_map(|l| l.trim().strip_prefix("case ")).filter_map(|l| l.split('u').next()).filter_map(|n| n.parse().ok()).collect()
    }

    #[test]
    fn the_plain_look_has_no_film_cases() {
        let plain = Settings::default().surface_params_body();
        assert!(cases(&plain).iter().all(|c| !(4..=11).contains(c)), "{plain}");
        assert_eq!(Tuned { base: Settings::default(), film: None }.bodies(), Settings::default().bodies());
        assert_eq!(start_bodies(None), Settings::default().bodies());
        // Film params at their defaults add nothing either.
        let off = FilmSettings::default();
        assert!(off.rig_cases().is_empty() && off.material_cases().is_empty());
        assert_eq!(off.tonemap_body(&Settings::default()), Settings::default().tonemap_body());
    }

    #[test]
    fn the_film_preset_sets_the_rig_skin_and_eyes() {
        let f = Film::preset().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(f.preset.name, "film");
        let bodies = start_bodies(Some(&f));
        let sp = &bodies.iter().find(|(n, _)| *n == "surface_params").unwrap_or_else(|| panic!("surface_params")).1;
        assert_eq!(cases(sp), [0, 1, 2, 3, 4, 5, 6, 7, 10, 11], "{sp}");
        let tm = &bodies.iter().find(|(n, _)| *n == "tonemap").unwrap_or_else(|| panic!("tonemap")).1;
        // A softer contrast (dark stone keeps mid-tones, white stays off the clip), with the
        // hue-keeping share (glows stay orange).
        assert!(tm.contains("mix(pc, bl, 0.90000)") && tm.contains("pow(o / 0.24, vec3<f32>(0.85000))"), "{tm}");
        // The preset's base values reach the base params (rim 0.3).
        assert!((f.preset.settings().rim - 0.3).abs() < 1e-6);
        assert!(f.preset.post.iter().any(|(n, _)| n == "render.light.intensity"));
    }

    #[test]
    fn presets_refuse_unknown_or_out_of_range_params() {
        assert!(Preset::parse(r#"{"name": "x", "params": {"fur_spec": 0.1}}"#).is_err(), "no fur keys");
        assert!(Preset::parse(r#"{"name": "x", "params": {"kick": 9.0}}"#).is_err());
        assert!(Preset::parse(r#"{"params": {}}"#).is_err());
        assert!(!FILM_PRESET_JSON.contains("fur_"));
    }

    #[test]
    fn rig_directions_are_unit_and_kinds_round_trip() {
        for (y, p) in [(0.0, 0.0), (40.0, 35.0), (120.0, 10.0), (180.0, 89.0)] {
            let d = FilmSettings::rig_dir(y, p);
            assert!(((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() - 1.0).abs() < 1e-5);
        }
        assert!(FilmSettings::rig_dir(0.0, 0.0)[0] > 0.99, "yaw 0 is straight behind");
        for k in [SurfaceKind::Plain, SurfaceKind::Skin, SurfaceKind::Eye] {
            assert_eq!(SurfaceKind::parse(k.name()), Some(k));
            assert_eq!(k.extras()[SURFACE_KEY]["kind"], k.name());
        }
        assert_eq!([SurfaceKind::Plain.code(), SurfaceKind::Skin.code(), SurfaceKind::Eye.code()], [0.0, 1.0, 2.0]);
        assert_eq!(SurfaceKind::parse("fur"), None);
    }

    #[test]
    fn hue_keep_sets_the_luminance_share_of_the_tonemap() {
        let base = Settings::default();
        let plain = base.tonemap_body();
        assert!(plain.contains(PLAIN_TONEMAP_TAIL), "{plain}");
        // The plain share with contrast 1 is the plain curve.
        let mut f = FilmSettings { hue_keep: PLAIN_HUE_KEEP, ..FilmSettings::default() };
        assert_eq!(f.tonemap_body(&base), plain);
        f.hue_keep = 0.8;
        let tm = f.tonemap_body(&base);
        assert!(tm.contains("mix(pc, bl, 0.80000)") && tm.contains("vec3<f32>(1.00000)"), "{tm}");
        f.hue_keep = 7.0;
        assert!(f.tonemap_body(&base).contains("mix(pc, bl, 1.00000)"), "clamped to 1");
    }
}
