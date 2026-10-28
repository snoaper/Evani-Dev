//! How packets move. [`Transport`] is the one interface the server and
//! client use; [`Loopback`] is the in-page transport for offline play and
//! bots, so they run the same encode, send, receive and decode path an
//! online game will.
//!
//! The loopback keeps its own tick clock (advanced once per sim tick by the
//! session), so its delay, jitter and loss are deterministic: the same seed
//! and conditions drop and reorder the same packets every run.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use evani_core::rng::Pcg32;

use crate::types::{LinkId, Tick};

/// An unreliable datagram transport: like UDP or an unordered, unreliable
/// WebRTC data channel, a packet may be dropped, delayed or reordered, and
/// the netcode copes (inputs are sent redundantly, snapshots are whole).
/// A reliable transport (a WebSocket) works too; it just never loses one.
///
/// The server and client hold it as `Box<dyn Transport>`, so their code is
/// compiled once whatever the link.
///
/// Packet sizes: a snapshot is the whole encoded state. A transport with a
/// message limit splits and joins packets itself.
pub trait Transport {
    /// Send one packet. On a client end, `to` is ignored: it goes to the
    /// server.
    fn send(&mut self, to: LinkId, packet: &[u8]);
    /// The next packet that has arrived, into `buf` (replaced), and who sent
    /// it. `None` when nothing is waiting.
    fn recv(&mut self, buf: &mut Vec<u8>) -> Option<LinkId>;
    /// Server end: connections closed since the last call, into `out`.
    fn closed(&mut self, _out: &mut Vec<LinkId>) {}
}

/// Simulated network conditions for the loopback (all zero: a perfect,
/// instant link). One-way values, per packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LinkConditions {
    /// Fixed delay in ticks.
    pub latency_ticks: u32,
    /// Extra random delay of 0 to this many ticks (reorders packets).
    pub jitter_ticks: u32,
    /// Packets dropped per thousand.
    pub loss_permille: u32,
}

/// Loopback totals, for the overlay and tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LoopbackStats {
    pub sent: u64,
    pub delivered: u64,
    pub dropped: u64,
    pub bytes: u64,
    pub in_flight: usize,
}

/// A packet in flight: when it is due, who sent it, and its bytes.
struct Packet {
    at: Tick,
    from: LinkId,
    bytes: Vec<u8>,
}

/// One destination's packets in flight, sorted by due tick; packets due on
/// the same tick keep their send order.
type Inbox = VecDeque<Packet>;

struct Hub {
    now: Tick,
    cond: LinkConditions,
    rng: Pcg32,
    next_link: u32,
    /// Every open link's inbox, sorted by link (links are numbered upwards,
    /// so a new one is always pushed at the end).
    inboxes: Vec<(LinkId, Inbox)>,
    closed: Vec<LinkId>,
    spare: Vec<Vec<u8>>,
    stats: LoopbackStats,
}

impl Hub {
    fn find(&self, link: LinkId) -> Option<usize> {
        self.inboxes.binary_search_by_key(&link, |e| e.0).ok()
    }

    fn post(&mut self, from: LinkId, to: LinkId, packet: &[u8]) {
        self.stats.sent += 1;
        let Some(slot) = self.find(to) else {
            // Closed or unknown link: lost, as on a real network.
            self.stats.dropped += 1;
            return;
        };
        if self.cond.loss_permille > 0 && self.rng.below(1000) < self.cond.loss_permille {
            self.stats.dropped += 1;
            return;
        }
        let jitter = if self.cond.jitter_ticks > 0 {
            self.rng.below(self.cond.jitter_ticks.saturating_add(1))
        } else {
            0
        };
        let at = self
            .now
            .saturating_add(self.cond.latency_ticks)
            .saturating_add(jitter);
        let mut bytes = self.spare.pop().unwrap_or_default();
        bytes.clear();
        bytes.extend_from_slice(packet);
        self.stats.bytes += packet.len() as u64;
        self.stats.in_flight += 1;
        let Some((_, inbox)) = self.inboxes.get_mut(slot) else {
            return;
        };
        // After every packet due at or before `at`: same-tick packets stay in
        // send order.
        let pos = inbox.partition_point(|p| p.at <= at);
        inbox.insert(pos, Packet { at, from, bytes });
    }

    fn take(&mut self, me: LinkId, buf: &mut Vec<u8>) -> Option<LinkId> {
        let now = self.now;
        let slot = self.find(me)?;
        let (_, inbox) = self.inboxes.get_mut(slot)?;
        if inbox.front().is_none_or(|p| p.at > now) {
            return None;
        }
        let Packet {
            from, mut bytes, ..
        } = inbox.pop_front()?;
        core::mem::swap(buf, &mut bytes);
        if self.spare.len() < 64 {
            self.spare.push(bytes);
        }
        self.stats.delivered += 1;
        self.stats.in_flight = self.stats.in_flight.saturating_sub(1);
        Some(from)
    }

    fn close(&mut self, link: LinkId) {
        if let Some(slot) = self.find(link) {
            let (_, inbox) = self.inboxes.remove(slot);
            self.stats.in_flight = self.stats.in_flight.saturating_sub(inbox.len());
        }
        if link != LinkId::SERVER {
            self.closed.push(link);
        }
    }
}

/// The in-page network: one server end and any number of client ends,
/// joined by queues. Single-threaded (wasm runs one thread), shared through
/// `Rc`.
#[derive(Clone)]
pub struct Loopback {
    hub: Rc<RefCell<Hub>>,
}

impl Loopback {
    /// A loopback with these conditions; `seed` drives loss and jitter.
    pub fn new(conditions: LinkConditions, seed: u64) -> Self {
        Self {
            hub: Rc::new(RefCell::new(Hub {
                now: 0,
                cond: conditions,
                rng: Pcg32::new(seed, 0x6c6f_6f70),
                next_link: 1,
                inboxes: vec![(LinkId::SERVER, VecDeque::new())],
                closed: Vec::new(),
                spare: Vec::new(),
                stats: LoopbackStats::default(),
            })),
        }
    }

    /// The server's end.
    pub fn server(&self) -> LoopbackEnd {
        LoopbackEnd {
            hub: Rc::clone(&self.hub),
            me: LinkId::SERVER,
        }
    }

    /// A new client connection.
    pub fn connect(&self) -> LoopbackEnd {
        let mut hub = self.hub.borrow_mut();
        let me = LinkId(hub.next_link);
        hub.next_link = hub.next_link.saturating_add(1);
        hub.inboxes.push((me, VecDeque::new()));
        LoopbackEnd {
            hub: Rc::clone(&self.hub),
            me,
        }
    }

    /// One tick of loopback time: packets due now become receivable.
    pub fn advance(&self) {
        let mut hub = self.hub.borrow_mut();
        hub.now = hub.now.saturating_add(1);
    }

    pub fn now(&self) -> Tick {
        self.hub.borrow().now
    }

    pub fn set_conditions(&self, conditions: LinkConditions) {
        self.hub.borrow_mut().cond = conditions;
    }

    pub fn conditions(&self) -> LinkConditions {
        self.hub.borrow().cond
    }

    pub fn stats(&self) -> LoopbackStats {
        self.hub.borrow().stats
    }
}

/// One end of a [`Loopback`]. Dropping a client end closes its connection
/// (the server sees it in [`Transport::closed`]).
pub struct LoopbackEnd {
    hub: Rc<RefCell<Hub>>,
    me: LinkId,
}

impl LoopbackEnd {
    pub fn link(&self) -> LinkId {
        self.me
    }
}

impl Transport for LoopbackEnd {
    fn send(&mut self, to: LinkId, packet: &[u8]) {
        let to = if self.me == LinkId::SERVER {
            to
        } else {
            LinkId::SERVER
        };
        self.hub.borrow_mut().post(self.me, to, packet);
    }

    fn recv(&mut self, buf: &mut Vec<u8>) -> Option<LinkId> {
        self.hub.borrow_mut().take(self.me, buf)
    }

    fn closed(&mut self, out: &mut Vec<LinkId>) {
        if self.me == LinkId::SERVER {
            out.append(&mut self.hub.borrow_mut().closed);
        }
    }
}

impl Drop for LoopbackEnd {
    fn drop(&mut self) {
        if let Ok(mut hub) = self.hub.try_borrow_mut() {
            hub.close(self.me);
        }
    }
}
