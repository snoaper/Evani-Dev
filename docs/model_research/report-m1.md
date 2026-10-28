# Evani.Model M1 report: the compiler

Branch `evani-model`, worktree `..\evani-dev-model`. Written for Chris's sign-off. Every claim below names its evidence; every gap is listed in section 5. Revised after the independent review (docs/model_research/review-m1.md, "approve with conditions"): musts are now judged on every tier, and the claims the review found unsupported are corrected here.

## 1. What M1 delivers

A kart (or any of the 30 benchmark items) written as a short `.evmodel` intent compiles, on the CPU and with no search, to one `.glb` per quality tier. Each tier carries the part tree, named anchors, a collision node and `extras.evani` metadata. The same intent always gives the same bytes, on any thread count. Plan-m1.md asks for this ("a kart written as intent compiles to valid glTF with LODs, anchors and collision, passing section A"); the table in section 3 says which part of section A is shown and which is not.

Delivered: the `evani-model` library, the `evani-model` command (`make`, `compile`, `check`, `sheet`, `describe-system`, `mcp`), a stdio MCP server, the reference tool `model-bench-ref` with the kart references, 24 class templates, the draft `toon_chunky` style, and 4-view blueprint sheets of the compiled kart, a character, a house, a tower and a building (`docs/model_research/m1/`).

## 2. Kart acceptance (T14): `kart_01_pip`, dev PC, CPU path, dev build with opt-level 3 for the crate

Measured 2026-10-15 with `evani-model compile tests/model_bench/vehicles/kart_01_pip.evmodel --tiers <tier>`, which now compiles that one tier only (`Workspace::tiers` builds just the tiers asked for). Default 8 threads, below-normal priority, one run each, on a shared PC whose CPU was 56 to 92% busy with other jobs; differences under a second are noise. The time is the compile reply's `ms`: the whole compile of that tier, including its musts, mesh checks, normal maps and files.

| Tier | Cell | Triangles | Budget | File size | Time | Musts | Stands: lowest point | Normal-map overlaps | Dropped parts |
|---|---|---|---|---|---|---|---|---|---|
| low | 126.7 mm | 2,830 | 3,000 | 66 KB | 1.8 s | 7/7 | +2.0 mm (allowed up to +63.4 mm) | 0 | steering column, steering wheel |
| medium | 73.3 mm | 7,514 | 8,000 | 137 KB | 2.3 s | 7/7 | +0.5 mm | 18 | none |
| high | 45.4 mm | 17,610 | 20,000 | 309 KB | 1.7 s | 7/7 | +0.1 mm | 1 | none |
| ultra | 25.0 mm | 51,298 | 60,000 | 830 KB | 2.8 s | 7/7 | 0.0 mm | 0 | none |
| all four in one run | | as above | | | 5.2 s | 7/7 on every tier | | 19 | |

"Musts" counts, for each tier, the musts judged on the exact field plus that tier's own mesh results (`make`, `check` and `compile` replies all show `tiers: {low: "7/7", ...}`). A tier compiled alone is measured exactly as in a four-tier run (unit test `musts_are_judged_per_tier_and_a_subset_measures_the_same`).

Other facts for this kart: 9 anchors, 17 collision proxies, every tier watertight, manifold and free of self-intersections, normal maps with a worst bake error of 0.4 degrees and a worst seam error of 0.6 degrees. 19 texels of the medium and high normal maps are claimed by two triangles of the same chart (a small fold where a flattened chart crosses itself); the count is pinned by the test `kart_compiles_to_identical_bytes_on_1_and_8_threads` and listed as a gap in section 5.

All 30 benchmark intents, all four tiers (120 files), compiled to a scratch folder and run through the Khronos glTF-Validator (gltf-validator for Node): 120 files, 0 errors, 0 warnings. Whole-item compile time (four tiers) was 0.5 to 5.6 s in that run; triangle counts at ultra range from 2,438 (sword) to 52,964 (kart_02_mo), all within budget. Must results per tier are in section 2b: 24 of the 30 items pass every must on every tier; 6 items fail on a coarse tier (9 tier results), listed by name.

Blueprint: `evani-model sheet tests/model_bench/vehicles/kart_01_pip.evmodel --out docs/model_research/m1/kart-blueprint.png` (front, side, top, three-quarter at ultra). `sheet` now compiles only the finest tier; the picture is byte-identical to the one committed before, and the command took 1.3 s from start to exit on the busy PC (plan target: under 1 s on 8 threads; not shown, section 5).

The sheets check shape and proportions only. They are plain flat-shaded geometry pictures, not a look target; the chunky toon look comes with style calibration later. Sheets of a character, a house, a tower and a building are in the same folder (`chibi-body.png`, `house.png`, `tower.png`, `building.png`, regenerated 2026-10-15 from the current templates); the mesh artifacts they show are listed in section 5.

## 2b. How the musts are judged, and the results per tier

Before the review, the mesh-based musts read only the finest tier given (`tiers.last()`), so `--tiers low` silently changed what was measured and the coarse tiers were never judged. Now each must kind has a fixed scope (`must::scope`, module notes in crates/evani-model/src/must.rs), and every tier is judged on its own with a tolerance taken from its own cell size `h`, never more. The mesher puts one vertex per cell on the surface, so sampling can lose an extreme by up to half a cell, but cannot put one outside the surface except by the vertex-merge tolerance (the "merge slack", at least 1 mm).

| Must kind | Judged on | Tolerance on a tier | Why that tolerance |
|---|---|---|---|
| `clear`, `swing_clear`, `wheels_clear_chassis_at_steer`, `seats`, `symmetric`, `anchor` | exact field only | none (field maths) | they measure the designed shape; meshing a tier does not change it |
| `stands` | every tier | lowest point from -max(2 mm, merge slack) to +max(2 mm, h/2); contact = every vertex within one cell of the lowest; centre of mass over the contact within one cell | a tier can miss the true lowest point by up to h/2 (one vertex per cell), but a vertex below the ground is the tier's own shape and fails |
| `min_thickness` | every tier | none | measured on the tier's own field (parts thickened to the tier's minimum feature, dropped details left out) at each vertex moved onto that field, at most one cell diagonal (√3·h) away |
| `tri_budget` | every tier | none | a count |
| `fit_box`, `max_size` | exact field and every tier | the tier's box may exceed the limit by the merge slack only | vertices sit on the surface; growth beyond the slack is real thickening |
| `min_size` | exact field and every tier | the tier's box may fall short by h/2 | a rounded extreme can be cut by up to half a cell |
| `connected` | both | none | joint gaps on the exact field; one piece per solid on each tier |

**Results (benchmark test `every_benchmark_intent_compiles_passes_its_musts_per_tier_and_meshes_clean`).** Pass on all four tiers: 24 items, namely all 8 karts (kart_01_pip to kart_08_rumble), house_small, tower, bush, cactus, candy_cane, chibi_body, cloud, gumdrop, lollipop, mushroom, rock, tree, barrel, bench, chest and lamp_post.

Known gaps: 9 tier results on 6 items. All come from one cause: each tier has one cell size for the whole model, and every part is thickened to at least 1.6 cells, so on a coarse tier thin parts of small props grow past their box or into the ground. Per-part cell size and the adaptive octree (T9, deferred to M2) are the fix. The test lists each one as an expected failure with its reason, and fails on any other failure and on a listed gap that starts passing.

| Item | Tier | Must | Measured | Limit |
|---|---|---|---|---|
| crate | low | `stands` | lowest point -26.9 mm (slats thickened to about 281 mm on a 175.8 mm cell) | -2.0 mm to +87.9 mm |
| door | low | `fit_box` | 0.369 m deep | 0.2 m (+ merge slack) |
| door | medium | `fit_box` | 0.256 m deep | 0.2 m |
| fence | low | `fit_box` | 0.318 m deep | 0.15 m |
| fence | medium | `fit_box` | 0.223 m deep | 0.15 m |
| shield | low | `fit_box` | 0.240 m | 0.18 m |
| shield | low | `max_size` | 0.240 m | 0.18 m |
| sign | low | `fit_box` | 0.323 m deep | 0.3 m |
| sword | low | `fit_box` | 0.124 m deep | 0.1 m |

For comparison, the review judged every tier with the old fixed limits and counted 26 failing tier results on 17 items (33 failing must results). The difference is the per-tier tolerances above: the karts' low tiers float 0.9 to 4.3 mm, within half their roughly 127 mm cell, and the ones that seemed to rest on one axle had their other wheels' lowest vertices within one cell of the ground, which is as close as that tier can place them; and the sword's thin blade is measured on the field its tiers were actually meshed from (thickened to the tier's minimum feature), not on the exact one. No must was loosened beyond what the tier's cell size justifies, and what a tier itself changes (thickening into a box or into the ground) still fails.

## 3. Tasks T1 to T15 against their section A tests

Commits are in `git log --oneline 8795ae1^..HEAD` (the M1 range; the earlier commits in `dfc9ae3..HEAD` are M0 and merged branches). Test results are in section 4. Note on 6483bfe: its message says every intent passes every must "on every tier"; at that commit the musts were judged on the finest tier only. a6666da judges them on every tier (section 2b).

| Task | Done | Evidence (commit, test) | Not shown or deferred |
|---|---|---|---|
| T1 crate skeletons | Yes | 8795ae1; Cargo.lock gains only `evani-model`, `evani-model-tool`, `model-bench-ref`; no third-party crate | the `gpu` feature (M2) |
| T2 bench reference tool, kart references | Kart half | 1137624; tests `base_kart_matches_the_manifest_size`, `masks_are_symmetric_and_repeatable` | The 22 non-kart `assembly.json` files (moved to M2, section 5) |
| T3 maths, hashing, tile map | Mostly | 8795ae1; tests `sin_cos_within_2_ulp`, `atan2_within_2_ulp`, `exp_ln_within_2_ulp`, `pow_cbrt_relative`, `standard_vectors`, `streaming_equals_one_shot`, `tiles_merge_in_order_for_any_thread_count` | wasm32 run in our own test suite (section 5) |
| T4 intent parser | Yes | 6b41da2; tests `all_benchmark_intents_parse_validate_and_round_trip`, `golden_errors`, `canonical_order_and_hash_ignore_source_order` | |
| T5 SDF graph | Mostly | 6d35882; tests `exact_distances_for_simple_primitives`, `exact_primitives_have_unit_gradients_matching_finite_differences`, `domain_op_gradients_match_finite_differences`, `lipschitz_bound_never_violated`, `bounds_contain_the_surface`, `prepared_fields_give_identical_samples` | subdivision cages (deferred); wasm32 identity in our suite |
| T6a, T6b generators | Yes | 32bfcaf; tests `every_generator_builds_with_surface_anchors_bounds_and_proxy`, `seeded_generators_repeat_and_vary`, `min_feature_thickens_thin_parts` | |
| T7 solver and musts | Yes | ab994d9, 0cf6f9f, a6666da; tests for every kind, including `a_one_millimetre_joint_gap_is_found`, `steer_clearance_finds_the_angle_of_contact`, `seats_pass_and_a_seat_2cm_too_narrow_fails`, `lm_solves_a_small_cycle_and_respects_bounds`, `order_puts_dependencies_first_and_finds_cycles`, `musts_are_judged_per_tier_and_a_subset_measures_the_same` | Solve times (0.1 ms, 2 ms) not timed on their own |
| T8a kart template, draft style | Yes | ab994d9; tests `the_base_kart_fits_its_benchmark_box`, `base_kart_intent_is_within_085_iou_of_the_reference` | |
| T8b other templates and six additions | Yes, with gaps | 0bcf7b7, a5dc1b6, fb5ce41, 3fced24, e32c3be, 042b351; the benchmark test compiles all 30 intents and checks every must per tier | 9 coarse-tier must failures on 6 items (section 2b); style is a draft |
| T9 mesher and `meshcheck` | Partly | 7e35750, 1135cf3; tests `sphere_is_closed_manifold_and_accurate`, `box_keeps_sharp_corners`, `thin_and_touching_shapes_stay_manifold`, `identical_for_any_thread_count`, `every_issue_kind_is_found`, `self_intersections_are_found_and_touching_is_not`; benchmark test: every item on every tier watertight, manifold, no self-intersections, within budget | The mesher is a uniform grid per tier, not the adaptive octree; budgeted adaptive refinement (curvature, screen size, part importance) and local re-extraction in the validator are deferred; Hausdorff distance per tier, meshcheck's 5% time share and agreement with geocheck were not measured |
| T10 LODs and collision | Mostly | 30706ff; tests `the_kart_compiles_every_tier_within_budget_and_clean`, `kart_proxies_cover_their_parts_and_the_footprint_matches_the_kit`; dropped parts are listed in the compile reply | silhouette protection when a tier drops or merges parts (deferred) |
| T11 normals, tangents, bake | Partly | e0ced05; tests `rounded_box_bakes_a_valid_seamless_map`, `packer_places_without_overlap`; kart bake error 0.4 degrees, seams 0.6 degrees | natural-parameter charts, the conformal fallback and the 5% texel-density test are deferred; 19 overlapping texels on the kart (section 2); agreement with Blender's tangents |
| T12 glTF writer and reader | Mostly | 30706ff, 38ff7b0; tests `kart_tier_round_trips_and_is_byte_identical`, `kart_compiles_to_identical_bytes_on_1_and_8_threads` (end to end: every `.glb`, normal map and metadata file byte-identical at 1 and 8 threads); Khronos validator on 120 files: 0 errors, 0 warnings | wasm32 identity in our suite (the reviewer's own run, section 5) |
| T13 CLI and MCP | Yes | f60e792, a6666da; test `initialize_list_and_errors` (errors carry a fix line); the kart compiles in two calls (`model_make`, `model_compile`); per-tier must results in `make`, `check` and `compile`; `.evmodel.json` stores file names, not paths | Reply length under 400 tokens not counted by a test |
| T15 preview sheets | Partly | f60e792; tests `identical_pictures_for_any_thread_count`, `masks_measure_area_and_iou`, `checksums_match_published_values`, `round_trip_including_multi_block`, `the_toon_ramp_has_not_drifted` | the `variants` kind and the reference check sheet are deferred (`model_sheet` no longer offers `variants`); under-1-s timing, wasm32, Blender silhouette comparison |
| T14 acceptance | Yes, with gaps | 6483bfe (benchmark test and edit-locality test), a6666da (per-tier musts); section 2 | Originality scan: run separately (section 6) |

Edit locality (T14): the test `editing_one_part_leaves_every_other_compiled_piece_byte_identical` edits a rear tyre width and then a wing span on `kart_01_pip`. Every other part's field graph and placement stays identical, and the untouched solids mesh to identical bytes at a fixed cell size.

## 4. Other checks run for this report

- `cargo test -p evani-model -p evani-model-tool` (2026-10-15): 61 library tests, the benchmark test, the determinism test and the command test pass, 0 failed (5 measuring probes are ignored). `cargo test -p model-bench-ref`: 4 passed.
- `cargo clippy -p evani-model -p evani-model-tool --all-targets -- -D warnings`: clean.
- `cargo xtask check`: `evani check: clean` (3 warnings that belong to the engine games, not to Evani.Model).
- No `cargo fmt` run; no `unwrap` or `expect` outside tests (clippy denies them). The musts take the caller's thread count, so on one thread (as on wasm32) they spawn no thread; the build step's surface samples use one thread on purpose (`build()` takes no thread count, and one thread is safe on wasm32).
- DECISIONS.md records the outcomes of D1 (approved, unused until M2's GPU feature), D2 (a hand-written serde_json MCP server, no rmcp or tokio) and D17 (option A, our own `meshcheck`); no new crate.

## 5. What was not done (stated plainly)

1. **wasm32 determinism is not in our test suite** (T3, T5, T12, T15). The reviewer built the pipeline for `wasm32-unknown-unknown`, ran it under Node on one thread and got the same SHA-256 as the native build for kart_01_pip, house_small, rock and chibi_body, with the musts left out because they then spawned threads; they now run on the caller's thread count, but that run was not repeated here. Identity across thread counts is tested natively, now end to end (`kart_compiles_to_identical_bytes_on_1_and_8_threads`).
2. **Blender checks were not run**: tangent agreement, silhouette agreement, and the geocheck cross-check of meshcheck.
3. **The 22 non-kart reference assemblies (T2's data half) are moved to M2.** What is missing is the silhouette comparison against a reference for the non-kart items.
4. **Coarse-tier must failures**: 9 tier results on 6 items (crate, door, fence, shield, sign, sword), named with their numbers in section 2b. Cause: one cell size per tier and a 1.6-cell minimum feature. Fix: per-part cell size and the adaptive octree (M2).
5. **Detail parts drop on big props.** The smallest feature a tier keeps is 1.6 times its cell size, and the cell size is one number for the whole model. On the door, grooves and hinges drop even at ultra. Same fix as item 4.
6. **Mesh locality holds only at a fixed cell size.** Each tier's cell size is global (it comes from the triangle budget), so editing one part can nudge other parts' triangles when the cell size changes. The compiled pieces (graphs and placements) stay identical; the test checks that, and the bytes at a fixed cell size.
7. **Plan items deferred to M2 or later and not built in M1**: T5 subdivision cages; T9 the adaptive octree (M1 meshes a uniform grid per tier), budgeted adaptive refinement and local re-extraction in the validator; T10 silhouette protection; T11 natural-parameter charts, the conformal fallback and the 5% texel-density test; T15 the `variants` sheet kind and the reference check sheet.
8. **19 overlapping texels in the kart's normal maps** (18 on medium, 1 on high): small folds where a flattened chart crosses itself. Pinned by a test so a change cannot go unnoticed; the conformal fallback (T11, item 7) is the fix.
9. **Mesh artifacts seen on the blueprint sheets** (shape faults of the mesher or the templates, not of the pictures' lighting): wedge-shaped shading on the tower roof seen from the top; small dark speckles between the tower's battlements in the top view; a crease (a thin stray sliver) beside the tower's door arch; a line across the house gable at eave height; bumps under the right roof-slope edge in the house's front view; the character's shoulder blend still reads as a dark smear across the chest in the front view (the arms were changed in 042b351 but still merge at the shoulders; part blend radii are calibrated in M2); an odd keyhole-shaped mark on the kart's hub faces in the side view; on the building's side view the middle windows are outlined on three sides only, and in the three-quarter view most window recesses show only two of their edges (not yet traced to the mesh or the sheet's outline pass).
10. **`toon_chunky` is uncalibrated.** Its word ranges are drafts; calibration against the references comes later.
11. **Blueprint speed**: 1.3 s from start to exit on a busy PC against a plan target of under 1 s on 8 idle threads. Not measured on an idle machine.
12. **Not measured**: solver time, meshcheck's share of compile time, MCP reply length, and the Hausdorff distance between each tier and its field. A probe while fixing the musts found one sword vertex on the medium tier about two thirds of a cell off its field at the guard, which a Hausdorff check would report.

## 6. What Chris approves

Tick the M1 line in `STAGES.md` (Evani.Model track): "a kart written as intent compiles to valid glTF with LODs, anchors and collision, passing section A", on the basis of sections 2 and 3 and knowing the gaps in section 5. Only Chris signs off; no agent ticks that line.

The originality scan the brief requires before every sign-off (plan-m1.md section I) is run by a separate agent that did not build M1; its full record is in its own file, `docs/model_research/originality-m1.md`; a short summary follows. The scan covers M1 up to commit 6856e9d, the last change to code or assets: **pass with notes**, nothing copied, no third-party licences, assets or generator output. One decision is Chris's: the SHA-256 round in our code matches the published standard, as any correct implementation does; it was kept rather than rewritten as the plan's rule for over-limit matches asks, and keeping it is Chris's call (originality-m1.md, note 6). The scan record itself was written by the scanner in commits bbc54d2 and f3ef8da; `git diff --stat 6856e9d..HEAD` shows that every commit after the scanned one changes only this report and that record. The merge into `m1-foundation` is a separate step after sign-off.