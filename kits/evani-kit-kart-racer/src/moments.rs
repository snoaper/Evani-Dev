//! Scripted capture moments for live's `capture.frame { moment }`: the kit's
//! signature shots from the target pack (`kits/kart-racer/target/
//! moments.md`: the countdown and GO, a drift with sparks, a boost, an item
//! hit, an overtake, the final lap, the finish line and the podium result)
//! set up the same way every time. Each one starts a fresh one-player race
//! (racing, or on the grid for the countdown), reseeds the race RNG, lets
//! the AI drive the player's kart
//! ([`Race::autopilot`]) and steps the sim with a fixed dt until the moment
//! holds; then the game camera frames it.
//!
//! A moment only gets `&World`, so it stages the race on the existing kart
//! and item entities ([`race::reset`] and [`race::tick_shared`]: no spawning
//! or despawning, which needs `&mut World`).
//!
//! The app registers them only in builds with live (next to
//! `BenchPath`), so a shipped game never references this code.

use evani_core::capture::CaptureMoments;
use evani_core::ecs::{Entity, World};
use evani_core::formulas::Formulas;
use evani_core::params::ParamRegistry;
use evani_core::rng::Pcg32;

use crate::kart::{Controls, Kart, Launch};
use crate::race::{self, Ctx, Fns, Phase, RACE_SEED, Race, RaceView};
use crate::{KartKit, Tune};

/// Fixed sim step of every moment (seconds).
pub const DT: f32 = 1.0 / 60.0;

/// The seed of the staged race the `overtake` moment cuts from.
const OVERTAKE_STREAM: u64 = 600;

/// Seconds of play a moment's own framing ([`Race::moment_cam`]) holds after
/// the capture, so the page and showcase shots right after it keep it.
pub const MOMENT_CAM_HOLD_S: f32 = 1.5;

/// Ticks between the bang and the `item_hit` capture (a quarter second).
const ITEM_HIT_TICKS: u32 = 15;

/// Register every moment of the target pack (`kits/kart-racer/target/
/// moments.md`), in the order a race shows them.
pub fn register_capture_moments(m: &mut CaptureMoments) -> Result<(), String> {
    m.add("countdown", "The grid at \"1\": three red start lights, about half a second before GO", countdown)?;
    m.add("countdown_go", "GO: green lights and the player's good start", countdown_go)?;
    m.add("rivals", "The whole roster lined up on the road ahead, each kart in its own colour", rivals)?;
    m.add("split", "Two-player split screen: the player markers, minimaps and HUDs of both halves", split)?;
    m.add("drift", "A drift through a bend with blue sparks (charge tier 1)", drift)?;
    m.add("boost", "A drift boost: flame, speed lines and the FOV kick above top speed", boost)?;
    m.add("item_hit", "A Boing Bomb blast spins out the kart just ahead", item_hit)?;
    m.add("overtake", "The player passing a rival side by side", overtake)?;
    m.add("final_lap", "Just over the line into the final lap, under the gate", final_lap)?;
    m.add("finish", "Just over the finish line (staged: the autopilot drives a full race with its boost kept topped up, so the place and time are not a real player's run): FINISH!, the place in large type, confetti, lit gate lights and the camera easing into its orbit", finish)?;
    m.add("podium", "The podium scene: three blocks, the candy-cane arch and the top three drivers, the results beside it (the same staged race as the finish; rivals still racing show the time they are on pace for)", podium)?;
    m.add("threat", "A Seeker Star homing in on the player from behind: the incoming-item warning under the item slot", threat)?;
    Ok(())
}

/// The grid at "1" with three red lights, half a second before GO.
pub fn countdown(world: &World) -> Result<(), String> {
    stage_from(world, Phase::Countdown, None, 404, 5.0, |world, _| {
        let race = world.res::<Race>();
        race.phase == Phase::Countdown && race.countdown <= 0.5
    })
}

/// A quarter of a second after GO: green lights, the player off the line
/// with a start boost ("GOOD START!"; the autopilot does not launch itself,
/// so the moment gives the player the boost a good launch gives).
pub fn countdown_go(world: &World) -> Result<(), String> {
    stage_from(world, Phase::Countdown, None, 405, 5.0, |world, t| {
        let Some((e, _)) = player(world) else {
            return false;
        };
        let (phase, phase_time) = {
            let race = world.res::<Race>();
            (race.phase, race.phase_time)
        };
        if phase != Phase::Racing {
            return false;
        }
        if let Some(mut k) = world.get_mut::<Kart>(e)
            && k.launch != Launch::Primed
        {
            k.launch = Launch::Primed;
            k.boost = k.boost.max(t.start_boost_s);
        }
        phase_time >= 0.25
    })
}

/// Seed of the staged line-up and split screen.
const RIVALS_STREAM: u64 = 909;

/// Where the line-up puts the roster, as (metres ahead of the player, metres
/// off the road's centre line, right positive): the player at the back, two
/// rivals beside them, four in the next row and one out in front. The rows are
/// 6.5 m apart and the karts 3.6 m, so every kart shows its own colour from
/// the chase camera, none hidden behind another.
pub const LINE_UP: [(f32, f32); 8] = [(0.0, 0.0), (0.0, -3.6), (0.0, 3.6), (6.5, -5.4), (6.5, -1.8), (6.5, 1.8), (6.5, 5.4), (13.0, 0.0)];

/// How far along the road the line-up's back row stands (m): on the start
/// straight, clear of the gantry.
const LINE_UP_AT: f32 = 30.0;

/// All eight roster karts standing side by side in rows on the road ahead of
/// the chase camera, still: the rival colours (the player is the roster's
/// first, pink). A grid of more than eight waits behind the camera.
pub fn rivals(world: &World) -> Result<(), String> {
    stage(world, None, RIVALS_STREAM, 5.0, |world, _| {
        line_up(world);
        true
    })
}

/// Two players racing side by side, a few seconds in: the split screen with
/// a marker over each kart, each half's HUD and minimap.
pub fn split(world: &World) -> Result<(), String> {
    let mut ticks = 0u32;
    stage_with(world, (Phase::Racing, 2), None, RIVALS_STREAM + 1, 20.0, |_, _| {
        ticks += 1;
        ticks as f32 * DT >= 4.0
    })
}

/// Put the karts in [`LINE_UP`] order: the player first, then the rivals in
/// roster order; any beyond eight wait behind the camera.
fn line_up(world: &World) {
    let race = world.res::<Race>();
    let track = &race.track;
    let base = LINE_UP_AT;
    let me = race.player_kart(0).unwrap_or(0);
    let mut rival = 0usize;
    for (i, &e) in race.grid.iter().enumerate() {
        let (spec, ahead, lat) = if i == me {
            (0, LINE_UP[0].0, LINE_UP[0].1)
        } else {
            rival += 1;
            match LINE_UP.get(rival) {
                Some(&(a, l)) => (rival % race.roster.len().max(1), a, l),
                None => (rival % race.roster.len().max(1), -26.0 - 6.0 * (rival - LINE_UP.len()) as f32, if rival.is_multiple_of(2) { 3.6 } else { -3.6 }),
            }
        };
        let s = base + ahead;
        let tan = track.tangent_at(s);
        let player = (i == me).then_some(0);
        let mut k = Kart::new(spec, player, track.point_at(s, lat), tan[0].atan2(tan[1]));
        k.hint = track.index_at(s).0;
        k.relocate(track);
        if let Some(mut m) = world.get_mut::<Kart>(e) {
            *m = k;
        }
    }
    drop(race);
    race::refresh_places(world);
}

/// The player boosting out of a drift well above top speed: the boost is
/// topped up once the player is up to speed, then held for half a second so
/// the FOV kick and the speed lines build.
pub fn boost(world: &World) -> Result<(), String> {
    let (mut ticks, mut boosting) = (0u32, 0u32);
    stage(world, None, 505, 60.0, |world, t| {
        ticks += 1;
        let Some((e, _)) = player(world) else {
            return false;
        };
        let Some(mut k) = world.get_mut::<Kart>(e) else {
            return false;
        };
        if boosting == 0 && ((ticks as f32) * DT < 4.0 || !k.on_road || k.speed < 0.85 * t.max_speed || k.spin > 0.0) {
            return false;
        }
        k.boost = k.boost.max(t.boost_tier2_s);
        boosting += 1;
        boosting >= 30 && k.speed > t.max_speed
    })
}

/// The player drawing level with a rival and passing it: the tick its place
/// improves with that rival within 4 m.
pub fn overtake(world: &World) -> Result<(), String> {
    let mut last: Option<usize> = None;
    let mut ticks = 0u32;
    stage(world, None, OVERTAKE_STREAM, 90.0, |world, _| {
        ticks += 1;
        let Some((_, i)) = player(world) else {
            return false;
        };
        let karts = race::kart_list(world, &world.res::<Race>().grid);
        let me = &karts[i];
        let before = last.replace(me.place);
        // After the start's shuffle, a clean pass of a kart that is not
        // spinning, side by side.
        let Some(was) = before else {
            return false;
        };
        let passed = (ticks as f32) * DT > 3.0
            && me.place < was
            && karts.iter().enumerate().any(|(j, k)| {
                j != i && k.place == me.place + 1 && k.spin <= 0.0 && {
                    let d = [k.pos[0] - me.pos[0], k.pos[2] - me.pos[2]];
                    (d[0] * d[0] + d[1] * d[1]).sqrt() < 4.0
                }
            });
        if passed && let Some(mut hud) = world.try_res_mut::<crate::hud::RaceHud>() {
            // The HUD last saw the place before the pass, so its next update
            // shows the "+1" pop on the position number.
            hud.players_hud[0].active = true;
            hud.players_hud[0].place = was + 1;
        }
        passed
    })
}

/// Just over the line into the last lap, under the gate with the stands
/// behind: "FINAL LAP" and the lap counter in yellow.
/// The camera sits well back and low, looking a little up, so the whole gate
/// (its posts, banner and the start-light housing) is in frame with the
/// player on the line under it; the first race (from stream 707 on) with no
/// rival between the camera and the player is the one shown.
pub fn final_lap(world: &World) -> Result<(), String> {
    for stream in FINAL_LAP_STREAM..FINAL_LAP_STREAM + 24 {
        stage(world, None, stream, 400.0, |world, _| {
            let Some((e, _)) = player(world) else {
                return false;
            };
            let Some(k) = world.get::<Kart>(e) else {
                return false;
            };
            let race = world.res::<Race>();
            let line = race.track.length * (race.counts.laps.max(2) - 1) as f32;
            race.progress(&k) >= line + 0.5
        })?;
        if rival_behind(world, FINAL_LAP_CAM.distance + 2.0).is_none() {
            let mut race = world.res_mut::<Race>();
            race.moment_cam = Some(FINAL_LAP_CAM);
            race.moment_cam_left = MOMENT_CAM_HOLD_S;
            return Ok(());
        }
    }
    Err("every staged final lap had a rival right behind the player".into())
}

/// First race stream the final-lap moment tries.
const FINAL_LAP_STREAM: u64 = 707;

/// The final-lap shot's framing ([`final_lap`]).
pub const FINAL_LAP_CAM: race::MomentCam = race::MomentCam {
    distance: 18.0,
    height: 2.6,
    look_ahead: 8.0,
    look_height: 2.8,
};

/// The nearest rival (track metres behind) that is at most `within` metres
/// behind the player along the track, if any.
fn rival_behind(world: &World, within: f32) -> Option<f32> {
    let (e, _) = player(world)?;
    let race = world.res::<Race>();
    let me = race.progress(&*world.get::<Kart>(e)?);
    race.grid
        .iter()
        .filter(|&&r| r != e)
        .filter_map(|&r| world.get::<Kart>(r).map(|k| me - race.progress(&k)))
        .filter(|&d| (0.0..=within).contains(&d))
        .min_by(f32::total_cmp)
}

/// The race both end-of-race moments are cut from (one staged race, so the
/// finish shot and the podium agree on the player's place). The autopilot
/// alone rarely makes the podium (one seed in sixty), so the player's boost is
/// kept topped up on the road, the same every time. `done` is asked once the
/// player has crossed the line, with the seconds since they did.
fn staged_finish(world: &World, mut done: impl FnMut(&World, f32) -> bool) -> Result<(), String> {
    stage(world, None, FINISH_STREAM, 600.0, |world, _| {
        if let Some((e, _)) = player(world)
            && let Some(mut k) = world.get_mut::<Kart>(e)
            && k.finished.is_none()
            && k.on_road
            && k.spin <= 0.0
        {
            k.boost = k.boost.max(0.25);
        }
        let Some((e, _)) = player(world) else {
            return false;
        };
        let Some(fin) = world.get::<Kart>(e).and_then(|k| k.finished) else {
            return false;
        };
        done(world, world.res::<Race>().clock - fin)
    })
}

/// Seed of the staged finish (the race's RNG stream).
pub const FINISH_STREAM: u64 = 808;

/// Seconds after the line the finish moment holds: the camera is well on its
/// way from the chase view into the orbit, the kart is still rolling past the
/// gate, and the confetti is in the air.
pub const FINISH_HOLD_S: f32 = 3.4;

/// The podium scene with the player in the top three: the results panel
/// shows beside it.
pub fn podium(world: &World) -> Result<(), String> {
    staged_finish(world, |world, _| {
        let race = world.res::<Race>();
        race.phase == Phase::Finished && race.phase_time >= crate::finale::CELEBRATE_S + 0.3
    })?;
    let (_, i) = player(world).ok_or("no player")?;
    let place = race::kart_list(world, &world.res::<Race>().grid)[i].place;
    if place < 3 {
        Ok(())
    } else {
        Err(format!("the player finished {}th, off the podium", place + 1))
    }
}

/// The player [`FINISH_HOLD_S`] after crossing the line on the last lap of
/// the same full race as [`podium`] (the usual lap count): the "FINISH!"
/// banner, the final place, confetti, the lit gate and the orbit.
pub fn finish(world: &World) -> Result<(), String> {
    staged_finish(world, |_, since| since >= FINISH_HOLD_S)?;
    // The HUD follows the staged race from its first frame.
    if let Some(mut hud) = world.try_res_mut::<crate::hud::RaceHud>() {
        let h = &mut hud.players_hud[0];
        h.active = true;
        h.lap = world.res::<Race>().counts.laps;
    }
    Ok(())
}

/// Start a one-player race on the autopilot (`laps` overrides the lap
/// count), reseed the RNG with `stream`, then step until `done` returns true,
/// for at most `max_s` seconds of race time.
fn stage(
    world: &World,
    laps: Option<u32>,
    stream: u64,
    max_s: f32,
    done: impl FnMut(&World, &Tune) -> bool,
) -> Result<(), String> {
    stage_from(world, Phase::Racing, laps, stream, max_s, done)
}

/// [`stage`] from `from`: `Phase::Countdown` stages the grid before GO.
fn stage_from(world: &World, from: Phase, laps: Option<u32>, stream: u64, max_s: f32, done: impl FnMut(&World, &Tune) -> bool) -> Result<(), String> {
    stage_with(world, (from, 1), laps, stream, max_s, done)
}

/// [`stage_from`] with `players` local players (`from`, `players`).
fn stage_with(
    world: &World,
    (from, players): (Phase, usize),
    laps: Option<u32>,
    stream: u64,
    max_s: f32,
    mut done: impl FnMut(&World, &Tune) -> bool,
) -> Result<(), String> {
    let kit = *world.try_res::<KartKit>().ok_or("the kart racer kit is not loaded")?;
    let (t, mut counts, gravity) = {
        let reg = world.res::<ParamRegistry>();
        let t = kit.tune.read(&reg);
        (t, kit.counts(&reg), kit.gravity(&reg, &t))
    };
    if let Some(l) = laps {
        counts.laps = l;
    }
    let formulas = world.res::<Formulas>();
    let steer = |x: f32| formulas.eval(kit.steer_by_speed, &[x]);
    let rubber = |g: f32, d: f32, s: f32| formulas.eval(kit.rubber_band, &[g, d, s]);
    let fns = Fns {
        steer: &steer,
        rubber: &rubber,
    };
    race::reset(world, from, players, &t, counts)?;
    // The HUD's last place belongs to whatever was staged before: start its
    // place tracking afresh so no "+1" pop is left over from another moment.
    if let Some(mut hud) = world.try_res_mut::<crate::hud::RaceHud>() {
        for h in &mut hud.players_hud {
            h.place = 0;
            h.gain_left = 0.0;
        }
    }
    {
        let mut race = world.res_mut::<Race>();
        race.rng = Pcg32::new(RACE_SEED, stream);
        race.autopilot = true;
        race.requests.clear();
    }
    let c = Ctx {
        t,
        counts,
        gravity,
        dt: DT,
        inputs: [Controls::default(); 2],
    };
    let steps = (max_s / DT).ceil() as u32;
    for _ in 0..steps {
        race::tick_shared(world, &c, &fns)?;
        if done(world, &t) {
            // Render exactly this tick (no interpolation from the last one)
            // and play no burst of queued sounds.
            let mut race = world.res_mut::<Race>();
            for &e in &race.grid {
                if let Some(mut k) = world.get_mut::<Kart>(e) {
                    k.prev_pos = k.pos;
                    k.prev_yaw = k.yaw;
                }
            }
            race.events.clear();
            race.cam_snap = true;
            // Frame it at a fixed time, with no camera shake left from the
            // last bump: a capture of this moment is the same every time.
            race.moment_time = Some(race.clock);
            race.trauma = [0.0; 2];
            return Ok(());
        }
    }
    Err(format!("the moment did not happen within {max_s} s of racing"))
}

/// The player's kart entity and grid index.
fn player(world: &World) -> Option<(Entity, usize)> {
    let race = world.res::<Race>();
    let i = race.player_kart(0)?;
    Some((race.grid[i], i))
}

/// The player sliding through a corner with blue sparks (charge tier 1, as
/// the target pack asks for: the indicator on "1"), a third of a second
/// after the sparks turned blue and well before the orange tier, so the
/// slide and the trail show. The autopilot lets go of a drift as soon as the
/// corner opens out (which is also when its charge first reaches tier 1), so
/// once the drift is set in a bend that runs on, the charge is topped up to
/// the start of tier 1, and the shot waits for a bend that still has a
/// way to go, so a page that settles a few ticks late still finds the slide.
pub fn drift(world: &World) -> Result<(), String> {
    let (mut drifting, mut blue) = (0u32, 0u32);
    stage(world, None, 101, 90.0, |world, t| {
        let Some((e, _)) = player(world) else {
            return false;
        };
        let Some(mut k) = world.get_mut::<Kart>(e) else {
            return false;
        };
        let curve = world.res::<Race>().track.curvature_ahead(k.s + 4.0, 25.0);
        let mid_corner = curve.signum() == k.drift && curve.abs() > t.ai_drift_curvature;
        // Not while a boost from the last drift is still burning (its flames
        // and orange trail would hide the blue sparks), and at a fair speed.
        if k.drift == 0.0 || k.boost > 0.0 || k.speed < 0.6 * t.max_speed {
            drifting = 0;
            blue = 0;
            return false;
        }
        drifting += 1;
        if drifting == 12 && mid_corner && k.tier(t) < 1 {
            k.charge = k.charge.max(t.drift_tier1 + 0.03);
        }
        blue = if k.tier(t) == 1 && mid_corner { blue + 1 } else { 0 };
        blue >= 20
    })
}

/// A Seeker Star closing on the player from behind, about 18 m away: the
/// "SEEKER INCOMING!" warning is up at full strength (urgency about 0.8, its
/// fastest blink). Not a target pack signature moment; the review shoots it
/// on every showcase tier and as a page shot (`[review] showcase_extra`).
pub fn threat(world: &World) -> Result<(), String> {
    let mut fired = false;
    let mut ticks = 0u32;
    stage(world, None, 303, 90.0, |world, _| {
        ticks += 1;
        let Some((_, me)) = player(world) else {
            return false;
        };
        // Let the pack spread out first.
        if !fired {
            if (ticks as f32) * DT >= 4.0 {
                let rival = if me == 0 { 1 } else { 0 };
                race::launch_seeker(world, rival, me, 70.0);
                fired = true;
            }
            return false;
        }
        let view = RaceView::new(world);
        view.seekers.iter().any(|s| {
            let k = &view.karts[me];
            ((s.pos[0] - k.pos[0]).powi(2) + (s.pos[2] - k.pos[2]).powi(2)).sqrt() < 18.0
        })
    })
}

/// A blast spinning out a kart 6.5 to 16 m ahead of the player (to one side of Pip, not behind his head), a quarter of
/// a second after the bang: the ring is out, the debris is in the air and
/// the rival is a good way round its first turn.
pub fn item_hit(world: &World) -> Result<(), String> {
    let mut since: Option<u32> = None;
    let mut ticks = 0u32;
    stage(world, None, 202, 90.0, |world, t| {
        ticks += 1;
        let Some((_, i)) = player(world) else {
            return false;
        };
        if let Some(n) = since.as_mut() {
            *n += 1;
            return *n >= ITEM_HIT_TICKS;
        }
        // Let the pack spread out first.
        if (ticks as f32) * DT < 4.0 {
            return false;
        }
        let karts = race::kart_list(world, &world.res::<Race>().grid);
        let (mp, f) = (karts[i].pos, karts[i].forward());
        let target = karts.iter().enumerate().find_map(|(j, k)| {
            let d = [k.pos[0] - mp[0], k.pos[2] - mp[2]];
            let ahead = d[0] * f[0] + d[1] * f[1];
            let side = (d[0] * f[1] - d[1] * f[0]).abs();
            (j != i && k.spin <= 0.0 && k.shield <= 0.0 && (6.5..16.0).contains(&ahead) && (1.2..4.2).contains(&side)).then_some(k.pos)
        });
        if let Some(p) = target {
            race::explode(world, [p[0], p[1] + 0.5, p[2]], t);
            since = Some(0);
        }
        false
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::KartRacerPlugin;
    use crate::race::RaceView;
    use evani_core::App;

    fn app() -> App {
        let mut a = App::new("t").with_kit(KartRacerPlugin);
        a.run_headless(2, 1000.0 / 60.0);
        a
    }

    fn snapshot(a: &App) -> Vec<[f32; 3]> {
        RaceView::new(&a.world).karts.iter().map(|k| k.pos).collect()
    }

    /// Slipstream in a real race: two players on the autopilot and six AI
    /// karts. Every kart that drafts gets exactly x1.05 on its speed cap and
    /// nothing else, players and AI alike, and each draft ends again.
    #[test]
    fn every_kind_of_kart_drafts_for_exactly_five_percent_and_loses_it() {
        use crate::slipstream::SLIPSTREAM_MULTIPLIER;
        let a = app();
        let mut steps = 0u32;
        let mut prev: Vec<bool> = Vec::new();
        // Drafts started, by AI (0) and player (1) karts.
        let mut started = [0u32; 2];
        let mut ended: std::collections::BTreeMap<&'static str, u32> = std::collections::BTreeMap::new();
        let mut worst = 0.0f32;
        // A player kart settled at its drafting cap (no drift boost, speed
        // within 1 cm/s of the cap): speed, cap, plain cap.
        let mut player_peak = (0.0f32, 0.0f32, 0.0f32);
        let mut first_ai: Option<String> = None;
        stage_with(&a.world, (Phase::Racing, 2), Some(3), 7, 200.0, |w, _| {
            steps += 1;
            let race = RaceView::new(w);
            prev.resize(race.karts.len(), false);
            for (i, k) in race.karts.iter().enumerate() {
                if k.slipstream {
                    worst = worst.max((k.cap / k.cap_base - SLIPSTREAM_MULTIPLIER).abs());
                    if k.player.is_some() && k.boost == 0.0 && (k.speed - k.cap).abs() < 0.01 && k.speed > player_peak.0 {
                        player_peak = (k.speed, k.cap, k.cap_base);
                    }
                    if !prev[i] {
                        started[usize::from(k.player.is_some())] += 1;
                        if k.player.is_none() && first_ai.is_none() {
                            first_ai = Some(format!("AI kart {i}: speed {:.2} m/s, cap {:.2} -> {:.2} m/s", k.speed, k.cap_base, k.cap));
                        }
                    }
                } else {
                    assert!((k.cap - k.cap_base).abs() < 1e-6, "no draft, no extra cap ({} vs {})", k.cap, k.cap_base);
                    if prev[i] {
                        *ended.entry(k.draft.label()).or_default() += 1;
                    }
                }
                prev[i] = k.slipstream;
            }
            steps >= (90.0 / DT) as u32
        })
        .unwrap();
        println!("slipstream over 90 s of racing: drafts started by AI {} and by players {}; ended by {ended:?}", started[0], started[1]);
        println!("first AI draft: {}", first_ai.clone().unwrap_or_default());
        println!(
            "player settled while drafting (no drift boost): speed {:.2} m/s, cap {:.2} m/s = plain cap {:.2} m/s x {:.4}",
            player_peak.0,
            player_peak.1,
            player_peak.2,
            player_peak.1 / player_peak.2.max(1e-6)
        );
        assert!(worst < 1e-5, "cap ratio off by {worst}");
        assert!(started[0] > 0, "AI karts draft");
        assert!(started[1] > 0, "player karts draft");
        assert!(player_peak.0 > player_peak.2 + 1.0, "a drafting player goes faster than the plain cap");
        assert!((player_peak.0 / player_peak.2 - SLIPSTREAM_MULTIPLIER).abs() < 0.001, "by 5 %: {player_peak:?}");
        assert!(ended.values().sum::<u32>() > 0, "drafts end again");
    }

    #[test]
    fn moments_are_registered_in_review_order() {
        let mut m = CaptureMoments::default();
        register_capture_moments(&mut m).unwrap();
        assert_eq!(
            m.names(),
            ["countdown", "countdown_go", "rivals", "split", "drift", "boost", "item_hit", "overtake", "final_lap", "finish", "podium", "threat"]
        );
    }

    #[test]
    fn countdown_shows_three_red_lights_then_go_shows_a_good_start() {
        let a = app();
        countdown(&a.world).unwrap();
        {
            let race = RaceView::new(&a.world);
            assert_eq!(race.phase, Phase::Countdown);
            assert_eq!(race.countdown.ceil(), 1.0, "\"1\" is up");
            assert!(race.karts.iter().all(|k| k.speed == 0.0), "the grid waits");
        }
        countdown_go(&a.world).unwrap();
        let t = a.world.res::<KartKit>().tune.read(&a.world.res::<ParamRegistry>());
        let race = RaceView::new(&a.world);
        assert_eq!(race.phase, Phase::Racing);
        assert!(race.phase_time < 0.5, "green lights are still on");
        let k = &race.karts[race.player_kart(0).unwrap()];
        assert_eq!(k.launch, Launch::Primed);
        assert!(k.boost > 0.0 && k.boost <= t.start_boost_s);
    }

    #[test]
    fn rivals_lines_up_the_whole_roster_each_in_its_own_colour_and_clear_of_the_others() {
        let a = app();
        rivals(&a.world).unwrap();
        let race = RaceView::new(&a.world);
        assert_eq!(race.karts.len(), race.roster.len(), "the field is the roster");
        let mut specs: Vec<usize> = race.karts.iter().map(|k| k.spec).collect();
        specs.sort_unstable();
        specs.dedup();
        assert_eq!(specs.len(), 8, "eight different karts");
        let colour = |k: &Kart| race.roster[k.spec].color;
        for (i, a) in race.karts.iter().enumerate() {
            assert_eq!(a.speed, 0.0, "they stand still");
            for b in &race.karts[i + 1..] {
                assert_ne!(colour(a), colour(b));
                let d = ((a.pos[0] - b.pos[0]).powi(2) + (a.pos[2] - b.pos[2]).powi(2)).sqrt();
                assert!(d > 3.0, "karts {} and {} are {d} m apart", a.spec, b.spec);
            }
        }
        assert_eq!(race.karts[race.player_kart(0).unwrap()].spec, 0, "the player is the first of the roster");
        assert!(race.karts.iter().all(|k| k.on_road), "all on the road");
    }

    #[test]
    fn split_races_two_players() {
        let a = app();
        split(&a.world).unwrap();
        let race = RaceView::new(&a.world);
        assert_eq!(race.players, 2);
        assert_eq!(race.phase, Phase::Racing);
        assert!(race.karts.iter().filter(|k| k.player.is_some()).count() == 2);
    }

    #[test]
    fn boost_is_above_top_speed_every_time() {
        let a = app();
        boost(&a.world).unwrap();
        let first = snapshot(&a);
        {
            let t = a.world.res::<KartKit>().tune.read(&a.world.res::<ParamRegistry>());
            let race = RaceView::new(&a.world);
            let k = &race.karts[race.player_kart(0).unwrap()];
            assert!(k.boost > 0.0 && k.speed > t.max_speed, "{} m/s", k.speed);
        }
        boost(&a.world).unwrap();
        assert_eq!(first, snapshot(&a), "deterministic");
    }

    #[test]
    fn overtake_is_deterministic() {
        let a = app();
        let state = |a: &App| -> Vec<[f32; 6]> {
            RaceView::new(&a.world).karts.iter().map(|k| [k.pos[0], k.pos[2], k.yaw, k.slip, k.speed, k.spin_angle]).collect()
        };
        overtake(&a.world).unwrap();
        let first = state(&a);
        overtake(&a.world).unwrap();
        assert_eq!(first, state(&a), "the overtake is the same every time");
        // And from a fresh game.
        let b = app();
        overtake(&b.world).unwrap();
        assert_eq!(first, state(&b), "the overtake is the same in a fresh game");
    }

    #[test]
    fn a_staged_moment_is_drawn_at_a_fixed_time_with_no_shake_until_the_next_tick() {
        let a = app();
        overtake(&a.world).unwrap();
        {
            let race = a.world.res::<Race>();
            assert_eq!(race.moment_time, Some(race.clock), "framed at the staged race's clock");
            assert_eq!(race.trauma, [0.0; 2], "no shake left from the last bump");
        }
        let mut a = a;
        a.run_headless(3, 1000.0 / 60.0);
        assert_eq!(a.world.res::<Race>().moment_time, None, "the game running again draws at its own time");
    }

    #[test]
    fn overtake_is_a_close_pass() {
        let a = app();
        overtake(&a.world).unwrap();
        let race = RaceView::new(&a.world);
        let me = &race.karts[race.player_kart(0).unwrap()];
        let passed = race.karts.iter().find(|k| k.place == me.place + 1).unwrap();
        let d = ((passed.pos[0] - me.pos[0]).powi(2) + (passed.pos[2] - me.pos[2]).powi(2)).sqrt();
        assert!(d < 4.0, "{d} m");
    }

    #[test]
    fn final_lap_is_just_over_the_line() {
        let a = app();
        final_lap(&a.world).unwrap();
        let race = RaceView::new(&a.world);
        let k = &race.karts[race.player_kart(0).unwrap()];
        assert_eq!(race.lap_of(k), race.counts.laps);
        let past = race.progress(k) - race.track.length * (race.counts.laps - 1) as f32;
        assert!((0.5..2.0).contains(&past), "{past} m past the line");
    }

    #[test]
    fn final_lap_frames_the_whole_gate_with_no_rival_at_the_camera() {
        let a = app();
        final_lap(&a.world).unwrap();
        assert_eq!(rival_behind(&a.world, FINAL_LAP_CAM.distance + 2.0), None, "no rival between the camera and the player");
        let race = RaceView::new(&a.world);
        assert_eq!(race.moment_cam, Some(FINAL_LAP_CAM));
        let k = &race.karts[race.player_kart(0).unwrap()];
        let c = FINAL_LAP_CAM;
        let f = [k.yaw.sin(), 0.0, k.yaw.cos()];
        let eye = [k.pos[0] - f[0] * c.distance, k.pos[1] + c.height, k.pos[2] - f[2] * c.distance];
        let at = [k.pos[0] + f[0] * c.look_ahead, k.pos[1] + c.look_height, k.pos[2] + f[2] * c.look_ahead];
        // A pinhole at the chase camera's vertical FOV on a 16:9 page.
        let fov = 62.0f32.to_radians();
        let (ty, tx) = ((fov * 0.5).tan(), (fov * 0.5).tan() * 16.0 / 9.0);
        let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
        let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let norm = |a: [f32; 3]| {
            let l = dot(a, a).sqrt();
            [a[0] / l, a[1] / l, a[2] / l]
        };
        let fwd = norm(sub(at, eye));
        let right = norm([fwd[2], 0.0, -fwd[0]]);
        let up = [fwd[1] * right[2] - fwd[2] * right[1], fwd[2] * right[0] - fwd[0] * right[2], fwd[0] * right[1] - fwd[1] * right[0]];
        let screen = |p: [f32; 3]| {
            let d = sub(p, eye);
            let z = dot(d, fwd);
            (dot(d, right) / z / tx, dot(d, up) / z / ty)
        };
        let p0 = race.track.point_at(0.0, 0.0);
        let w = race.track.half_width;
        let tune = a.world.res::<KartKit>().tune.read(&a.world.res::<ParamRegistry>());
        let post = w + tune.verge_width + crate::draw::GANTRY_POST_OUT;
        // The banner's top edge and the light housing's top, then both posts'
        // feet: all inside the frame, with a margin.
        for (what, p) in [
            ("banner top", [p0[0], p0[1] + 7.45, p0[2]]),
            ("light housing top", [p0[0], p0[1] + 6.05, p0[2]]),
            ("left post foot", race.track.point_at(0.0, -post)),
            ("right post foot", race.track.point_at(0.0, post)),
        ] {
            let (x, y) = screen(p);
            assert!(x.abs() < 0.97 && y.abs() < 0.95, "{what} at ({x:.2}, {y:.2}) on screen");
        }
        let (_, y) = screen(k.pos);
        assert!(y < 0.0, "the player sits below the middle, on the line under the gate ({y:.2})");
    }

    #[test]
    fn the_final_lap_framing_holds_for_the_page_shots_then_hands_back() {
        // The showcase and page shots land a few frames to most of a second
        // after the capture: they must still frame the gate.
        let mut a = app();
        final_lap(&a.world).unwrap();
        a.run_headless(45, 1000.0 / 60.0);
        assert_eq!(a.world.res::<Race>().moment_cam, Some(FINAL_LAP_CAM), "held three quarters of a second on");
        a.run_headless(60, 1000.0 / 60.0);
        assert_eq!(a.world.res::<Race>().moment_cam, None, "the chase camera is back after the hold");
    }

    #[test]
    fn podium_is_the_celebration_over_with_the_player_in_the_top_three() {
        let a = app();
        podium(&a.world).unwrap();
        let race = RaceView::new(&a.world);
        assert_eq!(race.phase, Phase::Finished);
        assert_eq!(crate::finale::stage(race.phase, race.phase_time, race.players), crate::finale::Stage::Podium);
        assert!(race.karts[race.player_kart(0).unwrap()].place < 3);
        // The places are latched, so the podium and the panel agree.
        assert_eq!(race.final_order.as_deref(), Some(&race.order[..]));
        // Every row has a time (rivals still racing get the time they are on
        // pace for), rising down the table.
        let times: Vec<f32> = race.results().iter().map(|r| r.time.expect("a row without a time")).collect();
        assert_eq!(times.len(), race.karts.len());
        assert!(times.windows(2).all(|w| w[0] <= w[1]), "{times:?}");
    }

    #[test]
    fn drift_shows_blue_sparks_every_time() {
        let a = app();
        drift(&a.world).unwrap();
        let first = snapshot(&a);
        {
            let t = a.world.res::<KartKit>().tune.read(&a.world.res::<ParamRegistry>());
            let race = RaceView::new(&a.world);
            let k = &race.karts[race.player_kart(0).unwrap()];
            assert!(k.drift != 0.0 && k.tier(&t) == 1, "blue sparks, the indicator on 1");
            assert!(race.autopilot && race.phase == Phase::Racing);
        }
        drift(&a.world).unwrap();
        assert_eq!(first, snapshot(&a), "deterministic");
    }

    #[test]
    fn a_page_shot_a_few_ticks_late_still_finds_the_blue_drift() {
        // The review's page shots land a few frames after the capture (the
        // phone one stages the moment again at the phone size first): the
        // slide, the blue sparks and the indicator on "1" must still be up.
        let mut a = app();
        drift(&a.world).unwrap();
        // A fifth of a second on (the slide lasts a third; the old phone
        // shot, taken most of a second on, caught the boost after it).
        a.run_headless(12, 1000.0 / 60.0);
        let t = a.world.res::<KartKit>().tune.read(&a.world.res::<ParamRegistry>());
        let race = RaceView::new(&a.world);
        let k = &race.karts[race.player_kart(0).unwrap()];
        assert!(k.drift != 0.0 && k.tier(&t) == 1 && k.boost <= 0.0, "drift {} tier {} boost {}", k.drift, k.tier(&t), k.boost);
    }

    #[test]
    fn item_hit_spins_out_a_kart_ahead() {
        let a = app();
        item_hit(&a.world).unwrap();
        let race = RaceView::new(&a.world);
        assert_eq!(race.blasts.len(), 1);
        let me = race.player_kart(0).unwrap();
        assert!(race.karts.iter().enumerate().any(|(j, k)| j != me && k.spin > 0.0));
        assert!(race.karts[me].spin <= 0.0, "the player watches, not spins");
    }

    #[test]
    fn threat_has_a_seeker_homing_in_on_the_player_and_the_warning_up() {
        let a = app();
        threat(&a.world).unwrap();
        let t = a.world.res::<KartKit>().tune.read(&a.world.res::<ParamRegistry>());
        let race = RaceView::new(&a.world);
        let me = race.player_kart(0).unwrap();
        let found = crate::threat::assess(me, &race.karts[me], &race.seekers, &race.bombs, t.bomb_radius).unwrap();
        assert_eq!(found.kind, crate::threat::ThreatKind::Seeker);
        assert!(found.level > 0.75, "full strength: {}", found.level);
    }

    #[test]
    fn finish_is_just_after_the_line_on_the_last_lap_of_a_full_race() {
        let a = app();
        finish(&a.world).unwrap();
        let race = RaceView::new(&a.world);
        let k = &race.karts[race.player_kart(0).unwrap()];
        // The full three laps, finished: the HUD reads "Lap 3/3".
        assert_eq!(race.counts.laps, 3);
        assert_eq!(race.lap_of(k), 3);
        assert_eq!(k.laps_done, 3);
        let since = race.clock - k.finished.expect("over the line");
        assert!((FINISH_HOLD_S..FINISH_HOLD_S + 0.1).contains(&since), "{since} s after the line");
        // The celebration, not the podium.
        assert_eq!(crate::finale::stage(race.phase, race.phase_time, race.players), crate::finale::Stage::Celebrate);
        // The kart rolls on past the gate and slows; it does not reverse.
        let goal = race.track.length * 3.0;
        assert!(race.progress(k) > goal && race.progress(k) < goal + 40.0, "{} m past", race.progress(k) - goal);
        assert!(k.speed >= 0.0);
    }

    #[test]
    fn the_finish_and_the_podium_are_one_race_with_one_place_for_the_player() {
        let a = app();
        finish(&a.world).unwrap();
        let (place_at_finish, time_at_finish) = {
            let race = RaceView::new(&a.world);
            let k = &race.karts[race.player_kart(0).unwrap()];
            (k.place, k.finished.unwrap())
        };
        podium(&a.world).unwrap();
        let race = RaceView::new(&a.world);
        let k = &race.karts[race.player_kart(0).unwrap()];
        assert_eq!(k.place, place_at_finish, "the card must not say 3rd over a 7th place shot");
        assert_eq!(k.finished.unwrap(), time_at_finish);
        // The podium shows the top three of the results panel, in order.
        let rows = race.results();
        let top: Vec<_> = rows.iter().take(3).map(|r| r.kart).collect();
        let on_podium: Vec<_> = crate::finale::podium_order(&race.order).into_iter().flatten().collect();
        assert_eq!(top, on_podium);
        // And the player is among them.
        assert!(rows.iter().take(3).any(|r| r.player == Some(0)));
    }

    #[test]
    fn the_overtake_moment_shows_a_plus_one_pop_on_the_position() {
        let a = app();
        overtake(&a.world).unwrap();
        let race = RaceView::new(&a.world);
        let mut hud = a.world.res_mut::<crate::hud::RaceHud>();
        hud.update(&race, &Tune::default(), 0.016, 5.0);
        let h = &hud.players_hud[0];
        assert_eq!(h.gain_by, 1);
        assert!(h.gain_left > 0.0, "the pop is showing");
        // A held capture clock keeps it showing.
        hud.update(&race, &Tune::default(), 0.016, 5.0);
        assert!(hud.players_hud[0].gain_left > 0.0);
    }

    #[test]
    fn other_moments_leave_no_pop_behind() {
        let a = app();
        overtake(&a.world).unwrap();
        {
            let race = RaceView::new(&a.world);
            a.world.res_mut::<crate::hud::RaceHud>().update(&race, &Tune::default(), 0.016, 5.0);
        }
        final_lap(&a.world).unwrap();
        let race = RaceView::new(&a.world);
        let mut hud = a.world.res_mut::<crate::hud::RaceHud>();
        hud.update(&race, &Tune::default(), 0.016, 5.0);
        assert_eq!(hud.players_hud[0].gain_left, 0.0);
    }

    #[test]
    fn the_finish_hud_says_lap_3_of_3() {
        let a = app();
        finish(&a.world).unwrap();
        let race = RaceView::new(&a.world);
        let mut hud = a.world.res_mut::<crate::hud::RaceHud>();
        hud.update(&race, &Tune::default(), 0.016, 0.0);
        let h = &hud.players_hud[0];
        assert_eq!((h.lap, h.laps), (3, 3));
        assert!(h.final_lap);
        assert!(h.finished.is_some(), "over the line: the HUD shows FINISH!, not FINAL LAP!");
    }

    #[test]
    fn a_new_race_takes_the_wheel_back() {
        let a = app();
        drift(&a.world).unwrap();
        let t = Tune::default();
        let counts = a.world.res::<Race>().counts;
        race::reset(&a.world, Phase::Countdown, 1, &t, counts).unwrap();
        assert!(!a.world.res::<Race>().autopilot);
    }

    #[test]
    fn a_moment_cannot_change_the_field_size() {
        // Through &World the grid cannot grow or shrink: a clear error, and
        // the moments still work afterwards.
        let a = app();
        let n = a.world.res::<Race>().grid.len();
        let t = Tune::default();
        let counts = race::Counts {
            karts: n - 1,
            ..Default::default()
        };
        assert!(race::reset(&a.world, Phase::Racing, 1, &t, counts).is_err());
        drift(&a.world).unwrap();
    }
}
