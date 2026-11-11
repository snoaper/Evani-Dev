//! Guard: a metal's colour A is its F0 (reflectance at normal incidence) stored
//! as sRGB8. Published F0 values are LINEAR. Typing them in as hex without the
//! linear-to-sRGB curve makes warm metals dark and orange (gold became f4c45a
//! instead of ffe29b, copper e6946c instead of fad1c2). Grey metals hide the
//! mistake, so every coloured metal row is held to a reference table.
#![cfg(feature = "catalog")]
#![allow(clippy::expect_used)]

use serde_json::json;
use std::path::Path;

/// Linear F0 of the reference metals (Hoffman, "Physics and Math of Shading").
const F0_GOLD: [f64; 3] = [1.000, 0.766, 0.336];
const F0_COPPER: [f64; 3] = [0.955, 0.638, 0.538];
const F0_BRASS: [f64; 3] = [0.910, 0.778, 0.423];
const F0_NICKEL: [f64; 3] = [0.660, 0.609, 0.526];
/// Rose gold: 35 % gold, 65 % copper. Bronze: no published value; an alloy
/// leaning copper-red, chosen between copper and brass.
const F0_ROSE: [f64; 3] = [0.9775, 0.7186, 0.4553];
const F0_BRONZE: [f64; 3] = [0.80, 0.50, 0.27];

/// (row id, linear F0, lowest share of that F0 a worn, aged or tinted row may
/// keep). A row passes when its colour A, read as sRGB, has the F0's hue (the
/// channels divided by the brightest agree within 0.14 in linear light) and a
/// brightness between that share and 1.05 of the F0.
const METAL_F0: [(&str, [f64; 3], f64); 16] = [
    ("metal.gold.polished", F0_GOLD, 0.95),
    ("metal.gold.rose", F0_ROSE, 0.9),
    ("metal.copper.polished", F0_COPPER, 0.95),
    ("metal.copper.hammered", F0_COPPER, 0.85),
    ("metal.brass.polished", F0_BRASS, 0.95),
    ("metal.brass.aged", F0_BRASS, 0.6),
    ("metal.bronze", F0_BRONZE, 0.85),
    ("metal.nickel", F0_NICKEL, 0.85),
    ("fit.button.brass", F0_BRASS, 0.85),
    ("fit.zip", F0_BRASS, 0.8),
    ("fit.rivet.copper", F0_COPPER, 0.85),
    ("ammo.cartridge.brass", F0_BRASS, 0.8),
    ("teeth.gold", F0_GOLD, 0.8),
    ("finish.gilt", F0_GOLD, 0.8),
    ("kitchen.copper_pot", F0_COPPER, 0.7),
    ("build.copper.roof", F0_COPPER, 0.6),
];

/// Coloured metal rows that are deliberately not an F0.
const METAL_NOT_F0: [(&str, &str); 3] = [
    ("metal.steel.blued", "blued steel is a thin dark-blue oxide film, not bare-metal reflectance"),
    ("glass.mirror_old", "a foxed mirror: a tarnished silver backing under glass, tinted by the spotting"),
    ("cloth.sequins", "sequins are lacquer-tinted metallised film discs (gold here), not a bare metal"),
];

fn srgb_to_linear(c: u8) -> f64 {
    let c = f64::from(c) / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn linear_to_srgb_hex(l: [f64; 3], k: f64) -> String {
    l.iter()
        .map(|&c| {
            let c = (c * k).clamp(0.0, 1.0);
            let s = if c <= 0.003_130_8 { 12.92 * c } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
            format!("{:02x}", (s * 255.0).round() as u8)
        })
        .collect()
}

fn hex_rgb(h: &str) -> Option<[u8; 3]> {
    let n = u32::from_str_radix(h, 16).ok().filter(|_| h.len() == 6)?;
    Some([(n >> 16) as u8, (n >> 8) as u8, n as u8])
}

/// Every error the metal colour guard finds in the catalogue's rows.
fn metal_colour_errors(rows: &[serde_json::Value]) -> Vec<String> {
    let mut errors = Vec::new();
    for row in rows {
        let id = row["id"].as_str().unwrap_or("?");
        let metal = row["params"]["numbers"]["metal"].as_f64().unwrap_or(1.0);
        if row["recipe"] != "metal" || row["status"] != "ready" || metal < 0.9 {
            continue;
        }
        let Some(a) = row["params"]["colours"][0].as_str() else { continue };
        let Some(rgb) = hex_rgb(a) else {
            errors.push(format!("ERROR {id}: colour A `{a}` is not a six-digit hex\n  fix: write it as sRGB hex, rrggbb"));
            continue;
        };
        let lin = rgb.map(srgb_to_linear);
        let chroma = rgb.iter().max().copied().unwrap_or(0) - rgb.iter().min().copied().unwrap_or(0);
        if let Some(&(_, f0, kmin)) = METAL_F0.iter().find(|e| e.0 == id) {
            let lmax = lin.iter().cloned().fold(0.0, f64::max);
            let fmax = f0.iter().cloned().fold(0.0, f64::max);
            let hue_off = (0..3).map(|i| (lin[i] / lmax - f0[i] / fmax).abs()).fold(0.0, f64::max);
            let k = lmax / fmax;
            if hue_off > 0.14 || !(kmin..=1.05).contains(&k) {
                errors.push(format!(
                    "ERROR {id}: colour A `{a}` is not the sRGB of this metal's published F0 (read as sRGB it is linear {:.2}/{:.2}/{:.2}, the F0 is linear {:.2}/{:.2}/{:.2}; hue off by {hue_off:.2}, brightness {k:.2} of F0, allowed {kmin} to 1.05)\n  fix: published F0 values are LINEAR; run them through the linear-to-sRGB curve before writing the hex. Full F0 is `{}`; a worn or aged row may scale the linear F0 by {kmin} to 1 first (`{}` at {kmin}).",
                    lin[0], lin[1], lin[2], f0[0], f0[1], f0[2], linear_to_srgb_hex(f0, 1.0), linear_to_srgb_hex(f0, kmin)
                ));
            }
        } else if chroma >= 20 && !METAL_NOT_F0.iter().any(|e| e.0 == id) {
            errors.push(format!(
                "ERROR {id}: coloured metal (colour A `{a}`) has no published F0 in METAL_F0, so its colour cannot be checked\n  fix: add the row to METAL_F0 with its published linear F0 (and enter colour A as the sRGB of it), or to METAL_NOT_F0 with the reason it is not a bare-metal reflectance."
            ));
        }
    }
    errors
}

fn catalogue_rows() -> Vec<serde_json::Value> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../catalog/materials/materials.json");
    let text = std::fs::read_to_string(path).expect("readable");
    let v: serde_json::Value = serde_json::from_str(&text).expect("json");
    v["rows"].as_array().expect("rows").clone()
}

/// The catalogue's coloured metals are entered as sRGB, not as published linear F0.
#[test]
fn metal_colours_are_srgb_of_the_published_f0() {
    let rows = catalogue_rows();
    let errors = metal_colour_errors(&rows);
    assert!(errors.is_empty(), "metal colours entered as linear values (or unchecked):\n{}", errors.join("\n"));
    // The table covers rows that exist and are drawn, so a rename cannot silently drop one.
    for (id, _, _) in METAL_F0 {
        assert!(rows.iter().any(|r| r["id"] == id && r["status"] == "ready"), "{id} in METAL_F0 is not a ready row");
    }
}

/// The guard fails on the review's mistakes (the old linear-entered gold,
/// copper and brass) and on a coloured metal nobody tabled; it passes the fix.
#[test]
fn the_metal_guard_catches_linear_entered_colours() {
    let row = |id: &str, a: &str| json!({"id": id, "recipe": "metal", "status": "ready", "params": {"colours": [a, "ffffff", "000000"], "numbers": {"metal": 1}}});
    assert!(metal_colour_errors(&[row("metal.gold.polished", "ffe29b"), row("metal.copper.polished", "fad1c2"), row("metal.brass.polished", "f5e4ac")]).is_empty());
    let old = metal_colour_errors(&[row("metal.gold.polished", "f4c45a"), row("metal.copper.polished", "e6946c"), row("metal.brass.polished", "dcb860")]);
    assert_eq!(old.len(), 3, "{old:?}");
    assert!(old[0].contains("fix:") && old[0].contains("metal.gold.polished") && old[0].contains("ffe39d"), "{}", old[0]);
    let unknown = metal_colour_errors(&[row("metal.new.alloy", "d08040")]);
    assert_eq!(unknown.len(), 1);
    assert!(unknown[0].contains("fix:"));
    // A grey metal and a low-metal paint are not held to the table.
    let paint = json!({"id": "roof.x", "recipe": "metal", "status": "ready", "params": {"colours": ["8a3a2c"], "numbers": {"metal": 0.35}}});
    assert!(metal_colour_errors(&[row("metal.steel.x", "c6c6c8"), paint]).is_empty());
}
