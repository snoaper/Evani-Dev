//! S3-3: the forward WGSL validates with and without the model entry points
//! and with either shadow tail, with the engine's slot bodies spliced in;
//! the model shadow pass validates; and the build's own forward source has
//! the model code exactly when the `models` feature is on.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code: a failed read is a failed test

use evani_core::slots::ShaderSlots;
use evani_render::splice::{Bodies, splice};

const FORWARD: &str = include_str!("../shaders/forward.wgsl");
const MODEL: &str = include_str!("../shaders/model.wgsl");
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

#[test]
fn wgsl_validates_with_and_without_models() {
    #[cfg_attr(not(any(feature = "relief", feature = "pcss")), allow(unused_mut))]
    let mut slots = ShaderSlots::with_builtins();
    // A soft-shadow build's shipped source also calls the pcss_params slot
    // (the engine declares it at startup; the signature is copied from
    // shadow_ext::PCSS_PARAMS, which is private to the crate).
    #[cfg(feature = "pcss")]
    slots.declare(evani_core::slots::SlotSignature {
        name: "pcss_params",
        params: &[("i", "u32")],
        ret: "vec4<f32>",
        description: "Soft shadows: penumbra per metre, search radius, min and max filter radius (texels)",
        default_body: "    return vec4<f32>(0.02, 6.0, 1.0, 12.0);",
    });
    // A relief build's shipped source also calls the micro_detail slot.
    #[cfg(feature = "relief")]
    slots.declare(evani_core::slots::MICRO_DETAIL);
    // A lit surface build's shipped source also calls specular and
    // surface_params (S5-R5b).
    #[cfg(all(feature = "look-surface", not(feature = "relief")))]
    {
        slots.declare(evani_render::model::surface::SPECULAR);
        slots.declare(evani_render::model::surface::SURFACE_PARAMS);
    }
    for (with_model, tail) in [(false, NOSHADOW), (false, SHADOWS), (true, NOSHADOW), (true, SHADOWS)] {
        let src = [FORWARD, if with_model { MODEL } else { "" }, tail].concat();
        for bodies in [Bodies::Current, Bodies::EngineDefaults] {
            let s = splice(&src, &slots, bodies).unwrap();
            let m = validate("forward", &s.source);
            assert!(entry(&m, "vs_main") && entry(&m, "fs_main"));
            assert_eq!(entry(&m, "model_vs") && entry(&m, "model_fs"), with_model);
            // The model material uniform is 5 vec4 (model::MATERIAL_WORDS).
            let sizes: Vec<u32> = m
                .global_variables
                .iter()
                .filter(|(_, v)| v.space == naga::AddressSpace::Uniform)
                .map(|(_, v)| m.types[v.ty].inner.size(m.to_ctx()))
                .collect();
            assert_eq!(sizes.len(), 1 + usize::from(with_model));
            if with_model {
                assert_eq!(sizes[1], 80);
            }
        }
    }
    let shadows = [include_str!("../shaders/shadows_depth.wgsl"), include_str!("../shaders/model_shadows.wgsl")].concat();
    let m = validate("model_shadows", &shadows);
    assert!(entry(&m, "model_shadow_vs") && entry(&m, "vs_main"));
    // The build's forward source carries the model code only with `models`.
    let fwd = evani_render::shaders::FORWARD.source;
    assert_eq!(fwd.contains("evani.shader.model"), cfg!(feature = "models"));
    assert_eq!(fwd.contains("evani.shader.shadows_sample"), cfg!(feature = "shadows"));
    // What the build ships (the models build strips comments): it still
    // splices and validates.
    let s = splice(fwd, &slots, Bodies::Current).unwrap();
    let m = validate("shipped forward", &(s.source + &lit_runtime_stubs()));
    assert_eq!(entry(&m, "model_vs"), cfg!(feature = "models"));
    #[cfg(all(feature = "models", feature = "shadows"))]
    assert!(entry(&validate("shipped model_shadows", evani_render::shaders::MODEL_SHADOWS), "model_shadow_vs"));
}

/// Evaluates the shipped one-line WGSL function `sig` (its body is only
/// clamp and arithmetic) with its arguments set to `vars`, with naga's
/// compile-time evaluator, so a test runs the shader's own numbers, not a
/// Rust copy of them.
fn eval(sig: &str, vars: &[(&str, f32)]) -> f32 {
    let start = MODEL.find(sig).unwrap_or_else(|| panic!("model.wgsl has {sig}"));
    let body = &MODEL[start..];
    let ret = body.find("return ").expect("the function returns an expression") + "return ".len();
    let end = body[ret..].find(';').expect("its return ends with ;") + ret;
    let expr = &body[ret..end];
    // Swap each whole-word argument for its value.
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

fn self_shadow(gl: f32) -> f32 {
    eval("fn self_shadow(gl: f32) -> f32 {", &[("gl", gl)])
}

#[cfg(feature = "models")]
fn sun_cone(d: f32, t: f32) -> f32 {
    eval("fn sun_cone(d: f32, t: f32) -> f32 {", &[("d", d), ("t", t)])
}

/// Chris's rule (2026-10-16): the look on a phone or tablet is the look on
/// the PC. A normal map can tip N.L above zero on a face turned away from the
/// sun; the shadow map darkens that face on Medium and up, so Lite and Low
/// (no shadow map, `shadow_vis` = 1) must darken it too, or the front of a
/// coat lit from behind is bright on a phone and dark on a PC. The model
/// shader multiplies its light by `self_shadow(geometric N.L)` on every tier.
/// This checks that it does, that the term is 0 for a face turned away and 1
/// for one facing the sun, and that the ramp input and shadow tint it leads
/// to are then the same with and without the shadow map.
#[test]
fn models_shade_the_same_with_and_without_a_shadow_map() {
    // The term is in the light of every tier (not inside the shadow tail).
    let fs = &MODEL[MODEL.find("fn model_fs(").expect("model_fs")..];
    let vis = fs.lines().find(|l| l.trim_start().starts_with("let vis =")).expect("model_fs sets vis");
    assert!(vis.contains("cast_vis *") && vis.contains("self_shadow(dot(g, frame.light_dir.xyz))"), "model_fs vis line: {vis}");
    // `cast_vis` is the shadow map where there is one, else the baked cone
    // (models_cast_their_own_shadow_without_a_shadow_map).
    let cast = fs.lines().find(|l| l.trim_start().starts_with("let cast_vis =")).expect("model_fs sets cast_vis");
    assert!(cast.contains("select(sun_cone(dot(normalize(in.sun.xyz), frame.light_dir.xyz), in.sun.w), shadow_vis(in.world, g), frame.shadow.x > 0.5)"), "model_fs cast line: {cast}");
    assert!(!SHADOWS.contains("self_shadow") && !NOSHADOW.contains("self_shadow"));
    // The shipped term: dark turned away, full light facing, monotonic.
    for gl in [-1.0, -0.5, -0.2, -0.05] {
        assert_eq!(self_shadow(gl), 0.0, "self_shadow({gl})");
    }
    for gl in [0.05, 0.2, 0.5, 1.0] {
        assert_eq!(self_shadow(gl), 1.0, "self_shadow({gl})");
    }
    let mut last = 0.0;
    for i in -20..=20 {
        let v = self_shadow(i as f32 * 0.01);
        assert!((0.0..=1.0).contains(&v) && v >= last, "self_shadow not a 0..1 ramp at {}", i as f32 * 0.01);
        last = v;
    }
    // The coat case: geometric N.L -0.4 (turned away), normal-mapped N.L
    // +0.3. Shadow map on (it sees the face as occluded: 0) and off (1).
    // model_fs feeds the ramp mix(min(ndl, -0.1), ndl, vis) and tints by
    // 1 - vis: both must match.
    assert!(fs.contains("slot_toon_ramp(mix(min(ndl, -0.1), ndl, vis))") && fs.contains("clamp(1.0 - vis, 0.0, 1.0)"));
    let (ndl, term) = (0.3_f32, self_shadow(-0.4));
    let shade = |map_vis: f32| {
        let vis = map_vis * term;
        let ramp_in = ndl.min(-0.1) + (ndl - ndl.min(-0.1)) * vis;
        (ramp_in, 1.0 - vis)
    };
    assert_eq!(shade(0.0), shade(1.0), "Lite (no shadow map) and Ultra shade a face turned away from the sun differently");
}

/// The model's own shadow on itself, without a shadow map (Q1, 2026-10-17).
/// On Medium and up the shadow map darkens the parts of a model its own
/// body hides from the sun: with the sun behind and above, the front of the
/// marshal's coat. Lite and Low have no map, so before the baked cone those
/// parts stayed lit and the phone looked brighter than the PC. This casts
/// exact rays (what a shadow map gives) from vertices of the shipped
/// marshal and of a synthetic overhang (mirrored node) and checks that the
/// baked cone, read and evaluated the way model.wgsl does, lets the sun
/// through the same places.
#[cfg(feature = "models")]
#[test]
fn sun_cones_bake_only_where_they_are_read() {
    use evani_render::model::sun::pick;
    // (baked, waiting to show, drawn this frame), in load order.
    let models = [(true, false, true), (false, false, false), (false, false, true), (false, true, false)];
    // A frame with a shadow map spends nothing.
    assert_eq!(pick(false, models), None);
    // Without one: the first unbaked model that is drawn or waiting.
    assert_eq!(pick(true, models), Some(2));
    assert_eq!(pick(true, [(false, false, false), (false, true, false)]), Some(1));
    // Undrawn, shown models wait until they are drawn (a tier drop bakes
    // what is on screen first); baked ones never run again.
    assert_eq!(pick(true, [(false, false, false), (true, true, true)]), None);
}

#[cfg(feature = "models")]
#[test]
fn models_cast_their_own_shadow_without_a_shadow_map() {
    use evani_render::model::{Part, VERTEX_BYTES, prepare, sun};
    // The shipped cone term against a Rust mirror on a grid, then the
    // mirror in the loops below (thousands of points).
    let cone = |d: f32, t: f32| ((d - t) * 10.0 + 0.5).clamp(0.0, 1.0);
    for i in -4..=4 {
        for j in -4..=4 {
            let (d, t) = (i as f32 * 0.25, j as f32 * 0.25);
            assert!((sun_cone(d, t) - cone(d, t)).abs() < 1e-6, "sun_cone({d}, {t})");
        }
    }
    assert_eq!(sun_cone(1.0, -1.0), 1.0, "an unbaked vertex (cone length 0) is open sky");

    let marshal = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../apps/sandbox/assets/models/proof-figure.glb")).unwrap();
    let marshal = evani_gltf::Gltf::from_glb(&marshal).unwrap();
    let mut marshal = prepare(&marshal).unwrap().parts;
    // Loading bakes nothing (only the tiers without a shadow map read the
    // cones, and the GPU side bakes them there).
    assert!(marshal.iter().all(|p| p.vertices.as_chunks::<VERTEX_BYTES>().0.iter().all(|v| [v[6], v[7], v[11]] == [0; 3])), "prepare left a cone baked");
    // Spread over frames as the GPU side does it: the same bytes as in one
    // go, in a bounded number of frames, each of them a bounded share.
    let mut whole = marshal.clone();
    let t0 = std::time::Instant::now();
    sun::bake(&mut whole);
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    let mut bake = sun::Bake::new(&marshal);
    let (mut frames, mut slowest) = (0, 0.0f64);
    loop {
        frames += 1;
        let t = std::time::Instant::now();
        let done = bake.step(&mut marshal, sun::FRAME_UNITS);
        slowest = slowest.max(t.elapsed().as_secs_f64() * 1000.0);        if done {
            break;
        }
        assert!(frames < 1000, "the bake does not finish");
    }
    assert!(bake.done());
    assert!(marshal.iter().zip(&whole).all(|(a, b)| a.vertices == b.vertices), "a bake spread over frames wrote other bytes");
    println!("proof-figure: the bake takes {ms:.1} ms in one go, or {frames} frames of at most {slowest:.1} ms (this build's opt level)");
    assert!((10..=200).contains(&frames), "{frames} frames: FRAME_UNITS no longer splits the marshal's bake into about a second of frames");
    // Lazy hands the baked parts back once, the frame it finishes.
    let mut lazy = sun::Lazy::new(whole.clone());
    let mut back = None;
    for _ in 0..1000 {
        if let Some(p) = lazy.advance(sun::FRAME_UNITS) {
            back = Some(p);
            break;
        }
    }
    assert!(lazy.baked() && lazy.advance(sun::FRAME_UNITS).is_none());
    assert!(back.unwrap().iter().zip(&whole).all(|(a, b)| a.vertices == b.vertices));
    let mut overhang = overhang_parts();
    sun::bake(&mut overhang);

    let norm = |a: [f32; 3]| {
        let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
        a.map(|x| x / l)
    };
    // The sandbox sun (render.light.direction), as each of its four copies
    // sees it in model space (yaw, and the mirrored one), then more suns.
    let sandbox = norm([0.4, 1.0, 0.3]);
    let mut suns = Vec::new();
    for (yaw, mirror) in [(0.0f32, false), (core::f32::consts::FRAC_PI_2, true), (core::f32::consts::PI, false), (-core::f32::consts::FRAC_PI_2, false)] {
        let (s, c) = yaw.sin_cos();
        let x = sandbox[0] * c - sandbox[2] * s;
        let z = sandbox[0] * s + sandbox[2] * c;
        suns.push((format!("sandbox sun, copy at yaw {:.0} deg{}", yaw.to_degrees(), if mirror { " mirrored" } else { "" }), norm([if mirror { -x } else { x }, sandbox[1], z])));
    }
    for (name, d) in [("toon look sun", [0.62, 0.85, 0.28]), ("high sun", [0.1, 1.0, 0.05]), ("low sun, front", [0.2, 0.5, 1.0]), ("low sun, side", [1.0, 0.6, -0.2]), ("sun behind", [-0.3, 0.8, -0.7])] {
        suns.push((name.to_string(), norm(d)));
    }

    for (model, parts, every) in [("proof-figure", &marshal, 7usize), ("overhang", &overhang, 1)] {
        let spaces: Vec<_> = parts.iter().map(sun::model_space).collect();
        let tris: Vec<[[f32; 3]; 3]> = parts
            .iter()
            .zip(&spaces)
            .flat_map(|(p, s)| p.indices.as_chunks::<3>().0.iter().map(move |t| [s.0[t[0] as usize], s.0[t[1] as usize], s.0[t[2] as usize]]))
            .collect();
        let (lo, hi) = tris.iter().flatten().fold(([f32::MAX; 3], [f32::MIN; 3]), |(l, h), p| ([0, 1, 2].map(|k| l[k].min(p[k])), [0, 1, 2].map(|k| h[k].max(p[k]))));
        let size = (0..3).map(|k| hi[k] - lo[k]).fold(0.0f32, f32::max);
        // Every sampled vertex: model-space position, normal and the baked
        // cone turned like the normal (as model_vs does).
        let mut verts = Vec::new();
        for (p, (pos, nrm)) in parts.iter().zip(&spaces) {
            let m = p.node;
            let c = [[m[0], m[1], m[2]], [m[4], m[5], m[6]], [m[8], m[9], m[10]]];
            let x = |a: [f32; 3], b: [f32; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
            let x12 = x(c[1], c[2]);
            let s = if c[0][0] * x12[0] + c[0][1] * x12[1] + c[0][2] * x12[2] < 0.0 { -1.0 } else { 1.0 };
            let cof = [x12, x(c[2], c[0]), x(c[0], c[1])];
            for (i, v) in p.vertices.as_chunks::<VERTEX_BYTES>().0.iter().enumerate().step_by(every) {
                let (d, t) = sun::decode(v);
                let len = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
                let d = if len < 1e-6 { nrm[i] } else { norm([0, 1, 2].map(|r| (cof[0][r] * d[0] + cof[1][r] * d[1] + cof[2][r] * d[2]) * s)) };
                verts.push((pos[i], nrm[i], d, t));
            }
        }
        for (si, (name, l)) in suns.iter().enumerate() {
            let hit = Caster::new(&tris, *l);
            let (mut n, mut agree, mut exact_lit, mut baked_lit) = (0usize, 0usize, 0.0f32, 0.0f32);
            for (p, nr, d, t) in &verts {
                if nr[0] * l[0] + nr[1] * l[1] + nr[2] * l[2] <= 0.05 {
                    continue;
                }
                // The shadow map's normal offset and bias, about 1% of the model.
                let from = [0, 1, 2].map(|k| p[k] + nr[k] * size * 0.01 + l[k] * size * 0.005);
                let open = !hit.blocked(from);
                let baked = cone(d[0] * l[0] + d[1] * l[1] + d[2] * l[2], *t);
                n += 1;
                agree += usize::from(open == (baked > 0.5));
                exact_lit += f32::from(u8::from(open));
                baked_lit += baked;
            }
            assert!(n > 20, "{model}, {name}: only {n} vertices face the sun");
            let (agree, exact_lit, baked_lit) = (agree as f32 / n as f32, exact_lit / n as f32, baked_lit / n as f32);
            println!(
                "{model}, {name}: {n} sunlit-facing vertices; lit with a shadow map {:.1}%, without one before the bake 100.0%, with the baked cone {:.1}%; per-vertex agreement {:.1}%",
                exact_lit * 100.0,
                baked_lit * 100.0,
                agree * 100.0
            );
            // The bake's grid starts its rays a little further off the
            // surface than this cast, so about 1 vertex in 10 differs right
            // at a shadow's edge (a shadow map's own bias and filtering move
            // those edges too). What the eye sees is the lit share: within 5
            // points under the lights the engine ships (the sandbox and toon
            // suns), 10 under the extreme ones; before the bake it was 100%,
            // 35 to 65 points off.
            let tol = if si < 5 { 0.05 } else { 0.10 };
            assert!(agree >= 0.80, "{model}, {name}: the baked cone agrees with the cast shadow at only {:.1}% of vertices", agree * 100.0);
            assert!((exact_lit - baked_lit).abs() <= tol, "{model}, {name}: lit {:.1}% with a shadow map, {:.1}% baked (allowed {:.0} points)", exact_lit * 100.0, baked_lit * 100.0, tol * 100.0);
        }
    }

    /// A floor with a roof above part of it, under a mirrored node.
    fn overhang_parts() -> Vec<Part> {
        let mut pos = Vec::new();
        let mut idx = Vec::new();
        let n = 24u32;
        for j in 0..=n {
            for i in 0..=n {
                pos.push([-2.0 + 4.0 * i as f32 / n as f32, 0.0, -2.0 + 4.0 * j as f32 / n as f32]);
                if i < n && j < n {
                    let a = j * (n + 1) + i;
                    idx.extend([a, a + n + 1, a + 1, a + 1, a + n + 1, a + n + 2]);
                }
            }
        }
        let floor = part(&pos, [0.0, 1.0, 0.0], idx, [-1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
        let roof = part(
            &[[-0.2, 1.0, -0.8], [1.2, 1.0, -0.8], [-0.2, 1.0, 0.8], [1.2, 1.0, 0.8]],
            [0.0, -1.0, 0.0],
            vec![0, 1, 2, 2, 1, 3],
            [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
        );
        vec![floor, roof]
    }

    fn part(pos: &[[f32; 3]], n: [f32; 3], indices: Vec<u32>, node: [f32; 16]) -> Part {
        let lo = [0, 1, 2].map(|k| pos.iter().map(|p| p[k]).fold(f32::MAX, f32::min));
        let ext = [0, 1, 2].map(|k| pos.iter().map(|p| p[k]).fold(f32::MIN, f32::max) - lo[k]);
        let mut vertices = vec![0u8; pos.len() * VERTEX_BYTES];
        for (p, v) in pos.iter().zip(vertices.as_chunks_mut::<VERTEX_BYTES>().0.iter_mut()) {
            for k in 0..3 {
                let q = if ext[k] > 0.0 { ((p[k] - lo[k]) / ext[k] * 65535.0).round() as u16 } else { 0 };
                v[k * 2..k * 2 + 2].copy_from_slice(&q.to_le_bytes());
                v[8 + k] = (n[k] * 127.0).round() as i8 as u8;
            }
        }
        Part { vertices, indices, node, pos_scale: ext, pos_offset: lo, uv: [0.0; 4], material: 0, tangents: false, source: [0; 3], lod: evani_render::model::SHARED, tags: 0, #[cfg(feature = "skinning")] skin: Vec::new() }
    }
}

/// Found by the S3-6 quality gate (2026-10-17): a kart model with LOD levels
/// scored SSIMULACRA2 -3 on Low against its master, with dark blotches. Only
/// one LOD level is drawn at a time, but the bake used every part of every
/// level as an occluder, so the hidden coarser level shaded the drawn one.
/// The rule: a part's occluders are the parts drawn with it (the shared
/// parts plus its own level's), and a shared part is baked against the shared
/// parts plus the lowest level present (LOD0). So a hidden level must change
/// nothing about the parts it is not drawn with: here a LOD1 roof hangs over
/// the floor (shared) and a LOD0 plate, and adding it must leave their cones
/// byte for byte as they are without it; the roof itself is shaded as if the
/// LOD0 plate were not there. The bake spread over frames writes the same
/// bytes as in one go, with several levels too.
#[cfg(feature = "models")]
#[test]
fn hidden_lod_levels_do_not_shade_the_drawn_one() {
    use evani_render::model::{Part, SHARED, VERTEX_BYTES, sun};

    // A flat n x n quad grid at height y over x0..x1, z0..z1 with a normal.
    fn quad(y: f32, (x0, x1): (f32, f32), (z0, z1): (f32, f32), n: [f32; 3], lod: u8) -> Part {
        let (mut pos, mut indices) = (Vec::new(), Vec::new());
        let s = 10u32;
        for j in 0..=s {
            for i in 0..=s {
                pos.push([x0 + (x1 - x0) * i as f32 / s as f32, y, z0 + (z1 - z0) * j as f32 / s as f32]);
                if i < s && j < s {
                    let a = j * (s + 1) + i;
                    indices.extend([a, a + s + 1, a + 1, a + 1, a + s + 1, a + s + 2]);
                }
            }
        }
        let lo = [0, 1, 2].map(|k| pos.iter().map(|p| p[k]).fold(f32::MAX, f32::min));
        let ext = [0, 1, 2].map(|k| pos.iter().map(|p| p[k]).fold(f32::MIN, f32::max) - lo[k]);
        let mut vertices = vec![0u8; pos.len() * VERTEX_BYTES];
        for (p, v) in pos.iter().zip(vertices.as_chunks_mut::<VERTEX_BYTES>().0.iter_mut()) {
            for k in 0..3 {
                let q = if ext[k] > 0.0 { ((p[k] - lo[k]) / ext[k] * 65535.0).round() as u16 } else { 0 };
                v[k * 2..k * 2 + 2].copy_from_slice(&q.to_le_bytes());
                v[8 + k] = (n[k] * 127.0).round() as i8 as u8;
            }
        }
        let node = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
        Part { vertices, indices, node, pos_scale: ext, pos_offset: lo, uv: [0.0; 4], material: 0, tangents: false, source: [0; 3], lod, tags: 0, #[cfg(feature = "skinning")] skin: Vec::new() }
    }
    let (up, down) = ([0.0, 1.0, 0.0], [0.0, -1.0, 0.0]);
    // Shared: the floor, and a lid high in one corner (together they fix the
    // box, so every variant below has the same grid). LOD0: a plate low in
    // the other corner. LOD1: a roof over the middle of the floor.
    let floor = quad(0.0, (-2.0, 2.0), (-2.0, 2.0), up, SHARED);
    let lid = quad(2.0, (1.0, 2.0), (1.0, 2.0), down, SHARED);
    let plate = quad(1.0, (-2.0, -1.0), (-2.0, -1.0), up, 0);
    let roof = quad(1.0, (-1.0, 1.0), (-1.0, 1.0), down, 1);
    let baked = |parts: &[&Part]| {
        let mut v: Vec<Part> = parts.iter().map(|p| (*p).clone()).collect();
        sun::bake(&mut v);
        v.into_iter().map(|p| p.vertices).collect::<Vec<_>>()
    };

    let with_both = baked(&[&floor, &lid, &plate, &roof]);
    let without_lod1 = baked(&[&floor, &lid, &plate]);
    let without_lod0 = baked(&[&floor, &lid, &roof]);
    assert!(with_both[0] == without_lod1[0], "a hidden LOD1 roof changed the cones of the shared floor");
    assert!(with_both[1] == without_lod1[1], "a hidden LOD1 roof changed the cones of the shared lid");
    assert!(with_both[2] == without_lod1[2], "a hidden LOD1 roof changed the cones of the LOD0 plate");
    assert!(with_both[3] == without_lod0[2], "the LOD1 roof is shaded by the hidden LOD0 plate");
    // Not vacuous: with every part in one set, as before the fix, the roof
    // does shade the floor (and the cones are not all open).
    let (mut f, mut l, mut p, mut r) = (floor.clone(), lid.clone(), plate.clone(), roof.clone());
    (f.lod, l.lod, p.lod, r.lod) = (SHARED, SHARED, SHARED, SHARED);
    let one_set = baked(&[&f, &l, &p, &r]);
    assert!(one_set[0] != without_lod1[0], "the roof no longer shades the floor when every part is drawn together: the test proves nothing");
    let blocked = |v: &[u8]| v.as_chunks::<VERTEX_BYTES>().0.iter().filter(|v| [v[6], v[7], v[11]] != [0; 3]).count();
    assert!(blocked(&one_set[0]) > blocked(&without_lod1[0]), "the roof did not darken the floor in the one-set bake");

    // Spread over frames with several sets: the same bytes as in one go.
    let mut parts: Vec<Part> = [&floor, &lid, &plate, &roof].iter().map(|p| (*p).clone()).collect();
    let mut bake = sun::Bake::new(&parts);
    for frames in 0.. {
        if bake.step(&mut parts, sun::FRAME_UNITS / 8) {
            break;
        }
        assert!(frames < 1000, "the bake of a model with levels does not finish");
    }
    assert!(parts.iter().map(|p| &p.vertices).eq(&with_both), "a bake with levels spread over frames wrote other bytes");
}

/// The shipped suns: the sandbox's (render.light.direction) and the toon
/// look's, then a low one in front and one behind.
#[cfg(feature = "models")]
const SUNS: [(&str, [f32; 3]); 4] = [("sandbox sun", [0.4, 1.0, 0.3]), ("toon look sun", [0.62, 0.85, 0.28]), ("low sun, front", [0.2, 0.5, 1.0]), ("sun behind", [-0.3, 0.8, -0.7])];

/// Found by the S3-10 relief proof (2026-10-17): on Lite and Low a large
/// diagonal shade lay across the relief tile's 2 m wall and floor in full
/// sun, which Medium and up (shadow map) drew lit. Cause: a vertex where
/// another surface meets its face (the wall's foot on the floor, the floor's
/// back edge at the wall) starts its rays inside that surface's grid cells,
/// so its cone sees almost no sky; the GPU blends the three corner cones
/// across a triangle, so on a 2 m face that one corner shaded metres of it,
/// along the triangle's diagonal. A corner of a triangle over `COARSE` cells
/// now takes its cone from votes inside the triangle (sun.rs module notes).
/// This draws the tile as the GPU does (cones interpolated over every
/// triangle) against exact casts at the same points: under the suns where
/// the shadow map lights the whole tile, the baked cone must too; under a
/// sun behind the wall, whose real shadow covers most of the floor, the lit
/// share stays within 10 points (a corner's cone cannot draw a sharp shadow
/// edge across the middle of a 2 m face; the shade must still be there).
#[cfg(feature = "models")]
#[test]
fn big_faces_get_no_diagonal_sun_smear() {
    use evani_render::model::{prepare, sun};
    let glb = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/models/relief-tile.glb")).unwrap();
    let mut parts = prepare(&evani_gltf::Gltf::from_glb(&glb).unwrap()).unwrap().parts;
    sun::bake(&mut parts);
    for (name, l) in SUNS {
        let r = smear::report(&parts, smear::norm(l));
        println!("relief tile, {name}: {r}");
        if name == "sun behind" {
            assert!(r.exact_lit < 0.9, "the wall no longer shades the floor under the sun behind: the check proves nothing");
            assert!((r.exact_lit - r.baked_lit).abs() <= 0.10, "relief tile, {name}: lit {:.1}% with a shadow map, {:.1}% baked", r.exact_lit * 100.0, r.baked_lit * 100.0);
        } else {
            assert_eq!(r.exact_lit, 1.0, "relief tile, {name}: the shadow map should light every sunlit face");
            assert!(r.baked_lit >= 0.99, "relief tile, {name}: the baked cone shades {:.1}% of a tile the shadow map lights whole", (1.0 - r.baked_lit) * 100.0);
            assert_eq!(r.smeared, 0.0, "relief tile, {name}: a big face the shadow map lights whole is drawn part dark");
        }
    }
}

/// Found by the S3-12 quality gate (2026-10-20): the look fixture's master
/// body is 3 parts and its look-matched tier file 1 part. The big-triangle
/// votes were grouped by vertex index, so a point carried by two parts heard
/// only its own part's triangles, and the master (re-welded for the gate's
/// reference, so each part keeps only its own vertices, packed to 16 bits
/// over its own box) baked other shade than the tier file: the best rung
/// scored SSIMULACRA2 30.3 and the tier file -18.0 against a bar of 90.
/// How a model's triangles are split over parts must not change its shade:
/// halves sharing one vertex buffer bake every cone byte for byte as the
/// whole, and halves that each keep and pack only their own vertices change
/// under 1% of the sunlit vertices' shade under any sun.
#[cfg(feature = "models")]
#[test]
fn bake_does_not_depend_on_how_a_model_is_split() {
    use evani_render::model::{Part, VERTEX_BYTES, prepare, sun};
    for file in ["relief-tile.glb", "look-match.low.glb"] {
        let glb = std::fs::read(format!("{}/../../tests/fixtures/models/{file}", env!("CARGO_MANIFEST_DIR"))).unwrap();
        let parts = prepare(&evani_gltf::Gltf::from_glb(&glb).unwrap()).unwrap().parts;
        let mut whole = parts.clone();
        sun::bake(&mut whole);
        let halves = |p: &Part| {
            let cut = p.indices.len() / 6 * 3;
            [0..cut, cut..p.indices.len()]
        };
        let mut shared = Vec::new();
        for p in &parts {
            for half in halves(p) {
                let mut h = p.clone();
                h.indices = p.indices[half].to_vec();
                shared.push(h);
            }
        }
        sun::bake(&mut shared);
        for (i, w) in whole.iter().enumerate() {
            for h in &shared[2 * i..2 * i + 2] {
                assert!(h.vertices == w.vertices, "{file} part {i}: a half carrying the whole vertex buffer bakes other cones than the unsplit part");
            }
        }

        // Each half keeps only the vertices it uses, packed over its own box, as
        // a weld leaves a glTF primitive; `from[k]` is the whole model's vertex.
        let (mut packed, mut from) = (Vec::new(), Vec::new());
        for (pi, p) in parts.iter().enumerate() {
            let src = p.vertices.as_chunks::<VERTEX_BYTES>().0;
            let local = |v: &[u8; VERTEX_BYTES]| [0, 1, 2].map(|k| p.pos_offset[k] + p.pos_scale[k] * f32::from(u16::from_le_bytes([v[k * 2], v[k * 2 + 1]])) / 65535.0);
            for half in halves(p) {
                let mut used: Vec<u32> = p.indices[half.clone()].to_vec();
                used.sort_unstable();
                used.dedup();
                let (lo, hi) = used.iter().map(|i| local(&src[*i as usize])).fold(([f32::MAX; 3], [f32::MIN; 3]), |(l, h), q| ([0, 1, 2].map(|k| l[k].min(q[k])), [0, 1, 2].map(|k| h[k].max(q[k]))));
                let mut h = p.clone();
                h.pos_offset = lo;
                h.pos_scale = [0, 1, 2].map(|k| hi[k] - lo[k]);
                h.vertices = used
                    .iter()
                    .flat_map(|i| {
                        let mut v = src[*i as usize];
                        let l = local(&v);
                        for k in 0..3 {
                            let q = if h.pos_scale[k] > 0.0 { ((l[k] - lo[k]) / h.pos_scale[k] * 65535.0).round() as u16 } else { 0 };
                            [v[k * 2], v[k * 2 + 1]] = q.to_le_bytes();
                        }
                        v
                    })
                    .collect();
                h.indices = p.indices[half].iter().map(|i| used.binary_search(i).unwrap() as u32).collect();
                let base: usize = parts[..pi].iter().map(|q| q.vertices.len() / VERTEX_BYTES).sum();
                from.extend(used.iter().map(|i| base + *i as usize));
                packed.push(h);
            }
        }
        sun::bake(&mut packed);
        let decoded = |ps: &[Part]| -> Vec<([f32; 3], f32, [f32; 3])> {
            ps.iter()
                .flat_map(|p| {
                    let m = p.node;
                    let c = [[m[0], m[1], m[2]], [m[4], m[5], m[6]], [m[8], m[9], m[10]]];
                    let n = sun::model_space(p).1;
                    p.vertices.as_chunks::<VERTEX_BYTES>().0.iter().zip(n).map(move |(v, n)| {
                        let (d, t) = sun::decode(v);
                        (smear::norm([0, 1, 2].map(|r| c[0][r] * d[0] + c[1][r] * d[1] + c[2][r] * d[2])), t, n)
                    })
                })
                .collect()
        };
        let (a, b) = (decoded(&whole), decoded(&packed));
        let lit = |(w, t, _): &([f32; 3], f32, [f32; 3]), l: [f32; 3]| *t <= -1.0 || smear::cone(w[0] * l[0] + w[1] * l[1] + w[2] * l[2], *t) > 0.5;
        for (name, l) in SUNS {
            let l = smear::norm(l);
            let facing: Vec<usize> = (0..b.len()).filter(|&k| b[k].2[0] * l[0] + b[k].2[1] * l[1] + b[k].2[2] * l[2] > 0.05).collect();
            let flipped = facing.iter().filter(|&&k| lit(&a[from[k]], l) != lit(&b[k], l)).count();
            println!("{file} in packed halves, {name}: {flipped} of {} sunlit vertices change their shade", facing.len());
            assert!(flipped * 100 <= facing.len(), "{file}, {name}: {flipped} of {} sunlit vertices change their baked shade when the model is split into halves that each pack their own vertices", facing.len());
        }
    }
}

/// The same artefact on any model: every model the project ships or tests
/// with (`apps/*/assets/models`, `tests/fixtures/models`), baked and drawn
/// as the GPU draws it, against exact casts. Under the shipped suns, no big
/// triangle (an edge over 8 of the 64 grid cells) that the shadow map lights
/// whole may be drawn more than a quarter dark (more than 1% of the sunlit
/// area in all fails), and the lit share stays within 6 points of the shadow
/// map's.
#[cfg(feature = "models")]
#[test]
fn baked_sun_draws_no_smear_on_any_model() {
    use evani_render::model::{Part, SHARED, prepare, sun};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    for dir in std::fs::read_dir(root.join("apps")).unwrap().flatten().map(|d| d.path().join("assets/models")).chain([root.join("tests/fixtures/models")]) {
        for f in std::fs::read_dir(&dir).into_iter().flatten().flatten().map(|f| f.path()) {
            if f.extension().is_some_and(|e| e == "glb") {
                files.push(f);
            }
        }
    }
    files.sort();
    let mut checked = 0;
    for f in &files {
        let name = f.file_name().unwrap().to_string_lossy();
        // A file the engine refuses is the converter's and the check's
        // business (`cargo xtask check`), not this one's.
        let Ok(g) = evani_gltf::Gltf::from_glb(&std::fs::read(f).unwrap()) else { continue };
        let Ok(p) = prepare(&g) else { continue };
        let mut parts = p.parts;
        if parts.iter().all(|p| p.indices.is_empty()) {
            continue;
        }
        sun::bake(&mut parts);
        checked += 1;
        // Each LOD level as it is drawn: the shared parts plus that level's,
        // never a hidden level's (no pass draws it, so the shadow map never
        // holds it). Cast against every level at once, look-match.low.glb's
        // coarse level read as 7 points shadowed by its own fine level lying
        // over it (2026-10-20).
        let mut levels: Vec<u8> = parts.iter().map(|p| p.lod).filter(|l| *l != SHARED).collect();
        levels.sort_unstable();
        levels.dedup();
        let shown: Vec<Option<u8>> = if levels.is_empty() { vec![None] } else { levels.into_iter().map(Some).collect() };
        for level in shown {
            let drawn: Vec<Part> = parts.iter().filter(|p| level.is_none_or(|l| p.lod == SHARED || p.lod == l)).cloned().collect();
            let at = level.map_or(String::new(), |l| format!(" level {l}"));
            for (sun_name, l) in &SUNS[..2] {
                let r = smear::report(&drawn, smear::norm(*l));
                println!("{name}{at}, {sun_name}: {r}");
                assert!(r.smeared <= 0.01, "{name}{at}, {sun_name}: {:.1}% of the sunlit area is big triangles the shadow map lights whole but the baked cone draws part dark (a smear)", r.smeared * 100.0);
                assert!((r.exact_lit - r.baked_lit).abs() <= 0.06, "{name}{at}, {sun_name}: lit {:.1}% with a shadow map, {:.1}% baked", r.exact_lit * 100.0, r.baked_lit * 100.0);
            }
        }
    }
    assert!(checked >= 3, "only {checked} models checked: the files moved?");
}

/// Found by the quality gate (S3-6, 2026-10-17): kart_fixture's converted
/// file shaded differently from its master on Lite and Low at the best
/// setting (SSIMULACRA2 4.8 against a bar of 90), though its geometry was the
/// master's to 16 bits. Its hidden simplified level reached 1 cm further out
/// than the drawn one, and the bake's cells sat on the whole model's box, so
/// every cell moved a fifth of a cell and the corners whose rays start near
/// another surface flipped (2 to 2.3% of the sunlit area). Packing a model
/// must not move its baked shade: a hidden level that widens the box leaves
/// the drawn level's cones byte for byte, and positions moved by one step of
/// their 16 bits change under 1% of the sunlit vertices under any sun.
#[cfg(feature = "models")]
#[test]
fn bake_is_stable_under_position_quantisation() {
    use evani_render::model::{Part, VERTEX_BYTES, prepare, sun};
    let glb = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/models/kart.low.glb")).unwrap();
    let parts = prepare(&evani_gltf::Gltf::from_glb(&glb).unwrap()).unwrap().parts;
    let cones = |p: &[Part]| -> Vec<[u8; 3]> { p.iter().flat_map(|p| p.vertices.as_chunks::<VERTEX_BYTES>().0.iter().map(|v| [v[6], v[7], v[11]])).collect() };
    let mut base = parts.clone();
    sun::bake(&mut base);
    let base_cones = cones(&base);

    // A hidden level a fifth of a cell (64 along the longest side) wider in x:
    // a copy of a rear wheel, moved out.
    let (lo, hi) = parts.iter().flat_map(|p| sun::model_space(p).0).fold(([f32::MAX; 3], [f32::MIN; 3]), |(l, h), q| ([0, 1, 2].map(|k| l[k].min(q[k])), [0, 1, 2].map(|k| h[k].max(q[k]))));
    let ext = (0..3).map(|k| hi[k] - lo[k]).fold(0.0f32, f32::max);
    let mut levels: Vec<Part> = parts.iter().cloned().map(|mut p| {
        p.lod = 0;
        p
    }).collect();
    let (far, _) = parts.iter().enumerate().map(|(i, p)| (i, sun::model_space(p).0.iter().fold(f32::MIN, |m, q| m.max(q[0])))).fold((0, f32::MIN), |b, x| if x.1 > b.1 { x } else { b });
    let mut wider = parts[far].clone();
    wider.lod = 1;
    wider.node[12] += 0.2 * ext / 64.0;
    levels.push(wider);
    sun::bake(&mut levels);
    let drawn = cones(&levels[..parts.len()]);
    let moved = drawn.iter().zip(&base_cones).filter(|(a, b)| a != b).count();
    assert_eq!(moved, 0, "a hidden level a fifth of a cell wider changed {moved} of {} cones of the drawn level: the bake's cells follow the box", base_cones.len());

    // Positions one 16-bit step off, as a repacking leaves them.
    let mut jittered = parts.clone();
    let mut i = 0u32;
    for p in &mut jittered {
        for v in p.vertices.as_chunks_mut::<VERTEX_BYTES>().0 {
            for k in 0..3 {
                i += 1;
                let q = i32::from(u16::from_le_bytes([v[k * 2], v[k * 2 + 1]])) + (i * 7 % 3) as i32 - 1;
                [v[k * 2], v[k * 2 + 1]] = (q.clamp(0, 65535) as u16).to_le_bytes();
            }
        }
    }
    sun::bake(&mut jittered);
    let normals: Vec<[f32; 3]> = base.iter().flat_map(|p| sun::model_space(p).1).collect();
    let lit = |p: &[Part], l: [f32; 3]| -> Vec<bool> {
        p.iter()
            .flat_map(|p| {
                let m = p.node;
                let c = [[m[0], m[1], m[2]], [m[4], m[5], m[6]], [m[8], m[9], m[10]]];
                p.vertices.as_chunks::<VERTEX_BYTES>().0.iter().map(move |v| {
                    let (d, t) = sun::decode(v);
                    // The part's nodes are rotations and scales: turned like the shader does, up to length.
                    let w = smear::norm([0, 1, 2].map(|r| c[0][r] * d[0] + c[1][r] * d[1] + c[2][r] * d[2]));
                    t <= -1.0 || smear::cone(w[0] * l[0] + w[1] * l[1] + w[2] * l[2], t) > 0.5
                })
            })
            .collect()
    };
    for (name, l) in SUNS {
        let l = smear::norm(l);
        let (a, b) = (lit(&base, l), lit(&jittered, l));
        let facing: Vec<usize> = (0..a.len()).filter(|&i| normals[i][0] * l[0] + normals[i][1] * l[1] + normals[i][2] * l[2] > 0.05).collect();
        let flipped = facing.iter().filter(|&&i| a[i] != b[i]).count();
        println!("kart, {name}: {flipped} of {} sunlit vertices change with positions one 16-bit step off", facing.len());
        assert!(flipped * 100 <= facing.len(), "kart, {name}: {flipped} of {} sunlit vertices change their baked shade with positions one 16-bit step off", facing.len());
    }
}

/// The baked cone over whole triangles, as the GPU draws it, against exact
/// shadow casts at the same points.
#[cfg(feature = "models")]
mod smear {
    use evani_render::model::{Part, VERTEX_BYTES, sun};

    /// A big triangle has an edge over this many grid cells (64 along the
    /// model's longest side): an eighth of the model, twice sun.rs's
    /// `COARSE`.
    const BIG: f32 = 8.0;

    pub fn norm(a: [f32; 3]) -> [f32; 3] {
        let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
        a.map(|x| x / l)
    }

    /// model.wgsl's `sun_cone`.
    pub fn cone(d: f32, t: f32) -> f32 {
        ((d - t) * 10.0 + 0.5).clamp(0.0, 1.0)
    }

    /// What one sun shows over a model's sunlit faces, by area.
    #[derive(Debug)]
    pub struct Report {
        /// Share of the sunlit-facing area the shadow map lights.
        pub exact_lit: f32,
        /// The same share as the baked cone, interpolated, lights it.
        pub baked_lit: f32,
        /// Share of that area where the two disagree (lit vs shadowed).
        pub wrong: f32,
        /// Share of it the baked cone shades where the map lights it.
        pub false_dark: f32,
        /// Share of it in big triangles (an edge over [`BIG`] grid cells)
        /// the shadow map lights whole but the baked cone draws over a
        /// quarter dark: the diagonal smear.
        pub smeared: f32,
    }

    impl core::fmt::Display for Report {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            let p = |v: f32| v * 100.0;
            write!(
                f,
                "lit {:.1}% with a shadow map, {:.1}% baked; they disagree on {:.1}% ({:.1}% baked dark but lit), smeared big faces {:.1}%",
                p(self.exact_lit),
                p(self.baked_lit),
                p(self.wrong),
                p(self.false_dark),
                p(self.smeared)
            )
        }
    }

    /// Every triangle is sampled at the centres of an n x n split of it (n
    /// from its size, about one sample per 1/64 of the model), each sample
    /// weighted by its share of the area. The cone is read and interpolated
    /// the way model_vs and model_fs do (the vertex's turned cone, or its
    /// normal when the cone has length 0, plus the threshold, interpolated
    /// linearly, then normalised).
    pub fn report(parts: &[Part], l: [f32; 3]) -> Report {
        let spaces: Vec<_> = parts.iter().map(sun::model_space).collect();
        let tris: Vec<[[f32; 3]; 3]> = parts
            .iter()
            .zip(&spaces)
            .flat_map(|(p, s)| p.indices.as_chunks::<3>().0.iter().map(move |t| [s.0[t[0] as usize], s.0[t[1] as usize], s.0[t[2] as usize]]))
            .collect();
        let (lo, hi) = tris.iter().flatten().fold(([f32::MAX; 3], [f32::MIN; 3]), |(l, h), p| ([0, 1, 2].map(|k| l[k].min(p[k])), [0, 1, 2].map(|k| h[k].max(p[k]))));
        let size = (0..3).map(|k| hi[k] - lo[k]).fold(0.0f32, f32::max);
        let hit = super::Caster::new(&tris, l);
        let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
        let x = |a: [f32; 3], b: [f32; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
        let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let (mut total, mut exact_lit, mut baked_lit, mut wrong, mut false_dark, mut smeared) = (0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64);
        for (p, (pos, nrm)) in parts.iter().zip(&spaces) {
            let m = p.node;
            let c = [[m[0], m[1], m[2]], [m[4], m[5], m[6]], [m[8], m[9], m[10]]];
            let x12 = x(c[1], c[2]);
            let s = if dot(c[0], x12) < 0.0 { -1.0 } else { 1.0 };
            let cof = [x12, x(c[2], c[0]), x(c[0], c[1])];
            let verts = p.vertices.as_chunks::<VERTEX_BYTES>().0;
            // model_vs's `sun` output, in model space.
            let out = |i: usize| -> [f32; 4] {
                let v = &verts[i];
                let cv = [v[6], v[7], v[11]].map(|b| f32::from(b as i8) / 127.0);
                let cl = dot(cv, cv).sqrt();
                let local_n = [0, 1, 2].map(|k| f32::from(v[8 + k] as i8) / 127.0);
                let cd = if cl < 0.01 { local_n } else { cv };
                let w = [0, 1, 2].map(|r| (cof[0][r] * cd[0] + cof[1][r] * cd[1] + cof[2][r] * cd[2]) * s);
                [w[0], w[1], w[2], 2.0 * cl - 1.0]
            };
            for t in p.indices.as_chunks::<3>().0 {
                let ix = t.map(|i| i as usize);
                let [a, b, cc] = ix.map(|i| pos[i]);
                let cr = x(sub(b, a), sub(cc, a));
                let area = 0.5 * dot(cr, cr).sqrt();
                if area <= 0.0 {
                    continue;
                }
                let o = ix.map(out);
                let edge = [dot(sub(b, a), sub(b, a)), dot(sub(cc, b), sub(cc, b)), dot(sub(a, cc), sub(a, cc))].into_iter().fold(0.0f32, f32::max).sqrt();
                let n = ((edge / (size / 64.0)).ceil() as usize).clamp(1, 32);
                let w = f64::from(area) / (n * n) as f64;
                // This triangle's samples: turned to the sun, shadow map
                // open, baked dark.
                let (mut facing, mut all_open, mut dark) = (0usize, true, 0usize);
                for i in 0..n {
                    for j in 0..n - i {
                        // Up to two small triangles per cell: (i, j) up,
                        // and (i, j) down when it fits.
                        for (bu, bv) in [((i as f32 + 1.0 / 3.0) / n as f32, (j as f32 + 1.0 / 3.0) / n as f32), ((i as f32 + 2.0 / 3.0) / n as f32, (j as f32 + 2.0 / 3.0) / n as f32)] {
                            if bu + bv > 1.0 {
                                continue;
                            }
                            let bw = [1.0 - bu - bv, bu, bv];
                            let p = [0, 1, 2].map(|k| a[k] * bw[0] + b[k] * bw[1] + cc[k] * bw[2]);
                            let pn = norm([0, 1, 2].map(|k| nrm[ix[0]][k] * bw[0] + nrm[ix[1]][k] * bw[1] + nrm[ix[2]][k] * bw[2]));
                            // Only where the shading normal turns to the sun
                            // (elsewhere self_shadow darkens it on every
                            // tier). The normal, not the winding: a model
                            // may wind its triangles either way.
                            if dot(pn, l) <= 0.05 {
                                continue;
                            }
                            facing += 1;
                            let sv = [0, 1, 2, 3].map(|k| o[0][k] * bw[0] + o[1][k] * bw[1] + o[2][k] * bw[2]);
                            let baked = cone(dot(norm([sv[0], sv[1], sv[2]]), l), sv[3]) > 0.5;
                            let from = [0, 1, 2].map(|k| p[k] + pn[k] * size * 0.01 + l[k] * size * 0.005);
                            let open = !hit.blocked(from);
                            total += w;
                            exact_lit += w * f64::from(u8::from(open));
                            baked_lit += w * f64::from(u8::from(baked));
                            wrong += w * f64::from(u8::from(open != baked));
                            false_dark += w * f64::from(u8::from(open && !baked));
                            all_open &= open;
                            dark += usize::from(!baked);
                        }
                    }
                }
                // Big is an edge over BIG cells: on smaller triangles a dark
                // corner spreads no further than the bake's own detail (rays
                // start 2 cells off the surface, the fix starts at 4), the
                // cone's ordinary per-vertex error, which `wrong` and the lit
                // share bound count.
                if edge > BIG * size / 64.0 && facing > 0 && all_open && dark * 4 > facing {
                    smeared += w * facing as f64;
                }
            }
        }
        let f = |v: f64| (v / total.max(1e-12)) as f32;
        Report { exact_lit: f(exact_lit), baked_lit: f(baked_lit), wrong: f(wrong), false_dark: f(false_dark), smeared: f(smeared) }
    }
}

/// Exact ray casts towards one light direction: triangles binned on a
/// plane across the light, Moller-Trumbore within the ray's bin.
#[cfg(feature = "models")]
struct Caster<'a> {
    tris: &'a [[[f32; 3]; 3]],
    l: [f32; 3],
    u: [f32; 3],
    v: [f32; 3],
    lo: [f32; 2],
    cell: f32,
    bins: Vec<Vec<u32>>,
}

#[cfg(feature = "models")]
impl<'a> Caster<'a> {
    const N: usize = 64;

    fn new(tris: &'a [[[f32; 3]; 3]], l: [f32; 3]) -> Self {
        let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let a = if l[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
        let u = [l[1] * a[2] - l[2] * a[1], l[2] * a[0] - l[0] * a[2], l[0] * a[1] - l[1] * a[0]];
        let ul = dot(u, u).sqrt();
        let u = u.map(|x| x / ul);
        let v = [l[1] * u[2] - l[2] * u[1], l[2] * u[0] - l[0] * u[2], l[0] * u[1] - l[1] * u[0]];
        let flat: Vec<[[f32; 2]; 3]> = tris.iter().map(|t| t.map(|p| [dot(p, u), dot(p, v)])).collect();
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for p in flat.iter().flatten() {
            for k in 0..2 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        let cell = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1e-6) / Self::N as f32 * 1.0001;
        let mut bins = vec![Vec::new(); Self::N * Self::N];
        let at = |x: f32, k: usize| (((x - lo[k]) / cell).floor().max(0.0) as usize).min(Self::N - 1);
        for (i, t) in flat.iter().enumerate() {
            let (x0, x1) = (at(t.iter().map(|p| p[0]).fold(f32::MAX, f32::min), 0), at(t.iter().map(|p| p[0]).fold(f32::MIN, f32::max), 0));
            let (y0, y1) = (at(t.iter().map(|p| p[1]).fold(f32::MAX, f32::min), 1), at(t.iter().map(|p| p[1]).fold(f32::MIN, f32::max), 1));
            for y in y0..=y1 {
                for x in x0..=x1 {
                    bins[y * Self::N + x].push(i as u32);
                }
            }
        }
        Self { tris, l, u, v, lo, cell, bins }
    }

    /// Whether the ray from `p` towards the light meets a triangle.
    fn blocked(&self, p: [f32; 3]) -> bool {
        let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
        let cross = |a: [f32; 3], b: [f32; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
        let (x, y) = ((dot(p, self.u) - self.lo[0]) / self.cell, (dot(p, self.v) - self.lo[1]) / self.cell);
        if !(x >= 0.0 && y >= 0.0 && x < Self::N as f32 && y < Self::N as f32) {
            return false;
        }
        let d = self.l;
        self.bins[y as usize * Self::N + x as usize].iter().any(|&i| {
            let [a, b, c] = self.tris[i as usize];
            let (e1, e2) = (sub(b, a), sub(c, a));
            let h = cross(d, e2);
            let det = dot(e1, h);
            if det.abs() < 1e-12 {
                return false;
            }
            let f = 1.0 / det;
            let s = sub(p, a);
            let bu = f * dot(s, h);
            if !(0.0..=1.0).contains(&bu) {
                return false;
            }
            let q = cross(s, e1);
            let bv = f * dot(d, q);
            bv >= 0.0 && bu + bv <= 1.0 && f * dot(e2, q) > 0.0
        })
    }
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
