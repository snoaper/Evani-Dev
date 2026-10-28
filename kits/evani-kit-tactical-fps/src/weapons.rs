//! Weapons, ported from Hexagon's `rules.ts` (`rayBody`, `trace`, `los`,
//! `stepProj`) and `runtime.ts` (`fire`, `explode`, `stepProjectiles`).
//!
//! Pistol and sniper are hitscan rays; rockets fly straight and grenades
//! arc, bounce and blow on a fuse. Blasts fall off linearly, knock soldiers
//! back, never hurt the shooter's team and hurt the shooter at
//! `self_damage`. Added (HEXAGON_PLAN section 2): recoil that climbs per
//! shot and settles, and a spread cone that grows with speed and in the
//! air, sampled from the seeded Pcg32.

use evani_core::rng::Pcg32;

use crate::arena::{Arena, Solid};
use crate::body::aim_dir;
use crate::match_rules::{Match, Shot, ShotEnd};
use crate::tune::{GRENADES, PISTOL, ROCKETS, SNIPER, Tune};

/// A rocket or grenade in flight.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Projectile {
    /// Weapon slot ([`ROCKETS`] or [`GRENADES`]).
    pub weapon: usize,
    /// Soldier index of the shooter.
    pub owner: usize,
    /// Team of the shooter.
    pub team: usize,
    /// Position.
    pub p: [f32; 3],
    /// Velocity.
    pub v: [f32; 3],
    /// Age (s).
    pub age: f32,
}

/// What a projectile step did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjStep {
    /// Still flying.
    Flying,
    /// Blows up where it is.
    Explode,
    /// Fell out of the world.
    Lost,
}

/// Distance along a unit ray to a soldier's hit cylinder (feet at `t`), if hit.
pub fn ray_body(arena: &Arena, o: [f32; 3], d: [f32; 3], t: [f32; 3]) -> Option<f32> {
    let (hit_r, hit_h) = (arena.def.hit_r, arena.def.hit_h);
    let (px, pz) = (o[0] - t[0], o[2] - t[2]);
    let a = d[0] * d[0] + d[2] * d[2];
    let b = 2.0 * (px * d[0] + pz * d[2]);
    let c = px * px + pz * pz - hit_r * hit_r;
    let (mut t0, mut t1) = (-1e9_f32, 1e9_f32);
    if a < 1e-9 {
        if c > 0.0 {
            return None;
        }
    } else {
        let disc = b * b - 4.0 * a * c;
        if disc < 0.0 {
            return None;
        }
        let q = disc.sqrt();
        t0 = (-b - q) / (2.0 * a);
        t1 = (-b + q) / (2.0 * a);
    }
    let (mut y0, mut y1) = (-1e9_f32, 1e9_f32);
    if d[1].abs() < 1e-9 {
        if o[1] < t[1] || o[1] > t[1] + hit_h {
            return None;
        }
    } else {
        let ta = (t[1] - o[1]) / d[1];
        let tb = (t[1] + hit_h - o[1]) / d[1];
        y0 = ta.min(tb);
        y1 = ta.max(tb);
    }
    let en = t0.max(y0).max(0.0);
    let ex = t1.min(y1);
    (en <= ex).then_some(en)
}

/// What a hitscan ray hit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hit {
    /// Nothing within range.
    Nothing,
    /// A soldier (index into the targets).
    Target(usize),
    /// The world (metal or a concrete piece).
    World(Solid),
}

/// A hitscan shot: the nearest target or world hit within `range`, and its distance.
pub fn trace(arena: &Arena, o: [f32; 3], d: [f32; 3], range: f32, targets: &[[f32; 3]]) -> (f32, Hit) {
    let mut best = range;
    let mut hit = Hit::Nothing;
    for (i, &t) in targets.iter().enumerate() {
        if let Some(dist) = ray_body(arena, o, d, t)
            && dist < best {
                best = dist;
                hit = Hit::Target(i);
            }
    }
    let at = |s: f32| arena.probe(o[0] + d[0] * s, o[1] + d[1] * s, o[2] + d[2] * s);
    let st = 0.12;
    let mut s = 0.05;
    while s < best {
        let c = at(s);
        if c.is_solid() {
            // Refine to the surface (a few mm), so holes land exactly where the shot meets the wall.
            let (mut lo, mut hi, mut hc) = ((s - st).max(0.0), s, c);
            for _ in 0..7 {
                let m = (lo + hi) / 2.0;
                let mc = at(m);
                if mc.is_solid() {
                    hi = m;
                    hc = mc;
                } else {
                    lo = m;
                }
            }
            return (hi, Hit::World(hc));
        }
        s += st;
    }
    (best, hit)
}

/// Line of sight between two points (`metal_only`: concrete does not block).
pub fn los(arena: &Arena, a: [f32; 3], b: [f32; 3], metal_only: bool) -> bool {
    let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    let n = (len / 0.25).ceil() as i32;
    for i in 1..n {
        let f = i as f32 / n as f32;
        match arena.probe(a[0] + d[0] * f, a[1] + d[1] * f, a[2] + d[2] * f) {
            Solid::Metal => return false,
            Solid::Concrete(_) if !metal_only => return false,
            _ => {}
        }
    }
    true
}

impl Projectile {
    /// One step of flight.
    pub fn step(&mut self, arena: &Arena, t: &Tune, dt: f32) -> ProjStep {
        self.age += dt;
        if self.weapon == GRENADES {
            if self.age >= t.grenade_fuse_s {
                return ProjStep::Explode;
            }
            self.v[1] -= t.gravity * dt;
        } else if self.age >= t.proj_life_s {
            return ProjStep::Explode;
        }
        let sp = (self.v[0] * self.v[0] + self.v[1] * self.v[1] + self.v[2] * self.v[2]).sqrt() * dt;
        let n = ((sp / 0.2).ceil() as i32).max(1);
        let h = dt / n as f32;
        let solid = |x: f32, y: f32, z: f32| arena.probe(x, y, z).is_solid();
        for _ in 0..n {
            let np = [self.p[0] + self.v[0] * h, self.p[1] + self.v[1] * h, self.p[2] + self.v[2] * h];
            if !solid(np[0], np[1], np[2]) {
                self.p = np;
                continue;
            }
            if self.weapon != GRENADES {
                return ProjStep::Explode;
            }
            let b = t.grenade_bounce;
            let mut hit = false;
            if solid(np[0], self.p[1], self.p[2]) {
                self.v[0] = -self.v[0] * b;
                hit = true;
            }
            if solid(self.p[0], np[1], self.p[2]) {
                self.v[1] = -self.v[1] * b;
                self.v[0] *= 0.75;
                self.v[2] *= 0.75;
                hit = true;
            }
            if solid(self.p[0], self.p[1], np[2]) {
                self.v[2] = -self.v[2] * b;
                hit = true;
            }
            if !hit {
                self.v = [-self.v[0] * b, -self.v[1] * b, -self.v[2] * b];
            }
            break;
        }
        if self.p[1] < t.fall_death_y { ProjStep::Lost } else { ProjStep::Flying }
    }
}

/// A hole seed (0..63) for the drawing's rim pattern, from the seeded RNG.
fn hole_seed(rng: &mut Pcg32) -> u32 {
    rng.below(64)
}

impl Match {
    /// The spread cone half-angle of soldier `i` right now.
    pub fn spread(&self, i: usize, t: &Tune) -> f32 {
        let Some(s) = self.soldiers.get(i) else {
            return 0.0;
        };
        let b = &s.body;
        let speed = (b.speed() / t.walk_speed.max(0.1)).min(1.0);
        let air = if b.grounded { 0.0 } else { t.spread_air };
        let cone = t.spread_base + t.spread_move * speed + air;
        if b.weapon == SNIPER { cone * t.sniper_spread_mult } else { cone }
    }

    /// Soldier `i` fires its weapon.
    pub fn fire(&mut self, i: usize, arena: &mut Arena, rng: &mut Pcg32, t: &Tune) {
        let Some(s) = self.soldiers.get(i) else {
            return;
        };
        let (team, w) = (s.team, s.body.weapon);
        let o = s.body.eye(arena);
        let (mut yaw, mut pitch) = (s.body.yaw + s.recoil_yaw, s.body.pitch + s.recoil);
        if w == PISTOL || w == SNIPER {
            // Spread: a point in the cone, uniform over its disc.
            let cone = self.spread(i, t);
            let a = rng.range_f32(0.0, core::f32::consts::TAU);
            let r = rng.next_f32().sqrt() * cone;
            yaw += r * a.cos() / pitch.cos().max(0.2);
            pitch += r * a.sin();
            let d = aim_dir(yaw, pitch);
            let foes: Vec<usize> = (0..self.soldiers.len())
                .filter(|&j| self.soldiers[j].team != team && self.soldiers[j].alive())
                .collect();
            #[cfg(not(feature = "net"))]
            let targets: Vec<[f32; 3]> = foes.iter().map(|&j| self.soldiers[j].body.p).collect();
            // Net builds: a player's shot is tested where the foes stood as
            // that player saw them (lag compensation, src/net).
            #[cfg(feature = "net")]
            let targets: Vec<[f32; 3]> = foes.iter().map(|&j| self.lag_target(i, j)).collect();
            let (dist, hit) = trace(arena, o, d, t.range(w), &targets);
            let end = [o[0] + d[0] * dist, o[1] + d[1] * dist, o[2] + d[2] * dist];
            let kind = match hit {
                Hit::Target(k) => {
                    if let Some(&v) = foes.get(k) {
                        self.hurt(v, t.damage(w), Some(i), w, t);
                    }
                    ShotEnd::Soldier
                }
                Hit::World(Solid::Concrete(_)) => {
                    // A small hole sphere just inside the surface where the shot met it.
                    let seed = hole_seed(rng);
                    arena.shot_hole(end, d, t.hole(w), t.hole_depth, seed, t.hole_cap.max(1) as usize, t.hole_max.max(16) as usize);
                    ShotEnd::Concrete
                }
                Hit::World(_) => ShotEnd::Metal,
                Hit::Nothing => ShotEnd::Air,
            };
            self.shots.push(Shot { from: o, to: end, end: kind, age: 0.0, team });
            // Recoil climbs per shot (it settles in `Soldier::settle`).
            let kick = if w == SNIPER { t.sniper_recoil } else { t.pistol_recoil };
            if let Some(s) = self.soldiers.get_mut(i) {
                s.recoil += kick;
                s.recoil_yaw += kick * 0.3 * rng.range_f32(-1.0, 1.0);
            }
            return;
        }
        let d = aim_dir(yaw, pitch);
        let start = [o[0] + d[0] * 0.7, o[1] + d[1] * 0.7 - 0.15, o[2] + d[2] * 0.7];
        if arena.probe(start[0], start[1], start[2]).is_solid() {
            // Fired into a wall at point blank.
            self.explode(o, Some(i), team, w, arena, rng, t);
            return;
        }
        let (sp, up) = if w == ROCKETS { (t.rocket_speed, 0.0) } else { (t.grenade_speed, t.grenade_up) };
        self.projectiles.push(Projectile {
            weapon: w,
            owner: i,
            team,
            p: start,
            v: [d[0] * sp, d[1] * sp + up, d[2] * sp],
            age: 0.0,
        });
    }

    /// A blast of weapon `w` at `c`.
    #[allow(clippy::too_many_arguments)]
    pub fn explode(&mut self, c: [f32; 3], owner: Option<usize>, team: usize, w: usize, arena: &mut Arena, rng: &mut Pcg32, t: &Tune) {
        let seed = hole_seed(rng);
        arena.blast_holes(c, t.hole(w), seed, t.hole_cap.max(1) as usize, t.hole_max.max(16) as usize);
        let r = t.blast_radius;
        for i in 0..self.soldiers.len() {
            let s = &self.soldiers[i];
            if !s.alive() {
                continue;
            }
            let p = s.body.p;
            let cy = p[1].max((p[1] + 1.8).min(c[1]));
            let d = (p[0] - c[0]).hypot(cy - c[1]).hypot(p[2] - c[2]);
            if d > r || !los(arena, c, [p[0], cy, p[2]], true) {
                continue;
            }
            let f = 1.0 - d / r;
            let kick = 9.0 * f;
            let n = d.max(0.3);
            let mul = if Some(i) == owner {
                t.self_damage
            } else if s.team == team {
                0.0
            } else {
                1.0
            };
            if let Some(s) = self.soldiers.get_mut(i) {
                s.body.v[0] += (p[0] - c[0]) / n * kick;
                s.body.v[2] += (p[2] - c[2]) / n * kick;
                s.body.v[1] += ((cy + 0.5 - c[1]) / n * kick).max(2.0);
            }
            self.hurt(i, t.damage(w) * f * mul, owner, w, t);
        }
        self.blasts.push(crate::match_rules::Blast { p: c, age: 0.0, team });
    }

    /// Fly every projectile one step; blow up those that hit something or a foe.
    pub fn step_projectiles(&mut self, arena: &mut Arena, rng: &mut Pcg32, t: &Tune, dt: f32) {
        let list = core::mem::take(&mut self.projectiles);
        let mut keep = Vec::with_capacity(list.len());
        for mut pr in list {
            let mut r = pr.step(arena, t, dt);
            if r == ProjStep::Flying {
                let touches = self.soldiers.iter().any(|v| {
                    v.alive()
                        && v.team != pr.team
                        && (v.body.p[0] - pr.p[0]).hypot(v.body.p[2] - pr.p[2]) < 0.6
                        && pr.p[1] > v.body.p[1] - 0.2
                        && pr.p[1] < v.body.p[1] + 2.0
                });
                if touches {
                    r = ProjStep::Explode;
                }
            }
            match r {
                ProjStep::Explode => self.explode(pr.p, Some(pr.owner), pr.team, pr.weapon, arena, rng, t),
                ProjStep::Flying => keep.push(pr),
                ProjStep::Lost => {}
            }
        }
        self.projectiles = keep;
    }
}
