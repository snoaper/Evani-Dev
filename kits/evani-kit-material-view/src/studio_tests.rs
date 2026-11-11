//! The owner's 8 acceptance tests (docs/studio/SPEC.md section 7) through the
//! viewer's real systems, headless and without a GPU: the page's commands go
//! into `Studio`, the frame runs, and the test reads what the scene will
//! draw (`Studio::current`), what was rebuilt (`Studio::builds`), which
//! catalogue row is open with which switches, the camera and the light. The
//! 3D proof of the same scenarios is WP9's harness.

use std::collections::BTreeMap;
use std::rc::Rc;

use evani_core::App;
use evani_core::params::{ParamRegistry, ParamValue};
use evani_core::tiers::Tiers;
use evani_studio::assets::Tone;
use evani_studio::resolve::{Availability, BoundBy};
use evani_studio::styles::control_param;
use evani_studio::{AssetStatus, Geometry};

use super::params::{StudioParams, VARIATIONS, VARIATIONS_MARKED};
use super::*;
use crate::MaterialViewKitPlugin;
use crate::catalogue::{self, CatalogueView, LibraryData, OPEN};
use crate::viewer::{Command, Viewer};

const DT: f64 = 1000.0 / 60.0;
const HORSE: &str = "animal.horse";
const COW: &str = "animal.cow";
const BAY: &str = "horse.coat.bay";
const GREY: &str = "horse.coat.grey_dapple";
const BRICK: &str = "brick.red.common";
const VOXEL: &str = "WK08";
const TOON: &str = "WK01";
/// A number of the fur recipe (the bay's own value is 0.8).
const SHEEN: &str = "kit.material_view.mat.sheen";

/// The viewer with the catalogue and the studio, the studio on screen, and
/// every B0 asset's source in place (as it will be once its package lands:
/// the logic is the same either way, but "ready" is what draws).
fn app() -> App {
    let mut a = App::new("t").with_module(evani_render::RenderModule::new()).with_kit(MaterialViewKitPlugin);
    catalogue::install(&mut a.context("material-view")).unwrap();
    install(&mut a.context("material-view")).unwrap();
    let mut c = (*a.world.res::<StudioData>().0).clone();
    for x in &mut c.assets.assets {
        x.status = AssetStatus::Ready;
    }
    a.world.res_mut::<StudioData>().0 = Rc::new(c);
    a.world.res_mut::<Studio>().active = true;
    a.run_headless(3, DT);
    a
}

/// Push the commands and run the frames the bridge needs (the pick, the
/// catalogue opening the row, the edits laid over it).
fn push(a: &mut App, cmds: &[StudioCommand]) {
    for c in cmds {
        a.world.res_mut::<Studio>().push(c.clone());
    }
    a.run_headless(3, DT);
}

fn pick(p: Pick) -> StudioCommand {
    StudioCommand::Pick(p)
}
fn style(id: &str) -> StudioCommand {
    pick(Pick::Style(id.into()))
}
fn asset(id: &str) -> StudioCommand {
    pick(Pick::Asset(id.into()))
}
fn mat(id: &str) -> StudioCommand {
    pick(Pick::Material(id.into()))
}

fn current(a: &App) -> Resolved {
    a.world.res::<Studio>().current.clone().unwrap()
}
fn builds(a: &App) -> Builds {
    a.world.res::<Studio>().builds
}
fn sel(a: &App) -> Selection {
    a.world.res::<Studio>().state.selection.clone()
}
fn no_error(a: &App) {
    assert_eq!(a.world.res::<Studio>().error, None);
}

/// The catalogue row open now (what the recipe stage builds the block from).
fn open_row(a: &App) -> Option<String> {
    let lib = a.world.res::<LibraryData>().0.clone();
    a.world.res::<CatalogueView>().open.map(|i| lib.cat.rows[i].id.clone())
}
fn opened(a: &App) -> u64 {
    a.world.res::<CatalogueView>().opened
}
fn param(a: &App, name: &str) -> ParamValue {
    let reg = a.world.res::<ParamRegistry>();
    reg.id(name).and_then(|id| reg.value(id)).unwrap()
}
fn set_param(a: &mut App, name: &str, v: ParamValue) {
    a.world.res_mut::<ParamRegistry>().set_by_name(name, v).unwrap();
    a.run_headless(2, DT);
}
fn orbit(a: &App) -> (f32, f32, f32) {
    let v = a.world.res::<Viewer>();
    (v.orbit.az, v.orbit.el, v.orbit.dist)
}
/// The open row's switches (its recipe's numbers, extras and conditions)
/// and every studio param, by name: the whole state a drawn frame depends
/// on. (Other recipes' switches, registered by rows opened earlier, aren't
/// drawn.)
fn switches(a: &App) -> BTreeMap<String, String> {
    let names: Vec<String> = a.world.res::<bridge::Bridge>().switches().map(str::to_string).collect();
    let reg = a.world.res::<ParamRegistry>();
    reg.defs()
        .iter()
        .map(|d| d.name.clone())
        .filter(|n| n.starts_with("kit.material_view.studio.") || n == OPEN || names.contains(n))
        .map(|n| {
            let v = format!("{:?}", reg.id(&n).and_then(|id| reg.value(id)));
            (n, v)
        })
        .collect()
}

/// Test 1: Voxel, then Horse: a voxel horse.
#[test]
fn t1_voxel_then_horse_is_a_voxel_horse() {
    let mut a = app();
    push(&mut a, &[style(VOXEL), asset(HORSE)]);
    let r = current(&a);
    assert_eq!((r.selection.asset.as_str(), r.selection.style.as_str()), (HORSE, VOXEL));
    assert!(matches!(r.geometry, Geometry::Voxel { keep_thin: true, .. }), "real voxel geometry, not a shader on a smooth horse");
    assert_eq!(r.availability, Availability::Generate(r.geometry_key), "the voxel stage runs on the horse itself");
    assert_eq!(open_row(&a).as_deref(), Some(BAY), "the horse's coat is open in the catalogue");
    no_error(&a);
}

/// Test 2: Horse, then Voxel: the same voxel horse as test 1.
#[test]
fn t2_horse_then_voxel_is_the_same_voxel_horse() {
    let mut first = app();
    push(&mut first, &[style(VOXEL), asset(HORSE)]);
    let mut second = app();
    push(&mut second, &[asset(HORSE), style(VOXEL)]);
    let (x, y) = (current(&first), current(&second));
    assert_eq!((x.selection, x.bindings, x.geometry_key, x.variant_key, x.pass_key), (y.selection, y.bindings, y.geometry_key, y.variant_key, y.pass_key));
    assert_eq!(open_row(&first), open_row(&second));
    assert_eq!(switches(&first), switches(&second));
}

/// Test 3: Horse coat, bay, then Voxel: a voxel horse in its bay colours.
/// It starts on a brick wall (the brick previewed where it belongs), so the
/// coat has to bring the horse back and colour it.
#[test]
fn t3_bay_then_voxel_is_a_bay_voxel_horse() {
    let mut a = app();
    push(&mut a, &[mat(BRICK)]);
    assert_eq!(sel(&a).asset, "building.wall", "nothing was chosen on purpose: the brick shows on a wall");
    push(&mut a, &[mat(BAY), style(VOXEL)]);
    let r = current(&a);
    assert_eq!(r.selection, Selection::new(HORSE, VOXEL, BAY));
    let b = |id: &str| r.bindings.iter().find(|b| b.region == id).unwrap().clone();
    assert_eq!((b("coat").material.as_deref(), b("coat").from), (Some(BAY), BoundBy::Selected));
    for id in ["leg", "mane", "tail"] {
        assert_eq!((b(id).material.as_deref(), b(id).tone), (Some(BAY), Tone::Points), "{id}: the bay's black points");
    }
    assert_eq!(b("hoof").material.as_deref(), Some("horse.hoof.dark"));
    // The block the coat is drawn with is the bay's own.
    let view = a.world.res::<CatalogueView>().clone();
    assert_eq!(view.edited.as_ref().map(|r| r.id.as_str()), Some(BAY));
    assert!(view.block.is_some(), "the recipe stage built the bay's block");
    assert_eq!(param(&a, SHEEN), ParamValue::F32(0.8), "the switches are on the bay's own values");
    no_error(&a);
}

/// Tests 1 to 3 in every order, from two starting points: the same record,
/// the same keys, the same open row and the same switches every time.
#[test]
fn every_click_order_ends_on_the_same_frame() {
    let three = [style(VOXEL), asset(HORSE), mat(BAY)];
    let orders: [[usize; 3]; 6] = [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]];
    let starts: [Vec<StudioCommand>; 2] = [vec![mat(BRICK), style("WK03")], vec![asset(COW), style("WK17")]];
    let mut seen = None;
    for start in &starts {
        for o in orders {
            let mut a = app();
            push(&mut a, start);
            for i in o {
                push(&mut a, std::slice::from_ref(&three[i]));
            }
            let r = current(&a);
            let got = (r.selection, r.bindings, r.geometry_key, r.variant_key, r.pass_key, open_row(&a), switches(&a));
            match &seen {
                None => seen = Some(got),
                Some(first) => assert_eq!(&got, first, "{start:?} {o:?}"),
            }
        }
    }
}

/// Test 4: Voxel to Toon: the horse and its coat stay, the style changes.
/// The coat's row isn't reopened.
#[test]
fn t4_voxel_to_toon_keeps_the_horse_and_its_coat() {
    let mut a = app();
    push(&mut a, &[asset(HORSE), mat(BAY), style(VOXEL)]);
    let (before, built, rows) = (current(&a), builds(&a), opened(&a));
    push(&mut a, &[style(TOON)]);
    let r = current(&a);
    assert_eq!((r.selection.asset.as_str(), r.selection.material.as_str(), r.selection.style.as_str()), (HORSE, BAY, TOON));
    assert_eq!(r.geometry, Geometry::None);
    assert_ne!(r.surface, before.surface, "the style's surface changed");
    assert_eq!(r.bindings, before.bindings);
    assert_eq!((builds(&a).geometry, builds(&a).variant), (built.geometry + 1, built.variant + 1));
    assert_eq!((opened(&a), open_row(&a).as_deref()), (rows, Some(BAY)), "the coat stays open");
}

/// Test 5: Horse to Cow: a cow in the same style, in a cow coat (the one
/// closest to the bay), said in one line.
#[test]
fn t5_horse_to_cow_keeps_the_style() {
    let mut a = app();
    push(&mut a, &[asset(HORSE), mat(BAY), style(VOXEL)]);
    let built = builds(&a);
    push(&mut a, &[asset(COW)]);
    let r = current(&a);
    assert_eq!((r.selection.asset.as_str(), r.selection.style.as_str()), (COW, VOXEL));
    assert_eq!(r.selection.material, "cattle.coat.black");
    assert!(matches!(r.geometry, Geometry::Voxel { .. }));
    assert_eq!(a.world.res::<Studio>().state.notice, Notice::Substituted { from: BAY.into(), to: "cattle.coat.black".into(), because: evani_studio::state::Because::Closest });
    assert_eq!(builds(&a).geometry, built.geometry + 1, "a new asset is a new shape");
    assert_eq!(open_row(&a).as_deref(), Some("cattle.coat.black"));
    no_error(&a);
}

/// Test 6: a new material: the asset and its shape stay, only the surface
/// is rebuilt.
#[test]
fn t6_a_material_change_keeps_the_asset() {
    let mut a = app();
    push(&mut a, &[asset(HORSE), style(VOXEL), mat(BAY)]);
    let (before, built) = (current(&a), builds(&a));
    push(&mut a, &[mat(GREY)]);
    let r = current(&a);
    assert_eq!((r.selection.asset.as_str(), r.selection.style.as_str(), r.selection.material.as_str()), (HORSE, VOXEL, GREY));
    assert_eq!(r.geometry_key, before.geometry_key, "the voxel horse isn't rebuilt");
    assert_eq!(builds(&a), Builds { variant: built.variant + 1, ..built });
    assert_eq!(open_row(&a).as_deref(), Some(GREY));
}

/// Test 7: a light change changes only the light: no rebuild, no reopened
/// row, the same record. The quality tier likewise. The light key writes
/// back into the record's view.
#[test]
fn t7_a_light_change_changes_only_the_light() {
    let mut a = app();
    push(&mut a, &[asset(HORSE), style(VOXEL), mat(BAY)]);
    let (record, built, rows, before) = (sel(&a), builds(&a), opened(&a), current(&a));
    let ids = bridge::light_ids(&a.world);
    for (light, want) in [("daylight", 4), ("sunset", 2), ("night", 3), ("studio", 0)] {
        push(&mut a, &[StudioCommand::Light(light.into())]);
        assert_eq!(a.world.res::<Viewer>().light, want, "{light}");
        assert_eq!(ids[want], light);
        assert_eq!((sel(&a), builds(&a), opened(&a)), (record.clone(), built, rows), "{light}");
        let r = current(&a);
        assert_eq!((r.geometry_key, r.variant_key, r.pass_key), (before.geometry_key, before.variant_key, before.pass_key));
    }
    // The L key: the viewer moves on a preset and the record's view follows.
    a.world.res_mut::<Viewer>().push(Command::NextLight);
    a.run_headless(2, DT);
    assert_eq!(a.world.res::<Studio>().state.view.light, "overcast");
    // An unknown light is refused and changes nothing.
    push(&mut a, &[StudioCommand::Light("disco".into())]);
    assert!(a.world.res::<Studio>().error.as_deref().is_some_and(|e| e.contains("disco")));
    assert_eq!(a.world.res::<Studio>().state.view.light, "overcast");
    // Quality: the tier changes, nothing is rebuilt.
    let tier = {
        let t = a.world.res::<Tiers>();
        let active = t.active().id.clone();
        t.selectable().map(|(_, p)| p.id.clone()).find(|id| *id != active).unwrap()
    };
    push(&mut a, &[StudioCommand::Quality(tier.clone())]);
    assert_eq!(a.world.res::<Tiers>().active().id, tier);
    assert_eq!((sel(&a), builds(&a), opened(&a)), (record, built, rows));
    push(&mut a, &[StudioCommand::Quality("no-such-tier".into())]);
    assert_eq!(a.world.res::<Studio>().state.view.quality, tier, "a refused tier changes nothing");
}

/// Test 8: going back to a combination brings back its settings: the
/// switches tuned there (over the panel or evani-live), its camera and its
/// style controls; another combination keeps its own. The whole state
/// survives the page's storage.
#[test]
fn t8_combinations_keep_their_settings() {
    let mut a = app();
    push(&mut a, &[asset(HORSE), style(VOXEL), mat(BAY)]);
    set_param(&mut a, SHEEN, ParamValue::F32(0.9));
    a.world.res_mut::<Viewer>().push(Command::Orbit { daz: 40.0, del: 10.0 });
    let cell = control_param(VOXEL, "geometry.cell");
    set_param(&mut a, &cell, ParamValue::F32(0.6));
    let voxel_cam = orbit(&a);
    assert_eq!(a.world.res::<Studio>().state.settings().unwrap().edits[SHEEN], Tweak::Num(0.9));
    assert_eq!(a.world.res::<Studio>().state.controls[VOXEL]["geometry.cell"], 0.6);
    let voxel = current(&a);

    // Toon: a combination of its own, starting from the bay's own values.
    push(&mut a, &[style(TOON)]);
    assert_eq!(param(&a, SHEEN), ParamValue::F32(0.8));
    set_param(&mut a, SHEEN, ParamValue::F32(0.2));
    a.world.res_mut::<Viewer>().push(Command::Orbit { daz: -90.0, del: 0.0 });
    a.run_headless(1, DT);
    let toon_cam = orbit(&a);
    assert_ne!(toon_cam, voxel_cam);

    // Back to Voxel: its sheen, camera and cube size; the same keys.
    push(&mut a, &[style(VOXEL)]);
    assert_eq!(param(&a, SHEEN), ParamValue::F32(0.9));
    assert_eq!(orbit(&a), voxel_cam);
    assert_eq!(param(&a, &cell), ParamValue::F32(0.6));
    let back = current(&a);
    assert_eq!((back.geometry_key, back.variant_key), (voxel.geometry_key, voxel.variant_key));
    push(&mut a, &[style(TOON)]);
    assert_eq!((param(&a, SHEEN), orbit(&a)), (ParamValue::F32(0.2), toon_cam));

    // A switch moved back onto the row's own value is no edit.
    set_param(&mut a, SHEEN, ParamValue::F32(0.8));
    assert!(!a.world.res::<Studio>().state.settings().unwrap().edits.contains_key(SHEEN));

    // The page's storage: a new viewer restores the whole state.
    push(&mut a, &[style(VOXEL)]);
    let json = a.world.res_mut::<Studio>().storage_due().unwrap();
    assert_eq!(a.world.res_mut::<Studio>().storage_due(), None, "nothing new to store");
    let mut b = app();
    push(&mut b, &[StudioCommand::Restore(json)]);
    assert_eq!(sel(&b), sel(&a));
    assert_eq!((param(&b, SHEEN), param(&b, &cell)), (ParamValue::F32(0.9), ParamValue::F32(0.6)));
    assert_eq!(orbit(&b), voxel_cam);
    assert_eq!(current(&b).variant_key, voxel.variant_key);

    // Reset: back to the bay's own values; a bad stored state is refused.
    push(&mut b, &[StudioCommand::Reset]);
    assert_eq!(param(&b, SHEEN), ParamValue::F32(0.8));
    push(&mut b, &[StudioCommand::Restore("{not json".into())]);
    assert!(b.world.res::<Studio>().error.is_some());
    assert_eq!(sel(&b), sel(&a), "a refused restore changes nothing");
}

/// A saved combination opens with its settings (the presets row).
#[test]
fn a_saved_combination_opens_with_its_settings() {
    let mut a = app();
    push(&mut a, &[asset(HORSE), style(VOXEL), mat(BAY)]);
    set_param(&mut a, SHEEN, ParamValue::F32(0.95));
    push(&mut a, &[StudioCommand::Save("Shiny voxel bay".into()), asset(COW), style(TOON)]);
    push(&mut a, &[StudioCommand::Open("Shiny voxel bay".into())]);
    assert_eq!(sel(&a), Selection::new(HORSE, VOXEL, BAY));
    assert_eq!(param(&a, SHEEN), ParamValue::F32(0.95));
    push(&mut a, &[StudioCommand::Open("nothing".into())]);
    assert!(a.world.res::<Studio>().error.as_deref().is_some_and(|e| e.contains("nothing")));
}

/// Every control each style lists is a registered param over its range; a
/// param move and a `Control` command are the same edit, enter the keys
/// exactly as `resolve` says, and a control back on the style's own value
/// leaves the keys as they were.
#[test]
fn every_style_control_is_a_param() {
    let mut a = app();
    let data = a.world.res::<StudioData>().0.clone();
    let handles = a.world.res::<StudioParams>().clone();
    let mut n = 0;
    for s in &data.styles.styles {
        for id in &s.controls {
            let name = control_param(&s.id, id);
            let reg = a.world.res::<ParamRegistry>();
            let def = reg.id(&name).and_then(|i| reg.def(i)).unwrap_or_else(|| panic!("{name} isn't registered"));
            let c = evani_studio::styles::control(id).unwrap();
            assert_eq!((def.min, def.max, def.step), (c.min, c.max, Some(c.step)), "{name}");
            assert_eq!(def.default, ParamValue::F32(s.control_default(id).unwrap()), "{name}");
            n += 1;
        }
    }
    assert_eq!(n, handles.controls.len());
    assert_eq!(n, data.styles.styles.iter().map(|s| s.controls.len()).sum::<usize>());
    assert!(n > 0);

    push(&mut a, &[asset(HORSE), style(VOXEL), mat(BAY)]);
    let plain = current(&a);
    let cell = control_param(VOXEL, "geometry.cell");
    set_param(&mut a, &cell, ParamValue::F32(0.5));
    let finer = current(&a);
    let want = evani_studio::resolve(&data, &sel(&a), &[("geometry.cell".to_string(), 0.5)].into(), None).unwrap();
    assert_eq!((finer.geometry_key, finer.variant_key), (want.geometry_key, want.variant_key));
    assert_ne!(finer.geometry_key, plain.geometry_key);
    // The command way: the same value, the param follows; out of range is
    // clamped.
    push(&mut a, &[StudioCommand::Control { id: "geometry.cell".into(), value: 9.0 }]);
    assert_eq!(param(&a, &cell), ParamValue::F32(2.0));
    // Back on the style's own value: the keys are the untouched ones.
    set_param(&mut a, &cell, ParamValue::F32(1.0));
    assert!(!a.world.res::<Studio>().state.controls.contains_key(VOXEL));
    assert_eq!(current(&a).geometry_key, plain.geometry_key);
    // Another style's param holds for that style only.
    let bands = control_param(TOON, "surface.bands");
    set_param(&mut a, &bands, ParamValue::F32(4.0));
    assert_eq!(current(&a).geometry_key, plain.geometry_key, "Toon's bands don't touch the voxel horse");
    push(&mut a, &[style(TOON)]);
    assert_eq!(current(&a).surface.bands, 4);
    // The strip's own params.
    let reg = a.world.res::<ParamRegistry>();
    assert!(reg.id(VARIATIONS).is_some() && reg.id(VARIATIONS_MARKED).is_some());
}

/// The variations strip: the same asset and style, the asset's own
/// materials first, the ones that can't go on it marked with the reason;
/// picking a tile never moves the asset.
#[test]
fn variations_keep_the_asset_and_style() {
    let mut a = app();
    let data = a.world.res::<StudioData>().0.clone();
    let (count, marked) = a.world.res::<StudioParams>().strip(&a.world.res::<ParamRegistry>());
    push(&mut a, &[asset(HORSE), style(VOXEL), mat(BAY)]);
    let tiles = variations(&data, &sel(&a), count, marked);
    assert_eq!(tiles, variations(&data, &sel(&a), count, marked), "the same strip every time");
    assert!(tiles.len() <= count && tiles.len() > 1);
    assert!(tiles[0].current && tiles[0].material == BAY);
    assert!(tiles.iter().all(|t| t.selection.asset == HORSE && t.selection.style == VOXEL));
    let coats: Vec<&str> = tiles.iter().skip(1).take_while(|t| t.compat == Compat::Native).map(|t| t.material.as_str()).collect();
    assert!(coats.len() >= 4 && coats.iter().all(|m| m.starts_with("horse.coat.")), "{coats:?}");
    for t in &tiles {
        assert_eq!(t.why.is_some(), !t.compat.applies(), "{}", t.material);
    }
    assert!(tiles.iter().filter(|t| t.why.is_some()).count() <= marked);

    // On a cow the horse coats show, greyed: "Made for horses".
    push(&mut a, &[asset(COW)]);
    let tiles = variations(&data, &sel(&a), 32, 8);
    let marked_tiles: Vec<&Variation> = tiles.iter().filter(|t| t.why.is_some()).collect();
    assert!(!marked_tiles.is_empty());
    assert!(marked_tiles.len() <= 8);
    assert!(marked_tiles.iter().all(|t| t.why.as_deref().is_some_and(|w| w.starts_with("Made for ") || w.starts_with("Can't go on "))));
    assert!(marked_tiles.iter().filter(|t| t.material.starts_with("horse.coat.")).all(|t| t.why.as_deref() == Some("Made for horses")));
    assert!(marked_tiles.iter().any(|t| t.material.starts_with("horse.coat.")));
    // Picking a greyed tile changes nothing and offers the horse.
    let record = sel(&a);
    let horse_coat = marked_tiles.iter().find(|t| t.material.starts_with("horse.coat.")).unwrap().material.clone();
    push(&mut a, &[StudioCommand::Variation(horse_coat.clone())]);
    assert_eq!(sel(&a), record);
    assert!(matches!(&a.world.res::<Studio>().state.notice, Notice::NotApplied { offer, .. } if offer == HORSE));

    // A tile never moves the asset, even one the auto-match chose: a brick
    // tile on the opening horse (nothing chosen on purpose) stays on the
    // horse, where picking brick in the materials bar previews it on a wall.
    let mut b = app();
    assert!(!b.world.res::<Studio>().state.explicit.asset);
    push(&mut b, &[StudioCommand::Variation(BRICK.into())]);
    assert_eq!((sel(&b).asset.as_str(), sel(&b).material.as_str()), (HORSE, BRICK));
    assert!(!b.world.res::<Studio>().state.explicit.asset, "the tile leaves the auto-match as it was");
    push(&mut b, &[mat(BRICK)]);
    assert_eq!(sel(&b).asset, "building.wall");
}

/// While the studio isn't on screen it leaves the legacy viewer alone: no
/// catalogue row opened, no light changed. On screen it takes them; off
/// again, the row it opened is closed.
#[test]
fn the_studio_leaves_the_legacy_viewer_alone() {
    let mut a = app();
    a.world.res_mut::<Studio>().active = false;
    a.run_headless(2, DT);
    assert_eq!(open_row(&a), None, "leaving the studio closes the row it opened");
    let light = a.world.res::<Viewer>().light;
    push(&mut a, &[mat(GREY), StudioCommand::Light("night".into())]);
    assert_eq!(sel(&a).material, GREY, "the record still follows the page");
    assert_eq!((open_row(&a), a.world.res::<Viewer>().light), (None, light));
    a.world.res_mut::<Studio>().active = true;
    a.run_headless(3, DT);
    assert_eq!((open_row(&a).as_deref(), a.world.res::<Viewer>().light), (Some(GREY), 3));
}

/// One decision of tools/materials/belongs_on.json (the same rule as
/// tools/materials/belongs_on.mjs): the longest matching id key, else the
/// row's subcategory.
fn decision<'a>(rules: &'a serde_json::Value, id: &str, category: &str, sub: &str) -> Option<&'a str> {
    let ids = rules["ids"].as_object()?;
    let best = ids.keys().filter(|k| if k.ends_with('.') { id.starts_with(k.as_str()) } else { id == k.as_str() }).max_by_key(|k| k.len());
    match best {
        Some(k) => ids[k].as_str(),
        None => rules["subcategories"][format!("{category}/{sub}")].as_str(),
    }
}

/// The belongs_on fill: every row of the material catalogue has a reviewed
/// decision (a class, deliberately generic, or waiting for its class), and
/// the catalogue says exactly what the rules say. Coats, furs, hides, wall,
/// roof and ground materials are all decided; none names a class that
/// doesn't exist (`Catalogs::parse` refuses that).
#[test]
fn every_material_row_has_a_reviewed_belongs_on() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/materials/belongs_on.json");
    let rules: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let c = catalogs().unwrap();
    let mut tagged: BTreeMap<String, usize> = BTreeMap::new();
    let (mut generic, mut waiting) = (0, 0);
    for m in &c.materials.rows {
        let d = decision(&rules, &m.id, &m.category, &m.subcategory).unwrap_or_else(|| panic!("{}: no rule (add {}/{} to belongs_on.json)", m.id, m.category, m.subcategory));
        if let Some(why) = d.strip_prefix("generic:") {
            assert!(!why.trim().is_empty(), "{}: a generic decision says why", m.id);
            assert!(m.belongs_on.is_empty(), "{}", m.id);
            generic += 1;
        } else if let Some(class) = d.strip_prefix("waits ") {
            assert!(class.contains(':'), "{}: a waiting decision says why", m.id);
            assert!(m.belongs_on.is_empty(), "{}", m.id);
            waiting += 1;
        } else {
            assert_eq!(m.belongs_on, [d.to_string()], "{}: run `node tools/materials/belongs_on.mjs --write`", m.id);
            *tagged.entry(d.to_string()).or_default() += 1;
        }
    }
    assert_eq!(generic + waiting + tagged.values().sum::<usize>(), c.materials.rows.len());
    let n = |k: &str| tagged.get(k).copied().unwrap_or(0);
    assert_eq!((n("quadruped.horse"), n("quadruped.cow"), n("quadruped.dog")), (40, 9, 12), "every horse, cattle and dog coat");
    assert!(n("building.wall") >= 10 && n("building.roof") >= 8 && n("land.terrain") >= 13, "{tagged:?}");
    // Every coat and fur subcategory is decided as made for an animal (a
    // class or waiting for one), never generic by accident.
    for m in c.materials.rows.iter().filter(|m| matches!(m.subcategory.as_str(), "horse_coats" | "livestock" | "dogs_cats" | "wild_mammals" | "hides")) {
        let d = decision(&rules, &m.id, &m.category, &m.subcategory).unwrap();
        assert!(!d.starts_with("generic:"), "{} is made for an animal", m.id);
    }
}

/// Why body parts stay generic: every region's default material goes on
/// its own asset, and the cow wears the horse's hooves and muzzle, so a
/// "made for horses" tag on them would make the cow's own defaults foreign
/// on it. Each part still lands on the region it names.
#[test]
fn body_parts_stay_where_their_assets_wear_them() {
    let c = catalogs().unwrap();
    for a in &c.assets.assets {
        for r in a.regions.iter().filter(|r| r.default_material.is_some()) {
            let id = r.default_material.as_deref().unwrap();
            let m = c.materials.get(id).unwrap();
            assert!(evani_studio::compat::compat(&c, a, m).applies(), "{}'s {} wears {id}, which must go on it", a.id, r.id);
        }
    }
    let mut tagged = c.clone();
    for m in &mut tagged.materials.rows {
        if m.id == "horse.hoof.dark" {
            m.belongs_on = vec!["quadruped.horse".into()];
        }
    }
    let cow = tagged.assets.asset(COW).unwrap();
    let hoof = tagged.materials.get("horse.hoof.dark").unwrap();
    assert!(!evani_studio::compat::compat(&tagged, cow, hoof).applies(), "tagged, the cow's own hoof would be foreign on it");
}
