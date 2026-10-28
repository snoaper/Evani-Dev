//! S3-8 cross-check: the game's clip decoder (`evani-anim`) and the
//! reference reader (`evani_guide::clipblock`) read the rig fixture's baked
//! clips to the same bits, at every bound the tiers use (both rotation
//! widths), and the committed `tests/fixtures/models/rig.clips.evc1` (which
//! evani-anim's own tests read) is what the baker writes today.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code: a failed step is a failed test

use std::path::{Path, PathBuf};

use evani_anim::Clips;
use evani_convert::clips::bake;
use evani_convert::fixture;
use evani_gltf::Gltf;
use evani_guide::clipblock;

/// The bound the committed fixture is baked at.
const FIXTURE_BOUND: f64 = 0.0005;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap().to_path_buf()
}

fn block(bound: f64) -> Vec<u8> {
    let g = Gltf::from_glb(&fixture::rig_glb().unwrap()).unwrap();
    bake(&g, bound, &|n| Some(n)).unwrap().block
}

/// Every track of every clip sampled by both readers at many frame
/// positions (keys, between keys, past the ends), compared bit for bit.
fn same_samples(b: &[u8]) -> usize {
    let reference = clipblock::parse(b).unwrap();
    let ours = Clips::decode(b).unwrap();
    assert_eq!(ours.len(), reference.len());
    let mut compared = 0;
    for (c, rc) in reference.iter().enumerate() {
        assert_eq!(ours.frames(c), rc.frames);
        let tracks = ours.tracks(c);
        assert_eq!(tracks.len(), rc.tracks.len(), "clip {c}");
        for (t, rt) in tracks.iter().zip(&rc.tracks) {
            assert_eq!((t.node, t.channel), (rt.node, rt.channel));
            let (keys, values) = ours.keys(t);
            assert_eq!(keys, &rt.keys[..]);
            let vb = |v: &[[f32; 4]]| v.iter().flatten().map(|x| x.to_bits()).collect::<Vec<_>>();
            assert_eq!(vb(values), vb(&rt.values), "clip {c} node {} channel {}: decoded keys differ", t.node, t.channel);
            // Forward with one cursor, then backwards with the same cursor.
            let mut key = 0u16;
            let n = f32::from(rc.frames) * 8.0 + 16.0;
            let xs: Vec<f32> = (0..n as u32).map(|k| k as f32 / 8.0 - 1.0).chain((0..n as u32).rev().map(|k| k as f32 / 8.0 - 1.0 + 0.0625)).collect();
            for x in xs {
                let a = ours.sample_track(t, x, &mut key);
                let b = rt.sample_frame(x);
                assert_eq!(a.map(f32::to_bits), b.map(f32::to_bits), "clip {c} node {} channel {} at frame {x}: {a:?} vs {b:?}", t.node, t.channel);
                compared += 1;
            }
        }
    }
    compared
}

#[test]
fn anim_decoder_matches_the_reference_reader() {
    let mut widths = Vec::new();
    for bound in [0.002, 0.001, FIXTURE_BOUND, 0.0003, 0.0002] {
        let b = block(bound);
        widths.extend(clipblock::parse(&b).unwrap().iter().map(|c| c.rot_bytes));
        assert!(same_samples(&b) > 1000, "{bound}: too few samples compared");
    }
    widths.sort_unstable();
    widths.dedup();
    assert_eq!(widths, [4, 6], "both rotation widths are cross-checked");
    // Bad blocks: both readers refuse them.
    let good = block(FIXTURE_BOUND);
    for cut in [0, 3, 8, 11, 20, good.len() / 2, good.len() - 1] {
        assert!(Clips::decode(&good[..cut]).is_err() && clipblock::parse(&good[..cut]).is_err(), "cut at {cut}");
    }
    for i in 0..good.len() {
        let mut b = good.clone();
        b[i] ^= 0x5a;
        assert_eq!(Clips::decode(&b).is_ok(), clipblock::parse(&b).is_ok(), "byte {i} flipped: the readers disagree on whether it is a block");
    }
}

#[test]
fn rig_clips_fixture_is_the_baker_output() {
    let path = root().join("tests/fixtures/models/rig.clips.evc1");
    let fresh = block(FIXTURE_BOUND);
    if std::env::var("EVANI_REWRITE_FIXTURES").is_ok_and(|v| v == "1") {
        std::fs::write(&path, &fresh).unwrap();
    }
    let committed = std::fs::read(&path).unwrap();
    assert_eq!(committed, fresh, "rig.clips.evc1 differs from the baker's output; rewrite it with EVANI_REWRITE_FIXTURES=1 cargo test -p evani-convert --test anim_crosscheck");
    same_samples(&committed);
}
