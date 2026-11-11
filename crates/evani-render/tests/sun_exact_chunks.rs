//! The material viewer's exact sun casts (`sun-exact`) are time-sliced
//! (sun.rs, "Time-sliced"): a Lite to Low switch stalled the viewer's frames
//! 332 ms once and drew unsteadily for 15 s when one direction's exact casts
//! over a whole vertex set ran in a single step (the bust's Low bake: one
//! 88 ms step native, 2026-10-27). Now the coarse cones show first, each shut
//! ray is cast as its own item of the frame's budget, and the exact cones
//! fade in; the bytes are the one-go bake's.

#![cfg(all(feature = "models", feature = "sun-exact"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code: a failed read is a failed test

use evani_render::model::{Part, prepare, sun};

fn load(f: &str) -> Vec<Part> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/material-view/assets/models");
    let g = evani_gltf::Gltf::from_glb(&std::fs::read(root.join(f)).unwrap()).unwrap();
    prepare(&g).unwrap().parts
}

/// A step's units may pass the frame's budget by one item at most: an exact
/// cast (a ray across the bust's grid walks a few hundred samples) or one of
/// the shipped bake's items (a direction's test of a whole vertex set, about
/// 40,000 units on the bust). Before the slicing one step was about 40 times
/// the budget (88 ms native).
const ITEM_SLACK: u64 = sun::FRAME_UNITS;

#[test]
fn exact_casts_are_time_sliced() {
    let mut report = Vec::new();
    for f in ["char-head.low.glb", "layer-coat.low.glb"] {
        let parts = load(f);
        let mut whole = parts.clone();
        sun::bake(&mut whole);

        // The bake's steps: units per step stay near the budget, the
        // coarse cones come first, then the casts.
        let mut split = parts.clone();
        let mut bake = sun::Bake::new(&split);
        let (mut steps, mut most, mut coarse_at, mut shut) = (0usize, 0u64, None, 0usize);
        loop {
            let done = bake.step(&mut split, sun::FRAME_UNITS);
            steps += 1;
            most = most.max(bake.last_units());
            if coarse_at.is_none() && bake.coarse() {
                coarse_at = Some(steps);
                shut = bake.shut_rays();
            }
            if done {
                break;
            }
            assert!(steps < 100_000, "{f}: the bake never ends");
        }
        assert!(split.iter().zip(&whole).all(|(a, b)| a.vertices == b.vertices), "{f}: the time-sliced bake wrote other bytes than the bake in one go");
        let coarse_at = coarse_at.unwrap_or(steps);
        assert!(shut > 0, "{f}: no ray to cast exactly (the test no longer covers the casts)");
        assert!(most <= sun::FRAME_UNITS + ITEM_SLACK, "{f}: a step did {most} units (budget {} plus one item, {ITEM_SLACK})", sun::FRAME_UNITS);

        // The GPU side's view: frames (wall time) until the coarse cones
        // show and until the exact ones have faded in, the slowest frame,
        // and the parts handed back last are the bake's.
        let mut lazy = sun::Lazy::new(parts);
        let (mut n, mut shown, mut slowest, mut total, mut handed, mut last) = (0usize, None, 0.0f64, 0.0f64, 0usize, None);
        let mut phase = [(0usize, 0.0f64, 0.0f64); 3];
        while !lazy.settled() {
            let t = std::time::Instant::now();
            let out = lazy.advance(sun::FRAME_UNITS);
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            (n, total, slowest) = (n + 1, total + ms, slowest.max(ms));
            let k = if shown.is_none() { 0 } else if handed == 1 { 1 } else { 2 };
            phase[k] = (phase[k].0 + 1, phase[k].1 + ms, phase[k].2.max(ms));
            if out.is_some() {
                handed += 1;
                last = out;
            }
            if shown.is_none() && lazy.baked() {
                shown = Some(n);
            }
            assert!(n < 100_000, "{f}: the fade never ends");
        }
        assert!(lazy.advance(sun::FRAME_UNITS).is_none());
        assert!(last.unwrap().iter().zip(&whole).all(|(a, b)| a.vertices == b.vertices), "{f}: the parts handed back last are not the bake's");
        assert_eq!(handed, sun::FADE_FRAMES as usize + 1, "{f}: the coarse cones, then each frame of the fade");
        let shown = shown.unwrap_or(n);
        report.push(format!("{f}: {shut} rays to cast; coarse cones in {coarse_at} steps; Lazy: shown after {shown} frames, settled after {n} ({total:.0} ms in all), slowest frame {slowest:.1} ms; per phase (frames, ms in all, slowest ms) coarse {:?}, casts {:?}, fade {:?}; most units in a step {most}", phase[0], phase[1], phase[2]));
    }
    println!("{}", report.join("\n"));
}
