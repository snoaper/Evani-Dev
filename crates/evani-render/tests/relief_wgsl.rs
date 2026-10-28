//! S3-10: surface relief on textured models (feature `relief`). The relief
//! fragment stage validates with both shadow tails and either set of slot
//! bodies, and shades exactly as `model_fs` after its normal; the
//! micro-detail fades out where its pattern is under 1.5 px on screen; and
//! the height-map parallax is fine on High and Ultra and cheap (2 steps)
//! elsewhere, while the normal map
//! and micro-detail read the same on every tier (Chris's same-look rule).

#![cfg(feature = "relief")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code: a failed read is a failed test

use evani_core::slots::{MICRO_DETAIL, ShaderSlots};
use evani_render::splice::{Bodies, splice};

const FORWARD: &str = include_str!("../shaders/forward.wgsl");
const MODEL: &str = include_str!("../shaders/model.wgsl");
const RELIEF: &str = include_str!("../shaders/model_relief.wgsl");
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

/// The body of `fn <name>(` in `src`, from `from` to its closing brace,
/// without comments or blank space (so a reworded comment is no drift).
fn tail(src: &str, name: &str, from: &str) -> String {
    let start = src.find(&format!("fn {name}(")).unwrap_or_else(|| panic!("{name}"));
    let body = &src[start..];
    let end = body.find("\n}").unwrap_or_else(|| panic!("{name} ends")) + 2;
    let body = &body[..end];
    let at = body.find(from).unwrap_or_else(|| panic!("{name} has `{from}`"));
    body[at..].lines().map(|l| l.split("//").next().unwrap_or("").trim()).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n")
}

#[test]
fn relief_wgsl_validates() {
    let mut slots = ShaderSlots::with_builtins();
    slots.declare(MICRO_DETAIL);
    for t in [NOSHADOW, SHADOWS] {
        let src = [FORWARD, MODEL, RELIEF, t].concat();
        for bodies in [Bodies::Current, Bodies::EngineDefaults] {
            let s = splice(&src, &slots, bodies).unwrap();
            let m = validate("forward + model + relief", &s.source);
            assert!(entry(&m, "model_vs") && entry(&m, "model_fs") && entry(&m, "model_relief_fs"));
            // Uniforms: the frame, the model material (5 vec4), the
            // material's relief (2 vec4, model::relief::RELIEF_WORDS) and
            // the shared relief frame (1 vec4).
            let sizes: Vec<u32> = m
                .global_variables
                .iter()
                .filter(|(_, v)| v.space == naga::AddressSpace::Uniform)
                .map(|(_, v)| m.types[v.ty].inner.size(m.to_ctx()))
                .collect();
            assert_eq!(sizes[1..], [80, 32, 16], "uniform sizes {sizes:?}");
            assert_eq!(evani_render::model::relief::RELIEF_WORDS * 4, 32);
        }
    }
    // A pattern body that returns a real gradient splices and validates too
    // (the engine default returns zero).
    let mut toonish = ShaderSlots::with_builtins();
    toonish.declare(MICRO_DETAIL);
    toonish.set_default("micro_detail", "    return vec2<f32>(cos(6.2831853 * uv.x), sin(pattern.y));", "test").unwrap();
    let s = splice(&[FORWARD, MODEL, RELIEF, SHADOWS].concat(), &toonish, Bodies::Current).unwrap();
    validate("relief with a pattern body", &s.source);
    // What the build ships: the relief stage, still splicing and validating.
    let fwd = evani_render::shaders::FORWARD.source;
    assert!(fwd.contains("fn model_relief_fs("), "a relief build ships the relief fragment stage");
    let m = validate("shipped forward", &splice(fwd, &slots, Bodies::Current).unwrap().source);
    assert!(entry(&m, "model_relief_fs"));
    // Drift guard: after the normal, model_relief_fs shades exactly as
    // model_fs (light, shadow, tint, fog), so relief never changes the look.
    assert_eq!(tail(RELIEF, "model_relief_fs", "n = n * side;"), tail(MODEL, "model_fs", "n = n * side;"), "model_relief_fs no longer shades like model_fs after its normal");
    // Every map is sampled with the unshifted UV's gradients (no implicit
    // derivatives after the parallax loop or a discard).
    let fs = &RELIEF[RELIEF.find("fn relief_parallax(").unwrap()..];
    assert!(!fs.contains("textureSample("), "the relief stage samples only with textureSampleGrad");
}

/// Evaluates the shipped one-line WGSL function `sig` of model_relief.wgsl
/// (its body is only clamp and arithmetic) with its arguments set to `vars`,
/// with naga's compile-time evaluator, so the test runs the shader's own
/// numbers (the same evaluator as tests/model_wgsl.rs).
fn eval(sig: &str, vars: &[(&str, f32)]) -> f32 {
    let start = RELIEF.find(sig).unwrap_or_else(|| panic!("model_relief.wgsl has {sig}"));
    let body = &RELIEF[start..];
    let ret = body.find("return ").expect("the function returns an expression") + "return ".len();
    let end = body[ret..].find(';').expect("its return ends with ;") + ret;
    let expr = &body[ret..end];
    let mut out = String::new();
    let mut word = String::new();
    for ch in expr.chars().chain(std::iter::once(' ')) {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            word.push(ch);
            continue;
        }
        match vars.iter().find(|(n, _)| *n == word) {
            Some((_, v)) => out.push_str(&format!("({v:?})")),
            None => out.push_str(&word),
        }
        word.clear();
        out.push(ch);
    }
    let src = format!("const V: f32 = {};", out.trim());
    let m = validate(sig, &src);
    let (_, c) = m.constants.iter().next().expect("one constant");
    match m.global_expressions[c.init] {
        naga::Expression::Literal(naga::Literal::F32(v)) => v,
        ref e => panic!("{sig} {vars:?} did not evaluate to an f32 at compile time: {e:?}"),
    }
}

/// The micro pattern is gone where one period covers 1.5 px or less (it
/// would only shimmer), fully there from 3 px, and ramps in between; the
/// shader measures the period with the UV's screen derivatives and scales
/// the pattern's tilt by this fade.
#[test]
fn micro_detail_fades_below_threshold() {
    let fade = |px: f32| eval("fn micro_fade(px: f32) -> f32 {", &[("px", px)]);
    for px in [0.0, 0.5, 1.0, 1.4, 1.5] {
        assert_eq!(fade(px), 0.0, "micro_fade({px})");
    }
    for px in [3.0, 4.0, 10.0, 100.0] {
        assert_eq!(fade(px), 1.0, "micro_fade({px})");
    }
    let mut last = 0.0;
    for i in 0..=40 {
        let px = 1.0 + i as f32 * 0.1;
        let v = fade(px);
        assert!((0.0..=1.0).contains(&v) && v >= last, "micro_fade not a 0..1 ramp at {px}");
        if px > 1.55 && px < 2.95 {
            assert!(v > 0.0 && v < 1.0, "micro_fade({px}) = {v}: no soft ramp");
        }
        last = v;
    }
    // The shader's own wiring: the pixel size of a period comes from the
    // unshifted UV's derivatives, and the fade scales the tilt.
    let fs = &RELIEF[RELIEF.find("fn model_relief_fs(").unwrap()..];
    assert!(fs.contains("let gx = dpdx(uv0);") && fs.contains("let gy = dpdy(uv0);"));
    let px = fs.lines().find(|l| l.trim_start().starts_with("let px =")).expect("model_relief_fs measures px");
    assert!(px.contains("fw.x, fw.y") && px.contains("model_relief.micro.y"), "px line: {px}");
    let km = fs.lines().find(|l| l.trim_start().starts_with("let km =")).expect("model_relief_fs sets km");
    assert!(km.contains("micro_fade(px)"), "km line: {km}");
    // The pattern is evaluated (and tips the normal) only where km > 0, so a
    // faded-out pattern costs nothing past the fade itself.
    assert!(fs.contains("if (km > 0.0) {"));
    // The slope ink fades with the same km, so a faded pattern leaves the
    // colour untouched too.
    assert!(fs.contains("base.rgb * (1.0 - km * min(dot(gp, gp), 1.0))"));
}

/// Parallax occlusion marches the height map finely on High and Ultra; Lite,
/// Low, Medium and VR take the cheap 2-step march (the plan said none there;
/// the same-tier captures read clearly flatter without it, DECISIONS
/// "Surface relief"). The micro-detail and normal map do not depend on the
/// tier: the relief stage reads no tier value except the parallax step
/// count, so the surface's pattern and shading read the same on every
/// device.
#[test]
fn parallax_cheap_on_low_and_medium() {
    use evani_render::model::relief::parallax_steps;
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/tiers");
    let mut ids = Vec::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "json") {
            let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
            ids.push(v["id"].as_str().unwrap().to_string());
        }
    }
    ids.sort();
    assert_eq!(ids, ["high", "lite", "low", "medium", "ultra", "vr"], "a tier was added: decide its parallax here");
    for id in &ids {
        let steps = parallax_steps(id);
        match id.as_str() {
            "high" | "ultra" => assert!((4..=32).contains(&steps), "{id}: {steps} parallax steps"),
            _ => assert_eq!(steps, 2, "{id}: the cheap parallax (2 steps, 3 height samples)"),
        }
    }
    assert!(parallax_steps("ultra") >= parallax_steps("high"));
    assert!(parallax_steps("high") > parallax_steps("medium"));
    // The shader turns the march off at zero steps, and reads the tier only
    // there.
    let fs = &RELIEF[RELIEF.find("fn model_relief_fs(").unwrap()..];
    assert!(fs.contains("relief_frame.x > 0.5"));
    assert_eq!(fs.matches("relief_frame").count(), 2, "the relief stage reads the tier's frame word only for the parallax");
}
