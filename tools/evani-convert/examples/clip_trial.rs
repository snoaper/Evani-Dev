//! Plan item R1's measurement: bake every animated fixture's clips with each
//! key-reduction choice at every tier bound in ladder.json and print the
//! clip sizes, the worst error and each clip's own bake time (fastest of three
//! runs, each animation baked alone),
//! as a Markdown table. The split (Douglas-Peucker) key choice was measured
//! with this tool at commit a12be8bf and left out (DECISIONS.md "Clip baker:
//! hierarchy-aware tolerance (R1)").
//!
//! `cargo run --release -p evani-convert --example clip_trial [-- <file.glb>...]`
//! (no files: the rig fixture and the dog, cow and horse masters).

#![allow(clippy::print_stdout)] // a measurement tool: the table is its output

use std::path::{Path, PathBuf};
use std::time::Instant;

use evani_convert::clips::{BakeOptions, bake_with};
use evani_convert::fixture;
use evani_convert::glb::Doc;
use evani_gltf::Gltf;

const BOUNDS: [(&str, f64); 5] = [("lite", 0.002), ("low", 0.001), ("medium", 0.0005), ("high", 0.0003), ("ultra", 0.0002)];

fn modes() -> Vec<(String, BakeOptions)> {
    let o = |chain, refine, probe| BakeOptions { chain, refine, probe };
    let mut v = vec![
        ("legacy (S3-7)".to_string(), BakeOptions::LEGACY),
        ("probe".into(), o(false, 0, true)),
        ("refine 4".into(), o(false, 4, true)),
    ];
    for r in [0, 2, 4, 6, 8] {
        let opt = o(true, r, true);
        v.push((format!("chain, refine {r}{}", if opt == BakeOptions::default() { " (default)" } else { "" }), opt));
    }
    v
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n == 0 {
        0.0
    } else if n % 2 == 1 {
        v[n / 2]
    } else {
        0.5 * (v[n / 2 - 1] + v[n / 2])
    }
}

/// One single-animation copy of `glb` per animation (each bakes, and is
/// timed, as one clip).
fn split_clips(glb: &[u8]) -> Result<Vec<Gltf>, String> {
    let d = Doc::read_glb(glb)?;
    let anims = d.json.get("animations").and_then(|a| a.as_array()).cloned().unwrap_or_default();
    let mut out = Vec::new();
    for a in anims {
        let mut one = Doc::read_glb(glb)?;
        one.json["animations"] = serde_json::Value::Array(vec![a]);
        out.push(Gltf::from_glb(&one.to_glb()?).map_err(|e| e.0)?);
    }
    Ok(out)
}

fn main() -> Result<(), String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).ok_or("no repo root")?.to_path_buf();
    let args: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    if args.is_empty() {
        files.push(("rig".into(), fixture::rig_glb()?));
        for a in ["dog", "cow", "horse"] {
            let p = root.join(format!("tests/fixtures/models/{a}/{a}.master.glb"));
            files.push((a.into(), std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?));
        }
    } else {
        for p in args {
            files.push((p.display().to_string(), std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?));
        }
    }
    let modes = modes();
    println!("| File | Clip | Tier | Bound mm | {} |", modes.iter().map(|(n, _)| format!("{n} bytes (worst mm, time, tries)")).collect::<Vec<_>>().join(" | "));
    println!("|---|---|---|---:|{}", "---:|".repeat(modes.len()));
    // Per mode: each clip's bytes and bake time against the S3-7 baker's.
    let mut ratio: Vec<Vec<f64>> = vec![Vec::new(); modes.len()];
    let mut time: Vec<Vec<f64>> = vec![Vec::new(); modes.len()];
    let mut tries: Vec<Vec<f64>> = vec![Vec::new(); modes.len()];
    let mut totals = vec![0usize; modes.len()];
    let mut all_within = vec![true; modes.len()];
    for (name, bytes) in &files {
        let clips = split_clips(bytes)?;
        for (tier, bound) in BOUNDS {
            for g in &clips {
                let mut row: Vec<String> = Vec::new();
                let mut legacy = (0usize, 0.0f64, false, 0u32);
                for (m, (_, opt)) in modes.iter().enumerate() {
                    // The fastest of three runs: the PC is shared, so one run is noisy.
                    let mut ms = f64::INFINITY;
                    let mut b = None;
                    for _ in 0..3 {
                        let t = Instant::now();
                        let x = bake_with(g, bound, &Some, *opt)?;
                        ms = ms.min(t.elapsed().as_secs_f64() * 1000.0);
                        b = Some(x);
                    }
                    let b = b.ok_or("no bake")?;
                    let c = b.clips.first().ok_or("no clip")?;
                    let n = b.tries.first().copied().unwrap_or(0);
                    if m == 0 {
                        legacy = (c.bytes, ms, c.within, n);
                        row.extend([name.clone(), format!("`{}`", c.name), tier.into(), format!("{:.1}", bound * 1000.0)]);
                    }
                    let (lb, lms, lwithin, ln) = legacy;
                    tries[m].push(f64::from(n) / f64::from(ln.max(1)));
                    // A clip no choice can bring within its bound (the storage's
                    // own limit) keeps its most accurate form: not a size result.
                    if lwithin {
                        ratio[m].push(c.bytes as f64 / lb.max(1) as f64);
                    }
                    // Clips that bake in under 5 ms are timer noise, not a cost.
                    if lms >= 5.0 {
                        time[m].push(ms / lms);
                    }
                    totals[m] += c.bytes;
                    all_within[m] &= c.within || !lwithin;
                    row.push(format!("{} ({:.3}{}, {:.0} ms, {n} tries)", c.bytes, c.worst_shell_error_m * 1000.0, if c.within { "" } else { " OVER" }, ms));
                }
                println!("| {} |", row.join(" | "));
            }
        }
    }
    println!();
    println!("| Mode | Median clip size vs legacy (clips legacy holds) | Largest clip size vs legacy | Total bytes | Median clip time vs legacy | Slowest clip vs legacy | Median tries vs legacy | Most tries vs legacy | Every clip legacy holds is held |");
    println!("|---|---:|---:|---:|---:|---:|---:|---:|---|");
    let max = |v: &[f64]| v.iter().copied().fold(0.0, f64::max);
    for (m, (n, _)) in modes.iter().enumerate() {
        println!(
            "| {n} | {:.1} % | {:.1} % | {} | {:.2}x | {:.2}x | {:.2}x | {:.2}x | {} |",
            median(ratio[m].clone()) * 100.0,
            max(&ratio[m]) * 100.0,
            totals[m],
            median(time[m].clone()),
            max(&time[m]),
            median(tries[m].clone()),
            max(&tries[m]),
            if all_within[m] { "yes" } else { "NO" }
        );
    }
    Ok(())
}
