//! The incoming-item warning: when a Seeker Star is homing in on a player or
//! a Boing Bomb is about to reach one, the HUD says so and a cue rises in
//! pitch and speed as the item nears (genre playbook: "a hit that could not
//! be seen coming feels unfair").
//!
//! [`assess`] is pure: it reads a kart and the items in flight and says how
//! urgent the threat is (0 far, 1 about to hit). The HUD keeps the result
//! ([`crate::hud::PlayerHud::threat`]); [`cue_gap`] and [`cue_step`] pace and
//! pitch the sound.

use crate::items::{Bomb, Seeker};
use crate::kart::Kart;
use crate::sfx::Sfx;

/// A Seeker Star that is homing on a kart warns from this far away (m): about
/// two seconds at its speed.
pub const SEEKER_RANGE: f32 = 90.0;
/// A Boing Bomb warns when it would reach the kart's blast ring within this
/// many seconds.
pub const BOMB_WINDOW_S: f32 = 2.5;
/// ... and only within this distance (m), so a far bomb on a long arc waits.
pub const BOMB_RANGE: f32 = 60.0;
/// The least urgency a warning shows (so the first frame reads).
const MIN_LEVEL: f32 = 0.1;

/// Which item is coming.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreatKind {
    Seeker,
    Bomb,
}

impl ThreatKind {
    /// The HUD's words for it.
    pub fn label(self) -> &'static str {
        match self {
            ThreatKind::Seeker => "SEEKER INCOMING!",
            ThreatKind::Bomb => "BOMB INCOMING!",
        }
    }
}

/// An item coming at a kart.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Threat {
    pub kind: ThreatKind,
    /// Urgency, 0.1 (just noticed) to 1.0 (about to hit).
    pub level: f32,
}

/// The most urgent item coming at kart `me` (grid index `index`), if any.
/// `bomb_radius` is the blast radius (`Tune::bomb_radius`).
pub fn assess(index: usize, me: &Kart, seekers: &[Seeker], bombs: &[Bomb], bomb_radius: f32) -> Option<Threat> {
    let mut best: Option<Threat> = None;
    let mut offer = |t: Threat| {
        if best.is_none_or(|b| t.level > b.level) {
            best = Some(t);
        }
    };
    for s in seekers {
        if s.target != Some(index) || s.life <= 0.0 {
            continue;
        }
        let d = dist(s.pos, me.pos);
        if d <= SEEKER_RANGE {
            offer(Threat {
                kind: ThreatKind::Seeker,
                level: (1.0 - d / SEEKER_RANGE).clamp(MIN_LEVEL, 1.0),
            });
        }
    }
    let f = me.forward();
    let vel = [f[0] * me.speed, 0.0, f[1] * me.speed];
    for b in bombs {
        // Your own bomb leaves your hands harmlessly.
        if b.owner == index && b.grace > 0.0 {
            continue;
        }
        let rel = [b.pos[0] - me.pos[0], b.pos[1] - me.pos[1], b.pos[2] - me.pos[2]];
        let d = (rel[0] * rel[0] + rel[1] * rel[1] + rel[2] * rel[2]).sqrt();
        if d > BOMB_RANGE {
            continue;
        }
        if d <= bomb_radius {
            offer(Threat {
                kind: ThreatKind::Bomb,
                level: 1.0,
            });
            continue;
        }
        // Closing speed: how fast the gap shrinks.
        let rv = [b.vel[0] - vel[0], b.vel[1] - vel[1], b.vel[2] - vel[2]];
        let closing = -(rel[0] * rv[0] + rel[1] * rv[1] + rel[2] * rv[2]) / d;
        if closing > 1.0 {
            let reach_s = (d - bomb_radius) / closing;
            if reach_s <= BOMB_WINDOW_S {
                offer(Threat {
                    kind: ThreatKind::Bomb,
                    level: (1.0 - reach_s / BOMB_WINDOW_S).clamp(MIN_LEVEL, 1.0),
                });
            }
        }
    }
    best
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// Seconds between warning cues: slow when it is far, fast when it is close.
pub fn cue_gap(level: f32) -> f32 {
    let l = if level.is_finite() { level.clamp(0.0, 1.0) } else { 0.0 };
    0.6 + (0.12 - 0.6) * l
}

/// The cue's pitch step: three notes, each higher than the last.
pub fn cue_step(level: f32) -> Sfx {
    if level < 0.34 {
        Sfx::WarnFar
    } else if level < 0.67 {
        Sfx::WarnNear
    } else {
        Sfx::WarnNow
    }
}

/// Run one player's cue timer by `dt` seconds. `threat` is the current
/// threat (None resets the timer so the next one cues at once); returns the
/// cue to play now, if it is time.
pub fn cue(timer: &mut f32, threat: Option<Threat>, racing: bool, dt: f32) -> Option<Sfx> {
    let Some(t) = threat.filter(|_| racing) else {
        *timer = 0.0;
        return None;
    };
    *timer -= dt;
    if *timer <= 0.0 {
        *timer = cue_gap(t.level);
        Some(cue_step(t.level))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A kart at the origin heading +z (yaw 0) at `speed`.
    fn kart(speed: f32) -> Kart {
        let mut k = Kart::new(0, Some(0), [0.0, 0.0, 0.0], 0.0);
        k.speed = speed;
        k
    }

    fn seeker(target: Option<usize>, z: f32) -> Seeker {
        Seeker {
            pos: [0.0, 0.0, z],
            target,
            life: 5.0,
            ..Seeker::default()
        }
    }

    fn bomb(pos: [f32; 3], vel: [f32; 3]) -> Bomb {
        Bomb {
            pos,
            vel,
            owner: 3,
            ..Bomb::default()
        }
    }

    #[test]
    fn a_seeker_aimed_at_you_warns_and_gets_more_urgent() {
        let k = kart(20.0);
        assert_eq!(assess(0, &k, &[], &[], 4.0), None);
        let far = assess(0, &k, &[seeker(Some(0), -80.0)], &[], 4.0).unwrap();
        let near = assess(0, &k, &[seeker(Some(0), -20.0)], &[], 4.0).unwrap();
        assert_eq!((far.kind, near.kind), (ThreatKind::Seeker, ThreatKind::Seeker));
        assert!(near.level > far.level, "{} vs {}", near.level, far.level);
        assert!(near.level > 0.7 && far.level < 0.3);
    }

    #[test]
    fn a_seeker_aimed_elsewhere_or_far_away_is_no_threat() {
        let k = kart(20.0);
        assert_eq!(assess(0, &k, &[seeker(Some(5), -20.0)], &[], 4.0), None);
        assert_eq!(assess(0, &k, &[seeker(None, -20.0)], &[], 4.0), None);
        assert_eq!(assess(0, &k, &[seeker(Some(0), -(SEEKER_RANGE + 5.0))], &[], 4.0), None);
    }

    #[test]
    fn a_bomb_flying_at_you_warns_and_one_flying_away_does_not() {
        let k = kart(0.0);
        // 20 m ahead, coming at 15 m/s: reaches the 4 m ring in about a second.
        let coming = assess(0, &k, &[], &[bomb([0.0, 0.0, 20.0], [0.0, 0.0, -15.0])], 4.0).unwrap();
        assert_eq!(coming.kind, ThreatKind::Bomb);
        assert!(coming.level > 0.4 && coming.level < 1.0, "{}", coming.level);
        // The same bomb going away.
        assert_eq!(assess(0, &k, &[], &[bomb([0.0, 0.0, 20.0], [0.0, 0.0, 15.0])], 4.0), None);
        // Too far, or too slow to matter inside the window.
        assert_eq!(assess(0, &k, &[], &[bomb([0.0, 0.0, 120.0], [0.0, 0.0, -50.0])], 4.0), None);
        assert_eq!(assess(0, &k, &[], &[bomb([0.0, 0.0, 40.0], [0.0, 0.0, -3.0])], 4.0), None);
    }

    #[test]
    fn driving_at_a_resting_bomb_warns_and_a_bomb_in_the_ring_is_urgent() {
        let lying = bomb([0.0, 0.0, 30.0], [0.0; 3]);
        assert_eq!(assess(0, &kart(0.0), &[], &[lying], 4.0), None, "sitting still, nothing closes");
        let t = assess(0, &kart(25.0), &[], &[lying], 4.0).unwrap();
        assert!(t.level > 0.0 && t.level < 1.0);
        assert_eq!(assess(0, &kart(0.0), &[], &[bomb([0.0, 0.0, 3.0], [0.0; 3])], 4.0).unwrap().level, 1.0);
    }

    #[test]
    fn your_own_fresh_bomb_is_no_threat_but_a_live_one_is() {
        let k = kart(10.0);
        let mut b = bomb([0.0, 0.0, 6.0], [0.0, 0.0, 14.0]);
        b.owner = 0;
        b.grace = 0.3;
        assert_eq!(assess(0, &k, &[], &[b], 4.0), None);
        // Inside its own blast ring, after the grace, it is.
        b.grace = 0.0;
        b.pos = [0.0, 0.0, 2.0];
        assert!(assess(0, &k, &[], &[b], 4.0).is_some());
    }

    #[test]
    fn the_most_urgent_item_wins() {
        let k = kart(0.0);
        let t = assess(
            0,
            &k,
            &[seeker(Some(0), -85.0)],
            &[bomb([0.0, 0.0, 15.0], [0.0, 0.0, -15.0])],
            4.0,
        )
        .unwrap();
        assert_eq!(t.kind, ThreatKind::Bomb);
    }

    #[test]
    fn the_cue_gets_faster_and_higher_as_it_nears() {
        assert!(cue_gap(0.1) > cue_gap(0.5) && cue_gap(0.5) > cue_gap(1.0));
        assert!(cue_gap(1.0) >= 0.1 && cue_gap(f32::NAN) <= 0.6 + 1e-6);
        assert_eq!(cue_step(0.2), Sfx::WarnFar);
        assert_eq!(cue_step(0.5), Sfx::WarnNear);
        assert_eq!(cue_step(0.9), Sfx::WarnNow);
    }

    #[test]
    fn the_cue_fires_when_the_threat_appears_and_then_at_its_pace() {
        let t = Some(Threat {
            kind: ThreatKind::Seeker,
            level: 1.0,
        });
        let mut timer = 0.0;
        // At once on the first frame, then silent until the gap has passed.
        assert_eq!(cue(&mut timer, t, true, 1.0 / 60.0), Some(Sfx::WarnNow));
        let mut fired = 0;
        for _ in 0..60 {
            if cue(&mut timer, t, true, 1.0 / 60.0).is_some() {
                fired += 1;
            }
        }
        // One second at the 0.12 s gap: about eight more.
        assert!((7..=9).contains(&fired), "{fired}");
        // No threat, or not racing (paused, countdown, results): no sound, timer reset.
        assert_eq!(cue(&mut timer, None, true, 0.016), None);
        assert_eq!(timer, 0.0);
        assert_eq!(cue(&mut timer, t, false, 0.016), None);
        assert_eq!(cue(&mut timer, t, true, 0.016), Some(Sfx::WarnNow));
    }
}
