//! The destruction proof (sandbox feature `destruct-proof`; never in
//! evani.toml, so the default and ship builds are unchanged): Hexagon's
//! destruction mechanics through `evani-destruct`. An 8 x 6 m concrete deck
//! on a metal rim and four posts, with a 4 x 1 x 3 wall and a 1 x 1 x 3
//! pillar on it, under a seeded barrage of glowing shells on a 16 s loop:
//!
//! * 0.5 - 0.9 s: shells chip the wall's face.
//! * 1.0 - 1.7 s: three blasts cut its footing; it tips over the ledge that
//!   is left, slumps into the trench and breaks up.
//! * 4.0 - 6.7 s: a ring of blasts cuts a 3 x 2.5 m patch of deck free; it
//!   drops with the pillar riding it, lands on the ground and breaks up.
//! * Then the rubble settles, fades and frees its pool slots.
//!
//! The picture is a pure function of `Time::render_seconds()`: the state is
//! stepped tick by tick at 60 Hz from the start of the loop, and replayed
//! from the start when the time goes back (the loop, a capture's
//! `sim_time`) or any `destruct.*` param changes, so a live `param_set`
//! shows at once on the same moment.

use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::params::{ParamRegistry, ParamValue};
use evani_core::plugin::LookData;
use evani_core::rng::Pcg32;
use evani_core::time::Time;
use evani_destruct::draw::{DrawStyle, draw_debris, draw_structure};
use evani_destruct::tuning::{DEBRIS_MAX, DUST_MAX};
use evani_destruct::{Debris, DestructEvent, DestructParams, FloorGrid, FloorKind, Structure, StructureDesc, Tuning};
use evani_render::{MeshId, MeshInstance, RenderWorld};

/// The loop length (seconds) and its ticks.
pub const CYCLE: f64 = 16.0;
const HZ: f64 = 60.0;
const DT: f32 = 1.0 / 60.0;
/// The barrage seed.
pub const SEED: u64 = 0xDE57_0C7;
/// Where the deck's corner (local 0, 0) sits in the world; its top is at y = 3.
const AT: [f32; 2] = [-4.0, -3.0];
/// Where the shells come from (up and to the left of the camera).
const GUN: [f32; 3] = [-10.0, 11.0, -13.0];
/// Shell speed (m/s).
const SHELL_SPEED: f32 = 40.0;
/// How long a blast's flash lasts (ticks).
const FLASH: u32 = 8;

/// A world point from deck-local `x, z` (y is already world height).
fn w(x: f32, y: f32, z: f32) -> [f32; 3] {
    [AT[0] + x, y, AT[1] + z]
}

/// The deck: 16 x 12 floor samples of 0.5 m (metal rim, concrete inside),
/// the wall (x 1..5, z 1..2) and the pillar (x 5..6, z 3..4), local.
pub fn deck() -> Structure {
    let (nx, nz) = (16, 12);
    let mut f = FloorGrid::filled(nx, nz, 0.5, FloorKind::Concrete);
    for a in 0..nx {
        for b in 0..nz {
            if a == 0 || b == 0 || a == nx - 1 || b == nz - 1 {
                f.set(a, b, FloorKind::Metal);
            }
        }
    }
    let mut cells = Vec::new();
    for u in 1..5 {
        for y in 0..3 {
            cells.push([u, 1, y]);
        }
    }
    for y in 0..3 {
        cells.push([5, 3, y]);
    }
    Structure::new(
        StructureDesc { origin: [AT[0], 3.0, AT[1]], slab: 0.5, floor: Some(f), cells, ground_y: Some(0.0), hub: [AT[0] + 4.0, AT[1] + 3.0] },
        &Tuning::default(),
    )
}

/// One shell: it lands at tick `hit` and cuts a hole of radius `r` at `at`
/// (`chip`: a plain hole wherever it lands; else a blast that needs concrete).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shell {
    pub hit: u32,
    pub at: [f32; 3],
    pub r: f32,
    pub chip: bool,
}

impl Shell {
    /// Ticks in flight.
    fn flight(&self) -> u32 {
        let d = dist(GUN, self.at);
        ((d / SHELL_SPEED) * HZ as f32).ceil().max(1.0) as u32
    }
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// The barrage, in hit order.
pub fn barrage() -> Vec<Shell> {
    let mut rng = Pcg32::seeded(SEED);
    let mut out = Vec::new();
    // Chips in the wall's face (it faces -z, local z = 1).
    for k in 0..5u32 {
        let (x, y) = (rng.range_f32(1.3, 4.7), rng.range_f32(3.4, 5.6));
        out.push(Shell { hit: 30 + k * 6, at: w(x, y, 1.0), r: 0.3, chip: true });
    }
    // The footing.
    for (k, x) in [1.5f32, 2.5, 3.5].into_iter().enumerate() {
        out.push(Shell { hit: 60 + k as u32 * 20, at: w(x, 3.0, 1.5), r: 0.6, chip: false });
    }
    // The ring round local x 4..7, z 2.5..5, walked round its edge.
    let mut ring = Vec::new();
    for k in 0..=15 {
        ring.push([4.0 + 0.2 * k as f32, 2.5]);
    }
    for k in 1..=10 {
        ring.push([7.0, 2.5 + 0.25 * k as f32]);
    }
    for k in (0..15).rev() {
        ring.push([4.0 + 0.2 * k as f32, 5.0]);
    }
    for k in (1..10).rev() {
        ring.push([4.0, 2.5 + 0.25 * k as f32]);
    }
    for (k, p) in ring.into_iter().enumerate() {
        out.push(Shell { hit: 240 + k as u32 * 3, at: w(p[0], 2.75, p[1]), r: 0.45, chip: false });
    }
    out
}

/// The simulation, stepped from the start of the loop.
pub struct Sim {
    pub structure: Structure,
    pub debris: Debris,
    rng: Pcg32,
    shells: Vec<Shell>,
    /// Ticks stepped since the loop began.
    pub tick: u32,
    /// Ticks at which a blast went off (for the flashes).
    blasts: Vec<(u32, [f32; 3], f32)>,
    events: Vec<DestructEvent>,
    /// Every event since the loop began, with its tick (for tests).
    pub log: Vec<(u32, DestructEvent)>,
}

impl Sim {
    pub fn new() -> Self {
        Self {
            structure: deck(),
            debris: Debris::new(DEBRIS_MAX, DUST_MAX),
            rng: Pcg32::seeded(SEED),
            shells: barrage(),
            tick: 0,
            blasts: Vec::new(),
            events: Vec::new(),
            log: Vec::new(),
        }
    }

    /// Back to the start of the loop (no allocation: every pool is kept).
    pub fn reset(&mut self) {
        self.structure.reset();
        self.debris.clear();
        self.rng = Pcg32::seeded(SEED);
        self.tick = 0;
        self.blasts.clear();
        self.log.clear();
    }

    /// One fixed tick.
    pub fn step(&mut self, t: &Tuning) {
        self.tick += 1;
        let now = self.tick;
        let Self { structure: s, debris: d, rng, shells, blasts, events, log, .. } = self;
        for sh in shells.iter().filter(|sh| sh.hit == now) {
            let cut = if sh.chip { s.shoot(sh.at, sh.r, rng.below(64) as u8) } else { s.blast(sh.at, sh.r, rng.below(64) as u8) };
            if cut {
                d.hole(sh.at, sh.r, t, rng, |p| s.solid(p));
            }
            blasts.push((now, sh.at, sh.r));
        }
        blasts.retain(|b| now - b.0 < FLASH);
        events.clear();
        s.step(DT, t, rng, events);
        d.react(events, t, rng, |p| s.solid(p));
        d.step(DT, t, |p| s.solid(p));
        log.extend(events.iter().map(|e| (now, *e)));
    }

    /// Step (or replay from the start) to `tick`.
    pub fn advance_to(&mut self, tick: u32, t: &Tuning) {
        if tick < self.tick {
            self.reset();
        }
        while self.tick < tick {
            self.step(t);
        }
    }

    /// Draw the shells in flight at `ticks` (fractional) and the blast flashes.
    fn draw_shells(&self, rw: &mut RenderWorld, ticks: f64, glow: u32) {
        for sh in &self.shells {
            let fl = sh.flight();
            let launch = f64::from(sh.hit.saturating_sub(fl));
            if ticks < launch || ticks >= f64::from(sh.hit) {
                continue;
            }
            let a = ((ticks - launch) / f64::from(fl)) as f32;
            let p = [GUN[0] + (sh.at[0] - GUN[0]) * a, GUN[1] + (sh.at[1] - GUN[1]) * a, GUN[2] + (sh.at[2] - GUN[2]) * a];
            let mut m = MeshInstance::new(MeshId::LOW_SPHERE, p);
            m.scale = [0.22; 3];
            m.material = glow;
            m.color = [1.0, 0.75, 0.35, 1.0];
            rw.draw_mesh(m);
        }
        for &(at, p, r) in &self.blasts {
            let age = (ticks - f64::from(at)).max(0.0) as f32 / FLASH as f32;
            if age >= 1.0 {
                continue;
            }
            let mut m = MeshInstance::new(MeshId::LOW_SPHERE, p);
            m.scale = [r * 2.2 * (1.0 - age * 0.7); 3];
            m.material = glow;
            m.color = [1.0, 0.62, 0.25, 1.0];
            rw.draw_mesh(m);
        }
    }
}

impl Default for Sim {
    fn default() -> Self {
        Self::new()
    }
}

/// World resource: the proof's simulation and the tuning it was stepped with.
#[derive(Default)]
pub struct ProofDestruct {
    pub sim: Sim,
    tuning: Option<Tuning>,
}

/// The `Stage::Frame` system, after the scene (it takes over the camera).
pub fn system() -> SystemDesc {
    SystemDesc::new("sandbox.proof_destruct", Stage::Frame, frame).order(10)
}

fn frame(world: &mut World) {
    if world.try_res::<ProofDestruct>().is_none() {
        world.insert_resource(ProofDestruct::default());
    }
    // Only the still scene behind it: the spinner grid would hide the deck.
    if let Some(mut reg) = world.try_res_mut::<ParamRegistry>() {
        let _ = reg.set_by_name("kit.example.static_world", ParamValue::Bool(true));
    }
    let t = match (world.try_res::<DestructParams>(), world.try_res::<ParamRegistry>()) {
        (Some(p), Some(reg)) => p.read(&reg),
        _ => Tuning::default(),
    };
    let secs = world.try_res::<Time>().map_or(0.0, |t| t.render_seconds()).rem_euclid(CYCLE);
    let ticks = secs * HZ;
    let (style, glow) = match world.try_res::<LookData>() {
        Some(look) => (DrawStyle::from_look(&look), RenderWorld::material(&look, "glow")),
        None => return,
    };
    let Some(mut proof) = world.try_res_mut::<ProofDestruct>() else { return };
    if proof.tuning != Some(t) {
        proof.sim.reset();
        proof.tuning = Some(t);
    }
    proof.sim.advance_to(ticks.floor() as u32, &t);
    let Some(mut rw) = world.try_res_mut::<RenderWorld>() else { return };
    rw.camera = evani_core::camera::CameraPose { eye: [7.0, 8.5, -11.5], target: [-0.3, 2.0, 0.3], up: [0.0, 1.0, 0.0], fov_y_deg: 50.0 };
    // The deck's four posts.
    for (x, z) in [(0.25, 0.25), (7.75, 0.25), (0.25, 5.75), (7.75, 5.75)] {
        let mut m = MeshInstance::new(MeshId::CUBE, w(x, 1.25, z));
        m.scale = [0.5, 2.5, 0.5];
        m.material = style.metal;
        m.color = style.metal_color;
        rw.draw_mesh(m);
    }
    draw_structure(&mut rw, &proof.sim.structure, &style);
    draw_debris(&mut rw, &proof.sim.debris, &style);
    proof.sim.draw_shells(&mut rw, ticks, glow);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(ticks: u32) -> Sim {
        let mut s = Sim::new();
        s.advance_to(ticks, &Tuning::default());
        s
    }

    #[test]
    fn the_loop_breaks_the_wall_drops_the_patch_and_clears_up() {
        let t = Tuning::default();
        let mut s = Sim::new();
        s.advance_to(60, &t);
        assert_eq!(s.structure.standing(), 15, "nothing falls before the footing is hit");
        s.advance_to(200, &t);
        let falls: Vec<_> = s.log.iter().filter_map(|(k, e)| matches!(e, DestructEvent::Fall { .. }).then_some((*k, *e))).collect();
        assert!(falls.iter().any(|(_, e)| matches!(e, DestructEvent::Fall { cells: 12, .. })), "{:?}", s.log);
        assert!(s.log.iter().any(|(_, e)| matches!(e, DestructEvent::Land { .. })), "{:?}", s.log);
        assert_eq!(s.structure.standing(), 3, "the pillar still stands");
        s.advance_to(460, &t);
        assert!(
            s.log.iter().any(|(_, e)| matches!(e, DestructEvent::Fall { cells: 3, samples, .. } if *samples > 4)),
            "the patch drops with the pillar riding it: {:?}",
            s.log
        );
        assert!(s.debris.live() > 20, "rubble flying: {}", s.debris.live());
        s.advance_to((CYCLE * HZ) as u32 - 30, &t);
        assert_eq!(s.debris.live(), 0, "every chunk settled and faded");
        assert_eq!(s.debris.dust_count(), 0);
        assert!(s.structure.falls().is_empty());
    }

    #[test]
    fn replaying_gives_the_same_moment() {
        let a = run(470);
        let mut b = run(700);
        b.advance_to(470, &Tuning::default());
        assert_eq!(a.log, b.log);
        assert_eq!(a.structure.holes().holes(), b.structure.holes().holes());
        let chunks = |s: &Sim| {
            let mut v = Vec::new();
            s.debris.each_chunk(|c| v.push(c));
            v
        };
        assert_eq!(chunks(&a), chunks(&b));
    }

    #[test]
    fn a_param_change_changes_the_rubble() {
        let t = Tuning { debris_per_m: 64.0, ..Tuning::default() };
        let mut a = Sim::new();
        a.advance_to(300, &Tuning::default());
        let mut b = Sim::new();
        b.advance_to(300, &t);
        assert!(b.debris.live() > a.debris.live() + 50, "{} vs {}", b.debris.live(), a.debris.live());
    }

    /// The whole loop's cost per tick (native, debug or release as built).
    #[test]
    fn cost_per_tick() {
        let t = Tuning::default();
        let mut s = Sim::new();
        let n = (CYCLE * HZ) as u32;
        let t0 = std::time::Instant::now();
        let mut worst = 0u128;
        for _ in 0..n {
            let a = std::time::Instant::now();
            s.step(&t);
            worst = worst.max(a.elapsed().as_micros());
        }
        let us = t0.elapsed().as_micros() / u128::from(n);
        eprintln!("destruct proof: {us} us per tick on average, worst {worst} us, {n} ticks");
    }
}
