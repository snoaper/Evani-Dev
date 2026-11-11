//! `cargo xtask check`: a game's kit texture sets (evani-render feature
//! `kit-textures`, TopGun W3; docs/GROUND_TEXTURES.md). Reads the same
//! `kit-textures.json` the engine reads at start, so the check and the game
//! can't disagree. It catches:
//!
//! * a set that doesn't list every tier (a tier the game forgot loads
//!   nothing, silently);
//! * a layer file missing for a size some tier asks for, in any shipped
//!   format (the device that picks that format would get a 404);
//! * a file the engine would refuse: not plain KTX2, supercompressed, not
//!   BC7 / ASTC 4x4 / RGBA8 as its name says, the wrong size, no full mip
//!   chain;
//! * a set over its download budget on a tier (`budget_mb`, else
//!   [`DEFAULT_BUDGET_MB`]): what one device downloads, the largest format;
//! * kit WGSL over the shader cap once the engine's forward module and the
//!   texture bindings are added ([`MAX_SHADER_BYTES`]);
//! * the manifest without the feature (the game would ship files it never
//!   binds).

use std::path::Path;

use serde_json::Value;

/// The manifest, in the game's web root (`<app>/assets`).
pub const MANIFEST: &str = "kit-textures.json";
/// Largest forward module with kit code (evani-render `kit_textures::MAX_SHADER_BYTES`).
pub const MAX_SHADER_BYTES: usize = 60 * 1024;
/// What the engine's forward module and the texture bindings take before
/// the kit code (evani-render's test `the_forward_module_leaves_the_kit_its_share`
/// keeps this at or above the real size).
pub const FORWARD_ALLOWANCE: usize = 24 * 1024;
/// Download per tier (MB, one device format) when the manifest sets none:
/// TopGun W3's budgets (blocking start at most 4 MB, a mission at most
/// 25 MB on High and 60 MB on Ultra).
pub const DEFAULT_BUDGET_MB: &[(&str, f64)] = &[("lite", 4.0), ("low", 4.0), ("medium", 12.0), ("high", 25.0), ("ultra", 60.0), ("vr", 25.0)];

/// One finding: error or warning, what, and the fix.
#[derive(Debug, Clone, PartialEq)]
pub struct Problem {
    pub error: bool,
    pub what: String,
    pub fix: String,
}

fn err(what: String, fix: String) -> Problem {
    Problem { error: true, what, fix }
}

/// The formats a file name may carry and the vkFormats each allows.
fn vk_allowed(fmt: &str) -> &'static [u32] {
    match fmt {
        "astc" => &[157, 158],
        "bc7" => &[145, 146],
        "rgba" => &[37, 43],
        _ => &[],
    }
}

/// Check one layer file's header against its name: `(width, levels)` or why
/// the engine would refuse it.
pub fn file_problem(b: &[u8], fmt: &str, size: u32) -> Result<(), String> {
    let vk = crate::modelcheck::ktx2_vk_format(b).ok_or("not a KTX2 file")?;
    if !vk_allowed(fmt).contains(&vk) {
        return Err(format!("vkFormat {vk} is not what `.{fmt}.` promises"));
    }
    let u = |o: usize| b.get(o..o + 4).map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]])).ok_or("truncated header");
    let q = |o: usize| b.get(o..o + 8).map(|x| u64::from_le_bytes([x[0], x[1], x[2], x[3], x[4], x[5], x[6], x[7]])).ok_or("truncated level index");
    let (w, h, depth, layers, faces, levels) = (u(20)?, u(24)?, u(28)?, u(32)?, u(36)?, u(40)?.max(1));
    if u(44)? != 0 {
        return Err("supercompressed (the engine reads plain levels)".into());
    }
    if depth > 1 || layers > 1 || faces != 1 {
        return Err("not a plain 2D texture".into());
    }
    let want = if fmt == "rgba" { size / 2 } else { size };
    if (w, h) != (want, want) {
        return Err(format!("{w}x{h}, the name says {size} ({want}x{want} for `.{fmt}.`)"));
    }
    let full = 32 - want.max(1).leading_zeros();
    if levels != full {
        return Err(format!("{levels} mip levels, a {want}x{want} texture needs the full chain of {full}"));
    }
    let (bw, bb) = if fmt == "rgba" { (1u64, 4u64) } else { (4, 16) };
    for i in 0..levels {
        let at = 80 + 24 * i as usize;
        let (off, len) = (q(at)?, q(at + 8)?);
        if off.checked_add(len).is_none_or(|end| end > b.len() as u64) {
            return Err(format!("level {i} runs past the end of the file"));
        }
        let lw = u64::from((want >> i).max(1));
        if len != lw.div_ceil(bw) * lw.div_ceil(bw) * bb {
            return Err(format!("level {i} is {len} bytes, not what {lw}x{lw} needs"));
        }
    }
    Ok(())
}

/// Every finding for the game whose web root is `assets` (`shown` names it
/// in messages). `tiers`: the engine's tier ids; `feature_on`: whether the
/// game's crates turn on evani-render's `kit-textures`; `kit_wgsl`: bytes
/// of the kit's WGSL files; `tier_mem`: each tier's `max_texture_memory`
/// in MB (kit textures may take at most half of it).
pub fn problems(assets: &Path, shown: &str, tiers: &[String], feature_on: bool, kit_wgsl: usize, tier_mem: &[(String, u32)]) -> Vec<Problem> {
    let mut out = Vec::new();
    let path = assets.join(MANIFEST);
    let doc = format!("{shown}/{MANIFEST}");
    if feature_on && kit_wgsl + FORWARD_ALLOWANCE > MAX_SHADER_BYTES {
        out.push(err(
            format!("the kit's WGSL is {} KB; with the engine's forward module (~{} KB) the shader is over the {} KB cap", kit_wgsl / 1024, FORWARD_ALLOWANCE / 1024, MAX_SHADER_BYTES / 1024),
            format!("Shrink the kit's shader code below {} KB: drop unused branches, share functions between materials, move tables into kit_data.", (MAX_SHADER_BYTES - FORWARD_ALLOWANCE) / 1024),
        ));
    }
    let Ok(text) = std::fs::read_to_string(&path) else {
        return out;
    };
    if !feature_on {
        out.push(err(
            format!("{doc} is there but evani-render's `kit-textures` feature is off, so the game downloads nothing it lists and binds dummies"),
            "Turn on `evani-render/kit-textures` in the kit's Cargo.toml (or delete the manifest if the game doesn't use ground textures).".into(),
        ));
        return out;
    }
    let v: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            out.push(err(format!("{doc}: {e}"), format!("Fix the JSON in {doc} (docs/GROUND_TEXTURES.md has the format).")));
            return out;
        }
    };
    let formats: Vec<String> = match v.get("formats").and_then(Value::as_array) {
        Some(a) => a.iter().filter_map(Value::as_str).map(str::to_string).collect(),
        None => vec!["astc".into(), "bc7".into(), "rgba".into()],
    };
    for f in &formats {
        if vk_allowed(f).is_empty() {
            out.push(err(format!("{doc}: format `{f}` is not astc, bc7 or rgba"), format!("List only astc, bc7 and rgba under \"formats\" in {doc}.")));
        }
    }
    let budget = |tier: &str| -> f64 {
        v.get("budget_mb").and_then(|b| b.get(tier)).and_then(Value::as_f64).or_else(|| DEFAULT_BUDGET_MB.iter().find(|(t, _)| *t == tier).map(|(_, m)| *m)).unwrap_or(25.0)
    };
    // Per tier, per format: bytes every set downloads.
    let mut per_tier: Vec<(String, Vec<(String, u64)>)> = tiers.iter().map(|t| (t.clone(), formats.iter().map(|f| (f.clone(), 0)).collect())).collect();
    // What the start downloads before every wanted layer shows (W3c).
    let mut boot_tier = per_tier.clone();
    // GPU memory per tier (bytes).
    let mut vram: Vec<(String, u64)> = tiers.iter().map(|t| (t.clone(), 0)).collect();
    let boot_budget = v.get("boot_budget_mb").and_then(Value::as_f64).unwrap_or(DEFAULT_BOOT_BUDGET_MB);
    for s in v.get("sets").and_then(Value::as_array).into_iter().flatten() {
        let set = s.get("set").and_then(Value::as_u64).unwrap_or(0);
        let sizes = s.get("size").and_then(Value::as_object);
        let size_of = |t: &str| sizes.and_then(|o| o.get(t)).and_then(Value::as_u64).map(|n| n as u32);
        for t in tiers {
            if size_of(t).is_none() {
                out.push(err(format!("{doc}: set {set} doesn't list tier `{t}`, so that tier loads none of its layers"), format!("Add \"{t}\": <size> (0 to leave the set off) to set {set}'s \"size\" in {doc}.")));
            }
        }
        let mut wanted: Vec<u32> = tiers.iter().filter_map(|t| size_of(t)).filter(|n| *n > 0).collect();
        // W3c: the boot size loads first wherever it is below the tier's.
        let boot = s.get("boot").and_then(Value::as_u64).unwrap_or(0) as u32;
        let boot_of = |t: &str| size_of(t).filter(|n| boot > 0 && boot < *n).map(|_| boot);
        if boot > 0 && (!boot.is_power_of_two() || boot < 8) {
            out.push(err(format!("{doc}: set {set} boot size {boot} is not a power of two of at least 8"), "Use 128 or 256 (small enough for the start budget).".into()));
        }
        if tiers.iter().any(|t| boot_of(t).is_some()) {
            wanted.push(boot);
        }
        wanted.sort_unstable();
        wanted.dedup();
        for t in tiers {
            // GPU memory of the set on this tier: every layer, both maps, the
            // tier size (BC7 / ASTC 1 byte a texel with mips), the boot arrays
            // too while they stream.
            if let Some(n) = size_of(t).filter(|n| *n > 0) {
                let layers = s.get("layers").and_then(Value::as_array).map_or(0, Vec::len) as u64;
                let per = |n: u32| (u64::from(n) * u64::from(n) * 4 / 3) * 2 * layers;
                let bytes = per(n) + boot_of(t).map_or(0, per);
                if let Some(x) = vram.iter_mut().find(|(g, _)| g == t) {
                    x.1 += bytes;
                }
            }
        }
        for n in &wanted {
            if !n.is_power_of_two() || *n < 8 {
                out.push(err(format!("{doc}: set {set} size {n} is not a power of two of at least 8"), "Use 512, 1024 or 2048 (the engine's tier sizes).".into()));
            }
        }
        let layers = s.get("layers").and_then(Value::as_array).cloned().unwrap_or_default();
        if layers.is_empty() || layers.len() > 16 {
            out.push(err(format!("{doc}: set {set} has {} layers", layers.len()), "Give each set 1 to 16 layers.".into()));
        }
        for l in &layers {
            let Some(p) = l.get("path").and_then(Value::as_str) else {
                out.push(err(format!("{doc}: a layer of set {set} has no \"path\""), "Give every layer a \"path\" (files are <path>_ah.<size>.<fmt>.ktx2 and <path>_nra...).".into()));
                continue;
            };
            for n in &wanted {
                for map in ["_ah", "_nra"] {
                    for f in &formats {
                        let rel = format!("{p}{map}.{n}.{f}.ktx2");
                        let file = assets.join(&rel);
                        match std::fs::read(&file) {
                            Err(_) => out.push(err(
                                format!("texture layer `{p}` is missing {rel}: tiers at size {n} on `{f}` devices get nothing"),
                                format!("Make it with `cargo xtask convert-textures` (docs/GROUND_TEXTURES.md), or drop size {n} / format `{f}` from {doc}."),
                            )),
                            Ok(b) => {
                                if let Err(e) = file_problem(&b, f, *n) {
                                    out.push(err(format!("{shown}/{rel}: {e}"), "Remake it with `cargo xtask convert-textures` (plain KTX2, full mips, the size in its name; rgba at half size).".into()));
                                }
                                let len = b.len() as u64;
                                for (t, row) in &mut per_tier {
                                    if (size_of(t) == Some(*n) || boot_of(t) == Some(*n))
                                        && let Some(x) = row.iter_mut().find(|(g, _)| g == f)
                                    {
                                        x.1 += len;
                                    }
                                }
                                // The start: the boot size, or the tier's size for a set without one.
                                for (t, row) in &mut boot_tier {
                                    let first = boot_of(t).or_else(|| size_of(t).filter(|s| *s > 0));
                                    if first == Some(*n)
                                        && let Some(x) = row.iter_mut().find(|(g, _)| g == f)
                                    {
                                        x.1 += len;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    for (t, row) in &per_tier {
        let (f, bytes) = row.iter().max_by_key(|(_, b)| *b).cloned().unwrap_or_default();
        let mb = bytes as f64 / (1024.0 * 1024.0);
        if mb > budget(t) {
            out.push(err(
                format!("kit textures download {mb:.1} MB on tier `{t}` (`{f}` devices), over its {:.1} MB budget", budget(t)),
                format!("Lower that tier's sizes in {doc}, drop layers it doesn't need, or raise \"budget_mb\".{t} if the owner agrees."),
            ));
        }
    }
    for (t, row) in &boot_tier {
        let (f, bytes) = row.iter().max_by_key(|(_, b)| *b).cloned().unwrap_or_default();
        let mb = bytes as f64 / (1024.0 * 1024.0);
        if mb > boot_budget {
            out.push(err(
                format!("kit textures start with {mb:.1} MB on tier `{t}` (`{f}` devices) before every layer shows, over the {boot_budget:.1} MB start budget"),
                format!("Give the big sets a \"boot\" size (128 or 256) in {doc} so they stream the rest after the start, lower that tier's sizes, or raise \"boot_budget_mb\" if the owner agrees."),
            ));
        }
    }
    for (t, bytes) in &vram {
        let cap = tier_mem.iter().find(|(g, _)| g == t).map_or(0, |(_, m)| *m);
        let mb = *bytes as f64 / (1024.0 * 1024.0);
        if cap > 0 && mb > f64::from(cap) / 2.0 {
            out.push(err(
                format!("kit textures need {mb:.0} MB of GPU memory on tier `{t}`, over half its {cap} MB texture budget (max_texture_memory)"),
                format!("Lower that tier's sizes in {doc} or split the layers so a mission loads fewer of them (the game's `KitTextures::want`)."),
            ));
        }
    }
    out
}

/// The start download budget in MB when the manifest sets none (TopGun W3:
/// at most 4 MB blocking before take-off).
pub const DEFAULT_BOOT_BUDGET_MB: f64 = 4.0;

/// Each tier's `max_texture_memory` (MB) from the engine's tier files.
pub fn tier_memory(tiers_dir: &Path) -> Vec<(String, u32)> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(tiers_dir).into_iter().flatten().flatten() {
        let p = e.path();
        let (Some(id), Ok(text)) = (p.file_stem().and_then(|s| s.to_str()), std::fs::read_to_string(&p)) else { continue };
        if let Some(m) = serde_json::from_str::<Value>(&text).ok().and_then(|v| v.get("max_texture_memory").and_then(Value::as_u64)) {
            out.push((id.to_string(), m as u32));
        }
    }
    out
}

/// Bytes of every `.wgsl` file under `dir` (the kit's shader code).
pub fn wgsl_bytes(dir: &Path) -> usize {
    let mut n = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                if p.file_name().is_none_or(|f| f != "target") {
                    stack.push(p);
                }
            } else if p.extension().is_some_and(|x| x == "wgsl") {
                n += e.metadata().map_or(0, |m| m.len() as usize);
            }
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A KTX2 header + levels: `vk`, square `w`, `levels`, bytes per level
    /// from the format.
    fn ktx(vk: u32, w: u32, levels: u32, rgba: bool) -> Vec<u8> {
        let mut b = vec![0u8; 80 + 24 * levels as usize];
        b[..12].copy_from_slice(&[0xAB, 0x4B, 0x54, 0x58, 0x20, 0x32, 0x30, 0xBB, 0x0D, 0x0A, 0x1A, 0x0A]);
        let put = |b: &mut Vec<u8>, o: usize, x: u32| b[o..o + 4].copy_from_slice(&x.to_le_bytes());
        put(&mut b, 12, vk);
        put(&mut b, 20, w);
        put(&mut b, 24, w);
        put(&mut b, 36, 1);
        put(&mut b, 40, levels);
        for i in 0..levels {
            let lw = u64::from((w >> i).max(1));
            let len = if rgba { lw * lw * 4 } else { lw.div_ceil(4) * lw.div_ceil(4) * 16 };
            let off = b.len() as u64;
            let at = 80 + 24 * i as usize;
            b[at..at + 8].copy_from_slice(&off.to_le_bytes());
            b[at + 8..at + 16].copy_from_slice(&len.to_le_bytes());
            b.extend(std::iter::repeat_n(0u8, len as usize));
        }
        b
    }

    #[test]
    fn layer_files_are_checked_against_their_names() {
        assert!(file_problem(&ktx(145, 64, 7, false), "bc7", 64).is_ok());
        assert!(file_problem(&ktx(158, 64, 7, false), "astc", 64).is_ok());
        assert!(file_problem(&ktx(43, 32, 6, true), "rgba", 64).is_ok());
        assert!(file_problem(&ktx(145, 64, 7, false), "astc", 64).is_err(), "format");
        assert!(file_problem(&ktx(145, 32, 6, false), "bc7", 64).is_err(), "size");
        assert!(file_problem(&ktx(145, 64, 1, false), "bc7", 64).is_err(), "mips");
        assert!(file_problem(b"nope", "bc7", 64).is_err());
    }

    fn game(files: &[(&str, Vec<u8>)], manifest: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("evani-groundtex-{}-{}", std::process::id(), crate::fnv_hex(manifest.as_bytes())));
        let _ = std::fs::remove_dir_all(&dir);
        for (rel, b) in files {
            let p = dir.join(rel);
            let _ = std::fs::create_dir_all(p.parent().unwrap_or(&dir));
            let _ = std::fs::write(p, b);
        }
        let _ = std::fs::write(dir.join(MANIFEST), manifest);
        dir
    }

    #[test]
    fn a_missing_tier_a_missing_file_and_an_over_budget_set_are_errors() {
        let tiers: Vec<String> = ["low", "high"].iter().map(|s| s.to_string()).collect();
        let ok = r#"{"formats":["bc7"],"sets":[{"set":0,"size":{"low":0,"high":64},"layers":[{"name":"rock","path":"g/rock"}]}]}"#;
        let files = [("g/rock_ah.64.bc7.ktx2", ktx(146, 64, 7, false)), ("g/rock_nra.64.bc7.ktx2", ktx(145, 64, 7, false))];
        let dir = game(&files, ok);
        assert_eq!(problems(&dir, "app", &tiers, true, 1000, &[]), vec![]);
        // Off: the manifest is an error on its own.
        assert_eq!(problems(&dir, "app", &tiers, false, 0, &[]).len(), 1);
        // A tier not listed.
        let missing_tier = r#"{"formats":["bc7"],"sets":[{"set":0,"size":{"high":64},"layers":[{"name":"rock","path":"g/rock"}]}]}"#;
        let p = problems(&game(&files, missing_tier), "app", &tiers, true, 0, &[]);
        assert!(p.iter().any(|p| p.error && p.what.contains("doesn't list tier `low`")), "{p:?}");
        // A tier's file missing (low now wants 32).
        let missing_file = r#"{"formats":["bc7"],"sets":[{"set":0,"size":{"low":32,"high":64},"layers":[{"name":"rock","path":"g/rock"}]}]}"#;
        let p = problems(&game(&files, missing_file), "app", &tiers, true, 0, &[]);
        assert!(p.iter().any(|p| p.error && p.what.contains("missing g/rock_ah.32.bc7.ktx2")), "{p:?}");
        // Over budget.
        let tight = r#"{"formats":["bc7"],"budget_mb":{"high":0.001},"sets":[{"set":0,"size":{"low":0,"high":64},"layers":[{"name":"rock","path":"g/rock"}]}]}"#;
        let p = problems(&game(&files, tight), "app", &tiers, true, 0, &[]);
        assert!(p.iter().any(|p| p.error && p.what.contains("over its 0.0 MB budget")), "{p:?}");
        // The shader cap.
        let p = problems(&dir, "app", &tiers, true, MAX_SHADER_BYTES, &[]);
        assert!(p.iter().any(|p| p.error && p.what.contains("cap")), "{p:?}");
        for m in [ok, missing_tier, missing_file, tight] {
            let _ = std::fs::remove_dir_all(game(&[], m));
        }
    }

    /// W3c: a boot size needs its files, the start download has its own
    /// budget, and a tier's GPU memory stays under half its texture budget.
    #[test]
    fn boot_files_the_start_budget_and_gpu_memory() {
        let tiers: Vec<String> = ["low", "high"].iter().map(|s| s.to_string()).collect();
        let full = [("g/rock_ah.64.bc7.ktx2", ktx(146, 64, 7, false)), ("g/rock_nra.64.bc7.ktx2", ktx(145, 64, 7, false))];
        let booted = r#"{"formats":["bc7"],"sets":[{"set":0,"boot":16,"size":{"low":0,"high":64},"layers":[{"name":"rock","path":"g/rock"}]}]}"#;
        let p = problems(&game(&full, booted), "app", &tiers, true, 0, &[]);
        assert!(p.iter().any(|p| p.error && p.what.contains("missing g/rock_ah.16.bc7.ktx2")), "{p:?}");
        let mut all = full.to_vec();
        all.push(("g/rock_ah.16.bc7.ktx2", ktx(146, 16, 5, false)));
        all.push(("g/rock_nra.16.bc7.ktx2", ktx(145, 16, 5, false)));
        let dir = game(&all, booted);
        assert_eq!(problems(&dir, "app", &tiers, true, 0, &[]), vec![]);
        // The start counts the boot files only: a budget between the two passes.
        let boot_bytes: u64 = all[2..].iter().map(|(_, b)| b.len() as u64).sum();
        let mb = (boot_bytes as f64 * 1.5) / (1024.0 * 1024.0);
        let tight = booted.replace("\"sets\"", &format!("\"boot_budget_mb\":{mb},\"sets\""));
        assert_eq!(problems(&game(&all, &tight), "app", &tiers, true, 0, &[]), vec![]);
        // Without the boot size the whole tier size is the start.
        let unbooted = tight.replace("\"boot\":16,", "");
        let p = problems(&game(&full, &unbooted), "app", &tiers, true, 0, &[]);
        assert!(p.iter().any(|p| p.error && p.what.contains("start budget")), "{p:?}");
        // GPU memory: 64 x 64 x 2 maps is ~11 KB; a 0 MB cap is no cap, 1 MB passes.
        assert_eq!(problems(&dir, "app", &tiers, true, 0, &[("high".into(), 1)]), vec![]);
        let big = r#"{"formats":["bc7"],"budget_mb":{"high":100000},"boot_budget_mb":100000,"sets":[{"set":0,"size":{"low":0,"high":8192},"layers":[{"name":"rock","path":"g/rock"}]}]}"#;
        let p = problems(&game(&[("readme.txt", Vec::new())], big), "app", &tiers, true, 0, &[("high".into(), 96)]);
        assert!(p.iter().any(|p| p.error && p.what.contains("GPU memory on tier `high`")), "{p:?}");
        for m in [booted, tight.as_str(), unbooted.as_str(), big] {
            let _ = std::fs::remove_dir_all(game(&[], m));
        }
        assert!(tier_memory(Path::new("../../assets/tiers")).iter().any(|(t, m)| t == "high" && *m == 512));
    }
}
