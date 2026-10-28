//! Multiplayer module (`evani.module.net`): a deterministic netcode core.
//! Only games that list `net = true` in evani.toml build it.
//!
//! * [`NetGame`]: the game's fixed-step simulation, input, state and pose
//!   types. The same step runs on the server, in client prediction and in
//!   rollback replays, seeded from [`StepCtx::rng`].
//! * [`msg`]: input and snapshot packets ([`wire`] encodes them).
//! * [`Server`]: the authority; queues every peer's inputs (bots included),
//!   steps the state, sends snapshots, and keeps the [`PoseHistory`] that
//!   lag compensation ([`StepCtx::rewind`]) rewinds through.
//! * [`Client`]: prediction with rollback and reconcile, input pacing, and
//!   entity interpolation ([`Client::remote_poses`]).
//! * [`Transport`]: the packet interface; [`Loopback`] is the in-page one.
//! * [`LocalSession`]: server, local client and loopback in one page, so
//!   offline play and bots run through the same path online play will.
//!
//! The module itself registers the `net.*` parameters, inserts the
//! [`NetStats`] resource and reports it in the overlay.

use core::fmt::Write;

use evani_core::ecs::World;
use evani_core::params::{BakeTarget, F32Param, I32Param, ParamDef, ParamRegistry};
use evani_core::plugin::{EngineModule, PluginContext, PluginError};

pub mod client;
pub mod config;
pub mod game;
pub mod interp;
#[cfg(feature = "lockstep")]
pub mod lockstep;
pub mod msg;
#[cfg(all(feature = "port", target_arch = "wasm32"))]
pub mod port;
#[cfg(feature = "referee")]
pub mod referee;
pub mod server;
pub mod session;
pub mod synctest;
pub mod transport;
pub mod types;
pub mod wire;

pub use client::{Client, ClientStatus};
pub use config::{ClientStats, NetConfig, NetMode, NetStats, ServerStats};
pub use game::{Bot, InputSource, NetGame, PeerInput, StepCtx, tick_rng};
pub use interp::{Interpolate, PoseHistory, Sampled, hold, lerp_angle};
pub use server::Server;
pub use session::LocalSession;
pub use transport::{LinkConditions, Loopback, LoopbackEnd, LoopbackStats, Transport};
pub use types::{LinkId, NetId, PeerId, Role, Tick, TickTime};
pub use wire::{Reader, Wire, WireError, Writer};

#[derive(Debug, Clone, Copy)]
pub struct NetParams {
    pub send_rate_hz: I32Param,
    pub interp_delay_ms: F32Param,
    pub lag_comp_max_ms: F32Param,
    pub loopback_latency_ms: F32Param,
    pub loopback_jitter_ms: F32Param,
    pub loopback_loss_pct: F32Param,
    /// Lockstep play only (feature `lockstep`): other games do not carry them.
    #[cfg(feature = "lockstep")]
    pub input_delay_ticks: I32Param,
    #[cfg(feature = "lockstep")]
    pub max_catchup_steps: I32Param,
}

#[derive(Debug, Default)]
pub struct NetModule {
    params: Option<NetParams>,
}

impl NetModule {
    pub fn new() -> Self {
        Self::default()
    }
}

impl EngineModule for NetModule {
    fn id(&self) -> &'static str {
        "evani.module.net"
    }

    fn label(&self) -> &'static str {
        "net"
    }

    fn version(&self) -> Option<&'static str> {
        Some(env!("CARGO_PKG_VERSION"))
    }

    fn build(&mut self, ctx: &mut PluginContext) -> Result<(), PluginError> {
        let p = NetParams {
            send_rate_hz: ctx.param_i32(
                ParamDef::i32("net.send_rate_hz", 20)
                    .range(1.0, 60.0)
                    .unit("Hz")
                    .describe("How often state is sent to peers.")
                    .owner("net")
                    .bake(BakeTarget::Kit),
            )?,
            interp_delay_ms: ctx.param_f32(
                ParamDef::f32("net.interp_delay_ms", 100.0)
                    .range(0.0, 500.0)
                    .unit("ms")
                    .describe("How far behind remote objects are drawn, to hide jitter.")
                    .owner("net")
                    .bake(BakeTarget::Kit),
            )?,
            lag_comp_max_ms: ctx.param_f32(
                ParamDef::f32("net.lag_comp_max_ms", 200.0)
                    .range(0.0, 1000.0)
                    .unit("ms")
                    .describe("How far back the server rewinds to check a shot against what the shooter saw (0 turns it off).")
                    .owner("net")
                    .bake(BakeTarget::Kit),
            )?,
            loopback_latency_ms: ctx.param_f32(
                ParamDef::f32("net.loopback_latency_ms", 0.0)
                    .range(0.0, 500.0)
                    .unit("ms")
                    .describe("Offline play: one-way delay added to the in-page link, to try the game as if online.")
                    .owner("net")
                    .bake(BakeTarget::Kit),
            )?,
            loopback_jitter_ms: ctx.param_f32(
                ParamDef::f32("net.loopback_jitter_ms", 0.0)
                    .range(0.0, 250.0)
                    .unit("ms")
                    .describe("Offline play: random extra delay per packet on the in-page link.")
                    .owner("net")
                    .bake(BakeTarget::Kit),
            )?,
            loopback_loss_pct: ctx.param_f32(
                ParamDef::f32("net.loopback_loss_pct", 0.0)
                    .range(0.0, 50.0)
                    .unit("%")
                    .describe("Offline play: share of packets the in-page link drops.")
                    .owner("net")
                    .bake(BakeTarget::Kit),
            )?,
            #[cfg(feature = "lockstep")]
            input_delay_ticks: ctx.param_i32(
                ParamDef::i32("net.input_delay_ticks", 0)
                    .range(0.0, 6.0)
                    .unit("ticks")
                    .describe("Lockstep play: a player's input applies this many ticks after it is read (0 reacts at once; more trims corrections of other players at the cost of feel). Read when a session starts.")
                    .owner("net")
                    .bake(BakeTarget::Kit),
            )?,
            #[cfg(feature = "lockstep")]
            max_catchup_steps: ctx.param_i32(
                ParamDef::i32("net.max_catchup_steps", 0)
                    .range(0.0, 240.0)
                    .unit("steps")
                    .describe("Lockstep play: most game steps one tick runs to catch up (prediction replay, a spectator's backlog); the rest waits for the next ticks. 0 is no cap; otherwise 2 to 240 (1 counts as 2). Lower it for a slow host.")
                    .owner("net")
                    .bake(BakeTarget::Kit),
            )?,
        };
        ctx.resource(p);
        ctx.resource(NetStats::default());
        self.params = Some(p);
        Ok(())
    }

    fn status(&self, world: &World, out: &mut String) {
        match world.try_res::<NetStats>() {
            Some(s) if s.mode != NetMode::Idle => {
                let _ = write!(
                    out,
                    "{} · {} peers · {} bots · {} rollbacks",
                    s.mode.label(),
                    s.peers(),
                    s.server.bots,
                    s.client.rollbacks
                );
            }
            _ => out.push_str("idle"),
        }
        if let Some(p) = self.params {
            let _ = write!(
                out,
                " · {} Hz",
                world.res::<ParamRegistry>().i32(p.send_rate_hz)
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::App;

    #[test]
    fn registers_params() {
        let mut a = App::new("t").with_module(NetModule::new());
        a.run_headless(1, 16.0);
        assert!(
            a.world
                .res::<ParamRegistry>()
                .id("net.send_rate_hz")
                .is_some()
        );
    }

    /// The lockstep pacing settings are the same registered parameters as
    /// every other `net.*` value: defaults equal the shipped behaviour, the
    /// limits hold, and a change reaches the client's settings.
    #[cfg(feature = "lockstep")]
    #[test]
    fn lockstep_pacing_params_default_to_todays_values_and_apply() {
        use crate::lockstep::ClientConfig;
        use evani_core::params::ParamValue;

        let mut a = App::new("t").with_module(NetModule::new());
        a.run_headless(1, 16.0);
        assert_eq!(ClientConfig::from_world(&a.world), ClientConfig::default());
        assert_eq!(ClientConfig::default().input_delay, 0);
        assert_eq!(ClientConfig::default().max_catchup_steps, 0);

        // (The registry borrow ends before the settings are read.)
        let (delay, cap) = {
            let mut reg = a.world.res_mut::<ParamRegistry>();
            let delay = reg.id("net.input_delay_ticks").expect("registered");
            let cap = reg.id("net.max_catchup_steps").expect("registered");
            assert!(reg.set(delay, ParamValue::I32(4)).is_ok());
            assert!(reg.set(cap, ParamValue::I32(3)).is_ok());
            (delay, cap)
        };
        let c = ClientConfig::from_world(&a.world);
        assert_eq!((c.input_delay, c.max_catchup_steps), (4, 3));
        // Beyond the limits the value is kept inside them (1 counts as 2).
        {
            let mut reg = a.world.res_mut::<ParamRegistry>();
            let _ = reg.set(delay, ParamValue::I32(99));
            let _ = reg.set(cap, ParamValue::I32(1));
        }
        let c = ClientConfig::from_world(&a.world);
        assert_eq!((c.input_delay, c.max_catchup_steps), (6, 2));
        {
            let mut reg = a.world.res_mut::<ParamRegistry>();
            let _ = reg.set(cap, ParamValue::I32(0));
        }
        assert_eq!(ClientConfig::from_world(&a.world).max_catchup_steps, 0);
    }
}
