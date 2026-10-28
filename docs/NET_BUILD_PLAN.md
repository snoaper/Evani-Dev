# Multiplayer engine: audit and build plan (`net`)

Written 2026-10-24 on branch `net-engine` (from `m1-foundation` 65e84978). Plan only: no engine code changes in this commit.

Go-ahead: Chris, 2026-10-24: "I want you to fully build the Stage 3 multi-player engine. However, as I was under the impression that this was already built, maybe it's just not incorporated. Either way, get it done." Later the same day, through the coordinator: "Build the multiplayer engines, stick it in Sugarpeak Racer, and then we'll send them back and say that that's what we're willing to do. And then give them instructions on how to build the bridge. Incorporate what you can without impacting performance or compatibility with other systems, other providers, and without putting specific ChimpVibe code inside our engine." His request overrides STAGES.md rule 1 for this work only (multiplayer sits in Stages 5 and 9 of STAGES.md; the current stage is 3). Recorded in STAGES.md, Stage 3 notes.

Hard rule (Chris, 2026-10-24): the engine stays platform-neutral. No ChimpVibe-specific (or any one platform's) behaviour inside the engine. Partner integrations live in separate bridges outside the engine. Never break compatibility with Evani.app, Poki, CrazyGames, itch.io or any other host.

Licence: the coordinator reports (2026-10-24, relaying Chris) that an agreement with ChimpVibe already exists. This plan therefore treats running the neutral referee build on a partner's servers as allowed. The referee is the same neutral build for every host; nothing in it is ChimpVibe-specific.

---

## 1. Audit: what exists

Chris was right: a multiplayer core was built, in Stage 5 (item S5-N1, 2026-10-20), and merged. It is not the whole engine: it has no real network connection, no server build, no spectators and almost no tests.

| What | Where | State |
|---|---|---|
| Netcode core `evani-net` (3,350 lines): `NetGame` trait (fixed-step `step`, `Input`, `State`, `Pose`, `PROTOCOL`), bit-packed wire format (varint/zigzag, quantise, FNV-1a), input and snapshot packets, server-authoritative `Server` (per-peer input queues, late input = repeat last, bots as server peers, pose history, lag compensation), predicting `Client` (rollback replay on checksum mismatch, slack-based pacing, interpolation), `Transport` trait, deterministic in-page `Loopback` (latency, jitter, loss from a seeded generator), `LocalSession`, six `net.*` parameters, `NetStats` overlay line | `crates/evani-net/src/*.rs` on `m1-foundation` (commits 3cc6cb03, ea3ffac1, 6d3b1946; merged) | Built and merged. **1 unit test** (params register). Chris deferred tests for Stages 4 to 6 to one combined pass (DECISIONS "Netcode core", 2026-10-20). Snapshot delta body reserved, not built. |
| `net` feature wiring | seven game apps (kart-racer, 2d-platformer, match-3, top-down, RTS, tactical-fps, sandbox) have `net = ["dep:evani-net"]` in their `Cargo.toml`; `evani.toml` `[modules] net` | Built. Optional dependency; games with `net = false` do not compile it. |
| First user: Hexagon (tactical FPS) over evani-net | `kits/evani-kit-tactical-fps/src/net/` (mod.rs, session.rs, wire.rs; 941 lines), app feature `net`, `net-dev` (second local player) | Merged (S5-K6b, 830d2c59). Offline with bots through the loopback; never played over a real network. Stage 5 gate line "The tactical FPS plays offline with bots through evani-net" is not ticked. |
| Host-driven play `evani-net/host` (`HostLink`: views in, actions out, spectator role; postMessage bridge with a `CHIMPVIBE` message-name table) | `crates/evani-net/src/host.rs`, `host_web.rs` on branch `game-broadside-3d` (73c7ac16), worktree `..\evani-dev-broadside3d` | **Not merged.** Breaks the neutrality rule as written: a ChimpVibe-named table and ChimpVibe's limits sit inside the engine crate. Must be made neutral before it merges (package X1). |
| ChimpVibe rules ABI shim for Broadside (`chimpvibe-rules/1` exports: initialize/restore/step/project, JSON in and out) | `kits/evani-kit-broadside-3d/rules/` on the same unmerged branch | A kit-side port of a JavaScript game, not engine. Useful prior work for the bridge guide's mapping; stays out of the engine. |
| Bridge proposal to ChimpVibe ("the browser half of multiplayer") | `..\Sugarpeak rally\docs\chimpvibe-multiplayer-bridge-proposal.md` (also .html, .pdf), draft 1, 2026-10-24 | Sent. Proposed clients sending car state (position, speed) for a JS `runtime.js` to sanity-check. ChimpVibe's answer replaces that with inputs only and a server referee (section 6). |
| ChimpVibe's answer (8 changes, 7 questions) | `C:\dev\evani-dev\.evani\tmp\chimpvibe-letter\letter.txt` | Answered by this plan (section 6, and K3's measurements for question 7). |
| ChimpVibe's rules add-on format (`chimpvibe-rules/1`, tickMs 50 = three 1/60 s steps, spectator `project`, limits) | `.vibedeck/chimpvibe/chimpvibe-reply-4.md` | Input to the bridge guide. |
| Sugarpeak Rally (= the kart racer, live build f4751068 plus the slipstream patch) | `..\Sugarpeak rally\` (ChimpVibe draft `sugarpeak-rally#0`, private), patch `source/0001-kart-racer-slipstream.patch` | Single-player only. `apps/kart-racer/evani.toml` has `net = false`. Race logic lives in the ECS (`race.rs` 1,963 lines over `World`), not in a plain struct the netcode can step. |
| Research | `.vibedeck/research/2026-10-24-multiplayer-net-module-for-evani-dev.md` | Written without knowing evani-net exists ("No earlier report covers multiplayer"). Its ideas are folded in below. |
| Plans | STAGES.md Stage 5 ("evani-net bots"), Stage 9 (Phase C online kits); ROADMAP.md: "online play arrives when evani-net matures"; `.vibedeck/plans/stages-4-6-plan.md` S5-N1 | No multiplayer plan beyond S5-N1. |
| Other games | sugarpeak/kart-racer, platformer, match-3, top-down, RTS, sandbox | `net` feature declared, unused. |

**Missing, in short:** tests and a determinism guard; a real transport (WebSocket, host port); a server build of a game's simulation (the referee); spectators; join mid-session, leave and reconnect handling under a host's roster; checkpoints (save/load) for a host that restarts; input pacing for hosts that cap message rates; the kart racer as a `NetGame`; a platform-neutral bridge contract and guide; measured bandwidth.

---

## 2. Architecture

### 2.1 Model: keep the built server-referee model; borrow lockstep's cheap parts

The coordinator relayed a lockstep-first decision based on the research report. That report did not know evani-net exists. Recommendation, taken here (Chris's standing "follow my recommendations"; he can overrule in one line):

**Keep the built model: clients send inputs only, the server (referee) runs the real simulation, and sends state snapshots; clients predict their own kart and correct.** Reasons:

1. It is built and merged (about 3,350 lines). Lockstep would be a second netcode.
2. It is exactly ChimpVibe's server shape: their server steps a rules module and sends each recipient a complete view every 50 ms (`chimpvibe-reply-4.md` 4 and 5). A snapshot is that view. Lockstep views would have to carry input streams and every client would have to re-simulate all eight karts.
3. Results come from the server's run (ChimpVibe point 7, research's cheat advice). With lockstep, every client's own run decides what it sees.
4. Late join, spectators and reconnect need no checkpoint replay: the next full snapshot is enough.
5. Floating-point drift between devices only costs a correction, never a broken race. With lockstep one differing bit desyncs the race for good.
6. AI racers run on the server as bots (already built: `Bot` trait).

**Borrowed from the research (lockstep, Gaffer on Games, GGRS ideas):** redundant inputs with acks (already built), a playout buffer (the server's input queue, built), catch-up capped at 4 steps per frame (new), an optional small input delay to cut mispredictions (new, `net.input_delay_ticks`), a 32-bit state hash in snapshots for desync telemetry (new; the client already keeps a 64-bit checksum per predicted tick), and GGRS's sync-test mode: run the simulation twice and compare hashes every tick (new, as a test helper and a dev switch). GGRS stays a design reference only: no crate dependency.

**Cost against lockstep:** more bytes down (a snapshot per 50 ms instead of inputs). Measured in K3. If eight-player bandwidth is too high, the reserved delta body (XOR against the acked snapshot, zero runs packed) is package N6. A lockstep relay mode is parked, built only if delta snapshots still miss the budget.

### 2.2 Pieces and where they live

```
engine (platform-neutral, opt-in `net`)                outside the engine
------------------------------------------------       ------------------------------------
crates/evani-net                                       host connectors / bridges
  core: NetGame, Server, Client, wire, interp            - Evani.app: reference host (tools/evani-host,
  spectator client role, checkpoints, pacing               Node, no dependencies) + WebSocket connector
  Transport trait                                        - ChimpVibe: their play-page adapter and
    Loopback (tests, offline, bots)                        their server glue (built by them from
    PortTransport (feature `port`): bytes over a           docs/NET_BRIDGE_GUIDE.md)
      MessagePort the page's host hands over            - any other host: same guide
    WebSocket transport (feature `ws`), JS outside wasm
  referee (feature `referee`): neutral wasm ABI,       tools/evani-host (dev tool, never shipped):
    no imports, no clock, seed in, bytes in/out          loads referee.wasm, serves WebSocket,
                                                         spectators, loopback-like link conditions
kits/<game>: the game's NetGame (Sugarpeak: K2)
```

Rules:
- Nothing reaches a game that doesn't list `net = true`. Each transport and the referee are their own features, so a game takes only what it uses. Byte-identical proof for every other game (P1).
- No platform names in engine code, docs aside (the guide may show a ChimpVibe mapping as one worked example, clearly outside the engine).
- No identity in the engine: a peer is an opaque slot number the host gives per join. No names, accounts, tokens or IPs pass through the engine. Display names, if a host has them, stay in the host's own UI or reach the game as plain text through the host's own channel; the netcode never sees them.
- The game's wasm opens no network on its own when it runs under a host port: all bytes go through the port. That is what ChimpVibe's frame requires (their point 1) and what Poki and others prefer.
- No `serde`, no async runtime, no `format!` on the hot path, no new dependencies in the engine. `tools/evani-host` is plain Node with a hand-written WebSocket server (RFC 6455 framing, about 150 lines) to keep "own tools only" and zero dependencies.

### 2.3 The tick contract

- The simulation steps at the engine's fixed 60 Hz with the seeded `Pcg32` (`StepCtx::rng`: session seed, tick and stream). No wall clock, no `HashMap` iteration, sorted peers (already the rule in `game.rs`).
- A host steps the referee in whole ticks: `step(n)`. ChimpVibe's 50 ms step = `step(3)`. Any host tick that is a multiple of 50 ms works the same way; a host with 1/60 s ticks calls `step(1)`.
- Snapshots go out every `snapshot_every` ticks (3 by default = 20 Hz, matching a 50 ms host step).
- Clients send one input packet every `net.input_send_every` ticks (new; default 2 on our own host, 3 under a host that caps at 30 messages a second), each carrying the newest frames plus every frame the server has not acked (already built: up to 32 frames).

### 2.4 Spectators

A new client role. The referee projects a spectator snapshot (no ack, no pacing fields, no private fields; the `NetGame` may hide per-player secrets through a new `spectator_view` hook that defaults to the whole state). Spectators send nothing; the server ignores any packet from a spectator link. They draw interpolated, `net.spectator_delay_ms` behind (default 0 for casual hosts; a competitive host can set a few seconds). A spectator may follow any player's camera; the view is the same snapshot. Each snapshot is complete on its own, so a spectator who falls behind skips some (ChimpVibe's rule).

### 2.5 Join, leave, reconnect, idle

- Join mid-session: the server seats the new peer through `StepCtx::joined` (built); the first full snapshot brings the client up to date. A racer joining mid-race can be held as a spectator until the next race (the game decides; Sugarpeak: spectator until the next grid).
- Leave: the host's roster is the truth. When the host says a peer left, the server removes it. New option `net.seat_bot_on_leave` (default on for racers): a bot finishes the leaver's kart so the race stays full.
- Reconnect: hosts with anonymous per-join IDs (ChimpVibe point 6) make a reconnect a new join. Hosts with stable IDs may map a returning player to its old slot; that mapping lives in the connector, not the engine.
- Idle: the engine never removes an idle player. `net.timeout_s` only drops a link that has gone fully silent on a transport the engine owns (our WebSocket host); under a host port it is 0 (off) and the host's roster decides (ChimpVibe point 5).

### 2.6 Checkpoints

`Server::save()` and `Server::load()`: the whole referee (state, tick, seed, peers and their input queues, bot state) as bytes. Used by hosts that restart or move a session (ChimpVibe's `restore`), and by tests. A loaded referee steps on bit for bit as if it never stopped (test in N2a).

### 2.7 Lag handling (already built, tuned here)

Client prediction of its own kart (no input lag), others interpolated `net.interp_delay_ms` behind, server slack report moves the client's lead one tick at a time, rollback replay on mismatch, prediction stops after `max_predict_ticks` of silence. New: catch-up cap of `net.max_catchup_steps` (4) per frame; replay cost capped per frame on Lite/Low tiers (the kart step is small, so 8 karts x 30 ticks fits; measured in K3).

### 2.8 Determinism and floats

- The referee is a wasm32 build of the same Rust simulation as the browser client. Wasm floats are IEEE with no fused or reordered operations, and Rust's `sin`/`cos` on `wasm32-unknown-unknown` are compiled-in, so the server and every browser should agree bit for bit. Tested on every tier device in K3, not assumed.
- A native (x86) referee is allowed for tests and tools but is not the shipped server: its `std` maths can call the platform's maths library and differ. Hosts run `referee.wasm` (in Node, Deno or any wasm runtime).
- If bits still differ somewhere, the server's snapshot wins and the client corrects; the hash telemetry shows how often.

### 2.9 Security (no trust in clients, no player data)

- Clients send inputs only, never positions, speeds, laps or results.
- Every decode is bounds-checked (built). New `NetGame::sanitize(&mut Input)` hook clamps each field to its legal range (Sugarpeak: steer and throttle in [-1, 1], known buttons only).
- Rate and size: the server accepts at most `max_input_ahead` ticks of future input (built), at most one input packet per tick per link on average with a small burst, a packet size cap, and drops and counts the rest.
- Protocol hash and wire version are checked on hello (built); a mismatch is refused.
- Spectator links can send nothing that reaches the simulation.
- Results (finishing order, times) come from the referee's run, exported by `results()`.
- The port transport accepts only `ArrayBuffer`/`Uint8Array` payloads up to the cap, from the one `MessagePort` handed over once at start, after checking `event.source` is the parent; it never evaluates, parses HTML from or navigates with relayed data.
- No identity, names, IPs or tokens in the engine (2.2).

### 2.10 Phones and tablets

Same look and same netcode on every device (Chris's rule). Touch controls already drive the same actions as keys, so they produce the same inputs. Costs to watch on Low/Lite: replay work (capped per frame), interpolation (cheap), and no extra allocations per frame (pooled buffers, as the loopback does). A phone on a weak link gets the same snapshots; `net.interp_delay_ms` can rise per tier if K3 shows jitter.

---

## 3. Live API: every tunable is a parameter

All under `net.*`, owner `net`, registered by the module (six exist; the rest are new in N2a/N2b). Each is readable and settable through `evani-live` (`schema_get`, `param_set`, `sweep_run`, `compare_ab`) like any other parameter; `NetStats` feeds `telemetry_sample` and the overlay line.

| Parameter | Default | Range | Applies |
|---|---|---|---|
| `net.send_rate_hz` (exists) | 20 | 1-60 | next snapshot |
| `net.interp_delay_ms` (exists) | 100 | 0-500 | live |
| `net.lag_comp_max_ms` (exists) | 200 (0 for Sugarpeak) | 0-1000 | live |
| `net.loopback_latency_ms`, `_jitter_ms`, `_loss_pct` (exist) | 0 | | live (offline/tests) |
| `net.input_send_every` | 2 | 1-6 ticks | live |
| `net.input_redundancy` | 4 | 1-32 frames | live |
| `net.input_delay_ticks` (built, N2b) | 0 | 0-6 | next session |
| `net.max_catchup_steps` (built, N2b) | 0 (no cap; today's behaviour) | 0, 2-240 | live |
| `net.spectator_delay_ms` | 0 | 0-5000 | live |
| `net.timeout_s` | 10 (0 under a host port) | 0-120 | live |
| `net.max_players` | 8 | 2-64 | next session |
| `net.hash_every_ticks` | 60 | 0-600 (0 off) | live |
| `net.seat_bot_on_leave` | on | bool | live |

`cargo xtask live --game kart-racer` with `net = true` runs a local session over the loopback, so an AI can sweep `net.interp_delay_ms` against the loopback's latency and jitter and read rollbacks and corrections from telemetry. No new MCP tools are needed; `net.*` lands in docs/LIVE_API.md (N2b).

---

## 4. Budgets (lightness first)

Measured, not estimated, in N1 (baseline) and P1 (final). Targets:

| Piece | Budget |
|---|---|
| Any game without `net` | **0 bytes**: SHA-1 identical ship build (P1) |
| Sugarpeak / kart racer with `net = false` after K1's refactor | identical behaviour (recorded-input replay hash equal before and after); wasm change at most +1 KB gz, target 0 |
| `evani-net` client core in a game (Client, wire, interp, spectator) | at most 22 KB gz wasm (baseline measured first in N1; the budget is reset to baseline + 4 KB if the baseline is higher) |
| Port transport | at most 2 KB gz wasm + 1.5 KB gz JS |
| WebSocket transport | JS only, at most 1.5 KB gz, loaded only by games on a WebSocket host |
| Referee build of Sugarpeak (never in the player download) | at most 250 KB raw, 100 KB gz (ChimpVibe's cap: 1 MiB) |
| Up, per player, 8 players | at most 2 KB/s (estimate: 3-byte kart input, a packet every 3 ticks with about 6 frames, about 1.4 KB/s with JSON and base64 overhead) |
| Down, per player, 8 players, 20 Hz full snapshots | at most 10 KB/s (estimate: about 300 bytes binary, 400 base64 per view, about 8 KB/s); delta snapshots (N6) target 4 KB/s |
| Referee CPU | `step(3)` for 8 karts at most 1 ms on a server core |
| Client CPU, Low tier phone | prediction and replay at most 2 ms per frame worst case |

---

## 5. Packages

One worker session each. Sizes: S (about an hour), M (half a session), L (a full session). Every package ends with `cargo xtask check --quick` while working and `cargo xtask check` clean before hand-over; P1 runs `--full`. Branch per package from `net-engine`, merged back into `net-engine` in wave order; `net-engine` joins `m1-foundation` after P1 and Chris's look at the K3 proof. Commit after each step.

### Wave 1 (start together; disjoint files)

| ID | Package | Files owned | Size, worker | Done when |
|---|---|---|---|---|
| **N1** | Tests and determinism guard for the built core. Loopback tests: 2 to 8 clients plus bots stay in sync with the server (every client's confirmed state hash equals the server's on every snapshot), with latency, jitter and 5% loss; prediction reconcile; timeout; refuse on protocol mismatch and full server. New `synctest.rs`: run any `NetGame` twice from one seed and inputs and compare hashes every tick (GGRS's sync-test idea). Measure the current `net` byte cost in the sandbox (`cargo xtask build sandbox --ship` with and without `net`) as the baseline for section 4. | `crates/evani-net/tests/*`, `crates/evani-net/src/synctest.rs`, one export line in `lib.rs`; a toy `NetGame` in tests | M, opus-high | tests pass in `cargo test -p evani-net`; baseline bytes in DECISIONS |
| **K1** | Sugarpeak race simulation out of the ECS: a plain `RaceSim` (karts, items, boxes, phase, standings, `Pcg32`) with `step(&mut self, inputs, rng)`; the ECS systems become thin readers and writers around it. Bring in the slipstream patch from `..\Sugarpeak rally\source\0001-kart-racer-slipstream.patch` first (it is the live Sugarpeak). Proof: record 3 races (1 player, 2 players split-screen, free roam) as input logs before the change, replay after: identical per-tick hash and finishing order. | `kits/evani-kit-kart-racer/src/{race.rs, kart.rs, ai.rs, items.rs, itembox.rs}` plus a new `sim.rs`; `tests/replay_*` | L, opus-xhigh | replay proof passes; kart racer plays the same; `check` clean |
| **T1** | Transports, engine side and JS. `PortTransport` (feature `port`): the neutral `evani-net/1` port contract (section 7) in wasm-bindgen glue plus a tiny JS shim; `WebSocket` connector (feature `ws`, JS outside the wasm). Single-player fallback when no host answers within the hello window. Headless Chrome test page (muted, `--headless=new`, own `--user-data-dir`) with a fake host on the parent frame. | `crates/evani-net/src/transport_port.rs`, `crates/evani-net/web/*.js`, `Cargo.toml` features | M, opus-high | fake-host test round-trips bytes; no network opened in port mode; feature off = no bytes |
| **B0** | Bridge guide and hand-off notes, first draft from this plan's sections 2, 6 and 7 (finalised in B1): Part B for any host, Part C for ChimpVibe (what the bundled bridge does, what they do on their side, testing, what not to change). Done 2026-10-24. | `docs/NET_BRIDGE_GUIDE.md` | S, sonnet-high | draft committed |

Status (2026-10-24): **N1 done** on branch `net-tests` (tests, `evani_net::synctest`, float probe, `check --full` wiring; DECISIONS "Netcode tests and the sync-test guard"). Open from N1: the sandbox `net` byte baseline (no spare cores at the end), and input-delay and catch-up-cap tests, which wait for N2b's code.

### Wave 2 (after N1)

| ID | Package | Files owned | Size, worker | Done when |
|---|---|---|---|---|
| **N2a** | Spectator role (client role, server projection, `spectator_view` hook), checkpoints (`Server::save/load`, bit-for-bit continue test), host-roster joins and leaves, `seat_bot_on_leave`, `NetGame::sanitize`, per-link rate and size caps, `results()` hook. | `crates/evani-net/src/{server.rs, client.rs, msg.rs, game.rs}` | L, opus-xhigh | N1 tests still pass; new tests: spectator sees same state, save/load continues identically, hostile packets (oversize, flood, out-of-range input, spectator sending input) are dropped and counted |
| **N2b** (after N2a) **DONE 2026-10-25 for input delay and catch-up** (`net.input_delay_ticks` default 0, range 0-6, per session; `net.max_catchup_steps` default 0 = no cap, then 2-240, live; defaults are today's shipped behaviour, so the cap default is not the 4 in section 3; tests: 8 players + watcher in sync at delays 0, 2, 4, a far-behind client never exceeds the cap, byte proof unchanged; the other items below were not part of this step) | Pacing for capped hosts: `input_send_every`, `input_delay_ticks`, catch-up cap, snapshot state hash and desync telemetry (`hash_every_ticks`, first diverging tick in `NetStats`); register all new `net.*` params; `docs/LIVE_API.md` net section; overlay line. | `crates/evani-net/src/{client.rs, config.rs, lib.rs}`, `docs/LIVE_API.md` | M, opus-high | tests: 20 messages/s cap keeps 8 clients in sync at 100 ms latency, 30 ms jitter, 2% loss |

### Wave 3 (after N2a; parallel with N2b)

| ID | Package | Files owned | Size, worker | Done when |
|---|---|---|---|---|
| **N3** | Referee build (feature `referee`): the neutral wasm ABI (section 7.2), `cargo xtask build <id> --referee` writes `referee/referee.wasm` beside (never inside) the shipped folder; check rules: a referee build has no imports, no wasm-bindgen, no clock or random imports. `tools/evani-host`: Node, zero dependencies, hand-written WebSocket server, loads `referee.wasm`, steps it on a timer, relays bytes, spectators, optional latency/loss for tests. | `crates/evani-net/src/referee.rs`, `tools/xtask/src/{build,check}*` (referee parts only), `tools/evani-host/*` | L, opus-xhigh | sandbox toy game: host plus 2 Node clients agree on every snapshot hash for 10,000 ticks; `referee.wasm` imports list is empty |
| **K2** (after K1 and N2a) | Sugarpeak as a `NetGame`: `KartInput` (steer i8, throttle i8, buttons u8: drift, item, look back), `KartState` wire (quantised positions mm, angles 16-bit), `KartPose`, server bots = the kit's own AI, kit feature `net` and app feature; lobby on the title screen (host offered: race online; no host: the menu is unchanged); spectator camera that follows any kart; results screen from the referee's results; `lag_comp_max_ms = 0` (no hitscan). Single-player and split-screen run exactly as before with `net` off. | `kits/evani-kit-kart-racer/src/net/*`, kit and app `Cargo.toml`, `apps/kart-racer/evani.toml` gains a `[net]` table (max players 8, spectators on) | L, opus-xhigh | loopback: 2, 4 and 8 karts (humans and bots) stay in sync for 3 full races; sync-test passes on the kart step; replay proof from K1 still passes with `net` off |

### Wave 4 (proof; after K2, N2b, N3, T1)

| ID | Package | Files owned | Size, worker | Done when |
|---|---|---|---|---|
| **K3** | Sugarpeak proof. (1) Native: referee plus 2 headless clients race 3 laps in sync over the loopback at 100 ms, 30 ms jitter, 2% loss; 8 players (2 human, 6 bots; then 8 human). (2) Browser: `tools/evani-host` runs `referee.wasm`; two muted headless Chrome clients race over WebSocket; one spectator; screenshots of both clients and the spectator at the same tick; join mid-race, leave (bot takes over), reconnect as a new join. (3) Same race through the port transport with a fake host page that rate-caps at 30 messages/s and steps the referee every 50 ms (a stand-in for any portal; no partner code). (4) Measure: bytes up and down per player, message rates, referee `step(3)` time, client frame time on the Low tier, rollbacks and corrections per minute; the numbers answer ChimpVibe's question 7. (5) Determinism: snapshot hashes compared across Chrome, Edge, and the leased phone and tablet (Evani.App device lease). | `apps/kart-racer/tests/*`, `.evani/shots/net-k3/*`, results in `docs/NET_PROOF.md` | L, opus-high | every item passes; numbers within section 4 or a written gap with the fix package named |
| **C1** (after N3; parallel with K3) | ChimpVibe adapter, outside the engine (Chris, 2026-10-24: "We write the adapter"). A separate crate `bridges/chimpvibe/rules-shim` that links a game's referee (Sugarpeak's first) and exports ChimpVibe's `chimpvibe-rules/1` ABI (`memory`, `alloc`, `initialize`, `restore`, `step`, `project`; JSON in and out; imports only `chimpvibe_rules.abort` and `.log`), mapping each call onto the neutral referee: `initialize` → `init` + joins, `step` → roster diff to `join`/`leave`, actions (`{type:"in", data:{b: base64}}`) to `input`, then `step(tickMs / 1000 * 60)` (3 for 50 ms), events from `results`; `restore` → `load`; `project` → `view` (spectator recipient → spectator view) wrapped as `{b: base64}`; state carried as `{b: base64 save()}`, rngState a hash of it. Plus `bridges/chimpvibe/play-adapter.js`: the reference browser half (their frame's `MessageChannel` ↔ `evani-net/1`, actions ↔ `data`), given to ChimpVibe to adapt into their pinned frame code. Built by `cargo xtask build kart-racer --referee --bridge chimpvibe` into `bridges/chimpvibe/out/`, never into the game folder shipped to Evani.app or other portals; it goes into the ChimpVibe hand-off package (`game/rules/rules.wasm`, `bridge/play-adapter.js`, rules source, `HANDOFF.md`; layout in docs/NET_BRIDGE_GUIDE.md Part C), which is Sugarpeak Racer with the bridge in it. Not a workspace default member; no other game or engine crate depends on it. Tests: a Node harness that drives `rules.wasm` exactly as their reply-4 describes (JSON calls, 50 ms steps, spectator `project`, `restore` mid-race) agrees with `tools/evani-host` on every snapshot hash; imports list is exactly the two allowed; size under 1 MiB; JSON limits (state 1 MiB, view 128 KiB, action 4 KiB) checked. | `bridges/chimpvibe/*` only; `tools/xtask` gains only the generic `--bridge <dir>` hook (no partner names in xtask) | L, opus-xhigh | harness passes; `grep -ri chimpvibe crates/ tools/xtask/` is empty; P1's byte proof unchanged |
| **B1** | Bridge guide final (section 7 frozen against N2b, N3 and T1's real code): messages, ABI, how to run the referee, spectators, IDs and sessions on the host's side, testing with the loopback and `tools/evani-host`, a worked mapping for a "rules-module host" (ChimpVibe's `initialize/restore/step/project` as the example), checklist. Written for a partner developer; no engine code. | `docs/NET_BRIDGE_GUIDE.md` | M, sonnet-high | every call in the guide exists in code; the fake-host example from K3 runs from the guide's steps |

### Wave 5 (close)

| ID | Package | Files owned | Size, worker | Done when |
|---|---|---|---|---|
| **P1** | Byte and compatibility proof: ship builds of every game (`kart-racer` with `net = false`, platformer, match-3, top-down, RTS, tactical-fps, sandbox) SHA-1 identical to `m1-foundation` except the kart racer's K1 delta (within budget); size per module for kart racer with `net = true` (client) and its referee; `cargo xtask check --full`; DECISIONS entries (model choice, ABI, bytes, no new dependencies); README "Multiplayer" section; STAGES.md notes; `apps/kart-racer` stays `net = false` in the shipped Evani.app build until Chris says otherwise. | DECISIONS.md, README.md, STAGES.md | M, sonnet-high | proof table committed; `check --full` clean |
| **X1** | Neutralise the unmerged Broadside 3D host code before that branch merges: rename `CHIMPVIBE`/ChimpVibe limits out of `crates/evani-net/src/host.rs`, move it onto the `evani-net/1` port contract (or drop it for `PortTransport`), move ChimpVibe's `chimpvibe-rules/1` shim into `bridges/chimpvibe/` beside C1's adapter (one ChimpVibe folder, never `crates/`). Coordinate with that branch's owner. | `game-broadside-3d` branch only | S, opus-high | `grep -ri chimpvibe crates/` is empty on that branch |

### Later, only if measurements ask for it

| ID | Package | When |
|---|---|---|
| **N6** | Delta snapshots (the reserved baseline body). | K3 shows down-bandwidth over 10 KB/s at 8 players, or a host asks |
| **N7** | WebTransport and WebRTC transports (JS, optional features). | A host wants unordered datagrams; Safari support confirmed from a primary source |
| **N8** | Lockstep relay mode. | N6 still misses the budget for a big-world game |

### Parallel map

```
Wave 1:  N1 ─┐   K1 ───────────┐   T1 ─────────┐   B0
Wave 2:  N2a ┴─> N2b ──────────│───────────────│──┐
Wave 3:      N3 (after N2a)    K2 (after K1+N2a)  │
Wave 4:                 K3 (after K2, N2b, N3, T1), B1 (with K3), C1 (after N3; Sugarpeak test after K2)
Wave 5:                 P1;  X1 any time after T1 (other branch)
```

Runs together: N1, K1, T1, B0 (wave 1); N2b, N3, K2 (once N2a is in). Strictly in order: N1 → N2a → N2b; K1 → K2 → K3 → P1.

Computer safety for builds: `CARGO_BUILD_JOBS=1`, through `run-low.mjs`, `resource_check` first; at most two packages building at once on this PC.

---

## 6. ChimpVibe's eight points

Legend: **Engine** = built into the neutral engine layer (every host benefits, zero bytes for games without `net`). **Bridge** = handled on the host's side, by the host's own connector. **Off-limits** = we will not build it.

| # | Their point | Where | What we do |
|---|---|---|---|
| 1 | No `connect-src 'self'`, no separate static origin; session traffic through their shell, the game gets no network | **Bridge** (their frame) | We drop our 4.1 request. The engine's port transport opens no network: every byte goes through the `MessagePort` their frame hands over. Fine for every host. |
| 2 | The bridge is in their frame's start-up code, pinned by hash, not an injected script | **Bridge** | Nothing to inject on our side: the game waits for an `evani-net/1` hello carrying a port (section 7.1). No global like `window.chimpvibeBridge` is needed. |
| 3 | Watchers (spectators) required for bridge games | **Engine** | Spectator role (N2a), shown in K3. Their server asks the referee for the spectator view. |
| 4 | No host-run physics sanity block | **Engine** (nothing to add) | Agreed. The referee runs the real simulation from inputs, so no sanity numbers are needed. Input range checks are inside the game (`NetGame::sanitize`). |
| 5 | No 60-second idle kick | **Engine** | Agreed. The engine never removes an idle player; under a host port `net.timeout_s` is 0 and the host's roster is the truth. |
| 6 | No signed member tokens; anonymous, random ID per join | **Bridge** | IDs and sessions are the host's. The engine has no identity at all: a peer is a slot number per join. A reconnect is a new join; a bot finishes a leaver's kart. |
| 7 | Server is the referee; actions carry only inputs (steer, throttle, drift); AI racers run in the referee | **Engine** | Already the built model. Sugarpeak sends a 3-byte input (steer, throttle, buttons). Bots run in the referee. Answers their questions 1 and 5: yes. |
| 8 | For physics games the referee is a small rules wasm on their server, exact and repeatable; a JavaScript referee meanwhile, while actions are inputs | **Engine** (referee) + **Bridge** (our ChimpVibe adapter, C1, outside the engine); JS referee **off-limits** | The neutral referee build (N3): wasm32, no imports, no clock, no randomness from outside except the seed. Our adapter in `bridges/chimpvibe/` (C1) wraps it as their `chimpvibe-rules/1` `rules.wasm`, so their server loads it unchanged. A JavaScript copy of the kart physics is off-limits: a second copy of the rules would drift, and we declined it before. The referee arrives in wave 3, so no interim is needed. |

Their seven questions, answered from this plan (K3 supplies the numbers):
1. Inputs only: yes (point 7).
2. Physics as a separate referee wasm with no graphics, clock or outside randomness, matching the browser exactly: yes, the same Rust simulation compiled to wasm; K3 tests bit-for-bit agreement across browsers and devices, and if any bit ever differs the server's view wins, so play never breaks.
3. A JavaScript referee: not needed (point 8).
4. Watchers with Sugarpeak: yes (K2, K3).
5. AI racers in the referee: yes.
6. Starting in their frame with no network: yes, with the port transport.
7. Bandwidth and tick rate with eight players: measured in K3 (estimate: about 1.4 KB/s up and 8 KB/s down per player at 20 views a second).

State between calls: their `step` passes the state in and out every call. Our adapter (C1) keeps the referee alive inside the instance and returns a `save()` copy as the state; when the state it gets back is not the one it last returned (a restarted or moved Session), it calls `load` first. Cost per call (a few KB) measured in C1.

---

## 7. The neutral contracts (frozen in N2b, N3 and T1)

### 7.1 Browser: `evani-net/1` over a MessagePort

The host page creates a `MessageChannel`, sends one port to the game frame once, and talks only on that port. Payloads are plain objects; bytes are `Uint8Array`.

Host to game: `{t:"hello", v:1, role:"player"|"spectator", port}` (on `window`, once; the game checks `event.source === parent`), then on the port: `{t:"data", b}` (one server packet), `{t:"status", s:"live"|"reconnecting"|"closed"}`, `{t:"pause"}`, `{t:"resume"}`, `{t:"caps", maxSendPerSec, maxBytes}`.
Game to host, on the port: `{t:"ready", v:1, protocol}` (the game's `NetGame::PROTOCOL`), `{t:"data", b}` (one client packet), `{t:"leave"}`.
No hello within the window: the game plays single-player, exactly as today.

The host maps `data` to its own session in both directions (for example base64 in a JSON action and a JSON view). The engine never sees how.

**Built (branch `sugarpeak-mp`, 2026-10-24)**, `crates/evani-net/src/port.rs` + `web/port.js`, feature `port`:
- **Two ways in.** (1) A real host frame: `{t:"hello", v:1, role, follow?}` posted on the game's window with the port in the transfer list (or as `port`), accepted only when `event.source === window.parent` and the page is not its own parent. (2) An in-page adapter (a bridge in the same page, which must never be trusted through `window` messages): the explicit registration call **`globalThis.evaniNet.attachPort(port, {role: "player" | "spectator", follow})`**, or, before the game has loaded, **`globalThis.evaniNetPorts.push([port, opts])`** (an array the game drains at load). The first port wins; later ones are refused.
- **The plain byte pipe** (for games that don't use the netcode, e.g. a JSON rules module): Rust **`evani_net::port::HostPort`** with `status()`, `role()`, `follow()`, `paused()`, `max_send_per_sec()`, **`recv(&mut Vec<u8>) -> Option<bool /*reliable*/>`**, **`send(&[u8])`** (datagram: `{t:"data", b}`), **`send_reliable(&[u8])`** (`{t:"rdata", b}`, the host delivers in order), `ready(protocol)`, `leave()`; it also implements `Transport`. Extra host messages: `{t:"follow", k}` (a spectator follows seat `k`).
- Payloads are copied as bytes only, at most 128 KiB (or the host's `caps.maxBytes`), 512 queued.

The model built for Sugarpeak is lockstep with a referee (DECISIONS.md "Multiplayer: lockstep with a referee"), so the `data` bytes are lockstep inputs (up) and views (down: confirmed frames, a state hash, the full state only on join or repair, and for spectators a state plus the frames since).

### 7.2 Server: the referee ABI (`evani-referee/1`)

A wasm32 module with no imports, exporting `memory` and:

| Export | Meaning |
|---|---|
| `evani_ref_abi() -> u32` | 1 |
| `evani_ref_alloc(len) -> ptr` | scratch buffer for the next call's input |
| `evani_ref_init(seed_lo, seed_hi, cfg_ptr, cfg_len) -> i32` | new session; config bytes (max players, spectators, send rate); 0 or a negative error |
| `evani_ref_join(peer, role) -> i32` / `evani_ref_leave(peer) -> i32` | roster changes from the host (role 0 player, 1 spectator) |
| `evani_ref_input(peer, ptr, len) -> i32` | one client packet exactly as the client sent it |
| `evani_ref_step(ticks) -> i32` | advance whole 1/60 s ticks (3 for a 50 ms host step) |
| `evani_ref_view(recipient) -> i64` | `(ptr << 32) \| len`: the packet for that peer, or for a spectator with `recipient = 0xFFFFFFFF`; valid until the next call |
| `evani_ref_save() -> i64` / `evani_ref_load(ptr, len) -> i32` | checkpoint out and in |
| `evani_ref_results() -> i64` | the game's results bytes (finishing order, times), from the referee's own run |
| `evani_ref_hash() -> i64` | state hash, for host-side logging |

A host that wants different names or JSON wraps this in its own glue (JavaScript on its server, or a small shim crate kept in the host's own repository or `bridges/<host>/`, never in `crates/`). For ChimpVibe we write that shim ourselves (C1, `bridges/chimpvibe/`); it is the worked example in the guide.

---

## 8. Prior art (ideas only; no code copied, no new dependencies)

| Source | Licence | Borrowed |
|---|---|---|
| Gaffer on Games, "Deterministic Lockstep" and the networking series | article | redundant inputs with acks, playout buffer, catch-up cap, periodic state hashes |
| GGRS (gschup/ggrs) | MIT OR Apache-2.0 | sync-test mode; save/load/advance request shape (our `save/load/step`); spectator idea. No dependency. |
| Matchbox | MIT OR Apache-2.0 | WebRTC signalling shape for the later N7. No dependency. |
| Lightyear, Naia, web-transport-rs | MIT OR Apache-2.0 | input-delay settings; per-player scope; one transport trait with several backends. No dependency. |
| Discord Activities, Poki, YouTube Playables docs | platform docs | iframe handshake, no outside network, pause/resume messages |
| Playgama Bridge | LGPL-3.0 | nothing (never ship) |

The research report: `.vibedeck/research/2026-10-24-multiplayer-net-module-for-evani-dev.md`.

---

## 9. Open for Chris

1. **Model choice** (section 2.1): keep the built referee-and-snapshots model (recommended, taken) or switch to lockstep as the coordinator relayed. One line from Chris overrules.
2. **ChimpVibe's server glue: decided** (Chris, 2026-10-24: "We write the adapter", then "Let's just write the bridge and then we'll hand them over the full game with the bridge in it and tell them to make it work."). We build and maintain a small ChimpVibe-only bridge in `bridges/chimpvibe/`, outside the engine (package C1). It translates between the neutral `evani-referee/1` / `evani-net/1` contracts and ChimpVibe's `chimpvibe-rules/1` and shell messaging, and ships inside the Sugarpeak Racer package we hand over, with docs/NET_BRIDGE_GUIDE.md Part C as the hand-off notes. ChimpVibe makes it work on their side (their frame and server glue) and changes nothing inside the engine or the game. The engine and every other game stay free of it. Recorded in DECISIONS.md.
3. Sugarpeak's shipped Evani.app build stays `net = false` until Chris has played the K3 proof.
