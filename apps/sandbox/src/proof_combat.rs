//! The combat proof (sandbox feature `combat-proof`; never in the default
//! build): a bot with a rifle against a standing target in a small
//! triangle level built with `evani_physics::collide::WorldBuilder` (a slab,
//! a dividing wall with a gap at its far end, a crate), every fixed step at
//! the live `combat.*` values.
//!
//! The lap: the target stands behind the wall, out of the bot's sight. The
//! bot hunts a goal round the far end of the wall along a route from
//! `evani_combat::path` (Dijkstra over a four-node net, then a string pull
//! that skips what a straight walk clears), walking with the collide
//! character mover. The wall blocks its line of sight until it rounds the
//! end; then it turns, waits out its reaction delay and fires. Every shot
//! is a `Gun::fire` (spread, recoil) traced with `evani_combat::trace`
//! against the level's triangles and the target's hitbox; a hit does
//! `damage_at` (range falloff) times the zone multiplier through
//! `Health::take` (armour, helmet). Yellow tracers show the shots, red dots
//! the body hits, dark dots the misses; the bar over the target shows
//! health (green) and armour (blue). The target falls when it dies; the
//! lap restarts every [`LAP_TICKS`]. The level stands outside the spinner
//! grid on the -z side, a little nearer than the collide proof's so the
//! sandbox's scattered slabs stay clear of the gap, and the camera is held
//! on it.

use evani_combat::path::{Graph, shortest_path, smooth};
use evani_combat::{Body, BotIn, Brain, Foe, Gun, Health, Hit, Orders, Router, Stance, Tuning, damage_at, trace, wrap};
use evani_core::camera::CameraPose;
use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::params::ParamRegistry;
use evani_core::plugin::LookData;
use evani_core::rng::Pcg32;
use evani_physics::collide::math::normalize;
use evani_physics::collide::trig;
use evani_physics::collide::{CharacterConfig, CollideParams, StaticWorld, V3, WorldBuilder};
use evani_render::{MeshId, MeshInstance, RenderWorld};

/// Where the level's origin stands: outside the grid on the -z side.
pub const AT: V3 = [0.0, 0.0, -32.0];
/// The held camera: above and in front of the level, looking at its middle.
pub const CAMERA: CameraPose = CameraPose {
    eye: [0.0, 9.0, -21.0],
    target: [0.0, 0.3, -32.0],
    up: [0.0, 1.0, 0.0],
    fov_y_deg: 50.0,
};

/// Surface tags.
pub const TAG_SLAB: u32 = 1;
pub const TAG_WALL: u32 = 5;
pub const TAG_CRATE: u32 = 6;

/// The level's boxes in local metres: `(min, max, tag, mesh, colour)`.
const BOXES: [(V3, V3, u32, MeshId, [f32; 4]); 3] = [
    ([-7.0, 0.0, -4.5], [7.0, 0.1, 4.5], TAG_SLAB, MeshId::CUBE, [0.80, 0.82, 0.86, 1.0]),
    // The dividing wall; the gap is local z 1.2..4.5, at the far end.
    ([-0.3, 0.1, -4.5], [0.3, 2.0, 1.2], TAG_WALL, MeshId::CUBE, [0.70, 0.72, 0.80, 1.0]),
    // A crate on the target's side, out of the line of fire.
    ([4.2, 0.1, 1.6], [5.2, 1.0, 2.6], TAG_CRATE, MeshId::BEVEL_CUBE, [0.95, 0.72, 0.48, 1.0]),
];

/// The bot's start and the target's spot (feet, local).
pub const BOT_START: V3 = [-5.5, 0.1, -2.5];
pub const TARGET_AT: V3 = [5.0, 0.1, -2.5];
/// The bot's hunt goal: round the far end of the wall.
pub const GOAL: V3 = [3.0, 0.1, 3.2];
/// The route net (local x, z): before the wall, both sides of the gap,
/// past the wall.
const NODES: [[f32; 2]; 4] = [[-4.0, -1.0], [-1.2, 3.0], [1.2, 3.0], [4.0, -1.0]];
/// Body height and eye height (m).
const HEIGHT: f32 = 1.8;
const EYE: f32 = 1.6;
/// Ticks before the bot moves each lap (1 s at 60 Hz).
pub const HOLD_TICKS: u32 = 60;
/// Ticks per lap (10 s at 60 Hz); then both start again.
pub const LAP_TICKS: u32 = 600;
/// How long a tracer and a hit mark stay (ticks).
const TRACER_TICKS: u32 = 5;
const MARK_TICKS: u32 = 30;
/// The fight's seed (the lap number is added, so laps differ but replay).
const SEED: u64 = 0xC0BA7;

/// Local level metres to world space (local +z runs away from the camera).
fn local(p: V3) -> V3 {
    [p[0] + AT[0], p[1] + AT[1], AT[2] - p[2]]
}

/// The level's collision world (world space).
pub fn level() -> StaticWorld {
    let mut b = WorldBuilder::new();
    for (min, max, tag, _, _) in BOXES {
        b.aabb(local(min), local(max), tag);
    }
    b.build()
}

/// The route net: nodes in a line, linked to their neighbours (cost in cm).
#[derive(Debug, Clone)]
pub struct Net {
    pos: [V3; 4],
    links: [Vec<(u32, u32)>; 4],
}

impl Default for Net {
    fn default() -> Self {
        let pos = NODES.map(|n| local([n[0], 0.1, n[1]]));
        let cost = |a: usize, b: usize| (flat(pos[a], pos[b]) * 100.0) as u32;
        let links = [vec![(1, cost(0, 1))], vec![(0, cost(1, 0)), (2, cost(1, 2))], vec![(1, cost(2, 1)), (3, cost(2, 3))], vec![(2, cost(3, 2))]];
        Net { pos, links }
    }
}

impl Graph for Net {
    fn node_count(&self) -> usize {
        self.pos.len()
    }
    fn links(&self, node: u32) -> &[(u32, u32)] {
        self.links.get(node as usize).map_or(&[], |l| l.as_slice())
    }
}

fn flat(a: V3, b: V3) -> f32 {
    let (x, z) = (a[0] - b[0], a[2] - b[2]);
    (x * x + z * z).sqrt()
}

/// The route net with the level, as the bot's [`Router`].
struct Routes<'a> {
    net: &'a Net,
    world: &'a StaticWorld,
}

impl Routes<'_> {
    fn nearest(&self, p: V3) -> u32 {
        let mut best = (0u32, f32::INFINITY);
        for (i, q) in self.net.pos.iter().enumerate() {
            let d = flat(p, *q);
            if d < best.1 {
                best = (i as u32, d);
            }
        }
        best.0
    }

    /// A straight walk from `a` to `b` is clear at knee height, down the
    /// middle and a body's half-width either side.
    fn clear(&self, a: V3, b: V3) -> bool {
        let d = [b[0] - a[0], 0.0, b[2] - a[2]];
        let len = flat(a, b);
        let Some(u) = normalize(d) else { return true };
        let side = [-u[2] * 0.35, 0.0, u[0] * 0.35];
        [0.0f32, 1.0, -1.0].iter().all(|&k| {
            let o = [a[0] + side[0] * k, a[1] + 0.5, a[2] + side[2] * k];
            self.world.raycast(o, u, len).is_none()
        })
    }
}

impl Router for Routes<'_> {
    fn route(&self, from: V3, to: V3) -> Option<Vec<V3>> {
        let nodes = shortest_path(self.net, self.nearest(from), self.nearest(to))?;
        let mut pts = Vec::with_capacity(nodes.len() + 2);
        pts.push(from);
        pts.extend(nodes.iter().filter_map(|&n| self.net.pos.get(n as usize).copied()));
        pts.push(to);
        let mut out = smooth(&pts, |a, b| self.clear(a, b));
        out.remove(0);
        Some(out)
    }
}

/// A shot's line and what it hit, for drawing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tracer {
    pub from: V3,
    pub to: V3,
    /// Hit the target's body.
    pub body: bool,
    /// The tick it was fired.
    pub at: u32,
}

/// The proof's state (a world resource).
#[derive(Debug, Clone)]
pub struct ProofCombat {
    pub world: StaticWorld,
    pub net: Net,
    /// The current `combat.*` table (names untouched, refreshed each step).
    pub tune: Tuning,
    pub brain: Brain,
    pub gun: Gun,
    pub rng: Pcg32,
    pub bot: Body,
    pub yaw: f32,
    pub pitch: f32,
    pub vy: f32,
    pub grounded: bool,
    pub target: Body,
    pub health: Health,
    /// Ticks into the current lap.
    pub tick: u32,
    pub laps: u32,
    /// This lap: the tick the bot first had the target, the first shot,
    /// the kill; shots fired, body hits, shots stopped by the wall.
    pub saw_at: Option<u32>,
    pub first_shot: Option<u32>,
    pub killed_at: Option<u32>,
    pub shots: u32,
    pub hits: u32,
    pub wall_hits: u32,
    /// The bot's feet (local x, z) when it first fired.
    pub fired_from: Option<[f32; 2]>,
    /// The last shots (a ring).
    pub tracers: [Option<Tracer>; 8],
}

impl Default for ProofCombat {
    fn default() -> Self {
        let tune = Tuning::default();
        Self {
            world: level(),
            net: Net::default(),
            brain: Brain::new(tune.bot.clone(), SEED),
            gun: Gun::new(tune.rifle.clone()),
            rng: Pcg32::seeded(SEED),
            bot: Body::new(local(BOT_START), HEIGHT, EYE),
            yaw: 0.0,
            pitch: 0.0,
            vy: 0.0,
            grounded: true,
            target: Body::new(local(TARGET_AT), HEIGHT, EYE),
            health: Health::new(tune.health, tune.armour, tune.helmet),
            tune,
            tick: 0,
            laps: 0,
            saw_at: None,
            first_shot: None,
            killed_at: None,
            shots: 0,
            hits: 0,
            wall_hits: 0,
            fired_from: None,
            tracers: [None; 8],
        }
    }
}

impl ProofCombat {
    fn restart(&mut self) {
        let seed = SEED.wrapping_add(u64::from(self.laps) + 1);
        self.brain = Brain::new(self.tune.bot.clone(), seed);
        self.gun = Gun::new(self.tune.rifle.clone());
        self.rng = Pcg32::seeded(seed);
        self.bot = Body::new(local(BOT_START), HEIGHT, EYE);
        self.target = Body::new(local(TARGET_AT), HEIGHT, EYE);
        self.health = Health::new(self.tune.health, self.tune.armour, self.tune.helmet);
        self.yaw = 0.0;
        self.pitch = 0.0;
        self.vy = 0.0;
        self.grounded = true;
        self.tick = 0;
        self.laps += 1;
        self.saw_at = None;
        self.first_shot = None;
        self.killed_at = None;
        self.shots = 0;
        self.hits = 0;
        self.wall_hits = 0;
        self.fired_from = None;
        self.tracers = [None; 8];
    }

    /// One fixed step of the fight.
    fn step(&mut self, dt: f32, cfg: &CharacterConfig, speed: f32, gravity: f32) {
        if self.tick >= LAP_TICKS {
            self.restart();
        }
        self.tick += 1;
        self.brain.tune.clone_from(&self.tune.bot);
        self.gun.stats.clone_from(&self.tune.rifle);
        let back = self.gun.tick(dt);
        self.yaw = wrap(self.yaw + back[0]);
        self.pitch += back[1];
        if self.tick <= HOLD_TICKS {
            return;
        }
        if self.tick == HOLD_TICKS + 1 && self.brain.orders().is_none() {
            self.brain.set_orders(Orders { goal: local(GOAL), watch: None });
        }
        self.target.alive = self.health.alive();
        let others = [Foe { body: self.bot, team: 0 }, Foe { body: self.target, team: 1 }];
        let eye = [self.bot.feet[0], self.bot.feet[1] + EYE, self.bot.feet[2]];
        let out = {
            let routes = Routes { net: &self.net, world: &self.world };
            let inp = BotIn {
                world: &self.world,
                me: 0,
                team: 0,
                feet: self.bot.feet,
                eye,
                yaw: self.yaw,
                pitch: self.pitch,
                hp: self.tune.health,
                mag: self.gun.mag,
                reloading: self.gun.reloading(),
                mag_frac: if self.gun.stats.mag > 0 { self.gun.mag as f32 / self.gun.stats.mag as f32 } else { 0.0 },
                interval: self.gun.stats.interval,
                others: &others,
                sounds: &[],
                hurt_by: None,
                dt,
            };
            self.brain.think(&inp, &routes)
        };
        if self.saw_at.is_none() && self.brain.target() == Some(1) {
            self.saw_at = Some(self.tick);
        }
        self.yaw = wrap(self.yaw + out.dyaw);
        self.pitch += out.dpitch;
        if out.reload {
            self.gun.start_reload();
        }
        let pace = if out.walk { speed * 0.5 } else { speed };
        let stance = Stance { speed: pace * (out.move_xz[0] * out.move_xz[0] + out.move_xz[1] * out.move_xz[1]).sqrt(), airborne: !self.grounded, crouched: false };
        if let Some(shot) = self.gun.fire(out.fire, self.yaw, self.pitch, stance, self.tune.run_speed, &mut self.rng) {
            self.yaw = wrap(self.yaw + shot.kick[0]);
            self.pitch += shot.kick[1];
            self.shots += 1;
            if self.first_shot.is_none() {
                self.first_shot = Some(self.tick);
                self.fired_from = Some([self.bot.feet[0] - AT[0], AT[2] - self.bot.feet[2]]);
            }
            let range = self.gun.stats.range;
            let hit = trace(&self.world, eye, shot.dir, range, &[self.bot, self.target], Some(0));
            let to = hit.map_or([eye[0] + shot.dir[0] * range, eye[1] + shot.dir[1] * range, eye[2] + shot.dir[2] * range], |h| h.point());
            let mut body = false;
            match hit {
                Some(Hit::Body { index: 1, t, zone, .. }) => {
                    body = true;
                    self.hits += 1;
                    let dmg = damage_at(&self.gun.stats, t) * zone.mult(&self.gun.stats);
                    if self.health.take(dmg, self.gun.stats.armor_pen, zone).killed {
                        self.killed_at = Some(self.tick);
                    }
                }
                Some(Hit::World { tag: TAG_WALL, .. }) => self.wall_hits += 1,
                _ => {}
            }
            let slot = (self.shots as usize) % self.tracers.len();
            self.tracers[slot] = Some(Tracer { from: eye, to, body, at: self.tick });
        }
        self.vy = if self.grounded { 0.0 } else { self.vy + gravity * dt };
        let motion = [out.move_xz[0] * pace * dt, self.vy * dt, out.move_xz[1] * pace * dt];
        let r = self.world.move_character(self.bot.feet, HEIGHT, motion, cfg, self.grounded);
        self.bot.feet = r.feet;
        self.grounded = r.grounded;
    }
}

/// The fixed-step fight (`Stage::Update`).
pub fn fight_system() -> SystemDesc {
    SystemDesc::new("sandbox.proof_combat.fight", Stage::Update, fight)
}

/// The `Stage::Frame` drawing (after the kit's, so its camera wins).
pub fn draw_system() -> SystemDesc {
    SystemDesc::new("sandbox.proof_combat.draw", Stage::Frame, draw)
}

fn fight(world: &mut World) {
    let dt = world.try_res::<evani_core::time::Time>().map_or(1.0 / 60.0, |t| t.fixed_dt);
    let collide = world.try_res::<CollideParams>().map(|p| *p);
    let combat = world.try_res::<evani_combat::CombatParams>().map(|p| *p);
    let Some(mut p) = world.try_res_mut::<ProofCombat>() else { return };
    let (cfg, speed) = {
        let reg = world.res::<ParamRegistry>();
        if let Some(c) = combat {
            c.apply(&reg, &mut p.tune);
        }
        collide.map_or((CharacterConfig::default(), 4.5), |c| (c.config(&reg), c.speed(&reg)))
    };
    p.step(dt, &cfg, speed, -9.81);
}

/// The quaternion `[x, y, z, w]` that turns +x onto unit `d`.
fn x_onto(d: V3) -> [f32; 4] {
    let yaw = trig::atan2(-d[2], d[0]);
    let pitch = trig::atan2(d[1], (d[0] * d[0] + d[2] * d[2]).sqrt());
    let (sy, cy) = (trig::sin(yaw * 0.5), trig::cos(yaw * 0.5));
    let (sz, cz) = (trig::sin(pitch * 0.5), trig::cos(pitch * 0.5));
    [sy * sz, sy * cz, cy * sz, cy * cz]
}

/// A thin box from `a` to `b`.
fn beam(a: V3, b: V3, width: f32, colour: [f32; 4], material: u32) -> Option<MeshInstance> {
    let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    let u = normalize(d)?;
    let mut m = MeshInstance::new(MeshId::CUBE, [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5, (a[2] + b[2]) * 0.5]);
    m.rotation = x_onto(u);
    m.scale = [len, width, width];
    m.material = material;
    m.color = colour;
    Some(m)
}

fn draw_body(rw: &mut RenderWorld, b: &Body, colour: [f32; 4], material: u32, standing: bool) {
    let r = b.radius;
    let feet = b.feet;
    let neck = b.neck() - feet[1];
    let mut c = MeshInstance::new(MeshId::CYLINDER, [feet[0], feet[1] + neck * 0.5, feet[2]]);
    c.scale = [2.0 * r, neck, 2.0 * r];
    if !standing {
        // Fallen: lying along +x on the floor.
        c.position = [feet[0] + neck * 0.5, feet[1] + r, feet[2]];
        c.rotation = [0.0, 0.0, -core::f32::consts::FRAC_1_SQRT_2, core::f32::consts::FRAC_1_SQRT_2];
    }
    c.material = material;
    c.color = colour;
    rw.draw_mesh(c);
    let head = if standing { b.head_centre() } else { [feet[0] + neck + b.head, feet[1] + b.head, feet[2]] };
    let mut s = MeshInstance::new(MeshId::SPHERE, head);
    s.scale = [2.0 * b.head; 3];
    s.material = material;
    s.color = colour;
    rw.draw_mesh(s);
}

fn draw(world: &mut World) {
    let (stone, slate) = world
        .try_res::<LookData>()
        .map_or((0, 0), |l| (RenderWorld::material(&l, "stone"), RenderWorld::material(&l, "slate")));
    let Some(p) = world.try_res::<ProofCombat>() else { return };
    let Some(mut rw) = world.try_res_mut::<RenderWorld>() else { return };
    for (min, max, tag, mesh, colour) in BOXES {
        let (a, b) = (local(min), local(max));
        let mut m = MeshInstance::new(mesh, [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5, (a[2] + b[2]) * 0.5]);
        m.scale = [(b[0] - a[0]).abs(), (b[1] - a[1]).abs(), (b[2] - a[2]).abs()];
        m.material = if tag == TAG_WALL { slate } else { stone };
        m.color = colour;
        rw.draw_mesh(m);
    }
    // The bot (blue) with its gun along its aim; the target (orange, grey
    // and fallen once dead).
    draw_body(&mut rw, &p.bot, [0.25, 0.55, 1.0, 1.0], stone, true);
    let eye = [p.bot.feet[0], p.bot.feet[1] + EYE - 0.2, p.bot.feet[2]];
    let aim = evani_combat::look_dir(p.yaw, p.pitch);
    if let Some(g) = beam(eye, [eye[0] + aim[0] * 0.7, eye[1] + aim[1] * 0.7, eye[2] + aim[2] * 0.7], 0.08, [0.15, 0.15, 0.18, 1.0], slate) {
        rw.draw_mesh(g);
    }
    let alive = p.health.alive();
    draw_body(&mut rw, &p.target, if alive { [1.0, 0.55, 0.15, 1.0] } else { [0.45, 0.45, 0.48, 1.0] }, stone, alive);
    // Health (green) and armour (blue) bars over the target.
    let top = [p.target.feet[0], p.target.feet[1] + HEIGHT + 0.45, p.target.feet[2]];
    let full = 1.2;
    for (i, (v, max, colour)) in [(p.health.hp, p.tune.health, [0.2, 0.9, 0.3, 1.0]), (p.health.armour, p.tune.armour, [0.3, 0.6, 1.0, 1.0])].into_iter().enumerate() {
        let y = top[1] - i as f32 * 0.16;
        let mut bg = MeshInstance::new(MeshId::CUBE, [top[0], y, top[2]]);
        bg.scale = [full + 0.06, 0.12, 0.04];
        bg.material = slate;
        bg.color = [0.1, 0.1, 0.12, 1.0];
        rw.draw_mesh(bg);
        let share = if max > 0.0 { evani_core::num::clamp(v / max, 0.0, 1.0) } else { 0.0 };
        if share > 0.0 {
            let w = full * share;
            let mut bar = MeshInstance::new(MeshId::CUBE, [top[0] - full * 0.5 + w * 0.5, y, top[2] - 0.03]);
            bar.scale = [w, 0.08, 0.04];
            bar.material = stone;
            bar.color = colour;
            rw.draw_mesh(bar);
        }
    }
    // Tracers for the newest shots, marks where they landed.
    for t in p.tracers.iter().flatten() {
        let age = p.tick.saturating_sub(t.at);
        if age < TRACER_TICKS
            && let Some(m) = beam(t.from, t.to, 0.035, [1.0, 0.92, 0.35, 1.0], stone)
        {
            rw.draw_mesh(m);
        }
        if age < MARK_TICKS {
            let mut s = MeshInstance::new(MeshId::SPHERE, t.to);
            s.scale = [0.14; 3];
            s.material = stone;
            s.color = if t.body { [1.0, 0.15, 0.15, 1.0] } else { [0.15, 0.15, 0.18, 1.0] };
            rw.draw_mesh(s);
        }
    }
    rw.camera = CAMERA;
}

/// The bot's state, for the tests.
#[cfg(test)]
fn state(p: &ProofCombat) -> evani_combat::BotState {
    p.brain.state
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_combat::BotState;
    use evani_core::overlay::ErrorLog;

    fn run(frames: u32) -> crate::App {
        let mut app = crate::build_app();
        app.run_headless(frames, 1000.0 / 60.0);
        app
    }

    /// The wall hides the target until the bot rounds its end; then the
    /// bot waits out its reaction delay, fires and kills it, no shot
    /// wasted on the wall; two runs give the same bits.
    #[test]
    fn bot_rounds_the_wall_and_kills_the_target() {
        let app = run(480);
        assert_eq!(app.world.res::<ErrorLog>().error_count(), 0);
        let p = app.world.res::<ProofCombat>().clone();
        assert_eq!(p.laps, 0);
        let saw = p.saw_at.unwrap_or(u32::MAX);
        let first = p.first_shot.unwrap_or(u32::MAX);
        let killed = p.killed_at.unwrap_or(u32::MAX);
        assert!(saw > HOLD_TICKS + 30, "nothing seen through the wall: {saw}");
        let from = p.fired_from.unwrap_or([0.0, 0.0]);
        assert!(from[0] > -1.6 && from[1] > 1.2, "fired only once round the wall's end: {from:?}");
        assert!((first - saw) as f32 / 60.0 >= p.tune.bot.react - 0.4, "reaction delay: saw {saw}, fired {first}");
        assert!(killed < 480, "killed in the first eight seconds: {p:?}");
        // A rifle head hit goes through the helmet, so two hits can kill.
        assert!(!p.health.alive() && p.hits >= 2 && p.shots >= p.hits, "shots {} hits {} hp {} armour {} saw {saw} first {first} killed {killed} from {from:?}", p.shots, p.hits, p.health.hp, p.health.armour);
        assert_eq!(p.wall_hits, 0);
        assert!(matches!(state(&p), BotState::Hunt | BotState::Hold | BotState::Attack | BotState::Investigate), "{:?}", state(&p));
        assert_eq!(app.world.res::<RenderWorld>().camera.eye, CAMERA.eye);

        let again = run(480);
        let q = again.world.res::<ProofCombat>().clone();
        assert_eq!((p.first_shot, p.killed_at, p.shots, p.hits), (q.first_shot, q.killed_at, q.shots, q.hits));
        assert_eq!(p.bot.feet.map(f32::to_bits), q.bot.feet.map(f32::to_bits), "deterministic");
        assert_eq!(p.health.armour.to_bits(), q.health.armour.to_bits());
    }

    /// The lap restarts with the target back on its feet.
    #[test]
    fn lap_restarts() {
        let app = run(LAP_TICKS + 10);
        let p = app.world.res::<ProofCombat>();
        assert_eq!(p.laps, 1);
        assert!(p.tick <= HOLD_TICKS && p.health.alive() && p.bot.feet == local(BOT_START));
    }

    /// A live `combat.*` change reaches the fight on the next step.
    #[test]
    fn live_params_reach_the_fight() {
        let mut app = crate::build_app();
        app.run_headless(2, 1000.0 / 60.0);
        {
            let mut reg = app.world.res_mut::<ParamRegistry>();
            let id = reg.id("combat.rifle.damage");
            assert!(id.is_some_and(|id| reg.set(id, evani_core::params::ParamValue::F32(11.0)).is_ok()));
        }
        app.run_headless(2, 1000.0 / 60.0);
        assert_eq!(app.world.res::<ProofCombat>().gun.stats.damage, 11.0);
    }
}
