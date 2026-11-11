//! `evani --version` and `evani -V` (the `evani` command is this binary):
//! one line, `evani <x.y.z>`, the pack version in packaging/engine-pack.json,
//! exit 0.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code: a failed step is a failed test

use std::path::Path;
use std::process::Command;

#[test]
fn version_flag_prints_one_evani_version_line_and_exits_zero() {
    let pins = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/engine-pack.json");
    let pins: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(pins).unwrap()).unwrap();
    let pack_version = pins["pack_version"].as_str().unwrap().to_string();

    for flag in ["--version", "-V"] {
        let out = Command::new(env!("CARGO_BIN_EXE_evani-xtask")).arg(flag).env_remove("EVANI_ENGINE_ROOT").output().unwrap();
        assert!(out.status.success(), "{flag}: exit {:?}", out.status.code());
        let text = String::from_utf8(out.stdout).unwrap();
        assert_eq!(text, format!("evani {pack_version}\n"), "{flag}");
        // The format: the word `evani`, one space, three numbers separated by dots.
        let shown = text.trim_end().strip_prefix("evani ").unwrap();
        let parts: Vec<&str> = shown.split('.').collect();
        assert_eq!(parts.len(), 3, "{flag}: {shown}");
        assert!(parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())), "{flag}: {shown}");
    }
}
