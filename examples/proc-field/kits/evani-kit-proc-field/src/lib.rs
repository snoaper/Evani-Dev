//! The procedural library proof kit: a floor, a fixed camera, and every
//! compiled-in procedural CPU asset placed in a row (`kit.proc_field.spacing`
//! apart). With no procedural asset listed it draws only the floor.

// The parity scene (feature `wall`), the faceted ball's live scene (feature
// `facet`) and the granite ball scene (feature `granite`) leave the field's
// own drawing out, as do the live oak block (feature `oak`) and the
// concrete cone scene (feature `cone`).
#![cfg_attr(any(feature = "wall", feature = "facet", feature = "granite", feature = "oak", feature = "cone", feature = "cube", feature = "pebbles", feature = "oaktree", feature = "clouds", feature = "shore", feature = "canyon", feature = "office", feature = "skydome", feature = "mountain", feature = "river", feature = "npc"), allow(unused_imports))]

use evani_core::camera::CameraPose;
use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::params::{F32Param, ParamDef, ParamRegistry};
use evani_core::plugin::{KitPlugin, LookData, PluginContext, PluginError};
use evani_render::{MeshId, MeshInstance, RenderWorld};

// The render service's own views of the plain field (kit feature `commons`,
// only the separate app apps/commons-render turns it on; docs/EVANI_COMMONS.md).
#[cfg(all(feature = "commons", not(any(feature = "wall", feature = "facet", feature = "granite", feature = "oak", feature = "cone", feature = "cube", feature = "pebbles", feature = "oaktree", feature = "clouds", feature = "shore", feature = "canyon", feature = "office", feature = "skydome", feature = "mountain", feature = "river", feature = "npc"))))]
mod commons;
// Its test scenes and measures (Evani.commons' behaviour assets).
#[cfg(all(feature = "commons", not(any(feature = "wall", feature = "facet", feature = "granite", feature = "oak", feature = "cone", feature = "cube", feature = "pebbles", feature = "oaktree", feature = "clouds", feature = "shore", feature = "canyon", feature = "office", feature = "skydome", feature = "mountain", feature = "river", feature = "npc"))))]
pub mod scenes;
#[cfg(all(test, feature = "commons", not(any(feature = "wall", feature = "facet", feature = "granite", feature = "oak", feature = "cone", feature = "cube", feature = "pebbles", feature = "oaktree", feature = "clouds", feature = "shore", feature = "canyon", feature = "office", feature = "skydome", feature = "mountain", feature = "river", feature = "npc"))))]
mod scenes_tests;

#[cfg(feature = "wall")]
pub mod wall;

#[cfg(feature = "oak")]
pub mod oak;

#[cfg(feature = "facet")]
pub mod facet;

// The granite ball scene (feature `granite`, the live granite session).
#[cfg(feature = "granite")]
pub mod granite;

// The concrete cone scene (feature `cone`, the live cone session).
#[cfg(feature = "cone")]
pub mod cone;

// The concrete cube scene (feature `cube`, the Asset Studio cube session;
// it turns on `facet` for its floor pass).
#[cfg(feature = "cube")]
pub mod cube;

// The oak trees scene (feature `oaktree`, the Asset Studio oak session).
#[cfg(feature = "oaktree")]
pub mod oaktree;
// The distance fog's neutral staging (feature `fogstage`, the Asset Studio
// distance fog session, column 74): plain grey blocks on flat grey ground at
// near, middle and far distance, in the oak trees scene's slot (it turns on
// `oaktree`).
#[cfg(feature = "fogstage")]
pub mod fogstage;

// The pebbles' scene (feature `pebbles`, the Asset Studio pebbles session;
// it turns on `facet` for its floor pass).
#[cfg(feature = "pebbles")]
pub mod pebbles;
// The medium rocks' scene (feature `mrocks`, the Asset Studio medium rocks
// session, column 23; it turns on `pebbles` and with it the floor pass).
#[cfg(feature = "mrocks")]
pub mod mrocks;
// The jagged rock formation's scene (feature `jrocks`, the Asset Studio
// jagged rock formations session, column 24; it turns on `pebbles` and with
// it the floor pass).
#[cfg(feature = "jrocks")]
pub mod jrocks;
// The rounded river rocks' scene (feature `rrocks`, the Asset Studio rounded
// river rocks session, column 25; it turns on `pebbles` and with it the
// floor pass).
#[cfg(feature = "rrocks")]
pub mod rrocks;
// The rock pile's scene (feature `rpiles`, the Asset Studio rock piles
// session, column 26; it turns on `pebbles` and with it the floor pass).
#[cfg(feature = "rpiles")]
pub mod rpiles;
// The dark loam soil's scene (feature `loam`, the Asset Studio soil
// session, column 69; it turns on `pebbles` and with it the floor pass).
#[cfg(feature = "loam")]
pub mod loam;
// The fresh powder snow's scene (feature `snowpowder`, the Asset Studio
// fresh powder session, column 76; it turns on `pebbles` and with it the
// floor pass).
#[cfg(feature = "snowpowder")]
pub mod snowpowder;

// The volumetric clouds' scene (feature `clouds`, the Asset Studio clouds
// session).
#[cfg(feature = "clouds")]
pub mod clouds;

// The shoreline's scene (feature `shore`, the Asset Studio shorelines
// session; it turns on `facet` for its floor pass).
#[cfg(feature = "shore")]
pub mod shore;

// The canyons' scene (feature `canyon`, the Asset Studio canyons session;
// it turns on `facet` for its floor pass).
#[cfg(feature = "canyon")]
pub mod canyon;

// The office building's scene (feature `office`, the Asset Studio office
// buildings session, column 13; it turns on `facet` for its floor pass).
#[cfg(feature = "office")]
pub mod office;
// The townhouses' scene (feature `townhouse`, the Asset Studio townhouses
// session, column 37): in the office building's slot (it turns `office` on
// for its modules), its own camera, light and placement.
#[cfg(feature = "townhouse")]
pub mod townhouse;

// The mountains' scene (feature `mountain`, the Asset Studio mountains
// session, column 19): the massif at the player's scale under a sky.
#[cfg(feature = "mountain")]
pub mod mountain;
// The scree slopes' scene (feature `scree`, the Asset Studio scree slopes
// session, column 27): in the mountains scene's slot (it turns `mountain`
// on for its modules), its own camera, light and placements.
#[cfg(feature = "scree")]
pub mod scree;
// The rolling hills' scene (feature `hills`, the Asset Studio rolling hills
// session, column 28): in the mountains scene's slot, as the scree's.
#[cfg(feature = "hills")]
pub mod hills;
// The short grass's scene (feature `sgrass`, the Asset Studio short grass
// session, column 68): in the rolling hills scene's slot (its sky pass).
#[cfg(feature = "sgrass")]
pub mod shortgrass;
// The caves' scene (feature `cave`, the Asset Studio caves session, column
// 29): in the mountains scene's slot, as the hills'.
#[cfg(feature = "cave")]
pub mod cave;
// The dynamic sky dome's scene (feature `skydome`, the Asset Studio sky dome
// session, column 21).
#[cfg(all(feature = "skydome", not(feature = "sunmoon")))]
pub mod skydome;
// The sun and moon discs' scene (feature `sunmoon`, the Asset Studio sun and
// moon session, column 38): in the sky dome scene's slot (it turns on
// `skydome`), golden hour over dark fields.
#[cfg(feature = "sunmoon")]
pub mod sunmoon;
// The rivers' scene (feature `river`, the Asset Studio rivers session,
// column 20): outdoors under the sky dome, two cameras.
#[cfg(feature = "river")]
pub mod river;
// The NPC body variations' scene (feature `npc`, the Asset Studio full NPC
// body variations session, column 22; it turns on `facet` for its floor
// pass).
#[cfg(feature = "npc")]
pub mod npc;

#[derive(Debug, Clone, Copy)]
#[cfg_attr(any(feature = "wall", feature = "facet", feature = "granite", feature = "oak", feature = "cone", feature = "cube", feature = "pebbles", feature = "oaktree", feature = "clouds", feature = "shore", feature = "canyon", feature = "office", feature = "skydome", feature = "mountain", feature = "river", feature = "npc"), allow(dead_code))]
struct Params {
    spacing: F32Param,
}

/// The proof kit.
#[derive(Debug, Default, Clone, Copy)]
pub struct ProcFieldKit;

impl KitPlugin for ProcFieldKit {
    fn name(&self) -> &'static str {
        "proc_field"
    }

    fn build(&self, kit: &mut PluginContext) -> Result<(), PluginError> {
        let spacing = kit.param_f32(ParamDef::f32("kit.proc_field.spacing", 30.0).range(1.0, 200.0).unit("m").describe("Distance between the placed assets"))?;
        kit.resource(Params { spacing });
        #[cfg(all(feature = "commons", not(any(feature = "wall", feature = "facet", feature = "granite", feature = "oak", feature = "cone", feature = "cube", feature = "pebbles", feature = "oaktree", feature = "clouds", feature = "shore", feature = "canyon", feature = "office", feature = "skydome", feature = "mountain", feature = "river", feature = "npc"))))]
        commons::build(kit)?;
        #[cfg(feature = "wall")]
        wall::build(kit)?;
        #[cfg(feature = "facet")]
        facet::build(kit)?;
        #[cfg(feature = "granite")]
        granite::build(kit)?;
        #[cfg(all(feature = "oak", not(any(feature = "wall", feature = "facet", feature = "granite", feature = "cone", feature = "cube", feature = "pebbles", feature = "oaktree", feature = "clouds", feature = "shore", feature = "canyon", feature = "office", feature = "skydome", feature = "mountain", feature = "river", feature = "npc"))))]
        oak::build(kit)?;
        #[cfg(feature = "cone")]
        cone::build(kit)?;
        #[cfg(feature = "cube")]
        cube::build(kit)?;
        #[cfg(all(feature = "oaktree", not(feature = "fogstage")))]
        oaktree::build(kit)?;
        #[cfg(feature = "fogstage")]
        fogstage::build(kit)?;
        #[cfg(all(feature = "pebbles", not(any(feature = "mrocks", feature = "jrocks", feature = "rrocks", feature = "rpiles", feature = "loam", feature = "snowpowder"))))]
        pebbles::build(kit)?;
        #[cfg(all(feature = "mrocks", not(feature = "jrocks")))]
        mrocks::build(kit)?;
        #[cfg(feature = "jrocks")]
        jrocks::build(kit)?;
        #[cfg(all(feature = "rrocks", not(any(feature = "mrocks", feature = "jrocks"))))]
        rrocks::build(kit)?;
        #[cfg(all(feature = "rpiles", not(any(feature = "mrocks", feature = "jrocks", feature = "rrocks"))))]
        rpiles::build(kit)?;
        #[cfg(all(feature = "loam", not(any(feature = "mrocks", feature = "jrocks", feature = "rrocks", feature = "rpiles"))))]
        loam::build(kit)?;
        #[cfg(all(feature = "snowpowder", not(any(feature = "mrocks", feature = "jrocks", feature = "rrocks", feature = "rpiles", feature = "loam"))))]
        snowpowder::build(kit)?;
        #[cfg(feature = "clouds")]
        clouds::build(kit)?;
        #[cfg(feature = "shore")]
        shore::build(kit)?;
        #[cfg(feature = "canyon")]
        canyon::build(kit)?;
        #[cfg(all(feature = "office", not(feature = "townhouse")))]
        office::build(kit)?;
        #[cfg(feature = "townhouse")]
        townhouse::build(kit)?;
        #[cfg(all(feature = "mountain", not(any(feature = "scree", feature = "hills", feature = "cave"))))]
        mountain::build(kit)?;
        #[cfg(all(feature = "scree", not(any(feature = "hills", feature = "cave"))))]
        scree::build(kit)?;
        #[cfg(all(feature = "hills", not(any(feature = "cave", feature = "sgrass"))))]
        hills::build(kit)?;
        #[cfg(all(feature = "sgrass", not(feature = "cave")))]
        shortgrass::build(kit)?;
        #[cfg(feature = "cave")]
        cave::build(kit)?;
        #[cfg(all(feature = "skydome", not(feature = "sunmoon")))]
        skydome::build(kit)?;
        #[cfg(feature = "sunmoon")]
        sunmoon::build(kit)?;
        #[cfg(feature = "river")]
        river::build(kit)?;
        #[cfg(feature = "npc")]
        npc::build(kit)?;
        kit.system(SystemDesc::new("proc_field.draw", Stage::Frame, draw));
        Ok(())
    }

    fn free_roam(&self) -> bool {
        true
    }
}

/// Place every CPU asset once (the procedural module draws them).
#[cfg(all(feature = "procedural", not(any(feature = "wall", feature = "facet", feature = "granite", feature = "oak", feature = "cone", feature = "cube", feature = "pebbles", feature = "oaktree", feature = "clouds", feature = "shore", feature = "canyon", feature = "office", feature = "skydome", feature = "mountain", feature = "river", feature = "npc"))))]
fn place(world: &mut World, spacing: f32) {
    use evani_procedural::{Path, ProcScene, Registry};
    let features: Vec<String> = match world.try_res::<Registry>() {
        Some(r) => r.assets().filter(|a| a.info().path == Path::Cpu).map(|a| a.info().feature()).collect(),
        None => return,
    };
    #[cfg(feature = "commons")]
    let off = commons::subject_off(world);
    let Some(mut s) = world.try_res_mut::<ProcScene>() else { return };
    #[cfg(feature = "commons")]
    if off {
        if !s.placements.is_empty() {
            s.clear();
        }
        return;
    }
    if s.placements.len() == features.len() {
        return;
    }
    s.clear();
    let n = features.len() as f32;
    for (i, f) in features.iter().enumerate() {
        s.place(f, [(i as f32 - (n - 1.0) * 0.5) * spacing, 0.0, 0.0]);
    }
}

/// The realism parity scene draws its own (feature `wall`).
#[cfg(all(feature = "wall", not(any(feature = "facet", feature = "granite", feature = "cone", feature = "cube", feature = "pebbles", feature = "oaktree", feature = "clouds", feature = "shore", feature = "canyon", feature = "office", feature = "skydome", feature = "mountain", feature = "river", feature = "npc"))))]
fn draw(world: &mut World) {
    wall::draw(world);
}

/// The faceted ball's live scene draws its own (feature `facet`).
#[cfg(all(feature = "facet", not(any(feature = "cone", feature = "cube", feature = "pebbles", feature = "oaktree", feature = "clouds", feature = "shore", feature = "canyon", feature = "office", feature = "skydome", feature = "mountain", feature = "river", feature = "npc"))))]
fn draw(world: &mut World) {
    facet::draw(world);
}

/// The granite ball scene draws its own (feature `granite`; the faceted
/// ball's scene wins when both are on).
#[cfg(all(feature = "granite", not(any(feature = "facet", feature = "cone", feature = "cube", feature = "pebbles", feature = "oaktree", feature = "clouds", feature = "shore", feature = "canyon", feature = "office", feature = "skydome", feature = "mountain", feature = "river", feature = "npc"))))]
fn draw(world: &mut World) {
    granite::draw(world);
}

/// The concrete cone scene draws its own (feature `cone`; it wins over the
/// other scenes when several are on, the cube's apart).
#[cfg(all(feature = "cone", not(any(feature = "cube", feature = "pebbles", feature = "oaktree", feature = "clouds", feature = "shore", feature = "canyon", feature = "office", feature = "skydome", feature = "mountain", feature = "river", feature = "npc"))))]
fn draw(world: &mut World) {
    cone::draw(world);
}

/// The concrete cube scene draws its own (feature `cube`; it wins over the
/// other scenes when several are on).
#[cfg(all(feature = "cube", not(any(feature = "pebbles", feature = "oaktree", feature = "clouds", feature = "shore", feature = "canyon", feature = "office", feature = "skydome", feature = "mountain", feature = "river", feature = "npc"))))]
fn draw(world: &mut World) {
    cube::draw(world);
}

/// The pebbles' scene draws its own (feature `pebbles`; it wins over the
/// other scenes when several are on).
#[cfg(all(feature = "pebbles", not(any(feature = "oaktree", feature = "clouds", feature = "shore", feature = "canyon", feature = "office", feature = "skydome", feature = "mountain", feature = "river", feature = "npc"))))]
fn draw(world: &mut World) {
    #[cfg(not(any(feature = "mrocks", feature = "jrocks", feature = "rrocks", feature = "rpiles", feature = "loam", feature = "snowpowder")))]
    pebbles::draw(world);
    // The medium rocks' scene takes the pebbles' place (feature `mrocks`),
    // the jagged rock formation's too (feature `jrocks`, which wins over it).
    #[cfg(all(feature = "mrocks", not(feature = "jrocks")))]
    mrocks::draw(world);
    #[cfg(feature = "jrocks")]
    jrocks::draw(world);
    // The rounded river rocks' scene too (feature `rrocks`; the other two
    // win over it).
    #[cfg(all(feature = "rrocks", not(any(feature = "mrocks", feature = "jrocks"))))]
    rrocks::draw(world);
    // The rock pile's scene too (feature `rpiles`; the other three win
    // over it).
    #[cfg(all(feature = "rpiles", not(any(feature = "mrocks", feature = "jrocks", feature = "rrocks"))))]
    rpiles::draw(world);
    // The dark loam soil's scene too (feature `loam`; the other four win
    // over it).
    #[cfg(all(feature = "loam", not(any(feature = "mrocks", feature = "jrocks", feature = "rrocks", feature = "rpiles"))))]
    loam::draw(world);
    // The fresh powder snow's scene too (feature `snowpowder`; the other
    // five win over it).
    #[cfg(all(feature = "snowpowder", not(any(feature = "mrocks", feature = "jrocks", feature = "rrocks", feature = "rpiles", feature = "loam"))))]
    snowpowder::draw(world);
}

/// The oak trees scene draws its own (feature `oaktree`; it wins over the
/// other scenes when several are on).
#[cfg(all(feature = "oaktree", not(any(feature = "fogstage", feature = "clouds", feature = "shore", feature = "canyon", feature = "office", feature = "skydome", feature = "mountain", feature = "river", feature = "npc"))))]
fn draw(world: &mut World) {
    oaktree::draw(world);
}

/// The distance fog's staging draws its own (feature `fogstage`, in the oak
/// trees scene's slot).
#[cfg(all(feature = "fogstage", not(any(feature = "clouds", feature = "shore", feature = "canyon", feature = "office", feature = "skydome", feature = "mountain", feature = "river", feature = "npc"))))]
fn draw(world: &mut World) {
    fogstage::draw(world);
}

/// The volumetric clouds' scene draws its own (feature `clouds`; it wins
/// over the other scenes when several are on, the shoreline's apart).
#[cfg(all(feature = "clouds", not(any(feature = "shore", feature = "canyon", feature = "office", feature = "skydome", feature = "mountain", feature = "river", feature = "npc"))))]
fn draw(world: &mut World) {
    clouds::draw(world);
}

/// The shoreline's scene draws its own (feature `shore`; it wins over the
/// other scenes when several are on, the canyons' apart).
#[cfg(all(feature = "shore", not(any(feature = "canyon", feature = "office", feature = "skydome", feature = "mountain", feature = "river", feature = "npc"))))]
fn draw(world: &mut World) {
    shore::draw(world);
}

/// The canyons' scene draws its own (feature `canyon`; it wins over the
/// other scenes when several are on, the office building's apart).
#[cfg(all(feature = "canyon", not(any(feature = "office", feature = "skydome", feature = "mountain", feature = "river", feature = "npc"))))]
fn draw(world: &mut World) {
    canyon::draw(world);
}

/// The office building's scene draws its own (feature `office`; it wins
/// over the other scenes when several are on, the sky dome's and the
/// rivers' apart).
#[cfg(all(feature = "office", not(any(feature = "skydome", feature = "mountain", feature = "river", feature = "npc"))))]
fn draw(world: &mut World) {
    #[cfg(not(feature = "townhouse"))]
    office::draw(world);
    #[cfg(feature = "townhouse")]
    townhouse::draw(world);
}

/// The dynamic sky dome's scene draws its own (feature `skydome`; it wins
/// over the other scenes when several are on, the rivers' apart).
#[cfg(all(feature = "skydome", not(any(feature = "river", feature = "sunmoon"))))]
fn draw(world: &mut World) {
    skydome::draw(world);
}

/// The sun and moon discs' scene draws its own (feature `sunmoon`, in the
/// sky dome scene's slot).
#[cfg(all(feature = "sunmoon", not(feature = "river")))]
fn draw(world: &mut World) {
    sunmoon::draw(world);
}

/// The mountains' scene draws its own (feature `mountain`; it wins over
/// the other scenes when several are on, the sky dome's and the rivers'
/// apart).
#[cfg(all(feature = "mountain", not(any(feature = "skydome", feature = "river", feature = "npc"))))]
fn draw(world: &mut World) {
    #[cfg(not(any(feature = "scree", feature = "hills", feature = "cave")))]
    mountain::draw(world);
    #[cfg(all(feature = "scree", not(any(feature = "hills", feature = "cave"))))]
    scree::draw(world);
    #[cfg(all(feature = "hills", not(any(feature = "cave", feature = "sgrass"))))]
    hills::draw(world);
    #[cfg(all(feature = "sgrass", not(feature = "cave")))]
    shortgrass::draw(world);
    #[cfg(feature = "cave")]
    cave::draw(world);
}

/// The rivers' scene draws its own (feature `river`; it wins over the
/// other scenes when several are on).
#[cfg(feature = "river")]
fn draw(world: &mut World) {
    river::draw(world);
}

/// The NPC body variations' scene draws its own (feature `npc`; it wins
/// over the other scenes when several are on, the sky dome's and the
/// rivers' apart).
#[cfg(all(feature = "npc", not(any(feature = "skydome", feature = "river"))))]
fn draw(world: &mut World) {
    npc::draw(world);
}

/// The live oak block draws its own (feature `oak`; any other scene wins
/// when it is on too).
#[cfg(all(feature = "oak", not(any(feature = "wall", feature = "facet", feature = "granite", feature = "cone", feature = "cube", feature = "pebbles", feature = "oaktree", feature = "clouds", feature = "shore", feature = "canyon", feature = "office", feature = "skydome", feature = "mountain", feature = "river", feature = "npc"))))]
fn draw(world: &mut World) {
    oak::draw(world);
}

#[cfg(not(any(feature = "wall", feature = "facet", feature = "granite", feature = "oak", feature = "cone", feature = "cube", feature = "pebbles", feature = "oaktree", feature = "clouds", feature = "shore", feature = "canyon", feature = "office", feature = "skydome", feature = "mountain", feature = "river", feature = "npc")))]
fn draw(world: &mut World) {
    let p = *world.res::<Params>();
    let spacing = world.res::<ParamRegistry>().f32(p.spacing);
    #[cfg(feature = "procedural")]
    place(world, spacing);
    #[cfg(feature = "commons")]
    let free = commons::update(world);
    #[cfg(feature = "commons")]
    let stone = RenderWorld::material(&world.res::<LookData>(), "stone");
    #[cfg(feature = "commons")]
    let commons_sweep = commons::sweep_floor(world);
    let floor = RenderWorld::material(&world.res::<LookData>(), "ground");
    let mut rw = world.res_mut::<RenderWorld>();
    rw.camera = CameraPose { eye: [0.0, 16.0, 34.0], target: [0.0, 1.0, 0.0], up: [0.0, 1.0, 0.0], fov_y_deg: 50.0 };
    #[cfg(feature = "commons")]
    if let Some(c) = free {
        rw.camera = c;
    }
    let _ = spacing;
    let mut f = MeshInstance::new(MeshId::PLANE, [0.0, -0.01, 0.0]);
    f.scale = [400.0, 1.0, 400.0];
    f.material = floor;
    f.color = [0.5, 0.5, 0.48, 1.0];
    // The render app: a neutral grey studio floor (its look has no "ground", whose
    // fallback is the look's first material, a warm brown); under the object
    // staging, the grey studio sweep's floor (commons.rs `staging`).
    #[cfg(feature = "commons")]
    {
        f.material = stone;
        f.color = commons_sweep.unwrap_or([1.0, 1.03, 1.12, 1.0]);
    }
    rw.draw_mesh(f);
    // The render app's wind meadow or stream (its test scenes 1 and 2).
    #[cfg(feature = "commons")]
    {
        drop(rw);
        commons::draw_scene(world);
    }
}
