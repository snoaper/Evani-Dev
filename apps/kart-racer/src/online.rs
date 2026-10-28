//! Online play (the app's `net` feature): when a host hands the page a link
//! (`evani-net/1`: a parent frame's `hello`, or an in-page adapter's
//! `evaniNet.attachPort`), the game joins that host's session instead of
//! the title menu. With no host the game is exactly the single-player game.
//!
//! One fixed-tick system, before the kit's (which stand still online):
//! views from the host go to the lockstep client, this tick's controls (the
//! same keys, pad and touch as offline) go out as a 3-byte input, and the
//! client's predicted race (a spectator: the played one) is copied into the
//! game's world, seen from the player's own kart, for the kit's drawing,
//! sound and HUD. Every view and input is bytes; the host carries them.

use evani_core::ecs::World;
use evani_kit_kart_racer::kart::Controls;
use evani_kit_kart_racer::net::{KartInput, NetRace};
use evani_kit_kart_racer::race::{Phase, Race};
use evani_net::lockstep::{ClientConfig, LockstepClient, LockstepGame};
use evani_net::wire::Writer;

/// What the page's host link looks like to the game (the browser's
/// [`evani_net::port::HostPort`]; tests use an in-page loopback).
pub trait Link {
    /// `Some(spectator)` once a host has handed a link over.
    fn joined(&self) -> Option<bool>;
    /// The seat a spectator follows, if the host chose one.
    fn follow(&self) -> Option<u8>;
    fn recv(&mut self, out: &mut Vec<u8>) -> bool;
    fn send(&mut self, bytes: &[u8]);
    fn ready(&mut self, protocol: u32);
}

/// Send an input packet every this many ticks (20 a second: under the 30
/// a second some hosts allow).
pub const SEND_EVERY: u64 = 3;

/// The online session (a world resource while one runs or waits).
pub struct Online {
    link: Box<dyn Link>,
    client: Option<LockstepClient<NetRace>>,
    shown: u64,
    ticks: u64,
    buf: Vec<u8>,
    w: Writer,
    /// Bytes sent and received (the overlay's line).
    pub up: u64,
    pub down: u64,
    pub error: Option<String>,
}

impl Online {
    pub fn new(link: Box<dyn Link>) -> Self {
        Self {
            link,
            client: None,
            shown: 0,
            ticks: 0,
            buf: Vec::new(),
            w: Writer::new(),
            up: 0,
            down: 0,
            error: None,
        }
    }

    pub fn client(&self) -> Option<&LockstepClient<NetRace>> {
        self.client.as_ref()
    }

    /// One fixed tick.
    fn run(&mut self, world: &mut World) {
        if self.client.is_none() {
            let Some(spectator) = self.link.joined() else { return };
            let copies = (NetRace::new(0, &[]), NetRace::new(0, &[]));
            let (Ok(a), Ok(b)) = copies else {
                self.error = Some("online: the race rules did not load".into());
                return;
            };
            // `net.input_delay_ticks` is read here, once per session.
            let cfg = ClientConfig::from_world(world);
            self.client = Some(if spectator { LockstepClient::spectator(a, b, cfg) } else { LockstepClient::player(a, b, cfg) });
            self.link.ready(NetRace::PROTOCOL);
            mark_page_online("joining");
        }
        let Some(client) = self.client.as_mut() else { return };
        while self.link.recv(&mut self.buf) {
            self.down += self.buf.len() as u64;
            let _ = client.on_view(&self.buf);
        }
        let input = (!client.is_spectator()).then(|| {
            let c: Controls = evani_kit_kart_racer::local_controls(world).unwrap_or_default();
            KartInput::from_controls(&c)
        });
        // `net.max_catchup_steps` can change while playing.
        client.set_max_catchup_steps(ClientConfig::from_world(world).max_catchup_steps);
        client.tick(input);
        self.ticks += 1;
        if self.ticks.is_multiple_of(SEND_EVERY) {
            client.packet(&mut self.w);
            if !self.w.is_empty() {
                self.up += self.w.len() as u64;
                self.link.send(self.w.as_bytes());
            }
        }
        if self.ticks.is_multiple_of(60) {
            let s = client.stats();
            let role = if client.is_spectator() { "spectator" } else { "player" };
            let seat = client.seat().map_or(-1, i32::from);
            mark_page_online(&format!(
                "{role} seat={seat} tick={} confirmed={} hash_ok={} desyncs={} states={} rollbacks={} up={} down={}",
                client.display_tick(),
                client.confirmed_tick(),
                s.hash_ok,
                s.desyncs,
                s.states,
                s.rollbacks,
                self.up,
                self.down
            ));
        }
        if !client.synced() {
            return;
        }
        let follow = match client.seat() {
            Some(s) if !client.is_spectator() => Some(usize::from(s)),
            _ => Some(usize::from(self.link.follow().unwrap_or(0))),
        };
        match client.display().show(world, follow, self.shown) {
            Ok(t) => self.shown = t,
            Err(e) => self.error = Some(format!("online: view: {e}")),
        }
        // The touch controls belong to a seated player's race only.
        let racing = matches!(world.res::<Race>().phase, Phase::Countdown | Phase::Racing);
        evani_core::touch::set_enabled(world, racing && client.seat().is_some() && !client.is_spectator());
    }
}

/// `kart_racer.online` (fixed `Update`, before the kit's systems).
pub fn online_system(world: &mut World) {
    let Some(mut st) = world.remove_resource::<Online>() else { return };
    st.run(world);
    if let Some(e) = st.error.take() {
        world.res_mut::<evani_core::overlay::ErrorLog>().note(e);
    }
    world.insert_resource(st);
}

/// `html[data-online]`: the page's menus that make no sense online
/// (restart, quit to the title) are hidden by the stylesheet, and the value
/// is a one-line status (seat, ticks, hash checks, desyncs, bytes) a host
/// or a test can read.
fn mark_page_online(status: &str) {
    #[cfg(target_arch = "wasm32")]
    if let Some(el) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.document_element()) {
        let _ = el.set_attribute("data-online", status);
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = status;
}

/// The browser's host link.
#[cfg(target_arch = "wasm32")]
pub struct PageLink(pub evani_net::port::HostPort);

#[cfg(target_arch = "wasm32")]
impl Link for PageLink {
    fn joined(&self) -> Option<bool> {
        use evani_net::port::{PortRole, PortStatus};
        match (self.0.status(), self.0.role()) {
            (PortStatus::Waiting, _) | (_, None) => None,
            (_, Some(r)) => Some(r == PortRole::Spectator),
        }
    }
    fn follow(&self) -> Option<u8> {
        self.0.follow()
    }
    fn recv(&mut self, out: &mut Vec<u8>) -> bool {
        self.0.recv(out).is_some()
    }
    fn send(&mut self, bytes: &[u8]) {
        self.0.send(bytes);
    }
    fn ready(&mut self, protocol: u32) {
        self.0.ready(protocol);
    }
}

/// The engine module: registers the system and, in the browser, waits for a
/// host link (none comes: the game stays single-player).
pub struct OnlineModule {
    link: Option<Box<dyn Link>>,
}

impl OnlineModule {
    /// In the browser: the page's host link. Natively (tests): none until a
    /// test inserts [`Online`] with its own link.
    pub fn new() -> Self {
        #[cfg(target_arch = "wasm32")]
        let link: Option<Box<dyn Link>> = Some(Box::new(PageLink(evani_net::port::HostPort)));
        #[cfg(not(target_arch = "wasm32"))]
        let link: Option<Box<dyn Link>> = None;
        Self { link }
    }
}

impl evani_core::plugin::EngineModule for OnlineModule {
    fn id(&self) -> &'static str {
        "evani.module.kart_online"
    }

    fn label(&self) -> &'static str {
        "online"
    }

    fn build(&mut self, ctx: &mut evani_core::plugin::PluginContext) -> Result<(), evani_core::plugin::PluginError> {
        ctx.system(evani_core::ecs::SystemDesc::new("kart_racer.online", evani_core::ecs::Stage::Update, online_system).order(-10));
        if let Some(link) = self.link.take() {
            ctx.resource(Online::new(link));
        }
        Ok(())
    }

    fn status(&self, world: &World, out: &mut String) {
        use core::fmt::Write as _;
        match world.try_res::<Online>().as_ref().and_then(|o| o.client().map(|c| (c.stats(), c.seat(), o.up, o.down))) {
            Some((s, seat, up, down)) => {
                let _ = write!(out, "seat {seat:?} · tick {} · rollbacks {} · desyncs {} · up {up} B · down {down} B", s.frames, s.rollbacks, s.desyncs);
            }
            None => out.push_str("offline"),
        }
    }
}
