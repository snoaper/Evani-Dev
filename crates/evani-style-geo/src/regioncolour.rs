//! An asset's region colours from its own materials (linear RGB, in the
//! row's region order), for the dev pictures and the asset browser's
//! thumbnails (`examples/studio_assets.rs thumb`).
//!
//! Each region takes, in this order: its own `default_material`'s first
//! colour; else (it leads or follows the coat) the asset's default
//! material, its first colour, or for a `points` region that colour blended
//! toward the material's third colour by its `points` amount (a bay's black
//! legs); a `keep` region with neither (eyes, a dog's nose) is near black.
//! So no region ever shows a made-up hue: a cow's legs are its coat, not
//! the blue of a debug palette.

use evani_studio::assets::{AssetRow, Tone};
use serde_json::Value;

/// The colour of a region that keeps the source model's own (eyes, nose).
pub const KEEP: [f32; 3] = [0.02, 0.018, 0.016];

fn linear(hex: &str) -> Option<[f32; 3]> {
    let h = hex.trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let ch = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok().map(|v| (f32::from(v) / 255.0).powf(2.2));
    Some([ch(0)?, ch(2)?, ch(4)?])
}

/// A material row's colours (linear) and its `points` amount.
fn material(rows: &[Value], id: &str) -> Option<(Vec<[f32; 3]>, f32)> {
    let row = rows.iter().find(|r| r.get("id").and_then(Value::as_str) == Some(id))?;
    let params = row.get("params")?;
    if let Some(target) = params.get("alias_of").and_then(Value::as_str)
        && params.get("colours").is_none()
    {
        return material(rows, target);
    }
    let cols: Vec<[f32; 3]> = params.get("colours")?.as_array()?.iter().filter_map(Value::as_str).filter_map(linear).collect();
    let points = params.get("numbers").and_then(|n| n.get("points")).and_then(Value::as_f64).unwrap_or(0.0) as f32;
    (!cols.is_empty()).then_some((cols, points))
}

/// The row's region colours from `catalog/materials/materials.json`'s text.
/// Errors name a material the catalogue lacks.
pub fn region_colours(asset: &AssetRow, materials_json: &str) -> Result<Vec<[f32; 3]>, String> {
    let v: Value = serde_json::from_str(materials_json).map_err(|e| format!("materials.json: {e}"))?;
    let rows = v.get("rows").and_then(Value::as_array).ok_or("materials.json: no `rows` list")?;
    let find = |id: &str| material(rows, id).ok_or_else(|| format!("{}: material `{id}` has no colours in materials.json", asset.id));
    let (coat, points) = find(&asset.default_material)?;
    let mut out = Vec::with_capacity(asset.regions.len());
    for r in &asset.regions {
        let c = if let Some(m) = &r.default_material {
            find(m)?.0[0]
        } else if r.keep {
            KEEP
        } else if r.tone == Tone::Points {
            let to = coat.get(2).copied().unwrap_or(coat[0]);
            [0, 1, 2].map(|k| coat[0][k] + (to[k] - coat[0][k]) * points)
        } else {
            coat[0]
        };
        out.push(c);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use evani_studio::Catalogs;

    use super::*;

    fn root() -> std::path::PathBuf {
        std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")).to_path_buf()
    }

    /// Every animal's regions take its materials' colours: legs, ears and
    /// tails follow the coat (no debug hues), hooves and muzzles their own
    /// materials, and a bay's points are dark.
    #[test]
    fn every_animal_is_coloured_from_its_materials() {
        let c = Catalogs::load(&root()).unwrap();
        let json = std::fs::read_to_string(root().join("catalog/materials/materials.json")).unwrap();
        let mut seen = 0;
        for a in c.assets.assets.iter().filter(|a| a.id.starts_with("animal.")) {
            let cols = region_colours(a, &json).unwrap();
            let coat = cols[a.region("coat").unwrap()];
            for (r, col) in a.regions.iter().zip(&cols) {
                if r.follows.as_deref() == Some("coat") && r.tone == Tone::Base {
                    assert_eq!(*col, coat, "{} {}: follows the coat", a.id, r.id);
                }
                // A debug palette's blue or green never shows on an animal.
                assert!(!(col[2] > col[0] * 1.3 && col[2] > col[1]), "{} {}: blue {col:?}", a.id, r.id);
                assert!(!(col[1] > col[0] * 1.3 && col[1] > col[2] * 1.3), "{} {}: green {col:?}", a.id, r.id);
            }
            seen += 1;
        }
        assert!(seen >= 3, "the horse, cow and dog");
        let horse = c.assets.asset("animal.horse").unwrap();
        let cols = region_colours(horse, &json).unwrap();
        let leg = cols[horse.region("leg").unwrap()];
        assert!(leg.iter().all(|x| *x < 0.05), "a bay's legs are dark: {leg:?}");
    }
}
