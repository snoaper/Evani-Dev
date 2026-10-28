//! The in-page session: one [`Server`], the local [`Client`] and the
//! [`Loopback`] between them, so the match runs through exactly the path
//! online play will (inputs out, snapshots back, prediction and reconcile)
//! and bots run on the server. With the dev-only `net-dev` feature a second
//! local client joins too, played by a bot brain reading that client's own
//! prediction: a two-player session over the loopback, never shipped.
//!
//! evani-net's `LocalSession` holds one client; this one holds a list.
//!
//! The kit's systems in net builds:
//! * [`step_system`] (`Stage::Update`, the fixed step): the local input,
//!   one session tick, the play outcomes from the server's state.
//! * [`view_system`] (`Stage::Frame`, before the HUD and the drawing): the
//!   [`Match`] and [`Arena`] resources the HUD and the drawing read become
//!   the local prediction, with the local soldier at index 0 and the other
//!   soldiers interpolated between snapshots.

use evani_core::ecs::World;
use evani_core::input::Input;
use evani_core::play::PlayOutcomes;
use evani_core::rng::Pcg32;
use evani_core::time::Time;
use evani_net::types::mix64;
use evani_net::{Client, Loopback, NetConfig, NetId, NetMode, NetStats, Server};

use super::{HexBot, HexInput, HexNet, HexPose, HexState, net_id, wire};
use crate::arena::Arena;
use crate::bot::Brain;
use crate::match_rules::{Match, Phase, WinReason};
use crate::tune::Tune;
use crate::{TITLE_SEED, TacticalFps, read_input, tune_now};

/// Mixed into the session seed for the loopback's loss and jitter.
const LOOPBACK_SALT: u64 = 0x6865_785f_6c6f_6f70;
/// Mixed into the session seed for the server bots' brains.
const BRAIN_SALT: u64 = 0x6865_785f_6272_6169;
/// Local players: one, or two with `net-dev`.
#[cfg(not(feature = "net-dev"))]
const LOCAL_PLAYERS: usize = 1;
#[cfg(feature = "net-dev")]
const LOCAL_PLAYERS: usize = 2;

/// The session (a resource in net builds).
pub struct NetMatch {
    game: HexNet,
    cfg: NetConfig,
    link: Loopback,
    server: Server<HexNet>,
    /// `clients[0]` is the local player; with `net-dev`, `clients[1]` the
    /// second local player.
    clients: Vec<Client<HexNet>>,
    /// The second local player's brain (`net-dev`).
    #[cfg(feature = "net-dev")]
    dev: (HexBot, Pcg32),
    /// The server's bots were added.
    bots: bool,
    free_roam: bool,
    brains: Pcg32,
    /// The local soldier was alive, and the phase, on the server last tick.
    last: (bool, Phase),
    poses: Vec<(NetId, HexPose)>,
}

impl NetMatch {
    /// A session at the title (or free roam's open fortress), its seed from
    /// the world's seeded generator. The local players say hello at once.
    pub fn new(world: &World, free_roam: bool, t: Tune) -> Result<Self, String> {
        let cfg = NetConfig::from_world(world);
        let seed = world.try_res_mut::<Pcg32>().map_or(TITLE_SEED, |mut r| u64::from(r.next_u32()) << 32 | u64::from(r.next_u32()));
        let dt = world.try_res::<Time>().map_or(1.0 / 60.0, |t| t.fixed_dt);
        let arena = wire::base_arena(TITLE_SEED)?;
        let m = Match::new_net(free_roam, &arena, &t);
        let link = Loopback::new(cfg.loopback, mix64(seed ^ LOOPBACK_SALT));
        let server = Server::new(HexState { m, arena, seats: Vec::new() }, Box::new(link.server()), cfg, seed);
        let clients = (0..LOCAL_PLAYERS)
            .map(|_| {
                let mut c = Client::new(Box::new(link.connect()), cfg);
                c.connect();
                c
            })
            .collect();
        let brains = Pcg32::new(mix64(seed ^ BRAIN_SALT), 1);
        #[cfg(feature = "net-dev")]
        let dev = {
            let mut rng = Pcg32::new(mix64(seed ^ BRAIN_SALT), 2);
            (HexBot { brain: Brain::new(true, &mut rng) }, rng)
        };
        Ok(NetMatch {
            game: HexNet { tune: t, dt },
            cfg,
            link,
            server,
            clients,
            #[cfg(feature = "net-dev")]
            dev,
            bots: false,
            free_roam,
            brains,
            last: (false, Phase::Title),
            poses: Vec::new(),
        })
    }

    /// The state the local player sees: the prediction, or the server's
    /// before the first snapshot.
    pub fn view_state(&self) -> &HexState {
        self.clients.first().and_then(Client::predicted).unwrap_or_else(|| self.server.state())
    }

    /// The server's true state.
    pub fn authoritative(&self) -> &HexState {
        self.server.state()
    }

    /// Client `c`'s input this tick from the actions `p` (its soldier as
    /// predicted gives the aim to turn from; before it is seated, no aim).
    fn input_of(&self, c: usize, p: &crate::match_rules::PlayerInput) -> HexInput {
        let Some(client) = self.clients.get(c) else {
            return HexInput::default();
        };
        let sol = client.peer().zip(client.predicted()).and_then(|(me, st)| st.seat_of(me).and_then(|i| st.m.soldiers.get(i)));
        match sol {
            Some(s) => HexInput::from_player(p, s),
            None => HexInput::from_controls(
                &crate::body::Controls { fire: p.fire, jump: p.jump, ..Default::default() },
                0,
            ),
        }
    }

    /// The second local player's input: its brain on its own prediction.
    #[cfg(feature = "net-dev")]
    fn dev_input(&mut self) -> HexInput {
        let Some(client) = self.clients.get(1) else {
            return HexInput::default();
        };
        let (bot, rng) = &mut self.dev;
        client
            .peer()
            .zip(client.predicted())
            .and_then(|(me, st)| st.seat_of(me).and_then(|i| bot.controls(&self.game, st, i, rng)))
            .unwrap_or_default()
    }

    /// One fixed tick: the loopback moves on, every local client reads its
    /// snapshots and predicts with its input, then the server reads the
    /// inputs, adds the bots once the local players are in, and steps.
    fn tick(&mut self, local: HexInput) {
        #[cfg(feature = "net-dev")]
        let inputs = [local, self.dev_input()];
        #[cfg(not(feature = "net-dev"))]
        let inputs = [local];
        self.link.advance();
        for (c, input) in self.clients.iter_mut().zip(inputs) {
            c.poll(&self.game);
            c.step(&self.game, input);
        }
        self.server.poll();
        if !self.bots && self.clients.iter().all(|c| c.peer().is_some()) {
            self.bots = true;
            if !self.free_roam {
                let want = 2 * self.game.tune.team_size.max(0) as usize;
                for k in 0..want.saturating_sub(self.clients.len()) {
                    let brain = Brain::new(k % 2 == 0, &mut self.brains);
                    if self.server.add_bot(Box::new(HexBot { brain })).is_none() {
                        break;
                    }
                }
            }
        }
        self.server.tick(&self.game);
    }

    /// New settings for every part (the `net.*` parameters changed).
    fn set_config(&mut self, cfg: NetConfig) {
        self.cfg = cfg;
        self.link.set_conditions(cfg.loopback);
        self.server.set_config(cfg);
        for c in &mut self.clients {
            c.set_config(cfg);
        }
    }

    /// The netcode's counters for the overlay (the local client's).
    pub fn stats(&self) -> NetStats {
        NetStats {
            mode: NetMode::Loopback,
            client: self.clients.first().map(Client::stats).unwrap_or_default(),
            server: self.server.stats(),
            loopback: self.link.stats(),
        }
    }

    /// What this tick ended on the server for the local player: a lost
    /// life, and the match's end with its reason.
    fn outcomes(&mut self) -> (bool, Option<WinReason>) {
        let st = self.server.state();
        let me = self.clients.first().and_then(Client::peer).and_then(|p| st.seat_of(p));
        let alive = me.and_then(|i| st.m.soldiers.get(i)).is_some_and(|s| s.alive());
        let phase = st.m.phase;
        let died = self.last.0 && !alive && matches!(phase, Phase::Play);
        let ended = (self.last.1 == Phase::Play && phase == Phase::Results).then_some(st.m.why).flatten();
        self.last = (alive, phase);
        (died, ended)
    }

    /// The prediction as the HUD and drawing read it: the local soldier at
    /// index 0, every other soldier where the interpolated snapshots put
    /// it (render `alpha` between ticks).
    fn view(&mut self, alpha: f32, out: &mut Match) {
        let me = self.clients.first().and_then(Client::peer);
        let mut poses = core::mem::take(&mut self.poses);
        if let Some(c) = self.clients.first_mut() {
            c.remote_poses(alpha, &mut poses);
        }
        let st = self.view_state();
        out.clone_from(&st.m);
        for (j, (seat, sol)) in st.seats.iter().zip(out.soldiers.iter_mut()).enumerate() {
            if Some(seat.peer) == me || st.m.soldiers.get(j).is_none() {
                continue;
            }
            let id = net_id(seat.peer);
            if let Some((_, p)) = poses.iter().find(|(n, p)| *n == id && p.life == sol.spawns) {
                sol.body.p = p.p;
                sol.body.yaw = p.yaw;
                sol.body.pitch = p.pitch;
            }
        }
        if let Some(i) = me.and_then(|p| st.seat_of(p)).filter(|&i| i > 0 && i < out.soldiers.len()) {
            out.soldiers.swap(0, i);
            for pr in &mut out.projectiles {
                if pr.owner == i {
                    pr.owner = 0;
                } else if pr.owner == 0 {
                    pr.owner = i;
                }
            }
        }
        self.poses = poses;
    }

    /// The [`Arena`] resource follows the viewed one: rebuilt for a new
    /// seed, otherwise only its damage and ring angle are copied.
    fn view_arena(&self, out: &mut Arena) {
        let a = &self.view_state().arena;
        if out.seed != a.seed || out.gone.len() != a.gone.len() {
            out.clone_from(a);
            return;
        }
        if out.holes.version != a.holes.version {
            out.holes.clone_from(&a.holes);
        }
        out.gone.copy_from_slice(&a.gone);
        out.set_ring(a.ang);
    }
}

/// Start the session (the scene's reset, after the title's match and arena
/// are in the world).
pub(crate) fn start(world: &mut World, free_roam: bool, t: Tune) -> Result<(), String> {
    let s = NetMatch::new(world, free_roam, t)?;
    world.insert_resource(s);
    Ok(())
}

/// One fixed step in net builds: the local input goes out, the session
/// ticks, and the server's state gives the play outcomes. The whole session
/// (server, bots, loopback and client) lives in this page, so a pause skips
/// its tick and everything waits together (the same rule as offline,
/// [`crate::next_paused`]).
pub fn step_system(world: &mut World) {
    let Some(k) = world.try_res::<TacticalFps>().map(|k| *k) else {
        return;
    };
    let t = tune_now(world, &k);
    let dt = world.res::<Time>().fixed_dt;
    let (input, keys) = {
        let i = world.res::<Input>();
        (read_input(&i, &k.actions, &t, dt), crate::PauseKeys::read(&i, &k.actions))
    };
    let phase = world.try_res::<Match>().map_or(Phase::Title, |m| m.phase);
    if crate::pause_step(world, phase, keys) {
        return;
    }
    let cfg = NetConfig::from_world(world);
    let (died, ended, free_roam, stats) = {
        let Some(mut s) = world.try_res_mut::<NetMatch>() else {
            return;
        };
        s.game.tune = t;
        s.game.dt = dt;
        if cfg != s.cfg {
            s.set_config(cfg);
        }
        let local = s.input_of(0, &input);
        s.tick(local);
        let (died, ended) = s.outcomes();
        (died, ended, s.free_roam, s.stats())
    };
    if let Some(mut n) = world.try_res_mut::<NetStats>() {
        *n = stats;
    }
    if free_roam {
        return;
    }
    if let Some(mut o) = world.try_res_mut::<PlayOutcomes>() {
        if died {
            o.life_lost();
        }
        if let Some(why) = ended {
            o.game_over();
            if why == WinReason::Time {
                o.timer_expired();
            }
        }
    }
}

/// Each frame in net builds, before the HUD and the drawing: the [`Match`]
/// and [`Arena`] resources become the local view of the session.
pub fn view_system(world: &mut World) {
    let alpha = world.try_res::<Time>().map_or(0.0, |t| t.alpha);
    let Some(mut s) = world.try_res_mut::<NetMatch>() else {
        return;
    };
    if let Some(mut m) = world.try_res_mut::<Match>() {
        s.view(alpha, &mut m);
    }
    if let Some(mut a) = world.try_res_mut::<Arena>() {
        s.view_arena(&mut a);
    }
}
