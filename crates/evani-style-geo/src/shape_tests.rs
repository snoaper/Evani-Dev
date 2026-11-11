//! The geometry stage on real sources: the rigged horse (its low tier at
//! rest, with its mane and tail) and the code shapes, in every geometry
//! style, held to the recognisability check (`shapecheck`), the contract,
//! determinism, the triangle budget and the shipped grid's bytes.

use evani_studio::Catalogs;
use evani_studio::assets::AssetRow;
use evani_studio::geo::{SourceMesh, StyledMesh};
use evani_studio::styles::{Geometry, GeometryKind};

use crate::{generator, generator_version, rest, shapecheck, standin, voxel};

fn root() -> std::path::PathBuf {
    std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")).to_path_buf()
}

fn catalogs() -> Catalogs {
    Catalogs::load(&root()).unwrap()
}

fn horse(c: &Catalogs) -> (SourceMesh, AssetRow) {
    let row = c.assets.asset("animal.horse").unwrap().clone();
    let glb = std::fs::read(root().join("tests/fixtures/models/horse/horse.low.glb")).unwrap();
    let rest = rest::rest_from_glb(&glb).unwrap();
    (standin::horse_source(&rest, &row).unwrap(), row)
}

/// Every style that changes the shape and has a working generator.
fn shape_styles(c: &Catalogs) -> Vec<(String, Geometry)> {
    c.styles.styles.iter().filter(|s| !matches!(s.geometry.kind(), GeometryKind::None) && generator_version(s.geometry.kind()) > 0).map(|s| (s.id.clone(), s.geometry.clone())).collect()
}

fn run(src: &SourceMesh, row: &AssetRow, style: &str, g: &Geometry) -> StyledMesh {
    let out = generator(g.kind()).generate(src, row, g).unwrap_or_else(|e| panic!("{} {style}: {e}", row.id));
    out.validate(row.regions.len()).unwrap();
    out
}

/// The horse the viewer loads: its studio tier in the app, read by WP2's
/// source reader (regions by rule, mane and tail proxies, first detail
/// level only).
fn studio_horse(c: &Catalogs) -> (SourceMesh, AssetRow) {
    let row = c.assets.asset("animal.horse").unwrap().clone();
    let glb = std::fs::read(root().join("apps/material-view/assets/models/horse.glb")).unwrap();
    (crate::source::source_from_glb(&glb, &row).unwrap(), row)
}

/// The owner's test: a voxel horse is a horse built from cubes (head,
/// body, four legs, ears, mane, tail all still there, one piece), and so
/// is every other shape style's horse; each is deterministic.
#[test]
fn the_horse_is_recognisable_in_every_shape_style() {
    let c = catalogs();
    let (src, row) = horse(&c);
    assert_recognisable(&c, &src, &row);
}

/// The same on the horse the viewer really loads (the app's studio tier).
#[test]
fn the_studio_horse_is_recognisable_in_every_shape_style() {
    let c = catalogs();
    let (src, row) = studio_horse(&c);
    assert_recognisable(&c, &src, &row);
}

/// The quadruped generator's animals (WP7) as the viewer loads them: the
/// cow and the dog are recognisable in every shape style too, eyes and all.
#[test]
fn the_cow_and_dog_are_recognisable_in_every_shape_style() {
    let c = catalogs();
    for (id, model) in [("animal.cow", "cow"), ("animal.dog", "dog")] {
        let row = c.assets.asset(id).unwrap().clone();
        let glb = std::fs::read(root().join(format!("apps/material-view/assets/models/{model}.glb"))).unwrap();
        let src = crate::source::source_from_glb(&glb, &row).unwrap();
        assert_recognisable(&c, &src, &row);
    }
}

fn assert_recognisable(c: &Catalogs, src: &SourceMesh, row: &AssetRow) {
    let (src, row) = (src.clone(), row.clone());
    let styles = shape_styles(c);
    assert!(styles.iter().any(|s| s.1.kind() == GeometryKind::Voxel) && styles.iter().any(|s| s.1.kind() == GeometryKind::Lowpoly));
    let mut problems = Vec::new();
    for (id, g) in &styles {
        let out = run(&src, &row, id, g);
        assert_eq!(run(&src, &row, id, g), out, "{id}: deterministic");
        let rep = shapecheck::check(&src, &row, id, g.kind(), &out);
        println!("{id} {}: {} tris, iou {:?}, {} thin pieces, {} pieces", g.kind().name(), rep.triangles, rep.iou, rep.thin_pieces, rep.shape_pieces);
        // Every tagged part of the row is there and checked: four legs, two
        // ears, the mane (with its forelock), the tail and both eyes.
        assert!(rep.thin_pieces >= 10, "{id}: {} tagged pieces", rep.thin_pieces);
        problems.extend(rep.problems());
        // Every region the source has is still drawn.
        for r in 0..row.regions.len() as u8 {
            if src.regions.contains(&r) {
                assert!(out.parts.iter().any(|p| p.region == r), "{id}: region {} lost", row.regions[usize::from(r)].id);
            }
        }
    }
    assert!(problems.is_empty(), "{problems:#?}");
}

/// The voxel horse: one piece, every leg cubed down to the hoof, a cube
/// per cell centre, and the shipped grid within 8 KB, rebuilt exactly.
#[test]
fn the_voxel_horse_is_one_piece_and_ships_small() {
    let c = catalogs();
    let (src, row) = horse(&c);
    let Geometry::Voxel { cell, keep_thin } = c.styles.styles.iter().find(|s| s.geometry.kind() == GeometryKind::Voxel).unwrap().geometry else { unreachable!() };
    let size = voxel::fit_cell(&src, &row, cell);
    assert!(size < cell && size >= cell * voxel::MIN_FIT, "the horse's thin legs and ears make its cubes finer: {size}");
    let grid = voxel::voxelise(&src, &row, size, keep_thin).unwrap();
    assert_eq!(voxel::pieces(&grid).len(), 1);
    let bytes = grid.encode();
    println!("voxel horse: cube {size:.4}, dims {:?}, {} cells solid, {} bytes", grid.dims, grid.solid(), bytes.len());
    assert!(bytes.len() <= 8 * 1024, "{} bytes", bytes.len());
    let back = voxel::VoxelGrid::decode(&bytes).unwrap();
    assert_eq!(voxel::mesh_grid(&back), voxel::mesh_grid(&grid));
    // Each leg (the leg region's four pieces) has cells from its top to its
    // hoof: no cube layer missing along it.
    let leg = row.region("leg").unwrap() as u8 + 1;
    let hoof = row.region("hoof").unwrap() as u8 + 1;
    let cols: Vec<(usize, usize)> = (0..grid.dims[1]).map(|y| {
        let n = (0..grid.dims[2]).flat_map(|z| (0..grid.dims[0]).map(move |x| (x, z))).filter(|&(x, z)| matches!(grid.cells[grid.index(x, y, z)], v if v == leg || v == hoof)).count();
        (y, n)
    }).collect();
    let low: Vec<&(usize, usize)> = cols.iter().filter(|c| c.1 > 0).collect();
    let (y0, y1) = (low.first().unwrap().0, low.last().unwrap().0);
    assert!((y0..=y1).all(|y| cols[y].1 > 0), "a layer of the legs is empty: {cols:?}");
}

/// A low-poly bake round-trips through its bytes and stays within 8 KB.
#[test]
fn a_low_poly_bake_round_trips() {
    let c = catalogs();
    let (src, row) = horse(&c);
    let m = crate::lowpoly::simplify(&src, &row, 500);
    let b = m.encode();
    assert!(b.len() <= 8 * 1024, "{} bytes", b.len());
    let back = crate::lowpoly::Simplified::decode(&b).unwrap();
    assert_eq!((back.faces.clone(), back.regions.clone()), (m.faces.clone(), m.regions.clone()));
    let worst = back.positions.iter().zip(&m.positions).map(|(a, b)| (0..3).map(|k| (a[k] - b[k]).abs()).fold(0.0f32, f32::max)).fold(0.0f32, f32::max);
    assert!(worst < 1e-4, "{worst}");
    assert!(crate::lowpoly::Simplified::decode(&b[..b.len() - 1]).is_err());
}

/// Low-poly lands near its target, inside the 300 to 800 an animal gets.
#[test]
fn the_low_poly_horse_keeps_its_budget() {
    let c = catalogs();
    let (src, row) = horse(&c);
    for s in &c.styles.styles {
        if let Geometry::Lowpoly { target_tris, .. } | Geometry::Papercraft { target_tris, .. } = s.geometry {
            let out = run(&src, &row, &s.id, &s.geometry);
            let n = out.triangles();
            assert!((300..=800).contains(&n), "{}: {n} triangles", s.id);
            assert!(n.abs_diff(target_tris as usize) <= target_tris as usize / 10, "{}: {n} for a target of {target_tris}", s.id);
        }
    }
}

/// Every code-built asset (sphere, cube, wall, roof, ground patch) from
/// WP2's real code sources, in every shape style: recognisable, every
/// region still drawn, deterministic.
#[test]
fn the_code_shapes_keep_their_shape_in_every_style() {
    let c = catalogs();
    let rows: Vec<&AssetRow> = c.assets.assets.iter().filter(|r| r.source.shape.is_some()).collect();
    for id in ["shape.sphere", "shape.cube", "building.wall", "building.roof", "land.patch"] {
        assert!(rows.iter().any(|r| r.id == id), "{id} has no code source row");
    }
    for row in rows {
        let shape = row.source.shape.clone().unwrap();
        let src = crate::code::code_source(&shape).unwrap_or_else(|| panic!("{}: no code source `{shape}`", row.id));
        for (sid, g) in shape_styles(&c) {
            let out = run(&src, row, &sid, &g);
            assert_eq!(run(&src, row, &sid, &g), out, "{} {sid}: deterministic", row.id);
            let rep = shapecheck::check(&src, row, &sid, g.kind(), &out);
            println!("{} {sid} {}: {} tris, iou {:?}", row.id, g.kind().name(), rep.triangles, rep.iou);
            assert!(rep.problems().is_empty(), "{} {sid}: {:#?}", row.id, rep.problems());
            for r in 0..row.regions.len() as u8 {
                if src.regions.contains(&r) {
                    assert!(out.parts.iter().any(|p| p.region == r), "{} {sid}: region {} lost", row.id, row.regions[usize::from(r)].id);
                }
            }
        }
    }
}

/// Errors become tools: the check catches a lost leg, a broken-off part
/// and a squashed silhouette.
#[test]
fn the_shape_check_catches_a_lost_leg() {
    let c = catalogs();
    let (src, row) = horse(&c);
    let g = c.styles.styles.iter().find(|s| s.geometry.kind() == GeometryKind::Lowpoly).unwrap().geometry.clone();
    let good = run(&src, &row, "lowpoly", &g);
    let leg = row.region("leg").unwrap() as u8;
    let mut no_legs = good.clone();
    no_legs.parts.retain(|p| p.region != leg);
    let rep = shapecheck::check(&src, &row, "lowpoly", g.kind(), &no_legs);
    assert!(rep.thin_failures.iter().filter(|f| f.contains("leg")).count() >= 4, "{rep:?}");
    let mut squashed = good.clone();
    for p in &mut squashed.parts {
        for v in &mut p.positions {
            v[1] *= 0.4;
        }
    }
    let rep = shapecheck::check(&src, &row, "lowpoly", g.kind(), &squashed);
    assert!(rep.problems().iter().any(|p| p.contains("silhouette")), "{rep:?}");
    let mut apart = good;
    for p in &mut apart.parts {
        if p.region == leg {
            for v in &mut p.positions {
                v[1] -= 0.3;
            }
        }
    }
    let rep = shapecheck::check(&src, &row, "lowpoly", g.kind(), &apart);
    assert!(rep.problems().iter().any(|p| p.contains("pieces")), "{rep:?}");
}

/// A thin part far thinner than a cube still gets a cube of its own
/// region (the keep pass), on a block with a paper-thin fin.
#[test]
fn a_paper_thin_part_keeps_a_cube() {
    let c = catalogs();
    let row = c.assets.asset("animal.horse").unwrap();
    let ear = row.region("ear").unwrap() as u8;
    let mut src = crate::code::code_source("cube").unwrap();
    // A fin 0.002 thick standing on the cube's top, all `ear`.
    let base = src.positions.len() as u32;
    let top = 1.0 / 3f32.sqrt();
    for (x, y, z) in [(-0.2, top - 0.05, 0.0), (0.2, top - 0.05, 0.0), (0.2, top + 0.3, 0.0), (-0.2, top + 0.3, 0.0), (-0.2, top - 0.05, 0.002), (0.2, top - 0.05, 0.002), (0.2, top + 0.3, 0.002), (-0.2, top + 0.3, 0.002)] {
        src.positions.push([x, y, z]);
        src.normals.push([0.0, 0.0, 1.0]);
        src.regions.push(ear);
    }
    let q = |a: u32, b: u32, c: u32, d: u32| [base + a, base + b, base + c, base + a, base + c, base + d];
    for f in [q(0, 3, 2, 1), q(4, 5, 6, 7), q(0, 1, 5, 4), q(3, 7, 6, 2), q(0, 4, 7, 3), q(1, 2, 6, 5)] {
        src.indices.extend(f);
    }
    let grid = voxel::voxelise(&src, row, 0.12, true).unwrap();
    assert!(grid.cells.contains(&(ear + 1)), "the fin kept no cube");
    let out = voxel::mesh_grid(&grid);
    let rep = shapecheck::check(&src, row, "voxel", GeometryKind::Voxel, &out);
    assert!(rep.thin_failures.is_empty(), "{rep:?}");
}
