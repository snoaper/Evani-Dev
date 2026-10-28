//! Offline play through the real netcode: a [`Server`] and one [`Client`]
//! in the same page, joined by a [`Loopback`]. The local player is a client
//! like any other (inputs, prediction, snapshots, reconcile); bots are
//! server-side peers. A game written against a `LocalSession` goes online
//! by swapping the loopback for a network transport, not by changing its
//! step.
//!
//! The loopback can add latency, jitter and loss
//! (`net.loopback_latency_ms`, `net.loopback_jitter_ms`,
//! `net.loopback_loss_pct`), so prediction and lag compensation can be
//! tuned offline. They are deterministic: the same seed loses the same
//! packets.

use evani_core::ecs::World;

use crate::client::{Client, ClientStatus};
use crate::config::{NetConfig, NetMode, NetStats};
use crate::game::{Bot, NetGame};
use crate::interp::Sampled;
use crate::server::Server;
use crate::transport::Loopback;
use crate::types::{NetId, PeerId, Tick, TickTime, mix64};

/// Mixed into the session seed for the loopback's loss and jitter, so they
/// are unrelated to the game's own random numbers.
const LOOPBACK_SALT: u64 = 0x6e65_745f_6c6f_6f70;

/// A server, one local client and the loopback between them.
pub struct LocalSession<G: NetGame> {
    game: G,
    net: Loopback,
    server: Server<G>,
    client: Client<G>,
    cfg: NetConfig,
}

impl<G: NetGame> LocalSession<G> {
    /// A session starting from `initial` (the server's state at tick 0).
    /// The local client says hello at once and is playing a few ticks later.
    pub fn new(game: G, initial: G::State, cfg: NetConfig, seed: u64) -> Self {
        let net = Loopback::new(cfg.loopback, mix64(seed ^ LOOPBACK_SALT));
        let server = Server::new(initial, Box::new(net.server()), cfg, seed);
        let mut client = Client::new(Box::new(net.connect()), cfg);
        client.connect();
        Self {
            game,
            net,
            server,
            client,
            cfg,
        }
    }

    /// One fixed tick with the local player's `input`: the loopback moves
    /// on, the client reads snapshots and predicts, then the server reads
    /// inputs and simulates.
    pub fn tick(&mut self, input: G::Input) {
        self.net.advance();
        self.client.poll(&self.game);
        self.client.step(&self.game, input);
        self.server.poll();
        self.server.tick(&self.game);
    }

    /// Add a server-side bot; `None` when full.
    pub fn add_bot(&mut self, bot: Box<dyn Bot<G>>) -> Option<PeerId> {
        self.server.add_bot(bot)
    }

    pub fn remove_peer(&mut self, peer: PeerId) -> bool {
        self.server.remove_peer(peer)
    }

    /// The local player's peer id, once the handshake is done.
    pub fn local_peer(&self) -> Option<PeerId> {
        self.client.peer()
    }

    /// True once the local player is predicting.
    pub fn playing(&self) -> bool {
        self.client.status() == ClientStatus::Connected && self.client.predicted().is_some()
    }

    /// The state to draw the local player from: the prediction, or the
    /// server's state before the first snapshot.
    pub fn view_state(&self) -> &G::State {
        self.client
            .predicted()
            .unwrap_or_else(|| self.server.state())
    }

    pub fn predicted(&self) -> Option<&G::State> {
        self.client.predicted()
    }

    /// The server's true state (scoring, round logic, tools).
    pub fn authoritative(&self) -> &G::State {
        self.server.state()
    }

    /// See [`Client::frame_view`].
    pub fn frame_view(&mut self, alpha: f32) -> TickTime {
        self.client.frame_view(alpha)
    }

    /// See [`Client::remote_poses`].
    pub fn remote_poses(&mut self, alpha: f32, out: &mut Vec<(NetId, G::Pose)>) -> Sampled {
        self.client.remote_poses(alpha, out)
    }

    /// The server's tick.
    pub fn now(&self) -> Tick {
        self.server.now()
    }

    pub fn config(&self) -> &NetConfig {
        &self.cfg
    }

    /// New settings for every part, the loopback's conditions included.
    pub fn set_config(&mut self, cfg: NetConfig) {
        self.cfg = cfg;
        self.net.set_conditions(cfg.loopback);
        self.server.set_config(cfg);
        self.client.set_config(cfg);
    }

    pub fn stats(&self) -> NetStats {
        NetStats {
            mode: NetMode::Loopback,
            client: self.client.stats(),
            server: self.server.stats(),
            loopback: self.net.stats(),
        }
    }

    /// Copy the counters into the world's [`NetStats`] (the net module's
    /// overlay line reads it). Call once per tick or frame.
    pub fn publish_stats(&self, world: &World) {
        if let Some(mut s) = world.try_res_mut::<NetStats>() {
            *s = self.stats();
        }
    }

    pub fn game(&self) -> &G {
        &self.game
    }

    pub fn server(&self) -> &Server<G> {
        &self.server
    }

    pub fn server_mut(&mut self) -> &mut Server<G> {
        &mut self.server
    }

    pub fn client(&self) -> &Client<G> {
        &self.client
    }

    pub fn client_mut(&mut self) -> &mut Client<G> {
        &mut self.client
    }

    pub fn loopback(&self) -> &Loopback {
        &self.net
    }
}
