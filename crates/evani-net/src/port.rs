//! The page side of `evani-net/1` (feature `port`, wasm only): bytes to and
//! from the host over the one `MessagePort` the host page (a parent frame's
//! `hello`) or an in-page adapter (`globalThis.evaniNet.attachPort(port,
//! opts)`) hands over. The game opens no network of its own. `web/port.js`
//! has the message contract; `docs/NET_BRIDGE_GUIDE.md` the whole of it.
//!
//! [`HostPort`] is a plain byte pipe (datagrams and reliable messages), so a
//! game that uses its own protocol (a JSON rules module, say) can use it
//! without any of the netcode; it also implements [`Transport`].

use wasm_bindgen::prelude::*;

use crate::transport::Transport;
use crate::types::LinkId;

#[wasm_bindgen(module = "/web/port.js")]
extern "C" {
    fn port_status() -> String;
    fn port_role() -> String;
    fn port_follow() -> i32;
    fn port_paused() -> bool;
    fn port_max_send() -> f64;
    fn port_recv() -> Option<js_sys::Uint8Array>;
    fn port_last_reliable() -> bool;
    fn port_send(bytes: &[u8], reliable: bool) -> bool;
    fn port_ready(protocol: u32);
    fn port_leave();
}

/// Where the link to the host stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortStatus {
    /// No host has handed a port over (yet): play single-player.
    Waiting,
    Live,
    /// The host says its own connection is down for now.
    Reconnecting,
    Closed,
}

/// What the host made this page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortRole {
    Player,
    Spectator,
}

/// The host link (one per page). Cheap to make: it holds nothing.
#[derive(Debug, Default, Clone, Copy)]
pub struct HostPort;

impl HostPort {
    pub fn status(&self) -> PortStatus {
        match port_status().as_str() {
            "live" => PortStatus::Live,
            "reconnecting" => PortStatus::Reconnecting,
            "closed" => PortStatus::Closed,
            _ => PortStatus::Waiting,
        }
    }

    /// The role the host gave (None before a port arrives).
    pub fn role(&self) -> Option<PortRole> {
        match port_role().as_str() {
            "player" => Some(PortRole::Player),
            "spectator" => Some(PortRole::Spectator),
            _ => None,
        }
    }

    /// The seat a spectator follows, if the host chose one.
    pub fn follow(&self) -> Option<u8> {
        u8::try_from(port_follow()).ok()
    }

    /// The host paused the game (its page is hidden, say).
    pub fn paused(&self) -> bool {
        port_paused()
    }

    /// The host's send cap (messages a second), if it gave one.
    pub fn max_send_per_sec(&self) -> Option<f32> {
        let v = port_max_send();
        (v > 0.0).then_some(v as f32)
    }

    /// The next message from the host into `out`; `Some(reliable)` when
    /// there was one.
    pub fn recv(&self, out: &mut Vec<u8>) -> Option<bool> {
        let b = port_recv()?;
        out.clear();
        out.resize(b.length() as usize, 0);
        b.copy_to(out);
        Some(port_last_reliable())
    }

    /// A datagram to the host (the host may drop or reorder it). False when
    /// there is no live port or it is over the host's size cap.
    pub fn send(&self, bytes: &[u8]) -> bool {
        port_send(bytes, false)
    }

    /// A reliable, ordered message to the host.
    pub fn send_reliable(&self, bytes: &[u8]) -> bool {
        port_send(bytes, true)
    }

    /// Tell the host which game (and rules version) is talking.
    pub fn ready(&self, protocol: u32) {
        port_ready(protocol);
    }

    /// Tell the host the player left (back to the menu).
    pub fn leave(&self) {
        port_leave();
    }
}

impl Transport for HostPort {
    fn send(&mut self, _to: LinkId, packet: &[u8]) {
        HostPort::send(self, packet);
    }

    fn recv(&mut self, buf: &mut Vec<u8>) -> Option<LinkId> {
        HostPort::recv(self, buf).map(|_| LinkId::SERVER)
    }
}
