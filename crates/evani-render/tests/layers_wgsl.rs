//! Layered materials (feature `material-layers` without `relief`): the
//! forward module with model_lit.wgsl, the layer BRDF and
//! model_lit_layers.wgsl validates (naga) with both shadow tails, with and
//! without the skinned stage, with the engine's zero slot bodies and with a
//! real specular lobe; its layer record and frame uniforms are the sizes the
//! CPU writes; the shipped forward source carries the layered stage.

#![cfg(all(feature = "material-layers", not(feature = "relief")))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code: a failed read is a failed test

use evani_core::slots::ShaderSlots;
use evani_material_layers::{BRDF_WGSL, FRAME_WORDS, LAYER_WORDS};
use evani_render::model::surface::{SPECULAR, SURFACE_PARAMS};
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

fn uniform_size(m: &naga::Module, name: &str) -> Option<u32> {
    m.global_variables
        .iter()
        .find(|(_, v)| v.space == naga::AddressSpace::Uniform && v.name.as_deref() == Some(name))
        .map(|(_, v)| m.types[v.ty].inner.size(m.to_ctx()))
}

fn slots(spec: bool) -> ShaderSlots {
    let mut s = ShaderSlots::with_builtins();
    s.declare(SPECULAR);
    s.declare(SURFACE_PARAMS);
    if spec {
        s.set_default("specular", SPECULAR_BODY, "test").unwrap();
    }
    s
}

#[test]
fn layered_wgsl_validates_with_both_tails_skinning_and_bodies() {
    for (tail_name, tail) in [("noshadow", NOSHADOW), ("shadows", SHADOWS)] {
        for skin in [false, true] {
            let mut parts = vec![FORWARD, MODEL];
            if skin {
                parts.push(SKIN);
            }
            parts.extend([tail, LIT, BRDF_WGSL, LAYERS, GLUE]);
            let src = parts.concat();
            for spec in [false, true] {
                for bodies in [Bodies::Current, Bodies::EngineDefaults] {
                    let name = format!("layers {tail_name} skin={skin} spec={spec} {bodies:?}");
                    let s = splice(&src, &slots(spec), bodies).unwrap_or_else(|e| panic!("{name}: {e:?}"));
                    let m = validate(&name, &s.source);
                    assert!(entry(&m, "model_layered_fs") && entry(&m, "model_lit_fs"), "{name}");
                    assert_eq!(uniform_size(&m, "ly_mat"), Some(LAYER_WORDS as u32 * 4), "{name}: the layer record");
                    assert_eq!(uniform_size(&m, "ly_frame"), Some(FRAME_WORDS as u32 * 4), "{name}: the frame uniform");
                }
            }
        }
    }
}

/// The shipped forward source (comments stripped, glue for this build's
/// features) carries the layered stage and validates with stubs for what
/// the runtime point-light and ambient patches add.
#[test]
fn shipped_forward_has_the_layered_stage() {
    let fwd = evani_render::shaders::FORWARD.source;
    assert!(fwd.contains("fn model_layered_fs("));
    assert!(fwd.contains("fn ly_d_charlie(") && fwd.contains("fn ly_iridescence("));
    assert!(fwd.contains("// evani.shader.model_lit_layers") && !fwd.contains("// Layer order (three.js)"), "comments stripped, the marker kept");
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
    for spec in [false, true] {
        let s = splice(fwd, &slots(spec), Bodies::Current).unwrap();
        let m = validate(&format!("shipped forward spec={spec}"), &(s.source + &stubs));
        assert!(entry(&m, "model_layered_fs"));
    }
}
