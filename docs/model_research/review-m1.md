# Evani.Model M1: independent review

Reviewer: QQ (Opus, high effort), 2026-10-15, on branch `evani-model` at 25b7b5f. I did not build M1. Scope: plan-m1.md (T1 to T15 and their section A tests), report-m1.md, plan-decisions.md, STAGES.md (M1 line), docs/EVANI_MODEL.md and the brief's tests, the code in `crates/evani-model`, `tools/evani-model` and `tools/model-bench-ref`, and the four blueprint pictures. Everything below was re-run or read by me unless it says "report only".

## Verdict: approve with conditions

The compiler is real and works: all 30 benchmark intents compile to four tiers of valid glTF with anchors, collision and normal maps; the output is byte-identical across runs and thread counts, and (new evidence) between a native and a WebAssembly build; the tests and clippy are clean; nothing reaches a game and no third-party crate was added. The kart reads as a kart.

Two things stop a plain approval. First, the report's "known bug" is not a quirk: the musts are only ever judged on the finest tier, and when each tier is judged on its own, 17 of the 30 items fail at least one must on a lower tier (all 8 karts but one included). Second, the brief requires the originality scan before every sign-off, and it was not run. Several plan items were also deferred without the report saying so (list below). None of this needs a redesign; the must-fixes are about a day of work plus the scan.

## Must fix before sign-off

1. **Judge the musts on every tier, and report them per tier.** `must::Ctx::finest_mesh` (must.rs:175) reads `tiers.last()`, so `stands`, `min_thickness`, `fit_box`, `min_size` and `max_size` only ever see the finest tier given, and `--tiers low` silently changes what is measured. Judged tier by tier (my run, below), 26 tier results fail (16 low, 6 medium, 4 high, 0 ultra; 33 failing must results in all): `stands` on 14 low, 4 medium and 3 high tiers (the karts' low tiers float 0.9 to 4.3 mm or rest on one axle; the crate's low tier sinks 26.9 mm below the ground), `fit_box` on 5 low and 3 medium tiers (door 0.369 m deep against a 0.2 m box, fence 0.318 against 0.15, shield 0.240 against 0.18: the 1.6-cell minimum thickness fattens thin props), `max_size` on the shield's low tier, `min_thickness` 0.0 mm on the sword's low, medium and high tiers. The report's suggested fix ("judge musts on the ultra tier even when fewer tiers are written") would hide all of this. Fix: run the mesh-based musts on every compiled tier with a tolerance the tier can meet (for example `stands` within the larger of 2 mm and half the tier's cell, contact found within a cell, not a fixed 5 mm), say per kind which musts are judged on the exact field and which per tier, put the per-tier results in `make`, `check` and `compile` replies, and make `kart_01_pip` pass on all four tiers. The other items' coarse-tier failures may stay open as a known gap (they belong with gap 5, per-part cell size), but they must be listed by name in the report.
2. **Correct report-m1.md where the code does not support it**: "all 30 pass every must" and commit 6483bfe's "pass every must ... on every tier" (true for the finest tier only); "the low mesh itself is clean" (clean as a mesh, but it fails `stands`); the per-tier "Time" column (`Workspace::tiers`, api.rs:73, builds all tiers at line 79 and filters them at line 89, so it always compiles all four tiers and filters afterwards, so 4.2 s for "low" is a four-tier compile); and add the deferrals the report leaves out (T5 subdivision cages; T9 octree, budgeted adaptive refinement and local re-extraction; T10 silhouette protection; T11 natural-parameter charts, conformal fallback and the 5% texel-density test; T15 `variants` and the reference check sheet; 19 overlapping texels in the kart's normal maps).
3. **Run the originality scan (plan-m1 section I) by a non-builder**, as the brief requires before every milestone sign-off (brief, originality rule 6). Steps 1, 2 and 5 need no Java; steps 3 and 4 need the corpus and JPlag (portable Java in the scratch tools folder). My quick part (below) found nothing, but it is not the scan.

## Should fix (M2 or with the must-fixes)

1. `must::surface_samples` (must.rs:48) hard-codes 2 threads (also build.rs:950 uses 1). It ignores the caller's thread count, and on a thread-less target (wasm32) the musts panic in `std::thread::scope` (seen in my wasm run). Pass the thread count through.
2. `model_sheet` advertises `kind: variants` in its MCP schema (mcp.rs:22) but `api::sheet` refuses it (api.rs:281). Drop it from the enum until M3.
3. The `.evmodel.json` metadata stores absolute output paths (api.rs:231), so the metadata differs per folder and machine; store file names.
4. DECISIONS.md still lists D1/D2 as "pending requests" (line 390): record the outcome (D2 taken as the hand-written serde_json server, D17 option A, no new crate).
5. Add an end-to-end determinism test: `gltf::kart_tier_round_trips_and_is_byte_identical` writes the same compiled tier twice; it does not recompile or vary threads. Compile the kart at 1 and 8 threads and compare the `.glb` and map bytes (my harness below does this).
6. The normal-map overlap count is reported (19 texels on the kart) but only tested to be 0 on a rounded box; test it on the kart or explain it.
7. Kart wheels show an odd keyhole-shaped mark on the hub faces in the blueprint's side view (check the hub detail). The chibi's upper arms fuse into the torso: the shoulder blend is too wide, so arm and body read as one shape, with a dark smear across the chest in the front view. Check it when part blend radii are calibrated (M2), not by changing the light.

## Evidence

### Re-runs (2026-10-15, worktree target folder, below-normal priority, CARGO_BUILD_JOBS 1 to 2, busy PC)

| Check | Result |
|---|---|
| `cargo test -p evani-model -p evani-model-tool` (RUST_TEST_THREADS=2) | library 60 passed, 5 ignored; `tests/benchmarks.rs` 1 passed (53.8 s); tool 1 passed; 0 failed. Matches the report. |
| `cargo clippy -p evani-model -p evani-model-tool --all-targets -- -D warnings` | clean |
| No unwrap or expect outside tests | the workspace denies `unwrap_used` and `expect_used`; clippy clean; a scan of non-test code finds no `unwrap`, `expect`, `panic!`, `todo!` or `unreachable!` |
| Dependencies against m1-foundation (merge base db5a647) | Cargo.lock gains only `evani-model`, `evani-model-tool`, `model-bench-ref`; `evani-model` depends on serde and serde_json only; the tool on evani-model and serde_json (D2's no-new-crate option); no `gpu` feature yet (M2) |
| Separation from the engine | `evani-model` and `evani-model-tool` have no evani-core or evani-render dependency; only the dev-only `model-bench-ref` links the kart kit, evani-render (`meshes`) and the toon look, to capture references (allowed by tests/model_bench/README.md); no app, kit or look depends on any of the three |
| Platform maths and hash maps | no std transcendental calls outside `dmath`, no `HashMap`/`HashSet` in the library |
| Determinism, all 30 intents (scratch harness outside the repo calling `api::compile_model`) | 1 thread, 3 threads and 1 thread again: every `.glb`, normal-map PNG and metadata file byte-identical (metadata compared with the output folder name masked); 30 of 30 |
| Determinism, 1 against 8 threads | kart_01_pip, house_small, rock, chibi_body: identical digests of all tiers' `.glb` and map bytes, with and without musts in the metadata |
| Determinism, wasm32 | the same pipeline built for `wasm32-unknown-unknown` (release) and run under Node 24 on one thread gives the same SHA-256 as the native build for kart_01_pip (5b24d14b...), house_small, rock and chibi_body. Musts were left out of this digest because of should-fix 1. Not covered: the musts on wasm32, the other 26 items, a browser engine, wasm32 with threads. |
| Khronos glTF-Validator 2.0.0-dev.3.10 (`node validate.mjs`, still present) | 120 files (30 items, 4 tiers) from my run: 0 errors, 0 warnings (2 to 6 infos each) |
| Kart through MCP in two calls (`model_make`, then `model_compile` with the returned id) | both succeed; replies 1,182 and 1,011 characters (about 340 and 290 tokens, under 400); musts 7/7, mesh ok, bake 0.4 deg, seams 0.6 deg, 19 overlapping texels |
| Musts judged on each tier alone (`api::musts` on one tier at a time, 2 threads) | fails on 17 of 30 items, 26 tier results; ultra never fails. Low: bench, bush, crate, door, fence, house_small, sign, karts 01 to 05, 07, 08 (`stands`); door, fence, shield, sign, sword (`fit_box`); shield (`max_size`); sword (`min_thickness`). Medium: bench, door, fence, lamp_post (`stands`); door, fence, sign (`fit_box`); sword (`min_thickness`). High: bench, door, fence (`stands`); sword (`min_thickness`). Clean on every tier: cactus, candy cane, chibi body, cloud, gumdrop, lollipop, mushroom, rock, tree, barrel, chest, kart_06_nib, tower. |
| Originality, quick part only | no licence texts, copyright notices or SPDX lines in the new crates, tools or benchmark data; file types are .rs, .toml, .json, .evmodel, .md, .png and 9 reference .glb files made by our own tool; largest file 1.07 MB (the blueprint PNGs); no weights or datasets. Not a substitute for the plan's scan. |
| Not re-run | `model-bench-ref` tests (kart size, masks, IoU 0.85, ramp drift): report only; `cargo xtask check`: report only; blueprint timing (report: 3.2 s on a busy PC) |

### T1 to T15 against plan-m1.md

| Task | Verdict | Evidence and differences from the report |
|---|---|---|
| T1 skeletons | Done | three crates, workspace wiring, no new third-party crate; the `gpu` feature is not there yet (fine, M2); DECISIONS.md not updated (should-fix 4) |
| T2 bench reference tool | Partly | kart capture, masks, provenance (`ref/kart_01_pip/ref.json` with hashes), tests named in the report exist; the 22 non-kart assemblies and the reference check sheet for Chris are not done (report: moved to M2) |
| T3 maths, SHA-256, tile map | Done except wasm32 | tests exist and pass; my wasm32 run now covers the compile path, not the maths tests themselves |
| T4 intent | Done | parse, round trip and golden-error tests pass |
| T5 SDF graph | Partly | primitives, booleans, smooth blends, mirror, repeat, twist, bend, taper, lathe, noise, Lipschitz and gradient tests are there; subdivision cages (Catmull-Clark, BVH, winding numbers) are not, and the report does not say so; culling is by 8^3 blocks, not an octree |
| T6a, T6b generators | Done | 16 generators; anchors within 0.1 mm of the surface tested; `external` returns "off in M1" |
| T7 solver, musts | Done (times not measured) | a passing and failing test for every kind, 1 mm gap, 29-degree steer, 2 cm narrow seat |
| T8a kart template | Done (report only for IoU) | IoU test exists in model-bench-ref; not re-run by me |
| T8b other templates | Done on the finest tier | all 30 pass on ultra; 17 fail a must on a lower tier (must-fix 1) |
| T9 mesher, meshcheck | Partly | uniform sparse grid with exact Hermite data, multi-patch cells, clamping and a Newton projection; every tier watertight, manifold, free of self-intersections and in budget (benchmark test); not built: adaptive octree, budgeted refinement by curvature, screen size and importance, local re-extraction (a failing tier is re-meshed whole at a finer global cell, compile.rs:195); not measured: Hausdorff, the 5% time share, the geocheck cross-check. The report lists only the last three. |
| T10 LODs, collision | Partly | detail dropping by pixel size with reasons; capsule footprint computed from the extents and within 5 cm of the kit's numbers; silhouette protection not built; proxy excess reported but not tested against a class tolerance |
| T11 normals, tangents, maps | Partly | MikkTSpace-convention tangents, exact field bake, seam measure, skyline packer; charts are six-axis projections per part, not natural parameterisations; no conformal fallback; texel density within 5% not tested (the density is lowered when the map size cap is hit, surface.rs:185); Blender tangent agreement not run |
| T12 glTF | Done | quantised `.glb` per tier, part tree, anchors, collision node, `extras.evani`; validator clean on 120 files; byte identity across runs, threads and wasm32 shown by my runs (the builders' own test is weaker, should-fix 5) |
| T13 CLI, MCP | Done | two-call kart compile and reply sizes verified above; `variants` mismatch (should-fix 2) |
| T15 preview sheets | Partly | blueprint only; `variants` and the reference check sheet not built; drift test exists; under-1-s, wasm32 and Blender checks not shown |
| T14 acceptance | Partly | kart compiles to valid glTF with LODs, anchors and collision; locality test as below; originality scan not run (must-fix 3); musts per tier (must-fix 1) |

### Edit locality (`editing_one_part_leaves_every_other_compiled_piece_byte_identical`, compile.rs:307)

What it proves: for two edits on `kart_01_pip` (rear tyre width; wing span, which its struts follow), every free measure outside the edited parts keeps its exact bits, every other part's field graph and placement prints identically (f64 Debug round-trips every bit), the edit did change something, and at one fixed cell size (4 cm) the untouched solids mesh to identical bytes while the edited solid changes. That is what plan-m1 asks for "at compiler level".

Limits: (1) per tier, the cell size comes from the whole model's triangle count (compile.rs:179), and the minimum thickness (1.6 cells) and dropped parts follow from it, so in the real `.glb` output an edit usually changes every solid's bytes; locality holds for the pieces, not the files. (2) Locality is per solid, not per part: the wing edit re-meshes the whole body solid. (3) Two edits on one item; no edit that pushes against `fit_box`, where Levenberg-Marquardt moves free measures of the whole model by design. (4) Anchors, collision proxies and dropped-part lists are not compared. The report's gap 6 states (1) fairly. Acceptable for M1 under D15; the output-level test belongs to M3.

### The low-tier "known bug" (report section 2 note, gap 9)

Cause: `Ctx::finest_mesh` uses `tiers.last()`, and `Workspace::tiers` (api.rs:73) compiles all four tiers (line 79) and then keeps only the ones asked for (line 89), so with `--tiers low` the low mesh is the "finest". The low kart mesh then fails `stands` as measured: its lowest point is 2.0 mm up (limit 2 mm) and the contact set (vertices within 5 mm of the lowest) misses one axle, so the centre of mass falls outside it. This is a real property of the low mesh (coarse wheels, about one cell of error), and a real gap in the design (coarse tiers are never judged in the normal run; the tolerances do not scale with the tier), not a reporting quirk. See must-fix 1.

### Blueprint pictures (docs/model_research/m1/)

- **kart-blueprint.png**: reads as a go-kart: small front and big rear wheels, front bumper bar, nose panel, steering column and wheel ahead of a tall seat back, a raised flat wing on two struts and two exhausts at the back (top view, front at the top). Odd keyhole-shaped marks on the hub faces in the side and three-quarter views.
- **chibi-body.png**: reads as a chibi figure: big round head, short body, stub arms, two legs with blocky feet. No hands or face (shape only). The upper arms fuse into the torso (too wide a shoulder blend), with a dark smear across the chest in the front view, so arm and body cannot be told apart there (should-fix 7).
- **house.png**: reads as a small house: gable roof with overhang and ridge, slightly leaning chimney, arched door, three round-top windows on the long side. As the report says.
- **tower.png**: reads as a castle tower: flared base ring, tapered round body, crenellated top, tall cone spire, arched door, slit windows. Some speckle on the merlons in the top view and a small notch at the door's left edge.

### Gaps listed in the report

| Gap | Carry past M1? | Why |
|---|---|---|
| wasm32 determinism | Yes, mostly closed | my run shows native and wasm32 give identical bytes for four items; fix should-fix 1 so the musts run there too, and add the run to the test suite in M2 |
| Blender checks (tangents, silhouettes) | Yes | external checks the plan put "run separately"; needed before M4 (relief), where tangents matter |
| geocheck cross-check of meshcheck | Yes, until M5 | meshcheck has its own passing and failing tests; geocheck agreement matters when models enter the game (M5, evani-convert) |
| 22 non-kart references moved to M2 | Yes | D12 and D16 said "pictures in M1", so tell Chris plainly; M2's IoU target is about karts, and the other items need references before M3 calibration |
| Originality scan not run | No | the brief requires it before every sign-off (must-fix 3) |
| Blueprint 3.2 s against under 1 s | Yes | not on the M1 gate line; measure on an idle PC in M2 |
| Detail parts dropping on big props (global cell size) | Yes, with must-fix 1 | it also causes the fattened low and medium tiers above; per-part cell size and the octree (T9) are the fix |
| Uncalibrated style | Yes | calibration is M2's job by plan |
| Single-tier musts | No | see must-fix 1 |
| Not timed or counted (solver, meshcheck share, Hausdorff, MCP reply length) | Yes | reply length now measured (under 400 tokens); the rest are speed and accuracy numbers that M2's targets will need |

## For the follow-up worker
Must-fix 1 needs an Opus-level worker (deciding per-kind tier tolerances touches the must design); must-fix 2 and the should-fixes are routine. Must-fix 3 needs a separate scanning agent, not a builder.
