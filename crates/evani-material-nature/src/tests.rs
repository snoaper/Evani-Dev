use super::*;
use evani_core::slots::{ShaderSlots, check_body};
use evani_render::post::{PostRecorder, UniformFrame};

fn validate(src: &str) {
    let module = naga::front::wgsl::parse_str(src).unwrap_or_else(|e| panic!("{}", e.emit_to_string(src)));
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
        .validate(&module)
        .unwrap_or_else(|e| panic!("{e:?}"));
}

fn spliced(src: &str) -> String {
    let mut slots = ShaderSlots::with_builtins();
    let id = slots.declare(params::SLOT);
    slots.set_body(id, &params::body(&params::default_table())).unwrap();
    let mut out = String::new();
    for line in src.lines() {
        match line.trim().strip_prefix("//@slot ") {
            Some(name) => out.push_str(&slots.wgsl_function(slots.id(name.trim()).unwrap()).unwrap()),
            None => out.push_str(line),
        }
        out.push('\n');
    }
    out
}

#[test]
fn every_shader_validates_with_its_slot() {
    for sh in effect::SHADERS.iter() {
        assert!(sh.source.starts_with("// evani.shader.nature"), "{} marker", sh.name);
        validate(&spliced(sh.source));
        assert_eq!(sh.slots.contains(&"nature_mat"), sh.source.contains("//@slot nature_mat"), "{}", sh.name);
    }
}

#[test]
fn slot_body_is_well_formed_and_has_every_row() {
    let b = params::body(&params::default_table());
    check_body(&b).unwrap();
    assert_eq!(b.matches("vec4<f32>(").count(), params::ROWS);
    assert!(!b.contains("NaN") && !b.contains("inf"));
}

#[test]
fn params_register_and_fill_the_table() {
    let mut reg = ParamRegistry::new();
    let p = NatureParams::register(&mut reg).unwrap();
    assert_eq!(p.table(&reg), params::default_table());
    assert!(reg.defs().iter().all(|d| !d.name.starts_with("material.nature.") || !d.description.is_empty()));
    let id = reg.id("material.nature.water.clarity").unwrap();
    reg.set(id, ParamValue::F32(7.5)).unwrap();
    let t = p.table(&reg);
    assert_eq!(t[2][3], 7.5);
    let id = reg.id("material.nature.glass.tint").unwrap();
    reg.set(id, ParamValue::Color([0.5, 0.6, 0.7])).unwrap();
    assert_eq!(p.table(&reg)[7], [0.5, 0.6, 0.7, 0.01]);
}

#[derive(Default)]
struct Fake {
    draws: Vec<(usize, Option<String>, String, Vec<f32>)>,
}

impl PostRecorder for Fake {
    fn target_size(&self, _: &str) -> [f32; 2] {
        [1280.0, 720.0]
    }
    fn draw(&mut self, shader: usize, src: Option<&str>, dst: &str, uniforms: &[f32]) {
        self.draws.push((shader, src.map(str::to_string), dst.to_string(), uniforms.to_vec()));
    }
}

fn scene(n: usize) -> NatureSurfaces {
    let s = NatureSurfaces::default();
    for i in 0..n {
        s.push(Surface::new(Kind::Foliage, [i as f32, 1.0, -5.0 - i as f32], [1.0, 1.0, 1.0]));
    }
    s.push(Surface::new(Kind::WaterRect, [0.0, 0.0, 30.0], [2.0, 0.0, 2.0])); // behind the camera
    s.0.camera.set(evani_core::camera::CameraPose {
        eye: [0.0, 2.0, 10.0],
        target: [0.0, 1.0, 0.0],
        up: [0.0, 1.0, 0.0],
        fov_y_deg: 60.0,
    });
    s
}

#[test]
fn each_mode_records_its_draws() {
    let reg = ParamRegistry::new();
    let s = scene(7);
    s.plan(40);
    assert_eq!(s.drawn(), 7, "the surface behind the camera is culled; every tier plans foliage");
    let fx = NatureEffect { surfaces: s.clone() };
    let order = |m: Mode| {
        s.0.mode.set(m);
        let mut f = Fake::default();
        fx.record("nature", &reg, &mut f);
        f.draws.iter().map(|d| (d.0, d.1.clone(), d.2.clone())).collect::<Vec<_>>()
    };
    let h = || "hdr".to_string();
    assert!(order(Mode::Off).is_empty());
    assert_eq!(
        order(Mode::Flat),
        vec![
            (effect::FLAT, None, h()),
            (effect::FLAT_ADD, None, h()),
            (effect::FLAT, None, h()),
            (effect::FLAT_ADD, None, h())
        ]
    );
    assert_eq!(order(Mode::Split).len(), 4);
    assert_eq!(
        order(Mode::Refract)[..2],
        [
            (effect::COPY, Some(h()), effect::SCENE.to_string()),
            (effect::REFRACT, Some(effect::SCENE.to_string()), h())
        ]
    );
    // Nothing for other passes.
    let mut f = Fake::default();
    fx.record("decals", &reg, &mut f);
    assert!(f.draws.is_empty());
}

#[test]
fn uniforms_hold_the_header_and_far_to_near_batches() {
    let reg = ParamRegistry::new();
    let s = scene(7);
    s.plan(40);
    let fx = NatureEffect { surfaces: s.clone() };
    let frame = |mip| UniformFrame {
        source_size: [0.0; 2],
        target_size: [1280.0, 720.0],
        mip,
    };
    let mut out = [0.0f32; 64];
    assert_eq!(fx.write_uniforms("nature_add", &reg, &frame(0), &mut out), 64);
    assert_eq!(out[15], 2.0, "mode");
    assert_eq!(out[19], 5.0, "a full first batch");
    assert_eq!(out[27], Kind::Foliage.code());
    // Farthest first: surface 6 sits at z -11.
    assert_eq!(out[26], -11.0);
    assert_eq!(fx.write_uniforms("nature_mul", &reg, &frame(1), &mut out), 64);
    assert_eq!(out[19], 2.0, "the rest");
    assert!(out[40..].iter().all(|v| *v == 0.0), "unused slots cleared");
    assert_eq!(fx.write_uniforms("nature_copy", &reg, &frame(0), &mut out), 4);
    assert_eq!(fx.write_uniforms("other", &reg, &frame(0), &mut out), 0);
    assert!(out.iter().all(|v| v.is_finite()));
}

/// Guard (geocheck round 4 of material-view: a plum's lit-through rim
/// flickered in the jittered shots only): under a capture's projection
/// jitter the pass's ray moves with the image, so a pixel rebuilds the point
/// the jittered draw put there. One pixel right is 2/width of the ray's
/// sideways reach; no jitter leaves the ray as it was.
#[cfg(feature = "capture")]
#[test]
fn the_ray_follows_a_capture_jitter() {
    let reg = ParamRegistry::new();
    let s = scene(3);
    s.plan(40);
    let fx = NatureEffect { surfaces: s.clone() };
    let frame = UniformFrame { source_size: [0.0; 2], target_size: [1280.0, 720.0], mip: 0 };
    let (mut a, mut b) = ([0.0f32; 64], [0.0f32; 64]);
    fx.write_uniforms("nature_add", &reg, &frame, &mut a);
    s.0.jitter.set([1.0, 0.0]);
    fx.write_uniforms("nature_add", &reg, &frame, &mut b);
    s.0.jitter.set([0.0; 2]);
    for i in 0..3 {
        let want = a[4 + i] - 2.0 / 1280.0 * a[8 + i];
        assert!((b[4 + i] - want).abs() < 1e-6, "fwd {i}: {} vs {want}", b[4 + i]);
    }
    assert_eq!(a[8..16], b[8..16], "right and up unchanged");
}

#[test]
fn the_cap_keeps_the_nearest() {
    let s = scene(30);
    s.plan(10);
    assert_eq!(s.drawn(), 10);
    let d = s.0.drawn.borrow();
    // The nearest (z -5) is drawn last.
    assert_eq!(d.last().map(|x| x.centre[2]), Some(-5.0));
    assert_eq!(d.first().map(|x| x.centre[2]), Some(-14.0));
}

#[test]
fn quality_picks_the_same_modes_as_the_tiers() {
    for (level, tier) in [(0, "lite"), (1, "low"), (2, "medium"), (3, "high")] {
        assert_eq!(Mode::for_quality(level, true), Mode::for_tier(tier, true), "level {level}");
    }
    assert_eq!(Mode::for_quality(3, false), Mode::Split, "no refraction: the Medium look");
    assert_eq!(Mode::for_quality(0, true), Mode::Flat);
}

#[test]
fn the_quality_param_is_registered_and_defaults_to_the_tier() {
    use evani_core::params::{ParamRegistry, ParamValue};
    let mut reg = ParamRegistry::new();
    let p = NatureParams::register(&mut reg).unwrap();
    assert_eq!(reg.f32(p.quality), -1.0, "by tier");
    assert_eq!(reg.id(params::QUALITY), Some(p.quality.id()));
    assert!(reg.set_by_name(params::QUALITY, ParamValue::F32(3.0)).is_ok());
    assert_eq!(reg.f32(p.quality), 3.0);
}

#[test]
fn tiers_pick_modes() {
    assert_eq!(Mode::for_tier("lite", true), Mode::Flat);
    assert_eq!(Mode::for_tier("low", true), Mode::Flat);
    assert_eq!(Mode::for_tier("medium", true), Mode::Split);
    assert_eq!(Mode::for_tier("vr", true), Mode::Split);
    assert_eq!(Mode::for_tier("high", true), Mode::Refract);
    assert_eq!(Mode::for_tier("ultra", false), Mode::Split);
    assert!(tier_cap("lite") < tier_cap("ultra"));
}

#[test]
fn kit_data_reads_surfaces() {
    let s = surfaces_from_json(
        r#"[{"kind":"water","centre":[0,0.5,0],"half":[4,0,3]},
            {"kind":"lampshade","center":[1,2,3],"half":[0.3,0.25,0.3],"yaw":0.5}]"#,
    )
    .unwrap();
    assert_eq!(s.len(), 2);
    assert_eq!(s[0].kind, Kind::WaterEllipse);
    assert_eq!(s[1].kind, Kind::Lamp);
    assert_eq!(s[1].yaw, 0.5);
    assert!(surfaces_from_json(r#"[{"kind":"lava","centre":[0,0,0],"half":[1,1,1]}]"#).is_err());
    assert!(surfaces_from_json(r#"[{"kind":"grass","centre":[0,0],"half":[1,1,1]}]"#).is_err());
    assert!(surfaces_from_json(r#"[{"kind":"grass","centre":[0,0,0],"half":[1,-1,1]}]"#).is_err());
    for k in Kind::ALL {
        assert_eq!(Kind::from_name(k.name()), Some(k));
    }
}

#[test]
fn placing_moves_turns_and_scales() {
    let s = Surface::new(Kind::GlassPane, [1.0, 0.0, 0.0], [0.5, 1.0, 0.01]);
    let p = s.placed([10.0, 0.0, 0.0], core::f32::consts::FRAC_PI_2, 2.0);
    assert!((p.centre[0] - 10.0).abs() < 1e-5 && (p.centre[2] + 2.0).abs() < 1e-5);
    assert_eq!(p.half, [1.0, 2.0, 0.02]);
}

/// FNV-1a over the bits of every value.
fn hash(words: impl Iterator<Item = f32>) -> u64 {
    words.fold(0xcbf2_9ce4_8422_2325u64, |h, v| {
        (h ^ u64::from(v.to_bits())).wrapping_mul(0x0100_0000_01b3)
    })
}

/// The waves depend only on position and sim time: two runs give the same
/// bits, a later time moves them, and the slopes stay bounded.
#[test]
fn water_waves_are_deterministic() {
    let grid = |t: f32| {
        let mut v = Vec::new();
        for z in -8..8 {
            for x in -8..8 {
                let g = water::slope([x as f32 * 0.37, z as f32 * 0.41], t, 0.9, 1.0, 0.12);
                v.extend(g);
            }
        }
        v
    };
    let a = grid(3.0);
    assert_eq!(hash(a.iter().copied()), hash(grid(3.0).iter().copied()));
    assert_ne!(hash(a.iter().copied()), hash(grid(3.5).iter().copied()));
    assert!(a.iter().all(|v| v.is_finite() && v.abs() < 1.0));
    // The integer hash matches the shader's PCG (reference values).
    assert_eq!(water::pcg(0), 129_708_002);
    assert_eq!(water::pcg(1), 2_831_084_092);
    // The same constants as the shader.
    let src = include_str!("../shaders/nature_common.wgsl");
    for c in ["747796405u", "2891336453u", "277803737u", "vec2<f32>(0.8, 0.6)", "3.9"] {
        assert!(src.contains(c), "{c}");
    }
}

/// The ocean upgrade (2026-11-01): every new param starts off, so a game
/// that sets none of them (Broadside3D) draws the water it drew before. The
/// default table leaves `water_more` false, the old 18 rows keep their
/// values, and the shader keeps the old wave code on that path. The GPU
/// side of the proof is `tools/nature/ocean-default-proof.mjs` (the old
/// and new shaders on the same inputs, pixel for pixel).
#[test]
fn the_ocean_upgrade_is_off_by_default() {
    let t = params::default_table();
    assert!(!params::ocean_upgrade_on(&t));
    let mut reg = ParamRegistry::new();
    let p = NatureParams::register(&mut reg).unwrap();
    for (name, row, col) in [
        ("swell_direction", 18, 0),
        ("swell", 18, 1),
        ("choppiness", 18, 2),
        ("flow_speed", 18, 3),
        ("sss_strength", 19, 3),
        ("whitecap_amount", 20, 0),
        ("whitecap_threshold", 20, 1),
        ("flow_direction", 20, 2),
        ("whitecap_depth", 20, 3),
    ] {
        let id = reg.id(&format!("material.nature.water.{name}")).unwrap_or_else(|| panic!("{name} is registered"));
        let def = reg.def(id).unwrap();
        assert!(!def.description.is_empty(), "{name} says what it does");
        // Each one, switched on alone, reaches its table cell; the gating
        // ones turn the upgrade on.
        let mut r2 = reg.clone();
        r2.set(id, ParamValue::F32(def.max)).unwrap();
        let t2 = p.table(&r2);
        assert_eq!(t2[row][col], def.max, "{name}");
        let gates = !matches!(name, "whitecap_threshold" | "flow_direction" | "whitecap_depth");
        assert_eq!(params::ocean_upgrade_on(&t2), gates, "{name}");
    }
    assert!(reg.id("material.nature.water.sss_color").is_some());
    // The shader's gate reads the same cells as `ocean_upgrade_on`.
    let common = include_str!("../shaders/nature_common.wgsl");
    assert!(common.contains("return any(mat(18u) != vec4<f32>(0.0)) || mat(19u).w != 0.0 || mat(20u).x != 0.0;"));
    // Off, `water` runs the old wave code: the unchanged `water_slope`, a
    // straight-up normal term of 1, the foam noise at the unmoved point.
    let water_fn = &common[common.find("fn water(").unwrap()..common.find("fn box_hit(").unwrap()];
    for c in [
        "var pf = p.xz;",
        "var ny = 1.0;",
        "} else {\n        g = water_slope(p.xz, u.time) * fade;\n    }",
        "let n = normalize(vec3<f32>(-g.x, ny, -g.y));",
        "if (more && wc.x > 0.0) {",
        "if (more && ss.w > 0.0) {",
    ] {
        assert!(water_fn.replace("\r\n", "\n").contains(c), "water(): `{c}`");
    }
    let slope_fn = &common[common.find("fn water_slope(").unwrap()..common.find("fn water_more(").unwrap()];
    assert!(slope_fn.contains("g = g + dirs[k] * (cos(ph) * 0.55 / freqs[k]);"), "the old wave sum is untouched");
}

/// The coast upgrade (2026-11-06, the Asset Studio shoreline): every new
/// param starts off, so a game that sets none of them draws the water it
/// drew before. Each one reaches its table cell and turns the upgrade on;
/// the shader runs none of it while `coast_more` is false, and its old
/// lines stay as they were.
#[test]
fn the_coast_upgrade_is_off_by_default() {
    let t = params::default_table();
    assert!(!params::coast_upgrade_on(&t));
    assert!(!params::ocean_upgrade_on(&t));
    let mut reg = ParamRegistry::new();
    let p = NatureParams::register(&mut reg).unwrap();
    for (name, row, col) in [
        ("coast_lace", 21, 0),
        ("coast_swash", 21, 1),
        ("coast_swash_spacing", 21, 2),
        ("coast_caustics", 21, 3),
        ("coast_caustic_scale", 22, 0),
        ("coast_ripples", 22, 1),
        ("coast_murk", 22, 2),
    ] {
        let id = reg.id(&format!("material.nature.water.{name}")).unwrap_or_else(|| panic!("{name} is registered"));
        let def = reg.def(id).unwrap();
        assert!(!def.description.is_empty(), "{name} says what it does");
        assert_eq!(def.default, ParamValue::F32(0.0), "{name} starts off");
        let mut r2 = reg.clone();
        r2.set(id, ParamValue::F32(def.max)).unwrap();
        let t2 = p.table(&r2);
        assert_eq!(t2[row][col], def.max, "{name}");
        assert!(params::coast_upgrade_on(&t2), "{name}");
        assert!(!params::ocean_upgrade_on(&t2), "{name} leaves the ocean upgrade alone");
    }
    let common = include_str!("../shaders/nature_common.wgsl").replace("\r\n", "\n");
    assert!(common.contains("return any(mat(21u) != vec4<f32>(0.0)) || any(mat(22u) != vec4<f32>(0.0));"));
    let water_fn = &common[common.find("fn water(").unwrap()..common.find("fn box_hit(").unwrap()];
    // Every coast line sits behind `if (coast)`; the old foam and colour
    // lines are untouched.
    for c in [
        "if (coast) {\n        g = g + coast_ripples(",
        "var caus = 0.0;\n    if (coast) {",
        "o.a = fr * refl + spec * (1.0 - foam) + (1.0 - fr) * (1.0 - foam) * inscatter + foam * foam_lit;\n    if (coast) {",
        "var foam = clamp(near * (0.35 + fn1 * 0.9 + fn2 * 0.4) * w5.x - (1.0 - near) * 0.2, 0.0, 1.0);",
        "o.m = (1.0 - fr) * max(trans * mix(vec3<f32>(1.0), deep, 0.4), deep * 0.5) * (1.0 - foam);",
    ] {
        assert!(water_fn.contains(c), "water(): `{c}`");
    }
    assert_eq!(water_fn.matches("coast_").count(), 4, "the coast helpers are called only inside the gated blocks");
}

/// The nature table's params and the spectral sea's share the
/// `material.nature.water.` names, registered by one module: none may be
/// taken twice (the coast upgrade's first `water.caustics` was the sea's,
/// and the sea then failed to register at all).
#[cfg(feature = "spectral-sea")]
#[test]
fn nature_and_sea_params_never_share_a_name() {
    let mut reg = ParamRegistry::new();
    NatureParams::register(&mut reg).unwrap();
    crate::sea::SeaParams::register(&mut reg).unwrap();
}

/// The CPU mirror of the upgraded waves: off, it is the old `slope` bit for
/// bit; the swell turns and gathers the waves; the flow carries the whole
/// pattern downstream; crests stay in -1..1; and it is deterministic.
#[test]
fn upgraded_waves_mirror_and_stay_deterministic() {
    let off = water::Waves::default();
    for z in -6..6 {
        for x in -6..6 {
            let p = [x as f32 * 0.53, z as f32 * 0.47];
            for t in [0.0, 1.5, 37.25] {
                let a = water::slope(p, t, off.scale, off.speed, off.strength);
                let b = water::waves(p, t, &off);
                assert_eq!([a[0].to_bits(), a[1].to_bits()], [b[0].to_bits(), b[1].to_bits()], "off = the old slope at {p:?} t {t}");
            }
        }
    }
    // Swell: turned by its direction, gathered round one heading.
    let dirs = water::wave_dirs(&water::Waves { swell_direction: 90.0, ..off });
    assert!((dirs[0][0] + 0.6).abs() < 1e-5 && (dirs[0][1] - 0.8).abs() < 1e-5, "turned a quarter: {:?}", dirs[0]);
    let gathered = water::wave_dirs(&water::Waves { swell: 1.0, swell_direction: 30.0, ..off });
    let head = gathered[0];
    for d in gathered {
        assert!(d[0] * head[0] + d[1] * head[1] > 0.6, "a full swell keeps every wave near its heading: {d:?}");
    }
    let crossing = water::wave_dirs(&off);
    assert!(crossing.iter().any(|d| d[0] * crossing[0][0] + d[1] * crossing[0][1] < 0.0), "off, the seas cross as before");
    // Flow: the pattern at p with the flow is the still pattern at p less
    // the drift.
    let flow = water::Waves { flow_speed: 1.3, flow_direction: 60.0, ..off };
    for t in [0.5f32, 2.0, 9.0] {
        let dr = water::drift(&flow, t);
        assert!(((dr[0] * dr[0] + dr[1] * dr[1]).sqrt() - 1.3 * t).abs() < 1e-3);
        for p in [[0.3f32, -1.2], [4.0, 2.5]] {
            let moved = water::waves([p[0] + dr[0], p[1] + dr[1]], t, &flow);
            let still = water::waves(p, t, &off);
            for k in 0..3 {
                assert!((moved[k] - still[k]).abs() < 2e-3, "flow carries the pattern: {moved:?} vs {still:?}");
            }
        }
    }
    // Everything on: bounded, the same twice, moving with time.
    let on = water::Waves { swell: 0.7, swell_direction: 200.0, flow_speed: 0.8, flow_direction: 15.0, ..off };
    let grid = |t: f32| {
        let mut v = Vec::new();
        for z in -8..8 {
            for x in -8..8 {
                v.extend(water::waves([x as f32 * 0.37, z as f32 * 0.41], t, &on));
            }
        }
        v
    };
    let a = grid(3.0);
    assert_eq!(hash(a.iter().copied()), hash(grid(3.0).iter().copied()));
    assert_ne!(hash(a.iter().copied()), hash(grid(3.5).iter().copied()));
    assert!(a.chunks(3).all(|w| w.iter().all(|v| v.is_finite()) && w[0].abs() < 1.0 && w[1].abs() < 1.0 && w[2].abs() <= 1.0));
    // The same constants as the shader's `water_waves`.
    let src = include_str!("../shaders/nature_common.wgsl");
    for c in ["array<f32, 4>(0.0, 0.38, -0.52, 0.85)", "crest / 1.21967", "radians(mat(20u).z)", "d = turn2(d, turn);"] {
        assert!(src.contains(c), "{c}");
    }
}

/// Writes the spliced default shaders for the GPU proof
/// (`tools/nature/ocean-default-proof.mjs`) when `EVANI_NATURE_DUMP` names a
/// folder: `new-nature.wgsl`, `new-refract.wgsl`, and, with
/// `EVANI_NATURE_OLD` naming a folder holding the old `nature.wgsl`,
/// `nature_refract.wgsl` and `nature_common.wgsl`, `old-*.wgsl` spliced with
/// the old 18-row table (the first 18 rows of today's default table, which
/// the upgrade left as they were). Does nothing otherwise.
#[test]
fn dump_default_shaders_for_the_gpu_proof() {
    let Ok(dir) = std::env::var("EVANI_NATURE_DUMP") else { return };
    let dir = std::path::Path::new(&dir);
    std::fs::create_dir_all(dir).unwrap();
    let new_body = params::body(&params::default_table());
    let splice = |src: &str, body: &str| {
        let mut slots = ShaderSlots::with_builtins();
        let id = slots.declare(params::SLOT);
        slots.set_body(id, body).unwrap();
        let mut out = String::new();
        for line in src.replace("\r\n", "\n").lines() {
            match line.trim().strip_prefix("//@slot ") {
                Some(name) => out.push_str(&slots.wgsl_function(slots.id(name.trim()).unwrap()).unwrap()),
                None => out.push_str(line),
            }
            out.push('\n');
        }
        out
    };
    for sh in effect::SHADERS.iter() {
        let file = match sh.name {
            "nature_mul" => "new-nature.wgsl",
            "nature_refract" => "new-refract.wgsl",
            _ => continue,
        };
        std::fs::write(dir.join(file), splice(sh.source, &new_body)).unwrap();
    }
    if let Ok(old) = std::env::var("EVANI_NATURE_OLD") {
        let old = std::path::Path::new(&old);
        let read = |f: &str| std::fs::read_to_string(old.join(f)).unwrap();
        let t = params::default_table();
        let mut b = String::from("    // material.nature: the nature material table\n    var t = array<vec4<f32>, 18>(\n");
        for r in &t[..18] {
            let lit = |v: f32| format!("{v:.5}");
            b.push_str(&format!("        vec4<f32>({}, {}, {}, {}),\n", lit(r[0]), lit(r[1]), lit(r[2]), lit(r[3])));
        }
        b.push_str("    );\n    return t[min(i, 17u)];");
        let common = read("nature_common.wgsl");
        std::fs::write(dir.join("old-nature.wgsl"), splice(&format!("{}\n{common}", read("nature.wgsl")), &b)).unwrap();
        std::fs::write(dir.join("old-refract.wgsl"), splice(&format!("{}\n{common}", read("nature_refract.wgsl")), &b)).unwrap();
    }
}

/// Refraction bends only onto pixels seen through the same water (the pale
/// wedge in the night pool on Ultra, material-view round 4): a raised basin
/// (water 0.4 m up, 2 x 1.4 m) with the lawn (y = 0) behind its far rim.
/// The bend lifts a water pixel's lookup up the screen; past the far rim it
/// lands on the lawn, which is behind the water surface too, so the old
/// rule (behind the surface) took it. The new rule asks the bent pixel's own
/// ray to cross the water, as the bed's pixels do.
#[test]
fn refraction_bends_only_onto_what_the_water_covers() {
    let eye = [0.0, 2.0, 4.0];
    let (centre, half) = ([0.0, 0.4, 0.0], [1.0, 0.7]);
    let at = |p: [f32; 3]| [p[0] - eye[0], p[1] - eye[1], p[2] - eye[2]];
    // The pixel being shaded: water at (0, 0.4, 0) over the bed.
    let r0 = at([0.0, 0.0, -1.0]);
    let tw0 = (centre[1] - eye[1]) / r0[1];
    assert!(water::ray_crosses_rect(eye, r0, 1.0, centre, half, 0.0));
    // Bent onto the lawn behind the basin: behind the water (the old test
    // passes) but not seen through it.
    let lawn = at([0.0, 0.0, -3.0]);
    assert!(1.0 > tw0, "the lawn is behind the water surface");
    assert!(!water::ray_crosses_rect(eye, lawn, 1.0, centre, half, 0.0));
    // Bent onto the bed: seen through the water, kept.
    assert!(water::ray_crosses_rect(eye, at([0.1, 0.0, -0.2]), 1.0, centre, half, 0.0));
    // Turned basins and the sky: the footprint turns with the basin, the
    // sky beside the basin is never seen through it.
    // (0.25, 0, 0.125) is seen through the plane at (0.2, 0.4, 0.9): past
    // the unturned basin's 0.7 m half depth, inside it turned a quarter.
    let side = at([0.25, 0.0, 0.125]);
    assert!(!water::ray_crosses_rect(eye, side, 1.0, centre, half, 0.0));
    assert!(water::ray_crosses_rect(eye, side, 1.0, centre, half, std::f32::consts::FRAC_PI_2));
    assert!(!water::ray_crosses_rect(eye, [0.0, 0.2, -1.0], f32::INFINITY, centre, half, 0.0));
    // The shader uses the same rule: the refract draw bends only where the
    // bent pixel is seen through the surface the bend came from.
    let refract = include_str!("../shaders/nature_refract.wgsl");
    let common = include_str!("../shaders/nature_common.wgsl");
    assert!(refract.contains("scene_dist(zb) > s.tnear && seen_through(s.idx, bent, zb)"));
    assert!(common.contains("return surface(u.s[i], pixel(uv, z), true).t > 0.0;"));
    assert!(common.contains("out.idx = ids[best];"));
    let water_fn = &common[common.find("fn water(").unwrap()..common.find("let deep =").unwrap()];
    for c in ["u.eye.y <= s.c.y", "tw >= px.ts", "rot_y(p - s.c.xyz, -s.h.w)", "abs(l.x) > s.h.x || abs(l.z) > s.h.z"] {
        assert!(water_fn.contains(c), "the water hit test has `{c}` as water::ray_crosses_rect does");
    }
}

/// Found by the material viewer's critic, round 4 (2026-10-27): on High
/// and Ultra the pool's water was milky and hid its bed, which Medium and
/// Low showed. Switching parts off live, only refraction removed it. The
/// refract draw bent each pixel by `screen_bend(n)`, the whole water normal
/// on the camera's axes: a calm pool's straight-up normal, seen from 20
/// degrees above, bent every pixel 0.94 x 0.05 x its path up the screen,
/// onto the basin's pale far wall. The bend is the waves' tilt only now: a
/// calm surface bends nothing, a tilted one by its tilt alone. A CPU mirror
/// of `screen_bend` and the camera axes, and the shader pinned to the rule.
#[test]
fn calm_water_does_not_bend_the_bed_away() {
    let norm = |v: [f32; 3]| {
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        v.map(|x| x / l)
    };
    let cross = |a: [f32; 3], b: [f32; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    // The viewer's pool camera (moment `pool`): eye 3 m up, 7 m back.
    let fwd = norm([0.02 - 0.76, 0.53 - 3.09, 0.34 - 7.33]);
    let right = norm(cross(fwd, [0.0, 1.0, 0.0]));
    let up = cross(right, fwd);
    let bend = |n: [f32; 3]| [dot(n, right), -dot(n, up)];
    let flat = [0.0, 1.0, 0.0];
    // The old bend: most of a unit up the screen for calm water.
    let old = bend(flat);
    assert!(old[1].abs() > 0.9, "the old rule bent calm water by {old:?}");
    // The new one: nothing for calm water, the tilt for a wave.
    let tilt = |n: [f32; 3]| bend([n[0] - flat[0], n[1] - flat[1], n[2] - flat[2]]);
    assert_eq!(tilt(flat), [0.0, 0.0]);
    let wave = norm([0.1, 1.0, 0.0]);
    let b = tilt(wave);
    assert!(b[0] > 0.05 && b[1].abs() < 0.02, "a wave tilted along x bends along the screen's x: {b:?}");
    let common = include_str!("../shaders/nature_common.wgsl");
    let water_fn = &common[common.find("fn water(").unwrap()..common.find("fn box_hit(").unwrap()];
    assert!(water_fn.contains("o.off = screen_bend(n - vec3<f32>(0.0, 1.0, 0.0)) * w5.y * min(path, 2.0) * 0.05;"), "the water bends by its waves' tilt only");
    assert!(!water_fn.contains("screen_bend(n) *"), "the water no longer bends by its whole normal");
}

/// A glb with only a JSON chunk (the reader never needs the binary one).
fn glb(json: &serde_json::Value) -> Vec<u8> {
    let mut j = serde_json::to_vec(json).unwrap();
    while !j.len().is_multiple_of(4) {
        j.push(b' ');
    }
    let mut out = Vec::new();
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(20 + j.len() as u32).to_le_bytes());
    out.extend_from_slice(&(j.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&j);
    out
}

fn nature_model() -> serde_json::Value {
    let acc = |lo: [f32; 3], hi: [f32; 3]| serde_json::json!({ "componentType": 5126, "count": 8, "type": "VEC3", "min": lo, "max": hi });
    let prim = |acc: u32, mat: u32| serde_json::json!({ "primitives": [{ "attributes": { "POSITION": acc }, "material": mat }] });
    // The pane turns 90 degrees about +y.
    let (s, c) = (core::f32::consts::FRAC_PI_4.sin(), core::f32::consts::FRAC_PI_4.cos());
    serde_json::json!({
        "asset": { "version": "2.0" },
        "scene": 0,
        "scenes": [{ "nodes": [0, 5, 6] }],
        "nodes": [
            { "name": "yard", "translation": [10.0, 0.0, 0.0], "children": [1, 2, 3, 4] },
            { "name": "pond", "mesh": 0, "translation": [0.0, 0.5, 0.0] },
            { "name": "pane", "mesh": 1, "rotation": [0.0, s, 0.0, c] },
            { "name": "bottle", "mesh": 2, "scale": [2.0, 2.0, 2.0] },
            { "name": "leaf", "mesh": 3 },
            { "name": "rock", "mesh": 4 },
            { "name": "bush", "mesh": 5, "matrix": [1.0,0.0,0.0,0.0, 0.0,1.0,0.0,0.0, 0.0,0.0,1.0,0.0, 0.0,3.0,-4.0,1.0] }
        ],
        "meshes": [prim(0, 0), prim(1, 1), prim(2, 2), prim(3, 3), prim(4, 4), prim(5, 5)],
        "accessors": [
            acc([-2.0, -0.1, -1.0], [2.0, 0.1, 1.0]),
            acc([-0.5, 0.0, -0.01], [0.5, 1.0, 0.01]),
            acc([-0.1, 0.0, -0.1], [0.1, 0.6, 0.1]),
            acc([-0.2, -0.2, -0.005], [0.2, 0.2, 0.005]),
            acc([-1.0, -1.0, -1.0], [1.0, 1.0, 1.0]),
            acc([-1.0, -0.8, -1.0], [1.0, 0.8, 1.0])
        ],
        "materials": [
            { "name": "water", "extensions": { "KHR_materials_transmission": { "transmissionFactor": 1.0 }, "KHR_materials_ior": { "ior": 1.33 } } },
            { "name": "window", "extensions": { "KHR_materials_transmission": { "transmissionFactor": 0.9 } } },
            { "name": "jar", "extensions": { "KHR_materials_transmission": { "transmissionFactor": 1.0 }, "KHR_materials_volume": { "thicknessFactor": 0.01 } } },
            { "name": "leafy", "extensions": { "KHR_materials_diffuse_transmission": { "diffuseTransmissionFactor": 0.5 } } },
            { "name": "stone", "extensions": { "KHR_materials_transmission": { "transmissionFactor": 0.0 } } },
            { "name": "crown", "extras": { "evani_kind": "foliage" } }
        ]
    })
}

fn near(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-4)
}

#[test]
fn gltf_picks_kinds_and_boxes_from_a_glb() {
    let n = gltf::from_glb(&glb(&nature_model())).unwrap();
    let kinds: Vec<Kind> = n.surfaces.iter().map(|s| s.kind).collect();
    assert_eq!(
        kinds,
        [Kind::WaterRect, Kind::GlassPane, Kind::GlassBottle, Kind::ThinPatch, Kind::Foliage],
        "the stone (no transmission) is left out"
    );
    let s = &n.surfaces;
    // Water: the level is the top of the box, moved by both nodes.
    assert!(near(s[0].centre, [10.0, 0.6, 0.0]), "{:?}", s[0].centre);
    assert!(near(s[0].half, [2.0, 0.1, 1.0]));
    // The pane keeps its size and takes the node's turn.
    assert!(near(s[1].centre, [10.0, 0.5, 0.0]));
    assert!(near(s[1].half, [0.5, 0.5, 0.01]));
    assert!((s[1].yaw - core::f32::consts::FRAC_PI_2).abs() < 1e-4, "{}", s[1].yaw);
    // The bottle: scaled 2x; radius 0.2, straight half 0.6 - 0.2.
    assert!(near(s[2].centre, [10.0, 0.6, 0.0]));
    assert!(near(s[2].half, [0.2, 0.4, 0.2]), "{:?}", s[2].half);
    // The tagged crown under a matrix node.
    assert!(near(s[4].centre, [0.0, 3.0, -4.0]));
    assert_eq!(n.hide, ["bottle", "pane", "pond"], "water and glass nodes only, sorted");
}

#[test]
fn gltf_root_map_and_tags_win_over_extensions() {
    let mut j = nature_model();
    // The converter's root map names the window material a bottle.
    j["extras"] = serde_json::json!({ "evani": { "nature": { "window": "glass_bottle", "stone": "lamp" } } });
    j["materials"][0]["extras"] = serde_json::json!({ "evani_kind": "water" });
    let n = gltf::from_json(&j);
    let kinds: Vec<Kind> = n.surfaces.iter().map(|s| s.kind).collect();
    assert_eq!(kinds[0], Kind::WaterEllipse, "the material's own tag first");
    assert_eq!(kinds[1], Kind::GlassBottle, "then the root map");
    assert!(kinds.contains(&Kind::Lamp), "the map picks a material with no extension");
    assert!(!n.hide.contains(&"rock".to_string()), "a lamp stays solid");
    assert_eq!(gltf::material_kind(&j, 3), Some(Kind::ThinBlob));
    assert_eq!(gltf::material_kind(&j, 9), None);
}

#[test]
fn gltf_refuses_what_is_not_a_model() {
    assert!(gltf::from_glb(b"nope").is_err());
    let mut bad = glb(&nature_model());
    bad[16] = b'B';
    assert!(gltf::from_glb(&bad).is_err());
    bad.truncate(30);
    assert!(gltf::from_glb(&bad).is_err());
    // A .gltf text works too; a node cycle does not hang.
    let mut j = nature_model();
    j["nodes"][1]["children"] = serde_json::json!([0]);
    let n = gltf::from_glb(&serde_json::to_vec(&j).unwrap()).unwrap();
    assert_eq!(n.surfaces.len(), 5);
}

#[test]
fn reflected_sky_takes_the_background_colour() {
    let mut table = [[0.0f32; 4]; params::ROWS];
    table[0] = [0.62, 0.72, 0.82, 1.0];
    let bg = [0.35, 0.55, 0.85];
    let t = sky_tint(&table, Some(bg));
    for i in 0..3 {
        assert!((t[i] * table[0][i] - bg[i]).abs() < 1e-5, "channel {i}: {t:?}");
    }
    // Off, or no background: the sky colours as given.
    assert_eq!(sky_tint(&table, None), [1.0; 3]);
    table[0][3] = 0.0;
    assert_eq!(sky_tint(&table, Some(bg)), [1.0; 3]);
    // A grey background matches the brightness only.
    table[0][3] = 1.0;
    let g = sky_tint(&table, Some([0.5; 3]));
    assert!((g[0] - g[2]).abs() < 1e-6);
    // The packed float is an exact integer and unpacks like the shader's sky_k.
    let p = pack_sky_tint(t);
    assert_eq!(p.fract(), 0.0);
    assert!(p < 16_777_216.0);
    let u = p as u32;
    let back = [u & 255, (u >> 8) & 255, (u >> 16) & 255].map(|q| q as f32 * SKY_TINT_MAX / 255.0);
    for i in 0..3 {
        assert!((back[i] - t[i]).abs() <= SKY_TINT_MAX / 255.0 * 0.5 + 1e-6);
    }
    assert_eq!(pack_sky_tint([1.0; 3]) as u32 & 255, 64);
}

/// Frame order guard (the Broadside3D free-look flicker): the ocean copies
/// the camera in its frame step, and the render module, added before it,
/// must draw with that frame's copy. A drawing probe added where the render
/// module sits (before nature) reads what the nature pass would draw with.
mod frame_order {
    use evani_core::App;
    use evani_core::ecs::{Stage, SystemDesc, World};
    use evani_core::plugin::EngineModule;
    use evani_core::time::Time;
    use evani_render::{RenderModule, RenderWorld};

    use crate::{NatureModule, NatureSurfaces};

    #[derive(Default)]
    struct Seen(Vec<([f32; 3], [f32; 3])>);

    struct DrawProbe;
    impl EngineModule for DrawProbe {
        fn id(&self) -> &'static str {
            "evani.module.test_draw_probe"
        }
        fn label(&self) -> &'static str {
            "probe"
        }
        fn frame(&mut self, world: &mut World) {
            let game = world.res::<RenderWorld>().camera.eye;
            let ocean = world.res::<NatureSurfaces>().0.camera.get().eye;
            world.res_mut::<Seen>().0.push((game, ocean));
        }
        fn draws(&self) -> bool {
            true
        }
    }

    #[test]
    fn the_ocean_is_drawn_with_this_frames_camera() {
        let mut app = App::new("t")
            .with_module(RenderModule::new())
            .with_module(DrawProbe)
            .with_module(NatureModule::new())
            // A free-look camera dragged tens of metres a frame.
            .with_system(SystemDesc::new("drag", Stage::Frame, |w: &mut World| {
                let f = w.res::<Time>().frame as f32;
                w.res_mut::<RenderWorld>().camera.eye = [40.0 * f, 30.0, 60.0];
            }));
        app.world.insert_resource(Seen::default());
        app.run_headless(6, 16.0);
        let seen = &app.world.res::<Seen>().0;
        assert_eq!(seen.len(), 6);
        for (i, (game, ocean)) in seen.iter().enumerate() {
            assert_eq!(game, ocean, "frame {}: the ocean must use this frame's camera", i + 1);
        }
    }

    /// Critic round 7 of material-view: a capture from its own pose (the
    /// geometry check's debug views) swaps the camera only when the frame is
    /// drawn, so water and glass planned from the game's camera were drawn
    /// away from their props in the back views. The pass plans from the
    /// capture's pose.
    #[cfg(feature = "capture")]
    #[test]
    fn a_capture_pose_is_the_camera_the_pass_plans_from() {
        use evani_core::camera::CameraPose;
        use evani_core::capture::CaptureQueue;
        let mut app = App::new("t").with_module(RenderModule::new()).with_module(NatureModule::new());
        let back = CameraPose { eye: [0.0, 3.0, -7.0], target: [0.0, 0.0, 0.0], ..CameraPose::default() };
        if app.world.try_res::<CaptureQueue>().is_none() {
            app.world.insert_resource(CaptureQueue::default());
        }
        app.world.res_mut::<CaptureQueue>().camera_override = Some(back);
        app.run_headless(2, 16.0);
        assert_eq!(app.world.res::<NatureSurfaces>().0.camera.get().eye, back.eye);
        app.world.res_mut::<CaptureQueue>().camera_override = None;
        app.run_headless(2, 16.0);
        let game = app.world.res::<RenderWorld>().camera.eye;
        assert_eq!(app.world.res::<NatureSurfaces>().0.camera.get().eye, game, "the game's camera again once the capture is done");
    }
}
