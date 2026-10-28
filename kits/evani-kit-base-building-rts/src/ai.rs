//! The rival's brain (SPEC "Skirmish AI"). Every `ai_think_s` it reads the
//! match (the whole map, no fog) and pushes ordinary [`Command`]s into the
//! same queue the player uses, so the AI is as lockstep-safe as the player:
//! sim time only, and placement jitter from the world's seeded [`Pcg32`].
//!
//! * Economy: idle workers gather at the nearest node; the hub trains
//!   workers up to `ai_workers`.
//! * Build order: a foundry, then a bastion, near its hub.
//! * Army: the foundry trains troopers (every third a rumbler once a bastion
//!   stands); new units rally in front of the hub.
//! * Defence: player units within `ai_defend_radius` of its hub pull the
//!   whole army onto them.
//! * Waves (never in free roam): from `ai_first_wave_s`, once the idle army
//!   reaches `ai_wave_size + ai_wave_growth * waves`, it attack-moves to the
//!   player's hub.

use evani_core::ecs::World;
use evani_core::rng::Pcg32;

use crate::control::{Phase, Ui};
use crate::data::{bs, us, BASTION, FOUNDRY, HUB, RUMBLER, TROOPER, WORKER};
use crate::grid::dist;
use crate::sim::{nearest_node, Command, CommandKind, Match, Order, PLAYER, RIVAL};
use crate::Tune;

/// The AI's memory between thoughts.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Brain {
    /// Sim seconds of play so far (pauses don't count).
    pub elapsed: f32,
    pub next_think: f32,
    pub waves: u32,
    /// Army units trained (every third is a rumbler).
    pub trained: u32,
    pub rally_set: bool,
}

pub fn ai_system(world: &mut World) {
    let Some((_, t)) = crate::tune(world) else { return };
    let playing = world.try_res::<Ui>().is_some_and(|u| u.phase == Phase::Playing && !u.paused);
    if !playing {
        return;
    }
    let dt = world.res::<evani_core::time::Time>().fixed_dt;
    let mut brain = world.res_mut::<Brain>();
    brain.elapsed += dt;
    if brain.elapsed < brain.next_think {
        return;
    }
    brain.next_think = brain.elapsed + t.ai_think_s.max(0.05);
    let mut m = world.res_mut::<Match>();
    if m.winner.is_some() {
        return;
    }
    let mut rng = world.res_mut::<Pcg32>();
    let cmds = think(&m, &mut brain, &t, &mut rng);
    m.pending.extend(cmds);
}

fn cmd(ids: Vec<u32>, kind: CommandKind) -> Command {
    Command { team: RIVAL, ids, kind }
}

fn center(m: &Match, b: &crate::sim::Building) -> [f32; 2] {
    let r = m.building_rect(b);
    [(r[0] + r[2]) * 0.5, (r[1] + r[3]) * 0.5]
}

/// One decision round.
pub fn think(m: &Match, brain: &mut Brain, t: &Tune, rng: &mut Pcg32) -> Vec<Command> {
    let mut out = Vec::new();
    let Some(hub) = m.hub(RIVAL) else { return out };
    let (hub_id, home) = (hub.id, center(m, hub));
    let crystal = m.crystal[RIVAL as usize];
    let mut spend = 0.0;

    // Economy: idle workers to the nearest node.
    let idle: Vec<u32> = m.units.iter().filter(|u| u.team == RIVAL && u.kind == WORKER && u.order == Order::Idle).map(|u| u.id).collect();
    if !idle.is_empty()
        && let Some(node) = nearest_node(&m.nodes, home, 70.0) {
            out.push(cmd(idle, CommandKind::Gather { node }));
        }
    let workers = m.units.iter().filter(|u| u.team == RIVAL && u.kind == WORKER).count() as f32;
    let worker_cost = m.types.unit(WORKER, us::COST);
    if workers < t.ai_workers && hub.built && hub.queue.is_empty() && crystal - spend >= worker_cost {
        spend += worker_cost;
        out.push(cmd(vec![hub_id], CommandKind::Train { kind: WORKER }));
    }

    // Build order: a foundry, then a bastion.
    let has = |kind: usize| m.buildings.iter().any(|b| b.team == RIVAL && b.kind == kind);
    let next = if !has(FOUNDRY) {
        Some(FOUNDRY)
    } else if m.has_built(RIVAL, FOUNDRY) && !has(BASTION) {
        Some(BASTION)
    } else {
        None
    };
    if let Some(kind) = next {
        let cost = m.types.building(kind, bs::COST);
        if crystal - spend >= cost
            && let Some(corner) = site(m, kind, rng) {
                // The worker nearest the spot (the sim picks one when none
                // is given, idle ones first).
                spend += cost;
                out.push(cmd(Vec::new(), CommandKind::Build { kind, corner }));
            }
    }

    // Army: rally in front of the hub (toward the map centre), then train.
    let front = [home[0] * 0.75, home[1] * 0.75];
    let foundries: Vec<u32> = m.buildings.iter().filter(|b| b.team == RIVAL && b.kind == FOUNDRY && b.built).map(|b| b.id).collect();
    if !foundries.is_empty() && !brain.rally_set {
        brain.rally_set = true;
        out.push(cmd(foundries.clone(), CommandKind::Rally { to: front }));
    }
    let short = m.buildings.iter().any(|b| foundries.contains(&b.id) && b.queue.len() < 2);
    if short {
        let heavy = brain.trained % 3 == 2 && m.has_built(RIVAL, BASTION);
        let kind = if heavy { RUMBLER } else { TROOPER };
        let cost = m.types.unit(kind, us::COST);
        // Workers first while the economy is thin.
        if crystal - spend >= cost && (workers >= t.ai_workers.min(6.0) || crystal - spend >= cost + worker_cost) {
            brain.trained += 1;
            out.push(cmd(foundries, CommandKind::Train { kind }));
        }
    }

    // Defence: intruders near the hub pull the army back.
    let army: Vec<&crate::sim::Unit> = m.units.iter().filter(|u| u.team == RIVAL && u.kind != WORKER).collect();
    let intruder = m
        .units
        .iter()
        .filter(|u| u.team == PLAYER && dist(u.pos, home) <= t.ai_defend_radius)
        .min_by(|a, b| dist(a.pos, home).total_cmp(&dist(b.pos, home)));
    if let Some(enemy) = intruder {
        let ids: Vec<u32> = army.iter().filter(|u| !matches!(u.order, Order::Attack { .. })).map(|u| u.id).collect();
        if !ids.is_empty() {
            out.push(cmd(ids, CommandKind::Move { to: enemy.pos, aggressive: true }));
        }
        return out;
    }

    // Waves: never in free roam.
    if m.free_roam || brain.elapsed < t.ai_first_wave_s {
        return out;
    }
    let idle_army: Vec<u32> = army.iter().filter(|u| u.order == Order::Idle).map(|u| u.id).collect();
    let need = t.ai_wave_size + t.ai_wave_growth * brain.waves as f32;
    if idle_army.len() as f32 >= need.max(1.0)
        && let Some(target) = m.hub(PLAYER) {
            brain.waves += 1;
            out.push(cmd(idle_army, CommandKind::Move { to: center(m, target), aggressive: true }));
        }
    out
}

/// A free spot for a `kind` near the rival hub, with a ring of passable
/// cells round it so lanes stay open. The search starts at a seeded angle.
fn site(m: &Match, kind: usize, rng: &mut Pcg32) -> Option<[i32; 2]> {
    let hub = m.hub(RIVAL)?;
    let size = m.types.size(kind);
    let hs = m.types.size(HUB);
    let c = [hub.corner[0] + hs / 2, hub.corner[1] + hs / 2];
    // Toward the map centre first (the rival sits north, so +z), bastions
    // nearer the front.
    let start = rng.range_i32(0, 7);
    for ring in 4..12 {
        let n = ring * 8;
        for k in 0..n {
            let a = (start * ring + k) % n;
            let ang = a as f32 / n as f32 * core::f32::consts::TAU;
            let corner = [c[0] + (ang.sin() * ring as f32).round() as i32 - size / 2, c[1] + (ang.cos() * ring as f32).round() as i32 - size / 2];
            if kind == BASTION && corner[1] < c[1] {
                continue;
            }
            if m.can_place(RIVAL, kind, corner).is_err() {
                continue;
            }
            let open = (corner[1] - 1..=corner[1] + size).all(|y| {
                (corner[0] - 1..=corner[0] + size).all(|x| {
                    let edge = x == corner[0] - 1 || x == corner[0] + size || y == corner[1] - 1 || y == corner[1] + size;
                    !edge || m.grid.passable([x, y])
                })
            });
            if open {
                return Some(corner);
            }
        }
    }
    None
}
