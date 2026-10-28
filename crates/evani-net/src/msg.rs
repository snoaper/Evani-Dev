//! The packets the server and clients exchange. Every packet starts with one
//! [`Kind`] byte. Layouts (varint = LEB128, zigzag = signed varint):
//!
//! Client to server:
//! * `Hello` 0x10: wire version (u16 varint), protocol hash (fixed u64,
//!   `fnv1a64(G::PROTOCOL)`). Resent until a `Welcome` or `Refuse` arrives.
//! * `Input` 0x11: [`InputHeader`] (ack snapshot, newest tick, frame count),
//!   then up to [`MAX_INPUT_FRAMES`] frames, newest first, on consecutive
//!   ticks. A frame is the view time, then the game's input. The view time
//!   is how far behind the frame's tick the player was looking, in
//!   sub-ticks ([`ViewCodec`]): a varint on the first frame, then a zigzag
//!   change from the frame before (usually 0: one byte). Sent every tick
//!   with the last few frames repeated, so a lost packet costs nothing.
//! * `Bye` 0x12: leaving.
//!
//! Server to client:
//! * `Welcome` 0x20: peer id, server tick, session seed (fixed u64), ticks
//!   between snapshots.
//! * `Snapshot` 0x21: [`SnapshotHeader`] (tick, ack input, slack, baseline),
//!   then the body: the last input of every peer (varint count, then peer
//!   id and input each), then the whole state. With no baseline the body is
//!   plain; a baseline tick is reserved for a delta body against the body
//!   of that earlier snapshot (one the client acked), which this version
//!   never sends and drops when it arrives.
//! * `Refuse` 0x22: a [`RefuseReason`] byte.
//! * `Bye` 0x23: the server dropped this client (timeout) or is closing.
//!
//! Sizes (payload only; the transport's own headers, about 30 to 60 bytes
//! on UDP or WebRTC, come on top). Ticks are 3-byte varints after the first
//! 4.5 minutes.
//! * Input: about 8 bytes of header, then per frame 1 to 2 bytes of view
//!   time plus the input. A shooter input of about 10 bytes with 4 frames
//!   is about 50 bytes; at 60 Hz about 3 KB/s up per player.
//! * Snapshot: about 8 bytes of header, about 11 bytes per peer for its
//!   last input (id plus a 10-byte input), then the state. Ten players with
//!   exact `f32` fields (about 40 bytes each) come to about 540 bytes; at
//!   20 Hz about 11 KB/s down per player. Millimetre `i32` or
//!   [`quantize`](crate::wire::quantize)d fields halve the state; the
//!   reserved delta body would bring a typical snapshot to about 60 to 150
//!   bytes.

use crate::types::{PeerId, Tick, TickTime};
use crate::wire::{Reader, Wire, WireError, Writer};

/// Version of the packet layouts in this file. A peer with another version
/// is refused in the handshake.
pub const WIRE_VERSION: u16 = 1;

/// Most input frames one `Input` packet carries.
pub const MAX_INPUT_FRAMES: usize = 32;

/// The first byte of every packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Kind {
    Hello = 0x10,
    Input = 0x11,
    ClientBye = 0x12,
    Welcome = 0x20,
    Snapshot = 0x21,
    Refuse = 0x22,
    ServerBye = 0x23,
}

impl Kind {
    pub fn from_u8(b: u8) -> Option<Self> {
        Some(match b {
            0x10 => Kind::Hello,
            0x11 => Kind::Input,
            0x12 => Kind::ClientBye,
            0x20 => Kind::Welcome,
            0x21 => Kind::Snapshot,
            0x22 => Kind::Refuse,
            0x23 => Kind::ServerBye,
            _ => return None,
        })
    }

    /// Read the kind byte that starts a packet.
    pub fn read(r: &mut Reader<'_>) -> Result<Self, WireError> {
        Self::from_u8(r.u8()?).ok_or(WireError::Invalid)
    }

    pub fn write(self, w: &mut Writer) {
        w.u8(self as u8);
    }
}

/// `Option<Tick>` as one varint: 0 for none, else tick + 1.
pub fn write_opt_tick(w: &mut Writer, t: Option<Tick>) {
    w.varint(t.map_or(0, |t| u64::from(t) + 1));
}

pub fn read_opt_tick(r: &mut Reader<'_>) -> Result<Option<Tick>, WireError> {
    let v = r.varint()?;
    if v == 0 {
        return Ok(None);
    }
    Tick::try_from(v - 1)
        .map(Some)
        .map_err(|_| WireError::Overflow)
}

/// The client's greeting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hello {
    pub wire: u16,
    pub protocol: u64,
}

impl Hello {
    /// A hello for this build and the game's protocol name.
    pub fn new(protocol_hash: u64) -> Self {
        Self {
            wire: WIRE_VERSION,
            protocol: protocol_hash,
        }
    }

    pub fn encode(&self, w: &mut Writer) {
        Kind::Hello.write(w);
        w.varint(u64::from(self.wire));
        w.u64_fixed(self.protocol);
    }

    /// Read the body (after the kind byte).
    pub fn decode(r: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self {
            wire: r.get()?,
            protocol: r.u64_fixed()?,
        })
    }
}

/// The front of an `Input` packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputHeader {
    /// The newest snapshot the client has (for delta snapshots later).
    pub ack_snapshot: Option<Tick>,
    /// The tick of the first (newest) frame.
    pub newest: Tick,
    /// Frames that follow, newest first: ticks `newest`, `newest - 1`, ...
    pub count: u8,
}

impl InputHeader {
    pub fn encode(&self, w: &mut Writer) {
        Kind::Input.write(w);
        write_opt_tick(w, self.ack_snapshot);
        w.varint(u64::from(self.newest));
        w.u8(self.count);
    }

    /// Read the header (after the kind byte). A count over
    /// [`MAX_INPUT_FRAMES`] is refused.
    pub fn decode(r: &mut Reader<'_>) -> Result<Self, WireError> {
        let ack_snapshot = read_opt_tick(r)?;
        let newest = r.varint_u32()?;
        let count = r.u8()?;
        if usize::from(count) > MAX_INPUT_FRAMES {
            return Err(WireError::TooLong);
        }
        Ok(Self {
            ack_snapshot,
            newest,
            count,
        })
    }

    /// The tick of frame `i` (0 is the newest); `None` before tick 0.
    pub fn frame_tick(&self, i: u8) -> Option<Tick> {
        self.newest.checked_sub(u32::from(i))
    }
}

/// The view time of each frame in one `Input` packet. A frame's view is
/// stored as how far behind its tick the player was looking ("back", in
/// sub-ticks): the first frame writes it whole, each later frame only the
/// change from the frame before. Back is steady while the interpolation
/// delay is, so later frames cost one byte. Use a fresh codec per packet;
/// the game's input is written after each view.
#[derive(Debug, Clone, Copy, Default)]
pub struct ViewCodec {
    prev: Option<u64>,
}

impl ViewCodec {
    pub fn new() -> Self {
        Self::default()
    }

    /// Write the view time of the frame for `tick`.
    pub fn write(&mut self, w: &mut Writer, tick: Tick, view: TickTime) {
        let back = TickTime::at(tick).0.saturating_sub(view.0);
        match self.prev {
            None => w.varint(back),
            Some(prev) => {
                let change = i64::try_from(back)
                    .unwrap_or(i64::MAX)
                    .saturating_sub(i64::try_from(prev).unwrap_or(i64::MAX));
                w.zigzag(change);
            }
        }
        self.prev = Some(back);
    }

    /// Read the view time of the frame for `tick`. A change that would put
    /// the view after its tick is refused.
    pub fn read(&mut self, r: &mut Reader<'_>, tick: Tick) -> Result<TickTime, WireError> {
        let back = match self.prev {
            None => r.varint()?,
            Some(prev) => prev
                .checked_add_signed(r.zigzag()?)
                .ok_or(WireError::Invalid)?,
        };
        self.prev = Some(back);
        Ok(TickTime::at(tick).sub_subs(back))
    }
}

/// The server's answer to a hello.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Welcome {
    pub peer: PeerId,
    /// The server's tick when it sent this.
    pub tick: Tick,
    pub seed: u64,
    pub snapshot_every: u32,
}

impl Welcome {
    pub fn encode(&self, w: &mut Writer) {
        Kind::Welcome.write(w);
        self.peer.write(w);
        w.varint(u64::from(self.tick));
        w.u64_fixed(self.seed);
        w.varint(u64::from(self.snapshot_every));
    }

    pub fn decode(r: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self {
            peer: r.get()?,
            tick: r.varint_u32()?,
            seed: r.u64_fixed()?,
            snapshot_every: r.varint_u32()?.max(1),
        })
    }
}

/// The front of a `Snapshot` packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotHeader {
    /// The state that follows is at this tick (after `tick` steps).
    pub tick: Tick,
    /// The newest input tick the server has from this client.
    pub ack_input: Option<Tick>,
    /// How many ticks early this client's inputs arrive (the smallest
    /// since the last snapshot): below the target the client runs a tick
    /// ahead, above it the client waits a tick.
    pub slack: i32,
    /// `None`: the body is plain. A tick is reserved for a delta body (the
    /// body XORed with that earlier snapshot's body, zero runs packed),
    /// built from the client's [`InputHeader::ack_snapshot`]. This version
    /// always sends `None`, and a client drops a snapshot with a baseline.
    pub baseline: Option<Tick>,
}

impl SnapshotHeader {
    pub fn encode(&self, w: &mut Writer) {
        Kind::Snapshot.write(w);
        w.varint(u64::from(self.tick));
        write_opt_tick(w, self.ack_input);
        w.zigzag(i64::from(self.slack));
        write_opt_tick(w, self.baseline);
    }

    pub fn decode(r: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self {
            tick: r.varint_u32()?,
            ack_input: read_opt_tick(r)?,
            slack: r.get()?,
            baseline: read_opt_tick(r)?,
        })
    }
}

/// The last input of every peer, as a snapshot carries it.
pub fn write_peer_inputs<'a, I: Wire + 'a>(
    w: &mut Writer,
    inputs: impl ExactSizeIterator<Item = (PeerId, &'a I)>,
) {
    w.varint(inputs.len() as u64);
    for (peer, input) in inputs {
        peer.write(w);
        input.write(w);
    }
}

/// Read the peer input list into `out` (replaced), sorted by peer.
pub fn read_peer_inputs<I: Wire>(
    r: &mut Reader<'_>,
    out: &mut Vec<(PeerId, I)>,
) -> Result<(), WireError> {
    out.clear();
    // A peer id is at least one byte.
    let n = r.list_len(1)?;
    for _ in 0..n {
        let peer = PeerId::read(r)?;
        let input = I::read(r)?;
        out.push((peer, input));
    }
    out.sort_by_key(|p| p.0);
    Ok(())
}

/// Why the server turned a client away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RefuseReason {
    /// Another wire version or game protocol.
    Protocol = 1,
    /// No free player slot.
    Full = 2,
}

impl RefuseReason {
    pub fn encode(self, w: &mut Writer) {
        Kind::Refuse.write(w);
        w.u8(self as u8);
    }

    pub fn decode(r: &mut Reader<'_>) -> Result<Self, WireError> {
        match r.u8()? {
            1 => Ok(RefuseReason::Protocol),
            2 => Ok(RefuseReason::Full),
            _ => Err(WireError::Invalid),
        }
    }
}
