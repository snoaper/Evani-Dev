# Milestone 1 acceptance

Run 2026-10-14 on branch `m1-foundation` (code at `7dd8bab`), Windows 11, RTX 4060 Ti, headless
Chrome 155 with WebGPU. Builds and live checks ran in a separate worktree
(`..\evani-dev-accept`, detached at the commit under test) so another worker's edits in
the main folder could not interfere; the bridge ran there on port 8098. Evidence files are in
`.vibedeck/screenshots/m1-accept/` (local, not committed, like the stage 6 ones).
The checks are repeatable with `tools/live-check/accept.mjs` (modes `ai`, `safety`, `tune`,
`frames`, `shot`, `probe`, `diff`; see its header).

| # | DONE WHEN | Result | Evidence |
|---|---|---|---|
| 1 | Sandbox: lit 3D scene + sprite layer at a steady 60 fps | PASS | Live page telemetry: fps 60, frame 16.67 ms avg, 17 draws, GPU 0.11 to 1.1 ms (Ultra, 1280x720). Overlay of the shipped build: `fps 60, cpu 0.38 ms, gpu 1.12 ms, draws 15, tris 156k, sprites 300` (`ship-overlay.png`, `bake/tuned-session-f360.png`). |
| 2 | No WebGPU: clear message, not a blank page | PASS | Shipped build with `navigator.gpu` removed before any script: full-screen "This game needs WebGPU" with advice (`no-webgpu.png`); overlay says `render: webgpu unavailable: navigator.gpu missing`. |
| 3 | Tiers switch at runtime without reload; overlay shows tier, fps, CPU, per-pass GPU ms, draws, budget | PASS | Tab lab check "tier switch ultra → low" PASS with no reload; probe low/vr/medium switched live. Overlay text: `tier ultra budget 5/24 pts`, `passes forward 0.36 sprites 0.03 bloom.bright 0.10 bloom.blur 0.17 bloom.composite 0.17 tonemap 0.09 vignette 0.12 present 0.08`. |
| 4 | Bloom and Vignette toggle live; each tier's budget switches off what doesn't fit, by value per point | PASS | Lab check bloom and vignette toggles PASS. Live per tier: low = bloom off (preset), vignette on; vr = vignette off (VR comfort), bloom shed by adaptive quality (a 60 Hz headless display can't hold VR's 72 to 90 Hz target); medium = both on. Ordering by value per point is unit-tested (`lowest_value_per_point_goes_first`, `weights_change_the_order`, `style_vr_forced_and_overflow`). |
| 5 | On/off at both levels | PASS | `cargo xtask sizes` (all four variants built cleanly): full 329.4 KB gzip; no-bloom 326.4 (-3.0 KB, no bloom WGSL markers); no-fx 321.5 (-7.9 KB, no fx marker); no-live 221.4 (-108.0 KB, no live marker; wasm 461.6 KB raw). Run time: see 3 and 4. |
| 6 | (Replaced) Evani.dev tab connects; "Talk to the AI" changes Bloom, rewrites tonemap, bakes; each change visible within one frame | PASS headless, NEEDS CHRIS in the app | The tab's own `scripts/evanidev-lab-check.mjs` (branch `evani-dev-tab`, not merged) against the real bridge: 19/19 PASS (discovery from evani.toml, token, schema over /ctl, tier, bloom, vignette, param + clamp, broken slot kept, slot swap, outside change shown 22 ms after send with its note, undo, snapshot, capture, A/B, bake dry run, session restored). The AI path, through the bridge's MCP server on stdio exactly as Claude runs it (`accept.mjs ai`): `param_set` Bloom intensity, `effect_toggle` Bloom off and on, `slot_set` tonemap (ACES body, compiled in 118 ms) and `bake_export` dry run (plans `params.json`, `slots/tonemap.wgsl`, `CHANGES.md`). Each change's `session.mirror` reached a separate /ctl client 15.8 to 18 ms after the MCP call was sent (one frame is 16.7 ms), with the mirror's frame equal to the change's first rendered frame; the tonemap rewrite showed 134 ms after send (its 118 ms async compile, then swapped at the next frame). Captures `ai-*.png` show each change. **In-app check needs Chris after merging the tab:** open the Evani.dev tab with this folder, ask the AI to change Bloom, rewrite the tonemap and bake. |
| 7 | Safety: bad WGSL rejected with the old shader kept; over-budget change auto-reverts within 3 s; undo/redo across 20 changes | PASS | Broken body: `compile_error`, `line 1 col 17: unresolved value 'undefined_thing'`, `kept: previous`, same hash after. Heavy tonemap (16,200-iteration loop): frame time 34.5 ms vs 20.8 ms limit (+107 %), `auto_revert` event 3.21 s after the change applied, shader hash restored. (This "hash restored" result predates `f01c127`: before that fix undo, redo, auto-revert and restore put back the stored WGSL body and its hash but did not rebuild the GPU pipelines, so the heavy shader kept running. Since `f01c127` they rebuild the pipelines; the sandbox review below re-ran the undo path under load and work time fell back to 3.3 ms.) That is the spec's rule (over for 3 s, measured on a 0.5 s average), so the 0.21 s is the average crossing the line. Undo/redo: 20 param changes across 6 params, every undo and every redo returned the full param set and slot hash exactly to the recorded state (20/20 and 20/20, 21 distinct states). |
| 8 | compare.ab returns per-pass GPU ms and paired captures on the fixed path | PASS | Stage 6 (`.vibedeck/screenshots/stage6/summary.json`): bloom on vs off, ABBA on the bench path, per-pass deltas (bloom.blur -0.19, bloom.bright -0.10, bloom.composite -0.23 ms), paired PNGs, diff mean_abs 19.89, PSNR 18.2 dB. Re-run here inside the lab check (A/B snapshot vs now, 7 pass deltas, images). |
| 9 | bake.export writes the tuned session; a rebuild without live looks identical | PASS | Bloom tuned by comparing `capture.frame` shots at sim time 6 s (`tune/`, `tune-r2/`): threshold 1.5, knee 0.2, intensity 0.3, 4 levels (was 1.1 / 0.5 / 0.6 / 5, which hazed the whole image). Baked through the bridge: `looks/evani-look-basic/params.json` and `CHANGES.md` (committed `7dd8bab`). Rebuilt without live (`cargo xtask sizes`, no-live variant; no live marker in the wasm). Both pages opened with a stepped clock (exactly 1/60 s per frame) and shot at frame 360: tuned live session (the pre-bake live build with the four values set through `window.evani.live`) vs the shipped build: **0 of 921,600 pixels differ** (mean 0, max 0). Noise floor: the same page twice = 0. Sensitivity: untuned vs shipped differs by mean 12.7, PSNR 23.0 dB. No threshold was needed. |
| 10 | All tests pass; apps/bench prints pass or fail for every tier | PASS | `cargo test --workspace`: 280 passed, 0 failed, 1 ignored. `cargo xtask bench`: 5 of 5 PASS (low 0.46 ms avg, medium 0.97, high 2.45, ultra 9.46 at 2560x1440 with 5.4 M tris / 450k sprites, vr 0.63 mono). Also clean: `cargo check --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, wasm32 clippy of sandbox and bench with `--cfg=web_sys_unstable_apis`. |
| 11 | README and DECISIONS explain what a second developer needs for the first genre kit | PASS | README: architecture, evani.toml, how to add a kit, look, param, curve, effect, slot, baked files, live API, bench. DECISIONS: per-pillar enforcement and every choice. LIVE_API: every command with examples, MCP tools, tab compatibility. Fixed here: the XR on-demand download design added to DECISIONS (it only said "deferred"), two stale "bridge on branch m1-bridge" notes. |

## Fixed during acceptance

- **Captures degraded the picture they captured.** Several `capture.frame` calls in a row (held clock
  plus readback) made adaptive quality shed bloom, then vignette, while `fx.*.enabled` still said
  on; the sixth tuning capture had no bloom at all. Adaptive quality now ignores frames with a
  pending capture or a held clock and the second after (live builds only). New test
  `captures_never_degrade_quality` fails without the fix. Commit `29fa8ab`.
- **Commit `c3cc9d7` did not build on its own**: it carried another worker's `mod review;` line in
  `tools/xtask` before `review.rs` existed. Their next commit (`127d325`) completed it; HEAD builds.

## Sandbox review follow-up (2026-10-14, code at `7971507`)

Chris's review of the sandbox, re-checked headlessly in a separate worktree (`C:VibeAreaEvani.Dev-review`,
bridge on port 8092) with `accept.mjs quality` and `accept.mjs session`. Evidence in
`.vibedeck/screenshots/sandbox-review/` (local, not committed): `quality.json`, `live-session.json`,
`live-session.md`, PNGs named below.

| Note | Result | Evidence |
|---|---|---|
| 1. Medium and High hold 60 fps at vsync without cutting anything | PASS | 24 samples over 12 s each: fps 60 to 60, work 3.07 to 5.08 ms (Medium), 3.21 to 4.44 ms (High) of 16.7, no effect or scale cut (`medium-vsync.png`, `high-vsync.png`). |
| 2. Under load: effects pause first, then scale steps 0.05 to the 80 % floor | PASS | Deliberate tonemap load at High: work 26.7 ms; bloom paused at 2.5 s, vignette at 4.7 s, then scale 0.95, 0.90, 0.85, 0.80 and held (`load-00` to `load-06`). No auto-revert. |
| 3. Effects come back after the load (bloom used to stay paused) | PASS | Load undone: scale 0.95 then 1.0, vignette, then bloom restored, all in 21.8 s (unit test 20.9 s, documented limit 25 s); overlay log `1:27.5 adaptive quality: RestoreEffect("bloom") (work 3.3 of 16.7 ms)` (`after-load-recovered.png`). Cause and fix: DECISIONS.md, "Adaptive quality judges a percentile". |
| 4. bloom.composite at High | PASS | 16 overlay samples over 6 s with bloom on at High: average 0.10 ms (min 0.02, max 0.38), against 0.92 ms in the review, which was measured while the GPU was shared with other work. `telemetry_sample` at Ultra in the session: 0.025 ms. |
| 5. MSAA from Medium | PASS | Overlay `msaa 4x` on all 24 Medium samples; Low draws one sample (`medium-msaa-zoom.png`, `low-no-msaa-zoom.png`). |
| 6. Command counter, download label, log times, no VR in the list | PASS | Counter 0 -> 10 commands (`overlay-commands.png`); `download 5.63 MB (dev build; published ~232.4 KB gzip)`; every log line starts with `m:ss.s`; the tier list opened in place shows Low, Medium, High, Ultra only (`tier-list-no-vr.png`). |
| Hitches ignored | PASS (unit) | `one_off_hitches_never_cut_quality`, `disturbances_and_their_grace_are_not_judged`, `disturbed_frames_and_the_grace_after_are_not_judged`, `a_blocked_step_is_tried_with_growing_waits`, `everything_comes_back_after_a_load_spike` (fails on the old code with bloom stuck). |
| Recorded live session (Stage 0 gate) | PASS | Through the bridge's MCP server on stdio: Bloom intensity 0.3 -> 0.4 (1.26 s with capture), tonemap rewritten (hash `b22f1223` -> `70ae6de3`), a broken patch rejected in 50 ms with `unresolved value 'undefined_thing'`, `kept: previous`, same hash, 219 page frames in the next 3.6 s (60 fps); bake dry run planned `params.json`, `slots/tonemap.wgsl`, `CHANGES.md` and wrote nothing; the real bake wrote them. Overlay command count after each command: 1, 2, 3 ... 13. Ship build (live off) rebuilt from the baked sources and compared at frame 360 with a stepped clock against the live session replayed on the pre-bake live build: **0 of 921,600 pixels differ**; sensitivity: the unchanged live build differs from it by mean 38.2. The baked look (ACES at 0.9 exposure) washes the scene out, so it was not kept: the worktree's look files were restored (`live-session-bake.diff`, `live-session-baked-tonemap.wgsl`). |

Download sizes (`cargo xtask size-report`, ship build, live off; `size-report.txt`):

| Game | raw | gzip | brotli | wasm-opt |
|---|---|---|---|---|
| sandbox | 587.9 KB | 232.4 KB | 194.2 KB | -5.7 % |
| kart-racer | 719.1 KB | 283.0 KB | 235.8 KB | -5.4 % |

Up from 228.7 / 191.1 (sandbox) and 279.6 / 233.1 (kart-racer) gzip / brotli: the percentile window, disturbance tracking and overlay times.

Also run: `cargo xtask bench` 5 of 5 PASS (low 0.52 ms avg, medium 1.07, high 2.72, ultra 10.22, vr 0.75); `cargo clippy --workspace --all-targets -- -D warnings` clean; wasm32 clippy of sandbox, bench and kart-racer with `--cfg=web_sys_unstable_apis` clean; `cargo test --workspace` 357 passed, 0 failed, 1 ignored.

## Gate 0 questions (2026-10-14, code at `2f2a090`)

Chris's three Gate 0 questions, checked in the main folder (bridge on port 8092, one headless Chrome 155, RTX 4060 Ti). Evidence in `.vibedeck/screenshots/gate0/` (local, not committed): `recovery-log.txt`, `quality.json`, `quality-run.txt`, `gate0-probe.json`, `bench-runs3*.txt`, PNGs named below.

| Question | Result | Evidence |
|---|---|---|
| A. Full quality back within 3 to 5 s once work is under 50 % of the target | PASS | Unit: `fast_recovery_within_5_s_when_work_is_under_half_the_target` (1.4 to 1.6 s at 30, 60 and 144 Hz), `everything_comes_back_after_a_load_spike` now asserts 5 s. Live (`accept.mjs quality`, 14 of 14 PASS): a tonemap load at High paused bloom (2.5 s), vignette (4.5 s), then scale 0.95 to 0.80 (12.5 s); after the undo, scale 1.0, vignette and bloom were all back 2.8 s later (that includes the 1 s grace after the undo, a live change), overlay `1:08.3 adaptive quality: Scale(1.0) / RestoreEffect("vignette") / RestoreEffect("bloom") (work 1.5 of 16.7 ms)` in the same 0.1 s (`recovery-log.txt`, `load-00` to `load-06`, `after-load-recovered.png`). Hysteresis, percentile, grace and hitch tests unchanged and passing. |
| B. "entities 0": does the ECS lack what a kit needs? | Partly; fixed additively | The kit could have used `kit.component`, `kit.system` and `World::query` (the example kit does), but a race needed pairs, an entity list, an explicit order and a reset. Added `World::get2_mut`, `entities_with`, `sorted_entities`, `count`, `despawn_all`, `SparseSet::get2_mut`; test `kit_objects_pairs_order_and_reset`. README "How a kit puts gameplay objects in the ECS". Kart migration is the kart worker's (Stage 2). |
| C. GPU cost rises Low < Medium < High < Ultra; a check that fails otherwise | PASS | Bench, mean of 3 runs (table below), order check holds on the medians. Unit: `gpu_cost_order_holds_within_the_tolerance`, `the_order_check_judges_medians_not_spiky_means`, `runs_are_averaged_tier_by_tier`, `settle_waits_for_pipelines_then_caps`. |

GPU ms, `cargo xtask bench --runs 3`, mean of 3 runs (3D phase passes; uncapped, card clocked up):

| Tier | Size | GPU ms (mean) | median | forward | shadows | outline | bloom (3 passes) | tonemap | vignette | present |
|---|---|---|---|---|---|---|---|---|---|---|
| Low | 1440x810, MSAA 1 | 0.18 | 0.18 | 0.12 | - | 0.03 | - | 0.01 | 0.01 | 0.01 |
| Medium | 1920x1080, MSAA 4 | 0.62 | 0.59 | 0.36 | 0.06 | 0.05 | 0.08 | 0.02 | 0.02 | 0.02 |
| High | 1920x1080, MSAA 4 | 1.02 | 0.99 | 0.66 | 0.16 | 0.05 | 0.10 | 0.02 | 0.02 | 0.02 |
| Ultra | 2560x1440, MSAA 4 | 3.12 | 2.98 | 1.72 | 0.50 | 0.10 | 0.15 | 0.05 | 0.05 | 0.04 |

Before the blob early-outs Low was 0.25 ms (forward 0.18). Per-run spread: Low 0.18 to 0.19, Medium 0.60 to 0.64, High 0.99 to 1.08, Ultra 3.12 to 3.13.

What MSAA and effects cost at High (mean of 3 runs each, before the blob change, which does not touch High): MSAA 1 forward 0.32 ms vs MSAA 4 0.65 ms; bloom off 0.93 ms total vs 1.04 on; outline off forward 0.54 and no outline pass vs 0.65 + 0.05. A 600-frame warm-up instead of 120 changed nothing at High (0.99 ms).

Why the game's overlay showed Low dearer than High, and the forward pass at 1.76 ms instead of 0.32: clock state. Live sandbox at vsync (`gate0-probe.json`, nvidia-smi sampled with every overlay reading): the card sat at 210 to 375 MHz on every tier, and at High the forward pass read 1.75 ms; the same scene with an extra tonemap load holding the card at 2775 MHz read forward 0.08 ms. Low's tonemap read 0.96 ms at vsync (one texture read a pixel; 0.01 ms in the bench). Per-pass numbers from a vsync-capped overlay are not costs; compare tiers with the bench.

Tier list: the panel's tier control is a closed drop-down with exactly 4 options, Low, Medium, High, Ultra, and no `size` attribute (`gate0-probe.json`: `options` 4, `size` null). The scrollbar in `tier-list-no-vr.png` comes from the check itself: to photograph every option it opens the list in place by setting `size` to 4, and Chrome draws list boxes with an always-on vertical scrollbar. Here it has no thumb, because the content fits (height 68 px = 4 x 17 px rows, scroll height 68). Players never see it.

Also run: `cargo clippy -D warnings` native (sandbox, bench, kart-racer, xtask, core, render, all targets) and wasm32 (sandbox, bench, kart-racer, `--cfg=web_sys_unstable_apis`) clean; `cargo test --workspace` 380 passed, 0 failed, 1 ignored.

## Gate 0 follow-up: steady timings (2026-10-14, branch `gate0-timing`)

Chris's follow-up: confirm no oscillation after a multi-step restore, measure the tier order and bloom.composite on a stress scene that keeps the GPU clocked up, and show the overlay's pass timings as a 2 s median. Evidence in `.vibedeck/screenshots/gate0/` (local, not committed): `stress-table.txt`, `stress-summary.json`, `stress-gpu-clock.csv`, `stress-run.log`, the same four for `stress-off-bloom-*`, and `overlay-median.png`.

| Item | Result | Evidence |
|---|---|---|
| A. No flapping after a fast restore of several steps | PASS (one fix) | `multi_step_fast_restore_then_steady_moderate_load_never_flaps` (7 steps back on consecutive frames, then 43 s+ unchanged at 63 % and 75 % of the target, 30/60/144 Hz); `a_fast_restore_that_only_partly_fits_cuts_once_then_settles` failed before the fix (bloom restored and cut again at about 24 s) and passes after. DECISIONS: "A step cut again soon after a restore is confirmed". |
| B. Tier order and bloom on the stress scene, GPU clocked up | PASS | `cargo xtask bench --stress`: 5 runs of Low to Ultra, 600 GPU readings per tier per run, median of run medians. Order holds. Clock while busy 2760 to 2790 MHz on every tier (694 of 721 samples; Low at about 60 % use held 2760 to 2790). |
| C. Overlay GPU timings as a 2 s median | PASS | `overlay-median.png` (sandbox on bridge port 8092, headless Chrome): `gpu 2.88 ms (median)` and `passes forward 1.84 ... present 0.06 (2 s median)`. Unit: `gpu_median_covers_the_last_two_seconds_at_any_rate`, `gpu_median_ignores_a_spike_and_follows_a_lasting_change`, `gpu_median_handles_changing_pass_sets`, `gpu_median_takes_each_reading_once`, `gpu_median_never_allocates_after_start`, `stats_show_the_median`, `overlay_shows_the_two_second_gpu_median`. Telemetry stays raw. |

Stress scene, median GPU ms per frame, median of 5 runs (lowest to highest run median). "Without fill" is the total minus the 500 fill layers drawn by the sprite pass, i.e. what the tier settings cost:

| Tier | Render | Total | Without fill | forward | shadows | outline | bloom.bright | bloom.blur | bloom.composite | tonemap | vignette |
|---|---|---|---|---|---|---|---|---|---|---|---|
| Low | 1440x810, MSAA 1 | 10.33 (10.27-10.59) | 0.215 | 0.147 | - | 0.027 | - | - | - | 0.013 | 0.014 |
| Medium | 1920x1080, MSAA 4 | 18.38 (18.31-18.49) | 0.790 | 0.549 | 0.059 | 0.046 | 0.015 | 0.033 | 0.035 (0.035-0.035) | 0.019 | 0.025 |
| High | 1920x1080, MSAA 4 | 18.47 (18.43-18.52) | 0.868 | 0.572 | 0.086 | 0.047 | 0.015 | 0.033 | 0.035 (0.035-0.035) | 0.019 | 0.025 |
| Ultra | 2560x1440, MSAA 4 | 27.62 (27.43-28.04) | 1.223 | 0.600 (0.585-0.769) | 0.159 (0.158-0.371) | 0.082 | 0.030 | 0.046 | 0.051 (0.050-0.051) | 0.033 | 0.044 |

Bloom at High, 5 runs each: on 18.47 ms total, 0.868 without fill (bloom passes 0.015 + 0.033 + 0.035); off 18.38 ms, 0.755 without fill. So bloom costs about 0.11 ms at High at 1920x1080, and bloom.composite reads 0.035 ms on every run (against 0.17 and 0.70 ms in the earlier captures, which ran with the card clocked down).

## Not covered headlessly

- The in-app DONE 6 run (needs the Evani.dev tab merged into Evani.App, which Chris decides).
- DONE 1 on "a normal laptop": measured on this desktop GPU only.
