//! The toon look's micro-detail patterns (feature `relief`, S3-10): the
//! body of the renderer's `micro_detail` slot, which tips a model's mapped
//! normal by a small pattern recipe from its material's
//! `extras.evani.relief.micro` (docs/MODEL_FORMAT.md). Smooth height
//! profiles, so the look's banded ramp turns them into crisp, flat-shaded
//! shapes instead of noise. Every pattern's steepest slope is about 1 (in
//! height per period); the renderer scales it by the recipe's depth and
//! fades it out below 1.5 px. Only arithmetic: no texture reads and no
//! derivatives (the renderer calls it in non-uniform control flow).

use evani_core::plugin::{LookContext, PluginError};
use evani_core::slots::{MICRO_DETAIL, ShaderSlots};

/// Patterns by id (1 knurl, 2 ribs, 3 dots, 4 grain; anything else none):
/// * knurl: two crossed diagonal ridge sets, a diamond grip;
/// * ribs: parallel rounded ridges across u (lines running along v);
/// * dots: round domes on a square grid, 0.35 periods across, soft rims;
/// * grain: wavy lines along u with a finer second line set (wood, brushed
///   metal).
pub const MICRO_DETAIL_BODY: &str = "    // evani-look-toon: micro-detail patterns (1 knurl, 2 ribs, 3 dots, 4 grain), slopes up to about 1\n    let tau = 6.2831853;\n    let id = i32(round(pattern.x));\n    if (id == 1) {\n        let a = cos(tau * (uv.x + uv.y));\n        let b = cos(tau * (uv.x - uv.y));\n        return 0.5 * vec2<f32>(a + b, a - b);\n    }\n    if (id == 2) {\n        return vec2<f32>(cos(tau * uv.x), 0.0);\n    }\n    if (id == 3) {\n        let f = fract(uv) - vec2<f32>(0.5);\n        return -f / 0.35 * (1.0 - smoothstep(0.31, 0.35, length(f)));\n    }\n    if (id == 4) {\n        let w = uv.y + 0.12 * sin(3.1415927 * uv.x);\n        let s = cos(tau * w) + 0.8 * cos(tau * 2.3 * w + 0.7);\n        return 0.5 * s * vec2<f32>(0.377 * cos(3.1415927 * uv.x), 1.0);\n    }\n    return vec2<f32>(0.0);";

/// Declare the slot (the look may build before the renderer, which
/// declares it too; declaring is idempotent) and install the body.
pub fn install(look: &mut LookContext) -> Result<(), PluginError> {
    look.world.res_mut::<ShaderSlots>().declare(MICRO_DETAIL);
    look.shader_slot("micro_detail", MICRO_DETAIL_BODY)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code
    use super::*;
    use crate::ToonLook;
    use evani_core::App;

    /// The installed body is real WGSL with the slot's signature, owned by
    /// the look, and safe in non-uniform control flow.
    #[test]
    fn micro_detail_body_is_valid_wgsl() {
        let mut a = App::new("t").with_look(ToonLook);
        a.start();
        let slots = a.world.res::<ShaderSlots>();
        let id = slots.id("micro_detail").expect("the look declares micro_detail");
        assert_eq!(slots.get(id).unwrap().owner, "look.toon");
        let mut src = slots.wgsl_function(id).unwrap();
        assert!(src.contains("evani-look-toon"));
        src.push_str(
            "\n@fragment fn fs_main(@location(0) uv: vec2<f32>, @location(1) p: vec4<f32>) -> @location(0) vec4<f32> {\n    return vec4<f32>(slot_micro_detail(uv, p), 0.0, 1.0);\n}\n",
        );
        let module = naga::front::wgsl::parse_str(&src).unwrap_or_else(|e| panic!("{}", e.emit_to_string(&src)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
            .validate(&module)
            .unwrap_or_else(|e| panic!("{e:?}"));
        for banned in ["texture", "dpdx", "dpdy", "fwidth"] {
            assert!(!MICRO_DETAIL_BODY.contains(banned), "the slot runs in non-uniform control flow: no {banned}");
        }
    }
}
