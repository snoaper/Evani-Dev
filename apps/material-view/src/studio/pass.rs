//! WP5 (docs/studio/WORK_PACKAGES.md): the screen pass hook. Writes
//! `Resolved::pass` (pixel, paper, tilt-shift, silhouette, none) to the
//! render passes' params (`render.studio_pass.*`, evani-render feature
//! `studio-passes`) each time `PassKey` changes, or the studio view is
//! entered or left; never touches the model, so switching a pass rebuilds
//! nothing. Outside the studio view the mode is `none`: the legacy scenes
//! draw exactly as before.
//!
//! Pixel's camera snap lives here too: the orbit camera's target is moved to
//! the nearest whole art pixel in the view plane, and what is left over goes
//! to the pass as the grid's offset, so pixels stay put on the object while
//! the view pans or zooms (a turning object changes its pixels anyway).

use evani_core::camera::CameraPose;
use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::params::{ParamRegistry, ParamValue};
use evani_core::plugin::PluginContext;
use evani_kit_material_view::Viewer;
use evani_kit_material_view::studio::evani_studio::hash::PassKey;
use evani_kit_material_view::studio::evani_studio::styles::{Pass, Surface};
use evani_kit_material_view::studio::{Studio, StudioData};
use evani_render::RenderWorld;
use evani_render::studio_passes::{MODES, StudioPassState};

/// The viewer scene the studio draws in (WP4 adds it to the scene row).
pub const SCENE: &str = "studio";

/// What was last written (the pass key, and whether the studio was showing).
#[derive(Debug, Default)]
pub struct PassSync {
    last: Option<((PassKey, u64), bool)>,
    /// Params written by this hook (a test reads it).
    pub writes: u64,
}

/// A style colour (linear) as the passes take it (display values).
fn display(c: [f32; 3]) -> ParamValue {
    ParamValue::Vec3(c.map(|x| x.max(0.0).powf(1.0 / 2.2)))
}

/// The params a record's pass sets, by name. Mode first; a pass leaves the
/// other passes' numbers alone. A surface with `dither` (the engraving
/// style; its pass is `none`) runs the screen-space engraving, in the
/// style's line and paper colours.
pub fn params_for(pass: &Pass, surface: &Surface) -> Vec<(&'static str, ParamValue)> {
    let mode = |name: &str| ParamValue::Enum(MODES.iter().position(|m| *m == name).unwrap_or(0) as u32);
    let engrave = matches!(pass, Pass::None) && surface.dither;
    let mut out = vec![("render.studio_pass.mode", mode(if engrave { "dither" } else { pass.name() }))];
    match *pass {
        Pass::None if engrave => {
            out.push(("render.studio_pass.ink", display(surface.outline_color)));
            out.push(("render.studio_pass.paper", display(surface.paper)));
        }
        Pass::None => {}
        Pass::Pixel { px, levels, outline } => {
            out.push(("render.studio_pass.px", ParamValue::F32(px.round().clamp(1.0, 12.0))));
            out.push(("render.studio_pass.levels", ParamValue::I32(levels.min(16) as i32)));
            out.push(("render.studio_pass.outline", ParamValue::F32(if outline { 1.0 } else { 0.0 })));
            // Dither only with a palette cut, and lighter on a rich palette
            // (Pixel HD keeps its ramps smooth).
            let dither = match levels {
                0 => 0.0,
                1..=7 => 0.35,
                _ => 0.15,
            };
            out.push(("render.studio_pass.dither", ParamValue::F32(dither)));
        }
        Pass::Paper { grain, edge, bleed } => {
            out.push(("render.studio_pass.grain", ParamValue::F32(grain)));
            out.push(("render.studio_pass.edge", ParamValue::F32(edge)));
            out.push(("render.studio_pass.bleed", ParamValue::F32(bleed)));
        }
        Pass::Tiltshift { focus, blur } => {
            out.push(("render.studio_pass.focus", ParamValue::F32(focus)));
            out.push(("render.studio_pass.blur", ParamValue::F32(blur)));
        }
        Pass::Silhouette { fog } => out.push(("render.studio_pass.fog", ParamValue::F32(fog))),
    }
    out
}

/// The studio's current pass and surface (and the keys that say when they
/// change), if the record resolves.
fn current(world: &World) -> Option<((PassKey, u64), Pass, Surface)> {
    let data = world.try_res::<StudioData>()?.0.clone();
    let r = world.try_res::<Studio>()?.state.resolve(&data).ok()?;
    let engrave = u64::from(r.surface.dither) | u64::from(r.surface.outline_color.map(f32::to_bits)[0]) << 1;
    Some(((r.pass_key, engrave), r.pass, r.surface))
}

fn sync(world: &mut World) {
    let showing = world.try_res::<Viewer>().is_some_and(|v| v.scene == SCENE);
    let Some((key, pass, surface)) = current(world) else { return };
    let (pass, surface) = if showing { (pass, surface) } else { (Pass::None, Surface::default()) };
    if world.try_res::<PassSync>().is_some_and(|s| s.last == Some((key, showing))) {
        return;
    }
    let list = params_for(&pass, &surface);
    let mut errors = Vec::new();
    {
        let mut reg = world.res_mut::<ParamRegistry>();
        for (name, v) in &list {
            if let Err(e) = reg.set_by_name(name, *v) {
                errors.push(format!("studio pass: {name}: {e:?}"));
            }
        }
    }
    for e in errors {
        world.res_mut::<evani_core::overlay::ErrorLog>().error(e);
    }
    if let Some(mut s) = world.try_res_mut::<PassSync>() {
        s.last = Some((key, showing));
        s.writes += list.len() as u64;
    }
}

/// Art pixel size in world units at distance `d` for a vertical field of
/// view `fov_y` (radians) and a view `h` pixels high.
pub fn art_pixel(d: f32, fov_y: f32, h: f32, px: f32) -> f32 {
    2.0 * d * (fov_y * 0.5).tan() / h.max(1.0) * px.max(1.0)
}

/// Snap `pose`'s target and eye by whole art pixels in the view plane;
/// returns the snapped pose and the remainder in screen pixels (x right,
/// y down), each in `0..px`.
pub fn snap(pose: &CameraPose, h: f32, px: f32) -> (CameraPose, [f32; 2]) {
    let f = sub(pose.target, pose.eye);
    let d = len(f).max(1e-4);
    let fw = scale(f, 1.0 / d);
    let right = norm(cross(fw, [0.0, 1.0, 0.0]));
    let up = cross(right, fw);
    let step = art_pixel(d, pose.fov_y_deg.to_radians(), h, px);
    let (x, y) = (dot(pose.target, right) / step, dot(pose.target, up) / step);
    let (dx, dy) = ((x.round() - x) * step, (y.round() - y) * step);
    let shift = add(scale(right, dx), scale(up, dy));
    let out = CameraPose { eye: add(pose.eye, shift), target: add(pose.target, shift), ..*pose };
    // The scene moved by -shift on screen; the grid follows it.
    let per = px / step.max(1e-9);
    let ox = (-dx * per).rem_euclid(px);
    let oy = (dy * per).rem_euclid(px);
    (out, [ox, oy])
}

fn snap_system(world: &mut World) {
    let showing = world.try_res::<Viewer>().is_some_and(|v| v.scene == SCENE);
    let Some(state) = world.try_res::<StudioPassState>().map(|s| s.clone()) else { return };
    let reg = world.res::<ParamRegistry>();
    let mode = reg.id("render.studio_pass.mode").and_then(|id| reg.value(id));
    let px = reg.id("render.studio_pass.px").and_then(|id| reg.value(id));
    drop(reg);
    let (Some(ParamValue::Enum(1)), Some(ParamValue::F32(px)), true) = (mode, px, showing) else {
        state.set_offset([0.0; 2]);
        return;
    };
    let Some(mut rw) = world.try_res_mut::<RenderWorld>() else { return };
    let h = if rw.target_px[1] > 0 { rw.target_px[1] as f32 } else { 720.0 };
    let (snapped, off) = snap(&rw.camera, h, evani_render::studio_passes::art_pixel(px, h));
    rw.camera = snapped;
    drop(rw);
    state.set_offset(off);
}

pub fn install(ctx: &mut PluginContext) {
    ctx.resource(PassSync::default());
    ctx.system(SystemDesc::new("material_view.studio.pass", Stage::Frame, sync));
    // After the kit's camera system (Frame, order 0) has set this frame's pose.
    ctx.system(SystemDesc::new("material_view.studio.pixel_snap", Stage::Frame, snap_system).order(100));
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn scale(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn len(a: [f32; 3]) -> f32 {
    dot(a, a).sqrt()
}
fn norm(a: [f32; 3]) -> [f32; 3] {
    scale(a, 1.0 / len(a).max(1e-9))
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_kit_material_view::studio::{Pick, StudioCommand};
    use evani_render::studio_passes::SHADERS;

    const DT: f64 = 1000.0 / 60.0;

    fn studio_app() -> evani_core::App {
        let mut app = crate::build_app();
        app.run_headless(2, DT);
        app.world.res_mut::<Viewer>().scene = SCENE.into();
        app.run_headless(1, DT);
        app
    }

    fn mode(app: &evani_core::App) -> &'static str {
        let reg = app.world.res::<ParamRegistry>();
        match reg.id("render.studio_pass.mode").and_then(|id| reg.value(id)) {
            Some(ParamValue::Enum(i)) => MODES[i as usize],
            _ => "?",
        }
    }

    fn f(app: &evani_core::App, name: &str) -> f32 {
        let reg = app.world.res::<ParamRegistry>();
        match reg.id(name).and_then(|id| reg.value(id)) {
            Some(ParamValue::F32(v)) => v,
            _ => f32::NAN,
        }
    }

    fn pick_style(app: &mut evani_core::App, id: &str) {
        app.world.res_mut::<Studio>().push(StudioCommand::Pick(Pick::Style(id.into())));
        app.run_headless(1, DT);
    }

    /// The automatic check that every style has its effect and stays within
    /// the shader caps: each style's pass is one of the render passes' modes
    /// with its own shader; a pass-led style (pass not `none`) really turns
    /// one on; a surface-led style really changes the surface (bands,
    /// outline, posterize, dither or a grade), and only the two PBR styles
    /// (the engine's own light) may leave all three stages alone; each pass
    /// shader is far under the per-module cap and all of them together add
    /// under 20 KB.
    #[test]
    fn every_style_has_its_effect_and_the_passes_fit_the_shader_caps() {
        let data = evani_kit_material_view::studio::catalogs().unwrap();
        assert_eq!(data.styles.styles.len(), 24);
        const PBR: [&str; 2] = ["WK05", "WK24"];
        for s in &data.styles.styles {
            let m = MODES.iter().position(|m| *m == s.pass.name());
            let m = m.unwrap_or_else(|| panic!("{} ({}): pass `{}` has no render mode", s.id, s.name, s.pass.name()));
            if m > 0 {
                let sh = &SHADERS[m - 1];
                assert!(sh.name.ends_with(s.pass.name()) && !sh.source.is_empty(), "{}: no shader for {}", s.id, s.pass.name());
            }
            let u = &s.surface;
            let surface = u.bands > 0 || u.outline > 0.0 || u.posterize > 0 || u.dither || u.desaturate > 0.0 || (u.value - 1.0).abs() > 1e-3 && u.value > 0.0;
            let geometry = !matches!(s.geometry, evani_kit_material_view::studio::evani_studio::styles::Geometry::None);
            assert!(surface || geometry || m > 0 || PBR.contains(&s.id.as_str()), "{} ({}) changes nothing", s.id, s.name);
        }
        let cap = evani_material_recipes::split::MODULE_BUDGET;
        let mut total = 0;
        for sh in &SHADERS {
            assert!(sh.source.len() < cap / 6, "{} is {} bytes", sh.name, sh.source.len());
            total += sh.source.len();
        }
        assert!(total < 20 * 1024, "{total}");
    }

    /// Picking a style writes its pass; Pixel and Pixel HD differ in grid
    /// size and palette; a pass control moves the param; leaving the studio
    /// view turns the pass off; and none of it ever touches the geometry or
    /// variant key (switching a pass rebuilds nothing).
    #[test]
    fn a_style_sets_its_pass_and_never_rebuilds_the_model() {
        let mut app = studio_app();
        let data = app.world.res::<StudioData>().0.clone();
        let keys = |app: &evani_core::App| {
            let r = app.world.res::<Studio>().state.resolve(&data).unwrap();
            (r.geometry_key, r.variant_key)
        };
        assert_eq!(mode(&app), "none", "the opening Toon style has no screen pass");
        pick_style(&mut app, "WK09");
        assert_eq!(mode(&app), "pixel");
        assert_eq!(f(&app, "render.studio_pass.px"), 4.0);
        let classic = keys(&app);
        app.world.res_mut::<Studio>().push(StudioCommand::Control { id: "pass.px".into(), value: 6.0 });
        app.run_headless(1, DT);
        assert_eq!(f(&app, "render.studio_pass.px"), 6.0);
        assert_eq!(keys(&app), classic, "a pass control moves neither key");
        pick_style(&mut app, "WK10");
        assert_eq!((mode(&app), f(&app, "render.studio_pass.px"), f(&app, "render.studio_pass.outline")), ("pixel", 3.0, 0.0));
        for (style, want) in [("WK14", "paper"), ("WK15", "paper"), ("WK16", "paper"), ("WK13", "paper"), ("WK07", "tiltshift"), ("WK22", "silhouette"), ("WK21", "dither"), ("WK03", "none")] {
            pick_style(&mut app, style);
            assert_eq!(mode(&app), want, "{style}");
        }
        pick_style(&mut app, "WK14");
        assert_eq!(f(&app, "render.studio_pass.bleed"), 0.5);
        app.world.res_mut::<Viewer>().scene = "ball".into();
        app.run_headless(1, DT);
        assert_eq!(mode(&app), "none", "outside the studio view no pass runs");
        let writes = app.world.res::<PassSync>().writes;
        app.run_headless(3, DT);
        assert_eq!(app.world.res::<PassSync>().writes, writes, "nothing is rewritten while nothing changes");
    }

    /// The snapped target sits on the art-pixel grid of the view plane; the
    /// remainder handed to the pass is under one art pixel; snapping a
    /// snapped pose moves nothing.
    #[test]
    fn the_pixel_camera_snaps_to_whole_art_pixels() {
        let pose = CameraPose { eye: [0.3, 1.7, 5.2], target: [0.123, 0.61, -0.04], up: [0.0, 1.0, 0.0], fov_y_deg: 40.0 };
        let (s, off) = snap(&pose, 720.0, 4.0);
        assert!(off.iter().all(|o| (0.0..4.0).contains(o)), "{off:?}");
        let d = len(sub(s.target, s.eye));
        let step = art_pixel(d, 40f32.to_radians(), 720.0, 4.0);
        let fw = norm(sub(s.target, s.eye));
        let right = norm(cross(fw, [0.0, 1.0, 0.0]));
        let up = cross(right, fw);
        for axis in [right, up] {
            let k = dot(s.target, axis) / step;
            assert!((k - k.round()).abs() < 1e-3, "{k}");
        }
        assert!(len(sub(sub(s.target, s.eye), sub(pose.target, pose.eye))) < 1e-5, "the view direction is kept");
        let (again, _) = snap(&s, 720.0, 4.0);
        assert!(len(sub(again.target, s.target)) < 1e-4 * step.max(1e-3) + 1e-6);
    }
}
