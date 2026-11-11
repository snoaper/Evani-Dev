//! The horse's studio source as the geometry stage's tests and
//! `cargo xtask style-bake` read it until WP2's `source::source_from_glb`
//! lands (docs/studio/WORK_PACKAGES.md WP2): the tier at rest
//! ([`crate::rest`]), the solid mane and tail built from its own anchors
//! ([`crate::proxy`]), each vertex given one of the asset row's regions from
//! the body chart, then centred and scaled to radius 1.
//!
//! The chart rules here are a stand-in, written for the horse's chart
//! (`_EVANI_REGION`: along, around, distal; region.rs on the horse-model
//! branch): glTF material `hoof` and `eye` by name, an ear is anything off
//! the head's surface (distal > 0 with along under 0.15), the dock is along
//! over 0.85, a leg is distal elsewhere, the muzzle is the front of the
//! head. Region names come from the row, never guessed past it.

use evani_studio::assets::AssetRow;
use evani_studio::geo::SourceMesh;

use crate::math::{V3, bounds, len, scale, sub};
use crate::proxy;
use crate::rest::RestMesh;

/// Picture colours for the horse row's regions, in its order (coat, leg,
/// ear, mane, tail, hoof, muzzle, eye): a bay. Dev pictures only; the
/// studio colours regions from their materials.
pub const HORSE_COLOURS: [[f32; 3]; 8] = [
    [0.42, 0.20, 0.09],
    [0.07, 0.05, 0.045],
    [0.36, 0.17, 0.08],
    [0.05, 0.04, 0.035],
    [0.05, 0.04, 0.035],
    [0.16, 0.15, 0.14],
    [0.20, 0.10, 0.06],
    [0.01, 0.01, 0.01],
];

fn region(asset: &AssetRow, id: &str) -> Result<u8, String> {
    asset.region(id).map(|r| r as u8).ok_or_else(|| format!("{}: the row has no `{id}` region", asset.id))
}

/// The horse row's source from its rest tier.
pub fn horse_source(rest: &RestMesh, asset: &AssetRow) -> Result<SourceMesh, String> {
    let [coat, leg, ear, mane, tail, hoof, muzzle, eye] = ["coat", "leg", "ear", "mane", "tail", "hoof", "muzzle", "eye"].map(|r| region(asset, r));
    let (coat, leg, ear, mane, tail, hoof, muzzle, eye) = (coat?, leg?, ear?, mane?, tail?, hoof?, muzzle?, eye?);
    let mut positions = rest.positions.clone();
    let mut normals = rest.normals.clone();
    let mut indices = rest.indices.clone();
    let mut regions: Vec<u8> = (0..rest.positions.len())
        .map(|v| {
            let [a, _g, d, _] = rest.chart[v].map(|b| f32::from(b) / 255.0);
            match rest.material_name(v) {
                "hoof" => hoof,
                "eye" => eye,
                _ if a <= 0.0 => coat,
                _ if a < 0.15 && d > 0.02 => ear,
                _ if a < 0.045 => muzzle,
                _ if a > 0.85 => tail,
                _ if d > 0.4 => leg,
                _ => coat,
            }
        })
        .collect();
    for (part, r) in [(proxy::mane(rest), mane), (proxy::tail(rest), tail)] {
        let Some(p) = part else { return Err(format!("{}: the tier has no anchors for its {}", asset.id, if r == mane { "mane" } else { "tail" })) };
        let base = positions.len() as u32;
        positions.extend(&p.positions);
        normals.extend(&p.normals);
        indices.extend(p.indices.iter().map(|i| base + i));
        regions.extend(std::iter::repeat_n(r, p.positions.len()));
    }
    let mut src = SourceMesh { positions, normals, indices, regions };
    to_radius_one(&mut src);
    src.validate(asset.regions.len())?;
    Ok(src)
}

/// Centre on the bounds and scale so the farthest vertex is at radius 1.
pub fn to_radius_one(src: &mut SourceMesh) {
    let (lo, hi) = bounds(&src.positions);
    let c: V3 = scale([lo[0] + hi[0], lo[1] + hi[1], lo[2] + hi[2]], 0.5);
    let r = src.positions.iter().map(|p| len(sub(*p, c))).fold(0.0f32, f32::max);
    let k = if r > 0.0 { 1.0 / r } else { 1.0 };
    for p in &mut src.positions {
        *p = scale(sub(*p, c), k);
    }
}

