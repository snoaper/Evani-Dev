//! `cargo xtask style-bake <asset|--all> [--check] [--shots <dir>] [--write]`:
//! the geometry stage at build time (docs/studio/WORK_PACKAGES.md WP3).
//!
//! For every asset whose source is in the tree and every style that
//! changes the shape (voxel, low-poly, papercraft, clay, yarn), it runs the
//! generator and prints a table: triangles, generation time, the shipped
//! bytes (a voxel grid, or a low-poly mesh the viewer re-facets on load),
//! the silhouette overlap with the source from three views and the thin
//! parts kept (`evani_style_geo::shapecheck`).
//!
//! - `--check`: fail on any shape that isn't recognisable (the table says
//!   which part and why).
//! - `--shots <dir>`: front, side and three-quarter pictures of the source
//!   and every shape, `<asset>-<style>-<view>.png`.
//! - `--write`: write each bake to
//!   `apps/material-view/assets/styles/<asset>/<style>.bin`, headed by its
//!   geometry key, for the viewer to fetch instead of generating.
//!
//! Sources: a model asset from its studio tier
//! (`apps/material-view/assets/models/<model>.glb`) through WP2's
//! `source::source_from_glb` (the horse falls back to the rigged test
//! fixture when its tier is missing); `code` shapes through
//! `code::code_source`. An asset without a source yet is listed as waiting,
//! never stood in for.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use evani_studio::Catalogs;
use evani_studio::assets::{AssetRow, SourceKind};
use evani_studio::geo::SourceMesh;
use evani_studio::styles::{Geometry, GeometryKind};
use evani_style_geo::view::{Frame, Soup, VIEWS, picture};
use evani_style_geo::{code, generator, generator_version, lowpoly, rest, shapecheck, source, standin, voxel};

use crate::workspace_root;

/// The bake of one shape: what the viewer would fetch.
fn bake_bytes(src: &SourceMesh, asset: &AssetRow, g: &Geometry) -> Result<Option<Vec<u8>>, String> {
    Ok(match *g {
        Geometry::Voxel { cell, keep_thin } => {
            let size = voxel::fit_cell(src, asset, cell);
            Some(voxel::voxelise(src, asset, size, keep_thin)?.encode())
        }
        Geometry::Lowpoly { target_tris, .. } | Geometry::Papercraft { target_tris, .. } => Some(lowpoly::simplify(src, asset, target_tris as usize).encode()),
        _ => None,
    })
}

/// An asset's source, if its model or code shape is in the tree.
fn source(root: &Path, asset: &AssetRow) -> Result<Option<SourceMesh>, String> {
    if matches!(asset.source.kind, SourceKind::Code) {
        return Ok(asset.source.shape.as_deref().and_then(code::code_source));
    }
    // The app's studio tier, read by WP2's source reader (regions by rule).
    if let Some(model) = &asset.source.model {
        let path = root.join("apps/material-view/assets/models").join(format!("{model}.glb"));
        if path.is_file() {
            let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            return source::source_from_glb(&bytes, asset).map(Some);
        }
    }
    // The horse before its studio tier: the rigged test fixture.
    if asset.id == "animal.horse" {
        let path = root.join("tests/fixtures/models/horse/horse.low.glb");
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        return Ok(Some(standin::horse_source(&rest::rest_from_glb(&bytes)?, asset)?));
    }
    Ok(None)
}

/// A plain colour per region for the pictures: the horse's bay, else a
/// spread of hues.
fn colour(asset: &AssetRow, r: u8) -> [f32; 3] {
    if asset.id == "animal.horse" {
        return standin::HORSE_COLOURS.get(usize::from(r)).copied().unwrap_or([0.5; 3]);
    }
    let h = f32::from(r) * 0.31 + 0.08;
    let f = |k: f32| 0.35 + 0.3 * (std::f32::consts::TAU * (h + k)).cos();
    [f(0.0), f(1.0 / 3.0), f(2.0 / 3.0)]
}

fn write_png(path: &Path, w: usize, h: usize, rgb: &[u8]) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut wr = enc.write_header().map_err(|e| e.to_string())?;
    wr.write_image_data(rgb).map_err(|e| e.to_string())
}

pub fn cmd_style_bake(args: &[String]) -> Result<(), String> {
    let root = workspace_root();
    let c = Catalogs::load(&root)?;
    let flag = |f: &str| args.iter().any(|a| a == f);
    let mut shots: Option<PathBuf> = None;
    let mut wanted: Option<String> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--shots" => shots = Some(PathBuf::from(it.next().ok_or("--shots needs a folder")?)),
            "--check" | "--write" | "--all" => {}
            s if s.starts_with("--") => return Err(format!("style-bake: unknown flag {s}")),
            s => wanted = Some(s.to_string()),
        }
    }
    if wanted.is_none() && !flag("--all") {
        return Err("usage: cargo xtask style-bake <asset id|--all> [--check] [--shots <dir>] [--write]".into());
    }
    let styles: Vec<_> = c.styles.styles.iter().filter(|s| !matches!(s.geometry.kind(), GeometryKind::None) && generator_version(s.geometry.kind()) > 0).collect();
    let mut problems = Vec::new();
    let mut waiting = Vec::new();
    println!("{:<14} {:<5} {:<10} {:>6} {:>8} {:>7}  {:<17} thin", "asset", "style", "shape", "tris", "ms", "bytes", "iou front/side/3q");
    for asset in &c.assets.assets {
        if wanted.as_deref().is_some_and(|w| w != asset.id) {
            continue;
        }
        let Some(src) = source(&root, asset)? else {
            waiting.push(asset.id.clone());
            continue;
        };
        let mut sets = vec![("source".to_string(), Soup::from_source(&src))];
        for s in &styles {
            let t = Instant::now();
            let out = generator(s.geometry.kind()).generate(&src, asset, &s.geometry)?;
            let ms = t.elapsed().as_secs_f64() * 1e3;
            let bytes = bake_bytes(&src, asset, &s.geometry)?;
            let rep = shapecheck::check(&src, asset, &s.id, s.geometry.kind(), &out);
            let thin = if rep.thin_failures.is_empty() { format!("{} kept", rep.thin_pieces) } else { format!("{} LOST", rep.thin_failures.len()) };
            println!(
                "{:<14} {:<5} {:<10} {:>6} {:>8.1} {:>7}  {:.2}/{:.2}/{:.2}   {thin}",
                asset.id,
                s.id,
                s.geometry.kind().name(),
                out.triangles(),
                ms,
                bytes.as_ref().map_or("-".to_string(), |b| b.len().to_string()),
                rep.iou[0],
                rep.iou[1],
                rep.iou[2]
            );
            problems.extend(rep.problems());
            if let (true, Some(b)) = (flag("--write"), &bytes) {
                let key = evani_studio::resolve::geometry_key(&c, &asset.id, &s.geometry, &BTreeMap::new());
                let dir = root.join("apps/material-view/assets/styles").join(&asset.id);
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                let mut file = format!("{key}").into_bytes();
                file.extend(b);
                std::fs::write(dir.join(format!("{}.bin", s.id)), file).map_err(|e| e.to_string())?;
            }
            sets.push((format!("{}-{}", s.id, s.geometry.kind().name()), Soup::from_styled(&out)));
        }
        if let Some(dir) = &shots {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            for v in VIEWS {
                let frame = Frame::fit(&sets[0].1, v, 720, 540, 0.06);
                for (name, soup) in &sets {
                    let img = picture(soup, &frame, &|r| colour(asset, r), [236, 238, 242]);
                    write_png(&dir.join(format!("{}-{name}-{}.png", asset.id, v.name)), frame.w, frame.h, &img)?;
                }
            }
        }
    }
    if !waiting.is_empty() {
        println!("waiting for a source (being built): {}", waiting.join(", "));
    }
    if let Some(d) = &shots {
        println!("pictures: {}", d.display());
    }
    if flag("--check") && !problems.is_empty() {
        for p in &problems {
            eprintln!("ERROR {p}");
        }
        return Err(format!("{} shape(s) not recognisable", problems.len()));
    }
    Ok(())
}
