# Evani.Materials: batch 2 build plan (land and plants)

**In plain words, for Chris.** Batch 2 adds **98 land and plant materials**: soils, sands, gravels, 24 rocks, grass and moss ground, snow and ice ground, 13 kinds of leaves, 5 kinds of needles and 12 barks. 8 of them already work from batch 1. Most of the rest need only their numbers set, but plants need one real new piece: a proper **leaf, needle and grass recipe** with see-through glow at the edges, veins and frost, because the old foliage support only lit leaves from behind and could not take rain or snow. Rocks, soil, bark and snow get a few new looks (cracked mud, ploughed rows, porous and ropy rock, stringy and charred bark, wind-ridged snow). The work is **12 packages**; **five of them can run at the same time** in separate copies, so the batch is mostly limited by how many builds the PC can take (two at once).

Status: design only (2026-10-24). Nothing here is built. Builders take one package at a time.

Inputs read for this plan: `CATALOGUE_PLAN.md` and `catalogue.csv` (branch `material-catalogue`); `BATCH1_BUILD_PLAN.md`, `DECISIONS.md` (P0 to P10a), `crates/evani-material-recipes/src/recipes/mod.rs`, the 21 built recipes and 8 layers, `src/gltf.rs`, `catalog/materials/{recipes,materials}.json` and `crates/evani-material-nature/src/{gltf,surface}.rs` on `matcat-engine` (read only, at 4ffa8ef8); the research report `.vibedeck/research/2026-10-24-material-catalogue-fur-hide-bark-ground-ice-gem-co.md`.

## 1. Stage check

- **Stage 3 (Real content).** The catalogue is already filed under Stage 3's Includes by batch 1's P0 as an opt-in extension of the shared materials library. Batch 2 adds no gate box and no new stage item; B2-P0 adds one DECISIONS line.
- **Starts when batch 1 is done** (P15 committed). Chris's standing order (2026-10-24): "Once you've finished with batch 1, move on to batch 2, and then batch 3."
- **Not in batch 2:** 3D fur or grass shells (Stage 8, `fur-shells`; rows that would gain from them are listed in section 8), style treatments in the engine (Stage 8 look transfer), a terrain splat or two-material mix (section 9, later).

## 2. Scope: the 98 rows

| Subcategory | Rows | Recipe now | Ready from batch 1 |
|---|---|---|---|
| land / soil | 13 | ground 13 | 0 |
| land / sand | 5 | ground 5 | 0 |
| land / gravel | 7 | ground 7 | ground.gravel.road |
| land / rock | 24 | stone 22, glass 1 (obsidian), gem 1 (rock.salt) | rock.salt |
| land / grass_ground | 11 | foliage 11 | 0 |
| land / snow_ice_ground | 8 | ice 7, ground 1 (permafrost) | snow.packed, snow.slush, snow.dirty, ice.lake, ice.glacier |
| plants / leaves | 13 | foliage 13 | 0 |
| plants / needles | 5 | foliage 5 | 0 |
| plants / bark | 12 | bark 11, wood 1 (dead snag) | bark.pine |
| **Total** | **98** | foliage 29, ground 26, stone 22, bark 11, ice 7, glass 1, gem 1, wood 1 | **8** |

So 90 rows are still `planned`.

## 3. What covers them

### 3.1 Recipes that already draw them (numbers only)

| Recipe | Rows | Drawn by the existing variants | Needs a new feature (3.2) |
|---|---|---|---|
| ground (1,331 B) | 26 | clay red and grey, peat, ash, sand desert, white, black (sparkle), river, the 7 gravels, riverbank mud (ripple), tidal mud, permafrost (stone + sparkle as frost) | cracked_dry, alkali (crack network); ploughed, wagon_ruts, trail (furrows and ruts); forest_floor, corral_dirt (cover litter) |
| stone (999 B) | 22 | granite pink, sandstone buff and striped (layered), chalk, slate, shale, marble white and green (veined, edge translucency), quartzite, flint, mossy boulder (growth layer default), the 4 ores | limestone, travertine, pumice (pits); lava.cooled (ropy); conglomerate (clasts); cave_wall, stalactite (flowstone) |
| bark (1,474 B) | 11 | pine (ready), beech, cottonwood (furrowed), mangrove (furrowed; the arching roots are geometry) | redwood, cypress (fibrous); palm (ringed); cherry (lenticel bands); aspen (eye marks); burnt (charred seams); vine_woody (twisted cords) |
| ice (1,377 B) | 7 | packed, slush, dirty, lake, glacier (all ready) | snow.crust (wind ridges); ice.sheet_road (becomes a condition, 3.3) |
| wood (1,119 B) | 0 | none (bark.dead_snag moved to the fibrous bark variant in B2-P10: the wood grain ran round the trunk and read as stacked plates) | none |
| gem (1,042 B) | 1 + 1 | rock.salt (ready); rock.obsidian moves here (3.3) | none |

### 3.2 New recipe work

**One new recipe-stage recipe: `foliage` (code 20).** The other 28 recipe codes stay as they are.

Why: today foliage's backend is the nature family, whose `Foliage`, `Grass` and `Leaf` kinds light a *placed* surface (a box per primitive) with a back-light. That gives no per-pixel look (no veins, no blade pattern, no dry edges), takes no condition layers (no rain on leaves, no snow on needles: batch 1 section 7 filed this), and has only two numbers. 29 of the 98 rows are foliage, and a forest is mostly leaves, so this is the batch's main new piece.

- **Backend:** `recipes`, with `also: ["nature"]` (the paint precedent from P10a, where the coat moved into the recipe stage). The recipe stage draws the leaf; the nature kind tag is still written, so a game that lists the nature module keeps its placed back-light pass at High and up. A game that lists only `foliage` gets the recipe stage alone: no nature module, about 29 KB less.
- **Variants** (byte 1; append only, never reorder): leaf 0, needle 1, grass 2, straw 3 (existing) + **turf 4** (ground cover seen from above: blades in world space, for grass_ground), **moss 5** (cushion lumps and fine fronds), **litter 6** (fallen leaves and needles on the ground, colour 1 and colour 2 cells).
- **Numbers** (n0, n1 keep their slots; new ones take free slots n2 to n9): variation, thickness, **vein** (vein amount, leaf and frond), **gloss** (waxy top face), **edge_dry** (brown tips and edges), **blade_scale** (blades or needles per metre for turf, needle, litter), **bare** (bare soil showing through turf and bunch grass, in colour C), **underside** (pale or rusty back face: cottonwood, magnolia, willow).
- **Colours:** colour_1, colour_2, back_light (existing); for turf and litter colour C is the soil showing through, and the back-light colour is derived (colour_1 brightened), so no fourth colour is needed.
- **Extras** (bits): back_light 1 (a wrap transmission from the sun behind, `o.sheen` like keratin's edge translucency), veins 2 (vein lines and their normals), bare 4 (bare patches).
- **Pattern space:** uv for leaf, needle, grass, straw and moss (a leaf card or a needle spray is a uv quad cut by its texture's alpha, which the model pass already masks); world space from above (`rc_proj`) for turf and litter, so a terrain needs no uv.
- **How each row reads** (a guide for B2-P1 and the numbers sessions): maple, birch, elm, grape: leaf with veins, back-light; cottonwood, magnolia, willow: leaf with `underside`; ivy, jungle, banana, magnolia: high `gloss`; palm and fern: leaf on frond cards, fine veins; dead_brown: leaf, back_light 0, edge_dry 1; spruce, fir, cedar, juniper, dead: needle (juniper's berries are a speck of colour 2, spruce's blue-green is colour 1); prairie, alpine, marsh, burnt, dead: turf (burnt: colour 1 black, colour C grey ash; marsh: the water around it is the scene's water surface); bunch: turf with `bare` high; clover: turf with round three-part cells; moss.carpet: moss; leaf_litter and pine_needles: litter; frosted: turf with the snow layer's default amount (3.3).
- **Quality group `foliage`** (until now a mirror with no target):

| Level | Draws |
|---|---|
| lite | flat colour 1 to colour 2 variation, no back-light (the light version in recipes.json) |
| low | blade, needle and litter cells, vein colour, edge dry, anti-aliased to the average under 1.5 px |
| medium | vein and blade normals within 25 m, back-light wrap, underside tint, gloss |
| high | a finer second blade layer for turf and moss, sun glints on waxy leaves |
| ultra | as high |

**Small extensions of four existing recipes** (each a new variant or extra, numbers in free slots):

| Recipe | Addition | Kind | Rows |
|---|---|---|---|
| ground | **cracks** extra: the F2-F1 lines of a cellular noise, raised plates with dark gaps; with high `sparkle` the plates turn white and crusty (alkali). Shared later with asphalt.cracked (batch 3) | extra bit 4, number `crack_scale` | cracked_dry, alkali |
| ground | **furrows** extra: parallel ridges along uv u (ploughed rows), or two dark ruts at fixed v (wagon_ruts, trail: a worn line with turf-coloured edges in colour C) | extra bit 8, number `furrow_scale` (for ruts it is the rut spacing; the rut width is a fixed share of it, since ground has only 3 free number slots) | ploughed, wagon_ruts, trail |
| ground | **cover** extra: cells of twigs, needles or straw over the soil, only on faces that look up | extra bit 16, number `cover` | forest_floor, corral_dirt (its hoof prints are decals) |
| stone | variants **porous 4** (pits from cellular F1 holes, bands for travertine), **ropy 5** (lava: stretched ridged noise), **clastic 6** (conglomerate: domed pebbles in a sandy matrix, ground's stone maths), **flowstone 7** (smooth wet drapery: vertical fbm streaks, low roughness) | variants, number `pits` | limestone, travertine, pumice, lava.cooled, conglomerate, cave_wall, stalactite |
| bark | variants **fibrous 4** (long stringy strips: stretched noise, soft relief), **ringed 5** (palm: rings and leaf scars along v), **banded 6** (cherry: glossy with horizontal lenticel dashes) + extras **marks 2** (aspen's dark eye marks: sparse cells of colour C) and **embers 4** (burnt: glowing seams in the furrows, `o.emit`, no time term, so no flicker) | variants, extras, number `cord_twist` | redwood, cypress, vine_woody (fibrous with twist), palm, cherry, aspen, burnt |
| ice | **ridges** extra: wind ridges and shiny crust (a slow directional sine warped by noise, in the height function, gloss on the crest) | extra bit 8 | snow.crust |

Why extensions and not new recipes: every one of these rows already shares its recipe's colours, lighting, levels and conditions. A new recipe would cost its plumbing again (about 2.5 KB for the first one listed, P4 notes); a variant costs a few hundred bytes and only for games that list that recipe.

**Extras numbering hazard.** Extras take consecutive global bits in `STAGE` order (P2a). Adding an extra to ground, bark or ice shifts the global bits of every later recipe. That is safe because everything is generated from the table, but two packages doing it in parallel will both renumber: B2-P6 (integration) regenerates the table once, and the tests that hold `EXTRA_NAMES` equal to `recipes.json` and the deny-bit words catch any miss. 128 deny bits hold the 45 of batch 1 plus the 9 here.

### 3.3 Catalogue changes (data only, no shader code)

- **rock.obsidian: recipe `glass` → `gem`**, variant raw crystal, clarity 0, dispersion 0, colour black, sparkle low. Obsidian is opaque: the nature glass pass would draw it see-through, which is wrong. The gem's normal-space facets read as conchoidal fractures. Recorded as a catalogue change, the CSV re-exported.
- **ground.ice.sheet_road becomes a condition, not a material.** Rule: weather and wear stay condition layers. Black ice is a road that is wet and frozen, so the row becomes an **alias**: `params.alias_of = "asphalt.road"` and `cond_defaults` wet 1.0 (the wet layer already drops roughness to a mirror and darkens). The viewer shows it as asphalt with the wet layer on; a game uses asphalt.road with a wet amount. It keeps its id and search words (people search "black ice"). New status value `alias` (catalog reader, tests: an alias resolves to a ready row, has no block of its own, cannot chain).
- **Rows that are a base look plus a default condition** keep their own row but carry the amount in `cond_defaults`, so they share the base's numbers: ground.sand.wet (wet 0.8 over the sand numbers), ground.grass.frosted (snow 0.4, frost high), rock.cave_wall (wet 0.6), rock.stalactite (wet 0.5), rock.mossy_boulder (growth 0.6; lichen spots are the growth layer's colour), ground.snow.dirty (dust and mud, already ready).
- No other row changes recipe.

## 4. Byte budgets (wasm gzip -9, measured as in batch 1)

Lightness first: every addition must keep its recipe inside its class. Measured as standalone stripped WGSL and as the sandbox build pair (all 8 layers on), once per package.

| Part | Budget | Today |
|---|---|---|
| foliage (new, class M) | ≤ 2.0 KB standalone, ≤ 1.2 KB added to a build that already has another recipe | none |
| ground + cracks, furrows, cover | stays ≤ 2.0 KB (so ≤ +650 B) | 1,331 B |
| stone + 4 variants | ≤ 1.6 KB (≤ +600 B) | 999 B |
| bark + 3 variants, 2 extras | ≤ 2.0 KB (≤ +520 B: tight; fibrous and ringed share one stretched-noise helper) | 1,474 B |
| ice + ridges | ≤ 1.6 KB (≤ +220 B) | 1,377 B |
| core, layers, families | +0 (no core change in batch 2) | |
| Material numbers | 32 B raw per material used, inside its model | |

**A forest world** (ground, stone, bark, foliage, ice; wet, dust, snow, growth): core 7.6 + four layers about 2.3 + ground 2.0 + stone 1.6 + bark 2.0 + foliage 2.0 + ice 1.6 ≈ **19 KB gzip**, with no nature module needed. For scale, the shipped sandbox wasm is about 313 KB gzip.

If a recipe goes over, the package trims before hand-over (share helpers, cut a variant's high-only term), or lists the overrun with a trim plan for B2-P10.

## 5. Tiers and live params

- **No tier file changes** (batch 1 rule: tiers are compiled into every game).
- Foliage follows `material.quality.foliage` (table in 3.2). The other extensions follow their recipe's group (`detail` for ground, stone, bark; `glass` for ice), with the batch-1 meaning: lite flat, low pattern, medium normals within 25 m, high glints and 2-step parallax near, ultra parallax everywhere. Cracks and furrows parallax at high (ground's existing 2-step path); embers glow at every level above lite (lite is the flat charred colour).
- **The nature mirror:** `material.quality.foliage` now drives the recipe; it also sets the nature module's placed back-light only if a nature quality param is registered (it isn't today: P2b's note), so nothing new is mirrored.
- **New live switches** (registered only for compiled recipes, each one deny bit of binding 13, default on): `material.recipe.foliage.back_light`, `material.recipe.foliage.veins`, `material.recipe.foliage.bare`, `material.recipe.ground.cracks`, `material.recipe.ground.furrows`, `material.recipe.ground.cover`, `material.recipe.bark.marks`, `material.recipe.bark.embers`, `material.recipe.ice.ridges`.
- **Viewer** (P14's names, unchanged): `kit.material_view.extra.<recipe>.<extra>`, `kit.material_view.mat.<number>`, `kit.material_view.cond.<layer>.on|amount`.
- **Build time:** `evani.toml [materials] recipes = ["foliage", ...]` turns on `recipe-foliage` only; it no longer turns on the nature module (B2-P1 updates P10b's mapping and its test: "a game listing `foliage` gets `recipe-foliage` and nothing from nature").

## 6. Proof sets and tests

**Proof scene `?set=b2`** (sandbox, feature `recipes-proof`, dev only, built in code so no new asset and no stream manifest change):

- a 20 × 20 m terrain patch from a height function (flat, slope and a bank), tiled with soil, sand, gravel and turf strips;
- three rocks (displaced spheres) and a boulder with growth;
- two trunks (bark, `?yaw=` turns them) and a log;
- a leaf card row and a needle spray row: quads whose base texture's alpha is a leaf or spray shape generated in code (a 64 × 64 mask), so the existing alpha-mask discard cuts them;
- `?row=<n>`, `?shape=cube` and `?q=` as in batch 1; `?q=` also sets `material.quality.foliage`.

**Tests every package adds or extends** (the P4 template):

- schema rows equal `recipes.json` (variants appended, slots unchanged, extras names equal);
- naga validates each changed recipe alone, with all eight layers, and all recipes together (`recipe_wgsl.rs`);
- the lite branch returns before any pattern work (a text test, as for fur);
- no `dpdx`, `dpdy` or `fwidth` outside `mat_frame` (the existing grep test);
- catalogue: 1,054 rows, every `ready` row encodes with numbers inside their ranges, aliases resolve and don't chain, every batch-2 recipe has a ready row;
- foliage: a game-style feature set with only `recipe-foliage` compiles without evani-material-nature (a cargo tree test);
- captures at Lite, Low and High, headless and muted, into `.evani/shots/b2-<package>/`.

**Byte proof against the shipped games:** the batch-1 method, unchanged. The 8 games (2d-platformer, base-building-rts, kart-racer, match-3, model-view, sandbox, tactical-fps, top-down-action-adventure), wasm and js, 16 files, must print `same`. Batch 2 changes only the new crate, `catalog/materials/`, xtask's materials mapping and the sandbox proof, so recipe packages prove with `git diff --stat` plus a `Cargo.lock` diff; **one full 16-file proof runs in B2-P6** (it touches xtask's mapping) and one in B2-P10. Before each, compare the toolchain timestamp with the batch-1 baseline and rebuild the baseline if it changed.

## 7. Work packages

Common checks for every package (as batch 1): `cargo xtask check --quick` while working and the plain check clean at the end; clippy `-D warnings` native and wasm32 for the touched crates and named feature sets; its tests; a DECISIONS entry with measured bytes; a local commit ending `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`; no push. Captures to `.evani/shots/`, temp files to `.evani/tmp/`. About 60 tool steps per package; commit and hand over if it runs long.

### B2-P0: Start (sonnet-medium)

- **Goal:** branch, record, check the baseline.
- Branch `matcat-b2` from the `matcat-engine` tip after P15; merge `material-catalogue` (this plan).
- DECISIONS: "Material catalogue batch 2: design" (sections 3 to 5 here, the obsidian and black-ice changes).
- Toolchain check against the batch-1 byte-proof baseline; rebuild it in the background only if the compiler changed.
- **Depends on:** batch 1 P15. **Sequential** (first).

### B2-P1: Foliage recipe (opus-high)

- **Files:** `shaders/rc_foliage.wgsl`, `src/recipes/foliage.rs`, feature `recipe-foliage`, cfg'd rows in `shader::COMPILED`, `ALL_RECIPES`, `BUILT`; `recipes.json` (backend, variants, numbers, extras); `src/gltf.rs` (foliage rows now write `evani_recipe` plus the nature tag); xtask's `[materials]` mapping (foliage → `recipe-foliage` only) and its test; sandbox `?set=b2` leaf, needle, turf and moss rows.
- **Acceptance:** as the P4 template; ≤ 2.0 KB standalone; the cargo tree test (no nature module); the adapter test reads the block back and the nature parser still finds the kind; screenshots of all 7 variants at Lite, Low, High; snow and wet on needles visible at High.
- **Depends on:** B2-P0. **Parallel** with P2, P3, P4, P5.

### B2-P2: Ground extensions (sonnet-high)

- cracks, furrows, cover (3.2); `rc_ground.wgsl`, `src/recipes/ground.rs`, `recipes.json`; proof terrain strips.
- **Acceptance:** P4 template; ground ≤ 2.0 KB; no seam on the slope at High (world space).
- **Depends on:** B2-P0. **Parallel.**

### B2-P3: Stone variants (sonnet-high)

- porous, ropy, clastic, flowstone (3.2); proof rocks.
- **Acceptance:** P4 template; stone ≤ 1.6 KB; the polished-stone roughness rule from P4 still holds.
- **Depends on:** B2-P0. **Parallel.**

### B2-P4: Bark variants and ice ridges (sonnet-high)

- bark fibrous, ringed, banded, marks, embers; ice ridges (3.2); proof trunks and a snow drift.
- **Acceptance:** P4 template; bark stays periodic in u (the P6 fix: `?yaw=` shows no seam); bark ≤ 2.0 KB, ice ≤ 1.6 KB.
- **Depends on:** B2-P0. **Parallel.**

### B2-P5: Catalogue changes (sonnet-medium)

- obsidian to gem, the `alias` status and sheet_road, the `cond_defaults` rows (3.3); `src/catalog.rs`, `tests/catalog.rs`, `materials.json`, CSV re-export, `tools/materials/import_csv.mjs` if the new status needs a column.
- **Acceptance:** catalogue tests (counts unchanged at 1,054, the alias rules); the viewer's planned-row rule treats `alias` as openable (a note for P14's code; no viewer change here).
- **Depends on:** B2-P0. **Parallel** (data and reader only, no shader).

### B2-P6: Integrate and prove (sonnet-high)

- Merge P1 to P5 into `matcat-b2` in that order (merges, never rebases). Expect add-beside conflicts in `shader::COMPILED`, `ALL_RECIPES`, `BUILT`, `EXTRA_NAMES`, `recipes.json`, `tests/recipes.rs`, `recipe_wgsl.rs` and the proof's set list: keep both sides; regenerate the extras numbering.
- `recipe_wgsl` with all 21 recipe-stage recipes plus foliage and all layers; the full **16-file byte proof**; sizes measured once for every changed recipe (section 4).
- **Depends on:** P1 to P5. **Sequential.**

### B2-P7a and B2-P7b: Numbers (sonnet-high, two sessions)

- P7a: land (soil, sand, gravel, rock, snow and ice ground: 57 rows, 7 already ready). P7b: plants and grass ground (bark, leaves, needles, grass: 41 rows, 1 ready).
- Write `params`, `cond_defaults`, variant; set `ready`. Numbers are original, from each row's look note, checked on `?set=b2`.
- **Acceptance:** every batch-2 row is `ready` or `alias`, or stays `planned` with a written reason; contact sheet at High; CSV re-exported.
- **Depends on:** B2-P6. **P7a and P7b parallel** with each other (different rows of `materials.json`; it is one row per line block, so the merge is clean; the second to finish merges).

### B2-P8: Viewer scenes (opus-high, viewer branch)

- `matcat-viewer` merges `matcat-b2`. New test shapes in `shapes.rs`: a terrain tile with a slope (turf, litter, ground), a leaf card with an alpha mask, a rock. `viewer_scene` values in `recipes.json` for foliage and the new variants; `alias` rows open their base with the condition on.
- **Acceptance:** native tests (a foliage row opens on the leaf card; "black ice" finds the alias and opens asphalt with wet on); screenshots desktop and phone; only viewer files change; existing viewer tests pass.
- **Depends on:** B2-P6 (and batch 1's P14). **Parallel** with P7a and P7b.

### B2-P9: Real-GPU frame time (sonnet-medium)

- One look at cost on the real GPU, since batch 1's fur number came from a software adapter: `?set=b2&q=high` and `q=ultra` (terrain patch full of turf and leaf cards) and batch 1's `?set=p6&q=high|ultra` (fur), frame time with `telemetry_sample` in the Evani.dev tab or the overlay. Record against a 16.7 ms frame; if turf or fur costs more than 2 ms at high on the 4060 Ti, file a trim in B2-P10.
- **Depends on:** B2-P6. **Parallel** with P7 and P8 (it builds nothing new; one release build).

### B2-P10: Final check and review (sonnet-high)

- `cargo xtask check --full`; clippy combinations; the 16-file byte proof; contact sheets of all 98 rows at High and Low in the default style plus 10 rows in 4 styles; the per-recipe byte table against section 4; `cargo xtask review material-view` then `cargo xtask critic material-view` once; DECISIONS with every number; the review card for Chris (what to judge; styles show colour, not detail, until Stage 8).
- **Depends on:** everything above. **Sequential** (last).

### Order and what runs together

```
B2-P0 ─┬─ P1 foliage ────┐
       ├─ P2 ground ─────┤
       ├─ P3 stone ──────┼─ P6 integrate ─┬─ P7a numbers land ──┐
       ├─ P4 bark, ice ──┤   + byte proof ├─ P7b numbers plants ┼─ P10 final
       └─ P5 catalogue ──┘                ├─ P8 viewer ─────────┤
                                          └─ P9 GPU frame time ─┘
```

- **Parallel groups:** {P1, P2, P3, P4, P5} each in its own worktree (`..\evani-dev-matcat-b2-<pkg>`, branch `matcat-b2-<pkg>` from `matcat-b2` after P0); then {P7a, P7b, P8, P9}.
- **Why they don't touch each other:** each owns its own `rc_<name>.wgsl` and `src/recipes/<name>.rs`; the shared lists get one-line add-beside edits that P6 resolves. P5 touches no shader.
- **How many at once (measured 2026-10-24: 16 cores, 64 GB RAM, about 50% CPU and 60% RAM already in use by other work):** **at most 2 sessions building Rust at the same time**, each with `CARGO_BUILD_JOBS=2` through the low-priority runner, plus any number of sessions that only edit data or docs. Each worktree has its own `target/` (several GB and a cold first build); check free disk before making the third. **A byte proof (8 games) runs alone,** never beside another build. Every session calls `resource_check` before a build and waits if the PC is over 80%. So of the five parallel packages, run P1 and P2 first (both build), P5 alongside (light), then P3 and P4.
- Batch 3's engine packages may start once B2-P6 is merged (see `BATCH3_BUILD_PLAN.md`), while P7 to P9 finish.

## 8. Batch 1 gaps, and what batch 2 needs from each

| Gap (batch 1) | Batch-2 rows that depend on it | Where it's best solved |
|---|---|---|
| **Blend pass** (fire, smoke, holograms draw as opaque cards; P9, P15 decision) | none need it. Leaves and needles are alpha-masked cards (works today). slush is fine opaque. gravel.creek and grass.marsh put water over them with the scene's water surface, not a blend. ground.ice.sheet_road is now a condition (3.3) | batch 3, before glass (BATCH3_BUILD_PLAN B3-P3), if P15 says yes |
| **Body-region mask** (deer belly, banded snakes; P6, P7, P9) | none. (Aspen eye marks and cherry bands are bark patterns, not regions.) | at the start of batch 5 (horses and livestock: 8 horse coats, cattle, sheep, dog and cat rows wait on it), as one shared helper in `rc_shared.wgsl`: a region number plus a second colour, drawn from uv v or the vertex colour; batch 1's deer.coat.brown is drawn then |
| **Nail heads, chequer patterns** (wood.plank.deck, stonework.marble_floor; P4) | none | batch 3, B3-P1, the shared layout helper (studs, chequer, herringbone, shingles) |
| **Fur cost on real GPUs** (P6 measured only in a software test) | none use fur; turf and moss are the same kind of cost over large areas | B2-P9 measures both on the real GPU, so it's settled before batch 5 |
| Condition layers on family materials (batch 1 section 7) | all 29 foliage rows (snow on needles, rain on leaves) | solved here by moving foliage into the recipe stage (B2-P1) |
| Kit meshes (box worlds) using catalogue rows (batch 1 section 7: "for batches 2 and 3") | any box-world terrain | batch 3, B3-P9, once, for land and buildings together |

**3D shells (Stage 8, not planned here).** Rows that would gain the most from shells once `fur-shells` is approved: ground.grass.prairie, ground.grass.alpine, ground.grass.marsh, ground.grass.bunch, ground.clover, ground.moss.carpet, ground.grass.frosted. Until then they are painted turf, which is also their light version.

## 9. Not in batch 2 (filed for later)

- **Two-material mix on one mesh** (grass fading into soil on a slope, a trail through grass drawn by a painted mask): needs two blocks or a painted weight (the vertex colour alpha, batch 1 risk 7). A CORE_REQUESTS item when a terrain kit asks for it; the turf `bare` number and the ground `cover` extra cover the common cases in one block.
- **Hoof prints, tracks, ruts on a curved road:** decals or geometry (the ruts here follow uv, so a road mesh needs its uv along the road).
- **Mangrove roots, basalt columns, stalactite shapes:** geometry.
- **Wind sway of grass and leaves:** vertex animation, not a material.

## 10. Risks and open questions (each with the recommended answer)

1. **Moving foliage's backend changes P10b's mapping.** Recommended: do it (the paint precedent); a game with foliage gets lighter, and the nature module stays available for its placed back-light. B2-P1 updates the mapping and its test.
2. **Obsidian and black ice are catalogue changes.** Recommended: make them; they follow the rules (an opaque stone isn't glass; weather is a layer). Recorded in DECISIONS; no question for Chris.
3. **Bark's budget is tight.** Recommended: fibrous and ringed share one helper; if still over 2 KB, embers moves to a lite-off high-only branch. Listed in B2-P10 otherwise.
4. **Five parallel branches renumber extras.** Recommended: generated tables plus P6's single regeneration; the existing equality tests catch a miss.
5. **Foliage on large terrains costs fill.** Recommended: measure in B2-P9 before batch 5; turf's lite version is one noise and one mix.

## 11. Progress (branch `b2-main`, 2026-10-24)

- [x] B2-P0 start (branch, decisions)
- [x] B2-P1 foliage recipe (+1,513 B in a full build; trim filed for B2-P10)
- [x] B2-P2, P3, P4 ground, stone, bark and ice options (as `b2-options`)
- [x] B2-P5 catalogue changes (obsidian to gem, alias, `cond_defaults`)
- [x] B2-P6 integrate and prove (16 of 16 files `same`)
- [x] B2-P7a numbers, land (33 rows) and B2-P7b numbers, plants (20 rows): every batch-2 row is ready or alias (97 + 1)
- [x] B2-P8 viewer scenes (shapes leaf, spray, tuft, slope, rock; aliases open)
- [x] B2-P9 real-GPU frame time (RTX 4060 Ti, no trim needed; DECISIONS)
- [x] Joined into batch 1's line (`b2-main` into `matcat-engine`, 2026-10-25), so both batches go to Chris as one join. Tally: batch 1 96 draw / 10 planned, batch 2 98 draw (97 + 1 alias) / 0 planned, together **194 of 204**; the whole catalogue 313 of 1,054 (BATCH1_BUILD_PLAN P15 notes, DECISIONS "Join: batch 2 into matcat-engine").
- [x] B2-P10 final check and review: look fixes (ground `crushed`, snag, beech), contact sheets, review card (`BATCH2_REVIEW_CARD.md`); foliage trim not possible without a look change (+1,513 B, filed), fur trim filed for the final batch review (DECISIONS, B2-P10)
