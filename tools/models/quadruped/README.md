# Quadruped generator

One Blender script makes every four-legged animal of the Asset & Style Studio from a species file: the cow and the dog now (WP7), and later the horse-like animals, sheep, pig, goat, cat, deer and the rest of the farm animals and pets. Everything is our own: the shapes are numbers in `species/<id>.json`; Blender (pinned 5.2.2, headless) and its bundled Rigify horse metarig are the only tools.

```
blender -b --factory-startup -t 2 --python tools/models/quadruped/build.py -- --species cow --out <folder> \
    [--stage surface|mesh|all] [--views side,front,threeq,rear,top] [--faces N] [--voxel M] [--ao] [--ao-samples 64]
cargo xtask convert <folder>/cow_studio.glb --id cow --out apps/material-view/assets/models
cargo xtask convert <folder>/cow.glb --id cow --out tests/fixtures/models/cow
blender -b --factory-startup -t 2 --python tools/blender/rig_check.py -- <folder>/cow.glb tools/blender/rig_check/cow.json
blender -b --factory-startup --python tools/models/quadruped/render.py -- --compare <horse.glb>,<folder>/cow.blend,<dog.blend> --out <folder>
```

`--stage surface` is a quick look at the shape (the dense union only, no quads, no rig; add `--voxel 0.012` for a cow-sized animal to make it fast). `mesh` adds the quad remesh, regions and coat chart and writes `<id>_studio.glb` (the rest pose, unskinned, no clips: what the studio's viewer loads, as it has `anim` off); `all` (the default) adds the rig, the clips and the rigged `<id>.glb` (the test fixture and the games' model). `--ao` bakes occlusion into `_EVANI_AO` (off: the studio's converter does not carry it yet). Each run writes `<id>.blend`, `<id>.report.json` and the asked views as PNGs.

Run Blender at below-normal priority with two threads (`run-low.mjs`, `-t 2`); a cow takes a few minutes.

## Files

| File | What it does |
|---|---|
| `build.py` | The pipeline: parts, voxel union, fillet, smoothing, carving, QuadriFlow, eyes, regions, coat chart, materials, then `rig.py`, then `glbfix.py`. |
| `body.py` | The body from a species file: the head frame, the section keys, the leg rings, ears, horns, tail, feet, udder, dewlap, mane, masses, and the rig's landmarks. |
| `geom.py` | Shape helpers: section profiles, lofts, tubes, ellipsoids, splines, distances. |
| `rig.py` | The Rigify horse metarig fitted to the species' joints, weights, walk and idle clips, baked occlusion, glb export. |
| `glbfix.py` | Rewrites the exported attributes to the studio's contract (`_EVANI_PART` u8, `_EVANI_COAT` unorm8x4); `--dump` lists a glb's attributes. Plain Python. |
| `render.py` | Review pictures: one animal by region, or a sheet of several side by side at true scale. |
| `species/*.json` | One file per species. |

## Frame and units

Metres. `f` forward (+ toward the head), `z` up (ground at 0), `x` to the animal's left; the files give only the right half, the generator mirrors it. In Blender the animal faces -Y (Rigify's metarig), in the glb +Z (glTF).

**The head frame**: `head.poll` is the top of the head behind the ears, `head.angle` the face line's angle below horizontal (a cow's 60, a dog's 8). Head numbers are `a` metres down the face line from the poll and `b` metres in from it toward the jaw.

**Section keys** describe a cross-section between two midline points (top and bottom, or front and back) by its half width `w`, where it is widest (`widest`, 0 at the top point .. 1 at the bottom one), and superellipse exponents above and below (`p_top`, `p_bot`: 2 round, 3 and more boxy), optionally `flat` (0..1, a flattened top: a broad forehead, a cow's square rump). Sections are blended with Catmull-Rom between keys, so each key is a measured slice. Instead of the four numbers a key may give an explicit list of half widths at `geom.TT`.

## The species file

| Field | Meaning |
|---|---|
| `id`, `asset` | The model id (file names) and the asset row in `catalog/assets/assets.json`; its `regions` are the ids `_EVANI_PART` indexes. |
| `voxel` | Union voxel size (m): about 1/240 of the withers height (cow 0.006, dog 0.003). Thin parts must be at least 3 voxels thick. |
| `fillet`, `smooth` | SDF fillet iterations at the joins (10 keeps folds), smoothing passes (8). |
| `faces`, `seed` | QuadriFlow's target quads (the master's triangles are about twice this) and its seed. |
| `girth` | Lateral scale on every section (a fatter or leaner animal of the same build). |
| `trunk` | `[f, z_top, z_bottom, w, widest, p_top, p_bot, flat]` from the rump to the breast. |
| `neck.keys` | `[[f, z] crest, [f, z] throat, w, widest, ...]` from the withers to behind the poll. |
| `neck.dewlap` | Optional fold under the neck: `[f, z_top, z_bottom, w]`. |
| `head.poll`, `angle`, `length`, `atlas_b` | The head frame (above); `atlas_b` where the neck's last bone ends below the poll. |
| `head.keys` | `[a, off, D, w, widest, p_top, p_bot, flat]`: the slice `a` down the face, its top `off` in from the face line, its depth `D` (to the jaw's lower edge). |
| `head.masses` | `[a, b, x, [along, in, lateral], mirror?]` ellipsoids in the head frame: cheeks, brows, the nose pad, a poll ridge. |
| `carve` | `nostril {a, b, x, ra, rb, rx, depth}` (an opening with a raised rim), `mouth {corner [a, b], front [a, b], depth, width}` (the lip line). |
| `eyes` | `{a, b, x, r, long, tall}`: eyeballs as separate closed parts (the `eye` region, `keep`: their own colour). |
| `ears` | `{a, b, x}` base, `dir` `[out, forward, up]` along the ear, `open` the way the cup opens, `length`, `width` (half), `thick` (wall), `widest` (0.35 a leaf ear, 0.06 a triangle), `tip` (taper), `cup`, `sink` (into the head), optional `droop` (m) and `droop_dir` for hanging ears, `bow`. |
| `horns` | Optional: `{a, b, x}` base, `line` `[[out, forward, up] ...]` offsets of the centre line from the base, `r` radii along it. |
| `legs.fore/hind.joints` | Joint centres `[f, z]`: fore `scapula, shoulder, elbow, wrist, fetlock, coronet, toe`; hind `hip, stifle, hock, fetlock, coronet, toe`. A dog's `fetlock` is the knuckle where the toes start (digitigrade); the rig's bones run joint to joint. |
| `legs.*.skin` | The leg's skin, top to bottom: `[f, z, half depth (front-back), half width, x (centre), squareness]` on the centre line; rings are cut square to it. |
| `feet` | `type` `hoof` (one), `cloven` (two claws, `cleft` the gap) or `paw` (a pad and four toes, `dewclaw` on the inside); per leg `f`, `x`, `width`, `length` (halves), `toe`, `heel` (hoof wall heights), `slant`, `flare`, `height` (paws), `dew_claws` `[[f, z, out, radii]]`. |
| `tail.line` | `[f, z, r]` centre line and radius from the root to the tip (a cow's switch is a thicker stretch near the end). |
| `udder` | Optional: centre `f`, `z`, radii `r`, `attach` ellipsoids into the belly and between the thighs, `teats` `[[df, x]]`, `teat_len`, `teat_r`. |
| `mane` | Optional (horse-like): `keys` `[f, z crest, height, w]`, a solid proxy shell along the crest (the `mane` region), for the voxel and low-poly styles; hair strands come from the fur look. |
| `muscles` | `[f, z, x, [long, across, lateral], [axis f, z], mirror?, tilt]` masses: shoulder, hooks and pins, thighs. |
| `spine` | Seven `[f, z]` points from the tail root to the withers: the six spine bones. |
| `rig` | `abdomen`, `chest` (vertical bones `[[f, z], [f, z]]`), `hip_tail`, `jaw` (three head-frame points), `breast` `[[f, z], [f, z], x]`, optional `breast_tie` (0..1, see Skin weights). |
| `regions` | Rules, first match wins, mapping each vertex to a region id of the asset row: `eyes`, `hooves` (inside a hoof wall), `horns` (`from` m along the horn), `ears` (`from` the fraction of the ear's length, `pad`), `tail` (`from` m along, `pad`), `head_box` (`a`, `b` ranges, `x` half width), `udder` (keeps the udder in `coat`), `legs` (`fore_top`, `hind_top`: below these heights the leg is `leg`), `rest`. Every region of the row must get vertices (the build fails otherwise). |
| `materials` | The glb's materials: name, the regions it covers, `color`, `roughness` (one primitive each; the studio draws its own per region). |
| `render_colors`, `coat` | Review picture colours per region; the coat chart's `patch_seed`, `patch_count`, `patch_size` `[min, max]` (m) and `face_width`. |
| `walk` | `stride` (m), `frames` (30 fps), `duty` (share of the cycle a foot is down), `lift` (m), `bob`, and optional `breakover`, `swing_fold`, `swing_flex`, `fore_fold`, `hind_fold` (deg), `cannon_share`, `scapula_share`, `nod`, `tail_swing`. |
| `idle` | `frames`, optional `ear_flick` (deg), `tail_wag` (deg), `tail_rate` (swings per swish). |

## What the glb carries

- **`_EVANI_PART`** (u8 per vertex in the master; in the converted tiers unorm8 x4 with the index in the first channel, see `tools/evani-convert/src/carry.rs`): the index of the vertex's region in the asset row's `regions` (the cow: coat, leg, ear, tail, horn, hoof, muzzle, eye). Each region takes one material in the studio; `thin` regions (legs, ears, tail, horns, mane) are solid, closed parts at least three voxels thick, so the voxel and low-poly geometry stages keep them.
- **`_EVANI_COAT`** (unorm8 x4 per vertex), the coat chart for per-body-part coats and per-region voxel colour: R **face** (1 down the middle of the face, 0 on the cheeks: blazes, white faces), G **belly** (the underside), B **lower legs** (1 at the hoof or paw, 0 above the knee or hock: socks, stockings, a bay's points), A **patches** (a smooth field of soft blobs, deterministic from `coat.patch_seed`; a material thresholds it for piebald or spotted coats). The fur look (hair-v2, a later package) reads these and the regions; there is no fur on these meshes.
- With `--ao`, **`_EVANI_AO`** (baked occlusion, rest pose; dropped by the converter until the hair-v2 attribute carry lands). Skin weights (four per vertex) on the Rigify horse metarig's deform bones plus the half-turn helpers, and the clips `walk` and `idle`.

## Skin weights (rig.py)

In order, all from the species' own joints, so a new species needs no weight painting:

1. **Heat on the body skin alone.** Bone heat runs on a copy without the separate solids (eyes, ears, horns, hooves cut into the skin and make heat fail there); any vertex heat leaves empty takes its weighted neighbours' average, front by front (`QUAD_FILL`). Ear bones never hold the body skin.
2. **Sides and midline.** A left limb never holds the right side; limb bones fade out toward the midline of belly, chest and groin (`midline`).
3. **Even leg blend** (`limb_share`, as the test horse's): round each armpit and groin the leg's share of the skin is solved as a harmonic blend between anchors: the leg below the elbow or stifle inside its own skin outline (`legs.*.skin`) and the outside of the upper leg are the leg's, a midline strip is the trunk's. `rig.breast_tie` (0..1, default 0; the dog 0.8) hands that much of the chest floor's trunk share round the elbow to the breast bones, which follow the arms (`walk.breast_follow`).
4. **Half-turn helpers** at hips, shoulders, stifles and elbows (`joint_blend`).
5. **Separate solids** are rigid: hooves on their hoof bone, eyes and horns on the head; an ear's buried base on the head, fading to its two ear bones outside the skin.
6. **Stretch-driven relaxation** (`relax_stretch`, `QUAD_RELAX`): the walk and idle are played, edges stretched past 1.5x or squeezed under 0.5x are found, and the weights round them are evened with their neighbours, 14 rounds; the best round is kept.

The build also collapses sliver edges and folded quads shorter than 0.4 voxel after the remesh (`QUAD_SLIVERS`).

**Rig check settings** come from `rigcheck_regions.py`: the test horse's regions (under the tail, elbow and armpit, flank and groin, back of the thigh, everything else) placed by the species' joints, with the horse's limits. With `--measured <report>` a region where the species misses the horse's limit is held at its own measure plus 5 percent and listed under `over_horse`: open work, never the goal.

```
blender -b <folder>/cow.blend --factory-startup --python tools/models/quadruped/rigcheck_regions.py -- --out tools/blender/rig_check/cow.json [--measured <rig_check --out report>]
```

## Adding a species

Copy the closest species file (a sheep or goat from the cow, a cat or fox from the dog), change `id` and `asset`, and add the asset row (its `regions`) to `catalog/assets/assets.json` first: the build reads them. Then change the numbers by eye with `--stage surface --voxel <coarse> --views side,front,threeq` (a minute or two), build `all`, convert, write `tools/blender/rig_check/<id>.json` from the measured walk and idle (the goal is 1.6x / 0.45x everywhere) and run the rig check. Keep the tier sizes under the horse's.
