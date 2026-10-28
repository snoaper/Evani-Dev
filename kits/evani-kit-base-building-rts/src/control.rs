//! The player's side: phases (title, playing, over), pause, the camera,
//! picking and box selection, the smart command, modes (attack, place),
//! control groups, markers and notices. All of it is local UI state
//! ([`Ui`]); the match only ever sees the [`Command`]s pushed here.
//!
//! Mouse: left click selects, left drag boxes, right click is the smart
//! command. Touch: tap selects an own unit or building, a tap elsewhere with
//! a selection is the smart command, a drag boxes; the touch buttons press
//! the same actions as the keys.

use evani_core::camera::CameraPose;
use evani_core::ecs::World;
use evani_core::input::{Input, PointerKind};
use evani_core::time::Time;
use evani_core::touch;
use evani_render::RenderWorld;

use crate::data::{Types, BASTION, FOUNDRY, RUMBLER, TROOPER, WORKER};
use crate::grid::{dist, dist_to_rect, EXPLORED};
use crate::sim::{is_site, trains, Command, CommandKind, Match, PLAYER};
use crate::{Actions, Tune};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Title,
    Playing,
    /// The match is over; the winning team.
    Over(u8),
}

/// What the next click or tap does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    /// Attack the enemy clicked, or attack-move to the ground.
    Attack,
    /// Place a building of this kind.
    Place(usize),
}

/// A command marker on the ground.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Marker {
    pub pos: [f32; 2],
    /// Seconds left.
    pub t: f32,
    pub attack: bool,
}

/// Local UI state (never part of the lockstep match).
#[derive(Debug, Clone, PartialEq)]
pub struct Ui {
    pub phase: Phase,
    pub paused: bool,
    /// Camera target on the ground `[x, z]`.
    pub cam: [f32; 2],
    /// Camera distance (m).
    pub zoom: f32,
    pub selected: Vec<u32>,
    pub groups: [Vec<u32>; 3],
    pub mode: Mode,
    /// Box select in screen pixels while dragging (from, to).
    pub drag: Option<([f32; 2], [f32; 2])>,
    pub markers: Vec<Marker>,
    /// HUD notices and seconds left.
    pub notices: Vec<(&'static str, f32)>,
    /// The cell under the pointer (the build ghost's corner follows it).
    pub hover: Option<[i32; 2]>,
    /// Whether the build ghost can go where it is.
    pub hover_ok: bool,
    /// The right button was down during the current press.
    cmd_seen: bool,
    /// The current press began on a touch button: not a map tap.
    ignore_press: bool,
}

impl Ui {
    pub fn new(m: &Match, t: &Tune, free_roam: bool) -> Self {
        let cam = m.hub(PLAYER).map_or([0.0; 2], |b| rect_center(m.building_rect(b)));
        Self {
            phase: if free_roam { Phase::Playing } else { Phase::Title },
            paused: false,
            cam,
            zoom: t.cam_zoom_start.clamp(t.cam_zoom_min, t.cam_zoom_max.max(t.cam_zoom_min)),
            selected: Vec::new(),
            groups: Default::default(),
            mode: Mode::Normal,
            drag: None,
            markers: Vec::new(),
            notices: Vec::new(),
            hover: None,
            hover_ok: false,
            cmd_seen: false,
            ignore_press: false,
        }
    }

    /// The match ended: `winner` won.
    pub fn finish(&mut self, winner: u8) {
        self.phase = Phase::Over(winner);
        self.mode = Mode::Normal;
        self.drag = None;
        self.paused = false;
    }

    fn notice(&mut self, text: &'static str) {
        self.notices.retain(|n| n.0 != text);
        self.notices.push((text, 2.0));
        if self.notices.len() > 3 {
            self.notices.remove(0);
        }
    }
}

pub fn rect_center(r: [f32; 4]) -> [f32; 2] {
    [(r[0] + r[2]) * 0.5, (r[1] + r[3]) * 0.5]
}

// ---- camera ------------------------------------------------------------------

/// The camera: a high three-quarter view from the south (north up), pulled
/// back on narrow screens so the same width of map shows.
pub fn pose(cam: [f32; 2], zoom: f32, t: &Tune, aspect: f32) -> CameraPose {
    let pull = if aspect > 0.0 && aspect < 1.0 { (1.0 / aspect).min(2.2) } else { 1.0 };
    let d = zoom * pull;
    let p = t.cam_pitch_deg.clamp(20.0, 89.0).to_radians();
    CameraPose {
        eye: [cam[0], d * p.sin(), cam[1] + d * p.cos()],
        target: [cam[0], 0.0, cam[1]],
        up: [0.0, 1.0, 0.0],
        fov_y_deg: t.cam_fov_deg,
    }
}

/// The camera's ground target `p` moved to where it may be: `cam_edge_margin`
/// inside the map edge (so the ridges beyond it never fill half the screen)
/// and no more than `cam_fog_margin` from the nearest cell the player has
/// seen (so at least the lower half of the view is known ground, never a
/// screen of solid fog). Pulled straight back towards that nearest cell.
pub fn clamp_cam(m: &Match, t: &Tune, p: [f32; 2]) -> [f32; 2] {
    let g = &m.grid;
    let half = g.half();
    let e = t.cam_edge_margin.clamp(0.0, half[0].min(half[1]));
    let inset = |q: [f32; 2]| [q[0].clamp(-half[0] + e, half[0] - e), q[1].clamp(-half[1] + e, half[1] - e)];
    let q = inset(p);
    let fog = &m.fog[PLAYER as usize];
    let mut best: Option<([f32; 2], f32)> = None;
    for (i, f) in fog.cells.iter().enumerate() {
        if *f == crate::grid::UNEXPLORED {
            continue;
        }
        let c = g.center([i as i32 % g.w, i as i32 / g.w]);
        let d2 = (q[0] - c[0]).powi(2) + (q[1] - c[1]).powi(2);
        if best.is_none_or(|b| d2 < b.1) {
            best = Some((c, d2));
        }
    }
    let r = t.cam_fog_margin.max(0.0);
    match best {
        Some((c, d2)) if d2 > r * r => {
            let k = r / d2.sqrt();
            inset([c[0] + (q[0] - c[0]) * k, c[1] + (q[1] - c[1]) * k])
        }
        _ => q,
    }
}

type V3 = [f32; 3];

fn basis(pose: &CameraPose) -> (V3, V3, V3) {
    let sub = |a: V3, b: V3| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let norm = |a: V3| {
        let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt().max(1e-6);
        [a[0] / l, a[1] / l, a[2] / l]
    };
    let f = norm(sub(pose.target, pose.eye));
    let r = norm([-f[2], 0.0, f[0]]);
    let u = [r[1] * f[2] - r[2] * f[1], r[2] * f[0] - r[0] * f[2], r[0] * f[1] - r[1] * f[0]];
    (f, r, u)
}

fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// A world point in canvas pixels (None behind the camera).
pub fn to_screen(pose: &CameraPose, p: V3, vp: [f32; 2]) -> Option<[f32; 2]> {
    if vp[0] <= 0.0 || vp[1] <= 0.0 {
        return None;
    }
    let (f, r, u) = basis(pose);
    let d = [p[0] - pose.eye[0], p[1] - pose.eye[1], p[2] - pose.eye[2]];
    let z = dot(d, f);
    if z < 1e-3 {
        return None;
    }
    let t = (pose.fov_y_deg * 0.5).to_radians().tan();
    let aspect = vp[0] / vp[1];
    let (nx, ny) = (dot(d, r) / z / (t * aspect), dot(d, u) / z / t);
    Some([(nx + 1.0) * 0.5 * vp[0], (1.0 - ny) * 0.5 * vp[1]])
}

/// Where the ray through canvas pixel `px` meets the ground (y = 0).
pub fn to_ground(pose: &CameraPose, px: [f32; 2], vp: [f32; 2]) -> Option<[f32; 2]> {
    if vp[0] <= 0.0 || vp[1] <= 0.0 {
        return None;
    }
    let (f, r, u) = basis(pose);
    let t = (pose.fov_y_deg * 0.5).to_radians().tan();
    let aspect = vp[0] / vp[1];
    let (nx, ny) = (px[0] / vp[0] * 2.0 - 1.0, 1.0 - px[1] / vp[1] * 2.0);
    let (a, b) = (nx * t * aspect, ny * t);
    let d = [f[0] + r[0] * a + u[0] * b, f[1] + r[1] * a + u[1] * b, f[2] + r[2] * a + u[2] * b];
    if d[1] > -1e-4 {
        return None;
    }
    let s = -pose.eye[1] / d[1];
    Some([pose.eye[0] + d[0] * s, pose.eye[2] + d[2] * s])
}

// ---- input -------------------------------------------------------------------

/// This tick's keys (edges are presses).
#[derive(Debug, Clone, Copy, Default)]
struct Keys {
    pan: [f32; 2],
    zoom: f32,
    zoom_in: bool,
    zoom_out: bool,
    command: bool,
    command_down: bool,
    attack: bool,
    stop: bool,
    build: bool,
    train: bool,
    heavy: bool,
    army: bool,
    home: bool,
    groups: [bool; 3],
    ctrl: bool,
    shift: bool,
    start: bool,
    pause: bool,
}

impl Keys {
    fn read(i: &Input, a: &Actions) -> Self {
        let p = |id| i.just_pressed(id);
        Self {
            pan: [i.value(a.pan_x).clamp(-1.0, 1.0), i.value(a.pan_y).clamp(-1.0, 1.0)],
            zoom: i.value(a.zoom),
            zoom_in: p(a.zoom_in),
            zoom_out: p(a.zoom_out),
            command: p(a.command),
            command_down: i.down(a.command),
            attack: p(a.attack),
            stop: p(a.stop),
            build: p(a.build),
            train: p(a.train),
            heavy: p(a.train_heavy),
            army: p(a.select_army),
            home: p(a.home),
            groups: [p(a.groups[0]), p(a.groups[1]), p(a.groups[2])],
            ctrl: i.down(a.ctrl),
            shift: i.down(a.shift),
            start: p(a.start),
            pause: p(a.pause),
        }
    }

    /// A touch button pressed this tick.
    fn any_button(&self) -> bool {
        self.zoom_in || self.zoom_out || self.attack || self.stop || self.build || self.train || self.heavy || self.army || self.pause
    }
}

/// What lies under a screen point, as the player may see it.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Hit {
    Own(u32),
    OwnBuilding(u32),
    Enemy(u32),
    Node(u32),
    Ground,
}

fn pick(m: &Match, pose: &CameraPose, vp: [f32; 2], px: [f32; 2], ground: [f32; 2], t: &Tune) -> Hit {
    let fog = &m.fog[PLAYER as usize];
    let mut best: Option<(u32, bool, f32)> = None;
    for u in &m.units {
        if u.team != PLAYER && !fog.visible(&m.grid, u.pos) {
            continue;
        }
        let r = m.types.unit(u.kind, crate::data::us::RADIUS);
        let Some(s) = to_screen(pose, [u.pos[0], r, u.pos[1]], vp) else { continue };
        let d = dist(s, px);
        if d <= t.pick_px * t.hud_scale.max(0.5) && best.is_none_or(|b| d < b.2) {
            best = Some((u.id, u.team == PLAYER, d));
        }
    }
    if let Some((id, own, _)) = best {
        return if own { Hit::Own(id) } else { Hit::Enemy(id) };
    }
    for b in &m.buildings {
        let r = m.building_rect(b);
        if dist_to_rect(ground, r) > 0.0 {
            continue;
        }
        if b.team == PLAYER {
            return Hit::OwnBuilding(b.id);
        }
        if fog.at(&m.grid, b.corner) >= EXPLORED {
            return Hit::Enemy(b.id);
        }
    }
    let cell = m.grid.cell_of(ground);
    if let Some(n) = m.nodes.iter().find(|n| n.amount > 0.0 && (n.cell == cell || dist(n.pos, ground) < 1.4)) {
        return Hit::Node(n.id);
    }
    Hit::Ground
}

fn own_units(m: &Match, ids: &[u32]) -> Vec<u32> {
    m.units.iter().filter(|u| u.team == PLAYER && ids.contains(&u.id)).map(|u| u.id).collect()
}

fn own_buildings(m: &Match, ids: &[u32]) -> Vec<u32> {
    m.buildings.iter().filter(|b| b.team == PLAYER && ids.contains(&b.id)).map(|b| b.id).collect()
}

fn push(m: &mut Match, ids: Vec<u32>, kind: CommandKind) {
    if !ids.is_empty() {
        m.pending.push(Command { team: PLAYER, ids, kind });
    }
}

/// The smart command at `hit` / `ground` for the selection.
fn smart(m: &mut Match, ui: &mut Ui, hit: Hit, ground: [f32; 2], attack_move: bool) {
    let units = own_units(m, &ui.selected);
    let buildings = own_buildings(m, &ui.selected);
    if units.is_empty() {
        if !buildings.is_empty() && matches!(hit, Hit::Ground | Hit::Node(_)) {
            push(m, buildings, CommandKind::Rally { to: ground });
            ui.markers.push(Marker { pos: ground, t: 1.0, attack: false });
        }
        return;
    }
    let mark = |ui: &mut Ui, attack| ui.markers.push(Marker { pos: ground, t: 1.0, attack });
    match hit {
        Hit::Enemy(target) => {
            push(m, units, CommandKind::Attack { target });
            mark(ui, true);
        }
        Hit::Node(node) => {
            push(m, units, CommandKind::Gather { node });
            mark(ui, false);
        }
        Hit::OwnBuilding(site) if is_site(m, PLAYER, site) => {
            let (workers, others): (Vec<u32>, Vec<u32>) =
                units.iter().partition(|id| m.unit(**id).is_some_and(|u| u.kind == WORKER));
            push(m, workers, CommandKind::Help { site });
            push(m, others, CommandKind::Move { to: ground, aggressive: false });
            mark(ui, false);
        }
        _ => {
            push(m, units, CommandKind::Move { to: ground, aggressive: attack_move });
            mark(ui, attack_move);
        }
    }
}

/// The corner a `kind` footprint gets when its centre is under `cell`.
pub fn place_corner(types: &Types, kind: usize, cell: [i32; 2]) -> [i32; 2] {
    let s = types.size(kind);
    [cell[0] - s / 2, cell[1] - s / 2]
}

fn select(ui: &mut Ui, ids: Vec<u32>, add: bool) {
    if add {
        for id in ids {
            if !ui.selected.contains(&id) {
                ui.selected.push(id);
            }
        }
    } else {
        ui.selected = ids;
    }
}

/// Train `which` (0: the producer's first kind, 1: its second) at the
/// selected producers.
fn train(m: &mut Match, ui: &mut Ui, which: usize) {
    let mut by_kind: [Vec<u32>; 3] = Default::default();
    for b in m.buildings.iter().filter(|b| b.team == PLAYER && ui.selected.contains(&b.id)) {
        if let Some(&k) = trains(m, b).get(which) {
            by_kind[k].push(b.id);
        }
    }
    let mut any = false;
    for (kind, ids) in by_kind.into_iter().enumerate() {
        if !ids.is_empty() {
            any = true;
            push(m, ids, CommandKind::Train { kind });
        }
    }
    if !any {
        ui.notice(if which == 0 { "Select a hub or foundry" } else { "Select a foundry" });
    }
}

pub fn control_system(world: &mut World) {
    let Some((k, t)) = crate::tune(world) else { return };
    let dt = world.res::<Time>().fixed_dt;
    let vp = world.try_res::<RenderWorld>().map_or([0.0; 2], |r| r.viewport_css);
    let touching = touch::active(world);
    let (keys, p) = {
        let input = world.res::<Input>();
        (Keys::read(&input, &k.actions), input.primary_pointer())
    };

    let phase = world.res::<Ui>().phase;
    if phase != Phase::Playing {
        if keys.start || (p.released && !keys.any_button()) {
            crate::new_match(world);
            let free_roam = world.res::<Match>().free_roam;
            let fresh = Ui::new(&world.res::<Match>(), &t, free_roam);
            let mut ui = world.res_mut::<Ui>();
            *ui = fresh;
            ui.phase = Phase::Playing;
        }
        return;
    }

    let mut ui = world.res_mut::<Ui>();
    let mut m = world.res_mut::<Match>();
    if keys.pause {
        ui.paused = !ui.paused;
    }
    // Notices from the match and timers.
    for n in std::mem::take(&mut m.notices) {
        ui.notice(n);
    }
    for n in &mut ui.notices {
        n.1 -= dt;
    }
    ui.notices.retain(|n| n.1 > 0.0);
    for mk in &mut ui.markers {
        mk.t -= dt / t.marker_s.max(0.05);
    }
    ui.markers.retain(|mk| mk.t > 0.0);
    if ui.paused {
        ui.drag = None;
        return;
    }
    // Forget what is gone.
    let alive = |m: &Match, id: &u32| m.unit(*id).is_some() || m.building(*id).is_some();
    let sel: Vec<u32> = ui.selected.iter().copied().filter(|id| alive(&m, id)).collect();
    ui.selected = sel;
    for g in 0..3 {
        let kept: Vec<u32> = ui.groups[g].iter().copied().filter(|id| alive(&m, id)).collect();
        ui.groups[g] = kept;
    }

    // Camera.
    let zmax = t.cam_zoom_max.max(t.cam_zoom_min);
    let speed = t.cam_pan_speed * (ui.zoom / t.cam_zoom_start.max(1.0)).max(0.3);
    ui.cam[0] += keys.pan[0] * speed * dt;
    ui.cam[1] += keys.pan[1] * speed * dt;
    let step = t.cam_zoom_step.max(1.01);
    if keys.zoom > 0.0 || keys.zoom_out {
        ui.zoom *= step;
    }
    if keys.zoom < 0.0 || keys.zoom_in {
        ui.zoom /= step;
    }
    ui.zoom = ui.zoom.clamp(t.cam_zoom_min, zmax);
    if keys.home
        && let Some(h) = m.hub(PLAYER) {
            ui.cam = rect_center(m.building_rect(h));
        }
    ui.cam = clamp_cam(&m, &t, ui.cam);
    let aspect = if vp[1] > 0.0 { vp[0] / vp[1] } else { 16.0 / 9.0 };
    let cam = pose(ui.cam, ui.zoom, &t, aspect);

    // Keys.
    if keys.army {
        ui.selected = m.units.iter().filter(|u| u.team == PLAYER && (u.kind == TROOPER || u.kind == RUMBLER)).map(|u| u.id).collect();
    }
    for g in 0..3 {
        if keys.groups[g] {
            if keys.ctrl {
                ui.groups[g] = ui.selected.clone();
            } else {
                ui.selected = ui.groups[g].clone();
            }
        }
    }
    if keys.attack {
        ui.mode = if ui.mode == Mode::Attack || own_units(&m, &ui.selected).is_empty() { Mode::Normal } else { Mode::Attack };
    }
    if keys.stop {
        let ids: Vec<u32> = ui.selected.clone();
        push(&mut m, ids, CommandKind::Stop);
        ui.mode = Mode::Normal;
    }
    if keys.build {
        ui.mode = match ui.mode {
            Mode::Place(k) if k == FOUNDRY => Mode::Place(BASTION),
            Mode::Place(_) => Mode::Normal,
            _ => Mode::Place(FOUNDRY),
        };
    }
    if keys.train {
        train(&mut m, &mut ui, 0);
    }
    if keys.heavy {
        train(&mut m, &mut ui, 1);
    }

    // The pointer.
    let ground = to_ground(&cam, p.pos, vp);
    ui.hover = ground.map(|g| m.grid.cell_of(g));
    ui.hover_ok = match (ui.mode, ui.hover) {
        (Mode::Place(kind), Some(c)) => m.can_place(PLAYER, kind, place_corner(&m.types, kind, c)).is_ok(),
        _ => false,
    };
    if keys.command {
        // Right click: the smart command (mouse only).
        if let Some(g) = ground {
            let hit = pick(&m, &cam, vp, p.pos, g, &t);
            match ui.mode {
                Mode::Place(_) | Mode::Attack => ui.mode = Mode::Normal,
                Mode::Normal => smart(&mut m, &mut ui, hit, g, false),
            }
        }
    }
    if p.pressed {
        ui.cmd_seen = keys.command_down;
        ui.ignore_press = touching && keys.any_button();
    }
    if keys.command_down {
        ui.cmd_seen = true;
    }
    let moved = dist(p.pos, p.press_pos) > t.drag_px;
    if p.down && !ui.cmd_seen && !ui.ignore_press && ui.mode == Mode::Normal && moved {
        ui.drag = Some((p.press_pos, p.pos));
    }
    if p.released {
        let skip = ui.cmd_seen || ui.ignore_press;
        let dragged = ui.drag.take();
        ui.cmd_seen = false;
        ui.ignore_press = false;
        if !skip {
            if let Some((a, b)) = dragged {
                box_select(&m, &mut ui, &cam, vp, a, b, keys.shift);
            } else if let Some(g) = ground {
                tap(&mut m, &mut ui, &cam, vp, p.pos, g, &t, p.kind == PointerKind::Touch, keys.shift);
            }
        }
    }
}

fn box_select(m: &Match, ui: &mut Ui, cam: &CameraPose, vp: [f32; 2], a: [f32; 2], b: [f32; 2], add: bool) {
    let (x0, x1) = (a[0].min(b[0]), a[0].max(b[0]));
    let (y0, y1) = (a[1].min(b[1]), a[1].max(b[1]));
    let ids: Vec<u32> = m
        .units
        .iter()
        .filter(|u| u.team == PLAYER)
        .filter(|u| to_screen(cam, [u.pos[0], 0.4, u.pos[1]], vp).is_some_and(|s| s[0] >= x0 && s[0] <= x1 && s[1] >= y0 && s[1] <= y1))
        .map(|u| u.id)
        .collect();
    select(ui, ids, add);
}

#[allow(clippy::too_many_arguments)]
fn tap(m: &mut Match, ui: &mut Ui, cam: &CameraPose, vp: [f32; 2], px: [f32; 2], g: [f32; 2], t: &Tune, touch: bool, add: bool) {
    let hit = pick(m, cam, vp, px, g, t);
    match ui.mode {
        Mode::Attack => {
            ui.mode = Mode::Normal;
            smart(m, ui, if matches!(hit, Hit::Enemy(_)) { hit } else { Hit::Ground }, g, true);
        }
        Mode::Place(kind) => {
            let corner = place_corner(&m.types, kind, m.grid.cell_of(g));
            match m.can_place(PLAYER, kind, corner) {
                Ok(()) => {
                    let workers: Vec<u32> = m.units.iter().filter(|u| u.team == PLAYER && u.kind == WORKER && ui.selected.contains(&u.id)).map(|u| u.id).collect();
                    // No workers selected: the sim sends the nearest one.
                    m.pending.push(Command { team: PLAYER, ids: workers, kind: CommandKind::Build { kind, corner } });
                    ui.markers.push(Marker { pos: g, t: 1.0, attack: false });
                    ui.mode = Mode::Normal;
                }
                Err(e) => ui.notice(e),
            }
        }
        Mode::Normal => {
            let has_units = !own_units(m, &ui.selected).is_empty();
            match hit {
                Hit::Own(id) => select(ui, vec![id], add),
                Hit::OwnBuilding(id) if !(touch && has_units && is_site(m, PLAYER, id)) => select(ui, vec![id], add),
                _ if touch && !ui.selected.is_empty() => smart(m, ui, hit, g, false),
                _ if !add => ui.selected.clear(),
                _ => {}
            }
        }
    }
}

/// The HUD's line for the current mode.
pub fn mode_line(ui: &Ui) -> &'static str {
    match ui.mode {
        Mode::Normal => "",
        Mode::Attack => "Attack: pick a target or ground",
        Mode::Place(k) if k == FOUNDRY => "Place a foundry (B: bastion)",
        Mode::Place(_) => "Place a bastion (B: cancel)",
    }
}
