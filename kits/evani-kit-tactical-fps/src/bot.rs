//! Bots, ported from Hexagon's `bot.ts`. Deterministic (only the world's
//! seeded Pcg32) and cheap. Attackers route level by level towards the
//! enemy base and its core; defenders hold their own base.
//!
//! The route (`data/arena.json` `nav`): the bottom level is split into two
//! halves by solid dividers, the ring cars and the top sectors are sealed.
//! So the way to the enemy base is: drop from the top into a ring car, drop
//! to the bottom, walk to your half's crossing sector, rise into the car
//! there, ride one turn to the enemy half, drop, walk to the enemy base
//! sector, rise to the ring, rise again into the base.
//!
//! A thought runs every third fixed step (Hexagon's 50 ms tick) and its
//! controls are held for that tick. Controls are -1..1 (Hexagon: +-100).

use evani_core::rng::Pcg32;

use crate::arena::{Arena, S60, Solid, sect, to_world};
use crate::body::{Controls, aim_dir};
use crate::match_rules::{Match, Phase, THINK_DT, ang_diff, wrap};
use crate::tune::{PISTOL, ROCKETS, SNIPER, Tune};
use crate::weapons::los;

/// Beyond this a bot picks the sniper (m).
const SNIPE_AT: f32 = 42.0;
/// Inside this a bot picks the pistol (m).
const CLOSE_AT: f32 = 9.0;
/// The ring must stay still this long more for a bot to ride it (s).
const RIDE_MARGIN: f32 = 2.5;
/// Middle of a sector along its axis, for a lost bot (m).
const MID_U: f32 = 30.0;

/// A bot's memory (Hexagon's `Brain`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Brain {
    /// Attacker (else defender).
    pub attacker: bool,
    /// Target soldier.
    target: Option<usize>,
    /// How long the target has been seen (s).
    seen: f32,
    /// Aim error yaw and pitch (rad).
    err: [f32; 2],
    /// Weapon picked for the target.
    weapon: usize,
    /// Time to the next scan (s).
    scan_in: f32,
    /// Stuck time (s).
    stuck: f32,
    /// Last position (x, z).
    last: [f32; 2],
    /// Strafe sign.
    side: f32,
    /// Wander point (x, z) and time until a new one.
    wander: [f32; 2],
    wander_t: f32,
}

impl Brain {
    /// A fresh brain.
    pub fn new(attacker: bool, rng: &mut Pcg32) -> Self {
        Brain {
            attacker,
            target: None,
            seen: 0.0,
            err: [0.0; 2],
            weapon: PISTOL,
            scan_in: rng.below(400) as f32 / 1000.0,
            stuck: 0.0,
            last: [0.0; 2],
            side: 1.0,
            wander: [0.0; 2],
            wander_t: 0.0,
        }
    }
}

/// A plan's output: goal point, hold jump, stop.
#[derive(Debug, Clone, Copy, Default)]
struct Goal {
    x: f32,
    z: f32,
    jump: bool,
    stop: bool,
}

impl Goal {
    fn go(&mut self, k: usize, u: f32, v: f32) {
        (self.x, self.z) = to_world(k, u, v);
    }

    /// A ring-local (car frame) point, turned with the ring.
    fn go_ring(&mut self, arena: &Arena, car: usize, u: f32, v: f32) {
        let (rx, rz) = to_world(car, u, v);
        (self.x, self.z) = arena.from_level(1, rx, rz);
    }

    fn dist(&self, p: [f32; 3]) -> f32 {
        (p[0] - self.x).hypot(p[2] - self.z)
    }
}

fn side_of(arena: &Arena, k: usize) -> usize {
    usize::from(!arena.def.nav.sides[0].contains(&k))
}

fn wander(arena: &Arena, p: [f32; 3], b: &mut Brain, k: usize, g: &mut Goal, rng: &mut Pcg32) {
    let nav = &arena.def.nav;
    b.wander_t -= THINK_DT;
    let (wk, _, _) = sect(b.wander[0], b.wander[1]);
    if b.wander_t <= 0.0 || wk != k || (p[0] - b.wander[0]).hypot(p[2] - b.wander[1]) < 1.0 {
        let u = rng.range_f32(nav.wander_u[0], nav.wander_u[1]);
        let v = rng.range_f32(-1.0, 1.0) * nav.wander_v;
        let (x, z) = to_world(k, u, v);
        b.wander = [x, z];
        b.wander_t = 3.0 + rng.next_f32() * 3.0;
    }
    g.x = b.wander[0];
    g.z = b.wander[1];
}

/// Route towards top sector `tk` (an attacker's enemy base, or a stray defender's own base).
fn plan(m: &Match, arena: &Arena, i: usize, b: &mut Brain, tk: usize, rng: &mut Pcg32) -> Goal {
    let mut g = Goal::default();
    let d = &arena.def;
    let s = &m.soldiers[i];
    let p = s.body.p;
    let r = &m.ring;
    let aligned = r.turning.is_none() && (m.phase != Phase::Play || r.timer < r.interval - RIDE_MARGIN);
    let level = d.level_of(p[1]);
    let lift = arena.lift_at(p[0], p[1], p[2]);
    let (ws, _, _) = sect(p[0], p[2]);
    if level == 2 {
        if ws == tk {
            if lift == 2 {
                g.jump = true;
            }
            if b.attacker && tk == d.base[1 - (s.team & 1)] {
                g.go(tk, d.core_u, 0.0);
                if g.dist(p) < 0.8 {
                    g.stop = true;
                }
            } else {
                wander(arena, p, b, tk, &mut g, rng);
            }
            return g;
        }
        // Leave this sealed sector through its lift hole (down onto the ring car below).
        g.go(ws, d.lift_u, -d.lift_v);
        if !aligned && g.dist(p) < 4.0 {
            g.stop = true;
        }
        return g;
    }
    if level == 1 {
        let (rx, rz) = arena.to_level(1, p[0], p[2]);
        let (car, ru, rv) = sect(rx, rz);
        let (side, tside) = (side_of(arena, ws), side_of(arena, tk));
        let mut down = false;
        if aligned && ws == tk {
            g.go(ws, d.lift_u, -d.lift_v);
            if g.dist(p) < 1.1 || lift == 2 {
                g.jump = true;
            }
        } else if aligned && (side == tside || ws != d.nav.cross[side]) {
            g.go(ws, d.lift_u, d.lift_v);
            down = true;
        } else {
            g.go_ring(arena, car, d.nav.car_u, 0.0);
        }
        if lift == 1 && !down {
            g.jump = true;
        }
        // Never near the open inner edge of a car.
        if ru < d.a_in + 2.5 {
            g.go_ring(arena, car, ru + 5.0, rv * 0.8);
        }
        return g;
    }
    // Bottom level.
    let (side, tside) = (side_of(arena, ws), side_of(arena, tk));
    let want = if side == tside { tk } else { d.nav.cross[side] };
    if ws == want {
        g.go(ws, d.lift_u, d.lift_v);
        if lift == 1 && (aligned || p[1] > 1.0) {
            g.jump = true;
        } else if !aligned && g.dist(p) < 3.0 {
            g.stop = true;
        }
        return g;
    }
    let ord = &d.nav.sides[side];
    let (Some(a), Some(z)) = (ord.iter().position(|&k| k == ws), ord.iter().position(|&k| k == want)) else {
        g.go(ws, MID_U, 0.0);
        return g;
    };
    let next = if z > a { ord[a + 1] } else { ord[a - 1] };
    let fwd = next == (ws + 1) % 6;
    let th = ((if fwd { ws } else { next }) as f32 + 0.5) * S60;
    let (cx, cz) = (th.cos(), th.sin());
    let sg = if fwd { 1.0 } else { -1.0 };
    let (tx, tz) = (-cz * sg, cx * sg);
    let along = p[0] * cx + p[2] * cz;
    let across = p[0] * tx + p[2] * tz;
    let off = if (along - d.nav.door_s).abs() < 1.0 && across > -1.6 { 1.6 } else { -1.2 };
    g.x = cx * d.nav.door_s + tx * off;
    g.z = cz * d.nav.door_s + tz * off;
    g
}

/// Look for the nearest foe in sight; pick a weapon for a new target and roll an aim error.
fn scan(m: &Match, arena: &Arena, i: usize, b: &mut Brain, rng: &mut Pcg32, t: &Tune) {
    let s = &m.soldiers[i];
    let p = s.body.p;
    let eye = [p[0], p[1] + arena.def.eye, p[2]];
    let (mut best, mut bd) = (None, t.bot_sight);
    for (j, e) in m.soldiers.iter().enumerate() {
        if e.team == s.team || !e.alive() {
            continue;
        }
        let q = e.body.p;
        let dist = (q[0] - p[0]).hypot(q[1] - p[1]).hypot(q[2] - p[2]);
        if dist >= bd || !los(arena, eye, [q[0], q[1] + 1.2, q[2]], false) {
            continue;
        }
        best = Some(j);
        bd = dist;
    }
    if best != b.target {
        b.target = best;
        b.seen = 0.0;
        let g = rng.next_f32();
        b.weapon = if bd > SNIPE_AT {
            SNIPER
        } else if bd < CLOSE_AT {
            PISTOL
        } else if g < 0.45 {
            ROCKETS
        } else if g < 0.7 {
            crate::tune::GRENADES
        } else {
            PISTOL
        };
    }
    let e = t.bot_aim_error * (1.0 + 12.0 / bd.max(6.0));
    b.err = [rng.range_f32(-1.0, 1.0) * e, rng.range_f32(-1.0, 1.0) * e * 0.6];
}

/// Is there a hole in the floor at `(x, z)` under feet height `y` that is not a lift hole?
fn hole_ahead(arena: &Arena, x: f32, y: f32, z: f32) -> bool {
    if arena.probe(x, y - 0.15, z).is_solid() || arena.probe(x, y - 0.4, z).is_solid() {
        return false;
    }
    let d = &arena.def;
    let level = d.level_of(y + 0.1);
    let (lx, lz) = arena.to_level(level, x, z);
    let (_, u, v) = sect(lx, lz);
    let lift = (u - d.lift_u).abs() < d.lift_h + 0.6 && (v.abs() - d.lift_v).abs() < d.lift_h + 0.6;
    !lift && u > d.a_in + 0.6
}

/// One bot thought (Hexagon's `botThink`): the controls it holds for the tick.
pub fn think(m: &Match, i: usize, b: &mut Brain, arena: &Arena, rng: &mut Pcg32, t: &Tune) -> Controls {
    let Some(s) = m.soldiers.get(i) else {
        return Controls::default();
    };
    let body = &s.body;
    let p = body.p;
    let (mut yaw, mut pitch, mut wpn) = (body.yaw, body.pitch, body.weapon);
    let (mut fire, mut jump) = (false, false);
    if m.phase != Phase::Play {
        b.target = None;
        b.stuck = 0.0;
        b.last = [p[0], p[2]];
        return Controls { yaw: wrap(yaw), weapon: wpn, ..Controls::default() };
    }
    let d = &arena.def;
    let own = d.base[s.team & 1];
    let tk = if b.attacker { d.base[1 - (s.team & 1)] } else { own };
    let g = plan(m, arena, i, b, tk, rng);
    b.scan_in -= THINK_DT;
    if b.scan_in <= 0.0 {
        b.scan_in = 0.35 + rng.below(200) as f32 / 1000.0;
        scan(m, arena, i, b, rng, t);
    }
    let foe = b.target.and_then(|j| m.soldiers.get(j)).filter(|e| e.alive());
    if foe.is_none() {
        b.target = None;
    }
    let (gdx, gdz) = (g.x - p[0], g.z - p[2]);
    let gd = gdx.hypot(gdz);
    let moving = !g.stop && gd > 0.35;
    let goal_yaw = gdz.atan2(gdx);
    // Stuck: wanted to move but barely did.
    let moved = (p[0] - b.last[0]).hypot(p[2] - b.last[1]);
    b.last = [p[0], p[2]];
    if moving && moved < 0.05 && body.grounded {
        b.stuck += THINK_DT;
    } else {
        b.stuck = (b.stuck - THINK_DT / 2.0).max(0.0);
    }
    let tick = m.steps / crate::match_rules::THINK_EVERY + i as u64 * 17;
    let (mut mx, mut mz) = (0.0_f32, 0.0_f32);
    if let Some(e) = foe {
        b.seen += THINK_DT;
        let q = e.body.p;
        let (dx, dz, dy) = (q[0] - p[0], q[2] - p[2], q[1] + 1.1 - (p[1] + d.eye));
        let hd = dx.hypot(dz);
        let lob = if b.weapon == crate::tune::GRENADES { (hd * 0.013).clamp(0.0, 0.55) } else { 0.0 };
        let settle = (1.0 - b.seen / 3.0).max(0.35);
        let want_yaw = dz.atan2(dx) + b.err[0] * settle;
        let want_pitch = dy.atan2(hd) + lob + b.err[1] * settle;
        let dyaw = ang_diff(yaw, want_yaw);
        yaw += dyaw.clamp(-0.22, 0.22);
        pitch += (want_pitch - pitch).clamp(-0.15, 0.15);
        wpn = b.weapon;
        if b.seen >= t.bot_react_s && dyaw.abs() < 0.09 && (want_pitch - pitch).abs() < 0.09 {
            fire = true;
        }
        if !b.attacker || g.stop || !moving {
            // Duck and weave: strafe across the line of fire.
            if tick.is_multiple_of(50) {
                b.side = -b.side;
            }
            mx = 0.7 * b.side;
        }
    } else {
        yaw += ang_diff(yaw, goal_yaw).clamp(-0.25, 0.25);
        pitch += (-pitch).clamp(-0.1, 0.1);
        if wpn != PISTOL && b.stuck == 0.0 {
            wpn = if b.attacker { ROCKETS } else { SNIPER };
        }
    }
    if moving {
        let rel = goal_yaw - yaw;
        let sp = if gd < 1.5 { 0.45 } else { 1.0 };
        mx = (mx + rel.sin() * sp).clamp(-1.0, 1.0);
        mz = rel.cos() * sp;
        // A blast crater in the floor ahead (not a lift hole): sidestep along its edge instead of dropping in.
        if body.grounded && hole_ahead(arena, p[0] + goal_yaw.cos() * 0.9, p[1], p[2] + goal_yaw.sin() * 0.9) {
            mz = 0.0;
            mx = b.side;
            if tick.is_multiple_of(60) {
                b.side = -b.side;
            }
        }
    }
    if g.jump || (b.stuck > 0.5 && b.stuck < 1.4) {
        jump = true;
    }
    if b.stuck >= 1.4 {
        // Still stuck: blast concrete out of the way, or sidestep metal.
        let dir = aim_dir(goal_yaw, 0.0);
        let low = arena.probe(p[0] + dir[0] * 1.1, p[1] + 0.6, p[2] + dir[2] * 1.1);
        let high = arena.probe(p[0] + dir[0] * 1.1, p[1] + 1.3, p[2] + dir[2] * 1.1);
        let concrete = |c: Solid| matches!(c, Solid::Concrete(_));
        if foe.is_none() && (concrete(low) || concrete(high)) {
            wpn = ROCKETS;
            yaw = goal_yaw;
            pitch = if concrete(low) { -0.25 } else { 0.0 };
            if body.weapon == ROCKETS && body.cooldown <= 0.0 {
                fire = true;
                b.stuck = 0.0;
            }
        } else {
            mx = b.side;
            if b.stuck > 2.6 {
                b.side = -b.side;
                b.stuck = 0.0;
            }
        }
    }
    Controls { strafe: mx, forward: mz, yaw: wrap(yaw), pitch: pitch.clamp(-1.4, 1.4), fire, jump, weapon: wpn }
}
