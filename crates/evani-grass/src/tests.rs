use evani_core::App;
use evani_core::camera::CameraPose;
use evani_core::params::{ParamRegistry, ParamValue};
use evani_render::{RenderModule, RenderWorld};

use super::*;
use crate::plan::{PatchPlan, at};

fn parse(src: &str) -> naga::Module {
    naga::front::wgsl::parse_str(src).unwrap_or_else(|e| panic!("{}", e.emit_to_string(src)))
}

fn validate(src: &str) -> naga::Module {
    let module = parse(src);
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
        .validate(&module)
        .unwrap_or_else(|e| panic!("{e:?}"));
    module
}

/// A fog slot like the fx fog writes: distance and height fog.
const SOME_FOG: &str = "fn slot_fog(distance: f32, height: f32) -> f32 {\n    let d = max(distance - 20.0, 0.0) * 0.004;\n    return clamp((1.0 - exp(-d)) * exp(-max(height, 0.0) * 0.02), 0.0, 1.0);\n}\n";

#[test]
fn every_wgsl_module_validates_without_optional_capabilities() {
    validate(&wgsl::cs_module());
    validate(&wgsl::draw_module(NO_FOG));
    validate(&wgsl::draw_module(SOME_FOG));
    // The shipped form is the squeezed one: no comments, well under the text.
    let src = include_str!("../shaders/grass_draw.wgsl").len() + include_str!("../shaders/grass_cs.wgsl").len();
    assert!(!wgsl::DRAW.contains("//") && wgsl::DRAW.len() + wgsl::CS.len() < src * 7 / 10);
}

#[test]
fn the_builtin_fog_slot_matches_what_the_draw_calls() {
    let slots = evani_core::slots::ShaderSlots::with_builtins();
    let fog = slots.wgsl_function(slots.id("fog").unwrap()).unwrap();
    validate(&wgsl::draw_module(&fog));
}

fn struct_layout(m: &naga::Module, name: &str) -> (u32, Vec<(String, u32)>) {
    for (_, ty) in m.types.iter() {
        if ty.name.as_deref() == Some(name)
            && let naga::TypeInner::Struct { members, span } = &ty.inner
        {
            return (*span, members.iter().map(|mb| (mb.name.clone().unwrap_or_default(), mb.offset)).collect());
        }
    }
    panic!("no struct {name}");
}

#[test]
fn the_uniform_words_match_the_wgsl_layout() {
    let m = validate(&wgsl::cs_module());
    let (span, members) = struct_layout(&m, "Sim");
    assert_eq!(span as usize, plan::SIM_WORDS * 4, "Sim size");
    let want = [
        ("view_proj", at::VIEW_PROJ),
        ("planes", at::PLANES),
        ("eye", at::EYE),
        ("wind", at::WIND),
        ("wind2", at::WIND2),
        ("lod", at::LOD),
        ("lod2", at::LOD2),
        ("trample", at::TRAMPLE),
        ("trample2", at::TRAMPLE2),
        ("stamps_n", at::STAMPS_N),
        ("caps", at::CAPS),
        ("bases", at::BASES),
        ("scales", at::SCALES),
        ("stamps", at::STAMPS),
        ("kinds", at::KINDS),
        ("patches", at::PATCHES),
    ];
    for (name, word) in want {
        let off = members.iter().find(|(n, _)| n == name).map(|(_, o)| *o).unwrap_or_else(|| panic!("no member {name}"));
        assert_eq!(off as usize, word * 4, "Sim.{name}");
    }
    assert_eq!(struct_layout(&m, "Kind").0 as usize, at::KIND_WORDS * 4);
    assert_eq!(struct_layout(&m, "Patch").0 as usize, at::PATCH_WORDS * 4);
    assert_eq!(u64::from(struct_layout(&m, "Blade").0), plan::BLADE_BYTES);
    let d = validate(&wgsl::draw_module(NO_FOG));
    assert_eq!(struct_layout(&d, "View").0 as usize, plan::VIEW_WORDS * 4);
}

#[test]
fn every_param_is_a_material_grass_name_and_registers_once() {
    let names = params::names();
    assert_eq!(names.len(), 27 + 4 * params::k::COUNT);
    for n in &names {
        assert!(n.starts_with("material.grass."), "{n}");
        assert!(evani_core::params::is_valid_param_name(n), "{n}");
    }
    let mut reg = ParamRegistry::new();
    let p = GrassParams::register(&mut reg).unwrap();
    assert!(GrassParams::register(&mut reg).is_err(), "a second registration is refused");
    for n in &names {
        let id = reg.id(n).unwrap_or_else(|| panic!("{n} missing"));
        assert_eq!(reg.def(id).unwrap().owner, params::OWNER);
    }
    // The four kinds are distinct parameter sets on one system.
    let h = |k| p.kf(&reg, k, params::k::HEIGHT);
    assert!(h(Kind::Lawn) < 0.15 && h(Kind::Meadow) > 0.4 && h(Kind::Wheat) > h(Kind::Meadow));
    assert!(p.kf(&reg, Kind::Wheat, params::k::EAR_LENGTH) > 0.0);
    assert_eq!(p.kf(&reg, Kind::Meadow, params::k::EAR_LENGTH), 0.0);
    assert!(p.kf(&reg, Kind::Mud, params::k::BARE) > 0.3 && p.kf(&reg, Kind::Mud, params::k::FLAT) > 0.3);
    // Live: a set value is read back.
    reg.set_by_name("material.grass.meadow.height", ParamValue::F32(0.9)).unwrap();
    assert_eq!(p.kf(&reg, Kind::Meadow, params::k::HEIGHT), 0.9);
}

#[test]
fn tiers_grow_in_cost_and_phones_stay_small() {
    for w in plan::TIERS.windows(2) {
        let (a, b) = (w[0], w[1]);
        assert!(a.max_dist < b.max_dist && a.budget < b.budget);
        assert!(a.caps.iter().sum::<u32>() < b.caps.iter().sum::<u32>());
        assert!(a.segs[0] <= b.segs[0] && a.trample_n <= b.trample_n);
    }
    let low = plan::tier(1);
    assert!(low.caps.iter().sum::<u32>() <= 40_000, "phones draw at most 40k blades");
    assert!((5..=7).contains(&(2 * low.segs[0] + 1)) && low.trample_n <= 64 && low.translucency == 0.0);
    let mid = plan::tier(2);
    assert!((100_000..=200_000).contains(&mid.caps.iter().sum::<u32>()));
    assert!(plan::tier(3).translucency > 0.0 && plan::tier(4).gloss > 0.0);
    assert_eq!(plan::level(-1.0, "low"), 1);
    assert_eq!(plan::level(4.0, "low"), 4);
    assert_eq!(plan::level(-1.0, "vr"), 2);
}

fn camera(eye: [f32; 3], target: [f32; 3]) -> CameraPose {
    CameraPose { eye, target, up: [0.0, 1.0, 0.0], fov_y_deg: 55.0 }
}

fn meadow(min: [f32; 2], max: [f32; 2]) -> PatchPlan {
    PatchPlan { patch: Patch::new(Kind::Meadow, min, max), density: 400.0, max_height: 0.9, reach: 1.0 }
}

#[test]
fn planes_keep_what_is_in_front_and_drop_what_is_behind() {
    let vp = evani_render::shaders::view_proj(&camera([0.0, 1.7, 0.0], [0.0, 1.0, -10.0]), 16.0 / 9.0);
    let pl = plan::planes(&vp);
    assert!(plan::box_visible(&pl, [-1.0, 0.0, -11.0], [1.0, 1.0, -9.0]));
    assert!(!plan::box_visible(&pl, [-1.0, 0.0, 9.0], [1.0, 1.0, 11.0]), "behind the camera");
    assert!(!plan::box_visible(&pl, [80.0, 0.0, -11.0], [82.0, 1.0, -9.0]), "far off to the side");
}

#[test]
fn the_plan_thins_with_distance_keeps_the_budget_and_repeats() {
    let cfg = plan::tier(3);
    let cam = camera([0.0, 1.7, 0.0], [0.0, 1.0, -10.0]);
    let vp = evani_render::shaders::view_proj(&cam, 16.0 / 9.0);
    let pl = plan::planes(&vp);
    let patches = [meadow([-100.0, -100.0], [100.0, 100.0])];
    let mut jobs = Vec::new();
    let st = plan::plan_jobs(&patches, cam.eye, Some(&pl), &cfg, cfg.max_dist, &mut jobs);
    assert!(st.tiles > 20 && st.jobs as usize == jobs.len());
    assert!(st.candidates <= cfg.budget + st.tiles, "within budget (rounding up per tile)");
    // Jobs come nearest tile first.
    let dist = |j: &[u32; 4]| ((j[0] as i32 as f32 + 0.5) * plan::TILE).hypot((j[1] as i32 as f32 + 0.5) * plan::TILE);
    assert!(dist(&jobs[0]) < 8.0 && dist(&jobs[jobs.len() - 1]) > 30.0);
    let mut again = Vec::new();
    plan::plan_jobs(&patches, cam.eye, Some(&pl), &cfg, cfg.max_dist, &mut again);
    assert_eq!(jobs, again, "deterministic");
    // Per tile: blades issued fall with distance; nothing past the reach or
    // behind the camera.
    let mut near = 0u32;
    let mut far = 0u32;
    for j in &jobs {
        let (ix, iz) = (j[0] as i32, j[1] as i32);
        let c = [(ix as f32 + 0.5) * plan::TILE, (iz as f32 + 0.5) * plan::TILE];
        let d = (c[0] * c[0] + c[1] * c[1]).sqrt();
        assert!(d < cfg.max_dist + plan::TILE, "tile at {d} m is past the reach");
        assert!(c[1] < plan::TILE * 2.0, "tile behind the camera at z {}", c[1]);
        let n = j[3] >> 16;
        if d < 8.0 {
            near += n;
        } else if d > 40.0 {
            far += n;
        }
    }
    assert!(near > 0 && far > 0);
    // Without culling, every direction is planned.
    let mut all = Vec::new();
    let st2 = plan::plan_jobs(&patches, cam.eye, None, &cfg, cfg.max_dist, &mut all);
    assert!(st2.tiles > st.tiles * 2);
    // Over budget: everything scales down evenly.
    let tight = TierCfg { budget: 5_000, ..cfg };
    let st3 = plan::plan_jobs(&patches, cam.eye, Some(&pl), &tight, cfg.max_dist, &mut all);
    assert!(st3.scale < 1.0 && st3.candidates <= 5_000 + st3.tiles);
    // A small near bucket thins only the near bucket.
    let small = TierCfg { caps: [2_000, cfg.caps[1], cfg.caps[2]], ..cfg };
    let st4 = plan::plan_jobs(&patches, cam.eye, Some(&pl), &small, cfg.max_dist, &mut all);
    assert!(st4.scales[0] < 0.2 && st4.scales[0] < st.scales[0], "{:?}", st4.scales);
    assert_eq!(st4.scales[1..], st.scales[1..], "the other buckets keep their share");
    // A lawn is drawn less far than knee-high grass.
    assert!(plan::reach(0.085) < 0.5 && plan::reach(0.62) == 1.0);
    let lawn = [PatchPlan { reach: plan::reach(0.085), ..patches[0] }];
    let st5 = plan::plan_jobs(&lawn, cam.eye, Some(&pl), &cfg, cfg.max_dist, &mut all);
    assert!(st5.tiles * 3 < st.tiles);
}

#[test]
fn keep_matches_full_near_and_none_past_the_reach() {
    assert_eq!(plan::keep_at(3.0, 10.0, 50.0), 1.0);
    assert!((plan::keep_at(20.0, 10.0, 50.0) - 0.25).abs() < 1e-6);
    assert_eq!(plan::keep_at(50.0, 10.0, 50.0), 0.0);
}

#[test]
fn trampled_grass_springs_back_with_its_half_life_slower_when_wet_or_kept_flat() {
    let r = plan::relax(7.0, 7.0, 0.0, 2.0, false);
    assert!((r - 0.5).abs() < 1e-5, "one half-life halves it: {r}");
    let wet = plan::relax(7.0, 7.0, 1.0, 2.0, false);
    assert!(wet > 0.75, "soaked grass rises three times slower: {wet}");
    assert_eq!(plan::relax(7.0, 7.0, 0.0, 2.0, true), 1.0);
    // The window: a centre in its middle cell.
    let o = plan::trample_origin([10.1, -3.9], 64, 0.25);
    assert_eq!(o, [40 - 32, -16 - 32]);
}

fn grass_app() -> App {
    let mut app = App::new("t").with_module(RenderModule::new()).with_module(GrassModule::new());
    app.run_headless(1, 16.0);
    app
}

#[test]
fn the_module_plans_patches_in_view_and_presses_with_tramplers() {
    let mut app = grass_app();
    {
        let g = app.world.res::<Grass>().clone();
        assert!(g.add_patch(Patch::new(Kind::Meadow, [-30.0, -30.0], [0.0, 30.0])).is_some());
        assert!(g.add_patch(Patch::new(Kind::Wheat, [0.0, -30.0], [30.0, 30.0])).is_some());
        assert!(g.add_patch(Patch::new(Kind::Lawn, [0.0, 0.0], [0.0, 5.0])).is_none(), "an empty patch is refused");
        g.trample(Trampler { id: 1, a: [0.0, 0.0, -5.0], b: [0.0, 1.8, -5.0], radius: 0.35 });
    }
    app.world.res_mut::<RenderWorld>().camera = camera([0.0, 1.7, 8.0], [0.0, 0.5, -5.0]);
    app.run_headless(1, 16.0);
    let g = app.world.res::<Grass>().clone();
    let st = g.stats();
    assert!(st.tiles > 10 && st.candidates > 1_000, "{st:?}");
    let m = app.world.res::<ParamRegistry>();
    assert!(m.id("material.grass.wetness").is_some());
    drop(m);
    // Off: nothing planned.
    app.world.res_mut::<ParamRegistry>().set_by_name("material.grass.enabled", ParamValue::Bool(false)).unwrap();
    app.run_headless(1, 16.0);
    assert_eq!(g.stats().tiles, 0);
}

#[test]
fn a_trampler_is_stamped_swept_from_last_frame() {
    let m = GrassModule::new();
    let shared = m.shared.clone();
    let g = m.grass();
    let mut app = App::new("t").with_module(RenderModule::new()).with_module(m);
    app.run_headless(1, 16.0);
    g.add_patch(Patch::new(Kind::Meadow, [-20.0, -20.0], [20.0, 20.0]));
    app.world.res_mut::<RenderWorld>().camera = camera([0.0, 1.7, 8.0], [0.0, 0.5, 0.0]);
    g.trample(Trampler { id: 7, a: [0.0, 0.0, 0.0], b: [0.0, 1.8, 0.0], radius: 0.3 });
    app.run_headless(1, 16.0);
    g.trample(Trampler { id: 7, a: [0.5, 0.0, 0.0], b: [0.5, 1.8, 0.0], radius: 0.3 });
    app.run_headless(1, 16.0);
    let sh = shared.borrow();
    assert!(sh.on);
    assert_eq!(sh.sim.f(at::STAMPS_N), 1.0);
    let s = at::STAMPS;
    assert_eq!([sh.sim.f(s), sh.sim.f(s + 1), sh.sim.f(s + 2), sh.sim.f(s + 3)], [0.0, 0.0, 0.5, 0.0], "from last frame's spot to this one's");
    assert_eq!([sh.sim.f(s + 6), sh.sim.f(s + 7)], [1.0, 0.0], "moving along +x");
    // The window follows the trampler, in whole cells.
    assert_eq!(sh.sim.f(at::TRAMPLE + 2), sh.cfg.trample_cell);
    assert!(sh.jobs.len() > 10);
    drop(sh);
    // No trampler next frame: no stamp, the window keeps relaxing.
    app.run_headless(1, 16.0);
    assert_eq!(shared.borrow().sim.f(at::STAMPS_N), 0.0);
}

/// What the live tools do (`param.set` through the control API): a changed
/// `material.grass.*` value reaches the GPU data on the next frame, with no
/// rebuild.
#[test]
fn a_live_param_change_shows_on_the_next_frame() {
    let m = GrassModule::new();
    let shared = m.shared.clone();
    let g = m.grass();
    let mut app = App::new("t").with_module(RenderModule::new()).with_module(m);
    app.run_headless(1, 16.0);
    g.add_patch(Patch::new(Kind::Meadow, [-20.0, -20.0], [20.0, 20.0]));
    app.world.res_mut::<RenderWorld>().camera = camera([0.0, 1.7, 8.0], [0.0, 0.5, 0.0]);
    app.run_headless(1, 16.0);
    let kind_word = |sh: &Shared, i: usize| sh.sim.f(at::KINDS + i);
    assert_eq!(shared.borrow().shade.wetness, 0.0);
    {
        let mut reg = app.world.res_mut::<ParamRegistry>();
        reg.set_by_name("material.grass.wetness", ParamValue::F32(0.8)).unwrap();
        reg.set_by_name("material.grass.meadow.height", ParamValue::F32(1.1)).unwrap();
        reg.set_by_name("material.grass.wind.strength", ParamValue::F32(2.0)).unwrap();
        reg.set_by_name("material.grass.trample.keep_flat", ParamValue::Bool(true)).unwrap();
    }
    app.run_headless(1, 16.0);
    let sh = shared.borrow();
    assert_eq!(sh.shade.wetness, 0.8);
    assert_eq!(sh.sim.f(at::LOD2 + 2), 0.8, "wetness in the generation uniform");
    assert_eq!(kind_word(&sh, 0), 1.1, "meadow height");
    assert_eq!(sh.sim.f(at::WIND + 2), 2.0, "wind strength");
    assert_eq!(sh.sim.f(at::TRAMPLE2 + 2), 1.0, "kept flat: no relaxing");
}

/// The trample window follows the camera, not the walker: a path stays in
/// it (and so stays pressed, or kept flat) while it is in view, however far
/// the walker has gone.
#[test]
fn the_trample_window_stays_in_front_of_the_camera_when_the_walker_leaves() {
    let m = GrassModule::new();
    let shared = m.shared.clone();
    let g = m.grass();
    let mut app = App::new("t").with_module(RenderModule::new()).with_module(m);
    app.run_headless(1, 16.0);
    g.add_patch(Patch::new(Kind::Meadow, [-60.0, -60.0], [60.0, 60.0]));
    app.world.res_mut::<RenderWorld>().camera = camera([0.0, 5.0, 8.0], [0.0, 0.0, 0.0]);
    let origin = |sh: &Shared| [sh.sim.f(at::TRAMPLE), sh.sim.f(at::TRAMPLE + 1)];
    g.trample(Trampler { id: 1, a: [0.0, 0.0, 0.0], b: [0.0, 1.8, 0.0], radius: 0.45 });
    app.run_headless(1, 16.0);
    let first = origin(&shared.borrow());
    for step in 1..=20 {
        g.trample(Trampler { id: 1, a: [step as f32 * 2.0, 0.0, 0.0], b: [step as f32 * 2.0, 1.8, 0.0], radius: 0.45 });
        app.run_headless(1, 16.0);
    }
    let sh = shared.borrow();
    assert_eq!(origin(&sh), first, "the window did not follow the walker 40 m away");
    // In front of the eye: the window's middle is between the eye and the target.
    let mid_z = (first[1] + sh.cfg.trample_n as f32 / 2.0) * sh.cfg.trample_cell;
    assert!(mid_z > 0.0 && mid_z < 8.0, "{mid_z}");
}
