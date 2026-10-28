# Evani.Dev multiplayer: bridge guide and hand-off notes

> **What is built (branch `sugarpeak-mp`, 2026-10-24), read this first.** This guide is the first draft (B0) and still describes the earlier snapshot design in Part B. What was built for Sugarpeak Rally is **lockstep with a referee**: players send only inputs, the referee relays every seat's confirmed inputs plus a state hash, and the full state only on join, repair and for spectators (DECISIONS.md "Multiplayer: lockstep with a referee"). The parts marked **[not built yet]** below are now built under these names: the page contract `evani-net/1` (`crates/evani-net/src/port.rs`, `web/port.js`: a parent frame's `hello`, or `globalThis.evaniNet.attachPort(port, {role, follow})`; the byte pipe `HostPort`; the general web mode `?evani-server=wss://...`, JSON under 4 KB); the referee ABI `evani-referee/1` (`crates/evani-net/src/referee.rs`, Sugarpeak's in `apps/kart-racer/referee`, README there); spectators; and the ChimpVibe bridge (`bridges/chimpvibe/`, `HANDOFF.md` there). Where this draft and `bridges/chimpvibe/HANDOFF.md` differ, the hand-over notes are right. The final rewrite is package B1.
>
> **Hosts run the shipped referee `.wasm` as-is, never a native recompile or a port.** The kart step uses `sin`, `cos`, `atan2`, `sqrt` and `powi`; those match bit for bit only between WebAssembly builds of the same code (the players' game and the referee are both wasm; a native build's maths library differs). Proven: browser game against `rules.wasm` and against `referee.wasm`, every hash check passed.

First draft (package B0 of `docs/NET_BUILD_PLAN.md`), 2026-10-24. It is finalised in package B1, once the parts marked **[not built yet]** exist and every call below has been checked against real code.

Who this is for: a game platform ("host") that wants to run Evani.Dev games with online multiplayer, and the developers who connect it. Part A is for everyone. Part B is for any host. Part C covers ChimpVibe, the first host, and the bridge we deliver with Sugarpeak Rally.

---

## Part A. One page in plain words

**What we hand over.** Sugarpeak Rally with online racing built in, plus a small "bridge": a translator, written by us, that lets the game talk to ChimpVibe's website and servers in ChimpVibe's own format. The bridge sits next to the game, not inside our engine, so the engine stays the same for every website that runs our games.

**How a race works.**
- Every player's game sends only what the player presses (steer, accelerate, drift, use item). Never positions, speeds or lap times.
- One copy of the race runs on the host's server: the **referee**. It takes everyone's presses, runs the real race, and sends every player and watcher a picture of the race 20 times a second.
- Each player's game shows their own kart instantly and quietly corrects itself if the referee disagrees. Computer racers are driven by the referee.
- The finishing order and times come from the referee's run, so nobody can cheat by changing their own game.

**What the engine does.** The racing, the netcode, the referee build, watchers (spectators), players joining and leaving, and tests to prove it all stays in step.

**What the host does.** Accounts, names, rooms, matchmaking, moving bytes between the players' games and the referee, and running the referee on its servers. The engine never knows who a player is: it only sees "seat 1", "seat 2" and so on.

**What we will not do.** Put any one platform's code inside the engine; trust anything a player's game says beyond its button presses; write a second copy of the race in JavaScript.

**What the host doesn't need to do.** Change anything inside the engine or the game to host it. If hosting needs a change, tell us and we make it for everyone, so every host runs the same build. This is about hosting only: it is not a limit on adapting the game. Anyone may read, adapt and remix the game under its own licence (MIT for Sugarpeak Rally), and the engine is MIT too (docs/LICENSING.md).

**Where things stand (2026-10-24).** The netcode core is built (about 3,350 lines, in the engine since 20 October). Still being built, in this order: tests, watchers and checkpoints, the connection to a host page, the referee build, Sugarpeak's online mode, the ChimpVibe bridge, and a full test race. Parts of this guide marked **[not built yet]** describe that work as planned.

---

## Part B. For any host

### B1. Who builds what

| Piece | Built by | Where it lives |
|---|---|---|
| The game's simulation (`NetGame`), netcode (`Server`, `Client`), packet format | Evani (engine) | `crates/evani-net`, the game's kit |
| Browser connection: the game side of the `evani-net/1` page contract | Evani (engine) | `crates/evani-net`, feature `port` **[not built yet: package T1]** |
| Referee build of a game: `referee.wasm`, ABI `evani-referee/1` | Evani (engine) | `crates/evani-net`, feature `referee`; built with `cargo xtask build <id> --referee` **[not built yet: package N3]** |
| Reference host for testing: `tools/evani-host` (Node, no dependencies) | Evani (dev tool, never shipped) | `tools/evani-host` **[not built yet: package N3]** |
| The host page that frames the game and relays bytes | Host | host's own code |
| The host server that runs `referee.wasm` and relays bytes | Host | host's own code |
| Accounts, names, rooms, matchmaking, reconnect policy, idle policy | Host | host's own code |
| A translator from `evani-referee/1` to the host's own server format, if it wants one | Host, or Evani by agreement | outside `crates/`: in the host's repository or `bridges/<host>/` |

Hard rules for the engine side: no platform names, no platform behaviour, no identity, and zero bytes for games that do not list `net = true` in `evani.toml`.

### B2. The model in one paragraph

Server-authoritative with client prediction. Clients send **inputs only**. The referee owns the true state, steps it at a fixed 60 Hz with every peer's input, and sends each client a **snapshot** (the whole state plus everyone's last input) every `snapshot_every` ticks (default 3, so 20 a second). A client predicts its own player from its own inputs, keeps a checksum of every predicted tick, and replays from the snapshot when the checksums differ. Remote entities are drawn interpolated slightly behind. Late or lost inputs are filled by repeating the peer's last input. Bots are peers that live inside the referee. Everything is deterministic: no wall clock, randomness only from the session seed and the tick (`StepCtx::rng`).

### B3. The packets (built: `crates/evani-net/src/msg.rs`, `WIRE_VERSION = 1`)

A host never needs to read these: it moves them as opaque bytes. They are listed so a host developer can recognise them in logs and size its limits. Every packet starts with one kind byte; numbers are LEB128 varints unless noted.

| Kind | Byte | Direction | Body |
|---|---|---|---|
| `Hello` | `0x10` | client to referee | wire version (varint), protocol hash (fixed u64: FNV-1a 64 of the game's `NetGame::PROTOCOL`, e.g. `"hexagon/1"`). Resent until answered. |
| `Input` | `0x11` | client to referee | ack snapshot tick (optional tick), newest tick, frame count (at most 32), then frames newest first, each a view time plus the game's input. Inputs are repeated in several packets, so a lost packet costs nothing. |
| `ClientBye` | `0x12` | client to referee | none: leaving. |
| `Welcome` | `0x20` | referee to client | peer id, referee tick, session seed (fixed u64), ticks between snapshots. |
| `Snapshot` | `0x21` | referee to client | tick, ack input (optional tick), slack (zigzag), baseline (optional tick, always none in this version), then every peer's last input, then the whole state. |
| `Refuse` | `0x22` | referee to client | reason byte: 1 protocol mismatch, 2 full. |
| `ServerBye` | `0x23` | referee to client | none: dropped or closing. |

Sizes: an input packet is about 8 bytes plus a few bytes per frame; a snapshot is about 8 bytes plus about 11 bytes per peer plus the game's state. Sugarpeak's estimate: about 300 bytes per snapshot for 8 karts (measured in package K3).

Every decode is bounds-checked; a malformed packet is counted (`ServerStats::bad_packets`) and dropped. A wrong wire version or protocol is refused at `Hello`.

### B4. In the browser: the `evani-net/1` page contract **[not built yet: package T1; shape from plan section 7.1]**

The game runs in a frame on the host's page. It opens **no network connection of its own** in this mode: every byte goes through one `MessagePort` the host page hands over.

**Start-up**
1. The host page creates a `MessageChannel`.
2. It posts, on the game frame's `window`, once: `{t:"hello", v:1, role:"player"|"spectator"}` with `port2` transferred. The game accepts it only if `event.source === window.parent`.
3. The game answers on the port: `{t:"ready", v:1, protocol}` (its `NetGame::PROTOCOL` string). The host may refuse to continue on an unknown protocol.
4. No hello within the start-up window: the game plays single-player exactly as it does today. A standard build on any portal is unaffected.

**Messages on the port**

| From | Message | Meaning |
|---|---|---|
| host | `{t:"data", b}` | one referee packet (`b` is a `Uint8Array`) |
| host | `{t:"status", s:"live"\|"reconnecting"\|"closed"}` | the host's link state, for the game's UI |
| host | `{t:"pause"}` / `{t:"resume"}` | tab hidden, ad break, host menu: the game stops sending and sound |
| host | `{t:"caps", maxSendPerSec, maxBytes}` | the host's limits; the game paces its input packets to fit |
| game | `{t:"data", b}` | one client packet for the referee |
| game | `{t:"leave"}` | the player chose to leave |

What the contract deliberately does **not** have:
- **No rooms or peer lists.** The game never picks a room, sees who else is in it, or learns a name. The host decides the room before the game starts and tells the referee who joined.
- **No "reliable" flag.** The netcode copes with lost, late or reordered packets (inputs are sent redundantly, snapshots are whole). A host may deliver reliably and in order (a WebSocket) or not; both work.
- **No identity.** No account id, token, name or IP reaches the game through the port.

The host maps `data` onto its own session in both directions (for example base64 inside a JSON action and a JSON view). The engine never sees how.

### B5. On the server: the referee **[not built yet: package N3; ABI from plan section 7.2]**

`cargo xtask build <id> --referee` writes `referee/referee.wasm` beside (never inside) the shipped player folder. It is the same Rust simulation as the player's game, compiled to `wasm32-unknown-unknown` with no graphics, no clock and no imports. Any wasm runtime runs it (Node, Deno, wasmtime...).

ABI `evani-referee/1`: a wasm32 module with **no imports**, exporting `memory` and:

| Export | Meaning |
|---|---|
| `evani_ref_abi() -> u32` | 1 |
| `evani_ref_alloc(len) -> ptr` | scratch buffer for the next call's input |
| `evani_ref_init(seed_lo, seed_hi, cfg_ptr, cfg_len) -> i32` | new session; config bytes (max players, spectators, send rate); 0 or a negative error |
| `evani_ref_join(peer, role) -> i32` | the host seats a peer (role 0 player, 1 spectator) |
| `evani_ref_leave(peer) -> i32` | the host removes a peer |
| `evani_ref_input(peer, ptr, len) -> i32` | one client packet exactly as the client sent it |
| `evani_ref_step(ticks) -> i32` | advance whole 1/60 s ticks |
| `evani_ref_view(recipient) -> i64` | `(ptr << 32) \| len`: bytes to deliver to that peer; `recipient = 0xFFFFFFFF` for the spectator view; valid until the next call |
| `evani_ref_save() -> i64` / `evani_ref_load(ptr, len) -> i32` | checkpoint out and in |
| `evani_ref_results() -> i64` | the game's results (finishing order, times) from the referee's own run |
| `evani_ref_hash() -> i64` | state hash, for host-side logging and desync checks |

A peer number is a small integer the host assigns per join (it becomes the netcode's `PeerId`, a `u16`). Open point for N3: whether `evani_ref_view` returns one packet or several length-prefixed packets (a `Welcome` and a `Snapshot` can be due on the same step); B1 records the final answer.

**How a host runs it, per room:**
1. Instantiate `referee.wasm`. Call `evani_ref_init` with a seed (any 64-bit number; the host picks it, per session) and the config.
2. When a player joins the room: pick a free peer number, call `evani_ref_join(peer, 0)`. Watchers: `evani_ref_join(peer, 1)`, or just read the spectator view.
3. Every host tick: for each packet received from a player's page, `evani_ref_input(peer, bytes)`; then `evani_ref_step(n)` where `n` = host tick length in 1/60 s (a 50 ms tick is `n = 3`; a 1/60 s tick is `n = 1`; any multiple of 50 ms works); then for each player `evani_ref_view(peer)` and send the bytes to that player's page; send the spectator view to every watcher.
4. When a player leaves (closed tab, kicked, timed out by the host): `evani_ref_leave(peer)`. The game decides what happens to their seat (Sugarpeak: a bot finishes the kart).
5. At the end: `evani_ref_results()` for the official result.
6. To move or restart a room: `evani_ref_save()`, later `evani_ref_load()` in a fresh instance. It continues bit for bit.

Until N3 lands, the same roles exist as Rust types: `Server::new(state, transport, cfg, seed)`, `Server::poll()` then `Server::tick(&game)` once per tick, `Server::add_bot`, `Server::remove_peer`, with any `Transport` (`send(to, packet)`, `recv(buf) -> Option<LinkId>`, `closed(out)`).

### B6. Watchers (spectators) **[not built yet: package N2a]**

- The referee makes one spectator view per step: the whole state with nothing private (a game with secrets hides them in its `spectator_view` hook; Sugarpeak has none).
- Watchers send nothing. Anything a spectator link sends is dropped and counted.
- Every view is complete on its own, so a watcher who falls behind just skips some.
- A watcher may follow any player's camera; it is the same view.
- The page tells the game it is a watcher with `role:"spectator"` in the hello; the game then takes no input and mutes sound if the host asks.
- `net.spectator_delay_ms` (default 0) delays what watchers see, for hosts that want it.

### B7. Join, leave, reconnect, idle

- **Join mid-session** (built: `StepCtx::joined`): the referee seats the peer, and its first snapshot brings the game up to date. The game decides what a late joiner does (Sugarpeak: watches until the next race starts).
- **Leave** (built: `Server::remove_peer`, `StepCtx::left`): the host's roster is the truth. `net.seat_bot_on_leave` **[not built yet: N2a]** lets a bot take the seat.
- **Reconnect:** the host's choice. With anonymous per-join ids, a reconnect is simply a new join. With stable ids, the host may give a returning player its old peer number; that mapping lives in the host or its bridge, never the engine.
- **Idle:** the engine never removes an idle player. `net.timeout_s` only drops a link that went fully silent on a transport the engine itself owns (our test WebSocket host). Under a host port it is 0 (off): the host's roster and the host's own idle setting decide.
- **Full room:** the referee refuses with reason 2 when `max_peers` (bots included) is reached.

### B8. Identity and sessions belong to the host

The engine has no accounts, names, tokens or IP addresses, and never will. A peer is a seat number for one join. If a host shows names, it draws them in its own interface or passes them to the game as plain text through its own channel; the netcode never carries them. Logs from the referee contain peer numbers only.

### B9. Settings a host may tune

All are live parameters (`net.*`); a host passes its choices in the referee config or the `caps` message.

| Setting | Default | Notes |
|---|---|---|
| `net.send_rate_hz` (built) | 20 | snapshots per second |
| `net.interp_delay_ms` (built) | 100 | how far behind remote karts are drawn |
| `net.input_send_every` **[N2b]** | 2 ticks | 3 under a host that caps at 30 messages a second |
| `net.input_redundancy` | 4 frames | repeated inputs per packet |
| `net.spectator_delay_ms` **[N2a]** | 0 | |
| `net.timeout_s` **[N2b]** | 10, 0 under a host port | the host's roster decides under a port |
| `net.max_players` **[N2b]** | 8 | bots included |
| `net.seat_bot_on_leave` **[N2a]** | on | |
| `net.input_delay_ticks` (lockstep, built) | 0 | 0-6 ticks. A player's input applies this many ticks after it is read (0 = the kart reacts at once; prediction hides the rest). Read when a session starts; changing it mid-session waits for the next one. Tested in sync at 0, 2 and 4 with 8 players and a watcher at 100 ms, 30 ms jitter, 2 % loss. |
| `net.max_catchup_steps` (lockstep, built) | 0 (no cap) | 0, or 2-240 steps (1 counts as 2). The most game steps one tick runs to catch up: a player's prediction replay after a correction, a spectator's backlog. The rest waits for the next ticks, so a slow host spreads the work over frames. Live. 0 is how the client always ran. |

### B10. Rules for every host

1. **Inputs only.** A host relays bytes; it never adds, edits or invents game data (positions, speeds, laps, scores).
2. **Never trust clients.** The referee checks every packet and clamps every input to its legal range (`NetGame::sanitize` **[N2a]**). A host should still cap message size and rate per player and drop the excess.
3. **Results come from the referee's run** (`evani_ref_results`), never from a player's page.
4. **No host code inside the engine.** Hosts never patch `crates/`, the game's kit, `referee.wasm` or the player build. Translators live in the host's code or `bridges/<host>/`. Ask us for changes; they go into the engine for every host.
5. **Keep compatibility.** The same game build runs on every portal; a standard build without a host port plays single-player as before. A host must not require changes that break another host.
6. **No outside randomness or clock in the referee.** Only the seed you pass at `init`.
7. **Send the game's own bytes untouched.** Base64 or JSON wrapping is fine as long as the bytes come out identical.

### B11. Testing a bridge

| Step | How | Available |
|---|---|---|
| 1. Netcode in one page, no network | `LocalSession` (server, one client and the `Loopback` in the same page). Add `net.loopback_latency_ms`, `_jitter_ms`, `_loss_pct` to simulate a bad line; the loss is seeded, so a failing run repeats exactly. | built |
| 2. Sync tests | `cargo test -p evani-net`: 2 to 8 clients plus bots agree with the referee on every snapshot hash under latency, jitter and 5% loss; sync-test runs any game twice and compares every tick. | package N1 |
| 3. Your host page against a fake referee | Point your page at `tools/evani-host` (WebSocket) or a fake host page that rate-caps messages; check the game reaches `ready`, sends `data`, and shows the race. | packages T1, N3 |
| 4. Your server against the reference host | Run the same seed and the same recorded inputs through your server and through `tools/evani-host`; `evani_ref_hash()` must match on every step. | package N3 |
| 5. Sugarpeak Rally end to end | The Sugarpeak multiplayer build: two muted headless browsers race 3 laps, one watcher, join mid-race, leave (a bot takes over), reconnect as a new join; bytes, rates and timings recorded in `docs/NET_PROOF.md`. | packages K2, K3 |

Pass condition for a bridge: step 4 shows identical hashes for a full race, and step 5 runs through your page and server.

### B12. Worked example: one race

Host tick 50 ms (`n = 3`), two players (peers 1 and 2), six bots in the referee, one watcher.

| When | Player page | Host | Referee |
|---|---|---|---|
| Lobby | | Matches two players into a room; picks a seed. | `init(seed)`, game adds 6 bots. |
| Join | Game frame loads; host page posts `hello` with a port. Game replies `ready` with `"sugarpeak/1"` (protocol name set in K2). | Checks the protocol; `join(1, 0)`, `join(2, 0)`. | Seats 1 and 2 on the grid (`StepCtx::joined`). |
| Handshake | Game sends `Hello` (`0x10`) as `data`. | Relays it: `input(1, bytes)`. | Answers `Welcome` (`0x20`: peer, tick, seed, snapshot interval) in peer 1's next view. |
| Countdown | Game draws the grid and the lights from snapshots; inputs are sent but the race state ignores throttle until "go". | Every 50 ms: relay inputs, `step(3)`, send each view. | Counts down inside the state; every snapshot carries the countdown. Nothing special on the wire. |
| Racing | Each frame: read keys or touch, predict own kart, every 2 to 3 ticks send an `Input` (`0x11`) with the newest frames plus any the referee has not acked. | Relays. | Uses each peer's input for its tick, or repeats the last one; bots think; steps; sends `Snapshot` (`0x21`) every 3 ticks. |
| Correction | A snapshot's checksum differs from the predicted tick: the game replays from it. No message. | | |
| Watcher | Watcher page gets `hello` with `role:"spectator"`; draws from the spectator view; sends nothing. | Sends the spectator view each step. | One spectator view per step. |
| A player drops | | Roster changes: `leave(2)`. | Removes peer 2; with `seat_bot_on_leave` a bot finishes kart 2. |
| Finish | Game shows the results screen drawn from the state. | Calls `results()` and stores the official result. | Finishing order and times from its own run. |
| End | Player leaves: game sends `{t:"leave"}`. | Closes the room; may `save()` first for a replay. | |

---

## Part C. ChimpVibe: the bundled bridge

Decision (Chris, 2026-10-24): "We write the adapter", then "Let's just write the bridge and then we'll hand them over the full game with the bridge in it and tell them to make it work." So **we** write a small ChimpVibe bridge, it ships inside the Sugarpeak Rally package we hand over, and it lives in `bridges/chimpvibe/`, outside the engine. No engine crate, and no other game, contains or depends on it.

Status: the bridge is package C1 of the build plan **[not built yet]**. Its design follows ChimpVibe's `chimpvibe-rules/1` description (reply 4) and the neutral contracts above.

### C1. What we hand over

Laid out the way ChimpVibe's uploads already work (`game/` folder):

| Path in the package | What it is |
|---|---|
| `game/manifest.json` | Your manifest, with `"rules": { "addon": "chimpvibe", "abi": "chimpvibe-rules/1", "module": "rules/rules.wasm" }`, `"schedule": { "mode": "fixed", "tickMs": 50 }`, the one action type `"in"`, and spectators on. |
| `game/rules/rules.wasm` | The Sugarpeak referee with our ChimpVibe bridge linked in. Exports exactly your ABI: `memory`, `alloc`, `initialize`, `restore`, `step`, `project`. Imports only `chimpvibe_rules.abort` and `chimpvibe_rules.log`. Players never download it. |
| `game/client/` | The Sugarpeak player build (same game as on other portals, with the `port` connection compiled in). |
| `game/assets/` | Pictures, models and sounds. |
| `bridge/play-adapter.js` | The browser half of the bridge: our reference code for your play frame (see C3). |
| `rules-source/` | The source of everything in `rules.wasm` (game rules, netcode, bridge), as your point 8 asks. |
| `HANDOFF.md` | This Part C, plus the test steps and the build's measured numbers. |

Licence note: `rules.wasm` contains Evani engine code (the netcode core and its random number generator), not only game rules. Running it on ChimpVibe's servers is covered by the hosting agreement between Evani and ChimpVibe (your points 1 and 2).

### C2. What the bundled bridge does (server half, inside `rules.wasm`)

It maps your four JSON calls onto the neutral referee. You load it with your existing server glue, unchanged.

| Your call | What the bridge does |
|---|---|
| `initialize { roster, time, config, seed }` | `evani_ref_init` with your 32-bit seed; seats each roster player (`join`); returns `{ state, rngState }`. |
| `step { state, commands, roster, time, config, rngState }` | If `state` is not the one it returned last time (a restarted or moved Session), it `load`s it first. Then: `join` and `leave` commands become `evani_ref_join` / `evani_ref_leave`; each action `{ type:"in", data:{ b:"<base64>" } }` becomes `evani_ref_input` with the decoded bytes; then `evani_ref_step(tickMs / 1000 * 60)` (3 for 50 ms). Returns the new `state`, `events` and `rngState`. |
| `restore { state, stateVersion, roster, time, config }` | `evani_ref_load`; returns the state unchanged; uses no randomness. |
| `project { state, recipient, roster, time, config }` | `evani_ref_view(peer)` for a player, or the spectator view for `recipient = { id: null, role: "spectator" }`, wrapped as `{ b: "<base64>" }`. The spectator view never fails and holds nothing private. |

Details:
- **State** is `{ b: "<base64 of save()>" }`; **rngState** is a non-zero 32-bit hash of it (the referee's generator is seeded from your seed and kept inside the state, as your reply allows).
- **IDs:** the bridge keeps a table from your roster ids to seat numbers, inside the state. Names in the roster are ignored: they never enter the engine.
- **Events:** the result at the finish, `{ type: "results", data: { ... finishing order and times ... }, recipients: "all" }`, built from `evani_ref_results`. Other events, if any, are listed in `HANDOFF.md` when C1 is built.
- **Limits** are checked by the bridge's tests: rules file under 1 MiB, state under 1 MiB of JSON, a view under 128 KiB, an action under 4 KiB, events under 4 KiB, no NaN or infinity, depth well under 32.
- **Time:** the bridge never reads `time` for gameplay; whole ticks only.

### C3. What ChimpVibe does on its side

**In your play frame (browser):**
1. Build your frame start-up code from our `bridge/play-adapter.js` (you pin your own code by hash, so you copy and adapt it; keep its messages exactly).
2. Create a `MessageChannel`. Post `{t:"hello", v:1, role}` with the port to the game frame, once, after it loads. `role` is `"spectator"` for watchers.
3. Wait for `{t:"ready", v:1, protocol}`.
4. Game to server: each `{t:"data", b}` from the game becomes an action `{ type:"in", data:{ b: base64(b) } }` through your shell.
5. Server to game: each view `{ b }` you receive becomes `{t:"data", b: decoded bytes}` on the port.
6. Send `{t:"caps", maxSendPerSec, maxBytes}` with your real limits (for example 30 and 4096), `{t:"pause"}` / `{t:"resume"}` when your shell pauses, and `{t:"status", ...}` when your connection state changes.
7. The game opens no network of its own, so your frame's no-network rule holds.

**On your server:**
1. Load `game/rules/rules.wasm` with your existing rules glue, every 50 ms, exactly as for any rules add-on game.
2. Declare the action type `"in"` in the manifest (we ship it filled in).
3. Call `project` for each player and once for the spectator every tick, as you already planned.
4. Store the `results` event as the official race result.

**IDs and sessions:** keep your anonymous, random id per join. A reconnect is a new join; the bridge seats a bot in the leaver's kart so the race stays full. You never need to send names, tokens or account data to the game.

**Idle:** no idle kick inside the game or engine. If you want one, it is your setting: remove the player from the roster and the bridge treats it as a leave.

**Watchers:** on (`"spectators": true`). A watcher can follow any kart.

### C4. How to test it

1. **Bridge harness** (ships with the package): a Node script that drives `rules.wasm` exactly as your reply 4 describes (JSON calls, 50 ms steps, spectator `project`, `restore` mid-race) and checks every state hash against our reference host. Run it on your machine first. **[C1]**
2. **Fake page:** `bridge/fake-host.html` frames the game, rate-caps to 30 messages a second, and steps the referee every 50 ms in the page, so you can see a race without your server. **[K3]**
3. **Your shell:** two browsers and one watcher in a private Session of the Sugarpeak draft; race 3 laps; have one player leave mid-race (a bot should take over) and rejoin (as a new join, watching until the next race).
4. **Numbers to compare:** bytes up and down per player, message rates, and `step` time per call, against `HANDOFF.md`. Our estimate for 8 players: about 1.4 KB/s up and 8 KB/s down per player at 20 views a second.

### C5. What not to change

- **Nothing inside the engine or the game:** not `rules.wasm`, not `game/client/`, not the rules source. If something must change, tell us; we fix it in the engine or the bridge and send a new package.
- **Not the message shapes** in `play-adapter.js` (`hello`, `ready`, `data`, `caps`, `pause`, `resume`, `status`, `leave`). You may restyle, rename variables and fold it into your frame code.
- **Not the bytes:** base64 in and out must give back the exact bytes.
- **No physics or position checks of your own** on actions: they are inputs only, and the referee already checks them.
- **No second copy of the race** (for example a JavaScript referee).

### C6. Open questions for ChimpVibe (to settle before B1)

1. Your events limit (128 per step) and view limit (128 KiB) are fine for Sugarpeak; confirm the same limits apply to the spectator view.
2. Can your shell deliver the views as binary instead of base64 one day? (It would cut down-traffic by about a quarter. Not needed for the first version.)
3. Which `maxSendPerSec` and `maxBytes` should the game assume in your frame?

---

## Status of every part of this guide

| Part | State |
|---|---|
| Packets (B3), `NetGame`, `Server`, `Client`, `Transport`, `Loopback`, `LocalSession`, bots, join and leave in the step, six `net.*` settings | Built (20 Oct 2026), 1 unit test |
| Tests and sync-test | N1 |
| Watchers, checkpoints, `sanitize`, roster joins and leaves, `seat_bot_on_leave`, `results` | N2a |
| Input pacing for capped hosts, state hashes, remaining settings | N2b |
| `evani-net/1` page contract (B4) | T1 |
| Referee build and `evani-referee/1` (B5), `tools/evani-host` | N3 |
| Sugarpeak Rally online | K1, K2 |
| ChimpVibe bridge (Part C), harness, hand-off package | C1 |
| Full test race and measured numbers | K3 |
| This guide, final | B1 |
