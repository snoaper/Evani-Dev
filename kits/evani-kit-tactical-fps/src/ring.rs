//! The middle level's ring of cars, ported from Hexagon's `rules.ts`
//! (`Ring`, `rotMs`, `ringAngle`, `ringAdvance`, `carAt`). Every
//! `ring_interval_s` the ring turns one sector (60 degrees) over
//! `ring_rotate_s`, eased; what stands in a car is carried with it
//! ([`crate::arena::Arena::carry`]).

use crate::arena::S60;

/// What [`Ring::advance`] reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingEvent {
    /// Nothing new.
    None,
    /// A turn started.
    Started,
    /// A turn finished.
    Finished,
}

/// The ring timer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ring {
    /// Whole sectors turned (0..6).
    pub index: u32,
    /// Time since the last turn started (s).
    pub timer: f32,
    /// Progress of a running turn (s), `None` while still.
    pub turning: Option<f32>,
    /// Time between turns (s).
    pub interval: f32,
}

fn ease(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

impl Ring {
    /// A still ring with turns every `interval` seconds.
    pub fn new(interval: f32) -> Self {
        Ring { index: 0, timer: 0.0, turning: None, interval }
    }

    /// How long one turn takes: `rotate`, never more than 40 % of the interval.
    pub fn turn_time(&self, rotate: f32) -> f32 {
        rotate.min(self.interval * 0.4).max(1e-3)
    }

    /// The ring angle (radians): whole sectors plus the eased progress of a running turn.
    pub fn angle(&self, rotate: f32) -> f32 {
        let e = self.turning.map_or(0.0, |p| ease((p / self.turn_time(rotate)).clamp(0.0, 1.0)));
        ((self.index % 6) as f32 + e) * S60
    }

    /// Advance by `dt` seconds.
    pub fn advance(&mut self, dt: f32, rotate: f32) -> RingEvent {
        let mut ev = RingEvent::None;
        self.timer += dt;
        if let Some(p) = self.turning {
            let p = p + dt;
            if p >= self.turn_time(rotate) {
                self.index = (self.index + 1) % 6;
                self.turning = None;
                ev = RingEvent::Finished;
            } else {
                self.turning = Some(p);
            }
        }
        if self.timer >= self.interval {
            self.timer -= self.interval;
            self.turning = Some(0.0);
            ev = RingEvent::Started;
        }
        ev
    }

    /// Seconds until the next turn starts (0 while turning).
    pub fn until_turn(&self) -> f32 {
        if self.turning.is_some() { 0.0 } else { (self.interval - self.timer).max(0.0) }
    }

    /// Which ring car (ring-local sector) is at world sector `ws`.
    pub fn car_at(&self, ws: usize) -> usize {
        (ws as i64 - i64::from(self.index)).rem_euclid(6) as usize
    }
}
