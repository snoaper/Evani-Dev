//! The surface-relief proof (S3-10, tactical-fps feature `relief`; never in the
//! default build): the shared fixture `relief-tile.glb` (a panelled floor
//! slab with a height map and a knurl micro pattern, and a wall in three
//! columns with the ribs, dots and grain patterns,
//! docs/MODEL_FORMAT.md "Surface relief") drawn in front of the grid, with
//! the toon look's `micro_detail` patterns layered over the basic look (the
//! toon look's slot bodies and light then shade the whole scene) and the
//! camera held on the tile (a raised view, from 8 s a grazing one, from 16 s
//! the wall head-on), so
//! captures on every tier see the same views.
//! The model is embedded in the wasm (about 12 KB), not streamed.

use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::camera::CameraPose;
use evani_core::time::Time;
use evani_render::RenderWorld;
use evani_render::model::{ModelId, ModelInstance, Models};

/// The fixture (made by `tests/fixtures/models/relief-tile.mjs`).
pub const GLB: &[u8] = include_bytes!("../../../tests/fixtures/models/relief-tile.glb");
/// Where the tile stands: outside the marshal on the +z side.
pub const AT: [f32; 3] = [0.0, 0.0, 24.0];
/// The held camera: a little above and in front of the tile, looking at its
/// middle, so the floor is seen at about 30 degrees and the wall head-on.
pub const CAMERA: CameraPose = CameraPose {
    eye: [1.1, 1.9, 27.4],
    target: [0.0, 0.55, 24.0],
    up: [0.0, 1.0, 0.0],
    fov_y_deg: 50.0,
};
/// From [`GRAZING_AT`] seconds of sim time: a low view about 12 degrees
/// above the floor, where parallax hides the far side of each panel (finely
/// on High and Ultra, with the cheap 2-step march on the other tiers).
pub const GRAZING: CameraPose = CameraPose {
    eye: [0.6, 0.66, 26.6],
    target: [0.0, 0.12, 24.0],
    up: [0.0, 1.0, 0.0],
    fov_y_deg: 50.0,
};
/// When the camera switches from [`CAMERA`] to [`GRAZING`] (sim seconds).
pub const GRAZING_AT: f64 = 8.0;
/// From [`WALL_AT`] seconds: head-on, 1.3 m from the wall, so its three
/// micro patterns (ribs, dots, grain) fill the frame side by side.
pub const WALL: CameraPose = CameraPose {
    eye: [0.0, 1.1, 24.3],
    target: [0.0, 1.1, 23.0],
    up: [0.0, 1.0, 0.0],
    fov_y_deg: 50.0,
};
/// When the camera switches from [`GRAZING`] to [`WALL`] (sim seconds).
pub const WALL_AT: f64 = 16.0;

/// The proof's model, once added.
#[derive(Debug, Default)]
pub struct ProofRelief {
    pub id: Option<ModelId>,
}

/// The `Stage::Frame` system (after the kit's, so its camera wins).
pub fn system() -> SystemDesc {
    SystemDesc::new("tactical-fps.proof_relief", Stage::Frame, draw)
}

fn draw(world: &mut World) {
    let Some(mut models) = world.try_res_mut::<Models>() else { return };
    let Some(mut proof) = world.try_res_mut::<ProofRelief>() else { return };
    let id = *proof.id.get_or_insert_with(|| models.add_glb("models/relief-tile.glb", GLB));
    models.draw(ModelInstance::new(id, AT));
    let t = world.try_res::<Time>().map_or(0.0, |t| t.sim_seconds);
    if let Some(mut rw) = world.try_res_mut::<RenderWorld>() {
        rw.camera = if t < GRAZING_AT {
            CAMERA
        } else if t < WALL_AT {
            GRAZING
        } else {
            WALL
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::overlay::ErrorLog;
    use evani_core::slots::ShaderSlots;

    /// The fixture prepares with tangents, its relief read as documented,
    /// the toon look's patterns are installed, and the tile is drawn with
    /// the camera held on it.
    #[test]
    fn relief_tile_draws_with_the_toon_patterns() {
        let g = evani_gltf::Gltf::from_glb(GLB).unwrap();
        let p = evani_render::model::prepare(&g).unwrap();
        assert!(p.parts.iter().all(|x| x.tangents));
        let plate = &p.materials[0].relief;
        assert_eq!(plate.words[..2], [1.0, 0.03], "plate: height map on, depth 0.03");
        assert_eq!(plate.words[2], 4.0, "plate: knurl masked by the base alpha");
        assert_eq!(plate.words[4..7], [1.0, 0.025, 0.6]);
        assert!(plate.height.is_some_and(|i| !p.textures[i].srgb), "the height map is linear");
        let panel = &p.materials[1].relief;
        assert_eq!((panel.words[0], panel.height, panel.words[4]), (0.0, None, 2.0), "panel: ribs, no height map");
        assert!((panel.words[7] - core::f32::consts::FRAC_PI_2).abs() < 1e-6, "angle in degrees in the file");
        let ids: Vec<f32> = p.materials.iter().map(|m| m.relief.words[4]).collect();
        assert_eq!(ids, [1.0, 2.0, 3.0, 4.0], "every pattern (knurl, ribs, dots, grain) shows on the tile");

        let mut app = crate::build_app();
        app.run_headless(3, 1000.0 / 60.0);
        assert_eq!(app.world.res::<ErrorLog>().error_count(), 0);
        let slots = app.world.res::<ShaderSlots>();
        let def = slots.get(slots.id("micro_detail").unwrap()).unwrap();
        assert_eq!(def.owner, "look.toon");
        assert!(app.world.res::<ProofRelief>().id.is_some());
        assert_eq!(app.world.res::<RenderWorld>().camera.eye, CAMERA.eye);
    }
}
