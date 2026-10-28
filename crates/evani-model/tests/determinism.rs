//! End to end determinism (review-m1.md should-fix 5): compiling the kart
//! from its intent on 1 thread and on 8 threads writes byte-identical
//! files: every tier's `.glb`, every normal map and the metadata.
//!
//! It also pins the normal maps' overlap count on the kart (should-fix 6).
//! An overlap is a texel whose centre lies strictly inside two triangles of
//! the same chart: a small fold where the flattened chart crosses itself
//! (surface.rs). The rounded box has none (`rounded_box_bakes_a_valid_seamless_map`);
//! the kart has a handful on its curved parts, a known M1 gap (report-m1.md:
//! T11's natural-parameter charts and conformal fallback are deferred to M2).
//! The test fails if the count grows, so a regression is caught, and if it
//! shrinks, so the report's number is updated with the fix.

use evani_model::api::{self, Workspace};
use std::path::{Path, PathBuf};

/// Texels claimed by two triangles of one chart, summed over every map of
/// every tier of kart_01_pip (measured 2026-10-15).
const KART_OVERLAPS: u64 = 19;

fn kart() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/model_bench/vehicles/kart_01_pip.evmodel")
}

fn files(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let Ok(rd) = std::fs::read_dir(dir) else { panic!("no output folder {}", dir.display()) };
    let mut out: Vec<(String, Vec<u8>)> = rd
        .flatten()
        .map(|e| {
            let p = e.path();
            let name = p.file_name().and_then(|x| x.to_str()).unwrap_or("?").to_string();
            let bytes = std::fs::read(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()));
            (name, bytes)
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn compile(threads: usize, tag: &str) -> (serde_json::Value, Vec<(String, Vec<u8>)>) {
    let root = std::env::temp_dir().join(format!("evani-model-determinism-{}", std::process::id()));
    let out = root.join(tag);
    let _ = std::fs::remove_dir_all(&out);
    let mut ws = Workspace::new(&root.join(format!("ws-{tag}")));
    ws.threads = threads;
    let r = match api::compile_model(&ws, &kart().to_string_lossy(), None, Some(&out)) {
        Ok(r) => r,
        Err(d) => panic!("kart compile on {threads} threads failed: {}", api::error_json(&d)),
    };
    let f = files(&out);
    let _ = std::fs::remove_dir_all(root.join(format!("ws-{tag}")));
    let _ = std::fs::remove_dir_all(&out);
    (r, f)
}

#[test]
fn kart_compiles_to_identical_bytes_on_1_and_8_threads() {
    let (r1, f1) = compile(1, "t1");
    let (r8, f8) = compile(8, "t8");
    let names1: Vec<&str> = f1.iter().map(|f| f.0.as_str()).collect();
    let names8: Vec<&str> = f8.iter().map(|f| f.0.as_str()).collect();
    assert_eq!(names1, names8, "the two compiles wrote different file sets");
    // Four tiers: one .glb each, at least one normal map each, plus the metadata.
    assert_eq!(names1.iter().filter(|n| n.ends_with(".glb")).count(), 4, "{names1:?}");
    assert!(names1.iter().filter(|n| n.ends_with(".normal.png")).count() >= 4, "{names1:?}");
    assert!(names1.iter().any(|n| n.ends_with(".evmodel.json")), "{names1:?}");
    for ((n, a), (_, b)) in f1.iter().zip(&f8) {
        assert!(a == b, "{n} differs between 1 and 8 threads ({} and {} bytes)", a.len(), b.len());
    }
    assert_eq!(r1["hash"], r8["hash"], "content hash differs between thread counts");
    let overlaps = r1["checks"]["normal_maps"]["overlaps"].as_u64().unwrap_or(u64::MAX);
    assert_eq!(overlaps, KART_OVERLAPS, "kart normal-map overlap count changed: update report-m1.md and this test");
}
