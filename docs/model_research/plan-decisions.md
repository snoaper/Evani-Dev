# Decisions for Chris before M1

M0, 2026-10-15; cleaned up after the independent review (review-m0.md) in the M0 follow-up. One list: 16 decisions that need Chris, then 3 that the rules already settle. Each says what is being asked in plain words, what we recommend and why. Technical detail for workers is in the second table.

**Costs money: D7 only** (renting test computers online, about 30 to 50 US dollars each time the full hardware test runs, two or three times in total). Everything else costs only agent time.

**Chris's answer (2026-10-15):** "Proceed with all recommendations", then "Yes, sign off and start step 1". Every decision below is decided as recommended (D17 as option A). M0 is signed off. D7 is approved in principle; each actual rental is still confirmed with Chris before any money is spent.

## Decisions that need Chris

| # | The question, in plain words | Recommended | Why | Waits on it |
|---|---|---|---|---|
| D1 | Let the modeling tool use the graphics card on Windows, Mac and Linux? (Only in the tool on our machines; games are not affected.) | **Yes** | The fast search needs the graphics card, and there is no other way to reach it on all three systems. | M2's graphics-card work |
| D2 | Let the tool reuse the same connection pieces the live tuning tools already use to talk to AI agents? | **Yes** | Nothing new enters the project, and the tool's effort goes into modeling, not plumbing. | the tool's first agent commands (M1) |
| D4 | Give the 7 rival karts their own shapes in the test set, instead of 8 copies of one shape? | **Yes** | Eight different targets test the tool properly and give the race visible variety (the rivals will look different once the new karts are in the game). | the kart test pictures (M1) |
| D5 | What "same request, same model" promises. The same request at the same effort level gives exactly the same model on any computer. On "automatic" effort, two computers of the same class (for example two mainstream gaming PCs) give the same model; a stronger and a weaker class may give different, equally valid models, and the reply names the effort level so either result can be repeated anywhere. Accept this? (The alternative, "automatic" using all the time each computer has, gives slightly better results on fast computers, but two computers would rarely match.) | **Yes, as described** | It is a promise we can keep and test, without slowing every computer down to the weakest one. | the search design (M2), the hardware test |
| D6 | Make the chunky toon modeling style a style of the existing toon look, without adding a new look to the catalog? | **Yes** | The catalog stays the single source of looks; modeling proportions are a separate layer on top. | the kart template (M1) |
| D7 | **Costs money.** Rent online test computers (a Mac, an AMD graphics card, a large 24 GB graphics card) for about 30 to 50 US dollars per full hardware test, two or three times in total, and use your laptop or a cheap mini PC with built-in graphics for the low class? | **Yes** | Four of the six computer classes the brief asks for are not available here, and M2 already needs the low class. Ask the provider for the right quotas early; new accounts often start without them. | end of M2 (low class), M5 (all classes) |
| D8 | Keep the outside AI model generators optional and switched off, for trying out only, and never publish or ship anything they make until their licences are cleared? | **Yes** | Their licences forbid commercial use. | nothing in M1 |
| D9 | Run the "same AI without the tool" comparison at the end (M5), with fair rules: up to 40 steps or 90 minutes per try, both sides measured at the same quality bar, about 25 to 40 hours of agent time? | **Yes, in M5, with these rules** | The "5 times fewer steps" goal needs a fair comparison, and the old limit of 12 steps could never show more than 6 times. | M5 |
| D11 | Wait for Stage 3's model converter for the final integration (M5), and for the in-game proof of surface detail (M4)? | **Yes** | Building the converter early would break the stage order, and M1 to M4 can be built and checked without it. | M4's in-game proof, M5 |
| D12 | Build the 22 non-kart test references from simple shapes described in words, rather than from AI-generated images? | **Yes** | Repeatable, original and free of licence questions; you check them once on one picture. | the test pictures (M1) |
| D13 | Leave out the "does it look like a kart" picture-judging AI for now, and score by measurements only? | **Yes, for now** | Measurements are enough for M2's goals, and the picture AI would need new software. | nothing before M3 |
| D15 | In M1, test what M1 builds, and add the surface-detail and edit tests in M3 and M4 when those parts exist? | **Yes** | Those parts do not exist until M3 and M4. | M1 sign-off |
| D16 | Sign M0 off although two things the brief listed under M0 are only planned: the test reference pictures (made in M1 by the first small tool) and the "without the tool" comparison runs (done at the end, D9)? | **Yes: pictures in M1, comparison runs at the end** | Both need working software first; M0 is documents only. The M0 report says this plainly (README, "M0 report"). | M0 sign-off |
| D17 | The mesh checker. The tool needs to check its models for holes and overlaps. Using the game engine's checker would drag the game engine's core into the tool. Options: A, the tool gets its own small checker now, and later, with your OK, the engine's checking tool switches to the same code so there is one checker; B, move the engine's checker into a shared small piece now (a small change to an engine tool); C, use the engine's checker as it is. | **A** (the reviewer recommended B; A reaches the same end without touching the engine now) | A starts M1 without changing any engine tool; the tool's checker is also faster on big models and gives the same answer on every computer. Not C: it breaks the rule that the tool stays separate from the game engine. | M1's mesh work |
| D18 | During automatic test runs nobody approves models, so the tool cannot show it "learns" from approvals. Count a model that passes every test as approved, for that test run only, kept apart from your real approvals? | **Yes** | It lets the test show the learning effect without ever touching your own approvals. | the learning test (M3, M5) |
| D19 | The game cannot show fine surface detail (tread, bolts, panel lines as shading) yet. Let the tool draw its own preview pictures for your sign-offs, and ask for the game-side support as an engine request for Stage 3 (filed today)? | **Yes** | You can judge surface detail at M4 without waiting, and the engine work goes through the normal engine queue. | M4 sign-off |

## Settled by the rules (no need for Chris)

| # | What | Settled as | Rule |
|---|---|---|---|
| D3 | Picture files for masks and maps | Plain picture files written by our own code, without a compression library | The brief allows only four libraries; evani-convert compresses later. |
| D10 | Names of the tool's agent commands | Underscores (`model_make`), dotted names in the docs | Agents reject dots in command names; the live tools already do this. |
| D14 | Six extra templates (house, tower, mushroom, cloud, cactus, bush) | Added | The brief's own test set lists these items. |

## Technical detail (for workers)

| # | Detail |
|---|---|
| D1 | wgpu features for Vulkan, Metal and DX12 enabled only for `tools/evani-model` and evani-model's `gpu` feature, same pinned wgpu version; game builds unchanged (note 18). |
| D2 | serde_json (already pinned in the workspace), rmcp and tokio (already pinned and vetted for evani-live-bridge); alternative: a hand-written stdio JSON-RPC server on serde_json only. No new crate in Cargo.lock. |
| D4 | Option B: rival shape variants as overrides in tests/model_bench/manifest.json; option A: all 8 references identical. |
| D5 | Promise: same request (canonical intent and seed, level, tool version with its schedule table, content hashes of style, class, vocabulary, prior, `like` items and masks) at the same level gives the same bytes. `auto` = largest level whose time at the tier's published reference speed fits `budget_s` (default: Pro 512, Standard 256, Lite 64, CPU-only 32); tier stored per machine. GPU interval scores, close calls settled on the CPU, no clock in any decision (notes 07, 08, 18; MODEL_API.md 1.8). Test H compares fixed levels across machines and `auto` within a tier. |
| D6 | `toon_chunky` as data in evani-model on `look: toon`; catalog/looks.json unchanged (note 02). |
| D7 | AWS EC2 Mac (24-hour minimum, about 16 US dollars), g4ad (AMD), g5 or g6e (24 to 48 GB); Lite on a laptop or mini PC with integrated graphics; plan-m1.md hardware matrix. Prices checked by the review on 2026-10-15. |
| D8 | TRELLIS.2 depends on nvdiffrast and nvdiffrec (NVIDIA Source Code License, non-commercial); OmniPart includes PartField-derived code under the same licence (README, flagged sources 8). |
| D9 | Caps 40 calls or 90 minutes; credited at the first call whose output meets the bar; "not reached" reported with lower bounds (plan-baselines.md sections 1 and 4). |
| D11 | Stage 3 brings evani-convert and glTF import; M5 and test J wait; M4's in-game check waits for the CORE_REQUESTS row of D19 as well (note 20, note 12). |
| D12 | Primitive assemblies from `manifest.json` descriptions, written as `ref/<id>/assembly.json` (tests/model_bench/README.md). |
| D13 | SigLIP 2 weights are Apache-2.0 but need an inference runtime (new dependencies); deferred to M3 or later. |
| D15 | M1 section A: determinism, must kinds, mesh validity, normal maps of the smooth field, compiler-level locality (plan-m1.md). |
| D16 | References: plan-m1.md T2 and T15; comparison runs: D9; M0 report in docs/model_research/README.md. |
| D17 | Option A: `meshcheck` module in evani-model, std only, plain arrays in, sorted issues out, spatial-grid self-intersection test; geocheck's checker used only as a cross-check from tools/model-bench-ref (note 13, 20). Option B (review): move geocheck's `meshcheck` into a tiny crate without engine dependencies that geocheck re-exports. Option C would add evani-core and evani-render to evani-model through tools/evani-geocheck's Cargo.toml. |
| D18 | Benchmark acceptance: top candidate meets the bar; added to a run-local prior file under `.evani/bench/<run>/priors/`; real priors neither read nor written; two run orders; `like` identical in both arms (note 09; plan-baselines.md section 4). |
| D19 | Tool side: bakes, recipe in `extras.evani`, readability maths, tier previews (notes 12, 21). Game side filed in CORE_REQUESTS.md (2026-10-15, "Relief display for imported models", status requested). |
