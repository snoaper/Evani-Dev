# Smoothness: frame pacing and motion steadiness

`cargo xtask smoothness <id>` measures how evenly a game moves on screen. It runs a built game in headless, muted Chrome on the graphics card at a fixed size (1280 px by default), drives it with synthetic keys for a set time, and records two things on every displayed frame:

- **Motion steadiness.** How far a tracked thing (the player's car, a character, the camera) moved since the previous frame, as a speed, divided by the average speed of its neighbouring frames (seven each side, within 140 ms). Steady motion stays at 1. Judder shows as frames near 0 (it stood still) and frames well above 1 (it caught up). Because each frame is compared with its neighbours, a game that is simply faster or slower overall does not count as judder.
- **Frame pacing.** The time between presented frames against the 60 Hz target (16.7 ms; 8.3 ms on a 120 Hz display).

It writes a JSON result and an HTML report with both charts and a table view, and exits non-zero when the build is over the limits. Given two builds it overlays them as **Before** and **After**.

It is a tool to run when motion may stutter (an online game, a new camera, an interpolation change), not an automatic gate. Nothing in it reaches a shipped game: see "What ships" below.

The method comes from APEX Velocity's stutter fix (2026-11-11), where it showed half of all frames juddering before the fix and none after.

## Running it

```sh
cargo xtask smoothness kart-racer                       # build the dev build, run 20 s, judge it
cargo xtask smoothness kart-racer --net wan             # online: a local host behind a jittery relay
cargo xtask smoothness kart-racer --baseline <folder>   # Before (that build) and After (this tree)
cargo xtask smoothness --app-dir <game app folder>      # a game outside the engine folder
```

| Option | What |
|---|---|
| `--baseline <folder>` | A built game folder to compare against (Before). Keep a copy of `target/evani-sizes/<id>-full` before your change, or any earlier dev build. |
| `--candidate <folder>` | The build to judge (After). Default: this tree's dev build (`target/evani-sizes/<id>-full`, built first). |
| `--no-build` | Use the existing dev build and referee if they are there. |
| `--net none\|lan\|wan\|bad` | The networked mode's relay profile (below). Default `none`. |
| `--out <folder>` | Where the result goes. Default `.evani/smoothness/<id>/`. |
| `--config <file>` | Default: `smoothness.json` beside the game's `evani.toml`. |
| `--seed n`, `--duration s`, `--width px`, `--height px` | Relay seed, run length, page size. |
| `--title`, `--baseline-label`, `--candidate-label` | The report's title and legend. |
| `--limit key=value` | Override a limit (below); repeatable. |
| `--no-fail` | Always exit 0 (the result still says pass or fail). |

The node runner can also be called directly: `node tools/smoothness/smoothness.mjs --config <file> --build <folder> [--baseline <folder>] ...` (`tools/smoothness/README.md`).

Output in the out folder: `result.json` (format `evani-smoothness/1`: summaries, limits, verdicts, notes), `report.html`, `trace-before.json` / `trace-after.json` (the raw frames) and a screenshot of each run's last frame.

Exit codes: 0 within the limits, 1 over a limit (the candidate's; the baseline is only shown), 2 the run could not be made.

## The game's config (`smoothness.json`)

The engine knows nothing about karts or players: the game says what to track and which keys to press.

```json
{
  "game": "kart-racer",
  "probe": { "kind": "camera_target" },
  "metric": "world",
  "min_speed": 4,
  "width": 1280, "height": 720,
  "duration_s": 20, "warmup_s": 5,
  "ready": "document.querySelector('#hud0') !== null",
  "start": [{ "at": 0, "click": "[data-cmd=start1]" }],
  "script": [{ "at": 0, "down": ["KeyW"] }, { "at": 1.2, "down": ["KeyA"] }, { "at": 1.6, "up": ["KeyA"] }],
  "repeat_s": 4,
  "thresholds": { "hitches_max": 3 },
  "net": { "referee_package": "evani-kart-racer-referee", "host_args": ["--laps", "3"], "ready": "...", "start": [] }
}
```

| Key | What |
|---|---|
| `probe` | What to track (live command `motion.trace`, docs/LIVE_API.md 6.32). `camera_target` / `camera_eye`: the main camera (a chase camera's target is the followed thing). `mesh` with `mesh` (id) and/or `material` (look material index), optional `near` `[x,y,z]`: the matching mesh instance nearest the camera target. `sprite` with `cell` and/or `layer`, optional `index`. |
| `metric` | `world` (metres; for a camera that follows the probe) or `screen` (CSS px on the canvas; for a fixed camera or a 2D game). |
| `min_speed` | Frames whose neighbourhood moves slower than this (units a second) are not judged: a parked car has no steadiness. |
| `ready` | A page expression that is true once the game can be driven (besides the engine's first frame). |
| `start` | Steps before the measured run (menus): `{ at, down, up, click }`, `at` in seconds, keys by `KeyboardEvent.code` (`KeyW`, `ArrowLeft`, `Space`, `ShiftLeft`, `Enter`...), `click` a CSS selector. |
| `warmup_s` | Wait after `start` before measuring. |
| `script`, `repeat_s` | The keys during the measured run, looped every `repeat_s` seconds. Keys go in as browser input events (CDP), never the real keyboard. |
| `chart_start_s`, `chart_window_s` | The steadiness chart's window (default 2 s in, 6 s long). |
| `thresholds` | This game's limits (over the defaults). |
| `net` | The networked mode: `referee_package` (the game's headless referee crate, `evani-referee/1`), `host_args` for `tools/evani-host/server.mjs`, `server_param` (default `evani-server`), and online `ready` / `start` (an online game may join its host instead of showing its menus). |

## Limits

The defaults come from the APEX Velocity fixed build's four runs (1280 and 2560 px, an empty and a busy server, relay delay and jitter), re-analysed with this harness, plus a little headroom (DECISIONS.md "Smoothness harness"):

| Limit | Default | APEX fixed, worst of 4 |
|---|---|---|
| `uneven_pct_max`: frames with a ratio outside 0.6 to 1.4, % | 0.1 | 0.038 |
| `ratio_dev_p99_max`: 99th percentile of the ratio's distance from 1 | 0.06 | 0.042 |
| `ratio_sd_max`: the ratio's standard deviation | 0.04 | 0.024 |
| `frame_max_ms`: the longest time between presented frames | 150 | 116.6 |
| `hitches_max`: presented frames more than twice the target apart | 2 | 1 |

A run with fewer than 60 moving frames fails: nothing was measured (is the probe moving, is `min_speed` too high?).

## Networked mode

`--net <profile>` starts the game's own referee in the engine's reference host (`tools/evani-host/server.mjs`, the local authoritative host stand-in) and puts a relay between it and the page (`tools/smoothness/relay.mjs`). The relay holds back what the host sends by a delay plus jitter, in order, the way a real relay delivers snapshots unevenly; the page joins as a player through `?evani-server=`. Profiles (ms): `lan` 2 + 1 jitter; `wan` 22 + exponential jitter (mean 6) + a 70 ms hiccup about one chunk in 90 (APEX's model); `bad` 60 + 25 jitter + a 200 ms hiccup one in 40, and 30 ms on the way up. The delays are seeded (`--seed`).

This measures what a player sees: whether the game's drawing hides the uneven delivery. The engine's answer for that is the net sync layer (docs/NET_SYNC.md: prediction, error smoothing, snapshot interpolation); this harness is how to check a game uses it well.

## What ships

Nothing. The probe is the live module's `motion.trace` command (crates/evani-live/src/motion.rs), and `live` is a dev-only module that every `--ship` build drops. The runner, relay and report are node tools under `tools/`. A ship build of a game is byte for byte the same with and without this harness (DECISIONS.md "Smoothness harness" has the proof).

## Limits of the method

- The probe reads `RenderWorld`: models drawn through the `models` module's own list are not probes yet; use the camera target or a mesh drawn with them.
- Synthetic keys arrive through the browser, not the OS: they test the game, not a real keyboard's timing.
- Headless Chrome presents at the display rate it is given (60 Hz here); a real monitor's compositor is not measured.
