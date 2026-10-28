//! AI drivers: follow the racing line with a look-ahead point and a lane
//! bias, drift through tight corners, use items by simple rules, and back
//! out when stuck.

use crate::Tune;
use crate::items::{Item, Slot};
use crate::kart::{Controls, Kart, wrap_angle};
use crate::track::Track;
use serde::Deserialize;

/// One roster entry from `karts.json` (all original).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct KartSpec {
    pub name: String,
    /// Linear RGB body colour.
    pub color: [f32; 3],
    /// Linear RGB driver suit colour, contrasting with the kart (default:
    /// the kart colour, muted).
    #[serde(default)]
    pub suit: Option<[f32; 3]>,
    /// 0..1: speed cap spread and drift patience.
    pub skill: f32,
    /// Metres left (+) or right (-) of the racing line.
    pub lane_bias: f32,
    /// 0..1: how quickly items are used.
    pub aggression: f32,
}

#[derive(Debug, Clone, Deserialize)]
struct RosterJson {
    karts: Vec<KartSpec>,
}

/// Parse `karts.json`.
pub fn roster_from_json(json: &str) -> Result<Vec<KartSpec>, String> {
    let r: RosterJson = serde_json::from_str(json).map_err(|e| format!("karts.json: {e}"))?;
    if r.karts.is_empty() {
        return Err("karts.json: no karts".into());
    }
    Ok(r.karts)
}

/// Per-AI memory.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AiState {
    /// Seconds the kart has been nearly stopped.
    pub stuck: f32,
    /// Seconds of reversing left (unsticking).
    pub reverse: f32,
    /// Seconds before the held item may be used.
    pub item_wait: f32,
    /// The item the wait was set for (re-arm on a new item).
    pub armed: Option<Item>,
}

/// What the AI can see of the other karts for item decisions.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Surroundings {
    /// Distance along the track to the nearest kart behind (within 40 m).
    pub behind: Option<f32>,
    /// Distance along the track to the nearest kart ahead (within 40 m).
    pub ahead: Option<f32>,
}

/// Look at the other karts from kart `i`.
pub fn surroundings(karts: &[Kart], i: usize, track: &Track) -> Surroundings {
    let mut s = Surroundings::default();
    let me = &karts[i];
    for (j, o) in karts.iter().enumerate() {
        if j == i || o.finished.is_some() {
            continue;
        }
        let d = track.delta_s(o.s, me.s);
        if (0.5..40.0).contains(&d) {
            s.ahead = Some(s.ahead.map_or(d, |a: f32| a.min(d)));
        } else if (-40.0..-0.5).contains(&d) {
            s.behind = Some(s.behind.map_or(-d, |b: f32| b.min(-d)));
        }
    }
    s
}

#[allow(clippy::too_many_arguments)]
/// Decide this tick's controls. `noise` is a 0..1 value from the race RNG
/// (used to stagger item timing).
pub fn drive(
    k: &Kart,
    spec: &KartSpec,
    ai: &mut AiState,
    around: Surroundings,
    track: &Track,
    t: &Tune,
    noise: f32,
    dt: f32,
) -> Controls {
    let mut c = Controls::default();
    if k.spin > 0.0 {
        return c;
    }

    // ---- unstick ----
    if ai.reverse > 0.0 {
        ai.reverse = (ai.reverse - dt).max(0.0);
        let tan = track.tangent_at(k.s);
        let err = wrap_angle(tan[0].atan2(tan[1]) - k.yaw);
        c.brake = 1.0;
        // Reversing turns the other way.
        c.steer = (err * 2.0).clamp(-1.0, 1.0);
        return c;
    }
    if k.speed.abs() < 2.0 {
        ai.stuck += dt;
    } else {
        ai.stuck = 0.0;
    }
    if ai.stuck > 1.6 {
        ai.stuck = 0.0;
        ai.reverse = 0.9;
    }

    // ---- steering towards the racing line ----
    let ahead = t.ai_lookahead + k.speed.max(0.0) * 0.12;
    let ts = k.s + ahead;
    let hw = track.half_width * 0.85;
    // num::clamp: f32::clamp's result without its panic, whose message links float formatting.
    let lane = evani_core::num::clamp(track.line_at(ts) + spec.lane_bias, -hw, hw);
    let p = track.point_at(ts, lane);
    let want = (p[0] - k.pos[0]).atan2(p[2] - k.pos[2]);
    let err = wrap_angle(want - k.yaw);
    // err > 0 wants the yaw up, which is steering left (negative).
    c.steer = (-err * t.ai_steer_gain).clamp(-1.0, 1.0);
    c.throttle = if err.abs() > 0.7 { 0.55 } else { 1.0 };
    if err.abs() > 1.3 && k.speed > 8.0 {
        c.throttle = 0.0;
        c.brake = 0.6;
    }

    // ---- drift through tight corners ----
    let curve = track.curvature_ahead(k.s + 4.0, 25.0);
    if k.drift == 0.0 {
        let corner_dir = curve.signum();
        let steer_dir = -c.steer.signum();
        if curve.abs() > t.ai_drift_curvature
            && k.speed > t.drift_min_speed + 2.0
            && corner_dir == steer_dir
            && c.steer.abs() > 0.25
        {
            c.drift = true;
        }
    } else {
        // Patient drivers hold for a higher tier; everyone lets go on the
        // corner exit or when the drift turns against the corner.
        let goal = if spec.skill > 0.7 {
            t.drift_tier2
        } else {
            t.drift_tier1
        };
        let against = curve.signum() != k.drift;
        let exit = curve.abs() < t.ai_drift_curvature * 0.55 || against;
        let release =
            exit && (k.charge >= goal || against || curve.abs() < t.ai_drift_curvature * 0.3);
        c.drift = !release;
    }

    // ---- items ----
    if let Slot::Holding(item) = k.slot {
        if ai.armed != Some(item) {
            ai.armed = Some(item);
            ai.item_wait = (0.4 + 2.2 * (1.0 - spec.aggression.clamp(0.0, 1.0))) * (0.5 + noise);
        }
        ai.item_wait -= dt;
        if ai.item_wait <= 0.0 {
            let straight = track.curvature_ahead(k.s, 30.0).abs() < 0.012;
            c.item = match item {
                Item::ZoomPop => straight && k.on_road,
                Item::GooBlob => around.behind.is_some_and(|d| d < 12.0) || ai.item_wait < -6.0,
                Item::BoingBomb => around.ahead.is_some_and(|d| d < 40.0) || ai.item_wait < -8.0,
                Item::SeekerStar => around.ahead.is_some() || ai.item_wait < -5.0,
                Item::ShieldBubble => true,
            };
        }
    } else {
        ai.armed = None;
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roster_has_eight_original_drivers() {
        let r = roster_from_json(include_str!("../data/karts.json")).unwrap();
        assert_eq!(r.len(), 8);
        assert!(r.iter().all(|k| (0.0..=1.0).contains(&k.skill)));
    }

    #[test]
    fn steers_back_towards_the_line() {
        let mut track = Track::from_json(include_str!("../data/track.json")).unwrap();
        track.set_line_inset(0.55);
        let spec = roster_from_json(include_str!("../data/karts.json")).unwrap()[0].clone();
        let t = Tune::default();
        let s = 20.0;
        let mut k = Kart::new(0, None, track.point_at(s, 5.5), track.tangent_at(s)[0].atan2(track.tangent_at(s)[1]));
        k.relocate(&track);
        k.speed = 15.0;
        let c = drive(&k, &spec, &mut AiState::default(), Surroundings::default(), &track, &t, 0.5, 1.0 / 60.0);
        // Far left of the line: steer right.
        assert!(c.steer > 0.0, "{c:?}");
        assert!(c.throttle > 0.5);
    }
}
