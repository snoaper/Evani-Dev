//! A bot brain: a small state machine with hearing, sight that the world's
//! walls block (line of sight to the chest or the head), an aim error that
//! settles the longer a foe stays in sight, and a reaction delay before the
//! first shot. Its random choices come from its own seeded
//! [`evani_core::rng::Pcg32`], so the same seed and inputs replay exactly.
//!
//! * **Idle**: a short random wait at spawn, then Hunt.
//! * **Hunt**: walk the route to the orders' goal (a hold spot); on arrival,
//!   Hold.
//! * **Hold**: stand at the spot and watch the approach, sweeping a little.
//! * **Attack**: a foe is in sight (or was, a moment ago): turn to it with a
//!   limited turn rate, fire once it has been seen for the reaction time and
//!   the aim is on; strafe up close, stand and tap at range.
//! * **Investigate**: walk to a heard sound (gunfire, footsteps, a blast, a
//!   hit from out of sight) or to where a lost foe was last seen.
//! * **Retreat**: back off from the foe while reloading or hurt, then come
//!   back.
//!
//! The game moves the body: the brain only says where to move (a world
//! direction), how to turn, and whether to fire, jump, walk or reload.

use evani_core::num::clamp;
use evani_core::rng::Pcg32;
use evani_physics::collide::math::fmax;
use evani_physics::collide::trig::sin;
use evani_physics::collide::{StaticWorld, V3};

use crate::hitscan::Body;
use crate::tune::BotTune;
use crate::{dist, dist_xz, look_dir, wrap, yaw_pitch_of};

/// The states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BotState {
    Idle,
    Hunt,
    Hold,
    Attack,
    Investigate,
    Retreat,
}

/// A noise: where, how far it carries (m), and the team that made it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sound {
    pub pos: V3,
    pub radius: f32,
    pub team: u8,
}

/// Where a bot should be.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Orders {
    /// Feet position to reach and hold.
    pub goal: V3,
    /// What to watch from there (an approach point).
    pub watch: Option<V3>,
}

/// Someone a bot can see (or not): its body and its team.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Foe {
    pub body: Body,
    pub team: u8,
}

/// Finds walking routes (a polyline of feet positions, ending at `to`).
pub trait Router {
    fn route(&self, from: V3, to: V3) -> Option<Vec<V3>>;
}

/// What the bot knows this step.
#[derive(Debug, Clone, Copy)]
pub struct BotIn<'a> {
    pub world: &'a StaticWorld,
    /// Its own index in `others` (skipped).
    pub me: usize,
    pub team: u8,
    pub feet: V3,
    pub eye: V3,
    pub yaw: f32,
    pub pitch: f32,
    pub hp: f32,
    /// Rounds left in the magazine, and whether a reload is running.
    pub mag: u32,
    pub reloading: bool,
    /// Share of a full magazine left (0..1).
    pub mag_frac: f32,
    /// Seconds between the gun's shots.
    pub interval: f32,
    /// Everyone (itself included at `me`).
    pub others: &'a [Foe],
    pub sounds: &'a [Sound],
    /// Someone hit it this step (their index), whether it can see them or not.
    pub hurt_by: Option<usize>,
    pub dt: f32,
}

/// What the bot does this step.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BotOut {
    /// Move along this world `(x, z)` direction (length 0..1).
    pub move_xz: [f32; 2],
    /// Turn this step (rad, the game's yaw and pitch signs).
    pub dyaw: f32,
    pub dpitch: f32,
    pub fire: bool,
    pub walk: bool,
    pub jump: bool,
    pub reload: bool,
}

/// A bot's mind.
#[derive(Debug, Clone)]
pub struct Brain {
    pub state: BotState,
    pub tune: BotTune,
    rng: Pcg32,
    orders: Option<Orders>,
    target: Option<usize>,
    /// Seconds the target has been in sight without a break.
    seen: f32,
    last_seen: Option<V3>,
    lost_t: f32,
    aim_head: bool,
    err: [f32; 2],
    scan_t: f32,
    route: Vec<V3>,
    route_i: usize,
    route_to: Option<V3>,
    investigate: Option<V3>,
    state_t: f32,
    strafe: f32,
    strafe_t: f32,
    stuck_t: f32,
    stuck_at: V3,
    stuck_n: u32,
    burst_t: f32,
    pause_t: f32,
    retreated: bool,
    idle_for: f32,
}

impl Brain {
    pub fn new(tune: BotTune, seed: u64) -> Self {
        let mut rng = Pcg32::seeded(seed);
        let idle_for = rng.range_f32(0.1, 0.7);
        let strafe = if rng.next_f32() < 0.5 { -1.0 } else { 1.0 };
        Brain {
            state: BotState::Idle,
            tune,
            rng,
            orders: None,
            target: None,
            seen: 0.0,
            last_seen: None,
            lost_t: 0.0,
            aim_head: false,
            err: [0.0; 2],
            scan_t: 0.0,
            route: Vec::new(),
            route_i: 0,
            route_to: None,
            investigate: None,
            state_t: 0.0,
            strafe,
            strafe_t: 0.0,
            stuck_t: 0.0,
            stuck_at: [0.0; 3],
            stuck_n: 0,
            burst_t: 0.0,
            pause_t: 0.0,
            retreated: false,
            idle_for,
        }
    }

    /// Back to a fresh life (new orders follow).
    pub fn reset(&mut self) {
        let tune = self.tune.clone();
        let seed = u64::from(self.rng.next_u32());
        *self = Brain::new(tune, seed);
    }

    pub fn set_orders(&mut self, o: Orders) {
        self.orders = Some(o);
        if matches!(self.state, BotState::Hold) {
            self.go(BotState::Hunt);
        }
    }

    pub fn orders(&self) -> Option<Orders> {
        self.orders
    }

    /// The foe it is fighting, if any.
    pub fn target(&self) -> Option<usize> {
        self.target
    }

    /// Where it is investigating, if it is.
    pub fn investigating(&self) -> Option<V3> {
        self.investigate
    }

    /// The route it is walking and the next waypoint's index.
    pub fn route(&self) -> (&[V3], usize) {
        (&self.route, self.route_i)
    }

    fn go(&mut self, s: BotState) {
        if self.state != s {
            self.state = s;
            self.state_t = 0.0;
        }
    }

    fn visible(&self, inp: &BotIn, i: usize) -> bool {
        let Some(f) = inp.others.get(i) else {
            return false;
        };
        f.body.alive
            && dist(inp.eye, f.body.chest()) <= self.tune.view
            && (inp.world.line_of_sight(inp.eye, f.body.chest())
                || inp.world.line_of_sight(inp.eye, f.body.head_centre()))
    }

    fn in_view(&self, inp: &BotIn, p: V3) -> bool {
        let (y, _) = yaw_pitch_of([p[0] - inp.eye[0], 0.0, p[2] - inp.eye[2]]);
        wrap(y - inp.yaw).abs() <= self.tune.fov
    }

    /// Look for the nearest visible foe in the field of view (or the one
    /// that just hurt it); roll a fresh aim error.
    fn scan(&mut self, inp: &BotIn) {
        let mut best: Option<(f32, usize)> = None;
        for (i, f) in inp.others.iter().enumerate() {
            if i == inp.me || f.team == inp.team || !f.body.alive {
                continue;
            }
            let p = f.body.chest();
            let facing = self.in_view(inp, p) || Some(i) == inp.hurt_by || Some(i) == self.target;
            if !facing || !self.visible(inp, i) {
                continue;
            }
            let d = dist(inp.eye, p);
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, i));
            }
        }
        if let Some((d, i)) = best {
            if self.target != Some(i) {
                self.target = Some(i);
                self.seen = 0.0;
                self.aim_head = self.rng.next_f32() < 0.2;
            }
            let e = self.tune.aim_err * (1.0 + self.tune.aim_near / fmax(d, 6.0));
            self.err = [
                (self.rng.next_f32() * 2.0 - 1.0) * e,
                (self.rng.next_f32() * 2.0 - 1.0) * e * 0.6,
            ];
            if !matches!(self.state, BotState::Attack | BotState::Retreat) {
                self.go(BotState::Attack);
            }
        }
    }

    /// The loudest enemy noise it can hear: `(pos, how far inside its
    /// radius, 0..1)`.
    fn hear(&self, inp: &BotIn) -> Option<V3> {
        let mut best: Option<(f32, V3)> = None;
        for s in inp.sounds {
            if s.team == inp.team {
                continue;
            }
            let r = s.radius * self.tune.hearing;
            let d = dist(inp.feet, s.pos);
            if d > r || r <= 0.0 {
                continue;
            }
            let k = d / r;
            if best.is_none_or(|(bk, _)| k < bk) {
                best = Some((k, s.pos));
            }
        }
        best.map(|(_, p)| p)
    }

    fn start_investigate(&mut self, p: V3) {
        self.investigate = Some(p);
        self.route.clear();
        self.route_to = None;
        self.go(BotState::Investigate);
    }

    /// One step of thought.
    pub fn think(&mut self, inp: &BotIn, router: &dyn Router) -> BotOut {
        let dt = fmax(inp.dt, 0.0);
        self.state_t += dt;
        let mut out = BotOut::default();

        // Keep track of the current target every step; look round for a
        // new one every scan period.
        if let Some(t) = self.target {
            if self.visible(inp, t) {
                self.seen += dt;
                self.lost_t = 0.0;
                if let Some(f) = inp.others.get(t) {
                    self.last_seen = Some(f.body.feet);
                }
            } else {
                self.target = None;
                self.seen = 0.0;
                self.lost_t = 0.0;
                if inp.others.get(t).is_none_or(|f| !f.body.alive) {
                    self.last_seen = None;
                }
            }
        }
        self.scan_t -= dt;
        if self.scan_t <= 0.0 || (inp.hurt_by.is_some() && self.target.is_none()) {
            self.scan_t = self.tune.scan + self.rng.next_f32() * self.tune.scan_jitter;
            self.scan(inp);
        }

        // Hearing (and being hit from out of sight) leads to an
        // investigation, unless it is already fighting.
        if self.target.is_none() && !matches!(self.state, BotState::Retreat) {
            let heard = inp
                .hurt_by
                .and_then(|i| inp.others.get(i))
                .map(|f| f.body.feet)
                .or_else(|| self.hear(inp));
            if let Some(p) = heard {
                let busy_near = self.investigate.is_some_and(|q| dist(q, p) < 3.0);
                if !busy_near && (self.state != BotState::Attack || self.lost_t > 0.3) {
                    self.start_investigate(p);
                }
            }
        }

        // Reload when dry, or topping up with nobody in sight.
        if !inp.reloading && (inp.mag == 0 || (self.target.is_none() && inp.mag_frac < 0.35)) {
            out.reload = true;
        }
        // Retreat while reloading in a fight, or once when badly hurt.
        if self.state == BotState::Attack
            && (inp.reloading || inp.mag == 0 || (inp.hp < self.tune.retreat_hp && !self.retreated))
        {
            self.retreated |= inp.hp < self.tune.retreat_hp;
            self.go(BotState::Retreat);
        }

        let mut look: Option<V3> = None;
        let mut wish: Option<V3> = None;
        match self.state {
            BotState::Idle => {
                if self.state_t >= self.idle_for && self.orders.is_some() {
                    self.go(BotState::Hunt);
                }
            }
            BotState::Hunt => {
                if let Some(o) = self.orders {
                    match self.follow(inp, router, o.goal, &mut out) {
                        Some(p) => {
                            wish = Some(p);
                            look = Some([p[0], inp.eye[1], p[2]]);
                        }
                        None => self.go(BotState::Hold),
                    }
                }
            }
            BotState::Hold => {
                if let Some(o) = self.orders {
                    if dist_xz(inp.feet, o.goal) > 2.5 {
                        self.go(BotState::Hunt);
                    }
                    let w = o.watch.unwrap_or_else(|| {
                        let f = look_dir(inp.yaw, 0.0);
                        [inp.eye[0] + f[0], inp.eye[1], inp.eye[2] + f[2]]
                    });
                    let (y, _) = yaw_pitch_of([w[0] - inp.eye[0], 0.0, w[2] - inp.eye[2]]);
                    let sweep = sin(self.state_t * 0.6) * 0.35;
                    let d = look_dir(y + sweep, 0.0);
                    look = Some([
                        inp.eye[0] + d[0] * 10.0,
                        inp.eye[1],
                        inp.eye[2] + d[2] * 10.0,
                    ]);
                }
            }
            BotState::Investigate => {
                let Some(p) = self.investigate else {
                    self.go(BotState::Hunt);
                    return self.finish(inp, out, look, wish);
                };
                if self.state_t > self.tune.investigate {
                    self.investigate = None;
                    self.go(BotState::Hunt);
                } else {
                    match self.follow(inp, router, p, &mut out) {
                        Some(w) => {
                            wish = Some(w);
                            // Eyes on the noise once close, else down the route.
                            look = Some(if dist(inp.feet, p) < 12.0 {
                                [p[0], p[1] + 1.0, p[2]]
                            } else {
                                [w[0], inp.eye[1], w[2]]
                            });
                            out.walk = dist(inp.feet, p) < 10.0;
                        }
                        None => {
                            self.investigate = None;
                            self.go(BotState::Hunt);
                        }
                    }
                }
            }
            BotState::Attack => {
                if let Some(t) = self.target.and_then(|t| inp.others.get(t)) {
                    let d = dist(inp.eye, t.body.chest());
                    look = Some(if self.aim_head {
                        t.body.head_centre()
                    } else {
                        t.body.chest()
                    });
                    // Strafe up close; stand still and tap at range.
                    if d < self.tune.burst_range {
                        self.strafe_t += dt;
                        if self.strafe_t > self.tune.strafe * self.rng.range_f32(0.7, 1.3) {
                            self.strafe_t = 0.0;
                            self.strafe = -self.strafe;
                        }
                        let f = look_dir(inp.yaw, 0.0);
                        // Right of yaw in the game's frame: (-cos, sin).
                        wish = Some([
                            inp.feet[0] - f[2] * self.strafe,
                            inp.feet[1],
                            inp.feet[2] + f[0] * self.strafe,
                        ]);
                        out.walk = true;
                    }
                    out.fire = self.seen >= self.tune.react && inp.mag > 0 && !inp.reloading;
                    if d >= self.tune.burst_range {
                        if self.pause_t > 0.0 {
                            self.pause_t -= dt;
                            out.fire = false;
                        } else if out.fire {
                            self.burst_t += dt;
                            if self.burst_t >= self.tune.burst as f32 * inp.interval {
                                self.burst_t = 0.0;
                                self.pause_t = self.tune.burst_pause;
                            }
                        }
                    }
                } else if let Some(p) = self.last_seen {
                    self.lost_t += dt;
                    look = Some([p[0], p[1] + 1.0, p[2]]);
                    if self.lost_t > self.tune.lost {
                        self.start_investigate(p);
                    }
                } else {
                    self.go(BotState::Hunt);
                }
            }
            BotState::Retreat => {
                let from = self
                    .target
                    .and_then(|t| inp.others.get(t))
                    .map(|f| f.body.feet)
                    .or(self.last_seen);
                if let Some(p) = from {
                    look = Some([p[0], p[1] + 1.0, p[2]]);
                    let away = [
                        inp.feet[0] * 2.0 - p[0],
                        inp.feet[1],
                        inp.feet[2] * 2.0 - p[2],
                    ];
                    wish = Some(away);
                }
                if let Some(t) = self.target.and_then(|t| inp.others.get(t)) {
                    look = Some(t.body.chest());
                    out.fire = self.seen >= self.tune.react && inp.mag > 0 && !inp.reloading;
                }
                let done = self.state_t > self.tune.retreat_time && !inp.reloading;
                if done {
                    if self.target.is_some() {
                        self.go(BotState::Attack);
                    } else if let Some(p) = self.last_seen {
                        self.start_investigate(p);
                    } else {
                        self.go(BotState::Hunt);
                    }
                }
            }
        }
        self.finish(inp, out, look, wish)
    }

    /// Turn towards `look` (with the aim error while fighting) and move
    /// towards `wish`.
    fn finish(
        &mut self,
        inp: &BotIn,
        mut out: BotOut,
        look: Option<V3>,
        wish: Option<V3>,
    ) -> BotOut {
        let dt = fmax(inp.dt, 0.0);
        if let Some(p) = look {
            let (mut y, mut pi) =
                yaw_pitch_of([p[0] - inp.eye[0], p[1] - inp.eye[1], p[2] - inp.eye[2]]);
            let fighting = self.target.is_some();
            if fighting {
                let settle = fmax(
                    1.0 - self.seen / fmax(self.tune.settle, 1e-3),
                    self.tune.settle_min,
                );
                y += self.err[0] * settle;
                pi += self.err[1] * settle;
            }
            let max = self.tune.turn * dt;
            out.dyaw = clamp(wrap(y - inp.yaw), -max, max);
            out.dpitch = clamp(pi - inp.pitch, -max, max);
            if fighting {
                let left = fmax(
                    wrap(y - inp.yaw - out.dyaw).abs(),
                    (pi - inp.pitch - out.dpitch).abs(),
                );
                out.fire &= left <= self.tune.fire_cone;
            }
        } else {
            out.dpitch = clamp(-inp.pitch, -self.tune.turn * dt, self.tune.turn * dt);
        }
        if self.target.is_none() {
            out.fire = false;
        }
        if let Some(w) = wish {
            let d = [w[0] - inp.feet[0], w[2] - inp.feet[2]];
            let l = (d[0] * d[0] + d[1] * d[1]).sqrt();
            if l > 1e-4 {
                out.move_xz = [d[0] / l, d[1] / l];
            }
        }
        out
    }

    /// Walk the route to `goal`; the next waypoint, or `None` on arrival.
    fn follow(
        &mut self,
        inp: &BotIn,
        router: &dyn Router,
        goal: V3,
        out: &mut BotOut,
    ) -> Option<V3> {
        if dist_xz(inp.feet, goal) < fmax(self.tune.reach, 0.6)
            && (inp.feet[1] - goal[1]).abs() < 1.5
        {
            self.route.clear();
            self.route_to = None;
            return None;
        }
        if self.route_to.is_none_or(|g| dist(g, goal) > 0.5) || self.route_i >= self.route.len() {
            self.route = router.route(inp.feet, goal).unwrap_or_else(|| vec![goal]);
            self.route_i = 0;
            self.route_to = Some(goal);
            self.stuck_t = 0.0;
            self.stuck_at = inp.feet;
        }
        while self.route_i + 1 < self.route.len()
            && dist_xz(inp.feet, self.route[self.route_i]) < self.tune.reach
        {
            self.route_i += 1;
        }
        // Stuck: little progress for a second means jump and look again.
        self.stuck_t += inp.dt;
        if self.stuck_t > 1.0 {
            if dist_xz(inp.feet, self.stuck_at) < 0.3 {
                out.jump = true;
                self.stuck_n += 1;
                if self.stuck_n >= 2 {
                    self.stuck_n = 0;
                    self.route_to = None;
                    if self.route_i + 1 < self.route.len() {
                        self.route_i += 1;
                    }
                }
            } else {
                self.stuck_n = 0;
            }
            self.stuck_t = 0.0;
            self.stuck_at = inp.feet;
        }
        self.route.get(self.route_i).copied().or(Some(goal))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tune::Tuning;
    use evani_physics::collide::WorldBuilder;

    struct Straight;
    impl Router for Straight {
        fn route(&self, _from: V3, to: V3) -> Option<Vec<V3>> {
            Some(vec![to])
        }
    }

    fn floor() -> StaticWorld {
        let mut b = WorldBuilder::new();
        b.aabb([-50.0, -0.2, -50.0], [50.0, 0.0, 50.0], 0);
        b.build()
    }

    fn body(x: f32, z: f32) -> Body {
        Body::new([x, 0.0, z], 1.372, 1.219)
    }

    fn input<'a>(
        w: &'a StaticWorld,
        others: &'a [Foe],
        sounds: &'a [Sound],
        yaw: f32,
    ) -> BotIn<'a> {
        BotIn {
            world: w,
            me: 0,
            team: 0,
            feet: others[0].body.feet,
            eye: [others[0].body.feet[0], 1.219, others[0].body.feet[2]],
            yaw,
            pitch: 0.0,
            hp: 100.0,
            mag: 30,
            reloading: false,
            mag_frac: 1.0,
            interval: 0.1,
            others,
            sounds,
            hurt_by: None,
            dt: 1.0 / 60.0,
        }
    }

    #[test]
    fn shoots_a_visible_target_only_after_the_reaction_delay() {
        let w = floor();
        let tune = Tuning::default().bot;
        let others = [
            Foe {
                body: body(0.0, 0.0),
                team: 0,
            },
            Foe {
                body: body(0.0, 10.0),
                team: 1,
            },
        ];
        let mut b = Brain::new(tune.clone(), 9);
        b.set_orders(Orders {
            goal: [0.0, 0.0, 0.0],
            watch: None,
        });
        let (mut yaw, mut pitch) = (0.0f32, 0.0f32);
        let mut first_fire = None;
        for k in 0..120 {
            let mut inp = input(&w, &others, &[], yaw);
            inp.pitch = pitch;
            let o = b.think(&inp, &Straight);
            yaw = wrap(yaw + o.dyaw);
            pitch += o.dpitch;
            let t = (k + 1) as f32 / 60.0;
            if o.fire && first_fire.is_none() {
                first_fire = Some(t);
            }
        }
        assert_eq!(b.state, BotState::Attack);
        let t = first_fire.expect("fires in two seconds");
        assert!(t >= tune.react, "no shot before the reaction delay: {t}");
        assert!(t < tune.react + 0.5, "shoots soon after it: {t}");
        // A foe behind a wall is never shot at.
        let mut wb = WorldBuilder::new();
        wb.aabb([-50.0, -0.2, -50.0], [50.0, 0.0, 50.0], 0);
        wb.aabb([-5.0, 0.0, 5.0], [5.0, 3.0, 5.5], 1);
        let walled = wb.build();
        let mut b = Brain::new(tune, 9);
        b.set_orders(Orders {
            goal: [0.0, 0.0, 0.0],
            watch: None,
        });
        for _ in 0..120 {
            let o = b.think(&input(&walled, &others, &[], 0.0), &Straight);
            assert!(!o.fire);
        }
        assert_ne!(b.state, BotState::Attack);
    }

    #[test]
    fn hearing_leads_to_investigate() {
        let w = floor();
        let tune = Tuning::default().bot;
        let others = [Foe {
            body: body(0.0, 0.0),
            team: 0,
        }];
        let mut b = Brain::new(tune, 4);
        b.set_orders(Orders {
            goal: [0.0, 0.0, 0.0],
            watch: Some([0.0, 1.0, 10.0]),
        });
        for _ in 0..60 {
            b.think(&input(&w, &others, &[], 0.0), &Straight);
        }
        assert_eq!(b.state, BotState::Hold);
        // A friendly gunshot and an enemy one too far away: nothing.
        let quiet = [
            Sound {
                pos: [20.0, 0.0, 0.0],
                radius: 40.0,
                team: 0,
            },
            Sound {
                pos: [0.0, 0.0, -30.0],
                radius: 9.0,
                team: 1,
            },
        ];
        b.think(&input(&w, &others, &quiet, 0.0), &Straight);
        assert_eq!(b.state, BotState::Hold);
        // An enemy gunshot 20 m behind it: it goes to look.
        let shot = [Sound {
            pos: [0.0, 0.0, -20.0],
            radius: 40.0,
            team: 1,
        }];
        let o = b.think(&input(&w, &others, &shot, 0.0), &Straight);
        assert_eq!(b.state, BotState::Investigate);
        assert_eq!(b.investigating(), Some([0.0, 0.0, -20.0]));
        assert!(o.move_xz[1] < -0.9, "walks towards it: {:?}", o.move_xz);
    }

    #[test]
    fn hunts_to_its_goal_then_holds() {
        let w = floor();
        let mut b = Brain::new(Tuning::default().bot, 5);
        b.set_orders(Orders {
            goal: [0.0, 0.0, 6.0],
            watch: None,
        });
        let mut feet = [0.0f32, 0.0, 0.0];
        for _ in 0..600 {
            let others = [Foe {
                body: body(feet[0], feet[2]),
                team: 0,
            }];
            let o = b.think(&input(&w, &others, &[], 0.0), &Straight);
            feet[0] += o.move_xz[0] * 4.76 / 60.0;
            feet[2] += o.move_xz[1] * 4.76 / 60.0;
        }
        assert_eq!(b.state, BotState::Hold);
        assert!(dist_xz(feet, [0.0, 0.0, 6.0]) < 0.7, "{feet:?}");
    }

    #[test]
    fn a_wall_hides_a_foe_until_it_steps_out() {
        // A wall across +z from x = -3 to 3; the bot at the origin faces it.
        let mut wb = WorldBuilder::new();
        wb.aabb([-50.0, -0.2, -50.0], [50.0, 0.0, 50.0], 0);
        wb.aabb([-3.0, 0.0, 5.0], [3.0, 3.0, 5.4], 1);
        let w = wb.build();
        let mut b = Brain::new(Tuning::default().bot, 21);
        b.set_orders(Orders {
            goal: [0.0, 0.0, 0.0],
            watch: Some([0.0, 1.0, 10.0]),
        });
        let mut yaw = 0.0f32;
        let mut fired_hidden = false;
        // Behind the wall for two seconds: never targeted, never shot at.
        for _ in 0..120 {
            let others = [
                Foe {
                    body: body(0.0, 0.0),
                    team: 0,
                },
                Foe {
                    body: body(1.0, 9.0),
                    team: 1,
                },
            ];
            let o = b.think(&input(&w, &others, &[], yaw), &Straight);
            yaw = wrap(yaw + o.dyaw);
            fired_hidden |= o.fire;
        }
        assert!(!fired_hidden && b.target().is_none(), "{:?}", b.state);
        // It steps out past the wall's end: seen, then fired at.
        let mut fired = false;
        for _ in 0..120 {
            let others = [
                Foe {
                    body: body(0.0, 0.0),
                    team: 0,
                },
                Foe {
                    body: body(8.0, 9.0),
                    team: 1,
                },
            ];
            let o = b.think(&input(&w, &others, &[], yaw), &Straight);
            yaw = wrap(yaw + o.dyaw);
            fired |= o.fire;
        }
        assert_eq!(b.target(), Some(1));
        assert!(fired, "shoots once it can see the foe");
    }

    #[test]
    fn retreats_to_reload_then_comes_back() {
        let w = floor();
        let mut b = Brain::new(Tuning::default().bot, 8);
        b.set_orders(Orders {
            goal: [0.0, 0.0, 0.0],
            watch: None,
        });
        let others = [
            Foe {
                body: body(0.0, 0.0),
                team: 0,
            },
            Foe {
                body: body(0.0, 8.0),
                team: 1,
            },
        ];
        let mut yaw = 0.0f32;
        for _ in 0..60 {
            let o = b.think(&input(&w, &others, &[], yaw), &Straight);
            yaw = wrap(yaw + o.dyaw);
        }
        assert_eq!(b.state, BotState::Attack);
        // The magazine runs dry: it asks to reload and backs off.
        let mut inp = input(&w, &others, &[], yaw);
        inp.mag = 0;
        let o = b.think(&inp, &Straight);
        assert!(o.reload && !o.fire);
        assert_eq!(b.state, BotState::Retreat);
        assert!(
            o.move_xz[1] < -0.5,
            "backs away from the foe: {:?}",
            o.move_xz
        );
        // Reloaded and the retreat time over: back to the fight.
        for _ in 0..200 {
            let o = b.think(&input(&w, &others, &[], yaw), &Straight);
            yaw = wrap(yaw + o.dyaw);
        }
        assert_eq!(b.state, BotState::Attack);
    }
}
