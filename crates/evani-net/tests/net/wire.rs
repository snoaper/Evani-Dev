//! Encoding: every wire type reads back exactly what it wrote, range and
//! length limits refuse bad values, and no byte string makes a decoder
//! panic.

use evani_core::rng::Pcg32;
use evani_net::msg::{
    self, Hello, InputHeader, Kind, MAX_INPUT_FRAMES, RefuseReason, SnapshotHeader, ViewCodec,
    WIRE_VERSION, Welcome,
};
use evani_net::wire::{Reader, Wire, WireError, Writer, dequantize, fnv1a64, quantize};
use evani_net::{PeerId, TickTime};

use crate::support::{ArenaInput, ArenaState, Player};

fn round_trip<T: Wire + PartialEq + core::fmt::Debug>(v: &T) -> usize {
    let mut w = Writer::new();
    v.write(&mut w);
    let mut r = Reader::new(w.as_bytes());
    let back = match T::read(&mut r) {
        Ok(b) => b,
        Err(e) => panic!("{v:?} did not read back: {e}"),
    };
    assert_eq!(&back, v);
    assert!(r.is_empty(), "{v:?}: read left {} bytes", r.remaining());
    w.len()
}

fn read_bytes<T: Wire>(bytes: &[u8]) -> Result<T, WireError> {
    T::read(&mut Reader::new(bytes))
}

#[test]
fn integers_round_trip_at_their_limits() {
    for v in [
        0u64,
        1,
        127,
        128,
        16_383,
        16_384,
        u64::from(u32::MAX),
        u64::MAX - 1,
        u64::MAX,
    ] {
        round_trip(&v);
    }
    for v in [0u32, 1, 300, u32::MAX] {
        round_trip(&v);
    }
    for v in [0u16, 1, 255, u16::MAX] {
        round_trip(&v);
    }
    for v in [0u8, 1, 0x7f, 0x80, u8::MAX] {
        round_trip(&v);
    }
    for v in [0i64, -1, 1, -64, 63, -65, 64, i64::MIN, i64::MAX] {
        round_trip(&v);
    }
    for v in [i32::MIN, -1, 0, 1, i32::MAX] {
        round_trip(&v);
    }
    for v in [i16::MIN, 0, i16::MAX] {
        round_trip(&v);
    }
    for v in [i8::MIN, -1, 0, 1, i8::MAX] {
        round_trip(&v);
    }
}

#[test]
fn varints_are_short_for_small_numbers() {
    assert_eq!(round_trip(&0u32), 1);
    assert_eq!(round_trip(&127u32), 1);
    assert_eq!(round_trip(&128u32), 2);
    assert_eq!(round_trip(&u64::MAX), 10);
    // Zigzag keeps small negatives small.
    assert_eq!(round_trip(&-1i32), 1);
    assert_eq!(round_trip(&-64i32), 1);
    assert_eq!(round_trip(&-65i32), 2);
}

#[test]
fn floats_keep_their_exact_bits() {
    let specials = [
        0.0f32,
        -0.0,
        1.0,
        -1.5,
        f32::MIN_POSITIVE,
        f32::from_bits(1), // smallest subnormal
        f32::MAX,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::from_bits(0x7fc0_0001), // a NaN with a payload
        f32::from_bits(0xffa0_0000), // a negative signalling-pattern NaN
    ];
    for v in specials {
        let mut w = Writer::new();
        v.write(&mut w);
        assert_eq!(w.len(), 4);
        let back: f32 = read_bytes(w.as_bytes()).unwrap();
        assert_eq!(back.to_bits(), v.to_bits(), "{v:?}");
    }
    for v in [
        0.0f64,
        -0.0,
        f64::from_bits(0x7ff8_0000_0000_0042),
        f64::MAX,
        1e-310,
    ] {
        let mut w = Writer::new();
        v.write(&mut w);
        let back: f64 = read_bytes(w.as_bytes()).unwrap();
        assert_eq!(back.to_bits(), v.to_bits());
    }
}

#[test]
fn containers_round_trip() {
    round_trip(&String::new());
    round_trip(&"Sugarpeak — 8 karts".to_string());
    round_trip(&Vec::<u32>::new());
    round_trip(&vec![1u32, 2, 3, u32::MAX]);
    round_trip(&Some(5u16));
    round_trip(&None::<u16>);
    round_trip(&[1i8, -2, 3]);
    round_trip(&(1u8, -2i32));
    round_trip(&(1u8, -2i32, 3u64));
    round_trip(&(1u8, -2i32, 3u64, true));
    round_trip(&true);
    round_trip(&false);
}

#[test]
fn game_types_round_trip() {
    round_trip(&ArenaInput::new(-1, 1, 0xff));
    round_trip(&ArenaInput::new(i8::MIN, i8::MAX, 0));
    let s = ArenaState {
        tick: 1234,
        players: vec![Player {
            peer: 3,
            x: -19.5,
            y: 0.25,
            vx: f32::from_bits(1),
            vy: -0.0,
            score: 7,
            prev_buttons: 1,
            shots: 2,
            last_rewind: 99 * 256 + 17,
        }],
        pickups: vec![(1.0, 2.0), (-3.5, 4.25)],
        spawned: 9,
    };
    let mut w = Writer::new();
    s.write(&mut w);
    let back: ArenaState = read_bytes(w.as_bytes()).unwrap();
    let mut w2 = Writer::new();
    back.write(&mut w2);
    assert_eq!(
        w.as_bytes(),
        w2.as_bytes(),
        "the state reads back to the same bytes"
    );
}

#[test]
fn out_of_range_values_are_refused() {
    let mut w = Writer::new();
    w.varint(300);
    assert_eq!(read_bytes::<i8>(w.as_bytes()), Err(WireError::Overflow));
    assert_eq!(read_bytes::<u16>(w.as_bytes()).ok(), Some(300));
    w.clear();
    w.varint(70_000);
    assert_eq!(read_bytes::<u16>(w.as_bytes()), Err(WireError::Overflow));
    w.clear();
    w.varint(u64::from(u32::MAX) + 1);
    assert_eq!(read_bytes::<u32>(w.as_bytes()), Err(WireError::Overflow));
    w.clear();
    w.zigzag(i64::from(i16::MIN) - 1);
    assert_eq!(read_bytes::<i16>(w.as_bytes()), Err(WireError::Overflow));
    // An input whose axis does not fit an i8 is refused whole.
    w.clear();
    w.zigzag(200);
    w.zigzag(0);
    w.u8(0);
    assert_eq!(
        read_bytes::<ArenaInput>(w.as_bytes()),
        Err(WireError::Overflow)
    );
    // bool, Option tags and text must be exact.
    assert_eq!(read_bytes::<bool>(&[2]), Err(WireError::Invalid));
    assert_eq!(read_bytes::<Option<u8>>(&[2, 0]), Err(WireError::Invalid));
    assert_eq!(
        read_bytes::<String>(&[2, 0xc3, 0x28]),
        Err(WireError::Invalid)
    );
}

#[test]
fn varint_overflow_and_truncation_are_errors() {
    // Eleven bytes, or a tenth byte above 1, do not fit 64 bits.
    let long = [0xffu8; 11];
    assert_eq!(read_bytes::<u64>(&long), Err(WireError::Overflow));
    let mut ten = [0xffu8; 10];
    ten[9] = 0x02;
    assert_eq!(read_bytes::<u64>(&ten), Err(WireError::Overflow));
    ten[9] = 0x01;
    assert_eq!(read_bytes::<u64>(&ten), Ok(u64::MAX));
    // Ends too early.
    assert_eq!(read_bytes::<u64>(&[0x80]), Err(WireError::Truncated));
    assert_eq!(read_bytes::<f32>(&[0, 0, 0]), Err(WireError::Truncated));
    assert_eq!(read_bytes::<f64>(&[0; 7]), Err(WireError::Truncated));
    assert_eq!(read_bytes::<u8>(&[]), Err(WireError::Truncated));
    assert_eq!(read_bytes::<String>(&[5, b'a']), Err(WireError::TooLong));
    assert_eq!(read_bytes::<(u8, u8)>(&[1]), Err(WireError::Truncated));
}

#[test]
fn a_forged_list_length_never_allocates() {
    // Claims four billion items in a 6-byte packet.
    let mut w = Writer::new();
    w.varint(u64::from(u32::MAX));
    w.u8(0);
    assert_eq!(
        read_bytes::<Vec<u32>>(w.as_bytes()),
        Err(WireError::TooLong)
    );
    assert_eq!(
        read_bytes::<Vec<Player>>(w.as_bytes()),
        Err(WireError::TooLong)
    );
    let mut r = Reader::new(w.as_bytes());
    assert_eq!(r.list_len(1), Err(WireError::TooLong));
}

#[test]
fn quantize_round_trips_within_one_step() {
    for bits in [1u32, 8, 12, 16, 24] {
        let steps = ((1u32 << bits) - 1) as f32;
        let step = 40.0 / steps;
        for i in 0..=200 {
            let v = -20.0 + 40.0 * i as f32 / 200.0;
            let q = quantize(v, -20.0, 20.0, bits);
            assert!(q < (1u32 << bits));
            let back = dequantize(q, -20.0, 20.0, bits);
            assert!(
                (back - v).abs() <= step * 0.5 + 1e-4,
                "bits {bits}: {v} -> {back}"
            );
        }
    }
    // Clamped, NaN to the minimum, an empty range to 0, bits clamped.
    assert_eq!(quantize(100.0, 0.0, 1.0, 8), 255);
    assert_eq!(quantize(-100.0, 0.0, 1.0, 8), 0);
    assert_eq!(quantize(f32::NAN, 0.0, 1.0, 8), 0);
    assert_eq!(quantize(0.5, 1.0, 1.0, 8), 0);
    assert_eq!(quantize(0.5, 0.0, f32::NAN, 8), 0);
    assert_eq!(quantize(1.0, 0.0, 1.0, 99), (1 << 24) - 1);
    assert_eq!(dequantize(u32::MAX, 0.0, 1.0, 8), 1.0);
    // The same value always gives the same code (both sides simulate it).
    assert_eq!(
        quantize(0.123_456, -1.0, 1.0, 16),
        quantize(0.123_456, -1.0, 1.0, 16)
    );
}

#[test]
fn fnv1a64_matches_the_published_vectors() {
    // FNV-1a 64-bit reference values (offset basis, "a", "foobar").
    assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
    assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
    assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_f739_67e8);
}

#[test]
fn packet_headers_round_trip() {
    let mut w = Writer::new();
    let hello = Hello::new(0x1234_5678_9abc_def0);
    hello.encode(&mut w);
    let mut r = Reader::new(w.as_bytes());
    assert_eq!(Kind::read(&mut r), Ok(Kind::Hello));
    assert_eq!(Hello::decode(&mut r), Ok(hello));
    assert_eq!(hello.wire, WIRE_VERSION);

    for (ack, newest, count) in [
        (None, 0u32, 0u8),
        (Some(0), 5, 1),
        (Some(u32::MAX - 1), u32::MAX, 32),
    ] {
        w.clear();
        let h = InputHeader {
            ack_snapshot: ack,
            newest,
            count,
        };
        h.encode(&mut w);
        let mut r = Reader::new(w.as_bytes());
        assert_eq!(Kind::read(&mut r), Ok(Kind::Input));
        assert_eq!(InputHeader::decode(&mut r), Ok(h));
    }

    w.clear();
    let wel = Welcome {
        peer: PeerId(7),
        tick: 99,
        seed: u64::MAX,
        snapshot_every: 3,
    };
    wel.encode(&mut w);
    let mut r = Reader::new(w.as_bytes());
    assert_eq!(Kind::read(&mut r), Ok(Kind::Welcome));
    assert_eq!(Welcome::decode(&mut r), Ok(wel));

    for slack in [i32::MIN, -5, 0, 7, i32::MAX] {
        w.clear();
        let s = SnapshotHeader {
            tick: 600,
            ack_input: Some(603),
            slack,
            baseline: None,
        };
        s.encode(&mut w);
        let mut r = Reader::new(w.as_bytes());
        assert_eq!(Kind::read(&mut r), Ok(Kind::Snapshot));
        assert_eq!(SnapshotHeader::decode(&mut r), Ok(s));
    }

    for why in [RefuseReason::Protocol, RefuseReason::Full] {
        w.clear();
        why.encode(&mut w);
        let mut r = Reader::new(w.as_bytes());
        assert_eq!(Kind::read(&mut r), Ok(Kind::Refuse));
        assert_eq!(RefuseReason::decode(&mut r), Ok(why));
    }
}

#[test]
fn header_limits_are_enforced() {
    // More frames than an input packet may carry.
    let mut w = Writer::new();
    InputHeader {
        ack_snapshot: None,
        newest: 10,
        count: u8::try_from(MAX_INPUT_FRAMES + 1).unwrap(),
    }
    .encode(&mut w);
    let mut r = Reader::new(w.as_bytes());
    Kind::read(&mut r).unwrap();
    assert_eq!(InputHeader::decode(&mut r), Err(WireError::TooLong));
    // Frames before tick 0 do not exist.
    let h = InputHeader {
        ack_snapshot: None,
        newest: 1,
        count: 3,
    };
    assert_eq!(h.frame_tick(0), Some(1));
    assert_eq!(h.frame_tick(1), Some(0));
    assert_eq!(h.frame_tick(2), None);
    // A welcome may not say "snapshot every 0 ticks".
    w.clear();
    Welcome {
        peer: PeerId(1),
        tick: 0,
        seed: 0,
        snapshot_every: 0,
    }
    .encode(&mut w);
    let mut r = Reader::new(w.as_bytes());
    Kind::read(&mut r).unwrap();
    assert_eq!(Welcome::decode(&mut r).map(|w| w.snapshot_every), Ok(1));
    // Unknown kinds and refusal reasons.
    for b in 0u8..=255 {
        let known = matches!(b, 0x10 | 0x11 | 0x12 | 0x20 | 0x21 | 0x22 | 0x23);
        assert_eq!(Kind::from_u8(b).is_some(), known, "kind byte {b:#x}");
    }
    assert_eq!(
        RefuseReason::decode(&mut Reader::new(&[9])),
        Err(WireError::Invalid)
    );
    // An optional tick of u32::MAX + 1 does not fit.
    w.clear();
    w.varint(u64::from(u32::MAX) + 2);
    assert_eq!(
        msg::read_opt_tick(&mut Reader::new(w.as_bytes())),
        Err(WireError::Overflow)
    );
}

#[test]
fn view_codec_round_trips_a_packet() {
    // Newest first, as a client writes them: steady delay, a jump, the view
    // at the tick itself, and a far-back view.
    let frames: [(u32, TickTime); 6] = [
        (500, TickTime::new(494, 0)),
        (499, TickTime::new(493, 0)),
        (498, TickTime::new(492, 128)),
        (497, TickTime::at(497)),
        (496, TickTime::new(10, 3)),
        (495, TickTime::new(489, 255)),
    ];
    let mut w = Writer::new();
    let mut enc = ViewCodec::new();
    for (t, v) in frames {
        enc.write(&mut w, t, v);
    }
    let mut r = Reader::new(w.as_bytes());
    let mut dec = ViewCodec::new();
    for (t, v) in frames {
        assert_eq!(dec.read(&mut r, t), Ok(v), "tick {t}");
    }
    assert!(r.is_empty());
    // A view after its own tick is sent as the tick itself.
    w.clear();
    ViewCodec::new().write(&mut w, 10, TickTime::new(12, 0));
    assert_eq!(
        ViewCodec::new().read(&mut Reader::new(w.as_bytes()), 10),
        Ok(TickTime::at(10))
    );
    // A steady delay costs one byte per frame after the first.
    w.clear();
    let mut enc = ViewCodec::new();
    for t in (100u32..110).rev() {
        enc.write(&mut w, t, TickTime::at(t).sub_ticks(6));
    }
    assert_eq!(w.len(), 2 + 9);
    // A change that would put the view before tick 0 of "back" is refused.
    w.clear();
    w.varint(5);
    w.zigzag(-6);
    let mut r = Reader::new(w.as_bytes());
    let mut dec = ViewCodec::new();
    assert!(dec.read(&mut r, 100).is_ok());
    assert_eq!(dec.read(&mut r, 99), Err(WireError::Invalid));
}

#[test]
fn peer_input_lists_round_trip_sorted() {
    let a = ArenaInput::new(1, 0, 0);
    let b = ArenaInput::new(-1, 1, 1);
    let mut w = Writer::new();
    let list = [(PeerId(5), &a), (PeerId(2), &b)];
    msg::write_peer_inputs(&mut w, list.into_iter());
    let mut out: Vec<(PeerId, ArenaInput)> = vec![(PeerId(99), a)];
    msg::read_peer_inputs(&mut Reader::new(w.as_bytes()), &mut out).unwrap();
    assert_eq!(out, vec![(PeerId(2), b), (PeerId(5), a)]);
}

/// Random and mutated bytes into every decoder: errors, never a panic.
#[test]
fn decoders_never_panic_on_garbage() {
    let mut rng = Pcg32::new(0x5eed, 1);
    // Real packets to mutate.
    let mut seeds: Vec<Vec<u8>> = Vec::new();
    let mut w = Writer::new();
    Hello::new(1).encode(&mut w);
    seeds.push(w.as_bytes().to_vec());
    seeds.push(crate::support::raw_input(
        400,
        &[ArenaInput::new(1, -1, 1); 6],
    ));
    w.clear();
    SnapshotHeader {
        tick: 30,
        ack_input: Some(33),
        slack: -2,
        baseline: None,
    }
    .encode(&mut w);
    msg::write_peer_inputs(&mut w, [(PeerId(1), &ArenaInput::new(1, 1, 0))].into_iter());
    let mut st = ArenaState::new();
    st.players.push(Player {
        peer: 1,
        x: 1.0,
        y: 2.0,
        vx: 0.0,
        vy: 0.0,
        score: 0,
        prev_buttons: 0,
        shots: 0,
        last_rewind: 0,
    });
    st.write(&mut w);
    seeds.push(w.as_bytes().to_vec());

    let mut decoded = 0u32;
    for i in 0..20_000u32 {
        let bytes: Vec<u8> = if i % 2 == 0 {
            let n = rng.below(64) as usize;
            (0..n).map(|_| rng.next_u32() as u8).collect()
        } else {
            let mut b = seeds[(i as usize / 2) % seeds.len()].clone();
            for _ in 0..=rng.below(4) {
                if b.is_empty() {
                    break;
                }
                let at = rng.below(b.len() as u32) as usize;
                match rng.below(3) {
                    0 => b[at] = rng.next_u32() as u8,
                    1 => b.truncate(at),
                    _ => b.insert(at, rng.next_u32() as u8),
                }
            }
            b
        };
        let mut r = Reader::new(&bytes);
        let Ok(kind) = Kind::read(&mut r) else {
            continue;
        };
        let ok = match kind {
            Kind::Hello => Hello::decode(&mut r).is_ok(),
            Kind::Input => InputHeader::decode(&mut r).is_ok_and(|h| {
                let mut views = ViewCodec::new();
                (0..h.count).all(|k| {
                    h.frame_tick(k).is_some_and(|t| {
                        views.read(&mut r, t).is_ok() && ArenaInput::read(&mut r).is_ok()
                    })
                })
            }),
            Kind::Welcome => Welcome::decode(&mut r).is_ok(),
            Kind::Snapshot => crate::support::decode_snapshot(&bytes).is_some(),
            Kind::Refuse => RefuseReason::decode(&mut r).is_ok(),
            Kind::ClientBye | Kind::ServerBye => true,
        };
        decoded += u32::from(ok);
    }
    // Some mutations stay valid; the point is that none panicked.
    assert!(decoded > 0);
}
