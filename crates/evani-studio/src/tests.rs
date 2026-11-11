//! The contracts' tests: the real catalogues, the owner's 8 acceptance
//! scenarios at state level, click-order independence, coverage, keys.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;

use crate::assets::{AssetStatus, Tone};
use crate::check::{coverage, foreign_pairs, gaps, problems};
use crate::compat::{Compat, compat, place};
use crate::geo::{GeometryGenerator, Passthrough, SourceMesh};
use crate::hash::fnv1a;
use crate::resolve::{BoundBy, resolve};
use crate::state::{Because, Notice, Pick, Selection, StudioState, Tweak};
use crate::styles::{Geometry, GeometryKind, Pass, control_param};
use crate::{AssetCatalog, Availability, Catalogs, MaterialIndex, StyleCatalog};

const HORSE: &str = "animal.horse";
const COW: &str = "animal.cow";
const BAY: &str = "horse.coat.bay";
const VOXEL: &str = "WK08";
const TOON: &str = "WK01";

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn read(p: &str) -> String {
    std::fs::read_to_string(root().join(p)).unwrap()
}

fn cats() -> Catalogs {
    Catalogs::load(root()).unwrap()
}

/// The catalogues with `id` marked ready (its source model in place), as
/// it will be once its package lands.
fn with_ready(mut c: Catalogs, ids: &[&str]) -> Catalogs {
    for a in c.assets.assets.iter_mut().filter(|a| ids.contains(&a.id.as_str())) {
        a.status = AssetStatus::Ready;
    }
    c
}

fn fresh(c: &Catalogs) -> StudioState {
    StudioState::new(c).unwrap()
}

fn picks(c: &Catalogs, s: &mut StudioState, list: &[Pick]) {
    for p in list {
        s.apply(c, p.clone()).unwrap();
    }
}

fn mat(id: &str) -> Pick {
    Pick::Material(id.into())
}
fn asset(id: &str) -> Pick {
    Pick::Asset(id.into())
}
fn style(id: &str) -> Pick {
    Pick::Style(id.into())
}

#[test]
fn fnv_matches_the_engines_content_hash() {
    assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
    assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
}

#[test]
fn the_shipped_catalogues_load_and_cross_check_clean() {
    let c = cats();
    assert!(problems(&c).is_empty(), "{:?}", problems(&c));
    assert_eq!(c.styles.styles.len(), 24, "every existing style has a studio definition");
    assert_eq!(c.assets.assets.len(), 8, "the proof set B0");
    for id in [HORSE, COW, "animal.dog", "building.wall", "building.roof", "land.patch", "shape.sphere", "shape.cube"] {
        assert!(c.assets.asset(id).is_some(), "{id}");
    }
    // Test shapes are ordinary rows of a swatch class; the studio opens on a
    // real subject.
    assert_eq!(c.assets.default_asset, HORSE);
    for id in ["shape.sphere", "shape.cube"] {
        let a = c.assets.asset(id).unwrap();
        assert!(c.assets.class_of(a).unwrap().swatch);
    }
    assert!(!c.assets.class(&c.assets.asset(HORSE).unwrap().class).unwrap().swatch);
}

#[test]
fn a_test_shape_can_never_be_the_opening_asset() {
    let json = read(crate::ASSETS_PATH).replacen("\"default_asset\": \"animal.horse\"", "\"default_asset\": \"shape.sphere\"", 1);
    let e = AssetCatalog::parse(&json).unwrap_err();
    assert!(e.contains("not a test shape"), "{e}");
}

#[test]
fn a_bad_asset_catalogue_is_refused() {
    let base = read(crate::ASSETS_PATH);
    let cases = [
        (base.replacen("\"id\": \"shape.cube\", \"name\"", "\"id\": \"shape.sphere\", \"name\"", 1), "unique"),
        (base.replacen("{\"id\": \"ear\", \"follows\": \"coat\"", "{\"id\": \"ear\", \"follows\": \"leg\"", 1), "follows"),
        (base.replacen("\"recipe:keratin\"]", "\"keratin\"]", 1), "isn't class, generic"),
        (base.replacen("\"source\": {\"kind\": \"code\", \"shape\": \"wall\"}", "\"source\": {\"kind\": \"code\"}", 1), "source"),
        (base.replacen("\"version\": 1", "\"version\": 2", 1), "version"),
        (base.replacen("\"fill\": 0.8}", "\"fill\": 0.8, \"zoom\": 2}", 1), "zoom"),
    ];
    for (json, want) in cases {
        let e = AssetCatalog::parse(&json).unwrap_err();
        assert!(e.contains(want), "{want}: {e}");
    }
    // An unknown class.
    let mut c = AssetCatalog::parse(&base).unwrap();
    c.assets.iter_mut().find(|a| a.id == "shape.cube").unwrap().class = "shape.odd".into();
    let e = AssetCatalog::parse(&serde_json::to_string(&c).unwrap()).unwrap_err();
    assert!(e.contains("isn't in `classes`"), "{e}");
    // More regions than material slots.
    let mut c = AssetCatalog::parse(&base).unwrap();
    let r = c.assets[0].regions[0].clone();
    for i in 0..20 {
        c.assets[0].regions.push(crate::assets::Region { id: format!("extra{i}"), ..r.clone() });
    }
    let e = AssetCatalog::parse(&serde_json::to_string(&c).unwrap()).unwrap_err();
    assert!(e.contains("1 to 16 regions"), "{e}");
}

#[test]
fn belongs_on_has_one_format() {
    use crate::materials::{class_matches, parse_belongs_on};
    assert_eq!(parse_belongs_on(None).unwrap(), Vec::<String>::new());
    assert_eq!(parse_belongs_on(Some(&Value::from("quadruped.horse"))).unwrap(), ["quadruped.horse"]);
    assert_eq!(parse_belongs_on(Some(&serde_json::json!(["quadruped.horse", "quadruped.*"]))).unwrap().len(), 2);
    for bad in [serde_json::json!("horse"), serde_json::json!([]), serde_json::json!(3), serde_json::json!("Quadruped.Horse"), serde_json::json!("*.horse")] {
        assert!(parse_belongs_on(Some(&bad)).is_err(), "{bad}");
    }
    assert!(class_matches("quadruped.*", "quadruped.cow"));
    assert!(!class_matches("quadruped.*", "building.wall"));
    assert!(!class_matches("quadruped.horse", "quadruped.cow"));
    // A material naming no real class is a catalogue problem, with a fix.
    let mut c = cats();
    let i = c.materials.rows.iter().position(|m| m.id == BAY).unwrap();
    c.materials.rows[i].belongs_on = vec!["quadruped.unicorn".into()];
    let p = problems(&c);
    assert!(p.iter().any(|p| p.what.contains("quadruped.unicorn") && p.fix.contains("assets.json")), "{p:?}");
    // `points_on` names `tone: points` regions of the row's asset: the
    // palomino's mane and tail pass, a coat region or a made-up one doesn't.
    let mut c = cats();
    let i = c.materials.rows.iter().position(|m| m.id == "horse.coat.palomino").unwrap();
    assert_eq!(c.materials.rows[i].points_on, ["mane", "tail"]);
    assert!(problems(&c).is_empty(), "{:?}", problems(&c));
    c.materials.rows[i].points_on = vec!["mane".into(), "coat".into(), "wing".into()];
    let p = problems(&c);
    for r in ["`coat`", "`wing`"] {
        assert!(p.iter().any(|p| p.what.contains("points_on") && p.what.contains(r)), "{r}: {p:?}");
    }
    assert!(!p.iter().any(|p| p.what.contains("`mane`")), "{p:?}");
}

#[test]
fn materials_index_reads_belongs_on_and_colours() {
    let m = MaterialIndex::parse(&read(crate::MATERIALS_PATH)).unwrap();
    let bay = m.get(BAY).unwrap();
    assert_eq!(bay.belongs_on, ["quadruped.horse"]);
    assert_eq!(bay.colour, Some([0x52, 0x1f, 0x10]));
    assert!(bay.ready());
    assert!(m.get("horse.coat.dun").is_some_and(|d| !d.ready()));
    let twice = read(crate::MATERIALS_PATH).replacen("\"id\":\"horse.coat.dark_bay\"", "\"id\":\"horse.coat.bay\"", 1);
    assert!(MaterialIndex::parse(&twice).unwrap_err().contains("twice"));
}

/// The style split covers every style and agrees with the legacy `render`
/// numbers wherever both describe the shape (Pixel and Pixel HD drop the
/// legacy voxel stand-in for a screen pixel pass, as the research says).
#[test]
fn every_style_is_split_and_agrees_with_its_render_block() {
    let c = cats();
    let raw: Value = serde_json::from_str(&read(crate::STYLES_PATH)).unwrap();
    let render = |id: &str, k: &str| -> f64 {
        let s = raw["styles"].as_array().unwrap().iter().find(|s| s["id"] == id).unwrap();
        s["render"].get(k).and_then(Value::as_f64).unwrap_or(0.0)
    };
    let kind = |id: &str| c.styles.style(id).unwrap().geometry.kind();
    let pass = |id: &str| c.styles.style(id).unwrap().pass.name();
    // ARCHITECTURE.md section 2: geometry-led, pass-led, surface-led.
    assert_eq!(kind(VOXEL), GeometryKind::Voxel);
    for id in ["WK06", "WK11"] {
        assert_eq!(kind(id), GeometryKind::Lowpoly, "{id}");
    }
    assert_eq!(kind("WK18"), GeometryKind::Papercraft);
    assert_eq!(kind("WK19"), GeometryKind::Ridges);
    assert_eq!(kind("WK20"), GeometryKind::Lumpy);
    for id in ["WK12", "WK13"] {
        assert_eq!(kind(id), GeometryKind::Proportion, "{id}");
    }
    for id in ["WK09", "WK10"] {
        assert_eq!((kind(id), pass(id)), (GeometryKind::None, "pixel"), "{id}");
        assert!(render(id, "voxel") > 0.0, "{id}: the legacy stand-in this replaces");
    }
    for id in ["WK13", "WK14", "WK15", "WK16"] {
        assert_eq!(pass(id), "paper", "{id}");
    }
    assert_eq!(pass("WK07"), "tiltshift");
    assert_eq!(pass("WK22"), "silhouette");
    for id in ["WK01", "WK02", "WK03", "WK04", "WK05", "WK17", "WK21", "WK23", "WK24"] {
        assert_eq!((kind(id), pass(id)), (GeometryKind::None, "none"), "{id}: surface-led");
    }
    // Shape numbers agree with the legacy block.
    let close = |a: f32, b: f64| (f64::from(a) - b).abs() < 1e-6;
    for s in &c.styles.styles {
        match s.geometry {
            Geometry::Voxel { cell, .. } => assert!(close(cell, render(&s.id, "voxel")), "{}", s.id),
            Geometry::Lowpoly { cluster, .. } | Geometry::Papercraft { cluster, .. } => assert!(close(cluster, render(&s.id, "cluster")), "{}", s.id),
            Geometry::Lumpy { amount } => assert!(close(amount, render(&s.id, "lumpy")), "{}", s.id),
            Geometry::Ridges { amount } => assert!(close(amount, render(&s.id, "ridges")), "{}", s.id),
            Geometry::None | Geometry::Proportion { .. } => {}
        }
        // The surface is the render block's.
        assert_eq!(f64::from(s.surface.bands), render(&s.id, "bands"), "{}", s.id);
        // Every listed control does something here.
        for ctl in &s.controls {
            assert!(s.control_default(ctl).is_some(), "{} {ctl}", s.id);
        }
    }
    assert_eq!(control_param(VOXEL, "geometry.cell"), "kit.material_view.studio.wk08.geometry.cell");
}

#[test]
fn a_style_without_a_studio_block_is_refused() {
    let json = read(crate::STYLES_PATH);
    let at = json.find("\"studio\"").unwrap();
    let end = json[at..].find("]},").unwrap() + at + 3;
    let e = StyleCatalog::parse(&format!("{}{}", &json[..at], &json[end..])).unwrap_err();
    assert!(e.contains("no `studio` block"), "{e}");
    let bad = json.replacen("\"geometry.cell\"]", "\"geometry.zoom\"]", 1);
    assert!(StyleCatalog::parse(&bad).unwrap_err().contains("geometry.zoom"));
    let wrong = json.replacen("\"controls\": [\"geometry.cell\"]", "\"controls\": [\"geometry.detail\"]", 1);
    assert!(StyleCatalog::parse(&wrong).unwrap_err().contains("does nothing"));
}

/// Tests 1, 2 and 3: Voxel then Horse, Horse then Voxel, bay then Voxel:
/// the same voxel horse in its bay coat, whatever the order. Every order of
/// the three picks, from two different starting points, ends on the same
/// record, the same bindings and the same keys.
#[test]
fn click_order_never_matters() {
    let c = with_ready(cats(), &[HORSE]);
    let three = [style(VOXEL), asset(HORSE), mat(BAY)];
    let orders: [[usize; 3]; 6] = [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]];
    // Start A: a brick wall shown in Cel (nothing chosen on purpose).
    // Start B: a cow chosen on purpose (a horse coat is foreign on it).
    let starts: [Vec<Pick>; 2] = [vec![mat("brick.red.common"), style("WK03")], vec![asset(COW), style("WK17")]];
    let mut seen = None;
    for start in &starts {
        for o in orders {
            let mut s = fresh(&c);
            picks(&c, &mut s, start);
            for i in o {
                s.apply(&c, three[i].clone()).unwrap();
            }
            let r = s.resolve(&c).unwrap();
            let got = (s.selection.clone(), r.bindings.clone(), r.geometry_key, r.variant_key, r.pass_key);
            assert_eq!(got.0, Selection::new(HORSE, VOXEL, BAY), "{start:?} {o:?}");
            match &seen {
                None => seen = Some(got),
                Some(first) => assert_eq!(&got, first, "{start:?} {o:?}"),
            }
            assert!(matches!(r.geometry, Geometry::Voxel { keep_thin: true, .. }));
            assert_eq!(r.availability, Availability::Generate(r.geometry_key));
        }
    }
}

/// Test 3's colouring contract: the coat lands on the coat, legs, mane and
/// tail follow it in its points colour, hooves keep horn, eyes keep the
/// model's colour.
#[test]
fn a_bay_horse_binds_every_region() {
    let c = cats();
    let mut s = fresh(&c);
    picks(&c, &mut s, &[mat(BAY), style(VOXEL)]);
    let r = s.resolve(&c).unwrap();
    let b = |id: &str| r.bindings.iter().find(|b| b.region == id).unwrap().clone();
    assert_eq!((b("coat").material.as_deref(), b("coat").from), (Some(BAY), BoundBy::Selected));
    for id in ["leg", "mane", "tail"] {
        assert_eq!((b(id).material.as_deref(), b(id).tone, b(id).from), (Some(BAY), Tone::Points, BoundBy::Follows("coat".into())), "{id}");
    }
    assert_eq!((b("ear").material.as_deref(), b("ear").tone), (Some(BAY), Tone::Base));
    assert_eq!(b("hoof").material.as_deref(), Some("horse.hoof.dark"));
    assert_eq!((b("eye").material, b("eye").from), (None, BoundBy::Keep));
    assert!(r.bindings.len() <= crate::MAX_SLOTS);
    // The horse's source is in the app (WP2): the voxel shape is generated
    // from it, never another object.
    assert!(matches!(r.availability, Availability::Generate(_)), "{:?}", r.availability);
    assert_eq!(r.selection.asset, HORSE);
}

/// Test 4: Voxel to Toon keeps the horse and its coat; only the style's
/// stages change.
#[test]
fn a_style_change_keeps_asset_and_material() {
    let c = with_ready(cats(), &[HORSE]);
    let mut s = fresh(&c);
    picks(&c, &mut s, &[asset(HORSE), mat(BAY), style(VOXEL)]);
    let voxel = s.resolve(&c).unwrap();
    s.apply(&c, style(TOON)).unwrap();
    let toon = s.resolve(&c).unwrap();
    assert_eq!((toon.selection.asset.as_str(), toon.selection.material.as_str()), (HORSE, BAY));
    assert_eq!(toon.geometry, Geometry::None);
    assert_eq!(toon.availability, Availability::Ready);
    assert_ne!(toon.geometry_key, voxel.geometry_key);
    assert_ne!(toon.variant_key, voxel.variant_key);
    assert_eq!(toon.bindings, voxel.bindings);
    assert_eq!(s.notice, Notice::None);
}

/// Test 5: Horse to Cow keeps the style and picks a sensible cow coat (the
/// closest in colour to the bay), saying so; back to the horse, the bay
/// returns.
#[test]
fn an_asset_change_keeps_the_style_and_finds_a_material() {
    let c = cats();
    let mut s = fresh(&c);
    picks(&c, &mut s, &[asset(HORSE), mat(BAY), style(VOXEL), asset(COW)]);
    assert_eq!(s.selection.asset, COW);
    assert_eq!(s.selection.style, VOXEL);
    assert_eq!(s.selection.material, "cattle.coat.black", "the cow coat closest to a bay");
    assert_eq!(s.notice, Notice::Substituted { from: BAY.into(), to: "cattle.coat.black".into(), because: Because::Closest });
    let cow = c.assets.asset(COW).unwrap();
    assert_eq!(compat(&c, cow, c.materials.get(&s.selection.material).unwrap()), Compat::Native);
    s.apply(&c, asset(HORSE)).unwrap();
    assert_eq!(s.selection.material, BAY);
    assert_eq!(s.notice, Notice::Substituted { from: "cattle.coat.black".into(), to: BAY.into(), because: Because::Remembered });
    // A generic material rides along unchanged.
    picks(&c, &mut s, &[mat("brick.red.common"), asset(COW)]);
    assert_eq!((s.selection.material.as_str(), &s.notice), ("brick.red.common", &Notice::None));
}

/// Test 6: a material change keeps the asset and the shape: the geometry
/// key (the expensive part) stays, only the variant changes.
#[test]
fn a_material_change_keeps_the_shape() {
    let c = with_ready(cats(), &[HORSE]);
    let mut s = fresh(&c);
    picks(&c, &mut s, &[asset(HORSE), style(VOXEL), mat(BAY)]);
    let before = s.resolve(&c).unwrap();
    s.apply(&c, mat("horse.coat.grey_dapple")).unwrap();
    let after = s.resolve(&c).unwrap();
    assert_eq!((after.selection.asset.as_str(), after.selection.style.as_str()), (HORSE, VOXEL));
    assert_eq!(after.geometry_key, before.geometry_key);
    assert_eq!(after.pass_key, before.pass_key);
    assert_ne!(after.variant_key, before.variant_key);
}

/// Test 7: the light and the quality change nothing the object is built
/// from.
#[test]
fn lighting_and_quality_never_rebuild() {
    let c = with_ready(cats(), &[HORSE]);
    let mut s = fresh(&c);
    picks(&c, &mut s, &[asset(HORSE), style(VOXEL), mat(BAY)]);
    let before = (s.selection.clone(), s.resolve(&c).unwrap());
    for l in ["daylight", "sunset", "night", "studio"] {
        s.set_light(l);
        s.set_quality("ultra");
        let r = s.resolve(&c).unwrap();
        assert_eq!(s.selection, before.0);
        assert_eq!((r.geometry_key, r.variant_key, r.pass_key), (before.1.geometry_key, before.1.variant_key, before.1.pass_key), "{l}");
    }
}

/// Test 8: returning to a combination restores its settings; the style's
/// controls hold per style.
#[test]
fn combinations_keep_their_settings() {
    let c = with_ready(cats(), &[HORSE]);
    let mut s = fresh(&c);
    picks(&c, &mut s, &[asset(HORSE), style(VOXEL), mat(BAY)]);
    s.settings_mut().edits.insert("kit.material_view.cat.n.roughness".into(), Tweak::Num(0.9));
    s.set_control(&c, "geometry.cell", 0.5).unwrap();
    let voxel = s.resolve(&c).unwrap();
    s.apply(&c, style(TOON)).unwrap();
    assert!(s.settings().is_none(), "a new combination starts clean");
    s.settings_mut().edits.insert("kit.material_view.cat.n.roughness".into(), Tweak::Num(0.2));
    s.set_control(&c, "surface.bands", 5.0).unwrap();
    s.apply(&c, style(VOXEL)).unwrap();
    assert_eq!(s.settings().unwrap().edits["kit.material_view.cat.n.roughness"], Tweak::Num(0.9));
    assert_eq!(s.resolve(&c).unwrap().variant_key, voxel.variant_key);
    assert!(matches!(s.resolve(&c).unwrap().geometry, Geometry::Voxel { cell, .. } if (cell - 0.06).abs() < 1e-6));
    // Out of range is clamped; a control the style lacks is refused.
    s.set_control(&c, "geometry.cell", 99.0).unwrap();
    assert!((s.style_controls()["geometry.cell"] - 2.0).abs() < 1e-6);
    assert!(s.set_control(&c, "surface.bands", 3.0).is_err());
    // The state survives the page's storage.
    s.save("Small voxel bay");
    let back = StudioState::from_json(&s.to_json().unwrap(), &c).unwrap();
    assert_eq!((back.selection.clone(), back.memory.clone(), back.controls.clone(), back.saved.clone()), (s.selection.clone(), s.memory.clone(), s.controls.clone(), s.saved.clone()));
}

/// The auto-match rule: a deliberate asset is never replaced.
#[test]
fn auto_match_never_replaces_a_deliberate_asset() {
    let c = cats();
    // Nothing chosen on purpose: each material previews where it belongs.
    for (m, a) in [("brick.red.common", "building.wall"), ("roof.tile.terracotta", "building.roof"), ("ground.grass.meadow", "land.patch"), ("dog.coat.tan", "animal.dog"), (BAY, HORSE)] {
        let mut s = fresh(&c);
        s.apply(&c, asset("shape.sphere")).unwrap();
        s.explicit.asset = false;
        s.apply(&c, mat(m)).unwrap();
        assert_eq!(s.selection.asset, a, "{m}");
        assert_eq!(s.notice, Notice::ShownOn { material: m.into(), asset: a.into() });
        assert!(!s.explicit.asset);
    }
    // A cow chosen on purpose: a horse coat isn't applied, the cow stays,
    // and the notice offers the horse; one click shows it there.
    let mut s = fresh(&c);
    s.apply(&c, asset(COW)).unwrap();
    let before = s.selection.clone();
    s.apply(&c, mat(BAY)).unwrap();
    assert_eq!(s.selection, before);
    assert_eq!(s.notice, Notice::NotApplied { material: BAY.into(), made_for: "quadruped.horse".into(), offer: HORSE.into() });
    assert_eq!(s.pending.as_deref(), Some(BAY));
    s.apply(&c, Pick::ShowOn(HORSE.into())).unwrap();
    assert_eq!((s.selection.asset.as_str(), s.selection.material.as_str()), (HORSE, BAY));
    assert!(s.pending.is_none() && s.explicit.asset);
    // A horse chosen on purpose takes a generic material (the mockup's brick
    // horse); a test shape takes any material, a horse coat too.
    picks(&c, &mut s, &[mat("brick.red.common")]);
    assert_eq!((s.selection.asset.as_str(), s.selection.material.as_str()), (HORSE, "brick.red.common"));
    picks(&c, &mut s, &[asset("shape.cube"), mat(BAY)]);
    assert_eq!((s.selection.asset.as_str(), s.selection.material.as_str()), ("shape.cube", BAY));
    // A planned material changes nothing and says so; an unknown id is an
    // error and changes nothing.
    let before = s.clone();
    s.apply(&c, mat("horse.coat.dun")).unwrap();
    assert_eq!(s.selection, before.selection);
    assert_eq!(s.notice, Notice::NotBuilt { material: "horse.coat.dun".into() });
    assert!(s.apply(&c, mat("no.such.material")).is_err());
    assert!(s.apply(&c, asset("animal.unicorn")).is_err());
    assert!(s.apply(&c, style("WK99")).is_err());
    assert_eq!(s.selection, before.selection);
    // Foreign pairs exist both ways.
    let f = foreign_pairs(&c);
    assert!(f.contains(&(COW.into(), BAY.into())) && f.contains(&(HORSE.into(), "cattle.coat.black".into())));
}

/// A part lands on the region its id names when several regions take its
/// recipe: the horse's muzzle skin on the muzzle (the coat also takes hide),
/// a hoof material on the cow's hoof (its horn also takes keratin), a horn
/// on the horn, a mane on the mane. Without a named region the asset's order
/// decides: a coat on the coat, a brick on the wall's face (its cap also
/// takes masonry), slate on the roof's tiles.
#[test]
fn a_part_lands_on_the_region_it_names() {
    let c = cats();
    let on = |a: &str, m: &str| {
        let a = c.assets.asset(a).unwrap();
        place(&c, a, c.materials.get(m).unwrap()).map(|i| a.regions[i].id.clone())
    };
    for (a, m, region) in [
        (HORSE, "horse.muzzle.pink", "muzzle"),
        (HORSE, "horse.muzzle.dark", "muzzle"),
        (HORSE, "horse.hoof.pale", "hoof"),
        (HORSE, "horse.mane.black", "mane"),
        (COW, "horse.hoof.pale", "hoof"),
        (COW, "horse.muzzle.pink", "muzzle"),
        (COW, "horn.cattle", "horn"),
        (HORSE, BAY, "coat"),
        (COW, "cattle.coat.black", "coat"),
        ("animal.dog", "dog.coat.tan", "coat"),
        ("building.wall", "brick.red.common", "face"),
        ("building.wall", "stonework.rubble", "face"),
        ("building.roof", "roof.tile.slate", "tiles"),
        (HORSE, "brick.red.common", "coat"),
    ] {
        assert_eq!(on(a, m).as_deref(), Some(region), "{m} on {a}");
    }
}

/// Coverage: every asset in every style resolves to itself, in its default
/// material, with one binding per region; no cell is "unavailable".
#[test]
fn every_asset_resolves_in_every_style() {
    for c in [cats(), with_ready(cats(), &[HORSE, COW, "animal.dog", "building.wall", "building.roof", "land.patch"])] {
        let cells = coverage(&c).unwrap_or_else(|p| panic!("{p:?}"));
        assert_eq!(cells.len(), c.assets.assets.len() * c.styles.styles.len());
        for cell in &cells {
            let a = c.assets.asset(&cell.asset).unwrap();
            let s = c.styles.style(&cell.style).unwrap();
            let want = match (a.status, &s.geometry) {
                (AssetStatus::Ready, Geometry::None) => "ready",
                (AssetStatus::Ready, _) => "generate",
                _ => "not built",
            };
            let got = match cell.availability {
                Availability::Ready => "ready",
                Availability::Generate(_) => "generate",
                Availability::NotBuilt => "not built",
            };
            assert_eq!(got, want, "{} x {}", cell.asset, cell.style);
        }
    }
}

/// resolve() never substitutes an object: for every asset and every ready
/// material, it either draws that asset with that material or refuses.
#[test]
fn resolve_draws_the_record_or_refuses() {
    let c = cats();
    let none = BTreeMap::new();
    for a in &c.assets.assets {
        for m in c.materials.rows.iter().filter(|m| m.ready()) {
            let sel = Selection::new(&a.id, VOXEL, &m.id);
            match resolve(&c, &sel, &none, None) {
                Ok(r) => {
                    assert!(compat(&c, a, m).applies());
                    assert_eq!(r.selection, sel);
                    let at = place(&c, a, m).unwrap();
                    assert_eq!(r.bindings[at].material.as_deref(), Some(m.id.as_str()));
                }
                Err(_) => assert!(!compat(&c, a, m).applies(), "{} on {}", m.id, a.id),
            }
        }
    }
}

/// Cache keys: every input that shapes the object moves the geometry key;
/// nothing else does.
#[test]
fn cache_keys_cover_their_inputs() {
    let mut c = with_ready(cats(), &[HORSE, COW]);
    let sel = Selection::new(HORSE, VOXEL, BAY);
    let none = BTreeMap::new();
    let base = resolve(&c, &sel, &none, None).unwrap();
    let key = |c: &Catalogs, sel: &Selection, ctl: &BTreeMap<String, f32>| resolve(c, sel, ctl, None).unwrap().geometry_key;
    // The asset, the style's geometry, a geometry control, the source hash.
    assert_ne!(key(&c, &Selection { asset: COW.into(), material: "cattle.coat.black".into(), ..sel.clone() }, &none), base.geometry_key);
    assert_ne!(key(&c, &Selection { style: "WK06".into(), ..sel.clone() }, &none), base.geometry_key);
    let finer: BTreeMap<String, f32> = [("geometry.cell".to_string(), 0.5)].into();
    assert_ne!(key(&c, &sel, &finer), base.geometry_key);
    c.source_hashes.insert(HORSE.into(), 0x1234);
    assert_ne!(key(&c, &sel, &none), base.geometry_key);
    c.source_hashes.clear();
    // Not the material, nor surface or pass controls.
    let grey = Selection { material: "horse.coat.grey_dapple".into(), ..sel.clone() };
    assert_eq!(key(&c, &grey, &none), base.geometry_key);
    let pixel = Selection { style: "WK09".into(), ..sel.clone() };
    let big: BTreeMap<String, f32> = [("pass.px".to_string(), 8.0)].into();
    let p0 = resolve(&c, &pixel, &none, None).unwrap();
    let p1 = resolve(&c, &pixel, &big, None).unwrap();
    assert_eq!((p0.geometry_key, p0.variant_key), (p1.geometry_key, p1.variant_key), "a pass control never rebuilds");
    assert_ne!(p0.pass_key, p1.pass_key);
    assert!(matches!(p1.pass, Pass::Pixel { px, .. } if (px - 8.0).abs() < 1e-6));
    // Edits move the variant, not the geometry; keys print as 16 hex digits.
    let mut st = crate::state::Settings::default();
    st.edits.insert("x".into(), Tweak::Flag(true));
    let edited = resolve(&c, &sel, &none, Some(&st)).unwrap();
    assert_eq!(edited.geometry_key, base.geometry_key);
    assert_ne!(edited.variant_key, base.variant_key);
    assert_eq!(base.geometry_key.to_string().len(), 16);
}

/// Until the generators land (WP3), the voxel and other shape styles are
/// listed as gaps; geometry `none` never is.
#[test]
fn gaps_list_styles_without_a_generator() {
    let c = cats();
    let g = gaps(&c);
    // WP3 built voxel, low-poly, papercraft, lumps and ridges; only the
    // proportion styles still wait for a generator.
    assert!(!g.is_empty());
    assert!(g.iter().all(|(_, k, _)| *k == GeometryKind::Proportion), "{g:?}");
    assert!(!g.iter().any(|(s, _, _)| s == VOXEL));
}

#[test]
fn passthrough_cuts_the_source_by_region() {
    let c = cats();
    let horse = c.assets.asset(HORSE).unwrap();
    // Two triangles, two regions (coat, hoof).
    let src = SourceMesh {
        positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]],
        normals: vec![[0.0, 0.0, 1.0]; 4],
        indices: vec![0, 1, 2, 0, 2, 3],
        regions: vec![0, 0, 0, 5],
    };
    let out = Passthrough.generate(&src, horse, &Geometry::None).unwrap();
    assert_eq!(out.parts.len(), 1, "a triangle takes its first vertex's region");
    assert_eq!(out.triangles(), 2);
    let two = SourceMesh { regions: vec![0, 0, 0, 0], indices: vec![0, 1, 2, 3, 0, 2], ..src.clone() };
    assert_eq!(Passthrough.generate(&two, horse, &Geometry::None).unwrap().parts.len(), 1);
    let split = SourceMesh { regions: vec![0, 0, 0, 5], indices: vec![0, 1, 2, 3, 0, 2], ..src.clone() };
    let parts = Passthrough.generate(&split, horse, &Geometry::None).unwrap().parts;
    assert_eq!(parts.iter().map(|p| p.region).collect::<Vec<_>>(), [0, 5]);
    // Malformed sources and other geometry kinds are refused.
    let bad = SourceMesh { regions: vec![0, 0, 0, 99], ..src.clone() };
    assert!(Passthrough.generate(&bad, horse, &Geometry::None).is_err());
    assert!(Passthrough.generate(&src, horse, &Geometry::Lumpy { amount: 0.1 }).is_err());
}
