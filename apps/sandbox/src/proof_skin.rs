//! The GPU skinning proof (feature `skin-proof`, STAGE3_PLAN S3-9; it turns
//! on `anim`): the S3-7 rig fixture (`tests/fixtures/models/rig.src.glb`,
//! a skinned tube with three clips) drawn by the skinned vertex stages, out
//! past the spinner grid with its own camera. The files are compiled in
//! (`include_bytes!`), so the sandbox's assets and its default build stay
//! as they are. `cargo xtask new-game` does not copy this file.
//!
//! The page address picks what is drawn (same camera for each):
//! * `?skin=bind`: the skinned rig alone in its rest pose, through
//!   evani-anim (`Rig::skinning` of the rest pose, [`Models::draw_posed`]);
//! * `?skin=static`: the rig's static copy alone (`rig.static.glb`, the same
//!   tube with no skin): the bind capture must look like this one;
//! * otherwise three rigs, three clearly different bends: the bind palette
//!   ([`Models::draw`]), the `idle` clip playing with the spine and neck bent
//!   well back by hand (leaning away from the sun: a long shadow), and the
//!   `pose` clip (leaning towards the sun: a short shadow; the tube is open
//!   at both ends, so seen down its length its shadow is a ring).
//!
//! The kit's drifting screen sprites are dropped from the proof's frames, so
//! the bind and static captures differ only where the rig does.
//!
//! Render-only: the clip frame comes from the fixed-step tick, nothing flows
//! back into the simulation.

use evani_anim::math::{Quat, qmul};
use evani_anim::{Clips, Graph, NONE, Node, Rig, Skeleton, Skin, Xform, frame_at};
use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::effects::EffectsStatus;
use evani_core::overlay::ErrorLog;
use evani_core::tiers::Tiers;
use evani_render::RenderWorld;
use evani_render::model::{ModelId, ModelInstance, ModelState, Models};

const SRC: &[u8] = include_bytes!("../../../tests/fixtures/models/rig.src.glb");
const STATIC: &[u8] = include_bytes!("../../../tests/fixtures/models/rig.static.glb");
const CLIPS: &[u8] = include_bytes!("../../../tests/fixtures/models/rig.clips.evc1");

/// The rig fixture's clips.
pub const IDLE: u16 = 0;
pub const POSE: u16 = 2;
/// Where the rigs stand (beyond the grid and the +z proof figure) and
/// how big they are (the tube is 0.6 m tall at scale 1).
pub const Z: f32 = 26.0;
pub const SCALE: f32 = 2.0;
/// The panels' x positions, left to right on screen (the camera looks along
/// +z, so screen right is -x): bind, idle bent back, pose.
pub const PANELS: [f32; 3] = [1.2, 0.0, -1.2];
/// The hand-made bend on the idle clip, degrees about the rig's x axis
/// (spine, neck): the other way from the `pose` clip's spine (+20).
pub const BEND: [f32; 2] = [-35.0, -25.0];
/// The `pose` clip drops the hips 0.1 m (scale 1): that rig stands raised by
/// as much, on the ground like the others.
pub const POSE_RAISE: f32 = 0.1 * SCALE;
/// The tube is a body with no legs (0.95 m to 1.55 m up at scale 1): the
/// rigs stand lowered so it starts just above the ground, its shadow close.
pub const LIFT: f32 = 0.05 - 0.95 * SCALE;

/// What the page asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// The skinned rig in its rest pose, alone.
    Bind,
    /// The static copy, alone.
    Static,
    /// Bind palette, `pose`, `idle`.
    Panels,
}

/// The mode a page address asks for (`skin=bind`, `skin=static`).
pub fn mode_from(href: &str) -> Mode {
    if href.contains("skin=bind") {
        Mode::Bind
    } else if href.contains("skin=static") {
        Mode::Static
    } else {
        Mode::Panels
    }
}

/// The animation side of the rig.
#[derive(Debug)]
pub struct Anim {
    pub skeleton: Skeleton,
    pub skin: Skin,
    pub clips: Clips,
    pub graph: Graph,
    pub rig: Rig,
    /// The spine and neck nodes (the hand-made bend).
    pub bend_nodes: [usize; 2],
    /// Scratch: the bent pose.
    pub bent: Vec<Xform>,
}

/// The rig's skeleton, skin and clips, read from the fixture.
pub fn anim() -> Result<Anim, String> {
    let g = evani_gltf::Gltf::from_glb(SRC).map_err(|e| e.0.to_owned())?;
    let parents: Vec<u16> = (0..g.nodes.len()).map(|n| g.parent(n).map_or(NONE, |p| p as u16)).collect();
    let rest: Vec<Xform> = g.nodes.iter().map(|n| Xform { t: n.translation, r: n.rotation, s: n.scale }).collect();
    let skeleton = Skeleton::new(&parents, &rest).map_err(|e| e.0.to_owned())?;
    let s = g.skins.first().ok_or("rig: no skin")?;
    let flat = match s.inverse_bind_matrices {
        Some(a) => g.read_f32(a).map_err(|e| e.0.to_owned())?,
        None => Vec::new(),
    };
    let ibm: Vec<[f32; 16]> = flat.chunks_exact(16).filter_map(|c| c.try_into().ok()).collect();
    let joints: Vec<u16> = s.joints.iter().map(|&j| j as u16).collect();
    let skin = Skin::new(&skeleton, &joints, &ibm).map_err(|e| e.0.to_owned())?;
    let clips = Clips::decode(CLIPS).map_err(|e| e.0.to_owned())?;
    let graph = Graph::new(&skeleton, &clips, 1);
    let rig = Rig::new(&skeleton, joints.len());
    let node = |name: &str| g.nodes.iter().position(|n| n.name == name).ok_or("rig: no spine or neck node");
    let bend_nodes = [node("spine")?, node("neck")?];
    Ok(Anim { skeleton, skin, clips, graph, rig, bend_nodes, bent: Vec::new() })
}

/// The proof's state.
#[derive(Debug, Default)]
pub struct ProofSkin {
    /// The skinned rig and its static copy, once added.
    pub models: Option<(ModelId, ModelId)>,
    pub anim: Option<Anim>,
    pub mode: Option<Mode>,
    logged: bool,
}

/// The `Stage::Frame` system; registered after the scene's, so its camera
/// wins.
pub fn system() -> SystemDesc {
    SystemDesc::new("sandbox.proof_skin", Stage::Frame, draw)
}

#[cfg(target_arch = "wasm32")]
fn page_mode() -> Mode {
    // `Document::url` needs no extra web-sys feature, so the sandbox's
    // default build keeps its exact web-sys bindings.
    mode_from(&web_sys::window().and_then(|w| w.document()).and_then(|d| d.url().ok()).unwrap_or_default())
}

#[cfg(not(target_arch = "wasm32"))]
fn page_mode() -> Mode {
    Mode::Panels
}

/// A quarter turn about y: the rig's spine bends in its y-z plane, which the
/// camera (looking along +z) then sees side-on.
const TURN: [f32; 4] = [0.0, core::f32::consts::FRAC_1_SQRT_2, 0.0, core::f32::consts::FRAC_1_SQRT_2];

fn place(id: ModelId, x: f32) -> ModelInstance {
    ModelInstance::new(id, [x, LIFT, Z]).rotation(TURN).scale([SCALE; 3])
}

/// Unit quaternion for `deg` degrees about the x axis.
fn about_x(deg: f32) -> Quat {
    let (s, c) = (deg.to_radians() * 0.5).sin_cos();
    [s, 0.0, 0.0, c]
}

/// `clip` at this tick, as skinning matrices; `bend` turns the spine and
/// neck further (degrees about x, after the clip's own rotation).
fn posed<'a>(a: &'a mut Anim, clip: u16, tick: u64, sub: f32, bend: Option<[f32; 2]>) -> Option<&'a [[f32; 12]]> {
    let frames = a.clips.frames(usize::from(clip));
    let frame = frame_at((tick % (1 << 24)) as u32, sub, frames, true);
    let pose = a.graph.eval(&a.skeleton, &a.clips, &[Node::Clip { clip, frame }], &[]).ok()?;
    match bend {
        Some(deg) => {
            a.bent.clear();
            a.bent.extend_from_slice(pose);
            for (n, d) in a.bend_nodes.into_iter().zip(deg) {
                if let Some(x) = a.bent.get_mut(n) {
                    x.r = qmul(x.r, about_x(d));
                }
            }
            a.rig.set_pose(&a.bent);
        }
        None => a.rig.set_pose(pose),
    }
    a.rig.update(&a.skeleton);
    Some(a.rig.skinning(&a.skin))
}

fn draw(world: &mut World) {
    let Some(mut models) = world.try_res_mut::<Models>() else { return };
    let Some(mut proof) = world.try_res_mut::<ProofSkin>() else { return };
    let mode = *proof.mode.get_or_insert_with(page_mode);
    let (skinned, still) = *proof.models.get_or_insert_with(|| (models.add_glb("models/rig.glb", SRC), models.add_glb("models/rig.static.glb", STATIC)));
    if proof.anim.is_none() && !proof.logged {
        match anim() {
            Ok(a) => proof.anim = Some(a),
            Err(e) => {
                proof.logged = true;
                world.res_mut::<ErrorLog>().error(format!("sandbox: skin proof: {e}"));
            }
        }
    }
    for id in [skinned, still] {
        if let (Some(ModelState::Failed(e)), false) = (models.state(id), proof.logged) {
            proof.logged = true;
            world.res_mut::<ErrorLog>().error(format!("sandbox: skin proof: {}: {e}", models.url(id).unwrap_or("?")));
        }
    }
    let (tick, sub) = world.try_res::<evani_core::time::Time>().map_or((0, 0.0), |t| (t.tick, t.alpha));
    match mode {
        Mode::Static => models.draw(place(still, 0.0)),
        Mode::Bind => {
            if let Some(a) = proof.anim.as_mut() {
                // A fresh rig is the rest pose: model(joint) x inverse bind.
                let rest = a.skeleton.rest().to_vec();
                a.rig.set_pose(&rest);
                a.rig.update(&a.skeleton);
                models.draw_posed(place(skinned, 0.0), a.rig.skinning(&a.skin));
            }
        }
        Mode::Panels => {
            models.draw(place(skinned, PANELS[0]));
            if let Some(a) = proof.anim.as_mut() {
                for (clip, x, bend, raise) in [(IDLE, PANELS[1], Some(BEND), 0.0), (POSE, PANELS[2], None, POSE_RAISE)] {
                    if let Some(m) = posed(a, clip, tick, sub, bend) {
                        let mut at = place(skinned, x);
                        at.position[1] += raise;
                        models.draw_posed(at, m);
                    }
                }
            }
        }
    }
    drop(models);
    drop(proof);
    // Without a shadow map (Lite, Low) the rigs stand on shadow discs, as
    // the scene's own things do: the same grounded look on every tier.
    let discs = !(world.try_res::<Tiers>().is_some_and(|t| t.active().shadow_map.is_some()) && world.try_res::<EffectsStatus>().is_some_and(|e| e.is_on("shadows")));
    if let Some(mut rw) = world.try_res_mut::<RenderWorld>() {
        rw.camera = camera();
        // The kit's drifting screen sprites would differ between captures.
        rw.sprites.clear();
        if discs {
            let xs: &[f32] = if mode == Mode::Panels { &PANELS } else { &[0.0] };
            for &x in xs {
                rw.draw_blob([x, 0.0, Z], DISC);
            }
        }
    }
}

/// The shadow disc's radius under each rig (Lite, Low).
pub const DISC: f32 = 0.5;

/// The proof's camera: at chest height a few metres back, looking a little
/// down, so the three rigs fill the lower middle of the frame with the
/// ground and their shadows below them (1280 x 720: the tubes span about
/// the middle half of the height).
pub fn camera() -> evani_core::camera::CameraPose {
    evani_core::camera::CameraPose { eye: [0.0, 1.4, Z - 3.6], target: [0.0, 0.95, Z], up: [0.0, 1.0, 0.0], fov_y_deg: 40.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_address_picks_the_mode() {
        assert_eq!(mode_from("http://127.0.0.1:8290/?skin=bind"), Mode::Bind);
        assert_eq!(mode_from("http://127.0.0.1:8290/?skin=static"), Mode::Static);
        assert_eq!(mode_from("http://127.0.0.1:8290/"), Mode::Panels);
    }

    /// The rest pose's skinning matrices are the identity (world x inverse
    /// bind), so the `bind` capture draws the static copy's vertices.
    #[test]
    fn rest_pose_skins_to_the_identity() {
        let mut a = anim().unwrap();
        let rest = a.skeleton.rest().to_vec();
        a.rig.set_pose(&rest);
        a.rig.update(&a.skeleton);
        let id = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        for m in a.rig.skinning(&a.skin) {
            assert!(m.iter().zip(id).all(|(x, y)| (x - y).abs() < 1e-5), "{m:?}");
        }
        let pose = posed(&mut a, POSE, 0, 0.0, None).unwrap().to_vec();
        assert!(pose.iter().any(|m| m.iter().zip(id).any(|(x, y)| (x - y).abs() > 0.05)), "the pose clip moves the rig");
        // The hand bend tips the idle rig's top (the neck ring, 1.55 m up)
        // the other way along the rig's z from the pose clip's tip.
        let idle = posed(&mut a, IDLE, 0, 0.0, None).unwrap().to_vec();
        let bent = posed(&mut a, IDLE, 0, 0.0, Some(BEND)).unwrap().to_vec();
        let top_z = |m: &[[f32; 12]]| m[2][9] * 1.55 + m[2][11];
        let (b, i, p) = (top_z(&bent), top_z(&idle), top_z(&pose));
        assert!(b < i - 0.1 && p > i + 0.05, "top z: bent {b}, idle {i}, pose {p}");
    }

    /// Both files are read and drawn (three rigs), with no error.
    #[test]
    fn proof_draws_the_rigs() {
        let mut app = crate::build_app();
        app.run_headless(4, 1000.0 / 60.0);
        assert_eq!(app.world.res::<ErrorLog>().error_count(), 0, "{:?}", app.world.res::<ErrorLog>());
        let proof = app.world.res::<ProofSkin>();
        let (skinned, still) = proof.models.unwrap();
        let models = app.world.res::<Models>();
        assert!(models.ready(skinned) && models.ready(still));
        assert!(proof.anim.is_some());
        assert_eq!(app.world.res::<RenderWorld>().camera, camera());
    }
}
