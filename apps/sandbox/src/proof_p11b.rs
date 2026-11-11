//! The P11b proof (`?set=p11b&row=<id>`, feature `recipes-nature-proof`; dev
//! only, never in evani.toml): one nature row at a time in a small scene under
//! a low sun behind it, so the back-light shows.
//!
//! The nature pass keeps one look table per scene, so a row cannot look unlike
//! another in a shared grid. Instead the row's own numbers are applied to the
//! live `material.nature.*` params (`gltf::nature_params`, the same mapping a
//! kit or the viewer uses) and its surface kind comes from the adapter
//! (`gltf::nature_kind`); the solid parts take the row's first colour. The
//! scene is built from the family: a basin for water, a frame and a bottle for
//! glass, a crown, a leaf spray, a grass patch or candles for the rest.
//! `?q=lite|low|medium|high|ultra` sets the water, glass and foliage groups.
//! Used by `.evani/tmp/p11` to take the pictures; a test builds every row.

use core::cell::RefCell;

use evani_core::camera::CameraPose;
use evani_core::ecs::World;
use evani_core::params::{ParamRegistry, ParamValue};
use evani_core::plugin::LookData;
use evani_material_nature::{Kind, NatureSurfaces, Surface};
use evani_material_recipes::catalog::{Catalog, Row};
use evani_material_recipes::gltf::{self, NatureValue};
use evani_render::math::quat_axis_angle;
use evani_render::{MeshId, MeshInstance, RenderWorld};

/// The sun: low, behind and to one side, so the back-light shows and the
/// front faces still catch some light.
const SUN: [f32; 3] = [0.7, 0.4, -0.7];

/// Which look material a solid part uses.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Mat {
    Accent,
    Ground,
    Glow,
}

/// One solid part: shape, centre, full size, yaw, material, linear colour.
#[derive(Clone, Copy, Debug)]
struct Part {
    mesh: MeshId,
    at: [f32; 3],
    size: [f32; 3],
    yaw: f32,
    mat: Mat,
    colour: [f32; 3],
}

/// What a row is shown as.
#[derive(Debug)]
pub struct Scene {
    parts: Vec<Part>,
    pub surfaces: Vec<Surface>,
    camera: CameraPose,
    pub params: Vec<(&'static str, NatureValue)>,
    pub kind: Kind,
}

fn part(mesh: MeshId, at: [f32; 3], size: [f32; 3], yaw: f32, mat: Mat, colour: [f32; 3]) -> Part {
    Part { mesh, at, size, yaw, mat, colour }
}

fn cam(eye: [f32; 3], target: [f32; 3], fov: f32) -> CameraPose {
    CameraPose { eye, target, up: [0.0, 1.0, 0.0], fov_y_deg: fov }
}

fn dim(c: [f32; 3], k: f32) -> [f32; 3] {
    c.map(|v| v * k)
}

/// A basin of water: a bed, four walls, a pebble.
fn basin(parts: &mut Vec<Part>, hx: f32, hz: f32, top: f32, bed: [f32; 3], walls: bool) {
    parts.push(part(MeshId::CUBE, [0.0, 0.01, 0.0], [hx * 2.0, 0.02, hz * 2.0], 0.0, Mat::Ground, bed));
    parts.push(part(MeshId::LOW_SPHERE, [hx * 0.3, 0.08, -hz * 0.2], [0.5, 0.2, 0.4], 0.4, Mat::Accent, [0.35, 0.3, 0.25]));
    parts.push(part(MeshId::LOW_SPHERE, [-hx * 0.5, 0.06, hz * 0.3], [0.4, 0.14, 0.3], 1.1, Mat::Accent, [0.3, 0.28, 0.25]));
    if !walls {
        return;
    }
    let (w, stone) = (0.15, [0.55, 0.52, 0.48]);
    for (c, s) in [
        ([0.0, top * 0.5, -hz - w * 0.5], [hx * 2.0 + w * 2.0, top, w]),
        ([0.0, top * 0.5, hz + w * 0.5], [hx * 2.0 + w * 2.0, top, w]),
        ([-hx - w * 0.5, top * 0.5, 0.0], [w, top, hz * 2.0]),
        ([hx + w * 0.5, top * 0.5, 0.0], [w, top, hz * 2.0]),
    ] {
        parts.push(part(MeshId::BEVEL_CUBE, c, s, 0.0, Mat::Accent, stone));
    }
}

/// Things to see through glass: a red block and a blue ball behind it.
fn backdrop(parts: &mut Vec<Part>, z: f32, y: f32) {
    parts.push(part(MeshId::CUBE, [-0.45, y - 0.2, z], [0.5, 0.5, 0.5], 0.3, Mat::Accent, [0.75, 0.12, 0.08]));
    parts.push(part(MeshId::SPHERE, [0.4, y - 0.15, z - 0.1], [0.6, 0.6, 0.6], 0.0, Mat::Accent, [0.1, 0.25, 0.7]));
    parts.push(part(MeshId::CUBE, [0.0, y - 0.55, z - 0.3], [2.4, 0.1, 0.6], 0.0, Mat::Accent, [0.9, 0.85, 0.75]));
}

/// The scene for `row` (a nature row; others are an error).
pub fn scene(cat: &Catalog, row: &Row) -> Result<Scene, String> {
    let v = gltf::row_values(cat, row).map_err(|e| e.to_string())?;
    let name = gltf::nature_kind(&row.recipe, v.variant);
    let kind = Kind::from_name(name).ok_or_else(|| format!("{}: not a nature row ({name})", row.id))?;
    let params = gltf::nature_params(cat, row).map_err(|e| e.to_string())?;
    if params.is_empty() {
        return Err(format!("{}: not a nature row", row.id));
    }
    let (c0, c1) = (v.colours[0], v.colours[1]);
    let wood = [0.42, 0.27, 0.15];
    let mut parts = Vec::new();
    let mut surfaces = Vec::new();
    let camera;
    match kind {
        Kind::WaterEllipse | Kind::WaterRect => {
            let (hx, hz, level, bed, walls) = match v.variant {
                "pool" => (1.3, 0.8, 0.5, [0.55, 0.62, 0.6], true),
                "river" => (2.0, 0.5, 0.5, [0.4, 0.38, 0.34], true),
                // Open sea too sits in a closed basin (walls just above the
                // surface), so the sheet never hangs over an open edge.
                "sea" => (3.0, 1.6, 0.6, [0.62, 0.55, 0.4], true),
                _ => (1.3, 0.9, 0.5, [0.2, 0.17, 0.1], true),
            };
            basin(&mut parts, hx, hz, level + 0.08, bed, walls);
            surfaces.push(Surface::new(kind, [0.0, level, 0.0], [hx, 0.0, hz]));
            camera = cam([0.3, level + 2.7, 3.0 + hz], [0.0, level * 0.6, 0.1], 50.0);
        }
        Kind::GlassPane => {
            let wy: f32 = -0.25;
            let (fw, fh, bar) = (1.1, 1.0, 0.07);
            for dx in [-fw * 0.5 - bar * 0.5, fw * 0.5 + bar * 0.5] {
                let (s, c) = wy.sin_cos();
                parts.push(part(MeshId::CUBE, [c * dx, 0.9, -s * dx], [bar, 1.8, bar], wy, Mat::Accent, wood));
            }
            for y in [1.1 - fh * 0.5 - bar * 0.5, 1.1 + fh * 0.5 + bar * 0.5] {
                parts.push(part(MeshId::CUBE, [0.0, y, 0.0], [fw + bar * 2.0, bar, bar], wy, Mat::Accent, wood));
            }
            backdrop(&mut parts, -0.8, 1.1);
            surfaces.push(Surface::new(kind, [0.0, 1.1, 0.0], [0.55, 0.5, 0.012]).with_yaw(wy));
            camera = cam([0.5, 1.25, 3.4], [0.0, 1.0, 0.0], 50.0);
        }
        Kind::GlassBottle => {
            parts.push(part(MeshId::CUBE, [0.0, 0.69, 0.0], [1.3, 0.06, 0.9], 0.0, Mat::Accent, wood));
            parts.push(part(MeshId::CYLINDER, [0.0, 0.33, 0.0], [0.12, 0.66, 0.12], 0.0, Mat::Accent, wood));
            backdrop(&mut parts, -0.7, 1.1);
            surfaces.push(Surface::new(kind, [0.0, 0.72 + 0.34, 0.0], [0.17, 0.3, 0.17]));
            camera = cam([0.3, 1.25, 2.6], [0.0, 1.0, 0.0], 50.0);
        }
        Kind::Foliage => {
            let leaf = c0;
            if v.variant == "needle" {
                parts.push(part(MeshId::CYLINDER, [0.0, 0.4, 0.0], [0.12, 0.8, 0.12], 0.0, Mat::Accent, wood));
                // A conifer: three tiers of cones, each tier's needles round it.
                for (y, r, hh) in [(0.75, 0.95, 0.5), (1.3, 0.7, 0.45), (1.8, 0.45, 0.4)] {
                    parts.push(part(MeshId::CONE, [0.0, y, 0.0], [r * 2.0, hh * 2.4, r * 2.0], 0.0, Mat::Accent, leaf));
                    surfaces.push(Surface::new(kind, [0.0, y, 0.0], [r, hh, r]));
                }
                camera = cam([0.2, 1.5, 5.4], [0.0, 1.05, 0.0], 50.0);
            } else if v.variant == "straw" {
                parts.push(part(MeshId::SPHERE, [0.0, 0.4, 0.0], [2.8, 1.2, 2.0], 0.0, Mat::Accent, leaf));
                surfaces.push(Surface::new(kind, [0.0, 0.4, 0.0], [1.4, 0.6, 1.0]));
                camera = cam([0.2, 1.2, 4.6], [0.0, 0.5, 0.0], 50.0);
            } else {
                parts.push(part(MeshId::CYLINDER, [0.0, 0.4, 0.0], [0.14, 0.8, 0.14], 0.0, Mat::Accent, wood));
                parts.push(part(MeshId::SPHERE, [0.0, 1.15, 0.0], [2.2, 1.6, 1.6], 0.0, Mat::Accent, leaf));
                surfaces.push(Surface::new(kind, [0.0, 1.15, 0.0], [1.1, 0.8, 0.8]));
                camera = cam([0.2, 1.4, 5.4], [0.0, 1.0, 0.0], 50.0);
            }
        }
        Kind::Grass => {
            parts.push(part(MeshId::CUBE, [0.0, 0.012, 0.0], [2.6, 0.024, 1.8], 0.0, Mat::Ground, dim(c0, 0.55)));
            surfaces.push(Surface::new(kind, [0.0, 0.14, 0.0], [1.3, 0.14, 0.9]));
            camera = cam([0.2, 0.8, 3.0], [0.0, 0.12, 0.0], 50.0);
        }
        Kind::Leaf => {
            // A spray of leaves on a stem.
            parts.push(part(MeshId::CYLINDER, [0.0, 0.7, 0.0], [0.03, 1.4, 0.03], 0.0, Mat::Accent, wood));
            for (i, (x, y, yaw)) in [(-0.45, 1.1, 0.5), (0.45, 1.25, -0.4), (-0.3, 1.55, -0.2), (0.3, 1.7, 0.3), (0.0, 1.95, 0.0), (-0.55, 1.4, 0.9), (0.55, 1.5, -0.8)].into_iter().enumerate() {
                let colour = if i % 2 == 0 { c0 } else { c1 };
                let (w, h) = (0.5, 0.32);
                parts.push(part(MeshId::CUBE, [x, y, 0.0], [w, h, 0.008], yaw, Mat::Accent, colour));
                surfaces.push(Surface::new(kind, [x, y, 0.0], [w * 0.5, h * 0.5, 0.004]).with_yaw(yaw));
            }
            camera = cam([0.0, 1.4, 3.6], [0.0, 1.35, 0.0], 50.0);
        }
        Kind::ThinPatch => {
            parts.push(part(MeshId::CYLINDER, [0.0, 0.55, -0.02], [0.04, 1.1, 0.04], 0.0, Mat::Accent, wood));
            parts.push(part(MeshId::CUBE, [0.0, 1.2, 0.0], [1.1, 0.8, 0.01], 0.35, Mat::Accent, c0));
            surfaces.push(Surface::new(kind, [0.0, 1.2, 0.0], [0.55, 0.4, 0.005]).with_yaw(0.35));
            camera = cam([0.0, 1.25, 3.0], [0.0, 1.15, 0.0], 50.0);
        }
        Kind::ThinBlob if v.variant == "membrane" => {
            // A slime: a low glossy puddle with two smaller blobs, on the ground.
            for (x, z, rx, ry, rz) in [(0.0, 0.0, 0.55, 0.22, 0.45), (0.5, 0.25, 0.22, 0.12, 0.2), (-0.45, 0.3, 0.18, 0.1, 0.16)] {
                parts.push(part(MeshId::SPHERE, [x, ry * 0.8, z], [rx * 2.0, ry * 2.0, rz * 2.0], 0.0, Mat::Accent, c0));
                surfaces.push(Surface::new(kind, [x, ry * 0.8, z], [rx, ry, rz]));
            }
            camera = cam([0.1, 0.9, 2.4], [0.0, 0.15, 0.0], 50.0);
        }
        Kind::ThinBlob | Kind::Lamp => {
            // Three candles of different heights.
            for (x, h) in [(-0.45, 0.55), (0.0, 0.8), (0.4, 0.4)] {
                parts.push(part(MeshId::CYLINDER, [x, h * 0.5, 0.0], [0.24, h, 0.24], 0.0, Mat::Accent, c0));
                surfaces.push(Surface::new(kind, [x, h * 0.5, 0.0], [0.12, h * 0.5, 0.12]));
                parts.push(part(MeshId::SPHERE, [x, h + 0.06, 0.0], [0.07, 0.12, 0.07], 0.0, Mat::Glow, [1.0, 0.85, 0.6]));
            }
            camera = cam([0.1, 0.8, 2.8], [0.0, 0.45, 0.0], 50.0);
        }
    }
    Ok(Scene { parts, surfaces, camera, params, kind })
}

/// Two optional-sixty rows (P12 left them for the nature family) shown too.
#[cfg(test)]
const EXTRA: [&str; 2] = ["car.windscreen", "magic.slime"];

/// The rows this proof shows: every nature row of batch 1, drawn or planned
/// (so a planned row can be judged), and [`EXTRA`].
#[cfg(test)]
pub fn rows(cat: &Catalog) -> Vec<&Row> {
    cat.rows.iter().filter(|r| (r.batch == 1 || EXTRA.contains(&r.id.as_str())) && ["foliage", "thin", "glass", "water"].contains(&r.recipe.as_str())).collect()
}

struct State {
    row: Option<Row>,
    scene: Option<Scene>,
    frames: u32,
    applied: bool,
    error: Option<String>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// One frame (wasm: the page address picks the row).
pub fn frame(world: &mut World) {
    let first = STATE.with(|s| s.borrow().is_none());
    if first {
        let id = super::url_value("row").unwrap_or_else(|| "water.pool.clear".into());
        let built = Catalog::bundled().map_err(|e| e.to_string()).and_then(|cat| {
            let row = cat.row(&id).ok_or_else(|| format!("no row {id}"))?.clone();
            let sc = scene(&cat, &row)?;
            Ok((row, sc))
        });
        let st = match built {
            Ok((row, sc)) => State { row: Some(row), scene: Some(sc), frames: 0, applied: false, error: None },
            Err(e) => State { row: None, scene: None, frames: 0, applied: false, error: Some(e) },
        };
        STATE.with(|s| *s.borrow_mut() = Some(st));
    }
    STATE.with(|cell| {
        let mut guard = cell.borrow_mut();
        let Some(st) = guard.as_mut() else { return };
        if let Some(e) = &st.error {
            if st.frames == 0 {
                world.res_mut::<evani_core::overlay::ErrorLog>().error(format!("sandbox: p11b: {e}"));
            }
            st.frames = st.frames.saturating_add(1);
            return;
        }
        let Some(sc) = st.scene.as_ref() else { return };
        if !st.applied {
            let mut reg = world.res_mut::<ParamRegistry>();
            let _ = reg.set_by_name("render.light.direction", ParamValue::Vec3(SUN));
            for (name, v) in &sc.params {
                let value = match *v {
                    NatureValue::F32(x) => ParamValue::F32(x),
                    NatureValue::Colour(c) => ParamValue::Color(c),
                };
                let _ = reg.set_by_name(name, value);
            }
            // The sky the water and glass reflect is the table's own, not the proof's background.
            let _ = reg.set_by_name("material.nature.sky.match_background", ParamValue::F32(0.0));
            drop(reg);
            if let Some(n) = world.try_res::<NatureSurfaces>() {
                n.set(&sc.surfaces);
            }
            st.applied = true;
        }
        // The groups the nature quality mirrors only follow a change made after
        // start-up, so `?q=` is applied on frame 4.
        if st.frames == 4
            && let Some(q) = super::quality_from_url()
        {
            let mut reg = world.res_mut::<ParamRegistry>();
            for g in ["material.quality.water", "material.quality.glass", "material.quality.foliage"] {
                let _ = reg.set_by_name(g, ParamValue::Enum(q));
            }
        }
        let (accent, ground, glow) = match world.try_res::<LookData>() {
            Some(look) => (RenderWorld::material(&look, "accent"), RenderWorld::material(&look, "ground"), RenderWorld::material(&look, "glow")),
            None => (0, 0, 0),
        };
        // The sandbox's own streamed showcase figure (proof_model) is not part
        // of a material row: drop this frame's model draws so it never stands
        // in the row's scene, whenever it finishes streaming. Only this dev
        // proof does it; the sandbox game is unchanged.
        if let Some(mut models) = world.try_res_mut::<evani_render::model::Models>() {
            models.draws.clear();
        }
        if let Some(mut rw) = world.try_res_mut::<RenderWorld>() {
            rw.camera = sc.camera;
            rw.sprites.clear();
            rw.meshes.clear();
            rw.blobs.clear();
            let mut plane = MeshInstance::new(MeshId::PLANE, [0.0, 0.0, 0.0]);
            plane.scale = [14.0, 1.0, 8.0];
            plane.material = ground;
            rw.draw_mesh(plane);
            for p in &sc.parts {
                let mut m = MeshInstance::new(p.mesh, p.at);
                m.scale = p.size;
                m.rotation = quat_axis_angle([0.0, 1.0, 0.0], p.yaw);
                m.material = match p.mat {
                    Mat::Accent => accent,
                    Mat::Ground => ground,
                    Mat::Glow => glow,
                };
                m.color = [p.colour[0], p.colour[1], p.colour[2], 1.0];
                rw.draw_mesh(m);
            }
        }
        st.frames = st.frames.saturating_add(1);
        if st.frames % 10 == 0 {
            let drawn = world.try_res::<NatureSurfaces>().map_or(0, |n| n.drawn());
            let title = format!("p11b {} kind={:?} surfaces={} drawn={} frames={}", st.row.as_ref().map_or("", |r| r.id.as_str()), sc.kind, sc.surfaces.len(), drawn, st.frames);
            #[cfg(target_arch = "wasm32")]
            if let Some(d) = web_sys::window().and_then(|w| w.document()) {
                d.set_title(&title);
            }
            #[cfg(not(target_arch = "wasm32"))]
            let _ = title;
        }
    });
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    /// Every batch-1 nature row builds a scene with its kind, some solid
    /// parts, the surfaces the pass needs (see-through rows have one) and
    /// params the nature module registers.
    #[test]
    fn every_batch_one_nature_row_has_a_scene() {
        let cat = Catalog::bundled().unwrap();
        let list = rows(&cat);
        assert_eq!(list.len(), 22, "the 20 nature rows of batch 1 and two optional ones");
        for row in list {
            let sc = scene(&cat, row).unwrap_or_else(|e| panic!("{e}"));
            assert!(!sc.parts.is_empty() && !sc.surfaces.is_empty(), "{}", row.id);
            assert!(sc.surfaces.iter().all(|s| s.kind == sc.kind), "{}", row.id);
            assert!(!sc.params.is_empty(), "{}", row.id);
            let v = gltf::row_values(&cat, row).unwrap();
            let want = Kind::from_name(gltf::nature_kind(&row.recipe, v.variant)).unwrap();
            assert_eq!(sc.kind, want, "{}", row.id);
        }
        assert!(scene(&cat, cat.row("wood.oak").or_else(|| cat.rows.iter().find(|r| r.recipe == "wood")).unwrap()).is_err());
    }

    /// Every water row sits in a closed basin: four walls whose tops are at
    /// or above the surface, the surface above the bed (critic round 1: the
    /// open sea floated over an open bed).
    #[test]
    fn water_sits_in_a_closed_basin() {
        let cat = Catalog::bundled().unwrap();
        for row in rows(&cat) {
            let sc = scene(&cat, row).unwrap();
            if !matches!(sc.kind, Kind::WaterEllipse | Kind::WaterRect) {
                continue;
            }
            let level = sc.surfaces[0].centre[1];
            let walls: Vec<&Part> = sc.parts.iter().filter(|p| p.mesh == MeshId::BEVEL_CUBE).collect();
            assert_eq!(walls.len(), 4, "{}: four walls", row.id);
            for w in walls {
                let top = w.at[1] + w.size[1] * 0.5;
                assert!(top >= level && w.at[1] - w.size[1] * 0.5 <= 0.0, "{}: wall {:?} {:?} under the surface {level}", row.id, w.at, w.size);
            }
            assert!(level > 0.05, "{}: above the bed", row.id);
        }
    }

    /// The quality groups reach the nature pass: turning the water group
    /// down sets `material.nature.quality`, and the pass draws its flat mode.
    #[test]
    fn the_water_group_sets_the_nature_pass_quality() {
        let mut app = crate::build_app();
        app.run_headless(2, 1000.0 / 60.0);
        app.world.res::<NatureSurfaces>().set(&[Surface::new(Kind::WaterRect, [0.0, 0.5, 0.0], [1.0, 0.0, 1.0])]);
        {
            let mut reg = app.world.res_mut::<ParamRegistry>();
            reg.set_by_name("material.quality.water", ParamValue::Enum(2)).unwrap();
        }
        app.run_headless(3, 1000.0 / 60.0);
        let reg = app.world.res::<ParamRegistry>();
        let id = reg.id("material.nature.quality").unwrap();
        assert_eq!(reg.value(id), Some(ParamValue::F32(1.0)), "low: nature quality 1");
    }
}
