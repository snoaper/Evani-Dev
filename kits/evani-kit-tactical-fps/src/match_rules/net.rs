//! The match over evani-net (net builds): its wire format and what the
//! server's step does to seats (players and bots joining and leaving, a
//! match starting with the soldiers already seated). The single-player
//! paths in the parent module are untouched.
//!
//! Not sent: a soldier's `brain` (bots think on the server, in
//! [`crate::net::HexBot`], so every soldier's brain is `None` here) and the
//! per-step lag-compensation table `lag`.

use evani_net::{Reader, Wire, WireError, Writer};

use super::{Match, Phase};
use crate::arena::Arena;
use crate::net::wire::{get_usize, put_usize};
use crate::ring::Ring;
use crate::tune::Tune;

impl Wire for Match {
    fn write(&self, w: &mut Writer) {
        self.phase.write(w);
        w.f32(self.phase_t);
        self.steps.write(w);
        self.free_roam.write(w);
        self.soldiers.write(w);
        self.projectiles.write(w);
        self.shots.write(w);
        self.blasts.write(w);
        self.score.write(w);
        self.hold.write(w);
        self.winner.map(|t| t as u8).write(w);
        self.why.write(w);
        self.ring.write(w);
        put_usize(w, self.spawn_next[0]);
        put_usize(w, self.spawn_next[1]);
        put_usize(w, self.bots_made);
        self.seen_holes.write(w);
        self.feed.write(w);
    }
    fn read(r: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Match {
            phase: r.get()?,
            phase_t: r.f32()?,
            steps: r.get()?,
            free_roam: r.get()?,
            soldiers: r.get()?,
            projectiles: r.get()?,
            shots: r.get()?,
            blasts: r.get()?,
            score: r.get()?,
            hold: r.get()?,
            winner: r.get::<Option<u8>>()?.map(usize::from),
            why: r.get()?,
            ring: r.get::<Ring>()?,
            spawn_next: [get_usize(r)? % 6, get_usize(r)? % 6],
            bots_made: get_usize(r)?,
            seen_holes: r.get()?,
            feed: r.get()?,
            lag: Vec::new(),
        })
    }
}

impl Match {
    /// An empty match for a session: no soldiers yet (players and bots
    /// join through the server), at the title, or in play for free roam.
    pub(crate) fn new_net(free_roam: bool, arena: &Arena, t: &Tune) -> Self {
        let mut m = Match::new(free_roam, arena, t);
        m.soldiers.clear();
        m.spawn_next = [0; 2];
        m
    }

    /// Seat a soldier that just joined, on the team with fewer soldiers
    /// (Blue on a tie), and spawn it. `player` is the player's number for
    /// a person, `None` for a bot (named from the arena's bot names).
    /// Returns its index.
    pub(crate) fn seat(&mut self, player: Option<usize>, arena: &Arena, t: &Tune) -> usize {
        let blue = self.soldiers.iter().filter(|s| s.team == 0).count();
        let team = usize::from(blue > self.soldiers.len() - blue);
        let name = match player {
            Some(n) => format!("Player {n}"),
            None => {
                let names = &arena.def.bot_names;
                let name = match names.get(self.bots_made % names.len().max(1)) {
                    Some(n) => format!("Bot {n}"),
                    None => format!("Bot {}", self.bots_made + 1),
                };
                self.bots_made += 1;
                name
            }
        };
        self.add_soldier(name, team, None, arena, t);
        self.soldiers.len() - 1
    }

    /// Take soldier `k` out (its peer left). Its rockets and grenades in
    /// flight go with it; later soldiers move down one index.
    pub(crate) fn unseat(&mut self, k: usize) {
        if k >= self.soldiers.len() {
            return;
        }
        self.soldiers.remove(k);
        self.projectiles.retain(|p| p.owner != k);
        for p in &mut self.projectiles {
            if p.owner > k {
                p.owner -= 1;
            }
        }
    }

    /// A new match on a fresh arena with the soldiers already seated
    /// (single player's `start_match` fills the teams with bots instead):
    /// scores cleared, everyone respawned, warm-up.
    pub(crate) fn start_match_net(&mut self, arena: &mut Arena, t: &Tune) {
        self.phase = Phase::Pre;
        self.phase_t = 0.0;
        self.score = [0; 2];
        self.hold = [0.0; 2];
        self.winner = None;
        self.why = None;
        self.projectiles.clear();
        self.shots.clear();
        self.blasts.clear();
        self.feed.clear();
        self.ring = Ring::new(t.ring_interval_s);
        arena.set_ring(0.0);
        self.seen_holes = arena.holes.version;
        self.spawn_next = [0; 2];
        for i in 0..self.soldiers.len() {
            if let Some(s) = self.soldiers.get_mut(i) {
                s.kills = 0;
                s.deaths = 0;
            }
            self.spawn(i, arena, t);
        }
    }

    /// Where shooter `i`'s hitscan tests soldier `j`: where `j` stood as
    /// `i` saw it (lag compensation), else where it is now.
    pub(crate) fn lag_target(&self, i: usize, j: usize) -> [f32; 3] {
        let live = self.soldiers.get(j).map_or([0.0; 3], |s| s.body.p);
        self.lag
            .iter()
            .find(|(shooter, _)| *shooter == i)
            .and_then(|(_, seen)| seen.get(j).copied().flatten())
            .unwrap_or(live)
    }
}
