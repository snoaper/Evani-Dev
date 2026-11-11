# Multiplayer engine: server-run mode (`authority`) and the match layer (`matchflow`)

Written 2026-10-25 on branch `net-authority` (from `m1-foundation` b56f3fae, Join 8). Plan only: no engine code changes in this commit.

**In plain words.** Today Evani.Dev multiplayer is lockstep: every player's game runs the whole simulation from everyone's button presses, so every player's device holds the whole world. That cannot hide anything: "fog of war" would only be painted over. The server-run mode adds a second way: one copy of the game runs on the host's server (the referee), and each player gets a small, private picture of only what they may see, 20 times a second. Hidden enemies are never sent, so they cannot be found by cheating. Players still send only their controls. A separate, optional match layer runs lobbies, mode and settings votes, countdowns, teams, scores, respawns and results for any game. Broadside 3D, Chris's naval game, is the first user.

Picks from the research report (`.vibedeck/research/2026-10-25-server-authoritative-mode-for-evani-net-broadside3.md`), chosen by Chris and relayed by the coordinator:
1. Quantised, byte-budgeted per-recipient views: absolute first; acked-baseline deltas later and only when measured. Ideas from Gaffer on Games, "Snapshot Compression" and "State Synchronization" (articles; ideas only), with a priority accumulator.
2. A per-recipient visible set that enforces fog by omission. Ideas from Colyseus `StateView` (MIT), naia rooms and scope (MIT OR Apache-2.0) and bevy_replicon visibility (MIT OR Apache-2.0). Ideas only, no code, no dependency.
3. (Research pick 4.) A lobby and match-phase layer shaped like Nakama's match handler (Apache-2.0; ideas only): lobby, vote_mode, vote_settings, countdown, playing, results, with a seeded `Pcg32` tie-break on its own stream.

Left for later (section 8): deltas, own-ship prediction, binary without base64, lag compensation, matchmaking, WebRTC.

Orchestrator's note, taken (2026-10-25): the authoritative simulation steps at the engine's fixed **60 Hz**; views go out every third tick (20 Hz). Rules that need Broadside's original 20 Hz cadence run every third tick inside the 60 Hz step. Parity tests compare at view ticks.

Rules this plan keeps: the engine stays platform-neutral (no partner code in `crates/`; bridges live in `bridges/` or the game's own repository); build-time on/off is strict (a game that does not turn a feature on ships byte for byte as before); no new dependencies; deterministic 60 Hz with the seeded `Pcg32`; every tunable is a registered `net.*` or `match.*` parameter; no identity in the engine (a peer is a number).

---

## 1. What exists, what this reuses, and the gaps

Read at b56f3fae: `crates/evani-net` (about 6,100 lines of source plus 2,700 of tests), `bridges/chimpvibe`, `tools/evani-host`, `apps/kart-racer/referee`, docs/NET_BUILD_PLAN.md, docs/NET_BRIDGE_GUIDE.md, DECISIONS.md (multiplayer entries), STAGES.md, AGENTS.md.

### 1.1 What is there

| Part | File | Feature | What it does | Reused here? |
|---|---|---|---|---|
| Wire helpers | `wire.rs` | always (with `net`) | `Writer`/`Reader`, varint, zigzag, `f32`/`f64`, fixed `u64`, `Wire` trait and `wire_struct!`, `fnv1a64`, `quantize`/`dequantize` (fixed at 24 bits in N1) | **Yes**: every byte the new mode writes goes through them. Bit packing on top is new (2.4). |
| Ids and time | `types.rs` | always | `PeerId`, `NetId`, `Tick`, `TickTime` (sub-ticks), `Role`, `mix64` | **Yes** |
| Seeded randomness | `game.rs` `tick_rng` | always | `Pcg32` per (seed, tick, stream) | **Yes**: the authority step context and the match layer's tie-break stream use it. |
| Snapshot core | `server.rs`, `client.rs`, `msg.rs`, `session.rs` | always | `NetGame`; `Server` steps the state and sends **the same whole state to every client**; `Client` predicts and rolls back; pacing by slack; bots; lag compensation | **Ideas and helpers only.** It cannot fog (one state for all), has no spectators, no save/load, no `sanitize`. Hexagon (tactical FPS) and the sandbox use it, so it stays untouched; the new mode is a sibling module. Reused: the redundant-input pattern (`on_input`), `InputHeader`'s ack field idea, `ViewCodec`-style compact headers. |
| Interpolation | `interp.rs` | always | `Interpolate`, `PoseHistory`, `Sampled`, `lerp_angle`, `hold` | **Yes**: the client draws every entity (own ship included) interpolated. Per-entity sample buffers are added because entities arrive in different views under a budget. |
| Lockstep and referee | `lockstep.rs` | `lockstep` | `LockstepGame`, `Referee` (seats, bots via `Brains`, frame history, hashes, per-recipient views, spectator views a few ticks back, save/load), `LockstepClient`, `sync_test` | **Shape only.** Seat handling, spectator lag, save/load layout and the "view is `&self`" rule are copied in spirit. Lockstep code is not changed. |
| Referee ABI `evani-referee/1` | `referee.rs` | `referee` (implies `lockstep`) | object-safe `Session` trait, thread-local state, `export_referee!`, no imports | **Yes, unchanged ABI** (2.9). The `Session` trait gets a second implementer. |
| Page link `evani-net/1` | `port.rs`, `web/port.js` | `port` (wasm) | `HostPort`: `send` (datagram), `send_reliable` (ordered), `recv -> reliable flag`, role, follow, pause, caps; 128 KiB cap | **Yes**, unchanged (2.6). |
| Transports | `transport.rs` | always | `Transport` trait, seeded `Loopback` with latency, jitter, loss | **Yes** for tests and offline play. |
| Sync test | `synctest.rs` | always | `SyncTest` for any `NetGame`: repeat, wire round trip, rollback; `first_divergence` | **Pattern.** A small `authority::sync_test` mirrors Repeat and WireRoundTrip for the new trait (no rollback check: clients never re-simulate). `first_divergence` is reused as is. |
| Parameters | `lib.rs` `NetParams` | always, some under `lockstep` | `net.send_rate_hz` (20), `net.interp_delay_ms` (100), `net.lag_comp_max_ms`, loopback settings; `net.input_delay_ticks`, `net.max_catchup_steps` under `lockstep` | **Yes**: `net.send_rate_hz` already gives the view divisor (60 / 20 = 3, `NetConfig::snapshot_every`). New `net.*` and `match.*` parameters register only under their features, the same way `lockstep`'s do. |
| Reference host | `tools/evani-host` (Node) | dev tool | loads a referee `.wasm`, WebSocket, spectators | **Yes**: it drives any `evani-referee/1` module, so an authority referee should run unchanged (checked in NA5). |
| ChimpVibe bridge | `bridges/chimpvibe/rules-shim` | own workspace | wraps Sugarpeak's referee in `chimpvibe-rules/1`, JSON with base64 views | **Yes, pattern**; made generic over any `Session` (bridge-side, outside `crates/`, package NB1). |
| Byte proof method | DECISIONS "Join 8", `.evani/tmp/sugarpeak-mp/ship.sh` | | SHA-1 of 16 shipped files of 8 games against a baseline, `EVANI_GIT_COMMIT=65e8497`, rustc 1.99.1 | **Yes**, extended with the kart racer's `referee.wasm` and `rules.wasm` (2.1). Lesson kept: new code goes **below** existing code (a moved panic line changed match-3 by one byte once). |

### 1.2 Gaps this plan fills

1. **No per-recipient view.** `Server::snapshot` encodes one state for all; `Referee::view` sends the same frames to all. Nothing can be hidden.
2. **No visibility, no priority, no byte budget.** Every view is the whole state.
3. **No bit-level quantisation tables.** `quantize` exists, but nothing declares a game's fields once and packs them.
4. **No reliable events in a pulled view.** Referee hosts pull one view per recipient; there is no way to make sure a "ship sunk" or "phase changed" reaches a player whose view was dropped.
5. **No spectator fog choice or delay** outside lockstep.
6. **No match layer.** Phases, votes, teams, scores, respawns and results would be written again by every game (Broadside already wrote `lobby.rs` and `modes.rs`).
7. **No recorded-match parity harness** for a game that moves from another rules format.
8. **Referee feature implies lockstep** (`referee = ["lockstep"]`), so an authority referee would drag lockstep in. Split in NA1.

### 1.3 Broadside 3D today (read only, as data)

- Rules: `evani-game/rules` (`broadside-3d-rules`, about 3,900 lines on `models-and-destruction`; plus `lobby.rs` 109 and `modes.rs` 1,005 lines on `feature/game-modes`). A line-for-line Rust port of v7's `runtime.js`: JavaScript number semantics (`js.rs`), v7's xorshift32 carried as a 32-bit state, stepped by the host every 50 ms. `tests/compare.mjs` proves byte equality with v7.
- Views: JSON, a complete projected view per recipient every 50 ms (`view.rs`). On `feature/game-modes`, fog leaves out enemy ships farther than `FOG_R` = 700 chart px from the player's ship, captain ashore or any team-mate; spectators see everything; the standings list every ship without positions.
- **Fog leak found:** fog filters `ships` only. Cannonballs, pistol shots and effects (smoke, splashes) are not filtered, so a hidden ship's broadside shows where it is. The new mode treats projectiles and effects as entities with their own visibility (2.3), which closes this.
- Lobby and modes: `mode` vote (race, team, ffa) 20 s or all voted; `rules` vote (ammo empty/half/full/unlimited, damage on/off, fog on/off) 20 s; `ready` 5 s; `play` (10 min cap; first to 10 sinks for team and ffa; race 2 laps with a 45 s grace after the winner); `over` 12 s; loop. Ties and no votes go to a random pick among leaders (one draw from the rules' xorshift).
- Page link: `evani-game/link`, a hand-written copy of `HostPort` (Broadside's audit A2 already plans the swap).
- Ship states the brief lists: damage stages, a grounded dig-out timer, destroyed piers and resupply progress. Damage stages are drawn by the kit from `hp` today; **the dig-out timer, destroyed piers and resupply progress are not in the rules on any branch yet** (grep of all three branches). They become ordinary kit state fields with quantisation entries when Broadside builds them (5.3).

---

## 2. The design

### 2.1 Features and the byte proof

New evani-net features, all off by default, all added below existing code:

```toml
[features]
lockstep = []                               # unchanged
referee-abi = []                            # NEW: the evani-referee/1 exports and the Session trait alone
referee = ["referee-abi", "lockstep"]       # same meaning as today (Sugarpeak's referee keeps working unchanged)
port = ["dep:wasm-bindgen", "dep:js-sys"]   # unchanged
authority = []                              # NEW: the server-run mode (src/authority/*)
matchflow = []                              # NEW: the match layer (src/matchflow.rs), usable with authority or lockstep
```

- An authority game's app: `net = ["dep:evani-net", "evani-net/authority", "evani-net/port", "evani-net/matchflow", "<kit>/net"]` (the kart racer's pattern; no new `evani.toml` key is needed, mode choice stays in Cargo features as it does for lockstep today). Its referee crate: `evani-net = { features = ["authority", "referee-abi", "matchflow"] }`.
- In `referee.rs`, `impl Session for Referee<..>` moves under `#[cfg(feature = "lockstep")]`; the ABI functions and `export_referee!` sit under `referee-abi`. `referee` keeps implying `lockstep`, so `apps/kart-racer/referee` and `bridges/chimpvibe/rules-shim` build exactly as before.
- New parameters register in `NetModule::build` only under their features, appended after the `lockstep` block, so the param list of every other game is unchanged.

**Proof that lockstep and other games stay byte-identical** (package NA1 sets it up, NP1 repeats it at the end):
1. Ship builds (`cargo xtask build <id> --ship`, rustc 1.99.1, `EVANI_GIT_COMMIT=65e8497`, `CARGO_BUILD_JOBS=1`) of all eight games: 16 files (js and wasm) SHA-1 identical to b56f3fae. This includes the kart racer (`net = true` with `lockstep` and `port`), the sandbox and the tactical FPS (`net = true`, snapshot core).
2. The kart racer's headless `referee.wasm` and `bridges/chimpvibe/out/rules.wasm`: SHA-1 identical to b56f3fae builds.
3. `cargo tree -e features -p <app>` for the eight apps shows no `authority`, `matchflow` or `referee-abi` except through `referee` where it was before.
4. The evani-net test suite passes unchanged (`cargo test -p evani-net`), and its pinned hashes (arena state hash, protocol hash, FNV vectors) are unchanged.

### 2.2 The authoritative game trait

`crates/evani-net/src/authority/mod.rs` (feature `authority`). The kit implements it once; the same crate builds into the referee (server) and into the player's game (client). The client never calls `step`, so with the kit's rules behind a kit feature (the kart racer's `render` split, reversed: a `rules` feature the referee turns on), the rules never reach the player download.

```rust
/// A game the server runs alone. Clients send actions and draw views.
pub trait AuthorityGame: 'static {
    /// One seat's held controls (helm, sail, aim, wrapping press counters, vote).
    type Action: Wire + Clone + Default;
    /// The whole world. Server only; never sent whole to a player.
    type State: Wire + Clone;
    /// Protocol name hash; a client with another is refused.
    const PROTOCOL: u32;
    /// The quantisation tables: every entity kind and its fields (2.4).
    const SCHEMA: &'static Schema;

    /// Clamp an action to legal values (unknown bits cleared, ranges clamped).
    /// Runs on every action before the step sees it, and before it is recorded.
    fn sanitize(&self, action: Self::Action) -> Self::Action;

    /// Advance one 60 Hz tick. Reads only `state` and `ctx`: no clock,
    /// randomness from `ctx.rng(stream)`, fixed iteration order.
    fn step(&self, state: &mut Self::State, ctx: &mut AuthCtx<'_, Self::Action>);

    /// Every networked thing now: id, kind, quantised-to-be field values,
    /// and the point visibility is tested at. Order does not matter.
    fn entities(&self, state: &Self::State, out: &mut EntitySink);

    /// What `viewer` may know about: entity ids, into `out` (cleared).
    /// Fog is enforced here, by omission. The engine adds nothing to it.
    fn visible_set(&self, state: &Self::State, viewer: Viewer, out: &mut VisibleSet);

    /// The recipient's private block (own ammo, gold, reload, own votes), as
    /// bytes. Written last into the recipient's view; never sent to others.
    fn view(&self, state: &Self::State, viewer: Viewer, w: &mut Writer) {}

    /// How much `viewer` needs entity `id` this view (default 1.0). Near,
    /// fighting or own things higher. Must be finite and >= 0.
    fn priority(&self, state: &Self::State, viewer: Viewer, id: NetId) -> f32 { 1.0 }

    /// Results bytes for the host (the match result, standings).
    fn results(&self, state: &Self::State, w: &mut Writer) {}

    /// State hash for parity tests and host logs (default: FNV-1a of the wire encoding).
    fn hash(&self, state: &Self::State, scratch: &mut Writer) -> u64 { /* fnv1a64 of state */ }
}

pub enum Viewer { Seat(u8), Spectator { team: Option<u8> } }
```

`AuthCtx` carries: `tick`, `seed`, `rng(stream)` (same derivation as `StepCtx::rng`), the sanitised action of every seat (sorted by seat), seats joined and left since the last tick, and `event(audience, kind, bytes)` for one-off cues (2.6). The kit holds its seats, teams and match state inside `State` (with `matchflow`, a `MatchFlow` value: 2.10), so save/load and hashing cover everything.

Why `view` is a hook and not the whole packet: the brief's "view(recipient) to bytes" is the engine's composed packet (2.5): header, public match block, entity records chosen by visibility, priority and budget, events, then the kit's private block. Keeping entity encoding in the engine is what makes fog, budgets and later deltas work for every game without each kit re-writing them.

Bots: a kit drives bots inside `step` (they are seats with no client), as Broadside's privateers already are; no `Brains` trait is needed.

### 2.3 Visible set: fog by omission

Per view tick, for each recipient:
1. The kit fills `VisibleSet` (a sorted, deduplicated `Vec<NetId>`, pooled).
2. The engine encodes **only** ids in the set. An id that was known to the recipient and is no longer in the set gets one `gone` marker (reliable, 2.6), and its per-recipient memory (accumulator, last sent) is forgotten, so a re-entering entity is always sent whole, never relative to a stale value.
3. Events carry an audience (2.6); `Near(point)` events go only to recipients whose visible set would include that point (the kit answers through `visible_point`, a default method built on its own rule), so a hidden ship's cannon sound is not a side channel.
4. The private block is the kit's and must only describe the recipient's own things.
5. Projectiles and effects are entities like ships (short-lived ids), so the default rule "visible if inside sight" covers them. This closes the leak found in 1.3.

Side channels the engine itself guards (each has a test in NT1): entity counts are not sent (the client never learns how many ids exist); ids are allocated by the kit, and the plan's guide tells kits to use per-match counters, not slot numbers that reveal roster order (Broadside's ship ids are seat-based and already public through the standings, which is acceptable because standings are public); the budget order never depends on hidden entities (priorities are computed only over the visible set); the view's size is not padded (size could hint at count; accepted, documented).

### 2.4 Quantisation tables declared by the kit

`authority/schema.rs` and `authority/bits.rs`:

```rust
pub struct Schema { pub kinds: &'static [EntityKind] }
pub struct EntityKind { pub id: u8, pub name: &'static str, pub fields: &'static [Field], pub base_priority: f32 }
pub struct Field { pub name: &'static str, pub q: Q }
pub enum Q {
    Range { min: f32, max: f32, bits: u8 },   // positions, speeds: wire::quantize at `bits` (1-24)
    Angle { bits: u8 },                       // headings: [0, 2pi) wrapping
    UInt { bits: u8 }, SInt { bits: u8 },     // counters, timers in ticks, stages
    Bool,
    Enum { count: u8 },                       // ceil(log2(count)) bits
}
```

- `BitWriter`/`BitReader` (little-endian bit order, bounds-checked, never panics) sit on `Writer`/`Reader`.
- A record is: kind (bits for the schema's kind count), id (varint inside the bit stream, or a small relative index when sorted), then each field in table order. Absolute only in this version.
- Both sides decode from the same `SCHEMA` constant; its FNV-1a hash is mixed into `PROTOCOL`, so a changed table refuses old clients.
- Broadside's ship, for scale: x, y on a 3200 x 2200 chart at 1/8 px (15 + 15 bits), heading (10), speed (8), hp (7), sail (2), helm (2), damage stage (2), flags alive/sinking/shield/repairing/ashore (5), team (2): about 68 bits, **9 bytes** with kind and id. A cannonball: x, y (2 x 13 bits at 1/2 px), owner id, life (6): about 5 bytes.
- Kits should store the quantised value in the state when a field feeds back into rules (AGENTS' determinism advice in `wire.rs`). For Broadside phase 1 that is not required: only the referee simulates.

### 2.5 Per-recipient byte budget and the priority accumulator

`authority/view.rs`, pure functions so they test without a server:

```
for each view tick, for each recipient r:
  must  = header + match block + gone markers + unacked events + private block + entities flagged "must" (own ship)
  room  = budget(r) - size(must)                       // budget: net.view_budget_bytes, spectators net.spectator_budget_bytes
  for id in visible_set(r):  acc[r][id] += base_priority(kind) * kit.priority(r, id)
  order visible ids by acc desc, ties by NetId asc       // deterministic
  write records while they fit; acc[r][id] = 0 for each written; stop at the first that does not fit
                                                         // (no skipping ahead to smaller records: keeps order stable and simple)
  entities "at rest" keep accumulating until sent, so nothing starves
```

- Absolute records mean any record that arrives is right on its own; the client keeps each entity's last two samples with their view ticks and interpolates per entity.
- A recipient's first views after a join can be over-full: the server sets a `complete` flag in the header once every visible entity has been sent at least once; the client shows a short "joining" state until then.
- Parameters (feature `authority`): `net.view_budget_bytes` (default 512, range 128-16,384), `net.spectator_budget_bytes` (1,024, 128-16,384), `net.view_must_cap_bytes` (2,048: a hard cap even for must-send data; over it, gone markers and events spill to the next view in order). The packet never exceeds the host's `caps.maxBytes` (the client side reports nothing; the host cap is a referee config value, 2.9).
- Stats: bytes per view per recipient (min, mean, max), entities deferred, oldest unsent age; into `NetStats` and the overlay line.

### 2.6 Reliable and unreliable channels

The host link has two kinds (`HostPort::send` and `send_reliable`; referee hosts pull views and may or may not deliver every one). The design never depends on which:

| Direction | What | Channel | Why |
|---|---|---|---|
| client to server | actions (held controls) | `HostPort::send` (unreliable), each packet repeats the last 4 actions with sequence numbers | Held state is idempotent; the newest sequence wins. One-shot presses (fire, repair, go ashore) are **wrapping counters** in the action, so a lost or repeated packet never loses or doubles a press. |
| client to server | votes, ship choice | inside the action as held fields (`vote: Option<(question, option)>`, latest wins) | Idempotent; no reliable channel needed, which matches Broadside's "a new vote replaces the old one". |
| client to server | `leave` | `HostPort::leave()` (host message) | The host's roster is the truth. |
| server to client | entity records, match block, private block | in every view (unreliable is fine) | Absolute and repeated by the accumulator. |
| server to client | gone markers, events (sink, pier destroyed, phase changed, results) | **reliable by redundancy**: repeated in every view to that recipient until the recipient's input packet acks a view tick at or after the one that first carried it | Works whether the host drops, repeats or reorders views; needs no second output from the referee. Gaffer's "reliability over unreliable" idea. |
| server to spectators | events | repeated for a window (`net.spectator_event_window_ms`, 2,000) because spectators send nothing | |

Rule for kit authors (in the guide): anything that decides correctness is **state** (phase, scores, alive, hp, pier destroyed), so a late joiner sees it in the next view; events are only cues (sounds, splashes, banners). A destroyed pier is both: a state field on the pier entity (or a public world bit-set in the match block) and a cue event.

Client to host: the client's `ack_view` travels in its normal input packet header, as `InputHeader::ack_snapshot` already reserves for the snapshot core.

### 2.7 Spectator views

- `evani_ref_view(0xFFFFFFFF)` returns the **shared spectator view**: `Viewer::Spectator { team: None }`, budget `net.spectator_budget_bytes`, built every view tick into a ring of past packets; the host gets the one `net.spectator_delay_ms` old (default 0; a competitive host can set seconds; ring size = delay / view interval + 1).
- Spectators who joined with role 1 have a peer id; `evani_ref_view(peer)` for such a peer returns a **team-fogged** spectator view when the kit's config says spectators follow a team (`Viewer::Spectator { team: Some(t) }`), else the shared view. No ABI change.
- A spectator never acks, so its view is complete on its own over a short cycle: the accumulator guarantees every visible entity is refreshed within `ceil(total_size / budget)` views, and the header carries `complete` once a full cycle has passed since that spectator's first view. Spectators send nothing; the server ignores any packet from a spectator peer.
- Follow: the host's `follow` message stays a client-side camera choice (the view is the same).

### 2.8 Late join and reconnect

- Join (`evani_ref_join(peer, 0)` or a Transport hello): the server gives the peer a seat (the kit decides, through `AuthCtx::joined`, whether it plays now, waits in the respawn queue or watches until the next match; with `matchflow`, `match.join_mid_match` = `play` | `next_match`), creates empty view memory, and its next views send every visible entity whole, in priority order, until `complete`.
- Leave: the host's roster is the truth; the kit removes or hands over the seat (Broadside: the privateer AI takes the ship, as v7 does).
- Reconnect: hosts with anonymous per-join ids make it a new join (ChimpVibe point 6). A host with stable ids may map a returning player to the same seat in its connector (outside the engine).
- Idle: never removed by the engine.

### 2.9 The referee ABI: `evani-referee/2` is not needed

`evani-referee/1` covers the mode as it is:

| Export | Authority meaning |
|---|---|
| `init(seed, cfg)` | a new `AuthorityServer`; config bytes carry the kit's settings plus `max_view_bytes` (the host's cap, e.g. 128 KiB for ChimpVibe; views are far under it) |
| `join(peer, role)` / `leave(peer)` | roster changes; role 1 = spectator |
| `input(peer, packet)` | one client packet exactly as sent; contains actions and `ack_view`; refused (-3) for spectators and bad packets |
| `step(ticks)` | whole 60 Hz ticks (3 for a 50 ms host step); on every view tick inside, the server **builds and caches every recipient's view** (and the spectator ring) |
| `view(recipient)` | returns the cached packet: pure and idempotent, so a host may call it any number of times (ChimpVibe's `project` per recipient) and the bytes do not depend on how often it asks. A view tick that has not come yet returns the last cached packet (the client drops a repeated view tick). |
| `save()` / `load()` | the whole server **including view memory, accumulators, unacked events and the spectator ring**, so a loaded referee produces byte-identical views (NT1 tests this) |
| `results()`, `hash()`, `tick()` | as today |

`Session::view(&self, ..)` keeps its `&self` signature: all mutation happens in `step`. The ABI's `evani_ref_abi()` stays 1; a host tells the modes apart by the game, not the ABI.

`evani-referee/2` would be needed only for: several packets per view (a view larger than a host's cap, which the budget prevents), a separate reliable output stream (redundancy covers it), or acks outside the input packet (none planned). Revisit only with deltas, if a host's link proves lossy enough that baseline acks need their own path.

### 2.10 The match layer (`matchflow`, separate optional feature, no game words)

`crates/evani-net/src/matchflow.rs`. Pure deterministic state the kit embeds in its `State` and steps inside its own `step` (works under `authority` and under `lockstep`; it has no dependency on either). All times in ticks; all ids are seats.

| Piece | What it does |
|---|---|
| Phases | `Lobby`, `VoteMode`, `VoteSettings`, `Countdown`, `Playing`, `Results`, each with a deadline tick and exit rule: timer, or "every present player voted" (early close), or `Lobby` until `match.min_players` are present for `match.lobby_wait_s`. A phase with duration 0 is skipped (Broadside has no lobby: it starts at `VoteMode`). After `Results`, back to `Lobby` or `VoteMode`. |
| Questions | declared by the kit as data: `Question { id: u8, options: u8, round: Mode or Settings }`, plus a hook `settings_questions(mode) -> &[Question]` so settings can depend on the chosen mode. Option **names** stay in the kit (UI text); the layer only sees counts. |
| Votes | `cast(seat, question, option)`: one per seat per question, latest wins, out-of-range or closed questions refused; leavers' votes dropped at tally; late joiners may vote while the phase is open. |
| Tally and tie-break | at the deadline or early close: most votes wins; a tie, or no votes, picks uniformly among the leaders with `tick_rng(seed, tick, MATCHFLOW_STREAM)` (a fixed stream id of its own, so gameplay randomness is never disturbed and a replay repeats it). A kit may supply its own `TieBreak` (used only by Broadside's parity phase, 5.4). |
| Roster and teams | seats with role (player, bot, spectator), team (`Option<u8>`), joined tick. `assign_team` default: the smallest team, ties to the lower team id; the kit may override. |
| Scores | `i32` per seat and per team: `add(seat, n)`; team totals derived. |
| Respawn queue | `(seat, ready_tick)`, `request(seat, delay_ticks)`, `ready(now) -> seats` in (ready_tick, seat) order; spawn placement is the kit's hook. |
| Win rules | built in: `ScoreToWin { n, per: Seat or Team }`, `TimeLimit { ticks }`, plus the kit's `check_win(state) -> Option<Outcome>` for custom ends (a race's finish and grace). `Outcome = Winner(seat) | WinnerTeam(t) | Draw`. |
| Public view block | encode and decode helpers for the phase, time left, round, chosen options, tally counts per option, voters and voted counts, team scores, per-seat scores and teams, outcome. Plus the recipient's own votes (goes in the private block). Fixed small layout; about 20 to 60 bytes. |
| Parameters (`match.*`, registered only with the feature) | `match.vote_s` 20, `match.settings_vote_s` 20, `match.countdown_s` 5, `match.results_s` 12, `match.time_limit_s` 600, `match.score_to_win` 10, `match.min_players` 1, `match.lobby_wait_s` 0, `match.join_mid_match` play. Read at the start of each match, so a mid-match change never alters a running match's rules (determinism in replays: the values in force are recorded). A kit sets its own defaults from its data file. |

Stays in the game's kit: ship states, checkpoints and laps, weapons, damage, what a "sink" scores, spawn points, the meaning of "ammo: half", race finishing order. The engine has no ship, gate, lap or weapon words.

### 2.11 The client side

`AuthorityClient<G>` (feature `authority`): builds input packets (newest actions with sequence numbers, `ack_view`, protocol), decodes views with `G::SCHEMA`, keeps per-entity sample buffers and returns interpolated values at draw time (`net.interp_delay_ms`, default 100; the research suggests about 120 for 20 Hz with loss), exposes the private block, the match block, `complete`, and a drained event list. No prediction in this version: the own ship is interpolated too, so a helm change shows after one round trip plus the delay (about 200 to 300 ms on a normal link; ships turn slowly). Own-ship prediction is a later package (8).

Packets start with `u8 WIRE_AUTH, varint PROTOCOL, u8 kind` (lockstep's convention), so a lockstep client and an authority referee refuse each other cleanly.

### 2.12 Cadence: 60 Hz step, 20 Hz views

- The server steps at 60 Hz. A view tick is every `snapshot_every` ticks, from `net.send_rate_hz` (20 Hz = every 3rd tick, exactly). Picked: reuse `net.send_rate_hz` (it already yields the divisor) instead of a second `net.view_rate_divisor`, so there is one knob; the referee config can pin the divisor for hosts with a fixed step.
- Parity runs pin the divisor to 3 and the view phase to the tick after a rules sub-step (5.4).

---

## 3. Record-and-replay parity tests

`crates/evani-net/src/authority/replay.rs` (generic, test helper, never in a download) and per-game tests.

**Recording** (`.evrec`, a small binary file; plus a JSON sidecar for people): wire version, game protocol, schema hash, seed, config bytes, `match.*` values in force, then per tick only what changed: roster events (join seat role, leave seat) and each seat's **sanitised** action when it changes (run-length). Recorded on the server, so a recording replays without any network.

**Replay checks**, each a function a game's test calls:
1. **Per-tick state hash**: replay into a fresh server; `G::hash` every tick equals the recorded list (`synctest::first_divergence` reports the first tick and, with named parts, which part).
2. **Per-recipient view bytes at sampled ticks**: at chosen view ticks (default every 60th view and every phase change), every recipient's packet and the spectator packet equal the recorded bytes, byte for byte.
3. **Fog absence**: for every sampled view, decode it with the client decoder and assert that no id outside the kit's own visible set for that recipient appears, and that the decoded events contain no `Near` event outside sight. The kit supplies a "truth" function (Broadside: its fog rule over the full state), so the test checks the encoder, not just the kit's set. A second check runs the client decoder over a whole match and asserts it never held a hidden enemy at any tick.
4. **Save and load mid-match**: at three ticks (lobby, mid-play, results), `save`, a fresh server `load`s, and the rest of the run gives the same state hashes **and** the same view bytes as the uninterrupted run.
5. **Repeat and wire round trip** (`authority::sync_test`): the step run twice from one recording gives the same hashes; the state encoded and decoded before every step changes nothing.
6. **Run as wasm**: the referee `.wasm` replays the recording in Node through the ABI (the `bridges/chimpvibe/test/rules-harness.mjs` pattern) and matches the native replay's hashes and view bytes. For games whose step uses `sin`, `cos` and friends, the wasm run is the reference (DECISIONS "float determinism"); a native run is a convenience.
7. **Bandwidth**: from a recording, bytes per view per recipient (min, mean, max, p95) and per second, written to a table; a test fails over the game's stated budget.

Pass bar for a game's hand-over: 1, 2, 3, 4 and 6 on at least three recorded matches per mode, 10 minutes each, with joins and leaves.

---

## 4. Packages

One worker session each. Sizes: S (about an hour), M (half a session), L (a full session). Every engine package: its own worktree off `net-authority` (or the branch named), `CARGO_TARGET_DIR` and `TEMP` on `<scratch>\evani-netauth-<id>`, builds through `run-low.mjs` after `resource_check`, `cargo xtask check --quick` while working and `cargo xtask check` clean before hand-over, commit after each step, no push. New code goes below existing code in every touched file.

### Engine (`crates/evani-net` and tools)

| ID | Package | Files owned | Size, worker | Depends on | Done when |
|---|---|---|---|---|---|
| **NA1** | Features and byte guard: `referee-abi`, `authority`, `matchflow` in Cargo.toml; `referee.rs` split (Session impl for lockstep under `cfg(lockstep)`); empty `authority/mod.rs` and `matchflow.rs` appended in `lib.rs`; byte-proof script covering 16 files + kart `referee.wasm` + `rules.wasm` + `cargo tree` feature check; baseline recorded; the sandbox `net` byte cost left open by N1 measured here. | `crates/evani-net/Cargo.toml`, `src/lib.rs` (appended lines only), `src/referee.rs`, `.evani/tmp/netauth/ship.sh` | S, opus-high | none | 16/16 + both referee files identical; `cargo test -p evani-net` passes; DECISIONS line with the baseline |
| **NA2** | Bit packing and quantisation tables: `BitWriter`/`BitReader`, `Schema`, `Q`, record encode/decode (absolute), schema hash into protocol. | `src/authority/{bits,schema}.rs`, `tests/net/authority_bits.rs` | M, opus-high | NA1 | round trips at every limit; 20,000 garbage and mutated inputs never panic; pinned vectors; quantise error within half a step; `Angle` wraps |
| **NM1** | Match layer: phases, timers, questions, votes, tally, `Pcg32` tie-break stream, roster and teams, scores, respawn queue, win rules, public view block, `match.*` params; `Wire` for all of it. | `src/matchflow.rs`, `tests/net/matchflow.rs`, `lib.rs` params block under `cfg(matchflow)` | M-L, opus-high | NA1 | tests: tallies; deterministic ties (same seed same pick, all leaders reachable over seeds); leaver's vote dropped; late joiner votes; early close; skipped zero-length phases; phase loop; respawn order; score to win per seat and team; time limit; draw; mid-match param change does not alter a running match; save/load mid-phase continues identically |
| **NA3** | View builder: visible set handling, per-recipient memory, gone markers, priority accumulator, budget fill, events with audiences and reliable-by-redundancy, `complete` flag, spectator ring with delay. Pure functions over an entity list. | `src/authority/view.rs`, `tests/net/authority_view.rs` | L, opus-xhigh | NA2 | tests: nothing outside the visible set is ever written (property test over random sets); re-entry is absolute; budget never exceeded except must-send, which spills in order; starvation-free (every visible entity sent within N views); deterministic order; events repeat until acked, stop after; spectator window; ring delay exact |
| **NA4** | `AuthorityGame`, `AuthCtx`, `AuthorityServer` (roster, sanitise, action intake with sequence numbers and redundancy, 60 Hz step, view cadence from `net.send_rate_hz`, cached views, spectators, joins and leaves, save/load including view memory), `AuthorityClient` (input packets, decode, per-entity interpolation via `interp.rs`, private and match blocks, events), Transport adapter and an `AuthoritySession` for offline play; `authority::sync_test`; a toy "fogged arena" game in tests; new `net.*` params under `cfg(authority)`. | `src/authority/{mod,server,client,session,synctest}.rs`, `tests/net/authority_*.rs`, `lib.rs` params block | L, opus-xhigh | NA3 (NM1 for the toy's match, optional) | loopback: 12 seats (4 humans, 8 bots) plus 2 spectators at 100 ms, 30 ms jitter, 5% loss: every client's decoded entities match the server's quantised values for visible ids, never contain hidden ids; late joiner `complete` within budgeted views; save/load continues byte-identical; hostile packets dropped and counted; bandwidth table printed |
| **NA5** | Referee ABI for authority: `impl Session for AuthorityServer`, `export_referee!` under `referee-abi`, config bytes (`max_view_bytes`, spectator team, delay), toy referee crate as a test fixture, headless wasm build with no imports; `tools/evani-host` runs it unchanged (fix only if a generic gap shows). | `src/authority/referee.rs`, `crates/evani-net/tests/fixtures/authority-referee/*`, `tools/evani-host/*` (generic fixes only) | M, opus-high | NA4 | toy `referee.wasm` imports list empty; Node harness drives init/join/input/step/view/save/load and matches the native server's hashes and view bytes for 10,000 ticks; `view` idempotent (100 calls, same bytes) |
| **NT1** | Record-and-replay harness: `.evrec` writer and reader, replayer, the seven checks of section 3 as reusable functions, wasm replay runner (Node, no dependencies), fog-absence helpers; wired into `cargo xtask check --full` through the existing `NET_TESTS` line. | `src/authority/replay.rs`, `tests/net/authority_replay.rs`, `tools/evani-host/replay.mjs` | M, opus-high | NA4; the wasm part after NA5 | toy game: three recorded matches pass all seven checks; a deliberately leaky toy (sends one hidden id) is caught by check 3 at the right tick |
| **NB1** | Generic ChimpVibe shim over any `Session` (bridge side, outside `crates/`): `rules-shim` made generic (a small trait for the game's ChimpVibe events), Sugarpeak's build unchanged byte for byte; documents the authority mapping (`project` = cached view, base64). | `bridges/chimpvibe/*` only | M, opus-high | NA5 | Sugarpeak's `rules.wasm` identical; the toy authority referee wrapped as `rules.wasm` passes the existing rules harness; `git grep -i chimpvibe -- crates tools` empty |
| **NP1** | Proof and docs: byte proof again (2.1), per-feature wasm size of a client build of the toy with `authority`, `matchflow`, both; referee sizes; bandwidth table at 4, 8, 12 seats; `cargo xtask check --full`; DECISIONS ("Multiplayer: server-run mode", picks and licences, no new dependencies); README "Multiplayer" section gains the mode; docs/LIVE_API.md `net.*` and `match.*`; docs/NET_BRIDGE_GUIDE.md gains "Part D: server-run games". | DECISIONS.md, README.md, docs/LIVE_API.md, docs/NET_BRIDGE_GUIDE.md | M, sonnet-high | all engine packages | proof table committed; every number within section 4.2 or a written gap with a fix package |

### Broadside 3D (in Broadside's own project and its bridge; not engine work)

| ID | Package | Where | Size, worker | Depends on | Done when |
|---|---|---|---|---|---|
| **BS1** | Parity references from today's rules: a recorder in Broadside's tests that runs the current rules (v7 port with game modes) and writes, per 50 ms step: seed, roster, commands, state JSON hash, every recipient's JSON view, the spectator view. Three matches per mode (race, team, ffa) plus classic, 10 minutes each, with joins, leaves and every vote path (ties included). | `..\Broadside3D\evani-game\rules\tests\record*` | M, sonnet-high | none: **start now** | recordings committed in Broadside; a re-run of the recorder reproduces them byte for byte |
| **BS2** | Authority adapter, phase 1 (parity): crate `evani-game/referee` implementing `AuthorityGame` around the existing rules. 60 Hz step that runs the v7 50 ms step on every third tick; the xorshift32 kept inside `State` (an exception for parity, 5.4); actions mapped from today's commands (held controls, press counters, vote fields); `SCHEMA` for ships, balls, shots, effects, pools, piers; `visible_set` = today's fog rule extended to balls, shots and effects; private block = own ammo, gold, reload, votes. Replays BS1's recordings. | `..\Broadside3D\evani-game\referee` | L, opus-xhigh | NA4, NT1 (NA5 for the wasm run) | every BS1 recording: state hash equal at every rules sub-step (every view tick); decoded views equal the JSON views' content within each field's quantisation step; fog absence passes; save/load passes; wasm replay matches |
| **BS3** | Match layer and client: Broadside's `modes.rs` lobby onto `matchflow` (questions mode, ammo, damage, fog; its timers as `match.*` defaults; the custom `TieBreak` keeping xorshift for parity); kit draws from `AuthorityClient` instead of JSON views; `link` replaced by `HostPort`; ChimpVibe shim on NB1. | Broadside3D (kit, app, bridge) | L, opus-high | BS2, NM1, NB1 | BS1 recordings still pass; browser run through the fake shell: two players, one spectator, every mode to the result screen; bytes per player measured against today's JSON |
| **BS4** | (Later, owner's choice) Phase 2: native Evani step (60 Hz maths, `AuthCtx::rng`), retiring v7 parity; new recordings become the reference. | Broadside3D | L, opus-xhigh | BS3; Chris's decision | synctest and new recordings pass |

### 4.1 Order and parallel map

```
Now:      NA1 ──┬──> NA2 ──> NA3 ──> NA4 ──┬──> NA5 ──> NB1 ──┐
                └──> NM1 (parallel with NA2-NA4) ──────────────┤
                                            └──> NT1 (wasm part after NA5)
          BS1 (Broadside, now; needs nothing)                  │
                                     BS2 (after NA4 + NT1) ──> BS3 (after BS2, NM1, NB1) ──> NP1 (after all engine) 
```

Runs in parallel: **NM1 with NA2, NA3 and NA4**; **BS1 with everything**; **NT1 with NA5** (its native part); **NB1 with BS2**. Strictly in order: NA1 → NA2 → NA3 → NA4 → NA5. At most two packages building at once on this PC.

### 4.2 Byte budgets

| Piece | Budget |
|---|---|
| Any game without `authority`, `matchflow` or `referee-abi` (all 8 games today, the kart racer's lockstep client and referee included) | **0 bytes**: SHA-1 identical (2.1) |
| `authority` client in a game (view decode, bit reader, interpolation buffers, input packets) | at most **6 KB gz wasm** |
| `matchflow` client (public block decode only; the tally runs on the referee) | at most **1.5 KB gz wasm** |
| Server parts (`AuthorityServer`, view builder, matchflow logic, ABI) in a referee | at most **40 KB raw** wasm over the game's rules |
| Broadside referee wasm | at most 1 MiB (ChimpVibe's cap), target 400 KB raw |
| Broadside client: rules leave the player download (only the referee simulates) | expected **smaller** than today's client by the rules' share; measured in BS3 |
| Down per player, 12 ships, 20 Hz | `net.view_budget_bytes` 512 per view (10 KB/s ceiling); typical about 200 to 250 bytes (4 to 5 KB/s, about 6.5 KB/s as base64 JSON) |
| Down per spectator | `net.spectator_budget_bytes` 1,024 per view (20 KB/s ceiling) |
| Up per player | one packet every 3 ticks with 4 actions of about 6 bytes: about 30 bytes, 0.6 KB/s, about 1 KB/s as base64 JSON |
| Referee CPU | `step(3)` with 12 recipients' views at most 1 ms on a server core |

---

## 5. For a game author: the integration API and workflow, and Broadside's migration

### 5.1 Workflow for any game

1. Put the rules in the kit behind a `rules` feature (the referee turns it on; the player build leaves it off). The kit's `State` is a plain struct with `Wire`.
2. Implement `AuthorityGame`: `sanitize`, `step` (60 Hz; use `ctx.rng(stream)`; bots inside), `entities`, `visible_set` (your fog rule), `view` (private block), optional `priority`, `results`.
3. Declare `SCHEMA`: one `EntityKind` per networked kind, one `Field` per drawn value, with ranges and bits.
4. Optional: embed `MatchFlow` in your state; declare questions; call `flow.step(..)` at the top of your `step`; write the public block with the helper.
5. Make a referee crate (copy `apps/kart-racer/referee`'s layout) that calls `export_referee!` with a function making an `AuthorityServer<YourGame>`.
6. In the player build: `AuthorityClient<YourGame>` over `HostPort` (or `AuthoritySession` offline with bots); draw from `client.entities_at(render_time)`, `client.private()`, `client.match_view()`, `client.events()`.
7. Tests: `authority::sync_test` on your step; record three matches per mode; the section 3 checks; a loopback test like NA4's.
8. Host: hand the host your `referee.wasm` (any `evani-referee/1` host runs it) or wrap it in a bridge outside the engine (`bridges/<host>/`).

### 5.2 What a player's game sees

```rust
let mut client = AuthorityClient::<MyGame>::new(cfg);
// each frame
while let Some(_) = port.recv(&mut buf) { client.on_view(&buf)?; }
client.set_action(my_action);              // held controls; press counters increment on a press
if let Some(pkt) = client.packet(&mut w) { port.send(w.as_bytes()); }   // every send interval
for e in client.entities_at(render_time) { draw(e.kind, e.id, e.values()); }
let me = client.private();                 // bytes from MyGame::view
let m  = client.match_view();              // with matchflow: phase, time left, tally, scores
for ev in client.drain_events() { play_cue(ev); }
```

### 5.3 Broadside migration guide (from rules.wasm plus JSON views to the authority trait, HostPort and a bridge)

Today: the host calls the rules (`initialize`, `step` every 50 ms with JSON commands, `project` per recipient to JSON) through Broadside's ChimpVibe shim; the page link (a copy of `HostPort`) passes JSON views to the kit, which draws them.

| Today | After |
|---|---|
| `initialize(seed)` with xorshift32 seeded from the host's seed | `AuthorityServer::new(seed, cfg)`; the kit seeds its xorshift from the session seed inside `State` (phase 1), or uses `ctx.rng` (phase 2) |
| host `step` every 50 ms | `step(3)`: three 60 Hz ticks; the kit's `step` runs v7's 50 ms step when `tick % 3 == 2` (fixed phase, recorded) and does nothing else on the other two ticks in phase 1 |
| JSON commands (`{type:"action", action:{type:"ship", data:"carrack"}}`, votes as `question:option`) | `Action` held fields: helm (-1/0/1), sail level, aim, press counters (fire port, fire starboard, repair, go ashore), walk direction, ship choice (enum), vote (question, option); `sanitize` clamps |
| the roster in every `step` | `join`/`leave`; the bridge reconciles the host's roster into them (as Sugarpeak's does) |
| `project(state, recipient)` JSON | `view(recipient)`: binary, quantised, fogged, budgeted; the bridge base64s it |
| fog in `view.rs` (ships only) | `visible_set`: the same rule (700 px from own ship, captain ashore or a team-mate) applied to ships, captains, balls, shots, effects and pools |
| standings for all ships (no positions) | the `matchflow` public block (seat scores, teams) plus a kit public block (names are host data and never enter the engine; ship names and classes are game data and may) |
| v7 events (feed lines, sinkings, banked gold) | `ctx.event(audience, ..)`: sinkings and phase changes to `All`, gun smoke and splashes to `Near(point)`, banked gold to `Seat`/`All` as today |
| `lobby.rs` + `modes.rs` phases | `matchflow` with questions `mode`(3), `ammo`(4), `damage`(2), `fog`(2); timers 20/20/5/600/12 s; `ScoreToWin 10` (team, ffa), `TimeLimit 10 min`, race via `check_win` |
| damage stage drawn from `hp` | a 2-bit `stage` field (or kept derived on the client from the quantised `hp`) |
| dig-out timer, destroyed piers, resupply progress (planned, not built) | when built: `grounded_ticks` `UInt{bits: 10}`; pier entity with `destroyed: Bool` plus a reliable cue; `resupply` `UInt{bits: 8}` (0-255) on the ship, visible only with the ship |
| `link` crate | `evani_net::port::HostPort` (audit A2) |
| ChimpVibe shim calling the rules | NB1's generic shim calling `evani-referee/1` |

**How today's behaviour becomes the parity reference (recorded matches).**
1. BS1 records matches from the current rules exactly as the host runs them (50 ms steps, JSON commands, roster changes) and stores, per step, the state hash and every recipient's JSON view.
2. BS2 converts each recording's commands into `Action` changes at tick `3 * step + 2` (the conversion is a small, tested function in Broadside's tests) and replays them through `AuthorityServer`.
3. Checks at every view tick (every third tick): the kit state hash equals the recorded rules state hash (the kit's state **is** the rules' state, so this is exact); each recipient's decoded view equals the recorded JSON view field by field within that field's quantisation step, and has exactly the same set of visible ids **plus** no balls, shots or effects outside sight (the deliberate fog fix: the comparison ignores those that the old view leaked, and the test lists them).
4. The vote tie-break in phase 1 uses a `TieBreak` that draws from the rules' xorshift exactly as `lobby::settle` does, so tied votes resolve as before. Phase 2 switches to the engine's `Pcg32` stream and re-records.

### 5.4 Two exceptions Broadside needs, both kit-side

- **The rules' own xorshift32 inside `State`** instead of `ctx.rng`, for phase 1 parity with v7. The engine is unaffected; the kit records it in its own DECISIONS note. It must still never read a clock. Ends with BS4.
- **A 20 Hz rules sub-step inside the 60 Hz step.** The step itself is 60 Hz as AGENTS requires; the rules advance on every third tick. No engine change.

Unlike the lockstep route in Broadside's audit (A1), only the referee simulates here, so the players' devices never need to agree bit for bit with JavaScript-number maths, and fog is real.

---

## 6. Questions only Broadside can answer (with defaults, so building is not blocked)

| # | Question | Default we build with |
|---|---|---|
| 1 | The exact fog rule | Today's rule, extended: an enemy ship, captain ashore, ball, shot, effect or whirlpool-made pool is visible if within 700 chart px (`FOG_R`, chart wrap respected) of the viewer's ship, captain ashore or any team-mate's ship or captain. Islands, rocks, shoals, wind and piers are public. Standings public without positions. No last-known ghosts in v1 (the client may keep a fading marker itself from what it last saw; no server data). Fog applies only in `ready` and `play` with fog voted on. |
| 2 | Byte budget per player and per spectator | 512 bytes per view per player, 1,024 per spectator (10 and 20 KB/s ceilings at 20 Hz; typical well under half). |
| 3 | Spectator fog and delay | Spectators see everything, no delay (as today). A host may set `net.spectator_delay_ms`; a team-fogged spectator view exists for a host that wants it. |
| 4 | Mid-match joins | Free-for-all and team battle: join now, team by smallest team, spawn through the respawn queue with today's harbour shield. Race: watch until the next match (`match.join_mid_match = next_match` for race only, set by the kit per mode). |
| 5 | Which events are reliable | Phase changes, results, sinkings, a destroyed pier, banked gold: reliable (repeated until acked). Gun smoke, splashes, hits, feed lines: cues, sent once with `Near` or `All` audience. Everything that decides play is state anyway. |
| 6 | Vote timers and options | Today's: mode vote 20 s (race, team, ffa), settings vote 20 s (ammo empty/half/full/unlimited, damage on/off, fog on/off), ready 5 s, match cap 10 min, 10 sinks to win, race 2 laps with 45 s grace, results 12 s, early close when everyone voted, ties and no votes random among leaders. |
| 7 | Classic (no modes) sessions | Kept: `matchflow` off for a classic session (all phase lengths 0, no questions), v7 behaviour unchanged. |

---

## 7. Stage placement under STAGES.md

- STAGES.md lists multiplayer in Stage 5 ("evani-net bots") and Stage 9 (online kits). Chris's request of 2026-10-24 ("I want you to fully build the Stage 3 multi-player engine ... Either way, get it done") overrides rule 1 **for the multiplayer engine build only**, recorded in STAGES.md's Stage 3 notes. Join 8 delivered that build (lockstep, referee ABI, page link, Sugarpeak online).
- This plan's engine packages (NA1 to NA5, NM1, NT1, NB1, NP1) are a new mode of the same multiplayer module. They fit the override's wording, but they were not in docs/NET_BUILD_PLAN.md, so whether they start now under it is Chris's call (the decision below). Without it, they wait for Stage 5.
- Belongs to later stages regardless:
  - Delta snapshots against acked baselines: Stage 7 (Lean delivery; bandwidth), and only when measured.
  - Own-ship prediction: Stage 5 at the earliest, only if steering feels laggy at real latency.
  - Binary views without base64, WebRTC and WebTransport: Stage 9 (Reality and reach), and need the host's agreement.
  - Lag compensation for the authority mode: not planned (naval shells are slow; `net.lag_comp_max_ms` stays in the snapshot core).
  - Matchmaking, rooms and accounts: never engine work (the host's job).
  - Replay viewer and kill-cam: Stage 8 or 9 (record now, view later).
- Broadside's packages (BS1 to BS4) are Broadside's own project work, outside Evani.Dev's stage gates. STAGES rule 9 (no game polish until Stage 8) does not block them; it does mean no Evani.Dev game is polished for this proof: the toy fogged arena in the tests is the engine's proof.
- No gate box is ticked by any of this.

---

## 8. Later, only when measured or asked

| ID | Package | When |
|---|---|---|
| NA6 | Acked-baseline deltas: per-entity "changed since baseline" bit mask, short baseline offset per entity, fall back to absolute past about 32 views (Gaffer, "Snapshot Compression"). | NP1 or BS3 shows views over budget at the target player count, or a host asks |
| NA7 | Own-ship prediction with reconciliation (sequence-numbered actions, replay unacked actions on the latest server state; irreversible things shown only on confirmation). | Steering feels laggy at real latency in BS3's browser runs |
| NA8 | Binary views without base64 in bridges. | A host's shell carries binary (ChimpVibe question C6.2) |
| NA9 | WebRTC or WebTransport transports. | A host wants unordered datagrams |

## 9. Prior art (ideas only; no code copied, no new dependencies)

| Source | Licence | Borrowed |
|---|---|---|
| Gaffer on Games, "Snapshot Compression", "State Synchronization", "Snapshot Interpolation" (gafferongames.com) | articles | quantisation to bounds, priority accumulator with a per-packet byte budget, reliability over an unreliable stream, interpolation delay of 2 to 3 send intervals; deltas later |
| Colyseus `StateView` (docs.colyseus.io/state/view) | MIT | per-client view with add and remove; fog by omission |
| naia (github.com/naia-lib/naia) | MIT OR Apache-2.0 | rooms plus per-user scope |
| bevy_replicon (github.com/projectharmonia/bevy_replicon) | MIT OR Apache-2.0 | per-client entity visibility |
| Nakama match handler (github.com/heroiclabs/nakama) | Apache-2.0 | init, join, leave, loop with queued messages; the phase as explicit state; seats, not accounts |
| Gabriel Gambetta, "Client-Side Prediction and Server Reconciliation" | article | for the later own-ship prediction (NA7) |
| GGRS (github.com/gschup/ggrs) | MIT OR Apache-2.0 | the sync-test idea, already in `synctest.rs` |

## 10. Open for Chris

1. **Start the engine packages now** under the 2026-10-24 multiplayer override, or wait for Stage 5? Recommended: now, because this is the multiplayer engine he asked to be fully built, Broadside needs it for real fog, and every piece is opt-in with a zero-byte proof for every other game.
2. Later, for Broadside's phase 1: allow its own xorshift32 inside its state for v7 parity (kit-side exception, ends with BS4). Recommended: allow; the engine is untouched.
3. Later: BS4 (end exact v7 parity) is Chris's call when Broadside gets there.
