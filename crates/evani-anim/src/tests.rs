//! STAGE3_PLAN S3-8's tests, on the rig fixture's clips
//! (`tests/fixtures/models/rig.clips.evc1`, baked by evani-convert from
//! `rig.src.glb`; evani-convert's `anim_crosscheck` test keeps the file in
//! step with the baker and decodes it with the reference reader too).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code: a failed step is a failed test

use crate::math::*;
use crate::{ALL, Aim, AnimModule, AnimParams, Blend1d, Clips, Graph, Mask, NONE, Node, Player, Point, Rig, Skeleton, Skin, TwoBone, frame_at, frame_at_phase, pick};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/models/");
// Rig nodes: 0 rig, 1 hips, 2 spine, 3 neck, 4 head, 5 arm_l, 6 hand_l, 7 body.
const IDLE: u16 = 0;
const WAVE: u16 = 1;

fn rig() -> (Skeleton, Skin, Clips) {
    let g = evani_gltf::Gltf::from_glb(&std::fs::read(format!("{ROOT}rig.src.glb")).unwrap()).unwrap();
    let parents: Vec<u16> = (0..g.nodes.len()).map(|n| g.parent(n).map_or(NONE, |p| p as u16)).collect();
    let rest: Vec<Xform> = g.nodes.iter().map(|n| Xform { t: n.translation, r: n.rotation, s: n.scale }).collect();
    let sk = Skeleton::new(&parents, &rest).unwrap();
    let s = &g.skins[0];
    let ibm: Vec<[f32; 16]> = g.read_f32(s.inverse_bind_matrices.unwrap()).unwrap().chunks(16).map(|c| c.try_into().unwrap()).collect();
    let joints: Vec<u16> = s.joints.iter().map(|&j| j as u16).collect();
    let skin = Skin::new(&sk, &joints, &ibm).unwrap();
    let clips = Clips::decode(&std::fs::read(format!("{ROOT}rig.clips.evc1")).unwrap()).unwrap();
    (sk, skin, clips)
}

fn bits(m: &[Mat43]) -> Vec<u32> {
    m.iter().flatten().map(|v| v.to_bits()).collect()
}

/// Every pose stage at one tick: sample, blend, model space, IK, skinning.
fn pose_at(sk: &Skeleton, skin: &Skin, clips: &Clips, g: &mut Graph, rig: &mut Rig, p: &Player, sub: f32) -> Vec<u32> {
    let fi = frame_at(p.prev_ticks, sub, clips.frames(usize::from(p.prev)), true);
    let fc = frame_at(p.ticks, sub, clips.frames(usize::from(p.state)), true);
    let nodes = [Node::Clip { clip: p.prev, frame: fi }, Node::Clip { clip: p.state, frame: fc }, Node::Blend { a: 0, b: 1, w: p.weight(sub) }];
    let pose = g.eval(sk, clips, &nodes, &[]).unwrap();
    rig.set_pose(pose);
    rig.update(sk);
    rig.two_bone(sk, &TwoBone { start: 2, mid: 5, end: 6, target: [0.4, 1.3, 0.1], pole: [0.2, 1.0, -1.0], soften: 0.97, weight: 1.0 }).unwrap();
    rig.aim(sk, &Aim { joint: 4, forward: [0.0, 0.0, 1.0], up: [0.0, 1.0, 0.0], target: [1.0, 1.7, 2.0], pole: [0.0, 1.0, 0.0], weight: 0.8 }).unwrap();
    bits(rig.skinning(skin))
}

#[test]
fn same_ticks_same_pose_bits() {
    let (sk, skin, clips) = rig();
    assert_eq!((clips.len(), clips.frames(0), clips.frames(1), clips.frames(2)), (3, 61, 31, 1));
    let run = || {
        let mut g = Graph::new(&sk, &clips, 3);
        let mut r = Rig::new(&sk, skin.joints().len());
        let mut p = Player::new(IDLE);
        let mut out = Vec::new();
        for t in 0..300u32 {
            if t == 70 {
                p.play(WAVE, 12);
            }
            if t == 150 {
                p.play(IDLE, 20);
            }
            p.step();
            out.push(pose_at(&sk, &skin, &clips, &mut g, &mut r, &p, 0.25));
        }
        out
    };
    let a = run();
    assert_eq!(a, run(), "two runs of the same ticks give different pose bits");
    // A fresh graph and rig jumping straight to a tick give the same bits as
    // playing up to it (the cursor never changes a sample).
    let mut p = Player::new(IDLE);
    for t in 0..200u32 {
        if t == 70 {
            p.play(WAVE, 12);
        }
        if t == 150 {
            p.play(IDLE, 20);
        }
        p.step();
    }
    let mut g = Graph::new(&sk, &clips, 3);
    let mut r = Rig::new(&sk, skin.joints().len());
    assert_eq!(pose_at(&sk, &skin, &clips, &mut g, &mut r, &p, 0.25), a[199]);
    assert!(a.iter().flatten().all(|b| f32::from_bits(*b).is_finite()));
    assert_ne!(a[10], a[40], "the idle moves");
    // Pinned bits: only + - * / and sqrt, all fixed by IEEE 754, so these
    // ticks give these bits on every machine, the wasm included (only native runs check it). A change to
    // the decoder, a blend, IK or the order of any operation shows here; if
    // it is meant, update the number and say why in the commit.
    let mut h = 0xcbf2_9ce4_8422_2325u64; // FNV-1a
    for t in [0usize, 75, 160, 299] {
        for b in a[t].iter().flat_map(|b| b.to_le_bytes()) {
            h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    assert_eq!(h, GOLDEN_POSE_BITS, "the pose bits at ticks 0, 75, 160 and 299 changed: {h:#018x}");
}

const GOLDEN_POSE_BITS: u64 = 0x920b_f4a2_3ce7_0227;

#[test]
fn crossfade_weights_sum_to_one() {
    let (sk, _, clips) = rig();
    let mut p = Player::new(IDLE);
    for _ in 0..30 {
        p.step();
    }
    p.play(WAVE, 10);
    assert_eq!((p.state, p.prev, p.ticks, p.prev_ticks), (WAVE, IDLE, 0, 30));
    let mut last = 0.0;
    for t in 0..14 {
        for sub in [0.0, 0.3, 0.99] {
            let w = p.weight(sub);
            assert!((0.0..=1.0).contains(&w), "{w}");
            assert!(w >= last, "the fade only goes one way");
            last = w;
        }
        assert_eq!(p.fading(), t < 10, "tick {t}");
        p.step();
    }
    assert_eq!(p.weight(0.5), 1.0);
    // The blend of a pose with itself is that pose at any weight, and the
    // ends are exact.
    let mut g = Graph::new(&sk, &clips, 3);
    let same = [Node::Clip { clip: IDLE, frame: 7.5 }, Node::Clip { clip: IDLE, frame: 7.5 }];
    let one = g.eval(&sk, &clips, &same[..1], &[]).unwrap().to_vec();
    for w in [0.0, 0.25, 0.5, 1.0] {
        let nodes = [same[0], same[1], Node::Blend { a: 0, b: 1, w }];
        let got = g.eval(&sk, &clips, &nodes, &[]).unwrap();
        for (x, y) in got.iter().zip(&one) {
            assert!((0..4).all(|i| (x.r[i] - y.r[i]).abs() < 1e-6) && (0..3).all(|i| (x.t[i] - y.t[i]).abs() < 1e-6), "w {w}");
        }
    }
    let wave = g.eval(&sk, &clips, &[Node::Clip { clip: WAVE, frame: 9.0 }], &[]).unwrap().to_vec();
    let mut ends = |w| g.eval(&sk, &clips, &[same[0], Node::Clip { clip: WAVE, frame: 9.0 }, Node::Blend { a: 0, b: 1, w }], &[]).unwrap().to_vec();
    assert_eq!(ends(0.0), one);
    assert_eq!(ends(1.0), wave);
    // Mid-fade, the blend node gives the previous pose weight 1 - w and the
    // new one w (w = the player's weight), node by node.
    let mut f = Player::new(IDLE);
    f.play(WAVE, 10);
    for _ in 0..3 {
        f.step();
    }
    let w = f.weight(0.5);
    assert!(w > 0.3 && w < 0.4, "{w}");
    let mid = ends(w);
    for ((m, a), b) in mid.iter().zip(&one).zip(&wave) {
        let want = Xform { t: lerp3(a.t, b.t, w), r: nlerp(a.r, b.r, w), s: lerp3(a.s, b.s, w) };
        assert_eq!(*m, want, "w {w}: not (1 - w) x previous + w x new");
    }
    assert_ne!(mid, ends(1.0 - w), "the blend must not run backwards");
    // A weight that is not a number leaves the first pose, never NaN.
    assert_eq!(ends(f32::NAN), one);
    // A cut (fade 0) is the new state at once.
    let mut c = Player::new(IDLE);
    c.play(WAVE, 0);
    assert_eq!(c.weight(0.0), 1.0);
}

/// An arm along +x: shoulder at the origin, upper 0.3, lower 0.25.
fn arm() -> (Skeleton, Rig) {
    let x = |t: f32| Xform { t: [t, 0.0, 0.0], ..Xform::IDENTITY };
    let sk = Skeleton::new(&[NONE, 0, 1, 2], &[Xform::IDENTITY, x(0.0), x(0.3), x(0.25)]).unwrap();
    let r = Rig::new(&sk, 0);
    (sk, r)
}

#[test]
fn two_bone_ik_reaches_or_stretches_safely() {
    let (sk, rest) = arm();
    let ik = |target: Vec3, soften: f32| TwoBone { start: 1, mid: 2, end: 3, target, pole: [0.0, 0.0, 1.0], soften, weight: 1.0 };
    let lengths = |r: &Rig| (length(sub(r.position(2), r.position(1))), length(sub(r.position(3), r.position(2))));
    // Reachable targets are reached, bones keep their length, the elbow bends to the pole.
    for target in [[0.4, 0.1, 0.0], [0.1, 0.3, 0.2], [-0.2, -0.2, 0.1], [0.0, 0.0, -0.3], [0.5, 0.0, 0.0]] {
        let mut r = rest.clone();
        assert!(r.two_bone(&sk, &ik(target, 1.0)).unwrap(), "{target:?} is in reach");
        let e = length(sub(r.position(3), target));
        assert!(e < 1e-4, "{target:?}: the hand is {e} m off");
        let (a, b) = lengths(&r);
        assert!((a - 0.3).abs() < 1e-5 && (b - 0.25).abs() < 1e-5, "{target:?}: bones {a} {b}");
        let dir = unit(target).unwrap();
        let elbow = r.position(2);
        let off = sub(elbow, scale(dir, dot(elbow, dir)));
        assert!(dot(off, sub([0.0, 0.0, 1.0], scale(dir, dir[2]))) >= -1e-6, "{target:?}: the elbow bends away from the pole");
    }
    // Out of reach, degenerate and softened: straight at the target, finite, no NaN.
    for (target, soften) in [([2.0, 1.0, 0.0], 1.0), ([0.0, 0.0, 0.0], 1.0), ([0.01, 0.0, 0.0], 1.0), ([0.0, 0.0, 1.0], 1.0), ([0.55, 0.0, 0.0], 0.9), ([1e30, 0.0, 0.0], 0.9), ([0.0, 3.0, 0.0], 0.0)] {
        let mut r = rest.clone();
        let reached = r.two_bone(&sk, &TwoBone { pole: [0.55, 0.0, 0.0], ..ik(target, soften) }).unwrap();
        assert!(r.model().iter().flatten().all(|v| v.is_finite()), "{target:?}: NaN or infinity in the pose");
        let (a, b) = lengths(&r);
        assert!((a - 0.3).abs() < 1e-5 && (b - 0.25).abs() < 1e-5, "{target:?}: bones {a} {b}");
        if length(target) > 0.56 {
            assert!(!reached, "{target:?} is out of reach");
            let hand = r.position(3);
            let big = target.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(dot(unit(hand).unwrap(), unit(scale(target, 1.0 / big)).unwrap()) > 0.9999, "{target:?}: the chain points at the target");
        }
    }
    // Softening: a target at full reach leaves the arm just short of straight,
    // and the hand's distance grows smoothly with the target's.
    let mut prev = 0.0;
    for k in 0..40 {
        let d = 0.45 + k as f32 * 0.01;
        let mut r = rest.clone();
        r.two_bone(&sk, &ik([d, 0.0, 0.0], 0.95)).unwrap();
        let reach = length(r.position(3));
        assert!(reach <= 0.55 + 1e-5 && reach >= prev - 1e-6, "{d}: {reach} after {prev}");
        if (d - 0.55).abs() < 1e-3 {
            assert!(reach < 0.55 - 1e-4, "softened short of full stretch at full reach");
        }
        prev = reach;
    }
    // Not a chain: an error, not a panic.
    assert!(rest.clone().two_bone(&sk, &TwoBone { start: 3, mid: 2, end: 1, ..ik([0.1; 3], 1.0) }).is_err());
    assert!(rest.clone().two_bone(&sk, &TwoBone { end: 9, ..ik([0.1; 3], 1.0) }).is_err());
    // Weight 0 leaves the pose.
    let mut r = rest.clone();
    r.two_bone(&sk, &TwoBone { weight: 0.0, ..ik([0.1, 0.3, 0.0], 1.0) }).unwrap();
    assert_eq!(r, rest);
    // A weight that is not a number is 0, never a NaN pose.
    r.two_bone(&sk, &TwoBone { weight: f32::NAN, ..ik([0.1, 0.3, 0.0], 1.0) }).unwrap();
    assert_eq!(r, rest);
}

#[test]
fn aim_ik_points_at_target() {
    let (sk, skin, clips) = rig();
    let mut g = Graph::new(&sk, &clips, 1);
    let mut r = Rig::new(&sk, skin.joints().len());
    for (k, target) in [[0.0, 1.6, 2.0], [-3.0, 0.5, 0.2], [0.3, 4.0, -1.0], [0.0, 1.66, -5.0]].into_iter().enumerate() {
        let pose = g.eval(&sk, &clips, &[Node::Clip { clip: IDLE, frame: k as f32 * 7.0 }], &[]).unwrap();
        r.set_pose(pose);
        r.update(&sk);
        let aim = Aim { joint: 4, forward: [0.0, 0.0, 1.0], up: [0.0, 1.0, 0.0], target, pole: [0.0, 1.0, 0.0], weight: 1.0 };
        r.aim(&sk, &aim).unwrap();
        let head = r.position(4);
        let want = unit(sub(target, head)).unwrap();
        let fwd = rotate(r.rotation(4), aim.forward);
        assert!(dot(fwd, want) > 0.99999, "{target:?}: forward {fwd:?} wants {want:?}");
        // The model matrix agrees with the rotation (model space was updated).
        let m = r.model()[4];
        let mf = unit([m[2], m[6], m[10]]).unwrap();
        assert!(dot(mf, want) > 0.9999, "{target:?}: the matrix still points the old way");
        // The up axis leans to the pole as far as the aim allows.
        let up = rotate(r.rotation(4), aim.up);
        let side = |v: Vec3| unit(sub(v, scale(want, dot(v, want)))).unwrap();
        assert!(dot(side(up), side(aim.pole)) > 0.9999, "{target:?}: up {up:?}");
    }
    // Half weight turns part way.
    let pose = g.eval(&sk, &clips, &[Node::Clip { clip: IDLE, frame: 0.0 }], &[]).unwrap();
    r.set_pose(pose);
    r.update(&sk);
    let before = rotate(r.rotation(4), [0.0, 0.0, 1.0]);
    let target = add(r.position(4), [5.0, 0.0, 0.0]);
    r.aim(&sk, &Aim { joint: 4, forward: [0.0, 0.0, 1.0], up: [0.0; 3], target, pole: [0.0; 3], weight: 0.5 }).unwrap();
    let after = rotate(r.rotation(4), [0.0, 0.0, 1.0]);
    let to = unit(sub(target, r.position(4))).unwrap();
    assert!(dot(after, to) > dot(before, to) + 0.1 && dot(after, to) < 0.999);
    // A weight that is not a number leaves the pose alone.
    let held = r.clone();
    r.aim(&sk, &Aim { joint: 4, forward: [0.0, 0.0, 1.0], up: [0.0, 1.0, 0.0], target, pole: [0.0, 1.0, 0.0], weight: f32::NAN }).unwrap();
    assert_eq!(r, held);
    assert!(r.model().iter().flatten().all(|v| v.is_finite()));
}

#[test]
fn blend_space_phase_sync() {
    let (_, _, clips) = rig();
    // Idle (120 ticks) at speed 0, wave (60 ticks) at speed 1.
    let pts = [Point { clip: IDLE, at: 0.0, ticks: clips.ticks(0) }, Point { clip: WAVE, at: 1.0, ticks: clips.ticks(1) }];
    assert_eq!((pts[0].ticks, pts[1].ticks), (120, 60));
    assert_eq!(pick(&pts, -1.0), (0, 0, 0.0));
    assert_eq!(pick(&pts, 0.25), (0, 1, 0.25));
    assert_eq!(pick(&pts, 7.0), (1, 1, 0.0));
    let mut b = Blend1d::default();
    let mut frames = Vec::new();
    for t in 0..400u32 {
        let x = (t as f32 / 400.0).min(1.0);
        let before = b.phase;
        b.step(&pts, x);
        assert!(b.phase != before, "the phase always moves");
        let (fa, fb) = (frame_at_phase(b.phase, clips.frames(0)), frame_at_phase(b.phase, clips.frames(1)));
        // Both clips at the same fraction of their cycle.
        assert!((fa / 60.0 - fb / 30.0).abs() < 1e-5, "tick {t}: {fa} of 60 and {fb} of 30");
        frames.push(fa);
    }
    // At one end the cycle is that clip's own length.
    let mut s = Blend1d::default();
    for _ in 0..60 {
        s.step(&pts, 1.0);
    }
    assert!(s.phase < 1000 || s.phase > u32::MAX - 1000, "60 ticks at the wave end make one cycle: {}", s.phase);
    let mut s = Blend1d::default();
    for _ in 0..120 {
        s.step(&pts, 0.0);
    }
    assert!(s.phase < 1000 || s.phase > u32::MAX - 1000, "120 ticks at the idle end make one cycle: {}", s.phase);
    // Moving along the axis never jumps the phase (no step over a 30th of a cycle).
    let mut s = Blend1d::default();
    for t in 0..240u32 {
        let before = s.phase;
        s.step(&pts, if t % 2 == 0 { 0.0 } else { 1.0 });
        assert!(s.phase.wrapping_sub(before) <= u32::MAX / 59, "tick {t}");
        assert!(s.crossed(before, before.wrapping_add(1)));
        assert!(!s.crossed(before, before));
    }
    // Same inputs, same phase.
    let mut c = Blend1d::default();
    for t in 0..400u32 {
        c.step(&pts, (t as f32 / 400.0).min(1.0));
    }
    assert_eq!(c, b);
    assert!(frames.windows(2).filter(|w| w[1] < w[0]).count() >= 2, "the cycle wraps");
}

#[test]
fn masked_additive_leaves_masked_joints() {
    let (sk, _, clips) = rig();
    let mut g = Graph::new(&sk, &clips, 4);
    // The wave's arm motion added over the idle, on the arm only (arm_l and hand_l).
    let mask = Mask::below(&sk, &[5], 1.0);
    assert_eq!(mask.0, [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0]);
    let base = g.eval(&sk, &clips, &[Node::Clip { clip: IDLE, frame: 13.0 }], &[]).unwrap().to_vec();
    let nodes = [
        Node::Clip { clip: IDLE, frame: 13.0 },
        Node::Clip { clip: WAVE, frame: 15.0 },
        Node::Clip { clip: WAVE, frame: 0.0 },
        Node::Add { base: 0, layer: 1, reference: 2, mask: 0, w: 1.0 },
    ];
    let out = g.eval(&sk, &clips, &nodes, std::slice::from_ref(&mask)).unwrap().to_vec();
    for n in 0..sk.len() {
        if mask.0[n] == 0.0 {
            assert_eq!(out[n], base[n], "node {n} is masked out but changed");
        }
    }
    assert!(out[5].r != base[5].r, "the arm takes the wave");
    // Against its own reference the layer adds nothing.
    let zero = [nodes[0], nodes[2], nodes[2], Node::Add { base: 0, layer: 1, reference: 2, mask: ALL, w: 1.0 }];
    let same = g.eval(&sk, &clips, &zero, &[]).unwrap();
    for (a, b) in same.iter().zip(&base) {
        assert!((0..4).all(|i| (a.r[i] - b.r[i]).abs() < 1e-6) && (0..3).all(|i| (a.t[i] - b.t[i]).abs() < 1e-6));
    }
    // Weight 0 is the base exactly; a bad input index is an error.
    let off = [nodes[0], nodes[1], nodes[2], Node::Add { base: 0, layer: 1, reference: 2, mask: ALL, w: 0.0 }];
    assert_eq!(g.eval(&sk, &clips, &off, &[]).unwrap(), &base[..]);
    assert!(g.eval(&sk, &clips, &[Node::Add { base: 0, layer: 0, reference: 0, mask: ALL, w: 1.0 }], &[]).is_err());
    assert!(g.eval(&sk, &clips, &[nodes[0], Node::Add { base: 0, layer: 0, reference: 0, mask: 3, w: 1.0 }], &[]).is_err());
    assert!(g.eval(&sk, &clips, &[Node::Clip { clip: 9, frame: 0.0 }], &[]).is_err());
}

#[test]
fn rest_pose_matches_the_reader_and_skins_to_identity() {
    let (sk, skin, _) = rig();
    let g = evani_gltf::Gltf::from_glb(&std::fs::read(format!("{ROOT}rig.src.glb")).unwrap()).unwrap();
    let mut r = Rig::new(&sk, skin.joints().len());
    let world = g.world_matrices();
    for (n, w) in world.iter().enumerate() {
        let m = r.model()[n];
        assert!((0..12).all(|i| (m[i] - from_cols(w)[i]).abs() < 1e-5), "node {n}");
    }
    // At the bind pose every skinning matrix is the identity.
    for m in r.skinning(&skin) {
        assert!((0..12).all(|i| (m[i] - MID[i]).abs() < 1e-5), "{m:?}");
    }
    // An attachment 0.1 m along the hand.
    let p = r.attach_point(6, [0.1, 0.0, 0.0]);
    assert!((length(sub(p, r.position(6))) - 0.1).abs() < 1e-5);
    assert_eq!(mpos(&r.attach(6, &Xform::IDENTITY)), r.position(6));
}

#[test]
fn sim_state_events_and_frames() {
    let mut p = Player::new(IDLE);
    let mut hits = Vec::new();
    for t in 1..=130u32 {
        p.step();
        if p.hit(30, 60, true) {
            hits.push(t);
        }
    }
    assert_eq!(hits, [30, 90]);
    assert!(!Player::new(0).hit(0, 60, true), "no event before the first step");
    assert_eq!(frame_at(0, 0.0, 61, true), 0.0);
    assert_eq!(frame_at(119, 0.5, 61, true), 59.75);
    assert_eq!(frame_at(120, 0.0, 61, true), 0.0);
    assert_eq!(frame_at(500, 0.0, 61, false), 60.0);
    assert_eq!(frame_at(9, 0.0, 1, true), 0.0);
}

#[test]
fn bad_blocks_are_errors_not_panics() {
    let good = std::fs::read(format!("{ROOT}rig.clips.evc1")).unwrap();
    assert!(Clips::decode(&good).is_ok());
    assert!(Clips::decode(b"").is_err());
    assert!(Clips::decode(b"EVC0\0\0\0\0").is_err());
    for cut in 0..good.len() {
        assert!(Clips::decode(&good[..cut]).is_err(), "cut at {cut}");
    }
    let mut s = 0x9e37_79b9u32;
    for i in 0..good.len() * 4 {
        let mut b = good.clone();
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        b[i % good.len()] ^= (s as u8) | 1;
        if let Ok(c) = Clips::decode(&b) {
            // Whatever decodes also samples without a panic.
            let mut cur = c.cursor();
            let mut pose = vec![Xform::IDENTITY; c.nodes_needed()];
            for k in 0..c.len() {
                c.sample(k, 3.3, &mut cur, &mut pose);
            }
        }
    }
    assert!(Skeleton::new(&[1, 0], &[Xform::IDENTITY; 2]).is_err(), "a cycle");
    assert!(Skeleton::new(&[5], &[Xform::IDENTITY]).is_err(), "a parent out of range");
}

#[test]
fn module_registers_its_parameter() {
    let mut a = evani_core::App::new("t").with_module(AnimModule);
    a.run_headless(1, 16.0);
    let reg = a.world.res::<evani_core::params::ParamRegistry>();
    let p = *a.world.res::<AnimParams>();
    assert_eq!(reg.i32(p.crossfade), 12);
    assert!(reg.id("anim.crossfade_ticks").is_some());
}

/// Size guard (DECISIONS.md "Animation runtime"): `f32::clamp` with bounds
/// that are not constants keeps its `min <= max` assert, and that assert's
/// message pulls the whole float formatter (about 10 KB gzip) into every
/// game with `anim`. Bounds must be number literals; use `max` then `min`
/// otherwise.
#[test]
fn no_clamp_with_computed_bounds() {
    let files = [
        ("clip.rs", include_str!("clip.rs")),
        ("graph.rs", include_str!("graph.rs")),
        ("ik.rs", include_str!("ik.rs")),
        ("lib.rs", include_str!("lib.rs")),
        ("math.rs", include_str!("math.rs")),
        ("play.rs", include_str!("play.rs")),
        ("rig.rs", include_str!("rig.rs")),
    ];
    let literal = |a: &str| {
        let a = a.trim().trim_end_matches("f32");
        !a.is_empty() && a.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '_' | 'e'))
    };
    let needle = concat!(".cla", "mp(");
    for (name, text) in files {
        for (at, _) in text.match_indices(needle) {
            let args = &text[at + needle.len()..];
            let args = &args[..args.find(')').unwrap_or(args.len())];
            assert!(args.split(',').all(literal), "{name}: clamp({args}) has bounds that are not number literals; use max then min");
        }
    }
}
