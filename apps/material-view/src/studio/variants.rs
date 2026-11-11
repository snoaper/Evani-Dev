//! WP3 (docs/studio/WORK_PACKAGES.md): the geometry stage at runtime.
//!
//! The cache and queue are `evani_style_geo::cache::Variants` (the shape
//! library, so the scene, the page and xtask share one copy); the app keeps
//! one as a world resource and runs one job a frame (a voxel or low-poly
//! horse takes about 50 to 70 ms natively, release), so a style change costs
//! one short hitch, never a stall. The scene (WP4) calls `request(job)` and
//! draws `get(key)` once it is `Some`; the page (WP6) shows `progress()`.

use evani_core::ecs::{Stage, SystemDesc};
use evani_core::plugin::PluginContext;
use evani_style_geo::cache::Variants;

/// Add the resource and the one-job-a-frame system.
pub fn install(ctx: &mut PluginContext) {
    ctx.world.insert_resource(Variants::default());
    ctx.schedule.add(SystemDesc::new("studio_variants", Stage::PreUpdate, |w| {
        w.res_mut::<Variants>().step();
    }));
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use evani_kit_material_view::studio::StudioData;
    use evani_style_geo::cache::{Job, Status, Variants};
    use evani_style_geo::{Geometry, SourceMesh};

    fn cube() -> SourceMesh {
        let h = 0.57;
        let p = [[-h, -h, -h], [h, -h, -h], [h, h, -h], [-h, h, -h], [-h, -h, h], [h, -h, h], [h, h, h], [-h, h, h]];
        let indices = vec![0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 3, 7, 6, 3, 6, 2, 0, 4, 7, 0, 7, 3, 1, 2, 6, 1, 6, 5];
        SourceMesh { positions: p.to_vec(), normals: p.to_vec(), indices, regions: vec![0; 8] }
    }

    /// A request queues once, one job runs a frame, the result comes from
    /// memory after, a broken job fails once with its reason, and the page
    /// sees the label meanwhile.
    #[test]
    fn a_shape_is_made_once_and_kept() {
        let app = crate::build_app();
        let data = app.world.res::<StudioData>().0.clone();
        let asset = data.assets.asset("shape.cube").unwrap().clone();
        let geometry = Geometry::Voxel { cell: 0.12, keep_thin: true };
        let key = evani_style_geo::GeometryKey(7);
        let job = Job { key, asset: asset.clone(), source: Rc::new(cube()), geometry: geometry.clone() };
        let mut v = Variants::default();
        assert_eq!(v.request(job.clone()), Status::Making("Making voxel cube...".into()));
        assert_eq!(v.request(job.clone()), Status::Making("Making voxel cube...".into()));
        assert_eq!(v.progress().as_deref(), Some("Making voxel cube..."));
        assert!(v.step());
        assert!(!v.step(), "queued once");
        assert!(matches!(v.request(job), Status::Ready(_)));
        assert_eq!(v.made, 1);
        assert!(v.bytes() > 0);
        let broken = Job { key: evani_style_geo::GeometryKey(8), asset, source: Rc::new(SourceMesh::default()), geometry };
        v.request(broken.clone());
        v.step();
        assert!(matches!(v.request(broken), Status::Failed(_)));
        assert_eq!(v.made, 2, "a failed job isn't retried");
    }

    /// The app installs the cache and steps it each frame: a job the scene
    /// queues is made by the next frame.
    #[test]
    fn the_app_steps_the_cache_each_frame() {
        let mut app = crate::build_app();
        let data = app.world.res::<StudioData>().0.clone();
        let asset = data.assets.asset("shape.cube").unwrap().clone();
        let geometry = Geometry::Voxel { cell: 0.12, keep_thin: true };
        let job = Job { key: evani_style_geo::GeometryKey(9), asset, source: Rc::new(cube()), geometry };
        assert!(matches!(app.world.res_mut::<Variants>().request(job.clone()), Status::Making(_)));
        app.run_headless(1, 1000.0 / 60.0);
        assert!(matches!(app.world.res_mut::<Variants>().request(job), Status::Ready(_)));
    }
}
