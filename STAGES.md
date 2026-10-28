# Evani.Dev stages

Evani.Dev is built in stages. A stage opens only when the stage before it is delivering: every gate check passes, and Chris has signed it off after using the result himself.

Current stage: 3 (Real content). Gate 0 was signed off by Chris on 2026-10-14, Gate 1 on 2026-10-15, Gate 2 on 2026-10-16 (its last line, the real-device bench, was ticked the same day after Chris answered "Tick it"; see the Stage 2 gate and the gate log). Chris opened Stage 3 on 2026-10-16 ("so get moving on finishing stage 3 and 4").

Guiding principle (Chris, 2026-10-20): the engine stays light. Small downloads and low bandwidth come first, so games can have bigger worlds; every module and feature is weighed by the bytes it adds.

## Rules
1. Work only inside the current stage. The next stage may be researched and planned (documents, or throwaway spikes on a scratch branch), never built into main.
2. Work already in flight may finish, but it is reviewed only when the stage it belongs to is open.
3. Two-step gates. QQ runs the machine checks first. Chris then signs off the human checks after using the result. Record every passed gate in the gate log, with the date and evidence.
4. Regressions reopen gates. If a later stage breaks something an earlier gate proved, that gate is reopened, and it is fixed before work continues.
5. New ideas go to the Parking lot, or to the stage they belong to, never straight into code.
6. Ongoing streams. Some stages start background work that keeps running after their gate (marked "Ongoing"). It continues alongside later stages, but never changes evani-core or evani-render.
7. Kits follow the stages. Kit work in ROADMAP.md happens only in the stage that allows it.
8. Every Next step is reviewed by an agent before it starts and after it finishes (see "Next step reviews").
9. No game polish until Stage 8 (Chris, 2026-10-16: "Lets stop fixing games until stage 8 and do minimum to prove the stage passed"). Effort goes to each stage's engine work. Game work is limited to the minimum that proves a gate check passes, plus fixing a regression that would reopen a passed gate (rule 4). Critic findings and polish ideas for existing games are recorded in the Parking lot and not built. Kit review and critic rounds run only where a gate check needs them, and then once.

## Next step reviews
- Before a Next step starts, an agent checks: it belongs to the current stage; its prerequisites are met; nothing in it is already done or made obsolete by the code; its description and "done when" still match reality. The agent reports to Chris and proposes edits. Work starts only after Chris approves.
- After a Next step finishes, QQ checks its "done when" items and the relevant gate checks with evidence. Failures go back to the worker with the evidence. Passes go to Chris.
- Only Chris signs off gates.

## Stage 0: Foundation
Project: Evani.Dev engine
Includes:
- docs/FOUNDATION_PROMPT.md (milestone 1).
- Adaptive-quality fixes: measure frame work, not the screen refresh; cut effects first by lowest value per point; then lower render scale in 0.05 steps, never below 80% of the tier's scale.
- VR tier only inside an immersive WebXR session; Enter VR shown only where supported.
- An effect the controller paused is shown as "on, paused to hold frame rate".
- MSAA 4x on the forward path from Medium up.
- docs/LIVE_API.md, with an API version reported in the first message.
Gate:
- [x] All foundation "done when" checks pass, except the Evani.dev tab part of check 6, which moves to Gate 1.
- [x] An AI agent connected through the MCP bridge changes Bloom, rewrites the tonemap slot (a broken patch is rejected while the old shader keeps running), and bakes.
- [x] With the GPU nearly idle, the controller never degrades quality. Under real load, it follows the cut order and limits above (unit-tested).
- [x] The published sandbox size is reported: release build, gzipped, without evani-live.

## Stage 1: Harness link
Project: Evani.App
Includes:
- "Connect the Evani.dev tab to the engine", with the version handshake.
- Evani.App rebuilt and installed with the new tab.
- "Kit review card".
Gate:
- [x] Opening the sandbox project connects the Evani.dev tab on its own, with no Try again.
  - Machine evidence (2026-10-15): passes in a hidden test copy of Evani.App against the real live server, also after a restart. .vibedeck/screenshots/gate1/tab-connect.md. Chris ran it in the installed app on 2026-10-15.
- [x] From "Talk to the AI" in the tab: change Bloom, rewrite the tonemap slot, A/B compare two snapshots, and bake. A rebuild without evani-live looks identical.
- [x] A version mismatch between tab and engine shows a clear "update" message.
  - Machine evidence (2026-10-15): a newer and an older engine version both show the update message, against the real live server behind a version-changing proxy. .vibedeck/screenshots/gate1/tab-connect.md
- [x] Look guard: before any bake, captures are compared with the look's baseline (clipped highlights, contrast, saturation, average brightness); anything outside the look's limits is flagged, and a flagged result can't be baked without Chris's approval.
- [x] The sandbox, submitted as a test kit, produces a complete review card. A deliberately broken kit is sent back by QQ and never reaches Chris.

## Stage 2: First kits
Project: Evani.Dev kits
Includes:
- Kits 001 to 003 in ROADMAP.md: kart racer (polish pass, then a re-run of bench and live tuning on the fixed controller), 2D platformer, match-3.
- Free roam in each of those kits before its game becomes a playground (Chris's rule, DECISIONS.md 2026-10-15; docs/PLAYGROUNDS.md): the same world with no goals, timer, lives or game over under `PlayMode::FreeRoam`, a passing free-roam test, then `[playground] free_roam = true`. The engine side (play mode, check, ship guard, Done-when 29) was built in Stage 1.
- docs/KIT_PROCESS.md, including the visual quality bar and the placeholder bar, the target pack, two build passes, scripted captures, automated checks, an independent critic and the lessons loop (genre playbooks in docs/playbooks/).
- Geometry integrity: tools/evani-geocheck (dev-time, never shipped): static overlaps, floating objects, gaps and cracks, z-fighting, mesh validation, moving clipping, collision mismatch, camera clipping; intentional cases tagged in level data with a reason. Engine prevention (snap-to-ground, footprint-aware placement, spline meshes, decal pass or depth offset) and a geometry debug view in the Evani.dev tab through LIVE_API.
Gate:
- [x] All three kits are Verified by Chris after playing them.
  - Signed off by Chris, 2026-10-16: "stage 2 looks good".
  - Evidence note (2026-10-16): Fresh packages built from m1-foundation 5becff2 are in publish/kart-racer, publish/2d-platformer and publish/match-3 (not published). Each boots to its title screen in muted headless Chrome with no errors: .vibedeck/screenshots/gate2/boot-kart-racer.png, boot-2d-platformer.png, boot-match-3.png. Status per line: .vibedeck/screenshots/gate2/gate2-status.md.
- [x] Each passes bench on Low, Medium, High and Ultra, and VR is reported correctly.
  - Evidence note (2026-10-16): machine check passes on desktop. `cargo xtask bench` on the RTX 4060 Ti, 6 of 6 tiers PASS (Lite, Low, Medium, High, Ultra, and VR reported as a preset-only run), GPU cost order holds (median ms: lite 0.08, low 0.25, medium 0.62, high 1.25, ultra 4.03). Log .vibedeck/screenshots/gate2/bench.log, data bench-results.json. The bench is the one shared bench app, so it covers all three kits' tiers; the real-device line below is separate.
- [x] A size table of the three published games shows the 2D platformer and match-3 well below the kart racer, proving build-time modularity.
  - Signed off by Chris, 2026-10-16: "stage 2 looks good". Measured: the 2D platformer is 78.4 KB (20.8 percent) and match-3 is 80.8 KB (21.4 percent) smaller than the kart racer in wasm + js gzip.
  - Evidence note (2026-10-16): machine check passes. Wasm + js gzip: kart racer 376.6 KB, 2d-platformer 298.3 KB (78.4 KB, 20.8 percent less), match-3 295.9 KB (80.8 KB, 21.4 percent less); every package is under 1.3 MB of the 10 MB cap. Table: .vibedeck/publish-check/size-table.md; ROADMAP.md sizes updated.
- [x] No open core requests.
  - Evidence note (2026-10-16): no row is open for Stage 2. Every row in CORE_REQUESTS.md is done, filed for a later stage (the two match-3 plug-ins, sprite art by name and canvas text, moved to Stage 3 by Chris on 2026-10-16: "Move them to Stage 3"), or parked. One row, the Evani.Model relief request, reads "requested (Evani.Model track)", wanted with Stage 3 (docs/STAGE3_PLAN.md S3-10). Nothing is built for any of them here. Evidence: CORE_REQUESTS.md.
- [x] geocheck reports zero untagged issues for every kit.
  - Sandbox (2026-10-16): 0 untagged after reversed-Z (df0a83e); report .vibedeck/geocheck/sandbox/report.md.
  - Evidence note (2026-10-16): machine check passes on m1-foundation d7b84d6 (the later lint-clean merge only fixes clippy findings), all four, full runs: kart-racer 140 issues, 0 untagged (all tagged; z-fight scan over 700 cameras, 0 patches), 2d-platformer 0 issues, match-3 0 issues, sandbox 0 untagged. Reports .vibedeck/geocheck/<id>/report.md and .vibedeck/geocheck/kart-racer/zfight/zfight.md; logs and copies in .vibedeck/screenshots/gate2/.
- [x] Bench passes on Chris's real devices, a Samsung phone (SM-G991B) and a Lenovo tablet (TB305FU), at the tiers they pick; a device without WebGPU shows the clear "needs WebGPU" message instead.
  - Signed off by Chris, 2026-10-16: asked "Shall I tick the last Stage 2 check, 'the speed test passes on your phone and tablet', now?", he answered "Tick it". Evidence: device run 3 (.vibedeck/screenshots/stage2-devices/results-3.md): phone picked Medium, PASS (avg 16.68 ms); tablet picked Lite, PASS on the current code without the flicker branch (avg 16.59 ms). The tablet-flicker branch stays unmerged: shown both side by side, Chris chose "Keep the original" (2026-10-16), so the shipped tablet result is the run-3 PASS without it.
  - (Superseded by Chris's sign-off above, 2026-10-16.) Machine evidence (2026-10-15), not passed: both devices have WebGPU (Mali-G78, Mali-G52). All three ship builds start and play at 60 fps on both at the tiers they pick (phone: kart High, platformer Ultra, match-3 Ultra; tablet: kart Low, platformer Ultra, match-3 Medium), but the bench fails at those tiers: phone passes Low and Medium only (High 20.6 ms avg), tablet fails every tier (Low 22.4 ms avg, 3D phase). The "needs WebGPU" message shows in all three games and the bench in desktop Chrome with WebGPU removed or with no adapter. Kart-racer and 2d-platformer have no touch controls. .vibedeck/screenshots/stage2-devices/results.md
  - (Superseded by Chris's sign-off above, 2026-10-16.) Update 2026-10-16: that note is out of date. The kart racer has touch controls and the 2d-platformer now has them too (branch `plat-touch`: left, right, run, jump, pause; headless proof in `.vibedeck/screenshots/plat-touch/`). Not yet play-verified on the phone or tablet. Nothing ticked.
  - (Superseded by Chris's sign-off above, 2026-10-16.) Evidence note (2026-10-16): not passed, not re-run. This line needs the phone and tablet; the games have changed since the last device run (forward-optional, platformer touch controls, Fennel's Leap polish, match-3 panel fit, lean-data, reversed-Z), so the 2026-10-15 numbers above no longer describe the shipped packages. Waiting for Chris and a new device run.
  - (Superseded by Chris's sign-off above, 2026-10-16.) Evidence note (2026-10-16, device run 3, not ticked): Chris's decision for this line: "Test each device at its own level, add a lighter one for cheap tablets" (the Lite tier, added in run 2). Run 2 (.vibedeck/screenshots/stage2-devices/results-2.md): phone picked Medium, PASS; tablet picked Lite, PASS. Run 3 (results-3.md, run3/): Lenovo TB305FU re-run on the current code: picked Lite, PASS (avg 16.59 ms, p95 16.70, p99 16.70, GPU 10.53 ms; after the MSAA change below, avg 16.59 to 17.09 ms, GPU 14.3 ms, PASS in both device-mode runs); all three fresh packages boot past the loader on the tablet with no console errors, and the kart racer and 2d-platformer show their touch controls once play starts. NOT done: the Samsung SM-G991B was not connected to the PC (Chris was asked to plug it in; still open), so the phone has no run on the current code and the three games were not opened on it. Chris also reported distance flicker on the tablet (curves in the kart racer); cause was Lite's missing MSAA; the fix (4x MSAA for Lite and Low) lives on the `tablet-flicker` branch, not yet merged (DECISIONS.md 2026-10-16, before and after screenshots in run3/flicker/). The tablet's Lite PASS holds with and without it, but the shipped result depends on that merge, and the publish/ packages need a rebuild after it. Box stays open: waiting on the phone run (phone not connected) and on the tablet-flicker merge, then a re-check of the tablet on the merged build.
  - (Superseded by Chris's sign-off above, 2026-10-16.) Evidence note (2026-10-16, device run 3, phone, not ticked): Phone PASS at Medium; the line waits on the tablet-flicker fix and a tablet re-check after it merges. Samsung SM-G991B (Mali-G78), bench device mode on m1-foundation 163f143: the warm-up picked Medium, PASS (avg 16.68 ms, p95 16.80, p99 16.90, GPU 14.25 ms, 1088 x 504, 359k tris); Lite and Low pass, High (avg 22.29), Ultra (84.35) and VR (19.17) fail, information only; same pick and verdict as run 2. All three fresh publish/ packages boot past the loader with no console errors and run at 60 fps in play; kart-racer and 2d-platformer show their touch controls once play starts. Evidence: .vibedeck/screenshots/stage2-devices/results-3.md (Phone section), run3/phone-*. Box stays open.

## Track: Evani.Model (between Stage 2 and Stage 3)
Project: Evani.Dev tools (crates/evani-model, tools/evani-model)
Chris started it on 2026-10-15 and placed it here ("Place Evani.Model in between"). It is an AI-only 3D modeler with its own geometry engine, run by MCP commands, and it is dev-only (only compiled models reach players). The brief, word for word: docs/EVANI_MODEL_BRIEF.md. It never changes evani-core or evani-render (engine needs go to CORE_REQUESTS.md). It integrates through glTF, evani-convert (Stage 3) and geocheck.
Milestones (each ends with a commit, a report against the brief's tests and Chris's approval): M0 research (clean-room notes, the benchmark set, baselines); M1 compiler; M2 search. If M2 misses its targets, stop and report. Then M3 language, M4 relief, M5 integration and proof (needs evani-convert from Stage 3).
2026-10-16: Chris set a character goal: Evani.Model should be able to model the Chimp Duty soldier from his concept sheet fully ("use this as one of the goals to be able to model this fully in Evani.model"). Plan and gap list: docs/model_research/target-chimp-duty.md.
2026-10-16: Chris chose a separate Evani.Model tab for building models ("Its own Evani.Model tab"). Plan and contract: docs/EVANI_MODEL_TAB.md.
2026-10-17: ChimpDuty sent a ranked list of 13 requests to make the code-built chimp look high-end (from ChimpDuty, sent with Chris's OK; report: ..\ChimpDuty\.vibedeck\research\2026-10-17-high-end-look-for-code-built-characters.md). The Evani.Model items (albedo, roughness and AO bake, wear and dirt, vector decals, high-detail normal bake, kit generators, hard-surface primitives, folds, cylindrical and spherical charts, adaptive octree meshing) are filed in docs/model_research/target-chimp-duty.md section 6b at MC3, CD1, CD3, CD4, CD5, CD6 and M4, in the Chimp Duty order. The engine items are in CORE_REQUESTS.md (`smooth-lighting` and `soft-shadows` for Stage 5, `fur-shells` for Stage 8); the grip-pose helper is in the Parking lot. Chris's rule for all 13: "all must be tools specifically developed for our Evani.dev and Evani.model", so each is built from scratch and every outside project is for ideas only, with no libraries, no copied code and no new dependencies. No gate or milestone changes, nothing built, and each item still needs Chris's approval in its milestone.
2026-10-16: Chris signed off MC1, the code-built "Frontier Marshal" cowboy, the first character step. Asked "With that target in mind, is the code-built cowboy good enough to sign off this first character step?", he answered "Sign it off". Evidence (in .vibedeck/screenshots/evani-model-characters/: marshal-compare.png, mc1-compare.json, mc1-tests.log, mc1-validator.txt, mc1-bench-hashes-*.txt): outline match about 0.79 with the Meshy Marshal, height within 8 mm, arm span within 16 mm, all musts pass on four tiers, Khronos validator 0 errors 0 warnings, the 30 benchmark fingerprints unchanged. This approves the colour, skinning and characters extension of the brief. No gate box is ticked for it.
Gate (the brief's tests A to K):
- [x] M0 approved by Chris (2026-10-15, "Yes, sign off and start step 1"; evidence: docs/model_research/ with review-m0.md and its follow-up, decisions in plan-decisions.md approved as recommended).
- [x] M1: a kart written as intent compiles to valid glTF with LODs, anchors and collision, passing section A (2026-10-16, "Sign it off and keep the code"; evidence: docs/model_research/report-m1.md, review-m1.md (approve with conditions, conditions met), originality-m1.md (pass with notes; SHA-256 kept, Chris: "Yes, keep it as it is")).
- [ ] M2: the 8 karts reach silhouette IoU 0.9 or better within the speed targets on Standard and Lite.
- [ ] M3 to M5 as in the brief; Chris prefers, or judges equal, the Evani.Model karts in an unlabelled comparison.
- Note (2026-10-16): Chris asked for code-built characters ("ok but we need to be able to build these models with code") and chose "Build fresh" on the prior-art card. This extends the brief (colour, materials, skinning and native detailed characters); the plan and its proposed milestones MC1 to MC4 are in docs/model_research/plan-characters.md. No box is ticked for them.

## Stage 3: Real content
Project: Evani.Dev engine and Evani.App
Includes:
- "Real models in Evani.Dev": glTF 2.0 import (meshopt, KTX2), skinned animation with blending and simple IK, LOD, a lightweight text and UI layer.
- "Evani.dev model converter": tools/evani-convert turns any detailed model (glTF, or .blend through headless Blender) into a much smaller one that looks the same. Textures resized to their largest on-screen size per tier, as per-device sets (ASTC for phones and BC7 for desktop, as KTX2 files listed in the Evani metadata, with a plain PNG or JPEG image any reader opens as the fallback); geometry quantized, compressed and simplified (attribute-aware, no baking); master kept untouched for Ultra; duplicates instanced; animation compressed; optional look matching. A quality gate renders original and converted from 8 angles per tier and backs off compression until the difference is invisible. Size and quality reports per model. Drop-to-convert in the Evani.dev tab with a before/after slider. A Blender add-on "Export to Evani.dev". Output is standard glTF (EXT_meshopt_compression, KHR_mesh_quantization, core PNG or JPEG images) plus Evani metadata (`extras.evani`, including the per-device texture sets). Plan: docs/STAGE3_PLAN.md.
- Match-3 sprite art by name (smooth heart and wrapped-sweet pieces from the look) and canvas text (floating scores and combo callouts drawn in the canvas), moved here from Stage 2 (Chris, 2026-10-16: "Move them to Stage 3"): docs/STAGE3_PLAN.md S3-13c, CORE_REQUESTS.md plan items A and B. Sprite art by name is done (merge 1c77dcb); canvas text is not built, deferred (it needs match-3 game changes and Chris's decision against Gate 3's unchanged-game proof). Every deferred and open Stage 3 item, with its reason, is listed in docs/STAGE3_PLAN.md "Deferred and open Stage 3 items" (2026-10-20).
- 2026-10-16: Chris opened Stage 3 building ("so get moving on finishing stage 3 and 4") before Gate 2's sign-off; Gate 2 still needs his play-through and signature.
- Size trim, open Stage 3 tooling task, a priority next step (Chris, 2026-10-20: lightness first; un-parked from the Parking lot): model streaming (S3-4, +8,628 B gz wasm against 3 KB), Lite/Low shading, GPU skinning (S3-9, +6,684 B gz against 2 KB), the Lite/Low sun-cone smear fix (+2,149 B gz) and the anim cuts (S3-8, +9,300 B gz against 6 KB: two-bone and aim IK as their own opt-in, about 1.5 to 2 KB; lighter clip checks, about 1 KB). Earlier calls it replaces: "Accept about 9 KB now, shrink later" (anim, 2026-10-17), "Accept it as it is" (streaming, 2026-10-17). Details: DECISIONS "Animation runtime", "Model handles, LOD and tiered streaming", "GPU skinning" and the smear entry; docs/STAGE3_PLAN.md "Deferred and open Stage 3 items". Each cut is its own step with the SHA-1 proof for non-users.
- Text-module split, open (Chris, 2026-10-20: un-parked, lightness first): split `msdf` from the bitmap text and the art packer. Today `ui` turns on `msdf`, which implies `text` and with it `sprite-art`, so a game that only wants MSDF text pays +17,066 B gz wasm; the split would save about 10 KB of that (DECISIONS "MSDF text", size; accepted on 2026-10-17 with "Accept now, split it apart in a later step"). It touches render code every game compiles, so it is its own step with the SHA-1 proof for non-users.
- ChimpDuty materials and sky intake (Chris, 2026-10-21: "Everything now"; the sky leaves the Parking lot). Opt-in only: crate `evani-material-proc` (procedural box surfaces, baked AO, 61 live `material.*` params; it is the start of the engine's shared materials library), its `sky` feature (Preetham dome with clouds, sun following the scene light, 14 `sky.*` params), photoreal's `daylight` preset, and a test of the existing ACES tonemap. Games that don't list them ship byte for byte as before. Gaps for Chris: the catalog has no sky or daylight entries; the forward mesh pass has no per-material roughness. Details: DECISIONS "ChimpDuty materials, sky and daylight intake", docs/CHIMPDUTY_INTAKE.md. No gate box is ticked by this.
- Multiplayer engine build moved into now (Chris, 2026-10-24: "I want you to fully build the Stage 3 multi-player engine. However, as I was under the impression that this was already built, maybe it's just not incorporated. Either way, get it done."; later the same day: put it in Sugarpeak Racer and give partners instructions to build their own bridge, with no ChimpVibe-specific code inside the engine). His request overrides rule 1 for this work only; multiplayer stays listed in Stages 5 and 9. Audit and packages: docs/NET_BUILD_PLAN.md (branch `net-engine`); partner hand-off: docs/NET_BRIDGE_GUIDE.md. The engine stays platform-neutral; games without `net` ship byte for byte as before. No gate box is ticked by this.
- Detail rules: hero assets come from detailed models; recipes or kits may assemble detailed parts into variants; looks provide shader micro-detail; detailed assets stream after the title screen, by tier.
Gate:
- [ ] A detailed Blender kart and an AI-generated character convert, pass the quality gate on every tier, and load into the kart racer.
- [ ] Size and quality reports exist for both, and Low downloads only the LODs it uses.
- [ ] Dropping a model onto the Evani.dev tab converts and hot-loads it, with a before/after slider.
- [ ] A game that doesn't use these features compiles without them.

## Stage 4: Flagship delivered
Project: Evani.Dev kits and Evani.App
Includes:
- Kart racer art pass: real hero assets (player kart and driver), rival karts built from detailed parts, a close-up camera (garage or podium).
- "One-click publish": build from evani.toml, show the size report, host on Evani.app, give a shareable link.
Gate:
- [ ] The kart racer is published on Evani.app and plays from a link on a phone and a laptop.
- [ ] Its published size meets the per-tier targets, and bench passes on every tier.
- [ ] Human check: Chris would show it to someone as a real game.

Notes:
- 2026-10-16: Chris decided "Start Stage 4's one-click publish now, alongside Stage 3", before Gates 2 and 3 are signed; the kart art pass stays for when Stage 4 opens. Delivered (Evani.Dev side): `cargo xtask publish <id>` builds from `evani.toml`, packages into `publish/<id>/` through a staging folder swapped in only when the preflight passes, prints the size report (by module, then by file; raw, gzip and Brotli; the 10 MB cap on the raw whole folder) and the diff against the live version (NEW / MODIFIED / SAME / DELETED, "nothing changed, skip"), serves a local preview with `--serve`, writes the summary outside the upload and records live versions with `--mark-published vN` (docs/PUBLISHING.md, DECISIONS.md "One-click publish"). Uploading stays Evani.App's: only the owner's click on its publish card uploads. Still open: the library side (a permanent link per version, a live pointer, rollback, a Publish button in the Evani.dev tab) in Evani.App, and per-tier size targets, which the catalog does not set yet (Stage 7). No gate box is ticked by this.
- 2026-10-20 (S4-3): per-tier download size targets approved as provisional (Chris, 2026-10-20), from docs/STAGE4_SIZE_TARGETS.md: code at most 350 KB gz for 2D games and 500 KB gz for 3D games with `models` and `anim`; boot assets 100 / 200 / 300 / 400 / 500 KB (Lite / Low / Medium / High / Ultra) and 300 KB (VR); total download 2.5 / 3.5 / 5 / 6.5 / 7 MB and 5 MB (VR), all under the 10 MB cap; texture memory at 75% of each tier's catalog ceiling (48 / 72 / 144 / 384 / 1152 MB and 192 MB VR). Provisional: the totals lean on unmeasured estimates and are fixed after the kart racer is measured. Not yet in catalog/ or assets/tiers/ (a separate step).

## Stage 5: Prove the range
Project: Evani.Dev kits and engine
Includes:
- Kits 004 to 006 in ROADMAP.md: top-down action-adventure, base-building RTS, tactical FPS, using the Stage 3 content tools.
- Render paths filed here (CORE_REQUESTS.md, `assets/render_paths.json` `deferred`): 2D normal-mapped lighting for kit 004's `pixelhd` look (High, Ultra); CPU frustum culling, point lights, two shadow cascades, clustered forward and image-based lighting for kits 005 and 006.
- Hexagon (planned 2026-10-15, plan only; Chris approved the plan's recommendations the same day): Chris's game rebuilt as kit 006's demo with a cinematic first-person look, "best look on strong PCs, lighter on phones" (docs/HEXAGON_PLAN.md: scope, modules, per-tier frame and download targets, decided questions). It depends on Stage 3 ("Plan now, build after Stage 3") and is built here, with the render paths above and evani-net bots; its Ultra extras wait for Stage 8. It adds no gate box; kit 006's existing gate lines cover it.
- Render paths filed here (CORE_REQUESTS.md, 2026-10-17, from ChimpDuty, sent with Chris's OK): `smooth-lighting` (smooth ramp, roughness specular from the metal-rough and occlusion maps, rim light and GTAO contact shadows, as lighting of the existing `stylizedpbr` look for Medium and up, not a new look) and `soft-shadows` (PCSS). Approved (Chris, 2026-10-20); they were proposed, not approved, and they change no gate line.
- Phase A hardening: work through CORE_REQUESTS.md, move systems built twice into shared crates, and update KIT_PROCESS.md with lessons learned.
Gate:
- [ ] All six Phase A kits are Verified on the hardened core.
- [ ] No system is duplicated across kits.
- [ ] The tactical FPS plays offline with bots through evani-net.

## Stage 6: Platform
Project: Evani.App
Includes:
- "Kit queue in Threads": Phase B kits start from the queue (Ongoing).
- "New Game in App Studio": pick a genre (ranked by most played), a look and effects; get a playable skeleton and a pinned reference image prompt.
- "Publish to the library": kits, looks, effects, tuned presets and playgrounds (playable, remixable games; docs/PLAYGROUNDS.md), with the creator's name.
- Library rule (S6-3; Chris, 2026-10-20): the library keeps Chris's 2026-10-15 rule, playgrounds and model assets only; looks, effects and presets travel inside the playgrounds that use them rather than being published on their own.
- "Model routing and costs": route work by task type; show cost per kit and per model.
- Render path filed here (CORE_REQUESTS.md): the pseudo-3D pass, built by the first Phase B kit whose genre lists `pseudo-3d-road` or `pseudo-3d-raycast`; until then `cargo xtask check` refuses a game with a pseudo-3d look.
Gate:
- [ ] A new user goes from App Studio to a published game without writing a prompt.
- [ ] A look or asset from one game is applied to another from the library.
- [ ] Phase B kits run from the queue, and the Costs tab shows the cost per kit and per model.

## Stage 7: Lean delivery
Project: Evani.Dev engine and Evani.app hosting
Includes:
- Game-level byte budget: bytes allocated by measured on-screen importance from bench camera paths, against a per-tier size target.
- Per-device delivery: native texture format, tier LODs, per-level assets, shared content-hash cache on evani.app.
- Shared tuning data (opt-in), used to improve default tier presets and effect weights (Ongoing).
Gate:
- [ ] The kart racer meets set per-tier download targets, with a report of where every byte went.
- [ ] A phone and a desktop each download only their own texture format and LODs.
- [ ] A second Evani game played on evani.app reuses cached library assets.

## Stage 8: Unlimited detail
Project: Evani.Dev engine and Evani.App
Includes:
- Cluster-based streaming geometry for High, Ultra and VR (GPU compute selection, indirect draws).
- Render paths filed here (CORE_REQUESTS.md): GPU-driven culling with indirect draws, four shadow cascades, volumetric passes and optional temporal upscale at Ultra. Hexagon's Ultra tier (docs/HEXAGON_PLAN.md) is the proposed proof scene for them.
- Render path filed here (CORE_REQUESTS.md, 2026-10-17, from ChimpDuty, sent with Chris's OK): `fur-shells`, instanced fur shells and fins with the baked clump texture as the fallback on low tiers. Proposed, not approved; Chris may move it earlier.
- Look transfer: one master model into toon, low-poly, voxel, brick, papercraft and sprite-sheet looks.
- Prompt to playable asset: AI 3D generation, conversion, auto-rigging, animation retargeting, with provenance metadata.
Gate:
- [ ] A very high-detail master model holds the Ultra frame target, with detail streaming in smoothly.
- [ ] One kart master appears correctly in toon, voxel, brick and 16-bit sprite form.
- [ ] A text prompt becomes a rigged, animated, look-matched character driving a kart in under ten minutes, saved to the library with its provenance.

## Stage 9: Reality and reach
Project: all
Includes:
- Real VR, once WebXR with WebGPU is stable in the main browsers: evani-xr as an on-demand module, VR enabled in the Phase A kits. The VR render paths (single-pass stereo, 4x MSAA, foveation; CORE_REQUESTS.md) are built here.
- Captured reality: Gaussian splats and photogrammetry, compressed by the asset compiler and mixed with meshes.
- Phase C massive online kits, once evani-net has been proven by several Phase B kits.
Gate:
- Each item writes its own gate before it starts, and Chris approves it.

## Parking lot (not scheduled)
- Critic and review tool gaps from Puff's Sweet Grid's round-4 critic report (Chris, 2026-10-16: "why dont we fix them after stage 4", then "or after stage 8"). Two branches are parked unmerged, with their worktrees kept. `critic-tools` (d8d7779, ../Evani.Dev-critic-tools) has the "Not captured" coverage step, the critic's own instructions, the extra-round wording and an 11-gap list in docs/CRITIC_TOOLING_GAPS.md; still to do: the full check, the hash proof and the merge. `s3-13c-critic4` (1a39bd4, ../Evani.Dev-m3fix4) has the even-ink-outline check and the detached-text-shadow lint, built and tested; the review-card change and the match-3 dump are written but not compiled. The game fixes themselves were stopped (Chris: "Stop the game fixes, keep only the new checks"). Pick these up after Stage 4 or Stage 8; Chris decides when.
- Spatial upscaler (FSR 1 style) as a rung for Low, Medium and High, with detect-and-fallback (2026-10-16, research only, not approved): the design is in docs/UPSCALING.md. It would change what catalog/tiers.json says (today only Ultra lists an upscale), so it waits for Chris's approval. The temporal rung stays in Stage 8 (`temporal-upscale`).
- Grip-pose helper (2026-10-17, from ChimpDuty, sent with Chris's OK; ChimpDuty rank 13, S-M, a separate kit): puts a character's fingers on a held rifle. Not a core or Evani.Model change; the named rifle-hold pose stays in the Chimp Duty plan (docs/model_research/target-chimp-duty.md gap 7, section 6b). Report: ..\ChimpDuty\.vibedeck\research\2026-10-17-high-end-look-for-code-built-characters.md. Rule 9 applies. Chris's rule for the whole list (via the ChimpDuty orchestrator, 2026-10-17): "all must be tools specifically developed for our Evani.dev and Evani.model", so this is built from scratch, outside projects are for ideas only, no new dependencies (docs/model_research/target-chimp-duty.md 6b).
- Tactical-fps game polish from the 2026-10-21 test pass (rule 9, Stage 8): an ammo count with reloads (the game has none today, so it is new gameplay), a minimap, and a real gun model for the bots (today a flat box). The pause, the match HUD in the page and the shadow and muzzle-flash brightness were fixed then as defects.
- TAA and auto-exposure (Chris, 2026-10-20): not in the catalog, left out for now. (The sky system left this list on 2026-10-21: Chris chose "Everything now" for ChimpDuty's materials and sky; see Stage 3.)
- Recipe modeler (evani-shape). Revisit after Stage 5 if kits keep rebuilding their own placeholder generators.
- Normal-map baking for imported models (Chris, 2026-10-15, Stage 3 plan decision 6): imported models keep detail through attribute-aware simplification checked by the quality gate; Evani.Model models bring their own normal maps. Revisit if the gate keeps refusing strong simplification.
- Neural texture and material compression. Research only, until browser decode cost is practical.
- Cloud-streamed rendering as a premium VR option. It conflicts with the low-bandwidth promise, so it stays optional.
- Look guard histogram-shape comparison (research option 2, 2026-10-14). Add only if the simple look measures miss real problems.
- A gameplay sweep for live tuning (2026-10-15, kit 001 gate work): `sweep.run` holds the sim clock on the camera path, so a handling value (speed, turn rate, boost) never shows in its table. A command that plays N seconds of the kit's own autopilot or bot per value and returns race measures (time, place, gap) would let an AI choose handling with one sweep, as it does for looks. Today the kart racer's `playtest::tune_table` bot races stand in for it.

## Gate log
| Stage | Passed on | Evidence | Signed off by |
|---|---|---|---|
| 0 | 2026-10-14 | Foundation "done when" checks 1-11 pass (docs/M1-ACCEPTANCE.md). An AI through the MCP bridge changed Bloom and rewrote the tonemap slot; a broken patch was rejected while the old shader kept running; the bake rebuilt pixel-identical without evani-live (.vibedeck/screenshots/sandbox-review/live-session.md). Controller: never degrades with the GPU idle; cut order and limits unit-tested; recovery 2.8 s live; multi-step restore and no-flapping tests (crates/evani-core/src/quality.rs). Tier cost order Low < Medium < High < Ultra on the stress-scene bench, 5 runs, GPU clock 2760-2790 MHz (docs/M1-ACCEPTANCE.md, f8a484f). Published sandbox size about 229 KB gzip (234,190 bytes; release, without evani-live). | Chris |
| 1 | 2026-10-15 | Machine checks: .vibedeck/screenshots/gate1/gate1-status.md (tab connect 26/26 and version message, tab-connect.md; Talk to the AI 23/23 with a pixel-identical rebuild without evani-live, talk-to-ai.md; look guard end to end in the installed Evani.App with Chris approving the limits and one flagged bake, look-guard-app.md; sandbox review card complete and the broken test kit sent back, broken-kit-sent-back.md). Human checks: Chris ran the Stage 1 steps in the installed Evani.App's Evani.dev tab and reported "stage 1 checks, round successfully". Open follow-ups, not blocking: the measured after-test look_check; the Evani.dev tab going blank after some project switches (sent to Evani.App). | Chris |
| 2 | 2026-10-16 | Machine checks, fresh on 2026-10-16 (status per line: .vibedeck/screenshots/gate2/gate2-status.md): all three packages rebuilt from m1-foundation 5becff2 in publish/kart-racer, publish/2d-platformer and publish/match-3 (not published), engine notice clean, each boots to its game in muted headless Chrome (boot-*.png); desktop bench 6 of 6 tiers PASS, cost order holds (bench.log, bench-results.json); size table (.vibedeck/publish-check/size-table.md): the 2D platformer is 78.4 KB (20.8 percent) and match-3 80.8 KB (21.4 percent) smaller than the kart racer in wasm + js gzip; geocheck 0 untagged for kart-racer (140 tagged, z-fight scan 0), 2d-platformer, match-3 and sandbox (.vibedeck/geocheck/<id>/report.md); no core request open for Stage 2 (CORE_REQUESTS.md; the two match-3 plug-ins moved to Stage 3 by Chris); `cargo xtask check` clean and `--full` passed (check.log, check-full.log). Human check: Chris said "stage 2 looks good" on 2026-10-16. Left open, not ticked: "Bench passes on Chris's real devices". Update 2026-10-16: the device bench was re-run at each device's own tier (Chris: "Test each device at its own level, add a lighter one for cheap tablets"): the Lenovo tablet picks Lite and PASSES (results-2.md, results-3.md); the phone passed Medium in results-2.md but has not been run on the current code (not connected on 2026-10-16), so the line stays open until it is. Chris's sign-off does not override it; it is carried into Stage 3 as an open follow-up. | Chris |
