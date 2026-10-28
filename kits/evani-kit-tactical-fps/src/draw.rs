//! Drawing of the Hexagon fortress and the match (a `Stage::Frame` system):
//! floor tiles and cover cells in concrete, cut by the holes through the kit
//! material ([`crate::cut`]) with crater walls inside; walls, dividers and
//! rails in worn steel; hazard markings in paint; soldiers in team paint;
//! tracers, rockets, grenades and blast flashes; and in first person the
//! held weapon in the view-model pass (its own lens and depth, so it never
//! clips a wall). Primitive meshes only.
//!
//! Camera: first person from the player's eye in a match, a slow orbit over
//! the fortress on the title and results, a raised look at the body while
//! dead. A capture moment can pin the camera with [`MomentCam`].
//!
//! The kit never depends on a look: it asks the loaded look for materials by
//! role name (`concrete`, `concrete_dark`, `steel_worn` / `steel` / `metal`,
//! `paint_blue` + `paint_orange` / `team`, `lamp` / `glow`, `muzzle_flash`,
//! `dirt` / `dust`) and otherwise uses the look's brightest lit (and
//! brightest unlit) material as a neutral base. On a material made for its
//! role the look's own colour shows; otherwise the look bible's palette
//! (`kits/tactical-fps/target/look-bible.md`, linear) tints it.

use evani_core::camera::{CameraPath, CameraPose};
use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::params::ParamRegistry;
use evani_core::plugin::LookData;
use evani_core::time::Time;
use evani_render::kit_material::KitMaterial;
use evani_render::math::quat_axis_angle;
use evani_render::{MeshId, MeshInstance, RenderWorld};

use crate::arena::{Arena, COS30, S60, TAN30, to_world};
use crate::cut::{CutState, WALL_SCALE};
use crate::body::aim_dir;
use crate::match_rules::{BLAST_LIFE, Match, Phase, ShotEnd};
use crate::tune::{GRENADES, ROCKETS, SNIPER};

/// Newest holes drawn (older ones still cut the concrete; this only bounds the draw).
const HOLES_DRAWN: usize = 600;
/// The held weapon's own vertical field of view (degrees).
const VIEW_FOV: f32 = 55.0;
/// How long the muzzle flash shows after a shot (s, sim time).
const FLASH_S: f32 = 0.06;

/// A pinned camera for capture moments (`None`: the game's own camera).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MomentCam(pub Option<CameraPose>);

/// sRGB hex to linear RGBA.
fn srgb(hex: u32) -> [f32; 4] {
    let ch = |s: u32| {
        let c = ((hex >> s) & 0xff) as f32 / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    [ch(16), ch(8), ch(0), 1.0]
}

/// The look bible's palette, linear.
struct Palette {
    concrete: [f32; 4],
    broken: [f32; 4],
    metal: [f32; 4],
    team: [[f32; 4]; 2],
    core: [f32; 4],
    hazard: [f32; 4],
}

impl Palette {
    fn new() -> Self {
        Palette {
            concrete: srgb(0xa9a39a),
            broken: srgb(0x6e675e),
            metal: srgb(0x3b4250),
            team: [srgb(0x2f7bff), srgb(0xff7a1a)],
            core: srgb(0xfff2a8),
            hazard: srgb(0xe8c547),
        }
    }
}

const WHITE: [f32; 4] = [1.0; 4];

/// A role's material slot, and whether the look has a material made for it
/// (then the look's own colour shows: the instance colour is white).
#[derive(Debug, Clone, Copy)]
struct Role {
    slot: u32,
    own: bool,
}

impl Role {
    /// The instance colour: white on the look's own material, else the palette's.
    fn color(self, pal: [f32; 4]) -> [f32; 4] {
        if self.own { WHITE } else { pal }
    }
}

/// Material slots for each role, found by name in whatever look is loaded.
struct Mats {
    concrete: Role,
    metal: Role,
    team: [Role; 2],
    glow: Role,
    flash: Role,
    dust: Role,
    /// The held weapon's grip and the glove holding it.
    grip: Role,
    glove: Role,
    /// A lit, non-metal paint for markings (hazard stripes, core zones), and its base colour.
    paint: u32,
    paint_base: [f32; 4],
    /// The concrete slot the kit material cuts (only the look's own `concrete`).
    cut: Option<u32>,
    /// The crater-wall slot (`concrete_dark`).
    wall: Option<u32>,
}

impl Mats {
    /// The instance colour that makes the paint material show `want`.
    fn tint(&self, want: [f32; 4]) -> [f32; 4] {
        let c = |i: usize| (want[i] / self.paint_base[i].max(0.02)).min(8.0);
        [c(0), c(1), c(2), want[3]]
    }
}

fn luminance(c: [f32; 4]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

fn mats(look: &LookData) -> Mats {
    let find = |n: &str| look.materials.iter().position(|m| m.name == n).map(|i| i as u32);
    // The first name found; `own`: a material made for the role (generic names keep the palette tint).
    let pick = |names: &[(&str, bool)]| names.iter().find_map(|&(n, own)| find(n).map(|slot| Role { slot, own }));
    let brightest = |unlit: bool| {
        look.materials
            .iter()
            .enumerate()
            .filter(|(_, m)| m.unlit == unlit)
            .max_by(|a, b| luminance(a.1.base_color).total_cmp(&luminance(b.1.base_color)))
            .map(|(i, _)| i as u32)
    };
    let neutral = Role { slot: brightest(false).unwrap_or(0), own: false };
    let glow = pick(&[("lamp", true), ("glow", false)]).unwrap_or(Role { slot: brightest(true).unwrap_or(neutral.slot), own: false });
    let team = match (find("paint_blue"), find("paint_orange")) {
        (Some(b), Some(o)) => [Role { slot: b, own: true }, Role { slot: o, own: true }],
        _ => [pick(&[("team", false)]).unwrap_or(neutral); 2],
    };
    let cut = find("concrete");
    let wall = find("concrete_dark").filter(|&w| cut.is_some_and(|c| c != w));
    // Paint: the brightest lit non-metal that is not cut or a crater wall.
    let paint = look
        .materials
        .iter()
        .enumerate()
        .filter(|&(i, m)| !m.unlit && m.metallic < 0.5 && Some(i as u32) != cut && Some(i as u32) != wall)
        .max_by(|a, b| luminance(a.1.base_color).total_cmp(&luminance(b.1.base_color)))
        .map_or(neutral.slot, |(i, _)| i as u32);
    let paint_base = look.materials.get(paint as usize).map_or(WHITE, |m| m.base_color);
    let metal = pick(&[("steel_worn", true), ("steel", true), ("metal", false)]).unwrap_or(neutral);
    Mats {
        concrete: pick(&[("concrete", true)]).unwrap_or(neutral),
        metal,
        grip: pick(&[("rubber", true)]).unwrap_or(metal),
        glove: pick(&[("cloth", true)]).unwrap_or(metal),
        team,
        glow,
        flash: pick(&[("muzzle_flash", true)]).unwrap_or(glow),
        dust: pick(&[("dirt", true), ("dust", false)]).unwrap_or(neutral),
        paint,
        paint_base,
        cut,
        wall,
    }
}

/// Rotation turning a box's local X onto the direction at angle `a` in the XZ plane.
fn yaw_rot(a: f32) -> [f32; 4] {
    quat_axis_angle([0.0, 1.0, 0.0], -a)
}

/// A box from its centre, size (x along `a`, y up, z across) and colour.
fn boxed(mesh: MeshId, c: [f32; 3], size: [f32; 3], a: f32, material: u32, color: [f32; 4]) -> MeshInstance {
    let mut m = MeshInstance::new(mesh, c);
    m.rotation = yaw_rot(a);
    m.scale = size;
    m.material = material;
    m.color = color;
    m
}

/// The path live's bench, compare and capture tools fly: the title orbit
/// (well outside the outer wall, above the top level, one turn a minute).
pub fn camera_path() -> CameraPath {
    let mut p = CameraPath::orbit([0.0, 8.0, 0.0], 60.0, 32.0, 60.0);
    p.fov_y_deg = 60.0;
    p
}

/// The `Stage::Frame` system that draws the match.
pub fn system() -> SystemDesc {
    SystemDesc::new("tactical_fps.draw", Stage::Frame, draw_scene)
}

/// Draw everything for this frame.
pub fn draw_scene(world: &mut World) {
    let Some(kit) = world.try_res::<crate::TacticalFps>().map(|k| k.handles) else {
        return;
    };
    let t = kit.read(&world.res::<ParamRegistry>());
    let now = world.res::<Time>().render_seconds() as f32;
    let Some(m) = world.try_res::<LookData>().map(|l| mats(&l)) else {
        return;
    };
    let pin = world.try_res::<MomentCam>().and_then(|c| c.0);
    let (Some(arena), Some(game)) = (world.try_res::<Arena>(), world.try_res::<Match>()) else {
        return;
    };
    // The hole cut-out follows the arena (code once, data on change).
    if let (Some(mut km), Some(mut cs)) = (world.try_res_mut::<KitMaterial>(), world.try_res_mut::<CutState>()) {
        cs.sync(&mut km, &arena, m.cut, m.wall);
    }
    let pal = Palette::new();
    let Some(mut rw) = world.try_res_mut::<RenderWorld>() else { return };
    rw.camera = pin.unwrap_or_else(|| camera(&arena, &game, t.fov_deg, t.title_orbit_speed, now));
    let first_person = pin.is_none() && matches!(game.phase, Phase::Pre | Phase::Play) && game.soldiers.first().is_some_and(|s| s.alive());
    fortress(&mut rw, &arena, &m, &pal);
    cover(&mut rw, &arena, &m, &pal);
    if m.cut.is_some() {
        crater_walls(&mut rw, &arena, &m);
    } else {
        holes(&mut rw, &arena, &pal);
    }
    cores(&mut rw, &arena, &game, &m, &pal, t.hold_s);
    for (i, s) in game.soldiers.iter().enumerate() {
        if !s.alive() || (i == 0 && first_person) {
            continue;
        }
        let p = s.body.p;
        let role = m.team[s.team & 1];
        let col = role.color(pal.team[s.team & 1]);
        let (r, h) = (arena.def.hit_r, arena.def.hit_h);
        let mut body = MeshInstance::new(MeshId::CYLINDER, [p[0], p[1] + (h - 2.0 * r) * 0.5 + r * 0.5, p[2]]);
        body.scale = [2.0 * r, h - r, 2.0 * r];
        body.material = role.slot;
        body.color = if s.shield > 0.0 { [col[0] * 1.6, col[1] * 1.6, col[2] * 1.6, 1.0] } else { col };
        rw.draw_mesh(body);
        let mut head = MeshInstance::new(MeshId::SPHERE, [p[0], p[1] + h - r * 0.9, p[2]]);
        head.scale = [r * 1.8; 3];
        head.material = role.slot;
        head.color = col;
        rw.draw_mesh(head);
        // The aim, so a bot's facing reads at a distance.
        let d = aim_dir(s.body.yaw, s.body.pitch);
        let e = s.body.eye(&arena);
        let gun = [e[0] + d[0] * 0.45, e[1] - 0.25 + d[1] * 0.45, e[2] + d[2] * 0.45];
        let mut g = boxed(MeshId::CUBE, gun, [0.6, 0.1, 0.1], s.body.yaw, m.metal.slot, m.metal.color(pal.metal));
        g.rotation = evani_render::math::quat_mul(yaw_rot(s.body.yaw), quat_axis_angle([0.0, 0.0, 1.0], s.body.pitch));
        rw.draw_mesh(g);
        rw.draw_blob([p[0], p[1] + 0.02, p[2]], r * 1.3);
    }
    rw.view_model.fov_y_deg = VIEW_FOV;
    if first_person && let Some(me) = game.soldiers.first() {
        held_weapon(&mut rw, &arena, &game, me, &m, &pal, now);
    }
    for sh in &game.shots {
        let (a, b) = (sh.from, sh.to);
        let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if len < 0.05 {
            continue;
        }
        let mid = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5 - 0.1, (a[2] + b[2]) * 0.5];
        let yaw = d[2].atan2(d[0]);
        let pitch = d[1].atan2(d[0].hypot(d[2]));
        let mut tr = MeshInstance::new(MeshId::CUBE, mid);
        tr.rotation = evani_render::math::quat_mul(yaw_rot(yaw), quat_axis_angle([0.0, 0.0, 1.0], pitch));
        tr.scale = [len, 0.03, 0.03];
        tr.material = m.glow.slot;
        tr.color = m.glow.color(pal.hazard);
        rw.draw_mesh(tr);
        if sh.end == ShotEnd::Concrete || sh.end == ShotEnd::Metal {
            // Dust off concrete, a puff of the metal off metal (neither is cut by the holes).
            let mut puff = MeshInstance::new(MeshId::LOW_SPHERE, b);
            puff.scale = [0.25; 3];
            let role = if sh.end == ShotEnd::Concrete { m.dust } else { m.metal };
            puff.material = role.slot;
            puff.color = role.color(if sh.end == ShotEnd::Concrete { pal.concrete } else { pal.metal });
            rw.draw_mesh(puff);
        }
    }
    for pr in &game.projectiles {
        let mut p = MeshInstance::new(if pr.weapon == GRENADES { MeshId::LOW_SPHERE } else { MeshId::CYLINDER }, pr.p);
        p.scale = if pr.weapon == GRENADES { [0.25; 3] } else { [0.18, 0.5, 0.18] };
        if pr.weapon != GRENADES {
            let yaw = pr.v[2].atan2(pr.v[0]);
            let pitch = pr.v[1].atan2(pr.v[0].hypot(pr.v[2]));
            // The cylinder runs along Y: lay it along the flight.
            p.rotation = evani_render::math::quat_mul(
                evani_render::math::quat_mul(yaw_rot(yaw), quat_axis_angle([0.0, 0.0, 1.0], pitch)),
                quat_axis_angle([0.0, 0.0, 1.0], -core::f32::consts::FRAC_PI_2),
            );
        }
        let role = m.team[pr.team & 1];
        p.material = role.slot;
        p.color = role.color(pal.team[pr.team & 1]);
        rw.draw_mesh(p);
    }
    for b in &game.blasts {
        let f = (b.age / BLAST_LIFE).clamp(0.0, 1.0);
        let mut ball = MeshInstance::new(MeshId::SPHERE, b.p);
        ball.scale = [t.blast_radius * 2.0 * (0.4 + 0.6 * f); 3];
        ball.material = m.flash.slot;
        let c = m.flash.color(pal.hazard);
        ball.color = [c[0], c[1] * (1.0 - f * 0.5), c[2], 1.0 - f];
        rw.draw_mesh(ball);
    }
}

/// Rotate `v` by the unit quaternion `q`.
fn rot(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let u = [q[0], q[1], q[2]];
    let cross = |a: [f32; 3], b: [f32; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let t = cross(u, v).map(|c| 2.0 * c);
    let c = cross(u, t);
    [v[0] + q[3] * t[0] + c[0], v[1] + q[3] * t[1] + c[1], v[2] + q[3] * t[2] + c[2]]
}

/// One part of the held weapon: mesh, centre and size in the weapon's own
/// frame (camera axes, -z forward), whether it runs along z (a cylinder),
/// and its role.
type Part = (MeshId, [f32; 3], [f32; 3], bool, Role);

/// The player's weapon in the view-model pass (camera space: x right, y up,
/// -z forward). Low and to the right, it bobs with the walk, kicks back and
/// up with the recoil, and shows a muzzle flash for a moment after firing.
fn held_weapon(rw: &mut RenderWorld, arena: &Arena, game: &Match, me: &crate::match_rules::Soldier, m: &Mats, pal: &Palette, now: f32) {
    let b = &me.body;
    // Walk bob (render time: the drawing only).
    let speed = (b.v[0].hypot(b.v[2]) / 5.0).clamp(0.0, 1.0) * if b.grounded { 1.0 } else { 0.3 };
    let ph = now * 9.0;
    let bob = [ph.sin() * 0.010 * speed, -(ph.cos().abs()) * 0.008 * speed, 0.0];
    // Recoil: back and nose up.
    let kick = me.recoil.clamp(0.0, 0.3);
    let at = [0.22 + bob[0], -0.2 + bob[1] - kick * 0.05, -0.45 + kick * 0.35];
    let q = quat_axis_angle([1.0, 0.0, 0.0], kick * 1.2);
    let team = m.team[me.team & 1];
    let along: [f32; 4] = quat_axis_angle([1.0, 0.0, 0.0], -core::f32::consts::FRAC_PI_2);
    let (parts, muzzle): (&[Part], [f32; 3]) = match b.weapon {
        SNIPER => (
            &[
                (MeshId::CUBE, [0.0, 0.0, 0.0], [0.055, 0.075, 0.42], false, m.metal),
                (MeshId::CYLINDER, [0.0, 0.012, -0.42], [0.024, 0.44, 0.024], true, m.metal),
                (MeshId::CYLINDER, [0.0, 0.07, -0.02], [0.045, 0.22, 0.045], true, m.grip),
                (MeshId::CUBE, [0.0, -0.025, 0.25], [0.05, 0.1, 0.16], false, m.grip),
                (MeshId::CUBE, [0.0, -0.07, 0.06], [0.035, 0.1, 0.045], false, m.grip),
                (MeshId::CUBE, [-0.005, -0.1, 0.07], [0.06, 0.07, 0.07], false, m.glove),
            ],
            [0.0, 0.012, -0.66],
        ),
        ROCKETS => (
            &[
                (MeshId::CYLINDER, [0.0, 0.03, -0.12], [0.12, 0.72, 0.12], true, m.metal),
                (MeshId::CYLINDER, [0.0, 0.03, -0.47], [0.135, 0.05, 0.135], true, team),
                (MeshId::CUBE, [0.0, -0.06, -0.05], [0.035, 0.11, 0.045], false, m.grip),
                (MeshId::CUBE, [-0.005, -0.1, -0.04], [0.06, 0.07, 0.07], false, m.glove),
            ],
            [0.0, 0.03, -0.5],
        ),
        GRENADES => (
            &[
                (MeshId::LOW_SPHERE, [0.0, 0.0, -0.05], [0.075, 0.09, 0.075], false, team),
                (MeshId::CYLINDER, [0.0, 0.05, -0.05], [0.03, 0.025, 0.03], false, m.metal),
                (MeshId::CUBE, [0.0, -0.035, -0.03], [0.08, 0.07, 0.09], false, m.glove),
            ],
            [0.0, 0.0, -0.12],
        ),
        _ => (
            &[
                (MeshId::CUBE, [0.0, 0.02, -0.04], [0.042, 0.048, 0.2], false, m.metal),
                (MeshId::CYLINDER, [0.0, 0.025, -0.15], [0.016, 0.04, 0.016], true, m.metal),
                (MeshId::CUBE, [0.0, -0.045, 0.03], [0.038, 0.1, 0.05], false, m.grip),
                (MeshId::CUBE, [-0.004, -0.07, 0.035], [0.06, 0.07, 0.07], false, m.glove),
            ],
            [0.0, 0.025, -0.17],
        ),
    };
    let place = |c: [f32; 3]| {
        let r = rot(q, c);
        [at[0] + r[0], at[1] + r[1], at[2] + r[2]]
    };
    for &(mesh, c, size, long, role) in parts {
        let mut p = MeshInstance::new(mesh, place(c));
        p.rotation = if long { evani_render::math::quat_mul(q, along) } else { q };
        p.scale = size;
        p.material = role.slot;
        p.color = role.color(if role.slot == team.slot { pal.team[me.team & 1] } else { pal.metal });
        rw.draw_view_model(p);
    }
    // Muzzle flash: a shot of ours left the muzzle this moment (thrown grenades show none).
    let e = b.eye(arena);
    // A shot starts at its shooter's eye; nobody else's eye comes this close to ours.
    let near = |p: [f32; 3]| (p[0] - e[0]).powi(2) + (p[1] - e[1]).powi(2) + (p[2] - e[2]).powi(2) < 0.36;
    let fired = game.shots.iter().any(|s| s.age < FLASH_S && s.team == me.team && near(s.from))
        || game.projectiles.iter().any(|p| p.owner == 0 && p.weapon == ROCKETS && p.age < FLASH_S);
    if fired && b.weapon != GRENADES {
        let mut f = MeshInstance::new(MeshId::LOW_SPHERE, place(muzzle));
        f.scale = [0.09, 0.09, 0.14];
        f.rotation = q;
        f.material = m.flash.slot;
        f.color = m.flash.color(pal.hazard);
        rw.draw_view_model(f);
    }
}

/// The camera for this frame.
fn camera(arena: &Arena, game: &Match, fov: f32, orbit: f32, now: f32) -> CameraPose {
    let d = &arena.def;
    let me = game.soldiers.first();
    if matches!(game.phase, Phase::Pre | Phase::Play)
        && let Some(s) = me {
            let e = s.body.eye(arena);
            if s.alive() {
                let dir = aim_dir(s.body.yaw + s.recoil_yaw, (s.body.pitch + s.recoil).clamp(-1.5, 1.5));
                return CameraPose { eye: e, target: [e[0] + dir[0], e[1] + dir[1], e[2] + dir[2]], up: [0.0, 1.0, 0.0], fov_y_deg: fov };
            }
            // Dead: look down at the body from above and behind.
            let back = aim_dir(s.body.yaw, 0.0);
            let eye = [e[0] - back[0] * 4.0, e[1] + 3.0, e[2] - back[2] * 4.0];
            return CameraPose { eye, target: s.body.p, up: [0.0, 1.0, 0.0], fov_y_deg: fov };
        }
    let a = now * orbit;
    let r = d.a_out * 1.6;
    CameraPose {
        eye: [a.cos() * r, d.fy[2] + d.top_wall + 18.0, a.sin() * r],
        target: [0.0, d.fy[1], 0.0],
        up: [0.0, 1.0, 0.0],
        fov_y_deg: 60.0,
    }
}

/// Height of a level's walls.
fn wall_h(arena: &Arena, level: usize) -> f32 {
    let d = &arena.def;
    if level == 2 { d.top_wall } else { d.fy[level + 1] - d.slab - d.fy[level] }
}

/// The world angle of sector `k` on `level` (the middle level turns with the ring).
fn sector_angle(arena: &Arena, level: usize, k: usize) -> f32 {
    k as f32 * S60 + if level == 1 { arena.ang } else { 0.0 }
}

/// A level-frame point to the world.
fn world_of(arena: &Arena, level: usize, x: f32, y: f32, z: f32) -> [f32; 3] {
    let (wx, wz) = arena.from_level(level, x, z);
    [wx, y, wz]
}

/// Floors, outer walls, dividers, rails and lift frames.
fn fortress(rw: &mut RenderWorld, arena: &Arena, m: &Mats, pal: &Palette) {
    let d = &arena.def;
    // Concrete floor tiles.
    for pc in arena.pieces.iter().filter(|p| p.floor) {
        let [u0, u1, v0, v1, y0, y1] = pc.bounds;
        if u1 <= u0 || v1 <= v0 {
            continue;
        }
        let (x, z) = to_world(pc.sector, (u0 + u1) * 0.5, (v0 + v1) * 0.5);
        let c = world_of(arena, pc.level, x, (y0 + y1) * 0.5, z);
        let a = sector_angle(arena, pc.level, pc.sector);
        rw.draw_mesh(boxed(MeshId::CUBE, c, [u1 - u0 - 0.04, y1 - y0, v1 - v0 - 0.04], a, m.concrete.slot, m.concrete.color(pal.concrete)));
    }
    for level in 0..3 {
        let h = wall_h(arena, level);
        let base = d.fy[level];
        for k in 0..6 {
            let a = sector_angle(arena, level, k);
            // Outer wall.
            let (x, z) = to_world(k, d.a_out + 0.25, 0.0);
            let c = world_of(arena, level, x, base - d.slab + (h + d.slab) * 0.5, z);
            rw.draw_mesh(boxed(MeshId::CUBE, c, [0.5, h + d.slab, 2.0 * (d.a_out + 0.5) * TAN30], a, m.metal.slot, m.metal.color(pal.metal)));
            // A metal underside plate under the concrete tiles (trim between them).
            let (x, z) = to_world(k, (d.a_in + d.a_out) * 0.5, 0.0);
            let c = world_of(arena, level, x, base - d.slab - 0.1, z);
            rw.draw_mesh(boxed(MeshId::CUBE, c, [d.a_out - d.a_in, 0.2, 2.0 * d.a_in * TAN30], a, m.metal.slot, m.metal.color(pal.metal)));
            // Inner rail on the bottom and top levels; hazard stripe on the ring cars.
            let (x, z) = to_world(k, d.a_in + d.rail_t * 0.5, 0.0);
            let len = 2.0 * d.a_in * TAN30;
            if level != 1 {
                let c = world_of(arena, level, x, base + d.rail_h * 0.5, z);
                rw.draw_mesh(boxed(MeshId::CUBE, c, [d.rail_t, d.rail_h, len], a, m.metal.slot, m.metal.color(pal.metal)));
            } else {
                let c = world_of(arena, level, x, base + 0.01, z);
                rw.draw_decal(boxed(MeshId::CUBE, c, [d.rail_t.max(0.3), 0.02, len], a, m.paint, m.tint(pal.hazard)));
            }
            // Divider on the edge between sector k and k + 1, with its doorway on doored levels.
            let th = sector_angle(arena, level, k) + S60 * 0.5;
            let (s0, s1) = (d.a_in / COS30, d.a_out / COS30);
            let (ct, st) = (th.cos(), th.sin());
            let piece = |rw: &mut RenderWorld, a0: f32, a1: f32, y0: f32, y1: f32| {
                if a1 - a0 < 0.05 || y1 - y0 < 0.05 {
                    return;
                }
                let s = (a0 + a1) * 0.5;
                let c = [ct * s, base + (y0 + y1) * 0.5, st * s];
                rw.draw_mesh(boxed(MeshId::CUBE, c, [a1 - a0, y1 - y0, 2.0 * d.divh], th, m.metal.slot, m.metal.color(pal.metal)));
            };
            if d.divs[level][k] == 1 {
                piece(rw, s0, d.door0, 0.0, h);
                piece(rw, d.door1, s1, 0.0, h);
                piece(rw, d.door0, d.door1, d.door_h, h);
            } else {
                piece(rw, s0, s1, 0.0, h);
            }
            // Lift frames: up from this level (the shaft at +v rises to the ring, -v to the top).
            for (lv, v) in [(1usize, d.lift_v), (2, -d.lift_v)] {
                if lv != level + 1 && lv != level {
                    continue;
                }
                let (x, z) = to_world(k, d.lift_u, v);
                let c = world_of(arena, level, x, base + 0.02, z);
                let s = 2.0 * (d.lift_h + d.lift_frame);
                rw.draw_decal(boxed(MeshId::CUBE, c, [s, 0.04, s], a, m.paint, m.tint(pal.hazard)));
            }
        }
    }
}

/// Cover cells still standing. With the cut-out every one is drawn (the
/// kit material removes what the holes took); without it a cell whose
/// centre is holed is left out.
fn cover(rw: &mut RenderWorld, arena: &Arena, m: &Mats, pal: &Palette) {
    for (i, c) in arena.cells.iter().enumerate() {
        if arena.gone.get(i).copied().unwrap_or(true) {
            continue;
        }
        let p = arena.cell_centre(c);
        if m.cut.is_none() && arena.holes.holed(c.level, p[0], p[1], p[2]) {
            continue;
        }
        let w = world_of(arena, c.level, p[0], p[1], p[2]);
        let a = sector_angle(arena, c.level, c.sector);
        rw.draw_mesh(boxed(MeshId::BEVEL_CUBE, w, [1.0; 3], a, m.concrete.slot, m.concrete.color(pal.concrete)));
    }
}

/// The crater walls of the newest holes: inside-out spheres just outside
/// each broken outline, in the wall material, which the kit material keeps
/// only inside solid concrete (src/cut.rs), so a hole shows a broken inside.
fn crater_walls(rw: &mut RenderWorld, arena: &Arena, m: &Mats) {
    let Some(wall) = m.wall else {
        return;
    };
    let list = &arena.holes.list;
    for h in list.iter().skip(list.len().saturating_sub(HOLES_DRAWN)) {
        let p = world_of(arena, h.level, h.p[0], h.p[1], h.p[2]);
        let mut s = MeshInstance::new(if h.r < 0.5 { MeshId::LOW_SPHERE } else { MeshId::SPHERE }, p);
        // Negative scale: the normals face the centre (lit as a hollow).
        s.scale = [-h.r * WALL_SCALE; 3];
        s.material = wall;
        rw.draw_mesh(s);
    }
}

/// Without the cut-out: the newest holes as dark balls (the broken concrete face).
fn holes(rw: &mut RenderWorld, arena: &Arena, pal: &Palette) {
    let list = &arena.holes.list;
    for h in list.iter().skip(list.len().saturating_sub(HOLES_DRAWN)) {
        let p = world_of(arena, h.level, h.p[0], h.p[1], h.p[2]);
        let mut s = MeshInstance::new(if h.r < 0.5 { MeshId::LOW_SPHERE } else { MeshId::SPHERE }, p);
        // A touch smaller than the cut, so it shades the hole's inside without poking out.
        s.scale = [h.r * 1.9; 3];
        s.color = pal.broken;
        rw.draw_mesh(s);
    }
}

/// Both teams' core zones on the top level, lit by how far the enemy's hold has got.
fn cores(rw: &mut RenderWorld, arena: &Arena, game: &Match, m: &Mats, pal: &Palette, hold_s: f32) {
    let d = &arena.def;
    for team in 0..2 {
        let c = arena.core_centre(team);
        let held = (game.hold[1 - team] / hold_s.max(0.1)).clamp(0.0, 1.0);
        let col = pal.team[team];
        let mix = |a: f32, b: f32| a + (b - a) * held;
        let mut ring = MeshInstance::new(MeshId::CYLINDER, [c[0], c[1] + 0.03, c[2]]);
        ring.scale = [d.core_r * 2.0, 0.04, d.core_r * 2.0];
        // Painted, not lit by a lamp: the look's emissive is not tinted, and the zone must read its team.
        ring.material = m.paint;
        ring.color = m.tint([mix(col[0], pal.core[0]), mix(col[1], pal.core[1]), mix(col[2], pal.core[2]), 1.0]);
        rw.draw_decal(ring);
    }
}
