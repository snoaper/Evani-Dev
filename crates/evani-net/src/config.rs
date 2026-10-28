//! Session settings (from the `net.*` parameters) and the counters the
//! overlay and tools read.

use evani_core::ecs::World;
use evani_core::params::ParamRegistry;
use evani_core::time::Time;

use crate::NetParams;
use crate::transport::{LinkConditions, LoopbackStats};
use crate::types::{Tick, TickTime};

/// How a session runs. All times are in ticks or sub-ticks of the fixed
/// step, so the netcode never reads a clock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NetConfig {
    /// Simulation ticks per second (the engine's fixed step).
    pub tick_hz: u32,
    /// The server sends a snapshot every this many ticks (`net.send_rate_hz`).
    pub snapshot_every: u32,
    /// How far behind the newest snapshot remote entities are drawn
    /// (`net.interp_delay_ms`), in sub-ticks.
    pub interp_delay_subs: u64,
    /// The furthest back lag compensation rewinds (`net.lag_comp_max_ms`),
    /// in sub-ticks; 0 turns it off.
    pub lag_comp_max_subs: u64,
    /// Input frames repeated in every input packet (at least this many,
    /// more while the server has not acknowledged them).
    pub input_redundancy: u32,
    /// A client stops predicting when it is this many ticks past its
    /// newest snapshot (the link has gone quiet).
    pub max_predict_ticks: u32,
    /// The server keeps inputs at most this many ticks ahead of its tick.
    pub max_input_ahead: u32,
    /// Ticks early a client aims for its inputs to arrive.
    pub target_slack: i32,
    /// Pose frames kept for interpolation and lag compensation.
    pub pose_history: usize,
    /// Players allowed at once, bots included.
    pub max_peers: usize,
    /// The server drops a client it has not heard from in this many ticks.
    pub timeout_ticks: u32,
    /// Compare a checksum of each predicted state with the snapshot and
    /// replay only on a mismatch. Off: replay after every snapshot.
    pub verify_prediction: bool,
    /// The in-page link's simulated latency, jitter and loss.
    pub loopback: LinkConditions,
}

impl Default for NetConfig {
    fn default() -> Self {
        Self {
            tick_hz: 60,
            snapshot_every: 3,
            interp_delay_subs: 6 * TickTime::SUBS,
            lag_comp_max_subs: 12 * TickTime::SUBS,
            input_redundancy: 4,
            max_predict_ticks: 30,
            max_input_ahead: 60,
            target_slack: 1,
            pose_history: 32,
            max_peers: 16,
            timeout_ticks: 600,
            verify_prediction: true,
            loopback: LinkConditions::default(),
        }
    }
}

impl NetConfig {
    /// The defaults with the fixed step from [`Time`] and the `net.*`
    /// parameters from the registry (when the net module is built in).
    pub fn from_world(world: &World) -> Self {
        let mut c = Self::default();
        let dt = world.try_res::<Time>().map_or(0.0, |t| t.fixed_dt);
        if dt > 0.0 && dt.is_finite() {
            c.tick_hz = ((1.0 / dt).round() as u32).clamp(1, 1000);
        }
        let (Some(p), Some(reg)) = (
            world.try_res::<NetParams>(),
            world.try_res::<ParamRegistry>(),
        ) else {
            return c;
        };
        let hz = c.tick_hz as f32;
        let send = reg.i32(p.send_rate_hz).max(1) as f32;
        c.snapshot_every = ((hz / send).round() as u32).max(1);
        c.interp_delay_subs = ms_to_subs(reg.f32(p.interp_delay_ms), c.tick_hz);
        c.lag_comp_max_subs = ms_to_subs(reg.f32(p.lag_comp_max_ms), c.tick_hz);
        c.loopback = LinkConditions {
            latency_ticks: ms_to_ticks(reg.f32(p.loopback_latency_ms), c.tick_hz),
            jitter_ticks: ms_to_ticks(reg.f32(p.loopback_jitter_ms), c.tick_hz),
            loss_permille: pct_to_permille(reg.f32(p.loopback_loss_pct)),
        };
        // Enough frames to cover the furthest rewind plus the draw delay.
        let span = c.lag_comp_max_subs.saturating_add(c.interp_delay_subs);
        let per_frame = u64::from(c.snapshot_every) * TickTime::SUBS;
        let needed = usize::try_from(span / per_frame.max(1)).unwrap_or(usize::MAX);
        c.pose_history = c.pose_history.max(needed.saturating_add(4)).min(1024);
        c
    }

    /// Ticks in `ms` at this tick rate, rounded.
    pub fn ms_to_ticks(&self, ms: f32) -> u32 {
        ms_to_ticks(ms, self.tick_hz)
    }
}

/// `ms` as sub-ticks at `hz`, rounded; negative or NaN gives 0.
pub fn ms_to_subs(ms: f32, hz: u32) -> u64 {
    let v = f64::from(ms) * f64::from(hz) / 1000.0 * TickTime::SUBS as f64;
    if v.is_finite() && v > 0.0 {
        v.round() as u64
    } else {
        0
    }
}

/// `ms` as whole ticks at `hz`, rounded; negative or NaN gives 0.
pub fn ms_to_ticks(ms: f32, hz: u32) -> u32 {
    let v = f64::from(ms) * f64::from(hz) / 1000.0;
    if v.is_finite() && v > 0.0 {
        v.round().min(f64::from(u32::MAX)) as u32
    } else {
        0
    }
}

fn pct_to_permille(pct: f32) -> u32 {
    if pct.is_finite() && pct > 0.0 {
        ((pct * 10.0).round() as u32).min(1000)
    } else {
        0
    }
}

/// What the session is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NetMode {
    /// No session running.
    #[default]
    Idle,
    /// Server and client in this page, joined by the loopback (offline
    /// play and bots).
    Loopback,
    /// A client of a remote server.
    Client,
    /// A server with remote clients.
    Server,
}

impl NetMode {
    pub fn label(self) -> &'static str {
        match self {
            NetMode::Idle => "idle",
            NetMode::Loopback => "loopback",
            NetMode::Client => "client",
            NetMode::Server => "server",
        }
    }
}

/// Client-side counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ClientStats {
    /// The tick the client predicts next.
    pub tick: Tick,
    /// The newest snapshot's tick.
    pub snapshot_tick: Tick,
    pub snapshots: u64,
    /// Snapshots whose state matched the prediction (no replay).
    pub confirmed: u64,
    /// Snapshots that disagreed and made the client replay.
    pub rollbacks: u64,
    /// Ticks replayed by rollbacks.
    pub resim_ticks: u64,
    /// Times the client dropped its prediction and took the snapshot as is.
    pub resyncs: u64,
    /// Ticks not stepped because prediction ran too far past the snapshot.
    pub stalls: u64,
    /// Extra ticks stepped and ticks skipped to keep inputs on time.
    pub lead_ups: u64,
    pub lead_downs: u64,
    /// The server's last slack report.
    pub slack: i32,
    pub bytes_in: u64,
    pub bytes_out: u64,
    /// Packets that did not decode.
    pub bad_packets: u64,
}

/// Server-side counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ServerStats {
    pub tick: Tick,
    /// Remote players.
    pub clients: u32,
    pub bots: u32,
    /// Ticks where a player's input was missing and the last one repeated.
    pub repeated_inputs: u64,
    pub joins: u64,
    pub leaves: u64,
    pub refused: u64,
    pub timeouts: u64,
    /// Size of the last encoded state.
    pub state_bytes: u32,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub bad_packets: u64,
}

/// The netcode's counters as a world resource (inserted by the net module;
/// a running session writes it each tick, see
/// [`crate::session::LocalSession::publish_stats`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NetStats {
    pub mode: NetMode,
    pub client: ClientStats,
    pub server: ServerStats,
    pub loopback: LoopbackStats,
}

impl NetStats {
    /// Players in the session, bots included.
    pub fn peers(&self) -> u32 {
        self.server.clients.saturating_add(self.server.bots)
    }
}
