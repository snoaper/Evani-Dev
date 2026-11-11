# The net sync layer: what an online game draws

`evani-net` feature `sync` (`crates/evani-net/src/sync.rs`). Opt-in: a game that does not turn it on builds none of it, and its download is unchanged.

## Why it exists

An online game must never draw raw server state. Before this layer the engine's net module handed a kit only the bare predicted state (`Client::predicted`, `LockstepClient::display`) and snapshot poses that stop dead when updates run late (`PoseHistory::sample` holds the newest), so every online kit had to invent its own smoothing. The kart racer did (`NetSmooth` in its kit, 2026-10-29) after the owner saw "the online game shows raw server updates with no smoothing or prediction of the player's car"; the next online game would have started without it. The sync layer is that smoothing, once, in the engine.

## What it does

| Part | For | What it does |
|---|---|---|
| `Predictor<S: SyncSim>` | the locally controlled entity | Each fixed tick, `predict(sim, input)` steps the entity with the input **now** (it answers on the frame the key goes down) and stores the input in an `InputHistory` under a sequence number. Each authoritative snapshot, `reconcile(sim, ack, server_state)` rewinds to the server's state and replays every input after `ack`. The server stays the authority; the client predicts only its own entity. |
| `ErrorSmoother<P>` | corrections | The gap between where the entity was drawn and where it belongs after a reconcile becomes a render offset that decays by `e^(-net.sync.smooth_rate * dt)` each frame. An error over `net.sync.snap_threshold` cuts at once (a respawn). `Predictor` uses one; use your own for anything else you correct. |
| `SnapshotBuffer<P>` | remote entities | Keeps snapshots by server time and draws them `net.interp_delay_ms` (default 100 ms) in the past, longer when the link is jittery (the delay covers the send interval plus `net.sync.jitter_scale` times the measured jitter, at most 500 ms). The render clock slews at most 10 % so it never jumps or runs backwards. When snapshots run out it extrapolates along the last motion for at most `net.sync.extrapolate_max_ms`, then holds; a late snapshot that disagrees is eased in, not jumped to. |

Poses implement `Interpolate` + `Offset`: `f32`, `[f32; N]`, `Angle` (radians, the short way round) and pairs, such as `([f32; 3], Angle)`. A pair's size for the snap threshold is its **first** part's, so put the position first.

Nothing here reads a wall clock or a random generator. The game's simulation stays its fixed 60 Hz step with `Pcg32`; the buffer runs on the times the game passes in (tick count times the step, plus the frame's alpha).

## Parameters (live, `schema_get` lists them)

| Name | Default | What |
|---|---|---|
| `net.interp_delay_ms` | 100 ms | The least render delay for remote entities (shared with `Client`). |
| `net.sync.smooth_rate` | 12 /s | How fast a correction eases away: half gone in about 58 ms, 95 % in 250 ms. 0 shows corrections at once. |
| `net.sync.snap_threshold` | 3 m | Corrections larger than this cut instead of blending. |
| `net.sync.extrapolate_max_ms` | 200 ms | How long remote entities keep moving when snapshots stop (0: they hold). |
| `net.sync.jitter_scale` | 2 | How much measured jitter is added to the delay (0: the delay stays fixed). |

Read them with `SyncConfig::from_world(world)` every frame and pass the result to `Predictor::set_config` and `SnapshotBuffer::set_config`, so a live change (an AI agent, the Look Lab) applies at once. All four are registered only when `sync` is built.

## How a kit switches it on

1. In the **app's** `Cargo.toml`, add `"evani-net/sync"` to the `net` feature (next to `lockstep` or `port` if the game uses them): `net = ["dep:evani-net", "evani-net/sync", ...]`. evani.toml keeps `net = true`; a game with `net = false` builds no evani-net at all. A kit that names the types depends on `evani-net` optionally with `features = ["sync"]` behind its own `net` feature.
2. Implement `SyncSim` for the local entity: `State` (what the step reads and writes), `Input`, `Pose` (what is drawn), `step` (the same fixed step the server runs) and `pose`.
3. Each fixed tick: `let seq = predictor.predict(&sim, input);` then send `predictor.inputs().newest(8)` (the input with the last few again, so a lost packet costs nothing).
4. On each authoritative snapshot: `predictor.reconcile(&sim, snapshot.ack, snapshot.my_state)` and `buffer.push(server_t, local_t, &snapshot.remote_poses)`.
5. Each drawn frame: `predictor.render_pose(alpha, dt)` for the player's own entity, `buffer.advance(local_t, dt)` then `buffer.sample(&mut poses)` for everyone else. Never draw the snapshot itself.

With the engine's `Client` (it already predicts and rolls back the whole state), draw remote entities from a `SnapshotBuffer` instead of `Client::remote_poses`, and ease the local entity by feeding an `ErrorSmoother` the pose change `Client::poll` caused (the pose before `poll` minus the pose after it) and drawing `smoother.apply(&pose)`.

## Proof

`cargo test -p evani-net` builds the layer (the crate's tests turn `sync` on) and runs:

- unit tests in `src/sync.rs`: the input history, the same-tick answer to input, reconciliation replay (agreeing and correcting), the error smoothing decay and settle, the snap threshold (one big error, small ones adding up, rate 0), angles, interpolation between two snapshots, the extrapolation cap, a late snapshot eased in after extrapolating, the delay adapting to jitter, reordered and departed entities, and the parameters reaching `SyncConfig`;
- `tests/net/sync.rs`: one server and one client over the engine's `Loopback` at 150 ms one-way latency, 0 to 33 ms jitter and 5 % loss each way, six seeded 40 s runs. The client's car answers input on the frame it is pressed (raw drawing waits a round trip); the server bumps it now and then, and the corrections never show as a jump beyond its own motion; three server cars drawn from the buffer never move further in a frame than they really can, while drawing them raw teleports them at every snapshot.

Out of scope here: lag compensation and server-side hit rewind (the `Server` has its own), transport and matchmaking, peer-to-peer rollback (lockstep has its own re-run).
