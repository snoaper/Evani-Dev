//! S3-9: the skinned vertex stages validate after the static ones, with
//! either shadow tail and the engine's slot bodies spliced in; the skinned
//! shadow stage validates in the model shadow module; and the build's own
//! shipped sources (comments stripped) carry both skinned entry points.

#![cfg(feature = "skinning")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code: a failed read is a failed test

use evani_core::slots::ShaderSlots;
use evani_render::splice::{Bodies, splice};

const FORWARD: &str = include_str!("../shaders/forward.wgsl");
const MODEL: &str = include_str!("../shaders/model.wgsl");
const SKIN: &str = include_str!("../shaders/model_skin.wgsl");
const SHADOWS: &str = include_str!("../shaders/forward_shadows.wgsl");
const NOSHADOW: &str = include_str!("../shaders/forward_noshadow.wgsl");

fn validate(name: &str, src: &str) -> naga::Module {
    let module = naga::front::wgsl::parse_str(src).unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(src)));
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
        .validate(&module)
        .unwrap_or_else(|e| panic!("{name}: {e:?}"));
    module
}

fn entry(m: &naga::Module, name: &str) -> bool {
    m.entry_points.iter().any(|e| e.name == name)
}

/// The joints buffer: one read-only storage array of `mat3x4<f32>` at
/// `group`, binding 0 (what `gpu::models::skin` binds).
fn joints_at(m: &naga::Module, group: u32) -> bool {
    m.global_variables.iter().any(|(_, v)| {
        matches!(v.space, naga::AddressSpace::Storage { access } if access == naga::StorageAccess::LOAD)
            && v.binding.as_ref().is_some_and(|b| b.group == group && b.binding == 0)
            && matches!(m.types[v.ty].inner, naga::TypeInner::Array { base, .. } if matches!(m.types[base].inner, naga::TypeInner::Matrix { columns: naga::VectorSize::Tri, rows: naga::VectorSize::Quad, .. }))
    })
}

#[test]
fn skinning_wgsl_validates() {
    let mut slots = ShaderSlots::with_builtins();
    // A lit surface build (S5-R5b) also calls specular and surface_params.
    #[cfg(all(feature = "look-surface", not(feature = "relief")))]
    {
        slots.declare(evani_render::model::surface::SPECULAR);
        slots.declare(evani_render::model::surface::SURFACE_PARAMS);
    }
    for tail in [NOSHADOW, SHADOWS] {
        let src = [FORWARD, MODEL, SKIN, tail].concat();
        for bodies in [Bodies::Current, Bodies::EngineDefaults] {
            let s = splice(&src, &slots, bodies).unwrap();
            let m = validate("forward + skin", &s.source);
            assert!(entry(&m, "model_vs") && entry(&m, "model_skin_vs") && entry(&m, "model_fs"));
            assert!(joints_at(&m, 2), "forward joints at group 2");
        }
    }
    // With surface relief (S3-10) too: the skinned stage feeds the relief
    // fragment stage (`model_relief_fs`, which also calls micro_detail).
    slots.declare(evani_core::slots::MICRO_DETAIL);
    for tail in [NOSHADOW, SHADOWS] {
        let src = [FORWARD, MODEL, include_str!("../shaders/model_relief.wgsl"), SKIN, tail].concat();
        for bodies in [Bodies::Current, Bodies::EngineDefaults] {
            let s = splice(&src, &slots, bodies).unwrap();
            let m = validate("forward + relief + skin", &s.source);
            assert!(entry(&m, "model_skin_vs") && entry(&m, "model_relief_fs"));
        }
    }
    assert_eq!(evani_render::shaders::FORWARD.source.contains("evani.shader.model_relief"), cfg!(feature = "relief"));
    let shadows = [include_str!("../shaders/shadows_depth.wgsl"), include_str!("../shaders/model_shadows.wgsl"), include_str!("../shaders/model_skin_shadows.wgsl")].concat();
    let m = validate("model_shadows + skin", &shadows);
    assert!(entry(&m, "model_shadow_vs") && entry(&m, "model_skin_shadow_vs"));
    assert!(joints_at(&m, 1), "shadow joints at group 1");

    // What the build ships: comments stripped, still splices and validates,
    // with both skinned stages.
    let fwd = evani_render::shaders::FORWARD.source;
    assert!(fwd.contains("evani.shader.model_skin"));
    let s = splice(fwd, &slots, Bodies::Current).unwrap();
    let m = validate("shipped forward", &(s.source + &lit_runtime_stubs()));
    assert!(entry(&m, "model_vs") && entry(&m, "model_skin_vs"));
    #[cfg(feature = "shadows")]
    {
        let m = validate("shipped model_shadows", evani_render::shaders::MODEL_SHADOWS);
        assert!(entry(&m, "model_shadow_vs") && entry(&m, "model_skin_shadow_vs"));
    }
}

/// Every `err!("K<n>", "<text>")` in the skin reader has its row in
/// docs/MODEL_FORMAT.md's "Skins" table with the same text, each code means
/// one thing, and the table lists no code the reader no longer uses (a code
/// in a shipped download is only the short code, so the doc is where it is
/// looked up).
#[test]
fn skin_error_codes_are_documented() {
    let src = include_str!("../src/model/skin.rs");
    let mut used = std::collections::BTreeMap::new();
    for chunk in src.split("err!(\"").skip(1) {
        let (code, rest) = chunk.split_once('"').unwrap();
        if code.starts_with('$') {
            continue; // the macro's own definition
        }
        let text = rest.split_once('"').unwrap().1.split_once('"').unwrap().0;
        assert!(code.starts_with('K') && code[1..].parse::<u32>().is_ok(), "skin error code {code:?} must be K and a number");
        assert!(text.starts_with("model: "), "{code}: {text:?} should start with \"model: \"");
        if let Some(old) = used.insert(code.to_string(), text.to_string()) {
            assert_eq!(old, text, "{code} means two things");
        }
    }
    assert!(used.len() >= 5, "found only {} K codes; the scan is broken", used.len());
    let doc = include_str!("../../../docs/MODEL_FORMAT.md");
    let section = doc.split("\n## Skins").nth(1).expect("docs/MODEL_FORMAT.md needs a \"## Skins\" section");
    let section = section.split("\n## ").next().unwrap_or(section);
    let mut documented = std::collections::BTreeMap::new();
    for line in section.lines() {
        let cells: Vec<&str> = line.trim().trim_matches('|').split('|').map(str::trim).collect();
        if cells.len() == 2 && cells[0].starts_with("`K") && cells[0].ends_with('`') {
            documented.insert(cells[0].trim_matches('`').to_string(), cells[1].to_string());
        }
    }
    assert_eq!(documented, used, "MODEL_FORMAT.md's Skins table and the skin reader's codes differ");
}

/// A lit surface build's glue (S5-R5b) calls what the runtime point-light
/// and ambient patches add; stubs for them, so the shipped source validates
/// without the GPU patches. Empty in every other build.
fn lit_runtime_stubs() -> String {
    let mut s = String::new();
    if !cfg!(all(feature = "look-surface", not(feature = "relief"))) {
        return s;
    }
    if cfg!(feature = "point-lights") {
        s.push_str("\nfn point_lights(p: vec3<f32>, n: vec3<f32>, frag: vec4<f32>) -> vec3<f32> {\n    return vec3<f32>(0.0);\n}\n");
    }
    if cfg!(feature = "ambient") {
        s.push_str("\nfn amb_occlusion(frag: vec4<f32>) -> f32 {\n    return 1.0;\n}\n");
    }
    if cfg!(feature = "ibl") {
        s.push_str("\nstruct LitStubAmb {\n    sun_color: vec4<f32>,\n};\n@group(0) @binding(15) var<uniform> amb: LitStubAmb;\nfn amb_sh(n: vec3<f32>) -> vec3<f32> {\n    return n;\n}\nfn amb_env(r: vec3<f32>, rough: f32) -> vec3<f32> {\n    return r * rough;\n}\n");
    }
    s
}
