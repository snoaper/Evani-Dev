//! Layered materials on textured models (feature `material-layers`; the
//! crate `evani-material-layers` holds the parsing, the weave map, the live
//! strengths and the layer BRDF): a material's `extras.evani_layers` (what
//! `cargo xtask convert` writes from the glTF `KHR_materials_sheen`,
//! `KHR_materials_clearcoat`, `KHR_materials_anisotropy` and
//! `KHR_materials_iridescence` extensions) and `extras.evani_cloth`, read
//! into the 16 floats of its layer record. A material without either reads
//! as no layers (every flag clear), and the layered shader then draws it
//! exactly like the plain lit pass.

use evani_gltf::Material;
use evani_material_layers::{LAYER_WORDS, Layers, parse::no_layers};

/// The layer record of material `m` (`None`, or no layer extras: no layers).
pub fn material(m: Option<&Material>) -> [f32; LAYER_WORDS] {
    m.and_then(|m| Layers::from_extras(&m.extras)).map_or_else(no_layers, |l| l.words())
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_material_layers::{CLEARCOAT, FLAGS_WORD, SHEEN, WEAVE_KNIT};
    use serde_json::json;

    fn mat(extras: serde_json::Value) -> Material {
        Material {
            name: String::new(),
            base_color_factor: [1.0; 4],
            base_color_texture: None,
            metallic_factor: 0.0,
            roughness_factor: 1.0,
            metallic_roughness_texture: None,
            normal_texture: None,
            occlusion_texture: None,
            emissive_texture: None,
            emissive_factor: [0.0; 3],
            alpha_mode: evani_gltf::AlphaMode::Opaque,
            alpha_cutoff: 0.5,
            double_sided: false,
            extras,
        }
    }

    #[test]
    fn plain_materials_carry_no_layer() {
        assert_eq!(material(None), no_layers());
        assert_eq!(material(Some(&mat(json!({})))), no_layers());
        assert_eq!(material(Some(&mat(json!({"evani_surface": {"kind": "skin"}}))))[FLAGS_WORD], 0.0);
    }

    #[test]
    fn layer_extras_reach_the_record() {
        let m = mat(json!({
            "evani_layers": {"sheen": {"color": [0.8, 0.2, 0.3], "roughness": 0.6}, "clearcoat": {"factor": 1.0, "roughness": 0.05}},
            "evani_cloth": {"weave": "knit", "scale": 40.0, "subsurface": [0.3, 0.05, 0.1]}
        }));
        let w = material(Some(&m));
        assert_eq!(&w[0..4], &[0.8, 0.2, 0.3, 0.6]);
        assert_eq!((w[4], w[5]), (1.0, 0.05));
        assert_eq!(w[FLAGS_WORD] as u32, SHEEN | CLEARCOAT | WEAVE_KNIT);
        assert_eq!(&w[12..16], &[0.3, 0.05, 0.1, 40.0]);
    }
}
