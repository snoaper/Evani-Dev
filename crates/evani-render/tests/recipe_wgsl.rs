//! The recipe stage in evani-render (feature `material-recipes` without
//! `relief`; batch 1 build plan P2a and P2b).
//!
//! With `char-materials` and `material-layers` on too (the P2a set, moved
//! here from evani-material-recipes so that crate has no dev-dependency
//! cycle): the patch validates (naga) on every text the model pipelines can
//! compile, the plain lit stage, the layered stage, each through the
//! character patch first, with both shadow tails, with and without the
//! skinned stage, at 1x and 4x MSAA, with no recipe compiled and with a
//! stand-in recipe and layer that use every core helper; every anchor is in
//! all four texts. Each of the eight condition layers (P3) alone, and all
//! eight together, validate on the same texts.
//!
//! With any feature set: what this build's model pipelines compile (every
//! material's own module, `model::recipes::patched_material`, the shipped
//! forward module through this build's patches) validates, draws the patched
//! stage and stays under the size budget, and a material with no block draws
//! through the unchanged lighting.
#![cfg(all(feature = "material-recipes", not(feature = "relief")))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code: a failed read is a failed test

use evani_core::slots::ShaderSlots;
#[cfg(feature = "char-materials")]
use evani_material_character::lut;
#[cfg(feature = "char-materials")]
use evani_material_character::params::{Values, body};
#[cfg(feature = "char-materials")]
use evani_material_character::shader::{self as chr, CHAR_PARAMS};
#[cfg(all(feature = "char-materials", feature = "material-layers"))]
use evani_material_layers::BRDF_WGSL;
use evani_material_recipes::shader as rec;
#[cfg(all(feature = "char-materials", feature = "material-layers"))]
use evani_material_recipes::shader::{LayerSrc, RecipeSrc};
use evani_render::model::surface::{SPECULAR, SURFACE_PARAMS};
use evani_render::splice::{Bodies, splice};

#[cfg(all(feature = "char-materials", feature = "material-layers"))]
const FORWARD: &str = include_str!("../shaders/forward.wgsl");
#[cfg(all(feature = "char-materials", feature = "material-layers"))]
const MODEL: &str = include_str!("../shaders/model.wgsl");
#[cfg(all(feature = "char-materials", feature = "material-layers"))]
const SKIN: &str = include_str!("../shaders/model_skin.wgsl");
#[cfg(all(feature = "char-materials", feature = "material-layers"))]
const LIT: &str = include_str!("../shaders/model_lit.wgsl");
#[cfg(all(feature = "char-materials", feature = "material-layers"))]
const LAYERS: &str = include_str!("../shaders/model_lit_layers.wgsl");
#[cfg(all(feature = "char-materials", feature = "material-layers"))]
const SHADOWS: &str = include_str!("../shaders/forward_shadows.wgsl");
#[cfg(all(feature = "char-materials", feature = "material-layers"))]
const NOSHADOW: &str = include_str!("../shaders/forward_noshadow.wgsl");

#[cfg(all(feature = "char-materials", feature = "material-layers"))]
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

/// A stand-in recipe (code 1, so `rc_metal`) and layer (wet) that call every
/// core helper, so the generated dispatch and condition pass are validated
/// with real cases, not only empty ones.
#[cfg(all(feature = "char-materials", feature = "material-layers"))]
const STAND_IN_RECIPE: &str = "
fn rc_metal(b: RecBlock, f: MatFrame, i: MatIn, q: u32) -> MatOut {
    var o = mat_identity(i);
    let vo = rc_voronoi2(i.uv * 8.0, b.seed, 1.0);
    let hx = rc_hex(i.uv * 4.0);
    let fb = rc_fbm2(i.uv * 3.0, b.seed, q + 1u);
    let k = rc_aa(f, 0.125) * rc_aa_world(f, 0.05);
    let p2 = rc_proj2(i.world, i.n);
    let w = mix(rc_vnoise2(p2.a, b.seed), rc_vnoise2(p2.b, b.seed), p2.w);
    o.base = vec4<f32>(mix(b.col_a, b.col_b, rc_fade(fb, 0.5, k)) * i.tex.rgb * i.vcol, i.base.a);
    o.rough = clamp(rc_range(b.na.x, 0.1, 0.9) * vo.z + rc_vnoise2(rc_proj(i.world, i.n), b.seed) * w, 0.04, 1.0);
    o.metal = b.nb.y * b.nc.x;
    if (rc_extra(b, RC_X_METAL, 0u)) {
        o.glint = b.col_c * hx.x * f.curv;
    }
    o.emit = vec3<f32>(rc_time() * 0.0 + f.dist * 0.0);
    o.sheen = vec4<f32>(b.col_c, 4.0);
    return o;
}
";

#[cfg(all(feature = "char-materials", feature = "material-layers"))]
const STAND_IN_LAYER: &str = "
fn cond_wet(b: RecBlock, amount: f32, o: MatOut, i: MatIn, f: MatFrame, nz: f32) -> MatOut {
    var r = o;
    r.rough = mix(o.rough, 0.05, amount * nz);
    return r;
}
";

#[cfg(all(feature = "char-materials", feature = "material-layers"))]
const RECIPES: &[RecipeSrc] = &[RecipeSrc {
    code: 1,
    wgsl: STAND_IN_RECIPE,
}];
#[cfg(all(feature = "char-materials", feature = "material-layers"))]
const COND: &[LayerSrc] = &[LayerSrc {
    bit: 0,
    wgsl: STAND_IN_LAYER,
}];

fn validate(name: &str, src: &str) -> naga::Module {
    let module = naga::front::wgsl::parse_str(src)
        .unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(src)));
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .unwrap_or_else(|e| panic!("{name}: {e:?}"));
    module
}

fn slots(with_char: bool) -> ShaderSlots {
    let mut s = ShaderSlots::with_builtins();
    s.declare(SPECULAR);
    s.declare(SURFACE_PARAMS);
    #[cfg(not(feature = "char-materials"))]
    assert!(!with_char, "no character patch in this build");
    #[cfg(feature = "char-materials")]
    if with_char {
        s.declare(CHAR_PARAMS);
        let v = Values {
            quality: 2.0,
            ..Values::default()
        };
        s.set_default("char_params", &body(&v, 2, 4, &lut::fitted()), "test")
            .unwrap();
    }
    s
}

fn uniform_size(m: &naga::Module, name: &str) -> Option<u32> {
    m.global_variables
        .iter()
        .find(|(_, v)| v.space == naga::AddressSpace::Uniform && v.name.as_deref() == Some(name))
        .map(|(_, v)| m.types[v.ty].inner.size(m.to_ctx()))
}

/// The four texts the model pipelines can draw, from `src` (a module with
/// both the plain and the layered stage): (name, entry, character on, text).
#[cfg(all(feature = "char-materials", feature = "material-layers"))]
fn texts(src: &str, msaa: u32) -> Vec<(&'static str, &'static str, bool, String)> {
    vec![
        ("plain", "model_lit_fs", false, src.to_string()),
        ("layers", "model_layered_fs", false, src.to_string()),
        (
            "char",
            "model_lit_fs",
            true,
            chr::patch_lit(src, msaa).expect("char patch"),
        ),
        (
            "char+layers",
            "model_layered_fs",
            true,
            chr::patch_layered(src, msaa).expect("char layered patch"),
        ),
    ]
}

#[cfg(all(feature = "char-materials", feature = "material-layers"))]
fn patch(
    entry: &str,
    text: &str,
    msaa: u32,
    recipes: &[RecipeSrc],
    layers: &[LayerSrc],
) -> Option<String> {
    if entry == "model_layered_fs" {
        rec::patch_layered_with(text, msaa, recipes, layers)
    } else {
        rec::patch_lit_with(text, msaa, recipes, layers)
    }
}

#[cfg(all(feature = "char-materials", feature = "material-layers"))]
#[test]
fn every_anchor_is_in_all_four_texts() {
    let src = [FORWARD, MODEL, NOSHADOW, LIT, BRDF_WGSL, LAYERS, GLUE].concat();
    for (name, entry, _, text) in texts(&src, 4) {
        let at = text.find(&format!("fn {entry}(")).unwrap();
        let body = &text[at..at + text[at..].find("\n}").unwrap()];
        for edit in rec::EDITS {
            assert!(
                edit.iter().any(|(find, _)| body.contains(find)),
                "{name}: none of {edit:?}"
            );
        }
    }
}

#[cfg(all(feature = "char-materials", feature = "material-layers"))]
#[test]
fn the_patch_validates_on_every_combination() {
    for (tail_name, tail) in [("noshadow", NOSHADOW), ("shadows", SHADOWS)] {
        for skin in [false, true] {
            let mut parts = vec![FORWARD, MODEL];
            if skin {
                parts.push(SKIN);
            }
            parts.extend([tail, LIT, BRDF_WGSL, LAYERS, GLUE]);
            let src = parts.concat();
            for msaa in [1, 4] {
                for (name, entry, with_char, text) in texts(&src, msaa) {
                    for (set, recipes, layers) in
                        [("empty", &[][..], &[][..]), ("stand-in", RECIPES, COND)]
                    {
                        let label = format!("{name} {tail_name} skin={skin} msaa={msaa} {set}");
                        let p = patch(entry, &text, msaa, recipes, layers)
                            .unwrap_or_else(|| panic!("{label}: an anchor is missing"));
                        let s = splice(&p, &slots(with_char), Bodies::Current)
                            .unwrap_or_else(|e| panic!("{label}: {e:?}"));
                        let m = validate(&label, &s.source);
                        assert_eq!(
                            uniform_size(&m, "rc_mat"),
                            Some(32),
                            "{label}: the block is 32 bytes"
                        );
                        assert_eq!(
                            uniform_size(&m, "rc_frame"),
                            Some(rec::FRAME_BYTES as u32),
                            "{label}"
                        );
                        assert!(m.entry_points.iter().any(|e| e.name == entry), "{label}");
                        let at = s.source.find(&format!("fn {entry}(")).unwrap();
                        assert!(
                            s.source[at..].contains("let ro = mat_surface("),
                            "{label}: the drawn stage is patched"
                        );
                        if set == "stand-in" {
                            assert!(
                                s.source.contains("case 1u: {")
                                    && s.source.contains("o = cond_wet("),
                                "{label}"
                            );
                        }
                    }
                }
            }
        }
    }
}

/// Each condition layer alone (on the four texts, 1x and 4x, with the
/// stand-in recipe) and all eight together (on every combination of shadow
/// tail, skinning, text and sample count) validate, with the generated
/// condition pass calling each compiled layer and the frame uniform at its
/// grown size.
#[cfg(all(feature = "char-materials", feature = "material-layers"))]
#[test]
fn every_condition_layer_alone_and_all_together_validate() {
    let all = rec::ALL_LAYERS;
    let mut sets: Vec<(&str, Vec<LayerSrc>, bool)> = all
        .iter()
        .map(|l| (rec::LAYER_NAMES[usize::from(l.bit)], vec![*l], false))
        .collect();
    sets.push(("all eight", all.to_vec(), true));
    for (set, layers, full) in &sets {
        let tails: &[(&str, &str)] = if *full {
            &[("noshadow", NOSHADOW), ("shadows", SHADOWS)]
        } else {
            &[("noshadow", NOSHADOW)]
        };
        for &(tail_name, tail) in tails {
            for skin in if *full { &[false, true][..] } else { &[false][..] } {
                let mut parts = vec![FORWARD, MODEL];
                if *skin {
                    parts.push(SKIN);
                }
                parts.extend([tail, LIT, BRDF_WGSL, LAYERS, GLUE]);
                let src = parts.concat();
                for msaa in [1, 4] {
                    for (name, entry, with_char, text) in texts(&src, msaa) {
                        let label = format!("{set}: {name} {tail_name} skin={skin} msaa={msaa}");
                        let p = patch(entry, &text, msaa, RECIPES, layers)
                            .unwrap_or_else(|| panic!("{label}: an anchor is missing"));
                        let s = splice(&p, &slots(with_char), Bodies::Current)
                            .unwrap_or_else(|e| panic!("{label}: {e:?}"));
                        let m = validate(&label, &s.source);
                        assert_eq!(uniform_size(&m, "rc_frame"), Some(rec::FRAME_BYTES as u32), "{label}");
                        for l in layers {
                            let f = format!("o = cond_{}(b, rc_amount(b, {}u)", rec::LAYER_NAMES[usize::from(l.bit)], l.bit);
                            assert!(s.source.contains(&f), "{label}: {f}");
                        }
                    }
                }
            }
        }
    }
}

/// The built recipes (P4 to P9: metal, paint, plastic, wood, bark, stone, masonry, plaster, ground, leather, fur,
/// feather, scales, hide, keratin, organic, ceramic, gem, ice, fire, glow; `rec::ALL_RECIPES`, so later packages are
/// covered as they land) each alone,
/// P6's four together, P7's two together, P8's two together, P9's five together, P4 to P9 (all twenty-one) together,
/// all together,
/// and all together with all eight condition layers validate on the four
/// texts at 1x and 4x; the last set also on both shadow tails and skinned.
/// The dispatch has one case per recipe and each calls its own function.
#[cfg(all(feature = "char-materials", feature = "material-layers"))]
#[test]
fn every_recipe_alone_together_and_with_all_layers_validates() {
    let all = rec::ALL_RECIPES;
    let mut sets: Vec<(String, Vec<RecipeSrc>, Vec<LayerSrc>, bool)> = all
        .iter()
        .map(|r| {
            let name = rec::stage_recipe(r.code).unwrap().name;
            (name.to_string(), vec![*r], Vec::new(), false)
        })
        .collect();
    let p6: Vec<RecipeSrc> = all.iter().copied().filter(|r| [11, 12, 15, 19].contains(&r.code)).collect();
    assert_eq!(p6.len(), 4);
    sets.push(("P6 recipes".into(), p6.clone(), Vec::new(), false));
    sets.push(("P6 recipes + all layers".into(), p6, rec::ALL_LAYERS.to_vec(), true));
    let p7: Vec<RecipeSrc> = all.iter().copied().filter(|r| [13, 14].contains(&r.code)).collect();
    assert_eq!(p7.len(), 2);
    sets.push(("P7 recipes".into(), p7.clone(), Vec::new(), false));
    sets.push(("P7 recipes + all layers".into(), p7, rec::ALL_LAYERS.to_vec(), true));
    let p8: Vec<RecipeSrc> = all.iter().copied().filter(|r| [25, 27].contains(&r.code)).collect();
    assert_eq!(p8.len(), 2);
    sets.push(("P8 recipes".into(), p8.clone(), Vec::new(), false));
    sets.push(("P8 recipes + all layers".into(), p8, rec::ALL_LAYERS.to_vec(), true));
    let p9: Vec<RecipeSrc> = all.iter().copied().filter(|r| [2, 3, 22, 28, 29].contains(&r.code)).collect();
    assert_eq!(p9.len(), 5);
    sets.push(("P9 recipes".into(), p9.clone(), Vec::new(), false));
    sets.push(("P9 recipes + all layers".into(), p9, rec::ALL_LAYERS.to_vec(), true));
    // P10a: paint's clear coat (rc_paint_coat through rc_post, after
    // lighting) on every text, alone and with all eight layers.
    let paint: Vec<RecipeSrc> = all.iter().copied().filter(|r| r.code == 2).collect();
    sets.push(("P10a paint coat + all layers".into(), paint, rec::ALL_LAYERS.to_vec(), true));
    // B3-P1: the recipes that draw a pattern layout (metal, paint, wood,
    // stone, masonry, cloth, thin, ceramic) together, with the layouts
    // appended once.
    let b3: Vec<RecipeSrc> = all.iter().copied().filter(rec::uses_layout).collect();
    assert_eq!(b3.iter().map(|r| r.code).collect::<Vec<_>>(), [1, 2, 4, 6, 7, 10, 21, 23, 24]);
    sets.push(("B3-P1 layout recipes + all layers".into(), b3, rec::ALL_LAYERS.to_vec(), true));
    // Wood without its layouts part (`recipe-wood-layouts` off: the planks
    // stand-in), alone and with all eight layers.
    sets.push(("wood without layouts + all layers".into(), vec![rec::WOOD_PLANKS], rec::ALL_LAYERS.to_vec(), true));
    // Organic without its food part (material-view round 6: the plain
    // stand-in), alone and with all eight layers.
    sets.push(("organic without food + all layers".into(), vec![rec::ORGANIC_PLAIN], rec::ALL_LAYERS.to_vec(), true));
    // Batch 2, B2-P1: foliage with all eight layers (rain and snow on leaves).
    let foliage: Vec<RecipeSrc> = all.iter().copied().filter(|r| r.code == 20).collect();
    assert_eq!(foliage.len(), 1);
    sets.push(("B2-P1 foliage + all layers".into(), foliage, rec::ALL_LAYERS.to_vec(), true));
    // B4-P3b: cloth's sheen and fuzz after lighting (rc_cloth_post through
    // rc_post, before paint's coat), with paint and all eight layers.
    let cloth: Vec<RecipeSrc> = all.iter().copied().filter(|r| [2, 10].contains(&r.code)).collect();
    sets.push(("B4-P3b cloth + paint + all layers".into(), cloth, rec::ALL_LAYERS.to_vec(), true));
    sets.push(("all recipes".into(), all.to_vec(), Vec::new(), false));
    sets.push((
        "all recipes + all layers".into(),
        all.to_vec(),
        rec::ALL_LAYERS.to_vec(),
        true,
    ));
    for (set, recipes, layers, full) in &sets {
        let tails: &[(&str, &str)] = if *full {
            &[("noshadow", NOSHADOW), ("shadows", SHADOWS)]
        } else {
            &[("noshadow", NOSHADOW)]
        };
        for &(tail_name, tail) in tails {
            for skin in if *full { &[false, true][..] } else { &[false][..] } {
                let mut parts = vec![FORWARD, MODEL];
                if *skin {
                    parts.push(SKIN);
                }
                parts.extend([tail, LIT, BRDF_WGSL, LAYERS, GLUE]);
                let src = parts.concat();
                for msaa in [1, 4] {
                    for (name, entry, with_char, text) in texts(&src, msaa) {
                        let label = format!("{set}: {name} {tail_name} skin={skin} msaa={msaa}");
                        let p = patch(entry, &text, msaa, recipes, layers)
                            .unwrap_or_else(|| panic!("{label}: an anchor is missing"));
                        let s = splice(&p, &slots(with_char), Bodies::Current)
                            .unwrap_or_else(|e| panic!("{label}: {e:?}"));
                        let m = validate(&label, &s.source);
                        assert_eq!(uniform_size(&m, "rc_frame"), Some(rec::FRAME_BYTES as u32), "{label}");
                        for r in recipes {
                            let row = rec::stage_recipe(r.code).unwrap();
                            let case = format!("case {}u: {{\n            r = rc_{}(b, f, i, rc_q({}u));", row.code, row.name, row.group);
                            assert!(s.source.contains(&case), "{label}: {case}");
                        }
                        assert_eq!(s.source.matches("fn rc_out(").count(), 1, "{label}: shared helpers once");
                        assert_eq!(s.source.matches("fn rc_post(").count(), 1, "{label}: one post-light hook");
                        assert_eq!(
                            s.source.matches("fn rc_lay_bond(").count(),
                            usize::from(recipes.iter().any(rec::uses_layout)),
                            "{label}: the layouts once, exactly when a recipe draws one"
                        );
                        assert_eq!(
                            s.source.contains("return rc_paint_coat("),
                            recipes.iter().any(|r| r.code == 2),
                            "{label}: the coat runs exactly when paint is compiled"
                        );
                        assert_eq!(
                            s.source.contains("rc_cloth_post(c, o, v, l, sun, vis)"),
                            rec::CLOTH_POST && recipes.iter().any(|r| r.code == 10),
                            "{label}: cloth's sheen runs exactly when cloth is compiled"
                        );
                    }
                }
            }
        }
    }
}

/// The Rust frame words and the WGSL `RecFrame` are the same size, and the
/// condition look numbers sit in the words the WGSL reads them from.
#[test]
fn the_frame_uniform_sizes_match() {
    let words = evani_material_recipes::params::default_frame("high", 0.0);
    assert_eq!(words.len() * 4, rec::FRAME_BYTES);
    let src = rec::stage_wgsl().replace(' ', "");
    let at = src.find("structRecFrame{").unwrap();
    let body = &src[at..at + src[at..].find("};").unwrap()];
    let vec4s = body.matches("vec4<").count();
    assert_eq!(vec4s * 16, rec::FRAME_BYTES, "RecFrame is all vec4 members");
    for lk in evani_material_recipes::conditions::LOOKS {
        let member = ["ka", "kb", "kc", "kd"][(lk.word - 16) / 4];
        assert!(body.contains(&format!("{member}:vec4<")), "{}", lk.name);
    }
}

/// Stubs for the hooks other features fill in, so the shipped module
/// validates on its own.
fn stubs(fwd: &str) -> String {
    let mut stubs = String::new();
    if fwd.contains("point_lights(") && !fwd.contains("fn point_lights(") {
        stubs.push_str("\nfn point_lights(p: vec3<f32>, n: vec3<f32>, frag: vec4<f32>) -> vec3<f32> {\n    return vec3<f32>(0.0);\n}\n");
    }
    if fwd.contains("amb_occlusion(") && !fwd.contains("fn amb_occlusion(") {
        stubs.push_str("\nfn amb_occlusion(frag: vec4<f32>) -> f32 {\n    return 1.0;\n}\n");
    }
    if fwd.contains("amb_sh(") && !fwd.contains("fn amb_sh(") {
        stubs.push_str("\nstruct LitStubAmb {\n    sun_color: vec4<f32>,\n};\n@group(0) @binding(15) var<uniform> amb: LitStubAmb;\nfn amb_sh(n: vec3<f32>) -> vec3<f32> {\n    return n;\n}\nfn amb_env(r: vec3<f32>, rough: f32) -> vec3<f32> {\n    return r * rough;\n}\n");
    }
    stubs
}

/// The stage this build's model pipelines draw (gpu/models.rs `LIT_FS`).
const DRAWN: &str = if cfg!(feature = "material-layers") {
    "model_layered_fs"
} else {
    "model_lit_fs"
};

/// The body of `entry` in `src`, up to its closing brace.
fn body_of<'a>(src: &'a str, entry: &str) -> &'a str {
    let at = src
        .find(&format!("fn {entry}("))
        .unwrap_or_else(|| panic!("no {entry}"));
    &src[at..at + src[at..].find("\n}").unwrap() + 2]
}

/// Every material key this build can make: (pass, msaa, recipe code,
/// character kind). No recipe and each compiled stage recipe, each kind the
/// build draws, each pass it draws (glass at one sample only).
fn material_keys() -> Vec<(u8, u32, u8, u8)> {
    use evani_material_recipes::split::{BLEND, GLASS, OPAQUE, see_built};
    let mut codes = vec![0u8];
    codes.extend(rec::COMPILED.iter().map(|r| r.code).filter(|c| rec::stage_recipe(*c).is_some()));
    let kinds: &[u8] = if cfg!(feature = "char-materials") { &[0, 1, 2, 3] } else { &[0] };
    let mut keys = Vec::new();
    // The skinned pass draws the opaque text through the skinned vertex
    // stage: its own pipeline, so its own module (gpu/split.rs).
    let skinned = cfg!(feature = "skinning").then_some(evani_render::model::recipes::SKINNED);
    for see in [OPAQUE, BLEND, GLASS].into_iter().filter(|s| see_built(*s)).chain(skinned) {
        for msaa in if see == GLASS { vec![1] } else { vec![1, 4] } {
            for &code in &codes {
                // A character kind's material names its character recipe,
                // which has no stage case: it shares the no-recipe text.
                for &kind in kinds.iter().filter(|k| code == 0 || **k == 0) {
                    keys.push((see, msaa, code, kind));
                }
            }
        }
    }
    keys
}

/// The entry points pass `see`'s material pipeline compiles: the engine's
/// own list (one vertex stage and the drawn stage), so the test sizes the
/// modules the engine builds, never one with both vertex stages.
fn entries(see: u8) -> Vec<&'static str> {
    let e = evani_render::model::recipes::material_entries(see);
    assert_eq!(e[1], DRAWN, "the engine draws the stage this test patches");
    e.to_vec()
}

/// What this build compiles: every material's module (the shipped forward
/// module through the character patch, with `char-materials`, and the recipe
/// patch for that one recipe, kind and pass, at 1x and 4x, cut to its entry
/// points) validates with the block and frame uniforms at their sizes and
/// the drawn stage patched, and stays under the size budget (60 KB; the
/// engine's own light and sky patches add a little over the stubs here, so
/// this holds a margin for them). Run under each feature set
/// (material-recipes alone, with material-layers, char-materials, both, and
/// with skinning).
#[test]
fn this_build_compiles_the_patched_stage() {
    use evani_material_recipes::split::{MODULE_BUDGET, OPAQUE, shake};
    let fwd = evani_render::shaders::FORWARD.source;
    let with_char = cfg!(feature = "char-materials");
    let mut largest = (0, String::new());
    let mut over: Vec<String> = Vec::new();
    for (see, msaa, code, kind) in material_keys() {
        let label = format!("pass {see} msaa={msaa} recipe {code} kind {kind}");
        // The skinned pass compiles the opaque text (gpu/split.rs `template`).
        let text_see = if see == evani_render::model::recipes::SKINNED { OPAQUE } else { see };
        let p = evani_render::model::recipes::patched_material(msaa, code, text_see, kind)
            .unwrap_or_else(|| panic!("{label}: an anchor is missing"));
        let s = splice(&p, &slots(with_char), Bodies::Current)
            .unwrap_or_else(|e| panic!("{label}: {e:?}"));
        let cut = shake(&(s.source.clone() + &stubs(fwd)), &entries(see));
        assert!(
            !(cut.contains("fn model_vs(") && cut.contains("fn model_skin_vs(")),
            "{label}: a material module has one vertex stage"
        );
        let src = rec::COMPILED.iter().find(|r| r.code == code);
        let stage = evani_material_recipes::split::stage_for(src, rec::LAYERS, rec::CUT_BUILT, cfg!(feature = "material-recipe-style"), text_see).len();
        let share = cut.len() - stage.min(cut.len());
        println!("SIZE {label} module={} stage={stage} share={share}", cut.len());        use evani_material_recipes::split::{CHAR_ALLOWANCE, CHAR_STAGE_BUDGET, STAGE_BUDGET, TEMPLATE_ALLOWANCE};
        let (allowance, stage_budget) = if kind == 0 { (TEMPLATE_ALLOWANCE, STAGE_BUDGET) } else { (CHAR_ALLOWANCE, CHAR_STAGE_BUDGET) };
        if share + PATCH_MARGIN > allowance {
            over.push(format!("{label}: the module's own share ({share} bytes, +{PATCH_MARGIN} for the light and sky patches) is over its {allowance}-byte allowance"));
        }
        if stage > stage_budget {
            over.push(format!("{label}: the recipe stage ({stage} bytes) is over its {stage_budget}-byte share"));
        }
        if cut.len() + PATCH_MARGIN > MODULE_BUDGET {
            over.push(format!("{label}: {} bytes (+{PATCH_MARGIN} for the light and sky patches) is over the {MODULE_BUDGET}-byte budget", cut.len()));
        }
        if cut.len() > largest.0 {
            largest = (cut.len(), label.clone());
        }
        if see != OPAQUE || code != 0 {
            continue;
        }
        let m = validate(&label, &(s.source.clone() + &stubs(fwd)));
        assert_eq!(
            uniform_size(&m, "rc_mat"),
            Some(32),
            "{label}: the block is 32 bytes"
        );
        assert_eq!(
            uniform_size(&m, "rc_frame"),
            Some(rec::FRAME_BYTES as u32),
            "{label}"
        );
        assert!(
            m.entry_points.iter().any(|e| e.name == DRAWN),
            "{label}: {DRAWN}"
        );
        assert!(
            body_of(&s.source, DRAWN).contains("let ro = mat_surface("),
            "{label}: the drawn stage is patched"
        );
        assert!(
            s.source
                .contains(&format!("const REC_MSAA: u32 = {msaa}u;")),
            "{label}"
        );
        // The style detail step: both hooks in, its WGSL validated above.
        assert_eq!(
            s.source.contains("rc_style_surface("),
            cfg!(feature = "material-recipe-style"),
            "{label}: the style step is in exactly when its feature is"
        );
        #[cfg(feature = "skinning")]
        assert!(
            m.entry_points.iter().any(|e| e.name.contains("skin")),
            "{label}: the skinned stage is in"
        );
        #[cfg(feature = "char-materials")]
        assert!(
            body_of(&s.source, DRAWN).contains("char_"),
            "{label}: the character patch came first"
        );
    }
    println!("largest material module: {} bytes ({})", largest.0, largest.1);
    assert!(over.is_empty(), "over the shader budget:\n{}", over.join("\n"));
}

/// Room the shader budget test keeps for what the engine's own patches add
/// on the web over the stubs here (point lights, the sky and ambient
/// occlusion): measured in the material viewer's ship build, 2026-10-29,
/// comments stripped: 1.4 KB (character kinds) to 3.3 KB (plain).
const PATCH_MARGIN: usize = 4 * 1024;

/// Each material's module, cut down, validates on its own (naga): every
/// recipe, kind and pass the build makes.
#[test]
fn every_material_module_validates() {
    use evani_material_recipes::split::shake;
    let fwd = evani_render::shaders::FORWARD.source;
    let with_char = cfg!(feature = "char-materials");
    for (see, msaa, code, kind) in material_keys() {
        let label = format!("pass {see} msaa={msaa} recipe {code} kind {kind}");
        let text_see = if see == evani_render::model::recipes::SKINNED { evani_material_recipes::split::OPAQUE } else { see };
        let p = evani_render::model::recipes::patched_material(msaa, code, text_see, kind).unwrap();
        let s = splice(&p, &slots(with_char), Bodies::Current).unwrap();
        let e = entries(see);
        let cut = shake(&(s.source + &stubs(fwd)), &e);
        let m = validate(&label, &cut);
        assert!(m.entry_points.iter().any(|x| x.name == DRAWN), "{label}: {DRAWN} kept");
        assert!(m.entry_points.iter().any(|x| x.name == e[0]), "{label}: {} kept", e[0]);
        assert_eq!(m.entry_points.len(), e.len(), "{label}: only the pipeline's entry points");
    }
}

/// A material with no block (recipe 0, all zero) draws through the
/// unchanged lighting: undoing the five edits gives back the drawn stage
/// exactly as the build compiles it without the feature, `mat_surface`
/// returns the lit stage's own values for recipe 0, and `mat_extra` then adds
/// nothing (emission, sheen and glint are zero).
#[test]
fn no_block_draws_the_unchanged_lighting() {
    let fwd = evani_render::shaders::FORWARD.source;
    #[cfg(all(feature = "char-materials", feature = "material-layers"))]
    let before = chr::patch_layered(fwd, 1).unwrap();
    #[cfg(all(feature = "char-materials", not(feature = "material-layers")))]
    let before = chr::patch_lit(fwd, 1).unwrap();
    #[cfg(not(feature = "char-materials"))]
    let before = fwd.to_string();
    #[cfg(feature = "material-layers")]
    let entry = "fn model_layered_fs(";
    #[cfg(not(feature = "material-layers"))]
    let entry = "fn model_lit_fs(";
    let after = evani_material_recipes::split::patch_one(&before, entry, 1, 0, evani_material_recipes::split::OPAQUE).unwrap();
    let mut undone = body_of(&after, DRAWN).to_string();
    for alts in rec::EDITS.iter().rev() {
        let with = alts[0].1;
        let orig = alts
            .iter()
            .map(|&(f, _)| f)
            .find(|f| body_of(&before, DRAWN).contains(f))
            .unwrap();
        assert!(undone.contains(with), "{with}");
        undone = undone.replacen(with, orig, 1);
    }
    assert_eq!(
        undone,
        body_of(&before, DRAWN),
        "outside the five edited lines, the drawn stage is untouched"
    );
    // Round 9 (CORE_REQUESTS object-local-position): the vertex output
    // gains the object-local position and normal; nothing else before the
    // drawn stage moves.
    let at = before.find(&format!("fn {DRAWN}(")).unwrap();
    let at_after = after.find(&format!("fn {DRAWN}(")).unwrap();
    let without_local = after[..at_after]
        .replace(
            " @location(7) local: vec3<f32>, @location(8) lnorm: vec3<f32>,",
            "",
        )
        .replace(" o.local = local.xyz; o.lnorm = in.normal.xyz;", "");
    assert_eq!(
        without_local,
        &before[..at],
        "nothing before the drawn stage moved but the object-local outputs"
    );
    // The stage is stripped tight (no spaces beside punctuation): compare
    // without spaces.
    let stage = rec::stage_wgsl().replace(' ', "");
    let tight = |s: &str| s.replace(' ', "");
    assert!(
        stage.contains(&tight("return MatOut(i.base, i.rough, i.metal, i.n, i.occ, vec3<f32>(0.0), vec4<f32>(0.0), vec3<f32>(0.0));")),
        "the identity keeps the lit stage's values and zeroes the post-light terms"
    );
    let surf = &stage[stage.find("fnmat_surface(").unwrap()..];
    assert!(
        surf.contains(&tight("let o = mat_identity(i);\nif (b.recipe == 0u || (rc_frame.misc.z & 1u) != 0u) {\nreturn o;")),
        "recipe 0 and recipes off return the identity"
    );
    let extra = &stage[stage.find("fnmat_extra(").unwrap()..];
    assert!(
        extra.contains(&tight("var c = o.emit;"))
            && extra.contains(&tight("if (o.sheen.w > 0.0)"))
            && extra.contains(&tight("c = c + o.glint *"))
            && extra.contains(&tight(if cfg!(feature = "material-recipe-style") {
                "return rc_style_post(c0, rc_post(c0 + c, o, g, v, l, sun, vis));"
            } else {
                "return rc_post(c0 + c, o, g, v, l, sun, vis);"
            })),
        "zero terms add nothing"
    );
    // The style detail step leaves a material it didn't style (no block,
    // recipe 0: mat_surface returns before the step) exactly as it was.
    #[cfg(feature = "material-recipe-style")]
    {
        let post = &stage[stage.find("fnrc_style_post(").unwrap()..];
        assert!(post.contains(&tight("if (!rc_styled) {\nreturn c;")), "unstyled colour unchanged");
        assert!(stage.contains(&tight("var<private> rc_styled: bool = false;")));
    }
    // rc_post is the identity without paint; with paint, the coat returns the
    // colour unchanged for any other recipe, with recipes off and at lite.
    let post = &stage[stage.find("fnrc_post(").unwrap()..];
    let paint = rec::COMPILED.iter().any(|r| r.code == 2);
    assert_eq!(post.starts_with(&tight("fn rc_post(c: vec3<f32>, o: MatOut, g: vec3<f32>, v: vec3<f32>, l: vec3<f32>, sun: vec3<f32>, vis: f32) -> vec3<f32> {
return c;")), !paint);
    if paint {
        let coat = &stage[stage.find("fnrc_paint_coat(").unwrap()..];
        assert!(coat.contains(&tight("if (b.recipe != 2u || (rc_frame.misc.z & 1u) != 0u || !rc_extra(b, RC_X_PAINT, 0u) || rc_q(0u) == 0u) {
return c;")));
    }
}

/// B3-P2, the cut-out: with metal and wood (the cut-out recipes) and all
/// eight layers, the cut-out's patch validates on every combination of text,
/// shadow tail, skinning and sample count (naga's uniformity analysis on:
/// every derivative is taken before both discards), calls `mat_cut` after the
/// recipe and marks partly covered pixels on the return.
#[cfg(all(feature = "char-materials", feature = "material-layers"))]
#[test]
fn the_cut_out_validates_on_every_combination() {
    let recipes: Vec<RecipeSrc> = rec::ALL_RECIPES.iter().copied().filter(|r| r.code == 1 || r.code == 4).collect();
    for (tail_name, tail) in [("noshadow", NOSHADOW), ("shadows", SHADOWS)] {
        for skin in [false, true] {
            let mut parts = vec![FORWARD, MODEL];
            if skin {
                parts.push(SKIN);
            }
            parts.extend([tail, LIT, BRDF_WGSL, LAYERS, GLUE]);
            let src = parts.concat();
            for msaa in [1, 4] {
                for (name, entry, with_char, text) in texts(&src, msaa) {
                    let label = format!("cut {name} {tail_name} skin={skin} msaa={msaa}");
                    let p = if entry == "model_layered_fs" {
                        rec::patch_layered_cut_with(&text, msaa, &recipes, &rec::ALL_LAYERS)
                    } else {
                        rec::patch_lit_cut_with(&text, msaa, &recipes, &rec::ALL_LAYERS)
                    }
                    .unwrap_or_else(|| panic!("{label}: an anchor is missing"));
                    let s = splice(&p, &slots(with_char), Bodies::Current).unwrap_or_else(|e| panic!("{label}: {e:?}"));
                    validate(&label, &s.source);
                    let at = s.source.find(&format!("fn {entry}(")).unwrap();
                    let body = &s.source[at..at + s.source[at..].find("\n}").unwrap()];
                    let (surf, cut) = (body.find("let ro = mat_surface(").unwrap(), body.find("let rk = mat_cut(").unwrap());
                    assert!(surf < cut, "{label}: the holes come after the recipe");
                    assert!(body.contains("select(1.0, 0.0, rcut < 0.999))"), "{label}: the edge mark");
                    assert_eq!(s.source.matches("fn rc_cut_cov(").count(), 1, "{label}");
                }
            }
        }
    }
}

/// B3-P2: without the cut-out the patch is exactly what it was (no cut text,
/// no edit), so a game that doesn't list `cutout` downloads none of it.
#[test]
fn without_the_cut_out_nothing_changes() {
    let src = [FORWARD, MODEL, NOSHADOW, LIT].concat();
    let recipes: Vec<RecipeSrc> = rec::ALL_RECIPES.to_vec();
    let plain = rec::patch_lit_with(&src, 4, &recipes, &rec::ALL_LAYERS).unwrap();
    for bad in ["mat_cut", "rc_cut_", "rcut", "RcCut"] {
        assert!(!plain.contains(bad), "{bad}");
    }
    let cut = rec::patch_lit_cut_with(&src, 4, &recipes, &rec::ALL_LAYERS).unwrap();
    assert!(cut.contains("fn mat_cut(") && cut.len() > plain.len());
    assert!(rec::patch_lit_cut_with(&src, 4, &[], &[]).unwrap() == rec::patch_lit_with(&src, 4, &[], &[]).unwrap(), "no recipe, nothing to cut");
}

/// B3-P2: the hole-aware depth module (the model shadow uniform and vertex
/// input, model_cut_shadows.wgsl and the recipe crate's cut.wgsl) validates,
/// discards on both the texture's alpha and the holes, and declares no
/// binding the skinned stage uses.
///
/// B3-P3: the same module with the see-through stipple, the cut-out, or both
/// (each build's tail, `shader::shadow_tail`), validates.
#[test]
fn the_cut_out_shadow_module_validates() {
    for (cut, blend) in [(true, false), (false, true), (true, true)] {
        let src = [
            include_str!("../shaders/shadows_depth.wgsl"),
            include_str!("../shaders/model_shadows.wgsl"),
            include_str!("../shaders/model_cut_shadows.wgsl"),
            "\n",
            &rec::shadow_tail(cut, blend),
        ]
        .concat();
        let label = format!("cut shadows cut={cut} blend={blend}");
        let m = validate(&label, &src);
        for e in ["model_cut_shadow_vs", "model_cut_shadow_fs"] {
            assert!(m.entry_points.iter().any(|p| p.name == e), "{label}: {e}");
        }
        assert_eq!(uniform_size(&m, "cut_block"), Some(32));
        assert_eq!(src.matches("discard;").count(), 2);
        assert!(src.contains("cut_mat.flags.x > 0.5 && a < cut_mat.emissive_cutoff.w"), "the mask cuts MASK parts only");
        assert_eq!(src.contains("fn rc_cut_cov("), cut, "{label}");
        assert_eq!(src.contains("fn rc_blend_density("), blend, "{label}");
    }
}

/// B3-P3, the see-through pass: with glass, fire and glow (the see-through
/// recipes) and all eight layers, the blend patch validates on every text,
/// shadow tail, skinning and sample count, as the blend module (the stub) and
/// as the glass module (the copy's sampler, one sample): the recipe colour is
/// premultiplied before lighting and the final return is wrapped.
#[cfg(all(feature = "char-materials", feature = "material-layers"))]
#[test]
fn the_blend_pass_validates_on_every_combination() {
    let recipes: Vec<RecipeSrc> = rec::ALL_RECIPES.iter().copied().filter(|r| [24, 28, 29].contains(&r.code)).collect();
    for (tail_name, tail) in [("noshadow", NOSHADOW), ("shadows", SHADOWS)] {
        for skin in [false, true] {
            let mut parts = vec![FORWARD, MODEL];
            if skin {
                parts.push(SKIN);
            }
            parts.extend([tail, LIT, BRDF_WGSL, LAYERS, GLUE]);
            let src = parts.concat();
            for msaa in [1, 4] {
                for (name, entry, with_char, text) in texts(&src, msaa) {
                    for refract in [false, true] {
                        if refract && msaa == 4 {
                            continue; // the glass pipeline is one sample
                        }
                        let label = format!("blend {name} {tail_name} skin={skin} msaa={msaa} refract={refract}");
                        let p = if entry == "model_layered_fs" {
                            rec::patch_layered_blend_with(&text, msaa, &recipes, &rec::ALL_LAYERS, refract)
                        } else {
                            rec::patch_lit_blend_with(&text, msaa, &recipes, &rec::ALL_LAYERS, refract)
                        }
                        .unwrap_or_else(|| panic!("{label}: an anchor is missing"));
                        let s = splice(&p, &slots(with_char), Bodies::Current).unwrap_or_else(|e| panic!("{label}: {e:?}"));
                        validate(&label, &s.source);
                        let at = s.source.find(&format!("fn {entry}(")).unwrap();
                        let body = &s.source[at..at + s.source[at..].find("\n}").unwrap()];
                        assert!(body.contains("base = mat_premul(ro.base);"), "{label}: premultiplied before lighting");
                        assert!(body[body.rfind("return ").unwrap()..].contains("mat_blend(vec4<f32>(mix(c, frame.fog_color.rgb, f)"), "{label}: the returned colour is wrapped");
                        assert!(s.source.contains("override REC_BLEND"), "{label}");
                        assert_eq!(s.source.contains("var rc_refr:"), refract, "{label}: the copy only in the glass module");
                    }
                }
            }
        }
    }
}

/// B3-P3: without the see-through pass the patch is exactly what it was, so
/// a game that doesn't list it downloads none of it; with it and no recipe,
/// nothing changes either.
#[test]
fn without_the_blend_pass_nothing_changes() {
    let src = [FORWARD, MODEL, NOSHADOW, LIT].concat();
    let recipes: Vec<RecipeSrc> = rec::ALL_RECIPES.to_vec();
    let plain = rec::patch_lit_with(&src, 4, &recipes, &rec::ALL_LAYERS).unwrap();
    for bad in ["mat_blend", "mat_premul", "REC_BLEND", "mat_refract", "rc_refr"] {
        assert!(!plain.contains(bad), "{bad}");
    }
    let blend = rec::patch_lit_blend_with(&src, 4, &recipes, &rec::ALL_LAYERS, false).unwrap();
    assert!(blend.contains("fn mat_blend(") && blend.len() > plain.len());
    assert!(rec::patch_lit_blend_with(&src, 4, &[], &[], false).unwrap() == rec::patch_lit_with(&src, 4, &[], &[]).unwrap());
}
