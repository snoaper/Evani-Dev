# Evani.Materials: batch 5 build plan (horses, livestock, dogs and cats)

**In plain words, for Chris.** Batch 5 adds **95 animal materials**: 37 horse coats, manes, hooves, muzzles and the horse eye, 28 farm animals (cattle, sheep, goats, pigs, donkeys, llamas, rabbits) and 20 dog and cat coats. None work yet, but **50 can be set right now** on the painted fur the engine already has (plain coats, stripes, spots, dapples, pinto patches). 23 need new coat looks: **roan** (white hairs mixed in), **merle**, **tortoiseshell and calico**, **woolly fleece**, plus bristly pig skin, striped hooves and a bar-shaped horse pupil. The last **22 need a "body-region mask"**: the engine has to know where an animal's belly, legs, face, back and tail are, so a buckskin gets black legs, a tuxedo cat a white chest and a dun horse its back stripe. That is the batch's one big new engine piece and it needs **research first**, because no animal model exists in the project yet and the choice decides how every animal model is made. The work is **11 packages and one research step**; three packages and the research can start today.

Status: design (2026-10-25). **B5-P1 (fur variants) and B5-P2 (hide and keratin) are built and joined into `matcat-engine`** (2026-10-25; see the progress note here and the tally at the end); the other packages are not. Builders take one package at a time.

**Progress (tally).** B5-P2 (hide and keratin) is built on branch `b5-p2` (2026-10-25). **12 rows are drawn early as its proof:** the 8 batch-5 rows it serves (pig.skin pink, black, spotted; horse.muzzle dark, pink; horse.hoof dark, pale, striped) and 4 batch-6 rows (rhino.hide, walrus.hide, turtle.shell, armadillo.shell). Of the 23 rows that need a new coat or part look (section 2), 8 are drawn by B5-P2 and 14 by B5-P1 (section 11); 1 waits: horse.eye.brown (B4-P1). Hide is 1,101 B gzip (budget 1.2 KB), keratin 1,198 B (budget 1.2 KB). Running total: 519 before batch 5, 527 with B5-P2's batch-5 rows; batch 6's four add 4 more (see `BATCH6_BUILD_PLAN.md`). Checks: `cargo xtask check --full` clean; byte proof 16 of 16 `same` against `sha-base-16afb434.txt`. Numbers: DECISIONS "B5-P2".

Inputs read for this plan: as `BATCH4_BUILD_PLAN.md`, plus `rc_fur.wgsl`, `rc_hide.wgsl`, `rc_keratin.wgsl` on `matcat-engine`, batch 1's P15 verdicts (the body-region list, the horse coat, fur shells PF) and the fur frame cost measured in P15.

## 1. Stage check

- **Stage 3 (Real content)**, inside the catalogue's Stage 3 Includes line. No gate box.
- **Fur stays painted.** 3D fur shells (`fur-shells`, PF) are a **Stage 8 item**; batch 1 pencilled PF in "before batch 5" only if Chris moves it to Stage 3, and he hasn't. This plan does not need shells: every row here reads by its colours, pattern and regions, which the painted coat draws the same on every device. The choice is put to Chris in the hand-over (section 10, risk 1); nothing here waits for it.
- **The fur, hide and keratin packages serve batch 6 too** (one owner per shader file): batch 6's giraffe, rhino, walrus, armadillo and turtle are built here. So batch 6 has no build package of its own besides proof shapes (see `BATCH6_BUILD_PLAN.md`).
- **Not in batch 5:** shells (Stage 8), hoof prints, saddles and tack (models), wet-coat animation (vertex work).

## 2. Scope: the 95 rows

| Subcategory | Rows | Recipe | Numbers now (3.1) | New coat or part looks (3.2) | Body regions (3.3) |
|---|---|---|---|---|---|
| horse_coats | 37 | fur 37 | 21 | 5 roan: grey_steel, red_roan, blue_roan, bay_roan, rabicano | 11: blood_bay, buckskin, dun, red_dun, grullo, mealy_bay, splash, spotted_blanket, sabino, sooty_bay, brown |
| horse_parts | 10 | hair 4, keratin 3, hide 2, eye 1 | 4 manes | 6: hoof dark, pale, striped (keratin `hoof`); muzzle dark, pink (hide `bristles`); horse.eye.brown (B4-P1's bar pupil) | 0 |
| livestock | 28 | fur 24, hide 4 | 14 | 8: cattle.coat.speckled (tortie mottle), sheep.wool black, brown, grey, sheep.skin.shorn (crimp); pig.skin pink, black, spotted (bristles) | 6: cattle red_whiteface, sheep.face.black, goat.coat.brown, pig.skin.belted, donkey grey, donkey brown |
| dogs_cats | 20 | fur 20 | 11 | 4: dog merle, dog curly_brown (crimp), cat calico, cat tortoiseshell | 5: dog black_tan, sable, tricolour; cat tuxedo, pointed |
| **Total** | **95** | fur 81, hide 6, hair 4, keratin 3, eye 1 | **50** | **23** | **22** |

50 + 23 + 22 = 95. Running total: **519 drawn before batch 5** (after batch 4), **614 after it**, of 1,054.

The region list follows batch 1's P15 scan (look notes keyed on belly, face, legs, back, rump, tail, chest), so rows the scan named stay region rows even where the existing `points` number (dark thin parts by curvature) could fake them: a buckskin's black legs drawn by curvature would also blacken its ears and muzzle.

## 3. What covers them

### 3.1 Numbers now (50 rows, no code)

On today's fur (variants solid, stripes, spots, patches, rosettes, dapple; base, tip and pattern colours; `points` for dark extremities by curvature), hair (family) and nothing else:

- **Horse coats (21):** dark_bay (points), chestnut, liver_chestnut, sorrel, flaxen_chestnut, black, smoky_black, chocolate, white, cremello, perlino (points, rusty), champagne (sheen), winter_shaggy (length), brindle (faint stripes), spotted_leopard (spots), spotted_snowflake (small white spots), tobiano_bay and tobiano_black (patches), overo_chestnut (patches, jagged), silver_dapple (dapple), grey_fleabitten (small red spots on white).
- **Manes (4):** black (blue highlight), flaxen, white, red on the family's hair.
- **Livestock (14):** cattle red, pied, dun (points), shaggy_red, cream, brindle; goat white, black, pied, long; mule.coat.bay; llama cream and brown (dense solid); rabbit.fur.domestic.
- **Dogs and cats (11):** dog black, brindle, spotted, golden_long, white_fluffy, wiry_grey; cat orange (soft stripes), black, white, grey, longhair.

### 3.2 Extensions of existing recipes (no new engine capability)

**Fur (B5-P1), one package for batches 5 and 6** (`rc_fur.wgsl` has one owner). Fur today: 1,021 B gzip, 9 of 10 number slots used, variants 0 to 5.

| Addition | What it draws | Rows (batch 5, batch 6) |
|---|---|---|
| variant **roan 6** | white (or colour C) hairs mixed strand by strand into the coat at the share `mix`; the pattern layer still works on top | grey_steel, red_roan, blue_roan, bay_roan, rabicano (ticking on the flanks: `mix` low plus the pattern); later the solid roan head comes from the face region |
| variant **merle 7** | ragged marbled patches of colour C over a diluted coat (colour B as the dilute), low-frequency fbm with torn edges | dog.coat.merle |
| variant **mottle 8** | tortoiseshell and calico: fine to coarse patches in **two** pattern colours, B and C, over A (the tip colour is free here because these coats have no tipping); calico is A white with coarse patches | cat tortoiseshell, calico; cattle.coat.speckled |
| variant **crimp 9** | wool and tight curls: crimped staple locks (a wavy stripe along each lock, lock cells from `clump`), dull, soft dark gaps between locks; `length` sets the staple | sheep.wool black, brown, grey; sheep.skin.shorn (short, pink colour C between locks); dog.coat.curly_brown |
| variant **reticulated 10** (for batch 6) | large polygon patches of colour C split by lines of the base colour (F2-F1 of a cellular noise), the giraffe | giraffe.coat (ready today as `patches`; re-pointed and shown on batch 6's review card) |
| number **mix** (the last free slot) | roan share, merle dilution, mottle balance, crimp gap, by variant | as above |

- Wet coats (seal, otter, beaver in batch 6) need no fur change: the wet layer's default amount plus sheen.
- Budget: fur ≤ 1.75 KB (≤ +730 B), leaving room for the region (3.3) inside its 2.5 KB class.

**Hide and keratin (B5-P2), one package for batches 5 and 6.** Hide 755 B (4 free slots), keratin 842 B (5 free slots).

| Recipe | Addition | Rows (batch 5, batch 6) |
|---|---|---|
| hide | extra **bristles** (sparse short hairs along a flow: pale on pink, black on dark; colour B; number `bristles`) | pig.skin pink, black, spotted; horse.muzzle dark (whiskers), pink (grey freckles are `spots`); batch 6's walrus.hide |
| hide | variant **plates 4** (thick plates with deep folds: large cells, fold lines, heavy relief; number `plate_scale`) | batch 6's rhino.hide |
| keratin | variant **hoof 6** (fine growth lines along v, a slight translucent sole edge; number `bands` gives dark and pale stripes round u) | horse.hoof dark, pale, striped |
| keratin | variant **scutes 7** (polygon plates with growth rings radiating from each plate's corner) | batch 6's turtle.shell |
| keratin | variant **banded 8** (rings of small plates across v with soft joints) | batch 6's armadillo.shell |

- Budgets: hide ≤ 1.2 KB (≤ +450 B), keratin ≤ 1.2 KB (≤ +360 B): both stay class S.

**Eye:** horse.eye.brown uses B4-P1's bar pupil (batch 4). Nothing new here.

### 3.3 New capability, research first: the body-region mask (B5-R1, then B5-P3)

P15's verdict: "one helper: a region number plus a second colour, taken from uv v or the vertex colour; the region convention should be designed against the horse batch's models, not a ball." About 50 rows wait on it across batches 1, 5, 6, 7 and 11 (22 here, 23 in batch 6, deer.coat.brown in batch 1, about 20 birds, fish and snakes in batch 7, the 13 optional stripe rows in batch 11), so the choice is made once, for all of them.

**Why it can't be built straight away:** the project has **no animal model** (no horse, cow, dog or cat glb in any branch), the fragment stage has **no object space** (a world-space "down is belly" breaks when a horse lies down or rolls), the vertex colour is already used as a tint (the recipes build their colour from it), and the 32-byte block has **one free fur slot**, so where the region's settings live is a real constraint.

**Research questions B5-R1 must answer** (documents, a throwaway spike on a scratch branch, and one quick probe build at most; written to `.vibedeck/research/` and as a DECISIONS proposal):

1. **Where the region comes from on a real model.** Compare: (a) the vertex colour (conflicts with tinting?); (b) a second vertex attribute (`COLOR_1` or a custom `_EVANI_REGION`), which changes the model pipeline's vertex layout; (c) **derived at convert time** from the skeleton (bone names for head, neck, legs, tail, spine) and the rest-pose normal (belly), baked into (a) or (b) by `cargo xtask convert`; (d) a uv convention (u round the body, v nose to tail); (e) a small per-model region texture; (f) world normals at run time (no data). For each: download bytes per model, authoring work, what a skinned animal does when it lies down, what a Meshy-made model (Chris's paid, owned models, auto-rigged) gives without hand work, and what a hand-painted override in Blender looks like.
2. **Which regions, how many.** Map every waiting row in batches 1, 5, 6, 7 and 11 to the smallest region set. A starting list: underside or belly, back line (dorsal stripe, saddle), face and muzzle, eye ring, lower legs (socks), chest and throat, rump, tail and tail tip, ear tips, shoulder band (belted pig, donkey cross), flank. Is 8 enough (one byte a vertex), or are soft weights needed (RGBA8, four regions at once)?
3. **Edges.** Countershading and mealy bays are soft gradients; a tuxedo or a pinto edge is sharp; a sabino edge is ragged and speckled. One softness and one noise break-up per material, or per region? How do edges anti-alias at every tier (the 1.5 px fade rule)?
4. **Where the material's region settings live.** Which regions take which colour, edge softness, noise: free number slots (fur has one after B5-P1 takes `mix`, so zero), the variant byte's high bits, the seed byte, the 8 condition-default bytes, a second block (binding 12 from 32 to 48 bytes: an engine and patch change), or a small per-recipe table? Bytes and compatibility for each. Which colour a region paints when fur already uses A, B and C (for example: a region paints colour C, and a pattern variant that also needs C gives way inside that region).
5. **Skinning and tiers.** Confirm the chosen data rides through the skinned vertex stage unchanged and costs nothing in the fragment stage beyond one read; it must draw the same at Lite as at Ultra (the same look on every device), and add no pipeline (the tier-switch rule).
6. **Shared with people, birds, fish and cars.** Does one convention cover a head (batch 4's rosy cheeks, kohl, under-eye shadow, stubble), a bird (head, wing, breast), a fish or dolphin (belly, back, fins), a snake (bands along the body: v) and a car (stripes, batch 11)? Or one mechanism with per-body region names? Batch 6 adds four questions of its own (bodies without legs, regions that change length rather than colour, patterns placed along a region's centre line, ringed tails and face masks): `BATCH6_BUILD_PLAN.md` section 3.3. B5-R1 answers them too.
7. **Converter and check.** `cargo xtask convert --regions auto|<map.json>`: a bone-name dictionary (common rig naming: Rigify, Mixamo-style, Meshy's auto-rig) to regions; the Blender add-on's override layer (`evani_region`); `rig_check.py` additions. Check rules: a region row on a model without regions (warning: draws the base coat, which must look acceptable), regions on a model whose game doesn't list the feature (error with `fix:`).
8. **Test models.** Recommend the proof set: a code-built, rigged quadruped (capsule body, legs, neck, head, ears, tail; proportions for horse, cow, dog, cat) generated in Rust for the sandbox and the viewer (no licence question, deterministic), plus one real rigged animal for the review (Chris's own Meshy model if one exists and its licence allows the repo; otherwise a model made with the project's Blender tools).
9. **Byte proof and budgets.** If the vertex layout changes, how a game without the feature stays byte-identical (a cfg'd sub-feature `material-recipes-region`, same-line edits, new code in child files: the cut-out's lesson about line numbers in bounds-check messages). Target: helper WGSL ≤ 500 B gzip, engine ≤ 1.5 KB wasm, ≤ 4 B per vertex in models that carry regions, 0 for everyone else.
10. **Prior art, ideas only:** how games and engines mask creature coats (vertex-colour masks, ID maps, Spore-style paint regions), coat-colour genetics for which patterns follow extremities (temperature-sensitive points: the curvature rule already drawn), and pattern papers (reaction-diffusion for spots and stripes) where they would replace noise. Code written fresh; licences noted.

**Then B5-P3 builds it:** `shaders/rc_region.wgsl` appended only when a compiled recipe calls `rc_region_*` (the layout helper's precedent: a game with no region row pays nothing), the data path the research chose, the converter and check rules, and a 16-file byte proof. B5-P4 builds the code-built quadruped in parallel once R1 has fixed the data format.

## 4. Byte and GPU budgets

| Package | Part | Bytes (gzip -9) | GPU (RTX 4060 Ti, headless muted Chrome, overlay 2 s median) |
|---|---|---|---|
| B5-P1 | fur variants roan, merle, mottle, crimp, reticulated; `mix` | fur ≤ 1.75 KB (≤ +730 B) | `?set=p6` and the new coat page at High 1080p within +0.1 ms of before (P15 measured 1.5 to 2.0 ms for the whole 30-shape page) |
| B5-P2 | hide bristles, plates; keratin hoof, scutes, banded | hide ≤ 1.2 KB, keratin ≤ 1.2 KB | ≤ +0.05 ms on their pages |
| B5-P3 | region helper and data path | WGSL ≤ 500 B; engine ≤ 1.5 KB wasm; ≤ 4 B per vertex, only in models with regions; 0 for any other game (16-file proof) | one data read per pixel; ≤ +0.05 ms on the herd page |
| B5-P4 | code-built quadruped (sandbox proof and viewer only) | dev only; 0 in any game | the herd page (8 animals) at High 1080p ≤ 2.5 ms whole frame |

**A ranch** (fur, hide, keratin, hair, eye, leather; wet, mud, dust, snow, blood; regions): core 7.6 + five layers 2.9 + fur 1.95 (with regions) + hide 1.2 + keratin 1.2 + leather 1.4 + region 0.5 + character family 13.6 (manes and eyes) ≈ **30 KB gzip**. Fur stays inside its 2.5 KB class.

## 5. Tiers and live params

- **No tier file changes.** Lite draws what Low draws: every variant and every region at every tier (`lite_keeps_the_pattern`); Lite and Low drop only the fibre normals and the sheen rim's second lobe. Regions are colour, not relief, so they cost the same everywhere.
- **Group `fur`** for fur (as today); hide's `skin` group; keratin's `detail` group.
- **New live switches** (registered only when compiled, default on): `material.recipe.hide.bristles`, `material.recipe.region.enabled` (off: every region row draws its base coat, the light version), `material.recipe.region.softness` (0 to 2, scales every edge; for tuning by eye or by an agent), viewer `kit.material_view.region.show` (paints each region a flat debug colour, for checking a model).
- **Build time:** `[materials] recipes` gains `"regions"` (the cut-out's precedent: a part, not a recipe), implied by no recipe; `cargo xtask check` ERRORs on a model carrying a region row or region data without it, with a `fix:` line.

## 6. Proof sets and tests

**Proof scene `?set=b5-herd`** (sandbox, code-built): B5-P4's quadruped in four proportions (horse, cow, dog, cat), each standing and one lying down (the lying horse must keep its belly where its belly is), a row per subcategory, `?row=`, `?q=`, `?yaw=`, `?wet=`, `?snow=`, `?regions=0|1`, `?debug=regions`. Hooves, muzzles, eyes and manes on their own close-up row. Until B5-P4 lands, B5-P1 and B5-P2 use batch 1's ball and trunk shapes.

**Tests:** the template (schema equality, naga for each changed recipe alone, with all eight layers and all together on every text, the lite rule, the derivative grep, catalogue counts and encodings). B5-P1: each new variant's pattern mask is stable along the flow and anti-aliases to its average; `mix` 0 draws the coat of the variant below it. B5-P3: region data survives skinning (a CPU mirror on the quadruped: every vertex keeps its region in every pose), the convert dictionary maps the test rig's bones, a model without regions draws exactly its base coat, a game without the feature compiles none of it (cargo tree and symbol checks).

**Byte proof:** against `sha-base-b456.txt` (made by B4-P0; see batch 4 section 6). B5-P3, the integration B5-P5 and the final B5-P7 prove all 16 files; B5-P1 and P2 are recipe-only (diff stat, `Cargo.lock` diff).

## 7. Work packages

Common checks as batch 4 section 7.

| # | Package | Worker | Depends on | Runs |
|---|---|---|---|---|
| B5-R1 | **Research: body-region mask** (3.3, ten questions) | opus-high | none | wave 1, no build (one probe at most) |
| B5-P1 | **Fur variants** roan, merle, mottle, crimp, reticulated, `mix` (for batches 5 and 6) | opus-high | `matcat-engine` | wave 1, parallel. **Built on `b5-p1`**: 14 new rows drawn, 2 re-pointed |
| B5-P2 | **Hide and keratin**: bristles, plates; hoof, scutes, banded (for batches 5 and 6) | sonnet-high | `matcat-engine` | wave 1, parallel. **Built on `b5-p2`**: 12 rows drawn (8 + 4 of batch 6) |
| B5-N1 | **Numbers now**: the 50 rows of 3.1 on the ball and trunk shapes | sonnet-high | `matcat-engine` | wave 1b (one build) |
| B5-P0 | **Start**: branch `matcat-animals` (batches 5 and 6 together, they share every new part) from `matcat-b4` after B4-P7, or from `matcat-engine` after the batch 3 join if batch 4 is later; merge P1, P2, N1; DECISIONS "batch 5: design" | sonnet-medium | batch 3 joined | wave 2 |
| B5-P3 | **Region mask**: helper, data path, converter `--regions`, check rules, Blender override layer; 16-file proof | opus-high | R1, P0 | wave 3 |
| B5-P4 | **Code-built quadruped** with a skeleton and regions (horse, cow, dog, cat proportions; lying pose) in the sandbox proof and the viewer's shapes | opus-high | R1 (its data format), P0 | wave 3, parallel with P3 (P4 owns the shapes, P3 the engine) |
| B5-P5 | **Integrate and prove**: merge P3, P4 (and B4-P1's eye if batch 4 isn't merged yet) into `matcat-animals`; every recipe with every layer and the region on every text through naga; 16-file proof; sizes once | sonnet-high | P3, P4 | sequential |
| B5-N2a | **Numbers, horses**: 11 region coats, 5 roans, 3 hooves, 2 muzzles, the eye: 22 rows | sonnet-high | P5 | parallel with N2b, P6 |
| B5-N2b | **Numbers, livestock, dogs and cats**: 11 region coats, 9 new-variant coats, 3 pigs: 23 rows | sonnet-high | P5 | parallel with N2a, P6 |
| B5-P6 | **Real-GPU frame time**: the herd page and `?set=p6` at High and Ultra, 720p and 1080p, and the phone-tier capture; if fur plus regions costs more than 2 ms at High, file a trim | sonnet-medium | P5 | parallel with N2a, N2b |
| B5-P7 | **Final check and review** (section 7.1) | sonnet-high | everything | sequential, last |

(11 packages plus the research.) 50 + 22 + 23 = 95.

### 7.1 Done when (B5-P7)

- `cargo xtask check` and `cargo xtask check --full` clean.
- **Byte proof 16/16 `same`** against `sha-base-b456.txt`.
- **Contact sheets of all 95 rows at High, Low and Lite**, on the quadruped (standing and lying) and on the ball; regions shown with `?debug=regions` beside each region row.
- **Same look on every device:** a phone-portrait viewer capture of 12 rows (4 region coats, 4 new variants, 2 plain coats, a hoof, a muzzle) beside the desktop one; every region and pattern at Lite.
- **Every art style keeps detail:** 12 rows (buckskin, tuxedo, dun with its back stripe, blue roan, merle, calico, sheep fleece, pinto, leopard appaloosa, pig with bristles, striped hoof, horse eye) in all 24 styles; each still tells apart from its plain sibling (a buckskin from a bay, a tuxedo from a black cat) in every style; weak spots named.
- The byte and GPU table against section 4, `review` then `critic` for material-view once, DECISIONS with every number, the review card (what to judge; the fur-shells question repeated as a choice, not a blocker).

### 7.2 Order and what runs together

```
wave 1 (now)     B5-R1 research ────────────────────────────┐
                 B5-P1 fur ─────────┐                        │
                 B5-P2 hide, keratin┤                        │
                 B5-N1 numbers now ─┤                        │
batch 3 joins ── B5-P0 start ───────┴────────────────────────┼─ P3 region ────┐
                                                             └─ P4 quadruped ─┴─ P5 integrate ─┬─ N2a ─┐
                                                                                               ├─ N2b ─┼─ P7 final
                                                                                               └─ P6 ──┘
```

- **Worktrees:** `..\evani-dev-matcat-b5-<pkg>`, branch `matcat-b5-<pkg>`; wave 1 from the `matcat-engine` tip, later from `matcat-animals`.
- **File ownership:** P1 owns `rc_fur.wgsl` and `fur.rs` (for batches 5 and 6); P2 owns `rc_hide.wgsl`, `rc_keratin.wgsl` and their `.rs`; P3 owns `rc_region.wgsl`, evani-render's region path and the converter's region step; P4 owns the quadruped (sandbox proof module and viewer `shapes.rs`). Fur's use of the region (a few lines in `rc_fur.wgsl`) is done in P3 after P1 has merged, by P3's owner, so P1 is never reopened.

## 8. Earlier gaps, and what batch 5 needs from each

| Gap | Batch-5 rows | Where it's best solved |
|---|---|---|
| **Body-region mask** (P15: "start of batch 5") | 22 here; deer.coat.brown (batch 1) | B5-R1, B5-P3 |
| **Fur shells** (PF, Stage 8, Chris's call) | none need them; close-up depth would gain | not here; section 10, risk 1 |
| **Fur cost on a real GPU** (P15 measured: about 2 ms for the 30-shape page at High) | all 81 fur rows | B5-P6 measures the herd with regions |
| **The horse coat look** (P15: accepted as painted; "a little grain-like up close") | every horse coat | B5-P1 and N2a re-judge the bay beside the new coats; no shells |
| **Horse and cat pupils** | horse.eye.brown | B4-P1 (batch 4, wave 1) |
| **Bristles** | pigs, muzzles; batch 6's walrus | B5-P2 |

## 9. Not in batch 5 (filed for later)

- Fur shells and fins (Stage 8, PF).
- Mane and tail geometry, hoof prints, brands and saddle marks (models and decals).
- Seasonal coat change over time (a game blends two materials; no engine work).

## 10. Risks and open questions (each with the recommended answer)

1. **Fur shells before the horse batch** (batch 1's provisional away-decision). Recommended: **keep the painted coat for batches 5 and 6** and decide shells after batch 6's review; what makes these coats recognisable is colour, pattern and regions, which the painted coat draws identically on a phone and a PC, while shells add download, GPU cost and a stage change only Chris can make. **This is the one decision for Chris** in batches 4 to 6. **Decided while Chris was away (2026-10-25, by Evani.App, not Chris's own answer):** keep the painted coat for batches 5 and 6; decide 3D fur after batch 6's review. Easy to change: nothing in this plan builds or blocks on shells, and PF still starts only if Chris moves `fur-shells` to Stage 3.
2. **No animal model in the project.** Recommended: B5-P4's code-built rigged quadruped for proofs and tests (no licence question), plus one real rigged model for the review if B5-R1 finds a usable one.
3. **Region data might change the vertex layout of every model pipeline.** Recommended: research first; a cfg'd sub-feature, new code in child files, and the 16-file proof; a game without regions is byte-identical.
4. **Fur's block is full after `mix`.** Recommended: B5-R1 decides where region settings live before P3 starts; nothing else in this plan needs another fur slot.
5. **Many region rows set before the region exists would be wrong.** Recommended: they stay `planned` with the reason "body region" until B5-P5; B5-N1 sets only the 50 rows of 3.1.
6. **Points by curvature vs a legs region.** Recommended: keep `points` for extremities (ear tips, muzzles, the pointed cat's mask can use both); use regions where the look note names a body part.
7. **Batches 5 and 6 on one branch.** Recommended: yes (`matcat-animals`), since every new part serves both; each batch still gets its own final check and review card, batch 5's first.

## 11. Tally (updated on branch `b5-p1`, 2026-10-25)

**B5-P1, fur variants: built.** `rc_fur.wgsl` has variants roan 6, merle 7, mottle 8, crimp 9, reticulated 10 and the number `mix` (the tenth slot; fur's block is now full, 10 of 10). Fur gzip 1,005 to 1,324 B by the stripped-WGSL method (+319 B against the +730 B budget; 1.75 KB limit kept with 426 B to spare). GPU: `?set=p6` at High 1080p 0.71 to 0.73 ms before and 0.72 ms after (+0.00 ms; forward 0.55 ms both); the new `?set=b5-fur` page 0.44 to 0.47 ms.

| Rows set to `ready` | Count | Variant |
|---|---|---|
| horse.coat.grey_steel, red_roan, blue_roan, bay_roan, rabicano | 5 | roan |
| dog.coat.merle | 1 | merle |
| cat.coat.tortoiseshell, cat.coat.calico, cattle.coat.speckled | 3 | mottle |
| sheep.wool.black, brown, grey, sheep.skin.shorn, dog.coat.curly_brown | 5 | crimp |
| **New rows** | **14** | |
| Re-pointed (already `ready`): sheep.wool.white (solid to crimp), giraffe.coat (patches to reticulated) | 2 | crimp, reticulated |

Of batch 5's 23 "new coat or part looks" (3.2), these 14 are drawn; the other 9 are B5-P2's (3 hooves, 2 muzzles, 3 pigs) and B4-P1's horse eye. The 50 "numbers now" and the 22 region rows are untouched. Drawn after B5-P1 (batch 5 only): 14 of 95; catalogue total 402 + 14 = 416 of 1,054 counted on `b3-patterns` (batch 4 not counted here).

Contact-sheet review fixes (2026-10-25): red roan and bay roan separated (strawberry pink against dark brown), blue roan's base lightened so toon keeps its flecks. `check --full` clean; byte proof 16 of 16 `same` against `sha-base-16afb434.txt`. The painted coats stay as the root colour under the b5-fur shells.
