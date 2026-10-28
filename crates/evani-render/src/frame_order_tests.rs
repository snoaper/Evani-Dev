//! Frame order guard (docs/SPEC-M1.md "Frame order"): whatever a module
//! sets in its frame step reaches the frame drawn the same frame, whatever
//! order the game added the modules in. The render module is added first
//! (the others add passes to its graph), so before the fix it drew before
//! every later module's frame step and their values (the ocean's camera,
//! fog, a module's draws) reached the GPU one frame late.

use evani_core::App;
use evani_core::ecs::World;
use evani_core::overlay::RenderStats;
use evani_core::plugin::EngineModule;

use crate::{MeshId, MeshInstance, RenderModule, RenderWorld};

/// Added after the render module: on frame `on` it adds one cube.
struct LateDrawer {
    on: u64,
    frame: u64,
}

impl EngineModule for LateDrawer {
    fn id(&self) -> &'static str {
        "evani.module.test_late_drawer"
    }
    fn label(&self) -> &'static str {
        "late"
    }
    fn frame(&mut self, world: &mut World) {
        self.frame += 1;
        if self.frame == self.on {
            world
                .res_mut::<RenderWorld>()
                .draw_mesh(MeshInstance::new(MeshId::CUBE, [0.0; 3]));
        }
    }
}

#[test]
fn a_later_modules_draws_land_in_the_same_frame() {
    let mut app = App::new("t")
        .with_module(RenderModule::new())
        .with_module(LateDrawer { on: 3, frame: 0 });
    let mut drawn = Vec::new();
    for _ in 0..5 {
        app.run_headless(1, 16.0);
        drawn.push(app.world.res::<RenderStats>().draw_calls);
    }
    // Frame 3 draws the cube that frame, not frame 4.
    assert_eq!(drawn, [0, 0, 1, 0, 0], "draw calls per frame");
    assert!(app.world.res::<RenderWorld>().meshes.is_empty(), "the draw still clears");
}
