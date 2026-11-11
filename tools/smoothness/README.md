# tools/smoothness

The smoothness harness: frame pacing and motion steadiness of a built Evani.Dev game, before and after a change. Node 22+, no dependencies; a dev and test tool, never part of a game's download. Full docs: [docs/SMOOTHNESS.md](../../docs/SMOOTHNESS.md).

```sh
cargo xtask smoothness <id> [--baseline <build folder>] [--net wan]       # usual way (builds first)
node tools/smoothness/smoothness.mjs --config apps/<id>/smoothness.json --build <folder> [--baseline <folder>] [--net wan] [--out <folder>]
node --test tools/smoothness/smoothness.test.mjs                           # the harness's own tests
```

| File | What |
|---|---|
| `smoothness.mjs` | The command line: runs each build, judges it, writes `result.json` and `report.html`. |
| `run.mjs` | One run: serves the build, starts the host and relay (networked mode), headless muted Chrome through `../bench-runner/chrome.mjs` (GPU lock, throwaway profile), synthetic CDP keys, the live API's `motion.trace` / `motion.read`, the presented-frame hook from `../live-check/frame-gaps.mjs`. |
| `analyze.mjs` | Pure analysis: per-frame speeds, neighbour ratios, frame intervals, summaries, limits. |
| `report.mjs` | The HTML report: steadiness chart, frame-time chart, table view; Before and After overlaid. |
| `relay.mjs` | The delay-and-jitter TCP relay in front of `../evani-host/server.mjs`. |
| `smoothness.test.mjs` | Tests (no browser). `cargo test -p xtask` runs them too. |

Ported from APEX Velocity's stutter harness (its `.evani/tmp/stutter/`: host-sim.mjs, analyze.mjs, chart.mjs, inject.js), made genre-agnostic.
