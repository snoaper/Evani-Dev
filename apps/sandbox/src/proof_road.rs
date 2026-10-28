//! The pseudo-3D road proof (S6-5, sandbox feature `pseudo3d-proof`; never
//! in evani.toml, so the default and ship builds are unchanged): a closed
//! seeded track of bends and hills with roadside scenery, driven at a fixed
//! speed. The road view covers the 3D grid; a car sprite sits on top, as a
//! kit's would. Every value comes from the fixed tick and the seeded
//! `Pcg32`, so the same frame number shows the same picture everywhere.
//! Tune it live with `render.pseudo3d.*` (curve, hill, draw distance,
//! segment length, fog, camera height, field of view, road width).

use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::rng::Pcg32;
use evani_core::time::Time;
use evani_render::atlas::cell;
use evani_render::pseudo3d::{RoadParams, RoadScene, RoadSprite, RoadTrack};
use evani_render::{RenderWorld, Sprite};

/// Road distance covered per fixed tick (world units).
pub const SPEED: f32 = 60.0;
/// The scenery seed.
pub const SEED: u64 = 0x5_6005;

/// The proof track: about 500 segments of bends and hills, closed back to
/// level, with trees, bushes and kerb posts.
pub fn track() -> RoadTrack {
    let mut t = RoadTrack::new();
    t.straight(25)
        .bend(60, 3.0)
        .hill(50, 30.0)
        .bend(40, -5.0)
        .add(20, 40, 20, 4.0, -25.0)
        .straight(20)
        .hill(60, 40.0)
        .bend(80, -3.0)
        .add(10, 30, 10, 6.0, 15.0)
        .close(50);
    let mut rng = Pcg32::seeded(SEED);
    t.scatter(&mut rng, 3, &[cell::TRIANGLE, cell::CIRCLE], [0.9, 1.4], [0.10, 0.30, 0.10, 1.0]);
    t.scatter(&mut rng, 7, &[cell::DIAMOND], [0.6, 0.6], [0.55, 0.20, 0.30, 1.0]);
    for i in (0..t.len()).step_by(8) {
        for side in [-1.0, 1.0] {
            t.sprite(i, RoadSprite { cell: cell::SQUARE, offset: side * 1.1, size: [0.06, 0.25], color: [0.9, 0.9, 0.9, 1.0] });
        }
    }
    t
}

/// The `Stage::Frame` system: insert the scene once, then place the camera
/// from the tick and draw the car.
pub fn system() -> SystemDesc {
    SystemDesc::new("sandbox.proof_road", Stage::Frame, drive)
}

fn drive(world: &mut World) {
    if world.try_res::<RoadScene>().is_none() {
        world.insert_resource(RoadScene::new(track()));
    }
    let tick = world.try_res::<Time>().map_or(0, |t| t.tick);
    let seg = match (world.try_res::<RoadParams>(), world.try_res::<evani_core::params::ParamRegistry>()) {
        (Some(p), Some(reg)) => p.values(&reg).segment_length,
        _ => 200.0,
    };
    let Some(mut scene) = world.try_res_mut::<RoadScene>() else { return };
    let z = tick as f32 * SPEED;
    scene.camera.z = z;
    // The driver leans out of each bend a little, as a player would drift.
    let curve = scene.track.curve_at(z + seg * 4.0, seg);
    scene.camera.x = (-curve * 0.06).clamp(-0.5, 0.5);
    drop(scene);
    if let Some(mut rw) = world.try_res_mut::<RenderWorld>() {
        let [w, h] = rw.viewport_css;
        if w > 0.0 && h > 0.0 {
            let car = (w * 0.16).min(160.0);
            let lean = (curve * 0.03).clamp(-0.15, 0.15);
            rw.sprites.push(Sprite {
                pos: [w * 0.5, h - car * 0.45],
                size: [car, car * 0.55],
                rotation: lean,
                cell: cell::SQUARE,
                color: [0.85, 0.08, 0.06, 1.0],
                layer: 10,
            });
            rw.sprites.push(Sprite {
                pos: [w * 0.5, h - car * 0.62],
                size: [car * 0.6, car * 0.25],
                rotation: lean,
                cell: cell::SQUARE,
                color: [0.10, 0.12, 0.20, 1.0],
                layer: 11,
            });
        }
    }
}
