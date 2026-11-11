//! What the Lite and Low sun bake (`model::sun`) costs on every model the
//! games ship: time in one go, frames at `FRAME_UNITS`, and the most scratch
//! memory it holds at once beyond the model itself (`Bake::scratch_bytes`,
//! read after every small step). Frames and memory do not depend on the
//! build's opt level, so they are bounded here; the time is printed (run with
//! `CARGO_PROFILE_DEV_OPT_LEVEL=3` for native numbers). A file is measured
//! only if Lite or Low draws it (see the loop). In the `--full` check's
//! look-parity list, so it cannot fail unseen again.

#![cfg(feature = "models")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code: a failed read is a failed test

/// The bake's peak memory budget per model, and its frames at
/// `FRAME_UNITS` (about 3 seconds at 60 fps). No one step (a triangle, part
/// of a direction or a vertex) may take more than a frame's `FRAME_UNITS`:
/// the near field's first pass splatted all of a direction's points in one
/// step, six frames' work on the material viewer's bust, so the page
/// stalled (2026-10-26).
const PEAK_BUDGET: usize = 8 << 20;
const FRAME_BUDGET: usize = 200;

use evani_render::model::{Part, SHARED, VERTEX_BYTES, prepare, sun};

#[test]
fn sun_bake_cost_on_shipped_models() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    for app in std::fs::read_dir(root.join("apps")).unwrap().flatten() {
        for f in std::fs::read_dir(app.path().join("assets/models")).into_iter().flatten().flatten().map(|f| f.path()) {
            if f.extension().is_some_and(|e| e == "glb") {
                files.push(f);
            }
        }
    }
    files.sort();
    assert!(files.len() >= 3, "only {} shipped models: the files moved?", files.len());
    let mut over = Vec::new();
    for f in &files {
        let name = format!("{}/{}", f.parent().and_then(|p| p.parent()).and_then(|p| p.parent()).and_then(|p| p.file_name()).unwrap().to_string_lossy(), f.file_name().unwrap().to_string_lossy());
        // Only the files Lite or Low draw are baked (Medium and up have a
        // shadow map and never run this bake, model.rs `prepare`). A tier
        // draws `<id>.<tier>.glb`, else the shared `<id>.glb`
        // (stream.rs `model_file`), so a `.medium`, `.high`, `.ultra` or
        // `.vr` file is never baked, and a shared file only by the tiers
        // without their own. (Before 2026-10-27 the test called every
        // non-low file "Lite", so the bust's High copy was held to a bake
        // no tier runs.)
        let stem = f.file_name().unwrap().to_string_lossy().trim_end_matches(".glb").to_owned();
        let (id, own) = match stem.rsplit_once('.') {
            Some((id, t)) if ["lite", "low", "medium", "high", "ultra", "vr"].contains(&t) => (id.to_owned(), Some(t.to_owned())),
            _ => (stem.clone(), None),
        };
        let baked_on: Vec<&str> = match own.as_deref() {
            Some(t) => ["lite", "low"].into_iter().filter(|b| *b == t).collect(),
            None => ["lite", "low"].into_iter().filter(|t| !f.with_file_name(format!("{id}.{t}.glb")).exists()).collect(),
        };
        if baked_on.is_empty() {
            println!("{name}: not drawn on Lite or Low, never baked");
            continue;
        }
        let tiers = match baked_on.as_slice() {
            ["lite"] => "Lite",
            ["low"] => "Low",
            _ => "Lite+Low",
        };
        let Ok(g) = evani_gltf::Gltf::from_glb(&std::fs::read(f).unwrap()) else { continue };
        let Ok(p) = prepare(&g) else { continue };
        let parts = p.parts;
        let verts: usize = parts.iter().map(|p| p.vertices.len() / VERTEX_BYTES).sum();
        let (ms, frames, cell, peak, step) = measure(&name, &parts);
        println!("{name} ({tiers}): {verts} vertices, cell {:.1} mm; {ms:.1} ms in one go, {frames} frames, peak {:.0} KB, largest step {step} units", cell * 1000.0, peak as f64 / 1024.0);
        if peak > PEAK_BUDGET || frames > FRAME_BUDGET || step > sun::FRAME_UNITS {
            over.push(format!("{name}: peak {} KB (budget {} KB), {frames} frames (budget {FRAME_BUDGET}), largest step {step} units (budget {})", peak / 1024, PEAK_BUDGET / 1024, sun::FRAME_UNITS));
        }
    }
    // A long model (the material catalogue's p10b proof row: eight balls of
    // radius 0.5 m in a 9.68 m row), whose cells are laid over twice its
    // width, not its length (sun.rs `Grid::empty`).
    let row: Vec<Part> = (0..8).map(|k| ball([(k as f32 - 3.5) * 1.24, 0.5, 0.0])).collect();
    let (ms, frames, cell, peak, step) = measure("p10b row", &row);
    println!("p10b row (8 balls, 9.68 m): 5544 vertices, cell {:.1} mm; {ms:.1} ms in one go, {frames} frames, peak {:.0} KB, largest step {step} units", cell * 1000.0, peak as f64 / 1024.0);
    if peak > PEAK_BUDGET || frames > FRAME_BUDGET || step > sun::FRAME_UNITS {
        over.push(format!("p10b row: peak {} KB, {frames} frames, largest step {step} units", peak / 1024));
    }
    assert!(over.is_empty(), "the sun bake is over its budget:\n{}", over.join("\n"));
}

/// The bake of `parts`: ms in one go, frames at `FRAME_UNITS`, the grid's
/// cell, the peak scratch bytes and the largest step's work (read after
/// every item: a triangle, part of a direction or a vertex). Spread over frames it must write the bytes of
/// one go.
fn measure(name: &str, parts: &[Part]) -> (f64, usize, f32, usize, u64) {
    let mut whole = parts.to_vec();
    let t0 = std::time::Instant::now();
    sun::bake(&mut whole);
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    let mut split = parts.to_vec();
    let mut bake = sun::Bake::new(&split);
    let mut frames = 1;
    while !bake.step(&mut split, sun::FRAME_UNITS) {
        frames += 1;
        assert!(frames < 10_000, "{name}: the bake does not finish");
    }
    assert!(split.iter().zip(&whole).all(|(a, b)| a.vertices == b.vertices), "{name}: a bake over frames wrote other bytes than one in one go");
    let mut fine = parts.to_vec();
    let mut bake = sun::Bake::new(&fine);
    let cell = bake.cell();
    let mut peak = bake.scratch_bytes();
    let mut step = 0;
    loop {
        let done = bake.step(&mut fine, 1);
        step = step.max(bake.last_units());
        if done {
            break;
        }
        peak = peak.max(bake.scratch_bytes());
    }
    (ms, frames, cell, peak, step)
}

/// p10b.mjs's ball (as in model_wgsl's long-row test): a uv sphere of
/// radius 0.5, 20 rings by 32 segments, as one part placed at `c`.
fn ball(c: [f32; 3]) -> Part {
    let (rings, segs, r) = (20u32, 32u32, 0.5f32);
    let mut vertices = Vec::new();
    for i in 0..=rings {
        let th = core::f32::consts::PI * i as f32 / rings as f32;
        for j in 0..=segs {
            let ph = 2.0 * core::f32::consts::PI * j as f32 / segs as f32;
            let n = [th.sin() * ph.cos(), th.cos(), th.sin() * ph.sin()];
            let mut v = [0u8; VERTEX_BYTES];
            for k in 0..3 {
                let q = ((n[k] * r + r) / (2.0 * r) * 65535.0).round() as u16;
                v[k * 2..k * 2 + 2].copy_from_slice(&q.to_le_bytes());
                v[8 + k] = (n[k] * 127.0).round() as i8 as u8;
            }
            vertices.extend(v);
        }
    }
    let mut indices = Vec::new();
    for i in 0..rings {
        for j in 0..segs {
            let a = i * (segs + 1) + j;
            let b = a + segs + 1;
            indices.extend([a, a + 1, b, a + 1, b + 1, b]);
        }
    }
    let node = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, c[0], c[1], c[2], 1.0];
    Part { vertices, indices, node, pos_scale: [2.0 * r; 3], pos_offset: [-r; 3], uv: [0.0; 4], material: 0, tangents: false, source: [0; 3], lod: SHARED, tags: 0, #[cfg(feature = "skinning")] skin: Vec::new() }
}
