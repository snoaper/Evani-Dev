//! A soldier's body and its movement step, ported from Hexagon's
//! `rules.ts` (`Body`, `stepBody`, `aimDir`). One call per fixed 60 Hz
//! step (Hexagon's sub-step, 50 ms / 3).

use crate::arena::{Arena, sect, to_world};
use crate::tune::Tune;

/// One step's controls of a soldier (the player's from input actions, a bot's from its brain).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Controls {
    /// Strafe, -1 left .. 1 right.
    pub strafe: f32,
    /// Forward, -1 back .. 1 forward.
    pub forward: f32,
    /// Aim yaw (radians; forward is `(cos yaw, 0, sin yaw)`).
    pub yaw: f32,
    /// Aim pitch (radians, up positive).
    pub pitch: f32,
    /// Fire held.
    pub fire: bool,
    /// Jump held (also rises in a lift).
    pub jump: bool,
    /// Weapon slot (0..4).
    pub weapon: usize,
}

/// A soldier's body.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Body {
    /// Feet position.
    pub p: [f32; 3],
    /// Velocity.
    pub v: [f32; 3],
    /// Aim yaw.
    pub yaw: f32,
    /// Aim pitch.
    pub pitch: f32,
    /// Standing on something.
    pub grounded: bool,
    /// Time until the weapon can fire again (s).
    pub cooldown: f32,
    /// Weapon slot.
    pub weapon: usize,
}

/// Unit aim direction from yaw and pitch.
pub fn aim_dir(yaw: f32, pitch: f32) -> [f32; 3] {
    [pitch.cos() * yaw.cos(), pitch.sin(), pitch.cos() * yaw.sin()]
}

impl Body {
    /// A body standing at `p` facing `yaw`.
    pub fn at(p: [f32; 3], yaw: f32) -> Self {
        Body { p, yaw, ..Body::default() }
    }

    /// Horizontal speed.
    pub fn speed(&self) -> f32 {
        self.v[0].hypot(self.v[2])
    }

    /// Eye position.
    pub fn eye(&self, arena: &Arena) -> [f32; 3] {
        [self.p[0], self.p[1] + arena.def.eye, self.p[2]]
    }

    /// One movement step with controls `c`. True when the trigger fires this step.
    pub fn step(&mut self, arena: &Arena, c: &Controls, t: &Tune, dt: f32) -> bool {
        let d = &arena.def;
        self.yaw = c.yaw;
        self.pitch = c.pitch;
        if c.weapon != self.weapon {
            self.weapon = c.weapon & 3;
        }
        let (fx, fz) = (self.yaw.cos(), self.yaw.sin());
        let (mut mx, mut mz) = (c.strafe, c.forward);
        let ml = mx.hypot(mz);
        if ml > 1.0 {
            mx /= ml;
            mz /= ml;
        }
        let want_x = (fx * mz - fz * mx) * t.walk_speed;
        let want_z = (fz * mz + fx * mx) * t.walk_speed;
        let acc = if self.grounded { t.accel_ground } else { t.accel_air } * dt;
        let (mut dvx, mut dvz) = (want_x - self.v[0], want_z - self.v[2]);
        let dl = dvx.hypot(dvz);
        let steering = self.grounded || ml > 0.01;
        if dl > acc && steering {
            dvx *= acc / dl;
            dvz *= acc / dl;
        }
        if steering {
            self.v[0] += dvx;
            self.v[2] += dvz;
        }
        let lift = arena.lift_at(self.p[0], self.p[1], self.p[2]);
        if lift > 0 {
            let top = d.fy[lift];
            if c.jump {
                self.v[1] = if self.p[1] < top + 0.3 { t.lift_up } else { 0.0 };
                // Centre in the shaft while rising, so the body clears the hole's rim.
                let cv = if lift == 1 { d.lift_v } else { -d.lift_v };
                if self.p[1] < top + 0.1 {
                    let (k, _, _) = sect(self.p[0], self.p[2]);
                    let (sx, sz) = to_world(k, d.lift_u, cv);
                    self.v[0] += (sx - self.p[0]) * 6.0 * dt;
                    self.v[2] += (sz - self.p[2]) * 6.0 * dt;
                }
            } else {
                self.v[1] = (self.v[1] - t.gravity * dt).max(-t.lift_down);
            }
        } else {
            if self.grounded && c.jump {
                self.v[1] = t.jump_speed;
            }
            self.v[1] = (self.v[1] - t.gravity * dt).max(-40.0);
        }
        if arena.inside(self.p[0], self.p[1], self.p[2]) {
            // Never stuck inside something (squeezed by the turning ring): rise out of it.
            for _ in 0..10 {
                if !arena.inside(self.p[0], self.p[1], self.p[2]) {
                    break;
                }
                self.p[1] += 0.2;
            }
            self.v[1] = self.v[1].max(0.0);
            self.grounded = false;
        } else {
            if !arena.try_move(&mut self.p, [self.v[0] * dt, 0.0, 0.0]) {
                self.v[0] = 0.0;
            }
            if !arena.try_move(&mut self.p, [0.0, 0.0, self.v[2] * dt]) {
                self.v[2] = 0.0;
            }
            let dy = self.v[1] * dt;
            if arena.try_move(&mut self.p, [0.0, dy, 0.0]) {
                self.grounded = false;
            } else {
                self.grounded = dy < 0.0;
                self.v[1] = 0.0;
            }
        }
        self.cooldown = (self.cooldown - dt).max(0.0);
        if c.fire && self.cooldown <= 0.0 {
            self.cooldown = t.cooldown(self.weapon);
            return true;
        }
        false
    }
}
