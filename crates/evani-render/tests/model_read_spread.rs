//! A tier switch never stops the frames on a model read (ov1, 2026-11-02):
//! a streamed `.glb` is read in steps over frames (`evani_gltf::Reading`,
//! `model::steps`, `model::read`) and the Lite and Low sun bake's setup
//! runs in steps too (`sun::Init`). Each stepped path must give exactly
//! what the one-go path gives, and must really be spread. The fixture is
//! material-view's bust: 850 KB at High, the file that held one frame for
//! 300 ms on the first visit to High (CORE_REQUESTS `model-read-spread`).

#![cfg(feature = "models")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code: a failed read is a failed test

use evani_gltf::Gltf;
use evani_render::model::{Part, prepare, sun};

fn bust(tier: &str) -> Vec<u8> {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../apps/material-view/assets/models/char-head{tier}.glb"));
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn same_parts(a: &[Part], b: &[Part]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.vertices == y.vertices && x.indices == y.indices && x.material == y.material && x.lod == y.lod && x.tags == y.tags)
}

#[test]
fn a_stepped_glb_read_is_the_one_go_read() {
    let b = bust(".high");
    let one = Gltf::from_glb(&b).unwrap();
    let mut r = Gltf::begin_glb(&b).unwrap();
    let mut steps = 1;
    while !r.step().unwrap() {
        steps += 1;
    }
    let g = r.finish().unwrap();
    assert!(steps >= 4, "the bust's meshopt views decode in one step each ({steps} steps)");
    assert_eq!(g.accessors.len(), one.accessors.len());
    for i in 0..one.accessors.len() {
        assert_eq!(g.accessor_bytes(i).unwrap(), one.accessor_bytes(i).unwrap(), "accessor {i}");
    }
    let (p, q) = (prepare(&g).unwrap(), prepare(&one).unwrap());
    assert!(same_parts(&p.parts, &q.parts));
    assert_eq!((p.triangles, p.materials.len(), p.textures.len()), (q.triangles, q.materials.len(), q.textures.len()));
}

#[test]
fn the_bake_setup_spreads_over_frames_and_bakes_the_same_bytes() {
    for tier in [".lite", ".low", ""] {
        let parts = prepare(&Gltf::from_glb(&bust(tier)).unwrap()).unwrap().parts;
        let mut whole = parts.clone();
        sun::bake(&mut whole);
        let mut init = sun::Init::new(&parts);
        let mut frames = 1;
        let mut bake = loop {
            if let Some(b) = init.step(&parts, sun::FRAME_UNITS) {
                break b;
            }
            frames += 1;
            assert!(frames < 10_000, "char-head{tier}: the setup does not finish");
        };
        // The setup used to be one frame (56 to 285 ms in a dev build).
        assert!(frames >= 3, "char-head{tier}: the bake's setup ran in {frames} frames, not spread");
        let mut split = parts.clone();
        while !bake.step(&mut split, sun::FRAME_UNITS) {}
        assert!(same_parts(&split, &whole), "char-head{tier}: a setup over frames baked other bytes than one in one go");
    }
}
