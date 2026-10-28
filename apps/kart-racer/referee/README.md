# Sugarpeak Rally referee (headless)

The race rules of Sugarpeak Rally with nothing else: no graphics, no sound, no clock, no network, no randomness but the session seed. A host runs it on its server as the **referee** of an online race: players send only their inputs (steer, throttle, drift, item, brake); the referee runs the real race, drives the AI racers in every empty seat, decides the results, and sends each player the confirmed inputs of every seat. Each player's game runs the same rules from those inputs (so everyone sees the same race), predicts its own kart so it reacts at once, and checks a state hash the referee sends twice a second.

It is the same Rust code the game runs (`kits/evani-kit-kart-racer` without its `render` part), compiled to plain WebAssembly.

## Build

```sh
node bridges/chimpvibe/build.mjs          # both server files into bridges/chimpvibe/out/
```

or just this file:

```sh
CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS="--cfg=web_sys_unstable_apis --cfg=evani_headless" \
  cargo build -p evani-kart-racer-referee --release --target wasm32-unknown-unknown --target-dir target/referee
# -> target/referee/wasm32-unknown-unknown/release/evani_kart_racer_referee.wasm
```

`--cfg evani_headless` leaves the engine's browser glue out, so the module imports **nothing**. Use your own target dir for it (the flag rebuilds everything).

## Run: the `evani-referee/1` ABI

The module exports `memory` and the calls below (all numbers are 32-bit unless noted). Bytes go in through `evani_ref_alloc(len)`: write them at the pointer it returns, then pass that pointer and the length. Results that are bytes come back as an i64 `(ptr << 32) | len`, valid until your next call.

| Call | What it does |
|---|---|
| `evani_ref_abi() -> 1` | the ABI version |
| `evani_ref_init(seed_lo, seed_hi, cfg_ptr, cfg_len) -> 0` | a new session. Config: optional byte 0 = laps (1 to 9, default 3) |
| `evani_ref_join(peer, role) -> seat` | someone joined (role 0 player, 1 spectator). Returns their seat 0 to 7, or 255 when they watch (spectators, or a ninth player). `peer` is any number you choose per join |
| `evani_ref_leave(peer) -> 0` | they left: the AI takes their kart over |
| `evani_ref_input(peer, ptr, len) -> 0` | one packet from that player's game, exactly as it sent it (-3: refused and counted, nothing changed) |
| `evani_ref_step(ticks)` | advance whole 1/60 s ticks: `step(3)` for a 50 ms server step |
| `evani_ref_view(peer) -> i64` | the bytes to send that player now. `0xFFFFFFFF` gives the spectator view (one for every spectator; each is complete on its own) |
| `evani_ref_save() -> i64`, `evani_ref_load(ptr, len)` | a checkpoint of everything, and back (a restarted or moved session carries on bit for bit) |
| `evani_ref_results() -> i64` | phase, race number, then per place: kart, human (0/1), finished (0/1), time (f32 seconds) |
| `evani_ref_hash()`, `evani_ref_tick()` | for your logs |

A server loop, every 50 ms: pass on the packets that arrived (`input`), `step(3)`, then `view(peer)` for each player and once for the spectators, and send them. Joins and leaves whenever your roster changes. Nothing else: no timeouts, no idle kick (an idle player's kart just sits), no sanity checks (the referee runs the real rules, so there is nothing to check).

Node, in full:

```js
const { instance } = await WebAssembly.instantiate(readFileSync("referee.wasm"), {});
const x = instance.exports, mem = () => new Uint8Array(x.memory.buffer);
const put = (b) => { const p = x.evani_ref_alloc(b.length); mem().set(b, p); return p; };
const bytes = (r) => { r = BigInt.asUintN(64, r); const p = Number(r >> 32n); return mem().slice(p, p + Number(r & 0xffffffffn)); };
x.evani_ref_init(seed, 0, put(new Uint8Array([3])), 1);
x.evani_ref_join(1, 0);                              // a player
setInterval(() => {
  for (const [peer, packet] of arrived.splice(0)) x.evani_ref_input(peer, put(packet), packet.length);
  x.evani_ref_step(3);
  for (const peer of players) send(peer, bytes(x.evani_ref_view(peer)));
  sendToSpectators(bytes(x.evani_ref_view(0xffffffff)));
}, 50);
```

`bridges/chimpvibe/test/rules-harness.mjs` drives it exactly like this and checks it.

## The player's side

The game page talks to its host only through one `MessagePort` (`evani-net/1`, `crates/evani-net/web/port.js`): the host page posts `{t:"hello", v:1, role}` with a port, or an in-page adapter calls `evaniNet.attachPort(port, {role})`; then `{t:"data", b: Uint8Array}` both ways. The game opens no network of its own, and with no host it is the single-player game.

## Numbers (measured, see the hand-over notes)

Per player with 8 players, 20 messages a second each way: see `HANDOFF.md`. Inputs are 3 bytes a tick; a view carries the inputs of every seat since that player's last confirmed tick, a state hash, and the full state only on join or repair.
