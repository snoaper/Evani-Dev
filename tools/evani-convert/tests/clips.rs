//! STAGE3_PLAN S3-7's clip baking tests, on the rig fixture
//! (`tests/fixtures/models/rig.src.glb`, written by `evani_convert::fixture`).
//! All but the last run without the external tools: the baker is plain Rust.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code: a failed step is a failed test

use std::path::{Path, PathBuf};

use evani_convert::clips::{Baked, bake};
use evani_convert::{Options, convert, fixture, glb::Doc, tools::Tools};
use evani_gltf::Gltf;
use evani_guide::clipblock::{self, ROTATION, SCALE, TRANSLATION};
use serde_json::Value;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap().to_path_buf()
}

fn rig() -> Gltf {
    Gltf::from_glb(&fixture::rig_glb().unwrap()).unwrap()
}

fn node(g: &Gltf, name: &str) -> u16 {
    g.node_by_name(name).unwrap() as u16
}

fn baked(g: &Gltf, bound: f64) -> Baked {
    bake(g, bound, &|n| Some(n)).unwrap()
}

fn angle_deg(a: [f32; 4], b: [f32; 4]) -> f32 {
    let dot: f32 = (0..4).map(|i| a[i] * b[i]).sum();
    2.0 * dot.abs().min(1.0).acos().to_degrees()
}

fn axis_angle(axis: [f32; 3], deg: f32) -> [f32; 4] {
    let h = deg.to_radians() * 0.5;
    [axis[0] * h.sin(), axis[1] * h.sin(), axis[2] * h.sin(), h.cos()]
}

#[test]
fn rig_fixture_is_the_writer_output() {
    let committed = std::fs::read(root().join("tests/fixtures/models/rig.src.glb")).unwrap();
    assert_eq!(committed, fixture::rig_glb().unwrap(), "rig.src.glb differs from evani_convert::fixture::rig_glb(); rewrite it with `cargo run -p evani-convert --example rig_source -- tests/fixtures/models/rig.src.glb`");
    assert!(committed.len() < 16 * 1024, "the fixture stays a few KB ({} bytes)", committed.len());
    let g = rig();
    assert_eq!((g.animations.len(), g.skins.len(), g.skins[0].joints.len()), (3, 1, 6));
}

/// `rig.static.glb` (GPU skinning's bind-pose reference, S3-9) is still the
/// writer's bytes: the rig's tube with no skin, joints, weights or clips.
#[test]
fn rig_static_fixture_is_the_writer_output() {
    let committed = std::fs::read(root().join("tests/fixtures/models/rig.static.glb")).unwrap();
    assert_eq!(committed, fixture::rig_static_glb().unwrap(), "rig.static.glb differs from evani_convert::fixture::rig_static_glb(); rewrite it with `cargo run -p evani-convert --example rig_source -- --static tests/fixtures/models/rig.static.glb`");
    let g = Gltf::from_glb(&committed).unwrap();
    assert!(g.skins.is_empty() && g.animations.is_empty() && g.nodes.iter().all(|n| n.skin.is_none()));
    let p = &g.meshes[0].primitives[0];
    assert!(p.attribute("JOINTS_0").is_none() && p.attribute("WEIGHTS_0").is_none() && p.attribute("POSITION").is_some());
}

#[test]
fn round_trip_error_within_bound() {
    let g = rig();
    for bound in [0.002, 0.0005, 0.0002] {
        let b = baked(&g, bound);
        assert_eq!(b.clips.len(), 3);
        for c in &b.clips {
            assert!(c.within && c.worst_shell_error_m <= bound, "{bound} m: clip {} is {} m off at {}", c.name, c.worst_shell_error_m, c.worst_node);
        }
        // The block reads back with the reference reader, and its values match
        // the source keys (spot checks that do not use the baker's own measure).
        let clips = clipblock::parse(&b.block).unwrap();
        assert_eq!(clips.iter().map(|c| c.frames).collect::<Vec<_>>(), vec![61, 31, 1]);
        let idle = &clips[0];
        let track = |n: &str, ch: u8| idle.tracks.iter().find(|t| t.node == node(&g, n) && t.channel == ch);
        let hips = track("hips", TRANSLATION).unwrap();
        // The bob: y 1.03 at 0.5 s and 1.5 s, 1.0 at 1 s.
        for (t, y) in [(0.0, 1.0), (0.5, 1.03), (1.0, 1.0), (1.5, 1.03), (2.0, 1.0)] {
            assert!((hips.sample(t)[1] - y).abs() <= bound as f32 + 1e-5, "{bound}: hips y at {t} s is {}", hips.sample(t)[1]);
        }
        // The sway: -4, +4, -4 degrees about z; the nods (STEP) hold 12 degrees from 0.5 s.
        let spine = track("spine", ROTATION).unwrap();
        let shell = 0.25; // the spine's farthest child (neck, arm_l) is about 0.25 m away
        let tol = (bound / shell).to_degrees() as f32 * 2.0 + 0.01;
        assert!(angle_deg(spine.sample(1.0), axis_angle([0.0, 0.0, 1.0], 4.0)) <= tol, "{bound}: spine at 1 s");
        let head = track("head", ROTATION).unwrap();
        assert!(angle_deg(head.sample(0.6), axis_angle([1.0, 0.0, 0.0], 12.0)) <= 1.0, "{bound}: head nod held at 0.6 s");
        assert!(angle_deg(head.sample(0.4), [0.0, 0.0, 0.0, 1.0]) <= 1.0, "{bound}: head at rest before the nod");
    }
}

#[test]
fn constant_tracks_cost_one_value() {
    let g = rig();
    let b = baked(&g, 0.0005);
    let clips = clipblock::parse(&b.block).unwrap();
    let idle = &clips[0];
    // The neck tilt never changes (and is not the rest pose): one value, 16 bytes.
    let neck = idle.tracks.iter().find(|t| t.node == node(&g, "neck") && t.channel == ROTATION).unwrap();
    assert_eq!((neck.keys.len(), neck.bytes), (1, 16));
    assert!(angle_deg(neck.values[0], axis_angle([1.0, 0.0, 0.0], 10.0)) < 1e-3);
    // The hand scale equals the rest pose: no track at all.
    assert!(!idle.tracks.iter().any(|t| t.node == node(&g, "hand_l") && t.channel == SCALE));
    assert_eq!((b.clips[0].constant, b.clips[0].rest), (1, 1), "{:?}", b.clips[0]);
    // A one-key clip: every track is a constant (12 bytes, 16 for a rotation).
    let pose = &clips[2];
    assert_eq!(pose.tracks.len(), 3);
    for t in &pose.tracks {
        assert_eq!((t.keys.len(), t.bytes), (1, if t.channel == ROTATION { 16 } else { 12 }), "{t:?}");
    }
    assert_eq!(b.clips[2].constant, 3);
    // Keyed tracks keep fewer keys than frames where the motion allows.
    let hips = idle.tracks.iter().find(|t| t.node == node(&g, "hips")).unwrap();
    assert!(hips.keys.len() < 61, "the piecewise-linear bob keeps its corners only: {} keys", hips.keys.len());
}

#[test]
fn same_clip_same_bytes() {
    let g = rig();
    let a = baked(&g, 0.0005);
    assert_eq!(a, baked(&g, 0.0005), "two bakes of one file differ");
    // The same animation with its channels (and samplers) listed in another
    // order bakes to the same bytes: tracks are sorted, never in input order.
    let mut d = Doc::read_glb(&fixture::rig_glb().unwrap()).unwrap();
    let ch = d.json.pointer_mut("/animations/0/channels").unwrap().as_array_mut().unwrap();
    ch.reverse();
    let shuffled = Gltf::from_glb(&d.to_glb().unwrap()).unwrap();
    assert_eq!(a.block, baked(&shuffled, 0.0005).block);
}

#[test]
fn clip_names_stay_unique() {
    // `walk`, `walk_1`, `walk`: the second `walk` must not become `walk_1`.
    let mut d = Doc::read_glb(&fixture::rig_glb().unwrap()).unwrap();
    for (k, n) in ["walk", "walk_1", "walk"].iter().enumerate() {
        d.json["animations"][k]["name"] = Value::from(*n);
    }
    let b = baked(&Gltf::from_glb(&d.to_glb().unwrap()).unwrap(), 0.001);
    let names: Vec<&str> = b.clips.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["walk", "walk_1", "walk_2"]);
}

#[test]
fn rotation_bits_follow_the_bound() {
    let g = rig();
    let loose = baked(&g, 0.002);
    let tight = baked(&g, 0.0001);
    assert!(loose.clips.iter().all(|c| c.rot_bits == 32), "a 2 mm bound fits 10-10-10-2: {:?}", loose.clips);
    assert!(tight.clips.iter().any(|c| c.rot_bits == 48), "a 0.1 mm bound needs smallest-three 48: {:?}", tight.clips);
    assert!(tight.block.len() > loose.block.len());
    for c in &tight.clips {
        assert!(c.within, "{c:?}");
    }
    // Below what 48-bit rotations can hold half a metre from a joint (about
    // 0.02 mm), the clip is reported over the bound, never passed silently.
    let impossible = baked(&g, 0.000005);
    let idle = &impossible.clips[0];
    assert!(!idle.within && idle.worst_shell_error_m > 0.000005 && idle.rot_bits == 48, "{idle:?}");
}

#[test]
fn a_long_smooth_clip_bakes_quickly() {
    // 10,000 frames (about 5.5 minutes at 30 Hz) of slow, smooth motion: the
    // key search doubles then bisects, so this stays a few seconds even in a
    // debug test build (one frame at a time it took minutes).
    let g = Gltf::from_glb(&fixture::slide_glb(9999.0 / 30.0, 2.0).unwrap()).unwrap();
    let start = std::time::Instant::now();
    let b = baked(&g, 0.0005);
    let took = start.elapsed();
    let c = &b.clips[0];
    assert_eq!(c.frames, 10_000);
    assert!(c.within, "{c:?}");
    assert!(c.keys < 2_000, "slow motion keeps few keys: {c:?}");
    assert!(took.as_secs() < 60, "baking 10,000 frames took {took:?}");
}

#[test]
fn a_bound_the_track_range_cannot_meet_names_the_track() {
    // 50 m of slide in 16-bit steps is about 0.38 mm a step at worst: a
    // 0.1 mm bound cannot be met, and the note says the range is why.
    let g = Gltf::from_glb(&fixture::slide_glb(2.0, 50.0).unwrap()).unwrap();
    let b = baked(&g, 0.0001);
    assert!(!b.clips[0].within, "{:?}", b.clips[0]);
    assert!(b.notes.iter().any(|n| n.contains("node `root` translation") && n.contains("range is the limit")), "{:?}", b.notes);
    // A bound the steps can hold passes, with no such note.
    let ok = baked(&g, 0.001);
    assert!(ok.clips[0].within && ok.notes.is_empty(), "{:?} {:?}", ok.clips, ok.notes);
}

#[test]
fn animated_model_converts_with_clips() {
    if let Err(e) = Tools::locate(&root()) {
        if std::env::var("EVANI_REQUIRE_TOOLS").is_ok_and(|v| v == "1") {
            panic!("EVANI_REQUIRE_TOOLS=1 but the converter's tools are missing ({e})");
        }
        println!("SKIPPED animated_model_converts_with_clips: the converter's tools are missing ({e})");
        return;
    }
    let t = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| root().join("target"));
    let dir = std::path::absolute(t).unwrap().join("evani-convert-tests").join("rig");
    let _ = std::fs::remove_dir_all(&dir);
    let mut o = Options::new(&root(), &root().join("tests/fixtures/models/rig.src.glb"));
    o.id = Some("rig".into());
    o.out = Some(dir.clone());
    o.tiers = vec!["low".into(), "ultra".into()];
    let out = convert(&o).unwrap();
    assert!(out.errors.is_empty(), "problems: {:?}", out.errors);
    // The master keeps the standard animations.
    let master = Gltf::from_glb(&std::fs::read(dir.join("rig.master.glb")).unwrap()).unwrap();
    assert_eq!(master.animations.len(), 3);
    for (k, tier) in ["low", "ultra"].iter().enumerate() {
        let g = Gltf::from_glb(&std::fs::read(dir.join(format!("rig.{tier}.glb"))).unwrap()).unwrap();
        assert!(g.animations.is_empty(), "{tier}: tier files carry clips, not glTF animations");
        let c = g.extras.pointer("/evani/clips").unwrap();
        let names: Vec<&str> = c["list"].as_array().unwrap().iter().map(|x| x["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["idle", "wave", "pose"]);
        let block = g.view_bytes(c["view"].as_u64().unwrap() as usize).unwrap();
        let clips = clipblock::parse(block).unwrap();
        assert_eq!(clips.len(), 3);
        for t in clips.iter().flat_map(|c| &c.tracks) {
            assert!(!g.nodes[usize::from(t.node)].name.is_empty(), "{tier}: a track targets an unnamed node");
        }
        assert!(evani_guide::modelcheck::clip_problems(&g, c).is_empty());
        let r = &out.report["tiers"][k];
        assert_eq!(r["tier"], *tier);
        let bound = r["clips"]["bound_m"].as_f64().unwrap();
        for c in r["clips"]["clips"].as_array().unwrap() {
            assert!(c["bytes"].as_u64().unwrap() > 0);
            assert!(c["worst_shell_error_m"].as_f64().unwrap() <= bound, "{tier}: {c}");
        }
        assert_eq!(r["clips"]["block_bytes"].as_u64().unwrap() as usize, block.len());
    }
    assert_ne!(out.report["tiers"][0]["clips"]["bound_m"], out.report["tiers"][1]["clips"]["bound_m"], "each tier bakes within its own bound");
    let md = std::fs::read_to_string(dir.join("rig.report.md")).unwrap();
    assert!(md.contains("## Animation clips") && md.contains("`idle`"), "{md}");
    let _: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("rig.report.json")).unwrap()).unwrap();
}
