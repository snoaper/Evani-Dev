//! `debug.geometry` on a native app: a tiny scene with two overlapping
//! boxes and a floating one, drawn into `RenderWorld` every frame.

use evani_core::App;
use evani_core::camera::CameraPose;
use evani_core::capture::{CaptureQueue, CaptureResult};
use evani_core::overlay::RenderStats;
use evani_core::ecs::{Stage, SystemDesc, World};
use evani_render::mesh::MeshId;
use evani_render::world::{MeshInstance, RenderWorld};
use serde_json::{Value, json};

use crate::geometry::{GREY, GeometryView, SKY};
use crate::{LiveModule, submit, take_replies};

const DT: f64 = 16.0;
const WHITE: [f32; 4] = [0.9, 0.8, 0.7, 1.0];
const RED: [f32; 3] = [1.0, 0.1, 0.1];
const YELLOW: [f32; 3] = [1.0, 0.9, 0.1];

fn cube(p: [f32; 3], s: [f32; 3]) -> MeshInstance {
    let mut m = MeshInstance::new(MeshId::CUBE, p);
    m.scale = s;
    m.color = WHITE;
    m
}

/// Ground, box A, a floating box C, box B overlapping A (drawn apart from
/// A, so they are two objects).
fn draw(world: &mut World) {
    let mut rw = world.res_mut::<RenderWorld>();
    rw.clear();
    rw.viewport_css = [1280.0, 720.0];
    rw.draw_mesh(cube([0.0, -0.5, 0.0], [40.0, 1.0, 40.0]));
    rw.draw_mesh(cube([0.0, 0.5, 0.0], [1.0, 1.0, 1.0]));
    rw.draw_mesh(cube([10.0, 2.0, 10.0], [1.0, 1.0, 1.0]));
    rw.draw_mesh(cube([0.6, 0.5, 0.0], [1.0, 1.0, 1.0]));
}

struct G {
    a: App,
    token: u64,
}

impl G {
    fn new(renderer: bool) -> Self {
        Self::with(renderer, draw)
    }

    fn with(renderer: bool, scene: fn(&mut World)) -> Self {
        let mut a = App::new("sandbox").with_module(LiveModule::new());
        if renderer {
            a.world.insert_resource(RenderWorld::default());
            a = a.with_system(SystemDesc::new("test.draw", Stage::Frame, scene));
        }
        a.run_headless(2, DT);
        Self { a, token: 0 }
    }

    fn call(&mut self, params: Value) -> Value {
        self.token += 1;
        let req = json!({ "type": "cmd", "v": 1, "id": format!("g-{}", self.token), "cmd": "debug.geometry", "params": params });
        submit(&self.a.world, self.token, &req.to_string());
        for _ in 0..10 {
            self.a.run_headless(1, DT);
            if let Some((_, text)) = take_replies(&self.a.world).into_iter().find(|(t, _)| *t == self.token) {
                return serde_json::from_str(&text).unwrap();
            }
        }
        panic!("no reply");
    }

    fn ok(&mut self, params: Value) -> Value {
        let r = self.call(params);
        assert_eq!(r["ok"], true, "{r}");
        r["result"].clone()
    }

    fn err(&mut self, params: Value) -> String {
        let r = self.call(params);
        assert_eq!(r["ok"], false, "{r}");
        r["error"]["code"].as_str().unwrap().to_string()
    }

    fn colors(&self) -> Vec<[f32; 3]> {
        let rw = self.a.world.res::<RenderWorld>();
        rw.meshes.iter().map(|m| [m.color[0], m.color[1], m.color[2]]).collect()
    }

    fn clear(&self) -> Option<[f32; 3]> {
        self.a.world.res::<CaptureQueue>().clear_override
    }

    fn plain(&self) -> bool {
        self.a.world.res::<CaptureQueue>().plain_materials
    }
}

#[test]
fn view_tints_overlaps_red_floating_yellow_the_rest_grey_and_paints_the_sky() {
    let mut g = G::new(true);
    let r = g.ok(json!({ "on": true }));
    assert_eq!(r["on"], true);
    assert_eq!(r["sky"], true);
    assert!(r["counts"]["overlap"].as_u64().unwrap() >= 1, "{r}");
    assert!(r["counts"]["floating"].as_u64().unwrap() >= 1, "{r}");
    assert_eq!(r["instances"], 4);
    let issues = r["issues"].as_array().unwrap();
    let overlap = issues.iter().find(|i| i["check"] == "overlap").unwrap();
    let mut items: Vec<u64> = overlap["items"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    items.sort_unstable();
    assert_eq!(items, vec![1, 3], "boxes A and B");
    assert!(!overlap["fix"].as_str().unwrap().is_empty());
    assert_eq!(r["colors"]["overlap"], "#ff1a1a");
    g.a.run_headless(1, DT);
    let c = g.colors();
    assert_eq!(c[0], GREY, "the ground has no issue");
    assert_eq!(c[1], RED);
    assert_eq!(c[2], YELLOW);
    assert_eq!(c[3], RED);
    assert_eq!(g.clear(), Some(SKY));
    assert!(g.plain(), "plain materials, so the tints show as they are");
    // Off: the overrides go back and the kit's colours return.
    let r = g.ok(json!({ "on": false }));
    assert_eq!(r["on"], false);
    assert_eq!(g.clear(), None);
    assert!(!g.plain());
    g.a.run_headless(1, DT);
    assert!(g.colors().iter().all(|c| *c == [WHITE[0], WHITE[1], WHITE[2]]));
}

#[test]
fn a_query_while_off_lists_issues_without_tinting() {
    let mut g = G::new(true);
    let r = g.ok(json!({}));
    assert_eq!(r["on"], false);
    assert!(r["total"].as_u64().unwrap() >= 2, "{r}");
    g.a.run_headless(1, DT);
    assert!(g.colors().iter().all(|c| *c == [WHITE[0], WHITE[1], WHITE[2]]));
    assert_eq!(g.clear(), None);
    assert!(!g.a.world.res::<GeometryView>().on);
}

#[test]
fn focus_frames_an_issue_and_gives_the_camera_back() {
    let mut g = G::new(true);
    let r = g.ok(json!({ "on": true, "sky": false, "focus": 0 }));
    assert_eq!(r["focus"], 0);
    assert_eq!(r["sky"], false);
    assert_eq!(g.clear(), None, "sky off");
    let pose = g.a.world.res::<CaptureQueue>().camera_override.expect("focus camera");
    let at = &r["issues"][0]["at"];
    let target = [at[0].as_f64().unwrap(), at[1].as_f64().unwrap(), at[2].as_f64().unwrap()];
    let d: f64 = (0..3).map(|k| (f64::from(pose.target[k]) - target[k]).powi(2)).sum::<f64>().sqrt();
    assert!(d < 2.0, "the camera looks at the issue ({pose:?} vs {target:?})");
    // Out of range: no focus.
    let r = g.ok(json!({ "focus": 99 }));
    assert_eq!(r["focus"], Value::Null);
    assert!(g.a.world.res::<CaptureQueue>().camera_override.is_none());
    let _ = g.ok(json!({ "focus": 0 }));
    assert!(g.a.world.res::<CaptureQueue>().camera_override.is_some());
    let _ = g.ok(json!({ "focus": null }));
    assert!(g.a.world.res::<CaptureQueue>().camera_override.is_none());
    let _ = g.ok(json!({ "focus": 0 }));
    let _ = g.ok(json!({ "on": false }));
    assert!(g.a.world.res::<CaptureQueue>().camera_override.is_none());
}

#[test]
fn tags_skip_intentional_cases_and_checks_can_be_chosen() {
    let mut g = G::new(true);
    let r = g.ok(json!({ "on": true, "tags": [{ "check": "floating", "object": "*", "reason": "a balloon" }] }));
    assert_eq!(r["counts"]["floating"], 0, "{r}");
    assert!(r["counts"]["tagged"].as_u64().unwrap() >= 1);
    let tagged = r["issues"].as_array().unwrap().iter().find(|i| i["check"] == "floating").unwrap();
    assert_eq!(tagged["tagged"], "a balloon");
    g.a.run_headless(1, DT);
    assert_eq!(g.colors()[2], GREY, "tagged issues are not painted");
    let r = g.ok(json!({ "checks": ["overlap"] }));
    assert_eq!(r["checks"], json!(["overlap"]));
    assert!(r["issues"].as_array().unwrap().iter().all(|i| i["check"] == "overlap"));
}

/// What a fake renderer saw when it took a capture request.
struct Shot {
    colors: Vec<[f32; 3]>,
    plain: bool,
    clear: Option<[f32; 3]>,
}

#[test]
fn a_geometry_capture_shows_check_colours_and_grey_on_plain_materials() {
    let mut g = G::new(true);
    g.token += 1;
    let req = json!({ "type": "cmd", "v": 1, "id": "cap", "cmd": "capture.frame",
                      "params": { "geometry": true, "sky": true } });
    submit(&g.a.world, g.token, &req.to_string());
    let mut shot: Option<Shot> = None;
    let mut inflight: Vec<CaptureResult> = Vec::new();
    let mut reply = None;
    for _ in 0..300 {
        // The fake renderer: finish last frame's captures, then draw this
        // frame (what the systems left in RenderWorld) and take requests.
        {
            let mut q = g.a.world.res_mut::<CaptureQueue>();
            for r in inflight.drain(..) {
                q.complete(r);
            }
            for r in q.take_pending() {
                shot = Some(Shot {
                    colors: g.colors(),
                    plain: q.plain_materials,
                    clear: q.clear_override,
                });
                inflight.push(CaptureResult {
                    id: r.id,
                    rgba: Ok(vec![128; 8 * 4 * 4]),
                    width: 8,
                    height: 4,
                    frame: 0,
                    sim_time: 0.0,
                    stats: RenderStats::default(),
                    camera: Some(CameraPose::default()),
                    canvas_hud: None,
                });
            }
        }
        g.a.run_headless(1, DT);
        if let Some((_, text)) = take_replies(&g.a.world).into_iter().find(|(t, _)| *t == g.token) {
            reply = Some(serde_json::from_str::<Value>(&text).unwrap());
            break;
        }
    }
    let r = reply.expect("the capture answered");
    assert_eq!(r["ok"], true, "{r}");
    assert!(r["result"]["geometry"]["total"].as_u64().unwrap() >= 2, "{r}");
    let shot = shot.expect("a frame was captured");
    assert!(shot.plain, "the shot is drawn with plain materials");
    assert_eq!(shot.clear, Some(SKY));
    assert_eq!(shot.colors, vec![GREY, RED, YELLOW, RED], "issues in their colour, the rest grey");
    // The view was off before the capture: everything is given back.
    assert!(!g.plain());
    assert_eq!(g.clear(), None);
    g.a.run_headless(1, DT);
    assert!(g.colors().iter().all(|c| *c == [WHITE[0], WHITE[1], WHITE[2]]));
}

#[test]
fn params_are_checked_and_a_build_without_a_renderer_says_so() {
    let mut g = G::new(true);
    assert_eq!(g.err(json!({ "checks": ["gap"] })), "invalid_value");
    assert_eq!(g.err(json!({ "checks": ["nope"] })), "invalid_value");
    assert_eq!(g.err(json!({ "checks": [] })), "invalid_value");
    assert_eq!(g.err(json!({ "on": 1 })), "invalid_value");
    assert_eq!(g.err(json!({ "every_s": 0 })), "invalid_value");
    assert_eq!(g.err(json!({ "focus": "a" })), "invalid_value");
    assert_eq!(g.err(json!({ "tags": [{ "check": "overlap" }] })), "invalid_value", "a tag needs a reason");
    assert_eq!(g.err(json!({ "thresholds": { "nope": 1 } })), "invalid_value");
    assert!(!g.a.world.res::<GeometryView>().on, "a refused request changes nothing");
    let mut g = G::new(false);
    assert_eq!(g.err(json!({ "on": true })), "unavailable");
}

/// The first custom mesh id: ids follow the built-ins.
const FIRST_CUSTOM: MeshId = MeshId(evani_render::mesh::BUILTIN_MESH_COUNT as u16);

/// Ground, a swept road (the first custom mesh) with a box sunk into it, a decal
/// 10 cm above the ground and a decal lying on it.
fn draw_road_and_decals(world: &mut World) {
    let mut rw = world.res_mut::<RenderWorld>();
    rw.clear();
    rw.viewport_css = [1280.0, 720.0];
    rw.draw_mesh(cube([0.0, -0.5, 0.0], [40.0, 1.0, 40.0]));
    let mut road = MeshInstance::new(FIRST_CUSTOM, [0.0; 3]);
    road.color = WHITE;
    rw.draw_mesh(road);
    rw.draw_mesh(cube([0.0, 0.5, -15.0], [1.0, 1.0, 1.0]));
    let mut up = MeshInstance::new(MeshId::PLANE, [10.0, 0.1, 5.0]);
    up.scale = [2.0, 1.0, 2.0];
    up.color = WHITE;
    rw.draw_decal(up);
    let mut on = MeshInstance::new(MeshId::PLANE, [-10.0, 0.0, 5.0]);
    on.scale = [2.0, 1.0, 2.0];
    on.color = WHITE;
    rw.draw_decal(on);
}

#[test]
fn custom_meshes_are_analysed_and_decals_are_judged_by_the_surface_they_lie_on() {
    use evani_render::mesh::MeshLibrary;
    use evani_render::spline::{Path, Profile, sweep};
    let mut g = G::with(true, draw_road_and_decals);
    let mut lib = MeshLibrary::default();
    let path = Path::new(vec![[-10.0, 0.3, -15.0], [10.0, 0.3, -15.0]], false);
    assert_eq!(lib.add(sweep(&path.frames(2.0), &Profile::slab(3.0, 0.3), false)), Some(FIRST_CUSTOM));
    g.a.world.insert_resource(lib);
    let r = g.ok(json!({ "on": true }));
    assert_eq!(r["instances"], 5, "three meshes and two decals");
    let issues = r["issues"].as_array().unwrap();
    // The box sunk into the custom road is found (items 1 and 2).
    let overlap = issues.iter().find(|i| i["check"] == "overlap").unwrap_or_else(|| panic!("no overlap: {r}"));
    let mut items: Vec<u64> = overlap["items"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    items.sort_unstable();
    assert_eq!(items, vec![1, 2], "the road and the box: {r}");
    // The decal 10 cm up (item 3) is off its surface; the one on the ground (item 4) is not;
    // neither fights the ground.
    let off: Vec<&Value> = issues.iter().filter(|i| i["kind"] == "decal off surface").collect();
    assert_eq!(off.len(), 1, "{r}");
    assert_eq!(off[0]["items"], json!([3]));
    assert_eq!(off[0]["check"], "floating");
    assert!(issues.iter().all(|i| i["check"] != "zfight"), "{r}");
    // Everything the road is made of was understood: nothing is skipped.
    assert!(r["notes"].as_array().unwrap().is_empty(), "{r}");
    // The tint reaches the decals.
    g.a.run_headless(1, DT);
    let rw = g.a.world.res::<RenderWorld>();
    let c = |m: &MeshInstance| [m.color[0], m.color[1], m.color[2]];
    assert_eq!(c(&rw.decals[0]), YELLOW);
    assert_eq!(c(&rw.decals[1]), GREY);
    assert_eq!(c(&rw.meshes[1]), RED);
}

/// The ground and a darker slab whose top is the ground's top (coplanar),
/// seen by the default camera from 18 m.
fn draw_coplanar(world: &mut World) {
    let mut rw = world.res_mut::<RenderWorld>();
    rw.clear();
    rw.viewport_css = [1280.0, 720.0];
    rw.draw_mesh(cube([0.0, -0.5, 0.0], [40.0, 1.0, 40.0]));
    rw.draw_mesh(cube([12.0, 2.0, -12.0], [1.0, 1.0, 1.0]));
    let mut slab = cube([0.0, -0.25, 0.0], [4.0, 0.5, 4.0]);
    slab.color = [0.2, 0.2, 0.2, 1.0];
    rw.draw_mesh(slab);
}

#[test]
fn coplanar_faces_fight_by_the_depth_maths_from_the_live_camera() {
    let mut g = G::with(true, draw_coplanar);
    let r = g.ok(json!({ "checks": ["zfight"] }));
    let issues = r["issues"].as_array().unwrap();
    let z = issues.iter().find(|i| i["check"] == "zfight").unwrap_or_else(|| panic!("no zfight: {r}"));
    assert_eq!(z["kind"], "coplanar faces", "{r}");
    let mut items: Vec<u64> = z["items"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    items.sort_unstable();
    assert_eq!(items, vec![0, 2], "the ground and the slab: {r}");
    let d = &z["data"];
    assert!(d["epsilon_m"].as_f64().unwrap() > 0.0, "{d}");
    assert_eq!(d["viewer"]["label"], "live", "{d}");
    assert!(d["polygon"].as_array().unwrap().len() >= 3, "{d}");
    // The readable summary lists it too.
    let s = r["summary"].as_str().unwrap();
    assert!(s.starts_with(&format!("{} issue(s)", r["total"])), "{s}");
    assert!(s.contains("zfight, coplanar faces"), "{s}");
}
