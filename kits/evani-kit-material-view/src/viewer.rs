//! The viewer's state ([`Viewer`], a world resource) and the commands that
//! change it. The page (pointer drags, taps on the scene row, the browser
//! and the chips) and the keys both push [`Command`]s; the kit applies them
//! once a frame in [`apply_system`], before the camera is placed.
//!
//! Lights are written to the render params only when the viewer's light
//! state changes (a preset chip or a sun drag), so a live edit of
//! `render.light.*` from the Look Lab or an AI stays until the next change.

use evani_core::ecs::World;
use evani_core::params::ParamRegistry;
use evani_core::time::Time;

use crate::MaterialViewKit;
use crate::library::{Data, set_json, under};
use crate::orbit::{Orbit, Turntable};

/// Something the page or the keys asked for.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Turn the orbit (degrees).
    Orbit { daz: f32, del: f32 },
    /// Slide the target (fractions of the view height, `dy` down-positive).
    Pan { dx: f32, dy: f32 },
    /// Distance times this factor.
    Zoom(f32),
    /// Back to the material's own camera.
    Reset,
    Scene(String),
    NextScene,
    Material(String),
    Light(usize),
    NextLight,
    /// Move the sun from the preset (degrees of azimuth and elevation).
    Sun { daz: f32, del: f32 },
    /// Turntable on, off or toggled (`None`).
    Turntable(Option<bool>),
    Compare(Option<bool>),
    Pause(Option<bool>),
}

/// The viewer's state.
#[derive(Debug, Clone, PartialEq)]
pub struct Viewer {
    pub scene: String,
    pub material: String,
    pub light: usize,
    pub orbit: Orbit,
    /// The camera a reset goes back to.
    pub home: Orbit,
    /// The turntable the user asked for.
    pub spin: bool,
    /// The turntable as it runs (held while paused or comparing).
    pub turn: Turntable,
    turn_speed: f32,
    /// Sun moved from the preset, degrees.
    pub sun_az: f32,
    pub sun_el: f32,
    pub compare: bool,
    pub paused: bool,
    /// Families compiled into this build (`Viewer::enable`, from the app).
    pub families_on: Vec<String>,
    pub pending: Vec<Command>,
    /// A request for the page's library drawer and tuning panel (open
    /// drawer, open panel), taken by the page on its next frame: a capture
    /// moment sets it (moments.md asks for them open on desktop). The page
    /// keeps both closed on a phone whatever is asked.
    pub panels: Option<(bool, bool)>,
    /// A request for the page to show its library drawer searched for this
    /// text, taken on its next frame: a held proof moment sets it, so the
    /// held row's greyed tile and its reason show (a planned row has no
    /// shape to draw). The page opens the drawer for it on a phone too.
    pub lib_search: Option<String>,
    /// Bumped on every change the page shows.
    pub revision: u64,
    started: bool,
    light_dirty: bool,
}

impl Default for Viewer {
    fn default() -> Self {
        Self {
            scene: String::new(),
            material: String::new(),
            light: 0,
            orbit: Orbit::default(),
            home: Orbit::default(),
            spin: true,
            turn: Turntable { on: true, ..Turntable::default() },
            turn_speed: 0.0,
            sun_az: 0.0,
            sun_el: 0.0,
            compare: false,
            paused: false,
            families_on: Vec::new(),
            pending: Vec::new(),
            panels: None,
            lib_search: None,
            revision: 1,
            started: false,
            light_dirty: true,
        }
    }
}

impl Viewer {
    /// Mark a material family as compiled in (the app calls this for each
    /// `material-<family>` feature it was built with).
    pub fn enable(world: &mut World, family: &str) {
        if let Some(mut v) = world.try_res_mut::<Viewer>()
            && !v.families_on.iter().any(|f| f == family)
        {
            v.families_on.push(family.to_string());
            v.revision += 1;
        }
    }

    pub fn has(&self, family: &str) -> bool {
        self.families_on.iter().any(|f| f == family)
    }

    pub fn push(&mut self, c: Command) {
        self.pending.push(c);
    }

    /// The turntable as it should run now.
    fn spinning(&self) -> bool {
        self.spin && !self.paused && !self.compare
    }
}

/// The sun direction for a preset moved by the viewer's drag.
pub fn sun_direction(el_deg: f32, az_deg: f32) -> [f32; 3] {
    let (el, az) = (el_deg.clamp(-5.0, 89.0).to_radians(), az_deg.to_radians());
    [el.cos() * az.sin(), el.sin(), el.cos() * az.cos()]
}

/// Open a material: its family's params back to their defaults (Quality
/// params kept), its own values set over them, its scene and camera.
/// Returns false (and changes nothing) for an unknown material or a family
/// this build lacks.
pub fn open_material(world: &World, data: &Data, id: &str) -> bool {
    let Some((family, mat)) = data.material(id) else { return false };
    if !world.try_res::<Viewer>().is_some_and(|v| v.has(&family.id)) {
        return false;
    }
    let Some(scene) = data.scene(&mat.scene) else { return false };
    {
        let mut reg = world.res_mut::<ParamRegistry>();
        let ids: Vec<_> = reg
            .defs()
            .iter()
            .filter(|_| !family.keep_params)
            .filter(|d| family.prefixes.iter().any(|p| under(&d.name, p)))
            .filter(|d| data.library.groups.group_of(&d.name).is_some_and(|g| g != "quality"))
            .filter_map(|d| reg.id(&d.name))
            .collect();
        for pid in ids {
            let _ = reg.reset(pid);
        }
        for (name, v) in &mat.params {
            set_json(&mut reg, name, v);
        }
    }
    let tick = world.res::<Time>().tick;
    let limits = Orbit::from_data(&scene.orbit, None);
    let orbit = mat.orbit.as_ref().map_or(limits, |o| Orbit::from_data(o, Some(&limits)));
    let mut v = world.res_mut::<Viewer>();
    // A scene with its own light params puts them on (or a scene without
    // takes them off again) through the light.
    if v.scene != scene.id {
        v.light_dirty = true;
    }
    v.scene = scene.id.clone();
    v.material = mat.id.clone();
    v.orbit = orbit;
    v.home = orbit;
    v.turn.zero(tick);
    v.revision += 1;
    true
}

/// Write the current light preset (and the sun drag) to the render params.
pub fn apply_light(world: &World, data: &Data) {
    let Some((i, daz, del, scene)) = world.try_res::<Viewer>().map(|v| (v.light, v.sun_az, v.sun_el, v.scene.clone())) else { return };
    let Some(p) = data.lights.presets.get(i).or_else(|| data.lights.presets.first()) else { return };
    let dir = sun_direction(p.sun.el + del, p.sun.az + daz);
    let mut reg = world.res_mut::<ParamRegistry>();
    let num = |x: f32| serde_json::json!(x);
    let rgb = |c: [f32; 3]| serde_json::json!(c);
    set_json(&mut reg, "render.light.direction", &rgb(dir));
    set_json(&mut reg, "render.light.color", &rgb(p.color));
    set_json(&mut reg, "render.light.intensity", &num(p.intensity));
    set_json(&mut reg, "render.ambient", &rgb(p.ambient));
    set_json(&mut reg, "render.exposure", &num(p.exposure));
    set_json(&mut reg, "render.clear_color", &rgb(p.clear));
    for (name, v) in &p.params {
        set_json(&mut reg, name, v);
    }
    for (name, v) in data.scene(&scene).map(|s| &s.light).into_iter().flatten() {
        set_json(&mut reg, name, v);
    }
}

/// The scene after `current` whose family this build has.
pub fn next_scene(data: &Data, v: &Viewer) -> Option<String> {
    let scenes = &data.library.scenes;
    let at = scenes.iter().position(|s| s.id == v.scene).unwrap_or(0);
    (1..=scenes.len()).map(|k| &scenes[(at + k) % scenes.len()]).find(|s| v.has(&s.family)).map(|s| s.id.clone())
}

/// Apply the frame's commands, keep the turntable and the lights in step.
pub fn apply_system(world: &mut World) {
    let Some(data) = world.try_res::<crate::ViewerData>().map(|d| d.0.clone()) else { return };
    let Some(k) = world.try_res::<MaterialViewKit>().map(|k| *k) else { return };
    let tick = world.res::<Time>().tick;
    let turn_speed = world.res::<ParamRegistry>().f32(k.turntable_speed);
    // First frame: the first scene this build can show.
    let start = world.try_res::<Viewer>().filter(|v| !v.started).map(|v| {
        let first = data.library.scenes.iter().find(|s| v.has(&s.family)).map(|s| s.default.clone());
        first.unwrap_or_default()
    });
    if let Some(m) = start {
        if !m.is_empty() {
            open_material(world, &data, &m);
        } else if let Some(s) = data.library.scenes.first() {
            // No family compiled in: the default scene's camera, the kit's
            // plain ball (draw.rs).
            let o = Orbit::from_data(&s.orbit, None);
            let mut v = world.res_mut::<Viewer>();
            v.scene = s.id.clone();
            v.orbit = o;
            v.home = o;
        }
        world.res_mut::<Viewer>().started = true;
    }
    let cmds = std::mem::take(&mut world.res_mut::<Viewer>().pending);
    for c in cmds {
        match c {
            Command::Scene(id) => {
                if let Some(s) = data.scene(&id) {
                    let m = s.default.clone();
                    open_material(world, &data, &m);
                }
            }
            Command::NextScene => {
                let next = next_scene(&data, &world.res::<Viewer>());
                if let Some(id) = next.and_then(|id| data.scene(&id).map(|s| s.default.clone())) {
                    open_material(world, &data, &id);
                }
            }
            Command::Material(id) => {
                open_material(world, &data, &id);
            }
            c => {
                let n = data.lights.presets.len().max(1);
                let mut v = world.res_mut::<Viewer>();
                match c {
                    Command::Orbit { daz, del } => {
                        v.orbit.az -= daz;
                        v.orbit.el += del;
                        v.orbit = v.orbit.clamped();
                    }
                    Command::Pan { dx, dy } => v.orbit.pan(dx, dy),
                    Command::Zoom(f) => v.orbit.zoom(f),
                    Command::Reset => {
                        v.orbit = v.home;
                        v.turn.zero(tick);
                    }
                    Command::Light(i) => {
                        v.light = i.min(n - 1);
                        v.sun_az = 0.0;
                        v.sun_el = 0.0;
                        v.light_dirty = true;
                    }
                    Command::NextLight => {
                        v.light = (v.light + 1) % n;
                        v.sun_az = 0.0;
                        v.sun_el = 0.0;
                        v.light_dirty = true;
                    }
                    Command::Sun { daz, del } => {
                        v.sun_az = (v.sun_az + daz).rem_euclid(360.0);
                        v.sun_el = (v.sun_el + del).clamp(-60.0, 60.0);
                        v.light_dirty = true;
                    }
                    Command::Turntable(on) => v.spin = on.unwrap_or(!v.spin),
                    Command::Compare(on) => v.compare = on.unwrap_or(!v.compare),
                    Command::Pause(on) => v.paused = on.unwrap_or(!v.paused),
                    Command::Scene(_) | Command::NextScene | Command::Material(_) => {}
                }
                v.revision += 1;
            }
        }
    }
    let dirty = {
        let mut v = world.res_mut::<Viewer>();
        let run = v.spinning();
        if v.turn_speed != turn_speed && v.turn.on {
            // Keep the angle when the speed param changes.
            let old = v.turn_speed;
            v.turn.set(false, tick, old);
            v.turn.set(true, tick, turn_speed);
        }
        v.turn_speed = turn_speed;
        v.turn.set(run, tick, turn_speed);
        std::mem::take(&mut v.light_dirty)
    };
    if dirty {
        apply_light(world, &data);
    }
}
