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
    s.plan(40, false);
    assert_eq!(s.drawn(), 7, "the surface behind the camera is culled");
    s.plan(40, true);
    assert_eq!(s.drawn(), 0, "Lite and Low plan only water and glass");
    s.plan(40, false);
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
    s.plan(40, false);
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

#[test]
fn the_cap_keeps_the_nearest() {
    let s = scene(30);
    s.plan(10, false);
    assert_eq!(s.drawn(), 10);
    let d = s.0.drawn.borrow();
    // The nearest (z -5) is drawn last.
    assert_eq!(d.last().map(|x| x.centre[2]), Some(-5.0));
    assert_eq!(d.first().map(|x| x.centre[2]), Some(-14.0));
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
}
