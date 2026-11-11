//! The sync layer under a bad link (feature `sync`).
//!
//! One server and one client over the engine's `Loopback` at 150 ms one-way
//! latency, 0 to 33 ms jitter (2 ticks) and 5 % loss each way, 60 Hz ticks,
//! snapshots at 20 Hz. The client drives a test car (prediction with
//! sequence-numbered inputs, sent 8 deep); the server is the authority and,
//! now and then, bumps the car sideways (a collision the client cannot
//! know of), so reconciles really correct. Three server cars drive their own
//! scripted laps and reach the client only as snapshots.
//!
//! Measured per drawn frame, against the same run drawn raw (the old way):
//! * the local car answers input on the very frame it is pressed, and the
//!   drawn car never jumps beyond its own motion by more than the snap
//!   threshold (in fact by centimetres);
//! * remote cars move continuously: no frame moves one further than its
//!   true speed allows (raw drawing teleports a car every snapshot).

use evani_core::rng::Pcg32;
use evani_net::sync::{Angle, Predictor, SnapshotBuffer, SyncConfig, SyncSampled, SyncSim};
use evani_net::{LinkConditions, LinkId, Loopback, NetId, Tick, Transport};

const HZ: u32 = 60;
const DT: f32 = 1.0 / 60.0;
const SNAPSHOT_EVERY: u32 = 3;
const REDUNDANCY: usize = 8;
const ACCEL: f32 = 12.0;
const DRAG: f32 = 0.6;
const TURN: f32 = 2.2;
const MAX_SPEED: f32 = 18.0;
/// A drawn step over twice the fastest a car can move in a tick is a
/// teleport.
const TELEPORT_M: f32 = 2.0 * MAX_SPEED * DT;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Car {
    x: f32,
    y: f32,
    heading: f32,
    speed: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Drive {
    throttle: i8,
    steer: i8,
}

type Pose = ([f32; 2], Angle);

struct CarSim;

fn drive(c: &mut Car, d: &Drive) {
    c.speed += f32::from(d.throttle) * ACCEL * DT - c.speed * DRAG * DT;
    c.speed = c.speed.clamp(-MAX_SPEED * 0.3, MAX_SPEED);
    c.heading += f32::from(d.steer) * TURN * DT * (c.speed / MAX_SPEED).clamp(-1.0, 1.0);
    c.x += c.heading.cos() * c.speed * DT;
    c.y += c.heading.sin() * c.speed * DT;
}

impl SyncSim for CarSim {
    type State = Car;
    type Input = Drive;
    type Pose = Pose;
    fn step(&self, s: &mut Car, d: &Drive) {
        drive(s, d);
    }
    fn pose(&self, s: &Car) -> Pose {
        ([s.x, s.y], Angle(s.heading))
    }
}

/// The player's scripted driving: still for half a second, then throttle
/// with turns that start and stop.
fn player_input(t: Tick) -> Drive {
    if t < 30 {
        return Drive::default();
    }
    let phase = (t / 50) % 6;
    Drive {
        throttle: if phase == 5 { 0 } else { 1 },
        steer: match phase {
            1 => 1,
            3 => -1,
            4 => 1,
            _ => 0,
        },
    }
}

fn bot_input(id: u32, t: Tick) -> Drive {
    let phase = (t / (70 + id * 13) + id) % 4;
    Drive {
        throttle: 1,
        steer: match phase {
            0 => 1,
            2 => -1,
            _ => 0,
        },
    }
}

// --- packets (little-endian, test-only) -------------------------------------

fn put_u32(b: &mut Vec<u8>, v: u32) {
    b.extend_from_slice(&v.to_le_bytes());
}
fn put_f32(b: &mut Vec<u8>, v: f32) {
    b.extend_from_slice(&v.to_le_bytes());
}
struct Rd<'a>(&'a [u8]);
impl Rd<'_> {
    // A short packet reads as zeros (the test's own packets are never short).
    fn u8(&mut self) -> u8 {
        let Some((h, t)) = self.0.split_first() else { return 0 };
        self.0 = t;
        *h
    }
    fn u32(&mut self) -> u32 {
        let Some((h, t)) = self.0.split_first_chunk::<4>() else { return 0 };
        self.0 = t;
        u32::from_le_bytes(*h)
    }
    fn f32(&mut self) -> f32 {
        f32::from_bits(self.u32())
    }
}

fn input_packet(inputs: &[(Tick, Drive)]) -> Vec<u8> {
    let mut b = Vec::new();
    b.push(inputs.len() as u8);
    for (seq, d) in inputs {
        put_u32(&mut b, *seq);
        b.push(d.throttle as u8);
        b.push(d.steer as u8);
    }
    b
}

struct Snapshot {
    tick: Tick,
    ack: Option<Tick>,
    car: Car,
    bots: Vec<(NetId, Pose)>,
}

fn snapshot_packet(s: &Snapshot) -> Vec<u8> {
    let mut b = Vec::new();
    put_u32(&mut b, s.tick);
    b.push(u8::from(s.ack.is_some()));
    put_u32(&mut b, s.ack.unwrap_or(0));
    for v in [s.car.x, s.car.y, s.car.heading, s.car.speed] {
        put_f32(&mut b, v);
    }
    b.push(s.bots.len() as u8);
    for (id, p) in &s.bots {
        put_u32(&mut b, id.0);
        put_f32(&mut b, p.0[0]);
        put_f32(&mut b, p.0[1]);
        put_f32(&mut b, p.1.0);
    }
    b
}

fn read_snapshot(data: &[u8]) -> Snapshot {
    let mut r = Rd(data);
    let tick = r.u32();
    let has = r.u8() == 1;
    let ack = r.u32();
    let car = Car {
        x: r.f32(),
        y: r.f32(),
        heading: r.f32(),
        speed: r.f32(),
    };
    let n = r.u8();
    let bots = (0..n)
        .map(|_| {
            let id = NetId(r.u32());
            let (x, y, h) = (r.f32(), r.f32(), r.f32());
            (id, ([x, y], Angle(h)))
        })
        .collect();
    Snapshot {
        tick,
        ack: has.then_some(ack),
        car,
        bots,
    }
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

fn sub(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn len(a: [f32; 2]) -> f32 {
    (a[0] * a[0] + a[1] * a[1]).sqrt()
}

/// What one run measured.
#[derive(Debug, Default)]
struct Run {
    /// Frames from the first throttle press to the drawn car moving.
    local_response_frames: u32,
    /// The same if the car were drawn from snapshots (no prediction).
    raw_response_frames: u32,
    /// Largest per-frame step of the drawn local car beyond its own motion.
    local_jump_max: f32,
    /// The same, drawn raw from the reconciled prediction (no smoothing).
    local_raw_jump_max: f32,
    corrections: u64,
    max_error: f32,
    snaps: u64,
    /// Largest per-frame step of a drawn remote car.
    remote_step_max: f32,
    /// Largest true per-tick motion of a remote car.
    remote_true_step_max: f32,
    /// Car-frames a remote car moved more than twice the fastest possible
    /// per-tick motion.
    remote_teleports: u32,
    /// The same count drawn raw from the newest snapshot.
    remote_raw_teleports: u32,
    remote_raw_step_max: f32,
    /// Largest distance from the true position at the render time
    /// (interpolated frames).
    remote_error_max: f32,
    extrapolated_frames: u64,
    capped_frames: u64,
    delay_s: f64,
    jitter_s: f64,
    snapshots_received: u64,
    final_error: f32,
}

fn run(seed: u64, ticks: Tick) -> Run {
    let link = Loopback::new(
        LinkConditions {
            latency_ticks: 9,  // 150 ms
            jitter_ticks: 2,   // + 0 to 33 ms
            loss_permille: 50, // 5 %
        },
        seed,
    );
    let mut server_end = link.server();
    let mut client_end = link.connect();
    let client_link: LinkId = client_end.link();
    let cfg = SyncConfig::default();
    let mut rng = Pcg32::new(seed, 0x6275_6d70);

    // Server: the authority for every car.
    let mut srv_car = Car::default();
    let mut srv_ack: Option<Tick> = None;
    let mut srv_last = Drive::default();
    let mut bots: Vec<Car> = (0..3)
        .map(|i| Car {
            x: 20.0 * i as f32,
            y: 10.0,
            heading: 0.5 * i as f32,
            speed: 8.0,
        })
        .collect();
    // True bot positions by server tick (to judge the drawing).
    let mut truth: Vec<Vec<[f32; 2]>> = Vec::new();

    // Client.
    let mut pred = Predictor::new(&CarSim, Car::default(), 0, cfg);
    let mut remote: SnapshotBuffer<Pose> = SnapshotBuffer::new(32, cfg);
    let mut last_snap: Option<Tick> = None;
    let mut raw_car: Option<Car> = None;
    let mut raw_bots: Vec<(NetId, Pose)> = Vec::new();

    let mut out = Run::default();
    let mut drawn_prev: Option<[f32; 2]> = None;
    let mut raw_prev: Option<[f32; 2]> = None;
    let mut first_press: Option<Tick> = None;
    let mut local_moved: Option<Tick> = None;
    let mut raw_moved: Option<Tick> = None;
    let mut remote_prev: Vec<Option<[f32; 2]>> = vec![None; 3];
    let mut remote_raw_prev: Vec<Option<[f32; 2]>> = vec![None; 3];
    let mut poses = Vec::new();
    let mut buf = Vec::new();
    let mut next_bump = 90 + rng.below(120);

    for t in 0..ticks {
        link.advance();

        // ---- server tick
        while server_end.recv(&mut buf).is_some() {
            let mut r = Rd(&buf);
            let n = r.u8();
            let mut got: Vec<(Tick, Drive)> = (0..n)
                .map(|_| {
                    let seq = r.u32();
                    let d = Drive {
                        throttle: r.u8() as i8,
                        steer: r.u8() as i8,
                    };
                    (seq, d)
                })
                .collect();
            got.sort_by_key(|g| g.0);
            for (seq, d) in got {
                let next = srv_ack.map_or(0, |a| a + 1);
                if seq < next {
                    continue;
                }
                // A gap redundancy did not cover: the missing inputs are
                // taken as the last one (the client will be corrected).
                for _ in next..seq {
                    drive(&mut srv_car, &srv_last);
                }
                drive(&mut srv_car, &d);
                srv_last = d;
                srv_ack = Some(seq);
            }
        }
        if t == next_bump && t + 120 < ticks {
            // A collision only the server sees: 0.3 to 1.0 m sideways.
            let m = 0.3 + 0.7 * rng.next_f32();
            let side = if rng.below(2) == 0 { 1.0 } else { -1.0 };
            srv_car.x += -srv_car.heading.sin() * m * side;
            srv_car.y += srv_car.heading.cos() * m * side;
            next_bump = t + 60 + rng.below(120);
        }
        for (i, b) in bots.iter_mut().enumerate() {
            drive(b, &bot_input(i as u32, t));
        }
        truth.push(bots.iter().map(|b| [b.x, b.y]).collect());
        if t % SNAPSHOT_EVERY == 0 {
            let snap = Snapshot {
                tick: t,
                ack: srv_ack,
                car: srv_car,
                bots: bots
                    .iter()
                    .enumerate()
                    .map(|(i, b)| (NetId(i as u32 + 1), CarSim.pose(b)))
                    .collect(),
            };
            server_end.send(client_link, &snapshot_packet(&snap));
        }

        // ---- client tick
        let local_t = f64::from(t) / f64::from(HZ);
        let mut correction = [0.0f32; 2];
        while client_end.recv(&mut buf).is_some() {
            out.snapshots_received += 1;
            let s = read_snapshot(&buf);
            if last_snap.is_some_and(|l| s.tick <= l) {
                // Reordered: the buffer still slots its poses in.
                remote.push(f64::from(s.tick) / f64::from(HZ), local_t, &s.bots);
                continue;
            }
            last_snap = Some(s.tick);
            let before = pred.pose().0;
            pred.reconcile(&CarSim, s.ack, s.car);
            let after = pred.pose().0;
            correction = [correction[0] + after[0] - before[0], correction[1] + after[1] - before[1]];
            remote.push(f64::from(s.tick) / f64::from(HZ), local_t, &s.bots);
            raw_car = Some(s.car);
            raw_bots = s.bots;
        }
        let input = player_input(t);
        if input.throttle != 0 && first_press.is_none() {
            first_press = Some(t);
        }
        let before_step = pred.pose().0;
        let seq = pred.predict(&CarSim, input);
        let motion = sub(pred.pose().0, before_step);
        let send: Vec<(Tick, Drive)> = pred.inputs().newest(REDUNDANCY).copied().collect();
        assert_eq!(send.last().map(|s| s.0), Some(seq));
        client_end.send(LinkId::SERVER, &input_packet(&send));

        // ---- the drawn frame (60 Hz, alpha 1: the newest tick)
        let drawn = pred.render_pose(1.0, DT).0;
        if let Some(p) = drawn_prev {
            let beyond = len(sub(sub(drawn, p), motion));
            out.local_jump_max = out.local_jump_max.max(beyond);
        }
        // Raw: the reconciled prediction drawn as is.
        out.local_raw_jump_max = out.local_raw_jump_max.max(len(correction));
        if first_press.is_some() && local_moved.is_none() && drawn_prev.is_some_and(|p| dist(p, drawn) > 0.0) {
            local_moved = Some(t);
        }
        drawn_prev = Some(drawn);
        if let Some(c) = raw_car {
            let p = [c.x, c.y];
            if first_press.is_some() && raw_moved.is_none() && raw_prev.is_some_and(|q| dist(q, p) > 0.0) {
                raw_moved = Some(t);
            }
            raw_prev = Some(p);
        }

        remote.advance(local_t, f64::from(DT));
        let mode = remote.sample(&mut poses);
        let rt = remote.render_time();
        for (id, pose) in &poses {
            let i = (id.0 - 1) as usize;
            if let Some(prev) = remote_prev[i] {
                let step = dist(prev, pose.0);
                out.remote_step_max = out.remote_step_max.max(step);
                out.remote_teleports += u32::from(step > TELEPORT_M);
            }
            remote_prev[i] = Some(pose.0);
            // Distance from the true position at the render time.
            if mode == SyncSampled::Interpolated
                && let Some(rt) = rt
            {
                let st = rt * f64::from(HZ);
                let k = st.floor() as usize;
                if let (Some(a), Some(b)) = (truth.get(k), truth.get(k + 1)) {
                    let f = (st - st.floor()) as f32;
                    let want = [
                        a[i][0] + (b[i][0] - a[i][0]) * f,
                        a[i][1] + (b[i][1] - a[i][1]) * f,
                    ];
                    out.remote_error_max = out.remote_error_max.max(dist(want, pose.0));
                }
            }
        }
        for (id, pose) in &raw_bots {
            let i = (id.0 - 1) as usize;
            if let Some(prev) = remote_raw_prev[i] {
                let step = dist(prev, pose.0);
                out.remote_raw_step_max = out.remote_raw_step_max.max(step);
                out.remote_raw_teleports += u32::from(step > TELEPORT_M);
            }
            remote_raw_prev[i] = Some(pose.0);
        }
        // True per-tick motion (the yardstick for a teleport).
        if let (Some(a), Some(b)) = (truth.len().checked_sub(2).and_then(|k| truth.get(k)), truth.last()) {
            for (pa, pb) in a.iter().zip(b) {
                out.remote_true_step_max = out.remote_true_step_max.max(dist(*pa, *pb));
            }
        }
    }

    let s = pred.stats();
    out.corrections = s.corrections;
    out.max_error = s.max_error;
    out.snaps = s.snaps;
    let b = remote.stats();
    out.extrapolated_frames = b.extrapolated_frames;
    out.capped_frames = b.capped_frames;
    out.delay_s = b.delay_s;
    out.jitter_s = b.jitter_s;
    out.local_response_frames = local_moved.zip(first_press).map_or(u32::MAX, |(m, p)| m - p);
    out.raw_response_frames = raw_moved.zip(first_press).map_or(u32::MAX, |(m, p)| m - p);
    // The server's car plus the inputs still on their way to it is where the
    // prediction (and, settled, the drawing) must be.
    let mut want = srv_car;
    for (seq, d) in pred.inputs().iter() {
        if srv_ack.is_none_or(|a| *seq > a) {
            drive(&mut want, d);
        }
    }
    out.final_error = dist(pred.peek_pose(1.0).0, [want.x, want.y]);
    out
}

/// Done-when 1 and 2 of the sync layer: 150 ms, 30 ms jitter, 5 % loss,
/// six seeded runs of 40 s each.
#[test]
fn sync_layer_holds_under_150ms_jitter_and_loss() {
    let snap = SyncConfig::default().snap_threshold;
    for seed in 1..=6u64 {
        let r = run(seed, 60 * 40);
        println!(
            "seed {seed}: local response {} frame(s) (raw {}), local jump max {:.3} m (raw {:.3} m), \
             corrections {} (max error {:.2} m, snaps {}), remote step max {:.3} m (true per tick {:.3} m, \
             raw {:.3} m, raw teleports {}), remote error max {:.3} m, extrapolated {} capped {} frames, delay {:.0} ms, \
             jitter {:.1} ms, snapshots {}, final error {:.3} m",
            r.local_response_frames,
            r.raw_response_frames,
            r.local_jump_max,
            r.local_raw_jump_max,
            r.corrections,
            r.max_error,
            r.snaps,
            r.remote_step_max,
            r.remote_true_step_max,
            r.remote_raw_step_max,
            r.remote_raw_teleports,
            r.remote_error_max,
            r.extrapolated_frames,
            r.capped_frames,
            r.delay_s * 1000.0,
            r.jitter_s * 1000.0,
            r.snapshots_received,
            r.final_error,
        );
        // 1. The local car answers on the same frame; drawn raw from
        //    snapshots it would wait a round trip.
        assert_eq!(r.local_response_frames, 0, "seed {seed}");
        assert!(r.raw_response_frames >= 18, "seed {seed}: raw {}", r.raw_response_frames);
        // Corrections really happened, and never show as a jump beyond the
        // car's own motion (under the snap threshold by far).
        assert!(r.corrections > 5, "seed {seed}: {} corrections", r.corrections);
        assert_eq!(r.snaps, 0, "seed {seed}");
        assert!(r.local_jump_max < snap, "seed {seed}");
        assert!(r.local_jump_max < 0.25, "seed {seed}: {:.3}", r.local_jump_max);
        assert!(r.local_raw_jump_max > 3.0 * r.local_jump_max, "seed {seed}: raw draw would jump");
        assert!(r.final_error < 0.05, "seed {seed}: prediction settles on the server");
        // 2. Remote cars move continuously: no step over twice the true
        //    per-tick motion, while raw drawing teleports every snapshot.
        assert_eq!(r.remote_teleports, 0, "seed {seed}: step {:.3}", r.remote_step_max);
        assert!(r.remote_step_max <= TELEPORT_M, "seed {seed}");
        assert!(r.remote_raw_teleports > 1000, "seed {seed}: raw {}", r.remote_raw_teleports);
        assert!(r.remote_error_max < 1.0, "seed {seed}: {:.3}", r.remote_error_max);
    }
}
