//! S5-R5b: the lit surface pass (feature `look-surface` without `relief`).
//! The assembled forward module with model_lit.wgsl and its lit_* glue
//! validates with both shadow tails, with and without the skinned stage,
//! with the engine's zero slot bodies and with a film-style `surface_params`
//! body (cases 0 to 11, as the stylizedpbr film preset writes it) and a real
//! specular lobe. The shipped forward source validates too, with stubs for
//! what the runtime point-light and ambient patches add.

#![cfg(all(feature = "look-surface", not(feature = "relief")))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code: a failed read is a failed test

use evani_core::slots::ShaderSlots;
use evani_render::model::surface::{SPECULAR, SURFACE_PARAMS, SURFACE_WORDS};
use evani_render::splice::{Bodies, splice};

const FORWARD: &str = include_str!("../shaders/forward.wgsl");
const MODEL: &str = include_str!("../shaders/model.wgsl");
const SKIN: &str = include_str!("../shaders/model_skin.wgsl");
const LIT: &str = include_str!("../shaders/model_lit.wgsl");
const SHADOWS: &str = include_str!("../shaders/forward_shadows.wgsl");
const NOSHADOW: &str = include_str!("../shaders/forward_noshadow.wgsl");

/// The lit_* glue of a build without point lights or ambient (the same
/// text src/shaders.rs appends then).
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

/// A film-style `surface_params` body: every case the lit pass reads (0 to
/// 7, 10 and 11) plus the two it leaves free (8, 9), with the hemisphere's
/// w = 1 (final light) form.
const FILM_SURFACE_PARAMS: &str = "    switch i {
        case 0u: { return vec4<f32>(0.85, 0.35, 2.2, 0.0); }
        case 1u: { return vec4<f32>(0.0, 1.0, 0.0, 0.0); }
        case 2u: { return vec4<f32>(0.2625, 0.287, 0.35, 1.0); }
        case 3u: { return vec4<f32>(0.15, 0.1338, 0.1176, 1.0); }
        case 4u: { return vec4<f32>(0.6275, 0.5265, 0.5736, 0.6); }
        case 5u: { return vec4<f32>(1.0, 0.96, 0.88, 0.2); }
        case 6u: { return vec4<f32>(-0.4924, 0.8529, 0.1736, 0.25); }
        case 7u: { return vec4<f32>(0.75, 0.82, 1.0, 0.5); }
        case 8u: { return vec4<f32>(0.0); }
        case 9u: { return vec4<f32>(0.0); }
        case 10u: { return vec4<f32>(0.45, 0.8, 0.35, 0.0); }
        case 11u: { return vec4<f32>(0.06, 2.5, 0.006, 0.0); }
        default: { return vec4<f32>(0.0); }
    }";

/// A real roughness/metal lobe (the stylizedpbr look's shape).
const SPECULAR_BODY: &str = "    let a = max(roughness * roughness, 0.003);
    let a2 = a * a;
    let d = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
    let ndf = a2 / (3.14159265 * d * d);
    let k = a * 0.5;
    let vis = 0.25 / max((n_dot_l * (1.0 - k) + k) * (n_dot_v * (1.0 - k) + k), 1e-4);
    let f0 = mix(vec3<f32>(0.04), base, metal);
    let e = 1.0 - clamp(n_dot_v, 0.0, 1.0);
    let fr = f0 + (vec3<f32>(1.0) - f0) * (e * e * e * e * e);
    return fr * min(ndf * vis, 64.0) * n_dot_l;";

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

/// Size in bytes of the uniform named `name`.
fn uniform_size(m: &naga::Module, name: &str) -> Option<u32> {
    m.global_variables
        .iter()
        .find(|(_, v)| v.space == naga::AddressSpace::Uniform && v.name.as_deref() == Some(name))
        .map(|(_, v)| m.types[v.ty].inner.size(m.to_ctx()))
}

fn slots(film: bool) -> ShaderSlots {
    let mut s = ShaderSlots::with_builtins();
    s.declare(SPECULAR);
    s.declare(SURFACE_PARAMS);
    if film {
        s.set_default("surface_params", FILM_SURFACE_PARAMS, "test").unwrap();
        s.set_default("specular", SPECULAR_BODY, "test").unwrap();
    }
    s
}

#[test]
fn lit_wgsl_validates_with_both_tails_skinning_and_film_bodies() {
    for (tail_name, tail) in [("noshadow", NOSHADOW), ("shadows", SHADOWS)] {
        for skin in [false, true] {
            let parts: Vec<&str> = if skin { vec![FORWARD, MODEL, SKIN, tail, LIT, GLUE] } else { vec![FORWARD, MODEL, tail, LIT, GLUE] };
            let src = parts.concat();
            for film in [false, true] {
                let sl = slots(film);
                for bodies in [Bodies::Current, Bodies::EngineDefaults] {
                    let name = format!("lit {tail_name} skin={skin} film={film} {bodies:?}");
                    let s = splice(&src, &sl, bodies).unwrap_or_else(|e| panic!("{name}: {e:?}"));
                    let m = validate(&name, &s.source);
                    assert!(entry(&m, "model_vs") && entry(&m, "model_fs") && entry(&m, "model_lit_fs"), "{name}");
                    assert_eq!(entry(&m, "model_skin_vs"), skin, "{name}");
                    assert_eq!(uniform_size(&m, "surface"), Some(SURFACE_WORDS as u32 * 4), "{name}: the surface uniform is SURFACE_WORDS floats");
                    assert_eq!(uniform_size(&m, "model_mat"), Some(80), "{name}");
                }
            }
        }
    }
}

#[test]
fn film_body_reaches_the_spliced_source() {
    let src = [FORWARD, MODEL, SHADOWS, LIT, GLUE].concat();
    let s = splice(&src, &slots(true), Bodies::Current).unwrap();
    for case in 0..=11 {
        assert!(s.source.contains(&format!("case {case}u:")), "case {case} spliced");
    }
    // The engine defaults stay zero (a look that sets nothing shades plain).
    let d = splice(&src, &slots(true), Bodies::EngineDefaults).unwrap();
    assert!(!d.source.contains("case 11u:"));
}

/// What the runtime patches add that the shipped glue calls, as stubs.
fn runtime_stubs() -> String {
    let mut s = String::new();
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

#[test]
fn shipped_forward_has_the_lit_stage() {
    let fwd = evani_render::shaders::FORWARD.source;
    assert!(fwd.contains("fn model_lit_fs("), "a look-surface build ships the lit fragment stage");
    // The glue never carries the runtime patches' hook text, so the patches
    // change the other stages exactly as before.
    let glue = &fwd[fwd.find("fn lit_points(").unwrap()..];
    assert!(!glue.contains("frame.ambient.rgb + ") && !glue.contains("frame.light_color.rgb * diffuse)"));
    let lit = &fwd[fwd.find("fn lit_hemi(").unwrap()..];
    assert!(!lit.contains("frame.ambient.rgb + ") && !lit.contains("frame.light_color.rgb * diffuse)"));
    for film in [false, true] {
        let s = splice(fwd, &slots(film), Bodies::Current).unwrap();
        let m = validate(&format!("shipped forward film={film}"), &(s.source + &runtime_stubs()));
        assert!(entry(&m, "model_lit_fs"));
        assert_eq!(entry(&m, "model_skin_vs"), cfg!(feature = "skinning"));
    }
}
