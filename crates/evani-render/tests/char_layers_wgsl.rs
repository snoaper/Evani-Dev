//! Character and layered materials together (features `char-materials` and
//! `material-layers`, without `relief`; DECISIONS.md "Join 6"): the model
//! pipelines draw `model_layered_fs`, so the character patch goes into that
//! stage. It validates with both shadow tails, with and without the skinned
//! stage, at 1x and 4x MSAA, with the zero and the real `char_params` body;
//! the layered stage writes the sample mask, a character kind clears the
//! layer flags, and the shipped forward source patches the same way.

#![cfg(all(feature = "char-materials", feature = "material-layers", not(feature = "relief")))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code: a failed read is a failed test

use evani_core::slots::ShaderSlots;
use evani_material_character::lut;
use evani_material_character::params::{Values, body};
use evani_material_character::shader::{CHAR_PARAMS, patch_layered};
use evani_material_layers::{BRDF_WGSL, LAYER_WORDS};
use evani_render::model::surface::{SPECULAR, SURFACE_PARAMS, SURFACE_WORDS};
use evani_render::splice::{Bodies, splice};

const FORWARD: &str = include_str!("../shaders/forward.wgsl");
const MODEL: &str = include_str!("../shaders/model.wgsl");
const SKIN: &str = include_str!("../shaders/model_skin.wgsl");
const LIT: &str = include_str!("../shaders/model_lit.wgsl");
const LAYERS: &str = include_str!("../shaders/model_lit_layers.wgsl");
const SHADOWS: &str = include_str!("../shaders/forward_shadows.wgsl");
const NOSHADOW: &str = include_str!("../shaders/forward_noshadow.wgsl");

const GLUE: &str = "
fn lit_points(p: vec3<f32>, n: vec3<f32>, frag: vec4<f32>) -> vec3<f32> {
    return vec3<f32>(0.0);
}
fn lit_ao(frag: vec4<f32>) -> f32 {
    return 1.0;
}
fn lit_sky(n: vec3<f32>) -> vec3<f32> {
    return frame.ambient.rgb;
}
fn lit_env(r: vec3<f32>, rough: f32) -> vec3<f32> {
    return lit_hemi(r);
}
";

fn validate(name: &str, src: &str) -> naga::Module {
    let module = naga::front::wgsl::parse_str(src).unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(src)));
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
        .validate(&module)
        .unwrap_or_else(|e| panic!("{name}: {e:?}"));
    module
}

fn slots(quality: Option<u32>) -> ShaderSlots {
    let mut s = ShaderSlots::with_builtins();
    s.declare(SPECULAR);
    s.declare(SURFACE_PARAMS);
    s.declare(CHAR_PARAMS);
    if let Some(q) = quality {
        let v = Values { quality: q as f32, ..Values::default() };
        s.set_default("char_params", &body(&v, q, 4, &lut::fitted()), "test").unwrap();
    }
    s
}

fn uniform_size(m: &naga::Module, name: &str) -> Option<u32> {
    m.global_variables
        .iter()
        .find(|(_, v)| v.space == naga::AddressSpace::Uniform && v.name.as_deref() == Some(name))
        .map(|(_, v)| m.types[v.ty].inner.size(m.to_ctx()))
}

/// The entry point `name` writes a colour and a sample mask.
fn writes_mask(m: &naga::Module, name: &str) -> bool {
    let Some(ep) = m.entry_points.iter().find(|e| e.name == name) else {
        return false;
    };
    let Some(r) = &ep.function.result else {
        return false;
    };
    match &m.types[r.ty].inner {
        naga::TypeInner::Struct { members, .. } => members.iter().any(|mb| matches!(mb.binding, Some(naga::Binding::BuiltIn(naga::BuiltIn::SampleMask)))),
        _ => false,
    }
}

#[test]
fn the_layered_stage_takes_the_character_patch() {
    for (tail_name, tail) in [("noshadow", NOSHADOW), ("shadows", SHADOWS)] {
        for skin in [false, true] {
            let mut parts = vec![FORWARD, MODEL];
            if skin {
                parts.push(SKIN);
            }
            parts.extend([tail, LIT, BRDF_WGSL, LAYERS, GLUE]);
            let src = parts.concat();
            for msaa in [1, 4] {
                let patched = patch_layered(&src, msaa).expect("every patched line is in the layered stage");
                for q in [None, Some(0), Some(1), Some(2), Some(3)] {
                    let name = format!("char+layers {tail_name} skin={skin} msaa={msaa} q={q:?}");
                    let s = splice(&patched, &slots(q), Bodies::Current).unwrap_or_else(|e| panic!("{name}: {e:?}"));
                    let m = validate(&name, &s.source);
                    assert!(writes_mask(&m, "model_layered_fs"), "{name}: the layered stage writes the mask");
                    assert!(!writes_mask(&m, "model_lit_fs"), "{name}: the unused plain stage is untouched");
                    assert_eq!(uniform_size(&m, "surface"), Some(SURFACE_WORDS as u32 * 4), "{name}");
                    assert_eq!(uniform_size(&m, "ly_mat"), Some(LAYER_WORDS as u32 * 4), "{name}");
                    assert!(s.source.contains("let flags = select(u32(ly_mat.iri.w + 0.5), 0u, surface.kind.z > 0.5);"), "{name}: character kinds drop their layers");
                }
            }
        }
    }
}

/// What the engine compiles: the shipped forward module through the same
/// patch the model pipelines use (gpu/char_material.rs).
#[test]
fn shipped_forward_patches_the_layered_stage() {
    let fwd = evani_render::shaders::FORWARD.source;
    let mut stubs = String::new();
    if cfg!(feature = "point-lights") {
        stubs.push_str("\nfn point_lights(p: vec3<f32>, n: vec3<f32>, frag: vec4<f32>) -> vec3<f32> {\n    return vec3<f32>(0.0);\n}\n");
    }
    if cfg!(feature = "ambient") {
        stubs.push_str("\nfn amb_occlusion(frag: vec4<f32>) -> f32 {\n    return 1.0;\n}\n");
    }
    if cfg!(feature = "ibl") {
        stubs.push_str("\nstruct LitStubAmb {\n    sun_color: vec4<f32>,\n};\n@group(0) @binding(15) var<uniform> amb: LitStubAmb;\nfn amb_sh(n: vec3<f32>) -> vec3<f32> {\n    return n;\n}\nfn amb_env(r: vec3<f32>, rough: f32) -> vec3<f32> {\n    return r * rough;\n}\n");
    }
    for msaa in [1, 4] {
        let p = patch_layered(fwd, msaa).expect("the shipped layered stage patches");
        let s = splice(&p, &slots(Some(2)), Bodies::Current).unwrap();
        let m = validate(&format!("shipped msaa={msaa}"), &(s.source + &stubs));
        assert!(writes_mask(&m, "model_layered_fs"));
    }
}
