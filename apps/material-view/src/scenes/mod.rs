//! The viewer's scenes, one module per material family, each compiled in
//! only when evani.toml lists the family (`[modules] materials`):
//!
//! * [`styles`]: the art styles' starter materials on a test shape (sphere,
//!   monkey, cube, teapot...), all in a row or the selected one alone;
//! * [`layers`]: the shader ball (clear coat, soap film, brushed steel) and
//!   the cushion (velvet, coarse weave);
//! * [`characters`]: the head bust (skin, wet eyes, hair and fur);
//! * [`nature`]: the pool (water, glass, leaves and grass) and the lamp
//!   corner (lamp glow, thin back-light), one small garden;
//! * [`playground`]: the ground patch (brick, cobble, tiles, planks, rain,
//!   puddles and moss) under the procedural sky.
//!
//! Every scene stands at the origin and draws only while the viewer shows
//! it (`Viewer.scene`); none sets the camera (the kit's orbit does). Scenes
//! are render-only: nothing flows back into the simulation.

#[cfg(feature = "material-characters")]
pub mod characters;
#[cfg(feature = "material-catalogue")]
pub mod catalogue;
#[cfg(feature = "material-styles")]
pub mod styles;
#[cfg(feature = "material-layers")]
pub mod layers;
#[cfg(feature = "material-nature")]
pub mod nature;
#[cfg(feature = "material-playground")]
pub mod playground;

use evani_core::plugin::PluginContext;

/// Add every compiled family's scene systems (after the kit's, so the camera
/// is placed before a scene draws round it).
pub fn install(ctx: &mut PluginContext) {
    #[cfg(all(not(target_arch = "wasm32"), any(feature = "material-styles", feature = "material-characters")))]
    ctx.resource(NativeFeed::default());
    #[cfg(feature = "material-styles")]
    ctx.system(evani_core::ecs::SystemDesc::new("material-view.scene.fit", evani_core::ecs::Stage::Frame, fit::system));
    #[cfg(feature = "material-styles")]
    ctx.resource(fit::SubjectFit::default());
    #[cfg(feature = "material-styles")]
    styles::install(ctx);
    #[cfg(feature = "material-catalogue")]
    catalogue::install(ctx);
    #[cfg(feature = "material-layers")]
    layers::install(ctx);
    #[cfg(feature = "material-characters")]
    characters::install(ctx);
    #[cfg(feature = "material-nature")]
    nature::install(ctx);
    #[cfg(feature = "material-playground")]
    playground::install(ctx);
    ctx.resource(StandIns::default());
    ctx.system(evani_core::ecs::SystemDesc::new("material-view.scene.stand_ins", evani_core::ecs::Stage::Frame, StandIns::report));
    ctx.system(evani_core::ecs::SystemDesc::new("material-view.scene.blob_strength", evani_core::ecs::Stage::Frame, full_blob_shadows));
}

/// Where there is no shadow map (Lite and Low) the scenes' cast blobs stand
/// in for it, so they block the sun fully, as Ultra's shadow map does: at
/// the engine's default 0.7 the bench and pillar cast faint smudges (critic
/// round 9; rounds 7 and 8 had judged Low frames still drawn with Ultra's
/// shadow map just after the tier switch). Tiers with a shadow map ignore it.
pub const BLOB_STRENGTH: f32 = 1.0;

fn full_blob_shadows(world: &mut evani_core::ecs::World) {
    if let Some(mut rw) = world.try_res_mut::<evani_render::RenderWorld>()
        && rw.blob_strength != BLOB_STRENGTH
    {
        rw.blob_strength = BLOB_STRENGTH;
    }
}

/// How many subjects the scenes drew as their plain stand-in this frame
/// (a styled or catalogue model still building). The page shows it as
/// `<html data-evani-building="n">`, which the review's captures wait for as
/// they wait for `data-evani-preparing` (critic round 7: a Low proof was
/// taken while three of its five spheres were still stand-ins, because only
/// the GPU pipeline builds were counted).
#[derive(Debug, Default)]
pub struct StandIns {
    /// Counted since the last report.
    pub n: u32,
    /// The value the page shows (`None` before the first report).
    pub shown: Option<u32>,
}

impl StandIns {
    /// Count `k` stand-ins drawn this frame.
    pub fn add(world: &evani_core::ecs::World, k: usize) {
        if k > 0
            && let Some(mut s) = world.try_res_mut::<StandIns>()
        {
            s.n += u32::try_from(k).unwrap_or(u32::MAX);
        }
    }

    /// Once a frame: show the count on the page (when it changed) and start
    /// counting again. Read-and-reset, so the systems' order does not matter
    /// (a count is at most a frame late).
    pub fn report(world: &mut evani_core::ecs::World) {
        let Some(mut s) = world.try_res_mut::<StandIns>() else { return };
        let n = std::mem::take(&mut s.n);
        if s.shown == Some(n) {
            return;
        }
        s.shown = Some(n);
        #[cfg(target_arch = "wasm32")]
        if let Some(el) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.document_element()) {
            let _ = el.set_attribute("data-evani-building", &n.to_string());
        }
    }
}

/// Native runs only: the manifest has been given to [`Models`].
#[cfg(all(not(target_arch = "wasm32"), any(feature = "material-styles", feature = "material-characters")))]
#[derive(Debug, Default)]
pub struct NativeFeed(bool);

/// Native runs (tests, the headless check): give the manifest once and read
/// the requested model files from the game's own folder. The browser
/// streams them itself, so this does nothing there.
#[cfg(any(feature = "material-styles", feature = "material-characters"))]
pub fn native_stream(world: &evani_core::ecs::World, models: &mut evani_render::model::Models) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use evani_render::model::stream;
        const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/models/");
        if let Some(mut fed) = world.try_res_mut::<NativeFeed>()
            && !fed.0
        {
            fed.0 = true;
            let m = std::fs::read_to_string(format!("{DIR}stream.json"))
                .map_err(|e| e.to_string())
                .and_then(|t| stream::manifest_from_text(&t));
            models.set_manifest(m.ok());
        }
        for (id, url) in models.take_requests() {
            let file = url.strip_prefix(stream::FOLDER).unwrap_or(&url).to_string();
            match std::fs::read(format!("{DIR}{file}")) {
                Ok(b) => models.deliver(id, Ok(&b)),
                Err(e) => models.deliver(id, Err(format!("{file}: {e}"))),
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (world, models);
}

/// Helpers the scenes share.
#[cfg(any(
    feature = "material-styles",
    feature = "material-layers",
    feature = "material-characters",
    feature = "material-nature",
    feature = "material-playground"
))]
pub mod common {
    use evani_core::ecs::World;
    use evani_core::plugin::LookData;
    use evani_kit_material_view::Viewer;
    use evani_render::{MeshId, MeshInstance, RenderWorld};

    /// The viewer shows one of `scenes` now.
    pub fn showing(world: &World, scenes: &[&str]) -> bool {
        world.try_res::<Viewer>().is_some_and(|v| scenes.contains(&v.scene.as_str()))
    }

    /// A look material's slot (0 when the look lacks it).
    pub fn material(world: &World, name: &str) -> u32 {
        world.try_res::<LookData>().map_or(0, |l| RenderWorld::material(&l, name))
    }

    /// The stage floor's width (metres): far past the fog, so no scripted
    /// or orbit camera sees the world end (critic round 1: the 30 m floor's
    /// edge showed as sky under the horizon).
    pub const FLOOR_SIZE: f32 = 4000.0;
    /// The studio floor, warm sand (#bfa58a, target look bible).
    pub const FLOOR_SAND: [f32; 4] = [0.521, 0.376, 0.254, 1.0];
    /// The garden's lawn: drawn as #6aa84f under the studio light (critic
    /// round 3: the old tint, #6aa84f's own value, came out lime once lit).
    pub const FLOOR_LAWN: [f32; 4] = [0.055, 0.155, 0.031, 1.0];

    /// The stage floor every scene stands on.
    pub fn floor(ground: u32, colour: [f32; 4]) -> MeshInstance {
        mesh(MeshId::PLANE, [0.0, 0.0, 0.0], [FLOOR_SIZE, 1.0, FLOOR_SIZE], ground, colour)
    }

    /// A contact shadow: a soft dark disc of `radius` metres on the upward
    /// faces at `ground` (evani-render's blob shadows). The renderer draws
    /// them only where there is no shadow map (Lite and Low), so every
    /// subject is grounded on every tier (critic round 2: on Low nothing
    /// had a shadow and everything floated) at no cost on the others.
    /// `height` is the subject's height above `ground`: without a shadow
    /// map the disc also casts away from the sun as far as the subject's
    /// shadow would reach (`RenderWorld::draw_blob_cast`; critic round 6:
    /// the Low bench, pillar and easel cast nothing a viewer could see,
    /// their discs hidden under them), so Low keeps Ultra's shadows.
    pub fn contact(rw: &mut RenderWorld, ground: [f32; 3], radius: f32, height: f32) {
        rw.draw_blob_cast(ground, radius, height);
    }

    /// The active tier has no shadow map (Lite, Low): the blob discs are the
    /// only shadows there.
    pub fn blob_only(world: &World) -> bool {
        world.try_res::<evani_core::tiers::Tiers>().is_none_or(|t| {
            let a = t.active();
            a.shadow_map.is_none() || !a.effects_on.iter().any(|e| e == "shadows")
        })
    }

    /// The direction towards the sun (unit length).
    pub fn sun(world: &World) -> [f32; 3] {
        use evani_core::params::{ParamRegistry, ParamValue};
        let d = world.try_res::<ParamRegistry>().and_then(|reg| match reg.id("render.light.direction").and_then(|id| reg.value(id)) {
            Some(ParamValue::Vec3(d)) => Some(d),
            _ => None,
        });
        let d = d.unwrap_or([0.45, 0.8, 0.3]);
        let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-4);
        d.map(|v| v / l)
    }

    /// The shadow a round body of radius `r` centred at `c` throws on the
    /// floor (y 0) from the sun `sun`, as soft discs where Ultra's shadow
    /// map draws it: an ellipse about the centre's shadow, `r` across and
    /// `r / sin(elevation)` along the sun, three discs down its long axis
    /// (a blob is dark to 0.45 of its radius and gone at its rim, so at 1.3
    /// `r` its half-dark edge lies on the ellipse's). Material-view critic
    /// round 13: the Low bust's one swept disc read as a halo and the Low
    /// spheres' shadows were fainter than Ultra's.
    pub fn body_shadow(rw: &mut RenderWorld, c: [f32; 3], r: f32, sun: [f32; 3]) {
        let flat = (sun[0] * sun[0] + sun[2] * sun[2]).sqrt();
        let up = sun[1].max(0.3);
        let away = if flat > 1e-3 { [-sun[0] / flat, -sun[2] / flat] } else { [0.0, 0.0] };
        let reach = c[1].max(0.0) * flat / up;
        let g = [c[0] + away[0] * reach, c[2] + away[1] * reach];
        let long = (r / up - r).max(0.0) * 0.7;
        for (t, k) in [(-long, 1.1f32), (0.0, 1.3), (long, 1.1)] {
            rw.draw_blob_cast([g[0] + away[0] * t, 0.0, g[1] + away[1] * t], k * r, 0.0);
        }
    }

    /// A primitive mesh.
    pub fn mesh(id: MeshId, at: [f32; 3], scale: [f32; 3], material: u32, color: [f32; 4]) -> MeshInstance {
        let mut m = MeshInstance::new(id, at);
        m.scale = scale;
        m.material = material;
        m.color = color;
        m
    }
}

/// Every scene but the art styles (which lays its shapes out itself) keeps
/// its subject in the part of the view the page's bars and panels leave
/// open (critic round 2: on phones the HUD covered or cropped the subject).
/// After the kit has placed the camera, the camera slides so the target
/// sits in the middle of that part, and, when the home view would draw the
/// subject (a sphere of the orbit's `fit` radius) bigger than it, it moves
/// back by the same factor; the user's own zoom stays relative to that.
#[cfg(feature = "material-styles")]
pub mod fit {
    use evani_core::camera::CameraPose;
    use evani_core::ecs::World;
    use evani_kit_material_view::{Viewer, ViewerData};
    use evani_render::RenderWorld;

    use super::styles::FreeArea;

    /// The share of the open part's smaller side the subject may fill.
    pub const FILL: f32 = 0.9;

    /// A subject size (half height, half width, m) that wins over the
    /// viewer material's own `fit`: the Procedural tab's library assets share
    /// one viewer material, so each sets its own (proc_view) and clears it
    /// when it leaves.
    #[derive(Debug, Clone, Copy, Default, PartialEq)]
    pub struct SubjectFit(pub Option<(f32, f32)>);
    /// The share of an upright view's open part the scene's prop box may fill.
    pub const BOX_FILL: f32 = 0.94;

    /// Where `p` draws on a `view`-sized canvas (CSS px) from `cam`, or None
    /// behind it.
    pub fn project(cam: &CameraPose, view: [f32; 2], p: [f32; 3]) -> Option<[f32; 2]> {
        let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
        let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let unit = |a: [f32; 3]| {
            let l = dot(a, a).sqrt().max(1e-6);
            a.map(|x| x / l)
        };
        let fwd = unit(sub(cam.target, cam.eye));
        let right = unit([fwd[1] * cam.up[2] - fwd[2] * cam.up[1], fwd[2] * cam.up[0] - fwd[0] * cam.up[2], fwd[0] * cam.up[1] - fwd[1] * cam.up[0]]);
        let up = [right[1] * fwd[2] - right[2] * fwd[1], right[2] * fwd[0] - right[0] * fwd[2], right[0] * fwd[1] - right[1] * fwd[0]];
        let d = sub(p, cam.eye);
        let z = dot(d, fwd);
        if z <= 1e-3 {
            return None;
        }
        let k = (cam.fov_y_deg.to_radians() * 0.5).tan() * z;
        Some([view[0] * 0.5 + dot(d, right) / k * view[1] * 0.5, view[1] * 0.5 - dot(d, up) / k * view[1] * 0.5])
    }

    /// The eight corners of a `[min, max]` box.
    pub fn corners(b: [[f32; 3]; 2]) -> [[f32; 3]; 8] {
        std::array::from_fn(|i| [b[i & 1][0], b[(i >> 1) & 1][1], b[(i >> 2) & 1][2]])
    }

    /// Every corner of `bounds` draws inside `free`, shrunk to [`BOX_FILL`]
    /// round its middle.
    pub fn box_inside(cam: &CameraPose, view: [f32; 2], free: [f32; 4], bounds: [[f32; 3]; 2]) -> bool {
        let (cx, cy) = ((free[0] + free[2]) * 0.5, (free[1] + free[3]) * 0.5);
        let (hw, hh) = ((free[2] - free[0]) * 0.5 * BOX_FILL, (free[3] - free[1]) * 0.5 * BOX_FILL);
        corners(bounds).iter().all(|c| project(cam, view, *c).is_some_and(|p| (p[0] - cx).abs() <= hw && (p[1] - cy).abs() <= hh))
    }

    /// [`fitted`], then, when the view is taller than wide (a phone or tablet
    /// held upright), pulled back until the scene's prop box `bounds` fits
    /// its open part too (critic round 6). A wider view (a desktop, a phone
    /// held sideways) keeps [`fitted`]'s framing exactly.
    pub fn fitted_box(cam: &CameraPose, view: [f32; 2], free: [f32; 4], radius: Option<(f32, f32)>, home_dist: f32, bounds: Option<[[f32; 3]; 2]>) -> CameraPose {
        let mut out = fitted(cam, view, free, radius, home_dist);
        let Some(b) = bounds else { return out };
        if view[1] <= view[0] {
            return out;
        }
        let back = |c: &CameraPose| [c.eye[0] - c.target[0], c.eye[1] - c.target[1], c.eye[2] - c.target[2]];
        let mut k = 1.0f32;
        while !box_inside(&out, view, free, b) && k < 6.0 {
            k *= 1.04;
            let mut c = *cam;
            let v = back(cam);
            c.eye = std::array::from_fn(|i| cam.target[i] + v[i] * k);
            out = fitted(&c, view, free, radius, home_dist);
        }
        out
    }

    /// `cam` moved so `cam.target` draws at the middle of `free` (CSS px
    /// `[left, top, right, bottom]` of a `view`-sized canvas) and a subject
    /// `radius` metres high and `radius.1` wide round it (both half sizes)
    /// fits, judged at the home distance `home_dist`.
    pub fn fitted(cam: &CameraPose, view: [f32; 2], free: [f32; 4], radius: Option<(f32, f32)>, home_dist: f32) -> CameraPose {
        let (vw, vh) = (view[0].max(1.0), view[1].max(1.0));
        let (fw, fh) = ((free[2] - free[0]).max(1.0), (free[3] - free[1]).max(1.0));
        let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
        let len = |v: [f32; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        let back = sub(cam.eye, cam.target);
        let d = len(back).max(1e-4);
        let tan = (cam.fov_y_deg.to_radians() * 0.5).tan();
        // Pull back when the home view's subject overflows the open part.
        let k = radius.map_or(1.0, |(rh, rw)| {
            let px = |r: f32| r / (home_dist.max(1e-3) * tan) * (vh * 0.5);
            (px(rh) / (0.5 * fh * FILL)).max(px(rw) / (0.5 * fw * FILL)).max(1.0)
        });
        let d2 = d * k;
        let fwd = back.map(|x| -x / d);
        let r = [fwd[1] * cam.up[2] - fwd[2] * cam.up[1], fwd[2] * cam.up[0] - fwd[0] * cam.up[2], fwd[0] * cam.up[1] - fwd[1] * cam.up[0]];
        let rl = len(r).max(1e-6);
        let right = r.map(|x| x / rl);
        let up = [right[1] * fwd[2] - right[2] * fwd[1], right[2] * fwd[0] - right[0] * fwd[2], right[0] * fwd[1] - right[1] * fwd[0]];
        // Metres per CSS px at the target's depth.
        let m = 2.0 * d2 * tan / vh;
        let (ox, oy) = ((free[0] + free[2]) * 0.5 - vw * 0.5, (free[1] + free[3]) * 0.5 - vh * 0.5);
        let shift: [f32; 3] = std::array::from_fn(|i| -right[i] * ox * m + up[i] * oy * m);
        let target: [f32; 3] = std::array::from_fn(|i| cam.target[i] + shift[i]);
        let eye: [f32; 3] = std::array::from_fn(|i| target[i] - fwd[i] * d2);
        CameraPose { eye, target, up: cam.up, fov_y_deg: cam.fov_y_deg }
    }

    pub fn system(world: &mut World) {
        let Some((scene, material, home_dist)) = world.try_res::<Viewer>().map(|v| (v.scene.clone(), v.material.clone(), v.home.dist)) else { return };
        if scene == "styles" || scene.is_empty() {
            return;
        }
        // A capture's own camera (geometry and world-edge renders) is kept.
        if world.try_res::<evani_core::capture::CaptureQueue>().is_some_and(|q| q.camera_override.is_some()) {
            return;
        }
        let Some(area) = world.try_res::<FreeArea>().map(|a| *a).filter(|a| a.view[0] > 0.0 && a.view[1] > 0.0) else { return };
        let orbit = world.try_res::<ViewerData>().and_then(|d| {
            let own = d.0.material(&material).and_then(|(_, m)| m.orbit);
            own.or_else(|| d.0.scene(&scene).map(|s| s.orbit))
        });
        let own = world.try_res::<SubjectFit>().and_then(|f| f.0);
        let radius = own.or_else(|| orbit.and_then(|o| o.fit.map(|h| (h, o.fit_width.unwrap_or(h)))));
        // The prop box only frames the scene's home view: a close-up (the
        // head moment) or the user's own zoom in is kept.
        let close = world.try_res::<Viewer>().is_some_and(|v| v.orbit.dist < v.home.dist * 0.99);
        let bounds = orbit.and_then(|o| o.bounds).filter(|_| !close);
        let Some(mut rw) = world.try_res_mut::<RenderWorld>() else { return };
        let vp = rw.viewport_css;
        let cv = if vp[0] > 0.0 && vp[1] > 0.0 { vp } else { area.view };
        let (sx, sy) = (cv[0] / area.view[0], cv[1] / area.view[1]);
        let free = [area.rect[0] * sx, area.rect[1] * sy, area.rect[2] * sx, area.rect[3] * sy];
        rw.camera = fitted_box(&rw.camera, cv, free, radius, home_dist, bounds);
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn cam() -> CameraPose {
            CameraPose { eye: [0.0, 0.5, 3.0], target: [0.0, 0.5, 0.0], up: [0.0, 1.0, 0.0], fov_y_deg: 35.0 }
        }

        /// The whole view open: nothing moves.
        #[test]
        fn an_open_view_keeps_the_camera() {
            let c = fitted(&cam(), [1600.0, 900.0], [0.0, 0.0, 1600.0, 900.0], Some((0.5, 0.5)), 3.0);
            for i in 0..3 {
                assert!((c.eye[i] - cam().eye[i]).abs() < 1e-4 && (c.target[i] - cam().target[i]).abs() < 1e-4, "{c:?}");
            }
        }

        /// Guard (critic round 6: held upright, phones cut the pool's lamp
        /// and hedge and the yard's stone ball off the edges). For every
        /// camera with a prop box: each prop the desktop view shows (a part
        /// under 4.5 m every way; floor slabs and walls are the set, not
        /// props) lies inside
        /// the box (so the data cannot go stale), the home view fitted to a phone
        /// and a tablet held upright keeps all eight corners in the open
        /// part, and a desktop view keeps the framing it had before the box.
        #[test]
        fn portrait_framing_keeps_every_prop_in_view() {
            use evani_kit_material_view::{Command, Viewer, ViewerData};
            let mut app = crate::build_app();
            // The renderer empties its draw list once a frame is drawn: a
            // system last in the frame keeps a copy.
            let snap: std::rc::Rc<std::cell::RefCell<Vec<evani_render::MeshInstance>>> = std::rc::Rc::default();
            {
                let snap = snap.clone();
                app.context("framing-test").system(
                    evani_core::ecs::SystemDesc::new("framing-test.snapshot", evani_core::ecs::Stage::Frame, move |w: &mut World| {
                        if let Some(rw) = w.try_res::<RenderWorld>() {
                            *snap.borrow_mut() = rw.meshes.clone();
                        }
                    })
                    .order(i32::MAX),
                );
            }
            let data = app.world.res::<ViewerData>().0.clone();
            let mut checked = 0;
            // Every material whose camera (its own, else its scene's) has a
            // prop box, opened as the viewer opens it.
            let cases: Vec<(String, String, evani_kit_material_view::library::OrbitData)> = data
                .library
                .families
                .iter()
                .flat_map(|f| f.materials.iter())
                .filter_map(|m| {
                    let scene = data.scene(&m.scene)?;
                    let o = m.orbit.unwrap_or(scene.orbit);
                    o.bounds.map(|_| (scene.id.clone(), m.id.clone(), o))
                })
                .collect();
            for (scene_id, material, orbit) in &cases {
                let b = orbit.bounds.expect("bounds");
                let scene = data.scene(scene_id).expect("scene");
                app.world.res_mut::<Viewer>().push(Command::Scene(scene_id.clone()));
                app.world.res_mut::<Viewer>().push(Command::Material(material.clone()));
                app.run_headless(4, 1000.0 / 60.0);
                let home = app.world.res::<Viewer>().home;
                let cam = home.pose(0.0);
                let radius = orbit.fit.map(|h| (h, orbit.fit_width.unwrap_or(h)));
                // The desktop view: the style bar, the panel on the right and
                // the bottom bar over it.
                let (dv, df) = ([1920.0, 1080.0], [0.0, 80.0, 1560.0, 900.0]);
                let desk = fitted(&cam, dv, df, radius, home.dist);
                let drawn = snap.borrow().clone();
                let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
                // Shown: drawn anywhere on the desktop page, under a panel too
                // (critic round 6: the stone ball at the panel's edge).
                let shown = |m: &&evani_render::MeshInstance| project(&desk, dv, m.position).is_some_and(|p| p[0] >= 0.0 && p[0] <= dv[0] && p[1] >= 0.0 && p[1] <= dv[1]);
                for m in drawn.iter().filter(|m| m.scale.iter().all(|s| s.abs() < 4.5)).filter(shown) {
                    // A turned part: its half diagonal every way.
                    let turned = (m.rotation[3].abs() - 1.0).abs() > 1e-4;
                    let r = 0.5 * (m.scale[0] * m.scale[0] + m.scale[1] * m.scale[1] + m.scale[2] * m.scale[2]).sqrt();
                    for i in 0..3 {
                        let h = if turned { r } else { 0.5 * m.scale[i].abs() };
                        lo[i] = lo[i].min(m.position[i] - h);
                        hi[i] = hi[i].max(m.position[i] + h);
                    }
                }
                if std::env::var_os("FRAMING_DUMP").is_some() {
                    for m in drawn.iter().filter(|m| m.scale.iter().all(|s| s.abs() < 4.5)).filter(shown) {
                        eprintln!("{material} mesh {} at {:?} scale {:?} rot {:?} px {:?}", m.mesh.0, m.position, m.scale, m.rotation, project(&desk, dv, m.position));
                    }
                }
                assert!(lo[0] < hi[0], "{}: no props drawn in the desktop view ({} meshes; first: {:?}; desk camera {:?})", scene.id, drawn.len(), drawn.iter().take(6).map(|m| (m.position, m.scale, project(&desk, dv, m.position))).collect::<Vec<_>>(), desk);
                for i in 0..3 {
                    assert!(lo[i] >= b[0][i] - 0.05 && hi[i] <= b[1][i] + 0.05, "{}: the props span {lo:?} to {hi:?}, outside the bounds {b:?}", scene.id);
                }
                // Phone (390 x 844) and tablet (820 x 1180) held upright, the
                // top cards and the bottom bar over the view.
                for (view, free) in [([390.0, 844.0], [0.0, 140.0, 390.0, 560.0]), ([820.0, 1180.0], [0.0, 150.0, 820.0, 900.0])] {
                    let c = fitted_box(&cam, view, free, radius, home.dist, Some(b));
                    assert!(box_inside(&c, view, free, b), "{}: a prop runs out of the {view:?} view", scene.id);
                }
                assert_eq!(fitted_box(&cam, dv, df, radius, home.dist, Some(b)), desk, "{}: desktop framing moved", scene.id);
                checked += 1;
            }
            assert!(checked >= 2, "cameras with a prop box: {checked}");
            // The review's pool and ground moments open these.
            for m in ["nature.water", "playground.cobble"] {
                assert!(cases.iter().any(|c| c.1 == m), "{m}'s camera has no prop box");
            }
        }

        /// A phone held upright with the bar over its lower part: the
        /// target is drawn in the open part's middle and the subject fits.
        #[test]
        fn a_narrow_open_part_pulls_back_and_centres() {
            let (view, free) = ([390.0, 844.0], [0.0, 120.0, 390.0, 600.0]);
            let c = fitted(&cam(), view, free, Some((0.5, 0.5)), 3.0);
            let d = ((c.eye[0] - c.target[0]).powi(2) + (c.eye[1] - c.target[1]).powi(2) + (c.eye[2] - c.target[2]).powi(2)).sqrt();
            let tan = (17.5f32).to_radians().tan();
            // The subject's drawn radius fits the open width.
            let px = 0.5 / (d * tan) * 422.0;
            assert!(px <= 0.5 * 390.0 * FILL + 0.5, "{px}");
            // The old target projects onto the open part's middle (360 px down).
            let fwd = [(c.target[0] - c.eye[0]) / d, (c.target[1] - c.eye[1]) / d, (c.target[2] - c.eye[2]) / d];
            let p = [0.0 - c.eye[0], 0.5 - c.eye[1], 0.0 - c.eye[2]];
            let z = p[0] * fwd[0] + p[1] * fwd[1] + p[2] * fwd[2];
            let y = p[1] - fwd[1] * z;
            let screen_y = 422.0 - y / (z * tan) * 422.0;
            assert!((screen_y - 360.0).abs() < 2.0, "{screen_y}");
        }
    }
}
