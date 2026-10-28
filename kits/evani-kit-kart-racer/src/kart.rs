//! Arcade kart kinematics: speed along the heading, steering by a speed
//! curve, drift with a hop and charge tiers that fire boosts, off-road
//! slow-down and the track-edge walls. A fixed-step, allocation-free update
//! per kart; no rigid bodies.
//!
//! Conventions: forward = `(sin yaw, 0, cos yaw)`, left = `(f.z, 0, -f.x)`;
//! steer `+1` is right, which turns the yaw down.

pub use evani_core::num::wrap_angle;
use evani_core::num::approach;
use crate::Tune;
use crate::items::Slot;
use crate::track::{Located, Track};

/// What a driver (human or AI) asks for this tick.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Controls {
    /// -1 (left) .. 1 (right).
    pub steer: f32,
    pub throttle: f32,
    pub brake: f32,
    /// Drift button held.
    pub drift: bool,
    /// Item button pressed this tick.
    pub item: bool,
}

/// Something a kart's own physics step did this tick (sounds, shake).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct StepEvents {
    pub hopped: bool,
    /// Drift reached a new charge tier (1..3).
    pub tier_up: Option<u8>,
    /// Drift released with a boost of this tier (1..3).
    pub boost_tier: Option<u8>,
    /// Hit a wall at this speed loss (m/s).
    pub wall: Option<f32>,
}

/// A player's throttle on the grid. Pressing it in the last
/// `start_boost_window_s` before GO and holding it through GO launches with
/// a start boost; holding it since earlier (revving too soon) does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Launch {
    #[default]
    Idle,
    /// Pressed inside the window and still held.
    Primed,
    /// Held since before the window.
    Early,
}

impl Launch {
    /// The next state after one countdown tick: `held` is the throttle,
    /// `left` the countdown seconds left, `window` the start window.
    pub fn next(self, held: bool, left: f32, window: f32) -> Self {
        match (held, self) {
            (false, _) => Launch::Idle,
            (true, Launch::Idle) if left <= window => Launch::Primed,
            (true, Launch::Idle) => Launch::Early,
            (true, s) => s,
        }
    }
}

/// One kart.
#[derive(Debug, Clone, PartialEq)]
pub struct Kart {
    /// Roster entry (`karts.json`) this kart uses.
    pub spec: usize,
    /// Local player index, or `None` for an AI.
    pub player: Option<usize>,
    pub pos: [f32; 3],
    /// Position and yaw at the start of the last tick (render interpolation).
    pub prev_pos: [f32; 3],
    pub prev_yaw: f32,
    pub yaw: f32,
    /// Speed along the heading (negative = reversing).
    pub speed: f32,
    /// Height above the road (hops, bumps) and its vertical speed.
    pub hop: f32,
    pub vy: f32,
    /// 0 = not drifting, otherwise the sign of the drift's yaw rate.
    pub drift: f32,
    /// Seconds of drift charge.
    pub charge: f32,
    pub boost: f32,
    /// Seconds of spin-out left (no control).
    pub spin: f32,
    pub shield: f32,
    /// Base speed cap multiplier (AI skill) and the rubber band on top.
    pub skill_mult: f32,
    pub band_mult: f32,
    pub slot: Slot,
    // Track position.
    pub hint: usize,
    pub s: f32,
    pub lateral: f32,
    pub on_road: bool,
    // Race progress.
    pub laps_done: u32,
    pub next_gate: u32,
    pub finished: Option<f32>,
    pub place: usize,
    pub wrong_way_t: f32,
    pub wrong_way: bool,
    /// Visual: spin angle while spun out, drift slip angle, hop squash.
    pub spin_angle: f32,
    pub slip: f32,
    /// Seconds since the last drift tier change (spark burst).
    pub tier_flash: f32,
    /// Visual only: steer eased towards the controls (front wheels, body
    /// lean), wheel roll angle (radians, from distance travelled) and the
    /// landing squash after a hop (1 at touchdown, easing to 0).
    pub steer_vis: f32,
    pub wheel_roll: f32,
    pub squash: f32,
    /// The start: throttle during the countdown (see [`Launch`]).
    pub launch: Launch,
    /// In another kart's slipstream this step (set by the race before
    /// [`step`]; see [`crate::slipstream`]): the speed cap is multiplied by
    /// [`crate::slipstream::SLIPSTREAM_MULTIPLIER`].
    pub slipstream: bool,
    /// Why it is or is not slipstreaming (the debug readout).
    pub draft: crate::slipstream::Slip,
    /// The speed cap of the last step before and after the slipstream (m/s),
    /// for the debug readout.
    pub cap_base: f32,
    pub cap: f32,
}

impl Kart {
    pub fn new(spec: usize, player: Option<usize>, pos: [f32; 3], yaw: f32) -> Self {
        Self {
            spec,
            player,
            pos,
            prev_pos: pos,
            prev_yaw: yaw,
            yaw,
            speed: 0.0,
            hop: 0.0,
            vy: 0.0,
            drift: 0.0,
            charge: 0.0,
            boost: 0.0,
            spin: 0.0,
            shield: 0.0,
            skill_mult: 1.0,
            band_mult: 1.0,
            slot: Slot::Empty,
            hint: 0,
            s: 0.0,
            lateral: 0.0,
            on_road: true,
            laps_done: 0,
            next_gate: 0,
            finished: None,
            place: 0,
            wrong_way_t: 0.0,
            wrong_way: false,
            spin_angle: 0.0,
            slip: 0.0,
            tier_flash: 0.0,
            steer_vis: 0.0,
            wheel_roll: 0.0,
            squash: 0.0,
            launch: Launch::Idle,
            slipstream: false,
            draft: crate::slipstream::Slip::NotDriving,
            cap_base: 0.0,
            cap: 0.0,
        }
    }

    pub fn forward(&self) -> [f32; 2] {
        [self.yaw.sin(), self.yaw.cos()]
    }

    /// The kart's footprint as it is drawn (heading plus the drift's slip
    /// angle): what bumps and the chase camera's probe use.
    pub fn hull(&self, radius: f32) -> crate::hull::Hull {
        crate::hull::Hull::new(self.pos, self.yaw + self.slip, radius)
    }

    pub fn left(&self) -> [f32; 2] {
        let f = self.forward();
        [f[1], -f[0]]
    }

    /// Current drift charge tier (0 = none yet, 1..3).
    pub fn tier(&self, t: &Tune) -> u8 {
        if self.drift == 0.0 {
            0
        } else if self.charge >= t.drift_tier3 {
            3
        } else if self.charge >= t.drift_tier2 {
            2
        } else if self.charge >= t.drift_tier1 {
            1
        } else {
            0
        }
    }

    /// Spin out (items). A shield absorbs the hit instead; returns `false`
    /// then.
    pub fn hit(&mut self, t: &Tune) -> bool {
        if self.shield > 0.0 {
            self.shield = 0.0;
            return false;
        }
        self.spin = t.spinout_s;
        // A little hop as it spins, so the spin-out reads from behind.
        self.vy = self.vy.max(4.5);
        self.speed *= 0.3;
        self.drift = 0.0;
        self.charge = 0.0;
        self.boost = 0.0;
        true
    }

    /// Re-read where the kart is on the track.
    pub fn relocate(&mut self, track: &Track) -> Located {
        let l = track.locate(self.pos, self.hint);
        self.hint = l.idx;
        self.s = l.s;
        self.lateral = l.lateral;
        self.on_road = l.lateral.abs() <= track.half_width + 0.3;
        l
    }
}

/// Wheel radius (m) of the drawn kart, for the wheel roll angle.
pub const WHEEL_RADIUS: f32 = 0.36;

/// One fixed step of one kart. `steer_curve` maps speed fraction to the
/// steering factor (the `steer_by_speed` formula); `gravity` is already
/// scaled.
pub fn step(
    k: &mut Kart,
    c: Controls,
    t: &Tune,
    steer_curve: &dyn Fn(f32) -> f32,
    track: &Track,
    gravity: f32,
    dt: f32,
) -> StepEvents {
    let mut ev = StepEvents::default();
    k.prev_pos = k.pos;
    k.prev_yaw = k.yaw;
    let mut c = c;
    if k.spin > 0.0 {
        k.spin = (k.spin - dt).max(0.0);
        let total = t.spinout_s.max(0.05);
        // Two full turns over the spin-out.
        k.spin_angle = (1.0 - k.spin / total) * 2.0 * core::f32::consts::TAU;
        c = Controls::default();
        k.speed = approach(k.speed, 0.0, 6.0 * dt);
        if k.spin == 0.0 {
            k.spin_angle = 0.0;
        }
    }
    k.tier_flash += dt;
    k.steer_vis = approach(k.steer_vis, c.steer.clamp(-1.0, 1.0), 7.0 * dt);
    k.wheel_roll = (k.wheel_roll + k.speed * dt / WHEEL_RADIUS) % core::f32::consts::TAU;
    k.squash = approach(k.squash, 0.0, 5.0 * dt);
    let grounded = k.hop <= 0.0 && k.vy <= 0.0;

    // ---- speed ----
    let mut cap = t.max_speed * k.skill_mult * k.band_mult;
    if !k.on_road {
        cap *= t.offroad_factor;
    }
    let boosting = k.boost > 0.0;
    if boosting {
        cap *= t.boost_speed_mult;
        k.boost = (k.boost - dt).max(0.0);
    }
    k.cap_base = cap;
    if k.slipstream {
        cap *= crate::slipstream::SLIPSTREAM_MULTIPLIER;
    }
    k.cap = cap;
    if boosting && k.speed < cap {
        k.speed = (k.speed + t.boost_accel * dt).min(cap);
    } else if c.brake > 0.05 && c.throttle < 0.5 {
        if k.speed > 0.0 {
            k.speed = (k.speed - t.brake * c.brake * dt).max(0.0);
        } else {
            k.speed = (k.speed - t.accel * 0.6 * c.brake * dt).max(-t.reverse_speed);
        }
    } else if c.throttle > 0.05 {
        let want = cap * c.throttle.min(1.0);
        if k.speed < want {
            k.speed = (k.speed + t.accel * c.throttle * dt).min(want);
        } else {
            k.speed -= (k.speed - want) * (t.drag * dt).min(1.0);
        }
    } else {
        k.speed -= k.speed * (t.drag * dt).min(1.0);
        if k.speed.abs() < 0.05 {
            k.speed = 0.0;
        }
    }
    if k.speed > cap {
        // Off-road or after a boost: bleed down to the cap.
        k.speed -= (k.speed - cap) * (3.0 * dt).min(1.0);
    }

    // ---- drift ----
    if k.drift == 0.0 {
        if c.drift && grounded && c.steer.abs() > 0.2 && k.speed > t.drift_min_speed && k.spin == 0.0 {
            k.drift = -c.steer.signum();
            k.charge = 0.0;
            k.vy = (2.0 * gravity * t.hop_height.max(0.0)).sqrt();
            ev.hopped = true;
        }
    } else {
        let released = !c.drift;
        let too_slow = k.speed < t.drift_min_speed * 0.5;
        if released || too_slow || k.spin > 0.0 {
            let tier = k.tier(t);
            if released && !too_slow && tier > 0 {
                let s = match tier {
                    1 => t.boost_tier1_s,
                    2 => t.boost_tier2_s,
                    _ => t.boost_tier3_s,
                };
                k.boost = k.boost.max(s);
                ev.boost_tier = Some(tier);
            }
            k.drift = 0.0;
            k.charge = 0.0;
        } else {
            let before = k.tier(t);
            let tight = (-c.steer * k.drift).clamp(0.0, 1.0);
            k.charge += t.drift_charge_rate * (0.6 + 0.4 * tight) * dt;
            let after = k.tier(t);
            if after > before {
                ev.tier_up = Some(after);
                k.tier_flash = 0.0;
            }
        }
    }

    // ---- steering ----
    let speed_frac = k.speed.abs() / t.max_speed.max(0.1);
    if k.drift != 0.0 {
        let d = k.drift;
        let shape = (1.0 + t.drift_steer * (-c.steer * d)).clamp(0.2, 1.8);
        k.yaw += d * t.drift_turn * shape * dt;
        k.slip = approach(k.slip, d * t.drift_slip_deg.to_radians(), 4.0 * dt);
    } else {
        let dir = if k.speed < 0.0 { -1.0 } else { 1.0 };
        k.yaw -= c.steer * t.turn_rate * steer_curve(speed_frac) * dir * dt;
        k.slip = approach(k.slip, 0.0, 5.0 * dt);
    }
    k.yaw = wrap_angle(k.yaw);

    // ---- move ----
    let f = k.forward();
    let mut vx = f[0] * k.speed;
    let mut vz = f[1] * k.speed;
    if k.drift != 0.0 {
        // Slide outwards a little.
        let l = k.left();
        vx += l[0] * -k.drift * k.speed * 0.12;
        vz += l[1] * -k.drift * k.speed * 0.12;
    }
    k.pos[0] += vx * dt;
    k.pos[2] += vz * dt;

    // ---- walls ----
    let loc = track.locate(k.pos, k.hint);
    // The kart's edge (not its centre) stops at the wall, so karts never
    // sink into the candy-cane rail drawn there; on the inside of a hairpin
    // the rail comes in (crate::trackgeo::rail_lat), and so does the wall.
    let cap = track.side_cap(loc.s)[if loc.lateral >= 0.0 { 0 } else { 1 }];
    let wall = crate::trackgeo::rail_lat(track, t.verge_width, cap) - crate::trackgeo::RAIL_OUT;
    let limit = (wall - t.kart_radius).max(track.half_width);
    if loc.lateral.abs() > limit {
        let left = [loc.tangent[1], -loc.tangent[0]];
        let over = loc.lateral - loc.lateral.signum() * limit;
        k.pos[0] -= left[0] * over;
        k.pos[2] -= left[1] * over;
        // Lose speed by how squarely the kart hits: head-on loses the full
        // `wall_speed_loss`, grinding along the wall almost nothing (else a
        // kart held against a curving wall stops dead every tick).
        let out = loc.lateral.signum();
        let square = ((f[0] * left[0] + f[1] * left[1]) * out * k.speed.signum()).clamp(0.0, 1.0);
        let loss = t.wall_speed_loss * square;
        let lost = k.speed.abs() * loss;
        k.speed *= 1.0 - loss;
        // Turn back along the wall.
        let along = loc.tangent[0].atan2(loc.tangent[1]);
        let along = if (wrap_angle(along - k.yaw)).abs() > core::f32::consts::FRAC_PI_2 {
            wrap_angle(along + core::f32::consts::PI)
        } else {
            along
        };
        k.yaw = wrap_angle(k.yaw + wrap_angle(along - k.yaw) * 0.35);
        if lost > 1.5 {
            ev.wall = Some(lost);
        }
    }

    // ---- height ----
    if !grounded || k.vy > 0.0 {
        k.vy -= gravity * dt;
        k.hop += k.vy * dt;
        if k.hop <= 0.0 {
            k.squash = k.squash.max((-k.vy / 5.0).clamp(0.0, 1.0));
            k.hop = 0.0;
            k.vy = 0.0;
        }
    }
    let road = k.relocate(track).height;
    k.pos[1] = road + k.hop;
    ev
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track() -> Track {
        let mut t = Track::from_json(include_str!("../data/track.json")).unwrap();
        t.set_line_inset(0.55);
        t
    }

    fn start(t: &Track) -> Kart {
        let p = t.point_at(10.0, 0.0);
        let mut k = Kart::new(0, None, p, t.samples[5].yaw());
        k.relocate(t);
        k
    }

    fn flat(_: f32) -> f32 {
        1.0
    }

    /// Open ground: no walls or grass slow-down in the way.
    fn open() -> Tune {
        Tune {
            verge_width: 1000.0,
            offroad_factor: 1.0,
            ..Tune::default()
        }
    }

    #[test]
    fn accelerates_to_cap_and_coasts_down() {
        let (t, tune) = (track(), open());
        let mut k = start(&t);
        let gas = Controls {
            throttle: 1.0,
            ..Default::default()
        };
        for _ in 0..60 {
            step(&mut k, gas, &tune, &flat, &t, 17.0, 1.0 / 60.0);
        }
        assert!((k.speed - tune.accel).abs() < 0.5, "{}", k.speed);
        for _ in 0..240 {
            step(&mut k, gas, &tune, &flat, &t, 17.0, 1.0 / 60.0);
        }
        assert!(k.speed <= tune.max_speed + 0.01 && k.speed > tune.max_speed * 0.5);
        let v = k.speed;
        for _ in 0..60 {
            step(&mut k, Controls::default(), &tune, &flat, &t, 17.0, 1.0 / 60.0);
        }
        assert!(k.speed < v);
    }

    #[test]
    fn steer_right_turns_yaw_down() {
        let (t, tune) = (track(), Tune::default());
        let mut k = start(&t);
        k.speed = 15.0;
        let y0 = k.yaw;
        let c = Controls {
            steer: 1.0,
            throttle: 1.0,
            ..Default::default()
        };
        step(&mut k, c, &tune, &flat, &t, 17.0, 1.0 / 60.0);
        assert!(wrap_angle(k.yaw - y0) < 0.0);
    }

    #[test]
    fn drift_charges_and_fires_tiered_boosts() {
        let (t, tune) = (track(), open());
        let mut k = start(&t);
        k.speed = 20.0;
        let mut c = Controls {
            steer: -1.0,
            throttle: 1.0,
            drift: true,
            ..Default::default()
        };
        let ev = step(&mut k, c, &tune, &flat, &t, 17.0, 1.0 / 60.0);
        assert!(ev.hopped);
        assert_eq!(k.drift, 1.0, "left steer drifts with the yaw going up");
        let mut tiers = Vec::new();
        for _ in 0..200 {
            let e = step(&mut k, c, &tune, &flat, &t, 17.0, 1.0 / 60.0);
            tiers.extend(e.tier_up);
        }
        assert_eq!(tiers, [1, 2, 3]);
        c.drift = false;
        let e = step(&mut k, c, &tune, &flat, &t, 17.0, 1.0 / 60.0);
        assert_eq!(e.boost_tier, Some(3));
        assert!((k.boost - tune.boost_tier3_s).abs() < 0.05);
        assert_eq!(k.drift, 0.0);
    }

    #[test]
    fn walls_hold_karts_on_the_track() {
        let (t, tune) = (track(), Tune::default());
        let mut k = start(&t);
        k.speed = 20.0;
        // Aim straight at the left wall.
        k.yaw += core::f32::consts::FRAC_PI_2;
        let c = Controls {
            throttle: 1.0,
            ..Default::default()
        };
        let mut walls = 0;
        for _ in 0..240 {
            let e = step(&mut k, c, &tune, &flat, &t, 17.0, 1.0 / 60.0);
            walls += usize::from(e.wall.is_some());
            assert!(k.lateral.abs() <= t.half_width + tune.verge_width + 0.5, "{}", k.lateral);
        }
        assert!(walls >= 1);
    }

    #[test]
    fn shield_blocks_one_hit() {
        let tune = Tune::default();
        let mut k = Kart::new(0, None, [0.0; 3], 0.0);
        k.speed = 20.0;
        k.shield = 3.0;
        assert!(!k.hit(&tune));
        assert_eq!(k.speed, 20.0);
        assert!(k.hit(&tune));
        assert!(k.spin > 0.0 && k.speed < 10.0);
    }
}
