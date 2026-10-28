//! [`RaceHud`]: everything a game's HUD shows, refreshed by the draw system
//! every frame. The demo renders it as light DOM; the kit itself draws only
//! the sprite minimap.

use crate::items::{Item, Slot};
use crate::race::{Phase, RaceView, ResultRow};
use crate::threat::{self, Threat};
use crate::Tune;

/// One local player's HUD.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PlayerHud {
    pub active: bool,
    pub name: String,
    /// 1-based race position.
    pub place: usize,
    pub racers: usize,
    /// Current lap (1-based) and the lap count.
    pub lap: u32,
    pub laps: u32,
    /// Item label held, or the roulette's current face while it spins.
    pub item: Option<&'static str>,
    /// Stable id of the same item (`zoom_pop`, ...) for an icon.
    pub item_id: Option<&'static str>,
    pub rolling: bool,
    pub wrong_way: bool,
    /// Drift charge tier (0..3) while drifting.
    pub drift_tier: u8,
    pub drifting: bool,
    pub boosting: bool,
    pub shielded: bool,
    pub spun_out: bool,
    pub speed_kmh: f32,
    /// Finish time once over the line.
    pub finished: Option<f32>,
    /// Podium (1st to 3rd) when finished.
    pub won: bool,
    /// Seconds since the last lap started (for a "Lap 2" flash), or large.
    pub lap_flash: f32,
    pub final_lap: bool,
    /// A start boost off the grid: "GOOD START!" for a moment after GO.
    pub good_start: bool,
    /// Seconds the "+1" pop on the position number still shows after the
    /// player passed someone (0 when it is not showing).
    pub gain_left: f32,
    /// Places gained by the last overtake (the pop reads "+N").
    pub gain_by: u32,
    /// Overtakes so far this race; the page restarts the pop when it changes.
    pub gains: u32,
    /// An item coming at this player (a Seeker Star homing in, a Boing Bomb
    /// about to arrive) and how urgent it is.
    pub threat: Option<Threat>,
    /// In another kart's slipstream (the "Slipstream" label), with the debug
    /// readout: speed, and the speed cap before and after the slipstream
    /// (m/s).
    pub slipstream: bool,
    pub speed_ms: f32,
    pub cap_base: f32,
    pub cap: f32,
}

/// How long the "+1" pop on the position number shows (s).
pub const GAIN_POP_S: f32 = 1.0;

impl PlayerHud {
    /// Move to `new_place` (1-based). A better place while racing starts the
    /// "+1" pop; otherwise the pop runs down by `dt`.
    pub fn track_place(&mut self, new_place: usize, racing: bool, dt: f32) {
        if racing && self.place > 0 && new_place < self.place {
            self.gain_by = (self.place - new_place) as u32;
            self.gain_left = GAIN_POP_S;
            self.gains += 1;
        } else {
            self.gain_left = (self.gain_left - dt).max(0.0);
        }
        self.place = new_place;
    }
}

/// World resource.
#[derive(Debug, Clone, PartialEq)]
pub struct RaceHud {
    pub phase: Phase,
    pub paused: bool,
    pub players: usize,
    /// 3, 2, 1 during the countdown, 0 otherwise.
    pub countdown: u8,
    /// Seconds since GO (the HUD flashes "GO!" for a moment).
    pub since_go: f32,
    pub clock: f32,
    pub players_hud: [PlayerHud; 2],
    /// Results by place once the race is finished (empty otherwise).
    pub results: Vec<ResultRow>,
    /// The podium scene is up (the celebration is over): the results panel
    /// shows beside it.
    pub podium: bool,
    pub track_name: String,
    /// Free roam: no laps, places, timer or countdown to show (the page puts
    /// a FREE ROAM label in their place).
    pub free_roam: bool,
    /// Render time of the last update, so timers that follow the sim clock
    /// (the "+1" pop) hold still while a capture holds it.
    pub last_render_t: f32,
    /// Debug log: one line per kart that entered or left a slipstream since
    /// the last update (the page prints them to the console).
    pub slip_log: Vec<String>,
    /// Which karts were slipstreaming at the last update.
    pub slip_prev: Vec<bool>,
}

impl Default for RaceHud {
    fn default() -> Self {
        Self {
            phase: Phase::Title,
            paused: false,
            players: 0,
            countdown: 0,
            since_go: 99.0,
            clock: 0.0,
            players_hud: [PlayerHud::default(), PlayerHud::default()],
            results: Vec::new(),
            podium: false,
            track_name: String::new(),
            free_roam: false,
            last_render_t: 0.0,
            slip_log: Vec::new(),
            slip_prev: Vec::new(),
        }
    }
}

/// How long "GOOD START!" shows after GO (s).
pub const GOOD_START_S: f32 = 1.5;

/// Format seconds as `m:ss.cc`.
pub fn format_time(s: f32) -> String {
    let s = s.max(0.0);
    let m = (s / 60.0).floor() as u32;
    let rest = s - m as f32 * 60.0;
    // fixed(): the same text as {rest:05.2} without float formatting code.
    format!("{m}:{:05}", evani_core::num::fixed(rest, 2))
}

/// Ordinal for a place (`1st`, `2nd`, ...).
pub fn ordinal(n: usize) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

impl RaceHud {
    /// Debug log lines for every kart (players and AI) whose slipstream
    /// started or ended since the last update, with the boosted numbers.
    fn log_slipstreams(&mut self, race: &RaceView) {
        use evani_core::num::fixed;
        self.slip_log.clear();
        self.slip_prev.resize(race.karts.len(), false);
        let racing = race.phase == Phase::Racing && !race.paused;
        for (i, k) in race.karts.iter().enumerate() {
            let on = racing && k.slipstream;
            if on == self.slip_prev[i] {
                continue;
            }
            self.slip_prev[i] = on;
            let who = match k.player {
                Some(p) => format!("{} (player {})", race.roster[k.spec].name, p + 1),
                None => format!("{} (kart {i}, AI)", race.roster[k.spec].name),
            };
            self.slip_log.push(if on {
                let behind = match k.draft {
                    crate::slipstream::Slip::Behind(j) => race.karts.get(j).map_or("?", |o| race.roster[o.spec].name.as_str()),
                    _ => "?",
                };
                format!(
                    "[slipstream] {who} ON behind {behind}: speed {} m/s, cap {} -> {} m/s (x{})",
                    fixed(k.speed, 2),
                    fixed(k.cap_base, 2),
                    fixed(k.cap, 2),
                    fixed(crate::slipstream::SLIPSTREAM_MULTIPLIER, 2)
                )
            } else {
                format!(
                    "[slipstream] {who} OFF ({}): speed {} m/s, cap back to {} m/s",
                    if racing { k.draft.label() } else { "race paused or over" },
                    fixed(k.speed, 2),
                    fixed(k.cap_base, 2)
                )
            });
        }
    }

    /// Refresh from the race. `lap_flash[p]` is maintained by the caller.
    pub fn update(&mut self, race: &RaceView, t: &Tune, frame_dt: f32, render_t: f32) {
        // Sim-clock seconds since the last update (0 while a capture holds it).
        let sim_dt = (render_t - self.last_render_t).clamp(0.0, 0.1);
        self.last_render_t = render_t;
        self.phase = race.phase;
        self.paused = race.paused;
        self.players = race.players;
        self.clock = race.clock;
        self.track_name.clone_from(&race.track.name);
        self.free_roam = race.free_roam;
        self.countdown = if race.phase == Phase::Countdown {
            race.countdown.ceil().clamp(1.0, 9.0) as u8
        } else {
            0
        };
        self.since_go = if race.free_roam {
            // No GO!: free roam starts already driving.
            99.0
        } else if race.phase == Phase::Racing || race.phase == Phase::Finished {
            race.clock
        } else {
            99.0
        };
        for p in 0..2 {
            let h = &mut self.players_hud[p];
            let Some(i) = race.player_kart(p) else {
                *h = PlayerHud::default();
                continue;
            };
            let k = &race.karts[i];
            let lap = race.lap_of(k);
            let was_active = h.active;
            if h.active && lap != h.lap {
                h.lap_flash = 0.0;
            } else {
                h.lap_flash += frame_dt;
            }
            h.active = true;
            h.name.clone_from(&race.roster[k.spec].name);
            // A new race starts from a clean slate: no pop for the grid.
            if !was_active || race.phase != Phase::Racing {
                h.place = 0;
            }
            h.track_place(k.place + 1, race.phase == Phase::Racing, sim_dt);
            h.racers = race.karts.len();
            h.lap = lap;
            h.laps = race.counts.laps;
            h.final_lap = lap == race.counts.laps && race.counts.laps > 1;
            let (item, rolling) = match k.slot {
                Slot::Empty => (None, false),
                Slot::Holding(it) => (Some(it), false),
                // The roulette flips faces several times a second.
                Slot::Rolling { .. } => {
                    let face = ((render_t * 12.0) as usize + p) % Item::ALL.len();
                    (Some(Item::ALL[face]), true)
                }
            };
            h.item = item.map(Item::label);
            h.item_id = item.map(Item::id);
            h.rolling = rolling;
            h.wrong_way = k.wrong_way && k.finished.is_none();
            h.drifting = k.drift != 0.0;
            h.drift_tier = k.tier(t);
            h.boosting = k.boost > 0.0;
            h.shielded = k.shield > 0.0;
            h.spun_out = k.spin > 0.0;
            h.speed_kmh = k.speed.abs() * 3.6;
            h.finished = k.finished;
            h.threat = if race.phase == Phase::Racing && !race.paused && k.finished.is_none() {
                threat::assess(i, k, &race.seekers, &race.bombs, t.bomb_radius)
            } else {
                None
            };
            h.won = k.finished.is_some() && k.place < 3;
            h.slipstream = k.slipstream && race.phase == Phase::Racing && k.finished.is_none();
            h.speed_ms = k.speed;
            h.cap_base = k.cap_base;
            h.cap = k.cap;
            h.good_start = k.launch == crate::kart::Launch::Primed && race.phase == Phase::Racing && race.phase_time < GOOD_START_S;
        }
        self.log_slipstreams(race);
        self.podium = crate::finale::stage(race.phase, race.phase_time, race.players) == crate::finale::Stage::Podium;
        if race.phase == Phase::Finished {
            self.results = race.results();
        } else {
            self.results.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The HUD text is the same as the `format!` it replaced (lean-data):
    /// every race time at frame steps up to 10 minutes, and speeds.
    #[test]
    fn hud_text_matches_the_old_float_formatting() {
        let old_time = |s: f32| {
            let s = s.max(0.0);
            let m = (s / 60.0).floor() as u32;
            let rest = s - m as f32 * 60.0;
            format!("{m}:{rest:05.2}")
        };
        for f in 0..=36_000u32 {
            let s = f as f32 / 60.0;
            assert_eq!(format_time(s), old_time(s), "{s}");
            let s2 = s + 0.004_7;
            assert_eq!(format_time(s2), old_time(s2), "{s2}");
        }
        for k in -200..=40_000i32 {
            let v = k as f32 * 0.0137;
            assert_eq!(evani_core::num::fixed(v, 0).to_string(), format!("{v:.0}"), "{v}");
        }
    }

    #[test]
    fn formats() {
        assert_eq!(format_time(83.456), "1:23.46");
        assert_eq!(format_time(5.0), "0:05.00");
        assert_eq!(ordinal(1), "1st");
        assert_eq!(ordinal(2), "2nd");
        assert_eq!(ordinal(3), "3rd");
        assert_eq!(ordinal(8), "8th");
        assert_eq!(ordinal(12), "12th");
    }

    #[test]
    fn passing_someone_pops_a_plus_one_that_runs_down() {
        let mut h = PlayerHud::default();
        // The grid, then the first racing frame: a baseline, no pop.
        h.track_place(8, false, 0.016);
        h.track_place(8, true, 0.016);
        assert_eq!((h.gain_left, h.gains), (0.0, 0));
        // Passing one kart.
        h.track_place(7, true, 0.016);
        assert_eq!((h.gain_by, h.gains), (1, 1));
        assert!((h.gain_left - GAIN_POP_S).abs() < 1e-6);
        // It runs down on the sim clock and then stays off.
        h.track_place(7, true, 0.6);
        assert!(h.gain_left > 0.0 && h.gain_left < GAIN_POP_S);
        h.track_place(7, true, 0.6);
        assert_eq!(h.gain_left, 0.0);
        // A held clock (dt 0) keeps it showing.
        h.track_place(6, true, 0.0);
        h.track_place(6, true, 0.0);
        assert!(h.gain_left > 0.0);
        assert_eq!(h.gains, 2);
    }

    #[test]
    fn losing_a_place_or_the_grid_shuffle_never_pops() {
        let mut h = PlayerHud::default();
        h.track_place(5, true, 0.016);
        h.track_place(6, true, 0.016);
        assert_eq!((h.gain_left, h.gains), (0.0, 0));
        // Not racing (the countdown, the results): no pop for a better place.
        h.track_place(3, false, 0.016);
        assert_eq!((h.gain_left, h.gains), (0.0, 0));
        // Two places at once reads "+2".
        h.track_place(5, true, 0.016);
        h.track_place(3, true, 0.016);
        assert_eq!((h.gain_by, h.gains), (2, 1));
    }
}
