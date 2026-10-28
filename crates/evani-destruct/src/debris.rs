//! [`Debris`]: the visual-only rubble. A fixed pool of chunks in flat
//! arrays used as a ring buffer (when full the oldest slot is reused), plus a
//! fixed pool of dust puffs. Nothing allocates after [`Debris::new`].
//!
//! A break throws a few big tumbling lumps from the middle of the hole and a
//! spray of fast gravel from its rim, plus dust. Chunks fall against the
//! caller's `solid(p)` query in short sub-steps so nothing tunnels, bounce,
//! lose speed and spin to friction, lie still when slow on a floor, fall
//! again if that floor is blown away, and after a while (or at the end of
//! their life) sink, shrink and free their slot.

use evani_core::rng::Pcg32;

use crate::tuning::Tuning;

/// Chunks below this height are removed.
const KILL_Y: f32 = -60.0;

/// One live chunk, for drawing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Chunk {
    pub position: [f32; 3],
    /// Edge length (already shrunk while it fades).
    pub size: f32,
    /// Unit spin axis.
    pub axis: [f32; 3],
    /// Spin angle (radians).
    pub angle: f32,
}

/// One dust puff.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dust {
    pub position: [f32; 3],
    velocity: [f32; 3],
    size: f32,
    left: f32,
    life: f32,
    /// Base opacity 0.35..0.6 (drawers that cannot blend scale size by it).
    pub alpha: f32,
    /// Brightness factor 0.85..1.05 of the dust colour.
    pub shade: f32,
}

impl Dust {
    /// World size now: puffs grow as they age.
    pub fn size(&self) -> f32 {
        let k = self.k();
        self.size * (1.0 + (1.0 - k) * 1.6)
    }

    /// Opacity now: fades in fast, thins out with age.
    pub fn opacity(&self) -> f32 {
        let k = self.k();
        self.alpha * k * ((1.0 - k) * 10.0).min(1.0)
    }

    fn k(&self) -> f32 {
        if self.life > 0.0 {
            (self.left / self.life).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }
}

/// The debris pool.
#[derive(Debug, Clone)]
pub struct Debris {
    pos: Vec<[f32; 3]>,
    vel: Vec<[f32; 3]>,
    /// Life left in seconds; 0 = free slot.
    life: Vec<f32>,
    size: Vec<f32>,
    axis: Vec<[f32; 3]>,
    angle: Vec<f32>,
    spin: Vec<f32>,
    /// How long it has lain still (0 = moving).
    rest: Vec<f32>,
    /// Draw scale 0..1 after the last step.
    shown: Vec<f32>,
    head: usize,
    live: usize,
    dust: Vec<Dust>,
    dust_max: usize,
}

fn sign(v: f32) -> f32 {
    if v > 0.0 {
        1.0
    } else if v < 0.0 {
        -1.0
    } else {
        0.0
    }
}

fn norm(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 0.0 {
        [v[0] / l, v[1] / l, v[2] / l]
    } else {
        [0.0, 1.0, 0.0]
    }
}

fn rv(rng: &mut Pcg32) -> [f32; 3] {
    norm([
        rng.range_f32(-1.0, 1.0),
        rng.range_f32(-1.0, 1.0),
        rng.range_f32(-1.0, 1.0),
    ])
}

impl Debris {
    /// A pool of `max` chunks and `dust_max` dust puffs.
    pub fn new(max: usize, dust_max: usize) -> Self {
        let max = max.max(1);
        Self {
            pos: vec![[0.0; 3]; max],
            vel: vec![[0.0; 3]; max],
            life: vec![0.0; max],
            size: vec![0.0; max],
            axis: vec![[0.0, 1.0, 0.0]; max],
            angle: vec![0.0; max],
            spin: vec![0.0; max],
            rest: vec![0.0; max],
            shown: vec![0.0; max],
            head: 0,
            live: 0,
            dust: Vec::with_capacity(dust_max),
            dust_max,
        }
    }

    /// Pool size.
    pub fn capacity(&self) -> usize {
        self.life.len()
    }

    /// Live chunks after the last step.
    pub fn live(&self) -> usize {
        self.live
    }

    /// Chunks lying still among the live ones.
    pub fn resting(&self) -> usize {
        (0..self.capacity())
            .filter(|&i| self.life[i] > 0.0 && self.rest[i] > 0.0)
            .count()
    }

    /// Dust puffs alive.
    pub fn dust_count(&self) -> usize {
        self.dust.len()
    }

    /// Remove every chunk and puff.
    pub fn clear(&mut self) {
        self.life.fill(0.0);
        self.rest.fill(0.0);
        self.shown.fill(0.0);
        self.dust.clear();
        self.head = 0;
        self.live = 0;
    }

    /// `n` chunks flying out of a break of size `r` centred on `p` (`up`:
    /// only upwards, for a block that broke up on landing). Spawn points
    /// inside something solid are skipped. A dust puff follows. Returns how
    /// many chunks were made.
    #[allow(clippy::too_many_arguments)]
    pub fn chunks(
        &mut self,
        p: [f32; 3],
        r: f32,
        n: u32,
        up: bool,
        t: &Tuning,
        rng: &mut Pcg32,
        solid: impl Fn([f32; 3]) -> bool,
    ) -> u32 {
        let s0 = t.debris_size_min;
        let s1 = t.debris_size_max.min((s0 * 1.6).max(r * 0.45));
        let sp = (0.5 + r).min(2.0);
        let lumps = (n as f32 * 0.4).ceil() as u32;
        let max = self.capacity();
        let mut made = 0;
        for i in 0..n {
            let lump = i < lumps;
            let d = norm([
                rng.range_f32(-1.0, 1.0),
                if up {
                    rng.range_f32(0.2, 1.0)
                } else {
                    rng.range_f32(-1.0, 1.0)
                },
                rng.range_f32(-1.0, 1.0),
            ]);
            // Lumps: the nearer the centre they came from, the bigger. Gravel: small, from the rim, fast.
            let f = if lump {
                rng.next_f32().cbrt()
            } else {
                rng.range_f32(0.6, 1.0)
            };
            let size = if lump {
                s0 * 1.5 + (s1 - s0 * 1.5) * (1.0 - f * 0.85).powf(1.5) * rng.range_f32(0.55, 1.0)
            } else {
                s0 * rng.range_f32(0.8, 2.2)
            };
            let v = if lump {
                rng.range_f32(1.0, 3.5) * (0.5 + 0.5 * f)
            } else {
                rng.range_f32(2.5, 7.0)
            } * sp;
            let a = [
                p[0] + d[0] * f * r * 0.8,
                p[1] + d[1] * f * r * 0.8,
                p[2] + d[2] * f * r * 0.8,
            ];
            let lift = rng.range_f32(0.5, 3.0) * sp;
            let life = t.debris_life * rng.range_f32(0.85, 1.0);
            let axis = rv(rng);
            let angle = rng.range_f32(0.0, 6.3);
            let spin = if lump {
                rng.range_f32(-7.0, 7.0)
            } else {
                rng.range_f32(-18.0, 18.0)
            };
            if solid(a) {
                continue;
            }
            let k = self.head;
            self.head = (k + 1) % max;
            self.pos[k] = a;
            self.vel[k] = [d[0] * v, d[1] * v + lift, d[2] * v];
            self.life[k] = life;
            self.rest[k] = 0.0;
            self.size[k] = size;
            self.shown[k] = 1.0;
            self.axis[k] = axis;
            self.angle[k] = angle;
            self.spin[k] = spin;
            made += 1;
        }
        self.dust(p, if up { r * 1.4 } else { r }, rng);
        made
    }

    /// A new hole of radius `r` at `p`: `debris_per_m` chunks per metre of
    /// radius, at least `debris_min`, at most 64.
    pub fn hole(
        &mut self,
        p: [f32; 3],
        r: f32,
        t: &Tuning,
        rng: &mut Pcg32,
        solid: impl Fn([f32; 3]) -> bool,
    ) -> u32 {
        let n = ((t.debris_per_m * r).round().max(0.0) as u32)
            .max(t.debris_min)
            .min(64);
        self.chunks(p, r, n, false, t, rng, solid)
    }

    /// A fallen block of `cells` cells broke up where it landed (`p`: the
    /// touch-down point): chunks thrown up and a big puff of dust.
    pub fn landed(
        &mut self,
        p: [f32; 3],
        cells: f32,
        t: &Tuning,
        rng: &mut Pcg32,
        solid: impl Fn([f32; 3]) -> bool,
    ) -> u32 {
        let c = cells.max(0.0).cbrt();
        let n = ((cells.max(0.0) * t.collapse_debris as f32).round() as u32).min(80);
        let made = self.chunks(
            [p[0], p[1] + 0.35, p[2]],
            (0.5 + c * 0.35).min(1.6),
            n,
            true,
            t,
            rng,
            solid,
        );
        self.dust([p[0], p[1] + 0.5, p[2]], (0.6 + c * 0.3).min(1.5), rng);
        made
    }

    /// A short puff of dust where something broke (`r`: size of the break).
    pub fn dust(&mut self, p: [f32; 3], r: f32, rng: &mut Pcg32) {
        let n = (3.0 + (r * 9.0).round()).min(16.0) as u32;
        for _ in 0..n {
            if self.dust.len() >= self.dust_max {
                break;
            }
            let d = rv(rng);
            let s = (0.4 + r * 1.5) * rng.range_f32(0.3, 1.0);
            let life = rng.range_f32(0.9, 1.7) * (0.7 + r.min(1.5) * 0.4);
            let shade = rng.range_f32(0.85, 1.05);
            let size = (0.25 + r * 0.9) * rng.range_f32(0.6, 1.2);
            let alpha = rng.range_f32(0.35, 0.6);
            self.dust.push(Dust {
                position: [
                    p[0] + d[0] * r * 0.5,
                    p[1] + d[1] * r * 0.5,
                    p[2] + d[2] * r * 0.5,
                ],
                velocity: [d[0] * s, d[1] * s + 0.3, d[2] * s],
                size,
                left: life,
                life,
                alpha,
                shade,
            });
        }
    }

    /// Advance everything by `dt` seconds (call at the fixed tick).
    pub fn step(&mut self, dt: f32, t: &Tuning, solid: impl Fn([f32; 3]) -> bool) {
        let (g, e, fr) = (t.debris_gravity, t.debris_bounce, t.debris_friction);
        let (settle, fade, still) = (
            t.debris_settle,
            t.debris_fade.max(1e-3),
            t.debris_rest_speed,
        );
        let mut n = 0;
        for i in 0..self.capacity() {
            if self.life[i] <= 0.0 {
                continue;
            }
            let left = self.life[i] - dt;
            if left <= 0.0 {
                self.life[i] = 0.0;
                self.shown[i] = 0.0;
                continue;
            }
            n += 1;
            let rad = self.size[i] * 0.3;
            let [mut x, mut y, mut z] = self.pos[i];
            let [mut vx, mut vy, mut vz] = self.vel[i];
            let mut spin = self.spin[i];
            let mut rest = self.rest[i];
            if rest > 0.0 {
                rest += dt;
                if !solid([x, y - rad - 0.06, z]) {
                    rest = 0.0;
                }
            }
            if rest == 0.0 {
                vy -= g * dt;
                let speed = (vx * vx + vy * vy + vz * vz).sqrt();
                let ns = ((speed * dt / 0.12).ceil() as u32).clamp(1, 8);
                let h = dt / ns as f32;
                for _ in 0..ns {
                    let nx = x + vx * h;
                    if solid([nx + sign(vx) * rad, y, z]) {
                        vx = -vx * e;
                        vz *= fr;
                        spin *= -fr;
                    } else {
                        x = nx;
                    }
                    let nz = z + vz * h;
                    if solid([x, y, nz + sign(vz) * rad]) {
                        vz = -vz * e;
                        vx *= fr;
                        spin *= -fr;
                    } else {
                        z = nz;
                    }
                    let ny = y + vy * h;
                    if !solid([x, ny + if vy < 0.0 { -rad } else { rad }, z]) {
                        y = ny;
                        continue;
                    }
                    if vy > 0.0 {
                        vy = -vy * e;
                        continue;
                    }
                    vy = -vy * e;
                    vx *= fr;
                    vz *= fr;
                    spin *= fr;
                    if vy < still && vx.hypot(vz) < still {
                        (vx, vy, vz, spin) = (0.0, 0.0, 0.0, 0.0);
                        rest = 1e-4;
                        break;
                    }
                }
            }
            let mut k = (left / fade).min(1.0);
            if rest > settle {
                k = k.min(1.0 - (rest - settle) / fade);
            }
            self.life[i] = if k <= 0.0 || y < KILL_Y { 0.0 } else { left };
            self.pos[i] = [x, y, z];
            self.vel[i] = [vx, vy, vz];
            self.spin[i] = spin;
            self.rest[i] = rest;
            self.angle[i] += spin * dt;
            self.shown[i] = k.max(0.0);
        }
        self.live = n;
        // Dust: drag, a little gravity, ageing.
        let drag = 1.0 - dt * 1.5;
        for q in &mut self.dust {
            q.left -= dt;
            q.velocity = [
                q.velocity[0] * drag,
                q.velocity[1] * drag - 0.3 * dt,
                q.velocity[2] * drag,
            ];
            q.position = [
                q.position[0] + q.velocity[0] * dt,
                q.position[1] + q.velocity[1] * dt,
                q.position[2] + q.velocity[2] * dt,
            ];
        }
        self.dust.retain(|q| q.left > 0.0);
    }

    /// Every live chunk (sinking and shrinking as it fades).
    pub fn each_chunk(&self, mut f: impl FnMut(Chunk)) {
        for i in 0..self.capacity() {
            if self.life[i] <= 0.0 {
                continue;
            }
            let k = self.shown[i];
            let rad = self.size[i] * 0.3;
            let [x, y, z] = self.pos[i];
            f(Chunk {
                position: [x, y - (1.0 - k) * rad, z],
                size: self.size[i] * k,
                axis: self.axis[i],
                angle: self.angle[i],
            });
        }
    }

    /// Every dust puff.
    pub fn dust_puffs(&self) -> &[Dust] {
        &self.dust
    }

    /// The rubble a [`crate::Structure::step`] calls for: a landed or
    /// crumbled block breaks up (chunks thrown up and a big puff), cut-away
    /// floor crumbs spill a few chunks. Returns the chunks made.
    pub fn react(
        &mut self,
        events: &[crate::DestructEvent],
        t: &Tuning,
        rng: &mut Pcg32,
        solid: impl Fn([f32; 3]) -> bool,
    ) -> u32 {
        use crate::DestructEvent as E;
        let mut made = 0;
        for e in events {
            made += match *e {
                E::Land { pos, blocks, .. } | E::Crumble { pos, blocks } => {
                    self.landed(pos, blocks, t, rng, &solid)
                }
                E::Crumbs { pos, samples } => {
                    self.chunks(pos, 0.5, (samples * 3).min(24), false, t, rng, &solid)
                }
                E::Fall { .. } | E::Out { .. } => 0,
            };
        }
        made
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn floor(p: [f32; 3]) -> bool {
        p[1] < 0.0
    }

    #[test]
    fn a_hole_throws_chunks_and_dust() {
        let t = Tuning::default();
        let mut d = Debris::new(512, 900);
        let mut rng = Pcg32::seeded(1);
        let made = d.hole([0.0, 1.0, 0.0], 0.5, &t, &mut rng, floor);
        assert_eq!(made, 12);
        assert!(d.dust_count() > 0);
        d.step(1.0 / 60.0, &t, floor);
        assert_eq!(d.live(), 12);
    }

    #[test]
    fn chunks_settle_then_despawn_before_their_life_ends() {
        let t = Tuning::default();
        let mut d = Debris::new(512, 900);
        let mut rng = Pcg32::seeded(2);
        d.hole([0.0, 1.5, 0.0], 1.0, &t, &mut rng, floor);
        let dt = 1.0 / 60.0;
        let mut rested_max = 0;
        for _ in 0..(3.0 / dt) as u32 {
            d.step(dt, &t, floor);
            rested_max = rested_max.max(d.resting());
        }
        assert!(d.live() > 0);
        assert_eq!(d.resting(), d.live(), "everything lies still after 3 s");
        assert!(rested_max >= 20);
        // Settle 2.5 s + fade 0.9 s after resting: gone well before the 7.65 s shortest life.
        for _ in 0..(4.0 / dt) as u32 {
            d.step(dt, &t, floor);
        }
        assert_eq!(d.live(), 0);
        assert_eq!(d.dust_count(), 0);
    }

    #[test]
    fn a_resting_chunk_falls_again_when_its_floor_goes() {
        let t = Tuning::default();
        let mut d = Debris::new(64, 0);
        let mut rng = Pcg32::seeded(3);
        d.chunks([0.0, 0.5, 0.0], 0.2, 4, false, &t, &mut rng, floor);
        let dt = 1.0 / 60.0;
        for _ in 0..120 {
            d.step(dt, &t, floor);
        }
        assert!(d.resting() > 0);
        let lower = |p: [f32; 3]| p[1] < -2.0;
        d.step(dt, &t, lower);
        assert_eq!(d.resting(), 0);
    }

    #[test]
    fn the_pool_reuses_the_oldest_slots_and_never_grows() {
        let t = Tuning::default();
        let mut d = Debris::new(32, 16);
        let mut rng = Pcg32::seeded(4);
        let cap = d.dust.capacity();
        for _ in 0..20 {
            d.hole([0.0, 5.0, 0.0], 1.0, &t, &mut rng, floor);
            d.step(1.0 / 60.0, &t, floor);
        }
        assert_eq!(d.capacity(), 32);
        assert_eq!(d.live(), 32);
        assert!(d.dust_count() <= 16);
        assert_eq!(d.dust.capacity(), cap);
    }

    #[test]
    fn spawn_inside_solid_is_skipped() {
        let t = Tuning::default();
        let mut d = Debris::new(64, 0);
        let mut rng = Pcg32::seeded(5);
        assert_eq!(
            d.chunks([0.0, 0.0, 0.0], 0.5, 10, false, &t, &mut rng, |_| true),
            0
        );
    }

    #[test]
    fn same_seed_same_rubble() {
        let t = Tuning::default();
        let run = || {
            let mut d = Debris::new(128, 64);
            let mut rng = Pcg32::seeded(9);
            for k in 0..5 {
                d.hole([k as f32, 2.0, 0.0], 0.6, &t, &mut rng, floor);
                for _ in 0..30 {
                    d.step(1.0 / 60.0, &t, floor);
                }
            }
            let mut out = Vec::new();
            d.each_chunk(|c| out.push(c));
            (out, d.dust_puffs().to_vec())
        };
        assert_eq!(run(), run());
    }
}
