//! Reading the layers: from the glTF extensions (the converter,
//! [`Layers::from_khr`]) into `extras.evani_layers` ([`Layers::to_extras`]),
//! and from those extras at model load ([`Layers::from_extras`]), plus the
//! artist-authored `extras.evani_cloth` block (subsurface colour, weave).
//!
//! Defaults follow the extension specs: a missing factor is the spec's
//! default, and with no thickness texture the iridescence film is its
//! maximum thickness. Textures inside the extensions (sheen colour map,
//! clear coat maps, anisotropy direction map, thickness map) are not read:
//! the record holds factors only (the converter reports what it dropped).

use serde_json::{Map, Value, json};

use crate::{
    ANISOTROPY, CLEARCOAT, FLAGS_WORD, IRIDESCENCE, LAYER_WORDS, SHEEN, WEAVE_KNIT, WEAVE_PLAIN,
};

/// The extension names this crate reads.
pub const KHR_SHEEN: &str = "KHR_materials_sheen";
pub const KHR_CLEARCOAT: &str = "KHR_materials_clearcoat";
pub const KHR_ANISOTROPY: &str = "KHR_materials_anisotropy";
pub const KHR_IRIDESCENCE: &str = "KHR_materials_iridescence";
pub const KHR_NAMES: [&str; 4] = [KHR_SHEEN, KHR_CLEARCOAT, KHR_ANISOTROPY, KHR_IRIDESCENCE];

/// The extras key the converter writes and the loader reads.
pub const EXTRAS_KEY: &str = "evani_layers";
/// The extras key for the cloth extras an artist sets by hand.
pub const CLOTH_KEY: &str = "evani_cloth";

/// The woven detail laid over a cloth material.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Weave {
    #[default]
    None,
    /// Over-under ridges of a plain weave (cotton, canvas, linen).
    Plain,
    /// V-shaped loops of a knit (jumpers, socks).
    Knit,
}

impl Weave {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "none" => Some(Self::None),
            "plain" => Some(Self::Plain),
            "knit" => Some(Self::Knit),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Plain => "plain",
            Self::Knit => "knit",
        }
    }
}

/// The layers of one material: factors only, zero where unused.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layers {
    /// Sheen colour (linear) and roughness (perceptual, 0..1).
    pub sheen_color: [f32; 3],
    pub sheen_roughness: f32,
    /// Clear coat strength and roughness.
    pub clearcoat: f32,
    pub clearcoat_roughness: f32,
    /// Anisotropy strength (0..1) and rotation (radians, counter-clockwise
    /// from the tangent).
    pub anisotropy: f32,
    pub anisotropy_rotation: f32,
    /// Iridescence strength, film index of refraction and film thickness
    /// (nanometres).
    pub iridescence: f32,
    pub iridescence_ior: f32,
    pub iridescence_thickness: f32,
    /// Cloth extras: subsurface colour (zero: none), weave and its repeats
    /// per UV unit.
    pub subsurface: [f32; 3],
    pub weave: Weave,
    pub weave_scale: f32,
}

impl Default for Layers {
    /// No layer at all (the spec defaults of the unused fields).
    fn default() -> Self {
        Self {
            sheen_color: [0.0; 3],
            sheen_roughness: 0.0,
            clearcoat: 0.0,
            clearcoat_roughness: 0.0,
            anisotropy: 0.0,
            anisotropy_rotation: 0.0,
            iridescence: 0.0,
            iridescence_ior: 1.3,
            iridescence_thickness: 400.0,
            subsurface: [0.0; 3],
            weave: Weave::None,
            weave_scale: 24.0,
        }
    }
}

fn num(v: Option<&Value>) -> Option<f32> {
    v.and_then(Value::as_f64)
        .map(|f| f as f32)
        .filter(|f| f.is_finite())
}

fn rgb(v: Option<&Value>) -> Option<[f32; 3]> {
    let a = v?.as_array()?;
    if a.len() < 3 {
        return None;
    }
    Some([num(a.first())?, num(a.get(1))?, num(a.get(2))?])
}

fn clamp3(c: [f32; 3]) -> [f32; 3] {
    c.map(|x| x.clamp(0.0, 1.0))
}

impl Layers {
    /// From a glTF material's `extensions` object (the converter). `None`
    /// when it carries none of the four extensions.
    pub fn from_khr(extensions: &Value) -> Option<Self> {
        let ext = extensions.as_object()?;
        if !KHR_NAMES.iter().any(|n| ext.contains_key(*n)) {
            return None;
        }
        let mut l = Self::default();
        if let Some(s) = ext.get(KHR_SHEEN) {
            l.sheen_color = clamp3(rgb(s.get("sheenColorFactor")).unwrap_or([0.0; 3]));
            l.sheen_roughness = num(s.get("sheenRoughnessFactor"))
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);
        }
        if let Some(c) = ext.get(KHR_CLEARCOAT) {
            l.clearcoat = num(c.get("clearcoatFactor")).unwrap_or(0.0).clamp(0.0, 1.0);
            l.clearcoat_roughness = num(c.get("clearcoatRoughnessFactor"))
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);
        }
        if let Some(a) = ext.get(KHR_ANISOTROPY) {
            l.anisotropy = num(a.get("anisotropyStrength"))
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);
            l.anisotropy_rotation = num(a.get("anisotropyRotation")).unwrap_or(0.0);
        }
        if let Some(i) = ext.get(KHR_IRIDESCENCE) {
            l.iridescence = num(i.get("iridescenceFactor"))
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);
            l.iridescence_ior = num(i.get("iridescenceIor")).unwrap_or(1.3).clamp(1.0, 3.0);
            // No thickness texture: the spec's film is its maximum.
            l.iridescence_thickness = num(i.get("iridescenceThicknessMaximum"))
                .unwrap_or(400.0)
                .clamp(0.0, 2000.0);
        }
        Some(l)
    }

    /// The textures inside the four extensions that the record leaves out
    /// (`"KHR_materials_sheen.sheenColorTexture"`...), for the converter's
    /// report.
    pub fn dropped_textures(extensions: &Value) -> Vec<String> {
        let mut out = Vec::new();
        let Some(ext) = extensions.as_object() else {
            return out;
        };
        for n in KHR_NAMES {
            if let Some(o) = ext.get(n).and_then(Value::as_object) {
                for (k, v) in o {
                    if k.ends_with("Texture") && v.is_object() {
                        out.push(format!("{n}.{k}"));
                    }
                }
            }
        }
        out
    }

    /// The extras block the converter writes (`extras.evani_layers`): only
    /// the layers in use.
    pub fn to_extras(&self) -> Value {
        let mut m = Map::new();
        if self.sheen_color.iter().any(|&c| c > 0.0) {
            m.insert(
                "sheen".into(),
                json!({"color": self.sheen_color, "roughness": self.sheen_roughness}),
            );
        }
        if self.clearcoat > 0.0 {
            m.insert(
                "clearcoat".into(),
                json!({"factor": self.clearcoat, "roughness": self.clearcoat_roughness}),
            );
        }
        if self.anisotropy > 0.0 {
            m.insert(
                "anisotropy".into(),
                json!({"strength": self.anisotropy, "rotation": self.anisotropy_rotation}),
            );
        }
        if self.iridescence > 0.0 {
            m.insert("iridescence".into(), json!({"factor": self.iridescence, "ior": self.iridescence_ior, "thickness": self.iridescence_thickness}));
        }
        Value::Object(m)
    }

    /// From a material's extras (model load): `evani_layers` and
    /// `evani_cloth`. `None` when neither is there.
    pub fn from_extras(extras: &Value) -> Option<Self> {
        let layers = extras.get(EXTRAS_KEY);
        let cloth = extras.get(CLOTH_KEY);
        if layers.is_none() && cloth.is_none() {
            return None;
        }
        let mut l = Self::default();
        if let Some(e) = layers {
            if let Some(s) = e.get("sheen") {
                l.sheen_color = clamp3(rgb(s.get("color")).unwrap_or([0.0; 3]));
                l.sheen_roughness = num(s.get("roughness")).unwrap_or(0.0).clamp(0.0, 1.0);
            }
            if let Some(c) = e.get("clearcoat") {
                l.clearcoat = num(c.get("factor")).unwrap_or(0.0).clamp(0.0, 1.0);
                l.clearcoat_roughness = num(c.get("roughness")).unwrap_or(0.0).clamp(0.0, 1.0);
            }
            if let Some(a) = e.get("anisotropy") {
                l.anisotropy = num(a.get("strength")).unwrap_or(0.0).clamp(0.0, 1.0);
                l.anisotropy_rotation = num(a.get("rotation")).unwrap_or(0.0);
            }
            if let Some(i) = e.get("iridescence") {
                l.iridescence = num(i.get("factor")).unwrap_or(0.0).clamp(0.0, 1.0);
                l.iridescence_ior = num(i.get("ior")).unwrap_or(1.3).clamp(1.0, 3.0);
                l.iridescence_thickness =
                    num(i.get("thickness")).unwrap_or(400.0).clamp(0.0, 2000.0);
            }
        }
        if let Some(c) = cloth {
            l.subsurface = clamp3(rgb(c.get("subsurface")).unwrap_or([0.0; 3]));
            l.weave = c
                .get("weave")
                .and_then(Value::as_str)
                .and_then(Weave::parse)
                .unwrap_or_default();
            l.weave_scale = num(c.get("scale")).unwrap_or(24.0).clamp(0.5, 512.0);
        }
        Some(l)
    }

    /// The flag bits of the layers this material really uses.
    pub fn flags(&self) -> u32 {
        let mut f = 0;
        if self.sheen_color.iter().any(|&c| c > 0.0) {
            f |= SHEEN;
        }
        if self.clearcoat > 0.0 {
            f |= CLEARCOAT;
        }
        if self.anisotropy > 0.0 {
            f |= ANISOTROPY;
        }
        if self.iridescence > 0.0 && self.iridescence_thickness > 0.0 {
            f |= IRIDESCENCE;
        }
        match self.weave {
            Weave::None => {}
            Weave::Plain => f |= WEAVE_PLAIN,
            Weave::Knit => f |= WEAVE_KNIT,
        }
        f
    }

    /// The material's layer record, as the shader's `LayerMat` reads it:
    /// sheen rgb + roughness; clear coat factor, roughness, anisotropy
    /// strength, rotation; iridescence factor, IOR, thickness, flags;
    /// subsurface rgb, weave scale.
    pub fn words(&self) -> [f32; LAYER_WORDS] {
        let mut w = [0.0; LAYER_WORDS];
        w[0..3].copy_from_slice(&self.sheen_color);
        w[3] = self.sheen_roughness;
        w[4] = self.clearcoat;
        w[5] = self.clearcoat_roughness;
        w[6] = self.anisotropy;
        w[7] = self.anisotropy_rotation;
        w[8] = self.iridescence;
        w[9] = self.iridescence_ior;
        w[10] = self.iridescence_thickness;
        w[FLAGS_WORD] = self.flags() as f32;
        w[12..15].copy_from_slice(&self.subsurface);
        w[15] = self.weave_scale;
        w
    }
}

/// The record of a material with no layers (every flag clear): the layered
/// shader draws it exactly like the plain lit pass.
pub fn no_layers() -> [f32; LAYER_WORDS] {
    let mut w = Layers::default().words();
    w[FLAGS_WORD] = 0.0;
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    fn khr() -> Value {
        json!({
            "KHR_materials_sheen": {"sheenColorFactor": [0.9, 0.2, 0.3], "sheenRoughnessFactor": 0.6, "sheenColorTexture": {"index": 2}},
            "KHR_materials_clearcoat": {"clearcoatFactor": 1.0, "clearcoatRoughnessFactor": 0.05},
            "KHR_materials_anisotropy": {"anisotropyStrength": 0.8, "anisotropyRotation": 1.5},
            "KHR_materials_iridescence": {"iridescenceFactor": 1.0, "iridescenceIor": 1.33, "iridescenceThicknessMinimum": 100, "iridescenceThicknessMaximum": 550},
            "KHR_texture_transform": {}
        })
    }

    #[test]
    fn reads_all_four_extensions() {
        let l = Layers::from_khr(&khr()).unwrap();
        assert_eq!(l.sheen_color, [0.9, 0.2, 0.3]);
        assert_eq!(
            (l.sheen_roughness, l.clearcoat, l.clearcoat_roughness),
            (0.6, 1.0, 0.05)
        );
        assert_eq!((l.anisotropy, l.anisotropy_rotation), (0.8, 1.5));
        assert_eq!(
            (l.iridescence, l.iridescence_ior, l.iridescence_thickness),
            (1.0, 1.33, 550.0),
            "no thickness map: the maximum"
        );
        assert_eq!(l.flags(), SHEEN | CLEARCOAT | ANISOTROPY | IRIDESCENCE);
    }

    #[test]
    fn spec_defaults_and_ranges() {
        // An empty extension object is the spec defaults: no layer.
        let l = Layers::from_khr(
            &json!({"KHR_materials_clearcoat": {}, "KHR_materials_iridescence": {}}),
        )
        .unwrap();
        assert_eq!(l.flags(), 0);
        assert_eq!((l.iridescence_ior, l.iridescence_thickness), (1.3, 400.0));
        // Out of range values are clamped, bad types ignored.
        let l = Layers::from_khr(&json!({"KHR_materials_sheen": {"sheenColorFactor": [2.0, -1.0, 0.5], "sheenRoughnessFactor": "x"}, "KHR_materials_clearcoat": {"clearcoatFactor": 7}})).unwrap();
        assert_eq!(l.sheen_color, [1.0, 0.0, 0.5]);
        assert_eq!((l.sheen_roughness, l.clearcoat), (0.0, 1.0));
        // No layer extension at all.
        assert!(Layers::from_khr(&json!({"KHR_texture_transform": {}})).is_none());
        assert!(Layers::from_khr(&Value::Null).is_none());
    }

    #[test]
    fn extras_round_trip() {
        let l = Layers::from_khr(&khr()).unwrap();
        let extras = json!({ EXTRAS_KEY: l.to_extras() });
        assert_eq!(Layers::from_extras(&extras), Some(l));
        // Only the layers in use are written.
        let only = Layers {
            clearcoat: 0.5,
            ..Layers::default()
        };
        assert_eq!(
            only.to_extras(),
            json!({"clearcoat": {"factor": 0.5, "roughness": 0.0}})
        );
        assert!(Layers::from_extras(&json!({"evani_surface": {"kind": "skin"}})).is_none());
    }

    #[test]
    fn cloth_extras() {
        let l = Layers::from_extras(
            &json!({"evani_cloth": {"subsurface": [0.3, 0.05, 0.1], "weave": "knit", "scale": 40}}),
        )
        .unwrap();
        assert_eq!(
            (l.subsurface, l.weave, l.weave_scale),
            ([0.3, 0.05, 0.1], Weave::Knit, 40.0)
        );
        assert_eq!(l.flags(), WEAVE_KNIT);
        let l = Layers::from_extras(&json!({"evani_cloth": {"weave": "tweed"}})).unwrap();
        assert_eq!(l.weave, Weave::None, "unknown weave names: none");
        assert_eq!(Weave::parse(Weave::Plain.name()), Some(Weave::Plain));
    }

    #[test]
    fn words_layout() {
        let mut l = Layers::from_khr(&khr()).unwrap();
        l.weave = Weave::Plain;
        l.subsurface = [0.1, 0.2, 0.3];
        let w = l.words();
        assert_eq!(&w[0..4], &[0.9, 0.2, 0.3, 0.6]);
        assert_eq!(&w[4..8], &[1.0, 0.05, 0.8, 1.5]);
        assert_eq!(&w[8..11], &[1.0, 1.33, 550.0]);
        assert_eq!(
            w[FLAGS_WORD] as u32,
            SHEEN | CLEARCOAT | ANISOTROPY | IRIDESCENCE | WEAVE_PLAIN
        );
        assert_eq!(&w[12..16], &[0.1, 0.2, 0.3, 24.0]);
        assert_eq!(no_layers()[FLAGS_WORD], 0.0);
    }

    #[test]
    fn reports_dropped_textures() {
        assert_eq!(
            Layers::dropped_textures(&khr()),
            vec!["KHR_materials_sheen.sheenColorTexture".to_string()]
        );
    }
}
