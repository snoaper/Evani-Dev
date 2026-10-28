//! Kit surface (feature `kit-surface`, playground materials): the kit
//! material hook with a bent normal and a roughness, so procedural bricks,
//! cobbles and tiles catch the sun on their relief.
//!
//! * The kit code (set through [`crate::kit_material::KitMaterial`] as
//!   before) still defines `kit_material(p, n, material) -> vec4<f32>`: the
//!   shadow map and the ambient-occlusion depth use it for cut-outs, and a
//!   forward source the splice can't fully patch falls back to it.
//! * It may also define
//!   `fn kit_surface(p: vec3<f32>, n: vec3<f32>, material: u32, nr: ptr<function, vec4<f32>>) -> vec4<f32>`:
//!   it returns what `kit_material` would (colour multiplier, cut-out
//!   alpha) and writes `nr`: the unit normal to light with (xyz) and a
//!   roughness (w, 0 = mirror, 1 = matte). `nr` comes in as the geometric
//!   normal with roughness 1.
//! * With it, the forward mesh pass lights with that normal (sun, shadow
//!   lookup, toon ramp, point lights, ambient) and adds [`SPEC_WGSL`]'s
//!   `kit_spec`: a GGX sun highlight and a faint sky reflection, both zero
//!   from roughness 0.99 (fading from 0.85), so roughness 1 lights exactly
//!   as `kit_material` alone.
//! * Without `kit_surface` in the kit code nothing changes: the plain
//!   kit-material splice stays as it is.
//!
//! Spliced into the forward source after the kit-material splice (three
//! more lines of `fs_main` change, `kit_spec` is appended), so the WGSL
//! files are untouched and games without the feature are byte for byte
//! unchanged.

use crate::kit_material::HOOKED;

/// What the kit code must define for the surface splice.
pub const SURFACE_FN: &str = "fn kit_surface(";

/// The kit-material line, as the surface version: one call gives colour,
/// cut-out, normal and roughness.
pub const HOOKED_SURFACE: &str = "var kit_nr = vec4<f32>(normalize(in.normal), 1.0); let kit = kit_surface(in.world, normalize(in.normal), in.material, &kit_nr); if (kit.a < 0.5) { discard; } let base = in.color.rgb * m.base.rgb * kit.rgb;";

/// The lit branch's normal.
pub const NORMAL_HOOK: &str = "let n = normalize(in.normal);";
/// ... becomes the kit's.
pub const NORMAL_HOOKED: &str = "let n = normalize(kit_nr.xyz);";

/// The end of the lit colour (after the rim term).
pub const SPEC_HOOK: &str = "+ slot_rim(ndv, ndl, dist, base);";
/// ... gains the highlight.
pub const SPEC_HOOKED: &str = "+ slot_rim(ndv, ndl, dist, base) + kit_spec(n, in.world, kit_nr.w, vis);";

/// `kit_spec`, appended to the forward module.
pub const SPEC_WGSL: &str = include_str!("../shaders/kit_surface.wgsl");

/// Whether `kit_wgsl` asks for the surface splice.
pub fn wants_surface(kit_wgsl: &str) -> bool {
    kit_wgsl.contains(SURFACE_FN)
}

/// Patch a forward source that already has the kit-material splice. Does
/// nothing unless the kit code defines `kit_surface` and every hook is
/// there (all or nothing, so a partial match never leaves a stray name).
/// Returns whether it patched.
pub fn patch(source: &mut String, kit_wgsl: &str) -> bool {
    if !wants_surface(kit_wgsl) || ![HOOKED, NORMAL_HOOK, SPEC_HOOK].iter().all(|h| source.contains(h)) {
        return false;
    }
    let s = source
        .replacen(HOOKED, HOOKED_SURFACE, 1)
        .replacen(NORMAL_HOOK, NORMAL_HOOKED, 1)
        .replacen(SPEC_HOOK, SPEC_HOOKED, 1);
    *source = s;
    source.push('\n');
    source.push_str(SPEC_WGSL);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kit_material::{BINDING_WGSL, patch_forward, shadows_source};

    /// A kit with both functions: rough brick-red, a normal tilted towards +x.
    const KIT: &str = "fn kit_surface(p: vec3<f32>, n: vec3<f32>, material: u32, nr: ptr<function, vec4<f32>>) -> vec4<f32> { *nr = vec4<f32>(normalize(n + vec3<f32>(0.2, 0.0, 0.0)), 0.6); return vec4<f32>(0.6, 0.3, 0.2, 1.0); }\nfn kit_material(p: vec3<f32>, n: vec3<f32>, material: u32) -> vec4<f32> { var nr = vec4<f32>(n, 1.0); return kit_surface(p, n, material, &nr); }";

    /// The forward source with the builtin slot bodies spliced in.
    fn forward() -> String {
        let mut slots = evani_core::slots::ShaderSlots::with_builtins();
        slots.declare(evani_core::slots::SlotSignature {
            name: "pcss_params",
            params: &[("i", "u32")],
            ret: "vec4<f32>",
            description: "Soft shadows (test stand-in)",
            default_body: "    return vec4<f32>(0.02, 6.0, 1.0, 12.0);",
        });
        // The look slots the lit and relief forward variants add (the same
        // declarations as the engine's own forward slots, shaders.rs).
        #[cfg(feature = "relief")]
        slots.declare(evani_core::slots::MICRO_DETAIL);
        #[cfg(all(feature = "look-surface", not(feature = "relief")))]
        {
            slots.declare(crate::model::surface::SPECULAR);
            slots.declare(crate::model::surface::SURFACE_PARAMS);
        }
        let mut fwd = String::new();
        for line in crate::shaders::FORWARD.source.lines() {
            match line.trim().strip_prefix("//@slot ") {
                Some(name) => match slots.id(name.trim()).and_then(|id| slots.wgsl_function(id)) {
                    Some(f) => fwd.push_str(&f),
                    None => panic!("no slot {name}"),
                },
                None => fwd.push_str(line),
            }
            fwd.push('\n');
        }
        fwd
    }

    fn validate(src: &str) {
        let module = naga::front::wgsl::parse_str(src).unwrap_or_else(|e| panic!("{}", e.emit_to_string(src)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
            .validate(&module)
            .unwrap_or_else(|e| panic!("{e:?}"));
    }

    #[test]
    fn every_hook_is_in_the_forward_source_once() {
        let fwd = patch_forward(&forward(), KIT).unwrap();
        for h in [HOOKED, NORMAL_HOOK, SPEC_HOOK] {
            assert_eq!(fwd.matches(h).count(), 1, "{h}");
        }
    }

    #[test]
    fn the_surface_splice_validates_and_shadows_still_build() {
        let mut fwd = patch_forward(&forward(), KIT).unwrap();
        assert!(patch(&mut fwd, KIT));
        for h in [HOOKED_SURFACE, NORMAL_HOOKED, SPEC_HOOKED, "fn kit_spec("] {
            assert!(fwd.contains(h), "{h}");
        }
        validate(&fwd);
        let shadows = shadows_source(KIT);
        assert!(!shadows.contains("kit_spec"));
        validate(&shadows);
        assert!(BINDING_WGSL.contains("kit_data"));
    }

    #[test]
    fn kits_without_kit_surface_are_untouched() {
        let plain = "fn kit_material(p: vec3<f32>, n: vec3<f32>, material: u32) -> vec4<f32> { return vec4<f32>(1.0); }";
        let fwd = patch_forward(&forward(), plain).unwrap();
        let mut patched = fwd.clone();
        assert!(!patch(&mut patched, plain));
        assert_eq!(patched, fwd);
        // A source without the hooks (models, sprites) is left alone too.
        let mut other = String::from("fn fs_main() {}");
        assert!(!patch(&mut other, KIT));
        assert_eq!(other, "fn fs_main() {}");
    }

    /// The highlight stays inside the point-light and ambient splices'
    /// hooks: they still find their lines after this one.
    #[test]
    fn the_other_forward_splices_still_find_their_lines() {
        let mut fwd = patch_forward(&forward(), KIT).unwrap();
        assert!(patch(&mut fwd, KIT));
        for h in ["frame.ambient.rgb + frame.light_color.rgb * diffuse", "frame.ambient.rgb + ", "frame.light_color.rgb * diffuse)"] {
            assert!(fwd.contains(h), "{h}");
        }
    }
}
