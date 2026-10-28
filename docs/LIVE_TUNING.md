# Tuning live: sweeps and batches

How an AI (or a script) should tune a running Evani.Dev game through the live API, and what it
costs. The commands are in docs/LIVE_API.md: `sweep.run` (6.29), `batch` (6.25), `param.set`
(6.3); over MCP `sweep_run`, `batch` and `param_set` (11).

## The rule

1. **Choose with one sweep.** Put the candidate values (one parameter, or a small grid of two or
   three) in one `sweep_run`. It flies the bench path once per row and returns one table: costs
   (median work, CPU, GPU and per-pass GPU ms) and the look guard's measures (brightness,
   contrast, saturation, colourfulness, clipped highlights, crushed shadows, bloom coverage). Row 0
   is the current state, so every limit can be read against it. Nothing is kept.
2. **Apply with one command.** Apply the chosen values in one `param_set` with several names, or
   one `batch` when slots, curves or the tier change too. One note, one undo step.
3. **Look only where it matters.** Ask for `thumbs` when the numbers aren't enough; they come back
   as file paths (open the one or two rows that matter), not as images in every reply.

Don't tune with one `param_set` + `capture_frame` + `telemetry_sample` round per try: every try
costs three commands and a full screenshot, writes a history entry, and measures the free camera
rather than the fixed path, so tries aren't comparable.

## Measured: one tuning task, both ways

Task: *find a bloom intensity and threshold that keep clipped highlights within the look's limit at
the lowest cost.* Grid: intensity 0.3, 0.6, 1.0, 1.5 × threshold 0.8, 1.2, 1.6 (12 tries). Limit:
the look guard's tolerance for `clipped` (stats v1: at most 1 point above the current look). Cost:
GPU ms. Kart racer, headless Chrome (default flags), RTX 4060 Ti shared with other jobs,
2026-10-14. Script: `tools/live-check/sweep.mjs`; raw numbers in
`.vibedeck/screenshots/sweep/tuning-cost.json`.

| | Old way: per try `param_set` + `capture_frame` (320 wide) + `telemetry_sample` (2 s), then apply | New way: one `sweep_run` (with 160-wide thumbnails as files) + one `batch` |
|---|---|---|
| MCP commands | **37** (13 `param_set`, 12 `capture_frame`, 12 `telemetry_sample`) | **2** (1 `sweep_run`, 1 `batch`) |
| Bytes through MCP (requests + replies) | 1,094,988 (1,025,564 of it image base64) | 11,542 |
| Tokens ≈ bytes / 4 | **≈ 273,700** | **≈ 2,900** |
| Tokens with images counted as Claude bills them (w × h / 750) | ≈ 18,300 (text ≈ 17,400 + 12 images ≈ 920) | ≈ 2,900 |
| Wall time | 40 s | 36 s |
| History entries left | 13 | 1 |
| Look numbers | none: the AI has to judge 12 screenshots by eye (the script decoded them to compare) | 9 measures per row, same formulas as the look guard |

So the new way takes about 18× fewer commands and between 6× (images billed as images) and 95×
(raw bytes / 4) fewer tokens, in about the same time, and leaves one undo step instead of
thirteen. Both found 4 settings within the limit. The old way's costs come from 2 s of free play
per try, so they don't compare with each other well; the sweep measures every row on the same
path frames.

The sweep's table (13 rows: the current state, then the grid), shortened to the columns the task
reads; `table.md` next to the JSON has every pass:

| intensity | threshold | gpu_ms | gpu.bloom.* (bright + blur + composite) | mean_luma | clipped | bloom |
|---|---|---|---|---|---|---|
| 0.6 | 2.0 | 1.794 | 0.245 | 0.8333 | 0.1474 | 0.0086 |
| 0.3 | 0.8 | 2.232 | 0.308 | 0.8780 | 0.5077 | 1.0 |
| 0.3 | 1.6 | 2.939 | 0.318 | 0.8341 | 0.1477 | 0.0107 |
| 0.6 | 1.2 | 1.469 | 0.210 | 0.8554 | 0.2654 | 0.8126 |
| 0.6 | 1.6 | 1.861 | 0.256 | 0.8351 | 0.1481 | 0.0376 |
| 1.0 | 1.6 | 2.668 | 0.362 | 0.8365 | 0.1490 | 0.0838 |
| 1.5 | 1.6 | 1.996 | 0.276 | 0.8381 | 0.1502 | 0.1346 |

What it shows: thresholds below 1.6 blow out the kart's bright sky (`clipped` 16–84 %, bloom
covering 39–100 % of the frame), so only threshold 1.6 and up stays within the limit. The GPU column
moves by ±30 % between rows while every pass moves together: that is the shared GPU's clock and
load, not the setting. On a busy machine use more `frames_per_value`, or sweep the two finalists
again, before trusting sub-millisecond differences.

## Tips

- One sweep is at most 32 rows. Start coarse (4–6 values), then sweep again around the best.
- `look_at: []` skips the captures when only cost matters; `bloom: false` skips the bloom-off
  frames when bloom coverage doesn't matter. Both make the sweep faster.
- A sweep holds the game like `bench.run`: changes answer `busy` with `until_ms` until it ends.
- After choosing, `compare_ab` the chosen set against the current state if the change is close.
