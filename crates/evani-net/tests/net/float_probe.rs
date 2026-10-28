//! Cross-target float determinism probe: the float kernel natively and as
//! wasm32 (compiled by `rustc` alone and run in Node, no dependencies, no
//! imports), compared family by family.
//!
//! Families IEEE 754 fixes exactly (arithmetic, sqrt, fused multiply-add,
//! rounding, casts, and a kart-like step built from them) must agree:
//! that is what lets a native tool reproduce a browser's run. Families that
//! go through a maths library (`sin`, `exp`, `powf`...) and NaN payloads may
//! differ in the last bit between native and wasm; they are reported, not
//! failed, because the shipped referee and every browser run the same wasm
//! (the same maths library compiled in), so they agree with each other.
//!
//! Needs `rustc` with the `wasm32-unknown-unknown` target and `node` on the
//! PATH; without either the test prints SKIPPED and what is missing.

use std::path::PathBuf;
use std::process::Command;

use crate::float_kernel::{EXACT, FAMILIES, run};

const ITERS: u32 = 20_000;

fn wasm_hashes() -> Result<Vec<u64>, String> {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("float-probe");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/net/float_probe_wasm.rs");
    let wasm = dir.join("float_probe.wasm");
    let out = Command::new("rustc")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args([
            "--edition",
            "2021",
            "--crate-type",
            "cdylib",
            "--target",
            "wasm32-unknown-unknown",
            "-C",
            "opt-level=2",
            "-o",
        ])
        .arg(&wasm)
        .arg(&src)
        .output()
        .map_err(|e| format!("rustc not found ({e})"))?;
    if !out.status.success() {
        return Err(format!(
            "rustc could not build the wasm32 probe (`rustup target add wasm32-unknown-unknown`?): {}",
            String::from_utf8_lossy(&out.stderr)
                .lines()
                .take(3)
                .collect::<Vec<_>>()
                .join(" | ")
        ));
    }
    let js = format!(
        "const fs=require('fs');\
         const m=new WebAssembly.Module(fs.readFileSync(process.argv[1]));\
         const imports=WebAssembly.Module.imports(m);\
         if(imports.length){{console.log('IMPORTS '+imports.map(i=>i.module+'.'+i.name).join(','));process.exit(0);}}\
         const e=new WebAssembly.Instance(m,{{}}).exports;\
         const n=e.evani_probe_families();\
         for(let f=0;f<n;f++){{console.log(BigInt.asUintN(64,e.evani_probe(f,{ITERS})).toString(16));}}"
    );
    let out = Command::new("node")
        .arg("-e")
        .arg(&js)
        .arg(&wasm)
        .output()
        .map_err(|e| format!("node not found ({e})"))?;
    if !out.status.success() {
        return Err(format!(
            "node failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    if let Some(l) = text.lines().find(|l| l.starts_with("IMPORTS")) {
        return Err(format!("the probe module has imports: {l}"));
    }
    text.lines()
        .map(|l| {
            u64::from_str_radix(l.trim(), 16).map_err(|e| format!("bad node output {l:?}: {e}"))
        })
        .collect()
}

#[test]
fn float_results_agree_between_native_and_wasm32() {
    let native: Vec<u64> = (0..FAMILIES.len() as u32).map(|f| run(f, ITERS)).collect();
    // The kernel itself is repeatable.
    assert_eq!(
        native,
        (0..FAMILIES.len() as u32)
            .map(|f| run(f, ITERS))
            .collect::<Vec<_>>()
    );
    let wasm = match wasm_hashes() {
        Ok(w) => w,
        Err(why) => {
            println!("SKIPPED float probe (native vs wasm32): {why}");
            return;
        }
    };
    assert_eq!(wasm.len(), FAMILIES.len());
    let mut table = String::new();
    let mut exact_bad = Vec::new();
    for (i, name) in FAMILIES.iter().enumerate() {
        let same = native[i] == wasm[i];
        let kind = if i < EXACT { "exact" } else { "libm/NaN" };
        table.push_str(&format!(
            "  {name:<24} {kind:<9} {}\n",
            if same { "same bits" } else { "DIFFERENT" }
        ));
        if i < EXACT && !same {
            exact_bad.push(*name);
        }
    }
    println!(
        "float probe, native ({}) vs wasm32 in node, {ITERS} inputs per family:\n{table}",
        std::env::consts::ARCH
    );
    assert!(
        exact_bad.is_empty(),
        "IEEE-exact float families differ between native and wasm32: {exact_bad:?}\n{table}"
    );
}
