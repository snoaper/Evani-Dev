//! The lit surface pass on textured models (feature `look-surface`,
//! S5-R5b): a material's glTF metallic-roughness and occlusion maps and
//! factors, and its film kind (`extras.evani_surface.kind`: `plain`, `skin`
//! or `eye`), read into the words the lit shader (`shaders/model_lit.wgsl`)
//! reads. Also the two slots that shader calls, declared by the engine with
//! zero bodies so a look that never sets them shades as plain PBR.
//!
//! Turned off by `relief` (it uses the same material bindings 5 to 7):
//! with both on, the relief stage draws (DECISIONS "Lit surface pass").
//! Adapted from ChimpDuty's look-surface patch (ChimpDuty, Chris Pearson).

use evani_core::slots::SlotSignature;
use evani_gltf::{Gltf, Material};
use serde_json::Value;

use super::TexNeed;

/// Floats per material surface uniform (4 vec4): metal factor, roughness
/// factor, occlusion strength, unused; 8 unused; unused, unused, kind, unused.
pub const SURFACE_WORDS: usize = 16;
/// The word holding the material's [`Kind`] code.
pub const KIND_WORD: usize = 14;

/// Specular light from the sun for one pixel (multiplied by the light
/// colour and shadow by the caller). Same signature as the stylizedpbr
/// look's; zero by default.
pub const SPECULAR: SlotSignature = SlotSignature {
    name: "specular",
    params: &[("n_dot_h", "f32"), ("n_dot_l", "f32"), ("n_dot_v", "f32"), ("roughness", "f32"), ("metal", "f32"), ("base", "vec3<f32>")],
    ret: "vec3<f32>",
    description: "Specular reflection of the sun from N.H, N.L, N.V, roughness, metalness and the base colour (zero = none)",
    default_body: "    return vec3<f32>(0.0);",
};

/// Surface constants by index (0 occlusion and reflections, 1 detail, 2 sky
/// tint, 3 ground tint, 4 to 7 the film rig, 10 skin, 11 eyes). Same
/// signature as the stylizedpbr look's; all zero by default (the shader
/// reads zero sky and ground tints as white).
pub const SURFACE_PARAMS: SlotSignature = SlotSignature {
    name: "surface_params",
    params: &[("i", "u32")],
    ret: "vec4<f32>",
    description: "Model surface constants by index: 0 occlusion/reflections, 1 detail, 2 sky tint, 3 ground tint",
    default_body: "    return vec4<f32>(0.0);",
};

/// A material's film kind: how the lit pass lights it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    /// Plain PBR.
    #[default]
    Plain,
    /// Wrapped skin with a warm terminator (wet where very smooth).
    Skin,
    /// Wet all over, with a catchlight.
    Eye,
}

impl Kind {
    /// `plain`, `skin` or `eye`.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "plain" => Some(Self::Plain),
            "skin" => Some(Self::Skin),
            "eye" => Some(Self::Eye),
            _ => None,
        }
    }

    /// The code the shader reads (`surface.kind.z`).
    pub fn code(self) -> f32 {
        match self {
            Self::Plain => 0.0,
            Self::Skin => 1.0,
            Self::Eye => 2.0,
        }
    }
}

/// A material's surface as the model pipeline reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct Surface {
    pub words: [f32; SURFACE_WORDS],
    /// The metallic-roughness map (linear): an index into
    /// [`super::Prepared::textures`].
    pub mr: Option<usize>,
    /// The occlusion map (linear, red channel).
    pub occlusion: Option<usize>,
}

impl Default for Surface {
    /// No material: a rough dielectric, no occlusion map, plain.
    fn default() -> Self {
        let mut words = [0.0; SURFACE_WORDS];
        words[1] = 1.0;
        Self { words, mr: None, occlusion: None }
    }
}

/// The kind named by `extras.evani_surface.kind` (missing or unknown:
/// plain; `cargo xtask check` is where a bad name gets reported).
pub fn kind(extras: &Value) -> Kind {
    extras.get("evani_surface").and_then(|e| e.get("kind")).and_then(Value::as_str).and_then(Kind::parse).unwrap_or_default()
}

/// The surface of material `m`, listing its metallic-roughness and
/// occlusion maps (missing maps read as white on the GPU).
pub fn material(g: &Gltf, m: Option<&Material>, textures: &mut Vec<TexNeed>) -> Surface {
    let mut s = Surface::default();
    let Some(m) = m else {
        return s;
    };
    s.words[0] = m.metallic_factor.clamp(0.0, 1.0);
    s.words[1] = m.roughness_factor.clamp(0.0, 1.0);
    s.mr = m.metallic_roughness_texture.as_ref().and_then(|r| super::need(g, r.texture, false, textures));
    if let Some(r) = &m.occlusion_texture {
        s.occlusion = super::need(g, r.texture, false, textures);
        s.words[2] = if s.occlusion.is_some() { r.amount.clamp(0.0, 1.0) } else { 0.0 };
    }
    s.words[KIND_WORD] = kind(&m.extras).code(); #[cfg(feature = "char-materials")] evani_material_character::record::apply(&m.extras, &mut s.words);
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn kinds_read_from_material_extras() {
        assert_eq!(kind(&json!({"evani_surface": {"kind": "skin"}})), Kind::Skin);
        assert_eq!(kind(&json!({"evani_surface": {"kind": "eye"}})), Kind::Eye);
        assert_eq!(kind(&json!({"evani_surface": {"kind": "plain"}})), Kind::Plain);
        assert_eq!(kind(&json!({"evani_surface": {"kind": "fur"}})), Kind::Plain, "unknown names stay plain");
        assert_eq!(kind(&Value::Null), Kind::Plain);
        assert_eq!([Kind::Plain.code(), Kind::Skin.code(), Kind::Eye.code()], [0.0, 1.0, 2.0]);
    }

    #[test]
    fn no_material_is_a_rough_plain_dielectric() {
        let s = Surface::default();
        assert_eq!((s.words[0], s.words[1], s.words[2], s.words[KIND_WORD]), (0.0, 1.0, 0.0, 0.0));
        assert!(s.mr.is_none() && s.occlusion.is_none());
    }

    #[test]
    fn slot_signatures_match_the_look() {
        // The stylizedpbr look declares the same two slots; whichever is
        // declared first wins, so the signatures must agree word for word.
        assert_eq!(SPECULAR.params.len(), 6);
        assert_eq!((SPECULAR.name, SPECULAR.ret), ("specular", "vec3<f32>"));
        assert_eq!((SURFACE_PARAMS.name, SURFACE_PARAMS.ret, SURFACE_PARAMS.params), ("surface_params", "vec4<f32>", &[("i", "u32")][..]));
    }
}
