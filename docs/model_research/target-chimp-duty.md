# Target: the Chimp Duty soldier, modelled fully by Evani.Model

Goal and plan, 2026-10-16. Docs only: nothing here is built yet. It extends the character plan (docs/model_research/plan-characters.md, milestones MC1 to MC6, on branch `evani-model`) and the brief (docs/EVANI_MODEL_BRIEF.md).

## 1. The goal

On 2026-10-16 Chris sent a character sheet and wrote: **"use this as one of the goals to be able to model this fully in Evani.model"**.

- Sheet: `art-source/concepts/chimp-duty/chimp-duty-sheet.png` (1536 x 1024 PNG; `art-source/` is gitignored, so the sheet stays out of git).
- The character: the "Chimp Duty" soldier from Chris's own ChimpDuty project. A stylised (animated-film-like) chimpanzee soldier: brown fur, camo helmet with a four-tube night-vision mount and a headset with a boom mic, a cigar, a black short-sleeved shirt with shoulder patches (a chimp face, a banana), a tan plate-carrier chest rig covered in pouches with a banana patch, grenades and a radio antenna, fingerless gloves with knuckle guards, wrist straps, baggy camo trousers with knee pads and straps, a thigh holster with a pistol, open-toe tactical boots with bare chimp toes, a rifle with a yellow banana-marked grip and magazine, and a backpack.
- The sheet shows: a front three-quarter view (holding the rifle), a side view (arms down), four face expressions (neutral, angry, confident, yelling with the mouth open and teeth showing) and four detail panels (helmet and NVG, chest rig, glove, backpack).

**What "fully" means.** Evani.Model can be said to model this character fully when, from an intent document and with no hand-made mesh:

1. **Every part on the sheet is modelled**: body and fur, face, the four expressions, all gear (section 2's list, nothing left out) and the rifle.
2. **It is coloured and textured to read like the sheet** at game distance (a soldier about 5 m away) and in a close-up (the face panels and the four detail panels).
3. **It is rigged so it can move**, on the MC1 skeleton (plus the digits and face joints this plan adds), and can be posed like the sheet.
4. **It fits the per-tier budgets** of section 5 on every game tier.

Each point is measured in section 4. "Reads like the sheet" is judged by numbers first, then the critic, then Chris.

## 2. Part-by-part breakdown

Status: **exists** (on branch `evani-model` today, from M1 or the MC1 work in progress), **planned** (named in a plan milestone), **MISSING** (in no plan yet). MC numbers are from plan-characters.md section 9; M numbers are the brief's milestones.

### Body and face

| Part on the sheet | Capability it needs | Status |
|---|---|---|
| Ape body: long arms, short legs, broad shoulders, slight hunch, big hands and feet | `character` template with layered body parts on a skeleton, proportions as measures | Template, skeleton, layers and measures exist (MC1). **MISSING**: an ape body variant (arm and leg ratios, hunch, longer torso, ranges set by our own ape proportion rules) |
| Hands with five fingers, big knuckles | finger variant of the hand, finger joints | Planned (MC2 "finger variant") |
| Bare feet with five toes and an opposable big toe (they stick out of open-toe boots) | toe digits as parts, toe joints | **MISSING** (MC1 has one `toe_l`/`toe_r` joint and a foot block; no toe digits anywhere in the plan) |
| Chimp head: muzzle, wide lips, flat nose with nostrils, heavy brow ridge, deep eye sockets, big round ears | head variant with face parts sized by measures | Face masses planned (MC2 face planes); real eyes, lids, lips, nostrils, ears planned for high and ultra (MC5). **MISSING**: a muzzle head variant (the plan has humans only; its "pumpkin fiend" head variant is named but not designed) |
| Bare skin areas (face, ears, palms, fingers, toes) in a lighter tan, separate from the fur | colour regions | Region colours exist (MC1, vertex colours); skin material planned (MC3) |
| Eyes (brown iris, readable at distance) | eye regions, eyeballs | Planned: colour region (MC1/MC2), geometry on high and ultra (MC5) |

### Fur

| Part | Capability | Status |
|---|---|---|
| Fur over arms, shoulders, legs (below the knee pads), back of the head, cheeks and around the face | fur that reads at game scale: a fur volume over the body, clumped tufts breaking the silhouette at edges (forearms, elbows, cheeks, shins), strand direction, darker roots and lighter tips | **MISSING** as a whole. Partly covered by planned pieces: hair and beard tufts (MC2), envelope substitution per tier (MC2), hair material with a strand-direction normal map (MC3). Nothing plans fur over a whole body, fur direction fields, or silhouette breakup as a rule |

### Clothes

| Part | Capability | Status |
|---|---|---|
| Black short-sleeved shirt (sleeves end above the elbow, collar) | garment shell over body parts with keep bands | Exists (MC1 `layer` generator with bands and cuts) |
| Baggy camo trousers bunched at the knees and ankles | garment shell with drape and loose volume | Shell and drape exist (MC1). **MISSING**: fabric folds (bunching under straps and above boots, knee creases) as geometry on high and ultra and as normal map below; MC3 only plans fold *shading* |
| Camo pattern (trousers, helmet, backpack, boot uppers, pouches) | procedural colour patterns in regions, our own camo design | **MISSING** (MC3 plans per-material looks such as leather and cloth, and M4's pattern library is height-only: no colour patterns, no camo) |
| Shoulder patches: chimp face (left arm), banana (right arm) | emblem shapes as decals: flat raised patch geometry plus a colour picture drawn by code | **MISSING** (no decals or emblem drawing in any plan) |
| Fingerless gloves with padded knuckle guards and a wrist strap | shell over the hand with finger cuts; knuckle guards as a row of small parts along the knuckle line | Shells and cuts exist (MC1); fingers planned (MC2); rows of parts along a curve planned (MC2 pattern parts). Small gap: a cut that ends each finger shell at the middle joint |
| Knee pads (hard cap, straps behind the knee) | accessory shell over the knee plus strap bands | Shell exists (MC1); straps: see gear kit |
| Open-toe tactical boots: laced upper, thick sole, toes out | boots shell (exists, MC1) with a front cut, sole and laces | Boots exist (MC1); front cut possible with today's cuts; laces as relief planned (M4 `stitching` is close; a `laces` pattern is **MISSING**) |

### Gear (hard surface and soft goods)

| Part | Capability | Status |
|---|---|---|
| Helmet: rounded shell, rails on the sides, padding visible inside | shell over the head plus rail parts | Shell and rounded boxes exist (M1, MC1). Rails as repeated parts: planned (MC2 pattern parts) |
| NVG mount with four tubes, lenses, mount arm | small hard-surface prop: cylinders, lathes, a mount box, lens regions | Generators exist (M1: cylinder, lathe, rounded_box). **MISSING**: a prop class for it (no template; today the kart, sword and shield are the only hard-surface objects) |
| Headset: ear cups, band, boom mic | accessory at head anchors, sweep for the boom | Generators exist (sweep, ellipsoid). **MISSING**: ear anchors and a template entry |
| Cigar (lit tip) in the mouth corner (in both full-body views and the confident face; not in the other three faces) | small lathe at a mouth anchor, switchable per expression; a glow region | Lathe exists. **MISSING**: a `mouth_corner` anchor that follows the jaw and lips (MC5 adds `jaw`, not mouth anchors) |
| Plate carrier: front and back plates, shoulder straps, side cummerbund | shell over torso with panel cuts, strap bands | Shells exist (MC1). **MISSING**: a strap kit (flat bands that follow a surface along a curve, with buckles and ends) |
| Pouches (magazine, utility, radio) in rows, flaps, buckles | a pouch generator (soft rounded box with a flap and a buckle), laid out in rows and grids on a surface | Rounded boxes and rows exist or are planned (M1, MC2). **MISSING**: a pouch generator with soft-goods shape (slight sag, rounded seams) and grid layout on a curved surface |
| MOLLE webbing (horizontal loops on rig, pouches, backpack) | a webbing relief pattern (bands with regular slots) | **MISSING** (not in the brief's pattern library; `panel lines` and `stitching` are the nearest, M4) |
| Grenades on the rig, radio with antenna | lathe and cylinder props, thin tube antenna | Generators exist (M1). Thin antenna survives tiers by the min-feature rule (exists) |
| Banana patch on the chest rig | decal (as the shoulder patches) | **MISSING** (decals) |
| Thigh holster with pistol, leg straps | holster shell over the thigh, pistol as a prop, straps | Shells exist (MC1, the Marshal's holster is an accessory). Pistol: **MISSING** (prop class). Straps: **MISSING** (strap kit) |
| Rifle: receiver, rails, barrel, stock, scope or sight, yellow banana-marked grip and magazine | a rifle prop class: boxes, cylinders, rail as repeated slots, grip and magazine with colour marks | Generators exist (M1). **MISSING**: a rifle class template (generic and original, no real-world model copied), colour marks on hard surfaces (decals), and a hold pose with two hand anchors |
| Backpack: main bag, side pouches, top straps, antenna, banana patch, MOLLE | soft-goods bag, pouches, straps, decal, attached at a back anchor | **MISSING**: back anchor, pouch generator, strap kit, decal (as above) |
| Wear and dirt on all gear | edge wear, dust in cavities | Planned: wear by plausibility (M4 relief) and colour wear (MC3) |

### Rig, pose and expressions

| Part | Capability | Status |
|---|---|---|
| Skeleton, skin weights, anchors | MC1 skeleton and weights | Exists (MC1) |
| Finger and toe joints | digit variant of the skeleton | Fingers planned (MC2); toes **MISSING** |
| Gear following the body (straps on the chest, pads on the knees, holster on the thigh) | accessories inherit the bone under them | Exists (MC1 rule); deformation tests planned (MC2) |
| Sheet pose (front view: rifle held across the body; side view: arms down) | pose files: named joint rotations applied to the rig, for sheets and the compare tool | Test poses planned (MC2 deformation tests). **MISSING**: named pose files usable by the compare tool and the sheets |
| Four expressions: neutral, angry (brows down, lips pressed), confident (half smile around the cigar, one brow down), yelling (jaw wide open, teeth and tongue showing, brows up and in) | face joints or morph targets on the **game** tiers, plus a mouth interior | Jaw and eye joints planned (MC5); lids, mouth interior, teeth, tongue and an expression set planned only for the `closeup` tier (MC6). **MISSING**: expressions on the game tiers (high and ultra), a chimp mouth (big canines) and these four specific expressions |

## 3. Gaps, ranked by how much of the character depends on them

| Rank | Gap | What depends on it | Size | Extends |
|---|---|---|---|---|
| 1 | **Ape body variant**: ape proportions, muzzle head, big ears, hand fingers and foot toes with joints | Everything: every garment, strap and pad is a shell over the body, so the body must be right first | L | MC1 template, MC2 finger variant |
| 2 | **Gear kit**: prop classes (helmet with NVG, headset, plate carrier, pouch, grenade, radio, holster, pistol, rifle, backpack, knee pad), a strap kit, a pouch generator, new anchors (mouth corner, ears, chest, back, thigh) | About 40% of the visible surface (helmet, rig, pouches, backpack, rifle) and most of the silhouette's detail | L | M1 generators, MC1 accessories, MC2 pattern parts |
| 3 | **Fur at game scale**: a fur volume over body parts, silhouette breakup clumps, direction field, root-to-tip colour; per-tier rules | About 30% of the visible surface (arms, shins, head) and the character's whole read as a chimp | XL | MC2 tufts and envelopes, MC3 hair material, M4 relief |
| 4 | **Colour patterns and decals**: our own camo patterns, MOLLE webbing relief, laces, emblem decals (banana, chimp face) drawn by code | Trousers, helmet, backpack, boots, every pouch and the three patches: most of the colour read | L | MC3 materials, M4 pattern library |
| 5 | **Game-tier expressions**: brow, lid, lip-corner and jaw joints or morph targets on high and ultra, chimp mouth interior (teeth, canines, tongue), the four named expressions | The four face panels; any close camera | M | MC5 face parts and joints, MC6 expression set (brought down to game tiers) |
| 6 | **Fabric folds**: bunching and creases as geometry on high and ultra, as normal maps below | The trousers and sleeves read as cloth, not plastic | M | MC3 cloth, M4 relief, MC2 envelope rule |
| 7 | **Pose files**: named poses for sheets and the compare tool (sheet pose, hold-the-rifle pose) | The front three-quarter comparison and the rifle hold | S | MC2 test poses |
| 8 | **Image targets in the compare tool**: compare against a 2D sheet instead of a 3D master | Measuring every point of section 4 | M | the compare tool (MC1) |

Also needed, but small: a finger-end cut for fingerless gloves (S), a `laces` pattern (S), a glow region for the cigar tip (S, MC3 materials).

**The brief, stretched once more.** plan-characters.md already extends the brief to colour, skinning and native organic characters. This goal adds, beyond that plan: fur over a whole body, colour patterns and decals, expressions on the game tiers, and prop classes for weapons. Each needs Chris's approval in its milestone, as the plan's own extensions do.

## 4. How "can model this fully" is measured

The compare tool (on `evani-model`, from branch `evani-model-compare`) compares our model with a **3D master**: same orthographic views, aligned without rescaling, silhouette IoU per view, region proportions, a side-by-side sheet. The Chimp Duty target is a **2D sheet**, so it needs a reference pack and an image mode (gap 8).

**Reference pack** (made once from the sheet, kept with it under `art-source/`, outside git): 

- The side view and the front three-quarter view cropped out, with their silhouette masks (the sheet's background is plain white: a threshold plus a hand check, no learned segmentation). The rifle and the figure are separate masks where they overlap in the front view, so the body can be scored without the rifle.
- A pose file for each view (joint rotations that put our rig in the sheet's pose, gap 7) and a camera for the three-quarter view (yaw and pitch fitted once by a small search over our own renders, then frozen).
- About 40 colour sample points per view, each tagged with a region (fur, skin, shirt, trouser camo light and dark, rig tan, helmet camo, glove, boot, rifle black, rifle yellow, banana yellow, ...).
- The part checklist of section 2 as data: every row a named part and region the compiled model must contain.
- The four face panels and four detail panels as crops, for the critic and for Chris.

**Image mode for the compare tool.** Our model is rendered in the view's pose and camera, at the sheet's crop size. The sheet has no scale, so this mode fits one uniform scale on the figure's height and reports it (unlike the 3D mode, which never rescales). Silhouette IoU and proportions are then scored exactly as in the 3D mode; the same sensitivity test applies (identical masks score 1.0, a model 10% taller or wider loses at least 0.05).

**Acceptance (all must pass):**

| Check | Pass when |
|---|---|
| Side-view silhouette (arms down, the cleanest view) | IoU at least 0.85 on the ultra tier |
| Front three-quarter silhouette (rifle pose), body mask | IoU at least 0.80 (a fitted camera and a posed rig cost some accuracy) |
| Proportions from the masks | head height, helmet width, shoulder width, arm length (shoulder to knuckles), leg length, foot length and backpack depth (side view), each within 7% of the sheet's ratio to total height |
| Part checklist | every row of section 2 present as a named part and region on high and ultra; on medium and low, present or replaced by its envelope (the MC2 rule), never silently dropped |
| Colour | per region, mean colour difference at most ΔE 15 against the sheet's sample points (MC3's colour compare, image mode); camo regions also match the sheet's light-to-dark share within 10 points |
| Expressions | all four exist as named joint sets or morph targets on high and ultra; each moves only the face (no helmet, headset, body or gear vertex moves more than 1 mm; the cigar moves only with the mouth); yelling opens the jaw at least 25 degrees with teeth and tongue visible in a front render; the cigar is a per-expression option: present in confident and in both full-body views (where it follows the mouth corner), absent in neutral, angry and yelling, as on the sheet |
| Rig | MC1's checks (1 to 4 influences, weights sum to 1, validator clean) plus MC2's test poses with fingers and toes; straps, pads and holster stay on their body parts in the sheet pose (no gap over 5 mm, no new self-intersection beyond a small reported count) |
| Determinism and checks | identical bytes at 1 and 8 threads, Khronos validator clean, meshes clean, geocheck with no untagged issues once evani-convert exists |
| Critic pass | the critic (note 11) scores each of the 10 panels (two views, four faces, four details) against our renders of the same view, 1 to 5 for "reads like the sheet"; every panel at least 4 |
| Chris | Chris looks at a side-by-side sheet (his sheet next to our renders, same views) and says it reads like his character. Only Chris signs this off |

## 5. Per-tier budgets

Assumption: the soldier stands about 1.5 m to the top of the helmet (the sheet has no scale; Chris can set it). He carries far more gear than the Marshal, so the character budget is a little above plan-characters.md section 7, and the rifle is a separate model with its own budget (it can be dropped, swapped or held by another character).

| Tier | Character triangles | Rifle triangles | Maps (from MC3) | Keeps |
|---|---|---|---|---|
| low | 4,000 | 600 | 256 | body with fur envelope, helmet, NVG as one block, rig and backpack as masses, boots, camo as colour |
| medium | 10,000 | 1,500 | 512 | plus pouches as shapes, knee pads, holster, headset, separate NVG tubes, face planes, webbing and folds in the normal map |
| high | 25,000 | 4,000 | 1024 (2048 with MC5) | plus fur clumps on the silhouette, fingers and toes, knuckle guards, straps and buckles, expressions, cigar |
| ultra | 70,000 | 10,000 | 2048 (4096 with MC5) | everything as geometry: fold geometry, all pouches with flaps, grenades, antenna, mouth interior |
| closeup (MC6, optional) | 120,000 to 200,000 | 20,000 | two 4096 atlases (body, face) | for the face panels and detail panels at full detail |

The head share rule of MC5 (about 15% on high and ultra) applies; the helmet and NVG are counted separately and reported, as the plan does for the hat.

## 6. Proposed milestone order (after the current Marshal work)

A plan only. Each milestone ends with a report and Chris's approval, like MC1 to MC6. MC2, MC3 and MC5 are needed on the way and come first where shown.

| # | Milestone | Size | Extends | Done when |
|---|---|---|---|---|
| 1 | **MC2** as planned, plus toe digits and named pose files (gap 7) | M (MC2) + S | MC2 | MC2's tests pass; a sheet pose and a rifle-hold pose load and deform cleanly |
| 2 | **CD1 ape body** (gap 1): ape template variant, muzzle head, ears, hands and feet with digits | L | MC1 template, MC2 | a bare chimp (no gear) compiles on all tiers; side-view silhouette of the fur-less body is reported against the sheet once CD2 exists |
| 3 | **CD2 sheet reference pack and image compare** (gap 8) | M | compare tool (MC1) | the pack exists; image mode passes its sensitivity test; first scores for the bare chimp |
| 4 | **CD3 gear kit** (gap 2): prop classes, strap kit, pouch generator, new anchors | L | M1 generators, MC1, MC2 | every gear row of section 2 present on high and ultra; part checklist passes for gear; silhouette scores reported |
| 5 | **MC3 materials**, then **CD4 colour patterns and decals** (gap 4) | MC3 as planned + L | MC3, M4 | colour acceptance of section 4 met; patches drawn by code, original |
| 6 | **CD5 fur** (gap 3) | XL | MC2, MC3, M4 | fur on every tier by the envelope rule; silhouette breakup present on high and ultra; side-view IoU at least 0.85 with fur |
| 7 | **CD6 folds** (gap 6) | M | MC3, M4 | fold geometry on high and ultra, normal maps below; critic scores trousers and sleeves |
| 8 | **MC5**, then **CD7 game-tier expressions** (gap 5) | MC5 as planned + M | MC5, MC6 | the four expressions pass section 4 |
| 9 | **CD8 fit and sign-off**: MC4's search fit run against the image compare, critic pass, Chris's look | M | MC4 | every row of section 4's acceptance passes |

Fur comes after the gear because the gear covers much of the body and decides where fur shows; expressions come late because they need MC5's face parts. If any milestone shows that a target in section 4 is out of reach, it stops and reports, as M2 does.

## 6b. Look-upgrade requests from ChimpDuty (2026-10-17, from ChimpDuty, sent with Chris's OK)

Report: `..\ChimpDuty\.vibedeck\research\2026-10-17-high-end-look-for-code-built-characters.md` (called "the report" below). Finding: the mesher already keeps edges, `surface.rs` already has charts, a packer, tangents and the normal bake, and `model.wgsl` already samples base colour, normal and emissive. The code-built chimp looks blobby and flat only because it uses per-vertex colour and toon bands, so the gap is the bake and the lighting, not the mesher. A plan only: nothing here is built, no milestone or gate changes, and each item still needs Chris's approval in its own milestone. **Rule for all 13 items (Chris, relayed by the ChimpDuty orchestrator, 2026-10-17): "all must be tools specifically developed for our Evani.dev and Evani.model".** Every item is built from scratch for Evani.Dev and Evani.Model. The outside projects named in the report and below (XeGTAO, Bevy, three.js, xatlas, Infinigen, GarmentCode, the papers) are for ideas only: no libraries, no porting or copying of their code even where the licence allows it, and no new dependencies. `evani-model` keeps `serde` and `serde_json` as its only dependencies; every recipe and generator is original code. Licences, for the record only: XeGTAO, Bevy, three.js and xatlas are MIT or Apache; Infinigen is BSD-3; "Dual Contouring of Signed Distance Data" (2026) is CC BY-NC-SA. All are ideas only, and nothing is copied from any of them.

The ChimpDuty rank is kept in the first column. Order of work inside this track: 11, then 1 and 3 together (they ride on MC3 materials), then 4, then 8 and 7 (with CD3), then 5 and 10 (with CD6), then the model half of 6 (with CD5), then 12 last.

| CD rank | Item | Crate, effort | Where it fits (milestone) | Notes |
|---|---|---|---|---|
| 1 (TOP) | Albedo, roughness and AO bake from code-defined material recipes (camo, weave, stitching, leather, metal, rubber) | `evani-model` `surface.rs` + glTF writer, M | **MC3 materials** (pull its albedo and roughness part ahead of CD3 if Chris agrees; it needs only M1's `surface.rs`) | Reuses the normal bake's texel-to-surface step; inputs from the field and its gradient. Fixes stair-stepped camo, flat shirt and boots. Pairs with CORE_REQUESTS `smooth-lighting` (the engine half) |
| 2 (TOP) | Smooth "stylised realistic" lighting mode | `evani-render`, L | Not this track: CORE_REQUESTS.md row `smooth-lighting`, Stage 5 | Evani.Model only supplies the roughness and AO maps (item 1) |
| 3 | Curvature-driven wear and dirt | `evani-model`, S-M | **MC3**, with item 1 | Convex curvature gives edge wear, concave gives dirt; a few extra field samples along the normal, no rays |
| 4 | Vector decals and patches stamped into the albedo bake | `evani-model`, S | **CD4 colour patterns and decals** (gap 4) | Needs item 1's albedo path first; replaces the blobby per-vertex badges |
| 5 | High-detail normal bake (stitching, weave, wrinkles) | `evani-model`, M | **M4 relief** (pattern library) and the fine layer of **CD6 folds** | Extends the existing exact normal bake |
| 6 | Fur: shells plus fins, baked clump texture as the low-tier fallback | model half `evani-model`; runtime half `evani-render`, L | Model half: **CD5 fur** (gap 3, the baked clump colour and normal texture and hair cards); runtime half: CORE_REQUESTS row `fur-shells`, Stage 8 | The clump texture also serves as the fallback on tiers without shells, so CD5 can ship without the runtime half |
| 7 | Kit generators: PALS webbing, pouches, laces, straps, buckles | `evani-model`, M | **CD3 gear kit** (gap 2) and the `laces` pattern in "Also needed" | Stitches go into the texture bake (items 1 and 4), not geometry |
| 8 | Hard-surface primitives: polygon extrude, chamfered blend | `evani-model`, M | **CD3 gear kit**, before the helmet, rifle, holster and NVG | Rail and bevel helpers belong here too |
| 9 | Soft shadows (PCSS) and contact-shading polish | `evani-render`, S-M | Not this track: CORE_REQUESTS.md row `soft-shadows`, Stage 5 | |
| 10 | Fold and wrinkle generator | `evani-model`, M | **CD6 folds** (gap 6) | Stress-line ridges for trouser folds; below the High tier the folds come as normal-map detail from CD rank 5 (the high-detail normal bake); item numbers in this table are ChimpDuty ranks |
| 11 | Per-part cylindrical and spherical UV charts | `evani-model`, S | First: before item 1, and used by **CD1 ape body** (face, hands, curved clothing) | Fixes texture stretch once the albedo bake lands; a full xatlas-style port stays low priority |
| 12 | Adaptive octree meshing | `evani-model`, L | After CD3 as an optional budget step; already noted as planned in `mesh.rs` | Stretches the triangle budget of section 5; not on the sign-off path |
| 13 | Grip-pose helper for fingers on a held rifle | separate kit, S-M | Extends gap 7 (pose files: the rifle-hold pose) and CD1 fingers | Parked in STAGES.md Parking lot as a separate kit; the named hold pose itself stays in MC2 and gap 7 |

## 7. Originality

This is Chris's own character from his own ChimpDuty project, so it is a legitimate target to reproduce: the sheet is a reference for measuring (silhouettes, proportions, colours), and no pixel of it enters Evani.Model's output. Everything is built by our own generators and patterns.

Keep third-party trademarks and real insignia off the model:
- No real brand logos, maker marks or model names on the NVG, headset, rifle, pistol, boots or pouches. The rifle and pistol are generic and original, not a copy of a real, trademarked firearm design.
- No real-world insignia: no national flags, unit patches, rank badges or camo patterns copied from a real military or a commercial camo maker. Our camo is our own procedural pattern in the sheet's colours.
- The only marks are Chris's own: the banana patch, the chimp-face patch and the banana marks on the rifle. The "CHIMP DUTY" title on the sheet is not part of the model.
