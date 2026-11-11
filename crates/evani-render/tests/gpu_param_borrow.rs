//! The GPU side never asks for a mutable borrow of the param registry.
//!
//! The frame (gpu/frame.rs) holds a shared borrow of `ParamRegistry` while
//! every GPU module prepares, so `try_res_mut::<ParamRegistry>()` there
//! always fails and the module silently falls back to its defaults: live
//! switches never reach the GPU. That hid the recipe stage's quality, extras
//! and condition switches until batch 1 P3. GPU code reads params with
//! `try_res`; anything that writes params runs as a plugin system instead.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::Path;

#[test]
fn gpu_modules_only_read_the_param_registry() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/gpu");
    let mut bad = Vec::new();
    for entry in fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap();
        for (n, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            if code.contains("res_mut::<ParamRegistry>") || code.contains("res_mut::<evani_core::params::ParamRegistry>") {
                bad.push(format!("{}:{}", path.display(), n + 1));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "GPU code borrows the param registry mutably (always fails while the frame reads it; \
         read with try_res and move writes to a plugin system): {bad:?}"
    );
}
