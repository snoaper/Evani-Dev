//! The kart racer's looping sounds: one engine per local player whose pitch
//! follows speed, a surface bed under each player (road, kerb or grass, each
//! its own loop), and a quieter engine for the nearest rival. They run on the
//! mixer's looping voices ([`evani_core::audio::AudioMixer::voice_start`]),
//! are set every frame from the race, and are stopped whenever the race is
//! not running (title, pause, results), so a paused or finished game is
//! silent. The page's mute and volume settings reach them through the
//! effects bus like every other sound, and the mixer holds them back until
//! the first user gesture has unlocked audio.

use evani_core::audio::{AudioMixer, Bus, VoiceId};

use crate::kart::Kart;
use crate::race::{Phase, RaceView};
use crate::sfx::{Loop, Sounds};
use crate::trackmesh::KERB_W;
use crate::Tune;

/// Playback rate of an idling engine (0.75 x 80 Hz = 60 Hz).
pub const IDLE_RATE: f32 = 0.75;
/// Playback rate at top speed on the road (3.0 x 80 Hz = 240 Hz).
pub const TOP_RATE: f32 = 3.0;
/// More rate per unit of speed fraction above top speed (boosting).
const BOOST_RATE: f32 = 1.2;

/// The engine's playback rate (pitch) for a speed fraction (speed over the
/// kart's top speed; above 1 while boosting). Rises with speed, falls when it
/// does, and reversing sounds like going forward.
pub fn engine_rate(speed_frac: f32) -> f32 {
    let f = if speed_frac.is_finite() { speed_frac.abs() } else { 0.0 };
    IDLE_RATE + (TOP_RATE - IDLE_RATE) * f.min(1.0).powf(0.85) + (f - 1.0).clamp(0.0, 1.0) * BOOST_RATE
}

/// The engine's loudness for a speed fraction. `players` is how many share
/// the screen: two engines are each a little quieter so together they are no
/// louder than one close to the speaker.
pub fn engine_gain(speed_frac: f32, players: usize) -> f32 {
    let f = if speed_frac.is_finite() { speed_frac.abs().min(1.3) } else { 0.0 };
    let share = if players > 1 { 0.7 } else { 1.0 };
    (0.14 + 0.26 * f.min(1.0) + 0.05 * (f - 1.0).max(0.0)) * share
}

/// What a kart's wheels roll on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    Road,
    Kerb,
    Grass,
}

/// Half the kart's width: its outer wheel reaches this far past `lateral`.
const WHEEL_HALF: f32 = 0.45;

/// The surface under a kart `lateral` metres off the centre line of a road
/// `half_width` wide: the road, the kerb strip along each edge
/// ([`KERB_W`] wide) while either outer wheel is on it, or the grass beyond.
pub fn surface_at(lateral: f32, half_width: f32) -> Surface {
    let inner = lateral.abs() - WHEEL_HALF;
    let outer = lateral.abs() + WHEEL_HALF;
    if inner >= half_width + KERB_W {
        Surface::Grass
    } else if outer > half_width {
        Surface::Kerb
    } else {
        Surface::Road
    }
}

/// Loudness of the road, kerb and grass loops for a kart at `speed_frac`.
/// Only the surface underfoot sounds, louder with speed, silent when stopped
/// or in the air.
pub fn surface_gains(surface: Surface, speed_frac: f32, grounded: bool) -> [f32; 3] {
    let f = if speed_frac.is_finite() { speed_frac.abs().min(1.2) } else { 0.0 };
    let mut g = [0.0; 3];
    if !grounded || f < 0.02 {
        return g;
    }
    let (i, base) = match surface {
        Surface::Road => (0, 0.16),
        Surface::Kerb => (1, 0.5),
        Surface::Grass => (2, 0.55),
    };
    g[i] = base * f.powf(0.7);
    g
}

/// How far a rival's engine carries (m) and how loud it is at the player's
/// elbow.
pub const RIVAL_RANGE: f32 = 30.0;
const RIVAL_GAIN: f32 = 0.16;

/// One local player's voices.
#[derive(Debug, Clone, Copy)]
struct PlayerVoices {
    engine: VoiceId,
    /// Road, kerb, grass.
    surface: [VoiceId; 3],
    rival: VoiceId,
}

/// What a player's voices should do this frame: `(gain, rate)` for the
/// engine, the three surface loops' gains, and `(gain, rate)` for the
/// nearest rival's engine.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Targets {
    pub engine: (f32, f32),
    pub surface: [f32; 3],
    pub rival: (f32, f32),
}

/// The player's voices for kart `index`, from the race (`karts` in grid
/// order) and the track's `half_width`.
pub fn targets(karts: &[Kart], index: usize, players: usize, t: &Tune, half_width: f32) -> Targets {
    let k = &karts[index];
    let frac = k.speed / t.max_speed.max(1.0);
    // Over the line the engine eases off to a coast.
    let done = if k.finished.is_some() { 0.5 } else { 1.0 };
    let surface = surface_at(k.lateral, half_width);
    let mut surf = surface_gains(surface, frac, k.hop <= 0.05);
    for g in &mut surf {
        *g *= done;
    }
    // The nearest rival (any other kart) within earshot.
    let mut rival = (0.0, 1.0);
    let mut best = RIVAL_RANGE;
    for (j, r) in karts.iter().enumerate() {
        if j == index || r.player.is_some() {
            continue;
        }
        let d = ((r.pos[0] - k.pos[0]).powi(2) + (r.pos[2] - k.pos[2]).powi(2)).sqrt();
        if d < best {
            best = d;
            let rf = r.speed / t.max_speed.max(1.0);
            let near = 1.0 - d / RIVAL_RANGE;
            // A little flat of the player's own pitch so the two read apart.
            rival = (RIVAL_GAIN * near * near * (0.4 + 0.6 * rf.abs().min(1.0)), engine_rate(rf) * 0.93);
        }
    }
    Targets {
        engine: (engine_gain(frac, players) * done, engine_rate(frac)),
        surface: surf,
        rival,
    }
}

/// The voices of both local players (draw state).
#[derive(Debug, Clone, Default)]
pub struct Voices {
    players: [Option<PlayerVoices>; 2],
}

impl Voices {
    /// Once per frame. The voices run while a race is on (countdown and
    /// racing) and not paused; otherwise they are stopped.
    pub fn update(&mut self, mixer: &mut AudioMixer, sounds: &Sounds, race: &RaceView, t: &Tune) {
        let on = matches!(race.phase, Phase::Countdown | Phase::Racing) && !race.paused && race.players > 0;
        if !on {
            self.stop(mixer);
            return;
        }
        let half_width = race.track.half_width;
        for p in 0..2 {
            let kart = if p < race.players { race.player_kart(p) } else { None };
            let Some(i) = kart else {
                self.stop_player(mixer, p);
                continue;
            };
            if self.players[p].is_none() {
                self.players[p] = Self::start(mixer, sounds);
            }
            let Some(v) = self.players[p] else { continue };
            let tg = targets(&race.karts, i, race.players, t, half_width);
            // Player 2's engine sits a little higher so two karts are two sounds.
            let tilt = if p == 1 { 1.12 } else { 1.0 };
            mixer.voice_set(v.engine, tg.engine.0, tg.engine.1 * tilt);
            for (id, g) in v.surface.iter().zip(tg.surface) {
                mixer.voice_set(*id, g, 1.0);
            }
            mixer.voice_set(v.rival, tg.rival.0, tg.rival.1 * tilt);
        }
    }

    fn start(mixer: &mut AudioMixer, sounds: &Sounds) -> Option<PlayerVoices> {
        let engine = sounds.loop_id(Loop::Engine)?;
        let road = sounds.loop_id(Loop::Road)?;
        let kerb = sounds.loop_id(Loop::Kerb)?;
        let grass = sounds.loop_id(Loop::Grass)?;
        Some(PlayerVoices {
            engine: mixer.voice_start(engine, Bus::Effects, 0.0, IDLE_RATE),
            surface: [
                mixer.voice_start(road, Bus::Effects, 0.0, 1.0),
                mixer.voice_start(kerb, Bus::Effects, 0.0, 1.0),
                mixer.voice_start(grass, Bus::Effects, 0.0, 1.0),
            ],
            rival: mixer.voice_start(engine, Bus::Effects, 0.0, IDLE_RATE),
        })
    }

    fn stop_player(&mut self, mixer: &mut AudioMixer, p: usize) {
        if let Some(v) = self.players[p].take() {
            mixer.voice_stop(v.engine);
            mixer.voice_stop(v.rival);
            for s in v.surface {
                mixer.voice_stop(s);
            }
        }
    }

    /// Stop every voice.
    pub fn stop(&mut self, mixer: &mut AudioMixer) {
        for p in 0..2 {
            self.stop_player(mixer, p);
        }
    }

    /// True while any voice is going.
    pub fn is_running(&self) -> bool {
        self.players.iter().any(Option::is_some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tune() -> Tune {
        Tune::default()
    }

    #[test]
    fn the_engine_pitch_rises_with_speed_and_falls_with_it() {
        let rates: Vec<f32> = [0.0, 0.2, 0.4, 0.6, 0.8, 1.0, 1.2, 1.35].iter().map(|f| engine_rate(*f)).collect();
        assert!(rates.windows(2).all(|w| w[1] > w[0]), "{rates:?}");
        assert_eq!(engine_rate(0.0), IDLE_RATE);
        assert!((engine_rate(1.0) - TOP_RATE).abs() < 1e-5);
        // A boost above top speed climbs past top pitch; braking comes back down.
        assert!(engine_rate(1.35) > TOP_RATE + 0.3);
        assert!(engine_rate(0.5) < engine_rate(0.9));
        // Reversing and bad numbers behave.
        assert_eq!(engine_rate(-0.5), engine_rate(0.5));
        assert_eq!(engine_rate(f32::NAN), IDLE_RATE);
    }

    #[test]
    fn the_engine_is_louder_with_speed_and_two_players_share_the_room() {
        assert!(engine_gain(1.0, 1) > engine_gain(0.0, 1));
        assert!(engine_gain(0.0, 1) > 0.05, "idle is audible");
        assert!(engine_gain(1.0, 2) < engine_gain(1.0, 1));
        assert!(engine_gain(1.0, 1) < 0.6, "never swamps the effects");
    }

    #[test]
    fn the_surface_follows_how_far_off_the_road_the_kart_is() {
        let hw = 7.0;
        assert_eq!(surface_at(0.0, hw), Surface::Road);
        assert_eq!(surface_at(-3.0, hw), Surface::Road);
        assert_eq!(surface_at(hw - 0.5, hw), Surface::Road);
        // An outer wheel over the edge is on the kerb, on either side.
        assert_eq!(surface_at(hw - 0.2, hw), Surface::Kerb);
        assert_eq!(surface_at(-(hw + 0.4), hw), Surface::Kerb);
        // Both wheels past the kerb strip is grass.
        assert_eq!(surface_at(hw + KERB_W + WHEEL_HALF + 0.1, hw), Surface::Grass);
        assert_eq!(surface_at(-(hw + 6.0), hw), Surface::Grass);
    }

    #[test]
    fn only_the_surface_underfoot_sounds_and_grass_is_louder_than_road() {
        let road = surface_gains(Surface::Road, 0.8, true);
        let kerb = surface_gains(Surface::Kerb, 0.8, true);
        let grass = surface_gains(Surface::Grass, 0.8, true);
        assert!(road[0] > 0.0 && road[1] == 0.0 && road[2] == 0.0);
        assert!(kerb[1] > 0.0 && kerb[0] == 0.0 && kerb[2] == 0.0);
        assert!(grass[2] > 0.0 && grass[0] == 0.0 && grass[1] == 0.0);
        assert!(grass[2] > road[0] * 2.0, "going off the road is heard at once");
        // Faster is louder; stopped or airborne is silent.
        assert!(surface_gains(Surface::Grass, 1.0, true)[2] > grass[2]);
        assert_eq!(surface_gains(Surface::Grass, 0.0, true), [0.0; 3]);
        assert_eq!(surface_gains(Surface::Road, 1.0, false), [0.0; 3]);
    }

    fn kart_at(player: Option<usize>, x: f32, lateral: f32, speed: f32) -> Kart {
        let mut k = Kart::new(0, player, [x, 0.0, 0.0], 0.0);
        k.lateral = lateral;
        k.speed = speed;
        k
    }

    #[test]
    fn a_player_on_the_grass_at_speed_gets_the_grass_loop_and_a_high_engine() {
        let t = tune();
        let karts = [kart_at(Some(0), 0.0, 20.0, t.max_speed * 0.9), kart_at(None, 500.0, 0.0, 10.0)];
        let tg = targets(&karts, 0, 1, &t, 7.0);
        assert!(tg.surface[2] > 0.1 && tg.surface[0] == 0.0 && tg.surface[1] == 0.0);
        assert!(tg.engine.1 > 2.5);
        assert_eq!(tg.rival.0, 0.0, "a rival far off is not heard");
    }

    #[test]
    fn a_rival_alongside_is_heard_quietly_and_a_finished_player_coasts() {
        let t = tune();
        let karts = [kart_at(Some(0), 0.0, 0.0, t.max_speed), kart_at(None, 6.0, 0.0, t.max_speed)];
        let tg = targets(&karts, 0, 1, &t, 7.0);
        assert!(tg.rival.0 > 0.0 && tg.rival.0 < tg.engine.0 * 0.6, "{:?}", tg);
        assert!(tg.rival.1 < tg.engine.1, "the rival sits flatter");
        let mut done = karts;
        done[0].finished = Some(60.0);
        let td = targets(&done, 0, 1, &t, 7.0);
        assert!(td.engine.0 < tg.engine.0);
    }

    #[test]
    fn the_pitch_tracks_a_kart_speeding_up_and_braking() {
        let t = tune();
        let mut last = 0.0;
        for step in 0..=10 {
            let karts = [kart_at(Some(0), 0.0, 0.0, t.max_speed * step as f32 / 10.0)];
            let r = targets(&karts, 0, 1, &t, 7.0).engine.1;
            assert!(r > last, "step {step}: {r} vs {last}");
            last = r;
        }
        for step in (0..10).rev() {
            let karts = [kart_at(Some(0), 0.0, 0.0, t.max_speed * step as f32 / 10.0)];
            let r = targets(&karts, 0, 1, &t, 7.0).engine.1;
            assert!(r < last, "braking step {step}");
            last = r;
        }
    }
}
