# Evani.Dev live API (protocol `evani-live/1`)

The single contract between Evani.Dev's live system and Evani.App's **Evani.dev tab** (the live
engine view with "Talk to the AI"). Anything that talks to a running Evani.Dev game — the tab, an
AI agent through MCP, the in-page debug panel, a script — uses what is written here.

Status: M1 contract, written 2026-10-13. Evani.App (a separate product, not part of this
repository) is one client of this protocol; any other tool can use it the same way. Section 12
notes where the protocol matched that client when it was written.

Updated 2026-10-14 (stage 6, capture, bench and A/B) to match what is built. Every command in
section 6 is implemented, including `capture.frame`, `bench.run` and `compare.ab` (6.14–6.16);
nothing answers `planned: true` any more.

### Changes (2026-10-16, held page shots)

Additive. `capture.frame` takes `hold_after` (6.14): the clock stays held on the captured frame
after the reply, until the next capture, bench, compare or sweep run ends, so the review's page
and showcase screenshots of a moment show exactly that frame instead of the game a few frames
(a varying, wall-clock number of them) later.

### Changes (2026-10-16, edit transactions)

All additive, so the protocol stays `evani-live/1`.

- **Edit transactions** (6.31): a request that asks for several live changes at once ("turn the
  lights down, make the sky blue, reverse the spin") is split by the AI into atomic edits, and one
  sub-agent per edit stages its own edit into one transaction, all in parallel. New commands
  `edit.begin`, `edit.stage`, `edit.commit`, `edit.resolve`, `edit.status`, `edit.cancel` and MCP
  tools `edit_begin` … `edit_cancel` (11).
- Two apply modes: `stream` (each edit shows as soon as it is staged; the default) and `together`
  (all show on one frame at commit). New parameter `live.edit.mode` (never baked) sets the default.
- Two edits that change the same thing are a `conflict`, never resolved silently: neither stays
  applied until `edit.resolve`, and the AI asks the user which wins. New event `edit.conflict`.
- Each edit reports `applied`, `failed` (with the reason; the others carry on) or `conflict`. New
  error code `out_of_range` (5.5) for an edit's value outside its range (unless `clamp: true`);
  `param.set` itself still clamps.
- `history.list` entries of edits carry `txn` and `edit`; a held-back edit is `auto_reverted`
  with `revert_reason: "conflict"`.

### Changes (2026-10-14, live code swapping)

All additive, so the protocol stays `evani-live/1`.

- **Live code swapping** (10.4): `serve` builds the game with a pinned `dx` (Dioxus CLI) instead of
  Trunk, and a saved Rust change (a kit, a look, an engine crate or the game itself) is compiled
  into a small patch and swapped into the running page between two frames, with no reload: the
  game state, camera and live values stay. On by default; Trunk is the fallback when dx is missing
  or fails, and `/live/status` says which (`code`).
- The engine does it for every game: with the dev-only `hotpatch` feature (pulled in by every
  game's `live` feature) all systems, module frames, scene updates and the frame step run through
  the swap table. A game needs no code of its own. Shipped builds never contain it
  (`cargo xtask build --ship` fails if they do).
- New page messages `code_patching`, `code_patch`, `code_error`, `code_reload`; page events
  `code.patched`, `code.failed`, `code.crashed`; `/ctl` events `code.patching`, `code.error`,
  `code.reload`; the page's `hello` carries `code`. The tab hears about it through the messages it
  already reads (`live_status` `applied` / `error` and `live_swap_failed`, `kind: "code"`, 3.4).
- New commands `cargo xtask dev --hot <game>` and `cargo xtask dx-install`; `serve --no-hot`.

### Changes (2026-10-15, trials off-screen)

All additive, so the protocol stays `evani-live/1`.

- `sweep.run` and `compare.ab` (and a `capture.frame` of anything but the next frame as shown)
  run their trials off-screen: the visible canvas keeps the last frame it showed before the run,
  with a small "measuring…" tag, and only the state the run ends on shows (6.30). Results,
  captures and timings are unchanged. New `HoldCanvas` resource (6.14).

### Changes (2026-10-15, hidden tab and bakes in hot mode)

All additive, so the protocol stays `evani-live/1`.

- The page reports its state (`page_state`: frame counter, `document.visibilityState`, Evani.App's
  `window.__evaniHost`) on every change and as a 1 s heartbeat (3.5). While it is paused (hidden
  or paused by Evani.App AND not running frames) the bridge answers every command at once with the
  new error code `paused` (`data` = `{reason, fix, page}`) instead of a 10 s timeout, and
  `/live/status` gains `page`.
- While Evani.App parks the tab (`shown: false`) the page draws about 5 frames a second; the 60 Hz
  simulation and live commands are unchanged (3.5).
- In hot mode a bake or a restore of a look's or kit's data files no longer sets off a code swap;
  `/` carries the changed files and the page applies them at start (10.4). A code patch the page
  doesn't take within 15 s while running frames reloads the page (`code.stalled`, 10.4).

### Changes (2026-10-15, look guard)

All additive, so the protocol stays `evani-live/1`.

- Every real `bake.export` runs the look check first (section 15): the look's captures are
  compared with its baseline and limits, and a bake outside them (or on a look without limits
  Chris approved) is refused with `look_flagged` / `look_unapproved`. Dry runs are unchanged.
- New commands `look.status`, `look.check`, `look.limits.propose`, `look.baseline.propose`
  (MCP `look_status`, `look_check`, `look_limits_propose`, `look_baseline_propose`). `look.approve*`
  is always `forbidden` on `/live/v1/cmd` and `/ctl`; the AI has no approve tool.
- Approvals arrive only on a separate loopback listener (`--approve-port`,
  `POST /look-guard/v1/approve`), signed by Evani.App (15.7). `serve` reads Evani.App's key line
  from its first stdin line (15.6). `/live/status` gains `look_guard`; `/ctl` and `/evani-dev`
  get `look_guard.approval_requested` and `look_guard.approved` events.

### Changes (2026-10-14, page hot-replace)

All additive, so the protocol stays `evani-live/1`.

- The bridge watches the game's page files (`apps/<game>/index.html` and the stylesheets it
  links) and swaps a saved edit into the open page **in place**: no reload, no engine restart, the
  race and every live value untouched (section 14). New page message `page_patch`, new page event
  `page.patched`, and the page's `hello` carries `page_rev`.
- `/` is now composed from the page files as they are on disk plus the build's script tags, so a
  page edit never needs a wasm rebuild (14.2).
- An edit that can't be swapped safely (it changes ids or structure the game's code finds its
  parts by, or scripts) tells the tab through the existing contract: `live_swap_failed`
  ("needs a fresh start", the tab's Reload note) and `live_status` `page_changed` (14.5).
- Kits take HUD positions and sizes from tunables (the kart racer's `kit.kart_racer.hud_*`), so a
  layout change is a `param.set` (docs/KIT_PROCESS.md, "Look numbers are tunables").

### Changes (2026-10-14, sweeps)

All additive, so the protocol stays `evani-live/1`.

- New run command `sweep.run` (6.29): a list of values for one parameter, or a small grid, on the
  bench path; returns one compact table of per-pass costs and the look guard's measures, with
  optional thumbnails. Nothing is kept; `busy` as `bench.run`. MCP tool `sweep_run` (11) writes the
  thumbnails to files and returns paths.
- Tuning guidance: the MCP server's instructions and the `param_set` / `batch` descriptions now say
  to choose with `sweep_run` and apply the chosen set in one `param_set` or `batch`, not one
  command per change (docs/LIVE_TUNING.md measures the difference).

### Changes (2026-10-14, asset focus)

All additive, so the protocol stays `evani-live/1`.

- New read commands `focus.list`, `focus.set` and `focus.clear` (6.28): show one of the game's
  assets on its own on a turntable camera (the game paused, its animation still playing) and go
  back to the full game without reloading. MCP tools `focus_list`, `focus_set`, `focus_clear`
  (11). New error code `not_focused` (5.5).
- The page takes `?focus=<asset id>` (and `&instance=<n>`) to open on one asset; the older
  `?view=` and `&box=` spellings work too.
- While an asset is focused, `schema.get` lists its tunables first and adds
  `focus: { asset, params }`; `session.status` has a `focus` object.
- Kits list their assets with `KitPlugin::focus_assets` (core feature `focus`, turned on only by
  evani-live and kits' `live` features), so nothing of it reaches a shipped game.

### Changes (2026-10-14, geometry checks)

All additive, so the protocol stays `evani-live/1`.

- New read command `debug.geometry` (6.27): the geometry debug view (overlaps red, floating
  objects yellow, z-fighting cyan, the rest grey, the sky magenta so gaps show) and the issue list.
  MCP tool `debug_geometry` (11).
- `capture.frame` (6.14) takes `camera: {eye, target, fov_y_deg}`, `sky`, `jitter_px` and
  `geometry`, and `width` up to 4096 (never wider than the canvas). `CaptureQueue` has two more
  overrides, `clear_override` and `jitter_px`. The geocheck tool (docs/GEOCHECK.md) uses them.

### Changes (2026-10-14, sandbox review)

All additive, so the protocol stays `evani-live/1` (rule in section 3).

- Telemetry has `work_ms` (CPU + GPU time of frames not held on purpose), in change results
  (5.4) and subscriptions (6.12); held frames are explained in 8.
- Auto-revert judges `work_ms`, not the vsync-bound frame interval; its event says so with
  `detail.measure: "work_ms"` (6.24).
- `tier.set` to the VR preset outside an immersive WebXR session fails with the new code
  `needs_xr_session` (`data.selectable` lists the tiers that can be picked) (5.5, 6.11).

### Changes (2026-10-14, rim slot)

Additive, so the protocol stays `evani-live/1`.

- New built-in shader slot `rim` (rim light on lit surfaces), listed by `slot.get` / `schema.get` like the other slots. Looks that do not define it are unchanged.

### Changes (2026-10-14, versions)

Additive, so the protocol stays `evani-live/1`.

- `session.status` has `versions`: the engine version and git commit, the protocol, and every
  Evani.Dev part compiled into the game with its crate version (6.26). The debug overlay's first
  line shows the same list.

### Changes (2026-10-14, bridge follow-ups)

- `session.mirror` (10.3) is now forwarded to every `/ctl` client, always on (no `telemetry.subscribe`
  needed); the bridge still stores the latest one. Section 12: the Evani.dev tab can drop polling.
- The launcher shim (2.1) passes `--game <name|apps/name>` on to `evani-live-bridge serve`.
- The bridge (and `cargo xtask` trunk builds) remove `NO_COLOR` from trunk's environment, so
  `serve` works where `NO_COLOR=1` is set (trunk refuses "1").

### Changes in stage 6

Choices made while building `capture.frame`, `bench.run` and `compare.ab`:

1. **Readback is raw pixels.** The renderer returns full-size RGBA8 (`CaptureResult.rgba`);
   evani-live downscales and PNG-encodes with its own encoder, so no encoder ships in a game.
   Captures are requested as `CaptureView::Current` after the clock hold and path camera have
   been in place for 2 frames; a capture the renderer doesn't answer in 5 s fails with `timeout`
   (6.14).
2. **The sim clock hold** (`sim_time_override`) is honoured by the core loop behind the core
   `capture` feature: no fixed ticks run while it is set and the sim resumes where it paused.
   Bench and compare keep it set for the whole run, so both sides see the same frozen scene.
3. **`busy`.** While `bench.run` or `compare.ab` runs, every mutating command, `history.undo`,
   `history.redo`, `snapshot.save`, `bake.export` and the three run commands answer `busy` with
   `data{until_ms, run}`; `until_ms` is the estimated **remaining** milliseconds. Reads still
   answer. A `capture.frame` run (a few frames) just holds the inbox until it ends.
   `session.status.busy` is `{run, cmd, until_ms}` or `null` (6.26).
4. **Compare sides as changes.** `a`/`b` may be `"current"`, a snapshot id or name, or an object
   of changes to the current state: `{values:{…}}` or `{cmds:[…]}` (planned like `batch`). `a`
   defaults to `"current"`; `b` is required (6.16).
5. **`keep` / `note`.** `compare.ab` reverts by default; `keep: true` (needs a `note`) ends on B
   and records it as one `compare.ab` transaction, reported as `kept{tx, applied}` (or `null`
   when B equals the start state).
6. **`warmup_s`** for compare (default 0.5 s, before every run of the order); bench keeps 1 s.
   Compare defaults to `seconds: 4` and `capture_at: [seconds/2]`, so it always returns images.
   At most 8 `capture_at` times; `capture_width` 64–4096 (default 320).
7. **Difference measure.** Each compare capture pair carries `diff{mean_abs, rmse, max,
   changed_pct, psnr_db}` (0–255 per channel; `changed_pct` = pixels where any channel differs by
   more than 8); the top-level `diff` covers all pairs together. `psnr_db` is `null` when the
   images are identical.
8. **`bench.progress`** events (about once a second) carry `side: "a" | "b"` during a compare.
9. **`opts.capture`** (`{width}` or `true` for 480) works on every change result: the reply's
   `capture` is filled once the deferred reply is due (5.4).
10. **Bench path.** The game inserts `BenchPath` (the sandbox: its own camera path, under its
    `live` feature); without one, live uses a default orbit. Bench pass/fail rules are a copy of
    apps/bench's, so live never depends on the bench app.
11. **Tier runs.** With `tier`, the run's targets leave out the effect switches the tier change
    would reset, unless a side set them itself; targets turn adaptive quality off for the run.

### Changes in stage 5a

Choices made while building, now part of the contract:

1. **Change results are deferred.** A mutating command's reply waits `opts.telemetry_ms`
   (default `live.telemetry_ms`, 1000) of telemetry, drained per frame without blocking;
   `telemetry_ms: 0` replies at once with `telemetry: null` (5.4).
2. **Policy entries.** `policy.set` reports `kind: "policy"` with the param name and `field`;
   `weights.<effect>` maps to `fx.<effect>.value_weight`, any other field `x` to `aq.x`.
   `policy.get` returns the tier target when `aq.target_ms` is 0 (6.8, 6.9).
3. **Slot compiles.** A failed compile keeps `"previous"`; messages are
   `{severity, line, col, len, message, source_line}` in slot-body coordinates. A WGSL change no
   renderer picks up within 3 frames completes with `compile.checked: "structural"`; a compile
   not finished in 8 s fails with `timeout` (6.5).
4. **Snapshots.** The summary counts changes since the session started; restore is one
   transaction (6.17, 6.19).
5. **Tier changes are exact to undo.** The effect switches a tier change resets on the next frame
   are folded into the same transaction (6.11).
6. **`effect.toggle` scope `tier`** edits the preset's `effects_on` and the session switch in one
   transaction; `reason: "over_budget"` comes from the effects status after the frame (6.10).
7. **History.** Undo/redo are not watched (`watch.auto_revert: "off"`, `reason: "history"`) and
   clear the current watch; their `tx` is the newest transaction touched, plus
   `history{head, can_undo, can_redo}`. 500 transactions are kept; a new one after an undo drops
   the redo tail (6.20).
8. **Telemetry.** The page caps subscriptions at 16 per session (`limit`); the 4-per-connection
   cap belongs to the bridge, which ends a connection's subscriptions when it closes. New params
   `live.revert.window_s` (10) and `live.telemetry_ms` (1000). `baseline_frame_ms` is the average
   over the 1 s before the change; `dpr` is always `null`; samples sit in a 1024-entry ring (6.12).
9. **Auto-revert.** The 0.5 s average uses every sample, including ones from before the change.
   The revert is a new transaction (`cmd: "auto_revert"`) holding the inverse of the change, so
   undoing it re-applies the change; the original is marked `auto_reverted`. `device_lost`
   reports the current average as `observed_ms`. `session.status.watch` shows the armed watch
   (6.24, 6.26).
10. **Session mirror** carries history summaries and the snapshot list, no state values; the page
    sends it but does not restore from it yet (10.3).
11. **`bake.export`** goes page → bridge → disk: the page returns a bundle, the bridge plans with
    the shared native planner and writes. It is not a transaction. Details in 6.23.
12. **Renderer interface** for capture, bench and A/B lives in `evani_core::capture` as plain data;
    only evani-live inserts the queue (6.14). `App::new` registers `aq.*` and `DeviceStatus`.
13. **Core:** a look's `post_param` overrides apply before baked params, so baked values win;
    crates hand their baked files to the core with `PluginContext::baked_files`.

## 0. A first-class engine interface

AI is part of the engine, not bolted on. This control API is how the engine is tuned, by a person
or an AI, and it is a first-class engine interface, not a debugging add-on:

- **Every tunable is reachable through it.** Parameters, shader slots, curves, policies, effects
  and tiers are all registered in the always-included core (`evani-core`'s registry), not in the
  dev-only crate. Every module, kit, look and effect exposes its tunables there, so `schema.get`
  lists all of them and the commands below can read and drive all of them. A new engine feature
  isn't done until it shows up here.
- **Every change is measured**: each one returns the next second of telemetry, and can return a
  capture (5.4); `bench.run` and `compare.ab` measure on a deterministic camera path.
- **Every change is reversible**: one undoable transaction per change (6.20), and an automatic
  revert when it makes frames too slow or loses the GPU device (6.24).
- **Every change is bakeable into source**: `bake.export` writes the session into the game's
  presets, parameters, slot files and curves with a CHANGES.md from the change notes (6.23), so a
  build without the live module reproduces it with zero live overhead.

Only the doors to the registry (the in-page API, the bridge, history, snapshots, auto-revert,
captures and bake) are in the dev-only `evani-live` module; the registry itself ships in every
game, so a baked look is just its values.

Words: **MUST / SHOULD / MAY** as usual. "The bridge" = `tools/evani-live-bridge`. "The page" =
the running game (apps/sandbox or any game built with the `live` module). "Engine root" = the
folder chosen in the tab's settings (for this repo, `C:\dev\evani-dev`).

---

## 1. Overview and roles

```
 Evani.App                                  Evani.Dev (dev build, `live` module on)
 ─────────                                  ───────────────────────────────────────
 Evani.dev tab ──HTTP GET /live/status────▶ evani-live-bridge serve  (native, 127.0.0.1:P)
   (Electron main)──WS /evani-dev─────────▶   │  HTTP: the game page, /live/status
   page view ──HTTP GET / (the game)──────▶   │  WS:   /engine  /evani-dev  /ctl
                                              │
 Chat strip ─▶ orchestrator of the project    │◀──WS /engine (token)── the page: window.evani.live
              whose folder = engine root      │                         (crates/evani-live, wasm)
              (a Claude Code session)         │
                 │ MCP stdio                  │
                 ▼                            │
            evani-live-bridge mcp ──WS /ctl (token)──▶
```

| Role | What it is | Talks |
|---|---|---|
| `window.evani.live` | The control API inside the game (crates/evani-live, dev builds only). Reads and drives the registry in `evani-core` (parameters, slots, curves, policy, effects, tiers) and owns history, snapshots, telemetry streaming, captures and auto-revert. **The source of truth** for the live session. | In-page JS calls; one WebSocket to the bridge (`/engine`). |
| Bridge, `serve` mode | `tools/evani-live-bridge serve`: the "live server" the tab looks for. Serves the game page and `/live/status`, routes commands between controllers and the page, writes bake files, generates the dev token. Holds no render state of its own. | HTTP + WebSocket on one localhost port. |
| Bridge, `mcp` mode | `tools/evani-live-bridge mcp`: an MCP server on stdio, one per AI session. Each MCP tool is one command, sent over `/ctl`. | stdio (MCP) ↔ WS `/ctl`. |
| Evani.dev tab | Shows the page, starts the bridge if needed, shows build/live news, sends what you type to the engine project's orchestrator. Sends no control commands today (it doesn't need to). | HTTP `/live/status`, WS `/evani-dev`. |
| AI agents | The orchestrator (and its workers) of the Evani.Dev project in Evani.App, or any Claude Code session in the engine root. "Talk to the AI" reaches them as chat; they act through the MCP tools. | MCP. |
| Debug panel | The sandbox's minimal in-page panel (tier switch, effect toggles, overlay). Calls `window.evani.live` directly. | In-page. |

One API, three doors: in-page (`window.evani.live.call`), WebSocket (`/ctl`), MCP tools. The same
command names, parameters, results and error codes go through all three.

---

## 2. Discovery and launch (per project)

### 2.1 What the tab does today (and what Evani.Dev provides for it)

The tab (`findEngineFolder` in `evaniDevCore.ts`) looks in the chosen engine folder for exactly
one file:

```
<engine root>/.vibedeck/dev-harness/live.mjs
```

If the folder setting is empty it uses the first Evani.App project that has that file. It then:

1. Probes `GET <origin of the tab's URL>/live/status` (2 s timeout). The tab's URL setting is one
   global value, default `http://localhost:8095/?live=1`, localhost/127.0.0.1 only. For Evani.Dev
   it is set to `http://localhost:8096/?live=1` (section 3).
2. If nothing answers, launches `<Evani.App's Node> <engine root>/.vibedeck/dev-harness/live.mjs --port <port of the tab's URL> --quiet`
   with `cwd` = the `dev-harness` folder, env `ELECTRON_RUN_AS_NODE=1`, below-normal priority,
   output to Evani.App's own `live-server.log`. It waits up to **60 s** for `/live/status` to
   answer, then loads the URL. On quit it kills the whole process tree (`taskkill /T /F`).

**Evani.Dev therefore ships a launcher shim at `.vibedeck/dev-harness/live.mjs`.** The tab needs
no change to find and start the bridge. The shim MUST:

- use only Node built-ins (the tab runs it with Electron's Node; Evani.Dev has no `node_modules`);
- accept `--port <P>`, `--quiet` (and ignore unknown flags);
- resolve the engine root as `../..` from its own folder;
- if `target/debug/evani-live-bridge(.exe)` is missing or older than anything under
  `tools/evani-live-bridge/` or `Cargo.lock`: first bind `127.0.0.1:<P>` itself and answer
  `GET /live/status` (the usual JSON with `phase: "building"`, `engine_connected: false` and an
  extra `"launcher": "building-bridge"`) and `GET /` (a holding page that polls `/live/status`
  and reloads once the `launcher` field is gone) while it runs
  `cargo build -q -j 4 -p evani-live-bridge` at below-normal priority; then close that server;
- run, in the engine root, `target/debug/evani-live-bridge(.exe)` with arguments
  `serve --root <engine root> --port <P> [--quiet]`, stdio inherited;
- exit with the bridge's exit code, and kill the bridge if it is itself told to stop.

So the tab's 60 s wait is never spent compiling: `/live/status` answers within a second of the
launch, first start or not, and the holding page carries the user through the bridge compile and
then the game build (2.3). If the bridge doesn't compile, the launcher shows `phase: "error"` with
the end of cargo's output on the holding page for 30 s, then exits with cargo's code, so the tab's
**Try again** starts a fresh attempt. `cargo xtask live` (below) does the same without the tab.

### 2.2 Starting it without the tab

```
cargo xtask live [--game apps/sandbox] [--port 8096]      # builds bridge + game, runs serve
cargo run -p evani-live-bridge -- serve --root . --port 8096
```

### 2.3 What `serve` does

1. Generates the dev token (section 4) and writes the session file `.evani/live/session.json`.
2. Binds `127.0.0.1:<P>` (HTTP and WebSocket on the same port).
3. Builds the game in dev mode with the `live` module (Trunk, below-normal priority, capped jobs)
   unless a current build exists. While building, `/` serves a small holding page ("Building the
   game…") that polls `/live/status` once a second and reloads itself when `phase` is `idle`.
   The build goes to `target/evani-live/<game>/dist` with its own `CARGO_TARGET_DIR`
   (`target/evani-live/cargo`), so it never waits on, or invalidates, the workspace's own
   `target/` builds; a stamp file in `dist` records the features, and the build counts as current
   while it is newer than every source file that goes into it.
4. Serves the built game at `/` (with the token injected, section 4.3).

### 2.4 The session file (for MCP and scripts)

`<engine root>/.evani/live/session.json`, rewritten at every start, deleted on clean exit.
`.evani/` MUST be in `.gitignore`. With several sessions it is the most recent one, and each also
has `.evani/live/sessions/<port>.json` (2.6).

`serve --session <file>` writes (and on exit removes) that file instead. Bridges started by
the review and test scripts (`tools/live-check/capture.mjs`, `pilot.mjs`) use
`.evani/live/review/<port>/session.json`, so they never overwrite or delete the Evani.dev tab's
session file; `cargo xtask review` starts its bridge from port 8180, away from the tab's 8096.

```json
{
  "protocol": "evani-live/1",
  "pid": 23816,
  "root": "C:\\dev\\evani-dev",
  "game": "sandbox",
  "http": "http://127.0.0.1:8096",
  "ws": "ws://127.0.0.1:8096",
  "token": "5b0c0f2e8d1a4c7e9f3b6a2d0e4c8b1f5a9d3e7c2b6f0a4d8e1c5b9f3a7d2e6c",
  "started": "2026-10-13T14:02:11Z"
}
```

A stale file (its `pid` not running, or the port not answering) means "no bridge".

### 2.5 Which AI gets "Talk to the AI"

The tab sends what you type to the orchestrator of **the Evani.App project whose folder is the
engine root** (`findLiveEngineProject`), falling back to a project named "Live-Engine". With the
tab's engine folder set to `C:\dev\evani-dev`, that is the Evani.Dev project. The message
arrives as `[Evani.dev] <your words>` (plus `Worker: <name> (chosen by the user)` when a worker is
picked). The orchestrator reaches the engine through the MCP server registered in the engine
root's `.mcp.json` (section 11).

### 2.6 Several live sessions in one folder

Several bridges can run at once in one engine folder, each on its own port (for example two
games side by side, or the Evani.dev tab on one game while a script drives another).

- **Ports.** `cargo xtask live` and `cargo xtask dev` without `--port` pass `serve --any-port`:
  8096 if it is free, else the first free port from 8098 to 8139 (8097 is the Model tab's, 8180 up
  the review bridges'), else any free port. The bridge prints the address it took
  (`port 8096 is taken (…); using 8098`, then `serving <game> on http://localhost:8098/?live=1`).
  With `--port <P>`, and without `--any-port` (the Evani.dev tab's launcher, 2.1, and the review
  and test scripts), a taken port is an error as before, so the tab still finds its bridge on
  8096 or reports that something else holds it. `live --models` keeps 8097.
- **Session files.** A bridge on the default session file writes
  `.evani/live/sessions/<port>.json` (same fields as 2.4, plus `started_ms`) and also
  `.evani/live/session.json`, which is therefore always the **most recently started** session.
  On clean exit it removes its own `sessions/<port>.json`; if `session.json` is still its own, it
  hands it to the newest session that still answers on its port, or removes it when none is left.
  Scripts that read `session.json` keep working and reach the newest session. Bridges with
  `--session <file>` (review, tests, the Model tab) write only that file, as before.
- **Build folders.** The first bridge on a game builds into `target/evani-live/<game>/` and the
  shared cargo and dx target dirs, as before; it records itself in
  `target/evani-live/<game>/live-owner.json` (`pid`, `port`). A bridge that starts while another
  running bridge (its port answers) holds that file builds into `target/evani-live/<game>-port<P>/`
  (`dist/`, and its own `cargo/` and `dx/` target dirs), so two builds of the same game never
  share a dist folder or wait on one cargo lock. If the shared Trunk build is current it is copied
  in first, so a Trunk bridge serves at once. A stale owner file (its port silent) is taken over.
  Different games already have different dist folders; their Trunk builds share
  `target/evani-live/cargo` (cargo's lock lets one compile at a time) and dx's share
  `target/evani-live/dx`.
- **MCP.** Every MCP tool that talks to a game takes an optional `session` argument: a port
  (`"8098"`) or a game name (its newest running session; a value ending in `.json` or holding a
  slash is a session file path relative to the root). Without it, tools use
  `EVANI_LIVE_SESSION` (same values) when that is set in the MCP server's environment, else
  `session.json`: the most recent session. The `live_sessions` tool lists every session file
  (`sessions/`, `model/`, `review/*/`) newest first with `port`, `game`, `kind`, `started`, `pid`,
  `alive`, `url` and which one is the `default`. The MCP server keeps one `/ctl` connection and
  reconnects when a call names another session, or when the file now holds another token (a
  newer session took over `session.json`). To pin one Claude Code session to one game, set the
  variable in `.mcp.json`:

```json
{ "mcpServers": { "evani-live": { "type": "stdio", "command": ".evani/bin/evani-live-bridge", "args": ["mcp"],
  "env": { "EVANI_LIVE_SESSION": "kart-racer" } } } }
```

Two sessions of the same game from two terminals:

```
cargo xtask live --game sandbox      # 8096, target/evani-live/sandbox/
cargo xtask live --game kart-racer   # 8098, target/evani-live/kart-racer/
cargo xtask live --game sandbox      # 8099, target/evani-live/sandbox-port8099/
```

Dev tools only: none of this is in a game's download (`live` is dropped from ship builds).

---

## 3. Transport

One port `P` on `127.0.0.1`. Evani.Dev's default is **8096** (decided by Chris 2026-10-13). The old
Live-Engine (8095) is retired and no longer used (Chris, 2026-10-13); Evani.Dev keeps its own port
anyway so nothing left over on 8095 can be mistaken for it. When the tab starts the bridge it passes the port of its
own address setting, and the bridge uses that; so the tab's address for Evani.Dev is
`http://localhost:8096/?live=1` (a settings change in the tab, not a code change).
Never `0.0.0.0`. Every request's `Host` header MUST be `localhost:P` or `127.0.0.1:P`, else 421
(stops DNS-rebinding pages).

The `protocol` value is the contract version the tab checks. Bump it to evani-live/2 only for changes that aren't backwards compatible; additive changes keep /1.

### 3.1 HTTP

| Method, path | Auth | What |
|---|---|---|
| `GET /` and the game's files | none | The game page (token injected into `index.html`, 4.3). |
| `GET /live/status` | none | Status JSON (below). Never contains the token. |
| `GET /live-pkg/<n>/…` | none | Live builds (section 10), when rebuilds are on. |
| `POST /live/v1/cmd` | token | One command (an envelope object, answered with one result) or a JSON array of envelopes (run in order, answered with an array of results), same envelope as WebSocket, for curl-style clients. Rejected if an `Origin` header is present. An array is *not* atomic; use the `batch` command for that. |

`/live/status` (a superset of Live-Engine's, so the tab reads it unchanged):

```json
{
  "type": "live_hello",
  "build": 0,
  "url": null,
  "phase": "idle",
  "error": null,
  "build_ms": 0,
  "relay": "ws://localhost:8096",
  "engine": "evani.dev",
  "protocol": "evani-live/1",
  "root": "C:\\dev\\evani-dev",
  "game": "sandbox",
  "engine_connected": true,
  "tier": "medium",
  "page": { "state": "running", "reason": null, "visibility": "visible",
            "host": { "v": 1, "shown": true, "paused": null },
            "frames": 5821, "frames_moving": true, "last_heard_ms": 412 }
}
```

`page` (3.5) is `{"state":"none"}` with no page connected, `{"state":"unknown"}` until the page's
first `page_state` (a page built before 3.5), and otherwise `running` or `paused` with the
`reason`.

`build`/`url` follow `docs/evani-live-builds.md` in Evani.App: `build` 0 and `url` null mean
"no numbered live build; load `/`". `phase` is `idle | building | error`.

### 3.2 WebSocket endpoints (same port)

| Path | Who | Auth | May send | Receives |
|---|---|---|---|---|
| `/engine` | the page (exactly one active; a newer one replaces the older) | token | results, events, `hello` | commands from controllers |
| `/ctl` | controllers: the MCP server, scripts, a future tab panel | token | commands | results to *its own* commands; events it subscribed to; broadcast events |
| `/evani-dev` | the Evani.dev tab | none (notifications only) | `chat_reply` only; anything else is dropped | `live_status`, `live_swapped`, `live_swap_failed`, `chat` (tab-format notes, 3.4) |

`/evani-dev` needs no token because it cannot change anything: it only receives news and may send
a `chat_reply` that the page shows as a toast. That keeps the tab working unchanged while every
command path requires the token.

### 3.3 Message envelope (`/engine`, `/ctl`, `POST /live/v1/cmd`, in-page)

Every message is one JSON object with a `type`, UTF-8 text frames, at most 8 MiB per frame
(captures are the large ones).

Request (controller → bridge → page):

```json
{ "type": "cmd", "v": 1, "id": "c-42", "cmd": "param.set",
  "params": { "values": { "fx.bloom.intensity": 0.9 } },
  "note": "Bloom a touch stronger so the sunset glows",
  "opts": { "telemetry_ms": 1000, "capture": false } }
```

| Field | |
|---|---|
| `id` | Client-chosen string, unique per connection, ≤ 64 chars. Echoed in the result. |
| `v` | Protocol major version. Must be `1`. |
| `cmd` | Command name (section 6). |
| `params` | Command parameters (object, may be `{}`). |
| `note` | Why. **Required on every mutating command** (section 5.3); ignored elsewhere. 1–500 chars. |
| `opts` | Optional. `telemetry_ms` (0–5000, default 1000; 0 = return at once), `capture` (`false` or `{ "width": 480 }`). |

Success:

```json
{ "type": "result", "v": 1, "id": "c-42", "ok": true, "result": { } }
```

Failure:

```json
{ "type": "result", "v": 1, "id": "c-42", "ok": false,
  "error": { "code": "unknown_target", "message": "No parameter named fx.blom.intensity", "data": { "did_you_mean": ["fx.bloom.intensity"] } } }
```

Event (page → bridge → subscribers / all controllers):

```json
{ "type": "event", "v": 1, "event": "telemetry", "sub": "t-1", "data": { } }
```

The bridge stamps `source` (`"mcp" | "ctl" | "http" | "page"`) on each command before forwarding;
history entries record it.

Hello: on connect, the bridge sends every `/ctl` client:

```json
{ "type": "hello", "v": 1, "protocol": "evani-live/1", "engine": "evani.dev", "game": "sandbox",
  "engine_connected": true, "schema_version": "c3f1a9e2", "session": "s-7f2c" }
```

If the page isn't connected, commands fail fast with `no_engine` (they are not queued).

### 3.4 What the tab's channel carries (`/evani-dev`)

Exactly the shapes the tab already parses (`parseRelay` in `evaniDevCore.ts`):

| Message | When |
|---|---|
| `{"type":"live_status","at":<ms>,"state":"applied","kind":"live","message":"Bloom intensity 0.6 → 0.9 (AI)"}` | after every committed change (one line; the tab shows it in its toolbar) |
| `{"type":"live_status","at":<ms>,"state":"reverted","kind":"live","message":"Change #17 undone: frame time stayed 41% over the 16.7 ms target for 3 s"}` | auto-revert (6.24) |
| `{"type":"live_status","at":<ms>,"state":"rebuilding" \| "built" \| "error" \| "page_changed", …}` | live rebuilds only (section 10) |
| `{"type":"live_swapped",…}`, `{"type":"live_swap_failed",…}` | live rebuilds (section 10) |
| `{"type":"live_status","at":<ms>,"state":"applied","kind":"page","files":["hud.css"],"message":"Swapped in place, no reload: hud.css (1 style)"}` then `{"type":"live_swapped","at":<ms>,"build":0,"kind":"page","files":[…]}` | a saved page file swapped into the open page (14.4) |
| `{"type":"live_swap_failed","at":<ms>,"build":0,"kind":"page","message":"needs a fresh start","reason":"…"}` then `{"type":"live_status","state":"page_changed","kind":"page",…}` | a page edit that can't be swapped in place (14.5) |
| `{"type":"live_status","at":<ms>,"state":"applied","kind":"code","message":"Code updated: 412 functions, 3.6 s, no reload"}` | a saved code change swapped into the running game (10.4) |
| `{"type":"live_status","at":<ms>,"state":"error","kind":"code","message":"Code change didn't build: error[E0609]: no field `lapz` on type `&PlayerHud` (apps/kart-racer/src/ui.rs:199)"}` | a saved code change that doesn't compile; the game keeps running the old code (10.4) |
| `{"type":"live_swap_failed","at":<ms>,"build":0,"kind":"code","message":"needs a fresh start","reason":"…"}` | a code change too big to swap (the page reloads with a full rebuild), a patch the page couldn't load, or a crash in changed code (10.4) |

Rules for the bridge, so the tab's toolbar logic reads them right: `state` for live changes is
never `error` and messages never contain "failed to build" (the tab would show "The last change
didn't build"). `error` is used only for a game build that failed, and for a code change that
didn't compile (10.4), which is the same thing for the person who saved it.

`chat_reply` received on `/evani-dev` (`{"type":"chat_reply","text":"…","from":"evani-dev"}`) is
forwarded to the page, which MAY show it as a toast in the debug panel.

### 3.5 Page state, paused answers and parked pacing (dev builds only)

The page answers live commands between two frames, so a page that runs no frames (a hidden or
suspended tab) answers nothing. So that a caller hears that at once instead of after the 10 s
limit, the page reports its own state and the bridge answers for it while it is paused. All of
this is in evani-live (the `live` feature); shipped builds contain none of it.

**What the page reads.** Evani.App's host state (`src/shared/evaniDevHidden.ts`): the object
`{ "v": 1, "shown": bool, "paused": null | "paused (tab hidden)" | "paused (window minimised)" |
"paused (window hidden)" | "paused (window covered)" }`, set as `window.__evaniHost` and sent as
the `detail` of a `CustomEvent("evani-host")` on `window` (on every dom-ready, on tab visibility
changes, de-duplicated). The page reads `window.__evaniHost` at start and on every `evani-host`
event, and `document.visibilityState` on every `visibilitychange`. Outside Evani.App there is no
`__evaniHost` and nothing below changes the game.

**`page_state` (page → bridge, on `/engine`).** Sent right after `hello`, at once on every change
of visibility or host state, and as a 1 s heartbeat from a timer (`setInterval`, which still runs
while the browser gives the page no animation frames):

```json
{ "type": "page_state", "v": 1, "reason": "hello" | "beat" | "visibility" | "host",
  "frames": 5821, "visibility": "visible" | "hidden",
  "host": { "v": 1, "shown": false, "paused": null } | null,
  "rendered": 1204, "skipped": 4617, "render_interval_ms": 200 }
```

`frames` counts the page's frames (every `LiveModule::frame`); `rendered` / `skipped` are the
renderer's drawn and skipped frames (pacing, below).

**The paused rule (bridge).** A page is *paused* when it has a reason AND is stalled:

- reason: `host.paused` (Evani.App's text, at most 60 characters), or else `visibility: "hidden"`,
  which counts as `paused (tab hidden)`;
- stalled: no `page_state` for more than 2.5 s (if Windows suspends the page, no script runs and
  the heartbeat stops), or `frames` stood still between two heartbeats.

A page Evani.App calls paused that still runs frames (the parked tab, below) is not paused: it
answers. A visible page that falls silent (a long task) is not paused either; its commands wait
out their limit as before.

**Paused answers.** While the page is paused, every command that would go to the page (all of
section 6, including `session.status`, so MCP `live_status`) is answered at once, not sent:

```json
{ "type": "result", "v": 1, "id": "c-9", "ok": false,
  "error": { "code": "paused",
             "message": "paused (tab hidden): the game in the Evani.dev tab isn't running, so it can't answer session.status (not sent). Show the Evani.dev tab, then try again.",
             "data": { "reason": "paused (tab hidden)",
                       "fix": "Show the Evani.dev tab in Evani.App (bring its window back if it is minimised or covered); the game runs again and live commands answer. Ask the user to do that if you can't.",
                       "page": { "state": "paused", "reason": "paused (tab hidden)", "visibility": "hidden", "host": { … }, "frames": 5821, "frames_moving": false, "last_heard_ms": 2610 } } } }
```

A command already sent when the page pauses is answered the same way within about 100 ms (the
message then says it was already sent and may still run when the game runs again). The look
check (section 15) passes the paused error on unchanged. `/live/status` `page` (3.1) says
`running` or `paused` with the reason, without asking the page. **For agents:** a `paused` error
is not a bug in the game or the tool; ask the user to show the Evani.dev tab, then retry.

**Parked pacing.** While `host.shown` is `false` (Evani.App parks the hidden tab just off-window,
still running at full rate), the renderer draws at most one frame per 200 ms (about 5 fps) and
skips the rest; the fixed 60 Hz simulation, live commands and the heartbeat keep running on every
frame, so determinism and answers are unchanged. A frame is always drawn when a capture is
waiting, a capture camera override is on or a readback is in flight, and pacing is off while a
`bench.run`, `compare.ab` or `sweep.run` measures. `shown: true` returns to full rate on the next
frame. The page shows its counts on `window.__evani_host_state`
(`{frames, rendered, skipped, render_interval_ms, shown, visibility}`, updated with every
`page_state`), for tests.

**Code patches while no frame runs.** A ready code patch (10.4) is normally applied at the top of
a frame. If no frame ran since the last heartbeat, the heartbeat applies it (a timer callback
always runs between two frames).

---

## 4. Auth: the dev token

### 4.1 Generation and storage

- 32 random bytes from the OS RNG, hex, generated at every `serve` start. Never logged, never in
  `/live/status`, never in a URL the bridge prints.
- Stored only in `.evani/live/session.json` (2.4) and injected into the served page (4.3).

### 4.2 How clients present it

| Client | How |
|---|---|
| Native (`/ctl`, HTTP) | `Authorization: Bearer <token>` header on the upgrade / request. |
| The page (`/engine`) | Browsers can't set headers: first message after open is `{"type":"auth","v":1,"token":"<token>"}`; anything before it, or a wrong token, closes the socket with code 4401. |

Compare in constant time. Wrong or missing token: HTTP 401 / WS close 4401, error `unauthorized`.

### 4.3 The page's copy

When serving `index.html`, the bridge inserts
`<meta name="evani-live-token" content="<token>">` into `<head>`. `window.evani.live` reads it to
open `/engine`. The page is same-origin with the bridge, so other web pages can't read it.

The page's side of `/engine` (`crates/evani-live/src/ws.rs`):

- opens only when that meta tag is present (the page was served by the bridge) and the address
  does not contain `live=0`; otherwise there is no socket at all;
- sends `auth` on open, then `hello` (`{"type":"hello","v":1,"protocol","game","schema_version","tier"}`)
  at the next frame;
- queues each incoming `cmd` with the in-page ones, answers at the frame boundary and sends the
  `result` back on the socket it came from;
- reconnects with backoff (1 s doubling to 30 s), but not after close code 4401 (wrong token) or
  4000 (a newer page took over), so two open pages never fight over `/engine`;
- never writes to `console.error`, and a blocked socket never stops the game starting.

### 4.4 Origin checks

| Endpoint | Allowed `Origin` |
|---|---|
| `/engine` | exactly `http://localhost:P` or `http://127.0.0.1:P` |
| `/ctl`, `/evani-dev` | none (native clients), or `http(s)://localhost` / `127.0.0.1` with any port (the same rule Live-Engine's relay uses) |
| `POST /live/v1/cmd` | none only |

Anything else: refused at the upgrade (HTTP 403), error `forbidden_origin`.

---

## 5. Rules shared by all commands

### 5.1 Versioning

- `protocol` is `evani-live/<major>`. Additive changes (new commands, new optional fields, new
  result fields) keep the major; clients MUST ignore unknown fields.
- `v` on every message is the major. A different `v` → `version_mismatch` (with
  `data.supported: [1]`).
- `schema.get` returns `protocol` and `schema_version` (a hash of the registry: names, types,
  ranges). It changes when the game is rebuilt with different parameters; clients that cache the
  schema refetch when `hello.schema_version` differs.

### 5.2 Batching and atomicity

- `param.set`, `curve.set`, `policy.set` take several values at once; each call is **one
  transaction**: all values are checked first, then all applied on the same frame, or none.
- `batch` (6.25) runs several mutating commands as one transaction (one history entry, one
  frame). If any part fails (including a slot compile), nothing is applied.
- Clamping is not a failure: out-of-range numbers are clamped and reported (`clamped: true`).
  Wrong types, unknown names and bad enum values are failures (`invalid_value`, `unknown_target`).
- Commands from all clients are applied in arrival order at frame boundaries. While `bench.run`
  or `compare.ab` runs, mutating commands, undo/redo, `snapshot.save`, `bake.export` and the run
  commands fail with `busy` (`data{until_ms, run}`, `until_ms` = estimated remaining ms); reads
  still answer. A `capture.frame` holds the inbox for its few frames instead.

### 5.3 Change notes

Every mutating command (`param.set`, `slot.set`, `curve.set`, `policy.set`, `effect.toggle`,
`tier.set`, `snapshot.restore`, `batch`) MUST carry `note`; without it → `note_required`.
`history.undo/redo` take an optional note. The debug panel writes its own short notes
("Debug panel: Bloom off"). Notes are stored in history with the source and time, and
`bake.export` turns them into `CHANGES.md` (6.23).

### 5.4 The change result

Every mutating command returns the same shape:

```json
{
  "tx": 17,
  "frame": 9312,
  "applied": [
    { "kind": "param", "name": "fx.bloom.intensity", "old": 0.6, "new": 0.9, "requested": 0.9, "clamped": false }
  ],
  "compile": null,
  "budget": { "tier": "medium", "points_used": 9, "points_limit": 11, "switched_off": [], "switched_on": [] },
  "telemetry": {
    "window_ms": 1000, "frames": 60,
    "frame_ms": { "avg": 16.7, "p95": 17.2, "max": 18.9 },
    "cpu_ms": { "avg": 3.1 },
    "work_ms": { "avg": 8.5 },
    "gpu_ms": { "avg_total": 5.4, "passes": { "forward": 3.1, "bloom.bright": 0.3, "bloom.blur": 0.8, "vignette": 0.1, "tonemap": 0.2 } },
    "gpu_timing": "timestamp",
    "draw_calls": 122, "triangles": 184000, "sprites": 3200,
    "baseline_frame_ms": 16.6
  },
  "capture": null,
  "watch": { "auto_revert": "armed", "target_ms": 16.7, "limit_ms": 20.9, "sustain_s": 3, "window_s": 10 }
}
```

- `frame`: the first frame rendered with the change (the spec's "within a frame").
- `applied[].kind`: `param | slot | curve | policy | effect | tier`. Shapes:
  - `param` / `policy`: `{ kind, name, old, new, requested, clamped }`; `policy` entries (any
    `aq.*` or `*.value_weight` param) also carry `field` when set through `policy.set` (6.9).
    Setting `fx.<e>.enabled` directly reports `kind: "effect"`.
  - `slot`: `{ "kind": "slot", "name": "tonemap", "old_hash": "…", "new_hash": "…" }` (WGSL and
    expression slots alike).
  - `curve`: `{ kind, name, old, new, requested, clamped }` with point arrays.
  - `effect`: `{ kind, name, scope, old, new }` (6.10), or
    `{ kind: "effect", name: "tier.<id>.effects_on", scope: "tier", old, new }` for a preset edit
    seen in undo/restore; `reason: "over_budget"` is added when it was asked on but stayed off.
  - `tier`: `{ "kind": "tier", "name": "tier", "old": "medium", "new": "low" }`.
- `compile`: set when a slot or expression was involved (6.5).
- `budget`: `points_used` / `points_limit` and `switched_off` / `switched_on` (effects the budget
  switched) are filled in when the deferred reply is sent, after the change has run.
- `telemetry`: the next `opts.telemetry_ms` after the change (default: param
  `live.telemetry_ms`, 1000; clamped 0–10000). The reply is **deferred** that long: the page keeps
  running and drains due replies once per frame, never blocking. With `telemetry_ms: 0` the
  reply is sent at once with `telemetry: null`. `baseline_frame_ms` is the average over the
  1 s before the change (`null` without samples). `work_ms.avg` is the mean CPU + GPU time
  of the frames in the window that were not held (section 8); `null` when every frame was held.
  `gpu_timing` is `"timestamp"` or
  `"unavailable"` (then `gpu_ms` is `null`).
- `tier`: added by `tier.set`, batches containing it, and undo/redo (6.11).
- `capture`: `null` unless the command asked for one with `opts.capture` (`{ "width": 320 }`, or
  `true` for 480). Then, when the deferred reply is due, the next frame is captured and
  `capture` is `{ png_base64, width, height, frame, sim_time, stats }` as in 6.14 (or
  `{ "error": "…" }` if the renderer failed). The MCP change tools set it with `capture_width`.
- `watch`: the auto-revert guard for this change (6.24):
  `{ auto_revert: "armed", target_ms, limit_ms, sustain_s, window_s }`, or `"off"` with a
  `reason`: `"baseline_over"` (work time was already over the limit before the change, so it
  isn't blamed) or `"history"` (undo/redo are never watched).

### 5.5 Error codes

| Code | Meaning |
|---|---|
| `bad_request` | Malformed JSON, missing field, wrong envelope. |
| `version_mismatch` | Unsupported `v`. |
| `unauthorized` | Missing or wrong token. |
| `forbidden_origin` | Origin/Host not allowed. |
| `unknown_command` | No such `cmd`. |
| `unknown_target` | No such parameter, slot, curve, tier or snapshot (`data.did_you_mean`). |
| `not_found` | No such telemetry subscription (`telemetry.unsubscribe`). |
| `limit` | A per-session limit was hit (e.g. 16 telemetry subscriptions). |
| `invalid_value` | Wrong type, bad enum, non-finite number, curve points out of order. |
| `out_of_range` | An edit's value (`edit.stage`, 6.31) is outside the parameter's range. `data` = `{name, requested, nearest, min, max}`. Stage it again within range or with `clamp: true`. (`param.set` and `batch` outside an edit clamp instead.) |
| `not_compiled_in` | The effect or module isn't in this build (it never appears in `schema.get`). |
| `note_required` | Mutating command without `note`. |
| `compile_error` | Slot WGSL or expression didn't compile. `data` = the compile result; the old one keeps running. |
| `needs_xr_session` | `tier.set` to a stereo (VR) preset outside an immersive WebXR session; Enter VR first. `data.selectable` lists the tiers that can be picked now. |
| `busy` | A bench or A/B run is in progress (`data.until_ms` = estimated remaining ms, `data.run`). |
| `no_engine` | The bridge has no page connected. |
| `paused` | The page is connected but not running (Evani.App paused or hid the tab and the page stopped reporting frames, 3.5). Answered at once instead of waiting out the limit. `data` = `{reason, fix, page}`; `message` starts with the reason, e.g. `paused (tab hidden): …`. Show the Evani.dev tab, then try again. |
| `device_lost` | The GPU device was lost during the command; the change was rolled back. |
| `history_empty` | Nothing to undo/redo. |
| `not_focused` | `focus.set` without `asset` while nothing is focused (`data.available` lists the asset ids). |
| `timeout` | The page didn't answer within the command's limit (10 s; bench/compare: their length + 30 s; the geometry view's `debug.geometry` on and `capture.frame` with `geometry: true`: 60 s + 30 s, as the page analyses the draw list first), or a slot compile didn't finish in 8 s (`data.kept: "previous"`). |
| `io_error` | Bake couldn't read or write a file, or an existing file isn't valid JSON (`data.path`, relative to the engine root). |
| `internal` | Bug. `message` says what. |

---

## 6. Commands

Names below are the `cmd` value. "Mutating" commands return the change result (5.4) and need a
`note`. Examples show only `params` (request) and `result` (response); the envelope is 3.3.

Naming: parameter names are dotted, owner first: `fx.bloom.intensity`, `fx.vignette.radius`,
`look.basic.exposure`, `kit.example.spin_speed`, `tier.medium.render_scale`,
`aq.target_ms` (adaptive quality), `budget.*`, `live.revert.*`. Slots, curves and effects use
short names (`tonemap`, `lod.distance`, `bloom`).

### 6.1 `schema.get`

Everything that can be read or changed, with types, ranges and descriptions.

```json
{ "params": { "include": ["params", "slots", "curves", "effects", "tiers", "policy"] } }
```

`include` is optional (default all).

```json
{
  "protocol": "evani-live/1",
  "schema_version": "c3f1a9e2",
  "game": "sandbox",
  "build": { "modules": ["fx", "live"], "effects": ["bloom", "vignette"], "look": "evani-look-basic", "kit": "evani-kit-example" },
  "params": [
    { "name": "fx.bloom.intensity", "type": "f32", "min": 0.0, "max": 4.0, "step": 0.01, "default": 0.6, "value": 0.6,
      "unit": "", "desc": "How strongly the blurred bright areas are added back.", "owner": "fx.bloom", "gpu": true, "bake": "look" },
    { "name": "fx.bloom.threshold", "type": "f32", "min": 0.0, "max": 10.0, "default": 1.0, "value": 1.0, "unit": "luminance", "desc": "Brightness above which pixels bloom.", "owner": "fx.bloom", "gpu": true, "bake": "look" },
    { "name": "look.basic.tint", "type": "color", "default": [1.0, 0.97, 0.92], "value": [1.0, 0.97, 0.92], "unit": "linear rgb", "desc": "Overall colour tint.", "owner": "look.basic", "gpu": true, "bake": "look" },
    { "name": "tier.medium.render_scale", "type": "f32", "min": 0.5, "max": 1.0, "default": 1.0, "value": 1.0, "unit": "x", "desc": "Internal resolution scale at Medium.", "owner": "tier.medium", "gpu": false, "bake": "tier" },
    { "name": "aq.hysteresis_pct", "type": "f32", "min": 0, "max": 50, "default": 10, "value": 10, "unit": "%", "desc": "How far frame time must recover before quality steps back up.", "owner": "aq", "gpu": false, "bake": "policy" },
    { "name": "fx.bloom.mips", "type": "i32", "min": 2, "max": 7, "default": 5, "value": 5, "unit": "levels", "desc": "Blur levels.", "owner": "fx.bloom", "gpu": false, "bake": "look" },
    { "name": "look.basic.fog_mode", "type": "enum", "options": ["off", "linear", "exp2"], "default": "exp2", "value": "exp2", "desc": "Fog model.", "owner": "look.basic", "gpu": true, "bake": "look" }
  ],
  "slots": [
    { "name": "tonemap", "lang": "wgsl", "signature": "fn tonemap(color: vec3<f32>) -> vec3<f32>", "owner": "look.basic",
      "inputs": ["color"], "uniforms": ["look.basic.exposure", "look.basic.tint"], "desc": "HDR to display colour.", "hash": "9a1c…", "default_hash": "9a1c…" },
    { "name": "bloom_weight", "lang": "wgsl", "signature": "fn bloom_weight(mip: u32) -> f32", "owner": "fx.bloom", "desc": "Weight of each blur level.", "hash": "…" },
    { "name": "camera.smoothing", "lang": "expr", "signature": "(dt, speed, distance) -> f32", "vars": ["dt", "speed", "distance"], "owner": "kit.example", "desc": "Camera follow factor per step.", "hash": "…" }
  ],
  "curves": [
    { "name": "lod.distance", "x": { "name": "screen_coverage", "min": 0, "max": 1 }, "y": { "name": "lod", "min": 0, "max": 4 },
      "points": [[0.0, 4.0], [0.05, 2.0], [0.2, 1.0], [0.5, 0.0]], "max_points": 16, "owner": "render", "desc": "Mesh LOD by screen coverage." }
  ],
  "effects": [
    { "name": "bloom", "on": true, "cost_points": 3, "value_weight": 0.8, "styles": ["3d", "pseudo3d"], "vr_comfort_off": false, "passes": ["bloom.bright", "bloom.blur", "bloom.composite"] },
    { "name": "vignette", "on": true, "cost_points": 1, "value_weight": 0.3, "styles": ["2d", "pseudo3d", "3d"], "vr_comfort_off": true, "passes": ["vignette"] }
  ],
  "tiers": { "active": "medium", "available": ["lite", "low", "medium", "high", "ultra", "vr"], "points": { "lite": 5, "low": 7, "medium": 11, "high": 16, "ultra": 24, "vr": 9 } },
  "policy": { "params": ["aq.target_ms", "aq.window_ms", "aq.hysteresis_pct", "aq.degrade_order", "aq.render_scale_step", "aq.render_scale_floor"], "weights": ["fx.bloom.value_weight", "fx.vignette.value_weight"] }
}
```

Types: `f32`, `i32`, `bool`, `vec2`, `vec3`, `vec4` (arrays), `color` (linear RGB array), `enum`
(string from `options`). `bake` says where `bake.export` writes it: `look`, `kit`, `tier`,
`policy`, or `none` (live-only, e.g. `live.revert.*`).

### 6.2 `param.get`

```json
{ "params": { "names": ["fx.bloom.intensity", "fx.bloom.threshold"] } }
```
or `{ "prefix": "fx.bloom." }`, or `{}` for all.

```json
{ "values": { "fx.bloom.intensity": 0.6, "fx.bloom.threshold": 1.0 } }
```

### 6.3 `param.set` (mutating, batched, atomic, clamped)

```json
{ "params": { "values": { "fx.bloom.intensity": 0.9, "fx.bloom.threshold": 12.0 } },
  "note": "Stronger, but only on the sky and lamps" }
```

Result: the change result. Here `fx.bloom.threshold` comes back
`{ "old": 1.0, "new": 10.0, "requested": 12.0, "clamped": true }`. Values apply on the next frame;
GPU-side values are uniform-buffer writes, never shader rebuilds.

### 6.4 `slot.get`

```json
{ "params": { "name": "tonemap" } }
```

```json
{ "name": "tonemap", "lang": "wgsl", "signature": "fn tonemap(color: vec3<f32>) -> vec3<f32>",
  "source": "let x = color * uniforms.exposure;\nreturn x / (1.0 + x);",
  "default_source": "…", "hash": "9a1c…", "is_default": false, "owner": "look.basic" }
```

`source` is the **body only** (between the braces). The signature is fixed; `{"with_prelude": true}`
adds `prelude`, the first line of the wrapper the body is spliced into. Expression slots answer
the same shape with `"lang": "expr"`, an `(vars) -> f32` signature and `vars`. A name that is a
curve answers `unknown_target` (use `curve.get`).

### 6.5 `slot.set` (mutating)

```json
{ "params": { "name": "tonemap",
  "source": "let x = color * uniforms.exposure;\nlet a = 2.51; let b = 0.03; let c = 2.43; let d = 0.59; let e = 0.14;\nreturn clamp((x*(a*x+b))/(x*(c*x+d)+e), vec3(0.0), vec3(1.0));" },
  "note": "ACES-style filmic curve for more contrast in highlights" }
```

For `lang: "expr"` slots, `source` is one expression in the built-in evaluator: numbers,
`+ - * / ^`, parentheses, `min max clamp mix smoothstep pow abs floor sqrt exp`, the slot's named
variables and any parameter by name. No loops, no assignments, no calls to anything else.

What happens (WGSL): the page checks the body's structure (not empty, length limit, brackets balance
and never close the enclosing function; a failure here is reported at line 0, with the line
in the message), then queues a rebuild request on the core's
`SlotRebuilds` and parks the command. The renderer wraps the body in the fixed signature,
validates it inside a GPU error scope (and `getCompilationInfo`), builds the new pipelines
asynchronously and swaps them in at a frame boundary, then completes the request. The command is
answered when every request in it has completed:

- If no renderer takes the requests within **3 frames** (a headless or renderer-less build), the
  page completes them itself and the result says `"checked": "structural"` (only the structural
  check ran).
- If they haven't completed after **8 s**, the change is dropped with `timeout`
  (`data.kept: "previous"`).
- If anything fails, any body already swapped in is put back, the old pipeline keeps running and
  nothing is recorded in history.

Expression slots compile on the CPU immediately (no parking).

Success: the change result, with

```json
"compile": { "ok": true, "ms": 38, "messages": [], "pipelines_rebuilt": ["post.tonemap"] }
```

(plus `"checked": "structural"` in the no-renderer case above; an unchanged body answers
`ok: true, ms: 0` without a rebuild).

Failure: `ok: false`, error `compile_error`, `data` =

```json
{ "ok": false, "ms": 12, "kept": "previous",
  "messages": [
    { "severity": "error", "line": 2, "col": 14, "len": 3, "message": "unresolved identifier 'aa'",
      "source_line": "let a = 2.51; let b = aa;" }
  ] }
```

`kept` is always `"previous"`. `line`/`col` are 1-based **within the submitted body** (the page
subtracts the prelude's lines). Messages that point into the prelude, and structural-check
failures, are reported at line 0 (`source_line: null` for the latter). Inside a `batch`, `data.index`
names the failing part. The
page MUST NOT `console.error` a rejected patch and MUST keep it inside an error scope (Evani.App's
build checker treats console WGSL errors and uncaptured GPU errors as a broken build).

### 6.6 `curve.get`

```json
{ "params": { "name": "lod.distance" } }
```
```json
{ "name": "lod.distance", "points": [[0.0, 4.0], [0.05, 2.0], [0.2, 1.0], [0.5, 0.0]], "x": { "min": 0, "max": 1 }, "y": { "min": 0, "max": 4 }, "max_points": 16 }
```

`min`/`max` are `null` for an axis with no range (`Formulas::set_ranges` sets them). A name that
is an expression slot answers `unknown_target`.

### 6.7 `curve.set` (mutating)

```json
{ "params": { "curves": { "lod.distance": [[0.0, 4.0], [0.08, 2.0], [0.25, 1.0], [0.6, 0.0]] } },
  "note": "Keep high detail a little longer on mid-distance karts" }
```

Points: 2 to `max_points`, x strictly increasing, both axes clamped to their ranges (reported as
`clamped`; two points landing on the same x after clamping is `invalid_value`). Piecewise-linear;
outside the first/last x the end values hold. Several curves in one call are one transaction.
`applied` entries: `{ "kind": "curve", name, old, new, requested, clamped }`.

### 6.8 `policy.get`

```json
{ "params": {} }
```
```json
{ "enabled": true, "target_ms": 16.7, "window_ms": 500, "degrade_pct": 10, "hysteresis_pct": 10,
  "degrade_frames": 30, "upgrade_frames": 120, "cooldown_ms": 1000,
  "degrade_order": "value_per_point",
  "render_scale_step": 0.1, "render_scale_floor": 0.6,
  "weights": { "bloom": 0.8, "vignette": 0.3 } }
```

(Numbers illustrative.) One field per registered `aq.*` param (the `aq.` prefix dropped; `App::new`
registers them for every game) plus `weights` from every `fx.<effect>.value_weight`.
`degrade_order` is `"value_per_point"` (lowest value per point goes first) or `"scale_first"`;
explicit effect lists are not supported yet. `target_ms` is `aq.target_ms`, or the active tier's
frame budget when that is 0 (its default).

### 6.9 `policy.set` (mutating)

```json
{ "params": { "set": { "hysteresis_pct": 15, "weights": { "vignette": 0.1 } } },
  "note": "Drop vignette before bloom when frames get slow" }
```

A partial patch; same atomic and clamping rules as `param.set`. Field `x` maps to param `aq.x`;
`weights.<effect>` maps to `fx.<effect>.value_weight`. Each `applied` entry is
`{ "kind": "policy", "name": "aq.hysteresis_pct", "field": "hysteresis_pct", old, new, requested, clamped }`
(`"field": "weights.vignette"` for a weight). An unknown field is `unknown_target`; a list for
`degrade_order` is `invalid_value` with `data.options`.

### 6.10 `effect.toggle` (mutating)

```json
{ "params": { "effect": "bloom", "on": false, "scope": "session" }, "note": "Compare without bloom" }
```

- `scope`: `"session"` (default: on/off now, like the overlay toggle) or `"tier"` (change which
  effects the **active tier's preset** starts with; this is what bake writes). Scope `tier` edits
  the preset's `effects_on` **and** sets the session switch, in one transaction.
- `applied[0]` = `{ "kind": "effect", "name": "bloom", "scope": "session", "old": true, "new": false }`.
- Turning an effect on reruns the budget calculator on the next frame. If it doesn't fit, the
  lowest value-per-point effects are switched off (listed in `budget.switched_off`); if the effect
  itself is the one that doesn't fit, it stays off and its entry gets `reason: "over_budget"`
  (read from the effects status when the deferred reply is sent). `override_budget` is not
  implemented yet.
- An effect not compiled in → `not_compiled_in`. In VR, comfort-off effects can't be turned on
  (`invalid_value`, reason `vr_comfort`).

### 6.11 `tier.set` (mutating)

```json
{ "params": { "tier": "low" }, "note": "Check how it looks on a weak laptop" }
```

Switches without reload (pipelines and targets rebuilt). Result adds
`"tier": { "active": "low", "render_scale": 0.75, "dpr_cap": 1.25, "points_limit": 7 }` and the
budget's switched effects. The VR preset can only be picked inside an immersive WebXR session;
outside one the command fails with `needs_xr_session`:

```json
{ "code": "needs_xr_session", "message": "Tier vr runs only inside an immersive WebXR session (Enter VR)",
  "data": { "selectable": ["lite", "low", "medium", "high", "ultra"] } }
```

History records it; bake does not (the active tier is a runtime
choice; tier *presets* are baked through `tier.*` params and `effect.toggle` with `scope: "tier"`).

The effects module resets effect switches to the new tier's defaults on the frame after the
switch. The page folds those resets into the same transaction (explicit switches in the same
batch win), so undoing the tier change restores the switches exactly; undo, redo and auto-revert
re-assert the history's switch values after a tier move the same way.

### 6.12 `telemetry.subscribe`

```json
{ "params": { "rate_hz": 4, "fields": ["frame_ms", "gpu_ms", "draw_calls", "triangles", "sprites", "budget"] } }
```

`rate_hz` 0.5–60 (default 2). `fields` optional (default all): `frame_ms`, `cpu_ms`, `work_ms`, `gpu_ms`,
`draw_calls`, `triangles`, `sprites`, `budget`, `render_scale` (an unknown name is
`invalid_value` with `data.fields`). `t`, `frame`, `tier` and `fps` are always sent; `dpr` comes
with `render_scale` and is always `null` for now.

Limits: the page allows at most 16 subscriptions per session (it can't see connections; over that
is `limit`). Subscriptions belong to the connection: the bridge records which connection owns
each one and unsubscribes them when it closes. The bridge should also cap 4 per connection (not
enforced yet). Samples come from a preallocated 1024-frame ring (about 17 s at 60 fps).

```json
{ "sub": "t-1", "rate_hz": 4 }
```

Pushed events, one per period, averaged over that period:

```json
{ "type": "event", "v": 1, "event": "telemetry", "sub": "t-1",
  "data": { "t": 152.40, "frame": 9150, "tier": "medium", "fps": 60.0,
            "frame_ms": { "avg": 16.6, "p95": 17.1, "max": 18.0 }, "cpu_ms": { "avg": 3.1 }, "work_ms": { "avg": 8.3 },
            "gpu_ms": { "avg_total": 5.2, "passes": { "forward": 3.1, "bloom.bright": 0.3, "bloom.blur": 0.8, "vignette": 0.1, "tonemap": 0.2 } },
            "draw_calls": 122, "triangles": 184000, "sprites": 3200,
            "budget": { "used": 9, "limit": 11 }, "render_scale": 1.0, "dpr": null } }
```

### 6.13 `telemetry.unsubscribe`

```json
{ "params": { "sub": "t-1" } }
```
```json
{ "sub": "t-1", "stopped": true }
```

An unknown `sub` is `not_found`.

MCP has no push to the model, so the MCP server offers `telemetry_sample` instead (section 11).

### 6.14 `capture.frame`

Built in stage 6, with `bench.run` (6.15) and `compare.ab` (6.16). All three are live-only: the
renderer readback is behind the render `capture` feature and the clock hold behind the core
`capture` feature, which only a game's `live` feature turns on.

#### Renderer interface

The renderer side lives in `evani_core::capture` (`crates/evani-core/src/capture.rs`) as plain
data; nothing there runs unless its resource is present.

- `DeviceStatus { lost_count: u32 }`: inserted by `App::new` for every game. The renderer adds 1
  whenever the GPU device is lost (and after each failed recovery). Auto-revert watches it (6.24).
- `CaptureQueue`: inserted **only by evani-live**, so a build without live never queues a
  capture. Methods: `request(width, CaptureView, hide_overlay) -> id`, `has_pending()`,
  `take_pending() -> Vec<CaptureRequest>`, `complete(CaptureResult)`, `take_result(id)`. Fields:
  `camera_override: Option<CameraPose>`, `sim_time_override: Option<f64>`,
  `clear_override: Option<[f32; 3]>` (clear the forward pass to this linear RGB instead of the sky
  colour; fog keeps the real sky colour) and `jitter_px: Option<[f32; 2]>` (shift the projection
  by this many pixels, x right, y down). `overriding()` is true while any of the four is set.
- `HoldCanvas(bool)`: inserted only by evani-live; keep the canvas on its last presented frame
  while trials run (6.30). Its own resource rather than a `CaptureQueue` field, so shipped games
  (which read `CaptureQueue`) compile exactly as before.
- `CaptureView::Current` (the next frame as rendered) or
  `CaptureView::Path { path: CameraPath, sim_time }` (the camera on `path` at `sim_time`).
- `CaptureResult { id, rgba: Result<Vec<u8>, String>, width, height, frame, sim_time, stats: RenderStats }`:
  `rgba` is the full render size, RGBA8, sRGB.
- `BenchPath(CameraPath)`: the deterministic bench camera, inserted by the game (the sandbox
  inserts its own camera path under its `live` feature). Without one, live flies a default orbit.

What the renderer does once per frame: honour `camera_override`, `clear_override` and `jitter_px`; `take_pending()` when the
frame's surface texture is acquired; after the graph walk copy the LDR colour target into a
mappable buffer (rows padded to 256 bytes), map it asynchronously after submit (no stall), and on
a later frame unpad the rows and `complete(...)` with the pixels and that frame's `RenderStats`.
On device loss every pending readback completes with `rgba: Err(reason)`. `sim_time_override`
is honoured by the core loop: while it is set no fixed ticks run and `Time::render_seconds`
returns it, so the scene renders as at that sim time and the sim resumes where it paused. The
overlay is a DOM element, never in the colour target, so `hide_overlay` needs no work.

Timing (2026-10-25, frame order fix): the renderer draws after every other module's frame step
(README "Frame order"), live included. A capture live queues in its frame step is taken by the
renderer in that same frame, drawn with every value set that frame (the overrides live set two
frames earlier, `CAPTURE_DELAY_FRAMES`, and the nature pass's camera copy), so a shot is never a
mix of this frame's game camera and last frame's module values. Values applied by `param.set`,
`sweep.run` or `compare.ab` reach the picture the frame they are applied; `HoldCanvas` changes
in the same frame as the values it hides, as before. Telemetry samples taken in live's frame step
carry the stats of the previous draw; since applying and drawing moved together, the first
sample that shows a new value is the same one as before.

evani-live downscales the pixels (area average) to the asked width and PNG-encodes them with its
own small encoder; no encoder ships in a game.

#### Request and result

```json
{ "params": { "width": 480, "sim_time": 12.5, "camera": "path", "hide_overlay": true } }
```

- `width` 64–4096 (default 480); height keeps the aspect ratio. A shot is never wider than the
  canvas, so a large width gives the full render (geocheck reads full-resolution pixels, and a
  pixel-art game captured at its own width is exact).
- `sim_time` (optional, seconds ≥ 0): hold the sim clock at that time, capture, then resume where
  it was. Without it, the next frame as it is.
- `camera`: `"current"` (default), `"path"` (the bench camera at `sim_time`, or at the current
  time if none is given), or a fixed camera `{ "eye": [x, y, z], "target": [x, y, z], "fov_y_deg": 50 }`
  in metres (`fov_y_deg` 5–150, default 50; optional `up`, default `[0, 1, 0]`). A fixed camera
  also holds the clock, at `sim_time` or at the current time.
- `sky` (optional): `true` clears the frame to magenta `[1, 0, 1]` for this shot, or give a linear
  RGB colour `[r, g, b]` (0–1). Holes and cracks in the geometry then show in that colour. Fog
  keeps the game's sky colour.
- `jitter_px` (optional, `[x, y]`, each within ±4): shift the view by that many pixels (sub-pixel
  allowed). Render the same view twice with different jitter to find flickering (z-fighting)
  pixels.
- `geometry` (optional, bool): show the geometry debug view (6.27) in this shot; it is switched
  back off afterwards if it was off.
- `hide_overlay`: accepted; the overlay is never in the capture.
- `moment` (optional, a name from `session.status` `moments`): a scripted capture moment the game
  registered in its `CaptureMoments` resource (`evani_core::capture`, inserted only in builds
  with live; a kit adds `(name, label, setup)` with `CaptureMoments::add`). The page runs the
  moment's `setup(&World)` (it stages the game state, e.g. a fresh race stepped with a fixed dt
  until a drift holds), holds the clock where the setup left the sim, waits 4 frames so the
  game's own camera frames it, then captures. Not with `sim_time` or `camera: "path"`
  (`invalid_value`); an unknown name is `unknown_target` (its `data.did_you_mean` lists the names); a
  setup that returns an error is `internal` with its message.
- `hold_after` (optional, bool, default false; only with a `moment`, `sim_time` or a path or
  fixed camera, else `invalid_value`): leave the clock (and camera) held where the shot was
  taken after the reply, so a page screenshot taken next shows exactly the captured frame
  (sparks, animation and HUD included) however long it takes. The next `capture.frame`,
  `bench.run`, `compare.ab` or `sweep.run` puts back what was there before (another
  `hold_after` capture keeps holding). The review's capture script uses it for every moment's
  page and showcase shots; nothing else does, so the player's game never stays held.

The page sets the hold and camera, waits 2 frames, requests a `Current` capture and replies when
the pixels arrive (a few frames). No other command is taken from the inbox meanwhile. A capture
the renderer doesn't deliver in 5 s fails with `timeout`; a renderer error is `internal`. A
moment capture's result also has `"moment": { "name": "drift", "label": "A drift through a bend…" }`.
With `sky`, `jitter_px` or `geometry` the page also waits 2 frames, and the result echoes `sky`
and `jitter_px`; with `geometry` it has `"geometry": { "counts": {...}, "total": 12, "frame": 9320 }`.
Every override is given back when the capture ends, also on failure (with `hold_after`, when the
next run ends; a failed `hold_after` capture gives them back at once).

```json
{ "run": "c-1", "png_base64": "iVBORw0KGgoAAAANSUhEUgAA…", "width": 480, "height": 270, "frame": 9321, "sim_time": 12.5,
  "stats": { "frame_ms": 16.4, "gpu_ms": { "avg_total": 5.2, "passes": { "forward": 3.1 } }, "draw_calls": 122, "triangles": 184000, "sprites": 3200, "tier": "medium" } }
```

`stats` are those of the captured frame (`gpu_ms` is `null` without timestamp queries);
`frame_ms` is the latest telemetry sample.

### 6.15 `bench.run`

```json
{ "params": { "seconds": 10, "tier": "medium", "warmup_s": 1, "capture_at": [2.0, 8.0], "capture_width": 320 } }
```

Flies `BenchPath` from sim time 0 for `seconds` (1–120, default 10) on `tier` (default: active;
adaptive quality is off for the run). First any `capture_at` frames (at most 8, each within
`0..=seconds`; `capture_width` 64–1024, default 320) are captured at their path time, then
`warmup_s` (0–60, default 1) holds t = 0, then frame k shows t = k/60 on the path with the sim
clock held there, so every run sees exactly the same frames; one telemetry sample is kept per
frame (the first is skipped). The wall time is capped at 3 × `seconds` + 5 s (`truncated: true`
when hit). Live changes are refused with `busy` meanwhile (5.2), and afterwards the state, camera
and clock are exactly what they were. Progress events about once a second:
`{"event":"bench.progress","data":{"run":"b-3","t":4.0,"of":10}}`.

```json
{ "run": "b-3", "tier": "medium", "seconds": 10, "warmup_s": 1, "frames": 600, "truncated": false,
  "frame_ms": { "avg": 16.6, "p50": 16.6, "p95": 17.4, "p99": 18.8, "max": 22.1 },
  "cpu_ms": { "avg": 3.1 },
  "gpu_ms": { "avg_total": 5.3, "passes": { "forward": { "avg": 3.1, "p95": 3.4 }, "bloom.blur": { "avg": 0.8, "p95": 0.9 } } },
  "gpu_timing": "timestamp",
  "draw_calls": { "max": 131 }, "triangles": { "max": 191000 }, "sprites": { "max": 3200 },
  "budget": { "used": 9, "limit": 11 },
  "pass": true, "failures": [],
  "captures": [ { "t": 2.0, "png_base64": "…", "width": 320, "height": 180, "frame": 9400 } ] }
```

Percentiles are nearest-rank. `gpu_ms` is `null` and `gpu_timing` `"unavailable"` without
timestamp queries. `pass` uses the same rules as apps/bench (copied, not shared): average frame
time within the tier's frame target + 10 %, p99 under the tier's fps floor, and draw calls,
triangles, sprites and budget points within the tier's limits; `failures` lists each broken
limit as `{ "limit": "draw_calls", "max": 150, "seen": 212 }`.

### 6.16 `compare.ab`

```json
{ "params": { "a": "current", "b": { "values": { "fx.bloom.enabled": false } }, "seconds": 4,
              "warmup_s": 0.5, "tier": "medium", "order": "ABBA", "capture_at": [2.0],
              "capture_width": 320, "keep": false } }
```

- `a` (default `"current"`) and `b` (required): `"current"`, a snapshot id or name (6.17), or
  changes to the current state: `{ "values": { … } }` (as `param.set`) or `{ "cmds": [ … ] }`
  (mutating commands, planned like `batch` 6.25). Changes are checked before anything runs; a
  bad one fails the command with its usual error and `data.side`.
- Both sides fly the same path with the same held sim times, as `bench.run` (`seconds` default 4,
  `warmup_s` default 0.5 before every run, `tier`, `capture_width`). `order` `"ABBA"` (default)
  or `"AB"` interleaves runs to cancel warm-up and thermal drift; samples of a side's runs are
  pooled. `capture_at` defaults to `[seconds / 2]`, so a compare always returns a pair of images;
  each side is captured on its first visit.
- With `tier`, effect switches the tier change would reset are left as the tier sets them unless
  a side changed them itself.
- Afterwards the session is exactly as before. With `keep: true` (needs a `note`) it ends on B
  instead (the real B: no tier override, adaptive quality as it was), recorded as one undoable
  transaction with `cmd: "compare.ab"`.
- `busy` rules as bench; progress events carry `side: "a" | "b"`.
- Both sides run off-screen (6.30): the visible game holds its last frame until the run ends,
  then shows the end state at once (B with `keep`, else the unchanged start state).

```json
{ "run": "ab-2", "tier": "ultra", "seconds": 2, "warmup_s": 0.5, "order": "AB", "truncated": false,
  "gpu_timing": "timestamp",
  "a": { "snapshot": "current", "frames": 120, "frame_ms": { "avg": 16.67, "p50": 16.7, "p95": 16.8, "p99": 16.9, "max": 17.3 },
         "cpu_ms": { "avg": 0.7 }, "gpu_ms": { "avg_total": 1.38, "passes": { "forward": 0.55, "bloom.composite": 0.23 } },
         "draw_calls": { "max": 17 }, "triangles": { "max": 156002 }, "sprites": { "max": 300 } },
  "b": { "snapshot": null, "changes": [ { "kind": "effect", "name": "fx.bloom.enabled", "old": true, "new": false, "requested": false, "clamped": false } ],
         "frames": 120, "frame_ms": { "avg": 16.67, "p95": 16.8 }, "gpu_ms": { "avg_total": 1.07, "passes": { "forward": 0.62 } } },
  "delta": { "frame_ms_avg": 0.0, "frame_ms_p95": 0.0, "gpu_ms_total": -0.31, "passes": { "bloom.composite": -0.23, "forward": 0.07 } },
  "captures": [ { "t": 1.0, "a_png_base64": "…", "b_png_base64": "…", "width": 480, "height": 270,
                  "diff": { "mean_abs": 19.89, "rmse": 31.311, "max": 213, "changed_pct": 64.15, "psnr_db": 18.22 } } ],
  "diff": { "mean_abs": 19.89, "rmse": 31.311, "max": 213, "changed_pct": 64.15, "psnr_db": 18.22 },
  "kept": null }
```

(A real result from the sandbox, shortened: bloom on vs off at 480 wide.)

- A side's `snapshot` is its id/name or `"current"`; a changes side has `snapshot: null` and
  `changes` (the planned entries, as in 5.4 `applied`).
- `delta` is B − A. Per-pass GPU ms in a side are averages; `gpu_ms` is `null` without timestamp
  queries.
- `diff` (per pair and over all pairs together): on the RGB pixels, 0–255 per channel:
  `mean_abs` (mean absolute difference), `rmse`, `max`, `changed_pct` (pixels where any channel
  differs by more than 8) and `psnr_db` (`null` when the images are identical).
- `kept`: `{ "tx": 18, "applied": [ … ] }` with `keep: true`, else `null` (also `null` when B
  equals the start state).

### 6.17 `snapshot.save`

```json
{ "params": { "name": "warm sunset" }, "note": "Before trying a cooler grade" }
```
```json
{ "snapshot": "snap-4", "name": "warm sunset", "tx": 17, "at": "2026-10-13T14:31:02Z",
  "summary": { "params_changed": 6, "slots_changed": ["tonemap"], "curves_changed": [], "effects": { "bloom": true, "vignette": false }, "tier": "medium" } }
```

A snapshot is the full live state: every parameter value, slot source, curve, policy, effect
on/off and tier-preset effect set, and the active tier. Snapshots live in the page for the
session (their list, not their values, is mirrored to the bridge, section 10.3). `snapshot.save`
is not mutating (no transaction; `note` optional and kept). `name` defaults to the id. `tx` is the
history head. `summary` counts changes **since the session started** (not since the previous
snapshot): `params_changed` is a count, `slots_changed` / `curves_changed` are names, `effects` is
each effect's current switch.

### 6.18 `snapshot.list`

```json
{ "params": {} }
```
```json
{ "snapshots": [ { "snapshot": "snap-4", "name": "warm sunset", "tx": 17, "at": "2026-10-13T14:31:02Z", "note": "Before trying a cooler grade" } ] }
```

### 6.19 `snapshot.restore` (mutating)

```json
{ "params": { "snapshot": "snap-4" }, "note": "Back to the warm sunset" }
```

`snapshot` is an id or a name. One transaction (undoable): the page diffs the current state
against the snapshot and applies every difference at once; `applied` lists each (in the shapes
of 5.4). An unknown snapshot is `unknown_target` with `data.did_you_mean` (the ids).

### 6.20 `history.undo` / 6.21 `history.redo` (mutating, note optional)

```json
{ "params": { "steps": 1 } }
```

`steps` 1–50 (default 1). Reverts (or reapplies) whole transactions, stopping early when history
runs out. History is linear: a new change after an undo drops the redo tail. Result: the change
result, plus `"history": { "head": 16, "can_undo": true, "can_redo": true }` and `tier`. Its
`tx` is the newest transaction touched (for undo, the first one undone; for redo, the last one
redone), not a new transaction: undo/redo add no history entries. They are never watched
(`watch: { "auto_revert": "off", "reason": "history" }`) and clear any current watch. Nothing to
undo/redo → `history_empty`. Up to 500 transactions are kept per session (the oldest drop off).

### 6.22 `history.list`

```json
{ "params": { "limit": 20 } }
```
```json
{ "head": 17, "can_undo": true, "can_redo": false,
  "entries": [
    { "tx": 17, "at": "2026-10-13T14:30:40Z", "cmd": "slot.set", "source": "mcp", "note": "ACES-style filmic curve for more contrast in highlights", "summary": "tonemap slot rewritten", "state": "applied" },
    { "tx": 16, "at": "2026-10-13T14:29:12Z", "cmd": "param.set", "source": "page", "note": "Debug panel: Bloom intensity", "summary": "fx.bloom.intensity 0.6 → 0.9", "state": "applied" },
    { "tx": 15, "at": "2026-10-13T14:28:55Z", "cmd": "param.set", "source": "mcp", "note": "Try 4 blur levels", "summary": "fx.bloom.mips 5 → 7", "state": "auto_reverted", "revert_reason": "frame_time" }
  ] }
```

`state`: `applied | undone | auto_reverted`. Each entry also has `frame`. `limit` 1–500
(default 20), newest first. `source` is the envelope's `source` (`mcp`, `ctl`, `page`; default
`page`). Auto-reverts appear as their own entries with `cmd: "auto_revert"` (6.24).

### 6.23 `bake.export`

Writes the session into source files. The page can't write files, so it goes
**page → bridge → disk**:

1. The page answers `bake.export` with `{ "bundle": … }`: every value that differs from the
   session's start state and was touched by an applied transaction, with its bake target, plus
   the CHANGES.md note lines. (Called in-page, e.g. `window.evani.live.call("bake.export")`,
   you get this bundle and nothing is written.)
2. The bridge, seeing an OK `bake.export` result, runs the native planner
   `evani_live::bake::plan(bundle, read, now)`: a pure function (the caller reads and writes) that
   merges the bundle into the files as they are on disk. The same planner is used by the tests.
3. The bridge writes the planned files (unless `dry_run`) and replaces the result with the one
   below. Writes are confined to `looks/evani-look-*/…`, `kits/evani-kit-*/…` and
   `assets/tiers/*.json` (every path part `[a-z0-9_.-]`, no `..`); the bridge also canonicalizes
   each parent folder and checks it is under the engine root, so a symlink can't lead a write
   elsewhere. A failed read or write is `io_error` with `data.path`.

`bake.export` is not a transaction (it changes no live state and needs no note).

```json
{ "params": { "dry_run": false, "include": ["tiers", "params", "slots", "curves", "policy"],
              "title": "Sunset look pass" } }
```

All optional: `dry_run` (default false), `include` (default all five), `title` (default
"Live session"). `dry_run: true` returns the same result without writing, with `preview` (the
full new contents) per file.

```json
{ "written": [
    { "path": "looks/evani-look-basic/params.json", "action": "updated", "bytes": 1840 },
    { "path": "looks/evani-look-basic/slots/tonemap.wgsl", "action": "updated", "bytes": 312 },
    { "path": "kits/evani-kit-example/curves/lod.distance.json", "action": "created", "bytes": 140 },
    { "path": "assets/tiers/medium.json", "action": "updated", "bytes": 610 },
    { "path": "looks/evani-look-basic/CHANGES.md", "action": "updated", "bytes": 1204 }
  ],
  "unchanged": ["assets/tiers/low.json", "assets/tiers/high.json"],
  "changes_md": "looks/evani-look-basic/CHANGES.md",
  "from_tx": 1, "to_tx": 17, "dry_run": false,
  "rebuild": "cargo xtask build sandbox --ship reproduces this look" }
```

Where things go:

- Params by their `bake` field (`Auto` resolves from the owner: `kit.*` → kit, `tier.*` → tier,
  `aq`/`budget` → policy, `live.*` → none, anything else → look): `look` / `kit` → that crate's
  `params.json`, `policy` → the look crate's `policy.json`, `tier` → `assets/tiers/<id>.json`.
  Files are merged: existing keys not in the bundle are kept.
- Tiers: bundle entries `{ "id": "medium", "set": { "<field>": value } }`, built from preset
  `effects_on` edits (`effect.toggle` scope `tier`) and `tier.<id>.<field>` params; merged into
  `assets/tiers/<id>.json`. The JSON stays the source of truth: evani-core's build script compiles
  those files into the shipped tier table and reruns when one changes, so the next build (or a
  `cargo xtask dev --hot` rebuild) picks the baked tier up (README "How data ships").
- WGSL slots always go to the **look** crate (`slots/<name>.wgsl`, written exactly as sent).
- Expression slots (`slots/<name>.expr`, ending with one newline) and curves
  (`curves/<name>.json`, `{ "points": [[x, y], …] }`) go to the crate named by the formula owner's
  prefix (`look.*` → look, `kit.*` → kit; others aren't baked).
- JSON is written with keys sorted, one key per line, arrays of scalars on one line; f32 values in
  their shortest form (`0.9`, not `0.8999999761581421`).
- A file whose contents wouldn't change is listed in `unchanged`, not written. Undone and
  auto-reverted transactions are left out (auto-reverts get a note line).
- `rebuild` is the command that reproduces the look without live: `cargo xtask build <game> --ship`. A game without the needed crate answers `bad_request`.

`CHANGES.md` (in the look crate, or the kit crate when there is no look crate) gets one new
section per bake, newest first, **only when another file changes**. A new file starts with
`# Changes`. Heading times are UTC. A value changed more than once lists every note, in order, joined by `; ` (repeats dropped). Authors: `mcp` → "AI", `page` → "you, debug panel", `ctl` → "you, evani-ctl".

```markdown
## 2026-10-13 14:35 UTC — Sunset look pass (tx 1–17)

- **fx.bloom.intensity** 0.6 → 0.9 — Stronger, but only on the sky and lamps. *(AI)*
- **tonemap** slot rewritten — ACES-style filmic curve for more contrast in highlights. *(AI)*
- **lod.distance** curve: 4 points moved — Keep high detail a little longer on mid-distance karts. *(you, debug panel)*
- Reverted automatically: fx.bloom.mips 5 → 7 (frame_time).
```

### 6.24 Auto-revert (event, no command)

The guard for each change (`watch` in 5.4): if, within `live.revert.window_s` (default 10 s)
after a change, the work time (`work_ms`: CPU + GPU of frames not held, section 8) averaged
over the last 0.5 s stays more than
`live.revert.over_pct` (default 25 %) over the target for `live.revert.sustain_s` (default 3 s),
the change is undone. The target is `aq.target_ms`, or the active tier's frame budget when that
is 0. The 0.5 s average uses every sample in the window, including ones from before the change.
It is also undone if `DeviceStatus.lost_count` goes up after it (6.14); then `observed_ms` is the
current average. Rules:

- **Baseline guard:** a change made when the 1 s work-time average before it was already over the limit is
  not armed (`watch.auto_revert = "off"`, `reason: "baseline_over"`).
- Only one watch at a time: a later change supersedes the watch on an earlier one; undo/redo
  clear it and are never armed (`reason: "history"`).
- The revert applies the **inverse** of the change as a new history entry (`cmd: "auto_revert"`,
  note "Auto-revert of <tx> (<reason>)"), and the original transaction's state becomes
  `auto_reverted` with `revert_reason`. Undoing the revert entry therefore re-applies the change.
- `session.status.watch` shows the armed watch.

Event to every `/ctl` client and the page:

```json
{ "type": "event", "v": 1, "event": "auto_revert",
  "data": { "tx": 17, "reason": "frame_time", "reverted_tx": 18,
            "detail": { "target_ms": 16.7, "limit_ms": 20.9, "observed_ms": 23.6, "over_pct": 41, "sustain_s": 3, "measure": "work_ms" },
            "undone": [ { "kind": "param", "name": "fx.bloom.mips", "restored": 5 } ],
            "note": "Try 7 blur levels" } }
```

`reason`: `frame_time | device_lost` (`frame_time` keeps its name, but since 2026-10-14 the
measure is work time: `detail.measure` is `"work_ms"`, and `observed_ms` / `limit_ms` are work
milliseconds). A 60 Hz display caps the frame interval at 16.7 ms however light the frame is, and
a slow frame interval can come from the browser or another tab; work time is what a change costs. `undone[].restored` is the old value (`old_hash` for slots).
The revert is itself a history entry (`reverted_tx`), so it can be undone to bring the change
back deliberately. The tab gets
`{"type":"live_status","state":"reverted","kind":"live","message":"…"}` on `/evani-dev` (3.4), the
debug panel shows a toast, and MCP returns it with the next tool result (section 11).

### 6.25 `batch` (mutating)

```json
{ "params": { "cmds": [
    { "cmd": "effect.toggle", "params": { "effect": "vignette", "on": false } },
    { "cmd": "param.set", "params": { "values": { "fx.bloom.intensity": 1.1 } } },
    { "cmd": "slot.set", "params": { "name": "tonemap", "source": "…" } } ] },
  "note": "Cleaner frame: no vignette, more bloom, filmic tonemap" }
```

One transaction, one note, one change result covering all parts. Only mutating commands are
allowed inside. Any failure (including a compile error) applies nothing; the error's `data.index`
says which part failed.

### 6.26 `session.status`

```json
{ "params": {} }
```
```json
{ "engine_connected": true, "game": "sandbox", "tier": "medium", "head": 17, "busy": null, "entities": 412,
  "schema_version": "c3f1a9e2",
  "watch": { "tx": 17, "auto_revert": "armed", "target_ms": 16.67, "limit_ms": 20.83,
             "sustain_s": 3, "window_s": 10, "over": false },
  "moments": [ { "name": "drift", "label": "A drift through a bend with orange sparks (charge tier 2)" } ],
  "versions": {
    "engine": "0.1.0", "commit": "03979fc", "protocol": "evani-live/1",
    "parts": [
      { "kind": "core", "name": "core", "version": "0.1.0" },
      { "kind": "module", "name": "render", "version": "0.1.0" },
      { "kind": "module", "name": "fx", "version": "0.1.0",
        "features": ["bloom", "outline", "speedlines", "shake", "shadows"] },
      { "kind": "module", "name": "physics", "version": "0.1.0" },
      { "kind": "module", "name": "live", "version": "0.1.0", "protocol": "evani-live/1" },
      { "kind": "kit", "name": "kart-racer", "version": "0.1.0" },
      { "kind": "look", "name": "toon", "version": "0.1.0" } ],
    "line": "evani 0.1.0 @03979fc · core 0.1.0 · render 0.1.0 · fx 0.1.0 [bloom outline speedlines shake shadows] · physics 0.1.0 · live 0.1.0 (evani-live/1) · kit kart-racer 0.1.0 · look toon 0.1.0" } }
```

Answered by the page (the bridge answers `no_engine` when no page is connected). `watch` is the
armed auto-revert guard (`over`: the 0.5 s average is over the limit right now), or `null`.
`entities` is the number of live ECS entities right now (the review's architecture check fails a kit
that runs with 0). `moments` lists the game's scripted capture moments for `capture.frame { moment }` (6.14), in the
order the game registered them; `[]` when it has none.
`busy` is `null`, or while a run is active `{ "run": "b-3", "cmd": "bench.run", "until_ms": 7400 }`
(`until_ms`: estimated remaining milliseconds; the same value a refused command gets in
`busy.data.until_ms`).
`versions` says which Evani.Dev the page was built from: `engine` (the workspace version),
`commit` (the engine's short git hash at build time, `unknown` when built without git),
`protocol`, and `parts`, every Evani.Dev part compiled into this game in the order core, modules
(as the game added them), kit, look. Each part has `kind` (`core`, `module`, `kit`, `look`),
`name` (the module label or the kit's or look's name) and `version` (its crate version, baked
at build time; `null` for a kit or look that does not report one); `features` (fx: the effects
compiled in) and `protocol` (live) appear only where they apply. Only parts that are compiled in
can appear; game-local modules (a debug panel, the game's UI) are not Evani.Dev parts and are
left out. `line` is the same list as the debug overlay's first line.

---

### 6.27 `debug.geometry`

The geometry debug view, for fixing geometry with the AI. Live-only (evani-live; the renderer
honours its sky and camera overrides only under its `capture` feature). It runs three checks of
the geocheck library (docs/GEOCHECK.md) on what the game drew this frame: `overlap` (objects
intersecting by more than a small volume), `floating` (objects with a gap under them, or over
nothing) and `zfight` (overlapping, parallel faces of different objects closer in depth than the
depth buffer can separate as the live camera sees them, worked out from the renderer's own depth
maths: projection, depth format, f32 vertex rounding, sub-pixel snapping and the decal depth
offset; docs/GEOCHECK.md "The depth maths"). The other five checks need several frames, captures or a kit
adapter: run `cargo xtask geocheck <app>`.

```json
{ "params": { "on": true, "checks": ["overlap", "floating", "zfight"], "sky": true, "focus": 0, "every_s": 0.5,
              "tags": [{ "check": "floating", "object": "balloon*", "reason": "flies on purpose" }],
              "thresholds": { "sink": 0.2 } } }
```

All params are optional.

- `on`: `true` shows the view, `false` switches it off and gives back the sky and camera. Leave
  it out to only list issues (one analysis, nothing painted).
- `checks`: which of the three to run (default all). Another check name is `invalid_value`.
- `sky` (default `true`): while the view is on, the sky is magenta so gaps and cracks show.
- `focus`: an issue index from the reply: the camera frames that issue (from above and to the
  side) until `focus: null` or `on: false`. An index out of range gives the camera back.
- `every_s` (0.05–30, default 0.5): while on, re-check at most this often.
- `tags`: intentional cases, as in `geocheck.toml` (each needs a `reason`); matching issues are
  listed with `tagged` and not painted.
- `thresholds`: tolerance overrides, as in `geocheck.toml` `[thresholds]`.

While on, the view paints every drawn instance: overlaps red `#ff1a1a`, floating objects yellow
`#ffe61a`, z-fighting cyan `#1af2ff`, everything else grey; the sky is magenta. The look's
materials are drawn plain (white, lit, no glow) meanwhile, so these colours are not mixed with
the material colours (`CaptureQueue::plain_materials`, `capture` feature). A static object
keeps its colour between checks; a moving one keeps it by draw slot. Check it with
`capture.frame` (or `geometry: true` on a capture).

The reply comes after the next analysis (the next frame); `on: false` answers at once. Not a
change: no note, no history entry.

```json
{ "on": true, "sky": true, "checks": ["overlap", "floating", "zfight"], "focus": null, "every_s": 0.5,
  "frame": 9320, "instances": 1840, "total": 3,
  "counts": { "overlap": 2, "floating": 1, "zfight": 0, "tagged": 0 },
  "colors": { "overlap": "#ff1a1a", "floating": "#ffe61a", "zfight": "#1af2ff" },
  "notes": [],
  "issues": [ { "index": 0, "check": "overlap", "kind": "prop overlap", "objects": ["tree (sphere)", "rock (cube)"],
                "items": [212, 230], "at": [12.4, 0.8, -40.1], "aabb": { "min": [...], "max": [...] },
                "measure": 0.31, "unit": "m3", "detail": "...", "fix": "Place props with evani_render::place::Footprints so they can't overlap, or move one apart.",
                "severity": "error", "tagged": null } ],
  "summary": "3 issue(s) (overlap 2, floating 1, zfight 0, tagged 0)\n0. overlap, prop overlap: tree (sphere) / rock (cube) at (12.4, 0.8, -40.1): ..." }
```

`summary` is the same list as readable text (one line per issue, with its detail and fix), for
reading the reply directly. A `zfight` issue also has `data`: `separation_m` (gap along the normal,
smallest and largest), `epsilon_m` (the smallest gap the depth buffer separates there, from the live
camera), `fix_epsilon_m` (the largest such gap over every camera that sees it: what the fix's
number clears), `decal_offset_m`, `view_distance_m`, `overlap_m2`, `fight_m2`, `fight_px`, `normals`,
`polygon` (the flickering patch, world corners), `decal` and the depth setup.

`issues` lists at most 200 (`total` counts all); `items` are indices into that frame's draw list
(`RenderWorld::meshes`), followed by its decals (`RenderWorld::decals`): decal `k` is index
`meshes.len() + k`, and `instances` counts both. Custom meshes from the app's `MeshLibrary` are
analysed too (a swept road is split into convex segments for the overlap volume). A decal that does
not lie on a surface (more than 3 cm off, over nothing, or buried) is a `floating` issue of kind
`decal off surface`; a decal fights its surface only where it lies behind it by about its depth
offset (`decal behind surface`), and two overlapping decals on one plane fight each other. An item thinner than
the depth epsilon fights itself: the engine draws back faces, lit dark, so its underside shows through its top
(`thin slab`, both `items` the same index). `notes` lists what could not be
checked in full: a closed concave custom mesh that is a surface only, tightly bent sweep segments,
instances whose mesh has no registered geometry. No 3D renderer in the build: `unavailable`.

### 6.28 `focus.list` / `focus.set` / `focus.clear`

Asset focus, for tuning one asset's look: show it on its own on a calm turntable camera, then go
back to the full game without reloading the page. Live-only. Focus is a view, not a change: no
note, no history entry, never baked, and not undone by `history.undo`.

**How kits take part (dev builds only).** evani-core's `focus` feature (turned on only by
evani-live, and by a kit's own `live` feature) adds `KitPlugin::focus_assets`. A kit lists its
assets there:

```rust
#[cfg(feature = "live")]
fn focus_assets(&self, assets: &mut evani_core::focus::FocusAssets) {
    assets.add(FocusAsset::new("item_box", "Item box").kind("item")
        .params(["kit.kart_racer.item_box_"]).radius(1.4));
}
```

`params` entries are exact parameter names, or prefixes ending in `.` or `_`. `App::add_kit`
collects every kit's assets into the `FocusAssets` resource and tags each with its kit; a
duplicate id is an overlay error. evani-live inserts `FocusState`. Each frame the kit that owns
the focused asset (`FocusState::active_for`) draws only that asset (plus its static backdrop
unless `backdrop` is off), calls `FocusState::frame(FocusFrame { center, radius, front_yaw_deg })`
with where it is, and puts the returned pose on the camera; evani-core computes the turntable.
While `paused` is on, the core loop runs no fixed ticks but `Time::render_seconds` keeps
advancing, so spins, bobs and colour cycles still play; the sim carries on where it stopped
after `focus.clear` or `paused: false`. While focused, the page sets
`<html data-evani-focus="<id>">` and hides `#ui` and any `[data-evani-ui]` element, so only the
3D view shows. `capture.frame` works as usual (it captures the focused view).

**Opening the page on an asset.** `?live=1&focus=item_box` (optionally `&instance=2`) focuses it
at start; `view=` / `box=` are accepted as older spellings. An unknown id is reported in the
overlay's error log and the full game shows.

#### `focus.list`

```json
{ "params": {} }
```
```json
{ "assets": [
    { "id": "item_box", "label": "Item box", "kind": "item", "owner": "kart-racer", "path": "kits/evani-kit-kart-racer/src/draw.rs", "radius": 1.4,
      "params": ["kit.kart_racer.item_box_bob_m", "kit.kart_racer.item_box_glyph_glow", "..."] },
    { "id": "kart", "label": "Kart with driver", "kind": "vehicle", "owner": "kart-racer", "radius": 1.9,
      "params": ["kit.kart_racer.drift_slip_deg"] } ],
  "focus": { "active": null } }
```

`params` lists the parameters that exist in this build (prefixes expanded), in registry order.
`focus` is the same object `focus.set` returns.

#### `focus.set`

```json
{ "params": { "asset": "item_box", "yaw_deg": 30, "pitch_deg": 18, "distance": null, "spin_deg_s": 12,
              "fov_deg": 35, "paused": true, "backdrop": true, "instance": null } }
```

All params are optional, but `asset` is needed when nothing is focused. Giving a different
`asset` (or the first one) starts it with the default turntable, then applies the other fields;
giving the focused asset again, or leaving `asset` out, only changes the fields given.

| Field | Range | Default | Meaning |
|---|---|---|---|
| `asset` | an id from `focus.list` | | The asset to show. |
| `yaw_deg` | any finite number (wrapped to 0–360) | 0 | Orbit angle from the asset's front (0 looks at its front, as the kit defines it). The turntable adds to it. |
| `pitch_deg` | -89 to 89 | 18 | Camera elevation. |
| `distance` | 0.05 to 10000 m, or `null` | `null` | Camera distance; `null` fits the asset (its bounding sphere fills 55% of the view height, so a bigger asset is framed from further away). |
| `spin_deg_s` | -360 to 360 | 12 | Turntable speed; 0 keeps the camera still. |
| `fov_deg` | 5 to 150 | 35 | Vertical field of view. |
| `paused` | bool | `true` | Freeze the game while focused (its own animation keeps playing). |
| `backdrop` | bool | `true` | Keep the kit's static scenery behind the asset; `false` shows it over the sky. |
| `instance` | integer ≥ 0, or `null` | `null` | Which one, for an asset that exists several times (item box 3); `null` = the kit's choice. |

Result: the focus state. `frame` is what the kit reported on the last drawn frame (`null` until
it has drawn the asset, so right after `focus.set` it is usually `null`; read `focus.list` or
`session.status` a frame later). `params` are the asset's tunables.

```json
{ "active": "item_box", "yaw_deg": 30, "pitch_deg": 18, "distance": null, "spin_deg_s": 12,
  "fov_deg": 35, "paused": true, "backdrop": true, "instance": null, "drawn": true,
  "frame": { "center": [12.1, 1.7, -40.3], "radius": 1.63, "front_yaw_deg": 92.4, "distance": 8.8 },
  "params": ["kit.kart_racer.item_box_bob_m", "..."] }
```

Errors: `unknown_target` (no such asset; `data.available` lists the ids, `data.did_you_mean`
close ones), `not_focused` (no `asset` and nothing focused; `data.available`), `invalid_value`
(a field of the wrong type or out of range; `data.field`, `data.min`, `data.max`). On an error
nothing changes.

#### `focus.clear`

```json
{ "params": {} }
```
```json
{ "active": null, "was": "item_box" }
```

Back to the full game: the camera, the other objects and the page UI return, and the sim resumes.
Fine when nothing is focused (`was: null`).

**Also while focused.** `schema.get` lists the focused asset's parameters first (the rest keep
registry order; `schema_version` doesn't change) and adds `"focus": { "asset", "params" }`
(`null` otherwise). `session.status` has `focus` (the `focus.set` result, or `{ "active": null }`).

#### The asset view: `asset.list` / `asset.focus` / `asset.clear`

The same focus mode under the names and shapes Evani.App's Evani.dev tab asks for (its
`docs/evani-dev-asset-view.md`); additive to `evani-live/1`. They are thin adapters over
`focus.list` / `focus.set` / `focus.clear`, so everything above applies (the game is held, the
page UI is hidden, no note, no history).

- `asset.list { query?, kind?, limit? }` (read-only; `limit` 1 to 1000, default 200; `query` matches
  id or label, case-insensitive) returns
  `{ "assets": [{ "id", "name", "kind", "path", "focusable": true }], "total", "focused": id|null }`.
  `name` is the focus asset's label; `path` is the source file that defines it
  (`FocusAsset::path`, relative to the engine folder). `instances` is not reported.
- `asset.focus { id, view? }` focuses `id` and returns its record plus `view`. `view` is
  `turntable` (default: the camera turns at the default turntable speed) or `orbit` (`spin_deg_s` 0:
  a still camera; in the page, a drag turns the camera round the asset, the wheel zooms, and no
  pointer lock is used). The same id again changes only the view. Errors: `unknown_target` (with
  `data.did_you_mean`), `invalid_value` (`id` missing or `view` unknown).
- `asset.clear` is `focus.clear` with the result `{ "focused": null }`.
- `session.status` also has `focused`: the focused id or `null`.

### 6.29 `sweep.run`

Try a list of values for one parameter (or a small grid of several) on the fixed camera path and
get one compact table back: per row the values, the costs and the look-check measures. The way to
choose a value: one sweep instead of a `param.set` + `capture.frame` + telemetry round per try
(docs/LIVE_TUNING.md has the numbers). Apply the chosen values afterwards in one `param.set` (several
names) or one `batch` (6.25).

```json
{ "params": { "param": "fx.bloom.intensity", "values": [0.3, 0.6, 1.0],
              "frames_per_value": 120, "warmup_s": 0.25, "tier": "high",
              "look_at": [0.33, 1.0, 1.67], "look_width": 320, "bloom": true, "baseline": true,
              "thumbs": { "w": 160 } } }
```

- `param` + `values` (at most 32), or `grid: { name: [values], … }` for several parameters: every
  combination, names in sorted order, the last name varying fastest, at most 32 rows. Every row
  is planned like `param.set` before anything runs; a bad one fails the command with its usual
  error (`unknown_target`, `invalid_value`, …) and `data.row`. Out-of-range values are clamped and
  listed in `clamped: [{ row, name, requested, applied }]`. `aq.enabled` can't be swept.
- `path`: `"bench"` (default; the game's bench path, as `bench.run`). Another name answers
  `unknown_target` with `data.did_you_mean`.
- `frames_per_value` (10–3600, default 120) measured frames per row at the held sim times of the
  path, after `warmup_s` (0–10, default 0.25) unmeasured seconds. `tier` as `bench.run`.
- `look_at` (default three times spread over the run, at most 8; `[]` for costs only) are the path
  times of the frames the look measures average over, captured `look_width` wide (64–1024,
  default 320). `bloom` (default true when bloom is in the build) also captures each look frame
  with bloom off, for bloom coverage: once for the whole sweep when only `fx.bloom.*` is swept,
  else per row.
- `baseline` (default true) adds the current state as row 0 (`base_row: 0`).
- `thumbs`: `true` (160 wide) or `{ w, h }` (32–480 each, `h` optional): one small picture per
  row, from its middle look frame. Over MCP they are written to files (below).
- Nothing is kept: each row is applied without a history entry, and the run always ends by
  applying the starting state again, also after an error (`restored: true`). `busy` rules as
  `bench.run` (5.2): changes and the other run commands answer `busy` with `until_ms`, and
  `session.status.busy.cmd` is `"sweep.run"`. Run ids are `sw-N`. A failure (for example a capture
  `timeout`) cancels the rest and carries `data.row` when it belongs to a row.
- The rows run off-screen (6.30): the person watching sees the game hold its last frame, never a
  row's values; after the run the unchanged state shows, and the values applied afterwards with
  `param.set` or `batch` show at once.

```json
{ "run": "sw-4", "path": "bench", "tier": "ultra", "frames_per_value": 120, "warmup_s": 0.25,
  "look_at": [0.333, 1.0, 1.667], "look_width": 320, "gpu_timing": "timestamp",
  "truncated": false, "restored": true, "base_row": 0,
  "columns": ["fx.bloom.intensity", "work_ms", "cpu_ms", "gpu_ms", "gpu.shadows", "gpu.forward",
              "gpu.bloom.composite", "mean_luma", "log_avg", "luma_std", "luma_range", "saturation",
              "colorfulness", "clipped", "crushed", "bloom"],
  "rows": [[0.6, 4.1, 1.6, 2.2, 0.31, 0.92, 0.21, 0.4612, 0.1712, 0.2011, 0.6431, 0.4421, 38.12, 0.0122, 0.0011, 0.2213]],
  "thumbs": [{ "row": 0, "path": "C:/…/.evani/live/sweeps/sw-4/row-0.png", "width": 160, "height": 90 }] }
```

(Shape only; a real kart-racer table is in docs/LIVE_TUNING.md.)

- Costs are medians over the row's frames: `work_ms` (CPU work of the frame), `cpu_ms`, `gpu_ms`
  (total) and one `gpu.<pass>` column per pass any row measured; `null` (and `gpu_timing:
  "unavailable"`) without timestamp queries. Milliseconds have 3 decimals.
- Look measures are the look guard's statistics, version 1 (`tools/evani-lookguard/src/stats.rs`,
  same formulas in `crates/evani-live/src/look.rs`), on the final sRGB capture, averaged over the
  look frames: `mean_luma` (Rec.709 luma 0–1), `log_avg` (log-average linear luminance),
  `luma_std`, `luma_range` (p95 − p5), `saturation` (mean HSV saturation where V > 0.1),
  `colorfulness` (Hasler & Süsstrunk, 0–255 scale), `clipped` (fraction with max channel ≥ 250),
  `crushed` (fraction with luma ≤ 5/255) and `bloom` (fraction whose luma rises more than 3/255
  with bloom on). `bloom` is left out with `bloom: false`; all of them with `look_at: []`.
- Over MCP (`sweep_run`, 11) each thumbnail's `png_base64` is written to
  `<root>/.evani/live/sweeps/<run>/row-N.png` (or `thumbs.dir`, a folder inside the root) and
  replaced by `path`; `thumbs.inline: true` keeps them as image content instead. The tool returns
  the table as compact JSON, one line per row.

### 6.30 Trials run off-screen (the canvas hold)

Choosing values must not look like indecision: while a run tries settings, the person watching
the game sees none of them, only the result that is applied. So while `sweep.run` or
`compare.ab` runs (and while `capture.frame` shoots anything other than the next frame as
shown: temporary `values` or `tier`, geocheck options, `sim_time`, a `path` or fixed camera,
or a `moment`), the canvas is **held**:

- evani-live sets the `HoldCanvas` resource from the active run on every frame (on from the
  frame the run starts, before its first trial state is applied; off on the frame it ends,
  however it ends: done, failed, timed out). It can't outlive the run.
- The renderer still draws every frame through the whole graph. Only the last step changes: the
  `present` pass draws into an off-screen texture of the canvas's own size and format instead of
  the canvas texture, and no canvas texture is taken or presented, so the browser keeps showing
  the last frame presented before the run. A canvas resize (a tier with another render scale)
  waits until the hold ends, because resizing the canvas clears it.
- Captures, look measures and thumbnails read the `ldr` target as always, so they show the
  trial values. Timings stay honest: each frame is drawn once, and the `present` pass does the
  same work into a same-size texture, so `gpu.present` and `gpu_ms` are what they were. What
  is skipped is the browser's compositing of a new canvas frame, which no engine timing
  includes. A sweep of the same game measured before and after this change gives the same table
  within noise (the headless check is `tools/live-check/offscreen-trials.mjs`).
- In the page, `<html data-evani-trial="<run id>">` is set and a small "measuring…" tag sits in
  the bottom-right corner while held (dev builds only; nothing of this is in a shipped game,
  which has no evani-live and no render `capture` feature).
- The simulation is frozen during bench, compare and sweep runs as before (5.2); the hold only
  changes what is shown. `bench.run` is not held: it measures the game as shown. Plain changes
  (`param.set`, `batch`, `snapshot.restore`, `history.undo`, …) are never held and show on
  the next frame.
- A hidden tab runs no frames, so a run pauses with the canvas held and carries on when the tab
  is shown again; the hold ends with the run.

### 6.31 Edit transactions (parallel edits)

A request that asks for several live changes at once is several edits, and they are fast only
when they run side by side. So the AI that hears "turn the lights down, turn the map blue, and
reverse the rotation" does not make three changes one after another: it splits the request into
independent **atomic edits** (one change each), opens one transaction, and starts **one
sub-agent per edit, all at once** (no fixed cap: 3, 6 or 20 edits means as many agents). Each
agent works out its own edit (reads the schema, picks values) and stages it; edits never wait on
each other. The splitting is the AI's job; the engine makes the parallel edits safe, reports
every edit's outcome, and keeps clashes for the user to decide.

**Commands** (all dev-only, like the rest of evani-live):

| Command | Params | Does |
|---|---|---|
| `edit.begin` | `request?` (the user's words), `edits?` (the plan: strings, or `{id, what}`), `mode?` (`stream` \| `together`) | Opens a transaction. Returns `txn` (e.g. `edit-3`), `mode`, `state: "open"`, `edits` (one `pending` entry per planned edit, ids `e1`, `e2`, … unless given), `counts` and `how`. Without `mode`, the `live.edit.mode` parameter decides. At most 64 edits; 32 transactions are kept: finished ones drop off, and when all 32 are still open or waiting `edit.begin` answers `limit` (an open one is never dropped under its agents). |
| `edit.stage` (mutating, note required) | `txn`, `edit?` (an id from the plan, or a new one), `cmd` (one mutating command: `param.set`, `slot.set`, `curve.set`, `policy.set`, `effect.toggle`, `tier.set`, `snapshot.restore`, `batch`), `params` (that command's params), `clamp?` | Plans the edit at once and checks it against the transaction's other edits. Answers with the edit's `state` (below). The note is the edit's own note in history and CHANGES.md. |
| `edit.commit` (mutating, note optional) | `txn` | `together`: plans every staged edit again against the state now and applies them all on one frame. `stream`: closes the transaction. Returns the transaction (every edit with its state) and, when something was applied, the usual change result (5.4) with `applied` (each item tagged with its `edit`) and `txs`. |
| `edit.resolve` (mutating, note optional) | `txn`, and `keep` (the edit that wins) or `drop` (an edit whose whole conflict is dropped) | Settles a conflict after the AI asked the user. `keep`: the winner is applied (staged again in an open `together` transaction), the others in its conflict become `dropped`; its note records the choice. `drop`: every edit in the conflict is `dropped`. |
| `edit.status` | `txn?` | One transaction with every edit, or the recent transactions and `default_mode`. |
| `edit.cancel` | `txn` | Pending, staged and conflicting edits become `cancelled`; edits already applied stay (undo them with `history.undo`). |

**Edit states:** `pending` (planned, not staged yet), `staged` (checked, waits for commit in
`together`), `applied` (live, with its history `tx`), `failed` (with `error`: code, message,
data), `conflict` (with `conflict_with`), `dropped` (lost a conflict), `cancelled`. Transaction
states: `open`, `waiting` (committed, a conflict still waits for `edit.resolve`), `closed`,
`cancelled`.

**Apply modes.** Both are built; choose per transaction with `mode`, or change the default with
`param.set` of `live.edit.mode` (`stream` or `together`; never baked). The default is `stream`.

- `stream`: each staged edit goes live as soon as it is checked, through the ordinary commit
  path (a slot edit waits for its compile first, 6.5). The user sees each edit land as its agent
  finishes. `edit.commit` only closes the transaction.
- `together`: staged edits are held; `edit.commit` applies every one in the same tick, so they
  all show on the same frame and no in-between state is ever drawn. Plain parameter, curve,
  policy and effect edits need nothing more. When an edit settles over several frames (a slot
  compile, a tier switch), the commit holds the canvas (6.30: `HoldCanvas`, the page tag reads
  "applying…" and `data-evani-trial` is the transaction id) from the frame the compiles are
  requested until the first frame after everything is applied and settled, so the viewer goes
  straight from the old picture to the new one. A slot edit that fails to compile fails alone;
  the others still apply.

**Conflicts.** Each edit's targets are what its plan changes: a parameter, a slot, a curve, the
active tier, a tier preset's effect list. Two edits conflict when a target is shared, or when one
switches the tier and the other switches an effect (`fx.<name>.enabled`) or a tier's effect list
(a tier switch resets both). The check runs when an edit is staged, against the transaction's
other staged, conflicting and live edits, so the outcome is the same whatever order the edits
arrive in: every edit in the clash becomes `conflict`, and none stays applied. In `stream`, an
edit that was already live is **held back**: undone as a new history entry `edit.hold_back`
(its note says which edits clash and that the user decides), and the original entry becomes
`auto_reverted` with `revert_reason: "conflict"`, exactly as an auto-revert would (6.24), so undo,
history and bake treat it the same way. While the conflict waits, `history.undo` refuses to undo
that hold-back entry (`invalid_value`, pointing at `edit.resolve`), so what is live always matches
the transaction's state. The staging agent gets `state: "conflict"` with
`conflict: {with, targets, rolled_back, ask}`, an `edit.conflict` event goes out (`data`: `txn`,
`edits`, `targets`, `rolled_back`), and the transaction answers `ask` until it is resolved. The
AI must ask the user which edit wins (for "darker" and "brighter" on the same light Chris's rule
is "ask me") and then call `edit.resolve`; it never picks one itself. Edits that don't clash are
never held up: they apply (or stage) as usual and the commit applies them while the conflict
waits.

**Failures.** A failed edit (unknown name, wrong type, a slot that doesn't compile, or a value
out of range) answers `ok: true` with `state: "failed"` and the `error`, so one bad edit never
fails the others or the transaction. Inside an edit, out-of-range values fail with
`out_of_range` (5.5) instead of being clamped, because the AI asked for a value it didn't get;
stage it again within range, or with `clamp: true` to take the nearest allowed value. A failed
edit can be staged again under the same id.

**History, undo and bake.** Every applied edit is its own history entry with its own note,
tagged `txn` and `edit` in `history.list`, so each can be undone on its own and `bake.export`
writes the result as it writes any change (the bake format is unchanged). A `together` commit
pushes one entry per edit, all with the same `frame`. Auto-revert watches the last entry
applied, as it does for any run of changes: for a `together` commit that is its last edit.

**Example** (three agents, `stream`):

```json
→ {"cmd":"edit.begin","params":{"request":"turn the lights down, turn the blue map, reverse the rotation",
    "edits":["lights down","sky blue","reverse spin"]}}
← {"txn":"edit-1","mode":"stream","state":"open","edits":[{"edit":"e1","what":"lights down","state":"pending"}, …]}
→ (agent 1) {"cmd":"edit.stage","note":"turn the lights down","params":{"txn":"edit-1","edit":"e1",
    "cmd":"param.set","params":{"values":{"render.light.intensity":0.6}}}}
→ (agent 2) {"cmd":"edit.stage","note":"blue sky","params":{"txn":"edit-1","edit":"e2",
    "cmd":"param.set","params":{"values":{"render.clear_color":[0.1,0.25,0.8]}}}}
→ (agent 3) {"cmd":"edit.stage","note":"reverse the spin","params":{"txn":"edit-1","edit":"e3",
    "cmd":"param.set","params":{"values":{"kit.demo.spin_speed":-1.2}}}}
← each: the change result (5.4) plus {"txn":"edit-1","edit":"e1","state":"applied","mode":"stream"}
→ {"cmd":"edit.commit","params":{"txn":"edit-1"}}
← {"txn":"edit-1","state":"closed","counts":{"applied":3}, "edits":[…]}
```

`edit.stage`, `edit.commit` and `edit.resolve` answer `busy` while a bench, compare or sweep run
is measuring, like any change. The headless proof is `tools/live-check/edit-txn.mjs`.

## 7. In-page API: `window.evani.live`

Present only in builds with the `live` module (never in a shipped game).

```js
const r = await window.evani.live.call("param.set",
  { values: { "fx.bloom.intensity": 0.9 } },
  { note: "Debug panel: Bloom intensity", telemetry_ms: 0 });
// r = the `result` object; a failure rejects with an Error that has .code, .message, .data

const off = window.evani.live.on("auto_revert", (data) => { /* … */ }); // returns an unsubscribe fn

const reply = await window.evani.live.send(
  { type: "cmd", v: 1, id: "x1", cmd: "param.get", params: {} }); // whole envelope in and out
window.evani.live.protocol   // "evani-live/1"
```

- `send(msg)` takes a full 3.3 envelope (object or JSON string) and resolves with the full reply
  envelope (`{type:"result", v, id, ok, result | error}`); it never rejects.
- `call(cmd, params, opts)` builds the envelope for you (`source: "page"`, id `page-<n>`; `opts`
  = `{ note, telemetry_ms, capture }`, `note` lifted into the envelope, the rest into `opts`) and
  returns a Promise of the `result` object; a failure rejects with an Error that has `.code`,
  `.message`, `.data`. Mutating results arrive after their telemetry window (5.4) unless
  `telemetry_ms: 0`.
- `on(event, fn)` registers a listener for page events (`auto_revert`, `telemetry`, …) and returns
  an unsubscribe function.

Everything is queued and answered at a frame boundary. Commands arriving from the bridge go
through the same queue and code path. `bake.export` called in-page returns the bundle only
(6.23).

The page also sets, for Evani.App's build checker (`docs/evani-live-builds.md`):
`window.__engine = { started: false, error: null }` early, `started: true` once the first frame
is drawn, `error: "<message>"` if it can't start. It MUST start normally when its socket to the
bridge is blocked (Evani.App's hidden check copy blocks sockets).

---

## 8. Telemetry sources

Frame time from `performance.now()` deltas; CPU ms = time spent in the frame callback; GPU ms per
pass from `timestamp-query` when the adapter has it, else `gpu_timing: "unavailable"` and
`gpu_ms: null` (the overlay then shows a submit-to-done estimate marked `~`).

**Work time.** Each frame sample carries `work_ms` = CPU ms + GPU ms (GPU 0 when unknown) and a
`held` flag. A frame is held when it is stalled on purpose: a pending `capture.frame` or held sim
clock (and the second after), a `bench.run` or `compare.ab` run, or the settle time after a
device start. Held frames still count in `frame_ms`, but `work_ms` averages, auto-revert (6.24)
and adaptive quality skip them, so a capture never reverts a change or lowers quality. `held` is
not a separate output field; the overlay shows `(quality holding: capture)` while it applies. Draw calls, triangles and sprites are counted by the renderer per frame. All
sampling uses preallocated ring buffers (no per-frame allocation).

---

## 9. Safety summary

| Rule | Where |
|---|---|
| Localhost only, Host check, Origin check, token on every command path | 3, 4 |
| Values clamped to their ranges | 5.2 |
| Shader/expression patches compile before they swap; old kept on failure | 6.5 |
| Every change a versioned, undoable transaction | 6.20–6.22 |
| Auto-revert on sustained slow frames or device loss | 6.24 |
| Bake writes only inside the engine root, only to look/kit crates and `assets/tiers/` | 6.23 |
| Live code absent from builds without `live` | evani.toml / xtask |

---

## 10. Live rebuilds (optional in M1)

**Whole-build swaps are not built in M1.** The bridge builds the game once at start (2.3) and
serves no `/live-pkg/` routes; `/live/status` always has `build: 0`, `url: null`. Saved code
changes reach the running game as in-place patches instead (10.4, built). What follows up to 10.3
is the contract for when whole rebuilds are added.

Live control (section 6) never needs a rebuild. When the bridge also watches the game's sources
and rebuilds, it follows Evani.App's live-build contract (`docs/evani-live-builds.md`) so the tab
can check each build in a hidden copy before the visible game takes it:

1. `/live/status` returns `build: N`, `url: "/live-pkg/N/"` for the newest build.
2. On the relay (`/evani-dev` and the page's `/engine` socket):
   `{"type":"live_status","state":"rebuilding","build":N,"files":[…]}`, then
   `{"type":"live_status","state":"built","build":N,"url":"/live-pkg/N/","ms":…}` or
   `{"type":"live_status","state":"error","message":"<compiler error>","details":[…]}`.
3. The page at `/` is a loader: it reads `/live/status` and starts the build at `url` (or the base
   build when `build` is 0), so the tab can pin it to the last good build.
4. On `built`, the page saves its live session (values, slots, curves, history, snapshots) to
   `sessionStorage` and reloads itself, then restores it; it MAY instead swap in place and send
   `live_swapped` / `live_swap_failed` (`{"type":"live_swap_failed","build":N,"message":"needs a fresh start"}`
   makes the tab offer a Reload button).

Note: the tab's page script holds `live_status` `built`/`page_changed` and `live_hello` messages
on **every** socket the page opens until the build passes the check. Command results and events
never use those types, so they are not affected.

### 10.3 Session mirror

The page sends the bridge a compact copy of its session after every transaction, undo/redo and
auto-revert (`{"type":"event","v":1,"event":"session.mirror","data":…}`, forwarded to every `/ctl` client, always on: no subscription needed, so the tab sees each
committed change within a frame instead of polling; the bridge also keeps the latest one):

```json
{ "head": 17, "next_tx": 18,
  "history": [ { "tx": 17, "cmd": "slot.set", "note": "…", "source": "mcp", "at": "2026-10-14T…Z",
                 "frame": 9312, "summary": "tonemap slot rewritten", "status": "applied", "revert_reason": null } ],
  "snapshots": [ { "id": "snap-4", "name": "warm sunset", "tx": 17, "at": "…", "note": "…" } ] }
```

It holds history summaries and the snapshot list only, **no state values** (no parameter
values, slot sources or snapshot contents), so it can't restore a session by itself.

The bridge keeps the latest mirror and offers it on **every** successful page connect, right
after `auth`, as the same event. Today the page does not restore from it; restoring (which would
need the values too) is a later stage.

### 10.4 Live code swapping (built; dev builds only)

A saved Rust change is compiled into a small patch and swapped into the running page between two
frames. No reload: game state, camera, live values, history and snapshots all stay. It works for
**every** game, kit, look and engine crate with no code in the game. Phase A; changes too big to
patch reload the page plainly (below). Evani.App's side is specified in `docs/live-code-tab.md`.

**Turning it on.** `serve` does it by default (so the Evani.dev tab gets it through
`live.mjs` with no setting), as does `cargo xtask dev --hot <game> [--port P]`. Off:
`serve --no-hot`, `EVANI_LIVE_HOT=0`, or `serve --no-build`; then the game is built with Trunk as
before.

**The builder.** The bridge runs a pinned `dx` (Dioxus CLI, git main at rev
`f420ca3d6cb6812ed27a021e239d7ec0941ff317`; released dx 0.7.x can't read this workspace's
wasm-bindgen output) as `dx serve --web --hot-patch -p <game package> --features live`, on a
private localhost port (`EVANI_DX_PORT` picks it), at below-normal priority with
`CARGO_BUILD_JOBS=2` unless set, into its own target dir `target/evani-live/dx`. On Windows dx and
everything it starts sit in a job object, so they end with the bridge. dx is looked for in
`EVANI_DX`, then `target/evani-tools/dx/bin/dx(.exe)` (`cargo xtask dx-install` builds it there,
about 30 minutes), then the spike's build. The first build of a game takes a few minutes; while
it runs `/` shows the holding page as for a Trunk build.

**Fallback.** No dx, or dx failing before its first build: the bridge builds with Trunk as before
and the page simply has no live code swapping. `/live/status` `code` always says which:

```json
"code": { "mode": "hot", "builder": "dx", "dx": "C:\\…\\dx.exe", "dx_rev": "f420ca3…",
          "dx_port": 8162, "state": "building" | "ready" | "error",
          "seq": 3, "history": [ { "state": "patched", "at": …, "seq": 3, "functions": 412, "ms": 3600,
                                    "message": "Code updated: 412 functions, 3.6 s, no reload" } ] }
"code": { "mode": "trunk", "reason": "the pinned dx isn't installed (run `cargo xtask dx-install`)", "seq": 0, "history": [] }
"code": { "mode": "off", "reason": "dx stopped (exit …)" }   // dx died after the game was up: the game keeps its last build
```

`seq` is the newest good patch; `history` the last 20 events (`patched`, `error`, `failed`,
`crashed`, `reload`, `stalled`).

**What swaps.** With evani-core's `hotpatch` feature (evani-live's default feature, so every
game's `live` feature pulls it in), every system registered through `SystemDesc` (kits, looks,
the game's own), every engine module's `frame`, every scene's `update` and the runner's frame
step are called through subsecond's jump table. Code they call is swapped with them. Not swapped:
code that only runs at start (`build`, scene `enter`) or in callbacks registered once; it takes
the new code on the next reload. A change dx can't patch (a struct's layout, a function
signature, `Cargo.toml`) is rebuilt in full and the page reloads (`code_reload`).

**Bridge → page** (on `/engine`; the page's own origin serves the patch files under `/wasm/`):

| Message | When |
|---|---|
| `{"type":"code_patching","at":<ms>,"files":[]}` | dx started compiling a saved change |
| `{"type":"code_patch","seq":N,"table":{…},"functions":412,"build_ms":2140,"started_at":<ms>,"files":["kits/…/draw.rs"]}` | a patch is ready. `table` is subsecond's `JumpTable` with `lib` rewritten to `/wasm/<file>` and `ifunc_count` counted from the patch's own element section (dx main undercounts). `functions` is the size of the swap map: every function in the recompiled code, not only the edited ones |
| `{"type":"code_error","at":<ms>,"message":"error[E0609]: no field `lapz` on type `&PlayerHud`\n  --> apps/kart-racer/src/ui.rs:199"}` | the change doesn't compile; the page keeps the old code (see "Compile errors" below) |
| `{"type":"code_reload","at":<ms>,"reason":"…"}` | a full rebuild was needed; the page reloads |

Patches are cumulative from the build the page loaded, so the newest one alone brings a page up to
date: a page whose `hello` says `"code":{"hot":true,"seq":S}` is sent the newest good patch if it
is newer than `S` (a reload, a second tab). A full rebuild drops the kept patches.

**The page.** It fetches and compiles the patch itself, catching every error, and checks that each
import resolves against the running module; only then, at the top of the next frame, does it hand
the patch to subsecond, which commits the new table between frames. It then shows a small toast
("Code updated: N functions, X s, no reload"; compile errors stay on screen, click to close),
sets `window.__evani_code = {hot, applied, failed, active, last, history}` and sends:

| Page event (`{"type":"event","v":1,"event":…,"data":…}`) | Data |
|---|---|
| `code.patched` | `{seq, functions, ms, build_ms}`; `ms` from the save being seen to the new code running |
| `code.failed` | `{seq, message}`: the patch could not be loaded; the old code keeps running |
| `code.crashed` | `{seq, message}`: a panic with patched code in use. wasm can't continue after a panic, so the page reloads (at most twice a minute) and gets the last patch that worked |

A failed or crashed patch is never sent again.

**Stuck patches.** A patch the page neither applied (`code.patched`) nor failed (`code.failed`)
15 s after it was sent, while the page ran at least 60 frames since (its `page_state`, 3.5), is
stuck. The bridge sends `code_reload` (the reloaded page gets the same patch afresh with its
`hello`), the `/ctl` event `code.stalled {at, seq, reason}`, a `stalled` history entry and the
tab's `live_swap_failed` `kind:"code"`. The same patch stuck a second time is dropped (marked bad,
like a failed one) and the page reloads with the last patch that worked. A parked or paused page
that runs no frames is never counted as stuck: its patch waits, and the heartbeat applies it
(3.5). A page from before 3.5 sends no `page_state`, so the watchdog stays off for it.

**Baked data doesn't swap code.** A bake (`bake.export`, 6.23) or a restore writes a look's or
kit's data files (`params.json`, `policy.json`, `slots/*.wgsl|expr`, `curves/*.json`, plus
`CHANGES.md` and `bake-manifest.json`). Those are not Rust, and they never set off a code swap:

- When it starts dx, the bridge reads every look's and kit's baked files and passes dx
  `EVANI_BAKED_STAMP` (an FNV-1a hash of them) and `EVANI_GIT_COMMIT` (the current short hash).
  With the stamp set, `tools/bake-index.rs` writes the file contents into the build as string
  literals and tracks no data file or folder, and `evani-core`'s build script tracks no git file,
  so nothing a bake or a `git restore` writes appears in the build's dependency list and dx
  ignores it. (Without the stamp, as in Trunk and ship builds, the build reads and tracks the
  files as before.)
- The bridge keeps the files as they were at that start. `/` carries
  `<script type="application/json" id="evani-live-baked">{"look.toon":{"params.json":"…"}}</script>`
  with every file that differs now (owners `look.<name>` for `looks/evani-look-<name>`,
  `kit.<name>` for `kits/evani-kit-<name>`; every `<` written as `<`). At start the page
  applies its own kit's and look's entries before anything reads them, so a reload after a bake
  shows the baked values. The running page needs nothing: a bake writes the values it is already
  running.

**Compile errors.** For a kit, look or engine crate dx passes on rustc's full rendered errors and
the message is that text (colours removed, at most 8 KB). For the game's own crate dx (run
non-interactively) logs only rustc's short message per error and a bare "Build failed: Cargo
build failed - no output location"; the bridge rebuilds rustc's shape from those lines
(`error[E0609]: no field `lapz` on type `&PlayerHud``, the code taken from rustc's "try `rustc
--explain E0609`" line when there is one error) and adds the likely place: the `.rs` file saved
most recently, and the first line in it naming the first plain identifier the message quotes
(`--> apps/kart-racer/src/ui.rs:199`, or the file alone). The full text of dx's run is in
`target/evani-live/dx/dx.log`.

**`/ctl` events** (always on, no subscription): `code.patching {at, files}`, `code.error {at,
message}`, `code.reload {at, reason}`, `code.stalled {at, seq, reason}`, and the page's `code.patched` / `code.failed` /
`code.crashed` as sent.

**The tab** (`/evani-dev`) gets the shapes it already reads (3.4): `live_status` `applied`
`kind:"code"` for a swap, `live_status` `error` `kind:"code"` for a compile error (one line:
`Code change didn't build: <first error> (<file>:<line>)`), `live_swap_failed` `kind:"code"`
"needs a fresh start" for a reload, a failed load or a crash.

**Never shipped.** The `live` feature is dropped from shipped builds, and with it `hotpatch` and
subsecond. `cargo xtask build <game> --ship` and `size-report` fail if the wasm or JS contains
`subsecond`, `hotpatch`, `evani-code-toast` or `__evani_code`. Measured 2026-10-14: the shipped
kart racer and sandbox are the same size as before this feature (byte-identical wasm length).

---

## 11. MCP server

`evani-live-bridge mcp [--root <engine root>]`: MCP over stdio. It reads
`<root>/.evani/live/session.json` (the most recent session; another with `session` or
`EVANI_LIVE_SESSION`, 2.6) (root defaults to the current folder, which is the project
folder for Claude Code sessions), connects to `/ctl` with the token, and maps each tool to one
command. If no bridge is running, tools return an error result: "The live server isn't running.
Open the Evani.dev tab with this engine's folder, or run `cargo xtask live`."

Registered in the engine root's `.mcp.json` (written by `cargo xtask init-ai`) so the project's
orchestrator and workers get it:

```json
{ "mcpServers": { "evani-live": { "type": "stdio", "command": ".evani/bin/evani-live-bridge", "args": ["mcp"] } } }
```

The command is a prebuilt binary that `cargo xtask doctor --fix` (or `init-ai`) builds into
`target/mcp` and copies to `.evani/bin/` (gitignored), with a stamp of the hash of its sources.
It used to be `cargo run`, which rebuilt the bridge at every session start whenever its sources
had changed (40 to 150 s at two build jobs) and ran past Claude Code's MCP start-up timeout
(about 30 s), so the server showed as failed to connect. The prebuilt binary answers
`initialize` at once; `cargo xtask check` and `doctor` warn when it is older than its sources.
The path is relative because Claude Code runs it from the project folder and doesn't expand
`${CLAUDE_PROJECT_DIR}` in `.mcp.json` commands. Each checkout runs `cargo xtask doctor --fix`
once.

Claude Code shows the tools as `mcp__evani-live__<tool>`. Evani.App lists project `.mcp.json`
servers and they need approving once for the project.

Tool input = the command's `params`, plus `note` (required on mutating tools), the `opts`
fields flattened (`telemetry_ms`, `capture_width`) and the optional `session` (which bridge, 2.6;
never sent on). Tool output = the command's `result` as JSON
text; captures are also returned as MCP `image` content (`image/png`) so the model sees them.
Every `*png_base64` field in the result is pulled out into image content and replaced in the
JSON text by the marker `"(image N, attached as image content)"`, so base64 never fills the
model's context twice.
Errors set `isError: true` with `"<code>: <message>"` and the `data` as JSON. Every tool result
also carries `events_since_last_call` (auto-reverts, device loss) because MCP can't push to the
model.

| Tool | Command | Description shown to the AI |
|---|---|---|
| `live_status` | `session.status` | Is the game connected; active tier; history head; whether a change is being watched for auto-revert. Call first. |
| `live_sessions` | (in the MCP server) | The live sessions running in this project folder (several games can be live at once, each on its own port): port, game, kind, started, whether it answers, and which one tools use by default (the most recent, or EVANI_LIVE_SESSION). Pass a port or game name as `session` on any other tool to drive that one. (2.6) |
| `schema_get` | `schema.get` | List every tunable parameter, shader slot, curve, effect, tier and the adaptive-quality policy, with types, ranges, units and descriptions. |
| `param_get` | `param.get` | Read current parameter values (by names or prefix). |
| `param_set` | `param.set` | Set one or more parameters atomically on the next frame; out-of-range values are clamped. Needs a note saying why. Returns old/new values and the next second of frame timing. Put every value of a change in one call (several names in values, or batch); to choose a value, use sweep_run first instead of one param_set per try. |
| `slot_get` | `slot.get` | Read a shader slot's WGSL body (or an expression slot's formula) and its fixed signature. |
| `slot_set` | `slot.set` | Replace a shader slot body (WGSL, body only) or an expression slot. Compiles first; on error returns line/column in your body and keeps the old shader running. Needs a note. |
| `curve_get` | `curve.get` | Read a piecewise-linear curve's control points. |
| `curve_set` | `curve.set` | Replace a curve's control points (x increasing). Needs a note. |
| `policy_get` | `policy.get` | Read the adaptive-quality policy (frame target, window, hysteresis, degrade order, render-scale step and floor, effect weights). |
| `policy_set` | `policy.set` | Patch the adaptive-quality policy. Needs a note. |
| `effect_toggle` | `effect.toggle` | Turn an effect on or off now (scope session) or in the active tier's preset (scope tier). Reports what the budget switched. Needs a note. |
| `tier_set` | `tier.set` | Switch quality tier live (lite, low, medium, high, ultra, vr). Needs a note. |
| `telemetry_sample` | `telemetry.subscribe` + wait + `telemetry.unsubscribe` | Measure for N seconds (default 2) and return averaged frame time, per-pass GPU ms, draw calls, triangles/sprites and budget points. |
| `capture_frame` | `capture.frame` | Take a downscaled screenshot with stats, optionally at a fixed simulation time on the deterministic camera path or from a placed camera; `sky`, `jitter_px` and `geometry` for geometry checks. |
| `focus_list` | `focus.list` | List the game's assets that can be shown on their own (id, label, kind, the parameters that tune each) and what is focused now. |
| `focus_set` | `focus.set` | Show one asset on its own on a slow turntable camera (game paused, its animation playing, its tunables first in `schema_get`), or move the turntable. No note. |
| `focus_clear` | `focus.clear` | Leave asset focus: back to the full game, which carries on where it paused. |
| `debug_geometry` | `debug.geometry` | The geometry debug view: list overlaps, floating objects and z-fighting with fixes; `on` paints them red, yellow and cyan with a magenta sky; `focus` frames one. |
| `bench_run` | `bench.run` | Run the fixed camera path for N seconds at a tier; returns timing stats, pass/fail against the tier's limits and captures. |
| `compare_ab` | `compare.ab` | Compare two states on the same camera path and sim time: A (default the current state) against B (a snapshot, or changes to try such as `{"values":{"fx.bloom.enabled":false}}`). Returns both screenshots, a per-pixel difference, frame and per-pass GPU timing for each and the delta. B is reverted afterwards unless keep is true (then it needs a note). |
| `sweep_run` | `sweep.run` | Try a list of values for one parameter (or a grid of a few) on the fixed camera path and get one compact table back: per row the values, median work/CPU/GPU ms and per-pass GPU ms, and the look-check measures. Row 0 is the current state. Nothing is kept; apply the chosen values afterwards with one param_set or batch. Thumbnails, if asked, are written to files and returned as paths. |
| `snapshot_save` | `snapshot.save` | Save the whole live state under a name to compare or return to. |
| `snapshot_list` | `snapshot.list` | List saved snapshots. |
| `snapshot_restore` | `snapshot.restore` | Return to a saved snapshot (undoable). Needs a note. |
| `history_undo` | `history.undo` | Undo the last change(s). |
| `history_redo` | `history.redo` | Redo undone change(s). |
| `history_list` | `history.list` | List recent changes with their notes, source and whether they were undone or auto-reverted. |
| `batch` | `batch` | Apply several changes as one atomic, undoable step with one note. The way to apply a set chosen with sweep_run: one batch of param.set (and slot/curve/tier) commands instead of one call per change. |
| `edit_begin` | `edit.begin` | Start an edit transaction for a request that asks for several live changes at once: split it into independent atomic edits, list them, then run one sub-agent per edit in parallel (no cap), each calling `edit_stage`, and call `edit_commit` when all are done. `mode` stream (default) or together. Returns the transaction id and the plan. |
| `edit_stage` | `edit.stage` | Stage one atomic edit (one mutating command with its own note). Answers applied (stream), staged (together), failed (this edit only, with why; `out_of_range` unless `clamp`) or conflict (ask the user which wins, then `edit_resolve`). Needs a note. |
| `edit_commit` | `edit.commit` | Finish a transaction: together applies every staged edit on one frame; stream closes it. Returns every edit's result. |
| `edit_resolve` | `edit.resolve` | After asking the user: `keep` the winning edit, or `drop` the whole conflict. Never pick a winner without the user. |
| `edit_status` | `edit.status` | One transaction with every edit's state, or the recent ones and the default mode. |
| `edit_cancel` | `edit.cancel` | Cancel a transaction: staged and conflicting edits are dropped, applied ones stay (undo with `history_undo`). |
| `bake_export` | `bake.export` | Write the tuned session into the game's source files (tier presets, look parameters, slot .wgsl files, curves) plus a CHANGES.md built from the notes. Use dry_run first to see the plan. |
| `evani_check` | `local.check` (runs in the MCP server, no game needed) | The Evani check gate (same as `cargo xtask check`): module lists, catalog fit, geocheck reports, review card FAIL lines and look guard, each issue with a fix and a doc. Repeat until clean before hand-over; `full` also runs the tests and the ship build. `app` limits it to one game. See docs/AI_GUIDANCE.md. |

`telemetry.subscribe/unsubscribe` remain available to WebSocket clients; over MCP they are
wrapped as `telemetry_sample`.

State after stage 6: every tool above is wired and answered by the page. `compare_ab` takes `a`
/ `b` as a string or an object (`oneOf`), plus `keep`, `note` (lifted into the envelope like any
note) and `warmup_s`; only `b` is required. The bridge's wait for `bench.run` / `compare.ab` is
their own length (warm-ups and every run of the order included) plus 30 s. The server
instructions tell the AI to measure with `compare_ab` before keeping a change and to wait and
retry on `busy`. Tools come in three kinds: reads (arguments are the params), changes (`note`, `telemetry_ms`,
`capture_width` lifted into the envelope; the note is optional only for `history_undo` /
`history_redo`, `edit_commit` and `edit_resolve`) and the telemetry sample. The server
instructions also tell the AI to split a request for several live changes into atomic edits and
run them as parallel sub-agents through an edit transaction (6.31), asking the user on a
conflict. `bake_export` is a read-kind tool even though the
bridge writes files (there is no separate "writes" kind); its description says to use
`dry_run` first.

Suggested line for the engine project's CLAUDE.md (not part of this contract): *Messages starting
with `[Evani.dev]` come from the live engine tab: use the `evani-live` MCP tools, give every change
a short note, check `capture_frame` after visual changes, and bake only when asked.*

---

## 12. Tab compatibility

**Verdict: matches the tab's current protocol. No change to Evani.App is required** for DONE WHEN
check 6 (open the Evani.dev tab with the engine folder selected, connect to the running sandbox,
"Talk to the AI" changes Bloom, rewrites the tonemap slot and bakes). How each part matches:

| Tab behaviour today (Evani.App source) | Evani.Dev side |
|---|---|
| Finds `<folder>/.vibedeck/dev-harness/live.mjs` (`findEngineFolder`, `LIVE_SCRIPT` in `src/main/evaniDevCore.ts`) | The launcher shim at that path (2.1) starts `evani-live-bridge serve`. |
| Starts it with Electron's Node, `--port <url port> --quiet`, waits 60 s for `/live/status` (`engineLaunch`, `startServer`, `waitUp` in `src/main/evaniDev.ts`) | Shim passes the port; the bridge answers `/live/status` before building (2.3). |
| Probes `GET /live/status`, reads `relay` (`ws://localhost:<port>` only) and `build`/`url` (`probe`, `relayFromStatus`, `pinFromStatus`) | Same JSON (3.1); `relay` is the bridge's own port. |
| Connects `${relay}/evani-dev`, no token; reads `chat`, `live_status`, `live_swapped`, `live_swap_failed`; sends `chat_reply` (`connectRelay`, `parseRelay`, `relayReply`) | `/evani-dev` endpoint, notifications only, no token (3.2, 3.4). |
| Shows the page from its URL setting in a locked-down view, same origin only | The bridge serves the game at `/` on that origin. |
| Hidden build check reads `window.__engine` and failure signals (`src/main/evaniDevLiveCore.ts`, `docs/evani-live-builds.md`) | Page sets `__engine`; rejected patches never hit `console.error` (6.5, 7); live rebuilds follow the contract (10). |
| "Talk to the AI" goes to the orchestrator of the project whose folder is the engine folder (`findLiveEngineProject` in `src/shared/evaniDev.ts`, `send` in `src/main/evaniDev.ts`) | That orchestrator drives the engine with the `evani-live` MCP tools from `.mcp.json` (11). |
| Reads `live_swapped` / `live_swap_failed` (`build` a whole number of 0 or more; "needs a fresh start" offers Reload) and holds `live_status` `page_changed` on the page's sockets until its hidden check passes | Page hot-replace (14) sends exactly those: `live_swapped` with `build: 0` after an in-place swap, `live_swap_failed` + `page_changed` when a reload is needed. CSS swaps send no `page_changed`, so they never trigger a hidden check. |

The tab sends no control commands itself, so the token rule doesn't touch it. The control API
(section 6) is new; the tab doesn't need to speak it.

### Recommended tab changes (not required; in order of value)

1. **Default to Evani.Dev.** Live-Engine is retired (Chris, 2026-10-13). The tab's default
   address (`DevSettings.url` in `src/shared/evaniDev.ts`, currently
   `http://localhost:8095/?live=1`) should become `http://localhost:8096/?live=1`, and the
   fallback to a project named "Live-Engine" (`findLiveEngineProject`) should go. Until then, set
   the address to 8096 by hand in the tab's settings. Also suggested: `probe()` / `check()` in
   `src/main/evaniDev.ts` accept whatever answers on the port; when `/live/status` has a `root`,
   compare it with the chosen engine folder and say "Another engine is using this address" if
   they differ.
2. **"Live-Engine" wording.** `LIVE_ENGINE_NAME`, `buildFixText`, `BUILD_FAILED_NOTE` and the
   "Send to Live-Engine to fix" / "Couldn't find the Live-Engine project" texts in
   `src/shared/evaniDev.ts` and `src/main/evaniDev.ts` name Live-Engine. Routing already follows
   the engine folder, so this is wording only: *Required:* use the target project's name.
3. **A manifest instead of a Node path.** `LIVE_SCRIPT` hard-codes
   `.vibedeck/dev-harness/live.mjs` and the launch is always `node <script>`. *Suggested:* also
   accept `<folder>/.evani/live.toml` (`start = ["cargo","run","-q","-p","evani-live-bridge","--","serve","--port","{port}"]`,
   `status = "/live/status"`, `start_timeout_s = 300`), so native engines need no shim and a
   first compile isn't cut off at 60 s.
4. **Optional live panel.** A future tab panel (sliders, slot editor, A/B view) would connect to
   `/ctl` with the token from `.evani/live/session.json`, the same way the MCP server does.

---

## 13. Open questions

1. (Decided 2026-10-13: Evani.Dev uses port 8096; the tab's address is set to
   `http://localhost:8096/?live=1`. Live-Engine is retired and no longer used.)
2. Auto-revert baseline guard (6.24): a change made while already over budget is not armed. The
   brief doesn't say this; it avoids undoing innocent changes on a slow machine.
3. Workers launched by the Evani.Dev orchestrator get the MCP tools only if the project's
   `.mcp.json` server is approved for the project in Evani.App.
4. (Resolved 2026-10-14.) First start compiles the bridge and the sandbox. The launcher answers
   `/live/status` itself while the bridge compiles, and the bridge answers it while the game
   builds (2.1, 2.3), so the tab's 60 s wait never runs out; the holding pages carry the user to
   the game. Tab change 3 would still remove the Node shim.
5. (Resolved 2026-10-15.) `.mcp.json` used to start the MCP server with `cargo run`, which timed
   out at session start after bridge changes. It now runs the prebuilt `.evani/bin` copy (11);
   `doctor --fix` renames a locked old `.exe` aside before copying the new one.
6. (Stage 5a, still open.) Not built yet: the bridge's 4-subscriptions-per-connection cap,
   `effect.toggle`'s `override_budget`, explicit `degrade_order` lists and restoring a session
   from `session.mirror`. The stage's choices are listed under "Changes in stage 5a" at the top.
   (`capture.frame`, `bench.run` and `compare.ab` were built in stage 6; see "Changes in
   stage 6".)
7. (Stage 6.) `bench.run` / `compare.ab` on the live page measure at the display's frame cap
   (vsync), so `frame_ms` is usually pinned near the refresh interval and the useful signal is
   `gpu_ms` (timestamp queries). Uncapped numbers still come from `cargo xtask bench`.

---

## 14. Page hot-replace (HUD and page files, no reload)

Live changes are swapped into the area that changed, never by reloading the engine. Which path
an edit takes depends on what it is:

| The edit is… | Path | Reload? |
|---|---|---|
| a number, colour or layout value (incl. HUD positions and sizes) | a tunable: `param.set` (6.3), the Look Lab | no |
| a shader | a slot: `slot.set` (6.5) | no |
| a stylesheet, inline `<style>`, the `<title>` or static page text | page hot-replace (this section) | no |
| page structure the game's code binds to, scripts | the tab's Reload note (14.5) | yes |
| Rust code | live rebuild (section 10; function-level patching is a separate, later path) | per 10 |

### 14.1 What the bridge watches

`serve` reads `apps/<game>/index.html` and every stylesheet it links that sits inside the app
folder: `<link rel="stylesheet" href="…">` and Trunk's `<link data-trunk rel="inline"
type="css" href="…">` / `rel="css"`. It checks them every 250 ms (size and modified time, then
content; a save of the same bytes is no change). Page versions are tagged `<session>:<rev>`
(rev 0 = the page when the bridge started); the bridge keeps the last few.

Page files never trigger a wasm build: the bridge re-serves the page part only. Rust sources
are section 10's business.

### 14.2 The served page (`compose`)

`/` (and `/index.html`) is composed from the page files **as they are on disk now** plus the
build's own tags (Trunk's module script and preload links, taken from the build's
`index.html`):

- every inline `<style>` gets `data-evani-style="style:N"` (N = its order in the page),
- every watched stylesheet is inlined as `<style data-evani-style="<href>">…</style>`,
- `<meta name="evani-page-rev" content="<session>:<rev>">` goes in `<head>`, then the token
  meta (4.3) as before.

If the page can't be composed (a Trunk asset the bridge doesn't know, a holding page) the
build's own `index.html` is served unchanged, and hot-replace is off for that page. A shipped
game is built by Trunk alone and has none of these marks.

The page's `hello` (3.3) adds `"page_rev": "<content of that meta>"` (or `null`).

### 14.3 `page_patch` (bridge → page, on `/engine`)

```json
{ "type": "page_patch", "v": 1, "seq": 4, "from": "s-1a2b:2", "to": "s-1a2b:3",
  "files": ["hud.css"],
  "ops": [ { "op": "css", "key": "hud.css", "text": ".hud .mid.wrong { … }" },
           { "op": "html", "id": "title", "old": "<h1>…</h1>…", "html": "<h1>…</h1>…" },
           { "op": "title", "html": "Sugarpeak Rally" } ] }
```

- `css`: the new text of `style[data-evani-style="<key>"]`. Keys and ids with `"` or `\` are
  refused.
- `html`: the new content of `#id`, **only if** the element's current children still equal
  `old` parsed the same way (`isEqualNode`, node by node). If the game changed that part since
  the page loaded, the patch is refused.
- `title`: the new `<title>` content.

The page applies a patch **all or nothing**: it checks its rev equals `from` and every op's
target first, then applies every op, sets the meta to `to`, and sets `window.__evaniPageRev`
(the rev) and `window.__evaniPagePatches` (swaps this page has taken, for tests). It replies:

```json
{ "type": "event", "v": 1, "event": "page.patched",
  "data": { "seq": 4, "ok": true, "rev": "s-1a2b:3", "applied": 1 } }
```

or `"ok": false, "error": "<plain reason>", "rev": "<unchanged rev>"`. The bridge also forwards
`page.patched` to every `/ctl` client. The bridge waits up to a few seconds for it.

### 14.4 What can be swapped in place

The bridge compares the page the browser has (`from`) with the new one. An element's content is
swappable when the element has an `id`, **no** `id` anywhere inside it, isn't raw text
(`script`, `style`, `title`, `textarea`, `template`) and has content in the file (an element left
empty in the file is one the game's code fills). With those parts and every `<style>` and the
title set aside, the rest of the page (tags, attributes, ids, scripts and Trunk links, text
outside the parts) must be the same; comments and white space don't count. Then the ops are:
one `css` per changed stylesheet or inline style, one `html` per changed part, a `title` if it
changed. An edit with nothing visible (a comment, re-indenting) is an empty patch: the page's
rev moves on, nothing else changes.

On success the tab gets `live_status` `applied` (`kind: "page"`, the files and a one-line
message) and `live_swapped` (`build: 0`, `kind: "page"`).

### 14.5 When a reload is needed

Everything else needs a fresh page: ids added, removed, renamed or reordered; markup changed
inside an element that holds ids or that the code fills; an element's own attributes; scripts
or Trunk links; a part the game changed since load (the page refuses); a page from before this
bridge started; or no answer from the page. Then the bridge sends, through the existing
contract (12):

- `/evani-dev`: `{"type":"live_swap_failed","build":0,"kind":"page","message":"needs a fresh start","reason":"…"}`
  (the tab's Reload note) and `{"type":"live_status","state":"page_changed","kind":"page","files":[…],"message":"…"}`,
- `/engine`: the same `page_changed` (the tab's gate holds it until its hidden check passes; the
  page itself ignores it).

The next load of `/` serves the new page. When no page is connected, a page edit only makes a
`live_status` `applied` line ("the game page takes them when it opens").

### 14.6 Rules for page authors

- Look numbers (positions, sizes, colours) are tunables with defaults in code; the page reads
  them as CSS variables with the same defaults as fallbacks (docs/KIT_PROCESS.md).
- Put a game's HUD styles in their own stylesheet (the kart racer: `apps/kart-racer/hud.css`,
  inlined by Trunk).
- Static page text sits in an element with an `id` and no ids inside it; elements the code fills
  stay empty in the file. Then text edits swap in place too.

Dev only: `compose`, the watcher and `page_patch` live in the bridge and in `evani-live` (the
`live` feature). A shipped game has no meta, no marks and no patch code.

## 15. Look guard (bake check and Chris's approval)

Before any bake, the look's captures are compared with its approved baseline; anything outside
the look's limits is flagged, and a flagged bake can't be written without Chris's approval in
Evani.App. The AI may read, check and *propose*; only Chris approves. Code:
`tools/evani-lookguard` (library) and `tools/evani-live-bridge/src/lookguard.rs` (commands,
bake gate, approve listener). Dev only: none of it reaches a game's download (the look's build
script ignores `guard/`).

### 15.1 Overview

| Who | Can |
|---|---|
| The AI (MCP, `/ctl`, `/live/v1/cmd`) | `look.status`, `look.check`, `look.limits.propose`, `look.baseline.propose`, `bake.export` (refused when flagged). `look.approve*` is always `forbidden`. |
| Evani.App (Chris's click) | Signs the approval record and each approval with its own Ed25519 key and posts it to the approve listener (15.7). |

### 15.2 Commands

- `look.status {look?}` → `guard` (`unguarded`, `verified` or `failed` with `problem`),
  `verified` (`hash`, `seq`, `key_id`, `test_key`, `rollback`), `guard_hash`, `moments`, `tier`,
  `width`, `approvals` (as `/live/status` `look_guard`), `pending`, `last_bake` (the bake
  manifest), `key_warning`, `audit` (`entries`, `chain`).
- `look.check {look?}` → captures every moment (15.3), returns `passed`, `sentences` (one plain
  sentence per stat outside its limits), `breaches`, per-moment stats, `guard_hash`,
  `captures_hash`. Changes nothing. `no_limits` when the look has no guard set.
- `look.baseline.propose {look?, moments?, note}` → captures the moments, generates limits from
  them and records a *proposal* (`status: "pending"`, `approvalId`, `rows` old vs new). `moments`
  is needed the first time: `{"tier":"medium","width":320,"moments":[{"name":"start","capture":{"sim_time":2,"camera":"path"}}]}`.
- `look.limits.propose {look?, tolerances?, band_widen?, note}` → a proposal to change the limits
  only; each row is marked `looser` or tighter.
- Every proposal sends `look_guard.approval_requested` (15.7). One open request per look and
  kind; a new one replaces the old. Requests expire after 5 minutes.

`look` defaults to the game's look (`look = "basic"` in its `evani.toml` → `evani-look-basic`).

### 15.3 Captures

Capture version 1: for each moment, one `capture.frame` at the moments' tier and width (adaptive
quality off) with the look as it is, then one with bloom off (for bloom coverage); the page's
state is restored after each shot. One capture sequence at a time. Stats version 1 (on the
final sRGB capture): `mean_luma`, `log_avg`, `luma_std`, `luma_range`, `saturation`,
`colorfulness`, `clipped`, `crushed`, `bloom` (`tools/evani-lookguard/src/stats.rs`).

### 15.4 The guard set and its hash

`looks/<look>/guard/`: `moments.json`, `limits.json`, `baseline/<moment>.png` and
`baseline/<moment>.nobloom.png`. The guard hash: take those files (JSON with every CR byte
removed, PNGs as is), sort by relative path, build the manifest `evani-look-guard/1`,
`look=<id>`, `stat_version=1`, `capture_version=1`, then one `file <path> <sha256>` line each
(every line ending in LF); the hash is the SHA-256 of the manifest (64 lowercase hex). Changing a
limit, a baseline picture, a moment or the stat code changes the hash, so it needs approval.

### 15.5 The approval record (Evani.App's store)

`<store>/<project id>/<look>.json`:

```json
{ "format": "evani-look-approval/1", "projectId": "evani-dev", "projectPath": "C:\\dev\\evani-dev",
  "look": "evani-look-basic", "seq": 3, "approvedAt": "2026-10-14T15:00:00Z", "hash": "<guard hash>", "sig": "<128 hex>" }
```

`sig` is Ed25519 over the canonical text (not the JSON), one `key=value` line per field, each
ending in LF: `evani-look-approval/1`, `projectId=`, `projectPath=`, `look=`, `seq=`,
`approvedAt=`, `hash=`. The check: the record names this look and this project folder (compared
case-insensitively, `\` = `/`), the signature verifies with the pinned key, the hash equals the
guard hash on disk, and (when Evani.App's seq endpoint answers) `seq` is not older than
Evani.App's latest (no rollback).

### 15.6 Evani.App's folder and the stdin key line

```text
%APPDATA%\Evani.App\look-approvals\
  pubkey                       64 hex chars: the pinned Ed25519 public key
  endpoint.json                {"port": N} while Evani.App runs (seq endpoint, optional)
  <project id>\<look id>.json  the signed record of each look (15.5)
```

The engine only reads this folder. When Evani.App starts the live server it writes one line to
its stdin (live.mjs passes its stdin through to the bridge):

```json
{"approve_secret":"<at least 32 random bytes, hex>","approvals_pubkey":"<64 hex>","project_id":"<id>","approvals_endpoint":"http://127.0.0.1:<seq port>/"}
```

Approvals switch on only if `approvals_pubkey` is the pinned `pubkey`; otherwise (or without the
line) they stay off with a reason in `/live/status` `look_guard`. The secret stays in memory
only. Seq endpoint (optional): `GET /look-approvals/v1/seq?project=<id>&look=<look>` →
`{"project","look","seq": N | null}`.

### 15.7 Approval requests and the approve listener

`look_guard.approval_requested` (on `/ctl` and `/evani-dev`):
`{approvalId, kind: "limits"|"bake", look, hash, oldHash, expiry (unix ms), session, origin,
note, rows, looser, files, approvePort, approvals}`. Evani.App shows the card; on Approve:

1. For `limits`: write the signed record (15.5) with `hash` and the next `seq`.
2. `POST http://127.0.0.1:<approvePort>/look-guard/v1/approve`, no `Origin` header,
   `Authorization: Evani-Approve <hex HMAC-SHA256(secret, "<approvalId>|<hash>|<expiry>")>`, body
   `{approvalId, kind, look, hash, expiry, session, sig}` where `sig` is Ed25519 over
   `evani-look-approve/1`, `approvalId=`, `kind=`, `look=`, `hash=`, `expiry=`, `session=` (one
   LF-ended line each).

Answers: 200 `{ok, approvalId, kind, look, hash}` (limits also `written`, `removed`,
`guard_hash`, `verify`); 401 `bad_auth` (MAC) or `bad_signature`; 409 `wrong_session`,
`already_used`, `expired`, `unknown_approval`, `mismatch`, `stale`; 503 `approvals_disabled`;
403 when an `Origin` is sent; 421 for a foreign `Host`. On success: `look_guard.approved`.

### 15.8 Audit log

`.evani/look-guard/audit.jsonl`: every proposal, approval, refusal, check and bake, one JSON line
each, hash-chained (`look.status` reports `audit.chain`).

### 15.9 The bake gate

Every real `bake.export` (dry runs pass straight through):

1. The look must be `verified` (15.5); else `look_unapproved` (`data.reason`: `unguarded` or the
   check's code).
2. A one-time grant for this exact bake (guard hash + bake bundle) lets it through as
   `check: "approved"`.
3. Otherwise the moments are captured and checked. Inside the limits: written as
   `check: "passed"`. Outside: `look_flagged` with `data` `{approvalId, sentences, breaches,
   expiry, hash}`, nothing written, and a `kind: "bake"` approval request. After Chris approves,
   the same bake run again within 5 minutes goes through once.

A written bake adds `looks/<look>/bake-manifest.json` (guard hash, seq, key id, `test_key`,
check, approval id, hashes, files written), a line in the look's `CHANGES.md`, and
`result.look_guard` in the reply.

### 15.10 Test key and vectors

TEST ONLY: the Ed25519 seed is the ASCII text `EVANI-LOOK-GUARD-TEST-KEY-NEVER!`, public key
`5f5dc4101d95c54733f938ebc02f8e0cdfac2510075d72d33ca98a8861d00c69`. Anything it signs is marked
`test_key: true` and proves nothing. Vector: the record `projectId=evani-dev`,
`projectPath=C:\dev\evani-dev`, `look=evani-look-basic`, `seq=1`,
`approvedAt=2026-10-14T15:00:00Z`, `hash=0123456789abcdef` ×4 signs to
`ef215c59990c8046b84795d1bed89dc6f685c52e49c63e29192ac58e1c856412383b7bf82069be0037f263dcfc69a66048896cc152e841b54e4fed3029a1dc0a`;
with `seq=2` it must fail as `bad_signature`. The end-to-end proof
(`node tools/live-check/accept.mjs talk`) plays Evani.App with this key in a temporary folder.

## 16. Drop-to-convert (model commands, dev builds only)

A model dropped on the game in the Evani.dev tab (or chosen with the zone's **Convert a model**
button) goes to the bridge, is converted by evani-convert at below-normal priority, and is drawn
on the turntable without a reload: the original on the left and the converted tier on the right
of a live split. Additive to `evani-live/1`. Dev only: `--ship` drops `evani-live` and the
bridge entirely, so no part of this reaches a game's download.

Code: `tools/evani-live-bridge/src/convert.rs` (the job, the checks, the recorder),
`server.rs` (the two routes and the token), `crates/evani-live/src/convert.rs` (the split view,
the commands) and `convert_ui.rs` (the page's zone, button, tier switch and report panel,
compiled only with evani-live's `models` feature).

### 16.1 The two routes

- `POST /live/v1/convert?id=<id>`: the raw body is a `.glb`, a zipped `.gltf` (with its
  buffers) or a `.blend`. Answers at once with `{job, hash, id}`; the conversion runs on and its
  events follow (16.3). `id` is lowercase letters, digits, `-` and `_`, up to 80; without it the
  job is `model`.
- `GET /live/v1/convert/<hash>/<file>`: a finished job's file (a tier `.glb`, the report, the
  sheets). Plain relative names only: a name with a path, a drive or `..` is refused.

Every result lives under `.evani/convert/<hash>/` (`in/`, `src/`, `out/`), named by the content
hash and never by the uploaded name. One conversion at a time: a second upload answers `busy`
until the first ends. A conversion that runs past 30 minutes is stopped with everything it
started (cargo, evani-convert, Blender, the gate; a job object on Windows), and its log ends
"stopped after the run limit". When a job starts, older conversions are removed: the newest 8
stay unless older than 7 days, and the current and latest ones always stay.

### 16.2 The trust boundary

The upload carries no useful name: the folder is the content hash, the id comes only from the
checked `[a-z0-9_-]` set and the served names are checked as plain relative names. The body is
checked in this order, cheapest first, and each refusal says what to do:

| Check | Refusal |
|---|---|
| `Sec-Fetch-Site` other than absent, `same-origin` or `none` | `403 forbidden_origin` |
| An `Origin` other than the page's own (`http://localhost:<port>`, `http://127.0.0.1:<port>`) | `403 forbidden_origin` |
| The dev token (16.4) | `401 unauthorized` |
| Body over 256 MiB (`MAX_UPLOAD`) | `413 too_large` |
| First bytes not `glTF`, a zip or a `.blend` (plain, zstd, gzip) | `415 unsupported` |

Cookies ignore the port, so another page on `localhost` (another dev server) would send the
cookie too: its browser marks such a request `Sec-Fetch-Site: same-site`, which is refused like
`cross-site`. The GET route checks the same first three rows. Every answer on both routes,
refusals included, carries `X-Content-Type-Options: nosniff` and
`Cross-Origin-Resource-Policy: same-origin`, so another origin's page can't load a converted file
even by a plain `<img>` or `<script>` tag.

A zip entry is refused (`400 unsafe_zip`) if it is absolute, has a drive letter, any `:`, a
backslash, a NUL, an empty, `.` or `..` part, a Windows reserved name (`CON`, `NUL`, `COM1`…,
with or without an extension) or a part ending in a dot or a space; so is a `.gltf` URI that
points outside the zip. A `.blend` is opened with `--disable-autoexec`, so its own scripts never
run.

The page repeats the same three checks (name, size, first 16 bytes) before it sends anything, so
a wrong file is a sentence in the zone rather than a failed request; the bridge's checks are the
ones that count. A conversion runs in its own folder and never writes into the game's or the
engine's sources.

### 16.3 Events (`/ctl` and `/evani-dev`)

- `convert_progress {job, hash, id, stage, message}`: what the converter is doing; the zone
  shows `message` (else `stage`).
- `convert_done {job, hash, id, ok, report, files, base_url, log?}`: `ok` false carries the
  converter's `log` tail, which the zone shows.

The page listens through `evani.live.on("convert_progress" | "convert_done", fn)`; its state is
`window.__evaniConvert` (`message`, `busy`, `job`, `view`, `zone`) and an `evani-convert` event
on `window`, for the tab and for tests.

### 16.4 The token

The bridge sets its own cookie (`evani_live_<port>`, HttpOnly, SameSite=Strict, Path
`/live/v1/convert`) on the page it serves; the page's own posts carry it and no other page can
read it. A native client sends the session's dev token as `Authorization: Bearer` (the Blender
add-on's "Send to the running game" does this). The bridge also refuses a `Cross-Origin` request
that is not from the page's own origin.

### 16.5 Commands

- `model.convert {path, id?}` with a `note`: a local `.glb`, `.gltf` (converted where it is),
  `.zip` or `.blend`; answers `{job, hash, id}`, then the events. A change (it writes files and
  starts the converter): it is in the mutating list and MCP `model_convert` requires the `note`,
  which goes into the job's `job.json` and its first `convert_progress`.
- `model.load {hash?, tier?, compare?}`: draw the conversion on the turntable, the game paused
  (focus `live.model`, `focus_clear` goes back), the frame counter still running. `tier` defaults
  to the page's active tier, else the first converted tier; `compare` (boolean, default true)
  draws the original left and the converted tier right of the split; `false` draws the converted
  tier whole. Answers with `tiers`, `tier`, `report`. MCP `model_load`. (`job`, the job record,
  is internal: the bridge fills it in from `hash` before the command reaches the page; a caller
  never sends it.)
- `model.compare {at?, split?, tier?}`: `at` (a number from `0` to `1`) moves the line: `0` all
  converted, `1` all original; `split` (boolean) turns the split view on or off; `tier` switches
  the converted tier. Everything is checked before anything changes; answers the same view. Not
  a change (no note). MCP `model_compare`. The page's slider sends `at`, its Split box `split`;
  the slider's left end is labelled "converted", its right end "original".
- `model.report {hash?}`: the conversion's record (per tier its file, bytes raw and gzip, draw
  calls, triangles, gate scores; the package total; notes; the files and their URLs). MCP
  `model_report`.

A load that fails leaves the game as it was. The page draws no file itself and writes nothing.
