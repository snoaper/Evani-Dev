//! The material record: which character kind a glTF material is and its own
//! colours and sizes, written into the 16-word surface uniform the lit
//! surface pass reads (evani-render `model::surface`).
//!
//! A material is tagged in its glTF `extras`:
//!
//! ```json
//! "extras": {
//!   "evani_kind": "eye",
//!   "evani_eye": { "iris_color": [0.30, 0.16, 0.05], "iris_size": 0.48, "pupil_size": 0.36, "sclera_color": [0.78, 0.74, 0.70] }
//! }
//! ```
//!
//! `evani_kind` is `plain`, `skin`, `eye` or `hair` (the older
//! `evani_surface.kind` tag is read too, so models tagged for the film rig
//! get the new shading). Every per-material value is optional: a missing
//! one is written as -1 and the shader uses the live param
//! (`material.eye.iris_color` and so on) instead. Unknown kinds stay plain.
//!
//! Word layout (`surface` uniform: `pbr` 0..3, `spare0` 4..7, `spare1`
//! 8..11, `kind` 12..15), only for the kinds below:
//!
//! | words | eye             | skin             | hair            |
//! |-------|-----------------|------------------|-----------------|
//! | 4-6   | iris colour     | scatter colour   | highlight tint  |
//! | 7     | iris size       | pore strength    | highlight shift |
//! | 8-10  | sclera colour   | wrinkle strength, pore scale, - | edge softness, tip width, - |
//! | 11    | pupil size      | -                | -               |
//! | 14    | kind code 2     | kind code 1      | kind code 3     |

use serde_json::Value;

/// Floats in the surface uniform (evani-render `surface::SURFACE_WORDS`).
pub const WORDS: usize = 16;
/// The word holding the kind code (evani-render `surface::KIND_WORD`).
pub const KIND_WORD: usize = 14;
/// What a per-material value reads as when the material doesn't set it.
pub const UNSET: f32 = -1.0;

/// A material's character kind.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Plain,
    Skin,
    Eye,
    Hair,
}

impl Kind {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "plain" => Some(Self::Plain),
            "skin" => Some(Self::Skin),
            "eye" => Some(Self::Eye),
            "hair" | "fur" => Some(Self::Hair),
            _ => None,
        }
    }

    /// The code the shader reads (`surface.kind.z`). Skin and eye keep the
    /// lit pass's existing codes.
    pub fn code(self) -> f32 {
        match self {
            Self::Plain => 0.0,
            Self::Skin => 1.0,
            Self::Eye => 2.0,
            Self::Hair => 3.0,
        }
    }

    /// The extras object holding this kind's own values.
    fn block(self) -> &'static str {
        match self {
            Self::Plain => "",
            Self::Skin => "evani_skin",
            Self::Eye => "evani_eye",
            Self::Hair => "evani_hair",
        }
    }
}

/// The kind named by `extras.evani_kind`, else `extras.evani_surface.kind`
/// (missing or unknown: plain).
pub fn kind(extras: &Value) -> Kind {
    let named = extras
        .get("evani_kind")
        .and_then(Value::as_str)
        .or_else(|| extras.get("evani_surface").and_then(|e| e.get("kind")).and_then(Value::as_str));
    named.and_then(Kind::parse).unwrap_or_default()
}

/// One value written by [`apply`]: a colour (3 words) or a number (1 word).
enum Field {
    Color(&'static str, usize),
    Num(&'static str, usize, f32, f32),
}

const EYE: [Field; 4] = [
    Field::Color("iris_color", 4),
    Field::Num("iris_size", 7, 0.1, 0.9),
    Field::Color("sclera_color", 8),
    Field::Num("pupil_size", 11, 0.05, 0.9),
];
const SKIN: [Field; 4] = [
    Field::Color("scatter_color", 4),
    Field::Num("pore_strength", 7, 0.0, 2.0),
    Field::Num("wrinkle_strength", 8, 0.0, 2.0),
    Field::Num("pore_scale", 9, 1.0, 4000.0),
];
const HAIR: [Field; 4] = [
    Field::Color("highlight_color", 4),
    Field::Num("shift", 7, -1.0, 1.0),
    Field::Num("edge_softness", 8, 0.0, 1.0),
    Field::Num("tip_width", 9, 0.0, 1.0),
];

fn fields(k: Kind) -> &'static [Field] {
    match k {
        Kind::Plain => &[],
        Kind::Skin => &SKIN,
        Kind::Eye => &EYE,
        Kind::Hair => &HAIR,
    }
}

fn color(v: &Value) -> Option<[f32; 3]> {
    let a = v.as_array()?;
    if a.len() < 3 {
        return None;
    }
    let mut c = [0.0; 3];
    for (o, x) in c.iter_mut().zip(a) {
        let x = x.as_f64()? as f32;
        if !x.is_finite() {
            return None;
        }
        *o = x.clamp(0.0, 16.0);
    }
    Some(c)
}

/// Write the kind code and the material's own values into `words` (a plain
/// or untagged material leaves them untouched, so models without tags load
/// exactly as before). Returns the kind.
pub fn apply(extras: &Value, words: &mut [f32; WORDS]) -> Kind {
    let k = kind(extras);
    if k == Kind::Plain {
        return k;
    }
    words[KIND_WORD] = k.code();
    let block = extras.get(k.block());
    for f in fields(k) {
        match *f {
            Field::Color(name, at) => {
                let c = block.and_then(|b| b.get(name)).and_then(color).unwrap_or([UNSET; 3]);
                words[at..at + 3].copy_from_slice(&c);
            }
            Field::Num(name, at, lo, hi) => {
                let x = block.and_then(|b| b.get(name)).and_then(Value::as_f64).map(|x| x as f32).filter(|x| x.is_finite());
                words[at] = x.map_or(UNSET, |x| x.clamp(lo, hi));
            }
        }
    }
    k
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn words() -> [f32; WORDS] {
        let mut w = [0.0; WORDS];
        w[1] = 0.5;
        w
    }

    #[test]
    fn kinds_from_both_tags() {
        assert_eq!(kind(&json!({"evani_kind": "eye"})), Kind::Eye);
        assert_eq!(kind(&json!({"evani_kind": "skin"})), Kind::Skin);
        assert_eq!(kind(&json!({"evani_kind": "hair"})), Kind::Hair);
        assert_eq!(kind(&json!({"evani_kind": "fur"})), Kind::Hair);
        assert_eq!(kind(&json!({"evani_surface": {"kind": "eye"}})), Kind::Eye, "the film rig's tag");
        assert_eq!(kind(&json!({"evani_kind": "hair", "evani_surface": {"kind": "eye"}})), Kind::Hair, "evani_kind wins");
        assert_eq!(kind(&json!({"evani_kind": "velvet"})), Kind::Plain, "unknown stays plain");
        assert_eq!(kind(&json!({"evani_kind": 3})), Kind::Plain);
        assert_eq!(kind(&Value::Null), Kind::Plain);
        let codes: Vec<f32> = [Kind::Plain, Kind::Skin, Kind::Eye, Kind::Hair].iter().map(|k| k.code()).collect();
        assert_eq!(codes, [0.0, 1.0, 2.0, 3.0]);
    }

    #[test]
    fn plain_and_untagged_materials_are_untouched() {
        for e in [Value::Null, json!({}), json!({"evani_kind": "plain"}), json!({"evani_kind": "nope", "evani_eye": {"iris_size": 0.5}})] {
            let mut w = words();
            assert_eq!(apply(&e, &mut w), Kind::Plain);
            assert_eq!(w, words(), "{e}");
        }
    }

    #[test]
    fn eye_values_land_in_their_words_and_missing_ones_are_unset() {
        let mut w = words();
        let e = json!({"evani_kind": "eye", "evani_eye": {"iris_color": [0.3, 0.2, 0.1], "pupil_size": 0.4}});
        assert_eq!(apply(&e, &mut w), Kind::Eye);
        assert_eq!(w[KIND_WORD], 2.0);
        assert_eq!(&w[4..7], &[0.3, 0.2, 0.1]);
        assert_eq!(w[7], UNSET, "iris size not given: the live param");
        assert_eq!(&w[8..11], &[UNSET; 3]);
        assert_eq!(w[11], 0.4);
        assert_eq!((w[0], w[1], w[2]), (0.0, 0.5, 0.0), "the PBR words stay");
    }

    #[test]
    fn values_are_clamped_and_bad_ones_ignored() {
        let mut w = words();
        let e = json!({"evani_kind": "skin", "evani_skin": {"scatter_color": [2.0, -1.0], "pore_strength": 9.0, "pore_scale": "big"}});
        apply(&e, &mut w);
        assert_eq!(w[KIND_WORD], 1.0);
        assert_eq!(&w[4..7], &[UNSET; 3], "a two-number colour is not a colour");
        assert_eq!(w[7], 2.0, "clamped to its range");
        assert_eq!(w[9], UNSET);
        let mut w = words();
        apply(&json!({"evani_kind": "hair", "evani_hair": {"highlight_color": [1.0, 0.9, 0.8], "shift": -3.0, "tip_width": 0.2}}), &mut w);
        assert_eq!((w[KIND_WORD], w[7], w[8], w[9]), (3.0, -1.0, UNSET, 0.2));
        assert_eq!(&w[4..7], &[1.0, 0.9, 0.8]);
    }
}
