//! The characters' scene (feature `material-characters`): the bust
//! (`models/char-head.glb`, made by `tools/blender/bust_build.py`: an adult
//! head with lidded eyes and hair pulled back into a bun, shoulders in a
//! round-necked top on a turned stone socle, critic round 5) with soft
//! skin, wet eyes and hair, lit by the film look's lit surface pass,
//! standing on a low stone base on the floor, so the camera holds the whole
//! base and bust clear of the HUD at every page size (critic rounds 4 and 5:
//! the taller pedestals ran under the bottom bar). The file streams in through the manifest
//! (`models.handle("char-head")`) the first time the head scene opens; a pale
//! ball stands in until it is ready.

use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::overlay::ErrorLog;
use evani_core::plugin::PluginContext;
use evani_render::model::{ModelHandle, ModelInstance, Models};
use evani_render::{MeshId, RenderWorld};

use super::common::{material, mesh, showing};

/// The head's model id (`assets/models/char-head.glb`).
pub const MODEL: &str = "char-head";
/// How far the bust's socle foot (the model's lowest point) is under the
/// head's centre (`SOCLE_FOOT` in tools/blender/bust_build.py).
pub const SOCLE_DROP: f32 = 0.428;
/// The top of the plinth the bust's socle stands on.
pub const PLINTH_TOP: f32 = 0.06;
/// Where the head's centre is (facing +z): the socle stands on the plinth.
pub const AT: [f32; 3] = [0.0, PLINTH_TOP + SOCLE_DROP, 0.0];
/// The scenes this family draws.
pub const SCENES: [&str; 1] = ["head"];

/// The scene's state.
#[derive(Debug, Default)]
pub struct HeadScene {
    pub handle: Option<ModelHandle>,
    /// The model drawn this frame (none while it streams in).
    pub drawn: Option<evani_render::model::ModelId>,
    logged: bool,
}

pub fn install(ctx: &mut PluginContext) {
    ctx.resource(HeadScene::default());
    ctx.system(SystemDesc::new("material-view.scene.characters", Stage::Frame, run));
}

/// The plinth: a low square stone base, a foot, a band and a cap, 6 cm tall
/// (critic round 5: on the 20 cm plinth the whole bust could not stand
/// clear of the bottom bar in a close framing, so the plinth ran under it).
/// Each part is `(centre height, width, height)`, square in plan; the parts
/// stack with no gap from the floor, and the last part's top is
/// [`PLINTH_TOP`].
pub const PEDESTAL: [(f32, f32, f32); 3] = [
    (0.01, 0.30, 0.02),
    (0.0325, 0.26, 0.025),
    (0.0525, 0.28, 0.015),
];

fn run(world: &mut World) {
    let on = showing(world, &SCENES);
    let (stone, ground) = (material(world, "stone"), material(world, "ground"));
    let (blob_only, sun) = (super::common::blob_only(world), super::common::sun(world));
    let Some(mut st) = world.try_res_mut::<HeadScene>() else { return };
    st.drawn = None;
    if !on {
        return;
    }
    {
        let Some(mut rw) = world.try_res_mut::<RenderWorld>() else { return };
        let pale = [0.694, 0.644, 0.558, 1.0];
        rw.draw_mesh(super::common::floor(ground, super::common::FLOOR_SAND));
        for (y, w, h) in PEDESTAL {
            rw.draw_mesh(mesh(MeshId::BEVEL_CUBE, [0.0, y, 0.0], [w, h, w], stone, pale));
        }
        // Contact shadows: the pedestal's foot on the floor, the socle on
        // the pedestal's top.
        if blob_only {
            // No shadow map (Lite, Low): the bust's own shadow where Ultra's
            // map throws it, so Low matches its direction and weight
            // (critic round 13: one disc swept from the foot read as a
            // halo). The plinth casts short, the socle its band of ovals; the
            // shoulders (about 0.34 m across, 0.31 m up) and the head (0.21
            // m across, 0.55 m up) throw theirs away from the sun, overlapping
            // into one shadow as Ultra's.
            super::common::contact(&mut rw, [0.0, 0.0, 0.0], 0.24, 0.1);
            // The socle up to the shoulders, so the ovals join the plinth's.
            for h in [0.1f32, 0.17, 0.24] {
                super::common::body_shadow(&mut rw, [0.0, h, 0.0], 0.12, sun);
            }
            super::common::body_shadow(&mut rw, [0.0, 0.31, 0.0], 0.17, sun);
            super::common::body_shadow(&mut rw, [0.0, 0.55, 0.0], 0.11, sun);
        } else {
            super::common::contact(&mut rw, [0.0, 0.0, 0.0], 0.3, AT[1] + 0.25);
        }
        super::common::contact(&mut rw, [0.0, PLINTH_TOP, 0.0], 0.12, 0.0);
    }
    let Some(mut models) = world.try_res_mut::<Models>() else { return };
    let h = *st.handle.get_or_insert_with(|| models.handle(MODEL));
    super::native_stream(world, &mut models);
    if let Some(id) = models.resolve(h) {
        models.draw(ModelInstance::new(id, AT));
        st.drawn = Some(id);
        return;
    }
    if models.handle_failed(h) && !st.logged {
        st.logged = true;
        world.res_mut::<ErrorLog>().error(format!("material-view: head: {MODEL} could not be loaded"));
    }
    drop((models, st));
    // While the head streams in: a pale ball its size.
    world.res_mut::<RenderWorld>().draw_mesh(mesh(MeshId::SPHERE, AT, [0.22, 0.28, 0.24], stone, [0.7, 0.7, 0.7, 1.0]));
}

#[cfg(test)]
mod plinth_tests {
    use super::*;

    /// The plinth's parts stack from the floor with no gap or overlap, and
    /// its top is where the bust's socle stands.
    #[test]
    fn the_plinth_stacks_from_the_floor_to_the_socle() {
        let mut top = 0.0f32;
        for (y, _, h) in PEDESTAL {
            assert!((y - 0.5 * h - top).abs() < 1e-4, "part at {y} starts at {} not {top}", y - 0.5 * h);
            top = y + 0.5 * h;
        }
        assert!((top - PLINTH_TOP).abs() < 1e-4, "{top}");
        assert!((AT[1] - SOCLE_DROP - PLINTH_TOP).abs() < 1e-6);
    }
}

#[cfg(test)]
mod close_up_tests {
    use super::*;

    /// The world positions of every vertex of the head model's parts whose
    /// material is named `material`.
    fn points(material: &str) -> Vec<[f32; 3]> {
        let f = format!("{}/assets/models/{MODEL}.glb", env!("CARGO_MANIFEST_DIR"));
        let g = evani_gltf::Gltf::from_glb(&std::fs::read(&f).expect("the head model")).expect("glb");
        let world = g.world_matrices();
        let mut out = Vec::new();
        for (n, node) in g.nodes.iter().enumerate() {
            let Some(m) = node.mesh else { continue };
            for p in &g.meshes[m].primitives {
                if p.material.map(|i| g.materials[i].name.as_str()) != Some(material) {
                    continue;
                }
                let pos = g.read_f32(p.attribute("POSITION").expect("positions")).expect("read");
                for c in pos.as_chunks::<3>().0 {
                    let q = evani_gltf::transform_point(&world[n], *c);
                    out.push([q[0] + AT[0], q[1] + AT[1], q[2] + AT[2]]);
                }
            }
        }
        assert!(!out.is_empty(), "no {material} part");
        out
    }

    /// Guard (critic round 6: the head moment showed the whole bust, the face
    /// about 170 px tall of 1080): the head scene's close-up draws the head
    /// from the crown of the hair to the lips over at least 45 % of a
    /// 1920 x 1080 view's height, with the hair and lips inside the view.
    #[test]
    fn the_head_close_up_fills_the_frame_with_the_face() {
        use evani_kit_material_view::library::{LIBRARY_JSON, Library};
        let data: Library = serde_json::from_str(LIBRARY_JSON).expect("library");
        let scene = data.scenes.iter().find(|s| s.id == "head").expect("head scene");
        let close = scene.close.expect("the head scene has a close-up");
        let cam = evani_kit_material_view::orbit::Orbit::from_data(&close, None).pose(0.0);
        let (w, h) = (1920.0f32, 1080.0f32);
        let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
        let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let norm = |a: [f32; 3]| {
            let l = dot(a, a).sqrt();
            [a[0] / l, a[1] / l, a[2] / l]
        };
        let fwd = norm(sub(cam.target, cam.eye));
        let right = norm([fwd[1] * cam.up[2] - fwd[2] * cam.up[1], fwd[2] * cam.up[0] - fwd[0] * cam.up[2], fwd[0] * cam.up[1] - fwd[1] * cam.up[0]]);
        let up = [right[1] * fwd[2] - right[2] * fwd[1], right[2] * fwd[0] - right[0] * fwd[2], right[0] * fwd[1] - right[1] * fwd[0]];
        let tan = (cam.fov_y_deg.to_radians() * 0.5).tan();
        let px = |p: [f32; 3]| {
            let d = sub(p, cam.eye);
            let z = dot(d, fwd);
            [w * 0.5 + dot(d, right) / (z * tan) * h * 0.5, h * 0.5 - dot(d, up) / (z * tan) * h * 0.5]
        };
        let hair: Vec<[f32; 2]> = points("hair").into_iter().map(px).collect();
        let lips: Vec<[f32; 2]> = points("lips").into_iter().map(px).collect();
        let crown = hair.iter().map(|p| p[1]).fold(f32::MAX, f32::min);
        let mouth = lips.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
        assert!(mouth - crown >= 0.45 * h, "crown to lips {:.0} px of {h}: not a close-up", mouth - crown);
        for p in hair.iter().chain(&lips) {
            assert!(p[0] >= 0.0 && p[0] <= w && p[1] >= 0.0 && p[1] <= h, "the head runs out of the view at {p:?}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::params::ParamRegistry;
    use evani_kit_material_view::{Command, Viewer};

    /// The head loads and draws with no error in its scene only, and its
    /// materials are the three character kinds (the hair's volume, the top
    /// and the socle are plain; the lips are a second skin).
    #[test]
    fn the_head_draws_in_its_scene() {
        let mut app = crate::build_app();
        app.world.res_mut::<Viewer>().push(Command::Scene("head".into()));
        app.run_headless(4, 1000.0 / 60.0);
        assert_eq!(app.world.res::<ErrorLog>().error_count(), 0, "{:?}", app.world.res::<ErrorLog>());
        let st = app.world.res::<HeadScene>();
        let id = st.drawn.expect("the head is drawn");
        drop(st);
        assert!(app.world.res::<Models>().ready(id));
        let f = format!("{}/assets/models/{MODEL}.glb", env!("CARGO_MANIFEST_DIR"));
        let g = evani_gltf::Gltf::from_glb(&std::fs::read(&f).unwrap()).unwrap();
        let kinds: Vec<_> = g.materials.iter().map(|m| m.extras.get("evani_kind").and_then(|k| k.as_str()).unwrap_or("")).collect();
        assert_eq!(kinds, ["skin", "eye", "hair", "", "", "", "skin"]);
        // The lips are their own skin part, a rose the face is not.
        let names: Vec<_> = g.materials.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names.last(), Some(&"lips"));
        let reg = app.world.res::<ParamRegistry>();
        assert!(reg.f32_handle("material.character.enabled").is_some() && reg.f32_handle("material.character.quality").is_some());
        drop(reg);
        if let Some(other) = crate::FAMILIES.iter().find(|f| **f != "characters") {
            let data = app.world.res::<evani_kit_material_view::ViewerData>().0.clone();
            let scene = data.library.scenes.iter().find(|s| s.family == *other).map(|s| s.id.clone()).unwrap();
            app.world.res_mut::<Viewer>().push(Command::Scene(scene));
            app.run_headless(2, 1000.0 / 60.0);
            assert!(app.world.res::<HeadScene>().drawn.is_none());
        }
    }
}
