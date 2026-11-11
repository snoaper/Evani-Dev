//! Ground textures proof kit (TopGun W3, docs/GROUND_TEXTURES.md): a
//! sandstone wall with a slanted buttress, a ploughed field and airbase
//! paving, drawn through `kit_surface` (`shaders/ground.wgsl`) with
//! evani-render's kit textures. The layers come from the game folder's
//! `kit-textures.json` (made by `cargo xtask convert-textures`).
//!
//! Every tunable is a registered parameter (`kit.ground_textures.*`), so
//! the live tools can sweep the fade, the hex band and the tile sizes.

use evani_core::camera::CameraPose;
use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::params::{BoolParam, F32Param, I32Param, ParamDef, ParamRegistry};
use evani_core::plugin::{KitPlugin, LookData, PluginContext, PluginError};
use evani_render::kit_material::KitMaterial;
use evani_render::{MeshId, MeshInstance, RenderWorld};

/// The kit's surface code.
pub const WGSL: &str = include_str!("../shaders/ground.wgsl");

/// The kit's parameters.
#[derive(Debug, Clone, Copy)]
pub struct Params {
    textures: BoolParam,
    view: I32Param,
    fade_start: F32Param,
    fade_end: F32Param,
    hex: BoolParam,
    hex_band: F32Param,
    sharpness: F32Param,
    normal: F32Param,
    hex_rotation: F32Param,
    chroma: F32Param,
    tile_rock: F32Param,
    tile_field: F32Param,
    tile_paving: F32Param,
}

/// The proof kit.
#[derive(Debug, Default, Clone, Copy)]
pub struct GroundTexturesKit;

impl KitPlugin for GroundTexturesKit {
    fn name(&self) -> &'static str {
        "ground_textures"
    }

    fn build(&self, kit: &mut PluginContext) -> Result<(), PluginError> {
        let f = |name: &str, v: f32, lo: f32, hi: f32, what: &str| ParamDef::f32(name, v).range(lo, hi).describe(what);
        let p = Params {
            textures: kit.param_bool(ParamDef::bool("kit.ground_textures.textures", true).describe("Sample the texture layers (off: the procedural recipe only, the 'before' picture)."))?,
            view: kit.param_i32(ParamDef::i32("kit.ground_textures.view", 0).range(0.0, 3.0).describe("Camera: 0 rock wall, 1 field, 2 paving, 3 far overview."))?,
            fade_start: kit.param_f32(f("kit.ground_textures.fade_start", 0.03, 0.0, 1.0, "Detail starts fading when a pixel covers this much of a tile."))?,
            fade_end: kit.param_f32(f("kit.ground_textures.fade_end", 0.125, 0.0, 1.0, "Detail is gone (no fetches) when a pixel covers this much of a tile."))?,
            hex: kit.param_bool(ParamDef::bool("kit.ground_textures.hex", true).describe("Hex-tiling on the near band of flat ground."))?,
            hex_band: kit.param_f32(f("kit.ground_textures.hex_band", 0.012, 0.0, 1.0, "Hex-tiling while a pixel covers less than this much of a tile."))?,
            sharpness: kit.param_f32(f("kit.ground_textures.biplanar_sharpness", 6.0, 1.0, 16.0, "How sharply the two biplanar projections hand over on walls."))?,
            normal: kit.param_f32(f("kit.ground_textures.normal_strength", 1.0, 0.0, 2.0, "Normal map strength."))?,
            hex_rotation: kit.param_f32(f("kit.ground_textures.hex_rotation", 0.0, 0.0, 1.0, "How far hex cells turn (0: offsets only, for directional layers such as furrows; 1: any angle)."))?,
            chroma: kit.param_f32(f("kit.ground_textures.colour_strength", 0.6, 0.0, 1.0, "How much of the texture's own colour shows over the recipe (0: brightness only)."))?,
            tile_rock: kit.param_f32(f("kit.ground_textures.tile_rock", 6.0, 0.5, 64.0, "Rock texture tile size (m)."))?,
            tile_field: kit.param_f32(f("kit.ground_textures.tile_field", 4.0, 0.5, 64.0, "Field texture tile size (m)."))?,
            tile_paving: kit.param_f32(f("kit.ground_textures.tile_paving", 10.0, 0.5, 64.0, "Paving texture tile size (m): four slabs."))?,
        };
        kit.resource(p);
        kit.system(SystemDesc::new("ground_textures.draw", Stage::Frame, draw));
        Ok(())
    }

    fn free_roam(&self) -> bool {
        true
    }
}

/// The look's material slots the three surfaces use, and the instance
/// colour that undoes each slot's base colour (the recipe owns the colour).
fn slots(world: &World) -> [(u32, [f32; 4]); 3] {
    let look = world.res::<LookData>();
    ["concrete_dark", "dirt", "concrete"].map(|name| {
        let i = RenderWorld::material(&look, name);
        let base = evani_look_photoreal::MATERIALS.get(i as usize).map_or([1.0; 4], |m| m.1);
        (i, [1.0 / base[0].max(0.01), 1.0 / base[1].max(0.01), 1.0 / base[2].max(0.01), 1.0])
    })
}

/// The cameras: rock wall, field and paving up close, and far.
pub fn camera(view: i32) -> CameraPose {
    let pose = |eye: [f32; 3], target: [f32; 3]| CameraPose { eye, target, up: [0.0, 1.0, 0.0], fov_y_deg: 55.0 };
    match view {
        1 => pose([-40.0, 1.7, 30.0], [-46.0, 0.0, 22.0]),
        2 => pose([40.0, 1.7, 30.0], [46.0, 0.0, 22.0]),
        3 => pose([0.0, 70.0, 150.0], [0.0, 0.0, -10.0]),
        _ => pose([-4.0, 3.0, -16.0], [0.0, 4.0, -30.0]),
    }
}

fn draw(world: &mut World) {
    let p = *world.res::<Params>();
    let s = slots(world);
    let (view, rec) = {
        let r = world.res::<ParamRegistry>();
        let on = if r.bool(p.textures) { 1.0 } else { 0.0 };
        let hex = if r.bool(p.hex) { 1.0 } else { 0.0 };
        (
            r.i32(p.view),
            [
                [s[0].0 as f32, s[1].0 as f32, s[2].0 as f32, on],
                [r.f32(p.fade_start), r.f32(p.fade_end), hex, r.f32(p.hex_band)],
                [r.f32(p.tile_rock), r.f32(p.tile_field), r.f32(p.tile_paving), r.f32(p.sharpness)],
                [r.f32(p.normal), r.f32(p.hex_rotation), r.f32(p.chroma), 0.0],
            ],
        )
    };
    if let Some(mut km) = world.try_res_mut::<KitMaterial>() {
        if !km.has_code() {
            km.set_wgsl(WGSL);
        }
        if km.data() != rec.as_slice() {
            *km.data_mut() = rec.to_vec();
        }
    }
    let mut rw = world.res_mut::<RenderWorld>();
    rw.camera = camera(view);
    let mut put = |mesh: MeshId, pos: [f32; 3], scale: [f32; 3], rot: [f32; 4], slot: (u32, [f32; 4])| {
        let mut m = MeshInstance::new(mesh, pos);
        m.scale = scale;
        m.rotation = rot;
        m.material = slot.0;
        m.color = slot.1;
        rw.draw_mesh(m);
    };
    let id = [0.0, 0.0, 0.0, 1.0];
    // The field (west) and the paving (east), each 200 m square.
    put(MeshId::PLANE, [-100.0, 0.0, 0.0], [200.0, 1.0, 200.0], id, s[1]);
    put(MeshId::PLANE, [100.0, 0.0, 0.0], [200.0, 1.0, 200.0], id, s[2]);
    // The canyon wall (north) and a slanted buttress in front of it.
    put(MeshId::CUBE, [0.0, 12.0, -40.0], [90.0, 24.0, 12.0], id, s[0]);
    put(MeshId::CUBE, [8.0, 5.0, -31.0], [14.0, 10.0, 10.0], evani_render::math::quat_axis_angle([0.3, 1.0, 0.2], 0.7), s[0]);
}
