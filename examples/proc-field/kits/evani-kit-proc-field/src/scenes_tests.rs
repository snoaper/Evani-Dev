//! The render app's cameras, test scenes and measures (kit feature
//! `commons`): the player camera's placement, the two-view presets, fixed
//! sim time, each test scene, the measure line and its spec ids against
//! catalog/procedural/ref_classes.json, and the library's own wind and
//! current measured against their spec ranges.

use evani_core::App;
use evani_core::params::{ParamRegistry, ParamValue};
use evani_procedural::{AssetInfo, Category, FnAsset, Output, ParamSpec as P, Part, ProcModule, Registry, Values};
use evani_render::mesh::Mesh;
use evani_render::{RenderModule, RenderWorld};

use crate::ProcFieldKit;
use crate::commons::Staging;
use crate::scenes;

/// A closed box from `lo` to `hi` (outward, counter-clockwise).
fn cuboid(lo: [f32; 3], hi: [f32; 3]) -> Mesh {
    let c = |i: u32| [if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] }, 0.0, 1.0, 0.0];
    let vertices = (0..8).map(c).collect();
    #[rustfmt::skip]
    let indices = vec![
        0, 2, 1, 1, 2, 3, // -z
        4, 5, 6, 5, 7, 6, // +z
        0, 1, 4, 1, 5, 4, // -y
        2, 6, 3, 3, 6, 7, // +y
        0, 4, 2, 2, 4, 6, // -x
        1, 3, 5, 3, 7, 5, // +x
    ];
    Mesh { vertices, indices }
}

const KNEES: &[P] = &[P::num("knees", 5.0, 0.0, 70.0)];
const FACE: &[P] = &[P::num("smile", 0.0, -1.0, 1.0), P::num("open", 0.0, 0.0, 1.0), P::num("brows", 0.0, -1.0, 1.0), P::num("blink", 0.0, 0.0, 1.0)];

/// A slab 20 m by 10 m, 2 m high: a landform with a flat top.
fn slab(_v: &Values, _s: u32) -> Output {
    Output::Parts(vec![Part::new(cuboid([-10.0, 0.0, -5.0], [10.0, 2.0, 5.0]), [0.5; 3])])
}

/// A body whose height drops with its knees (70 degrees: three quarters).
fn body(v: &Values, _s: u32) -> Output {
    let h = 2.0 - 0.5 * v.f("knees") / 70.0;
    Output::Parts(vec![Part::new(cuboid([-0.3, 0.0, -0.2], [0.3, h, 0.2]), [0.5; 3])])
}

/// A face: a grid whose corners lift with the smile on both sides alike
/// (`asym`: the left side only).
fn face_mesh(v: &Values, asym: bool) -> Output {
    let mut m = Mesh::default();
    for j in 0..5 {
        for i in 0..9 {
            let x = -0.08 + 0.02 * i as f32;
            let w = if asym { x.max(0.0) } else { x.abs() };
            let y = 0.02 * j as f32 + v.f("smile") * 0.5 * w + v.f("open") * 0.01 * (j as f32 - 2.0).abs();
            m.vertices.push([x, y, 0.1, 0.0, 0.0, 1.0]);
        }
    }
    for j in 0..4u32 {
        for i in 0..8u32 {
            let a = j * 9 + i;
            m.indices.extend_from_slice(&[a, a + 1, a + 9, a + 1, a + 10, a + 9]);
        }
    }
    Output::Parts(vec![Part::new(m, [0.5; 3])])
}

/// Gust fronts at known uneven times, 6 m/s along +x, over a 3 m/s breeze.
fn breeze(_v: &Values, _s: u64, p: [f32; 3], t: f32) -> [f32; 3] {
    const STARTS: [f32; 10] = [3.0, 9.0, 13.5, 22.0, 27.0, 36.0, 40.0, 49.5, 55.0, 63.0];
    let mut g = 0.0;
    for s in STARTS {
        let tau = t - s - (p[0] + 15.0) / 6.0;
        if tau > 0.0 && tau < 3.0 {
            g += (std::f32::consts::PI * tau / 3.0).sin().powi(2);
        }
    }
    let turb = (std::f32::consts::TAU * 0.9 * t - 0.3 * p[0]).sin();
    [3.0 * (1.0 + 0.2 * turb) + 3.6 * g, 0.0, 0.0]
}

/// The library's flow in miniature: a channel profile and a lee eddy.
fn brook(_v: &Values, _s: u64, c: &evani_procedural::behaviour::Channel, p: [f32; 3], _t: f32) -> [f32; 2] {
    let zr = (p[2] / c.half_width).clamp(-1.0, 1.0);
    let mut u = [1.0 * (1.0 - zr * zr), 0.0];
    for &([cx, cz], r) in &c.rocks {
        let (ex, ez) = (p[0] - (cx + 2.0 * r), p[2] - cz);
        let q = (ex * ex + ez * ez) / (4.0 * r * r);
        let w = (-q * q).exp();
        let om = std::f32::consts::TAU / 4.0;
        u = [u[0] * (1.0 - w) - om * ez * w, u[1] * (1.0 - w) + om * ex * w];
    }
    u
}

fn registry(asset: FnAsset) -> Registry {
    let mut r = Registry::new();
    r.add_category(Category { id: "test", name: "Test" });
    r.add(Box::new(asset));
    r.behaviours_mut().add_wind("test-breeze", breeze);
    r.behaviours_mut().add_flow("test-brook", brook);
    r
}

fn app_with(asset: FnAsset) -> App {
    let mut a = App::new("t").with_module(RenderModule::new()).with_module(ProcModule::new(registry(asset))).with_kit(ProcFieldKit);
    a.start();
    a
}

fn asset(id: &'static str, params: &'static [P], make: fn(&Values, u32) -> Output) -> FnAsset {
    FnAsset { info: AssetInfo::cpu("test", id, id), params, make }
}

fn set(a: &mut App, name: &str, v: f32) {
    let mut reg = a.world.res_mut::<ParamRegistry>();
    let id = reg.id(name).unwrap_or_else(|| panic!("no param {name}"));
    reg.set(id, ParamValue::F32(v)).unwrap_or_else(|e| panic!("{name}: {e:?}"));
}

fn get(a: &App, name: &str) -> f32 {
    let reg = a.world.res::<ParamRegistry>();
    match reg.id(name).and_then(|id| reg.value(id)) {
        Some(ParamValue::F32(x)) => x,
        v => panic!("{name}: {v:?}"),
    }
}

fn camera(a: &App) -> evani_core::camera::CameraPose {
    a.world.res::<RenderWorld>().camera
}

fn near(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() <= tol
}

#[test]
fn the_defaults_keep_the_fixed_camera_and_draw_nothing_new() {
    let mut a = app_with(asset("slab", &[], slab));
    a.run_headless(3, 1000.0 / 60.0);
    let c = camera(&a);
    assert_eq!((c.eye, c.target, c.fov_y_deg), ([0.0, 16.0, 34.0], [0.0, 1.0, 0.0], 50.0));
    assert!(a.world.res::<Staging>().instances.is_empty());
    assert!(a.world.res::<Staging>().line.is_none());
    // The orbit is the one it was: framed on the bounds at the fixed pitch.
    set(&mut a, "kit.proc_field.cam_free", 1.0);
    a.run_headless(1, 1000.0 / 60.0);
    let c = camera(&a);
    let r = (100.0f32 + 1.0 + 25.0).sqrt();
    let d = r / 25f32.to_radians().sin() * 1.1;
    let p = 23.81f32.to_radians();
    assert_eq!(c.target, [0.0, 1.0, 0.0]);
    assert!(near(c.eye[1], 1.0 + d * p.sin(), 1e-3) && near(c.eye[2], d * p.cos(), 1e-3) && c.fov_y_deg == 50.0, "{c:?}");
}

#[test]
fn the_player_camera_stands_on_the_ground_at_cam_stand() {
    let mut a = app_with(asset("slab", &[], slab));
    for (k, v) in [("cam_free", 2.0), ("cam_yaw_deg", 90.0), ("cam_stand", 0.5), ("cam_pitch_deg", 0.0)] {
        set(&mut a, &format!("kit.proc_field.{k}"), v);
    }
    a.run_headless(2, 1000.0 / 60.0);
    // Half the horizontal radius (sqrt(10^2 + 5^2)) out on +x: on the
    // slab's top (2 m), eye 1.7 m above it, looking level towards -x.
    let rh = 125f32.sqrt();
    let c = camera(&a);
    assert!(near(c.eye[0], 0.5 * rh, 1e-3) && near(c.eye[1], 3.7, 1e-3) && near(c.eye[2], 0.0, 1e-3), "{c:?}");
    assert!(near(c.target[0] - c.eye[0], -1.0, 1e-4) && near(c.target[1], c.eye[1], 1e-4), "{c:?}");
    // 60 degrees across a 3:2 picture.
    assert!(near(c.fov_y_deg, (2.0 * (30f32.to_radians().tan() / 1.5).atan()).to_degrees(), 1e-3), "{c:?}");
    // Outside the slab: on the floor. Looking up 10 degrees, a 2 m eye.
    set(&mut a, "kit.proc_field.cam_stand", 2.0);
    set(&mut a, "kit.proc_field.cam_pitch_deg", 10.0);
    set(&mut a, "kit.proc_field.cam_eye_m", 2.0);
    a.run_headless(1, 1000.0 / 60.0);
    let c = camera(&a);
    assert!(near(c.eye[0], 2.0 * rh, 1e-3) && near(c.eye[1], 2.0, 1e-3), "{c:?}");
    assert!(near(c.target[1] - c.eye[1], 10f32.to_radians().sin(), 1e-4), "{c:?}");
    // From the +z side at the centre: facing -z.
    set(&mut a, "kit.proc_field.cam_yaw_deg", 0.0);
    set(&mut a, "kit.proc_field.cam_stand", 0.0);
    a.run_headless(1, 1000.0 / 60.0);
    let c = camera(&a);
    assert!(near(c.eye[0], 0.0, 1e-3) && near(c.eye[1], 4.0, 1e-3) && c.target[2] < c.eye[2], "{c:?}");
}

#[test]
fn the_two_view_presets_frame_the_landform_from_above_and_inside() {
    let mut a = app_with(asset("slab", &[], slab));
    set(&mut a, "kit.proc_field.view_preset", 1.0);
    a.run_headless(2, 1000.0 / 60.0);
    // The overview: 45 degrees down, 30 degrees off the long (x) axis.
    let c = camera(&a);
    let v = [c.eye[0] - c.target[0], c.eye[1] - c.target[1], c.eye[2] - c.target[2]];
    let h = (v[0] * v[0] + v[2] * v[2]).sqrt();
    assert!(near(v[1].atan2(h).to_degrees(), 45.0, 0.01), "{c:?}");
    assert!(near(v[0].atan2(v[2]).to_degrees(), 120.0, 0.01), "{c:?}");
    assert!(near(c.target[1], 1.0, 1e-4));
    // The whole slab fits: its bounds sphere inside the narrower view.
    let r = 126f32.sqrt();
    let d = (h * h + v[1] * v[1]).sqrt();
    assert!(d * (c.fov_y_deg * 0.5).to_radians().sin() >= r, "{c:?}");
    // The inside view: 0.3 of the radius out on +x, on the top, level.
    set(&mut a, "kit.proc_field.view_preset", 2.0);
    a.run_headless(1, 1000.0 / 60.0);
    let c = camera(&a);
    assert!(near(c.eye[0], 0.3 * 125f32.sqrt(), 1e-3) && near(c.eye[1], 3.7, 1e-3), "{c:?}");
    assert!(near(c.target[1], c.eye[1], 1e-4) && c.target[0] < c.eye[0], "{c:?}");
}

#[test]
fn a_fixed_time_gives_the_same_frame_twice() {
    let mut a = app_with(asset("breeze", &[], slab));
    set(&mut a, "kit.proc_field.test_scene", 1.0);
    set(&mut a, "kit.proc_field.time_free", 1.0);
    set(&mut a, "kit.proc_field.time_s", 10.0);
    a.run_headless(2, 1000.0 / 60.0);
    let first = format!("{:?}", a.world.res::<Staging>().instances);
    set(&mut a, "kit.proc_field.time_s", 10.5);
    a.run_headless(3, 1000.0 / 60.0);
    let later = format!("{:?}", a.world.res::<Staging>().instances);
    set(&mut a, "kit.proc_field.time_s", 10.0);
    a.run_headless(7, 1000.0 / 60.0);
    let again = format!("{:?}", a.world.res::<Staging>().instances);
    assert_eq!(first, again, "the same time_s draws the same frame, whatever the sim clock did");
    assert_ne!(first, later, "half a second later the meadow has moved");
}

#[test]
fn the_wind_field_stages_the_meadow_moved_by_the_wind() {
    let mut a = app_with(asset("breeze", &[], slab));
    set(&mut a, "kit.proc_field.test_scene", 1.0);
    set(&mut a, "kit.proc_field.time_free", 1.0);
    set(&mut a, "kit.proc_field.time_s", 20.0);
    a.run_headless(2, 1000.0 / 60.0);
    // Its own camera: at the meadow's edge, eye height.
    let c = camera(&a);
    assert!(near(c.eye[1], 1.7, 1e-4) && c.eye[2] > scenes::MEADOW_HALF, "{c:?}");
    // No asset mesh: it acts only through its wind.
    assert!(a.world.res::<evani_procedural::ProcScene>().placements.is_empty());
    let inst = a.world.res::<Staging>().instances.clone();
    assert_eq!(inst.len(), 1 + 48 * 48 + 2 * 4 + 1 + 6);
    // The grass leans downwind (+x): the tufts' tips are displaced along +x.
    let lean: f32 = inst[1..1 + 48 * 48].iter().map(|m| m.rotation[2]).sum::<f32>();
    assert!(lean < 0.0, "a tuft leaning +x turns about -z: {lean}");
    // And drawn: the frame's draw adds every proxy.
    a.world.res_mut::<RenderWorld>().meshes.clear();
    crate::commons::draw_scene(&mut a.world);
    assert_eq!(a.world.res::<RenderWorld>().meshes.len(), inst.len());
}

#[test]
fn the_current_channel_carries_the_leaves_downstream() {
    let mut a = app_with(asset("brook", &[], slab));
    set(&mut a, "kit.proc_field.test_scene", 2.0);
    set(&mut a, "kit.proc_field.time_free", 1.0);
    set(&mut a, "kit.proc_field.time_s", 20.0);
    a.run_headless(2, 1000.0 / 60.0);
    let first = a.world.res::<Staging>().instances.clone();
    set(&mut a, "kit.proc_field.time_s", 20.5);
    a.run_headless(1, 1000.0 / 60.0);
    let later = a.world.res::<Staging>().instances.clone();
    assert_eq!(first.len(), 4 + 19);
    let moved = first[4..].iter().zip(&later[4..]).filter(|(p, q)| q.position[0] > p.position[0] + 0.05).count();
    assert!(moved >= 10, "most leaves drift downstream: {moved}");
    assert!(a.world.res::<evani_procedural::ProcScene>().placements.is_empty());
}

#[test]
fn the_pose_set_poses_the_asset_and_puts_it_back() {
    let mut a = app_with(asset("body", KNEES, body));
    set(&mut a, "kit.proc_field.test_scene", 3.0);
    set(&mut a, "kit.proc_field.pose", 2.0);
    a.run_headless(2, 1000.0 / 60.0);
    // The crouch: the table's 110 degrees, clamped to the asset's 70.
    assert_eq!(get(&a, "procedural.test.body.knees"), 70.0);
    assert_eq!(a.world.res::<evani_procedural::ProcScene>().placements.len(), 1);
    set(&mut a, "kit.proc_field.pose", 1.0);
    a.run_headless(1, 1000.0 / 60.0);
    assert_eq!(get(&a, "procedural.test.body.knees"), 30.0);
    set(&mut a, "kit.proc_field.measure", 1.0);
    a.run_headless(1, 1000.0 / 60.0);
    let line = a.world.res::<Staging>().line.clone().unwrap_or_default();
    // The crouch keeps three quarters of the rest volume; what needs the
    // rig's joints is null.
    assert_eq!(line, "evani-field-measure {\"elbow_range_deg\":null,\"knee_range_deg\":null,\"volume_kept\":0.75,\"intersection_cm\":null,\"foot_slide_cm\":null}");
    set(&mut a, "kit.proc_field.test_scene", 0.0);
    a.run_headless(1, 1000.0 / 60.0);
    assert_eq!(get(&a, "procedural.test.body.knees"), 5.0, "back to its own value");
}

#[test]
fn the_expression_set_measures_symmetry_from_the_vertices() {
    let mut a = app_with(asset("face", FACE, |v, _| face_mesh(v, false)));
    set(&mut a, "kit.proc_field.test_scene", 4.0);
    set(&mut a, "kit.proc_field.pose", 1.0);
    a.run_headless(2, 1000.0 / 60.0);
    assert_eq!(get(&a, "procedural.test.face.smile"), 1.0);
    set(&mut a, "kit.proc_field.pose", 2.0);
    a.run_headless(1, 1000.0 / 60.0);
    assert_eq!((get(&a, "procedural.test.face.smile"), get(&a, "procedural.test.face.open"), get(&a, "procedural.test.face.brows")), (0.0, 1.0, 1.0));
    set(&mut a, "kit.proc_field.measure", 1.0);
    a.run_headless(1, 1000.0 / 60.0);
    let line = a.world.res::<Staging>().line.clone().unwrap_or_default();
    assert_eq!(line, "evani-field-measure {\"symmetry_err\":0,\"blink_gap_mm\":null,\"lip_seal_mm\":null,\"teeth_clip_mm\":null}");
    // A one-sided smile is caught.
    let mut b = app_with(asset("face", FACE, |v, _| face_mesh(v, true)));
    set(&mut b, "kit.proc_field.test_scene", 4.0);
    set(&mut b, "kit.proc_field.measure", 1.0);
    b.run_headless(2, 1000.0 / 60.0);
    let line = b.world.res::<Staging>().line.clone().unwrap_or_default();
    assert!(!line.contains("\"symmetry_err\":0,") && !line.contains("\"symmetry_err\":null"), "{line}");
}

#[test]
fn the_wind_measures_read_the_proxies() {
    let m = scenes::wind_measures(&|p, t| breeze(&Values::default(), 0, p, t));
    let v = |id: &str| m.iter().find(|(k, _)| *k == id).and_then(|(_, v)| *v).unwrap_or_else(|| panic!("{id}: {m:?}"));
    assert!(v("direction_coherence") > 0.95, "{m:?}");
    assert!(v("flag_alignment_deg") < 5.0, "{m:?}");
    assert!(near(v("gust_front_speed"), 6.0, 1.5), "the fronts run at 6 m/s: {m:?}");
    // The starts in the window (5..65 s) after their 15..25 m run-in.
    assert!(near(v("gust_interval_s"), 6.9, 1.5), "{m:?}");
    assert!(v("gust_peak_ratio") > 1.35, "{m:?}");
    assert!((0.1..=0.6).contains(&v("tree_crown_hz")), "the crowns sway slowly: {m:?}");
    assert!(near(v("flag_flutter_hz"), 0.75 * 3.0, 0.6), "{m:?}");
    // No wind: nothing sways, nothing to time.
    let still = scenes::wind_measures(&|_, _| [0.0; 3]);
    let s = |id: &str| still.iter().find(|(k, _)| *k == id).and_then(|(_, v)| *v);
    assert_eq!(s("grass_sway_amp"), Some(0.0));
    assert_eq!((s("grass_sway_hz"), s("gust_peak_ratio"), s("gust_interval_s"), s("tree_crown_hz")), (None, None, None, None));
}

#[test]
fn the_current_measures_read_the_markers() {
    let m = scenes::current_measures(&|c, p, t| brook(&Values::default(), 0, c, p, t));
    let v = |id: &str| m.iter().find(|(k, _)| *k == id).and_then(|(_, v)| *v).unwrap_or_else(|| panic!("{id}: {m:?}"));
    assert!(near(v("surface_speed"), 0.98, 0.05), "{m:?}");
    assert!(near(v("bank_ratio"), 0.44, 0.06), "1 - 0.75^2 = 0.4375: {m:?}");
    assert!(v("direction_follow_deg") < 3.0, "{m:?}");
    assert!(near(v("eddy_period_s"), 4.0, 1.0), "{m:?}");
}

#[test]
fn the_measure_line_is_json_with_null_for_unmeasured() {
    let m: scenes::Measures = vec![("a", Some(1.234_56)), ("b", None), ("c", Some(f32::NAN)), ("d", Some(-2.0))];
    assert_eq!(scenes::measure_line(&m), "evani-field-measure {\"a\":1.2346,\"b\":null,\"c\":null,\"d\":-2}");
}

/// The spec ids per test scene are ref_classes.json's, in its order.
#[test]
fn the_spec_ids_match_the_catalogue() {
    let cat = include_str!("../../../../../catalog/procedural/ref_classes.json");
    let ids_of = |scene: &str| -> Vec<String> {
        let at = cat.find(&format!("\"test_scene\": \"{scene}\"")).unwrap_or_else(|| panic!("{scene}"));
        let rest = &cat[at..];
        let end = rest.find("] }").unwrap_or(rest.len());
        rest[..end].split("{ \"id\": \"").skip(1).map(|s| s.split('"').next().unwrap_or("").to_string()).collect()
    };
    for (scene, ids) in scenes::SCENES.iter().zip([&scenes::WIND_IDS[..], &scenes::CURRENT_IDS[..], &scenes::POSE_IDS[..], &scenes::EXPRESSION_IDS[..]]) {
        assert_eq!(ids_of(scene), ids.to_vec(), "{scene}");
        assert!(cat.contains(&format!("\"{scene}\": \"")), "{scene} is a test scene");
    }
}

/// The library's own wind gusts and water currents pass their spec.
#[test]
fn the_library_wind_and_current_pass_their_spec() {
    let lib = evani_procedural_lib::registry();
    let cat = include_str!("../../../../../catalog/procedural/ref_classes.json");
    let range = |id: &str| -> (f32, f32) {
        let at = cat.find(&format!("{{ \"id\": \"{id}\"")).unwrap_or_else(|| panic!("{id}"));
        let r = &cat[at..];
        let r = &r[r.find("\"range\": [").unwrap_or(0) + 10..];
        let r = &r[..r.find(']').unwrap_or(0)];
        let mut it = r.split(',').map(|x| x.trim().parse::<f32>().unwrap_or(f32::NAN));
        (it.next().unwrap_or(f32::NAN), it.next().unwrap_or(f32::NAN))
    };
    let mut fails = Vec::new();
    let defaults = |f: &str| {
        let a = lib.get(f).unwrap_or_else(|| panic!("{f}"));
        Values::defaults(&Registry::specs(a))
    };
    let wv = defaults("sky-wind-gust-effects");
    let wind = lib.behaviours().wind("sky-wind-gust-effects").unwrap_or_else(|| panic!("no wind"));
    let mut all = scenes::wind_measures(&|p, t| wind(&wv, 1, p, t));
    let fv = defaults("water-water-currents");
    let flow = lib.behaviours().flow("water-water-currents").unwrap_or_else(|| panic!("no flow"));
    all.extend(scenes::current_measures(&|c, p, t| flow(&fv, 1, c, p, t)));
    for (id, v) in &all {
        let (lo, hi) = range(id);
        match v {
            Some(x) if *x >= lo && *x <= hi => {}
            _ => fails.push(format!("{id} = {v:?} not in [{lo}, {hi}]")),
        }
    }
    assert!(fails.is_empty(), "{fails:#?}\nall: {all:?}");
}
