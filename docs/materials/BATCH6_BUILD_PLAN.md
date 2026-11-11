# Evani.Materials: batch 6 build plan (wild mammals, hides, horn and bone)

**In plain words, for Chris.** Batch 6 adds **74 materials**: 50 wild animal coats (deer, bears, wolves, foxes, big cats, zebra, giraffe, small animals from mice to otters), 9 bare hides (elephant, rhino, hippo, walrus, dolphin, orca, whale, armadillo, bat wing) and 15 animal parts (horns, ivory, bone, claws, beaks, turtle shell, quills, paw pads, a dog's nose, whiskers). 7 already work. **40 more can be set right now.** Batch 6 needs **no new engine piece of its own**: its 4 new looks (rhino plates, walrus bristles, armadillo bands, turtle shell plates, plus a proper giraffe pattern) are built inside batch 5's packages, and its **23 marked coats** (a raccoon's mask and ringed tail, a skunk's stripes, a fawn's spots, white bellies, a black-and-white orca) use batch 5's body-region mask. So batch 6 is mostly setting numbers and checking pictures, plus a few more test body shapes (a seal and dolphin body, a heavy bear, a long-necked giraffe, a mouse). The work is **5 packages**; one can start today.

Status: design only (2026-10-25). Nothing here is built. Builders take one package at a time.

Inputs read for this plan: as `BATCH5_BUILD_PLAN.md`; the rows already drawn come from `materials.json` on `b3-patterns`.

## 1. Stage check

- **Stage 3 (Real content)**, inside the catalogue's Stage 3 Includes line. No gate box.
- **Builds on batch 5.** Every new shader part batch 6 uses is built in batch 5's packages (B5-P1 fur, B5-P2 hide and keratin, B5-P3 region), because each shader file has one owner. Batches 5 and 6 share the branch `matcat-animals` (B5-P0).
- **Chris judges batch 5 first**; batch 6's review card follows.
- **Fur shells are in (Chris, 2026-10-25: "the decision should have been to get fur not keep painted").** They are built in batch 5 as the opt-in package **B5-F** (PF moved from Stage 8 to Stage 3; see `BATCH5_BUILD_PLAN.md` risk 1 and `STAGE_MOVE_PF.md`). **Furred wild animals wait for B5-F:** the 50 fur rows (49 wild mammal coats plus antler.velvet). Rows drawn by the hide recipe (rhino, walrus, armadillo, and any bare-skin row such as dolphin or orca) and the horn, bone, tusk, claw, beak, quill and turtle rows do not wait. The painted patterns (tiger, leopard, cheetah, zebra, giraffe reticulated, region coats) stay as the strand root colour under the shells.
- **Not in batch 6:** antler and horn shapes (models), wet-fur animation.

## 2. Scope: the 74 rows

| Subcategory | Rows | Recipe | Drawn already | Numbers now (3.1) | New looks, built in batch 5 (3.2) | Body regions (3.3) |
|---|---|---|---|---|---|---|
| wild_mammals | 50 | fur 49, hair 1 | 4: tiger, leopard, cheetah, giraffe | 25 | giraffe re-pointed to `reticulated` (a revisit, not a new row) | 21 |
| hides | 9 | hide 7, keratin 1, thin 1 | 1: elephant | 3: hippo, whale, bat.wing | 3: rhino (hide `plates`), walrus (hide `bristles`), armadillo (keratin `banded`) | 2: dolphin, orca |
| animal_parts | 15 | keratin 11, hide 2, fur 1, hair 1 | 2: horn.cattle, bone.fresh | 12 | 1: turtle.shell (keratin `scutes`) | 0 |
| **Total** | **74** | fur 50, keratin 12, hide 9, hair 2, thin 1 | **7** | **40** | **4** | **23** |

7 + 40 + 4 + 23 = 74. Running total: **614 drawn before batch 6** (after batch 5), **681 after it**, of 1,059 (64%).

**Revisits** (rows already `ready` that improve with batch 5's parts, shown on the review card as a look change on a ready row): tiger.coat (white belly: face and underside regions), giraffe.coat (`reticulated` instead of `patches`).

## 3. What covers them

### 3.1 Numbers now (40 rows, no code)

On today's fur, hide, keratin, thin and the family's hair:

- **Coats (25):** deer.coat.winter (dense, hollow: high density, low sheen), moose.coat.dark, mountain_goat.coat (long), bear.coat.grizzled (pale tips), bear.coat.cinnamon, bear.coat.polar, wolf.coat.white, coyote.coat (rusty legs: `points` with a warm pattern colour), fox.coat.arctic, fox.coat.silver (tips), lynx.coat (faint spots), lion.coat, zebra.coat (stripes, full contrast), camel.coat, kangaroo.coat, monkey.coat.brown, hare.fur.winter (dark ear tips: `points`), squirrel.fur.red, beaver.fur (sheen), mouse.fur, rat.fur, boar.coat (coarse: clump), seal.fur (spots, the wet layer's default amount), mink.fur (sheen); lion.mane on the family's hair.
- **Hides (3):** hippo.hide (slick, pink creases in colour B), whale.skin (warty, pale barnacle clusters), bat.wing (thin `membrane`, light through).
- **Parts (12):** antler.velvet (short fur, fuzzy), horn.dark, horn.pale_ringed (rings), ivory.tusk (tooth, fine grain), bone.bleached (bone, chalky), claw.dark, beak.yellow, beak.black, quill.porcupine (horn with a dark tip), paw.pad (hide warty, rough), nose.dog (hide warty, wet), animal.whisker (family hair).

### 3.2 New looks (4 rows and 1 revisit), built in batch 5

| Row | Built by | Look |
|---|---|---|
| rhino.hide | B5-P2 hide variant `plates` | thick plates with deep folds |
| walrus.hide | B5-P2 hide extra `bristles` over `warty` | lumpy cinnamon-pink skin, sparse bristles |
| armadillo.shell | B5-P2 keratin variant `banded` | rings of small plates with soft joints |
| turtle.shell | B5-P2 keratin variant `scutes` | polygon plates, growth rings radiating in brown and amber |
| giraffe.coat (revisit) | B5-P1 fur variant `reticulated` | polygon patches split by cream lines |

Budgets are batch 5's (section 4 there); batch 6 adds no code.

### 3.3 Body regions (23 rows): batch 5's mask, plus four questions for its research

The rows: deer.coat.fawn (rows of white spots along the back), elk.coat.tan (dark neck and legs), caribou.coat.grey (white mane and rump), pronghorn.coat (white bands on throat and belly), bighorn.coat (white rump), bison.coat (woolly hump and mane, short hind coat), bear.coat.black (tan muzzle), wolf.coat.black (grey muzzle), fox.coat.grey (rust sides), cougar.coat (pale belly), ape.coat.black (silver back), rabbit.fur.wild (white tail), squirrel.fur.grey (white belly), chipmunk.fur (back stripes), otter.fur (silvery throat), raccoon.fur (black mask, ringed tail), skunk.fur (two white back stripes), badger.fur (face stripes), opossum.fur (pale face), sea_otter.fur (pale head), ermine.fur (black tail tip), dolphin.skin (paler belly), orca.skin (white patches).

**Batch 6 adds four questions to B5-R1** (research first; the answers go into the same research note and DECISIONS proposal):

1. **Bodies without legs.** Dolphin, orca, whale and seal have a back, a belly, flippers and flukes. Does the region convention (and the convert-time bone dictionary) cover a finned body, or does a finned body need its own region names?
2. **Regions that change the coat, not only its colour.** Bison (long woolly front, short hind coat), caribou and lion (a mane line), the ape's silver back (a tip colour). Should a region carry a length, tip or pattern-contrast change as well as a colour, and where would those numbers live in a full fur block?
3. **Patterns placed along a region's line.** A fawn's rows of spots along the spine, a skunk's two stripes and a chipmunk's three run along the back, not across the fur's uv. Can the region give a coordinate along and across its centre line (distance from the spine), so a pattern can follow it?
4. **Rings and masks.** A raccoon's ringed tail (bands round a region along its length) and a raccoon's or badger's face mask and stripes. Are these regions, region coordinates (question 3) or decals?

If B5-R1 finds that any of these can't be done without a heavy change, the row stays `planned` with the written reason and goes to the parking list; batch 6 is never blocked as a whole.

## 4. Byte and GPU budgets

- **Code:** 0 new bytes in batch 6 (every part is batch 5's; its sizes are in BATCH5 section 4).
- **B6-P1 body shapes:** dev only (the sandbox proof and the viewer), 0 in any game.
- **GPU:** the wild page (`?set=b6-wild`, 10 animals of mixed size) at High 1080p ≤ 2.5 ms whole frame on the RTX 4060 Ti; phone-tier capture drawn at Lite with every pattern and region.

**A forest with wildlife** (batch 2's forest at about 19 KB, plus fur with regions 1.95, hide 1.2, keratin 1.2, the region helper 0.5): about **24 KB gzip**, without the character family unless a lion's mane or whiskers are drawn as hair (+13.6 KB; a game can draw them as fur instead).

## 5. Tiers and live params

- No tier file changes; no new params (batch 5's `material.recipe.region.*` and `material.recipe.hide.bristles` cover everything). Lite draws what Low draws for every row.

## 6. Proof sets and tests

- **Proof scene `?set=b6-wild`** (sandbox, code-built): B5-P4's quadruped in new proportions from B6-P1 (deer, bear, big cat, small rodent, long-necked giraffe), a finned body (seal, dolphin, orca, whale), a bat-wing membrane card, and a parts row (horns, tusk, bones, claws, beaks, shells, quills, a paw pad, a nose on the ball and trunk shapes). `?row=`, `?q=`, `?debug=regions`, `?wet=`.
- **Tests:** catalogue counts and encodings; B6-P1's shapes pass the viewer's shape test (no degenerate triangle, normals on their triangles' side) and the viewer's geocheck with 0 untagged issues; the finned body keeps its regions in every pose (B5-P3's CPU mirror).
- **Byte proof:** B6-P1 touches only the sandbox proof module and the viewer's shapes (dev only): a sandbox build pair and the viewer's own checks; the final check B6-P2 proves all 16 files against `sha-base-b456.txt`.

## 7. Work packages

Common checks as batch 4 section 7.

| # | Package | Worker | Depends on | Runs |
|---|---|---|---|---|
| B6-N1 | **Numbers now**: the 40 rows of 3.1 on the ball and trunk shapes | sonnet-high | `matcat-engine` | wave 1b (one build) |
| B6-P1 | **Body shapes**: deer, bear, big cat, rodent and giraffe proportions, a finned body with regions, a bat-wing card; sandbox `?set=b6-wild` and viewer shapes | opus-high | B5-P4 (the quadruped), B5-R1's answers to 3.3 | after B5-P4, parallel with B5-P5 |
| B6-N2 | **Numbers, region coats and revisits**: the 23 region rows; tiger and giraffe revisits | sonnet-high | B5-P5, B6-P1 | after B5-P5 |
| B6-N3 | **Numbers, new looks**: rhino, walrus, armadillo, turtle (4 rows) | sonnet-medium | B5-P2 merged into `matcat-animals` | as soon as B5-P0 has merged B5-P2; parallel with anything |
| B6-P2 | **Final check and review** (section 7.1) | sonnet-high | everything, and B5-P7 | sequential, last |

(5 packages.) 40 + 23 + 4 = 67 rows set here, plus the 7 already drawn = 74.

### 7.1 Done when (B6-P2)

- `cargo xtask check` and `cargo xtask check --full` clean.
- **Byte proof 16/16 `same`** against `sha-base-b456.txt`.
- **Contact sheets of all 74 rows at High, Low and Lite** on their body shapes, with `?debug=regions` beside each region row; the two revisits before and after.
- **Same look on every device:** a phone-portrait viewer capture of 12 rows (raccoon, skunk, orca, fawn, zebra, giraffe, tiger, rhino, turtle, polar bear, seal, horn) beside the desktop one; every pattern and region at Lite.
- **Every art style keeps detail:** 12 rows (raccoon, skunk, orca, fawn, giraffe, zebra, tiger, grizzled bear, rhino, turtle, armadillo, pale ringed horn) in all 24 styles; each tells apart from a plain coat or plain hide in every style; weak spots named.
- The GPU table against section 4, `review` then `critic` for material-view once, DECISIONS with every number, the review card (what to judge, the two revisits, shells on and off beside each other on a bear, a lion and a deer).

### 7.2 Order and what runs together

```
wave 1b (now)        B6-N1 numbers now ──────────────────────────────────────┐
batch 5 (B5-P0) ──── B6-N3 new looks (after B5-P2 is merged) ────────────────┤
batch 5 (B5-P4) ──── B6-P1 body shapes ─┐                                    │
batch 5 (B5-P5) ────────────────────────┴─ B6-N2 region coats, revisits ─────┴─ B6-P2 final (after B5-P7)
```

- **Worktrees:** `..\evani-dev-matcat-b6-<pkg>`, branch `matcat-b6-<pkg>`; N1 from the `matcat-engine` tip, the rest from `matcat-animals`.
- **File ownership:** B6-P1 owns the new body shapes (it adds to B5-P4's files after B5-P4 merged); the N packages own only their rows in `materials.json` (one row per line block, so merges are clean).

## 8. Earlier gaps, and what batch 6 needs from each

| Gap | Batch-6 rows | Where it's solved |
|---|---|---|
| Body-region mask | 23 + the tiger revisit | B5-R1 (with 3.3's four questions), B5-P3 |
| Fur, hide and keratin looks | rhino, walrus, armadillo, turtle, giraffe | B5-P1, B5-P2 |
| Bristles | walrus | B5-P2 |
| Fur shells | the 50 fur rows (wild mammal coats, antler velvet) | B5-F (Chris's decision, 2026-10-25) |

## 9. Not in batch 6 (filed for later)

- Horn, antler, tusk and quill shapes; ear tufts (lynx); whisker geometry: models.
- Scars, brands, tags: decals.
- Batch 7's birds, reptiles and fish reuse the region mask and B4-P1's slit pupils; their own plan follows batch 6.

## 10. Risks and open questions (each with the recommended answer)

1. **Four region questions may not all have a light answer.** Recommended: each row that can't be drawn stays `planned` with its reason; batch 6 finishes without it.
2. **Revisiting ready rows (tiger, giraffe) changes what Chris has already seen.** Recommended: show both on the review card, old and new side by side; keep the new only if he agrees at the review.
3. **Hair rows (lion.mane, animal.whisker) pull the 13.6 KB character family into a wildlife game.** Recommended: note it in the review card and README; a game can draw a mane with fur's long-coat numbers instead (a numbers choice, no code).
4. **Batch 6 depends on batch 5 almost entirely.** Recommended: run N1 (40 rows) at once so batch 6 is half done before batch 5's region lands; the rest follows batch 5's integration directly. The furred rows of N1 are set painted first and get their shell length and density after B5-F lands; B6-N2 (region coats) and B6-P2 (final) wait for B5-F as well as B5-P5.
