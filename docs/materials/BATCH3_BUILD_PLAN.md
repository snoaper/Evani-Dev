# Evani.Materials: batch 3 build plan (buildings and town)

**In plain words, for Chris.** Batch 3 adds **90 building materials**: bricks, stonework, plaster and concrete, building timber, roofs, metalwork, wall and floor tiles, interior finishes (wallpaper, carpet, lino, cork) and window glass. 6 already work from batch 1. What's missing is mostly **laid-out patterns**: herringbone and basket bricks, chequered floors, parquet, overlapping shingles and roof tiles, rivets and nail heads, patterned tiles and wallpaper. So the batch first builds **one shared pattern-layout tool** that every recipe can use, then adds three new recipe versions: **window glass** (frosted, stained, leaded, wired), **wallpaper** and **patterned carpet and rugs**. It also needs two engine pieces: **cut-out holes** (grates, chain-link fences, lattice) and, if you agree at the end of batch 1, **see-through drawing** for glass (which also makes fire and holograms see-through). The work is **17 packages**; up to **six can run side by side** once the shared pieces are in, two building at a time.

Status: design only (2026-10-24). Nothing here is built. Builders take one package at a time.

Inputs read for this plan: as `BATCH2_BUILD_PLAN.md` (the batch-1 plan, DECISIONS P0 to P10a, the recipe crate and its recipes and layers, `gltf.rs`, the catalogue JSON, the nature family's glTF reader), plus batch 2's plan, which this one follows.

### Progress (branch b3-patterns, 2026-10-25)

| Package | State |
|---|---|
| B3-P1 layout helper | done (11 rows) |
| B3-P4 masonry (stone's slate moved here) | done except the final review; 14 rows |
| B3-P5 plaster | done for the plaster recipe (8 rows) and plastic's sheet; asphalt.cracked drawn on batch 2's ground cracks after the join |
| B3-P6 wood | done; 12 rows (wood.lattice through the cut-out) |
| B3-P7 metal and paint | done; 10 rows (grate, chain link and mesh through the cut-out) |
| B3-P8 ceramic | done; 8 rows |
| B3-P10b thin and cloth | done; 7 rows (three wallpapers, two carpets, a woven rug, batch 1's denim) |
| Rows that need only numbers or a small variant | done; 6 rows (terrazzo, tar paper, linoleum, log in bark, two mirrors) |
| Wood's layouts part | `recipe-wood-layouts` (`"wood-layouts"`): wood 1,676 B without it, +523 B with it |
| B3-P2 cut-out | done (2026-10-25): `cutout` / `material-recipes-cut`, one path with the glTF alpha mask, holes in the shadow map, cascades and AO depth; 5 kinds (grate, chain link, mesh, lattice, and batch 1's lace); about 2.2 KB gzip, 0 for games without it (DECISIONS "B3-P2") |
| Join with batch 2 | matcat-engine 378f9b02 joined; roof.sod (foliage turf) drawn |
| B3-P3 see-through pass | done (2026-10-25): `material-recipes-blend` (sorted premultiplied blend, add as alpha 0, stipple shadows) and `material-recipes-refract` (half-size copy with three mips for frosted, wavy and block glass); `"blend"`, `"refraction"` in `[materials] recipes` (DECISIONS "B3-P3") |
| B3-P10a glass recipe stage | done with B3-P3: `recipe-glass`, the eight glass rows |
| **Tally (2026-10-25, after B3-P3)** | **90 of 90 drawn**. Batch 1's lace, damask and newsprint are drawn too (batch 1 rows) |
| B3-P9, P11 to P13 | not started here (kit meshes, integration, numbers review, final) |

## 1. Stage check

- **Stage 3 (Real content)**, inside the catalogue's existing Stage 3 Includes line. No gate box, no new stage item.
- **Starts** when batch 2's integration (B2-P6) is merged: its engine packages don't need batch 2's numbers, so they may overlap B2-P7 to B2-P9. Batch 3's numbers and review come after batch 2's review (B2-P10), so Chris judges batch 2 first. That follows his order "batch 2, and then batch 3" for what he sees, while keeping the PC busy.
- **Two engine pieces touch evani-render's model pass** (B3-P2 cut-out, B3-P3 blend). Both are opt-in sub-features of `material-recipes`, so a game that doesn't list them is byte-identical (proved).
- **B3-P3 waits for batch 1's P15 decision** on the blend pass (section 8). If P15 says no, glass draws through the nature pass as today plus its recipe-stage surface (section 3.3, the fallback).
- **Not in batch 3:** shells for carpet pile and sod roofs (Stage 8), style treatments in the engine (Stage 8), interior props and furniture (batch 8).

## 2. Scope: the 90 rows

| Subcategory | Rows | Recipe now | Ready from batch 1 |
|---|---|---|---|
| brick | 9 | masonry 9 | 0 |
| stonework | 10 | masonry 7, stone 2 (marble_floor, terrazzo), ceramic 1 (mosaic) | setts, flagstone |
| plaster_concrete | 12 | plaster 9, masonry 1 (concrete.block), ground 2 (asphalt) | lime_wash, stucco, asphalt.road |
| wood_build | 15 | wood 14, bark 1 (log in bark) | 0 |
| roofing | 6 | ceramic 2, stone 1, metal 1, plastic 1, foliage 1 (sod) | 0 |
| metalwork | 10 | metal 7, paint 3 | 0 |
| tiles | 7 | ceramic 7 | 0 |
| interior | 11 | thin 3, cloth 3, metal 1, plaster 1, paint 1, plastic 1, wood 1 | wall.paint.gloss |
| glass_build | 10 | glass 8, metal 2 (mirrors) | 0 |
| **Total** | **90** | masonry 17, wood 15, metal 11, ceramic 10, plaster 10, glass 8, paint 4, stone 3, thin 3, cloth 3, ground 2, plastic 2, bark 1, foliage 1 | **6** |

(Per recipe, from the CSV: masonry 17, wood 15, metal 11, ceramic 10, plaster 10, glass 8, paint 4, stone 3, thin 3, cloth 3, ground 2, plastic 2, bark 1, foliage 1 = 90.) 84 rows are `planned`.

## 3. What covers them

### 3.1 The shared layout helper (the main new piece)

Batch 1 left a family of rows `planned` for one reason: no recipe could lay units out in anything but a running bond or square tiles (DECISIONS P4, P5: the plank deck's nail heads, the chequered marble floor, herringbone and basket brick). Batch 3 has about 35 rows of that kind across eight recipes. Building the layout once, as a helper the recipes call, is far lighter than eight copies.

**`shaders/rc_layout.wgsl`**, appended (like `rc_shared.wgsl`) only when a compiled recipe needs it, through an internal feature `layout` that those recipes' features turn on. It returns, for a 2D point, a **unit cell**: the unit's id (for a per-unit hash and tone), the local position in the unit (0..1), the distance to its edge (joint, grout, seam or lead line), and a lap height where units overlap.

| Layout | Used by (rows) |
|---|---|
| running and stack bond with any unit ratio (exists in masonry; moved into the helper) | brick, concrete.block, subway tile (2:1), clapboard boards |
| **herringbone** (units at 90° in zigzag rows) | brick.herringbone, wood.parquet |
| **basket** (pairs alternating direction) | brick.basket, plaster.mud_daub (the woven sticks showing) |
| **chequer** (two colours) | stonework.marble_floor, tile.encaustic (base) |
| **hex** (the core's `rc_hex`) | tile.hex, build.mesh |
| **shingle courses** (rows offset half a unit, each lying over the next, a shadow under the free edge, per-unit length jitter) | wood.shingle, wood.clapboard (lap), roof.tile.slate, roof.tile.flat_clay, roof.tile.terracotta (with a barrel curve) |
| **panels** (a frame with a raised or sunk field and a bevel) | wood.panelling, wall.tin_ceiling, wood.board_batten (battens as narrow raised strips) |
| **studs** (dots in rows on a unit's edge: nail heads, rivets) | wood.crate, wood.floor.*, build.iron.rivet_plate, build.steel.girder, and batch 1's wood.plank.deck |
| **grid and diamond** (thin crossing lines, raised diamonds) | glass.leaded, glass.wired, build.grate, build.chain_link, build.diamond_plate, wood.lattice |
| **stripes and seams** | roof.tin_standing, build.copper.roof, roof.tar_paper, wall.paper.stripe |
| **motif** (a seeded rosette, star or flower from a few polar terms, 2 or 3 colours inside a tile, with a hand-painted wobble) | tile.encaustic, tile.mexican, tile.blue_painted, stonework.mosaic, wall.paper.floral, wall.carpet.patterned, wall.rug.woven |

- **Budget:** ≤ 1.2 KB gzip for the whole helper, paid once by any game listing a recipe that uses it. A game with only plain running-bond brick pays it too (masonry's own bond code moves into the helper, so masonry itself shrinks by about the same).
- **Anti-aliasing:** each layout fades its lines and units to their average under 1.5 px (the core's AA fade), so a far wall doesn't shimmer.
- **Selected by the recipe's variant** (an appended variant per layout, never reordering), with the unit size from the recipe's existing numbers where they fit (masonry's unit width and height, wood's plank width, ceramic's tile scale), so few new number slots are used.

### 3.2 Recipes extended (variants appended, extras in free bits, numbers in free slots)

| Recipe (today, gzip) | Additions | Rows |
|---|---|---|
| **masonry** (1,238 B; 3 free slots) | variants herringbone 6, basket 7, rubble 8 (thick irregular joints: cellular units, wide mortar), drystone 9 (no mortar, dark deep gaps), shingle 10 (slate courses); extras **painted 4** (one paint colour over units and mortar, the bumps kept, peeling to the unit with the wear layer), **glaze 8** (glossy units), **flecks 16** (straw or fossil specks in colour C); number `fleck` | all 9 bricks, rubble, drystone, river_rock (cobble, round), sandstone and limestone (ashlar, flecks), concrete.block (stack, grainy), roof.tile.slate (moves here from stone, 3.4) |
| **wood** (1,119 B; 2 free slots) | variants clapboard 4, board_batten 5, logs 6 (rounded bands along v with chinking in colour C), shingle 7, parquet 8, panels 9, culm 10 (bamboo: nodes along v, glossy), granular 11 (cork, chipboard); extras **studs 4** (nail heads), **painted 8** (a paint colour peeling to the grain: wear shows the wood), **adze 16** (scalloped tool marks); number `paint_peel` | all 14 wood rows; wood.lattice is cut out (3.5); closes batch 1's wood.plank.deck |
| **metal** (820 B; 2 free slots) | extras **studs 8** (rivets), **seams 16** (standing seams and creases), **pattern 32** (diamond plate, embossed tin squares through the layout); variant **wire 4** (grate, chain link, mesh, cut out, 3.5); number `layout_scale` | rivet_plate, copper.roof, lead.flashing (seams as creases), diamond_plate, grate, chain_link, mesh, tin_ceiling, roof.tin_standing; mirrors are numbers only (3.4) |
| **paint** (≈1,320 B; 2 free slots) | extra **studs 16** (rivets under the paint, for girders) | build.iron.railing, steel.girder, painted_crate (the wear layer chips edges to colour B), wall.paint.gloss (ready) |
| **ceramic** (1,155 B; 4 free slots) | variants subway 6 (2:1 bevelled), hex 7, mosaic 8 (small tiles, a tone per tile, motif colours), roof 9 (shingle courses, barrel curve for terracotta); extra **motif 4** (painted motifs on the glaze in colour C); numbers `tile_ratio`, `motif_scale` | all 7 tiles, mosaic, both clay roof tiles |
| **plaster** (1,072 B; 4 free slots) | extras **chunks 4** (big broken patches showing lath stripes or woven sticks in colour C), **aggregate 8** (pebbles through the surface: pebbledash, exposed aggregate; ground's dome maths), **boards 16** (board-marked concrete: plank stripes with grain streaks); variant **polished 4** (low roughness, soft mottle); numbers `chunk_amount`, `aggregate_size` | peach, cracked, pebbledash, mud_daub, concrete.board, concrete.aggregate, concrete.polished, wall.paint.matte |
| **stone** (≈1.6 KB after batch 2) | extra **tiles 8** (chequer or square slabs with polished joints, through the layout); variant **terrazzo 8** (chips of colour 2 and C in a cement colour 1) | marble_floor (closes the batch-1 gap), terrazzo |
| **ground** (≈2.0 KB after batch 2) | none: asphalt.cracked uses batch 2's cracks extra, with the gaps glossy and dark (tar) | asphalt.cracked |
| **plastic** (867 B; 4 free slots) | variant **sheet 4** (linoleum mottle: stretched fbm in colours A and B; tar paper: grit and seams through the layout's stripes) | wall.linoleum, roof.tar_paper |
| **foliage** (batch 2) | none: roof.sod is turf with `bare` showing earth | roof.sod |
| **bark** | none: wood.log.bark is bark on a log (numbers) | wood.log.bark |

### 3.3 New recipe-stage versions of three family recipes

Batch 1 served these through their families; batch 3's rows need per-pixel patterns those families don't have. Each keeps its family for what the family does well (the paint and foliage precedent): `backend: recipes`, `also: [family]`.

**glass (code 24)**: `also: ["nature"]`.

- The recipe stage draws the **surface**: tint, frost (milky, raised roughness, a soft noise), waviness (normal ripple that warps reflections: old wavy panes), stained (a deep colour with a back-light glow in `o.emit` when the sun is behind the pane), leaded (the layout's grid of small panes with dark lead came lines in colour B, opaque), wired (a fine square wire grid inside, colour B), glass block (the layout's square units with rippled faces and a thick edge).
- **See-through:** with the blend pass (B3-P3), the pane is drawn blended: tint times what is behind, lead and wire lines opaque. Without it (or at lite and low), the pane is opaque tinted glass with a sky reflection, the light version recipes.json already names ("flat tinted see-through" becomes "flat tinted gloss" at lite). Where a game also lists the nature module, the nature pass's real refraction still draws placed panes at high and ultra.
- **Variants:** window, bottle, crystal, stained (existing) + leaded 4, wired 5, block 6. **Numbers:** thickness, frost, waviness + `pane_scale`, `backlight`. **Colours:** tint + **came** (lead or wire colour).
- **Group:** `glass`. Budget ≤ 1.5 KB.

**thin (code 21)**: `also: ["nature"]`.

- The recipe stage draws **printed and papery** surfaces: wallpaper stripes and florals through the layout (stripes, motif), a fade and a paper grain, peeling (curled patches showing the plaster under, in colour C, with a rolled edge highlight). Light-through (paper lanterns, petals, membranes) stays the nature kind's job, written by the adapter as today.
- **Variants:** paper, wax, petal, membrane, lampshade (existing) + wallpaper 5. **Numbers:** thickness, pattern + `motif_scale`, `peel`, `fade`. **Colours:** colour, light_through (for wallpaper: the print colour), + C (what's under).
- **Group:** `detail` for wallpaper (it's a wall), `foliage` for the rest (as now). Budget ≤ 1.0 KB.

**cloth (code 10)**: `also: ["recipes"]` (the layers family stays the backend for its sheen).

- The recipe stage adds what the layers family lacks: **weave relief** for plain and twill (closes batch 1's cloth.denim.indigo gap: the diagonal), **pile** (dense short tufts: carpet), **woven motif** through the layout (patterned carpet, flat-woven geometric rugs: stripes, diamonds, steps, the motif), then the layered stage adds its sheen and fuzz lobes as today. A recipe block composes with layers (batch 1 rule).
- **Variants:** plain, twill, knit, pile, satin (existing) + flatweave 5. **Numbers:** weave_scale, sheen, sheen_rough, fuzz, rough + `motif_scale`, `motif_contrast`. **Colours:** colour, sheen + C (motif colour).
- **Group:** `detail`. Budget ≤ 1.2 KB. Whether cloth's sheen also moves into the recipe stage (as paint's coat did, saving the 7.2 KB layers family) is batch 4's call (clothing, 120 rows): noted there, not done here.

With these three, **24 of the 29 recipes have their own recipe-stage code** (21 + foliage in batch 2 + glass and thin here, with cloth's patterns as a recipe-stage extra like paint's). The remaining family-only recipes are skin, hair, eye and water, for batches 4 and 10.

### 3.4 Catalogue changes (data only)

- **roof.tile.slate: stone → masonry** (shingle variant). Slates are laid units, not a rock face; masonry has the joints, the courses and the unit variation.
- **glass.mirror and glass.mirror_old stay metal**: rough 0, metal 1, silver; the old mirror's dark speckled patches are the oxide layer's default amount with a dark colour C (desilvering is wear, so it is a condition, not a new look).
- **brick.firebrick's soot** is the dust layer's default amount; **plaster.cracked** keeps cracks in the recipe and its missing chunks in the new `chunks` extra (batch 1 called them geometry; a painted hole with lath stripes reads at game distance, a real hole stays geometry).
- **wood.plank.painted, brick.white_painted**: one row each, drawn by the recipe's `painted` extra (not a paint row over a wood row), so the wear layer peels the paint to what's under: weather and wear stay layers.

### 3.5 Engine pieces

**B3-P2: late cut-out.** Grates, chain-link, wire mesh and lattice are see-through holes in a pattern. The model pass's alpha-mask discard runs *before* the recipe (batch 1, P9), so a recipe can't make a hole. Plan: a sixth patch edit, applied only to materials whose glTF alpha mode is MASK and whose block sets the recipe's cut bit: after `mat_surface`, `if (ro.base.a < cutoff) { discard; }`.

- Every derivative is already taken in `mat_frame`, before both discards, so the recipe stays valid. The worker must confirm with naga's uniformity analysis that nothing *after* the new discard needs uniform control flow (the shadow and environment reads in the lit stage: if one uses an implicit-derivative sample, it is moved to an explicit-level sample in the patched text only, or the cut is written as alpha-to-coverage at 4x and a discard at 1x).
- Cost: ≤ 300 B (a line in the patch and one bit). Sub-feature `material-recipes-cut`, turned on by `[materials]` when a listed row uses a cut layout; off otherwise.
- Shadows: a cut material casts a solid shadow unless the shadow pass also runs the recipe; batch 3 accepts the solid shadow (a fence's shadow is soft and far) and files a cut shadow for later.
- Also usable by fire cards (batch 1's "flames are opaque cards": a hard cut-out flame is better than a dark card, while the blend pass is undecided).

**B3-P3: blend pass (if P15 says yes).** A second model pipeline for recipe materials with glTF alpha mode BLEND: alpha blending (premultiplied; an additive mode for fire, glow and holograms via a block bit), depth test on and depth write off, drawn after the opaque models, sorted back to front per model, before the nature pass.

- Sub-feature `material-recipes-blend`; a game that lists none of glass, fire or glow's hologram gets none of it.
- Budget ≤ 2.5 KB gzip (one more pipeline per sample count, the sort, the bit).
- What it unlocks: glass panes see-through (8 batch-3 rows), batch 1's fire, smoke and steam cards, the hologram, and later batch 10's water-adjacent rows. Batch 1's P15 list holds this decision; this plan's recommendation for P15 is **yes, in batch 3**, because glass is the first batch with many rows that need it and a cut-out (B3-P2) alone can't draw a tinted pane.

**B3-P9: catalogue rows on box-world meshes (kit meshes).** Batch 1 filed this "for batches 2 and 3". Kit meshes (box worlds) draw through the `kit_surface` path with proc and relief, not the model pass, so catalogue rows don't reach them. Plan: a design spike first (on a scratch branch, documents only), choosing between (a) the same recipe patch applied to the kit-surface stage (one more anchor set, tested like the model stage) and (b) a mapping from a row to proc and relief tuning (cheaper, fewer looks). Recommended: (a), so a box-world town and a model town look the same. Then build. It is an engine path change: 16-file byte proof. Optional for batch 3's review: if it runs long it moves to batch 4 without blocking anything.

## 4. Byte budgets (wasm gzip -9, measured as batch 1)

| Part | Budget | Today |
|---|---|---|
| layout helper (shared, once) | ≤ 1.2 KB | none (masonry's bond code moves into it) |
| masonry + 5 variants, 3 extras | ≤ 1.75 KB (≤ +500 B, after moving its bonds out) | 1,238 B |
| wood + 8 variants, 3 extras | ≤ 2.0 KB (≤ +880 B) | 1,119 B |
| metal + extras, wire | ≤ 1.5 KB (≤ +680 B) | 820 B |
| paint + studs | ≤ 1.5 KB (≤ +180 B) | ≈1,320 B |
| ceramic + 4 variants, motif | ≤ 1.75 KB (≤ +600 B) | 1,155 B |
| plaster + 3 extras, polished | ≤ 1.6 KB (≤ +530 B) | 1,072 B |
| stone + tiles, terrazzo | ≤ 1.9 KB (≤ +300 B) | ≈1.6 KB |
| plastic + sheet | ≤ 1.1 KB (≤ +250 B) | 867 B |
| glass (new recipe stage) | ≤ 1.5 KB | 0 (nature family) |
| thin (new recipe stage) | ≤ 1.0 KB | 0 (nature family) |
| cloth patterns (new recipe stage) | ≤ 1.2 KB | 0 (layers family) |
| cut-out (engine, sub-feature) | ≤ 300 B | |
| blend pass (engine, sub-feature) | ≤ 2.5 KB | |

**A western town** (masonry, wood, metal, paint, ceramic, plaster, glass, ground; wet, dust, snow, wear, oxide): core 7.6 + five layers about 2.9 + layout 1.2 + masonry 1.75 + wood 2.0 + metal 1.5 + paint 1.5 + ceramic 1.75 + plaster 1.6 + glass 1.5 + ground 2.0 + blend 2.5 ≈ **28 KB gzip**, with no nature or layers family needed. Every recipe stays inside the 1.2 to 2.5 KB range; an overrun is trimmed before hand-over or listed with a plan in B3-P13.

## 5. Tiers and live params

- **No tier file changes.** Recipes keep their groups (`detail` for building surfaces, `glass` for glass, `foliage` for thin's non-wallpaper variants). Layout lines follow the levels: lite draws the layout as flat colour (units and joints, no bevels, motifs as flat colour), low adds per-unit tone and the motif detail anti-aliased, medium adds bevels, lap shadows, stud and seam normals within 25 m, high adds 2-step parallax on joints and laps within 8 m, ultra parallax everywhere.
- **Glass:** lite opaque tinted gloss; low adds frost and the lead and wire lines; medium adds waviness and the back-light glow; high adds the blend (if built) and sharper reflections through `material.quality.reflections`; ultra as high.
- **New live switches** (registered only for compiled recipes, default on): `material.recipe.masonry.painted|glaze|flecks`, `material.recipe.wood.studs|painted|adze`, `material.recipe.metal.studs|seams|pattern`, `material.recipe.paint.studs`, `material.recipe.ceramic.motif`, `material.recipe.plaster.chunks|aggregate|boards`, `material.recipe.stone.tiles`, `material.recipe.glass.backlight`, `material.recipe.cloth.weave|motif`, `material.recipe.thin.peel`; engine: `material.recipe.cut.enabled` (with the cut sub-feature), `material.recipe.blend.enabled` and `material.recipe.blend.sort` (with the blend sub-feature; off draws blended rows opaque, the light version).
- **Build time:** `[materials] recipes = ["glass"]` turns on `recipe-glass` (and `material-recipes-blend` when the game also sets `[materials] blend = true`, so the extra pipeline is a choice, not a surprise); `thin` → `recipe-thin`; `cloth` → `material-layers` plus `recipe-cloth`; any row using a cut layout → `material-recipes-cut`. P10b's check rules gain: a blended row with blend off (a warning: it draws opaque), a cut row with cut off (an error with a `fix:` line).

## 6. Proof sets and tests

**Proof scene `?set=b3`** (sandbox, code-built, no new asset): a **street corner** with uv in metres: two wall panels (brick, plaster), a house corner (cube walls with a roof wedge for shingles, slates and tiles), a floor plane (marble chequer, parquet, tiles), a window frame with a pane and a bright backdrop behind it (to judge see-through and the stained back-light), a fence panel (chain-link, lattice) in front of a coloured wall (to judge the cut-out), a rug and carpet plane, a wallpapered wall with a peeling patch. `?row=`, `?q=`, `?yaw=` as before; `?blend=0|1` and `?cut=0|1` flip the engine switches.

**Tests every package adds or extends** (the P4 template, as batch 2): schema equality (appended variants, slots unchanged); naga for each changed recipe alone, with all eight layers, and all together; lite early-return text tests; the derivative grep; catalogue counts (1,054) and every ready row encoding.

**Engine tests:** B3-P2: naga for the cut edit on every text (plain, layers, char, char+layers, both shadow tails, skinned or not, 1x and 4x) with the uniformity analysis on; a text test that a non-MASK material's stage is unchanged. B3-P3: the blend pipeline builds per sample count; a native sort test (back to front, stable on ties by id); a test that with the sub-feature off no blend code is compiled (cargo tree and a symbol check).

**Byte proof:** the 16-file proof (8 games, wasm and js) in **B3-P2, B3-P3 and B3-P9** (each touches evani-render) and in B3-P10 and B3-P13; recipe-only packages prove with `git diff --stat` and the `Cargo.lock` diff. Toolchain check before each, as batch 1.

## 7. Work packages

Common checks as batch 1 and 2 (check clean, clippy native and wasm, tests, DECISIONS with bytes, local commit ending `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`, no push, about 60 tool steps).

| # | Package | Worker | Depends on | Runs |
|---|---|---|---|---|
| B3-P0 | Start: branch `matcat-b3` from `matcat-b2` after B2-P6; DECISIONS "batch 3: design"; toolchain check | sonnet-medium | B2-P6 | sequential, first |
| B3-P1 | **Layout helper** `rc_layout.wgsl` (3.1), masonry's bonds moved into it, unit tests on the CPU mirror of the cell maths (ids stable, edges continuous, herringbone and shingle cells cover the plane with no gaps) | opus-high | P0 | sequential (every pattern package uses it) |
| B3-P2 | **Late cut-out** (3.5), evani-render patch, 16-file proof | opus-high | P0 | **parallel with P1** (evani-render vs the recipe crate) |
| B3-P3 | **Blend pass** (3.5), only if P15 said yes; 16-file proof | opus-high | P2 (same patch and pipeline files) | sequential after P2; parallel with P4 to P8 |
| B3-P4 | **Masonry and stone** (3.2) | sonnet-high | P1 | parallel |
| B3-P5 | **Plaster, plastic, ground's tar cracks** (3.2) | sonnet-high | P1 | parallel |
| B3-P6 | **Wood** (3.2), including closing wood.plank.deck | sonnet-high | P1; P2 for the lattice | parallel |
| B3-P7 | **Metal and paint** (3.2), grate, chain-link and mesh via cut | sonnet-high | P1, P2 | parallel |
| B3-P8 | **Ceramic** (3.2), tiles, mosaic, roof tiles, motifs | sonnet-high | P1 | parallel |
| B3-P9 | **Kit meshes**: spike, then build (3.5); 16-file proof; may move to batch 4 | opus-high | P2 (and P3 if built) | parallel with P4 to P8, never beside another byte proof |
| B3-P10a | **Glass recipe stage** (3.3), adapter (`gltf.rs`: block plus the nature tag), xtask mapping and check rules | opus-high | P1, P3 (or P2 alone if no blend) | parallel with P10b |
| B3-P10b | **Thin and cloth recipe stage** (3.3), adapter, xtask mapping; cloth denim twill closes the batch-1 row | opus-high | P1 | parallel with P10a |
| B3-P11 | **Integrate and prove**: merge P4 to P10b in order (merges, never rebases; add-beside conflicts in `COMPILED`, `ALL_RECIPES`, `BUILT`, `EXTRA_NAMES`, `recipes.json`, the tests and the proof's set list; regenerate the extras numbering), all recipes with all layers through naga, the 16-file proof, sizes once | sonnet-high | P4 to P10b | sequential |
| B3-P12a | **Numbers, exteriors**: brick, stonework, plaster_concrete, wood_build, roofing (52 rows, 5 ready) | sonnet-high | P11, B2-P10 | parallel with P12b |
| B3-P12b | **Numbers, interiors and metal**: metalwork, tiles, interior, glass_build (38 rows, 1 ready) | sonnet-high | P11, B2-P10 | parallel with P12a |
| B3-P12c | **Viewer scenes**: wall panel, floor tile, window with backdrop, fence, roof corner; `viewer_scene` values; `?blend` in the viewer | opus-high | P11 | parallel with P12a and P12b |
| B3-P13 | **Final check and review**: check `--full`, clippy combinations, 16-file proof, contact sheets of the 90 rows at High and Low plus 10 in 4 styles, byte table against section 4, `review` then `critic` once, DECISIONS, the review card | sonnet-high | everything | sequential, last |

(17 packages: P0 to P13 with P10a/b and P12a/b/c.) The row counts in P12a and P12b come from section 2: exteriors 9 + 10 + 12 + 15 + 6 = 52, interiors 10 + 7 + 11 + 10 = 38, together 90.

### Order and what runs together

```
B3-P0 ─┬─ P1 layout ─────┬─ P4 masonry, stone ─┐
       │                 ├─ P5 plaster, plastic ┤
       │                 ├─ P8 ceramic ─────────┤
       │                 ├─ P10b thin, cloth ───┤
       └─ P2 cut-out ─┬──┼─ P6 wood ────────────┼─ P11 integrate ─┬─ P12a numbers ─┐
                      │  ├─ P7 metal, paint ────┤   + byte proof  ├─ P12b numbers ─┼─ P13 final
                      ├─ P3 blend ─ P10a glass ─┤                 └─ P12c viewer ──┘
                      └─ P9 kit meshes ─────────┘
```

- **Parallel groups:** {P1, P2}; then {P3, P4, P5, P6, P7, P8, P9, P10b} with P10a after P3; then {P12a, P12b, P12c}. Each package in its own worktree (`..\evani-dev-matcat-b3-<pkg>`, branch `matcat-b3-<pkg>` from `matcat-b3` after its dependencies merged).
- **File ownership** (why they don't clash): each recipe package owns its `rc_<name>.wgsl` and `src/recipes/<name>.rs`; only P1 edits `rc_layout.wgsl` (later packages that find a layout bug report it to the orchestrator, who sends it back to P1's owner or folds it into P11); only P2 and P3 edit evani-render's patch and pipeline files, one after the other; only P9 edits the kit-surface path; P10a/b own the adapter's glass, thin and cloth sections. Shared lists are one-line add-beside edits P11 resolves.
- **How many at once** (measured 2026-10-24: 16 cores, 64 GB RAM, about half the CPU and 60% of RAM already used by other work): **two sessions building Rust at a time**, each `CARGO_BUILD_JOBS=2` through the low-priority runner, plus sessions that only edit data, docs or numbers. **A 16-file byte proof runs alone** (8 ship builds): P2, P3 and P9 queue their proofs one after another, never beside another build. Each session calls `resource_check` before a build and waits if the PC is over 80%; each worktree's `target/` is several GB, so check free disk before a third worktree. A sensible running order: P1 + P2; then P4 + P6 (P5, P8 queued), with P3's proof slotted when nothing else builds; then P7 + P8, P5 + P10b, P10a + P9.

## 8. Batch 1 gaps, and what batch 3 needs from each

| Gap (batch 1) | Batch-3 rows that depend on it | Where it's best solved |
|---|---|---|
| **Nail heads and chequer patterns** (wood.plank.deck, stonework.marble_floor) | stonework.marble_floor (batch 3's own row), wood.crate, wood.floor.oak, wood.floor.pine, build.iron.rivet_plate, build.steel.girder; plus every herringbone, basket, shingle, panel and motif row (about 35) | **here, B3-P1** (the layout helper), used by P4 to P10b; batch 1's wood.plank.deck becomes ready in P6 |
| **Blend pass** (fire, smoke, holograms opaque; the P15 decision) | the 8 glass rows (see-through panes, stained glass against the light) | **here, B3-P3**, if P15 says yes (this plan recommends yes). It also fixes batch 1's fire, smoke, steam and hologram rows. Without it, glass draws as opaque tinted gloss plus the nature pass where listed, and fire cards can at least use B3-P2's hard cut-out |
| **Cut-out after the recipe** (new, found here) | build.grate, build.chain_link, build.mesh, wood.lattice | **here, B3-P2** |
| **Body-region mask** (deer belly, banded snakes, shotgun shell, racing stripes) | none in batch 3. Logs with chinking, painted brick and painted planks are drawn by uv bands and the `painted` extras, not regions | **start of batch 5** (horses and livestock: most rows waiting on it), as batch 2's plan says: one helper in `rc_shared.wgsl` (a region number plus a second colour from uv v or the vertex colour) |
| **Fur cost on a real GPU** | none | measured in batch 2 (B2-P9), before batch 5 |
| **Cloth twill** (cloth.denim.indigo, batch 1) | wall.carpet.wool, wall.carpet.patterned, wall.rug.woven use the same weave code | **here, B3-P10b**: the denim row becomes ready too |
| **Kit meshes** (batch 1 section 7) | a box-world town | **here, B3-P9** (may move to batch 4 if it runs long) |
| **Condition layers on family materials** | glass (wet, dust, snow on panes), thin (wallpaper wet, wear), cloth (dust and wear on carpets) | solved by the recipe-stage versions in B3-P10a/b, which run the layers like every recipe-stage material |

**3D shells (Stage 8, not planned here).** Rows that would gain from shells once `fur-shells` is approved: wall.carpet.wool, wall.carpet.patterned (pile), roof.sod (grass). Until then they are painted pile and turf, which are also their light versions.

## 9. Not in batch 3 (filed for later)

- **Cut shadows** (a chain-link fence casting a mesh shadow): the shadow pass would need the recipe; later, if a game asks.
- **Real holes and broken plaster, true log profiles, corrugation, standing seams in profile:** geometry. The recipes paint them convincingly at game distance; a model gives the silhouette.
- **Signs, lettering, posters, graffiti:** decals.
- **Rendered swatches per style, styles in the engine:** as batch 1 (Stage 8 for styles).
- **Cloth's sheen into the recipe stage** (dropping the layers family for cloth-only games): batch 4's decision.

## 10. Risks and open questions (each with the recommended answer)

1. **The blend pass is batch 1's open P15 decision.** Recommended answer for P15: yes, built here as B3-P3, an opt-in sub-feature with its own byte proof. If P15 says no, B3-P3 is skipped and glass uses its opaque light version plus the nature pass; nothing else in this plan changes.
2. **Late cut-out and uniform control flow.** Recommended: confirm with naga's uniformity analysis on every text in B3-P2 before building on it; the fallback (explicit-level samples after the cut, or alpha-to-coverage at 4x) is in the package.
3. **The layout helper is a shared dependency for six packages.** Recommended: P1 runs first and alone on its file; its CPU-mirror tests fix the cell maths before anyone builds on it; later fixes go through one owner.
4. **Moving masonry's bonds into the helper changes batch 1's masonry pictures.** Recommended: P1 shows before-and-after captures of every ready masonry row and keeps the look identical (the same maths moved, not rewritten).
5. **Three families gain recipe-stage versions.** Recommended: keep each family as `also` (nothing removed), so a game that already uses the nature or layers family keeps working; the adapter writes both the block and the family's data, as for paint and foliage.
6. **Kit meshes could sprawl.** Recommended: spike first with a written choice; move to batch 4 if it doesn't fit one session after the spike.
7. **Many parallel worktrees on one PC.** Recommended: two Rust builds at a time, proofs alone, `resource_check` before each build (section 7).
