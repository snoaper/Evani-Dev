//! Parts: every asset is made of named parts, each taking its own material
//! (docs/studio/CONTRACTS.md section 2, "Parts", and section 6).

use std::collections::BTreeMap;
use std::path::Path;

use crate::Catalogs;
use crate::resolve::{BoundBy, Resolved, part_bindings, resolve};
use crate::state::{Notice, Pick, Saved, Selection, Settings, StudioState};

const ROOF: &str = "building.roof";
const WALL: &str = "building.wall";
const HORSE: &str = "animal.horse";
const COW: &str = "animal.cow";
const TOON: &str = "WK01";

fn cats() -> Catalogs {
    Catalogs::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))).unwrap()
}

fn res(c: &Catalogs, s: &Selection) -> Resolved {
    resolve(c, s, &BTreeMap::new(), None).unwrap()
}

/// Part id -> what it wears.
fn wears(c: &Catalogs, s: &Selection) -> BTreeMap<String, Option<String>> {
    let r = res(c, s);
    part_bindings(c, &r).into_iter().map(|(p, b)| (p.id.clone(), b.material.clone())).collect()
}

fn on_asset(c: &Catalogs, asset: &str) -> StudioState {
    let mut s = StudioState::new(c).unwrap();
    s.apply(c, Pick::Asset(asset.into())).unwrap();
    s
}

/// Every asset has parts; every region is in exactly one part; the roof and
/// the wall have the parts the owner asked for.
#[test]
fn every_asset_has_parts() {
    let c = cats();
    for a in &c.assets.assets {
        assert!(!a.parts.is_empty(), "{}", a.id);
        for (i, r) in a.regions.iter().enumerate() {
            assert!(a.part_of_region(i).is_some(), "{}: region {} in no part", a.id, r.id);
        }
    }
    let ids = |a: &str| c.assets.asset(a).unwrap().parts.iter().map(|p| p.id.clone()).collect::<Vec<_>>();
    assert_eq!(ids(ROOF), ["tiles", "ridge", "gable", "fascia"]);
    assert_eq!(ids(WALL), ["face", "cap", "plinth"]);
    assert_eq!(ids(HORSE), ["coat", "mane", "tail", "hooves", "muzzle", "eyes"]);
    // The roof opens with its own material on each part: terracotta tiles,
    // a clay ridge, brick gables, an oak fascia (never tiles on the gable).
    let s = Selection::new(ROOF, TOON, "roof.tile.terracotta");
    let w = wears(&c, &s);
    assert_eq!(w["tiles"].as_deref(), Some("roof.tile.terracotta"));
    assert_eq!(w["ridge"].as_deref(), Some("roof.tile.flat_clay"));
    assert_eq!(w["gable"].as_deref(), Some("brick.red.common"));
    assert_eq!(w["fascia"].as_deref(), Some("wood.beam.oak"));
    assert_eq!(wears(&c, &Selection::new(WALL, TOON, "brick.red.common"))["cap"].as_deref(), Some("stonework.limestone"));
}

/// A part's material change touches only that part: the other bindings
/// stay, and the geometry key doesn't move.
#[test]
fn a_part_material_change_touches_only_that_part() {
    let c = cats();
    let mut s = on_asset(&c, ROOF);
    let before = res(&c, &s.selection);
    s.apply(&c, Pick::Part(Some("gable".into()))).unwrap();
    s.apply(&c, Pick::Material("plaster.white".into())).unwrap();
    assert_eq!(s.selection.parts.get("gable").map(String::as_str), Some("plaster.white"));
    let after = res(&c, &s.selection);
    assert_eq!(after.geometry_key, before.geometry_key);
    assert_ne!(after.variant_key, before.variant_key);
    for (b, a) in before.bindings.iter().zip(&after.bindings) {
        if a.region == "gable" {
            assert_eq!(a.material.as_deref(), Some("plaster.white"));
            assert_eq!(a.from, BoundBy::Part("gable".into()));
        } else {
            assert_eq!(a, b, "{} changed", a.region);
        }
    }
    // A horse coat can't go on the gable: nothing changes, the notice says so.
    let rec = s.selection.clone();
    s.apply(&c, Pick::Material("horse.coat.bay".into())).unwrap();
    assert_eq!(s.selection, rec);
    assert!(matches!(s.notice, Notice::NotApplied { .. } | Notice::NotOnPart { .. }), "{:?}", s.notice);
    // Glass doesn't go on the hooves part (keratin only).
    let mut h = on_asset(&c, HORSE);
    h.apply(&c, Pick::PartMaterial { part: "hooves".into(), material: "plaster.white".into() }).unwrap();
    assert!(matches!(h.notice, Notice::NotOnPart { .. }), "{:?}", h.notice);
    assert!(h.selection.parts.is_empty());
}

/// "All parts": a roof tile goes on the tiles and the ridge, never on the
/// brick gable; giving the selected part its own default back stores
/// nothing (the record stays canonical, so click order can't matter).
#[test]
fn all_parts_spreads_only_where_a_part_is_made_for_it() {
    let c = cats();
    let mut s = on_asset(&c, ROOF);
    s.apply(&c, Pick::Material("roof.tile.slate".into())).unwrap();
    let w = wears(&c, &s.selection);
    assert_eq!(w["tiles"].as_deref(), Some("roof.tile.slate"));
    assert_eq!(w["ridge"].as_deref(), Some("roof.tile.slate"));
    assert_eq!(w["gable"].as_deref(), Some("brick.red.common"));
    assert_eq!(w["fascia"].as_deref(), Some("wood.beam.oak"));
    // Two orders, one record.
    let mut a = on_asset(&c, ROOF);
    let mut b = on_asset(&c, ROOF);
    for (st, order) in [(&mut a, ["gable", "fascia"]), (&mut b, ["fascia", "gable"])] {
        for p in order {
            let m = if p == "gable" { "stonework.ashlar" } else { "wood.plank.painted" };
            st.apply(&c, Pick::PartMaterial { part: p.into(), material: m.into() }).unwrap();
        }
    }
    assert_eq!(a.selection, b.selection);
    a.apply(&c, Pick::PartMaterial { part: "gable".into(), material: "brick.red.common".into() }).unwrap();
    assert!(!a.selection.parts.contains_key("gable"), "the gable's own material isn't stored");
    // The horse: a coat picked for the coat part is the record's material;
    // the mane follows it again, in its points tone.
    let mut h = on_asset(&c, HORSE);
    h.apply(&c, Pick::PartMaterial { part: "mane".into(), material: "horse.mane.flaxen".into() }).ok();
    h.apply(&c, Pick::Part(Some("coat".into()))).unwrap();
    h.apply(&c, Pick::Material("horse.coat.palomino".into())).unwrap();
    assert_eq!(h.selection.material, "horse.coat.palomino", "{:?} {:?}", h.notice, h.selection);
    assert!(!h.selection.parts.contains_key("coat"));
}

/// Parts carry over by kind when the asset changes: the horse's tail
/// colour goes onto the cow's tail; the horse's mane has nowhere to go.
#[test]
fn part_materials_carry_over_by_kind() {
    let c = cats();
    let hair = c.materials.rows.iter().find(|m| m.ready() && m.recipe == "hair" && m.belongs_on.is_empty()).map(|m| m.id.clone());
    let Some(hair) = hair else { return };
    let mut s = on_asset(&c, HORSE);
    s.apply(&c, Pick::PartMaterial { part: "tail".into(), material: hair.clone() }).unwrap();
    s.apply(&c, Pick::PartMaterial { part: "mane".into(), material: hair.clone() }).unwrap();
    assert_eq!(s.selection.parts.len(), 2, "{:?}", s.selection.parts);
    s.apply(&c, Pick::Asset(COW.into())).unwrap();
    assert_eq!(s.selection.parts.get("tail"), Some(&hair));
    assert!(!s.selection.parts.contains_key("mane"));
}

/// Presets round-trip per-part materials: saved, stored as JSON, restored
/// and opened again, the parts are the same.
#[test]
fn presets_round_trip_part_materials() {
    let c = cats();
    let mut s = on_asset(&c, ROOF);
    s.apply(&c, Pick::PartMaterial { part: "gable".into(), material: "plaster.white".into() }).unwrap();
    s.apply(&c, Pick::PartMaterial { part: "fascia".into(), material: "wood.plank.painted".into() }).unwrap();
    s.save("my roof");
    let json = s.to_json().unwrap();
    let back = StudioState::from_json(&json, &c).unwrap();
    assert_eq!(back.selection, s.selection);
    assert_eq!(back.saved, vec![Saved { name: "my roof".into(), selection: s.selection.clone(), settings: Settings::default() }]);
    assert_eq!(back.saved[0].selection.parts.len(), 2);
    assert!(back.selection.key().contains("gable=plaster.white"));
    // A part material the catalogue no longer allows is dropped on load,
    // the rest of the record kept.
    let bad = json.replace("plaster.white", "horse.coat.bay");
    let back = StudioState::from_json(&bad, &c).unwrap();
    assert_eq!(back.selection.asset, ROOF);
    assert!(!back.selection.parts.contains_key("gable"));
    assert!(back.selection.parts.contains_key("fascia"));
}
